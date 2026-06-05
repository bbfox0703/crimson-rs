"""Verify the 1.09 → 1.10 schema hypothesis.

Hypothesis: 1.10 removed `money_icon_path: u32` (4 bytes) from each
iteminfo entry. No other schema change.

For every item key common to 1.09 and 1.10:
  1. Locate the item in both binaries.
  2. From the 1.09 tracked-span data, find the byte range for
     `money_icon_path`.
  3. Reconstruct a "synthetic 1.10" by deleting those 4 bytes.
  4. Compare with the actual 1.10 chunk.

A perfect schema-only change would produce: schema_match = 6314,
content_drift = 0. Anything else indicates either an additional
schema change we missed OR a content drift mid-item (the latter is
fine, just informational).

Usage:
    python scripts/verify_109_to_110.py
"""

from __future__ import annotations

import struct
import sys
from collections import Counter
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO))
import crimson_rs  # noqa: E402

OLD_PABGB = REPO / "out" / "1.09" / "iteminfo.pabgb"
NEW_PABGB = REPO / "out" / "iteminfo.1.10.pabgb"

# Reuse anchoring helpers from the diff script
sys.path.insert(0, str(REPO / "scripts"))
from diff_109_110 import find_anchors  # noqa: E402


def main():
    old_bytes = OLD_PABGB.read_bytes()
    new_bytes = NEW_PABGB.read_bytes()
    print(f"1.09: {len(old_bytes):,}B   1.10: {len(new_bytes):,}B")

    tracked = crimson_rs.parse_iteminfo_tracked(old_bytes)
    old_items, old_spans = tracked["items"], tracked["spans"]
    old_by_key = {it["key"]: (i, it) for i, it in enumerate(old_items)}

    new_keys = [int(l.strip()) for l in (REPO / "data" / "keys.txt").read_text().split() if l.strip()]
    new_anchors = find_anchors(new_bytes, new_keys)

    # Compute item ends in 1.10 (next anchor)
    next_off = [None] * len(new_anchors)
    last = len(new_bytes)
    for i in range(len(new_anchors) - 1, -1, -1):
        if new_anchors[i] is not None:
            next_off[i] = last
            last = new_anchors[i]

    schema_match = 0
    content_drift = 0
    size_mismatch_after_patch = 0  # synthetic length != actual 1.10 length
    field_missing = 0  # no money_icon_path range found
    samples_drift = []
    size_mismatch_records: list[tuple] = []  # (key, old_size, new_size, synth_size, item)

    for new_idx, k in enumerate(new_keys):
        if k not in old_by_key:
            continue
        new_off = new_anchors[new_idx]
        new_end = next_off[new_idx]
        if new_off is None or new_end is None:
            continue
        old_idx, _ = old_by_key[k]
        sp = old_spans[old_idx]
        old_lo, old_hi = sp["start"], sp["end"]

        # Find money_icon_path range (absolute offsets)
        money_range = None
        for r in sp["ranges"]:
            if r["path"] == "money_icon_path":
                money_range = r
                break
        if money_range is None:
            field_missing += 1
            continue

        # Reconstruct synthetic 1.10 = 1.09[old_lo:money_start] + 1.09[money_end:old_hi]
        synth = (
            old_bytes[old_lo : money_range["start"]]
            + old_bytes[money_range["end"] : old_hi]
        )
        actual = new_bytes[new_off:new_end]
        if len(synth) != len(actual):
            size_mismatch_after_patch += 1
            size_mismatch_records.append((k, old_hi - old_lo, len(actual), len(synth), old_items[old_idx]))
            continue
        if synth == actual:
            schema_match += 1
        else:
            content_drift += 1
            if len(samples_drift) < 5:
                # find first differing byte
                first = next(i for i in range(len(synth)) if synth[i] != actual[i])
                samples_drift.append((k, first, synth[first:first + 8].hex(), actual[first:first + 8].hex()))

    print()
    print(f"schema_match (byte-perfect after removing money_icon_path): {schema_match}")
    print(f"content_drift (size matches but body differs): {content_drift}")
    print(f"size_mismatch_after_patch (additional schema change?): {size_mismatch_after_patch}")
    print(f"field_missing (no money_icon_path range): {field_missing}")
    print()
    if samples_drift:
        print(f"Content-drift samples (first 5):")
        for k, off, s, a in samples_drift:
            print(f"  key={k}  first_diff_off={off}  synth=0x{s}  actual=0x{a}")
    if size_mismatch_after_patch > 0:
        print()
        print(f"WARNING: {size_mismatch_after_patch} items don't match length after the patch.")
        print(f"Additional schema drift is present.")
        # Bucket by (actual - synth)
        delta_counter: Counter[int] = Counter()
        for k, old_sz, new_sz, synth_sz, _ in size_mismatch_records:
            delta_counter[new_sz - synth_sz] += 1
        print(f"  Length deltas (1.10_actual - synth_109_minus_money_icon_path):")
        for delta, cnt in delta_counter.most_common(8):
            print(f"    {delta:+5d}B  ({cnt}x)")

        # Show samples per delta bucket — useful for spotting per-bucket
        # schema-change patterns by name.
        print()
        for delta, cnt in delta_counter.most_common(10):
            samples = [k for k, _, ns, ss, _ in size_mismatch_records if ns - ss == delta][:5]
            # Map keys back to string_keys for a name hint
            names = []
            for k in samples:
                old_idx, it = old_by_key[k]
                names.append(f"{k}:{it.get('string_key', '?')[:32]}")
            print(f"    delta={delta:+d}B ({cnt}x) e.g.: {names}")


if __name__ == "__main__":
    main()
