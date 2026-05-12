//! Per-object field decoder for the save body.
//!
//! Each [`super::toc::TocEntry`] points to a `(data_offset, data_size)`
//! slice in the body. That slice's layout is:
//!
//! ```text
//! u16 mask_byte_count
//! u8  mask_bytes[mask_byte_count]   // one presence bit per field
//! u32 reserved_u32
//! ...field payloads, decoded per the schema's TypeDef...
//! ```
//!
//! The presence mask gates whether a given field is encoded at all. The
//! field walk itself is **bidirectional**: trailing fixed-size fields are
//! peeled off the tail first, then the remaining fields are decoded
//! forward from the head. This is what the Python `save_parser.py`
//! does and we mirror it byte-for-byte so existing fixtures keep matching.
//!
//! Decode failures are tolerated: when a field decoder hits a structural
//! issue we stop walking forward from that point and let the caller see
//! the residue in the block's `undecoded_ranges`. This matches the
//! Python parser's defensive style — better to decode 95% of a save
//! than fail outright on one unfamiliar field shape.

use std::collections::HashMap;
use std::io;

use super::object::{
    DecodedField, FieldKind, FieldValue, ObjectBlock, ScalarValue, decode_scalar, field_present,
    type_index_map,
};
use super::schema::{FieldDef, Schema, TypeDef};
use super::toc::Toc;

/// Decode every TOC entry whose `class_index` resolves to a schema type.
/// Entries with unknown class indices are skipped silently (matches the
/// Python parser's tolerance).
pub fn decode_blocks(raw: &[u8], schema: &Schema, toc: &Toc) -> Vec<ObjectBlock> {
    let by_index = type_index_map(&schema.types);
    toc.entries
        .iter()
        .filter_map(|entry| {
            let type_def = by_index.get(&entry.class_index).copied()?;
            decode_one_block(raw, entry, type_def, &by_index)
        })
        .collect()
}

fn decode_one_block(
    raw: &[u8],
    entry: &super::toc::TocEntry,
    type_def: &TypeDef,
    by_index: &HashMap<u32, &TypeDef>,
) -> Option<ObjectBlock> {
    let block_start = entry.data_offset as usize;
    let block_end = (entry.data_offset as usize + entry.data_size as usize).min(raw.len());
    if block_end <= block_start || block_start + 2 > block_end {
        return None;
    }

    let expected_mask_bytes = type_def.fields.len().div_ceil(8).max(1);
    let actual_mask_bytes = u16::from_le_bytes(raw[block_start..block_start + 2].try_into().ok()?);
    let mask_byte_count = if (1..=16).contains(&actual_mask_bytes) {
        actual_mask_bytes as usize
    } else {
        expected_mask_bytes
    };

    let header_end = block_start + 2 + mask_byte_count + 4;
    if header_end > block_end {
        return None;
    }
    let mask_bytes = raw[block_start + 2..block_start + 2 + mask_byte_count].to_vec();
    let reserved_u32 = u32::from_le_bytes(
        raw[block_start + 2 + mask_byte_count..block_start + 2 + mask_byte_count + 4]
            .try_into()
            .ok()?,
    );

    let (fields, mut undecoded) =
        decode_fields_in_region(raw, type_def, &mask_bytes, header_end, block_end, by_index);

    // Absorb the well-known 1-byte trailer between the forward-walk end
    // and the reverse-peel start (or, when no reverse pass ran, between
    // the forward end and `block_end`). Both this Rust decoder and the
    // original Python parser leave this byte unconsumed otherwise. The
    // value varies per block (it's data, not a constant marker), so we
    // capture it on the block for round-trip rather than dropping it.
    // See docs/save-body-format.md.
    let trailing_pad = if undecoded.len() == 1 && undecoded[0].1 - undecoded[0].0 == 1 {
        let (s, _) = undecoded[0];
        let byte = raw.get(s).copied();
        if byte.is_some() {
            undecoded.clear();
        }
        byte
    } else {
        None
    };

    Some(ObjectBlock {
        class_index: entry.class_index,
        class_name: type_def.name.clone(),
        data_offset: entry.data_offset,
        data_size: entry.data_size,
        mask_byte_count: mask_byte_count as u16,
        mask_bytes,
        reserved_u32,
        fields,
        trailing_pad,
        undecoded_ranges: undecoded,
    })
}

/// Walk a region's fields head/tail-style, producing a `DecodedField` per
/// schema field plus a list of byte ranges nothing placed.
pub(crate) fn decode_fields_in_region(
    raw: &[u8],
    type_def: &TypeDef,
    mask_bytes: &[u8],
    region_start: usize,
    region_end: usize,
    by_index: &HashMap<u32, &TypeDef>,
) -> (Vec<DecodedField>, Vec<(usize, usize)>) {
    // Seed with one DecodedField per schema field, all "unknown" until a
    // decoder places them.
    let mut fields: Vec<DecodedField> = type_def
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| DecodedField {
            field_index: i as u32,
            name: f.name.clone(),
            type_name: f.type_name.clone(),
            meta_kind: f.meta_kind,
            meta_size: f.meta_size,
            meta_aux: f.meta_aux,
            present: field_present(mask_bytes, i),
            kind: if field_present(mask_bytes, i) {
                FieldKind::Unknown
            } else {
                FieldKind::Absent
            },
            value: FieldValue::None,
            start: 0,
            end: 0,
            note: String::new(),
        })
        .collect();

    // Reverse pass: peel trailing fixed-size scalars from the tail.
    let mut tail_cursor = region_end;
    for index in (0..type_def.fields.len()).rev() {
        let field = &type_def.fields[index];
        let target = &mut fields[index];
        if !target.present {
            continue;
        }
        if field.meta_kind != 0 && field.meta_kind != 2 {
            break;
        }
        let size = field.meta_size as usize;
        if size == 0 || tail_cursor < region_start + size {
            break;
        }
        let start = tail_cursor - size;
        target.kind = FieldKind::FixedSuffix;
        target.value = FieldValue::Scalar(decode_scalar(&raw[start..tail_cursor], field));
        target.start = start;
        target.end = tail_cursor;
        tail_cursor = start;
    }

    // Forward pass: decode head fields until we hit an issue. Each decoder
    // returns either Ok(new_cursor) or breaks out, leaving the field as
    // Unknown and the remaining bytes in undecoded_ranges.
    let mut head_cursor = region_start;
    'outer: for index in 0..type_def.fields.len() {
        let field = &type_def.fields[index];
        let target_present = fields[index].present;
        let target_already = fields[index].kind != FieldKind::Unknown
            && fields[index].kind != FieldKind::Absent;
        if !target_present {
            continue;
        }
        if target_already {
            // Already placed by the reverse pass.
            continue;
        }
        if head_cursor >= tail_cursor {
            break;
        }

        match field.meta_kind {
            0 | 2 if field.meta_size > 0 => {
                let size = field.meta_size as usize;
                if head_cursor + size > tail_cursor {
                    break 'outer;
                }
                let target = &mut fields[index];
                target.kind = FieldKind::FixedPrefix;
                target.value =
                    FieldValue::Scalar(decode_scalar(&raw[head_cursor..head_cursor + size], field));
                target.start = head_cursor;
                target.end = head_cursor + size;
                head_cursor += size;
            }
            1 if field.meta_size > 0 => {
                match decode_inline_bytes(raw, head_cursor, field, tail_cursor) {
                    Ok((end, value)) => {
                        let target = &mut fields[index];
                        target.kind = FieldKind::InlineBytes;
                        target.value = value;
                        target.start = head_cursor;
                        target.end = end;
                        head_cursor = end;
                    }
                    Err(_) => break 'outer,
                }
            }
            3 if field.meta_size > 0 => {
                match decode_dynamic_array(raw, head_cursor, field, tail_cursor) {
                    Ok((end, value, header_variant)) => {
                        let target = &mut fields[index];
                        target.kind = FieldKind::DynamicArray;
                        target.value = value;
                        target.start = head_cursor;
                        target.end = end;
                        target.note = header_variant.into();
                        head_cursor = end;
                    }
                    Err(_) => break 'outer,
                }
            }
            4 | 5 => {
                match decode_inline_object_locator(raw, head_cursor, tail_cursor, by_index, field.meta_kind) {
                    Ok((end, value)) => {
                        let target = &mut fields[index];
                        target.kind = FieldKind::ObjectLocator;
                        target.value = value;
                        target.start = head_cursor;
                        target.end = end;
                        head_cursor = end;
                    }
                    Err(_) => break 'outer,
                }
            }
            6 | 7 => match decode_object_list(raw, head_cursor, tail_cursor, by_index) {
                Ok((end, value, header_variant)) => {
                    let target = &mut fields[index];
                    target.kind = FieldKind::ObjectList;
                    target.value = value;
                    target.start = head_cursor;
                    target.end = end;
                    target.note = header_variant.into();
                    head_cursor = end;
                }
                Err(_) => break 'outer,
            },
            _ => break 'outer,
        }
    }

    let undecoded = compute_undecoded_ranges(region_start, region_end, &fields);
    (fields, undecoded)
}

fn compute_undecoded_ranges(
    region_start: usize,
    region_end: usize,
    fields: &[DecodedField],
) -> Vec<(usize, usize)> {
    let mut decoded: Vec<(usize, usize)> = Vec::new();
    collect_decoded_ranges(fields, &mut decoded);
    decoded.sort_by_key(|r| r.0);

    let mut spans = Vec::new();
    let mut cursor = region_start;
    for (s, e) in decoded {
        if cursor < s {
            spans.push((cursor, s));
        }
        if e > cursor {
            cursor = e;
        }
    }
    if cursor < region_end {
        spans.push((cursor, region_end));
    }
    spans
}

/// Collect every byte range that some decoder claimed — top-level field
/// ranges plus the byte ranges of any nested child / list-element blocks.
/// Used to account for non-inline locator payloads that sit elsewhere in
/// the block and whose bytes wouldn't otherwise be covered by the
/// top-level field walk.
fn collect_decoded_ranges(fields: &[DecodedField], out: &mut Vec<(usize, usize)>) {
    for f in fields {
        if f.start < f.end {
            out.push((f.start, f.end));
        }
        match &f.value {
            FieldValue::Locator { child: Some(c), .. } => {
                out.push((
                    c.data_offset as usize,
                    (c.data_offset + c.data_size) as usize,
                ));
                collect_decoded_ranges(&c.fields, out);
            }
            FieldValue::ObjectList { elements, .. } => {
                for el in elements {
                    out.push((
                        el.data_offset as usize,
                        (el.data_offset + el.data_size) as usize,
                    ));
                    collect_decoded_ranges(&el.fields, out);
                }
            }
            _ => {}
        }
    }
}

// ── Field decoders ─────────────────────────────────────────────────────────

fn decode_inline_bytes(
    raw: &[u8],
    offset: usize,
    field: &FieldDef,
    tail_cursor: usize,
) -> io::Result<(usize, FieldValue)> {
    if offset + 4 > tail_cursor {
        return overrun("inline bytes");
    }
    let count = read_u32(raw, offset);
    let total = 4 + (count as usize).saturating_mul(field.meta_size as usize);
    let end = offset
        .checked_add(total)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "inline bytes size overflow"))?;
    if end > tail_cursor {
        return overrun("inline bytes");
    }
    let bytes = raw[offset + 4..end].to_vec();
    Ok((end, FieldValue::InlineBytes { count, bytes }))
}

fn decode_dynamic_array(
    raw: &[u8],
    offset: usize,
    field: &FieldDef,
    tail_cursor: usize,
) -> io::Result<(usize, FieldValue, &'static str)> {
    if offset + 5 > tail_cursor {
        return overrun("dynamic array");
    }

    // Variant 1: prefix `00 00 XX 01 00`, count u32, data, trailing
    // `01 01 01 01 01`. The 3rd byte (XX) is a flag whose meaning we
    // don't yet know — observed values include 0x06 (most arrays) and
    // 0x01 (e.g. FactionNodeElementSaveData._reviveQuestList). The
    // surrounding constraints (fixed bytes at 0,1,3,4 + count < 0x10000
    // + 5-byte trailing pattern) are strict enough that allowing any
    // byte at position 2 doesn't risk false matches on real data.
    if offset + 14 <= tail_cursor
        && raw[offset] == 0
        && raw[offset + 1] == 0
        && raw[offset + 3] == 1
        && raw[offset + 4] == 0
    {
        let count = read_u32(raw, offset + 5);
        let total = 9 + (count as usize) * (field.meta_size as usize) + 5;
        let end = offset + total;
        if count < 0x10000 && end <= tail_cursor && raw[end - 5..end] == [1u8; 5] {
            let data_offset = offset + 9;
            let data_end = data_offset + (count as usize) * (field.meta_size as usize);
            let bytes = raw[data_offset..data_end].to_vec();
            return Ok((
                end,
                FieldValue::DynamicArray {
                    count,
                    bytes,
                    header_variant: "prefix_00xx0100",
                },
                "prefix_00xx0100",
            ));
        }
    }

    // Variant 2: 1..N leading `0x01` bytes, then `0x00`, then u32 count.
    if raw[offset] == 1 && offset + 7 <= tail_cursor {
        let mut marker_end = offset;
        while marker_end < tail_cursor && raw[marker_end] == 1 {
            marker_end += 1;
        }
        if marker_end > offset
            && marker_end < tail_cursor
            && raw[marker_end] == 0
            && marker_end + 5 <= tail_cursor
        {
            let count = read_u32(raw, marker_end + 1);
            let total = (marker_end - offset + 1) + 4 + (count as usize) * (field.meta_size as usize);
            let mut end = offset + total;
            if count < 0x10000 && end <= tail_cursor {
                if end < tail_cursor && raw[end] == 1 {
                    end += 1;
                }
                let data_offset = marker_end + 5;
                let data_end = data_offset + (count as usize) * (field.meta_size as usize);
                let bytes = raw[data_offset..data_end].to_vec();
                return Ok((
                    end,
                    FieldValue::DynamicArray {
                        count,
                        bytes,
                        header_variant: "marker_prefix",
                    },
                    "marker_prefix",
                ));
            }
        }
    }

    // Variant 3: compact header `00 00 <u16 count> 00 00`.
    let compact = offset + 6 <= tail_cursor
        && raw[offset] == 0
        && raw[offset + 1] == 0
        && raw[offset + 4] == 0
        && raw[offset + 5] == 0;
    let (count, total, header_variant, data_skip) = if compact {
        let cnt = read_u16(raw, offset + 2) as u32;
        let total = 6 + (cnt as usize) * (field.meta_size as usize);
        (cnt, total, "compact", 6)
    } else {
        // Variant 4: generic `<u8 prefix> <u32 count> <data>`.
        let cnt = read_u32(raw, offset + 1);
        let total = 1 + 4 + (cnt as usize) * (field.meta_size as usize);
        (cnt, total, "generic", 5)
    };
    let end = offset + total;
    if end > tail_cursor {
        return overrun("dynamic array");
    }
    let data_offset = offset + data_skip;
    let bytes = raw[data_offset..end].to_vec();
    Ok((
        end,
        FieldValue::DynamicArray {
            count,
            bytes,
            header_variant,
        },
        header_variant,
    ))
}

fn decode_inline_object_locator(
    raw: &[u8],
    cursor: usize,
    tail_cursor: usize,
    by_index: &HashMap<u32, &TypeDef>,
    locator_kind: u16,
) -> io::Result<(usize, FieldValue)> {
    // Kind 5 hides the wrapper behind up to 8 prefix bytes; scan for the
    // first plausible mask_byte_count u16 in 1..=16.
    let body_cursor = if locator_kind == 5 {
        let mut found: Option<usize> = None;
        for delta in 0..=8usize {
            let probe = cursor + delta;
            if probe + 2 > tail_cursor {
                continue;
            }
            let mbc = read_u16(raw, probe);
            if (1..=16).contains(&mbc) {
                found = Some(probe);
                break;
            }
        }
        found.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "kind5 locator: no plausible mask_byte_count"))?
    } else {
        cursor
    };

    if body_cursor + 18 > tail_cursor {
        return overrun("object locator");
    }
    let mask_byte_count = read_u16(raw, body_cursor);
    if !(1..=16).contains(&mask_byte_count) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "locator: invalid mask_byte_count",
        ));
    }
    let mbc = mask_byte_count as usize;
    let wrapper_end = body_cursor + 2 + mbc + 2 + 1 + 4 + 4 + 4;
    if wrapper_end > tail_cursor {
        return overrun("object locator");
    }
    // Skipped fields here (kept for reference, not currently surfaced):
    //   child_mask_bytes = raw[body_cursor + 2 .. body_cursor + 2 + mbc]
    //   child_reserved_u8 = raw[body_cursor + 2 + mbc + 2]
    //   child_sentinel1_u32 = u32 at body_cursor + 2 + mbc + 3
    //   child_sentinel2_u32 = u32 at body_cursor + 2 + mbc + 7
    let child_type_index = read_u16(raw, body_cursor + 2 + mbc);
    let child_payload_offset = read_u32(raw, body_cursor + 2 + mbc + 11);

    let child_type_def = by_index.get(&(child_type_index as u32)).copied();
    let child_type_name = child_type_def
        .map(|t| t.name.clone())
        .unwrap_or_else(|| format!("<type_{child_type_index}>"));

    let mut end = wrapper_end;
    let mut child_block: Option<Box<ObjectBlock>> = None;

    if let Some(td) = child_type_def {
        let child_mask_owned = raw[body_cursor + 2..body_cursor + 2 + mbc].to_vec();

        // The wrapper's `child_payload_offset` is informational: most of
        // the time it equals `wrapper_end` (truly inline), but in some
        // blocks the engine writes a different value while the actual
        // payload still sits at wrapper_end (observed in FactionSaveData
        // list elements). So we always try inline first, then fall back
        // to the stated offset.
        let try_payload = |start: usize, tail: usize| {
            if start + 8 > tail {
                return None;
            }
            decode_inline_object_payload(raw, td, &child_mask_owned, start, tail, by_index)
                .ok()
                .map(|(child_end, _, _, fields)| (start, child_end, fields))
        };

        let mut decoded = try_payload(wrapper_end, tail_cursor);
        if decoded.is_none() && (child_payload_offset as usize) != wrapper_end {
            decoded = try_payload(child_payload_offset as usize, raw.len());
        }

        if let Some((payload_start, child_end, fields)) = decoded {
            if payload_start == wrapper_end {
                // Inline: outer forward cursor advances past wrapper + child.
                end = child_end;
            }
            child_block = Some(Box::new(ObjectBlock {
                class_index: child_type_index as u32,
                class_name: td.name.clone(),
                data_offset: payload_start as u32,
                data_size: (child_end - payload_start) as u32,
                mask_byte_count: mbc as u16,
                mask_bytes: child_mask_owned,
                reserved_u32: 0,
                fields,
                trailing_pad: None,
                undecoded_ranges: Vec::new(),
            }));
        }
    }

    let value = FieldValue::Locator {
        child_type_index,
        child_type_name,
        child_payload_offset,
        child: child_block,
    };
    Ok((end, value))
}

/// Decode one inline-object payload (used by the locator path when the
/// child sits right after the wrapper). The payload terminates at a
/// trailing `u32 size` equal to `(probe_offset - payload_start)`.
fn decode_inline_object_payload(
    raw: &[u8],
    type_def: &TypeDef,
    mask_bytes: &[u8],
    payload_start: usize,
    tail_cursor: usize,
    by_index: &HashMap<u32, &TypeDef>,
) -> io::Result<(usize, u32, u32, Vec<DecodedField>)> {
    if payload_start + 8 > tail_cursor {
        return overrun("inline object payload");
    }
    let reserved_u32 = read_u32(raw, payload_start);
    let mut cursor = payload_start + 4;

    let mut fields: Vec<DecodedField> = type_def
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| DecodedField {
            field_index: i as u32,
            name: f.name.clone(),
            type_name: f.type_name.clone(),
            meta_kind: f.meta_kind,
            meta_size: f.meta_size,
            meta_aux: f.meta_aux,
            present: field_present(mask_bytes, i),
            kind: if field_present(mask_bytes, i) {
                FieldKind::Unknown
            } else {
                FieldKind::Absent
            },
            value: FieldValue::None,
            start: 0,
            end: 0,
            note: String::new(),
        })
        .collect();

    // Field walk is best-effort: if a field's decode fails, we break out
    // of the loop and leave the remaining fields as `Unknown`. The
    // trailing-size probe below then locates the payload's true end from
    // wherever `cursor` got stuck. This is the only way to recover from
    // unfamiliar sub-format variants (e.g. an unknown dynamic_array
    // header in one of FactionNodeElementSaveData's quest lists) without
    // abandoning the rest of the block.
    for (index, field) in type_def.fields.iter().enumerate() {
        if !fields[index].present {
            continue;
        }
        match field.meta_kind {
            0 | 2 if field.meta_size > 0 => {
                let size = field.meta_size as usize;
                if cursor + size > tail_cursor {
                    break;
                }
                fields[index].kind = FieldKind::FixedPrefix;
                fields[index].value =
                    FieldValue::Scalar(decode_scalar(&raw[cursor..cursor + size], field));
                fields[index].start = cursor;
                fields[index].end = cursor + size;
                cursor += size;
            }
            1 if field.meta_size > 0 => {
                match decode_inline_bytes(raw, cursor, field, tail_cursor) {
                    Ok((end, value)) => {
                        fields[index].kind = FieldKind::InlineBytes;
                        fields[index].value = value;
                        fields[index].start = cursor;
                        fields[index].end = end;
                        cursor = end;
                    }
                    Err(_) => break,
                }
            }
            3 if field.meta_size > 0 => {
                match decode_dynamic_array(raw, cursor, field, tail_cursor) {
                    Ok((end, value, hv)) => {
                        fields[index].kind = FieldKind::DynamicArray;
                        fields[index].value = value;
                        fields[index].start = cursor;
                        fields[index].end = end;
                        fields[index].note = hv.into();
                        cursor = end;
                    }
                    Err(_) => break,
                }
            }
            4 | 5 => {
                match decode_inline_object_locator(
                    raw,
                    cursor,
                    tail_cursor,
                    by_index,
                    field.meta_kind,
                ) {
                    Ok((end, value)) => {
                        fields[index].kind = FieldKind::ObjectLocator;
                        fields[index].value = value;
                        fields[index].start = cursor;
                        fields[index].end = end;
                        cursor = end;
                    }
                    Err(_) => break,
                }
            }
            6 | 7 => {
                match decode_object_list(raw, cursor, tail_cursor, by_index) {
                    Ok((end, value, hv)) => {
                        fields[index].kind = FieldKind::ObjectList;
                        fields[index].value = value;
                        fields[index].start = cursor;
                        fields[index].end = end;
                        fields[index].note = hv.into();
                        cursor = end;
                    }
                    Err(_) => break,
                }
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unsupported inline field kind {}", field.meta_kind),
                ));
            }
        }
    }

    // The payload ends at a probe-located `u32 size = probe - payload_start`.
    let mut size_at = None;
    let max_probe = tail_cursor.saturating_sub(4);
    let mut probe = cursor;
    while probe <= max_probe {
        let candidate = read_u32(raw, probe) as usize;
        if candidate == probe - payload_start {
            size_at = Some(probe);
            break;
        }
        probe += 1;
    }
    let size_at = size_at.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "inline object missing trailing size",
        )
    })?;
    let size_u32 = read_u32(raw, size_at);
    Ok((size_at + 4, reserved_u32, size_u32, fields))
}

fn decode_object_list(
    raw: &[u8],
    cursor: usize,
    tail_cursor: usize,
    by_index: &HashMap<u32, &TypeDef>,
) -> io::Result<(usize, FieldValue, &'static str)> {
    if cursor + 18 > tail_cursor {
        return overrun("object list");
    }

    // The Python parser tries cursors {0, 1, 2, 3} bytes off and picks the
    // result whose decode reached the furthest. We do the same — header
    // shapes vary across game versions and a couple of bytes of padding
    // sometimes precede the real header.
    let mut best: Option<(usize, FieldValue, &'static str)> = None;
    let mut last_err: Option<io::Error> = None;

    for body_offset in 0..=3 {
        let body_cursor = cursor + body_offset;
        if body_cursor + 18 > tail_cursor {
            continue;
        }
        match try_decode_object_list_at(raw, cursor, body_cursor, tail_cursor, by_index) {
            Ok(result) => {
                if best.as_ref().is_none_or(|b| result.0 > b.0) {
                    best = Some(result);
                }
            }
            Err(e) => last_err = Some(e),
        }
    }
    best.ok_or_else(|| {
        last_err.unwrap_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "object list decode failed"))
    })
}

fn try_decode_object_list_at(
    raw: &[u8],
    field_cursor: usize,
    body_cursor: usize,
    tail_cursor: usize,
    by_index: &HashMap<u32, &TypeDef>,
) -> io::Result<(usize, FieldValue, &'static str)> {
    let prefix_u8 = raw[body_cursor];
    let mut marker_end = body_cursor;
    while marker_end < tail_cursor && raw[marker_end] == 1 {
        marker_end += 1;
    }

    let (count, header_size, header_variant): (u32, usize, &'static str);

    if marker_end > body_cursor
        && marker_end + 17 <= tail_cursor
        && raw[marker_end] == 0
        && raw[marker_end + 5..marker_end + 18] == [0u8; 13]
    {
        count = read_u32(raw, marker_end + 1);
        header_size = (marker_end - body_cursor + 1) + 4 + 13;
        header_variant = "marker_run_plus_zeros";
    } else if prefix_u8 == 0
        && raw[body_cursor + 1] == 0
        && raw[body_cursor + 2] == 0
        && raw[body_cursor + 3] == 0
    {
        count = read_u32(raw, body_cursor + 4);
        header_size = 18;
        header_variant = "zero4_count_u32";
    } else if prefix_u8 == 0 {
        count = read_u24(raw, body_cursor + 1);
        header_size = 18;
        header_variant = "zero1_count_u24";
    } else if prefix_u8 == 1
        && body_cursor + 21 <= tail_cursor
        && raw[body_cursor + 1] == 1
        && raw[body_cursor + 2] == 1
        && raw[body_cursor + 3] == 0
    {
        count = read_u32(raw, body_cursor + 4);
        header_size = 21;
        header_variant = "ones_then_count";
    } else if prefix_u8 == 1 {
        if body_cursor + 19 > tail_cursor {
            return overrun("object list 1-prefix");
        }
        count = read_u16be(raw, body_cursor + 1) as u32;
        header_size = 19;
        header_variant = "one_count_u16be";
    } else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported object list prefix 0x{prefix_u8:02X}"),
        ));
    }

    let mut element_cursor = body_cursor + header_size;
    let mut elements: Vec<ObjectBlock> = Vec::new();
    for _ in 0..count {
        match decode_object_list_element(raw, element_cursor, tail_cursor, by_index) {
            Ok((end, block)) => {
                elements.push(block);
                element_cursor = end;
            }
            Err(_) => break,
        }
    }

    let end = if !elements.is_empty() {
        element_cursor
    } else {
        // No elements decoded: end at the header.
        body_cursor + header_size
    };

    let value = FieldValue::ObjectList {
        count,
        header_variant,
        elements,
    };
    // `field_cursor` is the field's start offset; `end` minus that gives
    // the field length the caller can record.
    let _ = field_cursor;
    Ok((end, value, header_variant))
}

fn decode_object_list_element(
    raw: &[u8],
    cursor: usize,
    tail_cursor: usize,
    by_index: &HashMap<u32, &TypeDef>,
) -> io::Result<(usize, ObjectBlock)> {
    let (end, value) = decode_inline_object_locator(raw, cursor, tail_cursor, by_index, 4)?;
    match value {
        FieldValue::Locator {
            child: Some(b),
            child_type_index,
            child_type_name,
            ..
        } => {
            let mut block = *b;
            // Inline iff the child's bytes end exactly where the locator
            // call ended — meaning wrapper and child are contiguous. We
            // expand the element block to cover wrapper + child so a
            // single range describes the whole element's bytes.
            //
            // Non-inline: the child sits elsewhere in the body. Keep the
            // block's data range pointing at the payload only; the wrapper
            // bytes are claimed by the outer object_list field's
            // start..end range.
            let _ = (child_type_index, child_type_name);
            let inline = (block.data_offset as usize + block.data_size as usize) == end;
            if inline {
                block.data_offset = cursor as u32;
                block.data_size = (end - cursor) as u32;
            }
            Ok((end, block))
        }
        FieldValue::Locator {
            child: None,
            child_type_index,
            child_type_name,
            ..
        } => {
            // The wrapper was structurally valid but the child payload
            // decode failed (e.g. an unfamiliar dynamic_array header
            // variant inside the child). Return a placeholder block
            // covering just the wrapper bytes so the outer list loop
            // can continue with the next element rather than abandoning
            // the rest of the list.
            let block = ObjectBlock {
                class_index: child_type_index as u32,
                class_name: child_type_name,
                data_offset: cursor as u32,
                data_size: (end - cursor) as u32,
                mask_byte_count: 0,
                mask_bytes: Vec::new(),
                reserved_u32: 0,
                fields: Vec::new(),
                trailing_pad: None,
                undecoded_ranges: Vec::new(),
            };
            Ok((end, block))
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "list element decode produced unexpected value",
        )),
    }
}

// ── Tiny readers ───────────────────────────────────────────────────────────

#[inline]
fn read_u16(raw: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(raw[at..at + 2].try_into().unwrap())
}

#[inline]
fn read_u16be(raw: &[u8], at: usize) -> u16 {
    u16::from_be_bytes(raw[at..at + 2].try_into().unwrap())
}

#[inline]
fn read_u32(raw: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(raw[at..at + 4].try_into().unwrap())
}

#[inline]
fn read_u24(raw: &[u8], at: usize) -> u32 {
    raw[at] as u32 | ((raw[at + 1] as u32) << 8) | ((raw[at + 2] as u32) << 16)
}

#[inline]
fn overrun<T>(what: &'static str) -> io::Result<T> {
    Err(io::Error::new(
        io::ErrorKind::UnexpectedEof,
        format!("{what} overruns region"),
    ))
}

/// Reduce a [`ScalarValue`] to a JSON-friendly integer when possible.
/// Lossy for `Bytes` / floats, kept for cross-language consumers (Python)
/// that want a plain int comparison.
#[allow(dead_code)]
pub(crate) fn scalar_as_i128(v: &ScalarValue) -> Option<i128> {
    match v {
        ScalarValue::Bool(b) => Some(*b as i128),
        ScalarValue::U8(x) => Some(*x as i128),
        ScalarValue::U16(x) => Some(*x as i128),
        ScalarValue::U32(x) => Some(*x as i128),
        ScalarValue::U64(x) => Some(*x as i128),
        ScalarValue::I8(x) => Some(*x as i128),
        ScalarValue::I16(x) => Some(*x as i128),
        ScalarValue::I32(x) => Some(*x as i128),
        ScalarValue::I64(x) => Some(*x as i128),
        ScalarValue::F32(_) | ScalarValue::F64(_) | ScalarValue::Bytes(_) => None,
    }
}
