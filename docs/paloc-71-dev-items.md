# Why 71 items have no paloc translation

71 items in the live 1.05 game (out of 6,236 total) have no paloc 0x70 entry in *any* of the 14 shipped languages — not even Korean (the source language). They're dev / QA records flagged with `is_editor_usable = 0` in `ItemInfo`, never surfaced to players.

The community's `item_names.json` "names" them by mechanically replacing `_` with space in `internalName` (= `string_key`); they didn't find a hidden source either. [`../scripts/export_for_ce.py`](../scripts/export_for_ce.py) does the same fallback, so these items still appear in the CE dropdown using their `string_key` directly.

## How it was confirmed

Six investigation scripts in [`../scripts/`](../scripts/) established this finding:

1. [`find_unknown_items.py`](../scripts/find_unknown_items.py) — list the 71 items missing a paloc 0x70 entry in `localizationstring_eng.paloc`.
2. [`probe_paloc_for_keys.py`](../scripts/probe_paloc_for_keys.py) — for those 71 keys, dump every paloc entry on those keys (any type byte). Confirms only `0x70` is the canonical "item name" type and the 71 have nothing relevant.
3. [`probe_paloc_types.py`](../scripts/probe_paloc_types.py) — tally type-byte distribution across paloc entries to confirm `0x70` is the canonical "item name" type and nothing else stands in.
4. [`probe_all_paloc_groups.py`](../scripts/probe_all_paloc_groups.py) — confirm `localizationstring_*.paloc` ships in only one group (no patch overlay hiding entries elsewhere).
5. [`probe_kor_fallback.py`](../scripts/probe_kor_fallback.py) — confirm Korean too lacks 0x70 entries for the 71 (rules out "missing from translated locales but present in source language").
6. [`probe_iteminfo_names.py`](../scripts/probe_iteminfo_names.py) — confirm `item_name.default` (the in-binary fallback name) for these items is just the encoded `(key << 32) | 0x70` paloc lookup index serialized as a numeric string, not a hidden human-readable fallback.
7. [`compare_community_paloc.py`](../scripts/compare_community_paloc.py) — confirm community names = humanized `internalName`. The community didn't find a hidden source either.
8. [`check_fallback_names.py`](../scripts/check_fallback_names.py) — show that `0x30` (sometimes thought to be a fallback type) is usually unrelated content (character names like "Pirate" for `LightSaber_TwoHandSword`), so it can't be used as a name source.

## Implication for the 1.05 parser

The numeric `item_name.default` strings (e.g. `"4295766159917168"` — a 16-digit decimal that happens to equal `(1000186 << 32) | 0x70`) on these 71 items are what tripped up the anchor scanner during Phase 3 of the parser RE. The scanner saw the numeric string sitting at the right structural position and locked onto it as a "fake" item start. See [`1.05-parser-history.md`](archive/1.05-parser-history.md) for the full story.

## Implication for downstream

CE table data (`output*.txt`) shows these items by `string_key` (e.g. `Item_Skill_AbyssGear_AddCriticalRateByMaterialKey_LeatherArmor_LV1` rather than a translated name). That's accurate for what the game actually ships.

## When to revisit

If a future patch adds paloc 0x70 entries for any of these keys, the count drops below 71 and the export's `string_key` fallback shrinks accordingly. No code changes needed — the script does the right thing automatically.
