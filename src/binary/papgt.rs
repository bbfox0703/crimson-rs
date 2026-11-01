use std::io::{self, Write};

#[allow(unused_imports)]
use super::trie::read_cstring;
use super::{BinaryRead, BinaryWrite, check_remaining};
use crate::binary_struct;
use crate::crypto::checksum;

// i32 is used for group_names_buffer length prefix

// ── Language flags ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguageType(pub u16);

#[allow(dead_code)]
impl LanguageType {
    pub const KOR: u16 = 1 << 0;
    pub const ENG: u16 = 1 << 1;
    pub const JPN: u16 = 1 << 2;
    pub const RUS: u16 = 1 << 3;
    pub const TUR: u16 = 1 << 4;
    pub const SPA_ES: u16 = 1 << 5;
    pub const SPA_MX: u16 = 1 << 6;
    pub const FRE: u16 = 1 << 7;
    pub const GER: u16 = 1 << 8;
    pub const ITA: u16 = 1 << 9;
    pub const POL: u16 = 1 << 10;
    pub const POR_BR: u16 = 1 << 11;
    pub const ZHO_TW: u16 = 1 << 12;
    pub const ZHO_CN: u16 = 1 << 13;
    pub const ARA: u16 = 1 << 14;
    pub const ALL: u16 = 0x7FFF;
}

impl<'a> BinaryRead<'a> for LanguageType {
    fn read_from(data: &'a [u8], offset: &mut usize) -> io::Result<Self> {
        Ok(LanguageType(u16::read_from(data, offset)?))
    }
}

impl BinaryWrite for LanguageType {
    fn write_to(&self, w: &mut dyn Write) -> io::Result<()> {
        self.0.write_to(w)
    }
}

// ── Structs ────────────────────────────────────────────────────────────────

binary_struct! {
    pub struct PackGroupTreeMetaHeader {
        pub unknown0: u32,
        pub checksum: u32,
        pub entry_count: u8,
        pub unknown1: u8,
        pub unknown2: u16,
    }
}

binary_struct! {
    pub struct PackGroupTreeMetaEntry {
        pub is_optional: u8,
        pub language: LanguageType,
        pub always_zero: u8,
        pub group_name_offset: u32,
        pub pack_meta_checksum: u32,
    }
}

// ── Resolved entry (with group name string) ────────────────────────────────

#[derive(Debug)]
pub struct ResolvedEntry {
    pub group_name: String,
    pub entry: PackGroupTreeMetaEntry,
}

// ── Top-level PAPGT ────────────────────────────────────────────────────────

/// Length of the signature-sized trailer closing a 2.04+ PAPGT.
pub const PAPGT_TRAILER_SIZE: usize = 64;

/// Layout of a 2.04+ PAPGT:
///
/// ```text
/// u16 unknown0, u8 entry_count, u8 unknown1 (2)
/// entries, group names buffer   (unchanged)
/// [u8; 64] trailer
/// ```
///
/// There is no Jenkins checksum any more. The trailer is high-entropy and no
/// plain hash of the body, and `meta/0.papk` sits next to it, so it is treated
/// as a signature: it is carried verbatim, and it cannot be recomputed here.
/// Told apart from the older layout by its section lengths adding up exactly.
fn is_signed_layout(data: &[u8]) -> bool {
    let Some(&entry_count) = data.get(2) else {
        return false;
    };
    let names_len_at = 4 + entry_count as usize * 12;
    let Some(names_len) = data.get(names_len_at..names_len_at + 4) else {
        return false;
    };
    let names_len = u32::from_le_bytes(names_len.try_into().unwrap()) as usize;
    names_len_at + 4 + names_len + PAPGT_TRAILER_SIZE == data.len()
}

#[derive(Debug)]
pub struct PackGroupTreeMeta {
    /// 2.04+ fills `unknown0` (low 16 bits), `entry_count` and `unknown1`;
    /// `checksum` and `unknown2` are ≤ 2.03 fields and stay 0.
    pub header: PackGroupTreeMetaHeader,
    pub entries: Vec<ResolvedEntry>,
    /// Raw group names buffer (for roundtrip writing).
    pub group_names_buffer: Vec<u8>,
    /// The 2.04+ closing trailer; `None` in a ≤ 2.03 PAPGT. Editing the entries
    /// does not refresh it — see [`Self::add_entry`].
    pub trailer: Option<[u8; PAPGT_TRAILER_SIZE]>,
}

impl PackGroupTreeMeta {
    /// Create a new empty PAPGT.
    #[allow(dead_code)]
    pub fn new() -> Self {
        PackGroupTreeMeta {
            header: PackGroupTreeMetaHeader {
                unknown0: 0,
                checksum: 0,
                entry_count: 0,
                unknown1: 0,
                unknown2: 0,
            },
            entries: Vec::new(),
            group_names_buffer: Vec::new(),
            trailer: None,
        }
    }

    /// Parse a PAPGT from raw bytes (the entire file contents), either layout.
    pub fn parse(data: &[u8]) -> io::Result<Self> {
        let mut offset = 0;
        let header;
        let mut trailer = None;
        if is_signed_layout(data) {
            header = PackGroupTreeMetaHeader {
                unknown0: u16::from_le_bytes([data[0], data[1]]) as u32,
                checksum: 0,
                entry_count: data[2],
                unknown1: data[3],
                unknown2: 0,
            };
            offset = 4;
            let tail = &data[data.len() - PAPGT_TRAILER_SIZE..];
            trailer = Some(tail.try_into().unwrap());
        } else {
            header = PackGroupTreeMetaHeader::read_from(data, &mut offset)?;

            // Everything after the header is checksummed
            let post_header_data = &data[offset..];
            checksum::validate_checksum(post_header_data, header.checksum)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        }

        // Read entries
        let mut raw_entries = Vec::with_capacity(header.entry_count as usize);
        for _ in 0..header.entry_count {
            raw_entries.push(PackGroupTreeMetaEntry::read_from(data, &mut offset)?);
        }

        // Read group names buffer: i32 length prefix + data
        check_remaining(data, offset, 4)?;
        let buf_len = i32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        check_remaining(data, offset, buf_len)?;
        let group_names_buffer = data[offset..offset + buf_len].to_vec();

        // Resolve group names
        let mut entries = Vec::with_capacity(raw_entries.len());
        for entry in raw_entries {
            let group_name = read_cstring(&group_names_buffer, entry.group_name_offset as usize)?;
            entries.push(ResolvedEntry { group_name, entry });
        }

        Ok(PackGroupTreeMeta {
            header,
            entries,
            group_names_buffer,
            trailer,
        })
    }

    /// Add or update an entry in this PAPGT (upsert), placed at the front.
    ///
    /// Mod entries are inserted at position 0 so they take priority over
    /// original game entries (matches other mod loaders' behavior).
    /// If an entry with the same `group_name` already exists, it is updated
    /// and moved to the front.
    /// Call `to_bytes()` afterwards to get the serialized form with recalculated checksum.
    ///
    /// A 2.04+ PAPGT keeps its old [`Self::trailer`], which no longer matches
    /// the edited entries; whether the game accepts that is not established.
    pub fn add_entry(
        &mut self,
        group_name: &str,
        pack_meta_checksum: u32,
        is_optional: u8,
        language: u16,
    ) {
        // Remove existing entry with the same group name (if any)
        self.entries.retain(|e| e.group_name != group_name);

        // Prepend the group name to the front of the names buffer
        let name_bytes = group_name.as_bytes();
        let insert_len = name_bytes.len() + 1; // +1 for null terminator
        let mut new_buffer = Vec::with_capacity(insert_len + self.group_names_buffer.len());
        new_buffer.extend_from_slice(name_bytes);
        new_buffer.push(0); // null terminator
        new_buffer.extend_from_slice(&self.group_names_buffer);
        self.group_names_buffer = new_buffer;

        // Shift all existing entries' group_name_offset forward
        for entry in &mut self.entries {
            entry.entry.group_name_offset += insert_len as u32;
        }

        self.entries.insert(
            0,
            ResolvedEntry {
                group_name: group_name.to_string(),
                entry: PackGroupTreeMetaEntry {
                    is_optional,
                    language: LanguageType(language),
                    always_zero: 0,
                    group_name_offset: 0, // at the front of the buffer
                    pack_meta_checksum,
                },
            },
        );

        self.header.entry_count = self.entries.len() as u8;
    }

    /// Serialize back to bytes (roundtrip).
    pub fn to_bytes(&self) -> io::Result<Vec<u8>> {
        // First, serialize the post-header data
        let mut post_header = Vec::new();

        for resolved in &self.entries {
            resolved.entry.write_to(&mut post_header)?;
        }

        // Write group names buffer with i32 length prefix
        (self.group_names_buffer.len() as i32).write_to(&mut post_header)?;
        post_header.write_all(&self.group_names_buffer)?;

        if let Some(trailer) = &self.trailer {
            let mut out = Vec::with_capacity(4 + post_header.len() + PAPGT_TRAILER_SIZE);
            out.extend_from_slice(&(self.header.unknown0 as u16).to_le_bytes());
            out.push(self.header.entry_count);
            out.push(self.header.unknown1);
            out.write_all(&post_header)?;
            out.write_all(trailer)?;
            return Ok(out);
        }

        // Now build the full output with recalculated checksum
        let computed_checksum = checksum::calculate_checksum(&post_header);

        let mut out = Vec::new();
        // Write header with updated checksum
        let header = PackGroupTreeMetaHeader {
            checksum: computed_checksum,
            ..PackGroupTreeMetaHeader {
                unknown0: self.header.unknown0,
                checksum: computed_checksum,
                entry_count: self.header.entry_count,
                unknown1: self.header.unknown1,
                unknown2: self.header.unknown2,
            }
        };
        header.write_to(&mut out)?;
        out.write_all(&post_header)?;

        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signed_papgt() -> Vec<u8> {
        let mut data = vec![0xc9, 0x16, 2, 2];
        for (name_offset, checksum) in [(0u32, 0x11u32), (5, 0x22)] {
            data.extend_from_slice(&[0, 0xff, 0x7f, 0]);
            data.extend_from_slice(&name_offset.to_le_bytes());
            data.extend_from_slice(&checksum.to_le_bytes());
        }
        data.extend_from_slice(&10u32.to_le_bytes());
        data.extend_from_slice(b"0000\x000001\x00");
        data.extend_from_slice(&[0xAB; PAPGT_TRAILER_SIZE]);
        data
    }

    #[test]
    fn signed_layout_roundtrips() {
        let data = signed_papgt();
        let papgt = PackGroupTreeMeta::parse(&data).unwrap();
        assert_eq!(papgt.header.unknown0, 0x16c9);
        assert_eq!(papgt.entries.len(), 2);
        assert_eq!(papgt.entries[1].group_name, "0001");
        assert_eq!(papgt.entries[1].entry.pack_meta_checksum, 0x22);
        assert_eq!(papgt.trailer, Some([0xAB; PAPGT_TRAILER_SIZE]));
        assert_eq!(papgt.to_bytes().unwrap(), data);
    }

    #[test]
    fn signed_layout_survives_add_entry() {
        let mut papgt = PackGroupTreeMeta::parse(&signed_papgt()).unwrap();
        papgt.add_entry("0036", 0x33, 0, LanguageType::ALL);
        let reparsed = PackGroupTreeMeta::parse(&papgt.to_bytes().unwrap()).unwrap();
        assert_eq!(reparsed.entries.len(), 3);
        assert_eq!(reparsed.entries[0].group_name, "0036");
        assert_eq!(reparsed.entries[2].group_name, "0001");
    }

    #[test]
    fn checksummed_layout_roundtrips() {
        let mut papgt = PackGroupTreeMeta::new();
        papgt.add_entry("0000", 0x11, 0, LanguageType::ALL);
        let data = papgt.to_bytes().unwrap();
        let reparsed = PackGroupTreeMeta::parse(&data).unwrap();
        assert_eq!(reparsed.trailer, None);
        assert_eq!(reparsed.entries[0].group_name, "0000");
        assert_eq!(reparsed.to_bytes().unwrap(), data);
    }

    #[test]
    fn garbage_is_still_a_checksum_error() {
        let err = PackGroupTreeMeta::parse(&[0x55; 40]).unwrap_err();
        assert!(err.to_string().to_lowercase().contains("checksum"), "{err}");
    }
}
