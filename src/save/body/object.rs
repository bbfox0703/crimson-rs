//! Types produced by the save-body field decoder.
//!
//! Each TOC entry's data slice decodes into an [`ObjectBlock`] holding the
//! header (mask + reserved) plus a list of [`DecodedField`]s, one per
//! schema field. A field's `value` is a typed [`FieldValue`] — scalars are
//! decoded into the natural Rust numeric type rather than reps strings, so
//! downstream consumers don't have to re-parse them.

use std::collections::HashMap;

use super::schema::{FieldDef, TypeDef};

/// Scalar value decoded from a fixed-size field. The variant is chosen
/// from `(type_name, size)` via the same heuristic the Python source used
/// (see `_type_to_edit_format`): `bool` → `Bool`, anything containing
/// "float" maps to `F32`/`F64` by size, anything starting with "int" maps
/// to `I8`/`I16`/`I32`/`I64` by size, everything else maps to `U*` by
/// size, and odd sizes fall through to `Bytes`.
#[derive(Debug, Clone)]
pub enum ScalarValue {
    Bool(bool),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    /// Anything we couldn't bucket into a primitive (non-power-of-2 size,
    /// or unrecognized type name with a weird size).
    Bytes(Vec<u8>),
}

/// What kind of decode produced this field's `value`.
///
/// The names line up with the Python parser's `decode_kind` strings so
/// cross-referencing remains easy. `Unknown` means the decoder couldn't
/// place this field — it sits in the source data but we don't know its
/// layout from the header schema alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Absent,
    FixedPrefix,
    FixedSuffix,
    InlineBytes,
    DynamicArray,
    ObjectLocator,
    ObjectList,
    Unknown,
}

impl FieldKind {
    pub fn as_str(self) -> &'static str {
        match self {
            FieldKind::Absent => "absent",
            FieldKind::FixedPrefix => "fixed_prefix",
            FieldKind::FixedSuffix => "fixed_suffix",
            FieldKind::InlineBytes => "inline_bytes",
            FieldKind::DynamicArray => "dynamic_array",
            FieldKind::ObjectLocator => "object_locator",
            FieldKind::ObjectList => "object_list",
            FieldKind::Unknown => "unknown",
        }
    }
}

/// Typed payload attached to a [`DecodedField`].
#[derive(Debug, Clone)]
pub enum FieldValue {
    /// Field was absent in the presence mask, or the decoder gave up here.
    None,
    /// Fixed-size scalar (meta_kind 0 / 2).
    Scalar(ScalarValue),
    /// Inline byte array (meta_kind 1). `count` is the element count from
    /// the 4-byte header; `bytes` is the raw payload after the header
    /// (length = count * meta_size).
    InlineBytes { count: u32, bytes: Vec<u8> },
    /// Dynamic primitive array (meta_kind 3). Same shape as
    /// [`FieldValue::InlineBytes`] but tagged separately so callers can
    /// distinguish the two header layouts.
    DynamicArray {
        count: u32,
        bytes: Vec<u8>,
        /// Which of the 4 known header layouts this matched; useful for
        /// regression analysis when a new game version drifts the format.
        header_variant: &'static str,
    },
    /// Inline-object locator (meta_kind 4 / 5). If the locator's child
    /// payload sits immediately after the wrapper and the child type was
    /// resolvable, we recurse and produce the nested block here.
    Locator {
        child_type_index: u16,
        child_type_name: String,
        child_payload_offset: u32,
        child: Option<Box<ObjectBlock>>,
    },
    /// Object list (meta_kind 6 / 7). `header_variant` is the
    /// disambiguator for the multiple known header shapes.
    ObjectList {
        count: u32,
        header_variant: &'static str,
        elements: Vec<ObjectBlock>,
    },
}

#[derive(Debug, Clone)]
pub struct DecodedField {
    pub field_index: u32,
    pub name: String,
    pub type_name: String,
    pub meta_kind: u16,
    pub meta_size: u16,
    pub meta_aux: u32,
    pub present: bool,
    pub kind: FieldKind,
    pub value: FieldValue,
    pub start: usize,
    pub end: usize,
    /// Free-form note from the decoder (matches Python's `note` field).
    /// Empty when not relevant.
    pub note: String,
}

#[derive(Debug, Clone)]
pub struct ObjectBlock {
    pub class_index: u32,
    pub class_name: String,
    /// Body offset where this block starts.
    pub data_offset: u32,
    /// Block size from the TOC entry (may differ from end - start when a
    /// list element shares the same block).
    pub data_size: u32,
    pub mask_byte_count: u16,
    pub mask_bytes: Vec<u8>,
    pub reserved_u32: u32,
    pub fields: Vec<DecodedField>,
    /// Byte ranges inside the block that no decoder placed (start, end).
    pub undecoded_ranges: Vec<(usize, usize)>,
}

/// Build a quick lookup `class_index -> &TypeDef` reusable across calls.
pub(crate) fn type_index_map(types: &[TypeDef]) -> HashMap<u32, &TypeDef> {
    types.iter().map(|t| (t.index, t)).collect()
}

/// Test bit `field_index` in `mask_bytes`. Out-of-range bits read as 0.
pub(crate) fn field_present(mask_bytes: &[u8], field_index: usize) -> bool {
    let byte_index = field_index / 8;
    let bit_index = field_index % 8;
    mask_bytes
        .get(byte_index)
        .is_some_and(|b| (b & (1 << bit_index)) != 0)
}

/// Decode the leading scalar of a field given its declared type name.
///
/// Matches Python `_type_to_edit_format` semantics: type-name + size pick
/// a primitive; anything that doesn't match falls back to raw bytes.
pub(crate) fn decode_scalar(data: &[u8], field: &FieldDef) -> ScalarValue {
    let size = field.meta_size as usize;
    let lower = field.type_name.to_ascii_lowercase();
    if lower == "bool" && size == 1 {
        return ScalarValue::Bool(data[0] != 0);
    }
    if lower.contains("float") {
        if size == 4 {
            return ScalarValue::F32(f32::from_le_bytes(data.try_into().unwrap_or([0; 4])));
        }
        if size == 8 {
            return ScalarValue::F64(f64::from_le_bytes(data.try_into().unwrap_or([0; 8])));
        }
    }
    if lower.starts_with("int") {
        match size {
            1 => return ScalarValue::I8(data[0] as i8),
            2 => return ScalarValue::I16(i16::from_le_bytes(data.try_into().unwrap_or([0; 2]))),
            4 => return ScalarValue::I32(i32::from_le_bytes(data.try_into().unwrap_or([0; 4]))),
            8 => return ScalarValue::I64(i64::from_le_bytes(data.try_into().unwrap_or([0; 8]))),
            _ => {}
        }
    }
    match size {
        1 => ScalarValue::U8(data[0]),
        2 => ScalarValue::U16(u16::from_le_bytes(data.try_into().unwrap_or([0; 2]))),
        4 => ScalarValue::U32(u32::from_le_bytes(data.try_into().unwrap_or([0; 4]))),
        8 => ScalarValue::U64(u64::from_le_bytes(data.try_into().unwrap_or([0; 8]))),
        _ => ScalarValue::Bytes(data.to_vec()),
    }
}
