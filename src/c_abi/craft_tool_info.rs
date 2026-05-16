//! `crafttoolinfo.pabgb` bridge — C ABI surface.
//!
//! Resolves save-side `CraftToolKey (u16-widened-u32)` to the row's
//! template internal name (`"CraftTool_Enchant"`,
//! `"CraftTool_Enchant_Kukupot"`, `"CraftTool_Burn_Kukupot"`, …).
//! Name-only, 17 rows in 1.07. Built via the shared
//! `impl_name_only_bridge!` macro — see `c_abi/mod.rs` for the
//! generated function shape.

crate::impl_name_only_bridge! {
    handle = CrimsonCraftToolInfoHandle,
    parser = crate::craft_tool_info::parse_craft_tool_info_lossy,
    entry_ty = crate::craft_tool_info::CraftToolInfoEntry,
    load_from_file = crimson_craft_tool_info_load_from_file,
    load_from_bytes = crimson_craft_tool_info_load_from_bytes,
    free = crimson_craft_tool_info_free,
    entry_count = crimson_craft_tool_info_entry_count,
    lookup_string_key = crimson_craft_tool_info_lookup_string_key,
    get_entry = crimson_craft_tool_info_get_entry,
    key_param = craft_tool_key,
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
        (28001, "CraftTool_Enchant"),
        (28002, "CraftTool_Enchant_Kukupot"),
        (28003, "CraftTool_Burn_Kukupot"),
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
    fn c_abi_craft_tool_info_live() {
        let Some(pamt_path) = find_pamt() else {
            eprintln!("skipping c_abi_craft_tool_info_live: no game install");
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let pabgb = extract_file(
            pamt.as_c_str(),
            "gamedata/binary__/client/bin",
            "crafttoolinfo.pabgb",
        );
        let pabgh = extract_file(
            pamt.as_c_str(),
            "gamedata/binary__/client/bin",
            "crafttoolinfo.pabgh",
        );
        let mut sh: *mut CrimsonCraftToolInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_craft_tool_info_load_from_bytes(
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
            unsafe { crimson_craft_tool_info_entry_count(sh, &mut count) },
            error::OK
        );
        assert_eq!(count, 17);
        for &(key, expected) in KNOWN {
            let mut req: usize = 0;
            assert_eq!(
                unsafe {
                    crimson_craft_tool_info_lookup_string_key(
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
                    crimson_craft_tool_info_lookup_string_key(
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
        unsafe { crimson_craft_tool_info_free(sh) };
    }
}
