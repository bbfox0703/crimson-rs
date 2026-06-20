"""Cross-version drift RE: 1.11 -> 1.12 partprefabdyeslotinfo schema.

Phase 1 found: per slot, 1.12 inserts 5 bytes (u8=0xff + u32=0) between
mask[3] and the tail CString. Confirmed on 968/968 records.

This phase answers the design question for the fix: is a per-row
dual-schema attempt SAFE? i.e. do the OLD (1.11) and NEW (1.12) per-slot
layouts ever cross-match (walk clean to body.len() under the wrong
schema)? If cross-match ~= 0, dual-schema is unambiguous and we can keep
1.07-1.11 support while adding 1.12. It also dumps fresh test pins from
the live 1.12 data.

Usage:
    python scripts/diff_dyeslot_111_112.py
"""

from __future__ import annotations

import os
import struct
from collections import Counter
from pathlib import Path

# Portable per-version archive root (drive letter irrelevant — copy the
# `gamedata-bin/<ver>/` folder anywhere and point this at it). Override
# with CRIMSON_GAMEDATA_BIN; defaults to the local X: archive.
ARCHIVE = Path(os.environ.get("CRIMSON_GAMEDATA_BIN", r"X:\Crimson Desert\gamedata-bin"))


def files(ver):
    base = ARCHIVE / ver / "partprefabdyeslotinfo"
    return base.with_suffix(".pabgb").read_bytes(), base.with_suffix(".pabgh").read_bytes()


def parse_pabgh(data):
    (count,) = struct.unpack_from("<H", data, 0)
    out, off = [], 2
    for _ in range(count):
        key, offset = struct.unpack_from("<II", data, off)
        out.append((key, offset))
        off += 8
    return out


def entry_ranges(index, pabgb_len):
    order = sorted(range(len(index)), key=lambda i: index[i][1])
    end_at = [pabgb_len] * len(index)
    for a, b in zip(order, order[1:]):
        end_at[a] = index[b][1]
    return [(index[i][1], end_at[i]) for i in range(len(index))]


def read_cstring(body, cur):
    if cur + 4 > len(body):
        return None, cur
    (ln,) = struct.unpack_from("<I", body, cur)
    cur += 4
    if cur + ln > len(body):
        return None, cur
    try:
        s = body[cur : cur + ln].decode("utf-8")
    except UnicodeDecodeError:
        return None, cur
    return s, cur + ln


def parse_header(body):
    if len(body) < 17:
        return None
    (key,) = struct.unpack_from("<I", body, 0)
    (slot_count,) = struct.unpack_from("<I", body, 9)
    name, cur = read_cstring(body, 13)
    if name is None or not (1 <= len(name.encode()) <= 128):
        return None
    return key, slot_count, name, cur


def walk_row(body, *, new_schema: bool):
    """Walk a full row. new_schema=True consumes the 1.12 per-slot 5-byte
    insert (u8 + u32) after mask. Returns (ok, end_cur, header, slots)."""
    hdr = parse_header(body)
    if not hdr:
        return False, 0, None, None
    key, slot_count, name, cur = hdr
    if slot_count == 0 or slot_count > 64:
        return False, cur, hdr, None
    slots = []
    for _ in range(slot_count):
        if cur + 3 > len(body):
            return False, cur, hdr, slots
        mat = bytes(body[cur : cur + 3]); cur += 3
        a, cur = read_cstring(body, cur)
        if a is None: return False, cur, hdr, slots
        b, cur = read_cstring(body, cur)
        if b is None: return False, cur, hdr, slots
        c, cur = read_cstring(body, cur)
        if c is None: return False, cur, hdr, slots
        if cur + 3 > len(body):
            return False, cur, hdr, slots
        mask = bytes(body[cur : cur + 3]); cur += 3
        nu8 = nu32 = None
        if new_schema:
            if cur + 5 > len(body):
                return False, cur, hdr, slots
            nu8 = body[cur]
            (nu32,) = struct.unpack_from("<I", body, cur + 1)
            cur += 5
        tail, cur = read_cstring(body, cur)
        if tail is None: return False, cur, hdr, slots
        slots.append((mat, [a, b, c], mask, nu8, nu32, tail))
    return (cur == len(body)), cur, hdr, slots


def load(ver):
    b, h = files(ver)
    idx = parse_pabgh(h)
    ranges = entry_ranges(idx, len(b))
    rows = []
    for i in range(len(idx)):
        key, _ = idx[i]
        s, e = ranges[i]
        rows.append((key, b[s:e]))
    return rows


def main():
    old_rows = load("1.11")
    new_rows = load("1.12")
    print(f"1.11 rows: {len(old_rows)}   1.12 rows: {len(new_rows)}")

    # Cross-match matrix: for each (data, schema) combo, how many walk clean?
    for label, rows in [("1.11", old_rows), ("1.12", new_rows)]:
        clean_old = sum(1 for _k, b in rows if walk_row(b, new_schema=False)[0])
        clean_new = sum(1 for _k, b in rows if walk_row(b, new_schema=True)[0])
        print(f"  {label} data: OLD-schema clean={clean_old}/{len(rows)}  "
              f"NEW-schema clean={clean_new}/{len(rows)}")

    # Dual-schema (try NEW then OLD): how many rows resolve per version?
    def dual(rows):
        n = 0
        for _k, b in rows:
            if walk_row(b, new_schema=True)[0] or walk_row(b, new_schema=False)[0]:
                n += 1
        return n
    print(f"\nDual-schema (NEW-then-OLD) resolves: "
          f"1.11={dual(old_rows)}/{len(old_rows)}  1.12={dual(new_rows)}/{len(new_rows)}")

    # Ambiguity: rows where BOTH schemas walk clean (would be mis-readable).
    for label, rows in [("1.11", old_rows), ("1.12", new_rows)]:
        both = sum(1 for _k, b in rows
                   if walk_row(b, new_schema=False)[0] and walk_row(b, new_schema=True)[0])
        print(f"  {label}: rows clean under BOTH schemas (ambiguous): {both}")

    # ---- Fresh test pins from live 1.12 ----
    print("\n=== Fresh 1.12 pins (key, name, slot_count, last-slot tail) ===")
    by_name = {}
    pins = []
    for key, b in new_rows:
        ok, cur, hdr, slots = walk_row(b, new_schema=True)
        if not ok:
            continue
        _key, slot_count, name, _ = hdr
        by_name[name] = (key, slot_count)
        pins.append((key, name, slot_count, slots[-1][5] if slots else ""))

    # Pick a stable spread: a 1-slot lb, a multi-slot, and any with
    # cloth/leather/metal materials.
    want_names = [
        "cd_phm_00_lb_00_0054",
        "cd_phm_00_hel_0057_02_inside",
        "cd_phm_00_cloak_0054_hair_spline",
        "cd_phw_00_vest_belt_0137_00",
        "cd_phw_00_vest_acc_0043_01",
        "cd_phm_00_vest_0051_01",
    ]
    print("-- status of the existing (1.07-era) KNOWN pins in 1.12 --")
    for nm in want_names:
        if nm in by_name:
            k, sc = by_name[nm]
            print(f"   PRESENT  0x{k:08x}  {nm!r}  slot_count={sc}")
        else:
            print(f"   MISSING  {nm!r}")

    # A few concrete pins: smallest slot_count, a 4-slot, and the max.
    print("\n-- candidate fresh pins --")
    pins.sort(key=lambda p: p[2])
    by_sc = {}
    for k, nm, sc, tail in pins:
        by_sc.setdefault(sc, []).append((k, nm, tail))
    for sc in sorted(by_sc)[:6] + sorted(by_sc)[-2:]:
        k, nm, tail = by_sc[sc][0]
        print(f"   slot_count={sc:<2} 0x{k:08x} {nm!r} last_tail={tail!r}")

    # A row with a recognizable material — find one whose slots include
    # cloth/leather/metal so the c_abi material test still has a target.
    print("\n-- a row exposing cloth/leather/metal materials --")
    for key, b in new_rows:
        ok, cur, hdr, slots = walk_row(b, new_schema=True)
        if not ok:
            continue
        mats = [m for sl in slots for m in sl[1]]
        if any(m in ("cloth", "leather", "metal") for m in mats) and hdr[1] >= 4:
            print(f"   0x{key:08x} {hdr[2]!r} slot_count={hdr[1]} "
                  f"materials={[m for m in mats if m]}")
            print(f"      last-slot tail={slots[-1][5]!r}")
            break

    # Slot-count distribution sanity (test asserts >1/4 have slot_count==1).
    sc_hist = Counter(p[2] for p in pins)
    n1 = sc_hist[1]
    print(f"\nslot_count distribution: total={len(pins)} sc==1: {n1} "
          f"({100*n1//max(1,len(pins))}%)  max={max(sc_hist)}")


if __name__ == "__main__":
    main()
