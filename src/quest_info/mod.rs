//! `questinfo.pabgb` parser — anchor-scan based.
//!
//! Sibling of [`crate::mission_info`]. Same architecture and same row
//! header shape (`[u32 key][u32 name_len][name_bytes][...body]`).
//!
//! Pearl Abyss's terminology assignment is the inverse of what the
//! English UI shows:
//!
//! - PA's `Mission_*` entries (in `missioninfo.pabgb`) = English UI's
//!   "individual quest line items" (e.g. "Unfamiliar Lands").
//! - PA's `Quest_*` entries (in this file, `questinfo.pabgb`) = English
//!   UI's "quest arcs / region headings / challenge groups"
//!   (e.g. "Roothold", "Mazes", "Record of the Greymanes").
//!
//! Resolution defaults differ from missioninfo:
//! `lo32 = 0x100` is the common namespace for arc/region titles here,
//! versus `lo32 = 0x101` for individual quest titles in missioninfo.
//! The bridge's `lookup_display_name` takes `lo32_namespace` as an
//! argument so the caller chooses per-row.

/// One parsed questinfo row, reduced to what the bridge consumes.
#[derive(Debug, Clone)]
pub struct QuestInfoEntry {
    /// `QuestKey` as stored in save blocks (e.g.
    /// `_questStateList[N]._questKey`).
    pub key: u32,
    /// ASCII internal name (e.g. `"Quest_Node_Her_RootFort_Normal"`).
    /// Feeds the PALOC u64 lookup chain via `hashlittle2(name) << 32 |
    /// lo32`.
    pub name: String,
}

/// Lossy anchor-scan parse of an in-memory `questinfo.pabgb` blob.
///
/// See [`crate::mission_info::parse_mission_info_lossy`] for the
/// validation rules — identical here.
pub fn parse_quest_info_lossy(data: &[u8]) -> Vec<QuestInfoEntry> {
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
        let name = String::from_utf8_lossy(name_bytes).into_owned();
        entries.push(QuestInfoEntry { key, name });
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
                if bytes.iter().all(|&b| is_ident_byte(b)) {
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
    //! Live-install integration test against the real `questinfo.pabgb`.
    //! Asserts five verified mappings from
    //! `docs/save-editor-keys-plan.md`.

    use super::*;
    use std::path::PathBuf;

    const KNOWN: &[(u32, &str)] = &[
        (1_000_619, "Quest_Node_Her_RootFort_Normal"),
        (1_000_881, "Quest_Node_Her_GreymaneCamp_Contents"),
        (1_001_032, "Quest_HumanDocumentary_Del"),
        (1_000_039, "Challenge_Maze"),
        (1_000_180, "Quest_BloodCoronation_WitchDukeAndDream"),
    ];

    fn find_questinfo_bytes() -> Option<Vec<u8>> {
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
        let file = dir.files.iter().find(|f| f.name == "questinfo.pabgb")?;
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
    fn quest_info_lossy_live() {
        let Some(data) = find_questinfo_bytes() else {
            eprintln!("skipping quest_info_lossy_live: no game install");
            return;
        };
        let entries = parse_quest_info_lossy(&data);
        println!("parsed {} questinfo entries from {} bytes", entries.len(), data.len());
        assert!(
            entries.len() > 200,
            "expected >200 quest entries, got {}",
            entries.len()
        );
        let by_key: std::collections::HashMap<u32, &str> =
            entries.iter().map(|e| (e.key, e.name.as_str())).collect();
        for &(key, expected) in KNOWN {
            assert_eq!(
                by_key.get(&key).copied(),
                Some(expected),
                "QuestKey {} mismatch",
                key,
            );
        }
    }
}
