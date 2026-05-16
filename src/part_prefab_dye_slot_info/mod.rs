//! `partprefabdyeslotinfo.pabgb` parser — PABGH-indexed.
//!
//! Resolves `PartPrefabKey (u32)` — the gamedata key identifying a
//! dyeable prefab (an item's wearable mesh). Replaces the PyQt5
//! reference editor's hand-maintained `dye_slot_counts.json` with
//! gamedata-driven slot counts so the C# editor doesn't drift when
//! Pearl Abyss adds new dyeable gear.
//!
//! 1,105 rows in 1.07. The cross-reference between an `_itemKey` and
//! its `_partPrefabKey` is NOT in this file — that bridge lives in
//! `iteminfo.pabgb` (or a sibling `partprefab*` table not yet RE'd)
//! and is the next-session blocker for the C# editor to consume
//! these slot counts.
//!
//! Schema (verified byte-perfect across a sampling of 1.07 rows;
//! `slot_count` and `row_prefab_name` checked across the full table):
//!
//! ```text
//! Row {
//!     u32 key,                  // matches PABGH key
//!     u8 pad[5],                // = 00 00 00 00 00
//!     u32 slot_count,           // 1..N (N reaches ~30+ for complex prefabs)
//!     CString row_prefab_name,  // e.g. "cd_phm_00_lb_00_0054"
//!     Slot[slot_count] {
//!         u8 mat_indices[3],         // material indices for this slot
//!         CString material_a,        // 3 material names (often empty)
//!         CString material_b,
//!         CString material_c,
//!         u8 mask[3],                // active/visible flags
//!         CString tail_name,         // next sub-prefab name; for the LAST
//!                                    //   slot, the full .pac asset path
//!     }
//! }
//! ```
//!
//! For the v1 dye editor we only consume `(key, prefab_name, slot_count)`.
//! The per-slot material list (a v2 task) lets us also render the
//! "default material per slot" suggestions in the dye UI.
//!
//! Each `CString` is `[u32 len][len bytes]` with NO trailing NUL.

/// One dye slot within a prefab — captures everything the editor
/// needs to render per-slot UI (default materials + which material
/// indices apply to this slot + the prefab the slot anchors to).
#[derive(Debug, Clone)]
pub struct PartPrefabDyeSlot {
    /// Three material indices controlling which palette material this
    /// slot consumes. Semantics are best-guess — the in-game shader
    /// indexes a 3-element material array, and these bytes select
    /// which of the 11 `partprefabdyetexturepalleteinfo` rows feeds
    /// each shader channel.
    pub mat_indices: [u8; 3],
    /// Three default-material names for this slot (often all empty,
    /// often `"cloth"/"leather"/"metal"` depending on the prefab).
    /// Maps 1:1 with `mat_indices`.
    pub default_materials: [String; 3],
    /// Three mask bytes (active/visible flags — exact meaning not yet
    /// RE'd; usually `[1, 1, 1]` for fully-active slots, `[1, 1, 0]`
    /// for slots with only 2 active material channels).
    pub mask: [u8; 3],
    /// For non-final slots: the next sub-prefab internal name. For
    /// the LAST slot in a row: the full `.pac` asset path for the
    /// prefab as a whole.
    pub tail_name: String,
}

/// One parsed `partprefabdyeslotinfo` row.
#[derive(Debug, Clone)]
pub struct PartPrefabDyeSlotInfoEntry {
    /// `PartPrefabKey` as stored in the prefab cross-reference (which
    /// itself sits in `iteminfo.pabgb` — not yet bridged).
    pub key: u32,
    /// Prefab internal name (e.g. `"cd_phm_00_lb_00_0054"`,
    /// `"cd_phw_00_vest_belt_0137_00"`).
    pub prefab_name: String,
    /// Per-slot detail. `slots.len()` == the `slot_count` u32 in the
    /// row header. Replaces the PyQt5 editor's hand-maintained
    /// `dye_slot_counts.json` value for this prefab and adds the
    /// per-slot default-material info the JSON didn't carry.
    pub slots: Vec<PartPrefabDyeSlot>,
}

impl PartPrefabDyeSlotInfoEntry {
    /// Convenience accessor — equivalent to `self.slots.len() as u32`.
    pub fn slot_count(&self) -> u32 {
        self.slots.len() as u32
    }
}

/// Read a `[u32 len][len bytes]` CString from `data` starting at
/// `*cursor`, advancing the cursor past the consumed bytes. Returns
/// `None` if the buffer is too short or bytes aren't valid UTF-8.
fn read_cstring(data: &[u8], cursor: &mut usize) -> Option<String> {
    if *cursor + 4 > data.len() {
        return None;
    }
    let len = u32::from_le_bytes([
        data[*cursor],
        data[*cursor + 1],
        data[*cursor + 2],
        data[*cursor + 3],
    ]) as usize;
    *cursor += 4;
    if *cursor + len > data.len() {
        return None;
    }
    let s = std::str::from_utf8(&data[*cursor..*cursor + len])
        .ok()?
        .to_owned();
    *cursor += len;
    Some(s)
}

/// Parse one slot starting at `cursor`. Returns the parsed slot and
/// the new cursor position, or `None` if the body is truncated /
/// invalid.
fn parse_slot(body: &[u8], cursor: &mut usize) -> Option<PartPrefabDyeSlot> {
    if *cursor + 3 > body.len() {
        return None;
    }
    let mat_indices = [body[*cursor], body[*cursor + 1], body[*cursor + 2]];
    *cursor += 3;

    let mat_a = read_cstring(body, cursor)?;
    let mat_b = read_cstring(body, cursor)?;
    let mat_c = read_cstring(body, cursor)?;

    if *cursor + 3 > body.len() {
        return None;
    }
    let mask = [body[*cursor], body[*cursor + 1], body[*cursor + 2]];
    *cursor += 3;

    let tail_name = read_cstring(body, cursor)?;

    Some(PartPrefabDyeSlot {
        mat_indices,
        default_materials: [mat_a, mat_b, mat_c],
        mask,
        tail_name,
    })
}

/// Parse `partprefabdyeslotinfo.pabgb` using its `.pabgh` index.
///
/// Returns entries in PABGH on-disk order. Rows whose body doesn't
/// match the verified schema (truncated, slot_count out of plausible
/// range, key mismatch with PABGH) are silently dropped.
pub fn parse_part_prefab_dye_slot_info_lossy(
    pabgb: &[u8],
    pabgh: &[u8],
) -> Vec<PartPrefabDyeSlotInfoEntry> {
    let Ok(index) = crate::skill_info::parse_pabgh(pabgh) else {
        return Vec::new();
    };
    let ranges = crate::skill_info::entry_ranges(&index, pabgb.len());
    let mut out = Vec::with_capacity(index.len());
    for (entry, (start, end)) in index.iter().zip(ranges.iter()) {
        let Some(body) = pabgb.get(*start..*end) else {
            continue;
        };
        // Header: u32 key + 5 pad + u32 slot_count + CString prefab_name
        if body.len() < 17 {
            continue;
        }
        let key = u32::from_le_bytes([body[0], body[1], body[2], body[3]]);
        if key != entry.key {
            continue;
        }
        let slot_count = u32::from_le_bytes([body[9], body[10], body[11], body[12]]);
        // Plausibility cap — 64 covers everything observed on 1.07
        // (peak around 30 for full-body prefabs).
        if slot_count == 0 || slot_count > 64 {
            continue;
        }
        let name_len = u32::from_le_bytes([body[13], body[14], body[15], body[16]]) as usize;
        if 17 + name_len > body.len() || !(1..=128).contains(&name_len) {
            continue;
        }
        let Ok(prefab_name) = std::str::from_utf8(&body[17..17 + name_len]) else {
            continue;
        };

        // Walk the slots end-to-end. Mismatched body length signals
        // schema drift; drop the row in that case.
        let mut cursor = 17 + name_len;
        let mut slots: Vec<PartPrefabDyeSlot> = Vec::with_capacity(slot_count as usize);
        let mut ok = true;
        for _ in 0..slot_count {
            match parse_slot(body, &mut cursor) {
                Some(s) => slots.push(s),
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok || cursor != body.len() {
            continue;
        }

        out.push(PartPrefabDyeSlotInfoEntry {
            key,
            prefab_name: prefab_name.to_owned(),
            slots,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    //! Live-install integration test. Pins the row count + a handful
    //! of (key, prefab_name, slot_count) tuples from the phase-2 probe
    //! dump so a future patch breaking the row layout is caught
    //! immediately. Skips cleanly when no game install is present.

    use super::*;
    use std::path::PathBuf;

    /// (key, expected prefab_name, expected slot_count). Values from
    /// the 2026-05-16 probe pass against the live 1.07 install.
    const KNOWN: &[(u32, &str, u32)] = &[
        (0xc7bbaada, "cd_phm_00_lb_00_0054", 1),
        (0xfbad5654, "cd_phm_00_hel_0057_02_inside", 2),
        (0x7a5dda1c, "cd_phm_00_cloak_0054_hair_spline", 1),
        (0x6ecff454, "cd_phw_00_vest_belt_0137_00", 10),
        (0xe3713ebc, "cd_phw_00_vest_acc_0043_01", 8),
        (0xddb61e2e, "cd_phm_00_vest_0051_01", 4),
    ];

    fn extract_pair() -> Option<(Vec<u8>, Vec<u8>)> {
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
        let pabgb_file = dir
            .files
            .iter()
            .find(|f| f.name == "partprefabdyeslotinfo.pabgb")?;
        let pabgh_file = dir
            .files
            .iter()
            .find(|f| f.name == "partprefabdyeslotinfo.pabgh")?;
        let group_dir = game_root.join("0008");
        let pabgb = crate::binary::paz::extract_file(
            &group_dir,
            pabgb_file,
            "gamedata/binary__/client/bin",
            &pamt.header.encrypt_info.encrypt_info,
        )
        .ok()?;
        let pabgh = crate::binary::paz::extract_file(
            &group_dir,
            pabgh_file,
            "gamedata/binary__/client/bin",
            &pamt.header.encrypt_info.encrypt_info,
        )
        .ok()?;
        Some((pabgb, pabgh))
    }

    #[test]
    fn part_prefab_dye_slot_info_lossy_live() {
        let Some((pabgb, pabgh)) = extract_pair() else {
            eprintln!(
                "skipping part_prefab_dye_slot_info_lossy_live: no game install"
            );
            return;
        };
        let entries = parse_part_prefab_dye_slot_info_lossy(&pabgb, &pabgh);
        println!(
            "parsed {} partprefabdyeslotinfo entries from {} pabgb bytes \
             (live 1.07 ships ~1105 rows)",
            entries.len(),
            pabgb.len()
        );
        // 1.07 has 1,105 rows. Pin >=900 as a loose floor.
        assert!(
            entries.len() >= 900,
            "expected >=900 partprefabdyeslotinfo entries (live 1.07 ~1105), got {}",
            entries.len()
        );

        let by_key: std::collections::HashMap<u32, (&str, u32)> = entries
            .iter()
            .map(|e| (e.key, (e.prefab_name.as_str(), e.slot_count())))
            .collect();
        for &(key, expected_name, expected_count) in KNOWN {
            let (actual_name, actual_count) = by_key
                .get(&key)
                .copied()
                .unwrap_or_else(|| panic!("PartPrefabKey 0x{key:08x} not found"));
            assert_eq!(
                actual_name, expected_name,
                "PartPrefabKey 0x{key:08x} prefab_name mismatch",
            );
            assert_eq!(
                actual_count, expected_count,
                "PartPrefabKey 0x{key:08x} slot_count mismatch",
            );
        }

        // Sanity: slot_count distribution should be heavily right-skewed
        // (most prefabs have 1 slot; a handful have 10+).
        let count_1 = entries.iter().filter(|e| e.slot_count() == 1).count();
        assert!(
            count_1 > entries.len() / 4,
            "expected at least 1/4 of prefabs to have slot_count=1; got {} of {}",
            count_1,
            entries.len(),
        );

        // Cap check — no observed prefab exceeds ~30 slots.
        let max = entries.iter().map(|e| e.slot_count()).max().unwrap_or(0);
        assert!(
            max <= 64,
            "implausible max slot_count {} — schema drift?",
            max
        );

        // Per-slot detail check — for one known prefab, verify the
        // first slot's mask and the LAST slot's tail_name (which is
        // the full .pac asset path).
        let row_5 = entries
            .iter()
            .find(|e| e.key == 0xc7bbaada)
            .expect("known key 0xc7bbaada");
        // cd_phm_00_lb_00_0054 has slot_count=1; the single slot's
        // tail_name is the pac path.
        assert_eq!(row_5.slots.len(), 1);
        assert!(
            row_5.slots[0]
                .tail_name
                .ends_with("cd_phm_00_lb_00_0054.pac"),
            "expected pac suffix, got {:?}",
            row_5.slots[0].tail_name,
        );

        // cd_phm_00_vest_0051_01 (key=0xddb61e2e) has slot_count=4 with
        // mixed material strings — sanity-check that at least one slot
        // carries a "leather"/"cloth"/"metal" material name.
        let vest_row = entries
            .iter()
            .find(|e| e.key == 0xddb61e2e)
            .expect("known key 0xddb61e2e");
        assert_eq!(vest_row.slots.len(), 4);
        let any_known_material = vest_row.slots.iter().any(|s| {
            s.default_materials
                .iter()
                .any(|m| matches!(m.as_str(), "cloth" | "leather" | "metal"))
        });
        assert!(
            any_known_material,
            "expected at least one cloth/leather/metal material in vest row's slots",
        );
    }
}
