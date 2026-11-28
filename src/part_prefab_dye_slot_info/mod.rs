//! `partprefabdyeslotinfo.pabgb` parser — PABGH-indexed.
//!
//! Resolves `PartPrefabKey (u32)` — the gamedata key identifying a
//! dyeable prefab (an item's wearable mesh). Gives gamedata-driven slot
//! counts in place of a hand-maintained list, so the C# editor doesn't
//! drift when Pearl Abyss adds new dyeable gear.
//!
//! Row counts drift per patch. The cross-reference
//! between an `_itemKey` and its `_partPrefabKey` is NOT in this
//! file: an item reaches its rows through its `.prefab` meshes, and the
//! row key is the checksum of the mesh's lower-case `.pac` path (see
//! [`crate::c_abi::item_part_prefab`]).
//!
//! Schema (pre-2.06 body; 2.06 rows are rebuilt into it by
//! `static_header::legacy_pair`):
//!
//! ```text
//! Row {
//!     CString row_name,         // may be empty (it was through 2.04.08)
//!     u32 key,                  // matches PABGH key
//!     u8 pad,                   // = 00
//!     u32 slot_count,           // 1..N (vehicle meshes have the most)
//!     CString row_prefab_name,  // e.g. "cd_phm_00_lb_00_0054"
//!     Slot[slot_count] {
//!         u8 mat_indices[3],         // dye slot number per material channel
//!                                    //   (0..12, 0xFF = unused)
//!         CString material_a,        // 3 material names (often empty)
//!         CString material_b,
//!         CString material_c,
//!         u8 mask[12],               // one flag per dye slot number
//!         u8 marker,                 // 0xFF, not stored
//!         u32 extra_layer_count,     // see `PartPrefabDyeSlot::extra_layers`
//!         ExtraLayer[extra_layer_count],
//!         CString tail_name,         // next sub-prefab name; for the LAST
//!                                    //   slot, the full .pac asset path
//!     }
//! }
//! ```
//!
//! ## Mask
//!
//! The parser reads the current layout only, and requires a row to walk
//! its body exactly. A patch that drifts the per-slot layout fails that
//! and the row drops out — the lossy safety net. Layout history is in
//! `docs/validated-patches.md`, the mask's in `docs/dye-editor-scope.md`.
//!
//! The twelve mask bytes are one flag per **dye slot number**, the value
//! each channel carries in `mat_indices` (0..12, `0xFF` for an unused
//! channel):
//!
//! ```text
//! active(ch) = mat_indices[ch] != 0xFF && mask[mat_indices[ch]] != 0
//! ```
//!
//! Every set bit is one of the slot's `mat_indices`. The dye slot numbers
//! are what the save's `ItemDyeSaveData._dyeSlotNo` stores: a one-slot
//! prefab whose `mat_indices` are 1/2/0 carries three dye entries numbered
//! 0, 1 and 2. The numbers do not form groups. See
//! [`PartPrefabDyeSlot::active_channels`] and
//! [`PartPrefabDyeSlotInfoEntry::dye_slot_bits`].
//!
//! The bridge exposes the whole row: `(key, prefab_name, slot_count)`, the
//! per-slot default materials, `mat_indices`, the mask and its decoded dye
//! slot bits, and the extra layers.
//!
//! Each `CString` is `[u32 len][len bytes]` with NO trailing NUL.

use crate::binary::row_head::read_row_head;

/// One dye slot within a prefab — captures everything the editor
/// needs to render per-slot UI (default materials + which material
/// indices apply to this slot + the prefab the slot anchors to).
#[derive(Debug, Clone)]
pub struct PartPrefabDyeSlot {
    /// Dye slot number (0..12) of each of the slot's three material
    /// channels; `0xFF` ([`NO_DYE_SLOT`]) when the channel is unused. The
    /// numbers are prefab-wide: channels of different slots that share a
    /// number take the same colour, and within a row they share one
    /// default material name. They index [`Self::mask`] and are the values
    /// of the save's `ItemDyeSaveData._dyeSlotNo`.
    pub mat_indices: [u8; 3],
    /// Three default-material names for this slot (often all empty,
    /// often `"cloth"/"leather"/"metal"` depending on the prefab).
    /// Maps 1:1 with `mat_indices`.
    pub default_materials: [String; 3],
    /// Dye-slot flags, one byte (0 or 1) per **dye slot number** 0..12:
    /// `mask[n] == 1` when dye slot `n` is dyeable on this slot. Each
    /// `mat_indices` entry is the dye slot number of a material channel
    /// (0xFF = no channel), so channel `ch` is active when
    /// `mask[mat_indices[ch]] == 1` ([`Self::active_channels`]). These
    /// numbers are the save's `_dyeSlotNo`. The numbers do not form groups.
    pub mask: [u8; 12],
    /// For non-final slots: the next sub-prefab internal name. For
    /// the LAST slot in a row: the full `.pac` asset path for the
    /// prefab as a whole.
    pub tail_name: String,
    /// Additional material/dye layers for this slot, counted by the
    /// per-slot `marker + extra_layer_count` field. Most slots have none;
    /// "expanded dyeable" gear (cloaks, shields, quivers, …) has one, a
    /// second dye layer. Surfaced through the
    /// `crimson_part_prefab_dye_slot_info_lookup_slot_extra_layer_*` C ABI.
    pub extra_layers: Vec<DyeExtraLayer>,
}

/// A secondary material/dye layer inside a [`PartPrefabDyeSlot`]
/// (see `PartPrefabDyeSlot::extra_layers`). Same shape as the slot's
/// primary layer minus the mesh tail: three default-material names, the
/// mask bytes, and a trailing flag byte. Surfaced through the
/// `crimson_part_prefab_dye_slot_info_lookup_slot_extra_layer_*` C ABI.
#[derive(Debug, Clone)]
pub struct DyeExtraLayer {
    /// Three default-material names for this extra layer (e.g. the
    /// `"leather"` layer paired with the primary `"cloth"` layer on the
    /// dyeable cloaks).
    pub default_materials: [String; 3],
    /// Mask bytes for the extra layer, indexed by dye slot number like the
    /// primary slot `mask` (the layer's channels use the owning slot's
    /// `mat_indices`).
    pub mask: [u8; 12],
    /// Trailing flag byte; meaning unknown. Its values are not limited to
    /// 0/1 (and are not 0), so don't validate it as a 0/1 flag.
    pub flag: u8,
}

/// One parsed `partprefabdyeslotinfo` row.
#[derive(Debug, Clone)]
pub struct PartPrefabDyeSlotInfoEntry {
    /// `PartPrefabKey`: the Jenkins checksum of the mesh's `.pac` path in
    /// lower case (the last slot's `tail_name`).
    pub key: u32,
    /// Prefab internal name (e.g. `"cd_phm_00_lb_00_0054"`,
    /// `"cd_phw_00_vest_belt_0137_00"`).
    pub prefab_name: String,
    /// Per-slot detail. `slots.len()` == the `slot_count` u32 in the
    /// row header.
    pub slots: Vec<PartPrefabDyeSlot>,
}

impl PartPrefabDyeSlotInfoEntry {
    /// Convenience accessor — equivalent to `self.slots.len() as u32`.
    pub fn slot_count(&self) -> u32 {
        self.slots.len() as u32
    }

    /// The prefab's dye slots: bit `n` is set when any slot (or extra
    /// layer) enables dye slot number `n`. These bits are the values the
    /// save's `ItemDyeSaveData._dyeSlotNo` takes for this prefab. This is
    /// not [`Self::slot_count`], which counts sub-meshes: a one-slot prefab
    /// can expose three dye slots.
    pub fn dye_slot_bits(&self) -> u32 {
        self.slots.iter().fold(0, |acc, s| {
            s.extra_layers
                .iter()
                .fold(acc | s.dye_slot_bits(), |a, l| a | l.dye_slot_bits())
        })
    }

    /// Every dye slot number a material channel of the prefab carries
    /// (`mat_indices` entries other than [`NO_DYE_SLOT`]), dyeable or not:
    /// a superset of [`Self::dye_slot_bits`]. Older saves hold `_dyeSlotNo`
    /// values whose mask bit the current table no longer sets (e.g. slot 3
    /// on a channel `[4, 3, 3]` that only enables 4); they are still
    /// channel numbers.
    pub fn channel_dye_slot_bits(&self) -> u32 {
        self.slots
            .iter()
            .flat_map(|s| s.mat_indices)
            .filter(|&n| n != NO_DYE_SLOT && n < 32)
            .fold(0, |a, n| a | (1 << n))
    }

    /// Default material of dye slot number `dye_slot_no`: the first
    /// non-empty primary-layer material name over every channel whose
    /// `mat_indices` entry is `dye_slot_no`. `Some("")` when channels use
    /// the number but none of them names a material; `None` when no channel
    /// uses it. Within a row every channel that shares a number shares its
    /// material name, so "first" does not pick between candidates.
    pub fn dye_slot_material(&self, dye_slot_no: u8) -> Option<&str> {
        let mut used = false;
        for s in &self.slots {
            for (ch, &n) in s.mat_indices.iter().enumerate() {
                if n == dye_slot_no && n != NO_DYE_SLOT {
                    used = true;
                    let m = s.default_materials[ch].as_str();
                    if !m.is_empty() {
                        return Some(m);
                    }
                }
            }
        }
        used.then_some("")
    }
}

/// `mat_indices` value for a material channel that has no dye slot.
pub const NO_DYE_SLOT: u8 = 0xFF;

/// Bit `n` set when `mask[n]` is non-zero.
fn mask_bits(mask: &[u8; 12]) -> u32 {
    mask.iter()
        .enumerate()
        .filter(|(_, b)| **b != 0)
        .fold(0, |acc, (n, _)| acc | (1 << n))
}

impl PartPrefabDyeSlot {
    /// Dye slot number of material channel `ch` (0..3), or `None` when the
    /// channel is unused (`mat_indices[ch] == 0xFF`) or `ch` is out of
    /// range.
    pub fn channel_dye_slot(&self, ch: usize) -> Option<u8> {
        self.mat_indices.get(ch).copied().filter(|&n| n != NO_DYE_SLOT)
    }

    /// The 12-byte mask as bits: bit `n` set when dye slot number `n` is
    /// dyeable on this slot. Every set bit is one of the slot's
    /// `mat_indices`.
    pub fn dye_slot_bits(&self) -> u32 {
        mask_bits(&self.mask)
    }

    /// Per-channel active flags, one byte (0 or 1) per material channel:
    /// channel `ch` is dyeable when it has a dye slot number `n` and
    /// `mask[n]` is set. Two channels that share a number share the flag.
    pub fn active_channels(&self) -> [u8; 3] {
        std::array::from_fn(|ch| {
            u8::from(
                self.channel_dye_slot(ch)
                    .is_some_and(|n| self.mask.get(usize::from(n)).is_some_and(|b| *b != 0)),
            )
        })
    }
}

impl DyeExtraLayer {
    /// The extra layer's mask as bits, indexed by dye slot number like
    /// [`PartPrefabDyeSlot::dye_slot_bits`]. The layer's channels use the
    /// owning slot's `mat_indices`.
    pub fn dye_slot_bits(&self) -> u32 {
        mask_bits(&self.mask)
    }
}

/// Read the 12 mask bytes (see the module's "Mask" section).
fn read_mask(body: &[u8], cursor: &mut usize) -> Option<[u8; 12]> {
    let mask: [u8; 12] = body.get(*cursor..*cursor + 12)?.try_into().ok()?;
    *cursor += 12;
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

/// Parse one slot starting at `cursor`, advancing it past the slot.
/// Returns `None` if the body is truncated / invalid.
fn parse_slot(body: &[u8], cursor: &mut usize) -> Option<PartPrefabDyeSlot> {
    if *cursor + 3 > body.len() {
        return None;
    }
    let mat_indices = [body[*cursor], body[*cursor + 1], body[*cursor + 2]];
    *cursor += 3;

    let mat_a = read_cstring(body, cursor)?;
    let mat_b = read_cstring(body, cursor)?;
    let mat_c = read_cstring(body, cursor)?;

    let mask = read_mask(body, cursor)?;

    // `u8 marker (0xFF) + u32 extra_layer_count`, then that many extra
    // material/dye layers inline.
    if *cursor + 5 > body.len() {
        return None;
    }
    // The 0xFF marker is not stored, but checked: a constant byte is the
    // cheapest place to notice a layout that moved without changing length.
    if body[*cursor] != 0xFF {
        return None;
    }
    let extra_count = u32::from_le_bytes([
        body[*cursor + 1],
        body[*cursor + 2],
        body[*cursor + 3],
        body[*cursor + 4],
    ]) as usize;
    *cursor += 5;
    // Plausibility cap — real counts are 0 or 1.
    if extra_count > 8 {
        return None;
    }
    let mut extra_layers: Vec<DyeExtraLayer> = Vec::with_capacity(extra_count);
    for _ in 0..extra_count {
        let e_a = read_cstring(body, cursor)?;
        let e_b = read_cstring(body, cursor)?;
        let e_c = read_cstring(body, cursor)?;
        let e_mask = read_mask(body, cursor)?;
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

    let tail_name = read_cstring(body, cursor)?;

    Some(PartPrefabDyeSlot {
        mat_indices,
        default_materials: [mat_a, mat_b, mat_c],
        mask,
        tail_name,
        extra_layers,
    })
}

/// Parse one full row body. Returns `Some(entry)` only when the row's
/// head validates against `expected_key` and the slot walk consumes the
/// body **exactly** (`cursor == body.len()`); otherwise `None`.
fn try_parse_row(body: &[u8], expected_key: u32) -> Option<PartPrefabDyeSlotInfoEntry> {
    let head = read_row_head(body, 4)?;
    if head.key != expected_key {
        return None;
    }
    // Head, then 1 zero pad + u32 slot_count + CString prefab_name.
    if body.get(head.body_at) != Some(&0) {
        return None;
    }
    let count_bytes = body.get(head.body_at + 1..head.body_at + 5)?;
    let slot_count =
        u32::from_le_bytes([count_bytes[0], count_bytes[1], count_bytes[2], count_bytes[3]]);
    // Plausibility cap — the largest prefab seen (vehicle/robot meshes)
    // has 36 slots.
    if slot_count == 0 || slot_count > 64 {
        return None;
    }
    let mut cursor = head.body_at + 5;
    let prefab_name = read_cstring(body, &mut cursor)?;
    if !(1..=128).contains(&prefab_name.len()) {
        return None;
    }

    let mut slots: Vec<PartPrefabDyeSlot> = Vec::with_capacity(slot_count as usize);
    for _ in 0..slot_count {
        slots.push(parse_slot(body, &mut cursor)?);
    }
    // Mismatched body length signals schema drift; reject the row.
    if cursor != body.len() {
        return None;
    }

    Some(PartPrefabDyeSlotInfoEntry {
        key: head.key,
        prefab_name,
        slots,
    })
}

/// Parse `partprefabdyeslotinfo.pabgb` using its `.pabgh` index.
///
/// Returns entries in PABGH on-disk order. Rows whose body doesn't match
/// the layout (truncated, slot_count out of plausible range, key mismatch
/// with PABGH, or leftover bytes) are silently dropped.
pub fn parse_part_prefab_dye_slot_info_lossy(
    pabgb: &[u8],
    pabgh: &[u8],
) -> Vec<PartPrefabDyeSlotInfoEntry> {
    // 2.06 moved row names into the header; rebuild the pre-2.06 layout.
    let (pabgb, pabgh) = crate::binary::static_header::legacy_pair(pabgb, pabgh);
    let (pabgb, pabgh): (&[u8], &[u8]) = (&pabgb, &pabgh);
    let Ok(index) = crate::skill_info::parse_pabgh(pabgh) else {
        return Vec::new();
    };
    let ranges = crate::skill_info::entry_ranges(&index, pabgb.len());
    let mut out = Vec::with_capacity(index.len());
    for (entry, (start, end)) in index.iter().zip(ranges.iter()) {
        let Some(body) = pabgb.get(*start..*end) else {
            continue;
        };
        if let Some(parsed) = try_parse_row(body, entry.key) {
            out.push(parsed);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    //! Live-install integration test. Pins the row count + a handful
    //! of (key, prefab_name, slot_count) tuples so a patch breaking the
    //! row layout is caught. Skips cleanly when no game install is present.

    use crate::binary::gamedata_layout;
    use super::*;
    use std::path::PathBuf;

    /// (key, expected prefab_name, expected slot_count).
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
        // Every indexed row must walk the layout exactly; the lossy parser
        // drops any that doesn't.
        // 2.06 headers carry names and sizes; count rows on the rebuilt index.
        let (_, legacy_header) = crate::binary::static_header::legacy_pair(&pabgb, &pabgh);
        assert_eq!(
            entries.len(),
            crate::skill_info::parse_pabgh(&legacy_header).unwrap().len(),
            "a partprefabdyeslotinfo row failed to walk"
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
        // drifts with content (it sits near 1/4), so a fixed fraction is
        // the wrong shape of check. Being the mode is what encodes
        // "right-skewed", and a record-schema drift (the failure mode this
        // guards) scatters slot_count instead of merely shifting its share.
        let modal = hist.iter().max_by_key(|&(_, n)| *n).map(|(k, n)| (*k, *n));
        assert_eq!(
            modal,
            Some((1, count_1)),
            "slot_count=1 should be the modal bucket ({count_1} of {}); histogram {hist:?}",
            entries.len(),
        );

        // Cap check — the largest prefab seen has 36 slots (vehicle/robot
        // meshes); 64 is the defensive ceiling.
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

        // "Expanded dyeable" gear carries a second per-slot dye layer: key
        // 0x54534e48 (cd_phm_00_cloak_0054_01_01_01) must parse AND one of
        // its slots must carry an extra layer whose materials include
        // "leather" (paired with the primary "cloth" layer). Guarded so an
        // install without the key doesn't fail.
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
