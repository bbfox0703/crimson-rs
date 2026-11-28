"""Guards shared by the packing helpers (audit H-06 to H-09).

The packers write a new group folder and a ``0.papgt`` entry for it. These
checks keep them from replacing a vanilla group, writing outside the target
folder, or rewriting the live ``0.papgt`` with no way back.
"""

from __future__ import annotations

import re
import shutil
import time
from pathlib import Path

_GROUP_NAME = re.compile(r"\d{4}")


def check_group_name(group_name: str) -> None:
    """Every vanilla group is a four-digit folder (``0000``..``0040`` on 2.06).
    Anything else, ``..`` or an absolute path included, could land outside the
    target folder, so it is refused."""
    if not _GROUP_NAME.fullmatch(group_name):
        raise ValueError(
            f"group_name must be four digits, like the game's own groups (got {group_name!r})"
        )


def check_group_free(papgt: dict, group_name: str, papgt_path: Path) -> None:
    """``add_papgt_entry`` replaces an entry of the same name. On 2.06 the
    PAPGT lists groups with no folder on disk (``0036``..``0040`` are the
    optional language groups), so a free folder name is not enough."""
    taken = {e["group_name"] for e in papgt["entries"]}
    if group_name in taken:
        raise ValueError(
            f"group {group_name!r} is already listed in {papgt_path}; adding it would replace that "
            f"entry. Pick a number not in the file (taken: {', '.join(sorted(taken))})"
        )


def backup_papgt(papgt_path: Path) -> Path:
    """Copy ``papgt_path`` next to itself before an in-place rewrite and
    return the copy's path. The 2.04+ file ends in a signature-like trailer
    that cannot be recomputed, so the original bytes are the only way back."""
    backup = papgt_path.with_name(f"{papgt_path.name}.bak-{time.strftime('%Y%m%d-%H%M%S')}")
    n = 1
    while backup.exists():
        backup = papgt_path.with_name(f"{papgt_path.name}.bak-{time.strftime('%Y%m%d-%H%M%S')}-{n}")
        n += 1
    shutil.copy2(papgt_path, backup)
    print(f"  Backed up {papgt_path} to {backup}")
    return backup


def mod_files(mod_path: Path) -> list[Path]:
    """The files under ``mod_path``, sorted. Raises before anything is written
    when there are none."""
    files = [p for p in sorted(mod_path.rglob("*")) if p.is_file()]
    if not files:
        raise ValueError(f"No files found in {mod_path}")
    return files


TRAILER_WARNING = (
    "  WARNING: this 0.papgt ends in a 64-byte trailer that looks like a signature over the"
    " file. It cannot be recomputed, so it no longer matches the edited entries. Whether"
    " the game still loads it is not established."
)
