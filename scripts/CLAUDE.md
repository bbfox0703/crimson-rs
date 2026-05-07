# scripts/ — Claude context

This file documents the *current state* of the 1.05 work for any future AI assistant continuing it. The user-facing documentation is in [`README.md`](README.md); this is the engineering side.

## Status of the 1.05 parser

The 5/2 patch (Crimson Desert 1.05) changed the `ItemInfo` binary layout in ways that aren't fully understood yet. The current Rust parser handles the changes that are known:

- `ItemIconData` grew by 5 bytes per entry: a second `StringInfoKey` (`icon_path_alt`) right after `icon_path`, plus a trailing `unk_flag: u8`.
- `SubItem` accepts a new tag value `15` (treated as the existing `None` variant — both 14 and 15 carry no payload).
- `ItemInfo` is split into `ItemInfoCore` (everything up to and including `max_endurance`), an *optional* 22-byte raw mid block, and `ItemInfoTail` (3 u8 + u16 sentinel + `repair_data_list`).
  The mid block is read **only when `max_endurance != 0 && max_endurance != 0xFFFF`** — this gets ~99% of Class A items right (the few it misses are ammo/projectile consumables with `max_endurance == 0`; see "remaining work" below).

With those changes, raw `parse_iteminfo_from_bytes` will throw on most 1.05 items. The two production-side workarounds are:

1. **`parse_iteminfo_lossy(bytes)`** — added in `src/python.rs`. Walks the binary, falls back to a byte-pattern scan (`u32 key + u32 small length + ASCII string_key + NUL`) on each parser error, jumps to the next plausible item start, and continues. Returns `{items, spans, errors}`.

2. **Anchor-based pipeline (`scripts/export_for_ce.py`)** — uses the CE-dumped `data/keys.txt` (in-game-ordered list of all 6,236 itemKeys) to locate every item by its key value in the binary, then parses each chunk independently. Items the parser can't consume cleanly fall back to a minimal record `{key, string_key, _index, _anchor_off, _anchor_size, _status}` so downstream tools (the CE dropdown generator) still get every item. **This is what makes the user-facing pipeline give 100% coverage even though the parser doesn't.**

Empirical 1.05 parser-fit on 6,236 items (`scripts/analyze_per_item.py`):

```
SUCCESS perfect : 2,967 (47.6%)   ← was 18 (0.3%) before max_endurance discriminator
SUCCESS leftover: 1,793 (28.8%)   top deltas: +9 (1,695), +14 (57), +13 (36)
FAIL            : 1,476 (23.7%)   top paths:    repair_data_list (740),
                                                 item_bundle_data_list (671)
```

## Iteration log

### Round 1 — `ItemInfo` split into Core + optional mid block + Tail

Layout class distribution (from `scripts/classify_items.py` ground truth):

```
post_size : count : meaning
---------:-------:--------------
   9      : 2967 : Class B  (no mid block)
  31      :   18 : Class A minimum (22 mid + 5 trailer + 4 repair=0)
  34      :  525 : Class A + 3 extra
  36      :  181 : Class A + 5 extra
  40      : 1695 : Class A + 9 extra ← suspected new RepairData entry size
  44      :   36 : Class A + 13
  45      :   57 : Class A + 14
  53      :   31 : Class A + 22
  63/94/102/124/137 : 1+ : long-tail outliers
```

Discriminator search (`scripts/find_discriminator.py`, `refine_discriminator.py`):
`max_endurance != 0 && max_endurance != 0xFFFF` → Class A. Currently in production. Misses 18 ammo/projectile items (arrows, cannonballs, bullets) which have `max_endurance == 0` *and* a 22-byte mid block — those still fall into the leftover/fail buckets.

## Remaining work (in order of payoff)

1. **`+9` leftover cluster (1,695 items)** — `scripts/inspect_leftover_bytes.py` shows the trailing 9 bytes are exactly `00 00 00 FF FF 00 00 00 00` = the standard trailer + `repair_data_list count = 0`. This means `repair_data_list` *entries* moved earlier in the struct: the 1.04 RepairData was 15 bytes (`u32 + u16 + u8 + u64`); 1.05 looks like a 9-byte struct (probably `u64` shrunk to `u16`, lopping 6 bytes). Implementing RepairData_1_05 with a 9-byte size should take this cluster perfect and probably solve `+13`/`+14` too (different entry counts).

2. **18 ammo items mis-discriminated** — they have `max_endurance == 0` but DO have the 22-byte mid block. Need a secondary predicate. `apply_max_stack_cap == 0` + `item_use_info_list count == 3` is a plausible heuristic (per `scripts/refine_discriminator.py` output). `consumable_type_list` count or `item_use_info_list[0]` value range (close to INT32_MAX) might also work.

3. **~700 items fail before reaching `max_endurance`** — these break at `item_bundle_data_list` or `pattern_description_data_list` count. Pre-`max_endurance` layout is mostly unchanged from 1.04 but probably has its own conditional somewhere (likely in one of the variable-size sub-structs).

4. **Long-tail outliers (post=63, 94, 102, 124, 137)** — 5 items. Don't fit any clean rule yet; likely items with multiple new entries in some new array.

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
