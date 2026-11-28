//! `royalsupply.pabgb` parser — custom-PABGH (u16-key).
//!
//! Resolves `RoyalSupplyKey (u16-widened-u32)` — the per-region royal
//! supply / faction-tribute template (`RoyalSupply_Hernand`,
//! `RoyalSupply_Demeniss`, `RoyalSupply_Varnia`, + 1 variant).
//! 4 rows in 1.07. PABGH layout: `u16 count + (u16 key, u32 offset)*`.
//! Body starts with `[u32 name_len][name][u16 key]` (2.04 swapped key
//! and name); remaining
//! ~1.6 KB / row carries the supply-item list which this bridge does
//! not consume.

use crate::binary::row_head::read_row_head;

/// One parsed royalsupply row.
#[derive(Debug, Clone)]
pub struct RoyalSupplyInfoEntry {
    pub key: u32,
    pub name: String,
}

/// Parse `royalsupply.pabgb` using its `(u16 key, u32 offset)*` PABGH.
pub fn parse_royal_supply_info_lossy(
    pabgb: &[u8],
    pabgh: &[u8],
) -> Vec<RoyalSupplyInfoEntry> {
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
        out.push(RoyalSupplyInfoEntry {
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
        (0x4242, "RoyalSupply_Hernand"),
        (0x4243, "RoyalSupply_Demeniss"),
        (0x4245, "RoyalSupply_Varnia"),
    ];

    fn find_table_bytes() -> Option<(Vec<u8>, Vec<u8>)> {
        // None only when the install (or the table) is absent; an install
        // that is there but unreadable fails the test (audit I-02).
        Some((
            gamedata_layout::extract_bin(&gamedata_layout::body("royalsupply"))?,
            gamedata_layout::extract_bin(&gamedata_layout::header("royalsupply"))?,
        ))
    }

    #[test]
    fn royal_supply_info_lossy_live() {
        let Some((pabgb, pabgh)) = find_table_bytes() else {
            eprintln!("skipping royal_supply_info_lossy_live: no game install");
            return;
        };
        let entries = parse_royal_supply_info_lossy(&pabgb, &pabgh);
        assert_eq!(entries.len(), 4, "expected 4 rows in 1.07");
        let by_key: std::collections::HashMap<u32, &str> =
            entries.iter().map(|e| (e.key, e.name.as_str())).collect();
        for &(k, expected) in KNOWN {
            assert_eq!(by_key.get(&k).copied(), Some(expected), "key 0x{k:04x}");
        }
    }
}
