# Crimson Desert - Explorer

[![Platform](https://img.shields.io/badge/Platform-Windows%20x64-0078D4)](https://www.microsoft.com/windows)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![Built with Claude Code](https://img.shields.io/badge/Built%20with-Claude%20Code-CC785C?logo=claude)](https://claude.ai/code)
[![Game Version](https://img.shields.io/badge/Game-Crimson%20Desert%201.05%E2%80%932.00-D63B3B)](https://www.playcrimsondesert.com/)

> **Personal playground.** This fork is where I poke at Crimson Desert internals at my own pace; the work here isn't intended to flow back upstream. Use it as you like, but don't expect upstream PRs from this branch.

## Overview

A Rust library (with Python bindings via PyO3) that pulls together the tools I use to read, modify, and re-pack Crimson Desert game files.

Currently supports:
- Extracting and parsing `.paz` archives, plus PAPGT / PAMT / PALOC containers
- Parsing, editing, and byte-perfect roundtripping `iteminfo.pabgb` from Python
- Save-file (`save.save`) read / write — ChaCha20 + HMAC + LZ4 header, full body decoder and encoder; idempotent full-body roundtrip on every live save tested
- **34 C-ABI key resolvers** used by the save editor:
  - **Title resolvers** (PALOC hash-hop / cat-byte strip): Skill, Mission, Quest, Stage, Knowledge, QuestGauge, SubLevel, GimmickInfo, Character (+ `resolve_portrait` matcher); plus the `checksum` extern "C" wrapper.
  - **Dye / appearance gamedata**: DyeColorGroupInfo, PartPrefabDyeTexturePallete, PartPrefabDyeSlot, and the combined **ItemKey → list of PartPrefabKey** join (iteminfo + stringinfo + partprefabdyeslotinfo).
  - **Faction gamedata**: FactionNode (1,119), FactionSpawnData (131), FactionRelationGroup (5) + per-row sibling-reference matrix.
  - **Catalog / template names** (Save Editor browser surfaces; row counts are 2.00): Store (436), Mercenary (21), House (12), RoyalSupply (4), CraftTool (19) + CraftToolGroup (12), TriggerRegion (13), GamePlayVariable (59), GlobalGameEvent (191) + GlobalGameEventGroup (12), GameAdvice (478) + GameAdviceGroup (8), ReserveSlot (28), Region (1,007), ItemGroup (1,597).
  - Plus an `iteminfo` socket / artifact-challenge / canonical-gem advisory surface, a PAZ-layer `crimson_paz_list_npc_portraits` enumerator, and PABGH-driven `_load_from_*_with_pabgh` loaders for `stageinfo` (51,860 entries) and `questinfo` (1,097) that return the in-game-authoritative key list instead of the anchor-scan path's under-count.
- **Save-handle helpers**: `list_inventory_items` and `list_character_refs` (flat enumeration of every present CharacterKey field across all blocks); `mutation_version` for snapshot staleness; a **deferred-redecode batch ABI** that suspends the per-call `decode_blocks` for bulk-mutation workflows — one decode at end of batch instead of N; typed `set_float3_field_path` / `set_float4_field_path` / `set_uint4_field_path` composite-scalar setters; `set_inline_bytes_field` + `set_scalar_field_path` / `_present` for finer-grained mutation; JSON `get_block_json` inlines dynamic-array u32/u64 contents directly.
- **Crimson Desert 1.05–2.00**: PALOC / PAZ / PAMT containers and the save format (v2 / flags 0x0080) are stable across the whole range. The `iteminfo.pabgb` parser **targets the latest patch only** (currently 2.00, 6,810 items, byte-perfect; `serialize_iteminfo` roundtrips every item). Older snapshots have schema differences and **will not parse with this build** — the per-patch drift list is the table under [Per-patch parser](#per-patch-parser-currently-200) below, and loading an older binary needs that patch's wheel via the recipe in [`docs/historical-parser-setup.md`](docs/historical-parser-setup.md).
- **2.00 is the first *major* version bump** — `meta/0.paver` goes `1 / 18 / 0` → `2 / 0 / 0`, so the gamedata `minor` **resets to 0** and can no longer identify a schema on its own. A `crimson_parser_target_gamedata_major()` C-ABI bridge was added beside the existing `crimson_parser_target_gamedata_minor()`; consumers should gate on the `(major, minor)` pair.
- **Save body**: 1.10 drifted one block (`ContentsMiscSaveData` widened its object-list leading-pad 3→4 bytes; handled by the decoder). **No patch since has touched it** — 1.11 through 2.00 are clean. See [`docs/archive/1.05-parser-history.md`](docs/archive/1.05-parser-history.md) for the full RE history and [`scripts/CLAUDE.md`](scripts/CLAUDE.md) for the per-patch validation table.
- Generating Cheat Engine dropdown lists (item id → name) covering every in-game item key

## Build & features

The crate ships **three composable Cargo features**, and the FFI surfaces are **opt-in** — the default build is a bare pure-Rust core with no foreign bindings at all:

| Feature | Default? | What it adds | How to build |
|---|---|---|---|
| `default = []` | — | Bare pure-Rust core (parsers + containers); no C-ABI, no PyO3 | `cargo build` |
| `c_abi` | **off** | `extern "C"` surface for native / C# consumers → `crimson_rs.dll` + `crimson_rs.lib` (PyO3 **not** linked) | `cargo build --release --features c_abi` |
| `python` | **off** | PyO3 bindings (`extension-module`) → the importable `crimson_rs` wheel | `maturin develop` / `maturin build --release` |

So **PyO3 is not built by default** — it only links when the `python` feature is active (which `[tool.maturin]` auto-enables for the wheel). Likewise the C-ABI surface only exists under `--features c_abi`. The two consumer features are independent and composable; CI lints their union (`--features c_abi,python`). Nothing is published — build what you need: `maturin build --release` produces the `python` wheel, `cargo build --release --features c_abi` produces the **`.dll` / `.lib`** in one pass (zero transitive DLL dependencies — see [`docs/c-sharp-nativeaot-integration.md`](docs/c-sharp-nativeaot-integration.md)).

## One-shot CE table data export

**Prerequisites:** Crimson Desert installed locally + this repo + the bundled [`data/keys.txt`](data/keys.txt) (already included; see [`data/README.md`](data/README.md) for how to refresh it after a game patch).

```powershell
python scripts\export_for_ce.py
```

That single command writes everything `Generate_item_id_list.ps1` needs to `out/`. The script auto-discovers every `localizationstring_<lang>.paloc` the game ships (15 languages as of 2.00 — eng, jpn, kor, zho-tw, zho-cn, …) and emits one `paloc_<lang>.json` plus one CE dropdown file per language. Counts below are for **2.00** (6,810 keys; the per-patch progression — 6,236 in 1.05, 6,253 in 1.06 / 1.07, 6,314 in 1.08 / 1.09 (identical key list), 6,325 in 1.10, 6,333 in 1.11, 6,483 in 1.12, 6,508 in 1.13 / 1.14 / 1.15, 6,581 in 1.16, 6,572 in 1.17, 6,573 in 1.18, 6,810 in 2.00 — is tabulated in [`scripts/CLAUDE.md`](scripts/CLAUDE.md)):

| File | Contents | 2.00 coverage |
|---|---|---|
| `iteminfo.pabgb` | Raw decompressed binary from group 0008 | — |
| `items.jsonl` | One item per line; `key` + `string_key` always populated, parsed fields when the parser succeeds | 6,810 (100%) |
| `paloc_<lang>.json` (one per language) | `{item_key: name}` lookup tables (paloc 0x70). Filenames use the raw paloc lang code, e.g. `paloc_eng.json`, `paloc_jpn.json`, `paloc_kor.json`, `paloc_zho-tw.json`, `paloc_zho-cn.json`, … | 6,988 each |
| `output.txt` (eng) + `output_<suffix>.txt` (one per remaining language) | CE dropdown lists `idx:name/key`. eng → `output.txt`; jpn → `output_ja.txt`; kor → `output_ko.txt`; zho-tw → `output_zh-tw.txt`; everything else falls through to its raw lang code | 6,810 each |

The CE dropdown coverage is full because the script falls back to `string_key` for the ~71 dev/QA items the game itself doesn't ship localized names for (in any of the 15 languages — see [`docs/paloc-71-dev-items.md`](docs/paloc-71-dev-items.md) for the analysis). 21 additional items are paloc-placeholder rows whose localized name in every language is the literal string `"Name"` — those still show as `<idx>:Name/<key>` in the dropdown lists.

Override the game install path or `keys.txt` location with `--game-dir` and `--keys`. Pass `--keys ''` to skip anchor mode and fall back to lossy parsing (lower coverage, useful when no `keys.txt` is available).

## Repository layout

```
crimson-rs/
├─ src/                    Rust library (PyO3 bindings → crimson_rs.pyd)
├─ python/crimson_rs/      Python wrapper package
├─ docs/                   Format references + API docs (api.md, archive-format.md, c-abi-surface.md, …)
│   └─ archive/                ← completed-RE write-ups + historical notes
├─ scripts/                Driver + diagnostic scripts (this is what most users run)
│   ├─ export_for_ce.py        ← one-shot exporter (the main entry point)
│   ├─ anchor_diff.py          ← key-by-key cross-version anchor analysis
│   ├─ analyze_per_item.py     ← per-item parser-fit diagnostic
│   ├─ archive/                ← retired scripts kept as cross-version diff templates
│   └─ ...                     (see scripts/README.md for the full index)
├─ data/                   Bundled inputs (keys.txt, lookup tables)
├─ references/             Hex-pattern files (and locally-kept iteminfo samples)
└─ out/                    Per-run generated output (gitignored — game data, do not commit)
```

## Python Library

A Python binding is provided to parse, modify, and write back `iteminfo_decompressed.pabgb` files.

### Installation

**Build from source** — there is no published wheel. The rolling "Development Build" pre-release this section used to link was retired on 2026-08-09 (traffic showed no external consumers), so the only supported path is a local build from a Python 3.12 venv:

```bash
python -m maturin build --release && pip install target/wheels/crimson_rs-*.whl
```

For development, `maturin develop --release` installs it editable into the active venv. The wheel is `abi3` for Python ≥ 3.12, so the venv must be 3.12 or newer.

### Quick Start

```python
import crimson_rs

# Parse all items from a binary file
items = crimson_rs.parse_iteminfo_from_file("iteminfo_decompressed.pabgb")
print(f"Loaded {len(items)} items")

# Each item is a plain dict with all fields accessible
item = items[0]
print(item["key"])         # 2200
print(item["string_key"])  # "Pyeonjeon_Arrow"
print(item["item_type"])   # 0
print(item["item_tier"])   # 0

# Nested types are also dicts
print(item["item_name"]["category"])  # localization category
print(item["item_name"]["index"])     # localization index
print(item["item_name"]["default"])   # default string value

# Arrays are plain lists
print(item["item_tag_list"])          # [123, 456, ...]
print(item["price_list"])             # [{"key": ..., "price": {...}}, ...]

# Optional fields are None when absent
print(item["docking_child_data"])     # None or dict

# Modify any field
item["item_tier"] = 5
item["max_stack_count"] = 999
item["item_name"]["default"] = "Custom Name"

# Write modified items back to a file (byte-perfect roundtrip)
crimson_rs.write_iteminfo_to_file(items, "output.pabgb")

# Or get raw bytes
data = crimson_rs.serialize_iteminfo(items)

# Parse from bytes
items = crimson_rs.parse_iteminfo_from_bytes(data)
```

### Filtering and Searching

```python
import crimson_rs

items = crimson_rs.parse_iteminfo_from_file("iteminfo_decompressed.pabgb")

# Find items by type
weapons = [i for i in items if i["item_type"] == 1]

# Find items by name substring
arrows = [i for i in items if "Arrow" in i["string_key"]]

# Find high-tier items
rare_items = [i for i in items if i["item_tier"] >= 3]

# Export to CSV
import csv
with open("items.csv", "w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["key", "string_key", "item_type", "item_tier"])
    for item in items:
        w.writerow([item["key"], item["string_key"], item["item_type"], item["item_tier"]])
```

### Per-patch parser (currently 2.00)

`parse_iteminfo_from_bytes` targets the **latest game patch only** — currently 2.00 (6,810 / 6,810 items, byte-perfect; `serialize_iteminfo` roundtrips every item). Older patches have schema differences and **will not parse with this build**:

| Patch | Items | Schema drift vs previous |
|---|---|---|
| 1.05 | 6,236 | `+` `icon_path_alt` (StringInfoKey) + `unk_flag: u8` inside each `ItemIconData`; `+` `unk_pre_pattern_key: u32` + `unk_pre_pattern_flag: u8` before `pattern_description_data_list` |
| 1.06 | 6,253 | none (parser unchanged from 1.05) |
| 1.07 | 6,253 | none (same key list as 1.06) |
| 1.08 | 6,314 | `−` `extract_additional_drop_set_info: u32`; `+` `is_equip_quick_slot_visible: u8` between `is_housing_only` and `quick_slot_index`; `+` trailing `unk_post_summon_tag: u8` inside `DockingChildData` (only present when `docking_child_data.tag = 1`, ~385 of 6,314 items) |
| 1.09 | 6,314 | none (key list byte-identical to 1.08; content-only delta, no schema drift) |
| 1.10 | 6,325 | `−` `money_icon_path: StringInfoKey` (between `map_icon_path` and `use_map_icon_alert`); `+` `unk_post_icon_path: u32` inside `UnitData` (between `icon_path` and `item_name`, in every populated `MoneyTypeDefine`) |
| 1.11 | 6,333 | `+` `unk_post_apply_drop_stat_type: u8` (boolean, reads 0/1) between `apply_drop_stat_type` and `drop_default_data`. No save-body drift. |
| 1.12 | 6,483 | `+` `SubItem type_id==16` payload-free variant (both sites); `+` unconditional `unk_pre_max_endurance: u32`; `+` `unk_pre_gimmick_visual: u32` gated on `equip_type_info != 0 \|\| item_type == 74`; `+` `enchant_data_list` N−1 inter-element u32 separators (`EnchantDataList`). No save-body drift. |
| 1.13 | 6,508 | **structural relocation**: `SubItem type_id==17`; `prefab_data_list` + `gimmick_visual_prefab_data_list` **merged into one `MergedPrefabVisualData` list relocated to the item end** (after `repair_data_list`); `+` constant `0xff00` item tail. `enchant_data_list` + gated `unk_pre_gimmick_visual` stay put. No save-body drift. |
| 1.14 | 6,508 | none — **content-only** patch (iteminfo layout byte-identical to 1.13; item field **values** changed, `knowledgeinfo` −1 key). The 1.13 parser reads it byte-perfectly; only `PARSER_TARGET_GAMEDATA_MINOR` bumped 13→14. No save-body drift. |
| 1.15 | 6,508 | none — **content-only** patch (iteminfo layout byte-identical; item field **values** changed, iteminfo SHA `c7ae5543…` vs 1.14 `de621624…`; all 30 gamedata key tables byte-identical to 1.14 — zero key changes). Only `PARSER_TARGET_GAMEDATA_MINOR` bumped 14→15. No save-body drift. |
| 1.16 | 6,581 | **four iteminfo drifts + one skill drift.** iteminfo: `−` head-side `inventory_info` (u16); `−` `DockingChildData.unk_post_summon_tag: u8` (conditional — only the 391 items with `docking_child_data` present); `+` a `u32 + u8 + CArray<UnkPreRespawnData> + u8` block before `respawn_time_seconds`, which also **swapped order with `unk_pre_max_endurance`**; `+` `inventory_info_list: [u16; 9]` at the item end, absorbing the 1.13-era constant `unk_tail` as slot 8. skill: `+` `PostBuff.unk_pre_damage_type: u8` before `damage_type`. No save-body drift. |
| 1.17 | 6,572 | none — **content-only** patch (**−9** items: keys 1004912–1004920, the nine `Item_Set_*_Tier0_Reminiscence` entries). `skill.pabgb`/`.pabgh` byte-identical to 1.16; of the 6,572 surviving items 6,435 are byte-identical and 137 changed values only, with **zero** size-changed. No save-body drift. |
| 1.18 | 6,573 | `+` `MergedPrefabVisualData.unk_pre_is_craft_material: u32` between `tribe_gender_list` and the 3-byte flag tail — unconditional, and reads the same constant `0xeac5e173` (the "empty string" Jenkins sentinel) on every element. +1 item (1005446 `Demian_Greyfur_Fabric_Cloak_II`). Skill content-only (2,013 → 2,027 entries). No save-body drift. |
| **2.00** | **6,810** | **two iteminfo drifts** (+237 items, none removed). `+` `SubItem type_id==18` payload-free variant — the same renumbering 1.12 (16) and 1.13 (17) did, and in 2.00 *every* site that read 17 reads 18. `+` an always-zero `u32` (`unk_pre_max_endurance_a`) **directly ahead of** `unk_pre_max_endurance`. ⚠️ That position is not decidable from the byte diff — the insert lands in an all-zero run, so three placements all round-trip byte-perfectly; only this one keeps `unk_pre_max_endurance` reading `0x01000000` on exactly the 59 `Trade_*_PackedInVehicle` items **and** `respawn_time_seconds` reading 0 / −1 / 604800. Skill content-only (2,027 → 2,046 entries, format still `WithField58`). No save-body drift. First **major** version bump — see the Overview note on `crimson_parser_target_gamedata_major()`. |

To parse a previous-patch binary, install that patch's wheel as a sibling package via the recipe in [`docs/historical-parser-setup.md`](docs/historical-parser-setup.md) — `pip install --target=.crimson_rs_107 …` and `import` from that path. Two helper modes are also available for diagnostic / recovery work on the current target:

```python
# Tracked mode — returns byte-range metadata for each parsed field; used by
# the diagnostic scripts in scripts/.
res = crimson_rs.parse_iteminfo_tracked(data)

# Lossy mode — best-effort. Skips items the parser cannot consume cleanly
# and continues from the next plausible item start. Useful if a future game
# patch breaks parsing on a subset of items.
res = crimson_rs.parse_iteminfo_lossy(data)
print(f"parsed {len(res['items'])}, skipped {len(res['errors'])}")
```

For a robust pipeline that anchors items by the in-game item-key dump rather than purely by parsing, use [`scripts/export_for_ce.py`](scripts/export_for_ce.py).

### API Reference

See [docs/api.md](docs/api.md) for the full Python API documentation.

# Credit

Thanks to [LukeFZ](https://github.com/LukeFZ).

This is a personal-playground fork. The repos below are where the real upstream work happens — if you're looking for a maintained crimson-rs to build on, start there, not here:

- [pycrimson](https://github.com/LukeFZ/pycrimson) — the original Python reference implementation
- [potter420/crimson-rs](https://github.com/potter420/crimson-rs) — the Rust upstream this fork branched from