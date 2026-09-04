//! `knowledgeinfo.pabgb` parser — anchor-scan based.
//!
//! Sibling of [`crate::mission_info`] / [`crate::quest_info`] /
//! [`crate::stage_info`]. Same row header shape, **same hash hop
//! pattern** — contrary to the editor's earlier hypothesis (status.md
//! item #6) that KnowledgeKey wouldn't resolve through PALOC, it does.
//! The probe (recorded in `docs/save-editor-keys-plan.md`) found
//! 29,330 PALOC hits across multiple namespaces when every
//! `Knowledge_*` name in this file was hashed.
//!
//! Common PALOC namespace bytes for knowledge rows:
//! - `lo32 = 0x490` (1168) — knowledge entry **title** (one per row)
//! - `lo32 = 0x491` (1169) — knowledge entry **description** (short)
//! - `lo32 = 0x49F` (1183) — description (alternate, identical to
//!   0x491 for many rows; appears on every row)
//! - `lo32 = 0x49D` (1181) / `0x49E` (1182) — secondary text on
//!   ~70% of rows; appears to be a "lore" or "discovery" variant
//!
//! Names follow `Knowledge_<theme>_<identifier>` (e.g.
//! `Knowledge_Node_Dem_Ruins_0007`, `Knowledge_Demian_Plate_Boots_V`,
//! `Knowledge_AbyssRuins_Dem_0020`, `Knowledge_Hp`).
//!
//! ## Two-namespace split (status.md §834 finding)
//!
//! The editor noted that **small KnowledgeKey values (1, 2, 4, 7, 51)
//! sit at PALOC 0x93 as knowledge *category* names** ("Various Combat
//! Skills", "Fundamentals of Cooking"), while large-numbered keys
//! don't appear there. The categorization lives in
//! `knowledgegroupinfo.pabgb` as integer category IDs (e.g.
//! Creatures=4, Terrestrial Creatures=104, Amphibians=1045 at
//! `lo32=0x92/0x93`). That's a separate bridge concern — this parser
//! and its bridge surface the *leaf* knowledge rows; the category
//! rollup would be a future enhancement on top.

/// One parsed knowledgeinfo row, reduced to what the bridge consumes.
#[derive(Debug, Clone)]
pub struct KnowledgeInfoEntry {
    /// `KnowledgeKey` as stored in save blocks (e.g.
    /// `_knowledgeStateList[N]._key`).
    pub key: u32,
    /// ASCII internal name (e.g. `"Knowledge_Node_Dem_Ruins_0007"`).
    /// Feeds the PALOC u64 lookup chain via `hashlittle2(name) << 32
    /// | lo32`.
    pub name: String,
}

/// Lossy anchor-scan parse of an in-memory `knowledgeinfo.pabgb` blob.
///
/// Uses the tightened validator that the quest_gauge_info parser
/// pioneered (first byte ASCII letter + must contain at least one
/// `_`). All real `Knowledge_*` names satisfy both rules, so the
/// strictness doesn't cost real entries; it just suppresses
/// body-byte false-positives.
pub fn parse_knowledge_info_lossy(data: &[u8]) -> Vec<KnowledgeInfoEntry> {
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
        entries.push(KnowledgeInfoEntry { key, name });
        cursor = start + 8 + slen;
    }
    entries
}

fn scan_next_anchor(data: &[u8], from: usize) -> Option<usize> {
    let n = data.len();
    let mut o = from;
    while o + 12 < n {
        let key = u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
        // Knowledge rows mostly use hi-byte=0 (regular knowledge IDs)
        // but a chunk use hi-byte=0x7F for the auto-unlock / recipe
        // category — e.g. `0x7FFFFD9F → "Knowledge_Recipe_Jerky"`,
        // `0x7FFFFDAE → "Knowledge_Colette"`. The handoff bundle had
        // 215 of these flagged as unresolved before the cap got
        // widened. `< 0x80` covers them plus headroom and excludes
        // 0x80+ body-byte noise.
        if key != 0 && (key >> 24) < 0x80 {
            let slen = u32::from_le_bytes([
                data[o + 4],
                data[o + 5],
                data[o + 6],
                data[o + 7],
            ]) as usize;
            if (2..=128).contains(&slen) && o + 8 + slen <= n {
                let bytes = &data[o + 8..o + 8 + slen];
                if bytes[0].is_ascii_alphabetic()
                    && bytes.contains(&b'_')
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
    //! `knowledgeinfo.pabgb`.

    use crate::binary::gamedata_layout;
    use super::*;
    use std::path::PathBuf;

    /// (KnowledgeKey, expected internal_name) — five rows verified
    /// against the live install. Each was independently hash-matched
    /// to a PALOC title (see the bridge tests).
    const KNOWN: &[(u32, &str)] = &[
        // hi-byte=0 — regular knowledge entries
        (1_000_424, "Knowledge_Hp"),
        (1_002_588, "Knowledge_Node_Dem_Ruins_0007"),
        (1_002_294, "Knowledge_Node_Dem_HiddenCave"),
        (1_002_763, "Knowledge_AbyssRuins_Dem_0020"),
        (1_004_037, "Knowledge_Demian_Plate_Boots_V"),
        // Regression for CrimsonAtomtic editor issue #2:
        // hi-byte=0x7F — auto-unlock / recipe / named-NPC knowledge.
        (0x7fff_fd9f, "Knowledge_Recipe_Jerky"),
        (0x7fff_fda0, "Knowledge_Recipe_Jerky_Pieces"),
        (0x7fff_fdae, "Knowledge_Colette"),
    ];

    fn find_knowledge_bytes() -> Option<Vec<u8>> {
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
        let file = dir
            .files
            .iter()
            .find(|f| f.name == gamedata_layout::body("knowledgeinfo"))?;
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
    fn knowledge_info_lossy_live() {
        let Some(data) = find_knowledge_bytes() else {
            eprintln!("skipping knowledge_info_lossy_live: no game install");
            return;
        };
        let entries = parse_knowledge_info_lossy(&data);
        println!(
            "parsed {} knowledgeinfo entries from {} bytes",
            entries.len(),
            data.len()
        );
        // 1.06 has ~5,400 knowledge rows. Pin to >1000 for plausibility.
        assert!(
            entries.len() > 1_000,
            "expected >1000 knowledge entries, got {}",
            entries.len()
        );

        let by_key: std::collections::HashMap<u32, &str> =
            entries.iter().map(|e| (e.key, e.name.as_str())).collect();
        for &(key, expected) in KNOWN {
            assert_eq!(
                by_key.get(&key).copied(),
                Some(expected),
                "KnowledgeKey {} mismatch",
                key,
            );
        }

        // Sanity: at least 95% of names start with Knowledge_. The
        // scanner can pick up a small number of body-byte
        // false-positives (e.g. region-name-shaped sequences like
        // `Valley_of_Vultures` lurking inside row bodies) but they're
        // harmless — the HashMap dedupes by key, so real
        // KnowledgeKey lookups return the correct name. Strict
        // `starts_with` would flake on those rare hits.
        let knowledge_prefixed = entries
            .iter()
            .filter(|e| e.name.starts_with("Knowledge_"))
            .count();
        let ratio = knowledge_prefixed as f32 / entries.len() as f32;
        assert!(
            ratio > 0.95,
            "expected >95% of names to start with Knowledge_, got {:.2}%",
            ratio * 100.0,
        );
    }
}
