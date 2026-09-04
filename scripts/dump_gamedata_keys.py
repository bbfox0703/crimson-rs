"""Per-table key-list dumper — the cross-version anchor snapshot tool.

For each PABGH-indexed gamedata table the c_abi bridges consume, parses
the live install's .pabgh and writes a one-decimal-key-per-line file to
`data/gamedata-keys-<version>/<table>.txt`. The two anchor-scan tables
(`questinfo`, `stageinfo`) — which have no usable PABGH — fall back to a
[u32 key][u32 slen][slen identifier-bytes][u8 zero] scan of their PABGB,
matching what `src/quest_info/` and `src/stage_info/` do.

These dumps play the same cross-version-diff role as `data/keys.txt`
plays for iteminfo: when the next patch ships, diff the new dump vs the
previous version's snapshot to see exactly which keys were added /
removed. Only integers are written — no gamedata-derived strings — so
the dumps are safe to commit (matching the existing keys-1.0X.txt
policy in `data/CLAUDE.md`).

Usage:
    python scripts\\dump_gamedata_keys.py [--game-dir PATH]
                                          [--version 1.08]
                                          [--out data\\gamedata-keys-1.08]
"""

from __future__ import annotations

import argparse
import struct
import sys
from pathlib import Path

import crimson_rs

from gamedata_layout import resolve_bin_layout


GAME_DIR_CANDIDATES = [
    r"D:\SteamLibrary\steamapps\common\Crimson Desert",
    r"F:\SteamLibrary\steamapps\common\Crimson Desert",
    r"E:\SteamLibrary\steamapps\common\Crimson Desert",
    r"C:\Program Files (x86)\Steam\steamapps\common\Crimson Desert",
]


# (table_label, pabgh_filename, pabgb_filename | None for pabgh-only,
#  key_size_bytes | None for auto-detect)
#
# Tables marked pabgb-fallback (pabgh_filename=None) use anchor-scan over
# the .pabgb instead — same pattern iteminfo uses through `keys.txt`.
PABGH_TABLES = [
    # (label,                              pabgh,                                pabgb,                                key_size_bytes_hint)
    ("skill",                               "skill.pabgh",                        "skill.pabgb",                        None),
    ("missioninfo",                         "missioninfo.pabgh",                  "missioninfo.pabgb",                  None),
    ("knowledgeinfo",                       "knowledgeinfo.pabgh",                "knowledgeinfo.pabgb",                None),
    ("questgaugeinfo",                      "questgaugeinfo.pabgh",               "questgaugeinfo.pabgb",               None),
    ("sublevelinfo",                        "sublevelinfo.pabgh",                 "sublevelinfo.pabgb",                 None),
    ("gimmickinfo",                         "gimmickinfo.pabgh",                  "gimmickinfo.pabgb",                  None),
    ("characterinfo",                       "characterinfo.pabgh",                "characterinfo.pabgb",                None),
    ("dyecolorgroupinfo",                   "dyecolorgroupinfo.pabgh",            "dyecolorgroupinfo.pabgb",            None),
    ("partprefabdyetexturepalleteinfo",     "partprefabdyetexturepalleteinfo.pabgh", "partprefabdyetexturepalleteinfo.pabgb", None),
    ("partprefabdyeslotinfo",               "partprefabdyeslotinfo.pabgh",        "partprefabdyeslotinfo.pabgb",        None),
    ("factionnode",                         "factionnode.pabgh",                  "factionnode.pabgb",                  None),
    ("factionspawndatainfo",                "factionspawndatainfo.pabgh",         "factionspawndatainfo.pabgb",         None),
    ("factionrelationgroup",                "factionrelationgroup.pabgh",         "factionrelationgroup.pabgb",         None),
    ("storeinfo",                           "storeinfo.pabgh",                    "storeinfo.pabgb",                    None),
    ("mercenaryinfo",                       "mercenaryinfo.pabgh",                "mercenaryinfo.pabgb",                None),
    ("houseinfo",                           "houseinfo.pabgh",                    "houseinfo.pabgb",                    None),
    ("royalsupply",                         "royalsupply.pabgh",                  "royalsupply.pabgb",                  None),
    ("crafttoolinfo",                       "crafttoolinfo.pabgh",                "crafttoolinfo.pabgb",                None),
    ("crafttoolgroupinfo",                  "crafttoolgroupinfo.pabgh",           "crafttoolgroupinfo.pabgb",           None),
    ("triggerregioninfo",                   "triggerregioninfo.pabgh",            "triggerregioninfo.pabgb",            None),
    ("gameplayvariableinfo",                "gameplayvariableinfo.pabgh",         "gameplayvariableinfo.pabgb",         None),
    ("globalgameevent",                     "globalgameevent.pabgh",              "globalgameevent.pabgb",              None),
    ("globalgameeventgroup",                "globalgameeventgroup.pabgh",         "globalgameeventgroup.pabgb",         None),
    ("gameadviceinfo",                      "gameadviceinfo.pabgh",               "gameadviceinfo.pabgb",               None),
    ("gameadvicegroupinfo",                 "gameadvicegroupinfo.pabgh",          "gameadvicegroupinfo.pabgb",          None),
    ("reserveslot",                         "reserveslot.pabgh",                  "reserveslot.pabgb",                  None),
    ("regioninfo",                          "regioninfo.pabgh",                   "regioninfo.pabgb",                   None),
    ("itemgroupinfo",                       "itemgroupinfo.pabgh",                "itemgroupinfo.pabgb",                None),
]

# Tables with PABGH backing (questinfo / stageinfo use the u32-count
# variant, picked up by parse_pabgh_keys above). The c_abi bridges for
# these still anchor-scan PABGB at runtime — but for the cross-version
# snapshot we just want the in-game key list, which the PABGH delivers
# without the bridge's body-validation extras.
PABGH_TABLES_U32_COUNT = [
    ("questinfo",  "questinfo.pabgh",  "questinfo.pabgb"),
    ("stageinfo",  "stageinfo.pabgh",  "stageinfo.pabgb"),
]

PABGB_GROUP = "0008"

# The tables above name files the pre-2.01 way; 2.01 renamed the directory
# and the extensions without touching a byte inside. `resolve_bin_layout`
# says which naming the install on disk uses, and `_resolved` maps a
# table entry onto it.


def find_game_dir(explicit: str | None) -> str:
    if explicit:
        if not Path(explicit).is_dir():
            sys.exit(f"--game-dir not found: {explicit}")
        return explicit
    for c in GAME_DIR_CANDIDATES:
        if Path(c).is_dir():
            return c
    sys.exit("Game install not found. Pass --game-dir.")


def parse_pabgh_keys(pabgh: bytes) -> list[int] | None:
    """Auto-detect PABGH shape and return keys in appearance order
    (= in-game iteration order). Two count widths and three key widths
    are supported:

      - u16 count + (u32 key, u32 offset)*   — most niche tables
      - u16 count + (u16 key, u32 offset)*   — storeinfo, regioninfo, …
      - u16 count + (u8 key, u32 offset)*    — mercenaryinfo (rare)
      - u32 count + (u32 key, u32 offset)*   — questinfo, stageinfo

    Returns None if no shape fits exactly. Empty tables (count = 0) are
    fine: the function returns an empty list."""
    if len(pabgh) < 2:
        return None
    # u32-count variant — only the questinfo / stageinfo shape so far.
    if len(pabgh) >= 4:
        count32 = struct.unpack_from("<I", pabgh, 0)[0]
        if 4 + 8 * count32 == len(pabgh):
            return [
                struct.unpack_from("<I", pabgh, 4 + i * 8)[0]
                for i in range(count32)
            ]
    # u16-count variants
    count = struct.unpack_from("<H", pabgh, 0)[0]
    for _, row_sz, unpack_fmt in [(4, 8, "<I"), (2, 6, "<H"), (1, 5, "<B")]:
        if 2 + row_sz * count == len(pabgh):
            return [
                struct.unpack_from(unpack_fmt, pabgh, 2 + i * row_sz)[0]
                for i in range(count)
            ]
    return None


def write_keys(out_path: Path, keys: list[int]) -> None:
    out_path.parent.mkdir(parents=True, exist_ok=True)
    with out_path.open("w", encoding="utf-8", newline="\n") as fh:
        for k in keys:
            fh.write(f"{k}\n")


def extract_safe(game: str, group: str, d: str, name: str) -> bytes | None:
    try:
        return bytes(crimson_rs.extract_file(game, group, d, name))
    except Exception:
        return None


def _resolved(layout, name: str) -> str:
    """`"skill.pabgh"` -> the header filename this install actually ships."""
    stem, _, ext = name.rpartition(".")
    return layout.header(stem) if ext == "pabgh" else layout.body(stem)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--game-dir", help="Crimson Desert install path")
    ap.add_argument("--version", default="1.08", help="version label for the output dir (default: 1.08)")
    ap.add_argument("--out", help="output directory (default: <repo>/data/gamedata-keys-<version>)")
    args = ap.parse_args()

    game = find_game_dir(args.game_dir)
    repo_root = Path(__file__).resolve().parent.parent
    out_dir = Path(args.out).resolve() if args.out else (
        repo_root / "data" / f"gamedata-keys-{args.version}"
    )
    out_dir.mkdir(parents=True, exist_ok=True)
    print(f"Game dir : {game}")
    print(f"Out dir  : {out_dir}")
    print("-" * 80)

    layout = resolve_bin_layout(game)
    summary: list[tuple[str, int | None, str]] = []

    for label, pabgh_name, pabgb_name, _ in PABGH_TABLES:
        pabgh = extract_safe(game, PABGB_GROUP, layout.dir, _resolved(layout, pabgh_name))
        if pabgh is None:
            print(f"  {label:<35} <pabgh missing>")
            summary.append((label, None, "missing"))
            continue
        keys = parse_pabgh_keys(pabgh)
        if keys is None:
            # New patch with a brand-new PABGH layout? Extend
            # parse_pabgh_keys above to add the new shape — silently
            # falling back to pabgb anchor-scan inflates counts via false
            # positives (the bridges' per-table validation rules are
            # what makes anchor-scan precise) and that asymmetry has
            # surprised us before.
            print(f"  {label:<35} <pabgh shape unknown — extend parse_pabgh_keys>")
            summary.append((label, None, "shape unknown"))
            continue
        out_path = out_dir / f"{label}.txt"
        write_keys(out_path, keys)
        print(f"  {label:<35} {len(keys):>6} keys  (pabgh) -> {out_path.name}")
        summary.append((label, len(keys), "pabgh"))

    for label, pabgh_name, pabgb_name in PABGH_TABLES_U32_COUNT:
        pabgh = extract_safe(game, PABGB_GROUP, layout.dir, _resolved(layout, pabgh_name))
        if pabgh is None:
            print(f"  {label:<35} <pabgh missing>")
            summary.append((label, None, "missing"))
            continue
        keys = parse_pabgh_keys(pabgh)
        if keys is None:
            print(f"  {label:<35} <pabgh shape unknown>")
            summary.append((label, None, "shape unknown"))
            continue
        out_path = out_dir / f"{label}.txt"
        write_keys(out_path, keys)
        print(f"  {label:<35} {len(keys):>6} keys  (pabgh u32cnt) -> {out_path.name}")
        summary.append((label, len(keys), "pabgh u32cnt"))

    print("-" * 80)
    total = sum(c or 0 for _, c, _ in summary)
    print(f"Done. {len(summary)} tables, {total:,} total keys written to: {out_dir}")


if __name__ == "__main__":
    main()
