# `mercenaryinfo.pabgb` body — investigation notes (2026-05-18)

Captured for future sessions. The bridge currently ships `(key, name)`
only (18 rows). This doc records what we know about the **40-byte
fixed-length body** that follows each row's name, along with the
in-game cross-checks that **did not confirm** the most useful field
guess.

Status: **paused** — `max_count` semantic doesn't cleanly match
in-game observation, so the body schema is not safe to bridge yet.
Pick this up when a downstream editor surfaces a need that motivates
finishing the RE.

## Body layout (40 bytes, 17 of 18 rows; Pet has +8 trailing bytes)

```text
offset  size  field                       observed values
─────────────────────────────────────────────────────────
[ 0]    u8    leading_zero                always 0x00 across 18 rows
[ 1- 4] u32   small_count                 1 / 3 / 10 / 20 / 30 / 50  (see "Open question 1")
[ 5- 8] u32   optional_secondary          0xFFFFFFFF on 13 rows; 30 or 50 on the other 5
[ 9-12] u32   sentinel                    always 0xFFFFFFFF — looks like a retired field
[13]    u8    type_enum                   1..=12, monotonic by-row (see table below)
[14-29] 16 B  flag_bitfield               16 mostly-0/1 bytes; per-row patterns vary
[30]    u8    sub_enum                    0..=3 — Vehicle family is the main differentiator
[31]    u8    group_letter                ASCII 0x40 + N — '@', 'A', 'B', … 'I' (one per type cluster)
[32-35] u32   default_hash                0xeac5e173 on 16 rows; 0x1f3fec11 on 2 (WarMachine + Raptor)
[36-39] u32   trailer                     0 on 17 rows; 1 on Pet (signals the +8 byte extras below)

[40-43] u32   ?key                        Pet only: 0x0000b02d (= 45101)
[44-47] u32   ?count                      Pet only: 1
```

### `type_enum` (byte 13) — pinned by name correlation

| Enum | Rows |
|---:|---|
| 0x01 | `Mercenary_Main` |
| 0x02 | `Vehicle`, `Vehicle_Horse`, `Vehicle_Dragon`, `Vehicle_WarMachine`, `Vehicle_WarMachine_Raptor`, `Vehicle_Special` |
| 0x03 | `Wagon` |
| 0x04 | `Pet` |
| 0x05 | `Domestic`, `Fish` |
| 0x06 | `Mercenary_Melee` |
| 0x07 | `Mercenary_Range` |
| 0x08 | `Mercenary_Worker` |
| 0x09 | `Mercenary_Shop` |
| 0x0a | `RemoteControl` |
| 0x0b | `RecoveryItem` |
| 0x0c | `Mercenary_GuestWorker` |

This is **the most useful field** in the body and is safe to bridge
even without resolving the rest — it lets the editor classify a
mercenary instance into Pet / Mount / Combat / Worker etc. without
hardcoded name-prefix matching.

### `group_letter` (byte 31)

| Letter | Type cluster |
|---|---|
| '@' (0x40) | Mercenary_Main |
| 'A' (0x41) | Vehicle family |
| 'B' (0x42) | Wagon |
| 'C' (0x43) | Pet |
| 'D' (0x44) | Animals (Domestic / Fish) |
| 'E' (0x45) | Workers (Worker / GuestWorker) |
| 'F' (0x46) | RemoteControl |
| 'G' (0x47) | RecoveryItem |
| 'H' (0x48) | Mercenary_Melee / Mercenary_Range |
| 'I' (0x49) | Mercenary_Shop |

Effectively a **coarser cluster ID** that groups related `type_enum`
values. Redundant with `type_enum` for most purposes; included here
for completeness.

## Open question 1: what is the `small_count` field?

The byte at offset 1 holds small positive integers (1 / 3 / 10 / 20 /
30 / 50). The natural guess is "maximum simultaneous instances per
category" — but **in-game behaviour disagrees** (user-supplied data,
2026-05-18):

| Template | small_count | In-game observation |
|---|---:|---|
| Mercenary_Main | 50 | Matches — 50 mercenaries cap is right |
| Vehicle_Horse | 10 | **Mismatch** — Stable holds more than 10 horses; only one ridden at a time |
| Wagon | 1 | **Mismatch** — player can own 2-4 wagons, only one in use |
| Pet | 3 | **Mismatch** — collection cap is 30, only one called out at a time |
| Domestic / Fish | 20 / 30 | Unverified |
| Mercenary_Melee/Range/Worker | 50 | Plausible |

The mismatch pattern doesn't fit "called-out simultaneously" either —
Pet 3 vs "1 called out" doesn't work. Possible interpretations to
test next:

- **Initial cap** (pre-upgrade) that the player can extend later via
  research / stable upgrades. The 50 mercenary cap fits because it
  doesn't appear to be upgradable.
- **Default UI display limit** (rows shown in some "Recruit X" panel).
- **Per-tier capacity** of a specific facility (Stable Tier 1 = 10 horses).
- **Some other balancing knob** unrelated to inventory.

**Resolution path**: peek at `cd_uitexture_worldmap_*` UI sequencer
files (`0014/sequencer/03_ui_seq/gamemain/play/`) that show the
recruitment UI — they may reference `mercenaryinfo` fields by name
in their bindings. Or IDA-decompile the recruitment loader for the
field offset / semantic.

## Open question 2: what is `default_hash` (bytes 32-35)?

Two distinct values across 18 rows:

| Hash | Rows |
|---|---|
| `0xeac5e173` | 16 rows (all except the WarMachine pair) |
| `0x1f3fec11` | `Vehicle_WarMachine`, `Vehicle_WarMachine_Raptor` |

Pattern looks like a **Jenkins `hashlittle2` hash** of some default
asset name — possibly:

- **Default skill set** the mercenary spawns with
- **Default appearance / customisation seed**
- **Default class / behaviour profile**

The WarMachine-specific value strongly suggests "default skill set" —
war machines have a distinct combat moveset. To resolve, brute-force
the hash against the skill name table (`skill.pabgb`) and the
animation table (`paa` files). The `crimson_calculate_checksum` ABI
plus the existing `crimson_skillinfo_*` bridge can drive that.

## Open question 3: the 16-byte `flag_bitfield` (bytes 14-29)

16 bytes of mostly 0 / 1 values per row, with patterns differing
across rows. Examples (Mercenary_Main):

```
01 01 01 01 00 01 00 00 00 01 00 00 00 00 00 00
```

vs Vehicle_Horse:

```
01 00 01 00 01 01 01 00 01 00 00 00 00 00 00 03
```

Could be:

- **16 booleans** (one per per-instance capability flag — `can_combat`,
  `can_carry_item`, `can_breed`, `can_skill_train`, …)
- **2 packed u64** representing some bit-flag enum

The byte-30 `sub_enum` (0/1/2/3) differentiates Vehicle subtypes
which suggests the flag region carries per-subtype capability hints.

## Open question 4: Pet's +8 byte trailer

Pet's body is 48 bytes, not 40. The trailing `01 00 00 00 2d b0 00
00 01 00 00 00` decodes as:

- `01 00 00 00` (u32 = 1) — "has extras" flag (`trailer` field at offset 36)
- `2d b0 00 00` (u32 = 45101) — **some key** (ItemKey? CharacterKey? HouseKey?)
- `01 00 00 00` (u32 = 1) — count?

The 45101 value is in the ItemKey range. Pet is the only row with a
mandatory pet-food / treat ItemKey reference — this could be the
"default pet food" or "starter accessory" reference. Cross-check
against the iteminfo bridge would resolve it.

## What to bridge when this is picked back up

**Tier 1 — safe to ship without further RE** (semantic confirmed by
name correlation):

```rust
pub struct MercenaryInfoBody {
    pub type_enum: u8,         // body[13] — 1..=12
    pub group_letter: u8,      // body[31] — 0x40..=0x49
    pub default_hash: u32,     // body[32..36] — raw, semantic TBD
}
```

ABI:
- `crimson_mercenaryinfo_lookup_type_enum(handle, key, *out_enum)`
- `crimson_mercenaryinfo_lookup_default_hash(handle, key, *out_hash)`

**Tier 2 — needs in-game / IDA work first**:

- `small_count` → "max" semantics undetermined (see Q1)
- 16-byte flag bitfield → per-bit semantics undetermined (Q3)
- Pet `+8 byte` extras → reference target undetermined (Q4)
- `default_hash` → Jenkins source undetermined (Q2)

## Probe to re-run

```text
cargo test --lib --features c_abi _probe_mercenary_body_dump \
  -- --ignored --nocapture
```

Lives in [`src/mercenary_info/mod.rs`](../../src/mercenary_info/mod.rs).
Dumps all 18 rows' bodies as hex + 8-byte rows + numeric-field probes.
Refresh after a new game patch to spot schema drift.
