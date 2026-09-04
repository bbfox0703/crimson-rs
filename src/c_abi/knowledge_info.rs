//! `knowledgeinfo.pabgb` bridge — C ABI surface.
//!
//! Sibling of [`super::mission_info`] / [`super::quest_info`] /
//! [`super::stage_info`]. Same resolution chain (`KnowledgeKey → name
//! → hashlittle2 → u64 PALOC key → display title`).
//!
//! The editor's earlier hypothesis (status.md item #6) was that
//! KnowledgeKey wouldn't resolve through PALOC. The
//! [`super::checksum`]-based probe disproved that — 29,330 PALOC
//! hits across multiple namespaces when every `Knowledge_*` name in
//! `knowledgeinfo.pabgb` was hashed. So the same one-shot bridge
//! shape applies.
//!
//! Default `lo32 = 0x490` (1168) for knowledge titles. Caller passes
//! a different byte if they want the description (0x491) or one of
//! the secondary text variants.
//!
//! Notable distinction from the mission/quest/stage family: the
//! editor side has been thinking of KnowledgeKey through a 4-level
//! categorical hierarchy (Creatures > Terrestrial > Amphibians >
//! Burrowing Salamander), which is **NOT** what this bridge
//! surfaces. The hierarchy lives in `knowledgegroupinfo.pabgb` as a
//! separate integer-ID-based namespace (categories at
//! `lo32=0x92/0x93` with tiny hi32 values that are group IDs, not
//! hashes). The leaf knowledge row's PALOC title is a name in its
//! own right (often the same string as the wiki leaf, sometimes a
//! richer phrase). The category breadcrumb is a separate concern —
//! see "Future enhancement" below.

use std::collections::HashMap;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::error;
use super::paloc::CrimsonPalocHandle;
use crate::crypto::checksum::calculate_checksum;
use crate::knowledge_info::parse_knowledge_info_lossy;

/// Opaque handle exposing `(KnowledgeKey, internal_name)` lookups
/// against the loaded `knowledgeinfo.pabgb`.
#[repr(C)]
pub struct CrimsonKnowledgeInfoHandle {
    by_key: HashMap<u32, String>,
    entries: Vec<(u32, String)>,
}

impl CrimsonKnowledgeInfoHandle {
    fn from_bytes(data: &[u8]) -> Self {
        let raw = parse_knowledge_info_lossy(data);
        // First-wins dedup (see mission_info::from_bytes for the
        // rationale; same comment applies here).
        let mut by_key: HashMap<u32, String> = HashMap::with_capacity(raw.len());
        let mut entries: Vec<(u32, String)> = Vec::with_capacity(raw.len());
        for e in raw {
            if let std::collections::hash_map::Entry::Vacant(v) = by_key.entry(e.key) {
                v.insert(e.name.clone());
                entries.push((e.key, e.name));
            }
        }
        CrimsonKnowledgeInfoHandle { by_key, entries }
    }
}

// ── Load / free ────────────────────────────────────────────────────────────

/// Parse `knowledgeinfo.pabgb` from disk.
///
/// # Safety
/// `path` must be a NUL-terminated UTF-8 string. `out_handle` must be
/// non-null and writable for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_knowledgeinfo_load_from_file(
    path: *const c_char,
    out_handle: *mut *mut CrimsonKnowledgeInfoHandle,
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
        let bytes: Vec<u8> = match std::fs::read(path_str) {
            Ok(b) => b,
            Err(_) => return error::IO,
        };
        let handle = CrimsonKnowledgeInfoHandle::from_bytes(&bytes);
        unsafe { *out_handle = Box::into_raw(Box::new(handle)) };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Parse knowledgeinfo bytes already in memory.
///
/// # Safety
/// `data` must point to `data_len` readable bytes (may be null iff
/// `data_len == 0`). `out_handle` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_knowledgeinfo_load_from_bytes(
    data: *const u8,
    data_len: usize,
    out_handle: *mut *mut CrimsonKnowledgeInfoHandle,
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
        let handle = CrimsonKnowledgeInfoHandle::from_bytes(slice);
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
pub unsafe extern "C" fn crimson_knowledgeinfo_free(
    handle: *mut CrimsonKnowledgeInfoHandle,
) {
    if handle.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(handle);
    }
}

// ── Scalar getters ─────────────────────────────────────────────────────────

/// Total number of knowledge rows in the loaded table.
///
/// # Safety
/// `handle` must be live; `out_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_knowledgeinfo_entry_count(
    handle: *const CrimsonKnowledgeInfoHandle,
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

// ── Internal-name lookup (fallback) ────────────────────────────────────────

/// Look up the internal name for a given `KnowledgeKey (u32)` and
/// write it into `buf` (NUL-terminated UTF-8). Two-call pattern.
///
/// # Safety
/// `handle` and `required` must be non-null; `buf` may be null iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_knowledgeinfo_lookup_string_key(
    handle: *const CrimsonKnowledgeInfoHandle,
    knowledge_key: u32,
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
        let Some(name) = h.by_key.get(&knowledge_key) else {
            return error::NOT_FOUND;
        };
        write_str_to_buf(name, buf, buf_len, required)
    }))
    .unwrap_or(error::PANIC)
}

// ── Display-name lookup (production — one-shot chain) ──────────────────────

/// One-shot title resolution: chain `KnowledgeKey → internal name →
/// hashlittle2 → PALOC u64 key → localized string`.
///
/// Common `lo32_namespace` values for knowledge rows (confirmed by
/// the probe in `docs/save-editor-keys-plan.md`):
/// - `0x490` (1168) — knowledge entry title (the typical case;
///   every row resolves at this byte)
/// - `0x491` (1169) — knowledge entry description (short text;
///   every row resolves)
/// - `0x49F` (1183) — description (alternate, identical to 0x491
///   for many rows; every row resolves)
/// - `0x49D` (1181) / `0x49E` (1182) — secondary text on ~70% of
///   rows; "lore" / "discovery" variant
///
/// # Safety
/// `handle`, `paloc_handle`, and `required` must point to live memory
/// for the duration of the call. `buf` (when non-null) must be
/// writable for `buf_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_knowledgeinfo_lookup_display_name(
    handle: *const CrimsonKnowledgeInfoHandle,
    paloc_handle: *const CrimsonPalocHandle,
    knowledge_key: u32,
    lo32_namespace: u32,
    buf: *mut u8,
    buf_len: usize,
    required: *mut usize,
) -> i32 {
    if handle.is_null() || paloc_handle.is_null() || required.is_null() {
        return error::NULL_ARG;
    }
    if buf.is_null() && buf_len != 0 {
        return error::NULL_ARG;
    }
    unsafe { *required = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let paloc = unsafe { &*paloc_handle };
        let Some(name) = h.by_key.get(&knowledge_key) else {
            return error::NOT_FOUND;
        };
        let hi32 = calculate_checksum(name.as_bytes()) as u64;
        let u64_key = (hi32 << 32) | u64::from(lo32_namespace);
        let decimal = format!("{u64_key}");
        let Some(display) = paloc.lookup_str(&decimal) else {
            return error::NOT_FOUND;
        };
        write_str_to_buf(display, buf, buf_len, required)
    }))
    .unwrap_or(error::PANIC)
}

// ── Enumeration ────────────────────────────────────────────────────────────

/// Get the `(knowledge_key, internal_name)` pair at insertion index
/// `idx`.
///
/// # Safety
/// `handle`, `out_key`, and `required` must be non-null; `buf` may be
/// null iff `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_knowledgeinfo_get_entry(
    handle: *const CrimsonKnowledgeInfoHandle,
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
    //! Live-install integration test against `knowledgeinfo.pabgb` +
    //! `localizationstring_eng.paloc`.
    //!
    //! Five `(KnowledgeKey, internal_name, lo32, display_title)`
    //! tuples verified by probing the live install. Includes one row
    //! (`Knowledge_Node_Dem_Ruins_0007`) tested at both 0x490 (title)
    //! and 0x491 (description) to pin the namespace argument as
    //! load-bearing.

    use crate::binary::gamedata_layout;
    use super::*;
    use crate::c_abi::paloc::{crimson_paloc_free, crimson_paloc_load_from_bytes};
    use crate::c_abi::paz::crimson_paz_extract_file;
    use std::ffi::{CStr, CString};
    use std::path::PathBuf;
    use std::ptr;

    /// (KnowledgeKey, internal_name, lo32_namespace, display_title)
    const KNOWN: &[(u32, &str, u32, &str)] = &[
        (1_002_588, "Knowledge_Node_Dem_Ruins_0007", 0x490, "Demenissian Ruins"),
        (
            1_002_588,
            "Knowledge_Node_Dem_Ruins_0007",
            0x491,
            "Ruins of the continent of Pywel.",
        ),
        (1_002_294, "Knowledge_Node_Dem_HiddenCave", 0x490, "Hidden Cave"),
        (1_002_763, "Knowledge_AbyssRuins_Dem_0020", 0x490, "Abyss Nexus"),
        (
            1_004_037,
            "Knowledge_Demian_Plate_Boots_V",
            0x490,
            "Executioner of Darkness Plate Boots",
        ),
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
    fn c_abi_knowledgeinfo_live_full_chain() {
        let Some(pamt_path) = find_pamt() else {
            eprintln!("skipping c_abi_knowledgeinfo_live_full_chain: no game install");
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let knowledge_bytes = extract_file(
            pamt.as_c_str(),
            gamedata_layout::bin_dir(),
            &gamedata_layout::body("knowledgeinfo"),
        );

        let mut kh: *mut CrimsonKnowledgeInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_knowledgeinfo_load_from_bytes(
                knowledge_bytes.as_ptr(),
                knowledge_bytes.len(),
                &mut kh,
            )
        };
        assert_eq!(rc, error::OK);
        assert!(!kh.is_null());

        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_knowledgeinfo_entry_count(kh, &mut count) },
            error::OK
        );
        assert!(count > 1_000, "expected >1000 knowledge rows, got {count}");

        let paloc_pamt = {
            let mut p = pamt_path.clone();
            p.pop();
            p.pop();
            p.push("0020");
            p.push("0.pamt");
            p
        };
        if !paloc_pamt.is_file() {
            eprintln!("skipping paloc chain: no 0020/0.pamt");
            unsafe { crimson_knowledgeinfo_free(kh) };
            return;
        }
        let paloc_bytes = gamedata_layout::paloc_bytes("0020", "eng")
            .expect("eng paloc must load from 0020");
        let mut ph: *mut CrimsonPalocHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_paloc_load_from_bytes(paloc_bytes.as_ptr(), paloc_bytes.len(), &mut ph)
        };
        assert_eq!(rc, error::OK);
        assert!(!ph.is_null());

        for &(key, expected_name, lo32, expected_title) in KNOWN {
            // Internal name (same for repeated key rows)
            let mut req: usize = 0;
            let rc_size = unsafe {
                crimson_knowledgeinfo_lookup_string_key(
                    kh,
                    key,
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            };
            let got_name = read_string_result(rc_size, req, |b, n, r| unsafe {
                crimson_knowledgeinfo_lookup_string_key(kh, key, b, n, r)
            });
            assert_eq!(got_name, expected_name, "KnowledgeKey {key} name mismatch");

            // Display title / description at the requested namespace
            let mut req2: usize = 0;
            let rc_size = unsafe {
                crimson_knowledgeinfo_lookup_display_name(
                    kh,
                    ph,
                    key,
                    lo32,
                    ptr::null_mut(),
                    0,
                    &mut req2,
                )
            };
            let got_title = read_string_result(rc_size, req2, |b, n, r| unsafe {
                crimson_knowledgeinfo_lookup_display_name(kh, ph, key, lo32, b, n, r)
            });
            assert_eq!(
                got_title, expected_title,
                "KnowledgeKey {key} at lo32=0x{lo32:X} title mismatch"
            );
        }

        // Negative paths
        let mut req: usize = 0;
        assert_eq!(
            unsafe {
                crimson_knowledgeinfo_lookup_string_key(
                    kh,
                    u32::MAX,
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            },
            error::NOT_FOUND
        );
        assert_eq!(
            unsafe {
                crimson_knowledgeinfo_lookup_display_name(
                    kh,
                    ph,
                    u32::MAX,
                    0x490,
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            },
            error::NOT_FOUND
        );

        unsafe { crimson_knowledgeinfo_free(kh) };
        unsafe { crimson_paloc_free(ph) };
    }

    #[test]
    fn c_abi_knowledgeinfo_null_args() {
        let mut kh: *mut CrimsonKnowledgeInfoHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_knowledgeinfo_load_from_bytes(ptr::null(), 16, &mut kh) },
            error::NULL_ARG,
        );
        assert_eq!(
            unsafe {
                crimson_knowledgeinfo_load_from_bytes([0u8; 1].as_ptr(), 1, ptr::null_mut())
            },
            error::NULL_ARG,
        );
        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_knowledgeinfo_entry_count(ptr::null(), &mut count) },
            error::NULL_ARG,
        );
        let mut req: usize = 0;
        assert_eq!(
            unsafe {
                crimson_knowledgeinfo_lookup_string_key(
                    ptr::null(),
                    0,
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            },
            error::NULL_ARG,
        );
        assert_eq!(
            unsafe {
                crimson_knowledgeinfo_lookup_display_name(
                    ptr::null(),
                    ptr::null(),
                    0,
                    0x490,
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            },
            error::NULL_ARG,
        );
    }

    #[test]
    fn c_abi_knowledgeinfo_empty_bytes_yields_empty_handle() {
        let mut kh: *mut CrimsonKnowledgeInfoHandle = ptr::null_mut();
        let rc = unsafe { crimson_knowledgeinfo_load_from_bytes(ptr::null(), 0, &mut kh) };
        assert_eq!(rc, error::OK);
        assert!(!kh.is_null());
        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_knowledgeinfo_entry_count(kh, &mut count) },
            error::OK
        );
        assert_eq!(count, 0);
        unsafe { crimson_knowledgeinfo_free(kh) };
    }

    #[test]
    fn c_abi_knowledgeinfo_load_bad_path_returns_io() {
        let bad = CString::new("Z:\\definitely\\does\\not\\exist.pabgb").unwrap();
        let mut kh: *mut CrimsonKnowledgeInfoHandle = ptr::null_mut();
        let rc = unsafe { crimson_knowledgeinfo_load_from_file(bad.as_ptr(), &mut kh) };
        assert_eq!(rc, error::IO);
        assert!(kh.is_null());
    }
}
