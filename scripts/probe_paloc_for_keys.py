"""For a given set of 'missing' item keys, scan the raw paloc files for
ANY entry that references those keys (regardless of the lower-byte type
filter). Lets us see whether dev/test items actually have a localized
string somewhere — and if so, under what type byte."""

from __future__ import annotations
import argparse
import json
from collections import Counter
from pathlib import Path

import crimson_rs

from gamedata_layout import paloc_entries


PALOC_GROUPS = [f"{n:04d}" for n in range(20, 36)]


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--game-dir", required=True)
    ap.add_argument("--unknown", required=True, help="unknown_items.json")
    ap.add_argument("--lang", default="eng")
    ap.add_argument("--limit", type=int, default=20, help="show first N hits per item")
    args = ap.parse_args()

    unknown = json.loads(Path(args.unknown).read_text(encoding="utf-8"))
    target_keys = {r["key"] for r in unknown}

    entries = paloc_entries(args.game_dir, PALOC_GROUPS, args.lang)
    print(f"parsed {len(entries):,} paloc entries")

    # For each entry, parse the string_key as int and check if upper 32 bits
    # match a target item_key.
    hits_by_key: dict[int, list[dict]] = {k: [] for k in target_keys}
    type_counts: Counter[int] = Counter()
    for e in entries:
        try:
            sid = int(e["string_key"])
        except (ValueError, TypeError):
            continue
        ik = sid >> 32
        if ik in target_keys:
            type_counts[sid & 0xFF] += 1
            hits_by_key[ik].append({
                "type_byte": sid & 0xFF,
                "string_key": e["string_key"],
                "value": e["string_value"],
            })

    print(f"\nType byte distribution among hits for missing keys:")
    for tb, n in type_counts.most_common():
        print(f"  0x{tb:02X}: {n}")

    print(f"\nFirst {args.limit} missing items + their paloc entries (any type):")
    listed = 0
    for r in unknown:
        ik = r["key"]
        hits = hits_by_key.get(ik, [])
        print(f"\n  i={r['i']:>4} key={ik:<10} {r['string_key']!r}")
        if not hits:
            print("    (no paloc entries at all)")
        else:
            for h in hits[:8]:
                print(f"    type=0x{h['type_byte']:02X} sid={h['string_key']:<22} value={h['value']!r}")
            if len(hits) > 8:
                print(f"    ... +{len(hits)-8} more")
        listed += 1
        if listed >= args.limit:
            break

    # also count items that have ZERO paloc entries even at any type byte
    zero_hits = sum(1 for k in target_keys if not hits_by_key[k])
    print(f"\nMissing items with NO paloc entry at any type byte: "
          f"{zero_hits}/{len(target_keys)}")


if __name__ == "__main__":
    main()
