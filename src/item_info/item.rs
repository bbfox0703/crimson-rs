use std::io::{self, Write};

use pyo3::prelude::*;
use pyo3::types::PyDict;

use super::keys::*;
use super::structs::*;
use crate::binary::*;
use crate::py_binary_struct;
use crate::python_traits::{ToPyValue, WritePyValue, get_field};

// ── Sub-struct: pre-max_endurance fields (always present) ───────────────────
//
// Everything from `key` through `max_endurance`. After this comes a
// conditional 22-byte mid block (Class A only — items with real durability),
// then a 5-byte trailer + repair_data_list common to all items.

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
        pub is_blocked_store_sell: u8,
        pub is_preorder_item: u8,
        pub is_has_item_use_data_inventory_buff: u8,
        pub is_preserved_on_extract: u8,
        pub respawn_time_seconds: i64,
        pub max_endurance: u16,
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

// ── ItemInfo: composite with conditional 22-byte mid block ──────────────────
//
// Discriminator (verified across all 5,518 items reaching max_endurance):
//   if max_endurance ∈ {0, 0xFFFF}      → no mid block (Class B, 2,967 items)
//   else                                → 22-byte mid block follows (Class A,
//                                          2,551 items)
//
// The 22 bytes are kept as a raw [u8; 22] here so the serializer roundtrips
// byte-perfectly while we work out the field-level decomposition. The bytes
// observed for items 0-6 carry an embedded ItemKey + several zero u32s;
// future iterations should split this into named fields once their meaning
// is understood.

#[derive(Debug)]
pub struct ItemInfo<'a> {
    pub core: ItemInfoCore<'a>,
    pub mid_block_class_a: Option<[u8; 22]>,
    pub tail: ItemInfoTail,
}

fn class_a(max_endurance: u16) -> bool {
    max_endurance != 0 && max_endurance != 0xFFFF
}

impl<'a> BinaryRead<'a> for ItemInfo<'a> {
    fn read_from(data: &'a [u8], offset: &mut usize) -> io::Result<Self> {
        let core = ItemInfoCore::read_from(data, offset)?;
        let mid_block_class_a = if class_a(core.max_endurance) {
            Some(<[u8; 22] as BinaryRead>::read_from(data, offset)?)
        } else {
            None
        };
        let tail = ItemInfoTail::read_from(data, offset)?;
        Ok(ItemInfo {
            core,
            mid_block_class_a,
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
        let mid_block_class_a = if class_a(core.max_endurance) {
            let saved = push_path(path, "mid_block_class_a");
            let mid = <[u8; 22] as BinaryReadTracked>::read_tracked(data, offset, path, ranges)?;
            pop_path(path, saved);
            Some(mid)
        } else {
            None
        };
        let tail = ItemInfoTail::read_tracked(data, offset, path, ranges)?;
        Ok(ItemInfo {
            core,
            mid_block_class_a,
            tail,
        })
    }
}

impl BinaryWrite for ItemInfo<'_> {
    fn write_to(&self, w: &mut dyn Write) -> io::Result<()> {
        self.core.write_to(w)?;
        if let Some(mid) = &self.mid_block_class_a {
            mid.write_to(w)?;
        }
        self.tail.write_to(w)
    }
}

impl<'a> ItemInfo<'a> {
    /// Build a flattened Python dict containing core fields, the optional
    /// mid_block_class_a (as a list[int] or None), and tail fields — keeping
    /// the public API a single flat dict so downstream code keeps working.
    pub fn to_py_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        use pyo3::types::PyDictMethods;

        let d = self.core.to_py_dict(py)?;
        match &self.mid_block_class_a {
            Some(arr) => d.set_item("mid_block_class_a", arr.to_py_value(py)?)?,
            None => d.set_item("mid_block_class_a", py.None())?,
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
        let mid = get_field(d, "mid_block_class_a")?;
        if !mid.is_none() {
            <[u8; 22] as WritePyValue>::write_from_py(w, &mid)?;
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

    const BINARY_PATH: &str =
        "/mnt/e/OpensourceGame/CrimsonDesert/Godmod/backups/iteminfo_1.0.4.1.pabgb";

    fn load_binary() -> Vec<u8> {
        std::fs::read(BINARY_PATH).expect("binary file not found")
    }

    #[test]
    fn test_parse_first_item() {
        let data = load_binary();
        let mut offset = 0;
        let item = ItemInfo::read_from(&data, &mut offset).unwrap();
        assert_eq!(item.core.key, ItemKey(2200));
        assert_eq!(item.core.string_key.data, "Pyeonjeon_Arrow");
        assert_eq!(offset, 0x00000270, "unexpected size for first item");
    }

    #[test]
    fn test_first_item_roundtrip() {
        let data = load_binary();
        let mut offset = 0;
        let item = ItemInfo::read_from(&data, &mut offset).unwrap();
        let end = offset;

        let mut out = Vec::new();
        item.write_to(&mut out).unwrap();
        assert_eq!(out.len(), end, "written size mismatch");
        assert_eq!(&out[..], &data[..end], "roundtrip bytes mismatch");
    }
}
