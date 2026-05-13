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

pub mod iteminfo;
pub mod paloc;
pub mod paz;
pub mod string_info;

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
    /// A mid-path navigation step in `crimson_save_set_scalar_field_path`
    /// targeted a field whose kind is not navigable (only `ObjectLocator`
    /// with a resolved child and `ObjectList` permit descent). Distinct
    /// from `NOT_SCALAR`, which only fires on the leaf.
    pub const NOT_NAVIGABLE: i32 = -15;
    /// `crimson_paloc_lookup` could not find the requested key in the
    /// loaded localization table. Distinct from `BUFFER_TOO_SMALL`
    /// (which means "key found, but caller's buffer is too small").
    pub const NOT_FOUND: i32 = -16;
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

/// One step of a descent path used by
/// [`crimson_save_set_scalar_field_path`].
///
/// Each step says: "from the current block, look up `field_idx`; if that
/// field is an `ObjectList`, descend into element `element_idx`; if it's
/// an `ObjectLocator` with a resolved child, descend into the child
/// (`element_idx` is ignored)". Anything else fails with `NOT_NAVIGABLE`.
///
/// The terminal field index (the scalar being written) is passed
/// separately from the path so the caller can address either a top-level
/// block field (empty path) or a deeply-nested one with the same API.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct CrimsonPathStep {
    pub field_idx: u32,
    pub element_idx: u32,
}

/// One element of a scalar batch mutation, passed to
/// [`crimson_save_set_scalar_fields_batch`].
///
/// Each op fully describes one scalar write: target block, descent path,
/// leaf field index, and replacement bytes. The path and byte buffers are
/// borrowed for the duration of the batch call only — the caller owns and
/// keeps them alive across the FFI boundary.
///
/// Layout matches the argument list of
/// [`crimson_save_set_scalar_field_path`] so the same caller-side
/// validation rules apply per-op.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CrimsonScalarBatchOp {
    pub block_idx: u32,
    pub field_idx: u32,
    pub path: *const CrimsonPathStep,
    pub path_len: usize,
    pub bytes: *const u8,
    pub bytes_len: usize,
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

/// Resolve `(block_idx, path, field_idx)` to the leaf scalar's byte range
/// in the body buffer, validating every navigation step.
///
/// On success returns `(start, end)` such that `start..end` is exactly the
/// slice to overwrite. Pure read of `blocks`; the caller does the write.
///
/// Shared between the single-op setters and the batch entry point so the
/// three surfaces stay in lockstep — same path semantics, same error
/// codes, same `bytes_len` invariant.
fn resolve_leaf_range(
    blocks: &[ObjectBlock],
    body_len: usize,
    block_idx: u32,
    path: &[CrimsonPathStep],
    field_idx: u32,
    bytes_len: usize,
) -> Result<(usize, usize), i32> {
    let Some(top) = blocks.get(block_idx as usize) else {
        return Err(error::OUT_OF_RANGE);
    };
    let mut current: &ObjectBlock = top;
    for step in path {
        let Some(field) = current.fields.get(step.field_idx as usize) else {
            return Err(error::OUT_OF_RANGE);
        };
        current = match &field.value {
            FieldValue::Locator {
                child: Some(child), ..
            } => child.as_ref(),
            FieldValue::ObjectList { elements, .. } => {
                let Some(el) = elements.get(step.element_idx as usize) else {
                    return Err(error::OUT_OF_RANGE);
                };
                el
            }
            _ => return Err(error::NOT_NAVIGABLE),
        };
    }
    let Some(leaf) = current.fields.get(field_idx as usize) else {
        return Err(error::OUT_OF_RANGE);
    };
    if !matches!(leaf.kind, FieldKind::FixedPrefix | FieldKind::FixedSuffix) {
        return Err(error::NOT_SCALAR);
    }
    let expected = leaf.end.saturating_sub(leaf.start);
    if bytes_len != expected {
        return Err(error::LENGTH_MISMATCH);
    }
    if leaf.end > body_len {
        // Defensive guard: decoder produced offsets into the same body
        // buffer it parsed, so leaf.end > body_len should be unreachable.
        return Err(error::OUT_OF_RANGE);
    }
    Ok((leaf.start, leaf.end))
}

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
        let (dst_start, dst_end) =
            match resolve_leaf_range(&h.blocks, h.save.body.len(), block_idx, &[], field_idx, bytes_len) {
                Ok(range) => range,
                Err(code) => return code,
            };
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

/// Path-addressed variant of [`crimson_save_set_scalar_field`].
///
/// Mutates a fixed-size scalar field reachable through a chain of inline
/// children and list elements. The `(block_idx, path[], field_idx)`
/// triple uniquely identifies any decoded scalar in the save tree:
///
/// - `block_idx` picks a top-level TOC block.
/// - `path` is a sequence of [`CrimsonPathStep`] descents from that
///   block. Each step's `field_idx` selects a nested-bearing field of
///   the current block (must resolve to `ObjectLocator` with an inline
///   child, or `ObjectList`). For lists, `element_idx` picks the element.
/// - `field_idx` (the leaf) is the scalar to write inside the block we
///   arrive at after `path_len` descents. With `path_len == 0` this
///   behaves identically to [`crimson_save_set_scalar_field`].
///
/// All other invariants match the top-level setter:
/// - leaf must be `FixedPrefix` / `FixedSuffix` → otherwise `NOT_SCALAR`
/// - `bytes_len` must equal the leaf's recorded byte range → otherwise
///   `LENGTH_MISMATCH`
/// - on success the body is patched in place and every block re-decoded
///
/// Errors:
/// - `OUT_OF_RANGE` for any bad index along the path or at the leaf
/// - `NOT_NAVIGABLE` when a mid-path field isn't a locator-with-child
///   or a list (e.g. a scalar in the middle of the chain)
/// - `NULL_ARG` on null `handle`, null `bytes` with non-zero length, or
///   null `path` with non-zero `path_len`
///
/// # Safety
/// `handle` must be a live, exclusive handle. `path` must point to
/// `path_len` readable [`CrimsonPathStep`] values, and `bytes` to
/// `bytes_len` readable bytes, both for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_set_scalar_field_path(
    handle: *mut CrimsonSaveHandle,
    block_idx: u32,
    path: *const CrimsonPathStep,
    path_len: usize,
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
    if path.is_null() && path_len != 0 {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &mut *handle };
        let steps: &[CrimsonPathStep] = if path_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(path, path_len) }
        };
        let (dst_start, dst_end) =
            match resolve_leaf_range(&h.blocks, h.save.body.len(), block_idx, steps, field_idx, bytes_len) {
                Ok(range) => range,
                Err(code) => return code,
            };
        let src = unsafe { std::slice::from_raw_parts(bytes, bytes_len) };
        h.save.body[dst_start..dst_end].copy_from_slice(src);
        h.blocks = h.body.decode_blocks(&h.save.body);
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Apply many scalar mutations in one FFI round trip, sharing a single
/// post-batch re-decode.
///
/// Semantics:
///
/// 1. **Validate everything first.** Each op's `(block_idx, path[],
///    field_idx, bytes_len)` is resolved through the same rules as
///    [`crimson_save_set_scalar_field_path`] (`NOT_SCALAR`,
///    `LENGTH_MISMATCH`, `OUT_OF_RANGE`, `NOT_NAVIGABLE`). On any error
///    the call returns immediately with no mutation applied — the save
///    body is left exactly as it was before the call.
/// 2. **Patch all ops in order.** Identical to running N
///    `crimson_save_set_scalar_field_path` calls sequentially, except no
///    re-decode happens between ops.
/// 3. **One re-decode at the end.** The decoded `blocks` are refreshed
///    a single time after the last patch, regardless of `op_count`.
///
/// This is the high-throughput path for bulk edits — e.g. "fill every
/// item's stack to max", which performs 168 writes today and would
/// otherwise pay 168 × `decode_blocks` cost. Wall-time goes from
/// O(N · block_count) to O(N + block_count).
///
/// `out_failed_op_index` (optional, may be NULL):
/// - On error, written with the index of the op whose validation failed
///   so the caller can pinpoint the offending mutation.
/// - On success, written with `usize::MAX` as a sentinel meaning
///   "no failure". Callers can either ignore the value on `OK` or check
///   it explicitly.
///
/// # Safety
/// `handle` must be a live, exclusive handle. `ops` must point to
/// `op_count` readable [`CrimsonScalarBatchOp`] values for the duration
/// of the call. Each op's `path` and `bytes` pointers must point to
/// `path_len` / `bytes_len` readable bytes respectively, for the
/// duration of the call. `out_failed_op_index` may be NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_set_scalar_fields_batch(
    handle: *mut CrimsonSaveHandle,
    ops: *const CrimsonScalarBatchOp,
    op_count: usize,
    out_failed_op_index: *mut usize,
) -> i32 {
    if handle.is_null() {
        return error::NULL_ARG;
    }
    if ops.is_null() && op_count != 0 {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        // Trivial batch: no work, no re-decode, no failure index to set.
        if op_count == 0 {
            if !out_failed_op_index.is_null() {
                unsafe {
                    *out_failed_op_index = usize::MAX;
                }
            }
            return error::OK;
        }

        let h = unsafe { &mut *handle };
        let ops_slice: &[CrimsonScalarBatchOp] =
            unsafe { std::slice::from_raw_parts(ops, op_count) };

        // Per-op NULL_ARG pre-check (path / bytes pointers). Mirrors
        // the single-op setter's invariants without rolling them into
        // resolve_leaf_range, which works on a typed slice.
        for (i, op) in ops_slice.iter().enumerate() {
            if op.path.is_null() && op.path_len != 0 {
                if !out_failed_op_index.is_null() {
                    unsafe {
                        *out_failed_op_index = i;
                    }
                }
                return error::NULL_ARG;
            }
            if op.bytes.is_null() && op.bytes_len != 0 {
                if !out_failed_op_index.is_null() {
                    unsafe {
                        *out_failed_op_index = i;
                    }
                }
                return error::NULL_ARG;
            }
        }

        // Phase 1 — validate every op against the (immutable) decoded
        // tree. Collect each op's resolved byte range. On the first
        // failure we abort without writing anything.
        let mut ranges: Vec<(usize, usize)> = Vec::with_capacity(op_count);
        for (i, op) in ops_slice.iter().enumerate() {
            let steps: &[CrimsonPathStep] = if op.path_len == 0 {
                &[]
            } else {
                unsafe { std::slice::from_raw_parts(op.path, op.path_len) }
            };
            match resolve_leaf_range(
                &h.blocks,
                h.save.body.len(),
                op.block_idx,
                steps,
                op.field_idx,
                op.bytes_len,
            ) {
                Ok(range) => ranges.push(range),
                Err(code) => {
                    if !out_failed_op_index.is_null() {
                        unsafe {
                            *out_failed_op_index = i;
                        }
                    }
                    return code;
                }
            }
        }

        // Phase 2 — apply every patch in input order. Pure memcpy over
        // already-validated ranges; no validation errors possible here.
        // Note: overlapping ranges between ops are not detected. Last
        // write wins, exactly as if the caller had run N sequential
        // single-op setters.
        for (op, (dst_start, dst_end)) in ops_slice.iter().zip(ranges) {
            let src = unsafe { std::slice::from_raw_parts(op.bytes, op.bytes_len) };
            h.save.body[dst_start..dst_end].copy_from_slice(src);
        }

        // Phase 3 — one re-decode covers all mutations.
        h.blocks = h.body.decode_blocks(&h.save.body);

        if !out_failed_op_index.is_null() {
            unsafe {
                *out_failed_op_index = usize::MAX;
            }
        }
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

    /// Locate a (block, path, leaf, current_value, scalar_byte_len) tuple
    /// pointing at a u32-shaped scalar reachable via a one-step descent
    /// (either through a Locator's inline child or the first element of
    /// an ObjectList). Returns `None` if nothing in the save matches —
    /// the live-save assertion below handles that case explicitly.
    fn find_nested_u32_scalar(
        handle: *mut CrimsonSaveHandle,
    ) -> Option<(u32, CrimsonPathStep, u32, u32, usize)> {
        let h = unsafe { &*handle };
        for (block_idx, block) in h.blocks.iter().enumerate() {
            for (parent_field_idx, parent_field) in block.fields.iter().enumerate() {
                // Inline locator → walk the child.
                if let FieldValue::Locator {
                    child: Some(child), ..
                } = &parent_field.value
                    && let Some(leaf) = pick_scalar_field(child)
                {
                    return Some((
                        block_idx as u32,
                        CrimsonPathStep {
                            field_idx: parent_field_idx as u32,
                            element_idx: 0,
                        },
                        leaf.0,
                        leaf.1,
                        leaf.2,
                    ));
                }
                // ObjectList → walk the first element.
                if let FieldValue::ObjectList { elements, .. } = &parent_field.value
                    && let Some(first) = elements.first()
                    && let Some(leaf) = pick_scalar_field(first)
                {
                    return Some((
                        block_idx as u32,
                        CrimsonPathStep {
                            field_idx: parent_field_idx as u32,
                            element_idx: 0,
                        },
                        leaf.0,
                        leaf.1,
                        leaf.2,
                    ));
                }
            }
        }
        None
    }

    /// Pick a u32-shaped FixedPrefix/FixedSuffix field from `block`.
    /// Returns (field_idx, current_u32_value, byte_len). Restricting to
    /// u32 keeps the test independent of which exact field we land on —
    /// we just need predictable bit width for the sentinel.
    fn pick_scalar_field(block: &ObjectBlock) -> Option<(u32, u32, usize)> {
        for (idx, f) in block.fields.iter().enumerate() {
            if !matches!(f.kind, FieldKind::FixedPrefix | FieldKind::FixedSuffix) {
                continue;
            }
            let len = f.end.saturating_sub(f.start);
            if len != 4 {
                continue;
            }
            if let FieldValue::Scalar(ScalarValue::U32(v)) = &f.value {
                return Some((idx as u32, *v, len));
            }
        }
        None
    }

    #[test]
    fn c_abi_set_scalar_field_path() {
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_set_scalar_field_path: no live save under %LOCALAPPDATA%");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();

        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );

        // ── Empty-path parity: path_len=0 must behave identically to
        // the top-level setter. Block 0 field 0 is the _characterKey
        // fixed_suffix u32 the existing tests already exercise.
        let sentinel: [u8; 4] = [0xAB, 0xCD, 0xEF, 0x01];
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_field_path(
                    handle,
                    0,
                    ptr::null(),
                    0,
                    0,
                    sentinel.as_ptr(),
                    sentinel.len(),
                )
            },
            error::OK
        );
        let after_top = read_block_json(handle, 0);
        assert!(
            after_top.contains("\"value\":\"32492971 <u32>\""),
            "empty-path mutation must equal the top-level setter; got: {after_top:.300}…"
        );

        // ── Find a nested scalar and mutate it.
        let Some((block_idx, step, leaf_idx, original, len)) = find_nested_u32_scalar(handle)
        else {
            // Any non-trivial save has at least one InventorySaveData /
            // EquipmentSaveData with nested scalars. If we hit this we
            // want to know — fail loudly rather than silently skip.
            unsafe { crimson_save_free(handle) };
            panic!("expected a nested u32 scalar in a live save; schema or fixture drifted");
        };
        assert_eq!(len, 4, "find_nested_u32_scalar contract");

        // Pick a sentinel guaranteed to differ from the original value.
        let nested_sentinel: u32 = original.wrapping_add(0x0BAD_F00D);
        let nested_bytes = nested_sentinel.to_le_bytes();
        let steps = [step];
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_field_path(
                    handle,
                    block_idx,
                    steps.as_ptr(),
                    steps.len(),
                    leaf_idx,
                    nested_bytes.as_ptr(),
                    nested_bytes.len(),
                )
            },
            error::OK,
            "nested-path mutation must succeed (block={block_idx}, parent_field={}, leaf={leaf_idx}, original=0x{:08x})",
            step.field_idx, original
        );

        // The mutation must round-trip through write_to_file + reload —
        // i.e. survive HMAC / ChaCha20 / LZ4 re-emission.
        let _tmp = tempfile::NamedTempFile::new().unwrap();
        let tmp_path_str = _tmp.path().to_str().unwrap().to_owned();
        let tmp_path = CString::new(tmp_path_str).unwrap();
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
        assert_eq!(hmac, 1, "HMAC must verify on reload after nested-path mutation");

        // Confirm the reloaded copy carries the nested sentinel where
        // we left it. Re-discover the path so this test stays robust to
        // any schema drift (we relocate the same logical position from
        // the in-memory tree, not from cached indices).
        let reloaded_target = find_nested_u32_scalar(reloaded);
        unsafe { crimson_save_free(reloaded) };
        let (_, _, _, post_value, _) = reloaded_target.expect("nested scalar still findable post-reload");
        assert_eq!(
            post_value, nested_sentinel,
            "reloaded save must carry the nested sentinel"
        );

        // ── Error paths ────────────────────────────────────────────────
        let mut h2: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut h2) },
            error::OK
        );
        let dummy = [0u8; 4];

        // NOT_NAVIGABLE: target block 0 field 0 (a scalar) as a mid-path
        // step. Walking *into* a scalar is the canonical "not navigable"
        // failure.
        let bad_steps = [CrimsonPathStep {
            field_idx: 0,
            element_idx: 0,
        }];
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_field_path(
                    h2,
                    0,
                    bad_steps.as_ptr(),
                    bad_steps.len(),
                    0,
                    dummy.as_ptr(),
                    dummy.len(),
                )
            },
            error::NOT_NAVIGABLE
        );

        // OUT_OF_RANGE on a path step's field_idx.
        let oor_step = [CrimsonPathStep {
            field_idx: u32::MAX,
            element_idx: 0,
        }];
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_field_path(
                    h2,
                    0,
                    oor_step.as_ptr(),
                    oor_step.len(),
                    0,
                    dummy.as_ptr(),
                    dummy.len(),
                )
            },
            error::OUT_OF_RANGE
        );

        // NULL_ARG: null handle.
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_field_path(
                    ptr::null_mut(),
                    0,
                    ptr::null(),
                    0,
                    0,
                    dummy.as_ptr(),
                    dummy.len(),
                )
            },
            error::NULL_ARG
        );

        // NULL_ARG: null path with non-zero path_len.
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_field_path(
                    h2,
                    0,
                    ptr::null(),
                    1,
                    0,
                    dummy.as_ptr(),
                    dummy.len(),
                )
            },
            error::NULL_ARG
        );

        // NULL_ARG: null bytes with non-zero bytes_len.
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_field_path(
                    h2,
                    0,
                    ptr::null(),
                    0,
                    0,
                    ptr::null(),
                    4,
                )
            },
            error::NULL_ARG
        );

        unsafe { crimson_save_free(h2) };
    }

    /// Collect every top-level u32 FixedPrefix/FixedSuffix scalar in the
    /// save, paired with its current value. Returned as
    /// `(block_idx, field_idx, current_u32)`. Used by the batch tests to
    /// build many-op fixtures without inventing schema knowledge.
    fn collect_top_level_u32_scalars(
        handle: *mut CrimsonSaveHandle,
    ) -> Vec<(u32, u32, u32)> {
        let h = unsafe { &*handle };
        let mut out = Vec::new();
        for (block_idx, block) in h.blocks.iter().enumerate() {
            for (field_idx, f) in block.fields.iter().enumerate() {
                if !matches!(f.kind, FieldKind::FixedPrefix | FieldKind::FixedSuffix) {
                    continue;
                }
                let len = f.end.saturating_sub(f.start);
                if len != 4 {
                    continue;
                }
                if let FieldValue::Scalar(ScalarValue::U32(v)) = &f.value {
                    out.push((block_idx as u32, field_idx as u32, *v));
                }
            }
        }
        out
    }

    /// Drives the batch entry point with a mix of top-level + one nested
    /// op against a live save: apply, verify each value reflected in the
    /// decoded JSON, write to a tempfile, reload, and confirm every
    /// sentinel survived HMAC / ChaCha20 / LZ4 re-emission.
    #[test]
    fn c_abi_set_scalar_fields_batch_smoke() {
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_set_scalar_fields_batch_smoke: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();

        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );

        // Grab 5 top-level u32 scalars + 1 nested op. The nested op
        // exercises the path-traversal branch alongside the empty-path
        // ones so the batch covers both code paths in resolve_leaf_range.
        let mut top = collect_top_level_u32_scalars(handle);
        assert!(
            top.len() >= 5,
            "expected ≥5 top-level u32 scalars in live save; got {}",
            top.len()
        );
        top.truncate(5);
        let nested = find_nested_u32_scalar(handle)
            .expect("expected a nested u32 scalar in live save");

        // Build a stable mapping (block, path[], field) → sentinel bytes
        // that owns its buffers across the batch call. The ops slice
        // borrows from this storage.
        struct OpFixture {
            block_idx: u32,
            field_idx: u32,
            steps: Vec<CrimsonPathStep>,
            bytes: [u8; 4],
            expected_decoded: u32,
        }
        let mut fixtures: Vec<OpFixture> = Vec::new();
        for (i, (b, f, original)) in top.iter().enumerate() {
            let sentinel = original.wrapping_add(0x5EED_0000 + i as u32);
            fixtures.push(OpFixture {
                block_idx: *b,
                field_idx: *f,
                steps: Vec::new(),
                bytes: sentinel.to_le_bytes(),
                expected_decoded: sentinel,
            });
        }
        let (nb, nstep, nleaf, noriginal, _) = nested;
        let nested_sentinel = noriginal.wrapping_add(0xBADD_F00D);
        fixtures.push(OpFixture {
            block_idx: nb,
            field_idx: nleaf,
            steps: vec![nstep],
            bytes: nested_sentinel.to_le_bytes(),
            expected_decoded: nested_sentinel,
        });

        let ops: Vec<CrimsonScalarBatchOp> = fixtures
            .iter()
            .map(|fx| CrimsonScalarBatchOp {
                block_idx: fx.block_idx,
                field_idx: fx.field_idx,
                path: if fx.steps.is_empty() {
                    ptr::null()
                } else {
                    fx.steps.as_ptr()
                },
                path_len: fx.steps.len(),
                bytes: fx.bytes.as_ptr(),
                bytes_len: fx.bytes.len(),
            })
            .collect();

        let mut failed_idx: usize = 0;
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_fields_batch(
                    handle,
                    ops.as_ptr(),
                    ops.len(),
                    &mut failed_idx,
                )
            },
            error::OK
        );
        assert_eq!(
            failed_idx,
            usize::MAX,
            "OK return must write usize::MAX sentinel to out_failed_op_index"
        );

        // Every top-level op must be reflected in the decoded JSON.
        for fx in fixtures.iter().filter(|fx| fx.steps.is_empty()) {
            let json = read_block_json(handle, fx.block_idx);
            let needle = format!("\"value\":\"{} <u32>\"", fx.expected_decoded);
            assert!(
                json.contains(&needle),
                "top-level batch op (block={}, field={}) not visible in JSON: {json:.300}…",
                fx.block_idx,
                fx.field_idx
            );
        }

        // Round-trip via write_to_file + reload — every sentinel must
        // survive HMAC / ChaCha20 / LZ4 re-emission.
        let _tmp = tempfile::NamedTempFile::new().unwrap();
        let tmp_path = CString::new(_tmp.path().to_str().unwrap()).unwrap();
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
        assert_eq!(hmac, 1, "HMAC must verify on reload after batch mutation");

        for fx in fixtures.iter().filter(|fx| fx.steps.is_empty()) {
            let json = read_block_json(reloaded, fx.block_idx);
            let needle = format!("\"value\":\"{} <u32>\"", fx.expected_decoded);
            assert!(
                json.contains(&needle),
                "reloaded JSON missing sentinel for block={}, field={}: {json:.300}…",
                fx.block_idx,
                fx.field_idx
            );
        }

        // Empty-batch: zero ops returns OK + writes the sentinel.
        let mut sentinel_slot: usize = 0;
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_fields_batch(
                    reloaded,
                    ptr::null(),
                    0,
                    &mut sentinel_slot,
                )
            },
            error::OK
        );
        assert_eq!(sentinel_slot, usize::MAX);

        unsafe { crimson_save_free(reloaded) };
    }

    /// Validate the all-or-nothing contract: if any op in the batch fails
    /// validation, the body must be left exactly as it was on entry, and
    /// `out_failed_op_index` must point at the offending op.
    #[test]
    fn c_abi_set_scalar_fields_batch_atomicity() {
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_set_scalar_fields_batch_atomicity: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();

        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );

        // Snapshot the entire body so we can compare bytes after the
        // failed batch and prove zero mutation happened.
        let body_before: Vec<u8> = {
            let h = unsafe { &*handle };
            h.save.body.clone()
        };

        let scalars = collect_top_level_u32_scalars(handle);
        assert!(
            scalars.len() >= 2,
            "expected ≥2 top-level u32 scalars for atomicity test"
        );
        let (b0, f0, v0) = scalars[0];
        let (b1, f1, _) = scalars[1];

        // Op 0: valid mutation. Op 1: same target but with bytes_len=3
        // — guaranteed LENGTH_MISMATCH because the field is u32 (4 bytes).
        let sentinel0 = v0.wrapping_add(0xDEAD_BEEF).to_le_bytes();
        let bad_bytes: [u8; 3] = [0, 0, 0];

        let ops = [
            CrimsonScalarBatchOp {
                block_idx: b0,
                field_idx: f0,
                path: ptr::null(),
                path_len: 0,
                bytes: sentinel0.as_ptr(),
                bytes_len: sentinel0.len(),
            },
            CrimsonScalarBatchOp {
                block_idx: b1,
                field_idx: f1,
                path: ptr::null(),
                path_len: 0,
                bytes: bad_bytes.as_ptr(),
                bytes_len: bad_bytes.len(),
            },
        ];

        let mut failed_idx: usize = 0;
        let rc = unsafe {
            crimson_save_set_scalar_fields_batch(
                handle,
                ops.as_ptr(),
                ops.len(),
                &mut failed_idx,
            )
        };
        assert_eq!(rc, error::LENGTH_MISMATCH);
        assert_eq!(failed_idx, 1, "failed_op_index must pinpoint the offending op");

        // The body must be byte-identical to entry — op 0 must NOT have
        // been applied just because validation reached it first.
        let body_after: &[u8] = unsafe { &(*handle).save.body };
        assert_eq!(
            body_after.len(),
            body_before.len(),
            "body length must not change on failed batch"
        );
        assert!(
            body_after == body_before.as_slice(),
            "body must be byte-identical to pre-batch on validation failure"
        );

        // Different error: NOT_NAVIGABLE via path that walks into a scalar.
        let bad_step = [CrimsonPathStep {
            field_idx: f0,
            element_idx: 0,
        }];
        let dummy: [u8; 4] = [0, 0, 0, 0];
        let ops2 = [CrimsonScalarBatchOp {
            block_idx: b0,
            field_idx: 0,
            path: bad_step.as_ptr(),
            path_len: bad_step.len(),
            bytes: dummy.as_ptr(),
            bytes_len: dummy.len(),
        }];
        let mut failed_idx2: usize = 0;
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_fields_batch(
                    handle,
                    ops2.as_ptr(),
                    ops2.len(),
                    &mut failed_idx2,
                )
            },
            error::NOT_NAVIGABLE
        );
        assert_eq!(failed_idx2, 0);

        // NULL_ARG: per-op null bytes with non-zero len must surface
        // through out_failed_op_index at the right index.
        let good_bytes = sentinel0;
        let ops3 = [
            CrimsonScalarBatchOp {
                block_idx: b0,
                field_idx: f0,
                path: ptr::null(),
                path_len: 0,
                bytes: good_bytes.as_ptr(),
                bytes_len: good_bytes.len(),
            },
            CrimsonScalarBatchOp {
                block_idx: b1,
                field_idx: f1,
                path: ptr::null(),
                path_len: 0,
                bytes: ptr::null(),
                bytes_len: 4,
            },
        ];
        let mut failed_idx3: usize = 0;
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_fields_batch(
                    handle,
                    ops3.as_ptr(),
                    ops3.len(),
                    &mut failed_idx3,
                )
            },
            error::NULL_ARG
        );
        assert_eq!(failed_idx3, 1);

        // NULL handle: top-level NULL_ARG; out_failed_op_index is not
        // touched (handle is null so we can't write back through it
        // safely, and the per-op pre-check never runs).
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_fields_batch(
                    ptr::null_mut(),
                    ops3.as_ptr(),
                    ops3.len(),
                    ptr::null_mut(),
                )
            },
            error::NULL_ARG
        );

        unsafe { crimson_save_free(handle) };
    }

    /// Equivalence check at scale: applying N ops through the batch
    /// entry point produces the byte-identical body that running the same
    /// N ops one-at-a-time through the single-op setter would. This is
    /// the soundness invariant the perf optimization rests on.
    #[test]
    fn c_abi_set_scalar_fields_batch_matches_single_op() {
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_set_scalar_fields_batch_matches_single_op: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();

        let mut handle_batch: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle_batch) },
            error::OK
        );
        let mut handle_single: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle_single) },
            error::OK
        );

        // Collect candidates from the batch handle; both handles parsed
        // the same file so positions are identical. Cap at 200 to keep
        // the test fast — 200 ops is already 5× the fill-stacks use
        // case and is plenty to exercise the batch path.
        let mut scalars = collect_top_level_u32_scalars(handle_batch);
        assert!(
            scalars.len() >= 50,
            "expected ≥50 top-level u32 scalars in live save; got {}",
            scalars.len()
        );
        scalars.truncate(200);
        let n = scalars.len();

        // Build (sentinel_bytes_owned, op) pairs. Each sentinel is a
        // deterministic mutation of the original so two ops on the same
        // target wouldn't accidentally land on the same byte pattern.
        let sentinels: Vec<[u8; 4]> = scalars
            .iter()
            .enumerate()
            .map(|(i, (_, _, v))| v.wrapping_add(0xC0DE_0000 + i as u32).to_le_bytes())
            .collect();
        let ops: Vec<CrimsonScalarBatchOp> = scalars
            .iter()
            .zip(sentinels.iter())
            .map(|((b, f, _), bytes)| CrimsonScalarBatchOp {
                block_idx: *b,
                field_idx: *f,
                path: ptr::null(),
                path_len: 0,
                bytes: bytes.as_ptr(),
                bytes_len: bytes.len(),
            })
            .collect();

        // Apply via batch on handle A.
        let mut failed_idx: usize = 0;
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_fields_batch(
                    handle_batch,
                    ops.as_ptr(),
                    ops.len(),
                    &mut failed_idx,
                )
            },
            error::OK,
            "batch of {n} ops should apply cleanly"
        );

        // Apply the same N ops one-at-a-time on handle B via the
        // pre-existing single-op setter.
        for ((b, f, _), bytes) in scalars.iter().zip(sentinels.iter()) {
            assert_eq!(
                unsafe {
                    crimson_save_set_scalar_field(
                        handle_single,
                        *b,
                        *f,
                        bytes.as_ptr(),
                        bytes.len(),
                    )
                },
                error::OK
            );
        }

        // Both bodies must be byte-identical. This is the equivalence
        // proof: batch == N × single-op, with one re-decode instead of N.
        let body_batch: &[u8] = unsafe { &(*handle_batch).save.body };
        let body_single: &[u8] = unsafe { &(*handle_single).save.body };
        assert_eq!(
            body_batch.len(),
            body_single.len(),
            "body lengths must match between batch and single-op handles"
        );
        assert!(
            body_batch == body_single,
            "batch body must be byte-identical to N × single-op body"
        );

        unsafe { crimson_save_free(handle_batch) };
        unsafe { crimson_save_free(handle_single) };
    }
}
