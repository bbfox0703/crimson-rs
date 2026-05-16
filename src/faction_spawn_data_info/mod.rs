//! `factionspawndatainfo.pabgb` parser — PABGH-indexed.
//!
//! Resolves `FactionSpawnDataKey (u32)` — the gamedata key referenced
//! by save-side fields that carry a "faction spawn" reference (a
//! per-location spawn manifest naming an NPC, a guard kit, or a
//! mercenary mix). Rows are named after the spawn theme + location,
//! e.g. `FactionSpawn_GlenbrightManor_Grace_ReedDevil`. Roughly 117
//! rows in 1.07.
//!
//! Schema (verified against the live 1.07 install via the
//! `_probe_faction_pabgh_shapes` ignored probe):
//!
//! ```text
//! Row {
//!     u32 key,           // matches PABGH key
//!     u32 name_len,
//!     u8 name[name_len], // ASCII identifier
//!     [...variable body — spawn manifest the bridge does not consume]
//! }
//! ```
//!
//! PABGH layout matches the standard `u16 count + (u32 key, u32 offset)*`.
//!
//! **PALOC chain**: none — same situation as [`crate::faction_node_info`].
//! The exhaustive probe at `_probe_faction_paloc_chains` surfaced only
//! coincidental collisions with iteminfo / character / skill keys
//! sharing the small u32 hi32 space. The bridge mirrors
//! [`crate::sub_level_info`] / [`crate::quest_gauge_info`] in shape:
//! **no `lookup_display_name` surface**.

/// One parsed factionspawndatainfo row.
#[derive(Debug, Clone)]
pub struct FactionSpawnDataInfoEntry {
    /// `FactionSpawnDataKey` — matches the PABGH key.
    pub key: u32,
    /// ASCII internal name (e.g.
    /// `"FactionSpawn_GlenbrightManor_Grace_ReedDevil"`).
    pub name: String,
}

/// Parse `factionspawndatainfo.pabgb` using its `.pabgh` index.
pub fn parse_faction_spawn_data_info_lossy(
    pabgb: &[u8],
    pabgh: &[u8],
) -> Vec<FactionSpawnDataInfoEntry> {
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
        out.push(FactionSpawnDataInfoEntry {
            key,
            name: name.to_owned(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    //! Live-install integration test against the real
    //! `factionspawndatainfo.pabgb` + `.pabgh`.

    use super::*;
    use std::path::PathBuf;

    /// (FactionSpawnDataKey, expected internal_name).
    const KNOWN: &[(u32, &str)] = &[
        (1000000, "FactionSpawn_GlenbrightManor_Grace_ReedDevil"),
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
            .find(|d| d.path == "gamedata/binary__/client/bin")?;
        let pabgb_file = dir
            .files
            .iter()
            .find(|f| f.name == "factionspawndatainfo.pabgb")?;
        let pabgh_file = dir
            .files
            .iter()
            .find(|f| f.name == "factionspawndatainfo.pabgh")?;
        let group_dir = game_root.join("0008");
        let pabgb = crate::binary::paz::extract_file(
            &group_dir,
            pabgb_file,
            "gamedata/binary__/client/bin",
            &pamt.header.encrypt_info.encrypt_info,
        )
        .ok()?;
        let pabgh = crate::binary::paz::extract_file(
            &group_dir,
            pabgh_file,
            "gamedata/binary__/client/bin",
            &pamt.header.encrypt_info.encrypt_info,
        )
        .ok()?;
        Some((pabgb, pabgh))
    }

    #[test]
    fn faction_spawn_data_info_lossy_live() {
        let Some((pabgb, pabgh)) = find_table_bytes() else {
            eprintln!("skipping faction_spawn_data_info_lossy_live: no game install");
            return;
        };
        let entries = parse_faction_spawn_data_info_lossy(&pabgb, &pabgh);
        println!(
            "parsed {} factionspawndatainfo entries from {} byte pabgb",
            entries.len(),
            pabgb.len()
        );
        assert!(
            entries.len() > 100,
            "expected >100 factionspawndatainfo entries, got {}",
            entries.len()
        );
        let by_key: std::collections::HashMap<u32, &str> =
            entries.iter().map(|e| (e.key, e.name.as_str())).collect();
        for &(key, expected) in KNOWN {
            assert_eq!(
                by_key.get(&key).copied(),
                Some(expected),
                "FactionSpawnDataKey {key} mismatch",
            );
        }
    }
}
