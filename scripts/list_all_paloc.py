"""List every .paloc file across all groups, with size."""
from __future__ import annotations
import argparse
import sys
from pathlib import Path

import crimson_rs


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--game-dir", required=True)
    ap.add_argument("--scan-range", default="0:100")
    args = ap.parse_args()
    sys.stdout.reconfigure(encoding="utf-8")  # type: ignore[attr-defined]

    lo, hi = (int(x) for x in args.scan_range.split(":"))
    seen: dict[str, list[tuple[str, int]]] = {}
    for n in range(lo, hi):
        g = f"{n:04d}"
        pamt_path = Path(args.game_dir) / g / "0.pamt"
        if not pamt_path.is_file():
            continue
        try:
            pamt = crimson_rs.parse_pamt_bytes(pamt_path.read_bytes())
        except Exception:
            continue
        for d in pamt["directories"]:
            for f in d["files"]:
                if not f["name"].endswith(".paloc"):
                    continue
                full = f"{d['path']}/{f['name']}"
                seen.setdefault(full, []).append((g, f["uncompressed_size"]))

    for full, hits in sorted(seen.items()):
        print(f"{full}")
        for g, sz in hits:
            print(f"  group {g}  {sz:>12,}B")


if __name__ == "__main__":
    main()
