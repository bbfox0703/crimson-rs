//! `BuffData` common-base parser + per-entry subclass-tail probing.
//!
//! Each `BuffData` starts with a `u8 flag`. `flag != 0` is a "null entry"
//! (no payload follows). `flag == 0` is followed by a fixed-shape common
//! base (~40 fields), then a *variable-length* subclass tail keyed off
//! `type_id` (the first byte of the common base).
//!
//! The tail size is not deterministically known. The loader
//! brute-forces sizes 0..500 for each unseen `type_id`, looking for a
//! size where the *remainder* of the entry (other buffs + post-buff
//! fields) parses cleanly. The discovered size is cached and reused for
//! later occurrences.
//!
//! Tail sizes for the same `type_id` differ between game versions, so the
//! cache is built per parse, not frozen at compile time.

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
    /// 2.06+ — as `WithField58`, but `field_60` (a `u32 ConditionInfoKey`
    /// through 2.05.01) is an inline condition: `00` for none, or `01`
    /// followed by the condition's compiled bytecode. 2.06 removed the
    /// anonymous conditions from `conditioninfo` and inlined them where they
    /// were used ("Inlined conditions (2.06)" in `scripts/CLAUDE.md`).
    /// The bytecode's length depends on each function's argument layout,
    /// which is not decoded, so the parser probes for it.
    InlineCondition,
}

/// Longest inline condition (tag byte included) the probe tries. The longest
/// anonymous condition in the 2.05.01 `conditioninfo` is 597 bytes of
/// bytecode; this leaves room for growth.
pub const MAX_INLINE_CONDITION: usize = 1024;

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
    /// `ConditionInfoKey` through 2.05.01; 0 in the 2.06 format, where
    /// `field_60_condition` holds the field instead.
    pub field_60: u32,
    /// 2.06+ ([`SkillFormat::InlineCondition`]): the raw inline condition,
    /// tag byte included (`[0]` for none, `[1, bytecode…]`). `None` in older
    /// formats.
    pub field_60_condition: Option<Vec<u8>>,
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

/// Where `field_60` sits when it holds an inline condition with bytecode
/// (tag `01`), whose length the caller has to probe for. `None` when there is
/// nothing to probe: an older format, or the `00` (no condition) tag. `offset`
/// is just after the buff's `flag` byte.
pub fn inline_condition_at(data: &[u8], offset: usize, format: SkillFormat) -> io::Result<Option<usize>> {
    if format != SkillFormat::InlineCondition {
        return Ok(None);
    }
    // type_id u8, field_12/16 u32, field_20/21 u8, field_24/32/40 i64.
    let mut at = offset + 1 + 4 + 4 + 1 + 1 + 8 * 3;
    let len = u32::read_from(data, &mut at)? as usize;
    // field_48 bytes, field_56 u32, field_58 u8.
    let pos = at + len + 4 + 1;
    match data.get(pos) {
        Some(0) => Ok(None),
        Some(1) => Ok(Some(pos)),
        Some(t) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("BuffData.field_60: inline condition tag {t:#04x} at {pos}"),
        )),
        None => Err(io::Error::new(io::ErrorKind::UnexpectedEof, "BuffData.field_60: past the end")),
    }
}

/// Read the common base for a non-null `BuffData` (caller has already
/// consumed the `flag` byte and confirmed `flag == 0`). Does *not* read
/// the subclass tail — that requires probing. In the 2.06 format an inline
/// condition with bytecode needs its length: use [`read_common_base_with`].
pub fn read_common_base(
    data: &[u8],
    offset: &mut usize,
    format: SkillFormat,
) -> io::Result<BuffDataBody> {
    read_common_base_with(data, offset, format, None)
}

/// [`read_common_base`] with the length of a 2.06 inline condition (tag byte
/// included) when it has bytecode (tag `01`). `None` accepts only the `00`
/// tag; the length is ignored in the older formats.
pub fn read_common_base_with(
    data: &[u8],
    offset: &mut usize,
    format: SkillFormat,
    condition_len: Option<usize>,
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
        SkillFormat::WithField58 | SkillFormat::InlineCondition => Some(u8::read_from(data, offset)?),
        SkillFormat::NoField58 => None,
    };
    let (field_60, field_60_condition) = if format == SkillFormat::InlineCondition {
        let len = match (data.get(*offset), condition_len) {
            (Some(0), _) => 1,
            (Some(1), Some(n)) if n >= 2 => n,
            (Some(1), _) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("BuffData.field_60: inline condition at {} needs a probed length", *offset),
                ));
            }
            (Some(t), _) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("BuffData.field_60: inline condition tag {t:#04x} at {}", *offset),
                ));
            }
            (None, _) => {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "BuffData.field_60: past the end"));
            }
        };
        let cond = data.get(*offset..*offset + len).ok_or_else(|| {
            io::Error::new(io::ErrorKind::UnexpectedEof, "BuffData.field_60: condition runs past the end")
        })?;
        *offset += len;
        (0, Some(cond.to_vec()))
    } else {
        (u32::read_from(data, offset)?, None)
    };
    let field_62 = u32::read_from(data, offset)?;
    let field_64 = u32::read_from(data, offset)?;
    let field_66 = u32::read_from(data, offset)?;
    let field_68 = u8::read_from(data, offset)?;
    let field_69 = u8::read_from(data, offset)?;
    let field_88 = u32::read_from(data, offset)?;
    let field_90 = u32::read_from(data, offset)?;

    let cnt96 = super::post_buff::read_count(data, offset, 4)?;
    let mut field_96_list = Vec::with_capacity(cnt96);
    for _ in 0..cnt96 {
        field_96_list.push(u32::read_from(data, offset)?);
    }

    let field_128 = u32::read_from(data, offset)?;
    let field_72 = u32::read_from(data, offset)?;
    let field_76 = u32::read_from(data, offset)?;
    let field_80 = u32::read_from(data, offset)?;
    let field_84 = u32::read_from(data, offset)?;

    let cnt112 = super::post_buff::read_count(data, offset, 4)?;
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
        field_60_condition,
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
    match &body.field_60_condition {
        Some(cond) => w.write_all(cond)?,
        None => body.field_60.write_to(w)?,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A common base up to (not including) `cnt96`, field_48 empty.
    fn base_until_cnt96() -> Vec<u8> {
        let mut b = vec![0u8]; // type_id
        b.extend_from_slice(&[0; 4 + 4 + 1 + 1 + 8 + 8 + 8]);
        b.extend_from_slice(&0u32.to_le_bytes()); // field_48 len
        b.extend_from_slice(&[0; 4 + 1 + 4 * 4 + 1 + 1 + 4 + 4]);
        b
    }

    /// The probe reads at mostly-wrong offsets, so a count can be any u32.
    /// 0xEB000000 would ask for 15.7 GB.
    #[test]
    fn huge_list_count_is_an_error_not_an_allocation() {
        let mut b = base_until_cnt96();
        b.extend_from_slice(&0xEB00_0000u32.to_le_bytes());
        b.extend_from_slice(&[0; 64]);
        let err = read_common_base(&b, &mut 0, SkillFormat::WithField58).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData, "{err}");

        // Same for field_112_list, behind an empty field_96_list.
        let mut b = base_until_cnt96();
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&[0; 4 * 5]);
        b.extend_from_slice(&0xEB00_0000u32.to_le_bytes());
        let err = read_common_base(&b, &mut 0, SkillFormat::WithField58).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData, "{err}");
    }

    /// A 2.06 common base whose `field_60` is `cond`, with empty lists.
    fn base_206(cond: &[u8]) -> Vec<u8> {
        let mut b = vec![7u8]; // type_id
        b.extend_from_slice(&[0; 4 + 4 + 1 + 1 + 8 + 8 + 8]);
        b.extend_from_slice(&0u32.to_le_bytes()); // field_48 len
        b.extend_from_slice(&[0; 4 + 1]); // field_56, field_58
        b.extend_from_slice(cond);
        b.extend_from_slice(&[0; 4 * 3 + 1 + 1 + 4 + 4]); // field_62..field_90
        b.extend_from_slice(&0u32.to_le_bytes()); // field_96_list
        b.extend_from_slice(&[0; 4 * 5]); // field_128, 72, 76, 80, 84
        b.extend_from_slice(&0u32.to_le_bytes()); // field_112_list
        b.extend_from_slice(&[0; 1 + 4]); // field_132, field_136
        b
    }

    #[test]
    fn inline_condition_none_reads_and_writes_back() {
        let b = base_206(&[0]);
        assert_eq!(inline_condition_at(&b, 0, SkillFormat::InlineCondition).unwrap(), None);
        let mut at = 0;
        let body = read_common_base(&b, &mut at, SkillFormat::InlineCondition).unwrap();
        assert_eq!(at, b.len());
        assert_eq!(body.field_60_condition.as_deref(), Some(&[0u8][..]));
        let mut out = Vec::new();
        write_buff_data(&mut out, &BuffData { flag: 0, body: Some(body) }).unwrap();
        assert_eq!(out[1..], b[..]);
    }

    #[test]
    fn inline_condition_with_bytecode_needs_its_length() {
        // `IsCharacter()` as 2.06 inlines it in skill.
        let cond = [1, 1, 3, 0x73, 1, 0, 0, 0, 0, 1];
        let b = base_206(&cond);
        let pos = inline_condition_at(&b, 0, SkillFormat::InlineCondition).unwrap();
        assert_eq!(pos, Some(1 + 4 + 4 + 1 + 1 + 24 + 4 + 4 + 1));
        assert!(read_common_base(&b, &mut 0, SkillFormat::InlineCondition).is_err());
        let mut at = 0;
        let body = read_common_base_with(&b, &mut at, SkillFormat::InlineCondition, Some(cond.len())).unwrap();
        assert_eq!(at, b.len());
        assert_eq!(body.field_60_condition.as_deref(), Some(&cond[..]));
        let mut out = Vec::new();
        write_buff_data(&mut out, &BuffData { flag: 0, body: Some(body) }).unwrap();
        assert_eq!(out[1..], b[..]);
    }

    #[test]
    fn unknown_inline_condition_tag_is_an_error() {
        let b = base_206(&[2]);
        assert!(inline_condition_at(&b, 0, SkillFormat::InlineCondition).is_err());
        assert!(read_common_base_with(&b, &mut 0, SkillFormat::InlineCondition, Some(4)).is_err());
    }

    #[test]
    fn list_count_that_fits_exactly_still_reads() {
        let mut b = base_until_cnt96();
        b.extend_from_slice(&2u32.to_le_bytes());
        b.extend_from_slice(&[7, 0, 0, 0, 9, 0, 0, 0]);
        b.extend_from_slice(&[0; 4 * 5]); // field_128, 72, 76, 80, 84
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&5u32.to_le_bytes());
        b.extend_from_slice(&[0; 1 + 4]); // field_132, field_136
        let body = read_common_base(&b, &mut 0, SkillFormat::WithField58).unwrap();
        assert_eq!(body.field_96_list, [7, 9]);
        assert_eq!(body.field_112_list, [5]);
    }
}
