//! `questinfo.pabgb` bridge — C ABI surface.
//!
//! Sibling of [`super::mission_info`]. Same architecture and same
//! resolution chain (`QuestKey → name → hashlittle2 → u64 PALOC key
//! → display title`). The terminology / namespace defaults differ
//! per Pearl Abyss's internal model:
//!
//! - `lo32 = 0x100` (256) is the common namespace for the **arc /
//!   region / chapter heading** titles that `questinfo.pabgb` rows
//!   carry (e.g. "Roothold", "Mazes", "Record of the Greymanes").
//! - `lo32 = 0x101` (257) is the namespace for **individual quest
//!   line items** that the missioninfo bridge surfaces — some
//!   `questinfo.pabgb` rows ALSO have entries at this byte (e.g.
//!   `Challenge_Maze` at `0x100` is "Mazes" and at `0x101` is
//!   "Winding Paths"), so the caller passes `lo32_namespace`
//!   explicitly per lookup.
//!
//! See `docs/save-editor-keys-plan.md` §3 for the full architecture
//! and `crate::mission_info` / `super::mission_info` for the
//! reference implementation.

use std::collections::HashMap;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::error;
use super::paloc::CrimsonPalocHandle;
use crate::crypto::checksum::calculate_checksum;
use crate::quest_info::{parse_quest_info_lossy, parse_quest_info_via_pabgh};

/// Opaque handle exposing `(QuestKey, internal_name)` lookups against
/// the loaded `questinfo.pabgb`.
#[repr(C)]
pub struct CrimsonQuestInfoHandle {
    by_key: HashMap<u32, String>,
    entries: Vec<(u32, String)>,
}

impl CrimsonQuestInfoHandle {
    fn from_bytes(data: &[u8]) -> Self {
        Self::build(parse_quest_info_lossy(data).into_iter().map(|e| (e.key, e.name)))
    }

    fn from_pabgh_and_pabgb(pabgh: &[u8], pabgb: &[u8]) -> Self {
        Self::build(
            parse_quest_info_via_pabgh(pabgh, pabgb)
                .into_iter()
                .map(|e| (e.key, e.name)),
        )
    }

    fn build(rows: impl Iterator<Item = (u32, String)>) -> Self {
        // First-wins dedup (see mission_info::from_bytes for the
        // rationale; same comment applies here).
        let rows: Vec<(u32, String)> = rows.collect();
        let mut by_key: HashMap<u32, String> = HashMap::with_capacity(rows.len());
        let mut entries: Vec<(u32, String)> = Vec::with_capacity(rows.len());
        for (key, name) in rows {
            if let std::collections::hash_map::Entry::Vacant(v) = by_key.entry(key) {
                v.insert(name.clone());
                entries.push((key, name));
            }
        }
        CrimsonQuestInfoHandle { by_key, entries }
    }
}

// ── Load / free ────────────────────────────────────────────────────────────

/// Parse `questinfo.pabgb` from disk via the anchor-scan path.
///
/// Under-counts by 1 row in 1.08 (the anchor-scan's `_`-in-name
/// validation drops `LevelSequencerSpawn` / `StartGame`, which have no
/// underscore). **Prefer [`crimson_questinfo_load_from_file_with_pabgh`]**
/// when both files are available — it returns the in-game-authoritative
/// 1,019 entries instead of the anchor-scan's 1,018.
///
/// See [`super::mission_info`] for the full resolution-chain contract.
///
/// # Safety
/// `path` must be a NUL-terminated UTF-8 string. `out_handle` must be
/// non-null and writable for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_questinfo_load_from_file(
    path: *const c_char,
    out_handle: *mut *mut CrimsonQuestInfoHandle,
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
        let handle = CrimsonQuestInfoHandle::from_bytes(&bytes);
        unsafe { *out_handle = Box::into_raw(Box::new(handle)) };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Parse questinfo bytes already in memory via the anchor-scan path.
///
/// Under-counts by 1 row in 1.08 (see
/// [`crimson_questinfo_load_from_file`] for details). **Prefer
/// [`crimson_questinfo_load_from_bytes_with_pabgh`]** for the
/// authoritative key set.
///
/// # Safety
/// `data` must point to `data_len` readable bytes (may be null iff
/// `data_len == 0`). `out_handle` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_questinfo_load_from_bytes(
    data: *const u8,
    data_len: usize,
    out_handle: *mut *mut CrimsonQuestInfoHandle,
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
        let handle = CrimsonQuestInfoHandle::from_bytes(slice);
        unsafe { *out_handle = Box::into_raw(Box::new(handle)) };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Parse `questinfo.pabgb` + `questinfo.pabgh` from disk — the
/// **recommended** path.
///
/// Uses the PABGH `[u32 count][(u32 key, u32 offset)*]` index for the
/// in-game-authoritative key list (1,019 on 1.08), then reads the
/// `[u32 key_repeat][u32 slen][slen bytes]` name header at each offset
/// from PABGB. Returns the same entries the in-game iterator does —
/// no `_`-in-name validation rejecting legitimate rows.
///
/// # Safety
/// Both paths must be NUL-terminated UTF-8. `out_handle` must be
/// non-null and writable for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_questinfo_load_from_file_with_pabgh(
    pabgb_path: *const c_char,
    pabgh_path: *const c_char,
    out_handle: *mut *mut CrimsonQuestInfoHandle,
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
        let handle = CrimsonQuestInfoHandle::from_pabgh_and_pabgb(&pabgh, &pabgb);
        unsafe { *out_handle = Box::into_raw(Box::new(handle)) };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Parse questinfo bytes already in memory via the PABGH-driven path —
/// the **recommended** loader for any caller that has both files.
///
/// See [`crimson_questinfo_load_from_file_with_pabgh`] for the contract
/// (PABGH-authoritative key list, no anchor-scan under-count).
///
/// # Safety
/// Each `*_ptr` must point to `*_len` readable bytes (may be null iff
/// the matching `*_len == 0`). `out_handle` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_questinfo_load_from_bytes_with_pabgh(
    pabgb_ptr: *const u8,
    pabgb_len: usize,
    pabgh_ptr: *const u8,
    pabgh_len: usize,
    out_handle: *mut *mut CrimsonQuestInfoHandle,
) -> i32 {
    if out_handle.is_null() {
        return error::NULL_ARG;
    }
    if (pabgb_ptr.is_null() && pabgb_len != 0)
        || (pabgh_ptr.is_null() && pabgh_len != 0)
    {
        return error::NULL_ARG;
    }
    unsafe { *out_handle = std::ptr::null_mut() };
    catch_unwind(AssertUnwindSafe(|| {
        let pabgb = if pabgb_len == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(pabgb_ptr, pabgb_len) }
        };
        let pabgh = if pabgh_len == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(pabgh_ptr, pabgh_len) }
        };
        let handle = CrimsonQuestInfoHandle::from_pabgh_and_pabgb(pabgh, pabgb);
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
pub unsafe extern "C" fn crimson_questinfo_free(handle: *mut CrimsonQuestInfoHandle) {
    if handle.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(handle);
    }
}

// ── Scalar getters ─────────────────────────────────────────────────────────

/// Total number of quests in the loaded table.
///
/// # Safety
/// `handle` must be live; `out_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_questinfo_entry_count(
    handle: *const CrimsonQuestInfoHandle,
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

/// Look up the internal name for a given `QuestKey (u32)` and write
/// it into `buf` (NUL-terminated UTF-8). Two-call pattern.
///
/// # Safety
/// `handle` and `required` must be non-null; `buf` may be null iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_questinfo_lookup_string_key(
    handle: *const CrimsonQuestInfoHandle,
    quest_key: u32,
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
        let Some(name) = h.by_key.get(&quest_key) else {
            return error::NOT_FOUND;
        };
        write_str_to_buf(name, buf, buf_len, required)
    }))
    .unwrap_or(error::PANIC)
}

// ── Display-name lookup (production — one-shot chain) ──────────────────────

/// One-shot title resolution: chain `QuestKey → internal name →
/// hashlittle2 → PALOC u64 key → localized string`. Same shape as
/// [`super::mission_info::crimson_missioninfo_lookup_display_name`].
///
/// Common `lo32_namespace` values for questinfo rows:
/// - `0x100` (256) — arc / region heading (the typical case)
/// - `0x101` (257) — secondary title (e.g. `Challenge_Maze` has
///   both: "Mazes" at `0x100` and "Winding Paths" at `0x101`)
///
/// # Safety
/// `handle`, `paloc_handle`, and `required` must point to live memory
/// for the duration of the call. `buf` (when non-null) must be
/// writable for `buf_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_questinfo_lookup_display_name(
    handle: *const CrimsonQuestInfoHandle,
    paloc_handle: *const CrimsonPalocHandle,
    quest_key: u32,
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
        let Some(name) = h.by_key.get(&quest_key) else {
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

/// Get the `(quest_key, internal_name)` pair at insertion index `idx`.
///
/// # Safety
/// `handle`, `out_key`, and `required` must be non-null; `buf` may be
/// null iff `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_questinfo_get_entry(
    handle: *const CrimsonQuestInfoHandle,
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
    //! Live-install integration test against `questinfo.pabgb` +
    //! `localizationstring_eng.paloc`. Asserts the verified
    //! `(QuestKey, internal_name, lo32, display_title)` tuples from
    //! `docs/save-editor-keys-plan.md`.

    use crate::binary::gamedata_layout;
    use super::*;
    use crate::c_abi::paloc::{crimson_paloc_free, crimson_paloc_load_from_bytes};
    use crate::c_abi::paz::crimson_paz_extract_file;
    use std::ffi::{CStr, CString};
    use std::path::PathBuf;
    use std::ptr;

    /// (QuestKey, internal_name, lo32_namespace, display_title)
    const KNOWN: &[(u32, &str, u32, &str)] = &[
        (1_000_619, "Quest_Node_Her_RootFort_Normal", 0x100, "Roothold"),
        (
            1_000_881,
            "Quest_Node_Her_GreymaneCamp_Contents",
            0x100,
            "Record of the Greymanes",
        ),
        (1_001_032, "Quest_HumanDocumentary_Del", 0x100, "Human Documentaries"),
        (1_000_039, "Challenge_Maze", 0x100, "Mazes"),
        (1_000_039, "Challenge_Maze", 0x101, "Winding Paths"),
        (1_000_180, "Quest_BloodCoronation_WitchDukeAndDream", 0x100, "Traitor"),
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
    fn c_abi_questinfo_live_full_chain() {
        let Some(pamt_path) = find_pamt() else {
            eprintln!("skipping c_abi_questinfo_live_full_chain: no game install");
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let quest_bytes = extract_file(
            pamt.as_c_str(),
            gamedata_layout::bin_dir(),
            &gamedata_layout::body("questinfo"),
        );

        let mut qh: *mut CrimsonQuestInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_questinfo_load_from_bytes(
                quest_bytes.as_ptr(),
                quest_bytes.len(),
                &mut qh,
            )
        };
        assert_eq!(rc, error::OK);
        assert!(!qh.is_null());

        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_questinfo_entry_count(qh, &mut count) },
            error::OK
        );
        assert!(count > 200, "expected >200 quests, got {count}");

        // Bootstrap PALOC.
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
            unsafe { crimson_questinfo_free(qh) };
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
            // Internal name (only varies by key; assert once per unique key)
            let mut req: usize = 0;
            let rc_size = unsafe {
                crimson_questinfo_lookup_string_key(qh, key, ptr::null_mut(), 0, &mut req)
            };
            let got_name = read_string_result(rc_size, req, |b, n, r| unsafe {
                crimson_questinfo_lookup_string_key(qh, key, b, n, r)
            });
            assert_eq!(got_name, expected_name, "QuestKey {key} name mismatch");

            // Display title at the requested namespace
            let mut req2: usize = 0;
            let rc_size = unsafe {
                crimson_questinfo_lookup_display_name(
                    qh,
                    ph,
                    key,
                    lo32,
                    ptr::null_mut(),
                    0,
                    &mut req2,
                )
            };
            let got_title = read_string_result(rc_size, req2, |b, n, r| unsafe {
                crimson_questinfo_lookup_display_name(qh, ph, key, lo32, b, n, r)
            });
            assert_eq!(
                got_title, expected_title,
                "QuestKey {key} at lo32=0x{lo32:X} title mismatch"
            );
        }

        // Negative paths
        let mut req: usize = 0;
        assert_eq!(
            unsafe {
                crimson_questinfo_lookup_string_key(qh, u32::MAX, ptr::null_mut(), 0, &mut req)
            },
            error::NOT_FOUND
        );
        assert_eq!(
            unsafe {
                crimson_questinfo_lookup_display_name(
                    qh,
                    ph,
                    u32::MAX,
                    0x100,
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            },
            error::NOT_FOUND
        );

        unsafe { crimson_questinfo_free(qh) };
        unsafe { crimson_paloc_free(ph) };
    }

    /// PABGH-driven loader: pins the 1,019 in-game-authoritative
    /// questinfo rows in 1.08 and confirms the anchor-scan path drops
    /// the two no-underscore entries (`LevelSequencerSpawn`,
    /// `StartGame`) that PABGH loads.
    #[test]
    fn c_abi_questinfo_with_pabgh_recovers_no_underscore_rows() {
        let Some(pamt_path) = find_pamt() else {
            eprintln!("skipping c_abi_questinfo_with_pabgh_*: no game install");
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let q_pabgb = extract_file(
            pamt.as_c_str(),
            gamedata_layout::bin_dir(),
            &gamedata_layout::body("questinfo"),
        );
        let q_pabgh = extract_file(
            pamt.as_c_str(),
            gamedata_layout::bin_dir(),
            &gamedata_layout::header("questinfo"),
        );

        let mut anchor_h: *mut CrimsonQuestInfoHandle = ptr::null_mut();
        assert_eq!(
            unsafe {
                crimson_questinfo_load_from_bytes(
                    q_pabgb.as_ptr(),
                    q_pabgb.len(),
                    &mut anchor_h,
                )
            },
            error::OK,
        );
        let mut anchor_count: u32 = 0;
        unsafe { crimson_questinfo_entry_count(anchor_h, &mut anchor_count) };

        let mut pabgh_h: *mut CrimsonQuestInfoHandle = ptr::null_mut();
        assert_eq!(
            unsafe {
                crimson_questinfo_load_from_bytes_with_pabgh(
                    q_pabgb.as_ptr(),
                    q_pabgb.len(),
                    q_pabgh.as_ptr(),
                    q_pabgh.len(),
                    &mut pabgh_h,
                )
            },
            error::OK,
        );
        let mut pabgh_count: u32 = 0;
        unsafe { crimson_questinfo_entry_count(pabgh_h, &mut pabgh_count) };

        assert!(
            pabgh_count >= anchor_count,
            "pabgh path returned {pabgh_count} entries vs anchor-scan {anchor_count}",
        );

        // `LevelSequencerSpawn` (key=1000414) has no underscore in its name
        // and is dropped by the anchor-scan `_`-in-name validation.
        let key = 1_000_414u32;
        let mut req: usize = 0;
        let rc_anchor = unsafe {
            crimson_questinfo_lookup_string_key(anchor_h, key, ptr::null_mut(), 0, &mut req)
        };
        let rc_pabgh = unsafe {
            crimson_questinfo_lookup_string_key(pabgh_h, key, ptr::null_mut(), 0, &mut req)
        };
        assert_eq!(rc_anchor, error::NOT_FOUND);
        assert_eq!(rc_pabgh, error::BUFFER_TOO_SMALL);

        unsafe { crimson_questinfo_free(anchor_h) };
        unsafe { crimson_questinfo_free(pabgh_h) };
    }

    #[test]
    fn c_abi_questinfo_null_args() {
        let mut qh: *mut CrimsonQuestInfoHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_questinfo_load_from_bytes(ptr::null(), 16, &mut qh) },
            error::NULL_ARG,
        );
        assert_eq!(
            unsafe { crimson_questinfo_load_from_bytes([0u8; 1].as_ptr(), 1, ptr::null_mut()) },
            error::NULL_ARG,
        );
        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_questinfo_entry_count(ptr::null(), &mut count) },
            error::NULL_ARG,
        );
        let mut req: usize = 0;
        assert_eq!(
            unsafe {
                crimson_questinfo_lookup_string_key(ptr::null(), 0, ptr::null_mut(), 0, &mut req)
            },
            error::NULL_ARG,
        );
        assert_eq!(
            unsafe {
                crimson_questinfo_lookup_display_name(
                    ptr::null(),
                    ptr::null(),
                    0,
                    0x100,
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            },
            error::NULL_ARG,
        );
    }

    #[test]
    fn c_abi_questinfo_empty_bytes_yields_empty_handle() {
        let mut qh: *mut CrimsonQuestInfoHandle = ptr::null_mut();
        let rc = unsafe { crimson_questinfo_load_from_bytes(ptr::null(), 0, &mut qh) };
        assert_eq!(rc, error::OK);
        assert!(!qh.is_null());
        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_questinfo_entry_count(qh, &mut count) },
            error::OK
        );
        assert_eq!(count, 0);
        unsafe { crimson_questinfo_free(qh) };
    }

    #[test]
    fn c_abi_questinfo_load_bad_path_returns_io() {
        let bad = CString::new("Z:\\definitely\\does\\not\\exist.pabgb").unwrap();
        let mut qh: *mut CrimsonQuestInfoHandle = ptr::null_mut();
        let rc = unsafe { crimson_questinfo_load_from_file(bad.as_ptr(), &mut qh) };
        assert_eq!(rc, error::IO);
        assert!(qh.is_null());
    }
}
