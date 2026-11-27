//! `faction.pabgb` bridge — C ABI surface.
//!
//! Resolves save-side `FactionKey (u32)` to the row's internal name
//! (`"Faction_Graymane"`, `"Faction_ThiefDusksong"`, …), its faction group /
//! relation group keys, and the group's localized name. Faction rows carry no
//! localizable string of their own, and the PALOC byte-`0x30` lookup a
//! `FactionKey` used to go through answers with *character* names (see
//! `src/faction_info/mod.rs`); the player-facing name is the group's
//! ("Greymanes", "Hernand"). 135 rows in 2.06.
//!
//! The six name functions have the shape of the other name-only bridges
//! (`impl_name_only_bridge!`); this file spells them out because the handle
//! also keeps the group keys.
//!
//! ## FactionRelationType (save side)
//!
//! A save's `FactionRelationElementSaveData._relationTypeRaw` (u8) takes these
//! values, named by the pointer array the exe keeps right after its
//! `FactionRelationGroupInfo` consistency message:
//!
//! | Value | Name | Evidence |
//! |---|---|---|
//! | 0 | Neutral | exe table; also what an absent field defaults to |
//! | 1 | War | **verified in game, 2026-11-27**: player ⇄ `Faction_Celeste_Hernand` set 3 → 1 both ways made the Hernand guards attack |
//! | 2 | Agreement | exe table only |
//! | 3 | BloodAlly | exe table only |
//! | 4 | ByGroup | exe table only; not seen in a save |
//!
//! Relations are stored both ways, and `_dueDate` reads 0xFFFFFFFF on every
//! one seen. Whether a place turns hostile follows the faction of its ordinary
//! NPCs (`crimson_characterinfo_lookup_faction_key`): `Faction_Celeste_Hernand`
//! has 66 characters, a named house like `Faction_Roberts` 2.

use std::collections::HashMap;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::paloc::CrimsonPalocHandle;
use super::{error, write_str_to_buf};
use crate::faction_info::{FACTION_GROUP_PALOC_LO, parse_faction_info_lossy};

/// Opaque handle over a parsed `faction` table.
#[repr(C)]
pub struct CrimsonFactionInfoHandle {
    by_key: HashMap<u32, String>,
    entries: Vec<(u32, String)>,
    /// `FactionKey → (faction_group_key, faction_relation_group_key)`.
    groups: HashMap<u32, (u16, u16)>,
}

impl CrimsonFactionInfoHandle {
    fn from_bytes(pabgb: &[u8], pabgh: &[u8]) -> Self {
        let raw = parse_faction_info_lossy(pabgb, pabgh);
        let mut by_key = HashMap::with_capacity(raw.len());
        let mut entries = Vec::with_capacity(raw.len());
        let mut groups = HashMap::with_capacity(raw.len());
        for e in raw {
            if let std::collections::hash_map::Entry::Vacant(v) = by_key.entry(e.key) {
                v.insert(e.name.clone());
                groups.insert(e.key, (e.group_key, e.relation_group_key));
                entries.push((e.key, e.name));
            }
        }
        CrimsonFactionInfoHandle { by_key, entries, groups }
    }
}

fn slice<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if len == 0 { &[] } else { unsafe { std::slice::from_raw_parts(ptr, len) } }
}

/// Load from a `.pabgb` / `.staticinfobody` path and its header path.
///
/// # Safety
/// Both path arguments must be NUL-terminated UTF-8 strings.
/// `out_handle` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_faction_info_load_from_file(
    pabgb_path: *const c_char,
    pabgh_path: *const c_char,
    out_handle: *mut *mut CrimsonFactionInfoHandle,
) -> i32 {
    if pabgb_path.is_null() || pabgh_path.is_null() || out_handle.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_handle = std::ptr::null_mut() };
    catch_unwind(AssertUnwindSafe(|| {
        let (Ok(b), Ok(h)) = (
            unsafe { std::ffi::CStr::from_ptr(pabgb_path) }.to_str(),
            unsafe { std::ffi::CStr::from_ptr(pabgh_path) }.to_str(),
        ) else {
            return error::INVALID_PATH;
        };
        let (Ok(pabgb), Ok(pabgh)) = (std::fs::read(b), std::fs::read(h)) else {
            return error::IO;
        };
        let handle = CrimsonFactionInfoHandle::from_bytes(&pabgb, &pabgh);
        unsafe { *out_handle = Box::into_raw(Box::new(handle)) };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Load from in-memory body + header bytes.
///
/// # Safety
/// `pabgb`/`pabgh` may be null iff their length is 0; `out_handle` must be
/// non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_faction_info_load_from_bytes(
    pabgb: *const u8,
    pabgb_len: usize,
    pabgh: *const u8,
    pabgh_len: usize,
    out_handle: *mut *mut CrimsonFactionInfoHandle,
) -> i32 {
    if out_handle.is_null() {
        return error::NULL_ARG;
    }
    if (pabgb.is_null() && pabgb_len != 0) || (pabgh.is_null() && pabgh_len != 0) {
        return error::NULL_ARG;
    }
    unsafe { *out_handle = std::ptr::null_mut() };
    catch_unwind(AssertUnwindSafe(|| {
        let handle =
            CrimsonFactionInfoHandle::from_bytes(slice(pabgb, pabgb_len), slice(pabgh, pabgh_len));
        unsafe { *out_handle = Box::into_raw(Box::new(handle)) };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// # Safety
/// `handle` must be null or a pointer previously returned by one of the
/// loaders and not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_faction_info_free(handle: *mut CrimsonFactionInfoHandle) {
    if handle.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(handle);
    }
}

/// # Safety
/// `handle` must be live; `out_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_faction_info_entry_count(
    handle: *const CrimsonFactionInfoHandle,
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

/// Internal name of `faction_key` (two-call pattern over `buf`).
///
/// # Safety
/// `handle` and `required` must be non-null; `buf` may be null iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_faction_info_lookup_string_key(
    handle: *const CrimsonFactionInfoHandle,
    faction_key: u32,
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
        let Some(name) = h.by_key.get(&faction_key) else {
            return error::NOT_FOUND;
        };
        write_str_to_buf(name, buf, buf_len, required)
    }))
    .unwrap_or(error::PANIC)
}

/// `(faction_key, internal_name)` at index `idx` (two-call pattern over `buf`;
/// `out_key` is always written). `OUT_OF_RANGE` past the end.
///
/// # Safety
/// `handle`, `out_key` and `required` must be non-null; `buf` may be null iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_faction_info_get_entry(
    handle: *const CrimsonFactionInfoHandle,
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
    unsafe { *required = 0 };
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

/// The faction's `factiongroup` key and `factionrelationgroup` key.
/// `*out_group_key` is 0 for a faction without a group (7 rows on 2.06).
/// `NOT_FOUND` (both outputs 0) when `faction_key` is not in the table.
///
/// # Safety
/// `handle`, `out_group_key` and `out_relation_group_key` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_faction_info_lookup_group_keys(
    handle: *const CrimsonFactionInfoHandle,
    faction_key: u32,
    out_group_key: *mut u16,
    out_relation_group_key: *mut u16,
) -> i32 {
    if handle.is_null() || out_group_key.is_null() || out_relation_group_key.is_null() {
        return error::NULL_ARG;
    }
    unsafe {
        *out_group_key = 0;
        *out_relation_group_key = 0;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some(&(group, relation)) = h.groups.get(&faction_key) else {
            return error::NOT_FOUND;
        };
        unsafe {
            *out_group_key = group;
            *out_relation_group_key = relation;
        }
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Localized name of a faction group: PALOC
/// `(faction_group_key << 32) | 0x214` (`faction.paloc`; 16965 → "Greymanes"
/// in English). Two-call pattern over `buf`. `NOT_FOUND` when the PALOC has no
/// such entry (including group key 0).
///
/// # Safety
/// `paloc_handle` and `required` must be non-null; `buf` may be null iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_faction_info_lookup_group_display_name(
    paloc_handle: *const CrimsonPalocHandle,
    faction_group_key: u16,
    buf: *mut u8,
    buf_len: usize,
    required: *mut usize,
) -> i32 {
    if paloc_handle.is_null() || required.is_null() {
        return error::NULL_ARG;
    }
    if buf.is_null() && buf_len != 0 {
        return error::NULL_ARG;
    }
    unsafe { *required = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let paloc = unsafe { &*paloc_handle };
        group_display_name(paloc, faction_group_key)
            .map_or(error::NOT_FOUND, |name| write_str_to_buf(name, buf, buf_len, required))
    }))
    .unwrap_or(error::PANIC)
}

/// The player-facing name of a faction: its group's localized name
/// (`Faction_Graymane` → "Greymanes", `Faction_Celeste_Hernand` → "Hernand").
/// Several factions share a group, so several share a name; fall back to
/// [`crimson_faction_info_lookup_string_key`] to tell them apart. `NOT_FOUND`
/// when the faction is unknown, has no group, or the PALOC has no name.
///
/// # Safety
/// `handle`, `paloc_handle` and `required` must be non-null; `buf` may be null
/// iff `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_faction_info_lookup_display_name(
    handle: *const CrimsonFactionInfoHandle,
    paloc_handle: *const CrimsonPalocHandle,
    faction_key: u32,
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
        let Some(&(group, _)) = h.groups.get(&faction_key) else {
            return error::NOT_FOUND;
        };
        group_display_name(paloc, group)
            .map_or(error::NOT_FOUND, |name| write_str_to_buf(name, buf, buf_len, required))
    }))
    .unwrap_or(error::PANIC)
}

fn group_display_name(paloc: &CrimsonPalocHandle, group: u16) -> Option<&str> {
    if group == 0 {
        return None;
    }
    let key = (u64::from(group) << 32) | u64::from(FACTION_GROUP_PALOC_LO);
    paloc.lookup_str(&key.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binary::gamedata_layout;
    use crate::c_abi::error;
    use crate::c_abi::paz::crimson_paz_extract_file;
    use std::ffi::{CStr, CString};
    use std::path::PathBuf;
    use std::ptr;

    const KNOWN: &[(u32, &str)] = &[
        (1000000, "Faction_Graymane"),
        (1000039, "Faction_Byron"),
        (1000065, "Faction_ThiefDusksong"),
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

    fn lookup(h: *const CrimsonFactionInfoHandle, key: u32) -> Option<String> {
        let mut req: usize = 0;
        let rc = unsafe {
            crimson_faction_info_lookup_string_key(h, key, ptr::null_mut(), 0, &mut req)
        };
        if rc == error::NOT_FOUND {
            return None;
        }
        assert_eq!(rc, error::BUFFER_TOO_SMALL);
        let mut buf = vec![0u8; req];
        let rc = unsafe {
            crimson_faction_info_lookup_string_key(h, key, buf.as_mut_ptr(), buf.len(), &mut req)
        };
        assert_eq!(rc, error::OK);
        Some(std::str::from_utf8(&buf[..req - 1]).unwrap().to_owned())
    }

    #[test]
    fn c_abi_faction_info_live() {
        let Some(pamt_path) = find_pamt() else {
            eprintln!("skipping c_abi_faction_info_live: no game install");
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let body = extract_file(
            pamt.as_c_str(),
            gamedata_layout::bin_dir(),
            &gamedata_layout::body("faction"),
        );
        let header = extract_file(
            pamt.as_c_str(),
            gamedata_layout::bin_dir(),
            &gamedata_layout::header("faction"),
        );
        let mut h: *mut CrimsonFactionInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_faction_info_load_from_bytes(
                body.as_ptr(),
                body.len(),
                header.as_ptr(),
                header.len(),
                &mut h,
            )
        };
        assert_eq!(rc, error::OK);
        let mut count: u32 = 0;
        assert_eq!(unsafe { crimson_faction_info_entry_count(h, &mut count) }, error::OK);
        assert!(count >= 130, "expected >=130 faction rows, got {count}");
        for &(key, expected) in KNOWN {
            assert_eq!(lookup(h, key).as_deref(), Some(expected), "FactionKey {key}");
        }
        assert_eq!(lookup(h, u32::MAX), None);

        // Group keys (2.06).
        let groups = |key: u32| {
            let (mut g, mut r) = (u16::MAX, u16::MAX);
            let rc = unsafe { crimson_faction_info_lookup_group_keys(h, key, &mut g, &mut r) };
            (rc, g, r)
        };
        assert_eq!(groups(1_000_000), (error::OK, 16_965, 16_963)); // Faction_Graymane
        assert_eq!(groups(1_000_131), (error::OK, 16_960, 16_964)); // Faction_Celeste_Hernand
        assert_eq!(groups(1_000_095), (error::OK, 0, 16_965)); // Faction_BountyHunter: no group
        assert_eq!(groups(u32::MAX), (error::NOT_FOUND, 0, 0));

        // Localized group names through the English PALOC.
        let paloc_bytes = gamedata_layout::paloc_bytes("0020", "eng").expect("eng paloc");
        let mut ph: *mut crate::c_abi::paloc::CrimsonPalocHandle = ptr::null_mut();
        assert_eq!(
            unsafe {
                crate::c_abi::paloc::crimson_paloc_load_from_bytes(
                    paloc_bytes.as_ptr(),
                    paloc_bytes.len(),
                    &mut ph,
                )
            },
            error::OK
        );
        let read = |f: &dyn Fn(*mut u8, usize, *mut usize) -> i32| -> Option<String> {
            let mut req = 0usize;
            match f(ptr::null_mut(), 0, &mut req) {
                rc if rc == error::NOT_FOUND => return None,
                rc => assert_eq!(rc, error::BUFFER_TOO_SMALL),
            }
            let mut buf = vec![0u8; req];
            assert_eq!(f(buf.as_mut_ptr(), buf.len(), &mut req), error::OK);
            Some(std::str::from_utf8(&buf[..req - 1]).unwrap().to_owned())
        };
        let group_name = |g: u16| {
            read(&|b, n, r| unsafe { crimson_faction_info_lookup_group_display_name(ph, g, b, n, r) })
        };
        let faction_name = |k: u32| {
            read(&|b, n, r| unsafe { crimson_faction_info_lookup_display_name(h, ph, k, b, n, r) })
        };
        assert_eq!(group_name(16_965).as_deref(), Some("Greymanes"));
        assert_eq!(group_name(16_960).as_deref(), Some("Hernand"));
        assert_eq!(group_name(0), None);
        assert_eq!(faction_name(1_000_000).as_deref(), Some("Greymanes"));
        assert_eq!(faction_name(1_000_131).as_deref(), Some("Hernand"));
        assert_eq!(faction_name(1_000_063).as_deref(), Some("Abyss"));
        assert_eq!(faction_name(1_000_095), None); // no group
        unsafe { crate::c_abi::paloc::crimson_paloc_free(ph) };
        unsafe { crimson_faction_info_free(h) };
    }
}
