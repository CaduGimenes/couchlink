//! One-time setup for virtual screens: the Virtual Display Driver (VirtualDrivers/Virtual-Display-Driver, MIT) and
//! write access to its settings folder for everyone, so sessions never need administrator rights.
//!
//! Like `accounts.rs`, the daemon starts itself elevated through the normal Windows prompt as
//! `couchlinkd --virtual-screens setup`; that process runs the steps the driver's own silent install uses (pinned
//! downloads checked against their SHA-256, the driver's signing certificates as trusted publishers, the device made
//! with nefcon, MIT) and leaves any error in `<state>/virtual-screens-setup.txt`.

use couchlink_core::paths;
use serde_json::{json, Value};

const DRIVER_URL: &str = "https://github.com/VirtualDrivers/Virtual-Display-Driver/releases/download/25.7.23/VirtualDisplayDriver-x86.Driver.Only.zip";
const DRIVER_SHA256: &str = "E24210692B442B39AF763536330CE78B423F19342B7A7792C26DE3944E418B3A";
const NEFCON_URL: &str = "https://github.com/nefarius/nefcon/releases/download/v1.14.0/nefcon_v1.14.0.zip";
const NEFCON_SHA256: &str = "A15557DA24A9EFCA203158DE3B43B0EAF982DB231F0194031F1ED428BC13E669";

fn outcome_file() -> std::path::PathBuf {
    paths::state_dir().join("virtual-screens-setup.txt")
}

/// `virtual_screens`: whether the driver answers and its settings can be changed without admin.
pub fn status() -> Value {
    let ready = crate::session::vscreens::driver_present();
    let writable = std::fs::OpenOptions::new().append(true).open(crate::session::vscreens::settings_path()).is_ok();
    json!({ "ready": ready && writable, "driver": ready, "windows": cfg!(windows) })
}

/// Runs the elevated setup and waits for it; then takes the driver's first monitor off the desktop.
pub fn run() -> Result<(), String> {
    if !cfg!(windows) {
        return Err("virtual screens are only available on Windows".to_string());
    }
    let _ = std::fs::remove_file(outcome_file());
    let code = crate::accounts::elevate(&["--virtual-screens".to_string(), "setup".to_string()])?;
    let note = std::fs::read_to_string(outcome_file()).unwrap_or_default();
    let _ = std::fs::remove_file(outcome_file());
    match (code, note.trim()) {
        (0, _) => {}
        (_, "") => return Err(format!("the display driver setup ended with code {code}")),
        (_, msg) => return Err(msg.to_string()),
    }
    // The driver starts with one monitor of its own on the desktop; Couchlink only uses them during a session.
    for _ in 0..40 {
        if crate::session::vscreens::driver_present() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    crate::session::vscreens::detach_now();
    if status()["ready"] == true {
        Ok(())
    } else {
        Err("the display driver was installed but does not answer yet; restart the PC and try again".to_string())
    }
}

/// The steps, as PowerShell (Windows' own tools: downloads, hashes, zips, certificates, folder rights).
fn script() -> String {
    format!(
        r#"$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') {{ throw 'Virtual screens need an x64 PC for now.' }}
$dir = Join-Path $env:TEMP 'couchlink-vdd'
Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $dir | Out-Null
function Fetch($url, $file, $sha) {{
  $p = Join-Path $dir $file
  Invoke-WebRequest $url -OutFile $p -UseBasicParsing
  if ((Get-FileHash $p -Algorithm SHA256).Hash -ne $sha) {{ throw "$file is not the expected download" }}
  Expand-Archive $p -DestinationPath $dir -Force
}}
$conf = 'C:\VirtualDisplayDriver'
New-Item -ItemType Directory -Force $conf | Out-Null
$have = Get-PnpDevice -Class Display -ErrorAction SilentlyContinue | Where-Object {{ $_.HardwareID -contains 'Root\MttVDD' -or $_.FriendlyName -eq 'Virtual Display Driver' }}
if (-not $have) {{
  Fetch '{DRIVER_URL}' 'vdd.zip' '{DRIVER_SHA256}'
  Fetch '{NEFCON_URL}' 'nefcon.zip' '{NEFCON_SHA256}'
  $certs = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2Collection
  $certs.Import((Join-Path $dir 'VirtualDisplayDriver\mttvdd.cat'))
  $store = New-Object System.Security.Cryptography.X509Certificates.X509Store('TrustedPublisher', 'LocalMachine')
  $store.Open('ReadWrite')
  foreach ($c in $certs) {{ $store.Add($c) }}
  $store.Close()
  if (-not (Test-Path "$conf\vdd_settings.xml")) {{ Copy-Item (Join-Path $dir 'VirtualDisplayDriver\vdd_settings.xml') "$conf\vdd_settings.xml" }}
  Push-Location $dir
  & (Join-Path $dir 'x64\nefconw.exe') install .\VirtualDisplayDriver\MttVDD.inf 'Root\MttVDD'
  Pop-Location
  Start-Sleep -Seconds 5
}}
& icacls $conf /grant '*S-1-5-32-545:(OI)(CI)M' /T | Out-Null
Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue
"#
    )
}

/// `couchlinkd --virtual-screens setup`: the elevated half. Returns the exit code, or None when the command line is
/// not this.
pub fn cli(args: &[String]) -> Option<i32> {
    if args.get(1).map(String::as_str) != Some("--virtual-screens") {
        return None;
    }
    if args.get(2).map(String::as_str) != Some("setup") {
        return Some(1);
    }
    let result = (|| -> Result<(), String> {
        let path = std::env::temp_dir().join("couchlink-vdd-setup.ps1");
        std::fs::write(&path, script()).map_err(|e| e.to_string())?;
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&path)
            .output()
            .map_err(|e| format!("cannot run PowerShell: {e}"))?;
        let _ = std::fs::remove_file(&path);
        if out.status.success() {
            Ok(())
        } else {
            let err = String::from_utf8_lossy(&out.stderr);
            let first = err.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("the setup script failed");
            Err(format!("display driver setup: {first}"))
        }
    })();
    if let Err(e) = &result {
        let _ = std::fs::write(outcome_file(), e);
    }
    Some(if result.is_ok() { 0 } else { 1 })
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_script_pins_both_downloads_and_never_names_a_user() {
        let s = super::script();
        assert!(s.contains(super::DRIVER_SHA256) && s.contains(super::NEFCON_SHA256));
        assert!(s.contains("S-1-5-32-545"), "write access goes to the built-in Users group");
        assert!(super::cli(&["couchlinkd".into(), "--virtual-screens".into(), "explode".into()]) == Some(1));
        assert!(super::cli(&["couchlinkd".into(), "--other".into()]).is_none());
    }
}
