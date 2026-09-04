//! `gameplayvariableinfo.pabgb` parser — standard PABGH (u32-key).
//!
//! Resolves `GamePlayVariableKey (u32)` — engine-level gameplay
//! switches and progression flags (`CD_Live`, `BaseCamp_Ranch_Lv1`,
//! `BaseCamp_Farm_Lv1`, …). 47 rows in 1.07. The save layer
//! references these as toggle / progression keys.
//!
//! Standard `u16 count + (u32 key, u32 offset)*` PABGH. Row body
//! starts with `[u32 key][u32 name_len][name]`; the trailing body
//! carries the per-row UTF-8 display string and is not consumed here.

#[derive(Debug, Clone)]
pub struct GamePlayVariableInfoEntry {
    pub key: u32,
    pub name: String,
}

pub fn parse_gameplay_variable_info_lossy(
    pabgb: &[u8],
    pabgh: &[u8],
) -> Vec<GamePlayVariableInfoEntry> {
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
        out.push(GamePlayVariableInfoEntry {
            key: e.key,
            name: name.to_owned(),
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
        (1000041, "CD_Live"),
        (1000001, "BaseCamp_Ranch_Lv1"),
        (1000000, "BaseCamp_Ranch_Lv2"),
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
            dir.files.iter().find(|f| f.name == gamedata_layout::body("gameplayvariableinfo"))?,
            gamedata_layout::bin_dir(),
            &pamt.header.encrypt_info.encrypt_info,
        )
        .ok()?;
        let pabgh = crate::binary::paz::extract_file(
            &group_dir,
            dir.files.iter().find(|f| f.name == gamedata_layout::header("gameplayvariableinfo"))?,
            gamedata_layout::bin_dir(),
            &pamt.header.encrypt_info.encrypt_info,
        )
        .ok()?;
        Some((pabgb, pabgh))
    }

    #[test]
    fn gameplay_variable_info_lossy_live() {
        let Some((pabgb, pabgh)) = find_table_bytes() else {
            eprintln!("skipping gameplay_variable_info_lossy_live: no game install");
            return;
        };
        let entries = parse_gameplay_variable_info_lossy(&pabgb, &pabgh);
        assert_eq!(
            entries.len(),
            62,
            "expected 62 rows in 2.01 (59 in 2.00, 56 in 1.16-1.18, 57 in 1.13-1.15, 56 in 1.12, 55 in 1.11, 47 through 1.10)"
        );
        let by_key: std::collections::HashMap<u32, &str> =
            entries.iter().map(|e| (e.key, e.name.as_str())).collect();
        for &(k, expected) in KNOWN {
            assert_eq!(by_key.get(&k).copied(), Some(expected), "key {k}");
        }
    }
}
