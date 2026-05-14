//! `sublevelinfo.pabgb` parser — anchor-scan based.
//!
//! Sibling of [`crate::mission_info`] / [`crate::quest_info`] /
//! [`crate::stage_info`] / [`crate::quest_gauge_info`]. Same row
//! header shape, different semantics:
//!
//! SubLevel rows define **per-faction / per-stat / per-skill progress
//! tracks** that the save's `SubLevelSaveData` block keys into via
//! `(SubLevelKey, _level, _experience, _maxAchievedLevel)`. Examples:
//! `Contribution_Graymane`, `Contribution_Pailunese`,
//! `SkillPoint_Kliff`, `Religion_Hernand`, `LiberationRefugee`,
//! `AchievementHP`. Roughly 40 rows total in 1.06.
//!
//! Like [`crate::quest_gauge_info`], **no PALOC entries** were found
//! at any namespace for these row keys — neither raw (Pattern A) nor
//! via hashlittle2 hash-hop. The handful of raw-key collisions that
//! surface (e.g. hi32=522 lo32=0x402f1 = "Unavailable during combat")
//! are coincidental matches with generic UI tooltip slots, not real
//! sub-level localizations. The localized track name a player sees
//! in the UI is composed at runtime from the prefix (`Contribution_`,
//! `Religion_`, ...) plus the suffix faction/character name resolved
//! through a different table.
//!
//! Practical consequence for the C ABI bridge: only
//! `lookup_string_key` makes sense. There's no `lookup_display_name`
//! analogue — see [`super::sub_level_info`] (the c_abi sibling) for
//! the surface and rationale.

/// One parsed sublevelinfo row, reduced to what the bridge consumes.
#[derive(Debug, Clone)]
pub struct SubLevelInfoEntry {
    /// `SubLevelKey` as stored in save blocks (e.g.
    /// `SubLevelSaveData._list[N]._key`).
    pub key: u32,
    /// ASCII internal name (e.g. `"Contribution_Graymane"`).
    pub name: String,
}

/// Lossy anchor-scan parse of an in-memory `sublevelinfo.pabgb` blob.
///
/// Validation rules:
///
/// - `key != 0 && (key >> 24) == 0` — all real row keys observed in
///   1.06 are in the 101..1003 range, well under 2^16. The hi-byte=0
///   check filters out body-byte garbage.
/// - `name_len ∈ [2, 128]` — actual range 2..28.
/// - The name's first byte must be an ASCII letter. Filters out the
///   all-digit "names" that appear at offsets where the row body has
///   embedded stringinfo hashes serialized as decimal strings
///   (observed for rows 201/202/203's `AbyssHp/AbyssMp/AbyssStamina`,
///   whose bodies reference 12-digit stringinfo IDs).
/// - All other bytes must be identifier-shape (alphanumeric / `_` /
///   space / UTF-8 high byte).
///
/// Returns entries in the order they appear in the file. The bridge's
/// HashMap deduplicates by last-wins, but `or_insert` in the loader
/// preserves first-wins to keep the real row (which appears first at
/// offset 0) over any later body-byte false-positive.
pub fn parse_sub_level_info_lossy(data: &[u8]) -> Vec<SubLevelInfoEntry> {
    let mut entries = Vec::new();
    let mut cursor = 0;
    while let Some(start) = scan_next_anchor(data, cursor) {
        let slen = u32::from_le_bytes([
            data[start + 4],
            data[start + 5],
            data[start + 6],
            data[start + 7],
        ]) as usize;
        let key = u32::from_le_bytes([
            data[start],
            data[start + 1],
            data[start + 2],
            data[start + 3],
        ]);
        let name_bytes = &data[start + 8..start + 8 + slen];
        // Scanner already validated valid UTF-8 — the unwrap is sound.
        let name = std::str::from_utf8(name_bytes).unwrap().to_owned();
        entries.push(SubLevelInfoEntry { key, name });
        cursor = start + 8 + slen;
    }
    entries
}

fn scan_next_anchor(data: &[u8], from: usize) -> Option<usize> {
    let n = data.len();
    let mut o = from;
    while o + 12 < n {
        let key = u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
        if key != 0 && (key >> 24) == 0 {
            let slen = u32::from_le_bytes([
                data[o + 4],
                data[o + 5],
                data[o + 6],
                data[o + 7],
            ]) as usize;
            if (2..=128).contains(&slen) && o + 8 + slen <= n {
                let bytes = &data[o + 8..o + 8 + slen];
                if bytes[0].is_ascii_alphabetic()
                    && bytes.iter().all(|&b| is_ident_byte(b))
                    && std::str::from_utf8(bytes).is_ok()
                {
                    return Some(o);
                }
            }
        }
        o += 1;
    }
    None
}

#[inline]
fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b' ' || b >= 0x80
}

#[cfg(test)]
mod tests {
    //! Live-install integration test against the real
    //! `sublevelinfo.pabgb`. Asserts the 7 mappings drawn from the
    //! Save Editor handoff bundle (one live 1.06 save's
    //! `SubLevelSaveData._list` block) plus a few extras that anchor
    //! the surrounding name clusters.

    use super::*;
    use std::path::PathBuf;

    /// (SubLevelKey, expected internal_name) — verified against
    /// `CrimsonAtomtic/out/analyze/handoff/keycases_unresolved.jsonl`.
    /// Covers Skill / Contribution / Liberation tracks; the
    /// surrounding rows (101–113 stat tracks, 401–403 achievement
    /// tracks, 1000–1002 religion tracks) anchor that the scanner
    /// didn't truncate or skip.
    const KNOWN: &[(u32, &str)] = &[
        // Stat tracks
        (101, "Hp"),
        (102, "Mp"),
        (103, "Stamina"),
        // Sub-skill tracks per character
        (522, "SkillPoint_Oongka"),
        (523, "SkillPoint_Damian"),
        // Faction contribution tracks (every one in the handoff)
        (600, "Contribution_Graymane"),
        (601, "Contribution_Hernandian"),
        (603, "Contribution_Demenissian"),
        (604, "Contribution_Pailunese"),
        (605, "Contribution_Delesyian"),
        (606, "Contribution_Tashkalpan"),
        // Story progress
        (701, "LiberationRefugee"),
        // Religion tracks
        (1000, "Religion_Hernand"),
        (1001, "Religion_Demenissian"),
        (1002, "Religion_Delesyian"),
    ];

    fn find_sublevelinfo_bytes() -> Option<Vec<u8>> {
        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let pamt_path = game_root.join("0008").join("0.pamt");
        if !pamt_path.is_file() {
            return None;
        }
        let pamt_data = std::fs::read(&pamt_path).ok()?;
        let pamt = crate::binary::pamt::PackMeta::parse(&pamt_data, None).ok()?;
        let dir = pamt
            .directories
            .iter()
            .find(|d| d.path == "gamedata/binary__/client/bin")?;
        let file = dir
            .files
            .iter()
            .find(|f| f.name == "sublevelinfo.pabgb")?;
        let group_dir = game_root.join("0008");
        crate::binary::paz::extract_file(
            &group_dir,
            file,
            "gamedata/binary__/client/bin",
            &pamt.header.encrypt_info.encrypt_info,
        )
        .ok()
    }

    #[test]
    fn sub_level_info_lossy_live() {
        let Some(data) = find_sublevelinfo_bytes() else {
            eprintln!("skipping sub_level_info_lossy_live: no game install");
            return;
        };
        let entries = parse_sub_level_info_lossy(&data);
        println!(
            "parsed {} sublevelinfo entries from {} bytes",
            entries.len(),
            data.len()
        );
        // 1.06 shipped 40 real rows after the strict scanner; pin to
        // >30 for plausibility while leaving headroom for future
        // patches adding new sub-level tracks.
        assert!(
            entries.len() > 30,
            "expected >30 sublevel entries, got {}",
            entries.len()
        );

        // First-wins to mirror the bridge's behaviour: the real row at
        // the smallest offset is the canonical entry; body-byte
        // collisions that share a row key get dropped.
        let mut by_key: std::collections::HashMap<u32, &str> = Default::default();
        for e in &entries {
            by_key.entry(e.key).or_insert(e.name.as_str());
        }
        for &(key, expected) in KNOWN {
            assert_eq!(
                by_key.get(&key).copied(),
                Some(expected),
                "SubLevelKey {} mismatch",
                key,
            );
        }

        // Every name must lead with a letter — sanity check that the
        // anchor scanner's first-byte filter is doing its job.
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
