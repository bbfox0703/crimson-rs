"""Dump the 1.12 iteminfo field-offset reference (requires the UN-edited,
1.12-capable parser — run after `git stash` of the 1.13 WIP edits).

Writes out/ref_112.json: {key: {"size": N, "ranges": [[path,start,end,ty],...]}}
with offsets relative to each item's start. Used as the 1.12 layout reference
during the 1.13 RE once the in-tree parser has been moved to 1.13.
"""
from __future__ import annotations
import json, sys
from pathlib import Path
REPO = Path('.').resolve(); sys.path.insert(0, str(REPO)); import crimson_rs

OLD = Path(r"X:\Crimson Desert\gamedata-bin\1.12\iteminfo.pabgb")
raw = OLD.read_bytes()
tr = crimson_rs.parse_iteminfo_tracked(raw)
items, spans = tr["items"], tr["spans"]
print(f"1.12 parsed items: {len(items)}")
ref = {}
for it, sp in zip(items, spans):
    base = sp["start"]
    ref[it["key"]] = {
        "size": sp["end"] - base,
        "ranges": [[r["path"], r["start"] - base, r["end"] - base, r["ty"]] for r in sp["ranges"]],
    }
out = REPO / "out" / "ref_112.json"
out.write_text(json.dumps(ref), encoding="utf-8")
print(f"wrote {out} ({out.stat().st_size:,}B) for {len(ref)} items")
