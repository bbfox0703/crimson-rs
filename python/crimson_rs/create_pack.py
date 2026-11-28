from __future__ import annotations

import shutil
from pathlib import Path

import crimson_rs
from crimson_rs._pack_safety import (
    TRAILER_WARNING,
    backup_papgt,
    check_group_free,
    check_group_name,
    mod_files,
)
from crimson_rs.enums import Compression, Crypto, Language


def create_pack_group(
    game_dir: str,
    group_name: str,
    files: dict[str, dict[str, bytes]],
    compression: Compression = Compression.LZ4,
    crypto: Crypto = Crypto.NONE,
    encrypt_info: bytes = b"\x00\x00\x00",
    max_chunk_size: int = 500_000_000,
    is_optional: bool = False,
    language: Language = Language.ALL,
) -> None:
    """Pack ``files`` (``{dir: {name: bytes}}``) into ``game_dir/<group_name>``
    and add the group to the game's ``meta/0.papgt`` **in place** (a timestamped
    backup is written next to it first). ``group_name`` must be four digits and
    not already in ``0.papgt``."""
    game_path = Path(game_dir)
    group_path = game_path / group_name
    papgt = _validate(game_path, group_name, group_path)
    if not any(dir_files for dir_files in files.values()):
        raise ValueError("No files provided")

    def add(builder) -> int:
        count = 0
        for dir_path, dir_files in files.items():
            for file_name, data in dir_files.items():
                builder.add_file(dir_path, file_name, data)
                count += 1
        return count

    count, pamt_bytes = _build(group_path, add, compression, crypto, encrypt_info, max_chunk_size)
    print(f"Packed {count} files into group '{group_name}'")
    _update_papgt(game_path, papgt, group_name, pamt_bytes, is_optional, language)


def create_pack_group_from_folder(
    game_dir: str,
    group_name: str,
    mod_folder: str,
    compression: Compression = Compression.LZ4,
    crypto: Crypto = Crypto.NONE,
    encrypt_info: bytes = b"\x00\x00\x00",
    max_chunk_size: int = 500_000_000,
    is_optional: bool = False,
    language: Language = Language.ALL,
) -> None:
    """As :func:`create_pack_group`, packing every file under ``mod_folder``."""
    game_path = Path(game_dir)
    mod_path = Path(mod_folder)
    group_path = game_path / group_name

    if not mod_path.is_dir():
        raise FileNotFoundError(f"Mod folder not found: {mod_folder}")
    papgt = _validate(game_path, group_name, group_path)
    paths = mod_files(mod_path)

    def add(builder) -> int:
        for count, file_path in enumerate(paths, 1):
            rel = file_path.relative_to(mod_path)
            dir_path = str(rel.parent).replace("\\", "/")
            if dir_path == ".":
                dir_path = ""
            builder.add_file_from_path(dir_path, rel.name, str(file_path))
            if count % 100 == 0:
                print(f"  Added {count} files...")
        return len(paths)

    count, pamt_bytes = _build(group_path, add, compression, crypto, encrypt_info, max_chunk_size)
    print(f"Packed {count} files into group '{group_name}'")
    _update_papgt(game_path, papgt, group_name, pamt_bytes, is_optional, language)


def _validate(game_path: Path, group_name: str, group_path: Path) -> dict:
    check_group_name(group_name)
    if not game_path.is_dir():
        raise FileNotFoundError(f"Game directory not found: {game_path}")

    papgt_path = game_path / "meta" / "0.papgt"
    if not papgt_path.exists():
        raise FileNotFoundError(f"PAPGT not found: {papgt_path}")

    if group_path.exists():
        raise FileExistsError(f"Group directory already exists: {group_path}")

    papgt = crimson_rs.parse_papgt_file(str(papgt_path))
    check_group_free(papgt, group_name, papgt_path)
    return papgt


def _build(group_path: Path, add, compression, crypto, encrypt_info, max_chunk_size) -> tuple[int, bytes]:
    """Create the group folder, pack into it, and remove it again if anything
    fails, so a rerun does not trip over a half-built group."""
    group_path.mkdir(parents=True)
    try:
        builder = crimson_rs.PackGroupBuilder(
            output_dir=str(group_path),
            compression=int(compression),
            crypto=int(crypto),
            encrypt_info=encrypt_info,
            max_chunk_size=max_chunk_size,
        )
        count = add(builder)
        return count, builder.finish()
    except BaseException:
        shutil.rmtree(group_path, ignore_errors=True)
        raise


def _update_papgt(
    game_path: Path,
    papgt: dict,
    group_name: str,
    pamt_bytes: bytes,
    is_optional: bool,
    language: Language,
) -> None:
    papgt_path = game_path / "meta" / "0.papgt"

    pamt_post_header = pamt_bytes[12:]
    pamt_checksum = crimson_rs.calculate_checksum(pamt_post_header)

    if papgt.get("_trailer"):
        print(TRAILER_WARNING)
    updated_papgt = crimson_rs.add_papgt_entry(
        papgt_data=papgt,
        group_name=group_name,
        pack_meta_checksum=pamt_checksum,
        is_optional=int(is_optional),
        language=int(language),
    )
    backup_papgt(papgt_path)
    crimson_rs.write_papgt_file(updated_papgt, str(papgt_path))
    print(f"Updated {papgt_path} (added entry for '{group_name}')")
    print("Done.")
