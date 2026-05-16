# Socket editor — scope + schema baselines

Reference data for `CrimsonAtomtic`'s socket editor feature. The current
C# editor implementation handles **"replace gem"** (overwriting an
existing socket's `_itemKey`). This doc records what's needed for the
two remaining transitions:

- **absent → insert gem** (filling an opened-but-empty slot)
- **gem → absent** (removing a gem from a filled slot)

> Status (2026-05-16): Schema verified against the live 1.07 install's
> `slot104/save.save` (5 North Wind Tridents in a known gem
> distribution). **No new C ABI required** — the existing
> `crimson_save_set_scalar_field_present` / batch variant already
> handle both transitions, end-to-end round-trip proven byte-perfect
> by `c_abi_socket_insert_then_remove_roundtrip_slot104`.

---

## What slot104 contains (reference test layout)

5 instances of **"North Wind Trident" / 北風三叉 / itemkey 310031**,
each configured to exercise one socket-state permutation:

| # | `_maxSocketCount` | `_validSocketCount` | Filled positions | What it exercises |
|---|---:|---:|---|---|
| 1 | 5 | 5 | 1,2,3,4,5 (5 distinct gems) | baseline: all opened, all filled |
| 2 | 5 | **4** | 1,2 (1002979); 3,4 opened-empty; **5 not-yet-opened** | distinguishes opened-empty from not-yet-opened |
| 3 | 5 | 5 | 1..5 all 1002979 | all opened, all filled with same gem |
| 4 | 5 | 5 | 1,3,5 (1002979); 2,4 empty | sparse-filled pattern (odd) |
| 5 | 5 | 5 | 2,4 (1002979); 1,3,5 empty | inverse sparse pattern (even) |

Item **1002979** (爆走的力量審判) is a high-durability gem
(`_endurance` ~100 per instance — verified 99/100 in the dumps).
Vanilla gems are durability-less.

The probe that dumps this layout is
[`_probe_item_socket_data`](../src/c_abi/character_info.rs) — re-run
with `cargo test --lib --features c_abi _probe_item_socket_data --
--ignored --nocapture` after a future patch to re-verify the schema.

---

## Save schema — `ItemSaveData` socket-related fields

Verified byte-perfect against **all 5,464 socket-bearing items across
10 save files** (slot0..slot200 range; both reference items + the
maintainer's own playthroughs). Zero anomalies — the schema below is
exhaustive within the saves observed.

```text
ItemSaveData (one item slot in the inventory)
├── [ 7] _endurance: u16                  // weapon's own durability
├── [11] _maxSocketCount: u8              // ALWAYS 5 — save-side padding constant
├── [12] _validSocketCount: u8            // currently OPENED slots (0..=5)
└── [13] _socketSaveDataList: ObjectList<ItemSocketSaveData>
        count == _maxSocketCount (always 5)
        Each element's mask byte tells you whether THAT physical slot is filled.

ItemSocketSaveData (2 fields, 1-byte mask = 2 bits used)
├── [ 0] _currentEndurance: u16    // gem's per-instance durability
└── [ 1] _itemKey: u32             // gem's gamedata itemkey
```

> **Important interpretive note**: `_maxSocketCount = 5` is **universal
> across every ItemSaveData** — consumables, materials, quest items, and
> weapons alike all carry the same value. The full-scan probe
> [`_probe_item_socket_data_all_slots`](../src/c_abi/character_info.rs)
> across 5,464 socket-bearing items returned exactly one bucket in
> the histogram: `max=5: 5464`. This means **`_maxSocketCount` is a
> save-side allocation padding constant, NOT the per-item true max**.
> The true per-item socket cap lives in gamedata's
> `iteminfo.pabgb → DropDefaultData::socket_valid_count: u8` (parsed by
> [`src/item_info/structs.rs:402`](../src/item_info/structs.rs)).
> The editor should treat `_maxSocketCount` only as "the size of the
> socket-list buffer the save format reserved" and read the true
> physical capacity from iteminfo when validating user actions.

### Where ItemSaveData lives in the save tree

`ItemSaveData` appears in TWO distinct places:

1. **InventorySaveData → `_inventorylist[N]` → `_itemList[M]`** — items
   in the player's bags / banks. Reached without any Locator step.
2. **EquipmentSaveData → `_list[N]` → `_item<child>`** — currently-
   equipped items (weapon, armor, helmet, shoes, …). The `_item`
   field is an `ObjectLocator` that wraps the `ItemSaveData`. The C
   ABI's `navigate_mut_to_parent` handles the Locator step
   transparently — pass `element_idx = 0` (ignored for Locator) in
   the path step.

The phase-4 probe
[`_probe_item_socket_data_anywhere`](../src/c_abi/character_info.rs)
walks the entire decoded tree depth-first and confirmed that 100% of
`_socketSaveDataList` occurrences host on `ItemSaveData` blocks (1,252
in slot104 across both InventorySaveData and EquipmentSaveData
contexts).

**Path examples** for `set_scalar_field_path` / `set_scalar_field_present`:

```text
Inventory item:
  block_idx = <InventorySaveData TOC index>
  path = [
    { field_idx: <_inventorylist>,      element_idx: <inventory bucket> },
    { field_idx: <_itemList>,            element_idx: <item slot> },
    { field_idx: <_socketSaveDataList>,  element_idx: <socket position 0..4> },
  ]
  field_idx (leaf) = 0 (_currentEndurance) or 1 (_itemKey)

Equipped item:
  block_idx = <EquipmentSaveData TOC index>
  path = [
    { field_idx: <_list>,                element_idx: <equipment slot N> },
    { field_idx: <_item>,                element_idx: 0 },  ← Locator: ignored
    { field_idx: <_socketSaveDataList>,  element_idx: <socket position 0..4> },
  ]
  field_idx (leaf) = 0 (_currentEndurance) or 1 (_itemKey)
```

Both shapes are validated by round-trip tests:
- [`c_abi_socket_insert_then_remove_roundtrip_slot104`](../src/c_abi/mod.rs)
  — InventorySaveData case (insert→remove on an empty opened slot)
- [`c_abi_socket_remove_then_reinsert_roundtrip_equipment_slot104`](../src/c_abi/mod.rs)
  — EquipmentSaveData case (remove→reinsert on a filled socket; works
  even when the user's CE-modified items have no empty slots)

### Gem `_currentEndurance` encoding — 0xFFFF sentinel

The per-socket `_currentEndurance: u16` distinguishes durability-bearing
gems from no-durability gems via a sentinel:

| Gem class | Encoding | Examples seen on slot104 |
|---|---|---|
| Has durability | `0..=100` (or whatever the gem's own `_endurance` max is) | 1002972/1002973/1002974/1002979 — typically `99..100` |
| **No durability** | `0xFFFF` (= 65535, max u16) | 1002815, 1002848 (also seen in the equipped armor) |

When inserting a gem the editor MUST pick the right sentinel:
- Look up the gem's iteminfo to determine if it has durability
- If yes: write `_currentEndurance = <gem's max durability>` (typically 100)
- If no: write `_currentEndurance = 0xFFFF`

Writing the wrong sentinel probably won't crash but may show oddly
in the gem-removal NPC UI.

### Slot encoding (verified)

| State | mask | data_size | Fields present |
|---|---|---:|---|
| Filled (gem installed) | `[0x03]` | 32 | Both `_currentEndurance` + `_itemKey` |
| Opened-but-empty | `[0x00]` | 26 | Neither |
| **Not-yet-opened** | `[0x00]` | 26 | **Same as opened-empty** |

> Critical insight: the list is **positional + fixed-size**. The
> distinction between "opened-empty" and "not-yet-opened" is NOT in
> the socket list — it's in `_validSocketCount`. Slots
> `0..=_validSocketCount-1` are opened; slots
> `_validSocketCount..=_maxSocketCount-1` are not yet opened. Their
> list entries still exist and look identical to opened-empty (the
> 1-byte mask difference between filled `[0x03]` and empty `[0x00]`
> accounts for the 6-byte size difference, which is exactly
> `sizeof(u16) + sizeof(u32)`).

### Cross-check on item-level `_endurance`

Confirmed via the slot104 histogram:
- 538 items at `_endurance = 0xFFFF` (max — weapons/armor at full)
- 11 items at `_endurance = 100` (durable items: the standalone gem
  + others)
- 1 item at `_endurance = 30`

The PyQt5 reference editor's `endurance & 0xFF` / `endurance >> 8`
decomposition (interpreting the low byte as durability + high byte as
"socket count") **does not apply to weapons** — those store full
sockets-related state separately in `_maxSocketCount` /
`_validSocketCount` / `_socketSaveDataList`. The `(>> 8) & 0xFF`
trick may be a heuristic the PyQt5 editor uses for items it can't
fully decode, but it isn't the gamedata-correct interpretation.

---

## Mutations — existing C ABI is sufficient

All four transitions the editor needs are implementable today via the
existing length-changing primitives. No new ABI needed.

| Editor action | C ABI calls |
|---|---|
| **Replace gem in slot N** | `crimson_save_set_scalar_field_path` on `_itemKey` (+ optionally `_currentEndurance` if gem has durability) — what the editor already does. |
| **Insert gem into empty opened slot N** | Two `crimson_save_set_scalar_field_present(make_present=1, init_bytes=…)` ops, or one `crimson_save_set_scalar_fields_present_batch` with two ops. One per field (`_currentEndurance` + `_itemKey`). The path descends `_inventorylist → _itemList → _socketSaveDataList`, with `socket_elem_idx = N`. |
| **Remove gem from slot N** | Mirror of insert: two `crimson_save_set_scalar_field_present(make_present=0)` ops (no init bytes needed). |
| **Open a new socket (`_validSocketCount += 1`)** | Single `crimson_save_set_scalar_field_path` on field index 12 (`_validSocketCount`, u8). No list mutation needed — the list entry was already there from save creation. |
| **Close a socket (`_validSocketCount -= 1`)** | Same as above. **Caller must ensure** the higher-index slot is empty first (mask=[0x00]); leaving a gem in a >valid_count slot probably hides it from the in-game UI but doesn't crash the save. |

### Insert example (pseudocode for the C# editor)

```csharp
// Insert gem (itemkey 1002979, endurance 100) into socket index N of
// item at (inv_elem M, item_elem K) in InventorySaveData block B.

var path = new CrimsonPathStep[] {
    new() { field_idx = INV_LIST_FIELD,    element_idx = (uint)M },
    new() { field_idx = ITEM_LIST_FIELD,   element_idx = (uint)K },
    new() { field_idx = SOCKET_LIST_FIELD, element_idx = (uint)N },
};

byte[] endurance = BitConverter.GetBytes((ushort)100);
byte[] itemKey   = BitConverter.GetBytes((uint)1002979);

var ops = new CrimsonScalarPresentBatchOp[] {
    new() {
        block_idx = B, field_idx = 0,             // _currentEndurance
        path = pathPin, path_len = (nuint)path.Length,
        make_present = 1, bytes = endurancePin, bytes_len = (nuint)endurance.Length,
    },
    new() {
        block_idx = B, field_idx = 1,             // _itemKey
        path = pathPin, path_len = (nuint)path.Length,
        make_present = 1, bytes = itemKeyPin, bytes_len = (nuint)itemKey.Length,
    },
};

int rc = NativeMethods.crimson_save_set_scalar_fields_present_batch(
    handle, opsPin, (nuint)ops.Length, out nuint failedIdx);
```

### Remove example

```csharp
var ops = new CrimsonScalarPresentBatchOp[] {
    new() {
        block_idx = B, field_idx = 0,
        path = pathPin, path_len = (nuint)path.Length,
        make_present = 0, bytes = IntPtr.Zero, bytes_len = 0,
    },
    new() {
        block_idx = B, field_idx = 1,
        path = pathPin, path_len = (nuint)path.Length,
        make_present = 0, bytes = IntPtr.Zero, bytes_len = 0,
    },
};
int rc = NativeMethods.crimson_save_set_scalar_fields_present_batch(
    handle, opsPin, (nuint)ops.Length, out nuint failedIdx);
```

### Round-trip contract

Both directions of the round-trip MUST yield a byte-identical body when
no other mutation runs between them:

| Direction | Test | Coverage |
|---|---|---|
| **Insert → Remove** | [`c_abi_socket_insert_then_remove_roundtrip_slot104`](../src/c_abi/mod.rs) | InventorySaveData path (no Locator) |
| **Remove → Re-insert** | [`c_abi_socket_remove_then_reinsert_roundtrip_equipment_slot104`](../src/c_abi/mod.rs) | EquipmentSaveData path (with Locator descent) |

Both tests skip cleanly when slot104 isn't present locally. Together
they cover both container shapes (inventory vs equipment) and both
mutation directions (fill→clear vs clear→fill).

---

## Caveats / known unknowns

1. **`_isLocked` interaction not tested.** The slot104 tridents don't
   have `_isLocked = true`. If a locked item rejects socket mutations
   at the engine level, the C ABI will still happily mutate the
   save — but the game might validate on load. Worth a one-off live
   test before shipping the editor UI.

2. **Higher-than-`_validSocketCount` mutations**. The C ABI does NOT
   validate that the socket index being mutated is within
   `_validSocketCount`. The full-scan probe found **zero saves with
   `filled > _validSocketCount`** across 10 save files / 5,464 items
   — including a save with several known CE-modified items. The user
   confirmed the CE pattern: bump `_validSocketCount` first (so it
   matches the desired filled count), then fill all the new slots.
   That keeps `filled == _validSocketCount` post-mod and looks
   "consistent" within the save format, so the in-save invariant is
   preserved. **The actual anomaly that CE produces is**
   `save._validSocketCount > iteminfo.socket_valid_count` — a
   gamedata cross-check, not a save-internal one. Concrete example
   from slot104: itemKey `1002285` (嘟嘟鳥放電盔甲) has
   `_validSocketCount = 5` in the save but iteminfo's
   `socket_valid_count` is around 1–2 vanilla. Engine accepts it,
   gem-removal NPC interface gets confused.

   **Editor recommendation**: validate user inputs against
   `iteminfo.socket_valid_count` (the gamedata cap), warn when the
   target slot index exceeds it. The save-side `_validSocketCount`
   is whatever the player has historically opened — trust it for the
   list-element addressing, but compare against gamedata before
   showing "Open new socket" UI.

3. **Gem `_endurance` interpretation**. Gems like 1002979 carry their
   own `_endurance` (u16, ~100 max). Whether the gem's durability ticks
   down with use is engine behaviour, not save-format behaviour. The
   editor can set any u16 value; the engine's clamp is unknown but the
   verified `99/100` values across slot104's filled sockets show the
   game rounds down with use (some sockets have been "used" once).

4. **`_transferredItemKey`**. All 5 tridents have
   `_transferredItemKey = 0xbb0f0101` (= `(itemkey << 8) | 0x0101`).
   This appears to be a derived value from `_itemKey` — not directly
   socket-related, but worth noting since the PyQt5 reference editor
   rewrites it when cloning items.

5. **Wrong-category gem validation is a gamedata cross-check, not a
   save check.** The save format accepts any u32 itemkey as a gem,
   regardless of whether the engine considers it valid for the host
   weapon. The "this gem isn't allowed on this weapon" rule lives in
   `iteminfo.pabgb`:
   - `DropDefaultData::socket_item_list: CArray<ItemKey>` — explicit
     allowed-gem list per weapon (parsed in
     [`src/item_info/structs.rs:399`](../src/item_info/structs.rs)).
   - `DropDefaultData::use_socket: u8` / `socket_valid_count: u8` —
     whether the item even supports sockets and how many.

   CE-bypassed gems (i.e. a weapon containing a gem itemkey NOT in
   its `socket_item_list`) load fine at the save format level but the
   game's gem-removal NPC interfaces may refuse to interact with them.
   **Surfaced via the four new advisory ABIs below** — purely
   informational, never block.

---

## Cross-check C ABI (advisory only — never block a mutation)

These four entry points on the iteminfo handle expose gamedata facts
the save's `_socketSaveDataList` cannot tell on its own. They are
**pure queries**: the editor decides what to do (warn / red icon /
log) and is **always free to write any mutation** regardless of what
they return. CE-modified saves with `_validSocketCount >
socket_valid_count`, or gems outside the allowed list, load cleanly
in the game (just with some NPC-UI quirks) — the ABI mirrors that
permissiveness.

| Function | Purpose |
|---|---|
| [`crimson_iteminfo_lookup_socket_caps(handle, item_key, *out_use_socket, *out_valid_count)`](../src/c_abi/iteminfo.rs) | Gamedata `use_socket` flag + `socket_valid_count`. Compare against save's `_validSocketCount` to flag CE-bumped overflows. `OK` even when `use_socket=0` so the editor can distinguish "item not in iteminfo" (`NOT_FOUND`) from "item exists but not socket-capable" (OK + `use_socket=0`). |
| [`crimson_iteminfo_socket_allows_gem(handle, item_key, gem_key, *out_allowed)`](../src/c_abi/iteminfo.rs) | Is `gem_key` on `item_key`'s **per-weapon vendor/crafting allowed list** (union of `socket_item_list + add_socket_material_item_list`)? `OK` with `*out_allowed = 1/0`. `NOT_FOUND` only when `item_key` itself is missing. Note: this list is narrower than "all gems the engine accepts" — for the wider "is this a gem at all" check, use the canonical gem set below. |
| [`crimson_iteminfo_socket_allowed_gem_count(handle, item_key, *out_count)`](../src/c_abi/iteminfo.rs) | Size of `item_key`'s per-weapon allowed list (0 for non-socket items). |
| [`crimson_iteminfo_socket_allowed_gem_at(handle, item_key, idx, *out_gem_key)`](../src/c_abi/iteminfo.rs) | Read the per-weapon allowed gem at insertion index `idx`. Order matches the on-disk concatenation. |
| [`crimson_iteminfo_canonical_gem_count(handle, *out_count)`](../src/c_abi/iteminfo.rs) | Size of the **canonical gem set** (every itemkey with `item_type=74` AND `category_info=2501`). This is the engine's own gem-row marker and captures all gems the editor's gem-picker should offer. **190 entries on 1.07**; the 43 distinct gems observed across the user's slot104 saves are all in it. |
| [`crimson_iteminfo_canonical_gem_at(handle, idx, *out_gem_key)`](../src/c_abi/iteminfo.rs) | Read the canonical gem itemkey at sorted-ascending index `idx`. Stable for the lifetime of one handle. |

### Verified against the user's CE-modified slot104 items

Running `c_abi_iteminfo_socket_caps_and_gem_allow_live` on the live
1.07 install shows the gamedata reality behind the user's CE
modifications:

| Item | itemKey | iteminfo `use_socket` | iteminfo `socket_valid_count` | iteminfo `allowed_gems` | Save state |
|---|---:|---:|---:|---:|---|
| 嘟嘟鳥馬羅尼雷射頭盔 | 1002284 | **0** (non-socket!) | 0 | 0 | save has 3 gems installed |
| 嘟嘟鳥放電盔甲 | 1002285 | 1 | 0 | 0 | save has 5 gems installed |
| 嘟嘟鳥里西的鞋子 | 1000316 | 1 | 0 | 0 | save has 3 gems installed |

The helmet (1002284) is vanilla-defined as **completely non-socket-
capable** (`use_socket = 0`), yet the CE-modified save has gems in
it and the game loads fine. The armor/shoes declare socket-capable
but with `valid_count = 0` (zero usable sockets) and no allowed-gem
list, yet the save has gems in them too. The C ABI surfaces this
state so the editor can show a warning like "vanilla cap: 0 / save
cap: 5" without blocking the mutation.

### Editor decision matrix (suggested)

```
save_valid > iteminfo_valid              → "CE / above-vanilla cap" warning chip
iteminfo.use_socket == 0                  → "not a sockets item in vanilla" warning chip
gem not in per-weapon allowed list        → "non-standard gem (out of weapon's vendor list)" warning chip
gem not in canonical gem set              → "this isn't a gem at all" warning chip (CE-style itemkey)
per-weapon allowed list is empty          → fall back to canonical gem set for the dropdown
```

All cases let the user proceed — they are signals, not gates.

### Gem-picker dropdown — which list to show?

Three tiers of granularity, all available via the C ABI; the editor
chooses based on the user's preferred mode:

1. **Strict (default UI)** — show only `canonical_gem_*` (190 gems).
   These are everything the engine considers a gem. Gem-picker user
   gets a clean filterable list keyed on `item_type=74 + category=2501`.
2. **Per-weapon recommended** — for a focused weapon, show the
   intersection of `canonical_gem_*` and `socket_allowed_gem_*`
   (smaller subset = "what vendor / crafting UIs explicitly call out
   for this weapon"). Useful as a "Recommended for this weapon" tab.
3. **Freeform / CE mode** — let the user type any u32 itemkey. The
   advisory checks still surface warnings but never block; the save
   format + runtime accept any itemkey in `_socketSaveDataList[N]._itemKey`.

---

## Cross-references

- [`src/c_abi/character_info.rs`](../src/c_abi/character_info.rs) —
  `_probe_item_socket_data` (the schema-dumping diagnostic against
  slot104).
- [`src/c_abi/mod.rs`](../src/c_abi/mod.rs) —
  `c_abi_socket_insert_then_remove_roundtrip_slot104` (the end-to-end
  insert+remove round-trip test).
- [`docs/dye-editor-scope.md`](dye-editor-scope.md) — sibling editor
  doc; the dye editor uses the same `set_scalar_field_present` shape
  to add/remove `_grimeOpacity` on existing dye entries.
- [`docs/save-mutation-version.md`](save-mutation-version.md) —
  cache-coherency contract; the C# socket editor MUST call
  `crimson_save_get_mutation_version` between snapshot reads and
  mutations.
