# Crimson Desert save-loader crash on length-changing edits

**Status (2026-06-06): root-caused (provisional).** Pinned to a fixed-buffer
`memcpy` overflow in the game's loader (dump below).

**Status (2026-06-09): FIXED.** Re-root-caused to a crimson-rs **decoder/encoder**
bug (not a game loader bug): on a length-changing edit the encoder failed to
relocate two absolute `payload_offset`s, leaving dangling pointers that crash the
game's loader (the WER-dump `memcpy` overflow is the *downstream symptom*). The
controlled "add sugar" set (`tests/fixtures/saves/1.10/broken_save_after_length_change/`)
pinned it; repro save **C** (offsets hand-relocated `+241`) **loads in-game —
confirmed by the user**. Fixed by the `prefix_00xx0100_notrailer` dynamic-array
variant in `src/save/body/decoder.rs`; regression test
`test_faction_revive_quest_no_trailer_relocates` in `src/lib.rs`. Everything in
the original "## Root cause" section below correctly describes the *crash site*;
this section describes *what put a bad pointer there*.

## 2026-06-09: the real root cause — a non-relocated `_factionNodeApplySkillList` offset

Controlled experiment (`broken_save_after_length_change/`): same logical edit —
**add sugar (`_itemKey = 752003`)** — three ways. `base_sample` (pristine, loads),
`add_2_sugar_in_game` (game-written, loads, +351 B body), `use_editor_add_sugar`
(editor-written, **CTD**, +241 B body). All three decode + round-trip
byte-perfectly.

**Hypotheses that the data REFUTES (do not chase these again):**

- **Body size / "got too big."** The game's own add-sugar grows the body **more**
  (+351 vs +241) and loads. Size is not the trigger.
- **Item field-presence (`_maxChargeUseableCount`, field 18).** The editor's sugar
  carries field 18 and the game's freshly-bought sugar does not — but field 18 is
  present on **490/490** items in `base_sample` (which loads). Its presence is
  universal and harmless. Earlier commits in this file's history blamed field 18;
  **that was wrong.**
- **`_itemNo` / `_slotNo` collision.** The editor's `_itemNo` (1000192) and
  `_slotNo` (220) do not collide with any existing item.
- **Compression ("LZW").** The body is **LZ4 block** (not LZW), decompressed
  *before* deserialize. `lz4_flex` output isn't byte-identical to the game's LZ4
  (~250 B larger) but the game decompressed `use_editor_add_sugar` fine (it
  reached the body deserializer to crash). Compression is irrelevant.

**What actually differs:** comparing every unchanged-size block base-vs-game-vs-editor
for u32s the *game* relocated but the *editor* left at the base value found exactly
**two**, both in `FactionSaveData` (block #1428):

```
FactionSaveData +39708 : base 5545036  game 5545387 (+351)  editor 5545036  (STALE)
FactionSaveData +39964 : base 5545292  game 5545643 (+351)  editor 5545292  (STALE)
```

`game − base = +351` = the game's exact body growth, and the values are valid body
offsets → these are **absolute `payload_offset`s**. The game shifted them by its
body delta; the editor left them at the base value, so after the editor's +241
shift they dangle **241 bytes short**.

**Why the encoder misses them (the bug):** both offsets sit in
`_factionNodeElementSaveDataList` elements (`FactionNodeElementSaveData`, class 98)
[401] and [403]. Those two elements have `_reviveQuestList` (field 40, a
`dynamic_array`) in a **header variant the decoder can't parse**, so the forward
field-walk **breaks** at field 40 and dumps the rest of the element — including the
next field `_factionNodeApplySkillList` (field 42, an `object_list` whose element
wrapper holds the relocatable `payload_offset`) — into the element's
`trailing_pad` as **opaque bytes** (86 / 94 B). The encoder writes `trailing_pad`
**verbatim** (`encode_inline_payload`: `out.extend_from_slice(&child.trailing_pad)`),
so the embedded `payload_offset` is **never recomputed**. Sibling elements [402]/[404],
whose `_factionNodeApplySkillList` *does* decode as an `object_list`, relocate
correctly (their wrapper `payload_offset` is recomputed by `encode_list_element_wrapper`).

**Mechanism end-to-end:** length-changing edit shifts `FactionSaveData` → the two
`payload_offset`s stay stale (−241) → on load the game follows a dangling pointer
→ sequential read desyncs → a few records later a `u16` length is misread (the
`0xAEBF = 44735` in the WER dump) → `memcpy` into the fixed stack buffer overflows.
This explains **why the game's own (larger) edit loads** (it re-serializes the
whole body, relocating every offset) and **why scalar in-place edits never crash**
(no shift → the `trailing_pad` offsets stay valid).

**Confirmation (done).** Exactly **2** broken nodes in the editor body; repro save
C (the editor save with those two `payload_offset`s patched `+241`:
5545036→5545277, 5545292→5545533, re-sealed) **loaded in-game** — the user
confirmed it. That isolates the two stale offsets as the sole cause.

**The fix (shipped) — two layers.**

1. **Decoder (`src/save/body/decoder.rs`)** — a `prefix_00xx0100_notrailer`
   `dynamic_array` variant (same `00 00 XX 01 00` header + `u32` count + data,
   ending right after the data; only taken when the trailer-present match fails,
   so the trailer-bearing arrays are untouched). The forward walk then reaches
   `_factionNodeApplySkillList`, decodes it as an `object_list`, and
   `encode_list_element_wrapper` relocates its `payload_offset` normally. This
   recovers the data for the common variant.

2. **Encoder safety net (`encode_top_level_block`)** — after emitting a block,
   `relocate_trailing_pad_offsets` rewrites any **self-referential wrapper
   offset** (a u32 whose value == its own absolute position + 4) that was copied
   verbatim from `trailing_pad`, moving it by the block's shift. This is
   independent of how the bytes decoded, so it covers **every**
   `_reviveQuestList` micro-variant — including the ones the decoder still can't
   parse (`00 00 01 00`, `00 01 01 00`) and any future shape. No-op when the
   block didn't move, so round-trips stay byte-identical.

A 22-save survey (fixtures + live install) found `_reviveQuestList` in 5 header
shapes, 2 still undecoded; layer 2 relocates **all 31,834** self-referential
wrapper offsets across every save on a shifted re-encode. Verified: golden
round-trips byte-identical; the sugar repro's 2 offsets move with the block; a
full `Body::write` add-item edit leaves **0** stale offsets. Regressions
(`src/lib.rs`): `test_faction_revive_quest_no_trailer_relocates` (decoder path,
`base_sample`) and `test_faction_node_offset_relocates_through_trailing_pad`
(encoder safety net, `engine-natural/before`, which carries an undecoded
variant).

**Double-confirmed on the original repro (`sealed-artifact-challenge/`).** The
`editor-edit/after` save (the one that first exposed this) carries **exactly 2**
stale `FactionSaveData` `payload_offset`s — found two independent ways: a raw
`before→after` self-referential scan (a correct wrapper offset always == its own
position + 4) and the fixed decoder's wrapper check both report 2. `editor-edit`
now decodes with **0** broken nodes. (This also retires the WER dump's
`FactionSpawnStageManagerSaveData` attribution — that was nearest-export noise;
the real stale offsets live in `FactionSaveData._factionNodeElementSaveDataList`.)

**Second `_reviveQuestList` variant — also covered (by layer 2).** A 22-save
survey found two header shapes the decoder still can't parse — `00 00 01 00` (16
nodes) and `00 01 01 00` (3 nodes), both 4-byte headers (verified genuine
variants, not over-reads: every upstream field aligns cleanly). The decoder
leaves their `_factionNodeApplySkillList` in `trailing_pad`, but the encoder
safety net (layer 2) relocates the buried `payload_offset` anyway, so a
length-changing edit on a save carrying these variants no longer strands them.
Confirmed by `test_faction_node_offset_relocates_through_trailing_pad` and the
all-saves survey. RE'ing the `00 XX 01 00` header into a proper decoder variant
is still worthwhile for *editability* of those quest/skill lists, but is no
longer needed for crash-safety.

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

## What was checked in the first pass (necessary but NOT sufficient)

These checks all passed and were read as "not a crimson-rs bug" — but a **no-op**
round-trip does not move any block, so it never exercises the relocation of
`payload_offset`s buried in `trailing_pad`. That blind spot is exactly the
2026-06-09 bug. Checked against the repro saves (`tests/fixtures/saves/1.10/sealed-artifact-challenge/`):

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

**Superseded by the 2026-06-09 root cause above.** This was written when the crash
looked unfixable ("it's in the game binary"). It is in fact a **crimson-rs encoder
relocation bug** — the loader's `memcpy` only overflows because the editor save
contains a dangling `payload_offset` the encoder failed to relocate. Fix it in
crimson-rs (decode `_reviveQuestList` so `_factionNodeApplySkillList` relocates, or
relocate self-referential offsets inside `trailing_pad` on a shifted block) and the
length-changing edit loads. The original "reverse the game's deserializer" plan is
unnecessary.
