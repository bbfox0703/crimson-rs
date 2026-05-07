# scripts/ — Claude context

This file documents the *current state* of the 1.05 work for any future AI assistant continuing it. The user-facing documentation is in [`README.md`](README.md); this is the engineering side.

## Status of the 1.05 parser

The 5/2 patch (Crimson Desert 1.05) changed the `ItemInfo` binary layout in ways that aren't fully understood yet. The current Rust parser handles the changes that are known:

- `ItemIconData` grew by 5 bytes per entry: a second `StringInfoKey` (`icon_path_alt`) right after `icon_path`, plus a trailing `unk_flag: u8`.
- `SubItem` accepts a new tag value `15` (treated as the existing `None` variant — both 14 and 15 carry no payload).
- A 27-byte block is read between `max_endurance` and `repair_data_list`, currently modelled as `3× u8 + ItemKey + 5× u32`. **This block is correct for some items but not all.** See "what we don't know" below.

With those changes, raw `parse_iteminfo_from_bytes` will throw on most 1.05 items. The two production-side workarounds are:

1. **`parse_iteminfo_lossy(bytes)`** — added in `src/python.rs`. Walks the binary, falls back to a byte-pattern scan (`u32 key + u32 small length + ASCII string_key + NUL`) on each parser error, jumps to the next plausible item start, and continues. Returns `{items, spans, errors}`.

2. **Anchor-based pipeline (`scripts/export_for_ce.py`)** — uses the CE-dumped `data/keys.txt` (in-game-ordered list of all 6,236 itemKeys) to locate every item by its key value in the binary, then parses each chunk independently. Items the parser can't consume cleanly fall back to a minimal record `{key, string_key, _index, _anchor_off, _anchor_size, _status}` so downstream tools (the CE dropdown generator) still get every item. **This is what makes the user-facing pipeline give 100% coverage even though the parser doesn't.**

Empirical 1.05 parser-fit on 6,236 items (`scripts/analyze_per_item.py`):

```
SUCCESS perfect : 18 (0.3%)
SUCCESS leftover: 1,791 (28.7%) — top deltas: +9 (1,695), +14 (57), +13 (36)
FAIL            : 4,427 (71.0%) — top paths:   unk_u32_a (2,967),
                                                repair_data_list (742),
                                                item_bundle_data_list (671)
```

## What we don't know about 1.05

The post-`max_endurance` block isn't a fixed 27-byte struct for all items. Items split into ~two layout classes:

- **Class A** (≈1,800 items, mostly arrows/consumables): 31 bytes after `max_endurance` (27-byte block + 4-byte `repair_data_list` count).
- **Class B** (≈4,400 items, equipment/etc.): only 9 bytes after `max_endurance` (5-byte trailer `00 00 00 FF FF` + 4-byte `repair_data_list` count).

Both classes share the same 9-byte `00 00 00 FF FF 00 00 00 00` *suffix* immediately before the next item's key. Class A has 22 extra bytes between `max_endurance` and that suffix; Class B doesn't.

The 22 extra bytes encode an `ItemKey + several u32`, all zero in Class B and (for some items) non-zero in Class A. The discriminator — what flag/field controls whether those 22 bytes are present — has not been identified. Hypotheses tried and rejected:

- Item type, category, equip type, knowledge type, drop_type — none correlate.
- A leading `COptional` tag — tag would be the first byte (`0x00` in both classes), not Some/None.
- A leading `CArray<X>` count — first 4 bytes don't decode as a sensible small count.
- Inside an existing field (`item_bundle_data_list`, `prefab_data_list`, etc.) — counts are identical between Class A and B representatives.

Likely next move: a brute-force search for a single byte/bit elsewhere in the struct that perfectly partitions items into A/B based on which post-`max_endurance` length they need. Once that flag is found, model the 22-byte block as conditional and the parser should hit ≥99%.

Until then, the anchor pipeline carries the user.

## Why 71 items have no paloc translation

Confirmed across 14 language paloc files (kor, eng, jpn, zho-tw, zho-cn, deu, fre, ger, ita, pol, por-br, rus, spa-es, spa-mx, tur — see `list_all_paloc.py` output). All 71 are dev/QA items with `is_editor_usable = 0`; even Korean (the source language) has no `0x70` entry for them. The community `item_names.json` "names" them by mechanically replacing `_` with space in `internalName` — i.e. they didn't find a hidden source either. `scripts/export_for_ce.py` does the same fallback (uses `string_key` directly, with underscores) so these items still appear in the CE dropdown.

The investigation scripts that established this:
1. `find_unknown_items.py` — list the 71
2. `probe_paloc_for_keys.py` — show all paloc entries on those keys (any type byte)
3. `probe_paloc_types.py` — tally type-byte distribution to confirm `0x70` is the canonical "item name" type
4. `probe_all_paloc_groups.py` — confirm `localizationstring_*.paloc` ships in only one group, no patch overlay
5. `probe_kor_fallback.py` — confirm Korean too lacks `0x70` for the 71
6. `probe_iteminfo_names.py` — confirm `item_name.default` is just the encoded `(key << 32) | 0x70` index, not a fallback string
7. `compare_community_paloc.py` — confirm community names = humanized `internalName`
8. `check_fallback_names.py` — show `0x30` is usually unrelated content (character names like "Pirate" for `LightSaber_TwoHandSword`)

## Layout invariants worth keeping in mind

- Every item begins with `u32 key` then `u32 string_key.len` then `len` ASCII bytes then `NUL`. The anchor scanner relies on this and it has held across all observed versions.
- Item keys are bounded comfortably below 2^24 (a ~6-digit decimal). The `(key >> 24) == 0` check in `scan_next_item_start` is solid.
- `string_key` length is between 2 and 64 in observed data. The `(2..=64).contains(&slen)` clamp in the scanner has zero false negatives across 6,236 keys.
- `data/keys.txt` is the ground truth for "is this key actually loaded into the game." 6,236 keys in 1.05 vs 6,389 paloc 0x70 entries (paloc carries 153 extra entries for keys that aren't in the live game).

## Avoid

- Trying to "fix" the 27-byte block by guessing field types — the current decomposition is already best-effort and won't get better without identifying the A/B discriminator.
- Removing `parse_iteminfo_lossy` or the anchor pipeline — they're the user-facing safety net while the parser is incomplete.
- Committing anything under `out/`, `references/samples/`, or `data/baselines/` — those contain extracted Pearl Abyss content. The `.gitignore` already excludes them; double-check after edits.
