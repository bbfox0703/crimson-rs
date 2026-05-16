//! `royalsupply.pabgb` bridge — C ABI surface.
//!
//! Resolves save-side `RoyalSupplyKey (u16-widened-u32)` to the row's
//! template internal name (`"RoyalSupply_Hernand"` etc.). Name-only,
//! 4 rows in 1.07.

use std::collections::HashMap;
use std::ffi::CStr;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::error;
use crate::royal_supply_info::parse_royal_supply_info_lossy;

#[repr(C)]
pub struct CrimsonRoyalSupplyInfoHandle {
    by_key: HashMap<u32, String>,
    entries: Vec<(u32, String)>,
}

impl CrimsonRoyalSupplyInfoHandle {
    fn from_bytes(pabgb: &[u8], pabgh: &[u8]) -> Self {
        let raw = parse_royal_supply_info_lossy(pabgb, pabgh);
        let mut by_key: HashMap<u32, String> = HashMap::with_capacity(raw.len());
        let mut entries: Vec<(u32, String)> = Vec::with_capacity(raw.len());
        for e in raw {
            if let std::collections::hash_map::Entry::Vacant(v) = by_key.entry(e.key) {
                v.insert(e.name.clone());
                entries.push((e.key, e.name));
            }
        }
        CrimsonRoyalSupplyInfoHandle { by_key, entries }
    }
}

/// # Safety
/// Path args must be NUL-terminated UTF-8 strings; out_handle non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_royal_supply_info_load_from_file(
    pabgb_path: *const c_char,
    pabgh_path: *const c_char,
    out_handle: *mut *mut CrimsonRoyalSupplyInfoHandle,
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
        let handle = CrimsonRoyalSupplyInfoHandle::from_bytes(&pabgb, &pabgh);
        unsafe { *out_handle = Box::into_raw(Box::new(handle)) };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// # Safety
/// `pabgb`/`pabgh` may be null iff length is 0; out_handle non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_royal_supply_info_load_from_bytes(
    pabgb: *const u8,
    pabgb_len: usize,
    pabgh: *const u8,
    pabgh_len: usize,
    out_handle: *mut *mut CrimsonRoyalSupplyInfoHandle,
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
        let handle =
            CrimsonRoyalSupplyInfoHandle::from_bytes(pabgb_slice, pabgh_slice);
        unsafe { *out_handle = Box::into_raw(Box::new(handle)) };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// # Safety
/// `handle` must be null or live and unfreed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_royal_supply_info_free(
    handle: *mut CrimsonRoyalSupplyInfoHandle,
) {
    if handle.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(handle);
    }
}

/// # Safety
/// `handle` live; `out_count` non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_royal_supply_info_entry_count(
    handle: *const CrimsonRoyalSupplyInfoHandle,
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

/// # Safety
/// `handle`/`required` non-null; `buf` may be null iff `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_royal_supply_info_lookup_string_key(
    handle: *const CrimsonRoyalSupplyInfoHandle,
    royal_supply_key: u32,
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
        let Some(name) = h.by_key.get(&royal_supply_key) else {
            return error::NOT_FOUND;
        };
        super::write_str_to_buf(name, buf, buf_len, required)
    }))
    .unwrap_or(error::PANIC)
}

/// # Safety
/// `handle`/`out_key`/`required` non-null; `buf` may be null iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_royal_supply_info_get_entry(
    handle: *const CrimsonRoyalSupplyInfoHandle,
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
        super::write_str_to_buf(name, buf, buf_len, required)
    }))
    .unwrap_or(error::PANIC)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::c_abi::paz::crimson_paz_extract_file;
    use std::ffi::{CStr, CString};
    use std::path::PathBuf;
    use std::ptr;

    const KNOWN: &[(u32, &str)] = &[
        (0x4242, "RoyalSupply_Hernand"),
        (0x4243, "RoyalSupply_Demeniss"),
        (0x4245, "RoyalSupply_Varnia"),
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

    #[test]
    fn c_abi_royal_supply_info_live() {
        let Some(pamt_path) = find_pamt() else {
            eprintln!("skipping c_abi_royal_supply_info_live: no game install");
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let pabgb = extract_file(
            pamt.as_c_str(),
            "gamedata/binary__/client/bin",
            "royalsupply.pabgb",
        );
        let pabgh = extract_file(
            pamt.as_c_str(),
            "gamedata/binary__/client/bin",
            "royalsupply.pabgh",
        );
        let mut sh: *mut CrimsonRoyalSupplyInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_royal_supply_info_load_from_bytes(
                pabgb.as_ptr(),
                pabgb.len(),
                pabgh.as_ptr(),
                pabgh.len(),
                &mut sh,
            )
        };
        assert_eq!(rc, error::OK);
        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_royal_supply_info_entry_count(sh, &mut count) },
            error::OK
        );
        assert_eq!(count, 4);
        for &(key, expected) in KNOWN {
            let mut req: usize = 0;
            assert_eq!(
                unsafe {
                    crimson_royal_supply_info_lookup_string_key(
                        sh,
                        key,
                        ptr::null_mut(),
                        0,
                        &mut req,
                    )
                },
                error::BUFFER_TOO_SMALL
            );
            let mut buf = vec![0u8; req];
            let mut req2: usize = 0;
            assert_eq!(
                unsafe {
                    crimson_royal_supply_info_lookup_string_key(
                        sh,
                        key,
                        buf.as_mut_ptr(),
                        buf.len(),
                        &mut req2,
                    )
                },
                error::OK
            );
            let got = std::str::from_utf8(&buf[..req2 - 1]).unwrap();
            assert_eq!(got, expected, "key 0x{key:04x}");
        }
        unsafe { crimson_royal_supply_info_free(sh) };
    }
}
