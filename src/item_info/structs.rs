use std::io::{self, Write};

#[cfg(feature = "python")]
use pyo3::exceptions::PyValueError;
#[cfg(feature = "python")]
use pyo3::prelude::*;
#[cfg(feature = "python")]
use pyo3::types::PyDict;

use super::keys::*;
use crate::binary::*;
use crate::py_binary_struct;
#[cfg(feature = "python")]
use crate::python_traits::{ToPyValue, WritePyValue, get_field};

// ── Simple structs ──────────────────────────────────────────────────────────

py_binary_struct! {
    pub struct OccupiedEquipSlotData {
        pub equip_slot_name_key: u32,
        pub equip_slot_name_index_list: CArray<u8>,
    }
}

py_binary_struct! {
    pub struct ItemIconData {
        pub icon_path: StringInfoKey,
        // Added in Crimson Desert 1.05: a second StringInfoKey paired with
        // icon_path (often holds the same texture hash as map_icon_path).
        pub icon_path_alt: StringInfoKey,
        pub check_exist_sealed_data: u8,
        pub gimmick_state_list: CArray<u32>,
        // Added in Crimson Desert 1.05: trailing flag byte.
        pub unk_flag: u8,
    }
}

py_binary_struct! {
    pub struct PassiveSkillLevel {
        pub skill: SkillKey,
        pub level: u32,
    }
}

py_binary_struct! {
    pub struct ReserveSlotTargetData {
        pub reserve_slot_info: ReserveSlotKey,
        pub condition_info: ConditionKey,
    }
}

py_binary_struct! {
    pub struct SocketMaterialItem {
        pub item: ItemKey,
        pub value: u64,
    }
}

py_binary_struct! {
    pub struct EnchantStatChange {
        pub stat: StatusKey,
        pub change_mb: i64,
    }
}

py_binary_struct! {
    pub struct EnchantLevelChange {
        pub stat: StatusKey,
        pub change_mb: i8,
    }
}

py_binary_struct! {
    pub struct EnchantStatData {
        pub max_stat_list: CArray<EnchantStatChange>,
        pub regen_stat_list: CArray<EnchantStatChange>,
        pub stat_list_static: CArray<EnchantStatChange>,
        pub stat_list_static_level: CArray<EnchantLevelChange>,
    }
}

py_binary_struct! {
    pub struct PriceFloor {
        pub price: u64,
        pub sym_no: u32,
        pub item_info_wrapper: ItemKey,
    }
}

py_binary_struct! {
    pub struct ItemPriceInfo {
        pub key: ItemKey,
        pub price: PriceFloor,
    }
}

py_binary_struct! {
    pub struct EquipmentBuff {
        pub buff: BuffKey,
        pub level: u32,
    }
}

py_binary_struct! {
    pub struct EnchantData {
        pub level: u16,
        pub enchant_stat_data: EnchantStatData,
        pub buy_price_list: CArray<ItemPriceInfo>,
        pub equip_buffs: CArray<EquipmentBuff>,
    }
}

// ── EnchantDataList (1.12 inter-element separator) ──────────────────────────
//
// Crimson Desert 1.12 inserts a `u32` (always 0) **between** consecutive
// `EnchantData` elements — N elements carry N-1 separators (element [0] has
// no leading separator; the last element has no trailing one). Verified by a
// per-boundary byte-walk vs the 1.11 binary: e.g. key=1000019's 6 enchant rows
// show the +4 only at the [1]…[5] boundaries (offsets 417/495/573/651/729),
// never before [0] (339). This is NOT a per-element field (that would be N, and
// would collide with the separate item-level `unk_pre_gimmick_visual` +4 that
// equipment/gem items also carry just past the list). Semantic role unknown —
// likely a per-pair delimiter Pearl Abyss added to the enchant table.
//
// Serialized to Python as a plain list of EnchantData dicts (the all-zero
// separators are reconstructed on write), so the items.jsonl shape is
// unchanged. The Rust round-trip stores the separators verbatim for byte
// fidelity.
#[derive(Debug)]
pub struct EnchantDataList {
    pub items: Vec<EnchantData>,
    /// `items.len().saturating_sub(1)` separators, in element order. Always
    /// zero on every 1.12 item observed; kept for exact byte round-trip.
    pub separators: Vec<u32>,
}

impl<'a> BinaryRead<'a> for EnchantDataList {
    fn read_from(data: &'a [u8], offset: &mut usize) -> io::Result<Self> {
        let count = u32::read_from(data, offset)? as usize;
        let remaining = data.len().saturating_sub(*offset);
        if count > remaining {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "EnchantDataList count {} exceeds remaining bytes {} at offset {}",
                    count, remaining, *offset,
                ),
            ));
        }
        let mut items = Vec::with_capacity(count);
        let mut separators = Vec::with_capacity(count.saturating_sub(1));
        for i in 0..count {
            if i > 0 {
                separators.push(u32::read_from(data, offset)?);
            }
            items.push(EnchantData::read_from(data, offset)?);
        }
        Ok(EnchantDataList { items, separators })
    }
}

impl BinaryWrite for EnchantDataList {
    fn write_to(&self, w: &mut dyn Write) -> io::Result<()> {
        (self.items.len() as u32).write_to(w)?;
        for (i, item) in self.items.iter().enumerate() {
            if i > 0 {
                // Reuse the stored separator when present; default to 0 (the
                // only value ever observed) when the list came from Python.
                let sep = self.separators.get(i - 1).copied().unwrap_or(0);
                sep.write_to(w)?;
            }
            item.write_to(w)?;
        }
        Ok(())
    }
}

impl<'a> BinaryReadTracked<'a> for EnchantDataList {
    fn read_tracked(
        data: &'a [u8],
        offset: &mut usize,
        path: &mut String,
        ranges: &mut Vec<FieldRange>,
    ) -> io::Result<Self> {
        let count_start = *offset;
        let count = u32::read_from(data, offset)? as usize;
        let saved = crate::binary::push_path(path, "__count__");
        ranges.push(FieldRange {
            path: path.clone(),
            start: count_start,
            end: *offset,
            ty: "EnchantDataList.count",
        });
        crate::binary::pop_path(path, saved);

        let remaining = data.len().saturating_sub(*offset);
        if count > remaining {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "EnchantDataList count {} exceeds remaining bytes {} at offset {}",
                    count, remaining, *offset,
                ),
            ));
        }

        let mut items = Vec::with_capacity(count);
        let mut separators = Vec::with_capacity(count.saturating_sub(1));
        for i in 0..count {
            if i > 0 {
                let sep_start = *offset;
                let sep = u32::read_from(data, offset)?;
                let saved = crate::binary::push_index(path, i);
                let saved2 = crate::binary::push_path(path, "__sep__");
                ranges.push(FieldRange {
                    path: path.clone(),
                    start: sep_start,
                    end: *offset,
                    ty: "EnchantDataList.sep",
                });
                crate::binary::pop_path(path, saved2);
                crate::binary::pop_path(path, saved);
                separators.push(sep);
            }
            let saved = crate::binary::push_index(path, i);
            items.push(EnchantData::read_tracked(data, offset, path, ranges)?);
            crate::binary::pop_path(path, saved);
        }
        Ok(EnchantDataList { items, separators })
    }
}

#[cfg(feature = "python")]
impl ToPyValue for EnchantDataList {
    fn to_py_value(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        use pyo3::types::{PyList, PyListMethods};
        let list = PyList::empty(py);
        for item in &self.items {
            list.append(item.to_py_value(py)?)?;
        }
        Ok(list.into_any().unbind())
    }
}

#[cfg(feature = "python")]
impl WritePyValue for EnchantDataList {
    fn write_from_py(w: &mut Vec<u8>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        use pyo3::types::{PyList, PyListMethods};
        let list = obj.cast::<PyList>()?;
        let n = list.len();
        w.extend_from_slice(&(n as u32).to_le_bytes());
        for (i, item) in list.iter().enumerate() {
            if i > 0 {
                // All observed separators are 0; reconstruct them on write.
                w.extend_from_slice(&0u32.to_le_bytes());
            }
            EnchantData::write_from_py(w, &item)?;
        }
        Ok(())
    }
}

py_binary_struct! {
    pub struct GimmickVisualPrefabData {
        pub tag_name_hash: u32,
        pub scale: [f32; 3],
        pub prefab_names: CArray<StringInfoKey>,
        pub animation_path_list: CArray<StringInfoKey>,
        pub use_gimmick_prefab: u8,
    }
}

py_binary_struct! {
    pub struct GameEventExecuteData {
        pub game_event_type: u8,
        pub player_condition: ConditionKey,
        pub target_condition: ConditionKey,
        pub event_condition: ConditionKey,
    }
}

py_binary_struct! {
    pub struct InventoryChangeData {
        pub game_event_execute_data: GameEventExecuteData,
        pub to_inventory_info: InventoryKey,
    }
}

py_binary_struct! {
    pub struct PageData<'a> {
        pub left_page_texture_path: CString<'a>,
        pub right_page_texture_path: CString<'a>,
        pub left_page_related_knowledge_info: KnowledgeKey,
        pub right_page_related_knowledge_info: KnowledgeKey,
    }
}

py_binary_struct! {
    pub struct InspectData<'a> {
        pub item_info: ItemKey,
        pub gimmick_info: GimmickInfoKey,
        pub character_info: CharacterKey,
        pub spawn_reason_hash: u32,
        pub socket_name: CString<'a>,
        pub speak_character_info: CharacterKey,
        pub inspect_target_tag: u32,
        pub reward_own_knowledge: u8,
        pub reward_knowledge_info: KnowledgeKey,
        pub item_desc: LocalizableString<'a>,
        pub board_key: u32,
        pub inspect_action_type: u8,
        pub gimmick_state_name_hash: u32,
        pub target_page_index: u32,
        pub is_left_page: u8,
        pub target_page_related_knowledge_info: KnowledgeKey,
        pub enable_read_after_reward: u8,
        pub refer_to_left_page_inspect_data: u8,
        pub inspect_effect_info_key: EffectKey,
        pub inspect_complete_effect_info_key: EffectKey,
    }
}

py_binary_struct! {
    pub struct InspectAction<'a> {
        pub action_name_hash: u32,
        pub catch_tag_name_hash: u32,
        pub catcher_socket_name: CString<'a>,
        pub catch_target_socket_name: CString<'a>,
    }
}

py_binary_struct! {
    pub struct ItemInfoSharpnessData {
        pub max_sharpness: u16,
        pub craft_tool_info: CraftToolKey,
        pub stat_data: EnchantStatData,
    }
}

py_binary_struct! {
    pub struct ItemBundleData {
        pub count_mb: u64,
        pub key: GimmickInfoKey,
    }
}

py_binary_struct! {
    pub struct UnitData<'a> {
        pub ui_component: CString<'a>,
        pub minimum: u32,
        pub icon_path: StringInfoKey,
        // 1.10 added a new u32 here. For Money_Copper's two
        // MoneyUnitEntries the value is `0xc52007c6` (decimal 3,306,518,982);
        // its in-game name is unknown — likely another StringInfoKey-shaped
        // hash. Confirmed by hand-decoding Money_Copper bytes (both entries
        // gained +4 B at the same UnitData-relative position, net item delta
        // +4 = -4 from money_icon_path removal + 2 × +4 from this field).
        pub unk_post_icon_path: u32,
        pub item_name: LocalizableString<'a>,
        pub item_desc: LocalizableString<'a>,
    }
}

py_binary_struct! {
    pub struct MoneyUnitEntry<'a> {
        pub key: u32,
        pub value: UnitData<'a>,
    }
}

py_binary_struct! {
    pub struct MoneyTypeDefine<'a> {
        pub price_floor_value: u64,
        pub unit_data_list_map: CArray<MoneyUnitEntry<'a>>,
    }
}

py_binary_struct! {
    pub struct PrefabData {
        pub prefab_names: CArray<StringInfoKey>,
        pub equip_slot_list: CArray<u16>,
        pub tribe_gender_list: CArray<StringInfoKey>,
        pub is_craft_material: u8,
    }
}

py_binary_struct! {
    /// 1.13: unified prefab + gimmick-visual element. In 1.13 the separate
    /// `prefab_data_list` and `gimmick_visual_prefab_data_list` (both of which
    /// sat in the item middle in 1.05–1.12) were **merged into one list**
    /// relocated to the item END (after `repair_data_list`). The merged element
    /// interleaves the fields of both source structs: a `scale` (default
    /// `(1,1,1)` for prefab-origin rows), the prefab/animation name lists, the
    /// equip-slot + tribe-gender lists, and a 3-byte flag tail (`is_craft`
    /// (prefab) / `use_gimmick` (gimmick) / a new byte; values seen in 0..=3).
    /// Verified byte-perfect on all 6,508 1.13 items. See the "ItemInfo (1.13)"
    /// header in `item.rs`.
    ///
    /// 1.18 inserted `unk_pre_is_craft_material` before the flag tail — the
    /// sole iteminfo layout change of that patch. See the "ItemInfo (1.18)"
    /// header in `item.rs`.
    pub struct MergedPrefabVisualData {
        pub scale: [f32; 3],
        pub prefab_names: CArray<StringInfoKey>,
        pub animation_path_list: CArray<StringInfoKey>,
        pub equip_slot_list: CArray<u16>,
        pub tribe_gender_list: CArray<StringInfoKey>,
        // 1.18: new u32 between `tribe_gender_list` and the flag tail. Reads
        // the constant `0xeac5e173` on all 12,274 elements of all 6,573 items
        // (zero exceptions) — the same "empty string" Jenkins
        // sentinel that used to sit in the `money_icon_path` field removed in
        // 1.10, so this is very likely a string/prefab name hash that ships
        // unset. Typed as a bare u32 rather than StringInfoKey until a
        // populated value shows up to confirm the referent.
        pub unk_pre_is_craft_material: u32,
        // 3-byte flag tail. Provisional names: 1.12's PrefabData ended with
        // `is_craft_material` and GimmickVisualPrefabData with
        // `use_gimmick_prefab`; the merged element carries both plus a new byte
        // (values 0..=3). Exact mapping pending RE — kept as three u8s so the
        // round-trip stays byte-exact regardless.
        pub is_craft_material: u8,
        pub use_gimmick_prefab: u8,
        pub unk_post_flag: u8,
    }
}

py_binary_struct! {
    pub struct DockingChildData<'a> {
        pub gimmick_info_key: GimmickInfoKey,
        pub character_key: CharacterKey,
        pub item_key: ItemKey,
        pub attach_parent_socket_name: CString<'a>,
        pub attach_child_socket_name: CString<'a>,
        pub docking_tag_name_hash: [u32; 4],
        pub docking_equip_slot_no: u16,
        pub spawn_distance_level: u32,
        pub is_item_equip_docking_gimmick: u8,
        pub send_damage_to_parent: u8,
        pub is_body_part: u8,
        pub docking_type: u8,
        pub is_summoner_team: u8,
        pub is_player_only: u8,
        pub is_npc_only: ConditionKey,
        pub is_sync_break_parent: u8,
        pub hit_part: u8,
        pub detected_by_npc: u8,
        pub is_bag_docking: u8,
        pub enable_collision: u8,
        pub disable_collision_with_other_gimmick: u8,
        pub docking_slot_key: CString<'a>,
        pub inherit_summoner: u8,
        pub summon_tag_name_hash: [u32; 4],
        // Crimson Desert 1.08 added a trailing `unk_post_summon_tag: u8` here;
        // **1.16 removed it again**. It read 0x00 on every item that ever
        // carried it, matching the reserved-placeholder reading in the 1.08
        // note — the slot was retired rather than ever populated.
        //
        // The removal is invisible on items with `docking_child_data.tag = 0`
        // (the struct isn't read at all), so it presents as a *conditional*
        // 1-byte drift: in 1.16 exactly the 391 items with
        // `docking_child_data.__tag__ != 0` lose a byte and every other item
        // is unaffected. That discriminator partitions the 6,581-item table
        // perfectly (fp=0, fn=0), which is what pinned the field. See the
        // "ItemInfo (1.16)" header in `item.rs`.
    }
}

py_binary_struct! {
    /// Element of the 1.16 `unk_pre_respawn_data_list` (28 B).
    ///
    /// New in Crimson Desert 1.16, in the block inserted immediately before
    /// `respawn_time_seconds`. Only 14 of the 6,581 items carry a non-empty
    /// list — housing/prop gimmick items (`Item_cabinet_02_index01`,
    /// `Collection_Prop_Plate_0003`, …) and large food items (`Rice_XLarge`,
    /// `Sinseollo_XLarge`, `Braised_Fish_Large`, …), with one or two elements.
    ///
    /// Across the 23 elements in the table: `unk_a` is only ever 11, 12 or 13;
    /// `unk_b == unk_c` on 23/23 and `unk_d == unk_c + 1` on 23/23, which reads
    /// like a `(kind, min, cur, max)` tuple. Sample rows: `(13, 50, 50, 51)`
    /// (`Item_gimmick_alchemy_globe_0002`), `(11, 8, 8, 9)` + `(12, 12, 12, 13)`
    /// (`Rice_XLarge`), `(11, 279, 279, 280)` + `(12, 465, 465, 466)`
    /// (`Meat_Fish_Vegetable_Egg_XLarge`).
    ///
    /// The three 8-byte fields could equally be six u32s — the high half of
    /// each reads 0 on all 23 elements — but u64 matches the `count_mb` /
    /// `max_stack_count` precedent elsewhere in this format. Either split is
    /// byte-identical, so the round-trip does not disambiguate them. Semantic
    /// role unknown.
    pub struct UnkPreRespawnData {
        pub unk_a: u32,
        pub unk_b: u64,
        pub unk_c: u64,
        pub unk_d: u64,
    }
}

py_binary_struct! {
    pub struct PatternParamString<'a> {
        pub flag: u8,
        pub unk_flag_2: u8,
        pub unk_value: [u32; 2],
        pub param_string: CString<'a>,
    }
}

py_binary_struct! {
    pub struct PatternDescriptionData<'a> {
        pub pattern_description_info: u32,
        pub param_string_list: CArray<PatternParamString<'a>>,
    }
}

py_binary_struct! {
    pub struct RepairData {
        pub resource_item_info: ItemKey,
        pub repair_value: u16,
        pub repair_style: u8,
        pub resource_item_count: u64,
    }
}

// ── SubItem (variant) ───────────────────────────────────────────────────────

#[derive(Debug)]
pub enum SubItemValue {
    Item(ItemKey),
    Character(CharacterKey),
    Gimmick(GimmickInfoKey),
    None,
}

#[derive(Debug)]
pub struct SubItem {
    pub type_id: u8,
    pub value: SubItemValue,
}

impl<'a> BinaryRead<'a> for SubItem {
    fn read_from(data: &'a [u8], offset: &mut usize) -> io::Result<Self> {
        let type_id = u8::read_from(data, offset)?;
        let value = match type_id {
            0 => SubItemValue::Item(ItemKey::read_from(data, offset)?),
            3 => SubItemValue::Character(CharacterKey::read_from(data, offset)?),
            9 => SubItemValue::Gimmick(GimmickInfoKey::read_from(data, offset)?),
            // Crimson Desert 1.05: tag 15 is a new "None"-style variant
            // (no payload), seen alongside the existing tag 14.
            // 1.12: tag 16 — items that read tag 15 (None) in 1.11 now read
            // tag 16; still payload-free (verified byte-aligned vs 1.11), so
            // it joins the None arm. Affects both SubItem sites.
            14..=17 => SubItemValue::None,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unknown SubItem type: {}", type_id),
                ));
            }
        };
        Ok(SubItem { type_id, value })
    }
}

impl<'a> BinaryReadTracked<'a> for SubItem {
    fn read_tracked(
        data: &'a [u8],
        offset: &mut usize,
        path: &mut String,
        ranges: &mut Vec<FieldRange>,
    ) -> io::Result<Self> {
        let saved = push_path(path, "type_id");
        let type_id = u8::read_tracked(data, offset, path, ranges)?;
        pop_path(path, saved);

        let saved = push_path(path, "value");
        let value = match type_id {
            0 => SubItemValue::Item(ItemKey::read_tracked(data, offset, path, ranges)?),
            3 => SubItemValue::Character(CharacterKey::read_tracked(data, offset, path, ranges)?),
            9 => SubItemValue::Gimmick(GimmickInfoKey::read_tracked(data, offset, path, ranges)?),
            14..=17 => SubItemValue::None,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unknown SubItem type: {}", type_id),
                ));
            }
        };
        pop_path(path, saved);
        Ok(SubItem { type_id, value })
    }
}

impl BinaryWrite for SubItem {
    fn write_to(&self, w: &mut dyn Write) -> io::Result<()> {
        self.type_id.write_to(w)?;
        match &self.value {
            SubItemValue::Item(k) => k.write_to(w),
            SubItemValue::Character(k) => k.write_to(w),
            SubItemValue::Gimmick(k) => k.write_to(w),
            SubItemValue::None => Ok(()),
        }
    }
}

#[cfg(feature = "python")]
impl ToPyValue for SubItem {
    fn to_py_value(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let d = PyDict::new(py);
        d.set_item("type_id", self.type_id)?;
        match &self.value {
            SubItemValue::Item(k) => d.set_item("value", k.0)?,
            SubItemValue::Character(k) => d.set_item("value", k.0)?,
            SubItemValue::Gimmick(k) => d.set_item("value", k.0)?,
            SubItemValue::None => d.set_item("value", py.None())?,
        };
        Ok(d.into_any().unbind())
    }
}

#[cfg(feature = "python")]
impl WritePyValue for SubItem {
    fn write_from_py(w: &mut Vec<u8>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        let d = obj.cast::<PyDict>()?;
        let type_id: u8 = get_field(d, "type_id")?.extract()?;
        w.push(type_id);
        match type_id {
            0 | 3 | 9 => {
                let v: u32 = get_field(d, "value")?.extract()?;
                w.extend_from_slice(&v.to_le_bytes());
            }
            14..=17 => {}
            _ => {
                return Err(PyValueError::new_err(format!(
                    "invalid SubItem type_id: {}",
                    type_id
                )));
            }
        }
        Ok(())
    }
}

// ── DropDefaultData ─────────────────────────────────────────────────────────

py_binary_struct! {
    pub struct DropDefaultData {
        pub drop_enchant_level: u16,
        pub socket_item_list: CArray<ItemKey>,
        pub add_socket_material_item_list: CArray<SocketMaterialItem>,
        pub default_sub_item: SubItem,
        pub socket_valid_count: u8,
        pub use_socket: u8,
    }
}

// ── SealableItemInfo (variant) ──────────────────────────────────────────────

#[derive(Debug)]
pub enum SealableValue<'a> {
    Item(ItemKey),
    Gimmick(GimmickInfoKey),
    String(CString<'a>),
    Character(CharacterKey),
    Tribe(TribeInfoKey),
}

#[derive(Debug)]
pub struct SealableItemInfo<'a> {
    pub type_tag: u8,
    pub item_key: ItemKey,
    pub unknown0: u64,
    pub value: SealableValue<'a>,
}

impl<'a> BinaryRead<'a> for SealableItemInfo<'a> {
    fn read_from(data: &'a [u8], offset: &mut usize) -> io::Result<Self> {
        let type_tag = u8::read_from(data, offset)?;
        let item_key = ItemKey::read_from(data, offset)?;
        let unknown0 = u64::read_from(data, offset)?;
        let value = match type_tag {
            0 => SealableValue::Item(ItemKey::read_from(data, offset)?),
            1 => SealableValue::Gimmick(GimmickInfoKey::read_from(data, offset)?),
            2 => SealableValue::String(CString::read_from(data, offset)?),
            3 => SealableValue::Character(CharacterKey::read_from(data, offset)?),
            4 => SealableValue::Tribe(TribeInfoKey::read_from(data, offset)?),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unknown SealableItemInfo type: {}", type_tag),
                ));
            }
        };
        Ok(SealableItemInfo {
            type_tag,
            item_key,
            unknown0,
            value,
        })
    }
}

impl<'a> BinaryReadTracked<'a> for SealableItemInfo<'a> {
    fn read_tracked(
        data: &'a [u8],
        offset: &mut usize,
        path: &mut String,
        ranges: &mut Vec<FieldRange>,
    ) -> io::Result<Self> {
        let saved = push_path(path, "type_tag");
        let type_tag = u8::read_tracked(data, offset, path, ranges)?;
        pop_path(path, saved);

        let saved = push_path(path, "item_key");
        let item_key = ItemKey::read_tracked(data, offset, path, ranges)?;
        pop_path(path, saved);

        let saved = push_path(path, "unknown0");
        let unknown0 = u64::read_tracked(data, offset, path, ranges)?;
        pop_path(path, saved);

        let saved = push_path(path, "value");
        let value = match type_tag {
            0 => SealableValue::Item(ItemKey::read_tracked(data, offset, path, ranges)?),
            1 => SealableValue::Gimmick(GimmickInfoKey::read_tracked(data, offset, path, ranges)?),
            2 => SealableValue::String(CString::read_tracked(data, offset, path, ranges)?),
            3 => SealableValue::Character(CharacterKey::read_tracked(data, offset, path, ranges)?),
            4 => SealableValue::Tribe(TribeInfoKey::read_tracked(data, offset, path, ranges)?),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unknown SealableItemInfo type: {}", type_tag),
                ));
            }
        };
        pop_path(path, saved);
        Ok(SealableItemInfo {
            type_tag,
            item_key,
            unknown0,
            value,
        })
    }
}

impl BinaryWrite for SealableItemInfo<'_> {
    fn write_to(&self, w: &mut dyn Write) -> io::Result<()> {
        self.type_tag.write_to(w)?;
        self.item_key.write_to(w)?;
        self.unknown0.write_to(w)?;
        match &self.value {
            SealableValue::Item(k) => k.write_to(w),
            SealableValue::Gimmick(k) => k.write_to(w),
            SealableValue::String(s) => s.write_to(w),
            SealableValue::Character(k) => k.write_to(w),
            SealableValue::Tribe(k) => k.write_to(w),
        }
    }
}

#[cfg(feature = "python")]
impl ToPyValue for SealableItemInfo<'_> {
    fn to_py_value(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let d = PyDict::new(py);
        d.set_item("type_tag", self.type_tag)?;
        d.set_item("item_key", self.item_key.0)?;
        d.set_item("unknown0", self.unknown0)?;
        match &self.value {
            SealableValue::Item(k) => d.set_item("value", k.0)?,
            SealableValue::Gimmick(k) => d.set_item("value", k.0)?,
            SealableValue::String(s) => d.set_item("value", s.data)?,
            SealableValue::Character(k) => d.set_item("value", k.0)?,
            SealableValue::Tribe(k) => d.set_item("value", k.0)?,
        };
        Ok(d.into_any().unbind())
    }
}

#[cfg(feature = "python")]
impl WritePyValue for SealableItemInfo<'_> {
    fn write_from_py(w: &mut Vec<u8>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
        let d = obj.cast::<PyDict>()?;
        let type_tag: u8 = get_field(d, "type_tag")?.extract()?;
        let item_key: u32 = get_field(d, "item_key")?.extract()?;
        let unknown0: u64 = get_field(d, "unknown0")?.extract()?;
        w.push(type_tag);
        w.extend_from_slice(&item_key.to_le_bytes());
        w.extend_from_slice(&unknown0.to_le_bytes());
        let value_obj = get_field(d, "value")?;
        match type_tag {
            0 | 1 | 3 | 4 => {
                let v: u32 = value_obj.extract()?;
                w.extend_from_slice(&v.to_le_bytes());
            }
            2 => {
                let s: String = value_obj.extract()?;
                w.extend_from_slice(&(s.len() as u32).to_le_bytes());
                w.extend_from_slice(s.as_bytes());
            }
            _ => {
                return Err(PyValueError::new_err(format!(
                    "invalid sealable type_tag: {}",
                    type_tag
                )));
            }
        }
        Ok(())
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sub_item_none_roundtrip() {
        let bytes = [14u8];
        let mut offset = 0;
        let si = SubItem::read_from(&bytes, &mut offset).unwrap();
        assert_eq!(offset, 1);
        assert_eq!(si.type_id, 14);

        let mut out = Vec::new();
        si.write_to(&mut out).unwrap();
        assert_eq!(out, bytes);
    }

    #[test]
    fn test_sub_item_item_key_roundtrip() {
        let mut bytes = vec![0u8];
        bytes.extend_from_slice(&42u32.to_le_bytes());
        let mut offset = 0;
        let si = SubItem::read_from(&bytes, &mut offset).unwrap();
        assert_eq!(offset, 5);
        assert_eq!(si.type_id, 0);

        let mut out = Vec::new();
        si.write_to(&mut out).unwrap();
        assert_eq!(out, bytes);
    }

    #[test]
    fn test_sealable_item_info_type0_roundtrip() {
        let mut bytes = Vec::new();
        bytes.push(0);
        bytes.extend_from_slice(&100u32.to_le_bytes());
        bytes.extend_from_slice(&999u64.to_le_bytes());
        bytes.extend_from_slice(&200u32.to_le_bytes());
        let mut offset = 0;
        let si = SealableItemInfo::read_from(&bytes, &mut offset).unwrap();
        assert_eq!(offset, bytes.len());

        let mut out = Vec::new();
        si.write_to(&mut out).unwrap();
        assert_eq!(out, bytes);
    }

    #[test]
    fn test_sealable_item_info_type2_string_roundtrip() {
        let mut bytes = Vec::new();
        bytes.push(2);
        bytes.extend_from_slice(&100u32.to_le_bytes());
        bytes.extend_from_slice(&0u64.to_le_bytes());
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(b"test");
        let mut offset = 0;
        let si = SealableItemInfo::read_from(&bytes, &mut offset).unwrap();
        assert_eq!(offset, bytes.len());

        let mut out = Vec::new();
        si.write_to(&mut out).unwrap();
        assert_eq!(out, bytes);
    }

    #[test]
    fn test_drop_default_data_roundtrip() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.push(14);
        bytes.push(0);
        bytes.push(0);

        let mut offset = 0;
        let dd = DropDefaultData::read_from(&bytes, &mut offset).unwrap();
        assert_eq!(offset, bytes.len());

        let mut out = Vec::new();
        dd.write_to(&mut out).unwrap();
        assert_eq!(out, bytes);
    }
}
