"""Cross-version drift RE: 1.17 → 1.18 iteminfo schema.

Same lightweight byte-diff workflow as `diff_112_113.py` (which is the
proven template — see its docstring for the algorithm). The in-tree
parser handles 1.17 cleanly and fails on every 1.18 item, so the 1.17
side supplies the per-field span map and the 1.18 side is walked against
it byte-for-byte.

Patches routinely ship *value* churn on top of the schema drift, so
same-length substitutions are common; MAX_SHIFT tolerates them as
`shift_old == shift_new` steps rather than aborting the walk.

Inputs:
    out/iteminfo.1.17.pabgb   (previous baseline; override with $OLD_ITEMINFO)
    out/iteminfo.pabgb        (current 1.18 extract, written by export_for_ce.py)
    data/keys.txt             (current 1.18 key order, 6,573 keys)

Output: `out/diff_117_118/report.txt` — sorted by event signature so
schema changes show up as the top-frequency lines.

Usage:
    python scripts/diff_117_118.py
"""

from __future__ import annotations

import os
import struct
import sys
from collections import Counter, defaultdict
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO))  # for crimson_rs from .venv
import crimson_rs  # noqa: E402

OLD_PABGB = Path(
    os.environ.get("OLD_ITEMINFO", str(REPO / "out" / "iteminfo.1.17.pabgb"))
)
NEW_PABGB = REPO / "out" / "iteminfo.pabgb"
OUT_DIR = REPO / "out" / "diff_117_118"
OUT_DIR.mkdir(parents=True, exist_ok=True)

MAX_SHIFT = 16  # brute-force shift range on each side at a mismatch
CONFIRM = 30  # consecutive matching downstream bytes required to accept a shift


def _is_ident_byte(b: int) -> bool:
    return (
        b == ord("_") or b == ord(" ")
        or 48 <= b <= 57 or 65 <= b <= 90 or 97 <= b <= 122
        or b >= 0x80
    )


def _looks_like_item_start(data: bytes, off: int, expected_key: int) -> bool:
    if off + 12 > len(data):
        return False
    key, slen = struct.unpack_from("<II", data, off)
    if key != expected_key:
        return False
    if not (2 <= slen <= 128):
        return False
    if off + 8 + slen + 1 > len(data):
        return False
    if any(not _is_ident_byte(b) for b in data[off + 8 : off + 8 + slen]):
        return False
    return data[off + 8 + slen] == 0


def find_anchors(data: bytes, keys: list[int]) -> list[int | None]:
    if not keys:
        return []
    if not _looks_like_item_start(data, 0, keys[0]):
        sys.exit("First key not at offset 0")
    anchors: list[int | None] = [0]
    last = 0
    for i in range(1, len(keys)):
        cursor = last + 60
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
            anchors.append(None)
        else:
            anchors.append(found)
            last = found
    return anchors


def field_at(spans_ranges: list[dict], rel_off: int) -> str:
    last_path = "<pre-item>"
    for r in spans_ranges:
        if r["start"] <= rel_off < r["end"]:
            return r["path"]
        if r["end"] <= rel_off:
            last_path = r["path"]
        if r["start"] > rel_off:
            break
    return f">{last_path}"


def tandem_walk(
    old_bytes: bytes,
    new_bytes: bytes,
    spans_ranges: list[dict],
) -> tuple[list[dict], bool]:
    events: list[dict] = []
    o, n = 0, 0
    while o < len(old_bytes) and n < len(new_bytes):
        if old_bytes[o] == new_bytes[n]:
            o += 1
            n += 1
            continue
        accepted = None
        for total in range(1, 2 * MAX_SHIFT + 1):
            best = None
            for so in range(0, min(MAX_SHIFT, total) + 1):
                sn = total - so
                if sn < 0 or sn > MAX_SHIFT:
                    continue
                no = o + so
                nn = n + sn
                old_rem = len(old_bytes) - no
                new_rem = len(new_bytes) - nn
                if min(old_rem, new_rem) >= CONFIRM:
                    ok = old_bytes[no : no + CONFIRM] == new_bytes[nn : nn + CONFIRM]
                else:
                    ok = old_bytes[no:] == new_bytes[nn:]
                if ok:
                    score = (min(so, sn), total)
                    if best is None or score < best[0]:
                        best = (score, so, sn)
            if best is not None:
                accepted = best
                break
        if accepted is None:
            events.append({
                "old_off": o,
                "new_off": n,
                "field": field_at(spans_ranges, o),
                "removed_hex": old_bytes[o:o + 16].hex(),
                "inserted_hex": new_bytes[n:n + 16].hex(),
                "ABORTED": True,
            })
            return events, False
        _, so, sn = accepted
        events.append({
            "old_off": o,
            "new_off": n,
            "field": field_at(spans_ranges, o),
            "removed_hex": old_bytes[o:o + so].hex(),
            "inserted_hex": new_bytes[n:n + sn].hex(),
            "shift_old": so,
            "shift_new": sn,
        })
        o += so
        n += sn
    return events, True


def main():
    old_bytes_full = OLD_PABGB.read_bytes()
    new_bytes_full = NEW_PABGB.read_bytes()
    print(f"1.17: {len(old_bytes_full):,}B   1.18: {len(new_bytes_full):,}B")

    tracked = crimson_rs.parse_iteminfo_tracked(old_bytes_full)
    old_items, old_spans = tracked["items"], tracked["spans"]
    print(f"1.17 tracked: {len(old_items)} items")

    old_by_key: dict[int, tuple[int, dict]] = {}
    for idx, it in enumerate(old_items):
        old_by_key[it["key"]] = (idx, it)

    new_keys = [
        int(l.strip())
        for l in (REPO / "data" / "keys.txt").read_text().split()
        if l.strip()
    ]
    print(f"1.18 keys.txt: {len(new_keys)} keys")
    new_anchors = find_anchors(new_bytes_full, new_keys)
    resolved = sum(1 for a in new_anchors if a is not None)
    print(f"1.18 anchors resolved: {resolved}/{len(new_keys)}")

    common = []
    for new_idx, k in enumerate(new_keys):
        a = new_anchors[new_idx]
        if a is None or k not in old_by_key:
            continue
        common.append((k, new_idx, a))
    print(f"Common items: {len(common)}")

    next_off = [None] * len(new_anchors)
    last = len(new_bytes_full)
    for i in range(len(new_anchors) - 1, -1, -1):
        if new_anchors[i] is not None:
            next_off[i] = last
            last = new_anchors[i]

    signature_count: Counter[str] = Counter()
    signature_examples: dict[str, list[tuple[int, str, int]]] = defaultdict(list)
    aborted_count = 0
    fully_realigned = 0
    per_item_events: dict[int, list[dict]] = {}
    offset_counter: Counter[int] = Counter()
    drift_distance_from_end: Counter[int] = Counter()
    size_delta: Counter[int] = Counter()
    first_event_field: Counter[str] = Counter()

    for k, new_idx, new_off in common:
        old_idx, _it = old_by_key[k]
        sp = old_spans[old_idx]
        old_lo, old_hi = sp["start"], sp["end"]
        new_lo = new_off
        new_hi = next_off[new_idx]
        if new_hi is None:
            continue
        old_chunk = old_bytes_full[old_lo:old_hi]
        new_chunk = new_bytes_full[new_lo:new_hi]
        size_delta[len(new_chunk) - len(old_chunk)] += 1
        rel_ranges = [
            {
                "start": r["start"] - old_lo,
                "end": r["end"] - old_lo,
                "path": r["path"],
                "ty": r["ty"],
            }
            for r in sp["ranges"]
        ]
        events, done = tandem_walk(old_chunk, new_chunk, rel_ranges)
        per_item_events[k] = events
        if events:
            first_event_field[events[0]["field"]] += 1
        if not done:
            aborted_count += 1
            continue
        fully_realigned += 1
        old_size = old_hi - old_lo
        for e in events:
            sig = (
                f"@{e['field']}  rm:{len(e['removed_hex']) // 2}B "
                f"ins:{len(e['inserted_hex']) // 2}B  "
                f"removed=0x{e['removed_hex'] or '(none)'} "
                f"inserted=0x{e['inserted_hex'] or '(none)'}"
            )
            signature_count[sig] += 1
            if len(signature_examples[sig]) < 5:
                signature_examples[sig].append((k, e["field"], e["old_off"]))
            offset_counter[e["old_off"]] += 1
            drift_distance_from_end[old_size - e["old_off"]] += 1

    # Field-level aggregation ignoring the concrete bytes — this is what
    # exposes "field X always gains/loses N bytes" across value churn.
    field_shift: Counter[str] = Counter()
    for evs in per_item_events.values():
        for e in evs:
            if e.get("ABORTED"):
                continue
            so = len(e["removed_hex"]) // 2
            sn = len(e["inserted_hex"]) // 2
            if so == sn:
                continue  # pure value substitution, not a length drift
            field_shift[f"{e['field']}  rm:{so}B ins:{sn}B"] += 1

    zero_event_items = [k for k, evs in per_item_events.items() if not evs]

    report_path = OUT_DIR / "report.txt"
    with report_path.open("w", encoding="utf-8") as fh:
        fh.write(
            f"1.17 -> 1.18 iteminfo drift report\n"
            f"{'=' * 60}\n"
            f"1.17 bytes: {len(old_bytes_full):,}  items: {len(old_items)}\n"
            f"1.18 bytes: {len(new_bytes_full):,}  keys: {len(new_keys)}  "
            f"anchors_resolved: {resolved}\n"
            f"common items walked: {len(common)}\n"
            f"items fully realigned: {fully_realigned}\n"
            f"items aborted mid-walk: {aborted_count}\n"
            f"items with zero drift events: {len(zero_event_items)}\n"
            f"\n--- Per-item size delta (new - old) ---\n"
        )
        for d, cnt in size_delta.most_common(30):
            fh.write(f"  {d:+6d} B  ({cnt}x)\n")

        fh.write("\n--- First drift event field (per item) ---\n")
        for f, cnt in first_event_field.most_common(30):
            fh.write(f"  {cnt:>6}x  {f}\n")

        fh.write("\n--- LENGTH-CHANGING drift by field (value subs excluded) ---\n")
        for f, cnt in field_shift.most_common(40):
            fh.write(f"  {cnt:>6}x  {f}\n")

        fh.write("\n--- Top drift signatures (by frequency) ---\n")
        for sig, cnt in signature_count.most_common(40):
            fh.write(f"\n[{cnt:>5}x] {sig}\n")
            for ex_key, ex_field, ex_off in signature_examples[sig][:3]:
                fh.write(
                    f"        e.g. key={ex_key}  rel_off={ex_off}  field={ex_field}\n"
                )

        fh.write(f"\n--- Distinct signature count: {len(signature_count)} ---\n")

        fh.write("\n--- Top drift offsets in the OLD item (rel) ---\n")
        for off, cnt in offset_counter.most_common(20):
            fh.write(f"  off={off}  ({cnt}x)\n")

        fh.write("\n--- Distribution: drift distance from item END (rel) ---\n")
        for dist, cnt in drift_distance_from_end.most_common(20):
            fh.write(f"  end-off={dist}  ({cnt}x)\n")

        fh.write("\n--- Aborted items (first 25) ---\n")
        aborted_keys = [
            k for k, evs in per_item_events.items() if evs and evs[-1].get("ABORTED")
        ]
        for k in aborted_keys[:25]:
            evs = per_item_events[k]
            fh.write(f"\nkey={k}  events={len(evs)}\n")
            for e in evs[-5:]:
                fh.write(f"  off=({e['old_off']},{e['new_off']})  field={e['field']}\n")
                fh.write(f"    rm  =0x{e['removed_hex']}\n")
                fh.write(f"    ins =0x{e['inserted_hex']}\n")
                if e.get("ABORTED"):
                    fh.write("    ABORTED\n")
        fh.write(f"\n--- Total aborted: {len(aborted_keys)} ---\n")

    print(f"Wrote {report_path}")
    print(f"fully realigned: {fully_realigned}  aborted: {aborted_count}")
    print("\nTop 15 LENGTH-CHANGING drifts by field:")
    for f, cnt in field_shift.most_common(15):
        print(f"  [{cnt:>5}x] {f}")


if __name__ == "__main__":
    main()
