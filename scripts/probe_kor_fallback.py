"""For each item missing paloc 0x70 in eng/zho-tw/jpn, check whether
Korean (kor) paloc has it. If kor has 0x70 entries for these dev/test
items, that is what the in-game UI displays when the player's language
file lacks an entry — the game falls back to the source-language string."""

from __future__ import annotations
import argparse
import json
import sys
from pathlib import Path

import crimson_rs


PALOC_GROUPS = [f"{n:04d}" for n in range(19, 36)]
PALOC_DIR = "gamedata/stringtable/binary__"


def load_paloc_70(game_dir: str, lang: str) -> tuple[dict[int, str], int]:
    fname = f"localizationstring_{lang}.paloc"
    raw = None
    hit_group = None
    for g in PALOC_GROUPS:
        try:
            raw = bytes(crimson_rs.extract_file(game_dir, g, PALOC_DIR, fname))
            hit_group = g
            break
        except Exception:
            continue
    if raw is None:
        return {}, -1
    out: dict[int, str] = {}
    for e in crimson_rs.parse_paloc_bytes(raw):
        try:
            sid = int(e["string_key"])
        except (ValueError, TypeError):
            continue
        if sid & 0xFF == 0x70:
            v = e["string_value"]
            if v and v != "[EMPTY]":
                out[sid >> 32] = v
    return out, len(raw)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--game-dir", required=True)
    ap.add_argument("--unknown", required=True)
    args = ap.parse_args()
    sys.stdout.reconfigure(encoding="utf-8")  # type: ignore[attr-defined]

    unknown = json.loads(Path(args.unknown).read_text(encoding="utf-8"))
    target_keys = {r["key"] for r in unknown}

    for lang in ("kor", "chs", "rus", "deu", "fra"):
        paloc70, size = load_paloc_70(args.game_dir, lang)
        if size < 0:
            print(f"  [{lang:<6}] NOT FOUND")
            continue
        hits = {k: paloc70[k] for k in target_keys if k in paloc70}
        print(
            f"  [{lang:<6}] paloc {size:>10,}B   "
            f"0x70 entries={len(paloc70):,}   "
            f"hits-on-our-71={len(hits)}"
        )
        for k, v in list(hits.items())[:8]:
            print(f"      key={k:<10} {v!r}")


if __name__ == "__main__":
    main()
