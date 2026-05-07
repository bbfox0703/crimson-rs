"""Find what distinguishes ammo / projectile items from misc Class B items.

In the 1.05 ItemInfo layout the parser branches on `new_icon_path.length`:

    length == 0    → respawn_time_seconds (i64) + max_endurance (u16)
                     [+ optional 22-byte ammo_mid_block]
                     + trailer + repair_data_list

    length  > 0    → icon_flag + 9 zero bytes + trailer + ...

Within the length==0 branch, two sub-shapes coexist:

    "ammo-like" (18 items): the 22-byte `ammo_mid_block` is present,
    so chunk size minus end-of-`max_endurance` (`post`) is ≥ 20.

    Class B (≈2,967 items): no `ammo_mid_block`, post < 20.

The Rust parser detects ammo at runtime by peeking the `FF FF` trailer
sentinel. This script tries instead to find a clean, *static* predicate
on the parsed fields that picks out the 18 ammo items — useful if we
ever want a non-peeking discriminator.
"""
from __future__ import annotations
import argparse
import json
import struct
from collections import Counter
from pathlib import Path

import crimson_rs


SIMPLE = {
    "u8": ("<B", 1), "u16": ("<H", 2), "u32": ("<I", 4),
    "u64": ("<Q", 8), "i64": ("<q", 8), "f32": ("<f", 4),
}


def extract(chunk: bytes, r: dict):
    f = SIMPLE.get(r["ty"])
    if f:
        return struct.unpack_from(f[0], chunk, r["start"])[0]
    if r["ty"] in ("CString.len", "CArray.count"):
        return struct.unpack_from("<I", chunk, r["start"])[0]
    return None


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--anchors", required=True)
    ap.add_argument("--pabgb", required=True)
    args = ap.parse_args()

    anchors = json.loads(Path(args.anchors).read_text(encoding="utf-8"))
    data = Path(args.pabgb).read_bytes()

    # collect (post_size, max_endurance, all_feats) per item
    me0_classA = []  # max_endurance == 0 AND post >= 20 (the 18-item bucket)
    me0_classB = []  # max_endurance == 0 AND post < 20 (the 2,669-item bucket)
    me65535 = []

    for i, row in enumerate(anchors):
        start = row["offset_105"]
        size = row["size_105"]
        chunk = data[start:start + size]
        res = crimson_rs.parse_iteminfo_tracked(chunk)
        ranges = res["spans"][0]["ranges"] if res["spans"] else res.get("error_span", {}).get("ranges", [])
        max_end = None
        max_end_val = None
        for r in ranges:
            if r["path"] == "max_endurance":
                max_end = r["end"]
                max_end_val = struct.unpack_from("<H", chunk, r["start"])[0]
                break
        if max_end is None:
            continue
        post = size - max_end

        feats = {}
        for r in ranges:
            v = extract(chunk, r)
            if v is not None:
                feats[r["path"]] = v

        rec = {
            "i": i,
            "key": row["key"],
            "string_key": row.get("string_key_104"),
            "post": post,
            "max_endurance": max_end_val,
            "feats": feats,
        }
        if max_end_val == 0 and post >= 20:
            me0_classA.append(rec)
        elif max_end_val == 0 and post < 20:
            me0_classB.append(rec)
        elif max_end_val == 65535:
            me65535.append(rec)

    print(f"max_endurance == 0 AND post >= 20  (Class A, ammo-like): {len(me0_classA)}")
    print(f"max_endurance == 0 AND post <  20  (Class B):            {len(me0_classB)}")
    print(f"max_endurance == 65535             (Class B):            {len(me65535)}")
    print()

    # Top sample of each
    print("Class A (ammo-like) examples:")
    for r in me0_classA[:8]:
        print(f"  i={r['i']:>4} key={r['key']:<10} post={r['post']:>3}  {r['string_key']!r}")
    print()

    # Find a field that perfectly partitions me0_classA vs me0_classB
    fields = set()
    for rec in me0_classA + me0_classB:
        fields.update(rec["feats"].keys())

    print(f"Searching {len(fields)} fields for a perfect partitioner of me0_classA vs me0_classB:")
    candidates = []
    for f in sorted(fields):
        a_vals = Counter(r["feats"].get(f) for r in me0_classA if f in r["feats"])
        b_vals = Counter(r["feats"].get(f) for r in me0_classB if f in r["feats"])
        if not a_vals or not b_vals:
            continue
        a_set = set(a_vals.keys())
        b_set = set(b_vals.keys())
        overlap = a_set & b_set
        a_only = a_set - b_set
        b_only = b_set - a_set
        if not overlap:
            print(f"  PERFECT: {f}")
            print(f"    A-only: {sorted(list(a_only))[:8]}")
            print(f"    B-only: {sorted(list(b_only))[:8]}")
            candidates.append((f, 1.0, a_only, b_only))
        else:
            ovl_a = sum(a_vals[v] for v in overlap)
            ovl_b = sum(b_vals[v] for v in overlap)
            tot = sum(a_vals.values()) + sum(b_vals.values())
            purity = 1 - (ovl_a + ovl_b) / tot
            candidates.append((f, purity, a_only, b_only))

    print()
    print("Top 12 partitioners (1.0 = perfect):")
    for f, p, a_only, b_only in sorted(candidates, key=lambda kv: -kv[1])[:12]:
        a_show = sorted(list(a_only))[:5]
        b_show = sorted(list(b_only))[:5]
        print(f"  {p*100:6.2f}%  {f:<55}  a-only={a_show}{'...' if len(a_only)>5 else ''}  b-only={b_show}{'...' if len(b_only)>5 else ''}")


if __name__ == "__main__":
    main()
