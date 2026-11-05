//! Own Steam accounts per seat, from the app: listing, one-time setup and removal (couchlink-steam `accounts`).
//!
//! Setting up or removing a seat's helper Windows user needs administrator rights, which the daemon does not have.
//! So the daemon starts itself once more, elevated through the normal Windows prompt, as
//! `couchlinkd --steam-account setup|remove <seat>`; that process does the one step and leaves its outcome in
//! `<state>/steam-account-<seat>.txt`. Everything else (starting that seat's Steam so the player signs in) runs as the
//! person at the PC.

use couchlink_core::paths;
use serde_json::{json, Value};

/// Seats that can have their own account: every seat except the first, which is the PC owner's Steam.
fn check_seat(seat: usize) -> Result<u8, String> {
    if seat == 0 || seat >= couchlink_core::MAX_SEATS {
        return Err(format!("seat {} cannot have its own Steam account (seat 1 uses yours)", seat + 1));
    }
    Ok(seat as u8)
}

fn config(seat: usize) -> Result<couchlink_steam::AccountsConfig, String> {
    let seat = check_seat(seat)?;
    couchlink_steam::seat_config(seat).ok_or_else(|| "Steam is not installed on this PC".to_string())
}

/// `steam_accounts`: every seat that can have one, whether it is set up, whether its Steam runs, and the plan the app
/// shows before setup.
pub fn list() -> Value {
    let rows: Vec<Value> = (1..couchlink_core::MAX_SEATS)
        .filter_map(|seat| {
            let cfg = config(seat).ok()?;
            let ready = couchlink_steam::accounts_ready(&cfg);
            Some(json!({
                "seat": seat,
                "ready": ready,
                "running": ready && couchlink_steam::accounts_steam_running(&cfg),
                "plan": couchlink_steam::accounts_plan(&cfg),
            }))
        })
        .collect();
    json!({ "accounts": rows, "steam": couchlink_steam::steam_root().is_some() })
}

fn outcome_file(seat: u8) -> std::path::PathBuf {
    paths::state_dir().join(format!("steam-account-{seat}.txt"))
}

/// Setup or removal, elevated; blocks until the Windows prompt is answered and the step is done.
pub fn run_elevated_step(step: &str, seat: usize) -> Result<(), String> {
    let s = check_seat(seat)?;
    let _ = std::fs::remove_file(outcome_file(s));
    let code = elevate(&["--steam-account".to_string(), step.to_string(), s.to_string()])?;
    let note = std::fs::read_to_string(outcome_file(s)).unwrap_or_default();
    let _ = std::fs::remove_file(outcome_file(s));
    match (code, note.trim()) {
        (0, _) => Ok(()),
        (_, "") => Err(format!("the {step} step ended with code {code}")),
        (_, msg) => Err(msg.to_string()),
    }
}

/// Starts the seat's own Steam so its player can sign in (or play).
pub fn open(seat: usize) -> Result<(), String> {
    let cfg = config(seat)?;
    if !couchlink_steam::accounts_ready(&cfg) {
        return Err(format!("Player {} has no Steam account set up yet", seat + 1));
    }
    start_steam(&cfg, seat)
}

/// Starts the seat's own Steam, or shows its window when it runs. On Windows a Steam Couchlink starts gets the hook
/// with only its overlay channels renamed ([`couchlink_steam::OVERLAY_CHANNELS`]), so the Shift+Tab overlay works in
/// that seat's games while the Steam of the person at the PC runs too.
pub fn start_steam(cfg: &couchlink_steam::AccountsConfig, seat: usize) -> Result<(), String> {
    let log = seat_steam_log(seat);
    #[cfg(windows)]
    owner_steam_first()?;
    #[cfg(windows)]
    if !couchlink_steam::accounts_steam_running(cfg) {
        crate::logfile::write("info", &format!("starting player {}'s own Steam with its own overlay channels", seat + 1));
        match start_hooked_steam(cfg) {
            Ok(()) => return Ok(()),
            Err(e) => crate::logfile::write("warn", &format!("player {}'s Steam starts without its own overlay channels: {e}", seat + 1)),
        }
    }
    crate::logfile::write("info", &format!("starting player {}'s own Steam (its output: {})", seat + 1, log.display()));
    couchlink_steam::accounts_start_steam_logged(cfg, true, Some(&log)).map(|_| ()).map_err(|e| e.user_message())
}

/// The Steam of the person at the PC (player 1's) has to be up and signed in before a player's own Steam runs: started
/// while one runs it quits at once, and signing in while one runs it crashes. So a player's Steam that runs without it
/// is closed for that moment (it is started again when its player needs it), and this waits for the sign-in. Nothing
/// happens when Steam is not installed or player 1's is signed in already. An error says what the person has to do.
#[cfg(windows)]
pub fn owner_steam_first() -> Result<(), String> {
    use couchlink_steam::owner::{active_account, owner_steam_running, start_owner_steam};
    use std::time::{Duration, Instant};
    let until = |secs: u64, done: &dyn Fn() -> bool| {
        let start = Instant::now();
        while !done() && start.elapsed() < Duration::from_secs(secs) {
            std::thread::sleep(Duration::from_millis(250));
        }
        done()
    };
    if couchlink_steam::steam_root().is_none() || (owner_steam_running() && active_account().is_some()) {
        return Ok(());
    }
    let running: Vec<usize> = (1..couchlink_core::MAX_SEATS)
        .filter(|&s| config(s).ok().is_some_and(|c| couchlink_steam::accounts_ready(&c) && couchlink_steam::accounts_steam_running(&c)))
        .collect();
    for &seat in &running {
        crate::logfile::write("info", &format!("closing player {}'s own Steam for a moment: yours signs in first", seat + 1));
        close(seat)?;
    }
    until(20, &|| !running.iter().any(|&s| config(s).is_ok_and(|c| couchlink_steam::accounts_steam_running(&c))));
    if !owner_steam_running() {
        crate::logfile::write("info", "starting your Steam for player 1");
        start_owner_steam().map_err(|e| format!("cannot start your Steam: {e}"))?;
        if !until(30, &owner_steam_running) {
            return Err("your Steam did not start".to_string());
        }
    }
    if !until(45, &|| active_account().is_some()) {
        return Err("sign in to your Steam first, then start again".to_string());
    }
    // Steam takes a moment after signing in before games can talk to it.
    std::thread::sleep(Duration::from_secs(3));
    Ok(())
}

/// True when the seat's Steam runs with its own overlay channels (a Steam Couchlink started with the hook).
#[cfg(windows)]
pub fn steam_has_own_channels(cfg: &couchlink_steam::AccountsConfig) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ACCESS_DENIED};
    use windows_sys::Win32::System::Threading::{OpenEventW, SYNCHRONIZATION_SYNCHRONIZE};
    let name: Vec<u16> = format!("{}{}", couchlink_steam::OVERLAY_MARKER, cfg.overlay_suffix()).encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: NUL terminated name; a handle we get is closed. The marker belongs to the seat's Windows user, so
    // "access denied" also means it is there.
    unsafe {
        let h = OpenEventW(SYNCHRONIZATION_SYNCHRONIZE, 0, name.as_ptr());
        if !h.is_null() {
            CloseHandle(h);
            return true;
        }
        GetLastError() == ERROR_ACCESS_DENIED
    }
}

#[cfg(windows)]
fn start_hooked_steam(cfg: &couchlink_steam::AccountsConfig) -> Result<(), String> {
    let s = couchlink_steam::accounts_start_steam_suspended(cfg).map_err(|e| e.user_message())?;
    let launched = couchlink_hook::Launched::adopt(s.process, s.thread, s.pid).map_err(|e| e.to_string())?;
    let config = couchlink_hook::HookConfig {
        steam_names: couchlink_steam::OVERLAY_CHANNELS.iter().map(|n| n.to_string()).collect(),
        steam_suffix: Some(cfg.overlay_suffix()),
        // Steam's overlay window (gameoverlayui64.exe) is a program Steam starts for each game; its browser draws the
        // overlay in its GPU process. The browser's page processes are sandboxed and left alone.
        hook_children: true,
        children_only: vec![
            "gameoverlayui64.exe".into(),
            "gameoverlayui.exe".into(),
            // Steam starts itself again after an update.
            "steam.exe".into(),
            // Steam's browser: only its main process, which reports its GPU process to Steam; its other processes are
            // sandboxed and left alone.
            "steamwebhelper.exe|!--type=".into(),
        ],
        // Both Steams share the Steam folder, and the first keeps its log files open: this one writes its own.
        saves: seat_steam_logs(cfg).map(|logs| vec![(cfg.steam_exe.parent().unwrap_or(std::path::Path::new(".")).join("logs"), logs)]).unwrap_or_default(),
        ..Default::default()
    };
    let options = couchlink_hook::HookOptions { xinput: false, test_pads: None };
    let injected = shared_hook_dll(launched.is_x86())
        .and_then(|dll| couchlink_hook::inject_as(&launched, &config, &options, &dll, Some(&cfg.user_name())).map_err(|e| e.to_string()));
    // Without the hook it still runs (the overlay in its games stays off while the other Steam runs).
    couchlink_hook::resume(&launched).map_err(|e| e.to_string())?;
    injected.map(|_| ())
}

/// Where a seat's own Steam writes its logs (`%ProgramData%\Couchlink\steam-logs\seat<n>`), made writable for that seat's
/// Windows user. None when it cannot be made.
#[cfg(windows)]
fn seat_steam_logs(cfg: &couchlink_steam::AccountsConfig) -> Option<std::path::PathBuf> {
    let root = std::env::var_os("ProgramData").map(std::path::PathBuf::from)?;
    let dir = root.join("Couchlink").join("steam-logs").join(format!("seat{}", cfg.seat as u32 + 1));
    std::fs::create_dir_all(&dir).ok()?;
    // The folder is ours, so changing who may write it needs no administrator.
    let grant = format!("{}:(OI)(CI)M", cfg.user_name());
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let ok = std::process::Command::new("icacls")
        .arg(&dir)
        .args(["/grant", &grant])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .is_ok_and(|o| o.status.success());
    ok.then_some(dir)
}

/// A copy of the hook DLL a seat's helper Windows user can read (`%ProgramData%\Couchlink\hook`): the one next to
/// couchlinkd may sit in a folder only the person at the PC can open. Named after its content, so a copy that a
/// running Steam has loaded is never overwritten.
#[cfg(windows)]
fn shared_hook_dll(x86: bool) -> Result<std::path::PathBuf, String> {
    let src = couchlink_hook::dll::dll_path(x86).map_err(|e| e.to_string())?;
    let bytes = std::fs::read(&src).map_err(|e| format!("cannot read {}: {e}", src.display()))?;
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3));
    let root = std::env::var_os("ProgramData").map(std::path::PathBuf::from).ok_or("ProgramData is not set")?;
    let dir = root.join("Couchlink").join("hook");
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
    let dst = dir.join(format!("couchlink_hook_{}_{hash:016x}.dll", if x86 { "x86" } else { "x64" }));
    if std::fs::metadata(&dst).map(|m| m.len()).ok() != Some(bytes.len() as u64) {
        std::fs::write(&dst, &bytes).map_err(|e| format!("cannot write {}: {e}", dst.display()))?;
    }
    Ok(dst)
}

/// Player 1's Steam (the PC owner's): its folder, whether that was picked by hand, and whether it runs signed in.
pub fn owner() -> Value {
    let folder = couchlink_steam::steam_root();
    json!({
        "folder": folder.as_ref().map(|f| f.display().to_string()),
        "picked": load_picked(),
        "ready": couchlink_steam::owner::owner_steam_ready(),
    })
}

/// True when the Steam folder was picked by hand (settings.json `steam_folder`).
fn load_picked() -> bool {
    let file = paths::state_dir().join("settings.json");
    std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).is_some_and(|v| v["steam_folder"].is_string())
}

/// Starts player 1's Steam (the PC owner's) to the tray.
pub fn open_owner() -> Result<(), String> {
    crate::logfile::write("info", "starting your Steam for player 1");
    couchlink_steam::owner::start_owner_steam().map_err(|e| format!("cannot start Steam: {e}"))
}

/// Closes the seat's own Steam, as its Exit menu does.
pub fn close(seat: usize) -> Result<(), String> {
    let cfg = config(seat)?;
    crate::logfile::write("info", &format!("closing player {}'s own Steam", seat + 1));
    couchlink_steam::accounts_stop_steam(&cfg).map_err(|e| e.user_message())
}

/// Closes every seat's own Steam that runs (Settings, "Close players' own Steam when a session ends").
pub fn close_all() {
    for seat in 1..couchlink_core::MAX_SEATS {
        let Ok(cfg) = config(seat) else { continue };
        if couchlink_steam::accounts_ready(&cfg) && couchlink_steam::accounts_steam_running(&cfg) {
            if let Err(e) = close(seat) {
                crate::logfile::write("warn", &format!("could not close player {}'s own Steam: {e}", seat + 1));
            }
        }
    }
}

/// Where a seat's own Steam writes its output (Linux): `<state>/logs/steam-seat-<n>.log`.
pub fn seat_steam_log(seat: usize) -> std::path::PathBuf {
    crate::logfile::dir().join(format!("steam-seat-{}.log", seat + 1))
}

/// `couchlinkd --steam-account setup|remove <seat>`: the elevated half. Returns the exit code, or None when the
/// command line is not this.
pub fn cli(args: &[String]) -> Option<i32> {
    if args.get(1).map(String::as_str) != Some("--steam-account") {
        return None;
    }
    let step = args.get(2).cloned().unwrap_or_default();
    let seat: Option<usize> = args.get(3).and_then(|s| s.parse().ok());
    if step != "setup" && step != "remove" {
        return Some(1);
    }
    let result = (|| -> Result<(), String> {
        let seat = seat.ok_or("missing seat")?;
        let cfg = config(seat)?;
        match step.as_str() {
            "setup" => couchlink_steam::accounts_apply(&cfg, true).map_err(|e| e.user_message()),
            "remove" => couchlink_steam::accounts_remove(&cfg, true).map_err(|e| e.user_message()),
            other => Err(format!("unknown step {other}")),
        }
    })();
    if let (Err(e), Some(seat)) = (&result, seat) {
        if let Ok(s) = check_seat(seat) {
            let _ = std::fs::write(outcome_file(s), e);
        }
    }
    Some(if result.is_ok() { 0 } else { 1 })
}

/// Runs this exe again with `args`, elevated (Windows asks the user), and waits for it.
#[cfg(windows)]
pub fn elevate(args: &[String]) -> Result<i32, String> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let file = wide(&exe.display().to_string());
    let verb = wide("runas");
    let params = wide(&args.join(" "));
    // SAFETY: zeroed SHELLEXECUTEINFOW is valid; the strings live through the call; the process handle is closed.
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = params.as_ptr();
        info.nShow = 0;
        info.hwnd = null_mut();
        info.lpDirectory = null();
        if ShellExecuteExW(&mut info) == 0 {
            let e = std::io::Error::last_os_error();
            return Err(if e.raw_os_error() == Some(1223) { "Windows did not get permission (the prompt was cancelled)".to_string() } else { e.to_string() });
        }
        if info.hProcess.is_null() {
            return Err("the elevated step did not start".to_string());
        }
        WaitForSingleObject(info.hProcess, INFINITE);
        let mut code = 1u32;
        GetExitCodeProcess(info.hProcess, &mut code);
        CloseHandle(info.hProcess);
        Ok(code as i32)
    }
}

/// Elsewhere no elevation is needed (Linux: a seat's Steam is a folder of the user's own): the step runs as a child.
#[cfg(not(windows))]
pub fn elevate(args: &[String]) -> Result<i32, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let status = std::process::Command::new(exe).args(args).status().map_err(|e| e.to_string())?;
    Ok(status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seat_one_keeps_the_owners_steam_and_the_cli_is_recognised() {
        assert!(check_seat(0).is_err());
        assert_eq!(check_seat(1), Ok(1));
        assert!(check_seat(couchlink_core::MAX_SEATS).is_err());
        assert_eq!(cli(&["couchlinkd".into(), "--dev".into()]), None);
        // An unknown step fails before touching anything.
        assert_eq!(cli(&["couchlinkd".into(), "--steam-account".into(), "explode".into(), "1".into()]), Some(1));
        assert_eq!(cli(&["couchlinkd".into(), "--steam-account".into(), "setup".into()]), Some(1), "no seat");
    }
}
