//! `faction.pabgb` parser — PABGH-indexed.
//!
//! Resolves `FactionKey (u32)` — the key a save carries in
//! `FactionElementSaveData._factionKey`,
//! `FactionRelationElementSaveData._targetFactionKey` and every other
//! field the save schema types `FactionKey`. Rows are named after the
//! faction, `Faction_Graymane` (1000000, the player's Greymanes),
//! `Faction_ThiefDusksong`, `Faction_GoblinSandfang`, ... 135 rows in
//! 2.06, plus a `faction.dlc_01` copy in the DLC tree.
//!
//! Schema (2.06 live install):
//!
//! ```text
//! Row {
//!     u32 name_len,
//!     u8 name[name_len], // ASCII identifier, e.g. "Faction_Graymane"
//!     u32 key,           // matches PABGH key (2.04 moved it after the name)
//!     [u8; 3]  (zero on 130 of 135 rows; bytes 1-2 hold 1..3 on five, e.g. Faction_Graymane),
//!     u16 faction_group_key,          // factiongroup, e.g. 16965 FactionGroup_Graymane
//!     u16 faction_relation_group_key, // factionrelationgroup, e.g. 16963 Graymane
//!     u32 (1 on most rows),
//!     u32 StringInfoKey (hash of "" on every row read),
//!     u32 StringInfoKey flag icon, e.g. "FactionFlagIcon(Graymane)",
//!     [...variable body this bridge does not consume]
//! }
//! ```
//!
//! (On 2.06 the name sits in the `.staticinfoheader`; the body row opens
//! `[key][u32 row index]` — [`crate::binary::static_header::legacy_pair`]
//! rebuilds the layout above.)
//!
//! PABGH layout is the standard `u16 count + (u32 key, u32 offset)*`
//! (re-used via [`crate::skill_info::parse_pabgh`]).
//!
//! **PALOC chain: none.** A faction row carries no localizable string —
//! the slot where `factiongroup` rows embed theirs
//! (`[u8 0x21][u32 lo][u32 hi][u32 len][decimal PALOC key]`) is all
//! zeros here. The `faction.paloc` namespace holds factiongroup names
//! (`hi` = factiongroup key, `lo` = 0x214: 16965 → "Greymanes") and
//! faction-operation strings, nothing keyed by `FactionKey`. Looking a
//! `FactionKey` up at PALOC type byte `0x30` (what the editor used to do)
//! lands in `character.paloc` — `CharacterKey 1000000` is
//! "Hernandian Guard", `CharacterKey 1000035` is "Dusksong" — because
//! factions and characters share the 1,000,000+ key range. The internal
//! name is the only faction-keyed name the game ships.
//!
//! **Group names (2026-11-27).** What the game does show is the faction
//! *group*: every row's `faction_group_key` names a `factiongroup` row, and
//! its display name is PALOC `(faction_group_key << 32) | 0x214`
//! ([`FACTION_GROUP_PALOC_LO`]) in `faction.paloc`. On 2.06, 128 of the 135
//! rows have a group (the other 7 read 0: `Faction_Empty_*`,
//! `Faction_BountyHunter`, `Faction_CrowBrothers`, `Faction_Ancient_People`,
//! `Faction_Golems`, `Faction_Hexe_Sanctuary`) and all 7 `factiongroup` rows
//! have an English name: 16965 "Greymanes", 16960 "Hernand", 16966 "Abyss", ….
//! `faction_relation_group_key` is a `factionrelationgroup` key on all 135
//! rows (16963 `Graymane`, 16964 `FriendlyCombat`, 16966 `NPC_Common`, …).

use crate::binary::row_head::read_row_head;

/// One parsed faction row, reduced to what the bridge consumes.
#[derive(Debug, Clone)]
pub struct FactionInfoEntry {
    /// `FactionKey` — matches the PABGH key.
    pub key: u32,
    /// ASCII internal name (e.g. `"Faction_Graymane"`).
    pub name: String,
    /// `factiongroup` key (e.g. 16965 `FactionGroup_Graymane`), 0 for none.
    pub group_key: u16,
    /// `factionrelationgroup` key (e.g. 16963 `Graymane`).
    pub relation_group_key: u16,
}

/// PALOC `lo32` of a faction group's display name: the string key is
/// `(faction_group_key << 32) | FACTION_GROUP_PALOC_LO` (`faction.paloc`).
pub const FACTION_GROUP_PALOC_LO: u32 = 0x214;

/// Parse `faction.pabgb` using its `.pabgh` index (or a 2.06
/// `.staticinfobody` with its `.staticinfoheader`).
///
/// Returns entries in PABGH on-disk order. Skips any row whose body key
/// disagrees with the PABGH key. The group keys sit at fixed offsets, so a row
/// too short to hold both means the layout moved: the whole table then reads
/// as empty (the C ABI loader returns `BODY_PARSE`) instead of reporting
/// group 0, which reads as "no group".
pub fn parse_faction_info_lossy(pabgb: &[u8], pabgh: &[u8]) -> Vec<FactionInfoEntry> {
    // 2.06 moved row names into the header; rebuild the pre-2.06 layout.
    let (pabgb, pabgh) = crate::binary::static_header::legacy_pair(pabgb, pabgh);
    let (pabgb, pabgh): (&[u8], &[u8]) = (&pabgb, &pabgh);
    let Ok(index) = crate::skill_info::parse_pabgh(pabgh) else {
        return Vec::new();
    };
    let ranges = crate::skill_info::entry_ranges(&index, pabgb.len());
    let mut out = Vec::with_capacity(index.len());
    for (entry, (start, end)) in index.iter().zip(ranges.iter()) {
        let Some(body) = pabgb.get(*start..*end) else {
            continue;
        };
        let Some(head) = read_row_head(body, 4) else { continue };
        if head.key != entry.key || !(1..=128).contains(&head.name.len()) {
            continue;
        }
        // `[u8; 3]`, then `u16 faction_group_key`, `u16 faction_relation_group_key`.
        let Some(&[_, _, _, g0, g1, r0, r1]) = body.get(head.body_at..head.body_at + 7) else {
            return Vec::new();
        };
        out.push(FactionInfoEntry {
            key: head.key,
            name: head.name.to_owned(),
            group_key: u16::from_le_bytes([g0, g1]),
            relation_group_key: u16::from_le_bytes([r0, r1]),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    //! Live-install integration test against the real `faction` table.
    //! Skips cleanly when the game install isn't present.

    use super::*;
    use crate::binary::gamedata_layout;

    /// (FactionKey, expected internal_name), read from the 2.06 header.
    /// 1000000 is the player's faction (leader CharacterKey 1, Kliff).
    const KNOWN: &[(u32, &str)] = &[
        (1000000, "Faction_Graymane"),
        (1000063, "Faction_Abyss"),
        (1000035, "Faction_Elimoor"),
        (1000058, "Faction_GoblinSandfang"),
        (1000065, "Faction_ThiefDusksong"),
    ];

    fn find_table_bytes(stem: &str) -> Option<(Vec<u8>, Vec<u8>)> {
        // None only when the install (or the table) is absent; an install
        // that is there but unreadable fails the test (audit I-02).
        Some((
            gamedata_layout::extract_bin(&gamedata_layout::body(stem))?,
            gamedata_layout::extract_bin(&gamedata_layout::header(stem))?,
        ))
    }

    #[test]
    fn faction_info_lossy_live() {
        let Some((pabgb, pabgh)) = find_table_bytes("faction") else {
            eprintln!("skipping faction_info_lossy_live: no game install");
            return;
        };
        let entries = parse_faction_info_lossy(&pabgb, &pabgh);
        // Every row the 2.06 header lists parses; a floor would let most of
        // the table go missing unnoticed.
        let listed = crate::binary::static_header::detect_named_pair(&pabgh, &pabgb).expect("2.06 header").2.len();
        assert_eq!(entries.len(), listed, "rows parsed vs rows the header lists");
        let by_key: std::collections::HashMap<u32, &str> =
            entries.iter().map(|e| (e.key, e.name.as_str())).collect();
        for &(key, expected) in KNOWN {
            assert_eq!(
                by_key.get(&key).copied(),
                Some(expected),
                "FactionKey {key} mismatch",
            );
        }
        // (FactionKey, faction_group_key, faction_relation_group_key) on 2.06.
        for &(key, group, relation) in &[
            (1_000_000u32, 16_965u16, 16_963u16), // Faction_Graymane: FactionGroup_Graymane, Graymane
            (1_000_131, 16_960, 16_964),          // Faction_Celeste_Hernand: FactionGroup_Akapen, FriendlyCombat
            (1_000_063, 16_966, 16_966),          // Faction_Abyss: FactionGroup_Abyss, NPC_Common
            (1_000_095, 0, 16_965),               // Faction_BountyHunter: no group
        ] {
            let e = entries.iter().find(|e| e.key == key).unwrap();
            assert_eq!((e.group_key, e.relation_group_key), (group, relation), "FactionKey {key}");
        }
        assert_eq!(entries.iter().filter(|e| e.group_key != 0).count(), 128);
        for e in &entries {
            assert!(
                e.name.starts_with("Faction_"),
                "unexpected faction name: key={}, name={:?}",
                e.key,
                e.name,
            );
        }
    }
}
