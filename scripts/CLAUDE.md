# scripts/ — Claude context

Engineering notes for the next session. User-facing docs live in [`README.md`](README.md). Full RE history (the long version) is in [`../docs/archive/1.05-parser-history.md`](../docs/archive/1.05-parser-history.md).

## Status

- **ItemInfo parser**: byte-perfect on every validated patch, 1.05 through 2.06 (per-patch item counts and schema drifts: [`../docs/validated-patches.md`](../docs/validated-patches.md)). The parser reads the latest layout only. `serialize_iteminfo` roundtrips every item; the pipeline (`scripts\export_for_ce.py`) runs end-to-end clean on every version.
- **Skill parser** (`src/skill_info/`): byte-perfect on every validated patch (per-patch counts in [`../docs/validated-patches.md`](../docs/validated-patches.md)). The brute-force BuffData subclass-tail probe absorbs `type_id` tail-size drift (11 sizes drifted between 1.03 and 1.05, so the size table can't be hardcoded) — but it does **not** cover `PostBuff`, which is where the one drift it couldn't absorb landed (1.16, 589 of 2,013 entries). Two rules follow from the history:
  - A changed skill-file hash is not evidence of drift, and an unchanged one means the parser cannot have regressed — hash the old and new files in `gamedata-bin/` first, then run `_probe_skill_entry_failures` (below) to tell one odd new skill from systematic drift before starting any RE.
  - A "file not found" on a fresh patch is the container moving (2.01 renamed `skill.pabgb`/`.pabgh` → `skill.staticinfobody`/`.staticinfoheader`), not the parser breaking.
- **Archive layout**: Pearl Abyss has moved the gamedata tables once, in **2.01** — `0008/gamedata/binary__/client/bin/<t>.pabgb`/`.pabgh` → `0008/gamedata/binarystaticinfo__/bin/<t>.staticinfobody`/`.staticinfoheader`, and each language's `localizationstring_<lang>.paloc` → 39 per-namespace files under `gamedata/stringtable/binary__/<lang>/`. File *contents* were untouched. Nothing in the library hardcodes these; the resolvers are [`gamedata_layout.py`](gamedata_layout.py) (scripts) and `src/binary/gamedata_layout.rs` (`#[cfg(test)]`, for the live-install tests). Both try the newest layout first and fall back, so an older install still reads — add a new entry at the front of `BIN_LAYOUTS` / `LAYOUTS` if it moves again.
- **PALOC container (2.03)**: every `.paloc` file — all 39 namespace files of all 15 languages — is now wrapped in a fixed 0x200-byte header (`"paloc"` magic, `u32` 0, `u32` LZ4-block length, `u32` decompressed length, zero padding) followed by one LZ4 block, and the PAMT stores those entries uncompressed; the entry list inside is the unchanged ≤ 2.02 format. `binary::paloc::unwrap_container` strips it and every PALOC reader (`parse_paloc_bytes`, `crimson_paloc_load_from_bytes` / `_from_file`, the test helpers) calls it, so both layouts load. Before the fix `parse_paloc_bytes` failed with `not enough data` on every file. Pearl Abyss's LZ4 compressor is not lz4_flex's (re-compressing reproduces only 21 of 585 blocks), so a wrapped file roundtrips through its decompressed body — `wrap_paloc_bytes` / `unwrap_paloc_bytes` are the Python helpers for mod writers.
- **Archive index (2.04)**: PAMT tries use relative parents and file extensions moved to a closing table; PAPGT lost its checksum and gained a 64-byte trailer. Readers accept both layouts; writers (`PackGroupBuilder`) emit 2.04's. `dlc/dlc_01/` is a second install root with the same layout, so `--game-dir <install>/dlc/dlc_01` works for the extractor: archive it as `<ver>/dlc_01` in step 0.
- **Key lists / CE order (2.04 → 2.05)**: `keys.txt` equalled the `iteminfo.staticinfoheader` key column on 2.04, and on 2.05 the in-memory array was the base header rows followed by the `iteminfo.dlc_01` rows whose keys the base lacks; an in-game DLC item (1004792 Clams, 651st DLC-only row) showed CE ID 7718 = 7068 + 650. `export_for_ce.py` therefore builds the order from the headers by default (`--order header`) and only cross-checks `keys.txt` / `--dlc-keys`. DLC items get a `(DLC<n>)` name prefix, their names come from the DLC's own PALOC (`paloc_<lang>.<dlc>.json`), and any missing or unreadable DLC file (a folder without `0008`, no `iteminfo.<dlc>`, a base language missing from the DLC PALOC) drops every DLC and exports the base game alone. The relative order of two or more DLCs (folder name for now) is untested. On 2.06 the in-game order is still base header rows then DLC-only rows (checked against the 48-byte in-memory array, all 8,379), and the DLC items' names live in the base PALOC; the DLC PALOC no longer needs item names.
- **Row head (2.06)**: every `.staticinfoheader` became `count + ([key][u32 name len][name][u32 offset][u32 size])*` and every body row `[key][u32 row index][the 2.05.01 row after its key]`, so the names left the body ([`../src/binary/static_header.rs`](../src/binary/static_header.rs)). The count and key widths are the old ones. Scripts reading headers handle both: `export_for_ce.py` (`_read_header_rows`, names into `HEADER_NAMES`) and `dump_gamedata_keys.py` (`parse_pabgh_keys`). The base row index is the row's position; in `iteminfo.dlc_01` it is something else (167..8342 over 8,218 rows; meaning open).
- **Inlined conditions (2.06)**: `conditioninfo` rows are named by their own expression (`IsHiredMercenary(), parserType:0`) or by a label (`BattleState`, `Faction_FlagOn`), and each row body is the compiled condition bytecode followed by the expression text. 2.06 removed 2,564 of its 10,831 rows (+49 added; 8,316 left; −1.09 MB), and the removed ones are anonymous expressions (`parserType:0`). They were inlined where they were used: a `u32 ConditionInfoKey` field became `01` + the condition's bytecode (the row body minus its leading `00 00` and the trailing `[u32 len][text]`). 2,509 of the 2,547 removed conditions with ≥ 12 bytes of bytecode are found verbatim that way, in storeinfo (760), factionnode (759), levelinfo (320), dropsetinfo (192), board (185), questinfo (149), globalgameevent (111), dyecolorgroupinfo (60), characterinfo (39) and gimmickinfo (10). Skill does it too, in `BuffData.field_60`: 3,055 buffs that pointed at `CheckNone()` now carry `00`, the rest `01` + bytecode, and the 2.06 skill parser probes for the length (`SkillFormat::InlineCondition`). Named conditions stay in the table and are still referenced by key. This is why `dyecolorgroupinfo` color records grew from 8 to 22 bytes (each unlocks via `CheckKnowledge(<knowledge>)`, all 1,090 records verified) and why `characterinfo` grew by 1.43 MB. A parser that read a `ConditionInfoKey` from one of these tables now finds a variable-length inline condition instead; none of the current bridges read one (their tests pass). A future `conditioninfo` bridge would no longer find the anonymous conditions.
- **CI gate** (`.github/workflows/ci.yml`): every push to `main`/`dev` and every PR runs three jobs. `clippy + cargo test` (Ubuntu, Python 3.12 pinned) runs `cargo check --lib` for the bare default build, `--features c_abi` alone and `--features python` alone, then `cargo clippy --all-targets --lib --features c_abi,python -- -D warnings` and `cargo test --lib --features c_abi -- --test-threads=4`. `windows native dll` builds the shipped `cargo build --release --lib --features c_abi` on MSVC and fails if the dll imports anything beyond system DLLs (`dumpbin /dependents`). `cargo audit` checks `Cargo.lock` against RustSec advisories. Branch protection on `main` requires `clippy + cargo test` only. The Rust toolchain floats on stable on purpose, so a new clippy lint shows up on the next push rather than at a pin bump.

The most likely trigger for the next session of work is a new game patch — see "On a new game patch" below.

## On a new game patch

The fastest "did Pearl Abyss break anything?" loop:

0. **Snapshot the new version's gamedata bin FIRST — before anything else touches it.**

    ```powershell
    $env:CRIMSON_GAMEDATA_BIN = "X:\Crimson Desert\gamedata-bin"   # wherever the archive lives
    python scripts\extract_gamedata_bin.py --version <new> --paloc eng
    ```

    From 2.04, also keep the DLC tree (add one `--game-dir` run per `dlc/<name>`):

    ```powershell
    python scripts\extract_gamedata_bin.py --version <new>-dlc_01 --game-dir "<install>\dlc\dlc_01" --paloc eng --out "<archive>\<new>\dlc_01"
    ```

    Then append a `<new>` entry to that folder's `README.txt`. This is the
    step that makes every later question answerable, because the previous
    version's install is gone the moment Steam patches. **On 2.01 this got
    skipped and the RE went down a wrong path**: the `partprefabdyeslotinfo`
    mask widening was argued from value distributions inside the *new* file
    alone, concluding the historical 3 bytes had been kept and 9 appended.
    A tandem walk against the kept 2.00 binary — 30 seconds once the archive
    is there — showed the field had been re-encoded outright and no sub-slice
    was the old mask, which also changed the C ABI story. Reach for the
    archive before arguing from statistics; distributions describe a file,
    a diff explains it.

    The archive holds `0008`'s tables and, from 2.02 on, the English PALOC:
    `--paloc eng` writes the paloc files to `<ver>/paloc/eng/` exactly as
    they sit in the game archive (~17 MB), with a `MANIFEST.txt` of sizes,
    entry counts and SHA256s. That is what makes a patch's text changes —
    "did this quest title change?" — diffable instead of guessed at.
    Versions before 2.02 have no PALOC, and the other 14 languages are not
    kept (all 15 would be ~242 MB per version). From 2.03 the files are
    stored in the game's LZ4 container (~7 MB for English); the MANIFEST
    says so, and `parse_paloc_bytes` reads either layout.

1. Update the live game install. Refresh `data\keys.txt` via the CE Lua dumper, since 2.06 the standalone `My-Cheat-Tables\Crimson Desert\Dump in-game item key array.CEA` (no longer an entry in the CT); it writes the whole base + DLC array to one `keys.txt`, which `--keys` compares in full (see [`../data/README.md`](../data/README.md)). If its AOB stops matching, read the array with `ReadProcessMemory` instead (see "keys.txt structure"). **Before the next step, copy the previous patch's export aside** (`out\iteminfo.pabgb`, `items.jsonl`, `paloc_*.json`, `output*.txt` → `out\export-<old>\`): the exporter overwrites them, and those per-language `paloc_*.json` tables are the only previous-version item-name *text* that survives a patch, because the `gamedata-bin` archive excludes PALOC. On 2.02 that copy is what showed item names unchanged in all 15 languages. Run `python scripts\export_for_ce.py --dlc-keys <dump>\keys-dlc.txt`. If parser status comes back `ok=<N>  leftover=0  fail=0  no_anchor=0` (plus a per-source `base` / `dlc_01` line), no schema drift — you're done; `N` is base + DLC-only items. The cross-check lines should say the CE dumps agree with the header order; if one doesn't, the in-game order has moved away from the headers, which the exporter would otherwise follow silently. Then run `python scripts\update_ce_table.py --ct <CrimsonDesert.CT>` to carry the CE table over (dropdowns, item defines by key, the item data table; conventions in [`README.md`](README.md) "Refresh CE dropdown after a game patch") and read the `[!]` lines and range changes in `out\ce_update_report.txt`. While the parser can't read the new patch, add `--props-bin <archive>\<prev>` so the per-item properties come from the last version.

    If it instead dies with `directory '...' not found in 0008/0.pamt` — the way 2.01 did — the container moved, not the schema. Dump the new tree with

    ```python
    import crimson_rs
    p = crimson_rs.parse_pamt_file(r"<game>\0008\0.pamt")
    for d in p["directories"]:
        print(d["file_count"], d["path"])
    ```

    and add the new `(dir, body_ext, header_ext)` triple to the front of `BIN_LAYOUTS` in [`gamedata_layout.py`](gamedata_layout.py) and `LAYOUTS` in `src/binary/gamedata_layout.rs` (plus `PALOC_ROOT` handling if localization moved too). Everything downstream picks it up.
2. Optional but cheap: run `cargo test --lib --features c_abi -- --test-threads=4` (the thread cap is a precaution from the old skill-probe allocation, fixed in `a9d7012`). The live tests skip only when the install is absent; an install that is present but unreadable, or in a layout `gamedata_layout` does not know, fails them, and so does a skill raw-fallback rate over 20%. A red run on a new patch is the drift to look at, not noise.
3. If iteminfo regressed (`fail > 0` or `leftover > 0`), follow "Investigation order" below.
4. If skill regressed, **first run the per-entry diagnostic** — `SkillData::parse` aborts on the first bad entry, which cannot distinguish one odd new skill from systematic drift:

    ```powershell
    cargo test --lib _probe_skill_entry_failures -- --ignored --nocapture
    ```

    It prints `skill entries: N total, X ok, Y failed` plus the first 40 failures. (Backed by `skill_info::probe_entry_failures`. Note the probe can hit a huge bogus allocation on badly-misaligned entries and abort the process — just re-run; it is non-deterministic via the `TailSizeCache` hash order. **The same artifact can take down the whole `cargo test --lib` process**, not just the probe: on 1.18 two consecutive default-threaded runs ballooned past 13 GB and died with `STATUS_STACK_BUFFER_OVERRUN` after ~200 tests, then `-- --test-threads=4` ran green twice. Reach for that flag before suspecting a real regression.) A high failure count with *old* keys among them means schema drift; a single failing brand-new key means content. For 1.16 this reported 589/2013 and pointed straight at `PostBuff`, which the BuffData tail probe does not cover.

    Then drop a copy of the new `0008/` archive next to the previous baselines (under `BASELINES_ROOT`) and run [`archive/probe_skill_versions.py`](archive/probe_skill_versions.py) with the new version added to its `VERSIONS` list. The drift table at the bottom shows exactly which `type_id` tail sizes changed (and whether the format flag flipped). Update `src/skill_info/` accordingly — usually nothing needs changing because the brute-force probe absorbs size drift, but a new format-flag flip would need parser logic changes.
5. **Save-side probes** (run only if downstream reports the editor breaking; not part of the routine green-light loop): the `#[ignore]` diagnostics and what each one catches are in [`../docs/save-side-probes.md`](../docs/save-side-probes.md). Invoke with `cargo test --lib --features c_abi <probe_name> -- --ignored --nocapture`.
6. There is no publish step — `ci.yml` is the only workflow.
7. **Snapshot the per-table key lists.** Run `python scripts\dump_gamedata_keys.py --version <new>` ([`dump_gamedata_keys.py`](dump_gamedata_keys.py); `--version` is required) to write the new `data\gamedata-keys-<ver>\<table>.txt` directory. This is the cross-version anchor snapshot for every non-iteminfo gamedata table (skill, mission, quest, stage, gimmick, character, faction, store, mercenary, the dye triple, the niche bridges, stringinfo, etc.), plus `<table>.<dlc>.txt` from every installed DLC, iteminfo included (65 files and 237 K keys on 2.06; 30 tables before it). Keep it committed alongside the new `data\keys-<ver>.txt`; future-patch diffs read it the same way `keys-1.07.txt` already serves iteminfo cross-version comparisons.
8. **Move every "current version" mention to the new patch** — they are easy to miss because nothing fails when they go stale (on 2.05 the README badge still said 2.04.08 after the merge). Check each of these, then grep the repo for the previous version string and update whatever still presents it as current (rows in history tables stay):
    - `README.md`: the `Game-Crimson Desert 1.05–<ver>` badge; the "Crimson Desert 1.05–<ver>" bullet under "Game versions" and its "currently <ver>" item counts (that list holds what is true now; per-patch history goes in the tables, not there); the "row counts are <ver>" catalog list and the `stageinfo` / `questinfo` counts beside it; the language count and coverage table under the `export_for_ce` section; the "Per-patch parser (currently <ver>)" heading, the anchor link pointing at it, and a new row at the top of its table (newest first).
    - `CLAUDE.md` (repo root): the opening "Supports … currently <ver>" paragraph, the "Current layout facts" list (only what is true now), "Current: …, paver …" in the carry-forward list, and "Latest validated" under Active workstreams. The new patch's story goes in [`../docs/validated-patches.md`](../docs/validated-patches.md) (a table row, and a summary paragraph if it drifted) and a [`../docs/dev-log.md`](../docs/dev-log.md) entry, not in `CLAUDE.md`, which must stay under 40 KB.
    - `data/README.md` (the `keys.txt` row plus a row per new snapshot) and the "Past versions" table in `data/CLAUDE.md`.
    - `bin_backup\README.txt` in the version archive, and the table in [`../docs/validated-patches.md`](../docs/validated-patches.md).

### Machine notes

The maintainer works on two Windows PCs, and these notes describe where things live on them (see also "Working conventions" in the root [`CLAUDE.md`](../CLAUDE.md)).

- **Version archive** (the step-0 output): on the PC with Google Drive mounted it is `G:\我的雲端硬碟\temp\Crimson Desert\bin_backup\<ver>\`, flat bins one folder per version, `paloc\eng\` from 2.02 and a `dlc_01\` subfolder (with its own `paloc\eng\`) from 2.04. That PC has no `X:` drive, so pass `--out "G:/我的雲端硬碟/temp/Crimson Desert/bin_backup/<ver>"` to `extract_gamedata_bin.py`. Append each new version to `bin_backup\README.txt`. Older baselines (1.13.01, 1.15, 1.16, loose `iteminfo.<ver>.pabgb` copies) used other layouts and some exist only on the other PC. **On the PC with an `X:` drive** (no Google Drive) the archive is `X:\Crimson Desert\gamedata-bin\<ver>\` with its own `README.txt`: 1.03.01 through 2.06 (2.04.08 and 2.05.01 included) as of 2026-11-27. There `$env:CRIMSON_GAMEDATA_BIN = "X:\Crimson Desert\gamedata-bin"` is the default and no `--out` is needed.
- **G: is a Google Drive stream.** `ls -la` on one version folder took minutes and `du -sb` timed out; walk it with Python `os.scandir` and hash with `read_bytes` instead (~30 s for a whole version).
- **Dating the install and spotting a queued patch:** `D:\SteamLibrary\steamapps\appmanifest_3321460.acf`. `lastupdated` is the epoch of the last Steam patch and `buildid` the build; a queued patch shows `StateFlags 6` plus `TargetBuildID`, `BytesToDownload` and `ScheduledAutoUpdate`. The top-level group folders' mtimes show which groups a patch rewrote. Builds seen: 2.02 = 25246367, 2.03 = 25381195, 2.04 = 26345091, 2.05 = 26636972, 2.05.01 = 26776744, 2.06 = 26949954.
- **Saves** live under `%LOCALAPPDATA%\Pearl Abyss\CD\save\<account id>\slot<N>\save.save`; the live-save tests search that folder themselves, but several pin a slot (`slot107`, `slot103`) and the save probes take `CRIMSON_SAVE_A` / `CRIMSON_SAVE_B`. Which slot holds which patch's save differs per machine, so check the slot's mtime instead of assuming.

### Validated patches

The per-patch table (item counts, parse status, what drifted and how it was pinned) is in [`../docs/validated-patches.md`](../docs/validated-patches.md). Add the new patch's row there.

## Sanity-check on a fresh checkout / new patch

```powershell
python scripts\export_for_ce.py
```

Expect `parser status: ok=<N>  leftover=0  fail=0  no_anchor=0`, where `N` is the base item count (the per-patch value is in [`../docs/validated-patches.md`](../docs/validated-patches.md)) plus the DLC-only rows (1,311 on 2.05), and a `keys.txt: agrees with the header order` line. `no_anchor` only applies to `--order keys`, where a non-zero value means `keys.txt` has entries past the real array end — see the "keys.txt structure" section below.

For a finer-grained per-cluster view if anything breaks:

```powershell
python scripts\anchor_diff.py --keys data\keys.txt --pabgb out\iteminfo.pabgb `
    --baseline out\baselines\1.04\items.jsonl --out out\anchors.json
python scripts\analyze_per_item.py --anchors out\anchors.json --pabgb out\iteminfo.pabgb
```

## Investigation order if a future patch breaks parsing

1. **Sanity-check the anchor scanner first.** [`build_items_jsonl.py`](build_items_jsonl.py) `looks_like_item_start` validates `[u32 key, u32 slen, slen identifier-bytes, u8 zero]`. If the new patch introduces longer names (`slen > 128`) or new identifier bytes, the scanner mis-anchors and downstream looks like a schema bug. Lesson from the 1.05 RE — see [`../docs/archive/1.05-parser-history.md`](../docs/archive/1.05-parser-history.md) Phase 3.
2. **Then check for genuine schema drift.** Two workflows; pick whichever the situation calls for:
    - **Lightweight (no sibling parser) — used for the 1.07 → 1.08 RE.** Keep one copy of the *previous* version's extracted `iteminfo.pabgb` next to the new one (e.g. snapshot `out/iteminfo.pabgb` to `out/iteminfo.<old>.pabgb` before refreshing the live install). The current parser fails on the new binary; the OLD binary still parses, so `crimson_rs.parse_iteminfo_tracked` gives you the old per-item span/offset map. Then for every key in `keys.txt` that's also in the old binary, run a precise per-byte tandem walk between the two item chunks: at each mismatch, brute-force shifts in `[1..8]` bytes on each side, accept the shift that yields ≥30 consecutive matching bytes downstream, and record `(old_offset, new_offset, removed_bytes_hex, inserted_bytes_hex)`. The detected drift events identify exactly which fields were added/removed; cross-checking against three or more items (one minimal, one with populated optionals like `docking_child_data`, one with stat arrays) immediately exposes conditional schema branches. Final validation: reconstruct synthetic new-version bytes by applying the detected removals/insertions to the old item bytes and assert byte-equality with the real new bytes. This single check is what turned the 1.08 RE from "three failure clusters in the parser" into "three confirmed schema changes" without ever building a 1.07 sibling parser.
    - **Sibling-parser (for when the structural change is too large to byte-diff).** Set up the historical parser as a sibling install (recipe in [`../docs/historical-parser-setup.md`](../docs/historical-parser-setup.md)).

      **Lightest route, used for the 1.16 RE** — needed as soon as you edit the in-tree parser, because it can then no longer read the *previous* version's binary, which kills `parse_iteminfo_tracked`-based field maps on the old side:

      ```powershell
      git archive HEAD src Cargo.toml Cargo.lock pyproject.toml python .cargo | tar -x -C <tmp>
      cd <tmp>; maturin build --release -o dist
      pip install --target <tmp>\pkg --no-deps --force-reinstall <tmp>\dist\*.whl
      ```

      then `sys.path.insert(0, r"<tmp>\pkg")` before `import crimson_rs` (and `del sys.modules["crimson_rs"]` between the two imports if you need both parsers in one process). **Don't reach for `git worktree add`** on a historical commit — commits before 2026-11-27 still carry git-crypt'd save fixtures under `tests/fixtures/saves/`, and a worktree of one can fail with `external filter 'git-crypt smudge' failed`. `git archive` with an explicit path list sidesteps them entirely. (The saves now live in the private `crimson-rs-private` repo; see `src/test_fixtures.rs`.)
 Use the cross-version diff templates in [`archive/`](archive/) — copy, rename to the new version pair, adapt path constants. This was the workflow for the larger 1.04 → 1.05 RE; the lightweight workflow above is enough for incremental patches.
3. **Don't add new schema fields by eyeballing diff output.** Difflib is unreliable when fields are zero-padded. Use `align_<old>_<new>.py`-style cumulative-shift analysis OR the precise tandem walk above to pinpoint the exact field where bytes were inserted.
4. **For conditionally-present new fields, identify the discriminator before patching.** If only a subset of items show a drift (e.g. 1.08's `DockingChildData::unk_post_summon_tag` only materialised for the 385 items with `docking_child_data.tag = 1`), cross-tab the new field's presence against every flag/COptional/CArray-count in the parser-tracked spans of the OLD version. The discriminator is whichever column gives a perfect partition — that's the schema's conditional clause.

## Layout invariants

- Every item begins with `u32 key` then `u32 string_key.len` then `len` bytes of identifier content. **There is no trailing NUL on `CString`** in this codebase — what looked like a NUL in earlier docs is actually the next field's first byte (`is_blocked: u8 = 0` for almost every item). The anchor scanner exploits that incidental zero as a cheap discriminator.
- Item keys are bounded comfortably below 2^24 (a ~6-digit decimal). The `(key >> 24) == 0` check in `scan_next_item_start` is solid.
- `string_key` length in 1.05 / 1.06 ranges 2..71 bytes. The scanner uses `2..=128` for headroom. Bytes are ASCII alphanumeric / `_` / ` ` *or* UTF-8 high bytes (1.05 introduced Roman numerals Ⅲ/Ⅳ/Ⅵ in some Goblin_Merchant_* names; 1.06 added 17 items without breaking the byte set).
- `data/keys.txt` is the ground truth for "is this key actually loaded into the game." Per-version totals: 6,236 keys in 1.05 (vs 6,389 paloc 0x70 entries — paloc carries 153 extras); 6,253 keys in 1.06 (vs 6,405 paloc 0x70 entries — 152 paloc extras).
- The same key value can appear multiple times in the binary: once as the item's own `key`, again as embedded `ItemKey`-typed fields in other items (`inventory_info`, `equip_type_info`, `convert_item_info_by_drop_npc`, …), and once more as the `(key << 32) | 0x70` paloc lookup index serialized as a numeric string in `item_name.default` for the 71 dev items. The scanner's `[key, slen, identifier-content, zero]` shape check is what disambiguates real anchors from those embeds.

## keys.txt structure (what the CE Lua dumper writes)

The in-game itemKey array lives at runtime as a packed table of `[u32 key][u32 unk]` pairs (24-byte elements on 2.05 / 2.05.01: `[u32 key][u32 unk][u64 pointer][u64 0]`, the pointer stepping 0x280 per row). **2.06 widened it to 48 bytes**: `+0x00` u32 key, `+0x04` u32 (unknown), `+0x08` u64 pointer, `+0x10` u32 row offset and `+0x14` u32 row size (both as in the 2.06 header), `+0x18` u16 which table the row comes from (0 = base, 1 = `dlc_01`), `+0x20` u64 runtime item object (0x280 apart), `+0x2C` u32 0. The array ends at the first element whose object pointer is 0 (the slots after it read keys 1008379, 1008380, …). On 2.06 the 6,907 base items that `iteminfo.dlc_01` repeats point at the DLC row (table 1), so base and DLC can no longer be split from memory, and the dumper writes one file. **`unk` is the byte offset of that item inside `iteminfo.pabgb`** — discovered in 1.06 by dumping the array region and watching `unk` increase monotonically by ~600 B/slot, with the final slot's `unk` matching the anchor offset of the last item in the binary exactly. On 2.04+ the `(key, unk)` pairs equal `iteminfo.staticinfoheader` row for row. Practical consequences:

- Since 2.06 the dumper is the standalone `My-Cheat-Tables/Crimson Desert/Dump in-game item key array.CEA`; its AOB is the first 8 keys at the 48-byte stride (verified unique in writable memory on 2.06) and it writes the whole array to `keys.txt`. Through 2.05.01 it was an entry in `CrimsonDesert.CT` and **auto-terminated the base segment on an `unk` monotonicity break**. What lands in `keys.txt` is exactly the base array. With a DLC loaded, the DLC's own rows follow in `iteminfo.dlc_01` order, with `unk` restarting as offsets into that file, and go to `keys-dlc.txt`; keys the DLC overrides keep their base slot and are not repeated. Other stops: `key sentinel (0xFFFFFFFF or 0)`, pointer 0, or `page fault`.
- **When the AOB stops matching, suspect the element layout before the data.** 2.05 widened the element from 8 to 24 bytes while every key and offset in the first 3,320 rows stayed the same, so the old 8-byte-stride AOB could not match. Finding it took a read-only `ReadProcessMemory` scan of the running game for the first three header keys at every stride from 4 to 64.
- `_find_anchors` in `export_for_ce.py` returns `list[int | None]` and the caller emits a `no_anchor` fallback record for any key whose item is missing from `iteminfo.pabgb`. This is the safety net for an over-trimmed (or under-trimmed) `keys.txt`; on a clean dump `no_anchor=0`.
- Bonus cross-check: the offset returned by the anchor scanner for each item should match the `unk` field for that key in the live-memory array. If a future patch shows mismatch, the anchor scanner is mis-anchoring.

## Runtime item objects

Why the CE table reads a generated item-data table instead of the running game's item objects, and the object offsets measured on 2.05.01 and re-checked on 2.06: [`../docs/runtime-item-objects.md`](../docs/runtime-item-objects.md).

## Don't

- Don't reintroduce the `new_icon_path` / `ammo_mid_block` / `ItemInfoTail` (3u8 + sentinel) variant-tail model. It coincidentally round-trips on a subset of items but is fundamentally wrong; see [`../docs/archive/1.05-parser-history.md`](../docs/archive/1.05-parser-history.md) Phase 1.
- Don't tighten the anchor scanner's `slen` bound or `is_ident_byte` set without checking real items first — Pearl Abyss has used UTF-8 (Ⅲ/Ⅳ/Ⅵ Roman numerals) and 70+-byte names in 1.05.
- Don't remove `parse_iteminfo_lossy` or the anchor pipeline. They're the user-facing safety net for any future patch that introduces unexpected schema drift.
- Don't commit anything under `out/`, `references/samples/`, `out/baselines/`, or `.crimson_rs_*/`. Those contain extracted Pearl Abyss content and locally-built historical wheels. `.gitignore` already excludes them; double-check after edits.

## Where to find things

- Active diagnostic / production scripts → this directory ([`README.md`](README.md) has the index)
- 1.04 → 1.05 cross-version diff templates → [`archive/`](archive/)
- Skill cross-version drift probe (run on new game patches) → [`archive/probe_skill_versions.py`](archive/probe_skill_versions.py)
- Full iteminfo RE history → [`../docs/archive/1.05-parser-history.md`](../docs/archive/1.05-parser-history.md)
- Historical parser setup → [`../docs/historical-parser-setup.md`](../docs/historical-parser-setup.md)
- 71 dev/QA items investigation → [`../docs/paloc-71-dev-items.md`](../docs/paloc-71-dev-items.md)
- Status of downstream Python bindings (all done as of 2026-05) → [`../docs/archive/downstream-api-gaps.md`](../docs/archive/downstream-api-gaps.md)
