//! `questgaugeinfo.pabgb` parser — anchor-scan based.
//!
//! Sibling of [`crate::mission_info`] / [`crate::quest_info`] /
//! [`crate::stage_info`]. Same row header shape, different semantics:
//!
//! QuestGauge rows are **internal-only progress meters** — kill
//! counters, faction-operation tickers, gauge bars tied to a region
//! or quest objective. They have **no PALOC entries** under any
//! namespace probed; the localized name a player sees in the UI
//! comes from the referenced stage / mission rather than the gauge
//! itself.
//!
//! Practical consequence for the C ABI bridge: only
//! `lookup_string_key` makes sense. There's no
//! `lookup_display_name` analogue — see
//! [`super::quest_gauge_info`] (the c_abi sibling) for the surface
//! and rationale.
//!
//! Names follow the convention `QuestGauge_<region_or_theme>`
//! (e.g. `QuestGauge_WindCliffFort`, `QuestGauge_AbandonedWyvernNest`,
//! `QuestGauge_DesertOasis_Reflection`). The internal name is enough
//! for a modder to identify what they're looking at, even without
//! a localized title.

/// One parsed questgaugeinfo row, reduced to what the bridge consumes.
#[derive(Debug, Clone)]
pub struct QuestGaugeInfoEntry {
    /// `QuestGaugeKey` as stored in save blocks (e.g.
    /// `_questGaugeStateList[N]._key`).
    pub key: u32,
    /// ASCII internal name (e.g. `"QuestGauge_WindCliffFort"`).
    pub name: String,
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
/// All real `QuestGauge_*` names start with `Q`, contain at least
/// one `_`, and are ≥ 13 bytes long, so the stricter rules pass
/// every real row and reject the small set of body-byte
/// false-positives the looser scanners would otherwise produce.
///
/// The same tightening could be applied to the mission / quest /
/// stage parsers; for now they stay looser because their HashMap
/// deduplication makes the false positives harmless and the tests
/// assert against real keys.
pub fn parse_quest_gauge_info_lossy(data: &[u8]) -> Vec<QuestGaugeInfoEntry> {
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
        entries.push(QuestGaugeInfoEntry { key, name });
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
                // Tighter than the sibling parsers: first byte must be
                // ASCII letter AND the name must contain at least one
                // `_`. Filters body-byte false-positives that happen
                // to lead with a UTF-8 high byte (e.g. `\xe9B`) as
                // well as short all-ASCII garbage (e.g. `HD`).
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
    //! `questgaugeinfo.pabgb`. Asserts that ~380 real gauge rows
    //! parse with `QuestGauge_*` names and that the screenshot's
    //! `_questGaugeStateList[0]._key = 1000083` row name comes back
    //! correctly.

    use crate::binary::gamedata_layout;
    use super::*;
    use std::path::PathBuf;

    /// (QuestGaugeKey, expected internal_name)
    const KNOWN: &[(u32, &str)] = &[
        (1_000_083, "QuestGauge_WindCliffFort"),
        (1_000_084, "QuestGauge_AbandonedWyvernNest"),
        (1_000_085, "QuestGauge_DesolateWyvernNest"),
        (1_000_086, "QuestGauge_ForgottenWyvernNest"),
        (1_000_091, "QuestGauge_FortManub"),
    ];

    fn find_questgauge_bytes() -> Option<Vec<u8>> {
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
            .find(|f| f.name == gamedata_layout::body("questgaugeinfo"))?;
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
    fn quest_gauge_info_lossy_live() {
        let Some(data) = find_questgauge_bytes() else {
            eprintln!("skipping quest_gauge_info_lossy_live: no game install");
            return;
        };
        let entries = parse_quest_gauge_info_lossy(&data);
        println!(
            "parsed {} questgaugeinfo entries from {} bytes",
            entries.len(),
            data.len()
        );
        // 1.06 has ~382 gauge rows after the tighter scanner filter.
        // Pin to >200 for plausibility.
        assert!(
            entries.len() > 200,
            "expected >200 gauge entries, got {}",
            entries.len()
        );

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
        // sequence the looser scanners would let through. The vast
        // majority follow the `QuestGauge_<region_or_theme>` convention;
        // 1.10's reconstruction phase introduced exactly one
        // mission-linked gauge whose name uses the `Mission_` prefix
        // instead (`Mission_Reconstruction_Node_Her_KarinQuarry`,
        // key=1000488 — confirmed a real questgaugeinfo PABGH entry, see
        // data/gamedata-keys-1.10/questgaugeinfo.txt). The scanner found
        // exactly 509 rows = the PABGH key count, so no false positives
        // slipped in. Accept both known-good prefixes.
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
