//! `characterinfo.pabgb` parser — anchor-scan based.
//!
//! Resolves save-side `CharacterKey (u32)` — the `_characterKey` field
//! that sits alongside `FieldNPCSaveDataKey` inside every
//! `FieldNPCSaveData` block (and elsewhere; not all CharacterKeys live
//! inside FieldNPC). Pairs cleanly with PALOC at
//! `((charkey & 0xFFFFFF) << 32) | 0x30` to resolve the localized
//! display name — the cat-byte in the save's hi-byte (`0x02..=0xfe`,
//! variant / region marker) is stripped at lookup time.
//!
//! Schema is the standard PA row header, identical in shape to
//! `missioninfo.pabgb` / `gimmickinfo.pabgb`:
//!
//! ```text
//! [u32 CharacterKey][u32 name_len][name_len bytes ASCII][...variable body]
//! ```
//!
//! Constraints observed on the live 1.06 / 1.07 `characterinfo.pabgb`
//! (see `docs/save-editor-keys-plan.md` §6):
//!
//! - **Strict hi-byte**: `(key >> 24) == 0`. Row keys top out around
//!   1.7M; every real row has a zero hi-byte. The cat-byte is a
//!   save-side variant only — gamedata rows are stored with the bare
//!   24-bit ID.
//! - **All-digit names rejected**: characterinfo bodies are dense with
//!   embedded numeric stringinfo refs serialized as ASCII (e.g. `"81923"`).
//!   They pass the byte-shape check but aren't real internal names.
//!   Filtering on `bytes.iter().any(|b| !b.is_ascii_digit())` removes
//!   them with zero observed collateral on the live file.
//! - `name_len ∈ [2, 128]` — matches sibling parsers; actual range
//!   ~3-50 bytes.
//!
//! Coverage on the editor's 221-key sample save: 49/221 (22%) resolve
//! via this parser + PALOC at `lo32 = 0x30`. The 78% miss path is
//! sample-bias rather than missing data — generic field NPCs that DO
//! exist in PALOC (Peddler / Stranger / Fisherman) simply don't appear
//! as save instances in that particular save. The bridge surfaces the
//! miss as `NOT_FOUND` and lets the caller fall back to
//! `lookup_string_key` for the internal-name surface.
//!
//! See `docs/save-editor-keys-plan.md` §6 for the full investigation
//! (verified mappings: `0x07000002 → "Yann"`, `0x06000f4a → "Pierre"`,
//! `0x0a000001 → "Kliff"`, `0x09000f4d → "Noble"`).

/// One parsed characterinfo row, reduced to what the bridge consumes.
#[derive(Debug, Clone)]
pub struct CharacterInfoEntry {
    /// `CharacterKey` as stored in the row header. The save's
    /// `_characterKey` carries a cat-byte in its hi-byte; the bridge
    /// strips that via `& 0xFFFFFF` before this lookup, so values
    /// here always have hi-byte zero.
    pub key: u32,
    /// ASCII internal name (e.g. `"Yann_Friendly"`, `"FieldNPC_Bandit"`).
    /// Used as the fallback when the PALOC chain misses (the 78% case
    /// where no localized display string exists).
    pub name: String,
}

/// Lossy anchor-scan parse of an in-memory `characterinfo.pabgb` blob.
///
/// Returns entries in the order they appear in the file. The bridge's
/// `from_bytes` does first-wins HashMap insertion so a legitimate
/// duplicate key (none observed in 1.06 / 1.07) would not replace the
/// earlier entry.
pub fn parse_character_info_lossy(data: &[u8]) -> Vec<CharacterInfoEntry> {
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
        entries.push(CharacterInfoEntry { key, name });
        cursor = start + 8 + slen;
    }
    entries
}

/// Try to find the next plausible row start at or after `from`.
///
/// Same shape as `mission_info::scan_next_anchor` plus the all-digit
/// rejection — characterinfo bodies pack numeric stringinfo refs as
/// ASCII bytes, and without this filter the anchor scanner picks them
/// up as fake "internal names" (`"5"`, `"81923"`, etc.).
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
                // Reject all-digit names — those are stringinfo numeric
                // refs ASCII-serialised in row bodies, not real anchors.
                // Otherwise accept identifier-shape bytes (alnum / `_` /
                // space / UTF-8 high byte). Internal names in
                // characterinfo can start with a digit (`2H_Soldier`)
                // or underscore (`_NPC_Bandit`), so we don't pin the
                // first byte to ASCII letter the way missioninfo does
                // — `any-non-digit` plus a valid-UTF-8 check is
                // sufficient to filter the numeric-ref body noise.
                let has_non_digit = bytes.iter().any(|&b| !b.is_ascii_digit());
                if has_non_digit
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
    //! `characterinfo.pabgb`. Pins the four mappings verified during
    //! the 2026-05-14 investigation (`docs/save-editor-keys-plan.md`
    //! §6) so a future patch breaking the row schema is caught
    //! immediately. Skips cleanly when no game install is present.

    use crate::binary::gamedata_layout;
    use super::*;
    use std::path::PathBuf;

    /// CharacterKey lo24 values that must exist in `characterinfo.pabgb`.
    /// Derived from the §6 PALOC-verified save-side keys with the
    /// cat-byte stripped (`& 0xFFFFFF`):
    ///
    /// | Save key     | lo24    | PALOC display @ lo32=0x30 |
    /// |--------------|---------|---------------------------|
    /// | 0x07000002   | 0x00002 | "Yann"                    |
    /// | 0x06000f4a   | 0x00f4a | "Pierre"                  |
    /// | 0x0a000001   | 0x00001 | "Kliff"                   |
    /// | 0x09000f4d   | 0x00f4d | "Noble"                   |
    ///
    /// The parser test only asserts that each lo24 has a non-empty,
    /// printable ASCII internal name — the actual string contents
    /// depend on the patch and aren't worth pinning here. PALOC
    /// display-name verification is the c_abi bridge's job.
    const KNOWN_LO24: &[u32] = &[0x000001, 0x000002, 0x000f4a, 0x000f4d];

    fn find_characterinfo_bytes() -> Option<Vec<u8>> {
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
        let file = dir.files.iter().find(|f| f.name == gamedata_layout::body("characterinfo"))?;
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
    fn character_info_lossy_live() {
        let Some(data) = find_characterinfo_bytes() else {
            eprintln!("skipping character_info_lossy_live: no game install");
            return;
        };
        let entries = parse_character_info_lossy(&data);
        println!(
            "parsed {} characterinfo entries from {} bytes",
            entries.len(),
            data.len()
        );
        // 1.07 yields ~7k anchor-scannable rows from the 24 MB file
        // (§6's ~17k figure was the 1.06 prototype against the 1.5 MB
        // file; the row schema's variable body grew with the dataset).
        // Pin >5000 as a loose plausibility floor that tolerates
        // patches adding / removing characters.
        assert!(
            entries.len() > 5_000,
            "expected >5000 characterinfo entries, got {}",
            entries.len()
        );

        // First-wins to mirror bridge behaviour. Existence + non-empty
        // name is the contract — exact internal-name strings vary
        // between patches and we don't want to lock the test to
        // 1.06 spellings.
        let mut by_key: std::collections::HashMap<u32, &str> = Default::default();
        for e in &entries {
            by_key.entry(e.key).or_insert(e.name.as_str());
        }
        for &key in KNOWN_LO24 {
            assert!(
                by_key.contains_key(&key),
                "CharacterKey lo24 0x{key:06x} not found in characterinfo"
            );
            let name = by_key[&key];
            assert!(
                !name.is_empty() && name.chars().all(|c| !c.is_control()),
                "CharacterKey 0x{key:06x} has bad name {name:?}"
            );
        }
    }
}
