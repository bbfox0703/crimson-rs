# scripts/ — Claude context

This file documents the *current state* of the 1.05 work for any future AI assistant continuing it. The user-facing documentation is in [`README.md`](README.md); this is the engineering side.

## Status of the 1.05 parser

The 5/2 patch (Crimson Desert 1.05) changed the `ItemInfo` binary layout in two places relative to 1.04:

1. **`ItemIconData` grew by 5 bytes per entry** — a new `icon_path_alt: StringInfoKey` between `icon_path` and `check_exist_sealed_data`, plus a trailing `unk_flag: u8` after `gimmick_state_list`.
2. **A new 5-byte field** (`unk_pre_pattern_key: u32 + unk_pre_pattern_flag: u8`) was inserted between `convert_item_info_by_drop_npc` and `pattern_description_data_list`. The u32 is always 0 in observed data; the u8 is `1` only for the 48 fish-food items (`Food_Salmon`, `Food_Trout`, `Food_Carp`, …) and `0` otherwise — looks like a recipe / cookable-flag pair added with the 1.05 cooking expansion.
3. **`SubItem` accepts a new tag value `15`** (treated as the existing `None` variant — both 14 and 15 carry no payload).

Apart from those, `ItemInfo` is byte-identical to the 1.04 layout. The "variant tail" in earlier iterations (`new_icon_path` CString + branched body + `ammo_mid_block` + `unk_pre_repair_*` sentinel) was a misinterpretation: it happened to round-trip on items where the misread bytes coincidentally satisfied later parser checks (e.g. ammo items where `max_endurance == 0xFFFF` provided the bogus "trailer sentinel"), but on the 800+ items that didn't coincide it broke parsing entirely. It has been deleted from `src/item_info/item.rs`.

Empirical 1.05 parse-fit on 6,236 items (`scripts/analyze_per_item.py`):

```
SUCCESS perfect : 6,215 (99.7%)   ← was 5,417 (86.9%) before the 1.04-anchored fix
SUCCESS leftover:     7 (0.1%)    +88 (3), +54 (3), +93 (1)
FAIL            :    14 (0.2%)    occupied_equip_slot_data_list (8),
                                  item_name.default            (6)
```

`serialize_iteminfo` produces byte-identical output on every one of the 6,215 perfectly-parsed items.

The two production-side workarounds are still in place for the remaining 21 stragglers:

1. **`parse_iteminfo_lossy(bytes)`** — added in `src/python.rs`. Walks the binary, falls back to a byte-pattern scan (`u32 key + u32 small length + ASCII string_key + NUL`) on each parser error, jumps to the next plausible item start, and continues. Returns `{items, spans, errors}`.
2. **Anchor-based pipeline (`scripts/export_for_ce.py`)** — uses the CE-dumped `data/keys.txt` (in-game-ordered list of all 6,236 itemKeys) to locate every item by its key value in the binary, then parses each chunk independently. Items the parser can't consume cleanly fall back to a minimal record `{key, string_key, _index, _anchor_off, _anchor_size, _status}` so downstream tools (the CE dropdown generator) still get every item.

## How the fix was found (1.04-anchored cross-version diff)

The 1.05-only iteration (Round 1 + Round 2) reached 86.9% by guessing field shapes inside the 1.05 binary alone. The remaining 13% only yielded after adding a 1.04 binary as a reference. Concretely:

1. **Extracted `out/baselines/1.04/iteminfo.pabgb`** from the user-supplied 1.04.01 game install via `crimson_rs.extract_file(..., "0008", "gamedata/binary__/client/bin", "iteminfo.pabgb")`.
2. **Built the historical 1.04 parser** by `git worktree add ../crimson-rs-104 56a57da` and `maturin build --release` in that worktree, then `pip install --target=.crimson_rs_104` (sibling install path so it can be `sys.path`-loaded without colliding with the in-tree 1.05 wheel).
3. **Dumped `parse_iteminfo_tracked` spans** for representative items via `scripts/dump_104_spans.py`, writing every named 1.04 field's start/end offset to `out/baselines/1.04/spans.json`. The 1.04 parser is correct on its own data, so its tracked spans are ground truth.
4. **Cross-aligned 1.04 spans against 1.05 chunks** via `scripts/align_104_105.py`. For each 1.04 field, search for the same byte sequence in the 1.05 chunk; the offset delta is the cumulative shift. The shift goes 0 → +5 across `item_icon_list[0]` (the known ItemIconData growth) and +5 → +10 right after `convert_item_info_by_drop_npc` for items where the next field (`pattern_description_data_list`) is non-empty enough to disambiguate (e.g. `Item_gimmick_resourcestorage_0001` with one PatternDescriptionData entry of two PatternParamStrings).
5. **The 5-byte block at the +5 → +10 transition** is `00 00 00 00 00` for 4,257 items and `00 00 00 00 01` for the 48 fish-food items. Confirmed by `scripts/probe_new_5b_field.py`. Modelled as `unk_pre_pattern_key: u32 + unk_pre_pattern_flag: u8`.
6. **Removed the (wrong) variant-tail / `ammo_mid_block` / `ItemInfoTail`-prefix model** and restored the 1.04 trailing fields verbatim (`is_blocked_store_sell..is_preserved_on_extract`, `respawn_time_seconds`, `max_endurance`, `repair_data_list`). With both fixes in place, parser status jumps from 5,417 ok / 812 fail to 6,215 ok / 14 fail.

## Remaining work (in order of payoff)

1. **8 items fail at `occupied_equip_slot_data_list`** — these are items with numeric string_keys (e.g. `4296796952068208`, `4311107783098480`) and no entry in `out/baselines/1.04/items.jsonl`, so they look brand-new in 1.05. Likely a new item category whose `OccupiedEquipSlotData` shape differs. Inspect with `python scripts/analyze_per_item.py` then dump the failing chunk's bytes around the parser error to figure out what changed.

2. **6 items fail at `item_name.default`** — same numeric-string_key pattern (`4311124962967664`, etc.). Failure is in the `LocalizableString` length field, suggesting an upstream field grew. Probably the same root cause as cluster (1) — both are new item families.

3. **7 leftover items**: 3 with 88 trailing bytes (`Item_Skill_AbyssGear_AddCriticalRateByMaterialKey_PlateArmor_LV{1,2,3}`), 3 with 54 trailing bytes (the "Fabric_Armor" cluster), 1 with 93 trailing bytes (`Recipe_Item_Skill_AbyssGear_Creature_AdditionalDamage_LV1`). Parser parses the chunk cleanly *up to* its known fields but doesn't consume the trailing bytes. Probably category-specific extension blocks.

   `scripts/inspect_leftover_bytes.py --leftover 88` dumps the unconsumed bytes; the AbyssGear "Special" 88-byte block looks like one `u32 + CString(len=66) + u8 + u64 + u32 + u8` shape (= 88 bytes), suggesting a moved / extended `item_bundle_data_list`-like extension at the end.

## Tooling added in this iteration

| Script | Purpose |
|---|---|
| [`diff_104_105.py`](diff_104_105.py) | Side-by-side hex dump of one item's 1.04 vs 1.05 chunks with a first-diff marker. |
| [`diff_104_105_full.py`](diff_104_105_full.py) | Full `difflib.SequenceMatcher` diff; emits every insert / replace span and labels each by the 1.05 parser-tracked field at that offset. |
| [`probe_new_5b_field.py`](probe_new_5b_field.py) | Tallies the second 5-byte insert across every paired item. |
| [`dump_104_spans.py`](dump_104_spans.py) | Loads the historical 1.04 parser wheel from `.crimson_rs_104/` and writes every named field's offsets for selected (or all) items to `out/baselines/1.04/spans.json`. |
| [`align_104_105.py`](align_104_105.py) | Walks 1.04 spans against the 1.05 chunk for one item and reports where the cumulative byte-shift transitions — pinpoints exactly which 1.04 field the new 1.05 bytes are inserted after. |

## Set-up the 1.04 parser locally (one-time)

```powershell
git worktree add ../crimson-rs-104 56a57da
cd ../crimson-rs-104
maturin build --release
cd ../crimson-rs
pip install --target=.crimson_rs_104 --force-reinstall --no-deps `
    ../crimson-rs-104/target/wheels/crimson_rs-0.1.0-cp312-abi3-win_amd64.whl
```

`.crimson_rs_104/` is gitignored. `dump_104_spans.py` does the `sys.path.insert(0, '.crimson_rs_104')` itself so the in-tree (1.05) wheel keeps working in the same Python process.

## Starting the next session — recommended workflow

The parser is at **99.7% perfect** (6,215 / 6,236 items) with 7 leftovers + 14 fails remaining. Both clusters look like brand-new 1.05 item families that never existed in 1.04, so the 1.04-anchored diff approach won't directly apply. Instead:

### What to do first
```powershell
# 1. Sanity-check the current state.
python scripts\export_for_ce.py            # should show ok=6,215 leftover=7 fail=14
python scripts\analyze_per_item.py          # per-cluster breakdown of the 21 stragglers

# 2. For each fail cluster, dump bytes around the parser error to see what differs.
python scripts\inspect_leftover_bytes.py --leftover 88   # AbyssGear "Special" cluster
python scripts\inspect_leftover_bytes.py --leftover 54   # Fabric_Armor cluster
python scripts\inspect_leftover_bytes.py --leftover 93   # AdditionalDamage_LV1
```

### Investigation strategy by cluster
- **`fail:occupied_equip_slot_data_list` (8) and `fail:item_name.default` (6)** — items with numeric string_keys (e.g. `4296796952068208`). Check `out/baselines/1.04/items.jsonl` for matching keys; if absent, these are new in 1.05 and the 1.04-anchored diff won't apply. Compare instead against a *similar* item (same numeric pattern in `out/items.jsonl`) that does parse, looking for what shape difference triggers the failure. Both clusters likely share a root cause since `item_name.default` is the very first variable-length field after the early failure point.
- **`leftover:88` (3 AbyssGear PlateArmor)** — parser reaches end-of-known-schema with 88 trailing bytes. The dump in [scripts/CLAUDE.md](#remaining-work-in-order-of-payoff) suggests one `u32 + CString(len=66) + u8 + u64 + u32 + u8` extension. Try modelling as a trailing `COptional<X>` or `CArray<X>` after `repair_data_list` (only present for AbyssGear "Special" tier).
- **`leftover:54` (3 Fabric_Armor)** — smaller trailing block; probably a different extension. Inspect first.
- **`leftover:93` (1 Recipe item)** — single-item edge case; lowest priority.

### Don't repeat
- Don't reintroduce the `new_icon_path` / `ammo_mid_block` / `ItemInfoTail` (3u8 + sentinel) variant-tail model. It coincidentally round-trips on a subset of items but is fundamentally wrong; see "How the fix was found" above.
- Don't add new schema fields without first running [`scripts/align_104_105.py`](align_104_105.py) on the affected item — eyeballing `difflib` output is unreliable when fields are zero-padded.
- For brand-new 1.05 items (no 1.04 baseline), `align_104_105.py` won't help. Use [`scripts/diff_104_105_full.py`](diff_104_105_full.py) to compare a *failing* 1.05 item against an *OK* 1.05 item with similar structure instead.

### Concrete numbers as of this session
```
parse-fit  : 6,215 / 6,236 (99.7%) perfect
leftover   :     7         (+88 ×3, +54 ×3, +93 ×1)
fail       :    14         (8 occupied_equip_slot_data_list, 6 item_name.default)
roundtrip  : byte-perfect on every parsed item via serialize_iteminfo
pipeline   : `python scripts/export_for_ce.py` runs end-to-end clean
last commit on dev: <this commit> (1.04-anchored 99.7% parser)
```

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

- Removing `parse_iteminfo_lossy` or the anchor pipeline — they're the user-facing safety net for the 21 remaining stragglers.
- Committing anything under `out/`, `references/samples/`, `data/baselines/`, or `.crimson_rs_104/` — those contain extracted Pearl Abyss content and a locally-built historical wheel. The `.gitignore` already excludes them; double-check after edits.
