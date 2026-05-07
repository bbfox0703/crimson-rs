# Crimson Desert - Explorer

[![Platform](https://img.shields.io/badge/Platform-Windows%20x64-0078D4)](https://www.microsoft.com/windows)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![Built with Claude Code](https://img.shields.io/badge/Built%20with-Claude%20Code-CC785C?logo=claude)](https://claude.ai/code)
[![Game Version](https://img.shields.io/badge/Game-Crimson%20Desert%201.05-D63B3B)](https://www.playcrimsondesert.com/)

## Overview

This library is a poor man attempts and assembling various tools to explore and understand Crimson Desert game files.

It is a work in progress and currently supports:
- Extracting and parsing .paz files (game archives)
- Parsing and editing iteminfo binary data via Python
- **Crimson Desert 1.05** (5/2 patch): 100% perfect parse on all 6,236 items; byte-perfect roundtrip via `serialize_iteminfo`. See [`docs/1.05-parser-history.md`](docs/1.05-parser-history.md) for the RE story and [`scripts/CLAUDE.md`](scripts/CLAUDE.md) for the next-session pickup notes.
- Generating Cheat Engine dropdown lists (item id → name) covering all 6,236 in-game item keys

## Crimson Desert 1.05 — One-shot CE table data export

**Prerequisites:** Crimson Desert installed locally + this repo + the bundled [`data/keys.txt`](data/keys.txt) (already included; see [`data/README.md`](data/README.md) for how to refresh it).

```powershell
python scripts\export_for_ce.py
```

That single command writes everything `Generate_item_id_list.ps1` needs to `out/`:

| File | Contents | Coverage |
|---|---|---|
| `iteminfo.pabgb` | Raw decompressed binary from group 0008 | — |
| `items.jsonl` | One item per line; `key` + `string_key` always populated, parsed fields when the parser succeeds | 6,236 (100%) |
| `paloc_eng.json` / `paloc_zho-tw.json` / `paloc_jpn.json` | `{item_key: name}` lookup tables (paloc 0x70) | 6,389 each |
| `output.txt` / `output_zh-tw.txt` / `output_ja.txt` | CE dropdown lists `idx:name/key` | 6,236 each |

The CE dropdown coverage is full because the script falls back to `string_key` for the ~71 dev/QA items the game itself doesn't ship localized names for (in any of 14 languages — see [`scripts/CLAUDE.md`](scripts/CLAUDE.md) for the analysis).

Override the game install path or `keys.txt` location with `--game-dir` and `--keys`. Pass `--keys ''` to skip anchor mode and fall back to lossy parsing (lower coverage, useful when no `keys.txt` is available).

## Repository layout

```
crimson-rs/
├─ src/                    Rust library (PyO3 bindings → crimson_rs.pyd)
├─ python/crimson_rs/      Python wrapper package
├─ docs/                   Format references + RE history (api.md, archive-format.md, 1.05-parser-history.md, …)
├─ examples/               Standalone Python examples
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

Download the wheel from the [latest release](../../releases/tag/develop) and install:

```bash
pip install crimson_rs-0.1.0-cp312-abi3-win_amd64.whl
```

Or build from source: `python -m maturin build --release && pip install target/wheels/crimson_rs-*.whl`.

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

### 1.05 parser

`parse_iteminfo_from_bytes` parses 1.05 binaries to 100% (6,236 / 6,236 items, byte-perfect roundtrip via `serialize_iteminfo`). Two helper modes are also available for diagnostic / recovery work:

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

Thanks to [LukeFZ](https://github.com/LukeFZ)
Reference:
- [pycrimson](https://github.com/LukeFZ/pycrimson)
- [potter420/crimson-rs](https://github.com/potter420/crimson-rs) — upstream
- [NattKh/crimson-rs](https://github.com/NattKh/crimson-rs) — fork with April-2026 game-update patches
