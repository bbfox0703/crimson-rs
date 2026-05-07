"""Cross-reference community item_names.json against paloc, to:
  1. Confirm community names match the official paloc 0x70 names.
  2. For items missing in paloc 0x70 but present in community, search the
     paloc bytes for the community name to discover what type byte
     stores it (i.e. where the game actually keeps that string).
  3. List items that are in the 1.05 itemKey list but missing from BOTH
     paloc 0x70 and community — those are the truly unnamed dev items.
"""

from __future__ import annotations
import argparse
import json
import sys
from collections import Counter, defaultdict
from pathlib import Path

import crimson_rs


PALOC_GROUPS = [f"{n:04d}" for n in range(20, 36)]
PALOC_DIR = "gamedata/stringtable/binary__"


def load_paloc(game_dir: str, lang: str) -> list[tuple[int, int, str]]:
    """Return list of (item_key, type_byte, value) tuples."""
    fname = f"localizationstring_{lang}.paloc"
    raw = None
    for g in PALOC_GROUPS:
        try:
            raw = bytes(crimson_rs.extract_file(game_dir, g, PALOC_DIR, fname))
            break
        except Exception:
            continue
    if raw is None:
        raise SystemExit(fname)
    out: list[tuple[int, int, str]] = []
    for e in crimson_rs.parse_paloc_bytes(raw):
        try:
            sid = int(e["string_key"])
        except (ValueError, TypeError):
            continue
        out.append((sid >> 32, sid & 0xFF, e["string_value"]))
    return out


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--game-dir", required=True)
    ap.add_argument("--community", required=True, help="path to item_names.json")
    ap.add_argument("--keys", required=True, help="path to keys.txt")
    args = ap.parse_args()

    sys.stdout.reconfigure(encoding="utf-8")  # type: ignore[attr-defined]

    community = json.loads(Path(args.community).read_text(encoding="utf-8"))
    com_by_key: dict[int, str] = {it["itemKey"]: it["name"] for it in community["items"]}
    print(f"community: {len(com_by_key):,} items")

    keys_text = Path(args.keys).read_text(encoding="utf-8").splitlines()
    keys_105: list[int] = []
    for ln in keys_text:
        s = ln.strip()
        if not s:
            continue
        try:
            k = int(s)
        except ValueError:
            break
        if k == 0xFFFFFFFF or k == 0:
            break
        keys_105.append(k)
    keyset_105 = set(keys_105)
    print(f"keys.txt (1.05): {len(keys_105):,} items")

    paloc = load_paloc(args.game_dir, "eng")
    print(f"paloc eng: {len(paloc):,} entries")

    # Index paloc: by (key, type_byte) -> value, and by value -> [(key, tb)]
    by_kt: dict[tuple[int, int], str] = {}
    by_value: dict[str, list[tuple[int, int]]] = defaultdict(list)
    for k, tb, v in paloc:
        by_kt[(k, tb)] = v
        if v and v != "[EMPTY]":
            by_value[v].append((k, tb))

    paloc_70: dict[int, str] = {k: v for (k, tb), v in by_kt.items() if tb == 0x70}
    print(f"paloc 0x70 (item names): {len(paloc_70):,}")

    # ── 1. Sanity: does community match paloc 0x70 where both exist? ──
    matches = 0
    diffs: list[tuple[int, str, str]] = []
    only_community = []
    for k, com_name in com_by_key.items():
        official = paloc_70.get(k)
        if official is None:
            only_community.append((k, com_name))
        elif official == com_name:
            matches += 1
        else:
            diffs.append((k, com_name, official))

    print()
    print(f"community ∩ paloc 0x70 — exact match: {matches:,}")
    print(f"community ∩ paloc 0x70 — different : {len(diffs):,}")
    print(f"community only (no paloc 0x70)     : {len(only_community):,}")

    if diffs:
        print(f"\nFirst 8 mismatches (community vs paloc 0x70):")
        for k, com, off in diffs[:8]:
            print(f"  key={k:<10} community={com!r:<35} paloc 0x70={off!r}")

    # ── 2. For community-only items, find the paloc type byte holding the name ──
    if only_community:
        print(f"\nFirst 20 community-only items — searching paloc for that string:")
        type_byte_hits: Counter[int] = Counter()
        for k, com_name in only_community[:60]:
            # check paloc entries on this key under any type byte
            hits = [(tb, v) for (kk, tb), v in by_kt.items() if kk == k and v and v != "[EMPTY]"]
            # also check entries with the SAME value but different keys
            value_hits = by_value.get(com_name, [])
            print(f"  key={k:<10} community={com_name!r}")
            if hits:
                for tb, v in sorted(hits):
                    marker = " <-- match" if v == com_name else ""
                    print(f"    on this key 0x{tb:02X} = {v[:60]!r}{marker}")
                    if v == com_name:
                        type_byte_hits[tb] += 1
            else:
                print("    (no paloc entries for this key)")
            if value_hits:
                # show 1-2 places where this string appears under different keys
                for kk, tb in value_hits[:2]:
                    if kk != k:
                        print(f"    string lives elsewhere: key={kk}, 0x{tb:02X}")

        print(f"\nType-byte hits where community name appears under same key:")
        for tb, n in type_byte_hits.most_common():
            print(f"  0x{tb:02X}: {n}")

    # ── 3. Truly unnamed items (in 1.05 keys.txt, no paloc 0x70, no community) ──
    truly_unnamed = []
    for k in keys_105:
        if k in paloc_70:
            continue
        if k in com_by_key:
            continue
        truly_unnamed.append(k)
    print(f"\n1.05 items missing from BOTH paloc 0x70 AND community: {len(truly_unnamed)}")
    for k in truly_unnamed[:20]:
        print(f"  key={k}")

    # ── 4. 1.05 items where community has a name and paloc 0x70 doesn't ──
    rescue: list[tuple[int, str]] = []
    for k in keys_105:
        if k in paloc_70:
            continue
        if k in com_by_key:
            rescue.append((k, com_by_key[k]))
    print(f"\n1.05 items WITHOUT paloc 0x70 but present in community: {len(rescue)}")
    for k, name in rescue[:30]:
        print(f"  key={k:<10} community={name!r}")


if __name__ == "__main__":
    main()
