"""One-shot exporter for Crimson Desert → CE Table data.

Produces, in a single run, everything `Generate_item_id_list.ps1` needs:

    out/
    ├── iteminfo.pabgb            # raw base table body, for reference
    ├── iteminfo.<dlc>.pabgb      # raw DLC table body (e.g. iteminfo.dlc_01.pabgb)
    ├── items.jsonl               # one item per line, in CE index order;
    │                              # key + string_key always present, parsed
    │                              # fields where the parser succeeds;
    │                              # `_index` = CE item ID, `_source` = base / <dlc>
    ├── items_skipped.json        # offsets where the lossy parser re-synced
    │                              # (only written in lossy mode)
    ├── paloc_<lang>.json         # base item names per language
    │                              # ({item_key: localized_name})
    ├── paloc_<lang>.<dlc>.json   # the DLC's own item names per language
    ├── output.txt                # CE dropdown list (en)   "<idx>:<name>/<key>"
    ├── output_zh-tw.txt          # CE dropdown list (zh-tw)
    ├── output_ja.txt             # CE dropdown list (ja)
    ├── output_ko.txt             # CE dropdown list (ko)
    └── output_<lang>.txt         # ...one per remaining language

Usage:
    python scripts/export_for_ce.py [--game-dir PATH] [--out PATH]
                                    [--order header|keys] [--no-dlc]
                                    [--keys PATH] [--dlc-keys PATH]

**CE item index (`--order header`, the default).** The game's runtime item
table is the base `iteminfo.staticinfoheader` in order (indices 0..N-1),
followed by each DLC's `iteminfo.<dlc>.staticinfoheader` rows whose keys the
base lacks, in that header's order; keys a DLC overrides keep their base
slot. The header also gives every row's offset, so no anchor scan is needed.
Verified on 2.05: the in-memory key array matched this row for row, and a
DLC item placed in-game (1004792 Clams, 651st DLC-only row) showed index
7068 + 650 = 7718. `keys.txt` (a CE dump) is then only a cross-check: a
mismatch is reported, and the header order is still used.

`--order keys` restores the older behaviour: keys.txt *is* the order and
each key is anchored in the base binary (base table only); with
`--keys ''` it falls back to lossy parsing of the binary alone.
"""

from __future__ import annotations

import argparse
import json
import os
import struct
import sys
from pathlib import Path
from typing import NamedTuple

import crimson_rs

from gamedata_layout import GAMEDATA_GROUP, discover_paloc_targets, resolve_bin_layout


GAME_DIR_CANDIDATES = [
    r"D:\SteamLibrary\steamapps\common\Crimson Desert",
    r"F:\SteamLibrary\steamapps\common\Crimson Desert",
    r"E:\SteamLibrary\steamapps\common\Crimson Desert",
    r"C:\Program Files (x86)\Steam\steamapps\common\Crimson Desert",
]


# Localization groups that may host the paloc files. Patches occasionally
# move them between groups, so we scan a generous range. Korean lives in
# 0019 (hence the lower bound), and the upper bound is intentionally loose
# to survive future shuffles.
PALOC_GROUPS = [f"{n:04d}" for n in range(19, 50)]

# Output filename suffix for the CE dropdown lists. Keep the historical
# names for the three languages the CE table already references; everything
# else falls through to its raw lang code (e.g. kor -> output_ko.txt is
# preferred over output_kor.txt for symmetry with ja / zh-tw).
OUTPUT_LANG_SUFFIX = {
    "eng":    "",        # output.txt
    "zho-tw": "zh-tw",   # output_zh-tw.txt
    "jpn":    "ja",      # output_ja.txt
    "kor":    "ko",      # output_ko.txt
    "zho-cn": "zh-cn",
    "cht":    "zh-tw",
    "chs":    "zh-cn",
}


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
    """A 2.04 row head: ``[u32 len][name][u32 key][is_blocked: u8 = 0]``."""
    import struct

    if off < 0 or off + 12 > len(data):
        return False
    slen = struct.unpack_from("<I", data, off)[0]
    if not (2 <= slen <= 128):
        return False
    key_at = off + 4 + slen
    if key_at + 5 > len(data):
        return False
    if struct.unpack_from("<I", data, key_at)[0] != expected_key:
        return False
    if any(not _is_ident_byte(b) for b in data[off + 4 : key_at]):
        return False
    return data[key_at + 4] == 0


def _read_string_key(data: bytes, off: int) -> str:
    import struct

    slen = struct.unpack_from("<I", data, off)[0]
    return data[off + 4 : off + 4 + slen].decode("utf-8", errors="replace")


def _row_start_for_key_at(data: bytes, key_at: int, expected_key: int) -> int:
    """The row start whose name ends at ``key_at``, or -1. Names are 2..128
    bytes, and only the true length reads back as its own length prefix."""
    import struct

    for slen in range(2, 129):
        off = key_at - 4 - slen
        if off < 0:
            break
        if struct.unpack_from("<I", data, off)[0] == slen and _looks_like_item_start(
            data, off, expected_key
        ):
            return off
    return -1


def _parse_row(raw: bytes, off: int, end: int, key: int) -> tuple[dict, str]:
    """Parse the row at ``raw[off:end]``. Returns the record and a status:
    ``ok``, ``leftover:<n>`` (parsed, n bytes unconsumed) or ``fail:<path>``
    (a key + string_key fallback record)."""
    chunk = raw[off:end]
    res = crimson_rs.parse_iteminfo_tracked(chunk)
    spans = res["spans"]
    if spans:
        rec = dict(res["items"][0])
        consumed = spans[0]["end"]
        status = "ok" if consumed == len(chunk) else f"leftover:{len(chunk) - consumed}"
    else:
        err = res.get("error_span", {})
        rec = {"key": key, "string_key": _read_string_key(raw, off)}
        status = f"fail:{err.get('path', '?')}"
    return rec, status


def _find_anchors(data: bytes, keys: list[int]) -> list[int | None]:
    """Locate each key's item-start offset in the binary.

    Returns one entry per input key. ``None`` means the key has no
    corresponding item in iteminfo.pabgb (CE dropdown sometimes carries
    extra trailer keys that aren't backed by an iteminfo entry — the
    caller emits a fallback record so dropdown indices stay aligned).
    """
    import struct

    if not keys:
        return []
    if not _looks_like_item_start(data, 0, keys[0]):
        sys.exit("First key does not appear at file offset 0")
    anchors: list[int | None] = [0]
    last_known_off = 0
    for i in range(1, len(keys)):
        cursor = last_known_off + 60
        target = struct.pack("<I", keys[i])
        found = -1
        while cursor + 12 <= len(data):
            idx = data.find(target, cursor)
            if idx < 0:
                break
            found = _row_start_for_key_at(data, idx, keys[i])
            if found >= 0:
                break
            cursor = idx + 1
        if found < 0:
            anchors.append(None)
        else:
            anchors.append(found)
            last_known_off = found
    return anchors


def export_iteminfo_anchored(
    game_dir: str, out_dir: Path, keys: list[int]
) -> list[dict]:
    """Extract iteminfo.pabgb and use the supplied key order to anchor
    every item in the binary. Each item is parsed if possible; otherwise
    a fallback record with key + string_key is emitted, so items.jsonl
    has 100% coverage even when the parser is incomplete."""
    print("-" * 60)
    print("Step 1: iteminfo (anchor mode)")
    layout = resolve_bin_layout(game_dir)
    raw = bytes(
        crimson_rs.extract_file(
            game_dir, "0008", layout.dir, layout.body("iteminfo")
        )
    )
    raw_path = out_dir / "iteminfo.pabgb"
    raw_path.write_bytes(raw)
    print(f"  raw: {len(raw):,}B -> {raw_path.name}")

    anchors = _find_anchors(raw, keys)
    resolved = sum(1 for a in anchors if a is not None)
    print(f"  anchors resolved: {resolved:,} / {len(keys):,}")

    items: list[dict] = []
    status_hist: dict[str, int] = {
        "ok": 0, "leftover": 0, "fail": 0, "no_anchor": 0,
    }

    jsonl_path = out_dir / "items.jsonl"
    with jsonl_path.open("w", encoding="utf-8") as fh:
        for i, key in enumerate(keys):
            off = anchors[i]
            if off is None:
                rec = {"key": key}
                status = "no_anchor"
                status_hist["no_anchor"] += 1
                rec["_index"] = i
                rec["_anchor_off"] = None
                rec["_anchor_size"] = 0
                rec["_status"] = status
                fh.write(json.dumps(rec, ensure_ascii=False, separators=(",", ":")))
                fh.write("\n")
                items.append(rec)
                continue

            next_off: int | None = None
            for j in range(i + 1, len(anchors)):
                if anchors[j] is not None:
                    next_off = anchors[j]
                    break
            end = next_off if next_off is not None else len(raw)
            rec, status = _parse_row(raw, off, end, key)

            status_hist[status.split(":", 1)[0]] += 1

            rec["_index"] = i
            rec["_anchor_off"] = off
            rec["_anchor_size"] = end - off
            rec["_status"] = status

            fh.write(json.dumps(rec, ensure_ascii=False, separators=(",", ":")))
            fh.write("\n")
            items.append(rec)

    print(f"  -> {jsonl_path.name}  ({jsonl_path.stat().st_size:,}B)")
    print(
        f"  parser status: ok={status_hist['ok']:,}  "
        f"leftover={status_hist['leftover']:,}  "
        f"fail={status_hist['fail']:,}  "
        f"no_anchor={status_hist['no_anchor']:,}"
    )
    return items


def export_iteminfo_lossy(game_dir: str, out_dir: Path) -> list[dict]:
    """Lossy fallback when no keys.txt is supplied. Only items the parser
    fully understands end up in items.jsonl."""
    print("-" * 60)
    print("Step 1: iteminfo (lossy mode — no --keys supplied)")
    layout = resolve_bin_layout(game_dir)
    raw = bytes(
        crimson_rs.extract_file(
            game_dir, "0008", layout.dir, layout.body("iteminfo")
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



# ── Header mode (default): CE index = base header rows, then DLC-only rows ──


class ItemRow(NamedTuple):
    key: int
    offset: int   # row start inside its source table's body
    source: str   # "base" or the DLC folder name ("dlc_01")


def _read_header_rows(hdr: bytes) -> list[tuple[int, int]]:
    """``(key, body offset)`` pairs, in order, from an iteminfo
    static-info header: u16 count (u32 on some tables) + (u32 key, u32 offset)*."""
    for cnt_fmt, start in (("<H", 2), ("<I", 4)):
        if len(hdr) < start:
            continue
        n = struct.unpack_from(cnt_fmt, hdr)[0]
        if start + 8 * n == len(hdr):
            return [struct.unpack_from("<II", hdr, start + 8 * i) for i in range(n)]
    raise ValueError(f"unrecognised iteminfo header shape ({len(hdr):,} B)")


def _load_iteminfo_table(root: str, stem: str) -> tuple[bytes, list[tuple[int, int]]]:
    """Extract ``<stem>`` body + header from ``<root>/0008`` and read the header."""
    layout = resolve_bin_layout(root)
    body = bytes(crimson_rs.extract_file(root, GAMEDATA_GROUP, layout.dir, layout.body(stem)))
    hdr = bytes(crimson_rs.extract_file(root, GAMEDATA_GROUP, layout.dir, layout.header(stem)))
    return body, _read_header_rows(hdr)


def discover_dlc_roots(game_dir: str) -> list[tuple[str, str]]:
    """``(name, root)`` for every ``<install>/dlc/<name>/`` folder, by folder
    name. An install without a ``dlc`` folder returns []."""
    dlc_dir = Path(game_dir) / "dlc"
    if not dlc_dir.is_dir():
        return []
    return [(d.name, str(d)) for d in sorted(p for p in dlc_dir.iterdir() if p.is_dir())]


class DlcTable(NamedTuple):
    name: str                      # folder name, "dlc_01"
    root: str                      # <install>/dlc/<name>
    body: bytes                    # iteminfo.<name> static-info body
    rows: list[tuple[int, int]]    # its header, (key, body offset) in order


class DlcUnavailable(Exception):
    """A DLC data file is missing or unreadable: export base items only."""


def load_dlc_tables(game_dir: str) -> list[DlcTable]:
    """Every installed DLC's iteminfo table, in folder-name order. Raises
    ``DlcUnavailable`` when any of them cannot be read: the CE index of a
    DLC depends on every DLC before it, so one bad DLC drops them all."""
    tables: list[DlcTable] = []
    for name, root in discover_dlc_roots(game_dir):
        if not (Path(root) / GAMEDATA_GROUP / "0.pamt").is_file():
            raise DlcUnavailable(f"{name}: no {GAMEDATA_GROUP}/0.pamt")
        try:
            body, rows = _load_iteminfo_table(root, f"iteminfo.{name}")
        except Exception as exc:
            raise DlcUnavailable(f"{name}: iteminfo.{name} unreadable ({exc})") from exc
        if not rows:
            raise DlcUnavailable(f"{name}: iteminfo.{name} header has no rows")
        tables.append(DlcTable(name, root, body, rows))
    return tables


def dlc_mark(source: str) -> str:
    """The name prefix for a DLC's own items in the CE lists:
    ``dlc_01`` -> ``(DLC1)``. Base items get none."""
    if source == "base":
        return ""
    head, _, num = source.rpartition("_")
    if head.lower() == "dlc" and num.isdigit():
        return f"(DLC{int(num)})"
    return f"({source.upper()})"


def _row_ends(rows: list[tuple[int, int]], body_len: int) -> dict[int, int]:
    """Row start -> row end (the next larger row start, or the body end)."""
    starts = sorted({off for _, off in rows})
    return {s: (starts[i + 1] if i + 1 < len(starts) else body_len)
            for i, s in enumerate(starts)}


def export_iteminfo_header(
    game_dir: str, out_dir: Path, dlc_tables: list[DlcTable]
) -> tuple[list[dict], list[ItemRow], int]:
    """Build the CE index order from the static-info headers (base rows,
    then each DLC's rows whose keys the base lacks) and parse every row.
    Returns ``(items, order, base_count)``."""
    print("-" * 60)
    print("Step 3: iteminfo (header order: base rows, then DLC-only rows)")
    base_body, base_rows = _load_iteminfo_table(game_dir, "iteminfo")
    (out_dir / "iteminfo.pabgb").write_bytes(base_body)
    print(f"  base    : {len(base_rows):,} rows -> index 0..{len(base_rows) - 1:,}"
          f"  ({len(base_body):,}B -> iteminfo.pabgb)")

    bodies = {"base": base_body}
    ends = {"base": _row_ends(base_rows, len(base_body))}
    order = [ItemRow(k, o, "base") for k, o in base_rows]
    seen = {k for k, _ in base_rows}

    for t in dlc_tables:
        fname = f"iteminfo.{t.name}.pabgb"
        (out_dir / fname).write_bytes(t.body)
        bodies[t.name] = t.body
        ends[t.name] = _row_ends(t.rows, len(t.body))
        new = [ItemRow(k, o, t.name) for k, o in t.rows if k not in seen]
        seen.update(r.key for r in new)
        first = len(order)
        order.extend(new)
        span = f"index {first:,}..{len(order) - 1:,}" if new else "nothing appended"
        print(f"  {t.name:<8}: {len(t.rows):,} rows, {len(new):,} new -> {span}; "
              f"{len(t.rows) - len(new):,} override base keys (keep their base slot)"
              f"  ({len(t.body):,}B -> {fname})")
    if not dlc_tables:
        print("  DLC     : none -> base items only")

    items: list[dict] = []
    hist: dict[str, dict[str, int]] = {}
    jsonl_path = out_dir / "items.jsonl"
    with jsonl_path.open("w", encoding="utf-8") as fh:
        for i, row in enumerate(order):
            body = bodies[row.source]
            end = ends[row.source][row.offset]
            rec, status = _parse_row(body, row.offset, end, row.key)
            h = hist.setdefault(row.source, {"ok": 0, "leftover": 0, "fail": 0})
            h[status.split(":", 1)[0]] += 1
            rec["_index"] = i
            rec["_source"] = row.source
            rec["_anchor_off"] = row.offset
            rec["_anchor_size"] = end - row.offset
            rec["_status"] = status
            fh.write(json.dumps(rec, ensure_ascii=False, separators=(",", ":")))
            fh.write("\n")
            items.append(rec)

    total = {s: sum(h[s] for h in hist.values()) for s in ("ok", "leftover", "fail")}
    print(f"  -> {jsonl_path.name}  ({jsonl_path.stat().st_size:,}B, {len(order):,} items)")
    print(f"  parser status: ok={total['ok']:,}  leftover={total['leftover']:,}  "
          f"fail={total['fail']:,}  no_anchor=0")
    if len(hist) > 1:
        for src, h in hist.items():
            print(f"    {src:<8} ok={h['ok']:,}  leftover={h['leftover']:,}  fail={h['fail']:,}")
    return items, order, len(base_rows)


def corroborate_keys(label: str, path: Path, expected: list[int], what: str) -> None:
    """Compare a CE-dumped key list with the header-derived order. Purely
    informational: the header order is used whatever this finds."""
    keys = load_keys_file(path)
    if keys == expected:
        print(f"  {label}: agrees with the header order ({len(keys):,} keys = {what})")
        return
    n = min(len(keys), len(expected))
    first = next((i for i in range(n) if keys[i] != expected[i]), n)
    missing = len(set(keys) - set(expected))
    if first == n and len(keys) > len(expected):
        detail = f"the {what} plus {len(keys) - len(expected):,} more"
    elif first == n:
        detail = f"the first {len(keys):,} of the {what}"
    else:
        detail = (f"first difference at index {first:,} "
                  f"(file {keys[first]}, header {expected[first]})")
    print(f"  {label}: WARNING — {len(keys):,} keys vs {len(expected):,} in the "
          f"{what}; {detail}; {missing:,} keys not in the header order. "
          f"Using the header order.")


def export_paloc(
    game_dir: str,
    out_dir: Path,
    tag: str = "",
    required_langs: set[str] | None = None,
) -> dict[str, dict[int, str]]:
    """Discover + extract + parse every paloc language file the game ships.
    Returns {lang: {item_key: name}}. ``tag`` (e.g. ``".dlc_01"``) goes into
    the json filenames when reading a DLC root.

    With ``required_langs`` (DLC roots) nothing is tolerated: no paloc at
    all, a language of that set missing, a file that fails to extract or
    parse, or a language without item names raises ``DlcUnavailable``
    before any json is written.
    """
    print("-" * 60)
    print(f"Step 2{tag}: paloc files (auto-discover all languages)")
    targets = discover_paloc_targets(game_dir, PALOC_GROUPS)
    strict = required_langs is not None
    if strict:
        missing = sorted(set(required_langs) - {t.lang for t in targets})
        if missing:
            raise DlcUnavailable(f"no {tag.lstrip('.')} paloc for: {', '.join(missing)}")
    if not targets:
        print(f"  no paloc files found in groups "
              f"{PALOC_GROUPS[0]}-{PALOC_GROUPS[-1]}")
        return {}
    print(f"  discovered {len(targets)} language(s): "
          + ", ".join(t.lang for t in targets))

    out: dict[str, dict[int, str]] = {}
    summary: list[tuple[str, str, int, int]] = []
    for lang, group, pdir, fnames in targets:
        # Pre-2.01 ships one blob per language; 2.01+ splits it into one
        # file per namespace. Item names (group byte 0x70) come out of the
        # merged set either way.
        item_names: dict[int, str] = {}
        n_entries = 0
        failed = 0
        for fname in fnames:
            try:
                raw = bytes(
                    crimson_rs.extract_file(game_dir, group, pdir, fname)
                )
            except Exception as exc:
                if strict:
                    raise DlcUnavailable(f"{lang} {fname}: {exc}") from exc
                if not failed:
                    print(f"  [{lang:<6}] extract failed from group {group} "
                          f"({fname}): {exc}")
                failed += 1
                continue
            try:
                entries = crimson_rs.parse_paloc_bytes(raw)
            except Exception as exc:
                if strict:
                    raise DlcUnavailable(f"{lang} {fname}: {exc}") from exc
                raise
            n_entries += len(entries)
            for e in entries:
                try:
                    sid = int(e["string_key"])
                except (ValueError, TypeError):
                    continue
                if sid & 0xFF != 0x70:
                    continue
                item_names[sid >> 32] = e["string_value"]
        if failed == len(fnames):
            continue
        if strict and not item_names:
            raise DlcUnavailable(f"{lang}: no item names in the {tag.lstrip('.')} paloc")
        out[lang] = item_names
        summary.append((lang, group, len(fnames), n_entries))

    # Written only once every language has loaded, so a strict (DLC) run
    # that bails out leaves no partial set of json files behind.
    for lang, group, n_files, n_entries in summary:
        item_names = out[lang]
        out_name = f"paloc_{lang}{tag}.json"
        with (out_dir / out_name).open("w", encoding="utf-8") as fh:
            json.dump(
                {str(k): v for k, v in sorted(item_names.items())},
                fh,
                ensure_ascii=False,
                indent=2,
                sort_keys=True,
            )
        print(
            f"  [{lang:<6}] group {group}, {n_files} file(s), "
            f"{n_entries:,} entries, "
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


def _output_filename(lang: str) -> str:
    """Map a paloc lang code to its CE dropdown filename. eng -> output.txt;
    everything else -> output_<suffix>.txt where suffix is the OUTPUT_LANG_SUFFIX
    override (e.g. jpn -> ja, kor -> ko) or the raw lang code."""
    suffix = OUTPUT_LANG_SUFFIX.get(lang, lang)
    return "output.txt" if suffix == "" else f"output_{suffix}.txt"


def write_ce_lists(
    out_dir: Path,
    keys: list[int],
    paloc: dict[str, dict[int, str]],
    string_keys: dict[int, str] | None = None,
    marks: list[str] | None = None,
) -> None:
    """Write one CE dropdown list per discovered language.

    ``marks[idx]``, when given, is prefixed to that row's name — the
    ``(DLC1)`` tag on a DLC's own items.

    For each item, the localized name comes from paloc 0x70 first, and
    falls back to the item's `string_key` (the internal id used by the
    game itself when no localized name exists — e.g. "TestShield",
    "LightSaber_TwoHandSword"). English additionally falls back to
    "Unknown" so output.txt always has one line per key. Other languages
    skip a row only when both paloc and string_key are missing.
    """
    print("-" * 60)
    print("Step 4: CE dropdown lists")
    string_keys = string_keys or {}

    if not paloc:
        print("  no paloc data -> nothing to write")
        return

    # Always emit English first if present, so output.txt is the anchor.
    lang_order = (["eng"] if "eng" in paloc else []) + sorted(
        l for l in paloc if l != "eng"
    )

    for lang in lang_order:
        lang_map = paloc[lang]
        is_eng = (lang == "eng")
        out_name = _output_filename(lang)

        lines: list[str] = []
        from_paloc = 0
        from_string_key = 0
        unknown = 0
        for idx, key in enumerate(keys):
            sk = string_keys.get(key)
            if key in lang_map:
                name = lang_map[key]
                from_paloc += 1
            elif sk:
                name = sk
                from_string_key += 1
            elif is_eng:
                name = "Unknown"
                unknown += 1
            else:
                continue
            mark = marks[idx] if marks else ""
            lines.append(f"{idx}:{mark}{name}/{key}")

        (out_dir / out_name).write_text("\n".join(lines), encoding="utf-8")

        bits = [f"paloc: {from_paloc}", f"string_key: {from_string_key}"]
        if is_eng:
            bits.append(f"Unknown: {unknown}")
        print(f"  [{lang:<6}] {out_name:<22} {len(lines):>6} lines  "
              f"({', '.join(bits)})")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--game-dir", help="Crimson Desert install path")
    ap.add_argument(
        "--out",
        default=str(Path(__file__).resolve().parent.parent / "out"),
        help="output directory (default: <repo>/out)",
    )
    ap.add_argument(
        "--order",
        choices=("header", "keys"),
        default="header",
        help="where the CE index order comes from. 'header' (default): the "
        "base iteminfo header, then each DLC's rows whose keys the base "
        "lacks; --keys is only cross-checked. 'keys': keys.txt is the order "
        "(base table only, anchored; lossy parsing with --keys '').",
    )
    ap.add_argument(
        "--no-dlc",
        action="store_true",
        help="leave DLC items out (header order) and skip DLC item names",
    )
    ap.add_argument(
        "--keys",
        default=str(Path(__file__).resolve().parent.parent / "data" / "keys.txt"),
        help="path to keys.txt (CE-dumped item key order). Defaults to "
        "<repo>/data/keys.txt. Header order: optional cross-check of the base "
        "segment (or base + DLC, if that long). Keys order: the order itself; "
        "pass --keys '' for lossy parsing.",
    )
    ap.add_argument(
        "--dlc-keys",
        default="",
        help="header order only: a CE-dumped DLC segment (e.g. "
        "data/keys-2.05-dlc_01.txt) to cross-check against the DLC-only rows",
    )
    args = ap.parse_args()

    game_dir = find_game_dir(args.game_dir)
    out_dir = Path(args.out).resolve()
    out_dir.mkdir(parents=True, exist_ok=True)
    print(f"Game dir : {game_dir}")
    print(f"Output   : {out_dir}")

    keys_arg = args.keys.strip() if args.keys else ""
    marks: list[str] | None = None
    if args.order == "header":
        # DLC data is checked up front: if any DLC file is missing or
        # unreadable, the whole export falls back to the base game alone.
        print("-" * 60)
        print("Step 1: DLC data")
        dlc_tables: list[DlcTable] = []
        if args.no_dlc:
            print("  skipped (--no-dlc)")
        else:
            try:
                dlc_tables = load_dlc_tables(game_dir)
            except DlcUnavailable as exc:
                print(f"  WARNING: {exc} -> exporting base items only")
            if dlc_tables:
                print("  found: " + ", ".join(
                    f"{t.name} ({len(t.rows):,} iteminfo rows)" for t in dlc_tables))
            elif not args.no_dlc and not discover_dlc_roots(game_dir):
                print("  none installed -> base items only")

        paloc = export_paloc(game_dir, out_dir)
        # DLC item names live in each DLC's own PALOC files; every language
        # the base ships must be there, or the DLC is dropped like above.
        # Base names win on keys both name (identical on 2.05).
        dlc_names: list[dict[str, dict[int, str]]] = []
        try:
            for t in dlc_tables:
                dlc_names.append(export_paloc(
                    t.root, out_dir, tag=f".{t.name}", required_langs=set(paloc)))
        except DlcUnavailable as exc:
            print(f"  WARNING: {exc} -> exporting base items only")
            dlc_tables, dlc_names = [], []
        for names_by_lang in dlc_names:
            for lang, names in names_by_lang.items():
                merged = paloc.setdefault(lang, {})
                for k, v in names.items():
                    merged.setdefault(k, v)

        items, order, base_count = export_iteminfo_header(game_dir, out_dir, dlc_tables)
        keys = [r.key for r in order]
        marks = [dlc_mark(r.source) for r in order]
        n_dlc = len(order) - base_count
        key_source = (f"iteminfo headers ({base_count:,} base"
                      + (f" + {n_dlc:,} DLC" if n_dlc else "") + ")")
        print("-" * 60)
        print("Cross-check against CE dumps (informational)")
        checked = False
        if keys_arg and Path(keys_arg).is_file():
            n_file = len(load_keys_file(Path(keys_arg)))
            whole = n_file > base_count and n_dlc
            corroborate_keys(
                Path(keys_arg).name, Path(keys_arg),
                keys if whole else keys[:base_count],
                "base + DLC order" if whole else "base segment",
            )
            checked = True
        if args.dlc_keys:
            p = Path(args.dlc_keys)
            if not p.is_file():
                sys.exit(f"--dlc-keys not found: {p}")
            if n_dlc:
                corroborate_keys(p.name, p, keys[base_count:], "DLC-only segment")
            else:
                print(f"  {p.name}: no DLC items exported -> not compared")
            checked = True
        if not checked:
            print("  no keys file -> skipped (the header order needs none)")
    else:
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
    write_ce_lists(out_dir, keys, paloc, string_keys, marks)

    print("-" * 60)
    print(f"Done. All files written to: {out_dir}")


if __name__ == "__main__":
    main()
