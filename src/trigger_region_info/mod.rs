//! `triggerregioninfo.pabgb` parser — standard PABGH (u32-key).
//!
//! Resolves `TriggerRegionKey (u32)` — terrain-effect zone templates
//! (`Swamp`, `IceTerrain`, `WaterPoison`, …). 12 rows in 1.07. The
//! save layer references these from `FieldGimmickSaveData` blocks that
//! tag a tile-region with one of the engine's hazard / terrain modes.
//!
//! Standard `u16 count + (u32 key, u32 offset)*` PABGH — re-uses
//! `skill_info::parse_pabgh`. Row body starts with `[u32 name_len]
//! [name][u32 key]` (2.04 swapped key and name).

use crate::binary::row_head::read_row_head;

#[derive(Debug, Clone)]
pub struct TriggerRegionInfoEntry {
    pub key: u32,
    pub name: String,
}

pub fn parse_trigger_region_info_lossy(
    pabgb: &[u8],
    pabgh: &[u8],
) -> Vec<TriggerRegionInfoEntry> {
    let Ok(index) = crate::skill_info::parse_pabgh(pabgh) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(index.len());
    for e in &index {
        let Some(body) = pabgb.get(e.offset as usize..) else {
            continue;
        };
        let Some(head) = read_row_head(body, 4) else { continue };
        if head.key != e.key || !(1..=128).contains(&head.name.len()) {
            continue;
        }
        out.push(TriggerRegionInfoEntry {
            key: e.key,
            name: head.name.to_owned(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::binary::gamedata_layout;
    use super::*;
    use std::path::PathBuf;

    const KNOWN: &[(u32, &str)] = &[
        (1000000, "Swamp"),
        (1000001, "IceTerrain"),
        (1000002, "WaterPoison"),
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
            .find(|d| d.path == gamedata_layout::bin_dir())?;
        let group_dir = game_root.join("0008");
        let pabgb = crate::binary::paz::extract_file(
            &group_dir,
            dir.files.iter().find(|f| f.name == gamedata_layout::body("triggerregioninfo"))?,
            gamedata_layout::bin_dir(),
            &pamt.header.encrypt_info.encrypt_info,
        )
        .ok()?;
        let pabgh = crate::binary::paz::extract_file(
            &group_dir,
            dir.files.iter().find(|f| f.name == gamedata_layout::header("triggerregioninfo"))?,
            gamedata_layout::bin_dir(),
            &pamt.header.encrypt_info.encrypt_info,
        )
        .ok()?;
        Some((pabgb, pabgh))
    }

    #[test]
    fn trigger_region_info_lossy_live() {
        let Some((pabgb, pabgh)) = find_table_bytes() else {
            eprintln!("skipping trigger_region_info_lossy_live: no game install");
            return;
        };
        let entries = parse_trigger_region_info_lossy(&pabgb, &pabgh);
        assert_eq!(entries.len(), 14, "expected 14 rows in 2.04 (13 in 1.18-2.03, 12 in 1.07-1.17)");
        let by_key: std::collections::HashMap<u32, &str> =
            entries.iter().map(|e| (e.key, e.name.as_str())).collect();
        for &(k, expected) in KNOWN {
            assert_eq!(by_key.get(&k).copied(), Some(expected), "key {k}");
        }
    }
}
