//! The `.staticinfoheader` index of a static-info table, 2.06 layout.
//!
//! Through 2.05.01 the header was `count` then `(key, u32 body offset)` per row,
//! and each named row in the `.staticinfobody` opened with its name
//! (`[u32 len][name][key]` since 2.04, see [`super::row_head`]).
//!
//! 2.06 moved the name into the header and gave every row its size:
//!
//! ```text
//! header: count, then per row  [key][u32 name len][name][u32 body offset][u32 body size]
//! body:   per row               [key][u32 row index][the rest of the row as before]
//! ```
//!
//! `count` is 1, 2 or 4 bytes and `key` 1, 2, 4, 8 or 12 bytes, both set per
//! table (they are the widths the old header used). The row index is the row's
//! position in its own table for every base table; the DLC copy of a table
//! carries other values there (see `scripts/CLAUDE.md`, 2.06). Rows are packed:
//! each row's offset is the previous offset plus the previous size, and the last
//! row ends the body.

/// One row of a 2.06 header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeaderRow<'a> {
    /// The key, zero-extended. Keys wider than 8 bytes keep their low 8 bytes.
    pub key: u64,
    pub name: &'a str,
    pub offset: u32,
    pub size: u32,
}

impl HeaderRow<'_> {
    /// The row's byte range in the body.
    pub fn range(&self) -> std::ops::Range<usize> {
        self.offset as usize..self.offset as usize + self.size as usize
    }
}

fn read_uint(bytes: &[u8]) -> u64 {
    let mut buf = [0u8; 8];
    let n = bytes.len().min(8);
    buf[..n].copy_from_slice(&bytes[..n]);
    u64::from_le_bytes(buf)
}

/// Parse a 2.06 header with known widths. `None` unless the whole header is
/// consumed exactly and every name is UTF-8.
pub fn parse_named_header(
    hdr: &[u8],
    count_width: usize,
    key_width: usize,
) -> Option<Vec<HeaderRow<'_>>> {
    let count = read_uint(hdr.get(..count_width)?) as usize;
    // Smallest possible row: key + len + offset + size.
    if count > hdr.len() / (key_width + 12).max(1) {
        return None;
    }
    let mut at = count_width;
    let mut rows = Vec::with_capacity(count);
    for _ in 0..count {
        let key = read_uint(hdr.get(at..at + key_width)?);
        at += key_width;
        let len = u32::from_le_bytes(hdr.get(at..at + 4)?.try_into().ok()?) as usize;
        at += 4;
        let name = std::str::from_utf8(hdr.get(at..at.checked_add(len)?)?).ok()?;
        at += len;
        let offset = u32::from_le_bytes(hdr.get(at..at + 4)?.try_into().ok()?);
        let size = u32::from_le_bytes(hdr.get(at + 4..at + 8)?.try_into().ok()?);
        at += 8;
        rows.push(HeaderRow { key, name, offset, size });
    }
    (at == hdr.len()).then_some(rows)
}

/// The count and key widths 2.06 uses, most common first.
const WIDTHS: [(usize, usize); 12] = [
    (2, 4),
    (2, 2),
    (4, 4),
    (2, 1),
    (4, 8),
    (2, 12),
    (1, 1),
    (4, 2),
    (1, 2),
    (1, 4),
    (4, 1),
    (2, 8),
];

/// Parse a 2.06 header whose widths are not known in advance: the first
/// `(count width, key width)` pair that consumes the header exactly and keeps
/// the rows packed against `body_len`. Returns the widths with the rows.
pub fn detect_named_header(
    hdr: &[u8],
    body_len: usize,
) -> Option<(usize, usize, Vec<HeaderRow<'_>>)> {
    WIDTHS.iter().find_map(|&(cw, kw)| {
        let rows = parse_named_header(hdr, cw, kw)?;
        rows_are_packed(&rows, body_len).then_some((cw, kw, rows))
    })
}

/// True when the rows tile `0..body_len` in order with no gaps.
pub fn rows_are_packed(rows: &[HeaderRow<'_>], body_len: usize) -> bool {
    let mut next = 0usize;
    for r in rows {
        if r.offset as usize != next {
            return false;
        }
        next += r.size as usize;
    }
    next == body_len
}

/// The body row of a 2.06 table: key, row index, then the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BodyRow<'a> {
    pub key: u64,
    pub index: u32,
    /// The row after the key and the index.
    pub rest: &'a [u8],
}

/// Split a 2.06 body row (`body[row.range()]`).
pub fn read_body_row(row: &[u8], key_width: usize) -> Option<BodyRow<'_>> {
    let key = read_uint(row.get(..key_width)?);
    let index = u32::from_le_bytes(row.get(key_width..key_width + 4)?.try_into().ok()?);
    Some(BodyRow { key, index, rest: &row[key_width + 4..] })
}

/// A table rebuilt in the pre-2.06 layout (2.04 through 2.05.01): header
/// `count` + `(key, u32 offset)` per row, rows `[u32 len][name][key][rest]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyTable {
    pub body: Vec<u8>,
    pub header: Vec<u8>,
    pub count_width: usize,
    pub key_width: usize,
}

/// Rebuild a 2.06 table (`body` + its `.staticinfoheader`) in the pre-2.06
/// layout the table parsers were written against: the name goes back in front
/// of the key, the row index is dropped, and the header loses names and sizes.
/// `None` when `header` is not a 2.06 header for `body` (an older install, for
/// which the parsers need no help) or a row is too short for its key and index.
pub fn to_legacy_layout(body: &[u8], header: &[u8]) -> Option<LegacyTable> {
    let (count_width, key_width, rows) = detect_named_header(header, body.len())?;
    let mut new_body = Vec::with_capacity(body.len() + header.len());
    let mut new_header = Vec::with_capacity(count_width + rows.len() * (key_width + 4));
    new_header.extend_from_slice(&(rows.len() as u64).to_le_bytes()[..count_width]);
    for r in &rows {
        let row = body.get(r.range())?;
        let key = row.get(..key_width)?;
        let rest = row.get(key_width + 4..)?;
        new_header.extend_from_slice(key);
        new_header.extend_from_slice(&(new_body.len() as u32).to_le_bytes());
        new_body.extend_from_slice(&(r.name.len() as u32).to_le_bytes());
        new_body.extend_from_slice(r.name.as_bytes());
        new_body.extend_from_slice(key);
        new_body.extend_from_slice(rest);
    }
    Some(LegacyTable { body: new_body, header: new_header, count_width, key_width })
}

/// The row indices of a 2.06 table, in header order (for
/// [`from_legacy_layout`]). `None` when it is not a 2.06 pair.
pub fn row_indices(body: &[u8], header: &[u8]) -> Option<Vec<u32>> {
    let (_, kw, rows) = detect_named_header(header, body.len())?;
    rows.iter()
        .map(|r| read_body_row(body.get(r.range())?, kw).map(|b| b.index))
        .collect()
}

/// The inverse of [`to_legacy_layout`]: rebuild the 2.06 pair from a table in
/// the pre-2.06 layout. `indices[i]` is the row index to write into header row
/// `i` (in a base table, `i` itself). Each legacy row runs from its offset to
/// the next larger offset (or the end of `body`). `None` on a header that does
/// not match `count_width` / `key_width`, a row too short for its head, or a
/// name that is not UTF-8. Returns `(body, header)`.
pub fn from_legacy_layout(
    body: &[u8],
    header: &[u8],
    count_width: usize,
    key_width: usize,
    indices: &[u32],
) -> Option<(Vec<u8>, Vec<u8>)> {
    let count = read_uint(header.get(..count_width)?) as usize;
    if header.len() != count_width + count * (key_width + 4) || indices.len() != count {
        return None;
    }
    let offsets: Vec<usize> = (0..count)
        .map(|i| {
            let at = count_width + i * (key_width + 4) + key_width;
            Some(u32::from_le_bytes(header.get(at..at + 4)?.try_into().ok()?) as usize)
        })
        .collect::<Option<_>>()?;
    let mut sorted = offsets.clone();
    sorted.sort_unstable();
    let mut new_body = Vec::with_capacity(body.len());
    let mut new_header = (count as u64).to_le_bytes()[..count_width].to_vec();
    for (i, &start) in offsets.iter().enumerate() {
        let end = sorted
            .iter()
            .find(|&&o| o > start)
            .copied()
            .unwrap_or(body.len());
        let row = body.get(start..end)?;
        let name_len = u32::from_le_bytes(row.get(..4)?.try_into().ok()?) as usize;
        let name = row.get(4..4 + name_len)?;
        std::str::from_utf8(name).ok()?;
        let key = row.get(4 + name_len..4 + name_len + key_width)?;
        let rest = row.get(4 + name_len + key_width..)?;
        let offset = new_body.len() as u32;
        new_body.extend_from_slice(key);
        new_body.extend_from_slice(&indices[i].to_le_bytes());
        new_body.extend_from_slice(rest);
        new_header.extend_from_slice(key);
        new_header.extend_from_slice(&(name_len as u32).to_le_bytes());
        new_header.extend_from_slice(name);
        new_header.extend_from_slice(&offset.to_le_bytes());
        new_header.extend_from_slice(&(new_body.len() as u32 - offset).to_le_bytes());
    }
    Some((new_body, new_header))
}

/// The `(key, name)` of every row a 2.06 header lists, in header order. This
/// is the table's authoritative key list, so a name-only bridge reads it
/// instead of anchor-scanning the body. `None` when `header` is not a 2.06
/// header for `body`, or when its keys are wider than a `u32`.
pub fn header_row_names(body: &[u8], header: &[u8]) -> Option<Vec<(u32, String)>> {
    let (_, key_width, rows) = detect_named_header(header, body.len())?;
    if key_width > 4 {
        return None;
    }
    Some(rows.iter().map(|r| (r.key as u32, r.name.to_owned())).collect())
}

/// Parsers that take `(body, header)` call this first: a 2.06 pair comes back
/// rebuilt in the pre-2.06 layout, anything else unchanged.
pub fn legacy_pair<'a>(
    body: &'a [u8],
    header: &'a [u8],
) -> (std::borrow::Cow<'a, [u8]>, std::borrow::Cow<'a, [u8]>) {
    use std::borrow::Cow;
    match to_legacy_layout(body, header) {
        Some(t) => (Cow::Owned(t.body), Cow::Owned(t.header)),
        None => (Cow::Borrowed(body), Cow::Borrowed(header)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebuilds_the_legacy_layout() {
        // Two 2-byte-key rows: "A" (rest = [1]) and "BC" (rest = [2, 3]).
        let mut body = vec![0x10, 0x00];
        body.extend_from_slice(&0u32.to_le_bytes());
        body.push(1);
        body.extend_from_slice(&[0x20, 0x00]);
        body.extend_from_slice(&1u32.to_le_bytes());
        body.extend_from_slice(&[2, 3]);
        let h = header(2, 2, &[(0x10, "A", 0, 7), (0x20, "BC", 7, 8)]);
        let t = to_legacy_layout(&body, &h).unwrap();
        assert_eq!((t.count_width, t.key_width), (2, 2));
        assert_eq!(t.body, [1, 0, 0, 0, b'A', 0x10, 0, 1, 2, 0, 0, 0, b'B', b'C', 0x20, 0, 2, 3]);
        assert_eq!(t.header, [2, 0, 0x10, 0, 0, 0, 0, 0, 0x20, 0, 8, 0, 0, 0]);
        // An older header passes through untouched.
        let (b, hh) = legacy_pair(&t.body, &t.header);
        assert_eq!((&*b, &*hh), (&t.body[..], &t.header[..]));
        // And the way back reproduces the 2.06 pair.
        let idx = row_indices(&body, &h).unwrap();
        assert_eq!(idx, [0, 1]);
        let (b2, h2) = from_legacy_layout(&t.body, &t.header, 2, 2, &idx).unwrap();
        assert_eq!((b2, h2), (body, h));
    }

    fn header(count_width: usize, key_width: usize, rows: &[(u64, &str, u32, u32)]) -> Vec<u8> {
        let mut h = (rows.len() as u64).to_le_bytes()[..count_width].to_vec();
        for &(key, name, off, size) in rows {
            let mut k = key.to_le_bytes().to_vec();
            k.resize(key_width, 0);
            h.extend_from_slice(&k);
            h.extend_from_slice(&(name.len() as u32).to_le_bytes());
            h.extend_from_slice(name.as_bytes());
            h.extend_from_slice(&off.to_le_bytes());
            h.extend_from_slice(&size.to_le_bytes());
        }
        h
    }

    #[test]
    fn parses_and_detects_widths() {
        let h = header(2, 2, &[(3101, "Store_Her_General", 0, 10), (3102, "B", 10, 6)]);
        let rows = parse_named_header(&h, 2, 2).unwrap();
        assert_eq!(rows[0], HeaderRow { key: 3101, name: "Store_Her_General", offset: 0, size: 10 });
        assert_eq!(rows[1].range(), 10..16);
        let (cw, kw, _) = detect_named_header(&h, 16).unwrap();
        assert_eq!((cw, kw), (2, 2));
        assert!(detect_named_header(&h, 17).is_none());
    }

    #[test]
    fn rejects_a_short_or_long_header() {
        let mut h = header(2, 4, &[(2200, "Pyeonjeon_Arrow", 0, 561)]);
        assert!(parse_named_header(&h, 2, 4).is_some());
        h.push(0);
        assert!(parse_named_header(&h, 2, 4).is_none());
        assert!(parse_named_header(&h[..h.len() - 3], 2, 4).is_none());
    }

    #[test]
    fn splits_a_body_row() {
        let mut row = 2200u32.to_le_bytes().to_vec();
        row.extend_from_slice(&7u32.to_le_bytes());
        row.extend_from_slice(&[0xAA, 0xBB]);
        let b = read_body_row(&row, 4).unwrap();
        assert_eq!((b.key, b.index, b.rest), (2200, 7, &[0xAA, 0xBB][..]));
    }
}
