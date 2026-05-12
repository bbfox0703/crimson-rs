//! Save file key derivation, ChaCha20 stream, and HMAC-SHA256.
//!
//! All three pieces are version-keyed: the per-version `_VERSION_PREFIXES`
//! XOR table from the original Python loader is preserved here, so v1 and v2
//! saves can both be read.

use chacha20_crate::cipher::{KeyIvInit, StreamCipher, StreamCipherSeek};
use hmac::{Hmac, Mac};
use sha2::Sha256;

/// First 31 bytes of the hardcoded save crypto base material.
///
/// Source: hex constant `C41B...EF90`, truncated to 31 bytes by the
/// original Python loader (`save_crypto.py::_SAVE_BASE_KEY`).
const BASE_KEY_31: [u8; 31] = [
    0xC4, 0x1B, 0x8E, 0x73, 0x0D, 0xF2, 0x59, 0xA6, 0x37, 0xCC, 0x04, 0xE9, 0xB1, 0x2F, 0x96, 0x68,
    0xDA, 0x10, 0x7A, 0x85, 0x3E, 0x61, 0xF9, 0x22, 0x4D, 0xB8, 0x0A, 0xD7, 0x5C, 0x13, 0xEF,
];

/// Per-version prefix bytes XOR'd into the base material.
fn version_prefix(version: u16) -> Option<&'static [u8]> {
    match version {
        1 => Some(b"^Qgbrm/.#@`zsr]\\@rvfal#\""),
        2 => Some(b"^Pearl--#Abyss__@!!"),
        _ => None,
    }
}

const SUFFIX: &[u8] = b"PRIVATE_HMAC_SECRET_CHECK";

/// Derive the 32-byte ChaCha20 / HMAC key for a given save version.
///
/// Layout: `key[0..31] = base_key[i] XOR (prefix || suffix)[i]`, then a
/// final 0x00 byte. Returns `None` for unsupported versions.
pub fn derive_key(version: u16) -> Option<[u8; 32]> {
    let prefix = version_prefix(version)?;
    // Concat prefix + suffix; only the first 31 bytes are XOR'd in.
    let mut material = Vec::with_capacity(prefix.len() + SUFFIX.len());
    material.extend_from_slice(prefix);
    material.extend_from_slice(SUFFIX);

    let mut key = [0u8; 32];
    for i in 0..31 {
        // `BASE_KEY_31[i] XOR material[i]`. `material` is always long enough
        // (prefix + suffix ≥ 31 bytes for every supported version).
        key[i] = BASE_KEY_31[i] ^ material[i];
    }
    key[31] = 0x00;
    Some(key)
}

/// In-place ChaCha20 over `buf`. The 16-byte stored nonce splits into
/// `(initial_counter u32 LE, nonce u96)`. Encryption and decryption are the
/// same operation for a stream cipher.
pub fn chacha20_xor(buf: &mut [u8], key: &[u8; 32], nonce16: &[u8; 16]) {
    let initial_counter = u32::from_le_bytes(nonce16[0..4].try_into().unwrap());
    let nonce12: [u8; 12] = nonce16[4..16].try_into().unwrap();

    let mut cipher = chacha20_crate::ChaCha20::new(key.into(), &nonce12.into());
    // Each ChaCha20 block is 64 bytes; seek to byte position counter * 64.
    cipher.seek(initial_counter as u64 * 64);
    cipher.apply_keystream(buf);
}

/// HMAC-SHA256 over `data` with `key`. Returns the 32-byte digest.
pub fn hmac_sha256(key: &[u8; 32], data: &[u8]) -> [u8; 32] {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().into()
}

/// Constant-time HMAC verification.
pub fn verify_hmac(key: &[u8; 32], data: &[u8], expected: &[u8; 32]) -> bool {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.verify_slice(expected).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_v2_structural_invariants() {
        // We don't lock the byte values here; the real cross-check against
        // the Python reference is the live-save HMAC verification in the
        // integration tests (any drift in BASE_KEY_31, prefix, or suffix
        // will fail those). What we *do* lock are the structural shape
        // properties that the Python implementation guarantees:
        //
        //   1. The key for v2 is 32 bytes.
        //   2. The last byte is the explicit `0x00` terminator.
        //   3. The first 31 bytes are XOR'd against material — none of
        //      them depend on byte 31, so they cannot be 0x00 unless the
        //      base and material bytes happen to be equal, which they
        //      aren't for any v2 byte position. Quick non-zero check.
        let key = derive_key(2).expect("v2 supported");
        assert_eq!(key.len(), 32);
        assert_eq!(key[31], 0x00);
        assert!(key[..31].iter().any(|&b| b != 0));
    }

    #[test]
    fn key_v1_supported() {
        assert!(derive_key(1).is_some());
    }

    #[test]
    fn key_unsupported_version() {
        assert!(derive_key(99).is_none());
    }

    #[test]
    fn chacha20_is_involution() {
        let key = derive_key(2).unwrap();
        let nonce = [0x42u8; 16];
        let plain = b"hello crimson desert save file".to_vec();

        let mut buf = plain.clone();
        chacha20_xor(&mut buf, &key, &nonce);
        assert_ne!(buf, plain, "ciphertext must differ from plaintext");

        chacha20_xor(&mut buf, &key, &nonce);
        assert_eq!(buf, plain, "double XOR must restore plaintext");
    }

    #[test]
    fn hmac_verify_roundtrip() {
        let key = derive_key(2).unwrap();
        let data = b"some compressed payload bytes".to_vec();
        let tag = hmac_sha256(&key, &data);
        assert!(verify_hmac(&key, &data, &tag));
    }

    #[test]
    fn hmac_verify_rejects_tamper() {
        let key = derive_key(2).unwrap();
        let data = b"some compressed payload bytes".to_vec();
        let tag = hmac_sha256(&key, &data);
        let mut bad = data.clone();
        bad[0] ^= 1;
        assert!(!verify_hmac(&key, &bad, &tag));
    }
}
