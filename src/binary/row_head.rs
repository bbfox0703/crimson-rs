//! The opening of a static-info table row.
//!
//! Through 2.03 a named row opened `[key][u32 len][name]`. 2.04 swapped the two
//! in every such table: `[u32 len][name][key]`, with the key still as wide as
//! the table's `.staticinfoheader` makes it (1, 2 or 4 bytes).

/// A decoded row head, and where the row carries on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowHead<'a> {
    pub key: u32,
    pub name: &'a str,
    /// Offset of the first byte after the key, relative to the slice read.
    pub body_at: usize,
}

/// Read a row head at the start of `row`. `None` when the bytes there cannot be
/// one: a length past the slice, or a name that is not UTF-8.
pub fn read_row_head(row: &[u8], key_width: usize) -> Option<RowHead<'_>> {
    let name_len = u32::from_le_bytes(row.get(..4)?.try_into().ok()?) as usize;
    let key_at = 4usize.checked_add(name_len)?;
    let name = std::str::from_utf8(row.get(4..key_at)?).ok()?;
    let key_bytes = row.get(key_at..key_at.checked_add(key_width)?)?;
    let mut key = [0u8; 4];
    key[..key_width.min(4)].copy_from_slice(&key_bytes[..key_width.min(4)]);
    Some(RowHead {
        key: u32::from_le_bytes(key),
        name,
        body_at: key_at + key_width,
    })
}

/// The byte set a row name uses: ASCII word characters and spaces, or any UTF-8
/// high byte (1.05 added Ⅲ/Ⅳ/Ⅵ). For scanners, which also validate the UTF-8.
pub fn is_row_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b' ' || b >= 0x80
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_each_key_width() {
        let mut row = 3u32.to_le_bytes().to_vec();
        row.extend_from_slice(b"abc");
        row.extend_from_slice(&[0x78, 0x56, 0x34, 0x12, 0xFF]);

        let head = read_row_head(&row, 4).unwrap();
        assert_eq!((head.key, head.name, head.body_at), (0x12345678, "abc", 11));
        assert_eq!(read_row_head(&row, 2).unwrap().key, 0x5678);
        assert_eq!(read_row_head(&row, 1).unwrap().key, 0x78);
    }

    #[test]
    fn rejects_what_cannot_be_a_head() {
        assert_eq!(read_row_head(&[1, 2], 4), None);
        assert_eq!(read_row_head(&u32::MAX.to_le_bytes(), 4), None);

        let mut not_utf8 = 1u32.to_le_bytes().to_vec();
        not_utf8.extend_from_slice(&[0xFF, 0, 0, 0, 0]);
        assert_eq!(read_row_head(&not_utf8, 4), None);

        let mut no_key = 1u32.to_le_bytes().to_vec();
        no_key.extend_from_slice(b"a");
        assert_eq!(read_row_head(&no_key, 4), None);
    }
}
