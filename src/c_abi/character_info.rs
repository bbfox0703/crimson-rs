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

}
