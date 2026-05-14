//! `.save` file format for Crimson Desert player saves.
//!
//! Layout of one `save.save` (or `lobby.save`):
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────────────┐
//! │ 0x00  magic        "SAVE"  (4 bytes)                               │
//! │ 0x04  version      u16 LE  (1 or 2)                                │
//! │ 0x06  flags        u16 LE  (observed 0x0080 for v2)                │
//! │ 0x08  …unknown…    10 bytes — preserved verbatim for roundtrip     │
//! │ 0x12  uncomp_size  u32 LE  — size of decompressed payload          │
//! │ 0x16  payload_size u32 LE  — size of compressed+encrypted payload  │
//! │ 0x1A  nonce        16 bytes — ChaCha20 nonce (counter || nonce12)  │
//! │ 0x2A  hmac         32 bytes — HMAC-SHA256 over compressed bytes    │
//! │ 0x4A  …padding…    54 bytes — preserved verbatim for roundtrip     │
//! │ 0x80  payload      `payload_size` bytes of ChaCha20-encrypted +    │
//! │                    LZ4-compressed body                             │
//! └────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Crypto
//!
//! - **Key**: derived per-version. `base_key[:31] XOR (version_prefix +
//!   "PRIVATE_HMAC_SECRET_CHECK")[:31]`, then append a single `0x00` byte
//!   to make a 32-byte ChaCha20 key. The same key is reused for the HMAC.
//! - **ChaCha20**: 16-byte stored nonce splits into `(initial_counter u32 LE,
//!   nonce u96)`. Standard IETF ChaCha20, seeked to `initial_counter * 64`.
//! - **HMAC**: HMAC-SHA256 over the **compressed (post-decryption)** blob.
//! - **Compression**: LZ4 block format (not frame), `uncomp_size` known
//!   externally from the header.
//!
//! ## Roundtrip
//!
//! [`Save::parse`] / [`Save::write`] preserve the header bytes that aren't
//! part of the dynamic fields. With the **same nonce** an unmodified save
//! round-trips byte-for-byte (subject to LZ4 determinism for identical input).

mod body;
mod crypto;
mod header;
mod io;

// `ObjectLocatorWrapper` and `encode_body` are part of the public API
// surface used by downstream C ABI / Python consumers; the `unused_imports`
// allow keeps clippy quiet when no in-crate code happens to reference them
// via the `crate::save::` path.
#[allow(unused_imports)]
pub use body::{
    Body, DecodedField, FieldKind, FieldValue, ObjectBlock, ObjectLocatorWrapper, ScalarValue,
    encode_body, encode_top_level_block,
};
pub use header::{HEADER_SIZE, SaveHeader};
pub use io::Save;
#[cfg(feature = "c_abi")]
pub use io::SaveError;
