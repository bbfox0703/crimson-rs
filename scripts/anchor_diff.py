"""Use the CE-dumped item-key list to anchor 1.05 items in the binary,
then compare against the 1.04 baseline items.jsonl to find structural
differences (new/removed fields and where the bytes are inserted).

Usage:
    python scripts/anchor_diff.py
        --keys "D:/.../keys.txt"
        --pabgb "D:/.../iteminfo_1.05.pabgb"
        --baseline "D:/.../iteminfo_dump/items.jsonl"
        --out anchors.json

Outputs to stdout a small per-key diff report; writes the full anchor list
(item_index, key, file_offset, size_in_1.05, size_in_1.04_if_known) to
--out as JSON for downstream consumption.
"""

from __future__ import annotations

import argparse
import json
import struct
from pathlib import Path


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


def load_baseline(path: Path) -> dict[int, dict]:
    """Load 1.04 items.jsonl into {key: item_dict}."""
    out: dict[int, dict] = {}
    with path.open(encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            it = json.loads(line)
            out[it["key"]] = it
    return out


def looks_like_item_start(data: bytes, off: int, expected_key: int) -> bool:
    """Return True if `off` looks like the start of an ItemInfo record."""
    if off + 12 > len(data):
        return False
    key, slen = struct.unpack_from("<II", data, off)
    if key != expected_key:
        return False
    if not (2 <= slen <= 128):
        return False
    if off + 8 + slen + 1 > len(data):
        return False
    sk = data[off + 8 : off + 8 + slen]
    for b in sk:
        # Allow ASCII word chars + space, plus UTF-8 high bytes (>=0x80) so
        # 1.05 string_keys with Roman numerals (Ⅲ/Ⅳ/Ⅵ) are accepted.
        if not (b == ord("_") or b == ord(" ") or 48 <= b <= 57 or 65 <= b <= 90 or 97 <= b <= 122 or b >= 0x80):
            return False
    if data[off + 8 + slen] != 0:
        return False
    return True


def find_anchors(data: bytes, keys: list[int]) -> list[int]:
    """For each key in `keys`, locate the file offset where that item begins.

    The first item must start at offset 0. Each subsequent item begins after
    the previous one, so we scan forward from the previous offset for the
    next key's u32 LE encoding and validate the structural prefix.
    """
    n = len(data)
    anchors: list[int] = []
    if not keys:
        return anchors
    if not looks_like_item_start(data, 0, keys[0]):
        raise SystemExit("First key does not appear at file offset 0")
    anchors.append(0)

    cursor = 0
    for i in range(1, len(keys)):
        prev = anchors[i - 1]
        # Items are at least ~50 bytes; skip ahead a little to avoid catching
        # the same key as a self-reference inside the previous item.
        min_size = 60
        cursor = prev + min_size
        target = struct.pack("<I", keys[i])
        found = -1
        while cursor + 12 <= n:
            idx = data.find(target, cursor)
            if idx < 0:
                break
            if looks_like_item_start(data, idx, keys[i]):
                found = idx
                break
            cursor = idx + 1
        if found < 0:
            raise SystemExit(
                f"Anchor scan failed at i={i} key={keys[i]} "
                f"(searching from offset 0x{cursor:X})"
            )
        anchors.append(found)
    return anchors


def field_size_in_baseline(item: dict) -> int:
    """Estimate the byte size of a baseline (1.04) item by serialising the
    same field shapes we used for 1.05. Used for diff size estimates only."""
    # We don't actually serialise — just sum approximate sizes per the known
    # 1.04 ItemInfo layout. For the purpose of "what changed" reporting we
    # only need the integer byte count of the same-named fields.
    raise NotImplementedError


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--keys", required=True)
    ap.add_argument("--pabgb", required=True)
    ap.add_argument("--baseline", required=True)
    ap.add_argument("--out", default="anchors.json")
    ap.add_argument(
        "--top",
        type=int,
        default=20,
        help="how many size-delta examples to print (default 20)",
    )
    args = ap.parse_args()

    keys = load_keys(Path(args.keys))
    data = Path(args.pabgb).read_bytes()
    baseline = load_baseline(Path(args.baseline))

    print(f"keys.txt           : {len(keys):,} keys")
    print(f"iteminfo (1.05)    : {len(data):,} bytes")
    print(f"baseline (1.04)    : {len(baseline):,} items")
    print()

    anchors = find_anchors(data, keys)
    print(f"resolved anchors   : {len(anchors)} ({len(anchors)/len(keys)*100:.1f}%)")

    # Build size table for 1.05
    sizes_105: list[int] = []
    for i, off in enumerate(anchors):
        if i + 1 < len(anchors):
            sizes_105.append(anchors[i + 1] - off)
        else:
            sizes_105.append(len(data) - off)

    # Cross-reference with 1.04
    rows: list[dict] = []
    in_baseline = 0
    not_in_baseline: list[int] = []
    for i, key in enumerate(keys):
        row = {
            "i": i,
            "key": key,
            "offset_105": anchors[i],
            "size_105": sizes_105[i],
            "in_104": key in baseline,
            "string_key_104": baseline.get(key, {}).get("string_key"),
        }
        if key in baseline:
            in_baseline += 1
        else:
            not_in_baseline.append(key)
        rows.append(row)
    print(
        f"keys in 1.04 baseline: {in_baseline:,} / {len(keys):,} "
        f"({in_baseline/len(keys)*100:.1f}%)"
    )
    print(f"new in 1.05         : {len(keys) - in_baseline}")
    print(f"removed (in 1.04 but not 1.05): {len(baseline) - in_baseline}")

    # Write full anchors json
    out_path = Path(args.out)
    with out_path.open("w", encoding="utf-8") as fh:
        json.dump(rows, fh, ensure_ascii=False, indent=1)
    print(f"\nwrote {out_path} ({out_path.stat().st_size:,} bytes)")

    # ── Size distribution & top samples ──
    print("\nFirst 20 items in 1.05 and their sizes:")
    for row in rows[:20]:
        sk = row["string_key_104"] or "(new in 1.05)"
        print(
            f"  i={row['i']:>4} key={row['key']:<10} off=0x{row['offset_105']:>7X} "
            f"size={row['size_105']:>5} {sk}"
        )

    # ── For diff: compare item sizes 1.04 vs 1.05 if same key
    # Re-compute 1.04 size from offset deltas IS NOT available without parsing
    # the baseline file. But we can at least flag new items not in 1.04 here.
    print(f"\nTop {args.top} items new in 1.05 (no 1.04 baseline):")
    seen = 0
    for row in rows:
        if not row["in_104"]:
            print(f"  i={row['i']:>4} key={row['key']:<10} size={row['size_105']:>5}")
            seen += 1
            if seen >= args.top:
                break

    # ── Print 1.04-only keys (removed in 1.05) ──
    rem = sorted(set(baseline.keys()) - set(keys))
    print(f"\nFirst {min(args.top, len(rem))} keys removed in 1.05:")
    for k in rem[: args.top]:
        sk = baseline[k].get("string_key", "")
        print(f"  key={k:<10} {sk}")


if __name__ == "__main__":
    main()
