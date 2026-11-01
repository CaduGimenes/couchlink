//! Windows games on Linux keep their profile folders (Documents, AppData, Saved Games) in a Proton prefix, not in the
//! home folder. For such a game a recipe's `$DOCUMENTS` and the other folder variables name the profile in the game's
//! own Steam prefix, where starting the game from Steam wrote its settings. Each seat plays in a prefix of its own
//! (`launch_linux.rs`), whose profile starts as a copy of that one; its save folders are used there as they are.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use couchlink_core::Recipe;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The profile folder inside a Proton prefix.
pub const USER: &str = "drive_c/users/steamuser";

/// What a seat's profile first gets from the Steam one: the folders games keep settings and saves in.
const PROFILE: [&str; 3] = ["Documents", "AppData", "Saved Games"];

/// Largest profile folder copied into a seat's prefix.
const SEED_LIMIT: u64 = 1 << 30;

/// Left in a seat's profile once it was filled, so a later start keeps what the seat changed.
const SEEDED: &str = ".couchlink-seeded";

/// The profile of `r`'s game in its Steam prefix: on Linux, for a Steam game that runs through Proton.
pub fn steam_profile(r: &Recipe) -> Option<PathBuf> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let id = r.matching.steam_app_id?;
    if !r.linux.clone().unwrap_or_default().uses_proton(&r.launch.exe) {
        return None;
    }
    let libs = couchlink_steam::steam_libraries();
    // An install is `<library>/steamapps/common/<game>`.
    let library = couchlink_steam::library::find_app_in(&libs, id).and_then(|a| a.install_dir.ancestors().nth(3).map(Path::to_path_buf));
    Some(compatdata(&libs, library.as_deref(), id)?.join("pfx").join(USER))
}

/// Steam's `steamapps/compatdata/<id>` for a game: the one that holds a prefix, else the one in the game's library.
pub fn compatdata(libs: &[PathBuf], library: Option<&Path>, id: u32) -> Option<PathBuf> {
    let of = |lib: &Path| lib.join("steamapps").join("compatdata").join(id.to_string());
    libs.iter().map(|l| of(l)).find(|d| d.join("pfx").is_dir()).or_else(|| library.map(of)).or_else(|| libs.first().map(|l| of(l)))
}

/// `$USERPROFILE`, `$DOCUMENTS`, `$APPDATA` and `$LOCALAPPDATA` for a Windows profile folder.
pub fn profile_vars(user: &Path) -> BTreeMap<String, String> {
    let s = |p: PathBuf| p.display().to_string();
    BTreeMap::from([
        ("USERPROFILE".to_string(), s(user.to_path_buf())),
        ("DOCUMENTS".to_string(), s(user.join("Documents"))),
        ("APPDATA".to_string(), s(user.join("AppData").join("Roaming"))),
        ("LOCALAPPDATA".to_string(), s(user.join("AppData").join("Local"))),
    ])
}

/// The profile folder in a seat's own prefix: umu-launcher makes the prefix at `<data>/prefix`, Steam's Proton at
/// `<data>/compat/pfx` (`launch_linux.rs`).
pub fn seat_profile(data_dir: &Path, umu: bool) -> PathBuf {
    let prefix = if umu { data_dir.join("prefix") } else { data_dir.join("compat").join("pfx") };
    prefix.join(USER)
}

/// The first time, fill a seat's profile with a copy of the Steam one, so the seat starts with the player's settings
/// and saves. Proton makes the rest of the prefix around it on the first start.
pub fn seed_profile(steam: &Path, seat: &Path) -> Result<(), String> {
    if seat.join(SEEDED).exists() {
        return Ok(());
    }
    std::fs::create_dir_all(seat).map_err(|e| format!("cannot create {}: {e}", seat.display()))?;
    for name in PROFILE {
        let from = steam.join(name);
        // Too much to copy for every seat: that folder starts empty and the game makes what it needs.
        if from.is_dir() && super::edits::tree_size(&from) <= SEED_LIMIT {
            copy_missing(&from, &seat.join(name))?;
        }
    }
    std::fs::write(seat.join(SEEDED), b"").map_err(|e| format!("cannot write {}: {e}", seat.join(SEEDED).display()))
}

/// Copy the files of `from` that `to` lacks. Links are left out: in a prefix they point at the home folder.
fn copy_missing(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("cannot create {}: {e}", to.display()))?;
    for e in std::fs::read_dir(from).map_err(|e| format!("cannot read {}: {e}", from.display()))?.flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        match e.file_type() {
            Ok(t) if t.is_dir() => copy_missing(&src, &dst)?,
            Ok(t) if t.is_file() && !dst.exists() => {
                std::fs::copy(&src, &dst).map_err(|e| format!("cannot copy {} to {}: {e}", src.display(), dst.display()))?;
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("couchlinkd-prefix-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_steam_prefix_is_the_one_that_exists_else_the_games_library() {
        let root = temp("compat");
        let (a, b) = (root.join("a"), root.join("b"));
        let libs = vec![a.clone(), b.clone()];
        assert_eq!(compatdata(&libs, Some(&b), 22380), Some(b.join("steamapps/compatdata/22380")), "none yet: the game's library");
        assert_eq!(compatdata(&libs, None, 22380), Some(a.join("steamapps/compatdata/22380")));
        std::fs::create_dir_all(b.join("steamapps/compatdata/22380/pfx")).unwrap();
        assert_eq!(compatdata(&libs, Some(&a), 22380), Some(b.join("steamapps/compatdata/22380")), "the one with a prefix wins");
        assert_eq!(compatdata(&[], None, 1), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn folder_variables_name_the_prefix_profile() {
        let v = profile_vars(Path::new("/lib/steamapps/compatdata/22380/pfx/drive_c/users/steamuser"));
        assert_eq!(Path::new(&v["DOCUMENTS"]), Path::new("/lib/steamapps/compatdata/22380/pfx/drive_c/users/steamuser/Documents"));
        assert_eq!(Path::new(&v["LOCALAPPDATA"]), Path::new("/lib/steamapps/compatdata/22380/pfx/drive_c/users/steamuser/AppData/Local"));
        assert_eq!(seat_profile(Path::new("/s/1"), true), Path::new("/s/1/prefix/drive_c/users/steamuser"));
        assert_eq!(seat_profile(Path::new("/s/1"), false), Path::new("/s/1/compat/pfx/drive_c/users/steamuser"));
    }

    #[test]
    fn a_seat_profile_starts_as_a_copy_once() {
        let root = temp("seed");
        let (steam, seat) = (root.join("steam"), root.join("seat"));
        std::fs::create_dir_all(steam.join("Documents/My Games/FalloutNV")).unwrap();
        std::fs::write(steam.join("Documents/My Games/FalloutNV/FalloutPrefs.ini"), "[Display]\n").unwrap();
        std::fs::create_dir_all(steam.join("Temp")).unwrap();
        std::fs::write(steam.join("Temp/x"), "x").unwrap();
        seed_profile(&steam, &seat).unwrap();
        assert_eq!(std::fs::read_to_string(seat.join("Documents/My Games/FalloutNV/FalloutPrefs.ini")).unwrap(), "[Display]\n");
        assert!(!seat.join("Temp").exists(), "only the profile folders");
        // The seat changes its copy; a later start keeps it.
        std::fs::write(seat.join("Documents/My Games/FalloutNV/FalloutPrefs.ini"), "seat").unwrap();
        std::fs::write(steam.join("Documents/My Games/FalloutNV/Fallout.ini"), "new").unwrap();
        seed_profile(&steam, &seat).unwrap();
        assert_eq!(std::fs::read_to_string(seat.join("Documents/My Games/FalloutNV/FalloutPrefs.ini")).unwrap(), "seat");
        assert!(!seat.join("Documents/My Games/FalloutNV/Fallout.ini").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
