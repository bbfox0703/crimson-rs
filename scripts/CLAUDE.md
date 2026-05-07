# scripts/ — Claude context

Engineering notes for the next session. User-facing docs live in [`README.md`](README.md). Full RE history (the long version) is in [`../docs/1.05-parser-history.md`](../docs/1.05-parser-history.md).

## Status

Crimson Desert 1.05 ItemInfo parser: **6,236 / 6,236 (100.0%) perfect parse**. `serialize_iteminfo` is byte-perfect on every parsed item. The pipeline (`scripts\export_for_ce.py`) runs end-to-end clean.

## Sanity-check on a fresh checkout / new patch

```powershell
python scripts\export_for_ce.py
```

Expect `parser status: ok=6,236  leftover=0  fail=0` (the count comes from `data\keys.txt`, which is the in-game item-key dump). For a finer-grained per-cluster view if anything breaks:

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
- `string_key` length in 1.05 ranges 2..71 bytes. The scanner uses `2..=128` for headroom. Bytes are ASCII alphanumeric / `_` / ` ` *or* UTF-8 high bytes (1.05 introduced Roman numerals Ⅲ/Ⅳ/Ⅵ in some Goblin_Merchant_* names).
- `data/keys.txt` is the ground truth for "is this key actually loaded into the game." 6,236 keys in 1.05 vs 6,389 paloc 0x70 entries (paloc carries 153 extra entries for keys not in the live game).
- The same key value can appear multiple times in the binary: once as the item's own `key`, again as embedded `ItemKey`-typed fields in other items (`inventory_info`, `equip_type_info`, `convert_item_info_by_drop_npc`, …), and once more as the `(key << 32) | 0x70` paloc lookup index serialized as a numeric string in `item_name.default` for the 71 dev items. The scanner's `[key, slen, identifier-content, zero]` shape check is what disambiguates real anchors from those embeds.

## Don't

- Don't reintroduce the `new_icon_path` / `ammo_mid_block` / `ItemInfoTail` (3u8 + sentinel) variant-tail model. It coincidentally round-trips on a subset of items but is fundamentally wrong; see [`../docs/1.05-parser-history.md`](../docs/1.05-parser-history.md) Phase 1.
- Don't tighten the anchor scanner's `slen` bound or `is_ident_byte` set without checking real items first — Pearl Abyss has used UTF-8 (Ⅲ/Ⅳ/Ⅵ Roman numerals) and 70+-byte names in 1.05.
- Don't remove `parse_iteminfo_lossy` or the anchor pipeline. They're the user-facing safety net for any future patch that introduces unexpected schema drift.
- Don't commit anything under `out/`, `references/samples/`, `out/baselines/`, or `.crimson_rs_*/`. Those contain extracted Pearl Abyss content and locally-built historical wheels. `.gitignore` already excludes them; double-check after edits.

## Where to find things

- Active diagnostic / production scripts → this directory ([`README.md`](README.md) has the index)
- 1.04 → 1.05 cross-version diff templates → [`archive/`](archive/)
- Full RE history → [`../docs/1.05-parser-history.md`](../docs/1.05-parser-history.md)
- Historical parser setup → [`../docs/historical-parser-setup.md`](../docs/historical-parser-setup.md)
- 71 dev/QA items investigation → [`../docs/paloc-71-dev-items.md`](../docs/paloc-71-dev-items.md)
