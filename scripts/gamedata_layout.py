"""Where the gamedata tables and paloc blobs live inside the archives.

Crimson Desert 2.01 reorganised the `0008` gamedata group and the
localization groups. Nothing about the file *contents* changed — PABGB
bodies, PABGH indices and PALOC records all parse byte-identically either
way — but every path and extension moved:

    static-info tables
      1.05 - 2.00   0008/gamedata/binary__/client/bin/<table>.pabgb
                                                     /<table>.pabgh
      2.01+         0008/gamedata/binarystaticinfo__/bin/<table>.staticinfobody
                                                        /<table>.staticinfoheader

    localization
      1.05 - 2.00   00NN/gamedata/stringtable/binary__/localizationstring_<lang>.paloc
      2.01+         00NN/gamedata/stringtable/binary__/<lang>/<namespace>.paloc

The 2.01 split gives 39 namespace files per language instead of one blob;
concatenating their entries reproduces the old single-file content, and the
`(key << 32) | group` string_key encoding is unchanged.

Both helpers probe the newest layout first and fall back, so the scripts
keep working against an older install — which the cross-version RE workflow
in `docs/historical-parser-setup.md` depends on.
"""

from __future__ import annotations

from pathlib import Path
from typing import NamedTuple

import crimson_rs


GAMEDATA_GROUP = "0008"


class BinLayout(NamedTuple):
    """Resolved location + extensions for the static-info gamedata tables."""

    dir: str
    body_ext: str
    header_ext: str

    def body(self, stem: str) -> str:
        """`"skill"` -> `"skill.pabgb"` / `"skill.staticinfobody"`."""
        return f"{stem}.{self.body_ext}"

    def header(self, stem: str) -> str:
        """`"skill"` -> `"skill.pabgh"` / `"skill.staticinfoheader"`."""
        return f"{stem}.{self.header_ext}"

    def stem_of(self, name: str) -> str | None:
        """Inverse of `body` / `header`: the table stem, or None if `name`
        is not a static-info table file in this layout."""
        for ext in (self.body_ext, self.header_ext):
            if name.endswith(f".{ext}"):
                return name[: -len(ext) - 1]
        return None


# Newest layout first.
BIN_LAYOUTS = (
    BinLayout("gamedata/binarystaticinfo__/bin", "staticinfobody", "staticinfoheader"),
    BinLayout("gamedata/binary__/client/bin", "pabgb", "pabgh"),
)

# Directory holding the per-language paloc files (or, since 2.01, the
# per-language subdirectories holding them).
PALOC_ROOT = "gamedata/stringtable/binary__"

# Pre-2.01 single-blob naming.
_PALOC_PREFIX = "localizationstring_"
_PALOC_SUFFIX = ".paloc"


def _read_pamt(game_dir: str | Path, group: str) -> dict | None:
    pamt_path = Path(game_dir) / group / "0.pamt"
    if not pamt_path.is_file():
        return None
    try:
        return crimson_rs.parse_pamt_bytes(pamt_path.read_bytes())
    except Exception:
        return None


def resolve_bin_layout(
    game_dir: str | Path, group: str = GAMEDATA_GROUP
) -> BinLayout:
    """Pick the static-info layout this install actually ships.

    Raises `LookupError` if neither layout is present — that means the
    archive moved again and this module needs a new `BIN_LAYOUTS` entry.
    """
    pamt = _read_pamt(game_dir, group)
    if pamt is None:
        raise LookupError(
            f"cannot read {Path(game_dir) / group / '0.pamt'} — is the game "
            f"installed at {game_dir}?"
        )
    present = {d.get("path") or d.get("name") or "" for d in pamt["directories"]}
    for layout in BIN_LAYOUTS:
        if layout.dir in present:
            return layout
    raise LookupError(
        f"no known static-info directory in {group}/0.pamt; tried "
        + ", ".join(repr(x.dir) for x in BIN_LAYOUTS)
        + ". Pearl Abyss moved the gamedata tables again — add the new "
        "layout to BIN_LAYOUTS in scripts/gamedata_layout.py."
    )


class PalocTarget(NamedTuple):
    """One language's paloc files, wherever they live."""

    lang: str
    group: str
    dir: str
    files: tuple[str, ...]


def discover_paloc_targets(
    game_dir: str | Path, groups: list[str]
) -> list[PalocTarget]:
    """Scan `groups` for every language's paloc file(s), both layouts.

    Deduped by language, first hit wins. `files` holds one entry for the
    pre-2.01 single blob and 39 namespace files for 2.01+.
    """
    found: list[PalocTarget] = []
    seen: set[str] = set()
    for g in groups:
        pamt = _read_pamt(game_dir, g)
        if pamt is None:
            continue
        for d in pamt["directories"]:
            dpath = d.get("path") or d.get("name") or ""
            if "stringtable" not in dpath:
                continue
            names = [f["name"] for f in d.get("files", [])]

            # 2.01+: the directory itself is the language, holding one
            # .paloc per namespace.
            if dpath.startswith(PALOC_ROOT + "/"):
                lang = dpath[len(PALOC_ROOT) + 1:]
                per_ns = tuple(sorted(n for n in names if n.endswith(_PALOC_SUFFIX)))
                if "/" not in lang and per_ns and lang not in seen:
                    seen.add(lang)
                    found.append(PalocTarget(lang, g, dpath, per_ns))
                continue

            # 1.05 - 2.00: one localizationstring_<lang>.paloc per language,
            # all sitting directly in the stringtable directory.
            for fname in names:
                if not (
                    fname.startswith(_PALOC_PREFIX) and fname.endswith(_PALOC_SUFFIX)
                ):
                    continue
                lang = fname[len(_PALOC_PREFIX): -len(_PALOC_SUFFIX)]
                if lang in seen:
                    continue
                seen.add(lang)
                found.append(PalocTarget(lang, g, dpath, (fname,)))
    return found


def paloc_entries(game_dir: str | Path, groups: list[str], lang: str) -> list[dict]:
    """Every paloc entry for one language, merged across whatever files the
    install ships (one blob pre-2.01, 39 namespace files from 2.01 on).

    Raises `SystemExit` when the language isn't in any of `groups` — these
    are diagnostic scripts, so failing loudly beats a silent empty result.
    """
    for t in discover_paloc_targets(game_dir, groups):
        if t.lang != lang:
            continue
        entries: list[dict] = []
        for fname in t.files:
            raw = bytes(crimson_rs.extract_file(game_dir, t.group, t.dir, fname))
            entries.extend(crimson_rs.parse_paloc_bytes(raw))
        return entries
    raise SystemExit(
        f"no paloc for language {lang!r} in groups {groups[0]}-{groups[-1]}"
    )
