//! `reserveslot.pabgb` parser — standard PABGH (u32-key).
//!
//! Resolves `ReserveSlotKey (u32)` — equip-reserve / hot-slot template
//! IDs (`ArrowItem`, `BulletItem`, `BombItem`, …).
//! Save references these from the hot-slot UI's reserve-slot
//! assignments. Each row body also carries an inline Korean (Hangul)
//! display string, which this parser does not read.
//!
//! Standard `u16 count + (u32 key, u32 offset)*` PABGH. Body:
//! `[u32 name_len][name][u32 key]`.

use crate::binary::row_head::read_row_head;

#[derive(Debug, Clone)]
pub struct ReserveSlotInfoEntry {
    pub key: u32,
    pub name: String,
}

pub fn parse_reserve_slot_info_lossy(
    pabgb: &[u8],
    pabgh: &[u8],
) -> Vec<ReserveSlotInfoEntry> {
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
        out.push(ReserveSlotInfoEntry {
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
        (1000000, "ArrowItem"),
        (1000014, "BulletItem"),
        (1000013, "BombItem"),
    ];

    fn find_table_bytes() -> Option<(Vec<u8>, Vec<u8>)> {
        // None only when the install (or the table) is absent; an install
        // that is there but unreadable fails the test.
        Some((
            gamedata_layout::extract_bin(&gamedata_layout::body("reserveslot"))?,
            gamedata_layout::extract_bin(&gamedata_layout::header("reserveslot"))?,
        ))
    }

    #[test]
    fn reserve_slot_info_lossy_live() {
        let Some((pabgb, pabgh)) = find_table_bytes() else {
            eprintln!("skipping reserve_slot_info_lossy_live: no game install");
            return;
        };
        let entries = parse_reserve_slot_info_lossy(&pabgb, &pabgh);
        assert_eq!(
            entries.len(),
            28,
            "expected 28 rows in 1.16 (was 30 in 1.13-1.15, 31 in 1.12, 27 through 1.11)"
        );
        let by_key: std::collections::HashMap<u32, &str> =
            entries.iter().map(|e| (e.key, e.name.as_str())).collect();
        for &(k, expected) in KNOWN {
            assert_eq!(by_key.get(&k).copied(), Some(expected), "key {k}");
        }
    }
}
