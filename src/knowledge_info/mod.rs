//! `knowledgeinfo.pabgb` parser — anchor-scan based.
//!
//! Sibling of [`crate::mission_info`] / [`crate::quest_info`] /
//! [`crate::stage_info`]. Same legacy row head shape (`[u32 name_len][name]
//! [u32 key][u8 0x00]...`), **same hash hop pattern**: hashed
//! `Knowledge_*` names resolve through PALOC across several namespaces
//! (the probe is recorded in `docs/save-editor-keys-plan.md`).
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
//! ## Two namespaces
//!
//! Small integers at PALOC `lo32 = 0x92 / 0x93` are knowledge **group**
//! names ("Various Combat Skills", "Fundamentals of Cooking"): their hi32
//! is a literal group ID from `knowledgegroupinfo.pabgb` (e.g.
//! Creatures=4, Terrestrial Creatures=104, Amphibians=1045), not a
//! hashed name. This parser and its bridge surface the *leaf* knowledge
//! rows, whose titles use the hash hop at `lo32 = 0x490`; the group
//! rollup is not parsed.

use crate::binary::row_head::{is_row_name_byte, read_row_head};

/// One parsed knowledgeinfo row, reduced to what the bridge consumes.
#[derive(Debug, Clone)]
pub struct KnowledgeInfoEntry {
    /// `KnowledgeKey` as stored in save blocks (e.g.
    /// `_knowledgeStateList[N]._key`).
    pub key: u32,
    /// ASCII internal name (e.g. `"Knowledge_Node_Dem_Ruins_0007"`).
    /// Feeds the PALOC u64 lookup chain via `hashlittle(name) << 32
    /// | lo32`.
    pub name: String,
}

/// 2.06+: row names live in the `.staticinfoheader`, which lists every row,
/// so with a 2.06 header the rows come from it, in header order (the anchor
/// scan drops rows its key and name rules do not expect). A pre-2.06
/// pair, or a header that is not the 2.06 shape, falls back to scanning `data`.
pub fn parse_knowledge_info_lossy_with_header(data: &[u8], header: &[u8]) -> Vec<KnowledgeInfoEntry> {
    match crate::binary::static_header::header_row_names(data, header) {
        Some(rows) => rows.into_iter().map(|(key, name)| KnowledgeInfoEntry { key, name }).collect(),
        None => parse_knowledge_info_lossy(data),
    }
}

/// Lossy anchor-scan parse of an in-memory `knowledgeinfo.pabgb` blob.
///
/// Uses the tightened validator of the quest_gauge_info parser (first
/// byte an ASCII letter, at least one `_`). Real `Knowledge_*` names
/// satisfy both rules, so the strictness doesn't cost real entries; it
/// suppresses body-byte false-positives.
pub fn parse_knowledge_info_lossy(data: &[u8]) -> Vec<KnowledgeInfoEntry> {
    let mut entries = Vec::new();
    let mut cursor = 0;
    while let Some(start) = scan_next_anchor(data, cursor) {
        // Scanner already validated the head — the unwrap is sound.
        let head = read_row_head(&data[start..], 4).unwrap();
        entries.push(KnowledgeInfoEntry {
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
            // Knowledge rows mostly use hi-byte=0 (regular knowledge IDs)
            // but a chunk use hi-byte=0x7F for the auto-unlock / recipe
            // category — e.g. `0x7FFFFD9F → "Knowledge_Recipe_Jerky"`,
            // `0x7FFFFDAE → "Knowledge_Colette"`. `< 0x80` covers them
            // plus headroom and excludes 0x80+ body-byte noise.
            if key != 0
                && (key >> 24) < 0x80
                && bytes[0].is_ascii_alphabetic()
                && bytes.contains(&b'_')
                // One name has an apostrophe (`Knowledge_King's_Tomb_Cave`, the
                // knowledge naming `Node_Her_Region_Cave_0080`); the shared rule
                // rejects it, so it is allowed here.
                && bytes.iter().all(|&b| is_row_name_byte(b) || b == b'\'')
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
    //! Live-install integration test against the real
    //! `knowledgeinfo.pabgb`.

    use crate::binary::gamedata_layout;
    use super::*;

    /// (KnowledgeKey, expected internal_name), covering both key hi-byte
    /// categories and the apostrophe name.
    const KNOWN: &[(u32, &str)] = &[
        // hi-byte=0 — regular knowledge entries
        (1_000_424, "Knowledge_Hp"),
        (1_002_588, "Knowledge_Node_Dem_Ruins_0007"),
        (1_002_294, "Knowledge_Node_Dem_HiddenCave"),
        (1_002_763, "Knowledge_AbyssRuins_Dem_0020"),
        (1_004_037, "Knowledge_Demian_Plate_Boots_V"),
        // hi-byte=0x7F — auto-unlock / recipe / named-NPC knowledge.
        (0x7fff_fd9f, "Knowledge_Recipe_Jerky"),
        (0x7fff_fda0, "Knowledge_Recipe_Jerky_Pieces"),
        (0x7fff_fdae, "Knowledge_Colette"),
        // The name with an apostrophe.
        (1_000_535, "Knowledge_King's_Tomb_Cave"),
    ];

    fn find_knowledge_bytes() -> Option<Vec<u8>> {
        gamedata_layout::extract_bin(&gamedata_layout::body("knowledgeinfo"))
    }

    /// The 2.06 `.staticinfoheader` beside the body (it holds the row
    /// names); panics when the install has the body but not the header.
    fn find_knowledge_header() -> Vec<u8> {
        gamedata_layout::extract_bin(&gamedata_layout::header("knowledgeinfo")).expect("the 2.06 header next to the table body (install present but header missing: drift?)")
    }

    #[test]
    fn knowledge_info_lossy_live() {
        let Some(data) = find_knowledge_bytes() else {
            eprintln!("skipping knowledge_info_lossy_live: no game install");
            return;
        };
        let entries = parse_knowledge_info_lossy_with_header(&data, &find_knowledge_header());
        println!(
            "parsed {} knowledgeinfo entries from {} bytes",
            entries.len(),
            data.len()
        );
        // Every row the 2.06 header lists parses; a floor would let most of
        // the table go missing unnoticed.
        let listed = crate::binary::static_header::detect_named_pair(&find_knowledge_header(), &data).expect("2.06 header").2.len();
        assert_eq!(entries.len(), listed, "rows parsed vs rows the header lists");

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
        // `Valley_of_Vultures` lurking inside row bodies). The bridge
        // dedupes first-wins, so a real KnowledgeKey keeps its name
        // unless a false-positive with the same key comes before it;
        // the KNOWN pins above check that. Strict `starts_with` would
        // flake on those rare hits.
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
