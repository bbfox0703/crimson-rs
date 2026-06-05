use super::keys::*;
use super::structs::*;
use crate::binary::*;
use crate::py_binary_struct;

// ── ItemInfo (1.10) ─────────────────────────────────────────────────────────
//
// Crimson Desert 1.10 makes two layout changes relative to 1.09 (which
// itself shared the 1.08 layout — same parser ran clean on both):
//
//   1. The `money_icon_path: StringInfoKey` field (4 bytes) between
//      `map_icon_path` and `use_map_icon_alert` was **removed**. Verified
//      against `out/1.09/iteminfo.pabgb` (1.09 sibling install) by
//      reconstructing every common item from 1.09 bytes with the 4-byte
//      money_icon_path span deleted: 4,459 / 6,314 items match byte-perfect
//      after this single edit; the remaining 1,855 items split into 1,435
//      content-only drifts (multi_change_info growth, weapon stat tweaks,
//      item_memo string changes for flags/carpets) + 420 with an additional
//      schema drift (see #2). The constant `0x73e1c5ea` (LE u32
//      = 0xeac5e173 — a Jenkins hash that was never referenced by anything
//      in the game install) shows up in the deleted span for ~78% of
//      items, suggesting Pearl Abyss had been packing a stub "no money
//      icon" sentinel into this field for years; the 1.10 cleanup just
//      removes the field outright.
//
//   2. `UnitData` (in `structs.rs`) gained a new `u32` field
//      (`unk_post_icon_path`) between `icon_path` and `item_name`. Affects
//      every populated `MoneyTypeDefine` — for Money_Copper's two
//      `MoneyUnitEntry` entries the value is `0xc52007c6` in both. The 1.10
//      patch notes (新增了「交戰」與「重建」階段, 貢獻管理員 / contribution
//      manager reorg, new camp/contribution currencies) match the 17 items
//      that toggled `money_type_define` from None→Some in 1.10
//      (`Money_Camp_*`, `Contribution_*`, `Money_Pearl`, `Cog`,
//      `Item_Pinball_Coin`, etc.).
//
// The RE workflow that pinned both edits lives in
// `scripts/diff_109_110.py` + `verify_109_to_110.py` +
// `scripts/trace_size_mismatch.py`. Result after both edits applied:
// `parser status: ok=6,325  leftover=0  fail=0  no_anchor=0`.
//
// Crimson Desert 1.08 had earlier changed the layout in three places relative
// to 1.07 (which itself shared the 1.05 / 1.06 layout — same parser ran clean
// on all three):
//
//   1. `extract_additional_drop_set_info: u32` was **removed** from between
//      `extract_multi_change_info` and `minimum_extract_enchant_level`.
//
//   2. A new `u8` field (`is_equip_quick_slot_visible`) was **inserted**
//      between `is_housing_only` and `quick_slot_index`. The field is a
//      boolean (only 0 or 1 observed across all 6,314 items: 5,365 zero,
//      949 one). Strongly correlates with the 1.08 patch notes about the
//      new equip-quick-bar reorganization (added tool slot, mask/crown
//      moved to armor slot): the 949 `1`-valued items are exactly the
//      "real equipment" set — every weapon, armor piece, accessory, and
//      tool-slot tool (伐木斧頭 / 鐵鎚 / 鏟子 / 掃帚 / 鐮刀 / 十字鎬 /
//      手鑽/電鋸 / 扇子). Quest items, recipes, consumables, and the
//      cosmetic/plot items that share an `item_type=58 cat=601` with the
//      real tools (Notepad, Feather_Pen, Item_Scale, Abacus, Equip_Drum,
//      Equip_Trumpet, …) all read 0. The strict implication
//      `value=1 ⇒ equip_type_info != 0` holds; the converse does NOT (2,155
//      items have a non-zero `equip_type_info` but `value=0` — those are
//      the quest-flavored equipment items). Name is a best-effort guess
//      from correlation; underlying gamedata symbol still TBD.
//
//   3. A new trailing `u8` field (`unk_post_summon_tag`) was **inserted**
//      inside `DockingChildData` (see `structs.rs`). It only materialises
//      for the 385 items with `docking_child_data.tag = 1` (items whose
//      visual mesh attaches to a character socket — weapons, armor that
//      docks to body sockets, etc.). Every sampled item reads `0x00`;
//      semantic role is unknown — likely a placeholder for a future
//      feature or a reserved field that current gamedata doesn't populate.
//
// Net per-item size delta: -3 bytes for items with `docking_child_data.tag = 0`
// (the 5,929-item majority), -2 bytes for the 385 items with
// `docking_child_data.tag = 1` (the extra `unk_post_summon_tag` cancels
// 1 byte of the removal). Confirmed byte-perfect by reconstructing synthetic
// 1.08 items from 1.07 bytes with `extract_additional_drop_set_info` excised
// and the two `u8` fields inserted at their respective positions (round-trip
// equality verified for Pyeonjeon_Arrow key=2200, Goblin_Pot key=52006,
// Marni_Devotee_PlateArmor_Helm key=14510 and
// KuKu_Lightning_TwoHandSpear key=1002175).
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
        // money_icon_path: StringInfoKey  — removed in 1.10. See header comment.
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
        // Added in Crimson Desert 1.08: boolean (only 0 / 1 observed). Marks
        // items that participate in the reorganized equip-quick-bar system
        // — every weapon, armor piece, accessory, and tool-slot tool reads
        // 1; quest items / recipes / consumables / cosmetic items read 0.
        // Name is a best-effort guess from value-distribution correlation
        // with the 1.08 patch-notes "tool slot" reorganization; revisit
        // once the underlying gamedata symbol is known.
        pub is_equip_quick_slot_visible: u8,
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
