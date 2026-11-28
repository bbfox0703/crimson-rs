# Documentation index

Every document in the repo, one row each. The root [`CLAUDE.md`](../CLAUDE.md) lists only the few a
session needs first, because it is loaded on every turn; **a new document gets its row here.**

Active reference:
- [`docs/dev-log.md`](dev-log.md) — **What shipped**: append-only, newest-first history per merged PR (why, what was measured, what is not established). Read it when asking when or why something changed, or after a `git pull` from the other PC.
- [`docs/validated-patches.md`](validated-patches.md) — Per-patch table (1.03 → 2.06): item counts, parse status, each drift and how it was pinned; the ItemInfo per-patch drift list; the per-patch summaries that used to open `CLAUDE.md`.
- [`docs/save-side-probes.md`](save-side-probes.md) — Catalog of the `#[ignore]` save-side and gamedata probes and what each one catches.
- [`docs/runtime-item-objects.md`](runtime-item-objects.md) — Runtime item-object offsets (2.05.01, re-checked on 2.06) and why the CE table does not read them.
- [`docs/c-abi-surface.md`](c-abi-surface.md) — Full C-ABI bridge catalog + per-module source map (the detail formerly inlined in `CLAUDE.md`).
- [`docs/save-editor-keys-plan.md`](save-editor-keys-plan.md) — Active save-editor workstream roadmap (has the "🎯 New session pickup" callout).
- [`docs/save-editor-keys-reference.md`](save-editor-keys-reference.md) — Frozen 1.06 ground-truth regression baseline for key resolution.
- [`docs/abyss-gate-map.md`](abyss-gate-map.md) — Per-gate abyss-gate mapping + 3 pinned `_initStateNameHash` constants (Path B per-gate unlock; Phase 2 hash-decode partial).
- [`docs/socket-editor-scope.md`](socket-editor-scope.md) — Socket/gem editor reference + verified `_socketSaveDataList` schema.
- [`docs/dye-editor-scope.md`](dye-editor-scope.md) — Dye editor reference + the three dye gamedata bridges.
- [`docs/worldmap-plotting.md`](worldmap-plotting.md) — World-map plotting (positioned-entity enumerator + affine fit + basemap tile discovery).
- [`docs/artifact-challenge-mapping.md`](artifact-challenge-mapping.md) — Artifact ↔ Challenge 1:1 bidirectional mapping.
- [`docs/save-mutation-version.md`](save-mutation-version.md) — Mutation-version cache-coherency contract for C-ABI snapshot consumers.
- [`docs/save-deferred-redecode.md`](save-deferred-redecode.md) — Deferred-redecode batch ABI for bulk mutations.
- [`docs/save-loader-length-change-crash.md`](save-loader-length-change-crash.md) — Game-side `memcpy` overflow that crashes the game's loader on length-changing edits to some saves (root-cause + dump + repro fixtures). Not a crimson-rs bug.
- [`docs/paloc-template-survey.md`](paloc-template-survey.md) — Template-density per PALOC namespace (settles the template-resolver question: titles no, descriptions yes).
- [`docs/ref-gamedata/main-quest-list.md`](ref-gamedata/main-quest-list.md) — Curated Chapter→Arc→Mission table (`main_quest_chapter` bridge source).
- [`docs/ref-gamedata/side-quest-list.md`](ref-gamedata/side-quest-list.md) — Curated quest→faction table (`side_quest_faction` bridge source).
- [`docs/archive-format.md`](archive-format.md) — Binary format specs (PAPGT, PAMT, PAZ, trie) + mod-loading overlay.
- [`docs/api.md`](api.md) — Full Python API reference (enums, pack_mod, PAPGT/PAMT/PAZ, ItemInfo).
- [`docs/c-sharp-nativeaot-integration.md`](c-sharp-nativeaot-integration.md) — C# NativeAOT single-file integration recipe.
- [`docs/historical-parser-setup.md`](historical-parser-setup.md) — Sibling-install recipe for a previous-version parser (cross-version diff).
- [`docs/paloc-71-dev-items.md`](paloc-71-dev-items.md) — Why 71 items have no PALOC translation in any of 14 languages.
- [`scripts/CLAUDE.md`](../scripts/CLAUDE.md) — Next-session engineering notes: the "On a new game patch" runbook, machine notes, layout invariants, don'ts.

Archived / historical (`docs/archive/`):
- [`docs/archive/1.05-parser-history.md`](archive/1.05-parser-history.md) — Long-form RE history of the 1.05 ItemInfo parser (read first when picking up parser-RE work).
- [`docs/archive/globalgameevent-body-re.md`](archive/globalgameevent-body-re.md) — RE of the globalgameevent per-row body (group_key + paloc_key).
- [`docs/archive/mercenaryinfo-body-re.md`](archive/mercenaryinfo-body-re.md) — RE of the mercenaryinfo `(u8,u32)` PABGH variant.
- [`docs/archive/downstream-api-gaps.md`](archive/downstream-api-gaps.md) — Status of the CrimsonGameMods Python bindings (all done as of 2026-05) + cross-version notes.
- [`docs/archive/crimsonforge-feature-gaps.md`](archive/crimsonforge-feature-gaps.md) — What CrimsonForge (Python toolkit) can parse that crimson-rs doesn't yet.
