use super::keys::*;
use super::structs::*;
use crate::binary::*;
use crate::py_binary_struct;

// ── ItemInfo (1.18) ─────────────────────────────────────────────────────────
//
// Crimson Desert 1.18 makes exactly ONE layout change relative to 1.17: every
// `MergedPrefabVisualData` element gained a `u32` (`unk_pre_is_craft_material`)
// between `tribe_gender_list` and the 3-byte flag tail. +1 net item
// (6,572 → 6,573, key 1005446 `Demian_Greyfur_Fabric_Cloak_II`); iteminfo
// 6,139,734 → 6,190,316 B (+50,582). RE'd via `scripts/diff_117_118.py`
// (a clone of `diff_115_116.py`).
//
// Why the single-field read is safe:
//
//   * The insert lands at the *start* of `is_craft_material` in the 1.17 span
//     map, i.e. after `tribe_gender_list` and before the flags — had it been
//     appended after `unk_post_flag` the walk would have reported it against
//     the following field instead, since the 3 flag bytes (values 0..=3) never
//     match `73 e1 c5`.
//   * The per-item insert count equals that item's 1.17 merged-element count
//     on 5,794 of the 5,797 fully-realigned items (the 3 stragglers are items
//     whose merged count itself changed), so the field is unconditional — not
//     gated on a sibling the way 1.12's `unk_pre_gimmick_visual` is.
//   * All 10,415 inserts observed in the walk carry the *same* value,
//     `73 e1 c5 ea` (LE u32 `0xeac5e173`) — the "empty string" Jenkins
//     sentinel already documented under 1.10 change #1 below.
//   * Per-item size deltas are +4 × (merged element count): +0 (822 items),
//     +4 (198), +8 (4,679), +12 (750), +16 (36), +20 (44), +24 (20). Nothing
//     else moved.
//
// Two other length-changing signatures in the report are walk artifacts, NOT
// drifts — each is one half of a compensating pair produced by 1.18 reordering
// the `item_group_info_list` u16s (a pure value churn that dominates the diff):
// `look_detail_mission_info rm:1B` (93×) pairs with `item_group_info_list[2]
// rm:11B ins:12B` (54×) + `rm:9B ins:10B` (39×), and
// `enable_alert_system_to_ui ins:1B` (5×) pairs with `item_group_info_list[2]
// rm:6B ins:5B` (5×). Both net to zero bytes per item.
//
// ── ItemInfo (1.17) ─────────────────────────────────────────────────────────
//
// Content-only over 1.16 — no layout change, no parser edit. 6,581 → 6,572
// items: nine `Item_Set_*_Tier0_Reminiscence` entries (keys 1004912–1004920)
// were removed and none added. iteminfo.pabgb 6,145,386 → 6,139,734 B, and the
// −5,652 B is *exactly* the sum of those nine items' spans, so nothing else
// moved. Of the 6,572 surviving items 6,435 are byte-identical to their 1.16
// bytes and 137 changed values only — **zero** changed size, which is the check
// that rules out a layout drift hiding behind a compensating value change.
//
// ── ItemInfo (1.16) ─────────────────────────────────────────────────────────
//
// Crimson Desert 1.16 makes FOUR layout changes relative to 1.13–1.15 (1.14 and
// 1.15 were content-only). +73 net items (6,508 → 6,581, none removed);
// iteminfo.pabgb 5,938,891 → 6,145,386 B (+206,495). RE'd via the lightweight
// tandem byte-walk vs the kept 1.15 binary (`scripts/diff_115_116.py`, cloned
// from `diff_112_113.py`) plus per-site decodes over all common items. The net
// per-item delta is **+24 B** on 6,085 of the 6,508 common items
// (−2 −1(cond) +10 +16). Full `serialize_iteminfo` round-trip is byte-identical
// on the live binary (in == out, 6,581 items, 0 skipped).
//
// The changes:
//
//   1. `inventory_info: InventoryKey` (u16) was **removed from the item head**
//      (it sat between `broken_item_prefix_string` and `equip_type_info`) and
//      relocated to the item end — see change 4.
//
//   2. `DockingChildData::unk_post_summon_tag: u8` — the trailing byte 1.08
//      added — was **removed** (structs.rs). Conditional drift: it only shows
//      on the 391 items with `docking_child_data.__tag__ != 0`, and that
//      discriminator partitions the table perfectly (fp=0, fn=0).
//
//   3. A **10 + 28×N byte block** was inserted immediately before
//      `unk_pre_max_endurance` / `respawn_time_seconds`, and those two fields
//      **swapped order** (1.15 read `respawn_time_seconds` then
//      `unk_pre_max_endurance`; 1.16 reads the u32 first). The block is
//      `u32 + u8 flag + CArray<UnkPreRespawnData> + u8`, so it costs a flat
//      10 B on the 6,567 items whose list is empty — which is why it first
//      looked like a fixed 10-byte insert. Only 14 items carry elements
//      (5 with one, 9 with two).
//
//      The swap is what makes `respawn_time_seconds` decode sanely: with it,
//      the field reads 0 (5,831 items), −1 (748) or 604,800 (2 — exactly 7
//      days in seconds) and `unk_pre_max_endurance` stays 0 as it has since
//      1.12; without it both fields decode to nonsense (−4294967296 and
//      0xffffffff). Byte-wise the swap is indistinguishable from "1.16 deleted
//      the old u32 and added a new always-zero u32 here", so the round-trip
//      alone does not settle it — the value distributions do.
//
//   4. The removed `inventory_info` **reappears at the item END, widened to
//      nine slots** (`inventory_info_list: [u16; 9]`, 18 B), replacing the
//      1.13-era 2-byte `unk_tail`. Slot 0 equals the old head-side
//      `inventory_info` on **all 6,054 cleanly-diffable items, zero
//      exceptions** — that equality is what identifies this as a relocation
//      rather than an unrelated new block. Slots hold the same small
//      InventoryKey domain (1, 2, 3, 5, 6, 7, 8, 9, 10, 12, 13, 14) with 0xFF
//      as the unused-slot sentinel; 14 distinct tuples across the table.
//
//      Slot 8 is the u16 that 1.13–1.15 carried as the constant `unk_tail`
//      (0x00ff on all 6,508 items). It is folded into the array rather than
//      kept separate because in 1.16 it reads 6 — not 0xFF — on exactly the
//      59 `Trade_*_PackedInVehicle` items, which are also exactly the items
//      whose slot 7 reads 7 instead of 0xFF (and the only ones whose
//      `unk_pre_max_endurance` is non-zero). That perfect three-way
//      correlation, plus the shared 0xFF sentinel, makes "slot 8 of the same
//      array, unused on every pre-1.16 item" a much better fit than
//      "unrelated constant that coincidentally changed on the same 59 items".
//
//      Alignment check: across all 6,581 items every one of the 9 × 6,581
//      slot values falls in {1, 2, 3, 5, 6, 7, 8, 9, 10, 13, 14, 255} — no
//      out-of-domain u16 anywhere. A mis-sized or mis-positioned array would
//      produce garbage here, so this is what pins the width at 9 rather than
//      8 + a separate tail.
//
// Downstream note: the `crimson_iteminfo_lookup_inventory_info` C ABI is
// unchanged and now sources `inventory_info_list[0]`, which is byte-for-byte
// the value it returned before 1.16.
//
// No save-body drift from the iteminfo side. 1.16 does introduce a new save
// format (slot108) — tracked separately.
//
// ── ItemInfo (1.13) ─────────────────────────────────────────────────────────
//
// Crimson Desert 1.13 makes the single largest structural change yet: it
// **relocates and merges** the prefab/gimmick-visual block. +25 net items
// (6,483 → 6,508); iteminfo.pabgb 5,754,919 → 5,938,891 B (+183,972). RE'd via
// the tandem/opcodes byte-walk vs the kept 1.12 binary
// (`gamedata-bin/1.12/iteminfo.pabgb`) + the "fix head/middle, read the parser's
// per-item leftover" technique (`scripts/decode_tail_113.py`). Full
// `serialize_iteminfo` round-trip is byte-identical on the live binary
// (in == out, all 6,508 items). The changes:
//
//   1. `SubItem` gained `type_id == 17` — a new payload-free variant (items
//      that read tag 16 in 1.12 now read 17). Joins the None arm; affects both
//      SubItem sites (`drop_default_data.default_sub_item` + item-level
//      `default_sub_item`).
//
//   2. `prefab_data_list` and `gimmick_visual_prefab_data_list` — which sat in
//      the item **middle** (right after `drop_default_data`) in 1.05–1.12 — were
//      **merged into a single list and relocated to the item END** (after
//      `repair_data_list`), modeled by `MergedPrefabVisualData` (structs.rs).
//      The merged element interleaves both source structs' fields: a `scale`
//      (`(1,1,1)` for prefab-origin rows), the prefab/animation name lists, the
//      equip-slot + tribe-gender lists, and a 3-byte flag tail. The merged
//      count equals the old prefab + gimmick element totals. `enchant_data_list`
//      and the equip/gem-gated `unk_pre_gimmick_visual` **stay** in the middle
//      (in their 1.12 order) — only the two prefab/gimmick lists moved.
//
//   3. A constant 2-byte item tail (`0xff, 0x00`; `unk_tail: u16`) now closes
//      every item, after the relocated merged list.
//
// Discriminator note: what first looked like an "equipment-gated +14 head
// block" was a mis-read — the bytes after `drop_default_data` are just
// `enchant_data_list` (whose count is large for equipment) followed by the
// gated `unk_pre_gimmick_visual`; both are unchanged from 1.12. The only moved
// fields are the two prefab/gimmick lists.
//
// No save-body drift in 1.13 (slot107 = the live 1.13 save: hmac_ok, body
// decode undecoded_bytes=0, all 12 live saves full-body round-trip + idempotent;
// format unchanged v2 / flags 0x0080). Gamedata bridge tables: all 30 PABGH key
// lists still auto-detect; `partprefabdyeslotinfo` grew +570 rows AND refined
// its per-slot record schema (the 1.12 `(0xFF,0)` 5-byte pad is really
// `u8 marker + u32 extra_layer_count`; 1.13's new dyeable gear sets count=1,
// adding a second dye layer). Fixed in `src/part_prefab_dye_slot_info/`.
//
// ── ItemInfo (1.12) ─────────────────────────────────────────────────────────
//
// Crimson Desert 1.12 makes FOUR layout changes relative to 1.11 (the largest
// schema drift since 1.08). +150 net items (6,333 → 6,483). RE'd with the
// lightweight tandem byte-walk vs the kept 1.11 binary
// (`gamedata-bin/1.11/iteminfo.pabgb`, 5,543,339 B) — `scripts/diff_111_112.py`
// + per-boundary byte dumps. Result after all four edits:
// `parser status: ok=6,483  leftover=0  fail=0  no_anchor=0`, and a full
// `serialize_iteminfo` round-trip on the live binary is byte-identical
// (5,754,919 B in == out). The four drifts:
//
//   1. `SubItem` (structs.rs) gained `type_id == 16` — a new payload-free
//      variant. Items that read tag 15 (None) in 1.11 now read 16; verified
//      byte-aligned (no payload, like 14/15). Affects both SubItem sites
//      (`drop_default_data.default_sub_item` and the item-level
//      `default_sub_item`). 4,496 items flipped 15→16.
//
//   2. A new `u32` (`unk_pre_max_endurance`, always 0) was inserted between
//      `respawn_time_seconds` and `max_endurance`. **Unconditional** (every
//      item). 4,496 items show it cleanly (the rest carry content drift on
//      top).
//
//   3. A new `u32` (`unk_pre_gimmick_visual`, always 0) was inserted between
//      `enchant_data_list` and `gimmick_visual_prefab_data_list`, but only for
//      **equipment** (`equip_type_info != 0`) and **gems** (`item_type == 74`
//      / `category_info == 2501`). The discriminator partitions all 4,496
//      cleanly-diffable common items perfectly (fp=0 fn=0; the 15 gem
//      exceptions are exactly `item_type==74`). Modeled as a conditional field
//      via the `=> <cond>` clause newly added to `py_binary_struct!`
//      (read-gated, stored `Option<u32>`; write/to_py drive off Option
//      presence — see `src/binary/mod.rs`). This is the first conditional
//      field gated on a *sibling value* rather than a `COptional`/`CArray`
//      length, hence the macro work.
//
//   4. `enchant_data_list` is no longer `CArray<EnchantData>`: 1.12 inserts a
//      `u32` (always 0) **between** consecutive `EnchantData` elements — N
//      elements carry N-1 separators (none before [0], none after the last).
//      Confirmed by a per-boundary byte-walk on key=1000019's 6 enchant rows
//      (the +4 appears only at the [1]…[5] boundaries, never before [0]).
//      Modeled by the custom `EnchantDataList` type (structs.rs); this is why
//      single-enchant items round-tripped fine under the 3-fix parser while
//      multi-enchant items failed at `enchant_data_list[1].max_stat_list`.
//
// No save-body drift in 1.12 (slot107 / the live 1.12 save: hmac_ok, body
// decode undecoded_bytes=0, body-stable write round-trip; format unchanged
// v2 / flags 0x0080).
//
// ── ItemInfo (1.11) ─────────────────────────────────────────────────────────
//
// Crimson Desert 1.11 makes a single layout change relative to 1.10: a new
// boolean `u8` field (`unk_post_apply_drop_stat_type`) was **inserted**
// between `apply_drop_stat_type` and `drop_default_data`. Every item gains
// exactly one byte (net per-item delta +1 on 6,321 / 6,325 items common to
// 1.10 and 1.11; the remaining 4 carry additional content-only drifts on top
// of the +1). On the cleanly-isolatable subset the value reads 0 or 1 only
// (4,861 one, 266 zero) — a boolean whose semantic role / gamedata symbol is
// still unknown; named for its position pending RE.
//
// RE workflow (lightweight tandem byte-walk, no sibling parser): the current
// parser parses the kept real-1.10 binary (`gamedata-bin/1.10/iteminfo.pabgb`,
// 5,532,062 B) at ok=6,325 leftover=0 but fails on every 1.11 item at
// `prefab_data_list[0].prefab_names`. A per-item tandem walk between the two
// binaries (matched by key) found one inserted byte at the `drop_default_data`
// boundary for all common items. Result after this edit applied:
// `parser status: ok=6,333  leftover=0  fail=0  no_anchor=0`.
// (Heads-up: the 1.10.01 hotfix install is NOT a real 1.10 — its 0008
// container is byte-identical to the 1.11 D: install; use the kept
// `gamedata-bin/1.10/iteminfo.pabgb` baseline for cross-version diffs. These
// per-version baselines are gitignored game content kept in a portable local
// archive — drive letter irrelevant, copy the folder wherever it's needed.)
//
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
        // 1.16: `inventory_info: InventoryKey` used to sit here. It moved to
        // the item end and widened to eight slots — see `inventory_info_list`
        // and the "ItemInfo (1.16)" header note above.
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
        // 1.11 inserted a new boolean u8 here (between `apply_drop_stat_type`
        // and `drop_default_data`). Reads 0/1 only; semantic role unknown.
        // See the "ItemInfo (1.11)" header note above.
        pub unk_post_apply_drop_stat_type: u8,
        pub drop_default_data: DropDefaultData,
        // 1.13 RE: prefab_data_list and gimmick_visual_prefab_data_list both
        // relocated to the item END (merged into one list; see the placeholder
        // removed after repair_data_list — the parser's leftover exposes it).
        // enchant_data_list and the equip/gem-gated unk_pre_gimmick_visual STAY
        // here in the middle, in their 1.12 order.
        pub enchant_data_list: EnchantDataList,
        pub unk_pre_gimmick_visual: u32 => equip_type_info.0 != 0 || item_type == 74,
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
        // 1.16 inserted a 10 + 28×N byte block here. On 6,567 of 6,581 items
        // the list is empty, so it costs a flat 10 B. Value distributions:
        //   unk_pre_respawn_a         0 on 6,545 items; on the other 36 it
        //                             holds an ItemKey-shaped value (1002011…
        //                             1002020) — all of them `Wood_Branch_*`
        //                             gathering-node tiers, which is why it is
        //                             NOT documented as a constant.
        //   unk_pre_respawn_flag      1 on 17 items, 0 on the rest. Tracks
        //                             "list non-empty" on 14 of those 17; the
        //                             3 exceptions (Sidmon/Silien/Leinstead
        //                             OneHandSword) set the flag with an empty
        //                             list, so it is read unconditionally
        //                             rather than as a COptional tag.
        //   unk_post_respawn_...      0 / 1 / 2 / 3.
        // See the "ItemInfo (1.16)" header above.
        pub unk_pre_respawn_a: u32,
        pub unk_pre_respawn_flag: u8,
        pub unk_pre_respawn_data_list: CArray<UnkPreRespawnData>,
        pub unk_post_respawn_data_list: u8,
        // 1.12 inserted this u32 (0 on every item through 1.15; in 1.16 it
        // reads 0x01000000 on the 59 `Trade_*_PackedInVehicle` items and 0
        // everywhere else). 1.16 also moved it from
        // *after* `respawn_time_seconds` to *before* it. Byte-wise the move is
        // indistinguishable from "1.16 deleted the old field and added a new
        // always-zero u32 here", but the swap reading is what makes
        // `respawn_time_seconds` decode sanely (0 / -1 / 604800 = 7 days)
        // instead of the nonsense values the un-swapped order produces.
        pub unk_pre_max_endurance: u32,
        pub respawn_time_seconds: i64,
        pub max_endurance: u16,
        pub repair_data_list: CArray<RepairData>,
        // 1.13: the merged prefab + gimmick-visual list, relocated here from the
        // item middle (see MergedPrefabVisualData). Its count equals the 1.12
        // `prefab_data_list` + `gimmick_visual_prefab_data_list` element totals.
        pub merged_prefab_visual_list: CArray<MergedPrefabVisualData>,
        // 1.16: the head-side `inventory_info` relocated here, widened to nine
        // InventoryKey (u16) slots. Slot 0 == the pre-1.16 `inventory_info` on
        // every item; 0xFF marks an unused slot. Slot 8 is the field 1.13–1.15
        // carried as the constant `unk_tail` (`0xff, 0x00`) — see the
        // "ItemInfo (1.16)" header note above for why it is read as part of
        // this array rather than as a separate tail.
        pub inventory_info_list: [u16; 9],
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
