//! `BuffData` common-base parser + per-entry subclass-tail probing.
//!
//! Each `BuffData` starts with a `u8 flag`. `flag != 0` is a "null entry"
//! (no payload follows). `flag == 0` is followed by a fixed-shape common
//! base (~40 fields), then a *variable-length* subclass tail keyed off
//! `type_id` (the first byte of the common base).
//!
//! The tail size is not deterministically known. The Python parser
//! brute-forces sizes 0..500 for each unseen `type_id`, looking for a
//! size where the *remainder* of the entry (other buffs + post-buff
//! fields) parses cleanly. The discovered size is cached and reused for
//! later occurrences.
//!
//! Cross-version probe (May 2026, 1.03 / 1.04 / 1.05) showed 11 type_id
//! sizes drift between versions, so the cache is built per parse, not
//! frozen at compile time.

use std::collections::HashMap;
use std::io;

use crate::binary::{BinaryRead, BinaryWrite};

/// Format flag — controls whether the common base contains an extra
/// `field_58: u8` between `field_56: u32` and `field_60: u32`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillFormat {
    /// 1.03 and earlier — no `field_58`.
    NoField58,
    /// 1.04+ — `field_58: u8` is present.
    WithField58,
}

/// Cache mapping `type_id → subclass tail size in bytes`.
pub type TailSizeCache = HashMap<u8, usize>;

/// One `BuffData` entry. Always carries the 1-byte flag; `body` is `Some`
/// for non-null entries (`flag == 0`).
#[derive(Debug, Clone)]
pub struct BuffData {
    pub flag: u8,
    pub body: Option<BuffDataBody>,
}

/// Common-base fields of a non-null `BuffData`. Field names mirror the
/// IDA-decomp names used in the GameMods Python parser to keep the
/// cross-reference tractable.
#[derive(Debug, Clone)]
pub struct BuffDataBody {
    pub type_id: u8,
    pub field_12: u32,
    pub field_16: u32,
    pub field_20: u8,
    pub field_21: u8,
    pub field_24: i64,
    pub field_32: i64,
    pub field_40: i64,
    /// `u32 len + len bytes` (no NUL terminator — same shape as iteminfo's CString).
    pub field_48: Vec<u8>,
    pub field_56: u32,
    /// 1.04+ only: `u8`. None for 1.03.
    pub field_58: Option<u8>,
    pub field_60: u32,
    pub field_62: u32,
    pub field_64: u32,
    pub field_66: u32,
    pub field_68: u8,
    pub field_69: u8,
    pub field_88: u32,
    pub field_90: u32,
    pub field_96_list: Vec<u32>,
    pub field_128: u32,
    pub field_72: u32,
    pub field_76: u32,
    pub field_80: u32,
    pub field_84: u32,
    pub field_112_list: Vec<u32>,
    pub field_132: u8,
    pub field_136: u32,
    /// Raw subclass-tail bytes — variable length, found by probing.
    pub subclass_tail: Vec<u8>,
}

/// Read the common base for a non-null `BuffData` (caller has already
/// consumed the `flag` byte and confirmed `flag == 0`). Does *not* read
/// the subclass tail — that requires probing.
pub fn read_common_base(
    data: &[u8],
    offset: &mut usize,
    format: SkillFormat,
) -> io::Result<BuffDataBody> {
    let type_id = u8::read_from(data, offset)?;
    let field_12 = u32::read_from(data, offset)?;
    let field_16 = u32::read_from(data, offset)?;
    let field_20 = u8::read_from(data, offset)?;
    let field_21 = u8::read_from(data, offset)?;
    let field_24 = i64::read_from(data, offset)?;
    let field_32 = i64::read_from(data, offset)?;
    let field_40 = i64::read_from(data, offset)?;
    let field_48_len = u32::read_from(data, offset)? as usize;
    if *offset + field_48_len > data.len() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            format!(
                "BuffData.field_48: need {} bytes at {}, have {}",
                field_48_len,
                *offset,
                data.len() - *offset
            ),
        ));
    }
    let field_48 = data[*offset..*offset + field_48_len].to_vec();
    *offset += field_48_len;

    let field_56 = u32::read_from(data, offset)?;
    let field_58 = match format {
        SkillFormat::WithField58 => Some(u8::read_from(data, offset)?),
        SkillFormat::NoField58 => None,
    };
    let field_60 = u32::read_from(data, offset)?;
    let field_62 = u32::read_from(data, offset)?;
    let field_64 = u32::read_from(data, offset)?;
    let field_66 = u32::read_from(data, offset)?;
    let field_68 = u8::read_from(data, offset)?;
    let field_69 = u8::read_from(data, offset)?;
    let field_88 = u32::read_from(data, offset)?;
    let field_90 = u32::read_from(data, offset)?;

    let cnt96 = u32::read_from(data, offset)? as usize;
    let mut field_96_list = Vec::with_capacity(cnt96);
    for _ in 0..cnt96 {
        field_96_list.push(u32::read_from(data, offset)?);
    }

    let field_128 = u32::read_from(data, offset)?;
    let field_72 = u32::read_from(data, offset)?;
    let field_76 = u32::read_from(data, offset)?;
    let field_80 = u32::read_from(data, offset)?;
    let field_84 = u32::read_from(data, offset)?;

    let cnt112 = u32::read_from(data, offset)? as usize;
    let mut field_112_list = Vec::with_capacity(cnt112);
    for _ in 0..cnt112 {
        field_112_list.push(u32::read_from(data, offset)?);
    }

    let field_132 = u8::read_from(data, offset)?;
    let field_136 = u32::read_from(data, offset)?;

    Ok(BuffDataBody {
        type_id,
        field_12,
        field_16,
        field_20,
        field_21,
        field_24,
        field_32,
        field_40,
        field_48,
        field_56,
        field_58,
        field_60,
        field_62,
        field_64,
        field_66,
        field_68,
        field_69,
        field_88,
        field_90,
        field_96_list,
        field_128,
        field_72,
        field_76,
        field_80,
        field_84,
        field_112_list,
        field_132,
        field_136,
        subclass_tail: Vec::new(),
    })
}

/// Serialise a `BuffData` (flag + optional common base + subclass tail).
pub fn write_buff_data<W: io::Write>(w: &mut W, bd: &BuffData) -> io::Result<()> {
    bd.flag.write_to(w)?;
    let Some(body) = &bd.body else {
        return Ok(());
    };
    body.type_id.write_to(w)?;
    body.field_12.write_to(w)?;
    body.field_16.write_to(w)?;
    body.field_20.write_to(w)?;
    body.field_21.write_to(w)?;
    body.field_24.write_to(w)?;
    body.field_32.write_to(w)?;
    body.field_40.write_to(w)?;
    (body.field_48.len() as u32).write_to(w)?;
    w.write_all(&body.field_48)?;
    body.field_56.write_to(w)?;
    if let Some(b) = body.field_58 {
        b.write_to(w)?;
    }
    body.field_60.write_to(w)?;
    body.field_62.write_to(w)?;
    body.field_64.write_to(w)?;
    body.field_66.write_to(w)?;
    body.field_68.write_to(w)?;
    body.field_69.write_to(w)?;
    body.field_88.write_to(w)?;
    body.field_90.write_to(w)?;
    (body.field_96_list.len() as u32).write_to(w)?;
    for v in &body.field_96_list {
        v.write_to(w)?;
    }
    body.field_128.write_to(w)?;
    body.field_72.write_to(w)?;
    body.field_76.write_to(w)?;
    body.field_80.write_to(w)?;
    body.field_84.write_to(w)?;
    (body.field_112_list.len() as u32).write_to(w)?;
    for v in &body.field_112_list {
        v.write_to(w)?;
    }
    body.field_132.write_to(w)?;
    body.field_136.write_to(w)?;
    w.write_all(&body.subclass_tail)?;
    Ok(())
}
