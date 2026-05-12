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
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::save::{Body, FieldKind, ObjectBlock, Save, SaveError};

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

        // Out-of-range index returns the right code.
        let rc = unsafe { crimson_save_get_block_info(handle, u32::MAX, &mut info) };
        assert_eq!(rc, error::OUT_OF_RANGE);

        // NULL arg validation.
        let rc = unsafe { crimson_save_get_version(ptr::null(), &mut version) };
        assert_eq!(rc, error::NULL_ARG);

        unsafe { crimson_save_free(handle) };
    }
}
