//! Save body Table of Contents (TOC).
//!
//! Sits immediately after the schema. The TOC is the index that maps each
//! type instance to its `(data_offset, data_size)` slice inside the body's
//! data section.
//!
//! ```text
//! u32 prefix_zero    (typically 0)
//! u32 toc_count      (N)
//! u32 stream_size    (total data section size, in bytes)
//!
//! repeat N times (20 bytes each):
//!     u32 class_index     (index into schema.types)
//!     u32 sentinel1       (validation marker, often 0)
//!     u32 sentinel2       (validation marker, often 0)
//!     u32 data_offset     (absolute offset inside the body)
//!     u32 data_size       (length of this entry's data, in bytes)
//! ```

use std::io;

const TOC_ENTRY_SIZE: usize = 5 * 4;
const TOC_HEADER_SIZE: usize = 3 * 4;

#[derive(Debug, Clone)]
pub struct TocEntry {
    pub index: u32,
    pub class_index: u32,
    pub sentinel1: u32,
    pub sentinel2: u32,
    pub data_offset: u32,
    pub data_size: u32,
    /// Byte offset where this entry begins in the body. Useful for a
    /// future writer that needs to reproduce the same layout.
    pub entry_offset: usize,
}

#[derive(Debug, Clone)]
pub struct Toc {
    pub prefix_zero: u32,
    pub toc_count: u32,
    pub stream_size: u32,
    pub entries: Vec<TocEntry>,
}

impl Toc {
    pub fn parse(raw: &[u8], schema_end: usize) -> io::Result<Self> {
        if schema_end + TOC_HEADER_SIZE > raw.len() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!(
                    "TOC header needs {TOC_HEADER_SIZE} bytes at offset {schema_end}, have {}",
                    raw.len()
                ),
            ));
        }

        let prefix_zero = read_u32(raw, schema_end);
        let toc_count = read_u32(raw, schema_end + 4);
        let stream_size = read_u32(raw, schema_end + 8);

        let toc_start = schema_end + TOC_HEADER_SIZE;
        let mut entries = Vec::with_capacity(toc_count as usize);
        for index in 0..toc_count {
            let entry_off = toc_start + (index as usize) * TOC_ENTRY_SIZE;
            if entry_off + TOC_ENTRY_SIZE > raw.len() {
                // Match Python: short TOC tolerated, partial entries skipped.
                break;
            }
            entries.push(TocEntry {
                index,
                class_index: read_u32(raw, entry_off),
                sentinel1: read_u32(raw, entry_off + 4),
                sentinel2: read_u32(raw, entry_off + 8),
                data_offset: read_u32(raw, entry_off + 12),
                data_size: read_u32(raw, entry_off + 16),
                entry_offset: entry_off,
            });
        }

        Ok(Toc {
            prefix_zero,
            toc_count,
            stream_size,
            entries,
        })
    }

}

#[inline]
fn read_u32(raw: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(raw[at..at + 4].try_into().unwrap())
}
