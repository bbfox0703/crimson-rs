# scripts/archive/ — Claude context

These scripts are **archived**. Don't run them, don't import from them, don't extend them. They're kept on disk because:

1. The cross-version diff family (`align_104_105.py`, `diff_104_105*.py`, `dump_104_spans.py`, `probe_new_5b_field.py`) is the **template** for the next-patch RE workflow. When the game ships a structural change, copy them, rename to the new version pair, and adapt.
2. The "hypotheses that turned out wrong" group (`probe_new_layout.py`, `dump_post_bytes.py`, `refine_discriminator.py`, `debug_post31.py`) documents a path the 1.05 RE went down and abandoned. Keep them so the failure modes are recorded in code rather than rumor.

If you're tempted to use these from the active pipeline, **stop and use the active scripts** in [`../`](../) instead. The full RE history (including why the wrong-hypothesis scripts exist) is in [`../../docs/archive/1.05-parser-history.md`](../../docs/archive/1.05-parser-history.md).

## Don't

- Don't reintroduce the `new_icon_path` / `ammo_mid_block` / `ItemInfoTail` (3u8 + sentinel) variant-tail model from `probe_new_layout.py`. It coincidentally round-trips on a subset of items but is fundamentally wrong — the schema didn't change in those places.
- Don't add new schema fields by eyeballing `diff_104_105_full.py` output without first running `align_104_105.py` to find shift transitions. Difflib is unreliable when fields are zero-padded.

## When porting these to the next version pair (e.g. 1.05 → 1.06)

1. Build the previous version's parser as a sibling install (`git worktree add` at the relevant commit, `maturin build --release`, `pip install --target=.crimson_rs_<ver>`). See [`../../docs/historical-parser-setup.md`](../../docs/historical-parser-setup.md).
2. Copy `align_104_105.py` → `align_105_106.py`, update path constants and the `.crimson_rs_<ver>` import path.
3. Same for `diff_104_105.py`, `diff_104_105_full.py`, `dump_104_spans.py`.
4. Sanity-check the anchor scanner (`scripts/build_items_jsonl.py` `looks_like_item_start`) against the new patch's `string_key` shapes (longer? non-ASCII?) **before** assuming any schema change. The 1.05 stragglers were anchor-scanner artifacts, not schema drift.
