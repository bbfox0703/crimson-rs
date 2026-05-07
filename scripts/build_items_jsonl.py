"""Anchor-based items.jsonl builder.

Strategy:
    1. Load the CE-dumped key list (keys.txt) — 6,236 keys in their in-game
       order.
    2. Scan the 1.05 iteminfo binary for each key in order, validating that
       the bytes at that offset look like an `ItemInfo` start (key + small
       length + ASCII string_key + NUL).
    3. For each item chunk (offset → next-anchor), try to parse with
       crimson_rs.parse_iteminfo_tracked. If it consumes the full chunk,
       use the parsed dict directly. Otherwise fall back to a minimal
       record so every item is still represented:
           {
             "_index"     : int,        # in-game item index (0-based)
             "_anchor_off": int,        # file offset where the item begins
             "_anchor_size": int,       # size in bytes
             "_status"    : "ok" | "leftover:N" | "fail:<path>",
             "key"        : int,
             "string_key" : str,
             ... (other parsed fields when available) ...
           }

This guarantees every in-game item appears in items.jsonl, and downstream
tools that only need (index, key, string_key) keep working even if the
1.05 parser is incomplete.
"""

from __future__ import annotations

import argparse
import json
import struct
import sys
from collections import Counter
from pathlib import Path

import crimson_rs


def load_keys(path: Path) -> list[int]:
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


def is_ident_byte(b: int) -> bool:
    return (
        b == ord("_")
        or b == ord(" ")
        or 48 <= b <= 57
        or 65 <= b <= 90
        or 97 <= b <= 122
    )


def looks_like_item_start(data: bytes, off: int, expected_key: int) -> bool:
    if off + 12 > len(data):
        return False
    key, slen = struct.unpack_from("<II", data, off)
    if key != expected_key:
        return False
    if not (2 <= slen <= 64):
        return False
    if off + 8 + slen + 1 > len(data):
        return False
    sk = data[off + 8 : off + 8 + slen]
    if any(not is_ident_byte(b) for b in sk):
        return False
    if data[off + 8 + slen] != 0:
        return False
    return True


def find_anchors(data: bytes, keys: list[int]) -> list[int]:
    if not keys:
        return []
    if not looks_like_item_start(data, 0, keys[0]):
        sys.exit("First key does not appear at file offset 0")
    anchors = [0]
    for i in range(1, len(keys)):
        prev = anchors[i - 1]
        cursor = prev + 60  # items are at least ~60 bytes
        target = struct.pack("<I", keys[i])
        found = -1
        while cursor + 12 <= len(data):
            idx = data.find(target, cursor)
            if idx < 0:
                break
            if looks_like_item_start(data, idx, keys[i]):
                found = idx
                break
            cursor = idx + 1
        if found < 0:
            sys.exit(
                f"Anchor scan failed at i={i} key={keys[i]} "
                f"(searching from offset 0x{cursor:X})"
            )
        anchors.append(found)
    return anchors


def read_string_key(data: bytes, off: int) -> str:
    slen = struct.unpack_from("<I", data, off + 4)[0]
    return data[off + 8 : off + 8 + slen].decode("utf-8", errors="replace")


def parse_chunk(chunk: bytes) -> tuple[dict | None, str]:
    """Return (parsed_dict, status) where status is 'ok', 'leftover:N',
    or 'fail:<path>'."""
    res = crimson_rs.parse_iteminfo_tracked(chunk)
    spans = res["spans"]
    if spans:
        item = res["items"][0]
        end = spans[0]["end"]
        if end == len(chunk):
            return item, "ok"
        return item, f"leftover:{len(chunk) - end}"
    err = res.get("error_span", {})
    return None, f"fail:{err.get('path', '?')}"


def build(
    keys: list[int],
    data: bytes,
    out_path: Path,
) -> dict[str, int]:
    anchors = find_anchors(data, keys)
    status_hist: Counter[str] = Counter()

    with out_path.open("w", encoding="utf-8") as fh:
        for i, key in enumerate(keys):
            off = anchors[i]
            end = anchors[i + 1] if i + 1 < len(anchors) else len(data)
            size = end - off
            chunk = data[off:end]
            string_key = read_string_key(data, off)

            parsed, status = parse_chunk(chunk)
            status_hist[status.split(":", 1)[0]] += 1

            if parsed is not None:
                rec = dict(parsed)
            else:
                rec = {"key": key, "string_key": string_key}

            rec["_index"] = i
            rec["_anchor_off"] = off
            rec["_anchor_size"] = size
            rec["_status"] = status

            fh.write(json.dumps(rec, ensure_ascii=False, separators=(",", ":")))
            fh.write("\n")

    return dict(status_hist)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--keys", required=True)
    ap.add_argument("--pabgb", required=True)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()

    keys = load_keys(Path(args.keys))
    data = Path(args.pabgb).read_bytes()
    print(f"keys.txt        : {len(keys):,} keys")
    print(f"iteminfo.pabgb  : {len(data):,} bytes")

    out_path = Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    status_hist = build(keys, data, out_path)

    print(f"\n-> {out_path} ({out_path.stat().st_size:,} bytes)")
    print("status breakdown:")
    for status, n in sorted(
        status_hist.items(), key=lambda kv: -kv[1]
    ):
        pct = n / len(keys) * 100
        print(f"  {status:<10} {n:>5}  ({pct:.1f}%)")


if __name__ == "__main__":
    main()
