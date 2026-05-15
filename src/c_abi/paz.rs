//! PAZ archive extraction — C ABI surface.
//!
//! Wraps [`crate::binary::paz::extract_file`] so native callers can
//! pull a single file out of a Crimson Desert PAZ archive group
//! without going through Python. One-shot, stateless: each call
//! parses the PAMT, locates the requested file by directory + name,
//! decrypts (ChaCha20 when applicable), decompresses (LZ4 / zlib /
//! none), and writes the bytes back through the standard two-call
//! "query size, then fill buffer" pattern.
//!
//! Use case driving this: the C# editor wants to load PALOC
//! localization tables at startup. PALOC files in a Steam install
//! live at `<group>/<group>.paz` and are referenced by the group's
//! `0.pamt`. Once extracted, the bytes get fed straight into
//! [`super::paloc::crimson_paloc_load_from_bytes`].
//!
//! Partial-compression entries (`is_partial`) ARE supported: the bulk
//! of `0012/ui/texture/icon/` lives in that layout (header(128) +
//! LZ4-with-prefix-dict, or identity when LZ4 declined). Some other
//! 1.06 subtrees (notably `0012/ui/texture/image/worldmap/` SDFs and
//! large mesh assets in 0009/0015) use an additional chunked variant
//! the decoder doesn't yet understand — those still surface as
//! `BODY_PARSE`, the same code as any other PAZ extraction failure.
//!
//! Not exposed here (future PR if needed):
//! - Full PAMT enumeration (list every directory / file without
//!   extracting). [`crimson_paz_list_npc_portraits`] is a narrow
//!   special case covering one well-known asset class.
//! - Batch extraction (avoid re-parsing PAMT N times).

use std::ffi::CStr;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use super::error;
use crate::binary::pamt::PackMeta;
use crate::binary::paz;

/// NPC-portrait filename prefixes Pearl Abyss has shipped across 1.05 /
/// 1.06 / 1.07. Each entry maps a (lowercase, ASCII) filename prefix to
/// a one-line note explaining what that bucket contains — useful when
/// reading the source, useless at runtime (the note is dropped).
///
/// The split reflects the actual game-side taxonomy, surfaced by
/// scanning every PAMT in a 1.07 install:
///
/// - `cd_portraitimage_character_` — 1.05 / 1.06 named-NPC bucket.
///   Gone in 1.07; kept here for cross-version compat.
/// - `cd_portraitimage_chracter_` — 1.07 named-NPC bucket. The typo
///   ("chracter") is shipped — Demian / Kliff / Oongka all live under
///   it. Keep both spellings.
/// - `cd_portraitimage_nhm_` — Hernandese NPCs (soldiers, sheriffs,
///   wandering mercenaries, tournament champions). 62 entries in 1.07.
/// - `cd_portraitimage_nom_` — Omeyan NPCs (Tomaso soldiers,
///   mercenaries). 10 entries in 1.07.
/// - `cd_portraitimage_muscan_` — One-off `muscan_boss` portrait.
/// - `cd_mercenary_portrait_` — Recruitable mercenary companions
///   (NPCs the player can hire). 76 entries in 1.07.
///
/// Explicitly excluded — these are "portrait-like" but NOT NPC head-
/// shots, so they don't belong in this list: `cd_portraitimage_animal_`,
/// `cd_portraitimage_riding_`, `cd_portrait_petimage_`, `cd_portrait_wagon_`,
/// `cd_image_portrait_` (XML metadata), `cd_knowledgeimage_*`.
const NPC_PORTRAIT_PREFIXES: &[&str] = &[
    "cd_portraitimage_character_",
    "cd_portraitimage_chracter_",
    "cd_portraitimage_nhm_",
    "cd_portraitimage_nom_",
    "cd_portraitimage_muscan_",
    "cd_mercenary_portrait_",
];

/// Filter predicate for NPC portrait DDS entries.
///
/// Match rules: filename ends with `.dds` (case-insensitive) AND
/// begins with one of [`NPC_PORTRAIT_PREFIXES`] (case-insensitive).
/// The token between prefix and suffix must be non-empty — a bare
/// `<prefix>.dds` wouldn't resolve to anything useful and is rejected.
///
/// ASCII-only compares throughout — no UTF-8 lowercasing alloc.
/// Separable from the PAMT-scanning C ABI so the filter logic can
/// be unit-tested without a live game install.
fn is_npc_portrait_name(name: &str) -> bool {
    const SUFFIX: &str = ".dds";
    if !name
        .get(name.len().saturating_sub(SUFFIX.len())..)
        .is_some_and(|s| s.eq_ignore_ascii_case(SUFFIX))
    {
        return false;
    }
    for prefix in NPC_PORTRAIT_PREFIXES {
        // Require a non-empty token: name strictly longer than prefix + suffix.
        if name.len() <= prefix.len() + SUFFIX.len() {
            continue;
        }
        if name
            .get(..prefix.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(prefix))
        {
            return true;
        }
    }
    false
}

/// Extract a single file from a Crimson Desert pack group.
///
/// `pamt_path` is the absolute path to `0.pamt` inside a group folder
/// (e.g. `D:\\…\\Crimson Desert\\0020\\0.pamt`). The `.paz` chunks are
/// expected to sit alongside it in the same parent directory.
/// `directory` is the in-archive directory path
/// (e.g. `gamedata/stringtable/binary__`); `file_name` is the leaf
/// (e.g. `localizationstring_eng.paloc`). All three are NUL-terminated
/// UTF-8 strings.
///
/// Two-call shape:
/// - First call with `out_buf = null, out_buf_len = 0` returns
///   `BUFFER_TOO_SMALL` and sets `*out_required` to the uncompressed
///   file size.
/// - Allocate, call again with that buffer to receive the bytes and
///   `OK`. `*out_required` is set to the bytes written.
///
/// Returns:
/// - `OK` on successful extraction.
/// - `BUFFER_TOO_SMALL` when `out_buf_len < required`. `*out_required`
///   carries the needed size.
/// - `NOT_FOUND` when `directory` or `file_name` isn't in the PAMT.
/// - `IO` when the PAMT or `.paz` chunk can't be read from disk.
/// - `BODY_PARSE` when the PAMT bytes don't parse, or extraction
///   fails downstream (bad checksum, unsupported crypto, partial-
///   compression file, decompression error).
/// - `NULL_ARG` on any null pointer; `INVALID_PATH` on bad UTF-8.
///
/// # Safety
/// All four string pointers must be non-null and NUL-terminated UTF-8.
/// `out_required` must point at writable memory; `out_buf` may be null
/// iff `out_buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_paz_extract_file(
    pamt_path: *const c_char,
    directory: *const c_char,
    file_name: *const c_char,
    out_buf: *mut u8,
    out_buf_len: usize,
    out_required: *mut usize,
) -> i32 {
    if pamt_path.is_null()
        || directory.is_null()
        || file_name.is_null()
        || out_required.is_null()
    {
        return error::NULL_ARG;
    }
    if out_buf.is_null() && out_buf_len != 0 {
        return error::NULL_ARG;
    }
    unsafe { *out_required = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        // ── String inputs ───────────────────────────────────────────────
        let pamt_str = match unsafe { CStr::from_ptr(pamt_path) }.to_str() {
            Ok(s) => s,
            Err(_) => return error::INVALID_PATH,
        };
        let dir_str = match unsafe { CStr::from_ptr(directory) }.to_str() {
            Ok(s) => s,
            Err(_) => return error::INVALID_PATH,
        };
        let name_str = match unsafe { CStr::from_ptr(file_name) }.to_str() {
            Ok(s) => s,
            Err(_) => return error::INVALID_PATH,
        };

        // ── Load + parse the PAMT ──────────────────────────────────────
        let pamt_bytes = match std::fs::read(pamt_str) {
            Ok(b) => b,
            Err(_) => return error::IO,
        };
        let pamt = match PackMeta::parse(&pamt_bytes, None) {
            Ok(p) => p,
            Err(_) => return error::BODY_PARSE,
        };

        // ── Resolve directory + file ───────────────────────────────────
        let Some(dir) = pamt.directories.iter().find(|d| d.path == dir_str) else {
            return error::NOT_FOUND;
        };
        let Some(file) = dir.files.iter().find(|f| f.name == name_str) else {
            return error::NOT_FOUND;
        };

        // The group directory hosts the .paz chunks; resolve it from the
        // PAMT file's parent.
        let group_dir = match Path::new(pamt_str).parent() {
            Some(p) => p.to_path_buf(),
            None => return error::INVALID_PATH,
        };

        // ── Extract (decrypt + decompress) ─────────────────────────────
        let extracted = match paz::extract_file(
            &group_dir,
            file,
            dir_str,
            &pamt.header.encrypt_info.encrypt_info,
        ) {
            Ok(b) => b,
            // Both io::Error variants from extract_file mean "couldn't
            // get the bytes" — surface as BODY_PARSE for now (the
            // user-visible difference between "missing crypto support"
            // and "bad checksum" isn't actionable at the editor level).
            Err(_) => return error::BODY_PARSE,
        };

        let needed = extracted.len();
        unsafe { *out_required = needed };
        if out_buf_len < needed {
            return error::BUFFER_TOO_SMALL;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(extracted.as_ptr(), out_buf, needed);
        }
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// List every NPC portrait DDS file in a PAZ group's PAMT.
///
/// Background: Crimson Desert ships per-NPC portrait textures as DDS
/// files under `ui/texture/image/portraitimage/` (named-story NPCs,
/// anonymous Hernandese / Omeyan NPCs, boss portraits) plus
/// `ui/texture/image/portraitimage/` and adjacent folders for
/// recruitable mercenaries. Pearl Abyss reorganised this naming
/// non-trivially across patches — see [`NPC_PORTRAIT_PREFIXES`] for
/// the full per-bucket breakdown the filter accepts.
///
/// `pamt_path` is the absolute path to `0.pamt` inside a group folder
/// (in 1.06 / 1.07 the portraits live under `0012/`). The function
/// filters by filename — not directory — so it tolerates Pearl Abyss
/// moving paths around in future patches. NUL-terminated UTF-8.
///
/// Non-NPC "portrait" assets — animal / riding / pet / wagon images,
/// knowledge thumbnails — are filtered out. See the prefix table for
/// the full exclusion list.
///
/// Output format: `<directory>/<filename>` strings concatenated
/// back-to-back, each followed by a single NUL byte. The caller walks
/// the buffer by stepping over each substring's terminating NUL until
/// `*out_required` bytes have been consumed. `*out_count` reports the
/// number of entries regardless of buffer size, so the caller can
/// pre-allocate a `Vec<String>` (or equivalent) before parsing.
///
/// Two-call shape (same as [`crimson_paz_extract_file`]):
/// - First call with `out_buf = null, out_buf_len = 0` returns
///   `BUFFER_TOO_SMALL` (or `OK` if the table has zero matches) and
///   populates `*out_required` and `*out_count`.
/// - Allocate `*out_required` bytes, call again. Returns `OK` on
///   success; `*out_required` is the bytes written (== total list size).
///
/// Returns:
/// - `OK` on success. When the PAMT contains zero portraits, both
///   `*out_required` and `*out_count` are 0 and the function returns
///   `OK` immediately (no `BUFFER_TOO_SMALL` round-trip needed).
/// - `BUFFER_TOO_SMALL` when there are matches but `out_buf_len <
///   *out_required`. The caller should re-allocate and retry.
/// - `IO` when the PAMT file can't be read from disk.
/// - `BODY_PARSE` when the PAMT bytes don't parse.
/// - `NULL_ARG` on any null pointer (including null `out_buf` with
///   non-zero `out_buf_len`); `INVALID_PATH` on bad UTF-8.
///
/// Note: this is a one-shot scan — each call reparses the PAMT. For
/// 0012's 751 KB PAMT that's fast (a few ms) but not free. Callers
/// that need repeated access should cache the returned list themselves.
///
/// # Safety
/// `pamt_path` must be non-null and NUL-terminated UTF-8.
/// `out_required` and `out_count` must point at writable memory.
/// `out_buf` may be null iff `out_buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_paz_list_npc_portraits(
    pamt_path: *const c_char,
    out_buf: *mut u8,
    out_buf_len: usize,
    out_required: *mut usize,
    out_count: *mut u32,
) -> i32 {
    if pamt_path.is_null() || out_required.is_null() || out_count.is_null() {
        return error::NULL_ARG;
    }
    if out_buf.is_null() && out_buf_len != 0 {
        return error::NULL_ARG;
    }
    unsafe {
        *out_required = 0;
        *out_count = 0;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let pamt_str = match unsafe { CStr::from_ptr(pamt_path) }.to_str() {
            Ok(s) => s,
            Err(_) => return error::INVALID_PATH,
        };

        let pamt_bytes = match std::fs::read(pamt_str) {
            Ok(b) => b,
            Err(_) => return error::IO,
        };
        let pamt = match PackMeta::parse(&pamt_bytes, None) {
            Ok(p) => p,
            Err(_) => return error::BODY_PARSE,
        };

        // Single pass: collect "<dir>/<name>" paths and tally bytes
        // needed. We materialise the Strings up front rather than
        // emitting straight into the caller's buffer because we need
        // to populate `*out_required` even when the buffer is too
        // small (or null on the sizing call).
        let mut entries: Vec<String> = Vec::new();
        let mut serialised_bytes: usize = 0;
        for dir in &pamt.directories {
            for f in &dir.files {
                if !is_npc_portrait_name(&f.name) {
                    continue;
                }
                // Empty directory path (root) shouldn't happen for
                // PAZ-packed assets, but if it ever does, omit the
                // leading slash so we don't emit "/name".
                let path = if dir.path.is_empty() {
                    f.name.clone()
                } else {
                    format!("{}/{}", dir.path, f.name)
                };
                serialised_bytes += path.len() + 1; // trailing NUL
                entries.push(path);
            }
        }

        unsafe {
            *out_count = entries.len() as u32;
            *out_required = serialised_bytes;
        }

        // Zero-match fast path: nothing to copy, signal OK directly
        // so a caller probing an empty group doesn't have to do the
        // BUFFER_TOO_SMALL round-trip.
        if serialised_bytes == 0 {
            return error::OK;
        }
        if out_buf_len < serialised_bytes {
            return error::BUFFER_TOO_SMALL;
        }

        // Fill the caller's buffer.
        let mut cursor: usize = 0;
        for path in &entries {
            unsafe {
                std::ptr::copy_nonoverlapping(path.as_ptr(), out_buf.add(cursor), path.len());
                *out_buf.add(cursor + path.len()) = 0;
            }
            cursor += path.len() + 1;
        }
        debug_assert_eq!(cursor, serialised_bytes);
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

#[cfg(test)]
mod tests {
    //! Live-install integration tests. Skip cleanly when the Steam
    //! install isn't present (same pattern as `test_paloc_parse` in
    //! lib.rs).

    use super::*;
    use std::ffi::CString;
    use std::path::PathBuf;
    use std::ptr;

    fn find_pamt() -> Option<PathBuf> {
        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        // Group 0020 hosts the English PALOC in 1.06.
        let p = game_root.join("0020").join("0.pamt");
        p.is_file().then_some(p)
    }

    /// Two-call extraction helper — returns the file's bytes (sans
    /// trailing buffer slack).
    fn extract_via_abi(pamt: &CStr, dir: &CStr, name: &CStr) -> Result<Vec<u8>, i32> {
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
        if rc == error::NOT_FOUND
            || rc == error::IO
            || rc == error::BODY_PARSE
        {
            return Err(rc);
        }
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
        if rc != error::OK {
            return Err(rc);
        }
        buf.truncate(needed);
        Ok(buf)
    }

    #[test]
    fn c_abi_paz_extract_eng_paloc() {
        let Some(pamt_path) = find_pamt() else {
            eprintln!("skipping c_abi_paz_extract_eng_paloc: no 0020/0.pamt in game install");
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let dir = CString::new("gamedata/stringtable/binary__").unwrap();
        let name = CString::new("localizationstring_eng.paloc").unwrap();

        let bytes = extract_via_abi(&pamt, &dir, &name)
            .expect("English PALOC must extract cleanly from 0020/0.pamt");

        // Sanity: the extracted bytes should parse as a real PALOC.
        // Use the C ABI loader to prove the extract → load round-trip
        // works without going through any Rust-only API.
        let mut handle: *mut super::super::paloc::CrimsonPalocHandle = ptr::null_mut();
        let rc = unsafe {
            super::super::paloc::crimson_paloc_load_from_bytes(
                bytes.as_ptr(),
                bytes.len(),
                &mut handle,
            )
        };
        assert_eq!(rc, error::OK, "extracted PALOC must parse");
        assert!(!handle.is_null());

        let mut count: u32 = 0;
        assert_eq!(
            unsafe { super::super::paloc::crimson_paloc_entry_count(handle, &mut count) },
            error::OK
        );
        assert!(
            count > 10_000,
            "1.06 English PALOC should have >10k entries, got {count}"
        );

        unsafe { super::super::paloc::crimson_paloc_free(handle) };
    }

    #[test]
    fn c_abi_paz_extract_not_found_directory() {
        let Some(pamt_path) = find_pamt() else {
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let dir = CString::new("not/a/real/directory").unwrap();
        let name = CString::new("anything.bin").unwrap();
        let err = extract_via_abi(&pamt, &dir, &name).unwrap_err();
        assert_eq!(err, error::NOT_FOUND);
    }

    #[test]
    fn c_abi_paz_extract_not_found_file_in_real_dir() {
        let Some(pamt_path) = find_pamt() else {
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        // Real directory, nonsensical file name.
        let dir = CString::new("gamedata/stringtable/binary__").unwrap();
        let name = CString::new("nonexistent_xyzzy_42.paloc").unwrap();
        let err = extract_via_abi(&pamt, &dir, &name).unwrap_err();
        assert_eq!(err, error::NOT_FOUND);
    }

    #[test]
    fn c_abi_paz_extract_bad_pamt_path_returns_io() {
        let pamt = CString::new("Z:\\definitely\\does\\not\\exist\\0.pamt").unwrap();
        let dir = CString::new("x").unwrap();
        let name = CString::new("y").unwrap();
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
        assert_eq!(rc, error::IO);
    }

    /// End-to-end check that the C ABI now extracts partial-compressed
    /// icons (the bulk of `0012/ui/texture/icon/`). Picks one of the
    /// LZ4-compressed entries (so we exercise the prefix-dict decoder,
    /// not just the identity case) and validates the standard DDS magic.
    #[test]
    fn c_abi_paz_extract_partial_dds_icon() {
        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let pamt_path = game_root.join("0012").join("0.pamt");
        if !pamt_path.is_file() {
            eprintln!(
                "skipping c_abi_paz_extract_partial_dds_icon: no {}",
                pamt_path.display()
            );
            return;
        }
        // Pick a partial DDS where compressed_size < uncompressed_size
        // (real LZ4 work, not the identity fast path). Use a font atlas
        // from 0012/ui/fonts/imagefont/ — every entry there is partial
        // compressed in 1.06 and they're small enough to keep the test
        // quick.
        let pamt_bytes = std::fs::read(&pamt_path).expect("read 0.pamt");
        let pamt = PackMeta::parse(&pamt_bytes, None).expect("parse 0.pamt");
        let mut pick: Option<(String, String, u32, u32)> = None;
        for d in &pamt.directories {
            if d.path != "ui/fonts/imagefont" {
                continue;
            }
            for f in &d.files {
                if !f.file.is_partial {
                    continue;
                }
                if !f.name.to_ascii_lowercase().ends_with(".dds") {
                    continue;
                }
                if f.file.compressed_size >= f.file.uncompressed_size {
                    continue;
                }
                pick = Some((
                    d.path.clone(),
                    f.name.clone(),
                    f.file.compressed_size,
                    f.file.uncompressed_size,
                ));
                break;
            }
            if pick.is_some() {
                break;
            }
        }
        let Some((dir_str, name_str, c_size, u_size)) = pick else {
            eprintln!("skipping: no LZ4-compressed partial DDS found under ui/fonts/imagefont");
            return;
        };
        let pamt_c = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let dir_c = CString::new(dir_str.clone()).unwrap();
        let name_c = CString::new(name_str.clone()).unwrap();
        let bytes = extract_via_abi(&pamt_c, &dir_c, &name_c).unwrap_or_else(|rc| {
            panic!(
                "extract failed for {}/{} (c={}, u={}): rc={}",
                dir_str, name_str, c_size, u_size, rc
            )
        });
        assert_eq!(
            bytes.len(),
            u_size as usize,
            "extracted size must match PAMT uncompressed_size"
        );
        assert_eq!(
            &bytes[..4],
            b"DDS ",
            "partial-compressed icon must round-trip to a valid DDS"
        );
    }

    // ── Portrait listing ───────────────────────────────────────────────

    #[test]
    fn portrait_filter_matches_expected() {
        // All six recognised prefixes (real filenames from 1.07's 0012 PAMT).
        assert!(is_npc_portrait_name("cd_portraitimage_chracter_demian.dds"));
        assert!(is_npc_portrait_name("cd_portraitimage_chracter_kliff.dds"));
        assert!(is_npc_portrait_name(
            "cd_portraitimage_nhm_hernand_soldiers_onehandbow_53219.dds"
        ));
        assert!(is_npc_portrait_name(
            "cd_portraitimage_nom_tomaso_soldiers_dualdagger_53778.dds"
        ));
        assert!(is_npc_portrait_name("cd_portraitimage_muscan_boss.dds"));
        assert!(is_npc_portrait_name("cd_mercenary_portrait_54632.dds"));
        // 1.06 legacy spelling — still accepted for cross-version compat.
        assert!(is_npc_portrait_name("cd_portraitimage_character_ogre_xxx.dds"));

        // Case insensitivity — both prefix and suffix.
        assert!(is_npc_portrait_name("CD_PortraitImage_NHM_TEST_01.DDS"));
        assert!(is_npc_portrait_name("CD_Mercenary_Portrait_HERO_01.DDS"));

        // ── Explicitly excluded non-NPC "portrait-like" assets ──
        assert!(!is_npc_portrait_name("cd_portraitimage_animal_wolf_01.dds"));
        assert!(!is_npc_portrait_name("cd_portraitimage_riding_horse_01.dds"));
        assert!(!is_npc_portrait_name("cd_portrait_petimage_falcon.dds"));
        assert!(!is_npc_portrait_name("cd_portrait_wagon_01.dds"));
        assert!(!is_npc_portrait_name("cd_image_portrait_meta.dds"));
        assert!(!is_npc_portrait_name(
            "cd_knowledgeimage_knowledge_world.dds"
        ));

        // Unrelated UI assets.
        assert!(!is_npc_portrait_name("cd_iteminfo_potion.dds"));
        assert!(!is_npc_portrait_name("ItemIcon_sword.dds"));

        // Right prefix, wrong suffix.
        assert!(!is_npc_portrait_name("cd_portraitimage_nhm_token.png"));
        assert!(!is_npc_portrait_name("cd_mercenary_portrait_token.png"));

        // Empty stem ("<prefix>.dds") is rejected — no resolvable token.
        for prefix in NPC_PORTRAIT_PREFIXES {
            let name = format!("{prefix}.dds");
            assert!(
                !is_npc_portrait_name(&name),
                "empty-stem name {name} must be rejected"
            );
        }

        // Degenerate inputs.
        assert!(!is_npc_portrait_name(""));
        assert!(!is_npc_portrait_name(".dds"));
    }

    /// Two-call helper for `crimson_paz_list_npc_portraits`.
    /// Returns the parsed `Vec<String>` plus the raw count the function
    /// reports, so tests can assert they agree.
    fn list_portraits_via_abi(pamt: &CStr) -> Result<(Vec<String>, u32), i32> {
        let mut required: usize = 0;
        let mut count: u32 = 0;
        let rc = unsafe {
            crimson_paz_list_npc_portraits(
                pamt.as_ptr(),
                ptr::null_mut(),
                0,
                &mut required,
                &mut count,
            )
        };
        // Zero matches: function short-circuits to OK on the first call.
        if rc == error::OK {
            return Ok((Vec::new(), count));
        }
        if rc != error::BUFFER_TOO_SMALL {
            return Err(rc);
        }
        let mut buf = vec![0u8; required];
        let rc = unsafe {
            crimson_paz_list_npc_portraits(
                pamt.as_ptr(),
                buf.as_mut_ptr(),
                buf.len(),
                &mut required,
                &mut count,
            )
        };
        if rc != error::OK {
            return Err(rc);
        }
        // Parse the NUL-terminated list back into Strings.
        let mut entries = Vec::with_capacity(count as usize);
        let mut start = 0usize;
        for (i, b) in buf[..required].iter().enumerate() {
            if *b == 0 {
                entries.push(std::str::from_utf8(&buf[start..i]).unwrap().to_string());
                start = i + 1;
            }
        }
        assert_eq!(start, required, "buffer must end exactly on a NUL");
        assert_eq!(entries.len() as u32, count, "parsed entries must match count");
        Ok((entries, count))
    }

    /// Live-install probe: 0012/0.pamt should expose >100 NPC portrait
    /// DDS entries — in 1.07 the breakdown is 62 NHM, 10 NOM, 3
    /// `chracter`, 1 muscan, 76 mercenary (152 total); 1.06 lands in
    /// the same ballpark. Verifies the listing shape end-to-end and
    /// that at least one entry round-trips through
    /// `crimson_paz_extract_file` to a valid DDS (or surfaces
    /// `BODY_PARSE` cleanly — partial-compression chunked variants the
    /// decoder doesn't yet understand are a known limitation, not a
    /// regression).
    #[test]
    fn c_abi_paz_list_npc_portraits_live() {
        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let pamt_path = game_root.join("0012").join("0.pamt");
        if !pamt_path.is_file() {
            eprintln!(
                "skipping c_abi_paz_list_character_portraits_live: no {}",
                pamt_path.display()
            );
            return;
        }
        let pamt_c = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let (entries, count) = list_portraits_via_abi(&pamt_c)
            .expect("listing must succeed on a real 0012/0.pamt");
        assert_eq!(entries.len() as u32, count);
        assert!(
            count >= 100,
            "0012 in 1.06+ must contain at least ~100 NPC portrait DDS entries, got {count}"
        );

        // Every reported entry must obey the filter contract: filename
        // (last path segment) matches one of the recognised NPC prefixes
        // and ends with .dds.
        for path in &entries {
            let (_, file) = path
                .rsplit_once('/')
                .unwrap_or((path.as_str(), path.as_str()));
            assert!(
                is_npc_portrait_name(file),
                "listed entry {path} fails the NPC portrait filter"
            );
        }

        // Probe extraction on the first entry. We don't *require* it to
        // succeed: the partial-compression chunked variant referenced
        // in the module docs is a known TODO. We DO require it to
        // surface as one of {OK, BODY_PARSE} — never as IO or NOT_FOUND
        // (which would mean the listing emitted a fake path).
        let probe = &entries[0];
        let (dir_str, name_str) = probe
            .rsplit_once('/')
            .expect("listed entry should be <dir>/<file>");
        let dir_c = CString::new(dir_str).unwrap();
        let name_c = CString::new(name_str).unwrap();
        match extract_via_abi(&pamt_c, &dir_c, &name_c) {
            Ok(bytes) => {
                assert_eq!(
                    &bytes[..4],
                    b"DDS ",
                    "extracted portrait {probe} must be a valid DDS"
                );
            }
            Err(rc) => {
                assert_eq!(
                    rc, error::BODY_PARSE,
                    "extraction of listed portrait {probe} should be OK or BODY_PARSE, got rc={rc}"
                );
                eprintln!(
                    "note: portrait {probe} hit BODY_PARSE — likely the chunked partial-compression variant"
                );
            }
        }
    }

    /// Cross-version probe: scan every group in the live install for
    /// any filename containing "portrait" / "face" / "headicon", then
    /// bucket the hits by (directory, three-segment prefix) so we can
    /// see at a glance whether Pearl Abyss has reorganised NPC asset
    /// naming. Used as the "On a new game patch" diagnostic referenced
    /// by `scripts/CLAUDE.md` — run with `--nocapture` and compare
    /// against the known 1.07 taxonomy in [`NPC_PORTRAIT_PREFIXES`].
    ///
    /// `#[ignore]` so it doesn't run in the default test suite — this
    /// is investigative tooling, not a regression gate.
    #[test]
    #[ignore = "investigation only — run with --nocapture when probing a new game version"]
    fn _scan_all_groups_for_portrait_like_names() {
        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let entries = match std::fs::read_dir(&game_root) {
            Ok(e) => e,
            Err(_) => {
                eprintln!("skipping: no {}", game_root.display());
                return;
            }
        };
        let mut groups: Vec<String> = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            // Match `NNNN` group directories.
            if name.len() == 4 && name.chars().all(|c| c.is_ascii_digit()) {
                groups.push(name);
            }
        }
        groups.sort();
        for group in &groups {
            let pamt_path = game_root.join(group).join("0.pamt");
            if !pamt_path.is_file() {
                continue;
            }
            let Ok(bytes) = std::fs::read(&pamt_path) else { continue };
            let Ok(pamt) = PackMeta::parse(&bytes, None) else {
                eprintln!("group {group}: PAMT parse failed");
                continue;
            };
            let mut hits: Vec<(String, String)> = Vec::new();
            for d in &pamt.directories {
                for f in &d.files {
                    let lower = f.name.to_ascii_lowercase();
                    if lower.contains("portrait")
                        || lower.contains("npcface")
                        || lower.contains("npc_face")
                        || lower.contains("characterface")
                        || lower.contains("character_face")
                        || lower.contains("headicon")
                    {
                        hits.push((d.path.clone(), f.name.clone()));
                    }
                }
            }
            if !hits.is_empty() {
                eprintln!("group {group}: {} portrait-like hit(s)", hits.len());
                // Bucket by (directory, three-segment prefix) so we
                // see distinct naming conventions, not 600 individual
                // files. E.g. "cd_portraitimage_nhm" or
                // "cd_mercenary_portrait".
                use std::collections::BTreeMap;
                let mut buckets: BTreeMap<(String, String), usize> = BTreeMap::new();
                for (d, n) in &hits {
                    let token: String = n.split('_').take(3).collect::<Vec<_>>().join("_");
                    *buckets.entry((d.clone(), token)).or_insert(0) += 1;
                }
                for ((d, token), count) in &buckets {
                    eprintln!("    [{count:>4}]  {d}/{token}_*");
                }
            }
        }
    }

    #[test]
    fn c_abi_paz_list_npc_portraits_bad_path_returns_io() {
        let pamt = CString::new("Z:\\definitely\\does\\not\\exist\\0.pamt").unwrap();
        let mut required: usize = 0;
        let mut count: u32 = 0;
        let rc = unsafe {
            crimson_paz_list_npc_portraits(
                pamt.as_ptr(),
                ptr::null_mut(),
                0,
                &mut required,
                &mut count,
            )
        };
        assert_eq!(rc, error::IO);
        assert_eq!(required, 0);
        assert_eq!(count, 0);
    }

    #[test]
    fn c_abi_paz_list_npc_portraits_null_args() {
        let pamt = CString::new("anything").unwrap();
        let mut required: usize = 0;
        let mut count: u32 = 0;

        // Each required pointer in turn must trigger NULL_ARG.
        for case in 0..3 {
            let rc = unsafe {
                crimson_paz_list_npc_portraits(
                    if case == 0 { ptr::null() } else { pamt.as_ptr() },
                    ptr::null_mut(),
                    0,
                    if case == 1 { ptr::null_mut() } else { &mut required },
                    if case == 2 { ptr::null_mut() } else { &mut count },
                )
            };
            assert_eq!(rc, error::NULL_ARG, "case {case} must be NULL_ARG");
        }

        // Null buffer with non-zero length is NULL_ARG.
        let rc = unsafe {
            crimson_paz_list_npc_portraits(
                pamt.as_ptr(),
                ptr::null_mut(),
                64,
                &mut required,
                &mut count,
            )
        };
        assert_eq!(rc, error::NULL_ARG);
    }

    #[test]
    fn c_abi_paz_extract_null_args() {
        let pamt = CString::new("anything").unwrap();
        let dir = CString::new("x").unwrap();
        let name = CString::new("y").unwrap();
        let mut needed: usize = 0;

        // Each pointer in turn must trigger NULL_ARG.
        for case in 0..4 {
            let rc = unsafe {
                crimson_paz_extract_file(
                    if case == 0 { ptr::null() } else { pamt.as_ptr() },
                    if case == 1 { ptr::null() } else { dir.as_ptr() },
                    if case == 2 { ptr::null() } else { name.as_ptr() },
                    ptr::null_mut(),
                    0,
                    if case == 3 { ptr::null_mut() } else { &mut needed },
                )
            };
            assert_eq!(rc, error::NULL_ARG, "case {case} must be NULL_ARG");
        }

        // Null buffer with non-zero length is also NULL_ARG.
        let rc = unsafe {
            crimson_paz_extract_file(
                pamt.as_ptr(),
                dir.as_ptr(),
                name.as_ptr(),
                ptr::null_mut(),
                16,
                &mut needed,
            )
        };
        assert_eq!(rc, error::NULL_ARG);
    }
}
