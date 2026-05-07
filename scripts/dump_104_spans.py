"""Dump 1.04 parser-tracked field offsets for selected items.

Run this with the historical 1.04 parser wheel (built from commit 56a57da)
which lives at `.crimson_rs_104/` after a `pip install --target` from
`crimson-rs-104/target/wheels/`. The 1.04 parser is correct on its own
data, so its `parse_iteminfo_tracked` output is ground truth for "this
field is at this byte offset" in the 1.04 chunk.

Output: JSON of {string_key: {fields: [{path, start, end, ty}, ...], size}}
written to out/baselines/1.04/spans.json (gitignored along with the rest
of out/baselines/).

Usage:
    python scripts/dump_104_spans.py
    python scripts/dump_104_spans.py --items Pyeonjeon_Arrow,High_Meat
    python scripts/dump_104_spans.py --all   # all 6,339 items, large file
"""

from __future__ import annotations

import argparse
import json
import struct
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

# Pin import to the historical wheel.
sys.path.insert(0, str(REPO / ".crimson_rs_104"))
import crimson_rs as cr104

DEFAULT_PABGB = REPO / "out" / "baselines" / "1.04" / "iteminfo.pabgb"
DEFAULT_BASELINE = REPO / "out" / "baselines" / "1.04" / "items.jsonl"
DEFAULT_OUT = REPO / "out" / "baselines" / "1.04" / "spans.json"

DEFAULT_ITEMS = [
    "Pyeonjeon_Arrow",
    "Arrow",
    "High_Meat",
    "Item_gimmick_resourcestorage_0001",
    "Item_gimmick_collectionstorage_0001",
    "Item_gimmick_foodstorage_0001",
    "Boss_Reward_SuperWeapon",
    "Gas_Mask_Helm_I",
    "Crude_Devil_Mask",
    "Food_Salmon",
    "Food_Trout",
]


def _is_ident(b: int) -> bool:
    return (
        b == ord("_")
        or b == ord(" ")
        or 48 <= b <= 57
        or 65 <= b <= 90
        or 97 <= b <= 122
    )


def _looks(data: bytes, off: int, key: int) -> bool:
    if off + 12 > len(data):
        return False
    k, slen = struct.unpack_from("<II", data, off)
    if k != key or not (2 <= slen <= 64) or off + 8 + slen + 1 > len(data):
        return False
    sk = data[off + 8 : off + 8 + slen]
    return all(_is_ident(b) for b in sk) and data[off + 8 + slen] == 0


def find_anchors(data: bytes, keys: list[int]) -> list[int]:
    if not _looks(data, 0, keys[0]):
        sys.exit("first key not at offset 0")
    anchors = [0]
    for i in range(1, len(keys)):
        cursor = anchors[-1] + 60
        target = struct.pack("<I", keys[i])
        found = -1
        while cursor + 12 <= len(data):
            idx = data.find(target, cursor)
            if idx < 0:
                break
            if _looks(data, idx, keys[i]):
                found = idx
                break
            cursor = idx + 1
        if found < 0:
            sys.exit(f"anchor failed at i={i} key={keys[i]}")
        anchors.append(found)
    return anchors


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--pabgb", default=str(DEFAULT_PABGB))
    ap.add_argument("--baseline", default=str(DEFAULT_BASELINE))
    ap.add_argument("--out", default=str(DEFAULT_OUT))
    ap.add_argument(
        "--items",
        help="comma-separated list of string_keys to dump (overrides default).",
    )
    ap.add_argument("--all", action="store_true", help="dump every 1.04 item")
    args = ap.parse_args()

    data = Path(args.pabgb).read_bytes()
    keys: list[int] = []
    sk_to_key: dict[str, int] = {}
    with Path(args.baseline).open(encoding="utf-8") as fh:
        for line in fh:
            d = json.loads(line)
            if "key" in d:
                keys.append(d["key"])
                sk_to_key[d["string_key"]] = d["key"]
    print(f"1.04 binary: {len(data):,} bytes  {len(keys):,} items")
    anchors = find_anchors(data, keys)

    if args.all:
        wanted = list(sk_to_key.keys())
    elif args.items:
        wanted = [s.strip() for s in args.items.split(",") if s.strip()]
    else:
        wanted = list(DEFAULT_ITEMS)

    out: dict[str, dict] = {}
    n_ok = 0
    n_fail = 0
    for sk in wanted:
        if sk not in sk_to_key:
            print(f"  !! {sk} not in 1.04")
            continue
        key = sk_to_key[sk]
        i = keys.index(key)
        off = anchors[i]
        end = anchors[i + 1] if i + 1 < len(anchors) else len(data)
        chunk = data[off:end]
        res = cr104.parse_iteminfo_tracked(chunk)
        spans = res.get("spans") or []
        if not spans:
            err = res.get("error_span", {})
            out[sk] = {
                "key": key,
                "size": len(chunk),
                "ok": False,
                "fail_path": err.get("path"),
                "fail_end": err.get("end"),
                "fields": err.get("ranges", []),
            }
            n_fail += 1
        else:
            out[sk] = {
                "key": key,
                "size": len(chunk),
                "ok": True,
                "fields": spans[0]["ranges"],
            }
            n_ok += 1

    Path(args.out).parent.mkdir(parents=True, exist_ok=True)
    with Path(args.out).open("w", encoding="utf-8") as fh:
        json.dump(out, fh, ensure_ascii=False, indent=1)
    print(f"wrote {args.out}  ({Path(args.out).stat().st_size:,}B)")
    print(f"items: ok={n_ok}  fail={n_fail}")


if __name__ == "__main__":
    main()
