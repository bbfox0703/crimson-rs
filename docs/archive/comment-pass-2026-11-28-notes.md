# Facts removed from source comments (2026-11-28)

The 2026-11-28 comment pass moved history out of the source (rule: "Code comments" in `CLAUDE.md`). Most of what it removed is already in [`../dev-log.md`](../dev-log.md), [`../validated-patches.md`](../validated-patches.md) or the other archived documents. These are the measurements and reverse-engineering findings that were not, kept here as the agents recorded them, grouped by source area. The per-patch iteminfo layout history is archived separately in [`iteminfo-layout-history.md`](iteminfo-layout-history.md). Paths name the file the comment sat in.

## Save-editor C ABI (`src/c_abi/mod.rs`)

- `src/c_abi/mod.rs` (`crimson_save_list_inventory_items`): the 1.07 reference save had 18 `_inventorylist` containers holding 543 items; listing them took under 1 ms. That sample showed about 7 distinct `inventory_key` categories.
- `src/c_abi/mod.rs` (`crimson_save_list_character_refs`): on the 1.07 reference save the walk covered ~10k blocks x ~30 fields in well under 100 ms.
- `src/c_abi/mod.rs` (`crimson_save_set_scalar_field`): the in-place set + full `decode_blocks` took milliseconds on a 1,112-block save.
- `src/c_abi/mod.rs` (`crimson_save_begin_deferred_redecode`): one length-changing cycle (`Body::write` + `Body::parse` + `decode_blocks`) took ~25 ms on a typical 1.07 save; completing 141 held abyss-artifact challenges (3 length-changing calls each, ~423 re-decodes) cost 10+ s without a deferred batch.
- `src/c_abi/mod.rs` (`crimson_save_set_scalar_fields_present_batch`): promoting `_completedTime` on ~1,300 `MissionStateData` blocks one call at a time was estimated at ~20 minutes on a ~1,100-block save, versus seconds for one batched re-emit.
- `src/c_abi/mod.rs` (`crimson_save_set_scalar_fields_batch`): the "fill every stack to max" tool issued 168 scalar writes.

## Probes, portraits, positions, item enumeration

- `src/c_abi/character_info.rs` (`_probe_character_appearance_index` doc): the probe doc said 16 of 122 distinct save-side appearance keys (13%) map to a PABGH entry; `docs/save-editor-keys-plan.md` says 9 of 122 (7%). Not reconciled; the rest of that probe's findings (file sizes 236 KB / 97 KB, 8,143 pabgh rows, 7,027 in the `0xfffffffe` bucket, 29-byte entries) are already in the plan's §9.
- `src/c_abi/character_info.rs` (`_probe_itemkey_partprefab_linkage` / `_probe_partprefab_linkage_table_scan` docs, 1.07 era): the direct `iteminfo.prefab_data_list[].prefab_names[].0` == `partprefabdyeslotinfo` row key match had 0% intersection (5,377 items x 4,261 distinct hashes); a stride-1 u32 scan of iteminfo found 0% direct overlap with partprefab row keys, and the row keys were not present in stringinfo. (`docs/dye-editor-scope.md` records the working stringinfo join but not these negative results.)

## Item, dye and curated-quest bridges

- `src/c_abi/element_template.rs` (module docs, inline object locator rule): across saves from 1.10 to 2.03, inline object locators (`meta_kind` 4) were present about 64k times and never absent; the template builder relies on this to create a missing one with an empty child.
- `src/c_abi/iteminfo.rs` (`canonical_gem_list`): the `item_type == 74 && category_info == 2501` gem rule was checked against the slot104 reference save: all 43 gems it holds match, and the two host items 1000316 / 1002285 do not.

## Gamedata key-resolver bridges

- `src/c_abi/stage_info.rs` (`crimson_stageinfo_load_from_file`, `c_abi_stageinfo_pabgh` test doc): on 1.08 the PABGH-driven loader returned 51,037 rows against 49,634 from the anchor scan; the ~1,400 missing were `LevelSequencerSpawn_UUID_…,…` rows whose names contain commas (first such row in 1.08: key 1047476). On 1.06 the table had ~57k rows (old test comment; inconsistent with the 1.08 figure, so treat as approximate).
- `src/c_abi/quest_info.rs` (`crimson_questinfo_load_from_file`): on 1.08 the PABGH path returned 1,019 quests against 1,018 from the anchor scan (the `_`-in-name check drops `LevelSequencerSpawn` and `StartGame`); 2.04 removed `LevelSequencerSpawn` (key 1000414).
- `src/c_abi/string_info.rs` (live test): `stringinfo.pabgb` had 30,206 entries on 1.06 and 33,855 on 2.06.
- `src/c_abi/mission_info.rs` (module doc): ~4,300 missions on 1.06.
- `src/c_abi/skill_info.rs` (module doc): about 2,160 skills on 2.06.

## Core, archive and save format

- `src/save/body/object.rs` (`absent_kind_has_marker`): adding `meta_kind` 1 (inline bytes) to the absence-marker rule breaks ~60k objects per save (measured on the 12 live saves, game 1.10 to 2.03).
- `src/save/body/object.rs` (`ObjectBlock::trailing_pad`): census on the sample save: 232 blocks carry exactly 1 pad byte (FieldNPCSaveData + 6 other classes), 1 block (MercenaryClanSaveData) carries `01 01 01`.
- `src/save/body/object.rs` (`ScalarValue::F32x3` / `U32x4`, composite arms of `scalar_from_bytes`): in the live 1.07 sample save, `float3` has ~58k occurrences, `uint4` ~11k, and the composite arms together cover ~70k field occurrences.

## Table parsers

### item_info

- `src/item_info/item.rs` (ItemInfo header, 1.10): the deleted `money_icon_path` span held `0xeac5e173` (Jenkins of the empty string) on ~78% of items; reconstructing 1.10 from 1.09 bytes with only that span deleted matched 4,459 / 6,314 items byte-perfect, the rest split into 1,435 content-only drifts + 420 with the `UnitData` drift. 17 items toggled `money_type_define` None→Some in 1.10 (`Money_Camp_*`, `Contribution_*`, `Money_Pearl`, `Cog`, `Item_Pinball_Coin`, …), matching the 1.10 patch notes (contribution manager reorg, new camp/contribution currencies).
- `src/item_info/structs.rs` (`UnitData::unk_post_icon_path`, 1.10): both of Money_Copper's `MoneyUnitEntry`s gained +4 B at the same UnitData-relative position; net item delta +4 = −4 (money_icon_path removal) + 2 × 4.
- `src/item_info/item.rs` (ItemInfo header, 1.08): `is_equip_quick_slot_visible` read 5,365 zero / 949 one across 6,314 items; `value = 1 ⇒ equip_type_info != 0` holds, the converse fails on 2,155 items (quest-flavoured equipment). The 949 are every weapon, armour piece, accessory and tool-slot tool; cosmetic/plot items sharing `item_type=58 cat=601` with the real tools (Notepad, Feather_Pen, Item_Scale, Abacus, Equip_Drum, Equip_Trumpet) read 0. `DockingChildData::unk_post_summon_tag` appeared on the 385 items with `docking_child_data.tag = 1`, always 0x00. Net per-item delta −3 B (tag 0) / −2 B (tag 1); confirmed byte-perfect on synthetic 1.08 rows for Pyeonjeon_Arrow 2200, Goblin_Pot 52006, Marni_Devotee_PlateArmor_Helm 14510, KuKu_Lightning_TwoHandSpear 1002175.
- `src/item_info/structs.rs` (`EnchantDataList`, 1.12): the separator was pinned on key 1000019's six enchant rows: +4 only at the [1]…[5] boundaries (offsets 417/495/573/651/729), never before [0] (339).
- `src/item_info/item.rs` (ItemInfo header, 1.16): across all 6,581 items every one of the 9 × 6,581 `inventory_info_list` values fell in {1, 2, 3, 5, 6, 7, 8, 9, 10, 13, 14, 255}, 14 distinct tuples; that is what pinned the width at 9 rather than 8 + a separate tail.
- `src/item_info/structs.rs` (`UnkPreRespawnData`, 1.16): 14 of 6,581 items carried a non-empty list (5 with one element, 9 with two), 23 elements in all; `unk_b == unk_c` and `unk_d == unk_c + 1` on 23/23; the high half of each u64 is 0 on all 23.
- `src/item_info/structs.rs` (`MergedPrefabVisualData::unk_pre_is_craft_material`): `0xeac5e173` on all 12,550 elements of all 6,810 items in 2.00 (12,274 / 6,573 in 1.18, which the doc has).
- `src/item_info/item.rs` (ItemInfo header + field comments, 2.04): fields removed with no byte left: `quick_slot_index`, `consumable_type_list`, `unk_pre_pattern_key`, `unk_pre_pattern_flag`, `unk_pre_respawn_flag`, `unk_post_respawn_data_list` (a structural walk of all 6,805 carried-over rows ends on each row's last byte without them). Added: `docking_child_data_list`, `EnchantData::unk_post_equip_buffs`, two trailing f32s on `InspectData`, `DockingChildData`'s three `unk_*` fields, two more `inventory_info_list` slots; `unk_pre_respawn_data_list` moved into the optional `unk_pre_respawn_block`. Misreadings exposed: `packed_item_info` read 1 on 1,486 items where it is really `unk_pre_packed_item_flag` (real keys read shifted, `00 fd 48 0f`); `unk_pre_pattern_key` was the top byte of `convert_item_info_by_drop_npc`; `is_packed_in_vehicle` was the top byte of the 4-byte `unk_pre_max_endurance` (0x01000000 on the 59 trade items). `item_importance_kind` reads 0 on the 654 items the old `is_important_item` marked, 2 on the rest, 1 once. `docking_child_data_list` is non-empty only on the eight `Item_wagon_wheel_*` items (four sockets each). `UnkPreRespawnBlock` on 197 of 1,552 DLC rows: all `unk_a` 1, `unk_flag` 0, `unk_b` 1..=4.

### Other parsers (1)

- `src/character_info/mod.rs` (module doc, 2.06 row changes): the two zero bytes that moved FactionKey 149 -> 151 sit somewhere in bytes 87-91 after the key (an all-zero run on every row; bytes 85-86 match both versions; `Riding_Big_pirateship_1`, the one row with a non-zero u32 at 91, shows it moved too); 7,313 of 7,315 rows match a two-byte insert at byte 95. Separately, 971 rows lost four zero bytes later in the row on 2.06 (860 have no other change; Kliff's sit just before `31 00 00 00 80 96 98 00`); not explained. Inlined conditions add 39-301 bytes to 2,500+ rows (+1.43 MB); old element was 13 bytes `[u32 CondKey A][u32 CondKey B][u8 op][u32 hash]` (example `NGW_Citizen_13083`).
- `src/character_info/mod.rs` (module doc / FactionKey): FactionKey pinned on all 7,315 rows of 2.04, 2.04.08, 2.05, 2.05.01, 2.06; same per character across all five; 2,303 rows non-zero. `NHM_Hernand_Soldiers_*` = 1000131 (Faction_Celeste_Hernand), Kliff/Kliff_Clone/Damian = 1000000 (Faction_Graymane), `NHM_Bandit_*` = 1000128 (Faction_Bandit).
- `src/character_info/mod.rs` (module doc): 359 of 7,315 indexed rows (2.03 and 2.04 alike) carry a hashed 32-bit key; row 24-bit keys top out near 1.7M. The anchor scan's all-digit filter had zero observed collateral; body strings followed by a small number still pass the scan (154 false anchors on 2.04, none on a real row's key).
- `src/faction_node_info/mod.rs` (module doc): 1,014 of the 1,120 factionnode rows name a knowledge row that is exactly `"Knowledge_" +` the node name; the other 106 differ. The 576 nodes whose knowledge sits in a `KnowledgeGroup_Topography_<Region>_<Area>` group span Hernand, Demeniss, Delesyian, CrimsonDesert and Kweiden.
- `src/part_prefab_dye_texture_pallete_info/mod.rs` (live test comment): from 1.13 key=0 carries the same cloth/leather/metal triple as keys 1..=10, each sub on `cd_texturelayer_dyetexture_white.dds`; through 1.12 it had 2 subs whose sub[0] icon_path fell back to its texture_path (`docs/dye-editor-scope.md` still describes the 1.12 shape: `sub_count` "2 for key=0").
- `src/faction_info/mod.rs` (module doc): on 2.06, 128 of 135 faction rows have a group; the 7 with group 0 are `Faction_Empty_*`, `Faction_BountyHunter`, `Faction_CrowBrothers`, `Faction_Ancient_People`, `Faction_Golems`, `Faction_Hexe_Sanctuary` (count in c-abi-surface.md, names not). The `[u8; 3]` after the key is zero on 130 of 135 rows.

### Other parsers (2)

- `src/skill_info/mod.rs` (module doc): the IDA addresses of the decompiled readers were `SkillInfo::readEntryFields` = `sub_1410F8680` and the `BuffData` factory = `sub_1419D8660` (game binary of the 1.03-1.05 era).
- `src/skill_info/post_buff.rs` (`EntryHead::unk_post_key`): through 2.03 this byte sat right after the skill name and was taken for a NUL terminator (the "BString-shape" name); 2.04 moved it into the entry head. Reads 0 on every entry.
- `src/skill_info/post_buff.rs` (`PostBuff::unk_pre_damage_type`): the 1.16 tandem byte-walk against 1.15 put the new byte on the `damage_type` boundary in 796 of 800 sampled entries.
- `src/string_info/mod.rs` (module doc): of the 30,313 hashes present in both 2.03 and 2.04, every stringinfo row is byte-identical after undoing the 2.04 key/name swap; layout otherwise unchanged since 1.05.
- `src/string_info/pabgh.rs` (module doc): 1.06 stringinfo.pabgh had 30,206 entries (u16 count).
- `src/gimmick_info/mod.rs` (module doc): on 2.04 the key and name rules alone matched 22,484 anchors against 14,070 real rows (14,210 indexed); two of the noise anchors took a real row's key (`InspectSocket_0` + the key of the gimmick it sockets) ahead of that row. Gimmick rows with a non-zero cat-byte were about 12% of rows (1.06). The 2.06 header lists 140 rows the scanner rules drop.
- `src/global_game_event_info/mod.rs` (test `global_game_event_info_body_fields_live`): globalgameeventgroup had 7 groups in 1.07; 1.08 added 0x424a–0x424e (the `FactionBlockEvent_*` groups), making 12. paloc_key absent on 24/103 rows in 1.07 and on 27 rows since 2.00.

### Other parsers (3)

- `src/store_info/mod.rs` (`_probe_store_variant_distribution`): 1.07 row counts per `(field_e, format_tag)` variant of the pre-2.04 body (2026-05-17 baseline): `field_e=0, format_tag=0x01FD/0x03FC/0x83FC` 181 rows (standard family); `field_e=1, 0x01FD` 10 rows (Trade); `field_e=2, 0x01FD` 9 rows (Short-tail, 15-byte tail); `field_e=0, 0x0600/0x83FC` 6 rows (Camp anomaly + leather); `field_e in {14, 1536, 1537, 1538}` 86 rows. Only the 86 is in docs/save-editor-keys-plan.md.
- `src/stage_info/mod.rs` (module doc): stageinfo was 26 MB with ~57k row anchors on 1.06; one save's `_stageStateData` had 46,541 instances. The 1.08 anchor scan returned 49,634 of the 51,037 `.pabgh` rows (1,403 `LevelSequencerSpawn_UUID_…` names dropped; the 1,403 is in docs/code-audit-2026-11-28-d.md, the totals are not).
- `src/mission_info/mod.rs` (test comment): the pre-UTF-8-check `from_utf8_lossy` scan let 791 of 4,939 mission rows (16%) through with U+FFFD in the name (CrimsonAtomtic issue 001).

## Scripts and the Python stub

- `python/crimson_rs/__init__.pyi` (ItemInfo.is_packed_in_vehicle): through 2.03 this u8 was read as the top byte of the 4-byte `unk_pre_max_endurance` (which read 0x01000000 on the Trade_*_PackedInVehicle items); 2.04 split it out. Partly in the item.rs "ItemInfo (2.00)" header, not stated as such in validated-patches.
- `python/crimson_rs/__init__.pyi` (ItemInfo.unk_pre_packed_item_flag): through 2.03 this u8 was misread as `packed_item_info == 1` (validated-patches 2.04 row says only "a byte out of step").
