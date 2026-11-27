//! Where the tests find the real save files they use as fixtures.
//!
//! The saves are real player data, so they live in the private
//! `crimson-rs-private` repo, cloned next to this checkout
//! (`../crimson-rs-private/saves`), or wherever `CRIMSON_FIXTURES_DIR` points
//! (the folder holding `1.09/`, `1.10/`, …). They used to sit in
//! `tests/fixtures/saves/` under git-crypt, until 2026-11-27.
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
