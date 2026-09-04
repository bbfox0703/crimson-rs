# Dye editor — scope + data baselines

Reference data for CrimsonAtomtic's Dye editor feature. The PyQt5
reference editor at
`D:\Github\CRIMSON-DESERT-SAVE-EDITOR-AND-GAME-MODS\CrimsonSaveEditor`
already implements this; this doc records what the in-house C# editor
needs and what crimson-rs needs to ship (or not ship).

> Status (2026-05-17): Save schema verified **and** all three
> gamedata-side `dye*.pabgb` tables RE'd + bridged behind C ABI **and**
> the `_itemKey → _partPrefabKey` cross-reference is bridged **and**
> the "add dye to undyed item" path lands via
> `crimson_save_set_object_list_present` (see §v2 below).
> Parsers + bridges shipped in
> [`src/dye_color_group_info/`](../src/dye_color_group_info/),
> [`src/part_prefab_dye_texture_pallete_info/`](../src/part_prefab_dye_texture_pallete_info/),
> [`src/part_prefab_dye_slot_info/`](../src/part_prefab_dye_slot_info/)
> with matching `c_abi/*.rs` bridges, plus the combined
> [`src/c_abi/item_part_prefab.rs`](../src/c_abi/item_part_prefab.rs)
> bridge that resolves `ItemKey → Vec<PartPrefabKey>` via the
> 3-table join (iteminfo → stringinfo → partprefabdyeslotinfo).
> **Replaces the PyQt5 reference editor's hand-maintained
> `dye_slot_counts.json`** end-to-end — the C# editor can now look up
> slot counts by item key directly.

---

## What "Dye editor" does in the PyQt5 reference editor

Picks an equipped item from a save, mutates its dye data:
- Per-slot **RGB** (0-255 each)
- Per-slot **material** (`_texturePalleteKey` u16 — selects a palette
  like "Cloth", "Metal", "Leather")
- Per-slot **grime opacity** (-128..127 i8)
- Per-slot **dye color group** (`_dyeColorGroupInfoKey` u32 — links to
  a named family like "Herenon" rather than freeform RGB)

Each item can have multiple dye slots (1-4 typically, per the
hand-maintained `dye_slot_counts.json` shipped with the editor).

The reference editor:
1. Walks `InventorySaveData → _inventorylist[N] → _itemList[M]`
2. Pulls the item's `_itemDyeDataList` ObjectList (field 14 of
   `ItemSaveData`)
3. Edits one element at a time for in-place mutations
4. Removes + reinserts the element when the mask changes (adding a
   previously-absent field)
5. For items with NO `_itemDyeDataList`, shells out to a `dye_cli.exe`
   subprocess that does PARC insertion + offset fix-up

---

## Save schema — `ItemSaveData._itemDyeDataList` element

**Verified 2026-05-15 against the live 1.07 `slot0/save.save`** via
`_probe_item_dye_data` in
[`src/c_abi/character_info.rs`](../src/c_abi/character_info.rs).
Sample dye element (`block_idx=8, inv_idx=1, item_idx=33, item_key=9102`,
mask `[0x5e, 0x00]`):

| Field | Mask bit | Type | Meta size | Sample value |
|---|---|---|---:|---|
| `_dyeSlotNo`              | 0x01 | `int8`   (signed!)              | 1 | absent |
| `_dyeColorR`              | 0x02 | `uint8`                          | 1 | `217` |
| `_dyeColorG`              | 0x04 | `uint8`                          | 1 | `133` |
| `_dyeColorB`              | 0x08 | `uint8`                          | 1 | `133` |
| `_dyeColorA`              | 0x10 | `uint8`                          | 1 | `255` |
| `_grimeOpacity`           | 0x20 | `int8`   (signed)                | 1 | absent |
| `_dyeColorGroupInfoKey`   | 0x40 | `DyeColorGroupInfoKey` (u32 LE)  | 4 | `0xc88211f5` |
| `_texturePalleteKey`      | 0x80 | `PartPrefabDyeTexturePalleteKey` (u16) | 2 | absent |
| `_disableSymbol`          | (byte 1, 0x01) | `int8`                | 1 | absent |

Class: `ItemDyeSaveData`. 9 fields total (PyQt5 RE reports 8 — it
missed `_disableSymbol`). Mask is **2 bytes**, not 1 (the 9th field's
bit lives in byte 1 bit 0).

**Two PyQt5 RE corrections** to note for the C# editor:
- `_dyeSlotNo` is **signed `int8`**, not unsigned u8.
- `_texturePalleteKey` is **fixed u16** (`meta_size=2`), not the
  variable u16/u32 the PyQt5 source suggests.

---

## Gamedata baselines — `0008/gamedata/binarystaticinfo__/bin/`

(`0008/gamedata/binary__/client/bin/` with `.pabgb` / `.pabgh` extensions on 1.05–2.00 —
2.01 renamed the directory and the extensions without changing any file's contents.)

Three `.pabgb` + `.pabgh` tables, all RE'd against the live 1.07 install.

| File | Size (cmp/unc) | Rows in 1.07 | Maps | Parser / Bridge |
|---|---:|---:|---|---|
| `dyecolorgroupinfo.pabgb` + `.pabgh` | 6 KB / 9 KB, index 82 B | **10** | `DyeColorGroupInfoKey (u32)` → color group name (`"Her_Color_Group_I"`, …) + 109-record RGBA gradient palette. Save's `_dyeColorGroupInfoKey` references this. | [`dye_color_group_info`](../src/dye_color_group_info/mod.rs) / [`c_abi/dye_color_group_info.rs`](../src/c_abi/dye_color_group_info.rs) |
| `partprefabdyeslotinfo.pabgb` + `.pabgh` | 86 KB / 370 KB, index 9 KB | **1,626** on 2.01 (1,105 when this table was first written on 1.07) | `PartPrefabKey (u32)` → prefab internal name + per-slot detail (3 material indices, 3 default-material names, 12 mask bytes — 3 before 2.01 — next-prefab / `.pac` tail per slot). Replaces `dye_slot_counts.json`. | [`part_prefab_dye_slot_info`](../src/part_prefab_dye_slot_info/mod.rs) / [`c_abi/part_prefab_dye_slot_info.rs`](../src/c_abi/part_prefab_dye_slot_info.rs) |
| `partprefabdyetexturepalleteinfo.pabgb` + `.pabgh` | 0.8 KB / 4 KB, index 68 B | **11** (keys 0..=10) | `PartPrefabDyeTexturePalleteKey (u16)` → palette tier with 2–3 sub-records (material name + icon DDS + texture DDS + optional variant name & strength). Save's `_texturePalleteKey` references this. | [`part_prefab_dye_texture_pallete_info`](../src/part_prefab_dye_texture_pallete_info/mod.rs) / [`c_abi/part_prefab_dye_texture_pallete_info.rs`](../src/c_abi/part_prefab_dye_texture_pallete_info.rs) |

The `binarygimmickchart__` `*dye*` / `dyewater` `.binarygimmick` files
in the scan output are decorative gimmicks (basecamp props, fabric
deco), NOT item-dye gamedata — ignore those.

### Verified row schemas (2026-05-16, live 1.07; partprefabdyeslotinfo re-verified on 1.12 2026-06-19)

**`dyecolorgroupinfo.pabgb`** — standard PABGH (`u16 count + (u32 key, u32 offset)*`):

```text
Row {
    u32 key,
    u32 name_len,
    u8 name[name_len],              // "Her_Color_Group_I", "Dem_Color_Group_I/II/III", etc.
    u8 flag,                        // always 0
    u32 color_count,                // always 109
    u8 color_data[8 * color_count], // RGBA + 4-byte gradient channel per record
    // 37-byte trailer:
    u8 trailer_marker[5],           // 23 31 02 00 00 — constant across rows
    u32 key_copy,                   // = key
    u32 numeric_id_len,             // always 20
    u8 numeric_id[20],              // u64 hash serialized as ASCII decimal
    u8 trailing_hash[4],            // per-row
}
```

**`partprefabdyetexturepalleteinfo.pabgb`** — **custom** 6-byte PABGH
entries (`u16 count + (u16 key, u32 offset)*`):

```text
Row {
    u32 key,                 // PABGH key extended to u32 (high bits always 0)
    u8 pad[3],
    u32 key_copy,
    u32 sub_count,           // 2 for key=0, 3 for key=1..10
    Sub sub[sub_count],
}
Sub {
    CString material_name,   // "cloth" / "leather" / "metal" / "wool" / "velvet" / "silk"
    CString icon_path,       // "ui/.../itemicon_*.dds" — for key=0 this duplicates texture_path
    CString texture_path,    // "character/texture/cd_texturelayer_*.dds"
    CString variant_name,    // empty by default, or "wool" / "velvet" / "silk"
    f32 variant_value,       // -1.0 default; positive (~0.1..0.4) when variant_name set
}
```

**`partprefabdyeslotinfo.pabgb`** — standard PABGH:

```text
Row {
    u32 key,
    u8 pad[5],
    u32 slot_count,           // 1..N
    CString row_prefab_name,  // e.g. "cd_phm_00_lb_00_0054"
    Slot slot[slot_count],
}
Slot {
    u8 mat_indices[3],        // material indices for this slot
    CString material_a / b / c, // 3 default material names (often empty)
    u8 mask[12],              // active/visible flags; 3 bytes before 2.01
    // 1.12 ONLY: u8 + u32 inserted here, observed uniformly (0xFF, 0)
    //   across all 3,893 live slots (semantics not yet RE'd). Absent
    //   in 1.07-1.11. See "Cross-version drift (1.12)" below.
    CString tail_name,        // next sub-prefab name; for the LAST slot in a row,
                              //   the full .pac asset path
}
```

`CString` here is `[u32 len][len bytes]` with NO trailing NUL — same
shape iteminfo.pabgb uses.

#### Cross-version drift (1.12)

1.12 inserted a 5-byte per-slot field (a `u8` + a `u32`, uniformly
`(0xFF, 0)`) between `mask` and `tail_name`, and removed 143 rows
(**1,111 → 968**). The two per-slot layouts are **empirically disjoint**
— across the full 1.11 (1,111 rows) and 1.12 (968 rows) tables every row
walks cleanly under exactly one layout and **zero** rows parse cleanly
under both. [`parse_part_prefab_dye_slot_info_lossy`](../src/part_prefab_dye_slot_info/mod.rs)
therefore tries the 1.12 layout first and falls back to the 1.07-1.11
layout per row, keeping older-install support while reading the live
1.12 table. The cross-version byte-walk that pinned this lives in
[`scripts/diff_dyeslot_111_112.py`](../scripts/diff_dyeslot_111_112.py).
Verified byte-perfect across the full live 1.12 table 2026-06-19. (Peak
1.12 `slot_count` is 36, for vehicle/robot meshes, vs ~30 in 1.07-1.11.)

#### Cross-version drift (1.13) — RESOLVED

1.13's patch note "擴大了可染色裝備的範圍 / expanded the range of dyeable
equipment" grew the table **968 → 1,538 keys** and refined the per-slot
record layout for the new gear. **Root cause:** what looked like a uniform
`(0xFF, 0)` **5-byte pad** in 1.12 (blindly skipped) is actually
`u8 marker (0xFF) + u32 extra_layer_count`. 1.12's count is always 0, but
1.13's new dyeable gear sets it to **1**, adding a **second material/dye
layer** inline before the slot's tail:

```
Slot (new_schema, 1.12+) =
    u8[3] mat_indices
    CString × 3    default_materials (primary layer)
    u8[N] mask                         // N = 3 through 2.00, 12 from 2.01
    u8    marker (0xFF)
    u32   extra_layer_count            // 0 in 1.07-1.12; 1 for new 1.13 gear
    extra_layer_count × ExtraLayer
    CString tail_name                  // next sub-prefab name, or the .pac path
ExtraLayer =
    CString × 3    default_materials (secondary layer, e.g. "leather")
    u8[N] mask                         // same widening as the slot mask
    u8    flag
```

The 9 rows that the old blind-pad model dropped — the new cloaks / kite-
& tower-shields / quivers / the `cd_m0001` skullknight set (keys
`0x54534e48`, `0xe0bffb36`, `0xb2cc6efa`, `0x625369c0`, `0x8cba6493`,
`0x199ceacd`, `0xac8a6ab6`, `0xbffdd4e0`, `0x5ed0a80e`) — now parse, and
the second layer is surfaced through the new
[`PartPrefabDyeSlot::extra_layers`](../src/part_prefab_dye_slot_info/mod.rs)
(`Vec<DyeExtraLayer>`, empty on 1.07-1.12 rows). The change is a
refinement of the existing `new_schema` branch (count=0 consumes byte-
identically to the old 5-byte skip), so the 1,529 previously-parsing rows
are untouched; 1.07-1.11 installs still use the no-marker fallback. RE'd
via [`scripts/decode_dyeslot_113.py`](../scripts/decode_dyeslot_113.py) —
enhanced model consumes **all 1,538 live 1.13 rows exactly**. Everything
else in the dye path (iteminfo `is_dyeable`, `ItemDyeSaveData` in the save
body, the dye-color-group + texture-pallete bridges) is byte-perfect /
unchanged on 1.13. Verified 2026-07-04.


#### Cross-version drift (2.01)

2.01 widened `mask` from 3 bytes to **12**, in the slot *and* in the
`ExtraLayer`. Nothing else about the record moved — `mat_indices`, the
three material names, the `0xFF` marker, `extra_layer_count` and the tail
are all where they were — but the old model consumes 9 bytes too few per
slot, so before the fix the parser dropped **all 1,626 rows**.

**Pinned by tandem walk against the kept 2.00 binary**
(`gamedata-bin/2.00/partprefabdyeslotinfo.pabgb`). Reading 2.00 at
`mask_len = 3` and 2.01 at `mask_len = 12`, **1,613 of the 1,620
carried-over rows have every non-mask field byte-identical** across the
two versions; the 7 that differ do so only by ordinary content (3 changed
`slot_count`, 4 changed a material name or `mat_indices`). That fixes the
width deterministically — no distribution argument needed.

**The contents were re-encoded, not extended.** In 5,572 of 6,555
comparable slot pairs the 2.00 three-byte mask does not appear as a
contiguous window anywhere inside the 2.01 twelve. There is no "original
three" — do not treat any sub-slice as the pre-2.01 field.

The 12 read as **four groups of three**. Per slot exactly one group is
non-zero on 4,276 of 6,585 live slots, two on 1,254, three on 72, none on
983 — never four. Summed across all 12 bytes the mask equals the slot's
non-empty-material-name count on **62.2%** of slots, versus 39.4% for
`mask[0..3]` alone, so the channel-active information lives across the
whole field. Which group a slot uses is not yet RE'd.

[`SLOT_LAYOUTS`](../src/part_prefab_dye_slot_info/mod.rs) now lists three
per-slot layouts (`(new_schema, mask_len)` = `(true, 12)`, `(true, 3)`,
`(false, 3)`), tried newest-first; on live 2.01 the first parses all
1,626 rows. `scripts/decode_dyeslot_113.py` still models 1.13 and so
reports 0/1,626 against a 2.01 install — that is the historical script
working as written.

**What the C# editor should call now.** The original
`..._lookup_slot_mask` / `..._lookup_slot_extra_layer_mask` bridges copy 3
bytes, which on 2.01+ is a *partial* read: for every slot whose active
group is not the first they hand back all zeros. Widening them in place
would overrun existing callers' buffers, so the full field is exposed
through two new sized-buffer entry points instead:

- `crimson_part_prefab_dye_slot_info_lookup_slot_mask_full`
- `crimson_part_prefab_dye_slot_info_lookup_slot_extra_layer_mask_full`

Both take `(buf, buf_len, required)` rather than a fixed-size out-param,
so they report the field's true width and return `BUFFER_TOO_SMALL`
instead of truncating — including if Pearl Abyss widens the mask again.
Call once with `buf_len = 0` to size, then again with the buffer. The
3-byte getters stay as the legacy shape and are exactly the head of the
full field.

How much this matters, measured through the release dll over all 1,626
prefabs / 6,585 slots on live 2.01: **2,196 slots (33.3%) read all-zero
through the 3-byte getter while the full field is non-zero.** That is the
share of the dye UI the editor renders blank today.

---

## C ABI surface (for `CrimsonAtomtic`)

All three bridges live under `src/c_abi/`. Loaders take both `.pabgb`
+ `.pabgh` (the index is required for row offsets). Lookups follow the
standard project pattern: scalar getters use out-params; string getters
use the two-call buffer pattern.

### `dye_color_group_info`

| Function | Purpose |
|---|---|
| `crimson_dye_color_group_info_load_from_file(pabgb_path, pabgh_path, *out)` | Load from disk. |
| `crimson_dye_color_group_info_load_from_bytes(pabgb, pabgb_len, pabgh, pabgh_len, *out)` | Load from memory. |
| `crimson_dye_color_group_info_free(handle)` | Free a handle. |
| `crimson_dye_color_group_info_entry_count(handle, *out_count)` | Total rows. |
| `crimson_dye_color_group_info_lookup_name(handle, color_group_key, buf, buf_len, *required)` | Resolve `_dyeColorGroupInfoKey` → internal name. |
| `crimson_dye_color_group_info_get_entry(handle, idx, *out_key, buf, buf_len, *required)` | Enumerate. |
| `crimson_dye_color_group_info_palette_size(handle, color_group_key, *out_count)` | Palette position count for the theme (109 in 1.07). |
| `crimson_dye_color_group_info_palette_at(handle, key, idx, *out_r, *out_g, *out_b, *out_a)` | Logical-RGBA at a palette position — ready to write into `_dyeColorR/G/B/A`. See §"Addendum (2026-05-17)" for the picker UX. |
| `crimson_dye_color_group_info_position_for_rgb(handle, key, r, g, b, *out_position)` | Reverse lookup — find the palette cell a currently-applied dye came from. |

### `part_prefab_dye_texture_pallete_info`

| Function | Purpose |
|---|---|
| `crimson_part_prefab_dye_texture_pallete_load_from_file` / `_load_from_bytes` / `_free` | Standard. |
| `crimson_part_prefab_dye_texture_pallete_entry_count(handle, *out_count)` | Total palette rows (11 in 1.07). |
| `crimson_part_prefab_dye_texture_pallete_lookup_sub_count(handle, palette_key, *out_count)` | Number of sub-records inside `palette_key`. |
| `crimson_part_prefab_dye_texture_pallete_lookup_sub_material_name(handle, palette_key, sub_idx, buf, ...)` | Material name. |
| `_lookup_sub_icon_path` / `_lookup_sub_texture_path` / `_lookup_sub_variant_name` | Per-field strings. |
| `crimson_part_prefab_dye_texture_pallete_lookup_sub_variant_value(handle, palette_key, sub_idx, *out_value)` | f32 variant strength. |
| `crimson_part_prefab_dye_texture_pallete_get_entry_key(handle, idx, *out_key)` | Enumerate keys. |

### `part_prefab_dye_slot_info`

| Function | Purpose |
|---|---|
| `crimson_part_prefab_dye_slot_info_load_from_file` / `_load_from_bytes` / `_free` | Standard. |
| `crimson_part_prefab_dye_slot_info_entry_count(handle, *out_count)` | Total prefabs (1,105 in 1.07). |
| `crimson_part_prefab_dye_slot_info_lookup_slot_count(handle, prefab_key, *out_count)` | **Direct replacement for `dye_slot_counts.json[item_id]`** once the `_itemKey → _partPrefabKey` linkage is in place. |
| `crimson_part_prefab_dye_slot_info_lookup_prefab_name(handle, prefab_key, buf, ...)` | Prefab internal name. |
| `crimson_part_prefab_dye_slot_info_lookup_slot_default_material(handle, prefab_key, slot_idx, mat_idx, buf, ...)` | Per-slot default material (`mat_idx ∈ {0,1,2}`). |
| `crimson_part_prefab_dye_slot_info_lookup_slot_tail_name(handle, prefab_key, slot_idx, buf, ...)` | Next-prefab / `.pac` path. |
| `crimson_part_prefab_dye_slot_info_lookup_slot_mat_indices(handle, prefab_key, slot_idx, out_indices[3])` | Raw 3-byte indices. |
| `crimson_part_prefab_dye_slot_info_lookup_slot_mask(handle, prefab_key, slot_idx, out_mask[3])` | First 3 mask bytes — the legacy shape. **Partial on 2.01+**, where the on-disk mask is 12 re-encoded bytes and a slot's active group is often not the first, so this returns all zeros for those. Kept at 3 bytes because widening it in place would overrun every existing caller's buffer. |
| `crimson_part_prefab_dye_slot_info_lookup_slot_mask_full(handle, prefab_key, slot_idx, buf, buf_len, *required)` | **The whole mask** (12 bytes on 2.01+, 3 on 1.07-2.00), sized-buffer style: call with `buf_len = 0` to learn the width, then again with the buffer. Returns `BUFFER_TOO_SMALL` rather than truncating if the field is wider than the buffer, so a future widening surfaces instead of silently degrading. Prefer this over the 3-byte getter. |
| `crimson_part_prefab_dye_slot_info_lookup_slot_extra_layer_mask_full(handle, prefab_key, slot_idx, layer_idx, buf, buf_len, *required)` | Same, for a 1.13 extra dye layer's mask. |
| `crimson_part_prefab_dye_slot_info_get_entry_key(handle, idx, *out_key)` | Enumerate keys. |

All bridges return `NULL_ARG` / `NOT_FOUND` / `OUT_OF_RANGE` /
`BUFFER_TOO_SMALL` / `PANIC` per the standard `c_abi/mod.rs::error`
codes. Live-install integration tests pin the row counts and a
handful of `(key → value)` invariants per table; they skip cleanly
when the game install isn't present.

---

## Implementation outline for `CrimsonAtomtic` Dye editor

### v1 — edit existing dye entries (no new crimson-rs ABI needed)

Existing primitives:
- `crimson_save_list_inventory_items` → find dyed items (any whose
  block contains `_itemDyeDataList` with `count > 0`)
- `crimson_save_get_block_json(block_idx)` → fetch the full
  `ItemSaveData` including its `_itemDyeDataList` content
- `crimson_save_set_scalar_field_path` → mutate individual scalars
  (RGB / material key / grime / color group)
- `crimson_save_set_scalar_field_present` → toggle present-bit on any
  scalar (e.g. add `_grimeOpacity` to an existing dye entry)
- `crimson_save_list_insert_element` → add a 2nd / 3rd dye slot to an
  item that already has one
- `crimson_save_list_remove_element` → drop a dye slot
- `crimson_save_get_mutation_version` → cache invalidation

Workflow:
1. Walk inventory via `list_inventory_items`.
2. For each candidate item, fetch its block JSON and look at
   `_itemDyeDataList` — if `present` and `count > 0`, it's dyed.
3. Show per-slot UI (4 sliders: R/G/B + grime, 2 dropdowns: material +
   color group).
4. On user save: per modified field, call `set_scalar_field_path` with
   the path `(block_idx, [(field=14 _itemDyeDataList, element=N)],
   field=<RGBA/grime/etc index>)`.

### v3 — one-shot "how many dye slots does this item have?" (shipped 2026-05-18)

The C# editor needed to chain `crimson_item_part_prefab_lookup_count`
→ `_lookup_key_at(0)` → `crimson_part_prefab_dye_slot_info_lookup_slot_count`
for every dyeable item it surfaces. New convenience wrapper:

```c
int32_t crimson_item_part_prefab_resolve_dye_slot_count(
    const CrimsonItemPartPrefabHandle* ipp,
    const CrimsonPartPrefabDyeSlotInfoHandle* slot_info,
    uint32_t item_key,
    uint32_t* out_slot_count,
    uint32_t* out_resolve_source
);
```

Always returns `OK` (or `NULL_ARG` / `PANIC`). Resolution outcome
communicated via `out_resolve_source`:

| Constant | Value | Meaning |
|---|---:|---|
| `DIRECT` | 0 | `out_slot_count` is authoritative — chained both bridges cleanly. |
| `NOT_RESOLVED_NO_PARTPREFAB` | 1 | Item has no partprefab mapping; editor should fall back to a curated default (e.g. the human-male equip-slot prefab's slot count) or show "unknown". 76% of 1.07 `is_dyeable=1` items hit this — they're body-type variants (goblin / dwarf / tribe meshes) that share the human male's dye-slot layout but aren't listed in `partprefabdyeslotinfo`. `out_slot_count = 0`. |
| `NOT_RESOLVED_NO_SLOT_INFO` | 2 | Partprefab present but missing from the slot-info table. Shouldn't happen given how the join is built — defensive guard for cross-version safety. `out_slot_count = 0`. |

**Resolution policy**: always uses `partprefab[0]` (first resolved
prefab in iteminfo's `prefab_data_list` traversal order). Multi-prefab
items usually share a dye-slot layout across variants; if per-variant
resolution is needed later, drop back to the manual chain via
`crimson_item_part_prefab_lookup_key_at`.

Live regression `c_abi_item_part_prefab_resolve_dye_slot_count_live`
pins:
- ≥30 items resolve via DIRECT in the 1..1M key range (1.07: 45)
- All resolved slot counts in `1..=32` (plausible range; 1.07
  observed max is 16)
- Wrapper output agrees with the manual chain on every resolved item
- Unknown key → `OK + NOT_RESOLVED_NO_PARTPREFAB`
- No `NOT_RESOLVED_NO_PARTPREFAB` leakage when iterating items
  with a known partprefab mapping (defensive branch-logic check)

C# call shape:

```csharp
var rc = crimson_item_part_prefab_resolve_dye_slot_count(
    ippHandle, slotInfoHandle, itemKey,
    out uint slotCount, out uint source);
if (source == 0 /* DIRECT */) {
    // slotCount is the answer — render slot picker with that many entries
} else {
    // Fallback path: either curated default or "unknown" UI
}
```

### v2 — "add dye to previously-undyed item" (shipped 2026-05-17)

```c
int32_t crimson_save_set_object_list_present(
    CrimsonSaveHandle* handle,
    uint32_t block_idx,
    const CrimsonPathStep* path,
    size_t path_len,
    uint32_t field_idx,
    int32_t present_flag    // 1 = make present, 0 = make absent
);
```

`present_flag == 1` flips the field's mask bit and materializes the
list as **count = 1** with a single default-empty `ItemDyeSaveData`
element (every dye scalar absent). The caller then mutates the
element's RGBA / material / color-group fields via
`crimson_save_set_scalar_field_present` to fill it in.

The count=1 shape is mandatory — a count=0 header is genuinely
ambiguous to the decoder's `body_offset` probing (it greedy-matches
`marker_run_plus_zeros`, which the encoder can't re-emit with a
different count). Materializing one default element + the
`zero1_count_u24` variant disambiguates the round-trip; the
[`c_abi_object_list_present_roundtrip_dye_data_list_slot104`](../src/c_abi/mod.rs)
integration test pins the contract.

`present_flag == 0` clears the mask bit and any elements; the encoder
emits nothing for an absent field, so `present(1) → present(0)` is
byte-identical to the original.

The element class for the default empty element is discovered by
scanning the save tree for any sibling block of the same parent class
where the field is present-and-non-empty, then copying its first
element's `class_index`. If no template exists in the save (e.g. the
user never dyed anything), the call returns `NOT_FOUND`; the C# editor
can either prompt the user to dye one item via the in-game UI first
or surface this as a "save state too clean to seed default class"
edge case.

C# call shape:
```csharp
// item dye list at ItemSaveData._itemDyeDataList (field index 14)
var path = new[] {
    new CrimsonPathStep { field_idx = _inventorylistIdx, element_idx = invIdx },
    new CrimsonPathStep { field_idx = _itemListIdx, element_idx = itemIdx },
};
var rc = crimson_save_set_object_list_present(
    handle, blockIdx, path, path.Length, 14, present_flag: 1
);
// rc == 0 (OK): the item now has one empty dye slot. Drive the RGBA
// scalars onto element 0 via set_scalar_field_present (path extended
// by one more step with field_idx = 14, element_idx = 0).
```

---

## Open RE — next session

**All three gamedata tables RE'd 2026-05-16 (earlier this iteration)
and the `_itemKey → _partPrefabKey` linkage bridged later the same day.**

Linkage details (from `_probe_partprefab_string_linkage`): the cross-
reference is a 3-table join, not a direct field. ItemInfo carries
`prefab_data_list[].prefab_names[]` as `StringInfoKey` u32 hashes;
each hash resolves through `stringinfo.pabgb` to a prefab name like
`"cd_phm_00_hel_00_0354_c"`; that name matches one of the 1,105
`prefab_name` strings in `partprefabdyeslotinfo`, whose row key is
the `PartPrefabKey` the dye editor needs. The bridge precomputes the
join at load time —
[`src/c_abi/item_part_prefab.rs`](../src/c_abi/item_part_prefab.rs)
exposes `lookup_count` / `lookup_key_at` / `lookup_prefab_name_at` so
the C# editor can drop `dye_slot_counts.json` entirely.

Coverage in 1.07: ~120 items resolve to at least one partprefab key
(out of 507 `is_dyeable=1` items). The remaining dyeable items carry
prefab variants for body types (goblin/dwarf/tribe meshes) that share
the human male's dye-slot layout but aren't themselves listed in
`partprefabdyeslotinfo`; the editor can fall back on the human male
prefab's slot count for those cases.

Probes for cross-version regression:

```powershell
cargo test --lib --features c_abi _probe_partprefab_string_linkage -- --ignored --nocapture
cargo test --lib --features c_abi _probe_itemkey_partprefab_linkage -- --ignored --nocapture
```

Diagnostic re-run (when a future patch may have changed the dye
schemas):

```powershell
cargo test --lib --features c_abi _probe_dye_gamedata_tables -- --ignored --nocapture
cargo test --lib --features c_abi _probe_dye_gamedata_rows -- --ignored --nocapture
```

Both probes live in [`src/c_abi/character_info.rs`](../src/c_abi/character_info.rs)
and write raw extracted bytes to `out/dye_probe/` for plcli inspection.

---

## Addendum (2026-05-17): slot103 multi-dye probe — model corrections

`slot103/save.save` has dye applications across the active character,
one mount, and (the previously-known) inventory test sample.
`_probe_item_dye_data_with_mercenary_resolution` enumerates the lot.
Three corrections to the model previously assumed in this doc:

### 1. `_dyeColorGroupInfoKey` is the **theme**, and the save's RGB indexes a 109-position palette inside it

The save's `_dyeColorR/G/B/A` are NOT freeform — they index into a
**109-position logical-RGBA palette** owned by `_dyeColorGroupInfoKey`.
The 10 colorGroup rows correspond to the in-game NPC dye-menu themes
(`Her_Color_Group_I` = 埃爾南德 / Hernand, `Por_Color_Group_I` = 波羅琳 /
Pororin, plus Dem×3 tiers, Kwe, Del, Cal, Tom, Bar).

**Confirmed by exhaustive cross-check (2026-05-17)**: every one of the
11 RGBs observed in `slot103/save.save` hits an exact gradient
position (zero off-grid values). On-disk byte order in the gradient
is **BGRA**, NOT RGBA — the parser swaps to logical `(R, G, B, A)`
order so the values match the save's u8 fields directly. The two
pinned mappings:

| Theme | colorGroup u32 | Observed save RGBs → palette positions |
|---|---|---|
| Her_Color_Group_I | `0xc88211f5` | `#f22121`→17, `#a65757`→43, `#d98585`→22, `#d99999`→21, `#594444`→70, `#a64848`→44 |
| Por_Color_Group_I | `0x2a85f874` | `#736e3f`→62, `#403913`→85, `#59542a`→73, `#736a15`→66, `#8c8530`→55 |

**Palette layout** (109 positions per theme):
- Positions **0-8**: 9 grayscale records (lightness ramp).
- Positions **9-108**: 10 chromatic rows × 10 columns. Each row is a
  lightness tier (R-channel of the swapped record steps through
  `0xf2 → 0xd9 → 0xbf → 0xa6 → 0x8c → 0x73 → 0x59 → 0x40 → 0x26 → 0x1a`),
  each column varies the secondary channel from "pale" (col 0) to
  "fully saturated" (col 9). The theme determines the **hue
  direction** baked into the row colors — Hernand uses red-dominant,
  Pororin uses olive/yellow-dominant, etc.

### C ABI for the dye picker

[`src/c_abi/dye_color_group_info.rs`](../src/c_abi/dye_color_group_info.rs)
exposes the palette to the C# editor:

| Function | Purpose |
|---|---|
| `crimson_dye_color_group_info_palette_size(handle, key, *out_count)` | Number of positions for the theme (109 in 1.07) |
| `crimson_dye_color_group_info_palette_at(handle, key, idx, *out_r, *out_g, *out_b, *out_a)` | RGBA at position — write these directly into `_dyeColorR/G/B/A` |
| `crimson_dye_color_group_info_position_for_rgb(handle, key, r, g, b, *out_position)` | Reverse lookup — highlight which cell a currently-applied dye came from. Returns `NOT_FOUND` for off-grid RGBs (e.g. CE-modified saves) |

### Recommended C# editor UX

Replace the PyQt5 reference editor's freeform R/G/B sliders with a
**visual palette grid per theme**:

```text
Theme dropdown:  [ Hernand (埃爾南德) ▼ ]    <-- backed by crimson_dye_color_group_info_get_entry()

Palette grid (11 rows × ~10 cols):

  Grayscale row:   [ ■ ][ ■ ][ ■ ][ ■ ][ ■ ][ ■ ][ ■ ][ ■ ][ ■ ]                positions 0-8
  Tier f2:         [ ■ ][ ■ ][ ■ ][ ■ ][ ■ ][ ■ ][ ■ ][ ■ ][ ★ ][ ■ ]            positions 9-18, ★ = position 17 = #f22121
  Tier d9:         [ ■ ][ ■ ][ ★ ][ ★ ][ ■ ][ ■ ][ ■ ][ ■ ][ ■ ][ ■ ]            positions 19-28
  ...
  Tier 1a:         [ ■ ][ ■ ][ ■ ][ ■ ][ ■ ][ ■ ][ ■ ][ ■ ][ ■ ][ ■ ]            positions 99-108
```

Each cell renders the RGB from `_palette_at(theme, position)`. The
currently-applied dye is highlighted via `_position_for_rgb`. User
clicks a cell → editor writes the cell's R/G/B back into the save's
three u8 scalars at the chosen dye slot via the existing
`set_scalar_field_path` API.

**The dye-consumable item is bookkeeping only** — the visual is fully
determined by the palette position. The editor doesn't need a
`(consumable_ItemKey, theme) → position` mapping to render the
picker. Showing a tooltip like "this matches 鮮紅色染劑 in Hernand
theme" would require additional RE, but the v2 dye editor is fully
shippable without it.

The 4-byte tail per gradient record (`d4..cf` / `fe..f9` lightness
keys + a constant `e1 ff ff`) is not exposed by the bridge —
diagnostic data only, included in the
`dye_gradient_vs_slot103_rgbs` probe output for future investigation
if the editor wants per-tier metadata.

### 2. Three playable characters, one container per "active"

The game has 3 playable characters (Kliff / Damine / Oongka). The
**currently-active** character's equipment lives in `EquipmentSaveData`;
the **other two playables** are stored under `MercenaryClanSaveData._mercenaryDataList[]`
as if they were mercenaries, distinguished by their `_characterKey`:

| Slot | `_characterKey` | Resolved name | Container |
|---|---:|---|---|
| Active | — | Kliff | `EquipmentSaveData._list[N]._item<child>` (18 equip slots) |
| Mercenary[0] | 4 | Damian (= Damine) | `MercenaryClanSaveData._mercenaryDataList[0]._equipItemList[]` |
| Mercenary[1] | 6 | Oongka | `MercenaryClanSaveData._mercenaryDataList[1]._equipItemList[]` |

Switching active character in-game presumably moves the equipment
between these two locations. **Any equipment-related editor feature
(dye, gem socket, item swap, …) must walk both locations** or it will
silently miss two-thirds of the player's gear.

### 3. Mounts use `MercenarySaveData` too — keyed by `_characterKey`

Mounts are stored in `MercenaryClanSaveData._mercenaryDataList[]` just
like human mercenaries, distinguished by `_characterKey` internal-name
prefix (`Riding_*` / `Animal_*` / `Vehicle_*`). The save uses
`_characterKey` (CharacterKey u32) — NOT the 18-row MercenaryKey from
[`mercenaryinfo.pabgb`](../src/c_abi/mercenary_info.rs) — to identify
each mount template, with a cat-byte in the hi-byte that must be
stripped (`& 0xFFFFFF`) before lookup against
[`characterinfo.pabgb`](../src/character_info/mod.rs).

slot103 example (the user's only dyed mount):

```
charKey=31378 mercNo=3135 name=Animal_Black_Horse_Wild_31378
  _equipItemList[1]: itemKey=1511010
    dye[0] mask=[de,00] R=140 G=133 B=48  colorGroup=Por palette=1
    dye[1] mask=[5f,00] R=217 G=153 B=153 colorGroup=Her  slotNo=1
    dye[2] mask=[5f,00] R= 89 G= 68 B= 68 colorGroup=Her  slotNo=2
    dye[3] mask=[5f,00] R=115 G=106 B= 21 colorGroup=Por  slotNo=3
```

slot103 holds 6 mount instances total (3 unique horses for each
playable + 1 wild tamed horse + 1 wild Stefano + 1 balloon + 1 wagon).
The `_isMainMercenary` flag identifies which mount is currently
summoned (the balloon at the moment), distinct from which mount has
dyed equipment.

### Implication for the C# editor's item enumerator — `crimson_save_list_all_items`

`crimson_save_list_inventory_items` (the original flat enumerator)
walks `InventorySaveData._inventoryList[N]._itemList[M]` only —
245 mercenary-equip + 20 mercenary-inv + 18 active-equip + 1 active
reserve items are invisible to it. **Shipped 2026-05-17 (later this
iteration)**: [`crimson_save_list_all_items`](../src/c_abi/all_items.rs)
yields each player-owned item with its container kind + owner
identity, in one call:

```c
// 64-byte repr(C) record per item — same two-call sizing pattern as
// list_inventory_items.
typedef struct CrimsonItemRecord {
    uint32_t block_idx;
    uint32_t container_kind;       // see container_kind constants
    uint32_t path_len;              // always 2 in v1
    uint32_t path_step_0_field;     // descent step 0: field idx
    uint32_t path_step_0_element;   // descent step 0: list element idx
    uint32_t path_step_1_field;
    uint32_t path_step_1_element;
    uint32_t inventory_key;
    uint32_t item_key;
    uint32_t slot_no;
    uint32_t flags;                  // LOCKED / NEW_MARK / HAS_DYE_DATA /
                                     //   HAS_SOCKET_DATA / OWNER_IS_MAIN_MERCENARY
    uint32_t owner_character_key;    // cat-byte stripped
    uint64_t item_no;
    uint64_t owner_mercenary_no;     // 0 for active-character records
} CrimsonItemRecord;

// Container kinds:
//   0 = ACTIVE_EQUIP
//   1 = ACTIVE_USE_RESERVE
//   2 = INVENTORY
//   3 = MERCENARY_EQUIP
//   4 = MERCENARY_INVENTORY
```

Slot103 baseline output:

| container_kind | count | source path |
|---|---:|---|
| ACTIVE_EQUIP | 18 | `EquipmentSaveData._list[N]._item<locator>` |
| ACTIVE_USE_RESERVE | 1 | `EquipmentSaveData._useItemSaveList[N]._reserveItem<locator>` (empty slots skipped) |
| INVENTORY | 545 | `InventorySaveData._inventoryList[N]._itemList[M]` |
| MERCENARY_EQUIP | 245 | `MercenaryClanSaveData._mercenaryDataList[N]._equipItemList[M]` |
| MERCENARY_INVENTORY | 20 | `MercenaryClanSaveData._mercenaryDataList[N]._inventoryItemList[M]` |
| **Total** | **829** | — |

Owner identity:
- **Active records** (`ACTIVE_EQUIP` / `ACTIVE_USE_RESERVE` /
  `INVENTORY`): `owner_character_key` is filled from
  `MercenaryClanSaveData._lastFocusCharacterKey` (cat-byte stripped),
  `owner_mercenary_no = 0`.
- **Mercenary records** (`MERCENARY_EQUIP` / `MERCENARY_INVENTORY`):
  `owner_character_key` is the enclosing `MercenarySaveData._characterKey`
  (cat-byte stripped); `owner_mercenary_no` distinguishes individual
  instances of the same template (e.g. multiple horses).

**Mutation compatibility for every kind**: the recorded 2-step path
plugs straight into `crimson_save_set_scalar_field_path` for ALL
five container kinds, including the mercenary ones. The previous
"3 levels deep" worry was a misread of the path-step semantics —
each step navigates one level relative to the previous, so
`[(_mercenaryDataList, N), (_equipItemList, M)]` from
`MercenaryClanSaveData` reaches `ItemSaveData` correctly (same
shape as `[(_inventoryList, N), (_itemList, M)]` from
`InventorySaveData`). Pinned by the
`live_path_navigation_reaches_item_save_data_for_every_kind` test.

**Filtering player-owned items** (the user's actual concern): the
829 records include 50+ NPC mercenary followers' gear. The
`item_record_flags::IS_PLAYER_OWNED` flag bit is set when the
container's owner is one of the three playables OR a mount owned
by a playable (`_characterKey` or `_ownedCharacterKey` in
`PLAYABLE_CHARACTER_KEYS = {1, 4, 6}`). Slot103 breakdown:
**619 player-owned** / **210 NPC followers**.

Known coverage gap: two mounts (`Riding_Horse_Tiuta_Unique_2050_kliff`
and `Animal_Stefano_Wild_31364`) have `_ownedCharacterKey` absent
and so fall outside the strict rule. The C ABI walker stays
conservative to avoid coupling the all-items hot path to a gamedata
load; the C# editor handles the widening client-side via the
**escape hatch** documented below.

### C# editor — `IS_PLAYER_OWNED` widening recipe

For the equipment-related UI tabs (dye / gem socket / item edit /
search), the editor walks `crimson_save_list_all_items` and decides
per-record whether to expose it. The strict `IS_PLAYER_OWNED` flag
covers 619 / 829 records on the slot103 baseline; two
player-controlled mounts are missed (see above). The recommended
pattern:

```csharp
// Pre-load characterinfo once (already loaded by the existing display-name
// resolver elsewhere in the editor — reuse that handle).
var charInfo = CrimsonCharacterInfo.Load(...);

// Mount-name prefixes the editor treats as "player-controlled when
// _ownedCharacterKey is absent". Pearl Abyss uses these consistently
// across 1.07; sanity-check against a fresh save on a new patch.
static readonly string[] MountNamePrefixes = {
    "Riding_",   // Tiuta horses, balloons, wagons, …
    "Animal_",   // tamed wild animals (Black Horse, Stefano, …)
    "Vehicle_",  // generic vehicle templates
};

bool IsEditable(CrimsonItemRecord r)
{
    // Fast path: strict flag covers the common case (Kliff's active
    // gear + inventory + Damine + Oongka + mounts with explicit
    // _ownedCharacterKey).
    if ((r.flags & CrimsonItemRecordFlags.IS_PLAYER_OWNED) != 0)
        return true;

    // Slow path: catch the two missing-_ownedCharacterKey mounts by
    // checking the resolved template name. Only runs for ~210 NPC
    // records on the slot103 baseline; trivial cost.
    if (r.container_kind == CrimsonContainerKind.MERCENARY_EQUIP ||
        r.container_kind == CrimsonContainerKind.MERCENARY_INVENTORY)
    {
        string? name = charInfo.LookupStringKey(r.owner_character_key);
        if (name != null && MountNamePrefixes.Any(p => name.StartsWith(p)))
            return true;
    }

    return false;
}
```

This widens the slot103 acceptance from 619 to 627 records (the
3 Tiuta_kliff equip items + 5 Stefano equip items). NPC mercenaries
still get rejected because their template names start with `NHM_*`
/ `NHW_*` / `NDM_*`, not `Riding_*` / `Animal_*` / `Vehicle_*`.

The C ABI does NOT promote `IS_PLAYER_OWNED` automatically for these
mounts because doing so would require the walker to parse
`characterinfo.pabgb` (a multi-megabyte gamedata table) on every
`crimson_save_list_all_items` call. Keeping that lookup on the C#
side preserves the enumerator as a gamedata-free, no-allocation
operation suitable for hot-path refresh after every edit.

**Excluded by design**: `FieldGimmickSaveData._item<locator>` (world
loot / chests, 4,260 in slot103), `StoreDataSaveData._storeSoldItemDataList`
(vendor inventory, 17), `FactionNodeElementSaveData._factionOwnedItemList`
(faction-owned, 1). Surface separately if a world-state / faction-state
editor lands.

### Probe ergonomics

Three new `#[ignore]` probes added in this session, all default to
`slot103/save.save` (override with `CRIMSON_DYE_PROBE_SAVE` or
`CRIMSON_LIVE_SAVE`):

| Probe | Purpose |
|---|---|
| `_probe_save_skeleton_slot103` | TOC class histogram + every host of `ItemSaveData` recursively. Use first to confirm the container shape in a new patch. |
| `_probe_item_dye_data_anywhere_slot103` | All `_itemDyeDataList` hits across **every** ItemSaveData host, regardless of parent class. |
| `_probe_item_dye_data_with_mercenary_resolution` | Adds CharacterKey resolution: every mercenary/mount tagged with its resolved name + `_mercenaryNo` + `_isMainMercenary` flag. |

```powershell
cargo test --lib --features c_abi _probe_save_skeleton_slot103 -- --ignored --nocapture
cargo test --lib --features c_abi _probe_item_dye_data_with_mercenary_resolution -- --ignored --nocapture
```

---

## Cross-references

- `src/c_abi/character_info.rs` — `_probe_item_dye_data` `#[ignore]`
  probe (slot0 schema baseline) plus three slot103 probes added 2026-05-17
  (`_probe_save_skeleton_slot103`, `_probe_item_dye_data_anywhere_slot103`,
  `_probe_item_dye_data_with_mercenary_resolution`). Re-run with
  `--ignored --nocapture` to refresh.
- `docs/save-mutation-version.md` — staleness contract; the C# editor
  must use `get_mutation_version` to invalidate its dye snapshot
  after each edit.
- `D:\Github\CRIMSON-DESERT-SAVE-EDITOR-AND-GAME-MODS\
  CrimsonSaveEditor\parc_inserter3.py:1740-1785` — PyQt5 RE for the
  per-element binary layout (8 fields). Subtract: that source has the
  inaccuracies noted above.
- `D:\Github\CRIMSON-DESERT-SAVE-EDITOR-AND-GAME-MODS\
  CrimsonSaveEditor\gui.py:6603-7201` — PyQt5 UI implementation;
  good reference for the C# editor's UI shape.
