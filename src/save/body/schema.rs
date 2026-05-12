//! Save body schema: a self-describing table of types + fields.
//!
//! Layout, starting at offset 0x0E of the decompressed body:
//!
//! ```text
//! u16 header_tag       (purpose unknown, observed nonzero)
//! u16 header_zero      (typically 0)
//! u16 type_count       (N — number of types that follow)
//!
//! u32 root_name_len
//! u8  root_name[len]   (ASCII, no NUL — the FIRST type's name)
//!
//! repeat N times:
//!     u16 field_count
//!     repeat field_count times:
//!         u32 field_name_len
//!         u8  field_name[len]
//!         u32 type_name_len
//!         u8  type_name[len]
//!         u16 meta_kind
//!         u16 meta_size
//!         u32 meta_aux
//!     if not the last type:
//!         u32 next_name_len
//!         u8  next_name[len]  (the name of the NEXT type)
//! ```
//!
//! The "interleaved name" layout — each type's name lives before its body
//! rather than as a header — matches the Python `save_parser.py`
//! implementation byte for byte. We deliberately preserve the same
//! traversal so a future round-trip writer is straightforward.

use std::io;

/// One field within a [`TypeDef`].
///
/// `meta_kind` selects the field decoder used for instance data (see the
/// upcoming object-decoder layer). The remaining `meta_size` and
/// `meta_aux` are opaque to the schema parser — they're passed through to
/// the decoder. Capturing `(start_offset, end_offset)` here lets a future
/// writer round-trip the schema without reparsing.
#[derive(Debug, Clone)]
pub struct FieldDef {
    pub name: String,
    pub type_name: String,
    pub meta_kind: u16,
    pub meta_size: u16,
    pub meta_aux: u32,
    pub start_offset: usize,
    pub end_offset: usize,
}

#[derive(Debug, Clone)]
pub struct TypeDef {
    pub index: u32,
    pub name: String,
    pub fields: Vec<FieldDef>,
    pub start_offset: usize,
    pub end_offset: usize,
}

#[derive(Debug, Clone)]
pub struct Schema {
    pub header_tag: u16,
    pub header_zero: u16,
    pub type_count: u16,
    pub root_type: String,
    pub types: Vec<TypeDef>,
    /// First byte after the schema; equals the TOC start.
    pub schema_end: usize,
}

impl Schema {
    pub fn parse(raw: &[u8]) -> io::Result<Self> {
        let mut pos = 0x0E_usize;

        let header_tag = read_u16(raw, &mut pos)?;
        let header_zero = read_u16(raw, &mut pos)?;
        let type_count = read_u16(raw, &mut pos)?;

        let root_type = read_lp_ascii(raw, &mut pos)?;

        let mut types = Vec::with_capacity(type_count as usize);
        let mut current_name = root_type.clone();

        for type_index in 0..type_count {
            let type_start = pos;
            let field_count = read_u16(raw, &mut pos)?;
            let mut fields = Vec::with_capacity(field_count as usize);
            for _ in 0..field_count {
                let field_start = pos;
                let name = read_lp_ascii(raw, &mut pos)?;
                let type_name = read_lp_ascii(raw, &mut pos)?;
                let meta_kind = read_u16(raw, &mut pos)?;
                let meta_size = read_u16(raw, &mut pos)?;
                let meta_aux = read_u32(raw, &mut pos)?;
                fields.push(FieldDef {
                    name,
                    type_name,
                    meta_kind,
                    meta_size,
                    meta_aux,
                    start_offset: field_start,
                    end_offset: pos,
                });
            }
            let type_end = pos;
            types.push(TypeDef {
                index: type_index as u32,
                name: std::mem::take(&mut current_name),
                fields,
                start_offset: type_start,
                end_offset: type_end,
            });
            if type_index != type_count - 1 {
                current_name = read_lp_ascii(raw, &mut pos)?;
            }
        }

        Ok(Schema {
            header_tag,
            header_zero,
            type_count,
            root_type,
            types,
            schema_end: pos,
        })
    }
}

fn read_u16(raw: &[u8], pos: &mut usize) -> io::Result<u16> {
    check_remaining(raw, *pos, 2)?;
    let v = u16::from_le_bytes(raw[*pos..*pos + 2].try_into().unwrap());
    *pos += 2;
    Ok(v)
}

fn read_u32(raw: &[u8], pos: &mut usize) -> io::Result<u32> {
    check_remaining(raw, *pos, 4)?;
    let v = u32::from_le_bytes(raw[*pos..*pos + 4].try_into().unwrap());
    *pos += 4;
    Ok(v)
}

/// Length-prefixed ASCII string: `u32 len + len bytes`. No NUL terminator.
/// Invalid UTF-8 is replaced with U+FFFD so a single corrupt byte doesn't
/// stop the whole parse.
fn read_lp_ascii(raw: &[u8], pos: &mut usize) -> io::Result<String> {
    let len = read_u32(raw, pos)? as usize;
    check_remaining(raw, *pos, len)?;
    let s = String::from_utf8_lossy(&raw[*pos..*pos + len]).into_owned();
    *pos += len;
    Ok(s)
}

fn check_remaining(raw: &[u8], pos: usize, need: usize) -> io::Result<()> {
    if pos.saturating_add(need) > raw.len() {
        Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            format!("need {need} bytes at offset {pos}, have {}", raw.len()),
        ))
    } else {
        Ok(())
    }
}
