"""One-shot probe: hunt save's self-describing schema for type / field names
that could rename our two hand-named 1.08 iteminfo fields.

Targets:
  - DockingChildData::unk_post_summon_tag (trailing u8 in DockingChildData)
  - ItemInfo::is_equip_quick_slot_visible (u8 between is_housing_only and quick_slot_index)

If the save schema carries a type named "DockingChildData" (or anything close),
the field name is right there. If it carries a "SummonTag" / "EquipQuickSlot" /
"HousingOnly" / "QuickSlotIndex"-bearing field, we can cross-reference.

Run from repo root::

    python scripts/probe_save_schema_for_iteminfo_names.py
"""
from __future__ import annotations
import os
import re
import sys
from pathlib import Path

import crimson_rs as cr


def find_live_save() -> Path | None:
    env = os.environ.get("CRIMSON_LIVE_SAVE")
    if env:
        p = Path(env)
        return p if p.is_file() else None
    root = Path(os.environ["LOCALAPPDATA"]) / "Pearl Abyss" / "CD" / "save"
    if not root.is_dir():
        return None
    # Prefer the most recently modified slot0/save.save
    cands = sorted(root.rglob("slot*/save.save"), key=lambda p: p.stat().st_mtime, reverse=True)
    return cands[0] if cands else None


def main() -> int:
    save_path = find_live_save()
    if not save_path:
        print("no live save found (set CRIMSON_LIVE_SAVE or install game)", file=sys.stderr)
        return 1
    print(f"save: {save_path}")
    print(f"size: {save_path.stat().st_size:,} bytes  mtime: {save_path.stat().st_mtime}")

    parsed = cr.parse_save_from_file(str(save_path))
    body_bytes = parsed["body"]
    parsed_body = cr.parse_save_body_from_bytes(body_bytes)
    schema = parsed_body["schema"]
    types = schema["types"]

    print(f"schema: type_count={schema['type_count']}  root={schema['root_type']!r}")
    print()

    # ── Pass 1: exact / substring matches on TYPE names ────────────────────
    needles_type = [
        ("DockingChild", "iteminfo's DockingChildData parent struct"),
        ("Docking",      "any Docking* type"),
        ("Summon",       "anything with summon/summoner concept"),
        ("EquipQuick",   "is_equip_quick_slot_visible context"),
        ("HousingOnly",  "field just before is_equip_quick_slot_visible"),
        ("QuickSlot",    "field just after is_equip_quick_slot_visible"),
        ("ItemInfo",     "if the save embeds iteminfo schema directly"),
    ]
    type_by_name = {t["name"]: t for t in types}
    print("=== type-name search ===")
    for needle, why in needles_type:
        hits = [t["name"] for t in types if needle.lower() in t["name"].lower()]
        flag = f"  ({len(hits)})" if hits else "  (none)"
        print(f"  {needle:<14} -> {flag}  // {why}")
        for h in hits[:20]:
            print(f"      {h}")
    print()

    # ── Pass 2: exact / substring matches on FIELD names (across all types) ─
    needles_field = [
        "summon_tag",
        "SummonTag",
        "is_equip_quick_slot_visible",
        "IsEquipQuickSlotVisible",
        "equip_quick_slot",
        "EquipQuickSlot",
        "quick_slot",
        "QuickSlot",
        "docking_child",
        "DockingChild",
        "is_housing_only",
        "HousingOnly",
    ]
    print("=== field-name search ===")
    matches: list[tuple[str, str, str]] = []  # (type_name, field_name, field_type)
    for t in types:
        for f in t["fields"]:
            name = f["name"]
            for needle in needles_field:
                if needle.lower() in name.lower():
                    matches.append((t["name"], name, f["type_name"]))
                    break
    if not matches:
        print("  (zero field-name hits)")
    else:
        print(f"  {len(matches)} hit(s)")
        for tn, fn, ft in matches[:80]:
            print(f"    {tn}.{fn}  : {ft}")
    print()

    # ── Pass 3: see if any type's FIELD TYPE references DockingChild ────────
    print("=== field-type-name search (DockingChild / Summon) ===")
    type_refs = []
    for t in types:
        for f in t["fields"]:
            ft = f["type_name"]
            if "DockingChild" in ft or "Docking" in ft:
                type_refs.append((t["name"], f["name"], ft))
    if not type_refs:
        print("  (no type-name reference)")
    else:
        for tn, fn, ft in type_refs[:30]:
            print(f"    {tn}.{fn}  : {ft}")
    print()

    # ── Pass 4: dump ALL type names so the user can eyeball ─────────────────
    out_path = Path("out") / "save_schema_typenames.txt"
    out_path.parent.mkdir(exist_ok=True)
    with open(out_path, "w", encoding="utf-8") as fh:
        for t in sorted(types, key=lambda x: x["name"]):
            fh.write(f"{t['name']}  ({len(t['fields'])} fields)\n")
            for f in t["fields"]:
                fh.write(f"    {f['name']}  : {f['type_name']}  "
                         f"meta_kind={f['meta_kind']} meta_size={f['meta_size']}\n")
    print(f"full schema dump -> {out_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
