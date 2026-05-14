//! Decompressed save body parser.
//!
//! The body has three sequential sections, in this order:
//!
//! 1. **Prefix** (14 bytes from offset 0). The first 4 bytes are the magic
//!    `0xFF 0xFF 0x04 0x00`. The remaining 10 bytes are preserved verbatim
//!    for byte-perfect round-trip but not interpreted here.
//!
//! 2. **Schema** (from offset 0x0E). A self-describing table of types, each
//!    with named fields and a 3-tuple `(meta_kind, meta_size, meta_aux)`
//!    that tells the field decoder how to read instances of that type from
//!    the data section. See [`schema`].
//!
//! 3. **TOC + data**. After the schema, a 12-byte TOC header plus 20-byte
//!    entries point into the data section. Each entry names a class index
//!    into the schema's types array plus a `(data_offset, data_size)`
//!    region inside the body. See [`toc`].
//!
//! The per-entry field decoder (turning a `(class_index, data_offset)` pair
//! into structured records like inventory items or character stats) lives
//! in [`decoder`] and the typed output in [`object`]. Use
//! [`Body::decode_blocks`] to run it.

mod decoder;
mod encoder;
mod object;
mod schema;
mod toc;

use std::io;

pub use encoder::{encode_body, encode_top_level_block};
pub use object::{
    DecodedField, FieldKind, FieldValue, ObjectBlock, ObjectLocatorWrapper, ScalarValue,
};
pub(crate) use schema::Schema;
pub(crate) use toc::Toc;

/// Bytes 0..4 of a decompressed save body.
pub const BODY_MAGIC: [u8; 4] = [0xFF, 0xFF, 0x04, 0x00];

/// Bytes 0..0x0E of a decompressed save body. The first 4 bytes are
/// [`BODY_MAGIC`]; the next 10 bytes are an opaque header we preserve
/// verbatim for byte-perfect round-trip.
pub const PREFIX_SIZE: usize = 0x0E;

/// A parsed save body: schema + TOC. Holds enough info to enumerate the
/// per-section data but does not decode individual objects.
#[derive(Debug, Clone)]
pub struct Body {
    /// Raw 14 bytes from offset 0. Includes the magic.
    pub prefix: [u8; PREFIX_SIZE],
    pub schema: Schema,
    pub toc: Toc,
}

impl Body {
    /// Parse the schema + TOC portion of a decompressed save body.
    ///
    /// The body's data section is **not** decoded by this call; that's the
    /// next layer. Errors are structural (bad magic, truncated header).
    pub fn parse(raw: &[u8]) -> io::Result<Self> {
        if raw.len() < PREFIX_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "body too small for prefix",
            ));
        }
        if raw[..4] != BODY_MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("bad body magic: {:02X?}", &raw[..4]),
            ));
        }

        let mut prefix = [0u8; PREFIX_SIZE];
        prefix.copy_from_slice(&raw[..PREFIX_SIZE]);

        let schema = Schema::parse(raw)?;
        let toc = Toc::parse(raw, schema.schema_end)?;

        Ok(Body { prefix, schema, toc })
    }

    /// Decode every TOC entry whose `class_index` resolves to a schema
    /// type. The returned blocks are independent of `self` — they own
    /// their decoded data. `raw` must be the same bytes [`Body::parse`]
    /// was called with.
    pub fn decode_blocks(&self, raw: &[u8]) -> Vec<ObjectBlock> {
        decoder::decode_blocks(raw, &self.schema, &self.toc)
    }

    /// Re-emit the body as bytes, recomputing the TOC offsets from the
    /// supplied `blocks`. `raw` must be the original body the schema was
    /// parsed from — only the prefix + schema region (`raw[0..schema_end]`)
    /// is reused verbatim; the TOC header + entries + data section are
    /// recomputed from scratch.
    ///
    /// For unmodified blocks (round-trip), the output equals `raw`
    /// byte-for-byte. For mutated blocks (after a length-changing edit)
    /// the data section grows / shrinks accordingly and TOC offsets +
    /// `stream_size` reflect the new layout.
    ///
    /// `allow(dead_code)` until Phase B.2 wires a C ABI consumer; the
    /// golden round-trip tests exercise this path via `cfg(test)` code.
    #[allow(dead_code)]
    pub fn write(&self, raw: &[u8], blocks: &[ObjectBlock]) -> io::Result<Vec<u8>> {
        if raw.len() < self.schema.schema_end {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "raw len {} < schema_end {}",
                    raw.len(),
                    self.schema.schema_end
                ),
            ));
        }
        encoder::encode_body(self, &raw[..self.schema.schema_end], blocks)
    }
}
