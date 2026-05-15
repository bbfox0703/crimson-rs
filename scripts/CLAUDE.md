# scripts/ — Claude context

Engineering notes for the next session. User-facing docs live in [`README.md`](README.md). Full RE history (the long version) is in [`../docs/1.05-parser-history.md`](../docs/1.05-parser-history.md).

## Status

- **ItemInfo parser**: byte-perfect on **1.05** (6,236 items), **1.06** (6,253 items) and **1.07** (6,253 items — identical item key list to 1.06). No schema drift across the three — the same parser handles all of them. `serialize_iteminfo` roundtrips every item; the pipeline (`scripts\export_for_ce.py`) runs end-to-end clean on every version.
- **Skill parser** (`src/skill_info/`): byte-perfect roundtrip on 1.03 / 1.04 / 1.05; the `c_abi_skillinfo_live_roundtrip` test runs against the live install and is green on 1.07 (so 1.06 / 1.07 are covered too, even without a dedicated re-probe). The brute-force BuffData subclass-tail probe absorbs size drift, so unless the format flag flips, no change is expected. The brute-force probe is essential — cross-version probing in May 2026 showed 11 `type_id` sizes drift between 1.03–1.05, so the size table cannot be hardcoded.
- **CI gate** (`.github/workflows/ci.yml`): every push to `main`/`dev` and every PR runs `cargo clippy --all-targets --lib -- -D warnings` + `cargo test --lib` on Ubuntu. Branch protection on `main` requires the check before merge.

The most likely trigger for the next session of work is a new game patch — see "On a new game patch" below.

## On a new game patch

The fastest "did Pearl Abyss break anything?" loop:

1. Update the live game install. Refresh `data\keys.txt` via the CE Lua dumper (see [`../data/README.md`](../data/README.md)). Run `python scripts\export_for_ce.py`. If parser status comes back `ok=<N>  leftover=0  fail=0  no_anchor=0`, no schema drift — you're done; that `N` is the new item count.
2. Optional but cheap: run `cargo test --lib`. The iteminfo / skill roundtrip tests are pinned to the local game install and skip cleanly when the install isn't on the expected version, so a green run gives extra confidence (a red run is informational, not blocking).
3. If iteminfo regressed (`fail > 0` or `leftover > 0`), follow "Investigation order" below.
4. If skill regressed, drop a copy of the new `0008/` archive next to the previous baselines (under `BASELINES_ROOT`) and run [`archive/probe_skill_versions.py`](archive/probe_skill_versions.py) with the new version added to its `VERSIONS` list. The drift table at the bottom shows exactly which `type_id` tail sizes changed (and whether the format flag flipped). Update `src/skill_info/` accordingly — usually nothing needs changing because the brute-force probe absorbs size drift, but a new format-flag flip would need parser logic changes.
5. **Save-side probes** (run only if downstream reports the editor breaking; not part of the routine green-light loop). Five `#[ignore]` diagnostics live in [`../src/c_abi/character_info.rs`](../src/c_abi/character_info.rs) and skip cleanly when their inputs are missing:
    - `_scan_all_groups_for_portrait_like_names` (in [`../src/c_abi/paz.rs`](../src/c_abi/paz.rs)) — buckets every portrait-like filename in the install by prefix. Catches Pearl Abyss reorganising NPC asset paths.
    - `_probe_live_save_field_blocks` — walks `FieldNPCSaveData` (228) and `FieldGimmickSaveData` (4264) blocks in a live save, dumps their field-level schemas and cat-byte distributions. Catches save-block schema drift.
    - `_scan_0008_appearance_files` — lists every `*appearance*` file in `0008/0.pamt`. Catches Pearl Abyss renaming the appearance tables.
    - `_probe_character_appearance_index` — validates the pinned save→pabgh transform for `CharacterAppearanceIndexKey` against the live save + game install. See [`../docs/save-editor-keys-plan.md`](../docs/save-editor-keys-plan.md) §9 for the resume plan.
    - `_probe_abyss_gate_mapping` — builds the per-gate abyss-gate mapping (gimmickinfo + save FieldGimmickSaveData) and pins the three known `_initStateNameHash` constants the Path B editor (`CrimsonAtomtic`) uses for per-gate unlock controls. Writes `out/abyss_gate_probe/mapping.json` (gitignored). See [`../docs/abyss-gate-map.md`](../docs/abyss-gate-map.md) for the full picture.

   Invoke with `cargo test --lib --features c_abi <probe_name> -- --ignored --nocapture`.
6. When the parsers are happy on the new version, **bump the rolling-release tag** in [`../.github/workflows/build.yml`](../.github/workflows/build.yml) (`tag: v1.0.<minor>.x`) so downstream sees the new wheel under the right minor. Optionally `gh release delete v1.0.<old>.x --yes` to retire the previous rolling release.

### Validated patches

| Patch | Items | iteminfo parse | Skill | Notes |
|---|---|---|---|---|
| 1.03 | — | byte-perfect | byte-perfect | baseline for skill cross-version drift table |
| 1.04 | — | byte-perfect | byte-perfect | |
| 1.05 | 6,236 | 100% ok | byte-perfect | first version where iteminfo was fully RE'd |
| 1.06 | 6,253 | 100% ok | live-roundtrip test green (no dedicated re-probe) | +17 items vs 1.05; no schema drift |
| **1.07** | **6,253** | **100% ok** | live-roundtrip test green | identical item key list to 1.06; save format unchanged (v2 / flags 0x0080); slot100 + slot105 full-body roundtrip idempotent |

## Sanity-check on a fresh checkout / new patch

```powershell
python scripts\export_for_ce.py
```

Expect `parser status: ok=<N>  leftover=0  fail=0  no_anchor=0` where `N` matches the line count of `data\keys.txt` (the in-game item-key dump). For 1.06 / 1.07 the expected line is `ok=6,253  leftover=0  fail=0  no_anchor=0`. A non-zero `no_anchor` means `keys.txt` has entries past the real array end — see the "keys.txt structure" section below.

For a finer-grained per-cluster view if anything breaks:

```powershell
python scripts\anchor_diff.py --keys data\keys.txt --pabgb out\iteminfo.pabgb `
    --baseline out\baselines\1.04\items.jsonl --out out\anchors.json
python scripts\analyze_per_item.py --anchors out\anchors.json --pabgb out\iteminfo.pabgb
```

## Investigation order if a future patch breaks parsing

1. **Sanity-check the anchor scanner first.** [`build_items_jsonl.py`](build_items_jsonl.py) `looks_like_item_start` validates `[u32 key, u32 slen, slen identifier-bytes, u8 zero]`. If the new patch introduces longer names (`slen > 128`) or new identifier bytes, the scanner mis-anchors and downstream looks like a schema bug. Lesson from the 1.05 RE — see [`../docs/1.05-parser-history.md`](../docs/1.05-parser-history.md) Phase 3.
2. **Then check for genuine schema drift.** Set up the historical parser as a sibling install (recipe in [`../docs/historical-parser-setup.md`](../docs/historical-parser-setup.md)). Use the cross-version diff templates in [`archive/`](archive/) — copy, rename to the new version pair, adapt path constants.
3. **Don't add new schema fields by eyeballing diff output.** Difflib is unreliable when fields are zero-padded. Use `align_<old>_<new>.py`-style cumulative-shift analysis to pinpoint the exact field where bytes were inserted.

## Layout invariants

- Every item begins with `u32 key` then `u32 string_key.len` then `len` bytes of identifier content. **There is no trailing NUL on `CString`** in this codebase — what looked like a NUL in earlier docs is actually the next field's first byte (`is_blocked: u8 = 0` for almost every item). The anchor scanner exploits that incidental zero as a cheap discriminator.
- Item keys are bounded comfortably below 2^24 (a ~6-digit decimal). The `(key >> 24) == 0` check in `scan_next_item_start` is solid.
- `string_key` length in 1.05 / 1.06 ranges 2..71 bytes. The scanner uses `2..=128` for headroom. Bytes are ASCII alphanumeric / `_` / ` ` *or* UTF-8 high bytes (1.05 introduced Roman numerals Ⅲ/Ⅳ/Ⅵ in some Goblin_Merchant_* names; 1.06 added 17 items without breaking the byte set).
- `data/keys.txt` is the ground truth for "is this key actually loaded into the game." Per-version totals: 6,236 keys in 1.05 (vs 6,389 paloc 0x70 entries — paloc carries 153 extras); 6,253 keys in 1.06 (vs 6,405 paloc 0x70 entries — 152 paloc extras).
- The same key value can appear multiple times in the binary: once as the item's own `key`, again as embedded `ItemKey`-typed fields in other items (`inventory_info`, `equip_type_info`, `convert_item_info_by_drop_npc`, …), and once more as the `(key << 32) | 0x70` paloc lookup index serialized as a numeric string in `item_name.default` for the 71 dev items. The scanner's `[key, slen, identifier-content, zero]` shape check is what disambiguates real anchors from those embeds.

## keys.txt structure (what the CE Lua dumper writes)

The in-game itemKey array lives at runtime as a packed `[u32 key][u32 unk]` table. **`unk` is the byte offset of that item inside `iteminfo.pabgb`** — discovered in 1.06 by dumping the array region and watching `unk` increase monotonically by ~600 B/slot, with the final slot's `unk` matching the anchor offset of the last item in the binary exactly. Practical consequences:

- The dumper Lua (`dump_item_keys.CEA` in `Mydev-Cheat-Engine-Tables`) now **auto-terminates on `unk` monotonicity break**. Stop reasons it can emit: `unk non-monotonic at idx=N` (the array's real end), `key sentinel at idx=N (key=0xFFFFFFFF or 0)`, or `page fault`. No more manual trimming — what lands in `keys.txt` is exactly the live array.
- `_find_anchors` in `export_for_ce.py` returns `list[int | None]` and the caller emits a `no_anchor` fallback record for any key whose item is missing from `iteminfo.pabgb`. This is the safety net for an over-trimmed (or under-trimmed) `keys.txt`; on a clean dump `no_anchor=0`.
- Bonus cross-check: the offset returned by the anchor scanner for each item should match the `unk` field for that key in the live-memory array. If a future patch shows mismatch, the anchor scanner is mis-anchoring.

## Don't

- Don't reintroduce the `new_icon_path` / `ammo_mid_block` / `ItemInfoTail` (3u8 + sentinel) variant-tail model. It coincidentally round-trips on a subset of items but is fundamentally wrong; see [`../docs/1.05-parser-history.md`](../docs/1.05-parser-history.md) Phase 1.
- Don't tighten the anchor scanner's `slen` bound or `is_ident_byte` set without checking real items first — Pearl Abyss has used UTF-8 (Ⅲ/Ⅳ/Ⅵ Roman numerals) and 70+-byte names in 1.05.
- Don't remove `parse_iteminfo_lossy` or the anchor pipeline. They're the user-facing safety net for any future patch that introduces unexpected schema drift.
- Don't commit anything under `out/`, `references/samples/`, `out/baselines/`, or `.crimson_rs_*/`. Those contain extracted Pearl Abyss content and locally-built historical wheels. `.gitignore` already excludes them; double-check after edits.

## Where to find things

- Active diagnostic / production scripts → this directory ([`README.md`](README.md) has the index)
- 1.04 → 1.05 cross-version diff templates → [`archive/`](archive/)
- Skill cross-version drift probe (run on new game patches) → [`archive/probe_skill_versions.py`](archive/probe_skill_versions.py)
- Full iteminfo RE history → [`../docs/1.05-parser-history.md`](../docs/1.05-parser-history.md)
- Historical parser setup → [`../docs/historical-parser-setup.md`](../docs/historical-parser-setup.md)
- 71 dev/QA items investigation → [`../docs/paloc-71-dev-items.md`](../docs/paloc-71-dev-items.md)
- Status of downstream Python bindings (all done as of 2026-05) → [`../docs/downstream-api-gaps.md`](../docs/downstream-api-gaps.md)
