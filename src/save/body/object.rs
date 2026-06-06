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
    /// Boolean, stored as its **raw byte**. The game writes `true` as
    /// either `0x01` or `0xff` (both occur in the same save — e.g.
    /// `_isNewMark` is `0xff` while most flags are `0x01`), so collapsing
    /// to a Rust `bool` and re-emitting a canonical `0x01` would corrupt
    /// the `0xff` ones. Preserving the exact byte is required for
    /// byte-perfect round-trip. Truthiness is `!= 0`.
    Bool(u8),
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
    /// `float3` (12 bytes, 3 × f32). Used pervasively for positions
    /// (`_spawnPosition`, `_deadPosition`, …). ~58k occurrences in the
    /// live 1.07 sample save.
    F32x3([f32; 3]),
    /// `float4` / `quaternion` (16 bytes, 4 × f32). Used for rotations
    /// and 4-vec data.
    F32x4([f32; 4]),
    /// `uint4` (16 bytes, 4 × u32). Used as a 128-bit UUID
    /// (`SceneObjectUuid` etc.). ~11k occurrences in the live sample.
    U32x4([u32; 4]),
    /// Anything we couldn't bucket into a primitive or known composite
    /// (non-power-of-2 size, or unrecognized type name with a weird
    /// size). `Transform` (40 B) currently lands here — schema is
    /// known (position 3×f32 + rotation 4×f32 + scale 3×f32) but the
    /// byte ordering hasn't been verified against a written-then-read
    /// round-trip yet, so the conservative `Bytes` fallback preserves
    /// the raw payload.
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
//
// Some preservation fields below (header_bytes / trailer_bytes / wrapper_prefix)
// are only consumed by the encoder, which itself is dead in non-test builds
// until the Phase B.2 C ABI wrapper lands. `allow(dead_code)` keeps clippy
// quiet for the field-read warning in the meantime — the bytes ARE captured
// at decode time so a future round-trip writer sees them.
#[allow(dead_code)]
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
    ///
    /// `header_bytes` captures the raw bytes preceding `bytes` (the
    /// variant's prefix + count field). `trailer_bytes` captures any
    /// bytes following `bytes` (e.g. the `01 01 01 01 01` sentinel for
    /// `prefix_00xx0100`, or the optional trailing `01` for
    /// `marker_prefix`). Together with `bytes` they reconstruct the
    /// on-disk field bytes exactly; the encoder (see [`super::encoder`])
    /// just splats `header_bytes ++ bytes ++ trailer_bytes`.
    DynamicArray {
        count: u32,
        bytes: Vec<u8>,
        /// Which of the 4 known header layouts this matched; useful for
        /// regression analysis when a new game version drifts the format.
        header_variant: &'static str,
        header_bytes: Vec<u8>,
        trailer_bytes: Vec<u8>,
    },
    /// Inline-object locator (meta_kind 4 / 5). If the locator's child
    /// payload sits immediately after the wrapper and the child type was
    /// resolvable, we recurse and produce the nested block here.
    ///
    /// `child_reserved`, `child_sentinel1`, `child_sentinel2` capture the
    /// wrapper bytes that sit between the child mask and the
    /// `child_payload_offset` field. The decoder discards them
    /// semantically but the encoder needs them for round-trip identity.
    ///
    /// `wrapper_prefix` captures 0..=8 padding bytes between
    /// `field.start` and the start of the wrapper proper. Always empty
    /// for `meta_kind == 4`; non-empty when `meta_kind == 5` and the
    /// engine wrote leading pad bytes before the wrapper (the decoder
    /// scans for the first plausible `mask_byte_count` at offsets 0..=8).
    Locator {
        child_type_index: u16,
        child_type_name: String,
        child_payload_offset: u32,
        child_reserved: u8,
        child_sentinel1: u32,
        child_sentinel2: u32,
        wrapper_prefix: Vec<u8>,
        child: Option<Box<ObjectBlock>>,
    },
    /// Object list (meta_kind 6 / 7). `header_variant` is the
    /// disambiguator for the multiple known header shapes.
    ///
    /// `header_bytes` captures the raw bytes of the list's variant-
    /// specific header (the slice the decoder consumed before walking
    /// elements). Holding it verbatim makes round-trip trivial and lets
    /// the encoder patch only the count bytes when an element is
    /// inserted / removed (Phase B.2).
    ObjectList {
        count: u32,
        header_variant: &'static str,
        header_bytes: Vec<u8>,
        elements: Vec<ObjectBlock>,
    },
}

/// Locator wrapper bytes attached to an [`ObjectBlock`] when it was decoded
/// as an `object_list` element or as an inline locator child.
///
/// Top-level TOC blocks set this to `None`. For list elements / inline
/// children the wrapper sits immediately before the block's payload bytes
/// and the encoder reconstructs it from this struct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectLocatorWrapper {
    pub type_index: u16,
    pub child_reserved: u8,
    pub sentinel1: u32,
    pub sentinel2: u32,
    pub payload_offset: u32,
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
    /// For top-level blocks: the `u32` immediately after the mask. For
    /// inline locator children / object_list elements: the `u32` at the
    /// start of the inline payload (which the decoder used to call
    /// "reserved_u32"). Either way the encoder writes it back at the
    /// same offset.
    pub reserved_u32: u32,
    pub fields: Vec<DecodedField>,
    /// Trailer bytes between the end of the forward walk and the start
    /// of the reverse-peeled tail (or the end of the block, when no
    /// reverse pass ran). The schema doesn't describe this region; the
    /// engine writes 1..N bytes there in some blocks and the original
    /// Python parser leaves them as undecoded too. We capture the bytes
    /// here so a future writer can round-trip them.
    ///
    /// Observed shapes:
    ///   - 232 blocks have exactly 1 byte (FieldNPCSaveData + 6 other classes).
    ///   - 1 block (MercenaryClanSaveData) has 3 bytes of `01 01 01`.
    ///
    /// Empty `Vec` when no such trailer exists for this block. See
    /// `docs/save-body-format.md` for the empirical analysis.
    pub trailing_pad: Vec<u8>,
    /// Byte ranges inside the block that no decoder placed (start, end).
    pub undecoded_ranges: Vec<(usize, usize)>,
    /// Locator wrapper bytes that precede this block on disk, when it was
    /// decoded as a list element or an inline locator child. `None` for
    /// top-level TOC blocks (which carry their own `mask_byte_count` +
    /// `mask_bytes` + `reserved_u32` header instead).
    pub locator_wrapper: Option<ObjectLocatorWrapper>,
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
    scalar_from_bytes(data, &field.type_name, field.meta_size as usize)
}

/// Decode a scalar from raw LE bytes given just its declared type name +
/// byte size. Same heuristic as [`decode_scalar`] but callable without a
/// full [`FieldDef`] — used by the c_abi mutation surface (Phase B.2)
/// when synthesizing a `ScalarValue` for a newly-present field.
pub fn scalar_from_bytes(data: &[u8], type_name: &str, size: usize) -> ScalarValue {
    let lower = type_name.to_ascii_lowercase();
    if lower == "bool" && size == 1 {
        // Preserve the exact byte (0x00 / 0x01 / 0xff / …) for round-trip;
        // truthiness is `!= 0`. See the `Bool` variant doc.
        return ScalarValue::Bool(data[0]);
    }
    // ── Composite vector / quaternion scalars (12 / 16 bytes) ──
    // These cover ~70k field occurrences in a live 1.07 save where the
    // previous fallback path emitted opaque `Bytes`. See
    // `_probe_save_composite_types` in `src/c_abi/character_info.rs`
    // for the survey that drove this list.
    if lower == "float3" && size == 12 && data.len() == 12 {
        return ScalarValue::F32x3([
            f32::from_le_bytes([data[0], data[1], data[2], data[3]]),
            f32::from_le_bytes([data[4], data[5], data[6], data[7]]),
            f32::from_le_bytes([data[8], data[9], data[10], data[11]]),
        ]);
    }
    if (lower == "float4" || lower == "quaternion") && size == 16 && data.len() == 16 {
        return ScalarValue::F32x4([
            f32::from_le_bytes([data[0], data[1], data[2], data[3]]),
            f32::from_le_bytes([data[4], data[5], data[6], data[7]]),
            f32::from_le_bytes([data[8], data[9], data[10], data[11]]),
            f32::from_le_bytes([data[12], data[13], data[14], data[15]]),
        ]);
    }
    if lower == "uint4" && size == 16 && data.len() == 16 {
        return ScalarValue::U32x4([
            u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
            u32::from_le_bytes([data[4], data[5], data[6], data[7]]),
            u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
            u32::from_le_bytes([data[12], data[13], data[14], data[15]]),
        ]);
    }
    // ── Scalar floats ──
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

#[cfg(test)]
mod scalar_from_bytes_tests {
    use super::*;

    #[test]
    fn float3_decodes_three_f32() {
        // [1.5, -2.25, 0.125] LE
        let bytes = [
            0x00, 0x00, 0xc0, 0x3f, // 1.5
            0x00, 0x00, 0x10, 0xc0, // -2.25
            0x00, 0x00, 0x00, 0x3e, // 0.125
        ];
        match scalar_from_bytes(&bytes, "float3", 12) {
            ScalarValue::F32x3([a, b, c]) => {
                assert_eq!(a, 1.5);
                assert_eq!(b, -2.25);
                assert_eq!(c, 0.125);
            }
            other => panic!("expected F32x3, got {other:?}"),
        }
    }

    #[test]
    fn quaternion_decodes_four_f32() {
        let mut bytes = [0u8; 16];
        bytes[..4].copy_from_slice(&1.0f32.to_le_bytes());
        bytes[4..8].copy_from_slice(&0.5f32.to_le_bytes());
        bytes[8..12].copy_from_slice(&(-0.25f32).to_le_bytes());
        bytes[12..16].copy_from_slice(&2.0f32.to_le_bytes());
        match scalar_from_bytes(&bytes, "quaternion", 16) {
            ScalarValue::F32x4([a, b, c, d]) => {
                assert_eq!([a, b, c, d], [1.0, 0.5, -0.25, 2.0]);
            }
            other => panic!("expected F32x4, got {other:?}"),
        }
    }

    #[test]
    fn float4_decodes_four_f32() {
        let mut bytes = [0u8; 16];
        bytes[..4].copy_from_slice(&3.0f32.to_le_bytes());
        bytes[4..8].copy_from_slice(&4.0f32.to_le_bytes());
        bytes[8..12].copy_from_slice(&5.0f32.to_le_bytes());
        bytes[12..16].copy_from_slice(&6.0f32.to_le_bytes());
        assert!(matches!(
            scalar_from_bytes(&bytes, "float4", 16),
            ScalarValue::F32x4([a, b, c, d]) if a == 3.0 && b == 4.0 && c == 5.0 && d == 6.0
        ));
    }

    #[test]
    fn uint4_decodes_four_u32() {
        let bytes = [
            0x78, 0x56, 0x34, 0x12, // 0x12345678
            0xef, 0xcd, 0xab, 0x89, // 0x89abcdef
            0x11, 0x22, 0x33, 0x44, // 0x44332211
            0x55, 0x66, 0x77, 0x88, // 0x88776655
        ];
        match scalar_from_bytes(&bytes, "uint4", 16) {
            ScalarValue::U32x4([a, b, c, d]) => {
                assert_eq!([a, b, c, d], [0x12345678, 0x89abcdef, 0x44332211, 0x88776655]);
            }
            other => panic!("expected U32x4, got {other:?}"),
        }
    }

    #[test]
    fn transform_still_falls_through_to_bytes() {
        // 40-byte Transform — schema known (3+4+3 f32) but layout
        // verification deferred; current behaviour preserves bytes.
        let bytes = vec![0u8; 40];
        match scalar_from_bytes(&bytes, "Transform", 40) {
            ScalarValue::Bytes(b) => assert_eq!(b.len(), 40),
            other => panic!("expected Bytes(40), got {other:?}"),
        }
    }

    #[test]
    fn primitive_scalars_unchanged() {
        // Regression guard — the new composite branches must NOT shadow
        // the primitive `float`/`int*`/`uint*` paths.
        assert!(matches!(
            scalar_from_bytes(&[0x00, 0x00, 0x80, 0x3f], "float", 4),
            ScalarValue::F32(x) if x == 1.0
        ));
        assert!(matches!(
            scalar_from_bytes(&[0xff, 0xff, 0xff, 0xff], "int32", 4),
            ScalarValue::I32(-1)
        ));
        assert!(matches!(
            scalar_from_bytes(&[0xff, 0xff, 0xff, 0xff], "uint32", 4),
            ScalarValue::U32(0xFFFFFFFF)
        ));
    }

    #[test]
    fn bool_preserves_raw_byte_round_trip() {
        // Regression: the game stores `true` as BOTH 0x01 and 0xFF in the
        // same save (e.g. `_isNewMark` is 0xFF, most other flags are 0x01).
        // Collapsing to a Rust `bool` and re-emitting a canonical 0x01
        // corrupted the 0xFF ones — a round-trip violation that produces a
        // save the game's loader rejects after any length-changing edit.
        // The exact byte must survive decode -> encode unchanged.
        for raw in [0x00u8, 0x01, 0xff] {
            let v = scalar_from_bytes(&[raw], "bool", 1);
            assert!(
                matches!(v, ScalarValue::Bool(b) if b == raw),
                "bool 0x{raw:02x} should decode to Bool({raw})"
            );
            let mut out = Vec::new();
            crate::save::body::encoder::encode_scalar(&mut out, &v, 1);
            assert_eq!(out, vec![raw], "bool 0x{raw:02x} must round-trip");
        }
    }

    #[test]
    fn float3_round_trips_through_encoder() {
        // Decode → encode → decode produces the same f32 triple.
        let original = [1.5f32, -2.25, 0.125];
        let mut buf = Vec::with_capacity(12);
        for x in &original {
            buf.extend_from_slice(&x.to_le_bytes());
        }
        let decoded = scalar_from_bytes(&buf, "float3", 12);
        // Re-encode via the encoder path.
        let mut reencoded = Vec::with_capacity(12);
        crate::save::body::encoder::encode_scalar(&mut reencoded, &decoded, 12);
        assert_eq!(reencoded, buf, "float3 round-trip differs");
    }
}

