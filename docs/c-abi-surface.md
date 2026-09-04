# C-ABI surface & module map

The detailed catalog of the C-ABI bridges and the per-module source map.
Pulled out of the root `CLAUDE.md` to keep that file minimal — this doc is
the authoritative reference. Update it when adding a bridge or moving a
module.

See also: [`save-editor-keys-plan.md`](save-editor-keys-plan.md) (the active
save-editor workstream, with the "🎯 New session pickup" callout) and
[`save-editor-keys-reference.md`](save-editor-keys-reference.md) (the
regression-baseline ground truth).

## Save Editor key-resolver bridges

**Thirty-four bridges shipped** (as of 2026-05-17): SkillKey, MissionKey,
QuestKey, StageKey, KnowledgeKey, QuestGaugeKey, SubLevelKey, GimmickInfoKey,
**CharacterKey**, **DyeColorGroupInfoKey**, **PartPrefabDyeTexturePalleteKey**,
**PartPrefabKey (dye-slot)**, **FactionNodeKey**, **FactionSpawnDataKey**,
**FactionRelationGroupKey**, **StoreKey**, **MercenaryKey**, **ItemKey → list
of PartPrefabKey** (combined iteminfo+stringinfo+partprefab join), **HouseKey**,
**RoyalSupplyKey**, **CraftToolKey** + **CraftToolGroupKey**, **TriggerRegionKey**,
**GamePlayVariableKey**, **GlobalGameEventInfoKey** + **GlobalGameEventGroupKey**,
**GameAdviceInfoKey** + **GameAdviceGroupKey**, **ReserveSlotKey**, **RegionKey**,
**ItemGroupKey**, **Main quest chapter rollup** (curated `(chapter, arc, mission)`
table sourced from [`ref-gamedata/main-quest-list.md`](ref-gamedata/main-quest-list.md)
— closes the long-deferred chapter rollup since the data was never located in any
gamedata table), **Side quest faction rollup** (curated `(quest, faction)` table
sourced from [`ref-gamedata/side-quest-list.md`](ref-gamedata/side-quest-list.md)
— side quests are organized by faction rather than chapter/arc, so the bridge
exposes both quest→faction and faction→list-of-quests directions) +
`crimson_calculate_checksum` extern "C" wrapper.

Plus a PAZ-layer `crimson_paz_list_npc_portraits` enumerator and a high-level
`crimson_characterinfo_resolve_portrait` matcher that chains CharacterKey →
PALOC display name → fuzzy match → NPC head-shot DDS path.

### Save-handle utilities

- `crimson_save_list_inventory_items` — flat enumeration of every item across
  all 18 `_inventoryList[N]` containers, 48-byte `repr(C)` records keyed by
  `inventory_key` / `item_key` / `item_no`.
- `crimson_save_list_all_items` — cross-container enumerator, 64-byte `repr(C)`
  records covering active equip + active reserve + inventory + mercenary equip
  + mercenary inventory (829 items vs 545 in the inventory-only ABI on the
  slot103 baseline; see [`dye-editor-scope.md`](dye-editor-scope.md)).
- `crimson_save_list_field_positions` — world-map positioned-entity enumerator,
  56-byte `CrimsonPositionedEntityRecord` covering `ACTIVE_CHAR` / `MERCENARY` /
  `GIMMICK`; slot103 baseline 3,317 records (1 active char + 76 mercenaries +
  3,240 position-bearing gimmicks). Each record carries `field_info_key` for
  region filtering, identity (`character_key` / `gimmick_info_key` /
  `mercenary_no`), `pos_x/y/z` global-frame coords, and `yaw`. See
  [`worldmap-plotting.md`](worldmap-plotting.md) + `src/c_abi/positions.rs`.
- `crimson_save_get_mutation_version` — monotonic u64 counter bumped on every
  successful mutation; caller pairs with snapshots for O(1) staleness detection.
  See [`save-mutation-version.md`](save-mutation-version.md).
- **Deferred-redecode batch** (`begin` / `end` / `abort` / `is_open`) —
  suspends the per-call `decode_blocks` for bulk-mutation workflows; one decode
  at `end_*`. See [`save-deferred-redecode.md`](save-deferred-redecode.md).
- `crimson_save_set_object_list_present` — toggles an absent `ObjectList` field
  on / off; make-present auto-materializes count=1 with one default-empty
  element so the round-trip stays byte-unambiguous (closes the "add dye to
  undyed item" path for CrimsonAtomtic's dye editor, see
  [`dye-editor-scope.md`](dye-editor-scope.md) §v2).

### Version / parser-target

- `crimson_paver_read_from_file` / `crimson_paver_read_from_bytes` — decode the
  install's `meta/0.paver` version stamp into `(major, minor, patch, build)`
  (accepts the file path or the install root, auto-appending `meta/0.paver`).
- `crimson_parser_target_gamedata_major() -> u16` — the gamedata `major` this
  build's parsers target (currently **2**). Added for Crimson Desert 2.00, the
  first major bump: the `minor` *resets* across one (1.18 → 2.00 is `18` → `0`),
  so a minor-only check can no longer identify a schema on its own. A sound gate
  compares the install's `(major, minor)` against this and the minor bridge.
  Backs onto `crate::binary::paver::PARSER_TARGET_GAMEDATA_MAJOR`.
- `crimson_parser_target_gamedata_minor() -> u16` — the gamedata `minor` this
  build's parsers target (currently **1**, i.e. 2.01). **Single source of
  truth**: the value lives in `crate::binary::paver::PARSER_TARGET_GAMEDATA_MINOR`,
  so a new patch is one Rust bump and every consumer follows — no more lock-step
  `ParserTargetMinor` edits on the C# side (promoting this killed the 5th such
  manual bump, 8→9→10→11→12). Infallible direct return (these two are the only
  non-`i32` surfaces — a pure compile-time constant has no error path).
- `crimson_parser_compatible_gamedata_minors(out_buf, cap, out_count) -> i32` —
  the allow-list of gamedata minors this build can load (first-call sizing:
  `null`/`cap=0` → `BUFFER_TOO_SMALL` with `out_count`; refill at that size →
  `OK`). Drives the consumer's `CompatibleMinors` / `IsCompatibleWithParser`;
  the target minor is always present in the set. Both back onto
  `crate::binary::paver::COMPATIBLE_GAMEDATA_MINORS`. Entries are minors
  *within* the target major — read them alongside
  `crimson_parser_target_gamedata_major()`.

### Status & remaining work

334 tests pass with `c_abi` (+45 ignored diagnostic probes); the bare-default
build (`cargo check --lib`) is also clean; clippy clean both modes. The Jenkins
hash-hop transform that drives Mission/Quest/Stage/Knowledge title resolution is
cracked and pinned. CharacterKey ships at the 22% display-name coverage §6
predicted plus a full-coverage `lookup_string_key` internal-name fallback. The
three dye gamedata bridges (2026-05-16) replace the PyQt5 `dye_slot_counts.json`
with byte-perfect gamedata-driven slot counts + per-slot material defaults +
named color-group + palette-tier resolvers — see
[`dye-editor-scope.md`](dye-editor-scope.md); the `_itemKey → _partPrefabKey`
linkage is bridged through `crimson_item_part_prefab_*`.

Remaining work is **optional follow-ons only**:
(a) **`CharacterAppearanceIndexKey`** (deferred 2026-05-15 — file located,
    PABGH/PABGB schema pinned, save→pabgh transform verified, but only 7% of
    sample save values hit the table and the 21-byte body is opaque; §9 of the
    plan doc);
(b) knowledge group breadcrumb;
(c) ~~quest chapter rollup~~ — **shipped 2026-05-17** as a curated static table;
(d) broader CharacterKey PALOC namespaces;
(e) extending the portrait matcher with mesh / customisation tokens;
(f) PALOC chain probe for StoreKey / MercenaryKey if a downstream editor wants
    localized display names;
(g) RE'ing storeinfo's per-row body (price/item lists);
(h) RE'ing **which** of 2.01's four 3-byte mask groups a dye slot uses — the
    bytes are exposed (`..._lookup_slot_mask_full`) but the group selector's
    meaning is not yet known (see [`dye-editor-scope.md`](dye-editor-scope.md)
    "Cross-version drift (2.01)").

## Rust module structure

```
src/
├── lib.rs                   # Crate root (mission/quest/stage/knowledge/questgauge/
│                            #   sub_level/gimmick/character modules gated behind #[cfg(feature = "c_abi")])
├── python.rs                # PyO3 bindings
├── binary/                  # Container formats
│   ├── pamt.rs, papgt.rs    # PAMT / PAPGT parse + write
│   ├── paz.rs               # PackGroupBuilder, compression, PAZ creation
│   ├── paloc.rs             # PALOC parse/write (numeric + symbolic keys)
│   ├── gamedata_layout.rs   # #[cfg(test)] — which archive layout the live install
│   │                        #   ships (2.01 renamed the gamedata dir + every
│   │                        #   extension); newest-first with fallback
│   ├── paver.rs             # meta/0.paver version-stamp reader
│   └── trie.rs              # Trie buffer read + build (radix-compressed)
├── crypto/
│   ├── checksum.rs          # Jenkins hashlittle2 (init = 0xDEBA1DCD)
│   └── chacha20.rs          # ChaCha20 encrypt/decrypt
├── item_info/               # iteminfo.pabgb parser (byte-roundtrip)
├── skill_info/              # skill.pabgb + skill.pabgh parser (byte-roundtrip,
│                            #   BuffData tail-size brute-force probe)
├── string_info/             # u32 hash → string bridge (PAS string tables)
├── character_info/          # characterinfo.pabgb anchor-scan (24-bit row keys)
├── save/                    # Save file format (header + crypto + body)
└── c_abi/                   # extern "C" surface (feature-gated)
    ├── all_items.rs        # crimson_save_list_all_items — cross-container item enumerator
    │                       #   (ActiveEquip/ActiveUseReserve/Inventory/MercenaryEquip/MercenaryInventory)
    ├── checksum.rs          # crimson_calculate_checksum (Jenkins hash hop helper)
    ├── paver.rs             # crimson_paver_read_from_file/_bytes (game-install version stamp)
    │                         # + crimson_parser_target_gamedata_major/_minor, _compatible_gamedata_minors
    ├── iteminfo.rs          # ItemKey → string_key / max_stack / icon_path_hash /
                             #   look_detail_mission_info (item→challenge) +
                             #   artifact_for_mission (challenge→artifact reverse, 1:1) /
                             #   socket_caps (use_socket + valid_count) +
                             #   socket_allows_gem + socket_allowed_gem_count/_at
                             #   (per-weapon vendor-list cross-checks) +
                             #   canonical_gem_count/_at (full 190-item gem set
                             #   classified by item_type=74 + category=2501; the
                             #   gem-picker dropdown source for CrimsonAtomtic).
                             #   All cross-checks are advisory — never block, just
                             #   inform; CE-modified saves still load.
    ├── skill_info.rs        # SkillKey → entry name
    ├── paloc.rs             # PALOC handle + length-prefixed lookup
    ├── string_info.rs       # u32 hash → string
    ├── paz.rs               # crimson_paz_extract_file + list_npc_portraits +
    │                        #   crimson_paz_list_dir (272-byte CrimsonPazFileEntry,
    │                        #   the worldmap-tile discovery surface — feeds the
    │                        #   editor's basemap caching loop).
    ├── positions.rs         # crimson_save_list_field_positions — world-map positioned-entity
    │                        #   enumerator (ACTIVE_CHAR / MERCENARY / GIMMICK kinds).
    │                        #   56-byte repr(C) records with pos_x/y/z (global frame),
    │                        #   yaw, field_info_key, character_key / gimmick_info_key,
    │                        #   mercenary_no. Slot103 baseline: 3,317 records. See
    │                        #   worldmap-plotting.md for the affine fit.
    ├── mission_info.rs      # MissionKey → name + display title (hash hop)
    ├── quest_info.rs        # QuestKey → name + arc heading (hash hop, lo32=0x100)
    ├── stage_info.rs        # StageKey → name + title (hash hop, lo32=0x101/0x102)
    ├── knowledge_info.rs    # KnowledgeKey → name + title (hash hop, lo32=0x490)
    ├── quest_gauge_info.rs  # QuestGaugeKey → name only (no PALOC chain)
    ├── sub_level_info.rs    # SubLevelKey → name only (no PALOC chain)
    ├── gimmick_info.rs      # GimmickInfoKey → name + display label (PALOC, NO hash hop, lo32=0x200)
    ├── character_info.rs    # CharacterKey → name + display (cat-byte strip, NO hash hop, lo32=0x30)
    │                        #   + resolve_portrait: fuzzy match against list_npc_portraits output
    ├── dye_color_group_info.rs                # DyeColorGroupInfoKey (u32) → "Her_Color_Group_I" etc.
    ├── part_prefab_dye_texture_pallete_info.rs # PartPrefabDyeTexturePalleteKey (u16) → palette tier (cloth/leather/metal + variants)
    ├── part_prefab_dye_slot_info.rs           # PartPrefabKey (u32) → slot_count + per-slot default materials
    │                                           #   (replaces dye_slot_counts.json); 1.13 adds
    │                                           #   lookup_slot_extra_layer_{count,material,mask,flag} for the
    │                                           #   new second per-slot dye layer ("expanded dyeable equipment");
    │                                           #   2.01 widened the mask 3 → 12 bytes, so lookup_slot_mask and
    │                                           #   ..._extra_layer_mask are partial reads there and
    │                                           #   ..._mask_full / ..._extra_layer_mask_full return the whole
    │                                           #   field via the sized-buffer pattern
    ├── faction_node_info.rs                   # FactionNodeKey (u32) → "Node_Her_Temporary_Camp" etc. (name only, no PALOC chain)
    ├── faction_spawn_data_info.rs             # FactionSpawnDataKey (u32) → "FactionSpawn_GlenbrightManor_Grace_ReedDevil" etc.
    ├── faction_relation_group_info.rs         # FactionRelationGroupKey (u16-widened-u32) → "Graymane"/"FriendlyCombat"/"HostileCombat"/"NPC_Common"/"Monster_Common"
    │                                           #   + lookup_related_count/_at for the per-row sibling-reference matrix
    ├── store_info.rs                          # StoreKey (u16-widened-u32) → "Store_Her_General" etc. (292 rows, name-only, no PALOC chain)
    ├── mercenary_info.rs                      # MercenaryKey (u8-widened-u32) → "Mercenary_Main"/"Vehicle_Horse"/... (18 rows, NEW (u8,u32) PABGH variant)
    ├── item_part_prefab.rs                    # ItemKey → list of PartPrefabKey (3-table join: iteminfo + stringinfo + partprefabdyeslotinfo)
    │                                           #   precomputed at load; lookup_count/_key_at/_prefab_name_at per item +
    │                                           #   resolve_dye_slot_count (v3 convenience wrapper — one-shot
    │                                           #   "how many dye slots does this item have?" chaining both bridges,
    │                                           #   returns (slot_count, resolve_source) with NOT_RESOLVED states
    │                                           #   for the 76% fallback path)
    ├── house_info.rs                          # HouseKey (u16) → "DefaultHouse_Lv1"/... (4 rows, name-only)
    ├── royal_supply_info.rs                   # RoyalSupplyKey (u16) → "RoyalSupply_Hernand"/... (4 rows, name-only)
    ├── craft_tool_info.rs                     # CraftToolKey (u16) → "CraftTool_Enchant"/... (17 rows, name-only)
    ├── craft_tool_group_info.rs               # CraftToolGroupKey (u16) → "CraftTool_Equip_Enchant"/... (10 rows, name-only)
    ├── trigger_region_info.rs                 # TriggerRegionKey (u32) → "Swamp"/"IceTerrain"/... (12 rows, name-only)
    ├── gameplay_variable_info.rs              # GamePlayVariableKey (u32) → "CD_Live"/"BaseCamp_Ranch_Lv1"/... (47 rows, name-only)
    ├── global_game_event_info.rs              # GlobalGameEventInfoKey (u16) → "Drought_Varnian"/... (103 rows).
    │                                           #   Hand-written (not macro) — exposes per-row body fields:
    │                                           #   group_key (100% coverage, cross-refs globalgameeventgroup) +
    │                                           #   paloc_key (76.7% coverage; 0 for RoyalSupply/FactionBlockEvent_*).
    │                                           #   See archive/globalgameevent-body-re.md.
    ├── global_game_event_group_info.rs        # GlobalGameEventGroupKey (u16) → "WeatherEventGroup"/... (7 rows, name-only)
    ├── game_advice_info.rs                    # GameAdviceInfoKey (u32) → "Advice_Control_Move"/... (461 rows, name-only; PALOC chain deferred)
    ├── game_advice_group_info.rs              # GameAdviceGroupKey (u32) → "GameAdviceGroup_ControlBasics"/... (8 rows, name-only)
    ├── reserve_slot_info.rs                   # ReserveSlotKey (u32) → "ArrowItem"/"BulletItem"/"BombItem"/... (27 rows, name-only; PALOC chain deferred)
    ├── region_info.rs                         # RegionKey (u16) → "Region_Pywel"/"Region_Kweiden"/... (1,004 rows, name-only)
    ├── item_group_info.rs                     # ItemGroupKey (u16) → "ItemGroup_Category_Equipment"/... (1,500 rows, name-only)
    ├── main_quest_chapter.rs                  # Curated (chapter, arc, mission) rollup from docs/ref-gamedata/main-quest-list.md
    │                                          #   (~170 rows, static table — no file load, no handle). Lookups:
    │                                          #   chapter_for_arc / chapter_for_mission / arc_for_mission.
    └── side_quest_faction.rs                  # Curated (quest, faction) rollup from docs/ref-gamedata/side-quest-list.md
                                               #   (~84 rows / 22 factions, static table — sibling of main_quest_chapter).
                                               #   Lookups: faction_for_quest (1:1) + quest_count_for_faction /
                                               #   quest_at_for_faction (reverse enumeration).
```

The `impl_name_only_bridge!` macro in `src/c_abi/mod.rs` generates the standard
6-function ABI surface (`load_from_file`/`_bytes`, `free`, `entry_count`,
`lookup_string_key`, `get_entry`) for the niche name-only bridges above. Each
`c_abi/<name>.rs` file becomes a ~15-line macro invocation + a live-install
integration test. The earlier bridges (`store_info`, `mercenary_info`,
`faction_node_info`, …) predate the macro and remain hand-written; they can be
migrated later if a cross-cutting ABI change ever lands.

Lossy anchor-scan parsers used by the c_abi bridges live alongside in
`src/<table>_info/mod.rs`. Each parser is feature-gated to the c_abi bridge that
consumes it.
