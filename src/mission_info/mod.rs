//! `missioninfo.pabgb` parser — anchor-scan based.
//!
//! Full schema RE for byte-roundtrip is **not done** for this file. The
//! Save Editor's contract only needs the `(MissionKey, internal_name)`
//! pair — the rest of each row (HP / drops / level data / etc.) is
//! opaque to the resolver chain and can be added later if needed.
//!
//! What's known about the row header (verified 2026-05-14 by anchor-
//! scanning the live 1.06 file and cross-referencing the editor's
//! handoff keycases — see `docs/save-editor-keys-plan.md`):
//!
//! ```text
//! [u32 MissionKey][u32 name_len][name_len bytes ASCII][...variable body]
//! ```
//!
//! The header is identical in shape to `iteminfo.pabgb`. The variable
//! body's size depends on the row's content (per-mission rewards / loc
//! refs / conditional fields), ranging 334–390 bytes for the prologue
//! cluster.
//!
//! Anchor scanner is byte-by-byte with rigorous validation, identical
//! in spirit to `python::scan_next_item_start` (which underpins the
//! iteminfo lossy parser). False-positive rate on the live file is
//! empirically zero — every anchor's key resolves to a valid
//! MissionKey in the save's keycases handoff.

/// One parsed missioninfo row, reduced to what the bridge consumes.
#[derive(Debug, Clone)]
pub struct MissionInfoEntry {
    /// `MissionKey` as stored in save blocks (e.g.
    /// `_missionStateList[N]._key`).
    pub key: u32,
    /// ASCII internal name (e.g. `"Mission_Intro_Tutorial_I"`). Feeds the
    /// PALOC u64 lookup chain via `hashlittle2(name) << 32 | lo32`.
    pub name: String,
}

/// Lossy anchor-scan parse of an in-memory `missioninfo.pabgb` blob.
///
/// Walks the file byte-by-byte, accepting each position where the next
/// 8 bytes plus name-bytes form a plausible row header. The validation
/// rules:
///
/// - `key != 0 && (key >> 24) == 0` — PA's gamedata keys are 7-digit
///   decimal numbers well under 2^24.
/// - `name_len ∈ [2, 128]` — internal names range from short to
///   ~70-byte qualified paths.
/// - Every byte in the name slice is an identifier byte (ASCII
///   alphanumeric / `_` / ` `) or a UTF-8 high byte. Pearl Abyss has
///   occasionally used Roman numerals (Ⅲ/Ⅳ/Ⅵ) in iteminfo names, so
///   matching iteminfo's relaxed check here keeps the scanner future-
///   proof.
///
/// Returns entries in the order they appear in the file. The same
/// MissionKey can in principle appear multiple times if Pearl Abyss
/// ever ships duplicate rows (none observed in 1.06), but the bridge's
/// HashMap will deduplicate by last-wins.
pub fn parse_mission_info_lossy(data: &[u8]) -> Vec<MissionInfoEntry> {
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
        // The scanner already validated each byte is identifier-shape
        // (ASCII or UTF-8 high). `from_utf8_lossy` would only fire on
        // an invalid UTF-8 high sequence; in practice every name in
        // 1.06 is pure ASCII so this never lossy-substitutes. Defensive
        // anyway so a future UTF-8 hiccup doesn't crash the bridge.
        let name = String::from_utf8_lossy(name_bytes).into_owned();
        entries.push(MissionInfoEntry { key, name });
        cursor = start + 8 + slen;
    }
    entries
}

/// Try to find the next plausible row start at or after `from`.
///
/// Mirrors the validation rules in
/// [`crate::python::scan_next_item_start`] modulo the trailing-NUL
/// check: iteminfo rows have an incidental NUL after the name (the next
/// field starts with `is_blocked: u8 = 0` for almost every item).
/// Missioninfo doesn't share that quirk — the byte after the name is
/// the row's first body field, which can be any value. Dropping the
/// NUL check keeps the scanner working without false negatives.
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
    //! Live-install integration test against the real missioninfo.pabgb.
    //! Skips cleanly when no Steam install is present. Synthesising a
    //! valid file from scratch is impractical (4000+ rows with variable
    //! body sizes), so the only meaningful test is against real bytes.
    //!
    //! Asserts the seven verified mappings from
    //! `docs/save-editor-keys-plan.md` ("Verified hash transform").
    //! These map save-side MissionKey values to internal names, and
    //! were independently traced to PALOC display titles. If any one
    //! regresses, the bridge's resolution chain breaks for that quest.

    use super::*;
    use std::path::PathBuf;

    /// (MissionKey, expected internal_name) pairs verified end-to-end.
    const KNOWN: &[(u32, &str)] = &[
        (1_000_157, "Mission_Intro_Tutorial_I"),
        (1_000_160, "Mission_Intro_MainBattle"),
        (1_000_620, "Mission_Intro_Abyss_Tutorial"),
        (1_000_164, "Mission_Intro_After_Horse"),
        (1_000_052, "Mission_MeetAlustain_Alustain_Strength"),
        (1_000_053, "Mission_MeetAlustain_Alustain_Wisdom"),
        (1_000_083, "Mission_IronStronghold_Block_ReturnToSister"),
    ];

    fn find_missioninfo_bytes() -> Option<Vec<u8>> {
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
        let file = dir.files.iter().find(|f| f.name == "missioninfo.pabgb")?;
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
    fn mission_info_lossy_live() {
        let Some(data) = find_missioninfo_bytes() else {
            eprintln!("skipping mission_info_lossy_live: no game install");
            return;
        };
        let entries = parse_mission_info_lossy(&data);
        println!("parsed {} missioninfo entries from {} bytes", entries.len(), data.len());

        // 1.06 sample showed ~4,300 mission entries; just assert plausible
        // population, not a fixed count (patches add/remove missions).
        assert!(
            entries.len() > 1_000,
            "expected >1000 mission entries, got {}",
            entries.len()
        );

        // Build a fast lookup and validate the seven verified mappings.
        let by_key: std::collections::HashMap<u32, &str> =
            entries.iter().map(|e| (e.key, e.name.as_str())).collect();

        for &(key, expected) in KNOWN {
            let actual = by_key.get(&key).copied();
            assert_eq!(
                actual,
                Some(expected),
                "MissionKey {} mismatch: got {:?}, expected {:?}",
                key,
                actual,
                expected,
            );
        }

        // Every key should be 7-digit-ish (< 2^24).
        for e in &entries {
            assert!(
                e.key < (1 << 24),
                "MissionKey {} ({:?}) exceeds 2^24 — scanner constraint violated",
                e.key,
                e.name,
            );
        }
    }
}
