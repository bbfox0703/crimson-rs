//! `partprefabdyetexturepalleteinfo.pabgb` bridge — C ABI surface.
//!
//! Resolves `PartPrefabDyeTexturePalleteKey (u16)` — the gamedata
//! key stored as `ItemDyeSaveData._texturePalleteKey` in the save's
//! `_itemDyeDataList`. 11 rows in 1.07 (key 0..=10). Each row defines
//! a "palette tier" — a set of 2 or 3 material sub-records (cloth /
//! leather / metal) with paired icon + texture DDS paths.
//!
//! Lookup keys here are exposed as `u32` for cross-bridge consistency
//! (other bridges all use u32) — the high bits are always zero and
//! the save field is a literal u16.
//!
//! Per-field getters (one call per scalar / string) mirror the verbose
//! style used by sibling bridges (`c_abi/gimmick_info.rs`, etc.). For
//! a richer dropdown UI, the editor calls
//! `lookup_sub_count(palette_key)` then iterates over each sub-index
//! with `lookup_sub_material_name`, `lookup_sub_icon_path`,
//! `lookup_sub_texture_path`, `lookup_sub_variant_name`, and
//! `lookup_sub_variant_value`.
//!
//! See [`crate::part_prefab_dye_texture_pallete_info`] for the row + sub schema.

use std::collections::HashMap;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::error;
use crate::part_prefab_dye_texture_pallete_info::{
    PartPrefabDyeTexturePalleteEntry, parse_part_prefab_dye_texture_pallete_info_lossy,
};

/// Opaque handle exposing palette-row lookups against the loaded
/// `partprefabdyetexturepalleteinfo.pabgb` + `.pabgh`.
#[repr(C)]
pub struct CrimsonPartPrefabDyeTexturePalleteHandle {
    by_key: HashMap<u32, PartPrefabDyeTexturePalleteEntry>,
    entries: Vec<PartPrefabDyeTexturePalleteEntry>,
}

impl CrimsonPartPrefabDyeTexturePalleteHandle {
    fn from_bytes(pabgb: &[u8], pabgh: &[u8]) -> Self {
        let raw = parse_part_prefab_dye_texture_pallete_info_lossy(pabgb, pabgh);
        let mut by_key: HashMap<u32, PartPrefabDyeTexturePalleteEntry> =
            HashMap::with_capacity(raw.len());
        let mut entries: Vec<PartPrefabDyeTexturePalleteEntry> =
            Vec::with_capacity(raw.len());
        for e in raw {
            if let std::collections::hash_map::Entry::Vacant(v) = by_key.entry(e.key) {
                v.insert(e.clone());
                entries.push(e);
            }
        }
        CrimsonPartPrefabDyeTexturePalleteHandle { by_key, entries }
    }
}

// ── Load / free ────────────────────────────────────────────────────────────

/// Parse the pair from disk.
///
/// # Safety
/// `pabgb_path` / `pabgh_path` must be NUL-terminated UTF-8 strings.
/// `out_handle` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_part_prefab_dye_texture_pallete_load_from_file(
    pabgb_path: *const c_char,
    pabgh_path: *const c_char,
    out_handle: *mut *mut CrimsonPartPrefabDyeTexturePalleteHandle,
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
        let handle = CrimsonPartPrefabDyeTexturePalleteHandle::from_bytes(&pabgb, &pabgh);
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
pub unsafe extern "C" fn crimson_part_prefab_dye_texture_pallete_load_from_bytes(
    pabgb: *const u8,
    pabgb_len: usize,
    pabgh: *const u8,
    pabgh_len: usize,
    out_handle: *mut *mut CrimsonPartPrefabDyeTexturePalleteHandle,
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
        let handle = CrimsonPartPrefabDyeTexturePalleteHandle::from_bytes(
            pabgb_slice,
            pabgh_slice,
        );
        unsafe { *out_handle = Box::into_raw(Box::new(handle)) };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Free a handle.
///
/// # Safety
/// `handle` must be null or a pointer previously returned by one of the
/// loaders and not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_part_prefab_dye_texture_pallete_free(
    handle: *mut CrimsonPartPrefabDyeTexturePalleteHandle,
) {
    if handle.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(handle);
    }
}

// ── Scalar getters ─────────────────────────────────────────────────────────

/// Total number of palette rows in the table.
///
/// # Safety
/// `handle` and `out_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_part_prefab_dye_texture_pallete_entry_count(
    handle: *const CrimsonPartPrefabDyeTexturePalleteHandle,
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

/// Number of sub-records inside the palette row keyed by `palette_key`.
///
/// # Safety
/// `handle` and `out_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_part_prefab_dye_texture_pallete_lookup_sub_count(
    handle: *const CrimsonPartPrefabDyeTexturePalleteHandle,
    palette_key: u32,
    out_count: *mut u32,
) -> i32 {
    if handle.is_null() || out_count.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_count = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some(row) = h.by_key.get(&palette_key) else {
            return error::NOT_FOUND;
        };
        unsafe { *out_count = row.subs.len() as u32 };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

// ── Per-field string getters ───────────────────────────────────────────────

/// Internal field selector for `lookup_sub_string`. Kept private —
/// callers go through the named per-field public entry points below
/// so the C# bindings get a stable, well-typed surface.
#[derive(Copy, Clone)]
enum SubStringField {
    MaterialName,
    IconPath,
    TexturePath,
    VariantName,
}

fn lookup_sub_string(
    handle: *const CrimsonPartPrefabDyeTexturePalleteHandle,
    palette_key: u32,
    sub_idx: u32,
    field: SubStringField,
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
        let Some(row) = h.by_key.get(&palette_key) else {
            return error::NOT_FOUND;
        };
        let Some(sub) = row.subs.get(sub_idx as usize) else {
            return error::OUT_OF_RANGE;
        };
        let src = match field {
            SubStringField::MaterialName => sub.material_name.as_str(),
            SubStringField::IconPath => sub.icon_path.as_str(),
            SubStringField::TexturePath => sub.texture_path.as_str(),
            SubStringField::VariantName => sub.variant_name.as_str(),
        };
        write_str_to_buf(src, buf, buf_len, required)
    }))
    .unwrap_or(error::PANIC)
}

/// Material identifier — `"cloth"`, `"leather"`, `"metal"`, …
///
/// # Safety
/// `handle` and `required` must be non-null; `buf` may be null iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_part_prefab_dye_texture_pallete_lookup_sub_material_name(
    handle: *const CrimsonPartPrefabDyeTexturePalleteHandle,
    palette_key: u32,
    sub_idx: u32,
    buf: *mut u8,
    buf_len: usize,
    required: *mut usize,
) -> i32 {
    lookup_sub_string(
        handle,
        palette_key,
        sub_idx,
        SubStringField::MaterialName,
        buf,
        buf_len,
        required,
    )
}

/// UI icon DDS path (falls back to texture path for `palette_key=0`).
///
/// # Safety
/// `handle` and `required` must be non-null; `buf` may be null iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_part_prefab_dye_texture_pallete_lookup_sub_icon_path(
    handle: *const CrimsonPartPrefabDyeTexturePalleteHandle,
    palette_key: u32,
    sub_idx: u32,
    buf: *mut u8,
    buf_len: usize,
    required: *mut usize,
) -> i32 {
    lookup_sub_string(
        handle,
        palette_key,
        sub_idx,
        SubStringField::IconPath,
        buf,
        buf_len,
        required,
    )
}

/// Material texture DDS path used at runtime.
///
/// # Safety
/// `handle` and `required` must be non-null; `buf` may be null iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_part_prefab_dye_texture_pallete_lookup_sub_texture_path(
    handle: *const CrimsonPartPrefabDyeTexturePalleteHandle,
    palette_key: u32,
    sub_idx: u32,
    buf: *mut u8,
    buf_len: usize,
    required: *mut usize,
) -> i32 {
    lookup_sub_string(
        handle,
        palette_key,
        sub_idx,
        SubStringField::TexturePath,
        buf,
        buf_len,
        required,
    )
}

/// Variant label inside a material — empty when absent, or
/// `"wool"` / `"velvet"` / `"silk"`.
///
/// # Safety
/// `handle` and `required` must be non-null; `buf` may be null iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_part_prefab_dye_texture_pallete_lookup_sub_variant_name(
    handle: *const CrimsonPartPrefabDyeTexturePalleteHandle,
    palette_key: u32,
    sub_idx: u32,
    buf: *mut u8,
    buf_len: usize,
    required: *mut usize,
) -> i32 {
    lookup_sub_string(
        handle,
        palette_key,
        sub_idx,
        SubStringField::VariantName,
        buf,
        buf_len,
        required,
    )
}

/// Variant strength — `-1.0` is the "no variant" sentinel.
///
/// # Safety
/// `handle` and `out_value` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_part_prefab_dye_texture_pallete_lookup_sub_variant_value(
    handle: *const CrimsonPartPrefabDyeTexturePalleteHandle,
    palette_key: u32,
    sub_idx: u32,
    out_value: *mut f32,
) -> i32 {
    if handle.is_null() || out_value.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_value = 0.0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some(row) = h.by_key.get(&palette_key) else {
            return error::NOT_FOUND;
        };
        let Some(sub) = row.subs.get(sub_idx as usize) else {
            return error::OUT_OF_RANGE;
        };
        unsafe { *out_value = sub.variant_value };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

// ── Enumeration ────────────────────────────────────────────────────────────

/// Read the palette key at insertion index `idx` (PABGH on-disk order).
///
/// # Safety
/// `handle` and `out_key` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_part_prefab_dye_texture_pallete_get_entry_key(
    handle: *const CrimsonPartPrefabDyeTexturePalleteHandle,
    idx: u32,
    out_key: *mut u32,
) -> i32 {
    if handle.is_null() || out_key.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_key = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some(entry) = h.entries.get(idx as usize) else {
            return error::OUT_OF_RANGE;
        };
        unsafe { *out_key = entry.key };
        error::OK
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
    //! Live-install integration test. Pins the 11-row count, the
    //! per-row sub_count invariants (key=0 → 2; key=1..=10 → 3), and
    //! the cloth/wool variant on key=1 sub=0 (positive variant_value).

    use crate::binary::gamedata_layout;
    use super::*;
    use std::path::PathBuf;
    use std::ptr;

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
            .find(|d| d.path == gamedata_layout::bin_dir())?;
        let pabgb_file = dir
            .files
            .iter()
            .find(|f| f.name == gamedata_layout::body("partprefabdyetexturepalleteinfo"))?;
        let pabgh_file = dir
            .files
            .iter()
            .find(|f| f.name == gamedata_layout::header("partprefabdyetexturepalleteinfo"))?;
        let group_dir = game_root.join("0008");
        let pabgb = crate::binary::paz::extract_file(
            &group_dir,
            pabgb_file,
            gamedata_layout::bin_dir(),
            &pamt.header.encrypt_info.encrypt_info,
        )
        .ok()?;
        let pabgh = crate::binary::paz::extract_file(
            &group_dir,
            pabgh_file,
            gamedata_layout::bin_dir(),
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
    fn c_abi_part_prefab_dye_texture_pallete_live() {
        let Some((pabgb, pabgh)) = extract_pair() else {
            eprintln!("skipping c_abi_part_prefab_dye_texture_pallete_live: no game install");
            return;
        };
        let mut h: *mut CrimsonPartPrefabDyeTexturePalleteHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_part_prefab_dye_texture_pallete_load_from_bytes(
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
            unsafe {
                crimson_part_prefab_dye_texture_pallete_entry_count(h, &mut count)
            },
            error::OK
        );
        assert_eq!(count, 11);

        // sub_count: key=0 → 3 in 1.13 (was 2 ≤1.12); key=1..=10 → 3.
        let mut sub_count: u32 = 0;
        assert_eq!(
            unsafe {
                crimson_part_prefab_dye_texture_pallete_lookup_sub_count(
                    h, 0, &mut sub_count,
                )
            },
            error::OK
        );
        assert_eq!(sub_count, 3);
        for key in 1..=10u32 {
            assert_eq!(
                unsafe {
                    crimson_part_prefab_dye_texture_pallete_lookup_sub_count(
                        h, key, &mut sub_count,
                    )
                },
                error::OK
            );
            assert_eq!(sub_count, 3, "palette key={key} should have 3 subs");
        }

        // Per-field getter on key=1, sub=0: material_name == "cloth",
        // variant_name == "wool", variant_value > 0.
        let mut req: usize = 0;
        let rc_size = unsafe {
            crimson_part_prefab_dye_texture_pallete_lookup_sub_material_name(
                h,
                1,
                0,
                ptr::null_mut(),
                0,
                &mut req,
            )
        };
        let mat = read_string_result(rc_size, req, |b, n, r| unsafe {
            crimson_part_prefab_dye_texture_pallete_lookup_sub_material_name(
                h, 1, 0, b, n, r,
            )
        });
        assert_eq!(mat, "cloth");

        let rc_size = unsafe {
            crimson_part_prefab_dye_texture_pallete_lookup_sub_variant_name(
                h,
                1,
                0,
                ptr::null_mut(),
                0,
                &mut req,
            )
        };
        let var_name = read_string_result(rc_size, req, |b, n, r| unsafe {
            crimson_part_prefab_dye_texture_pallete_lookup_sub_variant_name(
                h, 1, 0, b, n, r,
            )
        });
        assert_eq!(var_name, "wool");

        let mut var_value: f32 = 0.0;
        assert_eq!(
            unsafe {
                crimson_part_prefab_dye_texture_pallete_lookup_sub_variant_value(
                    h,
                    1,
                    0,
                    &mut var_value,
                )
            },
            error::OK
        );
        assert!(
            var_value > 0.0 && var_value < 1.0,
            "expected (0,1), got {var_value}",
        );

        // key=1 sub=1 (leather) has no variant — variant_value ~-1.0.
        assert_eq!(
            unsafe {
                crimson_part_prefab_dye_texture_pallete_lookup_sub_variant_value(
                    h,
                    1,
                    1,
                    &mut var_value,
                )
            },
            error::OK
        );
        assert!(
            (var_value + 1.0).abs() < 1e-6,
            "expected ~-1.0, got {var_value}",
        );

        // texture_path shape check on key=5 sub=0.
        let rc_size = unsafe {
            crimson_part_prefab_dye_texture_pallete_lookup_sub_texture_path(
                h,
                5,
                0,
                ptr::null_mut(),
                0,
                &mut req,
            )
        };
        let tex = read_string_result(rc_size, req, |b, n, r| unsafe {
            crimson_part_prefab_dye_texture_pallete_lookup_sub_texture_path(
                h, 5, 0, b, n, r,
            )
        });
        assert!(
            tex.starts_with("character/texture/cd_texturelayer_"),
            "unexpected texture path: {tex:?}",
        );
        assert!(tex.ends_with(".dds"));

        // Negative: unknown palette key.
        let mut sub_c: u32 = 0;
        assert_eq!(
            unsafe {
                crimson_part_prefab_dye_texture_pallete_lookup_sub_count(
                    h,
                    9999,
                    &mut sub_c,
                )
            },
            error::NOT_FOUND
        );
        // OUT_OF_RANGE on sub_idx past the end.
        assert_eq!(
            unsafe {
                crimson_part_prefab_dye_texture_pallete_lookup_sub_variant_value(
                    h,
                    1,
                    99,
                    &mut var_value,
                )
            },
            error::OUT_OF_RANGE
        );

        // Enumeration: keys come out as 0..=10.
        for i in 0..count {
            let mut key: u32 = 0;
            assert_eq!(
                unsafe {
                    crimson_part_prefab_dye_texture_pallete_get_entry_key(
                        h, i, &mut key,
                    )
                },
                error::OK
            );
            assert_eq!(key, i, "entry {i} should have key {i}");
        }

        unsafe { crimson_part_prefab_dye_texture_pallete_free(h) };
    }

    #[test]
    fn c_abi_part_prefab_dye_texture_pallete_null_args() {
        let mut h: *mut CrimsonPartPrefabDyeTexturePalleteHandle = ptr::null_mut();
        assert_eq!(
            unsafe {
                crimson_part_prefab_dye_texture_pallete_load_from_bytes(
                    ptr::null(),
                    16,
                    ptr::null(),
                    0,
                    &mut h,
                )
            },
            error::NULL_ARG,
        );
        let mut req: usize = 0;
        assert_eq!(
            unsafe {
                crimson_part_prefab_dye_texture_pallete_lookup_sub_material_name(
                    ptr::null(),
                    0,
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
    fn c_abi_part_prefab_dye_texture_pallete_empty_bytes_yields_empty_handle() {
        let mut h: *mut CrimsonPartPrefabDyeTexturePalleteHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_part_prefab_dye_texture_pallete_load_from_bytes(
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
            unsafe {
                crimson_part_prefab_dye_texture_pallete_entry_count(h, &mut count)
            },
            error::OK
        );
        assert_eq!(count, 0);
        unsafe { crimson_part_prefab_dye_texture_pallete_free(h) };
    }
}
