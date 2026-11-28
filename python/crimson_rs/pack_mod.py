from __future__ import annotations

import shutil
from pathlib import Path

import crimson_rs
from crimson_rs._pack_safety import TRAILER_WARNING, check_group_free, check_group_name, mod_files
from crimson_rs.enums import Compression, Crypto, Language


def pack_mod(
    game_dir: str,
    mod_folder: str,
    output_dir: str,
    group_name: str,
    compression: Compression = Compression.LZ4,
    crypto: Crypto = Crypto.NONE,
    encrypt_info: bytes = b"\x00\x00\x00",
    max_chunk_size: int = 500_000_000,
    is_optional: bool = False,
    language: Language = Language.ALL,
) -> None:
    """Pack ``mod_folder`` into a new group ``output_dir/<group_name>`` and write
    ``output_dir/meta/0.papgt`` with an entry for it. The game directory is only
    read. ``group_name`` must be four digits and not already in the game's
    ``0.papgt``. Whether a 2.04+ game loads the result is not established (see
    the trailer warning)."""
    game_path = Path(game_dir)
    mod_path = Path(mod_folder)
    out_path = Path(output_dir)

    check_group_name(group_name)
    if not game_path.is_dir():
        raise FileNotFoundError(f"Game directory not found: {game_path}")

    original_papgt = game_path / "meta" / "0.papgt"
    if not original_papgt.exists():
        raise FileNotFoundError(f"Original PAPGT not found: {original_papgt}")

    if not mod_path.is_dir():
        raise FileNotFoundError(f"Mod folder not found: {mod_path}")

    papgt = crimson_rs.parse_papgt_file(str(original_papgt))
    check_group_free(papgt, group_name, original_papgt)

    group_path = out_path / group_name
    if group_path.exists():
        raise FileExistsError(f"Group directory already exists: {group_path}")
    files = mod_files(mod_path)

    print(f"Packing files from: {mod_path}")
    print(f"Output group dir:   {group_path}")

    group_path.mkdir(parents=True)
    try:
        builder = crimson_rs.PackGroupBuilder(
            output_dir=str(group_path),
            compression=int(compression),
            crypto=int(crypto),
            encrypt_info=encrypt_info,
            max_chunk_size=max_chunk_size,
        )
        for count, file_path in enumerate(files, 1):
            rel = file_path.relative_to(mod_path)
            dir_path = str(rel.parent).replace("\\", "/")
            if dir_path == ".":
                dir_path = ""
            builder.add_file_from_path(dir_path, rel.name, str(file_path))
            if count % 100 == 0:
                print(f"  Added {count} files...")
        pamt_bytes = builder.finish()
    except BaseException:
        # Don't leave a half-built group behind: a rerun would hit
        # FileExistsError on it.
        shutil.rmtree(group_path, ignore_errors=True)
        raise

    print(f"Packed {len(files)} file(s) into group '{group_name}'")
    print(f"  .paz chunk(s) + 0.pamt written to {group_path}")

    pamt_post_header = pamt_bytes[12:]
    pamt_checksum = crimson_rs.calculate_checksum(pamt_post_header)
    print(f"  PAMT checksum: 0x{pamt_checksum:08X}")

    print(f"  Original PAPGT {original_papgt} has {len(papgt['entries'])} entries")
    updated_papgt = crimson_rs.add_papgt_entry(
        papgt_data=papgt,
        group_name=group_name,
        pack_meta_checksum=pamt_checksum,
        is_optional=int(is_optional),
        language=int(language),
    )
    print(f"  Added entry for '{group_name}', now {len(updated_papgt['entries'])} entries")
    if papgt.get("_trailer"):
        print(TRAILER_WARNING)

    meta_dir = out_path / "meta"
    meta_dir.mkdir(parents=True, exist_ok=True)
    output_papgt = meta_dir / "0.papgt"
    crimson_rs.write_papgt_file(updated_papgt, str(output_papgt))
    print(f"  Written updated PAPGT to: {output_papgt}")

    print()
    print("Done! To install, copy these into the game directory (keep a copy of the")
    print("game's own meta/0.papgt first):")
    print(f"  {group_path}  ->  {game_path / group_name}")
    print(f"  {output_papgt}  ->  {game_path / 'meta' / '0.papgt'}")
