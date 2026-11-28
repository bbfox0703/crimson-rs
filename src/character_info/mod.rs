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
//! Schema is the standard PA row head, identical in shape to
//! `missioninfo.pabgb` / `gimmickinfo.pabgb`. A pre-2.06 body row opens with
//! the name, then the key, then a 0x00 byte:
//!
//! ```text
//! [u32 name_len][name_len bytes ASCII][u32 CharacterKey][u8 0x00][...variable body]
//! ```
//!
//! The anchor scan relies on these rules (see `docs/save-editor-keys-plan.md` §6):
//!
//! - **Strict hi-byte**: `(key >> 24) == 0`. The cat-byte is a save-side
//!   variant only — gamedata rows are stored with the bare 24-bit ID. A
//!   minority of rows have a hashed 32-bit key (`NHM_Noble_1` = 0x77395fed,
//!   …), which the scanner does not see.
//! - **All-digit names rejected**: characterinfo bodies are dense with
//!   embedded numeric stringinfo refs serialized as ASCII (e.g. `"81923"`).
//!   They pass the byte-shape check but aren't real internal names.
//! - `name_len ∈ [2, 128]` — matches sibling parsers; real names are
//!   ~3-50 bytes.
//!
//! Many save-side CharacterKeys have no localized display string at
//! `lo32 = 0x30`. The bridge surfaces that miss as `NOT_FOUND` and lets the
//! caller fall back to `lookup_string_key` for the internal-name surface.
//!
//! ## FactionKey
//!
//! Every row carries the character's `FactionKey` (the `faction` table's key,
//! 0 for none) as a `u32` at a fixed position in the fixed-width head of the
//! body:
//!
//! - pre-2.06 layout: byte 149 after the key ([`FACTION_AT_PRE_2_06`]).
//! - 2.06 layout: byte 151 of the row after `[key][u32 row index]`
//!   ([`FACTION_AT_2_06`]); 2.06 inserted two zero bytes earlier in the
//!   fixed head.
//!
//! The value is a `faction` key or 0 on every row. The save does not record
//! an NPC's faction; this table is the source.
//!
//! 2.06 also inlined anonymous conditions in this row as `01` + compiled
//! bytecode ("Inlined conditions (2.06)" in `scripts/CLAUDE.md`); nothing
//! here reads past the fixed head, so that does not affect this parser.
//!
//! On 2.06 the header lists every row, so [`parse_character_info_lossy_with_header`]
//! reads the rows from it instead of anchor-scanning, which also returns the
//! rows with a hashed 32-bit key.
//!
//! See `docs/save-editor-keys-plan.md` §6 for the save-key mappings
//! (`0x07000002 → "Yann"`, `0x06000f4a → "Pierre"`, `0x0a000001 → "Kliff"`,
//! `0x09000f4d → "Noble"`).

use crate::binary::row_head::{is_row_name_byte, read_row_head};

/// One parsed characterinfo row, reduced to what the bridge consumes.
#[derive(Debug, Clone)]
pub struct CharacterInfoEntry {
    /// `CharacterKey` as stored in the row header. The save's
    /// `_characterKey` carries a cat-byte in its hi-byte; the bridge
    /// strips that via `& 0xFFFFFF` before this lookup, so values
    /// here always have hi-byte zero.
    pub key: u32,
    /// ASCII internal name (e.g. `"Yann_Friendly"`, `"FieldNPC_Bandit"`).
    /// Used as the fallback when the PALOC chain misses (no localized
    /// display string exists).
    pub name: String,
    /// The character's `FactionKey` (a `faction` row key), 0 when it has none
    /// or the row is too short to hold the field.
    pub faction_key: u32,
}

/// Where `FactionKey` sits in a pre-2.06 row: bytes after the row key.
pub const FACTION_AT_PRE_2_06: usize = 149;
/// Where `FactionKey` sits on 2.06: bytes after the row's `[key][u32 index]`.
pub const FACTION_AT_2_06: usize = 151;

fn read_faction(rest: &[u8], at: usize) -> u32 {
    rest.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .unwrap_or(0)
}

/// 2.06+: row names live in the `.staticinfoheader`, not in the body. With
/// a 2.06 header every row comes from it, in header
/// order (hashed 32-bit keys included); otherwise `data` is anchor-scanned as a
/// pre-2.06 body.
pub fn parse_character_info_lossy_with_header(data: &[u8], header: &[u8]) -> Vec<CharacterInfoEntry> {
    use crate::binary::static_header::{detect_named_pair, read_body_row};
    let Some((_, key_width, rows)) = detect_named_pair(header, data) else {
        return parse_character_info_lossy(data);
    };
    rows.iter()
        .filter_map(|r| {
            let row = read_body_row(data.get(r.range())?, key_width)?;
            Some(CharacterInfoEntry {
                key: r.key as u32,
                name: r.name.to_owned(),
                faction_key: read_faction(row.rest, FACTION_AT_2_06),
            })
        })
        .collect()
}

/// Lossy anchor-scan parse of an in-memory `characterinfo.pabgb` blob.
///
/// Returns entries in the order they appear in the file. The bridge's
/// `from_bytes` does first-wins HashMap insertion so a duplicate key
/// (a false anchor in a body string) does not replace the earlier entry.
pub fn parse_character_info_lossy(data: &[u8]) -> Vec<CharacterInfoEntry> {
    let mut entries = Vec::new();
    let mut cursor = 0;
    while let Some(start) = scan_next_anchor(data, cursor) {
        // Scanner already validated the head — the unwrap is sound.
        let head = read_row_head(&data[start..], 4).unwrap();
        entries.push(CharacterInfoEntry {
            key: head.key,
            name: head.name.to_owned(),
            faction_key: read_faction(&data[start + head.body_at..], FACTION_AT_PRE_2_06),
        });
        cursor = start + head.body_at;
    }
    entries
}

/// Try to find the next plausible row start at or after `from`.
///
/// Same shape as `mission_info::scan_next_anchor` plus the all-digit
/// rejection — characterinfo bodies pack numeric stringinfo refs as
/// ASCII bytes, and without this filter the anchor scanner picks them
/// up as fake "internal names" (`"5"`, `"81923"`, etc.). Body strings
/// followed by a small number still pass, so the bridge's first-wins dedup
/// stays necessary.
fn scan_next_anchor(data: &[u8], from: usize) -> Option<usize> {
    let n = data.len();
    let mut o = from;
    while o + 12 < n {
        let slen = u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]) as usize;
        // Length, name, key, then the 0x00 that opens every pre-2.06 row body —
        // the byte that rejects most body strings followed by a small number.
        if (2..=128).contains(&slen) && o + 9 + slen <= n && data[o + 8 + slen] == 0 {
            let bytes = &data[o + 4..o + 4 + slen];
            let key = u32::from_le_bytes([
                data[o + 4 + slen],
                data[o + 5 + slen],
                data[o + 6 + slen],
                data[o + 7 + slen],
            ]);
            // Key rule and all-digit rejection as documented on the module.
            // Names can start
            // with a digit (`2H_Soldier`) or underscore (`_NPC_Bandit`), so the
            // first byte is not pinned to a letter the way missioninfo does.
            if key != 0
                && (key >> 24) == 0
                && bytes.iter().any(|&b| !b.is_ascii_digit())
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
    //! `characterinfo.pabgb`. Pins the four save-key mappings of
    //! `docs/save-editor-keys-plan.md` §6 so a patch breaking the row schema
    //! is caught. Skips cleanly when no game install is present.

    use crate::binary::gamedata_layout;
    use super::*;

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

    /// (CharacterKey, FactionKey) pairs the table must hold.
    const KNOWN_FACTION: &[(u32, u32)] = &[
        (1, 1_000_000),         // Kliff → Faction_Graymane
        (4, 1_000_000),         // Damian
        (1_001_367, 1_000_000), // Kliff_Clone
        (53_213, 1_000_131),    // NHM_Hernand_Soldiers_TwoHandSword_53213 → Faction_Celeste_Hernand
        (50_001, 1_000_128),    // NHM_Bandit_OneHandAxe_50001 → Faction_Bandit
        (1_000_173, 0),         // MainVehicle: none
    ];

    fn find_characterinfo_bytes() -> Option<Vec<u8>> {
        gamedata_layout::extract_bin(&gamedata_layout::body("characterinfo"))
    }

    /// 2.06+: the row names live here. Empty when absent (an older install
    /// needs none, and `parse_character_info_lossy_with_header` then scans the body alone).
    fn find_characterinfo_header() -> Vec<u8> {
        gamedata_layout::extract_bin(&gamedata_layout::header("characterinfo")).expect("the 2.06 header next to the table body (install present but header missing: drift?)")
    }

    #[test]
    fn character_info_lossy_live() {
        let Some(data) = find_characterinfo_bytes() else {
            eprintln!("skipping character_info_lossy_live: no game install");
            return;
        };
        let entries = parse_character_info_lossy_with_header(&data, &find_characterinfo_header());
        println!(
            "parsed {} characterinfo entries from {} bytes",
            entries.len(),
            data.len()
        );
        // Every row the 2.06 header lists parses; a floor would let most of
        // the table go missing unnoticed.
        let listed = crate::binary::static_header::detect_named_pair(&find_characterinfo_header(), &data).expect("2.06 header").2.len();
        assert_eq!(entries.len(), listed, "rows parsed vs rows the header lists");

        // First-wins to mirror bridge behaviour. Existence + non-empty
        // name is the contract — exact internal-name strings vary
        // between patches.
        let mut by_key: std::collections::HashMap<u32, &str> = Default::default();
        for e in &entries {
            by_key.entry(e.key).or_insert(e.name.as_str());
        }
        let faction: std::collections::HashMap<u32, u32> =
            entries.iter().map(|e| (e.key, e.faction_key)).collect();
        for &(key, expected) in KNOWN_FACTION {
            assert_eq!(faction.get(&key).copied(), Some(expected), "CharacterKey {key} faction");
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
