//! Virtual screens: each player on a screen set to "Virtual screens" gets a monitor of their own, sized exactly to
//! their part of the real screen, so every game runs true full screen. The monitors come from the Virtual Display
//! Driver (VirtualDrivers/Virtual-Display-Driver, MIT, signed); `present.rs` shows each one in the player's part.
//!
//! How it works:
//! - the driver reads its monitor count and resolutions from `C:\VirtualDisplayDriver\vdd_settings.xml` and reloads
//!   when told to over the pipe `\\.\pipe\MTTVirtualDisplayPipe` (open to everyone, no admin needed);
//! - [`Session::begin`] journals the settings file as it was, writes one monitor per player with their sizes added to
//!   the resolution list, reloads the driver, waits for the monitors and gives each its size and a place below and to
//!   the right of the real desktop, touching it only at one corner so the mouse rarely wanders onto them;
//! - the journal undoer (also run after a crash) detaches the driver's monitors and puts the settings file back.
//!
//! The driver never goes below one monitor, so outside a session its monitors stay detached from the desktop rather
//! than removed.

use couchlink_core::session::Rect;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

/// Journal kind of the settings file Couchlink changed.
pub const KIND: &str = "virtual-screens";

/// The driver's settings file (its fixed default location).
pub fn settings_path() -> PathBuf {
    PathBuf::from(r"C:\VirtualDisplayDriver\vdd_settings.xml")
}

/// Device names (`\\.\DISPLAY9`) of the monitors a session made: the display list leaves them out.
static OURS: LazyLock<Mutex<Vec<String>>> = LazyLock::new(|| Mutex::new(Vec::new()));

/// True for a monitor a virtual screen session is using right now.
pub fn is_ours(device: &str) -> bool {
    OURS.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|d| d.eq_ignore_ascii_case(device))
}

/// Where the virtual monitors go: in a row starting at the bottom right corner of the real desktop, so they touch it
/// at that one point only.
pub fn layout(desktop: Rect, sizes: &[(i32, i32)]) -> Vec<Rect> {
    let mut x = desktop.x + desktop.w;
    let y = desktop.y + desktop.h;
    sizes
        .iter()
        .map(|&(w, h)| {
            let r = Rect { x, y, w, h };
            x += w;
            r
        })
        .collect()
}

/// The box around all of `rects`.
pub fn bounds(rects: &[Rect]) -> Option<Rect> {
    let first = rects.first()?;
    let (mut l, mut t, mut r, mut b) = (first.x, first.y, first.x + first.w, first.y + first.h);
    for x in rects {
        l = l.min(x.x);
        t = t.min(x.y);
        r = r.max(x.x + x.w);
        b = b.max(x.y + x.h);
    }
    Some(Rect { x: l, y: t, w: r - l, h: b - t })
}

/// Monitor sizes the driver can show: even numbers, at least 640 by 480 (the driver and many games want that).
pub fn monitor_size(rect: Rect) -> (i32, i32) {
    ((rect.w.max(640) + 1) & !1, (rect.h.max(480) + 1) & !1)
}

/// A settings file in the driver's own layout (its release default, comments left out): one monitor.
const TEMPLATE: &str = "<?xml version='1.0' encoding='utf-8'?>
<vdd_settings>
    <monitors>
        <count>1</count>
    </monitors>
    <gpu>
        <friendlyname>default</friendlyname>
    </gpu>
    <global>
        <g_refresh_rate>60</g_refresh_rate>
        <g_refresh_rate>120</g_refresh_rate>
        <g_refresh_rate>144</g_refresh_rate>
    </global>
    <resolutions>
    </resolutions>
    <options>
        <CustomEdid>false</CustomEdid>
        <PreventSpoof>false</PreventSpoof>
        <EdidCeaOverride>false</EdidCeaOverride>
        <HardwareCursor>true</HardwareCursor>
        <SDR10bit>false</SDR10bit>
        <HDRPlus>false</HDRPlus>
        <logging>false</logging>
        <debuglogging>false</debuglogging>
    </options>
</vdd_settings>
";

/// Every `<width>`/`<height>` pair in a settings file, in order.
fn resolutions(xml: &str) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(a) = rest.find("<width>") {
        rest = &rest[a + "<width>".len()..];
        let w = rest.split("</width>").next().and_then(|v| v.trim().parse().ok());
        let h = rest.find("<height>").and_then(|b| rest[b + "<height>".len()..].split("</height>").next()).and_then(|v| v.trim().parse().ok());
        if let (Some(w), Some(h)) = (w, h) {
            out.push((w, h));
        }
    }
    out
}

/// The settings file with `count` monitors and every size in `sizes` among its resolutions. A file in the driver's
/// layout (`<vdd_settings>`) keeps everything else; an older layout (some streaming tools write
/// `<monitors><monitor>`, which this driver does not read) is replaced by the driver's own, keeping its sizes.
pub fn settings_with(xml: &str, count: usize, sizes: &[(i32, i32)]) -> Result<String, String> {
    if !xml.contains("<vdd_settings") {
        let mut all = resolutions(xml);
        all.extend_from_slice(sizes);
        return settings_with(TEMPLATE, count, &all);
    }
    let mut out = replace_first_tag(xml, "count", &count.to_string()).ok_or("the driver's settings file has no monitor count")?;
    let close = "</resolutions>";
    let at = out.find(close).ok_or("the driver's settings file has no resolution list")?;
    let mut add = String::new();
    for &(w, h) in sizes {
        if has_resolution(&out, w, h) || has_resolution(&add, w, h) {
            continue;
        }
        add.push_str(&format!("        <resolution>\n            <width>{w}</width>\n            <height>{h}</height>\n            <refresh_rate>60</refresh_rate>\n        </resolution>\n"));
    }
    // Indent the closing tag as it was.
    let line_start = out[..at].rfind('\n').map_or(0, |i| i + 1);
    out.insert_str(line_start, &add);
    Ok(out)
}

fn replace_first_tag(xml: &str, tag: &str, value: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let a = xml.find(&open)? + open.len();
    let b = a + xml[a..].find(&close)?;
    Some(format!("{}{}{}", &xml[..a], value, &xml[b..]))
}

fn has_resolution(xml: &str, w: i32, h: i32) -> bool {
    let compact: String = xml.chars().filter(|c| !c.is_whitespace()).collect();
    compact.contains(&format!("<width>{w}</width><height>{h}</height>"))
}

/// The monitor count in the settings file.
pub fn settings_count(xml: &str) -> Option<usize> {
    let a = xml.find("<count>")? + "<count>".len();
    let b = a + xml[a..].find("</count>")?;
    xml[a..b].trim().parse().ok()
}

/// One player's virtual monitor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualDisplay {
    /// `\\.\DISPLAY9`
    pub device: String,
    /// Where it sits on the desktop, in pixels.
    pub rect: Rect,
}

/// The virtual monitors of one session. Dropping it does not undo anything: the journal does (see [`undo`]).
pub struct Session {
    pub displays: Vec<VirtualDisplay>,
}

impl Session {
    /// One virtual monitor per entry of `sizes`, placed off the real desktop. Journals the change first.
    pub fn begin(sizes: &[(i32, i32)], journal: &mut couchlink_core::Journal) -> Result<Session, String> {
        let path = settings_path();
        if !platform::driver_present() {
            return Err("virtual screens need the display driver; install it from the screen editor (Show games as, Virtual screens)".to_string());
        }
        // Kept exactly as it was (byte order mark too) for the undo.
        let original = std::fs::read_to_string(&path).map_err(|e| format!("cannot read the display driver's settings ({}): {e}", path.display()))?;
        let new = settings_with(original.trim_start_matches('\u{feff}'), sizes.len(), sizes)?;
        journal
            .record_external(KIND, json!({ "path": path, "xml": original }))
            .map_err(|e| format!("could not write the journal: {e}"))?;
        std::fs::write(&path, &new).map_err(|e| format!("cannot change the display driver's settings ({}): {e}; install the driver again from the screen editor", path.display()))?;
        // Monitors of the driver still on the desktop (another program's) would shift the corner; take them off first.
        platform::detach_all();
        let rects = layout(platform::desktop(), sizes);
        let devices = platform::bring_up(&rects)?;
        let displays: Vec<VirtualDisplay> = devices.into_iter().zip(rects).map(|(device, rect)| VirtualDisplay { device, rect }).collect();
        *OURS.lock().unwrap_or_else(|e| e.into_inner()) = displays.iter().map(|d| d.device.clone()).collect();
        Ok(Session { displays })
    }
}

/// Undo a journalled change: the driver's monitors off the desktop and its settings file as it was.
pub fn undo(data: &Value) -> Result<(), String> {
    OURS.lock().unwrap_or_else(|e| e.into_inner()).clear();
    platform::detach_all();
    let path = data["path"].as_str().map(Path::new).ok_or("no settings path")?;
    let xml = data["xml"].as_str().ok_or("no settings text")?;
    // No reload here: the driver reads the file at its next start, and reloading it on a layout it does not read
    // (some streaming tools write one) can stop the device. Its monitors stay off the desktop until then.
    std::fs::write(path, xml).map_err(|e| format!("cannot restore {}: {e}", path.display()))?;
    Ok(())
}

pub fn register_undoers(undoers: &couchlink_core::Undoers) {
    undoers.register(KIND, undo);
}

/// Takes the driver's monitors off the desktop (after its setup, which leaves one on).
pub fn detach_now() {
    platform::detach_all_after_reload();
}

/// Whether the driver is installed and answering.
pub fn driver_present() -> bool {
    platform::driver_present()
}

#[cfg(windows)]
mod platform {
    use super::Rect;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_PIPE_BUSY, GENERIC_WRITE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Graphics::Gdi::{
        ChangeDisplaySettingsExW, EnumDisplayDevicesW, CDS_NORESET, CDS_UPDATEREGISTRY, DEVMODEW, DISPLAY_DEVICEW, DISP_CHANGE_SUCCESSFUL,
        DM_PELSHEIGHT, DM_PELSWIDTH, DM_POSITION,
    };
    use windows_sys::Win32::Storage::FileSystem::{CreateFileW, WriteFile, OPEN_EXISTING};

    const PIPE: &str = r"\\.\pipe\MTTVirtualDisplayPipe";
    /// DISPLAY_DEVICE_ATTACHED_TO_DESKTOP
    const ATTACHED: u32 = 0x1;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn from_wide(buf: &[u16]) -> String {
        let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..n])
    }

    pub fn driver_present() -> bool {
        let name = wide(PIPE);
        // SAFETY: opening a pipe by its NUL terminated name; the handle is closed at once.
        unsafe {
            let h = CreateFileW(name.as_ptr(), GENERIC_WRITE, 0, std::ptr::null(), OPEN_EXISTING, 0, std::ptr::null_mut());
            if h == INVALID_HANDLE_VALUE {
                return GetLastError() == ERROR_PIPE_BUSY;
            }
            CloseHandle(h);
            true
        }
    }

    /// Sends one command to the driver (UTF-16, as its pipe reads them). Waits a moment while the pipe is busy.
    fn send(cmd: &str) -> bool {
        let name = wide(PIPE);
        let text: Vec<u16> = cmd.encode_utf16().collect();
        let until = Instant::now() + Duration::from_secs(3);
        loop {
            // SAFETY: as in `driver_present`; the buffer outlives WriteFile.
            unsafe {
                let h = CreateFileW(name.as_ptr(), GENERIC_WRITE, 0, std::ptr::null(), OPEN_EXISTING, 0, std::ptr::null_mut());
                if h != INVALID_HANDLE_VALUE {
                    let mut wrote = 0u32;
                    let ok = WriteFile(h, text.as_ptr() as *const u8, (text.len() * 2) as u32, &mut wrote, std::ptr::null_mut()) != 0;
                    CloseHandle(h);
                    return ok;
                }
                if GetLastError() != ERROR_PIPE_BUSY || Instant::now() > until {
                    return false;
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn reload() {
        send("RELOAD_DRIVER");
    }

    /// The whole desktop: every monitor on it.
    pub fn desktop() -> Rect {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN};
        // SAFETY: plain metric queries.
        unsafe { Rect { x: GetSystemMetrics(SM_XVIRTUALSCREEN), y: GetSystemMetrics(SM_YVIRTUALSCREEN), w: GetSystemMetrics(SM_CXVIRTUALSCREEN), h: GetSystemMetrics(SM_CYVIRTUALSCREEN) } }
    }

    /// The driver's displays: device name and whether it is on the desktop now. A display counts when its adapter
    /// is the Virtual Display Driver and a monitor is plugged into it.
    fn vdd_displays() -> Vec<(String, bool)> {
        let mut out = Vec::new();
        for i in 0..64u32 {
            // SAFETY: zeroed DISPLAY_DEVICEW with cb set, as the API asks.
            let (adapter, monitor) = unsafe {
                let mut a: DISPLAY_DEVICEW = std::mem::zeroed();
                a.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;
                if EnumDisplayDevicesW(std::ptr::null(), i, &mut a, 0) == 0 {
                    break;
                }
                let mut m: DISPLAY_DEVICEW = std::mem::zeroed();
                m.cb = a.cb;
                let has_monitor = EnumDisplayDevicesW(a.DeviceName.as_ptr(), 0, &mut m, 0) != 0;
                (a, has_monitor.then_some(m))
            };
            // The driver's monitors carry its EDID id MTT1337; its adapter name has changed between versions.
            let Some(m) = monitor else { continue };
            if !from_wide(&m.DeviceID).to_ascii_uppercase().contains("MTT1337") && !from_wide(&adapter.DeviceString).contains("Virtual Display Driver") {
                continue;
            }
            out.push((from_wide(&adapter.DeviceName), adapter.StateFlags & ATTACHED != 0));
        }
        out
    }

    /// Sets one display's size and place (or takes it off the desktop with a zero size), to apply with `apply`.
    fn stage(device: &str, rect: Option<Rect>) -> bool {
        let name = wide(device);
        // SAFETY: zeroed DEVMODEW with dmSize set; only the fields named in dmFields are read.
        unsafe {
            let mut dm: DEVMODEW = std::mem::zeroed();
            dm.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
            dm.dmFields = DM_POSITION | DM_PELSWIDTH | DM_PELSHEIGHT;
            if let Some(r) = rect {
                dm.Anonymous1.Anonymous2.dmPosition.x = r.x;
                dm.Anonymous1.Anonymous2.dmPosition.y = r.y;
                dm.dmPelsWidth = r.w as u32;
                dm.dmPelsHeight = r.h as u32;
            }
            ChangeDisplaySettingsExW(name.as_ptr(), &dm, std::ptr::null_mut(), CDS_UPDATEREGISTRY | CDS_NORESET, std::ptr::null()) == DISP_CHANGE_SUCCESSFUL
        }
    }

    fn apply() {
        // SAFETY: a null device and mode apply the staged changes.
        unsafe { ChangeDisplaySettingsExW(std::ptr::null(), std::ptr::null(), std::ptr::null_mut(), 0, std::ptr::null()) };
    }

    /// Reloads the driver with the new settings, waits for one display per rect and puts each there.
    pub fn bring_up(rects: &[Rect]) -> Result<Vec<String>, String> {
        if !send("RELOAD_DRIVER") {
            return Err("the display driver did not take the new settings".to_string());
        }
        // The driver takes a moment to drop its old monitors; counting at once would find those.
        std::thread::sleep(Duration::from_millis(1500));
        let until = Instant::now() + Duration::from_secs(15);
        let found = loop {
            std::thread::sleep(Duration::from_millis(300));
            let d = vdd_displays();
            if d.len() >= rects.len() {
                break d;
            }
            if Instant::now() > until {
                return Err(format!("the display driver made {} of {} virtual screens", d.len(), rects.len()));
            }
        };
        let mut devices = Vec::new();
        for (i, (device, _)) in found.iter().enumerate() {
            match rects.get(i) {
                Some(r) => {
                    if !stage(device, Some(*r)) {
                        detach_all();
                        return Err(format!("virtual screen {} cannot be set to {}x{}", i + 1, r.w, r.h));
                    }
                    devices.push(device.clone());
                }
                None => {
                    stage(device, None);
                }
            }
        }
        apply();
        Ok(devices)
    }

    pub fn detach_all() {
        let shown: Vec<String> = vdd_displays().into_iter().filter(|(_, on)| *on).map(|(d, _)| d).collect();
        if shown.is_empty() {
            return;
        }
        for d in &shown {
            stage(d, None);
        }
        apply();
    }

    pub fn detach_all_after_reload() {
        let until = Instant::now() + Duration::from_secs(5);
        while Instant::now() < until {
            std::thread::sleep(Duration::from_millis(300));
            if vdd_displays().iter().any(|(_, on)| *on) {
                detach_all();
                return;
            }
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::Rect;

    pub fn driver_present() -> bool {
        false
    }
    pub fn reload() {}
    pub fn desktop() -> Rect {
        Rect { x: 0, y: 0, w: 0, h: 0 }
    }
    pub fn bring_up(_rects: &[Rect]) -> Result<Vec<String>, String> {
        Err("virtual screens are only available on Windows".to_string())
    }
    pub fn detach_all() {}
    pub fn detach_all_after_reload() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    const NEW: &str = "<vdd_settings>\n    <monitors>\n        <count>1</count>\n    </monitors>\n    <resolutions>\n        <resolution>\n            <width>1920</width>\n            <height>1080</height>\n            <refresh_rate>60</refresh_rate>\n        </resolution>\n    </resolutions>\n</vdd_settings>\n";
    const OLD: &str = "<monitors>\n  <monitor>\n    <count>1</count>\n    <resolutions>\n      <resolution><width>1920</width><height>1080</height><refresh_rates><rate>60</rate></refresh_rates></resolution>\n    </resolutions>\n  </monitor>\n</monitors>\n";

    #[test]
    fn monitors_sit_off_the_desktop_touching_one_corner() {
        let desk = Rect { x: 0, y: 0, w: 3840, h: 1080 };
        let r = layout(desk, &[(1280, 1440), (1280, 1440)]);
        assert_eq!(r, vec![Rect { x: 3840, y: 1080, w: 1280, h: 1440 }, Rect { x: 5120, y: 1080, w: 1280, h: 1440 }]);
        assert_eq!(bounds(&[desk, Rect { x: -1920, y: 200, w: 1920, h: 1080 }]), Some(Rect { x: -1920, y: 0, w: 5760, h: 1280 }));
    }

    /// Live, on a PC with the driver: one virtual screen shown for a few seconds, then everything put back.
    /// `cargo test -p couchlinkd --bin couchlinkd live_virtual_screen -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_virtual_screen() {
        let dir = std::env::temp_dir().join("couchlink-vscreen-live");
        let _ = std::fs::create_dir_all(&dir);
        let (mut journal, _) = couchlink_core::Journal::open(&dir.join("journal.json"));
        let before = std::fs::read_to_string(settings_path()).unwrap();
        let s = Session::begin(&[(960, 540)], &mut journal).expect("virtual screen");
        println!("made {:?}", s.displays);
        assert!(is_ours(&s.displays[0].device));
        let shown = super::super::present::Presenter::start(s.displays[0].device.clone(), Rect { x: 100, y: 100, w: 960, h: 540 });
        std::thread::sleep(std::time::Duration::from_secs(5));
        drop(shown);
        let undoers = couchlink_core::Undoers::new();
        register_undoers(&undoers);
        let report = journal.replay(&undoers);
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        let strip = |t: &str| t.trim_start_matches('\u{feff}').to_string();
        assert_eq!(strip(&std::fs::read_to_string(settings_path()).unwrap()), strip(&before));
        assert!(!is_ours(&s.displays[0].device));
    }

    #[test]
    fn sizes_are_even_and_not_tiny() {
        assert_eq!(monitor_size(Rect { x: 0, y: 0, w: 853, h: 1080 }), (854, 1080));
        assert_eq!(monitor_size(Rect { x: 0, y: 0, w: 300, h: 200 }), (640, 480));
    }

    #[test]
    fn settings_get_the_count_and_new_sizes_in_both_layouts() {
        for xml in [NEW, OLD] {
            let s = settings_with(xml, 2, &[(1280, 1440), (1920, 1080), (1280, 1440)]).unwrap();
            assert_eq!(settings_count(&s), Some(2));
            assert!(has_resolution(&s, 1280, 1440) && has_resolution(&s, 1920, 1080));
            assert_eq!(s.matches("<width>1280</width>").count(), 1, "a size is added once:\n{s}");
            assert!(s.contains("<vdd_settings>") && s.ends_with("</vdd_settings>\n"), "{s}");
        }
        // The old layout becomes the driver's own and keeps its sizes.
        let s = settings_with(OLD, 2, &[(1280, 1440)]).unwrap();
        assert!(!s.contains("<monitor>") && s.contains("<friendlyname>default</friendlyname>"));
        assert_eq!(resolutions(&s), vec![(1920, 1080), (1280, 1440)]);
        assert!(settings_with("<x/>", 1, &[]).unwrap().contains("<vdd_settings>"), "anything else becomes the driver's layout");
        assert!(settings_with("<vdd_settings></vdd_settings>", 1, &[]).is_err());
    }
}
