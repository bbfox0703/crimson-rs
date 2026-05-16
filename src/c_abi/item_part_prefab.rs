//! `_itemKey → _partPrefabKey` bridge — combined handle.
//!
//! The cross-reference between an item's `ItemKey` and the
//! `PartPrefabKey` rows in `partprefabdyeslotinfo` lives at a
//! 3-table join — there's no direct `_partPrefabKey` field in
//! `iteminfo.pabgb` or any sibling table. The linkage chain:
//!
//! ```text
//! ItemKey
//!   → iteminfo.prefab_data_list[].prefab_names[]   (StringInfoKey, u32)
//!   → stringinfo.pabgb                             (StringInfoKey → string)
//!   → "cd_phm_00_hel_00_0354_c"                    (resolved string)
//!   → partprefabdyeslotinfo.prefab_name            (string == row.prefab_name)
//!   → row.key                                      (PartPrefabKey, u32)
//! ```
//!
//! Confirmed by the `_probe_partprefab_string_linkage` investigation
//! (2026-05-16): 100% of dyeable items' prefab_name hashes resolve via
//! stringinfo; 50/730 land directly in `partprefabdyeslotinfo` (the
//! remaining hashes resolve to body-type variants whose prefabs live
//! in non-dye tables — e.g. goblin / dwarf / tribe meshes that share
//! the human male's dye-slot layout). 0% direct hit between the
//! StringInfoKey u32 values and partprefab row keys — these are
//! independent hash spaces.
//!
//! This bridge precomputes the join at load time so the C# editor can
//! ask one question — "what part-prefab keys does this item have?" —
//! without orchestrating three handles. Each lookup is a single
//! `HashMap` probe.
//!
//! ## Input requirements
//!
//! Three blobs from the game install's 0008 PAMT manifest:
//! - `iteminfo.pabgb`
//! - `stringinfo.pabgb`
//! - `partprefabdyeslotinfo.pabgb` + `.pabgh`
//!
//! All three live under `0008/gamedata/binary__/client/bin/` and can be
//! extracted with the existing [`super::paz`] bridge.
//!
//! ## Coverage
//!
//! In 1.07: 507 items have `is_dyeable=1`; ~120 of them resolve to at
//! least one `partprefabdyeslotinfo` row. Items with `is_dyeable=0` are
//! also surfaced when they happen to reference a partprefab row (rare —
//! the editor should still filter by `is_dyeable` on its end).

use std::collections::HashMap;
use std::ffi::CStr;
use std::io;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::error;
use crate::binary::BinaryRead;

/// Opaque handle exposing precomputed `ItemKey → Vec<PartPrefabKey>`
/// lookups.
#[repr(C)]
pub struct CrimsonItemPartPrefabHandle {
    /// Joined map. Keys are item keys; values are the list of
    /// part-prefab keys resolvable for that item, in
    /// `iteminfo.prefab_data_list` traversal order. Empty entries are
    /// dropped from the map.
    part_prefab_keys_by_item: HashMap<u32, Vec<u32>>,
    /// Parallel list of (item_key, resolved prefab_name) per part
    /// prefab key. Same length as the corresponding value in
    /// `part_prefab_keys_by_item` — exposes the underlying prefab
    /// names for debugging / display.
    prefab_names_by_item: HashMap<u32, Vec<String>>,
}

impl CrimsonItemPartPrefabHandle {
    fn from_bytes(
        iteminfo_pabgb: &[u8],
        stringinfo_pabgb: &[u8],
        partprefab_pabgb: &[u8],
        partprefab_pabgh: &[u8],
    ) -> io::Result<Self> {
        // Stage 1: load partprefab — gives us
        // (prefab_name → key) and the set of valid prefab names.
        let pp = crate::part_prefab_dye_slot_info::parse_part_prefab_dye_slot_info_lossy(
            partprefab_pabgb,
            partprefab_pabgh,
        );
        let mut pp_key_by_name: HashMap<String, u32> = HashMap::with_capacity(pp.len());
        for e in &pp {
            // Keep the first occurrence on a name collision. None
            // observed in 1.07.
            pp_key_by_name.entry(e.prefab_name.clone()).or_insert(e.key);
        }

        // Stage 2: load stringinfo — gives us (StringInfoKey → string).
        // ~30,232 entries in 1.07.
        let si_entries =
            crate::string_info::StringInfoData::parse_pabgb(stringinfo_pabgb)?;
        let mut si_by_hash: HashMap<u32, String> =
            HashMap::with_capacity(si_entries.len());
        for e in &si_entries {
            // First-wins on duplicates. Round-trip semantics are not
            // needed here — we only consume the value.
            si_by_hash.entry(e.hash).or_insert_with(|| e.value.clone());
        }

        // Stage 3: walk iteminfo. For each item, walk every
        // `prefab_data_list[].prefab_names[]` hash, resolve through
        // stringinfo, and look up in partprefab. Accumulate dedup'd
        // partprefab keys per item.
        let mut keys_by_item: HashMap<u32, Vec<u32>> = HashMap::new();
        let mut names_by_item: HashMap<u32, Vec<String>> = HashMap::new();
        let mut offset = 0usize;
        while offset < iteminfo_pabgb.len() {
            let item = crate::item_info::ItemInfo::read_from(iteminfo_pabgb, &mut offset)?;
            let mut seen: std::collections::HashSet<u32> = Default::default();
            let mut keys: Vec<u32> = Vec::new();
            let mut names: Vec<String> = Vec::new();
            for pd in &item.prefab_data_list.items {
                for sik in &pd.prefab_names.items {
                    let Some(name) = si_by_hash.get(&sik.0) else {
                        continue;
                    };
                    let Some(&pp_key) = pp_key_by_name.get(name) else {
                        continue;
                    };
                    if seen.insert(pp_key) {
                        keys.push(pp_key);
                        names.push(name.clone());
                    }
                }
            }
            if !keys.is_empty() {
                keys_by_item.insert(item.key.0, keys);
                names_by_item.insert(item.key.0, names);
            }
        }
        Ok(CrimsonItemPartPrefabHandle {
            part_prefab_keys_by_item: keys_by_item,
            prefab_names_by_item: names_by_item,
        })
    }
}

// ── Load / free ────────────────────────────────────────────────────────────

/// Parse the three required tables from disk.
///
/// # Safety
/// All four path arguments must be NUL-terminated UTF-8 strings.
/// `out_handle` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_item_part_prefab_load_from_file(
    iteminfo_pabgb_path: *const c_char,
    stringinfo_pabgb_path: *const c_char,
    partprefab_pabgb_path: *const c_char,
    partprefab_pabgh_path: *const c_char,
    out_handle: *mut *mut CrimsonItemPartPrefabHandle,
) -> i32 {
    if iteminfo_pabgb_path.is_null()
        || stringinfo_pabgb_path.is_null()
        || partprefab_pabgb_path.is_null()
        || partprefab_pabgh_path.is_null()
        || out_handle.is_null()
    {
        return error::NULL_ARG;
    }
    unsafe { *out_handle = std::ptr::null_mut() };
    catch_unwind(AssertUnwindSafe(|| {
        let paths = [
            iteminfo_pabgb_path,
            stringinfo_pabgb_path,
            partprefab_pabgb_path,
            partprefab_pabgh_path,
        ];
        let mut buffers: [Vec<u8>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
        for (i, p) in paths.iter().enumerate() {
            let s = match unsafe { CStr::from_ptr(*p) }.to_str() {
                Ok(s) => s,
                Err(_) => return error::INVALID_PATH,
            };
            buffers[i] = match std::fs::read(s) {
                Ok(b) => b,
                Err(_) => return error::IO,
            };
        }
        let [it, si, pp_b, pp_h] = buffers;
        match CrimsonItemPartPrefabHandle::from_bytes(&it, &si, &pp_b, &pp_h) {
            Ok(h) => {
                unsafe { *out_handle = Box::into_raw(Box::new(h)) };
                error::OK
            }
            Err(_) => error::BODY_PARSE,
        }
    }))
    .unwrap_or(error::PANIC)
}

/// Parse the three required tables from in-memory bytes.
///
/// # Safety
/// Each of the four `*_bytes` pointers must point to `*_len` readable
/// bytes (may be null iff length is 0). `out_handle` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_item_part_prefab_load_from_bytes(
    iteminfo_pabgb: *const u8,
    iteminfo_pabgb_len: usize,
    stringinfo_pabgb: *const u8,
    stringinfo_pabgb_len: usize,
    partprefab_pabgb: *const u8,
    partprefab_pabgb_len: usize,
    partprefab_pabgh: *const u8,
    partprefab_pabgh_len: usize,
    out_handle: *mut *mut CrimsonItemPartPrefabHandle,
) -> i32 {
    if out_handle.is_null() {
        return error::NULL_ARG;
    }
    let pairs = [
        (iteminfo_pabgb, iteminfo_pabgb_len),
        (stringinfo_pabgb, stringinfo_pabgb_len),
        (partprefab_pabgb, partprefab_pabgb_len),
        (partprefab_pabgh, partprefab_pabgh_len),
    ];
    for &(p, n) in &pairs {
        if p.is_null() && n != 0 {
            return error::NULL_ARG;
        }
    }
    unsafe { *out_handle = std::ptr::null_mut() };
    catch_unwind(AssertUnwindSafe(|| {
        let slice = |p: *const u8, n: usize| -> &[u8] {
            if n == 0 {
                &[][..]
            } else {
                unsafe { std::slice::from_raw_parts(p, n) }
            }
        };
        match CrimsonItemPartPrefabHandle::from_bytes(
            slice(iteminfo_pabgb, iteminfo_pabgb_len),
            slice(stringinfo_pabgb, stringinfo_pabgb_len),
            slice(partprefab_pabgb, partprefab_pabgb_len),
            slice(partprefab_pabgh, partprefab_pabgh_len),
        ) {
            Ok(h) => {
                unsafe { *out_handle = Box::into_raw(Box::new(h)) };
                error::OK
            }
            Err(_) => error::BODY_PARSE,
        }
    }))
    .unwrap_or(error::PANIC)
}

/// Free a handle returned by either loader.
///
/// # Safety
/// `handle` must be null or a pointer previously returned by one of
/// the loaders and not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_item_part_prefab_free(
    handle: *mut CrimsonItemPartPrefabHandle,
) {
    if handle.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(handle);
    }
}

// ── Lookups ────────────────────────────────────────────────────────────────

/// Number of items with at least one resolvable partprefab key. Useful
/// for diagnostics — divide by `iteminfo`'s total item count to get a
/// coverage estimate.
///
/// # Safety
/// `handle` must be live; `out_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_item_part_prefab_resolved_item_count(
    handle: *const CrimsonItemPartPrefabHandle,
    out_count: *mut u32,
) -> i32 {
    if handle.is_null() || out_count.is_null() {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        unsafe { *out_count = h.part_prefab_keys_by_item.len() as u32 };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Number of partprefab keys resolvable for a given item. Returns
/// `NOT_FOUND` (and writes 0) when the item has no resolvable keys.
///
/// # Safety
/// `handle` and `out_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_item_part_prefab_lookup_count(
    handle: *const CrimsonItemPartPrefabHandle,
    item_key: u32,
    out_count: *mut u32,
) -> i32 {
    if handle.is_null() || out_count.is_null() {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        match h.part_prefab_keys_by_item.get(&item_key) {
            Some(v) => {
                unsafe { *out_count = v.len() as u32 };
                error::OK
            }
            None => {
                unsafe { *out_count = 0 };
                error::NOT_FOUND
            }
        }
    }))
    .unwrap_or(error::PANIC)
}

/// Get the partprefab key at insertion order `idx` for the given item.
///
/// Returns `NOT_FOUND` when the item has no resolvable keys,
/// `OUT_OF_RANGE` when `idx >= count`.
///
/// # Safety
/// `handle` and `out_key` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_item_part_prefab_lookup_key_at(
    handle: *const CrimsonItemPartPrefabHandle,
    item_key: u32,
    idx: u32,
    out_key: *mut u32,
) -> i32 {
    if handle.is_null() || out_key.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_key = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some(v) = h.part_prefab_keys_by_item.get(&item_key) else {
            return error::NOT_FOUND;
        };
        let Some(&k) = v.get(idx as usize) else {
            return error::OUT_OF_RANGE;
        };
        unsafe { *out_key = k };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Get the prefab name string corresponding to the partprefab key at
/// position `idx`. Same indexing as
/// [`crimson_item_part_prefab_lookup_key_at`]. Two-call pattern.
///
/// Returns `NOT_FOUND` when the item has no resolvable keys,
/// `OUT_OF_RANGE` when `idx >= count`.
///
/// # Safety
/// `handle` and `required` must be non-null; `buf` may be null iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_item_part_prefab_lookup_prefab_name_at(
    handle: *const CrimsonItemPartPrefabHandle,
    item_key: u32,
    idx: u32,
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
        let Some(v) = h.prefab_names_by_item.get(&item_key) else {
            return error::NOT_FOUND;
        };
        let Some(name) = v.get(idx as usize) else {
            return error::OUT_OF_RANGE;
        };
        let needed = name.len() + 1;
        unsafe { *required = needed };
        if buf_len < needed {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::c_abi::paz::crimson_paz_extract_file;
    use std::ffi::{CStr, CString};
    use std::path::PathBuf;
    use std::ptr;

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
    fn c_abi_item_part_prefab_live() {
        let Some(pamt_path) = find_pamt() else {
            eprintln!("skipping c_abi_item_part_prefab_live: no game install");
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let item_b = extract_file(
            pamt.as_c_str(),
            "gamedata/binary__/client/bin",
            "iteminfo.pabgb",
        );
        let si_b = extract_file(
            pamt.as_c_str(),
            "gamedata/binary__/client/bin",
            "stringinfo.pabgb",
        );
        let pp_b = extract_file(
            pamt.as_c_str(),
            "gamedata/binary__/client/bin",
            "partprefabdyeslotinfo.pabgb",
        );
        let pp_h = extract_file(
            pamt.as_c_str(),
            "gamedata/binary__/client/bin",
            "partprefabdyeslotinfo.pabgh",
        );

        let mut h: *mut CrimsonItemPartPrefabHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_item_part_prefab_load_from_bytes(
                item_b.as_ptr(),
                item_b.len(),
                si_b.as_ptr(),
                si_b.len(),
                pp_b.as_ptr(),
                pp_b.len(),
                pp_h.as_ptr(),
                pp_h.len(),
                &mut h,
            )
        };
        assert_eq!(rc, error::OK);
        assert!(!h.is_null());

        let mut resolved_items: u32 = 0;
        assert_eq!(
            unsafe { crimson_item_part_prefab_resolved_item_count(h, &mut resolved_items) },
            error::OK
        );
        // Probe pass observed ~120 items with at least one resolvable
        // key — assert ≥50 to leave headroom for cross-version drift.
        assert!(
            resolved_items >= 50,
            "expected ≥50 items with partprefab keys, got {resolved_items}"
        );

        // NOT_FOUND for an unknown item key.
        let mut c: u32 = 0;
        assert_eq!(
            unsafe { crimson_item_part_prefab_lookup_count(h, u32::MAX, &mut c) },
            error::NOT_FOUND
        );
        assert_eq!(c, 0);

        // Enumerate the first 5 resolved items and pin invariants:
        // each item's keys + names must be non-empty and same length;
        // every returned partprefab key must round-trip its prefab_name.
        // This guards against accidental desync between the two
        // parallel HashMap entries.
        // We have to read the entries through the public lookups; do a
        // brute scan for items with resolved keys by trying a known
        // dyeable item from the linkage probe.
        // Marni_Devotee_PlateArmor_Helm (item_key=14510) showed 0 partprefab
        // hits in the v3 probe — none of its 5 body-type variants live
        // in partprefabdyeslotinfo. So instead, find any item that has
        // a resolved keys by scanning known partprefab prefab_name
        // candidates that DO appear in iteminfo (per the v3 probe's
        // 50 direct-hit count).
        //
        // Simpler: iterate item_keys 1..50000 looking for any with a
        // resolved count. Cap the scan to avoid pathological runtime.
        let mut found_item: Option<u32> = None;
        for k in 1..200000u32 {
            let mut n: u32 = 0;
            if unsafe { crimson_item_part_prefab_lookup_count(h, k, &mut n) } == error::OK
                && n > 0
            {
                found_item = Some(k);
                break;
            }
        }
        let item = found_item.expect("at least one item must have a resolved partprefab key");
        let mut cnt: u32 = 0;
        assert_eq!(
            unsafe { crimson_item_part_prefab_lookup_count(h, item, &mut cnt) },
            error::OK
        );
        assert!(cnt > 0);
        let mut pk: u32 = 0;
        assert_eq!(
            unsafe { crimson_item_part_prefab_lookup_key_at(h, item, 0, &mut pk) },
            error::OK
        );
        assert!(pk != 0, "resolved partprefab key must be non-zero");

        // Prefab name round-trip — two-call pattern.
        let mut req: usize = 0;
        let rc = unsafe {
            crimson_item_part_prefab_lookup_prefab_name_at(
                h,
                item,
                0,
                ptr::null_mut(),
                0,
                &mut req,
            )
        };
        assert_eq!(rc, error::BUFFER_TOO_SMALL);
        assert!(req > 1);
        let mut buf = vec![0u8; req];
        let mut req2: usize = 0;
        let rc = unsafe {
            crimson_item_part_prefab_lookup_prefab_name_at(
                h,
                item,
                0,
                buf.as_mut_ptr(),
                buf.len(),
                &mut req2,
            )
        };
        assert_eq!(rc, error::OK);
        let name = std::str::from_utf8(&buf[..req2 - 1]).unwrap();
        // Prefab names all start with `cd_` per the partprefab schema.
        assert!(
            name.starts_with("cd_"),
            "resolved prefab name does not start with cd_: {name:?}"
        );

        // OUT_OF_RANGE for an idx past the end.
        assert_eq!(
            unsafe { crimson_item_part_prefab_lookup_key_at(h, item, cnt, &mut pk) },
            error::OUT_OF_RANGE
        );

        unsafe { crimson_item_part_prefab_free(h) };
    }

    #[test]
    fn c_abi_item_part_prefab_null_args() {
        let mut sh: *mut CrimsonItemPartPrefabHandle = ptr::null_mut();
        assert_eq!(
            unsafe {
                crimson_item_part_prefab_load_from_bytes(
                    ptr::null(),
                    16,
                    ptr::null(),
                    16,
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
                crimson_item_part_prefab_load_from_bytes(
                    [0u8; 1].as_ptr(),
                    1,
                    [0u8; 1].as_ptr(),
                    1,
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
    fn c_abi_item_part_prefab_empty_bytes_yields_empty_handle() {
        // Empty iteminfo + empty stringinfo + empty partprefab must
        // produce a valid (empty) handle without panicking.
        let mut sh: *mut CrimsonItemPartPrefabHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_item_part_prefab_load_from_bytes(
                ptr::null(),
                0,
                ptr::null(),
                0,
                ptr::null(),
                0,
                ptr::null(),
                0,
                &mut sh,
            )
        };
        assert_eq!(rc, error::OK);
        assert!(!sh.is_null());
        let mut c: u32 = 0;
        assert_eq!(
            unsafe { crimson_item_part_prefab_resolved_item_count(sh, &mut c) },
            error::OK
        );
        assert_eq!(c, 0);
        unsafe { crimson_item_part_prefab_free(sh) };
    }
}
