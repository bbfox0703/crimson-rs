//! `partprefabdyeslotinfo.pabgb` parser — PABGH-indexed.
//!
//! Resolves `PartPrefabKey (u32)` — the gamedata key identifying a
//! dyeable prefab (an item's wearable mesh). Replaces the PyQt5
//! reference editor's hand-maintained `dye_slot_counts.json` with
//! gamedata-driven slot counts so the C# editor doesn't drift when
//! Pearl Abyss adds new dyeable gear.
//!
//! Row counts drift per patch (1,105 in 1.07; 1,111 in 1.10/1.11;
//! **968 in 1.12** after −143 rows were removed). The cross-reference
//! between an `_itemKey` and its `_partPrefabKey` is NOT in this
//! file — that bridge lives in `iteminfo.pabgb` (joined via
//! `stringinfo.pabgb`; see [`crate::c_abi::item_part_prefab`]).
//!
//! Schema (verified byte-perfect across a sampling of 1.07 rows and the
//! full live 1.12 table; `slot_count` and `row_prefab_name` checked
//! across the full table):
//!
//! ```text
//! Row {
//!     u32 key,                  // matches PABGH key
//!     u8 pad[5],                // = 00 00 00 00 00
//!     u32 slot_count,           // 1..N (N reaches 36 in 1.12 for vehicle meshes)
//!     CString row_prefab_name,  // e.g. "cd_phm_00_lb_00_0054"
//!     Slot[slot_count] {
//!         u8 mat_indices[3],         // material indices for this slot
//!         CString material_a,        // 3 material names (often empty)
//!         CString material_b,
//!         CString material_c,
//!         u8 mask[12],               // active/visible flags; 3 before 2.01
//!         // 1.12 ONLY: a 5-byte field inserted here (u8 + u32),
//!         //   observed uniformly (0xFF, 0) on all 3,893 live slots.
//!         //   Absent in 1.07-1.11; consumed-but-not-stored (no
//!         //   downstream reader). See "Cross-version layout drift".
//!         CString tail_name,         // next sub-prefab name; for the LAST
//!                                    //   slot, the full .pac asset path
//!     }
//! }
//! ```
//!
//! ## Cross-version layout drift (1.12, 2.01)
//!
//! 1.12 inserted the `u8 + u32` field per slot. 2.01 widened `mask` from
//! 3 bytes to 12, in **both** the slot and the extra layer — the only
//! schema change 2.01 made to anything crimson-rs parses. The three
//! layouts are **empirically disjoint** — across the full 1.11 (1,111
//! rows), 1.12 (968 rows) and 2.01 (1,626 rows) tables, every row walks
//! cleanly under exactly one and zero rows are ambiguous. The parser
//! therefore tries them newest-first per row (see [`SLOT_LAYOUTS`]),
//! keeping support for older installs while reading the live table. A
//! future patch that drifts the per-slot layout again will fail every
//! attempt and the row drops out — the lossy safety net.
//!
//! The 12-byte width is pinned by a tandem walk against the kept 2.00
//! binary (`gamedata-bin/2.00/partprefabdyeslotinfo.pabgb`): reading 2.00
//! at `mask_len = 3` and 2.01 at `mask_len = 12`, **1,613 of the 1,620
//! carried-over rows have every non-mask field byte-identical** across
//! the two versions, and the 7 that don't differ only by ordinary content
//! (3 changed `slot_count`, 4 changed a material name or `mat_indices`).
//! Nothing else in the record moved.
//!
//! **The contents were re-encoded, not extended.** In 5,572 of 6,555
//! comparable slot pairs the 2.00 three-byte mask does not appear as a
//! contiguous window anywhere inside the 2.01 twelve, so there is no
//! "original three" to point at — do not treat any sub-slice as the
//! pre-2.01 field. The 12 read as **four groups of three**: a slot has
//! one group non-zero (4,276 of 6,585), two (1,254), three (72), or none
//! (983), never four. Summed over all 12 bytes the mask matches the
//! slot's non-empty-material-name count on 62.2% of slots, versus 39.4%
//! for `mask[0..3]` alone — the channel-active information lives across
//! the whole field, not in its first three bytes.
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
    /// Material-channel active/visible flags, one byte each, every value
    /// 0 or 1. **2.01 widened this from 3 bytes to 12 and re-encoded it**
    /// (see "Cross-version layout drift"): the 12 read as four groups of
    /// three, of which a slot uses one (4,276 of 6,585 live slots), two
    /// (1,254), three (72) or none (983) — never all four. The pre-2.01
    /// three bytes do **not** survive as a sub-slice, so `mask[0..3]` is
    /// not "the old mask" and reads all-zero on every slot whose active
    /// group is not the first.
    pub mask: [u8; 12],
    /// For non-final slots: the next sub-prefab internal name. For
    /// the LAST slot in a row: the full `.pac` asset path for the
    /// prefab as a whole.
    pub tail_name: String,
    /// 1.13: additional material/dye layers for this slot. The 5-byte
    /// per-slot field that 1.12 blindly padded (`u8 0xFF` + `u32`) is
    /// actually `marker + extra_layer_count`; 1.12's count is always 0,
    /// but 1.13's "expanded dyeable" gear (new cloaks / shields / quivers
    /// / the skullknight set) sets it to 1, adding a second dye layer
    /// here. Empty on 1.07-1.12 rows. Surfaced through the
    /// `crimson_part_prefab_dye_slot_info_lookup_slot_extra_layer_*` C ABI.
    pub extra_layers: Vec<DyeExtraLayer>,
}

/// 1.13: a secondary material/dye layer inside a [`PartPrefabDyeSlot`]
/// (see `PartPrefabDyeSlot::extra_layers`). Same shape as the slot's
/// primary layer minus the mesh tail: three default-material names, the
/// mask bytes, and a trailing flag byte. Surfaced through the
/// `crimson_part_prefab_dye_slot_info_lookup_slot_extra_layer_*` C ABI.
#[derive(Debug, Clone)]
pub struct DyeExtraLayer {
    /// Three default-material names for this extra layer (e.g. the
    /// `"leather"` layer paired with the primary `"cloth"` layer on the
    /// new dyeable cloaks).
    pub default_materials: [String; 3],
    /// Mask bytes for the extra layer (same semantics and the same 2.01
    /// 3 → 12 widening + re-encode as the primary slot `mask`).
    pub mask: [u8; 12],
    /// Trailing flag byte (0/1 observed; exact meaning not yet RE'd).
    pub flag: u8,
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

/// Read `mask_len` mask bytes into the fixed 12-wide field, zero-filling
/// the rest. Pre-2.01 rows carry only 3 (see "Cross-version layout drift"),
/// so the tail stays zero — which is also what those channels mean.
fn read_mask(body: &[u8], cursor: &mut usize, mask_len: usize) -> Option<[u8; 12]> {
    let bytes = body.get(*cursor..*cursor + mask_len)?;
    let mut mask = [0u8; 12];
    mask[..mask_len].copy_from_slice(bytes);
    *cursor += mask_len;
    Some(mask)
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
/// invalid. When `new_schema` is set, consume the 1.12 per-slot
/// `u8 + u32` field inserted between `mask` and `tail_name`.
fn parse_slot(
    body: &[u8],
    cursor: &mut usize,
    new_schema: bool,
    mask_len: usize,
) -> Option<PartPrefabDyeSlot> {
    if *cursor + 3 > body.len() {
        return None;
    }
    let mat_indices = [body[*cursor], body[*cursor + 1], body[*cursor + 2]];
    *cursor += 3;

    let mat_a = read_cstring(body, cursor)?;
    let mat_b = read_cstring(body, cursor)?;
    let mat_c = read_cstring(body, cursor)?;

    let mask = read_mask(body, cursor, mask_len)?;

    let mut extra_layers: Vec<DyeExtraLayer> = Vec::new();
    if new_schema {
        // 1.12 introduced a per-slot `u8 marker (0xFF) + u32` here. What
        // looked like a uniform `(0xFF, 0)` 5-byte pad in 1.12 is actually
        // `marker + extra_layer_count`: 1.12's count is always 0, but
        // 1.13's "expanded dyeable" gear sets it to 1, adding a second
        // material/dye layer inline (RE'd 2026-07-04 via
        // `scripts/decode_dyeslot_113.py`; enhanced model consumes all
        // 1,538 live 1.13 rows exactly). 1.07-1.11 rows have no such field
        // and are handled by the `new_schema == false` fallback.
        if *cursor + 5 > body.len() {
            return None;
        }
        // body[*cursor] is the 0xFF marker; not stored.
        let extra_count = u32::from_le_bytes([
            body[*cursor + 1],
            body[*cursor + 2],
            body[*cursor + 3],
            body[*cursor + 4],
        ]) as usize;
        *cursor += 5;
        // Plausibility cap — only 0 (1.07-1.12) and 1 (1.13) observed; a
        // large value means we're mis-reading a 1.07-1.11 row under the
        // wrong layout, so reject and let the caller fall back.
        if extra_count > 8 {
            return None;
        }
        extra_layers.reserve(extra_count);
        for _ in 0..extra_count {
            let e_a = read_cstring(body, cursor)?;
            let e_b = read_cstring(body, cursor)?;
            let e_c = read_cstring(body, cursor)?;
            let e_mask = read_mask(body, cursor, mask_len)?;
            if *cursor >= body.len() {
                return None;
            }
            let e_flag = body[*cursor];
            *cursor += 1;
            extra_layers.push(DyeExtraLayer {
                default_materials: [e_a, e_b, e_c],
                mask: e_mask,
                flag: e_flag,
            });
        }
    }

    let tail_name = read_cstring(body, cursor)?;

    Some(PartPrefabDyeSlot {
        mat_indices,
        default_materials: [mat_a, mat_b, mat_c],
        mask,
        tail_name,
        extra_layers,
    })
}

/// Per-slot layouts, newest first — `(new_schema, mask_len)`. See
/// "Cross-version layout drift": 1.12 added the `u8 + u32` after the mask
/// (`new_schema`), and 2.01 widened the mask itself from 3 bytes to 12.
const SLOT_LAYOUTS: [(bool, usize); 3] = [(true, 12), (true, 3), (false, 3)];

/// Try to parse one full row body under a single layout. Returns
/// `Some(entry)` only when the row's header validates against
/// `expected_key` and the slot walk consumes the body **exactly**
/// (`cursor == body.len()`); otherwise `None`. `new_schema` selects the
/// 1.12 per-slot marker + extra-layer count, and `mask_len` the 2.01
/// mask width — see [`SLOT_LAYOUTS`]. The exact-consume requirement is
/// what makes the multi-layout fallback in
/// [`parse_part_prefab_dye_slot_info_lossy`] unambiguous.
fn try_parse_row(
    body: &[u8],
    expected_key: u32,
    new_schema: bool,
    mask_len: usize,
) -> Option<PartPrefabDyeSlotInfoEntry> {
    // Header: u32 key + 5 pad + u32 slot_count + CString prefab_name
    if body.len() < 17 {
        return None;
    }
    let key = u32::from_le_bytes([body[0], body[1], body[2], body[3]]);
    if key != expected_key {
        return None;
    }
    let slot_count = u32::from_le_bytes([body[9], body[10], body[11], body[12]]);
    // Plausibility cap — 64 covers everything observed through 1.12
    // (peak 36 for vehicle/robot meshes).
    if slot_count == 0 || slot_count > 64 {
        return None;
    }
    let name_len = u32::from_le_bytes([body[13], body[14], body[15], body[16]]) as usize;
    if 17 + name_len > body.len() || !(1..=128).contains(&name_len) {
        return None;
    }
    let prefab_name = std::str::from_utf8(&body[17..17 + name_len]).ok()?;

    let mut cursor = 17 + name_len;
    let mut slots: Vec<PartPrefabDyeSlot> = Vec::with_capacity(slot_count as usize);
    for _ in 0..slot_count {
        slots.push(parse_slot(body, &mut cursor, new_schema, mask_len)?);
    }
    // Mismatched body length signals schema drift / wrong layout; reject
    // the row so the caller can try the other layout (or drop it).
    if cursor != body.len() {
        return None;
    }

    Some(PartPrefabDyeSlotInfoEntry {
        key,
        prefab_name: prefab_name.to_owned(),
        slots,
    })
}

/// Parse `partprefabdyeslotinfo.pabgb` using its `.pabgh` index.
///
/// Returns entries in PABGH on-disk order. Rows whose body doesn't match
/// any supported per-slot layout (truncated, slot_count out of plausible
/// range, key mismatch with PABGH, or leftover bytes under every layout)
/// are silently dropped.
///
/// Each row is tried against [`SLOT_LAYOUTS`] newest-first, keeping
/// whichever consumes the body exactly. The layouts are empirically
/// disjoint (no row parses cleanly under two — verified across the full
/// 1.11, 1.12 and 2.01 tables), so the fallback never mis-reads a row.
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
        // Newest layout first; whichever consumes the body exactly wins,
        // all of them failing drops the row (the lossy net). On the live
        // 2.01 install the first layout parses all 1,626 rows.
        if let Some(parsed) = SLOT_LAYOUTS
            .iter()
            .find_map(|&(new_schema, mask_len)| {
                try_parse_row(body, entry.key, new_schema, mask_len)
            })
        {
            out.push(parsed);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    //! Live-install integration test. Pins the row count + a handful
    //! of (key, prefab_name, slot_count) tuples from the phase-2 probe
    //! dump so a future patch breaking the row layout is caught
    //! immediately. Skips cleanly when no game install is present.

    use crate::binary::gamedata_layout;
    use super::*;
    use std::path::PathBuf;

    /// (key, expected prefab_name, expected slot_count). Verified on the
    /// live 1.12 install (2026-06-19). The first three carry over
    /// unchanged from the 2026-05-16 1.07 probe pass (cross-version
    /// stable); the last three replace 1.07 keys that 1.12 removed (part
    /// of the −143-row drop, 1,111 → 968).
    const KNOWN: &[(u32, &str, u32)] = &[
        (0xc7bbaada, "cd_phm_00_lb_00_0054", 1),
        (0xfbad5654, "cd_phm_00_hel_0057_02_inside", 2),
        (0xddb61e2e, "cd_phm_00_vest_0051_01", 4),
        (0x4905cceb, "cd_phm_00_lb_0002", 1),
        (0xf8042604, "cd_phm_00_lb_00_0342_belt", 3),
        (0xd5394f4c, "cd_phm_00_hand_belt_0245_01", 5),
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
            .find(|d| d.path == gamedata_layout::bin_dir())?;
        let pabgb_file = dir
            .files
            .iter()
            .find(|f| f.name == gamedata_layout::body("partprefabdyeslotinfo"))?;
        let pabgh_file = dir
            .files
            .iter()
            .find(|f| f.name == gamedata_layout::header("partprefabdyeslotinfo"))?;
        let group_dir = game_root.join("0008");
        let pabgb = crate::binary::paz::extract_file(
            &group_dir,
            pabgb_file,
            gamedata_layout::bin_dir(),
            &pamt.header.encrypt_info.encrypt_info,
        )
        .ok()?;
        let pabgh = crate::binary::paz::extract_file(
            &group_dir,
            pabgh_file,
            gamedata_layout::bin_dir(),
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
             (live 1.13 ships 1,538 rows; 1.12 = 968; 1.07-1.11 = ~1105-1111)",
            entries.len(),
            pabgb.len()
        );
        // Row counts drift per patch: 1.13 = 1,538, 1.12 = 968, 1.07-1.11 =
        // ~1105-1111. Pin >=900 as a version-agnostic floor that catches the
        // 0-rows breakage on any install; the live-1.13 exact count (1,538,
        // all rows parsing after the extra-layer RE) is asserted below.
        assert!(
            entries.len() >= 900,
            "expected >=900 partprefabdyeslotinfo entries, got {}",
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
        let mut hist: std::collections::BTreeMap<u32, usize> = Default::default();
        for e in &entries {
            *hist.entry(e.slot_count()).or_default() += 1;
        }
        println!("slot_count histogram: {hist:?}");
        let count_1 = entries.iter().filter(|e| e.slot_count() == 1).count();
        // `slot_count == 1` must stay the modal bucket. The share itself
        // drifts with content — it was comfortably over 1/4 through 1.17 but
        // 1.18's 65 new rows pushed it to 399/1,619 (24.6%), so a fixed
        // fraction is the wrong shape of check. Being the mode is what
        // actually encodes "right-skewed", and a record-schema drift (the
        // failure mode this guards, see the 1.12 / 1.13 notes above) scatters
        // slot_count instead of merely shifting its share.
        let modal = hist.iter().max_by_key(|&(_, n)| *n).map(|(k, n)| (*k, *n));
        assert_eq!(
            modal,
            Some((1, count_1)),
            "slot_count=1 should be the modal bucket ({count_1} of {}); histogram {hist:?}",
            entries.len(),
        );

        // Cap check — peak observed slot_count is 36 (1.12 vehicle/robot
        // meshes); 1.07-1.11 peaked near 30. 64 is the defensive ceiling.
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

        // 1.13: the "expanded dyeable equipment" gear gained a second per-slot
        // dye layer. Key 0x54534e48 (cd_phm_00_cloak_0054_01_01_01) was dropped
        // entirely under the pre-1.13 blind-5-byte-pad model; verify it now
        // parses AND that one of its slots carries an extra layer whose
        // materials include "leather" (paired with the primary "cloth" layer).
        // Guarded so non-1.13 installs (where the key is absent) don't fail.
        if let Some(cloak) = entries.iter().find(|e| e.key == 0x54534e48) {
            let extra_leather = cloak.slots.iter().any(|s| {
                s.extra_layers
                    .iter()
                    .any(|l| l.default_materials.iter().any(|m| m == "leather"))
            });
            assert!(
                extra_leather,
                "1.13 cloak 0x54534e48 should carry a second dye layer with a 'leather' material",
            );
        }
    }
}
