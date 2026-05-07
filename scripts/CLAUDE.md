# scripts/ — Claude context

This file documents the *current state* of the 1.05 work for any future AI assistant continuing it. The user-facing documentation is in [`README.md`](README.md); this is the engineering side.

## Status of the 1.05 parser

The 5/2 patch (Crimson Desert 1.05) changed the `ItemInfo` binary layout in ways that aren't fully understood yet. The current Rust parser handles the changes that are known:

- `ItemIconData` grew by 5 bytes per entry: a second `StringInfoKey` (`icon_path_alt`) right after `icon_path`, plus a trailing `unk_flag: u8`.
- `SubItem` accepts a new tag value `15` (treated as the existing `None` variant — both 14 and 15 carry no payload).
- The four 1.04 bool fields `is_blocked_store_sell..is_preserved_on_extract` were *replaced* by a u32 length prefix of a new CString `new_icon_path` (e.g. `cd_icon_common_camp_donation_00`). The trailing layout becomes a discriminated union:
  - **`new_icon_path == ""` (legacy branch):** the original `respawn_time_seconds: i64` + `max_endurance: u16` follow. For the 18 ammo / projectile items only, an additional 22-byte `ammo_mid_block` sits between `max_endurance` and the trailer; we detect it by peeking the trailer sentinel `FF FF` and consuming 22 bytes if it isn't present yet.
  - **`new_icon_path != ""` (icon-path branch):** no respawn / max_endurance pair; instead one byte `icon_flag` (observed `01`) + 9 unknown zero bytes precede the trailer.
- `ItemInfoTail` (3 u8 + u16 sentinel `0xFFFF` + `repair_data_list`) is unchanged.

Empirical 1.05 parse-fit on 6,236 items (`scripts/analyze_per_item.py`):

```
SUCCESS perfect : 5,417 (86.9%)   ← was 2,967 (47.6%) before the variant tail
SUCCESS leftover:     7 (0.1%)    +88 (3), +54 (3), +93 (1)
FAIL            :   812 (13.0%)   top paths: item_bundle_data_list (671),
                                              ammo_mid_block        (93),
                                              emoji_texture_id      (18)
```

The `serialize_iteminfo` writer produces byte-identical output on every one of the 5,417 perfectly-parsed items.

With those changes, raw `parse_iteminfo_from_bytes` will throw on the remaining ~13% of 1.05 items. The two production-side workarounds are:

1. **`parse_iteminfo_lossy(bytes)`** — added in `src/python.rs`. Walks the binary, falls back to a byte-pattern scan (`u32 key + u32 small length + ASCII string_key + NUL`) on each parser error, jumps to the next plausible item start, and continues. Returns `{items, spans, errors}`.

2. **Anchor-based pipeline (`scripts/export_for_ce.py`)** — uses the CE-dumped `data/keys.txt` (in-game-ordered list of all 6,236 itemKeys) to locate every item by its key value in the binary, then parses each chunk independently. Items the parser can't consume cleanly fall back to a minimal record `{key, string_key, _index, _anchor_off, _anchor_size, _status}` so downstream tools (the CE dropdown generator) still get every item. **This is what makes the user-facing pipeline give 100% coverage even though the parser doesn't.**

## Iteration log

### Round 1 — `ItemInfo` split into Core + optional mid block + Tail

Initial cluster classification under the (now-superseded) interpretation that the post-`max_endurance` bytes were a single `[u8; 22]` mid block:

```
post_size : count : meaning (Round-1 reading; see Round 2 for the real model)
---------:-------:--------------
   9      : 2967 : Class B  (no mid block)
  31      :   18 : "Class A minimum" — these turned out to be the 18 ammo
  34      :  525 : "Class A + 3 extra"
  36      :  181 : "Class A + 5 extra"
  40      : 1695 : "Class A + 9 extra"   ← length=31 icon path
  44      :   36 : "Class A + 13"        ← length=35
  45      :   57 : "Class A + 14"        ← length=36
  53      :   31 : "Class A + 22"        ← length=44
```

Round-1 discriminator: `max_endurance != 0 && max_endurance != 0xFFFF` → read 22-byte mid block. This appeared to fit because for items with the new `cd_icon_*` icon path the parser was reading two bytes of ASCII string content as `max_endurance` (e.g. `co` = `0x6F63`), which happens to be non-zero / non-`0xFFFF`. So the predicate accidentally matched the right items, but the 22 bytes consumed were the *wrong* 22 bytes (the middle of the icon path string, not a separate field).

### Round 2 — variant tail driven by `new_icon_path` length

`scripts/dump_post_bytes.py` showed the supposed "mid block" was the **continuation of an ASCII string** for everything but the 18 ammo items: post=34 mid started with `mmon_housing_00`, post=40 with `mmon_camp_donation_00`, post=44 with `mmon_faction_coin_Hern`, etc. Walking 14 bytes back from the parser's `max_endurance` end revealed a u32 whose value matched the visible string length exactly:

| post | length value | string content |
|------|---|---|
|  31  | 0  (ammo)   | (none)                                    |
|  34  | 25          | `cd_icon_common_housing_00`               |
|  36  | 27          | `cd_icon_common_AbyssGear_00`             |
|  40  | 31          | `cd_icon_common_camp_donation_00`         |
|  44  | 35          | `cd_icon_common_faction_coin_Hernand`     |
|  45  | 36          | `cd_icon_common_auction_balance_scale`    |
|  53  | 44          | `...auction_balance_scale_package`        |

That u32 sits exactly where 1.04 had four contiguous bool fields (`is_blocked_store_sell..is_preserved_on_extract`). For Class B items the length is genuinely `0` because all four bools were false in 1.04, which is why the Round-1 parser parsed Class B correctly even with a wrong mental model.

The full 1.05 model:

```
ItemInfoCore (unchanged) ... enable_equip_in_clone_actor: u8
new_icon_path: CString                 // u32 len + len bytes (no NUL)

if new_icon_path.length == 0:          // ~3,005 items (Class B + 18 ammo)
    respawn_time_seconds: i64
    max_endurance: u16
    if !trailer_at(off):               // 18 ammo items only
        ammo_mid_block: [u8; 22]
else:                                  // ~3,231 items
    icon_flag: u8                      // observed = 0x01
    icon_unk_zeros: [u8; 9]            // observed all zero

ItemInfoTail (unchanged):
    3 × u8 + u16 sentinel = 0xFFFF
    repair_data_list: CArray<RepairData>
```

`scripts/probe_new_layout.py` validates the model end-to-end: 5,403 / 6,236 items reach the trailer cleanly, and `serialize_iteminfo` then roundtrips byte-perfect on every one of the 5,417 items the Rust parser accepts. The 1.04 `RepairData` size hypothesis from Round 1 turned out to be wrong; the 9 trailing bytes were `icon_unk_zeros`, not shrunk repair entries.

## Remaining work (in order of payoff)

1. **671 items fail at `item_bundle_data_list`** — top remaining failure path. The parser fails *before* reaching the variant tail, so these are blocked by an unrelated 1.05 change in core. Likely a new field inside `item_bundle_data_list` entries or a sibling array. Examples: `Item_gimmick_resourcestorage_0001`, `Item_gimmick_collectionstorage_0001`.

2. **93 items fail at `ammo_mid_block`** — these have `new_icon_path.length == 0` but the trailer pattern `xx xx xx FF FF` is at neither offset 0 nor offset 22 from the end of `max_endurance`. Suggests another conditional shape we haven't classified yet. Examples: `Boss_Reward_SuperWeapon`, `Gas_Mask_Helm_I`.

3. **18 items fail at `emoji_texture_id`** — Boss_Reward_*Map and similar. Failure is in a `CString` length field, suggesting upstream misalignment (a different field somewhere earlier got a length change in 1.05).

4. **15 items fail at `pattern_description_data_list`** — all `*_Armor_I` entries (Katunan, Vennebis, etc.). Likely a new field inside `PatternParamString` or `PatternDescriptionData`.

5. **8 items fail at `occupied_equip_slot_data_list`** — `Recipe_Item_Skill_AbyssGear_*`, very early in core. Probably a knock-on from item 3 / 4.

6. **7 leftover items**: +88 (3), +54 (3), +93 (1). Likely additional new fields after `icon_unk_zeros` for specific item categories (e.g. AbyssGear "Special" variants — see `scripts/probe_new_layout.py` output for the `cd_icon_common_AbyssGear_00` case, where the parser overshoots before the real trailer).

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
