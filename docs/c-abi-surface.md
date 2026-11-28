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
  on a 2.06 body and returns `BODY_PARSE`.
  With a 2.06 header every one of these returns exactly the rows the header
  lists (since 2026-11-28; before that seven of them anchor-scanned the rebuilt
  body and dropped rows: mission 2,444, stage 3,198, gimmick 140, quest 1).
- `crimson_iteminfo_load_from_bytes_with_header` — without the header iteminfo
  still parses every row, but `string_key` comes back empty on 2.06. Here the
  header must be the 2.06 shape: an older install's index returns `BODY_PARSE`,
  so load those with `crimson_iteminfo_load_from_bytes`.
- The `*_load_from_file(path)` loaders of all nine read the sibling header
  automatically if it is present (`x.staticinfobody` → `x.staticinfoheader`,
  `x.pabgb` → `x.pabgh`, extension case ignored; `sibling_header` in
  `src/c_abi/mod.rs`). A header that exists but cannot be read returns `IO`.
- **Every table loader refuses a table it cannot read** (since 2026-11-28, audit
  B-02 / E-04; `check_rows` in `src/c_abi/mod.rs`). A non-empty header that is
  not a 2.06 header matching the body, a header that lists rows none of which
  read, and a non-empty body read without a header that gives no row all return
  `BODY_PARSE` with no handle. They used to return `OK` with 0 rows (or, for a
  body without its header, noise rows). The DLC merges check the same way:
  a DLC header that does not match its body is `BODY_PARSE`, not `OK` with
  `added == 0`. Single rows dropped inside a readable table are not reported;
  the live tests pin those counts.
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
  32-bit key are included too. Every lookup matches the key as given first,
  then with the cat-byte stripped, so `..._lookup_string_key`,
  `..._lookup_display_name` and `..._resolve_portrait` reach those rows as well
  (their names sit at PALOC `(full_key << 32) | 0x30`; no hashed key's low 24
  bits are another row's key). Before 2026-11-28 only `get_entry` and
  `..._lookup_faction_key` did.
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
without the DLC has no `dlc/dlc_01` folder. Five bridges can merge it:

```c
/* Add the DLC table's rows whose key the handle lacks; *out_added = how many.
   The header is required: an empty one returns BODY_PARSE (since 2026-11-28;
   before, knowledge / characterinfo body-scanned the 2.06 body and added junk
   rows). A merge that adds nothing no longer uses up a source number. */
int32_t crimson_factionnode_merge_dlc_from_bytes(CrimsonFactionNodeInfoHandle *handle,
    const uint8_t *body, size_t body_len, const uint8_t *header, size_t header_len,
    uint32_t *out_added);
int32_t crimson_knowledgeinfo_merge_dlc_from_bytes(...);   /* same shape */
int32_t crimson_faction_info_merge_dlc_from_bytes(...);
int32_t crimson_characterinfo_merge_dlc_from_bytes(...);
/* The item → dye-row join takes iteminfo + stringinfo + the DLC character
   PAMT instead (see dye-editor-scope.md "Item → mesh join"): */
int32_t crimson_item_part_prefab_merge_dlc_from_bytes(CrimsonItemPartPrefabHandle *handle,
    const uint8_t *iteminfo, size_t iteminfo_len, const uint8_t *stringinfo, size_t stringinfo_len,
    const char *dlc_prefab_pamt_path /* nullable */, uint32_t *out_added);

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
- **Save-editing safety fixes (2026-11-28, audit G-01 / G-03 / G-04 / G-06)** — no
  signature changed, but four behaviours did:
  `crimson_save_list_all_items` and `crimson_save_list_field_positions` now take
  the same process-global lock as every other `crimson_save_*` call, so they are
  safe beside a mutation on another thread. `crimson_save_set_object_list_present`
  leaves a list already in the requested state alone (making a present list
  present again used to replace its elements with one default element). Every
  length-changing list edit (remove, clone, insert, transplant, template insert)
  returns the new `LIST_PARTIALLY_DECODED` (−26) instead of editing a list whose
  header count exceeds the elements the decoder read. And `mutation_version` now
  moves with every mutation inside a deferred batch and forward on abort, see
  [`save-mutation-version.md`](save-mutation-version.md).
- **Save-editing safety net (2026-11-28, audit F-01 / G-02 / G-10 / G-11)** — the
  encoder refuses a block the decoder did not fully explain (undecoded bytes,
  `Unknown` fields, placeholder elements, a locator child away from its wrapper),
  so a length-changing edit returns `MUTATION_INVALID` instead of writing a body
  with bytes missing. After every commit the re-decoded tree must decode
  completely too, or the edit rolls back with `MUTATION_INVALID`; this is what
  makes the transplant's "definition mismatch" promise hold. Inserts, transplants
  and template inserts return the new `ELEMENT_CLASS_MISMATCH` (−27) for an
  element whose class differs from the list's (no game save mixes classes in a
  list: 0 of 213,171 lists in 21 saves). Exporting a placeholder element as a
  template returns `BODY_PARSE`. Still open: the template compares a scalar's
  kind and size but not its type name, which the CRET v1 format does not record.
- **Unverified saves are not re-sealed (2026-11-28, audit F-10)** —
  `crimson_save_write_to_file` returns the new `HMAC_UNVERIFIED` (−28) and
  writes nothing when the save's HMAC did not verify on load
  (`crimson_save_get_hmac_ok` = 0). Writing computes a fresh HMAC, so it used to
  turn a damaged or tampered save the game rejects into one it accepts, even
  with no edit. `crimson_save_set_allow_hmac_rewrap(h, 1)` (new) opts in for a
  deliberate repair. Every real save verifies, so a normal edit is unaffected.

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

396 tests pass with `c_abi` on 2.06 (+48 ignored diagnostic probes); the bare-default
build (`cargo check --lib`) is also clean; clippy clean both modes. The Jenkins
hash-hop transform that drives Mission/Quest/Stage/Knowledge title resolution is
cracked and pinned. CharacterKey ships at the 22% display-name coverage §6
predicted plus a full-coverage `lookup_string_key` internal-name fallback. The
three dye gamedata bridges (2026-05-16) replace the PyQt5 `dye_slot_counts.json`
with byte-perfect gamedata-driven slot counts + per-slot material defaults +
named color-group + palette-tier resolvers — see
[`dye-editor-scope.md`](dye-editor-scope.md); the `_itemKey → _partPrefabKey`
linkage is bridged through `crimson_item_part_prefab_*`. Since 2026-11-27 it
goes through the character prefabs (`..._load_*_with_prefab_pamt`: item prefab
→ `.prefab` in the 0009 group → `.pac` meshes → row key = checksum of the
lower-case `.pac` path), reaching 909 of 917 dyeable 2.06 items instead of 70.

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
(i) ~~dye slots of body-type armor (no `partprefabdyeslotinfo` row)~~ —
    **resolved 2026-11-27**: the rows exist; they are per `.pac` mesh, and an
    item reaches them through its `.prefab`. See
    `crimson_item_part_prefab_load_*_with_prefab_pamt` /
    `..._lookup_dye_slot_bits` / `..._lookup_channel_dye_slot_bits` /
    `..._lookup_dye_slot_material` and [`dye-editor-scope.md`](dye-editor-scope.md)
    "Item → mesh join (2026-11-27)". The DLC tree joins through the optional
    `crimson_item_part_prefab_merge_dlc_from_bytes` (82 of 85 DLC-only dyeable
    items on 2.06).

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
    ├── dye_color_group_info.rs                # DyeColorGroupInfoKey (u32) → "Her_Color_Group_I" etc., palette, each color's unlock KnowledgeKey, localized theme name
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
    ├── item_part_prefab.rs                    # ItemKey → list of PartPrefabKey. 2026-11-27 prefab join
    │                                           #   (load_*_with_prefab_pamt): item prefab name → .prefab in the
    │                                           #   character PAMT (0009) → .pac paths → key = checksum(lower(pac));
    │                                           #   per-variant (tribe_gender_list) storage; lookup_dye_slot_bits /
    │                                           #   lookup_channel_dye_slot_bits / lookup_dye_slot_material (item-level,
    │                                           #   optional tribe/gender filter). Original 3-table name join
    │                                           #   (iteminfo + stringinfo + partprefabdyeslotinfo) kept as load_from_*.
    │                                           #   Optional, atomic merge_dlc_from_bytes (dlc iteminfo + stringinfo +
    │                                           #   dlc 0009 PAMT) + lookup_source (0 base / n-th DLC).
    │                                           #   precomputed at load; lookup_count/_key_at/_prefab_name_at per item +
    │                                           #   resolve_dye_slot_count (v3 convenience wrapper — one-shot
    │                                           #   "how many dye slots does this item have?": popcount of the item's
    │                                           #   dye slot bits since 2026-11-28 (was the first row's sub-mesh count),
    │                                           #   returns (slot_count, resolve_source) with NOT_RESOLVED states)
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

## Symbol index

<!-- symbol-index:start -->

Every exported `crimson_*` function, 375 in all, grouped by source file, with the first line
of its rustdoc. `repo_doc_budget::c_abi_surface_lists_every_symbol` fails when a symbol is
missing from this file, so add a line here (or a mention above) with any new export.

**`src/c_abi/all_items.rs`**

- `crimson_save_list_all_items` — Flat-list every player-owned item across the save's container classes (active equip, active reserve, inventory, mercenary equip, mercenary…

**`src/c_abi/character_info.rs`**

- `crimson_characterinfo_load_from_file` — Parse `characterinfo.pabgb` from disk.
- `crimson_characterinfo_load_from_bytes` — Parse characterinfo bytes already in memory.
- `crimson_characterinfo_load_from_bytes_with_header` — Parse in-memory `characterinfo` bytes together with the table's `.staticinfoheader`.
- `crimson_characterinfo_free` — Free a handle returned by either loader.
- `crimson_characterinfo_entry_count` — Total number of characters in the loaded table.
- `crimson_characterinfo_lookup_string_key` — Look up the internal name for a given `CharacterKey (u32)` and write it into `buf` (NUL-terminated UTF-8).
- `crimson_characterinfo_merge_dlc_from_bytes` — Merge a DLC copy of the table (`<install>/dlc/<name>/0008`, e.g. `characterinfo.dlc_01.staticinfobody` + `.staticinfoheader`) into a loade…
- `crimson_characterinfo_entry_source` — Where entry `idx` came from: 0 = the base table, `n` = the `n`-th DLC merged with [`crimson_characterinfo_merge_dlc_from_bytes`].
- `crimson_characterinfo_lookup_faction_key` — The `FactionKey` of `character_key`'s row — the `faction` table key the character belongs to, which the save does not record (see `src/cha…
- `crimson_characterinfo_faction_member_count` — How many characters belong to `faction_key` (rows whose `FactionKey` equals it).
- `crimson_characterinfo_lookup_display_name` — One-shot display name resolution: `CharacterKey → PALOC at `(row_key << 32) | lo32_namespace` → localized string`, where `row_key` is the…
- `crimson_characterinfo_get_entry` — Get the `(character_key, internal_name)` pair at insertion index `idx`.
- `crimson_characterinfo_resolve_portrait` — Resolve a `CharacterKey` to the best-matching NPC portrait DDS path in a single call.

**`src/c_abi/checksum.rs`**

- `crimson_calculate_checksum` — Compute the Jenkins hashlittle2 `c` output (Pearl Abyss variant — init constant `0xDEBA1DCD`) over `data`.

**`src/c_abi/craft_tool_group_info.rs`**

- `crimson_craft_tool_group_info_load_from_file` — Load from a `.staticinfobody` path (the header beside it is read too). (`impl_name_only_bridge!`)
- `crimson_craft_tool_group_info_load_from_bytes` — Load from body + header bytes. (`impl_name_only_bridge!`)
- `crimson_craft_tool_group_info_free` — Free the handle. (`impl_name_only_bridge!`)
- `crimson_craft_tool_group_info_entry_count` — Number of rows. (`impl_name_only_bridge!`)
- `crimson_craft_tool_group_info_lookup_string_key` — Key → internal name (two-call buffer). (`impl_name_only_bridge!`)
- `crimson_craft_tool_group_info_get_entry` — Row at an index: key + internal name. (`impl_name_only_bridge!`)

**`src/c_abi/craft_tool_info.rs`**

- `crimson_craft_tool_info_load_from_file` — Load from a `.staticinfobody` path (the header beside it is read too). (`impl_name_only_bridge!`)
- `crimson_craft_tool_info_load_from_bytes` — Load from body + header bytes. (`impl_name_only_bridge!`)
- `crimson_craft_tool_info_free` — Free the handle. (`impl_name_only_bridge!`)
- `crimson_craft_tool_info_entry_count` — Number of rows. (`impl_name_only_bridge!`)
- `crimson_craft_tool_info_lookup_string_key` — Key → internal name (two-call buffer). (`impl_name_only_bridge!`)
- `crimson_craft_tool_info_get_entry` — Row at an index: key + internal name. (`impl_name_only_bridge!`)

**`src/c_abi/dye_color_group_info.rs`**

- `crimson_dye_color_group_info_load_from_file` — Parse `dyecolorgroupinfo.pabgb` + `.pabgh` from disk.
- `crimson_dye_color_group_info_load_from_bytes` — Parse bytes already in memory.
- `crimson_dye_color_group_info_free` — Free a handle returned by either loader.
- `crimson_dye_color_group_info_entry_count` — Number of color-group rows in the table.
- `crimson_dye_color_group_info_lookup_name` — Look up the internal name for a given `DyeColorGroupInfoKey (u32)`.
- `crimson_dye_color_group_info_get_entry` — Get the `(key, internal_name)` pair at insertion index `idx`.
- `crimson_dye_color_group_info_palette_size` — Number of palette positions for `color_group_key` (109 in 1.07, but the ABI doesn't pin the count — a future patch with a different palett…
- `crimson_dye_color_group_info_palette_at` — Logical RGBA at `position_idx` inside `color_group_key`'s palette.
- `crimson_dye_color_group_info_position_for_rgb` — Reverse lookup — find the palette position that matches the given `(r, g, b)`.
- `crimson_dye_color_group_info_palette_knowledge_at` — The `KnowledgeKey` that unlocks palette position `position` of `color_group_key`: the key of the color record's inlined `CheckKnowledge(<k…
- `crimson_dye_color_group_info_lookup_display_name` — The theme's localized display name ("Hernandian Dye", "埃爾南德染劑"), from the row's trailer: PALOC `(color_group_key << 32) | 0x231`.

**`src/c_abi/element_template.rs`**

- `crimson_save_export_element_template` — Export one `object_list` element as a name-keyed element template (see the module docs).
- `crimson_save_list_insert_element_template` — Build a list element from a name-keyed template under THIS save's schema and insert it into an `object_list` field at `insert_at`.

**`src/c_abi/faction_info.rs`**

- `crimson_faction_info_load_from_file` — Load from a `.pabgb` / `.staticinfobody` path and its header path.
- `crimson_faction_info_load_from_bytes` — Load from in-memory body + header bytes.
- `crimson_faction_info_free` — # Safety `handle` must be null or a pointer previously returned by one of the loaders and not yet freed.
- `crimson_faction_info_entry_count` — # Safety `handle` must be live; `out_count` must be non-null.
- `crimson_faction_info_lookup_string_key` — Internal name of `faction_key` (two-call pattern over `buf`).
- `crimson_faction_info_get_entry` — `(faction_key, internal_name)` at index `idx` (two-call pattern over `buf`; `out_key` is always written).
- `crimson_faction_info_merge_dlc_from_bytes` — Merge a DLC copy of the table (`<install>/dlc/<name>/0008`, e.g. `faction.dlc_01.staticinfobody` + `.staticinfoheader`) into a loaded hand…
- `crimson_faction_info_entry_source` — Where entry `idx` came from: 0 = the base table, `n` = the `n`-th DLC merged with [`crimson_faction_info_merge_dlc_from_bytes`].
- `crimson_faction_info_lookup_group_keys` — The faction's `factiongroup` key and `factionrelationgroup` key.
- `crimson_faction_info_lookup_group_display_name` — Localized name of a faction group: PALOC `(faction_group_key << 32) | 0x214` (`faction.paloc`; 16965 → "Greymanes" in English).
- `crimson_faction_info_lookup_display_name` — The player-facing name of a faction: its group's localized name (`Faction_Graymane` → "Greymanes", `Faction_Celeste_Hernand` → "Hernand").

**`src/c_abi/faction_node_info.rs`**

- `crimson_factionnode_load_from_file` — Parse `factionnode.pabgb` + `.pabgh` from disk.
- `crimson_factionnode_load_from_bytes` — Parse factionnode pabgb + pabgh bytes already in memory.
- `crimson_factionnode_free` — Free a handle returned by either loader.
- `crimson_factionnode_entry_count` — Total number of `factionnode` rows in the loaded table.
- `crimson_factionnode_lookup_string_key` — Look up the internal name for a given `FactionNodeKey (u32)` and write it into `buf` (NUL-terminated UTF-8).
- `crimson_factionnode_get_entry` — Get the `(faction_node_key, internal_name)` pair at insertion index `idx`.
- `crimson_factionnode_merge_dlc_from_bytes` — Merge a DLC copy of the table (`<install>/dlc/<name>/0008`, e.g. `factionnode.dlc_01.staticinfobody` + `.staticinfoheader`) into a loaded…
- `crimson_factionnode_entry_source` — Where entry `idx` came from: 0 = the base table, `n` = the `n`-th DLC merged with [`crimson_factionnode_merge_dlc_from_bytes`].
- `crimson_factionnode_lookup_knowledge_key` — The `KnowledgeKey` naming a node: the knowledge row whose title is the place name the game shows (`Node_Her_HernandCastle` 1000044 → 10000…
- `crimson_factionnode_lookup_display_name` — The node's localized place name: `FactionNodeKey → KnowledgeKey →` knowledge name `→ PALOC (hashlittle2(name) << 32) | 0x490`, the knowled…

**`src/c_abi/faction_relation_group_info.rs`**

- `crimson_factionrelationgroup_load_from_file` — Parse `factionrelationgroup.pabgb` + `.pabgh` from disk.
- `crimson_factionrelationgroup_load_from_bytes` — Parse factionrelationgroup pabgb + pabgh bytes already in memory.
- `crimson_factionrelationgroup_free` — Free a handle returned by either loader.
- `crimson_factionrelationgroup_entry_count` — Total number of `factionrelationgroup` rows in the loaded table (always 5 in 1.07).
- `crimson_factionrelationgroup_lookup_string_key` — Look up the internal name for a given `FactionRelationGroupKey` (passed as u32 — only the low 16 bits are meaningful).
- `crimson_factionrelationgroup_lookup_relation` — The `FactionRelationType` between two relation groups: 1 War or 3 BloodAlly for every pair of distinct groups on 2.06 (0 Neutral and 2 Agr…
- `crimson_factionrelationgroup_lookup_related_count` — Total number of groups a row lists, across all its relation lists (War, then BloodAlly); `Graymane` returns 4.
- `crimson_factionrelationgroup_lookup_related_at` — Read a single sibling-row reference at `idx`.
- `crimson_factionrelationgroup_get_entry` — Get the `(faction_relation_group_key, internal_name)` pair at insertion index `idx`.

**`src/c_abi/faction_spawn_data_info.rs`**

- `crimson_factionspawndatainfo_load_from_file` — Parse `factionspawndatainfo.pabgb` + `.pabgh` from disk.
- `crimson_factionspawndatainfo_load_from_bytes` — Parse factionspawndatainfo pabgb + pabgh bytes already in memory.
- `crimson_factionspawndatainfo_free` — Free a handle returned by either loader.
- `crimson_factionspawndatainfo_entry_count` — Total number of `factionspawndatainfo` rows in the loaded table.
- `crimson_factionspawndatainfo_lookup_string_key` — Look up the internal name for a given `FactionSpawnDataKey (u32)`.
- `crimson_factionspawndatainfo_get_entry` — Get the `(faction_spawn_data_key, internal_name)` pair at insertion index `idx`.

**`src/c_abi/game_advice_group_info.rs`**

- `crimson_game_advice_group_info_load_from_file` — Load from a `.staticinfobody` path (the header beside it is read too). (`impl_name_only_bridge!`)
- `crimson_game_advice_group_info_load_from_bytes` — Load from body + header bytes. (`impl_name_only_bridge!`)
- `crimson_game_advice_group_info_free` — Free the handle. (`impl_name_only_bridge!`)
- `crimson_game_advice_group_info_entry_count` — Number of rows. (`impl_name_only_bridge!`)
- `crimson_game_advice_group_info_lookup_string_key` — Key → internal name (two-call buffer). (`impl_name_only_bridge!`)
- `crimson_game_advice_group_info_get_entry` — Row at an index: key + internal name. (`impl_name_only_bridge!`)

**`src/c_abi/game_advice_info.rs`**

- `crimson_game_advice_info_load_from_file` — Load from a `.staticinfobody` path (the header beside it is read too). (`impl_name_only_bridge!`)
- `crimson_game_advice_info_load_from_bytes` — Load from body + header bytes. (`impl_name_only_bridge!`)
- `crimson_game_advice_info_free` — Free the handle. (`impl_name_only_bridge!`)
- `crimson_game_advice_info_entry_count` — Number of rows. (`impl_name_only_bridge!`)
- `crimson_game_advice_info_lookup_string_key` — Key → internal name (two-call buffer). (`impl_name_only_bridge!`)
- `crimson_game_advice_info_get_entry` — Row at an index: key + internal name. (`impl_name_only_bridge!`)

**`src/c_abi/gameplay_variable_info.rs`**

- `crimson_gameplay_variable_info_load_from_file` — Load from a `.staticinfobody` path (the header beside it is read too). (`impl_name_only_bridge!`)
- `crimson_gameplay_variable_info_load_from_bytes` — Load from body + header bytes. (`impl_name_only_bridge!`)
- `crimson_gameplay_variable_info_free` — Free the handle. (`impl_name_only_bridge!`)
- `crimson_gameplay_variable_info_entry_count` — Number of rows. (`impl_name_only_bridge!`)
- `crimson_gameplay_variable_info_lookup_string_key` — Key → internal name (two-call buffer). (`impl_name_only_bridge!`)
- `crimson_gameplay_variable_info_get_entry` — Row at an index: key + internal name. (`impl_name_only_bridge!`)

**`src/c_abi/gimmick_info.rs`**

- `crimson_gimmickinfo_load_from_file` — Parse `gimmickinfo.pabgb` from disk.
- `crimson_gimmickinfo_load_from_bytes` — Parse gimmickinfo bytes already in memory.
- `crimson_gimmickinfo_load_from_bytes_with_header` — Parse in-memory `gimmickinfo` bytes together with the table's `.staticinfoheader`.
- `crimson_gimmickinfo_free` — Free a handle returned by either loader.
- `crimson_gimmickinfo_entry_count` — Total number of gimmicks in the loaded table.
- `crimson_gimmickinfo_lookup_string_key` — Look up the internal name for a given `GimmickInfoKey (u32)` and write it into `buf` (NUL-terminated UTF-8).
- `crimson_gimmickinfo_lookup_display_name` — One-shot display name resolution: `GimmickInfoKey → PALOC at `(gimmick_key << 32) | lo32_namespace` → localized string`.
- `crimson_gimmickinfo_get_entry` — Get the `(gimmick_key, internal_name)` pair at insertion index `idx`.

**`src/c_abi/global_game_event_group_info.rs`**

- `crimson_global_game_event_group_info_load_from_file` — Load from a `.staticinfobody` path (the header beside it is read too). (`impl_name_only_bridge!`)
- `crimson_global_game_event_group_info_load_from_bytes` — Load from body + header bytes. (`impl_name_only_bridge!`)
- `crimson_global_game_event_group_info_free` — Free the handle. (`impl_name_only_bridge!`)
- `crimson_global_game_event_group_info_entry_count` — Number of rows. (`impl_name_only_bridge!`)
- `crimson_global_game_event_group_info_lookup_string_key` — Key → internal name (two-call buffer). (`impl_name_only_bridge!`)
- `crimson_global_game_event_group_info_get_entry` — Row at an index: key + internal name. (`impl_name_only_bridge!`)

**`src/c_abi/global_game_event_info.rs`**

- `crimson_global_game_event_info_load_from_file` — Parse `globalgameevent.{pabgb,pabgh}` from disk.
- `crimson_global_game_event_info_load_from_bytes` — Parse globalgameevent bytes already in memory.
- `crimson_global_game_event_info_free` — Free a handle returned by either loader.
- `crimson_global_game_event_info_entry_count` — Total number of entries in the loaded table.
- `crimson_global_game_event_info_lookup_string_key` — Look up the internal template name for a `GlobalGameEventInfoKey`.
- `crimson_global_game_event_info_lookup_group_key` — Look up the `GlobalGameEventGroupKey` for a given event key.
- `crimson_global_game_event_info_lookup_paloc_key` — Look up the 64-bit PALOC key for a given event's localized display name.
- `crimson_global_game_event_info_get_entry` — Fetch the (key, name) pair at a positional index for iteration.

**`src/c_abi/house_info.rs`**

- `crimson_house_info_load_from_file` — # Safety Both path arguments must be NUL-terminated UTF-8 strings.
- `crimson_house_info_load_from_bytes` — # Safety Both `pabgb` and `pabgh` must point to `*_len` readable bytes (may be null iff length is 0).
- `crimson_house_info_free` — # Safety `handle` must be null or a pointer previously returned by one of the loaders and not yet freed.
- `crimson_house_info_entry_count` — # Safety `handle` must be live; `out_count` must be non-null.
- `crimson_house_info_lookup_string_key` — # Safety `handle` and `required` must be non-null; `buf` may be null iff `buf_len == 0`.
- `crimson_house_info_get_entry` — # Safety `handle`, `out_key`, and `required` must be non-null; `buf` may be null iff `buf_len == 0`.

**`src/c_abi/item_group_info.rs`**

- `crimson_item_group_info_load_from_file` — Load from a `.staticinfobody` path (the header beside it is read too). (`impl_name_only_bridge!`)
- `crimson_item_group_info_load_from_bytes` — Load from body + header bytes. (`impl_name_only_bridge!`)
- `crimson_item_group_info_free` — Free the handle. (`impl_name_only_bridge!`)
- `crimson_item_group_info_entry_count` — Number of rows. (`impl_name_only_bridge!`)
- `crimson_item_group_info_lookup_string_key` — Key → internal name (two-call buffer). (`impl_name_only_bridge!`)
- `crimson_item_group_info_get_entry` — Row at an index: key + internal name. (`impl_name_only_bridge!`)

**`src/c_abi/item_part_prefab.rs`**

- `crimson_item_part_prefab_load_from_file` — Parse the three required tables from disk (the original name join; prefer [`crimson_item_part_prefab_load_from_file_with_prefab_pamt`], se…
- `crimson_item_part_prefab_load_from_bytes` — Parse the three required tables from in-memory bytes.
- `crimson_item_part_prefab_load_from_bytes_with_prefab_pamt` — Build the join through the character prefabs (the 2026-11-27 join; see the module docs).
- `crimson_item_part_prefab_load_from_file_with_prefab_pamt` — File-path form of [`crimson_item_part_prefab_load_from_bytes_with_prefab_pamt`].
- `crimson_item_part_prefab_merge_dlc_from_bytes` — Merge an installed DLC into a loaded handle: its `iteminfo` and `stringinfo` bodies (`iteminfo.dlc_01.staticinfobody`, `stringinfo.dlc_01.…
- `crimson_item_part_prefab_lookup_source` — Where an item's join came from: 0 = the base table, `n` = the `n`-th [`crimson_item_part_prefab_merge_dlc_from_bytes`].
- `crimson_item_part_prefab_free` — Free a handle returned by any loader.
- `crimson_item_part_prefab_resolved_item_count` — Number of items with at least one resolvable partprefab key.
- `crimson_item_part_prefab_lookup_count` — Number of partprefab keys resolvable for a given item.
- `crimson_item_part_prefab_lookup_key_at` — Get the partprefab key at insertion order `idx` for the given item.
- `crimson_item_part_prefab_lookup_prefab_name_at` — Get the prefab name string corresponding to the partprefab key at position `idx`.
- `crimson_item_part_prefab_lookup_dye_slot_bits` — The dye slot numbers an item offers: the union of the dye slot bits of every `partprefabdyeslotinfo` row of its body variants (bit `n` = `…
- `crimson_item_part_prefab_lookup_channel_dye_slot_bits` — Every dye slot number a material channel of the item's rows carries (`mat_indices`), whether or not the current mask enables it: a superse…
- `crimson_item_part_prefab_lookup_dye_slot_material` — Default material name of dye slot number `dye_slot_no` on an item: the first non-empty name over its rows, in variant then mesh order.
- `crimson_item_part_prefab_resolve_dye_slot_count` — One-shot "how many dye slots does this item have?": the number of `_dyeSlotNo` values the item offers, i.e. the bits set in the union of […

**`src/c_abi/iteminfo.rs`**

- `crimson_iteminfo_load_from_file` — Parse an `iteminfo.pabgb` blob from disk.
- `crimson_iteminfo_load_from_bytes` — Parse iteminfo bytes already in memory.
- `crimson_iteminfo_load_from_bytes_with_header` — Parse in-memory iteminfo bytes together with the table's `.staticinfoheader`.
- `crimson_iteminfo_free` — Free a handle returned by either loader.
- `crimson_iteminfo_entry_count` — Total number of items in the loaded `iteminfo.pabgb`.
- `crimson_iteminfo_lookup_string_key` — Look up the `string_key` for a given `ItemKey (u32)` and write it into `buf` (NUL-terminated UTF-8).
- `crimson_iteminfo_lookup_icon_path_hash` — Look up the first `item_icon_list[0].icon_path` for a given `ItemKey (u32)` and write the resulting `StringInfoKey` (u32 hash) into `*out_…
- `crimson_iteminfo_lookup_max_stack` — Look up the `max_stack_count` for a given `ItemKey (u32)` and write it into `*out_max_stack`.
- `crimson_iteminfo_lookup_look_detail_mission_info` — Look up the `look_detail_mission_info` (a `MissionKey` u32) for a given `ItemKey (u32)` and write it into `*out_mission_key`.
- `crimson_iteminfo_lookup_artifact_for_mission` — Reverse of [`crimson_iteminfo_lookup_look_detail_mission_info`]: given a `MissionKey (u32)`, return the `ItemKey (u32)` of the artifact wh…
- `crimson_iteminfo_lookup_socket_caps` — Look up the gamedata socket caps for `item_key`.
- `crimson_iteminfo_socket_allows_gem` — Check whether `gem_key` is in `item_key`'s gamedata-defined allowed-gem list (`DropDefaultData::socket_item_list`).
- `crimson_iteminfo_socket_allowed_gem_count` — Number of entries in `item_key`'s gamedata allowed-gem list.
- `crimson_iteminfo_socket_allowed_gem_at` — Read the allowed-gem itemkey at insertion index `idx` for `item_key`.
- `crimson_iteminfo_canonical_gem_count` — Number of itemkeys in the canonical gem set (sorted-ascending union of every item's `socket_item_list`).
- `crimson_iteminfo_canonical_gem_at` — Read the canonical gem itemkey at sorted-ascending index `idx`.
- `crimson_iteminfo_lookup_flags` — Read the static-flag bitmask for `item_key`.
- `crimson_iteminfo_lookup_summary` — One-shot lookup that fills the entire static-metadata snapshot for `item_key`.
- `crimson_iteminfo_get_entry` — Get the `(item_key, string_key)` pair at insertion index `idx`.

**`src/c_abi/knowledge_info.rs`**

- `crimson_knowledgeinfo_load_from_file` — Parse `knowledgeinfo.pabgb` from disk.
- `crimson_knowledgeinfo_load_from_bytes` — Parse knowledgeinfo bytes already in memory.
- `crimson_knowledgeinfo_load_from_bytes_with_header` — Parse in-memory `knowledgeinfo` bytes together with the table's `.staticinfoheader`.
- `crimson_knowledgeinfo_free` — Free a handle returned by either loader.
- `crimson_knowledgeinfo_entry_count` — Total number of knowledge rows in the loaded table.
- `crimson_knowledgeinfo_lookup_string_key` — Look up the internal name for a given `KnowledgeKey (u32)` and write it into `buf` (NUL-terminated UTF-8).
- `crimson_knowledgeinfo_merge_dlc_from_bytes` — Merge a DLC copy of the table (`<install>/dlc/<name>/0008`, e.g. `knowledgeinfo.dlc_01.staticinfobody` + `.staticinfoheader`) into a loade…
- `crimson_knowledgeinfo_entry_source` — Where entry `idx` came from: 0 = the base table, `n` = the `n`-th DLC merged with [`crimson_knowledgeinfo_merge_dlc_from_bytes`].
- `crimson_knowledgeinfo_lookup_display_name` — One-shot title resolution: chain `KnowledgeKey → internal name → hashlittle2 → PALOC u64 key → localized string`.
- `crimson_knowledgeinfo_get_entry` — Get the `(knowledge_key, internal_name)` pair at insertion index `idx`.

**`src/c_abi/main_quest_chapter.rs`**

- `crimson_main_quest_table_entry_count` — Total number of `(chapter, arc, mission)` rows in the curated table.
- `crimson_main_quest_table_get_entry` — Read the row at `idx`.
- `crimson_main_quest_chapter_for_arc` — Resolve a quest arc display title (the bold bullets in the source MD — e.g. "Trials of Kindness", "Hernand in Chaos", "Journey's End") to…
- `crimson_main_quest_chapter_for_mission` — Resolve a mission display title (e.g. "Where Rumors Gather", "Unfamiliar Lands") to its chapter heading.
- `crimson_main_quest_arc_for_mission` — Resolve a mission display title to its quest arc title (e.g. "Where Rumors Gather" → "Trials of Kindness").
- `crimson_main_quest_chapter_for_mission_key` — Resolve a `MissionKey` — the save-side key — to its chapter heading.
- `crimson_main_quest_arc_for_mission_key` — Resolve a `MissionKey` to its arc title — the empty string for Prologue missions, as with [`crimson_main_quest_arc_for_mission`].
- `crimson_main_quest_chapter_for_quest_key` — Resolve a `QuestKey` to its chapter heading: the key of an arc's quest (e.g. 1000027 `Quest_MeetAlustain_Test`, "Trials of Kindness") or o…
- `crimson_main_quest_table_get_entry_keys` — Read the game keys of the row at `idx` — the companion of [`crimson_main_quest_table_get_entry`], which returns its strings.

**`src/c_abi/mercenary_info.rs`**

- `crimson_mercenary_info_load_from_file` — Parse `mercenaryinfo.pabgb` + `.pabgh` from disk.
- `crimson_mercenary_info_load_from_bytes` — Parse mercenaryinfo pabgb + pabgh bytes already in memory.
- `crimson_mercenary_info_free` — Free a handle returned by either loader.
- `crimson_mercenary_info_entry_count` — Total number of `mercenaryinfo` rows in the loaded table.
- `crimson_mercenary_info_lookup_string_key` — Look up the internal template name for a given `MercenaryKey` and write it into `buf` (NUL-terminated UTF-8).
- `crimson_mercenary_info_get_entry` — Get the `(mercenary_key, internal_name)` pair at insertion index `idx`.

**`src/c_abi/mission_info.rs`**

- `crimson_missioninfo_load_from_file` — Parse `missioninfo.pabgb` from disk.
- `crimson_missioninfo_load_from_bytes` — Parse missioninfo bytes already in memory (preferred — the editor pulls them through PAZ extraction first, then calls in).
- `crimson_missioninfo_load_from_bytes_with_header` — Parse in-memory `missioninfo` bytes together with the table's `.staticinfoheader`.
- `crimson_missioninfo_free` — Free a handle returned by either loader.
- `crimson_missioninfo_entry_count` — Total number of missions in the loaded table.
- `crimson_missioninfo_lookup_string_key` — Look up the internal name for a given `MissionKey (u32)` and write it into `buf` (NUL-terminated UTF-8).
- `crimson_missioninfo_lookup_display_name` — One-shot title resolution: chain `MissionKey → internal name → hashlittle2 → PALOC u64 key → localized string` and write the result into `…
- `crimson_missioninfo_get_entry` — Get the `(mission_key, internal_name)` pair at insertion index `idx`.

**`src/c_abi/mod.rs`**

- `crimson_save_load_from_file` — Load and fully decode a `.save` file.
- `crimson_save_free` — Free a handle returned by [`crimson_save_load_from_file`].
- `crimson_save_list_inventory_items` — Flat-list every item slot across every `InventorySaveData` block in the save.
- `crimson_save_list_character_refs` — Flat-list every present `CharacterKey` field across every block in the save (top-level + nested via ObjectList / Locator).
- `crimson_save_get_mutation_version` — Read the handle's mutation counter — a monotonic `u64` that bumps by exactly 1 on every successful mutation through the C ABI surface ([`c…
- `crimson_save_get_version` — Scalar getter on the save handle (header field).
- `crimson_save_get_flags` — Scalar getter on the save handle (header field).
- `crimson_save_get_block_count` — Scalar getter on the save handle (header field).
- `crimson_save_get_hmac_ok` — HMAC verification result.
- `crimson_save_set_allow_hmac_rewrap` — Allow (`allow != 0`) or refuse (`0`, the default) writing this save when its HMAC did not verify on load.
- `crimson_save_get_block_info` — Fill `out` with the summary of the block at TOC `index`.
- `crimson_save_get_block_class_name` — Write the class name of the block at TOC `index` into `buf` as a NUL-terminated UTF-8 string.
- `crimson_save_get_block_json` — Serialize the full per-field decode of the block at TOC `index` as a JSON document.
- `crimson_save_set_scalar_field` — Overwrite the bytes of a fixed-size scalar field with `bytes`.
- `crimson_save_set_scalar_field_path` — Path-addressed variant of [`crimson_save_set_scalar_field`].
- `crimson_save_set_float3_field_path` — Set the value of an already-present `float3` (12-byte / 3 × f32) field.
- `crimson_save_set_float3_field_present` — Toggle the presence of a `float3` field.
- `crimson_save_set_float4_field_path` — Set the value of an already-present `float4` / `quaternion` (16-byte / 4 × f32) field.
- `crimson_save_set_float4_field_present` — Toggle the presence of a `float4` / `quaternion` field.
- `crimson_save_set_uint4_field_path` — Set the value of an already-present `uint4` (16-byte / 4 × u32) field.
- `crimson_save_set_uint4_field_present` — Toggle the presence of a `uint4` field.
- `crimson_save_set_scalar_fields_batch` — Apply many scalar mutations in one FFI round trip, sharing a single post-batch re-decode.
- `crimson_save_list_remove_element` — Remove element `element_idx` from the `object_list` field reached by `(block_idx, path[], field_idx)`.
- `crimson_save_list_remove_elements_batch` — Drop many `object_list` elements in one FFI round trip, sharing a single post-batch re-emit + re-decode.
- `crimson_save_list_clone_element` — Clone element `src_element_idx` of an `object_list` and insert the copy at `dst_element_idx`.
- `crimson_save_transplant_list_element` — Transplant one `object_list` element from a SOURCE save into a TARGET save's list, retargeting the element's embedded schema type-indices…
- `crimson_save_set_scalar_field_present` — Flip the mask bit of a fixed-size scalar field, inserting (or removing) the corresponding bytes in the enclosing block's payload.
- `crimson_save_set_object_list_present` — Flip the mask bit of an `object_list` field, creating (or removing) the corresponding list payload in the enclosing block.
- `crimson_save_set_scalar_fields_present_batch` — Apply many [`CrimsonScalarPresentBatchOp`] mutations in one FFI round trip, sharing a single post-batch re-emit + re-decode.
- `crimson_save_make_empty_element_bytes` — Produce the minimal valid bytes for a list element of `class_index`.
- `crimson_save_list_insert_element` — Insert a caller-supplied list-element bytes blob into an `object_list` field at `insert_at`.
- `crimson_save_dynamic_array_set_u32_elements` — Serialize the in-memory save back to `path` using the original nonce.
- `crimson_save_dynamic_array_get_u32_elements` — Read the contents of a `dynamic_array<u32>` field as a flat little-endian `u32` sequence.
- `crimson_save_set_inline_bytes_field` — Wholesale-replace the contents of an `inline_bytes` field (the schema's `meta_kind == 1` shape: a `u32 count` header followed by `count *…
- `crimson_save_get_inline_bytes_field` — Read the raw payload bytes of an `inline_bytes` field — the read counterpart to [`crimson_save_set_inline_bytes_field`].
- `crimson_save_write_to_file` — Uses `Save::write_with_nonce` so the on-disk layout matches what the game produced — HMAC re-computed, ChaCha20 re-applied.
- `crimson_save_begin_deferred_redecode` — Open a deferred-redecode batch on the save handle.
- `crimson_save_end_deferred_redecode` — Commit the deferred-redecode batch opened by [`crimson_save_begin_deferred_redecode`].
- `crimson_save_abort_deferred_redecode` — Abort the deferred-redecode batch opened by [`crimson_save_begin_deferred_redecode`], discarding every in-memory mutation since `begin_*`…
- `crimson_save_is_deferred_redecode_open` — Returns `1` (in `*out_open`) if a deferred-redecode batch is currently open on the handle, `0` otherwise.
- `crimson_house_info_load_from_file` — Load from a `.staticinfobody` path (the header beside it is read too). (`impl_name_only_bridge!`)
- `crimson_house_info_load_from_bytes` — Load from body + header bytes. (`impl_name_only_bridge!`)
- `crimson_house_info_free` — Free the handle. (`impl_name_only_bridge!`)
- `crimson_house_info_entry_count` — Number of rows. (`impl_name_only_bridge!`)
- `crimson_house_info_lookup_string_key` — Key → internal name (two-call buffer). (`impl_name_only_bridge!`)
- `crimson_house_info_get_entry` — Row at an index: key + internal name. (`impl_name_only_bridge!`)

**`src/c_abi/paloc.rs`**

- `crimson_paloc_load_from_file` — Load and parse a `.paloc` file from disk.
- `crimson_paloc_load_from_bytes` — Load and parse PALOC bytes that already live in memory.
- `crimson_paloc_free` — Free a handle returned by [`crimson_paloc_load_from_file`].
- `crimson_paloc_entry_count` — Total number of (key, value) pairs in the table.
- `crimson_paloc_lookup` — Look up the value for `key` and copy its UTF-8 bytes into `buf`.
- `crimson_paloc_get_entry` — Get the (key, value) pair at index `idx` in insertion order.

**`src/c_abi/part_prefab_dye_slot_info.rs`**

- `crimson_part_prefab_dye_slot_info_load_from_file` — Parse from disk.
- `crimson_part_prefab_dye_slot_info_load_from_bytes` — Parse bytes already in memory.
- `crimson_part_prefab_dye_slot_info_free` — Free a handle.
- `crimson_part_prefab_dye_slot_info_entry_count` — Total number of prefab rows.
- `crimson_part_prefab_dye_slot_info_lookup_slot_count` — Number of slots on the mesh row identified by `prefab_key`: one per sub-mesh.
- `crimson_part_prefab_dye_slot_info_lookup_prefab_name` — Prefab internal name (e.g. `"cd_phm_00_lb_00_0054"`).
- `crimson_part_prefab_dye_slot_info_lookup_slot_default_material` — Default material name for `slot_idx`'s `mat_idx` channel (`mat_idx ∈ {0, 1, 2}`).
- `crimson_part_prefab_dye_slot_info_lookup_slot_tail_name` — Tail name — next sub-prefab for non-final slots, full `.pac` path for the last slot.
- `crimson_part_prefab_dye_slot_info_lookup_slot_mat_indices` — Read the 3 material-index bytes into `out_indices[0..3]`.
- `crimson_part_prefab_dye_slot_info_lookup_slot_mask` — Read the first 3 mask bytes into `out_mask[0..3]`.
- `crimson_part_prefab_dye_slot_info_lookup_slot_mask_full` — Read a slot's **complete** mask into `buf` (12 bytes on 2.01+, 3 on 1.07-2.00), sized-buffer style.
- `crimson_part_prefab_dye_slot_info_lookup_slot_active_channels` — Which of `slot_idx`'s three material channels are dyeable, one byte (0 or 1) per channel into `out_flags[0..3]`: channel `ch` is active wh…
- `crimson_part_prefab_dye_slot_info_lookup_slot_dye_slot_bits` — `slot_idx`'s mask as bits: bit `n` of `*out_bits` is set when dye slot number `n` is dyeable on this slot.
- `crimson_part_prefab_dye_slot_info_lookup_dye_slot_bits` — The prefab's dye slots: bit `n` of `*out_bits` is set when any slot or extra layer of the prefab enables dye slot number `n`.
- `crimson_part_prefab_dye_slot_info_lookup_dye_slot_material` — Default material name (`"cloth"` / `"leather"` / `"metal"` / …) of dye slot number `dye_slot_no` on the prefab: the name of any primary-la…
- `crimson_part_prefab_dye_slot_info_lookup_slot_extra_layer_dye_slot_bits` — Extra layer `layer_idx`'s mask on `slot_idx` as bits, indexed by dye slot number like [`crimson_part_prefab_dye_slot_info_lookup_slot_dye_…
- `crimson_part_prefab_dye_slot_info_lookup_slot_extra_layer_count` — Number of extra (secondary) dye layers on `slot_idx`.
- `crimson_part_prefab_dye_slot_info_lookup_slot_extra_layer_material` — Default material name for extra layer `layer_idx`'s `mat_idx` channel (`mat_idx ∈ {0, 1, 2}`) on `slot_idx`.
- `crimson_part_prefab_dye_slot_info_lookup_slot_extra_layer_mask` — Read the first 3 of extra layer `layer_idx`'s mask bytes on `slot_idx` into `out_mask[0..3]`.
- `crimson_part_prefab_dye_slot_info_lookup_slot_extra_layer_mask_full` — Read an extra layer's **complete** mask into `buf` (12 bytes on 2.01+, 3 on 1.13-2.00), sized-buffer style.
- `crimson_part_prefab_dye_slot_info_lookup_slot_extra_layer_flag` — Read extra layer `layer_idx`'s trailing flag byte on `slot_idx`.
- `crimson_part_prefab_dye_slot_info_get_entry_key` — Read the prefab key at insertion index `idx` (PABGH on-disk order).

**`src/c_abi/part_prefab_dye_texture_pallete_info.rs`**

- `crimson_part_prefab_dye_texture_pallete_load_from_file` — Parse the pair from disk.
- `crimson_part_prefab_dye_texture_pallete_load_from_bytes` — Parse bytes already in memory.
- `crimson_part_prefab_dye_texture_pallete_free` — Free a handle.
- `crimson_part_prefab_dye_texture_pallete_entry_count` — Total number of palette rows in the table.
- `crimson_part_prefab_dye_texture_pallete_lookup_sub_count` — Number of sub-records inside the palette row keyed by `palette_key`.
- `crimson_part_prefab_dye_texture_pallete_lookup_sub_material_name` — Material identifier — `"cloth"`, `"leather"`, `"metal"`, … # Safety `handle` and `required` must be non-null; `buf` may be null iff `buf_l…
- `crimson_part_prefab_dye_texture_pallete_lookup_sub_icon_path` — UI icon DDS path (falls back to texture path for `palette_key=0`).
- `crimson_part_prefab_dye_texture_pallete_lookup_sub_texture_path` — Material texture DDS path used at runtime.
- `crimson_part_prefab_dye_texture_pallete_lookup_sub_variant_name` — Variant label inside a material — empty when absent, or `"wool"` / `"velvet"` / `"silk"`.
- `crimson_part_prefab_dye_texture_pallete_lookup_sub_variant_value` — Variant strength — `-1.0` is the "no variant" sentinel.
- `crimson_part_prefab_dye_texture_pallete_get_entry_key` — Read the palette key at insertion index `idx` (PABGH on-disk order).

**`src/c_abi/paver.rs`**

- `crimson_paver_read_from_file` — Parse `meta/0.paver` from disk into the four version components.
- `crimson_paver_read_from_bytes` — Parse a paver buffer already in memory (e.g. when the caller pulled the file through some other mechanism).
- `crimson_parser_target_gamedata_major` — The Crimson Desert gamedata `major` version this build's parsers target.
- `crimson_parser_target_gamedata_minor` — The Crimson Desert gamedata `minor` version this build's parsers target.
- `crimson_parser_min_gamedata_patch` — The lowest gamedata `patch` this build's parsers read on the target `(major, minor)`.
- `crimson_parser_supports` — `1` when this build's parsers read gamedata `major.minor.patch` (from [`crimson_paver_read_from_file`]), else `0`: the full gate (target m…
- `crimson_parser_compatible_gamedata_minors` — Write the full allow-list of gamedata `minor` versions this build's parsers can load into `out_buf`, and the count into `out_count`.

**`src/c_abi/paz.rs`**

- `crimson_paz_extract_file` — Extract a single file from a Crimson Desert pack group.
- `crimson_paz_list_npc_portraits` — List every NPC portrait DDS file in a PAZ group's PAMT.
- `crimson_paz_list_dir` — List every file in a single PAMT directory, with the metadata a caller needs to drive [`crimson_paz_extract_file`] (filename) and pre-size…

**`src/c_abi/positions.rs`**

- `crimson_save_list_field_positions` — Flat-list every positioned entity across the save's container classes (active character, mercenaries / mounts, field gimmicks).

**`src/c_abi/quest_gauge_info.rs`**

- `crimson_questgaugeinfo_load_from_file` — Parse `questgaugeinfo.pabgb` from disk.
- `crimson_questgaugeinfo_load_from_bytes` — Parse questgaugeinfo bytes already in memory.
- `crimson_questgaugeinfo_load_from_bytes_with_header` — Parse in-memory `questgaugeinfo` bytes together with the table's `.staticinfoheader`.
- `crimson_questgaugeinfo_free` — Free a handle returned by either loader.
- `crimson_questgaugeinfo_entry_count` — Total number of gauges in the loaded table.
- `crimson_questgaugeinfo_lookup_string_key` — Look up the internal name for a given `QuestGaugeKey (u32)` and write it into `buf` (NUL-terminated UTF-8).
- `crimson_questgaugeinfo_get_entry` — Get the `(gauge_key, internal_name)` pair at insertion index `idx`.

**`src/c_abi/quest_info.rs`**

- `crimson_questinfo_load_from_file` — Parse `questinfo.pabgb` from disk via the anchor-scan path.
- `crimson_questinfo_load_from_bytes` — Parse questinfo bytes already in memory via the anchor-scan path.
- `crimson_questinfo_load_from_bytes_with_header` — Parse in-memory `questinfo` bytes together with the table's `.staticinfoheader`.
- `crimson_questinfo_load_from_file_with_pabgh` — Parse `questinfo.pabgb` + `questinfo.pabgh` from disk — the **recommended** path.
- `crimson_questinfo_load_from_bytes_with_pabgh` — Parse questinfo bytes already in memory via the PABGH-driven path — the **recommended** loader for any caller that has both files.
- `crimson_questinfo_free` — Free a handle returned by either loader.
- `crimson_questinfo_entry_count` — Total number of quests in the loaded table.
- `crimson_questinfo_lookup_string_key` — Look up the internal name for a given `QuestKey (u32)` and write it into `buf` (NUL-terminated UTF-8).
- `crimson_questinfo_lookup_display_name` — One-shot title resolution: chain `QuestKey → internal name → hashlittle2 → PALOC u64 key → localized string`.
- `crimson_questinfo_get_entry` — Get the `(quest_key, internal_name)` pair at insertion index `idx`.

**`src/c_abi/region_info.rs`**

- `crimson_region_info_load_from_file` — Load from a `.staticinfobody` path (the header beside it is read too). (`impl_name_only_bridge!`)
- `crimson_region_info_load_from_bytes` — Load from body + header bytes. (`impl_name_only_bridge!`)
- `crimson_region_info_free` — Free the handle. (`impl_name_only_bridge!`)
- `crimson_region_info_entry_count` — Number of rows. (`impl_name_only_bridge!`)
- `crimson_region_info_lookup_string_key` — Key → internal name (two-call buffer). (`impl_name_only_bridge!`)
- `crimson_region_info_get_entry` — Row at an index: key + internal name. (`impl_name_only_bridge!`)

**`src/c_abi/reserve_slot_info.rs`**

- `crimson_reserve_slot_info_load_from_file` — Load from a `.staticinfobody` path (the header beside it is read too). (`impl_name_only_bridge!`)
- `crimson_reserve_slot_info_load_from_bytes` — Load from body + header bytes. (`impl_name_only_bridge!`)
- `crimson_reserve_slot_info_free` — Free the handle. (`impl_name_only_bridge!`)
- `crimson_reserve_slot_info_entry_count` — Number of rows. (`impl_name_only_bridge!`)
- `crimson_reserve_slot_info_lookup_string_key` — Key → internal name (two-call buffer). (`impl_name_only_bridge!`)
- `crimson_reserve_slot_info_get_entry` — Row at an index: key + internal name. (`impl_name_only_bridge!`)

**`src/c_abi/royal_supply_info.rs`**

- `crimson_royal_supply_info_load_from_file` — # Safety Path args must be NUL-terminated UTF-8 strings; out_handle non-null.
- `crimson_royal_supply_info_load_from_bytes` — # Safety `pabgb`/`pabgh` may be null iff length is 0; out_handle non-null.
- `crimson_royal_supply_info_free` — # Safety `handle` must be null or live and unfreed.
- `crimson_royal_supply_info_entry_count` — # Safety `handle` live; `out_count` non-null.
- `crimson_royal_supply_info_lookup_string_key` — # Safety `handle`/`required` non-null; `buf` may be null iff `buf_len == 0`.
- `crimson_royal_supply_info_get_entry` — # Safety `handle`/`out_key`/`required` non-null; `buf` may be null iff `buf_len == 0`.

**`src/c_abi/side_quest_faction.rs`**

- `crimson_side_quest_table_entry_count` — Total number of `(quest, faction)` rows in the curated table.
- `crimson_side_quest_table_get_entry` — Read the row at `idx`.
- `crimson_side_quest_faction_for_quest` — Resolve a side-quest display title (the `quest:` value in the source MD — e.g. "Record of the Greymanes", "Carl's Request") to its faction…
- `crimson_side_quest_quest_count_for_faction` — Number of side quests in the curated set that belong to `faction_name`.
- `crimson_side_quest_quest_at_for_faction` — The `idx`-th side quest title in `faction_name`'s curated list.
- `crimson_side_quest_faction_for_mission_key` — Resolve a `MissionKey` — the save-side key — to its faction name, independent of the display title, so it survives a retitle.
- `crimson_side_quest_faction_for_quest_key` — Resolve a `QuestKey` to its faction name.
- `crimson_side_quest_table_get_entry_key` — Read the game key of the row at `idx` — the companion of [`crimson_side_quest_table_get_entry`], which returns its strings.

**`src/c_abi/skill_info.rs`**

- `crimson_skillinfo_load_from_file` — Parse `skill.pabgh` + `skill.pabgb` from disk.
- `crimson_skillinfo_load_from_bytes` — Parse skill bytes already in memory (preferred — the editor pulls them through PAZ extraction first, then calls in).
- `crimson_skillinfo_free` — Free a handle returned by either loader.
- `crimson_skillinfo_entry_count` — Total number of skills in the loaded pair.
- `crimson_skillinfo_lookup_string_key` — Look up the entry name for a given `SkillKey (u32)` and write it into `buf` (NUL-terminated UTF-8).
- `crimson_skillinfo_get_entry` — Get the `(skill_key, name)` pair at PABGH on-disk index `idx`.

**`src/c_abi/stage_info.rs`**

- `crimson_stageinfo_load_from_file` — Parse `stageinfo.pabgb` from disk via the anchor-scan path.
- `crimson_stageinfo_load_from_bytes` — Parse stageinfo bytes already in memory via the anchor-scan path.
- `crimson_stageinfo_load_from_bytes_with_header` — Parse in-memory `stageinfo` bytes together with the table's `.staticinfoheader`.
- `crimson_stageinfo_load_from_file_with_pabgh` — Parse `stageinfo.pabgb` + `stageinfo.pabgh` from disk — the **recommended** path.
- `crimson_stageinfo_load_from_bytes_with_pabgh` — Parse stageinfo bytes already in memory via the PABGH-driven path — the **recommended** loader for any caller that has both files.
- `crimson_stageinfo_free` — Free a handle returned by either loader.
- `crimson_stageinfo_entry_count` — Total number of stages in the loaded table.
- `crimson_stageinfo_lookup_string_key` — Look up the internal name for a given `StageKey (u32)` and write it into `buf` (NUL-terminated UTF-8).
- `crimson_stageinfo_lookup_display_name` — One-shot title resolution: chain `StageKey → internal name → hashlittle2 → PALOC u64 key → localized string`.
- `crimson_stageinfo_get_entry` — Get the `(stage_key, internal_name)` pair at insertion index `idx`.

**`src/c_abi/store_info.rs`**

- `crimson_store_info_load_from_file` — Parse `storeinfo.pabgb` + `.pabgh` from disk.
- `crimson_store_info_load_from_bytes` — Parse storeinfo pabgb + pabgh bytes already in memory.
- `crimson_store_info_free` — Free a handle returned by either loader.
- `crimson_store_info_entry_count` — Total number of `storeinfo` rows in the loaded table.
- `crimson_store_info_lookup_string_key` — Look up the internal template name for a given `StoreKey` and write it into `buf` (NUL-terminated UTF-8).
- `crimson_store_info_get_entry` — Get the `(store_key, internal_name)` pair at insertion index `idx`.

**`src/c_abi/string_info.rs`**

- `crimson_string_info_load_from_file` — Parse a `stringinfo.pabgb` blob from disk.
- `crimson_string_info_load_from_bytes` — Parse stringinfo bytes already in memory (preferred — the editor pulls them through PAZ extraction first).
- `crimson_string_info_free` — Free a handle returned by either loader.
- `crimson_string_info_entry_count` — Total number of entries in the loaded `stringinfo.pabgb`.
- `crimson_string_info_lookup_by_hash` — Look up the resolved string for a given `StringInfoKey` (u32 hash) and write it into `buf` (NUL-terminated UTF-8).
- `crimson_string_info_get_entry` — Get the `(hash, value)` pair at insertion index `idx`.

**`src/c_abi/sub_level_info.rs`**

- `crimson_sublevelinfo_load_from_file` — Parse `sublevelinfo.pabgb` from disk.
- `crimson_sublevelinfo_load_from_bytes` — Parse sublevelinfo bytes already in memory.
- `crimson_sublevelinfo_load_from_bytes_with_header` — Parse in-memory `sublevelinfo` bytes together with the table's `.staticinfoheader`.
- `crimson_sublevelinfo_free` — Free a handle returned by either loader.
- `crimson_sublevelinfo_entry_count` — Total number of sub-level tracks in the loaded table.
- `crimson_sublevelinfo_lookup_string_key` — Look up the internal name for a given `SubLevelKey (u32)` and write it into `buf` (NUL-terminated UTF-8).
- `crimson_sublevelinfo_get_entry` — Get the `(sub_level_key, internal_name)` pair at insertion index `idx`.

**`src/c_abi/trigger_region_info.rs`**

- `crimson_trigger_region_info_load_from_file` — Load from a `.staticinfobody` path (the header beside it is read too). (`impl_name_only_bridge!`)
- `crimson_trigger_region_info_load_from_bytes` — Load from body + header bytes. (`impl_name_only_bridge!`)
- `crimson_trigger_region_info_free` — Free the handle. (`impl_name_only_bridge!`)
- `crimson_trigger_region_info_entry_count` — Number of rows. (`impl_name_only_bridge!`)
- `crimson_trigger_region_info_lookup_string_key` — Key → internal name (two-call buffer). (`impl_name_only_bridge!`)
- `crimson_trigger_region_info_get_entry` — Row at an index: key + internal name. (`impl_name_only_bridge!`)

<!-- symbol-index:end -->
