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
//! **PABGB row layout** (verified byte-exact across all 5 rows in 1.07):
//!
//! ```text
//! Row {
//!     u16 key,              // matches PABGH key
//!     u32 name_len,
//!     u8  name[name_len],   // ASCII identifier
//!     u8  pad1[5],          // = 00 00 00 00 00
//!     u32 list1_count,
//!     u16 list1[list1_count],   // u16 references back into this same table
//!     u8  pad2[4],          // = 00 00 00 00 — separator between the two lists
//!     u32 list2_count,
//!     u16 list2[list2_count],   // second u16-key list
//! }
//! ```
//!
//! `list1` and `list2` are sibling-group references — the relation
//! semantics for each row are stored as lists of *other* row keys.
//! E.g. `Graymane`'s lists point at `HostileCombat` (0x4245),
//! `Monster_Common` (0x4247) and `FriendlyCombat` (0x4244),
//! `NPC_Common` (0x4246).
//!
//! The bridge only consumes `(key, name)` for the standard
//! `lookup_string_key` surface; the embedded reference lists are
//! exposed via [`FactionRelationGroupEntry::related`] for callers that
//! want to render the relation matrix in a UI.
//!
//! **PALOC chain**: none — the `_probe_faction_paloc_chains` pass
//! returned only coincidental collisions at `lo32 = 0x80` (UI tooltip
//! sentinels). The bridge follows the
//! [`crate::sub_level_info`] pattern: no `lookup_display_name`.

/// One parsed factionrelationgroup row.
#[derive(Debug, Clone)]
pub struct FactionRelationGroupEntry {
    /// `FactionRelationGroupKey` — u16 in the on-disk table; widened
    /// to u32 here so the bridge API stays uniform with the other
    /// faction tables. The high 16 bits are always zero.
    pub key: u32,
    /// ASCII internal name (e.g. `"Graymane"`, `"HostileCombat"`).
    pub name: String,
    /// Sibling-row references this group relates to (union of the two
    /// embedded u16 lists in the on-disk body — surface them as a
    /// single flat list since the partition between "list1" and
    /// "list2" is not yet RE'd to a known semantic split).
    pub related: Vec<u32>,
}

/// Parse `factionrelationgroup.pabgb` using its custom-shape `.pabgh`
/// index (`u16 count + (u16 key, u32 offset)*`).
pub fn parse_faction_relation_group_info_lossy(
    pabgb: &[u8],
    pabgh: &[u8],
) -> Vec<FactionRelationGroupEntry> {
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
        if body.len() < 6 {
            continue;
        }
        let body_key = u16::from_le_bytes([body[0], body[1]]);
        if body_key != *key16 {
            continue;
        }
        let name_len =
            u32::from_le_bytes([body[2], body[3], body[4], body[5]]) as usize;
        if !(1..=128).contains(&name_len) || 6 + name_len > body.len() {
            continue;
        }
        let Ok(name) = std::str::from_utf8(&body[6..6 + name_len]) else {
            continue;
        };
        // Parse the two embedded reference lists. Layout per row:
        //   [u32 list1_count][list1: list1_count × u16]
        //   [u8 pad2[4]]
        //   [u32 list2_count][list2: list2_count × u16]
        // Tolerate truncation — a row with no lists, or a body byte
        // that fails the layout check, still surfaces the (key, name)
        // pair with whatever `related` was parsed so far.
        let mut related: Vec<u32> = Vec::new();
        let mut cursor = 6 + name_len + 5; // skip the 5 pad bytes after `name`
        for list_idx in 0..2 {
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
                let r = u16::from_le_bytes([body[off], body[off + 1]]);
                related.push(u32::from(r));
            }
            cursor += n * 2;
            // 4-byte separator between list1 and list2; nothing after list2.
            if list_idx == 0 {
                cursor += 4;
            }
        }
        out.push(FactionRelationGroupEntry {
            key: u32::from(*key16),
            name: name.to_owned(),
            related,
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
    use std::path::PathBuf;

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
        let pabgb_file = dir
            .files
            .iter()
            .find(|f| f.name == gamedata_layout::body("factionrelationgroup"))?;
        let pabgh_file = dir
            .files
            .iter()
            .find(|f| f.name == gamedata_layout::header("factionrelationgroup"))?;
        let group_dir = game_root.join("0008");
        let pabgb = crate::binary::paz::extract_file(
            &group_dir,
            pabgb_file,
            gamedata_layout::bin_dir(),
            &pamt.header.encrypt_info.encrypt_info,
        )
        .ok()?;
        let pabgh = crate::binary::paz::extract_file(
            &group_dir,
            pabgh_file,
            gamedata_layout::bin_dir(),
            &pamt.header.encrypt_info.encrypt_info,
        )
        .ok()?;
        Some((pabgb, pabgh))
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
    }
}
