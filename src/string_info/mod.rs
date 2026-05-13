//! `stringinfo.pabgb` + `stringinfo.pabgh` parser for Crimson Desert.
//!
//! Resolves `StringInfoKey` (u32 hash) values referenced from
//! `iteminfo.pabgb` and other PABGB tables — notably `icon_path` /
//! `map_icon_path`, where the resolved string is a texture filename like
//! `cd_icon_arrow_basic.dds` consumed by the icon-extraction pipeline.
//!
//! Layout (`stringinfo.pabgb`)
//! --------------------------
//!
//! Linear, self-describing. Each record:
//!
//! ```text
//! [u32 hash] [u32 reserved_zero] [u8 reserved_flag] [u32 slen] [N bytes utf-8]
//! ```
//!
//! `reserved_zero` and `reserved_flag` are always 0 in 1.06; carried for
//! byte-identical round-trip in case a future patch promotes either to
//! a real field. Walking the file front to back consumes every byte —
//! no padding between entries.
//!
//! Layout (`stringinfo.pabgh`)
//! --------------------------
//!
//! Flat index, parsed by [`pabgh`]. See its module docs.
//!
//! Cross-version stability
//! -----------------------
//!
//! Both halves are pure data tables — fields and byte layout are
//! unchanged from 1.05 to 1.06. A schema drift in a future patch will
//! surface as either a size mismatch in `pabgh` (header count vs file
//! size) or a non-zero `reserved_zero` / `reserved_flag` in `pabgb`;
//! the latter currently parses (we keep the bytes) but the round-trip
//! test below will start exercising the non-zero path.

// Most of this module is public API + round-trip helpers exercised only
// by `#[cfg(test)]` callers (the live integration tests below). The
// `c_abi` bridge consumes a strict subset (`parse_pabgb`). Match the
// `item_info` precedent and silence dead-code warnings file-wide.
#![allow(dead_code)]

pub mod pabgh;

use std::collections::HashMap;
use std::io::{self, Write};

use crate::binary::{BinaryRead, BinaryWrite};

pub use pabgh::{CountWidth, StringIndexEntry, StringInfoIndex, parse_pabgh, write_pabgh};

/// One entry from `stringinfo.pabgb`. Keeps both the decoded UTF-8
/// string and the original raw bytes so round-trip is byte-identical
/// even for entries that ship invalid UTF-8 (none observed in 1.06 but
/// the contract should match `CString` in `binary::types`).
#[derive(Debug, Clone)]
pub struct StringInfoEntry {
    pub hash: u32,
    /// Reserved 4-byte field after the hash. Always 0 in 1.06 — kept
    /// for round-trip.
    pub reserved_zero: u32,
    /// Reserved 1-byte field after the zero field. Always 0 in 1.06 —
    /// kept for round-trip.
    pub reserved_flag: u8,
    /// UTF-8 (lossy) view of the payload. Use [`Self::value_bytes`]
    /// when you need the original bytes for round-trip.
    pub value: String,
    /// Original payload bytes. Equals `value.as_bytes()` for the
    /// well-formed case; differs when the source wasn't valid UTF-8
    /// (we preserve the bytes for write).
    raw: Option<Vec<u8>>,
}

impl StringInfoEntry {
    /// Original payload bytes (UTF-8 for well-formed entries).
    pub fn value_bytes(&self) -> &[u8] {
        self.raw.as_deref().unwrap_or(self.value.as_bytes())
    }
}

impl PartialEq for StringInfoEntry {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
            && self.reserved_zero == other.reserved_zero
            && self.reserved_flag == other.reserved_flag
            && self.value_bytes() == other.value_bytes()
    }
}

/// Parsed `stringinfo.pabgb` + `stringinfo.pabgh` pair.
#[derive(Debug, Clone)]
pub struct StringInfoData {
    /// Entries in PABGB file order (matches PABGH on-disk order when
    /// parsed via [`Self::parse_pair`]).
    pub entries: Vec<StringInfoEntry>,
    /// On-disk PABGH order plus its count-prefix width. `None` when
    /// the data was parsed via [`Self::parse_pabgb`] alone — round-
    /// tripping the pabgh side then requires the caller to supply the
    /// original index.
    pub index_order: Option<StringInfoIndex>,
}

impl StringInfoData {
    /// Walk the pabgb body. Each record is self-describing (the slen
    /// prefix encodes its full length) so the index isn't required for
    /// parsing — only for verifying offsets match and for byte-identical
    /// round-trip of the pabgh side.
    pub fn parse_pabgb(pabgb: &[u8]) -> io::Result<Vec<StringInfoEntry>> {
        let mut offset = 0usize;
        let mut entries = Vec::new();
        while offset < pabgb.len() {
            let entry_start = offset;
            let entry = read_entry(pabgb, &mut offset).map_err(|e| {
                io::Error::new(
                    e.kind(),
                    format!("stringinfo.pabgb entry at offset 0x{entry_start:X}: {e}"),
                )
            })?;
            entries.push(entry);
        }
        if offset != pabgb.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "stringinfo.pabgb parse ended at 0x{:X} but file is 0x{:X} bytes",
                    offset,
                    pabgb.len()
                ),
            ));
        }
        Ok(entries)
    }

    /// Parse both files and verify each pabgh entry's offset/hash matches
    /// the corresponding pabgb entry. Returns entries in pabgh on-disk
    /// order (which is also pabgb file order — verified at parse time).
    pub fn parse_pair(pabgb: &[u8], pabgh: &[u8]) -> io::Result<Self> {
        let index = parse_pabgh(pabgh)?;

        // Walk pabgb sequentially, recording (offset_at_start, hash) for
        // every entry. Compare against the index in pabgh on-disk order.
        let mut offset = 0usize;
        let mut entries: Vec<StringInfoEntry> = Vec::with_capacity(index.entries.len());
        let mut starts: Vec<u32> = Vec::with_capacity(index.entries.len());
        while offset < pabgb.len() {
            let entry_start = offset;
            let entry = read_entry(pabgb, &mut offset).map_err(|e| {
                io::Error::new(
                    e.kind(),
                    format!("stringinfo.pabgb entry at offset 0x{entry_start:X}: {e}"),
                )
            })?;
            starts.push(entry_start as u32);
            entries.push(entry);
        }
        if offset != pabgb.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "stringinfo.pabgb parse ended at 0x{:X} but file is 0x{:X} bytes",
                    offset,
                    pabgb.len()
                ),
            ));
        }
        if entries.len() != index.entries.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "stringinfo: pabgb has {} entries but pabgh declares {}",
                    entries.len(),
                    index.entries.len()
                ),
            ));
        }
        for (i, (idx, (pabgb_off, entry))) in index
            .entries
            .iter()
            .zip(starts.iter().zip(entries.iter()))
            .enumerate()
        {
            if idx.offset != *pabgb_off {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "stringinfo: pabgh entry {i} offset 0x{:X} != pabgb offset 0x{:X}",
                        idx.offset, *pabgb_off
                    ),
                ));
            }
            if idx.hash != entry.hash {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "stringinfo: pabgh entry {i} hash 0x{:08X} != pabgb hash 0x{:08X}",
                        idx.hash, entry.hash
                    ),
                ));
            }
        }
        Ok(StringInfoData { entries, index_order: Some(index) })
    }

    /// Serialise the pabgb body back to bytes. Linear walk of `entries`;
    /// no re-ordering. Pair with [`Self::write_pabgh`] for the index.
    pub fn write_pabgb(&self) -> io::Result<Vec<u8>> {
        let mut buf = Vec::new();
        for entry in &self.entries {
            write_entry(entry, &mut buf)?;
        }
        Ok(buf)
    }

    /// Serialise the pabgh index back to bytes. Uses the cached
    /// `index_order` when available; otherwise re-derives offsets by
    /// walking the entries. The auto-derived path always emits a `u16`
    /// count prefix (matches every observed game build).
    pub fn write_pabgh(&self) -> io::Result<Vec<u8>> {
        let index = match &self.index_order {
            Some(idx) => idx.clone(),
            None => {
                let mut offset = 0u32;
                let mut entries = Vec::with_capacity(self.entries.len());
                for e in &self.entries {
                    entries.push(StringIndexEntry { hash: e.hash, offset });
                    let len = entry_size(e)?;
                    offset = offset.checked_add(len).ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "stringinfo: pabgb size overflows u32",
                        )
                    })?;
                }
                StringInfoIndex { entries, count_width: CountWidth::U16 }
            }
        };
        write_pabgh(&index)
    }

    /// Map hashes to their resolved string values. Drops the reserved
    /// bytes — round-trip callers should hold the `Vec<StringInfoEntry>`
    /// instead.
    pub fn into_lookup_map(self) -> HashMap<u32, String> {
        self.entries.into_iter().map(|e| (e.hash, e.value)).collect()
    }
}

// ── Per-entry codec ──────────────────────────────────────────────────────

fn read_entry(data: &[u8], offset: &mut usize) -> io::Result<StringInfoEntry> {
    let hash = u32::read_from(data, offset)?;
    let reserved_zero = u32::read_from(data, offset)?;
    let reserved_flag = u8::read_from(data, offset)?;
    let slen = u32::read_from(data, offset)? as usize;
    if *offset + slen > data.len() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            format!(
                "stringinfo entry hash=0x{hash:08X}: slen={slen} overruns file (offset 0x{:X}, file 0x{:X})",
                *offset,
                data.len()
            ),
        ));
    }
    let bytes = &data[*offset..*offset + slen];
    *offset += slen;

    let (value, raw) = match std::str::from_utf8(bytes) {
        Ok(s) => (s.to_owned(), None),
        Err(_) => (
            String::from_utf8_lossy(bytes).into_owned(),
            Some(bytes.to_vec()),
        ),
    };
    Ok(StringInfoEntry {
        hash,
        reserved_zero,
        reserved_flag,
        value,
        raw,
    })
}

fn write_entry(entry: &StringInfoEntry, w: &mut Vec<u8>) -> io::Result<()> {
    entry.hash.write_to(w)?;
    entry.reserved_zero.write_to(w)?;
    entry.reserved_flag.write_to(w)?;
    let bytes = entry.value_bytes();
    let slen = u32::try_from(bytes.len()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "stringinfo entry hash=0x{:08X}: value len {} doesn't fit in u32",
                entry.hash,
                bytes.len()
            ),
        )
    })?;
    slen.write_to(w)?;
    w.write_all(bytes)?;
    Ok(())
}

fn entry_size(entry: &StringInfoEntry) -> io::Result<u32> {
    let bytes = entry.value_bytes();
    let slen = u32::try_from(bytes.len()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "stringinfo entry hash=0x{:08X}: value len {} doesn't fit in u32",
                entry.hash,
                bytes.len()
            ),
        )
    })?;
    // 4 (hash) + 4 (reserved_zero) + 1 (reserved_flag) + 4 (slen) + slen
    Ok(13u32 + slen)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(hash: u32, value: &str) -> StringInfoEntry {
        StringInfoEntry {
            hash,
            reserved_zero: 0,
            reserved_flag: 0,
            value: value.to_owned(),
            raw: None,
        }
    }

    #[test]
    fn pabgb_roundtrip_synthetic() {
        let entries = vec![
            make_entry(0x2ad9f89e, "RealWorld"),
            make_entry(0x04f6d06d, "ChildWild"),
            make_entry(0xdeadbeef, ""),
        ];
        let data = StringInfoData {
            entries: entries.clone(),
            index_order: None,
        };
        let pabgb_bytes = data.write_pabgb().unwrap();

        // Hash 0x2ad9f89e + zero4 + flag + slen=9 + "RealWorld" = 22 bytes
        // Hash 0x04f6d06d + zero4 + flag + slen=9 + "ChildWild" = 22 bytes
        // Hash 0xdeadbeef + zero4 + flag + slen=0 + ""          = 13 bytes
        assert_eq!(pabgb_bytes.len(), 22 + 22 + 13);

        let parsed = StringInfoData::parse_pabgb(&pabgb_bytes).unwrap();
        assert_eq!(parsed, entries);
    }

    #[test]
    fn pair_roundtrip_synthetic() {
        let mut entries = vec![
            make_entry(0x11111111, "alpha"),
            make_entry(0x22222222, "beta"),
            make_entry(0x33333333, "gamma"),
        ];
        // Build the matching pabgh by walking the entries.
        let mut offset = 0u32;
        let mut index_entries = Vec::new();
        for e in &entries {
            index_entries.push(StringIndexEntry { hash: e.hash, offset });
            offset += entry_size(e).unwrap();
        }
        let index = StringInfoIndex {
            entries: index_entries,
            count_width: CountWidth::U16,
        };
        // Synthesize bytes through write_*, then re-parse.
        let pabgb_bytes = {
            let data = StringInfoData {
                entries: entries.clone(),
                index_order: Some(index.clone()),
            };
            data.write_pabgb().unwrap()
        };
        let pabgh_bytes = write_pabgh(&index).unwrap();

        let parsed = StringInfoData::parse_pair(&pabgb_bytes, &pabgh_bytes).unwrap();
        assert_eq!(parsed.entries, entries);
        assert_eq!(
            parsed
                .index_order
                .as_ref()
                .map(|i| i.entries.clone())
                .unwrap_or_default(),
            index.entries
        );

        // Mutate one entry's value; round-trip should still parse and the
        // pabgb bytes should change (the pabgh stays the same — offset
        // recomputation is the caller's job; we only verify the parser
        // tolerates a "stale" index when offsets still line up).
        entries[0].value = "alpha_v2".to_owned();
        let data2 = StringInfoData {
            entries: entries.clone(),
            index_order: None,
        };
        let pabgb2 = data2.write_pabgb().unwrap();
        let pabgh2 = data2.write_pabgh().unwrap();
        let parsed2 = StringInfoData::parse_pair(&pabgb2, &pabgh2).unwrap();
        assert_eq!(parsed2.entries[0].value, "alpha_v2");
    }

    #[test]
    fn pabgb_rejects_truncated_string() {
        // hash=1, zero4, flag=0, slen=10, but only 5 bytes of payload
        let mut bad = Vec::new();
        bad.extend_from_slice(&1u32.to_le_bytes());
        bad.extend_from_slice(&0u32.to_le_bytes());
        bad.push(0);
        bad.extend_from_slice(&10u32.to_le_bytes());
        bad.extend_from_slice(b"abcde");
        assert!(StringInfoData::parse_pabgb(&bad).is_err());
    }

    #[test]
    fn pair_rejects_hash_mismatch() {
        // pabgb has hash=1, pabgh declares hash=2 at the same offset.
        let entry = make_entry(1, "x");
        let data = StringInfoData {
            entries: vec![entry],
            index_order: None,
        };
        let pabgb = data.write_pabgb().unwrap();
        let bad_index = StringInfoIndex {
            entries: vec![StringIndexEntry { hash: 2, offset: 0 }],
            count_width: CountWidth::U16,
        };
        let pabgh = write_pabgh(&bad_index).unwrap();
        let err = StringInfoData::parse_pair(&pabgb, &pabgh).unwrap_err();
        assert!(err.to_string().contains("hash"), "got: {err}");
    }

    // ── Live-install integration test ────────────────────────────────────
    // Exercises the parser against the real 1.06 stringinfo files when
    // they're present in the maintainer's `out/` (extracted by Python
    // tooling). Skips on a fresh checkout — same pattern as `item_info`.
    const PABGB_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), r"\out\stringinfo.pabgb");
    const PABGH_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), r"\out\stringinfo.pabgh");

    #[test]
    fn live_pair_roundtrip() {
        let (Ok(pabgb), Ok(pabgh)) = (std::fs::read(PABGB_PATH), std::fs::read(PABGH_PATH)) else {
            eprintln!(
                "skipping live_pair_roundtrip: stringinfo files not present at \
                 out/stringinfo.pabg{{b,h}}"
            );
            return;
        };
        let parsed = StringInfoData::parse_pair(&pabgb, &pabgh).unwrap();

        // 1.06 ships ~30k entries; sanity-check the order of magnitude.
        assert!(
            parsed.entries.len() > 20_000,
            "expected >20k entries, got {}",
            parsed.entries.len()
        );

        // Byte-identical round-trip.
        let pabgb_out = parsed.write_pabgb().unwrap();
        let pabgh_out = parsed.write_pabgh().unwrap();
        assert_eq!(pabgb_out, pabgb, "pabgb round-trip differs");
        assert_eq!(pabgh_out, pabgh, "pabgh round-trip differs");

        // First entry's hash + value are what we observed during format
        // investigation. Pinned so future schema drift fails loudly.
        assert_eq!(parsed.entries[0].hash, 0x2ad9f89e);
        assert_eq!(parsed.entries[0].value, "RealWorld");
    }
}
