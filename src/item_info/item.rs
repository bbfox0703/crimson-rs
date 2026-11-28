use super::keys::*;
use super::structs::*;
use crate::binary::*;
use crate::py_binary_struct;

// ── ItemInfo row layout ─────────────────────────────────────────────────────
//
// The parser reads the latest patch's layout only; what each patch changed and
// how it was pinned is in `docs/validated-patches.md`.
//
//   - **Row head.** The name is not in the row: the `.staticinfoheader` lists
//     `[key][u32 len][name][u32 offset][u32 size]` per row (see
//     `binary::static_header`), and the row opens `[key][u32 row index]`.
//     `string_key` is therefore a `HeaderName`, which takes no bytes in the row
//     and is filled in from the header by `parse_iteminfo_with_header`. In the
//     base table the row index equals the row's position; in `iteminfo.dlc_01`
//     it does not, and its meaning there is open.
//   - **A fixed block, then the variable part.** `is_blocked` through
//     `unk_post_max_charged_b` sit at fixed offsets on every row, u8s first,
//     then (mostly) u32s. The struct below is in file order, not topic order.
//   - **Empty lists and strings cost one byte.** Most top-level lists are
//     `CFlaggedArray` (`0x00`, or `0x01` + a `CArray`), the three display texts
//     are `OptLocalizableString`, and `drop_default_data`, `sharpness_data` and
//     `inspect_action` are `COptional`. Lists inside elements keep the plain
//     count prefix.
//   - **Some fields cannot be ordered from the data.** Where two fields are
//     always equal or always zero (`is_dyeable` / `is_editable_grime`,
//     `material_key` / `material_match_info`, the zero u8s and u32s of the
//     fixed block), the struct keeps an order that round-trips but is not
//     proven.
//   - Fields named `unk_*` have no known game name and are named for their
//     position.

py_binary_struct! {
    pub struct ItemInfo<'a> {
        // 2.06: the name lives in the `.staticinfoheader`; no bytes here.
        pub string_key: HeaderName<'a>,
        pub key: ItemKey,
        // The row's index: its position in the base table, not in `dlc_01`.
        pub row_index: u32,
        pub is_blocked: u8,
        pub max_endurance: u16,
        pub max_stack_count: u64,
        pub apply_drop_stat_type: u8,
        // Three-valued: 0 marks an important item, 2 the rest; 1 is rare.
        pub item_importance_kind: u8,
        pub is_dyeable: u8,
        pub is_editable_grime: u8,
        pub discardable: u8,
        // 1 on the items whose `item_type` is 102 or 103, mostly gimmick attach
        // and puzzle parts (`Item_gimmick_puzzle_antumbra_tokamak_*`,
        // `Item_gimmick_attach_*`), and 0 elsewhere.
        pub unk_post_discardable: u8,
        // Set on the `Trade_*_PackedInVehicle` items.
        pub is_packed_in_vehicle: u8,
        pub is_housing_only: u8,
        pub is_destroy_when_broken: u8,
        pub hide_from_inventory_on_pop_item: u8,
        pub is_blocked_store_sell: u8,
        pub enable_equip_in_clone_actor: u8,
        pub is_save_game_data_at_use_item: u8,
        pub is_preserved_on_extract: u8,
        pub is_has_item_use_data_inventory_buff: u8,
        pub is_preorder_item: u8,
        pub is_wild: u8,
        pub is_tower_shield_item: u8,
        pub is_shield_item: u8,
        pub is_logout_at_use_item: u8,
        pub enable_alert_system_to_ui: u8,
        // A flag of its own, not a byte of `packed_item_info`: reading it as one
        // puts the keys that follow one byte out of step.
        pub unk_pre_packed_item_flag: u8,
        pub is_editor_usable: u8,
        pub use_map_icon_alert: u8,
        // Boolean: marks items that take part in the equip quick bar. Every
        // weapon, armor piece, accessory and tool-slot tool reads 1; quest
        // items, recipes, consumables and cosmetic items read 0.
        pub is_equip_quick_slot_visible: u8,
        pub use_immediately: u8,
        pub apply_max_stack_cap: u8,
        pub can_disassemble: u8,
        pub is_register_trade_market: u8,
        pub use_drop_set_target: u8,
        pub delete_by_gimmick_unlock: u8,
        pub is_all_gimmick_sealable: u8,
        pub item_type: u8,
        pub knowledge_obtain_type: u8,
        pub item_tier: u8,
        pub usable_alert_type: u8,
        pub item_charge_type: u8,
        // Boolean of unknown role.
        pub unk_post_apply_drop_stat_type: u8,
        pub look_detail_mission_info: MissionKey,
        pub look_detail_game_advice_info_wrapper: GameAdviceInfoKey,
        pub convert_item_info_by_drop_npc: ItemKey,
        pub unpacked_item_info: ItemKey,
        // 0 on nearly every item; an ItemKey-shaped value on the
        // `Wood_Branch_*` gathering-node tiers.
        pub unk_pre_respawn_a: u32,
        pub broken_item_prefix_string: LocalStringInfoKey,
        pub equip_type_info: EquipTypeKey,
        pub material_key: u32,
        pub map_icon_path: StringInfoKey,
        pub packed_item_info: ItemKey,
        pub gimmick_info: GimmickInfoKey,
        pub destroy_effec_info: EffectKey,
        pub equipable_level: u32,
        pub minimum_extract_enchant_level: u16,
        pub category_info: CategoryKey,
        pub gimmick_unlock_message_local_string_info: LocalStringInfoKey,
        pub knowledge_info: KnowledgeKey,
        pub max_drop_result_sub_item_count: u32,
        pub equipable_hash: u32,
        pub default_sub_item: SubItem,
        pub discard_offset_y: f32,
        pub extract_multi_change_info: MultiChangeKey,
        pub material_match_info: MaterialMatchKey,
        pub shared_cool_time_group_name_hash: u32,
        pub unk_pre_max_endurance_a: u32,
        pub max_charged_useable_count: u32,
        pub unk_post_max_charged_a: u32,
        pub unk_post_max_charged_b: u32,
        pub item_name: OptLocalizableString<'a>,
        pub item_desc: OptLocalizableString<'a>,
        pub item_desc2: OptLocalizableString<'a>,
        pub item_memo: CString<'a>,
        pub filter_type: CString<'a>,
        // Empty except on the `iteminfo.dlc_01` ship cannons, which read
        // "env_walla_ship_combat_cannon_ready" / "SoundNameOnMaxCharged". Named
        // from those rows.
        pub charged_sound_name: CString<'a>,
        pub charged_sound_param: CString<'a>,
        // Jenkins hashes of a string that lives in `stringinfo`; the empty
        // string hashes to 0xeac5e173.
        pub unk_texture_path: StringInfoKey,
        pub emoji_texture_id: StringInfoKey,
        pub occupied_equip_slot_data_list: CFlaggedArray<OccupiedEquipSlotData>,
        pub item_tag_list: CFlaggedArray<u32>,
        pub item_use_info_list: CFlaggedArray<ItemUseKey>,
        pub item_icon_list: CArray<ItemIconData>,
        pub equip_passive_skill_list: CFlaggedArray<PassiveSkillLevel>,
        pub gimmick_tag_list: CFlaggedArray<CString<'a>>,
        pub sealable_item_info_list: CFlaggedArray<SealableItemInfo<'a>>,
        pub sealable_character_info_list: CFlaggedArray<SealableItemInfo<'a>>,
        pub sealable_gimmick_info_list: CFlaggedArray<SealableItemInfo<'a>>,
        pub sealable_gimmick_tag_list: CFlaggedArray<SealableItemInfo<'a>>,
        pub sealable_tribe_info_list: CFlaggedArray<SealableItemInfo<'a>>,
        pub sealable_money_info_list: CFlaggedArray<ItemKey>,
        pub transmutation_material_gimmick_list: CFlaggedArray<GimmickInfoKey>,
        pub transmutation_material_item_list: CFlaggedArray<ItemKey>,
        pub transmutation_material_item_group_list: CFlaggedArray<ItemGroupKey>,
        pub multi_change_info_list: CFlaggedArray<MultiChangeKey>,
        pub reserve_slot_target_data_list: CFlaggedArray<ReserveSlotTargetData>,
        // Absent where it would be all defaults (no sockets, tag-18 sub item).
        pub drop_default_data: COptional<DropDefaultData>,
        pub enchant_data_list: EnchantDataList,
        pub unk_pre_gimmick_visual: u32 => equip_type_info.0 != 0 || item_type == 74,
        pub price_list: CArray<ItemPriceInfo>,
        pub docking_child_data: COptional<DockingChildData<'a>>,
        // Empty except on the `Item_wagon_wheel_*` items, one element per wheel
        // socket. `docking_child_data` above is a separate, singular field.
        pub docking_child_data_list: CFlaggedArray<DockingChildData<'a>>,
        pub inventory_change_data: COptional<InventoryChangeData>,
        pub fixed_page_data_list: CFlaggedArray<PageData<'a>>,
        pub dynamic_page_data_list: CFlaggedArray<PageData<'a>>,
        pub inspect_data_list: CFlaggedArray<InspectData<'a>>,
        pub inspect_action: COptional<InspectAction<'a>>,
        // Reads 1 on every item.
        pub unk_pre_cooltime_flag: u8,
        pub cooltime: i64,
        pub unk_post_cooltime_a: i64,
        pub unk_post_cooltime_b: i64,
        pub sharpness_data: COptional<ItemInfoSharpnessData>,
        pub hackable_character_group_info_list: CFlaggedArray<CharacterGroupKey>,
        pub item_group_info_list: CArray<ItemGroupKey>,
        pub pattern_description_data_list: CFlaggedArray<PatternDescriptionData<'a>>,
        pub item_bundle_data_list: CFlaggedArray<ItemBundleData>,
        pub money_type_define: COptional<MoneyTypeDefine<'a>>,
        // Absent on base-table items; some `iteminfo.dlc_01` rows carry it.
        pub unk_pre_respawn_block: COptional<UnkPreRespawnBlock>,
        pub respawn_time_seconds: i64,
        pub repair_data_list: CFlaggedArray<RepairData>,
        // Prefab and gimmick-visual entries in one list (see
        // `MergedPrefabVisualData`).
        pub merged_prefab_visual_list: CArray<MergedPrefabVisualData>,
        // InventoryKey slots; 0xFF marks an unused slot. Slot 0 is the item's
        // inventory, which `crimson_iteminfo_lookup_inventory_info` returns.
        pub inventory_info_list: [u16; 12],
    }
}

/// Parse a 2.06 iteminfo table: the rows its `.staticinfoheader` lists, in
/// order, each with `string_key` filled in from the header. Fails on a header
/// that is not the 2.06 shape, a row whose key disagrees with the header, or a
/// row the parser does not consume exactly.
pub fn parse_iteminfo_with_header<'a>(
    body: &'a [u8],
    header: &'a [u8],
) -> std::io::Result<Vec<ItemInfo<'a>>> {
    use std::io::{Error, ErrorKind};
    let rows = crate::binary::static_header::parse_named_header(header, 2, 4)
        .ok_or_else(|| Error::new(ErrorKind::InvalidData, "not a 2.06 iteminfo header"))?;
    // The rows must tile the body: a skipped row or bytes after the last row
    // would otherwise be lost on write-back.
    if !crate::binary::static_header::rows_are_packed(&rows, body.len()) {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "iteminfo header rows do not tile the body (gap, overlap or trailing bytes)",
        ));
    }
    let mut out = Vec::with_capacity(rows.len());
    for (i, r) in rows.iter().enumerate() {
        let row = body.get(r.range()).ok_or_else(|| {
            Error::new(ErrorKind::UnexpectedEof, format!("row {i} ({}) runs past the body", r.name))
        })?;
        let mut off = 0;
        let mut item = ItemInfo::read_from(row, &mut off)
            .map_err(|e| Error::new(e.kind(), format!("row {i} ({}): {e}", r.name)))?;
        if off != row.len() {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!("row {i} ({}): {} of {} bytes read", r.name, off, row.len()),
            ));
        }
        if u64::from(item.key.0) != r.key {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!("row {i}: key {} in the row, {} in the header", item.key.0, r.key),
            ));
        }
        item.string_key = HeaderName { data: r.name };
        out.push(item);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The live install's tables; the tests skip without one.
    fn try_load_binary() -> Option<Vec<u8>> {
        crate::binary::gamedata_layout::extract_bin(&crate::binary::gamedata_layout::body("iteminfo"))
    }

    #[test]
    fn test_parse_first_item() {
        let Some(data) = try_load_binary() else {
            eprintln!("skipping: no game install");
            return;
        };
        let mut offset = 0;
        let item = ItemInfo::read_from(&data, &mut offset).unwrap();
        assert_eq!(item.key, ItemKey(2200));
        assert_eq!(item.row_index, 0);
        // 2.06: the name comes from the header, not the row.
        assert_eq!(item.string_key.data, "");
    }

    #[test]
    fn test_parse_with_header() {
        let header = crate::binary::gamedata_layout::extract_bin(&crate::binary::gamedata_layout::header("iteminfo"));
        let (Some(data), Some(header)) = (try_load_binary(), header) else {
            eprintln!("skipping: no game install");
            return;
        };
        let items = parse_iteminfo_with_header(&data, &header).unwrap();
        assert_eq!(items[0].key, ItemKey(2200));
        assert_eq!(items[0].string_key.data, "Pyeonjeon_Arrow");
        assert!(items.iter().enumerate().all(|(i, it)| it.row_index as usize == i));
    }

    /// Every row of the live install's iteminfo, base and `dlc_01`, writes back
    /// byte for byte. The DLC half matters: some fields are populated only in
    /// the DLC table. Skips without an install; the DLC half skips without
    /// `dlc_01`.
    #[test]
    fn live_iteminfo_rows_write_back_byte_for_byte() {
        use crate::binary::BinaryWrite;
        use crate::binary::gamedata_layout::{body, extract_bin, extract_dlc_bin, header};
        let tables = [
            ("iteminfo", extract_bin(&body("iteminfo")), extract_bin(&header("iteminfo"))),
            (
                "iteminfo.dlc_01",
                extract_dlc_bin("dlc_01", &body("iteminfo.dlc_01")),
                extract_dlc_bin("dlc_01", &header("iteminfo.dlc_01")),
            ),
        ];
        for (name, b, h) in tables {
            let (Some(b), Some(h)) = (b, h) else {
                eprintln!("skipping {name}: not installed");
                continue;
            };
            let rows = crate::binary::static_header::parse_named_header(&h, 2, 4).expect("2.06 header");
            let items = parse_iteminfo_with_header(&b, &h).unwrap();
            assert_eq!(items.len(), rows.len(), "{name}: rows");
            for (item, row) in items.iter().zip(&rows) {
                let mut out = Vec::new();
                item.write_to(&mut out).unwrap();
                assert!(out == b[row.range()], "{name}: row {} ({}) differs on write-back", item.key.0, row.name);
            }
            eprintln!("{name}: {} rows write back byte for byte", items.len());

            // A header that drops the last row does not tile the body and is
            // refused.
            let mut short = ((rows.len() - 1) as u16).to_le_bytes().to_vec();
            for r in &rows[..rows.len() - 1] {
                short.extend_from_slice(&(r.key as u32).to_le_bytes());
                short.extend_from_slice(&(r.name.len() as u32).to_le_bytes());
                short.extend_from_slice(r.name.as_bytes());
                short.extend_from_slice(&r.offset.to_le_bytes());
                short.extend_from_slice(&r.size.to_le_bytes());
            }
            assert!(parse_iteminfo_with_header(&b, &short).is_err(), "{name}: short header");
        }
    }
}
