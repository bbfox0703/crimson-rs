# `globalgameevent.pabgb` body — investigation notes (2026-05-18)

Per-row body schema RE for the globalgameevent gamedata table (103
rows in 1.07).

Status: **v1 shipped 2026-05-18**. Two new bridge fields exposed via
C ABI — `group_key` (universal) + `paloc_key` (78% coverage). Per-
group action lists documented below but not yet implemented (Tier 2).

## Body-size histogram (1.07)

```text
 56 bytes:  1 row   (RoyalSupply)
 61 bytes: 23 rows  (FactionBlockEvent_* family)
121 bytes: 10 rows  (single-action events — e.g. Sudden_Fish_Increase_*)
143 bytes: 29 rows  (2-action events — Drought, Flood, Typhoon, …)
165 bytes: 18 rows  (3-action events — ColdWave, Pest_Infestation, …)
187 bytes:  6 rows  (4-action events — Epidemic, …)
209 bytes:  6 rows  (5-action events — Royal_Hoarding, …)
231 bytes:  8 rows  (6-action events — Bandit_Appearance, …)
253 bytes:  2 rows  (7-action events — Earthquake_Pywel, …)
```

Each step of +22 bytes between consecutive sizes signals a **22-byte
fixed-length action entry** that repeats in the action list. Different
shapes for RoyalSupply / FactionBlock confirm that the body is
**polymorphic by `GlobalGameEventGroupKey`**.

## Universal fields (safe to bridge for all 103 rows)

| Offset | Type | Field | Notes |
|---:|---|---|---|
| `body[0]` | u8 | leading flag | Always 0x00 across all 103 rows |
| `body[1..3]` | u16 LE | **`group_key`** | Cross-reference to `globalgameeventgroup` (7 distinct values: `0x4240, 0x4241, 0x4244, 0x4246, 0x4247, 0x4248, 0x4249`) |
| `body[3..7]` | u32 LE | constant `0x1F8AF380` | Class-tag / format-version constant; same across every row |

The `group_key` is the most useful new field — it lets the editor
categorise events by kind (e.g. all WeatherEventGroup events use the
same 22-byte action-list shape).

## Group key → category mapping

Cross-checked against the existing `globalgameeventgroup` bridge:

| `group_key` | Group name | Body shape | Row count |
|---:|---|---|---:|
| `0x4240` | `WeatherEventGroup` | header + N×22-byte actions + tail PalocStringRef | many (Drought / Flood / …) |
| `0x4241` | (RoyalSupply / event-of-events) | header + u32 count + N×u16 cross-refs | 1 (the single `RoyalSupply` event) |
| `0x4244` | (FactionBlockEvent — likely) | header + sparse fields + trailing FactionNodeKey | ~23 (the `FactionBlockEvent_*` family) |
| `0x4246` / `0x4247` / `0x4248` / `0x4249` | TBD — fetch group names from bridge | TBD | TBD |

## Conditionally-present field: `paloc_key`

Most rows (everything *except* the RoyalSupply / FactionBlockEvent
groups) carry a **`PalocStringRef`** structure at body offset
`~0x12..0x2D`:

```text
body[0x12..0x16]: u32 class_tag = 0x0002C12C  (always — the "PalocStringRef" type tag)
body[0x16]:       u8 zero
body[0x17..0x19]: u16 key_echo = same as PABGH key
body[0x19..0x1B]: u16 zero
body[0x1B..0x1F]: u32 name_len = 14   (the 14-char ASCII decimal number)
body[0x1F..0x2D]: 14 ASCII bytes — PALOC key as decimal u64 string
```

The **14-char ASCII number is the PALOC localization key** for the
event's display name. Confirmed by:

- Consecutive events differ by exactly `2^32` in the PALOC key value
  (e.g. `Typhoon_Delesyian` = `Drought_Varnian` + 19 × 2^32) — the
  classic `(hi32 = event_key, lo32 = namespace)` PALOC layout.
- The same PalocStringRef appears **twice** in each row (once after
  the header, once at the end) — looks like `(start_state, end_state)`
  or `(input, computed)` mirror; values are identical.

When present, this lets the editor display the localized event name
via the existing `crimson_paloc_lookup_*` ABI. The `RoyalSupply` and
`FactionBlockEvent_*` groups don't have this structure and need
internal-name resolution instead (which the existing bridge already
provides).

## Per-group body shapes (deferred — research-grade only)

### WeatherEventGroup (0x4240) — well-understood

```text
[0x00..0x07]  universal header
[0x07..0x0F]  8 zero bytes
[0x0F]        0x01 flag
[0x10..0x12]  00 00
[0x12..0x2D]  PalocStringRef #1
[0x2D..0x3E]  17 zero bytes
[0x3E..0x42]  u16 ??? + u16 action_count   (or u32 + u32?)
[0x42..0x42+N×22]  Action entries (each 22 bytes — schema TBD)
[end - 23 bytes]   PalocStringRef #2 (same content as #1)
```

Each 22-byte action entry contains:
- `u32 = 1` (flag)
- `u16 target_key` (cross-reference to some other event / group / item)
- `u32 = 0x0007A120` (= 500000, constant — duration in ms?)
- `u32 = 0x000C3500` (= 800000, constant — magnitude?)
- 4 bytes mixed zeros and a trailing 0x01

**Editor-actionable**: action_count is countable, target_key is
exposable. The constants 500000 / 800000 across all actions
suggest they're "default impact magnitude" — possibly per-event
overridable but the test sample never deviates.

### RoyalSupply (0x4241) — well-understood

Only 1 row (the meta-event `RoyalSupply`). Body:

```text
[0x00..0x07]  universal header (group_key = 0x4241)
[0x07..0x1D]  zeros + 01 01 01 flag bytes
[0x1D..0x2C]  zeros
[0x2C..0x30]  u32 count = 4
[0x30..0x38]  4 × u16 RoyalSupplyInfoKey (0x4242, 0x4243, 0x4245, 0x4248)
```

The cross-referenced keys match the `royalsupply.pabgb` rows
(`RoyalSupply_Hernand`, `RoyalSupply_Demeniss`, `RoyalSupply_Varnia`,
`RoyalSupply_???`). Worth bridging if a "royal supply event" editor
surfaces.

### FactionBlockEvent (0x4244) — partially understood

23 rows, all named `FactionBlockEvent_Her_Node_*`. Body:

```text
[0x00..0x07]  universal header (group_key = 0x4244)
[0x07..0x0B]  zeros (possible "0001 0000 0001 0000 76FB FFFF" header — TBD)
[0x0B..0x14]  `01 00 00 00 76 fb ff ff` — looks like a flag + u32 negative sentinel
[0x14..0x35]  zeros (mostly)
[0x35..0x39]  flag bytes (`01 02 02 00`)
[0x39..0x3D]  4-byte trailer — `u32 FactionNodeKey` (e.g. 0x000F43EB = 1000939)
```

The trailing FactionNodeKey points into `factionnode.pabgb` and
identifies which node the event blocks. Worth bridging if a
"faction territory event" editor surfaces.

### Groups 0x4246 / 0x4247 / 0x4248 / 0x4249

Not yet examined per-shape. Likely each has its own body layout. If
needed, run the probe and inspect samples — the universal header
fields will identify the group key for routing.

## Shipped v1 bridge surface (2026-05-18)

Rust:

```rust
pub struct GlobalGameEventInfoEntry {
    pub key: u32,
    pub name: String,
    pub group_key: u32,    // body[1..3] widened to u32
    pub paloc_key: u64,    // 0 when absent (RoyalSupply / FactionBlock* groups)
}
```

C ABI (in [`src/c_abi/global_game_event_info.rs`](../src/c_abi/global_game_event_info.rs)):
- `crimson_global_game_event_info_lookup_string_key` — internal name
- `crimson_global_game_event_info_lookup_group_key` — `(handle, key, *out_group_key) → i32`
- `crimson_global_game_event_info_lookup_paloc_key` — `(handle, key, *out_paloc_key) → i32`
- Standard 6 functions (`load_from_file`, `load_from_bytes`, `free`,
  `entry_count`, `lookup_string_key`, `get_entry`) — hand-written
  instead of macro-generated to accommodate the extra per-entry data.

**Coverage on the 1.07 live install**: `group_key` 100% (103/103);
`paloc_key` 76.7% (79/103 rows — `RoyalSupply` + the 23
`FactionBlockEvent_*` rows have body shapes that lack the embedded
`PalocStringRef` and return 0).

**Test pinning** (`global_game_event_info_body_fields_live` + the
matching C ABI test):
- 4 baseline `(key, group_key, paloc_key)` tuples
- Every row's `group_key` ∈ {0x4240, 0x4241, 0x4244, 0x4246..=0x4249}
- When `paloc_key != 0`, its hi32 equals the row's event key (PALOC
  `(hi32, lo32)` convention)
- Synthetic-payload unit tests for `extract_group_key` /
  `extract_paloc_key` covering happy path + wrong-tag + wrong-length
  rejection.

## What's deferred

- **22-byte action entry schema** for WeatherEventGroup — what are
  the 500000 / 800000 constants? Need to find a row where these
  deviate. Resolution path: probe across multiple game versions; if
  values are universally constant, hardcode and move on.
- **Groups 0x4246..0x4249 body shapes** — never sampled. Run the
  probe with extended print to capture one row per group.
- **The dual PalocStringRef question** — both refs always carry the
  same data in 1.07. Possible explanation: redundancy for hot-patching
  during runtime; could differ in modded saves. Worth re-probing if
  someone reports an event with mismatched start/end paloc keys.

## Probe to re-run

```text
cargo test --lib --features c_abi _probe_global_game_event_body_dump \
  -- --ignored --nocapture
```

Lives in [`src/global_game_event_info/mod.rs`](../src/global_game_event_info/mod.rs).
Dumps body-size histogram, first 8 rows in full, one example per
distinct size, plus per-offset cross-row analysis (constant detection,
distinct-value count, magnitude buckets).
