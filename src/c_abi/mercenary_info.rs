//! `mercenaryinfo.pabgb` bridge — C ABI surface.
//!
//! Resolves save-side `MercenaryKey (u8-widened-u32)` to the row's
//! **template** internal name (e.g. `"Mercenary_Main"`,
//! `"Vehicle_Horse"`). Distinct from `MercenaryNo`, which is the
//! per-save instance ID — the editor's "Rename Mercenary" UI uses this
//! bridge to surface the template lineage cleanly.
//!
//! Mirrors [`super::faction_node_info`] in shape — name-only, **no
//! `lookup_display_name` function**, no PALOC chain probed yet.
//!
//! 18 rows in 1.07. PABGH is a NEW variant: `u16 count + (u8 key, u32 offset)*`
//! (5-byte entries). See `src/mercenary_info/mod.rs` for byte-level
//! schema notes.

use std::collections::HashMap;
use std::ffi::CStr;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::error;
use crate::mercenary_info::parse_mercenary_info_lossy;

/// Opaque handle exposing `(MercenaryKey, internal_name)` lookups
/// against the loaded `mercenaryinfo.pabgb` + `.pabgh` pair.
#[repr(C)]
pub struct CrimsonMercenaryInfoHandle {
    by_key: HashMap<u32, String>,
    entries: Vec<(u32, String)>,
}

impl CrimsonMercenaryInfoHandle {
    fn from_bytes(pabgb: &[u8], pabgh: &[u8]) -> Self {
        let raw = parse_mercenary_info_lossy(pabgb, pabgh);
        let mut by_key: HashMap<u32, String> = HashMap::with_capacity(raw.len());
        let mut entries: Vec<(u32, String)> = Vec::with_capacity(raw.len());
        for e in raw {
            if let std::collections::hash_map::Entry::Vacant(v) = by_key.entry(e.key) {
                v.insert(e.name.clone());
                entries.push((e.key, e.name));
            }
        }
        CrimsonMercenaryInfoHandle { by_key, entries }
    }
}

// ── Load / free ────────────────────────────────────────────────────────────

/// Parse `mercenaryinfo.pabgb` + `.pabgh` from disk.
///
/// # Safety
/// Both path arguments must be NUL-terminated UTF-8 strings.
/// `out_handle` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_mercenary_info_load_from_file(
    pabgb_path: *const c_char,
    pabgh_path: *const c_char,
    out_handle: *mut *mut CrimsonMercenaryInfoHandle,
) -> i32 {
    if pabgb_path.is_null() || pabgh_path.is_null() || out_handle.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_handle = std::ptr::null_mut() };
    catch_unwind(AssertUnwindSafe(|| {
        let pabgb_str = match unsafe { CStr::from_ptr(pabgb_path) }.to_str() {
            Ok(s) => s,
            Err(_) => return error::INVALID_PATH,
        };
        let pabgh_str = match unsafe { CStr::from_ptr(pabgh_path) }.to_str() {
            Ok(s) => s,
            Err(_) => return error::INVALID_PATH,
        };
        let pabgb = match std::fs::read(pabgb_str) {
            Ok(b) => b,
            Err(_) => return error::IO,
        };
        let pabgh = match std::fs::read(pabgh_str) {
            Ok(b) => b,
            Err(_) => return error::IO,
        };
        let handle = CrimsonMercenaryInfoHandle::from_bytes(&pabgb, &pabgh);
        unsafe { *out_handle = Box::into_raw(Box::new(handle)) };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Parse mercenaryinfo pabgb + pabgh bytes already in memory.
///
/// # Safety
/// Both `pabgb` and `pabgh` must point to `*_len` readable bytes (may
/// be null iff length is 0). `out_handle` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_mercenary_info_load_from_bytes(
    pabgb: *const u8,
    pabgb_len: usize,
    pabgh: *const u8,
    pabgh_len: usize,
    out_handle: *mut *mut CrimsonMercenaryInfoHandle,
) -> i32 {
    if out_handle.is_null() {
        return error::NULL_ARG;
    }
    if (pabgb.is_null() && pabgb_len != 0) || (pabgh.is_null() && pabgh_len != 0) {
        return error::NULL_ARG;
    }
    unsafe { *out_handle = std::ptr::null_mut() };
    catch_unwind(AssertUnwindSafe(|| {
        let pabgb_slice = if pabgb_len == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(pabgb, pabgb_len) }
        };
        let pabgh_slice = if pabgh_len == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(pabgh, pabgh_len) }
        };
        let handle = CrimsonMercenaryInfoHandle::from_bytes(pabgb_slice, pabgh_slice);
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
pub unsafe extern "C" fn crimson_mercenary_info_free(
    handle: *mut CrimsonMercenaryInfoHandle,
) {
    if handle.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(handle);
    }
}

// ── Scalar getters ─────────────────────────────────────────────────────────

/// Total number of `mercenaryinfo` rows in the loaded table.
///
/// # Safety
/// `handle` must be live; `out_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_mercenary_info_entry_count(
    handle: *const CrimsonMercenaryInfoHandle,
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

// ── Lookups ────────────────────────────────────────────────────────────────

/// Look up the internal template name for a given `MercenaryKey` and
/// write it into `buf` (NUL-terminated UTF-8). Two-call pattern.
///
/// Returns `NOT_FOUND` when `mercenary_key` isn't in the table.
///
/// # Safety
/// `handle` and `required` must be non-null; `buf` may be null iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_mercenary_info_lookup_string_key(
    handle: *const CrimsonMercenaryInfoHandle,
    mercenary_key: u32,
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
        let Some(name) = h.by_key.get(&mercenary_key) else {
            return error::NOT_FOUND;
        };
        write_str_to_buf(name, buf, buf_len, required)
    }))
    .unwrap_or(error::PANIC)
}

/// Get the `(mercenary_key, internal_name)` pair at insertion index
/// `idx`. Two-call pattern.
///
/// # Safety
/// `handle`, `out_key`, and `required` must be non-null; `buf` may be
/// null iff `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_mercenary_info_get_entry(
    handle: *const CrimsonMercenaryInfoHandle,
    idx: u32,
    out_key: *mut u32,
    buf: *mut u8,
    buf_len: usize,
    required: *mut usize,
) -> i32 {
    if handle.is_null() || out_key.is_null() || required.is_null() {
        return error::NULL_ARG;
    }
    if buf.is_null() && buf_len != 0 {
        return error::NULL_ARG;
    }
    unsafe {
        *out_key = 0;
        *required = 0;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some((key, name)) = h.entries.get(idx as usize) else {
            return error::OUT_OF_RANGE;
        };
        unsafe { *out_key = *key };
        write_str_to_buf(name, buf, buf_len, required)
    }))
    .unwrap_or(error::PANIC)
}

fn write_str_to_buf(
    src: &str,
    buf: *mut u8,
    buf_len: usize,
    required: *mut usize,
) -> i32 {
    let needed = src.len() + 1;
    unsafe { *required = needed };
    if buf_len < needed {
        return error::BUFFER_TOO_SMALL;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(src.as_ptr(), buf, src.len());
        *buf.add(src.len()) = 0;
    }
    error::OK
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::c_abi::paz::crimson_paz_extract_file;
    use std::ffi::{CStr, CString};
    use std::path::PathBuf;
    use std::ptr;

    const KNOWN: &[(u32, &str)] = &[
        (64, "Mercenary_Main"),
        (78, "Vehicle_Horse"),
        (79, "Vehicle_Dragon"),
        (80, "Vehicle_WarMachine"),
        (82, "Vehicle_WarMachine_Raptor"),
    ];

    fn find_pamt() -> Option<PathBuf> {
        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let p = game_root.join("0008").join("0.pamt");
        p.is_file().then_some(p)
    }

    fn extract_file(pamt: &CStr, dir: &str, name: &str) -> Vec<u8> {
        let dir_c = CString::new(dir).unwrap();
        let name_c = CString::new(name).unwrap();
        let mut needed: usize = 0;
        let rc = unsafe {
            crimson_paz_extract_file(
                pamt.as_ptr(),
                dir_c.as_ptr(),
                name_c.as_ptr(),
                ptr::null_mut(),
                0,
                &mut needed,
            )
        };
        assert_eq!(rc, error::BUFFER_TOO_SMALL);
        let mut buf = vec![0u8; needed];
        let rc = unsafe {
            crimson_paz_extract_file(
                pamt.as_ptr(),
                dir_c.as_ptr(),
                name_c.as_ptr(),
                buf.as_mut_ptr(),
                buf.len(),
                &mut needed,
            )
        };
        assert_eq!(rc, error::OK);
        buf.truncate(needed);
        buf
    }

    fn read_string_result(
        rc_first: i32,
        first_required: usize,
        run_fill: impl FnOnce(*mut u8, usize, *mut usize) -> i32,
    ) -> String {
        assert_eq!(rc_first, error::BUFFER_TOO_SMALL);
        let mut buf = vec![0u8; first_required];
        let mut req2: usize = 0;
        let rc = run_fill(buf.as_mut_ptr(), buf.len(), &mut req2);
        assert_eq!(rc, error::OK);
        std::str::from_utf8(&buf[..req2 - 1]).unwrap().to_owned()
    }

    #[test]
    fn c_abi_mercenary_info_live() {
        let Some(pamt_path) = find_pamt() else {
            eprintln!("skipping c_abi_mercenary_info_live: no game install");
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let pabgb = extract_file(
            pamt.as_c_str(),
            "gamedata/binary__/client/bin",
            "mercenaryinfo.pabgb",
        );
        let pabgh = extract_file(
            pamt.as_c_str(),
            "gamedata/binary__/client/bin",
            "mercenaryinfo.pabgh",
        );

        let mut sh: *mut CrimsonMercenaryInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_mercenary_info_load_from_bytes(
                pabgb.as_ptr(),
                pabgb.len(),
                pabgh.as_ptr(),
                pabgh.len(),
                &mut sh,
            )
        };
        assert_eq!(rc, error::OK);
        assert!(!sh.is_null());

        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_mercenary_info_entry_count(sh, &mut count) },
            error::OK
        );
        assert_eq!(count, 18, "expected exactly 18 mercenaryinfo rows in 1.07");

        for &(key, expected) in KNOWN {
            let mut req: usize = 0;
            let rc_size = unsafe {
                crimson_mercenary_info_lookup_string_key(
                    sh,
                    key,
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            };
            let got = read_string_result(rc_size, req, |b, n, r| unsafe {
                crimson_mercenary_info_lookup_string_key(sh, key, b, n, r)
            });
            assert_eq!(got, expected, "MercenaryKey {key} name mismatch");
        }

        let mut req: usize = 0;
        assert_eq!(
            unsafe {
                crimson_mercenary_info_lookup_string_key(
                    sh,
                    u32::MAX,
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            },
            error::NOT_FOUND,
        );

        unsafe { crimson_mercenary_info_free(sh) };
    }

    #[test]
    fn c_abi_mercenary_info_null_args() {
        let mut sh: *mut CrimsonMercenaryInfoHandle = ptr::null_mut();
        assert_eq!(
            unsafe {
                crimson_mercenary_info_load_from_bytes(
                    ptr::null(),
                    16,
                    ptr::null(),
                    16,
                    &mut sh,
                )
            },
            error::NULL_ARG,
        );
        assert_eq!(
            unsafe {
                crimson_mercenary_info_load_from_bytes(
                    [0u8; 1].as_ptr(),
                    1,
                    [0u8; 1].as_ptr(),
                    1,
                    ptr::null_mut(),
                )
            },
            error::NULL_ARG,
        );
    }

    #[test]
    fn c_abi_mercenary_info_empty_bytes_yields_empty_handle() {
        let mut sh: *mut CrimsonMercenaryInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_mercenary_info_load_from_bytes(
                ptr::null(),
                0,
                ptr::null(),
                0,
                &mut sh,
            )
        };
        assert_eq!(rc, error::OK);
        assert!(!sh.is_null());
        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_mercenary_info_entry_count(sh, &mut count) },
            error::OK
        );
        assert_eq!(count, 0);
        unsafe { crimson_mercenary_info_free(sh) };
    }
}
