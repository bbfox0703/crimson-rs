# crimson-rs — Crimson Desert Archive Toolkit

Rust library with Python bindings (via PyO3/maturin) for reading, writing, and modding Crimson Desert game archives.

# Development
- Build and install: `maturin develop` (in a venv; the project uses `.venv/`)
- Run tests: `cargo test --lib`
- Lint: `cargo clippy --all-targets --lib -- -D warnings` (must pass clean — enforced by CI on every PR and push to main/dev, see `.github/workflows/ci.yml`; `main` is branch-protected on this check)
- Workflow: develop on `dev` → PR to `main` → CI green → merge → `build.yml` rebuilds the rolling-release Windows wheel attached to the `v1.0.<minor>.x` "Development Build" GitHub release (currently `v1.0.6.x`; bump the minor in `build.yml` when Pearl Abyss ships a new patch series)

# Documentation
- `docs/save-editor-keys-plan.md` — **Active workstream as of 2026-05-14.** Roadmap and decisions log for the Save Editor key-resolver bridges (mission/quest/stage/knowledge/questgauge + checksum). Has a "🎯 New session pickup" callout at the top. Read this first if you're continuing the save-editor work.
- `docs/save-editor-keys-reference.md` — Frozen 1.06 ground-truth comparison set. Verified `(key → internal_name → PALOC title)` mappings + PALOC namespace map + user-supplied wiki/quest ground truth. Use this as the regression baseline when a future patch breaks key resolution.
- `docs/crimsonforge-feature-gaps.md` — What CrimsonForge (Python toolkit) can parse that crimson-rs doesn't yet; useful when extending coverage
- `docs/archive-format.md` — Binary format specs (PAPGT, PAMT, PAZ, trie), mod loading overlay approach
- `docs/api.md` — Full Python API reference (enums, pack_mod, PAPGT/PAMT/PAZ, ItemInfo)
- `docs/1.05-parser-history.md` — Long-form RE history of the 1.05 ItemInfo parser (read first when picking up parser-RE work)
- `docs/historical-parser-setup.md` — Recipe for building a previous-version parser as a sibling install (cross-version diff workflow)
- `docs/paloc-71-dev-items.md` — Why 71 items have no paloc translation in any of 14 languages
- `docs/downstream-api-gaps.md` — Status of the Python bindings the CrimsonGameMods app needs (all three items DONE as of 2026-05; doc kept for the cross-version notes); out-of-scope items recorded explicitly
- `scripts/CLAUDE.md` — Engineering notes for next-session pickup (sanity-check workflow, layout invariants, don'ts, **the "On a new game patch" runbook**)
- Update docs when adding new features or changing binary format understanding

# When picking up the next session
Two active workstreams — read the relevant one first, then proceed:

1. **Save Editor key resolver bridges** (active as of 2026-05-14, dev = `e379624`). Seven of nine planned items shipped: SkillKey, MissionKey, QuestKey, StageKey, KnowledgeKey, QuestGaugeKey bridges + `crimson_calculate_checksum` extern "C" wrapper. The Jenkins hash hop transform that drives Mission/Quest/Stage/Knowledge title resolution is cracked and pinned. **Start at the "🎯 New session pickup" callout at the top of [`docs/save-editor-keys-plan.md`](docs/save-editor-keys-plan.md).** Next concrete item is **#8 FieldNPC + FieldGimmick** — first half-session is investigation (locate the spawn-template `.pabgb` table); standard bridge follows if the table is clean. [`docs/save-editor-keys-reference.md`](docs/save-editor-keys-reference.md) is the regression-baseline ground truth.

2. **New game patch breaks parsing** (the longstanding fallback workstream). Core parsers (iteminfo + skill) are byte-perfect across every version probed; if a patch drifts, follow the diagnostic flow in `scripts/CLAUDE.md` ("On a new game patch"). CI gates the lint+test invariants. The 7 new save-editor bridges have similar version-drift risk — their tests skip cleanly when the live game install isn't present, so cross-version verification is `cargo test --lib --features c_abi` with the appropriate game install hooked up.

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
├── lib.rs                   # Crate root (mission/quest/stage/knowledge/questgauge
│                            #   modules gated behind #[cfg(feature = "c_abi")])
├── python.rs                # PyO3 bindings
├── binary/                  # Container formats
│   ├── pamt.rs, papgt.rs    # PAMT / PAPGT parse + write
│   ├── paz.rs               # PackGroupBuilder, compression, PAZ creation
│   ├── paloc.rs             # PALOC parse/write (numeric + symbolic keys)
│   └── trie.rs              # Trie buffer read + build (radix-compressed)
├── crypto/
│   ├── checksum.rs          # Jenkins hashlittle2 (init = 0xDEBA1DCD)
│   └── chacha20.rs          # ChaCha20 encrypt/decrypt
├── item_info/               # iteminfo.pabgb parser (byte-roundtrip)
├── skill_info/              # skill.pabgb + skill.pabgh parser (byte-roundtrip,
│                            #   BuffData tail-size brute-force probe)
├── string_info/             # u32 hash → string bridge (PAS string tables)
├── save/                    # Save file format (header + crypto + body)
└── c_abi/                   # extern "C" surface (feature-gated)
    ├── checksum.rs          # crimson_calculate_checksum (Jenkins hash hop helper)
    ├── iteminfo.rs          # ItemKey → string_key / max_stack / icon_path_hash
    ├── skill_info.rs        # SkillKey → entry name
    ├── paloc.rs             # PALOC handle + length-prefixed lookup
    ├── string_info.rs       # u32 hash → string
    ├── paz.rs               # crimson_paz_extract_file
    ├── mission_info.rs      # MissionKey → name + display title (hash hop)
    ├── quest_info.rs        # QuestKey → name + arc heading (hash hop, lo32=0x100)
    ├── stage_info.rs        # StageKey → name + title (hash hop, lo32=0x101/0x102)
    ├── knowledge_info.rs    # KnowledgeKey → name + title (hash hop, lo32=0x490)
    └── quest_gauge_info.rs  # QuestGaugeKey → name only (no PALOC chain)

# Lossy anchor-scan parsers used by the c_abi bridges live alongside in
# src/<table>_info/mod.rs. Each parser is feature-gated to the c_abi
# bridge that consumes it.
```
