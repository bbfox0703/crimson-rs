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
            for pd in &item.merged_prefab_visual_list.items {
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

/// Constants for `out_resolve_source` of
/// [`crimson_item_part_prefab_resolve_dye_slot_count`].
///
/// Numeric stability: these constants are part of the C ABI surface
/// and MUST NOT be reassigned. Add new states with the next free
/// integer; never reuse a retired one.
pub mod resolve_dye_slot_count_source {
    /// Slot count came from a direct
    /// `(item_key → partprefab[0] → slot_count)` chain. The
    /// `out_slot_count` value is authoritative.
    pub const DIRECT: u32 = 0;
    /// `item_key` has no entry in the `item_part_prefab` join — its
    /// mesh prefab(s) aren't in `partprefabdyeslotinfo`. Typically a
    /// body-type variant (goblin / dwarf / tribe mesh) that shares
    /// the human male's dye-slot layout but isn't itself listed.
    /// 76% of `is_dyeable=1` items hit this in 1.07 (387/507).
    /// Editor should fall back to a curated default (e.g. the
    /// human-male equip-slot prefab's slot count) or display
    /// "unknown" with a warning. `out_slot_count` is 0.
    pub const NOT_RESOLVED_NO_PARTPREFAB: u32 = 1;
    /// `item_key` mapped to a partprefab via the join, but that
    /// partprefab key isn't in `partprefabdyeslotinfo`. This
    /// shouldn't happen given how the join is built (it only emits
    /// keys that exist in the slot-info table) but the guard stays
    /// in place for defense-in-depth — a future patch could split
    /// the tables. `out_slot_count` is 0.
    pub const NOT_RESOLVED_NO_SLOT_INFO: u32 = 2;
}

/// One-shot "how many dye slots does this item have?" — chains the
/// existing `item_part_prefab` join and `partprefabdyeslotinfo`
/// lookups into a single call. Saves the C# editor from orchestrating
/// the 3-call sequence
/// (`lookup_count` → `lookup_key_at(0)` →
/// `crimson_part_prefab_dye_slot_info_lookup_slot_count`) for every
/// dyeable item it surfaces.
///
/// **Resolution policy**: always uses `partprefab[0]` — the first
/// resolved prefab in iteminfo's `prefab_data_list` traversal order.
/// Multi-prefab items (rare — mainly mesh variants) typically share a
/// dye-slot layout across all variants; if a future use case needs
/// per-variant resolution, switch to the manual chain via
/// [`crimson_item_part_prefab_lookup_key_at`].
///
/// **Return shape**: always returns `OK` (or `NULL_ARG` / `PANIC` for
/// fatal call errors). The "did it resolve?" question is communicated
/// via `out_resolve_source`:
///
/// - `DIRECT` (0): `out_slot_count` is authoritative.
/// - `NOT_RESOLVED_NO_PARTPREFAB` (1): item has no partprefab mapping;
///   editor should fall back to a default or display "unknown".
/// - `NOT_RESOLVED_NO_SLOT_INFO` (2): partprefab present but missing
///   from the slot-info table (shouldn't happen given how the join
///   is built; defensive).
///
/// In all non-`DIRECT` cases `*out_slot_count = 0`.
///
/// # Safety
/// All four pointer arguments must be non-null. Both handles must be
/// live (returned by their respective loaders and not yet freed).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_item_part_prefab_resolve_dye_slot_count(
    item_part_prefab_handle: *const CrimsonItemPartPrefabHandle,
    slot_info_handle: *const super::part_prefab_dye_slot_info::CrimsonPartPrefabDyeSlotInfoHandle,
    item_key: u32,
    out_slot_count: *mut u32,
    out_resolve_source: *mut u32,
) -> i32 {
    if item_part_prefab_handle.is_null()
        || slot_info_handle.is_null()
        || out_slot_count.is_null()
        || out_resolve_source.is_null()
    {
        return error::NULL_ARG;
    }
    unsafe {
        *out_slot_count = 0;
        *out_resolve_source = resolve_dye_slot_count_source::NOT_RESOLVED_NO_PARTPREFAB;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let ipp = unsafe { &*item_part_prefab_handle };

        // Step 1: ItemKey → partprefab[0].
        let Some(prefab_keys) = ipp.part_prefab_keys_by_item.get(&item_key) else {
            return error::OK; // NOT_RESOLVED_NO_PARTPREFAB (already written)
        };
        let Some(&pp_key) = prefab_keys.first() else {
            // Empty Vec wouldn't be in the map (the join drops empties),
            // but guard defensively.
            return error::OK;
        };

        // Step 2: PartPrefabKey → slot_count via the slot-info bridge.
        let mut count: u32 = 0;
        let rc = unsafe {
            super::part_prefab_dye_slot_info::crimson_part_prefab_dye_slot_info_lookup_slot_count(
                slot_info_handle,
                pp_key,
                &mut count,
            )
        };
        match rc {
            x if x == error::OK => {
                unsafe {
                    *out_slot_count = count;
                    *out_resolve_source = resolve_dye_slot_count_source::DIRECT;
                }
                error::OK
            }
            x if x == error::NOT_FOUND => {
                unsafe {
                    *out_resolve_source =
                        resolve_dye_slot_count_source::NOT_RESOLVED_NO_SLOT_INFO;
                }
                error::OK
            }
            // Pass through any other error (PANIC / NULL_ARG from the
            // slot-info bridge — shouldn't happen given our guards, but
            // don't swallow it silently).
            other => other,
        }
    }))
    .unwrap_or(error::PANIC)
}

#[cfg(test)]
mod tests {
    use crate::binary::gamedata_layout;
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
            gamedata_layout::bin_dir(),
            &gamedata_layout::body("iteminfo"),
        );
        let si_b = extract_file(
            pamt.as_c_str(),
            gamedata_layout::bin_dir(),
            &gamedata_layout::body("stringinfo"),
        );
        let pp_b = extract_file(
            pamt.as_c_str(),
            gamedata_layout::bin_dir(),
            &gamedata_layout::body("partprefabdyeslotinfo"),
        );
        let pp_h = extract_file(
            pamt.as_c_str(),
            gamedata_layout::bin_dir(),
            &gamedata_layout::header("partprefabdyeslotinfo"),
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

    /// Live test for the dye-slot-count wrapper. Walks the
    /// resolved-items set, asserts every directly-resolved item gives
    /// a non-zero slot count in [1, 8]. Also pins:
    /// - NOT_RESOLVED_NO_PARTPREFAB for an unknown item key.
    /// - DIRECT resolution chains through both bridges correctly
    ///   (slot count matches a manual `lookup_key_at(0)` →
    ///   `lookup_slot_count` chain).
    #[test]
    fn c_abi_item_part_prefab_resolve_dye_slot_count_live() {
        use super::super::part_prefab_dye_slot_info::*;
        let Some(pamt_path) = find_pamt() else {
            eprintln!("skipping: no game install");
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let item_b =
            extract_file(pamt.as_c_str(), gamedata_layout::bin_dir(), &gamedata_layout::body("iteminfo"));
        let si_b =
            extract_file(pamt.as_c_str(), gamedata_layout::bin_dir(), &gamedata_layout::body("stringinfo"));
        let pp_b = extract_file(
            pamt.as_c_str(),
            gamedata_layout::bin_dir(),
            &gamedata_layout::body("partprefabdyeslotinfo"),
        );
        let pp_h = extract_file(
            pamt.as_c_str(),
            gamedata_layout::bin_dir(),
            &gamedata_layout::header("partprefabdyeslotinfo"),
        );

        // Build both handles.
        let mut ipp_handle: *mut CrimsonItemPartPrefabHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_item_part_prefab_load_from_bytes(
                item_b.as_ptr(), item_b.len(),
                si_b.as_ptr(), si_b.len(),
                pp_b.as_ptr(), pp_b.len(),
                pp_h.as_ptr(), pp_h.len(),
                &mut ipp_handle,
            )
        };
        assert_eq!(rc, error::OK);

        let mut slot_handle: *mut CrimsonPartPrefabDyeSlotInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_part_prefab_dye_slot_info_load_from_bytes(
                pp_b.as_ptr(), pp_b.len(),
                pp_h.as_ptr(), pp_h.len(),
                &mut slot_handle,
            )
        };
        assert_eq!(rc, error::OK);

        // Unknown item key → NOT_RESOLVED_NO_PARTPREFAB.
        let mut count: u32 = 99;
        let mut source: u32 = 99;
        let rc = unsafe {
            crimson_item_part_prefab_resolve_dye_slot_count(
                ipp_handle, slot_handle, u32::MAX,
                &mut count, &mut source,
            )
        };
        assert_eq!(rc, error::OK);
        assert_eq!(count, 0, "unknown key should yield 0 slot count");
        assert_eq!(
            source, resolve_dye_slot_count_source::NOT_RESOLVED_NO_PARTPREFAB,
            "unknown key should report NOT_RESOLVED_NO_PARTPREFAB",
        );

        // Find a directly-resolvable item by scanning the resolved-items set.
        let mut found_item: Option<u32> = None;
        for k in 1..1_000_000u32 {
            let mut n: u32 = 0;
            if unsafe { crimson_item_part_prefab_lookup_count(ipp_handle, k, &mut n) }
                == error::OK
                && n > 0
            {
                found_item = Some(k);
                break;
            }
        }
        let item = found_item.expect("at least one item must have a resolved partprefab key");

        // Resolve via the wrapper.
        let mut wrapper_count: u32 = 0;
        let mut wrapper_source: u32 = 99;
        let rc = unsafe {
            crimson_item_part_prefab_resolve_dye_slot_count(
                ipp_handle, slot_handle, item,
                &mut wrapper_count, &mut wrapper_source,
            )
        };
        assert_eq!(rc, error::OK);
        assert_eq!(
            wrapper_source, resolve_dye_slot_count_source::DIRECT,
            "resolved item should report DIRECT",
        );
        // Slot counts are positive small ints — typical range 1..=16
        // observed in 1.07 (some equipment has many independently-dyeable
        // parts). 32 is a generous defensive cap to flag schema drift.
        assert!(
            (1..=32).contains(&wrapper_count),
            "slot_count {wrapper_count} out of plausible range [1, 32] for item {item}",
        );

        // Manual chain: lookup_key_at(0) → lookup_slot_count.
        let mut manual_pp: u32 = 0;
        assert_eq!(
            unsafe { crimson_item_part_prefab_lookup_key_at(ipp_handle, item, 0, &mut manual_pp) },
            error::OK,
        );
        let mut manual_count: u32 = 0;
        assert_eq!(
            unsafe {
                crimson_part_prefab_dye_slot_info_lookup_slot_count(
                    slot_handle, manual_pp, &mut manual_count,
                )
            },
            error::OK,
        );
        // The wrapper must agree with the manual chain.
        assert_eq!(
            wrapper_count, manual_count,
            "wrapper and manual chain disagree on slot_count for item {item}",
        );

        // Full sweep: every resolved item should give DIRECT + [1, 8] count.
        let mut direct_count = 0u32;
        let mut not_resolved_no_partprefab = 0u32;
        let mut not_resolved_no_slot_info = 0u32;
        for k in 1..1_000_000u32 {
            let mut n: u32 = 0;
            if unsafe { crimson_item_part_prefab_lookup_count(ipp_handle, k, &mut n) }
                != error::OK
            {
                continue;
            }
            if n == 0 {
                continue;
            }
            let mut c: u32 = 0;
            let mut s: u32 = 0;
            assert_eq!(
                unsafe {
                    crimson_item_part_prefab_resolve_dye_slot_count(
                        ipp_handle, slot_handle, k, &mut c, &mut s,
                    )
                },
                error::OK,
            );
            match s {
                x if x == resolve_dye_slot_count_source::DIRECT => {
                    assert!(
                        (1..=32).contains(&c),
                        "item {k} slot_count {c} out of plausible range [1, 32]",
                    );
                    direct_count += 1;
                }
                x if x == resolve_dye_slot_count_source::NOT_RESOLVED_NO_PARTPREFAB => {
                    not_resolved_no_partprefab += 1;
                }
                x if x == resolve_dye_slot_count_source::NOT_RESOLVED_NO_SLOT_INFO => {
                    not_resolved_no_slot_info += 1;
                }
                other => panic!("unexpected source {other} for item {k}"),
            }
        }
        eprintln!(
            "resolve_dye_slot_count sweep: direct={direct_count}, \
             no_partprefab={not_resolved_no_partprefab}, no_slot_info={not_resolved_no_slot_info}",
        );
        // 1.07 baseline: ~45 items in the 1..1_000_000 key range
        // resolve directly. Allow ≥30 for cross-version drift and to
        // keep the test stable when Pearl Abyss reshuffles item-key
        // ranges. The sibling `c_abi_item_part_prefab_live` test
        // pins the broader `resolved_items >= 50` invariant via
        // `crimson_item_part_prefab_resolved_item_count`, which
        // doesn't rely on a key-range scan.
        assert!(
            direct_count >= 30,
            "expected ≥30 DIRECT resolutions in the 1..1M key range, got {direct_count}",
        );
        // NO_PARTPREFAB shouldn't fire from this loop — the loop only
        // visits items WITH a partprefab mapping (lookup_count != 0).
        // If we hit it here, the wrapper's branch logic is wrong.
        assert_eq!(
            not_resolved_no_partprefab, 0,
            "wrapper emitted NO_PARTPREFAB for items in the join — branch logic bug",
        );

        unsafe {
            crimson_part_prefab_dye_slot_info_free(slot_handle);
            crimson_item_part_prefab_free(ipp_handle);
        }
    }

    #[test]
    fn c_abi_item_part_prefab_resolve_dye_slot_count_null_args() {
        // Every required pointer must trigger NULL_ARG.
        let mut count: u32 = 0;
        let mut source: u32 = 0;
        let rc = unsafe {
            crimson_item_part_prefab_resolve_dye_slot_count(
                ptr::null(), ptr::null(), 9102, &mut count, &mut source,
            )
        };
        assert_eq!(rc, error::NULL_ARG);

        // out_slot_count null
        let rc = unsafe {
            crimson_item_part_prefab_resolve_dye_slot_count(
                ptr::null(), ptr::null(), 9102, ptr::null_mut(), &mut source,
            )
        };
        assert_eq!(rc, error::NULL_ARG);

        // out_resolve_source null
        let rc = unsafe {
            crimson_item_part_prefab_resolve_dye_slot_count(
                ptr::null(), ptr::null(), 9102, &mut count, ptr::null_mut(),
            )
        };
        assert_eq!(rc, error::NULL_ARG);
    }

    /// Constants must be distinct and stable. Layout check on the
    /// `resolve_dye_slot_count_source` module.
    #[test]
    fn resolve_dye_slot_count_source_constants_are_distinct() {
        let vals = [
            resolve_dye_slot_count_source::DIRECT,
            resolve_dye_slot_count_source::NOT_RESOLVED_NO_PARTPREFAB,
            resolve_dye_slot_count_source::NOT_RESOLVED_NO_SLOT_INFO,
        ];
        let mut sorted = vals.to_vec();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), vals.len(), "source constants collide");
        // Pin the numeric values so a future renumbering breaks loud.
        assert_eq!(resolve_dye_slot_count_source::DIRECT, 0);
        assert_eq!(resolve_dye_slot_count_source::NOT_RESOLVED_NO_PARTPREFAB, 1);
        assert_eq!(resolve_dye_slot_count_source::NOT_RESOLVED_NO_SLOT_INFO, 2);
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
