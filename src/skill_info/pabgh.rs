//! `skill.pabgh` index parser.
//!
//! Layout: `u16 count + count × (u32 key, u32 offset)`. Returned sorted by
//! offset (matches the GameMods Python parser's contract — the 1.05 dump
//! had 1963 entries, 0008 directory).
//!
//! Roundtrip note: the file order is by-offset on disk, but the on-disk
//! key order is *not* sorted. We preserve the on-disk order by reading
//! into a `Vec` first and only sorting a *copy* by offset for offset-based
//! navigation. `write_pabgh` writes back the original on-disk order.

use std::io;

use crate::binary::{BinaryRead, BinaryWrite};

/// One `(key, offset)` pair in the PABGH index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkillIndexEntry {
    pub key: u32,
    pub offset: u32,
}

/// Parse the on-disk index in file order. Caller may sort by offset
/// to navigate the PABGB; serialisation MUST use the on-disk order
/// to keep the file byte-identical.
pub fn parse_pabgh(data: &[u8]) -> io::Result<Vec<SkillIndexEntry>> {
    let mut offset = 0;
    let count = u16::read_from(data, &mut offset)? as usize;

    let needed = count * 8;
    if data.len() < 2 + needed {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            format!(
                "PABGH: header says {} entries (={} bytes) but file is {} bytes",
                count,
                2 + needed,
                data.len()
            ),
        ));
    }
    if data.len() != 2 + needed {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "PABGH: trailing bytes — header says {}*8={} payload bytes \
                 but file has {} payload bytes after the count u16",
                count,
                needed,
                data.len() - 2,
            ),
        ));
    }

    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let key = u32::read_from(data, &mut offset)?;
        let offs = u32::read_from(data, &mut offset)?;
        entries.push(SkillIndexEntry { key, offset: offs });
    }
    Ok(entries)
}

/// Serialise an index back to bytes. Preserves the input order — pass the
/// same vec you got from `parse_pabgh` for byte-identical roundtrip.
pub fn write_pabgh(entries: &[SkillIndexEntry]) -> io::Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(2 + entries.len() * 8);
    let count = u16::try_from(entries.len()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("PABGH: too many entries to fit in u16 ({})", entries.len()),
        )
    })?;
    count.write_to(&mut buf)?;
    for e in entries {
        e.key.write_to(&mut buf)?;
        e.offset.write_to(&mut buf)?;
    }
    Ok(buf)
}

/// Compute `(start, end)` byte ranges for each PABGB entry, given the
/// on-disk index and the PABGB length. Entries are returned in the same
/// order as the input. The `end` of the last on-disk entry is the file
/// length; for the rest, it's the offset of the entry that immediately
/// follows it on disk (sorted by offset, regardless of on-disk index order).
pub fn entry_ranges(entries: &[SkillIndexEntry], pabgb_len: usize) -> Vec<(usize, usize)> {
    // Sort indices by offset so we can compute end = next.offset.
    let mut by_offset: Vec<usize> = (0..entries.len()).collect();
    by_offset.sort_by_key(|&i| entries[i].offset);

    let mut next_offset_at_idx = vec![pabgb_len; entries.len()];
    for win in by_offset.windows(2) {
        let curr_idx = win[0];
        let next_idx = win[1];
        next_offset_at_idx[curr_idx] = entries[next_idx].offset as usize;
    }

    entries
        .iter()
        .enumerate()
        .map(|(i, e)| (e.offset as usize, next_offset_at_idx[i]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pabgh_roundtrip_synthetic() {
        let entries = vec![
            SkillIndexEntry { key: 999_999, offset: 0 },
            SkillIndexEntry { key: 109, offset: 484 },
            SkillIndexEntry { key: 111, offset: 882 },
        ];
        let bytes = write_pabgh(&entries).unwrap();
        let parsed = parse_pabgh(&bytes).unwrap();
        assert_eq!(parsed, entries);
        let bytes2 = write_pabgh(&parsed).unwrap();
        assert_eq!(bytes, bytes2);
    }

    #[test]
    fn pabgh_rejects_truncated() {
        // count=2 (4 bytes) but only 4 bytes of payload — should fail
        let bad = [
            0x02, 0x00, // count=2
            0x01, 0x00, 0x00, 0x00, // key
            0x00, 0x00, 0x00, 0x00, // offset
        ];
        assert!(parse_pabgh(&bad).is_err());
    }

    #[test]
    fn entry_ranges_handles_unsorted_keys() {
        // On-disk order: [a@0, c@800, b@200]. Sorted-by-offset: [a, b, c].
        let entries = vec![
            SkillIndexEntry { key: 1, offset: 0 },
            SkillIndexEntry { key: 3, offset: 800 },
            SkillIndexEntry { key: 2, offset: 200 },
        ];
        let ranges = entry_ranges(&entries, 1000);
        assert_eq!(ranges[0], (0, 200)); // a -> next is b
        assert_eq!(ranges[1], (800, 1000)); // c -> last on disk
        assert_eq!(ranges[2], (200, 800)); // b -> next is c
    }
}

