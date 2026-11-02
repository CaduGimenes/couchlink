//! `couchlinkd --play <game>` and `couchlinkd --play-exe <program>`: a launcher's start (see launchers.rs). Asks the
//! running daemon for the session, starting the daemon first when none answers. With `--notify` (the app adds it for a
//! launcher, which has no console) what went wrong also shows in a message box.

use couchlink_core::paths;
use futures_util::{SinkExt, StreamExt};
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::Message;

/// What a `--play` command line asks for.
#[derive(Debug, PartialEq, Eq)]
pub enum Target {
    Game(String),
    Exe(String),
}

/// `--play <game>` or `--play-exe <program>` anywhere on the command line (`args` without the program itself).
pub fn target(args: &[String]) -> Option<Target> {
    let at = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    at("--play").map(Target::Game).or_else(|| at("--play-exe").map(Target::Exe))
}

/// Runs the `--play` client. Returns the exit code, or None when the command line is not a `--play`.
pub fn cli(args: &[String]) -> Option<i32> {
    let target = target(args.get(1..).unwrap_or_default())?;
    Some(match run(&target) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("couchlinkd: {e}");
            crate::logfile::write("warn", &format!("a launcher's start failed: {e}"));
            if args.iter().any(|a| a == "--notify") {
                message_box(&e);
            }
            1
        }
    })
}

fn run(target: &Target) -> Result<(), String> {
    let info = daemon()?;
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
    rt.block_on(async {
        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{}/", info.port)).await.map_err(|e| format!("cannot reach Couchlink: {e}"))?;
        let hello = serde_json::json!({ "id": 1, "cmd": "hello", "token": info.token, "client": format!("couchlink-play {}", env!("CARGO_PKG_VERSION")) });
        ws.send(Message::text(hello.to_string())).await.map_err(|e| e.to_string())?;
        let play = match target {
            Target::Game(g) => serde_json::json!({ "id": 2, "cmd": "play", "recipe": g }),
            Target::Exe(e) => serde_json::json!({ "id": 2, "cmd": "play", "exe": e }),
        };
        ws.send(Message::text(play.to_string())).await.map_err(|e| e.to_string())?;
        let reply = tokio::time::timeout(Duration::from_secs(30), async {
            while let Some(Ok(m)) = ws.next().await {
                let Message::Text(t) = m else { continue };
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) else { continue };
                if v["id"] == 2 {
                    return Some(v);
                }
                if v["id"] == 1 && v["ok"] == false {
                    return Some(v);
                }
            }
            None
        })
        .await
        .ok()
        .flatten()
        .ok_or("Couchlink did not answer")?;
        let _ = ws.close(None).await;
        if reply["ok"] == true { Ok(()) } else { Err(reply["error"].as_str().unwrap_or("the game could not be started").to_string()) }
    })
}

/// The running daemon's address, starting this program as the daemon when nothing answers.
fn daemon() -> Result<paths::DaemonInfo, String> {
    let answers = |i: &paths::DaemonInfo| std::net::TcpStream::connect_timeout(&([127, 0, 0, 1], i.port).into(), Duration::from_millis(500)).is_ok();
    if let Some(info) = paths::read_daemon_info().filter(answers) {
        return Ok(info);
    }
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut cmd = std::process::Command::new(me);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000 | 0x0000_0008); // CREATE_NO_WINDOW | DETACHED_PROCESS
    }
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    cmd.spawn().map_err(|e| format!("cannot start Couchlink: {e}"))?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(20) {
        if let Some(info) = paths::read_daemon_info().filter(answers) {
            // A fresh daemon finds its controllers and screens first.
            std::thread::sleep(Duration::from_secs(2));
            return Ok(info);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err("Couchlink did not start".to_string())
}

#[cfg(windows)]
fn message_box(text: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONWARNING, MB_OK};
    let body: Vec<u16> = format!("{text}\0").encode_utf16().collect();
    let title: Vec<u16> = "Couchlink\0".encode_utf16().collect();
    // SAFETY: both strings are NUL terminated and outlive the call.
    unsafe { MessageBoxW(std::ptr::null_mut(), body.as_ptr(), title.as_ptr(), MB_OK | MB_ICONWARNING) };
}

#[cfg(not(windows))]
fn message_box(_: &str) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_play_flags_are_found_anywhere() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(target(&a(&["--play", "sworn"])), Some(Target::Game("sworn".into())));
        assert_eq!(target(&a(&["--dev", "--play-exe", r"C:\Games\Sworn\SWORN.exe"])), Some(Target::Exe(r"C:\Games\Sworn\SWORN.exe".into())));
        assert_eq!(target(&a(&["--play"])), None, "no game named");
        assert_eq!(target(&a(&["--dev"])), None);
    }
}
