//! Saved screen layouts: a screen's regions kept under a name, so any game can use them again.
//!
//! A layout is one screen without its players: the regions in seat order, the UI hints (shape, sizes, custom)
//! and how games are shown. The player on `regions[i]` is whoever sits `i`th on the screen it is applied to.
//! A game can also pin a layout, which couchlinkd then applies when that game starts.

use crate::screens::{self, Present, Screen};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Longest layout name, in characters.
pub const MAX_NAME: usize = 40;

/// One saved layout. `display` is the display it was saved from; it only helps pick a screen at game start.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedLayout {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub display: String,
    pub regions: Vec<[f64; 4]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub custom: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sizes: Option<Value>,
    #[serde(default)]
    pub present: Present,
    /// Unix seconds when it was saved.
    #[serde(default)]
    pub saved: u64,
}

impl SavedLayout {
    /// Whether it can be applied to a screen with `seats` players: the same number of regions, or (for free
    /// rectangles) more of them, of which the first ones are used.
    pub fn fits(&self, seats: usize) -> bool {
        seats > 0 && (self.regions.len() == seats || (self.custom && self.regions.len() > seats))
    }

    /// Puts the layout on `screen`, keeping its players. Returns false (and leaves the screen alone) when it does not fit.
    pub fn apply(&self, screen: &mut Screen) -> bool {
        let n = screen.seats.len();
        if !self.fits(n) {
            return false;
        }
        let same = self.regions.len() == n;
        screen.regions = self.regions[..n].to_vec();
        screen.shape = if same { self.shape.clone() } else { None };
        screen.custom = self.custom;
        screen.sizes = if same { self.sizes.clone() } else { None };
        screen.present = self.present;
        true
    }
}

/// Checks and trims a layout name.
pub fn clean_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("give the layout a name".to_string());
    }
    if name.chars().count() > MAX_NAME {
        return Err(format!("the name is longer than {MAX_NAME} characters"));
    }
    if name.chars().any(char::is_control) {
        return Err("the name has characters that cannot be used".to_string());
    }
    Ok(name.to_string())
}

/// Every saved layout, and the layout each game (by recipe id) starts with.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Layouts {
    #[serde(default)]
    pub layouts: Vec<SavedLayout>,
    #[serde(default)]
    pub pins: BTreeMap<String, String>,
}

impl Layouts {
    pub fn get(&self, id: &str) -> Option<&SavedLayout> {
        self.layouts.iter().find(|l| l.id == id)
    }

    fn name_taken(&self, name: &str, except: Option<&str>) -> bool {
        self.layouts.iter().any(|l| Some(l.id.as_str()) != except && l.name.to_lowercase() == name.to_lowercase())
    }

    /// Saves the screen as it is (its players are left out). Returns the new layout.
    pub fn save(&mut self, name: &str, screen: &Screen, now: u64) -> Result<SavedLayout, String> {
        let name = clean_name(name)?;
        if self.name_taken(&name, None) {
            return Err(format!("a layout called {name:?} exists already"));
        }
        let n = screen.regions.len();
        if n == 0 {
            return Err("the screen has no players to save".to_string());
        }
        // The regions must be a sound arrangement of their own, whoever ends up on them.
        let seats: Vec<u8> = (0..n.min(crate::MAX_SEATS) as u8).collect();
        let probe = Screen { display: screen.display.clone(), on: true, seats, regions: screen.regions.clone(), shape: None, custom: false, sizes: None, present: Present::Window };
        screens::validate(&[probe], &[true; crate::MAX_SEATS])?;
        let next = self.layouts.iter().filter_map(|l| l.id.strip_prefix('l')?.parse::<u64>().ok()).max().unwrap_or(0) + 1;
        let layout = SavedLayout {
            id: format!("l{next}"),
            name,
            display: screen.display.clone(),
            regions: screen.regions.clone(),
            shape: screen.shape.clone(),
            custom: screen.custom,
            sizes: screen.sizes.clone(),
            present: screen.present,
            saved: now,
        };
        self.layouts.push(layout.clone());
        Ok(layout)
    }

    pub fn rename(&mut self, id: &str, name: &str) -> Result<(), String> {
        let name = clean_name(name)?;
        if self.name_taken(&name, Some(id)) {
            return Err(format!("a layout called {name:?} exists already"));
        }
        let l = self.layouts.iter_mut().find(|l| l.id == id).ok_or("no such layout")?;
        l.name = name;
        Ok(())
    }

    /// Deletes a layout; games that started with it go back to the screens as they are.
    pub fn delete(&mut self, id: &str) -> Result<(), String> {
        let before = self.layouts.len();
        self.layouts.retain(|l| l.id != id);
        if self.layouts.len() == before {
            return Err("no such layout".to_string());
        }
        self.pins.retain(|_, v| v != id);
        Ok(())
    }

    /// Makes `recipe` start with layout `id`, or (`None`) with the screens as they are.
    pub fn pin(&mut self, recipe: &str, id: Option<&str>) -> Result<(), String> {
        if recipe.is_empty() {
            return Err("no game given".to_string());
        }
        match id {
            Some(id) => {
                if self.get(id).is_none() {
                    return Err("no such layout".to_string());
                }
                self.pins.insert(recipe.to_string(), id.to_string());
            }
            None => {
                self.pins.remove(recipe);
            }
        }
        Ok(())
    }

    /// Applies the layout `recipe` is pinned to onto `screens`, if there is one and it fits somewhere. It goes
    /// to the display it was saved from when that one is on and fits, else to the fitting screen with the most
    /// players. Returns the layout's name when something was applied.
    pub fn apply_pin(&self, recipe: &str, screens: &mut [Screen]) -> Option<String> {
        let layout = self.get(self.pins.get(recipe)?)?;
        let fitting = |s: &Screen| s.on && layout.fits(s.seats.len());
        let target = screens
            .iter()
            .position(|s| s.display == layout.display && fitting(s))
            .or_else(|| screens.iter().enumerate().filter(|(_, s)| fitting(s)).max_by_key(|(i, s)| (s.seats.len(), std::cmp::Reverse(*i))).map(|(i, _)| i))?;
        layout.apply(&mut screens[target]).then(|| layout.name.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(display: &str, seats: &[u8], regions: &[[f64; 4]]) -> Screen {
        Screen { display: display.into(), on: true, seats: seats.to_vec(), regions: regions.to_vec(), shape: None, custom: false, sizes: None, present: Present::Window }
    }
    const HALVES: [[f64; 4]; 2] = [[0.0, 0.0, 0.5, 1.0], [0.5, 0.0, 0.5, 1.0]];
    const GAPPY: [[f64; 4]; 2] = [[0.0, 0.0, 0.62, 1.0], [0.62, 0.0, 0.38, 0.55]];

    #[test]
    fn saving_keeps_the_screen_as_it_is() {
        let mut s = screen("tv", &[2, 3], &GAPPY);
        s.custom = true;
        s.present = Present::Virtual;
        let mut all = Layouts::default();
        let l = all.save("  Stream  ", &s, 100).unwrap();
        assert_eq!((l.id.as_str(), l.name.as_str(), l.display.as_str(), l.saved), ("l1", "Stream", "tv", 100));
        assert_eq!(l.regions, GAPPY.to_vec());
        assert!(l.custom);
        assert_eq!(l.present, Present::Virtual);
        assert_eq!(all.save("two", &screen("tv", &[0, 1], &HALVES), 5).unwrap().id, "l2");
    }

    #[test]
    fn bad_saves_are_refused() {
        let mut all = Layouts::default();
        let s = screen("tv", &[0, 1], &HALVES);
        assert!(all.save("   ", &s, 0).is_err());
        assert!(all.save(&"x".repeat(41), &s, 0).is_err());
        assert!(all.save("a\nb", &s, 0).is_err());
        assert!(all.save("empty", &screen("tv", &[], &[]), 0).is_err());
        assert!(all.save("clash", &screen("tv", &[0, 1], &[[0.0, 0.0, 0.6, 1.0], [0.5, 0.0, 0.5, 1.0]]), 0).unwrap_err().contains("overlap"));
        all.save("Halves", &s, 0).unwrap();
        assert!(all.save("halves", &s, 0).unwrap_err().contains("exists already"), "names compare without case");
        assert_eq!(all.layouts.len(), 1);
    }

    #[test]
    fn rename_and_delete() {
        let mut all = Layouts::default();
        let a = all.save("A", &screen("tv", &[0, 1], &HALVES), 0).unwrap().id;
        let b = all.save("B", &screen("tv", &[0, 1], &GAPPY), 0).unwrap().id;
        all.rename(&a, "Couch").unwrap();
        assert_eq!(all.get(&a).unwrap().name, "Couch");
        assert!(all.rename(&a, "b").is_err(), "taken by another layout");
        all.rename(&a, "COUCH").unwrap();
        assert!(all.rename("nope", "X").is_err());
        all.pin("sworn", Some(&b)).unwrap();
        all.delete(&b).unwrap();
        assert!(all.pins.is_empty(), "a deleted layout is unpinned");
        assert!(all.delete(&b).is_err());
        // Ids are not reused while a later one exists.
        assert_eq!(all.save("C", &screen("tv", &[0, 1], &HALVES), 0).unwrap().id, "l2");
    }

    #[test]
    fn applying_keeps_players_and_gaps() {
        let mut all = Layouts::default();
        let mut s = screen("tv", &[0, 1], &GAPPY);
        s.custom = true;
        let l = all.save("Gappy", &s, 0).unwrap();
        let mut target = screen("mon", &[3, 1], &HALVES);
        assert!(l.apply(&mut target));
        assert_eq!(target.seats, vec![3, 1]);
        assert_eq!(target.regions, GAPPY.to_vec());
        assert!(target.custom);
        // A custom layout with spare regions serves fewer players; a count that cannot fit is refused.
        let mut one = screen("mon", &[2], &[[0.0, 0.0, 1.0, 1.0]]);
        assert!(l.apply(&mut one));
        assert_eq!(one.regions, vec![GAPPY[0]]);
        assert!(!l.apply(&mut screen("mon", &[0, 1, 2], &[[0.0; 4]; 3])));
        let mut shaped = screen("tv", &[0, 1], &HALVES);
        shaped.shape = Some("lr".into());
        let lr = all.save("Side", &shaped, 0).unwrap();
        assert!(!lr.apply(&mut screen("mon", &[0], &[[0.0, 0.0, 1.0, 1.0]])), "a shape layout needs its player count");
    }

    #[test]
    fn pins_apply_to_the_right_screen() {
        let mut all = Layouts::default();
        let mut s = screen("tv", &[0, 1], &GAPPY);
        s.custom = true;
        let id = all.save("Gappy", &s, 0).unwrap().id;
        assert!(all.pin("g", Some("nope")).is_err());
        all.pin("g", Some(&id)).unwrap();
        let mut screens = vec![screen("mon", &[0, 1], &HALVES), screen("tv", &[2, 3], &HALVES)];
        assert_eq!(all.apply_pin("g", &mut screens).as_deref(), Some("Gappy"));
        assert_eq!(screens[1].regions, GAPPY.to_vec(), "the display it was saved from");
        assert_eq!(screens[0].regions, HALVES.to_vec());
        // That display is off now: the other fitting screen takes it.
        let mut screens = vec![screen("mon", &[0, 1], &HALVES), Screen { on: false, ..screen("tv", &[], &[]) }];
        assert_eq!(all.apply_pin("g", &mut screens).as_deref(), Some("Gappy"));
        assert_eq!(screens[0].regions, GAPPY.to_vec());
        // Nothing fits, or the game has no pin: nothing changes.
        let mut screens = vec![screen("mon", &[0, 1, 2], &[[0.0; 4]; 3])];
        assert_eq!(all.apply_pin("g", &mut screens), None);
        assert_eq!(all.apply_pin("other", &mut screens), None);
        all.pin("g", None).unwrap();
        assert!(all.pins.is_empty());
    }

    #[test]
    fn json_round_trip() {
        let mut all = Layouts::default();
        let id = all.save("Halves", &screen("tv", &[0, 1], &HALVES), 7).unwrap().id;
        all.pin("g", Some(&id)).unwrap();
        let back: Layouts = serde_json::from_str(&serde_json::to_string(&all).unwrap()).unwrap();
        assert_eq!(back, all);
        assert_eq!(serde_json::from_str::<Layouts>("{}").unwrap(), Layouts::default());
    }
}
