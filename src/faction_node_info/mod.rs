//! `factionnode.pabgb` parser — PABGH-indexed.
//!
//! Resolves `FactionNodeKey (u32)` — the gamedata key referenced by
//! save-side fields that carry a "faction node" reference (a
//! per-location entry in the faction system; e.g. encampments, manors,
//! tribe hubs). Rows are named after the location they tag,
//! `Node_Her_Temporary_Camp`, `Node_Her_HernandCastle`, ...
//!
//! ## The player-facing name: a KnowledgeKey
//!
//! Every row names a `knowledgeinfo` row — the node's map / discovery entry
//! (`Knowledge_Node_Her_HernandCastle`; usually `"Knowledge_" +` the node
//! name, but not always) — and that knowledge's title is the place name
//! the game shows: PALOC `(hashlittle(knowledge name) << 32) | 0x490`, as the
//! `knowledge_info` bridge resolves it. `Node_Her_HernandCastle` → "Hernand
//! Castle", `Node_Her_GreymaneCamp` → "Howling Hill", `Node_Her_GlenbrightFarm`
//! → "Glenbright Farm".
//!
//! The key is a `u32` at a fixed place after the row key: byte 11 in the
//! pre-2.06 layout ([`KNOWLEDGE_AT_PRE_2_06`]) and byte 8 of the row after
//! `[key][u32 row index]` on 2.06 ([`KNOWLEDGE_AT_2_06`]): 2.06 narrowed the
//! leading `u32 1/2/3` (node kind) to a `u16` and dropped a zero byte, so
//! `01 00 00 00 00 78 00` became `01 00 78 00`. The value is always a
//! knowledge key.
//!
//! **No owning faction or region field was found.** Faction keys do turn up in
//! rows, but the `faction` and `factionnode` key ranges overlap (the sibling-node
//! list of `Node_Her_HernandCastle` reads as `Faction_Grace`,
//! `Faction_RegleGoblin`, … when matched against the wrong table), and the rest
//! sit inside inlined conditions (`CheckFactionKey(...)`). The node's knowledge
//! belongs to one `KnowledgeGroup_Topography_<Region>_<Area>` group for about
//! half the nodes and to none for the rest, so that is not a complete region
//! either. The name prefix (`Node_Her_`, `Node_Dem_`, …) is a convention, not
//! a field.
//!
//! Schema (pre-2.06 body layout; the `_probe_faction_pabgh_shapes` ignored
//! probe in [`src/c_abi/character_info.rs`](../c_abi/character_info.rs)
//! dumps it):
//!
//! ```text
//! Row {
//!     u32 name_len,
//!     u8 name[name_len], // ASCII identifier, e.g. "Node_Her_Temporary_Camp"
//!     u32 key,          // matches PABGH key
//!     [...variable body — 200..800 bytes of position / sibling-node
//!      references that this bridge does not consume]
//! }
//! ```
//!
//! PABGH layout matches the standard `u16 count + (u32 key, u32 offset)*`
//! used by `skill.pabgh` (re-used here via [`crate::skill_info::parse_pabgh`]).
//!
//! **PALOC chain**: none keyed by the node itself. Every plausible
//! namespace, at both identity and hashlittle hash-hop, holds only
//! coincidental collisions with the item / character / gimmick display
//! namespaces at `lo32 = 0x70 / 0x30 / 0x200` (the
//! `_probe_faction_paloc_chains` probe scans them). The place name comes
//! through the row's knowledge key instead (above), which
//! `crimson_factionnode_lookup_display_name` resolves.

use crate::binary::row_head::read_row_head;

/// One parsed factionnode row, reduced to what the bridge consumes.
#[derive(Debug, Clone)]
pub struct FactionNodeInfoEntry {
    /// `FactionNodeKey` — matches the PABGH key.
    pub key: u32,
    /// ASCII internal name (e.g. `"Node_Her_Temporary_Camp"`).
    pub name: String,
    /// `KnowledgeKey` whose title is the node's player-facing name; 0 when the
    /// row is too short to hold it.
    pub knowledge_key: u32,
}

/// Where the node's `KnowledgeKey` sits in a pre-2.06 row: bytes after the
/// row key.
pub const KNOWLEDGE_AT_PRE_2_06: usize = 11;
/// Where it sits on 2.06: bytes after the row's `[key][u32 row index]`.
pub const KNOWLEDGE_AT_2_06: usize = 8;

/// Parse `factionnode.pabgb` using its `.pabgh` index.
///
/// Returns entries in PABGH on-disk order. Skips any row whose body is
/// too short or whose body key disagrees with the PABGH key — both
/// treated as anchor noise rather than panics so a future patch with
/// new row shapes doesn't break the build.
pub fn parse_faction_node_info_lossy(
    pabgb: &[u8],
    pabgh: &[u8],
) -> Vec<FactionNodeInfoEntry> {
    // 2.06 moved row names into the header; rebuild the pre-2.06 layout.
    // Which one came in decides where the KnowledgeKey sits.
    let knowledge_at =
        if crate::binary::static_header::detect_named_header(pabgh, pabgb.len()).is_some() {
            KNOWLEDGE_AT_2_06
        } else {
            KNOWLEDGE_AT_PRE_2_06
        };
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
        let at = head.body_at + knowledge_at;
        let knowledge_key = body
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .unwrap_or(0);
        out.push(FactionNodeInfoEntry {
            key: head.key,
            name: head.name.to_owned(),
            knowledge_key,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    //! Live-install integration test against the real
    //! `factionnode.pabgb` + `.pabgh`. Pins a handful of mappings so a
    //! patch changing the row layout or PABGH header shape is caught.
    //! Skips cleanly when the game install isn't present.

    use crate::binary::gamedata_layout;
    use super::*;

    /// (FactionNodeKey, expected internal_name). Taken from the
    /// `_probe_faction_pabgh_shapes` output — these are the first
    /// entries by on-disk PABGH order.
    const KNOWN: &[(u32, &str)] = &[
        (1000401, "Node_Her_Temporary_Camp"),
        (1000044, "Node_Her_HernandCastle"),
    ];

    fn find_table_bytes(stem: &str) -> Option<(Vec<u8>, Vec<u8>)> {
        // None only when the install (or the table) is absent; an install
        // that is there but unreadable fails the test.
        Some((
            gamedata_layout::extract_bin(&gamedata_layout::body(stem))?,
            gamedata_layout::extract_bin(&gamedata_layout::header(stem))?,
        ))
    }

    #[test]
    fn faction_node_info_lossy_live() {
        let Some((pabgb, pabgh)) = find_table_bytes("factionnode") else {
            eprintln!("skipping faction_node_info_lossy_live: no game install");
            return;
        };
        let entries = parse_faction_node_info_lossy(&pabgb, &pabgh);
        println!(
            "parsed {} factionnode entries from {} byte pabgb",
            entries.len(),
            pabgb.len()
        );
        assert!(
            entries.len() > 1000,
            "expected >1000 factionnode entries, got {}",
            entries.len()
        );
        let by_key: std::collections::HashMap<u32, &str> =
            entries.iter().map(|e| (e.key, e.name.as_str())).collect();
        for &(key, expected) in KNOWN {
            assert_eq!(
                by_key.get(&key).copied(),
                Some(expected),
                "FactionNodeKey {key} mismatch",
            );
        }
        // Sanity: every name leads with an ASCII letter.
        for e in &entries {
            assert!(
                e.name.bytes().next().is_some_and(|b| b.is_ascii_alphabetic()),
                "non-letter-leading name: key={}, name={:?}",
                e.key,
                e.name,
            );
        }
    }
}
