"""Find which items have no paloc name in any language, and dump them so
we can search the game data for their localization."""

from __future__ import annotations
import argparse
import json
from collections import Counter
from pathlib import Path


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=str(Path(__file__).resolve().parent.parent / "out"))
    args = ap.parse_args()
    out_dir = Path(args.out)

    items = [json.loads(l) for l in (out_dir / "items.jsonl").read_text(encoding="utf-8").splitlines() if l]
    en = json.loads((out_dir / "paloc_eng.json").read_text(encoding="utf-8"))
    zh = json.loads((out_dir / "paloc_zho-tw.json").read_text(encoding="utf-8"))
    ja = json.loads((out_dir / "paloc_jpn.json").read_text(encoding="utf-8"))

    rows = []
    for it in items:
        k = it["key"]
        sk = str(k)
        if sk in en:
            continue
        rows.append({
            "i": it["_index"],
            "key": k,
            "string_key": it["string_key"],
            "size": it["_anchor_size"],
            "status": it.get("_status", ""),
            "in_zh": sk in zh,
            "in_ja": sk in ja,
            "zh_name": zh.get(sk),
            "ja_name": ja.get(sk),
        })

    print(f"items missing EN paloc name: {len(rows)}")
    print()
    print(f"{'i':>5} {'key':<10} {'size':>4} {'zh':>2} {'ja':>2}  string_key")
    for r in rows:
        zh_mark = "Y" if r["in_zh"] else "."
        ja_mark = "Y" if r["in_ja"] else "."
        print(f"{r['i']:>5} {r['key']:<10} {r['size']:>4}  {zh_mark}  {ja_mark}  {r['string_key']!r}")

    print()
    print("Coverage breakdown for these 71 keys:")
    bm = sum(1 for r in rows if not r["in_zh"] and not r["in_ja"])
    print(f"  no EN, no zh, no ja: {bm}")
    print(f"  no EN, zh only      : {sum(1 for r in rows if r['in_zh'] and not r['in_ja'])}")
    print(f"  no EN, ja only      : {sum(1 for r in rows if r['in_ja'] and not r['in_zh'])}")
    print(f"  no EN, zh AND ja    : {sum(1 for r in rows if r['in_zh'] and r['in_ja'])}")

    # group string_keys to spot patterns
    print()
    print("string_key prefixes among the 71:")
    prefix_counts: Counter[str] = Counter()
    for r in rows:
        sk = r["string_key"]
        prefix = sk.split("_")[0] if "_" in sk else sk
        prefix_counts[prefix] += 1
    for p, n in prefix_counts.most_common():
        print(f"  {p!r:<30} {n}")

    # write the list as JSON for downstream lookup
    with (out_dir / "unknown_items.json").open("w", encoding="utf-8") as fh:
        json.dump(rows, fh, ensure_ascii=False, indent=2)
    print(f"\n-> {out_dir/'unknown_items.json'}")


if __name__ == "__main__":
    main()
