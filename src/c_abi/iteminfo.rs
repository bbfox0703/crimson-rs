//! `iteminfo.pabgb` bridge — C ABI surface.
//!
//! Parses an in-memory `iteminfo.pabgb` blob (after PAZ extraction
//! from group `0008`, directory `gamedata/binary__/client/bin/`) and
//! exposes a single primitive the downstream editor needs: map an
//! `ItemKey (u32)` to its `string_key (String)`. The string key then
//! feeds into the PALOC catalog from [`super::paloc`] to yield a
//! localized display name.
//!
//! Memory cost: ~6,400 items in 1.06 × ~30 bytes per string key ≈
//! 200 KB resident plus HashMap overhead. The full `ItemInfo` parse
//! runs once at load time, but only the `(key, string_key)` pair is
//! retained — the other 100+ fields are dropped. A future PR can
//! expose richer queries (icon path, item description, …) without
//! reshaping this handle.

use std::collections::HashMap;
use std::io;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::error;
use crate::binary::BinaryRead;
use crate::item_info::ItemInfo;

/// Opaque handle exposing lean per-item lookups against the loaded
/// iteminfo: the `string_key` (internal id) and the `max_stack_count`
/// (stack cap). The full ItemInfo parse runs once; only the bits the
/// downstream editor needs are retained.
#[repr(C)]
pub struct CrimsonItemInfoHandle {
    by_key: HashMap<u32, String>,
    max_stack_by_key: HashMap<u32, u64>,
    /// First entry of `item_icon_list[0].icon_path` (a `StringInfoKey`
    /// hash) per item key. Items with no icon list aren't inserted —
    /// the lookup returns `NOT_FOUND` instead of writing 0.
    icon_path_by_key: HashMap<u32, u32>,
    /// Per-item `look_detail_mission_info` (a `MissionKey` u32). Only
    /// items with a non-zero value are inserted — that maps cleanly to
    /// the editor's "does this artifact item belong to a catalog
    /// challenge?" predicate. Quest-reward items (most notably the
    /// Sealed Abyss Artifact series) point at the catalog mission key
    /// of the challenge that rewards them; regular items leave this
    /// field at 0 and don't show up in the map.
    look_detail_mission_by_key: HashMap<u32, u32>,
    /// `(key, string_key)` in file order so the caller can enumerate
    /// via [`crimson_iteminfo_get_entry`].
    entries: Vec<(u32, String)>,
}

impl CrimsonItemInfoHandle {
    fn from_bytes(data: &[u8]) -> io::Result<Self> {
        // Walk the input the same way `test_full_roundtrip` in lib.rs
        // does. Each `ItemInfo::read_from` advances `offset`; we keep
        // going until the buffer is consumed.
        let mut offset = 0usize;
        let mut entries: Vec<(u32, String)> = Vec::new();
        let mut max_stack_by_key: HashMap<u32, u64> = HashMap::new();
        let mut icon_path_by_key: HashMap<u32, u32> = HashMap::new();
        let mut look_detail_mission_by_key: HashMap<u32, u32> = HashMap::new();
        while offset < data.len() {
            let item = ItemInfo::read_from(data, &mut offset)?;
            entries.push((item.key.0, item.string_key.data.to_owned()));
            max_stack_by_key.insert(item.key.0, item.max_stack_count);
            // Only capture the first per-item icon. Items without an
            // `item_icon_list` entry are intentionally skipped — the
            // lookup surface reports those as NOT_FOUND, mirroring the
            // string-key lookup's contract for dev-only items.
            if let Some(first) = item.item_icon_list.items.first() {
                let hash = first.icon_path.0;
                if hash != 0 {
                    icon_path_by_key.insert(item.key.0, hash);
                }
            }
            // 0 means "no associated mission" and is the overwhelming
            // majority case (vanilla items / weapons / consumables).
            // Skip those so the lookup's NOT_FOUND signal is meaningful.
            if item.look_detail_mission_info.0 != 0 {
                look_detail_mission_by_key
                    .insert(item.key.0, item.look_detail_mission_info.0);
            }
        }
        if offset != data.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "iteminfo parse ended at 0x{:X} but file is 0x{:X} bytes",
                    offset,
                    data.len()
                ),
            ));
        }
        let by_key = entries.iter().cloned().collect();
        Ok(CrimsonItemInfoHandle {
            by_key,
            max_stack_by_key,
            icon_path_by_key,
            look_detail_mission_by_key,
            entries,
        })
    }
}

// ── Load / free ────────────────────────────────────────────────────────────

/// Parse an `iteminfo.pabgb` blob from disk.
///
/// The file must be **already-decrypted, raw `.pabgb` bytes** — the
/// wrapped copy under `0008/0.paz` needs to come through PAZ
/// extraction first (see [`super::paz::crimson_paz_extract_file`]).
///
/// On success `*out_handle` receives an owned
/// [`CrimsonItemInfoHandle`] that the caller must release via
/// [`crimson_iteminfo_free`].
///
/// # Safety
/// `path` must be a NUL-terminated UTF-8 string and `out_handle` must
/// point at writable memory for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_iteminfo_load_from_file(
    path: *const c_char,
    out_handle: *mut *mut CrimsonItemInfoHandle,
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
        let bytes = match std::fs::read(path_str) {
            Ok(b) => b,
            Err(_) => return error::IO,
        };
        let handle = match CrimsonItemInfoHandle::from_bytes(&bytes) {
            Ok(h) => h,
            Err(_) => return error::BODY_PARSE,
        };
        unsafe { *out_handle = Box::into_raw(Box::new(handle)) };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Parse iteminfo bytes already in memory (preferred — the editor pulls
/// them through PAZ extraction first).
///
/// # Safety
/// `data` must point to `data_len` readable bytes; `out_handle` must
/// be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_iteminfo_load_from_bytes(
    data: *const u8,
    data_len: usize,
    out_handle: *mut *mut CrimsonItemInfoHandle,
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
        let handle = match CrimsonItemInfoHandle::from_bytes(slice) {
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
pub unsafe extern "C" fn crimson_iteminfo_free(handle: *mut CrimsonItemInfoHandle) {
    if handle.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(handle);
    }
}

// ── Scalar getters ─────────────────────────────────────────────────────────

/// Total number of items in the loaded `iteminfo.pabgb`.
///
/// # Safety
/// `handle` must be live; `out_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_iteminfo_entry_count(
    handle: *const CrimsonItemInfoHandle,
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

/// Look up the `string_key` for a given `ItemKey (u32)` and write it
/// into `buf` (NUL-terminated UTF-8). Two-call pattern, identical
/// shape to the PALOC catalog:
///
/// - First call with `buf = null, buf_len = 0` returns
///   `BUFFER_TOO_SMALL` and sets `*required` (includes trailing NUL).
/// - Allocate, call again to receive the bytes and `OK`.
///
/// Returns `NOT_FOUND` when `item_key` doesn't match any item in the
/// loaded table.
///
/// # Safety
/// `handle` and `required` must be non-null; `buf` may be null iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_iteminfo_lookup_string_key(
    handle: *const CrimsonItemInfoHandle,
    item_key: u32,
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
        let Some(name) = h.by_key.get(&item_key) else {
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

/// Look up the first `item_icon_list[0].icon_path` for a given `ItemKey
/// (u32)` and write the resulting `StringInfoKey` (u32 hash) into
/// `*out_hash`. The downstream icon-extraction pipeline pipes the hash
/// through the stringinfo bridge to obtain a texture name like
/// `ItemIcon_Prefab_cd_phm_04_arw_0020`, lowercases it, appends
/// `.dds`, and PAZ-extracts the texture from group `0012`'s
/// `ui/texture/icon/` directory.
///
/// Returns `NOT_FOUND` when the key isn't in the loaded table OR the
/// item ships without an `item_icon_list` entry OR the first entry's
/// `icon_path` is 0 (interpreted as "no icon"). On `NOT_FOUND`,
/// `*out_hash` is set to 0.
///
/// # Safety
/// `handle` and `out_hash` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_iteminfo_lookup_icon_path_hash(
    handle: *const CrimsonItemInfoHandle,
    item_key: u32,
    out_hash: *mut u32,
) -> i32 {
    if handle.is_null() || out_hash.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_hash = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some(hash) = h.icon_path_by_key.get(&item_key) else {
            return error::NOT_FOUND;
        };
        unsafe { *out_hash = *hash };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Look up the `max_stack_count` for a given `ItemKey (u32)` and write
/// it into `*out_max_stack`. Returns `NOT_FOUND` when the key isn't
/// in the loaded table, `OK` otherwise. The downstream editor uses
/// this to drive a "Set to max stack" action that fills a save's
/// item-count field with the game's own per-item cap (so the user
/// gets a maxed stack without exceeding what the game considers
/// valid).
///
/// # Safety
/// `handle` and `out_max_stack` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_iteminfo_lookup_max_stack(
    handle: *const CrimsonItemInfoHandle,
    item_key: u32,
    out_max_stack: *mut u64,
) -> i32 {
    if handle.is_null() || out_max_stack.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_max_stack = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some(max) = h.max_stack_by_key.get(&item_key) else {
            return error::NOT_FOUND;
        };
        unsafe { *out_max_stack = *max };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Look up the `look_detail_mission_info` (a `MissionKey` u32) for a
/// given `ItemKey (u32)` and write it into `*out_mission_key`. Returns
/// `NOT_FOUND` when the key isn't in the loaded table OR the item ships
/// without a mission link (the field is 0). On `NOT_FOUND`,
/// `*out_mission_key` is set to 0.
///
/// Use case: the editor's "Mark Challenge Complete" UI only enables when
/// the in-focus catalog `MissionStateData` challenge has a corresponding
/// quest-reward item (e.g. a Sealed Abyss Artifact) currently in the
/// player's inventory. Walking inventory + matching this lookup is the
/// fast path for that gate.
///
/// # Safety
/// `handle` and `out_mission_key` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_iteminfo_lookup_look_detail_mission_info(
    handle: *const CrimsonItemInfoHandle,
    item_key: u32,
    out_mission_key: *mut u32,
) -> i32 {
    if handle.is_null() || out_mission_key.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_mission_key = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some(mk) = h.look_detail_mission_by_key.get(&item_key) else {
            return error::NOT_FOUND;
        };
        unsafe { *out_mission_key = *mk };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

// ── Enumeration ────────────────────────────────────────────────────────────

/// Get the `(item_key, string_key)` pair at insertion index `idx`.
/// Two-call pattern over `buf`; the `out_key` u32 is always written.
///
/// Returns `OUT_OF_RANGE` when `idx >= entry_count`.
///
/// # Safety
/// `handle`, `out_key`, and `required` must be non-null; `buf` may
/// be null iff `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_iteminfo_get_entry(
    handle: *const CrimsonItemInfoHandle,
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
    //! Live-install integration tests against the real iteminfo.pabgb.
    //! Skip cleanly when no Steam install is present. Synthesizing
    //! `iteminfo.pabgb` from scratch is impractical (each item is
    //! ~600 B with 100+ fields) so we rely on round-tripping the
    //! real bytes through PAZ extraction + the new C ABI.
    //!
    //! Coverage for the C ABI's error paths (NULL_ARG, BODY_PARSE on
    //! garbage, OUT_OF_RANGE, NOT_FOUND) uses synthetic inputs that
    //! exercise the wrappers without needing valid item bytes.

    use super::*;
    use crate::c_abi::paz::crimson_paz_extract_file;
    use std::ffi::CString;
    use std::path::PathBuf;
    use std::ptr;

    fn find_pamt_for_iteminfo() -> Option<PathBuf> {
        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let p = game_root.join("0008").join("0.pamt");
        p.is_file().then_some(p)
    }

    /// Pull iteminfo.pabgb via the standard PAZ path, returns its bytes.
    fn extract_iteminfo_bytes(pamt: &CStr) -> Vec<u8> {
        let dir = CString::new("gamedata/binary__/client/bin").unwrap();
        let name = CString::new("iteminfo.pabgb").unwrap();
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

    use std::ffi::CStr;

    #[test]
    fn c_abi_iteminfo_live_roundtrip() {
        let Some(pamt_path) = find_pamt_for_iteminfo() else {
            eprintln!(
                "skipping c_abi_iteminfo_live_roundtrip: no 0008/0.pamt in game install"
            );
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let bytes = extract_iteminfo_bytes(pamt.as_c_str());

        let mut handle: *mut CrimsonItemInfoHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_iteminfo_load_from_bytes(bytes.as_ptr(), bytes.len(), &mut handle) },
            error::OK
        );
        assert!(!handle.is_null());

        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_iteminfo_entry_count(handle, &mut count) },
            error::OK
        );
        // 1.05 had ~6,236; 1.06 has ~6,400. Just assert plausibly populated.
        assert!(count > 5_000, "expected >5k items, got {count}");

        // ── Round-trip: pick entry 0, then look up its string_key by
        // the u32 we got back. Must match.
        let mut out_key: u32 = 0;
        let mut req: usize = 0;
        let rc = unsafe {
            crimson_iteminfo_get_entry(
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
            crimson_iteminfo_get_entry(
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
        assert!(!enum_name.is_empty(), "item 0's string_key should be non-empty");

        // Now lookup by the u32 we just read.
        let mut req2: usize = 0;
        let rc = unsafe {
            crimson_iteminfo_lookup_string_key(
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
            crimson_iteminfo_lookup_string_key(
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

        // NOT_FOUND on a definitely-invalid item key (0 might be valid
        // depending on the data, so use a value picked to be outside
        // the range — u32::MAX is safely out of bounds).
        let mut req3: usize = 0;
        let rc = unsafe {
            crimson_iteminfo_lookup_string_key(
                handle,
                u32::MAX,
                ptr::null_mut(),
                0,
                &mut req3,
            )
        };
        assert_eq!(rc, error::NOT_FOUND);

        // ── max_stack lookup: round-trip against entry 0's key, then
        // assert u32::MAX is NOT_FOUND. Don't pin the value of any
        // specific item — the schema is the contract, not "Camp Funds
        // caps at 999999".
        let mut max_stack: u64 = 999;
        let rc = unsafe { crimson_iteminfo_lookup_max_stack(handle, out_key, &mut max_stack) };
        assert_eq!(rc, error::OK);

        let mut bogus: u64 = 0;
        let rc = unsafe { crimson_iteminfo_lookup_max_stack(handle, u32::MAX, &mut bogus) };
        assert_eq!(rc, error::NOT_FOUND);
        assert_eq!(bogus, 0, "out_max_stack should be reset on NOT_FOUND");

        // ── icon_path_hash lookup: at least one item in any real game
        // install ships an icon. Walk the table and assert there's at
        // least one hit, then assert u32::MAX is NOT_FOUND.
        let mut found_at_least_one_icon = false;
        for i in 0..count.min(2000) {
            let mut ik: u32 = 0;
            let mut req: usize = 0;
            let _ = unsafe {
                crimson_iteminfo_get_entry(
                    handle, i, &mut ik, ptr::null_mut(), 0, &mut req,
                )
            };
            let mut icon_hash: u32 = 0;
            let rc = unsafe {
                crimson_iteminfo_lookup_icon_path_hash(handle, ik, &mut icon_hash)
            };
            if rc == error::OK && icon_hash != 0 {
                found_at_least_one_icon = true;
                break;
            }
        }
        assert!(
            found_at_least_one_icon,
            "expected at least one item in the first 2000 to have an icon"
        );

        let mut bogus_icon: u32 = 0;
        let rc = unsafe {
            crimson_iteminfo_lookup_icon_path_hash(handle, u32::MAX, &mut bogus_icon)
        };
        assert_eq!(rc, error::NOT_FOUND);
        assert_eq!(bogus_icon, 0, "out_hash should be reset on NOT_FOUND");

        // OUT_OF_RANGE on get_entry past the end.
        let rc = unsafe {
            crimson_iteminfo_get_entry(
                handle,
                u32::MAX,
                &mut out_key,
                ptr::null_mut(),
                0,
                &mut req,
            )
        };
        assert_eq!(rc, error::OUT_OF_RANGE);

        unsafe { crimson_iteminfo_free(handle) };
        // free(null) is a no-op.
        unsafe { crimson_iteminfo_free(ptr::null_mut()) };
    }

    #[test]
    fn c_abi_iteminfo_garbage_bytes_returns_body_parse() {
        // 32 zero bytes won't parse as an ItemInfo (the leading u32
        // ItemKey reads as 0; the next CString reads a length of 0
        // then 0 bytes; then the next field tries to read more bytes
        // than remain → InvalidData → BODY_PARSE).
        let garbage = [0u8; 32];
        let mut handle: *mut CrimsonItemInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_iteminfo_load_from_bytes(garbage.as_ptr(), garbage.len(), &mut handle)
        };
        assert_eq!(rc, error::BODY_PARSE);
        assert!(handle.is_null());
    }

    #[test]
    fn c_abi_iteminfo_null_args() {
        let mut handle: *mut CrimsonItemInfoHandle = ptr::null_mut();
        assert_eq!(
            unsafe {
                crimson_iteminfo_load_from_bytes(ptr::null(), 16, &mut handle)
            },
            error::NULL_ARG
        );
        assert_eq!(
            unsafe {
                crimson_iteminfo_load_from_bytes([0u8; 1].as_ptr(), 1, ptr::null_mut())
            },
            error::NULL_ARG
        );

        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_iteminfo_entry_count(ptr::null(), &mut count) },
            error::NULL_ARG
        );

        let mut req: usize = 0;
        assert_eq!(
            unsafe {
                crimson_iteminfo_lookup_string_key(
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
    fn c_abi_iteminfo_load_bad_path_returns_io() {
        let bad = CString::new("Z:\\definitely\\does\\not\\exist.pabgb").unwrap();
        let mut handle: *mut CrimsonItemInfoHandle = ptr::null_mut();
        let rc = unsafe { crimson_iteminfo_load_from_file(bad.as_ptr(), &mut handle) };
        assert_eq!(rc, error::IO);
        assert!(handle.is_null());
    }
}
