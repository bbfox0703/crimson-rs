# scripts/ — Claude context

Engineering notes for the next session. User-facing docs live in [`README.md`](README.md). Full RE history (the long version) is in [`../docs/1.05-parser-history.md`](../docs/1.05-parser-history.md).

## Status

- **ItemInfo parser**: byte-perfect on **1.05** (6,236 items), **1.06 / 1.07** (6,253 items — 1.07 ships an identical key list to 1.06) and **1.08** (6,314 items — +61 vs 1.07; 1.08 ships three schema drifts vs 1.07, see `src/item_info/item.rs` header). `serialize_iteminfo` roundtrips every item; the pipeline (`scripts\export_for_ce.py`) runs end-to-end clean on every version.
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
    - `_probe_paloc_template_density` — per-namespace count of template-bearing PALOC entries (`{StaticInfo:Type:Key#label}`, `<br/>`, `%0`/`%1`, `[EMPTY]` sentinels). Source of the "do we need a template-resolver?" answer in [`../docs/paloc-template-survey.md`](../docs/paloc-template-survey.md). Run when expanding bridge scope beyond titles into descriptions / dialogue.
    - `_probe_inventory_save_data_schema` — dumps `InventorySaveData → _inventorylist[N] → _itemList[M]` field tree. Source-of-truth for [`crimson_save_list_inventory_items`](../src/c_abi/mod.rs); re-run after a patch if `list_inventory_items` starts mis-parsing.
    - `_probe_item_dye_data` — walks `ItemSaveData._itemDyeDataList` across the save, dumps the per-element `ItemDyeSaveData` field schema (9 fields including the PyQt5 RE-missed `_disableSymbol`) + scans 0008 for `dye*.pabgb` gamedata. Source data for [`../docs/dye-editor-scope.md`](../docs/dye-editor-scope.md).
    - `_probe_dye_gamedata_tables` + `_probe_dye_gamedata_rows` — phase-1/2 RE of the three `dye*.pabgb` tables. Dumps row-by-row schema; used as the source for [`../docs/dye-editor-scope.md`](../docs/dye-editor-scope.md)'s "Verified row schemas" tables and the bridges under `src/{dye_color_group_info,part_prefab_dye_texture_pallete_info,part_prefab_dye_slot_info}/`.
    - `_probe_item_socket_data` + `_probe_item_socket_data_anywhere` + `_probe_item_socket_data_all_slots` + `_probe_item_socket_data_full_scan` — four-phase RE of the `_socketSaveDataList` schema (slot104-focused → anywhere-in-tree → cross-save histogram → anomaly hunt). Source data for [`../docs/socket-editor-scope.md`](../docs/socket-editor-scope.md).
    - `_probe_artifact_challenge_mapping` (in [`../src/c_abi/iteminfo.rs`](../src/c_abi/iteminfo.rs)) — verifies the 1:1 invariant between `iteminfo.look_detail_mission_info` and `Challenge_SealedArtifact_*` missions. Source data for [`../docs/artifact-challenge-mapping.md`](../docs/artifact-challenge-mapping.md). Re-run after a patch to confirm the invariant still holds.
    - `_probe_iteminfo_gem_classification` (in [`../src/c_abi/iteminfo.rs`](../src/c_abi/iteminfo.rs)) — finds the iteminfo field combo (`item_type=74` + `category_info=2501`) that marks an itemkey as a gem. Source of the `crimson_iteminfo_canonical_gem_*` ABI.
    - `_scan_0008_faction_files` + `_probe_faction_pabgh_shapes` + `_probe_faction_small_tables` + `_probe_faction_paloc_chains` — four-phase RE of the faction-bridge gamedata (`factionnode.pabgb`, `factionspawndatainfo.pabgb`, `factionrelationgroup.pabgb`, plus the bonus `factiongroup` / `factionnodespawninfo` / `factionwaypoint` / `allygroupinfo` / `tribeinfo` tables). Pins the standard `u16 cnt + (u32 key, u32 off)*` PABGH for node + spawn-data, the custom `u16 cnt + (u16 key, u32 off)*` PABGH for relation-group, and the conclusion that none of these tables localize through PALOC. Re-run on a new patch to confirm shapes still hold; on a structural drift the bridges' lossy parsers return empty handles rather than panic.
    - `_probe_store_mercenary_partprefab` + `_probe_itemkey_partprefab_linkage` + `_probe_partprefab_linkage_table_scan` + `_probe_partprefab_string_linkage` — four-phase RE behind the StoreKey / MercenaryKey / `_itemKey → _partPrefabKey` bridges. Pins the `(u16 key, u32 off)*` PABGH for storeinfo, the **new `(u8 key, u32 off)*`** 5-byte PABGH variant for mercenaryinfo, and the 3-table-join linkage (iteminfo → stringinfo → partprefabdyeslotinfo). Output to `out/store_mercenary_partprefab_probe/`.
    - `_probe_niche_bridge_candidates` — schema dump for the 12 niche-bridge candidate files (house, royalsupply, crafttool + group, triggerregion, gameplayvariable, globalgameevent + group, gameadvice + group, reserveslot, region, itemgroup). Each row's `[u16/u32 key][u32 name_len][name]` leading triple plus PABGH shape (standard `(u32, u32)` vs custom `(u16, u32)`). Output to `out/niche_bridges_probe/`.
    - `_probe_save_composite_types` — surveys every `(type_name, meta_size, meta_kind, decoded_kind)` triple in a live save, focusing on composite scalars (meta_kind 0/2 with size ∉ {1,2,4,8}). Drove the F32x3 / F32x4 / U32x4 typed-decode work in 2026-05-17; confirms Transform (40 B) is the only remaining size that falls through to `ScalarValue::Bytes`. Output to `out/composite_scalar_survey/summary.txt`.

   Invoke with `cargo test --lib --features c_abi <probe_name> -- --ignored --nocapture`.
6. When the parsers are happy on the new version, **bump the rolling-release tag** in [`../.github/workflows/build.yml`](../.github/workflows/build.yml) (`tag: v1.0.<minor>.x`) so downstream sees the new wheel under the right minor. Optionally `gh release delete v1.0.<old>.x --yes` to retire the previous rolling release.
7. **Snapshot the per-table key lists.** Run [`dump_gamedata_keys.py`](dump_gamedata_keys.py) to write the new `data\gamedata-keys-<ver>\<table>.txt` directory. This is the cross-version anchor snapshot for every non-iteminfo gamedata table (skill, mission, quest, stage, gimmick, character, faction, store, mercenary, the dye triple, the niche bridges, etc. — 30 tables, ~93 K keys for 1.08). Keep it committed alongside the new `data\keys-<ver>.txt`; future-patch diffs read it the same way `keys-1.07.txt` already serves iteminfo cross-version comparisons.

### Validated patches

| Patch | Items | iteminfo parse | Skill | Notes |
|---|---|---|---|---|
| 1.03 | — | byte-perfect | byte-perfect | baseline for skill cross-version drift table |
| 1.04 | — | byte-perfect | byte-perfect | |
| 1.05 | 6,236 | 100% ok | byte-perfect | first version where iteminfo was fully RE'd |
| 1.06 | 6,253 | 100% ok | live-roundtrip test green (no dedicated re-probe) | +17 items vs 1.05; no schema drift |
| 1.07 | 6,253 | 100% ok | live-roundtrip test green | identical item key list to 1.06; save format unchanged (v2 / flags 0x0080); slot100 + slot105 full-body roundtrip idempotent |
| **1.08** | **6,314** | **100% ok** | live-roundtrip test green | +61 items vs 1.07; **three schema drifts** in iteminfo (removed `extract_additional_drop_set_info: u32`; added `is_equip_quick_slot_visible: u8` between `is_housing_only` and `quick_slot_index`; added trailing `unk_post_summon_tag: u8` inside `DockingChildData`) — see `src/item_info/item.rs` header. Save format unchanged (v2 / flags 0x0080); slot100..slot107 + slot2 all full-body roundtrip. Cross-version-diff workflow that pinned the drifts is documented under "Investigation order" step 2 below. |

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
2. **Then check for genuine schema drift.** Two workflows; pick whichever the situation calls for:
    - **Lightweight (no sibling parser) — used for the 1.07 → 1.08 RE.** Keep one copy of the *previous* version's extracted `iteminfo.pabgb` next to the new one (e.g. snapshot `out/iteminfo.pabgb` to `out/iteminfo.<old>.pabgb` before refreshing the live install). The current parser fails on the new binary; the OLD binary still parses, so `crimson_rs.parse_iteminfo_tracked` gives you the old per-item span/offset map. Then for every key in `keys.txt` that's also in the old binary, run a precise per-byte tandem walk between the two item chunks: at each mismatch, brute-force shifts in `[1..8]` bytes on each side, accept the shift that yields ≥30 consecutive matching bytes downstream, and record `(old_offset, new_offset, removed_bytes_hex, inserted_bytes_hex)`. The detected drift events identify exactly which fields were added/removed; cross-checking against three or more items (one minimal, one with populated optionals like `docking_child_data`, one with stat arrays) immediately exposes conditional schema branches. Final validation: reconstruct synthetic new-version bytes by applying the detected removals/insertions to the old item bytes and assert byte-equality with the real new bytes. This single check is what turned the 1.08 RE from "three failure clusters in the parser" into "three confirmed schema changes" without ever building a 1.07 sibling parser.
    - **Sibling-parser (for when the structural change is too large to byte-diff).** Set up the historical parser as a sibling install (recipe in [`../docs/historical-parser-setup.md`](../docs/historical-parser-setup.md)). Use the cross-version diff templates in [`archive/`](archive/) — copy, rename to the new version pair, adapt path constants. This was the workflow for the larger 1.04 → 1.05 RE; the lightweight workflow above is enough for incremental patches.
3. **Don't add new schema fields by eyeballing diff output.** Difflib is unreliable when fields are zero-padded. Use `align_<old>_<new>.py`-style cumulative-shift analysis OR the precise tandem walk above to pinpoint the exact field where bytes were inserted.
4. **For conditionally-present new fields, identify the discriminator before patching.** If only a subset of items show a drift (e.g. 1.08's `DockingChildData::unk_post_summon_tag` only materialised for the 385 items with `docking_child_data.tag = 1`), cross-tab the new field's presence against every flag/COptional/CArray-count in the parser-tracked spans of the OLD version. The discriminator is whichever column gives a perfect partition — that's the schema's conditional clause.

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
