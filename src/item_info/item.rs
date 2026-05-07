use std::io::{self, Write};

use pyo3::prelude::*;
use pyo3::types::PyDict;

use super::keys::*;
use super::structs::*;
use crate::binary::*;
use crate::py_binary_struct;
use crate::python_traits::{ToPyValue, WritePyValue, get_field};

// ── Sub-struct: stable core fields (always present) ─────────────────────────
//
// Everything from `key` through `enable_equip_in_clone_actor`. After this
// comes the 1.05 variant tail (see ItemInfo below): a length-prefixed
// `new_icon_path` CString, then either the legacy respawn/max_endurance
// pair (with an optional 22-byte ammo mid block) or — when the icon path
// is non-empty — a `flag + 9 zero bytes` block. A 5-byte trailer +
// `repair_data_list` (ItemInfoTail) follows in either branch.

py_binary_struct! {
    pub struct ItemInfoCore<'a> {
        pub key: ItemKey,
        pub string_key: CString<'a>,
        pub is_blocked: u8,
        pub max_stack_count: u64,
        pub item_name: LocalizableString<'a>,
        pub broken_item_prefix_string: LocalStringInfoKey,
        pub inventory_info: InventoryKey,
        pub equip_type_info: EquipTypeKey,
        pub occupied_equip_slot_data_list: CArray<OccupiedEquipSlotData>,
        pub item_tag_list: CArray<u32>,
        pub equipable_hash: u32,
        pub consumable_type_list: CArray<u32>,
        pub item_use_info_list: CArray<ItemUseKey>,
        pub item_icon_list: CArray<ItemIconData>,
        pub map_icon_path: StringInfoKey,
        pub money_icon_path: StringInfoKey,
        pub use_map_icon_alert: u8,
        pub item_type: u8,
        pub material_key: u32,
        pub material_match_info: MaterialMatchKey,
        pub item_desc: LocalizableString<'a>,
        pub item_desc2: LocalizableString<'a>,
        pub equipable_level: u32,
        pub category_info: CategoryKey,
        pub knowledge_info: KnowledgeKey,
        pub knowledge_obtain_type: u8,
        pub destroy_effec_info: EffectKey,
        pub equip_passive_skill_list: CArray<PassiveSkillLevel>,
        pub use_immediately: u8,
        pub apply_max_stack_cap: u8,
        pub extract_multi_change_info: MultiChangeKey,
        pub extract_additional_drop_set_info: u32,
        pub minimum_extract_enchant_level: u16,
        pub item_memo: CString<'a>,
        pub filter_type: CString<'a>,
        pub gimmick_info: GimmickInfoKey,
        pub gimmick_tag_list: CArray<CString<'a>>,
        pub max_drop_result_sub_item_count: u32,
        pub use_drop_set_target: u8,
        pub is_all_gimmick_sealable: u8,
        pub sealable_item_info_list: CArray<SealableItemInfo<'a>>,
        pub sealable_character_info_list: CArray<SealableItemInfo<'a>>,
        pub sealable_gimmick_info_list: CArray<SealableItemInfo<'a>>,
        pub sealable_gimmick_tag_list: CArray<SealableItemInfo<'a>>,
        pub sealable_tribe_info_list: CArray<SealableItemInfo<'a>>,
        pub sealable_money_info_list: CArray<ItemKey>,
        pub delete_by_gimmick_unlock: u8,
        pub gimmick_unlock_message_local_string_info: LocalStringInfoKey,
        pub can_disassemble: u8,
        pub transmutation_material_gimmick_list: CArray<GimmickInfoKey>,
        pub transmutation_material_item_list: CArray<ItemKey>,
        pub transmutation_material_item_group_list: CArray<ItemGroupKey>,
        pub is_register_trade_market: u8,
        pub multi_change_info_list: CArray<MultiChangeKey>,
        pub is_editor_usable: u8,
        pub discardable: u8,
        pub is_dyeable: u8,
        pub is_editable_grime: u8,
        pub is_destroy_when_broken: u8,
        pub is_housing_only: u8,
        pub quick_slot_index: u8,
        pub reserve_slot_target_data_list: CArray<ReserveSlotTargetData>,
        pub item_tier: u8,
        pub is_important_item: u8,
        pub apply_drop_stat_type: u8,
        pub drop_default_data: DropDefaultData,
        pub prefab_data_list: CArray<PrefabData>,
        pub enchant_data_list: CArray<EnchantData>,
        pub gimmick_visual_prefab_data_list: CArray<GimmickVisualPrefabData>,
        pub price_list: CArray<ItemPriceInfo>,
        pub docking_child_data: COptional<DockingChildData<'a>>,
        pub inventory_change_data: COptional<InventoryChangeData>,
        pub unk_texture_path: CString<'a>,
        pub fixed_page_data_list: CArray<PageData<'a>>,
        pub dynamic_page_data_list: CArray<PageData<'a>>,
        pub inspect_data_list: CArray<InspectData<'a>>,
        pub inspect_action: InspectAction<'a>,
        pub default_sub_item: SubItem,
        pub cooltime: i64,
        pub unk_post_cooltime_a: i64,
        pub unk_post_cooltime_b: i64,
        pub item_charge_type: u8,
        pub usable_alert_type: u8,
        pub sharpness_data: ItemInfoSharpnessData,
        pub max_charged_useable_count: u32,
        pub unk_post_max_charged_a: u32,
        pub unk_post_max_charged_b: u32,
        pub hackable_character_group_info_list: CArray<CharacterGroupKey>,
        pub item_group_info_list: CArray<ItemGroupKey>,
        pub discard_offset_y: f32,
        pub hide_from_inventory_on_pop_item: u8,
        pub is_shield_item: u8,
        pub is_tower_shield_item: u8,
        pub is_wild: u8,
        pub packed_item_info: ItemKey,
        pub unpacked_item_info: ItemKey,
        pub convert_item_info_by_drop_npc: ItemKey,
        pub pattern_description_data_list: CArray<PatternDescriptionData<'a>>,
        pub look_detail_game_advice_info_wrapper: GameAdviceInfoKey,
        pub look_detail_mission_info: MissionKey,
        pub enable_alert_system_to_ui: u8,
        pub is_save_game_data_at_use_item: u8,
        pub is_logout_at_use_item: u8,
        pub shared_cool_time_group_name_hash: u32,
        pub item_bundle_data_list: CArray<ItemBundleData>,
        pub money_type_define: COptional<MoneyTypeDefine<'a>>,
        pub emoji_texture_id: CString<'a>,
        pub enable_equip_in_clone_actor: u8,
    }
}

// ── Sub-struct: trailer + repair_data_list (always present, 1.05) ──────────

py_binary_struct! {
    pub struct ItemInfoTail {
        pub unk_pre_repair_a: u8,
        pub unk_pre_repair_b: u8,
        pub unk_pre_repair_c: u8,
        pub unk_pre_repair_sentinel: u16, // observed = 0xFFFF on every item
        pub repair_data_list: CArray<RepairData>,
    }
}

// ── ItemInfo: composite with 1.05 variant tail ──────────────────────────────
//
// In Crimson Desert 1.05 the bytes that 1.04 used for
// `is_blocked_store_sell..is_preserved_on_extract` (4 u8) +
// `respawn_time_seconds` (i64) + `max_endurance` (u16) + (optional 22-byte
// "Class A" mid block) were repurposed:
//
//   * The first 4 bytes are now the u32 length prefix of a new CString
//     `new_icon_path` (e.g. "cd_icon_common_camp_donation_00").
//   * If `new_icon_path.length == 0`, the trailing layout matches 1.04 with
//     one quirk: the 18 ammo / projectile items keep an embedded 22-byte
//     `ammo_mid_block` between `max_endurance` and the trailer. We detect
//     ammo by peeking the trailer sentinel — if `data[off+3..off+5]` is
//     `FF FF` the trailer is here, otherwise read 22 bytes first.
//   * If `new_icon_path.length > 0`, the legacy respawn/max_endurance pair
//     is gone. Instead a single `icon_flag: u8` (observed `01`) and 9
//     unknown zero bytes follow before the trailer.
//
// Across 6,236 items in 1.05 this layout reaches the trailer cleanly for
// 5,403 items (86.6%); the remaining failures break in earlier core
// fields (e.g. `item_bundle_data_list`) and are out of scope here.

#[derive(Debug)]
pub struct ItemInfo<'a> {
    pub core: ItemInfoCore<'a>,
    pub new_icon_path: CString<'a>,
    // Present iff new_icon_path.length == 0 (legacy branch).
    pub respawn_time_seconds: Option<i64>,
    pub max_endurance: Option<u16>,
    pub ammo_mid_block: Option<[u8; 22]>,
    // Present iff new_icon_path.length > 0 (icon-path branch).
    pub icon_flag: Option<u8>,
    pub icon_unk_zeros: Option<[u8; 9]>,
    pub tail: ItemInfoTail,
}

/// Peek 5 bytes ahead to see whether the next field is the
/// `(3 u8 + u16=0xFFFF)` trailer. Used as the ammo discriminator after
/// `max_endurance`: if the trailer is here we don't consume the 22-byte
/// `ammo_mid_block`; otherwise we do.
fn trailer_is_at(data: &[u8], offset: usize) -> bool {
    offset + 5 <= data.len() && data[offset + 3] == 0xFF && data[offset + 4] == 0xFF
}

impl<'a> BinaryRead<'a> for ItemInfo<'a> {
    fn read_from(data: &'a [u8], offset: &mut usize) -> io::Result<Self> {
        let core = ItemInfoCore::read_from(data, offset)?;
        let new_icon_path = CString::read_from(data, offset)?;

        let mut respawn_time_seconds = None;
        let mut max_endurance = None;
        let mut ammo_mid_block = None;
        let mut icon_flag = None;
        let mut icon_unk_zeros = None;

        if new_icon_path.length == 0 {
            respawn_time_seconds = Some(i64::read_from(data, offset)?);
            max_endurance = Some(u16::read_from(data, offset)?);
            if !trailer_is_at(data, *offset) {
                ammo_mid_block = Some(<[u8; 22] as BinaryRead>::read_from(data, offset)?);
            }
        } else {
            icon_flag = Some(u8::read_from(data, offset)?);
            icon_unk_zeros = Some(<[u8; 9] as BinaryRead>::read_from(data, offset)?);
        }

        let tail = ItemInfoTail::read_from(data, offset)?;
        Ok(ItemInfo {
            core,
            new_icon_path,
            respawn_time_seconds,
            max_endurance,
            ammo_mid_block,
            icon_flag,
            icon_unk_zeros,
            tail,
        })
    }
}

impl<'a> BinaryReadTracked<'a> for ItemInfo<'a> {
    fn read_tracked(
        data: &'a [u8],
        offset: &mut usize,
        path: &mut String,
        ranges: &mut Vec<FieldRange>,
    ) -> io::Result<Self> {
        let core = ItemInfoCore::read_tracked(data, offset, path, ranges)?;

        let saved = push_path(path, "new_icon_path");
        let new_icon_path = CString::read_tracked(data, offset, path, ranges)?;
        pop_path(path, saved);

        let mut respawn_time_seconds = None;
        let mut max_endurance = None;
        let mut ammo_mid_block = None;
        let mut icon_flag = None;
        let mut icon_unk_zeros = None;

        if new_icon_path.length == 0 {
            let saved = push_path(path, "respawn_time_seconds");
            respawn_time_seconds = Some(i64::read_tracked(data, offset, path, ranges)?);
            pop_path(path, saved);

            let saved = push_path(path, "max_endurance");
            max_endurance = Some(u16::read_tracked(data, offset, path, ranges)?);
            pop_path(path, saved);

            if !trailer_is_at(data, *offset) {
                let saved = push_path(path, "ammo_mid_block");
                let mid =
                    <[u8; 22] as BinaryReadTracked>::read_tracked(data, offset, path, ranges)?;
                pop_path(path, saved);
                ammo_mid_block = Some(mid);
            }
        } else {
            let saved = push_path(path, "icon_flag");
            icon_flag = Some(u8::read_tracked(data, offset, path, ranges)?);
            pop_path(path, saved);

            let saved = push_path(path, "icon_unk_zeros");
            let zeros = <[u8; 9] as BinaryReadTracked>::read_tracked(data, offset, path, ranges)?;
            pop_path(path, saved);
            icon_unk_zeros = Some(zeros);
        }

        let tail = ItemInfoTail::read_tracked(data, offset, path, ranges)?;
        Ok(ItemInfo {
            core,
            new_icon_path,
            respawn_time_seconds,
            max_endurance,
            ammo_mid_block,
            icon_flag,
            icon_unk_zeros,
            tail,
        })
    }
}

impl BinaryWrite for ItemInfo<'_> {
    fn write_to(&self, w: &mut dyn Write) -> io::Result<()> {
        self.core.write_to(w)?;
        self.new_icon_path.write_to(w)?;
        if self.new_icon_path.length == 0 {
            if let Some(v) = self.respawn_time_seconds {
                v.write_to(w)?;
            }
            if let Some(v) = self.max_endurance {
                v.write_to(w)?;
            }
            if let Some(mid) = &self.ammo_mid_block {
                mid.write_to(w)?;
            }
        } else {
            if let Some(f) = self.icon_flag {
                f.write_to(w)?;
            }
            if let Some(z) = &self.icon_unk_zeros {
                z.write_to(w)?;
            }
        }
        self.tail.write_to(w)
    }
}

impl<'a> ItemInfo<'a> {
    /// Build a flattened Python dict containing core fields, the variant
    /// tail fields (with `None` for the branch that wasn't taken), and the
    /// trailer fields — keeping the public API a single flat dict so
    /// downstream code keeps working.
    pub fn to_py_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        use pyo3::types::PyDictMethods;

        let d = self.core.to_py_dict(py)?;
        d.set_item("new_icon_path", self.new_icon_path.data)?;
        match self.respawn_time_seconds {
            Some(v) => d.set_item("respawn_time_seconds", v)?,
            None => d.set_item("respawn_time_seconds", py.None())?,
        };
        match self.max_endurance {
            Some(v) => d.set_item("max_endurance", v)?,
            None => d.set_item("max_endurance", py.None())?,
        };
        match &self.ammo_mid_block {
            Some(arr) => d.set_item("ammo_mid_block", arr.to_py_value(py)?)?,
            None => d.set_item("ammo_mid_block", py.None())?,
        };
        match self.icon_flag {
            Some(v) => d.set_item("icon_flag", v)?,
            None => d.set_item("icon_flag", py.None())?,
        };
        match &self.icon_unk_zeros {
            Some(arr) => d.set_item("icon_unk_zeros", arr.to_py_value(py)?)?,
            None => d.set_item("icon_unk_zeros", py.None())?,
        };
        let tail = self.tail.to_py_dict(py)?;
        for (k, v) in tail.iter() {
            d.set_item(k, v)?;
        }
        Ok(d)
    }

    pub fn write_from_py_dict(
        w: &mut Vec<u8>,
        d: &Bound<'_, PyDict>,
    ) -> PyResult<()> {
        ItemInfoCore::write_from_py_dict(w, d)?;
        let icon_path = get_field(d, "new_icon_path")?;
        let icon_path_str: String = icon_path.extract()?;
        let length = icon_path_str.len() as u32;
        w.extend_from_slice(&length.to_le_bytes());
        w.extend_from_slice(icon_path_str.as_bytes());
        if length == 0 {
            let respawn = get_field(d, "respawn_time_seconds")?;
            if !respawn.is_none() {
                <i64 as WritePyValue>::write_from_py(w, &respawn)?;
            }
            let me = get_field(d, "max_endurance")?;
            if !me.is_none() {
                <u16 as WritePyValue>::write_from_py(w, &me)?;
            }
            let mid = get_field(d, "ammo_mid_block")?;
            if !mid.is_none() {
                <[u8; 22] as WritePyValue>::write_from_py(w, &mid)?;
            }
        } else {
            let flag = get_field(d, "icon_flag")?;
            if !flag.is_none() {
                <u8 as WritePyValue>::write_from_py(w, &flag)?;
            }
            let zeros = get_field(d, "icon_unk_zeros")?;
            if !zeros.is_none() {
                <[u8; 9] as WritePyValue>::write_from_py(w, &zeros)?;
            }
        }
        ItemInfoTail::write_from_py_dict(w, d)?;
        Ok(())
    }
}

impl ToPyValue for ItemInfo<'_> {
    fn to_py_value(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        Ok(self.to_py_dict(py)?.into_any().unbind())
    }
}

impl WritePyValue for ItemInfo<'_> {
    fn write_from_py(w: &mut Vec<u8>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        Self::write_from_py_dict(w, obj.cast::<PyDict>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 1.04 baseline binary used by the legacy parse / roundtrip tests below.
    // The path points at a WSL mount that doesn't exist on most CI runners
    // and on Windows checkouts — `try_load_binary` returns `None` in that
    // case so the tests skip cleanly instead of failing.
    //
    // Note: the 1.05 parser is NOT byte-compatible with the 1.04 file
    // wholesale (the new_icon_path CString reinterprets four 1.04 u8 bool
    // fields). These tests will only pass on the subset of 1.04 items
    // where those four bools are all zero — which happens to include
    // Pyeonjeon_Arrow.
    const BINARY_PATH: &str =
        "/mnt/e/OpensourceGame/CrimsonDesert/Godmod/backups/iteminfo_1.0.4.1.pabgb";

    fn try_load_binary() -> Option<Vec<u8>> {
        std::fs::read(BINARY_PATH).ok()
    }

    #[test]
    fn test_parse_first_item() {
        let Some(data) = try_load_binary() else {
            eprintln!("skipping: 1.04 baseline binary not found at {BINARY_PATH}");
            return;
        };
        let mut offset = 0;
        let item = ItemInfo::read_from(&data, &mut offset).unwrap();
        assert_eq!(item.core.key, ItemKey(2200));
        assert_eq!(item.core.string_key.data, "Pyeonjeon_Arrow");
    }

    #[test]
    fn test_first_item_roundtrip() {
        let Some(data) = try_load_binary() else {
            eprintln!("skipping: 1.04 baseline binary not found at {BINARY_PATH}");
            return;
        };
        let mut offset = 0;
        let item = ItemInfo::read_from(&data, &mut offset).unwrap();
        let end = offset;

        let mut out = Vec::new();
        item.write_to(&mut out).unwrap();
        assert_eq!(out.len(), end, "written size mismatch");
        assert_eq!(&out[..], &data[..end], "roundtrip bytes mismatch");
    }
}
