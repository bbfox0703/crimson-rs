"""For a single item key, trace the byte-by-byte divergence between
1.09 (after removing money_icon_path) and 1.10. Used to investigate
the 420 items where verify_109_to_110.py reports a size mismatch.

Strategy:
  1. Get the 1.09 item's tracked field spans.
  2. Synthesise the post-money_icon_path-patch 1.09 bytes.
  3. Tandem-walk vs 1.10 actual bytes; at each mismatch, report
     the field (using span lookups) AND attempt to find the next
     point where the suffixes realign.

Usage:
    python scripts/trace_size_mismatch.py 1
    python scripts/trace_size_mismatch.py 1002138
"""

from __future__ import annotations

import struct
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO))
sys.path.insert(0, str(REPO / "scripts"))
import crimson_rs  # noqa: E402
from diff_109_110 import find_anchors  # noqa: E402

OLD_PABGB = REPO / "out" / "1.09" / "iteminfo.pabgb"
NEW_PABGB = REPO / "out" / "iteminfo.1.10.pabgb"


def main(keys: list[int]):
    old_bytes = OLD_PABGB.read_bytes()
    new_bytes = NEW_PABGB.read_bytes()
    tracked = crimson_rs.parse_iteminfo_tracked(old_bytes)
    items, spans = tracked["items"], tracked["spans"]
    by_key = {it["key"]: (i, it) for i, it in enumerate(items)}

    new_keys = [int(l.strip()) for l in (REPO / "data" / "keys.txt").read_text().split() if l.strip()]
    new_anchors = find_anchors(new_bytes, new_keys)
    next_off = [None] * len(new_anchors)
    last = len(new_bytes)
    for i in range(len(new_anchors) - 1, -1, -1):
        if new_anchors[i] is not None:
            next_off[i] = last
            last = new_anchors[i]
    new_key_to_idx = {k: i for i, k in enumerate(new_keys)}

    for target in keys:
        if target not in by_key or target not in new_key_to_idx:
            print(f"key={target}: not found")
            continue
        old_idx, item = by_key[target]
        sp = spans[old_idx]
        old_lo, old_hi = sp["start"], sp["end"]

        new_idx = new_key_to_idx[target]
        new_off = new_anchors[new_idx]
        new_end = next_off[new_idx]

        # rel ranges
        rel_ranges = [
            {"start": r["start"] - old_lo, "end": r["end"] - old_lo, "path": r["path"], "ty": r["ty"]}
            for r in sp["ranges"]
        ]

        # Synth 1.09 -> 1.10 = drop bytes [money_start, money_end)
        m = next((r for r in rel_ranges if r["path"] == "money_icon_path"), None)
        if m is None:
            print(f"key={target}: no money_icon_path")
            continue
        old_chunk = old_bytes[old_lo:old_hi]
        synth = old_chunk[:m["start"]] + old_chunk[m["end"]:]
        actual = new_bytes[new_off:new_end]

        print(f"\n=== key={target} string_key={item.get('string_key', '?')} ===")
        print(f"  1.09 item bytes: {old_hi - old_lo}")
        print(f"  1.10 item bytes: {new_end - new_off}")
        print(f"  synth_after_patch: {len(synth)}")
        print(f"  delta (actual - synth): {(new_end - new_off) - len(synth):+d}")

        # Rebuild rel_ranges to point at synth coordinates: ranges with
        # start >= money_end shift down by 4; the money_icon_path range
        # is omitted.
        ms, me = m["start"], m["end"]
        shift = me - ms
        synth_ranges = []
        for r in rel_ranges:
            if r["path"] == "money_icon_path":
                continue
            if r["start"] >= me:
                synth_ranges.append({**r, "start": r["start"] - shift, "end": r["end"] - shift})
            else:
                synth_ranges.append(r)

        # Walk byte by byte. Stop at first mismatch, then try to find
        # the next 32-byte window in actual that matches a 32-byte
        # window in synth (allows skipping over CArray content changes).
        i = 0
        n = min(len(synth), len(actual))
        while i < n and synth[i] == actual[i]:
            i += 1
        if i == n and len(synth) == len(actual):
            print(f"  byte-perfect match (no diff)")
            continue

        # Report first mismatch in field context
        first_field = "?"
        for r in synth_ranges:
            if r["start"] <= i < r["end"]:
                first_field = f"{r['path']}@{i - r['start']}/{r['end'] - r['start']}"
                break
        print(f"  first diff at synth_off={i}  field={first_field}")
        print(f"    synth [{i:>4d}]: {synth[i:i + 16].hex(' ')}")
        print(f"    actual[{i:>4d}]: {actual[i:i + 16].hex(' ')}")

        # Try to realign — scan actual for a 32-byte window that occurs
        # in synth, picking the EARLIEST one in synth (the diverged
        # field) and EARLIEST in actual (start of new content).
        WIN = 32
        realigned = False
        for actual_off in range(i, min(len(actual) - WIN, i + 200)):
            target_win = actual[actual_off:actual_off + WIN]
            synth_off = synth.find(target_win, i)
            if synth_off >= 0:
                synth_gap = synth_off - i
                actual_gap = actual_off - i
                # find the resync field
                fld = "?"
                for r in synth_ranges:
                    if r["start"] <= synth_off < r["end"]:
                        fld = f"{r['path']}@{synth_off - r['start']}"
                        break
                print(f"  realign found: synth_off={synth_off} (+{synth_gap})  "
                      f"actual_off={actual_off} (+{actual_gap})  resync_field={fld}")
                # Show the removed/inserted bytes
                rm = synth[i:synth_off]
                ins = actual[i:actual_off]
                print(f"    removed (synth): len={len(rm)}  {rm[:32].hex(' ')}{' ...' if len(rm) > 32 else ''}")
                print(f"    inserted (actual): len={len(ins)}  {ins[:32].hex(' ')}{' ...' if len(ins) > 32 else ''}")
                realigned = True
                break
        if not realigned:
            print(f"  could not find realignment within 200 bytes")


if __name__ == "__main__":
    if len(sys.argv) < 2:
        # Default: trace one from each delta bucket
        defaults = [
            1,         # Money_Copper, +8B
            300023,    # Light_Arrowhead_Leader_OneHandBow, +6B
            1002180,   # Kuku_FlameThrower, +4B
            1000083,   # Verheim_TwoHandSword, +2B
            1001136,   # Bastion_OneHandMace, +10B
            1001329,   # gimmick_tool_kinetic_barbell_0001, +20B
            1004411,   # Item_gimmick_in_dpf_carpet_0001, -45B
            11,        # Money_Camp_Money, +58B
            380506,    # Drunk_BlackBears_Flag, -26B
        ]
        main(defaults)
    else:
        main([int(a) for a in sys.argv[1:]])
