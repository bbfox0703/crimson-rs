"""Look at items with post-max_endurance==31 directly to see whether the
conditional mid block correctly puts them into the perfect bucket."""
from __future__ import annotations
import argparse
import json
import struct
from pathlib import Path

import crimson_rs


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--anchors", required=True)
    ap.add_argument("--pabgb", required=True)
    args = ap.parse_args()

    anchors = json.loads(Path(args.anchors).read_text(encoding="utf-8"))
    data = Path(args.pabgb).read_bytes()

    # First, compute classify-style post-size for each item without the mid block
    # — i.e. find max_endurance offset via the tracked parser.
    samples = []
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
        if post == 31:
            consumed = res["spans"][0]["end"] if res["spans"] else None
            leftover = (size - consumed) if consumed is not None else None
            samples.append({
                "i": i,
                "key": row["key"],
                "string_key": row.get("string_key_104"),
                "size": size,
                "max_endurance": max_end_val,
                "consumed": consumed,
                "leftover": leftover,
                "is_class_a_predicted": max_end_val not in (0, 65535),
            })
        if len(samples) >= 25:
            break

    print(f"items with post=31 (first 25):")
    print(f"{'i':>5} {'key':<10} {'me':>6}  {'classA?':<8} {'cons':>5} {'lo':>4}  string_key")
    for s in samples:
        print(
            f"{s['i']:>5} {s['key']:<10} {s['max_endurance']:>6}  "
            f"{str(s['is_class_a_predicted']):<8} "
            f"{s['consumed']!s:>5} {s['leftover']!s:>4}  {s['string_key']!r}"
        )


if __name__ == "__main__":
    main()
