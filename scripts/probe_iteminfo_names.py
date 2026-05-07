"""For the 71 'unknown' dev/test items, look at their iteminfo.pabgb
(parsed in 1.04 — same item structure for these keys) to see if the
binary itself carries an embedded display name in `item_name.default`."""
from __future__ import annotations
import argparse
import json
import sys
from pathlib import Path


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--unknown", required=True)
    ap.add_argument("--baseline", required=True)
    args = ap.parse_args()
    sys.stdout.reconfigure(encoding="utf-8")  # type: ignore[attr-defined]

    unknown = json.loads(Path(args.unknown).read_text(encoding="utf-8"))
    target = {r["key"] for r in unknown}

    hits = []
    with Path(args.baseline).open(encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            it = json.loads(line)
            if it["key"] in target:
                hits.append(it)

    print(f"found {len(hits)} of {len(target)} dev items in baseline")
    print()
    nonempty = 0
    for h in hits:
        nm = h.get("item_name") or {}
        nm_default = nm.get("default", "")
        nm_index = nm.get("index", 0)
        if nm_default and not nm_default.isdigit():
            nonempty += 1
        sk = h["string_key"]
        # show first 25 with detail
        if hits.index(h) < 25:
            print(f"  key={h['key']:<10} string_key={sk!r}")
            print(f"      item_name.default = {nm_default!r}")
            print(f"      item_name.index   = {nm_index}")

    print(f"\ndev items where item_name.default is non-numeric (real text): {nonempty}/{len(hits)}")


if __name__ == "__main__":
    main()
