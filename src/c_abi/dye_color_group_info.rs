//! `dyecolorgroupinfo.pabgb` bridge — C ABI surface.
//!
//! Resolves `DyeColorGroupInfoKey (u32)` — the gamedata key stored as
//! `ItemDyeSaveData._dyeColorGroupInfoKey` in the save's
//! `_itemDyeDataList`. Returns the row's internal name (e.g.
//! `"Her_Color_Group_I"`, `"Dem_Color_Group_III"`); the dye UI shows
//! a localized version, but for v1 the C# editor renders the raw
//! internal name in the "Named family" dropdown.
//!
//! Both `dyecolorgroupinfo.pabgb` and `dyecolorgroupinfo.pabgh` must
//! be loaded together — the PABGH provides the explicit (key, offset)
//! index that locates each row.
//!
//! See [`crate::dye_color_group_info`] for the row schema.

use std::collections::HashMap;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::error;
use crate::dye_color_group_info::parse_dye_color_group_info_lossy;

/// Opaque handle exposing `(DyeColorGroupInfoKey, internal_name)` lookups
/// against the loaded `dyecolorgroupinfo.pabgb` + `.pabgh`.
#[repr(C)]
pub struct CrimsonDyeColorGroupInfoHandle {
    by_key: HashMap<u32, String>,
    entries: Vec<(u32, String)>,
}

impl CrimsonDyeColorGroupInfoHandle {
    fn from_bytes(pabgb: &[u8], pabgh: &[u8]) -> Self {
        let raw = parse_dye_color_group_info_lossy(pabgb, pabgh);
        let mut by_key: HashMap<u32, String> = HashMap::with_capacity(raw.len());
        let mut entries: Vec<(u32, String)> = Vec::with_capacity(raw.len());
        for e in raw {
            if let std::collections::hash_map::Entry::Vacant(v) = by_key.entry(e.key) {
                v.insert(e.name.clone());
                entries.push((e.key, e.name));
            }
        }
        CrimsonDyeColorGroupInfoHandle { by_key, entries }
    }
}

// ── Load / free ────────────────────────────────────────────────────────────

/// Parse `dyecolorgroupinfo.pabgb` + `.pabgh` from disk.
///
/// # Safety
/// `pabgb_path` and `pabgh_path` must be NUL-terminated UTF-8 strings.
/// `out_handle` must be non-null and writable for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_dye_color_group_info_load_from_file(
    pabgb_path: *const c_char,
    pabgh_path: *const c_char,
    out_handle: *mut *mut CrimsonDyeColorGroupInfoHandle,
) -> i32 {
    if pabgb_path.is_null() || pabgh_path.is_null() || out_handle.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_handle = std::ptr::null_mut() };
    catch_unwind(AssertUnwindSafe(|| {
        let pabgb_str = match unsafe { std::ffi::CStr::from_ptr(pabgb_path) }.to_str() {
            Ok(s) => s,
            Err(_) => return error::INVALID_PATH,
        };
        let pabgh_str = match unsafe { std::ffi::CStr::from_ptr(pabgh_path) }.to_str() {
            Ok(s) => s,
            Err(_) => return error::INVALID_PATH,
        };
        let pabgb: Vec<u8> = match std::fs::read(pabgb_str) {
            Ok(b) => b,
            Err(_) => return error::IO,
        };
        let pabgh: Vec<u8> = match std::fs::read(pabgh_str) {
            Ok(b) => b,
            Err(_) => return error::IO,
        };
        let handle = CrimsonDyeColorGroupInfoHandle::from_bytes(&pabgb, &pabgh);
        unsafe { *out_handle = Box::into_raw(Box::new(handle)) };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Parse bytes already in memory.
///
/// # Safety
/// `pabgb` / `pabgh` may be null iff the corresponding `_len` is 0.
/// `out_handle` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_dye_color_group_info_load_from_bytes(
    pabgb: *const u8,
    pabgb_len: usize,
    pabgh: *const u8,
    pabgh_len: usize,
    out_handle: *mut *mut CrimsonDyeColorGroupInfoHandle,
) -> i32 {
    if out_handle.is_null() {
        return error::NULL_ARG;
    }
    if (pabgb.is_null() && pabgb_len != 0) || (pabgh.is_null() && pabgh_len != 0) {
        return error::NULL_ARG;
    }
    unsafe { *out_handle = std::ptr::null_mut() };
    catch_unwind(AssertUnwindSafe(|| {
        let pabgb_slice: &[u8] = if pabgb_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(pabgb, pabgb_len) }
        };
        let pabgh_slice: &[u8] = if pabgh_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(pabgh, pabgh_len) }
        };
        let handle = CrimsonDyeColorGroupInfoHandle::from_bytes(pabgb_slice, pabgh_slice);
        unsafe { *out_handle = Box::into_raw(Box::new(handle)) };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Free a handle returned by either loader.
///
/// # Safety
/// `handle` must be null or a pointer previously returned by one of the
/// loaders and not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_dye_color_group_info_free(
    handle: *mut CrimsonDyeColorGroupInfoHandle,
) {
    if handle.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(handle);
    }
}

// ── Scalar getters ─────────────────────────────────────────────────────────

/// Number of color-group rows in the table.
///
/// # Safety
/// `handle` and `out_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_dye_color_group_info_entry_count(
    handle: *const CrimsonDyeColorGroupInfoHandle,
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

/// Look up the internal name for a given `DyeColorGroupInfoKey (u32)`.
/// Writes the UTF-8 result (NUL-terminated) into `buf`; two-call pattern.
///
/// # Safety
/// `handle` and `required` must be non-null; `buf` may be null iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_dye_color_group_info_lookup_name(
    handle: *const CrimsonDyeColorGroupInfoHandle,
    color_group_key: u32,
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
        let Some(name) = h.by_key.get(&color_group_key) else {
            return error::NOT_FOUND;
        };
        write_str_to_buf(name, buf, buf_len, required)
    }))
    .unwrap_or(error::PANIC)
}

// ── Enumeration ────────────────────────────────────────────────────────────

/// Get the `(key, internal_name)` pair at insertion index `idx`. Index
/// matches PABGH on-disk order.
///
/// # Safety
/// `handle`, `out_key`, `required` must be non-null; `buf` may be null
/// iff `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_dye_color_group_info_get_entry(
    handle: *const CrimsonDyeColorGroupInfoHandle,
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
    //! Live-install integration test. Pins all 10 (key → name) mappings
    //! from the 1.07 install and exercises the lookup + enumeration +
    //! NULL-arg paths.

    use super::*;
    use std::ffi::CString;
    use std::path::PathBuf;
    use std::ptr;

    /// (key, expected internal name) — all 10 rows from the live 1.07
    /// install, same set as the parser test pins.
    const KNOWN: &[(u32, &str)] = &[
        (0xc88211f5, "Her_Color_Group_I"),
        (0xdc274476, "Dem_Color_Group_I"),
        (0x068f0cce, "Dem_Color_Group_II"),
        (0x40707e94, "Dem_Color_Group_III"),
        (0x001835e0, "Kwe_Color_Group_I"),
        (0xa7ec4d9b, "Del_Color_Group_I"),
        (0x2d0517c9, "Cal_Color_Group_I"),
        (0x2a85f874, "Por_Color_Group_I"),
        (0x4f40e9d2, "Tom_Color_Group_I"),
        (0x47564f94, "Bar_Color_Group_I"),
    ];

    fn extract_pair() -> Option<(Vec<u8>, Vec<u8>)> {
        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let pamt_path = game_root.join("0008").join("0.pamt");
        if !pamt_path.is_file() {
            return None;
        }
        let pamt_data = std::fs::read(&pamt_path).ok()?;
        let pamt = crate::binary::pamt::PackMeta::parse(&pamt_data, None).ok()?;
        let dir = pamt
            .directories
            .iter()
            .find(|d| d.path == "gamedata/binary__/client/bin")?;
        let pabgb_file = dir.files.iter().find(|f| f.name == "dyecolorgroupinfo.pabgb")?;
        let pabgh_file = dir.files.iter().find(|f| f.name == "dyecolorgroupinfo.pabgh")?;
        let group_dir = game_root.join("0008");
        let pabgb = crate::binary::paz::extract_file(
            &group_dir,
            pabgb_file,
            "gamedata/binary__/client/bin",
            &pamt.header.encrypt_info.encrypt_info,
        )
        .ok()?;
        let pabgh = crate::binary::paz::extract_file(
            &group_dir,
            pabgh_file,
            "gamedata/binary__/client/bin",
            &pamt.header.encrypt_info.encrypt_info,
        )
        .ok()?;
        Some((pabgb, pabgh))
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
    fn c_abi_dye_color_group_info_live() {
        let Some((pabgb, pabgh)) = extract_pair() else {
            eprintln!("skipping c_abi_dye_color_group_info_live: no game install");
            return;
        };
        let mut h: *mut CrimsonDyeColorGroupInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_dye_color_group_info_load_from_bytes(
                pabgb.as_ptr(),
                pabgb.len(),
                pabgh.as_ptr(),
                pabgh.len(),
                &mut h,
            )
        };
        assert_eq!(rc, error::OK);
        assert!(!h.is_null());

        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_dye_color_group_info_entry_count(h, &mut count) },
            error::OK
        );
        assert!(count >= 5, "expected >=5 dye color groups, got {count}");

        for &(key, expected) in KNOWN {
            let mut req: usize = 0;
            let rc_size = unsafe {
                crimson_dye_color_group_info_lookup_name(
                    h, key, ptr::null_mut(), 0, &mut req,
                )
            };
            let got = read_string_result(rc_size, req, |b, n, r| unsafe {
                crimson_dye_color_group_info_lookup_name(h, key, b, n, r)
            });
            assert_eq!(got, expected, "DyeColorGroupInfoKey 0x{key:08x} mismatch");
        }

        // Negative — unknown key.
        let mut req: usize = 0;
        let rc = unsafe {
            crimson_dye_color_group_info_lookup_name(
                h,
                u32::MAX,
                ptr::null_mut(),
                0,
                &mut req,
            )
        };
        assert_eq!(rc, error::NOT_FOUND);

        // Enumeration round-trips through the entries vector.
        for i in 0..count {
            let mut key: u32 = 0;
            let mut req: usize = 0;
            let rc_size = unsafe {
                crimson_dye_color_group_info_get_entry(
                    h, i, &mut key, ptr::null_mut(), 0, &mut req,
                )
            };
            let name = read_string_result(rc_size, req, |b, n, r| unsafe {
                crimson_dye_color_group_info_get_entry(h, i, &mut key, b, n, r)
            });
            assert!(!name.is_empty(), "entry {i} has empty name");
            assert_ne!(key, 0, "entry {i} has zero key");
        }

        // OUT_OF_RANGE past the end.
        let mut k: u32 = 0;
        let mut r: usize = 0;
        assert_eq!(
            unsafe {
                crimson_dye_color_group_info_get_entry(
                    h, count, &mut k, ptr::null_mut(), 0, &mut r,
                )
            },
            error::OUT_OF_RANGE,
        );

        unsafe { crimson_dye_color_group_info_free(h) };
    }

    #[test]
    fn c_abi_dye_color_group_info_null_args() {
        let mut h: *mut CrimsonDyeColorGroupInfoHandle = ptr::null_mut();
        assert_eq!(
            unsafe {
                crimson_dye_color_group_info_load_from_bytes(
                    ptr::null(),
                    16,
                    ptr::null(),
                    0,
                    &mut h,
                )
            },
            error::NULL_ARG,
        );
        assert_eq!(
            unsafe {
                crimson_dye_color_group_info_load_from_bytes(
                    [0u8; 1].as_ptr(),
                    1,
                    [0u8; 1].as_ptr(),
                    1,
                    ptr::null_mut(),
                )
            },
            error::NULL_ARG,
        );
        let mut req: usize = 0;
        assert_eq!(
            unsafe {
                crimson_dye_color_group_info_lookup_name(
                    ptr::null(),
                    0,
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            },
            error::NULL_ARG,
        );
    }

    #[test]
    fn c_abi_dye_color_group_info_empty_bytes_yields_empty_handle() {
        let mut h: *mut CrimsonDyeColorGroupInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_dye_color_group_info_load_from_bytes(
                ptr::null(),
                0,
                ptr::null(),
                0,
                &mut h,
            )
        };
        assert_eq!(rc, error::OK);
        assert!(!h.is_null());
        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_dye_color_group_info_entry_count(h, &mut count) },
            error::OK
        );
        assert_eq!(count, 0);
        unsafe { crimson_dye_color_group_info_free(h) };
    }

    #[test]
    fn c_abi_dye_color_group_info_load_bad_path_returns_io() {
        let bad = CString::new("Z:\\definitely\\does\\not\\exist.pabgb").unwrap();
        let bad2 = CString::new("Z:\\definitely\\does\\not\\exist.pabgh").unwrap();
        let mut h: *mut CrimsonDyeColorGroupInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_dye_color_group_info_load_from_file(
                bad.as_ptr(),
                bad2.as_ptr(),
                &mut h,
            )
        };
        assert_eq!(rc, error::IO);
        assert!(h.is_null());
    }
}
