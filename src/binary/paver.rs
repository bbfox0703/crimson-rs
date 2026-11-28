//! `meta/0.paver` — game-install version stamp (10 bytes through 2.03, longer
//! once an install ships DLC).
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
//! Example stamps from live installs:
//!
//! ```text
//! 1.06.xx: 01 00 06 00 ...
//! 1.07.xx: 01 00 07 00 ...
//! 1.08.00: 01 00 08 00 00 00 3e b0 39 dc   (build = 0xdc39b03e)
//! 1.18.00: 01 00 12 00 00 00 0f 7c 57 28   (build = 0x28577c0f)
//! 2.00.00: 02 00 00 00 00 00 14 d2 41 b2   (build = 0xb241d214)
//! ```
//!
//! 2.04 inserts a DLC list between `patch` and `build`, so `build` moves to
//! the end of the file:
//!
//! ```text
//! 2.04.03: 02 00 04 00 03 00
//!          01 00 00 00                       u32 DLC count
//!          06 00 00 00 "dlc_01" 02 00 00 00  u32 len, name, u32 revision
//!          e2 7e be 3e                       (build = 0x3ebe7ee2)
//! ```
//!
//! The per-DLC u32 is named by position only: it reads 2 on the one DLC seen.
//! An install without DLC has not been seen; a DLC count of 0 (a 14-byte
//! stamp) and the 10-byte stamp both read, and both write back as they came.
//!
//! No HMAC / checksum protection on this file — it's plain bytes the
//! game writes once per install.

use std::io::{self, Write};
use std::path::Path;

use crate::binary::{BinaryRead, BinaryWrite};

/// Smallest on-disk size of `meta/0.paver`, the whole file through 2.03.
pub const PAVER_SIZE: usize = 10;

/// The Crimson Desert gamedata `major` version this build's parsers target.
///
/// `1` for every patch through 1.18; Crimson Desert 2.00 is the first major
/// bump. It matters because [`PARSER_TARGET_GAMEDATA_MINOR`] *resets* across
/// one (1.18 → 2.00 is minor `18` → `0`), so a minor-only compatibility check
/// cannot tell 2.00 apart from a hypothetical 1.00. Consumers read it
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
/// number.
pub const PARSER_TARGET_GAMEDATA_MINOR: u16 = 6;

/// Every gamedata `minor` this build's parsers can load without mis-decoding.
///
/// Always includes [`PARSER_TARGET_GAMEDATA_MINOR`]. Kept a single-element
/// allow-list tracking just the target. The table parsers read both the 2.06
/// and the pre-2.06 row layout, but iteminfo's own row and the dye palette
/// record differ from minor 5, so minor 5 is out. Minor 4 stays out even
/// where a parser would still read it, because the patch floor below applies
/// to the target minor only, and listing 4 would let 2.04.03 through.
/// Widen the list when a patch ships data an existing parser still reads
/// byte-perfectly (a content-only patch).
/// Entries are `minor`s *within* [`PARSER_TARGET_GAMEDATA_MAJOR`] — a minor
/// from a different major says nothing about compatibility.
pub const COMPATIBLE_GAMEDATA_MINORS: &[u16] = &[PARSER_TARGET_GAMEDATA_MINOR];

/// The lowest gamedata `patch`, within [`PARSER_TARGET_GAMEDATA_MAJOR`].
/// [`PARSER_TARGET_GAMEDATA_MINOR`], that this build's parsers read.
///
/// A patch-level update can change a layout (2.04.08 added two `CString`s to
/// every iteminfo row, so 2.04.03 shares minor 4 with it and does not parse),
/// so `(major, minor)` alone can let an unreadable install through. A full
/// gate is `major == MAJOR && minor ∈ COMPATIBLE_GAMEDATA_MINORS &&
/// (minor != MINOR || patch >= this)`. Reset it to `0` when the target minor
/// moves. Consumers read it through `crimson_parser_min_gamedata_patch()`.
pub const PARSER_MIN_GAMEDATA_PATCH: u16 = 0;

/// The version gate the constants above describe: does this build's parser
/// read gamedata `major.minor.patch`? `major` must be the target, `minor` one
/// of [`COMPATIBLE_GAMEDATA_MINORS`], and on the target minor `patch` at least
/// [`PARSER_MIN_GAMEDATA_PATCH`].
// `patch >= 0` is always true while the floor is 0; the floor moves per patch.
#[allow(clippy::absurd_extreme_comparisons)]
pub fn parser_supports(major: u16, minor: u16, patch: u16) -> bool {
    major == PARSER_TARGET_GAMEDATA_MAJOR
        && COMPATIBLE_GAMEDATA_MINORS.contains(&minor)
        && (minor != PARSER_TARGET_GAMEDATA_MINOR || patch >= PARSER_MIN_GAMEDATA_PATCH)
}

/// Parsed version stamp from `meta/0.paver`.
///
/// `(major, minor)` identifies the parser-compatible schema: callers
/// that target a single `minor` (e.g. the parsers, currently
/// [`PARSER_TARGET_GAMEDATA_MINOR`]) should refuse / warn on mismatched values
/// rather than risk a parse crash. `build` is informational only; `patch`
/// matters because a patch-level update can change a layout (see
/// [`PARSER_MIN_GAMEDATA_PATCH`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Paver {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
    pub build: u32,
    /// The DLC list of a 2.04+ stamp, which may be empty; `None` on a 10-byte
    /// stamp. Nothing here requires a DLC to be present.
    pub dlcs: Option<Vec<PaverDlc>>,
}

/// One entry of the 2.04+ DLC list.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PaverDlc {
    pub name: String,
    pub revision: u32,
}

/// The DLC list and closing `build` of a 2.04+ stamp, starting at `offset`.
/// `None` when the rest of the file does not read exactly as one.
fn read_dlc_tail(bytes: &[u8], mut offset: usize) -> Option<(Vec<PaverDlc>, u32)> {
    let count = u32::read_from(bytes, &mut offset).ok()?;
    if count as usize > bytes.len() {
        return None;
    }
    let mut dlcs = Vec::new();
    for _ in 0..count {
        let len = u32::read_from(bytes, &mut offset).ok()? as usize;
        let name = bytes.get(offset..offset.checked_add(len)?)?;
        let name = std::str::from_utf8(name).ok()?.to_string();
        offset += len;
        let revision = u32::read_from(bytes, &mut offset).ok()?;
        dlcs.push(PaverDlc { name, revision });
    }
    let build = u32::read_from(bytes, &mut offset).ok()?;
    (offset == bytes.len()).then_some((dlcs, build))
}

impl Paver {
    /// Parse from a byte slice. Returns `Err(UnexpectedEof)` when fewer
    /// than `PAVER_SIZE` bytes are available. A longer file must read exactly
    /// as the 2.04+ DLC stamp; anything else is `InvalidData` rather than a
    /// 10-byte reading that would put the DLC count in `build` and drop the
    /// rest on write-back: a new layout is drift to look at, not something to
    /// half-read.
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
        // A 10-byte stamp never reads as a DLC list: the count would take
        // `build`'s place and leave nothing for `build` itself.
        if let Some((dlcs, build)) = read_dlc_tail(bytes, offset) {
            return Ok(Self { major, minor, patch, build, dlcs: Some(dlcs) });
        }
        if bytes.len() != PAVER_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unrecognised {}-byte paver layout (neither the 10-byte stamp nor a DLC list)", bytes.len()),
            ));
        }
        let build = u32::read_from(bytes, &mut offset)?;
        Ok(Self { major, minor, patch, build, dlcs: None })
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
    /// The stamp owns the whole slice: a 2.04+ one is variable-length.
    fn read_from(data: &'a [u8], offset: &mut usize) -> io::Result<Self> {
        let rest = data.get(*offset..).ok_or_else(|| {
            io::Error::new(io::ErrorKind::UnexpectedEof, "paver offset past the end")
        })?;
        let paver = Self::from_bytes(rest)?;
        *offset = data.len();
        Ok(paver)
    }
}

impl BinaryWrite for Paver {
    fn write_to(&self, w: &mut dyn Write) -> io::Result<()> {
        self.major.write_to(w)?;
        self.minor.write_to(w)?;
        self.patch.write_to(w)?;
        if let Some(dlcs) = &self.dlcs {
            (dlcs.len() as u32).write_to(w)?;
            for dlc in dlcs {
                (dlc.name.len() as u32).write_to(w)?;
                w.write_all(dlc.name.as_bytes())?;
                dlc.revision.write_to(w)?;
            }
        }
        self.build.write_to(w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_supports_follows_the_gate() {
        let (ma, mi, pf) = (PARSER_TARGET_GAMEDATA_MAJOR, PARSER_TARGET_GAMEDATA_MINOR, PARSER_MIN_GAMEDATA_PATCH);
        assert!(parser_supports(ma, mi, pf));
        assert!(parser_supports(ma, mi, pf + 7));
        if pf > 0 {
            assert!(!parser_supports(ma, mi, pf - 1));
        }
        assert!(!parser_supports(ma + 1, mi, pf), "another major");
        assert!(!parser_supports(ma, mi + 1, 0), "a minor not in the list");
    }

    /// A stamp longer than 10 bytes that is not a DLC list is an error, not a
    /// 10-byte reading with the DLC count in `build`.
    #[test]
    fn unrecognised_long_stamp_is_an_error() {
        let mut b = vec![2, 0, 6, 0, 0, 0];
        b.extend_from_slice(&5u32.to_le_bytes()); // a count of 5 with no entries
        b.extend_from_slice(&[0xAA; 3]);
        assert!(Paver::from_bytes(&b).is_err());
        let ten = [2, 0, 6, 0, 0, 0, 1, 0, 0, 0];
        assert_eq!(Paver::from_bytes(&ten).unwrap().build, 1);
    }

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
        // Doc'd pattern: 01 00 06 00 ...; the 1.06 sample's trailing 6 bytes
        // are not recorded, so only the version triple is asserted.
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
    fn parse_extra_bytes_is_an_error() {
        // PAVER_SIZE + 6 bytes that are not a DLC list: a layout nothing
        // reads is an error, not a parse of the leading 10 bytes.
        let mut bytes = PAVER_1_08_LIVE.to_vec();
        bytes.extend_from_slice(b"future");
        assert!(Paver::from_bytes(&bytes).is_err());
    }

    /// Live-install hex dump (2.04.03, build 0x3ebe7ee2).
    const PAVER_2_04_LIVE: [u8; 28] = [
        0x02, 0x00, 0x04, 0x00, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, b'd',
        b'l', b'c', b'_', b'0', b'1', 0x02, 0x00, 0x00, 0x00, 0xe2, 0x7e, 0xbe, 0x3e,
    ];

    #[test]
    fn parse_2_04_dlc_stamp() {
        let p = Paver::from_bytes(&PAVER_2_04_LIVE).unwrap();
        assert_eq!((p.major, p.minor, p.patch), (2, 4, 3));
        assert_eq!(p.build, 0x3ebe7ee2);
        assert_eq!(p.dlcs, Some(vec![PaverDlc { name: "dlc_01".into(), revision: 2 }]));

        let mut out = Vec::new();
        p.write_to(&mut out).unwrap();
        assert_eq!(out, PAVER_2_04_LIVE);
    }

    /// No DLC: the 2.04 shape with a count of 0 keeps its real `build`.
    #[test]
    fn parse_stamp_without_dlc() {
        let mut bytes = PAVER_2_04_LIVE[..6].to_vec();
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0x3ebe7ee2u32.to_le_bytes());
        let p = Paver::from_bytes(&bytes).unwrap();
        assert_eq!((p.major, p.minor, p.patch, p.build), (2, 4, 3, 0x3ebe7ee2));
        assert_eq!(p.dlcs, Some(Vec::new()));
        let mut out = Vec::new();
        p.write_to(&mut out).unwrap();
        assert_eq!(out, bytes);

        assert_eq!(Paver::from_bytes(&PAVER_1_08_LIVE).unwrap().dlcs, None);
    }

    #[test]
    fn round_trip_bytes() {
        let p = Paver::from_bytes(&PAVER_1_08_LIVE).unwrap();
        let mut out = Vec::new();
        p.write_to(&mut out).unwrap();
        assert_eq!(out, PAVER_1_08_LIVE);
    }
}
