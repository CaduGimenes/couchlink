//! The Steam of the person at the PC (seat 1's): whether it runs with someone signed in, and starting it for a game
//! that needs Steam.

use std::path::PathBuf;

/// True while the PC owner's Steam runs (it signs in by itself with the account it remembers).
pub fn owner_steam_ready() -> bool {
    #[cfg(windows)]
    {
        owner_steam_running()
    }
    #[cfg(target_os = "linux")]
    {
        // Steam writes its process id to ~/.steam/steam.pid while it runs.
        let Some(home) = std::env::var_os("HOME") else { return false };
        let pid = std::fs::read_to_string(PathBuf::from(home).join(".steam").join("steam.pid")).ok();
        pid.and_then(|p| p.trim().parse::<u32>().ok()).is_some_and(|p| std::path::Path::new(&format!("/proc/{p}")).exists())
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        false
    }
}

/// Start the PC owner's Steam in the background (`-silent`: to the tray, no window), as its autostart does. Signs in
/// with the account it remembers.
pub fn start_owner_steam() -> std::io::Result<()> {
    let root = crate::steam_root().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "Steam is not installed"))?;
    let exe: PathBuf = if cfg!(windows) { root.join("steam.exe") } else { PathBuf::from("steam") };
    std::process::Command::new(exe).arg("-silent").spawn().map(|_| ())
}

/// The account signed in to the PC owner's Steam (its `userdata` folder name), when one is.
#[cfg(windows)]
pub fn active_account() -> Option<u32> {
    active_user().filter(|u| *u != 0)
}

/// True while a steam.exe of this Windows user runs, signed in or not. Found by the process list with each process's
/// user, not by Steam's registry values: a second steam.exe that hands over to the running one clears those on its
/// way out. A player's own Steam runs as their helper user and is not counted.
#[cfg(windows)]
pub fn owner_steam_running() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, FALSE, HANDLE};
    use windows_sys::Win32::Security::{EqualSid, GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
    use windows_sys::Win32::System::RemoteDesktop::{WTSEnumerateProcessesW, WTSFreeMemory, WTS_PROCESS_INFOW};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    // SAFETY: the token is closed; the user buffer is as large as the length passed; the process list is read only up
    // to `count` and freed.
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == FALSE {
            return false;
        }
        let mut buf = vec![0u8; 256];
        let mut len = 0u32;
        let ok = GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), buf.len() as u32, &mut len);
        CloseHandle(token);
        if ok == FALSE {
            return false;
        }
        let me = (*(buf.as_ptr() as *const TOKEN_USER)).User.Sid;
        let mut list: *mut WTS_PROCESS_INFOW = std::ptr::null_mut();
        let mut count = 0u32;
        if WTSEnumerateProcessesW(std::ptr::null_mut(), 0, 1, &mut list, &mut count) == FALSE || list.is_null() {
            return false;
        }
        let mut found = false;
        for p in std::slice::from_raw_parts(list, count as usize) {
            if p.pUserSid.is_null() || p.pProcessName.is_null() || EqualSid(p.pUserSid, me) == FALSE {
                continue;
            }
            let mut n = 0;
            while *p.pProcessName.add(n) != 0 {
                n += 1;
            }
            if String::from_utf16_lossy(std::slice::from_raw_parts(p.pProcessName, n)).eq_ignore_ascii_case("steam.exe") {
                found = true;
                break;
            }
        }
        WTSFreeMemory(list.cast());
        found
    }
}

/// `HKCU\Software\Valve\Steam\ActiveProcess` `ActiveUser`: the signed in account, 0 when nobody is.
#[cfg(windows)]
fn active_user() -> Option<u32> {
    active_value("ActiveUser")
}

/// A number under `HKCU\Software\Valve\Steam\ActiveProcess`, which Steam keeps up to date while it runs.
#[cfg(windows)]
fn active_value(name: &str) -> Option<u32> {
    use std::ptr::null_mut;
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    let sub: Vec<u16> = "Software\\Valve\\Steam\\ActiveProcess\0".encode_utf16().collect();
    let val: Vec<u16> = format!("{name}\0").encode_utf16().collect();
    let mut data: u32 = 0;
    let mut size: u32 = 4;
    // SAFETY: both strings are NUL terminated; data is a DWORD and size says so.
    let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, sub.as_ptr(), val.as_ptr(), RRF_RT_REG_DWORD, null_mut(), (&mut data as *mut u32).cast(), &mut size) };
    (r == 0).then_some(data)
}

#[cfg(all(test, windows))]
mod tests {
    /// By hand on a PC whose Steam runs: `cargo test -p couchlink-steam owner_steam_is_seen -- --ignored`.
    #[test]
    #[ignore = "needs Steam running on this PC"]
    fn owner_steam_is_seen() {
        assert!(super::owner_steam_running(), "your Steam runs, so it must be seen");
    }
}
