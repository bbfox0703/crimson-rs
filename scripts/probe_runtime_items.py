"""Read-only probe: match the running game's runtime item objects against items.jsonl.

Reads (never writes) the CrimsonDesert.exe process with ReadProcessMemory:

    key array  : base header rows then DLC-only rows (see scripts/CLAUDE.md,
                 "keys.txt structure"); `--array` is its address, e.g. from the
                 CE key dumper or the data AOB `98 08 00 00 ...`. The element
                 grew with patches: 24 B on 2.05 / 2.05.01 (object pointer at
                 +0x08), 48 B on 2.06 (object pointer at +0x20); `--stride` and
                 `--object-at` select the layout (default: 2.06).
    object     : one runtime item object per element (0x280 apart).

For every item it compares the array key with `items.jsonl` (CE order), then
scans each object for offsets whose value equals a known iteminfo field on
*every* item: key, `max_stack_count`, `item_importance_kind`, and
`inventory_info_list[0]` (stored as InventoryKey - 1). Re-run it on a new
patch to see whether the offsets recorded in scripts/CLAUDE.md still hold.

Usage (the game must be running; run export_for_ce.py first):
    python -I scripts/probe_runtime_items.py --array 0x2A962770000 [--items out/items.jsonl]
        [--stride 48 --object-at 0x20]     (2.05.01: --stride 24 --object-at 0x08)
"""

from __future__ import annotations

import argparse
import ctypes
import ctypes.wintypes as wt
import json
import struct
import subprocess
import sys

OBJ_READ = 0x300

PROCESS_VM_READ = 0x0010
PROCESS_QUERY_INFORMATION = 0x0400


def open_game() -> tuple[int, object]:
    out = subprocess.run(
        ["tasklist", "/FI", "IMAGENAME eq CrimsonDesert.exe", "/FO", "CSV", "/NH"],
        capture_output=True, text=True,
    ).stdout
    if "CrimsonDesert.exe" not in out:
        sys.exit("CrimsonDesert.exe is not running")
    pid = int(out.split(",")[1].strip('"'))
    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    k32.OpenProcess.restype = wt.HANDLE
    k32.ReadProcessMemory.argtypes = [
        wt.HANDLE, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t,
        ctypes.POINTER(ctypes.c_size_t),
    ]
    h = k32.OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, False, pid)
    if not h:
        sys.exit(f"OpenProcess failed: {ctypes.get_last_error()}")

    def rd(addr: int, n: int) -> bytes | None:
        buf = ctypes.create_string_buffer(n)
        got = ctypes.c_size_t()
        ok = k32.ReadProcessMemory(h, ctypes.c_void_p(addr), buf, n, ctypes.byref(got))
        return buf.raw[:got.value] if ok else None

    return pid, rd


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--array", required=True, help="key array address (hex)")
    ap.add_argument("--items", default="out/items.jsonl")
    ap.add_argument("--stride", type=int, default=48, help="key array element size (2.06: 48)")
    ap.add_argument("--object-at", default="0x20",
                    help="offset of the object pointer in an element (2.06: 0x20)")
    args = ap.parse_args()

    items = [json.loads(line) for line in open(args.items, encoding="utf-8")]
    if "max_stack_count" not in items[0]:
        sys.exit("items.jsonl has no parsed fields; the parser must read this patch first")
    n = len(items)
    pid, rd = open_game()
    base = int(args.array, 16)
    stride, obj_at = args.stride, int(args.object_at, 0)
    arr = rd(base, n * stride)
    if arr is None or len(arr) < n * stride:
        sys.exit("could not read the key array at that address")

    ptrs, bad = [], 0
    for i, it in enumerate(items):
        key = struct.unpack_from("<I", arr, i * stride)[0]
        bad += key != it["key"]
        ptrs.append(struct.unpack_from("<Q", arr, i * stride + obj_at)[0])
    print(f"pid {pid}: {n} elements, {bad} keys out of CE order")
    deltas = {ptrs[i + 1] - ptrs[i] for i in range(min(n - 1, 64))}
    print("object spacing (first 64):", sorted(hex(d) for d in deltas))

    objs = [rd(p, OBJ_READ) for p in ptrs]
    missing = sum(o is None for o in objs)
    if missing:
        sys.exit(f"{missing} objects unreadable")

    fields = [
        ("key (u32)", "<I", lambda it: it["key"]),
        ("max_stack_count (u64)", "<Q", lambda it: it["max_stack_count"]),
        ("item_importance_kind (u8)", "<B", lambda it: it["item_importance_kind"]),
        ("inventory_info_list[0] - 1 (u8)", "<B", lambda it: (it["inventory_info_list"][0] - 1) & 0xFF),
    ]
    for name, fmt, want in fields:
        size = struct.calcsize(fmt)
        wants = [want(it) for it in items]
        hits = [
            hex(off) for off in range(OBJ_READ - size + 1)
            if all(struct.unpack_from(fmt, o, off)[0] == w for o, w in zip(objs, wants))
        ]
        print(f"{name:<34} offsets matching all {n}: {hits or 'none'}")


if __name__ == "__main__":
    main()
