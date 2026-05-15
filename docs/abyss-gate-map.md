# Abyss-gate per-gate mapping

**Status (2026-05-15)**: Phase 1 (per-gate mapping) **complete**.
Phase 2 (state-name hash decode) **partial** — three pinned state-hash
constants with empirical labels, but the underlying `HashCode32`
algorithm hasn't been cracked yet.

> Path B target: replace the bulk "Unlock All Abyss Gates" UX from the
> PyQt5 reference editor (`D:\Github\CRIMSON-DESERT-SAVE-EDITOR-AND-GAME-MODS\CrimsonSaveEditor\gui.py:12774-13052`)
> with per-gate controls in CrimsonAtomtic — toggle lock state, mark
> discovered, set puzzle state. This doc gives CrimsonAtomtic the data
> it needs to wire that up.

---

## What the existing reference impl does (and doesn't)

The PyQt5 editor's "Unlock All Abyss Gates" loads a hand-curated 398-entry
JSON pack (`knowledge_packs/No Map Reveal Abyss Gate Unlock Only.json`)
of `KnowledgeKey` values and bulk-injects them via `inject_knowledge_fast`
into `KnowledgeSaveData._list`. That **only flips the discovery flag**
(makes the gates show up on the map). It does NOT:

- Change the actual lock state of the gate gimmick.
- Resolve any puzzle the gate gates.
- Identify gates individually for selective unlock.

So the "unlock" is partial — the map shows everything, but the world
state remains as it was. Path B wants the full three-layer toggle.

---

## The three layers of "unlock"

To actually unlock a gate the way the player experiences it, an editor
needs to touch **three different things** in the save:

| Layer | Save target | What it does |
|---|---|---|
| **Discovery flag** | `KnowledgeSaveData._list` ← inject `KnowledgeKey` | Map icon appears |
| **Gate state** | `FieldGimmickSaveData._initStateNameHash` ← set to "activated" hash | Gate visibly opens / bridge extends / etc. |
| **Puzzle state** | `FieldGimmickSaveData._initStateNameHash` (different hash) | If the gate requires a puzzle, mark it solved |

In this save, `_initStateNameHash` carries both gate-open and
puzzle-solved variants — they're separate hash values for the same
field.

---

## Phase 1 — per-gate identification (mapping)

### Pipeline

1. Extract `0008/gamedata/binary__/client/bin/gimmickinfo.pabgb` via
   the shipped `crimson_paz_extract_file`.
2. Parse with `crate::gimmick_info::parse_gimmick_info_lossy`. Filter
   for rows whose internal name contains `"abyss"` or `"hyperspace"`
   (case-insensitive). **1.07 sample: 2,313 abyss-related rows.**
3. Parse the live save with `Save::parse` + `Body::parse` +
   `Body::decode_blocks`. Walk all `FieldGimmickSaveData` blocks
   (top-level + nested in `ObjectList`).
4. Filter for blocks whose `_gimmickInfoKey` is in the abyss set.
   **1.07 sample: 356 abyss-gate blocks** (out of 4,264 total
   `FieldGimmickSaveData`).
5. For each, pull `(_gimmickInfoKey, internal_name, _ownerLevelName,
   _initStateNameHash, _isLockState, _fieldGimmickSaveDataKey)`.

The probe lives at `_probe_abyss_gate_mapping` in
[`src/c_abi/character_info.rs`](../src/c_abi/character_info.rs)
(`#[ignore]`'d). It pins the three known state-hash constants and
writes a full per-gate JSON to `out/abyss_gate_probe/mapping.json`
(gitignored).

### Observed gimmick taxonomy

The 356 abyss-gate blocks split into a small set of gimmick types,
each playing a specific role:

| `_gimmickInfoKey` | Internal name | Typical role |
|---|---|---|
| `0x000f60c7` | `gimmick_abyssone_bridge_gate_01` | Bridge body |
| `0x000f60c8` | `gimmick_abyssone_bridge_gate_01_parts01` | Bridge supports |
| `0x000f4ed1` | `abyss_standstone_01` | Marker stone (decoration) |
| `0x000f5986` | `gimmick_abyss_standstone_01_part_02` | Marker stone parts |
| `0x000f63bc` | `abyssruins_useartifact_04` | Hyperspace activation artifact |
| `0x000f63bd` | `abyssruins_useartifact_04_part` | Artifact parts |
| (+ many more) | various | Lighting, transmitters, walls, circuits, pipes |

**Sample mapping (12 of 356 — see JSON for full)**:

```
gimmickKey   internal_name                              owner_level                  stateHash    slotKey
0x000f60c7   gimmick_abyssone_bridge_gate_01            AbyssBridge_0001_Phase00_00  0x866c7489    62770
0x000f60c8   gimmick_abyssone_bridge_gate_01_parts01    AbyssBridge_0001_Phase00_00  0x866c7489    62769
0x000f4ed1   abyss_standstone_01                        AbyssBridge_0002_Phase00_00  0x150b14d0    61366
0x000f5986   gimmick_abyss_standstone_01_part_02        AbyssBridge_0002_Phase00_00  0x150b14d0    61364
0x000f60c7   gimmick_abyssone_bridge_gate_01            AbyssBridge_0002_Phase00_00  0xe300acfe    60805 ← crossed
0x000f60c8   gimmick_abyssone_bridge_gate_01_parts01    AbyssBridge_0002_Phase00_00  0xe300acfe    60804 ← crossed
0x000f4ed1   abyss_standstone_01                        AbyssBridge_0016_Phase00_00  0x150b14d0   777098
0x000f5986   gimmick_abyss_standstone_01_part_02        AbyssBridge_0016_Phase00_00  0x150b14d0   777096
0x000f60c7   gimmick_abyssone_bridge_gate_01            AbyssBridge_0016_Phase00_00  0xe300acfe   777071 ← crossed
0x000f60c8   gimmick_abyssone_bridge_gate_01_parts01    AbyssBridge_0016_Phase00_00  0xe300acfe   777070 ← crossed
0x000f60c7   gimmick_abyssone_bridge_gate_01            AbyssBridge_0023_Phase00_00  0x866c7489   310946
0x000f60c8   gimmick_abyssone_bridge_gate_01_parts01    AbyssBridge_0023_Phase00_00  0x866c7489   310945
```

`_ownerLevelName` is the human-readable level key (e.g. `AbyssBridge_0001_Phase00_00`).
Use that to group gates by world location in the editor UI.

`_isLockState` is **absent** on every observed abyss-gate block — the
state machine encodes lock status entirely through `_initStateNameHash`,
not the standalone bool. The editor should NOT try to flip
`_isLockState`; flip the hash instead.

---

## Phase 2 — state-name hash constants

### What the save has

Across the 356 abyss-gate blocks, only **three** distinct
`_initStateNameHash` values appear:

| Hash | Count | Empirical label | What it means |
|---|---|---|---|
| `0x866c7489` | 88 | `default_untouched` | Bridge gimmick in its initial state — the player hasn't interacted with it yet. Crossing the bridge transitions this hash to `activated_crossed`. |
| `0xe300acfe` | 16 | `activated_crossed` | Bridge gimmick the player has activated (e.g. crossed). Persistent — the bridge stays in this state for the rest of the save. |
| `0x150b14d0` | 252 | `idle_decoration` | Default state for standstones, artifacts, and other ambient abyss pieces. Doesn't change with player action. |

The label assignments come from cross-checking the user's save:
bridges in `AbyssBridge_0002` / `_0016` / `_0041` / `_0109` / `_0131` /
`_0140` (visibly traversed in-game) carry `0xe300acfe`; bridges in
`AbyssBridge_0001` / `_0023` (not yet traversed) carry `0x866c7489`.

### What the editor should do with them

For CrimsonAtomtic's per-gate UI:

```text
toggle_gate(gimmick_block, want_unlocked):
    if want_unlocked:
        gimmick_block._initStateNameHash = 0xe300acfe   // activated
    else:
        gimmick_block._initStateNameHash = 0x866c7489   // default
```

The write uses `crimson_save_set_scalar_field_path` from the existing
C ABI. No new bridge needed.

For "mark discovered" — keep using the knowledge-injection approach
from the reference editor (the 398-entry pack works as-is). Per-gate:
inject just that gate's `KnowledgeKey` instead of bulk.

### Why the hash → name decode isn't pinned

PA's `HashCode32` algorithm doesn't match any standard hash the probe
tried (Jenkins hashlittle / hashlittle2 with seeds `0x00000000`,
`0xdeadbeef`, `0xdeba1dcd`, `0xfeedbabe`, `0x12345678`, `0x9e3779b9`,
`0xbadc0ffe`, `0xcafef00d`, `0xc0debabe`, `0xffffffff`; plus FNV-1a,
SDBM, DJB2, CRC32-IEEE) against any of:

- 10,521 generated candidate strings spanning common state-machine
  vocabulary (Init/Idle/Active/Locked/Unlocked/Open/Closed/Solved/…)
  with prefixes (`Gate_`, `AbyssGate_`, `State_`, `Puzzle_`, …) and
  suffixes (`_State`, `_Status`, `_Node`, …).
- 424 distinct ASCII strings harvested from the matching
  `.binarygimmick` files (`gimmick_abyssone_bridge_gate_01.binarygimmick`,
  `abyss_standstone_01.binarygimmick`) — including the
  state-machine vocabulary `GimmickOnEnterState` /
  `GimmickOnExitState` / `ClearEnterState` / `DeactiveExitState` /
  `Active_Ing` / `InitialBranchState` / `Root` / `Wait`.

The empirical labels are still enough for the editor today. Cracking
the decode would be nice-to-have (let the editor show "Locked" /
"Activated" / "Idle" strings instead of hex values) but isn't a
blocker.

### Breadcrumb for the resume

Each state hash appears inside its `.binarygimmick` file as a
structured record. For `gimmick_abyssone_bridge_gate_01.binarygimmick`
at offset `0x16b`:

```
[0x283bf40d][0x7c9c9e2f][0xfd45d6ee][0x5bdda844] ← four "handler" hashes
[0x866c7489 00 00 00 00]                          ← state node hash + 4 zero bytes
[0x866c7489 00 00 00 00]                          ← state node hash repeated
```

The four preceding hashes (`0x283bf40d`, `0x7c9c9e2f`, `0xfd45d6ee`,
`0x5bdda844`) re-appear in front of OTHER state-node records too — they
look like event-handler-name hashes (Enter / Exit / Frame / …) shared
across every state-machine node. Cracking any one of those four would
back-fit the algorithm. Likely path: IDA-decompile PA's
`GimmickStateMachine` loader and read the hash routine directly.

---

## Path B implementation outline for CrimsonAtomtic

The data above is enough for the C# editor to ship the feature.
Outline:

### 1. Hardcode the state constants

```csharp
static class AbyssGateStateHash
{
    public const uint DefaultUntouched   = 0x866c7489;
    public const uint ActivatedCrossed   = 0xe300acfe;
    public const uint IdleDecoration     = 0x150b14d0;
}
```

### 2. Build the gimmick-name allowlist

At app startup (once per save load), extract
`gimmickinfo.pabgb` via the shipped `crimson_paz_extract_file`, then
filter with the shipped `crimson_gimmickinfo_*` bridge for entries
matching `"abyss"` / `"hyperspace"`. Cache the resulting
`(gimmick_info_key → internal_name)` map.

### 3. Walk save for abyss-gate blocks

Use the existing `crimson_save_get_block_class_name` /
`crimson_save_get_block_json` surface to iterate
`FieldGimmickSaveData` blocks. For each, check whether
`_gimmickInfoKey` is in the abyss allowlist.

### 4. Per-gate UI row

Group by `_ownerLevelName` (e.g. `AbyssBridge_0001_Phase00_00`) for
display. Per-row controls:

- **Lock state checkbox** → writes
  `_initStateNameHash` via `crimson_save_set_scalar_field_path`.
  Bool-to-hash: `false` → `DefaultUntouched`, `true` →
  `ActivatedCrossed`.
- **Mark discovered checkbox** → injects the gate's
  `KnowledgeKey` into `KnowledgeSaveData._list` (the reference editor's
  `inject_knowledge_fast` flow, but for a single key — the C ABI
  already exposes the list-insert primitives via
  `crimson_save_list_insert_element`).
- **(Optional) Puzzle state** — out of scope for v1. The hash field
  carries puzzle state too but we haven't catalogued which gates have
  puzzle variants. Re-probe with a save that has a partly-solved
  puzzle to identify those gates.

### 5. Batch "unlock all" — performance

The bottleneck is **decompress + recompress** of the save body (one
LZ4 pass each way + one ChaCha20 pass each way). All in-memory edits
are constant-time. So:

- Single-gate edit: one save round-trip per click. Probably fine for
  a few clicks.
- Batch ("unlock all 397"): coalesce edits into a single save
  round-trip. CrimsonAtomtic already has the C ABI for
  `crimson_save_set_scalar_field_path` per-edit; the save handle
  stays open across many edits, so one final
  `crimson_save_write_to_file` materialises everything in one pass.

The 1.07 sample save (slot0) is 1.5 MB compressed / ~5 MB
decompressed. End-to-end including disk I/O the round-trip is well
under a second on a modern desktop.

### 6. Map gates to their `KnowledgeKey`s

The reference pack `No Map Reveal Abyss Gate Unlock Only.json` (in
`D:\Github\CRIMSON-DESERT-SAVE-EDITOR-AND-GAME-MODS\CrimsonSaveEditor\knowledge_packs\`)
has 398 entries: 1 icon header (`Knowledge_LevelGimmickIcon_AbyssGate`,
key 1001030), 1 icon header for the hyperspace ruins (key 1001031),
and the rest are `AbyssGate_NNNN` / `Knowledge_AbyssRuins_HyperSpace_NNNN`
entries. CrimsonAtomtic should treat that pack as data-driven (load
it at startup) rather than hardcoding the 398 keys.

A future improvement: build a structured `(gimmick_info_key,
knowledge_key, owner_level_name, display_name)` mapping by joining
the probe's output against the pack. This makes the per-gate UI
show "AbyssGate_0001 — Dimension's End" (display name from the pack)
instead of the gimmick internal name. The pack already has
`display` strings.

---

## Open RE

1. **Crack PA's `HashCode32` algorithm**. IDA path: find the function
   that loads `.binarygimmick` files into a `GimmickStateMachine` and
   identify the state-name hash routine. With one known
   `(state_name, hash)` pair we can back-fit and decode the rest.
2. **Puzzle-state hash values**. The user's save doesn't surface
   distinct puzzle-state hashes (the bridge state machine is
   simpler — just untouched/crossed). A save with a partly-solved
   abyss-ruin puzzle would let us catalogue more hash values for the
   "puzzle solved" UX.
3. **Cross-version stability**. The 3 state constants need to be
   re-verified after each PA patch. The
   `_probe_abyss_gate_mapping` test asserts their presence — running
   it on a fresh save after a patch is the patch-verification step
   for this feature.
