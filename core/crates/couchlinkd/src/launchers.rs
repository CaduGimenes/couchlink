//! Starting a game through Couchlink from somewhere else: a launcher (Playnite, LaunchBox), a desktop shortcut or
//! Steam as a non-Steam game. All of them run `couchlink.exe --play <recipe>`; the app hands that to
//! `couchlinkd --play`, which asks the running daemon (starting it when needed) for the session (`play`).

use std::path::{Path, PathBuf};

/// The app next to this daemon: what shortcuts and launchers start.
pub fn app_exe() -> Result<PathBuf, String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let name = if cfg!(windows) { "couchlink.exe" } else { "couchlink" };
    let app = me.parent().map(|d| d.join(name)).filter(|p| p.is_file());
    app.ok_or_else(|| format!("{name} is not next to couchlinkd ({}); shortcuts need the installed app", me.display()))
}

/// The arguments that start `recipe` through Couchlink.
pub fn play_args(recipe: &str) -> String {
    format!("--play {}", quote(recipe))
}

/// One command line argument, quoted when it holds a space or a quote.
fn quote(arg: &str) -> String {
    if arg.is_empty() || arg.contains([' ', '\t', '"']) {
        format!("\"{}\"", arg.replace('"', "\\\""))
    } else {
        arg.to_string()
    }
}

/// A file name for the game's shortcut: the characters Windows does not allow in names are left out.
pub fn shortcut_name(game: &str) -> String {
    let clean: String = game.chars().filter(|c| !matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') && !c.is_control()).collect();
    format!("{} (Couchlink)", clean.trim().trim_end_matches('.'))
}

/// Writes `<dir>/<game> (Couchlink).lnk` that starts the game through Couchlink, with the game's own program as its
/// icon when there is one. Returns the shortcut's path.
#[cfg(windows)]
pub fn create_shortcut(dir: &Path, game: &str, recipe: &str, icon: Option<&Path>) -> Result<PathBuf, String> {
    let app = app_exe()?;
    let path = dir.join(format!("{}.lnk", shortcut_name(game)));
    // WScript.Shell writes .lnk files; the values travel in variables, so no quoting can go wrong.
    let script = "$s = (New-Object -ComObject WScript.Shell).CreateShortcut($env:CL_LNK); $s.TargetPath = $env:CL_TARGET; \
                  $s.Arguments = $env:CL_ARGS; $s.WorkingDirectory = $env:CL_DIR; $s.IconLocation = $env:CL_ICON; \
                  $s.Description = $env:CL_DESC; $s.Save()";
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("CL_LNK", &path)
        .env("CL_TARGET", &app)
        .env("CL_ARGS", play_args(recipe))
        .env("CL_DIR", app.parent().unwrap_or(Path::new(".")))
        .env("CL_ICON", format!("{},0", icon.unwrap_or(&app).display()))
        .env("CL_DESC", format!("Play {game} with Couchlink"))
        .creation_flags_hidden()
        .output()
        .map_err(|e| format!("cannot run PowerShell: {e}"))?;
    if !out.status.success() || !path.is_file() {
        return Err(format!("the shortcut could not be written: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(path)
}

/// The Steam game a link starts: `steam://rungameid/<id>` (a Start menu or desktop `.url`), or Steam itself started
/// with `-applaunch <id>`.
pub fn steam_app_in(text: &str) -> Option<u32> {
    let lower = text.to_ascii_lowercase();
    for key in ["steam://rungameid/", "-applaunch "] {
        if let Some(at) = lower.find(key) {
            let digits: String = lower[at + key.len()..].trim_start().chars().take_while(char::is_ascii_digit).collect();
            if let Ok(id) = digits.parse() {
                return Some(id);
            }
        }
    }
    None
}

/// What a program a launcher hands over really is. LaunchBox and others often name a shortcut: a `.lnk` leads to its
/// program, and a link to a Steam game (a `.url` to `steam://rungameid/<id>`, or a `.lnk` to Steam with `-applaunch`)
/// to that game's folder. Anything else, or a shortcut that cannot be read, stays as it is.
pub fn resolve_launch_target(given: &Path) -> PathBuf {
    let ext = given.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let steam_dir = |text: &str| steam_app_in(text).and_then(couchlink_steam::find_app).map(|a| a.install_dir);
    match ext.as_str() {
        "url" => {
            let text = std::fs::read_to_string(given).unwrap_or_default();
            let url = text.lines().find_map(|l| l.trim().strip_prefix("URL=")).unwrap_or_default().to_string();
            steam_dir(&url).unwrap_or_else(|| given.to_path_buf())
        }
        "lnk" => match lnk_target(given) {
            Some((target, args)) => steam_dir(&format!("{} {args}", target.display())).unwrap_or(target),
            None => given.to_path_buf(),
        },
        _ => given.to_path_buf(),
    }
}

/// A `.lnk`'s program and its arguments, read through WScript.Shell. None when it names no program (a Store app's).
#[cfg(windows)]
fn lnk_target(lnk: &Path) -> Option<(PathBuf, String)> {
    let script = "$s = (New-Object -ComObject WScript.Shell).CreateShortcut($env:CL_LNK); $s.TargetPath; $s.Arguments";
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("CL_LNK", lnk)
        .creation_flags_hidden()
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let mut lines = text.lines();
    let target = lines.next().map(str::trim).filter(|t| !t.is_empty())?;
    Some((PathBuf::from(target), lines.next().unwrap_or_default().trim().to_string()))
}

#[cfg(not(windows))]
fn lnk_target(_: &Path) -> Option<(PathBuf, String)> {
    None
}

/// The desktop folder of the person at the PC.
#[cfg(windows)]
pub fn desktop() -> Result<PathBuf, String> {
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", "[Environment]::GetFolderPath('Desktop')"])
        .creation_flags_hidden()
        .output()
        .map_err(|e| format!("cannot run PowerShell: {e}"))?;
    let dir = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    if dir.is_dir() { Ok(dir) } else { Err("the desktop folder was not found".to_string()) }
}

#[cfg(windows)]
trait Hidden {
    fn creation_flags_hidden(&mut self) -> &mut Self;
}

#[cfg(windows)]
impl Hidden for std::process::Command {
    fn creation_flags_hidden(&mut self) -> &mut Self {
        use std::os::windows::process::CommandExt;
        self.creation_flags(0x0800_0000) // CREATE_NO_WINDOW
    }
}

// ---- Steam's shortcuts.vdf ------------------------------------------------------------------------------------------

/// One value in Steam's binary key-value files.
#[derive(Debug, Clone, PartialEq)]
pub enum Kv {
    Map(Vec<(String, Kv)>),
    Str(String),
    Int(u32),
}

fn read_cstr(b: &[u8], at: &mut usize) -> Result<String, String> {
    let end = b[*at..].iter().position(|&c| c == 0).ok_or("shortcuts.vdf: a string has no end")?;
    let s = String::from_utf8_lossy(&b[*at..*at + end]).to_string();
    *at += end + 1;
    Ok(s)
}

fn read_map(b: &[u8], at: &mut usize) -> Result<Vec<(String, Kv)>, String> {
    let mut out = Vec::new();
    loop {
        let kind = *b.get(*at).ok_or("shortcuts.vdf ends early")?;
        *at += 1;
        if kind == 0x08 {
            return Ok(out);
        }
        let key = read_cstr(b, at)?;
        let value = match kind {
            0x00 => Kv::Map(read_map(b, at)?),
            0x01 => Kv::Str(read_cstr(b, at)?),
            0x02 => {
                let v = b.get(*at..*at + 4).ok_or("shortcuts.vdf ends early")?;
                *at += 4;
                Kv::Int(u32::from_le_bytes([v[0], v[1], v[2], v[3]]))
            }
            k => return Err(format!("shortcuts.vdf: unknown value type {k}")),
        };
        out.push((key, value));
    }
}

/// Parses `shortcuts.vdf` (binary key-values); an empty file is an empty list.
pub fn parse_kv(b: &[u8]) -> Result<Vec<(String, Kv)>, String> {
    if b.is_empty() {
        return Ok(vec![("shortcuts".to_string(), Kv::Map(Vec::new()))]);
    }
    let mut at = 0;
    read_map(b, &mut at)
}

fn write_map(out: &mut Vec<u8>, map: &[(String, Kv)]) {
    for (k, v) in map {
        match v {
            Kv::Map(m) => {
                out.push(0x00);
                out.extend(k.as_bytes());
                out.push(0);
                write_map(out, m);
            }
            Kv::Str(s) => {
                out.push(0x01);
                out.extend(k.as_bytes());
                out.push(0);
                out.extend(s.as_bytes());
                out.push(0);
            }
            Kv::Int(i) => {
                out.push(0x02);
                out.extend(k.as_bytes());
                out.push(0);
                out.extend(i.to_le_bytes());
            }
        }
    }
    out.push(0x08);
}

/// Writes binary key-values as Steam does.
pub fn write_kv(map: &[(String, Kv)]) -> Vec<u8> {
    let mut out = Vec::new();
    write_map(&mut out, map);
    out
}

/// The id Steam gives a non-Steam game: the CRC-32 of its quoted program and name, with the top bit set.
pub fn shortcut_app_id(exe: &str, name: &str) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for b in exe.bytes().chain(name.bytes()) {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc | 0x8000_0000
}

/// Adds (or updates, by its name) a non-Steam game entry that starts `recipe` through `app`.
pub fn add_shortcut(file: &mut Vec<(String, Kv)>, name: &str, app: &Path, recipe: &str, icon: &str) -> Result<(), String> {
    let exe = format!("\"{}\"", app.display());
    let dir = format!("\"{}\"", app.parent().unwrap_or(Path::new(".")).display());
    let entry = vec![
        ("appid".to_string(), Kv::Int(shortcut_app_id(&exe, name))),
        ("AppName".to_string(), Kv::Str(name.to_string())),
        ("Exe".to_string(), Kv::Str(exe)),
        ("StartDir".to_string(), Kv::Str(dir)),
        ("icon".to_string(), Kv::Str(icon.to_string())),
        ("ShortcutPath".to_string(), Kv::Str(String::new())),
        ("LaunchOptions".to_string(), Kv::Str(play_args(recipe))),
        ("IsHidden".to_string(), Kv::Int(0)),
        ("AllowDesktopConfig".to_string(), Kv::Int(1)),
        ("AllowOverlay".to_string(), Kv::Int(1)),
        ("OpenVR".to_string(), Kv::Int(0)),
        ("Devkit".to_string(), Kv::Int(0)),
        ("DevkitGameID".to_string(), Kv::Str(String::new())),
        ("DevkitOverrideAppID".to_string(), Kv::Int(0)),
        ("LastPlayTime".to_string(), Kv::Int(0)),
        ("FlatpakAppID".to_string(), Kv::Str(String::new())),
        ("tags".to_string(), Kv::Map(Vec::new())),
    ];
    let Some((_, Kv::Map(list))) = file.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case("shortcuts")) else {
        return Err("shortcuts.vdf has no shortcuts list".to_string());
    };
    let same = |e: &Kv| matches!(e, Kv::Map(m) if m.iter().any(|(k, v)| k.eq_ignore_ascii_case("AppName") && *v == Kv::Str(name.to_string())));
    if let Some((_, e)) = list.iter_mut().find(|(_, e)| same(e)) {
        *e = Kv::Map(entry);
        return Ok(());
    }
    let next = list.iter().filter_map(|(k, _)| k.parse::<usize>().ok()).max().map_or(0, |n| n + 1);
    list.push((next.to_string(), Kv::Map(entry)));
    Ok(())
}

/// `<Steam>/userdata/<account>/config/shortcuts.vdf` of the account signed in to the PC owner's Steam, else of the one
/// account there is.
#[cfg(windows)]
fn shortcuts_file() -> Result<PathBuf, String> {
    let root = couchlink_steam::steam_root().ok_or("Steam is not installed")?;
    let users: Vec<PathBuf> = std::fs::read_dir(root.join("userdata"))
        .map_err(|_| "Steam has no signed in account on this PC yet: open Steam and sign in once".to_string())?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("config").is_dir() && p.file_name().is_some_and(|n| n.to_string_lossy().parse::<u32>().is_ok_and(|n| n != 0)))
        .collect();
    let active = couchlink_steam::owner::active_account().map(|a| a.to_string());
    let user = match active.and_then(|a| users.iter().find(|u| u.file_name().is_some_and(|n| n.to_string_lossy() == a))) {
        Some(u) => u.clone(),
        None if users.len() == 1 => users[0].clone(),
        None => return Err("several Steam accounts use this PC: open Steam and sign in to yours, then try again".to_string()),
    };
    Ok(user.join("config").join("shortcuts.vdf"))
}

/// Adds the game to the PC owner's Steam as a non-Steam game. Steam keeps its shortcuts in memory and writes them back
/// when it closes, so it is closed first and opened again afterwards. Blocks while Steam closes.
#[cfg(windows)]
pub fn add_to_steam(game: &str, recipe: &str, icon: Option<&Path>) -> Result<(), String> {
    let app = app_exe()?;
    let file = shortcuts_file()?;
    let root = couchlink_steam::steam_root().ok_or("Steam is not installed")?;
    let was_running = couchlink_steam::owner::owner_steam_running();
    if was_running {
        std::process::Command::new(root.join("steam.exe")).arg("-shutdown").spawn().map_err(|e| format!("cannot close Steam: {e}"))?;
        let start = std::time::Instant::now();
        while couchlink_steam::owner::owner_steam_running() {
            if start.elapsed() > std::time::Duration::from_secs(60) {
                return Err("Steam did not close; close it and try again".to_string());
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        // It writes its files on the way out.
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
    let bytes = std::fs::read(&file).unwrap_or_default();
    let mut kv = parse_kv(&bytes)?;
    let icon = icon.unwrap_or(&app).display().to_string();
    add_shortcut(&mut kv, &format!("{game} (Couchlink)"), &app, recipe, &icon)?;
    if !bytes.is_empty() {
        let _ = std::fs::write(file.with_extension("vdf.couchlink-backup"), &bytes);
    }
    std::fs::write(&file, write_kv(&kv)).map_err(|e| format!("cannot write {}: {e}", file.display()))?;
    std::process::Command::new(root.join("steam.exe")).spawn().map_err(|e| format!("added, but Steam could not be opened again: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod launch_target_tests {
    use super::*;

    #[test]
    fn steam_links_name_their_game() {
        assert_eq!(steam_app_in("steam://rungameid/1790600"), Some(1790600));
        assert_eq!(steam_app_in(r"C:\Program Files (x86)\Steam\steam.exe -applaunch 1790600 -silent"), Some(1790600));
        assert_eq!(steam_app_in(r"C:\Games\Sparking.exe"), None);
        let url = std::env::temp_dir().join("couchlink-not-steam.url");
        std::fs::write(&url, "[InternetShortcut]
URL=https://example.com/
").unwrap();
        assert_eq!(resolve_launch_target(&url), url, "a link to something else stays as it is");
        let exe = std::env::temp_dir().join("couchlink-plain.exe");
        assert_eq!(resolve_launch_target(&exe), exe);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_arguments_and_names_are_safe() {
        assert_eq!(play_args("sworn"), "--play sworn");
        assert_eq!(play_args("own game"), "--play \"own game\"");
        assert_eq!(shortcut_name("Sworn: Deluxe?"), "Sworn Deluxe (Couchlink)");
        assert_eq!(shortcut_name("A/B..."), "AB (Couchlink)");
    }

    #[test]
    fn a_non_steam_game_is_added_once_and_steams_file_keeps_the_rest() {
        let mut kv = parse_kv(&[]).unwrap();
        let app = Path::new(r"C:\Couchlink\couchlink.exe");
        add_shortcut(&mut kv, "Sworn (Couchlink)", app, "sworn", "icon.exe").unwrap();
        let bytes = write_kv(&kv);
        let mut again = parse_kv(&bytes).unwrap();
        assert_eq!(again, kv, "what is written reads back the same");
        add_shortcut(&mut again, "Sworn (Couchlink)", app, "sworn", "icon.exe").unwrap();
        add_shortcut(&mut again, "Other (Couchlink)", app, "other", "icon.exe").unwrap();
        let Kv::Map(list) = &again[0].1 else { panic!("no list") };
        assert_eq!(list.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["0", "1"], "the same game is updated, a new one appended");
        let Kv::Map(entry) = &list[0].1 else { panic!("no entry") };
        let get = |key: &str| entry.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
        assert_eq!(get("LaunchOptions"), Some(Kv::Str("--play sworn".into())));
        assert_eq!(get("Exe"), Some(Kv::Str("\"C:\\Couchlink\\couchlink.exe\"".into())));
        let Some(Kv::Int(id)) = get("appid") else { panic!("no appid") };
        assert!(id & 0x8000_0000 != 0, "a non-Steam game's id has the top bit set");
    }

    /// Reads every real `shortcuts.vdf` on this PC and writes it again in memory: the bytes must come back the same.
    /// Nothing is written to disk.
    #[test]
    #[ignore = "reads the Steam install of this PC"]
    fn steams_own_shortcuts_files_read_back_byte_for_byte() {
        let root = couchlink_steam::steam_root().expect("Steam installed");
        let mut seen = 0;
        for user in std::fs::read_dir(root.join("userdata")).unwrap().flatten() {
            let Ok(bytes) = std::fs::read(user.path().join("config").join("shortcuts.vdf")) else { continue };
            let kv = parse_kv(&bytes).unwrap();
            assert_eq!(write_kv(&kv), bytes);
            seen += 1;
        }
        assert!(seen > 0, "no shortcuts.vdf on this PC");
    }

    #[test]
    fn the_shortcut_id_is_steams_crc() {
        // CRC-32 of "123456789" is CBF43926.
        assert_eq!(shortcut_app_id("1234", "56789"), 0xCBF4_3926 | 0x8000_0000);
    }
}
