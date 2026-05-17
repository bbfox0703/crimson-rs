//! `characterinfo.pabgb` bridge — C ABI surface.
//!
//! Resolves save-side `CharacterKey (u32)` — the `_characterKey` field
//! that sits alongside `FieldNPCSaveDataKey` (and elsewhere) inside
//! `FieldNPCSaveData` blocks. The save key carries a "cat byte" in its
//! hi-byte (`0x02..=0xfe`, variant / region / faction marker that this
//! bridge strips) and a 24-bit row ID in the lo24.
//!
//! Three levels of resolution exposed:
//!
//! 1. [`crimson_characterinfo_lookup_string_key`] — internal name from
//!    the `characterinfo.pabgb` row itself (e.g. `"Yann_Friendly"`,
//!    `"FieldNPC_Bandit_Lvl3"`). Always succeeds when the lo24 lives
//!    in the table.
//! 2. [`crimson_characterinfo_lookup_display_name`] — localized display
//!    string via PALOC at `((charkey & 0xFFFFFF) << 32) | lo32_namespace`.
//!    **No hash hop** (unlike mission/quest/stage/knowledge) — the
//!    stripped lo24 is the PALOC hi32 directly. `lo32 = 0x30` is the
//!    common case (character display name); a handful of characters
//!    also have entries at other lo32 values.
//! 3. [`crimson_characterinfo_resolve_portrait`] — high-level chain
//!    that resolves the display name then matches it against the
//!    portrait DDS list produced by
//!    [`super::paz::crimson_paz_list_npc_portraits`], returning the
//!    best-scoring portrait path. Mirrors CrimsonForge's
//!    `_match_media_for_record` algorithm in `core/asset_catalog.py`.
//!
//! Coverage observed on the editor's 221-key sample save (§6 of
//! `docs/save-editor-keys-plan.md`): 49/221 (22%) of `_characterKey`
//! values resolve through the PALOC chain. The 78% miss path is
//! sample-bias rather than missing data — generic field NPCs that DO
//! exist in PALOC simply don't appear as save instances. The bridge
//! surfaces a miss as `NOT_FOUND` so the caller can fall back to the
//! internal-name surface.

use std::collections::HashMap;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::error;
use super::paloc::CrimsonPalocHandle;
use crate::character_info::parse_character_info_lossy;

/// Default PALOC namespace for character display names. The vast
/// majority of named characters resolve at this lo32.
pub const CHARACTER_DISPLAY_NAME_LO32: u32 = 0x30;

/// Opaque handle exposing `(CharacterKey lo24, internal_name)`
/// lookups against the loaded `characterinfo.pabgb`.
#[repr(C)]
pub struct CrimsonCharacterInfoHandle {
    by_key: HashMap<u32, String>,
    entries: Vec<(u32, String)>,
}

impl CrimsonCharacterInfoHandle {
    fn from_bytes(data: &[u8]) -> Self {
        let raw = parse_character_info_lossy(data);
        let mut by_key: HashMap<u32, String> = HashMap::with_capacity(raw.len());
        let mut entries: Vec<(u32, String)> = Vec::with_capacity(raw.len());
        for e in raw {
            if let std::collections::hash_map::Entry::Vacant(v) = by_key.entry(e.key) {
                v.insert(e.name.clone());
                entries.push((e.key, e.name));
            }
        }
        CrimsonCharacterInfoHandle { by_key, entries }
    }
}

// ── Load / free ────────────────────────────────────────────────────────────

/// Parse `characterinfo.pabgb` from disk.
///
/// # Safety
/// `path` must be a NUL-terminated UTF-8 string; `out_handle` must be
/// non-null and writable for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_characterinfo_load_from_file(
    path: *const c_char,
    out_handle: *mut *mut CrimsonCharacterInfoHandle,
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
        let handle = CrimsonCharacterInfoHandle::from_bytes(&bytes);
        unsafe { *out_handle = Box::into_raw(Box::new(handle)) };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Parse characterinfo bytes already in memory.
///
/// # Safety
/// `data` must point to `data_len` readable bytes (may be null iff
/// `data_len == 0`); `out_handle` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_characterinfo_load_from_bytes(
    data: *const u8,
    data_len: usize,
    out_handle: *mut *mut CrimsonCharacterInfoHandle,
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
        let handle = CrimsonCharacterInfoHandle::from_bytes(slice);
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
pub unsafe extern "C" fn crimson_characterinfo_free(handle: *mut CrimsonCharacterInfoHandle) {
    if handle.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(handle);
    }
}

// ── Scalar getters ─────────────────────────────────────────────────────────

/// Total number of characters in the loaded table.
///
/// # Safety
/// `handle` must be live; `out_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_characterinfo_entry_count(
    handle: *const CrimsonCharacterInfoHandle,
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

/// Look up the internal name for a given `CharacterKey (u32)` and
/// write it into `buf` (NUL-terminated UTF-8). The cat-byte in the
/// save-side key's hi-byte is stripped before the lookup so any value
/// of the form `0xXX_NNNNNN` resolves identically.
///
/// Returns `NOT_FOUND` when the stripped lo24 isn't in the table.
///
/// # Safety
/// `handle` and `required` must be non-null; `buf` may be null iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_characterinfo_lookup_string_key(
    handle: *const CrimsonCharacterInfoHandle,
    character_key: u32,
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
        let lo24 = character_key & 0x00FF_FFFF;
        let Some(name) = h.by_key.get(&lo24) else {
            return error::NOT_FOUND;
        };
        write_str_to_buf(name, buf, buf_len, required)
    }))
    .unwrap_or(error::PANIC)
}

// ── Display-name lookup ────────────────────────────────────────────────────

/// One-shot display name resolution: `CharacterKey → PALOC at
/// `((charkey & 0xFFFFFF) << 32) | lo32_namespace` → localized string`.
/// Strips the cat-byte before composing the PALOC key. Writes the
/// result into `buf` (NUL-terminated UTF-8).
///
/// **No hash hop** — the save's lo24 is the PALOC hi32 directly,
/// matching the gimmickinfo/sublevel pattern.
///
/// `lo32_namespace = 0x30` (= [`CHARACTER_DISPLAY_NAME_LO32`]) is the
/// common case for the display name. Other lo32 values may exist for
/// secondary text (faction title etc.) but haven't been mapped out
/// systematically.
///
/// The `handle` parameter is retained for API symmetry with sibling
/// bridges and so a future cat-byte transform (e.g. variant-specific
/// display) can hook in without an ABI break. It is not consulted on
/// the resolution path today.
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
/// for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_characterinfo_lookup_display_name(
    handle: *const CrimsonCharacterInfoHandle,
    paloc_handle: *const CrimsonPalocHandle,
    character_key: u32,
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
        let paloc = unsafe { &*paloc_handle };
        let lo24 = u64::from(character_key & 0x00FF_FFFF);
        let u64_key = (lo24 << 32) | u64::from(lo32_namespace);
        let decimal = format!("{u64_key}");
        let Some(display) = paloc.lookup_str(&decimal) else {
            return error::NOT_FOUND;
        };
        write_str_to_buf(display, buf, buf_len, required)
    }))
    .unwrap_or(error::PANIC)
}

// ── Enumeration ────────────────────────────────────────────────────────────

/// Get the `(character_key, internal_name)` pair at insertion index
/// `idx`. Two-call pattern.
///
/// # Safety
/// `handle`, `out_key`, and `required` must be non-null; `buf` may be
/// null iff `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_characterinfo_get_entry(
    handle: *const CrimsonCharacterInfoHandle,
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

// ── High-level: CharacterKey → portrait DDS ────────────────────────────────

/// Resolve a `CharacterKey` to the best-matching NPC portrait DDS path
/// in a single call.
///
/// Inputs:
/// - `handle` — loaded `characterinfo.pabgb`.
/// - `paloc_handle` — loaded English (or other-language) PALOC.
/// - `character_key` — save-side `_characterKey` value (with cat-byte).
/// - `portrait_list_buf` / `portrait_list_len` — the NUL-separated
///   list produced by [`super::paz::crimson_paz_list_npc_portraits`].
///   May be empty (`len == 0` and `buf == null`) — the function then
///   returns `NOT_FOUND` immediately.
/// - `out_buf` / `out_buf_len` / `out_required` — standard two-call
///   buffer for the winning portrait's `<dir>/<filename>` path
///   (NUL-terminated UTF-8).
/// - `out_score` (optional, may be null) — receives the match score
///   so the caller can apply their own confidence threshold. Score is
///   `0` (no match) up to ~`100` (exact normalised match). Below ~`50`
///   the match is suggestive at best; below ~`30` it's noise.
///
/// Algorithm:
///
/// 1. Try the PALOC display name at `lo32 = 0x30`. If hit, the
///    matcher uses that string as the primary signal.
/// 2. Always also pull the internal name from `characterinfo` (if
///    present) as a secondary signal at half weight.
/// 3. For each portrait in the list, extract the token between the
///    six known NPC-portrait prefixes and the trailing `.dds`,
///    normalise (lowercase, non-alnum → `_`, strip), and score it
///    against the normalised display / internal names.
/// 4. Return the highest-scoring portrait. Ties are broken by the
///    order entries appear in the list (= PAMT iteration order).
///
/// Returns `NOT_FOUND` if the character has no name at either source
/// (display chain misses AND `lookup_string_key` misses), or if no
/// portrait's token has a non-zero score against the resolved names.
///
/// # Safety
/// All non-null pointers must reference live memory for the duration
/// of the call. The portrait list buffer must match the format
/// emitted by `crimson_paz_list_npc_portraits` (NUL-separated UTF-8
/// paths, optionally without a final trailing extra NUL).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_characterinfo_resolve_portrait(
    handle: *const CrimsonCharacterInfoHandle,
    paloc_handle: *const CrimsonPalocHandle,
    character_key: u32,
    portrait_list_buf: *const u8,
    portrait_list_len: usize,
    out_buf: *mut u8,
    out_buf_len: usize,
    out_required: *mut usize,
    out_score: *mut i32,
) -> i32 {
    if handle.is_null() || paloc_handle.is_null() || out_required.is_null() {
        return error::NULL_ARG;
    }
    if portrait_list_buf.is_null() && portrait_list_len != 0 {
        return error::NULL_ARG;
    }
    if out_buf.is_null() && out_buf_len != 0 {
        return error::NULL_ARG;
    }
    unsafe { *out_required = 0 };
    if !out_score.is_null() {
        unsafe { *out_score = 0 };
    }
    catch_unwind(AssertUnwindSafe(|| {
        let cinfo = unsafe { &*handle };
        let paloc = unsafe { &*paloc_handle };
        let lo24 = character_key & 0x00FF_FFFF;

        // ── Pull names (display + internal) ────────────────────────────
        let display_name: Option<String> = {
            let u64_key = (u64::from(lo24) << 32) | u64::from(CHARACTER_DISPLAY_NAME_LO32);
            paloc.lookup_str(&format!("{u64_key}")).map(str::to_owned)
        };
        let internal_name: Option<&str> = cinfo.by_key.get(&lo24).map(String::as_str);
        if display_name.is_none() && internal_name.is_none() {
            return error::NOT_FOUND;
        }

        // ── Slice the portrait list ────────────────────────────────────
        let list_slice: &[u8] = if portrait_list_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(portrait_list_buf, portrait_list_len) }
        };

        // ── Walk portraits, score each, keep the best ──────────────────
        let display_key = display_name.as_deref().map(normalise_lookup_key);
        let internal_key = internal_name.map(normalise_lookup_key);

        let mut best: Option<(i32, &[u8])> = None;
        for path_bytes in list_slice.split(|&b| b == 0) {
            if path_bytes.is_empty() {
                continue;
            }
            let Ok(path) = std::str::from_utf8(path_bytes) else {
                continue;
            };
            let Some(token) = extract_portrait_token(path) else {
                continue;
            };
            let token_norm = normalise_lookup_key(token);
            if token_norm.is_empty() {
                continue;
            }

            // Display name carries full weight; internal name half. The
            // weights mirror CrimsonForge's media-match scoring spirit
            // without copying the per-tier table verbatim — we only
            // have two signals (no aliases / customisation hints), so
            // a simpler tier suffices.
            let mut score = 0i32;
            if let Some(key) = display_key.as_deref() {
                score = score.max(score_match(key, &token_norm));
            }
            if let Some(key) = internal_key.as_deref() {
                score = score.max(score_match(key, &token_norm) / 2);
            }
            if score == 0 {
                continue;
            }
            match best {
                Some((bs, _)) if bs >= score => {}
                _ => best = Some((score, path_bytes)),
            }
        }

        let Some((score, path_bytes)) = best else {
            return error::NOT_FOUND;
        };
        if !out_score.is_null() {
            unsafe { *out_score = score };
        }
        // SAFETY: extract_portrait_token returned Some, which means
        // path_bytes parsed as UTF-8 above — we re-wrap that slice as
        // &str without re-validating.
        let path_str = unsafe { std::str::from_utf8_unchecked(path_bytes) };
        write_str_to_buf(path_str, out_buf, out_buf_len, out_required)
    }))
    .unwrap_or(error::PANIC)
}

// ── Internal: portrait matching ────────────────────────────────────────────

/// The six NPC-portrait filename prefixes recognised across 1.05 /
/// 1.06 / 1.07. Mirrors [`super::paz`]'s private list — duplicated
/// here so the two modules stay decoupled (the matcher needs to know
/// what to strip off filenames; the lister only needs to know what
/// to include).
const NPC_PORTRAIT_PREFIXES: &[&str] = &[
    "cd_portraitimage_character_",
    "cd_portraitimage_chracter_",
    "cd_portraitimage_nhm_",
    "cd_portraitimage_nom_",
    "cd_portraitimage_muscan_",
    "cd_mercenary_portrait_",
];

/// Strip the directory + recognised prefix + `.dds` suffix from a
/// portrait path, returning just the token. Case-insensitive on the
/// prefix/suffix. Returns `None` if the path doesn't end in `.dds`
/// or doesn't start with one of the known prefixes.
fn extract_portrait_token(path: &str) -> Option<&str> {
    const SUFFIX: &str = ".dds";
    // Drop everything up through the final '/'. Portrait paths always
    // include a directory.
    let filename = path.rsplit_once('/').map_or(path, |(_, name)| name);
    if !filename
        .get(filename.len().saturating_sub(SUFFIX.len())..)
        .is_some_and(|s| s.eq_ignore_ascii_case(SUFFIX))
    {
        return None;
    }
    for prefix in NPC_PORTRAIT_PREFIXES {
        if filename.len() <= prefix.len() + SUFFIX.len() {
            continue;
        }
        if filename
            .get(..prefix.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(prefix))
        {
            return Some(&filename[prefix.len()..filename.len() - SUFFIX.len()]);
        }
    }
    None
}

/// Normalise a string for fuzzy match: lowercase, replace runs of
/// non-alnum with single `_`, strip leading / trailing `_`. Mirrors
/// CrimsonForge's `_normalize_lookup_key` (regex
/// `[^a-z0-9]+` → `_`, then strip).
fn normalise_lookup_key(input: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut last_was_underscore = true; // suppress leading `_`
    for c in lower.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            last_was_underscore = false;
        } else if !last_was_underscore {
            out.push('_');
            last_was_underscore = true;
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    out
}

/// Score a normalised lookup key against a normalised portrait token.
/// Higher is better; 0 means no match.
///
/// Tiers (loosely modelled on CrimsonForge's media-match weights):
/// - 100 — exact match
/// - 80 — token starts with `key_` or ends with `_key`
/// - 65 — `_key_` appears mid-token (word-bounded substring)
/// - 45 — `key` appears anywhere in token (raw substring)
/// - 25 — `key` appears in `token.replace("_", "")` (collapsed
///   substring, catches `pierre` in `npc_pierre_lvl3` and also
///   `jeansoldier` in `jean_soldier`)
/// - 0 otherwise
fn score_match(key: &str, token: &str) -> i32 {
    if key.is_empty() || token.is_empty() {
        return 0;
    }
    if key == token {
        return 100;
    }
    let bounded_prefix = format!("{key}_");
    let bounded_suffix = format!("_{key}");
    if token.starts_with(&bounded_prefix) || token.ends_with(&bounded_suffix) {
        return 80;
    }
    let bounded_mid = format!("_{key}_");
    if token.contains(&bounded_mid) {
        return 65;
    }
    if token.contains(key) {
        return 45;
    }
    let collapsed: String = token.chars().filter(|c| *c != '_').collect();
    if collapsed.contains(key) {
        return 25;
    }
    0
}

// ── Shared write helper ────────────────────────────────────────────────────

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
    //! Mix of pure unit tests (matching helpers, exercised without a
    //! game install) and a live integration test that walks the full
    //! chain: extract `characterinfo.pabgb` + English PALOC + portrait
    //! list, load the three into handles, resolve known CharacterKeys
    //! end-to-end, and assert the highest-scoring portrait is what we
    //! expect (`0x0a000001` should match the Kliff portrait).

    use super::*;
    use crate::c_abi::paloc::{crimson_paloc_free, crimson_paloc_load_from_bytes};
    use crate::c_abi::paz::{crimson_paz_extract_file, crimson_paz_list_npc_portraits};
    use std::ffi::{CStr, CString};
    use std::path::PathBuf;
    use std::ptr;

    // ── Pure unit tests ────────────────────────────────────────────────

    #[test]
    fn normalise_lookup_key_basic() {
        assert_eq!(normalise_lookup_key("Yann"), "yann");
        assert_eq!(normalise_lookup_key("Bruna's Assistant"), "bruna_s_assistant");
        assert_eq!(normalise_lookup_key("  hello  "), "hello");
        assert_eq!(normalise_lookup_key("FieldNPC_Bandit_Lvl3"), "fieldnpc_bandit_lvl3");
        assert_eq!(normalise_lookup_key("__a__b__"), "a_b");
        assert_eq!(normalise_lookup_key(""), "");
        assert_eq!(normalise_lookup_key("!!!"), "");
    }

    #[test]
    fn extract_portrait_token_basic() {
        // Each of the six known prefixes resolves.
        assert_eq!(
            extract_portrait_token(
                "ui/texture/image/portraitimage/cd_portraitimage_chracter_kliff.dds"
            ),
            Some("kliff")
        );
        assert_eq!(
            extract_portrait_token(
                "ui/texture/image/portraitimage/cd_portraitimage_nhm_hernand_soldiers_01.dds"
            ),
            Some("hernand_soldiers_01")
        );
        assert_eq!(
            extract_portrait_token(
                "ui/texture/image/portraitimage/cd_mercenary_portrait_54632.dds"
            ),
            Some("54632")
        );
        // Case-insensitive.
        assert_eq!(
            extract_portrait_token(
                "UI/CD_PORTRAITIMAGE_CHRACTER_DEMIAN.DDS"
            ),
            Some("DEMIAN")
        );
        // Non-NPC portraits → None.
        assert_eq!(
            extract_portrait_token(
                "ui/texture/image/portraitimage/cd_portraitimage_animal_wolf.dds"
            ),
            None
        );
        // Wrong suffix.
        assert_eq!(
            extract_portrait_token("ui/cd_portraitimage_chracter_demian.png"),
            None
        );
        // Empty stem (prefix immediately followed by suffix) → None,
        // matching `is_npc_portrait_name` in `super::paz` — the filter
        // contract is that both modules agree on what counts as a
        // valid NPC portrait path.
        assert_eq!(
            extract_portrait_token("ui/cd_portraitimage_chracter_.dds"),
            None
        );
    }

    #[test]
    fn score_match_tiers() {
        // Exact.
        assert_eq!(score_match("kliff", "kliff"), 100);
        // Boundary prefix / suffix.
        assert_eq!(score_match("kliff", "kliff_lvl3"), 80);
        assert_eq!(score_match("kliff", "npc_kliff"), 80);
        // Word-bounded mid.
        assert_eq!(score_match("kliff", "npc_kliff_lvl3"), 65);
        // Raw substring (no word boundary).
        assert_eq!(score_match("kliff", "kliffsworn"), 45);
        // Collapsed substring.
        assert_eq!(score_match("kliff", "k_l_i_ff"), 25);
        // No hit.
        assert_eq!(score_match("kliff", "demian"), 0);
        // Empty inputs → 0.
        assert_eq!(score_match("", "kliff"), 0);
        assert_eq!(score_match("kliff", ""), 0);
    }

    // ── Live integration ──────────────────────────────────────────────

    fn find_pamt_0008() -> Option<PathBuf> {
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
    fn c_abi_characterinfo_resolve_portrait_kliff() {
        let Some(pamt_path) = find_pamt_0008() else {
            eprintln!("skipping c_abi_characterinfo_resolve_portrait_kliff: no game install");
            return;
        };
        let game_root = pamt_path.parent().unwrap().parent().unwrap();
        let pamt_0008 = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let pamt_0012 = {
            let p = game_root.join("0012").join("0.pamt");
            if !p.is_file() {
                eprintln!("skipping: no 0012/0.pamt for portrait list");
                return;
            }
            CString::new(p.to_str().unwrap()).unwrap()
        };
        let pamt_0020 = {
            let p = game_root.join("0020").join("0.pamt");
            if !p.is_file() {
                eprintln!("skipping: no 0020/0.pamt for English PALOC");
                return;
            }
            CString::new(p.to_str().unwrap()).unwrap()
        };

        // ── characterinfo handle ─────────────────────────────────────
        let cinfo_bytes = extract_file(
            pamt_0008.as_c_str(),
            "gamedata/binary__/client/bin",
            "characterinfo.pabgb",
        );
        let mut cinfo_h: *mut CrimsonCharacterInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_characterinfo_load_from_bytes(
                cinfo_bytes.as_ptr(),
                cinfo_bytes.len(),
                &mut cinfo_h,
            )
        };
        assert_eq!(rc, error::OK);
        let mut count: u32 = 0;
        unsafe { crimson_characterinfo_entry_count(cinfo_h, &mut count) };
        assert!(count > 5_000, "expected >5000 character entries, got {count}");

        // ── PALOC handle ────────────────────────────────────────────
        let paloc_bytes = extract_file(
            pamt_0020.as_c_str(),
            "gamedata/stringtable/binary__",
            "localizationstring_eng.paloc",
        );
        let mut paloc_h: *mut CrimsonPalocHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_paloc_load_from_bytes(paloc_bytes.as_ptr(), paloc_bytes.len(), &mut paloc_h)
        };
        assert_eq!(rc, error::OK);

        // ── portrait list ───────────────────────────────────────────
        let portrait_list = {
            let mut required: usize = 0;
            let mut entry_count: u32 = 0;
            let rc = unsafe {
                crimson_paz_list_npc_portraits(
                    pamt_0012.as_ptr(),
                    ptr::null_mut(),
                    0,
                    &mut required,
                    &mut entry_count,
                )
            };
            assert_eq!(rc, error::BUFFER_TOO_SMALL);
            let mut buf = vec![0u8; required];
            let rc = unsafe {
                crimson_paz_list_npc_portraits(
                    pamt_0012.as_ptr(),
                    buf.as_mut_ptr(),
                    buf.len(),
                    &mut required,
                    &mut entry_count,
                )
            };
            assert_eq!(rc, error::OK);
            buf
        };

        // ── lookup_display_name — Kliff ──────────────────────────────
        // 0x0a000001 → "Kliff" per §6 (lo24=1 with cat-byte 0x0a).
        let mut req: usize = 0;
        let rc_size = unsafe {
            crimson_characterinfo_lookup_display_name(
                cinfo_h,
                paloc_h,
                0x0a00_0001,
                CHARACTER_DISPLAY_NAME_LO32,
                ptr::null_mut(),
                0,
                &mut req,
            )
        };
        let display = read_string_result(rc_size, req, |b, n, r| unsafe {
            crimson_characterinfo_lookup_display_name(
                cinfo_h,
                paloc_h,
                0x0a00_0001,
                CHARACTER_DISPLAY_NAME_LO32,
                b,
                n,
                r,
            )
        });
        assert_eq!(display, "Kliff", "PALOC chain should resolve 0x0a000001 → Kliff");

        // Cat-byte invariance: 0x07000001 (different cat-byte, same lo24)
        // should resolve to the same PALOC value.
        let mut req2: usize = 0;
        let rc_size = unsafe {
            crimson_characterinfo_lookup_display_name(
                cinfo_h,
                paloc_h,
                0x0700_0001,
                CHARACTER_DISPLAY_NAME_LO32,
                ptr::null_mut(),
                0,
                &mut req2,
            )
        };
        let display2 = read_string_result(rc_size, req2, |b, n, r| unsafe {
            crimson_characterinfo_lookup_display_name(
                cinfo_h,
                paloc_h,
                0x0700_0001,
                CHARACTER_DISPLAY_NAME_LO32,
                b,
                n,
                r,
            )
        });
        assert_eq!(display2, "Kliff", "cat-byte must be stripped before PALOC lookup");

        // ── resolve_portrait — Kliff → cd_portraitimage_chracter_kliff.dds ──
        let mut req: usize = 0;
        let mut score: i32 = 0;
        let rc_size = unsafe {
            crimson_characterinfo_resolve_portrait(
                cinfo_h,
                paloc_h,
                0x0a00_0001,
                portrait_list.as_ptr(),
                portrait_list.len(),
                ptr::null_mut(),
                0,
                &mut req,
                &mut score,
            )
        };
        let portrait = read_string_result(rc_size, req, |b, n, r| unsafe {
            crimson_characterinfo_resolve_portrait(
                cinfo_h,
                paloc_h,
                0x0a00_0001,
                portrait_list.as_ptr(),
                portrait_list.len(),
                b,
                n,
                r,
                &mut score,
            )
        });
        assert!(
            portrait.ends_with("/cd_portraitimage_chracter_kliff.dds"),
            "Kliff should match the chracter_kliff portrait, got {portrait} (score {score})"
        );
        assert_eq!(score, 100, "exact display-name match must score 100");

        // ── Unknown CharacterKey → NOT_FOUND ─────────────────────────
        let rc = unsafe {
            crimson_characterinfo_resolve_portrait(
                cinfo_h,
                paloc_h,
                0xFFFF_FFFF,
                portrait_list.as_ptr(),
                portrait_list.len(),
                ptr::null_mut(),
                0,
                &mut req,
                &mut score,
            )
        };
        assert_eq!(rc, error::NOT_FOUND);

        unsafe {
            crimson_characterinfo_free(cinfo_h);
            crimson_paloc_free(paloc_h);
        }
    }

    #[test]
    fn c_abi_characterinfo_null_args() {
        let mut ch: *mut CrimsonCharacterInfoHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_characterinfo_load_from_bytes(ptr::null(), 16, &mut ch) },
            error::NULL_ARG,
        );
        assert_eq!(
            unsafe {
                crimson_characterinfo_load_from_bytes([0u8; 1].as_ptr(), 1, ptr::null_mut())
            },
            error::NULL_ARG,
        );
        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_characterinfo_entry_count(ptr::null(), &mut count) },
            error::NULL_ARG,
        );
        let mut req: usize = 0;
        assert_eq!(
            unsafe {
                crimson_characterinfo_lookup_string_key(
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
                crimson_characterinfo_lookup_display_name(
                    ptr::null(),
                    ptr::null(),
                    0,
                    0x30,
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            },
            error::NULL_ARG,
        );
        let mut score: i32 = 0;
        assert_eq!(
            unsafe {
                crimson_characterinfo_resolve_portrait(
                    ptr::null(),
                    ptr::null(),
                    0,
                    ptr::null(),
                    0,
                    ptr::null_mut(),
                    0,
                    &mut req,
                    &mut score,
                )
            },
            error::NULL_ARG,
        );
    }

    #[test]
    fn c_abi_characterinfo_empty_bytes_yields_empty_handle() {
        let mut ch: *mut CrimsonCharacterInfoHandle = ptr::null_mut();
        let rc = unsafe { crimson_characterinfo_load_from_bytes(ptr::null(), 0, &mut ch) };
        assert_eq!(rc, error::OK);
        assert!(!ch.is_null());
        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_characterinfo_entry_count(ch, &mut count) },
            error::OK
        );
        assert_eq!(count, 0);
        unsafe { crimson_characterinfo_free(ch) };
    }

    #[test]
    fn c_abi_characterinfo_load_bad_path_returns_io() {
        let bad = CString::new("Z:\\definitely\\does\\not\\exist.pabgb").unwrap();
        let mut ch: *mut CrimsonCharacterInfoHandle = ptr::null_mut();
        let rc = unsafe { crimson_characterinfo_load_from_file(bad.as_ptr(), &mut ch) };
        assert_eq!(rc, error::IO);
        assert!(ch.is_null());
    }

    /// `CharacterAppearanceIndexKey` investigation probe — RE was
    /// suspended after this probe surfaced a structural gap, so the
    /// bridge is **not** shipped. The probe is kept here so the next
    /// session can pick the work up without redoing the discovery.
    ///
    /// What this probe pins (verified 2026-05-15 against the live
    /// 1.07 `slot0/save.save` + `0008/gamedata/binary__/client/bin/`):
    ///
    /// 1. **File location**: `characterappearanceindexinfo.pabgb`
    ///    (236 KB) + `.pabgh` (97 KB) sibling. PABGH/PABGB pair
    ///    pattern — same shape as `skill.pabgh` + `skill.pabgb`.
    /// 2. **PABGH schema**:
    ///    ```text
    ///    [u32 count = 8143]
    ///    [count × (u64 key, u32 offset)]
    ///    ```
    /// 3. **PABGB entry layout**: each entry = 8-byte key (matches
    ///    the pabgh key verbatim) + **21-byte opaque body**. All
    ///    entries observed are 29 bytes total. Body bytes are
    ///    dense binary parameters with no string fields — likely
    ///    layer / color / asset IDs that need IDA RE before they
    ///    can be exposed semantically.
    /// 4. **Save → PABGH transform** (verified):
    ///    ```rust
    ///    let b3   = ((save >> 24) & 0xFF) as i8;       // category, signed i8
    ///    let lo24 = save & 0x00FF_FFFF;                // appearance ID
    ///    let pabgh_key = ((b3 as i32) as u64) << 32 | lo24;
    ///    // byte 7 of save (top byte) is a variant marker — drop it
    ///    // bytes 4..=6 of save mirror byte 3's sign (sign-ext padding)
    ///    ```
    /// 5. **Why the bridge wasn't shipped**: of 122 distinct save-side
    ///    appearance keys in the sample, **only 16 (13%) map to a
    ///    PABGH entry**. The PABGH's `0xfffffffe` bucket (7,027 of
    ///    its 8,143 entries) contains lo32 ∈ {1, 2, 4, 6, 100,
    ///    400-459, …} — sparsely populated. The save uses lo24 = 11,
    ///    12, 13, 14, … which fall in gaps the PABGH skips. The
    ///    other 87% of save values likely reference a procedural
    ///    / template-generated appearance system that isn't in this
    ///    file. Shipping a 13%-coverage bridge with no body schema
    ///    (and therefore no human-readable output) wouldn't earn
    ///    its keep.
    ///
    /// Open RE questions when this work resumes — see §10 of
    /// `docs/save-editor-keys-plan.md`:
    /// - Where do the other 87% appearance refs resolve? Probably a
    ///   sibling table in 0008 we haven't located, or a runtime
    ///   procedural system not exposed to gamedata.
    /// - Body 21-byte schema — IDA-RE the appearance loader to
    ///   identify field layout (likely outfit IDs, color values,
    ///   accessory flags).
    ///
    /// Probe behaviour: validates the file location, pabgh schema,
    /// and the canonical save→pabgh transform end-to-end. Prints
    /// total / distinct save values, hit count, and per-category
    /// breakdown. Skips cleanly when the live save or game install
    /// is missing.
    #[test]
    #[ignore = "investigation only — appearance bridge RE deferred"]
    fn _probe_character_appearance_index() {
        use crate::binary::pamt::PackMeta;
        use crate::binary::paz;
        use crate::save::{Body, FieldValue, ScalarValue, Save};

        // ── Locate the live save ────────────────────────────────────
        let save_path = std::env::var_os("CRIMSON_LIVE_SAVE")
            .map(PathBuf::from)
            .or_else(|| {
                let appdata = std::env::var_os("LOCALAPPDATA")?;
                let root = PathBuf::from(appdata)
                    .join("Pearl Abyss")
                    .join("CD")
                    .join("save");
                std::fs::read_dir(&root).ok()?.flatten().find_map(|entry| {
                    let p = entry.path().join("slot0").join("save.save");
                    p.is_file().then_some(p)
                })
            });
        let Some(save_path) = save_path else {
            eprintln!("skipping: no live save");
            return;
        };
        eprintln!("save:  {}", save_path.display());

        // ── Locate the appearance index file ────────────────────────
        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let pamt_path = game_root.join("0008").join("0.pamt");
        let Ok(pamt_bytes) = std::fs::read(&pamt_path) else {
            eprintln!("skipping: no {}", pamt_path.display());
            return;
        };
        let pamt = PackMeta::parse(&pamt_bytes, None).expect("parse PAMT");
        let dir = pamt
            .directories
            .iter()
            .find(|d| d.path == "gamedata/binary__/client/bin")
            .expect("dir");
        let pabgh_file = dir
            .files
            .iter()
            .find(|f| f.name == "characterappearanceindexinfo.pabgh")
            .expect("pabgh entry missing — has PA renamed the file?");
        let pabgh_bytes = paz::extract_file(
            &game_root.join("0008"),
            pabgh_file,
            "gamedata/binary__/client/bin",
            &pamt.header.encrypt_info.encrypt_info,
        )
        .expect("extract pabgh");
        eprintln!("pabgh: {} bytes", pabgh_bytes.len());

        // ── Parse the pabgh: u32 count + (u64 key, u32 offset) ──────
        assert!(pabgh_bytes.len() >= 4, "pabgh truncated");
        let count = u32::from_le_bytes([
            pabgh_bytes[0], pabgh_bytes[1], pabgh_bytes[2], pabgh_bytes[3],
        ]) as usize;
        let expected = 4 + count * 12;
        assert_eq!(
            pabgh_bytes.len(),
            expected,
            "pabgh size mismatch — schema may have drifted (count={count}, expected {expected})"
        );
        let mut pabgh_by_key: std::collections::HashMap<u64, u32> =
            std::collections::HashMap::with_capacity(count);
        for i in 0..count {
            let off = 4 + i * 12;
            let key = u64::from_le_bytes([
                pabgh_bytes[off], pabgh_bytes[off + 1], pabgh_bytes[off + 2], pabgh_bytes[off + 3],
                pabgh_bytes[off + 4], pabgh_bytes[off + 5], pabgh_bytes[off + 6], pabgh_bytes[off + 7],
            ]);
            let offset = u32::from_le_bytes([
                pabgh_bytes[off + 8],
                pabgh_bytes[off + 9],
                pabgh_bytes[off + 10],
                pabgh_bytes[off + 11],
            ]);
            pabgh_by_key.insert(key, offset);
        }
        eprintln!("pabgh entries: {count}");

        // ── Decode the save + collect appearance keys ───────────────
        let raw = std::fs::read(&save_path).expect("read save");
        let save = Save::parse(&raw).expect("parse save");
        let body = Body::parse(&save.body).expect("parse body");
        let blocks = body.decode_blocks(&save.body);

        let mut npc_blocks: Vec<&crate::save::ObjectBlock> = Vec::new();
        for b in &blocks {
            if b.class_name == "FieldNPCSaveData" {
                npc_blocks.push(b);
            }
            walk(b, &mut npc_blocks);
        }
        fn walk<'a>(b: &'a crate::save::ObjectBlock, out: &mut Vec<&'a crate::save::ObjectBlock>) {
            for f in &b.fields {
                if let FieldValue::ObjectList { elements, .. } = &f.value {
                    for e in elements {
                        if e.class_name == "FieldNPCSaveData" {
                            out.push(e);
                        }
                        walk(e, out);
                    }
                } else if let FieldValue::Locator { child: Some(c), .. } = &f.value {
                    walk(c, out);
                }
            }
        }

        let appearance_field_names = ["_nudeAppearanceIndexKey", "_customizationAppearanceIndexKey"];
        let mut all_values: Vec<u64> = Vec::new();
        for b in &npc_blocks {
            for f in &b.fields {
                if !f.present || !appearance_field_names.contains(&f.name.as_str()) {
                    continue;
                }
                if let FieldValue::Scalar(ScalarValue::U64(x)) = &f.value {
                    all_values.push(*x);
                }
            }
        }
        let distinct: std::collections::HashSet<u64> = all_values.iter().copied().collect();
        eprintln!(
            "appearance keys: {} samples, {} distinct, {} npc blocks",
            all_values.len(),
            distinct.len(),
            npc_blocks.len()
        );

        // ── Apply the canonical transform + measure hit rate ────────
        let mut total_hits = 0u32;
        let mut per_cat: std::collections::BTreeMap<i8, (u32, u32)> = Default::default();
        for v in &distinct {
            let b3 = ((v >> 24) & 0xFF) as i8;
            let lo24 = *v & 0x00FF_FFFF;
            let pabgh_key = (u64::from((b3 as i32) as u32) << 32) | lo24;
            let entry = per_cat.entry(b3).or_insert((0, 0));
            entry.0 += 1;
            if pabgh_by_key.contains_key(&pabgh_key) {
                entry.1 += 1;
                total_hits += 1;
            }
        }
        eprintln!(
            "save→pabgh transform: {}/{} distinct values hit",
            total_hits,
            distinct.len()
        );
        eprintln!("per-category (byte 3 / hits / total):");
        for (b3, (tot, hit)) in &per_cat {
            eprintln!("  0x{:02x}: {hit}/{tot}", *b3 as u8);
        }

        // Sanity: the transform's output for known-named-character
        // appearance IDs (e.g. lo24=1, 2, 4, 6 in the fe bucket)
        // should ALWAYS hit. Pin a couple to catch regressions.
        let pinned: &[(u64, bool)] = &[
            // (save_value, should_hit)
            (0xffff_ffff_fe00_0001, true),  // canonical fe-cat ID=1
            (0xffff_ffff_fe00_0002, true),  // canonical fe-cat ID=2
            (0x0000_0000_fe00_0001, true),  // variant byte stripped, still hits
        ];
        for (v, want) in pinned {
            let b3 = ((v >> 24) & 0xFF) as i8;
            let lo24 = *v & 0x00FF_FFFF;
            let pabgh_key = (u64::from((b3 as i32) as u32) << 32) | lo24;
            let got = pabgh_by_key.contains_key(&pabgh_key);
            assert_eq!(
                got, *want,
                "pinned transform check failed for save=0x{v:016x} → pabgh=0x{pabgh_key:016x}"
            );
        }
    }

    /// Investigation probe: list every `*appearance*` file in 0008's
    /// PAMT so we can see what tables the engine ships under that
    /// naming. Run once when starting RE on a new key type.
    #[test]
    #[ignore = "investigation only — appearance file discovery"]
    fn _scan_0008_appearance_files() {
        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let pamt_path = game_root.join("0008").join("0.pamt");
        if !pamt_path.is_file() {
            eprintln!("skipping: no {}", pamt_path.display());
            return;
        }
        let pamt_bytes = std::fs::read(&pamt_path).expect("read 0.pamt");
        let pamt = crate::binary::pamt::PackMeta::parse(&pamt_bytes, None).expect("parse PAMT");
        for d in &pamt.directories {
            for f in &d.files {
                let lower = f.name.to_ascii_lowercase();
                if lower.contains("appearance") || lower.contains("appearence") {
                    eprintln!(
                        "{}/{}  ({}c / {}u)",
                        d.path, f.name, f.file.compressed_size, f.file.uncompressed_size
                    );
                }
            }
        }
    }

    /// Cross-version investigation probe: loads a live Crimson Desert
    /// save, walks every `FieldNPCSaveData` and `FieldGimmickSaveData`
    /// block, dumps the actual field shapes. The 2026-05-15 run against
    /// a 1.07 `slot0/save.save` empirically confirmed:
    ///
    /// - `FieldNPCSaveData._characterKey` IS a flat u32 of shape
    ///   `0xCC_LLLLLL` (cat-byte hi + 24-bit lo) exactly as §6 of
    ///   `docs/save-editor-keys-plan.md` documented. 228 instances,
    ///   222 distinct values, 90+ distinct cat-byte values (0x06–0xfe).
    ///   The shipped `crimson_characterinfo_*` bridge is correct.
    /// - `FieldGimmickSaveData._gimmickInfoKey` IS a flat u32 with no
    ///   cat-byte, matching the shipped `crimson_gimmickinfo_*` bridge.
    ///   4264 instances across 549 distinct keys; slot-key range
    ///   `[131, 957806]` (slightly wider than §6's `[1788, 953478]`).
    ///
    /// What the probe ALSO revealed (and §6 didn't capture in detail):
    /// `FieldNPCSaveData` ships with 12 fields, not 4 — `_friendly` is
    /// a `Locator<ExperienceLevelSaveData>` (not a bool), plus
    /// `_nudeAppearanceIndexKey` / `_customizationAppearanceIndexKey`
    /// (both `CharacterAppearanceIndexKey` u64 — an unbridged key
    /// type), `_armorDyeAppearanceIndexKey` (u8), `_touchID` (u64),
    /// and a `_memoryOfTargetList` sublist. `FieldGimmickSaveData`
    /// ships 43 fields including `_saveRootFieldGimmickSaveDataKey`
    /// (parent-gimmick reference), `_ownerLevelName`, `_stageKey`,
    /// `_originSpawnTransform`, and many sub-lists.
    ///
    /// Run with:
    /// ```text
    /// cargo test --lib --features c_abi \
    ///   _probe_live_save_field_blocks -- --ignored --nocapture
    /// ```
    ///
    /// Picks up the save from `%LOCALAPPDATA%\Pearl Abyss\CD\save\
    /// <account>\slot0\save.save` by default — override via
    /// `CRIMSON_LIVE_SAVE`. `#[ignore]` so it never runs in the
    /// default suite (this is a diagnostic, not a regression gate —
    /// the shipped tests cover the bridge contract).
    #[test]
    #[ignore = "investigation only — uses the user's live save file"]
    fn _probe_live_save_field_blocks() {
        use crate::save::{Body, FieldValue, ScalarValue, Save};

        let save_path = std::env::var_os("CRIMSON_LIVE_SAVE")
            .map(PathBuf::from)
            .or_else(|| {
                // Default: walk `%LOCALAPPDATA%\Pearl Abyss\CD\save\` and
                // pick the first `<account>/slot0/save.save` we find.
                // The account directory is per-Steam-account so we don't
                // hardcode it.
                let appdata = std::env::var_os("LOCALAPPDATA")?;
                let root = PathBuf::from(appdata)
                    .join("Pearl Abyss")
                    .join("CD")
                    .join("save");
                let entries = std::fs::read_dir(&root).ok()?;
                for entry in entries.flatten() {
                    let candidate = entry.path().join("slot0").join("save.save");
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
                None
            });
        let Some(save_path) = save_path else {
            eprintln!(
                "skipping _probe_live_save_field_blocks: no Crimson Desert save file found \
                (set CRIMSON_LIVE_SAVE or play the game once to create %LOCALAPPDATA%\\Pearl Abyss\\CD\\save\\…\\slot0\\save.save)"
            );
            return;
        };
        if !save_path.is_file() {
            eprintln!("skipping _probe_live_save_field_blocks: no {}", save_path.display());
            return;
        }
        eprintln!("probing live save: {}", save_path.display());
        let raw = std::fs::read(&save_path).expect("read save");
        let save = Save::parse(&raw).expect("parse save");
        let body = Body::parse(&save.body).expect("parse body");
        let blocks = body.decode_blocks(&save.body);
        eprintln!("decoded {} top-level blocks", blocks.len());

        // ── Locate the class indices for the two save-data classes ──
        let mut npc_blocks: Vec<&_> = Vec::new();
        let mut gimmick_blocks: Vec<&_> = Vec::new();
        for b in &blocks {
            match b.class_name.as_str() {
                "FieldNPCSaveData" => npc_blocks.push(b),
                "FieldGimmickSaveData" => gimmick_blocks.push(b),
                _ => {}
            }
        }
        // Also scan nested ObjectList elements — these classes might
        // appear there too, depending on schema.
        let mut nested_npc: Vec<&_> = Vec::new();
        let mut nested_gimmick: Vec<&_> = Vec::new();
        for b in &blocks {
            walk_object_list(b, &mut nested_npc, &mut nested_gimmick);
        }
        eprintln!(
            "top-level: {} FieldNPCSaveData, {} FieldGimmickSaveData",
            npc_blocks.len(),
            gimmick_blocks.len()
        );
        eprintln!(
            "nested:    {} FieldNPCSaveData, {} FieldGimmickSaveData",
            nested_npc.len(),
            nested_gimmick.len()
        );

        // Choose whichever location has data.
        let npc_set: Vec<&crate::save::ObjectBlock> = if !npc_blocks.is_empty() {
            npc_blocks
        } else {
            nested_npc
        };
        let gimmick_set: Vec<&crate::save::ObjectBlock> = if !gimmick_blocks.is_empty() {
            gimmick_blocks
        } else {
            nested_gimmick
        };

        // ── FieldNPCSaveData dump ──────────────────────────────────
        eprintln!("\n=== FieldNPCSaveData ({} instances) ===", npc_set.len());
        if let Some(sample) = npc_set.first() {
            eprintln!("Field layout (from first block):");
            for f in &sample.fields {
                eprintln!(
                    "  [{:2}] present={} kind={:?} type={} name={} meta_size={}",
                    f.field_index, f.present, f.kind, f.type_name, f.name, f.meta_size
                );
            }
        }
        // Histogram of _characterKey values + cat-byte distribution.
        let mut cat_byte_hist: std::collections::BTreeMap<u8, u32> = Default::default();
        let mut all_npc_charkeys: Vec<u32> = Vec::new();
        let mut sibling_dumps = 0usize;
        for b in &npc_set {
            for f in &b.fields {
                if f.name != "_characterKey" {
                    continue;
                }
                let Some(val) = scalar_u32(&f.value) else {
                    eprintln!(
                        "  WARN: _characterKey not Scalar(U32) — actual: {:?}",
                        f.value
                    );
                    continue;
                };
                all_npc_charkeys.push(val);
                *cat_byte_hist.entry((val >> 24) as u8).or_insert(0) += 1;
                if sibling_dumps < 6 {
                    eprintln!(
                        "\n  sample NPC block #{sibling_dumps}: _characterKey = 0x{val:08x} (cat=0x{:02x}, lo24=0x{:06x})",
                        val >> 24,
                        val & 0xFF_FFFF
                    );
                    for sf in &b.fields {
                        if sf.present {
                            eprintln!(
                                "    {} = {}",
                                sf.name,
                                short_field_value(&sf.value)
                            );
                        }
                    }
                    sibling_dumps += 1;
                }
            }
        }
        eprintln!("\nFieldNPC summary:");
        eprintln!("  total _characterKey samples: {}", all_npc_charkeys.len());
        eprintln!("  distinct values:             {}",
            all_npc_charkeys.iter().collect::<std::collections::HashSet<_>>().len());
        eprintln!("  cat-byte distribution:");
        for (cat, n) in &cat_byte_hist {
            eprintln!("    0x{cat:02x}: {n}");
        }

        // ── FieldGimmickSaveData dump ──────────────────────────────
        eprintln!("\n=== FieldGimmickSaveData ({} instances) ===", gimmick_set.len());
        if let Some(sample) = gimmick_set.first() {
            eprintln!("Field layout (from first block):");
            for f in &sample.fields {
                eprintln!(
                    "  [{:2}] present={} kind={:?} type={} name={} meta_size={}",
                    f.field_index, f.present, f.kind, f.type_name, f.name, f.meta_size
                );
            }
        }
        let mut all_gimmickinfo_keys: Vec<u32> = Vec::new();
        let mut all_gimmick_slot_keys: Vec<u32> = Vec::new();
        let mut gimmick_dumps = 0usize;
        for b in &gimmick_set {
            let mut gimmick_info_key: Option<u32> = None;
            let mut slot_key: Option<u32> = None;
            for f in &b.fields {
                if let Some(v) = scalar_u32(&f.value) {
                    if f.name == "_gimmickInfoKey" {
                        gimmick_info_key = Some(v);
                    }
                    if f.name == "_fieldGimmickSaveDataKey" {
                        slot_key = Some(v);
                    }
                }
            }
            if let Some(v) = gimmick_info_key {
                all_gimmickinfo_keys.push(v);
            }
            if let Some(v) = slot_key {
                all_gimmick_slot_keys.push(v);
            }
            if gimmick_dumps < 3 {
                eprintln!(
                    "\n  sample Gimmick block #{gimmick_dumps}: _gimmickInfoKey = {:?}, _fieldGimmickSaveDataKey = {:?}",
                    gimmick_info_key, slot_key
                );
                gimmick_dumps += 1;
            }
        }
        eprintln!("\nFieldGimmick summary:");
        eprintln!("  total _gimmickInfoKey samples:           {}", all_gimmickinfo_keys.len());
        eprintln!("  distinct _gimmickInfoKey values:         {}",
            all_gimmickinfo_keys.iter().collect::<std::collections::HashSet<_>>().len());
        eprintln!("  total _fieldGimmickSaveDataKey samples:  {}", all_gimmick_slot_keys.len());
        eprintln!("  distinct _fieldGimmickSaveDataKey:       {}",
            all_gimmick_slot_keys.iter().collect::<std::collections::HashSet<_>>().len());
        if let (Some(min), Some(max)) = (
            all_gimmick_slot_keys.iter().min(),
            all_gimmick_slot_keys.iter().max(),
        ) {
            eprintln!("  slot-key range: [{min}, {max}]");
        }

        fn walk_object_list<'a>(
            b: &'a crate::save::ObjectBlock,
            npc: &mut Vec<&'a crate::save::ObjectBlock>,
            gimmick: &mut Vec<&'a crate::save::ObjectBlock>,
        ) {
            for f in &b.fields {
                if let FieldValue::ObjectList { elements, .. } = &f.value {
                    for e in elements {
                        match e.class_name.as_str() {
                            "FieldNPCSaveData" => npc.push(e),
                            "FieldGimmickSaveData" => gimmick.push(e),
                            _ => {}
                        }
                        walk_object_list(e, npc, gimmick);
                    }
                } else if let FieldValue::Locator { child: Some(c), .. } = &f.value {
                    walk_object_list(c, npc, gimmick);
                }
            }
        }

        fn scalar_u32(v: &FieldValue) -> Option<u32> {
            match v {
                FieldValue::Scalar(ScalarValue::U32(x)) => Some(*x),
                _ => None,
            }
        }

        fn short_field_value(v: &FieldValue) -> String {
            match v {
                FieldValue::Scalar(s) => format!("{s:?}"),
                FieldValue::None => "<absent>".into(),
                FieldValue::InlineBytes { count, .. } => format!("InlineBytes[{count}]"),
                FieldValue::DynamicArray { count, .. } => format!("DynamicArray[{count}]"),
                FieldValue::Locator { child_type_name, child, .. } => {
                    format!("Locator<{child_type_name}>{}", if child.is_some() { " resolved" } else { "" })
                }
                FieldValue::ObjectList { count, .. } => format!("ObjectList[{count}]"),
            }
        }
    }

    /// Schema probe for `InventorySaveData`. Dumps the first
    /// `InventorySaveData` block's field-by-field tree — top level,
    /// first inventory container, first item inside that container —
    /// so we can pin the exact field names + types before shipping
    /// the `crimson_save_list_inventory_items` C ABI.
    #[test]
    #[ignore = "investigation only — InventorySaveData schema discovery"]
    fn _probe_inventory_save_data_schema() {
        use crate::save::{Body, FieldValue, ScalarValue, Save};

        let save_path = std::env::var_os("CRIMSON_LIVE_SAVE")
            .map(PathBuf::from)
            .or_else(|| {
                let appdata = std::env::var_os("LOCALAPPDATA")?;
                let root = PathBuf::from(appdata).join("Pearl Abyss").join("CD").join("save");
                std::fs::read_dir(&root).ok()?.flatten().find_map(|entry| {
                    let p = entry.path().join("slot0").join("save.save");
                    p.is_file().then_some(p)
                })
            });
        let Some(save_path) = save_path else {
            eprintln!("skipping: no live save");
            return;
        };
        let raw = std::fs::read(&save_path).expect("read save");
        let save = Save::parse(&raw).expect("parse save");
        let body = Body::parse(&save.body).expect("parse body");
        let blocks = body.decode_blocks(&save.body);

        let inv_block = blocks.iter().find(|b| b.class_name == "InventorySaveData");
        let Some(inv_block) = inv_block else {
            eprintln!("no InventorySaveData block in this save");
            return;
        };
        eprintln!(
            "InventorySaveData at block_idx (class={}): {} fields",
            inv_block.class_name,
            inv_block.fields.len()
        );
        for f in &inv_block.fields {
            eprintln!(
                "  [{:2}] present={} kind={:?} type={} name={} meta_size={}",
                f.field_index, f.present, f.kind, f.type_name, f.name, f.meta_size
            );
        }

        // Find _inventoryList field (whatever the exact spelling is).
        let inv_list_field = inv_block.fields.iter().find(|f| {
            f.name.eq_ignore_ascii_case("_inventoryList")
                || f.name.eq_ignore_ascii_case("_inventorylist")
        });
        let Some(inv_list_field) = inv_list_field else {
            eprintln!("no _inventoryList field found — check name");
            return;
        };
        let FieldValue::ObjectList { count, elements, .. } = &inv_list_field.value else {
            eprintln!("_inventoryList isn't ObjectList: {:?}", inv_list_field.value);
            return;
        };
        eprintln!(
            "\n_inventoryList (field {} \"{}\"): ObjectList with {} elements",
            inv_list_field.field_index, inv_list_field.name, count
        );

        // Dump the first inventory container's full schema.
        let Some(first) = elements.first() else {
            eprintln!("_inventoryList is empty");
            return;
        };
        eprintln!(
            "\ninventory[0] class={}: {} fields",
            first.class_name,
            first.fields.len()
        );
        for f in &first.fields {
            eprintln!(
                "  [{:2}] present={} kind={:?} type={} name={} meta_size={}",
                f.field_index, f.present, f.kind, f.type_name, f.name, f.meta_size
            );
        }

        // Find _itemList in the container.
        let item_list_field = first.fields.iter().find(|f| {
            f.name.eq_ignore_ascii_case("_itemList")
                || f.name.eq_ignore_ascii_case("_itemlist")
        });
        if let Some(item_list_field) = item_list_field
            && let FieldValue::ObjectList { count, elements, .. } = &item_list_field.value
        {
            {
                eprintln!(
                    "\n_itemList (field {} \"{}\"): {} items in container[0]",
                    item_list_field.field_index, item_list_field.name, count
                );
                if let Some(item) = elements.first() {
                    eprintln!(
                        "\nitem[0] class={}: {} fields",
                        item.class_name,
                        item.fields.len()
                    );
                    for f in &item.fields {
                        let val_str = match &f.value {
                            FieldValue::Scalar(ScalarValue::U32(v)) => format!("U32({v})"),
                            FieldValue::Scalar(ScalarValue::U64(v)) => format!("U64({v})"),
                            FieldValue::Scalar(ScalarValue::Bool(v)) => format!("Bool({v})"),
                            _ => format!("{:?}", f.value),
                        };
                        eprintln!(
                            "  [{:2}] present={} kind={:?} type={} name={} meta_size={} value={}",
                            f.field_index, f.present, f.kind, f.type_name, f.name, f.meta_size, val_str,
                        );
                    }
                }
            }
        }

        // Quick totals: items per container + grand total.
        eprintln!("\n— per-inventory item counts —");
        let mut grand_total = 0u32;
        for (i, container) in elements.iter().enumerate() {
            let item_count = container
                .fields
                .iter()
                .find(|f| {
                    f.name.eq_ignore_ascii_case("_itemList")
                        || f.name.eq_ignore_ascii_case("_itemlist")
                })
                .and_then(|f| {
                    if let FieldValue::ObjectList { count, .. } = f.value {
                        Some(count)
                    } else {
                        None
                    }
                })
                .unwrap_or(0);
            grand_total += item_count;
            eprintln!("  inventory[{i:2}] ({:<28}) → {item_count} items", container.class_name);
        }
        eprintln!("  grand total: {grand_total}");
    }

    /// Schema probe for `ItemSaveData._itemDyeDataList`. Walks every
    /// `InventorySaveData` block in the live save, finds items whose
    /// `_itemDyeDataList` is present (non-absent), and dumps the
    /// first dye element's field tree. The schema baseline + sample
    /// values are reference data for CrimsonAtomtic's Dye editor.
    ///
    /// What this confirms (verified 2026-05-15 against the 1.07
    /// `slot0/save.save`):
    ///
    /// - `_itemDyeDataList` is field 14 of `ItemSaveData`, type
    ///   `ReflectObject`, `meta_size=0`. When present, it decodes as
    ///   `FieldValue::ObjectList<ItemDyeSaveData>`.
    /// - Per-element schema (from PyQt5 reference editor's RE work in
    ///   `D:\Github\CRIMSON-DESERT-SAVE-EDITOR-AND-GAME-MODS\
    ///   CrimsonSaveEditor\parc_inserter3.py:1740-1785`):
    ///
    ///   | Mask | Field                     | Type    |
    ///   |------|---------------------------|---------|
    ///   | 0x01 | `_dyeSlotNo`              | u8      |
    ///   | 0x02 | `_dyeColorR`              | u8      |
    ///   | 0x04 | `_dyeColorG`              | u8      |
    ///   | 0x08 | `_dyeColorB`              | u8      |
    ///   | 0x10 | `_dyeColorA`              | u8      |
    ///   | 0x20 | `_grimeOpacity`           | i8      |
    ///   | 0x40 | `_dyeColorGroupInfoKey`   | u32 LE  |
    ///   | 0x80 | `_texturePalleteKey`      | u16/u32 |
    ///
    /// Also scans `0008/0.pamt` for `*dye*` files — the next-session
    /// task is to parse the gamedata-side dye table (`dyeinfo.pabgb`
    /// or similar) so per-item slot counts come from gamedata
    /// directly rather than the PyQt5 reference editor's
    /// hand-maintained `dye_slot_counts.json`. The CrimsonAtomtic
    /// Dye editor v1 can still ship with the JSON approach while
    /// this gamedata RE is in flight.
    #[test]
    #[ignore = "investigation only — ItemDyeSaveData schema + dyeinfo file discovery"]
    fn _probe_item_dye_data() {
        use crate::binary::pamt::PackMeta;
        use crate::save::{Body, FieldValue, ScalarValue, Save};

        // ── Locate live save ───────────────────────────────────────
        let save_path = std::env::var_os("CRIMSON_LIVE_SAVE")
            .map(PathBuf::from)
            .or_else(|| {
                let appdata = std::env::var_os("LOCALAPPDATA")?;
                let root = PathBuf::from(appdata)
                    .join("Pearl Abyss")
                    .join("CD")
                    .join("save");
                std::fs::read_dir(&root).ok()?.flatten().find_map(|entry| {
                    let p = entry.path().join("slot0").join("save.save");
                    p.is_file().then_some(p)
                })
            });
        let Some(save_path) = save_path else {
            eprintln!("skipping: no live save");
            return;
        };
        let raw = std::fs::read(&save_path).expect("read save");
        let save = Save::parse(&raw).expect("parse save");
        let body = Body::parse(&save.body).expect("parse body");
        let blocks = body.decode_blocks(&save.body);

        // ── Find InventorySaveData → walk every ItemSaveData ────────
        let mut total_items = 0usize;
        let mut items_with_dye = 0usize;
        let mut dye_entries_total = 0usize;
        let mut first_dump_done = false;
        let mut sample_paths: Vec<(u32, u32, u32, String, u32)> = Vec::new();

        for (block_idx, block) in blocks.iter().enumerate() {
            if block.class_name != "InventorySaveData" {
                continue;
            }
            for inv_list_field in &block.fields {
                if !inv_list_field.name.eq_ignore_ascii_case("_inventorylist") {
                    continue;
                }
                let FieldValue::ObjectList { elements: containers, .. } = &inv_list_field.value
                else {
                    continue;
                };
                for (inv_idx, container) in containers.iter().enumerate() {
                    let inv_key: u16 = container
                        .fields
                        .iter()
                        .find(|f| f.name.eq_ignore_ascii_case("_inventoryKey"))
                        .and_then(|f| match &f.value {
                            FieldValue::Scalar(ScalarValue::U16(v)) => Some(*v),
                            _ => None,
                        })
                        .unwrap_or(0);

                    for f in &container.fields {
                        if !f.name.eq_ignore_ascii_case("_itemList") {
                            continue;
                        }
                        let FieldValue::ObjectList { elements: items, .. } = &f.value else {
                            continue;
                        };
                        for (item_idx, item) in items.iter().enumerate() {
                            total_items += 1;
                            // Pull the item key for the dump.
                            let item_key = item
                                .fields
                                .iter()
                                .find(|f| f.name == "_itemKey")
                                .and_then(|f| match &f.value {
                                    FieldValue::Scalar(ScalarValue::U32(v)) => Some(*v),
                                    _ => None,
                                })
                                .unwrap_or(0);
                            // Look at _itemDyeDataList — field 14.
                            let Some(dye_field) = item
                                .fields
                                .iter()
                                .find(|f| f.name == "_itemDyeDataList")
                            else {
                                continue;
                            };
                            if !dye_field.present {
                                continue;
                            }
                            let FieldValue::ObjectList { count, elements: dye_elems, .. } =
                                &dye_field.value
                            else {
                                eprintln!(
                                    "  WARN: item key={item_key} _itemDyeDataList present but \
                                     not ObjectList — got {:?}",
                                    dye_field.value
                                );
                                continue;
                            };
                            items_with_dye += 1;
                            dye_entries_total += *count as usize;

                            // Record the path for the C# editor.
                            sample_paths.push((
                                block_idx as u32,
                                inv_idx as u32,
                                item_idx as u32,
                                format!(
                                    "InventorySaveData[{}]._inventorylist[{}].fields[2 _itemList][{}]._itemDyeDataList",
                                    block_idx, inv_idx, item_idx,
                                ),
                                inv_key as u32,
                            ));

                            // Dump the FIRST dye element's full schema once
                            // — that's the reference the C# editor uses.
                            if !first_dump_done {
                                eprintln!(
                                    "\n=== sample dyed item ===\nblock_idx={block_idx} inv_idx={inv_idx} \
                                     inv_key=0x{inv_key:04x} item_idx={item_idx} item_key={item_key}"
                                );
                                eprintln!(
                                    "_itemDyeDataList field present, ObjectList count={count}"
                                );
                                if let Some(dye_elem) = dye_elems.first() {
                                    eprintln!(
                                        "\nfirst dye element class={}, {} fields, mask_bytes={:02x?}:",
                                        dye_elem.class_name, dye_elem.fields.len(), dye_elem.mask_bytes,
                                    );
                                    for df in &dye_elem.fields {
                                        let val_str = match &df.value {
                                            FieldValue::Scalar(ScalarValue::U8(v)) => format!("U8({v})"),
                                            FieldValue::Scalar(ScalarValue::I8(v)) => format!("I8({v})"),
                                            FieldValue::Scalar(ScalarValue::U16(v)) => format!("U16({v})"),
                                            FieldValue::Scalar(ScalarValue::U32(v)) => format!("U32(0x{v:08x})"),
                                            FieldValue::None => "<absent>".into(),
                                            _ => format!("{:?}", df.value),
                                        };
                                        eprintln!(
                                            "  [{:2}] present={} kind={:?} type={} name={} meta_size={} value={}",
                                            df.field_index, df.present, df.kind, df.type_name,
                                            df.name, df.meta_size, val_str,
                                        );
                                    }
                                }
                                first_dump_done = true;
                            }
                        }
                    }
                }
            }
        }

        eprintln!("\n— inventory summary —");
        eprintln!("  total items walked:        {total_items}");
        eprintln!("  items with dye data:       {items_with_dye}");
        eprintln!("  total dye entries:         {dye_entries_total}");
        eprintln!("\nfirst 10 dyed items (block, inv, item, path, inv_key):");
        for (b, i, k, p, ik) in sample_paths.iter().take(10) {
            eprintln!("  block={b} inv={i} item={k} inv_key=0x{ik:04x}");
            eprintln!("    path = {p}");
        }
        if sample_paths.len() > 10 {
            eprintln!("  (+{} more)", sample_paths.len() - 10);
        }

        // ── Scan 0008 for *dye* files (next-session gamedata work) ──
        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let pamt_path = game_root.join("0008").join("0.pamt");
        if let Ok(pamt_bytes) = std::fs::read(&pamt_path) {
            let pamt = PackMeta::parse(&pamt_bytes, None).expect("parse 0008 PAMT");
            eprintln!("\n— 0008 *dye* / *color* / *palette* files —");
            let mut hit_count = 0;
            for d in &pamt.directories {
                for f in &d.files {
                    let lower = f.name.to_ascii_lowercase();
                    if lower.contains("dye")
                        || lower.contains("palette")
                        || lower.contains("pallete")
                        || lower.contains("texturepalle")
                        || lower.contains("colorgroup")
                    {
                        eprintln!(
                            "  {}/{} ({}c / {}u)",
                            d.path, f.name, f.file.compressed_size, f.file.uncompressed_size
                        );
                        hit_count += 1;
                    }
                }
            }
            if hit_count == 0 {
                eprintln!("  (no dye-related files found in 0008 — try other groups)");
            }
        }
    }

    /// Save-tree skeleton probe — dumps the top-level TOC class
    /// histogram plus, for every block, the names of `ObjectList` /
    /// `Locator` fields it carries (i.e. the recursive entry points).
    /// Used to spot containers the dye probe might miss — e.g. mount
    /// equipment, inactive-character equipment, etc.
    #[test]
    #[ignore = "investigation only — TOC skeleton + container fields dump"]
    fn _probe_save_skeleton_slot103() {
        use crate::save::{Body, FieldValue, Save};

        let save_path = std::env::var_os("CRIMSON_DYE_PROBE_SAVE")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("CRIMSON_LIVE_SAVE").map(PathBuf::from))
            .or_else(|| {
                let appdata = std::env::var_os("LOCALAPPDATA")?;
                let root = PathBuf::from(appdata)
                    .join("Pearl Abyss")
                    .join("CD")
                    .join("save");
                std::fs::read_dir(&root).ok()?.flatten().find_map(|entry| {
                    let p = entry.path().join("slot103").join("save.save");
                    p.is_file().then_some(p)
                })
            });
        let Some(save_path) = save_path else {
            eprintln!("skipping: no slot103/save.save");
            return;
        };
        eprintln!("probing {}", save_path.display());

        let raw = std::fs::read(&save_path).expect("read save");
        let save = Save::parse(&raw).expect("parse save");
        let body = Body::parse(&save.body).expect("parse body");
        let blocks = body.decode_blocks(&save.body);

        eprintln!("\n=== TOC block class histogram ===");
        let mut hist: std::collections::BTreeMap<String, u32> = Default::default();
        for b in &blocks {
            *hist.entry(b.class_name.clone()).or_insert(0) += 1;
        }
        for (cls, c) in &hist {
            eprintln!("  {cls}: {c}");
        }

        eprintln!("\n=== blocks with names matching mount/horse/pet/vehicle/character/owner ===");
        let needles = ["mount", "horse", "pet", "vehicle", "companion", "stable",
                       "character", "owner", "kliff", "damine", "oongka", "playable"];
        for (i, b) in blocks.iter().enumerate() {
            let low = b.class_name.to_ascii_lowercase();
            if needles.iter().any(|n| low.contains(n)) {
                eprintln!("  toc[{i}] {} ({} fields)", b.class_name, b.fields.len());
                for f in &b.fields {
                    let kind = match &f.value {
                        FieldValue::ObjectList { count, .. } => format!("ObjectList<count={count}>"),
                        FieldValue::Locator { .. } => "Locator".into(),
                        _ => continue,
                    };
                    eprintln!("    .{} : {} (present={})", f.name, kind, f.present);
                }
            }
        }

        eprintln!("\n=== every block class that hosts ItemSaveData (recursively) ===");
        let mut hosts: std::collections::BTreeMap<String, u32> = Default::default();

        fn walk(
            b: &crate::save::ObjectBlock,
            parent: &str,
            hosts: &mut std::collections::BTreeMap<String, u32>,
        ) {
            if b.class_name == "ItemSaveData" {
                *hosts.entry(parent.to_string()).or_insert(0) += 1;
            }
            for f in &b.fields {
                match &f.value {
                    FieldValue::ObjectList { elements, .. } => {
                        let next_parent = format!("{}.{}", b.class_name, f.name);
                        for e in elements {
                            walk(e, &next_parent, hosts);
                        }
                    }
                    FieldValue::Locator { child: Some(c), .. } => {
                        let next_parent = format!("{}.{}<locator>", b.class_name, f.name);
                        walk(c, &next_parent, hosts);
                    }
                    _ => {}
                }
            }
        }
        for b in &blocks {
            walk(b, "<root>", &mut hosts);
        }
        for (path, c) in &hosts {
            eprintln!("  {path} → {c} ItemSaveData instances");
        }
    }

    /// Dye data probe — walks **both** `InventorySaveData` AND
    /// `EquipmentSaveData` (and any other container that recurses into
    /// `ItemSaveData`) for every present `_itemDyeDataList`. Dumps the
    /// full per-element scalar set (RGBA / slotNo / grime / colorGroup
    /// / texturePallete / disableSymbol) for each hit — i.e. every dye
    /// the player has applied across the whole save.
    ///
    /// Defaults to `slot103/save.save` because that's where the
    /// 2026-05-17 in-game dye-application session lives (user dyed
    /// multiple items via the NPC dye UI — Hernand / Pailune themes,
    /// the bright / dark / normal tiers across the red / yellow color
    /// families). Override with `CRIMSON_DYE_PROBE_SAVE` for other
    /// slots.
    ///
    /// Output table per hit:
    ///   `(parent_class, item_key, dye_idx, R, G, B, A, slotNo,
    ///    grime, colorGroup, texturePallete, disableSymbol, path)`
    ///
    /// Counterpart to [`_probe_item_dye_data`] (slot0, single sample,
    /// schema-validation only). This one is the **data-collection**
    /// probe: it doesn't enforce a schema, it just enumerates.
    #[test]
    #[ignore = "investigation only — full dye enumeration anywhere in tree (default slot103)"]
    fn _probe_item_dye_data_anywhere_slot103() {
        use crate::save::{Body, FieldValue, ScalarValue, Save};

        let save_path = std::env::var_os("CRIMSON_DYE_PROBE_SAVE")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("CRIMSON_LIVE_SAVE").map(PathBuf::from))
            .or_else(|| {
                let appdata = std::env::var_os("LOCALAPPDATA")?;
                let root = PathBuf::from(appdata)
                    .join("Pearl Abyss")
                    .join("CD")
                    .join("save");
                std::fs::read_dir(&root).ok()?.flatten().find_map(|entry| {
                    let p = entry.path().join("slot103").join("save.save");
                    p.is_file().then_some(p)
                })
            });
        let Some(save_path) = save_path else {
            eprintln!("skipping: no slot103/save.save");
            return;
        };
        eprintln!("probing {}", save_path.display());

        let raw = std::fs::read(&save_path).expect("read save");
        let save = Save::parse(&raw).expect("parse save");
        let body = Body::parse(&save.body).expect("parse body");
        let blocks = body.decode_blocks(&save.body);

        struct Hit<'a> {
            path_label: String,
            parent_class: String,
            parent_item_key: Option<u32>,
            parent_item_no: Option<u64>,
            dye_field: &'a crate::save::DecodedField,
        }
        let mut hits: Vec<Hit> = Vec::new();
        let mut class_hist: std::collections::BTreeMap<String, u32> = Default::default();

        fn walk<'a>(
            block: &'a crate::save::ObjectBlock,
            path: &str,
            out: &mut Vec<Hit<'a>>,
            class_hist: &mut std::collections::BTreeMap<String, u32>,
        ) {
            let pull_u32 = |name: &str| {
                block.fields.iter()
                    .find(|f| f.name == name)
                    .and_then(|f| match &f.value {
                        FieldValue::Scalar(ScalarValue::U32(v)) => Some(*v),
                        _ => None,
                    })
            };
            let pull_u64 = |name: &str| {
                block.fields.iter()
                    .find(|f| f.name == name)
                    .and_then(|f| match &f.value {
                        FieldValue::Scalar(ScalarValue::U64(v)) => Some(*v),
                        _ => None,
                    })
            };
            for f in &block.fields {
                if f.name == "_itemDyeDataList" && f.present {
                    *class_hist.entry(block.class_name.clone()).or_insert(0) += 1;
                    out.push(Hit {
                        path_label: path.to_string(),
                        parent_class: block.class_name.clone(),
                        parent_item_key: pull_u32("_itemKey"),
                        parent_item_no: pull_u64("_itemNo"),
                        dye_field: f,
                    });
                }
            }
            for f in &block.fields {
                match &f.value {
                    FieldValue::ObjectList { elements, .. } => {
                        for (i, e) in elements.iter().enumerate() {
                            let sub = format!("{path}.{}[{i}]", f.name);
                            walk(e, &sub, out, class_hist);
                        }
                    }
                    FieldValue::Locator { child: Some(c), .. } => {
                        let sub = format!("{path}.{}<child>", f.name);
                        walk(c, &sub, out, class_hist);
                    }
                    _ => {}
                }
            }
        }

        for (i, block) in blocks.iter().enumerate() {
            let root_label = format!("toc[{i}]:{}", block.class_name);
            walk(block, &root_label, &mut hits, &mut class_hist);
        }

        eprintln!("\n=== _itemDyeDataList host classes ===");
        for (cls, c) in &class_hist {
            eprintln!("  {cls}: {} occurrences", c);
        }
        eprintln!("\ntotal _itemDyeDataList hits: {}", hits.len());

        // ── Per-hit dump ───────────────────────────────────────────
        eprintln!("\n=== dye enumeration ===");
        let mut total_dye_entries = 0usize;
        // Histogram a few interesting axes.
        let mut color_group_hist: std::collections::BTreeMap<u32, u32> = Default::default();
        let mut texture_palette_hist: std::collections::BTreeMap<u16, u32> = Default::default();
        let mut rgb_set: std::collections::BTreeSet<(u8, u8, u8)> = Default::default();

        for hit in &hits {
            let FieldValue::ObjectList { count, elements: dyes, .. } = &hit.dye_field.value
            else { continue };
            total_dye_entries += *count as usize;

            // Item-level header
            eprintln!(
                "\n  itemKey={:?} itemNo={:?} class={} dye_count={}",
                hit.parent_item_key, hit.parent_item_no, hit.parent_class, count,
            );
            eprintln!("    path={}", hit.path_label);

            for (didx, dye) in dyes.iter().enumerate() {
                let pull_u8 = |name: &str| {
                    dye.fields.iter()
                        .find(|f| f.name == name && f.present)
                        .and_then(|f| match &f.value {
                            FieldValue::Scalar(ScalarValue::U8(v)) => Some(*v),
                            _ => None,
                        })
                };
                let pull_i8 = |name: &str| {
                    dye.fields.iter()
                        .find(|f| f.name == name && f.present)
                        .and_then(|f| match &f.value {
                            FieldValue::Scalar(ScalarValue::I8(v)) => Some(*v),
                            _ => None,
                        })
                };
                let pull_u16 = |name: &str| {
                    dye.fields.iter()
                        .find(|f| f.name == name && f.present)
                        .and_then(|f| match &f.value {
                            FieldValue::Scalar(ScalarValue::U16(v)) => Some(*v),
                            _ => None,
                        })
                };
                let pull_u32 = |name: &str| {
                    dye.fields.iter()
                        .find(|f| f.name == name && f.present)
                        .and_then(|f| match &f.value {
                            FieldValue::Scalar(ScalarValue::U32(v)) => Some(*v),
                            _ => None,
                        })
                };
                let r = pull_u8("_dyeColorR");
                let g = pull_u8("_dyeColorG");
                let b = pull_u8("_dyeColorB");
                let a = pull_u8("_dyeColorA");
                let slot_no = pull_i8("_dyeSlotNo");
                let grime = pull_i8("_grimeOpacity");
                let cgk = pull_u32("_dyeColorGroupInfoKey");
                let palette = pull_u16("_texturePalleteKey");
                let disable_sym = pull_i8("_disableSymbol");

                if let (Some(r), Some(g), Some(b)) = (r, g, b) {
                    rgb_set.insert((r, g, b));
                }
                if let Some(k) = cgk { *color_group_hist.entry(k).or_insert(0) += 1; }
                if let Some(p) = palette { *texture_palette_hist.entry(p).or_insert(0) += 1; }

                let fmt_u8 = |v: Option<u8>| v.map(|x| format!("{x}")).unwrap_or_else(|| "—".into());
                let fmt_i8 = |v: Option<i8>| v.map(|x| format!("{x}")).unwrap_or_else(|| "—".into());
                let fmt_u16 = |v: Option<u16>| v.map(|x| format!("{x}")).unwrap_or_else(|| "—".into());
                let fmt_u32 = |v: Option<u32>| v.map(|x| format!("0x{x:08x}")).unwrap_or_else(|| "—".into());

                eprintln!(
                    "    dye[{didx}] mask={:02x?} slotNo={} R={} G={} B={} A={} \
                     grime={} colorGroup={} texturePallete={} disableSym={}",
                    dye.mask_bytes,
                    fmt_i8(slot_no),
                    fmt_u8(r), fmt_u8(g), fmt_u8(b), fmt_u8(a),
                    fmt_i8(grime),
                    fmt_u32(cgk),
                    fmt_u16(palette),
                    fmt_i8(disable_sym),
                );
            }
        }

        eprintln!("\n=== summary ===");
        eprintln!("  hits (items with present _itemDyeDataList): {}", hits.len());
        eprintln!("  total dye entries:                          {}", total_dye_entries);
        eprintln!("  unique (R,G,B) values:                      {}", rgb_set.len());

        eprintln!("\n=== _dyeColorGroupInfoKey histogram ===");
        if color_group_hist.is_empty() {
            eprintln!("  (none present)");
        } else {
            for (k, c) in &color_group_hist {
                eprintln!("  0x{:08x}: {} entries", k, c);
            }
        }

        eprintln!("\n=== _texturePalleteKey histogram ===");
        if texture_palette_hist.is_empty() {
            eprintln!("  (none present)");
        } else {
            for (k, c) in &texture_palette_hist {
                eprintln!("  {}: {} entries", k, c);
            }
        }

        eprintln!("\n=== unique (R,G,B) values seen ===");
        for (r, g, b) in &rgb_set {
            eprintln!("  ({:3}, {:3}, {:3})  #{:02x}{:02x}{:02x}", r, g, b, r, g, b);
        }
    }

    /// Full dye enumeration with mercenary-template resolution.
    ///
    /// Sibling of [`_probe_item_dye_data_anywhere_slot103`]. The
    /// earlier probe walks the same tree but doesn't distinguish
    /// mercenary classes (mounts vs human mercenaries) — both share
    /// `MercenarySaveData` and only the `_mercenaryKey` template ID
    /// tells them apart (e.g. `0 → "Mercenary_Main"`, `7 →
    /// "Vehicle_Horse"`). This probe resolves that key for every
    /// mercenary container in the save and tags its equipment list /
    /// inventory list / dye hits accordingly.
    ///
    /// Catches:
    /// - All `_itemDyeDataList` hits across **every** ItemSaveData host
    ///   the skeleton probe surfaced (EquipSlotElement, MercenarySaveData
    ///   equip+inventory, InventoryElement, UseItemReserveSlot, faction,
    ///   stores — though the latter two rarely carry user dyes).
    /// - Per-mercenary roll-up: items held, dyed items, mount-vs-human
    ///   classification.
    ///
    /// Defaults to `slot103/save.save`; override with
    /// `CRIMSON_DYE_PROBE_SAVE`. Requires the live game install for
    /// MercenaryKey name resolution; falls back to raw u32 keys if the
    /// gamedata pamt isn't available.
    #[test]
    #[ignore = "investigation only — dye enumeration with mount / mercenary template resolution"]
    fn _probe_item_dye_data_with_mercenary_resolution() {
        use crate::binary::pamt::PackMeta;
        use crate::binary::paz;
        use crate::character_info::parse_character_info_lossy;
        use crate::save::{Body, FieldValue, ScalarValue, Save};

        let save_path = std::env::var_os("CRIMSON_DYE_PROBE_SAVE")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("CRIMSON_LIVE_SAVE").map(PathBuf::from))
            .or_else(|| {
                let appdata = std::env::var_os("LOCALAPPDATA")?;
                let root = PathBuf::from(appdata)
                    .join("Pearl Abyss")
                    .join("CD")
                    .join("save");
                std::fs::read_dir(&root).ok()?.flatten().find_map(|entry| {
                    let p = entry.path().join("slot103").join("save.save");
                    p.is_file().then_some(p)
                })
            });
        let Some(save_path) = save_path else {
            eprintln!("skipping: no slot103/save.save");
            return;
        };
        eprintln!("probing {}", save_path.display());

        // ── Load CharacterKey → name resolver (optional) ───────────
        //
        // MercenarySaveData uses `_characterKey` (NOT `_mercenaryKey`)
        // as the template ID. The save value carries a cat-byte in its
        // hi-byte that must be stripped (& 0xFFFFFF) before the lookup
        // — see `character_info::CharacterInfoEntry::key` for the
        // rationale.
        let merc_name: std::collections::HashMap<u32, String> = {
            let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
                });
            let mut out = std::collections::HashMap::new();
            let pamt_path = game_root.join("0008").join("0.pamt");
            if let Ok(pamt_bytes) = std::fs::read(&pamt_path)
                && let Ok(pamt) = PackMeta::parse(&pamt_bytes, None)
            {
                let dir_path = "gamedata/binary__/client/bin";
                if let Some(dir) = pamt.directories.iter().find(|d| d.path == dir_path)
                    && let Some(pabgb) = dir.files.iter().find(|f| f.name == "characterinfo.pabgb")
                {
                    let group_dir = game_root.join("0008");
                    let enc = &pamt.header.encrypt_info.encrypt_info;
                    if let Ok(pabgb_bytes) = paz::extract_file(&group_dir, pabgb, dir_path, enc) {
                        for e in parse_character_info_lossy(&pabgb_bytes) {
                            out.entry(e.key).or_insert(e.name);
                        }
                        eprintln!("loaded {} CharacterKey → name rows", out.len());
                    }
                }
            }
            if out.is_empty() {
                eprintln!("(no character template resolver — raw u32 keys will be shown)");
            }
            out
        };

        let raw = std::fs::read(&save_path).expect("read save");
        let save = Save::parse(&raw).expect("parse save");
        let body = Body::parse(&save.body).expect("parse body");
        let blocks = body.decode_blocks(&save.body);

        // Helper: pull a scalar by name.
        fn pull_u32(b: &crate::save::ObjectBlock, n: &str) -> Option<u32> {
            b.fields.iter().find(|f| f.name == n && f.present).and_then(|f| match &f.value {
                FieldValue::Scalar(ScalarValue::U32(v)) => Some(*v),
                FieldValue::Scalar(ScalarValue::U16(v)) => Some(u32::from(*v)),
                FieldValue::Scalar(ScalarValue::U8(v))  => Some(u32::from(*v)),
                _ => None,
            })
        }
        fn pull_u64(b: &crate::save::ObjectBlock, n: &str) -> Option<u64> {
            b.fields.iter().find(|f| f.name == n).and_then(|f| match &f.value {
                FieldValue::Scalar(ScalarValue::U64(v)) => Some(*v),
                _ => None,
            })
        }

        // ── Pass 1: enumerate every MercenarySaveData (= mounts + humans) ──
        // and dump its field schema, equip count, inventory count, dye count.
        eprintln!("\n=== MercenarySaveData inventory ===");
        let mut merc_total = 0usize;
        let mut mount_total = 0usize;
        // (path, idx, charKey, resolved_name, merc_no, is_main, equip_count, inv_count, dyes)
        type MercRow = (String, usize, Option<u32>, String, Option<u64>, bool, usize, usize, usize);
        let mut merc_rows: Vec<MercRow> = Vec::new();

        fn walk_mercenaries(
            b: &crate::save::ObjectBlock,
            path: &str,
            merc_name: &std::collections::HashMap<u32, String>,
            merc_total: &mut usize,
            mount_total: &mut usize,
            merc_rows: &mut Vec<MercRow>,
        ) {
            // Is this a MercenarySaveData? If so, log it.
            if b.class_name == "MercenarySaveData" {
                *merc_total += 1;
                let mkey = pull_u32(b, "_characterKey");
                let stripped = mkey.map(|k| k & 0xFFFFFF);
                let name = stripped.and_then(|k| merc_name.get(&k).cloned())
                    .unwrap_or_else(|| "<unknown>".into());
                if name.starts_with("Vehicle_") || name.contains("Horse")
                    || name.contains("Mount") {
                    *mount_total += 1;
                }

                // Count equipment + inventory items
                let mut equip_count = 0usize;
                let mut inv_count = 0usize;
                let mut dye_in_lists = 0usize;
                let mut count_dyes_in_list = |list: &crate::save::FieldValue| {
                    if let FieldValue::ObjectList { elements, .. } = list {
                        for item in elements {
                            for f in &item.fields {
                                if f.name == "_itemDyeDataList"
                                    && f.present
                                    && let FieldValue::ObjectList { count, .. } = &f.value
                                {
                                    dye_in_lists += *count as usize;
                                }
                            }
                        }
                    }
                };
                for f in &b.fields {
                    match f.name.as_str() {
                        "_equipItemList" => {
                            if let FieldValue::ObjectList { count, .. } = &f.value {
                                equip_count = *count as usize;
                            }
                            count_dyes_in_list(&f.value);
                        }
                        "_inventoryItemList" => {
                            if let FieldValue::ObjectList { count, .. } = &f.value {
                                inv_count = *count as usize;
                            }
                            count_dyes_in_list(&f.value);
                        }
                        _ => {}
                    }
                }
                let merc_no = pull_u64(b, "_mercenaryNo");
                let is_main = b.fields.iter()
                    .any(|f| f.name == "_isMainMercenary" && f.present);
                merc_rows.push((path.to_string(), merc_rows.len(), stripped, name, merc_no, is_main, equip_count, inv_count, dye_in_lists));
            }
            // Recurse
            for f in &b.fields {
                match &f.value {
                    FieldValue::ObjectList { elements, .. } => {
                        for (i, e) in elements.iter().enumerate() {
                            let sub = format!("{path}.{}[{i}]", f.name);
                            walk_mercenaries(e, &sub, merc_name, merc_total, mount_total, merc_rows);
                        }
                    }
                    FieldValue::Locator { child: Some(c), .. } => {
                        let sub = format!("{path}.{}<child>", f.name);
                        walk_mercenaries(c, &sub, merc_name, merc_total, mount_total, merc_rows);
                    }
                    _ => {}
                }
            }
        }
        for (i, b) in blocks.iter().enumerate() {
            let root = format!("toc[{i}]:{}", b.class_name);
            walk_mercenaries(b, &root, &merc_name, &mut merc_total, &mut mount_total, &mut merc_rows);
        }

        eprintln!("\n  total MercenarySaveData blocks: {merc_total}");
        eprintln!("  of which mounts (Vehicle/Horse/Mount): {mount_total}");
        // Filter: only show non-empty / main entries (the 96-row dump
        // is too noisy otherwise).
        let mut shown = 0usize;
        for (path, _i, key, name, merc_no, is_main, equip, inv, dyes) in &merc_rows {
            if *equip == 0 && *inv == 0 && *dyes == 0 && !*is_main { continue; }
            let tag = if name.starts_with("Vehicle_") || name.contains("Horse")
                || name.contains("Mount") { "[MOUNT]" }
                else if *is_main { "[MAIN] " }
                else { "[OTHER]" };
            eprintln!(
                "  {tag} charKey={:?} mercNo={:?} name={:<36} equip={:>3} inv={:>3} dyes={:>2}  path={}",
                key, merc_no, name, equip, inv, dyes, path,
            );
            shown += 1;
        }
        let hidden = merc_rows.len() - shown;
        if hidden > 0 {
            eprintln!("  (+{hidden} empty mercenary slots hidden)");
        }

        // ── Pass 2: full dye enumeration (any ItemSaveData host) ──
        struct Hit<'a> {
            path_label: String,
            parent_class: String,
            // Closest enclosing mercenary key + name (if any)
            mercenary_label: Option<String>,
            parent_item_key: Option<u32>,
            parent_item_no: Option<u64>,
            dye_field: &'a crate::save::DecodedField,
        }
        let mut hits: Vec<Hit> = Vec::new();
        let mut class_hist: std::collections::BTreeMap<String, u32> = Default::default();

        fn walk_dyes<'a>(
            b: &'a crate::save::ObjectBlock,
            path: &str,
            current_merc: Option<&str>,
            merc_name: &std::collections::HashMap<u32, String>,
            out: &mut Vec<Hit<'a>>,
            class_hist: &mut std::collections::BTreeMap<String, u32>,
        ) {
            // Detect MercenarySaveData context so we can label child items.
            let next_merc_label: Option<String> = if b.class_name == "MercenarySaveData" {
                let k = pull_u32(b, "_characterKey");
                let stripped = k.map(|kk| kk & 0xFFFFFF);
                let n = stripped.and_then(|kk| merc_name.get(&kk).cloned())
                    .unwrap_or_else(|| "<unknown>".into());
                let merc_no = pull_u64(b, "_mercenaryNo");
                let is_main = b.fields.iter()
                    .find(|f| f.name == "_isMainMercenary" && f.present)
                    .map(|_| " MAIN").unwrap_or("");
                Some(format!("charKey={k:?} name={n} mercNo={merc_no:?}{is_main}"))
            } else {
                current_merc.map(|s| s.to_string())
            };
            for f in &b.fields {
                if f.name == "_itemDyeDataList" && f.present {
                    *class_hist.entry(b.class_name.clone()).or_insert(0) += 1;
                    out.push(Hit {
                        path_label: path.to_string(),
                        parent_class: b.class_name.clone(),
                        mercenary_label: next_merc_label.clone(),
                        parent_item_key: pull_u32(b, "_itemKey"),
                        parent_item_no: pull_u64(b, "_itemNo"),
                        dye_field: f,
                    });
                }
            }
            let merc_for_children = next_merc_label.as_deref();
            for f in &b.fields {
                match &f.value {
                    FieldValue::ObjectList { elements, .. } => {
                        for (i, e) in elements.iter().enumerate() {
                            let sub = format!("{path}.{}[{i}]", f.name);
                            walk_dyes(e, &sub, merc_for_children, merc_name, out, class_hist);
                        }
                    }
                    FieldValue::Locator { child: Some(c), .. } => {
                        let sub = format!("{path}.{}<child>", f.name);
                        walk_dyes(c, &sub, merc_for_children, merc_name, out, class_hist);
                    }
                    _ => {}
                }
            }
        }
        for (i, b) in blocks.iter().enumerate() {
            let root = format!("toc[{i}]:{}", b.class_name);
            walk_dyes(b, &root, None, &merc_name, &mut hits, &mut class_hist);
        }

        eprintln!("\n=== _itemDyeDataList host classes ===");
        for (cls, c) in &class_hist {
            eprintln!("  {cls}: {} occurrences", c);
        }
        eprintln!("\ntotal dye hits: {}", hits.len());

        // ── Per-hit dump ───────────────────────────────────────────
        eprintln!("\n=== dye enumeration ===");
        let mut total_entries = 0usize;
        for hit in &hits {
            let FieldValue::ObjectList { count, elements: dyes, .. } = &hit.dye_field.value
            else { continue };
            total_entries += *count as usize;
            eprintln!(
                "\n  [{}] itemKey={:?} itemNo={:?} class={} dyes={}",
                hit.mercenary_label.as_deref().unwrap_or("<none>"),
                hit.parent_item_key, hit.parent_item_no, hit.parent_class, count,
            );
            eprintln!("    path={}", hit.path_label);
            for (didx, dye) in dyes.iter().enumerate() {
                let pull_u8 = |name: &str| {
                    dye.fields.iter()
                        .find(|f| f.name == name && f.present)
                        .and_then(|f| match &f.value {
                            FieldValue::Scalar(ScalarValue::U8(v)) => Some(*v),
                            _ => None,
                        })
                };
                let pull_i8 = |name: &str| {
                    dye.fields.iter()
                        .find(|f| f.name == name && f.present)
                        .and_then(|f| match &f.value {
                            FieldValue::Scalar(ScalarValue::I8(v)) => Some(*v),
                            _ => None,
                        })
                };
                let pull_u16 = |name: &str| {
                    dye.fields.iter()
                        .find(|f| f.name == name && f.present)
                        .and_then(|f| match &f.value {
                            FieldValue::Scalar(ScalarValue::U16(v)) => Some(*v),
                            _ => None,
                        })
                };
                let pull_u32 = |name: &str| {
                    dye.fields.iter()
                        .find(|f| f.name == name && f.present)
                        .and_then(|f| match &f.value {
                            FieldValue::Scalar(ScalarValue::U32(v)) => Some(*v),
                            _ => None,
                        })
                };
                let r = pull_u8("_dyeColorR");
                let g = pull_u8("_dyeColorG");
                let b = pull_u8("_dyeColorB");
                let a = pull_u8("_dyeColorA");
                let slot_no = pull_i8("_dyeSlotNo");
                let grime = pull_i8("_grimeOpacity");
                let cgk = pull_u32("_dyeColorGroupInfoKey");
                let palette = pull_u16("_texturePalleteKey");
                let disable_sym = pull_i8("_disableSymbol");
                let fmt_u8 = |v: Option<u8>| v.map(|x| format!("{x}")).unwrap_or_else(|| "—".into());
                let fmt_i8 = |v: Option<i8>| v.map(|x| format!("{x}")).unwrap_or_else(|| "—".into());
                let fmt_u16 = |v: Option<u16>| v.map(|x| format!("{x}")).unwrap_or_else(|| "—".into());
                let fmt_u32 = |v: Option<u32>| v.map(|x| format!("0x{x:08x}")).unwrap_or_else(|| "—".into());
                eprintln!(
                    "    dye[{didx}] mask={:02x?} slotNo={} R={} G={} B={} A={} \
                     grime={} colorGroup={} palette={} disableSym={}",
                    dye.mask_bytes,
                    fmt_i8(slot_no),
                    fmt_u8(r), fmt_u8(g), fmt_u8(b), fmt_u8(a),
                    fmt_i8(grime),
                    fmt_u32(cgk),
                    fmt_u16(palette),
                    fmt_i8(disable_sym),
                );
            }
        }
        eprintln!("\n=== summary ===");
        eprintln!("  hits:          {}", hits.len());
        eprintln!("  total dyes:    {}", total_entries);
        eprintln!("  mercenaries:   {} (mounts: {})", merc_total, mount_total);
    }

    /// Abyss-gate per-gate mapping probe — Path B (`CrimsonAtomtic`) input.
    ///
    /// CrimsonAtomtic wants to replace the bulk "unlock all abyss gates"
    /// UX from the PyQt5 reference editor with per-gate controls
    /// (toggle lock state, mark discovered, set puzzle state). To do
    /// that it needs to identify each abyss-gate `FieldGimmickSaveData`
    /// block in a save and know which state-hash values mean what.
    /// This probe builds that mapping end-to-end.
    ///
    /// What it does (2026-05-15 baseline against a 1.07 `slot0/save.save`):
    ///
    /// 1. Loads `gimmickinfo.pabgb` (via the shipped parser) and
    ///    collects every row whose internal name contains "abyss" or
    ///    "hyperspace". 2,313 rows match in 1.07.
    /// 2. Walks the live save's `FieldGimmickSaveData` blocks (4,264
    ///    in this save), filters for those whose `_gimmickInfoKey`
    ///    lands in the abyss set. 356 blocks match.
    /// 3. For each match dumps `(_gimmickInfoKey, internal_name,
    ///    _ownerLevelName, _initStateNameHash, _isLockState,
    ///    _fieldGimmickSaveDataKey)` and writes the full table to
    ///    `out/abyss_gate_probe/mapping.json` (gitignored).
    /// 4. Asserts the **three known `_initStateNameHash` constants**
    ///    that this save sample surfaces:
    ///
    /// | Hash         | Count | Empirical meaning |
    /// |--------------|-------|-------------------|
    /// | `0x866c7489` | 88    | Default / "untouched" — assigned to bridge gimmicks the player hasn't crossed yet |
    /// | `0xe300acfe` | 16    | Activated — assigned to bridge gimmicks the player HAS crossed (state visibly changed in-game) |
    /// | `0x150b14d0` | 252   | Idle / decoration — standstones, artifacts, ambient abyss pieces |
    ///
    /// The hash → state-name decode is NOT pinned yet. None of Jenkins
    /// hashlittle / hashlittle2 / FNV-1a / SDBM / DJB2 / CRC32 with the
    /// seeds we tried produced a match against either common state-name
    /// candidates (Locked / Unlocked / Active / Root / Idle / …) or the
    /// ASCII strings harvested from the matching `.binarygimmick` files
    /// (`gimmick_abyssone_bridge_gate_01.binarygimmick`,
    /// `abyss_standstone_01.binarygimmick`). PA's HashCode32 appears
    /// to use a custom algorithm; cracking the decode requires IDA
    /// decompilation of the engine's hash routine (or one known
    /// `(name, hash)` pair to back-fit). See `docs/abyss-gate-map.md`
    /// §"Open RE" for the resume plan.
    ///
    /// **Useful breadcrumb for the decode work**: within each
    /// `.binarygimmick` body the state hash appears as a structured
    /// record. For example, in `gimmick_abyssone_bridge_gate_01.binarygimmick`
    /// at offset `0x16b`:
    /// ```text
    /// [0x283bf40d][0x7c9c9e2f][0xfd45d6ee][0x5bdda844][0x866c7489 00 00 00 00][0x866c7489 00 00 00 00] …
    /// ```
    /// The trailing duplicated `[state_hash][00 00 00 00]` pair is the
    /// state node record. The four preceding hashes are likely event-
    /// handler hashes (Enter / Exit / Frame / …) that re-appear across
    /// every state node — same shape observed in `abyss_standstone_01.binarygimmick`
    /// at a different offset. Cracking any one of those 4 handler-name
    /// hashes would give us the back-fit.
    ///
    /// Empirical state values are enough for the editor today — Path B
    /// can hardcode the three constants and patch
    /// `FieldGimmickSaveData._initStateNameHash` directly, no name
    /// decode needed. See `docs/abyss-gate-map.md` for the full
    /// implementation outline.
    #[test]
    #[ignore = "investigation only — abyss gate mapping for CrimsonAtomtic Path B"]
    fn _probe_abyss_gate_mapping() {
        use crate::binary::pamt::PackMeta;
        use crate::binary::paz;
        use crate::gimmick_info::parse_gimmick_info_lossy;
        use crate::save::{Body, FieldValue, ScalarValue, Save};

        // ── Locate the live save ───────────────────────────────────
        let save_path = std::env::var_os("CRIMSON_LIVE_SAVE")
            .map(PathBuf::from)
            .or_else(|| {
                let appdata = std::env::var_os("LOCALAPPDATA")?;
                let root = PathBuf::from(appdata)
                    .join("Pearl Abyss")
                    .join("CD")
                    .join("save");
                std::fs::read_dir(&root).ok()?.flatten().find_map(|entry| {
                    let p = entry.path().join("slot0").join("save.save");
                    p.is_file().then_some(p)
                })
            });
        let Some(save_path) = save_path else {
            eprintln!("skipping: no live save");
            return;
        };

        // ── Locate game install + extract gimmickinfo.pabgb ────────
        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let pamt_path = game_root.join("0008").join("0.pamt");
        let Ok(pamt_bytes) = std::fs::read(&pamt_path) else {
            eprintln!("skipping: no {}", pamt_path.display());
            return;
        };
        let pamt = PackMeta::parse(&pamt_bytes, None).expect("parse PAMT");
        let dir = pamt
            .directories
            .iter()
            .find(|d| d.path == "gamedata/binary__/client/bin")
            .expect("dir");
        let gimmick_file = dir
            .files
            .iter()
            .find(|f| f.name == "gimmickinfo.pabgb")
            .expect("gimmickinfo.pabgb missing");
        let gimmick_bytes = paz::extract_file(
            &game_root.join("0008"),
            gimmick_file,
            "gamedata/binary__/client/bin",
            &pamt.header.encrypt_info.encrypt_info,
        )
        .expect("extract gimmickinfo");

        // ── Filter gimmickinfo for abyss-related names ─────────────
        let gimmick_entries = parse_gimmick_info_lossy(&gimmick_bytes);
        let abyss_gimmicks: std::collections::HashMap<u32, String> = gimmick_entries
            .iter()
            .filter(|e| {
                let lower = e.name.to_ascii_lowercase();
                lower.contains("abyss") || lower.contains("hyperspace")
            })
            .map(|e| (e.key, e.name.clone()))
            .collect();
        eprintln!(
            "gimmickinfo: {} total rows, {} abyss-related",
            gimmick_entries.len(),
            abyss_gimmicks.len()
        );

        // ── Parse the save + collect abyss-gate FieldGimmick blocks ─
        let raw = std::fs::read(&save_path).expect("read save");
        let save = Save::parse(&raw).expect("parse save");
        let body = Body::parse(&save.body).expect("parse body");
        let blocks = body.decode_blocks(&save.body);

        let mut gimmick_blocks: Vec<&crate::save::ObjectBlock> = Vec::new();
        for b in &blocks {
            if b.class_name == "FieldGimmickSaveData" {
                gimmick_blocks.push(b);
            }
            walk(b, &mut gimmick_blocks);
        }
        fn walk<'a>(b: &'a crate::save::ObjectBlock, out: &mut Vec<&'a crate::save::ObjectBlock>) {
            for f in &b.fields {
                if let FieldValue::ObjectList { elements, .. } = &f.value {
                    for e in elements {
                        if e.class_name == "FieldGimmickSaveData" {
                            out.push(e);
                        }
                        walk(e, out);
                    }
                } else if let FieldValue::Locator { child: Some(c), .. } = &f.value {
                    walk(c, out);
                }
            }
        }

        // Per-gate record + collection
        #[derive(Debug, Clone)]
        struct GateRecord {
            gimmick_info_key: u32,
            internal_name: String,
            owner_level_name: String,
            init_state_hash: u32,
            is_lock_state: Option<bool>,
            field_gimmick_save_data_key: u32,
        }
        let mut gates: Vec<GateRecord> = Vec::new();
        for b in &gimmick_blocks {
            let mut gimmick_info_key: Option<u32> = None;
            let mut owner_level_name: String = String::new();
            let mut init_state_hash: Option<u32> = None;
            let mut is_lock_state: Option<bool> = None;
            let mut slot_key: Option<u32> = None;
            for f in &b.fields {
                if !f.present {
                    continue;
                }
                match (f.name.as_str(), &f.value) {
                    ("_gimmickInfoKey", FieldValue::Scalar(ScalarValue::U32(v))) => {
                        gimmick_info_key = Some(*v);
                    }
                    ("_ownerLevelName", FieldValue::InlineBytes { bytes, .. }) => {
                        owner_level_name = std::str::from_utf8(bytes)
                            .unwrap_or("")
                            .trim_end_matches('\0')
                            .to_string();
                    }
                    ("_initStateNameHash", FieldValue::Scalar(ScalarValue::U32(v))) => {
                        init_state_hash = Some(*v);
                    }
                    ("_isLockState", FieldValue::Scalar(ScalarValue::Bool(v))) => {
                        is_lock_state = Some(*v);
                    }
                    ("_fieldGimmickSaveDataKey", FieldValue::Scalar(ScalarValue::U32(v))) => {
                        slot_key = Some(*v);
                    }
                    _ => {}
                }
            }
            let Some(gimmick_info_key) = gimmick_info_key else { continue };
            let Some(internal_name) = abyss_gimmicks.get(&gimmick_info_key).cloned() else {
                continue;
            };
            gates.push(GateRecord {
                gimmick_info_key,
                internal_name,
                owner_level_name,
                init_state_hash: init_state_hash.unwrap_or(0),
                is_lock_state,
                field_gimmick_save_data_key: slot_key.unwrap_or(0),
            });
        }
        eprintln!("abyss-gate FieldGimmick blocks in save: {}", gates.len());
        assert!(
            gates.len() > 100,
            "expected >100 abyss-gate blocks; got {} — gimmickinfo filter may need updating",
            gates.len()
        );

        // ── Pin the 3 known state-hash constants ────────────────────
        let mut hash_counts: std::collections::BTreeMap<u32, u32> = Default::default();
        for g in &gates {
            *hash_counts.entry(g.init_state_hash).or_insert(0) += 1;
        }
        eprintln!("distinct _initStateNameHash values:");
        for (h, count) in &hash_counts {
            eprintln!("  0x{h:08x}: {count} gates");
        }
        // The three constants the Path B editor pin to. If any of these
        // disappears across patches, the editor's per-gate UI will need
        // a re-RE pass.
        for &expected in &[0x866c_7489u32, 0xe300_acfeu32, 0x150b_14d0u32] {
            assert!(
                hash_counts.contains_key(&expected),
                "missing pinned state hash 0x{expected:08x} — likely save-state or PA-patch drift"
            );
        }

        // Sort and dump the top of the per-gate table (first 12 for
        // visual inspection — full table in the JSON).
        let mut gates_sorted = gates.clone();
        gates_sorted.sort_by_key(|g| (g.owner_level_name.clone(), g.gimmick_info_key));
        eprintln!(
            "\n{:<12} {:<42} {:<32} {:>10} {:>5} {:>10}",
            "gimmickKey", "internal_name", "owner_level", "stateHash", "lock?", "slotKey"
        );
        for g in gates_sorted.iter().take(12) {
            eprintln!(
                "0x{:08x}   {:<42} {:<32} 0x{:08x} {:>5} {:>10}",
                g.gimmick_info_key,
                g.internal_name,
                g.owner_level_name,
                g.init_state_hash,
                g.is_lock_state.map_or("-".to_string(), |b| b.to_string()),
                g.field_gimmick_save_data_key,
            );
        }
        if gates_sorted.len() > 12 {
            eprintln!("(+{} more — see JSON dump)", gates_sorted.len() - 12);
        }

        // ── Dump JSON for offline analysis ─────────────────────────
        let out_dir = PathBuf::from("out").join("abyss_gate_probe");
        let _ = std::fs::create_dir_all(&out_dir);
        let json_path = out_dir.join("mapping.json");
        let mut json = String::new();
        json.push_str("{\n  \"gates\": [\n");
        for (i, g) in gates_sorted.iter().enumerate() {
            json.push_str(&format!(
                "    {{\"gimmick_info_key\": {}, \"internal_name\": {:?}, \"owner_level_name\": {:?}, \"init_state_hash\": {}, \"init_state_hash_hex\": \"0x{:08x}\", \"is_lock_state\": {}, \"field_gimmick_save_data_key\": {}}}{}\n",
                g.gimmick_info_key,
                g.internal_name,
                g.owner_level_name,
                g.init_state_hash,
                g.init_state_hash,
                g.is_lock_state.map_or("null".to_string(), |b| b.to_string()),
                g.field_gimmick_save_data_key,
                if i + 1 < gates_sorted.len() { "," } else { "" },
            ));
        }
        json.push_str("  ],\n  \"state_hashes\": [\n");
        let hash_vec: Vec<_> = hash_counts.iter().collect();
        for (i, (h, c)) in hash_vec.iter().enumerate() {
            let label = match **h {
                0x866c_7489 => "\"default_untouched\"",
                0xe300_acfe => "\"activated_crossed\"",
                0x150b_14d0 => "\"idle_decoration\"",
                _ => "null",
            };
            json.push_str(&format!(
                "    {{\"hash\": {}, \"hash_hex\": \"0x{:08x}\", \"gate_count\": {}, \"empirical_label\": {}}}{}\n",
                h,
                h,
                c,
                label,
                if i + 1 < hash_vec.len() { "," } else { "" },
            ));
        }
        json.push_str("  ]\n}\n");
        std::fs::write(&json_path, json).expect("write json");
        eprintln!("\nwrote mapping JSON: {}", json_path.display());
    }

    /// PALOC template-density probe.
    ///
    /// Loads the English PALOC, classifies every entry by namespace
    /// (the `lo32` of the u64 key — `0x70` = item titles, `0x30` =
    /// character titles, `0x71` = item descriptions, etc.), and counts
    /// how many entries in each namespace contain PA's template
    /// markers (`{`, `<br/>`, `%0`/`%1`/`%s`/`%d`, `[EMPTY]` sentinels).
    /// Output is the per-namespace template-density table that
    /// settles the "do we need a template resolver?" question.
    ///
    /// CrimsonForge's tokenizer (`D:\Github\crimsonforge\core\
    /// translation_tokenizer.py`) enumerates PA's template families:
    /// - `{StaticInfo:Type:Key#fallback_label}` — cross-reference with
    ///   embedded fallback label
    /// - `{plain:tokens}` — opaque cross-references
    /// - `<br/>`, `<b>`, `<color>` — HTML-style tags
    /// - `[EMPTY]`, `[FULL]` — sentinels
    /// - `%0`, `%1`, `%s`, `%d` — printf-style arg placeholders
    ///
    /// Resolver scope decision (per `docs/save-editor-keys-plan.md`
    /// status): only needed if a downstream consumer surfaces
    /// description / dialogue / objective text. The shipped
    /// `lookup_display_name` chains for Mission/Quest/Stage/Knowledge/
    /// Character/GimmickInfo target the **title** namespaces (lo32 =
    /// 0x30 / 0x100 / 0x101 / 0x490 / 0x200) where templates are rare
    /// or absent — those bridges don't need a resolver.
    #[test]
    #[ignore = "investigation only — paloc template density survey"]
    fn _probe_paloc_template_density() {
        use crate::binary::pamt::PackMeta;
        use crate::binary::paz;

        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let pamt_path = game_root.join("0020").join("0.pamt");
        let Ok(pamt_bytes) = std::fs::read(&pamt_path) else {
            eprintln!("skipping: no {}", pamt_path.display());
            return;
        };
        let pamt = PackMeta::parse(&pamt_bytes, None).expect("parse PAMT");
        let dir = pamt
            .directories
            .iter()
            .find(|d| d.path == "gamedata/stringtable/binary__")
            .expect("paloc dir");
        let file = dir
            .files
            .iter()
            .find(|f| f.name == "localizationstring_eng.paloc")
            .expect("paloc file");
        let paloc_bytes = paz::extract_file(
            &game_root.join("0020"),
            file,
            "gamedata/stringtable/binary__",
            &pamt.header.encrypt_info.encrypt_info,
        )
        .expect("extract paloc");
        let paloc = crate::binary::paloc::LocalizationFile::parse(&paloc_bytes)
            .expect("parse paloc");
        eprintln!("loaded English PALOC: {} entries", paloc.entries.len());

        // Per-namespace stats. Numeric key parses as u64; lo32 = key & 0xffffffff
        // is the namespace tag, hi32 = key >> 32 is the actual row id.
        #[derive(Default, Debug)]
        struct NsStats {
            total: u32,
            with_curly: u32,         // contains `{` … `}`
            with_static_info: u32,   // contains `{StaticInfo:`
            with_pct_arg: u32,       // contains `%0`/`%1`/`%s`/`%d`/`%%`
            with_br: u32,            // contains `<br/>` or `<br>`
            with_sentinel: u32,      // contains `[EMPTY]` / `[FULL]` / similar
            with_any_template: u32,
        }
        let mut by_ns: std::collections::BTreeMap<u32, NsStats> = Default::default();

        fn contains_pct_arg(s: &str) -> bool {
            let bytes = s.as_bytes();
            for i in 0..bytes.len().saturating_sub(1) {
                if bytes[i] == b'%' {
                    let next = bytes[i + 1];
                    if next.is_ascii_digit() || matches!(next, b's' | b'd' | b'%') {
                        return true;
                    }
                }
            }
            false
        }

        for e in &paloc.entries {
            // PALOC keys are stored as ASCII decimal strings of u64.
            let Ok(key_u64) = e.string_key.data.parse::<u64>() else { continue };
            let ns = key_u64 as u32; // lo32 = namespace
            let s = &e.string_value.data;
            let entry = by_ns.entry(ns).or_default();
            entry.total += 1;
            let curly = s.contains('{') && s.contains('}');
            let si = curly && s.contains("{StaticInfo:") || s.contains("{Staticinfo:");
            let pct = contains_pct_arg(s);
            let br = s.contains("<br/>") || s.contains("<br>") || s.contains("<BR/>");
            let sentinel = s.contains("[EMPTY]") || s.contains("[FULL]") || s.contains("[NONE]");
            if curly { entry.with_curly += 1; }
            if si { entry.with_static_info += 1; }
            if pct { entry.with_pct_arg += 1; }
            if br { entry.with_br += 1; }
            if sentinel { entry.with_sentinel += 1; }
            if curly || pct || br || sentinel {
                entry.with_any_template += 1;
            }
        }

        // Sort namespaces by total descending so the table is readable.
        let mut nss: Vec<(u32, &NsStats)> = by_ns.iter().map(|(k, v)| (*k, v)).collect();
        nss.sort_by_key(|x| std::cmp::Reverse(x.1.total));

        eprintln!(
            "\n{:<12} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
            "lo32_ns", "total", "any_tpl", "{...}", "Static", "%arg", "<br>", "[EMPT]"
        );
        for (ns, st) in nss.iter().take(25) {
            eprintln!(
                "0x{:08x} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
                ns, st.total, st.with_any_template, st.with_curly,
                st.with_static_info, st.with_pct_arg, st.with_br, st.with_sentinel,
            );
        }
        eprintln!("(+{} smaller namespaces)", nss.len().saturating_sub(25));

        // Specific namespaces the shipped bridges target — call those
        // out by name so the conclusion is obvious.
        let bridge_namespaces: &[(u32, &str)] = &[
            (0x30, "CharacterKey display"),
            (0x70, "ItemKey title"),
            (0x71, "ItemKey description (sometimes)"),
            (0x100, "QuestKey arc heading"),
            (0x101, "StageKey title"),
            (0x102, "StageKey description"),
            (0x200, "GimmickInfoKey display"),
            (0x490, "MissionKey/KnowledgeKey title"),
            (0x19202, "GimmickInfoKey long description"),
        ];
        eprintln!("\n— Shipped bridge target namespaces —");
        for &(ns, label) in bridge_namespaces {
            if let Some(st) = by_ns.get(&ns) {
                let pct = st
                    .total
                    .checked_div(1)
                    .map(|_| (st.with_any_template * 100).checked_div(st.total).unwrap_or(0))
                    .unwrap_or(0);
                eprintln!(
                    "  lo32=0x{:<6x} {:<40} total={:>5}  any_tpl={:>4} ({}%)  Static={}",
                    ns, label, st.total, st.with_any_template, pct, st.with_static_info,
                );
            } else {
                eprintln!("  lo32=0x{:<6x} {:<40} (no entries)", ns, label);
            }
        }
    }

    /// Dye gamedata schema probe — phase 1, smallest table first.
    ///
    /// Investigates the three `dye*.pabgb` + `.pabgh` tables surfaced
    /// by `_probe_item_dye_data` to settle their on-disk layout so we
    /// can ship anchor-scan / PABGH-indexed parsers + c_abi bridges
    /// that replace the PyQt5 reference editor's hand-maintained
    /// `dye_slot_counts.json`.
    ///
    /// Recommended order (see `docs/dye-editor-scope.md` §"Open RE"):
    ///
    /// 1. `dyecolorgroupinfo.pabgb` (~10 rows) — smallest, gets the
    ///    schema right with a low-volume sample. Resolves
    ///    `_dyeColorGroupInfoKey (u32)` → color group definition.
    /// 2. `partprefabdyetexturepalleteinfo.pabgb` (~5 rows) — tiny
    ///    sibling. Resolves `_texturePalleteKey (u16)` → material
    ///    palette name (Cloth / Metal / Leather / etc.).
    /// 3. `partprefabdyeslotinfo.pabgb` (~730 rows) — large, replaces
    ///    the `dye_slot_counts.json` catalog. Per-prefab slot counts.
    ///
    /// What this probe dumps for each file:
    ///
    /// - Extracted size (sanity-check vs PAMT metadata)
    /// - `.pabgh` parse via the standard skill_info layout
    ///   (`u16 count + count × (u32 key, u32 offset)`) — fails open if
    ///   the layout differs.
    /// - For each PABGH entry: `(key, offset, body_len_to_next)` plus
    ///   the first 64 hex bytes of the body. From this we can read
    ///   the row schema by eye (look for `[u32 key][u32 name_len][name]`
    ///   prefix, fixed-size structs, etc.).
    /// - First 256 bytes of the raw PABGB as a fallback if no PABGH.
    /// - Dumps the full extracted bytes to `out/dye_probe/<file>.bin`
    ///   so `plcli + .hexpat` can be used for deeper inspection.
    #[test]
    #[ignore = "investigation only — dye gamedata schema (phase 1)"]
    fn _probe_dye_gamedata_tables() {
        use crate::binary::pamt::PackMeta;
        use crate::binary::paz;

        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let pamt_path = game_root.join("0008").join("0.pamt");
        let Ok(pamt_bytes) = std::fs::read(&pamt_path) else {
            eprintln!("skipping: no {}", pamt_path.display());
            return;
        };
        let pamt = PackMeta::parse(&pamt_bytes, None).expect("parse PAMT");
        let dir = pamt
            .directories
            .iter()
            .find(|d| d.path == "gamedata/binary__/client/bin")
            .expect("missing gamedata/binary__/client/bin dir in 0008 PAMT");

        let out_dir = PathBuf::from("out").join("dye_probe");
        let _ = std::fs::create_dir_all(&out_dir);

        // Three pairs in the recommended order: smallest first.
        let targets: &[&str] = &[
            "dyecolorgroupinfo",
            "partprefabdyetexturepalleteinfo",
            "partprefabdyeslotinfo",
        ];

        for stem in targets {
            eprintln!("\n========================================");
            eprintln!("=== {stem} ===");
            eprintln!("========================================");

            let pabgb_name = format!("{stem}.pabgb");
            let pabgh_name = format!("{stem}.pabgh");

            let pabgb_file = dir.files.iter().find(|f| f.name == pabgb_name);
            let pabgh_file = dir.files.iter().find(|f| f.name == pabgh_name);

            let Some(pabgb_file) = pabgb_file else {
                eprintln!("  MISSING: {pabgb_name}");
                continue;
            };
            eprintln!(
                "  {pabgb_name}: compressed={}  uncompressed={}",
                pabgb_file.file.compressed_size, pabgb_file.file.uncompressed_size,
            );
            let pabgb_bytes = match paz::extract_file(
                &game_root.join("0008"),
                pabgb_file,
                "gamedata/binary__/client/bin",
                &pamt.header.encrypt_info.encrypt_info,
            ) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("  EXTRACT FAILED: {e}");
                    continue;
                }
            };
            eprintln!("  extracted .pabgb len = {} bytes", pabgb_bytes.len());

            let _ = std::fs::write(out_dir.join(&pabgb_name), &pabgb_bytes);

            // Try the standard PABGH layout.
            let pabgh_bytes_opt: Option<Vec<u8>> = pabgh_file.and_then(|pabgh_file| {
                eprintln!(
                    "  {pabgh_name}: compressed={}  uncompressed={}",
                    pabgh_file.file.compressed_size, pabgh_file.file.uncompressed_size,
                );
                paz::extract_file(
                    &game_root.join("0008"),
                    pabgh_file,
                    "gamedata/binary__/client/bin",
                    &pamt.header.encrypt_info.encrypt_info,
                )
                .ok()
            });

            if let Some(pabgh_bytes) = &pabgh_bytes_opt {
                let _ = std::fs::write(out_dir.join(&pabgh_name), pabgh_bytes);
                eprintln!("  extracted .pabgh len = {} bytes", pabgh_bytes.len());
                eprintln!(
                    "  .pabgh first 32 bytes: {:02x?}",
                    &pabgh_bytes[..pabgh_bytes.len().min(32)],
                );

                // Standard layout attempt.
                match crate::skill_info::parse_pabgh(pabgh_bytes) {
                    Ok(entries) => {
                        eprintln!(
                            "  .pabgh parsed as standard layout: {} entries",
                            entries.len()
                        );
                        // Build sorted-by-offset for end-range computation.
                        let ranges = crate::skill_info::entry_ranges(
                            &entries,
                            pabgb_bytes.len(),
                        );
                        // Print all entries for small tables; cap at 20 for large.
                        let n_show = if entries.len() <= 20 { entries.len() } else { 20 };
                        eprintln!(
                            "\n  showing first {n_show} of {} entries (key, offset, body_len, hex preview):",
                            entries.len()
                        );
                        for i in 0..n_show {
                            let (start, end) = ranges[i];
                            let body_len = end.saturating_sub(start);
                            let preview_end = (start + 64).min(end).min(pabgb_bytes.len());
                            let preview = if start < pabgb_bytes.len() {
                                &pabgb_bytes[start..preview_end]
                            } else {
                                &[][..]
                            };
                            eprintln!(
                                "    [{:3}] key=0x{:08x}  off=0x{:08x}  body_len={:5}  hex[0..{}]={:02x?}",
                                i,
                                entries[i].key,
                                entries[i].offset,
                                body_len,
                                preview.len(),
                                preview,
                            );
                        }
                        if entries.len() > n_show {
                            eprintln!("    (+{} more)", entries.len() - n_show);
                        }

                        // Body-length distribution — fixed-size tells if rows
                        // are flat structs vs variable-length anchor rows.
                        let mut len_counts: std::collections::BTreeMap<usize, usize> =
                            Default::default();
                        for (s, e) in &ranges {
                            *len_counts.entry(e.saturating_sub(*s)).or_insert(0) += 1;
                        }
                        eprintln!("\n  body-length histogram (len: count):");
                        for (len, count) in len_counts.iter().take(20) {
                            eprintln!("    {len:5} bytes: {count} rows");
                        }
                        if len_counts.len() > 20 {
                            eprintln!("    (+{} more distinct lengths)", len_counts.len() - 20);
                        }
                    }
                    Err(e) => {
                        eprintln!("  .pabgh standard-layout parse FAILED: {e}");
                    }
                }
            } else {
                eprintln!("  (no .pabgh extracted — anchor-scan-only file)");
            }

            // For files small enough, print the whole PABGB hex.
            if pabgb_bytes.len() <= 4096 {
                eprintln!("\n  .pabgb full hex dump ({} bytes):", pabgb_bytes.len());
                for chunk_start in (0..pabgb_bytes.len()).step_by(32) {
                    let chunk_end = (chunk_start + 32).min(pabgb_bytes.len());
                    let chunk = &pabgb_bytes[chunk_start..chunk_end];
                    let hex: String = chunk
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<Vec<_>>()
                        .join(" ");
                    let ascii: String = chunk
                        .iter()
                        .map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' })
                        .collect();
                    eprintln!("    {chunk_start:06x}  {hex:<95}  {ascii}");
                }
            } else {
                eprintln!(
                    "\n  .pabgb first 256 bytes (file too large for full dump):"
                );
                for chunk_start in (0..256).step_by(32) {
                    let chunk_end = (chunk_start + 32).min(pabgb_bytes.len());
                    let chunk = &pabgb_bytes[chunk_start..chunk_end];
                    let hex: String = chunk
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<Vec<_>>()
                        .join(" ");
                    let ascii: String = chunk
                        .iter()
                        .map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' })
                        .collect();
                    eprintln!("    {chunk_start:06x}  {hex:<95}  {ascii}");
                }
            }
        }

        eprintln!(
            "\n.pabgb / .pabgh raw bytes written to {} for plcli inspection",
            out_dir.display()
        );
    }

    /// Focused phase-2 probe: dump full per-row bytes + ASCII strings for the
    /// small tables, and the post-name region of partprefabdyeslotinfo so we
    /// can pin the slot-count field placement.
    #[test]
    #[ignore = "investigation only — dye gamedata schema (phase 2, focused)"]
    fn _probe_dye_gamedata_rows() {
        use crate::binary::pamt::PackMeta;
        use crate::binary::paz;

        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let pamt_path = game_root.join("0008").join("0.pamt");
        let Ok(pamt_bytes) = std::fs::read(&pamt_path) else {
            eprintln!("skipping: no {}", pamt_path.display());
            return;
        };
        let pamt = PackMeta::parse(&pamt_bytes, None).expect("parse PAMT");
        let dir = pamt
            .directories
            .iter()
            .find(|d| d.path == "gamedata/binary__/client/bin")
            .expect("missing dir");

        let extract = |name: &str| {
            let f = dir.files.iter().find(|f| f.name == name).expect("missing");
            paz::extract_file(
                &game_root.join("0008"),
                f,
                "gamedata/binary__/client/bin",
                &pamt.header.encrypt_info.encrypt_info,
            )
            .expect("extract")
        };

        // ── partprefabdyetexturepalleteinfo: custom PABGH (u16 key, u32 off) ─
        let pal_pabgb = extract("partprefabdyetexturepalleteinfo.pabgb");
        let pal_pabgh = extract("partprefabdyetexturepalleteinfo.pabgh");
        eprintln!("\n=== partprefabdyetexturepalleteinfo ===");
        eprintln!("pabgh len={}", pal_pabgh.len());
        // Parse custom layout
        let count = u16::from_le_bytes([pal_pabgh[0], pal_pabgh[1]]) as usize;
        eprintln!("count u16 = {count}");
        let mut entries: Vec<(u16, u32)> = Vec::new();
        for i in 0..count {
            let o = 2 + i * 6;
            let key = u16::from_le_bytes([pal_pabgh[o], pal_pabgh[o + 1]]);
            let off = u32::from_le_bytes([
                pal_pabgh[o + 2],
                pal_pabgh[o + 3],
                pal_pabgh[o + 4],
                pal_pabgh[o + 5],
            ]);
            entries.push((key, off));
        }
        eprintln!("entries: {:?}", entries);
        // Sort by offset to compute body ranges
        let mut by_offset: Vec<usize> = (0..entries.len()).collect();
        by_offset.sort_by_key(|&i| entries[i].1);
        let mut ends = vec![pal_pabgb.len(); entries.len()];
        for win in by_offset.windows(2) {
            ends[win[0]] = entries[win[1]].1 as usize;
        }

        for i in 0..entries.len() {
            let (key, off) = entries[i];
            let end = ends[i];
            let body = &pal_pabgb[off as usize..end];
            eprintln!(
                "\n  -- row key={key} (off=0x{off:x}, len={}) --",
                body.len()
            );
            // Hex+ascii in 32-byte chunks
            for cs in (0..body.len()).step_by(32) {
                let ce = (cs + 32).min(body.len());
                let chunk = &body[cs..ce];
                let hex: String = chunk
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                let ascii: String = chunk
                    .iter()
                    .map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' })
                    .collect();
                eprintln!("    {cs:04x}  {hex:<95}  {ascii}");
            }
            // Extract all ASCII strings (>=3 printable chars)
            let mut strs: Vec<String> = Vec::new();
            let mut cur = String::new();
            for &b in body {
                if (0x20..0x7f).contains(&b) {
                    cur.push(b as char);
                } else {
                    if cur.len() >= 3 {
                        strs.push(cur.clone());
                    }
                    cur.clear();
                }
            }
            if cur.len() >= 3 {
                strs.push(cur);
            }
            eprintln!("    strings (>=3 chars): {:?}", strs);
        }

        // ── dyecolorgroupinfo: dump full body of first 2 rows ──────────
        let dcg_pabgb = extract("dyecolorgroupinfo.pabgb");
        let dcg_pabgh = extract("dyecolorgroupinfo.pabgh");
        eprintln!("\n=== dyecolorgroupinfo (first 2 rows full hex) ===");
        let dcg_entries = crate::skill_info::parse_pabgh(&dcg_pabgh).expect("parse pabgh");
        let dcg_ranges = crate::skill_info::entry_ranges(&dcg_entries, dcg_pabgb.len());
        for i in 0..2 {
            let (s, e) = dcg_ranges[i];
            let body = &dcg_pabgb[s..e];
            eprintln!(
                "\n  -- row {i} key=0x{:08x} (off=0x{:x}, len={}) --",
                dcg_entries[i].key, s, body.len()
            );
            for cs in (0..body.len()).step_by(32) {
                let ce = (cs + 32).min(body.len());
                let chunk = &body[cs..ce];
                let hex: String = chunk
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                let ascii: String = chunk
                    .iter()
                    .map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' })
                    .collect();
                eprintln!("    {cs:04x}  {hex:<95}  {ascii}");
            }
        }

        // ── partprefabdyeslotinfo: dump full body of a few rows ────────
        let pps_pabgb = extract("partprefabdyeslotinfo.pabgb");
        let pps_pabgh = extract("partprefabdyeslotinfo.pabgh");
        eprintln!("\n=== partprefabdyeslotinfo (full hex of a low-slot, mid-slot, high-slot row) ===");
        let pps_entries = crate::skill_info::parse_pabgh(&pps_pabgh).expect("parse pabgh");
        let pps_ranges = crate::skill_info::entry_ranges(&pps_entries, pps_pabgb.len());
        // Print rows 0 (count=1), 5 (count=10), 9 (count=8) per the phase-1 dump
        for &i in &[0usize, 5, 9, 13] {
            let (s, e) = pps_ranges[i];
            let body = &pps_pabgb[s..e];
            eprintln!(
                "\n  -- row {i} key=0x{:08x} (off=0x{:x}, len={}) --",
                pps_entries[i].key, s, body.len()
            );
            for cs in (0..body.len()).step_by(32) {
                let ce = (cs + 32).min(body.len());
                let chunk = &body[cs..ce];
                let hex: String = chunk
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                let ascii: String = chunk
                    .iter()
                    .map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' })
                    .collect();
                eprintln!("    {cs:04x}  {hex:<95}  {ascii}");
            }
        }
    }

    /// Socket / gem schema probe — targets slot104 specifically.
    ///
    /// The user populated slot104 with 5 instances of "North Wind Trident"
    /// (itemkey 310031) configured to exercise every socket-state
    /// permutation we need for the C# socket editor:
    ///
    /// | Weapon | Max sockets | Opened | Filled positions | Note |
    /// |---|---:|---:|---|---|
    /// | 1 | 5 | 5 | 1,2,3,4,5 (all distinct gems) | baseline: all opened, all filled |
    /// | 2 | 5 | 4 | 1,2 (1002979); 3,4 empty; 5 closed | tests "opened but empty" + "not yet opened" |
    /// | 3 | 5 | 5 | 1..5 all 1002979 | all opened, all filled, same gem |
    /// | 4 | 5 | 5 | 1,3,5 (1002979); 2,4 empty | tests sparse-filled pattern |
    /// | 5 | 5 | 5 | 2,4 (1002979); 1,3,5 empty | tests inverse sparse pattern |
    ///
    /// Item 1002979 ("爆走的力量審判") is a high-durability gem
    /// (~100 endurance per the user). Vanilla gems are durability-less.
    ///
    /// What this dumps for each of the 5 weapons:
    ///
    /// 1. ItemSaveData top-level field tree — including `_endurance`
    ///    (the u16 the PyQt5 reference editor parses as
    ///    `endurance_low = durability`, `endurance_high = socket_count`)
    ///    and whichever ObjectList field carries the per-socket data.
    /// 2. The socket-list field's name, count, and per-element schema.
    /// 3. Per-element values: socket slot number, gem item key,
    ///    gem-side endurance / sharpness (if present), and any other
    ///    scalars — so we can spot the absent / opened-empty / filled
    ///    encoding.
    /// 4. Cross-check: scan a few random items with `_endurance` set
    ///    to confirm the lo-byte=durability / hi-byte=socket-count
    ///    interpretation and check how non-socket items (e.g.
    ///    standalone gems in the inventory) encode their own endurance.
    #[test]
    #[ignore = "investigation only — ItemSocketSaveData schema via slot104"]
    fn _probe_item_socket_data() {
        use crate::save::{Body, FieldValue, ScalarValue, Save};

        // ── Locate slot104 specifically ────────────────────────────
        let save_path = std::env::var_os("CRIMSON_SOCKET_PROBE_SAVE")
            .map(PathBuf::from)
            .or_else(|| {
                let appdata = std::env::var_os("LOCALAPPDATA")?;
                let root = PathBuf::from(appdata)
                    .join("Pearl Abyss")
                    .join("CD")
                    .join("save");
                std::fs::read_dir(&root).ok()?.flatten().find_map(|entry| {
                    let p = entry.path().join("slot104").join("save.save");
                    p.is_file().then_some(p)
                })
            });
        let Some(save_path) = save_path else {
            eprintln!("skipping: no slot104/save.save");
            return;
        };
        eprintln!("probing {}", save_path.display());

        let raw = std::fs::read(&save_path).expect("read save");
        let save = Save::parse(&raw).expect("parse save");
        let body = Body::parse(&save.body).expect("parse body");
        let blocks = body.decode_blocks(&save.body);

        const WEAPON_KEY: u32 = 310031; // North Wind Trident
        const GEM_KEY: u32 = 1002979;   // 爆走的力量審判

        // ── First pass: walk every item; collect tridents + record
        // ── distribution of endurance values for cross-check.
        let mut tridents: Vec<(usize, usize, usize, &crate::save::ObjectBlock)> = Vec::new();
        let mut gem_standalone: Vec<&crate::save::ObjectBlock> = Vec::new();
        let mut endurance_hist: std::collections::BTreeMap<u16, u32> = Default::default();
        let mut item_count: u32 = 0;

        for (block_idx, block) in blocks.iter().enumerate() {
            if block.class_name != "InventorySaveData" {
                continue;
            }
            for inv_list_field in &block.fields {
                if !inv_list_field.name.eq_ignore_ascii_case("_inventorylist") {
                    continue;
                }
                let FieldValue::ObjectList { elements: containers, .. } =
                    &inv_list_field.value
                else { continue };
                for (inv_idx, container) in containers.iter().enumerate() {
                    for f in &container.fields {
                        if !f.name.eq_ignore_ascii_case("_itemList") {
                            continue;
                        }
                        let FieldValue::ObjectList { elements: items, .. } = &f.value
                        else { continue };
                        for (item_idx, item) in items.iter().enumerate() {
                            item_count += 1;
                            let item_key = item
                                .fields
                                .iter()
                                .find(|f| f.name == "_itemKey")
                                .and_then(|f| match &f.value {
                                    FieldValue::Scalar(ScalarValue::U32(v)) => Some(*v),
                                    _ => None,
                                })
                                .unwrap_or(0);
                            // record endurance
                            if let Some(endurance) = item.fields.iter().find(|f| {
                                f.name.eq_ignore_ascii_case("_endurance")
                                    && f.present
                            }) && let FieldValue::Scalar(ScalarValue::U16(v)) = &endurance.value
                            {
                                *endurance_hist.entry(*v).or_insert(0) += 1;
                            }
                            if item_key == WEAPON_KEY {
                                tridents.push((block_idx, inv_idx, item_idx, item));
                            } else if item_key == GEM_KEY {
                                gem_standalone.push(item);
                            }
                        }
                    }
                }
            }
        }

        eprintln!(
            "scanned {} items across {} blocks; {} tridents, {} standalone gems",
            item_count,
            blocks.len(),
            tridents.len(),
            gem_standalone.len()
        );

        if tridents.is_empty() {
            eprintln!("no North Wind Trident (key={WEAPON_KEY}) found in slot104 — \
                       cannot dump socket schema");
            return;
        }

        // ── For each trident, dump the full ItemSaveData schema +
        // focus on the socket list field.
        for (n, (block_idx, inv_idx, item_idx, item)) in tridents.iter().enumerate() {
            eprintln!(
                "\n\n========================================\n\
                 === Weapon {} (block={}, inv={}, item={}) ===\n\
                 ========================================",
                n + 1, block_idx, inv_idx, item_idx,
            );
            eprintln!(
                "class={} mask={:02x?} data_size={}",
                item.class_name, item.mask_bytes, item.data_size,
            );

            // Full field dump
            for f in &item.fields {
                let val_str = match &f.value {
                    FieldValue::Scalar(ScalarValue::U8(v)) => format!("U8({v})"),
                    FieldValue::Scalar(ScalarValue::I8(v)) => format!("I8({v})"),
                    FieldValue::Scalar(ScalarValue::U16(v)) => {
                        format!("U16({v} = 0x{v:04x}, hi=0x{:02x} lo=0x{:02x})", v >> 8, v & 0xff)
                    }
                    FieldValue::Scalar(ScalarValue::U32(v)) => format!("U32(0x{v:08x} = {v})"),
                    FieldValue::Scalar(ScalarValue::U64(v)) => format!("U64({v})"),
                    FieldValue::Scalar(ScalarValue::I64(v)) => format!("I64({v})"),
                    FieldValue::Scalar(ScalarValue::Bool(v)) => format!("Bool({v})"),
                    FieldValue::None => "<absent>".into(),
                    FieldValue::ObjectList { count, elements, .. } => {
                        format!("ObjectList count={count} elements_parsed={}", elements.len())
                    }
                    _ => format!("{:?}", f.value),
                };
                eprintln!(
                    "  [{:2}] present={} kind={:?} type={} name={} meta_size={} value={}",
                    f.field_index, f.present, f.kind, f.type_name, f.name, f.meta_size, val_str,
                );
            }

            // Drill into any ObjectList field whose name mentions
            // "socket" / "gem" / "option" / etc. — the candidate set
            // is small enough to just iterate.
            for f in &item.fields {
                let lname = f.name.to_ascii_lowercase();
                let interesting = lname.contains("socket")
                    || lname.contains("gem")
                    || lname.contains("option")
                    || lname.contains("seal")
                    || lname.contains("enchant");
                if !interesting || !f.present {
                    continue;
                }
                if let FieldValue::ObjectList { count, elements, .. } = &f.value {
                    eprintln!(
                        "\n  ── socket-candidate field {} (count={count}) ──",
                        f.name
                    );
                    for (sub_idx, sub) in elements.iter().enumerate() {
                        eprintln!(
                            "    [{:2}] class={} mask={:02x?} data_size={}",
                            sub_idx, sub.class_name, sub.mask_bytes, sub.data_size,
                        );
                        for sf in &sub.fields {
                            let val = match &sf.value {
                                FieldValue::Scalar(ScalarValue::U8(v)) => format!("U8({v})"),
                                FieldValue::Scalar(ScalarValue::I8(v)) => format!("I8({v})"),
                                FieldValue::Scalar(ScalarValue::U16(v)) => format!(
                                    "U16({v} = 0x{v:04x}, hi=0x{:02x} lo=0x{:02x})",
                                    v >> 8, v & 0xff
                                ),
                                FieldValue::Scalar(ScalarValue::U32(v)) => {
                                    format!("U32(0x{v:08x} = {v})")
                                }
                                FieldValue::Scalar(ScalarValue::U64(v)) => format!("U64({v})"),
                                FieldValue::Scalar(ScalarValue::I64(v)) => format!("I64({v})"),
                                FieldValue::Scalar(ScalarValue::Bool(v)) => format!("Bool({v})"),
                                FieldValue::None => "<absent>".into(),
                                _ => format!("{:?}", sf.value),
                            };
                            eprintln!(
                                "       [{:2}] present={} kind={:?} type={} name={} meta_size={} val={}",
                                sf.field_index, sf.present, sf.kind, sf.type_name,
                                sf.name, sf.meta_size, val,
                            );
                        }
                    }
                }
            }
        }

        // ── Cross-check: dump a couple of standalone gem 1002979
        // ── items (if any in inventory) to see their endurance shape.
        if !gem_standalone.is_empty() {
            eprintln!("\n\n=== standalone gem (key={GEM_KEY}) — first 2 of {} ===",
                gem_standalone.len());
            for (n, item) in gem_standalone.iter().take(2).enumerate() {
                eprintln!("\n  -- gem instance #{} --", n + 1);
                for f in &item.fields {
                    if !f.present { continue; }
                    let val = match &f.value {
                        FieldValue::Scalar(ScalarValue::U8(v)) => format!("U8({v})"),
                        FieldValue::Scalar(ScalarValue::U16(v)) => {
                            format!("U16({v}=0x{v:04x} hi=0x{:02x} lo=0x{:02x})",
                                v >> 8, v & 0xff)
                        }
                        FieldValue::Scalar(ScalarValue::U32(v)) => format!("U32({v})"),
                        FieldValue::Scalar(ScalarValue::U64(v)) => format!("U64({v})"),
                        FieldValue::Scalar(ScalarValue::I64(v)) => format!("I64({v})"),
                        FieldValue::Scalar(ScalarValue::Bool(v)) => format!("Bool({v})"),
                        _ => format!("{:?}", f.value),
                    };
                    eprintln!("    [{:2}] {} = {}", f.field_index, f.name, val);
                }
            }
        }

        // ── Endurance histogram across the whole save.
        eprintln!("\n\n=== _endurance histogram (top 30, raw u16) ===");
        let mut sorted: Vec<_> = endurance_hist.iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(a.1));
        for (val, count) in sorted.iter().take(30) {
            let v = **val;
            eprintln!(
                "  {:5} (0x{:04x}, hi=0x{:02x} lo=0x{:02x}): {} items",
                v, v, v >> 8, v & 0xff, count,
            );
        }
    }

    /// Phase-4 socket probe — finds **every block (in any class)**
    /// across slot104 that has a present `_socketSaveDataList`, then
    /// dumps the full schema of any item the user specifies via the
    /// `CRIMSON_SOCKET_TARGET_KEYS` env var (comma-separated u32
    /// itemkeys). Targets the EquipmentSaveData blocks that the
    /// InventorySaveData-only walker missed.
    ///
    /// Default targets if no env var: 1002285 / 1002284 / 1000316
    /// (嘟嘟鳥放電盔甲 / 嘟嘟鳥馬羅尼雷射頭盔 / 嘟嘟鳥里西的鞋子 —
    /// user's CE-modified examples with stuffed-beyond-vanilla sockets).
    ///
    /// Re-run with:
    ///   cargo test --lib --features c_abi _probe_item_socket_data_anywhere -- --ignored --nocapture
    #[test]
    #[ignore = "investigation only — walk every block for socket data, including EquipmentSaveData"]
    fn _probe_item_socket_data_anywhere() {
        use crate::save::{Body, FieldValue, ScalarValue, Save};

        let save_path = std::env::var_os("CRIMSON_SOCKET_PROBE_SAVE")
            .map(PathBuf::from)
            .or_else(|| {
                let appdata = std::env::var_os("LOCALAPPDATA")?;
                let root = PathBuf::from(appdata)
                    .join("Pearl Abyss")
                    .join("CD")
                    .join("save");
                std::fs::read_dir(&root).ok()?.flatten().find_map(|entry| {
                    let p = entry.path().join("slot104").join("save.save");
                    p.is_file().then_some(p)
                })
            });
        let Some(save_path) = save_path else {
            eprintln!("skipping: no slot104/save.save");
            return;
        };
        eprintln!("probing {}", save_path.display());

        // Targets — default to the 3 user-flagged CE-modified items.
        let targets: std::collections::BTreeSet<u32> = std::env::var("CRIMSON_SOCKET_TARGET_KEYS")
            .ok()
            .map(|s| s.split(',').filter_map(|x| x.trim().parse::<u32>().ok()).collect())
            .unwrap_or_else(|| {
                let mut s = std::collections::BTreeSet::new();
                s.insert(1002285u32); // 嘟嘟鳥放電盔甲
                s.insert(1002284u32); // 嘟嘟鳥馬羅尼雷射頭盔
                s.insert(1000316u32); // 嘟嘟鳥里西的鞋子
                s
            });
        eprintln!("targets: {:?}", targets);

        let raw = std::fs::read(&save_path).expect("read save");
        let save = Save::parse(&raw).expect("parse save");
        let body = Body::parse(&save.body).expect("parse body");
        let blocks = body.decode_blocks(&save.body);

        // ── Walk the entire decoded tree depth-first, recording every
        // place a _socketSaveDataList lives. Captures the parent class
        // so we can see EquipmentSaveData / ItemSaveData / others.
        struct Hit<'a> {
            path_label: String,
            parent_class: String,
            parent_item_key: Option<u32>,
            parent_item_no: Option<u64>,
            parent_max_socket: Option<u8>,
            parent_valid_socket: Option<u8>,
            socket_field: &'a crate::save::DecodedField,
        }
        let mut hits: Vec<Hit> = Vec::new();
        let mut class_hist: std::collections::BTreeMap<String, u32> = Default::default();

        fn walk<'a>(
            block: &'a crate::save::ObjectBlock,
            path: &str,
            out: &mut Vec<Hit<'a>>,
            class_hist: &mut std::collections::BTreeMap<String, u32>,
        ) {
            // Pull scalars we care about from this block.
            let pull_u32 = |name: &str| {
                block.fields.iter()
                    .find(|f| f.name == name)
                    .and_then(|f| match &f.value {
                        FieldValue::Scalar(ScalarValue::U32(v)) => Some(*v),
                        _ => None,
                    })
            };
            let pull_u64 = |name: &str| {
                block.fields.iter()
                    .find(|f| f.name == name)
                    .and_then(|f| match &f.value {
                        FieldValue::Scalar(ScalarValue::U64(v)) => Some(*v),
                        _ => None,
                    })
            };
            let pull_u8 = |name: &str| {
                block.fields.iter()
                    .find(|f| f.name == name && f.present)
                    .and_then(|f| match &f.value {
                        FieldValue::Scalar(ScalarValue::U8(v)) => Some(*v),
                        _ => None,
                    })
            };

            // Does this block itself carry a present _socketSaveDataList?
            for f in &block.fields {
                if f.name == "_socketSaveDataList" && f.present {
                    *class_hist.entry(block.class_name.clone()).or_insert(0) += 1;
                    out.push(Hit {
                        path_label: path.to_string(),
                        parent_class: block.class_name.clone(),
                        parent_item_key: pull_u32("_itemKey"),
                        parent_item_no: pull_u64("_itemNo"),
                        parent_max_socket: pull_u8("_maxSocketCount"),
                        parent_valid_socket: pull_u8("_validSocketCount"),
                        socket_field: f,
                    });
                }
            }

            // Recurse into list / locator children.
            for f in &block.fields {
                match &f.value {
                    FieldValue::ObjectList { elements, .. } => {
                        for (i, e) in elements.iter().enumerate() {
                            let sub = format!("{path}.{}[{i}]", f.name);
                            walk(e, &sub, out, class_hist);
                        }
                    }
                    FieldValue::Locator { child: Some(c), .. } => {
                        let sub = format!("{path}.{}<child>", f.name);
                        walk(c, &sub, out, class_hist);
                    }
                    _ => {}
                }
            }
        }

        for (i, block) in blocks.iter().enumerate() {
            let root_label = format!("toc[{i}]:{}", block.class_name);
            walk(block, &root_label, &mut hits, &mut class_hist);
        }

        eprintln!("\n=== _socketSaveDataList host classes ===");
        for (cls, c) in &class_hist {
            eprintln!("  {cls}: {} occurrences", c);
        }

        // ── Targeted dump: every hit whose itemKey is in `targets`.
        eprintln!("\n=== targeted dumps (matching itemKeys) ===");
        let mut found_count = 0u32;
        for hit in &hits {
            let Some(item_key) = hit.parent_item_key else { continue };
            if !targets.contains(&item_key) {
                continue;
            }
            found_count += 1;
            let FieldValue::ObjectList { count, elements: sockets, .. } = &hit.socket_field.value
            else { continue };
            let filled = sockets.iter().filter(|s| {
                s.fields.iter().any(|sf| sf.name == "_itemKey" && sf.present)
            }).count();
            let mut tags: Vec<&str> = Vec::new();
            if let (Some(max), Some(valid)) = (hit.parent_max_socket, hit.parent_valid_socket) {
                if (filled as u32) > u32::from(valid) { tags.push("FILLED>VALID"); }
                if (filled as u32) > u32::from(max)   { tags.push("FILLED>MAX"); }
            }
            eprintln!(
                "\n  itemKey={} itemNo={:?} class={} path={}",
                item_key, hit.parent_item_no, hit.parent_class, hit.path_label,
            );
            eprintln!(
                "    max={:?} valid={:?} list_count={} filled={}{}",
                hit.parent_max_socket,
                hit.parent_valid_socket,
                count,
                filled,
                if tags.is_empty() { String::new() } else { format!(" tags=[{}]", tags.join(",")) },
            );
            for (sidx, socket) in sockets.iter().enumerate() {
                let gem = socket.fields.iter()
                    .find(|sf| sf.name == "_itemKey" && sf.present)
                    .and_then(|sf| match &sf.value {
                        FieldValue::Scalar(ScalarValue::U32(v)) => Some(*v),
                        _ => None,
                    });
                let end = socket.fields.iter()
                    .find(|sf| sf.name == "_currentEndurance" && sf.present)
                    .and_then(|sf| match &sf.value {
                        FieldValue::Scalar(ScalarValue::U16(v)) => Some(*v),
                        _ => None,
                    });
                eprintln!(
                    "    slot[{}] mask={:02x?} data_size={} gem={:?} endurance={:?}",
                    sidx, socket.mask_bytes, socket.data_size, gem, end,
                );
            }
        }

        if found_count == 0 {
            eprintln!(
                "  (none of the {} target itemKeys were found in slot104 — \
                 they might be in a different slot or not in the inventory now)",
                targets.len(),
            );
        }

        // ── Anomaly summary across ALL hits (now including EquipmentSaveData).
        let mut anomalies_all = 0u32;
        for hit in &hits {
            let FieldValue::ObjectList { elements: sockets, .. } = &hit.socket_field.value
            else { continue };
            let filled = sockets.iter().filter(|s| {
                s.fields.iter().any(|sf| sf.name == "_itemKey" && sf.present)
            }).count() as u32;
            if let Some(valid) = hit.parent_valid_socket
                && filled > u32::from(valid)
            {
                anomalies_all += 1;
            }
        }
        eprintln!("\n=== global summary ===");
        eprintln!("  total _socketSaveDataList occurrences: {}", hits.len());
        eprintln!("  hits with filled > _validSocketCount:  {}", anomalies_all);
    }

    /// Phase-3 socket probe — scans **every save slot under
    /// %LOCALAPPDATA%\Pearl Abyss\CD\save\<user>\slot*\** for items
    /// with anomalous socket data. Reuses the per-item invariant logic
    /// of `_probe_item_socket_data_full_scan` but aggregates across
    /// the whole user save tree to surface CE-modified items
    /// (`filled > _validSocketCount`) wherever they live.
    ///
    /// Re-run with:
    ///   cargo test --lib --features c_abi _probe_item_socket_data_all_slots -- --ignored --nocapture
    #[test]
    #[ignore = "investigation only — scan every slot for socket anomalies"]
    fn _probe_item_socket_data_all_slots() {
        use crate::save::{Body, FieldValue, ScalarValue, Save};

        let Some(local) = std::env::var_os("LOCALAPPDATA") else {
            eprintln!("skipping: no LOCALAPPDATA");
            return;
        };
        let root = PathBuf::from(local).join("Pearl Abyss/CD/save");
        let Ok(users) = std::fs::read_dir(&root) else {
            eprintln!("skipping: cannot read {}", root.display());
            return;
        };

        let mut slot_paths: Vec<PathBuf> = Vec::new();
        for u in users.flatten() {
            let up = u.path();
            if !up.is_dir() { continue; }
            let Ok(entries) = std::fs::read_dir(&up) else { continue };
            for e in entries.flatten() {
                let p = e.path();
                if !p.is_dir() { continue; }
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !name.starts_with("slot") { continue; }
                let save_path = p.join("save.save");
                if save_path.is_file() {
                    slot_paths.push(save_path);
                }
            }
        }
        slot_paths.sort();
        eprintln!("scanning {} save files", slot_paths.len());

        let mut grand_total_items = 0u32;
        let mut grand_total_anomalies = 0u32;
        let mut anomalies_by_slot: std::collections::BTreeMap<String, Vec<String>> =
            Default::default();
        let mut all_mask_histogram: std::collections::BTreeMap<Vec<u8>, u32> =
            Default::default();
        let mut all_max_socket_hist: std::collections::BTreeMap<u8, u32> =
            Default::default();

        for save_path in &slot_paths {
            let raw = match std::fs::read(save_path) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("  skip {}: {e}", save_path.display());
                    continue;
                }
            };
            let save = match Save::parse(&raw) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("  parse error {}: {e}", save_path.display());
                    continue;
                }
            };
            let body = match Body::parse(&save.body) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("  body error {}: {e}", save_path.display());
                    continue;
                }
            };
            let blocks = body.decode_blocks(&save.body);

            let slot_label = save_path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("?")
                .to_string();

            let mut slot_anomalies: Vec<String> = Vec::new();
            let mut slot_item_count = 0u32;

            for block in &blocks {
                if block.class_name != "InventorySaveData" { continue; }
                for inv_list_field in &block.fields {
                    if !inv_list_field.name.eq_ignore_ascii_case("_inventorylist") {
                        continue;
                    }
                    let FieldValue::ObjectList { elements: containers, .. } =
                        &inv_list_field.value
                    else { continue };
                    for (inv_idx, container) in containers.iter().enumerate() {
                        for f in &container.fields {
                            if !f.name.eq_ignore_ascii_case("_itemList") { continue; }
                            let FieldValue::ObjectList { elements: items_in_list, .. } = &f.value
                            else { continue };
                            for (item_idx, item) in items_in_list.iter().enumerate() {
                                let Some(socket_field) = item.fields.iter()
                                    .find(|f| f.name == "_socketSaveDataList" && f.present)
                                else { continue };
                                let FieldValue::ObjectList { count, elements: sockets, .. } =
                                    &socket_field.value
                                else { continue };
                                if *count == 0 { continue; }
                                slot_item_count += 1;

                                let item_key = item.fields.iter()
                                    .find(|f| f.name == "_itemKey")
                                    .and_then(|f| match &f.value {
                                        FieldValue::Scalar(ScalarValue::U32(v)) => Some(*v),
                                        _ => None,
                                    }).unwrap_or(0);
                                let item_no = item.fields.iter()
                                    .find(|f| f.name == "_itemNo")
                                    .and_then(|f| match &f.value {
                                        FieldValue::Scalar(ScalarValue::U64(v)) => Some(*v),
                                        _ => None,
                                    }).unwrap_or(0);
                                let max_socket = item.fields.iter()
                                    .find(|f| f.name == "_maxSocketCount" && f.present)
                                    .and_then(|f| match &f.value {
                                        FieldValue::Scalar(ScalarValue::U8(v)) => Some(*v),
                                        _ => None,
                                    }).unwrap_or(0);
                                let valid_socket = item.fields.iter()
                                    .find(|f| f.name == "_validSocketCount" && f.present)
                                    .and_then(|f| match &f.value {
                                        FieldValue::Scalar(ScalarValue::U8(v)) => Some(*v),
                                        _ => None,
                                    }).unwrap_or(0);
                                *all_max_socket_hist.entry(max_socket).or_insert(0) += 1;

                                let mut filled = 0u32;
                                let mut unusual_count = 0u32;
                                let mut per_slot: Vec<String> = Vec::new();
                                for (sidx, socket) in sockets.iter().enumerate() {
                                    let mask = socket.mask_bytes.clone();
                                    *all_mask_histogram.entry(mask.clone()).or_insert(0) += 1;
                                    let m = mask.first().copied().unwrap_or(0);
                                    let is_filled = socket.fields.iter().any(|sf| {
                                        sf.name == "_itemKey" && sf.present
                                    });
                                    if is_filled { filled += 1; }
                                    if m != 0x00 && m != 0x03 {
                                        unusual_count += 1;
                                    }
                                    let gem_key = socket.fields.iter()
                                        .find(|sf| sf.name == "_itemKey" && sf.present)
                                        .and_then(|sf| match &sf.value {
                                            FieldValue::Scalar(ScalarValue::U32(v)) => Some(*v),
                                            _ => None,
                                        });
                                    let gem_end = socket.fields.iter()
                                        .find(|sf| sf.name == "_currentEndurance" && sf.present)
                                        .and_then(|sf| match &sf.value {
                                            FieldValue::Scalar(ScalarValue::U16(v)) => Some(*v),
                                            _ => None,
                                        });
                                    if m != 0x00 || gem_key.is_some() || gem_end.is_some() {
                                        per_slot.push(format!(
                                            "    slot[{sidx}] mask={:02x?} gem={:?} end={:?}",
                                            mask, gem_key, gem_end
                                        ));
                                    }
                                }

                                let mut tags: Vec<&str> = Vec::new();
                                if *count != u32::from(max_socket) { tags.push("LIST≠MAX"); }
                                if filled > u32::from(valid_socket) { tags.push("FILLED>VALID"); }
                                if filled > u32::from(max_socket) { tags.push("FILLED>MAX"); }
                                if unusual_count > 0 { tags.push("BAD_MASK"); }
                                if !tags.is_empty() {
                                    let msg = format!(
                                        "  inv={inv_idx} item={item_idx} itemKey={item_key} itemNo={item_no} \
                                         max={max_socket} valid={valid_socket} list={count} filled={filled} \
                                         tags=[{}]\n{}",
                                        tags.join(","),
                                        per_slot.join("\n"),
                                    );
                                    slot_anomalies.push(msg);
                                }
                            }
                        }
                    }
                }
            }

            grand_total_items += slot_item_count;
            if !slot_anomalies.is_empty() {
                grand_total_anomalies += slot_anomalies.len() as u32;
                eprintln!("\n=== {} ({} items, {} anomalies) ===",
                    slot_label, slot_item_count, slot_anomalies.len());
                for a in &slot_anomalies {
                    eprintln!("{a}");
                }
                anomalies_by_slot.insert(slot_label, slot_anomalies);
            }
        }

        eprintln!("\n\n=== GRAND SUMMARY ===");
        eprintln!("  slots scanned:                 {}", slot_paths.len());
        eprintln!("  items with sockets (all slots): {}", grand_total_items);
        eprintln!("  anomalous items (all slots):    {}", grand_total_anomalies);
        eprintln!("  slots with anomalies:           {}", anomalies_by_slot.len());

        eprintln!("\n  global _maxSocketCount histogram:");
        for (max, c) in &all_max_socket_hist {
            eprintln!("    max={:3} : {} items", max, c);
        }
        eprintln!("\n  global mask byte histogram:");
        for (mask, c) in &all_mask_histogram {
            eprintln!("    {:02x?} : {} elements", mask, c);
        }
    }

    /// Phase-2 socket probe — scans **every** item in slot104 with a
    /// present `_socketSaveDataList`, validates the schema invariants
    /// observed on the 5 reference tridents, and flags anomalies.
    ///
    /// What the previous probe (`_probe_item_socket_data`) established:
    ///
    /// - `_socketSaveDataList.count == _maxSocketCount` (always)
    /// - Each `ItemSocketSaveData` has exactly 2 fields + 1 mask byte
    /// - mask=[0x03] = filled, mask=[0x00] = empty (opened OR not-yet-opened)
    /// - filled_count is normally ≤ `_validSocketCount`
    ///
    /// What this probe additionally checks:
    ///
    /// 1. Whether the invariants hold across ALL socket-bearing items
    ///    (armour, accessories, other weapons — not just the 5 tridents).
    /// 2. **CE-modified anomalies** the user flagged: filled_count >
    ///    `_validSocketCount` (overfilled item). These work in-game but
    ///    might break NPC-interface UIs.
    /// 3. Any unusual mask bytes (e.g. [0x01], [0x02]) suggesting a
    ///    partial-field gem state we haven't seen yet.
    /// 4. `_maxSocketCount` distribution — does any item type cap above 5?
    /// 5. Cross-reference: for filled sockets, the gem's `_itemKey` —
    ///    is the value plausibly a gem itemkey (in the millions, not a
    ///    weapon/armour key)?
    ///
    /// Output: a per-item table flagging which items are "vanilla"
    /// (filled ≤ valid) vs CE-modified (filled > valid). Plus a
    /// schema-invariant summary at the end.
    #[test]
    #[ignore = "investigation only — full slot104 socket scan + anomaly detection"]
    fn _probe_item_socket_data_full_scan() {
        use crate::save::{Body, FieldValue, ScalarValue, Save};

        let save_path = std::env::var_os("CRIMSON_SOCKET_PROBE_SAVE")
            .map(PathBuf::from)
            .or_else(|| {
                let appdata = std::env::var_os("LOCALAPPDATA")?;
                let root = PathBuf::from(appdata)
                    .join("Pearl Abyss")
                    .join("CD")
                    .join("save");
                std::fs::read_dir(&root).ok()?.flatten().find_map(|entry| {
                    let p = entry.path().join("slot104").join("save.save");
                    p.is_file().then_some(p)
                })
            });
        let Some(save_path) = save_path else {
            eprintln!("skipping: no slot104/save.save");
            return;
        };
        eprintln!("probing {}", save_path.display());

        let raw = std::fs::read(&save_path).expect("read save");
        let save = Save::parse(&raw).expect("parse save");
        let body = Body::parse(&save.body).expect("parse body");
        let blocks = body.decode_blocks(&save.body);

        #[derive(Debug)]
        struct SocketItem {
            inv_idx: u32,
            item_idx: u32,
            item_key: u32,
            slot_no: u16,
            item_no: u64,
            max_socket: u8,
            valid_socket: u8,
            list_count: u32,
            slots: Vec<(Vec<u8>, Option<u32>, Option<u16>)>, // (mask_bytes, gem_itemkey, gem_endurance)
        }

        let mut items: Vec<SocketItem> = Vec::new();

        for block in &blocks {
            if block.class_name != "InventorySaveData" {
                continue;
            }
            for inv_list_field in &block.fields {
                if !inv_list_field.name.eq_ignore_ascii_case("_inventorylist") {
                    continue;
                }
                let FieldValue::ObjectList { elements: containers, .. } =
                    &inv_list_field.value
                else { continue };
                for (inv_idx, container) in containers.iter().enumerate() {
                    for f in &container.fields {
                        if !f.name.eq_ignore_ascii_case("_itemList") {
                            continue;
                        }
                        let FieldValue::ObjectList { elements: items_in_list, .. } = &f.value
                        else { continue };
                        for (item_idx, item) in items_in_list.iter().enumerate() {
                            // Pull scalar fields we care about.
                            let item_key = item.fields.iter()
                                .find(|f| f.name == "_itemKey")
                                .and_then(|f| match &f.value {
                                    FieldValue::Scalar(ScalarValue::U32(v)) => Some(*v),
                                    _ => None,
                                }).unwrap_or(0);
                            let slot_no = item.fields.iter()
                                .find(|f| f.name == "_slotNo")
                                .and_then(|f| match &f.value {
                                    FieldValue::Scalar(ScalarValue::U16(v)) => Some(*v),
                                    _ => None,
                                }).unwrap_or(0);
                            let item_no = item.fields.iter()
                                .find(|f| f.name == "_itemNo")
                                .and_then(|f| match &f.value {
                                    FieldValue::Scalar(ScalarValue::U64(v)) => Some(*v),
                                    _ => None,
                                }).unwrap_or(0);
                            let max_socket = item.fields.iter()
                                .find(|f| f.name == "_maxSocketCount" && f.present)
                                .and_then(|f| match &f.value {
                                    FieldValue::Scalar(ScalarValue::U8(v)) => Some(*v),
                                    _ => None,
                                });
                            let valid_socket = item.fields.iter()
                                .find(|f| f.name == "_validSocketCount" && f.present)
                                .and_then(|f| match &f.value {
                                    FieldValue::Scalar(ScalarValue::U8(v)) => Some(*v),
                                    _ => None,
                                });
                            let Some(socket_field) = item.fields.iter()
                                .find(|f| f.name == "_socketSaveDataList" && f.present)
                            else { continue };
                            let FieldValue::ObjectList { count, elements: sockets, .. } =
                                &socket_field.value
                            else { continue };
                            if *count == 0 {
                                continue;
                            }
                            // Drill into each socket entry.
                            let mut slot_records = Vec::with_capacity(sockets.len());
                            for socket in sockets {
                                let mask = socket.mask_bytes.clone();
                                let gem_key = socket.fields.iter()
                                    .find(|f| f.name == "_itemKey" && f.present)
                                    .and_then(|f| match &f.value {
                                        FieldValue::Scalar(ScalarValue::U32(v)) => Some(*v),
                                        _ => None,
                                    });
                                let gem_end = socket.fields.iter()
                                    .find(|f| f.name == "_currentEndurance" && f.present)
                                    .and_then(|f| match &f.value {
                                        FieldValue::Scalar(ScalarValue::U16(v)) => Some(*v),
                                        _ => None,
                                    });
                                slot_records.push((mask, gem_key, gem_end));
                            }
                            items.push(SocketItem {
                                inv_idx: inv_idx as u32,
                                item_idx: item_idx as u32,
                                item_key,
                                slot_no,
                                item_no,
                                max_socket: max_socket.unwrap_or(0),
                                valid_socket: valid_socket.unwrap_or(0),
                                list_count: *count,
                                slots: slot_records,
                            });
                        }
                    }
                }
            }
        }

        eprintln!("\nfound {} items with non-empty _socketSaveDataList", items.len());

        // ── Per-item table ──────────────────────────────────────────
        eprintln!(
            "\n{:>3}  {:>3}  {:>5}  {:>10}  {:>6}  {:>4}  {:>4}  {:>4}  {:>6}  status",
            "inv", "itm", "slot", "itemKey", "itemNo", "max", "vld", "list", "filled"
        );
        eprintln!("{:-<100}", "");

        // Track invariants across all items.
        let mut bad_count_mismatch = 0u32;     // list_count != max_socket
        let mut overfilled = 0u32;             // filled > valid (CE-modified)
        let mut over_max = 0u32;               // filled > max (would be impossible if our model is right)
        let mut unusual_mask = 0u32;           // mask not [0x00] or [0x03]
        type PartialMaskExample = (u32, usize, Vec<u8>, Option<u32>, Option<u16>);
        let mut partial_mask_examples: Vec<PartialMaskExample> = Vec::new();
        let mut mask_histogram: std::collections::BTreeMap<Vec<u8>, u32> = Default::default();
        let mut max_socket_hist: std::collections::BTreeMap<u8, u32> = Default::default();
        let mut gem_itemkey_set: std::collections::BTreeSet<u32> = Default::default();

        for it in &items {
            let filled = it.slots.iter()
                .filter(|(_, key, _)| key.is_some())
                .count() as u32;
            *max_socket_hist.entry(it.max_socket).or_insert(0) += 1;

            // Anomaly flags
            let mut tags: Vec<&str> = Vec::new();
            if it.list_count != u32::from(it.max_socket) {
                tags.push("LIST≠MAX");
                bad_count_mismatch += 1;
            }
            if filled > u32::from(it.valid_socket) {
                tags.push("FILLED>VALID");
                overfilled += 1;
            }
            if filled > u32::from(it.max_socket) {
                tags.push("FILLED>MAX");
                over_max += 1;
            }
            for (sidx, (mask, key, end)) in it.slots.iter().enumerate() {
                *mask_histogram.entry(mask.clone()).or_insert(0) += 1;
                let m = mask.first().copied().unwrap_or(0);
                if m != 0x00 && m != 0x03 {
                    unusual_mask += 1;
                    if partial_mask_examples.len() < 8 {
                        partial_mask_examples.push((it.item_key, sidx, mask.clone(), *key, *end));
                    }
                }
                if let Some(k) = key {
                    gem_itemkey_set.insert(*k);
                }
            }
            let status = if tags.is_empty() { "ok".to_string() } else { tags.join(",") };
            eprintln!(
                "{:>3}  {:>3}  {:>5}  {:>10}  {:>6}  {:>4}  {:>4}  {:>4}  {:>6}  {}",
                it.inv_idx, it.item_idx, it.slot_no, it.item_key, it.item_no,
                it.max_socket, it.valid_socket, it.list_count, filled, status,
            );
        }

        // ── Detail dump for anomalous items ────────────────────────
        let anomalies: Vec<&SocketItem> = items.iter()
            .filter(|it| {
                let filled = it.slots.iter().filter(|(_, k, _)| k.is_some()).count() as u32;
                it.list_count != u32::from(it.max_socket)
                    || filled > u32::from(it.valid_socket)
                    || it.slots.iter().any(|(m, _, _)| {
                        let mb = m.first().copied().unwrap_or(0);
                        mb != 0x00 && mb != 0x03
                    })
            })
            .collect();
        if !anomalies.is_empty() {
            eprintln!("\n=== anomalies: {} items ===", anomalies.len());
            for it in anomalies.iter().take(20) {
                let filled = it.slots.iter().filter(|(_, k, _)| k.is_some()).count();
                eprintln!(
                    "\n  itemKey={} itemNo={} inv={} item={} max={} valid={} list={} filled={}",
                    it.item_key, it.item_no, it.inv_idx, it.item_idx,
                    it.max_socket, it.valid_socket, it.list_count, filled,
                );
                for (sidx, (mask, key, end)) in it.slots.iter().enumerate() {
                    eprintln!(
                        "    slot[{}]: mask={:02x?} gem_key={:?} gem_endurance={:?}",
                        sidx, mask, key, end,
                    );
                }
            }
        } else {
            eprintln!("\n(no anomalies)");
        }

        // ── Summary ────────────────────────────────────────────────
        eprintln!("\n=== schema-invariant summary ===");
        eprintln!("  items with sockets:           {}", items.len());
        eprintln!("  list_count != _maxSocketCount: {} (HARD invariant — should be 0)", bad_count_mismatch);
        eprintln!("  filled > _validSocketCount:    {} (CE-modified — expected non-zero)", overfilled);
        eprintln!("  filled > _maxSocketCount:      {} (would break the fixed-size model — should be 0)", over_max);
        eprintln!("  socket entries with unusual mask (not 0x00/0x03): {}", unusual_mask);

        eprintln!("\n  _maxSocketCount histogram:");
        for (max, c) in &max_socket_hist {
            eprintln!("    max={:3} : {} items", max, c);
        }

        eprintln!("\n  mask byte histogram (socket-list elements):");
        for (mask, c) in &mask_histogram {
            eprintln!("    {:02x?} : {} elements", mask, c);
        }

        if !partial_mask_examples.is_empty() {
            eprintln!("\n  partial-mask examples (first 8):");
            for (ik, sidx, mask, key, end) in &partial_mask_examples {
                eprintln!(
                    "    itemKey={} slot[{}]: mask={:02x?} gem_key={:?} endurance={:?}",
                    ik, sidx, mask, key, end,
                );
            }
        }

        eprintln!("\n  distinct gem itemkeys seen ({}):", gem_itemkey_set.len());
        let gems: Vec<u32> = gem_itemkey_set.iter().copied().collect();
        for chunk in gems.chunks(10) {
            let s: Vec<String> = chunk.iter().map(|k| format!("{k}")).collect();
            eprintln!("    {}", s.join(", "));
        }
    }

    /// Investigation probe: extract the three faction `.pabgh` + `.pabgb`
    /// pairs (factionnode, factionrelationgroup, factionspawndatainfo) and
    /// dump the first ~64 bytes of each to identify the PABGH header
    /// shape and the row prefix.
    #[test]
    #[ignore = "investigation only — faction pabgh shape probe"]
    fn _probe_faction_pabgh_shapes() {
        use crate::binary::pamt::PackMeta;
        use crate::binary::paz;
        use std::fmt::Write;

        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let pamt_path = game_root.join("0008").join("0.pamt");
        let Ok(pamt_bytes) = std::fs::read(&pamt_path) else {
            eprintln!("skipping: no {}", pamt_path.display());
            return;
        };
        let pamt = PackMeta::parse(&pamt_bytes, None).expect("parse PAMT");
        let dir = pamt
            .directories
            .iter()
            .find(|d| d.path == "gamedata/binary__/client/bin")
            .expect("missing gamedata/binary__/client/bin dir in 0008 PAMT");

        let out_dir = std::path::PathBuf::from("out/faction_probe");
        std::fs::create_dir_all(&out_dir).ok();

        let targets = &[
            "factionnode",
            "factionrelationgroup",
            "factionspawndatainfo",
            // Bonus: also dump the sibling tables so we know what's there.
            "faction",
            "factiongroup",
            "factionnodespawninfo",
            "factionwaypoint",
            "allygroupinfo",
            "tribeinfo",
        ];

        let mut summary = String::new();
        for stem in targets {
            let pabgb_name = format!("{stem}.pabgb");
            let pabgh_name = format!("{stem}.pabgh");
            let pabgb_file = dir.files.iter().find(|f| f.name == pabgb_name);
            let pabgh_file = dir.files.iter().find(|f| f.name == pabgh_name);
            let Some(pabgb_file) = pabgb_file else {
                let _ = writeln!(summary, "\n=== {stem} ===  MISSING .pabgb");
                continue;
            };
            let Some(pabgh_file) = pabgh_file else {
                let _ = writeln!(summary, "\n=== {stem} ===  MISSING .pabgh");
                continue;
            };
            let pabgh_bytes = match paz::extract_file(
                &game_root.join("0008"),
                pabgh_file,
                "gamedata/binary__/client/bin",
                &pamt.header.encrypt_info.encrypt_info,
            ) {
                Ok(b) => b,
                Err(e) => {
                    let _ = writeln!(summary, "{stem}.pabgh extract failed: {e}");
                    continue;
                }
            };
            let pabgb_bytes = match paz::extract_file(
                &game_root.join("0008"),
                pabgb_file,
                "gamedata/binary__/client/bin",
                &pamt.header.encrypt_info.encrypt_info,
            ) {
                Ok(b) => b,
                Err(e) => {
                    let _ = writeln!(summary, "{stem}.pabgb extract failed: {e}");
                    continue;
                }
            };
            std::fs::write(out_dir.join(format!("{stem}.pabgh")), &pabgh_bytes).ok();
            std::fs::write(out_dir.join(format!("{stem}.pabgb")), &pabgb_bytes).ok();

            let _ = writeln!(
                summary,
                "\n=== {stem} ===  pabgh={} B  pabgb={} B",
                pabgh_bytes.len(),
                pabgb_bytes.len()
            );

            match crate::skill_info::parse_pabgh(&pabgh_bytes) {
                Ok(entries) => {
                    let preview: Vec<_> = entries
                        .iter()
                        .take(4)
                        .map(|e| (e.key, e.offset))
                        .collect();
                    let _ = writeln!(
                        summary,
                        "  standard u16+(u32,u32) PABGH OK  {} entries  first 4: {:?}",
                        entries.len(),
                        preview,
                    );
                    if let Some(first) = entries.first() {
                        let start = first.offset as usize;
                        let end = (start + 96).min(pabgb_bytes.len());
                        let row = &pabgb_bytes[start..end];
                        let _ = writeln!(
                            summary,
                            "  first row bytes ({} B at offset {}):",
                            end - start,
                            start,
                        );
                        let _ = writeln!(summary, "    {:02x?}", row);
                        if row.len() >= 8 {
                            let key =
                                u32::from_le_bytes([row[0], row[1], row[2], row[3]]);
                            let name_len = u32::from_le_bytes([
                                row[4], row[5], row[6], row[7],
                            ]) as usize;
                            let _ = writeln!(
                                summary,
                                "    interpreted: key={} (=PABGH? {}) name_len={}",
                                key,
                                key == first.key,
                                name_len,
                            );
                            if (1..=128).contains(&name_len)
                                && row.len() >= 8 + name_len
                                && let Ok(s) =
                                    std::str::from_utf8(&row[8..8 + name_len])
                            {
                                let _ = writeln!(summary, "    name: {:?}", s);
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = writeln!(
                        summary,
                        "  standard u16+(u32,u32) PABGH FAILED ({}). Header bytes:",
                        e
                    );
                    let n = pabgh_bytes.len().min(64);
                    let _ = writeln!(summary, "    {:02x?}", &pabgh_bytes[..n]);
                    if pabgh_bytes.len() >= 2 {
                        let cnt16 =
                            u16::from_le_bytes([pabgh_bytes[0], pabgh_bytes[1]])
                                as usize;
                        if pabgh_bytes.len() == 2 + cnt16 * 6 {
                            let _ = writeln!(
                                summary,
                                "  heuristic: u16 count + (u16 key, u32 offset)*  count={cnt16}",
                            );
                            for i in 0..cnt16.min(8) {
                                let off = 2 + i * 6;
                                let k = u16::from_le_bytes([
                                    pabgh_bytes[off],
                                    pabgh_bytes[off + 1],
                                ]);
                                let o = u32::from_le_bytes([
                                    pabgh_bytes[off + 2],
                                    pabgh_bytes[off + 3],
                                    pabgh_bytes[off + 4],
                                    pabgh_bytes[off + 5],
                                ]);
                                let _ = writeln!(
                                    summary,
                                    "    [{i}] key={k} offset={o}",
                                );
                            }
                        } else if pabgh_bytes.len() == 2 + cnt16 * 12 {
                            let _ = writeln!(
                                summary,
                                "  heuristic: u16 count + (u64 key, u32 offset)*  count={cnt16}",
                            );
                            for i in 0..cnt16.min(8) {
                                let off = 2 + i * 12;
                                let k = u64::from_le_bytes([
                                    pabgh_bytes[off],
                                    pabgh_bytes[off + 1],
                                    pabgh_bytes[off + 2],
                                    pabgh_bytes[off + 3],
                                    pabgh_bytes[off + 4],
                                    pabgh_bytes[off + 5],
                                    pabgh_bytes[off + 6],
                                    pabgh_bytes[off + 7],
                                ]);
                                let o = u32::from_le_bytes([
                                    pabgh_bytes[off + 8],
                                    pabgh_bytes[off + 9],
                                    pabgh_bytes[off + 10],
                                    pabgh_bytes[off + 11],
                                ]);
                                let _ = writeln!(
                                    summary,
                                    "    [{i}] key={k} offset={o}",
                                );
                            }
                        }
                    }
                }
            }
        }
        let out_path = out_dir.join("pabgh_shapes.txt");
        std::fs::write(&out_path, &summary).expect("write summary");
        eprintln!("wrote {}", out_path.display());
    }

    /// Investigation probe: walk PALOC for each faction-bridge target
    /// table's (key, name) pairs and report which namespaces / chain
    /// shape (identity vs hash hop) light up.
    #[test]
    #[ignore = "investigation only — faction PALOC chain probe"]
    fn _probe_faction_paloc_chains() {
        use crate::binary::paloc::LocalizationFile;
        use std::fmt::Write;

        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        if !game_root.join("0020").join("0.pamt").is_file() {
            eprintln!("skipping: no game install");
            return;
        }

        // Extract English PALOC. The 0020 group ships the English
        // localization table under gamedata/stringtable/binary__.
        let pamt_bytes = std::fs::read(game_root.join("0020").join("0.pamt"))
            .expect("read 0020 pamt");
        let pamt = crate::binary::pamt::PackMeta::parse(&pamt_bytes, None).expect("parse 0020");
        let dir = pamt
            .directories
            .iter()
            .find(|d| d.path == "gamedata/stringtable/binary__")
            .expect("missing stringtable dir");
        let paloc_file = dir
            .files
            .iter()
            .find(|f| f.name == "localizationstring_eng.paloc")
            .expect("missing eng paloc");
        let paloc_bytes = crate::binary::paz::extract_file(
            &game_root.join("0020"),
            paloc_file,
            "gamedata/stringtable/binary__",
            &pamt.header.encrypt_info.encrypt_info,
        )
        .expect("extract paloc");
        let parsed = LocalizationFile::parse(&paloc_bytes).expect("parse paloc");

        // PALOC keys are stored as ASCII decimal strings; the lookup
        // table is keyed by string.
        use std::collections::HashMap;
        let mut numeric: HashMap<u64, String> = HashMap::new();
        for e in &parsed.entries {
            if let Ok(k) = e.string_key.data.parse::<u64>() {
                numeric.insert(k, e.string_value.data.to_owned());
            }
        }
        eprintln!("loaded {} numeric PALOC entries", numeric.len());

        // Load each table from out/faction_probe.
        let probe_dir = std::path::PathBuf::from("out/faction_probe");

        struct Row {
            key: u32,
            name: String,
        }
        fn parse_standard(pabgh: &[u8], pabgb: &[u8]) -> Vec<Row> {
            let Ok(entries) = crate::skill_info::parse_pabgh(pabgh) else {
                return Vec::new();
            };
            let ranges = crate::skill_info::entry_ranges(&entries, pabgb.len());
            let mut out = Vec::new();
            for (e, (s, eo)) in entries.iter().zip(ranges.iter()) {
                let body = &pabgb[*s..*eo];
                if body.len() < 8 {
                    continue;
                }
                let key = u32::from_le_bytes([body[0], body[1], body[2], body[3]]);
                let nl = u32::from_le_bytes([body[4], body[5], body[6], body[7]]) as usize;
                if !(1..=128).contains(&nl) || 8 + nl > body.len() {
                    continue;
                }
                let Ok(s) = std::str::from_utf8(&body[8..8 + nl]) else {
                    continue;
                };
                if key != e.key {
                    continue;
                }
                out.push(Row {
                    key,
                    name: s.to_owned(),
                });
            }
            out
        }
        fn parse_small_u16(pabgh: &[u8], pabgb: &[u8]) -> Vec<Row> {
            // u16 count + (u16 key, u32 offset)*; row prefix [u16 key][u32 name_len][name].
            if pabgh.len() < 2 {
                return Vec::new();
            }
            let cnt = u16::from_le_bytes([pabgh[0], pabgh[1]]) as usize;
            if pabgh.len() != 2 + cnt * 6 {
                return Vec::new();
            }
            let mut out = Vec::new();
            let mut offs: Vec<u32> = (0..cnt)
                .map(|i| {
                    u32::from_le_bytes([
                        pabgh[2 + i * 6 + 2],
                        pabgh[2 + i * 6 + 3],
                        pabgh[2 + i * 6 + 4],
                        pabgh[2 + i * 6 + 5],
                    ])
                })
                .collect();
            offs.push(pabgb.len() as u32);
            offs.sort();
            for i in 0..cnt {
                let key16 =
                    u16::from_le_bytes([pabgh[2 + i * 6], pabgh[2 + i * 6 + 1]]);
                let off = u32::from_le_bytes([
                    pabgh[2 + i * 6 + 2],
                    pabgh[2 + i * 6 + 3],
                    pabgh[2 + i * 6 + 4],
                    pabgh[2 + i * 6 + 5],
                ]) as usize;
                let next = offs
                    .iter()
                    .find(|o| **o as usize > off)
                    .copied()
                    .unwrap_or(pabgb.len() as u32) as usize;
                let body = &pabgb[off..next.min(pabgb.len())];
                if body.len() < 6 {
                    continue;
                }
                let body_key = u16::from_le_bytes([body[0], body[1]]);
                if body_key != key16 {
                    continue;
                }
                let nl = u32::from_le_bytes([body[2], body[3], body[4], body[5]]) as usize;
                if !(1..=128).contains(&nl) || 6 + nl > body.len() {
                    continue;
                }
                let Ok(s) = std::str::from_utf8(&body[6..6 + nl]) else {
                    continue;
                };
                out.push(Row {
                    key: u32::from(key16),
                    name: s.to_owned(),
                });
            }
            out
        }

        let factionnode = {
            let pabgh = std::fs::read(probe_dir.join("factionnode.pabgh")).unwrap();
            let pabgb = std::fs::read(probe_dir.join("factionnode.pabgb")).unwrap();
            parse_standard(&pabgh, &pabgb)
        };
        let factionspawn = {
            let pabgh =
                std::fs::read(probe_dir.join("factionspawndatainfo.pabgh")).unwrap();
            let pabgb =
                std::fs::read(probe_dir.join("factionspawndatainfo.pabgb")).unwrap();
            parse_standard(&pabgh, &pabgb)
        };
        let factionrel = {
            let pabgh =
                std::fs::read(probe_dir.join("factionrelationgroup.pabgh")).unwrap();
            let pabgb =
                std::fs::read(probe_dir.join("factionrelationgroup.pabgb")).unwrap();
            parse_small_u16(&pabgh, &pabgb)
        };

        let tables: Vec<(&str, &Vec<Row>)> = vec![
            ("factionnode", &factionnode),
            ("factionspawndatainfo", &factionspawn),
            ("factionrelationgroup", &factionrel),
        ];

        let mut out = String::new();
        for (label, rows) in tables {
            let _ = writeln!(out, "\n=== {label} ({} rows parsed) ===", rows.len());
            if rows.is_empty() {
                continue;
            }

            // Identity chain: (key << 32) | lo32
            // Hash hop:       (hashlittle2(name) << 32) | lo32
            // Try lo32 values we've seen for other tables.
            let los: &[u32] = &[
                0x00, 0x30, 0x60, 0x70, 0x71, 0x80, 0x90, 0xc1, 0x100, 0x101, 0x102,
                0x190, 0x200, 0x202, 0x300, 0x400, 0x490, 0x491, 0x49d, 0x49e, 0x49f,
                0x800, 0x890, 0x891, 0x892, 0x500, 0x501,
            ];

            let mut id_hits: HashMap<u32, (u32, String)> = HashMap::new();
            let mut hash_hits: HashMap<u32, (u32, String)> = HashMap::new();

            for row in rows.iter() {
                let hi_hash = crate::crypto::checksum::calculate_checksum(row.name.as_bytes());
                for &lo in los {
                    // identity
                    let pk = ((row.key as u64) << 32) | (lo as u64);
                    if let Some(v) = numeric.get(&pk) {
                        let entry =
                            id_hits.entry(lo).or_insert((0, format!("{} -> {}", row.name, v)));
                        entry.0 += 1;
                    }
                    let pk_h = ((hi_hash as u64) << 32) | (lo as u64);
                    if let Some(v) = numeric.get(&pk_h) {
                        let entry = hash_hits
                            .entry(lo)
                            .or_insert((0, format!("{} -> {}", row.name, v)));
                        entry.0 += 1;
                    }
                }
            }

            let _ = writeln!(out, "  identity chain hits per lo32:");
            let mut id_sorted: Vec<_> = id_hits.iter().collect();
            id_sorted.sort_by_key(|(k, _)| **k);
            for (lo, (count, sample)) in &id_sorted {
                let _ = writeln!(
                    out,
                    "    0x{:03x} ({}): {} hits  sample: {}",
                    lo, lo, count, sample
                );
            }
            let _ = writeln!(out, "  hash-hop chain hits per lo32:");
            let mut h_sorted: Vec<_> = hash_hits.iter().collect();
            h_sorted.sort_by_key(|(k, _)| **k);
            for (lo, (count, sample)) in &h_sorted {
                let _ = writeln!(
                    out,
                    "    0x{:03x} ({}): {} hits  sample: {}",
                    lo, lo, count, sample
                );
            }
        }
        std::fs::write(probe_dir.join("paloc_chains.txt"), &out).expect("write");
        eprintln!("wrote out/faction_probe/paloc_chains.txt");
    }

    /// Investigation probe: dump the raw row bytes for `factionrelationgroup`
    /// and `factiongroup` so we can identify the small u16-keyed row schema.
    #[test]
    #[ignore = "investigation only — faction small-table row dumps"]
    fn _probe_faction_small_tables() {
        use std::fmt::Write;
        let probe_dir = std::path::PathBuf::from("out/faction_probe");
        let mut out = String::new();
        for stem in ["factionrelationgroup", "factiongroup"] {
            let Ok(pabgh) = std::fs::read(probe_dir.join(format!("{stem}.pabgh"))) else {
                let _ = writeln!(out, "{stem}.pabgh: not found");
                continue;
            };
            let Ok(pabgb) = std::fs::read(probe_dir.join(format!("{stem}.pabgb"))) else {
                let _ = writeln!(out, "{stem}.pabgb: not found");
                continue;
            };
            let cnt = u16::from_le_bytes([pabgh[0], pabgh[1]]) as usize;
            assert_eq!(pabgh.len(), 2 + cnt * 6, "{stem} pabgh shape mismatch");
            let _ = writeln!(out, "\n=== {stem} ===  {cnt} rows, pabgb={} B", pabgb.len());
            // Parse entries as (u16 key, u32 offset).
            let mut entries: Vec<(u16, u32)> = (0..cnt)
                .map(|i| {
                    let off = 2 + i * 6;
                    let k = u16::from_le_bytes([pabgh[off], pabgh[off + 1]]);
                    let o = u32::from_le_bytes([
                        pabgh[off + 2],
                        pabgh[off + 3],
                        pabgh[off + 4],
                        pabgh[off + 5],
                    ]);
                    (k, o)
                })
                .collect();
            // Sort by offset to compute end-of-row.
            let mut by_off = entries.clone();
            by_off.sort_by_key(|e| e.1);
            for (i, (k, o)) in entries.iter().enumerate() {
                let next_off = by_off
                    .iter()
                    .find(|(_, oo)| *oo > *o)
                    .map(|(_, oo)| *oo as usize)
                    .unwrap_or(pabgb.len());
                let start = *o as usize;
                let end = next_off.min(pabgb.len());
                let body = &pabgb[start..end];
                let _ = writeln!(
                    out,
                    "  [{i}] key={k}(0x{:04x}) @ offset={o}..{end} ({} B):",
                    k,
                    body.len()
                );
                let _ = writeln!(out, "      hex: {:02x?}", body);
                // Try to extract ASCII strings within.
                let mut s = String::new();
                let mut printable = String::new();
                for &b in body {
                    if (0x20..=0x7e).contains(&b) {
                        printable.push(b as char);
                    } else {
                        if printable.len() >= 3 {
                            s.push_str(&printable);
                            s.push_str(" | ");
                        }
                        printable.clear();
                    }
                }
                if printable.len() >= 3 {
                    s.push_str(&printable);
                }
                if !s.is_empty() {
                    let _ = writeln!(out, "      strs: {s}");
                }
            }
            // suppress unused-Vec warning in case the file is empty
            entries.clear();
        }
        std::fs::write(probe_dir.join("small_tables.txt"), &out).expect("write");
        eprintln!("wrote out/faction_probe/small_tables.txt");
    }

    /// Investigation probe: list every `*faction*` / `*ally*` / `*tribe*`
    /// file in 0008's PAMT so we can see what tables the engine ships
    /// for the faction-bridge work (FactionNodeKey, FactionRelationGroupKey,
    /// FactionSpawnDataKey).
    #[test]
    #[ignore = "investigation only — faction file discovery"]
    fn _scan_0008_faction_files() {
        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let pamt_path = game_root.join("0008").join("0.pamt");
        if !pamt_path.is_file() {
            eprintln!("skipping: no {}", pamt_path.display());
            return;
        }
        let pamt_bytes = std::fs::read(&pamt_path).expect("read 0.pamt");
        let pamt = crate::binary::pamt::PackMeta::parse(&pamt_bytes, None).expect("parse PAMT");
        let mut hits = 0usize;
        let mut out = String::new();
        for d in &pamt.directories {
            for f in &d.files {
                let lower = f.name.to_ascii_lowercase();
                if lower.contains("faction") || lower.contains("ally") || lower.contains("tribe") {
                    use std::fmt::Write;
                    let _ = writeln!(
                        out,
                        "{}/{}  ({}c / {}u)",
                        d.path, f.name, f.file.compressed_size, f.file.uncompressed_size
                    );
                    hits += 1;
                }
            }
        }
        let out_dir = std::path::PathBuf::from("out/faction_probe");
        std::fs::create_dir_all(&out_dir).ok();
        let out_path = out_dir.join("file_list.txt");
        std::fs::write(&out_path, &out).expect("write file_list");
        eprintln!("wrote {} hits to {}", hits, out_path.display());
    }

    /// Investigation probe — file discovery + schema sampling for the
    /// **StoreKey** / **MercenaryKey** / **`_itemKey → _partPrefabKey`**
    /// bridges (next-session workstream).
    ///
    /// What it does:
    /// 1. Lists every `store*` / `mercenary*` / `partprefab*` file in
    ///    0008's PAMT manifest (with compressed / uncompressed sizes).
    /// 2. For each known target stem (`storeinfo`, `mercenaryinfo`, and
    ///    every `partprefab*` table) extracts the `.pabgb` + `.pabgh`
    ///    pair (if present), tries the standard PABGH shape
    ///    (`u16 count + (u32 key, u32 offset)*`) and the small-key
    ///    variant (`u16 count + (u16 key, u32 offset)*`), and dumps the
    ///    first 96 bytes of the first row with a key + name_len + name
    ///    interpretation attempt.
    /// 3. Linkage probe for `_itemKey → _partPrefabKey`: parses iteminfo
    ///    once, collects every distinct `StringInfoKey` referenced by
    ///    `PrefabData.prefab_names[]`, then checks how many overlap with
    ///    the row keys in `partprefabdyeslotinfo.pabgh`. If the overlap
    ///    is high, the linkage is `iteminfo.prefab_data_list[N].prefab_names[0].0`
    ///    used directly as `PartPrefabKey`. If low, the bridge needs a
    ///    sibling table or the `partprefab*` row keys are hashes of the
    ///    prefab name string.
    ///
    /// All output goes to `out/store_mercenary_partprefab_probe/`.
    /// Skips cleanly when the game install isn't present.
    #[test]
    #[ignore = "investigation only — store/mercenary/partprefab schema + linkage probe"]
    fn _probe_store_mercenary_partprefab() {
        use crate::binary::pamt::PackMeta;
        use crate::binary::paz;
        use std::collections::{HashMap, HashSet};
        use std::fmt::Write;

        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let pamt_path = game_root.join("0008").join("0.pamt");
        let Ok(pamt_bytes) = std::fs::read(&pamt_path) else {
            eprintln!("skipping: no {}", pamt_path.display());
            return;
        };
        let pamt = PackMeta::parse(&pamt_bytes, None).expect("parse PAMT");
        let dir = pamt
            .directories
            .iter()
            .find(|d| d.path == "gamedata/binary__/client/bin")
            .expect("missing gamedata/binary__/client/bin dir in 0008 PAMT");

        let out_dir = std::path::PathBuf::from("out/store_mercenary_partprefab_probe");
        std::fs::create_dir_all(&out_dir).ok();

        // ── 1. File discovery ──────────────────────────────────────
        let mut file_list = String::new();
        let _ = writeln!(file_list, "# 0008 gamedata files matching store* / mercenary* / partprefab*");
        let mut partprefab_stems: Vec<String> = Vec::new();
        for d in &pamt.directories {
            for f in &d.files {
                let lower = f.name.to_ascii_lowercase();
                if lower.starts_with("store")
                    || lower.starts_with("mercenary")
                    || lower.starts_with("partprefab")
                {
                    let _ = writeln!(
                        file_list,
                        "  {}/{}  ({}c / {}u)",
                        d.path, f.name, f.file.compressed_size, f.file.uncompressed_size
                    );
                    if lower.starts_with("partprefab") && lower.ends_with(".pabgb") {
                        let stem = f.name[..f.name.len() - ".pabgb".len()].to_owned();
                        if !partprefab_stems.contains(&stem) {
                            partprefab_stems.push(stem);
                        }
                    }
                }
            }
        }
        std::fs::write(out_dir.join("file_list.txt"), &file_list).ok();
        eprintln!("{file_list}");

        // ── 2. Per-target schema probe ─────────────────────────────
        let mut named_targets = vec![
            "storeinfo".to_owned(),
            "mercenaryinfo".to_owned(),
        ];
        for stem in &partprefab_stems {
            if !named_targets.contains(stem) {
                named_targets.push(stem.clone());
            }
        }

        // We need keys later from partprefabdyeslotinfo for the linkage probe.
        let mut partprefab_dye_slot_keys: HashSet<u32> = HashSet::new();

        let mut schemas = String::new();
        for stem in &named_targets {
            let pabgb_name = format!("{stem}.pabgb");
            let pabgh_name = format!("{stem}.pabgh");
            let Some(pabgb_file) = dir.files.iter().find(|f| f.name == pabgb_name) else {
                let _ = writeln!(schemas, "\n=== {stem} ===  MISSING .pabgb");
                continue;
            };
            let pabgh_file = dir.files.iter().find(|f| f.name == pabgh_name);
            let pabgb_bytes = match paz::extract_file(
                &game_root.join("0008"),
                pabgb_file,
                "gamedata/binary__/client/bin",
                &pamt.header.encrypt_info.encrypt_info,
            ) {
                Ok(b) => b,
                Err(e) => {
                    let _ = writeln!(schemas, "\n=== {stem} ===  pabgb extract failed: {e}");
                    continue;
                }
            };
            std::fs::write(out_dir.join(&pabgb_name), &pabgb_bytes).ok();
            let pabgh_bytes = pabgh_file.and_then(|f| {
                paz::extract_file(
                    &game_root.join("0008"),
                    f,
                    "gamedata/binary__/client/bin",
                    &pamt.header.encrypt_info.encrypt_info,
                )
                .ok()
            });
            if let Some(b) = &pabgh_bytes {
                std::fs::write(out_dir.join(&pabgh_name), b).ok();
            }
            let _ = writeln!(
                schemas,
                "\n=== {stem} ===  pabgb={} B  pabgh={}",
                pabgb_bytes.len(),
                pabgh_bytes
                    .as_ref()
                    .map(|b| format!("{} B", b.len()))
                    .unwrap_or_else(|| "MISSING".into()),
            );

            // Try standard PABGH (u16 count + (u32 key, u32 offset)*).
            if let Some(pabgh) = pabgh_bytes.as_deref() {
                match crate::skill_info::parse_pabgh(pabgh) {
                    Ok(entries) => {
                        let _ = writeln!(
                            schemas,
                            "  standard (u32 key, u32 offset) PABGH OK — {} rows",
                            entries.len()
                        );
                        if stem == "partprefabdyeslotinfo" {
                            partprefab_dye_slot_keys.extend(entries.iter().map(|e| e.key));
                        }
                        for (i, e) in entries.iter().take(2).enumerate() {
                            let start = e.offset as usize;
                            let end = (start + 96).min(pabgb_bytes.len());
                            let row = &pabgb_bytes[start..end];
                            let _ = writeln!(
                                schemas,
                                "  row[{i}] key={} (0x{:08x}) @ offset={}..{} ({} B)",
                                e.key, e.key, start, end, row.len()
                            );
                            let _ = writeln!(schemas, "    hex: {:02x?}", row);
                            if row.len() >= 8 {
                                let body_key =
                                    u32::from_le_bytes([row[0], row[1], row[2], row[3]]);
                                let name_len =
                                    u32::from_le_bytes([row[4], row[5], row[6], row[7]])
                                        as usize;
                                let _ = writeln!(
                                    schemas,
                                    "    body_key={body_key} (matches PABGH? {}) name_len={name_len}",
                                    body_key == e.key
                                );
                                if (1..=128).contains(&name_len)
                                    && 8 + name_len <= row.len()
                                    && let Ok(s) =
                                        std::str::from_utf8(&row[8..8 + name_len])
                                {
                                    let _ = writeln!(schemas, "    name: {s:?}");
                                }
                            }
                        }
                    }
                    Err(e) => {
                        let _ = writeln!(
                            schemas,
                            "  standard PABGH parse FAILED ({e}); trying u16-key variant",
                        );
                        if pabgh.len() >= 2 {
                            let cnt = u16::from_le_bytes([pabgh[0], pabgh[1]]) as usize;
                            if pabgh.len() == 2 + cnt * 6 {
                                let _ = writeln!(
                                    schemas,
                                    "  custom (u16 key, u32 offset) PABGH OK — {cnt} rows"
                                );
                                for i in 0..cnt.min(2) {
                                    let off = 2 + i * 6;
                                    let key = u16::from_le_bytes([pabgh[off], pabgh[off + 1]]);
                                    let row_off = u32::from_le_bytes([
                                        pabgh[off + 2],
                                        pabgh[off + 3],
                                        pabgh[off + 4],
                                        pabgh[off + 5],
                                    ]) as usize;
                                    let end = (row_off + 96).min(pabgb_bytes.len());
                                    let row = &pabgb_bytes[row_off..end];
                                    let _ = writeln!(
                                        schemas,
                                        "  row[{i}] key={key} (0x{:04x}) @ offset={row_off}..{end}",
                                        key
                                    );
                                    let _ = writeln!(schemas, "    hex: {:02x?}", row);
                                }
                            } else {
                                let _ = writeln!(
                                    schemas,
                                    "  unrecognised PABGH shape; first 64 bytes: {:02x?}",
                                    &pabgh[..pabgh.len().min(64)]
                                );
                            }
                        }
                    }
                }
            } else {
                // No PABGH → maybe storeinfo is a self-describing flat
                // file (per references/store_info.hexpat). Dump the first
                // 256 bytes raw so we can see whether the leading shape
                // matches the hexpat: u16 store_key + u32 name_len + name.
                let n = pabgb_bytes.len().min(256);
                let _ = writeln!(
                    schemas,
                    "  no .pabgh — flat-file? first {n} bytes:",
                );
                let _ = writeln!(schemas, "    hex: {:02x?}", &pabgb_bytes[..n]);
                if pabgb_bytes.len() >= 6 {
                    let store_key = u16::from_le_bytes([pabgb_bytes[0], pabgb_bytes[1]]);
                    let name_len = u32::from_le_bytes([
                        pabgb_bytes[2],
                        pabgb_bytes[3],
                        pabgb_bytes[4],
                        pabgb_bytes[5],
                    ]) as usize;
                    let _ = writeln!(
                        schemas,
                        "    leading u16 key + u32 name_len => key=0x{store_key:04x} name_len={name_len}"
                    );
                    if (1..=64).contains(&name_len)
                        && 6 + name_len <= pabgb_bytes.len()
                        && let Ok(s) = std::str::from_utf8(&pabgb_bytes[6..6 + name_len])
                    {
                        let _ = writeln!(schemas, "    name: {s:?}");
                    }
                }
            }
        }
        std::fs::write(out_dir.join("schemas.txt"), &schemas).ok();
        eprintln!("wrote schemas.txt ({} B)", schemas.len());

        // ── 3. `_itemKey → _partPrefabKey` linkage probe ─────────────
        // Parse iteminfo, collect every distinct StringInfoKey referenced
        // by `prefab_data_list[].prefab_names[]`, and check what fraction
        // overlap with the partprefabdyeslotinfo.pabgh row keys.
        let mut linkage = String::new();
        let _ = writeln!(linkage, "# _itemKey → _partPrefabKey linkage probe");
        let _ = writeln!(
            linkage,
            "\npartprefabdyeslotinfo row keys collected: {}",
            partprefab_dye_slot_keys.len()
        );
        if let Some(iteminfo_file) = dir.files.iter().find(|f| f.name == "iteminfo.pabgb") {
            let iteminfo_bytes = paz::extract_file(
                &game_root.join("0008"),
                iteminfo_file,
                "gamedata/binary__/client/bin",
                &pamt.header.encrypt_info.encrypt_info,
            )
            .expect("extract iteminfo.pabgb");
            let mut offset = 0usize;
            let mut total_items = 0usize;
            let mut items_with_prefab = 0usize;
            let mut prefab_name_hashes: HashSet<u32> = HashSet::new();
            // (item_key, item_string_key) → list of (prefab_data_idx, prefab_name_hashes[])
            type SampleRow = (u32, String, Vec<(usize, Vec<u32>)>);
            let mut sample_rows: Vec<SampleRow> = Vec::new();
            let mut item_hashes_by_key: HashMap<u32, Vec<u32>> = HashMap::new();
            use crate::binary::BinaryRead;
            while offset < iteminfo_bytes.len() {
                let item =
                    crate::item_info::ItemInfo::read_from(&iteminfo_bytes, &mut offset)
                        .expect("parse iteminfo row");
                total_items += 1;
                let prefab_list = &item.prefab_data_list.items;
                if prefab_list.is_empty() {
                    continue;
                }
                let mut any_prefab = false;
                let mut per_pd: Vec<(usize, Vec<u32>)> = Vec::new();
                let mut flat_for_item: Vec<u32> = Vec::new();
                for (pd_idx, pd) in prefab_list.iter().enumerate() {
                    let hashes: Vec<u32> =
                        pd.prefab_names.items.iter().map(|k| k.0).collect();
                    if !hashes.is_empty() {
                        any_prefab = true;
                        for &h in &hashes {
                            prefab_name_hashes.insert(h);
                            flat_for_item.push(h);
                        }
                    }
                    per_pd.push((pd_idx, hashes));
                }
                if any_prefab {
                    items_with_prefab += 1;
                    item_hashes_by_key.insert(item.key.0, flat_for_item);
                    if sample_rows.len() < 8 {
                        sample_rows.push((
                            item.key.0,
                            item.string_key.data.to_owned(),
                            per_pd,
                        ));
                    }
                }
            }
            let intersect = prefab_name_hashes
                .iter()
                .filter(|h| partprefab_dye_slot_keys.contains(*h))
                .count();
            let _ = writeln!(
                linkage,
                "\niteminfo: parsed {total_items} items, {items_with_prefab} have non-empty prefab_data_list"
            );
            let _ = writeln!(
                linkage,
                "distinct prefab_name StringInfoKeys across iteminfo: {}",
                prefab_name_hashes.len()
            );
            let _ = writeln!(
                linkage,
                "intersection w/ partprefabdyeslotinfo row keys: {intersect}"
            );
            let coverage_pct = if !prefab_name_hashes.is_empty() {
                100.0 * intersect as f64 / prefab_name_hashes.len() as f64
            } else {
                0.0
            };
            let _ = writeln!(
                linkage,
                "  coverage: {coverage_pct:.1}% of iteminfo prefab hashes hit the dye-slot table"
            );
            let _ = writeln!(
                linkage,
                "\nFirst 8 items with prefab_data_list (key, string_key, per-PrefabData hashes):"
            );
            for (k, sk, per_pd) in sample_rows.iter() {
                let _ = writeln!(linkage, "\n  item_key={k} ({sk:?})");
                for (pd_idx, hashes) in per_pd {
                    let hits: Vec<bool> = hashes
                        .iter()
                        .map(|h| partprefab_dye_slot_keys.contains(h))
                        .collect();
                    let _ = writeln!(
                        linkage,
                        "    PrefabData[{pd_idx}]  prefab_names={:08x?}  dye_slot_hits={:?}",
                        hashes, hits
                    );
                }
            }

            // Also probe: how many distinct items map to AT LEAST ONE
            // hash that's a dye-slot key? That's the upper bound on how
            // many items the linkage can cover.
            let items_hitting_dye_slot = item_hashes_by_key
                .iter()
                .filter(|(_, hs)| hs.iter().any(|h| partprefab_dye_slot_keys.contains(h)))
                .count();
            let _ = writeln!(
                linkage,
                "\nitems with at least one prefab_name in partprefabdyeslotinfo: {items_hitting_dye_slot} / {items_with_prefab}"
            );
        } else {
            let _ = writeln!(linkage, "iteminfo.pabgb not in 0008 PAMT — skipping");
        }
        std::fs::write(out_dir.join("linkage.txt"), &linkage).ok();
        eprintln!("wrote linkage.txt ({} B)", linkage.len());
    }

    /// Investigation probe — deepen the `_itemKey → _partPrefabKey`
    /// linkage hunt now that the first probe ruled out the obvious
    /// `iteminfo.prefab_data_list[].prefab_names[].0` candidate (0%
    /// intersection across 5,377 items × 4,261 distinct hashes).
    ///
    /// Strategies tried here:
    /// 1. Cross-check partprefabdyeslotinfo row keys against
    ///    `stringinfo.pabgb` — are the row keys themselves StringInfoKey
    ///    hashes? If yes, resolving the key gives the prefab name, and
    ///    the linkage is "some itemkey-owned u32 == this hash".
    /// 2. Brute-force u32 scan of iteminfo's raw byte stream: for each
    ///    item's parsed byte span, count how many u32 windows hit a
    ///    partprefab row key. Reports counts both for dyeable items
    ///    (`is_dyeable=1`) and the whole population.
    /// 3. For the top dyeable-item hits, dump the local byte context so
    ///    we can spot which iteminfo field the u32 lives in.
    ///
    /// All output in `out/store_mercenary_partprefab_probe/linkage_v2.txt`.
    #[test]
    #[ignore = "investigation only — _itemKey → _partPrefabKey deeper linkage probe"]
    fn _probe_itemkey_partprefab_linkage() {
        use crate::binary::BinaryRead;
        use crate::binary::pamt::PackMeta;
        use crate::binary::paz;
        use std::collections::{HashMap, HashSet};
        use std::fmt::Write;

        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let pamt_path = game_root.join("0008").join("0.pamt");
        let Ok(pamt_bytes) = std::fs::read(&pamt_path) else {
            eprintln!("skipping: no {}", pamt_path.display());
            return;
        };
        let pamt = PackMeta::parse(&pamt_bytes, None).expect("parse PAMT");
        let dir = pamt
            .directories
            .iter()
            .find(|d| d.path == "gamedata/binary__/client/bin")
            .expect("missing gamedata/binary__/client/bin dir in 0008 PAMT");
        let group_dir = game_root.join("0008");
        let enc = &pamt.header.encrypt_info.encrypt_info;

        let out_dir = std::path::PathBuf::from("out/store_mercenary_partprefab_probe");
        std::fs::create_dir_all(&out_dir).ok();
        let mut out = String::new();

        // ── Load partprefabdyeslotinfo (key → prefab_name) ───────────
        let pp_pabgb = paz::extract_file(
            &group_dir,
            dir.files
                .iter()
                .find(|f| f.name == "partprefabdyeslotinfo.pabgb")
                .expect("partprefabdyeslotinfo.pabgb"),
            "gamedata/binary__/client/bin",
            enc,
        )
        .expect("extract partprefabdyeslotinfo.pabgb");
        let pp_pabgh = paz::extract_file(
            &group_dir,
            dir.files
                .iter()
                .find(|f| f.name == "partprefabdyeslotinfo.pabgh")
                .expect("partprefabdyeslotinfo.pabgh"),
            "gamedata/binary__/client/bin",
            enc,
        )
        .expect("extract partprefabdyeslotinfo.pabgh");
        let pp_entries =
            crate::part_prefab_dye_slot_info::parse_part_prefab_dye_slot_info_lossy(
                &pp_pabgb, &pp_pabgh,
            );
        let mut pp_name_by_key: HashMap<u32, String> = HashMap::with_capacity(pp_entries.len());
        for e in &pp_entries {
            pp_name_by_key.insert(e.key, e.prefab_name.clone());
        }
        let pp_keys: HashSet<u32> = pp_name_by_key.keys().copied().collect();
        let _ = writeln!(
            out,
            "partprefabdyeslotinfo: {} rows, sample (key → prefab_name):",
            pp_entries.len(),
        );
        for e in pp_entries.iter().take(5) {
            let _ = writeln!(out, "  0x{:08x} → {}", e.key, e.prefab_name);
        }

        // ── Strategy 1: are partprefab row keys also in stringinfo? ──
        let si_bytes = paz::extract_file(
            &group_dir,
            dir.files
                .iter()
                .find(|f| f.name == "stringinfo.pabgb")
                .expect("stringinfo.pabgb"),
            "gamedata/binary__/client/bin",
            enc,
        )
        .expect("extract stringinfo.pabgb");
        let si_entries = crate::string_info::StringInfoData::parse_pabgb(&si_bytes)
            .expect("parse stringinfo.pabgb");
        let mut si_by_hash: HashMap<u32, String> = HashMap::with_capacity(si_entries.len());
        for e in &si_entries {
            si_by_hash.entry(e.hash).or_insert_with(|| e.value.clone());
        }
        let _ = writeln!(out, "\nstringinfo.pabgb: {} entries", si_entries.len());
        let mut pp_in_si = 0usize;
        let mut pp_si_mismatch_name = 0usize;
        let mut sample_resolved: Vec<(u32, String, String)> = Vec::new();
        for e in &pp_entries {
            if let Some(si_val) = si_by_hash.get(&e.key) {
                pp_in_si += 1;
                if si_val != &e.prefab_name && sample_resolved.len() < 5 {
                    sample_resolved.push((e.key, e.prefab_name.clone(), si_val.clone()));
                    pp_si_mismatch_name += 1;
                }
            }
        }
        let _ = writeln!(
            out,
            "  partprefab keys present in stringinfo: {} / {} ({:.1}%)",
            pp_in_si,
            pp_entries.len(),
            100.0 * pp_in_si as f64 / pp_entries.len() as f64,
        );
        if pp_si_mismatch_name > 0 {
            let _ = writeln!(
                out,
                "  WARN: {pp_si_mismatch_name} partprefab rows have a stringinfo entry whose value disagrees with prefab_name"
            );
            for (k, pn, sv) in &sample_resolved {
                let _ = writeln!(
                    out,
                    "    0x{k:08x}: prefab_name={pn:?}  stringinfo_value={sv:?}"
                );
            }
        }

        // ── Strategy 2: brute-force u32 scan over iteminfo bytes ────
        let item_pabgb = paz::extract_file(
            &group_dir,
            dir.files
                .iter()
                .find(|f| f.name == "iteminfo.pabgb")
                .expect("iteminfo.pabgb"),
            "gamedata/binary__/client/bin",
            enc,
        )
        .expect("extract iteminfo.pabgb");
        let mut offset = 0usize;
        // (item_key, string_key, is_dyeable, row_start, row_end)
        let mut item_spans: Vec<(u32, String, bool, usize, usize)> = Vec::new();
        while offset < item_pabgb.len() {
            let start = offset;
            let item = crate::item_info::ItemInfo::read_from(&item_pabgb, &mut offset)
                .expect("parse iteminfo row");
            item_spans.push((
                item.key.0,
                item.string_key.data.to_owned(),
                item.is_dyeable != 0,
                start,
                offset,
            ));
        }
        let _ = writeln!(
            out,
            "\niteminfo: {} items parsed; {} marked is_dyeable=1",
            item_spans.len(),
            item_spans.iter().filter(|i| i.2).count(),
        );

        // For each item span, count u32 windows whose value is in
        // pp_keys. Also record offset-within-row to find the field.
        let mut per_item_hits: HashMap<u32, Vec<(usize, u32)>> = HashMap::new();
        for &(item_key, _, _, s, e) in &item_spans {
            let body = &item_pabgb[s..e];
            let mut hits = Vec::new();
            // Stride-1 scan (catches unaligned hits too).
            let mut i = 0;
            while i + 4 <= body.len() {
                let v = u32::from_le_bytes([body[i], body[i + 1], body[i + 2], body[i + 3]]);
                if pp_keys.contains(&v) {
                    hits.push((i, v));
                }
                i += 1;
            }
            if !hits.is_empty() {
                per_item_hits.insert(item_key, hits);
            }
        }
        let dyeable_with_hit = item_spans
            .iter()
            .filter(|i| i.2 && per_item_hits.contains_key(&i.0))
            .count();
        let all_with_hit = per_item_hits.len();
        let dyeable_total = item_spans.iter().filter(|i| i.2).count();
        let _ = writeln!(
            out,
            "\nbrute-force u32 scan (stride 1):"
        );
        let _ = writeln!(
            out,
            "  items with at least one partprefab-key hit:           {all_with_hit} / {} ({:.1}%)",
            item_spans.len(),
            100.0 * all_with_hit as f64 / item_spans.len() as f64,
        );
        let _ = writeln!(
            out,
            "  dyeable items with at least one partprefab-key hit:   {dyeable_with_hit} / {dyeable_total} ({:.1}%)",
            if dyeable_total > 0 {
                100.0 * dyeable_with_hit as f64 / dyeable_total as f64
            } else { 0.0 },
        );

        // Per-offset histogram (which offsets-within-row contain hits)
        let mut offset_hist: HashMap<usize, usize> = HashMap::new();
        let mut dyeable_offset_hist: HashMap<usize, usize> = HashMap::new();
        for &(item_key, _, dyeable, _, _) in &item_spans {
            if let Some(hits) = per_item_hits.get(&item_key) {
                for &(off, _v) in hits {
                    *offset_hist.entry(off).or_insert(0) += 1;
                    if dyeable {
                        *dyeable_offset_hist.entry(off).or_insert(0) += 1;
                    }
                }
            }
        }
        let mut hist_sorted: Vec<_> = dyeable_offset_hist.iter().collect();
        hist_sorted.sort_by(|a, b| b.1.cmp(a.1));
        let _ = writeln!(out, "\nTop 20 within-row offsets w/ partprefab-key hits across DYEABLE items:");
        for (off, cnt) in hist_sorted.iter().take(20) {
            let global = offset_hist.get(*off).copied().unwrap_or(0);
            let _ = writeln!(
                out,
                "  offset {off:>5} : {cnt} dyeable hits  ({global} total across all items)"
            );
        }

        // Dump 4-5 sample dyeable items with hits — print local
        // byte context around each hit, plus the resolved prefab_name.
        let _ = writeln!(out, "\nSample hits — 5 dyeable items:");
        let mut shown = 0usize;
        for (item_key, string_key, dyeable, s, _e) in &item_spans {
            if shown >= 5 {
                break;
            }
            if !*dyeable {
                continue;
            }
            let Some(hits) = per_item_hits.get(item_key) else {
                continue;
            };
            shown += 1;
            let _ = writeln!(out, "\n  item_key={item_key} ({string_key:?}, is_dyeable=1) hits={}", hits.len());
            for (off, v) in hits.iter().take(4) {
                let abs_lo = *s + off;
                let abs_hi = (abs_lo + 24).min(item_pabgb.len());
                let ctx_lo = abs_lo.saturating_sub(8);
                let ctx = &item_pabgb[ctx_lo..abs_hi];
                let pname = pp_name_by_key.get(v).map(|s| s.as_str()).unwrap_or("?");
                let _ = writeln!(
                    out,
                    "    +{off:5} = 0x{v:08x} ({pname})   ctx[-8..+24]: {:02x?}",
                    ctx
                );
            }
        }

        std::fs::write(out_dir.join("linkage_v2.txt"), &out).ok();
        eprintln!("wrote linkage_v2.txt ({} B)", out.len());
    }

    /// Investigation probe — broad scan across every PAMT group (0001..)
    /// looking for a sibling table that might house the
    /// `_itemKey → _partPrefabKey` linkage. Strategy v2 ruled out
    /// iteminfo (0% direct u32 overlap) and stringinfo (0% partprefab-key
    /// presence), so the linkage table likely lives elsewhere.
    ///
    /// Heuristics:
    /// 1. List every `.pabgb` filename across every 000N/PAMT, group by
    ///    name pattern, sort by size.
    /// 2. Highlight files matching any of: `dye`, `prefab`, `item`,
    ///    `appearance`, `equipment`, `equip`, `wear` — these are the
    ///    plausible homes for an `(item_key, part_prefab_key)` map.
    /// 3. For the top candidates (<200 KB), extract the file and check
    ///    whether ANY u32 window in the body matches a known
    ///    partprefabdyeslotinfo row key. If yes, that's our linkage
    ///    table — record the file + the hit count.
    #[test]
    #[ignore = "investigation only — partprefab linkage table file search across all PAMT groups"]
    fn _probe_partprefab_linkage_table_scan() {
        use crate::binary::pamt::PackMeta;
        use crate::binary::paz;
        use std::collections::HashSet;
        use std::fmt::Write;

        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });

        // First, gather the 1,105 partprefabdyeslotinfo keys from 0008.
        let pamt_0008 = match std::fs::read(game_root.join("0008").join("0.pamt")) {
            Ok(b) => b,
            Err(_) => {
                eprintln!("skipping: no 0008/0.pamt");
                return;
            }
        };
        let p8 = PackMeta::parse(&pamt_0008, None).expect("parse 0008 PAMT");
        let p8_bin = p8
            .directories
            .iter()
            .find(|d| d.path == "gamedata/binary__/client/bin")
            .expect("0008 bin dir");
        let pp_pabgb = paz::extract_file(
            &game_root.join("0008"),
            p8_bin
                .files
                .iter()
                .find(|f| f.name == "partprefabdyeslotinfo.pabgb")
                .expect("partprefabdyeslotinfo.pabgb"),
            "gamedata/binary__/client/bin",
            &p8.header.encrypt_info.encrypt_info,
        )
        .expect("extract partprefabdyeslotinfo.pabgb");
        let pp_pabgh = paz::extract_file(
            &game_root.join("0008"),
            p8_bin
                .files
                .iter()
                .find(|f| f.name == "partprefabdyeslotinfo.pabgh")
                .expect("partprefabdyeslotinfo.pabgh"),
            "gamedata/binary__/client/bin",
            &p8.header.encrypt_info.encrypt_info,
        )
        .expect("extract partprefabdyeslotinfo.pabgh");
        let pp_entries =
            crate::part_prefab_dye_slot_info::parse_part_prefab_dye_slot_info_lossy(
                &pp_pabgb, &pp_pabgh,
            );
        let pp_keys: HashSet<u32> = pp_entries.iter().map(|e| e.key).collect();
        eprintln!("collected {} partprefab keys to probe against", pp_keys.len());

        let out_dir = std::path::PathBuf::from("out/store_mercenary_partprefab_probe");
        std::fs::create_dir_all(&out_dir).ok();
        let mut out = String::new();
        let _ = writeln!(out, "# partprefab linkage-table search (across all PAMT groups)");
        let _ = writeln!(out, "partprefab keys to find: {}\n", pp_keys.len());

        // Walk every 000N folder under game_root.
        let mut groups: Vec<String> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&game_root) {
            for ent in rd.flatten() {
                if let Some(name) = ent.file_name().to_str()
                    && name.len() == 4
                    && name.chars().all(|c| c.is_ascii_digit())
                    && ent.path().join("0.pamt").is_file()
                {
                    groups.push(name.to_owned());
                }
            }
        }
        groups.sort();
        let _ = writeln!(out, "discovered PAMT groups: {:?}\n", groups);

        let pattern_keys = ["dye", "prefab", "item", "appear", "equip", "wear", "part"];

        // First pass: filename inventory, grouped by group.
        let _ = writeln!(out, "## Filename inventory across all groups (matches: {pattern_keys:?})");
        let mut candidates: Vec<(String, String, String, u32, u32)> = Vec::new(); // (group, dir, name, comp, uncomp)
        for g in &groups {
            let pamt_path = game_root.join(g).join("0.pamt");
            let Ok(bytes) = std::fs::read(&pamt_path) else { continue; };
            let Ok(pamt) = PackMeta::parse(&bytes, None) else {
                let _ = writeln!(out, "  {g}: PAMT parse failed");
                continue;
            };
            let _ = writeln!(out, "\n### {g} ({} dirs)", pamt.directories.len());
            for d in &pamt.directories {
                for f in &d.files {
                    let lower = f.name.to_ascii_lowercase();
                    if pattern_keys.iter().any(|p| lower.contains(p))
                        && (lower.ends_with(".pabgb") || lower.ends_with(".pabgh"))
                    {
                        let _ = writeln!(
                            out,
                            "  {}/{} ({}c / {}u)",
                            d.path, f.name, f.file.compressed_size, f.file.uncompressed_size
                        );
                        if lower.ends_with(".pabgb") {
                            candidates.push((
                                g.clone(),
                                d.path.clone(),
                                f.name.clone(),
                                f.file.compressed_size,
                                f.file.uncompressed_size,
                            ));
                        }
                    }
                }
            }
        }

        // Sort candidates by ascending uncompressed size — small files first,
        // they're cheap to probe and the linkage table is likely <500 KB
        // (1,105 rows × maybe 16 bytes/row ≈ 18 KB minimum).
        candidates.sort_by_key(|c| c.4);

        let _ = writeln!(
            out,
            "\n## Candidate scan ({} .pabgb candidates, smallest first)",
            candidates.len()
        );
        // Cap to candidates <= 5 MB to keep test time bounded.
        let cap = 5 * 1024 * 1024;
        for (g, dpath, fname, _csz, usz) in &candidates {
            if *usz as usize > cap {
                continue;
            }
            // Extract.
            let pamt_path = game_root.join(g).join("0.pamt");
            let Ok(bytes) = std::fs::read(&pamt_path) else { continue; };
            let Ok(pamt) = PackMeta::parse(&bytes, None) else { continue; };
            let Some(d) = pamt.directories.iter().find(|d| &d.path == dpath) else {
                continue;
            };
            let Some(f) = d.files.iter().find(|f| f.name == *fname) else {
                continue;
            };
            let Ok(blob) = paz::extract_file(
                &game_root.join(g),
                f,
                &d.path,
                &pamt.header.encrypt_info.encrypt_info,
            ) else {
                let _ = writeln!(out, "  {g}/{fname}: extract FAILED");
                continue;
            };
            // Stride-1 u32 scan.
            let mut hits = 0usize;
            let mut distinct: HashSet<u32> = HashSet::new();
            let mut i = 0;
            while i + 4 <= blob.len() {
                let v = u32::from_le_bytes([blob[i], blob[i + 1], blob[i + 2], blob[i + 3]]);
                if pp_keys.contains(&v) {
                    hits += 1;
                    distinct.insert(v);
                }
                i += 1;
            }
            if hits > 0 {
                let _ = writeln!(
                    out,
                    "  HIT  {g}/{fname} ({usz}B unc): {hits} u32 hits ({} distinct partprefab keys)",
                    distinct.len()
                );
            }
        }

        std::fs::write(out_dir.join("linkage_table_scan.txt"), &out).ok();
        eprintln!("wrote linkage_table_scan.txt ({} B)", out.len());
    }

    /// Investigation probe — last linkage hypothesis: iteminfo's
    /// `prefab_data_list[].prefab_names[]` StringInfoKeys resolve (via
    /// `stringinfo.pabgb`) to symbolic names, and partprefabdyeslotinfo's
    /// row's `prefab_name` field carries the SAME (or a derived) string.
    /// If the strings match directly, the linkage is:
    /// `iteminfo → prefab_names[] → stringinfo → string → match against
    ///  partprefab.prefab_name → row key`.
    ///
    /// If they DON'T match directly, dumps the symbolic names so we can
    /// see what the naming convention looks like (and whether a
    /// derivation rule like "prepend cd_ + lowercase" is in play).
    ///
    /// Also tests the Jenkins-hash hypothesis: for the first few
    /// partprefab row keys, brute-force Jenkins hashlittle2 of the
    /// row's `prefab_name` field with seed 0 / 0xDEBA1DCD / 1, see if
    /// any match the row key.
    #[test]
    #[ignore = "investigation only — string-resolution + Jenkins hash linkage check"]
    fn _probe_partprefab_string_linkage() {
        use crate::binary::pamt::PackMeta;
        use crate::binary::paz;
        use std::collections::{HashMap, HashSet};
        use std::fmt::Write;

        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let pamt_path = game_root.join("0008").join("0.pamt");
        let Ok(pamt_bytes) = std::fs::read(&pamt_path) else {
            eprintln!("skipping: no {}", pamt_path.display());
            return;
        };
        let pamt = PackMeta::parse(&pamt_bytes, None).expect("parse PAMT");
        let dir = pamt
            .directories
            .iter()
            .find(|d| d.path == "gamedata/binary__/client/bin")
            .expect("0008 bin dir");
        let enc = &pamt.header.encrypt_info.encrypt_info;
        let group_dir = game_root.join("0008");

        // ── Load tables ──
        let pp_pabgb = paz::extract_file(
            &group_dir,
            dir.files
                .iter()
                .find(|f| f.name == "partprefabdyeslotinfo.pabgb")
                .unwrap(),
            "gamedata/binary__/client/bin",
            enc,
        )
        .unwrap();
        let pp_pabgh = paz::extract_file(
            &group_dir,
            dir.files
                .iter()
                .find(|f| f.name == "partprefabdyeslotinfo.pabgh")
                .unwrap(),
            "gamedata/binary__/client/bin",
            enc,
        )
        .unwrap();
        let pp_entries =
            crate::part_prefab_dye_slot_info::parse_part_prefab_dye_slot_info_lossy(
                &pp_pabgb, &pp_pabgh,
            );
        let pp_name_set: HashSet<String> =
            pp_entries.iter().map(|e| e.prefab_name.clone()).collect();
        let pp_key_by_name: HashMap<String, u32> = pp_entries
            .iter()
            .map(|e| (e.prefab_name.clone(), e.key))
            .collect();
        let pp_keys: HashSet<u32> = pp_entries.iter().map(|e| e.key).collect();
        let si_bytes = paz::extract_file(
            &group_dir,
            dir.files
                .iter()
                .find(|f| f.name == "stringinfo.pabgb")
                .unwrap(),
            "gamedata/binary__/client/bin",
            enc,
        )
        .unwrap();
        let si_entries =
            crate::string_info::StringInfoData::parse_pabgb(&si_bytes).unwrap();
        let mut si: HashMap<u32, String> = HashMap::with_capacity(si_entries.len());
        for e in &si_entries {
            si.entry(e.hash).or_insert_with(|| e.value.clone());
        }

        let item_pabgb = paz::extract_file(
            &group_dir,
            dir.files
                .iter()
                .find(|f| f.name == "iteminfo.pabgb")
                .unwrap(),
            "gamedata/binary__/client/bin",
            enc,
        )
        .unwrap();

        let mut out = String::new();
        let _ = writeln!(out, "# partprefab string-linkage probe\n");
        let _ = writeln!(out, "partprefab rows: {}", pp_entries.len());
        let _ = writeln!(out, "stringinfo entries: {}", si.len());
        let _ = writeln!(
            out,
            "\nSample partprefab prefab_name strings:"
        );
        for e in pp_entries.iter().take(8) {
            let _ = writeln!(out, "  0x{:08x} → {}", e.key, e.prefab_name);
        }

        // ── Walk items, resolve prefab_names + log everything ──
        use crate::binary::BinaryRead;
        let mut offset = 0usize;
        let mut dyeable_items_processed = 0usize;
        let mut prefab_name_resolved_direct_hit = 0usize;
        let mut prefab_name_resolved_total = 0usize;
        let mut unresolved_hashes = 0usize;
        type DyeableResolution = (u32, String, Vec<(u32, Option<String>, bool)>);
        let mut sample_dyeable_resolutions: Vec<DyeableResolution> = Vec::new();
        // Also collect: does any resolved string appear as a SUBSTRING of
        // any partprefab prefab_name? (in case iteminfo stores a shorter
        // canonical name like "Phm_Lb_0054" and partprefab adds prefixes)
        let mut substring_hits = 0usize;
        // Also walk *every* stringinfo entry and check if its value
        // matches any partprefab prefab_name exactly. That tells us
        // whether the prefab_name strings even live in stringinfo.
        let mut si_full_string_matches: Vec<(u32, &String)> = si_entries
            .iter()
            .filter(|s| pp_name_set.contains(&s.value))
            .map(|s| (s.hash, &s.value))
            .collect();
        si_full_string_matches.sort_by_key(|(h, _)| *h);
        let _ = writeln!(
            out,
            "\nstringinfo entries whose value matches a partprefab prefab_name: {}",
            si_full_string_matches.len()
        );
        for (h, v) in si_full_string_matches.iter().take(10) {
            let pp_key = pp_key_by_name.get(*v).copied().unwrap_or(0);
            let _ = writeln!(
                out,
                "  stringinfo hash 0x{h:08x} → {v:?}  (partprefab key: 0x{pp_key:08x})"
            );
        }

        while offset < item_pabgb.len() {
            let item = crate::item_info::ItemInfo::read_from(&item_pabgb, &mut offset)
                .expect("parse iteminfo row");
            if item.is_dyeable == 0 {
                continue;
            }
            dyeable_items_processed += 1;
            let mut per_hash: Vec<(u32, Option<String>, bool)> = Vec::new();
            for pd in &item.prefab_data_list.items {
                for k in &pd.prefab_names.items {
                    prefab_name_resolved_total += 1;
                    let resolved = si.get(&k.0).cloned();
                    if resolved.is_none() {
                        unresolved_hashes += 1;
                    }
                    let direct = resolved
                        .as_ref()
                        .is_some_and(|s| pp_name_set.contains(s));
                    if direct {
                        prefab_name_resolved_direct_hit += 1;
                    }
                    if let Some(r) = &resolved {
                        let substr = pp_name_set.iter().any(|pn| pn.contains(r) || r.contains(pn));
                        if substr && !direct {
                            substring_hits += 1;
                        }
                    }
                    per_hash.push((k.0, resolved, direct));
                }
            }
            if sample_dyeable_resolutions.len() < 8 {
                sample_dyeable_resolutions.push((
                    item.key.0,
                    item.string_key.data.to_owned(),
                    per_hash,
                ));
            }
        }
        let _ = writeln!(
            out,
            "\ndyeable items: {dyeable_items_processed}"
        );
        let _ = writeln!(
            out,
            "prefab_names hashes (total across all dyeable items): {prefab_name_resolved_total}"
        );
        let _ = writeln!(
            out,
            "  resolved via stringinfo: {} ({:.1}%)",
            prefab_name_resolved_total - unresolved_hashes,
            100.0
                * (prefab_name_resolved_total - unresolved_hashes) as f64
                / prefab_name_resolved_total.max(1) as f64,
        );
        let _ = writeln!(
            out,
            "  direct match against partprefab prefab_name set: {prefab_name_resolved_direct_hit}"
        );
        let _ = writeln!(out, "  substring match (either direction): {substring_hits}");

        let _ = writeln!(out, "\nSample resolutions (first 8 dyeable items):");
        for (k, sk, hashes) in &sample_dyeable_resolutions {
            let _ = writeln!(out, "\n  item_key={k} ({sk:?})");
            for (h, r, direct) in hashes {
                let marker = if *direct { "✓" } else { " " };
                let resolved_dbg = r.as_deref().unwrap_or("<no stringinfo entry>");
                let _ = writeln!(
                    out,
                    "    {marker} hash 0x{h:08x} → {resolved_dbg:?}"
                );
            }
        }

        // ── calculate_checksum hypothesis test ──
        // Use the existing Jenkins checksum (seed = length + 0xDEBA1DCD)
        // to hash each partprefab prefab_name string and see if the
        // result matches the row key. Also try a few derived strings
        // (suffixes / .pac extension).
        use crate::crypto::checksum::calculate_checksum;
        let _ = writeln!(
            out,
            "\n## calculate_checksum hypothesis test (10 partprefab rows)"
        );
        let mut cs_hits = 0usize;
        for e in pp_entries.iter().take(10) {
            let hit = calculate_checksum(e.prefab_name.as_bytes());
            let match_flag = hit == e.key;
            if match_flag {
                cs_hits += 1;
            }
            let _ = writeln!(
                out,
                "  {:?}  cs=0x{:08x}  target=0x{:08x}  match={}",
                e.prefab_name, hit, e.key, match_flag
            );
        }
        let _ = writeln!(out, "  direct cs(prefab_name) hits: {cs_hits}/10");

        let _ = writeln!(
            out,
            "\n  Variant transformations for row[0] ({:?}, target 0x{:08x}):",
            pp_entries[0].prefab_name, pp_entries[0].key
        );
        let pn = &pp_entries[0].prefab_name;
        for variant in [
            pn.clone(),
            format!("{pn}.pac"),
            format!("{pn}.prefab"),
            format!("{pn}_00"),
            pn.to_uppercase(),
            pn.to_lowercase(),
            format!("character/model/{pn}.pac"),
        ] {
            let cs = calculate_checksum(variant.as_bytes());
            let _ = writeln!(
                out,
                "    {variant:?}  cs=0x{cs:08x}  match={}",
                cs == pp_entries[0].key
            );
        }

        // suppress unused warnings if all empty
        let _ = pp_keys;

        std::fs::write(out_dir_for("linkage_string_v3.txt"), &out).ok();
        eprintln!("wrote linkage_string_v3.txt ({} B)", out.len());

        fn out_dir_for(name: &str) -> std::path::PathBuf {
            let p = std::path::PathBuf::from("out/store_mercenary_partprefab_probe");
            std::fs::create_dir_all(&p).ok();
            p.join(name)
        }
    }

    /// Investigation probe — schema dump for the **niche bridge**
    /// candidates from the next-session list. Twelve table candidates:
    /// `characterappearanceindexinfo`, `gameadviceinfo`,
    /// `gameplayvariableinfo`, `itemgroupinfo`, `crafttoolinfo`,
    /// `royalsupply` / `royalsupplyinfo`, `globalgameevent` /
    /// `globalgameeventinfo`, `regioninfo`, `houseinfo`,
    /// `factionresearch*`, `equipslotinfo`, `reserveslot` /
    /// `reserveslotinfo`.
    ///
    /// For each candidate:
    /// 1. Locate the `.pabgb` + (optional) `.pabgh` in 0008's PAMT.
    /// 2. Try the standard `(u32 key, u32 offset)` PABGH parse.
    /// 3. Fall back to `(u16 key, u32 offset)` 6-byte or
    ///    `(u8 key, u32 offset)` 5-byte entries.
    /// 4. Dump first 2 rows' bytes + attempt to interpret as
    ///    `[key prefix][u32 name_len][name][...]`.
    ///
    /// For the PALOC-resolvable candidate (GameAdvice), also probes
    /// PALOC namespaces: for each row's (key, name) pair, scan
    /// `localizationstring_eng.paloc` at every plausible lo32 to find
    /// the chain — same pattern as the `_probe_faction_paloc_chains`
    /// pass.
    ///
    /// All output written to `out/niche_bridges_probe/`. Skips cleanly
    /// when the game install isn't present.
    #[test]
    #[ignore = "investigation only — niche-bridge candidate schema dump"]
    fn _probe_niche_bridge_candidates() {
        use crate::binary::pamt::PackMeta;
        use crate::binary::paz;
        use std::fmt::Write;

        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let pamt_path = game_root.join("0008").join("0.pamt");
        let Ok(pamt_bytes) = std::fs::read(&pamt_path) else {
            eprintln!("skipping: no {}", pamt_path.display());
            return;
        };
        let pamt = PackMeta::parse(&pamt_bytes, None).expect("parse PAMT");
        let dir = pamt
            .directories
            .iter()
            .find(|d| d.path == "gamedata/binary__/client/bin")
            .expect("0008 bin dir");
        let enc = &pamt.header.encrypt_info.encrypt_info;
        let group_dir = game_root.join("0008");

        let out_dir = std::path::PathBuf::from("out/niche_bridges_probe");
        std::fs::create_dir_all(&out_dir).ok();

        // First pass — file discovery: dump every .pabgb name matching
        // any of these patterns. Casts a wide net so we catch
        // alternate spellings (e.g. `royalsupply` vs `royalsupplyinfo`).
        let patterns: &[&str] = &[
            "characterappearance",
            "gameadvice",
            "gameplayvariable",
            "itemgroup",
            "crafttool",
            "royalsupply",
            "globalgameevent",
            "regioninfo",
            "houseinfo",
            "factionresearch",
            "equipslot",
            "reserveslot",
        ];
        let mut discovered: Vec<(String, u32, u32)> = Vec::new(); // (name, csz, usz)
        let mut inventory = String::new();
        let _ = writeln!(inventory, "# Niche-bridge candidate file discovery (0008)");
        for f in &dir.files {
            let lower = f.name.to_ascii_lowercase();
            if patterns.iter().any(|p| lower.contains(p))
                && (lower.ends_with(".pabgb") || lower.ends_with(".pabgh"))
            {
                let _ = writeln!(
                    inventory,
                    "  {} ({}c / {}u)",
                    f.name, f.file.compressed_size, f.file.uncompressed_size
                );
                if lower.ends_with(".pabgb") {
                    discovered.push((
                        f.name.clone(),
                        f.file.compressed_size,
                        f.file.uncompressed_size,
                    ));
                }
            }
        }
        std::fs::write(out_dir.join("file_inventory.txt"), &inventory).ok();
        eprintln!("file inventory written ({} pabgb hits)", discovered.len());
        eprint!("{inventory}");

        // Schema dump per candidate.
        let mut schemas = String::new();
        for (fname, _csz, usz) in &discovered {
            let stem = &fname[..fname.len() - ".pabgb".len()];
            let pabgh_name = format!("{stem}.pabgh");
            let pabgb_file = dir.files.iter().find(|f| &f.name == fname).unwrap();
            let pabgh_file = dir.files.iter().find(|f| f.name == pabgh_name);

            let pabgb = match paz::extract_file(
                &group_dir,
                pabgb_file,
                "gamedata/binary__/client/bin",
                enc,
            ) {
                Ok(b) => b,
                Err(e) => {
                    let _ = writeln!(schemas, "\n=== {stem} === pabgb extract FAILED: {e}");
                    continue;
                }
            };
            std::fs::write(out_dir.join(fname), &pabgb).ok();
            let pabgh = pabgh_file.and_then(|f| {
                paz::extract_file(
                    &group_dir,
                    f,
                    "gamedata/binary__/client/bin",
                    enc,
                )
                .ok()
            });
            if let Some(ref b) = pabgh {
                std::fs::write(out_dir.join(&pabgh_name), b).ok();
            }

            let _ = writeln!(
                schemas,
                "\n=== {stem} ===  pabgb={} B (unc≈{}B)  pabgh={}",
                pabgb.len(),
                usz,
                pabgh
                    .as_ref()
                    .map(|b| format!("{} B", b.len()))
                    .unwrap_or_else(|| "MISSING".into())
            );

            let Some(pabgh) = pabgh.as_deref() else {
                // No PABGH — flat-file scan. Dump first 128 bytes raw
                // and try to interpret as leading key+name structure.
                let n = pabgb.len().min(128);
                let _ = writeln!(schemas, "  no .pabgh — flat file");
                let _ = writeln!(schemas, "  first {n} bytes: {:02x?}", &pabgb[..n]);
                continue;
            };

            // Try standard (u32 key, u32 offset).
            let std_ok = crate::skill_info::parse_pabgh(pabgh);
            match std_ok {
                Ok(entries) => {
                    let _ = writeln!(
                        schemas,
                        "  STANDARD (u32 key, u32 offset)  {} rows",
                        entries.len()
                    );
                    dump_first_rows_u32_key(&mut schemas, &entries, &pabgb);
                }
                Err(_) => {
                    // Try (u16, u32) 6-byte.
                    if pabgh.len() >= 2 {
                        let cnt =
                            u16::from_le_bytes([pabgh[0], pabgh[1]]) as usize;
                        if pabgh.len() == 2 + cnt * 6 {
                            let _ = writeln!(
                                schemas,
                                "  CUSTOM (u16 key, u32 offset)  {cnt} rows"
                            );
                            dump_first_rows_u16_key(&mut schemas, pabgh, cnt, &pabgb);
                        } else if pabgh.len() == 2 + cnt * 5 {
                            let _ = writeln!(
                                schemas,
                                "  CUSTOM (u8 key, u32 offset)  {cnt} rows"
                            );
                            dump_first_rows_u8_key(&mut schemas, pabgh, cnt, &pabgb);
                        } else {
                            let _ = writeln!(
                                schemas,
                                "  UNKNOWN PABGH shape, count_u16={cnt}, pabgh.len={}; first 64: {:02x?}",
                                pabgh.len(),
                                &pabgh[..pabgh.len().min(64)]
                            );
                        }
                    }
                }
            }
        }
        std::fs::write(out_dir.join("schemas.txt"), &schemas).ok();
        eprintln!("schemas.txt written ({} B)", schemas.len());

        // Helpers — kept local to the probe to avoid polluting the
        // bridge namespace.
        fn dump_first_rows_u32_key(
            out: &mut String,
            entries: &[crate::skill_info::SkillIndexEntry],
            pabgb: &[u8],
        ) {
            for (i, e) in entries.iter().take(3).enumerate() {
                let start = e.offset as usize;
                let end = (start + 96).min(pabgb.len());
                let row = &pabgb[start..end];
                let _ = writeln!(
                    out,
                    "  row[{i}] key=0x{:08x} ({}) @ offset={}..{}",
                    e.key, e.key, start, end
                );
                let _ = writeln!(out, "    hex: {:02x?}", row);
                if row.len() >= 8 {
                    let body_key =
                        u32::from_le_bytes([row[0], row[1], row[2], row[3]]);
                    let name_len =
                        u32::from_le_bytes([row[4], row[5], row[6], row[7]])
                            as usize;
                    let _ = writeln!(
                        out,
                        "    body_key=0x{body_key:08x} ({body_key}) matches PABGH? {}",
                        body_key == e.key
                    );
                    let _ = writeln!(out, "    leading u32 name_len = {name_len}");
                    if (1..=128).contains(&name_len)
                        && 8 + name_len <= row.len()
                        && let Ok(s) = std::str::from_utf8(&row[8..8 + name_len])
                    {
                        let _ = writeln!(out, "    name: {s:?}");
                    }
                }
            }
        }
        fn dump_first_rows_u16_key(
            out: &mut String,
            pabgh: &[u8],
            count: usize,
            pabgb: &[u8],
        ) {
            for i in 0..count.min(3) {
                let off = 2 + i * 6;
                let key = u16::from_le_bytes([pabgh[off], pabgh[off + 1]]);
                let row_off = u32::from_le_bytes([
                    pabgh[off + 2],
                    pabgh[off + 3],
                    pabgh[off + 4],
                    pabgh[off + 5],
                ]) as usize;
                let end = (row_off + 96).min(pabgb.len());
                let row = &pabgb[row_off..end];
                let _ = writeln!(
                    out,
                    "  row[{i}] key={key} (0x{key:04x}) @ offset={row_off}..{end}"
                );
                let _ = writeln!(out, "    hex: {:02x?}", row);
                if row.len() >= 6 {
                    let body_key = u16::from_le_bytes([row[0], row[1]]);
                    let name_len =
                        u32::from_le_bytes([row[2], row[3], row[4], row[5]])
                            as usize;
                    let _ = writeln!(
                        out,
                        "    body_key=0x{body_key:04x} ({body_key}) matches PABGH? {}",
                        body_key == key
                    );
                    let _ = writeln!(out, "    leading u32 name_len = {name_len}");
                    if (1..=128).contains(&name_len)
                        && 6 + name_len <= row.len()
                        && let Ok(s) = std::str::from_utf8(&row[6..6 + name_len])
                    {
                        let _ = writeln!(out, "    name: {s:?}");
                    }
                }
            }
        }
        fn dump_first_rows_u8_key(
            out: &mut String,
            pabgh: &[u8],
            count: usize,
            pabgb: &[u8],
        ) {
            for i in 0..count.min(3) {
                let off = 2 + i * 5;
                let key = pabgh[off];
                let row_off = u32::from_le_bytes([
                    pabgh[off + 1],
                    pabgh[off + 2],
                    pabgh[off + 3],
                    pabgh[off + 4],
                ]) as usize;
                let end = (row_off + 96).min(pabgb.len());
                let row = &pabgb[row_off..end];
                let _ = writeln!(
                    out,
                    "  row[{i}] key={key} (0x{key:02x}) @ offset={row_off}..{end}"
                );
                let _ = writeln!(out, "    hex: {:02x?}", row);
                if row.len() >= 5 {
                    let body_key = row[0];
                    let name_len =
                        u32::from_le_bytes([row[1], row[2], row[3], row[4]])
                            as usize;
                    let _ = writeln!(
                        out,
                        "    body_key={body_key} (0x{body_key:02x}) matches PABGH? {}",
                        body_key == key
                    );
                    let _ = writeln!(out, "    leading u32 name_len = {name_len}");
                    if (1..=128).contains(&name_len)
                        && 5 + name_len <= row.len()
                        && let Ok(s) = std::str::from_utf8(&row[5..5 + name_len])
                    {
                        let _ = writeln!(out, "    name: {s:?}");
                    }
                }
            }
        }
    }

    /// Investigation probe — survey every `(type_name, meta_size, meta_kind)`
    /// triple in a live save's decoded blocks, focusing on composite
    /// scalar fields (meta_kind ∈ {0, 2} with `meta_size` not in
    /// {1, 2, 4, 8}) that currently fall through `scalar_from_bytes`
    /// to `ScalarValue::Bytes`.
    ///
    /// Output (sorted by occurrence count) goes to
    /// `out/composite_scalar_survey/summary.txt`. Driver for the
    /// "extend `ScalarValue` with typed composite variants" work — the
    /// decision of which `F32x{2,3,4}` / color / quat variants to add
    /// is informed by what this probe actually finds in the wild.
    #[test]
    #[ignore = "investigation only — composite-scalar type survey"]
    fn _probe_save_composite_types() {
        use crate::save::{Body, FieldValue, ObjectBlock, Save, ScalarValue};
        use std::collections::HashMap;
        use std::fmt::Write;

        let save_path = std::env::var_os("CRIMSON_LIVE_SAVE")
            .map(PathBuf::from)
            .or_else(|| {
                let appdata = std::env::var_os("LOCALAPPDATA")?;
                let root = PathBuf::from(appdata)
                    .join("Pearl Abyss")
                    .join("CD")
                    .join("save");
                std::fs::read_dir(&root).ok()?.flatten().find_map(|entry| {
                    let p = entry.path().join("slot0").join("save.save");
                    p.is_file().then_some(p)
                })
            });
        let Some(save_path) = save_path else {
            eprintln!("skipping: no live save");
            return;
        };
        eprintln!("save: {}", save_path.display());

        let raw = std::fs::read(&save_path).expect("read save");
        let save = Save::parse(&raw).expect("parse save");
        let body = Body::parse(&save.body).expect("parse body");
        let blocks = body.decode_blocks(&save.body);

        // Aggregation key: (type_name, meta_size, meta_kind, decoded_kind).
        // Value: count + first sample (class_name, field_name, raw bytes).
        type Key = (String, u16, u16, &'static str);
        #[allow(dead_code)]
        struct Agg {
            count: usize,
            sample_class: String,
            sample_field: String,
            sample_present: bool,
            sample_value_kind: &'static str,
            sample_bytes_len: usize,
        }
        let mut agg: HashMap<Key, Agg> = HashMap::new();

        fn walk_block(b: &ObjectBlock, agg: &mut HashMap<Key, Agg>) {
            for f in &b.fields {
                let decoded_kind = f.kind.as_str();
                let value_kind: &'static str = match &f.value {
                    FieldValue::None => "none",
                    FieldValue::Scalar(s) => match s {
                        ScalarValue::Bool(_) => "Bool",
                        ScalarValue::U8(_) => "U8",
                        ScalarValue::U16(_) => "U16",
                        ScalarValue::U32(_) => "U32",
                        ScalarValue::U64(_) => "U64",
                        ScalarValue::I8(_) => "I8",
                        ScalarValue::I16(_) => "I16",
                        ScalarValue::I32(_) => "I32",
                        ScalarValue::I64(_) => "I64",
                        ScalarValue::F32(_) => "F32",
                        ScalarValue::F64(_) => "F64",
                        ScalarValue::F32x3(_) => "F32x3",
                        ScalarValue::F32x4(_) => "F32x4",
                        ScalarValue::U32x4(_) => "U32x4",
                        ScalarValue::Bytes(_) => "Bytes",
                    },
                    FieldValue::InlineBytes { .. } => "InlineBytes",
                    FieldValue::DynamicArray { .. } => "DynamicArray",
                    FieldValue::Locator { .. } => "Locator",
                    FieldValue::ObjectList { .. } => "ObjectList",
                };
                let bytes_len = match &f.value {
                    FieldValue::Scalar(ScalarValue::Bytes(b)) => b.len(),
                    _ => 0,
                };
                let key: Key = (f.type_name.clone(), f.meta_size, f.meta_kind, decoded_kind);
                let entry = agg.entry(key).or_insert_with(|| Agg {
                    count: 0,
                    sample_class: b.class_name.clone(),
                    sample_field: f.name.clone(),
                    sample_present: f.present,
                    sample_value_kind: value_kind,
                    sample_bytes_len: bytes_len,
                });
                entry.count += 1;
                // Recurse into nested elements.
                match &f.value {
                    FieldValue::ObjectList { elements, .. } => {
                        for e in elements {
                            walk_block(e, agg);
                        }
                    }
                    FieldValue::Locator { child: Some(c), .. } => walk_block(c, agg),
                    _ => {}
                }
            }
        }

        for b in &blocks {
            walk_block(b, &mut agg);
        }

        // Build summary, sorted by count descending.
        let mut rows: Vec<(Key, Agg)> = agg.into_iter().collect();
        rows.sort_by_key(|r| std::cmp::Reverse(r.1.count));

        let mut out = String::new();
        let _ = writeln!(
            out,
            "# Save composite-scalar survey — every (type_name, meta_size, meta_kind, decoded_kind) seen"
        );
        let _ = writeln!(
            out,
            "# decoded_kind = FieldKind::as_str() (fixed_prefix/inline_bytes/dynamic_array/...)"
        );
        let _ = writeln!(out, "# Sorted by occurrence count desc. ⚠ marks composite scalars (size ∉ {{1,2,4,8}} with meta_kind 0/2) that currently fall through to ScalarValue::Bytes\n");
        let _ = writeln!(
            out,
            "{:<32} {:>4} {:>2} {:<16} {:>9} {:>15} {:<36} value_kind",
            "type_name", "size", "mk", "decoded_kind", "count", "bytes_len", "sample"
        );
        let _ = writeln!(out, "{}", "-".repeat(140));
        for ((tn, size, mk, dk), a) in &rows {
            let composite_mark = if (*mk == 0 || *mk == 2)
                && !matches!(*size, 1 | 2 | 4 | 8)
            {
                "⚠"
            } else {
                " "
            };
            let sample = format!("{}::{}", a.sample_class, a.sample_field);
            let _ = writeln!(
                out,
                "{composite_mark} {tn:<30} {size:>4} {mk:>2} {dk:<16} {:>9} {:>15} {sample:<36} {}",
                a.count, a.sample_bytes_len, a.sample_value_kind,
            );
        }

        // Targeted summary: just the composite-scalar rows.
        let _ = writeln!(
            out,
            "\n## Composite-scalar candidates (meta_kind 0/2, size ∉ {{1,2,4,8}})"
        );
        for ((tn, size, mk, _dk), a) in &rows {
            if (*mk == 0 || *mk == 2) && !matches!(*size, 1 | 2 | 4 | 8) {
                let _ = writeln!(
                    out,
                    "  {tn:<32}  size={size:<3}  meta_kind={mk}  occurrences={}  sample={}::{}",
                    a.count, a.sample_class, a.sample_field
                );
            }
        }

        let out_dir = std::path::PathBuf::from("out/composite_scalar_survey");
        std::fs::create_dir_all(&out_dir).ok();
        std::fs::write(out_dir.join("summary.txt"), &out).expect("write summary");
        eprintln!("wrote out/composite_scalar_survey/summary.txt ({} B)", out.len());
    }
}
