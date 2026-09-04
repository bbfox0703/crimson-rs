"""Extract the decompressed gamedata "bin" directory for one game version.

Mirrors `<game>/0008/`'s static-info table directory (iteminfo + skill +
stringinfo + every gamedata bridge table crimson-rs parses) into a flat
per-version folder — the portable cross-version RE archive described in
`<archive>/README.txt`. PALOC localization is intentionally NOT included
(content, ~242 MB/ver, not a cross-version RE target).

This is the per-patch companion to `dump_gamedata_keys.py`: the keys dumper
snapshots the integer key lists (safe to commit), this snapshots the full
decompressed binaries (gitignored game content — they live in the portable
archive, never the repo).

Usage:
    python scripts\\extract_gamedata_bin.py --version 1.12 \\
        --out "X:\\Crimson Desert\\gamedata-bin\\1.12"
    # --out defaults to <archive-from-env>/<version> if CRIMSON_GAMEDATA_BIN is set,
    # else must be given explicitly.
"""

from __future__ import annotations

import argparse
import os
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

BIN_GROUP = "0008"


def find_game_dir(explicit: str | None) -> str:
    if explicit:
        if not Path(explicit).is_dir():
            sys.exit(f"--game-dir not found: {explicit}")
        return explicit
    for c in GAME_DIR_CANDIDATES:
        if Path(c).is_dir():
            return c
    sys.exit("Game install not found. Pass --game-dir.")


def list_bin_files(game_dir: str, bin_dir: str) -> list[tuple[str, str]]:
    """Enumerate the static-info table files: `bin_dir` itself plus any
    subdirectory of it (2.01 moved the `*misc` blob into `bin/misc`).
    Returns `(archive_dir, filename)` pairs."""
    pamt_path = Path(game_dir) / BIN_GROUP / "0.pamt"
    if not pamt_path.is_file():
        sys.exit(f"missing {pamt_path}")
    pamt = crimson_rs.parse_pamt_bytes(pamt_path.read_bytes())
    found: list[tuple[str, str]] = []
    for d in pamt["directories"]:
        dpath = (d.get("path") or d.get("name") or "").replace("\\", "/")
        if dpath != bin_dir and not dpath.startswith(bin_dir + "/"):
            continue
        for f in d.get("files", []):
            found.append((dpath, f["name"]))
    return sorted(set(found))


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--game-dir", help="Crimson Desert install path")
    ap.add_argument("--version", required=True, help="version label (e.g. 1.12)")
    ap.add_argument(
        "--out",
        help="output directory (default: $CRIMSON_GAMEDATA_BIN/<version> if set)",
    )
    args = ap.parse_args()

    game = find_game_dir(args.game_dir)
    if args.out:
        out_dir = Path(args.out)
    elif os.environ.get("CRIMSON_GAMEDATA_BIN"):
        out_dir = Path(os.environ["CRIMSON_GAMEDATA_BIN"]) / args.version
    else:
        sys.exit("Pass --out or set CRIMSON_GAMEDATA_BIN.")
    out_dir.mkdir(parents=True, exist_ok=True)

    print(f"Game dir : {game}")
    print(f"Out dir  : {out_dir}")
    print("-" * 70)

    layout = resolve_bin_layout(game)
    names = list_bin_files(game, layout.dir)
    print(f"{len(names)} files in {BIN_GROUP}/{layout.dir}")

    total = 0
    written = 0
    failed: list[str] = []
    for adir, name in names:
        try:
            raw = bytes(crimson_rs.extract_file(game, BIN_GROUP, adir, name))
        except Exception as exc:  # noqa: BLE001
            print(f"  FAIL {name}: {exc}")
            failed.append(name)
            continue
        (out_dir / name).write_bytes(raw)
        total += len(raw)
        written += 1

    print("-" * 70)
    print(f"Wrote {written}/{len(names)} files, {total:,} B total -> {out_dir}")
    if failed:
        print(f"FAILED ({len(failed)}): {', '.join(failed)}")
        sys.exit(1)


if __name__ == "__main__":
    main()
