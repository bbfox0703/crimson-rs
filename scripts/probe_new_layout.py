"""Probe whether the 1.05 ItemInfo variant tail fits across all anchors.

Layout under test (now in production in `src/item_info/item.rs`):

    ItemInfoCore (unchanged) ... enable_equip_in_clone_actor: u8
    new_icon_path: CString             // u32 len + len bytes (no NUL)

    if new_icon_path.length == 0:
        respawn_time_seconds: i64
        max_endurance: u16
        if !trailer_at(off):           // 18 ammo items only
            ammo_mid_block: [u8; 22]
    else:
        icon_flag: u8                  // observed = 0x01
        icon_unk_zeros: [u8; 9]

    ItemInfoTail: 3 × u8 + u16=0xFFFF + repair_data_list

Method: use `parse_iteminfo_tracked` to find where `enable_equip_in_clone_actor`
ends (= where `new_icon_path` starts), then re-parse the rest of the chunk in
Python according to the model and report what fraction of items it consumes
exactly. Useful as a regression check and as a reproducer for any cluster
that doesn't fit (currently 7 items leftover, 826 fail in earlier core
fields out of scope for this probe).
"""

from __future__ import annotations

import argparse
import json
import struct
from collections import Counter
from pathlib import Path

import crimson_rs


def parse_new_layout(chunk: bytes, start: int) -> tuple[bool, int, dict]:
    """Try to parse the new layout starting at `start`. Return (ok, end_off, info)."""
    info: dict = {}
    if start + 4 > len(chunk):
        return False, start, {"reason": "no room for length"}
    length = struct.unpack_from("<I", chunk, start)[0]
    info["length"] = length
    off = start + 4

    if length == 0:
        # Old-style tail: i64 + u16 [+ optional 22-byte ammo mid block] + trailer + count
        if off + 8 + 2 > len(chunk):
            return False, off, {**info, "reason": "no room for i64+u16"}
        respawn = struct.unpack_from("<q", chunk, off)[0]
        max_end = struct.unpack_from("<H", chunk, off + 8)[0]
        info["respawn"] = respawn
        info["max_endurance"] = max_end
        off += 10

        # Detect ammo: chunk has 22 extra bytes between max_endurance and trailer.
        # Heuristic: probe for the trailer pattern at off and at off+22.
        def is_trailer_at(p: int) -> bool:
            return (
                p + 5 + 4 <= len(chunk)
                and chunk[p + 3 : p + 5] == b"\xff\xff"
                and struct.unpack_from("<I", chunk, p + 5)[0] * 15 + p + 9 <= len(chunk)
            )

        if is_trailer_at(off):
            ammo = False
        elif is_trailer_at(off + 22):
            ammo = True
            off += 22  # consume ammo mid block
        else:
            return False, off, {**info, "reason": "no trailer at off or off+22"}
        info["ammo"] = ammo
    else:
        # New-style tail: N content + 1 flag + 9 zeros + trailer + count
        if off + length + 1 + 9 > len(chunk):
            return False, off, {**info, "reason": "no room for content+flag+9"}
        info["content"] = chunk[off : off + length]
        off += length
        info["flag"] = chunk[off]
        off += 1
        info["zeros"] = chunk[off : off + 9]
        off += 9

    # Trailer
    if off + 5 + 4 > len(chunk):
        return False, off, {**info, "reason": "no room for trailer"}
    if chunk[off + 3 : off + 5] != b"\xff\xff":
        return False, off, {
            **info,
            "reason": f"trailer sentinel mismatch: {chunk[off+3:off+5].hex()}",
        }
    off += 5
    cnt = struct.unpack_from("<I", chunk, off)[0]
    info["repair_count"] = cnt
    off += 4

    if off + cnt * 15 > len(chunk):
        return False, off, {**info, "reason": "no room for repair entries"}
    off += cnt * 15

    return True, off, info


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--anchors", required=True)
    ap.add_argument("--pabgb", required=True)
    ap.add_argument("--show-fails", type=int, default=8)
    args = ap.parse_args()

    anchors = json.loads(Path(args.anchors).read_text(encoding="utf-8"))
    data = Path(args.pabgb).read_bytes()

    perfect = 0
    leftover = Counter()
    fails = Counter()
    fail_examples: list[dict] = []
    length_dist = Counter()
    ammo_count = 0

    for i, row in enumerate(anchors):
        start = row["offset_105"]
        size = row["size_105"]
        chunk = data[start : start + size]
        res = crimson_rs.parse_iteminfo_tracked(chunk)
        ranges = (
            res["spans"][0]["ranges"]
            if res["spans"]
            else res.get("error_span", {}).get("ranges", [])
        )
        # Anchor to the byte right after `enable_equip_in_clone_actor` —
        # that's where `new_icon_path` (the new variant tail) starts.
        anchor = next(
            (
                r["end"]
                for r in ranges
                if r["path"] == "enable_equip_in_clone_actor"
            ),
            None,
        )
        if anchor is None:
            fails["no_anchor"] += 1
            continue

        ok, end, info = parse_new_layout(chunk, anchor)
        length_dist[info.get("length", -1)] += 1
        if info.get("ammo"):
            ammo_count += 1

        if not ok:
            fails[info.get("reason", "unknown")] += 1
            if len(fail_examples) < args.show_fails:
                fail_examples.append(
                    {
                        "i": i,
                        "key": row["key"],
                        "string_key": row.get("string_key_104"),
                        "info": info,
                    }
                )
            continue
        delta = size - end
        if delta == 0:
            perfect += 1
        else:
            leftover[delta] += 1

    total = len(anchors)
    print(f"total items     : {total:,}")
    print(f"PERFECT         : {perfect:,} ({perfect/total*100:.1f}%)")
    print(f"leftover        : {sum(leftover.values()):,}")
    print(f"FAIL            : {sum(fails.values()):,}")
    print()
    print("ammo detected   :", ammo_count)
    print()
    print("Top leftover deltas:")
    for d, c in leftover.most_common(10):
        print(f"  +{d:>4}: {c}")
    print()
    print("Top fail reasons:")
    for r, c in fails.most_common(10):
        print(f"  {c:>5} : {r}")
    print()
    print("Top length values (u32 at me-14):")
    for v, c in length_dist.most_common(15):
        print(f"  {v:>6} : {c}")
    print()
    print("Fail examples:")
    for ex in fail_examples:
        print(
            f"  i={ex['i']:>5} key={ex['key']:<10} {ex['string_key']!r}  "
            f"info={ex['info']}"
        )


if __name__ == "__main__":
    main()
