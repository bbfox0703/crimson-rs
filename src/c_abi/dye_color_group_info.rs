//! `dyecolorgroupinfo.pabgb` bridge — C ABI surface.
//!
//! Resolves `DyeColorGroupInfoKey (u32)` — the gamedata key stored as
//! `ItemDyeSaveData._dyeColorGroupInfoKey` in the save's
//! `_itemDyeDataList`. Returns the row's internal name (e.g.
//! `"Her_Color_Group_I"`, `"Dem_Color_Group_III"`); the dye UI shows
//! a localized version, but for v1 the C# editor renders the raw
//! internal name in the "Named family" dropdown.
//!
//! Both `dyecolorgroupinfo.pabgb` and `dyecolorgroupinfo.pabgh` must
//! be loaded together — the PABGH provides the explicit (key, offset)
//! index that locates each row.
//!
//! See [`crate::dye_color_group_info`] for the row schema.

use std::collections::HashMap;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::error;
use crate::dye_color_group_info::parse_dye_color_group_info_lossy;

/// Opaque handle exposing `(DyeColorGroupInfoKey, internal_name)`
/// lookups + the 109-position **logical-RGBA palette** for each
/// row, against the loaded `dyecolorgroupinfo.pabgb` + `.pabgh`.
///
/// See [`crate::dye_color_group_info`] for the BGRA-to-RGBA swap
/// rationale + the palette layout (positions 0-8 grayscale + 9-108
/// ten chromatic rows × ten columns).
#[repr(C)]
pub struct CrimsonDyeColorGroupInfoHandle {
    by_key: HashMap<u32, String>,
    entries: Vec<(u32, String)>,
    /// Logical-RGBA palette per key (post-swap from on-disk BGRA).
    /// 109 records per row in 1.07; the bridge doesn't pin the
    /// count so a future patch with a different palette size still
    /// resolves.
    palettes: HashMap<u32, Vec<[u8; 4]>>,
}

impl CrimsonDyeColorGroupInfoHandle {
    fn from_bytes(pabgb: &[u8], pabgh: &[u8]) -> Self {
        let raw = parse_dye_color_group_info_lossy(pabgb, pabgh);
        let mut by_key: HashMap<u32, String> = HashMap::with_capacity(raw.len());
        let mut entries: Vec<(u32, String)> = Vec::with_capacity(raw.len());
        let mut palettes: HashMap<u32, Vec<[u8; 4]>> = HashMap::with_capacity(raw.len());
        for e in raw {
            if let std::collections::hash_map::Entry::Vacant(v) = by_key.entry(e.key) {
                v.insert(e.name.clone());
                entries.push((e.key, e.name));
                palettes.insert(e.key, e.palette);
            }
        }
        CrimsonDyeColorGroupInfoHandle { by_key, entries, palettes }
    }
}

// ── Load / free ────────────────────────────────────────────────────────────

/// Parse `dyecolorgroupinfo.pabgb` + `.pabgh` from disk.
///
/// # Safety
/// `pabgb_path` and `pabgh_path` must be NUL-terminated UTF-8 strings.
/// `out_handle` must be non-null and writable for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_dye_color_group_info_load_from_file(
    pabgb_path: *const c_char,
    pabgh_path: *const c_char,
    out_handle: *mut *mut CrimsonDyeColorGroupInfoHandle,
) -> i32 {
    if pabgb_path.is_null() || pabgh_path.is_null() || out_handle.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_handle = std::ptr::null_mut() };
    catch_unwind(AssertUnwindSafe(|| {
        let pabgb_str = match unsafe { std::ffi::CStr::from_ptr(pabgb_path) }.to_str() {
            Ok(s) => s,
            Err(_) => return error::INVALID_PATH,
        };
        let pabgh_str = match unsafe { std::ffi::CStr::from_ptr(pabgh_path) }.to_str() {
            Ok(s) => s,
            Err(_) => return error::INVALID_PATH,
        };
        let pabgb: Vec<u8> = match std::fs::read(pabgb_str) {
            Ok(b) => b,
            Err(_) => return error::IO,
        };
        let pabgh: Vec<u8> = match std::fs::read(pabgh_str) {
            Ok(b) => b,
            Err(_) => return error::IO,
        };
        let handle = CrimsonDyeColorGroupInfoHandle::from_bytes(&pabgb, &pabgh);
        unsafe { *out_handle = Box::into_raw(Box::new(handle)) };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Parse bytes already in memory.
///
/// # Safety
/// `pabgb` / `pabgh` may be null iff the corresponding `_len` is 0.
/// `out_handle` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_dye_color_group_info_load_from_bytes(
    pabgb: *const u8,
    pabgb_len: usize,
    pabgh: *const u8,
    pabgh_len: usize,
    out_handle: *mut *mut CrimsonDyeColorGroupInfoHandle,
) -> i32 {
    if out_handle.is_null() {
        return error::NULL_ARG;
    }
    if (pabgb.is_null() && pabgb_len != 0) || (pabgh.is_null() && pabgh_len != 0) {
        return error::NULL_ARG;
    }
    unsafe { *out_handle = std::ptr::null_mut() };
    catch_unwind(AssertUnwindSafe(|| {
        let pabgb_slice: &[u8] = if pabgb_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(pabgb, pabgb_len) }
        };
        let pabgh_slice: &[u8] = if pabgh_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(pabgh, pabgh_len) }
        };
        let handle = CrimsonDyeColorGroupInfoHandle::from_bytes(pabgb_slice, pabgh_slice);
        unsafe { *out_handle = Box::into_raw(Box::new(handle)) };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Free a handle returned by either loader.
///
/// # Safety
/// `handle` must be null or a pointer previously returned by one of the
/// loaders and not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_dye_color_group_info_free(
    handle: *mut CrimsonDyeColorGroupInfoHandle,
) {
    if handle.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(handle);
    }
}

// ── Scalar getters ─────────────────────────────────────────────────────────

/// Number of color-group rows in the table.
///
/// # Safety
/// `handle` and `out_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_dye_color_group_info_entry_count(
    handle: *const CrimsonDyeColorGroupInfoHandle,
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

// ── Lookup ─────────────────────────────────────────────────────────────────

/// Look up the internal name for a given `DyeColorGroupInfoKey (u32)`.
/// Writes the UTF-8 result (NUL-terminated) into `buf`; two-call pattern.
///
/// # Safety
/// `handle` and `required` must be non-null; `buf` may be null iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_dye_color_group_info_lookup_name(
    handle: *const CrimsonDyeColorGroupInfoHandle,
    color_group_key: u32,
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
        let Some(name) = h.by_key.get(&color_group_key) else {
            return error::NOT_FOUND;
        };
        write_str_to_buf(name, buf, buf_len, required)
    }))
    .unwrap_or(error::PANIC)
}

// ── Enumeration ────────────────────────────────────────────────────────────

/// Get the `(key, internal_name)` pair at insertion index `idx`. Index
/// matches PABGH on-disk order.
///
/// # Safety
/// `handle`, `out_key`, `required` must be non-null; `buf` may be null
/// iff `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_dye_color_group_info_get_entry(
    handle: *const CrimsonDyeColorGroupInfoHandle,
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

// ── Palette accessors (dye picker UX) ─────────────────────────────────────
//
// The save's `_dyeColorR/G/B` u8 scalars index into the 109-position
// palette stored on the theme row. These three functions let the C#
// editor render the theme's palette as a visual grid, write a chosen
// position's RGB back to the save, and reverse-look-up which cell a
// currently-applied dye came from.
//
// **Logical RGBA order**: on-disk bytes are BGRA; the parser swaps to
// (R, G, B, A) so the values returned here match the save's u8 fields
// directly. See [`crate::dye_color_group_info`] for the rationale.

/// Number of palette positions for `color_group_key` (109 in 1.07,
/// but the ABI doesn't pin the count — a future patch with a
/// different palette size will report the new value).
///
/// Returns [`error::NOT_FOUND`] if the key isn't in the table.
///
/// # Safety
/// `handle` must be live; `out_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_dye_color_group_info_palette_size(
    handle: *const CrimsonDyeColorGroupInfoHandle,
    color_group_key: u32,
    out_count: *mut u32,
) -> i32 {
    if handle.is_null() || out_count.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_count = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some(palette) = h.palettes.get(&color_group_key) else {
            return error::NOT_FOUND;
        };
        unsafe { *out_count = palette.len() as u32 };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Logical RGBA at `position_idx` inside `color_group_key`'s palette.
/// The four output bytes are in `(R, G, B, A)` order, ready to write
/// straight into the save's `_dyeColorR/G/B/A` u8 scalars.
///
/// Returns [`error::NOT_FOUND`] for an unknown key,
/// [`error::OUT_OF_RANGE`] when `position_idx` is past the palette.
///
/// # Safety
/// `handle` must be live; the four output pointers must be non-null
/// and writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_dye_color_group_info_palette_at(
    handle: *const CrimsonDyeColorGroupInfoHandle,
    color_group_key: u32,
    position_idx: u32,
    out_r: *mut u8,
    out_g: *mut u8,
    out_b: *mut u8,
    out_a: *mut u8,
) -> i32 {
    if handle.is_null()
        || out_r.is_null()
        || out_g.is_null()
        || out_b.is_null()
        || out_a.is_null()
    {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some(palette) = h.palettes.get(&color_group_key) else {
            return error::NOT_FOUND;
        };
        let Some(rgba) = palette.get(position_idx as usize) else {
            return error::OUT_OF_RANGE;
        };
        unsafe {
            *out_r = rgba[0];
            *out_g = rgba[1];
            *out_b = rgba[2];
            *out_a = rgba[3];
        }
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Reverse lookup — find the palette position that matches the given
/// `(r, g, b)`. Returns [`error::NOT_FOUND`] when no position is an
/// exact match (e.g. the save's RGB was set by a tool like Cheat
/// Engine off-grid), [`error::NOT_FOUND`] when the key is unknown.
/// Alpha is not part of the match (every observed position uses
/// `0xFF`).
///
/// The C# editor uses this to highlight which palette cell a
/// currently-applied dye came from in its picker grid.
///
/// # Safety
/// `handle` must be live; `out_position` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_dye_color_group_info_position_for_rgb(
    handle: *const CrimsonDyeColorGroupInfoHandle,
    color_group_key: u32,
    r: u8,
    g: u8,
    b: u8,
    out_position: *mut u32,
) -> i32 {
    if handle.is_null() || out_position.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_position = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some(palette) = h.palettes.get(&color_group_key) else {
            return error::NOT_FOUND;
        };
        for (i, rgba) in palette.iter().enumerate() {
            if rgba[0] == r && rgba[1] == g && rgba[2] == b {
                unsafe { *out_position = i as u32 };
                return error::OK;
            }
        }
        error::NOT_FOUND
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
    //! Live-install integration test. Pins all 10 (key → name) mappings
    //! from the 1.07 install and exercises the lookup + enumeration +
    //! NULL-arg paths.

    use super::*;
    use std::ffi::CString;
    use std::path::PathBuf;
    use std::ptr;

    /// (key, expected internal name) — all 10 rows from the live 1.07
    /// install, same set as the parser test pins.
    const KNOWN: &[(u32, &str)] = &[
        (0xc88211f5, "Her_Color_Group_I"),
        (0xdc274476, "Dem_Color_Group_I"),
        (0x068f0cce, "Dem_Color_Group_II"),
        (0x40707e94, "Dem_Color_Group_III"),
        (0x001835e0, "Kwe_Color_Group_I"),
        (0xa7ec4d9b, "Del_Color_Group_I"),
        (0x2d0517c9, "Cal_Color_Group_I"),
        (0x2a85f874, "Por_Color_Group_I"),
        (0x4f40e9d2, "Tom_Color_Group_I"),
        (0x47564f94, "Bar_Color_Group_I"),
    ];

    fn extract_pair() -> Option<(Vec<u8>, Vec<u8>)> {
        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let pamt_path = game_root.join("0008").join("0.pamt");
        if !pamt_path.is_file() {
            return None;
        }
        let pamt_data = std::fs::read(&pamt_path).ok()?;
        let pamt = crate::binary::pamt::PackMeta::parse(&pamt_data, None).ok()?;
        let dir = pamt
            .directories
            .iter()
            .find(|d| d.path == "gamedata/binary__/client/bin")?;
        let pabgb_file = dir.files.iter().find(|f| f.name == "dyecolorgroupinfo.pabgb")?;
        let pabgh_file = dir.files.iter().find(|f| f.name == "dyecolorgroupinfo.pabgh")?;
        let group_dir = game_root.join("0008");
        let pabgb = crate::binary::paz::extract_file(
            &group_dir,
            pabgb_file,
            "gamedata/binary__/client/bin",
            &pamt.header.encrypt_info.encrypt_info,
        )
        .ok()?;
        let pabgh = crate::binary::paz::extract_file(
            &group_dir,
            pabgh_file,
            "gamedata/binary__/client/bin",
            &pamt.header.encrypt_info.encrypt_info,
        )
        .ok()?;
        Some((pabgb, pabgh))
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
    fn c_abi_dye_color_group_info_live() {
        let Some((pabgb, pabgh)) = extract_pair() else {
            eprintln!("skipping c_abi_dye_color_group_info_live: no game install");
            return;
        };
        let mut h: *mut CrimsonDyeColorGroupInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_dye_color_group_info_load_from_bytes(
                pabgb.as_ptr(),
                pabgb.len(),
                pabgh.as_ptr(),
                pabgh.len(),
                &mut h,
            )
        };
        assert_eq!(rc, error::OK);
        assert!(!h.is_null());

        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_dye_color_group_info_entry_count(h, &mut count) },
            error::OK
        );
        assert!(count >= 5, "expected >=5 dye color groups, got {count}");

        for &(key, expected) in KNOWN {
            let mut req: usize = 0;
            let rc_size = unsafe {
                crimson_dye_color_group_info_lookup_name(
                    h, key, ptr::null_mut(), 0, &mut req,
                )
            };
            let got = read_string_result(rc_size, req, |b, n, r| unsafe {
                crimson_dye_color_group_info_lookup_name(h, key, b, n, r)
            });
            assert_eq!(got, expected, "DyeColorGroupInfoKey 0x{key:08x} mismatch");
        }

        // Negative — unknown key.
        let mut req: usize = 0;
        let rc = unsafe {
            crimson_dye_color_group_info_lookup_name(
                h,
                u32::MAX,
                ptr::null_mut(),
                0,
                &mut req,
            )
        };
        assert_eq!(rc, error::NOT_FOUND);

        // Enumeration round-trips through the entries vector.
        for i in 0..count {
            let mut key: u32 = 0;
            let mut req: usize = 0;
            let rc_size = unsafe {
                crimson_dye_color_group_info_get_entry(
                    h, i, &mut key, ptr::null_mut(), 0, &mut req,
                )
            };
            let name = read_string_result(rc_size, req, |b, n, r| unsafe {
                crimson_dye_color_group_info_get_entry(h, i, &mut key, b, n, r)
            });
            assert!(!name.is_empty(), "entry {i} has empty name");
            assert_ne!(key, 0, "entry {i} has zero key");
        }

        // OUT_OF_RANGE past the end.
        let mut k: u32 = 0;
        let mut r: usize = 0;
        assert_eq!(
            unsafe {
                crimson_dye_color_group_info_get_entry(
                    h, count, &mut k, ptr::null_mut(), 0, &mut r,
                )
            },
            error::OUT_OF_RANGE,
        );

        unsafe { crimson_dye_color_group_info_free(h) };
    }

    #[test]
    fn c_abi_dye_color_group_info_null_args() {
        let mut h: *mut CrimsonDyeColorGroupInfoHandle = ptr::null_mut();
        assert_eq!(
            unsafe {
                crimson_dye_color_group_info_load_from_bytes(
                    ptr::null(),
                    16,
                    ptr::null(),
                    0,
                    &mut h,
                )
            },
            error::NULL_ARG,
        );
        assert_eq!(
            unsafe {
                crimson_dye_color_group_info_load_from_bytes(
                    [0u8; 1].as_ptr(),
                    1,
                    [0u8; 1].as_ptr(),
                    1,
                    ptr::null_mut(),
                )
            },
            error::NULL_ARG,
        );
        let mut req: usize = 0;
        assert_eq!(
            unsafe {
                crimson_dye_color_group_info_lookup_name(
                    ptr::null(),
                    0,
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            },
            error::NULL_ARG,
        );
    }

    #[test]
    fn c_abi_dye_color_group_info_empty_bytes_yields_empty_handle() {
        let mut h: *mut CrimsonDyeColorGroupInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_dye_color_group_info_load_from_bytes(
                ptr::null(),
                0,
                ptr::null(),
                0,
                &mut h,
            )
        };
        assert_eq!(rc, error::OK);
        assert!(!h.is_null());
        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_dye_color_group_info_entry_count(h, &mut count) },
            error::OK
        );
        assert_eq!(count, 0);
        unsafe { crimson_dye_color_group_info_free(h) };
    }

    /// Pin the palette accessors against the slot103 ground truth:
    /// every observed save RGB (6 Hernand reds + 5 Pororin olives,
    /// from `_probe_item_dye_data_with_mercenary_resolution`) must
    /// reverse-resolve to its exact gradient position, and the
    /// forward lookup must return the same RGB. Closes the BGRA-byte-
    /// order question — if a future patch reorders bytes or shifts
    /// positions, this test fires first.
    #[test]
    fn c_abi_dye_color_group_info_palette_pins_slot103_observations() {
        let Some((pabgb, pabgh)) = crate::dye_color_group_info::extract_pair_for_tests() else {
            eprintln!("skipping: no game install");
            return;
        };
        let mut h: *mut CrimsonDyeColorGroupInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_dye_color_group_info_load_from_bytes(
                pabgb.as_ptr(), pabgb.len(),
                pabgh.as_ptr(), pabgh.len(),
                &mut h,
            )
        };
        assert_eq!(rc, error::OK);

        // (color_group_key, position_idx, expected_r, expected_g, expected_b)
        // from the dye_gradient_vs_slot103_rgbs probe + the BGRA swap.
        const HER: u32 = 0xc88211f5;
        const POR: u32 = 0x2a85f874;
        let cases: &[(u32, u32, u8, u8, u8)] = &[
            (HER, 17, 0xf2, 0x21, 0x21),
            (HER, 43, 0xa6, 0x57, 0x57),
            (HER, 22, 0xd9, 0x85, 0x85),
            (HER, 21, 0xd9, 0x99, 0x99),
            (HER, 70, 0x59, 0x44, 0x44),
            (HER, 44, 0xa6, 0x48, 0x48),
            (POR, 62, 0x73, 0x6e, 0x3f),
            (POR, 85, 0x40, 0x39, 0x13),
            (POR, 73, 0x59, 0x54, 0x2a),
            (POR, 66, 0x73, 0x6a, 0x15),
            (POR, 55, 0x8c, 0x85, 0x30),
        ];

        // 1.07 palettes have 109 positions per theme. Floor at >=50 so
        // a patch with a slightly different size doesn't false-fail.
        let mut size: u32 = 0;
        assert_eq!(
            unsafe { crimson_dye_color_group_info_palette_size(h, HER, &mut size) },
            error::OK,
        );
        assert!(size >= 50, "Her palette too small ({size})");
        assert_eq!(
            unsafe { crimson_dye_color_group_info_palette_size(h, POR, &mut size) },
            error::OK,
        );
        assert!(size >= 50, "Por palette too small ({size})");

        for &(key, idx, r, g, b) in cases {
            // Forward: position_idx → (R, G, B, A)
            let (mut or, mut og, mut ob, mut oa) = (0u8, 0u8, 0u8, 0u8);
            let rc = unsafe {
                crimson_dye_color_group_info_palette_at(
                    h, key, idx, &mut or, &mut og, &mut ob, &mut oa,
                )
            };
            assert_eq!(rc, error::OK, "palette_at(0x{key:08x}, {idx})");
            assert_eq!(
                (or, og, ob, oa),
                (r, g, b, 0xFF),
                "palette_at(0x{key:08x}, {idx}) RGB mismatch",
            );

            // Reverse: (R, G, B) → position_idx
            let mut pos: u32 = u32::MAX;
            let rc = unsafe {
                crimson_dye_color_group_info_position_for_rgb(h, key, r, g, b, &mut pos)
            };
            assert_eq!(rc, error::OK, "position_for_rgb(0x{key:08x}, {r:02x}{g:02x}{b:02x})");
            assert_eq!(pos, idx, "round-trip position mismatch");
        }

        // OOR position
        let (mut a, mut b, mut c, mut d) = (0u8, 0u8, 0u8, 0u8);
        assert_eq!(
            unsafe {
                crimson_dye_color_group_info_palette_at(
                    h, HER, 9999, &mut a, &mut b, &mut c, &mut d,
                )
            },
            error::OUT_OF_RANGE,
        );

        // Unknown key
        assert_eq!(
            unsafe { crimson_dye_color_group_info_palette_size(h, u32::MAX, &mut size) },
            error::NOT_FOUND,
        );
        let mut pos: u32 = 0;
        assert_eq!(
            unsafe {
                crimson_dye_color_group_info_position_for_rgb(h, HER, 0, 0, 0, &mut pos)
            },
            error::NOT_FOUND,
            "RGB (0,0,0) shouldn't be reachable in Hernand palette",
        );

        unsafe { crimson_dye_color_group_info_free(h) };
    }

    #[test]
    fn c_abi_dye_color_group_info_load_bad_path_returns_io() {
        let bad = CString::new("Z:\\definitely\\does\\not\\exist.pabgb").unwrap();
        let bad2 = CString::new("Z:\\definitely\\does\\not\\exist.pabgh").unwrap();
        let mut h: *mut CrimsonDyeColorGroupInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_dye_color_group_info_load_from_file(
                bad.as_ptr(),
                bad2.as_ptr(),
                &mut h,
            )
        };
        assert_eq!(rc, error::IO);
        assert!(h.is_null());
    }
}
