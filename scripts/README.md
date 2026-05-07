# scripts/

Driver and diagnostic Python scripts for working with Crimson Desert game data.

## What you almost always want to run

```powershell
python scripts\export_for_ce.py
```

One command, three deliverables (`out/items.jsonl`, three `paloc_*.json`, three `output*.txt`). See the project [`README.md`](../README.md#crimson-desert-105--one-shot-ce-table-data-export) for the full description.

**Prerequisites**:
1. Crimson Desert installed (the script auto-detects D:/F:/E:/C: SteamLibrary paths; override with `--game-dir`)
2. `crimson_rs` Python wheel installed (`maturin build --release && pip install target/wheels/crimson_rs-*.whl` from the repo root)
3. `data/keys.txt` (bundled — 6,236 itemKey list dumped from game memory; see [`../data/README.md`](../data/README.md))

## Index

### Production scripts

| Script | Purpose | Inputs | Outputs |
|---|---|---|---|
| [`export_for_ce.py`](export_for_ce.py) | **Main one-shot pipeline.** Extracts iteminfo + paloc, builds items.jsonl + paloc JSONs + CE dropdown lists. | game install + `data/keys.txt` | `out/iteminfo.pabgb`, `out/items.jsonl`, `out/paloc_*.json`, `out/output*.txt` |
| [`build_items_jsonl.py`](build_items_jsonl.py) | Standalone anchor-based items.jsonl builder (subset of `export_for_ce.py`). | `keys.txt` + raw `iteminfo.pabgb` | `items.jsonl` |

### Diagnostic / analysis scripts

| Script | Purpose |
|---|---|
| [`anchor_diff.py`](anchor_diff.py) | Locate every item from `keys.txt` in a 1.05 binary and cross-reference with a 1.04 baseline. Reports added/removed/renamed items + anchor-derived per-item byte sizes. |
| [`analyze_per_item.py`](analyze_per_item.py) | For each anchored item chunk, run the parser independently and classify the result as `ok` / `leftover:N` / `fail:<path>`. Used to drive parser improvements. |
| [`find_unknown_items.py`](find_unknown_items.py) | List items missing a paloc 0x70 name. |
| [`compare_community_paloc.py`](compare_community_paloc.py) | Diff the community `item_names.json` against paloc 0x70 to spot 1.05 renames and items the community handled but paloc didn't. |
| [`probe_paloc_for_keys.py`](probe_paloc_for_keys.py) | For a given key list, dump every paloc entry on those keys (any type byte). |
| [`probe_paloc_types.py`](probe_paloc_types.py) | Tally the lower-byte type distribution across all paloc entries. |
| [`probe_all_paloc_groups.py`](probe_all_paloc_groups.py) | Search every group `0NNN/0.paz` for `localizationstring_<lang>.paloc` to spot patch overlays. |
| [`probe_kor_fallback.py`](probe_kor_fallback.py) | Check kor / chs / rus / deu / fra paloc files for 0x70 entries on a given key list (used to confirm dev items have no localization in any language). |
| [`probe_iteminfo_names.py`](probe_iteminfo_names.py) | Inspect `item_name.default` field for given keys in a 1.04 baseline. |
| [`check_fallback_names.py`](check_fallback_names.py) | Sanity-check whether type-byte 0x30 is a usable fallback for missing 0x70 entries (it isn't — see [`CLAUDE.md`](CLAUDE.md)). |
| [`list_pamt_dirs.py`](list_pamt_dirs.py) | List directories and files inside one group's PAMT (`game_dir/0NNN/0.pamt`). |
| [`list_all_paloc.py`](list_all_paloc.py) | List every `.paloc` file across all groups, with sizes — useful for finding which group ships which language. |

### Parser-improvement scripts (1.05 reverse-engineering)

| Script | Purpose |
|---|---|
| [`probe_new_layout.py`](probe_new_layout.py) | Validate the 1.05 ItemInfo variant tail (CString `new_icon_path` + branched body) against every anchor. Reports per-cluster pass/fail counts and the u32 length distribution. |
| [`dump_post_bytes.py`](dump_post_bytes.py) | Hex-dump bytes between end-of-`max_endurance` and end-of-chunk for items in given post-size clusters. Useful for inspecting items that don't fit the new layout (`+88 / +54 / +93` leftover, ammo_mid_block fail cases). Note: only works for items in the `new_icon_path.length == 0` branch. |
| [`refine_discriminator.py`](refine_discriminator.py) | Within the length==0 branch, search for a static field-value predicate that distinguishes the 18 ammo / projectile items (which carry an extra 22-byte `ammo_mid_block`) from misc Class B items. The runtime parser instead peeks the `FF FF` trailer sentinel. |
| [`debug_post31.py`](debug_post31.py) | List the 18 items with `post == 31` (length==0 + 22-byte ammo_mid_block + trailer + count) and confirm the runtime ammo detector finds each one. |
| [`inspect_leftover_bytes.py`](inspect_leftover_bytes.py) | For each "leftover:N" bucket, print the exact bytes the parser left unconsumed — tells you what new fields are missing. |

### Common arguments

Every diagnostic script accepts `--game-dir <path>` to point at the Crimson Desert install. Most also take an `--out` directory or input path arguments — run with `-h` for the full list.

## Typical workflows

### Refresh CE dropdown after a game patch

```powershell
# 1. Re-dump itemKey order from the running game (in CE — see Mydev-Cheat-Engine-Tables/dump_item_keys.CEA)
# 2. Replace data/keys.txt with the new dump (or pass --keys path/to/new/keys.txt)
# 3. Run the exporter
python scripts\export_for_ce.py
```

### Investigate parser failures after a game patch

```powershell
# Compare item layout between the previous and the new patch
python scripts\anchor_diff.py `
    --keys data\keys.txt `
    --pabgb out\iteminfo.pabgb `
    --baseline out\baselines\1.05\items.jsonl `
    --out out\anchors.json

# Per-item analysis to see how often the parser fits cleanly vs leaves
# trailing bytes vs fails mid-item — guides what fields to add/remove.
python scripts\analyze_per_item.py `
    --anchors out\anchors.json `
    --pabgb out\iteminfo.pabgb

# After hypothesising a new tail layout, validate it across every item
# end-to-end before touching Rust:
python scripts\probe_new_layout.py `
    --anchors out\anchors.json `
    --pabgb out\iteminfo.pabgb

# Hex-dump the bytes around the trailer for items that don't fit the new
# layout (e.g. the +88 / +54 / +93 leftover clusters) to figure out what
# extra fields are present:
python scripts\inspect_leftover_bytes.py `
    --anchors out\anchors.json `
    --pabgb out\iteminfo.pabgb `
    --leftover 88
```

Per-version reference data (extracted iteminfo + paloc names per game
version) lives under [`../out/baselines/`](../out/baselines/). Each
sub-directory (`1.04/`, `1.05/`, …) holds the `items.jsonl` snapshot for
that patch — useful as `--baseline` input to `anchor_diff.py`. These
files are gitignored (they ship Pearl Abyss content) and should NOT be
deleted; they are how `anchor_diff.py` correlates items across patches.

### Find a string the game displays — which paloc holds it?

```powershell
python scripts\probe_paloc_for_keys.py `
    --game-dir "D:\SteamLibrary\steamapps\common\Crimson Desert" `
    --unknown out\unknown_items.json `
    --lang eng
```
