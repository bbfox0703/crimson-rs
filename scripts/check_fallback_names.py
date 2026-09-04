"""For items that lack a 0x70 paloc entry, list the type-0x30 fallback
side by side with the string_key so we can sanity-check whether 0x30
is a usable fallback name."""

from __future__ import annotations
import argparse
import json
from pathlib import Path

import crimson_rs

from gamedata_layout import paloc_entries


PALOC_GROUPS = [f"{n:04d}" for n in range(20, 36)]


def load_paloc(game_dir: str, lang: str) -> dict[tuple[int, int], str]:
    entries = paloc_entries(game_dir, PALOC_GROUPS, lang)
    out: dict[tuple[int, int], str] = {}
    for e in entries:
        try:
            sid = int(e["string_key"])
        except (ValueError, TypeError):
            continue
        ik = sid >> 32
        tb = sid & 0xFF
        out[(ik, tb)] = e["string_value"]
    return out


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--game-dir", required=True)
    ap.add_argument("--unknown", required=True)
    args = ap.parse_args()

    eng = load_paloc(args.game_dir, "eng")
    zh = load_paloc(args.game_dir, "zho-tw")
    ja = load_paloc(args.game_dir, "jpn")

    unknown = json.loads(Path(args.unknown).read_text(encoding="utf-8"))
    print(f"{'i':>5} {'key':<10}  string_key                                | en 0x30 | zh 0x30 | ja 0x30")
    print("-" * 130)

    for r in unknown:
        ik = r["key"]
        sk = r["string_key"]
        en30 = eng.get((ik, 0x30)) or ""
        zh30 = zh.get((ik, 0x30)) or ""
        ja30 = ja.get((ik, 0x30)) or ""
        # truncate long names for display
        def trunc(s: str, n: int = 20) -> str:
            return s if len(s) <= n else s[:n - 1] + "…"
        print(
            f"{r['i']:>5} {ik:<10}  {sk[:40]:<40} | {trunc(en30):<20} | {trunc(zh30):<20} | {trunc(ja30):<20}"
        )

    # how many of the 71 have a non-empty zh 0x30 fallback?
    zh_30_n = sum(1 for r in unknown if zh.get((r["key"], 0x30)))
    ja_30_n = sum(1 for r in unknown if ja.get((r["key"], 0x30)))
    en_30_n = sum(1 for r in unknown if eng.get((r["key"], 0x30)))
    print()
    print(f"missing 0x70 but has 0x30 — eng:{en_30_n}/{len(unknown)} zh:{zh_30_n}/{len(unknown)} ja:{ja_30_n}/{len(unknown)}")


if __name__ == "__main__":
    main()
