"""For items with leftover bytes after my struct, dump those bytes so
we can guess the new fields they encode."""
from __future__ import annotations
import argparse
import json
from collections import defaultdict
from pathlib import Path

import crimson_rs


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--anchors", required=True)
    ap.add_argument("--pabgb", required=True)
    ap.add_argument("--leftover", type=int, default=9)
    ap.add_argument("--samples", type=int, default=5)
    args = ap.parse_args()

    anchors = json.loads(Path(args.anchors).read_text(encoding="utf-8"))
    data = Path(args.pabgb).read_bytes()

    matches: list[tuple[dict, int, bytes]] = []
    for i, row in enumerate(anchors):
        start = row["offset_105"]
        size = row["size_105"]
        chunk = data[start:start + size]
        res = crimson_rs.parse_iteminfo_tracked(chunk)
        if not res["spans"]:
            continue
        consumed = res["spans"][0]["end"]
        leftover = size - consumed
        if leftover != args.leftover:
            continue
        matches.append((row, consumed, chunk[consumed:size]))
        if len(matches) >= args.samples:
            break

    print(f"Found {len(matches)} items with exactly +{args.leftover} leftover bytes:")
    for row, consumed, leftover_bytes in matches:
        sk = row.get("string_key_104") or "?"
        print(f"\n  i={row['i']:>4} key={row['key']:<10} size={row['size_105']:>4} consumed={consumed:>4}  {sk!r}")
        hexs = " ".join(f"{x:02X}" for x in leftover_bytes)
        ascii = "".join(chr(x) if 32 <= x < 127 else "." for x in leftover_bytes)
        print(f"  leftover ({len(leftover_bytes)}B): {hexs}  |{ascii}|")


if __name__ == "__main__":
    main()
