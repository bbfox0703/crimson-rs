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
//! `missioninfo.pabgb` / `gimmickinfo.pabgb`. 2.04 moved the key after the
//! name, and the body now always opens with a 0x00 byte:
//!
//! ```text
//! [u32 name_len][name_len bytes ASCII][u32 CharacterKey][u8 0x00][...variable body]
//! ```
//!
//! Constraints observed on the live 1.06 / 1.07 `characterinfo.pabgb`
//! (see `docs/save-editor-keys-plan.md` §6):
//!
//! - **Strict hi-byte**: `(key >> 24) == 0`. Row keys top out around
//!   1.7M. The cat-byte is a save-side variant only — gamedata rows are
//!   stored with the bare 24-bit ID. Exception: 359 of the 7,315 indexed
//!   rows in 2.04 (the same 359 as 2.03) have a hashed 32-bit key
//!   (`NHM_Noble_1` = 0x77395fed, …), which this scanner does not see.
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
//! ## FactionKey (2026-11-27)
//!
//! Every row carries the character's `FactionKey` (the `faction` table's key,
//! 0 for none) as a `u32` at a fixed position in the fixed-width head of the
//! body, counted from the first byte after the key:
//!
//! - 2.04 through 2.05.01: byte 149 ([`FACTION_AT_PRE_2_06`]).
//! - 2.06: byte 151 of the row after `[key][u32 row index]`
//!   ([`FACTION_AT_2_06`]). 2.06 inserted two zero bytes at byte 95 of every
//!   row (7,313 of 7,315 rows are exactly that), which moved it by two.
//!
//! Pinned on all 7,315 rows of 2.04, 2.04.08, 2.05, 2.05.01 and 2.06: the value
//! is a `faction` key or 0 on every row, the same per character across all five
//! versions, and 2,303 rows are non-zero. Every `NHM_Hernand_Soldiers_*` row is
//! 1000131 (`Faction_Celeste_Hernand`), Kliff (1), `Kliff_Clone` and Damian (4)
//! are 1000000 (`Faction_Graymane`), `NHM_Bandit_*` is 1000128
//! (`Faction_Bandit`). The save does not record an NPC's faction; this table is
//! the only source.
//!
//! ## Other 2.06 changes in the row (not parsed here)
//!
//! - The two zero bytes moving `FactionKey`: inserted somewhere in bytes
//!   87–91 after the key, an all-zero run on every row, so the byte diff cannot
//!   place them exactly (bytes 85–86 match both versions; `Riding_Big_pirateship_1`,
//!   the one row with a non-zero `u32` at 91, shows it moved too).
//! - **Inlined conditions.** A count-prefixed list whose 13-byte elements were
//!   `[u32 ConditionInfoKey A][u32 ConditionInfoKey B][u8 op][u32 hash]` now
//!   stores each anonymous condition in place as `01` + its compiled bytecode
//!   (`NGW_Citizen_13083`: `0x0F4293 CheckFactionKey(Faction_Graymane) && …` →
//!   `01 01 01 03 27 01 40 42 0f 00 …`). Named conditions (`BattleState`,
//!   `Faction_FlagOn`, …) are still referenced by key. This is the patch-wide
//!   change described under "Inlined conditions (2.06)" in `scripts/CLAUDE.md`;
//!   it adds 39–301 bytes to 2,500+ rows (+1.43 MB in all).
//! - 971 rows lost four zero bytes later in the row (860 of them have no other
//!   change; Kliff's sit just before `31 00 00 00 80 96 98 00`). Not explained.
//!
//! On 2.06 the header lists every row, so [`parse_character_info_lossy_with_header`]
//! reads the rows from it instead of anchor-scanning, which also returns the
//! 359 rows with a hashed 32-bit key.
//!
//! See `docs/save-editor-keys-plan.md` §6 for the full investigation
//! (verified mappings: `0x07000002 → "Yann"`, `0x06000f4a → "Pierre"`,
//! `0x0a000001 → "Kliff"`, `0x09000f4d → "Noble"`).

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
    /// Used as the fallback when the PALOC chain misses (the 78% case
    /// where no localized display string exists).
    pub name: String,
    /// The character's `FactionKey` (a `faction` row key), 0 when it has none
    /// or the row is too short to hold the field.
    pub faction_key: u32,
}

/// Where `FactionKey` sits on 2.04 through 2.05.01: bytes after the row key.
pub const FACTION_AT_PRE_2_06: usize = 149;
/// Where `FactionKey` sits on 2.06: bytes after the row's `[key][u32 index]`.
pub const FACTION_AT_2_06: usize = 151;

fn read_faction(rest: &[u8], at: usize) -> u32 {
    rest.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .unwrap_or(0)
}

/// 2.06+: row names live in the `.staticinfoheader`, so the body alone no
/// longer carries them. With a 2.06 header every row comes from it, in header
/// order (hashed 32-bit keys included); otherwise `data` is anchor-scanned as a
/// pre-2.06 body.
pub fn parse_character_info_lossy_with_header(data: &[u8], header: &[u8]) -> Vec<CharacterInfoEntry> {
    use crate::binary::static_header::{detect_named_header, read_body_row};
    let Some((_, key_width, rows)) = detect_named_header(header, data.len()) else {
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
/// `from_bytes` does first-wins HashMap insertion so a legitimate
/// duplicate key (none observed in 1.06 / 1.07) would not replace the
/// earlier entry.
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
/// followed by a small number still pass (154 on 2.04, none on a real
/// row's key), so the bridge's first-wins dedup stays necessary.
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
            // Key rule and all-digit rejection as documented on the module.
            // Reject all-digit names — those are stringinfo numeric refs
            // ASCII-serialised in row bodies, not real anchors. Names can start
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
    //! `characterinfo.pabgb`. Pins the four mappings verified during
    //! the 2026-05-14 investigation (`docs/save-editor-keys-plan.md`
    //! §6) so a future patch breaking the row schema is caught
    //! immediately. Skips cleanly when no game install is present.

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

    /// (CharacterKey, FactionKey) on 2.06 (unchanged since 2.04).
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
        // FactionKey, read from the 2.06 rows (see the module docs).
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
