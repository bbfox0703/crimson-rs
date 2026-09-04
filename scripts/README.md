# scripts/

Driver and diagnostic Python scripts for working with Crimson Desert game data.

## What you almost always want to run

```powershell
python scripts\export_for_ce.py
```

One command, three deliverables (`out\items.jsonl`, three `paloc_*.json`, three `output*.txt`). See the project [`README.md`](../README.MD) for the full description.

**Prerequisites:**
1. Crimson Desert installed (the script auto-detects D:/F:/E:/C: SteamLibrary paths; override with `--game-dir`).
2. `crimson_rs` Python wheel installed — build into a **Python 3.12** venv (the wheel is `abi3` for ≥ 3.12): `maturin develop --release` inside `.venv`, or `maturin build --release && pip install target/wheels/crimson_rs-*.whl` from the repo root. Run this script from that same env.
3. `data/keys.txt` (bundled — 6,333 itemKey list dumped from game memory; see [`../data/README.md`](../data/README.md)).

## Index

### Production scripts

| Script | Purpose | Inputs | Outputs |
|---|---|---|---|
| [`export_for_ce.py`](export_for_ce.py) | **Main one-shot pipeline.** Extracts iteminfo + paloc, builds items.jsonl + paloc JSONs + CE dropdown lists. | game install + `data\keys.txt` | `out\iteminfo.pabgb`, `out\items.jsonl`, `out\paloc_*.json`, `out\output*.txt` |
| [`build_items_jsonl.py`](build_items_jsonl.py) | Standalone anchor-based items.jsonl builder (subset of `export_for_ce.py`). | `keys.txt` + raw `iteminfo.pabgb` | `items.jsonl` |
| [`gamedata_layout.py`](gamedata_layout.py) | **Shared helper, not a runnable script.** Resolves where the gamedata tables and paloc files live in a given install — 2.01 renamed the directory and every extension, so every script that touches an archive goes through this. Newest layout first, falls back to the pre-2.01 one. | live install | — |
| [`dump_gamedata_keys.py`](dump_gamedata_keys.py) | Snapshot per-version key lists for the 30 non-iteminfo gamedata tables (skill, mission, quest, stage, gimmick, character, faction triple, store, mercenary, dye triple, niche bridges, …). Auto-detects four PABGH shapes. Mirrors the role `keys.txt` plays for iteminfo. | live install | `data\gamedata-keys-<ver>\<table>.txt` |

### Diagnostic / analysis scripts

| Script | Purpose |
|---|---|
| [`anchor_diff.py`](anchor_diff.py) | Locate every item from `keys.txt` in the current binary and cross-reference with a baseline `items.jsonl` (any version). Reports added/removed/renamed items + per-item byte sizes. |
| [`analyze_per_item.py`](analyze_per_item.py) | For each anchored item chunk, run the parser independently and classify the result as `ok` / `leftover:N` / `fail:<path>`. Used to drive parser improvements. |
| [`inspect_leftover_bytes.py`](inspect_leftover_bytes.py) | For each `leftover:N` bucket, print the exact bytes the parser left unconsumed — tells you what new fields are missing. |

### Localization / paloc scripts

| Script | Purpose |
|---|---|
| [`find_unknown_items.py`](find_unknown_items.py) | List items missing a paloc 0x70 name. |
| [`compare_community_paloc.py`](compare_community_paloc.py) | Diff the community `item_names.json` against paloc 0x70 to spot renames and items the community handled but paloc didn't. |
| [`probe_paloc_for_keys.py`](probe_paloc_for_keys.py) | For a given key list, dump every paloc entry on those keys (any type byte). |
| [`probe_paloc_types.py`](probe_paloc_types.py) | Tally the lower-byte type distribution across all paloc entries. |
| [`probe_all_paloc_groups.py`](probe_all_paloc_groups.py) | Search every group `0NNN/0.paz` for `localizationstring_<lang>.paloc` to spot patch overlays. |
| [`probe_kor_fallback.py`](probe_kor_fallback.py) | Check kor / chs / rus / deu / fra paloc files for 0x70 entries on a given key list. |
| [`probe_iteminfo_names.py`](probe_iteminfo_names.py) | Inspect `item_name.default` field for given keys in a baseline. |
| [`check_fallback_names.py`](check_fallback_names.py) | Sanity-check whether type-byte 0x30 is a usable fallback for missing 0x70 entries (it isn't — see [`../docs/paloc-71-dev-items.md`](../docs/paloc-71-dev-items.md)). |

### Listing utilities

| Script | Purpose |
|---|---|
| [`list_pamt_dirs.py`](list_pamt_dirs.py) | List directories and files inside one group's PAMT (`game_dir/0NNN/0.pamt`). |
| [`list_all_paloc.py`](list_all_paloc.py) | List every `.paloc` file across all groups, with sizes. |

### Archived scripts

[`archive/`](archive/) holds (a) cross-version diff *templates* tied to the 1.04 → 1.05 transition and (b) scripts that validated and then dis-proved the wrong "variant tail" hypothesis during 1.05 RE. They're kept on disk as reference but **not** wired into any active workflow. See [`archive/README.md`](archive/README.md) for the per-script index and [`../docs/archive/1.05-parser-history.md`](../docs/archive/1.05-parser-history.md) for the full story.

## Common arguments

Every diagnostic script accepts `--game-dir <path>` to point at the Crimson Desert install. Most also take an `--out` directory or input path arguments — run with `-h` for the full list.

## Typical workflows

### Refresh CE dropdown after a game patch

```powershell
# 1. Re-dump itemKey order from the running game (CE — see Mydev-Cheat-Engine-Tables/dump_item_keys.CEA).
# 2. Replace data\keys.txt with the new dump (or pass --keys path/to/new/keys.txt).
# 3. Run the exporter.
python scripts\export_for_ce.py
```

### Investigate parser failures after a game patch

```powershell
# 1. Anchor every key in the new binary, cross-reference with the previous baseline.
python scripts\anchor_diff.py `
    --keys data\keys.txt `
    --pabgb out\iteminfo.pabgb `
    --baseline out\baselines\1.05\items.jsonl `
    --out out\anchors.json

# 2. Per-item analysis — how often the parser fits cleanly vs leaves trailing
#    bytes vs fails mid-item. Guides what fields to add/remove.
python scripts\analyze_per_item.py `
    --anchors out\anchors.json `
    --pabgb out\iteminfo.pabgb

# 3. If anything has leftover, dump the unconsumed bytes to figure out what fields are missing.
python scripts\inspect_leftover_bytes.py `
    --anchors out\anchors.json `
    --pabgb out\iteminfo.pabgb `
    --leftover 88
```

For a fail-mode check: **start with the anchor scanner**, not the schema. The 1.05 RE wasted significant effort on phantom schema drift before realizing the issue was in `looks_like_item_start`. See [`../docs/archive/1.05-parser-history.md`](../docs/archive/1.05-parser-history.md) Phase 3.

### Find a string the game displays — which paloc holds it?

```powershell
python scripts\probe_paloc_for_keys.py `
    --game-dir "D:\SteamLibrary\steamapps\common\Crimson Desert" `
    --unknown out\unknown_items.json `
    --lang eng
```

## Per-version reference data

`out/baselines/<version>/items.jsonl` snapshots are gitignored (they ship Pearl Abyss content) but should NOT be deleted locally — `anchor_diff.py` correlates items across patches by reading them. Each sub-directory (`1.04/`, `1.05/`, …) holds the canonical `items.jsonl` snapshot for that patch.
