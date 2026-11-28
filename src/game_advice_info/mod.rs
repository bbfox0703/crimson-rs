//! `gameadviceinfo.pabgb` parser — standard PABGH (u32-key).
//!
//! Resolves `GameAdviceInfoKey (u32)` — gameplay-tip / hint
//! templates surfaced by the in-game advice system (`Advice_Control_Move`,
//! `Advice_Control_Battle`, …). 482 rows in 2.06. Each row carries a
//! pair of 20-byte ASCII PALOC numeric keys that resolve to the
//! localized tip body / title via `localizationstring_eng.paloc`, in
//! the same hash-hop pattern used by mission / quest / stage / knowledge.
//!
//! This bridge ships **name-only** for now — the PALOC chain probe is
//! deferred to a follow-on session. The internal name
//! (`Advice_Control_Move` etc.) is the save-layer's stable identifier
//! and is what most consumers need today.
//!
//! Standard `u16 count + (u32 key, u32 offset)*` PABGH. Row body starts
//! with `[u32 name_len][name][u32 key]` (2.04 swapped key and name).

use crate::binary::row_head::read_row_head;

#[derive(Debug, Clone)]
pub struct GameAdviceInfoEntry {
    pub key: u32,
    pub name: String,
}

pub fn parse_game_advice_info_lossy(
    pabgb: &[u8],
    pabgh: &[u8],
) -> Vec<GameAdviceInfoEntry> {
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
        out.push(GameAdviceInfoEntry {
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
        (0x9cfd99b0, "Advice_Control_Move"),
        (0x6dfa5b08, "Advice_Control_Battle"),
        (0xc6c517f3, "Advice_Control_Interaction_Quest"),
    ];

    fn find_table_bytes() -> Option<(Vec<u8>, Vec<u8>)> {
        // None only when the install (or the table) is absent; an install
        // that is there but unreadable fails the test (audit I-02).
        Some((
            gamedata_layout::extract_bin(&gamedata_layout::body("gameadviceinfo"))?,
            gamedata_layout::extract_bin(&gamedata_layout::header("gameadviceinfo"))?,
        ))
    }

    #[test]
    fn game_advice_info_lossy_live() {
        let Some((pabgb, pabgh)) = find_table_bytes() else {
            eprintln!("skipping game_advice_info_lossy_live: no game install");
            return;
        };
        let entries = parse_game_advice_info_lossy(&pabgb, &pabgh);
        assert_eq!(
            entries.len(),
            482,
            "expected 482 rows in 2.04 (478 in 2.00-2.03, 472 in 1.12-1.18, 470 in 1.10/1.11)"
        );
        let by_key: std::collections::HashMap<u32, &str> =
            entries.iter().map(|e| (e.key, e.name.as_str())).collect();
        for &(k, expected) in KNOWN {
            assert_eq!(by_key.get(&k).copied(), Some(expected), "key 0x{k:08x}");
        }
    }
}
