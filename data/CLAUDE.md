# data/ — Claude context

User-facing notes are in [`README.md`](README.md); this file is for AI assistants continuing the work.

## What's safe to commit here

- `keys.txt` — pure list of integers, no copyrighted content. ✅
- `internal_name_overrides.json` (hypothetical, not yet present) — community-curated friendly names for dev items. Could be added to humanise the `string_key` fallback in `scripts/export_for_ce.py`.

## What is NOT safe to commit here (or anywhere)

Anything derived from `iteminfo.pabgb`, `*.paloc`, or other paz-archive contents. These are Pearl Abyss assets. Examples:
- `items.jsonl` — even though it's just JSON, it contains Korean item descriptions and English item names from paloc.
- `paloc_*.json` — translation tables.
- `iteminfo_*.pabgb` — raw game binaries.
- Any `output*.txt` derived from paloc lookups.

Such files belong under `out/` (gitignored) or `references/samples/` (gitignored).

## When `keys.txt` needs replacing

After every game patch that adds/removes items. The patch notes will tell you "X new items added"; if the count or values in `keys.txt` haven't changed, the existing file is still valid.

If parser fixes for a new patch require regenerating, snapshot the previous `keys.txt` first — the *order* matters because Cheat Engine table dropdowns reference items by index. A saved table from a prior version will desync if `keys.txt` ordering changes between game patches.

## Past versions

The 1.04 `keys.txt` was not preserved (the user's older CE table flow read items.jsonl directly). The 1.05 version here was the first generation. If a 1.06 ships and reorders keys, drop a copy of the current 1.05 `keys.txt` into `out/baselines/1.05/keys.txt` before overwriting.
