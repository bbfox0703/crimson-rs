"""Per-item analyser — uses anchors to slice the 1.05 file into exact per-
item byte chunks, then tries to parse each independently and reports:

    - parser SUCCESS, no leftover bytes: struct fits this item exactly.
    - parser SUCCESS, leftover bytes: struct misses some trailing fields.
    - parser FAIL at field X: struct is misaligned, breaks at X.

The leftover-bytes case is the most informative for fixing the parser:
the leftover counts cluster into a small number of "delta classes" that
correspond to optional fields the struct should have.
"""

from __future__ import annotations

import argparse
import json
import struct
import sys
from collections import Counter, defaultdict
from pathlib import Path

import crimson_rs


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--anchors", required=True)
    ap.add_argument("--pabgb", required=True)
    ap.add_argument("--top", type=int, default=20)
    args = ap.parse_args()

    anchors = json.loads(Path(args.anchors).read_text(encoding="utf-8"))
    data = Path(args.pabgb).read_bytes()

    full_ok = 0           # parser succeeded AND consumed all bytes
    leftover_ok = 0       # parser succeeded but left bytes in the chunk
    parse_fail = 0
    leftover_hist: Counter[int] = Counter()
    fail_path_hist: Counter[str] = Counter()
    leftover_examples: dict[int, list[dict]] = defaultdict(list)
    fail_examples: dict[str, list[dict]] = defaultdict(list)

    for i, row in enumerate(anchors):
        start = row["offset_105"]
        size = row["size_105"]
        chunk = data[start : start + size]
        try:
            res = crimson_rs.parse_iteminfo_tracked(chunk)
            spans = res["spans"]
            if spans:
                end = spans[-1]["end"]
                leftover = size - end
                if leftover == 0:
                    full_ok += 1
                else:
                    leftover_ok += 1
                    leftover_hist[leftover] += 1
                    if len(leftover_examples[leftover]) < 3:
                        leftover_examples[leftover].append(
                            {
                                "i": i,
                                "key": row["key"],
                                "string_key": row.get("string_key_104"),
                                "size": size,
                                "consumed": end,
                            }
                        )
            else:
                # No spans means it failed mid-item — fall through to fail path
                err_span = res.get("error_span", {})
                path = err_span.get("path", "<unknown>")
                parse_fail += 1
                fail_path_hist[path] += 1
                if len(fail_examples[path]) < 3:
                    fail_examples[path].append(
                        {
                            "i": i,
                            "key": row["key"],
                            "string_key": row.get("string_key_104"),
                            "size": size,
                        }
                    )
        except Exception as e:
            parse_fail += 1
            fail_path_hist[str(e)[:60]] += 1

    total = len(anchors)
    print(f"total items     : {total:,}")
    print(f"SUCCESS perfect : {full_ok:,} ({full_ok/total*100:.1f}%)")
    print(f"SUCCESS leftover: {leftover_ok:,} ({leftover_ok/total*100:.1f}%)")
    print(f"FAIL            : {parse_fail:,} ({parse_fail/total*100:.1f}%)")
    print()

    print(f"Top {args.top} leftover-byte classes (parser stopped short by N bytes):")
    for delta, cnt in leftover_hist.most_common(args.top):
        print(f"  +{delta:>3} bytes : {cnt:>5} items")
        for ex in leftover_examples[delta][:2]:
            print(
                f"    e.g. i={ex['i']:>4} key={ex['key']:<10} size={ex['size']:>4} "
                f"consumed={ex['consumed']:>4}  {ex['string_key']!r}"
            )
    print()

    print(f"Top {args.top} failure paths (parser broke mid-item):")
    for path, cnt in fail_path_hist.most_common(args.top):
        print(f"  {cnt:>5}x  {path}")
        for ex in fail_examples[path][:2]:
            print(
                f"    e.g. i={ex['i']:>4} key={ex['key']:<10} size={ex['size']:>4}  {ex['string_key']!r}"
            )


if __name__ == "__main__":
    main()
