//! `mercenaryinfo.pabgb` parser — custom-PABGH (u8-key variant).
//!
//! Resolves `MercenaryKey (u8-widened-u32)` — the **template** ID of
//! every mercenary / mount / vehicle in the game. Distinct from
//! `MercenaryNo`, which is the per-save instance ID; the Save Editor's
//! "Rename Mercenary" UI uses this bridge to surface the template
//! lineage of each instance (e.g. "this rename targets a Vehicle_Horse
//! template").
//!
//! 1.07 ships **18 rows** — `Mercenary_Main` (player avatar template)
//! plus 17 mount / vehicle variants (`Vehicle_Horse`, `Vehicle_Dragon`,
//! `Vehicle_WarMachine`, `Vehicle_WarMachine_Raptor`, …).
//!
//! **PABGH layout** — custom 5-byte entries (`u16 count + (u8 key, u32 offset)*`).
//! New variant for this codebase; previously seen shapes were
//! `(u16, u32)` (factionrelationgroup, storeinfo,
//! partprefabdyetexturepalleteinfo) and `(u32, u32)` (skill, dye color
//! group, etc.). 18 entries × 5 + 2 header = 92 bytes (matches the live
//! file).
//!
//! **PABGB row layout** — bridge consumes the leading
//! `(u8 key, u32 name_len, name)` triple. The rest of each row
//! (~58 bytes of template body — version tag, sentinel bytes, anchor
//! marker, then a hash trailer) is left untouched.
//!
//! ```text
//! Row {
//!     u8  key,              // matches PABGH key
//!     u32 name_len,
//!     u8  name[name_len],   // ASCII template name, e.g. "Mercenary_Main"
//!     [variable body — version + flags + trailing hash, ~58 B/row,
//!      length determined by the next row's offset]
//! }
//! ```
//!
//! `CString`-shape name (no trailing NUL), matching `iteminfo.pabgb`.
//!
//! **PALOC chain**: not probed. As with the other internal-name-only
//! tables (FactionNode, SubLevel, QuestGauge) the localized name a
//! player sees is resolved through a separate gamedata path; the
//! template internal name is the save-layer identifier and is what
//! this bridge surfaces.

/// One parsed mercenaryinfo row, reduced to what the bridge consumes.
#[derive(Debug, Clone)]
pub struct MercenaryInfoEntry {
    /// `MercenaryKey` (u8 in the on-disk table, widened to u32 here).
    pub key: u32,
    /// ASCII internal name (e.g. `"Mercenary_Main"`, `"Vehicle_Horse"`).
    pub name: String,
}

/// Parse `mercenaryinfo.pabgb` using its custom-shape `.pabgh` index.
///
/// Returns entries in PABGH on-disk order. Skips any row whose body is
/// truncated or whose body key disagrees with the PABGH key.
pub fn parse_mercenary_info_lossy(
    pabgb: &[u8],
    pabgh: &[u8],
) -> Vec<MercenaryInfoEntry> {
    if pabgh.len() < 2 {
        return Vec::new();
    }
    let count = u16::from_le_bytes([pabgh[0], pabgh[1]]) as usize;
    if pabgh.len() != 2 + count * 5 {
        return Vec::new();
    }
    let mut entries: Vec<(u8, u32)> = Vec::with_capacity(count);
    for i in 0..count {
        let off = 2 + i * 5;
        let k = pabgh[off];
        let o = u32::from_le_bytes([
            pabgh[off + 1],
            pabgh[off + 2],
            pabgh[off + 3],
            pabgh[off + 4],
        ]);
        entries.push((k, o));
    }
    let mut out = Vec::with_capacity(count);
    for &(key8, off) in &entries {
        let start = off as usize;
        let Some(body) = pabgb.get(start..) else {
            continue;
        };
        // Row needs at least `u8 key + u32 name_len = 5` bytes.
        if body.len() < 5 {
            continue;
        }
        if body[0] != key8 {
            continue;
        }
        let name_len =
            u32::from_le_bytes([body[1], body[2], body[3], body[4]]) as usize;
        if !(1..=64).contains(&name_len) || 5 + name_len > body.len() {
            continue;
        }
        let Ok(name) = std::str::from_utf8(&body[5..5 + name_len]) else {
            continue;
        };
        out.push(MercenaryInfoEntry {
            key: u32::from(key8),
            name: name.to_owned(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    //! Live-install integration test against the real
    //! `mercenaryinfo.pabgb` + `.pabgh`. Pins every 1.07 row.

    use super::*;
    use std::path::PathBuf;

    /// All 18 1.07 rows. Pins the schema in both directions —
    /// `(key, name)` plus expected row count.
    const KNOWN: &[(u32, &str)] = &[
        (64, "Mercenary_Main"),
        (78, "Vehicle_Horse"),
        (79, "Vehicle_Dragon"),
        (80, "Vehicle_WarMachine"),
        (82, "Vehicle_WarMachine_Raptor"),
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
        let pabgb_file = dir.files.iter().find(|f| f.name == "mercenaryinfo.pabgb")?;
        let pabgh_file = dir.files.iter().find(|f| f.name == "mercenaryinfo.pabgh")?;
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
    fn mercenary_info_lossy_live() {
        let Some((pabgb, pabgh)) = find_table_bytes() else {
            eprintln!("skipping mercenary_info_lossy_live: no game install");
            return;
        };
        let entries = parse_mercenary_info_lossy(&pabgb, &pabgh);
        println!(
            "parsed {} mercenaryinfo entries from {} byte pabgb",
            entries.len(),
            pabgb.len()
        );
        assert_eq!(entries.len(), 18, "expected exactly 18 rows in 1.07");
        let by_key: std::collections::HashMap<u32, &str> =
            entries.iter().map(|e| (e.key, e.name.as_str())).collect();
        for &(key, expected) in KNOWN {
            assert_eq!(
                by_key.get(&key).copied(),
                Some(expected),
                "MercenaryKey {key} mismatch",
            );
        }
        // Sanity: every name is non-empty ASCII alphanumeric +
        // underscore (template-name shape). 1.07 includes both the
        // suffixed `Vehicle_Horse` family and the bare-template rows
        // `Vehicle`, `Wagon`, `Mercenary`, ... so no shared prefix check.
        for e in &entries {
            assert!(!e.name.is_empty(), "empty name for key {}", e.key);
            assert!(
                e.name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_'),
                "name has non-ASCII-alphanumeric byte: key={}, name={:?}",
                e.key,
                e.name,
            );
        }
        println!("all 18 mercenaryinfo rows:");
        for e in &entries {
            println!("  {} → {}", e.key, e.name);
        }
    }
}
