# Save Editor key resolvers — plan

The Save Editor surfaces five categories of integer key that decode against the
game's gamedata tables. This doc records the current state and the RE roadmap
for each — what's shipped, what's blocked on what, and the recommended order
to tackle the rest.

Companion to [`crimsonforge-feature-gaps.md`](./crimsonforge-feature-gaps.md);
that doc surveyed CrimsonForge in general, this one targets the specific keys
the Save Editor consumes.

Cross-referenced 2026-05-14 against the editor's own
`CrimsonAtomtic/docs/status.md` ("Key resolvers we still need — C#
consumption expectations" section, items #4–#6 in the deferred list). The
editor side has already enumerated the C# integration touchpoints
(`NativeMethods` block in `NativeSaveLoader.cs`, new `I<Catalog>Catalog.cs`
+ `Native<Catalog>Catalog.cs` files, `LocalizationProvider.Resolve<Key>`
methods, `TypeNameToTypeByte` dispatch), so the only non-trivial choice
for each upstream PR is the ABI shape itself. ABI recommendations below
reflect their stated preferences.

---

## Status snapshot

| Key | Source file(s) | Rust parser | C ABI bridge | Notes |
| --- | --- | --- | --- | --- |
| **SkillKey** | `skill.pabgb` + `skill.pabgh` | ✅ `src/skill_info/` (byte-roundtrip across 1.03/1.04/1.05) | ✅ `src/c_abi/skill_info.rs` | Two-hop: bridge gives `key → entry name`, caller paloc-looks-up `"SkillName_<name>"` |
| **KnowledgeKey** | `knowledgeinfo.pabgb` (+ `knowledgegroupinfo.pabgb`) | ✗ | ✗ | CrimsonForge has only a heuristic regex scanner for `Knowledge_<name>\x00`, not a schema parser. |
| **MissionKey / QuestKey** | `questinfo.pabgb` + `missioninfo.pabgb` (+ `questgroupinfo`, `questgaugeinfo`, `wantedinfo`) | ✗ | ✗ | Confirmed present in 0008. Schema unknown. `{StaticInfo:Mission:...}` / `{StaticInfo:Quest:...}` template-resolver also needed. |
| **FieldNPC CharacterKey → real CharacterKey** | unknown — likely `charactertemplate*.pabgb` or a spawn table in `gamedata/` | ✗ | ✗ | The "FieldNPC key" is a spawn-template ID, not the underlying character. The lookup table has not been located yet. |
| **FieldGimmickSaveDataKey** | unknown — likely co-located with the FieldNPC spawn table | ✗ | ✗ | Treat as a sibling of FieldNPC — same investigation pass. |
| **SubLevelKey** | unknown — likely world/streaming-level data | ✗ | ✗ | `localization_usage_index.py` tags `sublevelinfo` under `CATEGORY_KNOWLEDGE`. Schema unknown. |

---

## Pattern the bridges mirror

All five keys flow through the same two-hop lookup that
`iteminfo` and `skill_info` already use:

```
SaveKey (u32 in save file)
   └─► <table>.pabgb row
           └─► internal name (BString-shape ASCII id)
                   └─► PALOC key (caller composes prefix)
                           └─► localized display name
```

The bridge's job is the middle hop only: `u32 key → &str internal_name`.
PAZ extraction (already shipped) feeds the bridge; PALOC lookup (already
shipped) consumes its output. The caller in the editor owns the prefix
convention (`"SkillName_"`, `"MissionTitle_"`, `"Knowledge_"`, …) because
the conventions differ per table.

This shape means **each new key category is the same five-step recipe**:

1. Extract the relevant `.pabgb` (and `.pabgh` if paired) from the live install via PAZ
2. Reverse the row schema (hexpat in `references/`, validate with `plcli`)
3. Land a Rust parser in `src/<table>_info/` (byte-roundtrip required — same bar as item/skill)
4. Add a `src/c_abi/<table>_info.rs` bridge mirroring `iteminfo.rs` / `skill_info.rs`
5. Smoke test against the live install, then commit

The hard step is (2); steps (1), (3–5) are mechanical once the schema is known.

---

## 1. SkillKey — DONE

Shipped in `src/c_abi/skill_info.rs`. Surface:

```c
crimson_skillinfo_load_from_file(pabgh_path, pabgb_path, &handle);
crimson_skillinfo_load_from_bytes(pabgh_bytes, pabgb_bytes, &handle);
crimson_skillinfo_entry_count(handle, &count);
crimson_skillinfo_lookup_string_key(handle, skill_key, buf, len, &required);
crimson_skillinfo_get_entry(handle, idx, &key, buf, len, &required);
crimson_skillinfo_free(handle);
```

Live install on 1.06 reports ~280 skills. Coverage: live roundtrip,
garbage bytes → `BODY_PARSE`, null arg matrix, bad path → `IO`.

The underlying parser in `src/skill_info/` was already byte-roundtripping
across 1.03 / 1.04 / 1.05 — the bridge just exposes the `(key, name)` pair
the editor needs and drops the rest of the entry graph.

---

## 2. KnowledgeKey — small, RE work needed first

**File**: `0008/.../gamedata/knowledgeinfo.pabgb` (+ `knowledgegroupinfo.pabgb`)

**Editor-side correction**: `CrimsonAtomtic/docs/status.md` item #4 groups
SkillKey and KnowledgeKey under one "`skill_info` bridge" line, expecting
the same parser to resolve both. It can't — they live in different
`.pabgb` files. KnowledgeKey is a **separate bridge** with its own parser
(`src/knowledge_info/` + `src/c_abi/knowledge_info.rs`). The editor side
will then need a `LocalizationProvider.ResolveKnowledgeKey(uint)` that's
distinct from `ResolveSkillKey`.

**Why it's not "just mirror iteminfo"**: CrimsonForge does **not** have a
schema parser for knowledgeinfo. The Python side (`translation/localization_usage_index.py`
`_tag_knowledgeinfo`) uses a regex scanner: find every `Knowledge_<name>\x00`
ASCII run in the buffer, walk backwards 8 bytes for the BString length
prefix, look for the `\x01\x01\x00\x73\xe1\xc5\xea` group marker 15 bytes
in to read the group_id u32. That's enough for "which loc keys does this
knowledge use" but it's not a row schema and it definitely doesn't roundtrip.

**Recommended path**:
1. Extract `knowledgeinfo.pabgb` from group 0008 and write a hexpat in
   `references/knowledge_info.hexpat`. Use the same workflow as
   `references/item_info.hexpat`: write a partial struct, run
   `plcli run -i knowledgeinfo.pabgb -p knowledge_info.hexpat -v -d`,
   iterate.
2. Build the parser as `src/knowledge_info/` with the same shape as
   `src/item_info/` — module with `keys.rs`, `structs.rs`, `mod.rs`. Required
   surface: `KnowledgeEntry { key: u32, name: String, group_id: u32 }`.
3. Bridge as `src/c_abi/knowledge_info.rs`. Same 6-function surface as
   skillinfo. Bonus: expose `lookup_group_id` since knowledge → group is the
   primary join the editor wants.
4. Live test against 1.06 install.

**Size estimate**: 1–2 sessions. The schema is reportedly simpler than
item or skill (group records appear to be flat name + group_id + loc_keys),
and CrimsonForge's heuristic gives a strong reference signal for what
the entries should contain.

**Anti-pattern to avoid**: don't ship CrimsonForge's regex scanner verbatim.
crimson-rs's contract is byte-roundtrip; a heuristic that misses bytes is
unfit for the modding pipeline that consumes this crate.

---

## 3. MissionKey / QuestKey — medium, schema + template resolver

**Files** (all in `0008/.../gamedata/`):
- `questinfo.pabgb` — primary
- `missioninfo.pabgb` — primary
- `questgroupinfo.pabgb` — grouping
- `questgaugeinfo.pabgb` — progress meters
- `wantedinfo.pabgb` — bounty-style sub-flavor

**Two unknowns**:

1. **Row schema** — none of these have been parsed. The generic heuristic
   in `core/pabgb_parser.py` (CrimsonForge) reads them as "simple" or
   "hashed" tables but doesn't claim semantic field names.
2. **`{StaticInfo:Mission:...}` / `{StaticInfo:Quest:...}` template tokens**
   embedded inside PALOC dialogue strings — these are cross-references to
   quest/mission rows by integer ID. The editor wants to render those
   inline ("[Mission: Prologue]"), so a **template-resolver** layer is
   needed on top of the row-lookup bridge.

**ABI shape — editor's explicit preference (status.md item #6)**:

> *Recommended: shape A (Rust expands templates).*
> `crimson_mission_info_lookup_display_name(mission_handle, paloc_handle, u32 key, byte* buf, …) → i32`.
> Rust gets a paloc handle alongside its own, does the `{StaticInfo:Mission:KEY}` walk
> internally, returns a **fully-resolved localized string**. C# stays simple.
>
> The editor side rejected the segmented-output alternative ("template syntax
> knowledge belongs in the parser, not spread across the FFI"). So the bridge
> needs to take a paloc handle as a second argument — not just expose a raw
> row lookup.

**Recommended path** (revised against editor preference):

1. Extract all five `.pabgb` files. Save under `out/baselines/1.06/` (gitignored).
2. Confirm `tools/patch_quest_hp.py` (CrimsonForge) — it already located an
   HP field at offset `0x226154` in row 0 of `questinfo.pabgb` for the
   Ogre quest. That row offset is a strong starting point for schema RE:
   work outward from a known byte to infer the row size, then the row count.
3. Hexpat pass for each table. Land them as `src/quest_info/`, `src/mission_info/`.
4. **Template resolver lives in Rust, not C#**. The shape A choice changes
   where this lives compared to the earlier draft of this doc. Implement
   `src/c_abi/template.rs` exposing a `resolve_template_string(paloc_handle,
   handles…, &str) -> String` that walks `{StaticInfo:Mission:KEY}` /
   `{StaticInfo:Quest:KEY}` / `{StaticInfo:Character:KEY}` etc. tokens and
   substitutes resolved localized strings. The mission / quest bridges
   call into this from their `_lookup_display_name` getters.
5. Bridges per table. The primary surface is now `_lookup_display_name`
   (fully-resolved string) plus the bare `_lookup_string_key` (raw row name)
   for debugging / non-templated callers. Five tables total — `_display_name`
   is the editor-facing API; `_string_key` is the debug API.

**Size estimate**: 3–5 sessions. Most of the time is the schema RE.

**Open question**: which of the five is primary for the Save Editor?
Likely `questinfo` + `missioninfo`. The other three can wait if a
prioritization signal comes from the editor side.

---

## 4. FieldNPC CharacterKey → real CharacterKey

The "FieldNPC key" stored in a save isn't a `CD_M0001_00_Ogre`-style
character ID — it's a **spawn template ID** that resolves through a
separate lookup to the underlying character. The save calls "the wandering
merchant at coord X" by template, not by character.

**State of knowledge**:
- The lookup table has **not been located**. Candidates worth probing in
  order:
  - `charactertemplate.pabgb` / `charactertemplateinfo.pabgb` if it exists
  - `npcspawn.pabgb` / `fieldnpc.pabgb` / `fieldspawn.pabgb`
  - The world-level files (sublevels, region info) — spawn tables sometimes
    live with the level rather than with the character data
- `core/character_asset_resolver.py` (CrimsonForge) walks 19 `.pabgb`
  tables looking for character key references. That walk-list is the
  shortlist to dump.

**ABI shape — editor's explicit preference (status.md item #5)**:

> *Recommended: two-output single call.*
> `crimson_<source>_lookup_character_key(handle, u32 spawnId, out u32 characterKey, out u32 stringInfoHash) → i32`.
> The two outputs let the C# caller resolve the display name in one of two
> ways: (a) trust the `stringInfoHash` directly via the stringinfo bridge, or
> (b) chain through a future `characterinfo` bridge using the `characterKey`.
> The editor side wants both options available without needing two FFI calls.
>
> Combine FieldNPC + FieldGimmick under one bridge **if they share the same
> source file** (extremely likely); otherwise ship as two bridges with
> identical shape.

**Recommended path**:

1. **Investigation pass** — list all `.pabgb` files in group 0008 and
   grep their decrypted bytes for a known FieldNPC key from a save file.
   Whichever file contains it is the lookup table. This is a half-day of
   pure detective work, but it's binary search through ~30 files, not
   open-ended.
2. Once located, the schema RE follows the same recipe as quest/mission.

**Size estimate**: 1 session for the investigation, then 2–3 for parser +
bridge — assuming the table is reasonably structured. If it turns out
to be a 100-field monster like `iteminfo`, double that.

**Risk**: the resolution might not be a single lookup — spawn templates
sometimes reference an intermediate "spawn config" that then references
the character. Plan to surface raw bytes early so the shape is visible
before committing to a parser design.

---

## 5. FieldGimmickSaveDataKey — pair with FieldNPC

Treat as a sibling of FieldNPC. The "gimmick" naming in Pearl Abyss code
refers to interactable world objects (doors, chests, levers, savable
state machines). They follow the same spawn-template pattern as NPCs and
are likely co-located with the FieldNPC spawn data.

**Recommended path**: piggyback on the FieldNPC investigation. When step
(1) above finds the FieldNPC table, scan the same file (and immediate
neighbors) for the gimmick keys from a save. Worst case it's a separate
file with the same shape — at which point the parser falls out cheaply.

**Size estimate**: marginal cost ≈ 0.5 session on top of FieldNPC.

---

## 6. SubLevelKey — unknown, lowest priority

`sublevelinfo.pabgb` is referenced by CrimsonForge's
`localization_usage_index.py` (tagged under `CATEGORY_KNOWLEDGE`), but
nothing else in either repo touches it. The key is presumably a streaming
chunk / region ID — relevant to save state (where the player is) and to
quest gating, but probably not surface-level UI.

**Recommended path**: defer until the Save Editor concretely needs it.
When it does, the recipe is the same: extract, hexpat, parser, bridge.

**Size estimate**: 1–2 sessions when prioritized, but the lack of any
known consumer makes it the right one to leave for last.

---

## Suggested execution order

Picking by `(value × tractability) / risk`:

1. ~~**SkillKey**~~ — done
2. **KnowledgeKey** — small parser, well-scoped reference behavior in CrimsonForge to validate against
3. **QuestKey** (just `questinfo.pabgb`) — known landmark (Ogre HP at `0x226154`), high editor value
4. **MissionKey** (`missioninfo.pabgb`) — probable schema cousin of quest
5. **FieldNPC + FieldGimmick** — same investigation pays for both
6. **SubLevelKey** — defer

Quest is ahead of mission because the `patch_quest_hp.py` landmark cuts the RE entry cost.

---

## Where these bridges plug in

The Save Editor calls each bridge once at startup (after PAZ-extracting
the relevant `.pabgb`s) and holds the handles for the session. Per-row
lookups during editing are zero-allocation HashMap hits, exactly like
the existing iteminfo bridge.

For the C# / Avalonia side: each bridge is a `*Handle`, an
`Open(bytes...)`, a `Lookup(u32) -> string?`, and a `Free()`. The same
six-function pattern that's already in production for iteminfo.

---

## Decision points the user owns

- Whether KnowledgeKey or QuestKey goes first (both are roughly tied on
  effort; QuestKey has higher Save Editor surface impact, KnowledgeKey has
  a known reference implementation).
- Whether to ship `mission_info` separately from `quest_info` or merge into
  one parser module (their shape might be near-identical — wait until the
  hexpat pass to decide).

## Decisions settled (recorded against editor-side prefs)

- ~~Template-resolver location~~ — **lives in Rust**. The editor's
  status.md explicitly prefers shape A: pass a paloc handle through the
  mission/quest bridge, do template expansion inside the parser, return
  a fully-resolved localized string across the FFI.
- ~~Skill / Knowledge — one bridge or two~~ — **two bridges**. They
  read different `.pabgb` files; the editor's assumption that one parser
  covers both is incorrect.
- ~~FieldNPC ABI shape~~ — **two outputs in one call**
  (`out characterKey`, `out stringInfoHash`), per editor preference.

## Vendor flow

`CrimsonAtomtic/vendor/update_vendors.ps1` does `git reset --hard origin/dev`
on `vendor/crimson-rs`, so any push to this repo's `dev` flows into the
editor on the next vendor refresh — **no PR coordination needed beyond
keeping `dev` green**. CI gates on `main` (the `clippy + cargo test`
required check) protect the merge path; the editor consumes `dev`
directly.
