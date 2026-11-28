# Runtime item objects

Moved from `scripts/CLAUDE.md` on 2026-11-28, unedited except for the heading level and links.

## Runtime item objects, and why the CE table does not read them (2.05.01, 2026-11-22; re-checked on 2.06)

**Question:** the CE table's amount scripts need each item's stack limit and inventory tab. Could they read those from the running game instead of from the "Item data table" record that `update_ce_table.py` generates? **Answer: they could, but they don't, by decision.**

**What was measured** (2.05.01, read-only `ReadProcessMemory`, all 8,379 items against `out/items.jsonl`; re-run with [`probe_runtime_items.py`](../scripts/probe_runtime_items.py)):

- The maintainer reached the key array two ways in CE: the data AOB `98 08 00 00 ?? ?? 00 00 ?? ?? ?? ?? ?? ?? 00 00 ?? ?? ?? ?? ?? ?? ?? ?? 51 C3 00 00 …` (keys 2200 and 50001 at a 24-byte stride), and the pointer path `["CrimsonDesert.exe"+071D2040] → +0 → +0x20 → +0`. `exe+71D2040` is an array of 32-byte entries whose element 0 holds the `pa::ItemInfoManager` instance, and that manager's `+0x20` is the key array. Code that walks that list: `exe+D6384B0` (the registration routine that grows it), `exe+23C5D00`, `exe+54FC5B6`.
- Key array: the 24-byte element layout above, in CE order (0 of 8,379 keys out of order). Each element's `u64` is a runtime item object, 0x280 apart.
- Object offsets that equal the iteminfo value on **every** item:
  - `+0x08` u32 key (it also appears at `+0x9C` and `+0xBC`); `+0x00` on 2.06
  - `+0x10` u64 `max_stack_count`
  - `+0x19` u8 `item_importance_kind`
  - `+0x248` u8 = `inventory_info_list[0] − 1`, so the Quest tab (InventoryKey 5) is stored as 4, Kuku (13) as 12 and the backpack (2) as 1. `inventory_info_list` is not stored inline as the file's u16 list, and no pointer field leads to it.
- A runtime lookup would therefore be `obj = [key_array + id*24 + 8]`, with the stack limit at `[obj+10]` and "quest item" meaning `byte [obj+248] == 4`.
- **2.06 re-check** (`probe_runtime_items.py --array <addr>`, 48-byte elements, object pointer at `+0x20`): 0 of 8,379 keys out of order, objects still 0x280 apart, and `+0x10` / `+0x19` / `+0x248` still match every item; only the key moved to `+0x00`. The 2.06 lookup is `obj = [key_array + id*48 + 0x20]`. One wrinkle the 2.05.01 reasoning missed: the 6,907 base items the DLC table repeats now use the DLC row, yet their stack limit and inventory tab equal the base row's on every item, so the generated table (built from base rows) still matches the game.

**Why the generated table stays:**

1. Every patch already needs `update_ce_table.py` for the dropdowns and the item defines, and the table is regenerated in the same run. A runtime lookup saves no per-patch step.
2. A runtime lookup adds two things that break on patches. Finding the array: the data AOB stopped matching when 2.05 widened the element to 24 bytes, and `exe+71D2040` moves with every exe rebuild and relies on the manager being entry 0. And `+0x248` sits at the end of the object: any field added before it shifts it, and 2.05.01 had just added two `u8`s to iteminfo. `+0x10` sits early and is the most likely to hold.
3. Failure modes differ. A shifted offset makes a runtime read wrong **silently**. A stale generated table shows up in the tool's report, and an ID beyond the table is always left alone.

**Option kept on file, not built:** use the runtime read as a check, not as the source. On script enable, sample a few IDs and compare `[obj+10]` with the table's stack limit, then refuse to enable on a mismatch ("table is stale, re-run `update_ce_table.py`"). It would depend only on `+0x10` plus an AOB for the key array, which the key dumper already maintains.