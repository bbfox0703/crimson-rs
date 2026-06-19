"""Cross-version drift RE: 1.11 → 1.12 iteminfo schema.

Lightweight byte-diff workflow (no sibling parser install needed — the
in-tree parser handles 1.11 cleanly). For each item key present in both
1.11 and 1.12, walk per-byte through the old + new chunks side-by-side;
when the bytes diverge, brute-force [1..8] byte shifts on each side and
accept whichever shift restores >=30 consecutive matching downstream
bytes. Record the drift event and resume.

Field context: every drift event is tagged with the 1.11 field path
whose span the old-side offset falls inside (from
`parse_iteminfo_tracked`'s `spans[*].ranges` — same data the parser
emits while reading the binary).

Inputs:
    out/iteminfo.1.11.pabgb   (previous-version baseline — copy from the
                               gamedata-bin/1.11/ portable archive)
    out/iteminfo.pabgb        (current 1.12 extract, written by export_for_ce.py)
    data/keys.txt             (current 1.12 key order, 6,483 keys)

Output: `out/diff_111_112/report.txt` — sorted by event signature so
schema changes show up as the top-frequency lines.

Usage:
    python scripts/diff_111_112.py
"""

from __future__ import annotations

import struct
import sys
from collections import Counter, defaultdict
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO))  # for crimson_rs from .venv
import crimson_rs  # noqa: E402

OLD_PABGB = REPO / "out" / "iteminfo.1.11.pabgb"
NEW_PABGB = REPO / "out" / "iteminfo.pabgb"
OUT_DIR = REPO / "out" / "diff_111_112"
OUT_DIR.mkdir(parents=True, exist_ok=True)

MAX_SHIFT = 8  # brute-force shift range on each side at a mismatch
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
    """Anchor every key to its item-start offset in `data` (same scanner
    export_for_ce.py uses)."""
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
    """Return the field path whose [start, end) contains rel_off in the
    1.11 item span. If past the end, returns the LAST tracked path with a
    ">" marker so we can still see context."""
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
    """Walk old + new in lock-step. At each mismatch, try all
    (shift_old, shift_new) with shift_old + shift_new > 0 and
    max(shift_old, shift_new) <= MAX_SHIFT. Accept the *minimum total
    shift* that yields CONFIRM consecutive matches downstream. Returns
    (events, fully_realigned).
    """
    events: list[dict] = []
    o, n = 0, 0
    while o < len(old_bytes) and n < len(new_bytes):
        if old_bytes[o] == new_bytes[n]:
            o += 1
            n += 1
            continue
        # Mismatch. Search shifts in increasing total magnitude.
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
                    # Near end-of-item: not enough downstream bytes for a
                    # CONFIRM-window. Accept only if the *entire* remaining
                    # tails match exactly (strict terminal realignment — this
                    # is what lets the walk get past a +4 field inserted a
                    # few bytes from the item end, instead of aborting).
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
    print(f"1.11: {len(old_bytes_full):,}B   1.12: {len(new_bytes_full):,}B")

    tracked = crimson_rs.parse_iteminfo_tracked(old_bytes_full)
    old_items, old_spans = tracked["items"], tracked["spans"]
    print(f"1.11 tracked: {len(old_items)} items")

    old_by_key: dict[int, tuple[int, dict]] = {}
    for idx, it in enumerate(old_items):
        old_by_key[it["key"]] = (idx, it)

    new_keys = [int(l.strip()) for l in (REPO / "data" / "keys.txt").read_text().split() if l.strip()]
    print(f"1.12 keys.txt: {len(new_keys)} keys")
    new_anchors = find_anchors(new_bytes_full, new_keys)
    resolved = sum(1 for a in new_anchors if a is not None)
    print(f"1.12 anchors resolved: {resolved}/{len(new_keys)}")

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
        rel_ranges = [
            {"start": r["start"] - old_lo, "end": r["end"] - old_lo, "path": r["path"], "ty": r["ty"]}
            for r in sp["ranges"]
        ]
        events, done = tandem_walk(old_chunk, new_chunk, rel_ranges)
        per_item_events[k] = events
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

    zero_event_items = [k for k, evs in per_item_events.items() if not evs]

    report_path = OUT_DIR / "report.txt"
    with report_path.open("w", encoding="utf-8") as fh:
        fh.write(
            f"1.11 -> 1.12 iteminfo drift report\n"
            f"{'=' * 60}\n"
            f"1.11 bytes: {len(old_bytes_full):,}  items: {len(old_items)}\n"
            f"1.12 bytes: {len(new_bytes_full):,}  keys: {len(new_keys)}  anchors_resolved: {resolved}\n"
            f"common items walked: {len(common)}\n"
            f"items fully realigned: {fully_realigned}\n"
            f"items aborted mid-walk: {aborted_count}\n"
            f"items with zero drift events: {len(zero_event_items)}\n"
            f"\n--- Top drift signatures (by frequency) ---\n"
        )
        for sig, cnt in signature_count.most_common(40):
            fh.write(f"\n[{cnt:>5}x] {sig}\n")
            for ex_key, ex_field, ex_off in signature_examples[sig][:3]:
                fh.write(f"        e.g. key={ex_key}  rel_off={ex_off}  field={ex_field}\n")

        fh.write(f"\n--- Distinct signature count: {len(signature_count)} ---\n")

        fh.write("\n--- Top drift offsets in the OLD item (rel) ---\n")
        for off, cnt in offset_counter.most_common(20):
            fh.write(f"  off={off}  ({cnt}x)\n")

        fh.write("\n--- Distribution: drift distance from item END (rel) ---\n")
        for dist, cnt in drift_distance_from_end.most_common(20):
            fh.write(f"  end-off={dist}  ({cnt}x)\n")

        fh.write(f"\n--- Aborted items (first 25) ---\n")
        aborted_keys = [k for k, evs in per_item_events.items() if evs and evs[-1].get("ABORTED")]
        for k in aborted_keys[:25]:
            evs = per_item_events[k]
            fh.write(f"\nkey={k}  events={len(evs)}\n")
            for e in evs[-5:]:
                fh.write(f"  off=({e['old_off']},{e['new_off']})  field={e['field']}\n")
                fh.write(f"    rm  =0x{e['removed_hex']}\n")
                fh.write(f"    ins =0x{e['inserted_hex']}\n")
                if e.get("ABORTED"):
                    fh.write(f"    ABORTED\n")
        fh.write(f"\n--- Total aborted: {len(aborted_keys)} ---\n")

    print(f"Wrote {report_path}")
    print(f"Top 8 signatures:")
    for sig, cnt in signature_count.most_common(8):
        try:
            print(f"  [{cnt:>5}x] {sig}")
        except UnicodeEncodeError:
            print(f"  [{cnt:>5}x] {sig.encode('ascii', 'replace').decode('ascii')}")


if __name__ == "__main__":
    main()
