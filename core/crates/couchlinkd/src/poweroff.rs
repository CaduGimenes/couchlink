//! Turning a wireless controller off. A Bluetooth controller (DualShock 4, DualSense, Switch Pro, Xbox over Bluetooth
//! through HID) is disconnected from the Bluetooth radio the way DS4Windows does it, with `IOCTL_BTH_DISCONNECT_DEVICE`
//! (Windows) or `bluetoothctl disconnect` (Linux); neither needs admin rights. A controller on the Xbox Wireless
//! Adapter or a 360 receiver (an XInput pad) is powered off with XInput's undocumented `XInputPowerOffController`.
//! Wired controllers cannot be turned off.
//!
//! The SDL thread registers a target per pad when it opens it; the hub and the SDL thread's hold-to-turn-off watcher ask
//! here whether a pad can be turned off and do it. The platform call is behind [`turn_off_with`] so tests never touch a
//! real controller.

use couchlink_core::Connection;
use std::sync::Mutex;

/// What turns one controller off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Disconnect this Bluetooth address (six bytes as an integer, `0xAABBCCDDEEFF`) from the radio.
    Bluetooth(u64),
    /// Power off the Xbox wireless controller in this XInput user slot (0 to 3).
    XInput(u32),
}

/// Each pad's target by SDL joystick id; pads that cannot be turned off have none.
static TARGETS: Mutex<Vec<(u32, Target)>> = Mutex::new(Vec::new());

fn targets() -> std::sync::MutexGuard<'static, Vec<(u32, Target)>> {
    TARGETS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Remember (or with None, forget) what turns the pad off.
pub fn register(id: u32, target: Option<Target>) {
    let mut t = targets();
    t.retain(|(i, _)| *i != id);
    if let Some(target) = target {
        t.push((id, target));
    }
}

/// True when the pad can be turned off.
pub fn can(id: u32) -> bool {
    targets().iter().any(|(i, _)| *i == id)
}

/// Turn the pad off.
pub fn turn_off(id: u32) -> Result<(), String> {
    turn_off_with(id, platform_turn_off)
}

/// Turn the pad off through `act` (the platform call; tests pass their own).
pub fn turn_off_with(id: u32, act: impl FnOnce(Target) -> Result<(), String>) -> Result<(), String> {
    let target = targets().iter().find(|(i, _)| *i == id).map(|(_, t)| *t);
    match target {
        Some(t) => act(t),
        None => Err("This controller cannot be turned off from here (wired controllers stay on)".to_string()),
    }
}

/// A Bluetooth address from the text a controller reports as its serial number: twelve hex digits, with or without
/// `:` or `-` between the pairs. An all-zero address means the controller did not tell.
pub fn parse_address(serial: &str) -> Option<u64> {
    let digits: String = serial.trim().chars().filter(|c| !matches!(c, ':' | '-')).collect();
    if digits.len() != 12 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    match u64::from_str_radix(&digits, 16) {
        Ok(0) | Err(_) => None,
        Ok(a) => Some(a),
    }
}

/// The text form `bluetoothctl` takes: `AA:BB:CC:DD:EE:FF`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn address_text(address: u64) -> String {
    let b = address.to_be_bytes();
    format!("{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}", b[2], b[3], b[4], b[5], b[6], b[7])
}

/// What can turn a pad off, from what SDL knows of it: its path (`XInput#N` for a pad SDL drives through XInput), its
/// serial number (the Bluetooth address for a Bluetooth HID pad) and whether it is wireless.
pub fn target_for(connection: Connection, path: Option<&str>, serial: Option<&str>) -> Option<Target> {
    if connection != Connection::Wireless {
        return None;
    }
    if let Some(slot) = path.and_then(|p| p.strip_prefix("XInput#")).and_then(|n| n.parse::<u32>().ok()) {
        return cfg!(windows).then_some(Target::XInput(slot));
    }
    if !cfg!(any(windows, target_os = "linux")) {
        return None;
    }
    serial.and_then(parse_address).map(Target::Bluetooth)
}

#[cfg(windows)]
fn platform_turn_off(target: Target) -> Result<(), String> {
    match target {
        Target::Bluetooth(address) => win::bluetooth_disconnect(address),
        Target::XInput(slot) => win::xinput_power_off(slot),
    }
}

#[cfg(target_os = "linux")]
fn platform_turn_off(target: Target) -> Result<(), String> {
    match target {
        Target::Bluetooth(address) => {
            let out = std::process::Command::new("bluetoothctl")
                .args(["disconnect", &address_text(address)])
                .output()
                .map_err(|e| format!("cannot run bluetoothctl: {e}"))?;
            if out.status.success() {
                Ok(())
            } else {
                Err(format!("bluetoothctl could not disconnect the controller: {}", String::from_utf8_lossy(&out.stdout).trim()))
            }
        }
        Target::XInput(_) => Err("XInput controllers exist on Windows only".to_string()),
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
fn platform_turn_off(_target: Target) -> Result<(), String> {
    Err("Turning a controller off is not supported on this system".to_string())
}

#[cfg(windows)]
mod win {
    use windows_sys::Win32::Devices::Bluetooth::{BluetoothFindFirstRadio, BluetoothFindNextRadio, BluetoothFindRadioClose, BLUETOOTH_FIND_RADIO_PARAMS};
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::IO::DeviceIoControl;
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};

    /// `IOCTL_BTH_DISCONNECT_DEVICE` (bthioctl.h), taking the 8 byte `BTH_ADDR`.
    const IOCTL_BTH_DISCONNECT_DEVICE: u32 = 0x0041_000C;
    /// `XInputPowerOffController` has no name in xinput1_4.dll, only this ordinal.
    const XINPUT_POWER_OFF_ORDINAL: usize = 103;

    /// Ask every Bluetooth radio to drop the connection to `address`.
    pub fn bluetooth_disconnect(address: u64) -> Result<(), String> {
        let params = BLUETOOTH_FIND_RADIO_PARAMS { dwSize: std::mem::size_of::<BLUETOOTH_FIND_RADIO_PARAMS>() as u32 };
        let mut radio: HANDLE = std::ptr::null_mut();
        // SAFETY: `params` and `radio` outlive the call.
        let find = unsafe { BluetoothFindFirstRadio(&params, &mut radio) };
        if find.is_null() {
            return Err("No Bluetooth radio found on this PC".to_string());
        }
        let mut done = false;
        loop {
            let mut returned = 0u32;
            // SAFETY: `radio` is an open radio handle; the input is the 8 byte address; no output is taken.
            let ok = unsafe { DeviceIoControl(radio, IOCTL_BTH_DISCONNECT_DEVICE, &address as *const u64 as *const _, 8, std::ptr::null_mut(), 0, &mut returned, std::ptr::null_mut()) };
            done |= ok != 0;
            // SAFETY: the handle came from the find calls and is closed once.
            unsafe { CloseHandle(radio) };
            // SAFETY: `find` is the live search handle.
            if unsafe { BluetoothFindNextRadio(find, &mut radio) } == 0 {
                break;
            }
        }
        // SAFETY: `find` is closed once.
        unsafe { BluetoothFindRadioClose(find) };
        if done { Ok(()) } else { Err("Windows could not disconnect the controller (is it still connected over Bluetooth?)".to_string()) }
    }

    /// Power off the controller in an XInput slot.
    pub fn xinput_power_off(slot: u32) -> Result<(), String> {
        for dll in [&b"xinput1_4.dll\0"[..], &b"xinput1_3.dll\0"[..]] {
            // SAFETY: `dll` is NUL terminated.
            let module = unsafe { LoadLibraryA(dll.as_ptr()) };
            if module.is_null() {
                continue;
            }
            // SAFETY: an ordinal is passed as a pointer-sized integer in place of a name.
            let Some(f) = (unsafe { GetProcAddress(module, XINPUT_POWER_OFF_ORDINAL as *const u8) }) else { continue };
            // SAFETY: the export at this ordinal is `DWORD XInputPowerOffController(DWORD dwUserIndex)`.
            let f: extern "system" fn(u32) -> u32 = unsafe { std::mem::transmute(f) };
            return match f(slot) {
                0 => Ok(()),
                code => Err(format!("The Xbox controller did not turn off (error {code})")),
            };
        }
        Err("This Windows has no XInput power off".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_read_from_a_serial_with_or_without_separators() {
        assert_eq!(parse_address("a4ae1234abcd"), Some(0xA4AE_1234_ABCD));
        assert_eq!(parse_address("A4:AE:12:34:AB:CD"), Some(0xA4AE_1234_ABCD));
        assert_eq!(parse_address(" a4-ae-12-34-ab-cd "), Some(0xA4AE_1234_ABCD));
        assert_eq!(address_text(0xA4AE_1234_ABCD), "A4:AE:12:34:AB:CD");
    }

    #[test]
    fn serials_that_are_not_addresses_are_refused() {
        for s in ["", "0000000000000", "000000000000", "a4ae1234abc", "a4ae1234abcdef", "g4ae1234abcd", "Controller 1"] {
            assert_eq!(parse_address(s), None, "{s:?}");
        }
    }

    #[test]
    fn only_wireless_pads_have_a_target() {
        assert_eq!(target_for(Connection::Usb, None, Some("a4ae1234abcd")), None);
        assert_eq!(target_for(Connection::Unknown, Some("XInput#0"), None), None);
        assert_eq!(target_for(Connection::Wireless, None, Some("not an address")), None);
        assert_eq!(target_for(Connection::Wireless, None, None), None);
        #[cfg(windows)]
        assert_eq!(target_for(Connection::Wireless, Some("XInput#2"), None), Some(Target::XInput(2)));
        #[cfg(any(windows, target_os = "linux"))]
        assert_eq!(target_for(Connection::Wireless, Some("\\\\?\\HID#VID_054C"), Some("a4:ae:12:34:ab:cd")), Some(Target::Bluetooth(0xA4AE_1234_ABCD)));
    }

    #[test]
    fn a_registered_pad_is_turned_off_through_the_given_action_only() {
        // High ids no real pad has; the platform call is never made.
        register(900_001, Some(Target::Bluetooth(0xA4AE_1234_ABCD)));
        assert!(can(900_001));
        let mut seen = None;
        assert!(turn_off_with(900_001, |t| { seen = Some(t); Ok(()) }).is_ok());
        assert_eq!(seen, Some(Target::Bluetooth(0xA4AE_1234_ABCD)));
        assert_eq!(turn_off_with(900_001, |_| Err("no".into())), Err("no".to_string()));
        register(900_001, None);
        assert!(!can(900_001));
        assert!(turn_off_with(900_001, |_| panic!("not registered")).unwrap_err().contains("wired"));
    }
}
