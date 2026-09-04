//! `questgaugeinfo.pabgb` bridge — C ABI surface.
//!
//! Resolves save-side `QuestGaugeKey (u32)` to the gauge's internal
//! name (e.g. `"QuestGauge_WindCliffFort"`). Unlike the
//! mission / quest / stage bridges, **no `lookup_display_name`
//! function is exposed** because QuestGauge rows don't have PALOC
//! entries at any namespace — they're internal-only progress meters
//! (kill counters, faction-operation tickers, region-defense gauges).
//! The localized name a player sees in the UI comes from the
//! referenced stage / mission, not the gauge itself.
//!
//! The internal name is still useful in the Save Editor's resolved-
//! name column: `"QuestGauge_WindCliffFort"` is enough context for
//! a modder to know what they're looking at. Future enhancement
//! could parse the row body to extract referenced StageKey / mission
//! keys and chain into the stage / mission bridges, but the bridge
//! itself stays single-purpose.
//!
//! See `docs/save-editor-keys-plan.md` §4 for the rationale and the
//! probe that confirmed zero PALOC hits across every namespace.

use std::collections::HashMap;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::error;
use crate::quest_gauge_info::parse_quest_gauge_info_lossy;

/// Opaque handle exposing `(QuestGaugeKey, internal_name)` lookups
/// against the loaded `questgaugeinfo.pabgb`.
#[repr(C)]
pub struct CrimsonQuestGaugeInfoHandle {
    by_key: HashMap<u32, String>,
    entries: Vec<(u32, String)>,
}

impl CrimsonQuestGaugeInfoHandle {
    fn from_bytes(data: &[u8]) -> Self {
        let raw = parse_quest_gauge_info_lossy(data);
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
        CrimsonQuestGaugeInfoHandle { by_key, entries }
    }
}

// ── Load / free ────────────────────────────────────────────────────────────

/// Parse `questgaugeinfo.pabgb` from disk.
///
/// # Safety
/// `path` must be a NUL-terminated UTF-8 string. `out_handle` must be
/// non-null and writable for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_questgaugeinfo_load_from_file(
    path: *const c_char,
    out_handle: *mut *mut CrimsonQuestGaugeInfoHandle,
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
        let handle = CrimsonQuestGaugeInfoHandle::from_bytes(&bytes);
        unsafe { *out_handle = Box::into_raw(Box::new(handle)) };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Parse questgaugeinfo bytes already in memory.
///
/// # Safety
/// `data` must point to `data_len` readable bytes (may be null iff
/// `data_len == 0`). `out_handle` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_questgaugeinfo_load_from_bytes(
    data: *const u8,
    data_len: usize,
    out_handle: *mut *mut CrimsonQuestGaugeInfoHandle,
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
        let handle = CrimsonQuestGaugeInfoHandle::from_bytes(slice);
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
pub unsafe extern "C" fn crimson_questgaugeinfo_free(
    handle: *mut CrimsonQuestGaugeInfoHandle,
) {
    if handle.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(handle);
    }
}

// ── Scalar getters ─────────────────────────────────────────────────────────

/// Total number of gauges in the loaded table.
///
/// # Safety
/// `handle` must be live; `out_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_questgaugeinfo_entry_count(
    handle: *const CrimsonQuestGaugeInfoHandle,
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

// ── Internal-name lookup (the only resolution surface) ─────────────────────

/// Look up the internal name for a given `QuestGaugeKey (u32)` and
/// write it into `buf` (NUL-terminated UTF-8). Two-call pattern.
///
/// Returns `NOT_FOUND` when `gauge_key` isn't in the table.
///
/// **No `lookup_display_name` analogue exists for this bridge** —
/// QuestGauge rows have no PALOC entries (confirmed by exhaustive
/// scan, see `docs/save-editor-keys-plan.md` §4). The internal name
/// is the only resolution surface; the editor surfaces it directly.
///
/// # Safety
/// `handle` and `required` must be non-null; `buf` may be null iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_questgaugeinfo_lookup_string_key(
    handle: *const CrimsonQuestGaugeInfoHandle,
    gauge_key: u32,
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
        let Some(name) = h.by_key.get(&gauge_key) else {
            return error::NOT_FOUND;
        };
        write_str_to_buf(name, buf, buf_len, required)
    }))
    .unwrap_or(error::PANIC)
}

// ── Enumeration ────────────────────────────────────────────────────────────

/// Get the `(gauge_key, internal_name)` pair at insertion index `idx`.
///
/// # Safety
/// `handle`, `out_key`, and `required` must be non-null; `buf` may be
/// null iff `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_questgaugeinfo_get_entry(
    handle: *const CrimsonQuestGaugeInfoHandle,
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
    //! Live-install integration test against `questgaugeinfo.pabgb`.

    use crate::binary::gamedata_layout;
    use super::*;
    use crate::c_abi::paz::crimson_paz_extract_file;
    use std::ffi::{CStr, CString};
    use std::path::PathBuf;
    use std::ptr;

    const KNOWN: &[(u32, &str)] = &[
        (1_000_083, "QuestGauge_WindCliffFort"),
        (1_000_084, "QuestGauge_AbandonedWyvernNest"),
        (1_000_091, "QuestGauge_FortManub"),
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
    fn c_abi_questgaugeinfo_live() {
        let Some(pamt_path) = find_pamt() else {
            eprintln!("skipping c_abi_questgaugeinfo_live: no game install");
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let bytes = extract_file(
            pamt.as_c_str(),
            gamedata_layout::bin_dir(),
            &gamedata_layout::body("questgaugeinfo"),
        );

        let mut gh: *mut CrimsonQuestGaugeInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_questgaugeinfo_load_from_bytes(bytes.as_ptr(), bytes.len(), &mut gh)
        };
        assert_eq!(rc, error::OK);
        assert!(!gh.is_null());

        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_questgaugeinfo_entry_count(gh, &mut count) },
            error::OK
        );
        assert!(count > 200, "expected >200 gauges, got {count}");

        for &(key, expected) in KNOWN {
            let mut req: usize = 0;
            let rc_size = unsafe {
                crimson_questgaugeinfo_lookup_string_key(
                    gh,
                    key,
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            };
            let got = read_string_result(rc_size, req, |b, n, r| unsafe {
                crimson_questgaugeinfo_lookup_string_key(gh, key, b, n, r)
            });
            assert_eq!(got, expected, "QuestGaugeKey {key} name mismatch");
        }

        // Negative path
        let mut req: usize = 0;
        assert_eq!(
            unsafe {
                crimson_questgaugeinfo_lookup_string_key(
                    gh,
                    u32::MAX,
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            },
            error::NOT_FOUND,
        );

        unsafe { crimson_questgaugeinfo_free(gh) };
    }

    #[test]
    fn c_abi_questgaugeinfo_null_args() {
        let mut gh: *mut CrimsonQuestGaugeInfoHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_questgaugeinfo_load_from_bytes(ptr::null(), 16, &mut gh) },
            error::NULL_ARG,
        );
        assert_eq!(
            unsafe {
                crimson_questgaugeinfo_load_from_bytes(
                    [0u8; 1].as_ptr(),
                    1,
                    ptr::null_mut(),
                )
            },
            error::NULL_ARG,
        );
        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_questgaugeinfo_entry_count(ptr::null(), &mut count) },
            error::NULL_ARG,
        );
        let mut req: usize = 0;
        assert_eq!(
            unsafe {
                crimson_questgaugeinfo_lookup_string_key(
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
    fn c_abi_questgaugeinfo_empty_bytes_yields_empty_handle() {
        let mut gh: *mut CrimsonQuestGaugeInfoHandle = ptr::null_mut();
        let rc = unsafe { crimson_questgaugeinfo_load_from_bytes(ptr::null(), 0, &mut gh) };
        assert_eq!(rc, error::OK);
        assert!(!gh.is_null());
        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_questgaugeinfo_entry_count(gh, &mut count) },
            error::OK
        );
        assert_eq!(count, 0);
        unsafe { crimson_questgaugeinfo_free(gh) };
    }

    #[test]
    fn c_abi_questgaugeinfo_load_bad_path_returns_io() {
        let bad = CString::new("Z:\\definitely\\does\\not\\exist.pabgb").unwrap();
        let mut gh: *mut CrimsonQuestGaugeInfoHandle = ptr::null_mut();
        let rc = unsafe { crimson_questgaugeinfo_load_from_file(bad.as_ptr(), &mut gh) };
        assert_eq!(rc, error::IO);
        assert!(gh.is_null());
    }
}
