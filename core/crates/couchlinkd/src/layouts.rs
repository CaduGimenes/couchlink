//! `layouts.json` in the state folder: the saved screen layouts and the layout each game starts with.

use couchlink_core::layouts::Layouts;
use couchlink_core::paths;
use std::path::{Path, PathBuf};

fn file() -> PathBuf {
    paths::state_dir().join("layouts.json")
}

/// The saved layouts, or none when the file is missing or broken.
pub fn load() -> Layouts {
    load_from(&file())
}

pub fn save(all: &Layouts) {
    save_to(&file(), all);
}

fn load_from(path: &Path) -> Layouts {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save_to(path: &Path, all: &Layouts) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(all) {
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use couchlink_core::screens::{Present, Screen};

    #[test]
    fn saved_layouts_and_pins_survive_a_restart() {
        let dir = std::env::temp_dir().join(format!("couchlink-layouts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("layouts.json");
        assert_eq!(load_from(&path), Layouts::default(), "no file yet");
        let screen = Screen { display: "tv".into(), on: true, seats: vec![0, 1], regions: vec![[0.0, 0.0, 0.5, 1.0], [0.5, 0.0, 0.5, 1.0]], shape: Some("lr".into()), custom: false, sizes: None, present: Present::Window };
        let mut all = Layouts::default();
        let id = all.save("Couch", &screen, 42).unwrap().id;
        all.pin("sworn", Some(&id)).unwrap();
        save_to(&path, &all);
        let back = load_from(&path);
        assert_eq!(back, all);
        assert_eq!(back.pins.get("sworn"), Some(&id));
        std::fs::write(&path, "{ nope").unwrap();
        assert_eq!(load_from(&path), Layouts::default(), "a broken file reads as empty");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
