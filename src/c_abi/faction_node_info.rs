//! `factionnode.pabgb` bridge — C ABI surface.
//!
//! Resolves save-side `FactionNodeKey (u32)` to the row's internal
//! name (e.g. `"Node_Her_Temporary_Camp"`) and, since 2026-11-27, to the
//! place name the game shows. That name is not keyed by the node (the
//! exhaustive PALOC probe `_probe_faction_paloc_chains` found nothing at any
//! namespace): every row names a `knowledgeinfo` row, and that knowledge's
//! title is the place name ("Hernand Castle", "Howling Hill"). See
//! `src/faction_node_info/mod.rs`; no owning-faction or region field was found.
//!
//! Standard `[u32 name_len][name][u32 key][body]` row layout indexed by
//! a `u16 count + (u32 key, u32 offset)*` PABGH — re-uses
//! `skill_info::parse_pabgh` under the hood. See
//! `src/faction_node_info/mod.rs` for the byte-level schema notes.

use std::collections::HashMap;
use std::ffi::CStr;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::error;
use super::EntrySources;
use super::knowledge_info::CrimsonKnowledgeInfoHandle;
use super::paloc::CrimsonPalocHandle;
use crate::faction_node_info::parse_faction_node_info_lossy;

/// Opaque handle exposing `(FactionNodeKey, internal_name)` lookups
/// against the loaded `factionnode.pabgb` + `.pabgh` pair.
#[repr(C)]
pub struct CrimsonFactionNodeInfoHandle {
    by_key: HashMap<u32, String>,
    entries: Vec<(u32, String)>,
    /// `FactionNodeKey → KnowledgeKey` (the knowledge naming the place).
    knowledge_by_key: HashMap<u32, u32>,
    /// Base table or which merged DLC, per entry.
    sources: EntrySources,
}

impl CrimsonFactionNodeInfoHandle {
    fn from_bytes(pabgb: &[u8], pabgh: &[u8]) -> Self {
        let mut h = CrimsonFactionNodeInfoHandle {
            by_key: HashMap::new(),
            entries: Vec::new(),
            knowledge_by_key: HashMap::new(),
            sources: EntrySources::default(),
        };
        h.absorb(pabgb, pabgh, 0);
        h
    }

    /// Add the rows whose key is new (first wins); returns how many.
    fn absorb(&mut self, pabgb: &[u8], pabgh: &[u8], source: u8) -> u32 {
        let mut added = 0;
        for e in parse_faction_node_info_lossy(pabgb, pabgh) {
            if let std::collections::hash_map::Entry::Vacant(v) = self.by_key.entry(e.key) {
                v.insert(e.name.clone());
                self.knowledge_by_key.insert(e.key, e.knowledge_key);
                self.entries.push((e.key, e.name));
                self.sources.push(source);
                added += 1;
            }
        }
        added
    }
}

// ── Load / free ────────────────────────────────────────────────────────────

/// Parse `factionnode.pabgb` + `.pabgh` from disk.
///
/// # Safety
/// Both path arguments must be NUL-terminated UTF-8 strings.
/// `out_handle` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_factionnode_load_from_file(
    pabgb_path: *const c_char,
    pabgh_path: *const c_char,
    out_handle: *mut *mut CrimsonFactionNodeInfoHandle,
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
        let handle = CrimsonFactionNodeInfoHandle::from_bytes(&pabgb, &pabgh);
        if let Err(code) = crate::c_abi::check_rows(&pabgb, Some(&pabgh[..]), handle.entries.len()) {
            return code;
        }
        unsafe { *out_handle = Box::into_raw(Box::new(handle)) };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Parse factionnode pabgb + pabgh bytes already in memory.
///
/// # Safety
/// Both `pabgb` and `pabgh` must point to `*_len` readable bytes (may
/// be null iff length is 0). `out_handle` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_factionnode_load_from_bytes(
    pabgb: *const u8,
    pabgb_len: usize,
    pabgh: *const u8,
    pabgh_len: usize,
    out_handle: *mut *mut CrimsonFactionNodeInfoHandle,
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
        let handle =
            CrimsonFactionNodeInfoHandle::from_bytes(pabgb_slice, pabgh_slice);
        if let Err(code) = crate::c_abi::check_rows(pabgb_slice, Some(pabgh_slice), handle.entries.len()) {
            return code;
        }
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
pub unsafe extern "C" fn crimson_factionnode_free(
    handle: *mut CrimsonFactionNodeInfoHandle,
) {
    if handle.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(handle);
    }
}

// ── Scalar getters ─────────────────────────────────────────────────────────

/// Total number of `factionnode` rows in the loaded table.
///
/// # Safety
/// `handle` must be live; `out_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_factionnode_entry_count(
    handle: *const CrimsonFactionNodeInfoHandle,
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

/// Look up the internal name for a given `FactionNodeKey (u32)` and
/// write it into `buf` (NUL-terminated UTF-8). Two-call pattern.
///
/// Returns `NOT_FOUND` when `faction_node_key` isn't in the table.
///
/// **No `lookup_display_name` analogue exists for this bridge** —
/// the exhaustive PALOC probe surfaced only coincidental collisions
/// (every "hit" pointed at iteminfo / character / skill strings that
/// happened to share the same small u32 hi32). Caller should display
/// the internal name directly, same pattern as the QuestGauge /
/// SubLevel bridges.
///
/// # Safety
/// `handle` and `required` must be non-null; `buf` may be null iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_factionnode_lookup_string_key(
    handle: *const CrimsonFactionNodeInfoHandle,
    faction_node_key: u32,
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
        let Some(name) = h.by_key.get(&faction_node_key) else {
            return error::NOT_FOUND;
        };
        write_str_to_buf(name, buf, buf_len, required)
    }))
    .unwrap_or(error::PANIC)
}

/// Get the `(faction_node_key, internal_name)` pair at insertion index
/// `idx`. Two-call pattern.
///
/// # Safety
/// `handle`, `out_key`, and `required` must be non-null; `buf` may be
/// null iff `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_factionnode_get_entry(
    handle: *const CrimsonFactionNodeInfoHandle,
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

/// Merge a DLC copy of the table (`<install>/dlc/<name>/0008`, e.g.
/// `factionnode.dlc_01.staticinfobody` + `.staticinfoheader`) into a loaded
/// handle. Adds only the rows whose key the handle lacks, after the existing
/// entries; a shared key keeps the base row (on 2.06 base and `dlc_01` agree
/// on every field this bridge exposes). `*out_added` = rows added.
///
/// **Optional.** Without the DLC installed (Steam installs `dlc/dlc_01` only
/// for owners) skip this call: every function works on the base rows alone.
/// Call it once per DLC, in folder order; [`crimson_factionnode_entry_source`] then
/// reports 1 for the first DLC's rows, 2 for the second's. Not thread-safe
/// against concurrent lookups on the same handle.
///
/// # Safety
/// `handle` and `out_added` must be non-null; `body` / `header` must point
/// to `*_len` readable bytes (null iff the length is 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_factionnode_merge_dlc_from_bytes(
    handle: *mut CrimsonFactionNodeInfoHandle,
    body: *const u8,
    body_len: usize,
    header: *const u8,
    header_len: usize,
    out_added: *mut u32,
) -> i32 {
    unsafe {
        super::merge_dlc_ffi(handle, body, body_len, header, header_len, out_added, |h, b, hd| {
            let source = h.sources.next_merge();
            h.absorb(b, hd, source)
        })
    }
}

/// Where entry `idx` came from: 0 = the base table, `n` = the `n`-th DLC
/// merged with [`crimson_factionnode_merge_dlc_from_bytes`]. Lets a list hide DLC rows.
///
/// # Safety
/// `handle` and `out_source` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_factionnode_entry_source(
    handle: *const CrimsonFactionNodeInfoHandle,
    idx: u32,
    out_source: *mut u32,
) -> i32 {
    unsafe { super::entry_source_ffi(handle, idx, out_source, |h| &h.sources) }
}

/// The `KnowledgeKey` naming a node: the knowledge row whose title is the
/// place name the game shows (`Node_Her_HernandCastle` 1000044 → 1000016
/// `Knowledge_Node_Her_HernandCastle`).
///
/// Returns `NOT_FOUND` for an unknown node and for a row too short to hold
/// the key.
///
/// # Safety
/// `handle` and `out_knowledge_key` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_factionnode_lookup_knowledge_key(
    handle: *const CrimsonFactionNodeInfoHandle,
    node_key: u32,
    out_knowledge_key: *mut u32,
) -> i32 {
    if handle.is_null() || out_knowledge_key.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_knowledge_key = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        match h.knowledge_by_key.get(&node_key) {
            Some(&k) if k != 0 => {
                unsafe { *out_knowledge_key = k };
                error::OK
            }
            _ => error::NOT_FOUND,
        }
    }))
    .unwrap_or(error::PANIC)
}

/// The node's localized place name: `FactionNodeKey → KnowledgeKey →`
/// knowledge name `→ PALOC (hashlittle2(name) << 32) | 0x490`, the knowledge
/// title (`crimson_knowledgeinfo_lookup_display_name` at `0x490`). The
/// language is the PALOC handle's: every node has a title in all 15 shipped
/// languages. Two-call pattern.
///
/// `knowledge` must be loaded from the same install (on 2.06 with its header,
/// `crimson_knowledgeinfo_load_from_bytes_with_header`). Returns `NOT_FOUND`
/// when the node, its knowledge row or the PALOC entry is missing.
///
/// # Safety
/// `handle`, `knowledge`, `paloc` and `required` must be non-null; `buf` may
/// be null iff `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_factionnode_lookup_display_name(
    handle: *const CrimsonFactionNodeInfoHandle,
    knowledge: *const CrimsonKnowledgeInfoHandle,
    paloc: *const CrimsonPalocHandle,
    node_key: u32,
    buf: *mut u8,
    buf_len: usize,
    required: *mut usize,
) -> i32 {
    if handle.is_null() || knowledge.is_null() || paloc.is_null() || required.is_null() {
        return error::NULL_ARG;
    }
    if buf.is_null() && buf_len != 0 {
        return error::NULL_ARG;
    }
    unsafe { *required = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let kh = unsafe { &*knowledge };
        let ph = unsafe { &*paloc };
        let Some(&knowledge_key) = h.knowledge_by_key.get(&node_key) else {
            return error::NOT_FOUND;
        };
        let Some(name) = kh.name(knowledge_key) else {
            return error::NOT_FOUND;
        };
        let hi32 = u64::from(crate::crypto::checksum::calculate_checksum(name.as_bytes()));
        let decimal = format!("{}", (hi32 << 32) | 0x490);
        let Some(display) = ph.lookup_str(&decimal) else {
            return error::NOT_FOUND;
        };
        write_str_to_buf(display, buf, buf_len, required)
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
    use crate::binary::gamedata_layout;
    use super::*;
    use crate::c_abi::paz::crimson_paz_extract_file;
    use std::ffi::{CStr, CString};
    use std::path::PathBuf;
    use std::ptr;

    const KNOWN: &[(u32, &str)] = &[
        (1000401, "Node_Her_Temporary_Camp"),
        (1000044, "Node_Her_HernandCastle"),
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
    fn c_abi_factionnode_live() {
        let Some(pamt_path) = find_pamt() else {
            eprintln!("skipping c_abi_factionnode_live: no game install");
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let pabgb = extract_file(
            pamt.as_c_str(),
            gamedata_layout::bin_dir(),
            &gamedata_layout::body("factionnode"),
        );
        let pabgh = extract_file(
            pamt.as_c_str(),
            gamedata_layout::bin_dir(),
            &gamedata_layout::header("factionnode"),
        );

        let mut sh: *mut CrimsonFactionNodeInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_factionnode_load_from_bytes(
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
            unsafe { crimson_factionnode_entry_count(sh, &mut count) },
            error::OK
        );
        assert!(count > 1000, "expected >1000 factionnode rows, got {count}");

        for &(key, expected) in KNOWN {
            let mut req: usize = 0;
            let rc_size = unsafe {
                crimson_factionnode_lookup_string_key(
                    sh,
                    key,
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            };
            let got = read_string_result(rc_size, req, |b, n, r| unsafe {
                crimson_factionnode_lookup_string_key(sh, key, b, n, r)
            });
            assert_eq!(got, expected, "FactionNodeKey {key} name mismatch");
        }

        let mut req: usize = 0;
        assert_eq!(
            unsafe {
                crimson_factionnode_lookup_string_key(
                    sh,
                    u32::MAX,
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            },
            error::NOT_FOUND,
        );

        unsafe { crimson_factionnode_free(sh) };
    }

    /// `FactionNodeKey → KnowledgeKey → English place name`, on every row.
    #[test]
    fn c_abi_factionnode_display_name_live() {
        use crate::c_abi::knowledge_info::{
            crimson_knowledgeinfo_free, crimson_knowledgeinfo_load_from_bytes_with_header,
        };
        use crate::c_abi::paloc::{crimson_paloc_free, crimson_paloc_load_from_bytes};
        let Some(pamt_path) = find_pamt() else {
            eprintln!("skipping c_abi_factionnode_display_name_live: no game install");
            return;
        };
        let Some(paloc_bytes) = gamedata_layout::paloc_bytes("0020", "eng") else {
            eprintln!("skipping c_abi_factionnode_display_name_live: no eng paloc");
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let get = |name: &str| extract_file(pamt.as_c_str(), gamedata_layout::bin_dir(), name);
        let (nb, nh) = (get(&gamedata_layout::body("factionnode")), get(&gamedata_layout::header("factionnode")));
        let (kb, kh_bytes) = (get(&gamedata_layout::body("knowledgeinfo")), get(&gamedata_layout::header("knowledgeinfo")));

        let mut sh: *mut CrimsonFactionNodeInfoHandle = ptr::null_mut();
        let mut kh: *mut CrimsonKnowledgeInfoHandle = ptr::null_mut();
        let mut ph: *mut CrimsonPalocHandle = ptr::null_mut();
        unsafe {
            assert_eq!(crimson_factionnode_load_from_bytes(nb.as_ptr(), nb.len(), nh.as_ptr(), nh.len(), &mut sh), error::OK);
            assert_eq!(
                crimson_knowledgeinfo_load_from_bytes_with_header(kb.as_ptr(), kb.len(), kh_bytes.as_ptr(), kh_bytes.len(), &mut kh),
                error::OK
            );
            assert_eq!(crimson_paloc_load_from_bytes(paloc_bytes.as_ptr(), paloc_bytes.len(), &mut ph), error::OK);
        }

        const PLACES: &[(u32, u32, &str)] = &[
            (1000044, 1000016, "Hernand Castle"),
            (1000003, 1001809, "Howling Hill"),
            (1000311, 1000350, "Glenbright Farm"),
        ];
        for &(node, knowledge, place) in PLACES {
            let mut k = 0u32;
            assert_eq!(unsafe { crimson_factionnode_lookup_knowledge_key(sh, node, &mut k) }, error::OK);
            assert_eq!(k, knowledge, "node {node}");
            let mut req = 0usize;
            let rc = unsafe { crimson_factionnode_lookup_display_name(sh, kh, ph, node, ptr::null_mut(), 0, &mut req) };
            let got = read_string_result(rc, req, |b, n, r| unsafe {
                crimson_factionnode_lookup_display_name(sh, kh, ph, node, b, n, r)
            });
            assert_eq!(got, place, "node {node}");
        }

        // Every node has a knowledge key and an English title.
        let h = unsafe { &*sh };
        let missing: Vec<u32> = h
            .entries
            .iter()
            .map(|&(key, _)| key)
            .filter(|&key| {
                let mut req = 0usize;
                let rc = unsafe { crimson_factionnode_lookup_display_name(sh, kh, ph, key, ptr::null_mut(), 0, &mut req) };
                rc != error::BUFFER_TOO_SMALL
            })
            .collect();
        assert!(missing.is_empty(), "{} nodes without a place name, e.g. {:?}", missing.len(), &missing[..missing.len().min(5)]);

        let mut k = 0u32;
        assert_eq!(unsafe { crimson_factionnode_lookup_knowledge_key(sh, u32::MAX, &mut k) }, error::NOT_FOUND);
        unsafe {
            crimson_paloc_free(ph);
            crimson_knowledgeinfo_free(kh);
            crimson_factionnode_free(sh);
        }
    }

    #[test]
    fn c_abi_factionnode_null_args() {
        let mut sh: *mut CrimsonFactionNodeInfoHandle = ptr::null_mut();
        assert_eq!(
            unsafe {
                crimson_factionnode_load_from_bytes(
                    ptr::null(),
                    16,
                    ptr::null(),
                    16,
                    &mut sh,
                )
            },
            error::NULL_ARG,
        );
        assert_eq!(
            unsafe {
                crimson_factionnode_load_from_bytes(
                    [0u8; 1].as_ptr(),
                    1,
                    [0u8; 1].as_ptr(),
                    1,
                    ptr::null_mut(),
                )
            },
            error::NULL_ARG,
        );
    }

    #[test]
    fn c_abi_factionnode_empty_bytes_yields_empty_handle() {
        let mut sh: *mut CrimsonFactionNodeInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_factionnode_load_from_bytes(ptr::null(), 0, ptr::null(), 0, &mut sh)
        };
        assert_eq!(rc, error::OK);
        assert!(!sh.is_null());
        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_factionnode_entry_count(sh, &mut count) },
            error::OK
        );
        assert_eq!(count, 0);
        unsafe { crimson_factionnode_free(sh) };
    }
}

#[cfg(test)]
mod dlc_tests {
    //! Merging `dlc_01`'s factionnode + knowledgeinfo copies. Skips when the
    //! DLC is not installed; the base-only path is what the other tests pin.
    use super::*;
    use crate::binary::gamedata_layout::{body, extract_bin, extract_dlc_bin, header, paloc_bytes};
    use crate::c_abi::knowledge_info::{
        crimson_knowledgeinfo_entry_count, crimson_knowledgeinfo_free,
        crimson_knowledgeinfo_entry_source, crimson_knowledgeinfo_load_from_bytes_with_header,
        crimson_knowledgeinfo_merge_dlc_from_bytes,
    };
    use crate::c_abi::paloc::{crimson_paloc_free, crimson_paloc_load_from_bytes};
    use std::ptr;

    fn name(sh: *const CrimsonFactionNodeInfoHandle, kh: *const CrimsonKnowledgeInfoHandle, ph: *const CrimsonPalocHandle, node: u32) -> Option<String> {
        let mut req = 0usize;
        if unsafe { crimson_factionnode_lookup_display_name(sh, kh, ph, node, ptr::null_mut(), 0, &mut req) } != error::BUFFER_TOO_SMALL {
            return None;
        }
        let mut buf = vec![0u8; req];
        let rc = unsafe { crimson_factionnode_lookup_display_name(sh, kh, ph, node, buf.as_mut_ptr(), buf.len(), &mut req) };
        assert_eq!(rc, error::OK);
        Some(String::from_utf8(buf[..req - 1].to_vec()).unwrap())
    }

    #[test]
    fn c_abi_factionnode_dlc_merge_live() {
        let (Some(nb), Some(nh), Some(kb), Some(khd), Some(pal)) = (
            extract_bin(&body("factionnode")),
            extract_bin(&header("factionnode")),
            extract_bin(&body("knowledgeinfo")),
            extract_bin(&header("knowledgeinfo")),
            paloc_bytes("0020", "eng"),
        ) else {
            eprintln!("skipping c_abi_factionnode_dlc_merge_live: no game install");
            return;
        };
        let (Some(dnb), Some(dnh), Some(dkb), Some(dkh)) = (
            extract_dlc_bin("dlc_01", &body("factionnode.dlc_01")),
            extract_dlc_bin("dlc_01", &header("factionnode.dlc_01")),
            extract_dlc_bin("dlc_01", &body("knowledgeinfo.dlc_01")),
            extract_dlc_bin("dlc_01", &header("knowledgeinfo.dlc_01")),
        ) else {
            eprintln!("skipping c_abi_factionnode_dlc_merge_live: dlc_01 not installed");
            return;
        };
        let mut sh: *mut CrimsonFactionNodeInfoHandle = ptr::null_mut();
        let mut kh: *mut CrimsonKnowledgeInfoHandle = ptr::null_mut();
        let mut ph: *mut CrimsonPalocHandle = ptr::null_mut();
        unsafe {
            assert_eq!(crimson_factionnode_load_from_bytes(nb.as_ptr(), nb.len(), nh.as_ptr(), nh.len(), &mut sh), error::OK);
            assert_eq!(crimson_knowledgeinfo_load_from_bytes_with_header(kb.as_ptr(), kb.len(), khd.as_ptr(), khd.len(), &mut kh), error::OK);
            assert_eq!(crimson_paloc_load_from_bytes(pal.as_ptr(), pal.len(), &mut ph), error::OK);
        }
        let (mut base_nodes, mut base_knowledge) = (0u32, 0u32);
        unsafe {
            crimson_factionnode_entry_count(sh, &mut base_nodes);
            crimson_knowledgeinfo_entry_count(kh, &mut base_knowledge);
        }

        // Without the DLC: a DLC-only node is simply unknown.
        const ILIYA: u32 = 1_000_566; // Node_Ocean_IliyaIsland, dlc_01 only
        assert_eq!(name(sh, kh, ph, ILIYA), None);

        let (mut added_nodes, mut added_knowledge) = (0u32, 0u32);
        unsafe {
            assert_eq!(crimson_factionnode_merge_dlc_from_bytes(sh, dnb.as_ptr(), dnb.len(), dnh.as_ptr(), dnh.len(), &mut added_nodes), error::OK);
            assert_eq!(crimson_knowledgeinfo_merge_dlc_from_bytes(kh, dkb.as_ptr(), dkb.len(), dkh.as_ptr(), dkh.len(), &mut added_knowledge), error::OK);
        }
        // 2.06: 142 nodes and 795 knowledge rows exist only in dlc_01.
        eprintln!("dlc_01 merge: +{added_nodes} nodes, +{added_knowledge} knowledge rows");
        assert!(added_nodes >= 100, "added {added_nodes} DLC nodes");
        assert!(added_knowledge >= 500, "added {added_knowledge} DLC knowledge rows");
        let mut total = 0u32;
        unsafe { crimson_factionnode_entry_count(sh, &mut total) };
        assert_eq!(total, base_nodes + added_nodes);

        let mut src = 9u32;
        unsafe {
            assert_eq!(crimson_factionnode_entry_source(sh, 0, &mut src), error::OK);
            assert_eq!(src, 0);
            assert_eq!(crimson_factionnode_entry_source(sh, total - 1, &mut src), error::OK);
            assert_eq!(src, 1);
            assert_eq!(crimson_factionnode_entry_source(sh, total, &mut src), error::OUT_OF_RANGE);
            let mut ksrc = 9u32;
            assert_eq!(crimson_knowledgeinfo_entry_source(kh, base_knowledge, &mut ksrc), error::OK);
            assert_eq!(ksrc, 1);
        }
        assert_eq!(name(sh, kh, ph, ILIYA).as_deref(), Some("Iliya Island"));
        assert_eq!(name(sh, kh, ph, 1_000_044).as_deref(), Some("Hernand Castle"));

        // Every node, base and DLC, has a place name once both are merged.
        let h = unsafe { &*sh };
        let missing = h.entries.iter().filter(|&&(k, _)| name(sh, kh, ph, k).is_none()).count();
        assert_eq!(missing, 0, "nodes without a place name after the DLC merge");

        // Merging the same table again adds nothing.
        let mut again = 9u32;
        unsafe {
            assert_eq!(crimson_factionnode_merge_dlc_from_bytes(sh, dnb.as_ptr(), dnb.len(), dnh.as_ptr(), dnh.len(), &mut again), error::OK);
        }
        assert_eq!(again, 0);

        unsafe {
            crimson_paloc_free(ph);
            crimson_knowledgeinfo_free(kh);
            crimson_factionnode_free(sh);
        }
    }

    #[test]
    fn c_abi_factionnode_dlc_merge_args() {
        let mut sh: *mut CrimsonFactionNodeInfoHandle = ptr::null_mut();
        assert_eq!(unsafe { crimson_factionnode_load_from_bytes(ptr::null(), 0, ptr::null(), 0, &mut sh) }, error::OK);
        let mut added = 9u32;
        unsafe {
            assert_eq!(crimson_factionnode_merge_dlc_from_bytes(ptr::null_mut(), ptr::null(), 0, ptr::null(), 0, &mut added), error::NULL_ARG);
            assert_eq!(crimson_factionnode_merge_dlc_from_bytes(sh, ptr::null(), 4, ptr::null(), 0, &mut added), error::NULL_ARG);
            // No header: refused, nothing added (audit D-05).
            assert_eq!(crimson_factionnode_merge_dlc_from_bytes(sh, ptr::null(), 0, ptr::null(), 0, &mut added), error::BODY_PARSE);
            assert_eq!(added, 0);
            let mut src = 0u32;
            assert_eq!(crimson_factionnode_entry_source(sh, 0, &mut src), error::OUT_OF_RANGE);
            assert_eq!(crimson_factionnode_entry_source(sh, 0, ptr::null_mut()), error::NULL_ARG);
            crimson_factionnode_free(sh);
        }
    }
}
