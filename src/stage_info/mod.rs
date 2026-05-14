//! `stageinfo.pabgb` parser — anchor-scan based.
//!
//! Sibling of [`crate::mission_info`] and [`crate::quest_info`]. Same
//! architecture and same row header shape.
//!
//! This is by far the largest gamedata table — **26 MB**, ~57k row
//! anchors on 1.06, and the screenshot save's `_stageStateData` had
//! 46,541 instances. The Save Editor's resolved-name column lights up
//! for tens of thousands of rows when this bridge is loaded.
//!
//! Stage internal names are **region-themed**, not `Stage_*`-prefixed
//! (e.g. `Intro_Tutorial_Miseenscene_00`, `DelesyiaCastle_Herbert_BlueStone`,
//! `Hernand_Normal_Start_Child6`). The anchor scanner's only assumption
//! is the `[u32 key][u32 name_len][identifier-byte name]` header shape,
//! which holds across the whole family.
//!
//! Stage rows resolve to PALOC at two common namespaces:
//! - `lo32 = 0x101` (257) — the stage title (6,492 matches observed)
//! - `lo32 = 0x102` (258) — the stage / shop description; longer text
//!   describing the row (404 matches observed)

/// One parsed stageinfo row, reduced to what the bridge consumes.
#[derive(Debug, Clone)]
pub struct StageInfoEntry {
    /// `StageKey` as stored in save blocks (e.g.
    /// `_stageStateData[N]._key`).
    pub key: u32,
    /// ASCII internal name. Feeds the PALOC u64 lookup chain via
    /// `hashlittle2(name) << 32 | lo32`.
    pub name: String,
}

/// Lossy anchor-scan parse of an in-memory `stageinfo.pabgb` blob.
///
/// See [`crate::mission_info::parse_mission_info_lossy`] for the
/// validation rules — identical here. The scanner runs over the full
/// 26 MB file in milliseconds; the bottleneck on load is the
/// HashMap build, not the scan.
pub fn parse_stage_info_lossy(data: &[u8]) -> Vec<StageInfoEntry> {
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
        entries.push(StageInfoEntry { key, name });
        cursor = start + 8 + slen;
    }
    entries
}

fn scan_next_anchor(data: &[u8], from: usize) -> Option<usize> {
    let n = data.len();
    let mut o = from;
    while o + 12 < n {
        let key = u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
        // Stage rows use category-prefixed keys: hi-byte 0x00 for
        // mainline stages, 0x05 for `HerStore_*` (Hernandian stores),
        // 0x41 for `Shop_*` (city shops), and a handful of others
        // observed in the 1.06 install. Cap at < 0x80 — covers every
        // real category seen, excludes the body-byte noise that
        // tends to start with high-bit bytes (e.g. checksum tails),
        // and naturally excludes the i32-negative-encoded save
        // sentinels (`0xFFFF*`) the editor's issue #2 pattern 2
        // flagged as save-internal. Verified against the editor's
        // CrimsonAtomtic issue #2 dump for `StageKey 1100170020`
        // (0x41934324 → "Shop_Hernand_General"), `1100170040`
        // (0x41934338 → "Shop_Hernand_Pub"), and `98200060`
        // (0x05DA69FC → "HerStore_Grocery").
        if key != 0 && (key >> 24) < 0x80 {
            let slen = u32::from_le_bytes([
                data[o + 4],
                data[o + 5],
                data[o + 6],
                data[o + 7],
            ]) as usize;
            if (2..=128).contains(&slen) && o + 8 + slen <= n {
                let bytes = &data[o + 8..o + 8 + slen];
                // Same `_` requirement as mission_info — every real
                // Stage_*/Intro_*/etc. row has it.
                if bytes.contains(&b'_')
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
    //! Live-install integration test against the real `stageinfo.pabgb`.

    use super::*;
    use std::path::PathBuf;

    /// (StageKey, expected internal_name) — five rows verified against
    /// the editor's keycases handoff and `docs/save-editor-keys-plan.md`.
    /// Includes `1004305` from the editor's UI screenshot.
    const KNOWN: &[(u32, &str)] = &[
        // hi-byte = 0x00 — mainline stages (well-covered by the
        // original scanner constraint).
        (1_004_305, "Intro_Tutorial_Miseenscene_00"),
        (1_000_001, "DelesyiaCastle_Herbert_BlueStone"),
        (1_000_002, "Varnia_UrdavahResearch_RedStone"),
        (1_001_566, "AnvilHill_Block_Patrol_I"),
        (1_012_833, "Shop_Demeniss_Faction_Elemore_Grocery"),
        // Regression for CrimsonAtomtic editor issue #2 pattern 1.
        // hi-byte = 0x05 — `HerStore_*` (Hernandian stores).
        (98_200_060, "HerStore_Grocery"),
        // hi-byte = 0x41 — `Shop_*` (city shops).
        (1_100_170_020, "Shop_Hernand_General"),
        (1_100_170_040, "Shop_Hernand_Pub"),
    ];

    fn find_stageinfo_bytes() -> Option<Vec<u8>> {
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
        let file = dir.files.iter().find(|f| f.name == "stageinfo.pabgb")?;
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
    fn stage_info_lossy_live() {
        let Some(data) = find_stageinfo_bytes() else {
            eprintln!("skipping stage_info_lossy_live: no game install");
            return;
        };
        let entries = parse_stage_info_lossy(&data);
        println!("parsed {} stageinfo entries from {} bytes", entries.len(), data.len());
        // 1.06 has ~57k stage rows. Pin to >10k for plausibility; the
        // exact count drifts patch-to-patch.
        assert!(
            entries.len() > 10_000,
            "expected >10k stage entries, got {}",
            entries.len()
        );
        let by_key: std::collections::HashMap<u32, &str> =
            entries.iter().map(|e| (e.key, e.name.as_str())).collect();
        for &(key, expected) in KNOWN {
            assert_eq!(
                by_key.get(&key).copied(),
                Some(expected),
                "StageKey {} mismatch",
                key,
            );
        }
    }
}
