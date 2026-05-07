"""Side-by-side byte diff of 1.04 vs 1.05 ItemInfo chunks.

Anchors both binaries by item key and prints the bytes for one or more
items in two parallel hex columns, highlighting the first divergence
offset. The intent is to localize the 1.05 binary-format change(s) that
break parsing — see scripts/CLAUDE.md "Remaining work" for the failure
clusters this is meant to crack.

Usage:
    python scripts/diff_104_105.py                                 # default: a few representative cases
    python scripts/diff_104_105.py --item Item_gimmick_resourcestorage_0001
    python scripts/diff_104_105.py --item High_Meat --bytes 800
    python scripts/diff_104_105.py --status fail:item_bundle_data_list
"""

from __future__ import annotations

import argparse
import json
import struct
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

DEFAULT_104_PABGB = REPO / "out" / "baselines" / "1.04" / "iteminfo.pabgb"
DEFAULT_104_BASELINE = REPO / "out" / "baselines" / "1.04" / "items.jsonl"
DEFAULT_105_PABGB = REPO / "out" / "iteminfo.pabgb"
DEFAULT_105_KEYS = REPO / "data" / "keys.txt"
DEFAULT_105_ITEMS_JSONL = REPO / "out" / "items.jsonl"


def _is_ident_byte(b: int) -> bool:
    return (
        b == ord("_")
        or b == ord(" ")
        or 48 <= b <= 57
        or 65 <= b <= 90
        or 97 <= b <= 122
    )


def _looks_like_item_start(data: bytes, off: int, expected_key: int) -> bool:
    if off + 12 > len(data):
        return False
    key, slen = struct.unpack_from("<II", data, off)
    if key != expected_key:
        return False
    if not (2 <= slen <= 64):
        return False
    if off + 8 + slen + 1 > len(data):
        return False
    if any(not _is_ident_byte(b) for b in data[off + 8 : off + 8 + slen]):
        return False
    return data[off + 8 + slen] == 0


def find_anchors(data: bytes, keys: list[int]) -> list[int]:
    if not keys:
        return []
    if not _looks_like_item_start(data, 0, keys[0]):
        sys.exit(f"First key {keys[0]} does not appear at offset 0")
    anchors = [0]
    for i in range(1, len(keys)):
        cursor = anchors[-1] + 60
        target = struct.pack("<I", keys[i])
        found = -1
        while cursor + 12 <= len(data):
            idx = data.find(target, cursor)
            if idx < 0:
                break
            if _looks_like_item_start(data, idx, keys[i]):
                found = idx
                break
            cursor = idx + 1
        if found < 0:
            sys.exit(f"Anchor scan failed at i={i} key={keys[i]}")
        anchors.append(found)
    return anchors


def load_baseline_keys(path: Path) -> list[int]:
    """Load 1.04 baseline jsonl in file order; return list of keys."""
    keys: list[int] = []
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
    keys: list[int] = []
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


def hex_block(data: bytes, off: int, length: int) -> list[str]:
    """Return per-row hex strings for `data[off:off+length]`, 16 bytes per row.

    Each row: '<offset:08X>  <hex bytes 16>  <ascii 16>'.
    """
    rows = []
    end = min(off + length, len(data))
    for r in range(off, end, 16):
        chunk = data[r : min(r + 16, end)]
        hex_part = " ".join(f"{b:02X}" for b in chunk)
        hex_part = hex_part.ljust(16 * 3 - 1)
        ascii_part = "".join(chr(b) if 32 <= b < 127 else "." for b in chunk)
        rows.append(f"{r - off:04X}  {hex_part}  {ascii_part}")
    return rows


def print_side_by_side(
    label_a: str,
    chunk_a: bytes,
    label_b: str,
    chunk_b: bytes,
    max_bytes: int,
    first_diff: int | None,
) -> None:
    rows_a = hex_block(chunk_a, 0, min(max_bytes, len(chunk_a)))
    rows_b = hex_block(chunk_b, 0, min(max_bytes, len(chunk_b)))

    width_a = max(len(r) for r in rows_a) if rows_a else 0
    width_a = max(width_a, len(label_a))

    print()
    print(f"  {label_a.ljust(width_a)}    {label_b}")
    print(f"  {'-' * width_a}    {'-' * len(label_b)}")
    n = max(len(rows_a), len(rows_b))
    for i in range(n):
        ra = rows_a[i] if i < len(rows_a) else ""
        rb = rows_b[i] if i < len(rows_b) else ""
        # Mark the row containing the first divergence
        marker = ""
        if first_diff is not None:
            row_off = i * 16
            if row_off <= first_diff < row_off + 16:
                marker = "  <-- first diff"
        print(f"  {ra.ljust(width_a)}    {rb}{marker}")


def first_byte_diff(a: bytes, b: bytes) -> int | None:
    n = min(len(a), len(b))
    for i in range(n):
        if a[i] != b[i]:
            return i
    if len(a) != len(b):
        return n
    return None


def diff_one(
    string_key: str,
    key104: int | None,
    key105: int | None,
    data104: bytes,
    anchors104: list[int],
    keys104: list[int],
    data105: bytes,
    anchors105: list[int],
    keys105: list[int],
    sk_to_status_105: dict[str, str],
    max_bytes: int,
) -> None:
    print("=" * 72)
    print(f"item: {string_key}")

    if key104 is None or key104 not in keys104:
        print("  not present in 1.04 baseline")
        return
    if key105 is None or key105 not in keys105:
        print("  not present in 1.05 keys.txt")
        return

    i104 = keys104.index(key104)
    off104 = anchors104[i104]
    end104 = anchors104[i104 + 1] if i104 + 1 < len(anchors104) else len(data104)
    chunk104 = data104[off104:end104]

    i105 = keys105.index(key105)
    off105 = anchors105[i105]
    end105 = anchors105[i105 + 1] if i105 + 1 < len(anchors105) else len(data105)
    chunk105 = data105[off105:end105]

    status = sk_to_status_105.get(string_key, "?")
    print(
        f"  1.04: key={key104} idx={i104} off=0x{off104:X} size={len(chunk104)}"
    )
    print(
        f"  1.05: key={key105} idx={i105} off=0x{off105:X} size={len(chunk105)}"
        f"  (size delta={len(chunk105)-len(chunk104):+d})"
    )
    print(f"  1.05 parser status: {status}")

    fd = first_byte_diff(chunk104, chunk105)
    if fd is None:
        print("  No byte differences in compared range")
    else:
        print(f"  First byte diff at offset 0x{fd:X} ({fd})")

    print_side_by_side(
        f"1.04 [{string_key}]", chunk104, f"1.05 [{string_key}]", chunk105, max_bytes, fd
    )


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--pabgb-104", default=str(DEFAULT_104_PABGB))
    ap.add_argument("--pabgb-105", default=str(DEFAULT_105_PABGB))
    ap.add_argument("--baseline-104", default=str(DEFAULT_104_BASELINE))
    ap.add_argument("--keys-105", default=str(DEFAULT_105_KEYS))
    ap.add_argument("--items-105", default=str(DEFAULT_105_ITEMS_JSONL))
    ap.add_argument(
        "--item",
        action="append",
        help="string_key of item(s) to diff. Can be passed multiple times. "
        "If omitted, a default representative set is used.",
    )
    ap.add_argument(
        "--status",
        help="Pick all 1.05 items whose status starts with this prefix "
        "(e.g. 'fail:item_bundle_data_list'). Limited by --top.",
    )
    ap.add_argument("--top", type=int, default=3, help="Max items when using --status")
    ap.add_argument(
        "--bytes",
        type=int,
        default=800,
        help="How many bytes per chunk to show in the hex dump (default 800)",
    )
    args = ap.parse_args()

    # Load 1.04
    data104 = Path(args.pabgb_104).read_bytes()
    keys104 = load_baseline_keys(Path(args.baseline_104))
    print(f"1.04 binary: {len(data104):,}B  baseline jsonl: {len(keys104):,} items")
    anchors104 = find_anchors(data104, keys104)
    print(f"1.04 anchors resolved: {len(anchors104):,}")

    # Build 1.04 string_key -> key map by reading anchor headers
    sk104_to_key: dict[str, int] = {}
    for i, off in enumerate(anchors104):
        key = keys104[i]
        slen = struct.unpack_from("<I", data104, off + 4)[0]
        sk = data104[off + 8 : off + 8 + slen].decode("utf-8", errors="replace")
        sk104_to_key[sk] = key

    # Load 1.05
    data105 = Path(args.pabgb_105).read_bytes()
    keys105 = load_keys_txt(Path(args.keys_105))
    print(f"1.05 binary: {len(data105):,}B  keys.txt: {len(keys105):,} items")
    anchors105 = find_anchors(data105, keys105)
    print(f"1.05 anchors resolved: {len(anchors105):,}")

    # Build 1.05 string_key -> (key, status) by reading items.jsonl
    sk105_to_key: dict[str, int] = {}
    sk_to_status_105: dict[str, str] = {}
    with Path(args.items_105).open(encoding="utf-8") as fh:
        for line in fh:
            d = json.loads(line)
            if "string_key" in d and "key" in d:
                sk105_to_key[d["string_key"]] = d["key"]
                sk_to_status_105[d["string_key"]] = d.get("_status", "?")

    # Pick which items to diff
    if args.status:
        wanted = [
            sk
            for sk, st in sk_to_status_105.items()
            if st.startswith(args.status)
        ][: args.top]
    elif args.item:
        wanted = args.item
    else:
        wanted = [
            "Item_gimmick_resourcestorage_0001",
            "Item_gimmick_collectionstorage_0001",
            "High_Meat",
            "Boss_Reward_SuperWeapon",
        ]

    for sk in wanted:
        key104 = sk104_to_key.get(sk)
        key105 = sk105_to_key.get(sk)
        diff_one(
            sk,
            key104,
            key105,
            data104,
            anchors104,
            keys104,
            data105,
            anchors105,
            keys105,
            sk_to_status_105,
            args.bytes,
        )


if __name__ == "__main__":
    main()
