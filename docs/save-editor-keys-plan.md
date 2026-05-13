# Save Editor key resolvers — plan

The Save Editor surfaces integer keys that decode against game-data tables
and PALOC. This doc records the current state and the RE roadmap for each
key category — what's shipped, what's blocked on what, and the recommended
order to tackle the rest.

Companion to [`crimsonforge-feature-gaps.md`](./crimsonforge-feature-gaps.md);
that doc surveyed CrimsonForge in general, this one targets the specific keys
the Save Editor consumes.

Cross-referenced 2026-05-14 against the editor's own
`CrimsonAtomtic/docs/status.md` (lines 821–895 — forensic PALOC scan of
MissionKey/QuestKey/KnowledgeKey, plus "Key resolvers we still need —
C# consumption expectations" items #4–#6) and the live `QuestSaveData`
block structure from the editor's UI. ABI recommendations reflect the
editor side's stated preferences.

---

## Status snapshot

| Key | Source | Rust parser | C ABI bridge | Resolution pattern |
| --- | --- | --- | --- | --- |
| **SkillKey** | `skill.pabgb` + `.pabgh` | ✅ | ✅ `src/c_abi/skill_info.rs` | Pattern A — gamedata table |
| **KnowledgeKey** | `knowledgeinfo.pabgb` | ✗ | ✗ | Pattern A — gamedata table |
| **MissionKey** | PALOC 0xC1 reverse-index | n/a (PALOC bridge already exists) | ✗ | **Pattern B — PALOC reverse-index** |
| **QuestKey (titles)** | unknown u64-hash → PALOC | ✗ | ✗ | Pattern C — unresolved RE |
| **QuestKey (gameplay data)** | `questinfo.pabgb` | ✗ | ✗ | Pattern A — gamedata table (optional, not needed for names) |
| **QuestGaugeKey** | `questgaugeinfo.pabgb` | ✗ | ✗ | Pattern A — gamedata table |
| **StageKey** | `stageinfo.pabgb` (likely) | ✗ | ✗ | Pattern A *or* save-internal (see §5) |
| **FieldNPC CharacterKey** | unknown spawn table | ✗ | ✗ | Pattern A — gamedata table (TBD location) |
| **FieldGimmickSaveDataKey** | unknown — likely with FieldNPC | ✗ | ✗ | Pattern A *or* save-internal |
| **SubLevelKey** | unknown | ✗ | ✗ | TBD |

---

## Two resolution patterns

### Pattern A — gamedata-table bridge

```
SaveKey (u32 in save file)
   └─► <table>.pabgb row → internal name (BString ASCII)
                            └─► caller composes PALOC key
                                  └─► localized display name
```

This is the pattern SkillKey and iteminfo already use. One bridge per
`.pabgb` file, six-function surface (load_from_file / load_from_bytes /
free / entry_count / lookup_string_key / get_entry), zero-allocation
HashMap hits at runtime. Caller-side knows the PALOC prefix convention
(`"SkillName_"`, `"Knowledge_"`, …).

### Pattern B — PALOC reverse-index (mission/quest titles)

Empirically established by the editor team (status.md §821–895): mission
and quest **titles do not live in the gamedata `.pabgb` tables**. They
live inside PALOC entries at **type byte 0xC1**, with embedded
`{staticInfo:Mission:KEY}` / `{staticInfo:Quest:KEY}` template tokens
pointing back at the save-side key. Resolution requires a one-shot scan
of every 0xC1 entry to extract the tokens and build a reverse index:

```
PALOC scan once → for each 0xC1 entry: regex `{staticInfo:(\w+):(\d+)}`
                  → reverse_index[(type_name, u32_key)] = paloc_text
SaveKey (u32) + type_name ("Mission" | "Quest" | …)
   └─► reverse_index lookup → text fragment containing the title
                              (may need further template expansion if
                               the same text contains more tokens)
```

So mission/quest name resolution is **a PALOC concern, not a
mission_info/quest_info concern**. The mission/quest `.pabgb` parsers are
only needed if the editor wants gameplay data (HP, drops, levels) — for
names alone they are irrelevant.

### Pattern C — unresolved hash transform (full quest titles)

The editor team also found `"Where the Wind Guides You"` at PALOC
**u64 key** `15438629828055531777`. Upper 32 bits ≠ save-side
QuestKey `1000725`, so there's a hash/transform converting `QuestKey →
PALOC u64 key`. This is a separate path from Pattern B (which uses
embedded tokens) and **the transform has not been reverse-engineered**.
Until it is, this resolution path is blocked. Pattern B may cover the
common cases (titles that happen to also appear in 0xC1 entries) without
needing the hash. To be determined by sampling once the reverse-index
lands.

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
keys are knowledge *entries* and resolve elsewhere. The bridge should
expose both lookups distinctly so the editor doesn't conflate them:

```
KnowledgeEntry { key: u32, name: String, group_id: u32, is_category: bool }
```

**Path** (unchanged from before):
1. Extract `knowledgeinfo.pabgb` + `knowledgegroupinfo.pabgb` from group 0008.
2. Hexpat in `references/`. Build parser as `src/knowledge_info/`.
3. Bridge in `src/c_abi/knowledge_info.rs`, six-function surface plus
   `lookup_group_id` (knowledge → group join the editor wants).
4. Anti-pattern: do **not** ship CrimsonForge's regex scanner verbatim
   — crimson-rs's contract is byte-roundtrip; a heuristic that misses
   bytes is unfit for the modding pipeline.

**Size**: 1–2 sessions.

---

## 3. MissionKey / QuestKey titles — Pattern B (PALOC reverse-index)

### Architecture flip from earlier draft

The earlier version of this doc proposed parsing `missioninfo.pabgb` /
`questinfo.pabgb` and exposing `u32 key → name` lookups. That was wrong.
The editor team's forensic PALOC scan (status.md §821–895) found:

- `MissionKey 1003440` at PALOC 0x70 resolves to `"Hearty Braised Meat
  and Fish"` — the **item reward**, not the mission title. Missions reuse
  their reward item's numeric ID, so 0x70 hits are misleading.
- `QuestKey 1000725` at 0x30 / 0x70 resolves to the quest's **associated
  character or item**, never to a quest title.
- The editor side has **deliberately excluded** MissionKey/QuestKey/
  KnowledgeKey from `TypeNameToTypeByte` because showing the wrong name
  is worse than showing nothing.
- Real mission text fragments appear at **PALOC type byte 0xC1** with
  embedded `{staticInfo:Mission:KEY}` / `{staticInfo:Quest:KEY}` template
  tokens — these are the entries the resolver must scan.

So name resolution is a PALOC concern. The mission/quest `.pabgb`
parsers are decoupled from name display and remain optional (only
needed for HP / drops / level editing).

### ABI shape — PALOC reverse-index

New surface on the existing PALOC bridge:

```c
// Build the reverse index once after loading PALOC.
// Walks every entry whose type byte is 0xC1, regex-extracts
// {staticInfo:<TypeName>:<Key>} tokens, populates a
// HashMap<(TypeName, u32), &str> keyed on the embedding entry's text.
crimson_paloc_build_static_info_index(paloc_handle, &index_handle);

// type_name is a NUL-terminated UTF-8 string ("Mission", "Quest",
// "Character", "Item", ...). Two-call buffer pattern.
crimson_paloc_lookup_static_info(
    index_handle,
    const char* type_name,
    uint32_t    key,
    uint8_t*    buf,
    size_t      buf_len,
    size_t*     required) → i32;

crimson_paloc_static_info_index_free(index_handle);
```

The returned string is the **raw PALOC text** containing the
`{staticInfo:…}` token, including any other template tokens still
embedded in it. Template expansion of the *other* tokens (item names,
character names, formatting tags) is the editor's job — same convention
as Pattern A.

Memory cost: 0xC1 entries are a small subset of PALOC (~1–5%?); each
token extracts one (type_name, u32) → &str pointer. Should be sub-MB
on the full 1.06 PALOC.

### Path

1. Extract `localizationstring_eng.paloc` from group 0020 (already
   trivial — PAZ extract + existing PALOC bridge handles the parse).
2. Write a sampling script (`scripts/probe_paloc_static_info.py` or
   similar) that lists every 0xC1 entry and its `{staticInfo:…}` tokens.
   Confirms type-name coverage (Mission, Quest, Character, Item, …) and
   per-type entry counts.
3. Implement `crimson_paloc_build_static_info_index` in Rust. Token
   pattern is a tight regex: `r"\{staticInfo:([A-Za-z]+):(\d+)\}"`.
4. Add C ABI surface above. Reuse the existing two-call buffer pattern
   and `NOT_FOUND` / `BUFFER_TOO_SMALL` error codes.
5. Live-install test against 1.06 PALOC: pick a known mission with a
   resolvable title (status.md hints at sample IDs), assert non-empty
   string return.

**Size**: 1–2 sessions. The work lives entirely in `src/c_abi/paloc.rs`
+ `src/binary/paloc.rs` — no new parser modules.

**Open question — full quest titles**: Pattern B may not cover titles
that are stored as primary PALOC entries (the `"Where the Wind Guides
You"` case at u64 key `15438629828055531777`). Resolving those requires
either reverse-engineering the u32 → u64 hash transform (Pattern C) or
walking ALL PALOC entries and looking for `{staticInfo:Quest:KEY}` tokens
elsewhere. Defer until Pattern B's coverage gap is concretely measured
on a real save.

---

## 4. QuestGaugeKey — small (Pattern A)

**File**: `0008/.../gamedata/questgaugeinfo.pabgb`

**From the QuestSaveData screenshots**: 311 instances per save. Each
`QuestGaugeStateData` has `_key: QuestGaugeKey`, `_killRatio: float`,
`_stageList: ReflectObject`, `_factionOperationList: ReflectObject`,
`_state: QuestStateType`, `_deadCount_deprecated: uint16`. So gauges are
progress meters tied to quest objectives (kill counts, faction ops).

The editor doesn't currently surface gauge data, but the key is in
`QuestSaveData` at the top level so it WILL appear in the editor's
field grid. Resolution path:
- Bridge `questgaugeinfo.pabgb` (Pattern A, mirror of skill_info).
- Likely the row's name field IS resolvable directly (gauges aren't in
  the "intentional collision with iteminfo" trap that missions are).

**Size**: 1 session, low risk.

---

## 5. StageKey — investigate first (Pattern A *or* save-internal)

**From the screenshots**: **46541 instances** in `_stageStateData` — the
dominant data type in QuestSaveData. Each `StageStateData` has `_key:
StageKey`, `_state: QuestStateType`, plus optional fields like
`_delayedFromMissionKey`, `_delayedFromStageKey`, `_subTimelineName:
IndexedStringA`.

### Conflicting signals

**Editor side (status.md §865)** classifies StageKey as save-internal:
> "intra-save block references (similar to `StageKey`, `FactionNodeKey`,
> `FieldNPCSaveDataKey`, `FieldInfoKey`). Anything ending in
> `SaveDataKey` is generally an internal index, not a name reference;
> don't try to resolve them."

But the screenshot shows StageKey values like `1004305` — a 7-digit ID in
the same 100xxxx range PA uses for global gamedata IDs, not a sequential
save-block index (which would start at 0 or 1 and stay small).

**CrimsonForge side**: `character_asset_resolver.py:145` lists
`gamedata/stageinfo.pabgb` in its walk list of gamedata tables. So a
`stageinfo.pabgb` file **exists in gamedata** — meaning StageKey
*probably does* resolve to a name there.

The editor's "save-internal" classification may be premature — they
appear to have not tried `stageinfo.pabgb`. Worth a half-session of
investigation before deciding to ship or skip the bridge.

### Path

1. **Extract `gamedata/stageinfo.pabgb` from group 0008.** Use the PAZ
   bridge (already available). Confirm the file exists and its size.
2. **Hex-inspect for the screenshot's StageKey `1004305`.** If the byte
   sequence `0xB1 0x52 0x0F 0x00` (LE u32) appears in the file, StageKey
   resolves to a row in stageinfo. If not, the editor's "save-internal"
   call is correct.
3. **If it resolves**: schema RE + Pattern A bridge as
   `src/c_abi/stage_info.rs`. With 46541 instances this is high editor
   value — by far the most rows in QuestSaveData.
4. **If it doesn't resolve**: confirm the save-internal classification,
   update both this doc and the editor's status.md. No bridge work.

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
- `core/character_asset_resolver.py` (CrimsonForge) walks 19 `.pabgb`
  tables for character key references — that walk-list is the shortlist.

### ABI shape — editor's explicit preference (status.md item #5)

> *Recommended: two-output single call.*
> `crimson_<source>_lookup_character_key(handle, u32 spawnId, out u32 characterKey, out u32 stringInfoHash) → i32`.
> The two outputs let the C# caller resolve the display name in one of two
> ways: (a) trust the `stringInfoHash` directly via the stringinfo bridge, or
> (b) chain through a future `characterinfo` bridge using the `characterKey`.
> The editor side wants both options available without needing two FFI calls.

Combine FieldNPC + FieldGimmick under one bridge **if they share the
same source file** (extremely likely); otherwise ship as two bridges
with identical shape.

### Path

1. **Investigation pass** — list all `.pabgb` files in group 0008 and
   grep their decrypted bytes for a known FieldNPC key from a save file
   (the editor team has sample IDs from their probes: `117_440_514` aka
   `0x07000002` is one such field NPC instance). Whichever file contains
   it is the lookup table.
2. Once located, the schema RE follows the same recipe as quest/mission.

**Size**: 1 session for the investigation, then 2–3 for parser + bridge.

**Risk**: the resolution might not be a single lookup — spawn templates
sometimes reference an intermediate "spawn config" that then references
the character. Surface raw bytes early so the shape is visible before
committing to a parser design.

---

## 7. FieldGimmickSaveDataKey — pair with FieldNPC

**Editor finding (status.md §862)**: every harvested gimmick sample
(881022, 40052, 267612, 62768, …) returns no PALOC entry at any type
byte. Editor classifies as save-internal alongside StageKey.

But — same reasoning as §5 above. The editor checked PALOC, not the
gamedata `.pabgb` tables. May still resolve through a gamedata file.
Worth the same one-step investigation as StageKey.

Treat as a sibling of FieldNPC otherwise. When the FieldNPC investigation
finds its spawn table, scan the same file (and immediate neighbors) for
the gimmick keys.

**Size**: marginal cost ≈ 0.5 session on top of FieldNPC.

---

## 8. SubLevelKey — unknown, lowest priority

`sublevelinfo.pabgb` is referenced by CrimsonForge's
`localization_usage_index.py` under `CATEGORY_KNOWLEDGE`. Probably a
streaming chunk / region ID — relevant to save state and quest gating,
not surface-level UI.

**Path**: defer until the Save Editor concretely needs it. Same recipe
when prioritized.

**Size**: 1–2 sessions when prioritized.

---

## Open RE questions

These don't block the bridges listed above, but they limit how complete
the name display can ever be:

### Q1. The u32 → u64 quest-title hash transform

Status.md §886 found `"Where the Wind Guides You"` at PALOC key
`15438629828055531777` (u64). Upper 32 bits `3595124794` ≠ save QuestKey
`1000725`. Some hash/transform produces the u64.

Candidates to probe:
- Pearl Abyss's `hashlittle2` (Jenkins) — we already have it for PAMT
  checksums. Try `hashlittle2(questkey_bytes, 0, 0)` and compare upper 32.
- A salt-prefixed hash — e.g. `hashlittle2("Quest", questkey_bytes)`.
- A namespace prefix u32 concatenated with the key — e.g. upper32 =
  type_namespace_id, lower32 = something else.

First check: is the lower 32 of the u64 PALOC key equal to the
QuestKey itself? If yes, the transform is just
`(namespace_hash << 32) | questkey` — trivial to reverse and probe.
If no, the transform mixes the QuestKey bits and is non-trivial.

**Action**: one-hour probe. If trivial, ship a `crimson_quest_title_hash
(QuestKey) → u64` helper. If non-trivial, document and defer.

### Q2. StageKey resolution path

See §5. Either `gamedata/stageinfo.pabgb` resolves it (then it's a
Pattern A bridge), or it's truly save-internal (then it's not the
parser's job). One-shot investigation.

### Q3. KnowledgeKey small-vs-large namespace split

See §2. Need to confirm whether `knowledgeinfo.pabgb` rows cover both
namespaces or whether knowledge-categories live in a different file
(`knowledgegroupinfo.pabgb` is the most likely candidate).

---

## Suggested execution order

Picking by `(value × tractability) / risk`, with new findings folded in:

1. ~~**SkillKey**~~ — done
2. **MissionKey + QuestKey via PALOC reverse-index** (Pattern B) — small
   work, all in `src/c_abi/paloc.rs`, no new parser modules. Highest
   editor value per LOC.
3. **QuestGaugeKey** (Pattern A) — small parser, gauges are in
   QuestSaveData at the top level so the editor needs them.
4. **StageKey investigation** — half-session to confirm resolvability,
   then ship the bridge if it works. 46541 rows per save means
   high impact if Pattern A applies.
5. **KnowledgeKey** (Pattern A) — small parser, well-defined two-namespace
   shape from status.md.
6. **u32 → u64 hash probe (Q1)** — one-hour experiment. Either unlocks
   full quest titles or confirms it's deferred.
7. **FieldNPC + FieldGimmick** — investigation pays for both.
8. **SubLevelKey** — defer until concretely needed.

Compared to the previous version of this doc, MissionKey/QuestKey moved
from "medium, 3–5 sessions" to "small, 1–2 sessions" because the work is
now a PALOC extension, not a fresh parser. The lost
`mission_info`/`quest_info` parser work doesn't disappear — it's just
no longer on the critical path for name display; it stays available for
the "edit quest HP / mission rewards" use case if/when that's prioritized.

---

## Where these bridges plug in

The Save Editor calls each bridge once at startup (after PAZ-extracting
the relevant inputs) and holds the handles for the session. Per-row
lookups during editing are zero-allocation HashMap hits.

C# integration touchpoints (per `CrimsonAtomtic/docs/status.md` §675):

- `NativeSaveLoader.cs` `NativeMethods` — add `[LibraryImport]` declarations
- New `I<NewCatalog>Catalog.cs` + `Native<NewCatalog>Catalog.cs` in
  `src/CrimsonAtomtic.RustInterop/`
- `LocalizationProvider.cs` — `TryBootstrap<NewCatalog>` + `Resolve<Key>`
- `TypeNameToTypeByte` dispatch — add an entry for the new resolver

No public API breakage on existing surface.

---

## Decisions settled (recorded against editor-side prefs)

- ~~Template-resolver location~~ — **lives in Rust**, as Pattern B
  (PALOC reverse-index) — editor's stated preference.
- ~~Skill / Knowledge — one bridge or two~~ — **two bridges** (different
  source files).
- ~~FieldNPC ABI shape~~ — **two outputs in one call**
  (`out characterKey`, `out stringInfoHash`).
- ~~Mission/Quest titles via gamedata parser~~ — **no**, via PALOC
  reverse-index (Pattern B). The gamedata parsers become optional
  (gameplay-data editing only, not name resolution).

## Open user decisions

- Whether to invest in Q1 (u32→u64 hash probe) before or after Pattern B
  ships. Pattern B may cover enough cases that Q1 isn't worth the
  forensic effort.
- Whether StageKey's gameplay value justifies the bridge if §5
  investigation shows it's gamedata-resolvable. 46541 rows is a lot to
  surface in the editor — could be UX noise as much as value.

## Vendor flow

`CrimsonAtomtic/vendor/update_vendors.ps1` does `git reset --hard origin/dev`
on `vendor/crimson-rs`, so pushes here flow into the editor on the next
vendor refresh — **no PR coordination needed beyond keeping `dev`
green**. CI gates on `main` (the `clippy + cargo test` required check)
protect the merge path.
