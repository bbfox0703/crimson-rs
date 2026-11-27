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

**Thirty-five bridges shipped** (as of 2026-11-27): SkillKey, MissionKey,
QuestKey, StageKey, KnowledgeKey, QuestGaugeKey, SubLevelKey, GimmickInfoKey,
**CharacterKey**, **DyeColorGroupInfoKey**, **PartPrefabDyeTexturePalleteKey**,
**PartPrefabKey (dye-slot)**, **FactionKey**, **FactionNodeKey**, **FactionSpawnDataKey**,
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

### 2.06 row names (`_load_from_bytes_with_header`)

Crimson Desert 2.06 moved every static-info row's name out of the
`.staticinfobody` into the `.staticinfoheader`: the header is now
`count + ([key][u32 name len][name][u32 offset][u32 size])*`, and each body row
opens `[key][u32 row index]` followed by the rest of the 2.05.01 row. A bridge
that only took the body can no longer see the names, so nine bridges gained an
additive loader with the same shape:

```c
int32_t crimson_<table>_load_from_bytes_with_header(
    const uint8_t *data, size_t data_len,      /* .staticinfobody / .pabgb */
    const uint8_t *header, size_t header_len,  /* .staticinfoheader / .pabgh */
    Crimson<Table>Handle **out_handle);
```

- `crimson_missioninfo_`, `crimson_characterinfo_`, `crimson_knowledgeinfo_`,
  `crimson_stageinfo_`, `crimson_gimmickinfo_`, `crimson_questinfo_`,
  `crimson_questgaugeinfo_`, `crimson_sublevelinfo_load_from_bytes_with_header`
  — **required from 2.06**: the old body-only `*_load_from_bytes` finds no rows
  on a 2.06 body. On an older install the header is not needed and is ignored.
- `crimson_iteminfo_load_from_bytes_with_header` — without the header iteminfo
  still parses every row, but `string_key` comes back empty on 2.06. Here the
  header must be the 2.06 shape: an older install's index returns `BODY_PARSE`,
  so load those with `crimson_iteminfo_load_from_bytes`.
- The `*_load_from_file(path)` loaders of all nine read the sibling header
  automatically if it is present (`x.staticinfobody` → `x.staticinfoheader`,
  `x.pabgb` → `x.pabgh`; `sibling_header` in `src/c_abi/mod.rs`).
- Bridges that already took the body and the header (`pabgb`, `pabgh`) — the
  name-only catalog bridges, the faction / dye / store / mercenary /
  global-game-event bridges, and the stageinfo / questinfo
  `_load_from_*_with_pabgh` loaders — did not change: they rebuild the
  pre-2.06 layout internally (`src/binary/static_header.rs`).

**Downstream:** the C# editor (CrimsonAtomtic) must switch its nine
body-only `*_load_from_bytes` calls to the `_with_header` variants above, passing
the table's `.staticinfoheader`, or those bridges come up empty (and item names
blank) on 2.06.

### Factions: character membership, groups and relation types (2026-11-27)

The save records which factions exist and their relations
(`FactionSaveData._factionElementSaveDataList[i]._factionRelationDataList[j]
= {_targetFactionKey, _relationTypeRaw, _dueDate}`), not which faction an NPC
belongs to. That lives in `characterinfo` and is now exposed:

```c
/* FactionKey of a character (0 = none). Matches the key as given, then with
   the save cat-byte stripped. NOT_FOUND when the character is not in the table. */
int32_t crimson_characterinfo_lookup_faction_key(
    const CrimsonCharacterInfoHandle *handle, uint32_t character_key,
    uint32_t *out_faction_key);

/* Characters whose FactionKey equals faction_key (0 counts the faction-less). */
int32_t crimson_characterinfo_faction_member_count(
    const CrimsonCharacterInfoHandle *handle, uint32_t faction_key,
    uint32_t *out_count);

/* faction row -> factiongroup key (0 = none) + factionrelationgroup key. */
int32_t crimson_faction_info_lookup_group_keys(
    const CrimsonFactionInfoHandle *handle, uint32_t faction_key,
    uint16_t *out_group_key, uint16_t *out_relation_group_key);

/* Localized group name: PALOC (group << 32) | 0x214, two-call over buf. */
int32_t crimson_faction_info_lookup_group_display_name(
    const CrimsonPalocHandle *paloc, uint16_t faction_group_key,
    uint8_t *buf, size_t buf_len, size_t *required);

/* faction -> its group's localized name ("Greymanes", "Hernand", ...). */
int32_t crimson_faction_info_lookup_display_name(
    const CrimsonFactionInfoHandle *handle, const CrimsonPalocHandle *paloc,
    uint32_t faction_key, uint8_t *buf, size_t buf_len, size_t *required);
```

- **Where the character's faction sits**: a `u32` in the fixed head of every
  `characterinfo` row, byte 149 after the key on 2.04–2.05.01 and byte 151 after
  `[key][u32 row index]` on 2.06 (2.06 inserted two zero bytes at byte 95).
  Pinned on all 7,315 rows of five versions: always a `faction` key or 0, the
  same per character in each. 2,303 characters have one on 2.06;
  `Faction_Celeste_Hernand` has 66 (every `NHM_Hernand_Soldiers_*`),
  `Faction_Graymane` 172 (Kliff, Damian, …), `Faction_Bandit` 170,
  `Faction_Roberts` 2, `Faction_Serkis` 0. With a 2.06 header the characterinfo
  bridge now reads every row from the header, so the 359 rows with a hashed
  32-bit key are included too.
- **Groups**: 128 of 135 factions have a group; the 7 groups all have a name in
  every language (`faction.paloc`). Several factions share a group, so
  `..._lookup_display_name` is not unique; keep the internal name
  (`..._lookup_string_key`) to tell them apart.
- **`_relationTypeRaw` (u8)**: 0 Neutral, 1 War, 2 Agreement, 3 BloodAlly,
  4 ByGroup, from the name table the exe keeps right after its
  `FactionRelationGroupInfo` consistency message. **1 = War and 3 = BloodAlly
  are verified in game** (2026-11-27: player ⇄ `Faction_Celeste_Hernand`
  changed 3 → 1 both ways, and the Hernand guards attacked; a new two-way 3
  between the player and `Faction_Bandit`, which had no record and falls under
  `Monster_Common`, stopped the bandits attacking, so an explicit relation
  overrides the relation group). 0 is also the default of an absent
  field; 4 has not been seen in a save. Relations are stored both ways, and
  `_dueDate` was 0xFFFFFFFF on every one seen.
- **Group defaults (2026-11-27)**: a faction pair without an explicit
  record falls back to the relation between the two factions' relation groups
  (`FactionElementSaveData._relationGroupKey`):

  ```c
  /* FactionRelationType between two relation groups (order does not matter):
     1 War or 3 BloodAlly. NOT_FOUND for an unknown group and for a group with
     itself (the table defines no same-group relation). */
  int32_t crimson_factionrelationgroup_lookup_relation(
      const CrimsonFactionRelationGroupInfoHandle *handle,
      uint32_t group_a, uint32_t group_b, uint32_t *out_relation_type);
  ```

  Each `factionrelationgroup` row is `_isBlocked` and `_relationGroupList`:
  four count-prefixed `u16` group lists, one per FactionRelationType (Neutral,
  War, Agreement, BloodAlly; names from the exe's reflection data). Neutral and
  Agreement are empty on every row. The matrix is symmetric, as the exe's
  consistency message requires, and covers all 10 pairs of the 5 groups:
  Graymane (the player) is at War with HostileCombat and Monster_Common and
  BloodAlly with FriendlyCombat and NPC_Common; FriendlyCombat is at War with
  HostileCombat and Monster_Common; HostileCombat is BloodAlly with NPC_Common
  and at War with Monster_Common; NPC_Common is at War with Monster_Common. That
  matches the game (Monster_Common and HostileCombat attack the player) and the
  explicit player ⇄ `Faction_Celeste_Hernand` record that was already 3. The
  table is unchanged since 1.03.01, has 5 rows on 2.06 and no `dlc_01` copy.
  `..._lookup_related_count` / `_at` keep their meaning: every group a row
  lists, War first, then BloodAlly.
- The faction bridge's six name functions keep their names and signatures;
  `src/c_abi/faction_info.rs` now spells them out instead of using
  `impl_name_only_bridge!`, because the handle also holds the group keys.

### Faction nodes: place names (2026-11-27)

A `FactionNodeKey` (a stronghold, camp, village, cave, …) has no PALOC entry of
its own. Every factionnode row names a `knowledgeinfo` row, and that
knowledge's title is the place name the game shows:

```c
/* FactionNodeKey -> KnowledgeKey (Node_Her_HernandCastle 1000044 -> 1000016). */
int32_t crimson_factionnode_lookup_knowledge_key(
    const CrimsonFactionNodeInfoHandle *handle, uint32_t node_key,
    uint32_t *out_knowledge_key);

/* Localized place name: knowledge title, PALOC (hashlittle2(name) << 32) | 0x490.
   The language is the PALOC handle's. Two-call over buf. */
int32_t crimson_factionnode_lookup_display_name(
    const CrimsonFactionNodeInfoHandle *handle,
    const CrimsonKnowledgeInfoHandle *knowledge, const CrimsonPalocHandle *paloc,
    uint32_t node_key, uint8_t *buf, size_t buf_len, size_t *required);
```

- **Where the key sits**: a `u32` at byte 11 after the row key on 2.04–2.05.01
  and byte 8 after `[key][u32 row index]` on 2.06. Pinned on all 1,120 rows of
  five versions. 1000044 → "Hernand Castle", 1000003 `Node_Her_GreymaneCamp` →
  "Howling Hill", 1000311 → "Glenbright Farm". Every node has a title in all 15
  shipped languages; load the knowledge handle with its 2.06 header.
- The knowledgeinfo scanner now accepts an apostrophe in a row name. One row
  has one, `Knowledge_King's_Tomb_Cave` (1000535, the knowledge naming
  `Node_Her_Region_Cave_0080`), and the bridge used to miss it.
- **No owning faction or region field was found** in a factionnode row; see
  `src/faction_node_info/mod.rs` for what was ruled out. 576 of the nodes'
  knowledge rows sit in a `KnowledgeGroup_Topography_<Region>_<Area>` group,
  the rest in none.

### DLC tables: optional merge (2026-11-27)

`<install>/dlc/dlc_01/` carries its own copy of many tables, with rows the base
table lacks. It is Steam depot 5001841 (DLC app 5001840, 29,654,458,571 bytes,
exactly the folder's size), so **Steam installs it only for owners**: an install
without the DLC has no `dlc/dlc_01` folder. Four bridges can merge it:

```c
/* Add the DLC table's rows whose key the handle lacks; *out_added = how many.
   header may be empty (null, 0) only for knowledge / characterinfo on a
   pre-2.06 body. */
int32_t crimson_factionnode_merge_dlc_from_bytes(CrimsonFactionNodeInfoHandle *handle,
    const uint8_t *body, size_t body_len, const uint8_t *header, size_t header_len,
    uint32_t *out_added);
int32_t crimson_knowledgeinfo_merge_dlc_from_bytes(...);   /* same shape */
int32_t crimson_faction_info_merge_dlc_from_bytes(...);
int32_t crimson_characterinfo_merge_dlc_from_bytes(...);

/* 0 = base table, n = the n-th merged DLC. OUT_OF_RANGE past the end. */
int32_t crimson_factionnode_entry_source(const CrimsonFactionNodeInfoHandle *handle,
    uint32_t idx, uint32_t *out_source);
/* + crimson_knowledgeinfo_ / crimson_faction_info_ / crimson_characterinfo_entry_source */
```

- **Without the DLC nothing changes.** Load the base table as before and skip
  the merge; every function answers from the base rows, and a DLC-only key is
  `NOT_FOUND` as it always was. The merge is never automatic.
- **Where the files are**: extract `<table>.dlc_01.staticinfobody` +
  `.staticinfoheader` with `crimson_paz_extract_file` from
  `<install>/dlc/dlc_01/0008/0.pamt`, directory `gamedata/binarystaticinfo__/bin`
  (the base tables' directory). Call the merge once per DLC folder, in folder
  order. The handle's rows are the base rows in table order, then each DLC's new
  rows; existing entry indices do not move.
- **Shared keys keep the base row.** On 2.06 the rows `dlc_01` shares with the
  base table agree on every field these bridges expose (name, knowledge key,
  faction group keys, character FactionKey), so which copy wins does not change
  an answer. Many shared rows differ elsewhere (4,426 of 7,308 characterinfo
  rows), and for iteminfo the game reads the DLC row; if a future field differs,
  revisit this.
- **What `dlc_01` adds on 2.06**: 142 faction nodes (`Node_Ocean_IliyaIsland` →
  "Iliya Island", `Node_Crim_Dungeon_1`, …; 139 of them name a DLC knowledge
  row, so merge knowledge too before `crimson_factionnode_lookup_display_name`),
  795 knowledge rows, 11 factions (`Faction_Satyros`, `Faction_Ocean_Island`,
  `Faction_Void_Knight`, …; 7 in `FactionGroup_Ocean`, whose name "Great Ocean"
  is in the base PALOC like every DLC name since 2.06) and 287 characters (e.g.
  9 `Faction_Satyros` members). `factionrelationgroup` has no DLC copy.
- **`entry_source`** lets a list (every faction, every node) hide DLC rows.
  Lookups by key do not need it: a save only holds DLC keys if the DLC was
  played.
- The six faction contribution (`貢獻度`) items, `Contribution_Graymane` …
  `Contribution_Tashkalpan`, are all base items; `iteminfo.dlc_01` repeats them
  and adds no contribution item.
- Not merged: the other bridges, which still read the base table only.

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
  [`dye-editor-scope.md`](dye-editor-scope.md) §v2). Make-absent writes the
  list's one-byte `0x01` absence marker (below), so present(1) → present(0)
  restores the original bytes.
- **Element templates** (`crimson_save_export_element_template` /
  `crimson_save_list_insert_element_template`, `src/c_abi/element_template.rs`)
  — carry an `object_list` element across schema versions. Export records an
  element by NAME (class, wrapper bytes, and per present field its name plus
  payload, recursively; two-call buffer). Insert rebuilds it under the target
  save's own schema: removed fields are dropped (count reported through
  `out_dropped_fields`), new fields stay absent — arrays / lists with their
  absence marker, inline object locators (`meta_kind` 4, which the game never
  writes absent) with an empty child of the class the target save uses for
  them — and a changed kind or size fails with `TEMPLATE_MISMATCH` (−25)
  instead of being reinterpreted. Written for CrimsonAtomtic's dragon unlock,
  whose captured 1.09-era element no longer fit `MercenarySaveData` after
  2.00 (`_occupationState` removed, `_shipStationSaveList` added in 2.01).

**Absence markers.** The engine writes one `0x01` byte for every *absent*
dynamic array (`meta_kind` 3) and object list (6 / 7), and nothing for other
absent fields; present arrays and lists open with a `0x00` tag and a u32
count. The decoder walks every object that way first
(`walk_fields_with_markers`) and keeps the legacy walk only as a fallback for
bytes the rule leaves unexplained — none on any save measured (12 live saves
from 1.10 to 2.03 plus the 1.09 / 1.10 fixtures, 118k–135k objects and
170k–181k markers each). `DecodedField::absent_marker` records the byte and
the encoder re-emits it; `crimson_save_make_empty_element_bytes` now writes a
marker for each of the new element's arrays and lists. Before this, the walk
skipped absent fields without consuming anything, so each marker was read as
part of the NEXT field — e.g. a mercenary's `_currentHp` behind absent lists
read as 1,134,700,311,674,881 instead of 1,032 — and the leftovers were
absorbed by list-header variants (`marker_run_plus_zeros`, `ones_then_count`,
`one_count_u16be`), dynamic-array variants (`marker_prefix`, and the
`prefix_00xx0100` family, a misread neighbour plus markers) and
`trailing_pad`. Byte-exact round-trips hid all of it.

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
  build's parsers target (currently **6**, i.e. 2.06). **Single source of
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
- `crimson_parser_min_gamedata_patch() -> u16` — the lowest gamedata `patch`
  this build reads on the target `(major, minor)` (currently **0**: any 2.06
  patch). Added when 2.04.08 changed the iteminfo layout without moving the
  minor, so `(major, minor)` alone would accept a 2.04.03 install that no longer
  parses. 2.05.01 did the same inside minor 5 (floor 1); 2.06 moved the minor, so
  the floor reset to 0. Full gate: `major == target_major && minor ∈ compatible_minors &&
  (minor != target_minor || patch >= min_patch)`. `0` means any patch. Backs onto
  `crate::binary::paver::PARSER_MIN_GAMEDATA_PATCH`.

### Status & remaining work

388 tests pass with `c_abi` on 2.06 (+48 ignored diagnostic probes); the bare-default
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
(h) ~~which of 2.01's "four 3-byte mask groups" a dye slot uses~~ — **resolved
    2026-11-27**: there are no groups. The 12-byte mask has one flag per dye slot
    number (the values in `mat_indices`, and the save's `_dyeSlotNo`); see
    `..._lookup_slot_active_channels` / `..._lookup_dye_slot_bits` and
    [`dye-editor-scope.md`](dye-editor-scope.md) "Dye slot numbers (2.01+)".

## Rust module structure

```
src/
├── lib.rs                   # Crate root (mission/quest/stage/knowledge/questgauge/
│                            #   sub_level/gimmick/character modules gated behind #[cfg(feature = "c_abi")])
├── python.rs                # PyO3 bindings
├── binary/                  # Container formats
│   ├── pamt.rs, papgt.rs    # PAMT / PAPGT parse + write
│   ├── paz.rs               # PackGroupBuilder, compression, PAZ creation
│   ├── paloc.rs             # PALOC parse/write (numeric + symbolic keys) + the
│   │                        #   2.03 LZ4 container (unwrap_container / wrap_container)
│   ├── gamedata_layout.rs   # #[cfg(test)] — which archive layout the live install
│   │                        #   ships (2.01 renamed the gamedata dir + every
│   │                        #   extension); newest-first with fallback
│   ├── paver.rs             # meta/0.paver version-stamp reader
│   ├── static_header.rs     # 2.06 named .staticinfoheader + [key][row index] body rows;
│   │                        #   rebuilds the pre-2.06 layout for the table parsers
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
    │                         # + crimson_parser_target_gamedata_major/_minor, _compatible_gamedata_minors, _min_gamedata_patch
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
    ├── paloc.rs             # PALOC handle + length-prefixed lookup (both loaders
    │                        #   accept the bare list and the 2.03 LZ4 container)
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
    │                        #   + lookup_faction_key / faction_member_count (FactionKey from the row head)
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
    │                                           #   2026-11-27: the 12 bytes are one flag per dye slot number
    │                                           #   (mat_indices values = save _dyeSlotNo); lookup_slot_active_channels
    │                                           #   (the pre-2.01 per-channel meaning, 3-byte out), lookup_slot_dye_slot_bits,
    │                                           #   lookup_slot_extra_layer_dye_slot_bits, lookup_dye_slot_bits (prefab
    │                                           #   union = the item's _dyeSlotNo set), lookup_dye_slot_material
    ├── faction_info.rs                        # FactionKey (u32) → "Faction_Graymane"/"Faction_ThiefDusksong"/... (135 rows in 2.06, name-only;
    │                                           #   rows carry no localizable string, and PALOC byte 0x30 at a FactionKey
    │                                           #   answers with *character* names — CharacterKey 1000000 "Hernandian Guard")
    │                                           #   + lookup_group_keys, lookup_group_display_name / lookup_display_name
    │                                           #   (factiongroup name, PALOC lo32 0x214: "Greymanes")
    ├── faction_node_info.rs                   # FactionNodeKey (u32) → "Node_Her_Temporary_Camp", + KnowledgeKey → place name
    ├── faction_spawn_data_info.rs             # FactionSpawnDataKey (u32) → "FactionSpawn_GlenbrightManor_Grace_ReedDevil" etc.
    ├── faction_relation_group_info.rs         # FactionRelationGroupKey (u16-widened-u32) → "Graymane"/"FriendlyCombat"/"HostileCombat"/"NPC_Common"/"Monster_Common", + group-to-group FactionRelationType
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
    │                                          #   (170 rows, static table — no file load, no handle). Every row carries
    │                                          #   the game row its title comes from (MissionKey / QuestKey). Lookups:
    │                                          #   chapter_for_arc / chapter_for_mission / arc_for_mission, and by key
    │                                          #   chapter_for_mission_key / arc_for_mission_key / chapter_for_quest_key
    │                                          #   + table_get_entry_keys. Titles reconciled against 2.02,
    │                                          #   updated for the 2.03 retitles.
    └── side_quest_faction.rs                  # Curated (quest, faction) rollup from docs/ref-gamedata/side-quest-list.md
                                               #   (84 rows / 23 factions, static table — sibling of main_quest_chapter).
                                               #   Lookups: faction_for_quest (1:1) + quest_count_for_faction /
                                               #   quest_at_for_faction (reverse enumeration), and by key
                                               #   faction_for_quest_key / faction_for_mission_key + table_get_entry_key.
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
