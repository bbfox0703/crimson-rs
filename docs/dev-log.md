# Dev Log

Append-only history of what shipped, **newest first**. One entry per merged PR, or per PR group that
landed together. **Reading tip:** grep `^## ` for the index, then read from the top.

- **Anchor:** the date the PR merged and its number, `## YYYY-MM-DD (#N) — Title`. There is no
  build number in this repo; `git log --merges` maps a PR to its merge commit. An entry written before
  its PR exists leaves the number out (`## YYYY-MM-DD — Title`); `git blame` on the heading finds the PR.
- **When:** in the same PR as the change, as its last commit, so the history never lags the code. A
  doc-only PR gets an entry too when it changes a rule or a workflow; a typo fix does not.
- **What goes in:** what changed and why, what was measured and how sure it is, what was ruled out or
  is still not established, the test count, and whether the C# editor (CrimsonAtomtic) has anything to
  do. Code comments say what code does; the story of how it got there lives here.
- **Never rewrite an old entry.** If one turns out wrong, say so in a new entry and name the old one.
- **Two PCs:** after `git pull` on the other machine, read the entries newer than the last one you
  saw. That is what changed while you were away, with the reasons.
- **Splitting:** when this file passes about 400 KB, move the older entries, unedited, to
  `docs/archive/dev-log-<yyyy-mm>-pre-pr-<N>.md` (cut on a date boundary), and list the new file
  under "Older entries" below and in the archived list of [`README.md`](README.md).

> **Looking for the current state?** `CLAUDE.md` says what is true now and what carries forward to
> every patch. [`validated-patches.md`](validated-patches.md) records how each game patch's drift was
> found and pinned, and [`c-abi-surface.md`](c-abi-surface.md) lists every bridge. This file records
> *what shipped when, and why*.

-----

## 2026-11-28 — Start the dev log; every CLAUDE.md under 40 KB

- **Dev log:** this file. Each PR adds an entry here, newest first, the way the sibling UE5CEDumper
  repo keeps its `docs/dev-log.md` (there anchored on build numbers; here on PR numbers). `CLAUDE.md`
  adds the rule to Working conventions. The entries below backfill the 2.06 work, PRs #115–#127, all
  merged on 2026-11-27, from their PR descriptions; nothing before #115 is backfilled.
- **Size rule:** every `CLAUDE.md` stays at or under 40 KB (40,960 bytes), the limit UE5CEDumper
  uses, because each one is loaded into every turn that touches its directory. A new test,
  `claude_md_files_stay_under_40_kb`, fails `cargo test --lib` (so CI) on any `CLAUDE.md` past it.
- **Moved, not rewritten** (links retargeted; a few "see the 2.0x row in `scripts/CLAUDE.md`" phrases
  now say "the table above"):
  - `scripts/CLAUDE.md` 107,049 → about 30 KB. The "Validated patches" table (64 KB) and the ItemInfo
    per-patch list went to [`validated-patches.md`](validated-patches.md), the save-side probe catalog
    to [`save-side-probes.md`](save-side-probes.md), and the runtime item-object study to
    [`runtime-item-objects.md`](runtime-item-objects.md).
  - Root `CLAUDE.md` 27,010 → about 18 KB. The per-patch paragraphs (2.06 back to 2.03, and the
    absence markers) went to "Patch summaries" in `validated-patches.md`, replaced by four bullets of
    what is true now; the full document list went to the new [`README.md`](README.md) index, and
    `CLAUDE.md` keeps six rows.
- **Not touched:** `data/CLAUDE.md` (13 KB) and `scripts/archive/CLAUDE.md` (2 KB) are under the limit.
- **Tests:** see the PR.

-----

## 2026-11-27 (#127) — Per-machine Claude memory moved into the repo

- **What changes:** two Working conventions in `CLAUDE.md`, "Shared knowledge goes in the repo, not in
  Claude memory" and "Verify a root cause before stating it" (moved from one PC's memory with the three
  cases behind it), and the X:-drive PC's version archive (`X:\Crimson Desert\gamedata-bin`,
  1.03.01–2.06) in the Machine notes of `scripts/CLAUDE.md`.
- **Why:** Claude memory lives under `~/.claude/projects/…` on one machine, so the other PC never saw it.
- Doc-only; no code change.

-----

## 2026-11-27 (#126) — README / CLAUDE.md catch-up with the dye work

- **What changes:** the README's dye line (the item → dye-row join goes through the item's `.prefab`
  meshes; the 12-byte mask is one flag per dye slot number; palette unlock keys and theme names), its
  DLC line, and a correction note on the 2.01 row's old "four groups of three" reading; the 3-byte
  slot-mask bullet in `CLAUDE.md`.
- Doc-only; no code change.

-----

## 2026-11-27 (#125) — Dye palette: each color's unlock KnowledgeKey and the theme's name

- **What changes:** every 2.06 color record carries an inlined `CheckKnowledge(<KnowledgeKey>)`. On all
  10 rows, positions 0–8 (the gray ramp) need `Knowledge_Grey_DyeColor_<n>`, and positions 9–108 come in
  five blocks of 20 that need `Knowledge_<Theme>_DyeColor_<1..5>`: 60 keys in all. The row trailer is a
  localizable string, PALOC key `(key << 32) | 0x231` ("Hernandian Dye"; the three Dem rows all read
  "Demenissian Dye"). The row head is the theme's map-icon StringInfoKey, not a condition.
- **New C ABI (additive):** `crimson_dye_color_group_info_palette_knowledge_at` (`NOT_FOUND`,
  `OUT_OF_RANGE`, and `UNSUPPORTED_VERSION` for a pre-2.06 `ConditionInfoKey` record) and
  `crimson_dye_color_group_info_lookup_display_name`.
- **Not established:** what makes a theme appear at a dyer. `Knowledge_<Theme>_DyeColor`
  (2147483445..454) is not referenced by this table, and no `Knowledge_Collection_Prop_Dyewater_*` key is.
- **Tests:** 398 passed (with `CRIMSON_FIXTURES_DIR`); strict clippy and `cargo check --lib` clean.
- **Downstream:** for CrimsonAtomtic's "known dye colors" editor.

-----

## 2026-11-27 (#124) — Optional DLC merge for the item → dye-row join

- **What changes:** `crimson_item_part_prefab_merge_dlc_from_bytes` (the `iteminfo.dlc_01` and
  `stringinfo.dlc_01` bodies plus `dlc/dlc_01/0009/0.pamt`; DLC prefabs tried first) and
  `crimson_item_part_prefab_lookup_source` (0 = base, n = the n-th merged DLC). The merge builds the
  whole join before touching the handle, so a missing PAMT or an unparsable table changes no answer.
- **Without the DLC nothing changes:** skip the call and every lookup answers from the base items.
- **Measured (2.06):** the DLC tree has no dye rows of its own (`partprefabdyeslotinfo.dlc_01` is an
  empty 2-byte header); its 432 prefabs share no name with a base prefab; `stringinfo.dlc_01` adds 3,280
  strings and conflicts with none of the 33,855 shared ones. The merge adds 89 items, 82 of the 85
  DLC-only dyeable ones; with it, 991 of 1,002 dyeable items resolve.
- **Tests:** 396 passed; strict clippy and `cargo check --lib` clean. The live DLC test skips without
  the DLC, and a base-only test checks that a failed merge changes nothing.

-----

## 2026-11-27 (#123) — Join items to dye rows through their prefab meshes

- **What changes:** a `partprefabdyeslotinfo` row is one `.pac` mesh, keyed by
  `calculate_checksum(lowercase(pac path))` (1,703 / 1,703 rows on 2.06). The join is now item →
  `.prefab` (group 0009) → `.pac` paths → checksum → rows, and an item's dye slot numbers are the union
  of its rows' masks.
- **Why:** the old name join resolved 70 of 917 dyeable items and was wrong on 5 of them (spear 1001163
  resolved to a horse-saddle hook mesh). The prefab join resolves 909; the other 8 use meshes with no
  dye row. Body-type armor works now.
- **New C ABI (additive):** `crimson_item_part_prefab_load_from_{bytes,file}_with_prefab_pamt`,
  `_lookup_dye_slot_bits`, `_lookup_channel_dye_slot_bits` and `_lookup_dye_slot_material`, each with an
  optional tribe/gender filter: 14 of the 526 multi-variant items offer different numbers per body
  variant (133172: male 0..4, female 0..2).
- **Saves:** in 2.06 slot102, 111147's `_dyeSlotNo` 0/1/7 fall inside its union; in the 1.10 fixture
  every `_dyeSlotNo` is a 2.06 channel number.

-----

## 2026-11-27 (#122) — The 2.01+ dye mask is one flag per dye slot number

- **What changes:** the 12-byte `partprefabdyeslotinfo` mask is not four groups of three bytes. Each
  `mat_indices[ch]` byte is the dye slot number (0..12) of material channel `ch` (`0xFF` = unused), and
  `mask[n] == 1` means dye slot `n` is dyeable. The save's `ItemDyeSaveData._dyeSlotNo` stores the same
  number.
- **Evidence:** every set bit is one of the slot's own `mat_indices` (6,585 / 6,585 slots on 2.01,
  6,955 / 6,955 on 2.06); `mask12[mat_indices[ch]] |= mask3[ch]` reproduces 6,228 of the 6,552
  comparable 2.00 → 2.01 slots exactly; channels sharing a number share one material name (0 conflicts).
- **Not established:** no save written by 2.01 or later with a dyed item that maps to a dye-table
  prefab was available.
- **New C ABI (additive):** `..._lookup_slot_active_channels`, `..._lookup_slot_dye_slot_bits`,
  `..._lookup_slot_extra_layer_dye_slot_bits`, `..._lookup_dye_slot_bits`, `..._lookup_dye_slot_material`.
  The 3-byte `..._lookup_slot_mask` stays, as a partial read.
- **Tests:** 388 passed; strict clippy clean.

-----

## 2026-11-27 (#121) — Save fixtures to crimson-rs-private; factionrelationgroup decoded

- **Fixtures:** the real saves (1.09, 1.10 and the 1.10 length-change repro sets) moved from
  `tests/fixtures/saves/` under git-crypt to the private `bbfox0703/crimson-rs-private` repo, cloned next
  to this one. `src/test_fixtures.rs` finds them (`CRIMSON_FIXTURES_DIR`, else
  `../crimson-rs-private/saves`) and skips with a note without them. CI never had the git-crypt key, so
  it skipped these tests before and still does. `*.save` is gitignored.
- **factionrelationgroup:** a row is `_isBlocked` plus four count-prefixed group lists, one per
  FactionRelationType (Neutral, War, Agreement, BloodAlly). The old "list1" / "list2" were War and
  BloodAlly; the bytes read as padding were `_isBlocked` and the two empty lists. The matrix is
  symmetric, covers all 10 pairs of the 5 groups, matches the game, and is unchanged since 1.03.01.
  New `crimson_factionrelationgroup_lookup_relation` (`NOT_FOUND` for a group with itself).
- **Tests:** 386 passed with the private repo cloned; strict clippy and `cargo check --lib` clean.

-----

## 2026-11-27 (#120) — README: faction surface, DLC merge, 2.06 inlined conditions

- Resolver count 34 → 35, the faction line, the optional DLC merge, and the 2.06 per-patch row
  (2,564 inlined `conditioninfo` rows; characterinfo head +2 bytes).
- Doc-only; no code change.

-----

## 2026-11-27 (#119) — Optional dlc_01 merge for factionnode, knowledge, faction, characterinfo

- **What changes:** `*_merge_dlc_from_bytes` adds the DLC rows whose key the handle lacks, after the
  existing entries, so indices don't move; a shared key keeps the base row (on 2.06 base and `dlc_01`
  agree on every field these bridges expose). `*_entry_source` returns 0 for base, n for the n-th DLC.
- **Why opt-in:** Steam installs `dlc/dlc_01` only for owners (depot 5001841, DLC app 5001840). Without
  it nothing changes.
- **Measured (2.06):** the DLC adds 142 faction nodes, 795 knowledge rows, 11 factions and 287
  characters; with both factionnode and knowledge merged every node has a place name
  (`Node_Ocean_IliyaIsland` → "Iliya Island"). The six `Contribution_*` items are all base items.
- **Tests:** 386 passed; strict clippy and `cargo check --lib` clean.

-----

## 2026-11-27 (#118) — Faction node place names; 2.06 inlined conditions

- **What changes:** every factionnode row names a `knowledgeinfo` row whose title is the place name
  (a `u32` at byte 11 after the key on 2.04–2.05.01, byte 8 after `[key][row index]` on 2.06; pinned on
  all 1,120 rows of five versions). New `crimson_factionnode_lookup_knowledge_key` and
  `crimson_factionnode_lookup_display_name` (any of the 15 languages).
- **Found on the way:** the knowledge scanner rejected an apostrophe, which hid
  `Knowledge_King's_Tomb_Cave`; it now reads 6,839 rows, matching the header. The 2.06 dye record tail is
  an inlined `CheckKnowledge` condition, so the parser now walks variable-length records. 2,564 anonymous
  `conditioninfo` rows were inlined as `01` + bytecode in ten tables ("Inlined conditions (2.06)" in
  `scripts/CLAUDE.md`).
- **Not established:** no owning-faction or region field on a node; what was ruled out is documented.
- **Tests:** 382 passed; strict clippy and `cargo check --lib` clean.

-----

## 2026-11-27 (#117) — Character faction membership and faction group names

- **What changes:** characterinfo's FactionKey (`u32`, byte 149 after the key on 2.04–2.05.01, byte 151
  after `[key][row index]` on 2.06; checked on all 7,315 rows of five versions). New
  `crimson_characterinfo_lookup_faction_key` / `_faction_member_count`. Faction rows' group and
  relation-group keys, with `crimson_faction_info_lookup_group_keys`,
  `_lookup_group_display_name` (PALOC `(group << 32) | 0x214`, 16965 → "Greymanes") and
  `_lookup_display_name`.
- **Why:** a save records relations between factions but not which faction an NPC belongs to.
- **Documented:** `_relationTypeRaw` 0 Neutral, 1 War, 2 Agreement, 3 BloodAlly, 4 ByGroup.
- **Tests:** 379 passed; strict clippy and `cargo check --lib` clean.

-----

## 2026-11-27 (#116) — Faction table bridge for FactionKey names

- **What changes:** `faction_info` parses `faction.pabgb` (135 rows on 2.06, plus `faction.dlc_01`); new
  `crimson_faction_info_load_from_bytes` / `_free` / `_entry_count` / `_lookup_string_key`.
- **Why:** CrimsonAtomtic looked FactionKey up at character.paloc's type byte, and factions share the
  1,000,000+ key range with characters, so FactionKey 1000000 (`Faction_Graymane`) read as "Hernandian
  Guard". Faction rows carry no localized name of their own.
- **Tests:** 379 passed; strict clippy clean.

-----

## 2026-11-27 (#115) — Crimson Desert 2.06: row names moved into the table headers

- **What changes:** 2.06 (paver `2/6/0`, build `0xe1bd9840`, `dlc_01` revision 11, Steam build
  26949954) lists `[key][u32 len][name][u32 offset][u32 size]` per row in every `.staticinfoheader`, and
  every body row opens `[key][u32 row index]`. `binary::static_header` reads it; iteminfo's `string_key`
  became a zero-width `HeaderName` filled from the header; skill and stringinfo round-trip the 2.06
  layout; the other table parsers rebuild the old layout in memory.
- **New C ABI (additive):** nine `*_load_from_bytes_with_header` loaders, because the eight body-only
  bridges find no rows in a 2.06 body alone and iteminfo needs the header for item names.
- **Field drift:** `dyecolorgroupinfo` color records grew from 8 to 22 bytes (explained in #118).
- **Content:** base iteminfo −1005744 (row 3,340) / +1007080 (row 6,076), so CE item IDs 3,340–6,075
  moved down by one; the 1,311 DLC-only IDs kept theirs.
- **Tests:** 377 passed; `_probe_skill_entry_failures` 2163 / 2163; `export_for_ce.py` 8,379 ok; every
  live save, 2.06 included, decodes with no trailing pad.
- **Downstream:** CrimsonAtomtic had to switch those nine loader calls.

-----

## Older entries

Not backfilled. For PRs before #115, read `git log --merges` and the PR descriptions on GitHub. The
per-patch drift history (1.03 → 2.06) is in [`validated-patches.md`](validated-patches.md).
