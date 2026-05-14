# Save Editor key resolvers — plan

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
| **KnowledgeKey** | 392 | `knowledgeinfo.pabgb` | ✗ | ✗ | A — gamedata table |
| **MissionKey** | 1,299 | `missioninfo.pabgb` + PALOC u64 | ✗ | ✗ | **A + hash hop** |
| **QuestKey** | 6 | `questinfo.pabgb` + PALOC u64 | ✗ | ✗ | **A + hash hop** |
| **QuestGaugeKey** | 0 (rare) | `questgaugeinfo.pabgb` | ✗ | ✗ | A — gamedata table |
| **StageKey** | 36,613 | `stageinfo.pabgb` (likely) + hash hop | ✗ | ✗ | A + hash hop *(presumed)* |
| **FieldNPC CharacterKey** | 103 | unknown spawn table | ✗ | ✗ | TBD |
| **FieldGimmickSaveDataKey** | 4,363 | likely save-internal | ✗ | ✗ | save-internal *(presumed)* |
| **SubLevelKey** | 7 | unknown | ✗ | ✗ | TBD |
| **ItemKey** | 669 | `iteminfo.pabgb` (parser ahead of save) | ✅ | ✅ | A — gamedata table |

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
numeric PALOC u64 key, **701 clean hits** came back, broken down:

- `lo32 = 256` (0x100): 673 hits — sub-arc / region / chapter headings
  ("Mazes", "Roothold", "Pailune Faction", "Emperor of the Bonepit")
- `lo32 = 257` (0x101): 28 hits at this scan — quest line items
- `lo32 = 1168` (0x490): not in this scan; reserved for follow-up

Zero false positives observed. The transform is byte-deterministic.

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

## 2. KnowledgeKey — small, RE work needed first (Pattern A)

**File**: `0008/.../gamedata/knowledgeinfo.pabgb` (+ `knowledgegroupinfo.pabgb`)

**Editor-side correction**: `CrimsonAtomtic/docs/status.md` item #4 groups
SkillKey and KnowledgeKey under one "`skill_info` bridge" line, expecting
the same parser to resolve both. It can't — they live in different
`.pabgb` files. KnowledgeKey is a **separate bridge** (`src/knowledge_info/`
+ `src/c_abi/knowledge_info.rs`).

**Two-namespace finding from status.md §834**: small KnowledgeKey values
(1, 2, 4, 7, 51) live at PALOC 0x93 as **knowledge category names**
("Various Combat Skills", "Fundamentals of Cooking"). Large-numbered
keys are knowledge *entries* and resolve elsewhere — quite possibly via
the same hash-hop transform now that we know it exists. Worth testing
the transform here first before committing to a bridge design.

**Size**: 1–2 sessions.

---

## 3. MissionKey + QuestKey titles — Pattern A + hash hop

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

## 4. QuestGaugeKey — small (Pattern A)

**File**: `0008/.../gamedata/questgaugeinfo.pabgb`

**From QuestSaveData**: 311 instances per save. Top-level field in the
save block. Editor needs to surface the gauge name in the resolved-name
column.

Plausibly same hash-hop transform as Mission/Quest, with row names
following a `QuestGauge_*` convention (not yet verified — confirm
during schema RE).

**Size**: 1 session, low risk.

---

## 5. StageKey — investigate first (Pattern A + hash hop, probably)

**From the screenshots**: **46,541 instances per save** — the dominant
data type in QuestSaveData. From the editor handoff: **36,613 distinct
unresolved values** — by far the biggest worklist.

### Conflicting signals (now resolvable)

**Editor's earlier status.md call**: classified as "save-internal"
because PALOC lookup at the 5 known u32 type bytes returned nothing.
With the hash transform now verified, that classification is most
likely **wrong** — StageKey resolves via the same chain:

```
save StageKey → stageinfo.pabgb row → "Stage_*" internal name
              → hashlittle2_c → hi32 → (hi32 << 32) | <namespace>
              → PALOC u64 lookup
```

CrimsonForge's `character_asset_resolver.py:145` confirms
`gamedata/stageinfo.pabgb` exists in the walk-list.

### Path

1. **One-hour test**: extract `stageinfo.pabgb`, grep for `Stage_*`
   ASCII identifiers, hash each, check against PALOC u64 keys at
   `lo32 ∈ {0x100, 0x101, 0x490}`. If matches come back, the bridge is
   mechanical. If not, dig deeper.
2. If the transform applies: schema RE + Pattern A bridge as
   `src/c_abi/stage_info.rs`. With 46,541 instances this is the
   highest-volume bridge by far.
3. If it doesn't: confirm save-internal, document, no bridge work.

**Size**: 0.5 session for the investigation, then 1–2 sessions for the
bridge if it lands.

---

## 6. FieldNPC CharacterKey → real CharacterKey

The "FieldNPC key" stored in a save isn't a `CD_M0001_00_Ogre`-style
character ID — it's a **spawn template ID** that resolves through a
separate lookup to the underlying character.

**State of knowledge**:
- The lookup table has **not been located**. Candidates worth probing:
  - `charactertemplate.pabgb` / `charactertemplateinfo.pabgb` if it exists
  - `npcspawn.pabgb` / `fieldnpc.pabgb` / `fieldspawn.pabgb`
  - The world-level files (sublevels, region info) — spawn tables sometimes
    live with the level rather than with the character data
- The handoff bundle includes 103 unresolved FieldNPCSaveDataKey values
  with rich sibling context (`_spawnFieldInfoKey`, `_characterKey`,
  `_mercenaryNo`) — useful disambiguation signal during the
  investigation.

### ABI shape — editor's explicit preference (status.md item #5)

> *Recommended: two-output single call.*
> `crimson_<source>_lookup_character_key(handle, u32 spawnId, out u32 characterKey, out u32 stringInfoHash) → i32`.

Combine FieldNPC + FieldGimmick under one bridge **if they share the
same source file**; otherwise ship as two bridges with identical shape.

**Size**: 1 session for the investigation, then 2–3 for parser + bridge.

---

## 7. FieldGimmickSaveDataKey — pair with FieldNPC

**Editor finding (status.md §862)**: every harvested gimmick sample
returns no PALOC entry at any of the 5 known u32 type bytes. Editor
classifies as save-internal — but same caveat as StageKey: with the
hash transform known, that classification needs retesting.

Treat as a sibling of FieldNPC. When the FieldNPC investigation finds
its spawn table, scan the same file (and immediate neighbors) for the
gimmick keys.

**Size**: marginal cost ≈ 0.5 session on top of FieldNPC.

---

## 8. SubLevelKey — unknown, lowest priority

`sublevelinfo.pabgb` is referenced by CrimsonForge's
`localization_usage_index.py` under `CATEGORY_KNOWLEDGE`. Only 7
distinct values in the handoff — defer until the editor concretely
needs it.

**Size**: 1–2 sessions when prioritized.

---

## Open RE questions

### ~~Q1. The u32 → u64 quest-title hash transform~~ ✅ RESOLVED

`hashlittle2_c(internal_name) << 32 | lo32_namespace`. Verified against
10/10 user-provided Main Quest titles and 701 broader matches. See
"Verified hash transform" above.

### Q2. Does StageKey use the same transform?

Half-session test described in §5. Likely yes given the parallels in
naming conventions (`Stage_*` strings should grep cleanly from
`stageinfo.pabgb`); confirm before committing to a bridge design.

### Q3. KnowledgeKey small-vs-large namespace split

See §2. Small KnowledgeKey values resolve at PALOC `0x93` directly
(category names). Large values may use the hash-hop transform on
`Knowledge_*` row names. One-hour test once the bridge work starts.

### Q4. What is lo32 = 0x490 (1168)?

Saw 4 PALOC hits for `lo32 = 0x490` in the verification pass (e.g.
"Ambush" at hi32 ∈ {679530922, 934361767, 2965653965}). All values at
this namespace look mission-related but the exact meaning vs `0x101` is
unclear. Probably a sub-category of mission text (objective vs title?
description vs subtitle?). Worth a quick scan during the missioninfo
bridge work — if it's noisy, document and ignore; if it carries useful
text the editor wants, bridge in.

### Q5. The 6 unresolved QuestKey values with odd magnitudes

From the keycases handoff: 10001, 10011000, 11006000, 10029000,
10040400, etc. — magnitudes that don't fit the standard `1xxxxxx` range
of regular QuestKeys. May be event/special-quest IDs or test data.
Investigate during the questinfo schema RE.

---

## Suggested execution order

Picking by `(value × tractability) / risk`:

1. ~~**SkillKey**~~ — done
2. **`crimson_calculate_checksum` C ABI** — 30-minute task, extern "C"
   wrapper over the existing Rust function. Unblocks the editor side
   from doing hash hops without a Python tooling round trip.
3. **MissionKey via missioninfo bridge** (Option B from §3) — the big
   user-visible win. Schema RE + bridge + one-shot
   `lookup_display_name`. The hash transform is verified, so this is
   schema RE only.
4. **QuestKey via questinfo bridge** — small parallel work; same shape
   as missioninfo. Together with missioninfo unlocks the full
   "Quest/Mission" name column in the editor.
5. **StageKey hash test (Q2)** — half-session. Likely cracks, leading
   to a sixth bridge that handles 46k rows per save.
6. **KnowledgeKey** (Pattern A) — small parser + the namespace test
   from Q3.
7. **QuestGaugeKey** — small parser.
8. **FieldNPC + FieldGimmick** — investigation pays for both.
9. **SubLevelKey** — defer until concretely needed.

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
  lo32_namespace`**, verified end-to-end.
- ~~MissionKey/QuestKey ABI shape~~ — **Option B** (one-shot
  `_lookup_display_name(paloc_handle, key, lo32)` returning the
  resolved string in one FFI call).

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
