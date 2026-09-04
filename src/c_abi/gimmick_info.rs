//! `gimmickinfo.pabgb` bridge — C ABI surface.
//!
//! Resolves save-side `GimmickInfoKey (u32)` (the
//! `FieldGimmickSaveData._gimmickInfoKey` sibling) to either:
//!
//! 1. The gamedata row's internal name
//!    (e.g. `"gimmick_cart_middle_03"`, `"Background_Lamp_16"`,
//!    `"mine_copper"`) via
//!    [`crimson_gimmickinfo_lookup_string_key`].
//! 2. A localized display string via
//!    [`crimson_gimmickinfo_lookup_display_name`] — chains directly
//!    into PALOC at `(gimmick_key << 32) | lo32_namespace`. Unlike
//!    mission/quest/stage/knowledge, **no hash hop** — the save's
//!    `_gimmickInfoKey` is already the PALOC hi32 verbatim. The
//!    common namespace bytes:
//!    - `0x200` (512) — gimmick display label (e.g. "Fire",
//!      "Prison", "Broken Box"). Default for the lookup.
//!    - `0x19202` (102914) — gimmick description (sparse — ~9 rows
//!      have one in the live install, used for furniture inspect text).
//!    - `0x60` (96) — interaction verb (Move/Skin/Load/Open).
//!    - Various `0x30`/`0x70`/`0x71`/etc. hits are coincidental
//!      collisions with character / item tables that share the
//!      small-integer key space; the caller selects which namespace
//!      they trust.
//!
//! Coverage observed on the 1.06 sample save: 527/530 (99.4%) of
//! `_gimmickInfoKey` values resolve through the chain. The 3 misses
//! are likely test/dev gimmicks without paloc entries OR rows the
//! anchor scanner couldn't isolate from surrounding body noise.
//!
//! See `docs/save-editor-keys-plan.md` §7 for the rationale and the
//! probe that confirmed the no-hash-hop resolution shape.

use std::collections::HashMap;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::error;
use super::paloc::CrimsonPalocHandle;
use crate::gimmick_info::parse_gimmick_info_lossy;

/// Opaque handle exposing `(GimmickInfoKey, internal_name)` lookups
/// against the loaded `gimmickinfo.pabgb`.
#[repr(C)]
pub struct CrimsonGimmickInfoHandle {
    by_key: HashMap<u32, String>,
    entries: Vec<(u32, String)>,
}

impl CrimsonGimmickInfoHandle {
    fn from_bytes(data: &[u8]) -> Self {
        let raw = parse_gimmick_info_lossy(data);
        // First-wins dedup so a real row keeps its slot when a later
        // body-byte collision shares the key.
        let mut by_key: HashMap<u32, String> = HashMap::with_capacity(raw.len());
        let mut entries: Vec<(u32, String)> = Vec::with_capacity(raw.len());
        for e in raw {
            if let std::collections::hash_map::Entry::Vacant(v) = by_key.entry(e.key) {
                v.insert(e.name.clone());
                entries.push((e.key, e.name));
            }
        }
        CrimsonGimmickInfoHandle { by_key, entries }
    }
}

// ── Load / free ────────────────────────────────────────────────────────────

/// Parse `gimmickinfo.pabgb` from disk.
///
/// # Safety
/// `path` must be a NUL-terminated UTF-8 string. `out_handle` must be
/// non-null and writable for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_gimmickinfo_load_from_file(
    path: *const c_char,
    out_handle: *mut *mut CrimsonGimmickInfoHandle,
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
        let handle = CrimsonGimmickInfoHandle::from_bytes(&bytes);
        unsafe { *out_handle = Box::into_raw(Box::new(handle)) };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Parse gimmickinfo bytes already in memory.
///
/// # Safety
/// `data` must point to `data_len` readable bytes (may be null iff
/// `data_len == 0`). `out_handle` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_gimmickinfo_load_from_bytes(
    data: *const u8,
    data_len: usize,
    out_handle: *mut *mut CrimsonGimmickInfoHandle,
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
        let handle = CrimsonGimmickInfoHandle::from_bytes(slice);
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
pub unsafe extern "C" fn crimson_gimmickinfo_free(handle: *mut CrimsonGimmickInfoHandle) {
    if handle.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(handle);
    }
}

// ── Scalar getters ─────────────────────────────────────────────────────────

/// Total number of gimmicks in the loaded table.
///
/// # Safety
/// `handle` must be live; `out_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_gimmickinfo_entry_count(
    handle: *const CrimsonGimmickInfoHandle,
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

/// Look up the internal name for a given `GimmickInfoKey (u32)` and
/// write it into `buf` (NUL-terminated UTF-8). Two-call pattern.
///
/// Returns `NOT_FOUND` when `gimmick_key` isn't in the table.
///
/// # Safety
/// `handle` and `required` must be non-null; `buf` may be null iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_gimmickinfo_lookup_string_key(
    handle: *const CrimsonGimmickInfoHandle,
    gimmick_key: u32,
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
        let Some(name) = h.by_key.get(&gimmick_key) else {
            return error::NOT_FOUND;
        };
        write_str_to_buf(name, buf, buf_len, required)
    }))
    .unwrap_or(error::PANIC)
}

// ── Display-name lookup (production — one-shot chain) ──────────────────────

/// One-shot display name resolution: `GimmickInfoKey → PALOC at
/// `(gimmick_key << 32) | lo32_namespace` → localized string`. Writes
/// the result into `buf` (NUL-terminated UTF-8).
///
/// Unlike mission/quest/stage/knowledge bridges, **no hash hop**.
/// The save's `_gimmickInfoKey` is already the PALOC hi32 directly.
/// This bridge does NOT need to consult the gimmickinfo handle at
/// resolve time (the handle is purely for the `lookup_string_key`
/// fallback), but the parameter is retained for API symmetry with
/// the sibling bridges and to allow a future change without an ABI
/// break.
///
/// `lo32_namespace` selects the PALOC sub-namespace:
/// - `0x200` (512) — gimmick display label (e.g. "Fire", "Prison",
///   "Broken Box"). The common UI case.
/// - `0x19202` (102914) — gimmick description (sparse — only ~9 of
///   530 sample keys have one; furniture / artifact inspect text).
/// - `0x60` (96) — interaction verb (Move/Skin/Load/Open).
///
/// Return codes:
/// - `OK` — chain resolved; bytes written.
/// - `BUFFER_TOO_SMALL` — first call returns this with the required
///   size in `*required`. Allocate and re-call.
/// - `NOT_FOUND` — the resulting PALOC u64 key has no entry at the
///   requested namespace.
/// - `NULL_ARG` — any of `handle` / `paloc_handle` / `required` is
///   null, or `buf` is null with non-zero `buf_len`.
///
/// # Safety
/// `handle`, `paloc_handle`, and `required` must point to live memory
/// for the duration of the call. `buf` (when non-null) must be
/// writable for `buf_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_gimmickinfo_lookup_display_name(
    handle: *const CrimsonGimmickInfoHandle,
    paloc_handle: *const CrimsonPalocHandle,
    gimmick_key: u32,
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
        // The gimmickinfo handle is intentionally not consulted — the
        // save's _gimmickInfoKey is already the PALOC hi32 directly,
        // no internal-name → hashlittle2 detour needed (unlike
        // mission/quest/stage/knowledge). We still validate that the
        // handle is live (above) so the surface matches its siblings
        // and future cat-byte transforms can hook in here without
        // an ABI change.
        let paloc = unsafe { &*paloc_handle };

        let u64_key = (u64::from(gimmick_key) << 32) | u64::from(lo32_namespace);
        let decimal = format!("{u64_key}");
        let Some(display) = paloc.lookup_str(&decimal) else {
            return error::NOT_FOUND;
        };

        write_str_to_buf(display, buf, buf_len, required)
    }))
    .unwrap_or(error::PANIC)
}

// ── Enumeration ────────────────────────────────────────────────────────────

/// Get the `(gimmick_key, internal_name)` pair at insertion index `idx`.
///
/// # Safety
/// `handle`, `out_key`, and `required` must be non-null; `buf` may be
/// null iff `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_gimmickinfo_get_entry(
    handle: *const CrimsonGimmickInfoHandle,
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
    //! Live-install integration test against `gimmickinfo.pabgb` +
    //! `localizationstring_eng.paloc`. Asserts both surfaces work
    //! end-to-end, plus negative paths and NULL handling.

    use crate::binary::gamedata_layout;
    use super::*;
    use crate::c_abi::paloc::{crimson_paloc_free, crimson_paloc_load_from_bytes};
    use crate::c_abi::paz::crimson_paz_extract_file;
    use std::ffi::{CStr, CString};
    use std::path::PathBuf;
    use std::ptr;

    /// (GimmickInfoKey, internal_name, display_name @ lo32=0x200).
    /// Each row is from the 2026-05-14 probe pass — both the internal
    /// name in gimmickinfo and the PALOC display name at lo32=0x200
    /// independently verified against the live install.
    const KNOWN: &[(u32, &str, &str)] = &[
        // cat=0x00 — generic gimmicks
        (0x000f_4257, "(see lookup_string_key)", "Prison"),
        (0x000f_427a, "(see lookup_string_key)", "Broken Box"),
        // cat=0x00, small key — Fire gimmick cluster
        (0x0008_608a, "(see lookup_string_key)", "Fire"),
        (0x0008_608b, "(see lookup_string_key)", "Fire"),
        (0x0008_6473, "(see lookup_string_key)", "Fire"),
    ];

    /// (GimmickInfoKey, internal_name only — for lookup_string_key tests).
    /// Pulled from the strict pass through the scanner, so the test
    /// also exercises the cat-byte loose-hi rule (the 0x09 entry).
    const INTERNAL_KNOWN: &[(u32, &str)] = &[
        (0x094c_861e, "gimmick_cart_middle_03"),
        (0x000f_6265, "gimmick_tool_dryingrope_herb_0002"),
        (0x000f_569e, "gimmick_trap_rope_03m_0001"),
        (0x0103_db72, "mine_copper"),
        (0x0053_9e50, "Background_Lamp_16"),
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
    fn c_abi_gimmickinfo_live_full_chain() {
        let Some(pamt_path) = find_pamt() else {
            eprintln!("skipping c_abi_gimmickinfo_live_full_chain: no game install");
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let gimmick_bytes = extract_file(
            pamt.as_c_str(),
            gamedata_layout::bin_dir(),
            &gamedata_layout::body("gimmickinfo"),
        );

        let mut gh: *mut CrimsonGimmickInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_gimmickinfo_load_from_bytes(
                gimmick_bytes.as_ptr(),
                gimmick_bytes.len(),
                &mut gh,
            )
        };
        assert_eq!(rc, error::OK);
        assert!(!gh.is_null());

        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_gimmickinfo_entry_count(gh, &mut count) },
            error::OK
        );
        assert!(count > 2_000, "expected >2000 gimmicks, got {count}");

        // ── lookup_string_key — internal name from gimmickinfo ────────────
        for &(key, expected_name) in INTERNAL_KNOWN {
            let mut req: usize = 0;
            let rc_size = unsafe {
                crimson_gimmickinfo_lookup_string_key(
                    gh,
                    key,
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            };
            let got = read_string_result(rc_size, req, |b, n, r| unsafe {
                crimson_gimmickinfo_lookup_string_key(gh, key, b, n, r)
            });
            assert_eq!(
                got, expected_name,
                "GimmickInfoKey 0x{key:08x} internal name mismatch"
            );
        }

        // Pull eng PALOC.
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
            unsafe { crimson_gimmickinfo_free(gh) };
            return;
        }
        let paloc_buf = gamedata_layout::paloc_bytes("0020", "eng")
            .expect("eng paloc must load from 0020");

        let mut ph: *mut CrimsonPalocHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_paloc_load_from_bytes(paloc_buf.as_ptr(), paloc_buf.len(), &mut ph)
        };
        assert_eq!(rc, error::OK);
        assert!(!ph.is_null());

        // ── lookup_display_name — chain into PALOC at lo32=0x200 ──────────
        for &(key, _, expected_display) in KNOWN {
            let mut req: usize = 0;
            let rc_size = unsafe {
                crimson_gimmickinfo_lookup_display_name(
                    gh,
                    ph,
                    key,
                    0x200,
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            };
            let got = read_string_result(rc_size, req, |b, n, r| unsafe {
                crimson_gimmickinfo_lookup_display_name(gh, ph, key, 0x200, b, n, r)
            });
            assert_eq!(
                got, expected_display,
                "GimmickInfoKey 0x{key:08x} display @ lo32=0x200 mismatch"
            );
        }

        // ── Negative paths ────────────────────────────────────────────────
        let mut req: usize = 0;
        let rc = unsafe {
            crimson_gimmickinfo_lookup_string_key(
                gh,
                u32::MAX,
                ptr::null_mut(),
                0,
                &mut req,
            )
        };
        assert_eq!(rc, error::NOT_FOUND);

        // Missing PALOC entry — valid gimmick key but nonsense namespace.
        let rc = unsafe {
            crimson_gimmickinfo_lookup_display_name(
                gh,
                ph,
                0x000f_4257,
                0xDEADBEEF,
                ptr::null_mut(),
                0,
                &mut req,
            )
        };
        assert_eq!(rc, error::NOT_FOUND);

        unsafe { crimson_gimmickinfo_free(gh) };
        unsafe { crimson_paloc_free(ph) };
    }

    #[test]
    fn c_abi_gimmickinfo_null_args() {
        let mut gh: *mut CrimsonGimmickInfoHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_gimmickinfo_load_from_bytes(ptr::null(), 16, &mut gh) },
            error::NULL_ARG,
        );
        assert_eq!(
            unsafe {
                crimson_gimmickinfo_load_from_bytes(
                    [0u8; 1].as_ptr(),
                    1,
                    ptr::null_mut(),
                )
            },
            error::NULL_ARG,
        );
        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_gimmickinfo_entry_count(ptr::null(), &mut count) },
            error::NULL_ARG,
        );
        let mut req: usize = 0;
        assert_eq!(
            unsafe {
                crimson_gimmickinfo_lookup_string_key(
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
                crimson_gimmickinfo_lookup_display_name(
                    ptr::null(),
                    ptr::null(),
                    0,
                    0x200,
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            },
            error::NULL_ARG,
        );
    }

    #[test]
    fn c_abi_gimmickinfo_empty_bytes_yields_empty_handle() {
        let mut gh: *mut CrimsonGimmickInfoHandle = ptr::null_mut();
        let rc = unsafe { crimson_gimmickinfo_load_from_bytes(ptr::null(), 0, &mut gh) };
        assert_eq!(rc, error::OK);
        assert!(!gh.is_null());
        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_gimmickinfo_entry_count(gh, &mut count) },
            error::OK
        );
        assert_eq!(count, 0);
        unsafe { crimson_gimmickinfo_free(gh) };
    }

    #[test]
    fn c_abi_gimmickinfo_load_bad_path_returns_io() {
        let bad = CString::new("Z:\\definitely\\does\\not\\exist.pabgb").unwrap();
        let mut gh: *mut CrimsonGimmickInfoHandle = ptr::null_mut();
        let rc = unsafe { crimson_gimmickinfo_load_from_file(bad.as_ptr(), &mut gh) };
        assert_eq!(rc, error::IO);
        assert!(gh.is_null());
    }
}
