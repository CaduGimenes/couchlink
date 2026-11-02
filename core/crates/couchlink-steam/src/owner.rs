//! The Steam of the person at the PC (seat 1's): whether it runs with someone signed in, and starting it for a game
//! that needs Steam.

use std::path::PathBuf;

/// True while the PC owner's Steam runs with an account signed in.
pub fn owner_steam_ready() -> bool {
    // Steam leaves the last account in the registry when it quits: it must also still run.
    #[cfg(windows)]
    {
        active_user().is_some_and(|u| u != 0) && owner_steam_running()
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

/// True while the PC owner's Steam process runs, signed in or not.
#[cfg(windows)]
pub fn owner_steam_running() -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    let Some(pid) = active_value("pid").filter(|p| *p != 0) else { return false };
    // SAFETY: plain calls; the handle is closed.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return false;
        }
        let mut code = 0u32;
        let alive = GetExitCodeProcess(h, &mut code) != 0 && code == 259; // STILL_ACTIVE
        CloseHandle(h);
        alive
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
