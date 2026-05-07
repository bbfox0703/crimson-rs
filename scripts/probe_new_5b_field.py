"""Across every 1.04/1.05 item pair, find the 5-byte block inserted after
`look_detail_mission_info` and tally its values. The point is to see
whether this new field is a `u32 + u8`, `u8 + u32`, all-zero padding, etc."""

from __future__ import annotations

import difflib
import json
import struct
import sys
from collections import Counter
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent


def _is_ident(b):
    return (
        b == ord("_")
        or b == ord(" ")
        or 48 <= b <= 57
        or 65 <= b <= 90
        or 97 <= b <= 122
    )


def _looks(data, off, key):
    if off + 12 > len(data):
        return False
    k, slen = struct.unpack_from("<II", data, off)
    if k != key or not (2 <= slen <= 64) or off + 8 + slen + 1 > len(data):
        return False
    sk = data[off + 8 : off + 8 + slen]
    return all(_is_ident(b) for b in sk) and data[off + 8 + slen] == 0


def find_anchors(data, keys):
    if not _looks(data, 0, keys[0]):
        sys.exit(f"first key {keys[0]} not at offset 0")
    anchors = [0]
    for i in range(1, len(keys)):
        cursor = anchors[-1] + 60
        target = struct.pack("<I", keys[i])
        found = -1
        while cursor + 12 <= len(data):
            idx = data.find(target, cursor)
            if idx < 0:
                break
            if _looks(data, idx, keys[i]):
                found = idx
                break
            cursor = idx + 1
        if found < 0:
            sys.exit(f"anchor failed at i={i} key={keys[i]}")
        anchors.append(found)
    return anchors


def main():
    data104 = (REPO / "out/baselines/1.04/iteminfo.pabgb").read_bytes()
    keys104 = []
    sk104_to_idx = {}
    with (REPO / "out/baselines/1.04/items.jsonl").open(encoding="utf-8") as fh:
        for line in fh:
            d = json.loads(line)
            if "key" in d:
                keys104.append(d["key"])
    anchors104 = find_anchors(data104, keys104)
    for i, off in enumerate(anchors104):
        slen = struct.unpack_from("<I", data104, off + 4)[0]
        sk = data104[off + 8 : off + 8 + slen].decode("utf-8", errors="replace")
        sk104_to_idx[sk] = i

    data105 = (REPO / "out/iteminfo.pabgb").read_bytes()
    keys105 = []
    for raw in (REPO / "data/keys.txt").read_text(encoding="utf-8").splitlines():
        s = raw.strip()
        if not s:
            continue
        try:
            k = int(s)
        except ValueError:
            break
        if k in (0, 0xFFFFFFFF):
            break
        keys105.append(k)
    anchors105 = find_anchors(data105, keys105)

    sk105_to_idx = {}
    with (REPO / "out/items.jsonl").open(encoding="utf-8") as fh:
        for line in fh:
            d = json.loads(line)
            if "string_key" in d and "_index" in d:
                sk105_to_idx[d["string_key"]] = d["_index"]

    # For every common (sk) item, run difflib and find the 5-byte insert
    # that's NOT the icon_path_alt one. Tally values.
    nonzero_examples = []
    val_counter: Counter = Counter()
    n_paired = 0
    for sk, i104 in sk104_to_idx.items():
        if sk not in sk105_to_idx:
            continue
        n_paired += 1
        off104 = anchors104[i104]
        end104 = anchors104[i104 + 1] if i104 + 1 < len(anchors104) else len(data104)
        c104 = data104[off104:end104]
        i105 = sk105_to_idx[sk]
        off105 = anchors105[i105]
        end105 = anchors105[i105 + 1] if i105 + 1 < len(anchors105) else len(data105)
        c105 = data105[off105:end105]

        s = difflib.SequenceMatcher(a=c104, b=c105, autojunk=False)
        # Collect non-icon_path_alt insertions (ignore the first 5 that are at <0x100)
        inserts = []
        for tag, i1, i2, j1, j2 in s.get_opcodes():
            if tag == "insert" and j2 - j1 == 5:
                inserts.append((j1, c105[j1:j2]))
        # Skip the first insert if it's at the icon_path_alt position (low offset)
        # Keep insertions at higher offsets (the new 5-byte field we're hunting)
        for j1, blob in inserts:
            if j1 < 0x100:
                continue  # skip icon_path_alt insert
            val_counter[blob] += 1
            if any(b != 0 for b in blob) and len(nonzero_examples) < 25:
                nonzero_examples.append((sk, j1, blob))

    print(f"paired items: {n_paired}")
    print()
    print("Top blobs (by count):")
    for blob, c in val_counter.most_common(20):
        hex_blob = " ".join(f"{b:02X}" for b in blob)
        print(f"  {c:5d}  {hex_blob}")
    print()
    print(f"nonzero examples ({len(nonzero_examples)}):")
    for sk, j1, blob in nonzero_examples:
        hex_blob = " ".join(f"{b:02X}" for b in blob)
        # Interpret bytes
        u32_le_first = int.from_bytes(blob[:4], "little")
        u8_last = blob[4]
        u8_first = blob[0]
        u32_le_last = int.from_bytes(blob[1:5], "little")
        print(
            f"  off=0x{j1:04X}  {hex_blob}  "
            f"u32+u8={u32_le_first}+{u8_last}  "
            f"u8+u32={u8_first}+{u32_le_last}  "
            f"{sk}"
        )


if __name__ == "__main__":
    main()
