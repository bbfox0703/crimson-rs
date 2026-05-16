//! `factionrelationgroup.pabgb` bridge — C ABI surface.
//!
//! Resolves save-side `FactionRelationGroupKey` to the row's internal
//! name (`"Graymane"`, `"FriendlyCombat"`, `"HostileCombat"`,
//! `"NPC_Common"`, `"Monster_Common"`). The on-disk key is u16; the
//! ABI widens it to u32 (high bits always zero) so it matches the
//! shape every other bridge uses and so the editor doesn't have to
//! special-case this table.
//!
//! **PALOC chain**: none — the exhaustive probe found only
//! coincidental collisions with UI-tooltip sentinel strings at
//! `lo32 = 0x80`. The bridge mirrors [`super::sub_level_info`] /
//! [`super::quest_gauge_info`] in shape — no `lookup_display_name`.
//!
//! Beyond the standard `lookup_string_key`, this bridge exposes
//! [`crimson_factionrelationgroup_lookup_related_count`] +
//! [`crimson_factionrelationgroup_lookup_related_at`] so callers can
//! walk each group's two embedded sibling-reference lists (the
//! per-row relation matrix the editor renders for the "this group
//! relates to ..." UI). See `src/faction_relation_group_info/mod.rs`
//! for the body schema.

use std::collections::HashMap;
use std::ffi::CStr;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::error;
use crate::faction_relation_group_info::parse_faction_relation_group_info_lossy;

/// Opaque handle exposing `(FactionRelationGroupKey, internal_name,
/// related[])` lookups against the loaded `factionrelationgroup.pabgb`
/// + `.pabgh`.
#[repr(C)]
pub struct CrimsonFactionRelationGroupInfoHandle {
    by_key: HashMap<u32, RowData>,
    entries: Vec<(u32, RowData)>,
}

#[derive(Clone)]
struct RowData {
    name: String,
    related: Vec<u32>,
}

impl CrimsonFactionRelationGroupInfoHandle {
    fn from_bytes(pabgb: &[u8], pabgh: &[u8]) -> Self {
        let raw = parse_faction_relation_group_info_lossy(pabgb, pabgh);
        let mut by_key: HashMap<u32, RowData> = HashMap::with_capacity(raw.len());
        let mut entries: Vec<(u32, RowData)> = Vec::with_capacity(raw.len());
        for e in raw {
            let row = RowData {
                name: e.name,
                related: e.related,
            };
            if let std::collections::hash_map::Entry::Vacant(v) = by_key.entry(e.key) {
                v.insert(row.clone());
                entries.push((e.key, row));
            }
        }
        CrimsonFactionRelationGroupInfoHandle { by_key, entries }
    }
}

// ── Load / free ────────────────────────────────────────────────────────────

/// Parse `factionrelationgroup.pabgb` + `.pabgh` from disk.
///
/// # Safety
/// Both path arguments must be NUL-terminated UTF-8 strings.
/// `out_handle` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_factionrelationgroup_load_from_file(
    pabgb_path: *const c_char,
    pabgh_path: *const c_char,
    out_handle: *mut *mut CrimsonFactionRelationGroupInfoHandle,
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
        let handle =
            CrimsonFactionRelationGroupInfoHandle::from_bytes(&pabgb, &pabgh);
        unsafe { *out_handle = Box::into_raw(Box::new(handle)) };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Parse factionrelationgroup pabgb + pabgh bytes already in memory.
///
/// # Safety
/// Both `pabgb` and `pabgh` must point to `*_len` readable bytes (may
/// be null iff length is 0). `out_handle` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_factionrelationgroup_load_from_bytes(
    pabgb: *const u8,
    pabgb_len: usize,
    pabgh: *const u8,
    pabgh_len: usize,
    out_handle: *mut *mut CrimsonFactionRelationGroupInfoHandle,
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
        let handle = CrimsonFactionRelationGroupInfoHandle::from_bytes(
            pabgb_slice,
            pabgh_slice,
        );
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
pub unsafe extern "C" fn crimson_factionrelationgroup_free(
    handle: *mut CrimsonFactionRelationGroupInfoHandle,
) {
    if handle.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(handle);
    }
}

// ── Scalar getters ─────────────────────────────────────────────────────────

/// Total number of `factionrelationgroup` rows in the loaded table
/// (always 5 in 1.07).
///
/// # Safety
/// `handle` must be live; `out_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_factionrelationgroup_entry_count(
    handle: *const CrimsonFactionRelationGroupInfoHandle,
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

/// Look up the internal name for a given `FactionRelationGroupKey`
/// (passed as u32 — only the low 16 bits are meaningful). Two-call
/// pattern.
///
/// Returns `NOT_FOUND` when `faction_relation_group_key` isn't in the
/// table.
///
/// # Safety
/// `handle` and `required` must be non-null; `buf` may be null iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_factionrelationgroup_lookup_string_key(
    handle: *const CrimsonFactionRelationGroupInfoHandle,
    faction_relation_group_key: u32,
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
        let Some(row) = h.by_key.get(&faction_relation_group_key) else {
            return error::NOT_FOUND;
        };
        write_str_to_buf(&row.name, buf, buf_len, required)
    }))
    .unwrap_or(error::PANIC)
}

/// Total number of sibling-row references attached to a given row.
/// (For 1.07 this is the union of the two on-disk `list1` + `list2`
/// counts; e.g. `Graymane` returns 4.)
///
/// Returns `NOT_FOUND` if `faction_relation_group_key` isn't in the
/// table.
///
/// # Safety
/// `handle` and `out_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_factionrelationgroup_lookup_related_count(
    handle: *const CrimsonFactionRelationGroupInfoHandle,
    faction_relation_group_key: u32,
    out_count: *mut u32,
) -> i32 {
    if handle.is_null() || out_count.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_count = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some(row) = h.by_key.get(&faction_relation_group_key) else {
            return error::NOT_FOUND;
        };
        unsafe { *out_count = row.related.len() as u32 };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Read a single sibling-row reference at `idx`.
///
/// Returns `NOT_FOUND` for an unknown key; `OUT_OF_RANGE` for an `idx`
/// past `related_count`.
///
/// # Safety
/// `handle` and `out_related_key` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_factionrelationgroup_lookup_related_at(
    handle: *const CrimsonFactionRelationGroupInfoHandle,
    faction_relation_group_key: u32,
    idx: u32,
    out_related_key: *mut u32,
) -> i32 {
    if handle.is_null() || out_related_key.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_related_key = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some(row) = h.by_key.get(&faction_relation_group_key) else {
            return error::NOT_FOUND;
        };
        let Some(k) = row.related.get(idx as usize) else {
            return error::OUT_OF_RANGE;
        };
        unsafe { *out_related_key = *k };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Get the `(faction_relation_group_key, internal_name)` pair at
/// insertion index `idx`. Two-call pattern.
///
/// # Safety
/// `handle`, `out_key`, and `required` must be non-null; `buf` may be
/// null iff `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_factionrelationgroup_get_entry(
    handle: *const CrimsonFactionRelationGroupInfoHandle,
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
        let Some((key, row)) = h.entries.get(idx as usize) else {
            return error::OUT_OF_RANGE;
        };
        unsafe { *out_key = *key };
        write_str_to_buf(&row.name, buf, buf_len, required)
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

    /// (FactionRelationGroupKey (u16-widened-to-u32), expected name,
    /// expected related count)
    const KNOWN: &[(u32, &str, u32)] = &[
        (0x4243, "Graymane", 4),
        (0x4244, "FriendlyCombat", 4),
        (0x4245, "HostileCombat", 4),
        (0x4246, "NPC_Common", 4),
        (0x4247, "Monster_Common", 4),
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
    fn c_abi_factionrelationgroup_live() {
        let Some(pamt_path) = find_pamt() else {
            eprintln!("skipping c_abi_factionrelationgroup_live: no game install");
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let pabgb = extract_file(
            pamt.as_c_str(),
            "gamedata/binary__/client/bin",
            "factionrelationgroup.pabgb",
        );
        let pabgh = extract_file(
            pamt.as_c_str(),
            "gamedata/binary__/client/bin",
            "factionrelationgroup.pabgh",
        );

        let mut sh: *mut CrimsonFactionRelationGroupInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_factionrelationgroup_load_from_bytes(
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
            unsafe { crimson_factionrelationgroup_entry_count(sh, &mut count) },
            error::OK
        );
        assert_eq!(count, 5, "expected 5 relation-group rows in 1.07");

        for &(key, expected_name, expected_related) in KNOWN {
            let mut req: usize = 0;
            let rc_size = unsafe {
                crimson_factionrelationgroup_lookup_string_key(
                    sh,
                    key,
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            };
            let got = read_string_result(rc_size, req, |b, n, r| unsafe {
                crimson_factionrelationgroup_lookup_string_key(sh, key, b, n, r)
            });
            assert_eq!(
                got, expected_name,
                "FactionRelationGroupKey 0x{key:04x} name mismatch"
            );

            let mut related_count: u32 = 0;
            assert_eq!(
                unsafe {
                    crimson_factionrelationgroup_lookup_related_count(
                        sh,
                        key,
                        &mut related_count,
                    )
                },
                error::OK
            );
            assert_eq!(
                related_count, expected_related,
                "{expected_name}: related count mismatch"
            );

            // Walk references — each must resolve to one of the five
            // known keys via lookup_string_key.
            for i in 0..related_count {
                let mut r_key: u32 = 0;
                assert_eq!(
                    unsafe {
                        crimson_factionrelationgroup_lookup_related_at(
                            sh, key, i, &mut r_key,
                        )
                    },
                    error::OK
                );
                assert!(
                    (0x4243..=0x4247).contains(&r_key),
                    "{expected_name} related[{i}] = 0x{r_key:04x}, expected 0x4243..=0x4247",
                );
                let mut req2: usize = 0;
                let rc2 = unsafe {
                    crimson_factionrelationgroup_lookup_string_key(
                        sh,
                        r_key,
                        ptr::null_mut(),
                        0,
                        &mut req2,
                    )
                };
                let _ = read_string_result(rc2, req2, |b, n, r| unsafe {
                    crimson_factionrelationgroup_lookup_string_key(sh, r_key, b, n, r)
                });
            }
        }

        // Negative: unknown key
        let mut req: usize = 0;
        assert_eq!(
            unsafe {
                crimson_factionrelationgroup_lookup_string_key(
                    sh,
                    u32::MAX,
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            },
            error::NOT_FOUND,
        );
        // OUT_OF_RANGE: related_at past the row's count
        let mut r_key: u32 = 0;
        assert_eq!(
            unsafe {
                crimson_factionrelationgroup_lookup_related_at(
                    sh, 0x4243, 100, &mut r_key,
                )
            },
            error::OUT_OF_RANGE,
        );

        unsafe { crimson_factionrelationgroup_free(sh) };
    }
}
