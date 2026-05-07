"""Classify every 1.05 item by the *true* size of its post-max_endurance
section, and look for fields in the part of the item that DOES parse
(everything up to and including max_endurance) that correlate with class.

Strategy:
  1. The parser succeeds up to max_endurance for every item — that's the
     part of the layout we already understand.
  2. For each item, we know its true total size from the anchor table.
     post_size = anchor_size - (offset where max_endurance ends).
  3. Group items by post_size; the values cluster into a small number of
     layout classes.
  4. For each pair of classes, check whether any single pre-max_endurance
     field's value perfectly partitions the items between the two classes.
"""

from __future__ import annotations
import argparse
import json
import struct
from collections import Counter, defaultdict
from pathlib import Path

import crimson_rs


SIMPLE_TYPES = {
    "u8":  ("<B", 1),
    "i8":  ("<b", 1),
    "u16": ("<H", 2),
    "i16": ("<h", 2),
    "u32": ("<I", 4),
    "i32": ("<i", 4),
    "u64": ("<Q", 8),
    "i64": ("<q", 8),
    "f32": ("<f", 4),
}


def extract_value(chunk: bytes, r: dict):
    ty = r["ty"]
    fmt_size = SIMPLE_TYPES.get(ty)
    if fmt_size is not None:
        fmt, n = fmt_size
        if r["end"] - r["start"] != n:
            return None
        return struct.unpack_from(fmt, chunk, r["start"])[0]
    if ty == "CString":
        try:
            return chunk[r["start"]:r["end"]].decode("utf-8", errors="replace")
        except Exception:
            return None
    if ty == "CString.len" or ty == "CArray.count":
        return struct.unpack_from("<I", chunk, r["start"])[0]
    return None


# Pre-max_endurance scalar fields we want to consider as discriminator candidates.
INTEREST = {
    "is_blocked", "max_stack_count", "broken_item_prefix_string",
    "inventory_info", "equip_type_info", "equipable_hash",
    "use_map_icon_alert", "item_type", "material_key", "material_match_info",
    "equipable_level", "category_info", "knowledge_info", "knowledge_obtain_type",
    "destroy_effec_info", "use_immediately", "apply_max_stack_cap",
    "extract_multi_change_info", "extract_additional_drop_set_info",
    "minimum_extract_enchant_level", "gimmick_info",
    "max_drop_result_sub_item_count", "use_drop_set_target",
    "is_all_gimmick_sealable", "delete_by_gimmick_unlock",
    "gimmick_unlock_message_local_string_info", "can_disassemble",
    "is_register_trade_market", "is_editor_usable", "discardable",
    "is_dyeable", "is_editable_grime", "is_destroy_when_broken",
    "is_housing_only", "quick_slot_index", "item_tier", "is_important_item",
    "apply_drop_stat_type", "item_charge_type", "usable_alert_type",
    "max_charged_useable_count", "unk_post_max_charged_a",
    "unk_post_max_charged_b", "discard_offset_y", "hide_from_inventory_on_pop_item",
    "is_shield_item", "is_tower_shield_item", "is_wild",
    "packed_item_info", "unpacked_item_info", "convert_item_info_by_drop_npc",
    "look_detail_game_advice_info_wrapper", "look_detail_mission_info",
    "enable_alert_system_to_ui", "is_save_game_data_at_use_item",
    "is_logout_at_use_item", "shared_cool_time_group_name_hash",
    "enable_equip_in_clone_actor", "is_blocked_store_sell",
    "is_preorder_item", "is_has_item_use_data_inventory_buff",
    "is_preserved_on_extract", "respawn_time_seconds", "max_endurance",
    # CArray counts for arrays that exist pre-max_endurance
    "occupied_equip_slot_data_list.__count__",
    "item_tag_list.__count__",
    "consumable_type_list.__count__",
    "item_use_info_list.__count__",
    "item_icon_list.__count__",
    "equip_passive_skill_list.__count__",
    "gimmick_tag_list.__count__",
    "sealable_item_info_list.__count__",
    "sealable_character_info_list.__count__",
    "sealable_gimmick_info_list.__count__",
    "sealable_gimmick_tag_list.__count__",
    "sealable_tribe_info_list.__count__",
    "sealable_money_info_list.__count__",
    "transmutation_material_gimmick_list.__count__",
    "transmutation_material_item_list.__count__",
    "transmutation_material_item_group_list.__count__",
    "multi_change_info_list.__count__",
    "reserve_slot_target_data_list.__count__",
    "prefab_data_list.__count__",
    "enchant_data_list.__count__",
    "gimmick_visual_prefab_data_list.__count__",
    "price_list.__count__",
    "fixed_page_data_list.__count__",
    "dynamic_page_data_list.__count__",
    "inspect_data_list.__count__",
    "hackable_character_group_info_list.__count__",
    "item_group_info_list.__count__",
    "pattern_description_data_list.__count__",
    "item_bundle_data_list.__count__",
    "money_type_define.__tag__",
    "docking_child_data.__tag__",
    "inventory_change_data.__tag__",
    "drop_default_data.default_sub_item.type_id",
    "default_sub_item.type_id",
}


def collect_feats(chunk: bytes, ranges: list[dict]) -> dict:
    feat = {}
    for r in ranges:
        path = r["path"]
        if path not in INTEREST:
            continue
        v = extract_value(chunk, r)
        if v is not None:
            feat[path] = v
    return feat


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--anchors", required=True)
    ap.add_argument("--pabgb", required=True)
    args = ap.parse_args()

    anchors = json.loads(Path(args.anchors).read_text(encoding="utf-8"))
    data = Path(args.pabgb).read_bytes()

    by_post: dict[int, list[dict]] = defaultdict(list)
    items_idx: dict[int, dict] = {}

    for i, row in enumerate(anchors):
        start = row["offset_105"]
        size = row["size_105"]
        chunk = data[start:start + size]
        res = crimson_rs.parse_iteminfo_tracked(chunk)

        if res["spans"]:
            ranges = res["spans"][0]["ranges"]
        else:
            ranges = res.get("error_span", {}).get("ranges", [])

        max_end_off = None
        for r in ranges:
            if r["path"] == "max_endurance":
                max_end_off = r["end"]
                break
        if max_end_off is None:
            continue

        post = size - max_end_off
        feat = collect_feats(chunk, ranges)
        rec = {
            "i": i,
            "key": row["key"],
            "size": size,
            "post": post,
            "feat": feat,
        }
        by_post[post].append(rec)
        items_idx[i] = rec

    print(f"items reaching max_endurance: {len(items_idx):,}")
    print()
    print("Layout classes (post-max_endurance bytes):")
    for ps, recs in sorted(by_post.items(), key=lambda kv: -len(kv[1])):
        print(f"  {ps:>4} bytes : {len(recs):>5} items")

    label = {i: ("B" if r["post"] < 20 else "A") for i, r in items_idx.items()}
    a_n = sum(1 for v in label.values() if v == "A")
    b_n = sum(1 for v in label.values() if v == "B")
    print()
    print(f"Class A (post >= 20): {a_n:,}")
    print(f"Class B (post <  20): {b_n:,}")

    # Find perfect / near-perfect partitioner
    fields = set()
    for rec in items_idx.values():
        fields.update(rec["feat"].keys())

    print()
    print(f"Trying {len(fields)} pre-max_endurance fields as A/B partitioner:")
    candidates: list[tuple[str, float, set, set]] = []
    for f in sorted(fields):
        a_vals: Counter = Counter()
        b_vals: Counter = Counter()
        for i, rec in items_idx.items():
            v = rec["feat"].get(f)
            if v is None:
                continue
            (a_vals if label[i] == "A" else b_vals)[v] += 1
        if not a_vals or not b_vals:
            continue
        a_set = set(a_vals.keys())
        b_set = set(b_vals.keys())
        overlap = a_set & b_set
        overlap_a = sum(a_vals[v] for v in overlap)
        overlap_b = sum(b_vals[v] for v in overlap)
        total = sum(a_vals.values()) + sum(b_vals.values())
        ambiguous = overlap_a + overlap_b
        purity = 1 - ambiguous / total
        candidates.append((f, purity, a_set, b_set))

    print()
    print("Top 20 partitioners (purity = 1 means perfect split):")
    for f, p, a_set, b_set in sorted(candidates, key=lambda kv: -kv[1])[:20]:
        only_a = a_set - b_set
        only_b = b_set - a_set
        print(
            f"  {p*100:6.2f}%  {f:<55} "
            f"a-only={sorted(list(only_a))[:6]}{'...' if len(only_a) > 6 else ''}  "
            f"b-only={sorted(list(only_b))[:6]}{'...' if len(only_b) > 6 else ''}"
        )


if __name__ == "__main__":
    main()
