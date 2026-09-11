"""Extract the decompressed gamedata "bin" directory for one game version.

Mirrors `<game>/0008/`'s static-info table directory (iteminfo + skill +
stringinfo + every gamedata bridge table crimson-rs parses) into a flat
per-version folder — the portable cross-version RE archive described in
`<archive>/README.txt`.

`--paloc <lang>` (repeatable) also keeps that language's PALOC localization:
the `.paloc` files exactly as they sit in the game archive, written to
`<out>/paloc/<lang>/` with a MANIFEST.txt giving each file's size, entry
count and SHA256. The archive keeps English from 2.02 on (`--paloc eng`,
~17 MB) so a patch's text changes can be diffed against the previous one;
all 15 languages would be ~242 MB per version, so the rest are left out.

This is the per-patch companion to `dump_gamedata_keys.py`: the keys dumper
snapshots the integer key lists (safe to commit), this snapshots the full
decompressed binaries (gitignored game content — they live in the portable
archive, never the repo).

Usage:
    python scripts\\extract_gamedata_bin.py --version 2.02 --paloc eng \\
        --out "X:\\Crimson Desert\\gamedata-bin\\2.02"
    # --out defaults to <archive-from-env>/<version> if CRIMSON_GAMEDATA_BIN is set,
    # else must be given explicitly.
"""

from __future__ import annotations

import argparse
import hashlib
import os
import sys
from pathlib import Path

import crimson_rs

from gamedata_layout import PalocTarget, discover_paloc_targets, resolve_bin_layout


GAME_DIR_CANDIDATES = [
    r"D:\SteamLibrary\steamapps\common\Crimson Desert",
    r"F:\SteamLibrary\steamapps\common\Crimson Desert",
    r"E:\SteamLibrary\steamapps\common\Crimson Desert",
    r"C:\Program Files (x86)\Steam\steamapps\common\Crimson Desert",
]

BIN_GROUP = "0008"

# Localization groups that may host the paloc files — the same generous range
# export_for_ce.py scans (Korean lives in 0019, English in 0020).
PALOC_GROUPS = [f"{n:04d}" for n in range(19, 50)]


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


def read_paver(game_dir: str) -> str:
    """`meta/0.paver` as `major / minor / patch / 0xbuild`, or `unknown`."""
    try:
        b = (Path(game_dir) / "meta" / "0.paver").read_bytes()
    except OSError:
        return "unknown"
    if len(b) < 10:
        return "unknown"
    major, minor, patch = (int.from_bytes(b[i:i + 2], "little") for i in (0, 2, 4))
    return f"{major} / {minor} / {patch} / {int.from_bytes(b[6:10], 'little'):#010x}"


def resolve_paloc(game_dir: str, langs: list[str]) -> list[PalocTarget]:
    """The paloc location of every requested language. Exits before any
    extraction when one is missing, listing what the install has."""
    if not langs:
        return []
    found = {t.lang: t for t in discover_paloc_targets(game_dir, PALOC_GROUPS)}
    missing = [lang for lang in langs if lang not in found]
    if missing:
        sys.exit(
            f"--paloc {', '.join(missing)}: not in this install "
            f"(available: {', '.join(sorted(found))})"
        )
    return [found[lang] for lang in langs]


def extract_paloc(game_dir: str, version: str, t: PalocTarget, out_dir: Path) -> tuple[int, int, int]:
    """Write one language's paloc files plus MANIFEST.txt to
    `out_dir/paloc/<lang>/`. Returns `(files, bytes, entries)`."""
    dest = out_dir / "paloc" / t.lang
    dest.mkdir(parents=True, exist_ok=True)
    rows = []
    for fname in t.files:
        raw = bytes(crimson_rs.extract_file(game_dir, t.group, t.dir, fname))
        entries = len(crimson_rs.parse_paloc_bytes(raw))
        digest = hashlib.sha256(raw).hexdigest()
        (dest / fname).write_bytes(raw)
        if hashlib.sha256((dest / fname).read_bytes()).hexdigest() != digest:
            sys.exit(f"--paloc {t.lang}: {fname} did not read back identically")
        rows.append((fname, len(raw), entries, digest))

    total_bytes = sum(r[1] for r in rows)
    total_entries = sum(r[2] for r in rows)
    w = max(len(r[0]) for r in rows)
    lines = [
        f"PALOC ({t.lang}), Crimson Desert {version} (paver {read_paver(game_dir)})",
        f"Source: {t.group}/{t.dir}/ -- the {len(rows)} paloc file(s) exactly as they",
        "sit in the archive. 2.01+ splits each language into per-namespace files;",
        "concatenating their entry lists reproduces the pre-2.01 single",
        f"localizationstring_{t.lang}.paloc blob. Read with crimson_rs.parse_paloc_bytes();",
        "a string_key is either a name or the decimal (hash << 32) | namespace, where",
        "hash = hashlittle2 of the owning row's internal name (0x100 quest titles,",
        "0x101 mission and stage titles).",
        "",
        f"{'file':<{w}}  {'bytes':>10}  {'entries':>8}  sha256",
    ]
    lines += [f"{n:<{w}}  {b:>10,}  {e:>8,}  {h}" for n, b, e, h in rows]
    lines += ["", f"TOTAL  {len(rows)} files, {total_bytes:,} bytes, {total_entries:,} entries", ""]
    (dest / "MANIFEST.txt").write_text("\n".join(lines), encoding="utf-8")
    return len(rows), total_bytes, total_entries


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--game-dir", help="Crimson Desert install path")
    ap.add_argument("--version", required=True, help="version label (e.g. 1.12)")
    ap.add_argument(
        "--out",
        help="output directory (default: $CRIMSON_GAMEDATA_BIN/<version> if set)",
    )
    ap.add_argument(
        "--paloc",
        action="append",
        default=[],
        metavar="LANG",
        help="also keep this language's PALOC under <out>/paloc/<LANG>/ "
        "(repeatable; the archive keeps 'eng' from 2.02 on)",
    )
    args = ap.parse_args()

    game = find_game_dir(args.game_dir)
    if args.out:
        out_dir = Path(args.out)
    elif os.environ.get("CRIMSON_GAMEDATA_BIN"):
        out_dir = Path(os.environ["CRIMSON_GAMEDATA_BIN"]) / args.version
    else:
        sys.exit("Pass --out or set CRIMSON_GAMEDATA_BIN.")
    paloc_targets = resolve_paloc(game, args.paloc)
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

    for t in paloc_targets:
        files, size, entries = extract_paloc(game, args.version, t, out_dir)
        print(f"PALOC {t.lang}: {files} files, {size:,} B, {entries:,} entries -> {out_dir / 'paloc' / t.lang}")


if __name__ == "__main__":
    main()
