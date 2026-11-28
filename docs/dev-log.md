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

## 2026-11-28 — Comment rules applied repo-wide; pyo3 0.30 and patched dependencies

After the audit batches, at the maintainer's request: the comment rules of the UE5CEDumper repo,
adapted here.

- **The rule** is now in `CLAUDE.md` under Working conventions ("Code comments"): present tense, and
  no history in a comment. That means no fix narrative, audit or review tags, commit hashes, PR
  numbers or "found on <date>"; that goes here or in the document this points to. Also:
  - no line numbers into the repo;
  - no restating the next line;
  - no enumerations that must be kept in sync;
  - no unenforced "only" / "every caller" claims;
  - no plan wording.

  Facts about the game data stay when they are the reason for the code. Counts that pin a patch
  belong in a test or here. The maintainer asked for no bug tags at all, which is stricter than
  UE5CEDumper's `[TAG]` + date.
- **Gate:** `repo_doc_budget::comments_cite_no_lines_or_audit_tags` (CI) fails on a source comment
  that cites `foo.rs:12` or carries an `audit X-NN` tag. It found 123 before the pass and 0 after.
  History phrasing beyond those two patterns is left to review: the leftover "used to" / "fix" hits
  are "used to compute", "fixed" and "fixture".
- **The pass:**
  - Seven subagents split the source by file (`src/c_abi/mod.rs`; the big `c_abi` files; the dye /
    item / quest bridges; the other bridges; core / archive / save; the table parsers; scripts and
    the Python stub). They edited comments only.
  - Each group was checked before its commit: with comments stripped, HEAD and the new file have
    identical code in every changed file. Seven commits, roughly 2,000 comment lines removed net.
  - `scripts/archive/` was left as historical tooling.
  - Docs that were wrong about the code got corrected along the way. Transplant within one handle is
    allowed. A failed `end_deferred_redecode` moves the version forward. The allowed-gem list is a
    union of two lists. factionnode has a display-name lookup. Several test docs now describe what
    their tests actually assert.
  - `#[ignore]` reasons and two code consistency fixes followed: stringinfo keeps the first row for a
    duplicate hash, as every other bridge does, and the house / royal-supply extern docs gained the
    summary line the symbol index reads.
- **Where the removed history went:**
  - `src/item_info/item.rs`'s 516-line per-patch layout history is archived verbatim in
    `docs/archive/iteminfo-layout-history.md`. The nine `validated-patches.md` / `api.md` pointers to
    that header now point there.
  - Measurements and RE findings that no document recorded are collected in
    `docs/archive/comment-pass-2026-11-28-notes.md` (44 entries).
  - Unreconciled, and noted there: the appearance-key hit count, 16 of 122 in the old probe doc vs 9
    of 122 in `save-editor-keys-plan.md`.
- **Dependencies (CI):** the new `cargo audit` job failed on its first run.
  - pyo3 0.27.2 had RUSTSEC-2026-0176 / 0177; it is now 0.30.1, with no code change.
  - anyhow 1.0.102 and rustix 1.1.4 were unsound; now 1.0.105 and 1.1.6.
  - chacha20 0.10.0 was yanked; now 0.10.2.
  - The rebuilt wheel round-trips live iteminfo, skill, PAPGT and all 74 PAMTs byte for byte and
    reads the English PALOC (222,887 entries). CI is green on `a037809`.
- **Measured on this PC:** 441 passed, 48 ignored, with the install and fixtures; the same 441 pass
  with neither. Strict clippy and all four feature combinations are clean.
- **CrimsonAtomtic:** nothing to do. No signature or behaviour changed, apart from stringinfo's
  first-wins on a duplicate hash; 2.06 has none.

## 2026-11-28 — Audit P10: docs, tooling, RustCrypto versions, last tests; every finding settled

The tenth and last fix batch, on the same branch as the entries below. Status table after it: 133
fixed, 14 not fixing (each with its reason in the table), 2 merged, none open.

- **Dependencies (I-21):**
  - `hmac` 0.12 → 0.13 and `sha2` 0.10 → 0.11, at the maintainer's request. `Cargo.lock` had two
    RustCrypto generations (digest 0.10 / crypto-common 0.1 beside chacha20's cipher 0.5 /
    crypto-common 0.2), and `cargo tree -d` now reports no duplicate crate.
  - `hmac` 0.13 moved `new_from_slice` to `KeyInit`, so the code imports it. The HMAC-checking save
    tests pass on the live saves and the fixtures.
  - CI gained a `cargo audit` job; it is not required and was not run locally.
- **Docs and comments matching the code:**
  - `hashlittle`, not `hashlittle2` (62 places). paz / paloc / pamt / paver docs (A-18).
  - The `BString` rule in `CLAUDE.md`, `row_head` key widths, checked size arithmetic in the
    stringinfo index (B-12).
  - The `CrimsonItemInfoSummary` table and the 28 item-flag bits in `c-abi-surface.md` (C-13).
  - Stale bridge comments (C-15).
  - A subagent did D-11, E-14, F-13 and G-16, comments and docs only: 54 files, every `.rs` hunk a
    comment. Its fixes include 2.06 row counts, the save docs agreeing that a no-op re-encode is
    byte-identical, and the macro example no longer making the symbol index list six phantom rows
    (369 symbols now).
  - `tools/README.md` records `git-crypt.exe`: 0.8.0, its SHA256, upstream, and why it stays
    (I-20). Whether it equals an official release asset is not established.
  - `store_info.hexpat` is marked as the pre-2.04 layout (I-23).
- **Code:**
  - A checksummed PAPGT with bytes after its name buffer is an error (A-10).
  - The unused `[u8; 22]` / `[u8; 9]` impls are gone, and `api.md` documents
    `MergedPrefabVisualData` (H-18).
  - `set_object_list_present` refuses to make present an absent list read without its `0x01` marker
    (G-09).
- **Tests:**
  - **F-04:** the fixture saves re-encode, unedited, byte for byte.
    - The 1.10 crash fixture, written by the old editor, re-encodes with exactly its two stale
      self-referential payload offsets moved 241 bytes on, and every other byte unchanged. That is
      the relocation the old editor missed, now a regression test.
  - **G-07:** a two-thread test runs batch open / abort beside item enumeration on one handle.
  - **D-07:** nine live table tests (store, faction, quest, stage, mission, knowledge, character, sub
    level, quest gauge) assert rows parsed == rows the 2.06 header lists, instead of a floor. All
    nine hold.
- **Correction:** `eddc8cb`'s message claims the D-07 change, but the edit never reached the files.
  The script that applied it hung on a backtracking regex, and its lost output read as a pass. It
  landed in `4528735`; `eddc8cb` holds only G-07 and G-09.
- **Not fixing**, each recorded in the status table: E-04 (per-row drop signal), E-05, G-07's
  per-mutation deferred tests, G-10 (CRET format change), G-14, H-11, H-16, I-11, I-19, I-22, F-09,
  D-06's other tables.
- **Measured on this PC:** 440 passed, 48 ignored, with the install and fixtures; the same 440 pass
  with neither. Strict clippy and all four feature combinations are clean. The wheel was rebuilt.
- **CrimsonAtomtic:**
  - `set_object_list_present` can return `MUTATION_INVALID` for an unmarked absent list; no
    game-written save has one.
  - The symbol index dropped six phantom house_info rows. These were never real exports.
  - No signature changed.

## 2026-11-28 — Audit P9: code nits (header widths, stringinfo layout, dye FFI hygiene, script lookup)

The ninth batch, on the same branch as the entries below: the nit-level code findings. Status table
after it: 116 fixed, 22 partly, 7 open (doc and tooling items), 2 not fixing, 2 merged.

- **2.06 headers:**
  - `static_header::WIDTHS` covers all 15 count/key width pairs (adds `(1,8)`, `(1,12)`, `(4,12)`).
    Unit tests detect every pair, and refuse gaps, overlaps, short rows, a lying count and a NUL in a
    name (B-04).
  - A header name with a NUL is refused: a C caller would read it cut short while `required`
    reported the full length (B-11).
  - `dump_gamedata_keys.py` tries the same 15 pairs in the same order and checks row packing (B-07).
    Its 2.06 snapshot is unchanged.
- **stringinfo layout (C-11):** a body-only read decided legacy vs 2.06 from the first word alone,
  so a 2.06 table whose first hash is 0 would swap every hash with its row index. It now takes 2.06
  as soon as one of the first eight rows has a non-zero first word; a legacy first word is always 0.
  - The first version of this fix, `5f1366f`, keyed on the row indices counting 0, 1, 2.
  - A DLC table's row index is not the row's position, so `stringinfo.dlc_01` read as legacy.
  - The item prefab DLC merge then added 3 items instead of 89. The live merge test caught it before
    the push, and `370672e` fixed it.
- **Dye / prefab:**
  - `.pac` paths match in any case, and a backslash separator becomes `/` (E-06). Coverage stays
    909 of 917 dyeable items, so neither explains the 8 unresolved ones; the live test prints it.
  - The item handle keeps the first row for a duplicate key (E-10).
  - A name-join merge no longer opens the prefab path it ignores (E-11).
  - The slot marker `0xFF`, the row pad and the palette pad are checked; every 2.06 row still parses
    (E-13).
  - FFI hygiene: raw-pointer helpers are `unsafe fn`, out-params are zeroed before a lookup can fail,
    and the three `write_str_to_buf` copies became one (E-15).
  - Test shifts are range-checked first (E-16). The key-invariant test reads the 0008 table alone and
    pins its row count to the header's.
- **Opaque handles:** the 27 C ABI handles that hold `HashMap` / `Vec` drop `#[repr(C)]`, which had
  no effect (A-19 / B-11 / E-15). Also gone: a stale `allow(unused_imports)` and the never-built
  `Compression::Partial` (A-19).
- **Smaller fixes:**
  - The name-only macro's byte loader uses `slice_from_raw_or_empty` (B-11).
  - `live_titles` keeps the first row, as the bridges do (D-10).
  - The inventory enumerator also accepts a `u32` `_slotNo` (G-14, partly; name matching and the
    playable-character list are unchanged, and the player-owned filter test passes on the 2.06 save).
- **Scripts (I-18):** one install lookup in `gamedata_layout.py`. It has `GAME_DIR_CANDIDATES` and
  `game_dir_candidates()`, which puts `CRIMSON_GAME_ROOT` first, plus `find_game_dir()`.
  `export_for_ce`, `extract_gamedata_bin`, `dump_gamedata_keys`, `update_ce_table` and
  `decode_dyeslot_113` use it.
- **Documented, behaviour kept for ABI stability:**
  - `quick_slot_index` is always 0 since 2.04, and what `IS_IMPORTANT_ITEM` means (C-16).
  - The two `lookup_dye_slot_material` functions take `dye_slot_no` as `uint32_t` (slot info) vs
    `uint8_t` (item) (E-09).
  - `position_for_rgb` returns the first match (E-12). No 2.06 palette repeats an RGB (measured over
    all 10), so the answer is unique there.
- **Measured on this PC:** 436 passed, 48 ignored, with the install and fixtures; the same 436 pass
  with neither. Strict clippy and all four feature combinations are clean. The wheel was rebuilt, and
  iteminfo round-trips body and header byte for byte.
- **CrimsonAtomtic:**
  - Out-params are now zeroed on errors where they used to keep the caller's value.
  - A 2.06 table with a NUL in a row name now fails to load.
  - A slot-info or palette row with a changed constant byte drops out, and the live tests would flag
    it.
  - No signature changed. The `dye_slot_no` widths are documented: the item function's parameter
    must be declared `byte`.

## 2026-11-28 — Audit P8: save pad, skill caps, PAMT dicts, CI builds, newest save, key snapshots

The eighth batch, on the same branch as the entries below: the remaining low findings. Status table
after it: 100 fixed, 21 partly, 24 open (mostly nits and doc items), 2 not fixing, 2 merged.

- **Save pad (F-08):** `encode_body` refuses a block whose length changed while it carries pad bytes.
  Pad bytes are trailing pad, or a wrapper prefix other than one `0x01`. Pad is written back verbatim
  and relocated only by the block's own shift, so an offset inside it goes stale once bytes move
  inside the block. Blocks that only move still relocate, and the 1.10 fixture's relocation test
  still passes.
  - **Measured:** every live and fixture save has 0 trailing-pad bytes, and every wrapper prefix is
    the single byte `01` (3,387 to 3,921 per save). The absence-marker test now pins the prefix too.
  - Rejecting every block with a non-empty prefix, as first considered, would have blocked all edits.
- **Skill validator caps (C-04):** the caps stay; real data past one would fail the whole table.
  - They are now named constants (lists 50, `dev_*` strings 2,000 B).
  - A live test fails once the largest value passes half a cap. 2.06 has list 13 and text 349 B.
- **PAMT dicts (H-04):** `serialize_pamt` raises `ValueError` when a directory's
  `file_start_index` / `file_count` do not match the files laid out in directory order. Those
  mismatches used to point directories at each other's files. The stub marks the decoded fields as
  read-only (`path`, `name`, `compression`, `crypto`, `is_partial`). All 74 live PAMTs (base and
  `dlc_01`) round-trip byte for byte.
- **CI (I-09, H-18, I-11):**
  - The lint job checks `--features c_abi` alone and `--features python` alone, and installs
    Python 3.12.
  - A new `windows native dll` job builds the shipped MSVC release dll and lib. It fails if the dll
    imports anything beyond system DLLs (`dumpbin /dependents`); locally that is
    `bcryptprimitives`, `KERNEL32`, `ntdll` and `api-ms-win-core-synch`.
  - `permissions: contents: read`.
  - The Rust toolchain stays on floating stable on purpose.
  - Not a required check; it has not run on GitHub yet.
- **CLAUDE.md correction:** the Windows abi3 `.pyd` does import `python3.dll` (`dumpbin` on the
  maturin build). The old line said it never does.
- **Live save tests (I-06):**
  - `test_fixtures::newest_live_save` picks the save with the latest mtime. The format and mutation
    tests use it instead of a pinned `slot107`, which can hold an old patch's save; the running tests
    lost their `slot107` names.
  - On this PC the newest is `slot1`, and 18 save tests pass on it.
  - The `#[ignore]` probes keep their slots.
- **Key snapshots (I-15):** `dump_gamedata_keys.py` adds `faction` and `stringinfo`. It also dumps
  every installed DLC's copy of each table, plus `iteminfo`, as `<table>.<dlc>.txt`. The 2.06 re-run
  left the 30 existing files unchanged and added 35 (65 files, 237,011 keys).
- **Faction (D-06, partly):** a row too short for its group keys now empties the table, so the loader
  returns `BODY_PARSE`. It used to report group 0, which reads as "no group".
  - The module doc said the three bytes before the keys are zero on every row. On 2.06, 5 of 135 rows
    hold 1..3 in bytes 1-2 (`Faction_Graymane` among them), so the zero check the audit suggested
    would have flagged the live table as drift.
  - The other fixed-offset reads are unchanged: factionnode, character, global game event, relation
    group.
- **Stub (H-16, partly):** `parse_iteminfo_tracked` / `_lossy` document that `string_key` is empty
  on 2.06 and that the lossy scan's 2.06 anchor is weak. They still take no header.
- **Measured on this PC:** 433 passed, 48 ignored, with the install and fixtures; the same 433 pass
  with neither. Strict clippy is clean, and so are all four feature combinations (none, `c_abi`,
  `python`, both). The wheel was rebuilt.
- **CrimsonAtomtic:**
  - A length-changing edit on a block carrying pad now fails with `MUTATION_INVALID`; no
    game-written save has such a block.
  - The faction loader returns `BODY_PARSE` on a drifted table.
  - No signature changed.

## 2026-11-28 — Audit P7: loaders report tables they cannot read, unverified saves, no leaked strings

The seventh batch, on the same branch as the entries below. Theme: a C ABI call that could not do its
job now says so instead of returning `OK`. Status table after it: 94 fixed, 19 partly, 32 open (mostly
nits and doc items), 2 not fixing, 2 merged.

- **Table loaders (B-02, E-04):**
  - Every table loader runs `check_rows` (`src/c_abi/mod.rs`) before it hands out a handle. That covers
    the `_with_header`, `_load_from_file`, body-only, `(pabgb, pabgh)`, name-only-macro and dye
    loaders, about 60 call sites. They return `BODY_PARSE` with no handle in three cases:
    - a non-empty header that is not a 2.06 header matching the body;
    - a header whose rows none read;
    - a non-empty body read without a header that gives no row.
  - All three used to return `OK` with 0 rows, or noise rows from the body scan.
  - The DLC merges check the DLC header against its body the same way. They used to return `OK`
    with `added == 0`.
  - `sibling_header` ignores the extension's case and reports read errors other than "not found"
    as `IO`. It used to turn both into "no header".
  - **Not caught:** single rows dropped inside an otherwise readable table. The live tests pin those
    counts (E-04 stays partly fixed).
  - **Pre-2.06 data:** a header in an older layout now returns `BODY_PARSE` instead of being ignored.
    The parsers target the latest patch only.
- **Unverified saves (F-10):** `crimson_save_write_to_file` returns the new `HMAC_UNVERIFIED` (−28) and
  writes nothing when the save's HMAC failed on load. Writing computes a fresh HMAC, so it used to make
  a damaged or tampered save valid, even with no edit. The new `crimson_save_set_allow_hmac_rewrap(h, 1)`
  opts in. Python's `write_save*` is unchanged.
- **No leaked strings (A-16, C-19):** `CString::data` is a `Cow`. It borrows when the bytes are UTF-8
  and owns a lossy copy otherwise; the raw bytes are still written back. The lossy copy used to be
  `Box::leak`ed, once per load in a long-running host. 2.06 iteminfo and the English PALOC have no
  such strings, so this was latent. The PALOC handle keeps the first entry for a duplicate key
  (`collect` kept the last), matching its enumeration and the other bridges.
- **Test (E-03):** the palette-knowledge live test fails on `UNSUPPORTED_VERSION` instead of skipping.
  On a latest-only parser that code means drift.
- **New tests:**
  - `load_check_tests`: the `check_rows` cases, and `sibling_header` case handling.
  - `hmac_rewrap_tests`: on the 1.10 fixture.
  - `cstring_tests`: non-UTF-8 is owned and round-trips.
- **Measured on this PC:** 431 passed, 48 ignored, with the install and fixtures; the same 431 pass
  with neither. Strict clippy and the bare-default build are clean. The C ABI live tests (251) all
  still get `OK` on the 2.06 tables, DLC merges included. Python, after `maturin develop --release`:
  - live iteminfo round-trips body and header byte for byte (7,068 rows, every name present);
  - the English PALOC parses (222,887 entries).
- **CrimsonAtomtic** (read-only look at its P/Invoke calls):
  - It loads every table with its header (`_with_header`, `_with_pabgh` or the `(pabgb, pabgh)`
    loaders), so the new checks change nothing on 2.06.
  - New exports: `crimson_save_set_allow_hmac_rewrap`, and the error code −28.
  - A save that fails HMAC can no longer be written until the editor opts in. It should surface
    `HMAC_UNVERIFIED` rather than treat it as a write failure.
  - A loader can now return `BODY_PARSE` where it used to return an empty handle.
  - No existing signature changed.

## 2026-11-28 — Fork guards drop the upstream remote

- `scripts/setup-fork-guards.ps1` now removes the `upstream` remote instead of keeping it with a
  disabled push URL. The `potter420/crimson-rs` parent was last pushed on 2026-04-25 and nothing is
  fetched from it, so the remote only offered a wrong target. The summary prints `STILL PRESENT` if
  the removal did not happen. `CLAUDE.md` says the same.
- **Checked on this PC:**
  - `gh repo set-default` resolves to `bbfox0703/crimson-rs`, and `gh pr create --dry-run` calls
    only `bbfox0703` APIs.
  - The pre-push hook refuses both the https and the SSH potter420 URLs and passes origin. GitHub
    also answers 403: this account cannot write to the parent.
  - The parent has no PRs from `bbfox0703`.
  - This clone's `pre-push` was a Git LFS hook, which kept the fork-guard hook from installing. It is
    saved as `.git/hooks/pre-push.lfs-backup`, and the repo tracks no LFS files. The other PC may
    have the same; the script's summary says `NOT installed` when it does.
- **Not done:** the fork is still attached on GitHub, so the web UI still offers the parent as a PR
  base. Self-serve "Leave fork network" is permanent and drops the 128 PR pages this log refers to
  by `#N`. Support-assisted detachment keeps them. That is the maintainer's call.
- **Other PC:** pull, then re-run the script once.
- **Tests:** none affected. **CrimsonAtomtic:** nothing to do.

## 2026-11-28 — Audit P6: archive builder and flags, bridge detection, save layout and rollback, scripts

The sixth batch, on the same branch as the entries below: the low / nit code findings left after P5.
Status table after it: 89 fixed, 19 partly, 37 open (mostly nits and doc items), 2 not fixing,
2 merged.

- **Archive and crypto:**
  - `PackGroupBuilder` writes its own `encrypt_info` into the PAMT header instead of a hardcoded
    `[2,14,97]`, so a ChaCha20 pack now reads back through its own PAMT (A-04; new round-trip test).
    Sizes and counts go through checked conversions, and a trie segment longer than 255 bytes is
    refused (A-15).
  - `Papgt::add_entry` returns `io::Result` and refuses a NUL in a name and a 256th entry (A-15).
    Python raises `ValueError`.
  - ChaCha20 pack and save crypto use `try_apply_keystream`, so a counter overflow is an error, not
    a panic (A-06 / F-06). A known-answer test pins the pack cipher (A-14). How the game carries the
    counter past 2³² is still unknown; we refuse instead of guessing.
  - Presence bytes other than 0/1 and a flagged array with flag 1 and count 0 are errors. Both used to
    read and then write back different bytes (A-10 / C-10; real data has only 0 and 1).
  - A paver stamp longer than 10 bytes that is not a DLC list is an error, not a 10-byte reading with
    the DLC count in `build` (A-17). `BinaryRead` no longer panics on an offset past the end.
  - The version gate is a function: `paver::parser_supports` and the C ABI
    `crimson_parser_supports(major, minor, patch)` (A-13).
- **Bridges:**
  - Width detection on 2.06 checks that the header and body keys agree (`detect_named_pair`), so the
    first width that happens to fit no longer wins (B-08).
  - `EntrySources::next_merge` only peeks, so a failed merge no longer uses up a source number (B-10).
  - A DLC merge with an empty header returns `BODY_PARSE`. It used to scan the 2.06 body without one
    and add noise rows (D-05).
  - `crimson_iteminfo_lookup_string_key` returns `NOT_FOUND` for an empty name (C-06).
  - `artifact_by_mission` is built from sorted pairs, so the lowest ItemKey wins, and the DLL no
    longer writes to stderr (C-12).
- **Dye, prefab and save:**
  - The equal-split fallback for dye colour records applies only when the span is exactly
    `count × 8` (E-02).
  - The prefab join reports an extraction failure instead of caching it as "no prefab" and falling
    back to a same-named base prefab during a DLC merge (E-05). A DLC merge still uses the PAMT
    parsed at load time.
  - `encode_body` checks that the blocks are back to back, from the end of the TOC to `stream_size`,
    before it relocates anything (F-11). The reader still checks bounds only.
  - A length-changing mutation that panics restores the blocks it backed up, then resumes the panic
    (G-05).
- **Python writers:**
  - `serialize_papgt` from a dict refuses overlapping or out-of-range name offsets and NUL in names
    (H-05).
  - The iteminfo writers re-parse each written row and raise `ValueError` when one no longer reads
    back as one item. Example: `equip_type_info` changed without its conditional field (H-12). They
    do not recompute the condition.
- **Scripts and tests:**
  - `test_full_roundtrip` and the item_info tests read iteminfo from the live install, not from
    whatever `export_for_ce.py` last left in `out\` (I-08).
  - `update_ce_table.py`: an empty `inventory_info_list` no longer raises `IndexError`, the
    `--props-bin` example no longer names a version the parser cannot read, and `read_paver` honours
    `CRIMSON_GAME_ROOT` (I-17).
  - `setup-fork-guards.ps1` stops when `gh` or `git` fails, and its summary says when it left an
    existing `pre-push` hook alone (I-19). The hook still does not cover `gh` API calls.
- **Not fixing:** D-04. The PALOC lookup by key also names rows that exist only in the DLC, so
  limiting it to keys in the base table would lose them.
- **Already fixed:** B-03, in `d541ce2` (an unknown layout now panics).
- **Found while testing:** on this PC `.git/hooks/pre-push` is a Git LFS hook, so the fork-guard hook
  has never been installed here. The repo tracks no LFS files. Until that hook is replaced, only
  `remote.pushDefault` and the disabled upstream push URL guard this clone.
- **Measured on this PC:** 426 passed, 48 ignored, with the install and fixtures; the same 426 pass
  with neither. Strict clippy and the bare-default build are clean. Python: the iteminfo and PAPGT
  round-trips are identical, and the H-05 / H-12 checks raise.
- **CrimsonAtomtic:**
  - New export: `crimson_parser_supports`.
  - `*_merge_dlc_from_bytes` with `header_len == 0` returns `BODY_PARSE`.
  - `crimson_iteminfo_lookup_string_key` returns `NOT_FOUND` for rows without a name.
  - The prefab load fails when a prefab cannot be extracted, instead of leaving those items empty.
  - A save whose blocks are not contiguous is refused on write.
  - No existing C signature changed.

## 2026-11-28 — Audit P5: reader bounds, 2.06 header checks, save FFI, Python writers, test data

The fifth batch, on the same branch as the entries below: the medium findings left after P1–P4 and
the low-severity code bugs. Status table after it: 69 fixed, 16 partly, 61 open (low / nit), 1 not
fixing, 2 merged.

- **Binary readers:**
  - Unknown PAMT compression / crypto nibbles are errors instead of reading as "none" (A-05).
  - The chunk-bounds sum is u64 and `check_remaining` uses `checked_add` (A-08).
  - A negative PAPGT name-buffer length is rejected (A-09).
  - `CArray` reserves at most what the remaining bytes can hold (A-11 / C-07).
  - `from_legacy_layout` checks its widths and finds row ends by binary search, not O(n²) (B-09).
  - Skill format detection checks ranges before slicing (C-05).
  - Object-list headers are bounds-checked to their 18 bytes first; the marker variant checked +17
    and sliced +18 (F-07).
  - `scalar_from_bytes` keeps a wrong-length slice as raw bytes (F-12).
- **2.06 header consistency:** iteminfo's header rows must tile the body (C-08; stringinfo already
  checked every offset), and `to_legacy_layout` refuses a row whose header and body keys disagree
  (B-06 / C-09; 0 such rows in 2.06).
- **Save FFI:**
  - In a deferred batch, a just-toggled scalar is length-checked against its schema size, not a stale
    0..0 range (G-08).
  - `crimson_save_write_to_file` writes a temp file, syncs, then renames (G-12).
  - Transplant clones the source element before borrowing the target, so copying within one save is
    sound (G-15).
  - Layout asserts for the remaining save-ABI structs (G-13).
  - faction `get_entry` zeroes `out_key` (D-09).
- **Python:**
  - New `serialize_iteminfo_with_header` writes a 2.06 `(body, header)` pair, so length-changing
    edits and renames survive (H-02). Unedited, it round-trips both live files byte for byte.
  - `inspect_legacy_patches` takes the header to match 2.06 row names (H-01).
  - `serialize_pamt` recomputes the header checksum (H-03).
  - Fixed-size list writers check lengths (H-10).
  - `serialize_skillinfo` refuses a dict without `row_indices` (H-19).
- **Test helpers (I-02):** the 28 per-module `find_table_bytes` copies (every step `.ok()?`) now
  call `gamedata_layout::extract_bin`, which fails on drift. The header helpers that turned a missing
  2.06 header into a silent body-only scan fail too. −643 / +126 lines.
- **Test data off the volatile archives:**
  - The cross-version skill test reads 2.04, 2.05.01 and 2.06 skill tables from
    `crimson-rs-private/gamedata/` (`test_fixtures::gamedata`), not from `X:`. 2.04.08 and 2.05 are
    byte-identical to those, so they are not copied.
  - The private repo got the six files, a README with their hashes, and binary attributes.
  - CLAUDE.md now says: `X:` / `G:` are volatile and no test may depend on them; per-version test data
    goes into the private repo, limited to what the tested pattern needs.
- **Measured on this PC:** 419 passed, 48 ignored, with the install and fixtures; the same 419 pass
  with neither. Strict clippy, the bare-default build and the `python`-only build are clean.
- **Not established / still open:** the 61 open rows (mostly nits) and the 16 partly fixed ones in
  the status table.
- **CrimsonAtomtic:**
  - No signature changed.
  - The save is now written through a temp file and a rename.
  - A transplant within one save is now allowed.
  - Header-inconsistent 2.06 tables now return errors instead of partial data.

-----

## 2026-11-28 — Audit P4: C ABI symbol index, doc drift, dead code, Python stub; status table

The fourth batch from the audit verification's fix order, on the same branch as the entries below.

- **Status table:** [`code-audit-2026-11-28-verification.md`](code-audit-2026-11-28-verification.md)
  gains a "修正狀態總表": one row per finding with its status (fixed / partly / open / not fixing /
  merged) and the fixing commit. After this batch: 45 fixed, 17 partly, 84 open (mostly the low / nit
  items no batch has taken yet), 1 not fixing (A-07, refuted), 2 merged, plus the new findings.
- **C ABI catalog (A-18, C-14, D-11, E-14, G-16):**
  - 272 of the 311 directly exported `crimson_*` functions were named nowhere in
    [`c-abi-surface.md`](c-abi-surface.md). A "Symbol index" now lists all 373, the macro-generated
    ones included, each with its rustdoc's first sentence.
  - `repo_doc_budget::c_abi_surface_lists_every_symbol` fails on any export the doc does not
    mention. Negative-controlled: dropping one line fails it and names the symbol.
- **Doc and comment drift (C-01, C-13, D-08, F-13, I-07, I-10, I-11):**
  - The CLAUDE.md and runbook CI / test-thread notes match the current CI and the fixed TailSizeCache
    cause.
  - `.cargo/config.toml` no longer cites the deleted wheel workflow, and `toc.rs` describes
    `stream_size` correctly.
  - The stale version labels in tests are gone.
  - `CrimsonItemInfoSummary`'s comments had `_reserved` at offset 71 of 72; it is 75 of 80. Every
    field's offset is now a build-time assert.
  - The globalgameevent paloc key accepts the 1..=20 digits its length field declares. It used to
    take only 14, on a premise no snapshot held; all 2.06 keys are still 14.
- **Dead code (C-17, F-14, I-12):**
  - Module-wide and "until Phase B.2" `allow(dead_code)` hid dead code from the strict lint.
  - Removed them, and with them `SkillGroupKey`, the pre-1.13 `PrefabData` /
    `GimmickVisualPrefabData`, `into_lookup_map` and `scalar_as_i128`.
  - string_info keeps a test-only allow for its round-trip writers.
- **Python (H-13, H-14, H-15):**
  - The `.pyi` matches the runtime: six missing functions declared, `ItemInfo` corrected to its 124
    real keys, nested fields and `MergedPrefabVisualData` added, plus a `py.typed` marker.
  - `crimson_rs.__version__` and `PARSER_TARGET` (`(2, 6, 0)`) let a script catch a stale wheel.
  - `docs/api.md`'s Language table was a wrong bit set; it now matches `enums.py`. ICE/AES are marked
    as rejected, and the field count is corrected.
- **Measured on this PC:** 413 passed, 48 ignored, with the install, fixtures and archive; the same
  413 pass with none of them. Strict clippy, the bare-default build and the `c_abi`- and
  `python`-only builds all compile clean.
- **Not established / still open:** the doc items the reports list beyond the symbol catalog
  (individual function docs in D-11, E-14, F-13, G-16) and every ⬜ row in the status table.
- **CrimsonAtomtic:** nothing changes for it. Documentation and test guards only, plus
  `CrimsonItemInfoSummary` offsets now asserted at their existing values.

-----

## 2026-11-28 — Audit P3: untrusted-input bounds, live tests, script guards

The third batch from the audit verification's fix order, on the same branch as the two entries
below.

- **Trie names (A-01):** `TrieStringBuffer::get_string` recursed up the parent chain, so a
  self-parent (possible in the ≤ 2.03 absolute encoding) or a deep chain overflowed the stack and
  aborted; caching every node's full string was also quadratic memory. It now walks the chain in a
  loop and refuses more than 4,096 links or a name over 4,096 bytes, and it caches prefixes only up
  to 512 bytes. A 4,096-deep chain resolves, and one link more errors.
- **Sizes from untrusted headers (A-02, A-03, F-05, H-17):**
  - PAMT dir/file counts reserve at most what the remaining bytes hold.
  - `extract_file` checks the entry against the `.paz` length before allocating.
  - `decompress` refuses a declared size beyond LZ4's 255:1 or zlib's 1032:1 before allocating it,
    caps zlib output, and requires exactly the declared size (a short LZ4 stream used to come back
    `Ok`). `decompress_partial` and the save loader apply the same LZ4 bound.
  - Python `decompress_data(b"\0", 3, 1 << 40)` is now an error instead of a process abort.
- **DDS per-mip header (A-12):** above 32,768 px a side, 32 mips or 128 bpp, the header is not
  decoded per mip, and the size math is checked. Direct3D caps textures at 16,384 px.
- **Save TOC and nesting (F-02, F-03):**
  - A TOC count the body cannot hold is an error. It used to be a ~128 GiB `with_capacity`, followed
    by entries read out of the data section.
  - A locator child whose payload is already being decoded higher up, or nesting past 16, is refused.
    The cycle used to crash the process in 0.48 s; game saves nest at most 5 deep (21 saves measured).
- **Live tests (C-02, C-18 / I-04, I-02, I-03, I-05):**
  - A new test writes back every row of the live `iteminfo` and `iteminfo.dlc_01` (7,068 / 8,218,
    all byte-identical).
  - The cross-version skill test now reads 2.04–2.06 from the version archive
    (`CRIMSON_GAMEDATA_BIN`): all round-trip, 303–308 raw fallback each. Its old `G:` path no longer
    existed, so it had always skipped.
  - `lib.rs` tests honour `CRIMSON_GAME_ROOT`.
  - The shared `gamedata_layout` helpers panic on an unreadable PAMT, a missing static-info directory
    or a failed extraction instead of reporting "no install".
  - The save round-trip tests also cover all nine fixture saves.
- **Scripts and the doc-size test (I-13, I-14, I-16):**
  - `extract_gamedata_bin.py` refuses a non-empty version folder unless `--force`.
  - `dump_gamedata_keys.py` requires `--version`; its old default overwrote the committed 1.08
    snapshot, and runbook step 7 now shows the flag.
  - The CLAUDE.md size test counts CRLF as LF, as git stores it.
- **Measured on this PC:** 412 passed, 48 ignored, with the live install, the fixtures and the
  version archive; the same 412 pass with none of them (what CI runs). Strict clippy and the
  bare-default `cargo check` are clean.
- **Not established / still open:**
  - About 20 per-module `find_table_bytes` helpers and the PALOC helpers still turn any failure into
    "skip".
  - The F-03 exponential-time case is bounded by the nesting cap (≤ 2^16 walks) but was not timed.
  - P4 (doc and comment drift) is not started.
- **CrimsonAtomtic:** no signature changed. Corrupt or crafted archives and saves now return errors
  where they used to abort the process or come back short.

-----

## 2026-11-28 — Audit P2: dye slot count, save-edit safety net, pack helpers

The second batch from the audit verification's fix order, on the same branch as the P0/P1 entry
below.

- **Dye slot count (E-01):** `crimson_item_part_prefab_resolve_dye_slot_count` returned the first
  row's `slot_count`, which counts that mesh's sub-meshes, not dye slots. On 2.06 the two agree on
  392 of 1,703 rows (item 121013: one row, one sub-mesh, three dye slots). It now returns the number
  of `_dyeSlotNo` values the item offers, the popcount of the union of its rows' dye slot bits
  (= `crimson_item_part_prefab_lookup_dye_slot_bits`). The live test checks all 59 resolved items in
  1..1M against that popcount and pins 121013 at 3. `..._lookup_slot_count` and the wrapper's docs
  now say what they count; `DyeExtraLayer::flag` records the 2.06 values (1, 2, 19; never 0).
- **Encoder refuses an incomplete decode (F-01):** the encoder rebuilds a block from its decoded
  fields, so bytes the decoder could not place vanished. One changed absence-marker byte in the 1.10
  fixture left 809 undecoded bytes and `Body::write` returned `Ok` with a body 809 bytes short.
  `encode_body` now fails on undecoded ranges, `Unknown` fields, placeholder list elements and
  locator children away from their wrapper (which the decoder counted as decoded). The C ABI maps
  that to `MUTATION_INVALID` with rollback. All 30 fixture and live saves decode with none of these
  and still re-encode byte for byte; a fixture test pins both.
- **Each commit's re-decode is checked (G-02):** after a length-changing edit or a deferred
  batch's `end_*`, the re-decoded tree must decode completely, or the edit rolls back with
  `MUTATION_INVALID`. That is what the transplant docs had promised for a definition mismatch.
- **No mixed-class lists (G-11):** insert, transplant and template insert return the new
  `ELEMENT_CLASS_MISMATCH` (−27) for an element whose class differs from the list's. Measured first:
  no game save mixes classes in a list (0 of 213,171 lists in 21 saves).
- **Templates (G-10, part):** exporting a placeholder element returns `BODY_PARSE`.
- **Pack helpers (H-06 to H-09):**
  - `pack_mod` and `create_pack_group*` refuse a `group_name` already in `0.papgt`. They used to
    replace that entry. On 2.06 `0036` to `0040` are listed (the optional language groups) with no
    folder on disk, and the docs' example was `"0036"`, the vanilla French group.
  - They also refuse a name that is not four digits, so `..` or an absolute path can't escape the
    target folder.
  - The in-place writers (`create_pack_group*`, `validate_game_dir`) copy `0.papgt` to
    `0.papgt.bak-<timestamp>` first, and `validate_game_dir --dry-run` only reports.
  - A pack that fails partway removes its half-built group folder.
  - Smoke-tested on a scratch copy of the live `0.papgt`. Examples now use `"0099"`.
- **Measured on this PC:** 405 passed, 48 ignored, with the live install and fixtures; the same 405
  pass with no game data (what CI runs). Strict clippy and the bare-default `cargo check` are clean.
- **Not established / still open:**
  - The template compares a scalar's kind and size but not its type name; CRET v1 does not record
    it, so a fix needs a format bump.
  - G-10's kind-4 default and zero4-header points are not changed.
  - The Python pack helpers have no automated tests, only the smoke run above.
  - P3 and later in the verification doc are not started.
- **CrimsonAtomtic:** no signature changed. New codes −27 (`ELEMENT_CLASS_MISMATCH`, alongside −26
  from the batch below) and more `MUTATION_INVALID` on saves that do not decode fully. Its
  `ResolveDyeSlotCount` wrapper now gets the real dye slot count (nothing in its UI calls it today).
  The maintainer handles that repo separately, after this round of fixes.

-----

## 2026-11-28 — Code audit, its verification, and the P0/P1 fixes

- **Audit:** [`code-audit-2026-11-28-plan.md`](code-audit-2026-11-28-plan.md) split the crate into
  nine blocks (A–I), and one report per block lists 149 findings. A second, independent pass,
  [`code-audit-2026-11-28-verification.md`](code-audit-2026-11-28-verification.md), checked every
  finding against the code and against live data (the 2.06 install, the `X:` version archive, the
  private fixtures): 115 confirmed, 32 partly right, 1 refuted (A-07), 1 still needs a run (G-09). It
  also found one defect the audit missed (NEW-1, below) and ends with a fix order. P0 and the four P1
  items shipped here; P2 and later are not started.
- **CI runs the `c_abi` tests** (P0; A-14 / D-03 / E-08 / I-01). `cargo test --lib` without features
  compiled in 111 of the 399 tests; every gamedata bridge and the save ABI are behind `c_abi`. All of
  them pass on a machine with no install, fixtures or saves (4.6 s), so the Linux runner can take
  them. The job name, which is the required check on `main`, is unchanged.
- **Skill on 2.06 (NEW-1):** 2.06 also inlined conditions into skill. `BuffData.field_60`, a
  `u32 ConditionInfoKey` through 2.05.01, is now `00` or `01` + bytecode, and the parser kept reading
  a `u32`. 1,683 of 2,163 entries (78%) fell to the raw fallback while the byte round-trip, the
  `_probe_skill_entry_failures` probe and `validated-patches.md` all said clean. The bytecode's length
  depends on each function's argument layout, which is not decoded, so it is probed like the subclass
  tail: a length counts only if the rest of the entry then parses to its end (memoized branch points,
  no recursion on long buff lists). Measured: raw fallback 308 of 2,163, the 2.04–2.05.01 level
  (303–304). 3,018 of 3,028 decoded buffs match 2.05.01 in every other field; the other 10 are real
  2.06 changes (nested buffs inside raw tails, one longer list). The 2.06 table still round-trips byte
  for byte. The live skill test now fails above 20% raw fallback. Python gains
  `field_60_condition` and the format string `"inline_condition"`.
- **2.06 name-only bridges (D-01, D-02):** the mission, quest, stage, knowledge, gimmick, questgauge
  and sublevel `_with_header` parsers rebuilt the old body and anchor-scanned it, dropping mission
  2,444, stage 3,198, gimmick 140 and quest 1 row, and adding 2 junk stage keys. They now return the
  header's rows (`static_header::header_row_names`); a pre-2.06 pair still scans. characterinfo's
  `lookup_string_key`, `lookup_display_name` and `resolve_portrait` now match the key as given before
  stripping the cat-byte, so the 359 hashed 32-bit rows resolve (their names sit at PALOC
  `(full_key << 32) | 0x30`). No hashed key's lo24 is another row's key, so nothing that resolved
  before changes.
- **Save editing (G-01, G-03, G-04, G-06):**
  - `crimson_save_list_all_items` and `_list_field_positions` take the save lock. CrimsonAtomtic calls
    them from `Task.Run` beside bulk mutations.
  - `mutation_version` now moves with every mutation inside a deferred batch, and forward on
    abort or a failed commit. Reads see the in-progress tree, and the old "restore on abort" could
    hand a later state a number a mid-batch snapshot already carried.
  - Making a present object list present again is a no-op (it used to replace the elements with one
    default element).
  - Length-changing list edits refuse a list whose header count exceeds its decoded elements, with
    the new `LIST_PARTIALLY_DECODED` (−26).
- **Measured on this PC:** 404 passed, 48 ignored, with the live install and the private fixtures;
  the same 404 pass with no game data, which is what CI now runs. Strict clippy and the bare-default
  `cargo check` are clean.
- **Not established:** the skill condition probe accepts the first length that fits; it agrees with
  2.05.01 on every buff that could be checked, but a buff whose condition is new in 2.06 has no older
  value to check against. The full condition bytecode grammar is not decoded. The P2+ findings in the
  verification doc (dye slot count semantics, transplant/encoder safety net, `pack_mod` safety,
  untrusted-input bounds, test and doc drift) are still open.
- **CrimsonAtomtic:** no signature changed, nothing must change to keep working. It should map the
  new error code −26 (`LIST_PARTIALLY_DECODED`) where it reports save-edit failures. The bridges
  above now return more rows and more names, and snapshot readers see extra version bumps (more
  refreshes, never stale data). Refresh the vendored copy when convenient.

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
