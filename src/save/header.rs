//! `.save` file header (128 bytes).
//!
//! The header is parsed and serialized as a raw 128-byte buffer with typed
//! accessors. This preserves the unknown padding regions byte-perfectly,
//! which is required for round-trip even when none of the editable fields
//! change.

use std::io::{self, Write};

use crate::binary::{BinaryRead, BinaryWrite};

pub const HEADER_SIZE: usize = 0x80;

pub const MAGIC_OFFSET: usize = 0x00;
pub const VERSION_OFFSET: usize = 0x04;
pub const FLAGS_OFFSET: usize = 0x06;
pub const UNCOMP_SIZE_OFFSET: usize = 0x12;
pub const PAYLOAD_SIZE_OFFSET: usize = 0x16;
pub const NONCE_OFFSET: usize = 0x1A;
pub const HMAC_OFFSET: usize = 0x2A;
pub const PAYLOAD_OFFSET: usize = 0x80;

pub const MAGIC: &[u8; 4] = b"SAVE";

pub const NONCE_LEN: usize = 16;
pub const HMAC_LEN: usize = 32;

/// 128-byte `.save` header. Holds the raw bytes for round-trip and exposes
/// the dynamic fields via typed accessors.
#[derive(Debug, Clone)]
pub struct SaveHeader {
    raw: [u8; HEADER_SIZE],
}

impl SaveHeader {
    /// Construct from raw bytes; validates magic but not HMAC.
    pub fn from_bytes(bytes: [u8; HEADER_SIZE]) -> io::Result<Self> {
        if &bytes[MAGIC_OFFSET..MAGIC_OFFSET + 4] != MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "bad magic: expected 'SAVE'",
            ));
        }
        Ok(Self { raw: bytes })
    }

    pub fn as_bytes(&self) -> &[u8; HEADER_SIZE] {
        &self.raw
    }

    pub fn version(&self) -> u16 {
        u16::from_le_bytes(self.raw[VERSION_OFFSET..VERSION_OFFSET + 2].try_into().unwrap())
    }

    pub fn flags(&self) -> u16 {
        u16::from_le_bytes(self.raw[FLAGS_OFFSET..FLAGS_OFFSET + 2].try_into().unwrap())
    }

    pub fn uncompressed_size(&self) -> u32 {
        u32::from_le_bytes(
            self.raw[UNCOMP_SIZE_OFFSET..UNCOMP_SIZE_OFFSET + 4]
                .try_into()
                .unwrap(),
        )
    }

    pub fn payload_size(&self) -> u32 {
        u32::from_le_bytes(
            self.raw[PAYLOAD_SIZE_OFFSET..PAYLOAD_SIZE_OFFSET + 4]
                .try_into()
                .unwrap(),
        )
    }

    pub fn nonce(&self) -> [u8; NONCE_LEN] {
        self.raw[NONCE_OFFSET..NONCE_OFFSET + NONCE_LEN]
            .try_into()
            .unwrap()
    }

    pub fn hmac(&self) -> [u8; HMAC_LEN] {
        self.raw[HMAC_OFFSET..HMAC_OFFSET + HMAC_LEN]
            .try_into()
            .unwrap()
    }

    pub fn set_uncompressed_size(&mut self, v: u32) {
        self.raw[UNCOMP_SIZE_OFFSET..UNCOMP_SIZE_OFFSET + 4].copy_from_slice(&v.to_le_bytes());
    }

    pub fn set_payload_size(&mut self, v: u32) {
        self.raw[PAYLOAD_SIZE_OFFSET..PAYLOAD_SIZE_OFFSET + 4].copy_from_slice(&v.to_le_bytes());
    }

    pub fn set_nonce(&mut self, v: [u8; NONCE_LEN]) {
        self.raw[NONCE_OFFSET..NONCE_OFFSET + NONCE_LEN].copy_from_slice(&v);
    }

    pub fn set_hmac(&mut self, v: [u8; HMAC_LEN]) {
        self.raw[HMAC_OFFSET..HMAC_OFFSET + HMAC_LEN].copy_from_slice(&v);
    }
}

impl<'a> BinaryRead<'a> for SaveHeader {
    fn read_from(data: &'a [u8], offset: &mut usize) -> io::Result<Self> {
        if data.len() < *offset + HEADER_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "not enough bytes for save header",
            ));
        }
        let buf: [u8; HEADER_SIZE] = data[*offset..*offset + HEADER_SIZE]
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "header slice length"))?;
        *offset += HEADER_SIZE;
        Self::from_bytes(buf)
    }
}

impl BinaryWrite for SaveHeader {
    fn write_to(&self, w: &mut dyn Write) -> io::Result<()> {
        w.write_all(&self.raw)
    }
}
