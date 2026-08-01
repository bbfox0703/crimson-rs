//! `reserveslot.pabgb` bridge — C ABI surface.
//!
//! Resolves save-side `ReserveSlotKey (u32)` to template names
//! (`"ArrowItem"`, `"BulletItem"`, `"BombItem"`, …). Name-only,
//! 27 rows in 1.07.

crate::impl_name_only_bridge! {
    handle = CrimsonReserveSlotInfoHandle,
    parser = crate::reserve_slot_info::parse_reserve_slot_info_lossy,
    entry_ty = crate::reserve_slot_info::ReserveSlotInfoEntry,
    load_from_file = crimson_reserve_slot_info_load_from_file,
    load_from_bytes = crimson_reserve_slot_info_load_from_bytes,
    free = crimson_reserve_slot_info_free,
    entry_count = crimson_reserve_slot_info_entry_count,
    lookup_string_key = crimson_reserve_slot_info_lookup_string_key,
    get_entry = crimson_reserve_slot_info_get_entry,
    key_param = reserve_slot_key,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::c_abi::error;
    use crate::c_abi::paz::crimson_paz_extract_file;
    use std::ffi::{CStr, CString};
    use std::path::PathBuf;
    use std::ptr;

    const KNOWN: &[(u32, &str)] = &[
        (1000000, "ArrowItem"),
        (1000014, "BulletItem"),
        (1000013, "BombItem"),
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

    #[test]
    fn c_abi_reserve_slot_info_live() {
        let Some(pamt_path) = find_pamt() else {
            eprintln!("skipping c_abi_reserve_slot_info_live: no game install");
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let pabgb = extract_file(
            pamt.as_c_str(),
            "gamedata/binary__/client/bin",
            "reserveslot.pabgb",
        );
        let pabgh = extract_file(
            pamt.as_c_str(),
            "gamedata/binary__/client/bin",
            "reserveslot.pabgh",
        );
        let mut sh: *mut CrimsonReserveSlotInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_reserve_slot_info_load_from_bytes(
                pabgb.as_ptr(),
                pabgb.len(),
                pabgh.as_ptr(),
                pabgh.len(),
                &mut sh,
            )
        };
        assert_eq!(rc, error::OK);
        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_reserve_slot_info_entry_count(sh, &mut count) },
            error::OK
        );
        assert_eq!(count, 28); // 1.16 (was 30 in 1.13-1.15, 31 in 1.12, 27 through 1.11)
        for &(key, expected) in KNOWN {
            let mut req: usize = 0;
            assert_eq!(
                unsafe {
                    crimson_reserve_slot_info_lookup_string_key(
                        sh,
                        key,
                        ptr::null_mut(),
                        0,
                        &mut req,
                    )
                },
                error::BUFFER_TOO_SMALL
            );
            let mut buf = vec![0u8; req];
            let mut req2: usize = 0;
            assert_eq!(
                unsafe {
                    crimson_reserve_slot_info_lookup_string_key(
                        sh,
                        key,
                        buf.as_mut_ptr(),
                        buf.len(),
                        &mut req2,
                    )
                },
                error::OK
            );
            let got = std::str::from_utf8(&buf[..req2 - 1]).unwrap();
            assert_eq!(got, expected, "key {key}");
        }
        unsafe { crimson_reserve_slot_info_free(sh) };
    }
}
