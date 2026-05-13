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
//! Not exposed here (future PR if needed):
//! - PAMT enumeration (list directories / files without extracting).
//! - Batch extraction (avoid re-parsing PAMT N times).
//! - Partial-compression files (`is_partial`), which `extract_file`
//!   itself doesn't support yet either.

use std::ffi::CStr;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use super::error;
use crate::binary::pamt::PackMeta;
use crate::binary::paz;

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
