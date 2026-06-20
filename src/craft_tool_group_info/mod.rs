//! `crafttoolgroupinfo.pabgb` parser — custom-PABGH (u16-key).
//!
//! Resolves `CraftToolGroupKey (u16-widened-u32)` — the parent group
//! template for crafting tools (`CraftTool_Equip_Enchant`,
//! `CraftTool_Kukupot`, `CraftTool_Cook`, …). 10 rows in 1.07. Each
//! group references the `crafttoolinfo` rows that belong to it.
//!
//! `u16 count + (u16 key, u32 offset)*` PABGH. Body starts with
//! `[u16 key][u32 name_len][name]`; bridge does not expose the member
//! list (the inline u16 children-keys at the row's tail).

#[derive(Debug, Clone)]
pub struct CraftToolGroupInfoEntry {
    pub key: u32,
    pub name: String,
}

pub fn parse_craft_tool_group_info_lossy(
    pabgb: &[u8],
    pabgh: &[u8],
) -> Vec<CraftToolGroupInfoEntry> {
    if pabgh.len() < 2 {
        return Vec::new();
    }
    let count = u16::from_le_bytes([pabgh[0], pabgh[1]]) as usize;
    if pabgh.len() != 2 + count * 6 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let pos = 2 + i * 6;
        let key16 = u16::from_le_bytes([pabgh[pos], pabgh[pos + 1]]);
        let off = u32::from_le_bytes([
            pabgh[pos + 2],
            pabgh[pos + 3],
            pabgh[pos + 4],
            pabgh[pos + 5],
        ]) as usize;
        let Some(body) = pabgb.get(off..) else { continue };
        if body.len() < 6 {
            continue;
        }
        if u16::from_le_bytes([body[0], body[1]]) != key16 {
            continue;
        }
        let name_len =
            u32::from_le_bytes([body[2], body[3], body[4], body[5]]) as usize;
        if !(1..=128).contains(&name_len) || 6 + name_len > body.len() {
            continue;
        }
        let Ok(name) = std::str::from_utf8(&body[6..6 + name_len]) else {
            continue;
        };
        out.push(CraftToolGroupInfoEntry {
            key: u32::from(key16),
            name: name.to_owned(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const KNOWN: &[(u32, &str)] = &[
        (16960, "CraftTool_Equip_Enchant"),
        (16961, "CraftTool_Kukupot"),
        (16962, "CraftTool_Cook"),
    ];

    fn find_table_bytes() -> Option<(Vec<u8>, Vec<u8>)> {
        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let pamt_path = game_root.join("0008").join("0.pamt");
        if !pamt_path.is_file() {
            return None;
        }
        let pamt_bytes = std::fs::read(&pamt_path).ok()?;
        let pamt = crate::binary::pamt::PackMeta::parse(&pamt_bytes, None).ok()?;
        let dir = pamt
            .directories
            .iter()
            .find(|d| d.path == "gamedata/binary__/client/bin")?;
        let group_dir = game_root.join("0008");
        let pabgb = crate::binary::paz::extract_file(
            &group_dir,
            dir.files.iter().find(|f| f.name == "crafttoolgroupinfo.pabgb")?,
            "gamedata/binary__/client/bin",
            &pamt.header.encrypt_info.encrypt_info,
        )
        .ok()?;
        let pabgh = crate::binary::paz::extract_file(
            &group_dir,
            dir.files.iter().find(|f| f.name == "crafttoolgroupinfo.pabgh")?,
            "gamedata/binary__/client/bin",
            &pamt.header.encrypt_info.encrypt_info,
        )
        .ok()?;
        Some((pabgb, pabgh))
    }

    #[test]
    fn craft_tool_group_info_lossy_live() {
        let Some((pabgb, pabgh)) = find_table_bytes() else {
            eprintln!("skipping craft_tool_group_info_lossy_live: no game install");
            return;
        };
        let entries = parse_craft_tool_group_info_lossy(&pabgb, &pabgh);
        assert_eq!(entries.len(), 12, "expected 12 rows in 1.12 (was 10 through 1.11)");
        let by_key: std::collections::HashMap<u32, &str> =
            entries.iter().map(|e| (e.key, e.name.as_str())).collect();
        for &(k, expected) in KNOWN {
            assert_eq!(by_key.get(&k).copied(), Some(expected), "key {k}");
        }
    }
}
