//! Top-level save read / write: decrypt + decompress on load,
//! compress + encrypt + reseal on write.

use std::fmt;
use std::io;

use super::crypto::{chacha20_xor, derive_key, hmac_sha256, verify_hmac};
use super::header::{HEADER_SIZE, NONCE_LEN, PAYLOAD_OFFSET, SaveHeader};

/// A parsed save file.
///
/// `header` is the 128-byte file header (preserved byte-perfectly except
/// for the dynamic fields). `body` is the decompressed payload — this is
/// the editable, format-specific blob that the higher-level parser reads.
#[derive(Debug, Clone)]
pub struct Save {
    pub header: SaveHeader,
    pub body: Vec<u8>,
    /// Result of HMAC verification at load time. The body is still returned
    /// even if `hmac_ok` is `false`, matching the behavior of the original
    /// Python loader (which raises a warning rather than failing).
    pub hmac_ok: bool,
}

/// Errors specific to save loading / saving.
#[derive(Debug)]
pub enum SaveError {
    Io(io::Error),
    /// File is too small to hold a header + at least one body byte.
    TooSmall { len: usize, need: usize },
    /// Magic at offset 0 is not `b"SAVE"`.
    BadMagic([u8; 4]),
    /// `payload_size` field points past the end of the file.
    PayloadOutOfRange { offset: usize, size: u32, file_len: usize },
    /// `version` field in the header isn't one we know how to key.
    UnsupportedVersion(u16),
    /// LZ4 decompressed length didn't match the header's `uncomp_size`.
    DecompressSizeMismatch { expected: u32, actual: usize },
    /// LZ4 reported a decode error.
    Lz4(lz4_flex::block::DecompressError),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::Io(e) => write!(f, "io error: {e}"),
            SaveError::TooSmall { len, need } => {
                write!(f, "file too small: {len} bytes, need at least {need}")
            }
            SaveError::BadMagic(m) => write!(f, "bad magic: {m:?}"),
            SaveError::PayloadOutOfRange { offset, size, file_len } => write!(
                f,
                "payload [{offset}..{}] exceeds file size {file_len}",
                offset + *size as usize
            ),
            SaveError::UnsupportedVersion(v) => write!(f, "unsupported save version: {v}"),
            SaveError::DecompressSizeMismatch { expected, actual } => write!(
                f,
                "lz4 decompressed {actual} bytes, header claims {expected}"
            ),
            SaveError::Lz4(e) => write!(f, "lz4 decompress error: {e}"),
        }
    }
}

impl std::error::Error for SaveError {}

impl From<io::Error> for SaveError {
    fn from(value: io::Error) -> Self {
        SaveError::Io(value)
    }
}

impl Save {
    /// Parse a save from the full file bytes.
    ///
    /// Returns the decoded body even when HMAC verification fails (see
    /// [`Save::hmac_ok`]). Returns an error only for structural issues.
    pub fn parse(file_bytes: &[u8]) -> Result<Self, SaveError> {
        if file_bytes.len() < HEADER_SIZE {
            return Err(SaveError::TooSmall {
                len: file_bytes.len(),
                need: HEADER_SIZE,
            });
        }
        if &file_bytes[0..4] != b"SAVE" {
            return Err(SaveError::BadMagic([
                file_bytes[0],
                file_bytes[1],
                file_bytes[2],
                file_bytes[3],
            ]));
        }

        let mut hdr_buf = [0u8; HEADER_SIZE];
        hdr_buf.copy_from_slice(&file_bytes[..HEADER_SIZE]);
        let header = SaveHeader::from_bytes(hdr_buf).map_err(SaveError::Io)?;

        let version = header.version();
        let key = derive_key(version).ok_or(SaveError::UnsupportedVersion(version))?;

        let payload_size = header.payload_size() as usize;
        let payload_end = PAYLOAD_OFFSET
            .checked_add(payload_size)
            .ok_or(SaveError::PayloadOutOfRange {
                offset: PAYLOAD_OFFSET,
                size: header.payload_size(),
                file_len: file_bytes.len(),
            })?;
        if payload_end > file_bytes.len() {
            return Err(SaveError::PayloadOutOfRange {
                offset: PAYLOAD_OFFSET,
                size: header.payload_size(),
                file_len: file_bytes.len(),
            });
        }

        let nonce = header.nonce();
        let stored_hmac = header.hmac();

        // ChaCha20 decrypt in place. After this `compressed` holds the
        // post-decryption (pre-decompression) blob.
        let mut compressed = file_bytes[PAYLOAD_OFFSET..payload_end].to_vec();
        chacha20_xor(&mut compressed, &key, &nonce);

        let hmac_ok = verify_hmac(&key, &compressed, &stored_hmac);

        let uncomp_size = header.uncompressed_size() as usize;
        let body = lz4_flex::block::decompress(&compressed, uncomp_size)
            .map_err(SaveError::Lz4)?;

        if body.len() != uncomp_size {
            return Err(SaveError::DecompressSizeMismatch {
                expected: header.uncompressed_size(),
                actual: body.len(),
            });
        }

        Ok(Save { header, body, hmac_ok })
    }

    /// Serialize the save back to bytes, using the supplied nonce.
    ///
    /// Re-runs LZ4 compression on `self.body`, computes a fresh HMAC and
    /// nonce-derived ciphertext, and updates the header's `uncomp_size`,
    /// `payload_size`, `nonce`, and `hmac` fields. All other header bytes
    /// (magic, version, flags, the two padding regions) are preserved
    /// from the in-memory header.
    ///
    /// For byte-perfect round-trip of an unmodified save, pass the
    /// original header's `nonce()` here.
    pub fn write_with_nonce(&self, nonce: [u8; NONCE_LEN]) -> Result<Vec<u8>, SaveError> {
        let key = derive_key(self.header.version())
            .ok_or(SaveError::UnsupportedVersion(self.header.version()))?;

        // LZ4 block compression. Use the same high-compression level as
        // the Python writer (compression=9, store_size=False).
        let compressed = lz4_flex::block::compress(&self.body);

        let tag = hmac_sha256(&key, &compressed);
        let mut encrypted = compressed.clone();
        chacha20_xor(&mut encrypted, &key, &nonce);

        let mut header = self.header.clone();
        header.set_uncompressed_size(self.body.len() as u32);
        header.set_payload_size(compressed.len() as u32);
        header.set_nonce(nonce);
        header.set_hmac(tag);

        let mut out = Vec::with_capacity(HEADER_SIZE + encrypted.len());
        out.extend_from_slice(header.as_bytes());
        out.extend_from_slice(&encrypted);
        Ok(out)
    }
}
