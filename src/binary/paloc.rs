use std::borrow::Cow;
use std::io::{self, Write};

use super::{BinaryRead, BinaryWrite, CString, check_remaining};

// ── 2.03 container ─────────────────────────────────────────────────────────
//
// Up to 2.02 a `.paloc` file is the bare entry list (below). Crimson Desert
// 2.03 wraps every one — all 39 namespace files of all 15 languages — in a
// fixed 0x200-byte header followed by a single LZ4 block:
//
//     0x000  "paloc"   magic, 5 bytes
//     0x005  u32       0 on every 2.03 file
//     0x009  u32       LZ4 block length (always file length - 0x200)
//     0x00D  u32       decompressed length
//     0x011  zeros     up to 0x200
//     0x200  LZ4 block decompresses to the pre-2.03 entry list, byte for byte
//
// The compression moved into the file: the PAMT now stores these entries
// uncompressed (`compression` 0). A bare entry list starts with a u64
// `unk_id`, never the magic, so the two layouts cannot be confused.
//
// Pearl Abyss's compressor is not lz4_flex's (re-compressing reproduces
// only 21 of the 585 2.03 blocks), so a wrapped file roundtrips through
// its decompressed body, not its compressed bytes — the same contract the
// save format has.

/// Magic that opens a 2.03+ paloc container.
pub const CONTAINER_MAGIC: &[u8; 5] = b"paloc";

/// Size of the container header; the LZ4 block starts here.
pub const CONTAINER_HEADER_LEN: usize = 0x200;

/// True when `data` opens with the 2.03 container magic.
pub fn is_wrapped(data: &[u8]) -> bool {
    data.starts_with(CONTAINER_MAGIC)
}

fn container_error(msg: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("PALOC container: {msg}"))
}

fn read_u32_at(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}

/// The bare entry list inside `data`: decompressed out of a 2.03 container,
/// or `data` itself (borrowed) when it has no container.
///
/// Every header field is checked, so a future change to the container
/// fails here with the field named instead of surfacing as a garbled
/// entry list.
pub fn unwrap_container(data: &[u8]) -> io::Result<Cow<'_, [u8]>> {
    if !is_wrapped(data) {
        return Ok(Cow::Borrowed(data));
    }
    if data.len() < CONTAINER_HEADER_LEN {
        return Err(container_error(format!(
            "{} bytes is shorter than the {CONTAINER_HEADER_LEN:#x}-byte header",
            data.len()
        )));
    }
    let reserved = read_u32_at(data, 0x05);
    if reserved != 0 {
        return Err(container_error(format!(
            "header u32 at 0x05 is {reserved:#x}, only 0 is known"
        )));
    }
    let block_len = read_u32_at(data, 0x09) as usize;
    let body_len = read_u32_at(data, 0x0D) as usize;
    if CONTAINER_HEADER_LEN + block_len != data.len() {
        return Err(container_error(format!(
            "header says a {block_len}-byte LZ4 block, file has {} bytes after the header",
            data.len() - CONTAINER_HEADER_LEN
        )));
    }
    if let Some(at) = data[0x11..CONTAINER_HEADER_LEN].iter().position(|&b| b != 0) {
        return Err(container_error(format!(
            "non-zero header byte at {:#x}",
            0x11 + at
        )));
    }
    // An LZ4 block expands at most ~255:1; refuse a length that could not
    // have come out of this block rather than allocate for it.
    if body_len > block_len.saturating_mul(255).saturating_add(16) {
        return Err(container_error(format!(
            "decompressed length {body_len} is implausible for a {block_len}-byte LZ4 block"
        )));
    }
    let body = lz4_flex::block::decompress(&data[CONTAINER_HEADER_LEN..], body_len)
        .map_err(|e| container_error(format!("LZ4 block: {e}")))?;
    if body.len() != body_len {
        return Err(container_error(format!(
            "LZ4 block decompressed to {} bytes, header says {body_len}",
            body.len()
        )));
    }
    Ok(Cow::Owned(body))
}

/// Inverse of [`unwrap_container`]: wrap a bare entry list the way 2.03
/// ships it. Decompresses back to `body` exactly; the compressed bytes are
/// lz4_flex's, not Pearl Abyss's. Refuses input that is already wrapped.
#[cfg_attr(not(feature = "python"), allow(dead_code))]
pub fn wrap_container(body: &[u8]) -> io::Result<Vec<u8>> {
    if is_wrapped(body) {
        return Err(container_error("input is already a container".into()));
    }
    let block = lz4_flex::block::compress(body);
    let len_u32 = |n: usize| {
        u32::try_from(n).map_err(|_| container_error(format!("{n} bytes does not fit a u32 length")))
    };
    let (block_len, body_len) = (len_u32(block.len())?, len_u32(body.len())?);
    let mut out = Vec::with_capacity(CONTAINER_HEADER_LEN + block.len());
    out.extend_from_slice(CONTAINER_MAGIC);
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&block_len.to_le_bytes());
    out.extend_from_slice(&body_len.to_le_bytes());
    out.resize(CONTAINER_HEADER_LEN, 0);
    out.extend_from_slice(&block);
    Ok(out)
}

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
    /// Parse a bare entry list. A 2.03+ file must go through
    /// [`unwrap_container`] first — the entries borrow from the buffer it
    /// returns.
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
        // PAZ pipeline; callers must extract first — and, from 2.03 on,
        // strip the in-file container with `unwrap_container`.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Two-entry bare list in the ≤ 2.02 wire format.
    fn bare_list() -> Vec<u8> {
        let mut out = Vec::new();
        for (id, key, value) in [(7u64, "4294967616", "Gear Socket"), (8, "k2", "")] {
            out.extend_from_slice(&id.to_le_bytes());
            out.extend_from_slice(&(key.len() as u32).to_le_bytes());
            out.extend_from_slice(key.as_bytes());
            out.extend_from_slice(&(value.len() as u32).to_le_bytes());
            out.extend_from_slice(value.as_bytes());
        }
        out.extend_from_slice(&2u32.to_le_bytes());
        out
    }

    #[test]
    fn bare_list_passes_through_borrowed() {
        let bare = bare_list();
        assert!(!is_wrapped(&bare));
        let out = unwrap_container(&bare).unwrap();
        assert!(matches!(out, Cow::Borrowed(_)));
        assert_eq!(&*out, &bare[..]);
    }

    #[test]
    fn container_roundtrips_through_its_body() {
        let bare = bare_list();
        let wrapped = wrap_container(&bare).unwrap();

        assert!(is_wrapped(&wrapped));
        assert_eq!(&wrapped[..5], b"paloc");
        assert_eq!(read_u32_at(&wrapped, 0x05), 0);
        assert_eq!(read_u32_at(&wrapped, 0x09) as usize, wrapped.len() - CONTAINER_HEADER_LEN);
        assert_eq!(read_u32_at(&wrapped, 0x0D) as usize, bare.len());
        assert!(wrapped[0x11..CONTAINER_HEADER_LEN].iter().all(|&b| b == 0));

        let body = unwrap_container(&wrapped).unwrap();
        assert_eq!(&*body, &bare[..]);
        let parsed = LocalizationFile::parse(&body).unwrap();
        assert_eq!(parsed.entries.len(), 2);
        assert_eq!(parsed.entries[0].string_value.data, "Gear Socket");
        assert_eq!(parsed.to_bytes().unwrap(), bare);

        assert!(wrap_container(&wrapped).is_err(), "double wrap must be refused");
    }

    #[test]
    fn container_rejects_a_drifted_header() {
        let wrapped = wrap_container(&bare_list()).unwrap();
        let expect_err = |bytes: &[u8], needle: &str| {
            let err = unwrap_container(bytes).unwrap_err().to_string();
            assert!(err.contains(needle), "{err:?} should mention {needle:?}");
        };

        let mut b = wrapped.clone();
        b[0x05] = 1;
        expect_err(&b, "0x05");

        let mut b = wrapped.clone();
        b[0x40] = 1;
        expect_err(&b, "non-zero header byte at 0x40");

        expect_err(&wrapped[..wrapped.len() - 1], "bytes after the header");
        expect_err(&wrapped[..0x100], "shorter than");

        let mut b = wrapped.clone();
        b[0x0D..0x11].copy_from_slice(&(bare_list().len() as u32 + 1).to_le_bytes());
        expect_err(&b, "header says");

        let mut b = wrapped;
        b[0x0D..0x11].copy_from_slice(&u32::MAX.to_le_bytes());
        expect_err(&b, "implausible");
    }
}
