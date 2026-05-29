# scripts/archive/

Reverse-engineering scripts that have outlived their immediate purpose. They're kept as **reference / templates** for the next time the game ships a structure-changing patch — copy, rename, and adapt.

These scripts are **not** wired into the production pipeline (`scripts/export_for_ce.py`). Active production / general-purpose scripts live one level up in [`../`](../). For the full story of what these scripts found, see [`../../docs/archive/1.05-parser-history.md`](../../docs/archive/1.05-parser-history.md).

## Index

### 1.04 → 1.05 cross-version diff (templates for the next patch)

When Pearl Abyss ships a new game version that changes `ItemInfo` layout, the workflow is:

1. Build the *previous* version's parser as a sibling install (see [`../../docs/historical-parser-setup.md`](../../docs/historical-parser-setup.md)).
2. Dump tracked-span offsets for representative items in the previous version.
3. Walk those spans against the new version's chunks; the byte-shift transitions pinpoint where new fields were inserted.

| Script | Purpose |
|---|---|
| [`align_104_105.py`](align_104_105.py) | Walks per-item 1.04 spans against 1.05 chunks and reports every cumulative-shift transition — pinpoints exactly which 1.04 field a new 1.05 byte block was inserted *after*. The "+5 → +10 transition right after `convert_item_info_by_drop_npc`" was the key signal that drove the 1.05 schema fix. |
| [`diff_104_105.py`](diff_104_105.py) | Side-by-side hex dump of one item's 1.04 vs 1.05 chunks with a first-diff marker. Anchors both binaries by key. |
| [`diff_104_105_full.py`](diff_104_105_full.py) | Full `difflib.SequenceMatcher` diff between paired 1.04 / 1.05 chunks; emits every insert / replace span and labels each by the 1.05 parser-tracked field at that offset. |
| [`dump_104_spans.py`](dump_104_spans.py) | Loads the historical 1.04 parser wheel from `.crimson_rs_104/` and writes every named field's offsets for selected (or all) items to `out/baselines/1.04/spans.json`. |
| [`probe_new_5b_field.py`](probe_new_5b_field.py) | Tallies one specific 5-byte insert pattern across every paired item — used to validate the `unk_pre_pattern_key u32 + unk_pre_pattern_flag u8` field hypothesis. |

### Hypotheses that turned out wrong

The 1.05 RE went down a wrong path before the 1.04-anchored cross-version diff fix. These scripts validated and then dis-proved the "variant tail" model (a `new_icon_path` CString + branched body + `ammo_mid_block` + `unk_pre_repair_*` sentinel). The model coincidentally round-tripped on items where the misread bytes matched a sentinel by chance, but failed catastrophically on the 800+ items that didn't coincide.

| Script | Purpose |
|---|---|
| [`probe_new_layout.py`](probe_new_layout.py) | Validated the (ultimately wrong) 1.05 variant tail (CString `new_icon_path` + branched body) against every anchor. |
| [`dump_post_bytes.py`](dump_post_bytes.py) | Hex-dumped bytes between end-of-`max_endurance` (per the wrong model) and end-of-chunk for items in given post-size clusters. |
| [`refine_discriminator.py`](refine_discriminator.py) | Searched for a static predicate distinguishing the 18 ammo items from misc Class B items in the old `new_icon_path == ""` branch. |
| [`debug_post31.py`](debug_post31.py) | Listed the 18 items the old runtime ammo detector matched. |

The lesson — see [`../../docs/archive/1.05-parser-history.md`](../../docs/archive/1.05-parser-history.md) for details — was that the schema is unchanged from 1.04 apart from two small documented additions, and what looked like a "variant tail" was the **anchor scanner** mis-anchoring on duplicate `key` values embedded inside other items. Don't chase schema mysteries before sanity-checking your anchors.

## When to revive a script from here

- Cross-version diff workflow (`align_104_105` / `diff_104_105*` / `dump_104_spans`): copy + rename when the next game patch changes ItemInfo layout.
- Hypotheses-that-were-wrong: don't run them. They're kept only so the failure modes are documented in code rather than rumor.
