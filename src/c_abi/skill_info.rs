//! `skill.pabgb` + `skill.pabgh` bridge — C ABI surface.
//!
//! Parses an in-memory skill data pair (after PAZ extraction from group
//! `0008`, directory `gamedata/binary__/client/bin/`) and exposes the
//! single primitive the Save Editor needs: map a `SkillKey (u32)` to its
//! entry name (a BString-shape ASCII identifier like `"PowerStrike"` or
//! `"BatPunch"`). The caller then prefixes the name into a PALOC lookup
//! (e.g. `"SkillName_PowerStrike"`) to obtain a localized display name —
//! exactly the same two-hop pattern that [`super::iteminfo`] uses for
//! `ItemKey → string_key → display name`.
//!
//! Why this exists separately from the PyO3 surface in `python.rs`
//! ---------------------------------------------------------------
//! The PyO3 binding already exposes the full `SkillEntry` graph (buff
//! matrix, post-buff resources, format flag, etc.) — appropriate for
//! Python tooling. The C ABI consumer (Avalonia Save Editor) only needs
//! the `(key, name)` lookup. Carrying the full graph across the ABI
//! would mean wiring up ~30 nested structs; instead this bridge runs the
//! parse once, retains only the pair, and drops the rest. Same trade as
//! [`super::iteminfo`].
//!
//! Memory cost: ~250 skills on 1.06 × ~20 bytes per name ≈ 5 KB resident
//! plus HashMap overhead. Negligible compared to the iteminfo bridge.

use std::collections::HashMap;
use std::io;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::error;
use crate::skill_info::SkillData;

/// Opaque handle exposing lean per-skill lookups against the loaded
/// `skill.pabgb` + `skill.pabgh` pair. The full `SkillData` parse runs
/// once; only `(key, name)` is retained.
#[repr(C)]
pub struct CrimsonSkillInfoHandle {
    by_key: HashMap<u32, String>,
    /// `(key, name)` in on-disk PABGH order so the caller can enumerate
    /// via [`crimson_skillinfo_get_entry`]. The order is stable and
    /// matches what the game itself sees when it iterates the skill
    /// index — useful when correlating against debugger output.
    entries: Vec<(u32, String)>,
}

impl CrimsonSkillInfoHandle {
    fn from_bytes(pabgh: &[u8], pabgb: &[u8]) -> io::Result<Self> {
        let data = SkillData::parse(pabgh, pabgb)?;
        let mut entries: Vec<(u32, String)> = Vec::with_capacity(data.entries.len());
        for entry in &data.entries {
            // `name_bytes` is the BString-shape entry name. Internal
            // names are ASCII by convention (`"PowerStrike"`, etc.);
            // `from_utf8_lossy` is defensive — a non-UTF-8 byte in a
            // future format wouldn't crash the bridge, it would just
            // surface as `U+FFFD` in the returned string. The caller
            // would notice and we'd revisit.
            let name = String::from_utf8_lossy(&entry.name_bytes).into_owned();
            entries.push((entry.key, name));
        }
        let by_key = entries.iter().cloned().collect();
        Ok(CrimsonSkillInfoHandle { by_key, entries })
    }
}

// ── Load / free ────────────────────────────────────────────────────────────

/// Parse `skill.pabgh` + `skill.pabgb` from disk.
///
/// Both files must be **already-decrypted, raw bytes** — the wrapped
/// copies under `0008/0.paz` need to come through PAZ extraction first
/// (see [`super::paz::crimson_paz_extract_file`]).
///
/// On success `*out_handle` receives an owned [`CrimsonSkillInfoHandle`]
/// that the caller must release via [`crimson_skillinfo_free`].
///
/// # Safety
/// `pabgh_path` and `pabgb_path` must be NUL-terminated UTF-8 strings
/// and `out_handle` must point at writable memory for the duration of
/// the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_skillinfo_load_from_file(
    pabgh_path: *const c_char,
    pabgb_path: *const c_char,
    out_handle: *mut *mut CrimsonSkillInfoHandle,
) -> i32 {
    if pabgh_path.is_null() || pabgb_path.is_null() || out_handle.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_handle = std::ptr::null_mut() };
    catch_unwind(AssertUnwindSafe(|| {
        let pabgh_str = match unsafe { std::ffi::CStr::from_ptr(pabgh_path) }.to_str() {
            Ok(s) => s,
            Err(_) => return error::INVALID_PATH,
        };
        let pabgb_str = match unsafe { std::ffi::CStr::from_ptr(pabgb_path) }.to_str() {
            Ok(s) => s,
            Err(_) => return error::INVALID_PATH,
        };
        let pabgh_bytes = match std::fs::read(pabgh_str) {
            Ok(b) => b,
            Err(_) => return error::IO,
        };
        let pabgb_bytes = match std::fs::read(pabgb_str) {
            Ok(b) => b,
            Err(_) => return error::IO,
        };
        let handle = match CrimsonSkillInfoHandle::from_bytes(&pabgh_bytes, &pabgb_bytes) {
            Ok(h) => h,
            Err(_) => return error::BODY_PARSE,
        };
        unsafe { *out_handle = Box::into_raw(Box::new(handle)) };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Parse skill bytes already in memory (preferred — the editor pulls
/// them through PAZ extraction first, then calls in).
///
/// # Safety
/// `pabgh_data` must point to `pabgh_len` readable bytes (may be null
/// iff `pabgh_len == 0`); same for `pabgb_data` / `pabgb_len`.
/// `out_handle` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_skillinfo_load_from_bytes(
    pabgh_data: *const u8,
    pabgh_len: usize,
    pabgb_data: *const u8,
    pabgb_len: usize,
    out_handle: *mut *mut CrimsonSkillInfoHandle,
) -> i32 {
    if out_handle.is_null() {
        return error::NULL_ARG;
    }
    if pabgh_data.is_null() && pabgh_len != 0 {
        return error::NULL_ARG;
    }
    if pabgb_data.is_null() && pabgb_len != 0 {
        return error::NULL_ARG;
    }
    unsafe { *out_handle = std::ptr::null_mut() };
    catch_unwind(AssertUnwindSafe(|| {
        let pabgh_slice = if pabgh_len == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(pabgh_data, pabgh_len) }
        };
        let pabgb_slice = if pabgb_len == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(pabgb_data, pabgb_len) }
        };
        let handle = match CrimsonSkillInfoHandle::from_bytes(pabgh_slice, pabgb_slice) {
            Ok(h) => h,
            Err(_) => return error::BODY_PARSE,
        };
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
pub unsafe extern "C" fn crimson_skillinfo_free(handle: *mut CrimsonSkillInfoHandle) {
    if handle.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(handle);
    }
}

// ── Scalar getters ─────────────────────────────────────────────────────────

/// Total number of skills in the loaded pair.
///
/// # Safety
/// `handle` must be live; `out_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_skillinfo_entry_count(
    handle: *const CrimsonSkillInfoHandle,
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

// ── Lookup ─────────────────────────────────────────────────────────────────

/// Look up the entry name for a given `SkillKey (u32)` and write it
/// into `buf` (NUL-terminated UTF-8). Two-call pattern, identical
/// shape to [`super::iteminfo::crimson_iteminfo_lookup_string_key`]:
///
/// - First call with `buf = null, buf_len = 0` returns
///   `BUFFER_TOO_SMALL` and sets `*required` (includes trailing NUL).
/// - Allocate, call again to receive the bytes and `OK`.
///
/// Returns `NOT_FOUND` when `skill_key` doesn't match any entry in the
/// loaded table.
///
/// The returned name is the **internal** identifier from `skill.pabgb`,
/// not a localized display name. The downstream editor combines it with
/// PALOC (typically by prefixing `"SkillName_"`, but the exact key
/// convention is the editor's responsibility, not this bridge's).
///
/// # Safety
/// `handle` and `required` must be non-null; `buf` may be null iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_skillinfo_lookup_string_key(
    handle: *const CrimsonSkillInfoHandle,
    skill_key: u32,
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
        let Some(name) = h.by_key.get(&skill_key) else {
            return error::NOT_FOUND;
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

// ── Enumeration ────────────────────────────────────────────────────────────

/// Get the `(skill_key, name)` pair at PABGH on-disk index `idx`.
/// Two-call pattern over `buf`; the `out_key` u32 is always written.
///
/// Returns `OUT_OF_RANGE` when `idx >= entry_count`.
///
/// # Safety
/// `handle`, `out_key`, and `required` must be non-null; `buf` may
/// be null iff `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_skillinfo_get_entry(
    handle: *const CrimsonSkillInfoHandle,
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
    //! Live-install integration tests against the real `skill.pabgb` +
    //! `skill.pabgh` pair. Skip cleanly when no Steam install is present.
    //! Synthesizing skill bytes from scratch is impractical (the buff
    //! tail-size probe needs realistic type_id values across the full
    //! 0..=119 range) so we rely on round-tripping the real bytes through
    //! PAZ extraction + the new C ABI.
    //!
    //! Coverage for the C ABI's error paths (NULL_ARG, BODY_PARSE on
    //! garbage, OUT_OF_RANGE, NOT_FOUND) uses synthetic inputs that
    //! exercise the wrappers without needing valid skill bytes.

    use crate::binary::gamedata_layout;
    use super::*;
    use crate::c_abi::paz::crimson_paz_extract_file;
    use std::ffi::{CStr, CString};
    use std::path::PathBuf;
    use std::ptr;

    fn find_pamt_for_skillinfo() -> Option<PathBuf> {
        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let p = game_root.join("0008").join("0.pamt");
        p.is_file().then_some(p)
    }

    /// Pull one of the skill files via the standard PAZ path.
    fn extract_skill_bytes(pamt: &CStr, file_name: &str) -> Vec<u8> {
        let dir = CString::new(gamedata_layout::bin_dir()).unwrap();
        let name = CString::new(file_name).unwrap();
        let mut needed: usize = 0;
        let rc = unsafe {
            crimson_paz_extract_file(
                pamt.as_ptr(),
                dir.as_ptr(),
                name.as_ptr(),
                ptr::null_mut(),
                0,
                &mut needed,
            )
        };
        assert_eq!(rc, error::BUFFER_TOO_SMALL, "first call should query size");
        let mut buf = vec![0u8; needed];
        let rc = unsafe {
            crimson_paz_extract_file(
                pamt.as_ptr(),
                dir.as_ptr(),
                name.as_ptr(),
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
    fn c_abi_skillinfo_live_roundtrip() {
        let Some(pamt_path) = find_pamt_for_skillinfo() else {
            eprintln!(
                "skipping c_abi_skillinfo_live_roundtrip: no 0008/0.pamt in game install"
            );
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let pabgh = extract_skill_bytes(pamt.as_c_str(), &gamedata_layout::header("skill"));
        let pabgb = extract_skill_bytes(pamt.as_c_str(), &gamedata_layout::body("skill"));

        let mut handle: *mut CrimsonSkillInfoHandle = ptr::null_mut();
        assert_eq!(
            unsafe {
                crimson_skillinfo_load_from_bytes(
                    pabgh.as_ptr(),
                    pabgh.len(),
                    pabgb.as_ptr(),
                    pabgb.len(),
                    &mut handle,
                )
            },
            error::OK
        );
        assert!(!handle.is_null());

        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_skillinfo_entry_count(handle, &mut count) },
            error::OK
        );
        // 1.05 / 1.06 land somewhere around 250–300 skills. Just assert
        // plausibly populated; pinning a specific count would break on
        // every patch.
        assert!(count > 100, "expected >100 skills, got {count}");

        // ── Round-trip: pick entry 0, then look up by the key we got.
        let mut out_key: u32 = 0;
        let mut req: usize = 0;
        let rc = unsafe {
            crimson_skillinfo_get_entry(
                handle,
                0,
                &mut out_key,
                ptr::null_mut(),
                0,
                &mut req,
            )
        };
        assert_eq!(rc, error::BUFFER_TOO_SMALL);
        let mut buf = vec![0u8; req];
        let rc = unsafe {
            crimson_skillinfo_get_entry(
                handle,
                0,
                &mut out_key,
                buf.as_mut_ptr(),
                buf.len(),
                &mut req,
            )
        };
        assert_eq!(rc, error::OK);
        let enum_name = std::str::from_utf8(&buf[..req - 1]).unwrap().to_string();
        assert!(!enum_name.is_empty(), "skill 0's name should be non-empty");

        // Now lookup by the u32 we just read.
        let mut req2: usize = 0;
        let rc = unsafe {
            crimson_skillinfo_lookup_string_key(
                handle,
                out_key,
                ptr::null_mut(),
                0,
                &mut req2,
            )
        };
        assert_eq!(rc, error::BUFFER_TOO_SMALL);
        let mut buf2 = vec![0u8; req2];
        let rc = unsafe {
            crimson_skillinfo_lookup_string_key(
                handle,
                out_key,
                buf2.as_mut_ptr(),
                buf2.len(),
                &mut req2,
            )
        };
        assert_eq!(rc, error::OK);
        let lookup_name = std::str::from_utf8(&buf2[..req2 - 1]).unwrap();
        assert_eq!(
            lookup_name, enum_name,
            "get_entry and lookup must agree for the same key"
        );

        // NOT_FOUND on a definitely-invalid skill key. u32::MAX is safely
        // out of bounds for any realistic game data.
        let mut req3: usize = 0;
        let rc = unsafe {
            crimson_skillinfo_lookup_string_key(
                handle,
                u32::MAX,
                ptr::null_mut(),
                0,
                &mut req3,
            )
        };
        assert_eq!(rc, error::NOT_FOUND);

        // OUT_OF_RANGE on get_entry past the end.
        let rc = unsafe {
            crimson_skillinfo_get_entry(
                handle,
                u32::MAX,
                &mut out_key,
                ptr::null_mut(),
                0,
                &mut req,
            )
        };
        assert_eq!(rc, error::OUT_OF_RANGE);

        unsafe { crimson_skillinfo_free(handle) };
        // free(null) is a no-op.
        unsafe { crimson_skillinfo_free(ptr::null_mut()) };
    }

    #[test]
    fn c_abi_skillinfo_garbage_bytes_returns_body_parse() {
        // 32 zero bytes won't parse as a SkillData pair — the pabgh
        // header check fails immediately.
        let garbage = [0u8; 32];
        let mut handle: *mut CrimsonSkillInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_skillinfo_load_from_bytes(
                garbage.as_ptr(),
                garbage.len(),
                garbage.as_ptr(),
                garbage.len(),
                &mut handle,
            )
        };
        assert_eq!(rc, error::BODY_PARSE);
        assert!(handle.is_null());
    }

    #[test]
    fn c_abi_skillinfo_null_args() {
        let mut handle: *mut CrimsonSkillInfoHandle = ptr::null_mut();
        // null data with nonzero len → NULL_ARG (on either side).
        assert_eq!(
            unsafe {
                crimson_skillinfo_load_from_bytes(
                    ptr::null(),
                    16,
                    [0u8; 1].as_ptr(),
                    1,
                    &mut handle,
                )
            },
            error::NULL_ARG
        );
        assert_eq!(
            unsafe {
                crimson_skillinfo_load_from_bytes(
                    [0u8; 1].as_ptr(),
                    1,
                    ptr::null(),
                    16,
                    &mut handle,
                )
            },
            error::NULL_ARG
        );
        // null out_handle → NULL_ARG even with valid data pointers.
        assert_eq!(
            unsafe {
                crimson_skillinfo_load_from_bytes(
                    [0u8; 1].as_ptr(),
                    1,
                    [0u8; 1].as_ptr(),
                    1,
                    ptr::null_mut(),
                )
            },
            error::NULL_ARG
        );

        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_skillinfo_entry_count(ptr::null(), &mut count) },
            error::NULL_ARG
        );

        let mut req: usize = 0;
        assert_eq!(
            unsafe {
                crimson_skillinfo_lookup_string_key(
                    ptr::null(),
                    0,
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            },
            error::NULL_ARG
        );
    }

    #[test]
    fn c_abi_skillinfo_load_bad_path_returns_io() {
        let bad_pabgh = CString::new(r"Z:\nope\skill.pabgh").unwrap();
        let bad_pabgb = CString::new(r"Z:\nope\skill.pabgb").unwrap();
        let mut handle: *mut CrimsonSkillInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_skillinfo_load_from_file(
                bad_pabgh.as_ptr(),
                bad_pabgb.as_ptr(),
                &mut handle,
            )
        };
        assert_eq!(rc, error::IO);
        assert!(handle.is_null());
    }
}
