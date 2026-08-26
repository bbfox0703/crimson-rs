//! `meta/0.paver` — game-install version stamp (10 bytes).
//!
//! Each Crimson Desert install carries a fixed-layout
//! `meta/0.paver` file that Pearl Abyss writes at install / patch
//! time. It is the authoritative source for "what game-data version
//! is this install?" — the question downstream parsers need to
//! answer before they attempt to load `iteminfo.pabgb`, `skill.pabgb`,
//! or anything else whose schema drifts across patches.
//!
//! ## On-disk layout (little-endian)
//!
//! | Offset | Size | Field   | Notes                                                    |
//! |-------:|-----:|---------|----------------------------------------------------------|
//! |    0   |   2  | `major` | u16 — `1` through 1.18; Crimson Desert 2.00 bumps it to `2`. |
//! |    2   |   2  | `minor` | u16 — the **schema-compatibility key**, and it *resets* on a major bump (1.18 → 2.00 goes `18` → `0`), so it only identifies a schema together with `major`. |
//! |    4   |   2  | `patch` | u16 — sub-version (`1.08` → 0, `1.08.01` → 1).           |
//! |    6   |   4  | `build` | u32 LE — opaque build id; bumps every PA hotfix.         |
//!
//! Verified hex dumps:
//!
//! ```text
//! 1.06.xx: 01 00 06 00 ...
//! 1.07.xx: 01 00 07 00 ...
//! 1.08.00: 01 00 08 00 00 00 3e b0 39 dc   (build = 0xdc39b03e)
//! 1.18.00: 01 00 12 00 00 00 0f 7c 57 28   (build = 0x28577c0f)
//! 2.00.00: 02 00 00 00 00 00 14 d2 41 b2   (build = 0xb241d214)
//! ```
//!
//! No HMAC / checksum protection on this file — it's plain bytes the
//! game writes once per install.

use std::io::{self, Write};
use std::path::Path;

use crate::binary::{BinaryRead, BinaryWrite};

/// On-disk size of `meta/0.paver`.
pub const PAVER_SIZE: usize = 10;

/// The Crimson Desert gamedata `major` version this build's parsers target.
///
/// `1` for every patch through 1.18; Crimson Desert 2.00 is the first major
/// bump. It matters because [`PARSER_TARGET_GAMEDATA_MINOR`] *resets* across
/// one (1.18 → 2.00 is minor `18` → `0`), so a minor-only compatibility check
/// can no longer tell 2.00 apart from a hypothetical 1.00. Consumers read it
/// through the `crimson_parser_target_gamedata_major()` C ABI.
pub const PARSER_TARGET_GAMEDATA_MAJOR: u16 = 2;

/// The Crimson Desert gamedata `minor` version this build's parsers
/// (iteminfo / skill / save body + the gamedata bridges) target.
///
/// **Single source of truth** for "which game patch does this build support",
/// together with [`PARSER_TARGET_GAMEDATA_MAJOR`] — read them as the pair
/// `(major, minor)`, since the minor resets on a major bump.
/// Bump it (and [`COMPATIBLE_GAMEDATA_MINORS`]) when the parsers are validated
/// against a new patch — that one edit is what every downstream consumer keys
/// off. Consumers read it through the
/// `crimson_parser_target_gamedata_minor()` C ABI instead of duplicating the
/// number; before that bridge existed this had to be hand-bumped in lock-step
/// on the C# side every patch (8 → 9 → … → 16 → 17 → 18 → 2.00's 0).
pub const PARSER_TARGET_GAMEDATA_MINOR: u16 = 0;

/// Every gamedata `minor` this build's parsers can load without mis-decoding.
///
/// Always includes [`PARSER_TARGET_GAMEDATA_MINOR`]. Kept a single-element
/// allow-list tracking just the target. The last *structural* change was 2.00,
/// which widened `SubItem` to tag 18 and inserted a `u32` between
/// `respawn_time_seconds` and `max_endurance` — so 1.18 and earlier are no
/// longer byte-compatible (1.18 had already broken compatibility with 1.17 via
/// the `MergedPrefabVisualData` u32, and 1.16 with 1.15 via four iteminfo
/// drifts plus the first-ever skill drift).
/// Widen the list when a patch ships data an existing parser still reads
/// byte-perfectly (a content-only patch, e.g. 1.06→1.07, 1.08→1.09, 1.16→1.17).
/// Entries are `minor`s *within* [`PARSER_TARGET_GAMEDATA_MAJOR`] — a minor
/// from a different major says nothing about compatibility.
pub const COMPATIBLE_GAMEDATA_MINORS: &[u16] = &[PARSER_TARGET_GAMEDATA_MINOR];

/// Parsed version stamp from `meta/0.paver`.
///
/// `(major, minor)` identifies the parser-compatible schema: callers
/// that target a single `minor` (e.g. the parsers, currently
/// [`PARSER_TARGET_GAMEDATA_MINOR`]) should refuse / warn on mismatched values
/// rather than risk a parse crash. `patch` and `build` change within a
/// compatible minor and are informational only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Paver {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
    pub build: u32,
}

impl Paver {
    /// Parse from a byte slice. Returns `Err(UnexpectedEof)` when fewer
    /// than `PAVER_SIZE` bytes are available; extra bytes past offset 10
    /// are tolerated (the file size is exactly 10 bytes on every install
    /// observed to date, but tolerating trailing bytes future-proofs
    /// against PA adding fields).
    pub fn from_bytes(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() < PAVER_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!(
                    "paver requires at least {PAVER_SIZE} bytes, got {}",
                    bytes.len()
                ),
            ));
        }
        let mut offset = 0;
        let major = u16::read_from(bytes, &mut offset)?;
        let minor = u16::read_from(bytes, &mut offset)?;
        let patch = u16::read_from(bytes, &mut offset)?;
        let build = u32::read_from(bytes, &mut offset)?;
        Ok(Self { major, minor, patch, build })
    }

    /// Read `meta/0.paver` from disk. The caller passes the full path
    /// to the file; if a directory is more convenient on the consumer
    /// side, the C ABI auto-appends `meta/0.paver` when the caller
    /// passes a game-install root.
    pub fn from_file<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let bytes = std::fs::read(path)?;
        Self::from_bytes(&bytes)
    }
}

impl<'a> BinaryRead<'a> for Paver {
    fn read_from(data: &'a [u8], offset: &mut usize) -> io::Result<Self> {
        let major = u16::read_from(data, offset)?;
        let minor = u16::read_from(data, offset)?;
        let patch = u16::read_from(data, offset)?;
        let build = u32::read_from(data, offset)?;
        Ok(Self { major, minor, patch, build })
    }
}

impl BinaryWrite for Paver {
    fn write_to(&self, w: &mut dyn Write) -> io::Result<()> {
        self.major.write_to(w)?;
        self.minor.write_to(w)?;
        self.patch.write_to(w)?;
        self.build.write_to(w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Live-install hex dump (1.08.00, build 0xdc39b03e).
    const PAVER_1_08_LIVE: [u8; PAVER_SIZE] =
        [0x01, 0x00, 0x08, 0x00, 0x00, 0x00, 0x3e, 0xb0, 0x39, 0xdc];

    #[test]
    fn parse_1_08_live_bytes() {
        let p = Paver::from_bytes(&PAVER_1_08_LIVE).unwrap();
        assert_eq!(p.major, 1);
        assert_eq!(p.minor, 8);
        assert_eq!(p.patch, 0);
        assert_eq!(p.build, 0xdc39b03e);
    }

    #[test]
    fn parse_legacy_1_06_layout() {
        // Doc'd pattern: 01 00 06 00 ... (trailing 6 bytes unknown for
        // 1.06 at time of writing; we only assert the version triple).
        let bytes = [0x01, 0x00, 0x06, 0x00, 0x05, 0x00, 0xaa, 0xbb, 0xcc, 0xdd];
        let p = Paver::from_bytes(&bytes).unwrap();
        assert_eq!(p.major, 1);
        assert_eq!(p.minor, 6);
        assert_eq!(p.patch, 5);
        assert_eq!(p.build, 0xddccbbaa);
    }

    #[test]
    fn parse_short_bytes_errors() {
        let bytes = [0x01, 0x00, 0x08, 0x00];
        assert!(Paver::from_bytes(&bytes).is_err());
    }

    #[test]
    fn parse_extra_bytes_tolerated() {
        // PAVER_SIZE + 6 extra; should still parse cleanly using just
        // the leading 10 bytes.
        let mut bytes = PAVER_1_08_LIVE.to_vec();
        bytes.extend_from_slice(b"future");
        let p = Paver::from_bytes(&bytes).unwrap();
        assert_eq!(p.major, 1);
        assert_eq!(p.minor, 8);
    }

    #[test]
    fn round_trip_bytes() {
        let p = Paver::from_bytes(&PAVER_1_08_LIVE).unwrap();
        let mut out = Vec::new();
        p.write_to(&mut out).unwrap();
        assert_eq!(out, PAVER_1_08_LIVE);
    }
}
