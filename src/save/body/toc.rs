//! Save body Table of Contents (TOC).
//!
//! Sits immediately after the schema. The TOC is the index that maps each
//! type instance to its `(data_offset, data_size)` slice inside the body's
//! data section.
//!
//! ```text
//! u32 prefix_zero    (typically 0)
//! u32 toc_count      (N)
//! u32 stream_size    (absolute end of the data section: equals the body
//!                     length on every save measured, and the encoder writes it so)
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
    /// Byte offset of this entry's 20-byte TOC record in the body.
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
        // The count is untrusted: a TOC the body cannot hold is an error, not
        // a count-sized allocation (0xFFFFFFFF would ask for ~128 GiB)
        // followed by entries read out of the data section.
        let room = (raw.len() - toc_start) / TOC_ENTRY_SIZE;
        if toc_count as usize > room {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("TOC count {toc_count} at offset {schema_end} needs more than the {room} entries the body holds"),
            ));
        }
        let mut entries = Vec::with_capacity(toc_count as usize);
        for index in 0..toc_count {
            let entry_off = toc_start + (index as usize) * TOC_ENTRY_SIZE;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A count the body cannot hold is an error, not a ~128 GiB allocation.
    #[test]
    fn toc_count_must_fit_the_body() {
        let mut raw = vec![0u8; 4];
        raw.extend_from_slice(&0u32.to_le_bytes());
        raw.extend_from_slice(&u32::MAX.to_le_bytes());
        raw.extend_from_slice(&0u32.to_le_bytes());
        raw.extend_from_slice(&[0; TOC_ENTRY_SIZE * 2]);
        let err = Toc::parse(&raw, 4).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData, "{err}");
        raw[8..12].copy_from_slice(&2u32.to_le_bytes());
        assert_eq!(Toc::parse(&raw, 4).unwrap().entries.len(), 2);
    }
}
