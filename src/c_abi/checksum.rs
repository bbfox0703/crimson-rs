//! Jenkins hashlittle2 checksum — C ABI surface.
//!
//! Thin extern "C" wrapper over [`crate::crypto::checksum::calculate_checksum`].
//! The same hash is used inside crimson-rs for PAMT / PAPGT integrity, and —
//! independently — by the game data layer to derive PALOC u64 lookup keys
//! from gamedata internal-name strings:
//!
//! ```text
//! PALOC u64 key = (hashlittle2_c(internal_name) << 32) | lo32_namespace
//! ```
//!
//! That two-hop chain is what lets the Save Editor map a save-side
//! `MissionKey` / `QuestKey` / `StageKey` (u32 in the save body) to a
//! localized display title via `missioninfo.pabgb` / `questinfo.pabgb` /
//! `stageinfo.pabgb`. See `docs/save-editor-keys-plan.md` for the verified
//! end-to-end mappings; this bridge exposes the hash hop so the C# side
//! doesn't have to re-implement the Jenkins variant.
//!
//! Stateless — no handle, no allocation. Mirrors the same error-code
//! contract as the rest of the C ABI.

use std::os::raw::c_int;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::error;
use crate::crypto::checksum::calculate_checksum;

/// Compute the Jenkins hashlittle2 `c` output (Pearl Abyss variant — init
/// constant `0xDEBA1DCD`) over `data`. The result is the primary 32-bit
/// hash used everywhere in the file format.
///
/// On success writes the u32 hash to `*out_checksum` and returns
/// [`error::OK`]. On null `out_checksum`, or null `data` with non-zero
/// `len`, returns [`error::NULL_ARG`].
///
/// Empty input is valid — pass `data = null, len = 0` to hash the empty
/// byte string. The hash of empty input is deterministic and matches the
/// init value.
///
/// # Safety
/// `data` must point to `len` readable bytes (may be null iff `len == 0`).
/// `out_checksum` must be non-null and writable for the duration of the
/// call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_calculate_checksum(
    data: *const u8,
    len: usize,
    out_checksum: *mut u32,
) -> c_int {
    if out_checksum.is_null() {
        return error::NULL_ARG;
    }
    if data.is_null() && len != 0 {
        return error::NULL_ARG;
    }
    unsafe { *out_checksum = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let slice = if len == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(data, len) }
        };
        let ck = calculate_checksum(slice);
        unsafe { *out_checksum = ck };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

#[cfg(test)]
mod tests {
    //! Hash-correctness tests pin against the Save Editor's actual
    //! workload — three known `internal_name → hi32` mappings from the
    //! verified Mission/Quest/Stage table in `docs/save-editor-keys-plan.md`.
    //! If any of these regresses, the title-resolution chain in the
    //! editor breaks, so they're worth keeping as live load-bearing
    //! tests (not just synthetic).

    use super::*;
    use std::ptr;

    /// One verified mapping per category — Mission, Quest-node-style,
    /// Stage. Each was end-to-end traced to a PALOC display string in
    /// the analysis recorded under `docs/save-editor-keys-plan.md`
    /// ("Verified hash transform"). The hashes here are the `hi32`
    /// values from that table.
    const KNOWN: &[(&[u8], u32)] = &[
        // Mission_Intro_Tutorial_I -> "Unfamiliar Lands"
        (b"Mission_Intro_Tutorial_I", 1_891_183_967),
        // Mission_IronStronghold_Block_ReturnToSister -> "Where the Wind Guides You"
        (b"Mission_IronStronghold_Block_ReturnToSister", 3_594_586_120),
        // Intro_Tutorial_Miseenscene_00 (StageKey 1004305 in the test save)
        // -> "Mise-en-scene Before Intro Combat"
        (b"Intro_Tutorial_Miseenscene_00", 3_530_966_846),
    ];

    #[test]
    fn c_abi_checksum_known_inputs() {
        for &(input, expected) in KNOWN {
            let mut out: u32 = 0;
            let rc = unsafe {
                crimson_calculate_checksum(input.as_ptr(), input.len(), &mut out)
            };
            assert_eq!(rc, error::OK, "rc for {:?}", std::str::from_utf8(input));
            assert_eq!(
                out,
                expected,
                "checksum mismatch for {:?}: got {}, expected {}",
                std::str::from_utf8(input),
                out,
                expected,
            );
        }
    }

    #[test]
    fn c_abi_checksum_empty_input() {
        // Hashing the empty byte string should succeed and produce a
        // deterministic value (the init constant after no mixing).
        // Pin to the underlying Rust function's behaviour, not a magic
        // number, so a future tweak to the hash propagates here without
        // a manual fixup.
        let expected = crate::crypto::checksum::calculate_checksum(&[]);
        let mut out: u32 = 0;
        // Both forms — null with len=0, and a real pointer with len=0 —
        // must succeed and produce the same value.
        let rc = unsafe { crimson_calculate_checksum(ptr::null(), 0, &mut out) };
        assert_eq!(rc, error::OK);
        assert_eq!(out, expected);

        let empty = [0u8; 1]; // pointer is non-null but len=0; bytes ignored
        let mut out2: u32 = 0;
        let rc = unsafe { crimson_calculate_checksum(empty.as_ptr(), 0, &mut out2) };
        assert_eq!(rc, error::OK);
        assert_eq!(out2, expected);
    }

    #[test]
    fn c_abi_checksum_null_args() {
        let mut out: u32 = 999;
        // null data with non-zero len → NULL_ARG, out_checksum untouched
        // by the early return (but the function does write 0 before the
        // catch_unwind body; that's deliberate so the caller sees a
        // sentinel even on error). Check NULL_ARG return is enough.
        let rc = unsafe { crimson_calculate_checksum(ptr::null(), 16, &mut out) };
        assert_eq!(rc, error::NULL_ARG);

        // null out_checksum → NULL_ARG even with valid data
        let data = [1u8, 2, 3, 4];
        let rc = unsafe {
            crimson_calculate_checksum(data.as_ptr(), data.len(), ptr::null_mut())
        };
        assert_eq!(rc, error::NULL_ARG);
    }

    #[test]
    fn c_abi_checksum_matches_rust_calculate_checksum() {
        // Sanity: the ABI wrapper must agree with the underlying Rust
        // function for arbitrary bytes — pick a few sizes that exercise
        // the 12-byte block loop + tail handling.
        for input in [
            &b"a"[..],
            &b"abc"[..],
            &b"abcdefghijkl"[..],          // exactly 12 bytes
            &b"abcdefghijklm"[..],         // 12 + 1 tail
            &b"abcdefghijklmnopqrstuvwx"[..], // 24 = 2 blocks
            &b"abcdefghijklmnopqrstuvwxyz"[..], // 24 + 2 tail
        ] {
            let mut out: u32 = 0;
            let rc = unsafe {
                crimson_calculate_checksum(input.as_ptr(), input.len(), &mut out)
            };
            assert_eq!(rc, error::OK);
            assert_eq!(
                out,
                calculate_checksum(input),
                "C ABI disagreed with Rust calculate_checksum on {:?}",
                std::str::from_utf8(input).ok(),
            );
        }
    }
}
