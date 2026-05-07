"""Full sequence-diff of 1.04 vs 1.05 ItemInfo chunks.

Uses difflib.SequenceMatcher to find ALL insertion / deletion / replace
spans, not just the first divergence. Annotates each span with the 1.05
parser-tracked field path that contains the 1.05 offset, so the new
fields can be located in the schema directly.

Usage:
    python scripts/diff_104_105_full.py --item Item_gimmick_resourcestorage_0001
    python scripts/diff_104_105_full.py --status fail:item_bundle_data_list --top 3
    python scripts/diff_104_105_full.py --status fail:ammo_mid_block --top 3
"""

from __future__ import annotations

import argparse
import difflib
import json
import struct
import sys
from pathlib import Path

import crimson_rs

REPO = Path(__file__).resolve().parent.parent

DEFAULT_104_PABGB = REPO / "out" / "baselines" / "1.04" / "iteminfo.pabgb"
DEFAULT_104_BASELINE = REPO / "out" / "baselines" / "1.04" / "items.jsonl"
DEFAULT_105_PABGB = REPO / "out" / "iteminfo.pabgb"
DEFAULT_105_KEYS = REPO / "data" / "keys.txt"
DEFAULT_105_ITEMS_JSONL = REPO / "out" / "items.jsonl"


def _is_ident(b: int) -> bool:
    return (
        b == ord("_")
        or b == ord(" ")
        or 48 <= b <= 57
        or 65 <= b <= 90
        or 97 <= b <= 122
    )


def _looks_like_start(data: bytes, off: int, key: int) -> bool:
    if off + 12 > len(data):
        return False
    k, slen = struct.unpack_from("<II", data, off)
    if k != key or not (2 <= slen <= 64) or off + 8 + slen + 1 > len(data):
        return False
    sk = data[off + 8 : off + 8 + slen]
    return all(_is_ident(b) for b in sk) and data[off + 8 + slen] == 0


def find_anchors(data: bytes, keys: list[int]) -> list[int]:
    if not keys:
        return []
    if not _looks_like_start(data, 0, keys[0]):
        sys.exit(f"first key {keys[0]} not at offset 0")
    anchors = [0]
    for i in range(1, len(keys)):
        cursor = anchors[-1] + 60
        target = struct.pack("<I", keys[i])
        found = -1
        while cursor + 12 <= len(data):
            idx = data.find(target, cursor)
            if idx < 0:
                break
            if _looks_like_start(data, idx, keys[i]):
                found = idx
                break
            cursor = idx + 1
        if found < 0:
            sys.exit(f"anchor scan failed at i={i} key={keys[i]}")
        anchors.append(found)
    return anchors


def load_baseline_keys(path: Path) -> list[int]:
    keys = []
    with path.open(encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            d = json.loads(line)
            if "key" in d:
                keys.append(d["key"])
    return keys


def load_keys_txt(path: Path) -> list[int]:
    keys = []
    for raw in path.read_text(encoding="utf-8").splitlines():
        s = raw.strip()
        if not s:
            continue
        try:
            k = int(s)
        except ValueError:
            break
        if k == 0xFFFFFFFF or k == 0:
            break
        keys.append(k)
    return keys


def field_at(ranges: list[dict], off105: int) -> str:
    """Find the deepest field span in `ranges` containing offset `off105`."""
    best = "?"
    best_len = 1 << 30
    for r in ranges:
        if r["start"] <= off105 < r["end"]:
            l = r["end"] - r["start"]
            if l < best_len:
                best_len = l
                best = r["path"]
    return best


def hex_snippet(data: bytes, off: int, length: int) -> str:
    end = min(off + length, len(data))
    return " ".join(f"{b:02X}" for b in data[off:end])


def diff_one(
    string_key: str,
    chunk104: bytes,
    chunk105: bytes,
) -> None:
    print("=" * 78)
    print(f"item: {string_key}")
    print(f"  size 1.04={len(chunk104)}  1.05={len(chunk105)}  delta={len(chunk105)-len(chunk104):+d}")

    # Track 1.05 chunk to get field offsets (may fail mid-way)
    res = crimson_rs.parse_iteminfo_tracked(chunk105)
    if res["spans"]:
        ranges = res["spans"][0]["ranges"]
        print("  1.05 parser: ok")
    else:
        err = res["error_span"]
        ranges = err["ranges"]
        print(f"  1.05 parser: FAIL at '{err['path']}' (offset 0x{err['end']:X})")

    s = difflib.SequenceMatcher(a=chunk104, b=chunk105, autojunk=False)
    print()
    print(f"  {'op':<8} {'1.04 range':<18} {'1.05 range':<18} {'len':<5} 1.05 field path")
    print(f"  {'-'*8} {'-'*18} {'-'*18} {'-'*5} {'-'*40}")
    for tag, i1, i2, j1, j2 in s.get_opcodes():
        if tag == "equal":
            continue
        len104 = i2 - i1
        len105 = j2 - j1
        path = field_at(ranges, j1)
        print(
            f"  {tag:<8} 0x{i1:04X}-0x{i2:04X}    0x{j1:04X}-0x{j2:04X}    "
            f"{len105 - len104:+5d}  {path}"
        )
        # show byte content for short blocks
        snip104 = hex_snippet(chunk104, i1, min(len104, 24))
        snip105 = hex_snippet(chunk105, j1, min(len105, 24))
        print(f"           1.04: {snip104}")
        print(f"           1.05: {snip105}")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--pabgb-104", default=str(DEFAULT_104_PABGB))
    ap.add_argument("--pabgb-105", default=str(DEFAULT_105_PABGB))
    ap.add_argument("--baseline-104", default=str(DEFAULT_104_BASELINE))
    ap.add_argument("--keys-105", default=str(DEFAULT_105_KEYS))
    ap.add_argument("--items-105", default=str(DEFAULT_105_ITEMS_JSONL))
    ap.add_argument("--item", action="append")
    ap.add_argument("--status")
    ap.add_argument("--top", type=int, default=3)
    args = ap.parse_args()

    data104 = Path(args.pabgb_104).read_bytes()
    keys104 = load_baseline_keys(Path(args.baseline_104))
    anchors104 = find_anchors(data104, keys104)

    sk104_to_idx: dict[str, int] = {}
    for i, off in enumerate(anchors104):
        slen = struct.unpack_from("<I", data104, off + 4)[0]
        sk = data104[off + 8 : off + 8 + slen].decode("utf-8", errors="replace")
        sk104_to_idx[sk] = i

    data105 = Path(args.pabgb_105).read_bytes()
    keys105 = load_keys_txt(Path(args.keys_105))
    anchors105 = find_anchors(data105, keys105)

    sk105_to_idx: dict[str, int] = {}
    sk_to_status: dict[str, str] = {}
    with Path(args.items_105).open(encoding="utf-8") as fh:
        for line in fh:
            d = json.loads(line)
            if "string_key" in d and "_index" in d:
                sk105_to_idx[d["string_key"]] = d["_index"]
                sk_to_status[d["string_key"]] = d.get("_status", "?")

    if args.status:
        wanted = [
            sk for sk, st in sk_to_status.items() if st.startswith(args.status)
        ][: args.top]
    elif args.item:
        wanted = args.item
    else:
        wanted = [
            "Item_gimmick_resourcestorage_0001",
            "High_Meat",
        ]

    for sk in wanted:
        if sk not in sk104_to_idx:
            print(f"!! {sk} not in 1.04 baseline; skipping")
            continue
        if sk not in sk105_to_idx:
            print(f"!! {sk} not in 1.05; skipping")
            continue
        i104 = sk104_to_idx[sk]
        off104 = anchors104[i104]
        end104 = anchors104[i104 + 1] if i104 + 1 < len(anchors104) else len(data104)
        chunk104 = data104[off104:end104]

        i105 = sk105_to_idx[sk]
        off105 = anchors105[i105]
        end105 = anchors105[i105 + 1] if i105 + 1 < len(anchors105) else len(data105)
        chunk105 = data105[off105:end105]

        diff_one(sk, chunk104, chunk105)
        print()


if __name__ == "__main__":
    main()
