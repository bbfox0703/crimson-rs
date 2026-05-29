# Save Editor key resolvers — reference / regression baseline

Companion to [`save-editor-keys-plan.md`](./save-editor-keys-plan.md).
The plan doc captures *what we built* and *why*; this doc captures
*what we built against* — the ground-truth values, user-supplied
reference material, and verified key mappings from game version
**1.06** that the shipped bridges are pinned to.

If a future game patch breaks key resolution (silent hash drift,
namespace byte rename, schema shift in a `.pabgb`), this file is the
comparison set. Re-run the bridge tests against the new version; any
mismatch against the tables below tells you exactly which layer
regressed.

The bridge tests under `src/c_abi/*_info.rs` already encode the same
fixtures as live `#[test]` assertions — this doc is the human-readable
mirror plus the wider context the tests don't carry (user-supplied
wiki excerpts, full quest hierarchies, the PALOC namespace tableau).

---

## How to use this for regression checks

When `cargo test --lib --features c_abi` starts failing after a game
patch:

1. **Localize the layer first.** Each bridge has its own test, and
   each test has its own fixture table. A failing
   `c_abi_missioninfo_live_full_chain` doesn't necessarily mean
   missioninfo is broken — it might mean PALOC drifted at the
   `lo32 = 0x101` namespace, or the hash function changed, or the
   missioninfo row schema's header layout shifted. Cross-check
   against the **PALOC namespace map** below first.

2. **Probe with the verified `(internal_name, hi32)` pairs** in
   `src/c_abi/checksum.rs::tests::KNOWN`. If those still hash to
   the recorded values, the Jenkins variant hasn't changed. If they
   don't, every downstream bridge is broken until the hash is
   re-anchored.

3. **For each broken fixture, check three things in order**:
   - Does the internal name still exist in the `.pabgb` file? (grep
     the extracted file for the ASCII name)
   - Does `hashlittle2_c(name)` still produce the recorded hi32?
   - Does PALOC still have an entry at
     `(hi32 << 32) | recorded_lo32`?

4. **Update the fixtures + this doc together.** The doc is the
   source of truth for ground-truth knowledge; the tests are the
   automation of that truth. If Pearl Abyss renames an internal
   name (`Mission_Intro_Tutorial_I` → `Mission_Prologue_FirstSteps`,
   say), update both.

5. **User-supplied ground truth is the ultimate anchor.** The
   English UI strings the user typed in (see "Main Quest titles"
   below) are what a player actually sees in 1.06. If those text
   strings vanish from PALOC, the patch has done something more
   serious than a renamespace and you need to dig.

---

## PALOC namespace map (1.06, English)

`lo32` is the low 32 bits of a PALOC numeric key; `hi32` is either a
Jenkins `hashlittle2_c(name)` output or a literal integer ID
depending on the namespace.

| lo32 | hex | hi32 source | What it carries | Bridges that use it |
| ---:| ---:| --- | --- | --- |
| 48 | `0x30` | hash | Character / faction display name | (iteminfo bridge composes for `_characterKey`) |
| 0 | `0x00` | hash | In-world gimmick / scenery (Grindstone, Anvil, Painting Fragment) | (none yet — editor handles via `LocalizationProvider`) |
| 112 | `0x70` | hash | Item name | iteminfo |
| 146 | `0x92` | **integer (group_id)** | Knowledge group rollup name ("Creatures", "Bosses", "Mounts") | not bridged; would be `knowledge_group_info` |
| 147 | `0x93` | integer (group_id) | Knowledge group alternate text | same |
| 256 | `0x100` | hash | Quest / arc / region / chapter heading | quest_info (default) |
| 257 | `0x101` | hash | Mission title, stage title, sometimes alt-quest text | mission_info (default), stage_info (default), quest_info (secondary) |
| 258 | `0x102` | hash | Stage / shop description (longer text) | stage_info (description) |
| 1168 | `0x490` | hash | Knowledge entry title | knowledge_info (default), also some mission text variant |
| 1169 | `0x491` | hash | Knowledge entry short description | knowledge_info (description) |
| 1181 | `0x49D` | hash | Knowledge alternate title (~70% of rows) | knowledge_info (variant) |
| 1182 | `0x49E` | hash | Knowledge alternate description | knowledge_info (variant) |
| 1183 | `0x49F` | hash | Knowledge description alt | knowledge_info (variant) |
| 2192 | `0x890` | hash | Knowledge paginated title (~1400 rows) | knowledge_info (variant) |
| 2193 | `0x891` | hash | Knowledge paginated description | same |
| 2207 | `0x89F` | hash | Knowledge paginated description alt | same |
| 193 | `0xC1` | hash | Mission text fragment with embedded `{staticInfo:Mission:KEY}` template tokens | not bridged (no consumer needs this yet) |
| 512 | `0x200` | hash | Item / gatherable secondary text ("Abyss Cell" at multiple keys) | not bridged |
| 400 | `0x190` | hash | Character "title" / role variant (Kliff at 0x190 / 0x191) | not bridged |

**Important distinctions**:

- The `hash` namespaces use **Jenkins hashlittle2** (Pearl Abyss
  variant with `init = 0xDEBA1DCD`), implemented in
  `crate::crypto::checksum::calculate_checksum` and exposed to the C
  ABI as `crimson_calculate_checksum`.
- The `integer (group_id)` namespaces at `0x92 / 0x93` are
  **literal integers**, not hashes. `Creatures` = `(4 << 32) | 0x92`,
  not `(hash("Creatures") << 32) | 0x92`. These are the only
  non-hash namespaces observed.

---

## User-supplied ground truth — Main Quest titles (1.06, English UI)

Source: user typed these from in-game UI during the 2026-05-14 session.
This is the **most authoritative anchor** — these are strings a real
player actually sees on screen. Format reflects the in-game
hierarchical menu (`# Chapter / ## Sub-arc / quest title`).

```
# Prologue: Dead of Night
## Ambush
Unfamiliar Lands
In Ashes
## Unknown Space
Realm of Uncertainty
New Journey

# Chapter 1: The First Encounter
## Trials of Kindness
Where Rumors Gather
Mysterious Man
True Wisdom in Kindness
Actions Speak Louder than Words
Heart Beyond Borders

## Trace
Mystical Key
Polar Opposites
Abyss Without Balance
Woman in White

# Chapter 2: Golden Greed
## Unexpected Gift
Where the Light Leads
Memory Fragment
Reunion

## Hernand in Chaos
For Honor
Awestruck
Shadow Cast Over the River
Where Misery Gathers
Trial After Trial
The Man Trapped in the Mire
Missing Companion
Secrets Hidden in the Dark

## The End of Greed
The Dark Veil
The Flames of Greed
Kidnapped Healer
Rebellion or Revolution
Cheers Echoing From the Edge

# Chapter 3: Howling Hill
## Homestead
Old Friend
First Step to Rebuilding
A Fresh Start
Reward for Their Sweat
Return of the Comrade
Familiar Curses

## The Face Behind the Mask
Return
Traces in the Manor
Nonhuman
Seed of Unease
Dance with the Devil

## Pioneering
Hope After the Draught
Scattered Comrades
Rumors from the Sawmill
A Gentle Touch
Bustling Hill
Greymanes Reunited
```

### How the bridges resolve these

- **Chapter headings** (`Prologue: Dead of Night`, `Chapter 1: The
  First Encounter`, etc.) — not yet verified to a specific bridge
  surface; presumed to be in PALOC at a chapter-rollup namespace
  but not located during the session.
- **Sub-arc headings** (`Ambush`, `Trials of Kindness`,
  `Unknown Space`, `Trace`, `Unexpected Gift`, `Hernand in Chaos`,
  `The End of Greed`, `Homestead`, `The Face Behind the Mask`,
  `Pioneering`) — resolve via `quest_info` bridge at `lo32 = 0x100`
  (arc-heading namespace). Verified during the hash-transform
  cracking probe — 673 hits at this namespace included all
  recognized arc names.
- **Individual quest titles** (`Unfamiliar Lands`, `In Ashes`,
  `Realm of Uncertainty`, etc.) — resolve via `mission_info` bridge
  at `lo32 = 0x101`. Seven entries verified end-to-end (table
  below).

---

## User-supplied ground truth — Wiki knowledge entries

Source: user pasted these from a web reference during the 2026-05-14
KnowledgeKey session. Format: `Category > Subcategory >
Subsubcategory > Leaf entry`.

```
Creatures > Terrestrial Creatures > Amphibians > Burrowing Salamander
People > Crimson Desert > Equipment Vendors > Brookner
People > Crimson Desert > Outlaws > Sigrun
Playable Characters > Kliff
People > The Greymanes > Combat Provisioners > Camp Tailor - Diederik
Bosses > Demeniss > Stonewalker Antiquum
Gatherables > Crafting Materials > Artifacts > Abyss Cell
Mounts > Loyal Companions > Horses > Herspia
```

### How the bridges resolve these

The leaf names are **not directly resolvable through the
`knowledge_info` bridge** — they're in PALOC under the character
namespace (`lo32 = 0x30`) and item namespace (`lo32 = 0x70`), not
the knowledge namespace. Specifically (PALOC values at each
candidate `(hi32, lo32)`):

| Leaf entry | PALOC hits |
| --- | --- |
| Burrowing Salamander | hi32=32428 lo32=0x30 (character); hi32=1001865 lo32=0x70 (item); hi32=1412938068 lo32=0x490 (knowledge) |
| Brookner | hi32=1000930 lo32=0x30 (character); hi32=1700777658 lo32=0x490 (knowledge) |
| Sigrun | hi32=1001922 lo32=0x30 (character); hi32=2125291524 lo32=0x490 (knowledge) |
| Kliff | hi32=400 lo32=0x30 (character); hi32=5114 lo32=0x190 / 0x191 (title variant) |
| Stonewalker Antiquum | hi32=57216 lo32=0x30 (character/boss); hi32=1618801302 lo32=0x490 (knowledge); hi32=2342110437 lo32=0x490 |
| Abyss Cell | hi32=1002063 lo32=0x70 (item); multiple at lo32=0x200 |
| Herspia | hi32=2000/2021/2022/2023 lo32=0x30 (multiple character variants — Herspia is a horse type) |
| Camp Tailor - Diederik | NOT FOUND as a single PALOC value (composite "<role> - <name>" string assembled by the UI) |

**What this tells future-session debuggers**: the wiki's
breadcrumb hierarchy is a UX presentation, NOT a single PALOC
lookup. The leaf string comes from one namespace (character / item),
the categories from another (`knowledge_group_info` at
`lo32 = 0x92/0x93` with integer group IDs). Surfacing the wiki-style
breadcrumb in the editor would require:

1. A `knowledge_group_info` bridge over `knowledgegroupinfo.pabgb`
2. Parsing each `knowledgeinfo` row's body for its `group_id` ref
3. Walking the group hierarchy to assemble the breadcrumb

Out of scope for the shipped bridges. Recorded here so the next
session knows what the user was asking for and why the shipped
surface stops at the leaf.

### Knowledge group IDs (1.06)

`lo32 = 0x92` (146), `hi32 = literal integer`:

| Group ID | Display name | Likely parent |
| ---:| --- | --- |
| 2 | People | — (top-level) |
| 4 | Creatures | — (top-level) |
| 5 | Gatherables | — (top-level) |
| 11 | Bosses | — (top-level; also has 0x93 entry) |
| 15 | Mounts | — (top-level) |
| 57 | Crafting Materials | (Gatherables?) |
| 104 | Terrestrial Creatures | Creatures |
| 151 | Loyal Companions | Mounts |
| 187 | Bosses (variant) | Bosses |
| 201 | The Greymanes | People (faction subgroup) |
| 570 | Artifacts | Crafting Materials (also 0x93) |
| 1045 | Amphibians | Terrestrial Creatures |
| 1510 | Horses | Loyal Companions |
| 1704 | Combat Provisioners | The Greymanes |
| 1711 | Equipment Vendors | People (variant 1) |
| 1716 | Outlaws | People (variant 1) |
| 1731 | Equipment Vendors | People (variant 2) |
| 1736 | Outlaws | People (variant 2) |
| 2041 | Equipment Vendors | People (variant 3) |
| 2046 | Outlaws | People (variant 3) |

(Parent inference is heuristic — the actual parent-child link lives
in `knowledgegroupinfo.pabgb` row bodies. The triplicate "Equipment
Vendors" / "Outlaws" IDs suggest faction-scoped variants per region.)

---

## Verified key → display title mappings (1.06)

Each row was traced through the full chain (save-side key →
`.pabgb` row → internal name → `hashlittle2` → PALOC u64 key →
localized string) and asserted in `cargo test --lib --features
c_abi`. If any of these mismatches in a future game version, the
relevant bridge has regressed.

### MissionKey (via `mission_info` bridge, `lo32 = 0x101`)

| MissionKey | Internal name | Display title | save context |
| ---:| --- | --- | --- |
| 1000157 | `Mission_Intro_Tutorial_I` | "Unfamiliar Lands" | first prologue quest |
| 1000160 | `Mission_Intro_MainBattle` | "In Ashes" | second prologue |
| 1000620 | `Mission_Intro_Abyss_Tutorial` | "Realm of Uncertainty" | abyss tutorial |
| 1000164 | `Mission_Intro_After_Horse` | "New Journey" | post-horse-acquisition |
| 1000052 | `Mission_MeetAlustain_Alustain_Strength` | "Where Rumors Gather" | Alustain meet, Chapter 1 |
| 1000053 | `Mission_MeetAlustain_Alustain_Wisdom` | "Mysterious Man" | Alustain meet, Chapter 1 |
| 1000083 | `Mission_IronStronghold_Block_ReturnToSister` | "Where the Wind Guides You" | iron stronghold quest (Hernand chapter) |

Verified completedTime monotonicity: prologue quests at ~97k–307k
in-game ticks, "Where the Wind Guides You" at ~7.1M — matches
in-game story order.

### QuestKey (via `quest_info` bridge, `lo32 = 0x100` for arcs)

| QuestKey | Internal name | lo32 | Display |
| ---:| --- | ---:| --- |
| 1000619 | `Quest_Node_Her_RootFort_Normal` | 0x100 | "Roothold" |
| 1000881 | `Quest_Node_Her_GreymaneCamp_Contents` | 0x100 | "Record of the Greymanes" |
| 1001032 | `Quest_HumanDocumentary_Del` | 0x100 | "Human Documentaries" |
| 1000039 | `Challenge_Maze` | 0x100 | "Mazes" |
| 1000039 | `Challenge_Maze` | 0x101 | "Winding Paths" *(same row, secondary text)* |
| 1000180 | `Quest_BloodCoronation_WitchDukeAndDream` | 0x100 | "Traitor" |

The `Challenge_Maze` dual-namespace pair pins `lo32` as load-bearing
in the bridge API — verifies the caller's namespace argument
actually changes the resolved string.

### StageKey (via `stage_info` bridge, `lo32 = 0x101` for titles)

| StageKey | Internal name | lo32 | Display |
| ---:| --- | ---:| --- |
| 1004305 | `Intro_Tutorial_Miseenscene_00` | 0x101 | "Mise-en-scene Before Intro Combat" |
| 1000001 | `DelesyiaCastle_Herbert_BlueStone` | 0x101 | "Herbert's Request (Azurite)" |
| 1000002 | `Varnia_UrdavahResearch_RedStone` | 0x101 | "Gatherables Placement Stage (Bloodstone)" |
| 1001566 | `AnvilHill_Block_Patrol_I` | 0x101 | "Goblin Bandits Patrol" |
| 1012833 | `Shop_Demeniss_Faction_Elemore_Grocery` | 0x101 | "Grocer's Shop" |
| 1012833 | `Shop_Demeniss_Faction_Elemore_Grocery` | 0x102 | "A grocer's shop run by Gromin, who has ill ties with the Inquisitors. Fresh local groceries of the area around Eastern Court can be purchased here." *(same row, description)* |

`StageKey 1004305` is the value shown in the editor's UI screenshot
for `_stageStateData[0]._key` — anchors this fixture to the
user's observed save state, not just any randomly-picked stage.

### KnowledgeKey (via `knowledge_info` bridge, `lo32 = 0x490` for titles)

| KnowledgeKey | Internal name | lo32 | Display |
| ---:| --- | ---:| --- |
| 1002588 | `Knowledge_Node_Dem_Ruins_0007` | 0x490 | "Demenissian Ruins" |
| 1002588 | `Knowledge_Node_Dem_Ruins_0007` | 0x491 | "Ruins of the continent of Pywel." *(same row, description)* |
| 1002294 | `Knowledge_Node_Dem_HiddenCave` | 0x490 | "Hidden Cave" |
| 1002763 | `Knowledge_AbyssRuins_Dem_0020` | 0x490 | "Abyss Nexus" |
| 1004037 | `Knowledge_Demian_Plate_Boots_V` | 0x490 | "Executioner of Darkness Plate Boots" |

### QuestGaugeKey (via `quest_gauge_info` bridge, **no PALOC chain**)

QuestGauge rows resolve only to internal names — no PALOC entries
at any namespace. Editor surfaces the internal name directly.

| QuestGaugeKey | Internal name |
| ---:| --- |
| 1000083 | `QuestGauge_WindCliffFort` |
| 1000084 | `QuestGauge_AbandonedWyvernNest` |
| 1000085 | `QuestGauge_DesolateWyvernNest` |
| 1000086 | `QuestGauge_ForgottenWyvernNest` |
| 1000091 | `QuestGauge_FortManub` |

QuestGaugeKey 1000083 is the editor screenshot's
`_questGaugeStateList[0]._key` value.

---

## Verified hash anchors (1.06)

These pin the **Jenkins hashlittle2** function itself — if these
hi32 values drift, the hash algorithm or its init constant has
changed.

| Internal name | Expected `hashlittle2_c` (hi32) | Hex |
| --- | ---:| ---:|
| `Mission_Intro_Tutorial_I` | 1,891,183,967 | `0x70B92D5F` |
| `Mission_IronStronghold_Block_ReturnToSister` | 3,594,586,120 | `0xD64ACBC8` |
| `Intro_Tutorial_Miseenscene_00` | 3,530,966,846 | `0xD276723E` |
| `Quest_Node_Her_RuinedChapel_Normal` | 1,207,446,228 | `0x47F32554` |

The first three are tested in
`src/c_abi/checksum.rs::tests::c_abi_checksum_known_inputs`. The
fourth is from the original PoC probe; recorded here for cross-
session verification.

The Jenkins init constant is `0xDEBA1DCD` (Pearl Abyss's custom
choice; standard Jenkins uses `0xDEADBEEF`). If this constant ever
changes in a future patch, every hi32 in this entire doc shifts
simultaneously — the giveaway would be that none of the verified
mappings work, not just one.

---

## Methodology / provenance

- **PALOC source**: `0020/.../gamedata/stringtable/binary__/localizationstring_eng.paloc`
  in 1.06. 16 MB, 179,513 entries (124,800 numeric-keyed + 54,713
  symbolic-keyed).
- **Gamedata source**: `0008/.../gamedata/binary__/client/bin/<table>.pabgb`
  for missioninfo, questinfo, stageinfo, knowledgeinfo,
  questgaugeinfo, knowledgegroupinfo.
- **Probe scripts**: ad-hoc Python under `out/` (not checked in;
  prototypes for what's now formalized in `src/<table>_info/` +
  `src/c_abi/<table>_info.rs`).
- **Editor's keycases handoff**: bundle at
  `D:\Github\CrimsonAtomtic\out\analyze\handoff\` from the editor
  session — provided 63,522 distinct `(type, value)` pairs from a
  live 1.06 save, used to validate which save-side key values
  actually appear in user playthroughs.
- **User-supplied data**: Main Quest titles typed from in-game UI;
  wiki knowledge entries pasted from web reference (specific source
  not recorded in session).

All verified mappings round-tripped through the C ABI bridges, not
just the Rust internals — the tests use the same FFI surface the
Save Editor's `LocalizationProvider` calls.

---

## Cross-references

- [`save-editor-keys-plan.md`](./save-editor-keys-plan.md) — the
  roadmap, decisions log, and architecture for the bridges
  themselves. This reference doc is the *data*; the plan doc is the
  *design*.
- [`crimsonforge-feature-gaps.md`](./archive/crimsonforge-feature-gaps.md) —
  what CrimsonForge (the Python toolkit) can parse that crimson-rs
  doesn't (yet); useful when extending coverage beyond the keys
  shipped so far.
- [`1.05-parser-history.md`](./archive/1.05-parser-history.md) — the RE
  history of the 1.05 iteminfo parser. Useful when the schema in a
  new patch shifts and the anchor scanner needs adjustment.
- [`scripts/CLAUDE.md`](../scripts/CLAUDE.md) "On a new game patch"
  runbook — the canonical first-response procedure when a new
  version drops.

---

## Coverage status snapshot (frozen at session end, 2026-05-14)

| Key | Bridge | Verified fixtures |
| --- | --- | ---:|
| ItemKey | ✅ shipped (earlier) | tests in `src/c_abi/iteminfo.rs` |
| SkillKey | ✅ shipped | tests in `src/c_abi/skill_info.rs` |
| MissionKey | ✅ shipped | 7 rows above |
| QuestKey | ✅ shipped | 6 rows above |
| StageKey | ✅ shipped | 6 rows above |
| KnowledgeKey | ✅ shipped | 5 rows above |
| QuestGaugeKey | ✅ shipped (internal name only) | 5 rows above |
| Hash function (Jenkins hashlittle2) | ✅ exposed via `crimson_calculate_checksum` | 4 anchor pairs above |
| FieldNPCSaveDataKey | ✗ not shipped (item #8) | — |
| FieldGimmickSaveDataKey | ✗ not shipped (item #8) | — |
| SubLevelKey | ✗ not shipped (item #9, deferred) | — |
| Knowledge category breadcrumb | ✗ not shipped (future enhancement) | reference only |
| Quest chapter rollup ("Prologue: Dead of Night") | ✅ shipped via `main_quest_chapter` (curated static table — chapter layer not in gamedata) | 8 tests in `src/c_abi/main_quest_chapter.rs`, source [`main-quest-list.md`](./ref-gamedata/main-quest-list.md) |
| Side quest → faction rollup | ✅ shipped via `side_quest_faction` (curated static table — sibling to `main_quest_chapter`, two-direction lookup) | 7 tests in `src/c_abi/side_quest_faction.rs`, source [`side-quest-list.md`](./ref-gamedata/side-quest-list.md) |
