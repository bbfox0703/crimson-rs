//! Name-keyed element templates: carry an `object_list` element across
//! schema versions.
//!
//! Every save embeds its own schema, and a class's field list moves between
//! patches — `MercenarySaveData` lost `_occupationState` somewhere between
//! 1.12 and 2.00 and gained `_shipStationSaveList` in 2.01. Raw element
//! bytes are valid only under the schema that wrote them: the class-name
//! remap in [`super::crimson_save_transplant_list_element`] fixes type
//! indices, not field order, so an element captured on one patch cannot be
//! inserted into a save another patch wrote.
//!
//! A template records the element by NAME instead: its class name, its
//! wrapper bytes, and for every present field the field name plus the
//! payload — scalar bytes, inline-bytes / dynamic-array contents, a
//! locator's child object, a list's header and elements — recursively.
//! [`crimson_save_export_element_template`] writes one from a decoded
//! element; [`crimson_save_list_insert_element_template`] rebuilds it under
//! the TARGET save's schema and inserts it:
//!
//! - a field the target class no longer has is dropped (and counted);
//! - a field the target class gained stays absent — with its `0x01` absence
//!   marker when it is a dynamic array or object list — except an inline
//!   object locator (`meta_kind` 4): the game never writes one absent (64k
//!   present, none absent across saves from 1.10 to 2.03), so it is created
//!   with an empty child of the class the target save itself uses for that
//!   field;
//! - a field whose kind or size changed is `TEMPLATE_MISMATCH`, never a
//!   silent reinterpretation.
//!
//! ## Wire format (v1, little-endian)
//!
//! ```text
//! "CRET" u8 version=1  object
//! object  = str class_name, u32 reserved_u32, u8 wrapper_reserved,
//!           u32 sentinel1, u32 sentinel2, u16 field_count, field*
//! field   = str name, u8 kind, payload
//!   0 scalar   u16 size, bytes
//!   1 inline   u16 elem_size, u32 count, bytes[count * elem_size]
//!   2 array    u16 elem_size, u32 count, bytes[count * elem_size]
//!   3 locator  u8 meta_kind, u8 prefix_len, prefix, object (the child)
//!   4 list     u8 meta_kind, u8 header_len, header, u32 count, object*
//! str     = u16 len, UTF-8
//! ```
//!
//! Only present fields are recorded, in schema order.

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::{
    CrimsonPathStep, CrimsonSaveHandle, apply_length_changing_mutation, error,
    navigate_mut_to_field, navigate_to_parent_ref, save_ffi_lock, slice_from_raw_or_empty,
    update_object_list_count_in_header,
};
use crate::save::{
    DecodedField, FieldDef, FieldKind, FieldValue, ObjectBlock, ObjectLocatorWrapper, TypeDef,
    absent_kind_has_marker, encode_scalar, scalar_from_bytes,
};

const MAGIC: &[u8; 4] = b"CRET";
const VERSION: u8 = 1;

/// Wrapper sentinels the game writes on every inline locator child (and on
/// almost every list element) — used for the children the builder creates.
const GAME_SENTINEL: u32 = 0xFFFF_FFFF;

/// Deepest nesting the builder follows. Real saves nest a handful of
/// levels; this only stops a malformed template, or a cycle among the
/// learned locator classes, from recursing without bound.
const MAX_DEPTH: usize = 32;

const KIND_SCALAR: u8 = 0;
const KIND_INLINE: u8 = 1;
const KIND_ARRAY: u8 = 2;
const KIND_LOCATOR: u8 = 3;
const KIND_LIST: u8 = 4;

#[derive(Debug, Clone, PartialEq)]
struct TObject {
    class_name: String,
    reserved_u32: u32,
    wrapper_reserved: u8,
    sentinel1: u32,
    sentinel2: u32,
    fields: Vec<TField>,
}

#[derive(Debug, Clone, PartialEq)]
struct TField {
    name: String,
    value: TValue,
}

#[derive(Debug, Clone, PartialEq)]
enum TValue {
    Scalar(Vec<u8>),
    Inline { elem_size: u16, count: u32, bytes: Vec<u8> },
    Array { elem_size: u16, count: u32, bytes: Vec<u8> },
    Locator { meta_kind: u8, prefix: Vec<u8>, child: TObject },
    List { meta_kind: u8, header: Vec<u8>, elements: Vec<TObject> },
}

// ── Export: decoded element -> template ────────────────────────────────────

/// Record `block` (a list element or locator child) by name. Refuses an
/// object the absence-marker walk did not fully explain — leftover
/// `trailing_pad` or undecoded bytes, a present field it could not place,
/// or an absent array / list without its marker — because its field values
/// cannot be trusted.
fn template_from_block(block: &ObjectBlock, depth: usize) -> Result<TObject, i32> {
    if depth > MAX_DEPTH || !block.trailing_pad.is_empty() || !block.undecoded_ranges.is_empty() {
        return Err(error::BODY_PARSE);
    }
    let wrapper = block.locator_wrapper.as_ref().ok_or(error::BODY_PARSE)?;
    let mut fields = Vec::new();
    for f in &block.fields {
        if !f.present {
            if absent_kind_has_marker(f.meta_kind) && !f.absent_marker {
                return Err(error::BODY_PARSE);
            }
            continue;
        }
        let value = match (&f.kind, &f.value) {
            (FieldKind::FixedPrefix | FieldKind::FixedSuffix, FieldValue::Scalar(v)) => {
                let mut bytes = Vec::with_capacity(f.meta_size as usize);
                encode_scalar(&mut bytes, v, f.meta_size as usize);
                TValue::Scalar(bytes)
            }
            (FieldKind::InlineBytes, FieldValue::InlineBytes { count, bytes }) => TValue::Inline {
                elem_size: f.meta_size,
                count: *count,
                bytes: bytes.clone(),
            },
            (FieldKind::DynamicArray, FieldValue::DynamicArray { count, bytes, .. }) => {
                TValue::Array { elem_size: f.meta_size, count: *count, bytes: bytes.clone() }
            }
            (
                FieldKind::ObjectLocator,
                FieldValue::Locator { wrapper_prefix, child: Some(child), inline_child: true, .. },
            ) => TValue::Locator {
                meta_kind: f.meta_kind as u8,
                prefix: wrapper_prefix.clone(),
                child: template_from_block(child, depth + 1)?,
            },
            (FieldKind::ObjectList, FieldValue::ObjectList { header_bytes, elements, .. })
                if header_bytes.len() == 18 && header_bytes[0] == 0 =>
            {
                TValue::List {
                    meta_kind: f.meta_kind as u8,
                    header: header_bytes.clone(),
                    elements: elements
                        .iter()
                        .map(|e| template_from_block(e, depth + 1))
                        .collect::<Result<_, _>>()?,
                }
            }
            _ => return Err(error::BODY_PARSE),
        };
        fields.push(TField { name: f.name.clone(), value });
    }
    Ok(TObject {
        class_name: block.class_name.clone(),
        reserved_u32: block.reserved_u32,
        wrapper_reserved: wrapper.child_reserved,
        sentinel1: wrapper.sentinel1,
        sentinel2: wrapper.sentinel2,
        fields,
    })
}

fn write_str(out: &mut Vec<u8>, s: &str) -> Result<(), i32> {
    let len = u16::try_from(s.len()).map_err(|_| error::OUT_OF_RANGE)?;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(s.as_bytes());
    Ok(())
}

fn write_object(out: &mut Vec<u8>, o: &TObject) -> Result<(), i32> {
    write_str(out, &o.class_name)?;
    out.extend_from_slice(&o.reserved_u32.to_le_bytes());
    out.push(o.wrapper_reserved);
    out.extend_from_slice(&o.sentinel1.to_le_bytes());
    out.extend_from_slice(&o.sentinel2.to_le_bytes());
    let count = u16::try_from(o.fields.len()).map_err(|_| error::OUT_OF_RANGE)?;
    out.extend_from_slice(&count.to_le_bytes());
    for f in &o.fields {
        write_str(out, &f.name)?;
        match &f.value {
            TValue::Scalar(bytes) => {
                out.push(KIND_SCALAR);
                let len = u16::try_from(bytes.len()).map_err(|_| error::OUT_OF_RANGE)?;
                out.extend_from_slice(&len.to_le_bytes());
                out.extend_from_slice(bytes);
            }
            TValue::Inline { elem_size, count, bytes } => {
                out.push(KIND_INLINE);
                write_sized(out, *elem_size, *count, bytes);
            }
            TValue::Array { elem_size, count, bytes } => {
                out.push(KIND_ARRAY);
                write_sized(out, *elem_size, *count, bytes);
            }
            TValue::Locator { meta_kind, prefix, child } => {
                out.push(KIND_LOCATOR);
                out.push(*meta_kind);
                out.push(u8::try_from(prefix.len()).map_err(|_| error::OUT_OF_RANGE)?);
                out.extend_from_slice(prefix);
                write_object(out, child)?;
            }
            TValue::List { meta_kind, header, elements } => {
                out.push(KIND_LIST);
                out.push(*meta_kind);
                out.push(u8::try_from(header.len()).map_err(|_| error::OUT_OF_RANGE)?);
                out.extend_from_slice(header);
                let n = u32::try_from(elements.len()).map_err(|_| error::OUT_OF_RANGE)?;
                out.extend_from_slice(&n.to_le_bytes());
                for e in elements {
                    write_object(out, e)?;
                }
            }
        }
    }
    Ok(())
}

fn write_sized(out: &mut Vec<u8>, elem_size: u16, count: u32, bytes: &[u8]) {
    out.extend_from_slice(&elem_size.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(bytes);
}

fn encode_template(o: &TObject) -> Result<Vec<u8>, i32> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    write_object(&mut out, o)?;
    Ok(out)
}

// ── Parse: template bytes -> template ──────────────────────────────────────

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], i32> {
        let end = self.at.checked_add(n).ok_or(error::BODY_PARSE)?;
        let s = self.bytes.get(self.at..end).ok_or(error::BODY_PARSE)?;
        self.at = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, i32> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, i32> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Result<u32, i32> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn str(&mut self) -> Result<String, i32> {
        let n = self.u16()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| error::BODY_PARSE)
    }
    fn sized(&mut self, elem_size: u16, count: u32) -> Result<Vec<u8>, i32> {
        let n = (elem_size as usize).checked_mul(count as usize).ok_or(error::BODY_PARSE)?;
        Ok(self.take(n)?.to_vec())
    }
}

fn read_object(r: &mut Reader, depth: usize) -> Result<TObject, i32> {
    if depth > MAX_DEPTH {
        return Err(error::BODY_PARSE);
    }
    let class_name = r.str()?;
    let reserved_u32 = r.u32()?;
    let wrapper_reserved = r.u8()?;
    let sentinel1 = r.u32()?;
    let sentinel2 = r.u32()?;
    let count = r.u16()?;
    let mut fields = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let name = r.str()?;
        let value = match r.u8()? {
            KIND_SCALAR => {
                let n = r.u16()? as usize;
                TValue::Scalar(r.take(n)?.to_vec())
            }
            KIND_INLINE => {
                let (elem_size, count) = (r.u16()?, r.u32()?);
                TValue::Inline { elem_size, count, bytes: r.sized(elem_size, count)? }
            }
            KIND_ARRAY => {
                let (elem_size, count) = (r.u16()?, r.u32()?);
                TValue::Array { elem_size, count, bytes: r.sized(elem_size, count)? }
            }
            KIND_LOCATOR => {
                let meta_kind = r.u8()?;
                let n = r.u8()? as usize;
                let prefix = r.take(n)?.to_vec();
                let child = read_object(r, depth + 1)?;
                TValue::Locator { meta_kind, prefix, child }
            }
            KIND_LIST => {
                let meta_kind = r.u8()?;
                let n = r.u8()? as usize;
                let header = r.take(n)?.to_vec();
                let len = r.u32()?;
                let mut elements = Vec::new();
                for _ in 0..len {
                    elements.push(read_object(r, depth + 1)?);
                }
                TValue::List { meta_kind, header, elements }
            }
            _ => return Err(error::BODY_PARSE),
        };
        fields.push(TField { name, value });
    }
    Ok(TObject { class_name, reserved_u32, wrapper_reserved, sentinel1, sentinel2, fields })
}

fn decode_template(bytes: &[u8]) -> Result<TObject, i32> {
    let mut r = Reader { bytes, at: 0 };
    if r.take(4)? != MAGIC || r.u8()? != VERSION {
        return Err(error::BODY_PARSE);
    }
    let o = read_object(&mut r, 0)?;
    if r.at != bytes.len() {
        return Err(error::BODY_PARSE);
    }
    Ok(o)
}

// ── Build: template -> element under the target schema ─────────────────────

struct Builder<'a> {
    by_name: HashMap<&'a str, &'a TypeDef>,
    /// `(parent class, field) -> child class` for every inline object
    /// locator present in the target save: how a locator the template lacks
    /// gets the child class the game uses for it.
    locator_classes: HashMap<(String, String), String>,
    /// Template fields the target classes no longer have.
    dropped: u32,
}

impl<'a> Builder<'a> {
    fn new(types: &'a [TypeDef], blocks: &[ObjectBlock]) -> Self {
        fn learn(block: &ObjectBlock, into: &mut HashMap<(String, String), String>) {
            for f in &block.fields {
                match &f.value {
                    FieldValue::Locator { child: Some(c), .. } => {
                        if f.meta_kind == 4 {
                            into.entry((block.class_name.clone(), f.name.clone()))
                                .or_insert_with(|| c.class_name.clone());
                        }
                        learn(c, into);
                    }
                    FieldValue::ObjectList { elements, .. } => {
                        for e in elements {
                            learn(e, into);
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut locator_classes = HashMap::new();
        for b in blocks {
            learn(b, &mut locator_classes);
        }
        Builder {
            by_name: types.iter().map(|t| (t.name.as_str(), t)).collect(),
            locator_classes,
            dropped: 0,
        }
    }

    fn class(&self, name: &str) -> Result<&'a TypeDef, i32> {
        self.by_name.get(name).copied().ok_or(error::TRANSPLANT_TYPE_MISSING)
    }

    /// Build `t` as an object of the same-named class in the target schema.
    fn build(&mut self, t: &TObject, depth: usize) -> Result<ObjectBlock, i32> {
        if depth > MAX_DEPTH {
            return Err(error::BODY_PARSE);
        }
        let class = self.class(&t.class_name)?;
        let wanted: HashMap<&str, &TValue> =
            t.fields.iter().map(|f| (f.name.as_str(), &f.value)).collect();
        self.dropped += t
            .fields
            .iter()
            .filter(|f| !class.fields.iter().any(|d| d.name == f.name))
            .count() as u32;
        let mut fields = Vec::with_capacity(class.fields.len());
        for (i, def) in class.fields.iter().enumerate() {
            let value = match wanted.get(def.name.as_str()) {
                Some(v) => Some(self.build_value(def, v, depth)?),
                None if def.meta_kind == 4 => {
                    Some(self.default_locator(&class.name, &def.name, depth)?)
                }
                None => None,
            };
            fields.push(make_field(i, def, value));
        }
        make_block(class, fields, t.reserved_u32, t.wrapper_reserved, t.sentinel1, t.sentinel2)
    }

    fn build_value(
        &mut self,
        def: &FieldDef,
        v: &TValue,
        depth: usize,
    ) -> Result<(FieldKind, FieldValue), i32> {
        match v {
            TValue::Scalar(bytes) => {
                if !matches!(def.meta_kind, 0 | 2) || def.meta_size as usize != bytes.len() {
                    return Err(error::TEMPLATE_MISMATCH);
                }
                let value = scalar_from_bytes(bytes, &def.type_name, bytes.len());
                Ok((FieldKind::FixedPrefix, FieldValue::Scalar(value)))
            }
            TValue::Inline { elem_size, count, bytes } => {
                if def.meta_kind != 1 || def.meta_size != *elem_size {
                    return Err(error::TEMPLATE_MISMATCH);
                }
                Ok((FieldKind::InlineBytes, FieldValue::InlineBytes { count: *count, bytes: bytes.clone() }))
            }
            TValue::Array { elem_size, count, bytes } => {
                if def.meta_kind != 3 || def.meta_size != *elem_size {
                    return Err(error::TEMPLATE_MISMATCH);
                }
                // The engine's header for a present array: 0x00 tag + u32
                // count (the decoder's `generic` shape).
                let mut header_bytes = vec![0u8];
                header_bytes.extend_from_slice(&count.to_le_bytes());
                Ok((
                    FieldKind::DynamicArray,
                    FieldValue::DynamicArray {
                        count: *count,
                        bytes: bytes.clone(),
                        header_variant: "generic",
                        header_bytes,
                        trailer_bytes: Vec::new(),
                    },
                ))
            }
            TValue::Locator { meta_kind, prefix, child } => {
                if !matches!(def.meta_kind, 4 | 5) || def.meta_kind != *meta_kind as u16 {
                    return Err(error::TEMPLATE_MISMATCH);
                }
                let block = self.build(child, depth + 1)?;
                Ok((FieldKind::ObjectLocator, locator_value(block, prefix.clone())))
            }
            TValue::List { meta_kind, header, elements } => {
                if !matches!(def.meta_kind, 6 | 7)
                    || def.meta_kind != *meta_kind as u16
                    || header.len() != 18
                    || header[0] != 0
                {
                    return Err(error::TEMPLATE_MISMATCH);
                }
                let built = elements
                    .iter()
                    .map(|e| self.build(e, depth + 1))
                    .collect::<Result<Vec<_>, _>>()?;
                let count = u32::try_from(built.len()).map_err(|_| error::OUT_OF_RANGE)?;
                let mut header_bytes = header.clone();
                update_object_list_count_in_header(&mut header_bytes, "zero1_count_u24", count)?;
                Ok((
                    FieldKind::ObjectList,
                    FieldValue::ObjectList {
                        count,
                        header_variant: "zero1_count_u24",
                        header_bytes,
                        elements: built,
                    },
                ))
            }
        }
    }

    /// An inline object locator the template lacks: the game always writes
    /// one, so build it present with an empty child of the class the target
    /// save uses for this `(class, field)`.
    fn default_locator(
        &mut self,
        parent: &str,
        field: &str,
        depth: usize,
    ) -> Result<(FieldKind, FieldValue), i32> {
        let child_class = self
            .locator_classes
            .get(&(parent.to_string(), field.to_string()))
            .cloned()
            .ok_or(error::TEMPLATE_MISMATCH)?;
        let class = self.class(&child_class)?;
        let block = self.default_object(class, depth + 1)?;
        Ok((FieldKind::ObjectLocator, locator_value(block, Vec::new())))
    }

    /// Every field absent — arrays and lists with their markers — except
    /// inline object locators, which get an empty child of their own.
    fn default_object(&mut self, class: &TypeDef, depth: usize) -> Result<ObjectBlock, i32> {
        if depth > MAX_DEPTH {
            return Err(error::BODY_PARSE);
        }
        let mut fields = Vec::with_capacity(class.fields.len());
        for (i, def) in class.fields.iter().enumerate() {
            let value = if def.meta_kind == 4 {
                Some(self.default_locator(&class.name, &def.name, depth)?)
            } else {
                None
            };
            fields.push(make_field(i, def, value));
        }
        make_block(class, fields, 0, 0, GAME_SENTINEL, GAME_SENTINEL)
    }
}

fn locator_value(child: ObjectBlock, wrapper_prefix: Vec<u8>) -> FieldValue {
    let wrapper = child.locator_wrapper.clone().unwrap_or(ObjectLocatorWrapper {
        type_index: child.class_index as u16,
        child_reserved: 0,
        sentinel1: GAME_SENTINEL,
        sentinel2: GAME_SENTINEL,
        payload_offset: 0,
    });
    FieldValue::Locator {
        child_type_index: wrapper.type_index,
        child_type_name: child.class_name.clone(),
        // The encoder writes the real offset for the block's final position.
        child_payload_offset: 0,
        child_reserved: wrapper.child_reserved,
        child_sentinel1: wrapper.sentinel1,
        child_sentinel2: wrapper.sentinel2,
        wrapper_prefix,
        child: Some(Box::new(child)),
        inline_child: true,
    }
}

fn make_field(index: usize, def: &FieldDef, value: Option<(FieldKind, FieldValue)>) -> DecodedField {
    let (present, kind, value) = match value {
        Some((kind, value)) => (true, kind, value),
        None => (false, FieldKind::Absent, FieldValue::None),
    };
    let note = match &value {
        FieldValue::DynamicArray { header_variant, .. }
        | FieldValue::ObjectList { header_variant, .. } => (*header_variant).to_string(),
        _ => String::new(),
    };
    DecodedField {
        field_index: index as u32,
        name: def.name.clone(),
        type_name: def.type_name.clone(),
        meta_kind: def.meta_kind,
        meta_size: def.meta_size,
        meta_aux: def.meta_aux,
        present,
        absent_marker: !present && absent_kind_has_marker(def.meta_kind),
        kind,
        value,
        // Positions are refreshed by the re-decode that follows the insert.
        start: 0,
        end: 0,
        note,
    }
}

fn make_block(
    class: &TypeDef,
    fields: Vec<DecodedField>,
    reserved_u32: u32,
    wrapper_reserved: u8,
    sentinel1: u32,
    sentinel2: u32,
) -> Result<ObjectBlock, i32> {
    let mbc = class.fields.len().div_ceil(8).max(1);
    if mbc > 16 {
        return Err(error::OUT_OF_RANGE);
    }
    let mut mask_bytes = vec![0u8; mbc];
    for f in fields.iter().filter(|f| f.present) {
        mask_bytes[f.field_index as usize / 8] |= 1 << (f.field_index % 8);
    }
    let type_index = u16::try_from(class.index).map_err(|_| error::OUT_OF_RANGE)?;
    Ok(ObjectBlock {
        class_index: class.index,
        class_name: class.name.clone(),
        data_offset: 0,
        data_size: 0,
        mask_byte_count: mbc as u16,
        mask_bytes,
        reserved_u32,
        fields,
        trailing_pad: Vec::new(),
        undecoded_ranges: Vec::new(),
        locator_wrapper: Some(ObjectLocatorWrapper {
            type_index,
            child_reserved: wrapper_reserved,
            sentinel1,
            sentinel2,
            payload_offset: 0,
        }),
    })
}

// ── C ABI ──────────────────────────────────────────────────────────────────

/// Export one `object_list` element as a name-keyed element template (see
/// the module docs). Two-call buffer pattern: pass `buf = NULL,
/// buf_len = 0` to learn the size through `*out_required`, then call again
/// with a buffer that big.
///
/// Errors: `NULL_ARG`, `OUT_OF_RANGE` (bad block / path / element index),
/// `NOT_NAVIGABLE`, `NOT_OBJECT_LIST` (the field isn't a present list),
/// `BODY_PARSE` (the element is not fully decoded under the
/// absence-marker rule, so its values cannot be trusted),
/// `BUFFER_TOO_SMALL` (`*out_required` is still set).
///
/// # Safety
/// `handle` must be a live handle and `out_required` non-null. `path` must
/// point to `path_len` readable [`CrimsonPathStep`]s (or be NULL with
/// `path_len == 0`); `buf` must point to `buf_len` writable bytes (or be
/// NULL with `buf_len == 0`).
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn crimson_save_export_element_template(
    handle: *const CrimsonSaveHandle,
    block_idx: u32,
    path: *const CrimsonPathStep,
    path_len: usize,
    field_idx: u32,
    element_idx: u32,
    buf: *mut u8,
    buf_len: usize,
    out_required: *mut usize,
) -> i32 {
    if handle.is_null()
        || out_required.is_null()
        || (path.is_null() && path_len != 0)
        || (buf.is_null() && buf_len != 0)
    {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let _ffi_guard = save_ffi_lock();
        let h = unsafe { &*handle };
        let steps = unsafe { slice_from_raw_or_empty(path, path_len) };
        let parent = match navigate_to_parent_ref(&h.blocks, block_idx, steps) {
            Ok(p) => p,
            Err(code) => return code,
        };
        let Some(field) = parent.fields.get(field_idx as usize) else {
            return error::OUT_OF_RANGE;
        };
        let FieldValue::ObjectList { elements, .. } = &field.value else {
            return error::NOT_OBJECT_LIST;
        };
        let Some(element) = elements.get(element_idx as usize) else {
            return error::OUT_OF_RANGE;
        };
        let bytes = match template_from_block(element, 0).and_then(|t| encode_template(&t)) {
            Ok(b) => b,
            Err(code) => return code,
        };
        unsafe { *out_required = bytes.len() };
        if buf_len < bytes.len() {
            return error::BUFFER_TOO_SMALL;
        }
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len()) };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Build a list element from a name-keyed template under THIS save's schema
/// and insert it into an `object_list` field at `insert_at`. The field-by-
/// field rules are in the module docs; in short, fields match by name,
/// removed ones are dropped, new ones stay absent (arrays / lists with
/// their absence marker, inline objects with an empty child), and a changed
/// kind or size is refused.
///
/// `out_dropped_fields` (may be NULL) receives, on success, how many
/// template fields the target classes no longer have.
///
/// Errors: `NULL_ARG`, `OUT_OF_RANGE`, `NOT_NAVIGABLE`, `BODY_PARSE`
/// (malformed template), `TRANSPLANT_TYPE_MISSING` (a class the template
/// names is not in this save's schema), `TEMPLATE_MISMATCH`,
/// `NOT_OBJECT_LIST`, `LIST_VARIANT_UNSUPPORTED`, `MUTATION_INVALID` (the
/// re-encoded body failed to re-parse). On any error the save is untouched.
///
/// # Safety
/// `handle` must be a live, exclusive handle. `path` must point to
/// `path_len` readable [`CrimsonPathStep`]s (or be NULL with
/// `path_len == 0`); `template` must point to `template_len` readable
/// bytes.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn crimson_save_list_insert_element_template(
    handle: *mut CrimsonSaveHandle,
    block_idx: u32,
    path: *const CrimsonPathStep,
    path_len: usize,
    field_idx: u32,
    insert_at: u32,
    template: *const u8,
    template_len: usize,
    out_dropped_fields: *mut u32,
) -> i32 {
    if handle.is_null() || template.is_null() || (path.is_null() && path_len != 0) {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let _ffi_guard = save_ffi_lock();
        let h = unsafe { &mut *handle };
        let steps = unsafe { slice_from_raw_or_empty(path, path_len) };
        let t = match decode_template(unsafe { std::slice::from_raw_parts(template, template_len) }) {
            Ok(t) => t,
            Err(code) => return code,
        };
        let (element, dropped) = {
            let mut builder = Builder::new(&h.body.schema.types, &h.blocks);
            match builder.build(&t, 0) {
                Ok(e) => (e, builder.dropped),
                Err(code) => return code,
            }
        };
        let rc = apply_length_changing_mutation(h, move |blocks| {
            let field = navigate_mut_to_field(blocks, block_idx, steps, field_idx)?;
            let FieldValue::ObjectList { count, header_variant, header_bytes, elements } =
                &mut field.value
            else {
                return Err(error::NOT_OBJECT_LIST);
            };
            if insert_at as usize > elements.len() {
                return Err(error::OUT_OF_RANGE);
            }
            elements.insert(insert_at as usize, element);
            *count = elements.len() as u32;
            update_object_list_count_in_header(header_bytes, header_variant, *count)
        });
        if rc == error::OK && !out_dropped_fields.is_null() {
            unsafe { *out_dropped_fields = dropped };
        }
        rc
    }))
    .unwrap_or(error::PANIC)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::path::PathBuf;
    use std::ptr;

    fn def(name: &str, meta_kind: u16, meta_size: u16) -> FieldDef {
        FieldDef {
            name: name.into(),
            type_name: if meta_kind == 0 { "uint32".into() } else { "ReflectObject".into() },
            meta_kind,
            meta_size,
            meta_aux: 0,
            start_offset: 0,
            end_offset: 0,
        }
    }

    fn class(index: u32, name: &str, fields: Vec<FieldDef>) -> TypeDef {
        TypeDef { index, name: name.into(), fields, start_offset: 0, end_offset: 0 }
    }

    fn obj(class_name: &str, fields: Vec<(&str, TValue)>) -> TObject {
        TObject {
            class_name: class_name.into(),
            reserved_u32: 0,
            wrapper_reserved: 0,
            sentinel1: GAME_SENTINEL,
            sentinel2: GAME_SENTINEL,
            fields: fields.into_iter().map(|(n, value)| TField { name: n.into(), value }).collect(),
        }
    }

    /// A schema shaped like the dragon case: the template's class lost one
    /// field and gained a list, and the nested class gained an inline object.
    fn target_schema() -> Vec<TypeDef> {
        vec![
            class(1, "Merc", vec![
                def("_key", 0, 4),
                def("_levelData", 4, 8),
                def("_equip", 6, 0),
                def("_hp", 0, 8),
                def("_ship", 6, 0),
            ]),
            class(2, "Level", vec![def("_daily", 4, 8), def("_shareDaily", 4, 8)]),
            class(3, "Daily", vec![def("_time", 0, 8)]),
        ]
    }

    fn dragon_like() -> TObject {
        obj("Merc", vec![
            ("_key", TValue::Scalar(1_000_799u32.to_le_bytes().to_vec())),
            ("_levelData", TValue::Locator {
                meta_kind: 4,
                prefix: vec![],
                child: obj("Level", vec![("_daily", TValue::Locator {
                    meta_kind: 4,
                    prefix: vec![],
                    child: obj("Daily", vec![]),
                })]),
            }),
            ("_occupation", TValue::Scalar(vec![0])),
            ("_hp", TValue::Scalar(1032u64.to_le_bytes().to_vec())),
        ])
    }

    fn builder(types: &[TypeDef]) -> Builder<'_> {
        let mut b = Builder::new(types, &[]);
        // What a target save's own instances would teach it.
        b.locator_classes.insert(("Merc".into(), "_levelData".into()), "Level".into());
        b.locator_classes.insert(("Level".into(), "_daily".into()), "Daily".into());
        b.locator_classes.insert(("Level".into(), "_shareDaily".into()), "Daily".into());
        b
    }

    fn field(block: &ObjectBlock, name: &str) -> DecodedField {
        block.fields.iter().find(|f| f.name == name).unwrap().clone()
    }

    #[test]
    fn wire_format_roundtrips_and_rejects_garbage() {
        let t = dragon_like();
        let bytes = encode_template(&t).unwrap();
        assert_eq!(decode_template(&bytes).unwrap(), t);
        assert_eq!(decode_template(&bytes[..bytes.len() - 1]), Err(error::BODY_PARSE));
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert_eq!(decode_template(&trailing), Err(error::BODY_PARSE));
        let mut wrong_magic = bytes.clone();
        wrong_magic[0] = b'X';
        assert_eq!(decode_template(&wrong_magic), Err(error::BODY_PARSE));
    }

    #[test]
    fn builds_by_name_under_the_target_schema() {
        let types = target_schema();
        let mut b = builder(&types);
        let block = b.build(&dragon_like(), 0).unwrap();
        assert_eq!(b.dropped, 1, "_occupation is not in the target class");

        // Kept by name, bytes intact, in the target's field order.
        let hp = field(&block, "_hp");
        let FieldValue::Scalar(v) = &hp.value else { panic!("_hp should be a scalar") };
        let mut bytes = Vec::new();
        encode_scalar(&mut bytes, v, 8);
        assert_eq!(bytes, 1032u64.to_le_bytes());
        // Lists the template lacks: absent, with their 0x01 marker.
        for name in ["_equip", "_ship"] {
            let f = field(&block, name);
            assert!(!f.present && f.absent_marker, "{name}");
        }
        // Mask: _key (0), _levelData (1), _hp (3).
        assert_eq!(block.mask_bytes, vec![0b0000_1011]);
        // The nested class gained an inline object: built present, empty.
        let FieldValue::Locator { child: Some(level), .. } = field(&block, "_levelData").value else {
            panic!("_levelData should be a locator with a child");
        };
        let share = field(&level, "_shareDaily");
        assert!(share.present, "an inline object is never left absent");
        let FieldValue::Locator { child: Some(daily), inline_child: true, .. } = share.value else {
            panic!("_shareDaily should carry an inline child");
        };
        assert_eq!(daily.class_name, "Daily");
        assert!(daily.fields.iter().all(|f| !f.present));
        assert_eq!(daily.locator_wrapper.as_ref().unwrap().sentinel1, GAME_SENTINEL);
    }

    #[test]
    fn refuses_what_it_cannot_carry_over() {
        let types = target_schema();
        // A scalar whose width changed.
        let narrowed = obj("Merc", vec![("_key", TValue::Scalar(vec![1, 0]))]);
        assert_eq!(builder(&types).build(&narrowed, 0).unwrap_err(), error::TEMPLATE_MISMATCH);
        // A scalar where the target has a list.
        let rekinded = obj("Merc", vec![("_equip", TValue::Scalar(vec![1, 0, 0, 0]))]);
        assert_eq!(builder(&types).build(&rekinded, 0).unwrap_err(), error::TEMPLATE_MISMATCH);
        // A class the target schema does not have.
        assert_eq!(
            builder(&types).build(&obj("Gone", vec![]), 0).unwrap_err(),
            error::TRANSPLANT_TYPE_MISSING
        );
        // An inline object the target save gives no instance to learn from.
        let mut unlearned = builder(&types);
        unlearned.locator_classes.remove(&("Level".to_string(), "_shareDaily".to_string()));
        assert_eq!(unlearned.build(&dragon_like(), 0).unwrap_err(), error::TEMPLATE_MISMATCH);
    }

    // ── Live saves ─────────────────────────────────────────────────────────

    fn newest_live_save() -> Option<PathBuf> {
        let root = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Pearl Abyss/CD/save");
        let mut saves: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(root)
            .ok()?
            .flatten()
            .flat_map(|user| std::fs::read_dir(user.path()).into_iter().flatten().flatten())
            .map(|slot| slot.path().join("save.save"))
            .filter_map(|p| Some((std::fs::metadata(&p).ok()?.modified().ok()?, p)))
            .collect();
        saves.sort();
        saves.pop().map(|(_, p)| p)
    }

    fn load(path: &std::path::Path) -> *mut CrimsonSaveHandle {
        let c = CString::new(path.to_str().unwrap()).unwrap();
        let mut h: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(unsafe { super::super::crimson_save_load_from_file(c.as_ptr(), &mut h) }, error::OK);
        h
    }

    /// `(block index, field index, elements)` of `_mercenaryDataList`.
    fn merc_list(h: *mut CrimsonSaveHandle) -> (u32, u32, Vec<ObjectBlock>) {
        let blocks = unsafe { &(*h).blocks };
        for (bi, b) in blocks.iter().enumerate() {
            if b.class_name != "MercenaryClanSaveData" {
                continue;
            }
            for f in &b.fields {
                if f.name == "_mercenaryDataList"
                    && let FieldValue::ObjectList { elements, .. } = &f.value
                {
                    return (bi as u32, f.field_index, elements.clone());
                }
            }
        }
        panic!("no _mercenaryDataList");
    }

    fn export(h: *mut CrimsonSaveHandle, block: u32, field: u32, element: u32) -> Vec<u8> {
        let mut need = 0usize;
        let rc = unsafe {
            crimson_save_export_element_template(
                h, block, ptr::null(), 0, field, element, ptr::null_mut(), 0, &mut need,
            )
        };
        assert_eq!(rc, error::BUFFER_TOO_SMALL);
        let mut buf = vec![0u8; need];
        let rc = unsafe {
            crimson_save_export_element_template(
                h, block, ptr::null(), 0, field, element, buf.as_mut_ptr(), buf.len(), &mut need,
            )
        };
        assert_eq!(rc, error::OK);
        buf
    }

    fn insert(h: *mut CrimsonSaveHandle, block: u32, field: u32, at: usize, template: &[u8]) -> (i32, u32) {
        let mut dropped = u32::MAX;
        let rc = unsafe {
            crimson_save_list_insert_element_template(
                h, block, ptr::null(), 0, field, at as u32, template.as_ptr(), template.len(), &mut dropped,
            )
        };
        (rc, dropped)
    }

    /// Exporting a mercenary and inserting the template back into the same
    /// save yields an element whose own template is byte-identical — the
    /// builder loses nothing when nothing needs mapping.
    #[test]
    fn template_roundtrips_within_a_live_save() {
        let Some(path) = newest_live_save() else {
            eprintln!("skipping: no live save");
            return;
        };
        let h = load(&path);
        let (block, field, before) = merc_list(h);
        let template = export(h, block, field, 0);
        assert_eq!(insert(h, block, field, before.len(), &template), (error::OK, 0));
        assert_eq!(merc_list(h).2.len(), before.len() + 1);
        assert_eq!(export(h, block, field, before.len() as u32), template);
        unsafe { super::super::crimson_save_free(h) };
    }

    /// The case this exists for: a mercenary from a 1.10 save, whose
    /// `MercenarySaveData` still has `_occupationState`, goes into the newest
    /// live save under that save's schema — the removed field dropped, every
    /// shared field carried by name, new arrays / lists absent with markers.
    #[test]
    fn template_crosses_schemas_from_the_1_10_fixture() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/saves/1.10/save.save");
        let (Ok(data), Some(target)) = (std::fs::read(&fixture), newest_live_save()) else {
            eprintln!("skipping: fixture or live save missing");
            return;
        };
        if data.get(..4) != Some(b"SAVE") {
            eprintln!("skipping: fixture is git-crypt locked");
            return;
        }
        let src = load(&fixture);
        let (sb, sf, src_elements) = merc_list(src);
        let (idx, source) = src_elements
            .iter()
            .enumerate()
            .find(|(_, e)| e.fields.iter().any(|f| f.name == "_occupationState" && f.present))
            .expect("a 1.10 mercenary with _occupationState");
        let template = export(src, sb, sf, idx as u32);

        let dst = load(&target);
        let (db, df, dst_elements) = merc_list(dst);
        if dst_elements[0].fields.iter().any(|f| f.name == "_occupationState") {
            eprintln!("skipping: the newest live save still has _occupationState");
            return;
        }
        let (rc, dropped) = insert(dst, db, df, dst_elements.len(), &template);
        assert_eq!(rc, error::OK);
        assert!(dropped >= 1, "_occupationState should have been dropped");

        let built = merc_list(dst).2[dst_elements.len()].clone();
        for f in &built.fields {
            match source.fields.iter().find(|s| s.name == f.name) {
                Some(s) if s.present => {
                    assert!(f.present, "{} lost", f.name);
                    if let (FieldValue::Scalar(a), FieldValue::Scalar(b)) = (&s.value, &f.value) {
                        let (mut x, mut y) = (Vec::new(), Vec::new());
                        encode_scalar(&mut x, a, s.meta_size as usize);
                        encode_scalar(&mut y, b, f.meta_size as usize);
                        assert_eq!(x, y, "{} changed value", f.name);
                    }
                }
                _ => {
                    if f.meta_kind != 4 {
                        assert!(!f.present, "{} appeared from nowhere", f.name);
                    }
                    if !f.present && matches!(f.meta_kind, 3 | 6 | 7) {
                        assert!(f.absent_marker, "{} lost its absence marker", f.name);
                    }
                }
            }
        }
        unsafe {
            super::super::crimson_save_free(src);
            super::super::crimson_save_free(dst);
        }
    }
}
