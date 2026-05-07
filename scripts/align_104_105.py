"""Cross-version field alignment for one item.

Take a 1.04 item with full parser-tracked spans (from
out/baselines/1.04/spans.json), and walk each named 1.04 field forward
in the 1.05 chunk to figure out where the 1.05 schema diverges from
1.04. Emits a report of "1.04 field X at 1.04 offset O is found at 1.05
offset O+shift; shift went 0 -> 5 between fields A and B" — the field
boundary at which `shift` jumps tells you what the new 1.05 bytes are
inserted between.

Uses the 1.05 parser only via `parse_iteminfo_tracked` for the early
core fields where it is provably correct (the parts of `ItemInfoCore`
that don't change between 1.04 and 1.05).

Usage:
    python scripts/align_104_105.py --item Item_gimmick_resourcestorage_0001
    python scripts/align_104_105.py --item Pyeonjeon_Arrow
"""

from __future__ import annotations

import argparse
import json
import struct
import sys
from pathlib import Path

import crimson_rs

REPO = Path(__file__).resolve().parent.parent

DEFAULT_104_PABGB = REPO / "out" / "baselines" / "1.04" / "iteminfo.pabgb"
DEFAULT_104_BASELINE = REPO / "out" / "baselines" / "1.04" / "items.jsonl"
DEFAULT_104_SPANS = REPO / "out" / "baselines" / "1.04" / "spans.json"
DEFAULT_105_PABGB = REPO / "out" / "iteminfo.pabgb"
DEFAULT_105_KEYS = REPO / "data" / "keys.txt"


def _is_ident(b):
    return (
        b == ord("_")
        or b == ord(" ")
        or 48 <= b <= 57
        or 65 <= b <= 90
        or 97 <= b <= 122
    )


def _looks(data, off, key):
    if off + 12 > len(data):
        return False
    k, slen = struct.unpack_from("<II", data, off)
    if k != key or not (2 <= slen <= 64) or off + 8 + slen + 1 > len(data):
        return False
    sk = data[off + 8 : off + 8 + slen]
    return all(_is_ident(b) for b in sk) and data[off + 8 + slen] == 0


def find_anchors(data, keys):
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


def load_baseline_keys(path):
    keys = []
    sk_to_key = {}
    with path.open(encoding="utf-8") as fh:
        for line in fh:
            d = json.loads(line)
            if "key" in d:
                keys.append(d["key"])
                sk_to_key[d["string_key"]] = d["key"]
    return keys, sk_to_key


def load_keys_txt(path):
    keys = []
    for raw in path.read_text(encoding="utf-8").splitlines():
        s = raw.strip()
        if not s:
            continue
        try:
            k = int(s)
        except ValueError:
            break
        if k in (0, 0xFFFFFFFF):
            break
        keys.append(k)
    return keys


def align_chunks(c104, c105, fields_104):
    """For each 1.04 field span, locate the same byte sequence in c105 and
    compute the shift. Emit shift transitions."""
    rows = []
    last_shift = None
    last_field = None
    for f in fields_104:
        s, e = f["start"], f["end"]
        ln = e - s
        if ln == 0:
            continue
        bytes_104 = bytes(c104[s:e])
        # search for bytes_104 within a window in c105 around s+last_shift
        window_lo = max(0, (s + (last_shift or 0)) - 32)
        window_hi = min(len(c105), e + (last_shift or 0) + 32)
        idx = -1
        if last_shift is not None:
            target_pos = s + last_shift
            if (
                0 <= target_pos
                and target_pos + ln <= len(c105)
                and c105[target_pos : target_pos + ln] == bytes_104
            ):
                idx = target_pos
        if idx < 0:
            try:
                idx = c105.index(bytes_104, window_lo, window_hi)
            except ValueError:
                idx = -1
        if idx < 0:
            # widen the window to whole chunk
            try:
                idx = c105.index(bytes_104)
            except ValueError:
                idx = -2
        shift = (idx - s) if idx >= 0 else None
        rows.append(
            {
                "path": f["path"],
                "ty": f.get("ty"),
                "104_start": s,
                "104_end": e,
                "len": ln,
                "found_in_105": idx,
                "shift": shift,
                "bytes": " ".join(f"{b:02X}" for b in bytes_104[: min(ln, 16)]),
            }
        )
        if shift is not None:
            if last_shift is not None and shift != last_shift:
                rows[-1]["transition_from"] = last_shift
                rows[-1]["transition_to"] = shift
                rows[-1]["transition_after"] = last_field
            last_shift = shift
        last_field = f["path"]
    return rows


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--pabgb-104", default=str(DEFAULT_104_PABGB))
    ap.add_argument("--baseline-104", default=str(DEFAULT_104_BASELINE))
    ap.add_argument("--spans-104", default=str(DEFAULT_104_SPANS))
    ap.add_argument("--pabgb-105", default=str(DEFAULT_105_PABGB))
    ap.add_argument("--keys-105", default=str(DEFAULT_105_KEYS))
    ap.add_argument("--item", required=True, help="string_key of the item to align")
    ap.add_argument(
        "--show",
        choices=["all", "transitions", "head"],
        default="transitions",
        help="how much detail to print (default: only shift transitions)",
    )
    ap.add_argument("--head", type=int, default=20)
    args = ap.parse_args()

    sk = args.item

    spans104 = json.loads(Path(args.spans_104).read_text(encoding="utf-8"))
    if sk not in spans104 or not spans104[sk].get("ok"):
        sys.exit(f"{sk} not in 1.04 spans (or 1.04 parse failed)")
    fields_104 = spans104[sk]["fields"]

    data104 = Path(args.pabgb_104).read_bytes()
    keys104, sk_to_key104 = load_baseline_keys(Path(args.baseline_104))
    anchors104 = find_anchors(data104, keys104)
    i104 = keys104.index(sk_to_key104[sk])
    c104 = data104[anchors104[i104] : anchors104[i104 + 1] if i104 + 1 < len(anchors104) else len(data104)]

    data105 = Path(args.pabgb_105).read_bytes()
    keys105 = load_keys_txt(Path(args.keys_105))
    anchors105 = find_anchors(data105, keys105)
    if sk_to_key104[sk] not in keys105:
        sys.exit(f"{sk} key={sk_to_key104[sk]} not in 1.05")
    i105 = keys105.index(sk_to_key104[sk])
    c105 = data105[anchors105[i105] : anchors105[i105 + 1] if i105 + 1 < len(anchors105) else len(data105)]

    print(f"item: {sk}")
    print(f"  1.04 chunk: {len(c104)}B    1.05 chunk: {len(c105)}B    delta=+{len(c105)-len(c104)}")
    print()
    rows = align_chunks(c104, c105, fields_104)

    if args.show == "all":
        print(f"{'104_off':>8}  {'shift':>5}  {'len':>3}  path")
        for r in rows:
            sh = "?" if r["shift"] is None else f"{r['shift']:+d}"
            print(f"  0x{r['104_start']:04X}  {sh:>5}  {r['len']:>3}  {r['path']}")
    elif args.show == "head":
        for r in rows[: args.head]:
            sh = "?" if r["shift"] is None else f"{r['shift']:+d}"
            print(f"  0x{r['104_start']:04X}  {sh:>5}  {r['len']:>3}  {r['path']:50s}  {r['bytes']}")
    else:
        # transitions only
        print("Shift transitions:")
        last_shift = 0
        for r in rows:
            if r["shift"] is not None and r["shift"] != last_shift:
                print(
                    f"  after '{rows[rows.index(r)-1]['path'] if rows.index(r) > 0 else '<start>'}' "
                    f"(1.04 0x{rows[rows.index(r)-1]['104_end'] if rows.index(r) > 0 else 0:04X}): "
                    f"shift {last_shift:+d} -> {r['shift']:+d}  "
                    f"(next field '{r['path']}' at 1.04 0x{r['104_start']:04X})"
                )
                last_shift = r["shift"]
        print(f"  final shift: {last_shift:+d}  (1.05 - 1.04 size = {len(c105) - len(c104):+d})")
        # Show fields where shift didn't resolve (= field couldn't be located in 1.05)
        unresolved = [r for r in rows if r["shift"] is None]
        if unresolved:
            print(f"\nUnresolved fields ({len(unresolved)}):")
            for r in unresolved[:20]:
                print(f"  0x{r['104_start']:04X} len={r['len']}  {r['path']:50s}  {r['bytes']}")


if __name__ == "__main__":
    main()
