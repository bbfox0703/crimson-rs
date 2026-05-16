# Save Editor key resolvers — plan

> **🎯 New session pickup — start here.**
>
> **Where we are (updated 2026-05-15)**: 10 of 10 originally-scoped key
> resolvers shipped. All save-side key types that the editor's
> `QuestSaveData` / `SubLevelSaveData` / `AlertHistorySaveData` /
> `FieldGimmickSaveData` / `FieldNPCSaveData` blocks surface resolve
> through one-shot FFI calls. CharacterKey is the latest addition
> (2026-05-15) — landed at the same 22% display-name coverage §6
> predicted, plus a full-coverage `lookup_string_key` fallback for the
> 78% of values that have a `characterinfo.pabgb` row but no PALOC
> display string, plus a high-level
> [`crimson_characterinfo_resolve_portrait`](../src/c_abi/character_info.rs)
> that chains display lookup with a fuzzy match against
> [`crimson_paz_list_npc_portraits`](../src/c_abi/paz.rs) to land NPC
> head-shots. The Jenkins hash hop transform stays verified and pinned.
> See "Verified hash transform" below for the architecture and
> [`save-editor-keys-reference.md`](./save-editor-keys-reference.md) for
> the 1.06 ground-truth comparison set.
>
> **Shipped so far**: `mission_info`, `quest_info`, `stage_info`,
> `quest_gauge_info`, `knowledge_info`, `sub_level_info`,
> `gimmick_info`, `character_info` bridges + `checksum` extern "C"
> wrapper + the legacy `skill_info` bridge + the
> `crimson_paz_list_npc_portraits` PAZ-layer NPC-portrait enumerator.
> **Three follow-on dye gamedata bridges shipped 2026-05-16**:
> `dye_color_group_info`, `part_prefab_dye_texture_pallete_info`,
> `part_prefab_dye_slot_info` — replace the PyQt5 reference editor's
> hand-maintained `dye_slot_counts.json` with gamedata-driven data.
> See [`dye-editor-scope.md`](./dye-editor-scope.md).
> 188 tests with `c_abi`, 69 without (+16 ignored diagnostic probes).
> Clippy clean both modes.
>
> **Remaining (optional follow-ons only)**:
> - **`CharacterAppearanceIndexKey`** — investigated 2026-05-15.
>   `characterappearanceindexinfo.pabgb` + `.pabgh` located in
>   `0008/gamedata/binary__/client/bin/`, schema pinned, save→pabgh
>   transform verified. **Bridge deferred** — only 9/122 distinct save
>   values map to the pabgh table, and entry bodies are 21-byte binary
>   blobs with no string fields. See §9 for the full picture and the
>   two open RE questions (where the 87% miss path resolves, and the
>   21-byte body schema). Resume from `_probe_character_appearance_index`
>   in `src/c_abi/character_info.rs`.
> - Knowledge group breadcrumb (would need `knowledge_group_info`
>   sibling bridge + row-body parse).
> - Quest chapter rollup ("Prologue: Dead of Night" et al. — never
>   located).
> - lo32=0x490 mission-variant meaning (Q4, partially overshadowed
>   by the knowledge work).
> - Broader PALOC namespace coverage for CharacterKey (the 78%
>   sample-bias miss path — different save samples touching named
>   field NPCs would pin more ground-truth).
>
> **2026-05-15 verification pass**: the live 1.07 `slot0/save.save` was
> parsed end-to-end through `Save::parse + Body::decode_blocks`,
> confirming the §6 verdict on both `_characterKey` and `_gimmickInfoKey`
> empirically. Same pass surfaced `CharacterAppearanceIndexKey` as the
> last unbridged save-side type; investigation findings are in §9.
> Probes live at `_probe_live_save_field_blocks` and
> `_probe_character_appearance_index` in `src/c_abi/character_info.rs`
> (`#[ignore]`'d, diagnostic only).
>
> **First concrete next action**: depends on user direction. The
> `CharacterAppearanceIndexKey` work has the most signal-per-effort
> upside (clear scope, well-defined gaps) but the body-schema RE step
> needs either an IDA pass or a save-diff dataset to unlock.
>
> Extracted `.pabgb` baselines for this work live in
> `out/baselines/1.06/` (gitignored). Re-extract with
> `crimson_rs.extract_file(GAME_DIR, "0008", "gamedata/binary__/client/bin", "<file>.pabgb")`
> if missing.

---

The Save Editor surfaces integer keys that decode against game-data tables
and PALOC. This doc records the current state and the RE roadmap for each
key category — what's shipped, what's blocked on what, and the recommended
order to tackle the rest.

Companion to [`crimsonforge-feature-gaps.md`](./crimsonforge-feature-gaps.md);
that doc surveyed CrimsonForge in general, this one targets the specific keys
the Save Editor consumes.

Cross-referenced 2026-05-14 against the editor's own
`CrimsonAtomtic/docs/status.md` (lines 821–895 — forensic PALOC scan) and
the live `QuestSaveData` block structure from the editor's UI. Also
incorporates a handoff bundle from the editor side at
`D:\Github\CrimsonAtomtic\out\analyze\handoff\` containing 63,522
`(type, value)` keycases from one live 1.06 save.

**Status of the unsolved hash transform**: the u32-key → u64 PALOC-key
transform mentioned in the editor's status.md is **RESOLVED** as of this
session. See "Verified hash transform" below. The previously-hypothesised
"PALOC reverse-index walking 0xC1 `{staticInfo:Mission:KEY}` tokens" is
no longer needed — quest titles resolve through a much cleaner one-hop
chain.

---

## Status snapshot

| Key | Unresolved count* | Source | Rust parser | C ABI bridge | Pattern |
| --- | ---:| --- | --- | --- | --- |
| **SkillKey** | 0 (absent in this save) | `skill.pabgb` + `.pabgh` | ✅ | ✅ `src/c_abi/skill_info.rs` | A — gamedata table |
| **KnowledgeKey** | 392 | `knowledgeinfo.pabgb` + PALOC u64 | ✅ | ✅ `src/c_abi/knowledge_info.rs` | **A + hash hop (shipped)** |
| **MissionKey** | 1,299 | `missioninfo.pabgb` + PALOC u64 | ✅ | ✅ `src/c_abi/mission_info.rs` | **A + hash hop (shipped)** |
| **QuestKey** | 6 | `questinfo.pabgb` + PALOC u64 | ✅ | ✅ `src/c_abi/quest_info.rs` | **A + hash hop (shipped)** |
| **QuestGaugeKey** | 0 (rare) | `questgaugeinfo.pabgb` | ✅ | ✅ `src/c_abi/quest_gauge_info.rs` | A — gamedata table (**no hash hop**; gauges aren't in PALOC) |
| **StageKey** | 36,613 | `stageinfo.pabgb` + PALOC u64 | ✅ | ✅ `src/c_abi/stage_info.rs` | **A + hash hop (shipped)** |
| **CharacterKey** *(via FieldNPCSaveData._characterKey)* | 221 | `characterinfo.pabgb` + PALOC u64 | ✅ | ✅ `src/c_abi/character_info.rs` | A — gamedata table (cat byte strip + lo32=0x30 PALOC, **no hash hop**); display chain 22% sample coverage, internal-name fallback 100%, plus `resolve_portrait` high-level matcher |
| **GimmickInfoKey** *(via FieldGimmickSaveData._gimmickInfoKey)* | 530 unique (6,666 occurrences) | `gimmickinfo.pabgb` + PALOC u64 | ✅ | ✅ `src/c_abi/gimmick_info.rs` | A — gamedata table (**no hash hop**; identity key into PALOC, default lo32=0x200), 99.4% coverage |
| **FieldNPCSaveDataKey** | 103 | save-internal (spawn-slot index) | n/a | n/a | save-internal *(verified — see §6)* |
| **FieldGimmickSaveDataKey** | 4,363 | save-internal (spawn-slot index) | n/a | n/a | save-internal — real bridge is sibling `_gimmickInfoKey` above |
| **CharacterAppearanceIndexKey** *(via FieldNPCSaveData._nudeAppearanceIndexKey + _customizationAppearanceIndexKey)* | 122 distinct (228 samples) | `characterappearanceindexinfo.pabgb` + `.pabgh` (located, but only 7% of save values hit the table) | ✗ (deferred) | ✗ (deferred) | u64 key. PABGH/PABGB pair (`u32 count + (u64 key, u32 offset)`). Save→pabgh transform pinned (byte-3 sign-ext + lo24). **Bridge deferred** — only 9/122 distinct save values map to a pabgh entry, and entry bodies are 21-byte binary blobs with no string field. See §9 for full investigation + the resume plan. |
| **SubLevelKey** | 7 | `sublevelinfo.pabgb` | ✅ | ✅ `src/c_abi/sub_level_info.rs` | A — gamedata table (**no hash hop, no PALOC**; sub-levels not localized) |
| **ItemKey** | 669 | `iteminfo.pabgb` (parser ahead of save) | ✅ | ✅ | A — gamedata table |
| **Hash hop helper** | n/a | `crypto::checksum::calculate_checksum` | ✅ | ✅ `src/c_abi/checksum.rs` | exposed for editor-side chain composition |

\* From `D:\Github\CrimsonAtomtic\out\analyze\handoff\keycases_unresolved.jsonl`,
one live 1.06 save (slot0).

---

## The one resolution pattern (with optional hash hop)

```
SaveKey (u32 in save file)
   └─► <table>.pabgb row → internal name (BString ASCII)
                            │
              ┌─────────────┴─────────────┐
              │                            │
     "simple" prefix join         hash-hop join
     for tables whose names       for tables whose names map
     paloc-resolve directly       to PALOC u64 keys via
     (e.g. SkillKey →             hashlittle2 (Mission/Quest/Stage)
      "SkillName_<name>")
              │                            │
              └─────────────┬──────────────┘
                            │
                  caller composes PALOC key
                            │
                            ▼
                  localized display name
```

The hash hop is the small extra step for Mission / Quest / Stage. It
collapses what previously looked like a separate "Pattern B" (PALOC
reverse-index over `{staticInfo:Mission:KEY}` tokens) and "Pattern C"
(unsolved u32 → u64 transform) into a single mechanical chain.

---

## Verified hash transform — Mission / Quest / Stage titles

**Cracked 2026-05-14** by cross-referencing the editor's handoff
keycases, the user's ground-truth Main Quest titles, and a brute-force
hash-of-every-ASCII-identifier-in-questinfo+missioninfo pass.

### The transform

```
PALOC u64 key = (hashlittle2_c(internal_name) << 32) | lo32_namespace

  hashlittle2_c      = the c output of the Jenkins hashlittle2 variant
                        already in crimson-rs/src/crypto/checksum.rs
                        (custom init constant 0xDEBA1DCD)
  internal_name      = the ASCII identifier from <table>.pabgb row,
                        e.g. "Mission_Intro_Tutorial_I"
  lo32_namespace     = 0x100 (256)  → sub-arc / region / chapter heading
                        0x101 (257)  → individual quest title (line item)
                        0x490 (1168) → another mission-related namespace
                                        (specifics TBD — fewer hits)
```

### Verified mappings (10/10 of the user's first prologue + chapter 1)

| User-facing title (English UI) | Internal name (from `missioninfo.pabgb`) | hi32 = hashlittle2_c |
| --- | --- | ---:|
| Unfamiliar Lands | `Mission_Intro_Tutorial_I` | 1,891,183,967 |
| In Ashes | `Mission_Intro_MainBattle` | 666,048,679 |
| Realm of Uncertainty | `Mission_Intro_Abyss_Tutorial` | 1,312,131,709 |
| New Journey | `Mission_Intro_After_Horse` | 1,617,235,650 |
| Where Rumors Gather | `Mission_MeetAlustain_Alustain_Strength` | 1,065,396,823 |
| Mysterious Man | `Mission_MeetAlustain_Alustain_Wisdom` | 1,751,060,790 |
| True Wisdom in Kindness | `Mission_MeetAlustain_Alchemist_Rescue` | 4,287,214,422 |
| Actions Speak Louder than Words | `Mission_MeetAlustain_Alchemist_Cleaning` | 3,882,977,810 |
| Heart Beyond Borders | `Mission_MeetAlustain_Alustain_CatchCat` | 1,792,046,031 |
| Where the Wind Guides You | `Mission_IronStronghold_Block_ReturnToSister` | 3,594,586,120 |

Last row matches the editor's existing status.md sample
(`PALOC key 15438629828055531777 → "Where the Wind Guides You"`,
upper 32 = 3,594,586,120 confirmed).

### Cross-validation

When all 7,972 distinct ASCII identifiers across `questinfo.pabgb +
missioninfo.pabgb` were hashed and the result tried against every
numeric PALOC u64 key, **701 clean hits** came back. The same probe on
`stageinfo.pabgb` (57,094 identifiers) added **6,911 more hits**.
Combined breakdown:

- `lo32 = 256` (0x100): 673 hits — sub-arc / region / chapter headings
  ("Mazes", "Roothold", "Pailune Faction", "Emperor of the Bonepit")
- `lo32 = 257` (0x101): 6,520 hits — individual quest / stage titles
  ("Unfamiliar Lands", "Beighen Militiaman at Stellen Manor Patrol",
   "Mise-en-scene Before Intro Combat")
- `lo32 = 258` (0x102): 404 hits — stage / shop descriptions
  ("A grocer's shop run by Gromin, who has ill ties with…")
- `lo32 = 1168` (0x490): mission text variant; not deeply probed yet

Zero false positives observed. The transform is byte-deterministic.

### End-to-end verified chain (save MissionKey → display title)

Live anchor scan against `missioninfo.pabgb` (2026-05-14) traced **all 7
of the user's prologue + chapter-1 Main Quest titles** back to a real
save-side MissionKey value, and confirmed each one is present in the
editor's `keycases_full.jsonl` handoff. Sample slice from the run
(`state=5` means completed in this save; `completedTime` is the
monotonic in-game clock):

| save MissionKey | Internal name | completedTime | Display title |
| ---:| --- | ---:| --- |
| 1000157 | Mission_Intro_Tutorial_I | 97,635 | Unfamiliar Lands |
| 1000160 | Mission_Intro_MainBattle | 188,301 | In Ashes |
| 1000620 | Mission_Intro_Abyss_Tutorial | 276,192 | Realm of Uncertainty |
| 1000164 | Mission_Intro_After_Horse | 307,325 | New Journey |
| 1000052 | Mission_MeetAlustain_Alustain_Strength | 396,512 | Where Rumors Gather |
| 1000053 | Mission_MeetAlustain_Alustain_Wisdom | 407,522 | Mysterious Man |
| 1000083 | Mission_IronStronghold_Block_ReturnToSister | 7,148,855 | Where the Wind Guides You |

The completedTime monotonicity matches the in-game story order
perfectly — last row is the Hernand-chapter quest, played ~70× later
than the prologue tutorial. **The architecture is concretely
verified.** Bridge implementation can rely on these rows as fixtures.

### Anchor-scan finding — `missioninfo.pabgb` row schema

The seven matches above also pinned the row header layout:

```text
[u32 key][u32 name_len][name_len bytes ASCII][...rest of row]
```

This matches `iteminfo.pabgb` exactly — same header shape PA uses
across the gamedata `.pabgb` family. Distance between consecutive
anchors gives the row body size (varies per row: 334–390 bytes
observed in the prologue cluster), consistent with variable-length
fields (PALOC string refs, conditional flags). Full schema RE for
a byte-roundtrip parser is still pending, but the **bridge only
needs the (key, name) pair**, which the anchor scan resolves
trivially — exactly the same shortcut `iteminfo` and `skill_info`
take internally.

### PA's terminology vs UI

The internal naming flips what the English UI calls a "quest":

| PA internal name | English UI label | Notes |
| --- | --- | --- |
| `Mission_*` (in `missioninfo.pabgb`) | "Quest" (individual line item) | The user's Main Quest list ("Unfamiliar Lands", …) is the `Mission_*` namespace |
| `Quest_Node_*` / `Quest_*` (in `questinfo.pabgb`) | "Arc" / "Region" / "Chapter" heading | The "## Ambush", "## Trials of Kindness" sub-arc headings |
| `Challenge_*` | Challenge category | The Sealed Artifact / Crime / Trust / Mastery groupings |
| `Schedule_*` | Spawn schedule | NPC/monster spawn tables |

### Hi32 magnitude is a discriminant

Across the 701 verified hits the hi32 distribution shows two clusters:
- Small hi32 (under ~2^24): side-content, dialog stages, objective text
- Large hi32 (above 2^27): main-quest-shaped titles, region/arc names

There's no semantic information in the magnitude — it's just an artefact
of the Jenkins hash distribution over the longer/shorter names that get
used for each category. Not load-bearing for the bridge logic; useful
only as a sanity heuristic when grepping PALOC manually.

---

## 1. SkillKey — DONE (Pattern A)

Shipped in `src/c_abi/skill_info.rs`. Surface:

```c
crimson_skillinfo_load_from_file(pabgh_path, pabgb_path, &handle);
crimson_skillinfo_load_from_bytes(pabgh_bytes, pabgb_bytes, &handle);
crimson_skillinfo_entry_count(handle, &count);
crimson_skillinfo_lookup_string_key(handle, skill_key, buf, len, &required);
crimson_skillinfo_get_entry(handle, idx, &key, buf, len, &required);
crimson_skillinfo_free(handle);
```

Live install on 1.06 reports ~280 skills.

---

## 2. KnowledgeKey — SHIPPED (Pattern A + hash hop)

**Bridge SHIPPED** in `src/c_abi/knowledge_info.rs`.

**File**: `0008/.../gamedata/knowledgeinfo.pabgb` (3 MB, ~5,500 rows).

### Probe finding — hash transform applies, contrary to earlier hypothesis

The editor's status.md and earlier drafts of this doc both
hypothesised that KnowledgeKey wouldn't resolve through PALOC under
the standard hash hop. **The probe disproved this.** Hashing every
`Knowledge_*` name in `knowledgeinfo.pabgb` against the full
124,800-entry English PALOC produced **29,330 hits** distributed
across multiple namespaces:

- `lo32 = 0x490` (1168): **5,483 hits** — knowledge entry title
  (every row resolves here)
- `lo32 = 0x491` (1169): 5,483 hits — short description (every row)
- `lo32 = 0x49F` (1183): 5,483 hits — description alternate
- `lo32 = 0x49D` (1181) / `0x49E` (1182): ~3,900 hits each —
  secondary "lore" / "discovery" text variants
- `lo32 = 0x890 / 0x891 / 0x89F` (~1,400 each), `0xC90 / 0xC91 /
  0xC9F` (~100 each), etc. — paginated text variants for certain
  sub-categories

Sample mappings (used as live test fixtures):

| KnowledgeKey | internal_name | lo32 | display title |
| ---:| --- | ---:| --- |
| 1002588 | `Knowledge_Node_Dem_Ruins_0007` | 0x490 | "Demenissian Ruins" |
| 1002588 | (same) | 0x491 | "Ruins of the continent of Pywel." |
| 1002294 | `Knowledge_Node_Dem_HiddenCave` | 0x490 | "Hidden Cave" |
| 1002763 | `Knowledge_AbyssRuins_Dem_0020` | 0x490 | "Abyss Nexus" |
| 1004037 | `Knowledge_Demian_Plate_Boots_V` | 0x490 | "Executioner of Darkness Plate Boots" |

### What the bridge DOESN'T cover (intentionally)

The user's "web data" reference showed a 4-level category hierarchy:

```
Creatures > Terrestrial Creatures > Amphibians > Burrowing Salamander
People > Crimson Desert > Equipment Vendors > Brookner
Mounts > Loyal Companions > Horses > Herspia
```

That hierarchy lives in **`knowledgegroupinfo.pabgb`** (a separate
99 KB file, extracted to `out/baselines/1.06/` but not parsed by
this bridge) as integer category IDs at `lo32 = 0x92 / 0x93`:

| Category | hi32 (= group ID) | lo32 |
| --- | ---:| --- |
| Creatures | 4 | 0x92 |
| People | 2 | 0x92 |
| Terrestrial Creatures | 104 | 0x92 |
| Amphibians | 1045 | 0x92 |
| Bosses | 11 | 0x92 (also 0x93) |
| Mounts | 15 | 0x92 |
| Loyal Companions | 151 | 0x92 |
| Horses | 1510 | 0x92 |

Note `hi32` here is a **literal integer group ID**, not a Jenkins
hash — confirms the editor's "small KnowledgeKey values live at
PALOC 0x93" finding from status.md §834 was actually a **group-ID
namespace**, not a small-leaf-KnowledgeKey namespace. The two are
distinct: leaf knowledge titles use the hash hop at `lo32=0x490`,
group rollups use the direct integer at `lo32=0x92/0x93`.

Mapping a leaf KnowledgeKey to its parent group requires parsing
the knowledgeinfo row body (the body bytes contain a group_id
reference — confirmed by inspection during the probe). That's
**out of scope for this bridge**. A future `knowledge_group_info`
sibling bridge plus a `lookup_category_breadcrumb` chain could
surface the full "Creatures > Terrestrial > Amphibians > X"
trail. The shipped bridge stops at the leaf title.

### Shipped surface

```c
crimson_knowledgeinfo_load_from_{file,bytes}(...)
crimson_knowledgeinfo_entry_count(handle, &count)
crimson_knowledgeinfo_lookup_string_key(handle, key, buf, len, &req)
crimson_knowledgeinfo_lookup_display_name(handle, paloc_handle, key,
                                          lo32_namespace,
                                          buf, len, &req)
crimson_knowledgeinfo_get_entry(handle, idx, &key, buf, len, &req)
crimson_knowledgeinfo_free(handle)
```

5 tests (4 c_abi bridge + 1 parser module) including a live
full-chain assertion on 5 (KnowledgeKey, internal_name, lo32,
display_title) tuples, with the dual-namespace
`Knowledge_Node_Dem_Ruins_0007` case to pin `lo32` as load-bearing
(0x490 → "Demenissian Ruins", 0x491 → "Ruins of the continent of
Pywel.").

### Earlier corrections (preserved for the record)

- `CrimsonAtomtic/docs/status.md` item #4 grouped SkillKey +
  KnowledgeKey under one "skill_info bridge" line. They live in
  different files; KnowledgeKey is a separate bridge.
- Earlier drafts of this doc warned against shipping CrimsonForge's
  regex scanner for `Knowledge_<name>\x00`. The shipped bridge
  uses the same anchor-scan approach as mission/quest/stage instead
  — header-based and deterministic.

---

## 3. MissionKey + QuestKey titles — Pattern A + hash hop

**Both bridges SHIPPED**: `src/c_abi/mission_info.rs` for individual
quest line items (English UI's "quest titles"), `src/c_abi/quest_info.rs`
for arc / region / chapter headings.

The architecture I drafted in earlier versions of this doc proposed
parsing PALOC `0xC1` entries and walking embedded
`{staticInfo:Mission:KEY}` tokens. That's no longer needed. With the
hash transform cracked, the resolution chain is just:

```
save MissionKey (u32) 
   ├─► missioninfo.pabgb row → internal_name (e.g. "Mission_Intro_Tutorial_I")
   ├─► hashlittle2_c(internal_name) = hi32
   ├─► (hi32 << 32) | 0x101 = u64 PALOC key
   └─► PALOC.lookup(u64_as_decimal_string) → "Unfamiliar Lands"
```

Same chain for QuestKey, with the row from `questinfo.pabgb` and (often)
`lo32 = 0x100` for arc headings rather than `0x101` for individual
quests. The two `.pabgb` files seem to share the naming convention
modulo the prefix.

### MissionKey bridge — shipped surface

```c
crimson_missioninfo_load_from_file(path, &handle);
crimson_missioninfo_load_from_bytes(data, len, &handle);
crimson_missioninfo_entry_count(handle, &count);
crimson_missioninfo_lookup_string_key(handle, key, buf, len, &req);
    // → "Mission_Intro_Tutorial_I" (debug / fallback)
crimson_missioninfo_lookup_display_name(
    handle, paloc_handle, key, lo32_namespace, buf, len, &req);
    // → "Unfamiliar Lands" (production, one FFI call)
crimson_missioninfo_get_entry(handle, idx, &key, buf, len, &req);
crimson_missioninfo_free(handle);
```

Loaded once at startup against the PAZ-extracted
`missioninfo.pabgb`. The display_name function chains through the
existing PALOC handle so the editor doesn't have to compose
`name → hash → u64 → decimal-format → PALOC.lookup` in C#.

Backed by a lossy anchor scanner in `src/mission_info/mod.rs` —
schema RE for byte-roundtrip is **not** done (and not needed) because
the bridge only consumes `(key, name)`. The scanner validates each
candidate header rigorously (key < 2^24, name_len ∈ [2,128],
identifier-byte name); zero false positives observed on 1.06.

### ABI shape — two options

**Option A (table bridges + paloc u64 helper, minimal new surface).**
Add the standard 6-function bridge per `.pabgb` file:

```c
crimson_missioninfo_load_from_{file,bytes}(...)
crimson_missioninfo_entry_count(handle, &count)
crimson_missioninfo_lookup_string_key(handle, MissionKey, buf, len, &req)
   //   returns "Mission_Intro_Tutorial_I"
crimson_missioninfo_get_entry(handle, idx, &key, buf, len, &req)
crimson_missioninfo_free(handle)
// (identical shape for questinfo)
```

Plus expose the hash to the C ABI (currently only in PyO3 binding):

```c
uint32_t crimson_calculate_checksum(const uint8_t* data, size_t len);
   // already exists in Rust as crypto::checksum::calculate_checksum;
   // just needs an extern "C" wrapper.
```

C# composes: `LookupStringKey → calculate_checksum → format decimal →
PALOC.Lookup`. Three FFI calls per title resolution (cheap; the bridges
are zero-allocation hash hits).

**Option B (one-shot lookup, fewer FFI calls).**
Bundle the chain into the missioninfo bridge:

```c
crimson_missioninfo_lookup_display_name(
    handle,
    paloc_handle,
    MissionKey,
    uint32_t lo32_namespace,   // caller decides 0x100 vs 0x101
    uint8_t* buf,
    size_t buf_len,
    size_t* required) → i32;
```

One FFI call per title resolution. C# becomes:

```csharp
var name = _missionInfo.LookupDisplayName(_paloc, missionKey, lo32: 0x101);
```

**Recommendation: Option B** — fewer FFI hops, fewer places where the
caller can compose the chain wrong. The editor's `LocalizationProvider`
already owns both handles; passing them both in is no inconvenience.
Pattern matches the editor team's status.md preference ("template
expansion belongs in the parser, not spread across the FFI").

### Open question — which lo32 to query

The user's main-quest titles are at `lo32 = 0x101` (individual quest
line items). Arc/chapter headings like "Ambush" or "Trials of Kindness"
are at `lo32 = 0x100`. The mission bridge defaults to `0x101` for the
common case but the caller may want both available; that's why Option B
takes `lo32_namespace` as an argument.

### Path

1. Extract `missioninfo.pabgb` and `questinfo.pabgb` from group 0008
   (one-line PAZ extract; already done into `out/baselines/1.06/`
   locally — gitignored).
2. Hexpat both. They look paired — same row shape across both files,
   different prefix on the internal-name string. The grep pass already
   surfaced ~1,000 `Quest_*` names + ~hundreds of `Mission_*` names, so
   the per-row layout is `[u32 key][u32 name_len][name bytes][rest]` —
   likely identical to iteminfo.
3. Build the parsers as `src/mission_info/` + `src/quest_info/`. The
   schema is small (probably 5–15 fields per row vs iteminfo's 100+);
   byte-roundtrip the same way as item_info.
4. Add `src/c_abi/mission_info.rs` + `src/c_abi/quest_info.rs` with
   Option B's surface. Also add `crimson_calculate_checksum` to
   `src/c_abi/checksum.rs` (new tiny file) — useful beyond just this
   workflow.
5. Live-install test against the user's verified mappings (the table
   above gives ground truth — assert that
   `missioninfo_lookup_display_name(handle, paloc, MissionKey, 0x101) ==
   "Unfamiliar Lands"` etc.).

**Size**: 2–3 sessions. The hash transform is verified; the bulk is
schema RE for the two `.pabgb` files. Less work than the original plan
because no template-resolver is needed.

---

## 4. QuestGaugeKey — SHIPPED (Pattern A, NO hash hop)

**Bridge SHIPPED** in `src/c_abi/quest_gauge_info.rs`.

**File**: `0008/.../gamedata/questgaugeinfo.pabgb` (40 KB on 1.06,
~380 valid rows).

**Important deviation from mission/quest/stage**: exhaustive PALOC
probe confirmed **QuestGauge names have zero PALOC hits at any
namespace byte** (every u64 PALOC key in the 124,800-entry English
table was scanned; not a single match). Gauges are internal-only
progress meters (kill counters, faction-operation tickers,
region-defense gauges) — the localized name a player sees comes from
the referenced stage / mission, not the gauge itself.

Practical consequence for the C ABI: the bridge exposes only
`lookup_string_key` — there is **no `lookup_display_name`** function.
The editor's resolved-name column surfaces the internal name directly
(e.g. `"QuestGauge_WindCliffFort"`), which is enough context for a
modder to recognise the gauge.

Names follow the convention `QuestGauge_<region_or_theme>`. The
parser uses a slightly tighter anchor-scan validator than its
sibling parsers (first byte must be ASCII letter AND name must
contain at least one `_`) to filter ~9 body-byte false positives the
looser rules would let through.

### Shipped surface

```c
crimson_questgaugeinfo_load_from_{file,bytes}(...)
crimson_questgaugeinfo_entry_count(handle, &count)
crimson_questgaugeinfo_lookup_string_key(handle, key, buf, len, &req)
    // → "QuestGauge_WindCliffFort" (the only resolution surface)
crimson_questgaugeinfo_get_entry(handle, idx, &key, buf, len, &req)
crimson_questgaugeinfo_free(handle)
```

5 tests (4 c_abi bridge + 1 parser module) pin known mappings
against the live install. No `paloc_handle` argument anywhere on
the bridge — keeps the API surface honest about what the gauge
table can and can't resolve.

### Future enhancement (out of scope)

The row body contains references to other keys — the first 4 bytes
after the name look like a 7-digit u32 in the 1000xxx range, likely
a related MissionKey or StageKey. A future enhancement could parse
the body and chain into the mission / stage bridges for "this gauge
tracks mission/stage X" resolution. The current bridge stays
single-purpose.

---

## 5. StageKey — SHIPPED (Pattern A + hash hop)

**Bridge SHIPPED** in `src/c_abi/stage_info.rs`. The editor's earlier
"save-internal" classification was wrong; the same hash transform
applies, and the bridge surfaces ~57k row names with PALOC display
titles at `lo32 ∈ {0x101, 0x102}`.

### Findings

- `stageinfo.pabgb` is the source of truth — **26 MB**, 57,094 distinct
  ASCII identifiers. Largest gamedata table we've touched.
- Names are **region-themed**, not `Stage_*` prefixed:
  `Intro_Tutorial_Miseenscene_00`, `DelesyiaCastle_Herbert_BlueStone`,
  `Beighen_Camp_Patrol_I`, `Hernand_Normal_Start_Child6`, etc.
- The hash probe surfaced **6,911 PALOC matches** at `lo32 ∈ {0x100,
  0x101, 0x102}`. `0x102` is new vs Mission/Quest — it's the
  description field for stages and shops (e.g. `Shop_Dem_GreenfieldFarm`
  at 0x101 → "Produce Shop", at 0x102 → "A merchant who sells fresh
  products from cultivators and ranchers…").
- **End-to-end test**: `StageKey 1004305` (from the editor's screenshot
  of `_stageStateData[0]._key`) → anchor in stageinfo at offset
  `0x159790B` → name `Intro_Tutorial_Miseenscene_00` → hash
  `3530966846` → PALOC u64 `(3530966846 << 32) | 0x101` →
  **"Mise-en-scene Before Intro Combat"**. ✓

### Path

1. Schema RE for `stageinfo.pabgb`. Same `[u32 key][u32 name_len][name]`
   header as mission/quest/iteminfo, so the bridge can use the anchor
   scan shortcut without a full schema parser.
2. Build `src/c_abi/stage_info.rs` mirroring the missioninfo bridge.
   Default `lo32 = 0x101` for title lookups.
3. **High value**: 46,541 rows per save means this bridge resolves more
   names than any other.

**Size**: 1–2 sessions for the bridge (no investigation gate needed
now). Schema RE may want a hexpat pass on the larger file but the
bridge's minimum surface doesn't need byte-roundtrip.

---

## 6. FieldNPC / FieldGimmick / CharacterKey — investigation findings (2026-05-14)

**Status (updated 2026-05-15): CharacterKey bridge SHIPPED. Resolution
chain is verbatim what §6 originally documented (cat-byte strip → PALOC
at `lo32=0x30`, NO hash hop). Coverage stays at the 22% sample-bias
figure; the bridge surfaces a miss as `NOT_FOUND` so the caller can
fall back to the `lookup_string_key` internal-name surface — that
surface lights up the full 100% of CharacterKeys that have a row in
`characterinfo.pabgb`. A high-level
[`crimson_characterinfo_resolve_portrait`](../src/c_abi/character_info.rs)
also ships, chaining the display-name lookup with a fuzzy match
against the
[`crimson_paz_list_npc_portraits`](../src/c_abi/paz.rs) output to land
the right DDS for named NPCs (`0x0a000001 → "Kliff" →
ui/texture/image/portraitimage/cd_portraitimage_chracter_kliff.dds`,
score 100, live-test verified).**

### What the earlier plan got wrong

Earlier text framed FieldNPCSaveDataKey as a *spawn template ID* that
resolves through a separate lookup into a real CharacterKey. The
handoff data disproves that framing — the original §6 sketch:

```
FieldNPCSaveData {
  _fieldNpcSaveDataKey: 3..233          ← save-local spawn-slot index, NOT hashed
  _characterKey:        0x02000010..    ← already the real char key (sibling!)
  _spawnFieldInfoKey:   FieldInfoKey
  _friendly:            bool
}
```

- **FieldNPCSaveDataKey** values are tiny u8-shaped integers (range 3–233,
  hi-byte=0 across all 103 entries). Save-internal index — no
  game-data table to look up.
- **FieldGimmickSaveDataKey** same shape: range 1788–953,478,
  hi-byte=0 across all 4,363 entries. Pairs with sibling
  `_gimmickInfoKey` (which IS the real game-data key for the gimmick).
- The **real bridge target** is the `_characterKey` u32 sibling, not
  the `*SaveDataKey` itself.

### 2026-05-15 empirical confirmation against a live 1.07 save

Re-verified the verdict directly by parsing a 1.07 `slot0/save.save`
through the shipped `Save::parse + Body::decode_blocks` pipeline. Probe
lives at `_probe_live_save_field_blocks` in
[`src/c_abi/character_info.rs`](../src/c_abi/character_info.rs)
(`#[ignore]`, run with `--ignored --nocapture`).

**Results — the original verdict stands, but the field-level schema
is richer than the 4-field sketch above suggested:**

```
FieldNPCSaveData {                                          // 12 fields total
  [ 0] _spawnFieldInfoKey:                FieldInfoKey u32
  [ 1] _fieldNpcSaveDataKey:              u32                  ← slot index (1..228 in sample)
  [ 2] _friendly:                         Locator<ExperienceLevelSaveData>
                                                              ← NOT a bool — has an
                                                                experience-level child block
  [ 3] _nudeAppearanceIndexKey:           CharacterAppearanceIndexKey u64
                                                              ← unbridged key type!
  [ 4] _customizationAppearanceIndexKey:  CharacterAppearanceIndexKey u64
  [ 5] _armorDyeAppearanceIndexKey:       u8
  [ 6] _characterKey:                     CharacterKey u32     ← 0xCC_LLLLLL as documented
  [ 7] _touchID:                          u64
  [ 8] _nextFeedTime:                     u64       (often absent)
  [ 9] _prevFeedTime:                     u64       (often absent)
  [10] _isGiftRewardGranted:              bool      (often absent)
  [11] _memoryOfTargetList:               ReflectObject sublist
}
```

`_characterKey` empirical samples from the 228 instances:
- `0x07000002` (cat=0x07, lo24=2) → "Yann" ✓
- `0x08000002`, `0x0a000002`, `0x0b000002` → also "Yann" (same character,
  different cat-byte — confirms the cat-byte is a variant marker, the
  lo24 is the real identifier)
- `0x09000f4d` (cat=0x09, lo24=0xf4d) → "Noble" ✓
- 228 samples / 222 distinct values / 90+ distinct cat-bytes
  (range 0x06–0xfe). The shipped `crimson_characterinfo_*` bridge
  resolves these correctly.

```
FieldGimmickSaveData {                                      // 43 fields total
  [ 0] _fieldGimmickSaveDataKey:          u32      ← slot index, range [131, 957806]
  [ 1] _fieldSaveDataReason:              u8
  [ 2] _saveRootFieldGimmickSaveDataKey:  u32      ← parent-gimmick reference
                                                     (gimmicks form a tree!)
  [ 4] _ownerLevelName:                   string (InlineBytes)
  [ 5] _stageKey:                         StageKey u32        (often absent)
  [ 6] _levelOriginSceneObjectUuid:       uint4 (16 bytes)
  [ 7] _item:                             Locator<ReflectObject>
  [ 8] _autoSpawnOwnerData:               Locator<ReflectObject>
  [ 9] _gimmickInfoKey:                   GimmickInfoKey u32  ← the real bridge target
                                                                (4264 instances, 549 distinct)
  [12] _originSpawnTransform:             Transform (40 bytes)
  [13] _initStateNameHash:                HashCode32 u32
  [23] _spawnStyle:                       SpawnStyle u8
  [37] _fieldGimmickSaveData_ConstraintList:    ObjectList
  [39] _fieldGimmickSaveData_TargetedConstraintList:    DynamicArray<u32>
  [41] _fieldGimmickSocketIndex:                DynamicArray<u32>
  (+ ~26 mostly-absent flags / sub-lists / timers)
}
```

Slot range `[131, 957806]` slightly wider than the handoff's
`[1788, 953478]` but same shape — save-internal index, no gamedata
table. The shipped `crimson_gimmickinfo_*` bridge resolves all 4264
sample `_gimmickInfoKey` values via the no-hash-hop chain at
`lo32 = 0x200`.

### Unbridged key types surfaced by the 2026-05-15 probe

- **`CharacterAppearanceIndexKey` (u64)** — appears as
  `FieldNPCSaveData._nudeAppearanceIndexKey` and
  `_customizationAppearanceIndexKey`. Unbridged. Likely indexes into
  `characterappearance_*.pabgb` (or similar) in `0008/gamedata/binary__/client/bin/`.
  Would resolve NPC outfit / customisation selection.
- **`_friendly: Locator<ExperienceLevelSaveData>`** — non-trivial
  nested block per NPC carrying friendliness / experience progression.
  Decoded by the existing save body parser; no separate bridge needed
  unless the editor wants typed accessors.
- **`_saveRootFieldGimmickSaveDataKey`** — gimmicks reference parent
  gimmicks via this field. Useful for reconstructing gimmick
  hierarchies (e.g. a container's contents). Same key space as the
  slot index; no separate bridge needed beyond walking the save.

### CharacterKey investigation — what was probed

- `characterinfo.pabgb` exists in `0008/gamedata/binary__/client/bin/`
  (1.5 MB, ~17k anchor-scannable rows after filtering all-digit
  embedded stringinfo refs). Schema same shape as missioninfo/etc:
  `[u32 key][u32 name_len][name][...variable body]`. Row keys top out
  around 1.7M; hi-byte=0 across real rows.
- **Resolution chain (Pattern A, NO hash hop)**:
  ```
  save._characterKey 0x07000002
    └─► row_key = (charkey & 0xFFFFFF) = 2  (strip cat byte)
    └─► PALOC u64 = (row_key << 32) | 0x30 = 0x200000030
    └─► PALOC[u64] = "Yann"
  ```
  Verified for a handful: `0x07000002 → "Yann"`, `0x06000f4a → "Pierre"`,
  `0x0a000001 → "Kliff"`, `0x09000f4d → "Noble"`.
- Coverage against the editor's 221 unresolved CharacterKey sample:
  **49 / 221 = 22%** at `lo32 = 0x30`. The other 172 have lo24 values
  that aren't in PALOC at lo32=0x30 (or any other namespace — those
  hits are coincidental collisions with iteminfo/skillinfo small IDs).
  - Sibling files probed: `npcinfo.pabgb` (46 KB, not a row table —
    different schema), `charactergroupinfo.pabgb` (293 KB, 496 rows
    with hashed-looking u32 keys like `0x42c70042`, none overlap with
    save CharacterKeys).
- **Hypothesis for the 78%**: characters without standalone PALOC
  display names. Generic field NPCs ("Peddler", "Stranger", "Fisherman"
  etc that the user listed) DO exist in PALOC at lo32=0x30 — but the
  sample save's 221 unresolved values don't include those specific
  characters. If a future save touches a Finley / Bruna's Assistant /
  Herspia Packhorse instance, those would resolve through this same
  chain.

### Cat byte (hi-byte of save's `_characterKey`)

- Range 0x02–0xfe, 90+ distinct values across the 221 sample, *and*
  re-verified against the 1.07 live save (228 samples) — the
  distribution shape is unchanged.
- Same lo24 appears under multiple cat bytes:
  `0x07000002 / 0x08000002 / 0x0a000002 / 0x0b000002` all resolve to
  `lo24=2 → "Yann"`.
- Looks like a variant / spawn-region / faction marker, not an
  indirection into a per-region row table. The bridge strips it.

### ABI shape — recommended when this work resumes

Mirrors mission/quest/stage modulo the no-hash-hop change:

```
crimson_characterinfo_load_from_bytes / _free / _entry_count
crimson_characterinfo_lookup_string_key(handle, charkey, …)
    -> internal name from characterinfo row (after `& 0xFFFFFF` strip)
crimson_characterinfo_lookup_display_name(handle, paloc_handle, charkey, lo32)
    -> PALOC display via ((charkey & 0xFFFFFF) << 32) | lo32
    Default lo32 = 0x30. NOT_FOUND on the 78% miss path; caller
    falls back to lookup_string_key.
```

### Open decisions for the next session

- **CharacterKey display-chain 78% miss**: still sample-bias rather
  than missing data. The 2026-05-15 re-probe against a 1.07 save
  showed the same distribution. Lifting coverage requires either
  re-probing `npcinfo.pabgb` with a different schema model (it's
  still flagged as "not a row table"), or a wider sample of saves
  that touch named field NPCs whose entries currently aren't in the
  221-key set.
- **`CharacterAppearanceIndexKey` (u64)** — newly surfaced by the
  2026-05-15 probe. Unbridged. Probably indexes into a
  `characterappearance*.pabgb` file in `0008/gamedata/binary__/client/bin/`
  but that hasn't been located yet. Would resolve NPC outfit /
  customisation selection.
- Whether to expose a standalone `crimson_paloc_lookup_character`
  helper (no characterinfo handle needed — pure PALOC arithmetic) so
  the editor can resolve display names without parsing characterinfo.
  Cheaper but loses the internal-name fallback for the 78%.
- FieldGimmickSaveDataKey verdict: **empirically confirmed
  save-internal** (slot range `[131, 957806]` in the 1.07 sample). No
  bridge target other than the already-shipped sibling
  `_gimmickInfoKey` chain.

### Scratch artifact

The investigation probe lived at `src/c_abi/_probe_characterinfo.rs`
behind `#[cfg(test)] #[ignore]`. Deleted after the findings were
folded into this doc. The bridge was re-built from this section's
chain spec on 2026-05-15 — see `src/character_info/mod.rs` (parser)
and `src/c_abi/character_info.rs` (bridge + portrait matcher).

### Shipped surface (2026-05-15)

```text
src/character_info/mod.rs            # anchor-scan parser
src/c_abi/character_info.rs          # C ABI bridge + portrait matcher

crimson_characterinfo_load_from_bytes / _load_from_file / _free
crimson_characterinfo_entry_count
crimson_characterinfo_lookup_string_key    # internal name fallback (full coverage)
crimson_characterinfo_lookup_display_name  # PALOC chain @ lo32=0x30 (22% coverage)
crimson_characterinfo_get_entry            # enumeration
crimson_characterinfo_resolve_portrait     # high-level CharacterKey → DDS
```

The portrait matcher (`resolve_portrait`) tokenises filenames from
the six recognised NPC-portrait prefixes, normalises against
CrimsonForge's `_normalize_lookup_key` rule (`[^a-z0-9]+ → _`, strip),
and scores via a five-tier ladder (exact 100 → boundary 80 →
word-bounded mid 65 → raw substring 45 → collapsed substring 25).
Display name carries full weight; internal name half — the caller
can apply their own threshold on the optional `out_score`.

---

## 7. GimmickInfoKey — SHIPPED (Pattern A, NO hash hop)

`FieldGimmickSaveDataKey` itself is save-internal (verified in §6).
The save block always pairs it with the `_gimmickInfoKey` sibling
(TypeName `GimmickInfoKey`), and that one IS the gamedata key.

### Bridge findings

`gimmickinfo.pabgb` lives in `0008/gamedata/binary__/client/bin/`,
same dir as the other shipped tables. The probe pass (2026-05-14)
established:

- **Schema same shape as iteminfo/missioninfo/etc.**:
  `[u32 key][u32 name_len][name][...variable body]`. Row keys span
  hi-bytes `0x00, 0x01, 0x07, 0x08, 0x09`, so the `(key >> 24) == 0`
  constraint other parsers use would miss ~12% of real rows. Scanner
  here caps at `(key >> 24) < 0x10` (handles seen + headroom).
- **Body-byte noise** — two specific false-positive patterns
  (`UnnamedTrigger_0` at `0x01000000`, `GimmickOnExitState` at
  `0x7475706e` = ASCII "tupn") filtered via `(key & 0xFFFFFF) != 0`
  and the hi-byte cap.
- **Identity PALOC chain, NO hash hop.** Save's `_gimmickInfoKey` is
  the PALOC hi32 directly. `(gimmick_key << 32) | 0x200` resolves to
  the display label (`"Fire"`, `"Prison"`, `"Broken Box"`, ...).
  Hash-hop probe over 30 sample internal names: zero hits at any
  namespace — confirms the row key, not its hashlittle2, is the
  PALOC key.
- **PALOC namespaces with meaningful content**:
  - `lo32 = 0x200` (512) — display label (530/530 of sample save's
    `_gimmickInfoKey` values resolve here). **Bridge default.**
  - `lo32 = 0x19202` (102914) — long description (~9 rows; furniture
    inspect text).
  - `lo32 = 0x60` (96) — interaction verb (Move / Skin / Load / Open).
  - Coincidental collisions at `0x30`, `0x70`, `0x71` (character /
    item tables share the small-integer key space) — caller selects
    the namespace they trust.
- **Coverage**: 527/530 (99.4%) of the editor's sample save's
  `_gimmickInfoKey` values resolve through the scanner; the 3 misses
  are dev/test gimmicks or rows the scanner couldn't isolate from
  surrounding body noise.

### Shipped surface

```text
src/gimmick_info/mod.rs            # parser
src/c_abi/gimmick_info.rs          # C ABI bridge

crimson_gimmickinfo_load_from_bytes / _load_from_file / _free
crimson_gimmickinfo_entry_count
crimson_gimmickinfo_lookup_string_key   # internal name fallback
crimson_gimmickinfo_lookup_display_name # PALOC chain, no hash hop
crimson_gimmickinfo_get_entry           # enumeration
```

The display-name function keeps the `handle` parameter in its
signature even though the chain doesn't consult the gimmickinfo
table at resolve time — kept for API symmetry with sibling bridges
and to allow a future cat-byte transform (or analogous indirection)
to hook in without an ABI break.

### Tests

5 tests added (4 c_abi plumbing/chain + 1 parser live).
Clippy clean both modes.

---

## 8. SubLevelKey — SHIPPED (Pattern A, NO PALOC chain)

Save-side `SubLevelKey (u32)` identifies a **per-faction /
per-stat / per-skill progress track**. Lives in
`SubLevelSaveData._list[N]._key` blocks paired with sibling `_level`,
`_maxAchievedLevel`, `_experience` fields, plus
`AlertHistorySaveData._subLevelKey` notification entries.

### What got shipped

`sublevelinfo.pabgb` is **8.6 KB** — by far the smallest of the
bridged tables. ~40 real rows after the anchor scanner's strict
filter (first byte must be ASCII letter, hi-byte=0 on the key).
All seven values from the handoff resolve cleanly:

| Key | Internal name | Track type |
|---:|---|---|
| 522 | `SkillPoint_Oongka` | Per-character skill points |
| 600 | `Contribution_Graymane` | Faction reputation |
| 603 | `Contribution_Demenissian` | Faction reputation |
| 604 | `Contribution_Pailunese` | Faction reputation |
| 605 | `Contribution_Delesyian` | Faction reputation |
| 606 | `Contribution_Tashkalpan` | Faction reputation |
| 701 | `LiberationRefugee` | Story progress |

Plus the surrounding row clusters: stat tracks (101–113 = Hp/Mp/
Stamina/CriticalRate/etc.), Abyss variants (201–203), achievement
tracks (401–403), other faction tracks not in this save, and religion
tracks (1000–1002).

### Shipped surface

```text
src/sub_level_info/mod.rs               # parser
src/c_abi/sub_level_info.rs             # C ABI bridge

crimson_sublevelinfo_load_from_bytes / _load_from_file / _free
crimson_sublevelinfo_entry_count
crimson_sublevelinfo_lookup_string_key
crimson_sublevelinfo_get_entry
```

**No `lookup_display_name`** — mirrors `quest_gauge_info`. PALOC
probe confirmed zero meaningful hits at any namespace:
- Pattern A (raw key) hits at `lo32 ∈ {0x402f1, 0x802f1, 0xc02f1}`
  return generic UI tooltip strings ("Unavailable during combat.")
  that share the small hi32 by coincidence, not real localizations.
- hashlittle2(name) hash-hop probe: zero hits at any namespace.

The localized UI label the player sees (e.g. "Demenissian
Reputation") is composed at runtime from the row's prefix
(`Contribution_`, `Religion_`, `SkillPoint_`) plus a suffix faction
or character name resolved through a different table — out of scope
for this bridge.

### Tests

5 tests added (4 c_abi plumbing + 1 parser live-integration).
Clippy clean both modes.

---

## 9. CharacterAppearanceIndexKey — INVESTIGATED, bridge deferred (2026-05-15)

Surfaced as a new key type by the live-save probe in §6 (live-save
verification pass). `FieldNPCSaveData._nudeAppearanceIndexKey` and
`_customizationAppearanceIndexKey` are both `CharacterAppearanceIndexKey
(u64)`. The user asked to ship a resolver bridge; the investigation
revealed a structural gap that makes a useful bridge premature.
**Probe lives at `_probe_character_appearance_index` in
[`src/c_abi/character_info.rs`](../src/c_abi/character_info.rs)
(`#[ignore]`)**. Resume work from there + this section.

### What's pinned

**File location**: `0008/gamedata/binary__/client/bin/`
- `characterappearanceindexinfo.pabgb` (236 KB) — entry bodies
- `characterappearanceindexinfo.pabgh` (97 KB) — index

PABGH/PABGB pair pattern (same shape as `skill.pabgh` + `skill.pabgb`).

**PABGH schema** (verified — file size matches exactly):

```text
[u32 count = 8143]
[count × (u64 key, u32 offset)]
```

**PABGB entry layout** (uniform across all observed entries):

```text
[u64 key (matches the pabgh key verbatim)]
[21 bytes opaque body]   ← layer / color / asset IDs, no strings
```

Entries are all 29 bytes (8 + 21). Body schema **not RE'd** — bytes
are dense binary parameters with no string fields. Would require
either IDA decompilation of the appearance loader or cross-version
diff against a save with known cosmetic changes to crack.

**Save → PABGH transform** (verified end-to-end):

```rust
// save_u64 bytes (LE): [b0, b1, b2, b3, b4, b5, b6, b7]
//   - bytes 0..2 = appearance ID (lo24)
//   - byte 3     = category, signed i8
//   - bytes 4..6 = sign-extension padding of byte 3 (all 0xff or 0x00)
//   - byte 7     = variant marker — DROP, not part of the lookup
let b3   = ((save >> 24) & 0xFF) as i8;
let lo24 = save & 0x00FF_FFFF;
let pabgh_key = (u64::from((b3 as i32) as u32) << 32) | lo24;
```

Example: `0xeeffffff_fe00000b → 0xfffffffe_0000000b` (variant byte
`0xee` stripped; signed-byte `0xfe` sign-extends to hi32 = `0xfffffffe`).

### Why the bridge wasn't shipped — the 87% miss path

Of the 122 distinct `CharacterAppearanceIndexKey` values in the
sample save's 228 `FieldNPCSaveData` blocks, **only 9 (7%) map to a
PABGH entry** via the canonical transform:

| Category byte | Distinct save values | PABGH hits |
|---|---:|---:|
| `0xfe`        | 112 | 9 |
| `0x01`, `0x02`, `0x06`, `0x11`, `0x13`, `0x33`, `0x57`, `0x60` | 10 (1-2 each) | 0 |

The PABGH's `0xfffffffe` bucket (7,027 of 8,143 entries) has lo32 ∈
`{1, 2, 4, 6, 100, 400-459, 3914, 3919, …}` — sparsely populated with
named-character + mercenary appearance templates. The save uses
lo24 = 11, 12, 13, 14, 15, 16, 18, 21, 36, 50, 57 (`0x0b`–`0x39`) —
all in gaps the PABGH skips.

The non-`0xfe` categories (`0x01`, `0x02`, etc.) have zero hits.
Those categories exist in the PABGH (hi32 = 1, 2, 3, …, 0x14 each
have 11–113 entries) but our save's lo24 values for those categories
don't land on real entries either.

**Conclusion**: the other 87% likely reference a procedural /
template-generated appearance system that isn't in this `.pabgh`
file. Without locating that source (and without the 21-byte body
schema), shipping a 7%-coverage validation-only bridge wouldn't earn
its keep.

### Open RE questions

1. **Where do the other 87% appearance refs resolve?** Candidate
   guesses: a sibling table in 0008 we haven't located, a runtime
   procedural appearance system not exposed via gamedata, or maybe
   a separate per-category mini-table per `b3` value. Worth scanning
   0008 for files with names like `*appearance*` / `*npccustomization*`
   / `*defaultappearance*`.
2. **21-byte body schema.** IDA decompile the appearance loader (look
   for code that reads `characterappearanceindexinfo.pabgb` — the
   factory function should reveal the field layout). Likely fields:
   outfit ID, color IDs (maybe 3-4 of them), accessory flags.
3. **Variant byte (save byte 7) semantics.** Is it a per-NPC instance
   seed for procedural variation, a faction marker, or something
   else? Multiple NPCs share `(b3, lo24)` = `(0xfe, 0x0b)` with
   different variant bytes — could be the same template "colored"
   differently per spawn.

### When this work resumes

1. Run the probe with `--nocapture` to refresh the data points.
2. Triangulate the 87% miss source (Q1 above).
3. Decide whether a bridge that surfaces the canonical key + raw
   21-byte body is useful even without body schema (it would let the
   editor diff appearance keys between saves, even if it can't show
   the user "Hair: long, Color: brown").
4. If yes, mirror the `skill_info` parser pattern for the
   PABGH+PABGB pair, add a C ABI surface with `load_from_bytes`,
   `lookup_offset`, `get_body_bytes`.

---

## Open RE questions

### ~~Q1. The u32 → u64 quest-title hash transform~~ ✅ RESOLVED

`hashlittle2_c(internal_name) << 32 | lo32_namespace`. Verified against
10/10 user-provided Main Quest titles and 701 broader matches. See
"Verified hash transform" above.

### ~~Q2. Does StageKey use the same transform?~~ ✅ RESOLVED

Yes — `hashlittle2_c(name) << 32 | 0x101` (titles) or `0x102`
(descriptions). 6,911 PALOC matches, including `StageKey 1004305 →
"Intro_Tutorial_Miseenscene_00" → "Mise-en-scene Before Intro Combat"`.
See §5.

### ~~Q3. KnowledgeKey small-vs-large namespace split~~ ✅ RESOLVED

Leaf KnowledgeKey rows resolve via the **hash hop** at `lo32 = 0x490`
(title), `0x491` (description), and 0x49D/E/F variants. The
"small-value PALOC 0x93 entries" the editor noted are actually the
**knowledge group rollup** (Creatures, People, Bosses, etc.) — a
separate namespace where `hi32` is a literal integer group ID, not
a hash. Those rollups live in `knowledgegroupinfo.pabgb`; the
shipped bridge only covers the leaf table. See §2 for the full
breakdown.

### Q4. What is lo32 = 0x490 (1168)?

Saw 4 PALOC hits for `lo32 = 0x490` in the early Mission/Quest pass
(e.g. "Ambush" at hi32 ∈ {679530922, 934361767, 2965653965}). All
values at this namespace look mission-related but the exact meaning
vs `0x101` is unclear. Probably a sub-category of mission text
(objective vs title? description vs subtitle?). Worth a quick scan
during the missioninfo bridge work — if it's noisy, document and
ignore; if it carries useful text the editor wants, bridge in.

The StageKey probe also surfaced **`lo32 = 0x102` (258)** as a new
namespace — 404 hits, all looking like description/secondary text
(shops, stages). The mission/quest bridges should probe this byte
too; if it carries useful descriptions, expose it via the
`lookup_display_name` API's namespace argument.

### Q5. The 6 unresolved QuestKey values with odd magnitudes

From the keycases handoff: 10001, 10011000, 11006000, 10029000,
10040400, etc. — magnitudes that don't fit the standard `1xxxxxx` range
of regular QuestKeys. May be event/special-quest IDs or test data.
Investigate during the questinfo schema RE.

---

## Suggested execution order

Picking by `(value × tractability) / risk`:

1. ~~**SkillKey**~~ — done
2. ~~**`crimson_calculate_checksum` C ABI**~~ — done
3. ~~**MissionKey via missioninfo bridge** (Option B)~~ — done
4. ~~**QuestKey via questinfo bridge**~~ — done
   (`src/c_abi/quest_info.rs`, 5 tests including live full-chain
   integration against 6 ground-truth mappings — covers both
   `lo32=0x100` arc headings and the `lo32=0x101` secondary
   namespace from `Challenge_Maze`)
5. ~~**StageKey via stageinfo bridge**~~ — done
   (`src/c_abi/stage_info.rs`, 5 tests including the shop description
   case at `lo32=0x102`; 57k+ rows resolved per save)
6. ~~**KnowledgeKey**~~ — done
   (`src/c_abi/knowledge_info.rs`, 5 tests; hash hop applies same
   as mission/quest/stage. Default `lo32 = 0x490` for title.
   Category breadcrumb via `knowledgegroupinfo.pabgb` is a future
   enhancement, not shipped.)
7. ~~**QuestGaugeKey**~~ — done
   (`src/c_abi/quest_gauge_info.rs`, 5 tests; no `lookup_display_name`
   because gauges have no PALOC entries at any namespace)
8. **FieldNPC / FieldGimmick / CharacterKey** — **PAUSED** after
   investigation pivot. The save-side `*SaveDataKey` is a slot index,
   not a hashed character ID — the real bridge target is the sibling
   `_characterKey` u32 → `characterinfo.pabgb` chain. Prototype hits
   22% (49/221) of the sample save; the 78% miss path needs
   re-investigation of `npcinfo.pabgb` or a different sample save.
   Full findings in §6.
9. ~~**SubLevelKey**~~ — done
   (`src/c_abi/sub_level_info.rs`, 5 tests; no `lookup_display_name`
   because sub-level rows have no PALOC entries at any namespace —
   localized UI label is composed at runtime from prefix + faction
   name resolved elsewhere)

Compared to my earlier plan: MissionKey/QuestKey moved from "medium
3–5 sessions" through "small 1–2 sessions (PALOC reverse-index)" to
"medium 2–3 sessions but the only real work is schema RE". The
implementation work is straightforward; the architecture is now
de-risked.

---

## Where these bridges plug in

The Save Editor calls each bridge once at startup (after PAZ-extracting
the relevant inputs) and holds the handles for the session. Per-row
lookups during editing are zero-allocation HashMap hits.

C# integration touchpoints (per `CrimsonAtomtic/docs/status.md` §675):

- `NativeSaveLoader.cs` `NativeMethods` — add `[LibraryImport]` declarations
- New `I<NewCatalog>Catalog.cs` + `Native<NewCatalog>Catalog.cs` in
  `src/CrimsonAtomtic.RustInterop/`
- `LocalizationProvider.cs` — `TryBootstrap<NewCatalog>` +
  `ResolveMissionKey(uint key) → string?` /
  `ResolveQuestKey(uint key) → string?` methods chaining
  `_missionInfo.LookupDisplayName(_paloc, key, 0x101)` (or
  `_questInfo.LookupDisplayName(_paloc, key, 0x100)`).
- `TypeNameToTypeByte` dispatch — `MissionKey` and `QuestKey` can now
  be added (currently excluded because the editor only checked u32 type
  bytes; the new resolvers operate on u64 keys and return real titles).

No public API breakage on existing surface.

---

## Decisions settled (recorded against editor-side prefs + verified findings)

- ~~Template-resolver location~~ — **no template resolver needed**. The
  hash-hop chain replaces the 0xC1 reverse-index approach entirely.
- ~~Skill / Knowledge — one bridge or two~~ — **two bridges** (different
  source files).
- ~~FieldNPC ABI shape~~ — **two outputs in one call**
  (`out characterKey`, `out stringInfoHash`).
- ~~Mission/Quest titles via gamedata parser~~ — **yes**, via the
  cracked hash hop: `missioninfo.pabgb` for individual quest titles,
  `questinfo.pabgb` for arc/chapter headings, hashed and looked up
  against PALOC u64 keys.
- ~~Q1 hash transform~~ — **`hashlittle2_c(internal_name) << 32 |
  lo32_namespace`**, verified end-to-end against 7 user-provided Main
  Quest titles with matching save-side MissionKey values + completedTime
  monotonicity.
- ~~MissionKey/QuestKey ABI shape~~ — **Option B** (one-shot
  `_lookup_display_name(paloc_handle, key, lo32)` returning the
  resolved string in one FFI call).
- ~~Q2 StageKey transform~~ — **same hash hop applies**. Editor's
  "save-internal" classification was wrong. 6,911 PALOC matches in
  `stageinfo.pabgb`. `lo32 = 0x102 (258)` is a new namespace for
  stage descriptions.
- ~~Hash exposed to C ABI~~ — shipped as
  `crimson_calculate_checksum(data, len, &out)` in
  `src/c_abi/checksum.rs`. Tests pinned against the verified
  `(internal_name, hi32)` mappings so a future regression to the
  Jenkins variant would surface immediately.
- ~~MissionKey bridge~~ — shipped (`src/c_abi/mission_info.rs` +
  `src/mission_info/`). Anchor-scan parser + Option B one-shot
  `lookup_display_name` that chains MissionKey → name → hash →
  PALOC u64 → display title in one FFI call. Live integration test
  against all 7 ground-truth mappings passes.
- ~~QuestKey bridge~~ — shipped (`src/c_abi/quest_info.rs` +
  `src/quest_info/`). Same architecture as missioninfo. Default
  `lo32 = 0x100` for arc/region headings; caller passes `0x101` for
  the secondary namespace some rows expose (e.g. `Challenge_Maze`).
- ~~StageKey bridge~~ — shipped (`src/c_abi/stage_info.rs` +
  `src/stage_info/`). Same architecture. Default `lo32 = 0x101` for
  stage titles; `0x102` for the longer descriptions that shops and
  some stages carry.
- ~~QuestGaugeKey bridge~~ — shipped
  (`src/c_abi/quest_gauge_info.rs` + `src/quest_gauge_info/`). Pattern
  A only — **no hash hop function**. Exhaustive PALOC probe confirmed
  gauges aren't localized at any namespace byte. The bridge surfaces
  internal names like `"QuestGauge_WindCliffFort"` for the editor's
  resolved-name column. The "honest" API design: no
  `paloc_handle` parameter anywhere, so callers can't be confused
  into thinking they'd get a localized title.
- ~~KnowledgeKey bridge~~ — shipped
  (`src/c_abi/knowledge_info.rs` + `src/knowledge_info/`). Same
  hash-hop shape as mission/quest/stage; 29,330 PALOC hits in the
  cross-validation probe. Default `lo32 = 0x490` for title;
  `0x491` for description; 0x49D/E/F for variants. Disproves the
  editor's earlier hypothesis that KnowledgeKey wouldn't resolve
  through PALOC.
- ~~SubLevelKey bridge~~ — shipped
  (`src/c_abi/sub_level_info.rs` + `src/sub_level_info/`). Pattern A
  only — **no `lookup_display_name` function**. PALOC probe across
  raw-key and hash-hop transforms returned only coincidental matches
  with generic UI tooltips. ~40 row entries for faction reputation,
  per-character skill points, stat caps, religion, and story
  progress tracks. All 7 handoff values pinned.
- ~~FieldNPCSaveDataKey / FieldGimmickSaveDataKey classification~~ —
  **save-internal verified** (values are dense small integers
  3–953,478 with hi-byte=0, never overlap with hashed gamedata
  keys). The real bridge target is the sibling u32 inside the same
  block (`_characterKey` for NPCs; `_gimmickInfoKey` for gimmicks).
  CharacterKey bridge prototyped to 22% coverage but not shipped —
  see §6 for the 78% miss-path TODO. GimmickInfoKey bridge SHIPPED
  at 99.4% coverage — see §7.
- ~~GimmickInfoKey bridge~~ — shipped
  (`src/c_abi/gimmick_info.rs` + `src/gimmick_info/`). Pattern A
  with **no hash hop** — the rarest shape so far. Save's
  `_gimmickInfoKey` is the PALOC hi32 verbatim; chain is
  `(gimmick_key << 32) | lo32`. Default `lo32 = 0x200` for display
  label; `0x19202` for description; `0x60` for interaction verb.
  Scanner uses a loose hi-byte cap (`< 0x10`) to cover cat-bytes
  0x01/0x07/0x08/0x09 + body-byte noise filter.

## Open user decisions

- Whether to invest in §5 (StageKey hash test) before or after §3
  (Mission/Quest bridge). The StageKey volume is 50× higher per save,
  but the user-facing surface in the editor is unclear for stages
  vs missions/quests. Likely Mission/Quest first because the editor
  already wants to render those.
- Whether to ship the `_lookup_string_key` (raw internal name) getter
  alongside `_lookup_display_name` on the mission/quest bridges. Cheap
  to expose, useful for debugging — recommended yes.

## Vendor flow

`CrimsonAtomtic/vendor/update_vendors.ps1` does `git reset --hard origin/dev`
on `vendor/crimson-rs`, so pushes here flow into the editor on the next
vendor refresh — **no PR coordination needed beyond keeping `dev`
green**. CI gates on `main` (the `clippy + cargo test` required check)
protect the merge path.
