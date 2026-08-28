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
>
> Update (2026-08-28, game 2.00): still no new C ABI required, but
> **two write rules in this doc were wrong or incomplete and shipped a
> real bug**. A socket edit that violates either makes the game
> discard the item's entire socket block — every socket reads back
> in-game as not-yet-opened, with no error visible from the format
> side. Both are now measured against **22,019** socket-bearing
> `ItemSaveData` blocks in four game-written saves (`slot0`,
> `slot101`, `slot104`, `slot107`) rather than slot104 alone:
>
> 1. [`_currentEndurance` is the gem's own
>    `iteminfo.max_endurance`](#gem-_currentendurance--it-is-the-gems-iteminfomax_endurance)
>    — not a sentinel the editor classifies gems into.
> 2. [`_validSocketCount` absent ≠
>    `0`](#_validsocketcount--absent-is-not-zero) — opening the first
>    socket is a presence promotion, and a gem must never sit outside
>    the opened window.
>
> Plus one primitive-level trap for callers that batch:
> [promote-then-write-in-place is invalid inside a deferred
> batch](#mutations--existing-c-abi-is-sufficient).

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

Item **1002979** (暴走的力量審判) is a durability-bearing gem
(`max_endurance` 100 — verified 99/100 in the dumps). It is one of
only 14 such gems in the 190-entry canonical gem set; the other 176
carry `max_endurance = 65535` and the client draws no durability row
for them. See the `_currentEndurance` section.

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
├── [12] _validSocketCount: u8            // currently OPENED slots (1..=5;
│                                        //   ABSENT means none — never 0)
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

### Gem `_currentEndurance` — it is the gem's `iteminfo.max_endurance`

> Revised 2026-08-28. The earlier text framed `0xFFFF` as a
> *no-durability sentinel* the editor had to classify gems into. The
> observation was right — `0xFFFF` really does mark a gem the game
> treats as having no durability — but the framing hid the simpler
> rule underneath it, and an editor written to the old text got the
> value wrong on every durability-bearing gem. See the consequence
> below; it is not cosmetic.

**The rule.** A freshly-socketed gem's `_currentEndurance` is that
gem's own `iteminfo.max_endurance`, read straight off the item:

```rust
// crimson_iteminfo_lookup_max_endurance(handle, gem_key, &mut out)
_currentEndurance = iteminfo[gem_key].max_endurance
```

There is no classification step and no sentinel to pick. `0xFFFF` is
not a magic value the save format assigns to durability-less gems —
it is simply what `max_endurance` *holds* for them, and the save
copies it like any other cap. A worn gem sits **below** its cap
(`99` / `95` observed); nothing ever sits above it.

#### Why we believe this — three independent lines of evidence

**1. Gamedata partitions the canonical gem set cleanly.** Over the
190-entry canonical gem set (`item_type == 74 && category_info ==
2501`) on the live 2.00 install, `max_endurance` takes exactly two
values:

| `max_endurance` | Gems | Which |
|---:|---:|---|
| `100` | **14** | the `AbyssGear_*_Special` family — item keys 1002862 and 1002969..1002982 (1002971 is absent) |
| `65535` | **176** | every other gem |

No third bucket, no gradient. The 14 are exactly the set a player
recognises as the durability-bearing "暴走 / Greater" gems.

**2. The in-game tooltip agrees with `max_endurance`, item by item.**
Two gems, both `item_type 74` / `category_info 2501`, differing only
in this field:

| Item key | `string_key` | `max_endurance` | In-game tooltip |
|---:|---|---:|---|
| `1002815` | `Item_Stat_AbyssGear_MoveSpeedRate_LV3` | `65535` | 移動速度 Lv.3 — **no durability row at all** |
| `1002974` | `Item_Stat_AbyssGear_ElectricityResistance_Special` | `100` | 雷電抗性 Lv5 — **耐久度 100/100** |

So `max_endurance == 0xFFFF` is what the client itself reads as "this
gem has no durability, don't draw the row". That is the affirmative
evidence for the original claim, and it is also why the claim and the
rule above are the same statement rather than competing ones.

**3. The save corpus never deviates.** Four game-written saves
(`slot0` / `slot101` / `slot104` / `slot107`) hold **22,019** blocks
carrying `_socketSaveDataList`, of which **734 sockets are filled**
across **54 distinct gems**. Every one of those 734 carries either its
gem's `max_endurance` or a lower worn value — never a higher one. Per
gem the values are deterministic, and crucially **no gem ever mixes
`65535` with a real durability value**:

| Gem | `max_endurance` | Values seen in saves | n |
|---:|---:|---|---:|
| `1002979` | `100` | `100` ×69, `99` ×2 | 71 |
| `1002974` | `100` | `100` ×42, `95` ×8 | 50 |
| `1002815` | `65535` | `65535` ×20 | 20 |

If `65535` were a per-socket sentinel rather than the gem's cap, a
durability gem would have to show it at least once. None does.

#### Consequence of getting it wrong

CrimsonAtomtic's socket editor implemented the old "pick the right
sentinel" text as a constant `0xFFFF`, so every durability-bearing gem
it inserted landed at `current > max`. The reported symptom was that
**every socket on the edited item reads back in-game as not-yet-opened
(未開封)** — gems included — while the save still loads and still
passes HMAC. The rejection is at the engine's own validation layer, so
there is nothing to observe from the format side.

**Be precise about what that does and doesn't prove.** The same editor
was violating the [`_validSocketCount`
rule](#_validsocketcount--absent-is-not-zero) at the same time, and
*that* violation is mechanically proven to produce the state (the ABI
returns `NOT_SCALAR`, the count stays absent, and the gem provably sits
outside the opened window). The two were not isolated from each other,
so the independent in-game effect of an over-cap `_currentEndurance`
is **not** established here.

What *is* established is that `current > max` is a value the game
itself never writes — 22,019 items, zero exceptions — so an editor has
no reason to produce it and no basis for predicting how the engine
treats it. Write `max_endurance`. Both rules have to hold anyway.

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

### `_validSocketCount` — absent is not zero

> Added 2026-08-28, alongside the `_currentEndurance` revision above.
> The schema block near the top of this doc describes the field as
> `u8 // currently OPENED slots (0..=5)`, which reads as though `0` is
> how a never-socketed item is stored. It is not, and an editor that
> assumed so could not open a socket at all.

The game encodes "no socket has ever been opened on this item" by
leaving `_validSocketCount` **absent from the presence mask**. It
never writes an explicit `0`.

Of the 22,019 blocks carrying `_socketSaveDataList` in the four
reference saves, **16,463 hold a 0-element list** — ordinary
non-socket items (consumables, materials, …), where
`_maxSocketCount` and `_validSocketCount` are both absent. The
socket-capable population is the other **5,556**, every one with a
5-element list:

| `_validSocketCount` | Items | Meaning |
|---|---:|---|
| **absent** | 5,278 | socket-capable, never opened |
| `1` | 27 | |
| `2` | 88 | |
| `3` | 73 | |
| `5` | 90 | |
| `0` | **0** | **never observed — this encoding does not exist** |

(No `4` either, but that is a content accident, not a rule.)

Two consequences for a mutating editor:

1. **Opening the first socket is a presence promotion, not a scalar
   write.** `crimson_save_set_scalar_field_path` resolves the leaf's
   byte range and rejects an absent field with `NOT_SCALAR (-12)`,
   because an absent field has no range. Use
   `crimson_save_set_scalar_field_present(make_present = 1,
   init_bytes = [n])` instead. Raising an already-present count is an
   ordinary scalar write.
2. **A filled socket must never sit at an index the count doesn't
   cover.** All **734** filled sockets in the corpus sit inside their
   item's opened window; **zero** sit outside it. An editor that
   writes a gem without first opening the slot produces the state
   behind CrimsonAtomtic's bug report: the item comes back in-game
   with every socket sealed, gem invisible. So open the slot *before*
   writing the gem — that ordering also means a failure to open leaves
   the save consistent instead of stranding a gem outside the
   window.

Note this is a **save-internal** invariant and is unrelated to
gamedata's `socket_valid_count`, which is advisory only — see caveat
2 below. Forcing `_validSocketCount` above the vanilla cap (e.g. 5 on
an item gamedata caps at 3, or at 0) is accepted by the engine and
confirmed working in-game by the maintainer.

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
| **Replace gem in slot N** | `crimson_save_set_scalar_field_path` on `_itemKey` **and** `_currentEndurance` — the latter is not optional, see the `_currentEndurance` section. |
| **Insert gem into empty opened slot N** | Two `crimson_save_set_scalar_field_present(make_present=1, init_bytes=…)` ops, or one `crimson_save_set_scalar_fields_present_batch` with two ops. One per field (`_currentEndurance` + `_itemKey`). The path descends `_inventorylist → _itemList → _socketSaveDataList`, with `socket_elem_idx = N`. |
| **Remove gem from slot N** | Mirror of insert: two `crimson_save_set_scalar_field_present(make_present=0)` ops (no init bytes needed). |
| **Open a new socket (raise `_validSocketCount` to N+1)** | **Two cases, and they take different primitives.** Field *present* → `crimson_save_set_scalar_field_path` on field index 12. Field *absent* (the common case — 21,742 of 22,019 items) → `crimson_save_set_scalar_field_present(make_present=1, init_bytes=[n])`; the scalar setter returns `NOT_SCALAR (-12)` here. No list mutation either way — the list entries were already there from save creation. Do this **before** writing the gem. |
| **Close a socket (lower `_validSocketCount`)** | Scalar write, same field. **Caller must ensure** every slot at or above the new count is empty first (mask=`[0x00]`) — a gem left outside the window is a state the game never writes and the engine rejects the item's whole socket block for it. |

> **Deferred batches change which of these are valid.** Inside a
> `crimson_save_begin_deferred_redecode` batch,
> `set_scalar_field_present` leaves the promoted field's decoded byte
> range at `start == end == 0` until the commit re-decodes — see the
> comment in `toggle_one_scalar_presence_in_place`
> ([`src/c_abi/mod.rs`](../src/c_abi/mod.rs)): *"start/end are stale
> but the encoder ignores them for scalar emission; they'll be
> refreshed by the re-decode."* A follow-up
> `set_scalar_field_path` on that same field therefore computes
> `expected = 0` and fails `LENGTH_MISMATCH`. In immediate mode the
> per-call re-decode hides this completely.
>
> So **do not promote a field and then write it in place within one
> batch.** Either compute the final value before promoting, or raise it
> through the presence surface — which writes from `init_bytes` and
> never reads `start`/`end`. Note `present(1)` on an already-present
> field is a documented no-op, so the latter takes a `present(0)` then
> `present(1, value)` pair.
>
> This is the shape that bit CrimsonAtomtic's "apply a gem set"
> action: it wraps its per-slot loop in a deferred batch, so on a
> never-socketed item the first slot promoted `_validSocketCount` to 1
> and every subsequent slot's raise failed — silently dropping every
> gem after the first.

### Insert example (pseudocode for the C# editor)

```csharp
// Insert gem (itemkey 1002979) into socket index N of item at
// (inv_elem M, item_elem K) in InventorySaveData block B.
//
// The endurance is NOT a constant: it is the gem's own
// iteminfo.max_endurance (100 for this key; 65535 for a
// durability-less gem). Assume the slot has already been opened —
// see the _validSocketCount section.

var path = new CrimsonPathStep[] {
    new() { field_idx = INV_LIST_FIELD,    element_idx = (uint)M },
    new() { field_idx = ITEM_LIST_FIELD,   element_idx = (uint)K },
    new() { field_idx = SOCKET_LIST_FIELD, element_idx = (uint)N },
};

ushort max = 0;   // crimson_iteminfo_lookup_max_endurance(iteminfo, 1002979, &max)
byte[] endurance = BitConverter.GetBytes(max);
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

   **Editor recommendation**: never gate on
   `iteminfo.socket_valid_count`. Confirmed in-game by the maintainer
   (2026-08-28): forcing an item to 5 opened sockets works fine even
   where gamedata caps it lower — and gamedata's cap can be
   misleadingly low anyway. Weapon `201004` (格萊斯刺劍) reports
   `use_socket = 1, socket_valid_count = 0`, yet the *game itself*
   wrote `_validSocketCount` 2 and 3 on the player's own copies of it.
   Treat the gamedata cap as advisory at most. The save-side
   `_validSocketCount` is the load-bearing value — trust it for
   list-element addressing, and keep the save-internal invariant
   (`filled index < _validSocketCount`), which is the one the engine
   actually enforces.

3. **Gem `_endurance` interpretation** — *resolved 2026-08-28, see
   the [`_currentEndurance`
   section](#gem-_currentendurance--it-is-the-gems-iteminfomax_endurance).*
   The open question here used to be "the engine's clamp is unknown".
   It is now known well enough to act on: the cap is the gem's
   `iteminfo.max_endurance`, a fresh gem is written *at* it, and worn
   gems sit below it. The editor is still free to write any u16, but
   writing **above** the cap is not a cosmetic choice — the engine
   discards the host item's whole socket block. Whether durability
   ticks down per use remains engine behaviour we don't model.

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
