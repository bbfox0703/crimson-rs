# data/

Bundled inputs that travel with the repo so the 1.05 export pipeline runs out of the box on any machine that has a Crimson Desert install.

| File | What it is | Source | Size |
|---|---|---|---|
| `keys.txt` | Crimson Desert 1.05 itemKey list (one decimal key per line, in in-game order) | Dumped from `CrimsonDesert.exe` process memory via Cheat Engine (see Generation below) | ~53 KB |

## How `keys.txt` is used

`scripts/export_for_ce.py` reads `keys.txt` and uses each key as an *anchor* in the 1.05 `iteminfo.pabgb` binary — it scans for the u32 key + valid `string_key` prefix, locates every item exactly, then parses each item chunk independently. This decouples the user-facing pipeline (items.jsonl + CE dropdown) from parser completeness, so the output is 100% covering even when the Rust parser doesn't yet understand the full 1.05 layout.

Order matters: the index of a key in `keys.txt` is the *in-game item index* used by the Cheat Engine dropdown. The `output.txt` line `123:Some Name/1004899` means index 123 = key 1004899 = "Some Name".

## How to regenerate after a game patch

The game ships an updated `iteminfo.pabgb` with each patch and may add/remove/reorder itemKeys. When that happens:

1. Launch the game (any save / main menu is fine).
2. Open Cheat Engine and attach to `CrimsonDesert.exe`.
3. Load the **Dump Item Keys** entry from the CE table at
   `D:\Github\Mydev-Cheat-Engine-Tables\Crimson Desert\` (source: `dump_item_keys.CEA`).
4. Toggle `[ENABLE]` — the script auto-AOBScans for the loaded itemKey array and writes the result to `keys.txt` next to the table.
5. Replace this file with that fresh `keys.txt` (or pass `--keys path\to\new\keys.txt` to the exporter without copying).

The format is plain text, one decimal `uint32` per line. The CE script emits a few sentinel values (`0xFFFFFFFF` or `0`) past the real array end; the exporter's parser stops on the first sentinel automatically, so you don't need to trim manually.

## Why this is committed (the only piece of game-derived data that is)

`keys.txt` is **a list of integer ids**, not asset data — it carries no copyrighted strings, art, or text content. It's also the smallest, most stable input the pipeline needs (50 KB and rarely changes).

By contrast everything in `out/` (`iteminfo.pabgb`, `items.jsonl`, `paloc_*.json`) IS extracted Pearl Abyss content and is gitignored. See [`../out/README.md`](../out/) is intentionally not present — `out/` is generated each run.
