//! `globalgameevent.pabgb` bridge — C ABI surface.
//!
//! Resolves save-side `GlobalGameEventInfoKey (u16-widened-u32)` and
//! exposes the per-event body fields documented in
//! [`docs/archive/globalgameevent-body-re.md`](../../docs/archive/globalgameevent-body-re.md):
//!
//! - `crimson_global_game_event_info_lookup_string_key` — internal
//!   template name (e.g. `"Drought_Varnian"`). Universal coverage.
//! - `crimson_global_game_event_info_lookup_group_key` — the
//!   `GlobalGameEventGroupKey` cross-reference (1 of 7 distinct
//!   values: `WeatherEventGroup`, `FactionBlockEventGroup`, …).
//!   Universal coverage.
//! - `crimson_global_game_event_info_lookup_paloc_key` — the 64-bit
//!   PALOC localization key (in `(hi32 = event_key, lo32 = namespace)`
//!   form) that resolves the event's localized display name through
//!   the existing PALOC bridge. Returns `0` for rows that lack the
//!   embedded `PalocStringRef` (the `RoyalSupply` + `FactionBlockEvent_*`
//!   groups — ~24 of 103 rows in 1.07).
//!
//! 103 rows in 1.07. **NOT** macro-generated (unlike most niche
//! bridges) because the additional `group_key` + `paloc_key` lookups
//! need per-entry data beyond the `(key, name)` pair the macro
//! supports.

use std::ffi::CStr;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::{error, write_str_to_buf};
use crate::global_game_event_info::{
    GlobalGameEventInfoEntry, parse_global_game_event_info_lossy,
};

/// Opaque handle. Stores the parsed entries in two indexes:
///
/// - `by_key`: `u32 key → entry index in `entries`` for O(1) lookups.
/// - `entries`: preserved on-disk order, used by `get_entry`.
#[repr(C)]
pub struct CrimsonGlobalGameEventInfoHandle {
    by_key: std::collections::HashMap<u32, usize>,
    entries: Vec<GlobalGameEventInfoEntry>,
}

impl CrimsonGlobalGameEventInfoHandle {
    fn from_bytes(pabgb: &[u8], pabgh: &[u8]) -> Self {
        let raw = parse_global_game_event_info_lossy(pabgb, pabgh);
        let mut by_key: std::collections::HashMap<u32, usize> =
            std::collections::HashMap::with_capacity(raw.len());
        let mut entries: Vec<GlobalGameEventInfoEntry> = Vec::with_capacity(raw.len());
        for e in raw {
            if let std::collections::hash_map::Entry::Vacant(v) = by_key.entry(e.key) {
                v.insert(entries.len());
                entries.push(e);
            }
        }
        CrimsonGlobalGameEventInfoHandle { by_key, entries }
    }
}

// ── Load / free ────────────────────────────────────────────────────────────

/// Parse `globalgameevent.{pabgb,pabgh}` from disk.
///
/// # Safety
/// Both path arguments must be NUL-terminated UTF-8 strings;
/// `out_handle` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_global_game_event_info_load_from_file(
    pabgb_path: *const c_char,
    pabgh_path: *const c_char,
    out_handle: *mut *mut CrimsonGlobalGameEventInfoHandle,
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
        let handle = CrimsonGlobalGameEventInfoHandle::from_bytes(&pabgb, &pabgh);
        unsafe { *out_handle = Box::into_raw(Box::new(handle)) };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Parse globalgameevent bytes already in memory.
///
/// # Safety
/// `pabgb`/`pabgh` may be null iff their length is 0; `out_handle`
/// must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_global_game_event_info_load_from_bytes(
    pabgb: *const u8,
    pabgb_len: usize,
    pabgh: *const u8,
    pabgh_len: usize,
    out_handle: *mut *mut CrimsonGlobalGameEventInfoHandle,
) -> i32 {
    if out_handle.is_null() {
        return error::NULL_ARG;
    }
    if (pabgb.is_null() && pabgb_len != 0) || (pabgh.is_null() && pabgh_len != 0) {
        return error::NULL_ARG;
    }
    unsafe { *out_handle = std::ptr::null_mut() };
    catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller-guaranteed — null+0 routes to empty slice via
        // the helper's null-aware branch; non-null+N points to N
        // readable bytes per the function's safety contract.
        let pabgb_slice = unsafe { super::slice_from_raw_or_empty(pabgb, pabgb_len) };
        let pabgh_slice = unsafe { super::slice_from_raw_or_empty(pabgh, pabgh_len) };
        let handle = CrimsonGlobalGameEventInfoHandle::from_bytes(pabgb_slice, pabgh_slice);
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
pub unsafe extern "C" fn crimson_global_game_event_info_free(
    handle: *mut CrimsonGlobalGameEventInfoHandle,
) {
    if handle.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(handle);
    }
}

// ── Scalar getters ─────────────────────────────────────────────────────────

/// Total number of entries in the loaded table.
///
/// # Safety
/// `handle` must be live; `out_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_global_game_event_info_entry_count(
    handle: *const CrimsonGlobalGameEventInfoHandle,
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

// ── Per-key lookups ────────────────────────────────────────────────────────

/// Look up the internal template name for a `GlobalGameEventInfoKey`.
/// Writes the result into `buf` as a NUL-terminated UTF-8 string.
///
/// Two-call shape: pass `buf = null, buf_len = 0` to query the required
/// byte count (including the NUL terminator); allocate and re-call.
///
/// # Safety
/// `handle` and `required` must be non-null; `buf` may be null iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_global_game_event_info_lookup_string_key(
    handle: *const CrimsonGlobalGameEventInfoHandle,
    global_game_event_key: u32,
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
        let Some(&idx) = h.by_key.get(&global_game_event_key) else {
            return error::NOT_FOUND;
        };
        write_str_to_buf(&h.entries[idx].name, buf, buf_len, required)
    }))
    .unwrap_or(error::PANIC)
}

/// Look up the `GlobalGameEventGroupKey` for a given event key.
/// Universal across all 103 rows in 1.07 — the returned value is one
/// of 7 distinct group keys that the editor can resolve through the
/// existing `crimson_global_game_event_group_info_*` bridge.
///
/// Returns `NOT_FOUND` when the event key isn't in the table.
///
/// # Safety
/// `handle` and `out_group_key` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_global_game_event_info_lookup_group_key(
    handle: *const CrimsonGlobalGameEventInfoHandle,
    global_game_event_key: u32,
    out_group_key: *mut u32,
) -> i32 {
    if handle.is_null() || out_group_key.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_group_key = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some(&idx) = h.by_key.get(&global_game_event_key) else {
            return error::NOT_FOUND;
        };
        unsafe { *out_group_key = h.entries[idx].group_key };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Look up the 64-bit PALOC key for a given event's localized display
/// name. Returns `0` (via `*out_paloc_key`) for rows whose body lacks
/// the embedded `PalocStringRef` (the `RoyalSupply` and
/// `FactionBlockEvent_*` groups in 1.07 — ~24 of 103 rows). The caller
/// should treat 0 as "no localized name available" and fall back to
/// the internal-name surface.
///
/// When present, the key has the standard PALOC shape
/// `(hi32 = event_key) << 32 | lo32 = namespace`, suitable for direct
/// lookup through `crimson_paloc_lookup_string_key`.
///
/// Returns `NOT_FOUND` when the event key isn't in the table
/// (distinct from a present-but-zero PALOC key, which returns `OK`
/// with `*out_paloc_key = 0`).
///
/// # Safety
/// `handle` and `out_paloc_key` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_global_game_event_info_lookup_paloc_key(
    handle: *const CrimsonGlobalGameEventInfoHandle,
    global_game_event_key: u32,
    out_paloc_key: *mut u64,
) -> i32 {
    if handle.is_null() || out_paloc_key.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_paloc_key = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some(&idx) = h.by_key.get(&global_game_event_key) else {
            return error::NOT_FOUND;
        };
        unsafe { *out_paloc_key = h.entries[idx].paloc_key };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

// ── Positional getter ──────────────────────────────────────────────────────

/// Fetch the (key, name) pair at a positional index for iteration.
/// Index ordering matches the on-disk PABGH order.
///
/// # Safety
/// `handle`, `out_key`, and `required` must be non-null; `buf` may be
/// null iff `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_global_game_event_info_get_entry(
    handle: *const CrimsonGlobalGameEventInfoHandle,
    index: u32,
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
        let Some(entry) = h.entries.get(index as usize) else {
            return error::OUT_OF_RANGE;
        };
        unsafe { *out_key = entry.key };
        write_str_to_buf(&entry.name, buf, buf_len, required)
    }))
    .unwrap_or(error::PANIC)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::c_abi::error;
    use crate::c_abi::paz::crimson_paz_extract_file;
    use std::ffi::{CStr, CString};
    use std::path::PathBuf;
    use std::ptr;

    const KNOWN: &[(u32, &str)] = &[
        (0x4258, "Drought_Varnian"),
        (0x426b, "Flood_Demenissian"),
        (0x426c, "Typhoon_Delesyian"),
    ];

    /// Pinned `(key, group_key, paloc_key)` triples — mirrors the
    /// `KNOWN_BODY` table in the parser's test module so the C ABI
    /// exposes the same values.
    const KNOWN_BODY: &[(u32, u32, u64)] = &[
        (0x4258, 0x4240, 72_945_724_555_969),
        (0x426b, 0x4240, 73_027_328_934_593),
        (0x426c, 0x4240, 73_031_623_901_889),
        // 2.00 split RoyalSupply (0x424a) into four per-faction rows.
        (0x4308, 0x4241, 0), // RoyalSupply_Her — paloc absent
        (0x4309, 0x4241, 0), // RoyalSupply_Dem — paloc absent
        (0x430a, 0x4241, 0), // RoyalSupply_Del — paloc absent
        (0x430b, 0x4241, 0), // RoyalSupply_Var — paloc absent
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

    fn load_handle() -> Option<*mut CrimsonGlobalGameEventInfoHandle> {
        let pamt_path = find_pamt()?;
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let pabgb = extract_file(
            pamt.as_c_str(),
            "gamedata/binary__/client/bin",
            "globalgameevent.pabgb",
        );
        let pabgh = extract_file(
            pamt.as_c_str(),
            "gamedata/binary__/client/bin",
            "globalgameevent.pabgh",
        );
        let mut sh: *mut CrimsonGlobalGameEventInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_global_game_event_info_load_from_bytes(
                pabgb.as_ptr(),
                pabgb.len(),
                pabgh.as_ptr(),
                pabgh.len(),
                &mut sh,
            )
        };
        assert_eq!(rc, error::OK);
        Some(sh)
    }

    #[test]
    fn c_abi_global_game_event_info_live() {
        let Some(sh) = load_handle() else {
            eprintln!("skipping c_abi_global_game_event_info_live: no game install");
            return;
        };
        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_global_game_event_info_entry_count(sh, &mut count) },
            error::OK,
        );
        assert_eq!(count, 191); // 2.00 (was 188 in 1.08-1.18)
        for &(key, expected) in KNOWN {
            let mut req: usize = 0;
            assert_eq!(
                unsafe {
                    crimson_global_game_event_info_lookup_string_key(
                        sh, key, ptr::null_mut(), 0, &mut req,
                    )
                },
                error::BUFFER_TOO_SMALL,
            );
            let mut buf = vec![0u8; req];
            let mut req2: usize = 0;
            assert_eq!(
                unsafe {
                    crimson_global_game_event_info_lookup_string_key(
                        sh,
                        key,
                        buf.as_mut_ptr(),
                        buf.len(),
                        &mut req2,
                    )
                },
                error::OK,
            );
            let got = std::str::from_utf8(&buf[..req2 - 1]).unwrap();
            assert_eq!(got, expected, "key 0x{key:04x}");
        }
        unsafe { crimson_global_game_event_info_free(sh) };
    }

    /// Pin the new `group_key` + `paloc_key` lookups against the
    /// live install. Skips when the game install isn't present.
    #[test]
    fn c_abi_global_game_event_info_body_fields_live() {
        let Some(sh) = load_handle() else {
            eprintln!("skipping: no game install");
            return;
        };
        for &(key, expected_group, expected_paloc) in KNOWN_BODY {
            let mut group_key: u32 = 0;
            let rc = unsafe {
                crimson_global_game_event_info_lookup_group_key(sh, key, &mut group_key)
            };
            assert_eq!(rc, error::OK, "key 0x{key:04x} group_key rc");
            assert_eq!(
                group_key, expected_group,
                "key 0x{key:04x} group_key value",
            );

            let mut paloc_key: u64 = 0;
            let rc = unsafe {
                crimson_global_game_event_info_lookup_paloc_key(sh, key, &mut paloc_key)
            };
            assert_eq!(rc, error::OK, "key 0x{key:04x} paloc_key rc");
            assert_eq!(
                paloc_key, expected_paloc,
                "key 0x{key:04x} paloc_key value",
            );
        }
        unsafe { crimson_global_game_event_info_free(sh) };
    }

    /// Lookups must return NOT_FOUND for an unknown key without
    /// touching the out-parameters' previous values.
    #[test]
    fn c_abi_global_game_event_info_not_found() {
        let Some(sh) = load_handle() else {
            eprintln!("skipping: no game install");
            return;
        };
        let bogus = 0xFFFFu32;
        let mut group_key: u32 = 0xDEAD_BEEF;
        let rc = unsafe {
            crimson_global_game_event_info_lookup_group_key(sh, bogus, &mut group_key)
        };
        assert_eq!(rc, error::NOT_FOUND);
        // Out-parameter cleared before NOT_FOUND so callers don't see
        // stale data.
        assert_eq!(group_key, 0);

        let mut paloc_key: u64 = 0xDEAD_BEEF_DEAD_BEEF;
        let rc = unsafe {
            crimson_global_game_event_info_lookup_paloc_key(sh, bogus, &mut paloc_key)
        };
        assert_eq!(rc, error::NOT_FOUND);
        assert_eq!(paloc_key, 0);

        unsafe { crimson_global_game_event_info_free(sh) };
    }

    #[test]
    fn c_abi_global_game_event_info_null_args() {
        // Every required pointer null → NULL_ARG. Sample the new
        // lookups.
        let mut out_u32: u32 = 0;
        let rc = unsafe {
            crimson_global_game_event_info_lookup_group_key(
                ptr::null(), 0x4258, &mut out_u32,
            )
        };
        assert_eq!(rc, error::NULL_ARG);

        let mut out_u64: u64 = 0;
        let rc = unsafe {
            crimson_global_game_event_info_lookup_paloc_key(
                ptr::null(), 0x4258, &mut out_u64,
            )
        };
        assert_eq!(rc, error::NULL_ARG);
    }
}
