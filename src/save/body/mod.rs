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
//! The full per-entry decoder (turning a `(class_index, data_offset)` pair
//! into structured records like inventory items or character stats) is the
//! next layer and isn't in this module yet — it sits in `body/object.rs`
//! when it lands. For now, schema + TOC are enough to enumerate what's in
//! a save and to plan the per-section work.

mod schema;
mod toc;

use std::io;

use schema::Schema;
use toc::Toc;

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
}
