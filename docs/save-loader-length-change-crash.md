# Crimson Desert save-loader crash on length-changing edits

**Status (2026-06-06): root-caused.** Game-side bug, not a crimson-rs / editor
data bug. Captured here because it cost a long investigation and constrains
what save edits are safe to ship.

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
