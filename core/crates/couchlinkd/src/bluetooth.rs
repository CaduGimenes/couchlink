//! Pairing new Bluetooth controllers from inside Couchlink (Windows: WinRT; other systems: not offered).
//!
//! Scanning runs on its own thread while the app's pairing dialog is open; pairing runs on a thread per controller.
//! The radio sits behind [`Backend`], so the unit tests use a fake and never touch a real one.

use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// An unpaired Bluetooth device the radio heard.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub id: String,
    pub name: String,
    /// Class of Device, when the backend read it.
    pub class: Option<u32>,
}

/// What talks to the Bluetooth radio.
pub trait Backend: Send + Sync + 'static {
    /// Whether this PC has a Bluetooth radio Couchlink can pair with.
    fn available(&self) -> bool;
    /// The unpaired devices in range right now.
    fn find(&self) -> Result<Vec<Candidate>, String>;
    /// Pairs with one of them. `Err` is a short readable reason.
    fn pair(&self, id: &str) -> Result<(), String>;
}

/// Names game controllers go by while pairing.
const NAMES: [&str; 6] = ["wireless controller", "dualsense", "dualshock", "xbox", "pro controller", "joy-con"];

/// Class of Device: major class Peripheral (5), minor class joystick (1) or gamepad (2).
pub fn class_is_controller(class: u32) -> bool {
    let major = (class >> 8) & 0x1f;
    let minor = (class >> 2) & 0x3f;
    // The top two minor bits flag a keyboard or a pointing device; the low four say what else it is.
    major == 5 && minor >> 4 == 0 && matches!(minor & 0xf, 1 | 2)
}

pub fn name_is_controller(name: &str) -> bool {
    let n = name.to_lowercase();
    NAMES.iter().any(|x| n.contains(x))
}

pub fn is_controller(c: &Candidate) -> bool {
    !c.name.trim().is_empty() && (c.class.is_some_and(class_is_controller) || name_is_controller(&c.name))
}

/// "PlayStation 4", "PlayStation 5", "Xbox" or "Switch"; empty when the name does not say.
pub fn guess_kind(name: &str) -> &'static str {
    let n = name.to_lowercase();
    if n.contains("xbox") {
        "Xbox"
    } else if n.contains("dualsense") {
        "PlayStation 5"
    } else if n.contains("wireless controller") || n.contains("dualshock") {
        "PlayStation 4"
    } else if n.contains("pro controller") || n.contains("joy-con") {
        "Switch"
    } else {
        ""
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Pairing {
    Found,
    Pairing,
    Paired,
    Failed,
}

impl Pairing {
    fn as_str(self) -> &'static str {
        match self {
            Pairing::Found => "found",
            Pairing::Pairing => "pairing",
            Pairing::Paired => "paired",
            Pairing::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone)]
struct Entry {
    id: String,
    name: String,
    kind: &'static str,
    state: Pairing,
    error: Option<String>,
}

#[derive(Default)]
struct Inner {
    /// Bumped on every start and stop; a scan thread of an older one ends.
    generation: u64,
    scanning: bool,
    entries: Vec<Entry>,
}

impl Inner {
    /// Folds one scan in: new controllers appear, ones no longer heard disappear (unless they are being paired or
    /// were paired or failed a moment ago, which the dialog still shows).
    fn merge(&mut self, found: &[Candidate]) {
        for c in found.iter().filter(|c| is_controller(c)) {
            if !self.entries.iter().any(|e| e.id == c.id) {
                self.entries.push(Entry { id: c.id.clone(), name: c.name.clone(), kind: guess_kind(&c.name), state: Pairing::Found, error: None });
            }
        }
        self.entries.retain(|e| e.state != Pairing::Found || found.iter().any(|c| c.id == e.id));
    }
}

pub struct Bluetooth {
    backend: Arc<dyn Backend>,
    inner: Arc<Mutex<Inner>>,
    /// Time between two scans.
    interval: Duration,
}

impl Bluetooth {
    pub fn new(backend: Arc<dyn Backend>, interval: Duration) -> Bluetooth {
        Bluetooth { backend, inner: Arc::new(Mutex::new(Inner::default())), interval }
    }

    pub fn system() -> Bluetooth {
        Bluetooth::new(system_backend(), Duration::from_secs(3))
    }

    /// Starts or stops looking for controllers in pairing mode.
    pub fn set_scan(&self, on: bool) {
        let generation = {
            let mut s = self.inner.lock().unwrap();
            if s.scanning == on {
                return;
            }
            s.scanning = on;
            s.generation += 1;
            if !on {
                s.entries.retain(|e| e.state == Pairing::Pairing);
            }
            s.generation
        };
        if !on || !self.backend.available() {
            return;
        }
        let (backend, inner, interval) = (self.backend.clone(), self.inner.clone(), self.interval);
        std::thread::spawn(move || loop {
            let found = backend.find();
            {
                let mut s = inner.lock().unwrap();
                if s.generation != generation {
                    return;
                }
                match found {
                    Ok(list) => s.merge(&list),
                    Err(e) => eprintln!("couchlinkd: bluetooth scan: {e}"),
                }
            }
            // Short sleeps, so a stop ends the thread at once.
            let mut waited = Duration::ZERO;
            while waited < interval {
                let step = Duration::from_millis(50).min(interval - waited);
                std::thread::sleep(step);
                waited += step;
                if inner.lock().unwrap().generation != generation {
                    return;
                }
            }
        });
    }

    /// Starts pairing with a controller the last scan found. Returns at once; `devices` shows how it goes.
    pub fn pair(&self, id: &str) -> Result<(), String> {
        {
            let mut s = self.inner.lock().unwrap();
            let Some(e) = s.entries.iter_mut().find(|e| e.id == id) else {
                return Err("That controller is not in pairing mode any more".to_string());
            };
            if e.state == Pairing::Pairing || e.state == Pairing::Paired {
                return Ok(());
            }
            e.state = Pairing::Pairing;
            e.error = None;
        }
        let (backend, inner, id) = (self.backend.clone(), self.inner.clone(), id.to_string());
        std::thread::spawn(move || {
            let result = backend.pair(&id);
            let mut s = inner.lock().unwrap();
            if let Some(e) = s.entries.iter_mut().find(|e| e.id == id) {
                match result {
                    Ok(()) => e.state = Pairing::Paired,
                    Err(err) => {
                        e.state = Pairing::Failed;
                        e.error = Some(err);
                    }
                }
            }
        });
        Ok(())
    }

    /// The `bluetooth_devices` answer.
    pub fn devices(&self) -> Value {
        let available = self.backend.available();
        let s = self.inner.lock().unwrap();
        let list: Vec<Value> = s
            .entries
            .iter()
            .map(|e| json!({ "id": e.id, "name": e.name, "kind": e.kind, "state": e.state.as_str(), "error": e.error }))
            .collect();
        json!({ "available": available, "scanning": s.scanning, "devices": list })
    }
}

/// Short, readable reasons for the Windows pairing results that are not a success. `None`: success.
pub fn pairing_error(status: i32) -> Option<&'static str> {
    Some(match status {
        0 | 3 => return None,
        1 => "The controller is not ready to pair. Put it in pairing mode again.",
        2 | 14 | 17 => "Pairing was cancelled. Try again.",
        4 => "The controller refused the connection.",
        5 => "The controller is already connected to something else.",
        6 => "The Bluetooth adapter reported a hardware problem.",
        7 => "The controller did not answer in time. Put it in pairing mode again.",
        8 | 9 => "The controller did not accept the pairing. Put it in pairing mode again.",
        10 => "Windows has no driver for this controller over Bluetooth.",
        12 => "Windows would not allow pairing.",
        15 => "Another pairing is already running.",
        _ => "Pairing did not work. Put the controller in pairing mode again.",
    })
}

#[cfg(windows)]
fn system_backend() -> Arc<dyn Backend> {
    Arc::new(win::Windows::default())
}

#[cfg(not(windows))]
fn system_backend() -> Arc<dyn Backend> {
    Arc::new(Unsupported)
}

/// No pairing on this system (Linux: use the desktop's Bluetooth settings).
#[cfg(not(windows))]
struct Unsupported;

#[cfg(not(windows))]
impl Backend for Unsupported {
    fn available(&self) -> bool {
        false
    }
    fn find(&self) -> Result<Vec<Candidate>, String> {
        Ok(Vec::new())
    }
    fn pair(&self, _id: &str) -> Result<(), String> {
        Err("Pairing is not offered on this system".to_string())
    }
}

#[cfg(windows)]
mod win {
    use super::{name_is_controller, pairing_error, Backend, Candidate};
    use std::sync::OnceLock;
    use windows::core::HSTRING;
    use windows::Devices::Bluetooth::{BluetoothAdapter, BluetoothDevice};
    use windows::Devices::Enumeration::{DeviceInformation, DevicePairingKinds, DevicePairingRequestedEventArgs};
    use windows::Foundation::TypedEventHandler;
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

    /// Runs `f` on a thread of its own in a multithreaded COM apartment (the hub's threads have none).
    fn in_mta<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        std::thread::spawn(move || {
            let inited = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
            let out = f();
            if inited {
                unsafe { CoUninitialize() };
            }
            out
        })
        .join()
        .expect("bluetooth thread")
    }

    #[derive(Default)]
    pub struct Windows {
        available: OnceLock<bool>,
    }

    impl Backend for Windows {
        fn available(&self) -> bool {
            *self.available.get_or_init(|| {
                in_mta(|| BluetoothAdapter::GetDefaultAsync().and_then(|a| a.join()).and_then(|a| a.IsClassicSupported()).unwrap_or(false))
            })
        }

        fn find(&self) -> Result<Vec<Candidate>, String> {
            in_mta(|| find_all().map_err(|e| e.message()))
        }

        fn pair(&self, id: &str) -> Result<(), String> {
            let id = id.to_string();
            in_mta(move || pair_one(&id))
        }
    }

    fn find_all() -> windows::core::Result<Vec<Candidate>> {
        let selector = BluetoothDevice::GetDeviceSelectorFromPairingState(false)?;
        let infos = DeviceInformation::FindAllAsyncAqsFilter(&selector)?.join()?;
        let mut out = Vec::new();
        for info in infos {
            let id = info.Id()?;
            let name = info.Name()?.to_string();
            // The Class of Device is only read for names that do not already say "controller".
            let class = if name_is_controller(&name) {
                None
            } else {
                BluetoothDevice::FromIdAsync(&id).and_then(|d| d.join()).and_then(|d| d.ClassOfDevice()).and_then(|c| c.RawValue()).ok()
            };
            out.push(Candidate { id: id.to_string(), name, class });
        }
        Ok(out)
    }

    fn pair_one(id: &str) -> Result<(), String> {
        let run = || -> windows::core::Result<i32> {
            let info = DeviceInformation::CreateFromIdAsync(&HSTRING::from(id))?.join()?;
            let custom = info.Pairing()?.Custom()?;
            // Controllers pair without a ceremony, or with a confirmation, or (some) with the PIN 0000: say yes to all.
            let token = custom.PairingRequested(&TypedEventHandler::new(|_, args: windows::core::Ref<DevicePairingRequestedEventArgs>| {
                if let Some(args) = args.as_ref() {
                    if args.PairingKind()? == DevicePairingKinds::ProvidePin {
                        args.AcceptWithPin(&HSTRING::from("0000"))?;
                    } else {
                        args.Accept()?;
                    }
                }
                Ok(())
            }))?;
            let kinds = DevicePairingKinds::ConfirmOnly | DevicePairingKinds::None | DevicePairingKinds::ProvidePin;
            let result = custom.PairAsync(kinds)?.join();
            let _ = custom.RemovePairingRequested(token);
            Ok(result?.Status()?.0)
        };
        match run() {
            Ok(status) => pairing_error(status).map_or(Ok(()), |e| Err(e.to_string())),
            Err(e) => Err(e.message()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct Fake {
        found: Mutex<Vec<Candidate>>,
        pair_result: Mutex<Result<(), String>>,
        release: AtomicBool,
    }

    impl Fake {
        fn new(found: Vec<Candidate>) -> Arc<Fake> {
            Arc::new(Fake { found: Mutex::new(found), pair_result: Mutex::new(Ok(())), release: AtomicBool::new(true) })
        }
    }

    impl Backend for Fake {
        fn available(&self) -> bool {
            true
        }
        fn find(&self) -> Result<Vec<Candidate>, String> {
            Ok(self.found.lock().unwrap().clone())
        }
        fn pair(&self, _id: &str) -> Result<(), String> {
            while !self.release.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(5));
            }
            self.pair_result.lock().unwrap().clone()
        }
    }

    fn cand(id: &str, name: &str, class: Option<u32>) -> Candidate {
        Candidate { id: id.to_string(), name: name.to_string(), class }
    }

    fn wait_for(bt: &Bluetooth, f: impl Fn(&Value) -> bool) -> Value {
        for _ in 0..400 {
            let v = bt.devices();
            if f(&v) {
                return v;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("timed out: {}", bt.devices());
    }

    #[test]
    fn controllers_are_told_from_other_devices() {
        // 0x002508 is the gamepad class DualShock 4 and DualSense report; 0x000504 a joystick; 0x002540 a keyboard.
        assert!(class_is_controller(0x002508));
        assert!(class_is_controller(0x000504));
        assert!(!class_is_controller(0x002540), "keyboard");
        assert!(!class_is_controller(0x002580), "mouse");
        assert!(!class_is_controller(0x240404), "headset");
        assert!(is_controller(&cand("a", "Wireless Controller", None)));
        assert!(is_controller(&cand("a", "Some pad", Some(0x002508))));
        assert!(!is_controller(&cand("a", "Some keyboard", Some(0x002540))));
        assert!(!is_controller(&cand("a", "", Some(0x002508))), "no name");
        assert!(!is_controller(&cand("a", "JBL Speaker", None)));
    }

    #[test]
    fn the_kind_comes_from_the_name() {
        assert_eq!(guess_kind("Wireless Controller"), "PlayStation 4");
        assert_eq!(guess_kind("DualSense Wireless Controller"), "PlayStation 5");
        assert_eq!(guess_kind("Xbox Wireless Controller"), "Xbox");
        assert_eq!(guess_kind("Pro Controller"), "Switch");
        assert_eq!(guess_kind("Joy-Con (L)"), "Switch");
        assert_eq!(guess_kind("Generic pad"), "");
    }

    #[test]
    fn windows_results_become_readable_errors() {
        assert_eq!(pairing_error(0), None);
        assert_eq!(pairing_error(3), None, "already paired is fine");
        assert!(pairing_error(7).unwrap().contains("pairing mode"));
        assert!(pairing_error(99).is_some());
    }

    #[test]
    fn scanning_lists_only_controllers_and_stops() {
        let fake = Fake::new(vec![cand("bt1", "Wireless Controller", None), cand("bt2", "JBL Speaker", None)]);
        let bt = Bluetooth::new(fake.clone(), Duration::from_millis(10));
        assert_eq!(bt.devices()["scanning"], false);
        bt.set_scan(true);
        let v = wait_for(&bt, |v| v["devices"].as_array().is_some_and(|d| !d.is_empty()));
        assert_eq!(v["scanning"], true);
        assert_eq!(v["devices"].as_array().unwrap().len(), 1);
        assert_eq!(v["devices"][0]["name"], "Wireless Controller");
        assert_eq!(v["devices"][0]["kind"], "PlayStation 4");
        assert_eq!(v["devices"][0]["state"], "found");
        // It leaves pairing mode: it drops off the list at the next scan.
        fake.found.lock().unwrap().clear();
        wait_for(&bt, |v| v["devices"].as_array().is_some_and(|d| d.is_empty()));
        bt.set_scan(false);
        assert_eq!(bt.devices()["scanning"], false);
    }

    #[test]
    fn pairing_goes_from_pairing_to_paired() {
        let fake = Fake::new(vec![cand("bt1", "Pro Controller", None)]);
        fake.release.store(false, Ordering::SeqCst);
        let bt = Bluetooth::new(fake.clone(), Duration::from_millis(10));
        bt.set_scan(true);
        wait_for(&bt, |v| !v["devices"].as_array().unwrap().is_empty());
        bt.pair("bt1").unwrap();
        assert_eq!(bt.devices()["devices"][0]["state"], "pairing");
        // Heard no more while pairing (it leaves the unpaired list): still shown.
        fake.found.lock().unwrap().clear();
        std::thread::sleep(Duration::from_millis(40));
        assert_eq!(bt.devices()["devices"][0]["state"], "pairing");
        fake.release.store(true, Ordering::SeqCst);
        let v = wait_for(&bt, |v| v["devices"][0]["state"] != "pairing");
        assert_eq!(v["devices"][0]["state"], "paired");
        assert_eq!(v["devices"][0]["error"], Value::Null);
        bt.set_scan(false);
    }

    #[test]
    fn a_failed_pairing_keeps_its_reason_and_can_be_retried() {
        let fake = Fake::new(vec![cand("bt1", "Xbox Wireless Controller", None)]);
        *fake.pair_result.lock().unwrap() = Err("The controller did not answer in time.".to_string());
        let bt = Bluetooth::new(fake.clone(), Duration::from_millis(10));
        bt.set_scan(true);
        wait_for(&bt, |v| !v["devices"].as_array().unwrap().is_empty());
        bt.pair("bt1").unwrap();
        let v = wait_for(&bt, |v| v["devices"][0]["state"] == "failed");
        assert_eq!(v["devices"][0]["error"], "The controller did not answer in time.");
        *fake.pair_result.lock().unwrap() = Ok(());
        bt.pair("bt1").unwrap();
        wait_for(&bt, |v| v["devices"][0]["state"] == "paired");
        assert!(bt.pair("nope").is_err());
        bt.set_scan(false);
    }
}
