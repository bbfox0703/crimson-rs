# Dye editor — scope + data baselines

Reference data for CrimsonAtomtic's Dye editor feature. The PyQt5
reference editor at
`D:\Github\CRIMSON-DESERT-SAVE-EDITOR-AND-GAME-MODS\CrimsonSaveEditor`
already implements this; this doc records what the in-house C# editor
needs and what crimson-rs needs to ship (or not ship).

> Status (2026-05-16): Save schema verified **and** all three
> gamedata-side `dye*.pabgb` tables RE'd + bridged behind C ABI.
> Parsers + bridges shipped in
> [`src/dye_color_group_info/`](../src/dye_color_group_info/),
> [`src/part_prefab_dye_texture_pallete_info/`](../src/part_prefab_dye_texture_pallete_info/),
> [`src/part_prefab_dye_slot_info/`](../src/part_prefab_dye_slot_info/)
> with matching `c_abi/*.rs` bridges. **Replaces the PyQt5 reference
> editor's hand-maintained `dye_slot_counts.json`** with gamedata-driven
> data, plus per-slot default materials the JSON never carried.
> Remaining work: `_itemKey → _partPrefabKey` cross-reference (lives
> in `iteminfo.pabgb` or a sibling `partprefab*` table) — the C# editor
> needs this linkage to actually look up slot counts by item.

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

## Gamedata baselines — `0008/gamedata/binary__/client/bin/`

Three `.pabgb` + `.pabgh` tables, all RE'd against the live 1.07 install.

| File | Size (cmp/unc) | Rows in 1.07 | Maps | Parser / Bridge |
|---|---:|---:|---|---|
| `dyecolorgroupinfo.pabgb` + `.pabgh` | 6 KB / 9 KB, index 82 B | **10** | `DyeColorGroupInfoKey (u32)` → color group name (`"Her_Color_Group_I"`, …) + 109-record RGBA gradient palette. Save's `_dyeColorGroupInfoKey` references this. | [`dye_color_group_info`](../src/dye_color_group_info/mod.rs) / [`c_abi/dye_color_group_info.rs`](../src/c_abi/dye_color_group_info.rs) |
| `partprefabdyeslotinfo.pabgb` + `.pabgh` | 86 KB / 370 KB, index 9 KB | **1,105** | `PartPrefabKey (u32)` → prefab internal name + per-slot detail (3 material indices, 3 default-material names, 3 mask bytes, next-prefab / `.pac` tail per slot). Replaces `dye_slot_counts.json`. | [`part_prefab_dye_slot_info`](../src/part_prefab_dye_slot_info/mod.rs) / [`c_abi/part_prefab_dye_slot_info.rs`](../src/c_abi/part_prefab_dye_slot_info.rs) |
| `partprefabdyetexturepalleteinfo.pabgb` + `.pabgh` | 0.8 KB / 4 KB, index 68 B | **11** (keys 0..=10) | `PartPrefabDyeTexturePalleteKey (u16)` → palette tier with 2–3 sub-records (material name + icon DDS + texture DDS + optional variant name & strength). Save's `_texturePalleteKey` references this. | [`part_prefab_dye_texture_pallete_info`](../src/part_prefab_dye_texture_pallete_info/mod.rs) / [`c_abi/part_prefab_dye_texture_pallete_info.rs`](../src/c_abi/part_prefab_dye_texture_pallete_info.rs) |

The `binarygimmickchart__` `*dye*` / `dyewater` `.binarygimmick` files
in the scan output are decorative gimmicks (basecamp props, fabric
deco), NOT item-dye gamedata — ignore those.

### Verified row schemas (2026-05-16, live 1.07)

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
    u8 mask[3],               // active/visible flags
    CString tail_name,        // next sub-prefab name; for the LAST slot in a row,
                              //   the full .pac asset path
}
```

`CString` here is `[u32 len][len bytes]` with NO trailing NUL — same
shape iteminfo.pabgb uses.

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
| `crimson_part_prefab_dye_slot_info_lookup_slot_mask(handle, prefab_key, slot_idx, out_mask[3])` | Raw 3-byte mask. |
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

### v2 — "add dye to previously-undyed item"

Blocker: `set_scalar_field_present` rejects `ObjectList` fields
(meta_kind 6/7). The reference editor uses a `dye_cli.exe` subprocess
to handle PARC insertion + offset fix-up.

To support this in C#, crimson-rs would need a new ABI:

```c
// Toggle an absent ObjectList field's present-bit AND emit an empty
// ObjectList header at the right offset, so the field becomes a
// valid zero-element list.
int32_t crimson_save_set_object_list_present(
    CrimsonSaveHandle* handle,
    uint32_t block_idx,
    const CrimsonPathStep* path,
    size_t path_len,
    uint32_t field_idx,
    int32_t make_present
);
```

Implementation outline: choose the `header_variant` based on the
field's `meta_kind` + neighbouring lists in the same block (in
practice probably `zero1_count_u24` — the variant `ItemSaveData`'s
other ObjectLists use). Defer until there's a concrete user demand.

---

## Open RE — next session

**All three gamedata tables RE'd 2026-05-16 (this iteration).** The
remaining blocker for the C# editor to consume slot counts is the
**`_itemKey → _partPrefabKey` cross-reference** — needs to live in
`iteminfo.pabgb` (or a sibling `partprefab*` table). Once that's
bridged, the C# editor can drop `dye_slot_counts.json` entirely.

Diagnostic re-run (when a future patch may have changed the dye
schemas):

```powershell
cargo test --lib --features c_abi _probe_dye_gamedata_tables -- --ignored --nocapture
cargo test --lib --features c_abi _probe_dye_gamedata_rows -- --ignored --nocapture
```

Both probes live in [`src/c_abi/character_info.rs`](../src/c_abi/character_info.rs)
and write raw extracted bytes to `out/dye_probe/` for plcli inspection.

---

## Cross-references

- `src/c_abi/character_info.rs` — `_probe_item_dye_data` `#[ignore]`
  probe (re-run with `--ignored --nocapture` to refresh).
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
