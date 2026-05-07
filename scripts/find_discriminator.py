"""Cross-tabulate post-size against (max_endurance, other fields) to find
the boolean predicate that perfectly identifies items needing the
22-byte mid block."""
from __future__ import annotations
import argparse
import json
import struct
from collections import Counter, defaultdict
from pathlib import Path

import crimson_rs


SIMPLE_TYPES = {
    "u8": ("<B", 1), "u16": ("<H", 2), "u32": ("<I", 4),
    "u64": ("<Q", 8), "i64": ("<q", 8), "f32": ("<f", 4),
}


def extract_value(chunk: bytes, r: dict):
    ty = r["ty"]
    f = SIMPLE_TYPES.get(ty)
    if f:
        return struct.unpack_from(f[0], chunk, r["start"])[0]
    if ty in ("CString.len", "CArray.count"):
        return struct.unpack_from("<I", chunk, r["start"])[0]
    return None


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--anchors", required=True)
    ap.add_argument("--pabgb", required=True)
    args = ap.parse_args()

    anchors = json.loads(Path(args.anchors).read_text(encoding="utf-8"))
    data = Path(args.pabgb).read_bytes()

    items: list[dict] = []
    for i, row in enumerate(anchors):
        start = row["offset_105"]
        size = row["size_105"]
        chunk = data[start:start + size]
        res = crimson_rs.parse_iteminfo_tracked(chunk)
        ranges = res["spans"][0]["ranges"] if res["spans"] else res.get("error_span", {}).get("ranges", [])
        max_end_off = None
        for r in ranges:
            if r["path"] == "max_endurance":
                max_end_off = r["end"]
                break
        if max_end_off is None:
            continue
        post = size - max_end_off
        feat = {}
        for r in ranges:
            v = extract_value(chunk, r)
            if v is not None:
                feat[r["path"]] = v
        items.append({"i": i, "key": row["key"], "size": size, "post": post, "feat": feat})

    # Class A = post >= 20, Class B = post < 20
    print(f"Total items reaching max_endurance: {len(items)}")
    print()

    # 1. max_endurance distribution per class
    print("max_endurance distribution by class:")
    a_max = Counter()
    b_max = Counter()
    for it in items:
        m = it["feat"].get("max_endurance")
        if m is None:
            continue
        if it["post"] >= 20:
            a_max[m] += 1
        else:
            b_max[m] += 1
    print(f"  Class A (post>=20): {sum(a_max.values())} items")
    print(f"    65535: {a_max.get(65535, 0)}  other: {sum(v for k, v in a_max.items() if k != 65535)}")
    print(f"  Class B (post<20):  {sum(b_max.values())} items")
    print(f"    65535: {b_max.get(65535, 0)}  other: {sum(v for k, v in b_max.items() if k != 65535)}")

    # 2. items in Class B with max_endurance != 65535 — what makes them Class B despite "having durability"?
    print()
    print("Class B items with max_endurance != 65535 (need a different rule):")
    weird_b = [it for it in items if it["post"] < 20 and it["feat"].get("max_endurance") not in (65535, None)]
    print(f"  count: {len(weird_b)}")
    me_counter = Counter(it["feat"].get("max_endurance") for it in weird_b)
    print(f"  max_endurance histogram: {dict(me_counter.most_common(10))}")

    # 3. items in Class A with max_endurance == 65535 — also a counterexample
    print()
    print("Class A items with max_endurance == 65535:")
    weird_a = [it for it in items if it["post"] >= 20 and it["feat"].get("max_endurance") == 65535]
    print(f"  count: {len(weird_a)}")

    # 4. find a perfect 2-field partitioner among (max_endurance, other-field)
    fields = set()
    for it in items:
        fields.update(it["feat"].keys())
    fields.discard("max_endurance")

    print()
    print("Search for `max_endurance==65535 AND/OR other-field condition` perfect partitioners:")
    best: list[tuple[str, str, float, int, int]] = []
    for f in fields:
        # gather (me_is_unset, f_value) -> class A/B
        vals = defaultdict(lambda: [0, 0])
        for it in items:
            me = it["feat"].get("max_endurance")
            v = it["feat"].get(f)
            if me is None or v is None:
                continue
            cls = 0 if it["post"] >= 20 else 1  # 0=A, 1=B
            key = (me == 65535, v)
            vals[key][cls] += 1
        # try thresholding f
        # pick simple combined predicate: (me==65535) ⟹ B
        # also test: (f == X) ⟹ class
        # for each value v of f, compute purity if we use "me==65535 OR f==v ⟹ B"
        a_total = sum(1 for it in items if it["post"] >= 20)
        b_total = sum(1 for it in items if it["post"] < 20)
        # simple "f == v identifies B" partition
        for v in {x[1] for x in vals.keys()}:
            a_match = sum(vals[(t, v)][0] for t in (True, False))
            b_match = sum(vals[(t, v)][1] for t in (True, False))
            # how many items have f==v?
            tot = a_match + b_match
            if tot < 20:
                continue
            # treat "f==v" as predicting Class B
            tp = b_match
            fp = a_match
            tn_b = b_total - b_match  # B not predicted (because f != v)
            tn_a = a_total - a_match  # A correctly NOT predicted (since f != v means A)
            # combined: predict B iff (max_endurance==65535) OR (f==v)
            # first count items where me==65535
            b_via_me = sum(b for (t, _), (_, b) in vals.items() if t)
            a_via_me = sum(a for (t, _), (a, _) in vals.items() if t)
            b_via_v = sum(vals[(False, v)][1])
            a_via_v = sum(vals[(False, v)][0])
            # union (using OR): items with me==65535 OR f==v
            pred_b = b_via_me + b_via_v
            pred_a_wrong = a_via_me + a_via_v  # A predicted as B (wrong)
            pred_a_right = a_total - pred_a_wrong  # A predicted as A (correct)
            pred_b_miss = b_total - pred_b  # B predicted as A (wrong)
            correct = pred_b + pred_a_right
            purity = correct / (a_total + b_total)
            best.append((f, repr(v), purity, pred_a_wrong, pred_b_miss))

    print()
    print("Best 2-field partitioners (predicate: max_endurance==65535 OR field==value ⟹ B):")
    for f, v, p, fp_a, fn_b in sorted(best, key=lambda kv: -kv[2])[:15]:
        print(f"  {p*100:6.2f}%  field={f:<45} value={v:<25}  A-mispredict={fp_a:<4} B-miss={fn_b}")


if __name__ == "__main__":
    main()
