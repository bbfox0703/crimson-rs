# CrimsonForge Feature Gaps

What `D:\Github\crimsonforge` (Python toolkit) can decode/extract that `crimson-rs` cannot yet — focused on **game data the user can read out**, not Blender/mesh/audio/UI/installer plumbing.

Captured 2026-05-13 from a cross-repo survey. The intent is to record the gap so it can be triaged, not to plan a port.

---

## Status snapshot

crimson-rs covers the **archive layer** byte-for-byte:

- `binary/{pamt,papgt,paz,trie}` — container read/write/roundtrip
- `crypto/{checksum,chacha20}` — Jenkins hashlittle2 + ChaCha20
- `item_info/` — full schema parser for `iteminfo.pabgb` (105 fields)
- `skill_info/` — `skill.pabgb` + `skill.pabgh` with brute-forced tail sizes
- `string_info/` — `u32 hash → string` bridge
- `save/` — game save read/write

What it does **not** cover is the **semantic / cross-reference layer**: linking the keys that live inside `.pabgb` rows to the human-facing strings inside `.paloc`, or to the prefab/PAC/`.pab` files referenced by hash.

CrimsonForge has spent considerable RE effort on that layer. The biggest concrete gaps the user asked about — **NPC/character key ↔ display name** and **quest key ↔ quest name** — are both in that bucket.

---

## 1. Character key ↔ display name (NPC names)

**The chain in the game files**

```
characterinfo.pabgb       (gamedata, group 0008)
   ├── character_key    e.g. "CD_M0001_00_Ogre"
   ├── family_code     e.g. "M0001" → parsed from the key
   ├── prefab/pac refs (hashes resolved via PAMT 0009)
   └── loc_key          → numeric/symbolic PALOC entry
                          → display name string in
                            localizationstring_eng.paloc
                            (group 0020, language-specific groups: 0019/0020/0036…)
```

**What crimsonforge does with it**

- [`core/asset_catalog.py`](D:/Github/crimsonforge/core/asset_catalog.py) — builds `CharacterRecord(family_code, gender, likely_human, prefabs_by_slot, loc_key, app_id, display_name)` for every entry in `gamedata/characterinfo.pabgb`. Family code (`M0001`, `F0008`, etc.) is parsed out of the `app_id` and gender + "likely human" are inferred from prefab slots and the family code.
- [`core/character_asset_resolver.py`](D:/Github/crimsonforge/core/character_asset_resolver.py) — given a character key (`CD_M0001_00_Ogre`) or a fuzzy string (`ogre`, `hexe`), walks every PAMT for filename matches, then **content-scans the ~20 most-relevant `.pabgb` system tables** for the character key as a literal byte string. Returns a `CharacterAssetBundle` grouped into 14 presentation categories (Mesh / Skeleton / Morph / Appearance / Animation / Physics / Effects / Cutscene / Texture / UI / Database / Localization / Audio / Other).
- [`core/character_unlock_service.py`](D:/Github/crimsonforge/core/character_unlock_service.py), [`core/condition_patcher.py`](D:/Github/crimsonforge/core/condition_patcher.py) — workflow layers on top.

**What crimson-rs has**

| Piece | crimson-rs status |
| --- | --- |
| Read `characterinfo.pabgb` rows with a typed schema | ✗ — only `iteminfo.pabgb` / `skill.pabgb` have schema parsers |
| Resolve PALOC `loc_key` → display string | ✗ — PALOC is not parsed at all in `src/binary` (no `paloc.rs`) |
| Resolve prefab/PAC hashes via PAMT 0009 | partial — PAMT is parsed but no "hash → stem" helper |
| Content-scan PABGB tables for a literal key | ✗ — generic search not exposed |

**Gap size**: this is the single biggest user-visible omission. The full chain takes three parsers we don't have: `characterinfo.pabgb` schema, generic PALOC key-value reader, and the prefab-hash resolver.

---

## 2. Quest key ↔ quest name

**The chain in the game files**

```
questinfo.pabgb            (gamedata, group 0008)
questgroupinfo.pabgb
questgaugeinfo.pabgb
missioninfo.pabgb
wantedinfo.pabgb
   ├── numeric quest IDs
   ├── per-row HP / level / reward refs
   └── loc_key (numeric)   → localizationstring_eng.paloc
                             → quest name / description
```

Also relevant:

- Dialogue keys starting with `quest_`, `questdialog_*`, `quest_node_`, `onetimequest_` (PALOC entries — quest narrative text, voice line keys).
- `StaticInfo:Quest:…` and `StaticInfo:Character:…` tokens embedded inside dialogue strings (cross-references to other rows).

**What crimsonforge does with it**

- [`translation/localization_usage_index.py`](D:/Github/crimsonforge/translation/localization_usage_index.py) — categorises every PALOC key by **which `.pabgb` it appears in** by scanning the bytes of each gamedata table for the key. The category map (lines 57–102) enumerates the full set of game tables we have not parsed:
  ```
  questinfo, questgroupinfo, questgaugeinfo, missioninfo, wantedinfo
  skill, skillgroupinfo, skilltreeinfo, skilltreegroupinfo,
  buffinfo, statusinfo, statusgroupinfo, conditioninfo, jobinfo
  iteminfo, itemgroupinfo, itemuseinfo, storeinfo,
  crafttoolinfo, crafttoolgroupinfo, socketinfo, socketgroupinfo,
  dropsetinfo, inventory, equipslotinfo, equiptypeinfo,
  dyecolorgroupinfo, elementalmaterialinfo, royalsupply
  faction, factiongroup, factionnode, allygroupinfo, tribeinfo
  vehicleinfo
  regioninfo, uimaptextureinfo, sublevelinfo
  ```
- [`core/dialogue_catalog.py`](D:/Github/crimsonforge/core/dialogue_catalog.py) — parses every symbolic PALOC key (`questdialog_hello_00496`, `aidialogstringinfo_*`, `intro_*`, `epilogue_*`) into a conversation/scene/speaker structure. Exports 30k+ dialogue lines with speaker role, chapter, mentions of other characters via `StaticInfo:Character:…` tokens.
- [`tools/patch_quest_hp.py`](D:/Github/crimsonforge/tools/patch_quest_hp.py) — already locates and writes back into a `questinfo.pabgb` row via the same PAZ pipeline.

**What crimson-rs has**

| Piece | crimson-rs status |
| --- | --- |
| Schema parser for `questinfo.pabgb` (and the four other quest tables) | ✗ |
| PALOC key-value reader | ✗ |
| Quest-key → name resolver | ✗ |
| Generic gamedata table search-by-key | ✗ |

---

## 3. PALOC semantic extraction

**File**: [`core/paloc_parser.py`](D:/Github/crimsonforge/core/paloc_parser.py) (lines 121–187). Reads `.paloc` files into:

- Symbolic key ↔ text pairs (e.g. `questdialog_hello_00496` → "Hello there, traveller.")
- Numeric key ↔ text pairs (legacy IDs, 6+ digits)
- Filters header sentinels (`@`, `#`)

**crimson-rs**: there is no `paloc` module in `src/binary/`. The existing `docs/paloc-71-dev-items.md` discusses paloc translation gaps but the project never built its own parser. Roundtrip is achieved indirectly because everything goes through PAZ — but the **content** of paloc is opaque to us.

This is the foundational missing piece. Almost every other gap below requires it.

---

## 4. Generic `.pabgb` parser (schema-less)

**File**: [`core/pabgb_parser.py`](D:/Github/crimsonforge/core/pabgb_parser.py)

- Heuristic field detection per row: `uint32` / `int32` / `float32` / length-prefixed ASCII / hash / opaque blob.
- Handles both "simple" (sequential ID, fixed row size) and "hashed" (lookup-by-hash, variable row size) table layouts.
- Read-only — no roundtrip.

**crimson-rs**: only schema-aware parsers exist (`iteminfo`, `skill.pabgb`, `skill.pabgh`). For every other `.pabgb` file in the list under section 2, we have nothing — not even a "show me the rows as best-effort" view.

Trade-off: the schema-aware parsers are byte-identical roundtrip; the heuristic one is not. Both have a place.

---

## 5. Item catalog enrichment (beyond raw iteminfo)

**File**: [`core/item_catalog.py`](D:/Github/crimsonforge/core/item_catalog.py) (~1 kLOC)

What it produces on top of `iteminfo.pabgb`:

- **Item variants and leveling chains** from `multichangeinfo.pabgb` (`_0.am`, `_1.am` suffix chains linked back to base items).
- **Equipment class** inferred from `equiptypeinfo.pabgb` with confidence scores (Upperbody, Shield, Helm, …).
- **Prefab hash → PAC stem** resolution against the PAMT 0009 hash table — gives "this item points at `base.pac` / `base_sub01.pac`".
- **Icon discovery** by cross-scanning every PAMT for `itemicon_prefab_*.dds`, linked to base items and inherited to leveled variants.
- **Display names** by resolving `loc_key` against PALOC.
- **4-level category taxonomy** with raw_type + confidence.

**File**: [`core/item_index.py`](D:/Github/crimsonforge/core/item_index.py) — derived search index used by the Explorer: display_name ↔ internal_name ↔ PAC stems, plus a reverse `mesh file → items` map.

**crimson-rs**: we parse iteminfo into a flat list of 105-field rows. Everything above (variants, equip type, icons, display names, taxonomy, reverse mesh index) is downstream of parsers we don't have (`multichangeinfo`, `equiptypeinfo`, PALOC) plus the PAMT hash resolver.

---

## 6. Audio index ↔ dialogue key

**File**: [`core/audio_index.py`](D:/Github/crimsonforge/core/audio_index.py)

Parses `.wem` filenames in groups 0005 (Korean), 0006 (English), 0035 (Chinese):

```
nhm_adult_noble_1_questdialog_hello_00496.wem
└── voice prefix ──┘ └── PALOC key ─────────┘
    NPC class/age/gender   dialogue key
```

Produces a voice-prefix → dialogue-key map plus per-language coverage stats. Useful for "who voices line X?" / "which lines does NPC Y have?" queries.

**crimson-rs**: not scoped. Pure filename parsing once you can list the `.wem` entries — could be added cheaply on top of PAMT enumeration if we cared. The value is the linkage to PALOC, which we still don't have.

---

## 7. Prefab parser

**File**: [`core/prefab_parser.py`](D:/Github/crimsonforge/core/prefab_parser.py)

Reads `.prefab` files (Pearl Abyss ReflectObject serialization). Extracts:

- File references — mesh / skeleton / material paths (`.pac`, `.pab`, `.pam`, `.pamlod`, `.xml`, `.dds`).
- Equipment slot tags (Upperbody, Cloak, …) used for body-part hiding.
- Property / type names for the UI.

**crimson-rs**: not scoped. Required if we want to walk "characterinfo row → prefab → mesh files" without filename heuristics.

---

## 8. Other parsers (not directly relevant to the question, listed for completeness)

These are out of scope for "what game data can be surfaced", but exist in CrimsonForge:

| Module | Reads | Purpose |
| --- | --- | --- |
| `core/skeleton_parser.py` | `.pab` | Bone hierarchy + bind matrices (mesh rigging) |
| `core/animation_parser.py` (+ v2, v3) | `.paa`, `.paa_metabin` | Animation keyframes |
| `core/mesh_parser.py`, `mesh_exporter.py` | `.pac`, `.pam` | Mesh → FBX pipeline |
| `core/havok_parser.py`, `havok_tag0*.py` | `.hkx` | Physics shapes |
| `core/paseq_parser.py` | `.paseq`, `.paseqc` | Sequencer / cutscene |
| `core/pac_xml_parser.py` | `.pac_xml` | PAC sidecar metadata |
| `core/pabc_parser.py` | `.pabc` | Morph |
| `core/font_builder.py`, `audio_converter.py`, `dds_reader.py` | fonts / audio / textures | Asset tooling |
| `core/navmesh_parser.py` | navmesh | Pathfinding data |

These are all Blender / asset-export oriented and not on the critical path for "show me character and quest names".

---

## Triage / what an MVP "close the question" port would need

If the goal is just to answer **"give me NPC names and quest names from the archive"** in crimson-rs, the minimum viable set is:

1. `binary/paloc.rs` — parse `.paloc` into `{key → string}` maps (symbolic + numeric). Exposed to Python as `read_paloc(bytes) -> dict[str, str]`.
2. `character_info/` — schema parser for `characterinfo.pabgb` modeled on the existing `item_info/` layout. Extract at minimum `character_key`, `loc_key`, `family_code`, prefab hash refs.
3. `quest_info/` — schema parser for `questinfo.pabgb` (and ideally `questgroupinfo`, `missioninfo`). At minimum `quest_id`, `loc_key`.
4. A high-level Python helper that walks PAMT, decrypts the relevant entries, joins `*.loc_key` against the PALOC map, and returns enriched records.

The character/quest schemas are unknown to us today — the same RE workflow that produced `item_info/` and `skill_info/` (hexpat patterns in `references/`, `plcli`, cross-version diffs against the 1.04/1.05/1.06 reference dumps) would apply.

`pabgb_parser.py`'s heuristic field-typing approach (section 4) is an attractive **fallback** for the long tail of tables we won't write full schemas for. Worth considering whether to mirror it.

---

## Out of scope (skipped per the brief)

Excluded from this report:

- Mesh / animation / skeleton / Havok exporters (FBX pipeline)
- DDS, audio, font asset tooling
- Blender helpers, Cheat Engine scripts, memory scanners (`tools/probe_*`, `cheat_engine.py`, `memory_snapshot.py`, `find_*`, `scan_*`)
- UI code (`ui/`)
- Installer / launcher plumbing
