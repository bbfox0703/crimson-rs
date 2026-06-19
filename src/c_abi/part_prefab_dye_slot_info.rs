//! `partprefabdyeslotinfo.pabgb` bridge — C ABI surface.
//!
//! Resolves `PartPrefabKey (u32)` — gamedata key identifying a
//! dyeable prefab (an item's wearable mesh). Replaces the PyQt5
//! reference editor's hand-maintained `dye_slot_counts.json`:
//!
//! - `lookup_slot_count(prefab_key)` — the count the JSON used to carry.
//! - `lookup_prefab_name(prefab_key)` — the prefab internal name.
//! - `lookup_slot_default_material(prefab_key, slot_idx, mat_idx)` —
//!   the per-slot default material that the editor can pre-populate
//!   in its dye dropdowns (richer than the JSON ever was).
//! - `lookup_slot_mat_indices` / `lookup_slot_mask` — raw 3-byte
//!   arrays for advanced UI / debugging.
//! - `lookup_slot_tail_name` — next sub-prefab name (or, for the
//!   LAST slot in a row, the full `.pac` asset path).
//!
//! The cross-reference between an `_itemKey` and its `PartPrefabKey`
//! is NOT in this file — it lives in `iteminfo.pabgb` (or a sibling
//! `partprefab*` table not yet RE'd). That linkage is the next-session
//! blocker for the C# editor to actually consume these slot counts.
//!
//! See [`crate::part_prefab_dye_slot_info`] for the row + slot schema.

use std::collections::HashMap;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::error;
use crate::part_prefab_dye_slot_info::{
    PartPrefabDyeSlotInfoEntry, parse_part_prefab_dye_slot_info_lossy,
};

/// Opaque handle.
#[repr(C)]
pub struct CrimsonPartPrefabDyeSlotInfoHandle {
    by_key: HashMap<u32, PartPrefabDyeSlotInfoEntry>,
    entries: Vec<PartPrefabDyeSlotInfoEntry>,
}

impl CrimsonPartPrefabDyeSlotInfoHandle {
    fn from_bytes(pabgb: &[u8], pabgh: &[u8]) -> Self {
        let raw = parse_part_prefab_dye_slot_info_lossy(pabgb, pabgh);
        let mut by_key: HashMap<u32, PartPrefabDyeSlotInfoEntry> =
            HashMap::with_capacity(raw.len());
        let mut entries: Vec<PartPrefabDyeSlotInfoEntry> = Vec::with_capacity(raw.len());
        for e in raw {
            if let std::collections::hash_map::Entry::Vacant(v) = by_key.entry(e.key) {
                v.insert(e.clone());
                entries.push(e);
            }
        }
        CrimsonPartPrefabDyeSlotInfoHandle { by_key, entries }
    }
}

// ── Load / free ────────────────────────────────────────────────────────────

/// Parse from disk.
///
/// # Safety
/// Both path arguments must be NUL-terminated UTF-8. `out_handle` must
/// be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_part_prefab_dye_slot_info_load_from_file(
    pabgb_path: *const c_char,
    pabgh_path: *const c_char,
    out_handle: *mut *mut CrimsonPartPrefabDyeSlotInfoHandle,
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
        let handle = CrimsonPartPrefabDyeSlotInfoHandle::from_bytes(&pabgb, &pabgh);
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
pub unsafe extern "C" fn crimson_part_prefab_dye_slot_info_load_from_bytes(
    pabgb: *const u8,
    pabgb_len: usize,
    pabgh: *const u8,
    pabgh_len: usize,
    out_handle: *mut *mut CrimsonPartPrefabDyeSlotInfoHandle,
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
        let handle = CrimsonPartPrefabDyeSlotInfoHandle::from_bytes(
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
pub unsafe extern "C" fn crimson_part_prefab_dye_slot_info_free(
    handle: *mut CrimsonPartPrefabDyeSlotInfoHandle,
) {
    if handle.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(handle);
    }
}

// ── Scalar getters ─────────────────────────────────────────────────────────

/// Total number of prefab rows.
///
/// # Safety
/// `handle` and `out_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_part_prefab_dye_slot_info_entry_count(
    handle: *const CrimsonPartPrefabDyeSlotInfoHandle,
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

/// Number of dye slots on the prefab identified by `prefab_key`.
/// **Direct replacement for `dye_slot_counts.json[item_id]`.**
///
/// # Safety
/// `handle` and `out_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_part_prefab_dye_slot_info_lookup_slot_count(
    handle: *const CrimsonPartPrefabDyeSlotInfoHandle,
    prefab_key: u32,
    out_count: *mut u32,
) -> i32 {
    if handle.is_null() || out_count.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_count = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some(row) = h.by_key.get(&prefab_key) else {
            return error::NOT_FOUND;
        };
        unsafe { *out_count = row.slot_count() };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Prefab internal name (e.g. `"cd_phm_00_lb_00_0054"`).
///
/// # Safety
/// `handle` and `required` must be non-null; `buf` may be null iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_part_prefab_dye_slot_info_lookup_prefab_name(
    handle: *const CrimsonPartPrefabDyeSlotInfoHandle,
    prefab_key: u32,
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
        let Some(row) = h.by_key.get(&prefab_key) else {
            return error::NOT_FOUND;
        };
        write_str_to_buf(&row.prefab_name, buf, buf_len, required)
    }))
    .unwrap_or(error::PANIC)
}

// ── Per-slot getters ───────────────────────────────────────────────────────

/// Default material name for `slot_idx`'s `mat_idx` channel
/// (`mat_idx ∈ {0, 1, 2}`). Often empty; common non-empty values are
/// `"cloth"` / `"leather"` / `"metal"`.
///
/// # Safety
/// `handle` and `required` must be non-null; `buf` may be null iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_part_prefab_dye_slot_info_lookup_slot_default_material(
    handle: *const CrimsonPartPrefabDyeSlotInfoHandle,
    prefab_key: u32,
    slot_idx: u32,
    mat_idx: u32,
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
        let Some(row) = h.by_key.get(&prefab_key) else {
            return error::NOT_FOUND;
        };
        let Some(slot) = row.slots.get(slot_idx as usize) else {
            return error::OUT_OF_RANGE;
        };
        if mat_idx >= 3 {
            return error::OUT_OF_RANGE;
        }
        let src = slot.default_materials[mat_idx as usize].as_str();
        write_str_to_buf(src, buf, buf_len, required)
    }))
    .unwrap_or(error::PANIC)
}

/// Tail name — next sub-prefab for non-final slots, full `.pac` path
/// for the last slot.
///
/// # Safety
/// `handle` and `required` must be non-null; `buf` may be null iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_part_prefab_dye_slot_info_lookup_slot_tail_name(
    handle: *const CrimsonPartPrefabDyeSlotInfoHandle,
    prefab_key: u32,
    slot_idx: u32,
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
        let Some(row) = h.by_key.get(&prefab_key) else {
            return error::NOT_FOUND;
        };
        let Some(slot) = row.slots.get(slot_idx as usize) else {
            return error::OUT_OF_RANGE;
        };
        write_str_to_buf(&slot.tail_name, buf, buf_len, required)
    }))
    .unwrap_or(error::PANIC)
}

/// Read the 3 material-index bytes into `out_indices[0..3]`.
///
/// # Safety
/// `handle` and `out_indices` must be non-null; `out_indices` must
/// point to 3 writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_part_prefab_dye_slot_info_lookup_slot_mat_indices(
    handle: *const CrimsonPartPrefabDyeSlotInfoHandle,
    prefab_key: u32,
    slot_idx: u32,
    out_indices: *mut u8,
) -> i32 {
    if handle.is_null() || out_indices.is_null() {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some(row) = h.by_key.get(&prefab_key) else {
            return error::NOT_FOUND;
        };
        let Some(slot) = row.slots.get(slot_idx as usize) else {
            return error::OUT_OF_RANGE;
        };
        unsafe {
            std::ptr::copy_nonoverlapping(slot.mat_indices.as_ptr(), out_indices, 3);
        }
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Read the 3 mask bytes into `out_mask[0..3]`.
///
/// # Safety
/// `handle` and `out_mask` must be non-null; `out_mask` must point to
/// 3 writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_part_prefab_dye_slot_info_lookup_slot_mask(
    handle: *const CrimsonPartPrefabDyeSlotInfoHandle,
    prefab_key: u32,
    slot_idx: u32,
    out_mask: *mut u8,
) -> i32 {
    if handle.is_null() || out_mask.is_null() {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some(row) = h.by_key.get(&prefab_key) else {
            return error::NOT_FOUND;
        };
        let Some(slot) = row.slots.get(slot_idx as usize) else {
            return error::OUT_OF_RANGE;
        };
        unsafe {
            std::ptr::copy_nonoverlapping(slot.mask.as_ptr(), out_mask, 3);
        }
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

// ── Enumeration ────────────────────────────────────────────────────────────

/// Read the prefab key at insertion index `idx` (PABGH on-disk order).
///
/// # Safety
/// `handle` and `out_key` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_part_prefab_dye_slot_info_get_entry_key(
    handle: *const CrimsonPartPrefabDyeSlotInfoHandle,
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
    //! Live-install integration test. Pins the row count and a handful
    //! of (key, prefab_name, slot_count) tuples; exercises per-slot
    //! getters on the multi-slot known row.

    use super::*;
    use std::path::PathBuf;
    use std::ptr;

    /// (key, expected prefab_name, expected slot_count). Verified on the
    /// live 1.12 install (2026-06-19). First three carry over unchanged
    /// from the 1.07 probe pass; the last three replace 1.07 keys that
    /// 1.12 removed (the −143-row drop, 1,111 → 968).
    const KNOWN: &[(u32, &str, u32)] = &[
        (0xc7bbaada, "cd_phm_00_lb_00_0054", 1),
        (0xfbad5654, "cd_phm_00_hel_0057_02_inside", 2),
        (0xddb61e2e, "cd_phm_00_vest_0051_01", 4),
        (0x4905cceb, "cd_phm_00_lb_0002", 1),
        (0xf8042604, "cd_phm_00_lb_00_0342_belt", 3),
        (0xd5394f4c, "cd_phm_00_hand_belt_0245_01", 5),
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
        let pabgb_file = dir
            .files
            .iter()
            .find(|f| f.name == "partprefabdyeslotinfo.pabgb")?;
        let pabgh_file = dir
            .files
            .iter()
            .find(|f| f.name == "partprefabdyeslotinfo.pabgh")?;
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
    fn c_abi_part_prefab_dye_slot_info_live() {
        let Some((pabgb, pabgh)) = extract_pair() else {
            eprintln!("skipping c_abi_part_prefab_dye_slot_info_live: no game install");
            return;
        };
        let mut h: *mut CrimsonPartPrefabDyeSlotInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_part_prefab_dye_slot_info_load_from_bytes(
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
            unsafe { crimson_part_prefab_dye_slot_info_entry_count(h, &mut count) },
            error::OK
        );
        assert!(count >= 900, "expected >=900 prefabs, got {count}");

        for &(key, expected_name, expected_count) in KNOWN {
            // slot_count check.
            let mut got_count: u32 = 0;
            assert_eq!(
                unsafe {
                    crimson_part_prefab_dye_slot_info_lookup_slot_count(
                        h,
                        key,
                        &mut got_count,
                    )
                },
                error::OK,
                "lookup_slot_count(0x{key:08x})",
            );
            assert_eq!(got_count, expected_count, "key 0x{key:08x} slot_count");

            // prefab_name check.
            let mut req: usize = 0;
            let rc_size = unsafe {
                crimson_part_prefab_dye_slot_info_lookup_prefab_name(
                    h,
                    key,
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            };
            let got_name = read_string_result(rc_size, req, |b, n, r| unsafe {
                crimson_part_prefab_dye_slot_info_lookup_prefab_name(h, key, b, n, r)
            });
            assert_eq!(got_name, expected_name, "key 0x{key:08x} prefab_name");
        }

        // Per-slot detail on the known multi-slot vest row.
        let vest = 0xddb61e2eu32; // slot_count=4
        // Slot 0's tail_name should be a CString — could be empty or a
        // next-prefab name. Slot 3 (last) should end with ".pac".
        let mut req: usize = 0;
        let rc_size = unsafe {
            crimson_part_prefab_dye_slot_info_lookup_slot_tail_name(
                h,
                vest,
                3,
                ptr::null_mut(),
                0,
                &mut req,
            )
        };
        let tail = read_string_result(rc_size, req, |b, n, r| unsafe {
            crimson_part_prefab_dye_slot_info_lookup_slot_tail_name(h, vest, 3, b, n, r)
        });
        assert!(
            tail.ends_with(".pac"),
            "vest row last slot tail should end in .pac, got {tail:?}",
        );

        // At least one of the vest row's slots has a known material name.
        let mut any_known = false;
        for slot_idx in 0..4 {
            for mat_idx in 0..3 {
                let mut req: usize = 0;
                let rc_size = unsafe {
                    crimson_part_prefab_dye_slot_info_lookup_slot_default_material(
                        h,
                        vest,
                        slot_idx,
                        mat_idx,
                        ptr::null_mut(),
                        0,
                        &mut req,
                    )
                };
                // Empty material names round-trip cleanly (required=1 for the NUL).
                if req <= 1 {
                    continue;
                }
                let mat = read_string_result(rc_size, req, |b, n, r| unsafe {
                    crimson_part_prefab_dye_slot_info_lookup_slot_default_material(
                        h, vest, slot_idx, mat_idx, b, n, r,
                    )
                });
                if matches!(mat.as_str(), "cloth" | "leather" | "metal") {
                    any_known = true;
                    break;
                }
            }
        }
        assert!(any_known, "vest row should expose at least one cloth/leather/metal default material");

        // mat_indices + mask checks.
        let mut idxs = [0u8; 3];
        assert_eq!(
            unsafe {
                crimson_part_prefab_dye_slot_info_lookup_slot_mat_indices(
                    h,
                    vest,
                    0,
                    idxs.as_mut_ptr(),
                )
            },
            error::OK
        );
        let mut mask = [0u8; 3];
        assert_eq!(
            unsafe {
                crimson_part_prefab_dye_slot_info_lookup_slot_mask(
                    h,
                    vest,
                    0,
                    mask.as_mut_ptr(),
                )
            },
            error::OK
        );

        // Negatives.
        let mut c: u32 = 0;
        assert_eq!(
            unsafe {
                crimson_part_prefab_dye_slot_info_lookup_slot_count(h, u32::MAX, &mut c)
            },
            error::NOT_FOUND
        );
        let mut idxs2 = [0u8; 3];
        assert_eq!(
            unsafe {
                crimson_part_prefab_dye_slot_info_lookup_slot_mat_indices(
                    h,
                    vest,
                    99,
                    idxs2.as_mut_ptr(),
                )
            },
            error::OUT_OF_RANGE
        );

        // mat_idx ≥ 3 is OUT_OF_RANGE.
        let mut req: usize = 0;
        assert_eq!(
            unsafe {
                crimson_part_prefab_dye_slot_info_lookup_slot_default_material(
                    h,
                    vest,
                    0,
                    3,
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            },
            error::OUT_OF_RANGE
        );

        unsafe { crimson_part_prefab_dye_slot_info_free(h) };
    }

    #[test]
    fn c_abi_part_prefab_dye_slot_info_null_args() {
        let mut h: *mut CrimsonPartPrefabDyeSlotInfoHandle = ptr::null_mut();
        assert_eq!(
            unsafe {
                crimson_part_prefab_dye_slot_info_load_from_bytes(
                    ptr::null(),
                    16,
                    ptr::null(),
                    0,
                    &mut h,
                )
            },
            error::NULL_ARG,
        );
        let mut c: u32 = 0;
        assert_eq!(
            unsafe {
                crimson_part_prefab_dye_slot_info_lookup_slot_count(
                    ptr::null(),
                    0,
                    &mut c,
                )
            },
            error::NULL_ARG,
        );
    }

    #[test]
    fn c_abi_part_prefab_dye_slot_info_empty_bytes_yields_empty_handle() {
        let mut h: *mut CrimsonPartPrefabDyeSlotInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_part_prefab_dye_slot_info_load_from_bytes(
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
            unsafe { crimson_part_prefab_dye_slot_info_entry_count(h, &mut count) },
            error::OK
        );
        assert_eq!(count, 0);
        unsafe { crimson_part_prefab_dye_slot_info_free(h) };
    }
}
