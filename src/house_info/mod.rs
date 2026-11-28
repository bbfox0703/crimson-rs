//! `houseinfo.pabgb` parser — custom-PABGH (u16-key).
//!
//! Resolves `HouseKey (u16-widened-u32)` — the template ID of the
//! player-housing tiers (`DefaultHouse_Lv1`, `DefaultHouse_Lv2`,
//! `DefaultHouse_Lv3`, plus one variant) referenced by save-side
//! housing blocks.
//!
//! 4 rows in 1.07. PABGH layout is the same custom 6-byte
//! `(u16 key, u32 offset)*` shape used by `factionrelationgroup`,
//! `storeinfo`, and `partprefabdyetexturepalleteinfo`.
//!
//! Row body — bridge consumes the leading `(u32 name_len, name, u16 key)`
//! triple (2.04 swapped key and name). The rest of each row (~140 B of asset references — a
//! decimal-ASCII ID and `KliffHouse_Hernand_…` model name) is not yet
//! surfaced; can be added later if the editor expands beyond name.

use crate::binary::row_head::read_row_head;

/// One parsed houseinfo row.
#[derive(Debug, Clone)]
pub struct HouseInfoEntry {
    /// `HouseKey` (u16 in the on-disk table, widened to u32 here).
    pub key: u32,
    /// ASCII internal name (e.g. `"DefaultHouse_Lv1"`).
    pub name: String,
}

/// Parse `houseinfo.pabgb` using its `(u16 key, u32 offset)*` PABGH.
pub fn parse_house_info_lossy(pabgb: &[u8], pabgh: &[u8]) -> Vec<HouseInfoEntry> {
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
        out.push(HouseInfoEntry {
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
        (0x4247, "DefaultHouse_Lv1"),
        (0x4248, "DefaultHouse_Lv2"),
        (0x4249, "DefaultHouse_Lv3"),
    ];

    fn find_table_bytes() -> Option<(Vec<u8>, Vec<u8>)> {
        // None only when the install (or the table) is absent; an install
        // that is there but unreadable fails the test (audit I-02).
        Some((
            gamedata_layout::extract_bin(&gamedata_layout::body("houseinfo"))?,
            gamedata_layout::extract_bin(&gamedata_layout::header("houseinfo"))?,
        ))
    }

    #[test]
    fn house_info_lossy_live() {
        let Some((pabgb, pabgh)) = find_table_bytes() else {
            eprintln!("skipping house_info_lossy_live: no game install");
            return;
        };
        let entries = parse_house_info_lossy(&pabgb, &pabgh);
        assert_eq!(entries.len(), 4, "expected 4 rows in 2.04 (22 in 2.01-2.03, 12 in 1.18-2.00, 4 in 1.07-1.17)");
        let by_key: std::collections::HashMap<u32, &str> =
            entries.iter().map(|e| (e.key, e.name.as_str())).collect();
        for &(k, expected) in KNOWN {
            assert_eq!(by_key.get(&k).copied(), Some(expected), "key 0x{k:04x}");
        }
    }
}
