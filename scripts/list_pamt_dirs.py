"""List directories and files in a group's 0.pamt — useful for finding
where alternate localization data lives."""

from __future__ import annotations
import argparse
import sys
from pathlib import Path

import crimson_rs


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--game-dir", required=True)
    ap.add_argument("--group", required=True, help="e.g. 0020")
    ap.add_argument("--filter", default="", help="only show paths containing this substring")
    args = ap.parse_args()

    sys.stdout.reconfigure(encoding="utf-8")  # type: ignore[attr-defined]

    pamt_path = Path(args.game_dir) / args.group / "0.pamt"
    pamt = crimson_rs.parse_pamt_bytes(pamt_path.read_bytes())

    print(f"group {args.group} has {len(pamt['directories'])} directories")
    for d in pamt["directories"]:
        path = d["path"]
        files = d["files"]
        if args.filter and args.filter not in path:
            continue
        print(f"\n{path}  ({len(files)} files)")
        for f in files:
            print(f"  {f['name']:<60} {f['uncompressed_size']:>10}B")


if __name__ == "__main__":
    main()
