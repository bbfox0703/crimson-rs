//! `stageinfo.pabgb` parser — anchor-scan based.
//!
//! Sibling of [`crate::mission_info`] and [`crate::quest_info`]. Same
//! architecture and same row header shape.
//!
//! This is by far the largest gamedata table (tens of MB, tens of
//! thousands of rows), and a save's `_stageStateData` holds tens of
//! thousands of instances.
//!
//! Stage internal names are **region-themed**, not `Stage_*`-prefixed
//! (e.g. `Intro_Tutorial_Miseenscene_00`, `DelesyiaCastle_Herbert_BlueStone`,
//! `Hernand_Normal_Start_Child6`). The anchor scanner assumes the legacy
//! `[u32 name_len][identifier-byte name][u32 key][u8 0x00]` head shape,
//! which holds across the whole family.
//!
//! Stage rows resolve to PALOC at two common namespaces:
//! - `lo32 = 0x101` (257) — the stage title
//! - `lo32 = 0x102` (258) — the stage / shop description; longer text
//!   describing the row

use crate::binary::row_head::{is_row_name_byte, read_row_head};

/// One parsed stageinfo row, reduced to what the bridge consumes.
#[derive(Debug, Clone)]
pub struct StageInfoEntry {
    /// `StageKey` as stored in save blocks (e.g.
    /// `_stageStateData[N]._key`).
    pub key: u32,
    /// ASCII internal name. Feeds the PALOC u64 lookup chain via
    /// `hashlittle(name) << 32 | lo32`.
    pub name: String,
}

/// PABGH-driven parse of `stageinfo` — the authoritative path.
///
/// `pabgh` carries the in-game iteration order as a 4-byte count header
/// followed by `(u32 key, u32 offset)` rows (the u32-count variant,
/// shared with questinfo). For each
/// pair we jump into `pabgb` at the offset and read the standard
/// `[u32 slen][slen bytes][u32 key_repeat]` row head. This bypasses
/// the anchor-scan's body-byte validation, so legitimate rows with
/// punctuation in their names (e.g. `LevelSequencerSpawn_UUID_2910822923,…`)
/// are kept — the bridge returns the same key set the in-game
/// `[u32 count][...]` index does.
///
/// Returns an empty vector if `pabgh` doesn't match the expected shape
/// (`4 + 8·count == pabgh.len()`); the row loop tolerates individual
/// malformed offsets by skipping them.
pub fn parse_stage_info_via_pabgh(pabgh: &[u8], pabgb: &[u8]) -> Vec<StageInfoEntry> {
    // 2.06 moved row names into the header; rebuild the pre-2.06 layout.
    let (pabgb, pabgh) = crate::binary::static_header::legacy_pair(pabgb, pabgh);
    let (pabgb, pabgh): (&[u8], &[u8]) = (&pabgb, &pabgh);
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

/// 2.06+: row names live in the `.staticinfoheader`, which lists every row,
/// so with a 2.06 header the rows come from it, in header order (the anchor
/// scan drops rows its key and name rules do not expect). A pre-2.06
/// pair, or a header that is not the 2.06 shape, falls back to scanning `data`.
pub fn parse_stage_info_lossy_with_header(data: &[u8], header: &[u8]) -> Vec<StageInfoEntry> {
    match crate::binary::static_header::header_row_names(data, header) {
        Some(rows) => rows.into_iter().map(|(key, name)| StageInfoEntry { key, name }).collect(),
        None => parse_stage_info_lossy(data),
    }
}

/// Lossy anchor-scan parse of an in-memory `stageinfo.pabgb` blob.
///
/// For callers that can't supply the `.pabgh` index (e.g. a saved
/// extract of just the `.pabgb`). The anchor-scan's body-byte
/// validation rejects rows whose names contain punctuation outside the
/// `[A-Za-z0-9_ ]+ U+0080..` set, so this path under-counts against the
/// `.pabgh`: it drops `LevelSequencerSpawn_UUID_<uid>,<uid>,…`-style
/// names, and its key rule drops keys of 2^31 and up. Prefer
/// [`parse_stage_info_via_pabgh`] when both files are available.
///
/// See [`crate::mission_info::parse_mission_info_lossy`] for the name
/// rules; the key rule here is `(key >> 24) < 0x80`. The scanner runs
/// over the full file in milliseconds; the bottleneck on load is the
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
        // Length, name, key, then the 0x00 that opens every legacy-layout row body —
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
            // 0x41 for `Shop_*` (city shops), and a handful of others.
            // Cap at < 0x80 — covers those categories, excludes the
            // body-byte noise that tends to start with high-bit bytes
            // (e.g. checksum tails), and excludes the i32-negative-encoded
            // save-internal sentinels (`0xFFFF*`). Examples:
            // `StageKey 1100170020`
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

    /// (StageKey, expected internal_name), from
    /// `docs/save-editor-keys-plan.md` and the editor's key cases, one
    /// group per key hi-byte category the scanner accepts.
    const KNOWN: &[(u32, &str)] = &[
        // hi-byte = 0x00 — mainline stages.
        (1_004_305, "Intro_Tutorial_Miseenscene_00"),
        (1_000_001, "DelesyiaCastle_Herbert_BlueStone"),
        (1_000_002, "Varnia_UrdavahResearch_RedStone"),
        (1_001_566, "AnvilHill_Block_Patrol_I"),
        (1_012_833, "Shop_Demeniss_Faction_Elemore_Grocery"),
        // hi-byte = 0x05 — `HerStore_*` (Hernandian stores).
        (98_200_060, "HerStore_Grocery"),
        // hi-byte = 0x41 — `Shop_*` (city shops).
        (1_100_170_020, "Shop_Hernand_General"),
        (1_100_170_040, "Shop_Hernand_Pub"),
    ];

    fn find_stageinfo_bytes() -> Option<Vec<u8>> {
        gamedata_layout::extract_bin(&gamedata_layout::body("stageinfo"))
    }

    /// The 2.06 `.staticinfoheader` beside the body (it holds the row
    /// names); panics when the install has the body but not the header.
    fn find_stageinfo_header() -> Vec<u8> {
        gamedata_layout::extract_bin(&gamedata_layout::header("stageinfo")).expect("the 2.06 header next to the table body (install present but header missing: drift?)")
    }

    #[test]
    fn stage_info_lossy_live() {
        let Some(data) = find_stageinfo_bytes() else {
            eprintln!("skipping stage_info_lossy_live: no game install");
            return;
        };
        let entries = parse_stage_info_lossy_with_header(&data, &find_stageinfo_header());
        println!("parsed {} stageinfo entries from {} bytes", entries.len(), data.len());
        // Every row the 2.06 header lists parses; a floor would let most of
        // the table go missing unnoticed.
        let listed = crate::binary::static_header::detect_named_pair(&find_stageinfo_header(), &data).expect("2.06 header").2.len();
        assert_eq!(entries.len(), listed, "rows parsed vs rows the header lists");
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
