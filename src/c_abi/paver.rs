//! `meta/0.paver` reader — C ABI surface.
//!
//! Exposes the game-install version stamp parsed by
//! [`crate::binary::paver::Paver`] so downstream consumers
//! (CrimsonAtomtic, modding tools) can decide whether the install's
//! game-data schema matches the parser's expectations before they
//! attempt to load `iteminfo.pabgb` / save body / etc.
//!
//! Two entry points: [`crimson_paver_read_from_file`] takes a path
//! (either the `.paver` file directly or the game-install root, with
//! `meta/0.paver` auto-appended); [`crimson_paver_read_from_bytes`]
//! parses a 10-byte buffer the caller already has in memory.
//!
//! Both surfaces return the four version components via out-pointers
//! and write nothing on failure. See [`crate::binary::paver`] for the
//! layout decode.

use std::ffi::CStr;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

use super::error;
use crate::binary::paver::{COMPATIBLE_GAMEDATA_MINORS, PARSER_TARGET_GAMEDATA_MINOR, Paver};

/// Parse `meta/0.paver` from disk into the four version components.
///
/// `path` accepts two shapes for caller convenience:
///
/// - **The file itself**: `…\Crimson Desert\meta\0.paver`.
/// - **The install root directory**: `…\Crimson Desert\`. The function
///   detects this case via `is_dir()` and auto-appends `meta/0.paver`.
///
/// Returns:
///
/// - `OK` — all four `out_*` written.
/// - `NULL_ARG` — any pointer is null.
/// - `INVALID_PATH` — `path` is not valid UTF-8.
/// - `NOT_FOUND` — the resolved path doesn't exist.
/// - `IO` — read failed for any other reason (permissions, etc).
/// - `BODY_PARSE` — the file is shorter than 10 bytes.
/// - `PANIC` — internal panic (should never happen).
///
/// # Safety
/// `path` must be a NUL-terminated UTF-8 string. Each `out_*` must
/// point to a writable instance of its type. The function writes to
/// the `out_*` pointers only on `OK`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_paver_read_from_file(
    path: *const c_char,
    out_major: *mut u16,
    out_minor: *mut u16,
    out_patch: *mut u16,
    out_build: *mut u32,
) -> i32 {
    if path.is_null()
        || out_major.is_null()
        || out_minor.is_null()
        || out_patch.is_null()
        || out_build.is_null()
    {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let path_str = match unsafe { CStr::from_ptr(path) }.to_str() {
            Ok(s) => s,
            Err(_) => return error::INVALID_PATH,
        };
        let mut resolved = PathBuf::from(path_str);
        // Auto-append meta/0.paver when the caller hands us the
        // install-root directory. This is the common case from the
        // C# side, which already has a "game install path" property
        // and shouldn't need a separate `meta/0.paver` join.
        if resolved.is_dir() {
            resolved.push("meta");
            resolved.push("0.paver");
        }
        let paver = match Paver::from_file(&resolved) {
            Ok(p) => p,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return error::NOT_FOUND;
            }
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return error::BODY_PARSE;
            }
            Err(_) => return error::IO,
        };
        unsafe {
            *out_major = paver.major;
            *out_minor = paver.minor;
            *out_patch = paver.patch;
            *out_build = paver.build;
        }
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Parse a paver buffer already in memory (e.g. when the caller pulled
/// the 10 bytes through some other mechanism). The leading 10 bytes
/// are read; any trailing bytes are tolerated so PA can add fields
/// in a future version without breaking this surface.
///
/// Returns the same code set as [`crimson_paver_read_from_file`]
/// minus the IO codes.
///
/// # Safety
/// `data` may be null iff `data_len == 0`. Each `out_*` must point to
/// a writable instance of its type. The function writes to the
/// `out_*` pointers only on `OK`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_paver_read_from_bytes(
    data: *const u8,
    data_len: usize,
    out_major: *mut u16,
    out_minor: *mut u16,
    out_patch: *mut u16,
    out_build: *mut u32,
) -> i32 {
    if out_major.is_null()
        || out_minor.is_null()
        || out_patch.is_null()
        || out_build.is_null()
    {
        return error::NULL_ARG;
    }
    if data.is_null() && data_len != 0 {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let slice = unsafe { super::slice_from_raw_or_empty(data, data_len) };
        let paver = match Paver::from_bytes(slice) {
            Ok(p) => p,
            Err(_) => return error::BODY_PARSE,
        };
        unsafe {
            *out_major = paver.major;
            *out_minor = paver.minor;
            *out_patch = paver.patch;
            *out_build = paver.build;
        }
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// The Crimson Desert gamedata `minor` version this build's parsers target.
///
/// This is the **single source of truth** for downstream compatibility checks:
/// instead of hard-coding (and re-bumping) a `ParserTargetMinor` constant every
/// patch, the consumer reads it from the DLL once and compares it against the
/// install's `meta/0.paver` minor (via [`crimson_paver_read_from_file`]).
///
/// Infallible — returns the compile-time constant
/// [`crate::binary::paver::PARSER_TARGET_GAMEDATA_MINOR`] directly (no error
/// code, no out-pointer). The current value is documented Rust-side; bump the
/// Rust constant on a new patch and every consumer follows automatically.
#[unsafe(no_mangle)]
pub extern "C" fn crimson_parser_target_gamedata_minor() -> u16 {
    PARSER_TARGET_GAMEDATA_MINOR
}

/// Write the full allow-list of gamedata `minor` versions this build's parsers
/// can load into `out_buf`, and the count into `out_count`.
///
/// This is the set form of [`crimson_parser_target_gamedata_minor`] — the
/// consumer's `CompatibleMinors` / `IsCompatibleWithParser` derive from it
/// (`out_buf` contains `minor` ⇒ compatible). The target minor is always
/// present in the set.
///
/// First-call sizing (same pattern as the other buffer-filling surfaces): pass
/// `out_buf = null`, `cap = 0` to learn the count via `out_count`, then call
/// again with a buffer of at least that many `u16`s.
///
/// Returns:
/// - `OK` — `out_count` written; when `cap >= count`, `out_buf[0..count]` filled.
/// - `BUFFER_TOO_SMALL` — `cap < count`; `out_count` still written so the caller
///   can resize and retry.
/// - `NULL_ARG` — `out_count` is null, or `out_buf` is null with `cap > 0`.
///
/// # Safety
/// `out_count` must point to a writable `usize`. When `cap > 0`, `out_buf` must
/// point to at least `cap` writable `u16`s. The function writes `out_buf` only
/// when `cap >= count`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_parser_compatible_gamedata_minors(
    out_buf: *mut u16,
    cap: usize,
    out_count: *mut usize,
) -> i32 {
    if out_count.is_null() || (out_buf.is_null() && cap != 0) {
        return error::NULL_ARG;
    }
    let minors = COMPATIBLE_GAMEDATA_MINORS;
    unsafe {
        *out_count = minors.len();
    }
    if cap < minors.len() {
        return error::BUFFER_TOO_SMALL;
    }
    if !out_buf.is_null() {
        unsafe {
            std::ptr::copy_nonoverlapping(minors.as_ptr(), out_buf, minors.len());
        }
    }
    error::OK
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::path::PathBuf;
    use std::ptr;

    const PAVER_1_08_LIVE: [u8; 10] =
        [0x01, 0x00, 0x08, 0x00, 0x00, 0x00, 0x3e, 0xb0, 0x39, 0xdc];
    const PAVER_1_09_LIVE: [u8; 10] =
        [0x01, 0x00, 0x09, 0x00, 0x00, 0x00, 0x24, 0x48, 0xf3, 0xbb];

    #[test]
    fn read_from_bytes_happy_path() {
        let mut major = 0u16;
        let mut minor = 0u16;
        let mut patch = 0u16;
        let mut build = 0u32;
        let rc = unsafe {
            crimson_paver_read_from_bytes(
                PAVER_1_08_LIVE.as_ptr(),
                PAVER_1_08_LIVE.len(),
                &mut major,
                &mut minor,
                &mut patch,
                &mut build,
            )
        };
        assert_eq!(rc, error::OK);
        assert_eq!((major, minor, patch, build), (1, 8, 0, 0xdc39b03e));
    }

    #[test]
    fn read_from_bytes_happy_path_1_09() {
        let mut major = 0u16;
        let mut minor = 0u16;
        let mut patch = 0u16;
        let mut build = 0u32;
        let rc = unsafe {
            crimson_paver_read_from_bytes(
                PAVER_1_09_LIVE.as_ptr(),
                PAVER_1_09_LIVE.len(),
                &mut major,
                &mut minor,
                &mut patch,
                &mut build,
            )
        };
        assert_eq!(rc, error::OK);
        assert_eq!((major, minor, patch, build), (1, 9, 0, 0xbbf34824));
    }

    #[test]
    fn read_from_bytes_short_buffer_body_parse() {
        let mut major = 0u16;
        let mut minor = 0u16;
        let mut patch = 0u16;
        let mut build = 0u32;
        let bytes = [0x01, 0x00, 0x08];
        let rc = unsafe {
            crimson_paver_read_from_bytes(
                bytes.as_ptr(),
                bytes.len(),
                &mut major,
                &mut minor,
                &mut patch,
                &mut build,
            )
        };
        assert_eq!(rc, error::BODY_PARSE);
        // Outputs must be untouched on failure.
        assert_eq!((major, minor, patch, build), (0, 0, 0, 0));
    }

    #[test]
    fn read_from_bytes_null_out_pointers_null_arg() {
        let mut a = 0u16;
        let mut b = 0u16;
        let mut c = 0u16;
        let rc = unsafe {
            crimson_paver_read_from_bytes(
                PAVER_1_08_LIVE.as_ptr(),
                PAVER_1_08_LIVE.len(),
                &mut a,
                &mut b,
                &mut c,
                ptr::null_mut(),
            )
        };
        assert_eq!(rc, error::NULL_ARG);
    }

    #[test]
    fn read_from_file_via_install_root_auto_appends_meta_paver() {
        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        if !game_root.is_dir() {
            return;
        }
        let path_c = CString::new(game_root.to_string_lossy().as_bytes()).unwrap();
        let mut major = 0u16;
        let mut minor = 0u16;
        let mut patch = 0u16;
        let mut build = 0u32;
        let rc = unsafe {
            crimson_paver_read_from_file(
                path_c.as_ptr(),
                &mut major,
                &mut minor,
                &mut patch,
                &mut build,
            )
        };
        assert_eq!(rc, error::OK);
        // Pin the major (always 1 in the shipped versions) and assert the live
        // install's minor matches the parser's target — i.e. the dev box is on
        // the patch this build supports. Driving off the constant means the
        // single Rust bump keeps this test honest too (no separate edit).
        assert_eq!(major, 1);
        assert_eq!(
            minor, PARSER_TARGET_GAMEDATA_MINOR,
            "live game install minor should match PARSER_TARGET_GAMEDATA_MINOR in this test environment",
        );
    }

    #[test]
    fn parser_target_gamedata_minor_returns_constant() {
        assert_eq!(
            crimson_parser_target_gamedata_minor(),
            PARSER_TARGET_GAMEDATA_MINOR
        );
    }

    #[test]
    fn compatible_minors_includes_target() {
        assert!(
            COMPATIBLE_GAMEDATA_MINORS.contains(&PARSER_TARGET_GAMEDATA_MINOR),
            "the target minor must always be in the compatible set",
        );
    }

    #[test]
    fn compatible_minors_first_call_sizing_then_fill() {
        // Sizing call: null buffer / cap 0 reports the count via `out_count`.
        // The set always has the target minor, so cap 0 < count ⇒
        // BUFFER_TOO_SMALL (same convention as the other buffer surfaces — the
        // caller reads the count and resizes regardless of the rc).
        let mut count: usize = 0;
        let rc = unsafe {
            crimson_parser_compatible_gamedata_minors(ptr::null_mut(), 0, &mut count)
        };
        assert_eq!(count, COMPATIBLE_GAMEDATA_MINORS.len());
        assert!(count >= 1, "target minor is always present");
        assert_eq!(rc, error::BUFFER_TOO_SMALL);

        // Second call with an exact-size buffer: OK, filled with the allow-list.
        let mut buf = vec![0u16; count];
        let mut c2: usize = 0;
        let rc = unsafe {
            crimson_parser_compatible_gamedata_minors(buf.as_mut_ptr(), buf.len(), &mut c2)
        };
        assert_eq!(rc, error::OK);
        assert_eq!(c2, count);
        assert_eq!(buf.as_slice(), COMPATIBLE_GAMEDATA_MINORS);

        // Over-size buffer is also OK (cap >= count).
        let mut big = vec![0u16; count + 3];
        let mut c3: usize = 0;
        let rc = unsafe {
            crimson_parser_compatible_gamedata_minors(big.as_mut_ptr(), big.len(), &mut c3)
        };
        assert_eq!(rc, error::OK);
        assert_eq!(c3, count);
        assert_eq!(&big[..count], COMPATIBLE_GAMEDATA_MINORS);
    }

    #[test]
    fn compatible_minors_null_out_count_is_null_arg() {
        let mut buf = [0u16; 4];
        let rc = unsafe {
            crimson_parser_compatible_gamedata_minors(buf.as_mut_ptr(), buf.len(), ptr::null_mut())
        };
        assert_eq!(rc, error::NULL_ARG);
    }

    #[test]
    fn read_from_file_missing_path_returns_not_found() {
        let path_c = CString::new(r"Z:\definitely-not-a-real-path\meta\0.paver").unwrap();
        let mut major = 0u16;
        let mut minor = 0u16;
        let mut patch = 0u16;
        let mut build = 0u32;
        let rc = unsafe {
            crimson_paver_read_from_file(
                path_c.as_ptr(),
                &mut major,
                &mut minor,
                &mut patch,
                &mut build,
            )
        };
        assert_eq!(rc, error::NOT_FOUND);
    }
}
