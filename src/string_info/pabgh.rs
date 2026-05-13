//! `stringinfo.pabgh` index parser.
//!
//! Layout: `count + count × (u32 hash, u32 offset_into_pabgb)`.
//!
//! The `count` prefix is `u16` in observed game builds (Crimson Desert
//! 1.06 has 30,206 entries — fits in u16) but the GameMods Python loader
//! auto-detects a `u32` fallback by checking whether `2 + count * 8`
//! equals the file size. We mirror that: parse as `u16` first, fall back
//! to `u32` on size mismatch. `write_pabgh` emits whichever width round-
//! trips the original file (recorded on the [`StringInfoIndex`] returned
//! by `parse_pabgh`).
//!
//! Pairs with [`super::pabgb`]: each entry's `offset` is a byte offset
//! into the matching `stringinfo.pabgb`, where the record at that offset
//! starts with the same `hash` u32.

// Index helpers are exercised by `string_info::tests::live_pair_roundtrip`
// and the synthetic pair test; the `c_abi` bridge intentionally ignores
// the pabgh side (every pabgb entry is self-describing). Match the
// `item_info` precedent and silence dead-code warnings file-wide.
#![allow(dead_code)]

use std::io;

use crate::binary::{BinaryRead, BinaryWrite};

/// On-disk count-prefix width. Recorded on the parsed index so the writer
/// can reproduce the original byte width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CountWidth {
    U16,
    U32,
}

/// One `(hash, offset)` pair in the PABGH index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StringIndexEntry {
    pub hash: u32,
    pub offset: u32,
}

/// Parsed PABGH index, plus the on-disk count width for round-trip.
#[derive(Debug, Clone)]
pub struct StringInfoIndex {
    pub entries: Vec<StringIndexEntry>,
    pub count_width: CountWidth,
}

/// Parse the on-disk index. The returned entries are in file order.
pub fn parse_pabgh(data: &[u8]) -> io::Result<StringInfoIndex> {
    // Try u16 count first (1.06 fits comfortably).
    if data.len() >= 2 {
        let mut offset = 0usize;
        let count16 = u16::read_from(data, &mut offset)? as usize;
        let needed = 2 + count16 * 8;
        if needed == data.len() {
            let entries = read_entries(data, offset, count16)?;
            return Ok(StringInfoIndex { entries, count_width: CountWidth::U16 });
        }
    }
    // Fall back to u32 count.
    if data.len() >= 4 {
        let mut offset = 0usize;
        let count32 = u32::read_from(data, &mut offset)? as usize;
        let needed = 4 + count32 * 8;
        if needed == data.len() {
            let entries = read_entries(data, offset, count32)?;
            return Ok(StringInfoIndex { entries, count_width: CountWidth::U32 });
        }
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "PABGH: file size {} doesn't match either u16 or u32 count width",
            data.len()
        ),
    ))
}

fn read_entries(data: &[u8], mut offset: usize, count: usize) -> io::Result<Vec<StringIndexEntry>> {
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let hash = u32::read_from(data, &mut offset)?;
        let off = u32::read_from(data, &mut offset)?;
        entries.push(StringIndexEntry { hash, offset: off });
    }
    Ok(entries)
}

/// Serialise an index back to bytes. Preserves the input order and the
/// original count-prefix width for byte-identical round-trip.
pub fn write_pabgh(index: &StringInfoIndex) -> io::Result<Vec<u8>> {
    let prefix_len = match index.count_width {
        CountWidth::U16 => 2,
        CountWidth::U32 => 4,
    };
    let mut buf = Vec::with_capacity(prefix_len + index.entries.len() * 8);

    match index.count_width {
        CountWidth::U16 => {
            let count = u16::try_from(index.entries.len()).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "PABGH: {} entries don't fit in u16 count prefix",
                        index.entries.len()
                    ),
                )
            })?;
            count.write_to(&mut buf)?;
        }
        CountWidth::U32 => {
            let count = u32::try_from(index.entries.len()).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "PABGH: {} entries don't fit in u32 count prefix",
                        index.entries.len()
                    ),
                )
            })?;
            count.write_to(&mut buf)?;
        }
    }
    for e in &index.entries {
        e.hash.write_to(&mut buf)?;
        e.offset.write_to(&mut buf)?;
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pabgh_roundtrip_u16() {
        let index = StringInfoIndex {
            entries: vec![
                StringIndexEntry { hash: 0x2ad9f89e, offset: 0 },
                StringIndexEntry { hash: 0x04f6d06d, offset: 22 },
            ],
            count_width: CountWidth::U16,
        };
        let bytes = write_pabgh(&index).unwrap();
        assert_eq!(bytes.len(), 2 + 2 * 8);
        let parsed = parse_pabgh(&bytes).unwrap();
        assert_eq!(parsed.entries, index.entries);
        assert_eq!(parsed.count_width, CountWidth::U16);
    }

    #[test]
    fn pabgh_roundtrip_u32() {
        let index = StringInfoIndex {
            entries: vec![
                StringIndexEntry { hash: 1, offset: 0 },
                StringIndexEntry { hash: 2, offset: 22 },
            ],
            count_width: CountWidth::U32,
        };
        let bytes = write_pabgh(&index).unwrap();
        assert_eq!(bytes.len(), 4 + 2 * 8);
        let parsed = parse_pabgh(&bytes).unwrap();
        assert_eq!(parsed.entries, index.entries);
        assert_eq!(parsed.count_width, CountWidth::U32);
    }

    #[test]
    fn pabgh_rejects_size_mismatch() {
        // count=2 (u16) but only 4 trailing bytes — half an entry
        let bad = [0x02, 0x00, 0x01, 0x00, 0x00, 0x00];
        assert!(parse_pabgh(&bad).is_err());
    }
}
