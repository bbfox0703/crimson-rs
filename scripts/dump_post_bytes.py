"""Per-cluster dump of bytes between end-of-max_endurance and end-of-chunk.

Originally written during Round 1 to inspect what we then called the
"22-byte mid block + extras". The dump turned out to actually be the
*continuation of an ASCII string*: the bytes between the parser's
`max_endurance` end and the trailer are the tail end of a new
`new_icon_path` CString plus a `01` flag plus 9 zero bytes.

This script remains useful for examining items that *don't* fit the
new layout — e.g. the `+88`/`+54`/`+93` leftover clusters and the 93
items that fail at `ammo_mid_block`. Pass `--clusters 88 54 93` (or
whatever post-size you want to investigate) and it'll hex-dump the
post-`max_endurance` region for a few examples.

Note: with the new parser, items in the icon-path branch
(new_icon_path.length > 0) do NOT have a `max_endurance` field at all,
so they are silently skipped here.
"""

from __future__ import annotations

import argparse
import json
import struct
from collections import defaultdict
from pathlib import Path

import crimson_rs


def hex_str(b: bytes) -> str:
    return " ".join(f"{x:02X}" for x in b)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--anchors", required=True)
    ap.add_argument("--pabgb", required=True)
    ap.add_argument("--samples", type=int, default=4)
    ap.add_argument(
        "--clusters",
        nargs="*",
        type=int,
        default=[31, 9],
        help="post-max_endurance lengths to inspect (defaults to ammo + Class B)",
    )
    args = ap.parse_args()

    anchors = json.loads(Path(args.anchors).read_text(encoding="utf-8"))
    data = Path(args.pabgb).read_bytes()

    by_post: dict[int, list[dict]] = defaultdict(list)

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
        me_off = None
        for r in ranges:
            if r["path"] == "max_endurance":
                me_off = r["end"]
                break
        if me_off is None:
            # Icon-path branch (new_icon_path non-empty) — no max_endurance.
            continue
        post = size - me_off
        if post not in args.clusters:
            continue
        if len(by_post[post]) >= args.samples:
            continue
        by_post[post].append(
            {
                "i": i,
                "key": row["key"],
                "string_key": row.get("string_key_104") or "",
                "size": size,
                "me_off": me_off,
                "post_bytes": bytes(chunk[me_off:size]),
            }
        )

    for post in args.clusters:
        recs = by_post.get(post) or []
        print(f"\n-- post = {post} bytes ({len(recs)} sample(s)) --")
        for rec in recs:
            pb = rec["post_bytes"]
            # Locate the trailer (`xx xx xx FF FF`) inside the post region by
            # scanning forward; if found, split into [pre-trailer | trailer
            # | u32 count | repair entries] for readability.
            trailer_off = None
            for off in range(0, post - 8):
                if pb[off + 3 : off + 5] == b"\xff\xff":
                    cnt = struct.unpack_from("<I", pb, off + 5)[0]
                    if off + 5 + 4 + cnt * 15 == post:
                        trailer_off = off
                        break
            print(
                f"  i={rec['i']:>4} key={rec['key']:<10} {rec['string_key']!r}"
            )
            if trailer_off is None:
                print(f"    raw post ({post}B): {hex_str(pb)}")
                continue
            pre = pb[:trailer_off]
            trailer = pb[trailer_off : trailer_off + 5]
            count_bytes = pb[trailer_off + 5 : trailer_off + 9]
            cnt = struct.unpack_from("<I", count_bytes, 0)[0]
            repair_blob = pb[trailer_off + 9 : post]
            if pre:
                print(f"    pre-trailer ({len(pre)}B): {hex_str(pre)}")
            print(f"    trailer (5B)          : {hex_str(trailer)}")
            print(
                f"    count (4B)            : {hex_str(count_bytes)}  = {cnt}"
            )
            if repair_blob:
                per = len(repair_blob) // max(cnt, 1)
                print(
                    f"    repair ({len(repair_blob)}B = {cnt} × {per}B): {hex_str(repair_blob)}"
                )


if __name__ == "__main__":
    main()
