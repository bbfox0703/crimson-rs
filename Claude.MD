# crimson-rs — Crimson Desert Archive Toolkit

Rust library with Python bindings (via PyO3/maturin) for reading, writing, and modding Crimson Desert game archives.

# Development
- Build and install: `maturin develop` (in a venv; the project uses `.venv/`)
- Run tests: `cargo test --lib`
- Lint: `cargo clippy --all-targets --lib -- -D warnings` (must pass clean — enforced by CI on every PR and push to main/dev, see `.github/workflows/ci.yml`; `main` is branch-protected on this check)
- Workflow: develop on `dev` → PR to `main` → CI green → merge → `build.yml` rebuilds the rolling-release Windows wheel attached to the `v1.0.4.x` "Development Build" GitHub release

# Documentation
- `docs/archive-format.md` — Binary format specs (PAPGT, PAMT, PAZ, trie), mod loading overlay approach
- `docs/api.md` — Full Python API reference (enums, pack_mod, PAPGT/PAMT/PAZ, ItemInfo)
- `docs/1.05-parser-history.md` — Long-form RE history of the 1.05 ItemInfo parser (read first when picking up next-session work)
- `docs/historical-parser-setup.md` — Recipe for building a previous-version parser as a sibling install (cross-version diff workflow)
- `docs/paloc-71-dev-items.md` — Why 71 items have no paloc translation in any of 14 languages
- `docs/downstream-api-gaps.md` — Status of the Python bindings the CrimsonGameMods app needs (all three items DONE as of 2026-05; doc kept for the cross-version notes); out-of-scope items recorded explicitly
- `scripts/CLAUDE.md` — Engineering notes for next-session pickup (sanity-check workflow, layout invariants, don'ts, **the "On a new game patch" runbook**)
- Update docs when adding new features or changing binary format understanding

# When picking up the next session
The codebase is in a settled state — both core parsers (iteminfo + skill) are byte-perfect across every version probed, all three downstream binding gaps are closed, and CI gates the lint+test invariants. The most likely thing to bring you back is a **new game patch breaking parsing**. Start at `scripts/CLAUDE.md` ("On a new game patch") for the diagnostic flow.

# Rules
- Ask before adding new dependencies to Cargo.toml
- Binary format must roundtrip (read and write back identically)
- Custom types for binary serialization: `CString` (u32 len + len bytes — NO trailing NUL; this is what `iteminfo.pabgb` uses), `BString`-shape (u32 len + len bytes + u8 NUL terminator — used by `skill.pabgb` entry names + the PAPGT group_names_buffer; no shared Rust type yet, handled inline in `src/skill_info/mod.rs`), `optional<T>` (u8 flag + T if nonzero), `array<T>` (u32 count + count × T), `Variant` (u8 tag + match on tag)

# Binary Exploration
- Use `plcli` + `.hexpat` pattern files for binary format exploration, NOT Python scripts
- Write partial hexpat patterns, test with `plcli run -i <file> -p <pattern.hexpat> -v -d`
- Iterate: write struct, run plcli, check output, refine
- Pattern files go in `references/`

# Reference Files
- `D:\SteamLibrary\steamapps\common\Crimson Desert` — Game installation folder (hardcoded in tests; tests skip cleanly when not present)
- `out/baselines/1.05/items.jsonl` — Per-version reference data (gitignored — keep local)

# Rust Module Structure
```
src/
├── lib.rs              # Crate root
├── python.rs           # PyO3 bindings
├── binary/
│   ├── mod.rs
│   ├── pamt.rs         # PAMT parse/write
│   ├── papgt.rs        # PAPGT parse/write + add_entry (upsert, front-insert)
│   ├── paz.rs          # PackGroupBuilder, compression, PAZ creation
│   └── trie.rs         # Trie buffer read + build (radix-compressed)
├── crypto/
│   ├── mod.rs
│   ├── checksum.rs     # Jenkins hashlittle2
│   └── chacha20.rs     # ChaCha20 encrypt/decrypt
├── item_info/          # iteminfo.pabgb parser
│   ├── mod.rs
│   ├── item.rs
│   ├── keys.rs
│   └── structs.rs
└── skill_info/         # skill.pabgb + skill.pabgh parser. BuffData subclass
    ├── mod.rs          # tail size depends on type_id and is brute-force
    ├── pabgh.rs        # probed (sizes 0..500, cached per parse). Entries
    ├── buff_data.rs    # the probe can't resolve fall back to a raw blob
    └── post_buff.rs    # path that preserves bytes for byte-identical roundtrip.
```
