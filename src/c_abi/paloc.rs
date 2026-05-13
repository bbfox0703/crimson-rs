//! PALOC localization table — C ABI surface.
//!
//! Loads a `.paloc` file (game localization string table) into an opaque
//! handle and exposes a single lookup primitive
//! ([`crimson_paloc_lookup`]) plus enumeration getters
//! ([`crimson_paloc_entry_count`], [`crimson_paloc_get_entry`]). The
//! handle owns its data — the bytes read from disk are copied into a
//! [`HashMap`] so the public API isn't bound by Rust lifetimes.
//!
//! Memory cost: roughly the file size on disk plus the HashMap overhead
//! (~3-4× the entry-string bytes). For the 7 MB `localizationstring_eng.paloc`
//! shipped with 1.06, that's ~25 MB resident — well within a desktop
//! editor's budget.
//!
//! Strings are passed as `(ptr, len)` rather than NUL-terminated. PALOC
//! values can in principle contain embedded NULs (rare but possible in
//! formatted text), so the length-prefixed shape avoids surprises.

use std::collections::HashMap;
use std::ffi::CStr;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::error;
use crate::binary::paloc::LocalizationFile;

/// Opaque handle handed across the FFI. Owns a `HashMap<String, String>`
/// of key → value plus the entries in insertion order so the caller can
/// enumerate them sequentially.
#[repr(C)]
pub struct CrimsonPalocHandle {
    by_key: HashMap<String, String>,
    /// Original (key, value) ordering, preserved for index-based
    /// enumeration via [`crimson_paloc_get_entry`].
    entries: Vec<(String, String)>,
}

impl CrimsonPalocHandle {
    fn from_bytes(data: &[u8]) -> Result<Self, std::io::Error> {
        let file = LocalizationFile::parse(data)?;
        let entries: Vec<(String, String)> = file
            .entries
            .iter()
            .map(|e| (e.string_key.data.to_owned(), e.string_value.data.to_owned()))
            .collect();
        let by_key = entries.iter().cloned().collect();
        Ok(CrimsonPalocHandle { by_key, entries })
    }
}

// ── Load / free ────────────────────────────────────────────────────────────

/// Load and parse a `.paloc` file from disk.
///
/// **Important**: this accepts **already-decrypted, raw PALOC bytes** on
/// disk (i.e. a file the caller previously extracted from a PAZ archive
/// via game tools). The raw `gamedata/*.paloc` files in a Crimson Desert
/// Steam install are wrapped (encrypted + compressed) and cannot be
/// passed straight in — feed them through PAZ extraction first, then
/// this loader, or use [`crimson_paloc_load_from_bytes`].
///
/// `path` must be a NUL-terminated UTF-8 string. On success `*out_handle`
/// receives an owned [`CrimsonPalocHandle`] that the caller must release
/// via [`crimson_paloc_free`]. Returns `OK` on success.
///
/// # Safety
/// `path` and `out_handle` must be non-null and point at writable memory
/// for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_paloc_load_from_file(
    path: *const c_char,
    out_handle: *mut *mut CrimsonPalocHandle,
) -> i32 {
    if path.is_null() || out_handle.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_handle = std::ptr::null_mut() };
    catch_unwind(AssertUnwindSafe(|| {
        let path_str = match unsafe { CStr::from_ptr(path) }.to_str() {
            Ok(s) => s,
            Err(_) => return error::INVALID_PATH,
        };
        let bytes = match std::fs::read(path_str) {
            Ok(b) => b,
            Err(_) => return error::IO,
        };
        let handle = match CrimsonPalocHandle::from_bytes(&bytes) {
            Ok(h) => h,
            Err(_) => return error::BODY_PARSE,
        };
        unsafe {
            *out_handle = Box::into_raw(Box::new(handle));
        }
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Load and parse PALOC bytes that already live in memory. Useful when
/// the caller pulls them through a PAZ extractor before handing them
/// here. Same semantics as [`crimson_paloc_load_from_file`] minus the
/// filesystem step.
///
/// # Safety
/// `data` must point to `data_len` readable bytes; `out_handle` must be
/// non-null. Pass `data_len == 0` and `data == null` together for an
/// empty input — that's rejected as `BODY_PARSE`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_paloc_load_from_bytes(
    data: *const u8,
    data_len: usize,
    out_handle: *mut *mut CrimsonPalocHandle,
) -> i32 {
    if out_handle.is_null() {
        return error::NULL_ARG;
    }
    if data.is_null() && data_len != 0 {
        return error::NULL_ARG;
    }
    unsafe { *out_handle = std::ptr::null_mut() };
    catch_unwind(AssertUnwindSafe(|| {
        let slice = if data_len == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(data, data_len) }
        };
        let handle = match CrimsonPalocHandle::from_bytes(slice) {
            Ok(h) => h,
            Err(_) => return error::BODY_PARSE,
        };
        unsafe {
            *out_handle = Box::into_raw(Box::new(handle));
        }
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Free a handle returned by [`crimson_paloc_load_from_file`].
///
/// # Safety
/// `handle` must either be null or a pointer previously returned by
/// [`crimson_paloc_load_from_file`] that has not yet been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_paloc_free(handle: *mut CrimsonPalocHandle) {
    if handle.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(handle);
    }
}

// ── Scalar getters ─────────────────────────────────────────────────────────

/// Total number of (key, value) pairs in the table.
///
/// # Safety
/// `handle` must be live. `out_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_paloc_entry_count(
    handle: *const CrimsonPalocHandle,
    out_count: *mut u32,
) -> i32 {
    if handle.is_null() || out_count.is_null() {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        unsafe { *out_count = h.entries.len() as u32 };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

// ── Lookup ─────────────────────────────────────────────────────────────────

/// Look up the value for `key` and copy its UTF-8 bytes into `buf`.
///
/// Two-call pattern, same shape as `crimson_save_get_block_class_name`:
/// pass `buf = null, buf_len = 0` first to read the required size
/// (returns `BUFFER_TOO_SMALL` with `*required` set); allocate, call
/// again to fill the buffer. `*required` includes a trailing NUL.
///
/// Returns:
/// - `OK` on success; `*required` is set to the bytes written (including NUL).
/// - `BUFFER_TOO_SMALL` if `buf_len < required`. `*required` is set
///   to the needed size.
/// - `NOT_FOUND` if the key is absent. `*required` is left at 0.
/// - `NULL_ARG` for null `handle` / `key` / `required`, or
///   for null `buf` with non-zero `buf_len`.
///
/// # Safety
/// `handle` and `key` must point to live memory for `key_len` bytes (any
/// bytes — UTF-8 is checked internally). `buf` (when non-null) and
/// `required` must point to writable memory for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_paloc_lookup(
    handle: *const CrimsonPalocHandle,
    key: *const u8,
    key_len: usize,
    buf: *mut u8,
    buf_len: usize,
    required: *mut usize,
) -> i32 {
    if handle.is_null() || required.is_null() {
        return error::NULL_ARG;
    }
    if key.is_null() && key_len != 0 {
        return error::NULL_ARG;
    }
    if buf.is_null() && buf_len != 0 {
        return error::NULL_ARG;
    }
    unsafe { *required = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let key_bytes = if key_len == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(key, key_len) }
        };
        let key_str = match std::str::from_utf8(key_bytes) {
            Ok(s) => s,
            Err(_) => return error::NULL_ARG, // bad UTF-8 in key
        };
        let Some(value) = h.by_key.get(key_str) else {
            return error::NOT_FOUND;
        };
        let needed = value.len() + 1; // +1 for trailing NUL
        unsafe { *required = needed };
        if buf_len < needed {
            return error::BUFFER_TOO_SMALL;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(value.as_ptr(), buf, value.len());
            *buf.add(value.len()) = 0;
        }
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

// ── Index-based enumeration ────────────────────────────────────────────────

/// Get the (key, value) pair at index `idx` in insertion order. Two-call
/// pattern: pass `key_buf = null, value_buf = null` first to read
/// `*key_required` / `*value_required` (returns `BUFFER_TOO_SMALL`);
/// allocate, call again to fill both buffers.
///
/// Returns:
/// - `OK` on success.
/// - `BUFFER_TOO_SMALL` when either buffer is too small. Both required
///   fields are populated on this path so the caller can allocate once.
/// - `OUT_OF_RANGE` when `idx >= entry_count`.
/// - `NULL_ARG` on null `handle` / `key_required` / `value_required`.
///
/// # Safety
/// `handle` must be live. The two `*_required` out-pointers and the two
/// optional buffers (when non-null) must point to writable memory for
/// the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_paloc_get_entry(
    handle: *const CrimsonPalocHandle,
    idx: u32,
    key_buf: *mut u8,
    key_buf_len: usize,
    key_required: *mut usize,
    value_buf: *mut u8,
    value_buf_len: usize,
    value_required: *mut usize,
) -> i32 {
    if handle.is_null() || key_required.is_null() || value_required.is_null() {
        return error::NULL_ARG;
    }
    if (key_buf.is_null() && key_buf_len != 0) || (value_buf.is_null() && value_buf_len != 0) {
        return error::NULL_ARG;
    }
    unsafe {
        *key_required = 0;
        *value_required = 0;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some((k, v)) = h.entries.get(idx as usize) else {
            return error::OUT_OF_RANGE;
        };
        let key_needed = k.len() + 1;
        let value_needed = v.len() + 1;
        unsafe {
            *key_required = key_needed;
            *value_required = value_needed;
        }
        if key_buf_len < key_needed || value_buf_len < value_needed {
            return error::BUFFER_TOO_SMALL;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(k.as_ptr(), key_buf, k.len());
            *key_buf.add(k.len()) = 0;
            std::ptr::copy_nonoverlapping(v.as_ptr(), value_buf, v.len());
            *value_buf.add(v.len()) = 0;
        }
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

#[cfg(test)]
mod tests {
    //! Self-contained tests over a synthetic 3-entry PALOC. Avoid the
    //! live `gamedata/*.paloc` files — those are encrypted/wrapped and
    //! must be PAZ-extracted before reaching this loader. The PALOC
    //! parser itself is already covered by `test_paloc_*` in lib.rs;
    //! these tests only exercise the C ABI plumbing.

    use super::*;
    use std::ffi::CString;
    use std::ptr;

    /// Build a tiny 3-entry PALOC blob directly. `LocalizationEntry`'s
    /// `CString` field has a private `raw` slot so struct-literal
    /// construction isn't available; emit the wire format byte-by-byte
    /// instead — same layout the parser reads.
    fn synthesise_paloc() -> Vec<u8> {
        fn push_entry(out: &mut Vec<u8>, unk_id: u64, key: &str, value: &str) {
            out.extend_from_slice(&unk_id.to_le_bytes());
            out.extend_from_slice(&(key.len() as u32).to_le_bytes());
            out.extend_from_slice(key.as_bytes());
            out.extend_from_slice(&(value.len() as u32).to_le_bytes());
            out.extend_from_slice(value.as_bytes());
        }

        let mut bytes = Vec::new();
        push_entry(&mut bytes, 1, "ITEM_GOLD", "Gold");
        push_entry(&mut bytes, 2, "ITEM_POTION", "Health Potion");
        // Empty value to make sure we handle 0-length strings.
        push_entry(&mut bytes, 3, "EMPTY_KEY", "");
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes
    }

    fn read_lookup(handle: *const CrimsonPalocHandle, key: &str) -> Option<String> {
        let mut required: usize = 0;
        let rc = unsafe {
            crimson_paloc_lookup(
                handle,
                key.as_ptr(),
                key.len(),
                ptr::null_mut(),
                0,
                &mut required,
            )
        };
        if rc == error::NOT_FOUND {
            return None;
        }
        // Empty-value entries (required == 1) take the OK path directly
        // because buf_len 0 >= 1 is false → BUFFER_TOO_SMALL is the only
        // valid first-call return for a present key.
        assert_eq!(rc, error::BUFFER_TOO_SMALL, "first lookup call should report size");
        let mut buf = vec![0u8; required];
        let rc = unsafe {
            crimson_paloc_lookup(
                handle,
                key.as_ptr(),
                key.len(),
                buf.as_mut_ptr(),
                buf.len(),
                &mut required,
            )
        };
        assert_eq!(rc, error::OK);
        // Strip the trailing NUL.
        Some(String::from_utf8(buf[..required - 1].to_vec()).unwrap())
    }

    #[test]
    fn c_abi_paloc_smoke() {
        let bytes = synthesise_paloc();
        let mut handle: *mut CrimsonPalocHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_paloc_load_from_bytes(bytes.as_ptr(), bytes.len(), &mut handle) },
            error::OK
        );
        assert!(!handle.is_null());

        // entry_count surfaces the fixture's 3.
        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_paloc_entry_count(handle, &mut count) },
            error::OK
        );
        assert_eq!(count, 3);

        // ── Lookup hits.
        assert_eq!(read_lookup(handle, "ITEM_GOLD"), Some("Gold".to_string()));
        assert_eq!(
            read_lookup(handle, "ITEM_POTION"),
            Some("Health Potion".to_string())
        );

        // ── Empty value: required == 1 (just the trailing NUL). The
        // first call reports BUFFER_TOO_SMALL with required = 1; the
        // second succeeds with a 1-byte buffer and an empty string.
        let key = "EMPTY_KEY";
        let mut required: usize = 0;
        let rc = unsafe {
            crimson_paloc_lookup(
                handle,
                key.as_ptr(),
                key.len(),
                ptr::null_mut(),
                0,
                &mut required,
            )
        };
        assert_eq!(rc, error::BUFFER_TOO_SMALL);
        assert_eq!(required, 1);
        let mut buf = [0u8; 1];
        let rc = unsafe {
            crimson_paloc_lookup(
                handle,
                key.as_ptr(),
                key.len(),
                buf.as_mut_ptr(),
                buf.len(),
                &mut required,
            )
        };
        assert_eq!(rc, error::OK);
        assert_eq!(buf[0], 0, "empty value still writes a trailing NUL");

        // ── Round-trip through get_entry: enumerate index 0, then look
        // up the same key and confirm the value matches.
        let mut k_req: usize = 0;
        let mut v_req: usize = 0;
        let rc = unsafe {
            crimson_paloc_get_entry(
                handle,
                0,
                ptr::null_mut(),
                0,
                &mut k_req,
                ptr::null_mut(),
                0,
                &mut v_req,
            )
        };
        assert_eq!(rc, error::BUFFER_TOO_SMALL);
        let mut k_buf = vec![0u8; k_req];
        let mut v_buf = vec![0u8; v_req];
        let rc = unsafe {
            crimson_paloc_get_entry(
                handle,
                0,
                k_buf.as_mut_ptr(),
                k_buf.len(),
                &mut k_req,
                v_buf.as_mut_ptr(),
                v_buf.len(),
                &mut v_req,
            )
        };
        assert_eq!(rc, error::OK);
        let enum_key = std::str::from_utf8(&k_buf[..k_req - 1]).unwrap();
        let enum_value = std::str::from_utf8(&v_buf[..v_req - 1]).unwrap();
        assert_eq!(enum_key, "ITEM_GOLD");
        assert_eq!(enum_value, "Gold");
        assert_eq!(
            read_lookup(handle, enum_key),
            Some(enum_value.to_string()),
            "get_entry and lookup must agree for the same key"
        );

        // ── NOT_FOUND on a definitely-absent key.
        let absent = "definitely_not_in_paloc_xyzzy_42";
        let mut req: usize = 0;
        let rc = unsafe {
            crimson_paloc_lookup(
                handle,
                absent.as_ptr(),
                absent.len(),
                ptr::null_mut(),
                0,
                &mut req,
            )
        };
        assert_eq!(rc, error::NOT_FOUND);

        // ── OUT_OF_RANGE on get_entry past the end.
        let rc = unsafe {
            crimson_paloc_get_entry(
                handle,
                u32::MAX,
                ptr::null_mut(),
                0,
                &mut k_req,
                ptr::null_mut(),
                0,
                &mut v_req,
            )
        };
        assert_eq!(rc, error::OUT_OF_RANGE);

        // ── NULL_ARG paths.
        assert_eq!(
            unsafe {
                crimson_paloc_lookup(
                    ptr::null(),
                    absent.as_ptr(),
                    absent.len(),
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            },
            error::NULL_ARG
        );
        assert_eq!(
            unsafe { crimson_paloc_entry_count(ptr::null(), &mut count) },
            error::NULL_ARG
        );
        assert_eq!(
            unsafe {
                crimson_paloc_load_from_bytes(ptr::null(), 16, ptr::null_mut())
            },
            error::NULL_ARG
        );

        unsafe { crimson_paloc_free(handle) };
        // Double-free guard: free(null) is a no-op.
        unsafe { crimson_paloc_free(ptr::null_mut()) };
    }

    #[test]
    fn c_abi_paloc_load_bad_path_returns_io() {
        let bad = CString::new("Z:\\definitely\\does\\not\\exist.paloc").unwrap();
        let mut handle: *mut CrimsonPalocHandle = ptr::null_mut();
        let rc = unsafe { crimson_paloc_load_from_file(bad.as_ptr(), &mut handle) };
        assert_eq!(rc, error::IO);
        assert!(handle.is_null());
    }

    #[test]
    fn c_abi_paloc_load_garbage_bytes_returns_body_parse() {
        // Anything that isn't a valid PALOC layout should surface as
        // BODY_PARSE — the loader must not allocate gigabytes based on
        // a corrupt trailing entry count (regression: the raw wrapped
        // gamedata/*.paloc files crash the parser this way).
        let garbage = [0u8; 32];
        let mut handle: *mut CrimsonPalocHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_paloc_load_from_bytes(garbage.as_ptr(), garbage.len(), &mut handle)
        };
        assert_eq!(rc, error::BODY_PARSE);
        assert!(handle.is_null());
    }
}
