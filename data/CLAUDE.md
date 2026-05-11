# data/ — Claude context

User-facing notes are in [`README.md`](README.md); this file is for AI assistants continuing the work.

## What's safe to commit here

- `keys.txt` — pure list of integers, no copyrighted content. ✅
- `keys-<previous-version>.txt` — per-version frozen snapshots of `keys.txt`, kept for cross-version reference and so users with saved CE tables can still find the right index ordering for older patches. Same integer-only content; same safe-to-commit rationale.
- `internal_name_overrides.json` (hypothetical, not yet present) — community-curated friendly names for dev items. Could be added to humanise the `string_key` fallback in `scripts/export_for_ce.py`.

## What is NOT safe to commit here (or anywhere)

Anything derived from `iteminfo.pabgb`, `*.paloc`, or other paz-archive contents. These are Pearl Abyss assets. Examples:
- `items.jsonl` — even though it's just JSON, it contains Korean item descriptions and English item names from paloc.
- `paloc_*.json` — translation tables.
- `iteminfo_*.pabgb` — raw game binaries.
- Any `output*.txt` derived from paloc lookups.
- `v1.0?-mem*.CEM` — raw process-memory dumps. Even if used only for RE diagnostics, dropping them in the repo would leak game-side memory layout. Keep these on disk under `data/` for current-session use, but never `git add` them; `.gitignore` already covers `*.CEM`.

Such files belong under `out/` (gitignored) or `references/samples/` (gitignored).

## When `keys.txt` needs replacing

After every game patch that adds/removes items. The patch notes will tell you "X new items added"; if the count or values in `keys.txt` haven't changed, the existing file is still valid.

If parser fixes for a new patch require regenerating, snapshot the previous `keys.txt` first as `keys-<old-version>.txt` next to the new one — the *order* matters because Cheat Engine table dropdowns reference items by index. A saved table from a prior version will desync if `keys.txt` ordering changes between game patches.

## In-memory array layout (relevant when re-RE'ing the dumper)

The game keeps the itemKey table in process memory as a packed `[u32 itemKey][u32 offset]` array — the second u32 is the byte offset of that item inside `iteminfo.pabgb`. This was confirmed in the 1.06 RE by dumping the array region: `offset` grows monotonically by ~600 B per slot, and the last live slot's `offset` matches the anchor scan's offset for the final item exactly. Two practical consequences:

- The CE Lua dumper terminates on `offset` monotonicity break — this is what cleanly delimits the array end. Sentinel checks (`key == 0xFFFFFFFF` / `key == 0`) are defensive backstops, not the primary signal.
- Anchor offsets produced by `scripts/export_for_ce.py` should match the `offset` field of the same key in the live-memory array. A future cross-check (`--verify-with-mem-dump`) could exploit this as a smoke test that the anchor scanner hasn't drifted.

## Past versions

| Patch | `keys.txt` shape | Where the snapshot lives now |
|---|---|---|
| 1.04 | Not preserved — older CE-table flow read `items.jsonl` directly | — |
| 1.05 | 6,236 keys | [`keys-1.05.01.txt`](keys-1.05.01.txt) (frozen 2026-05) |
| **1.06** | **6,253 keys** | [`keys.txt`](keys.txt) (current) |

When 1.07 ships and reorders keys, snapshot the current file to `keys-1.06.txt` before overwriting.
