# data/

Bundled inputs that travel with the repo so the iteminfo export pipeline runs out of the box on any machine that has a Crimson Desert install.

| File | What it is | Source | Size |
|---|---|---|---|
| `keys.txt` | Current-version Crimson Desert itemKey list (one decimal key per line, in in-game order). Tracks the latest game patch — currently **1.06** with 6,253 keys. | Dumped from `CrimsonDesert.exe` process memory via Cheat Engine (see Generation below) | ~53 KB |
| `keys-1.05.01.txt` | Snapshot of the 1.05 keys file kept for cross-version reference / saved-CE-table compatibility. Do not regenerate. | Frozen from `keys.txt` before the 1.06 refresh | ~53 KB |

## How `keys.txt` is used

`scripts/export_for_ce.py` reads `keys.txt` and uses each key as an *anchor* in the live `iteminfo.pabgb` binary — it scans for the u32 key + valid `string_key` prefix, locates every item exactly, then parses each item chunk independently. This decouples the user-facing pipeline (items.jsonl + CE dropdown) from parser completeness, so the output is 100% covering even when the Rust parser doesn't yet understand the full layout.

Order matters: the index of a key in `keys.txt` is the *in-game item index* used by the Cheat Engine dropdown. The `output.txt` line `123:Some Name/1004899` means index 123 = key 1004899 = "Some Name".

## How to regenerate after a game patch

The game ships an updated `iteminfo.pabgb` with each patch and may add/remove/reorder itemKeys. When that happens:

1. Launch the game (any save / main menu is fine).
2. Open Cheat Engine and attach to `CrimsonDesert.exe`.
3. Load the **Dump Item Keys** entry from the CE table at
   `D:\Github\Mydev-Cheat-Engine-Tables\Crimson Desert\` (source: `dump_item_keys.CEA`).
4. Toggle `[ENABLE]` — the script AOBScans for the loaded itemKey array, then walks it
   slot-by-slot. The in-game array is `[u32 itemKey][u32 offset]` where the second
   field is the byte offset of the item inside `iteminfo.pabgb`. The Lua auto-terminates
   on the first slot whose offset is non-monotonic (= one past the real array end),
   so what lands in `keys.txt` is exactly the live array — **no manual trimming required**.
   It also stops on `key == 0xFFFFFFFF` / `key == 0` / page fault as defensive backstops.
5. Replace this file with that fresh `keys.txt` (or pass `--keys path\to\new\keys.txt`
   to the exporter without copying). Before overwriting, consider snapshotting the
   outgoing version as `keys-<previous-patch>.txt` next to the new one (the bundled
   `keys-1.05.01.txt` is an example of this pattern).

The format is plain text, one decimal `uint32` per line. A clean dump has exactly N lines for N in-game items (e.g. 6,253 in 1.06). If the file contains trailing `4294967295` sentinels or other garbage past the real array end, `export_for_ce.py`'s anchor scanner will emit `no_anchor` fallback records for those — visible in the parser-status line — but the pipeline still produces aligned output. Cleaner is better; if you see `no_anchor > 0`, re-run the Lua or hand-trim.

## Why this is committed (the only piece of game-derived data that is)

`keys.txt` is **a list of integer ids**, not asset data — it carries no copyrighted strings, art, or text content. It's also the smallest, most stable input the pipeline needs (~53 KB and rarely changes).

By contrast everything in `out/` (`iteminfo.pabgb`, `items.jsonl`, `paloc_*.json`, `output*.txt`) IS extracted Pearl Abyss content and is gitignored — `out/` is generated each run.
