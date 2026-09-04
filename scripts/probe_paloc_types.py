"""Across the whole paloc, see what 'lower-byte type' codes carry item
names. We currently only use 0x70; many dev/test items only exist under
other type bytes."""

from __future__ import annotations
import argparse
from collections import Counter, defaultdict
from pathlib import Path

import crimson_rs

from gamedata_layout import paloc_entries


PALOC_GROUPS = [f"{n:04d}" for n in range(20, 36)]


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--game-dir", required=True)
    ap.add_argument("--lang", default="eng")
    args = ap.parse_args()

    entries = paloc_entries(args.game_dir, PALOC_GROUPS, args.lang)
    print(f"{len(entries):,} paloc entries")

    # Group entries by item_key (upper 32 bits) and tally lower-byte types.
    by_item: dict[int, dict[int, str]] = defaultdict(dict)
    type_freq: Counter[int] = Counter()
    type_with_value_freq: Counter[int] = Counter()
    for e in entries:
        try:
            sid = int(e["string_key"])
        except (ValueError, TypeError):
            continue
        ik = sid >> 32
        tb = sid & 0xFF
        type_freq[tb] += 1
        v = e["string_value"]
        if v and v != "[EMPTY]":
            type_with_value_freq[tb] += 1
        by_item[ik][tb] = v

    print("\nTop 20 type bytes by total count:")
    for tb, n in type_freq.most_common(20):
        with_val = type_with_value_freq[tb]
        print(f"  0x{tb:02X}  {n:>7}  ({with_val:>7} non-empty)")

    # For items that LACK 0x70, what type bytes do they have?
    missing_70 = 0
    fallback_dist: Counter[int] = Counter()
    for ik, types in by_item.items():
        if 0x70 in types:
            continue
        missing_70 += 1
        for tb, v in types.items():
            if v and v != "[EMPTY]":
                fallback_dist[tb] += 1

    print(f"\nItems with NO 0x70 entry: {missing_70:,}")
    print("Type-byte distribution for those items (entries with non-empty value):")
    for tb, n in fallback_dist.most_common(20):
        print(f"  0x{tb:02X}  {n}")

    # Sample: 5 items that lack 0x70 and what they have
    print("\nSamples of items with no 0x70:")
    listed = 0
    for ik, types in by_item.items():
        if 0x70 in types:
            continue
        if listed >= 12:
            break
        listed += 1
        kept = [(tb, v) for tb, v in types.items() if v and v != "[EMPTY]"]
        if not kept:
            continue
        print(f"  key={ik:<10}")
        for tb, v in sorted(kept):
            print(f"    0x{tb:02X}: {v!r}")


if __name__ == "__main__":
    main()
