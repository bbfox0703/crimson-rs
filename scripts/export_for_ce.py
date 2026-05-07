"""One-shot exporter for Crimson Desert 1.05 → CE Table data.

Produces, in a single run, everything `Generate_item_id_list.ps1` needs:

    out/
    ├── iteminfo.pabgb            # raw decompressed binary, for reference
    ├── items.jsonl               # one item per line; anchor mode = 100%
    │                              # coverage (with key + string_key always
    │                              # present, and parsed fields where the
    │                              # 1.05 parser succeeds)
    ├── items_skipped.json        # offsets where the lossy parser re-synced
    │                              # (only written in lossy mode)
    ├── paloc_eng.json            # {item_key: en_name}
    ├── paloc_zho-tw.json         # {item_key: zh-tw_name}
    ├── paloc_jpn.json            # {item_key: jp_name}
    ├── output.txt                # CE dropdown list (en)   "<idx>:<name>/<key>"
    ├── output_zh-tw.txt          # CE dropdown list (zh-tw)
    └── output_ja.txt             # CE dropdown list (ja)

Usage:
    python scripts/export_for_ce.py [--game-dir PATH] [--out PATH]
                                    [--keys PATH]

`--keys keys.txt` (the CE-dumped, in-game-ordered list of all itemKeys)
turns on **anchor mode**: every item from keys.txt is located in the
binary by its key, so items.jsonl has 100% item coverage and the CE
dropdown list covers every item. Without `--keys` the script falls back
to lossy parsing of the binary alone, which currently extracts only
the items the parser fully understands.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

import crimson_rs


GAME_DIR_CANDIDATES = [
    r"D:\SteamLibrary\steamapps\common\Crimson Desert",
    r"F:\SteamLibrary\steamapps\common\Crimson Desert",
    r"E:\SteamLibrary\steamapps\common\Crimson Desert",
    r"C:\Program Files (x86)\Steam\steamapps\common\Crimson Desert",
]


# Localization groups that may host the paloc files. Patches occasionally
# move them between groups, so we try each in order until we find one.
PALOC_GROUPS = [f"{n:04d}" for n in range(20, 36)]
PALOC_DIR = "gamedata/stringtable/binary__"

PALOC_TARGETS = [
    ("eng", "paloc_eng.json"),
    ("zho-tw", "paloc_zho-tw.json"),
    ("jpn", "paloc_jpn.json"),
]


def find_game_dir(explicit: str | None) -> str:
    if explicit:
        if not Path(explicit).is_dir():
            sys.exit(f"--game-dir not found: {explicit}")
        return explicit
    for c in GAME_DIR_CANDIDATES:
        if Path(c).is_dir():
            return c
    sys.exit(
        "Game install not found. Pass --game-dir, or edit GAME_DIR_CANDIDATES "
        "in this script."
    )


def _is_ident_byte(b: int) -> bool:
    # ASCII alphanumeric / underscore / space, OR any UTF-8 high byte.
    return (
        b == ord("_")
        or b == ord(" ")
        or 48 <= b <= 57
        or 65 <= b <= 90
        or 97 <= b <= 122
        or b >= 0x80
    )


def _looks_like_item_start(data: bytes, off: int, expected_key: int) -> bool:
    import struct

    if off + 12 > len(data):
        return False
    key, slen = struct.unpack_from("<II", data, off)
    if key != expected_key:
        return False
    if not (2 <= slen <= 128):
        return False
    if off + 8 + slen + 1 > len(data):
        return False
    if any(not _is_ident_byte(b) for b in data[off + 8 : off + 8 + slen]):
        return False
    return data[off + 8 + slen] == 0


def _read_string_key(data: bytes, off: int) -> str:
    import struct

    slen = struct.unpack_from("<I", data, off + 4)[0]
    return data[off + 8 : off + 8 + slen].decode("utf-8", errors="replace")


def _find_anchors(data: bytes, keys: list[int]) -> list[int]:
    import struct

    if not keys:
        return []
    if not _looks_like_item_start(data, 0, keys[0]):
        sys.exit("First key does not appear at file offset 0")
    anchors = [0]
    for i in range(1, len(keys)):
        cursor = anchors[-1] + 60
        target = struct.pack("<I", keys[i])
        found = -1
        while cursor + 12 <= len(data):
            idx = data.find(target, cursor)
            if idx < 0:
                break
            if _looks_like_item_start(data, idx, keys[i]):
                found = idx
                break
            cursor = idx + 1
        if found < 0:
            sys.exit(f"Anchor scan failed at i={i} key={keys[i]}")
        anchors.append(found)
    return anchors


def export_iteminfo_anchored(
    game_dir: str, out_dir: Path, keys: list[int]
) -> list[dict]:
    """Extract iteminfo.pabgb and use the supplied key order to anchor
    every item in the binary. Each item is parsed if possible; otherwise
    a fallback record with key + string_key is emitted, so items.jsonl
    has 100% coverage even when the parser is incomplete."""
    print("-" * 60)
    print("Step 1: iteminfo.pabgb (anchor mode)")
    raw = bytes(
        crimson_rs.extract_file(
            game_dir, "0008", "gamedata/binary__/client/bin", "iteminfo.pabgb"
        )
    )
    raw_path = out_dir / "iteminfo.pabgb"
    raw_path.write_bytes(raw)
    print(f"  raw: {len(raw):,}B -> {raw_path.name}")

    anchors = _find_anchors(raw, keys)
    print(f"  anchors resolved: {len(anchors):,} / {len(keys):,}")

    items: list[dict] = []
    status_hist: dict[str, int] = {"ok": 0, "leftover": 0, "fail": 0}

    jsonl_path = out_dir / "items.jsonl"
    with jsonl_path.open("w", encoding="utf-8") as fh:
        for i, key in enumerate(keys):
            off = anchors[i]
            end = anchors[i + 1] if i + 1 < len(anchors) else len(raw)
            chunk = raw[off:end]
            string_key = _read_string_key(raw, off)

            res = crimson_rs.parse_iteminfo_tracked(chunk)
            spans = res["spans"]
            if spans:
                rec = dict(res["items"][0])
                consumed = spans[0]["end"]
                if consumed == len(chunk):
                    status = "ok"
                else:
                    status = f"leftover:{len(chunk) - consumed}"
            else:
                err = res.get("error_span", {})
                rec = {"key": key, "string_key": string_key}
                status = f"fail:{err.get('path', '?')}"

            status_hist[status.split(":", 1)[0]] += 1

            rec["_index"] = i
            rec["_anchor_off"] = off
            rec["_anchor_size"] = len(chunk)
            rec["_status"] = status

            fh.write(json.dumps(rec, ensure_ascii=False, separators=(",", ":")))
            fh.write("\n")
            items.append(rec)

    print(f"  -> {jsonl_path.name}  ({jsonl_path.stat().st_size:,}B)")
    print(
        f"  parser status: ok={status_hist['ok']:,}  "
        f"leftover={status_hist['leftover']:,}  "
        f"fail={status_hist['fail']:,}"
    )
    return items


def export_iteminfo_lossy(game_dir: str, out_dir: Path) -> list[dict]:
    """Lossy fallback when no keys.txt is supplied. Only items the parser
    fully understands end up in items.jsonl."""
    print("-" * 60)
    print("Step 1: iteminfo.pabgb (lossy mode — no --keys supplied)")
    raw = bytes(
        crimson_rs.extract_file(
            game_dir, "0008", "gamedata/binary__/client/bin", "iteminfo.pabgb"
        )
    )
    raw_path = out_dir / "iteminfo.pabgb"
    raw_path.write_bytes(raw)
    print(f"  raw: {len(raw):,}B -> {raw_path.name}")

    res = crimson_rs.parse_iteminfo_lossy(raw)
    items = res["items"]
    errors = res["errors"]
    print(f"  parsed: {len(items):,} items   skipped: {len(errors):,} regions")

    jsonl_path = out_dir / "items.jsonl"
    with jsonl_path.open("w", encoding="utf-8") as fh:
        for it in items:
            fh.write(json.dumps(it, ensure_ascii=False, separators=(",", ":")))
            fh.write("\n")
    print(f"  -> {jsonl_path.name}  ({jsonl_path.stat().st_size:,}B)")

    skip_path = out_dir / "items_skipped.json"
    with skip_path.open("w", encoding="utf-8") as fh:
        json.dump(
            [
                {
                    "item_start": e["item_start"],
                    "fail_offset": e["fail_offset"],
                    "recovered_at": e["recovered_at"],
                    "skipped_bytes": e["skipped_bytes"],
                    "path": e["path"],
                    "message": e["message"],
                }
                for e in errors
            ],
            fh,
            ensure_ascii=False,
            indent=2,
        )
    print(f"  -> {skip_path.name}")
    return items


def export_paloc(game_dir: str, out_dir: Path) -> dict[str, dict[int, str]]:
    """Extract + parse the requested paloc files. Returns {lang: {item_key: name}}."""
    print("-" * 60)
    print("Step 2: paloc files")
    out: dict[str, dict[int, str]] = {}
    for lang, out_name in PALOC_TARGETS:
        fname = f"localizationstring_{lang}.paloc"
        raw: bytes | None = None
        hit_group: str | None = None
        for g in PALOC_GROUPS:
            try:
                raw = bytes(
                    crimson_rs.extract_file(game_dir, g, PALOC_DIR, fname)
                )
                hit_group = g
                break
            except Exception:
                continue
        if raw is None:
            print(f"  [{lang:<6}] not found in groups 0020-0035, skipped")
            continue
        entries = crimson_rs.parse_paloc_bytes(raw)
        item_names: dict[int, str] = {}
        for e in entries:
            try:
                sid = int(e["string_key"])
            except (ValueError, TypeError):
                continue
            if sid & 0xFF != 0x70:
                continue
            item_key = sid >> 32
            item_names[item_key] = e["string_value"]
        out[lang] = item_names
        out_path = out_dir / out_name
        with out_path.open("w", encoding="utf-8") as fh:
            json.dump(
                {str(k): v for k, v in sorted(item_names.items())},
                fh,
                ensure_ascii=False,
                indent=2,
                sort_keys=True,
            )
        print(
            f"  [{lang:<6}] group {hit_group}, {len(entries):,} entries, "
            f"{len(item_names):,} item names -> {out_name}"
        )
    return out


def load_keys_file(keys_path: Path) -> list[int]:
    keys: list[int] = []
    for raw_line in keys_path.read_text(encoding="utf-8").splitlines():
        s = raw_line.strip()
        if not s:
            continue
        try:
            k = int(s)
        except ValueError:
            break
        if k == 0xFFFFFFFF or k == 0:
            break
        keys.append(k)
    return keys


def write_ce_lists(
    out_dir: Path,
    keys: list[int],
    paloc: dict[str, dict[int, str]],
    string_keys: dict[int, str] | None = None,
) -> None:
    """Write the three CE dropdown lists.

    For each item, the English name comes from paloc 0x70 first, and
    falls back to the item's `string_key` (the internal id used by the
    game itself when no localized name exists — e.g. "TestShield",
    "LightSaber_TwoHandSword"). zh-tw and ja fall back the same way so
    every line is non-empty across all three files.
    """
    print("-" * 60)
    print("Step 3: CE dropdown lists")
    en_map = paloc.get("eng", {})
    zh_map = paloc.get("zho-tw", {})
    ja_map = paloc.get("jpn", {})
    string_keys = string_keys or {}

    en_lines: list[str] = []
    zh_lines: list[str] = []
    ja_lines: list[str] = []
    en_from_paloc = 0
    en_from_string_key = 0
    en_unknown = 0
    zh_from_paloc = 0
    ja_from_paloc = 0
    for idx, key in enumerate(keys):
        sk = string_keys.get(key)
        if key in en_map:
            en_name = en_map[key]
            en_from_paloc += 1
        elif sk:
            en_name = sk
            en_from_string_key += 1
        else:
            en_name = "Unknown"
            en_unknown += 1
        en_lines.append(f"{idx}:{en_name}/{key}")

        if key in zh_map:
            zh_lines.append(f"{idx}:{zh_map[key]}/{key}")
            zh_from_paloc += 1
        elif sk:
            zh_lines.append(f"{idx}:{sk}/{key}")

        if key in ja_map:
            ja_lines.append(f"{idx}:{ja_map[key]}/{key}")
            ja_from_paloc += 1
        elif sk:
            ja_lines.append(f"{idx}:{sk}/{key}")

    (out_dir / "output.txt").write_text("\n".join(en_lines), encoding="utf-8")
    (out_dir / "output_zh-tw.txt").write_text(
        "\n".join(zh_lines), encoding="utf-8"
    )
    (out_dir / "output_ja.txt").write_text(
        "\n".join(ja_lines), encoding="utf-8"
    )
    print(
        f"  output.txt        {len(en_lines):>6} lines  "
        f"(paloc: {en_from_paloc}, string_key: {en_from_string_key}, "
        f"Unknown: {en_unknown})"
    )
    print(
        f"  output_zh-tw.txt  {len(zh_lines):>6} lines  "
        f"(paloc: {zh_from_paloc}, string_key: {len(zh_lines)-zh_from_paloc})"
    )
    print(
        f"  output_ja.txt     {len(ja_lines):>6} lines  "
        f"(paloc: {ja_from_paloc}, string_key: {len(ja_lines)-ja_from_paloc})"
    )


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--game-dir", help="Crimson Desert install path")
    ap.add_argument(
        "--out",
        default=str(Path(__file__).resolve().parent.parent / "out"),
        help="output directory (default: <repo>/out)",
    )
    ap.add_argument(
        "--keys",
        default=str(Path(__file__).resolve().parent.parent / "data" / "keys.txt"),
        help="path to keys.txt (CE-dumped item key order). Defaults to "
        "<repo>/data/keys.txt; pass --keys '' to disable anchor mode "
        "and fall back to lossy parsing.",
    )
    args = ap.parse_args()

    game_dir = find_game_dir(args.game_dir)
    out_dir = Path(args.out).resolve()
    out_dir.mkdir(parents=True, exist_ok=True)
    print(f"Game dir : {game_dir}")
    print(f"Output   : {out_dir}")

    keys_arg = args.keys.strip() if args.keys else ""
    if keys_arg:
        keys_path = Path(keys_arg)
        if not keys_path.is_file():
            sys.exit(f"--keys not found: {keys_path}")
        keys = load_keys_file(keys_path)
        print(f"keys.txt        : {len(keys):,} keys")
        items = export_iteminfo_anchored(game_dir, out_dir, keys)
        key_source = f"keys.txt ({len(keys):,} keys)"
    else:
        items = export_iteminfo_lossy(game_dir, out_dir)
        keys = [it["key"] for it in items if "key" in it]
        key_source = f"items.jsonl ({len(keys):,} keys, lossy)"

    paloc = export_paloc(game_dir, out_dir)

    print(f"Key source: {key_source}")
    string_keys = {it["key"]: it["string_key"] for it in items if "string_key" in it}
    write_ce_lists(out_dir, keys, paloc, string_keys)

    print("-" * 60)
    print(f"Done. All files written to: {out_dir}")


if __name__ == "__main__":
    main()
