//! Where the tests find the real save files they use as fixtures.
//!
//! The saves are real player data, so they live in the private
//! `crimson-rs-private` repo, cloned next to this checkout
//! (`../crimson-rs-private/saves`), or wherever `CRIMSON_FIXTURES_DIR` points
//! (the folder holding `1.09/`, `1.10/`, …).
//!
//! A test asks for one file and skips when it is missing: CI never has the
//! private repo, and neither does a clone that hasn't fetched it.
//!
//! Test-only.

use std::path::PathBuf;

/// The saves folder: `CRIMSON_FIXTURES_DIR`, else `../crimson-rs-private/saves`.
pub(crate) fn saves_dir() -> PathBuf {
    match std::env::var_os("CRIMSON_FIXTURES_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("crimson-rs-private")
            .join("saves"),
    }
}

/// The bytes of fixture `rel` (e.g. `"1.10/save.save"`), or `None` with a
/// note on stderr when it is not there. A file that does not start with
/// `SAVE` counts as missing too.
pub(crate) fn save(rel: &str) -> Option<Vec<u8>> {
    let p = saves_dir().join(rel);
    match std::fs::read(&p) {
        Ok(data) if data.get(..4) == Some(b"SAVE") => Some(data),
        Ok(_) => {
            eprintln!("skipping fixture {}: not a save file", p.display());
            None
        }
        Err(_) => {
            eprintln!(
                "skipping fixture {}: not found (clone crimson-rs-private next to this repo, or set CRIMSON_FIXTURES_DIR)",
                p.display()
            );
            None
        }
    }
}

/// The game-data fixtures folder, `<private repo>/gamedata`: decrypted
/// static-info tables of older versions (see its README), so a test does not
/// depend on a local version archive. It sits next to `saves/`, so
/// `CRIMSON_FIXTURES_DIR` moves both.
pub(crate) fn gamedata_dir() -> PathBuf {
    let saves = saves_dir();
    saves.parent().map(|p| p.join("gamedata")).unwrap_or_else(|| saves.join("..").join("gamedata"))
}

/// The bytes of game-data fixture `rel` (e.g. `"2.04/skill.staticinfobody"`),
/// or `None` with a note on stderr when it is not there.
pub(crate) fn gamedata(rel: &str) -> Option<Vec<u8>> {
    let p = gamedata_dir().join(rel);
    match std::fs::read(&p) {
        Ok(data) => Some(data),
        Err(_) => {
            eprintln!(
                "skipping fixture {}: not found (clone crimson-rs-private next to this repo, or set CRIMSON_FIXTURES_DIR)",
                p.display()
            );
            None
        }
    }
}

/// The most recently written live save,
/// `%LOCALAPPDATA%\Pearl Abyss\CD\save\<account>\slot<N>\save.save`, or `None`.
/// Slot numbers differ per machine, and a fixed slot can hold an old patch's
/// save, so the newest file stands for the current save format.
pub(crate) fn newest_live_save() -> Option<PathBuf> {
    let root = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Pearl Abyss").join("CD").join("save");
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for account in std::fs::read_dir(&root).ok()?.flatten() {
        let Ok(slots) = std::fs::read_dir(account.path()) else {
            continue;
        };
        for slot in slots.flatten() {
            let p = slot.path().join("save.save");
            let Ok(modified) = std::fs::metadata(&p).and_then(|m| m.modified()) else {
                continue;
            };
            if newest.as_ref().is_none_or(|(t, _)| modified > *t) {
                newest = Some((modified, p));
            }
        }
    }
    newest.map(|(_, p)| p)
}
