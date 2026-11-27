//! `faction.pabgb` bridge — C ABI surface.
//!
//! Resolves save-side `FactionKey (u32)` to the row's internal name
//! (`"Faction_Graymane"`, `"Faction_ThiefDusksong"`, …). Name-only:
//! faction rows carry no localizable string, and the PALOC byte-`0x30`
//! lookup a `FactionKey` used to go through answers with *character*
//! names (see `src/faction_info/mod.rs`). 135 rows in 2.06.

crate::impl_name_only_bridge! {
    handle = CrimsonFactionInfoHandle,
    parser = crate::faction_info::parse_faction_info_lossy,
    entry_ty = crate::faction_info::FactionInfoEntry,
    load_from_file = crimson_faction_info_load_from_file,
    load_from_bytes = crimson_faction_info_load_from_bytes,
    free = crimson_faction_info_free,
    entry_count = crimson_faction_info_entry_count,
    lookup_string_key = crimson_faction_info_lookup_string_key,
    get_entry = crimson_faction_info_get_entry,
    key_param = faction_key,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binary::gamedata_layout;
    use crate::c_abi::error;
    use crate::c_abi::paz::crimson_paz_extract_file;
    use std::ffi::{CStr, CString};
    use std::path::PathBuf;
    use std::ptr;

    const KNOWN: &[(u32, &str)] = &[
        (1000000, "Faction_Graymane"),
        (1000039, "Faction_Byron"),
        (1000065, "Faction_ThiefDusksong"),
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

    fn lookup(h: *const CrimsonFactionInfoHandle, key: u32) -> Option<String> {
        let mut req: usize = 0;
        let rc = unsafe {
            crimson_faction_info_lookup_string_key(h, key, ptr::null_mut(), 0, &mut req)
        };
        if rc == error::NOT_FOUND {
            return None;
        }
        assert_eq!(rc, error::BUFFER_TOO_SMALL);
        let mut buf = vec![0u8; req];
        let rc = unsafe {
            crimson_faction_info_lookup_string_key(h, key, buf.as_mut_ptr(), buf.len(), &mut req)
        };
        assert_eq!(rc, error::OK);
        Some(std::str::from_utf8(&buf[..req - 1]).unwrap().to_owned())
    }

    #[test]
    fn c_abi_faction_info_live() {
        let Some(pamt_path) = find_pamt() else {
            eprintln!("skipping c_abi_faction_info_live: no game install");
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let body = extract_file(
            pamt.as_c_str(),
            gamedata_layout::bin_dir(),
            &gamedata_layout::body("faction"),
        );
        let header = extract_file(
            pamt.as_c_str(),
            gamedata_layout::bin_dir(),
            &gamedata_layout::header("faction"),
        );
        let mut h: *mut CrimsonFactionInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_faction_info_load_from_bytes(
                body.as_ptr(),
                body.len(),
                header.as_ptr(),
                header.len(),
                &mut h,
            )
        };
        assert_eq!(rc, error::OK);
        let mut count: u32 = 0;
        assert_eq!(unsafe { crimson_faction_info_entry_count(h, &mut count) }, error::OK);
        assert!(count >= 130, "expected >=130 faction rows, got {count}");
        for &(key, expected) in KNOWN {
            assert_eq!(lookup(h, key).as_deref(), Some(expected), "FactionKey {key}");
        }
        assert_eq!(lookup(h, u32::MAX), None);
        unsafe { crimson_faction_info_free(h) };
    }
}
