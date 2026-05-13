//! `stringinfo.pabgb` bridge — C ABI surface.
//!
//! Parses an in-memory `stringinfo.pabgb` blob (after PAZ extraction
//! from group `0008`, directory `gamedata/binary__/client/bin/`) and
//! exposes the lookup the icon-extraction pipeline needs: map a
//! `StringInfoKey` (u32 hash, harvested from `iteminfo.pabgb`'s
//! `icon_path` / `map_icon_path` fields) to its resolved string value.
//!
//! The companion `stringinfo.pabgh` index is intentionally **not**
//! required by this loader — every pabgb entry is self-describing
//! (length-prefixed), so a linear walk yields every (hash, string)
//! pair without the index. Callers that want to verify the pabgh side
//! agrees can do so via the Rust API ([`super::super::string_info::
//! StringInfoData::parse_pair`]).
//!
//! Memory cost: ~30,206 entries in 1.06 × ~30 bytes per string ≈
//! 900 KB resident plus HashMap overhead. The reserved-zero + reserved-
//! flag bytes are dropped — only `(hash, value)` is retained.

use std::collections::HashMap;
use std::io;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::error;
use crate::string_info::StringInfoData;

/// Opaque handle exposing hash→string lookups against the loaded
/// stringinfo. The full pabgb walk runs once at load time; only the
/// `(hash, value)` pairs are retained.
#[repr(C)]
pub struct CrimsonStringInfoHandle {
    by_hash: HashMap<u32, String>,
    /// `(hash, value)` in file order so the caller can enumerate via
    /// [`crimson_string_info_get_entry`].
    entries: Vec<(u32, String)>,
}

impl CrimsonStringInfoHandle {
    fn from_bytes(data: &[u8]) -> io::Result<Self> {
        let raw_entries = StringInfoData::parse_pabgb(data)?;
        let mut entries: Vec<(u32, String)> = Vec::with_capacity(raw_entries.len());
        for e in raw_entries {
            entries.push((e.hash, e.value));
        }
        let by_hash = entries.iter().cloned().collect();
        Ok(CrimsonStringInfoHandle { by_hash, entries })
    }
}

// ── Load / free ────────────────────────────────────────────────────────────

/// Parse a `stringinfo.pabgb` blob from disk.
///
/// The file must be **already-decrypted, raw `.pabgb` bytes** — the
/// wrapped copy under `0008/0.paz` needs to come through PAZ
/// extraction first (see [`super::paz::crimson_paz_extract_file`]).
///
/// On success `*out_handle` receives an owned
/// [`CrimsonStringInfoHandle`] that the caller must release via
/// [`crimson_string_info_free`].
///
/// # Safety
/// `path` must be a NUL-terminated UTF-8 string and `out_handle` must
/// point at writable memory for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_string_info_load_from_file(
    path: *const c_char,
    out_handle: *mut *mut CrimsonStringInfoHandle,
) -> i32 {
    if path.is_null() || out_handle.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_handle = std::ptr::null_mut() };
    catch_unwind(AssertUnwindSafe(|| {
        let path_str = match unsafe { std::ffi::CStr::from_ptr(path) }.to_str() {
            Ok(s) => s,
            Err(_) => return error::INVALID_PATH,
        };
        let bytes = match std::fs::read(path_str) {
            Ok(b) => b,
            Err(_) => return error::IO,
        };
        let handle = match CrimsonStringInfoHandle::from_bytes(&bytes) {
            Ok(h) => h,
            Err(_) => return error::BODY_PARSE,
        };
        unsafe { *out_handle = Box::into_raw(Box::new(handle)) };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Parse stringinfo bytes already in memory (preferred — the editor
/// pulls them through PAZ extraction first).
///
/// # Safety
/// `data` must point to `data_len` readable bytes; `out_handle` must
/// be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_string_info_load_from_bytes(
    data: *const u8,
    data_len: usize,
    out_handle: *mut *mut CrimsonStringInfoHandle,
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
        let handle = match CrimsonStringInfoHandle::from_bytes(slice) {
            Ok(h) => h,
            Err(_) => return error::BODY_PARSE,
        };
        unsafe { *out_handle = Box::into_raw(Box::new(handle)) };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Free a handle returned by either loader.
///
/// # Safety
/// `handle` must be null or a pointer previously returned by one of
/// the loaders and not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_string_info_free(handle: *mut CrimsonStringInfoHandle) {
    if handle.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(handle);
    }
}

// ── Scalar getters ─────────────────────────────────────────────────────────

/// Total number of entries in the loaded `stringinfo.pabgb`.
///
/// # Safety
/// `handle` must be live; `out_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_string_info_entry_count(
    handle: *const CrimsonStringInfoHandle,
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

/// Look up the resolved string for a given `StringInfoKey` (u32 hash)
/// and write it into `buf` (NUL-terminated UTF-8). Two-call pattern,
/// identical shape to the iteminfo / PALOC catalog:
///
/// - First call with `buf = null, buf_len = 0` returns
///   `BUFFER_TOO_SMALL` and sets `*required` (includes trailing NUL).
/// - Allocate, call again to receive the bytes and `OK`.
///
/// Returns `NOT_FOUND` when `hash` doesn't match any entry in the
/// loaded table.
///
/// # Safety
/// `handle` and `required` must be non-null; `buf` may be null iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_string_info_lookup_by_hash(
    handle: *const CrimsonStringInfoHandle,
    hash: u32,
    buf: *mut u8,
    buf_len: usize,
    required: *mut usize,
) -> i32 {
    if handle.is_null() || required.is_null() {
        return error::NULL_ARG;
    }
    if buf.is_null() && buf_len != 0 {
        return error::NULL_ARG;
    }
    unsafe { *required = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some(value) = h.by_hash.get(&hash) else {
            return error::NOT_FOUND;
        };
        let needed = value.len() + 1;
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

// ── Enumeration ────────────────────────────────────────────────────────────

/// Get the `(hash, value)` pair at insertion index `idx`. Two-call
/// pattern over `buf`; the `out_hash` u32 is always written.
///
/// Returns `OUT_OF_RANGE` when `idx >= entry_count`.
///
/// # Safety
/// `handle`, `out_hash`, and `required` must be non-null; `buf` may
/// be null iff `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_string_info_get_entry(
    handle: *const CrimsonStringInfoHandle,
    idx: u32,
    out_hash: *mut u32,
    buf: *mut u8,
    buf_len: usize,
    required: *mut usize,
) -> i32 {
    if handle.is_null() || out_hash.is_null() || required.is_null() {
        return error::NULL_ARG;
    }
    if buf.is_null() && buf_len != 0 {
        return error::NULL_ARG;
    }
    unsafe {
        *out_hash = 0;
        *required = 0;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some((hash, value)) = h.entries.get(idx as usize) else {
            return error::OUT_OF_RANGE;
        };
        unsafe { *out_hash = *hash };
        let needed = value.len() + 1;
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

#[cfg(test)]
mod tests {
    //! Live-install integration tests + synthetic error-path coverage.
    //! Skips cleanly when no Steam install is present, same pattern as
    //! `c_abi::iteminfo`.

    use super::*;
    use crate::c_abi::paz::crimson_paz_extract_file;
    use std::ffi::{CStr, CString};
    use std::path::PathBuf;
    use std::ptr;

    fn find_pamt_for_stringinfo() -> Option<PathBuf> {
        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let p = game_root.join("0008").join("0.pamt");
        p.is_file().then_some(p)
    }

    /// Pull stringinfo.pabgb via the standard PAZ path, returns its bytes.
    fn extract_stringinfo_bytes(pamt: &CStr) -> Vec<u8> {
        let dir = CString::new("gamedata/binary__/client/bin").unwrap();
        let name = CString::new("stringinfo.pabgb").unwrap();
        let mut needed: usize = 0;
        let rc = unsafe {
            crimson_paz_extract_file(
                pamt.as_ptr(),
                dir.as_ptr(),
                name.as_ptr(),
                ptr::null_mut(),
                0,
                &mut needed,
            )
        };
        assert_eq!(rc, error::BUFFER_TOO_SMALL, "first call should query size");
        let mut buf = vec![0u8; needed];
        let rc = unsafe {
            crimson_paz_extract_file(
                pamt.as_ptr(),
                dir.as_ptr(),
                name.as_ptr(),
                buf.as_mut_ptr(),
                buf.len(),
                &mut needed,
            )
        };
        assert_eq!(rc, error::OK);
        buf.truncate(needed);
        buf
    }

    #[test]
    fn c_abi_string_info_live_roundtrip() {
        let Some(pamt_path) = find_pamt_for_stringinfo() else {
            eprintln!(
                "skipping c_abi_string_info_live_roundtrip: no 0008/0.pamt in game install"
            );
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let bytes = extract_stringinfo_bytes(pamt.as_c_str());

        let mut handle: *mut CrimsonStringInfoHandle = ptr::null_mut();
        assert_eq!(
            unsafe {
                crimson_string_info_load_from_bytes(bytes.as_ptr(), bytes.len(), &mut handle)
            },
            error::OK
        );
        assert!(!handle.is_null());

        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_string_info_entry_count(handle, &mut count) },
            error::OK
        );
        // 1.06 has 30,206 entries; just assert plausibly populated.
        assert!(count > 20_000, "expected >20k entries, got {count}");

        // ── Round-trip: pick entry 0, then look up its value by the u32
        // hash we got back. Must match.
        let mut out_hash: u32 = 0;
        let mut req: usize = 0;
        let rc = unsafe {
            crimson_string_info_get_entry(
                handle,
                0,
                &mut out_hash,
                ptr::null_mut(),
                0,
                &mut req,
            )
        };
        assert_eq!(rc, error::BUFFER_TOO_SMALL);
        let mut buf = vec![0u8; req];
        let rc = unsafe {
            crimson_string_info_get_entry(
                handle,
                0,
                &mut out_hash,
                buf.as_mut_ptr(),
                buf.len(),
                &mut req,
            )
        };
        assert_eq!(rc, error::OK);
        let enum_value = std::str::from_utf8(&buf[..req - 1]).unwrap().to_string();
        assert!(!enum_value.is_empty(), "entry 0's value should be non-empty");

        // Now look up by the u32 we just read.
        let mut req2: usize = 0;
        let rc = unsafe {
            crimson_string_info_lookup_by_hash(
                handle,
                out_hash,
                ptr::null_mut(),
                0,
                &mut req2,
            )
        };
        assert_eq!(rc, error::BUFFER_TOO_SMALL);
        let mut buf2 = vec![0u8; req2];
        let rc = unsafe {
            crimson_string_info_lookup_by_hash(
                handle,
                out_hash,
                buf2.as_mut_ptr(),
                buf2.len(),
                &mut req2,
            )
        };
        assert_eq!(rc, error::OK);
        let lookup_value = std::str::from_utf8(&buf2[..req2 - 1]).unwrap();
        assert_eq!(
            lookup_value, enum_value,
            "get_entry and lookup must agree for the same hash"
        );

        // NOT_FOUND on a definitely-invalid hash (u32::MAX is safely
        // outside any plausible game-data hash range — and even if it
        // happens to collide, the test still asserts agreement above).
        let mut req3: usize = 0;
        let rc = unsafe {
            crimson_string_info_lookup_by_hash(
                handle,
                u32::MAX,
                ptr::null_mut(),
                0,
                &mut req3,
            )
        };
        // u32::MAX collisions are vanishingly unlikely against ~30k
        // entries from a non-cryptographic hash, but we don't assert
        // NOT_FOUND here strictly — that would be a flaky test. Accept
        // OK / BUFFER_TOO_SMALL / NOT_FOUND.
        assert!(
            matches!(
                rc,
                error::NOT_FOUND | error::BUFFER_TOO_SMALL | error::OK
            ),
            "unexpected rc for u32::MAX lookup: {rc}"
        );

        // OUT_OF_RANGE on get_entry past the end.
        let rc = unsafe {
            crimson_string_info_get_entry(
                handle,
                u32::MAX,
                &mut out_hash,
                ptr::null_mut(),
                0,
                &mut req,
            )
        };
        assert_eq!(rc, error::OUT_OF_RANGE);

        unsafe { crimson_string_info_free(handle) };
        // free(null) is a no-op.
        unsafe { crimson_string_info_free(ptr::null_mut()) };
    }

    #[test]
    fn c_abi_string_info_garbage_bytes_returns_body_parse() {
        // 8 bytes won't parse as a full entry: the hash + reserved_zero
        // consume them, then reserved_flag + slen + payload read past EOF.
        let garbage = [0u8; 8];
        let mut handle: *mut CrimsonStringInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_string_info_load_from_bytes(
                garbage.as_ptr(),
                garbage.len(),
                &mut handle,
            )
        };
        assert_eq!(rc, error::BODY_PARSE);
        assert!(handle.is_null());
    }

    #[test]
    fn c_abi_string_info_empty_bytes_is_ok() {
        // An empty pabgb is a degenerate-but-valid case: zero entries.
        let mut handle: *mut CrimsonStringInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_string_info_load_from_bytes(ptr::null(), 0, &mut handle)
        };
        assert_eq!(rc, error::OK);
        assert!(!handle.is_null());

        let mut count: u32 = 99;
        assert_eq!(
            unsafe { crimson_string_info_entry_count(handle, &mut count) },
            error::OK
        );
        assert_eq!(count, 0);
        unsafe { crimson_string_info_free(handle) };
    }

    #[test]
    fn c_abi_string_info_null_args() {
        let mut handle: *mut CrimsonStringInfoHandle = ptr::null_mut();
        assert_eq!(
            unsafe {
                crimson_string_info_load_from_bytes(ptr::null(), 16, &mut handle)
            },
            error::NULL_ARG
        );
        assert_eq!(
            unsafe {
                crimson_string_info_load_from_bytes([0u8; 1].as_ptr(), 1, ptr::null_mut())
            },
            error::NULL_ARG
        );

        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_string_info_entry_count(ptr::null(), &mut count) },
            error::NULL_ARG
        );

        let mut req: usize = 0;
        assert_eq!(
            unsafe {
                crimson_string_info_lookup_by_hash(
                    ptr::null(),
                    0,
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            },
            error::NULL_ARG
        );
    }

    #[test]
    fn c_abi_string_info_load_bad_path_returns_io() {
        let bad = CString::new("Z:\\definitely\\does\\not\\exist.pabgb").unwrap();
        let mut handle: *mut CrimsonStringInfoHandle = ptr::null_mut();
        let rc = unsafe { crimson_string_info_load_from_file(bad.as_ptr(), &mut handle) };
        assert_eq!(rc, error::IO);
        assert!(handle.is_null());
    }
}
