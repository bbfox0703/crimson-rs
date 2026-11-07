use super::keys::*;
use super::structs::*;
use crate::binary::*;
use crate::py_binary_struct;

// ── ItemInfo (2.04.08) ──────────────────────────────────────────────────────
//
// Crimson Desert 2.04.08 (`meta/0.paver` 2/4/8, build 0xe4d53a97, `dlc_01`
// revision 7) adds two `CString`s between `filter_type` and `unk_texture_path`
// (`charged_sound_name`, `charged_sound_param`). Nothing else moved. Every one
// of the 7,064 base rows carried over from 2.04.03 is exactly +8 B: the two
// empty strings (`u32 0` each). Every one of the 1,552 `iteminfo.dlc_01` rows is
// also +8 B, except seven ship cannons, which fill both strings (+63 B), and
// one row with a content change. Splicing 8 zero bytes in after `filter_type`
// reproduces the 2.04.08 row on 6,003 base and 1,275 DLC rows. Every other row
// differs further on only in `multi_change_info_list`, whose keys were renumbered
// (+18). Within the minor, a patch-level drift: base 7,064 → 7,069 rows
// (+5, none removed), iteminfo 5,804,162 → 5,865,384 B.
//
// The first parse error on 2.04.08 (`CArray count 1944765921` at row offset
// 275) is the old `unk_texture_path` hash `0xeac5e173` read as a list count,
// eight bytes early.
//
// ── ItemInfo (2.04) ─────────────────────────────────────────────────────────
//
// Crimson Desert 2.04 (`meta/0.paver` 2/4/3, Steam build 26345091) changes how
// every static-info row is laid out, and iteminfo with it. Nothing below is a
// new fact about what an item holds; the same values were rearranged. 6,816 →
// 7,064 items (+259 / -11), iteminfo 6,465,724 → 5,804,162 B (the rows shrank).
//
//   - **The name comes first**: `[u32 len][name][u32 key]`, not `[key][len][name]`.
//     This is table-wide; see `binary::row_header`.
//   - **A fixed block, then the variable part**: `is_blocked` through
//     `unk_post_max_charged_b` are 157 B after the name on every item, u8s first,
//     then (mostly) u32s. The struct below is in file order, not in topic order.
//   - **Empty lists and strings cost one byte.** Most top-level `CArray`s became
//     `CFlaggedArray` (`0x00`, or `0x01` + the old `CArray`), `item_name` /
//     `item_desc` / `item_desc2` became `OptLocalizableString`, and
//     `drop_default_data`, `sharpness_data` and `inspect_action` became
//     `COptional`. Lists inside elements keep the old count prefix.
//   - **Strings became hashes**: `unk_texture_path` and `emoji_texture_id`.
//   - **Removed, with no byte left for them**: `quick_slot_index`,
//     `consumable_type_list`, `unk_pre_pattern_key`, `unk_pre_pattern_flag`,
//     `unk_pre_respawn_flag`, `unk_post_respawn_data_list`. A structural walk of
//     all 6,805 carried-over rows ends exactly on each row's last byte without
//     them.
//   - **Added**: `docking_child_data_list`, `EnchantData::unk_post_equip_buffs`,
//     two f32s on `InspectData`, `DockingChildData`'s three `unk_*` fields, two
//     more `inventory_info_list` slots. `unk_pre_respawn_data_list` moved into
//     the optional `unk_pre_respawn_block`.
//   - **Old misreadings the new order exposed**: `packed_item_info`,
//     `unpacked_item_info` and `convert_item_info_by_drop_npc` were one byte out
//     of step, so `packed_item_info` read 1 on 1,486 items where it is really
//     the separate `unk_pre_packed_item_flag`, and the real keys read shifted
//     (`00 fd 48 0f`). `unk_pre_pattern_key` was the top byte of `convert...`.
//
// Pinned by re-encoding every 2.03 field and matching it against the 2.04 row:
// fields found at one fixed offset on every item give the 157-byte head, and a
// structural walk gives the tail. `iteminfo.dlc_01` (`dlc/dlc_01/0008`, 1,552
// rows) reads and round-trips with the same layout, and is what exposed
// `unk_pre_respawn_block`, absent from the base table. Which of two always-equal
// or always-zero fields is which (`is_dyeable` / `is_editable_grime`,
// `material_key` / `material_match_info`, the zero u8s and u32s of the head)
// cannot be told from the data, and those keep the order they had.

// ── ItemInfo (2.03) ─────────────────────────────────────────────────────────
//
// Crimson Desert 2.03 (`meta/0.paver` 2/3/0/0x03045138) makes ONE layout
// change relative to 2.02: `inventory_info_list` gained a tenth InventoryKey
// slot, so every item ends 2 bytes later. +3 items (6,813 → 6,816, keys
// 1006028–1006030, the `Dev_QA_dlc_Ocean_*` trio; none removed);
// iteminfo 6,450,232 → 6,465,724 B (+15,492).
//
// Evidence (tandem walk of every common item against the kept 2.02 binary,
// placed on the 2.02 parser's field spans):
//
//   * All 6,813 carried-over items are exactly +2 B, and the 2 bytes are the
//     only insertion in every one of them. Slots 0–8 are unchanged on all
//     6,813; 4 items also changed an `item_icon_list[0].icon_path` value.
//   * The new u16 reads 0xFF — the array's unused-slot sentinel — on 6,757
//     items and 21 on exactly the 59 `Trade_*_PackedInVehicle` items: the
//     same 59 that set slots 7 and 8 (PetAndVehicle, Wagon) and the only
//     items with a non-zero `unk_pre_max_endurance`. InventoryKey 21 is
//     `Ship` in the (unchanged) inventory table, so their tuple reads
//     WareHouse / CampWareHouse ×4 / PetAndVehicle / Wagon / Ship.
//   * On the 6,754 other items slots 7 and 8 are 0xFF too, so the byte walk
//     cannot tell which trailing 0xFF is the new one — but on the 59 trade
//     items only "after slot 8" reproduces the 2.03 bytes. The same sentinel
//     and the same 59 items is the argument that folded slot 8 into this
//     array in 1.16, so the insert is read as a tenth slot rather than a
//     separate trailing field.
//
// Slot values across 2.03 are {1, 2, 3, 5, 6, 7, 8, 9, 10, 13, 14, 21, 255}.
//
// ── ItemInfo (2.00) ─────────────────────────────────────────────────────────
//
// Crimson Desert 2.00 is the first *major* bump (`meta/0.paver` goes
// 1/18/0/0x28577c0f → 2/0/0/0xb241d214 — note the minor resets to 0). It makes
// TWO layout changes relative to 1.18. +237 net items (6,573 → 6,810, none
// removed); iteminfo.pabgb 6,190,316 → 6,446,719 B (+256,403). Full
// `serialize_iteminfo` round-trip is byte-identical on the live binary
// (in == out, 6,810 items, 0 skipped), and `export_for_ce.py` reports
// `ok=6,810 leftover=0 fail=0 no_anchor=0`.
//
//   1. `SubItem` (structs.rs) gained `type_id == 18` — payload-free, joining
//      the 14..=17 None arm. This is the same renumbering 1.12 (tag 16) and
//      1.13 (tag 17) did: in 2.00 *every* site that read 17 reads 18, at both
//      SubItem sites (item-level `default_sub_item` on all 6,810 items, and
//      `drop_default_data.default_sub_item` on 6,806 — the other 4 read tag 0).
//      No item still reads 14..=17. Payload-free is confirmed byte-wise: on a
//      carried-over item, flipping only those two tag bytes in the 1.18 bytes
//      keeps the rest aligned.
//
//   2. A new `u32` (`unk_pre_max_endurance_a`, 0 on all 6,810 items) was
//      inserted ahead of the 1.12-era `unk_pre_max_endurance`, so the block
//      before `respawn_time_seconds` now carries two u32s. **Unconditional**:
//      6,513 of the 6,573 items carried over from 1.18 are exactly +4 B (the
//      other 60 also changed content), and on 1,500 sampled carry-overs
//      `old[max_endurance..] == new[max_endurance + 4..]` byte-for-byte,
//      1500/1500.
//
// Why the insert lands *there* and not 4 or 12 bytes later — the byte diff
// alone cannot say, because the whole run is zeros on most items and all three
// candidate positions round-trip byte-perfectly. What settles it is the same
// value-sanity argument the 1.16 swap rests on (see change #3 in the 1.16
// header): only with the new u32 read **first** do both known distributions
// survive 1.18 → 2.00 unchanged —
//
//   * `unk_pre_max_endurance` still reads `0x01000000` on exactly the 59
//     `Trade_*_PackedInVehicle` items and 0 on the other 6,751, and
//   * `respawn_time_seconds` still reads 0 (6,053) / -1 (755) / 604800 (2 —
//     `Item_Egg_KuKu` and `Item_Egg_Wyvern`, i.e. 7 days).
//
// Reading the new u32 *after* `unk_pre_max_endurance` drags the `0x01000000`
// into it and leaves `unk_pre_max_endurance` dead; reading it after
// `respawn_time_seconds` additionally turns the two eggs' 604800 into
// `604800 << 32` and -1 into `-4294967296`. Both are the nonsense signature
// 1.16 taught us to reject.
//
// The 1.18 `MergedPrefabVisualData.unk_pre_is_craft_material` sentinel is
// unchanged: still the constant `0xeac5e173`, now on all 12,550 elements.
//
// ── ItemInfo (1.18) ─────────────────────────────────────────────────────────
//
// Crimson Desert 1.18 makes exactly ONE layout change relative to 1.17: every
// `MergedPrefabVisualData` element gained a `u32` (`unk_pre_is_craft_material`)
// between `tribe_gender_list` and the 3-byte flag tail. +1 net item
// (6,572 → 6,573, key 1005446 `Demian_Greyfur_Fabric_Cloak_II`); iteminfo
// 6,139,734 → 6,190,316 B (+50,582). RE'd via `scripts/diff_117_118.py`
// (a clone of `diff_115_116.py`).
//
// Why the single-field read is safe:
//
//   * The insert lands at the *start* of `is_craft_material` in the 1.17 span
//     map, i.e. after `tribe_gender_list` and before the flags — had it been
//     appended after `unk_post_flag` the walk would have reported it against
//     the following field instead, since the 3 flag bytes (values 0..=3) never
//     match `73 e1 c5`.
//   * The per-item insert count equals that item's 1.17 merged-element count
//     on 5,794 of the 5,797 fully-realigned items (the 3 stragglers are items
//     whose merged count itself changed), so the field is unconditional — not
//     gated on a sibling the way 1.12's `unk_pre_gimmick_visual` is.
//   * All 10,415 inserts observed in the walk carry the *same* value,
//     `73 e1 c5 ea` (LE u32 `0xeac5e173`) — the "empty string" Jenkins
//     sentinel already documented under 1.10 change #1 below.
//   * Per-item size deltas are +4 × (merged element count): +0 (822 items),
//     +4 (198), +8 (4,679), +12 (750), +16 (36), +20 (44), +24 (20). Nothing
//     else moved.
//
// Two other length-changing signatures in the report are walk artifacts, NOT
// drifts — each is one half of a compensating pair produced by 1.18 reordering
// the `item_group_info_list` u16s (a pure value churn that dominates the diff):
// `look_detail_mission_info rm:1B` (93×) pairs with `item_group_info_list[2]
// rm:11B ins:12B` (54×) + `rm:9B ins:10B` (39×), and
// `enable_alert_system_to_ui ins:1B` (5×) pairs with `item_group_info_list[2]
// rm:6B ins:5B` (5×). Both net to zero bytes per item.
//
// ── ItemInfo (1.17) ─────────────────────────────────────────────────────────
//
// Content-only over 1.16 — no layout change, no parser edit. 6,581 → 6,572
// items: nine `Item_Set_*_Tier0_Reminiscence` entries (keys 1004912–1004920)
// were removed and none added. iteminfo.pabgb 6,145,386 → 6,139,734 B, and the
// −5,652 B is *exactly* the sum of those nine items' spans, so nothing else
// moved. Of the 6,572 surviving items 6,435 are byte-identical to their 1.16
// bytes and 137 changed values only — **zero** changed size, which is the check
// that rules out a layout drift hiding behind a compensating value change.
//
// ── ItemInfo (1.16) ─────────────────────────────────────────────────────────
//
// Crimson Desert 1.16 makes FOUR layout changes relative to 1.13–1.15 (1.14 and
// 1.15 were content-only). +73 net items (6,508 → 6,581, none removed);
// iteminfo.pabgb 5,938,891 → 6,145,386 B (+206,495). RE'd via the lightweight
// tandem byte-walk vs the kept 1.15 binary (`scripts/diff_115_116.py`, cloned
// from `diff_112_113.py`) plus per-site decodes over all common items. The net
// per-item delta is **+24 B** on 6,085 of the 6,508 common items
// (−2 −1(cond) +10 +16). Full `serialize_iteminfo` round-trip is byte-identical
// on the live binary (in == out, 6,581 items, 0 skipped).
//
// The changes:
//
//   1. `inventory_info: InventoryKey` (u16) was **removed from the item head**
//      (it sat between `broken_item_prefix_string` and `equip_type_info`) and
//      relocated to the item end — see change 4.
//
//   2. `DockingChildData::unk_post_summon_tag: u8` — the trailing byte 1.08
//      added — was **removed** (structs.rs). Conditional drift: it only shows
//      on the 391 items with `docking_child_data.__tag__ != 0`, and that
//      discriminator partitions the table perfectly (fp=0, fn=0).
//
//   3. A **10 + 28×N byte block** was inserted immediately before
//      `unk_pre_max_endurance` / `respawn_time_seconds`, and those two fields
//      **swapped order** (1.15 read `respawn_time_seconds` then
//      `unk_pre_max_endurance`; 1.16 reads the u32 first). The block is
//      `u32 + u8 flag + CArray<UnkPreRespawnData> + u8`, so it costs a flat
//      10 B on the 6,567 items whose list is empty — which is why it first
//      looked like a fixed 10-byte insert. Only 14 items carry elements
//      (5 with one, 9 with two).
//
//      The swap is what makes `respawn_time_seconds` decode sanely: with it,
//      the field reads 0 (5,831 items), −1 (748) or 604,800 (2 — exactly 7
//      days in seconds) and `unk_pre_max_endurance` stays 0 as it has since
//      1.12; without it both fields decode to nonsense (−4294967296 and
//      0xffffffff). Byte-wise the swap is indistinguishable from "1.16 deleted
//      the old u32 and added a new always-zero u32 here", so the round-trip
//      alone does not settle it — the value distributions do.
//
//   4. The removed `inventory_info` **reappears at the item END, widened to
//      nine slots** (`inventory_info_list: [u16; 9]`, 18 B), replacing the
//      1.13-era 2-byte `unk_tail`. Slot 0 equals the old head-side
//      `inventory_info` on **all 6,054 cleanly-diffable items, zero
//      exceptions** — that equality is what identifies this as a relocation
//      rather than an unrelated new block. Slots hold the same small
//      InventoryKey domain (1, 2, 3, 5, 6, 7, 8, 9, 10, 12, 13, 14) with 0xFF
//      as the unused-slot sentinel; 14 distinct tuples across the table.
//
//      Slot 8 is the u16 that 1.13–1.15 carried as the constant `unk_tail`
//      (0x00ff on all 6,508 items). It is folded into the array rather than
//      kept separate because in 1.16 it reads 6 — not 0xFF — on exactly the
//      59 `Trade_*_PackedInVehicle` items, which are also exactly the items
//      whose slot 7 reads 7 instead of 0xFF (and the only ones whose
//      `unk_pre_max_endurance` is non-zero). That perfect three-way
//      correlation, plus the shared 0xFF sentinel, makes "slot 8 of the same
//      array, unused on every pre-1.16 item" a much better fit than
//      "unrelated constant that coincidentally changed on the same 59 items".
//
//      Alignment check: across all 6,581 items every one of the 9 × 6,581
//      slot values falls in {1, 2, 3, 5, 6, 7, 8, 9, 10, 13, 14, 255} — no
//      out-of-domain u16 anywhere. A mis-sized or mis-positioned array would
//      produce garbage here, so this is what pins the width at 9 rather than
//      8 + a separate tail.
//
// Downstream note: the `crimson_iteminfo_lookup_inventory_info` C ABI is
// unchanged and now sources `inventory_info_list[0]`, which is byte-for-byte
// the value it returned before 1.16.
//
// No save-body drift from the iteminfo side. 1.16 does introduce a new save
// format (slot108) — tracked separately.
//
// ── ItemInfo (1.13) ─────────────────────────────────────────────────────────
//
// Crimson Desert 1.13 makes the single largest structural change yet: it
// **relocates and merges** the prefab/gimmick-visual block. +25 net items
// (6,483 → 6,508); iteminfo.pabgb 5,754,919 → 5,938,891 B (+183,972). RE'd via
// the tandem/opcodes byte-walk vs the kept 1.12 binary
// (`gamedata-bin/1.12/iteminfo.pabgb`) + the "fix head/middle, read the parser's
// per-item leftover" technique (`scripts/decode_tail_113.py`). Full
// `serialize_iteminfo` round-trip is byte-identical on the live binary
// (in == out, all 6,508 items). The changes:
//
//   1. `SubItem` gained `type_id == 17` — a new payload-free variant (items
//      that read tag 16 in 1.12 now read 17). Joins the None arm; affects both
//      SubItem sites (`drop_default_data.default_sub_item` + item-level
//      `default_sub_item`).
//
//   2. `prefab_data_list` and `gimmick_visual_prefab_data_list` — which sat in
//      the item **middle** (right after `drop_default_data`) in 1.05–1.12 — were
//      **merged into a single list and relocated to the item END** (after
//      `repair_data_list`), modeled by `MergedPrefabVisualData` (structs.rs).
//      The merged element interleaves both source structs' fields: a `scale`
//      (`(1,1,1)` for prefab-origin rows), the prefab/animation name lists, the
//      equip-slot + tribe-gender lists, and a 3-byte flag tail. The merged
//      count equals the old prefab + gimmick element totals. `enchant_data_list`
//      and the equip/gem-gated `unk_pre_gimmick_visual` **stay** in the middle
//      (in their 1.12 order) — only the two prefab/gimmick lists moved.
//
//   3. A constant 2-byte item tail (`0xff, 0x00`; `unk_tail: u16`) now closes
//      every item, after the relocated merged list.
//
// Discriminator note: what first looked like an "equipment-gated +14 head
// block" was a mis-read — the bytes after `drop_default_data` are just
// `enchant_data_list` (whose count is large for equipment) followed by the
// gated `unk_pre_gimmick_visual`; both are unchanged from 1.12. The only moved
// fields are the two prefab/gimmick lists.
//
// No save-body drift in 1.13 (slot107 = the live 1.13 save: hmac_ok, body
// decode undecoded_bytes=0, all 12 live saves full-body round-trip + idempotent;
// format unchanged v2 / flags 0x0080). Gamedata bridge tables: all 30 PABGH key
// lists still auto-detect; `partprefabdyeslotinfo` grew +570 rows AND refined
// its per-slot record schema (the 1.12 `(0xFF,0)` 5-byte pad is really
// `u8 marker + u32 extra_layer_count`; 1.13's new dyeable gear sets count=1,
// adding a second dye layer). Fixed in `src/part_prefab_dye_slot_info/`.
//
// ── ItemInfo (1.12) ─────────────────────────────────────────────────────────
//
// Crimson Desert 1.12 makes FOUR layout changes relative to 1.11 (the largest
// schema drift since 1.08). +150 net items (6,333 → 6,483). RE'd with the
// lightweight tandem byte-walk vs the kept 1.11 binary
// (`gamedata-bin/1.11/iteminfo.pabgb`, 5,543,339 B) — `scripts/diff_111_112.py`
// + per-boundary byte dumps. Result after all four edits:
// `parser status: ok=6,483  leftover=0  fail=0  no_anchor=0`, and a full
// `serialize_iteminfo` round-trip on the live binary is byte-identical
// (5,754,919 B in == out). The four drifts:
//
//   1. `SubItem` (structs.rs) gained `type_id == 16` — a new payload-free
//      variant. Items that read tag 15 (None) in 1.11 now read 16; verified
//      byte-aligned (no payload, like 14/15). Affects both SubItem sites
//      (`drop_default_data.default_sub_item` and the item-level
//      `default_sub_item`). 4,496 items flipped 15→16.
//
//   2. A new `u32` (`unk_pre_max_endurance`, always 0) was inserted between
//      `respawn_time_seconds` and `max_endurance`. **Unconditional** (every
//      item). 4,496 items show it cleanly (the rest carry content drift on
//      top).
//
//   3. A new `u32` (`unk_pre_gimmick_visual`, always 0) was inserted between
//      `enchant_data_list` and `gimmick_visual_prefab_data_list`, but only for
//      **equipment** (`equip_type_info != 0`) and **gems** (`item_type == 74`
//      / `category_info == 2501`). The discriminator partitions all 4,496
//      cleanly-diffable common items perfectly (fp=0 fn=0; the 15 gem
//      exceptions are exactly `item_type==74`). Modeled as a conditional field
//      via the `=> <cond>` clause newly added to `py_binary_struct!`
//      (read-gated, stored `Option<u32>`; write/to_py drive off Option
//      presence — see `src/binary/mod.rs`). This is the first conditional
//      field gated on a *sibling value* rather than a `COptional`/`CArray`
//      length, hence the macro work.
//
//   4. `enchant_data_list` is no longer `CArray<EnchantData>`: 1.12 inserts a
//      `u32` (always 0) **between** consecutive `EnchantData` elements — N
//      elements carry N-1 separators (none before [0], none after the last).
//      Confirmed by a per-boundary byte-walk on key=1000019's 6 enchant rows
//      (the +4 appears only at the [1]…[5] boundaries, never before [0]).
//      Modeled by the custom `EnchantDataList` type (structs.rs); this is why
//      single-enchant items round-tripped fine under the 3-fix parser while
//      multi-enchant items failed at `enchant_data_list[1].max_stat_list`.
//
// No save-body drift in 1.12 (slot107 / the live 1.12 save: hmac_ok, body
// decode undecoded_bytes=0, body-stable write round-trip; format unchanged
// v2 / flags 0x0080).
//
// ── ItemInfo (1.11) ─────────────────────────────────────────────────────────
//
// Crimson Desert 1.11 makes a single layout change relative to 1.10: a new
// boolean `u8` field (`unk_post_apply_drop_stat_type`) was **inserted**
// between `apply_drop_stat_type` and `drop_default_data`. Every item gains
// exactly one byte (net per-item delta +1 on 6,321 / 6,325 items common to
// 1.10 and 1.11; the remaining 4 carry additional content-only drifts on top
// of the +1). On the cleanly-isolatable subset the value reads 0 or 1 only
// (4,861 one, 266 zero) — a boolean whose semantic role / gamedata symbol is
// still unknown; named for its position pending RE.
//
// RE workflow (lightweight tandem byte-walk, no sibling parser): the current
// parser parses the kept real-1.10 binary (`gamedata-bin/1.10/iteminfo.pabgb`,
// 5,532,062 B) at ok=6,325 leftover=0 but fails on every 1.11 item at
// `prefab_data_list[0].prefab_names`. A per-item tandem walk between the two
// binaries (matched by key) found one inserted byte at the `drop_default_data`
// boundary for all common items. Result after this edit applied:
// `parser status: ok=6,333  leftover=0  fail=0  no_anchor=0`.
// (Heads-up: the 1.10.01 hotfix install is NOT a real 1.10 — its 0008
// container is byte-identical to the 1.11 D: install; use the kept
// `gamedata-bin/1.10/iteminfo.pabgb` baseline for cross-version diffs. These
// per-version baselines are gitignored game content kept in a portable local
// archive — drive letter irrelevant, copy the folder wherever it's needed.)
//
// ── ItemInfo (1.10) ─────────────────────────────────────────────────────────
//
// Crimson Desert 1.10 makes two layout changes relative to 1.09 (which
// itself shared the 1.08 layout — same parser ran clean on both):
//
//   1. The `money_icon_path: StringInfoKey` field (4 bytes) between
//      `map_icon_path` and `use_map_icon_alert` was **removed**. Verified
//      against `out/1.09/iteminfo.pabgb` (1.09 sibling install) by
//      reconstructing every common item from 1.09 bytes with the 4-byte
//      money_icon_path span deleted: 4,459 / 6,314 items match byte-perfect
//      after this single edit; the remaining 1,855 items split into 1,435
//      content-only drifts (multi_change_info growth, weapon stat tweaks,
//      item_memo string changes for flags/carpets) + 420 with an additional
//      schema drift (see #2). The constant `0x73e1c5ea` (LE u32
//      = 0xeac5e173 — a Jenkins hash that was never referenced by anything
//      in the game install) shows up in the deleted span for ~78% of
//      items, suggesting Pearl Abyss had been packing a stub "no money
//      icon" sentinel into this field for years; the 1.10 cleanup just
//      removes the field outright.
//
//   2. `UnitData` (in `structs.rs`) gained a new `u32` field
//      (`unk_post_icon_path`) between `icon_path` and `item_name`. Affects
//      every populated `MoneyTypeDefine` — for Money_Copper's two
//      `MoneyUnitEntry` entries the value is `0xc52007c6` in both. The 1.10
//      patch notes (新增了「交戰」與「重建」階段, 貢獻管理員 / contribution
//      manager reorg, new camp/contribution currencies) match the 17 items
//      that toggled `money_type_define` from None→Some in 1.10
//      (`Money_Camp_*`, `Contribution_*`, `Money_Pearl`, `Cog`,
//      `Item_Pinball_Coin`, etc.).
//
// The RE workflow that pinned both edits lives in
// `scripts/diff_109_110.py` + `verify_109_to_110.py` +
// `scripts/trace_size_mismatch.py`. Result after both edits applied:
// `parser status: ok=6,325  leftover=0  fail=0  no_anchor=0`.
//
// Crimson Desert 1.08 had earlier changed the layout in three places relative
// to 1.07 (which itself shared the 1.05 / 1.06 layout — same parser ran clean
// on all three):
//
//   1. `extract_additional_drop_set_info: u32` was **removed** from between
//      `extract_multi_change_info` and `minimum_extract_enchant_level`.
//
//   2. A new `u8` field (`is_equip_quick_slot_visible`) was **inserted**
//      between `is_housing_only` and `quick_slot_index`. The field is a
//      boolean (only 0 or 1 observed across all 6,314 items: 5,365 zero,
//      949 one). Strongly correlates with the 1.08 patch notes about the
//      new equip-quick-bar reorganization (added tool slot, mask/crown
//      moved to armor slot): the 949 `1`-valued items are exactly the
//      "real equipment" set — every weapon, armor piece, accessory, and
//      tool-slot tool (伐木斧頭 / 鐵鎚 / 鏟子 / 掃帚 / 鐮刀 / 十字鎬 /
//      手鑽/電鋸 / 扇子). Quest items, recipes, consumables, and the
//      cosmetic/plot items that share an `item_type=58 cat=601` with the
//      real tools (Notepad, Feather_Pen, Item_Scale, Abacus, Equip_Drum,
//      Equip_Trumpet, …) all read 0. The strict implication
//      `value=1 ⇒ equip_type_info != 0` holds; the converse does NOT (2,155
//      items have a non-zero `equip_type_info` but `value=0` — those are
//      the quest-flavored equipment items). Name is a best-effort guess
//      from correlation; underlying gamedata symbol still TBD.
//
//   3. A new trailing `u8` field (`unk_post_summon_tag`) was **inserted**
//      inside `DockingChildData` (see `structs.rs`). It only materialises
//      for the 385 items with `docking_child_data.tag = 1` (items whose
//      visual mesh attaches to a character socket — weapons, armor that
//      docks to body sockets, etc.). Every sampled item reads `0x00`;
//      semantic role is unknown — likely a placeholder for a future
//      feature or a reserved field that current gamedata doesn't populate.
//
// Net per-item size delta: -3 bytes for items with `docking_child_data.tag = 0`
// (the 5,929-item majority), -2 bytes for the 385 items with
// `docking_child_data.tag = 1` (the extra `unk_post_summon_tag` cancels
// 1 byte of the removal). Confirmed byte-perfect by reconstructing synthetic
// 1.08 items from 1.07 bytes with `extract_additional_drop_set_info` excised
// and the two `u8` fields inserted at their respective positions (round-trip
// equality verified for Pyeonjeon_Arrow key=2200, Goblin_Pot key=52006,
// Marni_Devotee_PlateArmor_Helm key=14510 and
// KuKu_Lightning_TwoHandSpear key=1002175).
//
// Crimson Desert 1.05 had earlier introduced two layout changes relative to
// 1.04:
//
//   1. Each `ItemIconData` entry grew by 5 bytes — a new `icon_path_alt`
//      `StringInfoKey` was added between `icon_path` and
//      `check_exist_sealed_data`, and a trailing `unk_flag: u8` was added
//      after `gimmick_state_list`.
//
//   2. A new 5-byte block (`unk_pre_pattern_key: u32 + unk_pre_pattern_flag: u8`)
//      was inserted between `convert_item_info_by_drop_npc` and
//      `pattern_description_data_list`. The u32 is always 0; the u8 is 1
//      only for the 48 fish-food items (`Food_Salmon`, `Food_Trout`, …) and
//      0 otherwise.
//
// Those 1.05 changes remain in the 1.08 layout. The earlier "variant tail"
// interpretation
// (`new_icon_path` + `ammo_mid_block` + `unk_pre_repair_*`) was a
// misinterpretation that happened to round-trip on items where the misread
// bytes coincidentally satisfied later parser checks (e.g. ammo items where
// `max_endurance == 0xFFFF` provided the bogus "trailer sentinel"); it is
// dropped here.

py_binary_struct! {
    pub struct ItemInfo<'a> {
        pub string_key: CString<'a>,
        pub key: ItemKey,
        pub is_blocked: u8,
        pub max_endurance: u16,
        pub max_stack_count: u64,
        pub apply_drop_stat_type: u8,
        // 2.04: reads 0 on the 654 items the old `is_important_item` marked,
        // 2 on the others, and 1 once. See "ItemInfo (2.04)" above.
        pub item_importance_kind: u8,
        pub is_dyeable: u8,
        pub is_editable_grime: u8,
        pub discardable: u8,
        // The 59 `Trade_*_PackedInVehicle` items. Through 2.03 this byte was the
        // top of the 4-byte `unk_pre_max_endurance` (0x01000000 on those 59).
        pub is_packed_in_vehicle: u8,
        pub is_housing_only: u8,
        pub is_destroy_when_broken: u8,
        pub hide_from_inventory_on_pop_item: u8,
        pub is_blocked_store_sell: u8,
        pub enable_equip_in_clone_actor: u8,
        pub is_save_game_data_at_use_item: u8,
        pub is_preserved_on_extract: u8,
        pub is_has_item_use_data_inventory_buff: u8,
        pub is_preorder_item: u8,
        pub is_wild: u8,
        pub is_tower_shield_item: u8,
        pub is_shield_item: u8,
        pub is_logout_at_use_item: u8,
        pub enable_alert_system_to_ui: u8,
        // Through 2.03 this byte was read as `packed_item_info` = 1 (1,486 items)
        // because the fields around it were one byte out of step.
        pub unk_pre_packed_item_flag: u8,
        pub is_editor_usable: u8,
        pub use_map_icon_alert: u8,
        // Added in Crimson Desert 1.08: boolean (only 0 / 1 observed). Marks
        // items that participate in the reorganized equip-quick-bar system
        // — every weapon, armor piece, accessory, and tool-slot tool reads
        // 1; quest items / recipes / consumables / cosmetic items read 0.
        pub is_equip_quick_slot_visible: u8,
        pub use_immediately: u8,
        pub apply_max_stack_cap: u8,
        pub can_disassemble: u8,
        pub is_register_trade_market: u8,
        pub use_drop_set_target: u8,
        pub delete_by_gimmick_unlock: u8,
        pub is_all_gimmick_sealable: u8,
        pub item_type: u8,
        pub knowledge_obtain_type: u8,
        pub item_tier: u8,
        pub usable_alert_type: u8,
        pub item_charge_type: u8,
        // 1.11 boolean; reads 0/1 only, semantic role unknown.
        pub unk_post_apply_drop_stat_type: u8,
        pub look_detail_mission_info: MissionKey,
        pub look_detail_game_advice_info_wrapper: GameAdviceInfoKey,
        pub convert_item_info_by_drop_npc: ItemKey,
        pub unpacked_item_info: ItemKey,
        // 1.16: 0 on nearly every item; an ItemKey-shaped value on the
        // `Wood_Branch_*` gathering-node tiers.
        pub unk_pre_respawn_a: u32,
        pub broken_item_prefix_string: LocalStringInfoKey,
        pub equip_type_info: EquipTypeKey,
        pub material_key: u32,
        pub map_icon_path: StringInfoKey,
        pub packed_item_info: ItemKey,
        pub gimmick_info: GimmickInfoKey,
        pub destroy_effec_info: EffectKey,
        pub equipable_level: u32,
        pub minimum_extract_enchant_level: u16,
        pub category_info: CategoryKey,
        pub gimmick_unlock_message_local_string_info: LocalStringInfoKey,
        pub knowledge_info: KnowledgeKey,
        pub max_drop_result_sub_item_count: u32,
        pub equipable_hash: u32,
        pub default_sub_item: SubItem,
        pub discard_offset_y: f32,
        pub extract_multi_change_info: MultiChangeKey,
        pub material_match_info: MaterialMatchKey,
        pub shared_cool_time_group_name_hash: u32,
        pub unk_pre_max_endurance_a: u32,
        pub max_charged_useable_count: u32,
        pub unk_post_max_charged_a: u32,
        pub unk_post_max_charged_b: u32,
        pub item_name: OptLocalizableString<'a>,
        pub item_desc: OptLocalizableString<'a>,
        pub item_desc2: OptLocalizableString<'a>,
        pub item_memo: CString<'a>,
        pub filter_type: CString<'a>,
        // New in 2.04.08, empty on every row except seven `iteminfo.dlc_01` ship
        // cannons, which read "env_walla_ship_combat_cannon_ready" /
        // "SoundNameOnMaxCharged". Named from those seven rows only.
        pub charged_sound_name: CString<'a>,
        pub charged_sound_param: CString<'a>,
        // Both were CStrings through 2.03. The hash is Jenkins of the old string,
        // which now lives in `stringinfo`; the empty string is 0xeac5e173.
        pub unk_texture_path: StringInfoKey,
        pub emoji_texture_id: StringInfoKey,
        pub occupied_equip_slot_data_list: CFlaggedArray<OccupiedEquipSlotData>,
        pub item_tag_list: CFlaggedArray<u32>,
        pub item_use_info_list: CFlaggedArray<ItemUseKey>,
        pub item_icon_list: CArray<ItemIconData>,
        pub equip_passive_skill_list: CFlaggedArray<PassiveSkillLevel>,
        pub gimmick_tag_list: CFlaggedArray<CString<'a>>,
        pub sealable_item_info_list: CFlaggedArray<SealableItemInfo<'a>>,
        pub sealable_character_info_list: CFlaggedArray<SealableItemInfo<'a>>,
        pub sealable_gimmick_info_list: CFlaggedArray<SealableItemInfo<'a>>,
        pub sealable_gimmick_tag_list: CFlaggedArray<SealableItemInfo<'a>>,
        pub sealable_tribe_info_list: CFlaggedArray<SealableItemInfo<'a>>,
        pub sealable_money_info_list: CFlaggedArray<ItemKey>,
        pub transmutation_material_gimmick_list: CFlaggedArray<GimmickInfoKey>,
        pub transmutation_material_item_list: CFlaggedArray<ItemKey>,
        pub transmutation_material_item_group_list: CFlaggedArray<ItemGroupKey>,
        pub multi_change_info_list: CFlaggedArray<MultiChangeKey>,
        pub reserve_slot_target_data_list: CFlaggedArray<ReserveSlotTargetData>,
        // Absent where it would be all defaults (no sockets, tag-18 sub item).
        pub drop_default_data: COptional<DropDefaultData>,
        pub enchant_data_list: EnchantDataList,
        pub unk_pre_gimmick_visual: u32 => equip_type_info.0 != 0 || item_type == 74,
        pub price_list: CArray<ItemPriceInfo>,
        pub docking_child_data: COptional<DockingChildData<'a>>,
        // New in 2.04, empty except on the eight `Item_wagon_wheel_*` items (four
        // wheel sockets each). `docking_child_data` above stays singular.
        pub docking_child_data_list: CFlaggedArray<DockingChildData<'a>>,
        pub inventory_change_data: COptional<InventoryChangeData>,
        pub fixed_page_data_list: CFlaggedArray<PageData<'a>>,
        pub dynamic_page_data_list: CFlaggedArray<PageData<'a>>,
        pub inspect_data_list: CFlaggedArray<InspectData<'a>>,
        pub inspect_action: COptional<InspectAction<'a>>,
        // Reads 1 on every item.
        pub unk_pre_cooltime_flag: u8,
        pub cooltime: i64,
        pub unk_post_cooltime_a: i64,
        pub unk_post_cooltime_b: i64,
        pub sharpness_data: COptional<ItemInfoSharpnessData>,
        pub hackable_character_group_info_list: CFlaggedArray<CharacterGroupKey>,
        pub item_group_info_list: CArray<ItemGroupKey>,
        pub pattern_description_data_list: CFlaggedArray<PatternDescriptionData<'a>>,
        pub item_bundle_data_list: CFlaggedArray<ItemBundleData>,
        pub money_type_define: COptional<MoneyTypeDefine<'a>>,
        // Absent on every base-table item; only `iteminfo.dlc_01` carries it.
        pub unk_pre_respawn_block: COptional<UnkPreRespawnBlock>,
        pub respawn_time_seconds: i64,
        pub repair_data_list: CFlaggedArray<RepairData>,
        // 1.13: the merged prefab + gimmick-visual list (see
        // MergedPrefabVisualData).
        pub merged_prefab_visual_list: CArray<MergedPrefabVisualData>,
        // InventoryKey slots; slot 0 is the pre-1.16 `inventory_info` and 0xFF
        // marks an unused slot. 1.16 had nine, 2.03 ten, 2.04 twelve.
        pub inventory_info_list: [u16; 12],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The parser targets Crimson Desert 1.05. The test reads the 1.05
    // binary that `scripts\export_for_ce.py` extracts to `out\iteminfo.pabgb`.
    // Both files are gitignored (they ship Pearl Abyss content) and the
    // test skips cleanly when they aren't present.
    const BINARY_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        r"\out\iteminfo.pabgb"
    );

    fn try_load_binary() -> Option<Vec<u8>> {
        std::fs::read(BINARY_PATH).ok()
    }

    #[test]
    fn test_parse_first_item() {
        let Some(data) = try_load_binary() else {
            eprintln!("skipping: 1.04 baseline binary not found at {BINARY_PATH}");
            return;
        };
        let mut offset = 0;
        let item = ItemInfo::read_from(&data, &mut offset).unwrap();
        assert_eq!(item.key, ItemKey(2200));
        assert_eq!(item.string_key.data, "Pyeonjeon_Arrow");
    }
}
