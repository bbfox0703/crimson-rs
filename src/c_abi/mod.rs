//! C ABI surface for native consumers.
//!
//! Built only when the `c_abi` Cargo feature is enabled. Exposes a
//! handle-based API: load a save once, then call zero-allocation getters
//! to read header / schema / TOC / per-block info. UTF-8 strings come out
//! through a "query size, then fill buffer" pattern (no callbacks, no
//! borrowed pointers held across calls).
//!
//! Layout mirrors the existing Rust API:
//!
//! - `crimson_save_load_from_file` parses the file with [`Save::parse`],
//!   then [`Body::parse`] and [`Body::decode_blocks`], and caches all
//!   three on the heap behind an opaque handle.
//! - Getters are pure reads into pre-existing fields — never re-parse.
//! - The handle is freed only by `crimson_save_free`. Aliasing or
//!   double-free is undefined behaviour, same as any C library.
//!
//! All entry points wrap their bodies in [`catch_unwind`] so a Rust
//! panic surfaces as [`error::PANIC`] instead of unwinding into C.

use std::ffi::CStr;
use std::fmt::Write;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::save::{Body, DecodedField, FieldKind, FieldValue, ObjectBlock, Save, SaveError, ScalarValue};

/// Stable error codes returned by every fallible C entry point.
///
/// Reserved space: `[-99, -1]`. New error categories add a new negative
/// constant; never reuse a number.
pub mod error {
    pub const OK: i32 = 0;
    pub const NULL_ARG: i32 = -1;
    pub const INVALID_PATH: i32 = -2;
    pub const IO: i32 = -3;
    pub const TOO_SMALL: i32 = -4;
    pub const BAD_MAGIC: i32 = -5;
    pub const UNSUPPORTED_VERSION: i32 = -6;
    pub const PAYLOAD_OUT_OF_RANGE: i32 = -7;
    pub const DECOMPRESS: i32 = -8;
    pub const BODY_PARSE: i32 = -9;
    pub const OUT_OF_RANGE: i32 = -10;
    pub const BUFFER_TOO_SMALL: i32 = -11;
    /// Field is not a fixed-size scalar (`fixed_prefix` / `fixed_suffix`).
    /// Returned by `crimson_save_set_scalar_field` when the caller targets
    /// a list / inline-bytes / locator / absent field.
    pub const NOT_SCALAR: i32 = -12;
    /// `bytes_len` doesn't match the field's recorded byte range.
    /// Length-changing edits are not supported by `set_scalar_field`.
    pub const LENGTH_MISMATCH: i32 = -13;
    /// `crimson_save_write_to_file` failed downstream of a successful
    /// re-serialize (filesystem error, permissions, etc.).
    pub const WRITE_FAILED: i32 = -14;
    pub const PANIC: i32 = -99;
}

/// Opaque handle handed out across the ABI boundary. C side only sees
/// `CrimsonSaveHandle*` and uses it as a token.
#[repr(C)]
pub struct CrimsonSaveHandle {
    save: Save,
    body: Body,
    blocks: Vec<ObjectBlock>,
}

/// Per-block summary returned by [`crimson_save_get_block_info`].
///
/// `fields_present`: count of fields whose presence bit was set in the
/// block's mask.
/// `fields_decoded`: subset of those that resolved to a concrete
/// [`FieldKind`] (excludes `Absent` and `Unknown`). When parsing a save
/// from a supported game version the two are expected to match.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct CrimsonBlockInfo {
    pub class_index: u32,
    pub data_offset: u32,
    pub data_size: u32,
    pub fields_present: u32,
    pub fields_decoded: u32,
}

// ── Load / free ────────────────────────────────────────────────────────────

/// Load and fully decode a `.save` file.
///
/// `path` must point to a NUL-terminated UTF-8 string. On success the
/// caller owns `*out_handle` and must release it via
/// [`crimson_save_free`].
///
/// # Safety
/// `path` must be a valid pointer to a NUL-terminated UTF-8 string for
/// the duration of the call. `out_handle` must be a writable pointer to
/// `*mut CrimsonSaveHandle`. On any error (`!= OK`) the function does
/// not write through `out_handle`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_load_from_file(
    path: *const c_char,
    out_handle: *mut *mut CrimsonSaveHandle,
) -> i32 {
    if path.is_null() || out_handle.is_null() {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let path_str = match unsafe { CStr::from_ptr(path) }.to_str() {
            Ok(s) => s,
            Err(_) => return error::INVALID_PATH,
        };

        let bytes = match std::fs::read(path_str) {
            Ok(b) => b,
            Err(_) => return error::IO,
        };

        let save = match Save::parse(&bytes) {
            Ok(s) => s,
            Err(e) => return save_error_code(&e),
        };
        let body = match Body::parse(&save.body) {
            Ok(b) => b,
            Err(_) => return error::BODY_PARSE,
        };
        let blocks = body.decode_blocks(&save.body);

        let boxed = Box::new(CrimsonSaveHandle { save, body, blocks });
        unsafe {
            *out_handle = Box::into_raw(boxed);
        }
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Free a handle returned by [`crimson_save_load_from_file`]. Passing a
/// NULL pointer is a no-op. Passing any other pointer is undefined
/// behaviour, exactly like `free()`.
///
/// # Safety
/// `handle` must either be NULL or a pointer previously returned by
/// [`crimson_save_load_from_file`] that has not already been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_free(handle: *mut CrimsonSaveHandle) {
    if handle.is_null() {
        return;
    }
    // catch_unwind because Drop on user-supplied data could in principle
    // panic; we still want the C side to see a normal return.
    let _ = catch_unwind(AssertUnwindSafe(|| {
        unsafe {
            drop(Box::from_raw(handle));
        }
    }));
}

// ── Scalar getters ─────────────────────────────────────────────────────────

macro_rules! scalar_getter {
    ($name:ident, $out_ty:ty, $expr:expr) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(
            handle: *const CrimsonSaveHandle,
            out: *mut $out_ty,
        ) -> i32 {
            with_handle(handle, out, |h, out| {
                let v: $out_ty = ($expr)(h);
                unsafe {
                    *out = v;
                }
                error::OK
            })
        }
    };
}

scalar_getter!(crimson_save_get_version, u16, |h: &CrimsonSaveHandle| h
    .save
    .header
    .version());
scalar_getter!(crimson_save_get_flags, u16, |h: &CrimsonSaveHandle| h
    .save
    .header
    .flags());
scalar_getter!(
    crimson_save_get_payload_size,
    u32,
    |h: &CrimsonSaveHandle| h.save.header.payload_size()
);
scalar_getter!(
    crimson_save_get_uncompressed_size,
    u32,
    |h: &CrimsonSaveHandle| h.save.header.uncompressed_size()
);
scalar_getter!(
    crimson_save_get_schema_type_count,
    u32,
    |h: &CrimsonSaveHandle| h.body.schema.type_count as u32
);
scalar_getter!(
    crimson_save_get_toc_entry_count,
    u32,
    |h: &CrimsonSaveHandle| h.body.toc.toc_count
);
scalar_getter!(crimson_save_get_block_count, u32, |h: &CrimsonSaveHandle| h
    .blocks
    .len() as u32);

/// HMAC verification result. Writes `1` if verified, `0` otherwise.
///
/// # Safety
/// `handle` must be a live handle. `out` must be a writable `*mut i32`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_get_hmac_ok(
    handle: *const CrimsonSaveHandle,
    out: *mut i32,
) -> i32 {
    with_handle(handle, out, |h, out| {
        unsafe {
            *out = if h.save.hmac_ok { 1 } else { 0 };
        }
        error::OK
    })
}

// ── Block info ─────────────────────────────────────────────────────────────

/// Fill `out` with the summary of the block at TOC `index`.
///
/// # Safety
/// `handle` must be a live handle. `out` must be a writable
/// `*mut CrimsonBlockInfo`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_get_block_info(
    handle: *const CrimsonSaveHandle,
    index: u32,
    out: *mut CrimsonBlockInfo,
) -> i32 {
    with_handle(handle, out, |h, out| {
        let Some(block) = h.blocks.get(index as usize) else {
            return error::OUT_OF_RANGE;
        };
        let (present, decoded) = count_fields(block);
        unsafe {
            *out = CrimsonBlockInfo {
                class_index: block.class_index,
                data_offset: block.data_offset,
                data_size: block.data_size,
                fields_present: present,
                fields_decoded: decoded,
            };
        }
        error::OK
    })
}

/// Write the class name of the block at TOC `index` into `buf` as a
/// NUL-terminated UTF-8 string.
///
/// `out_required` (optional, may be NULL) is always set to the buffer
/// size needed including the NUL terminator.
///
/// Return value:
/// - `OK` when the name fit and `buf` holds a valid C string.
/// - `BUFFER_TOO_SMALL` when the buffer is too small. Caller should
///   read `*out_required`, reallocate, and call again.
/// - other negative codes for invalid arguments / out-of-range index /
///   panic.
///
/// # Safety
/// `handle` must be a live handle. If `buf` is non-NULL it must be
/// writable for at least `buf_len` bytes. If `out_required` is non-NULL
/// it must be a writable `*mut usize`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_get_block_class_name(
    handle: *const CrimsonSaveHandle,
    index: u32,
    buf: *mut u8,
    buf_len: usize,
    out_required: *mut usize,
) -> i32 {
    if handle.is_null() {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some(block) = h.blocks.get(index as usize) else {
            return error::OUT_OF_RANGE;
        };
        let name = block.class_name.as_bytes();
        let required = name.len() + 1;
        if !out_required.is_null() {
            unsafe {
                *out_required = required;
            }
        }
        if buf.is_null() || buf_len < required {
            return error::BUFFER_TOO_SMALL;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(name.as_ptr(), buf, name.len());
            *buf.add(name.len()) = 0;
        }
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Serialize the full per-field decode of the block at TOC `index` as a
/// JSON document. Used by the UI to populate the field-detail pane.
///
/// Shape:
/// ```json
/// {
///   "class_index": u32,
///   "data_offset": u32,
///   "data_size":   u32,
///   "mask_bytes_hex":    "hex",   // empty when mask is empty
///   "trailing_pad_hex":  "hex",   // empty when no trailing pad
///   "fields": [
///     { "field_index", "name", "type_name",
///       "meta_kind", "meta_size", "meta_aux",
///       "present", "kind", "value", "start", "end", "note" }
///   ],
///   "undecoded_ranges": [[start, end], ...]
/// }
/// ```
///
/// `value` is a pre-formatted human string mirroring
/// `tools/inspect/inspect_save_section.py --pretty`. Empty for fields
/// whose `present` is false (or `kind` is `absent`).
///
/// Uses the standard two-call pattern: pass `buf=NULL, buf_len=0` to
/// learn the required size (including NUL), then allocate and call
/// again. `out_required` is always populated when non-NULL.
///
/// # Safety
/// `handle` must be a live handle. If `buf` is non-NULL it must be
/// writable for at least `buf_len` bytes. If `out_required` is non-NULL
/// it must be a writable `*mut usize`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_get_block_json(
    handle: *const CrimsonSaveHandle,
    index: u32,
    buf: *mut u8,
    buf_len: usize,
    out_required: *mut usize,
) -> i32 {
    if handle.is_null() {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some(block) = h.blocks.get(index as usize) else {
            return error::OUT_OF_RANGE;
        };
        let json = format_block_json(block);
        let bytes = json.as_bytes();
        let required = bytes.len() + 1;
        if !out_required.is_null() {
            unsafe {
                *out_required = required;
            }
        }
        if buf.is_null() || buf_len < required {
            return error::BUFFER_TOO_SMALL;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
            *buf.add(bytes.len()) = 0;
        }
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

// ── Mutation ───────────────────────────────────────────────────────────────

/// Overwrite the bytes of a fixed-size scalar field with `bytes`.
///
/// Constraints:
/// - `(block_idx, field_idx)` must resolve to a field whose
///   [`FieldKind`] is `FixedPrefix` or `FixedSuffix` — list, inline-byte,
///   locator, and absent fields are rejected with `NOT_SCALAR`. The
///   set is a same-size byte-level replacement.
/// - `bytes_len` must equal the field's recorded byte range
///   (`field.end - field.start`); otherwise `LENGTH_MISMATCH`.
///
/// On success the cached body bytes are patched in place and **every**
/// block is re-decoded so subsequent
/// [`crimson_save_get_block_json`] / `_info` calls see the new value.
/// The re-decode is O(toc_count) but completes in milliseconds on the
/// 1112-block save.
///
/// # Safety
/// `handle` must be a live, exclusive handle. `bytes` must be a valid
/// pointer to `bytes_len` readable bytes for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_set_scalar_field(
    handle: *mut CrimsonSaveHandle,
    block_idx: u32,
    field_idx: u32,
    bytes: *const u8,
    bytes_len: usize,
) -> i32 {
    if handle.is_null() {
        return error::NULL_ARG;
    }
    if bytes.is_null() && bytes_len != 0 {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &mut *handle };
        let Some(block) = h.blocks.get(block_idx as usize) else {
            return error::OUT_OF_RANGE;
        };
        let Some(field) = block.fields.get(field_idx as usize) else {
            return error::OUT_OF_RANGE;
        };
        if !matches!(field.kind, FieldKind::FixedPrefix | FieldKind::FixedSuffix) {
            return error::NOT_SCALAR;
        }
        let expected = field.end.saturating_sub(field.start);
        if bytes_len != expected {
            return error::LENGTH_MISMATCH;
        }
        let dst_start = field.start;
        let dst_end = field.end;
        if dst_end > h.save.body.len() {
            // Shouldn't happen — decoder produced offsets, body is the
            // same buffer it parsed. Defensive guard for the unsafe write.
            return error::OUT_OF_RANGE;
        }
        let src = unsafe { std::slice::from_raw_parts(bytes, bytes_len) };
        h.save.body[dst_start..dst_end].copy_from_slice(src);
        // Refresh decoded blocks so consumers see the new value on the
        // next get_block_json. Re-parsing the body is cheap (schema/TOC
        // unchanged); decode_blocks is the only meaningful work.
        h.blocks = h.body.decode_blocks(&h.save.body);
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Serialize the in-memory save back to `path` using the original nonce.
///
/// Uses `Save::write_with_nonce` so the on-disk layout matches what the
/// game produced — HMAC re-computed, ChaCha20 re-applied. The header's
/// `uncompressed_size` and `payload_size` get patched to match the
/// current body / freshly-compressed payload.
///
/// # Safety
/// `handle` must be a live handle. `path` must be a NUL-terminated
/// UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_write_to_file(
    handle: *const CrimsonSaveHandle,
    path: *const c_char,
) -> i32 {
    if handle.is_null() || path.is_null() {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let path_str = match unsafe { CStr::from_ptr(path) }.to_str() {
            Ok(s) => s,
            Err(_) => return error::INVALID_PATH,
        };
        let nonce = h.save.header.nonce();
        let bytes = match h.save.write_with_nonce(nonce) {
            Ok(b) => b,
            Err(e) => return save_error_code(&e),
        };
        match std::fs::write(path_str, &bytes) {
            Ok(()) => error::OK,
            Err(_) => error::WRITE_FAILED,
        }
    }))
    .unwrap_or(error::PANIC)
}

// ── Internal helpers ───────────────────────────────────────────────────────

fn with_handle<T, F>(handle: *const CrimsonSaveHandle, out: *mut T, body: F) -> i32
where
    F: FnOnce(&CrimsonSaveHandle, *mut T) -> i32,
{
    if handle.is_null() || out.is_null() {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        body(h, out)
    }))
    .unwrap_or(error::PANIC)
}

fn count_fields(block: &ObjectBlock) -> (u32, u32) {
    let mut present = 0u32;
    let mut decoded = 0u32;
    for f in &block.fields {
        if f.present {
            present += 1;
            if !matches!(f.kind, FieldKind::Absent | FieldKind::Unknown) {
                decoded += 1;
            }
        }
    }
    (present, decoded)
}

fn save_error_code(e: &SaveError) -> i32 {
    match e {
        SaveError::Io(_) => error::IO,
        SaveError::TooSmall { .. } => error::TOO_SMALL,
        SaveError::BadMagic(_) => error::BAD_MAGIC,
        SaveError::PayloadOutOfRange { .. } => error::PAYLOAD_OUT_OF_RANGE,
        SaveError::UnsupportedVersion(_) => error::UNSUPPORTED_VERSION,
        SaveError::DecompressSizeMismatch { .. } | SaveError::Lz4(_) => error::DECOMPRESS,
    }
}

// ── JSON serialization ─────────────────────────────────────────────────────
//
// Hand-rolled to avoid pulling serde + serde_json into the cdylib. The
// shape is small and stable; if it grows past a few dozen lines, swap to
// serde_json under a feature.

fn format_block_json(b: &ObjectBlock) -> String {
    let mut s = String::with_capacity(256 + b.fields.len() * 96);
    write_block_json(&mut s, b);
    s
}

fn write_block_json(s: &mut String, b: &ObjectBlock) {
    s.push('{');
    write!(s, "\"class_index\":{},", b.class_index).unwrap();
    s.push_str("\"class_name\":");
    write_json_string(s, &b.class_name);
    s.push(',');
    write!(s, "\"data_offset\":{},", b.data_offset).unwrap();
    write!(s, "\"data_size\":{},", b.data_size).unwrap();

    s.push_str("\"mask_bytes_hex\":");
    write_json_hex(s, &b.mask_bytes);
    s.push(',');

    s.push_str("\"trailing_pad_hex\":");
    write_json_hex(s, &b.trailing_pad);
    s.push(',');

    s.push_str("\"fields\":[");
    for (i, f) in b.fields.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        write_field_json(s, f);
    }
    s.push_str("],");

    s.push_str("\"undecoded_ranges\":[");
    for (i, (start, end)) in b.undecoded_ranges.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        write!(s, "[{start},{end}]").unwrap();
    }
    s.push(']');

    s.push('}');
}

fn write_field_json(s: &mut String, f: &DecodedField) {
    s.push('{');
    write!(s, "\"field_index\":{},", f.field_index).unwrap();

    s.push_str("\"name\":");
    write_json_string(s, &f.name);
    s.push(',');

    s.push_str("\"type_name\":");
    write_json_string(s, &f.type_name);
    s.push(',');

    write!(s, "\"meta_kind\":{},", f.meta_kind).unwrap();
    write!(s, "\"meta_size\":{},", f.meta_size).unwrap();
    write!(s, "\"meta_aux\":{},", f.meta_aux).unwrap();
    write!(s, "\"present\":{},", f.present).unwrap();

    s.push_str("\"kind\":");
    write_json_string(s, f.kind.as_str());
    s.push(',');

    s.push_str("\"value\":");
    let formatted = format_field_value(f);
    write_json_string(s, &formatted);
    s.push(',');

    write!(s, "\"start\":{},", f.start).unwrap();
    write!(s, "\"end\":{},", f.end).unwrap();

    s.push_str("\"note\":");
    write_json_string(s, &f.note);

    // Nested-block payload. `child` is populated when the field is an
    // inline object_locator with a resolvable inline child; `elements`
    // is populated for object_list fields (possibly empty). Both are
    // `null` otherwise so the C# side can rely on a stable shape.
    s.push_str(",\"child\":");
    match &f.value {
        FieldValue::Locator { child: Some(c), .. } => write_block_json(s, c),
        _ => s.push_str("null"),
    }

    s.push_str(",\"elements\":");
    match &f.value {
        FieldValue::ObjectList { elements, .. } => {
            s.push('[');
            for (i, el) in elements.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                write_block_json(s, el);
            }
            s.push(']');
        }
        _ => s.push_str("null"),
    }

    s.push('}');
}

/// Pre-formatted, human-readable value mirroring Python's
/// `format_field_value` in `inspect_save_section.py`. Empty when the
/// field is absent / unknown.
fn format_field_value(f: &DecodedField) -> String {
    match (&f.kind, &f.value) {
        (FieldKind::FixedPrefix | FieldKind::FixedSuffix, FieldValue::Scalar(v)) => {
            let (val, ty) = format_scalar(v);
            format!("{val} <{ty}>")
        }
        (FieldKind::InlineBytes, FieldValue::InlineBytes { count, bytes }) => {
            format!("<{count} items, {} bytes>", bytes.len())
        }
        (FieldKind::DynamicArray, FieldValue::DynamicArray { count, bytes, header_variant }) => {
            format!("<{count} items, {} bytes, {header_variant}>", bytes.len())
        }
        (FieldKind::ObjectLocator, FieldValue::Locator { child_type_name, child_payload_offset, child, .. }) => {
            match child {
                Some(c) => format!(
                    "-> {child_type_name} (offset {child_payload_offset}) inline -> {} fields",
                    c.fields.len()
                ),
                None => format!("-> {child_type_name} (offset {child_payload_offset})"),
            }
        }
        (FieldKind::ObjectList, FieldValue::ObjectList { count, header_variant, .. }) => {
            format!("[{count} elements, variant={header_variant}]")
        }
        (FieldKind::Absent, _) => "(absent)".to_string(),
        (FieldKind::Unknown, _) => "<unknown>".to_string(),
        _ => String::new(),
    }
}

fn format_scalar(v: &ScalarValue) -> (String, &'static str) {
    match v {
        ScalarValue::Bool(b) => (b.to_string(), "bool"),
        ScalarValue::U8(n)   => (n.to_string(), "u8"),
        ScalarValue::U16(n)  => (n.to_string(), "u16"),
        ScalarValue::U32(n)  => (n.to_string(), "u32"),
        ScalarValue::U64(n)  => (n.to_string(), "u64"),
        ScalarValue::I8(n)   => (n.to_string(), "i8"),
        ScalarValue::I16(n)  => (n.to_string(), "i16"),
        ScalarValue::I32(n)  => (n.to_string(), "i32"),
        ScalarValue::I64(n)  => (n.to_string(), "i64"),
        ScalarValue::F32(n)  => (format!("{n}"), "f32"),
        ScalarValue::F64(n)  => (format!("{n}"), "f64"),
        ScalarValue::Bytes(b) => (format!("{} bytes", b.len()), "bytes"),
    }
}

fn write_json_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"'  => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                write!(out, "\\u{:04x}", c as u32).unwrap();
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write_json_hex(out: &mut String, bytes: &[u8]) {
    out.push('"');
    for b in bytes {
        write!(out, "{b:02x}").unwrap();
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    //! End-to-end smoke test that drives the C ABI exactly as a native
    //! caller would: load → query → enumerate → free. Skips cleanly when
    //! no live save file is present (CI / fresh machines).

    use super::*;
    use std::ffi::CString;
    use std::path::PathBuf;
    use std::ptr;

    fn find_save() -> Option<PathBuf> {
        let local = std::env::var_os("LOCALAPPDATA")?;
        let root = PathBuf::from(local).join("Pearl Abyss/CD/save");
        for user in std::fs::read_dir(&root).ok()?.flatten() {
            for slot in ["slot0", "slot1", "slot2"] {
                let p = user.path().join(slot).join("save.save");
                if p.is_file() {
                    return Some(p);
                }
            }
        }
        None
    }

    #[test]
    fn c_abi_smoke() {
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_smoke: no live save under %LOCALAPPDATA%");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();

        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        let rc = unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) };
        assert_eq!(rc, error::OK, "load failed with code {rc}");
        assert!(!handle.is_null());

        let mut version: u16 = 0;
        assert_eq!(unsafe { crimson_save_get_version(handle, &mut version) }, error::OK);
        assert_eq!(version, 2, "expected save version 2");

        let mut hmac_ok: i32 = -1;
        assert_eq!(unsafe { crimson_save_get_hmac_ok(handle, &mut hmac_ok) }, error::OK);
        assert_eq!(hmac_ok, 1, "expected HMAC ok");

        let mut block_count: u32 = 0;
        assert_eq!(
            unsafe { crimson_save_get_block_count(handle, &mut block_count) },
            error::OK
        );
        assert!(block_count > 0, "expected at least one block");

        // First block: read info + class name (with the two-call pattern).
        let mut info = CrimsonBlockInfo::default();
        assert_eq!(
            unsafe { crimson_save_get_block_info(handle, 0, &mut info) },
            error::OK
        );
        assert_eq!(info.fields_present, info.fields_decoded);

        let mut needed: usize = 0;
        let rc = unsafe {
            crimson_save_get_block_class_name(handle, 0, ptr::null_mut(), 0, &mut needed)
        };
        assert_eq!(rc, error::BUFFER_TOO_SMALL);
        assert!(needed > 1);

        let mut buf = vec![0u8; needed];
        let rc = unsafe {
            crimson_save_get_block_class_name(
                handle,
                0,
                buf.as_mut_ptr(),
                buf.len(),
                ptr::null_mut(),
            )
        };
        assert_eq!(rc, error::OK);
        assert_eq!(*buf.last().unwrap(), 0, "expected NUL terminator");
        let name = std::str::from_utf8(&buf[..needed - 1]).unwrap();
        assert!(!name.is_empty());

        // Block JSON: query size, then read, sanity-check content.
        let mut needed: usize = 0;
        let rc = unsafe {
            crimson_save_get_block_json(handle, 0, ptr::null_mut(), 0, &mut needed)
        };
        assert_eq!(rc, error::BUFFER_TOO_SMALL);
        assert!(needed > 16, "expected non-trivial JSON, got {needed} bytes");
        let mut json_buf = vec![0u8; needed];
        let rc = unsafe {
            crimson_save_get_block_json(
                handle,
                0,
                json_buf.as_mut_ptr(),
                json_buf.len(),
                ptr::null_mut(),
            )
        };
        assert_eq!(rc, error::OK);
        assert_eq!(*json_buf.last().unwrap(), 0);
        let json = std::str::from_utf8(&json_buf[..needed - 1]).unwrap();
        assert!(json.starts_with('{') && json.ends_with('}'), "json shape: {json:.120}…");
        for needle in [
            "\"class_index\":",
            "\"class_name\":",
            "\"data_offset\":",
            "\"mask_bytes_hex\":",
            "\"trailing_pad_hex\":",
            "\"fields\":[",
            "\"undecoded_ranges\":[",
            "\"child\":",
            "\"elements\":",
        ] {
            assert!(json.contains(needle), "missing {needle:?} in {json:.200}…");
        }

        // Block JSON out-of-range -> proper error code.
        let rc = unsafe {
            crimson_save_get_block_json(handle, u32::MAX, ptr::null_mut(), 0, &mut needed)
        };
        assert_eq!(rc, error::OUT_OF_RANGE);

        // Out-of-range index on block_info returns the right code.
        let rc = unsafe { crimson_save_get_block_info(handle, u32::MAX, &mut info) };
        assert_eq!(rc, error::OUT_OF_RANGE);

        // NULL arg validation.
        let rc = unsafe { crimson_save_get_version(ptr::null(), &mut version) };
        assert_eq!(rc, error::NULL_ARG);

        unsafe { crimson_save_free(handle) };
    }

    /// Drives the new mutation API end-to-end against a live save:
    /// mutate block 0 field 0 (a fixed_suffix u32 in
    /// CharacterStatusSaveData), assert the in-memory JSON reflects the
    /// new value, write to a tempfile, reload that tempfile, and assert
    /// the value survived the encrypt → LZ4 → HMAC → decrypt round-trip.
    /// Then exercises every error path on the same handle.
    #[test]
    fn c_abi_mutate_and_write_roundtrip() {
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_mutate_and_write_roundtrip: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();

        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );

        // Confirm block 0 field 0 is the fixed_suffix u32 (_characterKey)
        // we're about to overwrite. If the schema ever drifts this assert
        // will tell us we picked the wrong target.
        let json = read_block_json(handle, 0);
        assert!(
            json.contains("\"field_index\":0") && json.contains("\"kind\":\"fixed_suffix\""),
            "expected block 0 field 0 to be fixed_suffix; got: {json:.200}…"
        );

        // 0x01EFCDAB = 32_492_971. Specific enough to be distinguishable
        // from any plausible original value.
        let sentinel: [u8; 4] = [0xAB, 0xCD, 0xEF, 0x01];
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_field(handle, 0, 0, sentinel.as_ptr(), sentinel.len())
            },
            error::OK
        );
        // After the set, get_block_json must reflect the new decoded value.
        let after = read_block_json(handle, 0);
        assert!(
            after.contains("\"value\":\"32492971 <u32>\""),
            "expected sentinel value in JSON after set; got: {after:.300}…"
        );

        // Write the modified handle to a temp file and reload it. The
        // tempfile crate's NamedTempFile deletes the file on drop, so we
        // keep `_tmp` alive until the reload completes.
        let _tmp = tempfile::NamedTempFile::new().unwrap();
        let tmp_path_str = _tmp.path().to_str().unwrap().to_owned();
        let tmp_path = CString::new(tmp_path_str.clone()).unwrap();
        assert_eq!(
            unsafe { crimson_save_write_to_file(handle, tmp_path.as_ptr()) },
            error::OK
        );

        unsafe { crimson_save_free(handle) };

        let mut reloaded: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(tmp_path.as_ptr(), &mut reloaded) },
            error::OK
        );
        let mut hmac: i32 = 0;
        assert_eq!(
            unsafe { crimson_save_get_hmac_ok(reloaded, &mut hmac) },
            error::OK
        );
        assert_eq!(hmac, 1, "HMAC must verify on reload of a write_to_file output");

        let reloaded_json = read_block_json(reloaded, 0);
        assert!(
            reloaded_json.contains("\"value\":\"32492971 <u32>\""),
            "mutation must persist across write+reload; got: {reloaded_json:.300}…"
        );
        unsafe { crimson_save_free(reloaded) };

        // ── Error paths ────────────────────────────────────────────────
        let mut h2: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut h2) },
            error::OK
        );
        let dummy = [0u8; 4];

        // NOT_SCALAR: target an absent field. Block 0 field 3 (_experience)
        // is absent in any low-level character.
        assert_eq!(
            unsafe { crimson_save_set_scalar_field(h2, 0, 3, dummy.as_ptr(), 0) },
            error::NOT_SCALAR
        );

        // LENGTH_MISMATCH: field 0 is 4 bytes; send 5.
        let dummy5 = [0u8; 5];
        assert_eq!(
            unsafe { crimson_save_set_scalar_field(h2, 0, 0, dummy5.as_ptr(), 5) },
            error::LENGTH_MISMATCH
        );

        // OUT_OF_RANGE on both axes.
        assert_eq!(
            unsafe { crimson_save_set_scalar_field(h2, 0, u32::MAX, dummy.as_ptr(), 4) },
            error::OUT_OF_RANGE
        );
        assert_eq!(
            unsafe { crimson_save_set_scalar_field(h2, u32::MAX, 0, dummy.as_ptr(), 4) },
            error::OUT_OF_RANGE
        );

        // NULL_ARG on handle and on bytes.
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_field(ptr::null_mut(), 0, 0, dummy.as_ptr(), 4)
            },
            error::NULL_ARG
        );
        assert_eq!(
            unsafe { crimson_save_set_scalar_field(h2, 0, 0, ptr::null(), 4) },
            error::NULL_ARG
        );

        // write_to_file with a NULL path is also NULL_ARG.
        assert_eq!(
            unsafe { crimson_save_write_to_file(h2, ptr::null()) },
            error::NULL_ARG
        );

        unsafe { crimson_save_free(h2) };
    }

    fn read_block_json(handle: *mut CrimsonSaveHandle, idx: u32) -> String {
        let mut needed: usize = 0;
        let _ = unsafe {
            crimson_save_get_block_json(handle, idx, ptr::null_mut(), 0, &mut needed)
        };
        let mut buf = vec![0u8; needed];
        let rc = unsafe {
            crimson_save_get_block_json(handle, idx, buf.as_mut_ptr(), buf.len(), ptr::null_mut())
        };
        assert_eq!(rc, error::OK);
        String::from_utf8(buf[..needed - 1].to_vec()).unwrap()
    }
}
