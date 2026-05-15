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
}
