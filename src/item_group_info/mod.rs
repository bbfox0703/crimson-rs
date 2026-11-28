//! `itemgroupinfo.pabgb` parser — custom-PABGH (u16-key).
//!
//! Resolves `ItemGroupKey (u16-widened-u32)` — item group / category
//! templates (`ItemGroup_Category_Equipment`,
//! `ItemGroup_Category_Food`, `ItemGroup_Category_Material`, …).
//! 1,682 rows in 2.06. The save layer references these as inventory-
//! filter / drop-table identifiers; ItemInfo also exposes them via
//! `item_group_info_list`.
//!
//! `u16 count + (u16 key, u32 offset)*` PABGH. Body: `[u32 name_len]
//! [name][u16 key]` (2.04 swapped key and name); rest carries the inline member-itemkey list
//! which this bridge does not surface.

use crate::binary::row_head::read_row_head;

#[derive(Debug, Clone)]
pub struct ItemGroupInfoEntry {
    pub key: u32,
    pub name: String,
}

pub fn parse_item_group_info_lossy(
    pabgb: &[u8],
    pabgh: &[u8],
) -> Vec<ItemGroupInfoEntry> {
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
        out.push(ItemGroupInfoEntry {
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
        (18167, "ItemGroup_Category_Equipment"),
        (18171, "ItemGroup_Category_Food"),
        (18172, "ItemGroup_Category_Material"),
    ];

    fn find_table_bytes() -> Option<(Vec<u8>, Vec<u8>)> {
        // None only when the install (or the table) is absent; an install
        // that is there but unreadable fails the test (audit I-02).
        Some((
            gamedata_layout::extract_bin(&gamedata_layout::body("itemgroupinfo"))?,
            gamedata_layout::extract_bin(&gamedata_layout::header("itemgroupinfo"))?,
        ))
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
            1682,
            "expected 1682 rows in 2.06 (1680 in 2.04-2.05.01, 1602 in 2.03, 1600 in 2.01-2.02, 1597 in 1.18-2.00, 1596 in 1.17, 1597 in 1.16, 1550 in 1.13-1.15, 1541 in 1.12, 1521 in 1.10/1.11)"
        );
        let by_key: std::collections::HashMap<u32, &str> =
            entries.iter().map(|e| (e.key, e.name.as_str())).collect();
        for &(k, expected) in KNOWN {
            assert_eq!(by_key.get(&k).copied(), Some(expected), "key {k}");
        }
    }
}
