//! `itemgroupinfo.pabgb` parser — custom-PABGH (u16-key).
//!
//! Resolves `ItemGroupKey (u16-widened-u32)` — item group / category
//! templates (`ItemGroup_Category_Equipment`,
//! `ItemGroup_Category_Food`, `ItemGroup_Category_Material`, …).
//! 1,500 rows in 1.07. The save layer references these as inventory-
//! filter / drop-table identifiers; ItemInfo also exposes them via
//! `item_group_info_list`.
//!
//! `u16 count + (u16 key, u32 offset)*` PABGH. Body: `[u16 key][u32
//! name_len][name]`; rest carries the inline member-itemkey list
//! which this bridge does not surface.

#[derive(Debug, Clone)]
pub struct ItemGroupInfoEntry {
    pub key: u32,
    pub name: String,
}

pub fn parse_item_group_info_lossy(
    pabgb: &[u8],
    pabgh: &[u8],
) -> Vec<ItemGroupInfoEntry> {
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
        out.push(ItemGroupInfoEntry {
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
        (18167, "ItemGroup_Category_Equipment"),
        (18171, "ItemGroup_Category_Food"),
        (18172, "ItemGroup_Category_Material"),
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
            dir.files.iter().find(|f| f.name == "itemgroupinfo.pabgb")?,
            "gamedata/binary__/client/bin",
            &pamt.header.encrypt_info.encrypt_info,
        )
        .ok()?;
        let pabgh = crate::binary::paz::extract_file(
            &group_dir,
            dir.files.iter().find(|f| f.name == "itemgroupinfo.pabgh")?,
            "gamedata/binary__/client/bin",
            &pamt.header.encrypt_info.encrypt_info,
        )
        .ok()?;
        Some((pabgb, pabgh))
    }

    #[test]
    fn item_group_info_lossy_live() {
        let Some((pabgb, pabgh)) = find_table_bytes() else {
            eprintln!("skipping item_group_info_lossy_live: no game install");
            return;
        };
        let entries = parse_item_group_info_lossy(&pabgb, &pabgh);
        assert_eq!(
            entries.len(),
            1597,
            "expected 1597 rows in 1.16 (was 1550 in 1.13-1.15, 1541 in 1.12, 1521 in 1.10/1.11)"
        );
        let by_key: std::collections::HashMap<u32, &str> =
            entries.iter().map(|e| (e.key, e.name.as_str())).collect();
        for &(k, expected) in KNOWN {
            assert_eq!(by_key.get(&k).copied(), Some(expected), "key {k}");
        }
    }
}
