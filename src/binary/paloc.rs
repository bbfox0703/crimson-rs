use std::io::{self, Write};

use super::{BinaryRead, BinaryWrite, CString, check_remaining};

// ── Localization Entry ─────────────────────────────────────────────────────

#[derive(Debug)]
pub struct LocalizationEntry<'a> {
    pub unk_id: u64,
    pub string_key: CString<'a>,
    pub string_value: CString<'a>,
}

impl<'a> BinaryRead<'a> for LocalizationEntry<'a> {
    fn read_from(data: &'a [u8], offset: &mut usize) -> io::Result<Self> {
        Ok(LocalizationEntry {
            unk_id: u64::read_from(data, offset)?,
            string_key: CString::read_from(data, offset)?,
            string_value: CString::read_from(data, offset)?,
        })
    }
}

impl BinaryWrite for LocalizationEntry<'_> {
    fn write_to(&self, w: &mut dyn Write) -> io::Result<()> {
        self.unk_id.write_to(w)?;
        self.string_key.write_to(w)?;
        self.string_value.write_to(w)
    }
}

// ── Localization File ──────────────────────────────────────────────────────

#[derive(Debug)]
pub struct LocalizationFile<'a> {
    pub entries: Vec<LocalizationEntry<'a>>,
}

impl<'a> LocalizationFile<'a> {
    pub fn parse(data: &'a [u8]) -> io::Result<Self> {
        check_remaining(data, 0, 4)?;
        let count_offset = data.len() - 4;
        let entry_count = u32::from_le_bytes(data[count_offset..].try_into().unwrap()) as usize;

        // A `LocalizationEntry` is at minimum 16 bytes: u64 unk_id +
        // u32 string_key len + u32 string_value len (both strings can be
        // empty, but the length headers always appear). If the trailing
        // count claims more entries than could possibly fit in the
        // remaining body, the file is malformed — refuse rather than
        // allocate gigabytes of `Vec::with_capacity`. The raw
        // `gamedata/*.paloc` files in a Steam install fail this check
        // because they're still wrapped (encrypted + compressed) by the
        // PAZ pipeline; callers must extract first.
        let max_plausible = count_offset / 16;
        if entry_count > max_plausible {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "PALOC entry_count {entry_count} exceeds plausible max {max_plausible} \
                     for {count_offset} body bytes — file may be wrapped/encrypted"
                ),
            ));
        }

        let mut offset = 0;
        let mut entries = Vec::with_capacity(entry_count);
        for _ in 0..entry_count {
            entries.push(LocalizationEntry::read_from(data, &mut offset)?);
        }

        if offset != count_offset {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "entry data ends at 0x{:X} but expected 0x{:X} (before trailing count)",
                    offset, count_offset,
                ),
            ));
        }

        Ok(LocalizationFile { entries })
    }

    /// Inverse of [`parse`](Self::parse). Currently exercised only by the
    /// roundtrip tests in `lib.rs`; the Python wrapper inlines its own
    /// serialiser in `python.rs::serialize_paloc_impl` because it works
    /// from a `PyList` of dicts rather than a borrowed `LocalizationFile`.
    #[allow(dead_code)]
    pub fn to_bytes(&self) -> io::Result<Vec<u8>> {
        let mut buf = Vec::new();
        for entry in &self.entries {
            entry.write_to(&mut buf)?;
        }
        (self.entries.len() as u32).write_to(&mut buf)?;
        Ok(buf)
    }
}
