"""Update the Crimson Desert Cheat Engine table after a game patch.

Run `export_for_ce.py` first; this script reads what it wrote to `out/`
and rewrites the CE table (`CrimsonDesert.CT`) in place:

1. **Dropdown lists.** Replaces the bodies of the `ItemID.`,
   `ItemID-zh_tw.` and `ItemID-ja.` DropDownList records with
   `out/output.txt`, `out/output_zh-tw.txt` and `out/output_ja.txt`.
2. **Item defines.** Every `define(Name, #<CE ID>) // ...name/<item key>`
   line in every AA script gets its CE ID recomputed from the item key at
   the end of its comment (the last `/<digits>`). The comment is the source
   of truth; when it disagrees with the old ID, the report says so.
3. **Range check.** For every ID range a script uses — `dq A, B, ...` where
   both are item defines, or a `cmp ..., A` / `jb` / `cmp ..., B` / `ja`
   pair — the report lists the items that entered or left the range between
   the old and the new order (compared by item key), and the items inside it
   that the data table protects.
4. **Item data table.** Writes (or replaces) the AA record
   `Item data table (auto-generated, do not edit)`: per CE ID, the item's
   max stack and a flag byte, which the amount scripts read. Flags:
   1 = Quest inventory tab (InventoryKey 5), 2 = Kuku tab (InventoryKey 13),
   4 = no properties found for this item.

The old CE ID -> key mapping comes from the table itself (the current
`ItemID.` list carries `/<key>` on every line), so no previous export is
needed. A backup of the table goes to `out/ce-backup/` and the report to
`out/ce_update_report.txt` (plus `out/ce_item_data.tsv`, one row per item).

Per-item properties (max stack, inventory tab) come from `out/items.jsonl`.
When the iteminfo parser cannot read the current patch, `items.jsonl` holds
only keys; `--props-bin <dir>` then supplies them from an archived version
(`<dir>/iteminfo.staticinfobody` plus `<dir>/<dlc>/iteminfo.<dlc>.staticinfobody`),
matched by item key. Base rows win over DLC rows for the same key, as in
the game's own key array. The archived tables are read with the installed
`crimson_rs`, which reads the newest layout only: `<prev>` must be a version
that build still parses, i.e. the last patch, before the parser moves on.

Usage:
    python scripts/update_ce_table.py --ct "D:/Github/My-Cheat-Tables/Crimson Desert/CrimsonDesert.CT"
        [--out out] [--props-bin "<archive>/<prev>"]
        [--game-dir PATH] [--dry-run]
"""

from __future__ import annotations

import argparse
import datetime as _dt
import json
import os
import re
import shutil
import sys
from dataclasses import dataclass, field
from pathlib import Path
from xml.sax.saxutils import escape as _xml_escape

# InventoryKey values from the `inventory` gamedata table.
INV_QUEST = 5
INV_KUKU = 13

FLAG_QUEST = 1
FLAG_KUKU = 2
FLAG_UNKNOWN = 4
FLAG_PROTECT = FLAG_QUEST | FLAG_KUKU | FLAG_UNKNOWN

# Max-stack values at or above this are written as 0 (= no limit); the
# money-tab items read 9,000,000,000,000,000,000.
UNLIMITED_STACK = 0xFFFFFFFF

DATA_DESC = "Item data table (auto-generated, do not edit) 道具資料表"
DATA_DESC_PREFIX = "Item data table (auto-generated"

DROPDOWNS = [
    ("ItemID.", "output.txt"),
    ("ItemID-zh_tw.", "output_zh-tw.txt"),
    ("ItemID-ja.", "output_ja.txt"),
]

LIST_LINE = re.compile(r"^(\d+):(.*)/(\d+)\s*$")
DEFINE_LINE = re.compile(
    r"^(?P<head>[ \t]*(?://[ \t]*)?define\([ \t]*(?P<name>\w+)[ \t]*,[ \t]*)"
    r"#(?P<id>\d+)(?P<tail>[ \t]*\).*)$",
    re.M,
)
KEY_IN_COMMENT = re.compile(r"/(\d+)")


# ---------------------------------------------------------------- inputs


@dataclass
class Item:
    idx: int
    key: int
    name_en: str
    name_tw: str
    max_stack: int | None = None  # None = unknown
    inv0: int | None = None
    importance: int | None = None

    @property
    def flags(self) -> int:
        if self.max_stack is None or self.inv0 is None:
            return FLAG_UNKNOWN
        f = 0
        if self.inv0 == INV_QUEST:
            f |= FLAG_QUEST
        if self.inv0 == INV_KUKU:
            f |= FLAG_KUKU
        return f

    @property
    def stack_word(self) -> int:
        """Max stack as the data table stores it: 0 = unlimited or unknown."""
        if self.max_stack is None or self.max_stack >= UNLIMITED_STACK:
            return 0
        return self.max_stack


def read_list(path: Path) -> list[tuple[int, str, int]]:
    rows = []
    for line in path.read_text(encoding="utf-8").splitlines():
        m = LIST_LINE.match(line)
        if m:
            rows.append((int(m[1]), m[2], int(m[3])))
    return rows


def load_items(out_dir: Path) -> tuple[list[Item], dict[str, list[tuple[int, str, int]]]]:
    lists = {desc: read_list(out_dir / fname) for desc, fname in DROPDOWNS}
    en = lists["ItemID."]
    for desc, rows in lists.items():
        if [r[0] for r in rows] != list(range(len(rows))):
            sys.exit(f"{desc}: the list is not numbered 0..N-1 without gaps")
        if [r[2] for r in rows] != [r[2] for r in en]:
            sys.exit(f"{desc}: keys differ from output.txt - re-run export_for_ce.py")
    tw = {r[0]: r[1] for r in lists["ItemID-zh_tw."]}
    items = [Item(i, k, n, tw[i]) for i, n, k in en]

    jsonl = out_dir / "items.jsonl"
    if jsonl.exists():
        by_idx = {it.idx: it for it in items}
        for line in jsonl.read_text(encoding="utf-8").splitlines():
            rec = json.loads(line)
            it = by_idx.get(rec.get("_index"))
            if it is None or it.key != rec.get("key"):
                continue
            if "max_stack_count" in rec and rec.get("inventory_info_list"):
                it.max_stack = rec["max_stack_count"]
                it.inv0 = rec["inventory_info_list"][0]
                it.importance = rec.get("item_importance_kind")
    return items, lists


def props_from_bin(root: Path) -> dict[int, tuple[int, int, int | None]]:
    """key -> (max_stack, inventory slot 0, importance) from an archived
    version: base table first, then DLC rows for keys the base lacks."""
    import crimson_rs

    def body(p: Path) -> bytes | None:
        for cand in (Path(str(p) + ".staticinfobody"), Path(str(p) + ".pabgb")):
            if cand.exists():
                return cand.read_bytes()
        return None

    out: dict[int, tuple[int, int, int | None]] = {}
    tables = [root / "iteminfo"]
    for sub in sorted(p for p in root.iterdir() if p.is_dir()):
        tables.append(sub / f"iteminfo.{sub.name}")
    for t in tables:
        data = body(t)
        if data is None:
            continue
        for r in crimson_rs.parse_iteminfo_from_bytes(data):
            out.setdefault(
                r["key"],
                (
                    r["max_stack_count"],
                    # Guarded as on the other path: a list can be empty.
                    r["inventory_info_list"][0] if r["inventory_info_list"] else 0,
                    r.get("item_importance_kind"),
                ),
            )
    if not out:
        sys.exit(f"--props-bin: no iteminfo table found under {root}")
    return out


def read_paver(game_dir: str | None) -> str:
    env_root = os.environ.get("CRIMSON_GAME_ROOT")
    candidates = [game_dir] if game_dir else ([env_root] if env_root else []) + [
        r"D:\SteamLibrary\steamapps\common\Crimson Desert",
        r"C:\Program Files (x86)\Steam\steamapps\common\Crimson Desert",
    ]
    for g in candidates:
        try:
            b = (Path(g) / "meta" / "0.paver").read_bytes()
        except OSError:
            continue
        if len(b) >= 10:
            major, minor, patch = (int.from_bytes(b[i:i + 2], "little") for i in (0, 2, 4))
            return f"{major}/{minor}/{patch}"
    return "unknown"


# ---------------------------------------------------------------- CT text


def xml_text(s: str) -> str:
    return _xml_escape(s)  # & < >


def xml_unescape(s: str) -> str:
    return s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", '"').replace("&amp;", "&")


def dropdown_span(ct: str, desc: str) -> tuple[int, int]:
    m = re.search(
        r'<Description>"' + re.escape(desc) + r'"</Description>\s*<DropDownList[^>]*>',
        ct,
    )
    if not m:
        sys.exit(f'CT: no DropDownList record named "{desc}"')
    end = ct.index("</DropDownList>", m.end())
    return m.end(), end


def old_mapping(ct: str) -> dict[int, int]:
    s, e = dropdown_span(ct, "ItemID.")
    mapping = {}
    for line in ct[s:e].split("\n"):
        m = LIST_LINE.match(xml_unescape(line))
        if m:
            mapping[int(m[1])] = int(m[3])
    return mapping


SCRIPT_RE = re.compile(
    r"<CheatEntry>\s*<ID>(?P<id>\d+)</ID>\s*<Description>\"(?P<desc>(?:(?!</Description>).)*)\"</Description>"
    r"(?:(?!<CheatEntry>).)*?<AssemblerScript[^>]*>(?P<body>.*?)</AssemblerScript>",
    re.S,
)


@dataclass
class Report:
    lines: list[str] = field(default_factory=list)
    warnings: int = 0

    def h(self, title: str) -> None:
        self.lines += ["", "=" * 72, title, "=" * 72]

    def add(self, s: str = "") -> None:
        self.lines.append(s)

    def warn(self, s: str) -> None:
        self.warnings += 1
        self.lines.append("  [!] " + s)


def rewrite_defines(
    body: str, old_map: dict[int, int], key_to_new: dict[int, int], rep: Report, label: str
) -> tuple[str, dict[str, tuple[int, int, int]]]:
    """Returns the new body and name -> (key, old id, new id)."""
    names: dict[str, tuple[int, int, int]] = {}

    def sub(m: re.Match) -> str:
        name, old_id, tail = m["name"], int(m["id"]), m["tail"]
        keys = KEY_IN_COMMENT.findall(tail.split("//", 1)[1]) if "//" in tail else []
        if not keys:
            return m.group(0)  # not an item define (no /<key> in its comment)
        key = int(keys[-1])
        if old_map.get(old_id) != key:
            rep.warn(
                f"{label}: define {name} #{old_id} 原本指到 key {old_map.get(old_id)}，"
                f"註解寫的是 key {key} → 依註解修正"
            )
        new_id = key_to_new.get(key)
        if new_id is None:
            rep.warn(f"{label}: define {name} 的 key {key} 在新版找不到（道具被移除？）→ 保留 #{old_id}")
            names[name] = (key, old_id, old_id)
            return m.group(0)
        names[name] = (key, old_id, new_id)
        if new_id != old_id:
            rep.add(f"  {label}: {name}  #{old_id} → #{new_id}  (key {key})")
        return f"{m['head']}#{new_id}{tail}"

    return DEFINE_LINE.sub(sub, body), names


def ranges_in(body: str, names: dict[str, tuple[int, int, int]]) -> list[tuple[str, str, bool]]:
    """Item ID ranges a script uses, as (first define, last define, forced).
    `forced` marks a `dq start, end, amount, 1` table row: a 4th field of 1
    applies the range to protected items too."""
    plain = xml_unescape(body)
    pairs: list[tuple[str, str, bool]] = []
    for line in plain.split("\n"):
        code = line.split("//", 1)[0]
        m = re.match(r"\s*dq\s+(\w+)\s*,\s*(\w+)\b(.*)", code)
        if m and m[1] in names and m[2] in names:
            rest = [x.strip() for x in m[3].split(",")]
            forced = len(rest) >= 3 and rest[2] == "1"
            pairs.append((m[1], m[2], forced))
    cmp_pat = re.compile(
        r"cmp\s+[^,\n]+,\s*(\w+)\s*\n(?:\s*//[^\n]*\n)*\s*jb[^\n]*\n"
        r"(?:\s*//[^\n]*\n)*\s*cmp\s+[^,\n]+,\s*(\w+)\s*\n(?:\s*//[^\n]*\n)*\s*ja"
    )
    for m in cmp_pat.finditer(plain):
        if m[1] in names and m[2] in names:
            pairs.append((m[1], m[2], False))
    seen, out = set(), []
    for p in pairs:
        if p not in seen:
            seen.add(p)
            out.append(p)
    return out


def check_ranges(
    label: str,
    pairs: list[tuple[str, str, bool]],
    names: dict[str, tuple[int, int, int]],
    old_map: dict[int, int],
    items: list[Item],
    rep: Report,
) -> None:
    by_key = {it.key: it for it in items}
    for a, b, forced in pairs:
        _, oa, na = names[a]
        _, ob, nb = names[b]
        rep.add(f"  {label}: {a} .. {b}   舊 #{oa}-#{ob} → 新 #{na}-#{nb}")
        if nb < na:
            rep.warn(f"{label}: 區間 {a}..{b} 起點大於終點 (#{na} > #{nb})，這個區間不會生效")
            continue
        new_keys = [items[i].key for i in range(na, nb + 1)]
        if ob < oa:
            rep.warn(f"{label}: 舊區間 {a}..{b} 起點大於終點 (#{oa} > #{ob})，原本不會生效；"
                     f"修正後包含 {len(new_keys)} 個道具 (#{na} {items[na].name_tw} ~ #{nb} {items[nb].name_tw})")
            old_keys = new_keys
        else:
            old_keys = [old_map[i] for i in range(oa, ob + 1) if i in old_map]
        added = [k for k in new_keys if k not in set(old_keys)]
        removed = [k for k in old_keys if k not in set(new_keys)]
        for k in added:
            it = by_key[k]
            rep.add(f"      + 新進區間  #{it.idx} {it.name_tw} / {it.name_en} (key {k})")
        for k in removed:
            it = by_key.get(k)
            where = f"現在是 #{it.idx} {it.name_tw}" if it else "新版已移除"
            rep.add(f"      - 離開區間  key {k} ({where})")
        protected = [items[i] for i in range(na, nb + 1)
                     if items[i].flags & FLAG_PROTECT or items[i].stack_word == 1]
        if protected and forced:
            rep.add(f"      第 4 欄 = 1 (強制套用)：其中 {len(protected)} 個受保護的道具照樣會被改")
        elif protected and len(protected) == nb - na + 1:
            rep.add(f"      區間內 {len(protected)} 個道具全部受保護 (任務 / Kuku / 堆疊1 / 屬性未知)")
        elif protected:
            rep.add(f"      保護中、不會被這個區間改到的道具 {len(protected)} 個"
                    f" (要照改就把這一行第 4 欄設成 1):")
            for it in protected:
                why = []
                if it.flags & FLAG_QUEST:
                    why.append("任務")
                if it.flags & FLAG_KUKU:
                    why.append("Kuku")
                if it.flags & FLAG_UNKNOWN:
                    why.append("屬性未知")
                if it.stack_word == 1:
                    why.append("堆疊1")
                rep.add(f"        #{it.idx} {it.name_tw} [{'/'.join(why)}]")


# ---------------------------------------------------------------- data table


def comment_name(s: str) -> str:
    """An item name made safe for an AA `//` comment inside XML."""
    s = s.replace("{", "(").replace("}", ")")
    return xml_text(s)


def build_data_script(items: list[Item], paver: str, source: str) -> str:
    n = len(items)
    n_quest = sum(1 for it in items if it.flags & FLAG_QUEST)
    n_kuku = sum(1 for it in items if it.flags & FLAG_KUKU)
    n_unknown = sum(1 for it in items if it.flags & FLAG_UNKNOWN)
    n_stack1 = sum(1 for it in items if it.stack_word == 1)
    n_unlim = sum(1 for it in items if it.max_stack is not None and it.max_stack >= UNLIMITED_STACK)
    size = 0x10 + ((4 * n + 0xF) & ~0xF) + n
    alloc = (size + 0x1000 + 0xFFF) & ~0xFFF  # one spare page for growth
    now = _dt.datetime.now().strftime("%Y-%m-%d %H:%M")

    out: list[str] = []
    w = out.append
    w("{")
    w("  ============================================================")
    w("  Item data table (auto-generated, do not edit) / 道具資料表 (自動產生，勿手改)")
    w("  ------------------------------------------------------------")
    w("  Generator / 產生工具 : crimson-rs  scripts/update_ce_table.py")
    w(f"  Generated / 產生時間 : {now}")
    w(f"  Game      / 遊戲版本 : paver {paver}")
    w(f"  Items     / 道具總數 : {n}  (CE item ID 0 ~ {n - 1})")
    w(f"  Source    / 屬性來源 : {source}")
    w("  ------------------------------------------------------------")
    w('  "fixed amount" and "When item amount changes" share this table to decide')
    w("  whether an item is protected (amount left alone) and to read its stack limit.")
    w("  Those scripts assemble this record themselves: it never needs to be ticked")
    w("  and can sit in any group.")
    w('  "fixed amount" 和 "When item amount changes" 共用這份表，判斷道具要不要「保護」')
    w("  (完全不改數量) 以及它的堆疊上限。兩支 script 會自己載入這一項，不需要勾選，")
    w("  放在哪個群組都可以。")
    w("")
    w("  Lookup (ID = CE item ID, the word-sized item number in game memory):")
    w("  查表方式 (ID = CE 道具 ID):")
    w("    item_tbl_count          = number of items in the table / 表格內的道具總數")
    w("                              ID >= this: not in the table (added by a newer patch),")
    w("                              both scripts leave it alone / 表格沒有這個道具，一律不介入")
    w("    item_max_stack + ID*4   = stack limit (dword) / 堆疊上限")
    w("                              1 = does not stack / 不能堆疊；0 = no limit / 無上限")
    w("    item_flags + ID         = flags (byte), may combine / 旗標，可以同時有好幾個:")
    w("                              1 = quest item (Quest tab) / 任務道具")
    w("                              2 = Kuku puzzle part (Kuku tab) / Kuku 解謎零件")
    w("                              4 = properties unknown, treated as protected / 屬性未知")
    w("")
    w(f"  Counts / 統計: quest {n_quest}, Kuku {n_kuku}, stack limit 1: {n_stack1}, "
      f"no limit: {n_unlim}, unknown: {n_unknown}")
    w("  Per-item list / 逐筆清單: crimson-rs out/ce_item_data.tsv")
    w("  ============================================================")
    w("}")
    w("[ENABLE]")
    w(f"alloc(item_data,${alloc:X})")
    w("label(item_tbl_count item_max_stack item_flags)")
    w("registersymbol(item_tbl_count item_max_stack item_flags)")
    w("")
    w("item_data:")
    w("item_tbl_count:")
    w(f"  dd #{n}")
    w("  align 10 0")
    w("")
    w("item_max_stack:")
    w("// ---- stack limit table BEGIN: 16 items per line; comment = ID range + first item ----")
    w("// ---- 堆疊上限表 BEGIN：每行 16 個道具，行尾註解 = 這一行的 ID 範圍與第一個道具 ----")
    for s in range(0, n, 16):
        chunk = items[s:s + 16]
        vals = ",".join(f"#{it.stack_word}" for it in chunk)
        w(f"  dd {vals}  // #{s}-#{s + len(chunk) - 1} {comment_name(chunk[0].name_tw)}")
    w("// ---- stack limit table END / 堆疊上限表 END ----")
    w("")
    w("item_flags:")
    w("// ---- flag table BEGIN: 32 items per line (1=quest 2=Kuku 4=unknown) ----")
    w("// ---- 旗標表 BEGIN：每行 32 個道具 (1=任務 2=Kuku 4=屬性未知) ----")
    for s in range(0, n, 32):
        chunk = items[s:s + 32]
        vals = ",".join(str(it.flags) for it in chunk)
        w(f"  db {vals}  // #{s}-#{s + len(chunk) - 1} {comment_name(chunk[0].name_tw)}")
    w("// ---- flag table END / 旗標表 END ----")
    w("")
    w("[DISABLE]")
    w("// Only reached if someone ticks and unticks this record. The memory is never")
    w("// freed: the two scripts have this block's address baked into their code, and")
    w("// freeing it while they run would crash the game.")
    w("// 只有手動勾選再取消時才會走到這裡。記憶體不釋放：兩支 script 的程式碼裡已經")
    w("// 寫死這塊記憶體的位址，在它們還開著時釋放會讓遊戲當掉。")
    w("unregistersymbol(item_tbl_count item_max_stack item_flags)")
    return "\n".join(out) + "\n"


def upsert_data_entry(ct: str, script: str, rep: Report) -> str:
    m = re.search(
        r'<Description>"' + re.escape(DATA_DESC_PREFIX) + r'[^"]*"</Description>'
        r"(?:(?!<CheatEntry>).)*?<AssemblerScript[^>]*>(.*?)</AssemblerScript>",
        ct,
        re.S,
    )
    if m:
        rep.add("  已更新既有的道具資料表")
        return ct[:m.start(1)] + script + ct[m.end(1):]

    new_id = max(int(x) for x in re.findall(r"<ID>(\d+)</ID>", ct)) + 1

    def entry_at(i: str) -> str:
        # Only the XML lines are indented; the script text goes in verbatim.
        return (
            f"{i}<CheatEntry>\n"
            f"{i}  <ID>{new_id}</ID>\n"
            f'{i}  <Description>"{DATA_DESC}"</Description>\n'
            f"{i}  <Color>808080</Color>\n"
            f"{i}  <VariableType>Auto Assembler Script</VariableType>\n"
            f"{i}  <AssemblerScript>{script}</AssemblerScript>\n"
            f"{i}</CheatEntry>\n"
        )

    # First choice: inside the table's hidden top-level group of helper records
    # ("Crimson Desert  /  https://opencheattables.com  /  CE 7.6+"), where the
    # dropdown lists live. The record never needs ticking, so hiding it is fine.
    group = re.search(
        r'\n    <CheatEntry>\s*<ID>\d+</ID>\s*<Description>"Crimson Desert  /[^"]*"</Description>'
        r"(?:(?!<CheatEntry>).)*?\n      <CheatEntries>\n",
        ct,
        re.S,
    )
    anchor = re.search(r'\n(    <CheatEntry>\s*<ID>\d+</ID>\s*<Description>"fixed amount:)', ct)
    entry = entry_at("    ")
    if group:
        pos = group.end()
        entry = entry_at("        ")
    elif anchor:
        pos = anchor.start(1)
    else:
        pos = ct.rindex("\n  </CheatEntries>") + 1
    rep.add(f"  新增道具資料表 (ID {new_id})")
    return ct[:pos] + entry + ct[pos:]


# ---------------------------------------------------------------- main


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--ct", required=True, help="path to CrimsonDesert.CT")
    ap.add_argument("--out", default="out", help="export_for_ce.py output folder (default: out)")
    ap.add_argument("--props-bin", help="archived version folder to take item properties from")
    ap.add_argument("--game-dir", help="install path (only for the paver stamp)")
    ap.add_argument("--dry-run", action="store_true", help="write the report only")
    args = ap.parse_args()

    out_dir = Path(args.out)
    ct_path = Path(args.ct)
    raw = ct_path.read_bytes().decode("utf-8")
    crlf = "\r\n" in raw
    if crlf and raw.count("\n") != raw.count("\r\n"):
        sys.exit("CT mixes CRLF and LF line endings; refusing to rewrite it")
    ct = raw.replace("\r\n", "\n")

    items, lists = load_items(out_dir)
    source = "out/items.jsonl"
    missing = [it for it in items if it.max_stack is None]
    if missing and args.props_bin:
        props = props_from_bin(Path(args.props_bin))
        filled = 0
        for it in missing:
            p = props.get(it.key)
            if p:
                it.max_stack, it.inv0, it.importance = p
                filled += 1
        source = f"{args.props_bin} (依 key 對應；items.jsonl 沒有屬性的 {filled} 筆)"
        if filled < len(missing):
            source += f"，仍有 {len(missing) - filled} 筆未知"
    elif missing:
        source = f"out/items.jsonl (其中 {len(missing)} 筆沒有屬性，標為屬性未知)"

    rep = Report()
    paver = read_paver(args.game_dir)
    rep.add(f"CE table update  {_dt.datetime.now():%Y-%m-%d %H:%M}  paver {paver}")
    rep.add(f"CT    : {ct_path}")
    rep.add(f"items : {len(items)}   properties: {source}")

    old_map = old_mapping(ct)
    key_to_new = {it.key: it.idx for it in items}
    moved = sum(1 for i, k in old_map.items() if key_to_new.get(k) != i)
    rep.add(f"old list: {len(old_map)} items, {moved} of them at a different CE ID now")

    # 1. defines + ranges, per AA script
    rep.h("1. Item defines (依註解中的 key 重算 CE ID)")
    all_names: list[tuple[str, dict]] = []
    pieces: list[str] = []
    last = 0
    for m in SCRIPT_RE.finditer(ct):
        label = f"[{m['id']}] {xml_unescape(m['desc'])[:40]}"
        new_body, names = rewrite_defines(m["body"], old_map, key_to_new, rep, label)
        if names:
            all_names.append((label, names, new_body))
        pieces.append(ct[last:m.start("body")])
        pieces.append(new_body)
        last = m.end("body")
    pieces.append(ct[last:])
    ct = "".join(pieces)
    rep.add(f"  ({sum(len(n) for _, n, _ in all_names)} item defines in {len(all_names)} scripts)")

    rep.h("2. 區間檢查 (新舊版的區間成員比對，以 key 為準)")
    for label, names, body in all_names:
        check_ranges(label, ranges_in(body, names), names, old_map, items, rep)

    # 3. dropdowns
    rep.h("3. Dropdown lists")
    for desc, _ in DROPDOWNS:
        s, e = dropdown_span(ct, desc)
        rows = lists[desc]
        body = "\n".join(f"{i}:{xml_text(n)}/{k}" for i, n, k in rows) + "\n"
        changed = "unchanged" if ct[s:e] == body else "replaced"
        ct = ct[:s] + body + ct[e:]
        rep.add(f"  {desc:<15} {len(rows)} lines, {changed}")

    # 4. data table
    rep.h("4. 道具資料表")
    script = build_data_script(items, paver, source)
    ct = upsert_data_entry(ct, xml_text(script), rep)
    by_flag = {f: sum(1 for it in items if it.flags & f) for f in (FLAG_QUEST, FLAG_KUKU, FLAG_UNKNOWN)}
    rep.add(f"  任務 {by_flag[FLAG_QUEST]}  Kuku {by_flag[FLAG_KUKU]}  屬性未知 {by_flag[FLAG_UNKNOWN]}  "
            f"堆疊1 {sum(1 for it in items if it.stack_word == 1)}")

    with (out_dir / "ce_item_data.tsv").open("w", encoding="utf-8", newline="\n") as fh:
        fh.write("ce_id\tkey\tmax_stack\tflags\tquest\tkuku\tunknown\tname_zh_tw\tname_en\n")
        for it in items:
            f = it.flags
            fh.write(f"{it.idx}\t{it.key}\t{'' if it.max_stack is None else it.max_stack}\t{f}\t"
                     f"{int(bool(f & FLAG_QUEST))}\t{int(bool(f & FLAG_KUKU))}\t"
                     f"{int(bool(f & FLAG_UNKNOWN))}\t{it.name_tw}\t{it.name_en}\n")

    report_path = out_dir / "ce_update_report.txt"
    report_path.write_text("\n".join(rep.lines) + "\n", encoding="utf-8")

    if not args.dry_run:
        backup_dir = out_dir / "ce-backup"
        backup_dir.mkdir(exist_ok=True)
        backup = backup_dir / f"{ct_path.stem}.{_dt.datetime.now():%Y%m%d-%H%M%S}{ct_path.suffix}"
        shutil.copy2(ct_path, backup)
        text = ct.replace("\n", "\r\n") if crlf else ct
        ct_path.write_bytes(text.encode("utf-8"))
        print(f"CT written ({ct_path}); backup: {backup}")
    else:
        print("dry run: CT not written")
    print(f"report: {report_path}  ({rep.warnings} warning(s))")
    print(f"items : {out_dir / 'ce_item_data.tsv'}")


if __name__ == "__main__":
    main()
