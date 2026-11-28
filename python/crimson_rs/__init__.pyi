"""Type stubs for crimson_rs - Crimson Desert archive toolkit."""

from __future__ import annotations

from typing import Any, TypedDict

__version__: str
"""The crate version (``Cargo.toml``)."""
PARSER_TARGET: tuple[int, int, int]
"""Game patch the parsers target, ``(major, minor, min_patch)`` in paver
numbering (``(2, 6, 0)`` for 2.06). Compare it with the install's
``meta/0.paver`` to catch a stale build."""

from crimson_rs.enums import Compression as Compression
from crimson_rs.enums import Crypto as Crypto
from crimson_rs.enums import Language as Language

# ── Nested Structs ──────────────────────────────────────────────────────────


class LocalizableString(TypedDict):
    category: int
    """Localization category (u8)."""
    index: int
    """Localization table index (u64)."""
    default: str
    """Default string value."""


class OccupiedEquipSlotData(TypedDict):
    equip_slot_name_key: int
    """u32"""
    equip_slot_name_index_list: list[int]
    """list of u8 values."""


class ItemIconData(TypedDict):
    icon_path: int
    """StringInfoKey (u32)."""
    check_exist_sealed_data: int
    """u8"""
    gimmick_state_list: list[int]
    """list of u32."""


class PassiveSkillLevel(TypedDict):
    skill: int
    """SkillKey (u32)."""
    level: int
    """u32"""


class ReserveSlotTargetData(TypedDict):
    reserve_slot_info: int
    """ReserveSlotKey (u32)."""
    condition_info: int
    """ConditionKey (u32)."""


class SocketMaterialItem(TypedDict):
    item: int
    """ItemKey (u32)."""
    value: int
    """u64"""


class EnchantStatChange(TypedDict):
    stat: int
    """StatusKey (u32)."""
    change_mb: int
    """i64"""


class EnchantLevelChange(TypedDict):
    stat: int
    """StatusKey (u32)."""
    change_mb: int
    """i8"""


class EnchantStatData(TypedDict):
    max_stat_list: list[EnchantStatChange]
    regen_stat_list: list[EnchantStatChange]
    stat_list_static: list[EnchantStatChange]
    stat_list_static_level: list[EnchantLevelChange]


class PriceFloor(TypedDict):
    price: int
    """u64"""
    sym_no: int
    """u32"""
    item_info_wrapper: int
    """ItemKey (u32)."""


class ItemPriceInfo(TypedDict):
    key: int
    """ItemKey (u32)."""
    price: PriceFloor


class EquipmentBuff(TypedDict):
    buff: int
    """BuffKey (u32)."""
    level: int
    """u32"""


class EnchantData(TypedDict):
    level: int
    """u16"""
    enchant_stat_data: EnchantStatData
    buy_price_list: list[ItemPriceInfo]
    equip_buffs: list[EquipmentBuff]
    unk_post_equip_buffs: int
    """u32 after ``equip_buffs``; meaning not RE'd."""


class MergedPrefabVisualData(TypedDict):
    """One element of ``merged_prefab_visual_list``, which holds prefab and
    gimmick-visual prefab entries in one list."""
    scale: list[float]
    """[f32; 3]; (1, 1, 1) on prefab-origin rows."""
    prefab_names: list[int]
    """StringInfoKey list (u32)."""
    animation_path_list: list[int]
    """StringInfoKey list (u32)."""
    equip_slot_list: list[int]
    """u16 list."""
    tribe_gender_list: list[int]
    """StringInfoKey list (u32)."""
    unk_pre_is_craft_material: int
    """u8"""
    is_craft_material: int
    """u8"""
    use_gimmick_prefab: int
    """u8"""
    unk_post_flag: int
    """u8"""


class GameEventExecuteData(TypedDict):
    game_event_type: int
    """u8"""
    player_condition: int
    """ConditionKey (u32)."""
    target_condition: int
    """ConditionKey (u32)."""
    event_condition: int
    """ConditionKey (u32)."""


class InventoryChangeData(TypedDict):
    game_event_execute_data: GameEventExecuteData
    to_inventory_info: int
    """InventoryKey (u16)."""


class PageData(TypedDict):
    left_page_texture_path: str
    right_page_texture_path: str
    left_page_related_knowledge_info: int
    """KnowledgeKey (u32)."""
    right_page_related_knowledge_info: int
    """KnowledgeKey (u32)."""
    unk_post_right_page_related_knowledge_info: int
    """u8. 0, or 2 on the DLC's treasure-map pages."""


class InspectData(TypedDict):
    item_info: int
    """ItemKey (u32)."""
    gimmick_info: int
    """GimmickInfoKey (u32)."""
    character_info: int
    """CharacterKey (u32)."""
    spawn_reason_hash: int
    """u32"""
    unk_post_inspect_effect_a: float
    """f32; meaning not RE'd."""
    unk_post_inspect_effect_b: float
    """f32; meaning not RE'd."""
    socket_name: str
    speak_character_info: int
    """CharacterKey (u32)."""
    inspect_target_tag: int
    """u32"""
    reward_own_knowledge: int
    """u8"""
    reward_knowledge_info: int
    """KnowledgeKey (u32)."""
    item_desc: LocalizableString
    board_key: int
    """u32"""
    inspect_action_type: int
    """u8"""
    gimmick_state_name_hash: int
    """u32"""
    target_page_index: int
    """u32"""
    is_left_page: int
    """u8"""
    target_page_related_knowledge_info: int
    """KnowledgeKey (u32)."""
    enable_read_after_reward: int
    """u8"""
    refer_to_left_page_inspect_data: int
    """u8"""
    inspect_effect_info_key: int
    """EffectKey (u32)."""
    inspect_complete_effect_info_key: int
    """EffectKey (u32)."""


class InspectAction(TypedDict):
    action_name_hash: int
    """u32"""
    catch_tag_name_hash: int
    """u32"""
    catcher_socket_name: str
    catch_target_socket_name: str


class ItemInfoSharpnessData(TypedDict):
    max_sharpness: int
    """u16"""
    craft_tool_info: int
    """CraftToolKey (u16)."""
    stat_data: EnchantStatData


class ItemBundleData(TypedDict):
    count_mb: int
    """u64"""
    key: int
    """GimmickInfoKey (u32)."""


class UnitData(TypedDict):
    ui_component: str
    minimum: int
    """u32"""
    icon_path: int
    """StringInfoKey (u32)."""
    item_name: LocalizableString
    item_desc: LocalizableString


class MoneyUnitEntry(TypedDict):
    key: int
    """u32"""
    value: UnitData


class MoneyTypeDefine(TypedDict):
    price_floor_value: int
    """u64"""
    unit_data_list_map: list[MoneyUnitEntry]



class RepairData(TypedDict):
    resource_item_info: int
    """ItemKey (u32)."""
    repair_value: int
    """u16"""
    repair_style: int
    """u8"""
    resource_item_count: int
    """u64"""


class SubItem(TypedDict):
    type_id: int
    """u8 variant tag. 0=Item, 3=Character, 9=Gimmick, 14=None."""
    value: int | None
    """Key value (u32) or None for type_id=14."""


class DropDefaultData(TypedDict):
    drop_enchant_level: int
    """u16"""
    socket_item_list: list[int]
    """ItemKey list (u32)."""
    add_socket_material_item_list: list[SocketMaterialItem]
    default_sub_item: SubItem
    socket_valid_count: int
    """u8"""
    use_socket: int
    """u8"""


class SealableItemInfo(TypedDict):
    type_tag: int
    """u8 variant tag. 0=Item, 1=Gimmick, 2=String, 3=Character, 4=Tribe."""
    item_key: int
    """ItemKey (u32)."""
    unknown0: int
    """u64"""
    value: int | str
    """Key value (u32) for types 0/1/3/4, or str for type 2."""


class DockingChildData(TypedDict):
    gimmick_info_key: int
    """GimmickInfoKey (u32)."""
    character_key: int
    """CharacterKey (u32)."""
    item_key: int
    """ItemKey (u32)."""
    attach_parent_socket_name: str
    attach_child_socket_name: str
    unk_post_summon_tag_a: int
    """u32; meaning not RE'd."""
    unk_post_summon_tag_b: int
    """u32; meaning not RE'd."""
    docking_tag_name_hash: list[int]
    """[u32; 4] - 4 ints."""
    docking_equip_slot_no: int
    """u16"""
    spawn_distance_level: int
    """u32"""
    is_item_equip_docking_gimmick: int
    """u8"""
    send_damage_to_parent: int
    """u8"""
    is_body_part: int
    """u8"""
    docking_type: int
    """u8"""
    is_summoner_team: int
    """u8"""
    is_player_only: int
    """u8"""
    is_npc_only: int
    """ConditionKey (u32)."""
    is_sync_break_parent: int
    """u8"""
    hit_part: int
    """u8"""
    detected_by_npc: int
    """u8"""
    is_bag_docking: int
    """u8"""
    enable_collision: int
    """u8"""
    unk_pre_disable_collision: int
    """u32, always 0."""
    disable_collision_with_other_gimmick: int
    """u8"""
    docking_slot_key: str
    inherit_summoner: int
    """u8"""
    summon_tag_name_hash: list[int]
    """[u32; 4] - 4 ints."""


class PatternParamString(TypedDict):
    flag: int
    """u8"""
    unk_flag_2: int
    """u8"""
    unk_value: list[int]
    """[u32; 2] - 2 ints."""
    param_string: str


class PatternDescriptionData(TypedDict):
    pattern_description_info: int
    """u32"""
    param_string_list: list[PatternParamString]


# ── ItemInfo ────────────────────────────────────────────────────────────────


class ItemInfo(TypedDict):
    """A single item parsed from the iteminfo binary file."""

    # Identity. On disk a row opens with ``key`` then ``row_index``, and
    # ``string_key`` lives in the table's ``.staticinfoheader``. Fields come in
    # file order, not this order: see the "ItemInfo (2.04)" header in
    # ``src/item_info/item.rs``.
    key: int
    """Unique item ID. ItemKey (u32)."""
    row_index: int
    """The row's position in the base table (u32), written right after ``key``."""
    string_key: str
    """String identifier, e.g. ``"Pyeonjeon_Arrow"``, read from the header;
    ``""`` when the parser is not given one."""
    is_blocked: int
    """u8"""
    max_stack_count: int
    """u64"""
    item_name: LocalizableString
    broken_item_prefix_string: int
    """LocalStringInfoKey (u32)."""

    # Inventory & Equipment
    equip_type_info: int
    """EquipTypeKey (u32)."""
    occupied_equip_slot_data_list: list[OccupiedEquipSlotData]
    item_tag_list: list[int]
    """u32 list."""
    equipable_hash: int
    """u32"""
    item_use_info_list: list[int]
    """ItemUseKey list (u32)."""
    item_icon_list: list[ItemIconData]
    map_icon_path: int
    """StringInfoKey (u32)."""
    use_map_icon_alert: int
    """u8"""
    item_type: int
    """u8"""
    material_key: int
    """u32"""
    material_match_info: int
    """MaterialMatchKey (u32)."""
    item_desc: LocalizableString
    item_desc2: LocalizableString
    equipable_level: int
    """u32"""
    category_info: int
    """CategoryKey (u16)."""
    knowledge_info: int
    """KnowledgeKey (u32)."""
    knowledge_obtain_type: int
    """u8"""
    destroy_effec_info: int
    """EffectKey (u32)."""
    equip_passive_skill_list: list[PassiveSkillLevel]
    use_immediately: int
    """u8"""
    apply_max_stack_cap: int
    """u8"""
    extract_multi_change_info: int
    """MultiChangeKey (u32)."""
    minimum_extract_enchant_level: int
    """u16"""
    item_memo: str
    filter_type: str
    charged_sound_name: str
    """Empty except on a few DLC ship cannons."""
    charged_sound_param: str
    """Empty except on a few DLC ship cannons."""
    gimmick_info: int
    """GimmickInfoKey (u32)."""
    gimmick_tag_list: list[str]
    max_drop_result_sub_item_count: int
    """u32"""
    use_drop_set_target: int
    """u8"""
    is_all_gimmick_sealable: int
    """u8"""
    sealable_item_info_list: list[SealableItemInfo]
    sealable_character_info_list: list[SealableItemInfo]
    sealable_gimmick_info_list: list[SealableItemInfo]
    sealable_gimmick_tag_list: list[SealableItemInfo]
    sealable_tribe_info_list: list[SealableItemInfo]
    sealable_money_info_list: list[int]
    """ItemKey list (u32)."""
    delete_by_gimmick_unlock: int
    """u8"""
    gimmick_unlock_message_local_string_info: int
    """LocalStringInfoKey (u32)."""
    can_disassemble: int
    """u8"""
    transmutation_material_gimmick_list: list[int]
    """GimmickInfoKey list (u32)."""
    transmutation_material_item_list: list[int]
    """ItemKey list (u32)."""
    transmutation_material_item_group_list: list[int]
    """ItemGroupKey list (u16)."""
    is_register_trade_market: int
    """u8"""
    multi_change_info_list: list[int]
    """MultiChangeKey list (u32)."""
    is_editor_usable: int
    """u8"""
    discardable: int
    """u8"""
    unk_post_discardable: int
    """u8. 1 on exactly the items with `item_type` 102 or 103, else 0."""
    is_dyeable: int
    """u8"""
    is_editable_grime: int
    """u8"""
    is_destroy_when_broken: int
    """u8"""
    is_housing_only: int
    """u8"""
    reserve_slot_target_data_list: list[ReserveSlotTargetData]
    item_tier: int
    """u8"""
    item_importance_kind: int
    """u8. 0 on important items, 2 on the others, and 1 on one item."""
    apply_drop_stat_type: int
    """u8"""
    drop_default_data: DropDefaultData | None
    """None where it would be all defaults (no sockets, tag-18 sub item)."""
    enchant_data_list: list[EnchantData]
    price_list: list[ItemPriceInfo]
    docking_child_data: DockingChildData | None
    docking_child_data_list: list[DockingChildData]
    """Empty except on the ``Item_wagon_wheel_*`` items."""
    inventory_change_data: InventoryChangeData | None
    unk_texture_path: int
    """StringInfoKey (u32). The empty string is 0xeac5e173."""
    fixed_page_data_list: list[PageData]
    dynamic_page_data_list: list[PageData]
    inspect_data_list: list[InspectData]
    inspect_action: InspectAction | None
    """None when absent."""
    unk_pre_cooltime_flag: int
    """u8, 1 on every item."""
    default_sub_item: SubItem
    cooltime: int
    """i64"""
    unk_post_cooltime_a: int
    """i64"""
    unk_post_cooltime_b: int
    """i64"""
    item_charge_type: int
    """u8"""
    usable_alert_type: int
    """u8"""
    sharpness_data: ItemInfoSharpnessData | None
    """None when absent."""
    max_charged_useable_count: int
    """u32"""
    unk_post_max_charged_a: int
    """u32"""
    unk_post_max_charged_b: int
    """u32"""
    hackable_character_group_info_list: list[int]
    """CharacterGroupKey list (u16)."""
    item_group_info_list: list[int]
    """ItemGroupKey list (u16)."""
    discard_offset_y: float
    """f32"""
    hide_from_inventory_on_pop_item: int
    """u8"""
    is_shield_item: int
    """u8"""
    is_tower_shield_item: int
    """u8"""
    is_wild: int
    """u8"""
    packed_item_info: int
    """ItemKey (u32)."""
    unpacked_item_info: int
    """ItemKey (u32)."""
    convert_item_info_by_drop_npc: int
    """ItemKey (u32)."""
    pattern_description_data_list: list[PatternDescriptionData]
    look_detail_game_advice_info_wrapper: int
    """GameAdviceInfoKey (u32)."""
    look_detail_mission_info: int
    """MissionKey (u32)."""
    enable_alert_system_to_ui: int
    """u8"""
    is_save_game_data_at_use_item: int
    """u8"""
    is_logout_at_use_item: int
    """u8"""
    shared_cool_time_group_name_hash: int
    """u32"""
    item_bundle_data_list: list[ItemBundleData]
    money_type_define: MoneyTypeDefine | None
    emoji_texture_id: int
    """StringInfoKey (u32). The empty string is 0xeac5e173."""
    enable_equip_in_clone_actor: int
    """u8"""
    is_blocked_store_sell: int
    """u8"""
    is_preorder_item: int
    """u8"""
    is_preserved_on_extract: int
    """u8"""
    is_equip_quick_slot_visible: int
    """u8"""
    is_has_item_use_data_inventory_buff: int
    """u8"""
    respawn_time_seconds: int | None
    """i64"""
    unk_pre_max_endurance_a: int
    """Meaning not RE'd."""
    max_endurance: int | None
    """u16"""
    unk_post_apply_drop_stat_type: int
    """Meaning not RE'd."""
    unk_pre_gimmick_visual: int | None
    """Conditional on ``equip_type_info``; writing it back does not re-check
    the condition, so keep it consistent when editing ``equip_type_info``."""
    merged_prefab_visual_list: list[MergedPrefabVisualData]
    """The merged prefab + gimmick-visual list (see the class)."""
    repair_data_list: list[RepairData]

    # The field set matches what `parse_iteminfo_from_bytes` returns; the order
    # and the per-field notes above are partial. `src/item_info/item.rs` is the
    # source of truth for the layout.
    unk_pre_respawn_a: int
    """u32. 0 except on the Wood_Branch_* tiers, which carry an ItemKey-shaped
    value."""
    unk_pre_respawn_block: UnkPreRespawnBlock | None
    """Holds `unk_pre_respawn_data_list`. None on every base-table item;
    only `iteminfo.dlc_01` carries it."""
    is_packed_in_vehicle: int
    """u8, 1 on the `Trade_*_PackedInVehicle` items."""
    unk_pre_packed_item_flag: int
    """u8."""
    inventory_info_list: list[int]
    """Twelve InventoryKey (u16) slots at the item end. Slot 0 is the
    inventory tab the item goes to; 0xFF marks an unused slot."""


class UnkPreRespawnBlock(TypedDict):
    unk_a: int
    """u32, 1 everywhere seen."""
    unk_flag: int
    """u8, 0 everywhere seen."""
    unk_pre_respawn_data_list: list[UnkPreRespawnData]
    unk_b: int
    """u32, 1..=4."""
    item_group_list: list[int]
    """ItemGroupKey (u16)."""


class UnkPreRespawnData(TypedDict):
    """Element of `ItemInfo.unk_pre_respawn_data_list` (28 bytes).
    Semantic role unknown; `unk_b == unk_c` and `unk_d == unk_c + 1` on every
    element in the table."""

    unk_a: int
    """u32. Only 11, 12 or 13 observed."""
    unk_b: int
    """u64"""
    unk_c: int
    """u64"""
    unk_d: int
    """u64"""


# ── Module Functions ────────────────────────────────────────────────────────


def parse_iteminfo_from_file(path: str, header_path: str | None = None) -> list[ItemInfo]:
    """Parse all items from a binary file.

    Since 2.06 an item's ``string_key`` lives in the table's
    ``.staticinfoheader``; pass it to get the names (and a check that every
    row's key and length match it). Without it ``string_key`` is ``""``.

    Args:
        path: Path to the decompressed iteminfo binary file.
        header_path: Optional path to the table's ``.staticinfoheader`` /
            ``.pabgh`` (2.06 shape).

    Returns:
        List of item dicts with full type info.

    Raises:
        IOError: If the file cannot be read.
        ValueError: On parse errors.
    """
    ...


def parse_iteminfo_from_bytes(data: bytes, header: bytes | None = None) -> list[ItemInfo]:
    """Parse all items from raw bytes.

    See ``parse_iteminfo_from_file``: pass the table's ``.staticinfoheader``
    to get ``string_key`` on 2.06; without it ``string_key`` is ``""``.

    Args:
        data: Raw binary data (the table body).
        header: Optional raw ``.staticinfoheader`` bytes (2.06 shape).

    Returns:
        List of item dicts.

    Raises:
        ValueError: On parse errors.
    """
    ...


class FieldRange(TypedDict):
    path: str
    """Dot-separated field path (e.g. "key", "enchant_data_list.0.level")."""
    start: int
    """Byte offset start (absolute)."""
    end: int
    """Byte offset end (absolute)."""
    ty: str
    """Rust type name."""


class ItemSpan(TypedDict):
    start: int
    """Byte offset of the item start."""
    end: int
    """Byte offset of the item end."""
    size: int
    """Total byte size of the item."""
    ranges: list[FieldRange]


class TrackedItemInfo(TypedDict):
    items: list[ItemInfo]
    spans: list[ItemSpan]


def parse_iteminfo_tracked(data: bytes) -> TrackedItemInfo:
    """Parse all items with byte-level field attribution.

    Returns:
        Dict with "items" (list of ItemInfo) and "spans" (list of per-item
        byte ranges for every field).

    Body only: on 2.06+ the row names live in the ``.staticinfoheader``, so
    every ``string_key`` comes back ``""``. Use
    :func:`parse_iteminfo_from_bytes` with the header for names.
    """
    ...


class LegacyPatch(TypedDict, total=False):
    entry: str
    """``string_key`` of the target item (e.g. ``"Pyeonjeon_Arrow"``)."""
    rel_offset: int
    """Byte offset within the item, relative to its span start."""
    length: int
    """Optional patch length, echoed back as ``hit_length``. Defaults to 0."""


class LegacyPatchHit(TypedDict):
    path: str
    """Dotted field path the patch lands on."""
    ty: str
    """Rust type name of the field."""
    abs_start: int
    """Absolute byte start of the field in the source bytes."""
    abs_end: int
    """Absolute byte end of the field (exclusive)."""
    hit_offset: int
    """Offset of the patch *within* the field (``abs_pos - abs_start``)."""
    hit_length: int
    """Echo of input ``length`` (0 if not provided)."""


def inspect_legacy_patches(
    vanilla_bytes: bytes,
    patches: list[LegacyPatch],
    header: bytes | None = None,
) -> list[LegacyPatchHit | None]:
    """Resolve JSON v2 byte patches to field-path attributions.

    Parses ``vanilla_bytes`` once with byte-level tracking, then for each
    patch finds the field whose ``[start, end)`` covers
    ``entry.start + rel_offset``. Useful for translating legacy byte-patch
    mods to a semantic / field-keyed format without having to apply the
    patch first and reparse.

    Args:
        vanilla_bytes: A vanilla ``iteminfo.pabgb`` blob.
        patches: List of dicts with ``entry``, ``rel_offset``, optional
            ``length``.
        header: 2.06+: the ``iteminfo.staticinfoheader``, which holds the row
            names ``entry`` is matched against. Without it every 2.06 patch
            resolves to None.

    Returns:
        Same-length list. Entry is ``None`` when the entry name is not
        found, or when the offset falls outside any tracked field of the
        target entry. Otherwise a dict with the field attribution and a
        ``hit_length`` echo.
    """
    ...


def write_iteminfo_to_file(items: list[ItemInfo], path: str) -> None:
    """Serialize items and write to a file.

    Args:
        items: List of item dicts (same structure as returned by parse_iteminfo_from_file).
        path: Output file path.

    Raises:
        IOError: On write failure.
        KeyError: If a required field is missing.
        ValueError: On invalid data.
    """
    ...


def serialize_iteminfo_with_header(items: list[ItemInfo]) -> tuple[bytes, bytes]:
    """Serialize items to a 2.06 ``(body, header)`` pair. The header is
    rebuilt from each item's ``key`` and ``string_key`` with fresh offsets and
    sizes, so length-changing edits and renamed ``string_key`` values survive.
    An unedited list round-trips both files byte for byte."""
    ...


def serialize_iteminfo(items: list[ItemInfo]) -> bytes:
    """Serialize items to raw bytes.

    Args:
        items: List of item dicts.

    Returns:
        Binary data as bytes.
    """
    ...


# ── Localization ──────────────────────────────────────────────────────────


class LocalizationEntry(TypedDict):
    unk_id: int
    """u64"""
    string_key: str
    """String key identifier."""
    string_value: str
    """Localized string value (UTF-8)."""


def parse_paloc_bytes(data: bytes) -> list[LocalizationEntry]:
    """Parse localization entries from an extracted ``.paloc`` file.

    Accepts both layouts: the bare entry list (up to 2.02) and the 2.03+
    container (``"paloc"`` magic, 0x200-byte header, one LZ4 block), which
    is unwrapped first.
    """
    ...


def serialize_paloc(items: list[LocalizationEntry]) -> bytes:
    """Serialize localization entries to a bare entry list (the <= 2.02
    layout). Pass the result through :func:`wrap_paloc_bytes` for a 2.03+
    install."""
    ...


def unwrap_paloc_bytes(data: bytes) -> bytes:
    """The bare entry list of a ``.paloc`` file: decompressed out of the
    2.03+ container, or ``data`` unchanged when it has none.

    Raises:
        ValueError: The container header or its LZ4 block does not check out.
    """
    ...


def wrap_paloc_bytes(data: bytes) -> bytes:
    """Wrap a bare entry list (e.g. :func:`serialize_paloc` output) in the
    2.03+ container. Unwraps back to ``data`` exactly; the compressed bytes
    are lz4_flex's, so they differ from the game's own.

    Raises:
        ValueError: ``data`` is already a container.
    """
    ...


# ── Checksum ───────────────────────────────────────────────────────────────


def calculate_checksum(data: bytes) -> int:
    """Compute the Jenkins hashlittle checksum used by Crimson Desert file formats.

    Args:
        data: Raw bytes to checksum.

    Returns:
        u32 checksum value.
    """
    ...


# ── PAPGT Types ────────────────────────────────────────────────────────────


class PapgtEntry(TypedDict):
    group_name: str
    """Pack group name (e.g. "0000", "0001")."""
    is_optional: int
    """u8"""
    language: int
    """u16 bitmask (LanguageType flags)."""
    always_zero: int
    """u8"""
    group_name_offset: int
    """u32 — offset in the group names buffer."""
    pack_meta_checksum: int
    """u32 — checksum of the corresponding 0.pamt file."""


class PapgtData(TypedDict):
    unknown0: int
    """u32 (<= 2.03) / u16 (2.04+)"""
    checksum: int
    """u32 — checksum of the post-header data; 0 on a 2.04+ file, which has none."""
    unknown1: int
    """u8"""
    unknown2: int
    """u16; <= 2.03 only."""
    entries: list[PapgtEntry]
    _trailer: bytes | None
    """The 64 closing bytes of a 2.04+ file, carried verbatim. They look like a
    signature over the file and cannot be recomputed, so they go stale when
    entries are edited. None on a <= 2.03 file."""


def parse_papgt_file(path: str) -> PapgtData:
    """Parse a PAPGT file (pack group tree meta).

    Args:
        path: Path to the .papgt file.

    Returns:
        Dict with header fields and list of entries.
    """
    ...


def parse_papgt_bytes(data: bytes) -> PapgtData:
    """Parse a PAPGT from raw bytes."""
    ...


def write_papgt_file(data: PapgtData, path: str) -> None:
    """Serialize PAPGT data and write to a file."""
    ...


def serialize_papgt(data: PapgtData) -> bytes:
    """Serialize PAPGT data to raw bytes."""
    ...


# ── PAMT Types ─────────────────────────────────────────────────────────────


class PamtEncryptInfo(TypedDict):
    unknown0: int
    """u8"""
    encrypt_info: bytes
    """3 bytes of encryption key material."""


class PamtChunk(TypedDict):
    id: int
    """u32 — paz file ID."""
    checksum: int
    """u32 — CRC32 of the paz file."""
    size: int
    """u32 — total paz file size."""


class PamtFile(TypedDict):
    name: str
    """Resolved file name. Read-only: ``serialize_pamt`` takes names from
    ``_file_names_buffer``."""
    name_offset: int
    """u32 — offset in file names trie buffer."""
    chunk_offset: int
    """u32 — offset within the paz chunk."""
    compressed_size: int
    """u32"""
    uncompressed_size: int
    """u32"""
    chunk_id: int
    """u16 — references a chunk by ID."""
    flags: int
    """u8 — raw flags byte."""
    extension_index: int
    """u8 — index into the 2.04+ extension table; ``name`` already includes the
    extension. Always 0 on a <= 2.03 file."""
    compression: int
    """u8 — decoded compression type (0=None, 2=LZ4, 3=Zlib, 4=QuickLZ).
    Read-only: ``serialize_pamt`` writes ``flags``."""
    crypto: int
    """u8 — decoded crypto type (0=None, 1=ICE, 2=AES, 3=ChaCha20).
    Read-only: ``serialize_pamt`` writes ``flags``."""
    is_partial: bool
    """Whether this is a partial-compression entry. Read-only: ``serialize_pamt``
    writes ``flags``."""


class PamtDirectory(TypedDict):
    path: str
    """Resolved directory path. Read-only: ``serialize_pamt`` takes paths from
    ``_dir_names_buffer``."""
    name_checksum: int
    """u32"""
    name_offset: int
    """i32 — offset in dir names trie buffer (-1 for root)."""
    file_start_index: int
    """u32 — must equal the number of files in the directories before this
    one: ``serialize_pamt`` rebuilds the file table from ``files`` in directory
    order and raises ``ValueError`` otherwise."""
    file_count: int
    """u32 — must equal ``len(files)``."""
    files: list[PamtFile]


class PamtData(TypedDict):
    checksum: int
    """u32"""
    unknown0: int
    """u16"""
    encrypt_info: PamtEncryptInfo
    chunks: list[PamtChunk]
    directories: list[PamtDirectory]
    _dir_names_buffer: bytes
    """Raw trie buffer (needed for roundtrip serialization)."""
    _file_names_buffer: bytes
    """Raw trie buffer (needed for roundtrip serialization)."""
    _extensions_buffer: bytes | None
    """Raw 2.04+ extension table, NUL-separated with entry 0 empty. None on a
    <= 2.03 file, whose tries also use absolute parent offsets."""


def parse_pamt_file(path: str) -> PamtData:
    """Parse a PAMT file (pack meta / VFS listing).

    Args:
        path: Path to the .pamt file.

    Returns:
        Dict with header, chunks, and directory/file listings.
    """
    ...


def parse_pamt_bytes(data: bytes) -> PamtData:
    """Parse a PAMT from raw bytes."""
    ...


def write_pamt_file(data: PamtData, path: str) -> None:
    """Serialize PAMT data and write to a file."""
    ...


def serialize_pamt(data: PamtData) -> bytes:
    """Serialize PAMT data to raw bytes."""
    ...


# ── Compression ───────────────────────────────────────────────────────────


def compress_data(data: bytes, compression: int) -> bytes:
    """Compress data using the specified algorithm.

    Args:
        data: Raw bytes to compress.
        compression: 0=None, 2=LZ4, 3=Zlib.

    Returns:
        Compressed bytes.
    """
    ...


def decompress_data(data: bytes, compression: int, uncompressed_size: int) -> bytes:
    """Decompress data using the specified algorithm.

    Args:
        data: Compressed bytes.
        compression: 0=None, 2=LZ4, 3=Zlib.
        uncompressed_size: Expected size after decompression.

    Returns:
        Decompressed bytes.
    """
    ...


# ── Pack Group Builder ────────────────────────────────────────────────────


class PackGroupBuilder:
    """Streaming pack group builder that writes .paz files to disk incrementally.

    Usage::

        builder = PackGroupBuilder("/path/to/0099", compression=2)
        builder.add_file("textures", "icon.dds", raw_bytes)
        builder.add_file_from_path("models", "mesh.obj", "/path/to/mesh.obj")
        pamt_bytes = builder.finish()  # writes .paz + 0.pamt to output_dir
    """

    def __init__(
        self,
        output_dir: str,
        compression: int = 2,
        crypto: int = 0,
        encrypt_info: bytes = b"\x00\x00\x00",
        max_chunk_size: int = 500_000_000,
    ) -> None: ...

    def add_file(self, dir_path: str, file_name: str, data: bytes) -> None:
        """Add a file from raw bytes. Data is compressed/encrypted and appended
        to the current .paz chunk immediately."""
        ...

    def add_file_from_path(self, dir_path: str, file_name: str, file_path: str) -> None:
        """Add a file by reading from a path on disk."""
        ...

    def finish(self) -> bytes:
        """Flush remaining chunk, write 0.pamt to output_dir.
        Returns the raw PAMT bytes (for computing checksum for PAPGT)."""
        ...


def add_papgt_entry(
    papgt_data: PapgtData,
    group_name: str,
    pack_meta_checksum: int,
    is_optional: int,
    language: int,
) -> PapgtData:
    """Add a new entry to a PAPGT dict.

    Args:
        papgt_data: Existing PAPGT data dict.
        group_name: Pack group name (e.g. "0099"). An existing entry of that name is replaced.
        pack_meta_checksum: Checksum of the group's 0.pamt post-header data.
        is_optional: Whether this group is optional (0 or 1).
        language: Language bitmask (0x7FFF for ALL).

    Returns:
        Updated PAPGT data dict with the new entry and recalculated checksum.
    """
    ...


# ── File Extraction ──────────────────────────────────────────────────────


def extract_file(
    game_dir: str,
    group_name: str,
    dir_path: str,
    file_name: str,
) -> bytes:
    """Extract a single file from a pack group archive.

    Reads the PAMT index, locates the file in the .paz chunk,
    decrypts and decompresses it.

    Args:
        game_dir: Path to the game installation directory.
        group_name: Pack group name (e.g. "0008").
        dir_path: Directory path within the archive (e.g. "gamedata/binary__/client/bin").
        file_name: File name (e.g. "iteminfo.pabgb").

    Returns:
        Raw decompressed file data as bytes.

    Raises:
        IOError: If the PAZ file cannot be read.
        ValueError: If the directory or file is not found in the PAMT.
    """
    ...


class Graph(TypedDict):
    val0: int
    """i64"""
    val1: int
    """i64"""
    val2: int
    """i64"""
    val3: int
    """u32"""


class ResourceStat(TypedDict):
    stat_type: int
    """u8"""
    stat_hash: int
    """u32"""
    flag: int
    """u8"""
    value: int
    """i64"""
    hash2: int
    """u32"""
    hash3: int
    """u32"""


class ResourceItem(TypedDict):
    item_hash: int
    """u32"""
    count: int
    """i64"""


class PostBuff(TypedDict):
    skill_group_key: int
    """u32"""
    parent_skill: int
    """u32"""
    learn_level: int
    """u32"""
    apply_type: int
    """u8"""
    icon_path: int
    """StringInfoKey (u32)."""
    need_upgrade_item_info: int
    """ItemKey (u32)."""
    need_upgrade_item_count_graph: Graph
    need_upgrade_experience_graph: Graph
    usable_character_info_list: list[int]
    """u32 list."""
    usable_condition: list[int]
    """ConditionKey list (u32)."""
    learn_knowledge_info: int
    """KnowledgeKey (u32)."""
    faction_info: int
    """FactionKey (u32)."""
    use_resource_stat_list: list[ResourceStat]
    use_resource_item_list: list[ResourceItem]
    use_driver_resource_stat_list: list[ResourceStat]
    use_battery_stat: int
    """i64"""
    is_ui_use_allowed: int
    """u8"""
    is_learn_use_artifact: int
    """u8"""
    allow_skill_with_low_resource: int
    """u8"""
    is_use_child_pattern_description_buff_data: int
    """u8"""
    unk_pre_damage_type: int
    """u8 between `is_use_child_pattern_description_buff_data` and
    `damage_type`; reads 0 on every sampled entry."""
    damage_type: int
    """u8"""
    ui_type: int
    """u8"""
    reserve_slot_info_list: list[int]
    """ReserveSlotKey list (u32)."""
    max_level: int
    """u32"""
    skill_group_key_list: list[int]
    """u16 list."""
    buff_sustain_flag: int
    """u32"""
    dev_skill_name: bytes
    """Korean UTF-8 in vanilla."""
    dev_skill_desc: bytes
    """Korean UTF-8 in vanilla."""
    video_path: int
    """StringInfoKey (u32)."""


class BuffDataBody(TypedDict):
    """Common-base fields of a non-null BuffData. Field names mirror the
    IDA-decomp / GameMods Python parser names."""
    type_id: int
    """u8 — selects subclass tail shape (0..119)."""
    field_12: int
    """u32"""
    field_16: int
    """u32"""
    field_20: int
    """u8"""
    field_21: int
    """u8"""
    field_24: int
    """i64"""
    field_32: int
    """i64"""
    field_40: int
    """i64"""
    field_48: bytes
    """``u32 len + len bytes`` (no NUL terminator)."""
    field_56: int
    """u32"""
    field_58: int | None
    """u8 in 1.04+ format; None in 1.03."""
    field_60: int
    """u32 ConditionInfoKey through 2.05.01; 0 in the 2.06 format."""
    field_60_condition: bytes | None
    """2.06+: the raw inline condition, tag byte included (``b"\x00"`` for none, ``b"\x01"`` + bytecode); None before 2.06."""
    field_62: int
    """u32"""
    field_64: int
    """u32"""
    field_66: int
    """u32"""
    field_68: int
    """u8"""
    field_69: int
    """u8"""
    field_88: int
    """u32"""
    field_90: int
    """u32"""
    field_96_list: list[int]
    """u32 list."""
    field_128: int
    """u32"""
    field_72: int
    """u32"""
    field_76: int
    """u32"""
    field_80: int
    """u32"""
    field_84: int
    """u32"""
    field_112_list: list[int]
    """u32 list."""
    field_132: int
    """u8"""
    field_136: int
    """u32"""
    subclass_tail: bytes
    """Variable-length tail bytes — size depends on ``type_id``, found
    by brute-force probing (cached per parse). Empty for type_ids whose
    tail size is 0."""


class BuffData(TypedDict):
    flag: int
    """u8 — 0 means a real buff (``body`` is set); non-zero is a null
    entry (``body`` is None)."""
    body: BuffDataBody | None


class SkillEntry(TypedDict):
    key: int
    """u32 — skill key."""
    name: str
    """UTF-8 lossy decode of ``name_bytes`` for convenience."""
    name_bytes: bytes
    """Raw name bytes (without trailing NUL). Source of truth for
    serialisation; ``name`` may differ on non-UTF-8 input."""
    unk_post_key: int
    """u8 after the key; meaning not RE'd."""
    is_blocked: int
    """u8"""
    pad_01: bytes
    """3 bytes of padding."""
    buff_level_list: list[list[BuffData]] | None
    """Decoded `[level][buff]` matrix. ``None`` when the entry's buff
    section could not be fully decoded (mutually exclusive with
    ``buff_raw_fallback``)."""
    buff_raw_fallback: bytes | None
    """Opaque buff blob preserved for entries the probe could not
    resolve. Mutually exclusive with ``buff_level_list``."""
    post_buff: PostBuff


class SkillIndexEntry(TypedDict):
    key: int
    """u32"""
    offset: int
    """u32 — byte offset into pabgb."""


class SkillData(TypedDict):
    entries: list[SkillEntry]
    """In on-disk PABGH order (which is offset-sorted in vanilla)."""
    format: str
    """``"inline_condition"`` (2.06+), ``"with_field_58"`` (1.04 to 2.05.01) or ``"no_field_58"`` (1.03)."""
    index_order: list[SkillIndexEntry]
    """On-disk PABGH order, preserved for byte-identical roundtrip."""
    row_indices: list[int] | None
    """2.06+: the row index (u32) that opens each ``index_order`` row's body,
    in the same order. ``None`` on a table from before 2.06."""


def parse_skillinfo_from_bytes(skill_pabgb: bytes, skill_pabgh: bytes) -> SkillData:
    """Parse skill.pabgb + skill.pabgh into structured entries.

    The parser handles the 1.03 (no field_58) and 1.04+ (with field_58)
    formats, brute-forces unknown BuffData subclass-tail sizes, and falls
    back to a raw blob path for entries it cannot fully decode (the blob
    is preserved for byte-identical roundtrip via ``serialize_skillinfo``).

    Args:
        skill_pabgb: Decompressed ``skill.pabgb`` bytes.
        skill_pabgh: Decompressed ``skill.pabgh`` bytes (the index).

    Returns:
        Dict with ``entries``, ``format``, ``index_order``, ``row_indices``.

    Raises:
        ValueError: On parse errors (truncated input, malformed index, ...).
    """
    ...


def serialize_skillinfo(data: SkillData) -> tuple[bytes, bytes]:
    """Serialise back to ``(pabgh_bytes, pabgb_bytes)``.

    Pass the dict returned by ``parse_skillinfo_from_bytes`` (optionally
    with edits) — the on-disk PABGH order is taken from ``index_order``
    so the result roundtrips byte-identically when no fields are changed.
    When ``row_indices`` is a list the 2.06 layout is written (named header,
    ``[key][row index]`` body rows); when it is ``None``, the pre-2.06 one.

    Returns:
        Tuple of ``(pabgh_bytes, pabgb_bytes)``.

    Raises:
        IOError: On write failure.
        ValueError, KeyError: On malformed input dict.
    """
    ...


def extract_file_from_paz(paz_path: str, vfs_path: str) -> bytes:
    """Extract a single file by pointing at any ``.paz`` in the group directory.

    Locates the sibling ``0.pamt`` next to ``paz_path`` and routes the read to
    whichever chunk PAMT indicates (so any ``.paz`` in the directory is a valid
    pointer; the input is just used for path resolution). Useful for mod-loader
    pipelines that bundle a self-contained snapshot directory and do not want to
    assume the standard ``<game_dir>/<group>/0.paz`` layout.

    Args:
        paz_path: Path to a ``.paz`` file. Its parent directory must contain
            the matching ``0.pamt``.
        vfs_path: VFS path inside the archive, split on the last ``/``
            (e.g. ``"gamedata/binary__/client/bin/iteminfo.pabgb"``). Files in
            the archive root may be passed as a bare filename.

    Returns:
        Raw decompressed file data as bytes.

    Raises:
        IOError: If the PAMT or PAZ file cannot be read.
        ValueError: If the directory or file is not found in the PAMT.
    """
    ...


# ── Saves and lossy iteminfo ────────────────────────────────────────────────


class LossyItemInfo(TypedDict):
    items: list[ItemInfo]
    spans: list[ItemSpan]
    errors: list[dict[str, Any]]
    """One dict per recovery: ``item_start``, ``fail_offset``, ``recovered_at``, …
    A single damaged row can log many entries, so ``len(errors)`` overstates
    the number of lost items."""


def parse_iteminfo_lossy(data: bytes) -> LossyItemInfo:
    """Like :func:`parse_iteminfo_tracked`, but on an error scans forward to
    the next plausible item start and continues.

    Body only, so ``string_key`` is ``""`` on 2.06+. The 2.06 row start it
    scans for (a small key, a row index below 100,000, then a zero byte) is a
    much weaker anchor than the pre-2.06 name, so ``recovered_at`` can land
    inside a row; treat a recovery as a hint for where to look, not a row
    boundary."""
    ...


def parse_save_from_bytes(data: bytes) -> dict[str, Any]:
    """Parse a save file's raw bytes. Keys: ``header`` (128 bytes), ``version``,
    ``flags``, ``payload_size``, ``uncompressed_size``, ``nonce`` (16 bytes),
    ``hmac`` (32 bytes), ``hmac_ok`` (bool), ``body`` (decompressed bytes)."""
    ...


def parse_save_from_file(path: str) -> dict[str, Any]:
    """:func:`parse_save_from_bytes` on a file."""
    ...


def write_save_with_nonce(header: bytes, body: bytes, nonce: bytes) -> bytes:
    """Encode a save: 128-byte ``header`` (from a ``parse_save_*`` result),
    decompressed ``body`` and a 16-byte ChaCha20 ``nonce`` (the original for a
    decode-stable round trip, random for a fresh write). Re-seals whatever
    ``body`` is given; it does not re-encode decoded blocks."""
    ...


def parse_save_body_from_bytes(body: bytes) -> dict[str, Any]:
    """Schema and TOC of a decompressed save body: ``prefix`` (14 bytes),
    ``schema`` (types and fields) and ``toc`` (``prefix_zero``, ``toc_count``,
    ``stream_size``, ``entries``)."""
    ...


def decode_save_body_blocks(body: bytes) -> dict[str, Any]:
    """Decode every TOC entry of a save body into nested block dicts (fields
    with ``kind``, ``start``, ``end`` and kind-specific values)."""
    ...
