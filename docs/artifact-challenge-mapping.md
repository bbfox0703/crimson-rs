# Artifact ↔ Challenge mapping

Reference for the editor's catalog UI — "which artifact starts this
challenge?" and "which challenge does this artifact start?". Both
directions are derivable from gamedata alone (no save state, no
sample data needed).

> Status (2026-05-16): mapping verified across the live 1.07 install
> via [`_probe_artifact_challenge_mapping`](../src/c_abi/iteminfo.rs).
> All facts below are computed at runtime from `iteminfo.pabgb` +
> `missioninfo.pabgb` + PALOC — no hardcoded tables, no per-patch
> maintenance.

---

## The gamedata link

Each `ItemInfo` row carries a `look_detail_mission_info: MissionKey`
field (a u32). When non-zero, picking up the item triggers that
catalog challenge — which is what the user means by
"拾取對應的 Sealed Abyss Artifact 才會開始對應的 challenge".

Verified statistics across 1.07 (`6,253` items × `4,075` missions):

| Stat | Value |
|---|---:|
| Items with `look_detail_mission_info != 0` | **141** |
| Distinct missions pointed at | **141** (1:1, no mission has >1 artifact) |
| 100% of pointed-at missions match prefix | `Challenge_SealedArtifact_*` |
| Fraction of all missions started by an artifact | 3.5% (141 / 4,075) |

The other 96.5% of missions start by other triggers — dialogue,
kill counters, geographic discovery, etc. They have no item entry
pointing at them, and the reverse-lookup ABI below correctly
returns `NOT_FOUND` for those.

### Naming pattern → mission classification (string-based)

Since the mission *internal name* fully encodes its type, the C#
editor's classification can be a pure prefix check on the result of
`crimson_missioninfo_lookup_string_key`:

| Internal name pattern | Meaning | Approx count (1.07) |
|---|---|---:|
| `Challenge_SealedArtifact_*` | Catalog challenge that REQUIRES an artifact pickup | 141 |
| `Challenge_*` (but NOT `Challenge_SealedArtifact_*`) | Catalog challenge that starts WITHOUT an artifact | varies — RE via probe |
| `Mission_*` / others | Regular story / side quest | majority of the 4,075 |

`Challenge_SealedArtifact_*` sub-tracks observed:

- `Mastery_OneHandSword_I/II/III`
- `Mastery_Shield_I..VI`
- `Mastery_Spear_I..III`
- `Mastery_Bow_I..III`
- `Mastery_Battle_I..XIII` (or so)
- `Hunting_*` (various roman-numeral tiers)
- `ChallengeAndChange_*`

---

## C ABI surface

| Function | Direction | Returns |
|---|---|---|
| [`crimson_iteminfo_lookup_look_detail_mission_info`](../src/c_abi/iteminfo.rs) | item → mission | `OK` + `*out_mission_key` if the item triggers a challenge; `NOT_FOUND` otherwise (regular items leave the field at 0) |
| [`crimson_iteminfo_lookup_artifact_for_mission`](../src/c_abi/iteminfo.rs) | mission → item | `OK` + `*out_item_key` if some artifact triggers this mission; `NOT_FOUND` if the mission starts by other means |
| [`crimson_missioninfo_lookup_string_key`](../src/c_abi/mission_info.rs) | mission → internal name | Used to classify a challenge as artifact-required vs not, via the `Challenge_SealedArtifact_*` prefix check |

### Editor pipeline (catalog UI)

```csharp
// Walking the user's catalog. For each focused mission:
string missionName = MissionLookupStringKey(missionHandle, missionKey);
bool isChallenge = missionName.StartsWith("Challenge_");
bool needsArtifact = missionName.StartsWith("Challenge_SealedArtifact_");

if (needsArtifact) {
    int rc = IteminfoLookupArtifactForMission(itemHandle, missionKey, out uint artifactKey);
    if (rc == OK) {
        // Show "Required: Sealed Abyss Artifact (item N)" badge.
        // Also walk the player's inventory to check ownership.
    }
} else if (isChallenge) {
    // "Sealed Artifact not required — starts by gameplay trigger"
}
```

### Inventory-aware progress (existing pattern)

The forward direction has long been used for the
"Mark Challenge Complete" gate — walk every item in the player's
inventory, ask `lookup_look_detail_mission_info`, and the missions
whose artifacts the player currently owns are the eligible
challenge-completion targets.

---

## Cross-references

- [`src/c_abi/iteminfo.rs`](../src/c_abi/iteminfo.rs) —
  `crimson_iteminfo_lookup_look_detail_mission_info` (forward) +
  `crimson_iteminfo_lookup_artifact_for_mission` (reverse) +
  `c_abi_iteminfo_artifact_challenge_roundtrip_live` test (pins 8
  artifact↔mission tuples and asserts the full-table 1:1 invariant).
- [`src/c_abi/iteminfo.rs`](../src/c_abi/iteminfo.rs) —
  `_probe_artifact_challenge_mapping` ignored probe (re-run after
  a patch to verify the 1:1 invariant still holds; dumps the prefix
  histogram + per-mission artifact lists).
- [`src/c_abi/mission_info.rs`](../src/c_abi/mission_info.rs) —
  `crimson_missioninfo_lookup_string_key` for the
  `Challenge_SealedArtifact_*` prefix check.
