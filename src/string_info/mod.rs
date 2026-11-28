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
//! [u32 reserved_zero] [u32 hash] [u8 reserved_flag] [u32 slen] [N bytes utf-8]
//! ```
//!
//! 2.06 moved every row name into the `.staticinfoheader`, which now lists
//! `[hash][u32 0][offset][size]` per row, and gave each row its index after
//! the key: `[u32 hash] [u32 row index] [u8 reserved_flag] [u32 slen] [N bytes]`.
//! [`RowLayout`] records which of the two a table uses; `reserved_zero` holds
//! the row index on a 2.06 table.
//!
//! `reserved_zero` is the row's empty name: 2.04 moved the key after the
//! name in every static-info table, which on a nameless row puts the zero
//! length first (it swapped with `hash`, nothing else moved). `reserved_zero`
//! and `reserved_flag` are 0 on all 33,888 rows of 2.04; carried for
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
//! Both halves are pure data tables. Apart from 2.04's swap above, fields
//! and byte layout are unchanged since 1.05: of the 30,313 hashes in both
//! 2.03 and 2.04, every row is byte-identical after undoing the swap. A
//! schema drift in a future patch will
//! surface as either a size mismatch in `pabgh` (header count vs file
//! size) or a non-zero `reserved_zero` / `reserved_flag` in `pabgb`;
//! the latter currently parses (we keep the bytes) but the round-trip
//! test below will start exercising the non-zero path.
// The writers and the full-table readers are round-trip support: the c_abi
// bridge uses only the lookup path, and the tests below exercise the rest.
// Outside tests they are dead, so only there is dead code allowed; the test
// build lints the module in full.
#![cfg_attr(not(test), allow(dead_code))]

pub mod pabgh;

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
    /// 2.04–2.05.01: the 4 bytes before the hash, the row's empty name length
    /// (always 0). 2.06+: the 4 bytes after the hash, the row's index.
    pub reserved_zero: u32,
    /// Reserved 1-byte field after the hash. Always 0 — kept for
    /// round-trip.
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
    /// Row shape: `[0][hash]…` through 2.05.01, `[hash][index]…` from 2.06.
    pub layout: RowLayout,
}

/// Where the hash sits in a stringinfo row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RowLayout {
    /// 2.04–2.05.01: `[u32 0][u32 hash][u8][u32 slen][bytes]`.
    #[default]
    Legacy,
    /// 2.06+: `[u32 hash][u32 row index][u8][u32 slen][bytes]`.
    V206,
}

impl RowLayout {
    /// A body's layout, from its leading rows. Both layouts have the same
    /// row lengths; a legacy row opens `[u32 0][u32 hash]`, a 2.06 row
    /// `[u32 hash][u32 row index]`. A legacy first word is always 0, so the
    /// body is 2.06 as soon as one of the first eight rows has a non-zero
    /// first word. The row index cannot decide it: in a DLC table it is not
    /// the row's position. Looking at the first row alone read a 2.06 table
    /// whose first hash is 0 as legacy, swapping every hash with its index.
    pub fn detect(pabgb: &[u8]) -> Self {
        let word = |at: usize| pabgb.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        let mut at = 0usize;
        for _ in 0..8 {
            let (Some(first), Some(slen)) = (word(at), word(at + 9)) else {
                break;
            };
            if first != 0 {
                return RowLayout::V206;
            }
            at += 13 + slen as usize;
        }
        RowLayout::Legacy
    }
}

impl StringInfoData {
    /// Walk the pabgb body. Each record is self-describing (the slen
    /// prefix encodes its full length) so the index isn't required for
    /// parsing — only for verifying offsets match and for byte-identical
    /// round-trip of the pabgh side.
    pub fn parse_pabgb(pabgb: &[u8]) -> io::Result<Vec<StringInfoEntry>> {
        let layout = RowLayout::detect(pabgb);
        let mut offset = 0usize;
        let mut entries = Vec::new();
        while offset < pabgb.len() {
            let entry_start = offset;
            let entry = read_entry(pabgb, &mut offset, layout).map_err(|e| {
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
        let layout = if index.named { RowLayout::V206 } else { RowLayout::Legacy };

        // Walk pabgb sequentially, recording (offset_at_start, hash) for
        // every entry. Compare against the index in pabgh on-disk order.
        let mut offset = 0usize;
        let mut entries: Vec<StringInfoEntry> = Vec::with_capacity(index.entries.len());
        let mut starts: Vec<u32> = Vec::with_capacity(index.entries.len());
        while offset < pabgb.len() {
            let entry_start = offset;
            let entry = read_entry(pabgb, &mut offset, layout).map_err(|e| {
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
        Ok(StringInfoData { entries, index_order: Some(index), layout })
    }

    /// Serialise the pabgb body back to bytes. Linear walk of `entries`;
    /// no re-ordering. Pair with [`Self::write_pabgh`] for the index.
    pub fn write_pabgb(&self) -> io::Result<Vec<u8>> {
        let mut buf = Vec::new();
        for entry in &self.entries {
            write_entry(entry, &mut buf, self.layout)?;
        }
        Ok(buf)
    }

    /// Serialise the pabgh index back to bytes. Uses the cached
    /// `index_order` when available; otherwise re-derives offsets by
    /// walking the entries. The auto-derived path always emits a `u16`
    /// count prefix (matches every observed game build).
    pub fn write_pabgh(&self) -> io::Result<Vec<u8>> {
        let named = match &self.index_order {
            Some(idx) => idx.named,
            None => self.layout == RowLayout::V206,
        };
        if named {
            return self.write_named_pabgh();
        }
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
                StringInfoIndex { entries, count_width: CountWidth::U16, named: false }
            }
        };
        write_pabgh(&index)
    }

    /// The 2.06 index: count, then `[hash][u32 0][offset][size]` per entry in
    /// `entries` order. The count width comes from the parsed index (u16 when
    /// there is none).
    fn write_named_pabgh(&self) -> io::Result<Vec<u8>> {
        let width = match self.index_order.as_ref().map(|i| i.count_width) {
            Some(CountWidth::U32) => 4,
            _ => 2,
        };
        let count = self.entries.len() as u64;
        if width == 2 && count > u64::from(u16::MAX) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("PABGH: {count} entries do not fit in a u16 count"),
            ));
        }
        let mut buf = count.to_le_bytes()[..width].to_vec();
        let mut offset = 0u32;
        for e in &self.entries {
            let size = entry_size(e)?;
            e.hash.write_to(&mut buf)?;
            0u32.write_to(&mut buf)?;
            offset.write_to(&mut buf)?;
            size.write_to(&mut buf)?;
            offset = offset.checked_add(size).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "stringinfo: pabgb size overflows u32")
            })?;
        }
        Ok(buf)
    }
}

// ── Per-entry codec ──────────────────────────────────────────────────────

fn read_entry(data: &[u8], offset: &mut usize, layout: RowLayout) -> io::Result<StringInfoEntry> {
    let (reserved_zero, hash) = match layout {
        RowLayout::Legacy => {
            let zero = u32::read_from(data, offset)?;
            (zero, u32::read_from(data, offset)?)
        }
        RowLayout::V206 => {
            let hash = u32::read_from(data, offset)?;
            (u32::read_from(data, offset)?, hash)
        }
    };
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

fn write_entry(entry: &StringInfoEntry, w: &mut Vec<u8>, layout: RowLayout) -> io::Result<()> {
    match layout {
        RowLayout::Legacy => {
            entry.reserved_zero.write_to(w)?;
            entry.hash.write_to(w)?;
        }
        RowLayout::V206 => {
            entry.hash.write_to(w)?;
            entry.reserved_zero.write_to(w)?;
        }
    }
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
    // 4 (reserved_zero / row index) + 4 (hash) + 1 (reserved_flag) + 4 (slen) + slen
    Ok(13u32 + slen)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2.06 body whose first hash is 0 still reads as 2.06, DLC row indices
    /// included; a legacy body reads as legacy.
    #[test]
    fn detect_uses_the_row_indices() {
        fn row(first: u32, second: u32, s: &str) -> Vec<u8> {
            let mut r = first.to_le_bytes().to_vec();
            r.extend_from_slice(&second.to_le_bytes());
            r.push(0);
            r.extend_from_slice(&(s.len() as u32).to_le_bytes());
            r.extend_from_slice(s.as_bytes());
            r
        }
        let v206 = [row(0, 0, "a"), row(0x1234, 1, "bc"), row(0x99, 2, "")].concat();
        assert_eq!(RowLayout::detect(&v206), RowLayout::V206);
        // A DLC table's row index is not the row's position.
        let dlc = [row(0, 167, "a"), row(0x1234, 170, "bc")].concat();
        assert_eq!(RowLayout::detect(&dlc), RowLayout::V206);
        let legacy = [row(0, 0x1234, "a"), row(0, 0x5678, "bc")].concat();
        assert_eq!(RowLayout::detect(&legacy), RowLayout::Legacy);
        let legacy_zero_first = [row(0, 0, "a"), row(0, 0x5678, "bc")].concat();
        assert_eq!(RowLayout::detect(&legacy_zero_first), RowLayout::Legacy);
    }

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
            index_order: None, layout: RowLayout::Legacy,
        };
        let pabgb_bytes = data.write_pabgb().unwrap();

        // zero4 + hash 0x2ad9f89e + flag + slen=9 + "RealWorld" = 22 bytes
        // zero4 + hash 0x04f6d06d + flag + slen=9 + "ChildWild" = 22 bytes
        // zero4 + hash 0xdeadbeef + flag + slen=0 + ""          = 13 bytes
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
            count_width: CountWidth::U16, named: false,
        };
        // Synthesize bytes through write_*, then re-parse.
        let pabgb_bytes = {
            let data = StringInfoData {
                entries: entries.clone(),
                index_order: Some(index.clone()), layout: RowLayout::Legacy,
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
            index_order: None, layout: RowLayout::Legacy,
        };
        let pabgb2 = data2.write_pabgb().unwrap();
        let pabgh2 = data2.write_pabgh().unwrap();
        let parsed2 = StringInfoData::parse_pair(&pabgb2, &pabgh2).unwrap();
        assert_eq!(parsed2.entries[0].value, "alpha_v2");
    }

    #[test]
    fn pabgb_rejects_truncated_string() {
        // zero4, hash=1, flag=0, slen=10, but only 5 bytes of payload
        let mut bad = Vec::new();
        bad.extend_from_slice(&0u32.to_le_bytes());
        bad.extend_from_slice(&1u32.to_le_bytes());
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
            index_order: None, layout: RowLayout::Legacy,
        };
        let pabgb = data.write_pabgb().unwrap();
        let bad_index = StringInfoIndex {
            entries: vec![StringIndexEntry { hash: 2, offset: 0 }],
            count_width: CountWidth::U16,
            named: false,
        };
        let pabgh = write_pabgh(&bad_index).unwrap();
        let err = StringInfoData::parse_pair(&pabgb, &pabgh).unwrap_err();
        assert!(err.to_string().contains("hash"), "got: {err}");
    }

    // ── Live-install integration test ────────────────────────────────────
    // Exercises the parser against the live install's stringinfo pair.
    // Skips when the game isn't present.
    fn find_table_bytes() -> Option<(Vec<u8>, Vec<u8>)> {
        // None only when the install (or the table) is absent; an install
        // that is there but unreadable fails the test (audit I-02).
        Some((
            crate::binary::gamedata_layout::extract_bin(&crate::binary::gamedata_layout::body("stringinfo"))?,
            crate::binary::gamedata_layout::extract_bin(&crate::binary::gamedata_layout::header("stringinfo"))?,
        ))
    }

    #[test]
    fn live_pair_roundtrip() {
        let Some((pabgb, pabgh)) = find_table_bytes() else {
            eprintln!("skipping live_pair_roundtrip: no game install");
            return;
        };
        let parsed = StringInfoData::parse_pair(&pabgb, &pabgh).unwrap();
        assert_eq!(parsed.layout, RowLayout::V206, "2.06 rows open with the hash");
        assert!(parsed.entries.iter().enumerate().all(|(i, e)| e.reserved_zero as usize == i));

        assert_eq!(
            parsed.entries.len(),
            33_855,
            "expected 33,855 rows in 2.06 (33,910 in 2.05, 33,890 in 2.04.08, 33,888 in 2.04.03, 31,812 in 2.03)"
        );

        // Byte-identical round-trip.
        let pabgb_out = parsed.write_pabgb().unwrap();
        let pabgh_out = parsed.write_pabgh().unwrap();
        assert_eq!(pabgb_out, pabgb, "pabgb round-trip differs");
        assert_eq!(pabgh_out, pabgh, "pabgh round-trip differs");

        // First entry's hash + value on 2.04, read from the bytes during
        // the 2.04 pass. Pinned so future schema drift fails loudly.
        assert_eq!(parsed.entries[0].hash, 0x04f6d06d);
        assert_eq!(parsed.entries[0].value, "rootlevel");
    }
}
