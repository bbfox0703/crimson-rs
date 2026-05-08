"""Cross-version skill format drift probe.

When a new game patch ships, run this against the new version's archive
alongside earlier baselines to see how the skill binary format moved
(format flag, BuffData tail sizes, entry count). Used to drive the
item-2 strategy decision in this repo's history (see
`docs/downstream-api-gaps.md` table); promoted here as a template for
the next time a Crimson Desert patch needs the skill parser
re-validated.

Drives three questions:

- Does roundtrip pass on the new version with the existing parser?
- Did the format flag (`_has_field_58`) flip again?
- Did the per-`type_id` BuffData tail-size table drift relative to
  earlier versions? (If yes, the brute-force probe is still essential
  and any "hardcode the sizes" optimisation idea remains a bad one.)

Usage
-----

This script depends on the GameMods Python `skillinfo_parser` as the
ground-truth parser to compare *our* Rust parser against. Set its path
via `GAMEMODS_DIR` (default below). Provide the cross-version archives
via `BASELINES_ROOT` (each subdirectory holding a `0008/0.paz` +
`0008/0.pamt` for the corresponding version). Edit `VERSIONS` to add
the new patch.

    set GAMEMODS_DIR=D:\path\to\CrimsonGameMods
    set BASELINES_ROOT=G:\path\to\Crimson Desert
    python scripts\archive\probe_skill_versions.py

The first run on a new version is the interesting one — the size-drift
diff at the bottom tells you which BuffData subclass tails moved.
"""

from __future__ import annotations

import importlib
import os
import sys

# Import the in-tree (editable-installed) crimson_rs first so it wins
# over the bundled crimson_rs/ inside the GameMods tree (which we add
# to sys.path next only to pick up skillinfo_parser).
import crimson_rs

GAMEMODS_DIR = os.environ.get(
    "GAMEMODS_DIR",
    r"D:\Github\CRIMSON-DESERT-SAVE-EDITOR-AND-GAME-MODS\CrimsonGameMods",
)
BASELINES_ROOT = os.environ.get(
    "BASELINES_ROOT",
    r"G:\我的雲端硬碟\temp\Crimson Desert",
)

sys.path.append(GAMEMODS_DIR)

# Add a new entry here when a new patch lands. Each must be the name of
# a sub-directory under BASELINES_ROOT containing 0008/0.paz + 0.pamt.
VERSIONS = ["1.03.01", "1.04.01", "1.05.01"]

INTERNAL = "gamedata/binary__/client/bin"


def probe_one(version: str):
    print(f"\n=== {version} ===")
    paz_path = os.path.join(BASELINES_ROOT, version, "0008", "0.paz")
    if not os.path.isfile(paz_path):
        print(f"  skip: no archive at {paz_path}")
        return None
    pabgh = bytes(crimson_rs.extract_file_from_paz(paz_path, f"{INTERNAL}/skill.pabgh"))
    pabgb = bytes(crimson_rs.extract_file_from_paz(paz_path, f"{INTERNAL}/skill.pabgb"))
    print(f"pabgh={len(pabgh):,}B  pabgb={len(pabgb):,}B")

    # Reload skillinfo_parser to get a fresh cache + format flag each run.
    if "skillinfo_parser" in sys.modules:
        del sys.modules["skillinfo_parser"]
    sip = importlib.import_module("skillinfo_parser")

    idx = sip.parse_skill_pabgh(pabgh)
    print(f"index entries: {len(idx)}")
    rt_ok_python = sip.roundtrip_test(pabgh, pabgb)
    print(f"_has_field_58 (python detect): {sip._has_field_58}")
    print(f"discovered type_id sizes: {len(sip._type_id_sizes)}")

    # Cross-check our Rust parser's roundtrip on the same bytes.
    parsed = crimson_rs.parse_skillinfo_from_bytes(pabgb, pabgh)
    out_h, out_b = crimson_rs.serialize_skillinfo(parsed)
    rt_ok_rust = bytes(out_h) == pabgh and bytes(out_b) == pabgb
    print(f"rust parse_skillinfo + serialize roundtrip: {'OK' if rt_ok_rust else 'FAIL'}")
    print(f"rust format detection: {parsed['format']}")

    return {
        "version": version,
        "n_entries": len(idx),
        "rt_python": rt_ok_python,
        "rt_rust": rt_ok_rust,
        "has_field_58": sip._has_field_58,
        "rust_format": parsed["format"],
        "sizes": dict(sip._type_id_sizes),
    }


def main():
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    results = [r for r in (probe_one(v) for v in VERSIONS) if r is not None]
    if not results:
        print("\nno versions probed", file=sys.stderr)
        return 1

    print("\n=== Summary ===")
    print(f"  {'version':10} {'entries':>8} {'py_rt':>6} {'rust_rt':>8} "
          f"{'field_58':>9} {'rust_format':>14} {'type_ids':>9}")
    for r in results:
        print(f"  {r['version']:10} {r['n_entries']:>8} {str(r['rt_python']):>6} "
              f"{str(r['rt_rust']):>8} {str(r['has_field_58']):>9} "
              f"{r['rust_format']:>14} {len(r['sizes']):>9}")

    # Compare type_id → tail_size across the probed versions.
    all_tids = sorted(set(tid for r in results for tid in r["sizes"]))
    print(f"\nUnion of type_ids across versions: {len(all_tids)}")
    diffs = []
    for tid in all_tids:
        sizes = [r["sizes"].get(tid) for r in results]
        if len({s for s in sizes if s is not None}) > 1:
            diffs.append((tid, sizes))
    if diffs:
        print(f"\n** {len(diffs)} type_ids differ across versions: **")
        header = "  type_id  " + " ".join(f"{r['version']:>8}" for r in results)
        print(header)
        for tid, sizes in diffs:
            print(f"  {tid:5d}    "
                  + " ".join(f"{str(s):>8}" for s in sizes))
        print("\n  → drift confirmed; brute-force probing is still essential")
    else:
        print("\n** type_id sizes IDENTICAL across all probed versions **")
        print("   → could in principle hardcode, but new patches may still drift")

    return 0


if __name__ == "__main__":
    sys.exit(main())
