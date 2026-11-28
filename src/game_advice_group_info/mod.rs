//! `gameadvicegroupinfo.pabgb` parser — standard PABGH (u32-key).
//!
//! Resolves `GameAdviceGroupKey (u32)` — the parent grouping template
//! (`GameAdviceGroup_ControlBasics`, `GameAdviceGroup_Interaction`,
//! `GameAdviceGroup_AdventureBasics`, …). 8 rows in 1.07. Pairs with
//! `gameadviceinfo` to break the 482 tips into UI categories.
//!
//! Row body starts with `[u32 name_len][name][u32 key]` (2.04 swapped key
//! and name).

use crate::binary::row_head::read_row_head;

#[derive(Debug, Clone)]
pub struct GameAdviceGroupInfoEntry {
    pub key: u32,
    pub name: String,
}

pub fn parse_game_advice_group_info_lossy(
    pabgb: &[u8],
    pabgh: &[u8],
) -> Vec<GameAdviceGroupInfoEntry> {
    // 2.06 moved row names into the header; rebuild the pre-2.06 layout.
    let (pabgb, pabgh) = crate::binary::static_header::legacy_pair(pabgb, pabgh);
    let (pabgb, pabgh): (&[u8], &[u8]) = (&pabgb, &pabgh);
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
        out.push(GameAdviceGroupInfoEntry {
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

    const KNOWN: &[(u32, &str)] = &[
        (1000008, "GameAdviceGroup_ControlBasics"),
        (1000003, "GameAdviceGroup_Interaction"),
        (1000007, "GameAdviceGroup_AdventureBasics"),
    ];

    fn find_table_bytes() -> Option<(Vec<u8>, Vec<u8>)> {
        // None only when the install (or the table) is absent; an install
        // that is there but unreadable fails the test (audit I-02).
        Some((
            gamedata_layout::extract_bin(&gamedata_layout::body("gameadvicegroupinfo"))?,
            gamedata_layout::extract_bin(&gamedata_layout::header("gameadvicegroupinfo"))?,
        ))
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
