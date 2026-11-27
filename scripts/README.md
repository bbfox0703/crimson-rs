# scripts/

Driver and diagnostic Python scripts for working with Crimson Desert game data.

## What you almost always want to run

```powershell
python scripts\export_for_ce.py
```

One command, three deliverables (`out\items.jsonl`, a `paloc_*.json` per language, an `output*.txt` CE dropdown list per language), covering the base game plus any installed DLC (`(DLC1)`-prefixed, after the base items). See the project [`README.md`](../README.md) for the full description.

**Prerequisites:**
1. Crimson Desert installed (the script auto-detects D:/F:/E:/C: SteamLibrary paths; override with `--game-dir`).
2. `crimson_rs` Python wheel installed — build into a **Python 3.12** venv (the wheel is `abi3` for ≥ 3.12): `maturin develop --release` inside `.venv`, or `maturin build --release && pip install target/wheels/crimson_rs-*.whl` from the repo root. Run this script from that same env.
3. Optional: `data/keys.txt` (bundled CE dump of the in-game key order). The order comes from the `iteminfo` headers; the dump is only cross-checked. See [`../data/README.md`](../data/README.md).

## Index

### Production scripts

| Script | Purpose | Inputs | Outputs |
|---|---|---|---|
| [`export_for_ce.py`](export_for_ce.py) | **Main one-shot pipeline.** Extracts iteminfo + paloc for the base game and each DLC, builds items.jsonl + paloc JSONs + CE dropdown lists in CE item-ID order (base header rows, then DLC-only rows). Falls back to the base game when any DLC file is missing. | game install (`data\keys.txt` optional cross-check) | `out\iteminfo*.pabgb`, `out\items.jsonl`, `out\paloc_*.json`, `out\output*.txt` |
| [`update_ce_table.py`](update_ce_table.py) | **Run after `export_for_ce.py`.** Rewrites the CE table in place: replaces the `ItemID.` / `ItemID-zh_tw.` / `ItemID-ja.` dropdowns, recomputes every `define(Name, #<CE ID>) // name/<item key>` from the key in its comment, reports the items that entered or left each ID range, and regenerates the shared "Item data table" AA record (per-item max stack + Quest / Kuku flags) that the amount scripts read. | `out\output*.txt`, `out\items.jsonl` (or `--props-bin <archived version>`), the `.CT` | the `.CT` (backup in `out\ce-backup\`), `out\ce_update_report.txt`, `out\ce_item_data.tsv` |
| [`build_items_jsonl.py`](build_items_jsonl.py) | Standalone anchor-based items.jsonl builder (subset of `export_for_ce.py`). | `keys.txt` + raw `iteminfo.pabgb` | `items.jsonl` |
| [`gamedata_layout.py`](gamedata_layout.py) | **Shared helper, not a runnable script.** Resolves where the gamedata tables and paloc files live in a given install — 2.01 renamed the directory and every extension, so every script that touches an archive goes through this. Newest layout first, falls back to the pre-2.01 one. | live install | — |
| [`dump_gamedata_keys.py`](dump_gamedata_keys.py) | Snapshot per-version key lists for the 30 non-iteminfo gamedata tables (skill, mission, quest, stage, gimmick, character, faction triple, store, mercenary, dye triple, niche bridges, …). Auto-detects four PABGH shapes. Mirrors the role `keys.txt` plays for iteminfo. | live install | `data\gamedata-keys-<ver>\<table>.txt` |

### Diagnostic / analysis scripts

| Script | Purpose |
|---|---|
| [`anchor_diff.py`](anchor_diff.py) | Locate every item from `keys.txt` in the current binary and cross-reference with a baseline `items.jsonl` (any version). Reports added/removed/renamed items + per-item byte sizes. |
| [`analyze_per_item.py`](analyze_per_item.py) | For each anchored item chunk, run the parser independently and classify the result as `ok` / `leftover:N` / `fail:<path>`. Used to drive parser improvements. |
| [`probe_runtime_items.py`](probe_runtime_items.py) | **Read-only** (`ReadProcessMemory`) check of the running game: walks the in-memory item key array and reports which runtime item-object offsets hold the key, `max_stack_count`, `item_importance_kind` and the inventory tab, verified against `items.jsonl`. Re-run on a patch to see whether the offsets in [`CLAUDE.md`](CLAUDE.md) ("Runtime item objects") still hold. Takes `--array <key array address>`. |
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
# 1. Run the exporter. The order comes from the iteminfo headers (base, then DLC-only rows).
python scripts\export_for_ce.py
# 2. Optional cross-check: re-dump the key array from the running game (CE: the standalone
#    My-Cheat-Tables/Crimson Desert/Dump in-game item key array.CEA next to CrimsonDesert.CT) and pass it;
#    the run reports whether it agrees. Since 2.06 the dump is the whole base + DLC array in one keys.txt,
#    so --dlc-keys is not needed.
python scripts\export_for_ce.py --keys path\to\keys.txt
#    Through 2.05.01 the dumper was an entry in CrimsonDesert.CT and wrote two segments:
#    python scripts\export_for_ce.py --keys path\to\keys.txt --dlc-keys path\to\keys-dlc.txt
# 3. Update the CE table itself: dropdowns, item defines, the item data table.
python scripts\update_ce_table.py --ct "D:\Github\My-Cheat-Tables\Crimson Desert\CrimsonDesert.CT"
#    If the parser can't read the new patch yet (items.jsonl has keys only), take max stack and
#    inventory tab from the last archived version, matched by item key:
python scripts\update_ce_table.py --ct "...\CrimsonDesert.CT" --props-bin "X:\Crimson Desert\gamedata-bin\<prev>"
# 4. Read out\ce_update_report.txt: lines marked [!] (a define whose comment and ID disagreed, a
#    range whose start is past its end) and any item that entered or left a range.
```

The table's conventions that make step 3 work:

- An item define carries its key at the end of its comment: `define(Copper, #2075) // 銅幣/1`. The comment wins; a define without `/<key>` is left alone.
- A range row is `dq <first>, <last>, <amount>, <force>`. Items the data table protects (Quest tab, Kuku tab, max stack 1, or not in the table) are skipped unless `<force>` is 1. Single-item exception rows always apply.
- The "Item data table (auto-generated, do not edit)" record is replaced wholesale on every run, wherever it sits in the table. The two amount scripts assemble its script themselves (Lua `autoAssemble`), reloading it when the game process changes or the table text changed, so the record never needs ticking. (They used to set the record's `Active`, which stopped working once it was moved into a group with `moHideChildren` / `moDeactivateChildrenAsWell`; observed 2026-11-21, CE's reason not established.) A table without the record gets it as the first child of the "Crimson Desert  /  …" helper group.
- Keys the base game and the DLC share keep their base CE ID and base properties, the same as in the game's own key array.

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
