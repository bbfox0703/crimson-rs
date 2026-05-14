//! `missioninfo.pabgb` bridge — C ABI surface.
//!
//! Resolves save-side `MissionKey (u32)` values for the Save Editor. Two
//! resolution surfaces are exposed, both backed by the same anchor-scan
//! parser in [`crate::mission_info`]:
//!
//! 1. [`crimson_missioninfo_lookup_string_key`] — returns the internal
//!    ASCII name (e.g. `"Mission_Intro_Tutorial_I"`). Always-works
//!    fallback. Mirror of [`super::iteminfo`] /
//!    [`super::skill_info`]'s string-key getters.
//! 2. [`crimson_missioninfo_lookup_display_name`] — chains internally
//!    through `hashlittle2(name) → u64 PALOC key → PALOC string`,
//!    returning the localized display title in one FFI call.
//!    Implements the "Option B" shape from
//!    `docs/save-editor-keys-plan.md` §3 (editor's stated preference:
//!    template/chain expansion belongs in the parser, not spread
//!    across the FFI).
//!
//! Resolution chain (1.06, verified end-to-end against 7 of the user's
//! Main Quest titles — see the plan doc's "Verified hash transform"):
//!
//! ```text
//! MissionKey 1000157
//!   └─► missioninfo row → "Mission_Intro_Tutorial_I"
//!          └─► hashlittle2_c = 1891183967
//!                └─► (1891183967 << 32) | 0x101 = 8122573288984543489
//!                      └─► PALOC.lookup_str("8122573288984543489")
//!                            └─► "Unfamiliar Lands"
//! ```
//!
//! The bridge keeps only `(key, name)` from the parse; the rest of the
//! missioninfo row (rewards / HP / loc refs / etc.) is dropped. A future
//! PR can grow the handle without reshaping this API.
//!
//! Memory cost: ~4,300 missions on 1.06 × ~40 bytes per name ≈ 170 KB
//! resident plus HashMap overhead.

use std::collections::HashMap;
use std::io;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::error;
use super::paloc::CrimsonPalocHandle;
use crate::crypto::checksum::calculate_checksum;
use crate::mission_info::parse_mission_info_lossy;

/// Opaque handle exposing `(MissionKey, internal_name)` lookups against
/// the loaded `missioninfo.pabgb`. The lossy anchor-scan parse runs
/// once; the bridge retains only the pair.
#[repr(C)]
pub struct CrimsonMissionInfoHandle {
    by_key: HashMap<u32, String>,
    /// On-disk order so the caller can enumerate via
    /// [`crimson_missioninfo_get_entry`].
    entries: Vec<(u32, String)>,
}

impl CrimsonMissionInfoHandle {
    fn from_bytes(data: &[u8]) -> Self {
        let raw = parse_mission_info_lossy(data);
        // First-wins dedup so both `by_key` and `entries` agree on
        // one canonical row per key. The earlier implementation built
        // a last-wins HashMap from a duplicate-containing Vec, which
        // made `get_entry` enumeration surface duplicates even though
        // `lookup_string_key` would silently pick the winner. With
        // the scanner's UTF-8 strict check in place, real duplicates
        // are unlikely — but the dedup also guards against any
        // future parser drift.
        let mut by_key: HashMap<u32, String> = HashMap::with_capacity(raw.len());
        let mut entries: Vec<(u32, String)> = Vec::with_capacity(raw.len());
        for e in raw {
            if let std::collections::hash_map::Entry::Vacant(v) = by_key.entry(e.key) {
                v.insert(e.name.clone());
                entries.push((e.key, e.name));
            }
        }
        CrimsonMissionInfoHandle { by_key, entries }
    }
}

// ── Load / free ────────────────────────────────────────────────────────────

/// Parse `missioninfo.pabgb` from disk.
///
/// The file must be **already-decrypted, raw `.pabgb` bytes** — the
/// wrapped copy under `0008/0.paz` must come through PAZ extraction
/// first (see [`super::paz::crimson_paz_extract_file`]).
///
/// On success `*out_handle` receives an owned
/// [`CrimsonMissionInfoHandle`] that the caller releases via
/// [`crimson_missioninfo_free`].
///
/// Note: this loader is **infallible** in practice — the underlying
/// parser is anchor-scan-based and recovers gracefully from non-row
/// bytes. A genuinely corrupted input might yield zero entries, which
/// the caller would notice via [`crimson_missioninfo_entry_count`].
///
/// # Safety
/// `path` must be a NUL-terminated UTF-8 string. `out_handle` must be
/// non-null and writable for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_missioninfo_load_from_file(
    path: *const c_char,
    out_handle: *mut *mut CrimsonMissionInfoHandle,
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
        let handle = CrimsonMissionInfoHandle::from_bytes(&bytes);
        unsafe { *out_handle = Box::into_raw(Box::new(handle)) };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Parse missioninfo bytes already in memory (preferred — the editor
/// pulls them through PAZ extraction first, then calls in).
///
/// # Safety
/// `data` must point to `data_len` readable bytes (may be null iff
/// `data_len == 0`). `out_handle` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_missioninfo_load_from_bytes(
    data: *const u8,
    data_len: usize,
    out_handle: *mut *mut CrimsonMissionInfoHandle,
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
        let handle = CrimsonMissionInfoHandle::from_bytes(slice);
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
pub unsafe extern "C" fn crimson_missioninfo_free(handle: *mut CrimsonMissionInfoHandle) {
    if handle.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(handle);
    }
}

// ── Scalar getters ─────────────────────────────────────────────────────────

/// Total number of missions in the loaded table.
///
/// # Safety
/// `handle` must be live; `out_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_missioninfo_entry_count(
    handle: *const CrimsonMissionInfoHandle,
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

/// Look up the internal name for a given `MissionKey (u32)` and write
/// it into `buf` (NUL-terminated UTF-8). Two-call pattern, same shape
/// as [`super::iteminfo::crimson_iteminfo_lookup_string_key`].
///
/// Returns `NOT_FOUND` when `mission_key` isn't in the table. The
/// returned name is the raw `Mission_*` identifier from the
/// `missioninfo.pabgb` row, not a localized title. Useful as a
/// fallback when [`crimson_missioninfo_lookup_display_name`] returns
/// `NOT_FOUND` (e.g. the PALOC table doesn't have an entry for that
/// row's namespace byte).
///
/// # Safety
/// `handle` and `required` must be non-null; `buf` may be null iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_missioninfo_lookup_string_key(
    handle: *const CrimsonMissionInfoHandle,
    mission_key: u32,
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
        let Some(name) = h.by_key.get(&mission_key) else {
            return error::NOT_FOUND;
        };
        write_str_to_buf(name, buf, buf_len, required)
    }))
    .unwrap_or(error::PANIC)
}

// ── Display-name lookup (production — one-shot chain) ──────────────────────

/// One-shot title resolution: chain `MissionKey → internal name →
/// hashlittle2 → PALOC u64 key → localized string` and write the
/// result into `buf` (NUL-terminated UTF-8).
///
/// `lo32_namespace` selects the PALOC sub-namespace:
/// - `0x101` (257) — individual mission title (the common UI case)
/// - `0x100` (256) — sub-arc / region heading
/// - `0x102` (258) — description / secondary text (sparse in
///   missioninfo, denser in stageinfo)
/// - `0x490` (1168) — mission text variant; sparse, exact meaning TBD
///
/// Two-call pattern over `buf`, identical shape to all other
/// `_lookup_*` functions in this crate.
///
/// Return codes:
/// - `OK` — chain resolved end-to-end; bytes written.
/// - `BUFFER_TOO_SMALL` — first call returns this with the required
///   size in `*required`. Allocate and re-call.
/// - `NOT_FOUND` — either the `mission_key` isn't in the missioninfo
///   table, or the resulting PALOC u64 key has no entry at the
///   requested namespace. The caller may then fall back to
///   [`crimson_missioninfo_lookup_string_key`] to surface the raw
///   internal name.
/// - `NULL_ARG` — any of `handle` / `paloc_handle` / `required` is null,
///   or `buf` is null with non-zero `buf_len`.
///
/// # Safety
/// `handle`, `paloc_handle`, and `required` must point to live memory
/// for the duration of the call. `buf` (when non-null) must be writable
/// for `buf_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_missioninfo_lookup_display_name(
    handle: *const CrimsonMissionInfoHandle,
    paloc_handle: *const CrimsonPalocHandle,
    mission_key: u32,
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

        // Step 1: MissionKey → internal name
        let Some(name) = h.by_key.get(&mission_key) else {
            return error::NOT_FOUND;
        };

        // Step 2: hashlittle2(name) → u64 PALOC key
        let hi32 = calculate_checksum(name.as_bytes()) as u64;
        let u64_key = (hi32 << 32) | u64::from(lo32_namespace);

        // Step 3: PALOC keys are stored as decimal strings.
        //
        // No allocation other than the format!() — the editor side
        // calls this on per-row renders, so a small per-call alloc
        // here is cheaper than reshaping PALOC's HashMap into a u64
        // map. If it ever becomes hot we can add a parallel u64-keyed
        // index inside `CrimsonPalocHandle`.
        let decimal = format!("{u64_key}");
        let Some(display) = paloc.lookup_str(&decimal) else {
            return error::NOT_FOUND;
        };

        write_str_to_buf(display, buf, buf_len, required)
    }))
    .unwrap_or(error::PANIC)
}

// ── Enumeration ────────────────────────────────────────────────────────────

/// Get the `(mission_key, internal_name)` pair at insertion index
/// `idx`. Two-call pattern over `buf`; the `out_key` u32 is always
/// written.
///
/// Returns `OUT_OF_RANGE` when `idx >= entry_count`.
///
/// # Safety
/// `handle`, `out_key`, and `required` must be non-null; `buf` may be
/// null iff `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_missioninfo_get_entry(
    handle: *const CrimsonMissionInfoHandle,
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

// ── Helper ─────────────────────────────────────────────────────────────────

/// Two-call buffer writer used by every lookup. Pulled out so all four
/// surfaces (string_key, display_name, get_entry) share the same
/// length math and never disagree on the trailing-NUL byte.
///
/// `required` is always written to the needed size (including the
/// trailing NUL). Returns `OK` on a successful write, `BUFFER_TOO_SMALL`
/// when the caller's buffer is too short.
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
    // Safety: caller validated buf is non-null when buf_len > 0;
    // needed > 0 by construction (the trailing NUL).
    unsafe {
        std::ptr::copy_nonoverlapping(src.as_ptr(), buf, src.len());
        *buf.add(src.len()) = 0;
    }
    error::OK
}

// Suppress io::Error unused-import lint when tests aren't compiled.
const _: fn() = || {
    let _: Option<io::Error> = None;
};

#[cfg(test)]
mod tests {
    //! Live-install integration test against the real
    //! `missioninfo.pabgb` + `localizationstring_eng.paloc` pair.
    //! Skips cleanly when no Steam install is present.
    //!
    //! Pin the same seven verified mappings used by
    //! `crate::mission_info::tests` and `crate::c_abi::checksum::tests`
    //! — keeping the assertions identical at every layer means a
    //! regression at any level surfaces here without ambiguity.

    use super::*;
    use crate::c_abi::paloc::{
        crimson_paloc_free, crimson_paloc_load_from_bytes,
    };
    use crate::c_abi::paz::crimson_paz_extract_file;
    use std::ffi::{CStr, CString};
    use std::path::PathBuf;
    use std::ptr;

    /// (MissionKey, internal_name, display_title) — verified
    /// end-to-end. Same seven rows as `mission_info::tests::KNOWN`.
    const KNOWN: &[(u32, &str, &str)] = &[
        (1_000_157, "Mission_Intro_Tutorial_I", "Unfamiliar Lands"),
        (1_000_160, "Mission_Intro_MainBattle", "In Ashes"),
        (1_000_620, "Mission_Intro_Abyss_Tutorial", "Realm of Uncertainty"),
        (1_000_164, "Mission_Intro_After_Horse", "New Journey"),
        (1_000_052, "Mission_MeetAlustain_Alustain_Strength", "Where Rumors Gather"),
        (1_000_053, "Mission_MeetAlustain_Alustain_Wisdom", "Mysterious Man"),
        (
            1_000_083,
            "Mission_IronStronghold_Block_ReturnToSister",
            "Where the Wind Guides You",
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
    fn c_abi_missioninfo_live_full_chain() {
        let Some(pamt_path) = find_pamt() else {
            eprintln!("skipping c_abi_missioninfo_live_full_chain: no game install");
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let mission_bytes = extract_file(pamt.as_c_str(), "gamedata/binary__/client/bin", "missioninfo.pabgb");

        // Load the missioninfo bridge.
        let mut mh: *mut CrimsonMissionInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_missioninfo_load_from_bytes(
                mission_bytes.as_ptr(),
                mission_bytes.len(),
                &mut mh,
            )
        };
        assert_eq!(rc, error::OK);
        assert!(!mh.is_null());

        // Sanity: entry count is plausibly populated.
        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_missioninfo_entry_count(mh, &mut count) },
            error::OK
        );
        assert!(count > 1_000, "expected >1000 missions, got {count}");

        // Pull eng paloc through the same PAZ pipeline.
        // 0020 is the typical English group; the existing test for
        // paloc bridge uses the same path.
        let paloc_dir = CString::new("gamedata/stringtable/binary__").unwrap();
        let paloc_name = CString::new("localizationstring_eng.paloc").unwrap();
        let paloc_pamt = {
            let mut p = pamt_path.clone();
            p.pop(); // drop "0.pamt"
            p.pop(); // drop "0008"
            p.push("0020");
            p.push("0.pamt");
            p
        };
        let Some(paloc_pamt) = paloc_pamt.is_file().then_some(paloc_pamt) else {
            eprintln!("skipping paloc chain: no 0020/0.pamt");
            unsafe { crimson_missioninfo_free(mh) };
            return;
        };
        let paloc_pamt_c = CString::new(paloc_pamt.to_str().unwrap()).unwrap();
        let mut needed: usize = 0;
        let rc = unsafe {
            crimson_paz_extract_file(
                paloc_pamt_c.as_ptr(),
                paloc_dir.as_ptr(),
                paloc_name.as_ptr(),
                ptr::null_mut(),
                0,
                &mut needed,
            )
        };
        assert_eq!(rc, error::BUFFER_TOO_SMALL);
        let mut paloc_buf = vec![0u8; needed];
        let rc = unsafe {
            crimson_paz_extract_file(
                paloc_pamt_c.as_ptr(),
                paloc_dir.as_ptr(),
                paloc_name.as_ptr(),
                paloc_buf.as_mut_ptr(),
                paloc_buf.len(),
                &mut needed,
            )
        };
        assert_eq!(rc, error::OK);
        paloc_buf.truncate(needed);

        let mut ph: *mut CrimsonPalocHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_paloc_load_from_bytes(
                paloc_buf.as_ptr(),
                paloc_buf.len(),
                &mut ph,
            )
        };
        assert_eq!(rc, error::OK);
        assert!(!ph.is_null());

        // Walk every known (key, name, title) and assert both surfaces
        // produce the expected output.
        for &(key, expected_name, expected_title) in KNOWN {
            // string_key — first call to size, then to fill.
            let mut req: usize = 0;
            let rc_size = unsafe {
                crimson_missioninfo_lookup_string_key(
                    mh,
                    key,
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            };
            let got_name = read_string_result(rc_size, req, |b, n, r| unsafe {
                crimson_missioninfo_lookup_string_key(mh, key, b, n, r)
            });
            assert_eq!(
                got_name, expected_name,
                "MissionKey {key} internal name mismatch"
            );

            // display_name — full chain.
            let mut req2: usize = 0;
            let rc_size = unsafe {
                crimson_missioninfo_lookup_display_name(
                    mh,
                    ph,
                    key,
                    0x101,
                    ptr::null_mut(),
                    0,
                    &mut req2,
                )
            };
            let got_title = read_string_result(rc_size, req2, |b, n, r| unsafe {
                crimson_missioninfo_lookup_display_name(mh, ph, key, 0x101, b, n, r)
            });
            assert_eq!(
                got_title, expected_title,
                "MissionKey {key} display title mismatch (expected {expected_title:?})"
            );
        }

        // Negative paths.
        let mut req: usize = 0;
        let rc = unsafe {
            crimson_missioninfo_lookup_string_key(
                mh,
                u32::MAX,
                ptr::null_mut(),
                0,
                &mut req,
            )
        };
        assert_eq!(rc, error::NOT_FOUND);

        let rc = unsafe {
            crimson_missioninfo_lookup_display_name(
                mh,
                ph,
                u32::MAX,
                0x101,
                ptr::null_mut(),
                0,
                &mut req,
            )
        };
        assert_eq!(rc, error::NOT_FOUND);

        // Missing PALOC entry — pick a valid MissionKey but a nonsense
        // namespace byte. Should return NOT_FOUND from the second hop.
        let rc = unsafe {
            crimson_missioninfo_lookup_display_name(
                mh,
                ph,
                1_000_157,
                0xDEADBEEF,
                ptr::null_mut(),
                0,
                &mut req,
            )
        };
        assert_eq!(rc, error::NOT_FOUND);

        unsafe { crimson_missioninfo_free(mh) };
        unsafe { crimson_paloc_free(ph) };
    }

    /// Regression for issue `001-missioninfo-invalid-utf8-names` from
    /// the CrimsonAtomtic editor outbox. The earlier last-wins
    /// HashMap + duplicate-Vec layout in
    /// `CrimsonMissionInfoHandle::from_bytes` made
    /// `crimson_missioninfo_get_entry` surface the same MissionKey
    /// multiple times (the editor's probe reported
    /// `MissionKey 1000038` appearing 4× in the first 5 samples).
    /// After the fix, `get_entry` enumeration is one row per key.
    #[test]
    fn c_abi_missioninfo_get_entry_no_duplicates() {
        let Some(pamt_path) = find_pamt() else {
            eprintln!("skipping c_abi_missioninfo_get_entry_no_duplicates: no game install");
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let mission_bytes = extract_file(
            pamt.as_c_str(),
            "gamedata/binary__/client/bin",
            "missioninfo.pabgb",
        );
        let mut mh: *mut CrimsonMissionInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_missioninfo_load_from_bytes(
                mission_bytes.as_ptr(),
                mission_bytes.len(),
                &mut mh,
            )
        };
        assert_eq!(rc, error::OK);

        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_missioninfo_entry_count(mh, &mut count) },
            error::OK
        );

        let mut seen: std::collections::HashSet<u32> = std::collections::HashSet::new();
        for idx in 0..count {
            let mut key: u32 = 0;
            let mut req: usize = 0;
            // Size-only call returns BUFFER_TOO_SMALL with the required
            // size — fine for this test, we don't need the name.
            let _ = unsafe {
                crimson_missioninfo_get_entry(mh, idx, &mut key, ptr::null_mut(), 0, &mut req)
            };
            assert!(
                seen.insert(key),
                "duplicate MissionKey {key} at index {idx} from get_entry"
            );
        }

        unsafe { crimson_missioninfo_free(mh) };
    }

    #[test]
    fn c_abi_missioninfo_null_args() {
        let mut mh: *mut CrimsonMissionInfoHandle = ptr::null_mut();
        // Null data with non-zero len.
        assert_eq!(
            unsafe { crimson_missioninfo_load_from_bytes(ptr::null(), 16, &mut mh) },
            error::NULL_ARG,
        );
        // Null out_handle.
        assert_eq!(
            unsafe {
                crimson_missioninfo_load_from_bytes([0u8; 1].as_ptr(), 1, ptr::null_mut())
            },
            error::NULL_ARG,
        );

        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_missioninfo_entry_count(ptr::null(), &mut count) },
            error::NULL_ARG,
        );

        let mut req: usize = 0;
        assert_eq!(
            unsafe {
                crimson_missioninfo_lookup_string_key(
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
                crimson_missioninfo_lookup_display_name(
                    ptr::null(),
                    ptr::null(),
                    0,
                    0x101,
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            },
            error::NULL_ARG,
        );
    }

    #[test]
    fn c_abi_missioninfo_empty_bytes_yields_empty_handle() {
        // Anchor scanner finds zero rows in empty input. The bridge
        // should succeed and report count=0 (vs erroring) — this lets
        // callers handle "no missioninfo loaded" as a soft state.
        let mut mh: *mut CrimsonMissionInfoHandle = ptr::null_mut();
        let rc = unsafe { crimson_missioninfo_load_from_bytes(ptr::null(), 0, &mut mh) };
        assert_eq!(rc, error::OK);
        assert!(!mh.is_null());
        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_missioninfo_entry_count(mh, &mut count) },
            error::OK
        );
        assert_eq!(count, 0);
        unsafe { crimson_missioninfo_free(mh) };
    }

    #[test]
    fn c_abi_missioninfo_load_bad_path_returns_io() {
        let bad = CString::new("Z:\\definitely\\does\\not\\exist.pabgb").unwrap();
        let mut mh: *mut CrimsonMissionInfoHandle = ptr::null_mut();
        let rc = unsafe { crimson_missioninfo_load_from_file(bad.as_ptr(), &mut mh) };
        assert_eq!(rc, error::IO);
        assert!(mh.is_null());
    }
}
