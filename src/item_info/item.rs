use super::keys::*;
use super::structs::*;
use crate::binary::*;
use crate::py_binary_struct;

// ── ItemInfo (1.05) ─────────────────────────────────────────────────────────
//
// Crimson Desert 1.08 changes the layout in two places relative to 1.07
// (which itself shared the 1.05 / 1.06 layout — same parser ran clean on all
// three):
//
//   1. `extract_additional_drop_set_info: u32` was **removed** from between
//      `extract_multi_change_info` and `minimum_extract_enchant_level`.
//
//   2. A new `u8` field (`unk_post_is_housing_only`) was **inserted** between
//      `is_housing_only` and `quick_slot_index`. Values span the full 0..255
//      range across the 6,314 items in 1.08 (many items hold a small value
//      that visually mirrors `quick_slot_index`), so the field is not a
//      boolean flag — its semantic role is unknown until a future RE pass
//      narrows it down.
//
// Net per-item size delta: -3 bytes (4 removed, 1 added). The change was
// confirmed byte-perfect by reconstructing synthetic 1.08 items from the
// 1.07 bytes with `extract_additional_drop_set_info` excised and the new
// u8 inserted (round-trip equality verified for Pyeonjeon_Arrow key=2200
// and Goblin_Pot key=52006).
//
// Crimson Desert 1.05 had earlier introduced two layout changes relative to
// 1.04:
//
//   1. Each `ItemIconData` entry grew by 5 bytes — a new `icon_path_alt`
//      `StringInfoKey` was added between `icon_path` and
//      `check_exist_sealed_data`, and a trailing `unk_flag: u8` was added
//      after `gimmick_state_list`.
//
//   2. A new 5-byte block (`unk_pre_pattern_key: u32 + unk_pre_pattern_flag: u8`)
//      was inserted between `convert_item_info_by_drop_npc` and
//      `pattern_description_data_list`. The u32 is always 0; the u8 is 1
//      only for the 48 fish-food items (`Food_Salmon`, `Food_Trout`, …) and
//      0 otherwise.
//
// Those 1.05 changes remain in the 1.08 layout. The earlier "variant tail"
// interpretation
// (`new_icon_path` + `ammo_mid_block` + `unk_pre_repair_*`) was a
// misinterpretation that happened to round-trip on items where the misread
// bytes coincidentally satisfied later parser checks (e.g. ammo items where
// `max_endurance == 0xFFFF` provided the bogus "trailer sentinel"); it is
// dropped here.

py_binary_struct! {
    pub struct ItemInfo<'a> {
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
        // Removed in Crimson Desert 1.08:
        //   pub extract_additional_drop_set_info: u32,
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
        // Added in Crimson Desert 1.08: u8 between `is_housing_only` and
        // `quick_slot_index`. Values span 0..255 across the 6,314 items, so
        // this is not a boolean flag — semantic role unknown until a future
        // RE pass identifies it.
        pub unk_post_is_housing_only: u8,
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
        // Added in Crimson Desert 1.05: a u32 + u8 pair before
        // `pattern_description_data_list`. Across 6,236 items the u32 is
        // always 0; the u8 is 1 only for the 48 fish-food items
        // (`Food_Salmon`, `Food_Trout`, `Food_Carp`, …) and 0 otherwise.
        pub unk_pre_pattern_key: u32,
        pub unk_pre_pattern_flag: u8,
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
        pub repair_data_list: CArray<RepairData>,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The parser targets Crimson Desert 1.05. The test reads the 1.05
    // binary that `scripts\export_for_ce.py` extracts to `out\iteminfo.pabgb`.
    // Both files are gitignored (they ship Pearl Abyss content) and the
    // test skips cleanly when they aren't present.
    const BINARY_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        r"\out\iteminfo.pabgb"
    );

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
        assert_eq!(item.key, ItemKey(2200));
        assert_eq!(item.string_key.data, "Pyeonjeon_Arrow");
    }
}
