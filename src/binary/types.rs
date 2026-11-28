use std::io::{self, Write};

use super::{
    BinaryRead, BinaryReadTracked, BinaryWrite, FieldRange, check_remaining, pop_path, push_index,
    push_path,
};

// ── CString ─────────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
pub struct CString<'a> {
    pub length: u32,
    pub data: std::borrow::Cow<'a, str>,
    raw: Option<&'a [u8]>,
}

impl<'a> CString<'a> {
    /// The text: borrowed from the table, or a lossy copy owned by this value
    /// when the bytes are not UTF-8.
    pub fn as_str(&self) -> &str {
        &self.data
    }

    /// Return the original bytes (preserves non-UTF-8 data for roundtrip).
    pub fn as_bytes(&self) -> &[u8] {
        self.raw.unwrap_or(self.data.as_bytes())
    }
}

impl<'a> BinaryRead<'a> for CString<'a> {
    fn read_from(data: &'a [u8], offset: &mut usize) -> io::Result<Self> {
        let length = u32::read_from(data, offset)?;
        let len = length as usize;
        check_remaining(data, *offset, len)?;
        let bytes = &data[*offset..*offset + len];
        *offset += len;
        match std::str::from_utf8(bytes) {
            Ok(s) => Ok(CString { length, data: s.into(), raw: None }),
            // A non-UTF-8 string keeps a lossy copy for display and its raw
            // bytes for the write-back. The copy is owned by this value, so a
            // long-running host that loads many tables leaks nothing.
            Err(_) => Ok(CString {
                length,
                data: String::from_utf8_lossy(bytes).into_owned().into(),
                raw: Some(bytes),
            }),
        }
    }
}

impl BinaryWrite for CString<'_> {
    fn write_to(&self, w: &mut dyn Write) -> io::Result<()> {
        self.length.write_to(w)?;
        w.write_all(self.as_bytes())
    }
}

impl<'a> BinaryReadTracked<'a> for CString<'a> {
    fn read_tracked(
        data: &'a [u8],
        offset: &mut usize,
        path: &mut String,
        ranges: &mut Vec<FieldRange>,
    ) -> io::Result<Self> {
        // length prefix (u32) — recorded under `<path>.__len__`
        let len_start = *offset;
        let length = u32::read_from(data, offset)?;
        let saved = push_path(path, "__len__");
        ranges.push(FieldRange {
            path: path.clone(),
            start: len_start,
            end: *offset,
            ty: "CString.len",
        });
        pop_path(path, saved);

        // payload bytes — recorded under the field path itself
        let payload_start = *offset;
        let len = length as usize;
        check_remaining(data, *offset, len)?;
        let bytes = &data[*offset..*offset + len];
        *offset += len;
        ranges.push(FieldRange {
            path: path.clone(),
            start: payload_start,
            end: *offset,
            ty: "CString",
        });
        match std::str::from_utf8(bytes) {
            Ok(s) => Ok(CString { length, data: s.into(), raw: None }),
            Err(_) => Ok(CString {
                length,
                data: String::from_utf8_lossy(bytes).into_owned().into(),
                raw: Some(bytes),
            }),
        }
    }
}

// ── HeaderName ──────────────────────────────────────────────────────────────

/// A row name that 2.06 moved out of the row into the table's
/// `.staticinfoheader` (see `binary::static_header`). It occupies no bytes in
/// the row: reading leaves it empty and writing emits nothing. Parsers that have
/// the header fill `data` in afterwards.
#[derive(Debug, PartialEq, Default, Clone, Copy)]
pub struct HeaderName<'a> {
    pub data: &'a str,
}

impl<'a> BinaryRead<'a> for HeaderName<'a> {
    fn read_from(_data: &'a [u8], _offset: &mut usize) -> io::Result<Self> {
        Ok(HeaderName::default())
    }
}

impl BinaryWrite for HeaderName<'_> {
    fn write_to(&self, _w: &mut dyn Write) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> BinaryReadTracked<'a> for HeaderName<'a> {
    fn read_tracked(
        _data: &'a [u8],
        _offset: &mut usize,
        _path: &mut String,
        _ranges: &mut Vec<FieldRange>,
    ) -> io::Result<Self> {
        Ok(HeaderName::default())
    }
}

// ── CArray ──────────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
pub struct CArray<T> {
    pub items: Vec<T>,
}

impl<'a, T: BinaryRead<'a>> BinaryRead<'a> for CArray<T> {
    fn read_from(data: &'a [u8], offset: &mut usize) -> io::Result<Self> {
        let count = u32::read_from(data, offset)? as usize;
        // Sanity clamp: even the smallest element is >= 1 byte, so a
        // count exceeding the remaining byte budget can only be a
        // corrupted stream (e.g. a mod that byte-patched a count
        // prefix). Without this check, `Vec::with_capacity(huge)` can
        // attempt a multi-GB allocation before the actual read fails.
        let remaining = data.len().saturating_sub(*offset);
        if count > remaining {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "CArray count {} exceeds remaining bytes {} at offset {}",
                    count, remaining, *offset,
                ),
            ));
        }
        // `count <= remaining` bounds the element count, not the memory: a
        // large `T` would still reserve remaining x size_of::<T>(). Reserve
        // what the remaining bytes could hold and let a real count grow past
        // it.
        let mut items = Vec::with_capacity(count.min(remaining / std::mem::size_of::<T>().max(1)));
        for _ in 0..count {
            items.push(T::read_from(data, offset)?);
        }
        Ok(CArray { items })
    }
}

impl<T: BinaryWrite> BinaryWrite for CArray<T> {
    fn write_to(&self, w: &mut dyn Write) -> io::Result<()> {
        (self.items.len() as u32).write_to(w)?;
        for item in &self.items {
            item.write_to(w)?;
        }
        Ok(())
    }
}

impl<'a, T: BinaryReadTracked<'a>> BinaryReadTracked<'a> for CArray<T> {
    fn read_tracked(
        data: &'a [u8],
        offset: &mut usize,
        path: &mut String,
        ranges: &mut Vec<FieldRange>,
    ) -> io::Result<Self> {
        // count prefix (u32) — recorded under `<path>.__count__`
        let count_start = *offset;
        let count = u32::read_from(data, offset)? as usize;
        let saved = push_path(path, "__count__");
        ranges.push(FieldRange {
            path: path.clone(),
            start: count_start,
            end: *offset,
            ty: "CArray.count",
        });
        pop_path(path, saved);

        // Same sanity clamp as `BinaryRead` impl — see notes there.
        let remaining = data.len().saturating_sub(*offset);
        if count > remaining {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "CArray count {} exceeds remaining bytes {} at offset {}",
                    count, remaining, *offset,
                ),
            ));
        }

        let mut items = Vec::with_capacity(count.min(remaining / std::mem::size_of::<T>().max(1)));
        for i in 0..count {
            let saved = push_index(path, i);
            items.push(T::read_tracked(data, offset, path, ranges)?);
            pop_path(path, saved);
        }
        Ok(CArray { items })
    }
}

// ── Presence flags ──────────────────────────────────────────────────────────

/// A presence byte must be 0 or 1. Any other value is an error: reading it as
/// "present" and writing back 1 would guess at a drifted field instead of
/// failing, and the round-trip would change the byte. Real data uses 0 and 1
/// alone.
fn presence_flag(flag: u8, what: &str, at: usize) -> io::Result<bool> {
    match flag {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{what}: presence byte {flag:#04x} at offset {at} (expected 0 or 1)"),
        )),
    }
}

/// A flagged array that says "present" and then holds no elements would write
/// back as the lone `0x00`, changing the bytes.
fn flagged_array_not_empty<T>(items: &[T], at: usize) -> io::Result<()> {
    if items.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("CFlaggedArray at offset {at}: flag 1 with count 0"),
        ));
    }
    Ok(())
}

// ── CFlaggedArray ───────────────────────────────────────────────────────────

/// A 2.04 list: one presence byte, then — only when it is non-zero — the
/// `CArray` layout. An empty list is the lone `0x00`.
#[derive(Debug, PartialEq)]
pub struct CFlaggedArray<T> {
    pub items: Vec<T>,
}

impl<'a, T: BinaryRead<'a>> BinaryRead<'a> for CFlaggedArray<T> {
    fn read_from(data: &'a [u8], offset: &mut usize) -> io::Result<Self> {
        let at = *offset;
        if !presence_flag(u8::read_from(data, offset)?, "CFlaggedArray", at)? {
            return Ok(CFlaggedArray { items: Vec::new() });
        }
        let CArray { items } = CArray::read_from(data, offset)?;
        flagged_array_not_empty(&items, at)?;
        Ok(CFlaggedArray { items })
    }
}

impl<T: BinaryWrite> BinaryWrite for CFlaggedArray<T> {
    fn write_to(&self, w: &mut dyn Write) -> io::Result<()> {
        if self.items.is_empty() {
            return 0u8.write_to(w);
        }
        1u8.write_to(w)?;
        (self.items.len() as u32).write_to(w)?;
        for item in &self.items {
            item.write_to(w)?;
        }
        Ok(())
    }
}

impl<'a, T: BinaryReadTracked<'a>> BinaryReadTracked<'a> for CFlaggedArray<T> {
    fn read_tracked(
        data: &'a [u8],
        offset: &mut usize,
        path: &mut String,
        ranges: &mut Vec<FieldRange>,
    ) -> io::Result<Self> {
        // presence tag (u8) — recorded under `<path>.__tag__`
        let tag_start = *offset;
        let flag = u8::read_from(data, offset)?;
        let saved = push_path(path, "__tag__");
        ranges.push(FieldRange {
            path: path.clone(),
            start: tag_start,
            end: *offset,
            ty: "CFlaggedArray.tag",
        });
        pop_path(path, saved);

        if !presence_flag(flag, "CFlaggedArray", tag_start)? {
            return Ok(CFlaggedArray { items: Vec::new() });
        }
        let CArray { items } = CArray::read_tracked(data, offset, path, ranges)?;
        flagged_array_not_empty(&items, tag_start)?;
        Ok(CFlaggedArray { items })
    }
}

// ── COptional ───────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
pub struct COptional<T> {
    pub value: Option<T>,
}

impl<'a, T: BinaryRead<'a>> BinaryRead<'a> for COptional<T> {
    fn read_from(data: &'a [u8], offset: &mut usize) -> io::Result<Self> {
        let at = *offset;
        let flag = u8::read_from(data, offset)?;
        let value = if presence_flag(flag, "COptional", at)? {
            Some(T::read_from(data, offset)?)
        } else {
            None
        };
        Ok(COptional { value })
    }
}

impl<T: BinaryWrite> BinaryWrite for COptional<T> {
    fn write_to(&self, w: &mut dyn Write) -> io::Result<()> {
        match &self.value {
            Some(v) => {
                1u8.write_to(w)?;
                v.write_to(w)
            }
            None => 0u8.write_to(w),
        }
    }
}

impl<'a, T: BinaryReadTracked<'a>> BinaryReadTracked<'a> for COptional<T> {
    fn read_tracked(
        data: &'a [u8],
        offset: &mut usize,
        path: &mut String,
        ranges: &mut Vec<FieldRange>,
    ) -> io::Result<Self> {
        // presence tag (u8) — recorded under `<path>.__tag__`
        let tag_start = *offset;
        let flag = u8::read_from(data, offset)?;
        let saved = push_path(path, "__tag__");
        ranges.push(FieldRange {
            path: path.clone(),
            start: tag_start,
            end: *offset,
            ty: "COptional.tag",
        });
        pop_path(path, saved);

        let value = if presence_flag(flag, "COptional", tag_start)? {
            Some(T::read_tracked(data, offset, path, ranges)?)
        } else {
            None
        };
        Ok(COptional { value })
    }
}

// ── LocalizableString ───────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
pub struct LocalizableString<'a> {
    pub category: u8,
    pub index: u64,
    pub default: CString<'a>,
}

impl<'a> BinaryRead<'a> for LocalizableString<'a> {
    fn read_from(data: &'a [u8], offset: &mut usize) -> io::Result<Self> {
        let category = u8::read_from(data, offset)?;
        let index = u64::read_from(data, offset)?;
        let default = CString::read_from(data, offset)?;
        Ok(LocalizableString {
            category,
            index,
            default,
        })
    }
}

impl BinaryWrite for LocalizableString<'_> {
    fn write_to(&self, w: &mut dyn Write) -> io::Result<()> {
        self.category.write_to(w)?;
        self.index.write_to(w)?;
        self.default.write_to(w)
    }
}

impl<'a> BinaryReadTracked<'a> for LocalizableString<'a> {
    fn read_tracked(
        data: &'a [u8],
        offset: &mut usize,
        path: &mut String,
        ranges: &mut Vec<FieldRange>,
    ) -> io::Result<Self> {
        let saved = push_path(path, "category");
        let category = u8::read_tracked(data, offset, path, ranges)?;
        pop_path(path, saved);

        let saved = push_path(path, "index");
        let index = u64::read_tracked(data, offset, path, ranges)?;
        pop_path(path, saved);

        let saved = push_path(path, "default");
        let default = CString::read_tracked(data, offset, path, ranges)?;
        pop_path(path, saved);

        Ok(LocalizableString {
            category,
            index,
            default,
        })
    }
}

// ── OptLocalizableString ────────────────────────────────────────────────────

/// A [`LocalizableString`] as 2.04's item head stores it: category 0 is the
/// lone `0x00`, with no index or default behind it. Reads back as the empty
/// string so callers see the same shape either way.
#[derive(Debug, PartialEq)]
pub struct OptLocalizableString<'a> {
    pub category: u8,
    pub index: u64,
    pub default: CString<'a>,
}

impl<'a> BinaryRead<'a> for OptLocalizableString<'a> {
    fn read_from(data: &'a [u8], offset: &mut usize) -> io::Result<Self> {
        let category = u8::read_from(data, offset)?;
        if category == 0 {
            return Ok(OptLocalizableString {
                category,
                index: 0,
                default: CString { length: 0, data: "".into(), raw: None },
            });
        }
        let index = u64::read_from(data, offset)?;
        let default = CString::read_from(data, offset)?;
        Ok(OptLocalizableString {
            category,
            index,
            default,
        })
    }
}

impl BinaryWrite for OptLocalizableString<'_> {
    fn write_to(&self, w: &mut dyn Write) -> io::Result<()> {
        self.category.write_to(w)?;
        if self.category == 0 {
            return Ok(());
        }
        self.index.write_to(w)?;
        self.default.write_to(w)
    }
}

impl<'a> BinaryReadTracked<'a> for OptLocalizableString<'a> {
    fn read_tracked(
        data: &'a [u8],
        offset: &mut usize,
        path: &mut String,
        ranges: &mut Vec<FieldRange>,
    ) -> io::Result<Self> {
        let saved = push_path(path, "category");
        let category = u8::read_tracked(data, offset, path, ranges)?;
        pop_path(path, saved);
        if category == 0 {
            return Ok(OptLocalizableString {
                category,
                index: 0,
                default: CString { length: 0, data: "".into(), raw: None },
            });
        }

        let saved = push_path(path, "index");
        let index = u64::read_tracked(data, offset, path, ranges)?;
        pop_path(path, saved);

        let saved = push_path(path, "default");
        let default = CString::read_tracked(data, offset, path, ranges)?;
        pop_path(path, saved);

        Ok(OptLocalizableString {
            category,
            index,
            default,
        })
    }
}

#[cfg(test)]
mod cstring_tests {
    use super::*;

    /// A non-UTF-8 string reads as an owned lossy copy (nothing leaked) and
    /// writes back its original bytes.
    #[test]
    fn non_utf8_is_owned_and_round_trips() {
        let mut data = 3u32.to_le_bytes().to_vec();
        data.extend_from_slice(&[b'a', 0xFF, b'b']);
        let mut off = 0;
        let s = CString::read_from(&data, &mut off).unwrap();
        assert_eq!(s.as_str(), "a\u{FFFD}b");
        assert!(matches!(s.data, std::borrow::Cow::Owned(_)));
        let mut out = Vec::new();
        s.write_to(&mut out).unwrap();
        assert_eq!(out, data);

        let mut ok = 2u32.to_le_bytes().to_vec();
        ok.extend_from_slice(b"hi");
        let mut off = 0;
        assert!(matches!(CString::read_from(&ok, &mut off).unwrap().data, std::borrow::Cow::Borrowed("hi")));
    }
}
