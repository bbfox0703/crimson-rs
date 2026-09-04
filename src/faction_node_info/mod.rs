//! `factionnode.pabgb` parser — PABGH-indexed.
//!
//! Resolves `FactionNodeKey (u32)` — the gamedata key referenced by
//! save-side fields that carry a "faction node" reference (a
//! per-location entry in the faction system; e.g. encampments, manors,
//! tribe hubs). Rows are named after the location they tag,
//! `Node_Her_Temporary_Camp`, `Node_Her_HernandCastle`, ... Roughly
//! 1,158 rows in 1.07.
//!
//! Schema (verified against the live 1.07 install via the
//! `_probe_faction_pabgh_shapes` ignored probe in
//! [`src/c_abi/character_info.rs`](../c_abi/character_info.rs)):
//!
//! ```text
//! Row {
//!     u32 key,          // matches PABGH key
//!     u32 name_len,
//!     u8 name[name_len], // ASCII identifier, e.g. "Node_Her_Temporary_Camp"
//!     [...variable body — 200..800 bytes of position / sibling-node
//!      references that this bridge does not consume]
//! }
//! ```
//!
//! PABGH layout matches the standard `u16 count + (u32 key, u32 offset)*`
//! used by `skill.pabgh` (re-used here via [`crate::skill_info::parse_pabgh`]).
//!
//! **PALOC chain**: none. The exhaustive probe in
//! `_probe_faction_paloc_chains` scanned every plausible namespace at
//! both identity and hashlittle2 hash-hop and returned only
//! coincidental collisions with iteminfo / character / skill keys at
//! `lo32 = 0x70 / 0x30 / 0x200` — no real localized faction-node names
//! exist. The bridge mirrors [`crate::sub_level_info`] / [`crate::quest_gauge_info`]
//! in shape: **no `lookup_display_name` surface**, the internal name
//! is the only resolution.

/// One parsed factionnode row, reduced to what the bridge consumes.
#[derive(Debug, Clone)]
pub struct FactionNodeInfoEntry {
    /// `FactionNodeKey` — matches the PABGH key.
    pub key: u32,
    /// ASCII internal name (e.g. `"Node_Her_Temporary_Camp"`).
    pub name: String,
}

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
    let Ok(index) = crate::skill_info::parse_pabgh(pabgh) else {
        return Vec::new();
    };
    let ranges = crate::skill_info::entry_ranges(&index, pabgb.len());
    let mut out = Vec::with_capacity(index.len());
    for (entry, (start, end)) in index.iter().zip(ranges.iter()) {
        let Some(body) = pabgb.get(*start..*end) else {
            continue;
        };
        if body.len() < 8 {
            continue;
        }
        let key = u32::from_le_bytes([body[0], body[1], body[2], body[3]]);
        if key != entry.key {
            continue;
        }
        let name_len =
            u32::from_le_bytes([body[4], body[5], body[6], body[7]]) as usize;
        if !(1..=128).contains(&name_len) || 8 + name_len > body.len() {
            continue;
        }
        let Ok(name) = std::str::from_utf8(&body[8..8 + name_len]) else {
            continue;
        };
        out.push(FactionNodeInfoEntry {
            key,
            name: name.to_owned(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    //! Live-install integration test against the real
    //! `factionnode.pabgb` + `.pabgh`. Pins a handful of mappings from
    //! the 2026-05-16 RE pass so a future patch changing the row layout
    //! or PABGH header shape is caught immediately. Skips cleanly when
    //! the game install isn't present.

    use crate::binary::gamedata_layout;
    use super::*;
    use std::path::PathBuf;

    /// (FactionNodeKey, expected internal_name). Taken from the
    /// `_probe_faction_pabgh_shapes` output — these are the first
    /// entries by on-disk PABGH order. The 1.07 file has 1,158 rows.
    const KNOWN: &[(u32, &str)] = &[
        (1000401, "Node_Her_Temporary_Camp"),
        (1000044, "Node_Her_HernandCastle"),
    ];

    fn find_table_bytes(stem: &str) -> Option<(Vec<u8>, Vec<u8>)> {
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
        let pabgb_file = dir.files.iter().find(|f| f.name == format!("{stem}.pabgb"))?;
        let pabgh_file = dir.files.iter().find(|f| f.name == format!("{stem}.pabgh"))?;
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
