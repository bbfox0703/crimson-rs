# crimson-rs — Crimson Desert Archive Toolkit

Rust library with Python bindings (via PyO3/maturin) for reading, writing, and modding Crimson Desert game archives. **Personal-playground fork — not intended to flow back to the `potter420/crimson-rs` upstream.** Targets Crimson Desert 1.05 / 1.06 / 1.07 (verified byte-perfect parse + save roundtrip on the live 1.07 install, 2026-05-15).

# Development
- Build and install: `maturin develop` (in a venv; the project uses `.venv/`)
- Run tests: `cargo test --lib`
- Lint: `cargo clippy --all-targets --lib -- -D warnings` (must pass clean — enforced by CI on every PR and push to main/dev, see `.github/workflows/ci.yml`; `main` is branch-protected on this check)
- Workflow: develop on `dev` → PR to `main` → CI green → merge → `build.yml` rebuilds the rolling-release Windows wheel attached to the `v1.0.<minor>.x` "Development Build" GitHub release (currently `v1.0.7.x`; bump the minor in `build.yml` when Pearl Abyss ships a new patch series)

# Documentation
- `docs/save-editor-keys-plan.md` — **Active workstream as of 2026-05-14.** Roadmap and decisions log for the Save Editor key-resolver bridges (mission/quest/stage/knowledge/questgauge + checksum). Has a "🎯 New session pickup" callout at the top. Read this first if you're continuing the save-editor work.
- `docs/abyss-gate-map.md` — Per-gate abyss-gate mapping + the three pinned `_initStateNameHash` constants. Path B (`CrimsonAtomtic`) input for replacing the bulk "unlock all" UX with per-gate controls. Phase 1 (mapping) complete; Phase 2 (hash → state-name decode) partial — PA's `HashCode32` algorithm needs an IDA pass to finish.
- `docs/paloc-template-survey.md` — Per-namespace template-density survey for the English PALOC. Settles the "do shipped bridges need a template-resolver" question: **no for title namespaces** (`0x30`/`0x70`/`0x100`/`0x490`/etc. — all ≤1% template-bearing), **yes for description namespaces** (`0x71`/`0x491`/`0x49e` — 18–19%, dominated by `{StaticInfo:Type:Key#label}` cross-references and `<br/>` paragraph breaks). Lists the recommended build-out order if the editor expands beyond titles.
- `docs/save-mutation-version.md` — Cache-coherency contract for C ABI consumers that hold snapshot reads (`get_block_json`, `list_inventory_items`, …) across mutations. Documents the `crimson_save_get_mutation_version` counter, the `(snapshot, version)` pairing pattern, and an idiomatic C# wrapper. Read first if you're adding a new mutation entry point.
- `docs/save-deferred-redecode.md` — Transactional "suspend re-decode" batch ABI (`crimson_save_begin_deferred_redecode` / `_end_*` / `_abort_*` / `_is_*_open`). Cuts the per-call `decode_blocks` overhead for bulk-mutation workflows from O(N) re-decodes to one. The motivating consumer is CrimsonAtomtic's "Complete All held sealed abyss artifact challenges" (3 length-changing calls × 141 challenges ≈ 423 re-decodes → 1). Read first if you're adding a multi-mutation editor action.
- `docs/artifact-challenge-mapping.md` — Artifact ↔ Challenge bidirectional mapping. Forward (item→mission) is `look_detail_mission_info`, reverse (mission→item) is the inverse; verified 1:1 across 1.07 (141 items, 141 missions, all named `Challenge_SealedArtifact_*`). Classification of "challenge that needs an artifact" vs "challenge that doesn't" is a string prefix check on the mission's internal name — no enum, no extra ABI. 96.5% of missions start by non-artifact triggers and correctly return `NOT_FOUND` on the reverse lookup.
- `docs/socket-editor-scope.md` — Reference data for `CrimsonAtomtic`'s socket editor (gem insert / remove). Schema verified against slot104 + full-scan across 10 saves / 5,464 socket-bearing items / 27,320 list elements — schema is bulletproof, zero anomalies in the save format. `_socketSaveDataList` is positional + fixed-size (`_maxSocketCount = 5` is a save-side padding constant, NOT the per-item cap — true cap lives in iteminfo's `socket_valid_count`); mask=`[0x00]` empty / `[0x03]` filled; `_currentEndurance = 0xFFFF` sentinel for no-durability gems. ItemSaveData lives in BOTH `InventorySaveData → _inventorylist[N] → _itemList[M]` AND `EquipmentSaveData → _list[N] → _item<Locator child>` — the Locator step is handled transparently by `navigate_mut_to_parent` (pass `element_idx=0`). **No new C ABI required** — existing `set_scalar_field_present` / batch variant handles both absent→insert and gem→absent across both container shapes; byte-perfect round-trip pinned by `c_abi_socket_insert_then_remove_roundtrip_slot104` (inventory) + `c_abi_socket_remove_then_reinsert_roundtrip_equipment_slot104` (equipment-via-Locator). CE-modified gem layouts (user's 嘟嘟鳥 set) bump `_validSocketCount` in lockstep with filled count — the real anomaly is `save._validSocketCount > iteminfo.socket_valid_count`, a gamedata cross-check, not a save-internal one.
- `docs/dye-editor-scope.md` — Reference data for `CrimsonAtomtic`'s Dye editor. Verified 9-field `ItemDyeSaveData` schema (PyQt5 RE had 8 — we found `_disableSymbol`); per-mask-bit field types corrected (`_dyeSlotNo` = i8 not u8; `_texturePalleteKey` = fixed u16). v1 implementation outline using existing C ABI primitives (no new ABI needed for edit-existing-dye scenarios; `set_object_list_present` deferred for "add dye to undyed item"). **As of 2026-05-16**: all three `dye*.pabgb` gamedata tables (`dyecolorgroupinfo`, `partprefabdyetexturepalleteinfo`, `partprefabdyeslotinfo`) RE'd + bridged behind C ABI — replaces `dye_slot_counts.json` and adds per-slot default materials the JSON never carried. Remaining blocker: `_itemKey → _partPrefabKey` cross-reference (lives in `iteminfo.pabgb` or a sibling `partprefab*` table).
- `docs/save-editor-keys-reference.md` — Frozen 1.06 ground-truth comparison set. Verified `(key → internal_name → PALOC title)` mappings + PALOC namespace map + user-supplied wiki/quest ground truth. Use this as the regression baseline when a future patch breaks key resolution.
- `docs/ref-gamedata/main-quest-list.md` — Curated `Chapter → Arc → Mission` breakdown for the main story (Prologue + 12 chapters + Epilogue, ~170 missions). Bold bullets = quest-arc display titles (resolved by `crimson_questinfo_lookup_display_name` at `lo32=0x100`); non-bold bullets = mission display titles (resolved by `crimson_missioninfo_lookup_display_name` at `lo32=0x101`). Chapter headings are curated — never located in any RE'd gamedata table. Source of truth for the `c_abi::main_quest_chapter` bridge.
- `docs/ref-gamedata/side-quest-list.md` — Curated `(quest → faction)` flat list for side quests (~84 quests, 22 factions). Each quest title is a `QuestKey` display title (resolved via `crimson_questinfo_lookup_display_name` at `lo32=0x100` — confirmed against `Record of the Greymanes` etc.); the faction column is the curated rollup. User-supplied list — completeness not guaranteed against shipped game content. Section-header Chinese annotations are informational. Source of truth for the `c_abi::side_quest_faction` bridge.
- `docs/crimsonforge-feature-gaps.md` — What CrimsonForge (Python toolkit) can parse that crimson-rs doesn't yet; useful when extending coverage
- `docs/archive-format.md` — Binary format specs (PAPGT, PAMT, PAZ, trie), mod loading overlay approach
- `docs/api.md` — Full Python API reference (enums, pack_mod, PAPGT/PAMT/PAZ, ItemInfo)
- `docs/1.05-parser-history.md` — Long-form RE history of the 1.05 ItemInfo parser (read first when picking up parser-RE work)
- `docs/historical-parser-setup.md` — Recipe for building a previous-version parser as a sibling install (cross-version diff workflow)
- `docs/paloc-71-dev-items.md` — Why 71 items have no paloc translation in any of 14 languages
- `docs/downstream-api-gaps.md` — Status of the Python bindings the CrimsonGameMods app needs (all three items DONE as of 2026-05; doc kept for the cross-version notes); out-of-scope items recorded explicitly
- `scripts/CLAUDE.md` — Engineering notes for next-session pickup (sanity-check workflow, layout invariants, don'ts, **the "On a new game patch" runbook**)
- Update docs when adding new features or changing binary format understanding

# When picking up the next session
Two active workstreams — read the relevant one first, then proceed:

1. **Save Editor key resolver bridges** (active as of 2026-05-17). **Thirty-four bridges shipped**: SkillKey, MissionKey, QuestKey, StageKey, KnowledgeKey, QuestGaugeKey, SubLevelKey, GimmickInfoKey, **CharacterKey**, **DyeColorGroupInfoKey**, **PartPrefabDyeTexturePalleteKey**, **PartPrefabKey (dye-slot)**, **FactionNodeKey**, **FactionSpawnDataKey**, **FactionRelationGroupKey**, **StoreKey**, **MercenaryKey**, **ItemKey → list of PartPrefabKey** (combined iteminfo+stringinfo+partprefab join), **HouseKey**, **RoyalSupplyKey**, **CraftToolKey** + **CraftToolGroupKey**, **TriggerRegionKey**, **GamePlayVariableKey**, **GlobalGameEventInfoKey** + **GlobalGameEventGroupKey**, **GameAdviceInfoKey** + **GameAdviceGroupKey**, **ReserveSlotKey**, **RegionKey**, **ItemGroupKey**, **Main quest chapter rollup** (curated `(chapter, arc, mission)` table sourced from [`docs/ref-gamedata/main-quest-list.md`](docs/ref-gamedata/main-quest-list.md) — closes the long-deferred chapter rollup since the data was never located in any gamedata table), **Side quest faction rollup** (curated `(quest, faction)` table sourced from [`docs/ref-gamedata/side-quest-list.md`](docs/ref-gamedata/side-quest-list.md) — sibling to the main-quest bridge; side quests are organized by faction rather than chapter/arc, so the bridge exposes both quest→faction and faction→list-of-quests directions) + `crimson_calculate_checksum` extern "C" wrapper. Plus a PAZ-layer `crimson_paz_list_npc_portraits` enumerator and a high-level `crimson_characterinfo_resolve_portrait` matcher that chains CharacterKey → PALOC display name → fuzzy match → NPC head-shot DDS path. Save-handle utilities: `crimson_save_list_inventory_items` (flat enumeration of every item across all 18 `_inventoryList[N]` containers, 48-byte `repr(C)` records keyed by `inventory_key` / `item_key` / `item_no`) + `crimson_save_list_all_items` (cross-container enumerator, 64-byte `repr(C)` records covering active equip + active reserve + inventory + mercenary equip + mercenary inventory — 829 items vs 545 in the inventory-only ABI on the slot103 baseline; see [`docs/dye-editor-scope.md`](docs/dye-editor-scope.md)) + `crimson_save_get_mutation_version` (monotonic u64 counter bumped on every successful mutation — caller pairs with snapshots for O(1) staleness detection; see [`docs/save-mutation-version.md`](docs/save-mutation-version.md)) + the **deferred-redecode batch** (`begin` / `end` / `abort` / `is_open` — suspends the per-call `decode_blocks` for bulk-mutation workflows; one decode at `end_*`, see [`docs/save-deferred-redecode.md`](docs/save-deferred-redecode.md)) + `crimson_save_set_object_list_present` (toggles an absent `ObjectList` field on / off; make-present auto-materializes count=1 with one default-empty element so the round-trip stays byte-unambiguous — closes the "add dye to undyed item" path for CrimsonAtomtic's dye editor, see [`docs/dye-editor-scope.md`](docs/dye-editor-scope.md) §v2). 273 tests with `c_abi`, 76 without (+31 ignored diagnostic probes: portrait-naming cross-version scan, live-save FieldNPC/Gimmick block dump, appearance-file discovery, CharacterAppearanceIndexKey transform validation, abyss-gate per-gate mapping, PALOC template-density survey, InventorySaveData schema dump, ItemDyeSaveData schema dump + dye gamedata file scan, dye-gamedata phase-1 + phase-2 row dumps, ItemSocketSaveData phase-1 dump + phase-2 slot104-wide scan + phase-3 all-slots anomaly hunt + phase-4 anywhere-in-tree walk including EquipmentSaveData, faction file discovery + PABGH-shape probe + small-table row dump + PALOC chain probe, store/mercenary/partprefab file discovery + schema dump, ItemKey↔PartPrefabKey deep-linkage hunt + cross-group .pabgb scan, partprefab string-resolution + Jenkins-checksum hypothesis test); clippy clean both modes. The Jenkins hash hop transform that drives Mission/Quest/Stage/Knowledge title resolution is cracked and pinned. **Start at the "🎯 New session pickup" callout at the top of [`docs/save-editor-keys-plan.md`](docs/save-editor-keys-plan.md).** CharacterKey ships at the 22% display-name coverage §6 predicted plus a full-coverage `lookup_string_key` internal-name fallback. The three dye gamedata bridges (2026-05-16) replace the PyQt5 `dye_slot_counts.json` with byte-perfect gamedata-driven slot counts + per-slot material defaults + named color-group + palette-tier resolvers — see [`docs/dye-editor-scope.md`](docs/dye-editor-scope.md) for the C ABI surface table; the `_itemKey → _partPrefabKey` linkage that completes the dye-editor stack is now bridged through `crimson_item_part_prefab_*`. Remaining work is **optional follow-ons only**: (a) **`CharacterAppearanceIndexKey`** (deferred 2026-05-15 — file located, PABGH/PABGB schema pinned, save→pabgh transform verified, but only 7% of sample save values hit the table and the 21-byte body is opaque; full investigation in §9 of the plan doc); (b) knowledge group breadcrumb; (c) ~~quest chapter rollup~~ — **shipped 2026-05-17** as a curated static table (see [`docs/ref-gamedata/main-quest-list.md`](docs/ref-gamedata/main-quest-list.md) for the source); (d) broader CharacterKey PALOC namespaces; (e) extending the portrait matcher with mesh / customisation tokens; (f) PALOC chain probe for StoreKey / MercenaryKey if a downstream editor wants localized display names; (g) RE'ing storeinfo's per-row body (price/item lists). [`docs/save-editor-keys-reference.md`](docs/save-editor-keys-reference.md) is the regression-baseline ground truth.

2. **New game patch breaks parsing** (the longstanding fallback workstream). Core parsers (iteminfo + skill) are byte-perfect across every version probed; if a patch drifts, follow the diagnostic flow in `scripts/CLAUDE.md` ("On a new game patch"). CI gates the lint+test invariants. The 14 gamedata bridges (mission/quest/stage/knowledge/questgauge/sublevel/gimmick/character + the 3 dye tables + the 3 faction tables) have similar version-drift risk — their tests skip cleanly when the live game install isn't present, so cross-version verification is `cargo test --lib --features c_abi` with the appropriate game install hooked up. The portrait list / matcher also has a `#[ignore]`'d cross-version probe (`_scan_all_groups_for_portrait_like_names` in `src/c_abi/paz.rs`) for spotting Pearl Abyss reorganising NPC asset paths in a new patch.

# Rules
- Ask before adding new dependencies to Cargo.toml
- Binary format must roundtrip (read and write back identically)
- Custom types for binary serialization: `CString` (u32 len + len bytes — NO trailing NUL; this is what `iteminfo.pabgb` uses), `BString`-shape (u32 len + len bytes + u8 NUL terminator — used by `skill.pabgb` entry names + the PAPGT group_names_buffer; no shared Rust type yet, handled inline in `src/skill_info/mod.rs`), `optional<T>` (u8 flag + T if nonzero), `array<T>` (u32 count + count × T), `Variant` (u8 tag + match on tag)

# Binary Exploration
- Use `plcli` + `.hexpat` pattern files for binary format exploration, NOT Python scripts
- Write partial hexpat patterns, test with `plcli run -i <file> -p <pattern.hexpat> -v -d`
- Iterate: write struct, run plcli, check output, refine
- Pattern files go in `references/`

# Reference Files
- `D:\SteamLibrary\steamapps\common\Crimson Desert` — Game installation folder (hardcoded in tests; tests skip cleanly when not present)
- `out/baselines/1.05/items.jsonl` — Per-version reference data (gitignored — keep local)

# Rust Module Structure
```
src/
├── lib.rs                   # Crate root (mission/quest/stage/knowledge/questgauge/
│                            #   sub_level/gimmick/character modules gated behind #[cfg(feature = "c_abi")])
├── python.rs                # PyO3 bindings
├── binary/                  # Container formats
│   ├── pamt.rs, papgt.rs    # PAMT / PAPGT parse + write
│   ├── paz.rs               # PackGroupBuilder, compression, PAZ creation
│   ├── paloc.rs             # PALOC parse/write (numeric + symbolic keys)
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
    ├── paz.rs               # crimson_paz_extract_file + list_npc_portraits
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
    │                                           #   (replaces dye_slot_counts.json)
    ├── faction_node_info.rs                   # FactionNodeKey (u32) → "Node_Her_Temporary_Camp" etc. (name only, no PALOC chain)
    ├── faction_spawn_data_info.rs             # FactionSpawnDataKey (u32) → "FactionSpawn_GlenbrightManor_Grace_ReedDevil" etc.
    ├── faction_relation_group_info.rs         # FactionRelationGroupKey (u16-widened-u32) → "Graymane"/"FriendlyCombat"/"HostileCombat"/"NPC_Common"/"Monster_Common"
    │                                           #   + lookup_related_count/_at for the per-row sibling-reference matrix
    ├── store_info.rs                          # StoreKey (u16-widened-u32) → "Store_Her_General" etc. (292 rows, name-only, no PALOC chain)
    ├── mercenary_info.rs                      # MercenaryKey (u8-widened-u32) → "Mercenary_Main"/"Vehicle_Horse"/... (18 rows, NEW (u8,u32) PABGH variant)
    ├── item_part_prefab.rs                    # ItemKey → list of PartPrefabKey (3-table join: iteminfo + stringinfo + partprefabdyeslotinfo)
    │                                           #   precomputed at load; lookup_count/_key_at/_prefab_name_at per item
    ├── house_info.rs                          # HouseKey (u16) → "DefaultHouse_Lv1"/... (4 rows, name-only)
    ├── royal_supply_info.rs                   # RoyalSupplyKey (u16) → "RoyalSupply_Hernand"/... (4 rows, name-only)
    ├── craft_tool_info.rs                     # CraftToolKey (u16) → "CraftTool_Enchant"/... (17 rows, name-only)
    ├── craft_tool_group_info.rs               # CraftToolGroupKey (u16) → "CraftTool_Equip_Enchant"/... (10 rows, name-only)
    ├── trigger_region_info.rs                 # TriggerRegionKey (u32) → "Swamp"/"IceTerrain"/... (12 rows, name-only)
    ├── gameplay_variable_info.rs              # GamePlayVariableKey (u32) → "CD_Live"/"BaseCamp_Ranch_Lv1"/... (47 rows, name-only)
    ├── global_game_event_info.rs              # GlobalGameEventInfoKey (u16) → "Drought_Varnian"/... (103 rows, name-only)
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

# impl_name_only_bridge! macro in src/c_abi/mod.rs generates the
# standard 6-function ABI surface (load_from_file/_bytes, free,
# entry_count, lookup_string_key, get_entry) for the 13 niche bridges
# above. Each c_abi/<name>.rs file becomes a ~15-line macro invocation +
# a live-install integration test. The earlier bridges
# (store_info, mercenary_info, faction_node_info, …) predate the macro
# and remain hand-written; they can be migrated later if a cross-cutting
# ABI change ever lands.
#
# Lossy anchor-scan parsers used by the c_abi bridges live alongside in
# src/<table>_info/mod.rs. Each parser is feature-gated to the c_abi
# bridge that consumes it.
```
