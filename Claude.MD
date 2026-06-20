# crimson-rs — Crimson Desert Archive Toolkit

Rust library with Python bindings (via PyO3/maturin) for reading, writing, and modding Crimson Desert game archives. **Personal-playground fork — not intended to flow back to the `potter420/crimson-rs` upstream.** Supports Crimson Desert 1.05–1.12 across the PALOC / PAZ / PAMT containers and the save format. The `iteminfo.pabgb` parser targets the **latest patch only** — currently 1.12, byte-perfect on 6,483 items (verified on the live 1.12 install, 2026-06-19; 1.12 ships **four** iteminfo schema drifts — SubItem `type_id==16`, unconditional `unk_pre_max_endurance`, equipment/gem-gated `unk_pre_gimmick_visual` (which drove a conditional-field `=> <cond>` extension of `py_binary_struct!`), and `EnchantData` inter-element separators (`EnchantDataList`) — and **no** save-body drift, vs 1.11 — see the "Validated patches" table in [`scripts/CLAUDE.md`](scripts/CLAUDE.md)); older patches need the sibling-install recipe in [`docs/historical-parser-setup.md`](docs/historical-parser-setup.md).

This file is intentionally a **minimal index** — detailed reference lives in the linked docs.

# Development
- **Build (Python wheel):** `maturin develop` (in a venv; the project uses `.venv/`) — auto-activates the `python` feature via `[tool.maturin]` in `pyproject.toml`. **The venv must be Python 3.12** — the wheel builds `abi3` for Python ≥ 3.12, so create `.venv` from a 3.12 interpreter (`py -3.12 -m venv .venv` or `C:\Users\<you>\AppData\Local\Programs\Python\Python312\python.exe -m venv .venv`), `source .venv/Scripts/activate`, then `maturin develop --release`. Run `export_for_ce.py` / any parsing from this same venv. **Gotcha:** a stray `crimson_rs` may sit in the *global* Python 3.12 `site-packages` from an earlier `pip install`; it can be **stale** (a build predating recent parser fixes) and will silently mis-parse a new patch. After every build, `import crimson_rs` from the env you actually run, and if you maintain the global install too, refresh it with `<Python312> -m pip install --force-reinstall --no-deps target/wheels/crimson_rs-*.whl`.
- **Build (C# / native):** `cargo build --release --features c_abi` → `target/release/crimson_rs.dll` (cdylib, ~1.2 MB) **and** `crimson_rs.lib` (staticlib, ~15 MB) in one pass. PyO3 is **not** linked; [`.cargo/config.toml`](.cargo/config.toml) statically links the MSVC CRT so the dll has zero transitive DLL deps. Downstream integration recipe (LoadLibrary vs NativeAOT single-file): [`docs/c-sharp-nativeaot-integration.md`](docs/c-sharp-nativeaot-integration.md).
- **Feature flags:** `default = []` (bare pure-Rust core, no FFI; dead_code allowed crate-wide via `cfg_attr` in `src/lib.rs`), `c_abi` (extern "C" surface), `python` (PyO3 + `extension-module`, so even the wheel never link-imports `python3.dll`). The two consumer features are independent and composable — enable both for the maximum surface (what CI lints).
- **Test:** `cargo test --lib`
- **Lint:** `cargo clippy --all-targets --lib --features c_abi,python -- -D warnings` (the union-of-all-features gate; CI also runs `cargo check --lib` for the bare-default build, see `.github/workflows/ci.yml`; `main` is branch-protected on the strict clippy check).
- **Workflow:** develop on `dev` → PR to `main` → CI green → merge → `build.yml` rebuilds the rolling-release Windows wheel attached to the `v1.0.<minor>.x` "Development Build" GitHub release (currently `v1.0.12.x`; bump the minor in `build.yml` when Pearl Abyss ships a new patch series). **Fork gotcha:** this is a fork of `potter420/crimson-rs`, so `gh` defaults PRs to the *upstream* parent (they fail with "No commits between main and dev / Head ref must be a branch"). Run [`scripts/setup-fork-guards.ps1`](scripts/setup-fork-guards.ps1) **once per clone** (pins `gh repo set-default bbfox0703/crimson-rs`, `remote.pushDefault=origin`, disables `git push upstream`), or always pass `gh pr create --repo bbfox0703/crimson-rs`. Never PR/push to upstream.

# Active workstreams
1. **Save Editor key-resolver bridges** — 34 bridges shipped. Full catalog + per-module source map: [`docs/c-abi-surface.md`](docs/c-abi-surface.md). Roadmap / decisions log: [`docs/save-editor-keys-plan.md`](docs/save-editor-keys-plan.md) (start at the "🎯 New session pickup" callout). Regression baseline: [`docs/save-editor-keys-reference.md`](docs/save-editor-keys-reference.md). Remaining work is optional follow-ons only (listed in c-abi-surface.md).
2. **New game patch breaks parsing** (fallback workstream) — core parsers (iteminfo + skill) and all gamedata bridges have version-drift risk; their tests skip cleanly when the live install isn't present. Diagnostic flow + per-patch validation table: [`scripts/CLAUDE.md`](scripts/CLAUDE.md) ("On a new game patch"). Latest validated: **1.12** (four iteminfo schema drifts — SubItem `type_id==16`, `unk_pre_max_endurance`, equipment/gem-gated `unk_pre_gimmick_visual`, `EnchantData` inter-element separators — RE'd and fixed, byte-perfect roundtrip on 6,483 items; no save-body drift; verified 2026-06-19).

# Documentation
Active reference:
- [`docs/c-abi-surface.md`](docs/c-abi-surface.md) — Full C-ABI bridge catalog + per-module source map (the detail formerly inlined here).
- [`docs/save-editor-keys-plan.md`](docs/save-editor-keys-plan.md) — Active save-editor workstream roadmap (has the "🎯 New session pickup" callout).
- [`docs/save-editor-keys-reference.md`](docs/save-editor-keys-reference.md) — Frozen 1.06 ground-truth regression baseline for key resolution.
- [`docs/abyss-gate-map.md`](docs/abyss-gate-map.md) — Per-gate abyss-gate mapping + 3 pinned `_initStateNameHash` constants (Path B per-gate unlock; Phase 2 hash-decode partial).
- [`docs/socket-editor-scope.md`](docs/socket-editor-scope.md) — Socket/gem editor reference + verified `_socketSaveDataList` schema.
- [`docs/dye-editor-scope.md`](docs/dye-editor-scope.md) — Dye editor reference + the three dye gamedata bridges.
- [`docs/worldmap-plotting.md`](docs/worldmap-plotting.md) — World-map plotting (positioned-entity enumerator + affine fit + basemap tile discovery).
- [`docs/artifact-challenge-mapping.md`](docs/artifact-challenge-mapping.md) — Artifact ↔ Challenge 1:1 bidirectional mapping.
- [`docs/save-mutation-version.md`](docs/save-mutation-version.md) — Mutation-version cache-coherency contract for C-ABI snapshot consumers.
- [`docs/save-deferred-redecode.md`](docs/save-deferred-redecode.md) — Deferred-redecode batch ABI for bulk mutations.
- [`docs/save-loader-length-change-crash.md`](docs/save-loader-length-change-crash.md) — Game-side `memcpy` overflow that crashes the game's loader on length-changing edits to some saves (root-cause + dump + repro fixtures). Not a crimson-rs bug.
- [`docs/paloc-template-survey.md`](docs/paloc-template-survey.md) — Template-density per PALOC namespace (settles the template-resolver question: titles no, descriptions yes).
- [`docs/ref-gamedata/main-quest-list.md`](docs/ref-gamedata/main-quest-list.md) — Curated Chapter→Arc→Mission table (`main_quest_chapter` bridge source).
- [`docs/ref-gamedata/side-quest-list.md`](docs/ref-gamedata/side-quest-list.md) — Curated quest→faction table (`side_quest_faction` bridge source).
- [`docs/archive-format.md`](docs/archive-format.md) — Binary format specs (PAPGT, PAMT, PAZ, trie) + mod-loading overlay.
- [`docs/api.md`](docs/api.md) — Full Python API reference (enums, pack_mod, PAPGT/PAMT/PAZ, ItemInfo).
- [`docs/c-sharp-nativeaot-integration.md`](docs/c-sharp-nativeaot-integration.md) — C# NativeAOT single-file integration recipe.
- [`docs/historical-parser-setup.md`](docs/historical-parser-setup.md) — Sibling-install recipe for a previous-version parser (cross-version diff).
- [`docs/paloc-71-dev-items.md`](docs/paloc-71-dev-items.md) — Why 71 items have no PALOC translation in any of 14 languages.
- [`scripts/CLAUDE.md`](scripts/CLAUDE.md) — Next-session engineering notes, layout invariants, don'ts, the "On a new game patch" runbook + per-patch validation table.

Archived / historical (`docs/archive/`):
- [`docs/archive/1.05-parser-history.md`](docs/archive/1.05-parser-history.md) — Long-form RE history of the 1.05 ItemInfo parser (read first when picking up parser-RE work).
- [`docs/archive/globalgameevent-body-re.md`](docs/archive/globalgameevent-body-re.md) — RE of the globalgameevent per-row body (group_key + paloc_key).
- [`docs/archive/mercenaryinfo-body-re.md`](docs/archive/mercenaryinfo-body-re.md) — RE of the mercenaryinfo `(u8,u32)` PABGH variant.
- [`docs/archive/downstream-api-gaps.md`](docs/archive/downstream-api-gaps.md) — Status of the CrimsonGameMods Python bindings (all done as of 2026-05) + cross-version notes.
- [`docs/archive/crimsonforge-feature-gaps.md`](docs/archive/crimsonforge-feature-gaps.md) — What CrimsonForge (Python toolkit) can parse that crimson-rs doesn't yet.

Update docs when adding new features or changing binary-format understanding.

# Rules
- Ask before adding new dependencies to Cargo.toml.
- Binary format must roundtrip (read and write back identically).
- Custom types for binary serialization: `CString` (u32 len + len bytes — NO trailing NUL; this is what `iteminfo.pabgb` uses), `BString`-shape (u32 len + len bytes + u8 NUL terminator — used by `skill.pabgb` entry names + the PAPGT group_names_buffer; no shared Rust type yet, handled inline in `src/skill_info/mod.rs`), `optional<T>` (u8 flag + T if nonzero), `array<T>` (u32 count + count × T), `Variant` (u8 tag + match on tag).

# Binary Exploration
- Use `plcli` + `.hexpat` pattern files for binary format exploration, NOT Python scripts.
- Write partial hexpat patterns, test with `plcli run -i <file> -p <pattern.hexpat> -v -d`; iterate.
- Pattern files go in `references/`.

# Reference Files
- `D:\SteamLibrary\steamapps\common\Crimson Desert` — Game installation folder (hardcoded in tests; tests skip cleanly when not present; override with the `CRIMSON_GAME_ROOT` env var).
- `out/baselines/1.05/items.jsonl` — Per-version reference data (gitignored — keep local).

# Module map
`src/` is organized as: `binary/` (PAMT/PAPGT/PAZ/PALOC/trie/paver container formats), `crypto/` (Jenkins checksum + ChaCha20), `item_info/` + `skill_info/` + `string_info/` + `character_info/` (byte-roundtrip parsers), `save/` (save header + crypto + body), and `c_abi/` (the feature-gated extern "C" surface). The full annotated per-file map is in [`docs/c-abi-surface.md`](docs/c-abi-surface.md).
