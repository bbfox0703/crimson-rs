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
//! [`Save::parse`] / [`Save::write_with_nonce`] preserve the header bytes
//! that aren't part of the dynamic fields. With the **same nonce** an
//! unmodified save decodes back to the same body, but the file bytes
//! differ from the game's: lz4_flex does not compress like the game's
//! writer, so the compressed payload (and with it the HMAC) changes.

mod body;
mod crypto;
mod header;
mod io;

// These re-exports are public API for downstream consumers; the
// `unused_imports` allow keeps clippy quiet in builds where no in-crate code
// reaches them through the `crate::save::` path.
#[allow(unused_imports)]
pub use body::{
    Body, DecodedField, FieldKind, FieldValue, ObjectBlock, ObjectLocatorWrapper, ScalarValue,
    encode_body, encode_top_level_block, scalar_from_bytes, scalar_tag,
};
// The c_abi length-changing edit path validates caller-supplied template
// bytes by decoding them with this helper; the re-export is c_abi-gated so
// other builds don't warn about an unused import.
#[cfg(feature = "c_abi")]
pub(crate) use body::decode_one_list_element_bytes;
// The empty-element builder writes the absence markers the engine expects;
// the element-template builder also needs the scalar encoder.
#[cfg(feature = "c_abi")]
pub(crate) use body::{
    ABSENT_MARKER, FieldDef, TypeDef, absent_kind_has_marker, blocks_fully_decoded, encode_scalar,
};
pub use header::{HEADER_SIZE, SaveHeader};
pub use io::Save;
#[cfg(feature = "c_abi")]
pub use io::SaveError;
