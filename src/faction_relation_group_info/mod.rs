//! `factionrelationgroup.pabgb` parser — custom-PABGH.
//!
//! Resolves `FactionRelationGroupKey` — a small u16-shaped table that
//! describes how the in-game faction system groups relationships
//! (friendly / hostile / neutral / monster-vs-NPC). 1.07 ships exactly
//! **5 rows**:
//!
//! | key (0x..)  | internal name      |
//! | ----------- | ------------------ |
//! | 0x4243 (16963) | `Graymane`         |
//! | 0x4244 (16964) | `FriendlyCombat`   |
//! | 0x4245 (16965) | `HostileCombat`    |
//! | 0x4246 (16966) | `NPC_Common`       |
//! | 0x4247 (16967) | `Monster_Common`   |
//!
//! **PABGH layout** — custom 6-byte entries (NOT the standard skill /
//! dye_color_group `u16 count + (u32 key, u32 offset)*` 10-byte shape;
//! the keys fit in u16, so PA squeezed them down):
//!
//! ```text
//! u16 count, then count × { u16 key, u32 offset }
//! ```
//!
//! For 5 entries: 2 + 5 × 6 = 32 bytes (matches the live file's
//! 32-byte `.pabgh`).
//!
//! **PABGB row layout** (all 5 rows; 2.04 moved `key` after the name, 2.06
//! moved the name into the header; the content has not changed since
//! 1.03.01):
//!
//! ```text
//! Row {
//!     u32 name_len,
//!     u8  name[name_len],   // ASCII identifier
//!     u16 key,              // matches PABGH key
//!     u8  is_blocked,       // 0 on every row
//!     // _relationGroupList: one list per FactionRelationType, in order
//!     [u32 count, u16 group_key[count]] × 4   // Neutral, War, Agreement, BloodAlly
//! }
//! ```
//!
//! ## The relation lists (2026-11-27)
//!
//! The exe's reflection data names the row's fields `_key`,
//! `_relationGroupList`, `_staticInfoIndex` and `_isBlocked`; the body is
//! `_isBlocked` and then four count-prefixed lists, one per
//! `FactionRelationType` (0 Neutral, 1 War, 2 Agreement, 3 BloodAlly — the
//! name table after the exe's `FactionRelationGroupInfo(%#)…상호 같은 Relation
//! 이어야 합니다` consistency message, which requires the two directions to
//! agree). The bytes earlier read as "5 pad bytes" and "a 4-byte separator"
//! are `_isBlocked` plus the empty Neutral list, and the empty Agreement list.
//!
//! | group | War (1) | BloodAlly (3) |
//! |---|---|---|
//! | `Graymane` (player) | HostileCombat, Monster_Common | FriendlyCombat, NPC_Common |
//! | `FriendlyCombat` | HostileCombat, Monster_Common | Graymane, NPC_Common |
//! | `HostileCombat` | Graymane, FriendlyCombat, Monster_Common | NPC_Common |
//! | `NPC_Common` | Monster_Common | Graymane, FriendlyCombat, HostileCombat |
//! | `Monster_Common` | Graymane, FriendlyCombat, HostileCombat, NPC_Common | — |
//!
//! Neutral and Agreement are empty on every row. The matrix is symmetric, and
//! each of the 10 pairs of distinct groups is in exactly one list, so between
//! two different groups the relation is always War or BloodAlly. A group's
//! relation to itself is not listed. This matches the game: Monster_Common
//! (`Faction_Bandit`) and HostileCombat attack the player, NPC_Common and
//! FriendlyCombat do not, and the save's explicit player ⇄
//! `Faction_Celeste_Hernand` record was already 3 (BloodAlly). An explicit
//! faction relation overrides the group default (verified in game with a
//! BloodAlly record for `Faction_Bandit`).
//!
//! [`FactionRelationGroupEntry::relation_lists`] holds the four lists;
//! [`FactionRelationGroupEntry::related`] is their concatenation (War, then
//! BloodAlly), as the bridge exposed it before.
//!
//! **PALOC chain**: none — the `_probe_faction_paloc_chains` pass
//! returned only coincidental collisions at `lo32 = 0x80` (UI tooltip
//! sentinels). The bridge follows the
//! [`crate::sub_level_info`] pattern: no `lookup_display_name`.

use crate::binary::row_head::read_row_head;

/// One parsed factionrelationgroup row.
#[derive(Debug, Clone)]
pub struct FactionRelationGroupEntry {
    /// `FactionRelationGroupKey` — u16 in the on-disk table; widened
    /// to u32 here so the bridge API stays uniform with the other
    /// faction tables. The high 16 bits are always zero.
    pub key: u32,
    /// ASCII internal name (e.g. `"Graymane"`, `"HostileCombat"`).
    pub name: String,
    /// Every group this one lists, the four relation lists concatenated in
    /// `FactionRelationType` order (in practice War, then BloodAlly).
    pub related: Vec<u32>,
    /// `_relationGroupList`: the groups this one has each relation with,
    /// indexed by `FactionRelationType` (0 Neutral, 1 War, 2 Agreement,
    /// 3 BloodAlly).
    pub relation_lists: [Vec<u32>; RELATION_LIST_COUNT],
}

/// Lists per row: one per `FactionRelationType` from Neutral (0) through
/// BloodAlly (3). `ByGroup` (4) has none: it is what a faction relation says
/// when it defers to this table.
pub const RELATION_LIST_COUNT: usize = 4;

/// Parse `factionrelationgroup.pabgb` using its custom-shape `.pabgh`
/// index (`u16 count + (u16 key, u32 offset)*`).
pub fn parse_faction_relation_group_info_lossy(
    pabgb: &[u8],
    pabgh: &[u8],
) -> Vec<FactionRelationGroupEntry> {
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
    // Pull (key, offset) pairs and a sort-by-offset view to compute
    // each row's end-of-body.
    let mut entries: Vec<(u16, u32)> = Vec::with_capacity(count);
    for i in 0..count {
        let off = 2 + i * 6;
        let k = u16::from_le_bytes([pabgh[off], pabgh[off + 1]]);
        let o = u32::from_le_bytes([
            pabgh[off + 2],
            pabgh[off + 3],
            pabgh[off + 4],
            pabgh[off + 5],
        ]);
        entries.push((k, o));
    }
    let mut offs: Vec<u32> = entries.iter().map(|(_, o)| *o).collect();
    offs.push(pabgb.len() as u32);
    offs.sort_unstable();

    let mut out = Vec::with_capacity(count);
    for (key16, off) in &entries {
        let start = *off as usize;
        let end = offs
            .iter()
            .find(|o| **o as usize > start)
            .copied()
            .unwrap_or(pabgb.len() as u32) as usize;
        let Some(body) = pabgb.get(start..end) else {
            continue;
        };
        let Some(head) = read_row_head(body, 2) else {
            continue;
        };
        if head.key != u32::from(*key16) || !(1..=128).contains(&head.name.len()) {
            continue;
        }
        // `_isBlocked`, then one `[u32 count][u16 × count]` list per
        // FactionRelationType. Tolerate truncation — a row whose lists do
        // not parse still surfaces the (key, name) pair with whatever was
        // read so far.
        let mut relation_lists: [Vec<u32>; RELATION_LIST_COUNT] = Default::default();
        let mut cursor = head.body_at + 1; // skip `_isBlocked`
        for list in relation_lists.iter_mut() {
            if cursor + 4 > body.len() {
                break;
            }
            let n = u32::from_le_bytes([
                body[cursor],
                body[cursor + 1],
                body[cursor + 2],
                body[cursor + 3],
            ]) as usize;
            cursor += 4;
            if n > 64 || cursor + n * 2 > body.len() {
                break;
            }
            for i in 0..n {
                let off = cursor + i * 2;
                list.push(u32::from(u16::from_le_bytes([body[off], body[off + 1]])));
            }
            cursor += n * 2;
        }
        out.push(FactionRelationGroupEntry {
            key: u32::from(*key16),
            name: head.name.to_owned(),
            related: relation_lists.concat(),
            relation_lists,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    //! Live-install integration test against the real
    //! `factionrelationgroup.pabgb` + `.pabgh`.

    use crate::binary::gamedata_layout;
    use super::*;

    /// (FactionRelationGroupKey (u16-widened-to-u32), expected
    /// internal_name). All five 1.07 rows pinned.
    const KNOWN: &[(u32, &str)] = &[
        (0x4243, "Graymane"),
        (0x4244, "FriendlyCombat"),
        (0x4245, "HostileCombat"),
        (0x4246, "NPC_Common"),
        (0x4247, "Monster_Common"),
    ];

    fn find_table_bytes() -> Option<(Vec<u8>, Vec<u8>)> {
        // None only when the install (or the table) is absent; an install
        // that is there but unreadable fails the test (audit I-02).
        Some((
            gamedata_layout::extract_bin(&gamedata_layout::body("factionrelationgroup"))?,
            gamedata_layout::extract_bin(&gamedata_layout::header("factionrelationgroup"))?,
        ))
    }

    #[test]
    fn faction_relation_group_info_lossy_live() {
        let Some((pabgb, pabgh)) = find_table_bytes() else {
            eprintln!("skipping faction_relation_group_info_lossy_live: no game install");
            return;
        };
        let entries = parse_faction_relation_group_info_lossy(&pabgb, &pabgh);
        println!(
            "parsed {} factionrelationgroup entries from {} byte pabgb",
            entries.len(),
            pabgb.len()
        );
        assert_eq!(entries.len(), 5, "expected exactly 5 relation-group rows in 1.07");
        let by_key: std::collections::HashMap<u32, &FactionRelationGroupEntry> =
            entries.iter().map(|e| (e.key, e)).collect();
        for &(key, expected) in KNOWN {
            assert_eq!(
                by_key.get(&key).map(|e| e.name.as_str()),
                Some(expected),
                "FactionRelationGroupKey 0x{key:04x} mismatch",
            );
        }
        // Each row references at least one sibling row in its relation
        // lists; the body-parse guard surfaces zero refs only on
        // truncation. All five rows in 1.07 have non-empty references.
        for e in &entries {
            assert!(
                !e.related.is_empty(),
                "row {:?} has no related references — body parse may have aborted early",
                e.name,
            );
            for r in &e.related {
                assert!(
                    by_key.contains_key(r),
                    "row {} references unknown key 0x{:04x}",
                    e.name,
                    r,
                );
            }
        }

        // The relation matrix: symmetric, every pair of distinct groups in
        // exactly one list, Neutral and Agreement empty.
        let relation = |a: u32, b: u32| -> Option<usize> {
            by_key[&a].relation_lists.iter().position(|l| l.contains(&b))
        };
        for a in by_key.keys() {
            let e = by_key[a];
            assert!(e.relation_lists[0].is_empty() && e.relation_lists[2].is_empty(), "{}", e.name);
            for b in by_key.keys() {
                if a == b {
                    assert_eq!(relation(*a, *b), None, "{} lists itself", e.name);
                } else {
                    let r = relation(*a, *b);
                    assert!(r.is_some(), "0x{a:04x} -> 0x{b:04x} unlisted");
                    assert_eq!(r, relation(*b, *a), "0x{a:04x} <-> 0x{b:04x} not symmetric");
                }
            }
        }
        const WAR: Option<usize> = Some(1);
        const BLOOD_ALLY: Option<usize> = Some(3);
        assert_eq!(relation(0x4243, 0x4247), WAR, "Graymane vs Monster_Common");
        assert_eq!(relation(0x4243, 0x4245), WAR, "Graymane vs HostileCombat");
        assert_eq!(relation(0x4243, 0x4246), BLOOD_ALLY, "Graymane vs NPC_Common");
        assert_eq!(relation(0x4243, 0x4244), BLOOD_ALLY, "Graymane vs FriendlyCombat");
        assert_eq!(relation(0x4245, 0x4246), BLOOD_ALLY, "HostileCombat vs NPC_Common");
    }
}
