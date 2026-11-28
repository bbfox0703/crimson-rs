//! `crafttoolinfo.pabgb` parser — custom-PABGH (u16-key).
//!
//! Resolves `CraftToolKey (u16-widened-u32)` — the per-tool crafting
//! template (`CraftTool_Enchant`, `CraftTool_Enchant_Kukupot`,
//! `CraftTool_Burn_Kukupot`, …). 17 rows in 1.07. Each row groups a
//! crafting-station tool name with its parent group key (via
//! `crafttoolgroupinfo`).
//!
//! `u16 count + (u16 key, u32 offset)*` PABGH. Body starts with
//! `[u32 name_len][name][u16 key]` (2.04 swapped key and name); trailing ~10 B carry the group
//! key reference + sentinels which this bridge doesn't surface.

use crate::binary::row_head::read_row_head;

#[derive(Debug, Clone)]
pub struct CraftToolInfoEntry {
    pub key: u32,
    pub name: String,
}

pub fn parse_craft_tool_info_lossy(
    pabgb: &[u8],
    pabgh: &[u8],
) -> Vec<CraftToolInfoEntry> {
    // 2.06 moved row names into the header; rebuild the pre-2.06 layout.
    let (pabgb, pabgh) = crate::binary::static_header::legacy_pair(pabgb, pabgh);
    let (pabgb, pabgh): (&[u8], &[u8]) = (&pabgb, &pabgh);
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
        let Some(head) = read_row_head(body, 2) else { continue };
        if head.key != u32::from(key16) || !(1..=128).contains(&head.name.len()) {
            continue;
        }
        out.push(CraftToolInfoEntry {
            key: u32::from(key16),
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
        (28001, "CraftTool_Enchant"),
        (28002, "CraftTool_Enchant_Kukupot"),
        (28003, "CraftTool_Burn_Kukupot"),
    ];

    fn find_table_bytes() -> Option<(Vec<u8>, Vec<u8>)> {
        // None only when the install (or the table) is absent; an install
        // that is there but unreadable fails the test (audit I-02).
        Some((
            gamedata_layout::extract_bin(&gamedata_layout::body("crafttoolinfo"))?,
            gamedata_layout::extract_bin(&gamedata_layout::header("crafttoolinfo"))?,
        ))
    }

    #[test]
    fn craft_tool_info_lossy_live() {
        let Some((pabgb, pabgh)) = find_table_bytes() else {
            eprintln!("skipping craft_tool_info_lossy_live: no game install");
            return;
        };
        let entries = parse_craft_tool_info_lossy(&pabgb, &pabgh);
        assert_eq!(entries.len(), 19, "expected 19 rows in 1.12 (was 17 through 1.11)");
        let by_key: std::collections::HashMap<u32, &str> =
            entries.iter().map(|e| (e.key, e.name.as_str())).collect();
        for &(k, expected) in KNOWN {
            assert_eq!(by_key.get(&k).copied(), Some(expected), "key {k}");
        }
    }
}
