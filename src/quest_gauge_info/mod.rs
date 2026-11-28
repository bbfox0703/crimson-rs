//! `questgaugeinfo.pabgb` parser — anchor-scan based.
//!
//! Sibling of [`crate::mission_info`] / [`crate::quest_info`] /
//! [`crate::stage_info`]. Same legacy row head shape (`[u32 name_len][name]
//! [u32 key][u8 0x00]...`), different semantics:
//!
//! QuestGauge rows are **internal-only progress meters** — kill
//! counters, faction-operation tickers, gauge bars tied to a region
//! or quest objective. They have **no PALOC entries** under any
//! namespace probed; the localized name a player sees in the UI
//! comes from the referenced stage / mission rather than the gauge
//! itself.
//!
//! The C ABI bridge therefore offers `lookup_string_key` and no
//! `lookup_display_name` analogue — see `c_abi::quest_gauge_info`
//! for the surface and rationale.
//!
//! Names follow the convention `QuestGauge_<region_or_theme>`
//! (e.g. `QuestGauge_WindCliffFort`, `QuestGauge_AbandonedWyvernNest`,
//! `QuestGauge_DesertOasis_Reflection`). The internal name is enough
//! for a modder to identify what they're looking at, even without
//! a localized title.

use crate::binary::row_head::{is_row_name_byte, read_row_head};

/// One parsed questgaugeinfo row, reduced to what the bridge consumes.
#[derive(Debug, Clone)]
pub struct QuestGaugeInfoEntry {
    /// `QuestGaugeKey` as stored in save blocks (e.g.
    /// `_questGaugeStateList[N]._key`).
    pub key: u32,
    /// ASCII internal name (e.g. `"QuestGauge_WindCliffFort"`).
    pub name: String,
}

/// 2.06+: row names live in the `.staticinfoheader`, which lists every row,
/// so with a 2.06 header the rows come from it, in header order (the anchor
/// scan drops rows its key and name rules do not expect). A pre-2.06
/// pair, or a header that is not the 2.06 shape, falls back to scanning `data`.
pub fn parse_quest_gauge_info_lossy_with_header(data: &[u8], header: &[u8]) -> Vec<QuestGaugeInfoEntry> {
    match crate::binary::static_header::header_row_names(data, header) {
        Some(rows) => rows.into_iter().map(|(key, name)| QuestGaugeInfoEntry { key, name }).collect(),
        None => parse_quest_gauge_info_lossy(data),
    }
}

/// Lossy anchor-scan parse of an in-memory `questgaugeinfo.pabgb` blob.
///
/// Validation rules are tighter than the mission / quest / stage
/// scanners:
///
/// - The name's first byte must be an ASCII letter (filters
///   `\xe9B`-style UTF-8-leading body-byte garbage).
/// - The name must contain at least one `_` (filters 2- to 3-letter
///   body-byte garbage like `"HD"`, where both bytes are ASCII
///   letters but the sequence is not a real row name).
///
/// Real gauge names (`QuestGauge_*`, and the `Mission_*`-named gauges)
/// start with a letter and contain at least one `_`, so the stricter
/// rules pass every real row and reject the small set of body-byte
/// false-positives the looser scanners would otherwise produce.
///
/// The same tightening could be applied to the mission / quest /
/// stage parsers. They stay looser: their bridges dedupe first-wins,
/// which keeps a real row over a later false positive with the same key
/// (not over an earlier one), and the tests assert against real keys.
pub fn parse_quest_gauge_info_lossy(data: &[u8]) -> Vec<QuestGaugeInfoEntry> {
    let mut entries = Vec::new();
    let mut cursor = 0;
    while let Some(start) = scan_next_anchor(data, cursor) {
        // Scanner already validated the head — the unwrap is sound.
        let head = read_row_head(&data[start..], 4).unwrap();
        entries.push(QuestGaugeInfoEntry {
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
            // Tighter than the sibling parsers: first byte must be ASCII letter
            // AND the name must contain at least one `_`. Filters body-byte
            // false-positives that happen to lead with a UTF-8 high byte (e.g.
            // `\xe9B`) as well as short all-ASCII garbage (e.g. `HD`).
            if key != 0
                && (key >> 24) == 0
                && bytes[0].is_ascii_alphabetic()
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
    //! Live-install integration test against the real
    //! `questgaugeinfo.pabgb`. Asserts that every header row parses
    //! with a gauge-shaped name and that the `KNOWN` keys resolve
    //! (1000083 is a live save's `_questGaugeStateList[0]._key`).

    use crate::binary::gamedata_layout;
    use super::*;

    /// (QuestGaugeKey, expected internal_name)
    const KNOWN: &[(u32, &str)] = &[
        (1_000_083, "QuestGauge_WindCliffFort"),
        (1_000_084, "QuestGauge_AbandonedWyvernNest"),
        (1_000_085, "QuestGauge_DesolateWyvernNest"),
        (1_000_086, "QuestGauge_ForgottenWyvernNest"),
        (1_000_091, "QuestGauge_FortManub"),
    ];

    fn find_questgauge_bytes() -> Option<Vec<u8>> {
        gamedata_layout::extract_bin(&gamedata_layout::body("questgaugeinfo"))
    }

    /// The 2.06 `.staticinfoheader` beside the body (it holds the row
    /// names); panics when the install has the body but not the header.
    fn find_questgauge_header() -> Vec<u8> {
        gamedata_layout::extract_bin(&gamedata_layout::header("questgaugeinfo")).expect("the 2.06 header next to the table body (install present but header missing: drift?)")
    }

    #[test]
    fn quest_gauge_info_lossy_live() {
        let Some(data) = find_questgauge_bytes() else {
            eprintln!("skipping quest_gauge_info_lossy_live: no game install");
            return;
        };
        let entries = parse_quest_gauge_info_lossy_with_header(&data, &find_questgauge_header());
        println!(
            "parsed {} questgaugeinfo entries from {} bytes",
            entries.len(),
            data.len()
        );
        // Every row the 2.06 header lists parses; a floor would let most of
        // the table go missing unnoticed.
        let listed = crate::binary::static_header::detect_named_pair(&find_questgauge_header(), &data).expect("2.06 header").2.len();
        assert_eq!(entries.len(), listed, "rows parsed vs rows the header lists");

        let by_key: std::collections::HashMap<u32, &str> =
            entries.iter().map(|e| (e.key, e.name.as_str())).collect();
        for &(key, expected) in KNOWN {
            assert_eq!(
                by_key.get(&key).copied(),
                Some(expected),
                "QuestGaugeKey {} mismatch",
                key,
            );
        }

        // Every name should be a real row name, not a body-byte garbage
        // sequence the looser scanners would let through. Names follow
        // the `QuestGauge_<region_or_theme>` convention, except
        // mission-linked gauges, which use the `Mission_` prefix (e.g.
        // `Mission_Reconstruction_Node_Her_KarinQuarry`, key 1000488, a
        // real questgaugeinfo PABGH entry). Accept both prefixes.
        for e in &entries {
            assert!(
                e.name.starts_with("QuestGauge_") || e.name.starts_with("Mission_"),
                "non-gauge name slipped through: key={}, name={:?}",
                e.key,
                e.name,
            );
        }
    }
}
