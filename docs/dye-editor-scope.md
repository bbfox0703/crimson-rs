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

### 1. `_dyeColorGroupInfoKey` is the **theme**, not freeform colour

The save's `_dyeColorR/G/B/A` are not freeform — they index into a
**109-RGBA gradient** owned by `_dyeColorGroupInfoKey` (the bridge
already records this layout per row). The 10 colorGroup rows
correspond to the in-game NPC dye-menu themes
(`Her_Color_Group_I` = 埃爾南德 / Hernand, `Por_Color_Group_I` = 波羅琳 /
Pororin, plus Dem×3 tiers, Kwe, Del, Cal, Tom, Bar — 10 rows total).

Empirical pattern from slot103: RGBA strictly clusters by theme:

| Theme | colorGroup u32 | Observed RGBs |
|---|---|---|
| Her_Color_Group_I | `0xc88211f5` | `#a65757 #f22121 #d98585 #d99999 #594444 #a64848` (red family) |
| Por_Color_Group_I | `0x2a85f874` | `#736e3f #403913 #59542a #736a15 #8c8530` (olive family) |

**UX implication for the C# editor**: replace the freeform R/G/B
sliders (the PyQt5 reference exposes) with **two dropdowns** — theme
(10 options) + position-within-gradient. Off-gradient RGB values aren't
reachable from the in-game UI.

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

### Implication for the C# editor's item enumerator

`crimson_save_list_inventory_items` (the existing flat enumerator)
walks `InventorySaveData._inventoryList[N]._itemList[M]` only —
245 mercenary-equip + 20 mercenary-inv + 18 active-equip + 22 reserve
items are invisible to it.

A future `crimson_save_list_all_items` enumerator should yield each
item with its **container kind** + **owner identity**:

```text
container_kind ∈ { ActiveEquip, Inventory, MercenaryEquip,
                   MercenaryInventory, UseItemReserve, FieldGimmick }
owner = (character_key_or_zero, mercenary_no_or_zero)
```

This gives the C# editor enough info to render separate tabs for each
of Kliff / Damine / Oongka / each mount / each follower, plus the
shared inventory.

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
