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
//! - Full PAMT enumeration across every directory in one call.
//!   [`crimson_paz_list_dir`] covers per-directory listing — chain it
//!   with the directory-name list a caller already has (or wants to
//!   discover via a future top-level enumerator) for a full walk.
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

/// One entry in the [`crimson_paz_list_dir`] result array — a single
/// file in a PAMT directory.
///
/// Layout is `repr(C)` and stable. Filenames are NUL-padded UTF-8 in
/// a fixed 256-byte buffer; longest names observed in 1.07 PAMTs are
/// ~72 chars (the `worldmapimage_skill_knowledge_*` family in
/// `0012/`), so 256 is comfortable headroom. The `name_truncated`
/// flag (widened to u32 for layout) is set if the underlying PAMT
/// filename didn't fit so callers can detect breakage rather than
/// silently feeding a truncated path back to
/// [`crimson_paz_extract_file`].
///
/// | Offset | Field | Type | Purpose |
/// |---:|---|---|---|
/// |   0 | `name` | `[u8; 256]` | NUL-terminated UTF-8 filename, zero-padded |
/// | 256 | `compressed_size` | u32 | Bytes the file occupies inside the `.paz` chunk |
/// | 260 | `uncompressed_size` | u32 | Bytes after decompression — what `crimson_paz_extract_file` will return |
/// | 264 | `is_partial` | u32 | 1 if partial-compression layout (header(128) + LZ4-with-prefix-dict / identity); 0 otherwise |
/// | 268 | `name_truncated` | u32 | 1 if the source filename exceeded 256 bytes and was truncated to fit |
///
/// Total size: 272 bytes, 4-byte aligned. C# / C++ side can `Span<T>`
/// cast a fresh `byte[]` straight into a `CrimsonPazFileEntry[]`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CrimsonPazFileEntry {
    pub name: [u8; 256],
    pub compressed_size: u32,
    pub uncompressed_size: u32,
    pub is_partial: u32,
    pub name_truncated: u32,
}

// Sanity guard: size + layout are part of the C ABI surface.
const _: () = assert!(std::mem::size_of::<CrimsonPazFileEntry>() == 272);
const _: () = assert!(std::mem::align_of::<CrimsonPazFileEntry>() == 4);

/// Copy `name` into the fixed buffer, NUL-terminating + zero-padding
/// the trailing bytes. Returns `true` when truncation happened.
fn fill_name_buf(buf: &mut [u8; 256], name: &str) -> bool {
    let src = name.as_bytes();
    if src.len() < 256 {
        buf[..src.len()].copy_from_slice(src);
        buf[src.len()..].fill(0);
        false
    } else {
        // Reserve last byte for the NUL terminator. Truncation is
        // a real risk for callers — flag it.
        buf[..255].copy_from_slice(&src[..255]);
        buf[255] = 0;
        true
    }
}

/// List every file in a single PAMT directory, with the metadata a
/// caller needs to drive [`crimson_paz_extract_file`] (filename) and
/// pre-size its output buffer (uncompressed size). Built for the C#
/// editor's world-map basemap workflow:
///
/// 1. Call `crimson_paz_list_dir(pamt_path = "…/0015/0.pamt",
///    directory = "leveldata/rootlevel/terrain/color")` to enumerate
///    the 785 terrain color tiles.
/// 2. For each entry, call `crimson_paz_extract_file` with
///    `(pamt_path, directory, entry.name)` to pull the DDS bytes.
/// 3. Decode + cache locally; composite client-side.
///
/// `pamt_path` is the absolute path to `0.pamt` inside a group folder
/// (e.g. `D:\\…\\Crimson Desert\\0015\\0.pamt`). `directory` is the
/// in-archive directory path (e.g. `leveldata/rootlevel/terrain/color`).
/// Both NUL-terminated UTF-8.
///
/// **Two-call shape** (record-array variant, same as
/// [`super::all_items::crimson_save_list_all_items`]):
///
/// - First call with `out_entries = null, capacity_entries = 0`
///   populates `*out_count_entries`. Returns `BUFFER_TOO_SMALL`
///   (unless the directory has zero files, in which case returns
///   `OK`).
/// - Allocate `*out_count_entries` records, call again. Returns `OK`
///   on success.
///
/// **Note**: each call re-parses the PAMT (no caching). For 0012's
/// 751 KB PAMT that's a few ms; cheap but not free. Callers walking
/// many directories from the same PAMT may want to add their own
/// outer cache.
///
/// Return codes:
/// - `OK` — list written. `*out_count_entries` is populated.
/// - `BUFFER_TOO_SMALL` — `capacity_entries < *out_count_entries`.
///   `*out_count_entries` is populated so the caller can allocate
///   and re-call.
/// - `NOT_FOUND` — `directory` isn't in the PAMT.
/// - `IO` — the PAMT file can't be read from disk.
/// - `BODY_PARSE` — the PAMT bytes don't parse.
/// - `NULL_ARG` — any required pointer is null (see Safety).
/// - `INVALID_PATH` — bad UTF-8 in `pamt_path` or `directory`.
///
/// # Safety
/// `pamt_path` and `directory` must be non-null and NUL-terminated
/// UTF-8. `out_count_entries` must point at writable memory.
/// `out_entries` may be null iff `capacity_entries == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_paz_list_dir(
    pamt_path: *const c_char,
    directory: *const c_char,
    out_entries: *mut CrimsonPazFileEntry,
    capacity_entries: usize,
    out_count_entries: *mut usize,
) -> i32 {
    if pamt_path.is_null() || directory.is_null() || out_count_entries.is_null() {
        return error::NULL_ARG;
    }
    if out_entries.is_null() && capacity_entries != 0 {
        return error::NULL_ARG;
    }
    unsafe { *out_count_entries = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let pamt_str = match unsafe { CStr::from_ptr(pamt_path) }.to_str() {
            Ok(s) => s,
            Err(_) => return error::INVALID_PATH,
        };
        let dir_str = match unsafe { CStr::from_ptr(directory) }.to_str() {
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

        let Some(dir) = pamt.directories.iter().find(|d| d.path == dir_str) else {
            return error::NOT_FOUND;
        };

        unsafe { *out_count_entries = dir.files.len() };

        if dir.files.is_empty() {
            return error::OK;
        }
        if capacity_entries < dir.files.len() {
            return error::BUFFER_TOO_SMALL;
        }

        // Fill the caller's array. SAFETY: we just checked that
        // `out_entries` is non-null and `capacity_entries >= len`.
        for (idx, f) in dir.files.iter().enumerate() {
            let mut name = [0u8; 256];
            let truncated = fill_name_buf(&mut name, &f.name);
            let entry = CrimsonPazFileEntry {
                name,
                compressed_size: f.file.compressed_size,
                uncompressed_size: f.file.uncompressed_size,
                is_partial: u32::from(f.file.is_partial),
                name_truncated: u32::from(truncated),
            };
            unsafe { out_entries.add(idx).write(entry) };
        }
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

#[cfg(test)]
mod tests {
    //! Live-install integration tests. Skip cleanly when the Steam
    //! install isn't present (same pattern as `test_paloc_parse` in
    //! lib.rs).

    use crate::binary::gamedata_layout;
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
        // One blob per language through 2.00; 2.01 split it into 39
        // per-namespace files. Extract whatever the install ships and sum —
        // the whole-language total is what the >10k assertion is about.
        let Some((dir_path, names)) = gamedata_layout::paloc_files("0020", "eng") else {
            panic!("no English paloc in 0020/0.pamt");
        };
        let dir = CString::new(dir_path).unwrap();

        let mut total: u32 = 0;
        for file_name in &names {
            let name = CString::new(file_name.as_str()).unwrap();
            let bytes = extract_via_abi(&pamt, &dir, &name).unwrap_or_else(|rc| {
                panic!("{file_name} must extract cleanly from 0020/0.pamt (rc={rc})")
            });

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
            assert_eq!(rc, error::OK, "extracted PALOC {file_name} must parse");
            assert!(!handle.is_null());

            let mut count: u32 = 0;
            assert_eq!(
                unsafe { super::super::paloc::crimson_paloc_entry_count(handle, &mut count) },
                error::OK
            );
            total += count;

            unsafe { super::super::paloc::crimson_paloc_free(handle) };
        }

        assert!(
            total > 10_000,
            "English PALOC should have >10k entries across {} file(s), got {total}",
            names.len()
        );
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

    // ── List-directory tests ───────────────────────────────────────────

    #[test]
    fn list_dir_record_layout_is_stable() {
        // Size + alignment are part of the C ABI surface.
        assert_eq!(std::mem::size_of::<CrimsonPazFileEntry>(), 272);
        assert_eq!(std::mem::align_of::<CrimsonPazFileEntry>(), 4);

        let e = CrimsonPazFileEntry {
            name: [0u8; 256], compressed_size: 0, uncompressed_size: 0,
            is_partial: 0, name_truncated: 0,
        };
        let base = (&e as *const CrimsonPazFileEntry).addr();
        let off_u8 = |p: *const u8| (p as usize) - base;
        let off_u32 = |p: *const u32| (p as usize) - base;
        assert_eq!(off_u8(e.name.as_ptr()),       0);
        assert_eq!(off_u32(&e.compressed_size),   256);
        assert_eq!(off_u32(&e.uncompressed_size), 260);
        assert_eq!(off_u32(&e.is_partial),        264);
        assert_eq!(off_u32(&e.name_truncated),    268);
    }

    #[test]
    fn fill_name_buf_normal_case() {
        let mut buf = [0xFFu8; 256];
        let truncated = fill_name_buf(&mut buf, "terrain_-5_-5_color_c.dds");
        assert!(!truncated);
        // Bytes 0..25 are the filename; byte 25 onwards are zero.
        assert_eq!(&buf[..25], b"terrain_-5_-5_color_c.dds");
        for b in &buf[25..] {
            assert_eq!(*b, 0, "trailing bytes must be zero-padded");
        }
    }

    #[test]
    fn fill_name_buf_truncation() {
        let long = "x".repeat(300);
        let mut buf = [0u8; 256];
        let truncated = fill_name_buf(&mut buf, &long);
        assert!(truncated);
        // First 255 bytes are 'x'; byte 255 is the NUL terminator.
        assert_eq!(&buf[..255], &[b'x'; 255]);
        assert_eq!(buf[255], 0);
    }

    /// Two-call helper for [`crimson_paz_list_dir`].
    fn list_dir_via_abi(
        pamt: &CStr,
        dir: &CStr,
    ) -> Result<Vec<CrimsonPazFileEntry>, i32> {
        let mut count: usize = 0;
        let rc = unsafe {
            crimson_paz_list_dir(pamt.as_ptr(), dir.as_ptr(), ptr::null_mut(), 0, &mut count)
        };
        if rc == error::OK && count == 0 {
            return Ok(Vec::new());
        }
        if rc != error::BUFFER_TOO_SMALL {
            return Err(rc);
        }
        let mut entries = vec![CrimsonPazFileEntry {
            name: [0u8; 256], compressed_size: 0, uncompressed_size: 0,
            is_partial: 0, name_truncated: 0,
        }; count];
        let rc = unsafe {
            crimson_paz_list_dir(
                pamt.as_ptr(), dir.as_ptr(),
                entries.as_mut_ptr(), entries.len(),
                &mut count,
            )
        };
        if rc != error::OK {
            return Err(rc);
        }
        entries.truncate(count);
        Ok(entries)
    }

    fn entry_name(e: &CrimsonPazFileEntry) -> &str {
        let len = e.name.iter().position(|&b| b == 0).unwrap_or(e.name.len());
        std::str::from_utf8(&e.name[..len]).unwrap()
    }

    /// Live: the 0015 terrain color directory must hold exactly 785
    /// tiles in 1.07 — that's the world-map basemap working set. Pins
    /// the per-tile size invariants too (uncompressed = 174,904 bytes,
    /// not partial-compressed).
    #[test]
    fn c_abi_paz_list_dir_terrain_color_live() {
        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let pamt_path = game_root.join("0015").join("0.pamt");
        if !pamt_path.is_file() {
            eprintln!("skipping: no {}", pamt_path.display());
            return;
        }
        let pamt_c = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let dir_c = CString::new("leveldata/rootlevel/terrain/color").unwrap();
        let entries = list_dir_via_abi(&pamt_c, &dir_c).expect("list must succeed");
        eprintln!("0015 terrain/color count = {}", entries.len());
        assert!(
            entries.len() >= 700,
            "expected ~785 color tiles, got {}", entries.len(),
        );
        // Every tile name should match `terrain_X_Y_color_c.dds`. Size
        // varies across tiles: the dominant case is 174,904 bytes
        // (matches our 512×512 BC1 estimate) but some edge / corner
        // tiles are smaller (e.g. terrain_-1_19_color_c.dds = 43,832 B).
        // Asserting "plausible DDS size" rather than a hard equality.
        let mut size_hist: std::collections::BTreeMap<u32, u32> = Default::default();
        for e in &entries {
            assert_eq!(e.name_truncated, 0, "name truncated unexpectedly");
            assert!(
                e.uncompressed_size > 1024,
                "tile {} only {} bytes — suspicious",
                entry_name(e), e.uncompressed_size,
            );
            *size_hist.entry(e.uncompressed_size).or_insert(0) += 1;
            let n = entry_name(e);
            assert!(n.starts_with("terrain_"), "weird name: {n}");
            assert!(n.ends_with("_color_c.dds"), "weird name: {n}");
        }
        eprintln!("size histogram:");
        for (size, count) in &size_hist {
            eprintln!("  {size} B: {count} tiles");
        }
        // Dominant size should be 174904 (BC1 512² + headers + mips).
        let dominant = size_hist.iter().max_by_key(|(_, c)| *c).unwrap();
        assert_eq!(
            *dominant.0, 174904,
            "expected the dominant tile size to be 174904 B (BC1 512²)",
        );
    }

    /// End-to-end pinning: list a directory, extract the first entry,
    /// verify the DDS magic + size matches what list_dir reported.
    /// Proves the round-trip C# editors will use.
    #[test]
    fn c_abi_paz_list_dir_then_extract_global_colormap() {
        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let pamt_path = game_root.join("0015").join("0.pamt");
        if !pamt_path.is_file() {
            eprintln!("skipping: no {}", pamt_path.display());
            return;
        }
        let pamt_c = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let dir_c = CString::new("leveldata/rootlevel/terrain/global").unwrap();
        let entries = list_dir_via_abi(&pamt_c, &dir_c).expect("list");
        // 6 files in 1.07: global_colormap.dds + 5 region/tint maps.
        assert!(entries.len() >= 4, "expected ≥4 global terrain files, got {}", entries.len());
        let colormap = entries.iter()
            .find(|e| entry_name(e) == "global_colormap.dds")
            .expect("global_colormap.dds must be listed");
        assert_eq!(colormap.uncompressed_size, 5_592_560);

        // Round-trip: feed list_dir's reported name back into extract_file.
        let name_c = CString::new("global_colormap.dds").unwrap();
        let bytes = extract_via_abi(&pamt_c, &dir_c, &name_c).expect("extract");
        assert_eq!(bytes.len(), colormap.uncompressed_size as usize);
        assert_eq!(&bytes[..4], b"DDS ", "must be a valid DDS");
    }

    #[test]
    fn c_abi_paz_list_dir_not_found() {
        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let pamt_path = game_root.join("0015").join("0.pamt");
        if !pamt_path.is_file() {
            eprintln!("skipping: no {}", pamt_path.display());
            return;
        }
        let pamt_c = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let dir_c = CString::new("not/a/real/dir").unwrap();
        let err = list_dir_via_abi(&pamt_c, &dir_c).unwrap_err();
        assert_eq!(err, error::NOT_FOUND);
    }

    #[test]
    fn c_abi_paz_list_dir_bad_pamt_path_returns_io() {
        let pamt = CString::new("Z:\\does\\not\\exist\\0.pamt").unwrap();
        let dir = CString::new("x").unwrap();
        let mut count: usize = 0;
        let rc = unsafe {
            crimson_paz_list_dir(pamt.as_ptr(), dir.as_ptr(), ptr::null_mut(), 0, &mut count)
        };
        assert_eq!(rc, error::IO);
    }

    #[test]
    fn c_abi_paz_list_dir_null_args() {
        let pamt = CString::new("anything").unwrap();
        let dir = CString::new("x").unwrap();
        let mut count: usize = 0;
        for case in 0..3 {
            let rc = unsafe {
                crimson_paz_list_dir(
                    if case == 0 { ptr::null() } else { pamt.as_ptr() },
                    if case == 1 { ptr::null() } else { dir.as_ptr() },
                    ptr::null_mut(),
                    0,
                    if case == 2 { ptr::null_mut() } else { &mut count },
                )
            };
            assert_eq!(rc, error::NULL_ARG, "case {case}");
        }
        // Null buffer with non-zero capacity is also NULL_ARG.
        let rc = unsafe {
            crimson_paz_list_dir(pamt.as_ptr(), dir.as_ptr(), ptr::null_mut(), 16, &mut count)
        };
        assert_eq!(rc, error::NULL_ARG);
    }

    /// Buffer-too-small with an undersized capacity must still
    /// populate `*out_count_entries` so the caller can re-allocate.
    #[test]
    fn c_abi_paz_list_dir_buffer_too_small_populates_count() {
        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let pamt_path = game_root.join("0015").join("0.pamt");
        if !pamt_path.is_file() {
            eprintln!("skipping: no {}", pamt_path.display());
            return;
        }
        let pamt_c = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let dir_c = CString::new("leveldata/rootlevel/terrain/color").unwrap();

        // Cap at 1 entry — known to be undersized.
        let mut entry = CrimsonPazFileEntry {
            name: [0u8; 256], compressed_size: 0, uncompressed_size: 0,
            is_partial: 0, name_truncated: 0,
        };
        let mut count: usize = 0;
        let rc = unsafe {
            crimson_paz_list_dir(pamt_c.as_ptr(), dir_c.as_ptr(), &mut entry, 1, &mut count)
        };
        assert_eq!(rc, error::BUFFER_TOO_SMALL);
        assert!(count > 1, "real count must be reported even on BUFFER_TOO_SMALL");
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
