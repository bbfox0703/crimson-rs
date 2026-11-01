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
//! is the `[u32 name_len][identifier-byte name][u32 key][u8 0x00]` head
//! shape (2.04 moved the key after the name), which holds across the
//! whole family.
//!
//! Stage rows resolve to PALOC at two common namespaces:
//! - `lo32 = 0x101` (257) — the stage title (6,492 matches observed)
//! - `lo32 = 0x102` (258) — the stage / shop description; longer text
//!   describing the row (404 matches observed)

use crate::binary::row_head::{is_row_name_byte, read_row_head};

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

/// PABGH-driven parse of `stageinfo` — the authoritative path.
///
/// `pabgh` carries the in-game iteration order as a 4-byte count header
/// followed by `(u32 key, u32 offset)` rows (the u32-count variant —
/// stageinfo and questinfo are the two tables on this shape). For each
/// pair we jump into `pabgb` at the offset and read the standard
/// `[u32 slen][slen bytes][u32 key_repeat]` row head. This bypasses
/// the anchor-scan's body-byte validation, so legitimate rows with
/// punctuation in their names (e.g. `LevelSequencerSpawn_UUID_2910822923,…`,
/// 1,403 of which are present in 1.08's stageinfo) are no longer
/// silently dropped — the bridge returns the same key set the in-game
/// `[u32 count][...]` index does.
///
/// Returns an empty vector if `pabgh` doesn't match the expected shape
/// (`4 + 8·count == pabgh.len()`); the row loop tolerates individual
/// malformed offsets by skipping them.
pub fn parse_stage_info_via_pabgh(pabgh: &[u8], pabgb: &[u8]) -> Vec<StageInfoEntry> {
    if pabgh.len() < 4 {
        return Vec::new();
    }
    let count = u32::from_le_bytes([pabgh[0], pabgh[1], pabgh[2], pabgh[3]]) as usize;
    if pabgh.len() != 4 + 8 * count {
        return Vec::new();
    }
    let mut entries = Vec::with_capacity(count);
    for i in 0..count {
        let pos = 4 + i * 8;
        let key = u32::from_le_bytes([
            pabgh[pos],
            pabgh[pos + 1],
            pabgh[pos + 2],
            pabgh[pos + 3],
        ]);
        let off = u32::from_le_bytes([
            pabgh[pos + 4],
            pabgh[pos + 5],
            pabgh[pos + 6],
            pabgh[pos + 7],
        ]) as usize;
        let Some(head) = pabgb.get(off..).and_then(|row| read_row_head(row, 4)) else {
            continue;
        };
        if head.key != key || !(1..=256).contains(&head.name.len()) {
            continue;
        }
        entries.push(StageInfoEntry {
            key,
            name: head.name.to_owned(),
        });
    }
    entries
}

/// Lossy anchor-scan parse of an in-memory `stageinfo.pabgb` blob.
///
/// Kept for callers that can't supply the `.pabgh` index (e.g. a saved
/// extract of just the `.pabgb`). The anchor-scan's body-byte
/// validation rejects rows whose names contain punctuation outside the
/// `[A-Za-z0-9_ ]+ U+0080..` set, so for the 1.08 install this
/// path returns **49,634** entries instead of the in-game-authoritative
/// **51,037** the `.pabgh` lists — under-counts by 1,403 due to
/// `LevelSequencerSpawn_UUID_<uid>,<uid>,…`-style names. Prefer
/// [`parse_stage_info_via_pabgh`] when both files are available.
///
/// See [`crate::mission_info::parse_mission_info_lossy`] for the
/// validation rules — identical here. The scanner runs over the full
/// 26 MB file in milliseconds; the bottleneck on load is the
/// HashMap build, not the scan.
pub fn parse_stage_info_lossy(data: &[u8]) -> Vec<StageInfoEntry> {
    let mut entries = Vec::new();
    let mut cursor = 0;
    while let Some(start) = scan_next_anchor(data, cursor) {
        // Scanner already validated the head — the unwrap is sound.
        let head = read_row_head(&data[start..], 4).unwrap();
        entries.push(StageInfoEntry {
            key: head.key,
            name: head.name.to_owned(),
        });
        cursor = start + head.body_at;
    }
    entries
}

fn scan_next_anchor(data: &[u8], from: usize) -> Option<usize> {
    let n = data.len();
    let mut o = from;
    while o + 12 < n {
        let slen = u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]) as usize;
        // Length, name, key, then the 0x00 that opens every 2.04 row body —
        // the byte that rejects most body strings followed by a small number.
        if (2..=128).contains(&slen) && o + 9 + slen <= n && data[o + 8 + slen] == 0 {
            let bytes = &data[o + 4..o + 4 + slen];
            let key = u32::from_le_bytes([
                data[o + 4 + slen],
                data[o + 5 + slen],
                data[o + 6 + slen],
                data[o + 7 + slen],
            ]);
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
            // Same `_` requirement as mission_info — every real
            // Stage_*/Intro_*/etc. row has it.
            if key != 0
                && (key >> 24) < 0x80
                && bytes.contains(&b'_')
                && bytes.iter().all(|&b| is_row_name_byte(b))
                && std::str::from_utf8(bytes).is_ok()
            {
                return Some(o);
            }
        }
        o += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    //! Live-install integration test against the real `stageinfo.pabgb`.

    use crate::binary::gamedata_layout;
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
            .find(|d| d.path == gamedata_layout::bin_dir())?;
        let file = dir.files.iter().find(|f| f.name == gamedata_layout::body("stageinfo"))?;
        let group_dir = game_root.join("0008");
        crate::binary::paz::extract_file(
            &group_dir,
            file,
            gamedata_layout::bin_dir(),
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
