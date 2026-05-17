#!/usr/bin/env python3
"""Decode the world-map DDS files in `out/worldmap/` to PNGs.

Reuses `crimsonforge/core/dds_reader.py` for decoding (handles Pearl
Abyss's type-1 per-mip LZ4 self-compression + DXT1/3/5/BC4/5/6/7 + the
uncompressed BGRA/RGBA cases). Writes side-by-side PNGs into the same
directory.

Usage:
    python scripts/decode_worldmap_dds.py
"""

from __future__ import annotations

import sys
from pathlib import Path

# Make crimsonforge's `core.dds_reader` importable. We don't depend on
# the rest of crimsonforge.
CRIMSONFORGE = Path(r"D:\Github\crimsonforge")
if not CRIMSONFORGE.is_dir():
    print(f"ERROR: crimsonforge not found at {CRIMSONFORGE}", file=sys.stderr)
    sys.exit(1)
sys.path.insert(0, str(CRIMSONFORGE))

# crimsonforge's logger expects an importable path. Use a minimal stub
# so we don't drag in its full app config.
import types
stub = types.ModuleType("utils.logger")
def _stub_logger(*_args, **_kwargs):
    import logging
    return logging.getLogger("decode_worldmap")
stub.get_logger = _stub_logger
utils_mod = types.ModuleType("utils")
utils_mod.logger = stub
sys.modules.setdefault("utils", utils_mod)
sys.modules.setdefault("utils.logger", stub)

from core.dds_reader import decode_dds_to_rgba
from PIL import Image

DDS_DIR = Path(r"D:\Github\crimson-rs\out\worldmap")


def main() -> int:
    if not DDS_DIR.is_dir():
        print(f"ERROR: DDS dir not found: {DDS_DIR}", file=sys.stderr)
        return 1

    dds_files = sorted(DDS_DIR.glob("*.dds"))
    if not dds_files:
        print(f"ERROR: no DDS files in {DDS_DIR}", file=sys.stderr)
        return 1

    ok = 0
    fail = 0
    for dds_path in dds_files:
        png_path = dds_path.with_suffix(".png")
        try:
            data = dds_path.read_bytes()
            w, h, rgba = decode_dds_to_rgba(data)
            img = Image.frombytes("RGBA", (w, h), rgba)
            img.save(png_path)
            print(f"  {dds_path.name:<48} -> {png_path.name}  ({w}x{h})")
            ok += 1
        except Exception as e:
            print(f"  {dds_path.name:<48} FAILED: {type(e).__name__}: {e}", file=sys.stderr)
            fail += 1

    print(f"\nDecoded: {ok} ok, {fail} failed.")
    return 0 if fail == 0 else 2


if __name__ == "__main__":
    sys.exit(main())
