//! `gameadvicegroupinfo.pabgb` parser — standard PABGH (u32-key).
//!
//! Resolves `GameAdviceGroupKey (u32)` — the parent grouping template
//! (`GameAdviceGroup_ControlBasics`, `GameAdviceGroup_Interaction`,
//! `GameAdviceGroup_AdventureBasics`, …). 8 rows in 1.07. Pairs with
//! `gameadviceinfo` to break the 461 tips into UI categories.

#[derive(Debug, Clone)]
pub struct GameAdviceGroupInfoEntry {
    pub key: u32,
    pub name: String,
}

pub fn parse_game_advice_group_info_lossy(
    pabgb: &[u8],
    pabgh: &[u8],
) -> Vec<GameAdviceGroupInfoEntry> {
    let Ok(index) = crate::skill_info::parse_pabgh(pabgh) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(index.len());
    for e in &index {
        let Some(body) = pabgb.get(e.offset as usize..) else {
            continue;
        };
        if body.len() < 8 {
            continue;
        }
        let body_key = u32::from_le_bytes([body[0], body[1], body[2], body[3]]);
        if body_key != e.key {
            continue;
        }
        let name_len =
            u32::from_le_bytes([body[4], body[5], body[6], body[7]]) as usize;
        if !(1..=128).contains(&name_len) || 8 + name_len > body.len() {
            continue;
        }
        let Ok(name) = std::str::from_utf8(&body[8..8 + name_len]) else {
            continue;
        };
        out.push(GameAdviceGroupInfoEntry {
            key: e.key,
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
        (1000008, "GameAdviceGroup_ControlBasics"),
        (1000003, "GameAdviceGroup_Interaction"),
        (1000007, "GameAdviceGroup_AdventureBasics"),
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
            dir.files.iter().find(|f| f.name == "gameadvicegroupinfo.pabgb")?,
            "gamedata/binary__/client/bin",
            &pamt.header.encrypt_info.encrypt_info,
        )
        .ok()?;
        let pabgh = crate::binary::paz::extract_file(
            &group_dir,
            dir.files.iter().find(|f| f.name == "gameadvicegroupinfo.pabgh")?,
            "gamedata/binary__/client/bin",
            &pamt.header.encrypt_info.encrypt_info,
        )
        .ok()?;
        Some((pabgb, pabgh))
    }

    #[test]
    fn game_advice_group_info_lossy_live() {
        let Some((pabgb, pabgh)) = find_table_bytes() else {
            eprintln!("skipping game_advice_group_info_lossy_live: no game install");
            return;
        };
        let entries = parse_game_advice_group_info_lossy(&pabgb, &pabgh);
        assert_eq!(entries.len(), 8, "expected 8 rows in 1.07");
        let by_key: std::collections::HashMap<u32, &str> =
            entries.iter().map(|e| (e.key, e.name.as_str())).collect();
        for &(k, expected) in KNOWN {
            assert_eq!(by_key.get(&k).copied(), Some(expected), "key {k}");
        }
    }
}
