//! Save body encoder — the structural inverse of [`super::decoder`].
//!
//! Phase B.1 ships only the encoder + golden tests; no in-crate caller
//! reaches into these functions yet (B.2 adds the C ABI wrappers). The
//! `#![allow(dead_code)]` below holds the door open without polluting
//! individual items.
#![allow(dead_code)]
//!
//! Takes a decoded `ObjectBlock` tree and re-emits the byte sequence the
//! decoder would have parsed. With the decoder's full set of preservation
//! fields (locator wrapper sentinels, dynamic-array header/trailer bytes,
//! object-list header bytes, inline-payload reserved_u32), the encoder
//! achieves byte-perfect round-trip identity against unmodified saves:
//!
//! ```text
//! Body::parse(raw) -> blocks  -> Body::write(blocks) == raw
//! ```
//!
//! This is the foundation for length-changing edits (PR B). For Phase B.1
//! the only consumer is the round-trip golden test in `lib.rs`; later
//! phases add a C ABI surface that lets callers mutate list shape (add /
//! remove elements) and re-emit through the same encoder.
//!
//! ## Field-walk ordering
//!
//! The decoder uses a bidirectional walk: a reverse pass peels trailing
//! fixed-size scalars off the tail, then a forward pass decodes the rest.
//! The encoder mirrors that ordering exactly:
//!
//! 1. Write block header (mask_byte_count u16 + mask + reserved_u32).
//! 2. For each field in index order, emit it **unless** its decoded kind
//!    is [`FieldKind::FixedSuffix`] (those go at the tail).
//! 3. Write `trailing_pad` bytes verbatim.
//! 4. For each [`FieldKind::FixedSuffix`] field in index order, emit it.
//!
//! ## Round-trip caveats
//!
//! - Blocks with `undecoded_ranges` are NOT round-trippable — those bytes
//!   were never captured by the decoder. The 1.06 test saves have zero
//!   undecoded bytes (enforced by `test_save_body_decode_all_blocks`), so
//!   this only matters if a future game patch drifts the format.
//! - Non-inline locators (where the child payload sits outside the
//!   wrapper's immediate `[wrapper_end..)` range) are NOT supported in
//!   B.1; the round-trip test would catch them. None observed in 1.06.

use std::io;

use super::Body;
use super::object::{
    DecodedField, FieldKind, FieldValue, ObjectBlock, ObjectLocatorWrapper, ScalarValue,
};

/// Encode a single top-level TOC block back to bytes.
///
/// The output matches the byte range `raw[block.data_offset..block.data_offset+block.data_size]`
/// for unmodified blocks parsed by the decoder.
pub fn encode_top_level_block(block: &ObjectBlock) -> io::Result<Vec<u8>> {
    let mut out = Vec::with_capacity(block.data_size as usize);
    write_block_header(&mut out, block);
    write_block_fields(&mut out, block)?;
    Ok(out)
}

/// Encode every block in `blocks` plus the surrounding body structure
/// (prefix + schema bytes copied verbatim from `prefix_schema_bytes`,
/// recomputed TOC header + entries, then the data section).
///
/// `prefix_schema_bytes` must be `raw[0..body.schema.schema_end]` from
/// the original body — the encoder does not re-emit the schema (its
/// layout is preserved verbatim across edits).
///
/// The TOC header's `stream_size` and every entry's `data_offset` are
/// recomputed from the encoded block sizes. `class_index`, `sentinel1`,
/// `sentinel2` are taken from the original [`Body::toc`].
pub fn encode_body(
    body: &Body,
    prefix_schema_bytes: &[u8],
    blocks: &[ObjectBlock],
) -> io::Result<Vec<u8>> {
    if prefix_schema_bytes.len() != body.schema.schema_end {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "prefix_schema_bytes len {} != schema_end {}",
                prefix_schema_bytes.len(),
                body.schema.schema_end,
            ),
        ));
    }
    if blocks.len() != body.toc.entries.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "blocks len {} != toc entry count {}",
                blocks.len(),
                body.toc.entries.len(),
            ),
        ));
    }

    // Encode each block first so we know its size.
    let mut block_bytes: Vec<Vec<u8>> = Vec::with_capacity(blocks.len());
    for block in blocks {
        block_bytes.push(encode_top_level_block(block)?);
    }

    let toc_header_size = 3 * 4;
    let toc_entry_size = 5 * 4;
    let toc_total = toc_header_size + blocks.len() * toc_entry_size;
    let data_start = body.schema.schema_end + toc_total;

    // Recompute data_offset for each block, accumulating from data_start.
    let mut offsets: Vec<u32> = Vec::with_capacity(blocks.len());
    let mut cursor = data_start;
    for bytes in &block_bytes {
        offsets.push(cursor as u32);
        cursor += bytes.len();
    }
    let stream_size = cursor as u32;

    let mut out = Vec::with_capacity(cursor);
    out.extend_from_slice(prefix_schema_bytes);

    // TOC header.
    write_u32(&mut out, body.toc.prefix_zero);
    write_u32(&mut out, blocks.len() as u32);
    write_u32(&mut out, stream_size);

    // TOC entries.
    for (i, original_entry) in body.toc.entries.iter().enumerate() {
        write_u32(&mut out, original_entry.class_index);
        write_u32(&mut out, original_entry.sentinel1);
        write_u32(&mut out, original_entry.sentinel2);
        write_u32(&mut out, offsets[i]);
        write_u32(&mut out, block_bytes[i].len() as u32);
    }

    // Data section.
    for bytes in &block_bytes {
        out.extend_from_slice(bytes);
    }

    Ok(out)
}

// ── Block header / field-walk dispatch ─────────────────────────────────────

fn write_block_header(out: &mut Vec<u8>, block: &ObjectBlock) {
    write_u16(out, block.mask_byte_count);
    out.extend_from_slice(&block.mask_bytes);
    write_u32(out, block.reserved_u32);
}

fn write_block_fields(out: &mut Vec<u8>, block: &ObjectBlock) -> io::Result<()> {
    // Forward pass: emit every field except FixedSuffix in index order.
    for field in &block.fields {
        if field.kind == FieldKind::FixedSuffix {
            continue;
        }
        encode_field(out, field)?;
    }
    // Trailing pad sits between forward fields and the reverse-peeled tail.
    out.extend_from_slice(&block.trailing_pad);
    // Reverse pass: emit FixedSuffix fields in index order.
    for field in &block.fields {
        if field.kind != FieldKind::FixedSuffix {
            continue;
        }
        encode_field(out, field)?;
    }
    Ok(())
}

fn encode_field(out: &mut Vec<u8>, field: &DecodedField) -> io::Result<()> {
    match field.kind {
        FieldKind::Absent | FieldKind::Unknown => Ok(()),
        FieldKind::FixedPrefix | FieldKind::FixedSuffix => match &field.value {
            FieldValue::Scalar(v) => {
                encode_scalar(out, v, field.meta_size as usize);
                Ok(())
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "field '{}' kind={:?} but value is not a Scalar",
                    field.name, field.kind
                ),
            )),
        },
        FieldKind::InlineBytes => match &field.value {
            FieldValue::InlineBytes { count, bytes } => {
                write_u32(out, *count);
                out.extend_from_slice(bytes);
                Ok(())
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("field '{}' kind=InlineBytes but value mismatch", field.name),
            )),
        },
        FieldKind::DynamicArray => match &field.value {
            FieldValue::DynamicArray {
                bytes,
                header_bytes,
                trailer_bytes,
                ..
            } => {
                out.extend_from_slice(header_bytes);
                out.extend_from_slice(bytes);
                out.extend_from_slice(trailer_bytes);
                Ok(())
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "field '{}' kind=DynamicArray but value mismatch",
                    field.name
                ),
            )),
        },
        FieldKind::ObjectLocator => encode_locator_field(out, field),
        FieldKind::ObjectList => match &field.value {
            FieldValue::ObjectList {
                header_bytes,
                elements,
                ..
            } => {
                out.extend_from_slice(header_bytes);
                for element in elements {
                    encode_list_element(out, element)?;
                }
                Ok(())
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("field '{}' kind=ObjectList but value mismatch", field.name),
            )),
        },
    }
}

fn encode_locator_field(out: &mut Vec<u8>, field: &DecodedField) -> io::Result<()> {
    let FieldValue::Locator {
        child_type_index,
        child_payload_offset,
        child_reserved,
        child_sentinel1,
        child_sentinel2,
        wrapper_prefix,
        child,
        ..
    } = &field.value
    else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "field '{}' kind=ObjectLocator but value mismatch",
                field.name
            ),
        ));
    };
    // Kind 5 locators may have 0..=8 prefix bytes ahead of the wrapper.
    out.extend_from_slice(wrapper_prefix);
    // We need the child mask + mbc to emit the wrapper. The decoder always
    // populates them on the child block (when child is Some). For empty
    // locators (child = None), we have no mask bytes to emit — that would
    // produce truncated output. Surface this loudly: if it happens on a
    // real save, we need to add a separate "wrapper-only mask" field to
    // FieldValue::Locator (none observed in 1.06).
    let child_block = child.as_deref().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "field '{}': locator with no child — encoder can't reconstruct wrapper mask",
                field.name
            ),
        )
    })?;
    let mbc = child_block.mask_byte_count;
    write_u16(out, mbc);
    if child_block.mask_bytes.len() != mbc as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "field '{}': child mask_bytes len {} != mbc {}",
                field.name,
                child_block.mask_bytes.len(),
                mbc
            ),
        ));
    }
    out.extend_from_slice(&child_block.mask_bytes);
    write_u16(out, *child_type_index);
    out.push(*child_reserved);
    write_u32(out, *child_sentinel1);
    write_u32(out, *child_sentinel2);
    write_u32(out, *child_payload_offset);

    // Inline child payload? The decoder advanced `end = child_end` only
    // when payload_start == wrapper_end. For non-inline cases (payload
    // lives elsewhere) the field's start..end covers wrapper_prefix +
    // wrapper bytes only. Wrapper bytes = mbc + 17 (u16 mbc + mbc mask
    // + u16 type_index + u8 reserved + u32×3 sentinel/offset = mbc+17).
    let wrapper_size = wrapper_prefix.len() + (mbc as usize) + 17;
    let field_len = field.end.saturating_sub(field.start);
    if field_len > wrapper_size {
        // Inline child payload follows. Emit it.
        encode_inline_payload(out, child_block)?;
    }
    Ok(())
}

fn encode_list_element(out: &mut Vec<u8>, element: &ObjectBlock) -> io::Result<()> {
    let wrapper = element.locator_wrapper.as_ref().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "list element class={} missing locator_wrapper",
                element.class_name
            ),
        )
    })?;
    encode_list_element_wrapper(out, element, wrapper);
    encode_inline_payload(out, element)
}

fn encode_list_element_wrapper(
    out: &mut Vec<u8>,
    element: &ObjectBlock,
    wrapper: &ObjectLocatorWrapper,
) {
    write_u16(out, element.mask_byte_count);
    out.extend_from_slice(&element.mask_bytes);
    write_u16(out, wrapper.type_index);
    out.push(wrapper.child_reserved);
    write_u32(out, wrapper.sentinel1);
    write_u32(out, wrapper.sentinel2);
    write_u32(out, wrapper.payload_offset);
}

/// Inline object payload: `u32 reserved | fields | pad | u32 trailing_size`.
///
/// The trailing size is `(size_at - payload_start)` where `size_at` is the
/// byte offset of the size u32 itself, measured from the start of the
/// payload (the `reserved` u32). `pad` is the bytes the decoder couldn't
/// claim under any field — captured on the child block as `trailing_pad`
/// so the encoder can splat them back here (no field ownership for them,
/// but they need to round-trip).
fn encode_inline_payload(out: &mut Vec<u8>, child: &ObjectBlock) -> io::Result<()> {
    let payload_start = out.len();
    write_u32(out, child.reserved_u32);
    write_inline_payload_fields(out, child)?;
    out.extend_from_slice(&child.trailing_pad);
    let trailing_size = (out.len() - payload_start) as u32;
    write_u32(out, trailing_size);
    Ok(())
}

fn write_inline_payload_fields(out: &mut Vec<u8>, child: &ObjectBlock) -> io::Result<()> {
    // Inline payload field walk is FORWARD-ONLY (no reverse pass) — see
    // decoder::decode_inline_object_payload. So no FixedSuffix handling,
    // no trailing_pad split; just walk fields in order.
    for field in &child.fields {
        encode_field(out, field)?;
    }
    Ok(())
}

// ── Scalar encoders ────────────────────────────────────────────────────────

fn encode_scalar(out: &mut Vec<u8>, value: &ScalarValue, meta_size: usize) {
    match value {
        ScalarValue::Bool(b) => out.push(if *b { 1 } else { 0 }),
        ScalarValue::U8(x) => out.push(*x),
        ScalarValue::U16(x) => out.extend_from_slice(&x.to_le_bytes()),
        ScalarValue::U32(x) => out.extend_from_slice(&x.to_le_bytes()),
        ScalarValue::U64(x) => out.extend_from_slice(&x.to_le_bytes()),
        ScalarValue::I8(x) => out.push(*x as u8),
        ScalarValue::I16(x) => out.extend_from_slice(&x.to_le_bytes()),
        ScalarValue::I32(x) => out.extend_from_slice(&x.to_le_bytes()),
        ScalarValue::I64(x) => out.extend_from_slice(&x.to_le_bytes()),
        ScalarValue::F32(x) => out.extend_from_slice(&x.to_le_bytes()),
        ScalarValue::F64(x) => out.extend_from_slice(&x.to_le_bytes()),
        ScalarValue::Bytes(b) => {
            // Raw-bytes fallback: emit verbatim. The decoder takes this
            // path for non-power-of-2 sizes / unrecognized type names;
            // the bytes are the original on-disk bytes.
            debug_assert_eq!(
                b.len(),
                meta_size,
                "ScalarValue::Bytes len {} != meta_size {}",
                b.len(),
                meta_size
            );
            out.extend_from_slice(b);
        }
    }
}

// ── Tiny writers ───────────────────────────────────────────────────────────

#[inline]
fn write_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

#[inline]
fn write_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
