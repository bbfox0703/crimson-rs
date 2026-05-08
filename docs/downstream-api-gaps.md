# Downstream API Gaps — CrimsonGameMods

Bindings the `CrimsonGameMods` Python app currently calls (or plans to call)
on `crimson_rs` but `src/python.rs` does not yet export. Captured 2026-05-08
from a survey of `D:\Github\CRIMSON-DESERT-SAVE-EDITOR-AND-GAME-MODS`
(78 `.py` references to `crimson_rs`).

The intent is to keep crimson-rs as the binary-format toolkit and to push the
small set of missing bindings — not to absorb the application-layer Python
that GameMods runs on top.

## Status snapshot

- **API surface gap is small.** Of every `crimson_rs.*` symbol GameMods uses,
  only three are not exported. Two of them are guarded with `hasattr` in
  GameMods, and the bundled `crimson_rs.pyd` (commit `b038c2d`) does not have
  them either — so adding these is forward progress, not catch-up.
- **Parser correctness is ahead of the bundled `.pyd`.** GameMods bundle is
  pinned at `b038c2d` and reports 8 housing items failing
  (`1003774, 1003823, 1003824, 1003825, 1003976, 1003977, 1003978, 1003979`).
  Current `dev` is at `648b1f2` with the 1.05 parser at 100% per
  `b33c522 parser(1.05): 100% parse + archive RE history`.
- **Cross-version strategy differs.** GameMods ships two `.pyd` side-by-side
  (`crimson_rs/crimson_rs.pyd` + `crimson_rs/_legacy/crimson_rs.pyd`,
  commits `b038c2d` + `dd3c1d3`); see the dual-parser doc inside the GameMods
  tree. crimson-rs uses the sibling-install + cross-version diff workflow in
  `docs/historical-parser-setup.md` and `docs/1.05-parser-history.md`.

## TODO — bindings to add

### 1. `extract_file_from_paz(paz_path: str, vfs_path: str) -> bytes` — **DONE**

Implemented as a sibling entry point to `extract_file`. The function uses the
parent directory of `paz_path` to locate `0.pamt`, splits `vfs_path` on the
last `/` for directory + file name, and routes the read through the existing
`paz::extract_file` path. PAMT decides which chunk file is actually opened, so
any `.paz` in the group directory is a valid pointer.

GameMods call sites that were `hasattr`-guarded against the bundled `.pyd`:

- `gui/tabs/mod_loader.py:677`
- `overlay_coordinator.py:252`

Both now resolve to a real binding.

### 2. `parse_skillinfo_from_bytes(skill_pabgb: bytes, skill_pabgh: bytes) -> list[SkillInfo]`

Parse the skill table the same way GameMods parses iteminfo — using both the
`.pabgb` payload and the matching `.pabgh` index.

- GameMods call site: `gui/tabs/buffs_v319.py:6359`.
- Use case: BuffsV319 reads `skill.pabgb` + `skill.pabgh` from group `0008`
  and walks `buff_level_list` per skill. GameMods currently has a 34 KB pure
  Python `skillinfo_parser.py` to substitute when this binding is missing.
- Note: this is a new binary parser, not a rebinding of an existing one.
  Scope it carefully — defer if format coverage isn't yet RE'd to the same
  level as iteminfo. The PABGH-bounded per-entry pattern from `iteminfo`
  applies (each entry parsed against an index-supplied range).

### 3. `inspect_legacy_patches(vanilla_bytes, [{entry, rel_offset, length}, ...])` — **DONE**

Implemented as a free function. Internally parses `vanilla_bytes` once with
the existing tracked reader (so the per-item field ranges already produced by
`parse_iteminfo_tracked` are reused), builds a `string_key → item_index`
HashMap, and for each patch binary-searches the target item's ranges for the
field whose `[start, end)` covers `entry.start + rel_offset`. Returns a
same-length list with `None` for missing entries / out-of-range offsets and
`{path, ty, abs_start, abs_end, hit_offset, hit_length}` for hits.

GameMods consumer:
`D:\Github\CRIMSON-DESERT-SAVE-EDITOR-AND-GAME-MODS\CrimsonGameMods\JSON_V2_TO_SEMANTIC_FORMAT3_PLAN.md`
— the JSON v2 → semantic translator no longer needs the reparse-diff
fallback for the typical case.

Note: the function only attributes the *start* of each patch. Multi-byte
patches that cross field boundaries still report a single field at the start
position; the caller decides whether that's an error.

## Out of scope — what GameMods builds *on top of* crimson_rs

Recording these so future me does not get tempted to absorb them. GameMods
does the following in pure Python (or via a separate native DLL); they are
not crimson-rs responsibilities:

1. **Save game (`.sav`)** — `save_parser.py` (55 KB), `save_crypto.py`,
   `save_pet_rename.py` (33 KB).
2. **PARC format** — `parc_parser.dll` (390 KB native) +
   `parc_inserter2.py` / `parc_inserter3.py` (114 KB) + `parc_serializer.py`
   (48 KB) + `parc_tree_*`.
3. **Other PABGB tables (16 Python parsers)** — gimmick, field, faction,
   character, equipslot, mercenary, region, reserveslot, skilltree, store,
   terrain_spawn, vehicle, wanted, quest, housing, plus
   `universal_pabgb_parser.py`. (Skill is the one called out in TODO #2
   because it has the most concrete Rust call site already.)
4. **In-place PAZ patching** — `paz_patcher.py` (83 KB) is a
   signature-scan-and-byte-replace patcher for existing `.paz` files; that's
   different from `PackGroupBuilder`, which builds new packs.
5. **Mod loader / overlay coordination** — `mod_loader.py` (28 KB),
   `overlay_coordinator.py` (24 KB).
6. **Application layer** — PyQt GUI, CLI, item/store/dropset editors,
   item creator, mesh / bone injectors, imbue editor, translation pipeline,
   31 MB pre-built SQLite (`crimson_data.db`).

## Reference documents inside the GameMods tree

When picking this up on another machine, these are the most useful files in
`D:\Github\CRIMSON-DESERT-SAVE-EDITOR-AND-GAME-MODS\CrimsonGameMods\` to read
first:

- `CRIMSON_RS_DUAL_PARSER_TECHNICAL_DOC.md` — why two `.pyd`'s, the field
  diff between the two parsers, and the housing-items hypothesis.
- `JSON_V2_TO_SEMANTIC_FORMAT3_PLAN.md` — what `inspect_legacy_patches` is
  expected to do and the reparse-diff fallback path.
- `crimson_rs/__init__.pyi` — older but parallel type stubs; useful diff
  target against `python/crimson_rs/__init__.pyi` in this repo.

## Upstream tracking

The dual-parser doc references an upstream PR at
`https://github.com/potter420/crimson-rs/pull/1`. This repo is
`bbfox0703/crimson-rs` with `potter420/crimson-rs` configured as `upstream`
(see `git remote -v`). 1.05 work landed locally first; upstream sync status
is not tracked here.
