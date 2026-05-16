# Dye editor — scope + data baselines

Reference data for CrimsonAtomtic's Dye editor feature. The PyQt5
reference editor at
`D:\Github\CRIMSON-DESERT-SAVE-EDITOR-AND-GAME-MODS\CrimsonSaveEditor`
already implements this; this doc records what the in-house C# editor
needs and what crimson-rs needs to ship (or not ship).

> Status (2026-05-15): Schema verified. **No new C ABI required for v1**
> (edit-existing-dye scenario). Gamedata-side parsing of `dye*.pabgb`
> tables — deferred to next session — would replace the PyQt5 reference
> editor's hand-maintained `dye_slot_counts.json`.

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

The probe scan turned up **three relevant `.pabgb` + `.pabgh` tables**.
These are next-session gamedata RE work — see "Open RE" below.

| File | Size (cmp/unc) | Estimated rows | Maps |
|---|---:|---:|---|
| `dyecolorgroupinfo.pabgb` + `.pabgh` | 6 KB / 9 KB index 82 B | ~10 | `DyeColorGroupInfoKey (u32)` → color group definition (name + base RGB?). Save's `_dyeColorGroupInfoKey` references this. |
| `partprefabdyeslotinfo.pabgb` + `.pabgh` | 86 KB / 370 KB, index 9 KB | **~730** | Per-prefab dye-slot configuration. **Replaces the PyQt5 editor's `dye_slot_counts.json`** — tells the editor how many dye slots an item supports without a hand-maintained database. |
| `partprefabdyetexturepalleteinfo.pabgb` + `.pabgh` | 0.8 KB / 4 KB, index 68 B | ~5 | `PartPrefabDyeTexturePalleteKey (u16)` → material palette (Cloth / Metal / Leather / etc.). Save's `_texturePalleteKey` references this. |

The `binarygimmickchart__` `*dye*` / `dyewater` `.binarygimmick` files
in the scan output are decorative gimmicks (basecamp props, fabric
deco), NOT item-dye gamedata — ignore those.

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

**Parse the three `dye*.pabgb` tables to replace the PyQt5 reference
editor's hand-maintained JSON catalog with gamedata-driven data.**
Recommended order:

1. **`dyecolorgroupinfo.pabgb`** (tiny — ~10 rows). Resolves
   `_dyeColorGroupInfoKey` → group name. Pattern: probably an
   anchor-scannable `[u32 key][u32 name_len][name][...body]` like
   `gimmickinfo.pabgb`, but the `.pabgh` exists too so it might be
   PABGH-indexed like `characterappearanceindexinfo`. Probe to
   confirm.
2. **`partprefabdyetexturepalleteinfo.pabgb`** (tiny — ~5 rows).
   Resolves `_texturePalleteKey` → material name. Smallest table; do
   this second to get the schema pattern right with a low-volume
   sample.
3. **`partprefabdyeslotinfo.pabgb`** (large — ~730 rows). Per-prefab
   slot counts. Replaces `dye_slot_counts.json`. Will need the
   `partprefab*` key type understood — see also `_partPrefabKey` in
   `ItemSaveData` (if it exists) for the cross-reference.

When these three are parsed, the C# editor can drop hand-maintained
JSON entirely.

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
