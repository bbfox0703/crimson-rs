"""Scan ALL groups (not just first hit) for localizationstring_*.paloc
files. Crimson Desert ships patch data in higher group numbers, so a
newer 1.05 paloc might live in a later group than the base one we hit
first."""

from __future__ import annotations
import argparse
import json
from pathlib import Path

import crimson_rs


PALOC_DIR = "gamedata/stringtable/binary__"


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--game-dir", required=True)
    ap.add_argument("--unknown", required=True)
    ap.add_argument("--lang", default="eng")
    ap.add_argument("--scan-range", default="20:36")
    args = ap.parse_args()

    lo, hi = (int(x) for x in args.scan_range.split(":"))
    groups = [f"{n:04d}" for n in range(lo, hi)]

    fname = f"localizationstring_{args.lang}.paloc"
    print(f"scanning groups {lo}..{hi-1} for {fname}\n")

    unknown = json.loads(Path(args.unknown).read_text(encoding="utf-8"))
    target_keys = {r["key"]: r["string_key"] for r in unknown}

    hits_per_group: dict[str, dict] = {}
    for g in groups:
        try:
            raw = bytes(crimson_rs.extract_file(args.game_dir, g, PALOC_DIR, fname))
        except Exception:
            continue
        entries = crimson_rs.parse_paloc_bytes(raw)
        # how many of the 71 unknown keys have a 0x70 entry here?
        found_70 = {}
        for e in entries:
            try:
                sid = int(e["string_key"])
            except (ValueError, TypeError):
                continue
            ik = sid >> 32
            if ik in target_keys and (sid & 0xFF) == 0x70:
                found_70[ik] = e["string_value"]
        hits_per_group[g] = {
            "size": len(raw),
            "entries": len(entries),
            "found_0x70_for_unknowns": len(found_70),
            "samples": list(found_70.items())[:5],
        }

    if not hits_per_group:
        print(f"no group contains {fname}")
        return

    for g, info in hits_per_group.items():
        print(
            f"  group {g}: {info['entries']:>7,} entries  "
            f"({info['size']:,}B)  unknowns_with_0x70={info['found_0x70_for_unknowns']}"
        )
        for k, v in info["samples"]:
            print(f"    {k} -> {v!r}  (was {target_keys[k]!r})")


if __name__ == "__main__":
    main()
