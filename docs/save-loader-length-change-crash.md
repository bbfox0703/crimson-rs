# Crimson Desert save-loader crash on length-changing edits

**Status (2026-06-06): root-caused (provisional).** Pinned to a fixed-buffer
`memcpy` overflow in the game's loader (dump below).

**Status (2026-06-09): re-opened — the trigger is more specific.** A controlled
"add sugar" A/B pair (`tests/fixtures/saves/1.10/broken_save_after_length_change/`)
shows the editor writes an `ItemSaveData` that is **not byte-faithful** to what
the game writes for the same item, and that — not "the body grew" — is what
correlates with the crash. The size-threshold framing below is **refuted** by the
new data (the game's own add-sugar grows the body *more* and still loads). See
the **2026-06-09 revision** section first. Whether the item drift triggers the
loader `memcpy` path or a separate validation is pending an in-game load test of
the two repro saves.

## 2026-06-09 revision: the editor writes a non-faithful item

A clean controlled experiment (`broken_save_after_length_change/`): same logical
edit — **add sugar (`_itemKey = 752003`) to inventory** — produced three ways:

| Save | Origin | Loads? | `save.save` |
|---|---|---|---|
| `base_sample` | pristine (another player's slot) | yes | 1,730,173 |
| `add_2_sugar_in_game` | **game-written** (bought in shop) | **yes** | 1,730,636 (**+463**) |
| `use_editor_add_sugar` | **editor-written** (crimson-rs splice) | **CTD** | 1,730,518 (**+345**) |

Decoding all three (every block round-trips byte-perfectly, `undecoded = []`):

1. **Size is not the trigger.** The game's own add-sugar grows the decompressed
   body by **+351**; the editor's by **+241**. The bigger one loads. So the old
   "the relocated body got too big / crossed a buffer" story is wrong.

2. **The game does more than add an item** — buying sugar also touches
   `StoreSaveData`, `QuestSaveData`, `FieldSaveData` (purchase side-effects) and
   deducts gold; the editor touches only `InventorySaveData`. Those side-effects
   are unrelated to the crash (an absent quest/store delta can't desync a reader).

3. **Both add exactly one `ItemSaveData`** to
   `InventorySaveData._inventorylist[1]._itemList` (count 191 → 192). The two
   added items differ:

   | field | game sugar (237 B) | editor sugar (241 B) |
   |---|---|---|
   | `[18] _maxChargeUseableCount` | **absent** | **present = 65536** |
   | `[4] _stackCount` | 2 | 1 |
   | `[23] _isNewMark` | 0 | 1 |
   | `_slotNo` | 164 | 220 |
   | `_chargedUseableCount` | (packed value) | 0 |

   `241 − 237 = 4` = exactly field `[18]`. **The editor marks
   `_maxChargeUseableCount` present (mask bit + 4 bytes) on an item type the game
   leaves it absent for.** Field `[18]` is the **only** presence-mask difference
   between the two items (editor `[…,16,18,19,20,…]` vs game `[…,16,19,20,…]`);
   everything else is values.

   **Confirmed in the editor source.** `CrimsonAtomtic`'s
   `MainWindowViewModel.AddItemToCurrentListAsync` clones a *donor* list element
   (`ISaveLoader.ListCloneElement` — copies the donor's **whole presence mask**)
   and then only patches scalar *values* (`_itemKey`, `_stackCount`, `_slotNo`,
   `_itemNo`, `_transferredItemKey`, `_isNewMark`). It never reconciles the
   cloned **mask** to the target item's iteminfo profile, so a donor that is a
   chargeable item (has `_maxChargeUseableCount`) bleeds field `[18]` onto a
   plain consumable. The editor author's own comment there already notes "the
   game's load-time validation crashes on mask shapes that don't match the
   item's iteminfo profile" — this case is exactly that.

### Why this reframes the "game loader bug" conclusion

crimson-rs round-trips the editor save perfectly, so it is self-consistent *by
crimson-rs's schema model*. But that does not prove it is consistent with the
**game's** deserializer. If the game's `ItemSaveData` reader does not honour the
presence mask identically (e.g. a hand-written fast path), the extra 4 bytes
desync the sequential read; a few records later a `u16` length is misread (the
`0xAEBF = 44735` seen in the dump) and `memcpy`'d into the fixed stack buffer →
overflow. That single mechanism explains every observation, including why the
game's own (larger) save loads: it is perfectly in sync.

### Pending confirmation — two repro saves

Built by `src/save/sugar_probe.rs` (investigation scaffold) into
`target/sugar/repro/`, both decode cleanly + HMAC-valid:

- **A — `A_game_item_structure/`**: editor body, sugar element replaced by the
  game's *exact* item (mask + fields + values). Isolates "is a game-faithful
  item, spliced by crimson-rs, loadable?"
- **B — `B_editor_minus_field18/`**: editor body, sugar with **only** field `[18]`
  removed (editor's other values kept). Isolates field `[18]` as the single var.

Load each in-game (copy `save.save` + `lobby.save` into the slot folder):

- **B loads** → field `[18]` presence is the culprit; fix = the editor must not
  set `_maxChargeUseableCount` for this item class.
- **B CTDs, A loads** → a *value* (not just field 18) matters; narrow next.
- **both CTD** → item content is *not* the cause; it is the splice/relocation →
  the original loader-buffer theory stands.

### Not compression (the "LZW" question)

The body is **LZ4 block** (not LZW), compressed *after* the body is built; the
game decompresses before deserializing, so compression cannot change the bytes
the loader sees. crimson-rs's `lz4_flex` output is **not** byte-identical to the
game's LZ4 (it is ~250 B larger on these saves) — but `use_editor_add_sugar` was
compressed by `lz4_flex` and the game decompressed it fine (it reached the body
deserializer to crash). So matching the game's exact compressor is unnecessary
and would not change the crash.

## Symptom

A heavily-progressed save, edited with a **length-changing** operation
(completing a sealed-abyss-artifact challenge, adding/removing a list element,
making an absent field present, growing a dynamic array), **crashes Crimson
Desert on load**. The save deserializes far enough that the launcher logs
`End Load SaveSlotNNN`, then the game faults. Reproduced on 1.10 / 1.10.01.

In-place **scalar** edits (item counts, states, gate/flag hashes) never crash.

## Root cause: a fixed-buffer `memcpy` overflow in the game's loader

From a WER full dump of the crashing process:

```
FAILURE_BUCKET_ID: INVALID_POINTER_WRITE_c0000005_VCRUNTIME140.dll!memcpy_repmovs_amd
Exception:  c0000005 (Access violation) — WRITE to 0x0000000100570000
Faulting:   VCRUNTIME140!memcpy_repmovs_amd+0xb   →   rep movs byte ptr [rdi], byte ptr [rsi]

.ecxr:
 rcx=0000000000008e5f   ; bytes left to copy
 rdx=00000213612265a3   ; src = heap (loaded save body)
 rdi=0000000100570000   ; dest = the faulting write address
 r8 =000000000000aebf   ; total count = 0xAEBF = 44735
 rsp=000000010056de88

TEB (faulting thread):
 StackBase  = 0x0000000100570000   ; dest (rsp+0x118) is only 0x2060 (8288) bytes below the stack TOP
 StackLimit = 0x0000000100470000   ; 1 MB stack, ~8.5 KB used → NOT recursion / exhaustion
```

The game's loader reads a **16-bit length-prefixed record** from the save
stream and `memcpy`s it into a **fixed stack buffer with no bounds check**:

```
call  qword ptr [rax+18h]          ; reader.Read(&local, 2)  — read 2 bytes
movzx r8d, word ptr [rsp+58h]      ; r8 = 16-bit length from the stream = 0xAEBF
...
call  memcpy(dest = stack buf, src, r8)   ; dest has ~8 KB to the stack top → overflow
```

`r8 = 44735` is a legitimate length-prefixed blob in
`FactionSpawnStageManagerSaveData` (body offset `0x965A1` in the repro). It is
**byte-for-byte identical** in a save that loads and one that crashes, so the
data is not corrupt — the buffer is just too small for it once the body has
shifted. The game has no PDBs, so the call stack resolves to
`CrimsonDesert.exe+<offset>` (`AK::WriteBytes*` symbol names are nearest-export
noise — ignore them).

## Why it only triggers on length-changing edits

- A **length-changing** edit shifts the body layout; only then does the game's
  loader reach this overflowing path on certain saves. Smaller / less-progressed
  saves take the same edit and load fine (the buffer isn't exceeded).
- The game can grow the save **itself** (load + resave adds a `_questStateList`
  element + global-event data, +71 bytes) and that result loads — so it is not
  "the body grew" per se, it is how crimson-rs's relocation interacts with the
  game's loader path on this save class. The exact divergence could not be
  pinned without the game's symbols.
- The dump is `INVALID_POINTER_WRITE` (a plain `memcpy` past mapped memory), not
  a detected stack-cookie / GS overrun.

## What is verified correct (it is NOT a crimson-rs / editor bug)

Checked exhaustively against the repro saves (`tests/fixtures/saves/1.10/sealed-artifact-challenge/`):

- **Encoder fidelity** — a no-op decode→encode of the game's own save diffs
  **0 bytes** (after the `Bool(u8)` round-trip fix, commit history `fix(save):
  preserve raw byte for bool scalars`; previously 135 benign `0xFF→0x01` flips).
- **Offsets** — all object-locator `payload_offset`s, the block TOC, and the
  header are consistent; the HMAC verifies; the LZ4 stream decodes with the
  reference (non-`lz4_flex`) decoder.
- **Completion semantics** — the editor's "complete sealed-artifact challenge"
  writes match the engine's natural completion **exactly** (FAR `_state`→5 +
  `_completedTime`; new `_2` sub-mission at `_state=2`; catalog untouched).
  Verified against `engine-natural/before` vs `after`.

## Mitigations

- **crimson-rs**: the `Bool(u8)` round-trip fix (so a written save is byte-exact
  to the game's, modulo intended edits).
- **CrimsonAtomtic editor**: warns once per document before saving a
  length-changing (structural) edit (`ISaveLoader.HasStructuralEdit`), always
  keeps a backup. Scalar / in-place edits are unaffected and safe.
- **Practical**: prefer in-place edits; treat length-changing features as
  "may not load on some saves."

## If someone wants to actually fix the load

It is in the game binary, so crimson-rs/editor can't patch it. The only path to
making length-changing edits safe on these saves would be to reverse the game's
deserializer enough to reproduce its **exact** layout/relocation so the loader's
reader never lands on the oversized record's length at the wrong spot — large,
no-symbols RE. The repro saves + this dump context are the starting point.
