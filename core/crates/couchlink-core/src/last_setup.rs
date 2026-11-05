//! The setup of the last game played: who sat where with which devices, and the screens. It comes back by itself the
//! next time Couchlink starts, as those devices connect, so nobody has to seat everyone and arrange the screens again.
//!
//! A controller's id changes every run, so a device is remembered by what stays the same (`identity` from the caller:
//! a serial number or device path), and by its name for when that changed too (another USB port).

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::screens::Screen;
use crate::seats::SeatManager;
use crate::types::Device;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RememberedDevice {
    pub key: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RememberedSeat {
    pub seat: usize,
    pub name: String,
    pub devices: Vec<RememberedDevice>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LastSetup {
    pub seats: Vec<RememberedSeat>,
    pub screens: Vec<Screen>,
}

impl LastSetup {
    /// The setup as it is now, or `None` when no seat has a device worth remembering. `identity` gives a device's
    /// lasting key, and `None` for one that should not come back (a test's virtual pad).
    pub fn capture(seats: &SeatManager, screens: &[Screen], identity: impl Fn(&Device) -> Option<String>) -> Option<LastSetup> {
        let mut out = Vec::new();
        for (index, seat) in seats.seats().iter().enumerate() {
            let Some(seat) = seat else { continue };
            let devices: Vec<RememberedDevice> = seat
                .devices
                .iter()
                .filter_map(|id| seats.device(id))
                .filter_map(|d| Some(RememberedDevice { key: identity(d)?, name: d.name.clone() }))
                .collect();
            if !devices.is_empty() {
                out.push(RememberedSeat { seat: index, name: seat.name.clone(), devices });
            }
        }
        (!out.is_empty()).then(|| LastSetup { seats: out, screens: screens.to_vec() })
    }

    /// Seats every remembered device that is connected and has no seat yet. True when any was seated. Call it again
    /// when more devices connect. An exact key goes first; a device of the same name only takes a place that its own
    /// device did not claim.
    pub fn restore(&self, seats: &mut SeatManager, identity: impl Fn(&Device) -> Option<String>) -> bool {
        let key_of = |d: &Device| identity(d);
        let mut placed = false;
        // Places still open, once the devices already in them are counted.
        let mut open: Vec<(usize, &RememberedDevice)> = Vec::new();
        let mut used: HashSet<String> = HashSet::new();
        for remembered in &self.seats {
            for want in &remembered.devices {
                let here = seats.seats()[remembered.seat].as_ref().map(|s| s.devices.clone()).unwrap_or_default();
                let filled = here
                    .iter()
                    .filter(|id| !used.contains(*id))
                    .find(|id| seats.device(id).is_some_and(|d| key_of(d).as_deref() == Some(want.key.as_str()) || d.name == want.name))
                    .cloned();
                match filled {
                    Some(id) => {
                        used.insert(id);
                    }
                    None => open.push((remembered.seat, want)),
                }
            }
        }
        let free = |seats: &SeatManager, used: &HashSet<String>| -> Vec<Device> {
            seats.devices().iter().filter(|d| seats.seat_of(&d.id).is_none() && !used.contains(&d.id) && key_of(d).is_some()).cloned().collect()
        };
        let mut later = Vec::new();
        for (seat, want) in open {
            match free(seats, &used).into_iter().find(|d| key_of(d).as_deref() == Some(want.key.as_str())) {
                Some(d) if seats.assign(&d.id, seat).is_ok() => {
                    used.insert(d.id);
                    placed = true;
                }
                _ => later.push((seat, want)),
            }
        }
        for (seat, want) in later {
            if let Some(d) = free(seats, &used).into_iter().find(|d| d.name == want.name) {
                if seats.assign(&d.id, seat).is_ok() {
                    used.insert(d.id);
                    placed = true;
                }
            }
        }
        if placed {
            for remembered in &self.seats {
                if seats.seats()[remembered.seat].is_some() {
                    let _ = seats.rename(remembered.seat, &remembered.name);
                }
            }
        }
        placed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Connection, DeviceKind, Family};

    fn device(id: &str, name: &str) -> Device {
        Device {
            id: id.to_string(),
            kind: if id.starts_with("pad:") { DeviceKind::Gamepad } else { DeviceKind::Kbm },
            name: name.to_string(),
            family: Family::Generic,
            connection: Connection::Unknown,
            battery: None,
            charging: false,
            can_turn_off: false,
            seat: None,
        }
    }

    // In these tests a pad's lasting key is written in a table, like the hub's; keyboards keep their id.
    fn identity(table: &'static [(&'static str, &'static str)]) -> impl Fn(&Device) -> Option<String> {
        move |d: &Device| {
            if d.id.starts_with("pad:") {
                table.iter().find(|(id, _)| *id == d.id).map(|(_, key)| key.to_string())
            } else {
                Some(d.id.clone())
            }
        }
    }

    fn played() -> LastSetup {
        let mut seats = SeatManager::new();
        seats.upsert_device(device("pad:1", "DualSense"));
        seats.upsert_device(device("pad:2", "DualSense"));
        seats.upsert_device(device("kbd:aa", "Keyboard"));
        seats.assign("pad:2", 0).unwrap();
        seats.assign("pad:1", 1).unwrap();
        seats.assign("kbd:aa", 2).unwrap();
        seats.rename(1, "Sam").unwrap();
        LastSetup::capture(&seats, &[], identity(&[("pad:1", "serial:one"), ("pad:2", "serial:two")])).unwrap()
    }

    #[test]
    fn devices_go_back_to_their_seats_though_their_ids_changed() {
        let setup = played();
        let mut seats = SeatManager::new();
        seats.upsert_device(device("pad:7", "DualSense"));
        seats.upsert_device(device("pad:8", "DualSense"));
        seats.upsert_device(device("kbd:aa", "Keyboard"));
        assert!(setup.restore(&mut seats, identity(&[("pad:7", "serial:one"), ("pad:8", "serial:two")])));
        assert_eq!(seats.seat_of("pad:8"), Some(0));
        assert_eq!(seats.seat_of("pad:7"), Some(1));
        assert_eq!(seats.seat_of("kbd:aa"), Some(2));
        assert_eq!(seats.seats()[1].as_ref().unwrap().name, "Sam");
    }

    #[test]
    fn a_controller_turned_on_later_finds_its_seat_and_the_others_stay() {
        let setup = played();
        let table: &[(&str, &str)] = &[("pad:7", "serial:one"), ("pad:8", "serial:two")];
        let mut seats = SeatManager::new();
        seats.upsert_device(device("pad:7", "DualSense"));
        assert!(setup.restore(&mut seats, identity(table)));
        // Its exact key says the second seat; the first waits for its own controller.
        assert_eq!(seats.seat_of("pad:7"), Some(1));
        seats.upsert_device(device("pad:8", "DualSense"));
        assert!(setup.restore(&mut seats, identity(table)));
        assert_eq!(seats.seat_of("pad:8"), Some(0));
        assert_eq!(seats.seat_of("pad:7"), Some(1), "unmoved");
        assert!(!setup.restore(&mut seats, identity(table)), "nothing left to do");
    }

    #[test]
    fn a_controller_on_another_port_is_known_by_its_name() {
        let setup = played();
        let mut seats = SeatManager::new();
        seats.upsert_device(device("pad:9", "DualSense"));
        assert!(setup.restore(&mut seats, identity(&[("pad:9", "path:new-port")])));
        assert_eq!(seats.seat_of("pad:9"), Some(0));
    }

    #[test]
    fn a_device_nobody_remembers_stays_unseated_and_a_virtual_pad_is_not_remembered() {
        let setup = played();
        let mut seats = SeatManager::new();
        seats.upsert_device(device("pad:9", "Some other pad"));
        assert!(!setup.restore(&mut seats, identity(&[("pad:9", "serial:other")])));
        assert_eq!(seats.seat_of("pad:9"), None);

        let mut seats = SeatManager::new();
        seats.upsert_device(device("pad:1", "Couchlink Virtual Pad 1"));
        seats.assign("pad:1", 0).unwrap();
        assert!(LastSetup::capture(&seats, &[], identity(&[])).is_none());
    }
}
