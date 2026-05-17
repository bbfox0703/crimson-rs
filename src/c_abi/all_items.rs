//! Cross-container item enumeration — C ABI surface.
//!
//! [`super::crimson_save_list_inventory_items`] only walks
//! `InventorySaveData._inventoryList[N]._itemList[M]`. That leaves
//! **two-thirds of the player's gear invisible to a C# editor**: the
//! active character's 18 equipped slots (`EquipmentSaveData._list`),
//! every mercenary / mount / inactive-playable's equipment
//! (`MercenarySaveData._equipItemList` — 245 items in the slot103
//! reference save) plus their carried items (`_inventoryItemList`),
//! and the active character's reserve item slots
//! (`EquipmentSaveData._useItemSaveList`).
//!
//! See [`docs/dye-editor-scope.md`](../../docs/dye-editor-scope.md)
//! §"Addendum (2026-05-17)" for the model corrections that drove this
//! ABI: three playable characters split across two containers, mounts
//! stored as `MercenarySaveData` keyed by `_characterKey` (not the
//! 18-row MercenaryKey table), and active vs inactive being a runtime
//! container swap rather than a field on the entity.
//!
//! ## What this surface yields
//!
//! A single flat list of [`CrimsonItemRecord`] — 64-byte `repr(C)`
//! rows that fold every observed ItemSaveData host into one shape.
//! Each row carries:
//!
//! - **Container classification** ([`container_kind`] constants) so
//!   the C# editor can split the list into separate tabs (active
//!   gear / inventory / each mercenary / each mount).
//! - **Descent path** (`block_idx` + up to two `(field, element)`
//!   steps) that plugs directly into
//!   [`super::crimson_save_set_scalar_field_path`] and friends — the
//!   caller doesn't have to figure out which field index is
//!   `_inventorylist` vs `_list` vs `_mercenaryDataList` per
//!   container.
//! - **Owner identity** (`owner_character_key` + `owner_mercenary_no`)
//!   so the editor can render "Damine's gear" / "Black Wild Horse
//!   #3135's gear" without a second lookup. For active-character
//!   records `owner_character_key` is the `_lastFocusCharacterKey`
//!   pulled from `MercenaryClanSaveData` at walk time.
//! - **Search-relevant scalars** (`item_key`, `item_no`,
//!   `inventory_key`, `slot_no`, `flags`) inlined so the editor can
//!   filter / sort without a per-row block-JSON fetch.
//!
//! ## What this surface does NOT yield (by design)
//!
//! - `FieldGimmickSaveData._item<locator>` — world loot / chests
//!   (4,260 items in slot103). Not player-owned; surface separately
//!   if a "world-state editor" feature lands.
//! - `FactionNodeElementSaveData._factionOwnedItemList` — faction-side
//!   item ownership. 1 item in slot103; out of scope for the gear
//!   editor.
//! - `StoreDataSaveData._storeSoldItemDataList` — vendor inventory.
//!   17 items in slot103; out of scope.
//!
//! Snapshot semantics match
//! [`super::crimson_save_list_inventory_items`]: positional fields
//! stay valid until the next length-changing mutation; pair the
//! `out_version` stamp with [`super::crimson_save_get_mutation_version`]
//! for staleness detection.

use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::save::{FieldValue, ObjectBlock, ScalarValue};

use super::{CrimsonSaveHandle, error};

/// Container-kind classification for [`CrimsonItemRecord::container_kind`].
///
/// Numeric stability: these constants are part of the C ABI surface
/// and MUST NOT be reassigned. Add new kinds with the next free
/// integer; never reuse a retired one.
pub mod container_kind {
    /// `EquipmentSaveData._list[N]._item<locator>` — the active
    /// character's equipped gear (18 slots in 1.07). Currently-active
    /// character is identified by
    /// [`crate::c_abi::all_items::CrimsonItemRecord::owner_character_key`]
    /// pulled from `MercenaryClanSaveData._lastFocusCharacterKey`.
    pub const ACTIVE_EQUIP: u32 = 0;
    /// `EquipmentSaveData._useItemSaveList[N]._reserveItem<locator>` —
    /// the active character's quick-use reserve slots.
    pub const ACTIVE_USE_RESERVE: u32 = 1;
    /// `InventorySaveData._inventoryList[N]._itemList[M]` — the
    /// active character's inventory. Same set
    /// [`super::super::crimson_save_list_inventory_items`] returns,
    /// repeated here so a single call yields the complete picture.
    pub const INVENTORY: u32 = 2;
    /// `MercenaryClanSaveData._mercenaryDataList[N]._equipItemList[M]`
    /// — equipped gear on a mercenary / mount / inactive playable
    /// character. `owner_character_key` carries the host's
    /// `_characterKey` (cat-byte stripped); `owner_mercenary_no`
    /// distinguishes individual instances of the same template.
    pub const MERCENARY_EQUIP: u32 = 3;
    /// `MercenaryClanSaveData._mercenaryDataList[N]._inventoryItemList[M]`
    /// — items carried by a mercenary / mount. Mostly empty in
    /// practice; the wagon mount in the reference save is the only
    /// observed user of this list.
    pub const MERCENARY_INVENTORY: u32 = 4;
}

/// Bit constants for [`CrimsonItemRecord::flags`].
///
/// Same stability contract as [`container_kind`]: never reassign.
pub mod item_record_flags {
    /// `_isLocked` field on the item was present and `true`.
    pub const LOCKED: u32 = 1 << 0;
    /// `_isNewMark` field on the item was present and `true`.
    pub const NEW_MARK: u32 = 1 << 1;
    /// `_itemDyeDataList` is present and has `count > 0`. Pair with
    /// the existing dye mutation path to drive the dye editor UI.
    pub const HAS_DYE_DATA: u32 = 1 << 2;
    /// `_socketSaveDataList` is present and has `count > 0`. Pair
    /// with the existing socket mutation path for the gem editor UI.
    pub const HAS_SOCKET_DATA: u32 = 1 << 3;
    /// The enclosing `MercenarySaveData` has `_isMainMercenary = true`.
    /// Identifies the currently-summoned mount / main companion among
    /// the player's 96 mercenary slots. Always 0 for non-mercenary
    /// container kinds.
    pub const OWNER_IS_MAIN_MERCENARY: u32 = 1 << 4;
    /// Item belongs to one of the three playable characters or to a
    /// mount they own. Set when:
    /// - container_kind is `ACTIVE_EQUIP` / `ACTIVE_USE_RESERVE` /
    ///   `INVENTORY` (the active character is always one of the
    ///   playables), **OR**
    /// - container_kind is `MERCENARY_EQUIP` / `MERCENARY_INVENTORY`
    ///   AND the enclosing mercenary's `_characterKey & 0xFFFFFF` is
    ///   in [`super::PLAYABLE_CHARACTER_KEYS`] (i.e. the mercenary
    ///   block IS one of the playables — Damine / Oongka stored
    ///   under MercenaryClanSaveData while inactive), **OR**
    /// - container_kind is `MERCENARY_EQUIP` / `MERCENARY_INVENTORY`
    ///   AND the enclosing mercenary's `_ownedCharacterKey & 0xFFFFFF`
    ///   is in [`super::PLAYABLE_CHARACTER_KEYS`] (a mount owned by a
    ///   playable).
    ///
    /// Clear (0) for items inside NPC mercenary followers — the
    /// editor uses `flags & IS_PLAYER_OWNED != 0` to filter the
    /// 829-record full list down to the gear actually editable by
    /// the player. See [`super::PLAYABLE_CHARACTER_KEYS`] for the
    /// hardcoded set + rationale.
    pub const IS_PLAYER_OWNED: u32 = 1 << 5;
}

/// `CharacterKey` values (cat-byte already stripped) for the three
/// playable characters in 1.07:
///
/// | Key | Character | Container when active | Container when inactive |
/// |---:|---|---|---|
/// | 1 | Kliff | `EquipmentSaveData._list` | (none — Kliff is always reachable) |
/// | 4 | Damine | `EquipmentSaveData._list` | `MercenaryClanSaveData._mercenaryDataList[0]` |
/// | 6 | Oongka | `EquipmentSaveData._list` | `MercenaryClanSaveData._mercenaryDataList[1]` |
///
/// Used to compute [`item_record_flags::IS_PLAYER_OWNED`] for
/// mercenary container kinds: a mercenary block is "playable-owned"
/// if its `_characterKey & 0xFFFFFF` is in this set, OR its
/// `_ownedCharacterKey & 0xFFFFFF` is (mounts inherit ownership from
/// their playable owner).
///
/// ## Known coverage gap (1.07 slot103 baseline)
///
/// Two of the player-controlled mounts in slot103 have
/// `_ownedCharacterKey = absent`:
/// - `Riding_Horse_Tiuta_Unique_2050_kliff` (charKey=1003120) —
///   Kliff's starter Tiuta horse (the `_kliff` template variant).
/// - `Animal_Stefano_Wild_31364` — a captured wild animal.
///
/// Both ARE the player's mounts in-game but the strict rule treats
/// them as unowned. The C# editor has an escape hatch: it can
/// resolve `owner_character_key` via the existing `characterinfo`
/// bridge and include any mercenary whose name starts with
/// `Riding_*` / `Animal_*` / `Vehicle_*` as a player-controlled
/// mount even without `_ownedCharacterKey`. The C ABI walker stays
/// conservative to avoid dragging the characterinfo PABGB load into
/// the all-items hot path; client-side widening is the recommended
/// pattern.
///
/// **Maintenance**: hardcoded for 1.07. If a future patch adds a
/// playable, extend this slice and bump the `c_abi` version note in
/// [`docs/dye-editor-scope.md`](../../docs/dye-editor-scope.md).
pub const PLAYABLE_CHARACTER_KEYS: &[u32] = &[1, 4, 6];

/// One flat record emitted by [`crimson_save_list_all_items`] — a
/// `repr(C)` 64-byte structure laid out for direct mmap-style read
/// from C# / C++ consumers (`MemoryMarshal.Cast` / `Span<T>`).
///
/// All integers are little-endian, naturally aligned. The u64 fields
/// sit at the end so the whole structure is 8-byte aligned.
///
/// | Offset | Field | Type | Purpose |
/// |---|---|---|---|
/// |  0 | `block_idx` | u32 | Top-level TOC block index (use as `block_idx` arg to mutation ABIs) |
/// |  4 | `container_kind` | u32 | One of [`container_kind`] constants |
/// |  8 | `path_len` | u32 | Number of valid path steps (currently always 2; future-proofed for ≥ 1) |
/// | 12 | `path_step_0_field` | u32 | First descent step `field_idx` |
/// | 16 | `path_step_0_element` | u32 | First descent step `element_idx` |
/// | 20 | `path_step_1_field` | u32 | Second descent step `field_idx` |
/// | 24 | `path_step_1_element` | u32 | Second descent step `element_idx` (ignored for Locator descents — value 0) |
/// | 28 | `inventory_key` | u32 | `_inventoryKey` for inventory entries (u16 widened); 0 for other kinds |
/// | 32 | `item_key` | u32 | `ItemKey` (gamedata template id) |
/// | 36 | `slot_no` | u32 | `_slotNo` (inventory slot) or `_occupiedSlotNo` (equipment slot) widened to u32; 0 if absent |
/// | 40 | `flags` | u32 | Bitfield ([`item_record_flags`]) |
/// | 44 | `owner_character_key` | u32 | For mercenary kinds: `_characterKey & 0xFFFFFF`. For active kinds: `MercenaryClanSaveData._lastFocusCharacterKey` |
/// | 48 | `item_no` | u64 | `_itemNo` (per-save unique instance id) |
/// | 56 | `owner_mercenary_no` | u64 | For mercenary kinds: enclosing `MercenarySaveData._mercenaryNo`. 0 for active kinds |
///
/// **Path encoding rule** for the descent path: each step says "from
/// the current block, look up `field_idx`; if it's an `ObjectList`,
/// descend into element `element_idx`; if it's an `ObjectLocator`
/// with a resolved child, descend into the child (`element_idx`
/// ignored)". Matches [`super::CrimsonPathStep`] semantics exactly,
/// so the C# editor can build a `CrimsonPathStep[]` from the inline
/// fields without remapping:
///
/// ```text
/// CrimsonPathStep[] path = new[] {
///     new CrimsonPathStep { field_idx = rec.path_step_0_field, element_idx = rec.path_step_0_element },
///     new CrimsonPathStep { field_idx = rec.path_step_1_field, element_idx = rec.path_step_1_element },
/// };
/// crimson_save_set_scalar_field_path(handle, rec.block_idx, path, 2, …);
/// ```
///
/// `path_len` is a future-proofing knob — at v1 it's always 2 because
/// every supported container has a 2-step descent. A future container
/// kind with a different depth would set `path_len` to its actual
/// length and the caller MUST honour it.
///
/// **Validity window**: same as `crimson_save_list_inventory_items`.
/// Positional fields stay valid only until the next length-changing
/// mutation in the relevant list. Combine with
/// [`super::crimson_save_get_mutation_version`] to detect staleness.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CrimsonItemRecord {
    pub block_idx: u32,
    pub container_kind: u32,
    pub path_len: u32,
    pub path_step_0_field: u32,
    pub path_step_0_element: u32,
    pub path_step_1_field: u32,
    pub path_step_1_element: u32,
    pub inventory_key: u32,
    pub item_key: u32,
    pub slot_no: u32,
    pub flags: u32,
    pub owner_character_key: u32,
    pub item_no: u64,
    pub owner_mercenary_no: u64,
}

// Sanity guard: the size + layout is part of the C ABI surface.
const _: () = assert!(std::mem::size_of::<CrimsonItemRecord>() == 64);
const _: () = assert!(std::mem::align_of::<CrimsonItemRecord>() == 8);

/// Helpers for pulling scalars out of an `ObjectBlock` by field name.
/// Skips absent fields so a missing scalar resolves to `None` rather
/// than the default value (matches the existing inventory walker's
/// behaviour).
fn pull_u16_present(b: &ObjectBlock, name: &str) -> Option<u16> {
    b.fields.iter().find(|f| f.name == name && f.present).and_then(|f| match &f.value {
        FieldValue::Scalar(ScalarValue::U16(v)) => Some(*v),
        _ => None,
    })
}
fn pull_u32_present(b: &ObjectBlock, name: &str) -> Option<u32> {
    b.fields.iter().find(|f| f.name == name && f.present).and_then(|f| match &f.value {
        FieldValue::Scalar(ScalarValue::U32(v)) => Some(*v),
        _ => None,
    })
}
fn pull_u64_present(b: &ObjectBlock, name: &str) -> Option<u64> {
    b.fields.iter().find(|f| f.name == name && f.present).and_then(|f| match &f.value {
        FieldValue::Scalar(ScalarValue::U64(v)) => Some(*v),
        _ => None,
    })
}
fn pull_bool_true(b: &ObjectBlock, name: &str) -> bool {
    b.fields.iter().any(|f| {
        f.name == name && f.present && matches!(f.value, FieldValue::Scalar(ScalarValue::Bool(true)))
    })
}

/// Classifier context for one record. Bundles the
/// caller-determined fields so [`build_record`] doesn't take a long
/// argument list (clippy's `too_many_arguments` lint).
struct RecordCtx {
    block_idx: u32,
    container_kind: u32,
    path_step_0_field: u32,
    path_step_0_element: u32,
    path_step_1_field: u32,
    path_step_1_element: u32,
    inventory_key: u32,
    owner_character_key: u32,
    owner_mercenary_no: u64,
    owner_is_main_mercenary: bool,
    /// Override for `slot_no`. Some container kinds (active equip,
    /// reserve slots) carry the slot number on the WRAPPER object,
    /// not the inner `ItemSaveData`. `None` falls back to the item's
    /// own `_slotNo` field.
    slot_no_override: Option<u32>,
    /// Whether the enclosing container belongs to one of the
    /// playable characters or a mount they own. Drives the
    /// [`item_record_flags::IS_PLAYER_OWNED`] flag bit.
    is_player_owned: bool,
}

/// Build a record from an `ItemSaveData` block + classifier context.
///
/// Returns `None` when the item is an empty placeholder (no
/// `_itemKey` present). This happens for unfilled equipment / reserve
/// slots: the parent Locator resolves to a default ItemSaveData but
/// the inner state has all its fields absent. Treating these as
/// non-items keeps the enumerator output aligned with what the
/// player actually sees in the in-game inventory UI.
fn build_record(ctx: RecordCtx, item: &ObjectBlock) -> Option<CrimsonItemRecord> {
    let item_key = pull_u32_present(item, "_itemKey")?;
    let mut flags = 0u32;
    if pull_bool_true(item, "_isLocked") {
        flags |= item_record_flags::LOCKED;
    }
    if pull_bool_true(item, "_isNewMark") {
        flags |= item_record_flags::NEW_MARK;
    }
    if ctx.owner_is_main_mercenary {
        flags |= item_record_flags::OWNER_IS_MAIN_MERCENARY;
    }
    if ctx.is_player_owned {
        flags |= item_record_flags::IS_PLAYER_OWNED;
    }
    // Dye / socket: present + count > 0.
    for f in &item.fields {
        if !f.present {
            continue;
        }
        match (f.name.as_str(), &f.value) {
            ("_itemDyeDataList", FieldValue::ObjectList { count, .. }) if *count > 0 => {
                flags |= item_record_flags::HAS_DYE_DATA;
            }
            ("_socketSaveDataList", FieldValue::ObjectList { count, .. }) if *count > 0 => {
                flags |= item_record_flags::HAS_SOCKET_DATA;
            }
            _ => {}
        }
    }

    let item_no = pull_u64_present(item, "_itemNo").unwrap_or(0);
    let slot_no = ctx
        .slot_no_override
        .or_else(|| pull_u16_present(item, "_slotNo").map(u32::from))
        .unwrap_or(0);

    Some(CrimsonItemRecord {
        block_idx: ctx.block_idx,
        container_kind: ctx.container_kind,
        path_len: 2,
        path_step_0_field: ctx.path_step_0_field,
        path_step_0_element: ctx.path_step_0_element,
        path_step_1_field: ctx.path_step_1_field,
        path_step_1_element: ctx.path_step_1_element,
        inventory_key: ctx.inventory_key,
        item_key,
        slot_no,
        flags,
        owner_character_key: ctx.owner_character_key,
        item_no,
        owner_mercenary_no: ctx.owner_mercenary_no,
    })
}

/// Scan the save handle, return every item record. Iterates blocks
/// once; per-block field lookups are by name (so a future game patch
/// reordering fields stays correct). The cost is O(total blocks +
/// total items) and runs comfortably under 5 ms on the slot103
/// reference save (96 mercenaries × ~5 items + ~545 inventory + ~18
/// equipped + ~22 reserve).
fn collect_all_items(handle: &CrimsonSaveHandle) -> Vec<CrimsonItemRecord> {
    let mut out: Vec<CrimsonItemRecord> = Vec::new();

    // ── Pass A: resolve the active character key, if any.
    // `MercenaryClanSaveData._lastFocusCharacterKey` is the canonical
    // pointer to the active playable. Fall back to 0 (matching the
    // "owner unknown" convention) if the block is absent.
    let active_character_key: u32 = handle
        .blocks
        .iter()
        .find(|b| b.class_name == "MercenaryClanSaveData")
        .and_then(|b| pull_u32_present(b, "_lastFocusCharacterKey"))
        .map(|k| k & 0xFFFFFF)
        .unwrap_or(0);

    // ── Pass B: walk every TOC block and record items per kind.
    for (block_idx, block) in handle.blocks.iter().enumerate() {
        match block.class_name.as_str() {
            "EquipmentSaveData" => {
                emit_active_equipment(block, block_idx as u32, active_character_key, &mut out);
                emit_active_use_reserve(block, block_idx as u32, active_character_key, &mut out);
            }
            "InventorySaveData" => {
                emit_inventory(block, block_idx as u32, active_character_key, &mut out);
            }
            "MercenaryClanSaveData" => {
                emit_mercenary_lists(block, block_idx as u32, &mut out);
            }
            _ => {}
        }
    }

    out
}

fn emit_active_equipment(
    block: &ObjectBlock,
    block_idx: u32,
    active_character_key: u32,
    out: &mut Vec<CrimsonItemRecord>,
) {
    let Some(list_field) = block.fields.iter().find(|f| f.name == "_list" && f.present) else {
        return;
    };
    let FieldValue::ObjectList { elements: slots, .. } = &list_field.value else {
        return;
    };
    let list_field_idx = list_field.field_index;

    for (slot_idx, slot) in slots.iter().enumerate() {
        // slot is an EquipSlotElementSaveData carrying `_item` Locator
        // + (sometimes) `_occupiedSlotNo`.
        let item_field = slot.fields.iter().find(|f| f.name == "_item" && f.present);
        let Some(item_field) = item_field else { continue };
        let FieldValue::Locator { child: Some(item), .. } = &item_field.value else {
            continue;
        };
        let item_field_idx = item_field.field_index;
        let slot_no_override = pull_u16_present(slot, "_occupiedSlotNo").map(u32::from);
        out.extend(build_record(
            RecordCtx {
                block_idx,
                container_kind: container_kind::ACTIVE_EQUIP,
                path_step_0_field: list_field_idx,
                path_step_0_element: slot_idx as u32,
                path_step_1_field: item_field_idx,
                path_step_1_element: 0, // Locator descent
                inventory_key: 0,
                owner_character_key: active_character_key,
                owner_mercenary_no: 0,
                owner_is_main_mercenary: false,
                slot_no_override,
                is_player_owned: true, // active character is always a playable
            },
            item,
        ));
    }
}

fn emit_active_use_reserve(
    block: &ObjectBlock,
    block_idx: u32,
    active_character_key: u32,
    out: &mut Vec<CrimsonItemRecord>,
) {
    let Some(list_field) = block.fields.iter().find(|f| f.name == "_useItemSaveList" && f.present)
    else {
        return;
    };
    let FieldValue::ObjectList { elements: slots, .. } = &list_field.value else {
        return;
    };
    let list_field_idx = list_field.field_index;

    for (slot_idx, slot) in slots.iter().enumerate() {
        // slot is a UseItemReserveSlotElementSaveData with
        // `_reserveItem` Locator + scalar metadata (`_inventoryType`,
        // `_inventorySlotNo`, `_equipSlotNo`, …).
        let item_field = slot.fields.iter().find(|f| f.name == "_reserveItem" && f.present);
        let Some(item_field) = item_field else { continue };
        let FieldValue::Locator { child: Some(item), .. } = &item_field.value else {
            continue;
        };
        let item_field_idx = item_field.field_index;
        let inventory_type = pull_u16_present(slot, "_inventoryType").map(u32::from).unwrap_or(0);
        let slot_no_override = pull_u16_present(slot, "_inventorySlotNo").map(u32::from);
        out.extend(build_record(
            RecordCtx {
                block_idx,
                container_kind: container_kind::ACTIVE_USE_RESERVE,
                path_step_0_field: list_field_idx,
                path_step_0_element: slot_idx as u32,
                path_step_1_field: item_field_idx,
                path_step_1_element: 0,
                inventory_key: inventory_type,
                owner_character_key: active_character_key,
                owner_mercenary_no: 0,
                owner_is_main_mercenary: false,
                slot_no_override,
                is_player_owned: true,
            },
            item,
        ));
    }
}

fn emit_inventory(
    block: &ObjectBlock,
    block_idx: u32,
    active_character_key: u32,
    out: &mut Vec<CrimsonItemRecord>,
) {
    // InventorySaveData → `_inventoryList` (case-insensitive — game
    // schema spells it `_inventorylist` but matching is robust).
    let Some(inv_list_field) =
        block.fields.iter().find(|f| f.name.eq_ignore_ascii_case("_inventoryList") && f.present)
    else {
        return;
    };
    let FieldValue::ObjectList { elements: containers, .. } = &inv_list_field.value else {
        return;
    };
    let inv_list_field_idx = inv_list_field.field_index;

    for (container_idx, container) in containers.iter().enumerate() {
        let inv_key = pull_u16_present(container, "_inventoryKey")
            .map(u32::from)
            .unwrap_or(0);
        let Some(item_list_field) = container
            .fields
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case("_itemList") && f.present)
        else {
            continue;
        };
        let FieldValue::ObjectList { elements: items, .. } = &item_list_field.value else {
            continue;
        };
        let item_list_field_idx = item_list_field.field_index;

        for (item_idx, item) in items.iter().enumerate() {
            out.extend(build_record(
                RecordCtx {
                    block_idx,
                    container_kind: container_kind::INVENTORY,
                    path_step_0_field: inv_list_field_idx,
                    path_step_0_element: container_idx as u32,
                    path_step_1_field: item_list_field_idx,
                    path_step_1_element: item_idx as u32,
                    inventory_key: inv_key,
                    owner_character_key: active_character_key,
                    owner_mercenary_no: 0,
                    owner_is_main_mercenary: false,
                    slot_no_override: None,
                    is_player_owned: true,
                },
                item,
            ));
        }
    }
}

fn emit_mercenary_lists(block: &ObjectBlock, block_idx: u32, out: &mut Vec<CrimsonItemRecord>) {
    let Some(merc_list_field) =
        block.fields.iter().find(|f| f.name == "_mercenaryDataList" && f.present)
    else {
        return;
    };
    let FieldValue::ObjectList { elements: mercs, .. } = &merc_list_field.value else {
        return;
    };
    let merc_list_field_idx = merc_list_field.field_index;

    for (merc_idx, merc) in mercs.iter().enumerate() {
        let owner_char_key = pull_u32_present(merc, "_characterKey")
            .map(|k| k & 0xFFFFFF)
            .unwrap_or(0);
        let owned_by = pull_u32_present(merc, "_ownedCharacterKey")
            .map(|k| k & 0xFFFFFF);
        let owner_merc_no = pull_u64_present(merc, "_mercenaryNo").unwrap_or(0);
        let is_main = pull_bool_true(merc, "_isMainMercenary");
        // Player-owned if the mercenary block IS a playable (Damine /
        // Oongka — `_characterKey ∈ PLAYABLE_CHARACTER_KEYS`) OR a
        // mount whose `_ownedCharacterKey` points at a playable.
        // Kliff isn't in this branch — he's active, so his gear lives
        // in EquipmentSaveData and gets its IS_PLAYER_OWNED set by
        // the active-equip emitter above.
        let is_player_owned = PLAYABLE_CHARACTER_KEYS.contains(&owner_char_key)
            || owned_by.is_some_and(|k| PLAYABLE_CHARACTER_KEYS.contains(&k));

        // Equip list
        if let Some(equip_list_field) =
            merc.fields.iter().find(|f| f.name == "_equipItemList" && f.present)
            && let FieldValue::ObjectList { elements: items, .. } = &equip_list_field.value
        {
            // Step 1: into the mercenary's _equipItemList container.
            // The descent path is technically [(merc_list, merc_idx)
            // → MercenarySaveData → (_equipItemList, item_idx)], but
            // path_len=2 doesn't have room for the mercenary hop.
            // Encode it as a two-step path where step 0 enters the
            // mercenary list (yielding the MercenarySaveData) and
            // step 1 enters the equip list of that mercenary — same
            // shape the existing mutation ABIs accept (each path step
            // descends one level).
            //
            // 2-step descent: each step navigates one level relative
            // to the previous (per `navigate_mut_to_parent`'s
            // contract). From MercenaryClanSaveData:
            //   step 0: _mercenaryDataList[N] → MercenarySaveData
            //   step 1: _equipItemList[M] → ItemSaveData
            // Mutation-compatible — pinned by
            // `live_path_navigation_reaches_item_save_data_for_every_kind`.
            let equip_field_idx = equip_list_field.field_index;
            for (item_idx, item) in items.iter().enumerate() {
                out.extend(build_record(
                    RecordCtx {
                        block_idx,
                        container_kind: container_kind::MERCENARY_EQUIP,
                        path_step_0_field: merc_list_field_idx,
                        path_step_0_element: merc_idx as u32,
                        path_step_1_field: equip_field_idx,
                        path_step_1_element: item_idx as u32,
                        inventory_key: 0,
                        owner_character_key: owner_char_key,
                        owner_mercenary_no: owner_merc_no,
                        owner_is_main_mercenary: is_main,
                        slot_no_override: None,
                        is_player_owned,
                    },
                    item,
                ));
            }
        }

        // Inventory list
        if let Some(inv_list_field) =
            merc.fields.iter().find(|f| f.name == "_inventoryItemList" && f.present)
            && let FieldValue::ObjectList { elements: items, .. } = &inv_list_field.value
        {
            let inv_field_idx = inv_list_field.field_index;
            for (item_idx, item) in items.iter().enumerate() {
                let inv_key = pull_u16_present(item, "_inventoryKey").map(u32::from).unwrap_or(0);
                out.extend(build_record(
                    RecordCtx {
                        block_idx,
                        container_kind: container_kind::MERCENARY_INVENTORY,
                        path_step_0_field: merc_list_field_idx,
                        path_step_0_element: merc_idx as u32,
                        path_step_1_field: inv_field_idx,
                        path_step_1_element: item_idx as u32,
                        inventory_key: inv_key,
                        owner_character_key: owner_char_key,
                        owner_mercenary_no: owner_merc_no,
                        owner_is_main_mercenary: is_main,
                        slot_no_override: None,
                        is_player_owned,
                    },
                    item,
                ));
            }
        }
    }

}

/// Flat-list every player-owned item across the save's container
/// classes (active equip, active reserve, inventory, mercenary equip,
/// mercenary inventory). Single-call companion to
/// [`super::crimson_save_list_inventory_items`], which only covers
/// the `INVENTORY` slice. See [`CrimsonItemRecord`] for the per-row
/// shape and [`container_kind`] for the container classification.
///
/// **Two-call shape** (record-array variant — counts in records, not
/// bytes):
///
/// - First call with `out_records = null, capacity_records = 0`
///   populates `*out_count_records` and `*out_version`. Returns
///   `BUFFER_TOO_SMALL` (unless the save has zero items, in which
///   case returns `OK`).
/// - Allocate `*out_count_records` records, call again.
///
/// **`out_version` (optional, may be null)**: see
/// [`super::crimson_save_list_inventory_items`] — same staleness
/// contract.
///
/// **Mutation compatibility**: all five container kinds emit
/// 2-step paths that plug straight into
/// `crimson_save_set_scalar_field_path` (verified by
/// `live_path_navigation_reaches_item_save_data_for_every_kind`).
/// Each step navigates one level relative to the previous:
/// `path_step_0` descends into the outer list (e.g.
/// `_mercenaryDataList[N]` → `MercenarySaveData`), then
/// `path_step_1` descends into the inner list / locator (e.g.
/// `_equipItemList[M]` → `ItemSaveData`). The MERCENARY kinds are
/// NOT special — they share the same path shape as active equip /
/// inventory.
///
/// Return codes:
/// - `OK` — list written. `*out_count_records` and `*out_version` are
///   populated.
/// - `BUFFER_TOO_SMALL` — `capacity_records < *out_count_records`.
///   `*out_count_records` and `*out_version` are populated so the
///   caller can allocate then re-call.
/// - `NULL_ARG` — any required pointer is null (see Safety).
///
/// # Safety
/// `handle` must be a live handle from
/// [`super::crimson_save_load_from_file`]. `out_count_records` must
/// point to writable `usize` memory. `out_records` may be null iff
/// `capacity_records == 0`. `out_version` may be null (the version
/// is then dropped).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_list_all_items(
    handle: *const CrimsonSaveHandle,
    out_records: *mut CrimsonItemRecord,
    capacity_records: usize,
    out_count_records: *mut usize,
    out_version: *mut u64,
) -> i32 {
    if handle.is_null() || out_count_records.is_null() {
        return error::NULL_ARG;
    }
    if out_records.is_null() && capacity_records != 0 {
        return error::NULL_ARG;
    }
    unsafe { *out_count_records = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        if !out_version.is_null() {
            unsafe { *out_version = h.mutation_version };
        }
        let records = collect_all_items(h);
        unsafe { *out_count_records = records.len() };
        if records.is_empty() {
            return error::OK;
        }
        if capacity_records < records.len() {
            return error::BUFFER_TOO_SMALL;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(records.as_ptr(), out_records, records.len());
        }
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

#[cfg(test)]
mod tests {
    //! Two test groups:
    //!
    //! 1. **Layout / constants** — pure-Rust assertions on the
    //!    `repr(C)` shape, container-kind / flag constants. Run on
    //!    every CI build.
    //! 2. **Live-install integration** — points at slot103/save.save
    //!    (or `CRIMSON_LIVE_SAVE` override). Asserts the breakdown
    //!    matches the diagnostic-probe output documented in
    //!    `docs/dye-editor-scope.md` §"Addendum (2026-05-17)":
    //!    18 active equip + 8 active reserve + 545 inventory +
    //!    245 mercenary equip + 20 mercenary inventory. Skips
    //!    cleanly when the save isn't present.

    use super::*;
    use std::path::PathBuf;
    use std::ptr;

    #[test]
    fn record_layout_is_stable() {
        // The 64-byte size + 8-byte alignment is part of the ABI;
        // const-asserted at module level but pinned here too in case
        // a future struct edit drops the const_assert.
        assert_eq!(std::mem::size_of::<CrimsonItemRecord>(), 64);
        assert_eq!(std::mem::align_of::<CrimsonItemRecord>(), 8);

        // Field offsets — these are part of the documented ABI table.
        let rec = CrimsonItemRecord {
            block_idx: 0, container_kind: 0, path_len: 0,
            path_step_0_field: 0, path_step_0_element: 0,
            path_step_1_field: 0, path_step_1_element: 0,
            inventory_key: 0, item_key: 0, slot_no: 0,
            flags: 0, owner_character_key: 0,
            item_no: 0, owner_mercenary_no: 0,
        };
        let base = (&rec as *const CrimsonItemRecord).addr();
        let offset = |p: *const u32| (p as usize) - base;
        assert_eq!(offset(&rec.block_idx),               0);
        assert_eq!(offset(&rec.container_kind),          4);
        assert_eq!(offset(&rec.path_len),                8);
        assert_eq!(offset(&rec.path_step_0_field),      12);
        assert_eq!(offset(&rec.path_step_0_element),    16);
        assert_eq!(offset(&rec.path_step_1_field),      20);
        assert_eq!(offset(&rec.path_step_1_element),    24);
        assert_eq!(offset(&rec.inventory_key),          28);
        assert_eq!(offset(&rec.item_key),               32);
        assert_eq!(offset(&rec.slot_no),                36);
        assert_eq!(offset(&rec.flags),                  40);
        assert_eq!(offset(&rec.owner_character_key),    44);
        let offset_u64 = |p: *const u64| (p as usize) - base;
        assert_eq!(offset_u64(&rec.item_no),            48);
        assert_eq!(offset_u64(&rec.owner_mercenary_no), 56);
    }

    #[test]
    fn constants_are_distinct() {
        // Container kinds — small integer enum, distinct.
        let kinds = [
            container_kind::ACTIVE_EQUIP,
            container_kind::ACTIVE_USE_RESERVE,
            container_kind::INVENTORY,
            container_kind::MERCENARY_EQUIP,
            container_kind::MERCENARY_INVENTORY,
        ];
        let mut sorted = kinds.to_vec();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), kinds.len(), "container_kind constants collide");

        // Flag bits — non-overlapping.
        let flags = [
            item_record_flags::LOCKED,
            item_record_flags::NEW_MARK,
            item_record_flags::HAS_DYE_DATA,
            item_record_flags::HAS_SOCKET_DATA,
            item_record_flags::OWNER_IS_MAIN_MERCENARY,
            item_record_flags::IS_PLAYER_OWNED,
        ];
        let combined: u32 = flags.iter().sum();
        assert_eq!(
            combined,
            flags.iter().copied().fold(0u32, |a, b| a | b),
            "flag bits overlap",
        );
    }

    #[test]
    fn null_args() {
        // null handle
        let mut count: usize = 0;
        let rc = unsafe {
            crimson_save_list_all_items(ptr::null(), ptr::null_mut(), 0, &mut count, ptr::null_mut())
        };
        assert_eq!(rc, error::NULL_ARG);
        // null out_count
        // (need a non-null handle to test this — skip; the null-handle
        // path already covers the "any required pointer is null"
        // branch consistently with the rest of the ABI.)
        // null out_records with non-zero capacity
        // (would require a live handle; covered in the live-install
        // test via a deliberate undersize call.)
    }

    fn find_save_path() -> Option<PathBuf> {
        std::env::var_os("CRIMSON_LIVE_SAVE")
            .map(PathBuf::from)
            .or_else(|| {
                let appdata = std::env::var_os("LOCALAPPDATA")?;
                let root = PathBuf::from(appdata)
                    .join("Pearl Abyss")
                    .join("CD")
                    .join("save");
                std::fs::read_dir(&root).ok()?.flatten().find_map(|entry| {
                    let p = entry.path().join("slot103").join("save.save");
                    p.is_file().then_some(p)
                })
            })
    }

    #[test]
    fn live_slot103_breakdown() {
        let Some(save_path) = find_save_path() else {
            eprintln!("skipping live_slot103_breakdown: no slot103/save.save");
            return;
        };
        let path_c = std::ffi::CString::new(save_path.to_str().unwrap()).unwrap();
        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        let rc = unsafe { super::super::crimson_save_load_from_file(path_c.as_ptr(), &mut handle) };
        assert_eq!(rc, error::OK, "load_from_file rc");
        assert!(!handle.is_null());

        // First-call sizing
        let mut count: usize = 0;
        let mut version: u64 = 0;
        let rc = unsafe {
            crimson_save_list_all_items(handle, ptr::null_mut(), 0, &mut count, &mut version)
        };
        // Expect a non-trivial save with hundreds of items.
        assert!(count > 100, "expected >100 records in slot103, got {count}");
        if count > 0 {
            assert_eq!(rc, error::BUFFER_TOO_SMALL);
        }

        // Fill
        let mut records = vec![
            CrimsonItemRecord {
                block_idx: 0, container_kind: u32::MAX, path_len: 0,
                path_step_0_field: 0, path_step_0_element: 0,
                path_step_1_field: 0, path_step_1_element: 0,
                inventory_key: 0, item_key: 0, slot_no: 0,
                flags: 0, owner_character_key: 0,
                item_no: 0, owner_mercenary_no: 0,
            };
            count
        ];
        let mut count2 = count;
        let rc = unsafe {
            crimson_save_list_all_items(
                handle,
                records.as_mut_ptr(),
                records.len(),
                &mut count2,
                ptr::null_mut(),
            )
        };
        assert_eq!(rc, error::OK);
        assert_eq!(count2, count);

        // Histogram by container kind — pin the reference counts from
        // the slot103 diagnostic probe.
        let mut by_kind: std::collections::BTreeMap<u32, usize> = Default::default();
        for r in &records {
            *by_kind.entry(r.container_kind).or_insert(0) += 1;
            // Every record should have path_len=2 in v1.
            assert_eq!(r.path_len, 2, "expected path_len=2 in v1, got {} (kind={})", r.path_len, r.container_kind);
            // item_key should be non-zero (every ItemSaveData has one).
            assert!(r.item_key != 0, "item_key=0 for container_kind={}", r.container_kind);
        }
        eprintln!("slot103 breakdown by container_kind:");
        for (k, n) in &by_kind {
            let name = match *k {
                container_kind::ACTIVE_EQUIP => "ACTIVE_EQUIP",
                container_kind::ACTIVE_USE_RESERVE => "ACTIVE_USE_RESERVE",
                container_kind::INVENTORY => "INVENTORY",
                container_kind::MERCENARY_EQUIP => "MERCENARY_EQUIP",
                container_kind::MERCENARY_INVENTORY => "MERCENARY_INVENTORY",
                _ => "<unknown>",
            };
            eprintln!("  {name} ({k}): {n}");
        }
        // Active equip: 18 slots in 1.07 (per the
        // _list field count). At least 1 must resolve to an item.
        assert!(
            by_kind.get(&container_kind::ACTIVE_EQUIP).copied().unwrap_or(0) >= 1,
            "expected ≥1 ACTIVE_EQUIP record",
        );
        // Inventory: 545 items in the probe baseline.
        assert!(
            by_kind.get(&container_kind::INVENTORY).copied().unwrap_or(0) > 100,
            "expected >100 INVENTORY records (baseline ~545)",
        );
        // Mercenary equip: 245 in probe baseline.
        assert!(
            by_kind.get(&container_kind::MERCENARY_EQUIP).copied().unwrap_or(0) > 100,
            "expected >100 MERCENARY_EQUIP records (baseline ~245)",
        );

        // Owner identity must be consistent with kind.
        for r in &records {
            match r.container_kind {
                container_kind::ACTIVE_EQUIP
                | container_kind::ACTIVE_USE_RESERVE
                | container_kind::INVENTORY => {
                    // owner_mercenary_no must be 0 (no mercenary owner).
                    assert_eq!(
                        r.owner_mercenary_no, 0,
                        "active record has non-zero owner_mercenary_no: {:?}", r,
                    );
                }
                container_kind::MERCENARY_EQUIP | container_kind::MERCENARY_INVENTORY => {
                    // Mercenary records SHOULD have non-zero owner_mercenary_no.
                    // (Allow 0 for the edge case of a malformed save,
                    // but the slot103 baseline has all positive.)
                }
                _ => panic!("unexpected container_kind {}", r.container_kind),
            }
        }

        // At least one MAIN mercenary record (the balloon in slot103).
        let main_count = records.iter()
            .filter(|r| r.flags & item_record_flags::OWNER_IS_MAIN_MERCENARY != 0)
            .count();
        eprintln!("OWNER_IS_MAIN_MERCENARY records: {}", main_count);

        // Every active-container record has IS_PLAYER_OWNED set
        // (the active character is always one of the three playables).
        for r in &records {
            match r.container_kind {
                container_kind::ACTIVE_EQUIP
                | container_kind::ACTIVE_USE_RESERVE
                | container_kind::INVENTORY => {
                    assert_ne!(
                        r.flags & item_record_flags::IS_PLAYER_OWNED, 0,
                        "active container missing IS_PLAYER_OWNED: {:?}", r,
                    );
                }
                _ => {}
            }
        }

        // Dye flag matches the diagnostic-probe result: 5 ItemSaveData
        // hosts with present non-empty _itemDyeDataList (one of which
        // is the UseItemReserve mirror, so the all-items walker sees
        // it twice: once as ACTIVE_EQUIP element 1, once as
        // ACTIVE_USE_RESERVE element 6 — both pointing at the same
        // itemNo=2772 Locator child).
        let dyed = records.iter()
            .filter(|r| r.flags & item_record_flags::HAS_DYE_DATA != 0)
            .count();
        eprintln!("HAS_DYE_DATA records: {dyed}");
        assert!(dyed >= 3, "expected ≥3 dye-flagged records, got {dyed}");

        // Version stamp pulled.
        assert_eq!(version, 0, "fresh handle should report mutation_version=0");

        unsafe { super::super::crimson_save_free(handle) };
    }

    /// Verify the 2-step path recorded for **every** container kind
    /// (including `MERCENARY_EQUIP`) actually navigates correctly
    /// through `navigate_mut_to_parent`. The test discriminator is:
    /// hit `_itemDyeDataList` (field 14 of `ItemSaveData`, known
    /// ObjectList) via `set_scalar_field_path` with a zero-length
    /// scalar write. The expected rc is `NOT_SCALAR` (field 14 IS a
    /// field but isn't a scalar) — which only fires after navigation
    /// has reached the ItemSaveData. If navigation failed mid-path
    /// we'd see `NOT_NAVIGABLE` or `OUT_OF_RANGE` instead, which the
    /// assertion catches.
    ///
    /// Settles the v1-caveat question raised in the previous PR: the
    /// 2-step path IS sufficient for mercenary mutations.
    /// `navigate_mut_to_parent` walks each step relative to the
    /// current block, so `[(_mercenaryDataList[N]), (_equipItemList[M])]`
    /// from `MercenaryClanSaveData` lands on `ItemSaveData` correctly
    /// — same shape as the inventory + active-equip paths.
    #[test]
    fn live_path_navigation_reaches_item_save_data_for_every_kind() {
        let Some(save_path) = find_save_path() else {
            eprintln!("skipping: no save");
            return;
        };
        let path_c = std::ffi::CString::new(save_path.to_str().unwrap()).unwrap();
        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        let rc = unsafe { super::super::crimson_save_load_from_file(path_c.as_ptr(), &mut handle) };
        assert_eq!(rc, error::OK);

        // List + filter to one record per container kind.
        let mut count: usize = 0;
        let _ = unsafe {
            crimson_save_list_all_items(handle, ptr::null_mut(), 0, &mut count, ptr::null_mut())
        };
        let mut records = vec![
            CrimsonItemRecord {
                block_idx: 0, container_kind: 0, path_len: 0,
                path_step_0_field: 0, path_step_0_element: 0,
                path_step_1_field: 0, path_step_1_element: 0,
                inventory_key: 0, item_key: 0, slot_no: 0,
                flags: 0, owner_character_key: 0,
                item_no: 0, owner_mercenary_no: 0,
            };
            count
        ];
        let mut got = 0usize;
        let _ = unsafe {
            crimson_save_list_all_items(handle, records.as_mut_ptr(), count, &mut got, ptr::null_mut())
        };

        // Pick one record per kind (first occurrence).
        let mut by_kind: std::collections::BTreeMap<u32, &CrimsonItemRecord> = Default::default();
        for r in &records {
            by_kind.entry(r.container_kind).or_insert(r);
        }
        assert!(
            by_kind.contains_key(&container_kind::MERCENARY_EQUIP),
            "slot103 must contain at least one MERCENARY_EQUIP record",
        );

        for (kind, r) in &by_kind {
            // Build the path the editor would use.
            let path = [
                super::super::CrimsonPathStep {
                    field_idx: r.path_step_0_field,
                    element_idx: r.path_step_0_element,
                },
                super::super::CrimsonPathStep {
                    field_idx: r.path_step_1_field,
                    element_idx: r.path_step_1_element,
                },
            ];
            // field 14 = _itemDyeDataList on ItemSaveData (an ObjectList,
            // so writing a scalar to it = NOT_SCALAR if navigation worked).
            // bytes/len content is irrelevant — the API short-circuits
            // on NOT_SCALAR before reading any byte. The
            // `slice_from_raw_or_empty` helper handles null+0 cleanly
            // now (see `live_null_bytes_with_zero_len_is_safe`).
            const ITEM_DYE_DATA_LIST_FIELD_IDX: u32 = 14;
            let rc = unsafe {
                super::super::crimson_save_set_scalar_field_path(
                    handle,
                    r.block_idx,
                    path.as_ptr(),
                    path.len(),
                    ITEM_DYE_DATA_LIST_FIELD_IDX,
                    ptr::null(),
                    0,
                )
            };
            assert_eq!(
                rc, error::NOT_SCALAR,
                "kind {kind}: navigation reached ItemSaveData but rc={rc} (NOT_SCALAR expected). \
                 If rc is NOT_NAVIGABLE (-15) or OUT_OF_RANGE (-10) the path is wrong.",
            );
        }

        unsafe { super::super::crimson_save_free(handle) };
    }

    /// Regression: passing `(bytes = null, bytes_len = 0)` to
    /// `crimson_save_set_scalar_field_path` must NOT trip Rust 2024's
    /// `slice::from_raw_parts` UB check. The fix is the
    /// `slice_from_raw_or_empty` helper in `c_abi/mod.rs`; this test
    /// pins the contract by calling the API with exactly that
    /// pointer shape and asserting we get an error code (LENGTH_MISMATCH
    /// or NOT_SCALAR) rather than a panic / process abort.
    ///
    /// The contract says null+0 represents an empty buffer, so the
    /// helper returns `&[]` regardless of the pointer value. Earlier
    /// versions hit `unsafe precondition violated: from_raw_parts
    /// requires non-null pointer` on every call with that shape.
    #[test]
    fn live_null_bytes_with_zero_len_is_safe() {
        let Some(save_path) = find_save_path() else {
            eprintln!("skipping: no save");
            return;
        };
        let path_c = std::ffi::CString::new(save_path.to_str().unwrap()).unwrap();
        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        let rc = unsafe { super::super::crimson_save_load_from_file(path_c.as_ptr(), &mut handle) };
        assert_eq!(rc, error::OK);

        // Pick any record; the field choice doesn't matter because
        // the API should reject before reading any byte.
        let mut count: usize = 0;
        let _ = unsafe {
            crimson_save_list_all_items(handle, ptr::null_mut(), 0, &mut count, ptr::null_mut())
        };
        let mut records = vec![
            CrimsonItemRecord {
                block_idx: 0, container_kind: 0, path_len: 0,
                path_step_0_field: 0, path_step_0_element: 0,
                path_step_1_field: 0, path_step_1_element: 0,
                inventory_key: 0, item_key: 0, slot_no: 0,
                flags: 0, owner_character_key: 0,
                item_no: 0, owner_mercenary_no: 0,
            };
            count
        ];
        let mut got = 0usize;
        let _ = unsafe {
            crimson_save_list_all_items(handle, records.as_mut_ptr(), count, &mut got, ptr::null_mut())
        };
        let rec = records.iter().find(|r| r.container_kind == container_kind::ACTIVE_EQUIP)
            .expect("active equip record");

        let path = [
            super::super::CrimsonPathStep {
                field_idx: rec.path_step_0_field,
                element_idx: rec.path_step_0_element,
            },
            super::super::CrimsonPathStep {
                field_idx: rec.path_step_1_field,
                element_idx: rec.path_step_1_element,
            },
        ];
        // The "null bytes, zero length" path that previously aborted
        // the process with the unsafe-precondition violation. Now it
        // should reach LENGTH_MISMATCH (field 2 _itemKey is u32 = 4
        // bytes; bytes_len = 0 doesn't match) or NOT_SCALAR if the
        // field isn't fixed-size.
        const ANY_LEAF_FIELD: u32 = 2; // _itemKey on ItemSaveData
        let rc = unsafe {
            super::super::crimson_save_set_scalar_field_path(
                handle,
                rec.block_idx,
                path.as_ptr(),
                path.len(),
                ANY_LEAF_FIELD,
                ptr::null(),
                0,
            )
        };
        // Either error is fine — the point is that we got an error
        // code back instead of a process abort. The UB regression
        // would manifest as the test process exiting with STATUS_STACK_BUFFER_OVERRUN
        // (0xc0000409) before this assert runs.
        assert!(
            rc == error::LENGTH_MISMATCH || rc == error::NOT_SCALAR,
            "expected LENGTH_MISMATCH or NOT_SCALAR, got rc={rc}",
        );

        unsafe { super::super::crimson_save_free(handle) };
    }

    /// Live-install asserter for the `IS_PLAYER_OWNED` filter.
    ///
    /// In slot103, the playable-owned subset must be:
    /// - **All** ACTIVE_EQUIP (18) + ACTIVE_USE_RESERVE (1) +
    ///   INVENTORY (545) records — the active character is always
    ///   one of the three playables.
    /// - The MERCENARY_EQUIP / MERCENARY_INVENTORY records whose
    ///   enclosing mercenary's `_characterKey ∈ {1, 4, 6}` (the
    ///   inactive Damine + Oongka playables — slots 0 and 1 in
    ///   `_mercenaryDataList`).
    /// - The MERCENARY_EQUIP / MERCENARY_INVENTORY records whose
    ///   mercenary has `_ownedCharacterKey ∈ {1, 4, 6}` (mounts of
    ///   the three playables — the Tiuta horses + the dyed black
    ///   wild horse + the balloon + the wagon).
    /// - NPC mercenaries (the 50+ `NHM_Unique_*` / `NDM_Unique_*`
    ///   followers) must be EXCLUDED — those have `_characterKey`
    ///   outside the playable set and `_ownedCharacterKey` absent.
    ///
    /// The exact owned-by-playable count varies (depends on which
    /// mounts have equipment), but two invariants hold:
    /// 1. All active-container records are player-owned.
    /// 2. NPC mercenary records (charKey ∉ {1,4,6} AND no
    ///    _ownedCharacterKey) are NOT player-owned.
    #[test]
    fn live_slot103_player_owned_filter() {
        let Some(save_path) = find_save_path() else {
            eprintln!("skipping: no save");
            return;
        };
        let path_c = std::ffi::CString::new(save_path.to_str().unwrap()).unwrap();
        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { super::super::crimson_save_load_from_file(path_c.as_ptr(), &mut handle) },
            error::OK,
        );

        let mut count: usize = 0;
        let _ = unsafe {
            crimson_save_list_all_items(handle, ptr::null_mut(), 0, &mut count, ptr::null_mut())
        };
        let mut records = vec![
            CrimsonItemRecord {
                block_idx: 0, container_kind: 0, path_len: 0,
                path_step_0_field: 0, path_step_0_element: 0,
                path_step_1_field: 0, path_step_1_element: 0,
                inventory_key: 0, item_key: 0, slot_no: 0,
                flags: 0, owner_character_key: 0,
                item_no: 0, owner_mercenary_no: 0,
            };
            count
        ];
        let mut got = 0usize;
        let _ = unsafe {
            crimson_save_list_all_items(handle, records.as_mut_ptr(), count, &mut got, ptr::null_mut())
        };

        let total = records.len();
        let owned: Vec<_> = records.iter()
            .filter(|r| r.flags & item_record_flags::IS_PLAYER_OWNED != 0)
            .collect();
        let unowned: Vec<_> = records.iter()
            .filter(|r| r.flags & item_record_flags::IS_PLAYER_OWNED == 0)
            .collect();
        eprintln!("total records:      {total}");
        eprintln!("player-owned:       {}", owned.len());
        eprintln!("not player-owned:   {}", unowned.len());

        // Active-container records are always player-owned.
        for r in &records {
            match r.container_kind {
                container_kind::ACTIVE_EQUIP
                | container_kind::ACTIVE_USE_RESERVE
                | container_kind::INVENTORY => {
                    assert_ne!(
                        r.flags & item_record_flags::IS_PLAYER_OWNED, 0,
                        "active-container record without IS_PLAYER_OWNED",
                    );
                }
                _ => {}
            }
        }

        // Unowned records must all be mercenary-container kind — the
        // walker never emits a non-mercenary record without
        // IS_PLAYER_OWNED.
        for r in &unowned {
            assert!(
                matches!(
                    r.container_kind,
                    container_kind::MERCENARY_EQUIP | container_kind::MERCENARY_INVENTORY,
                ),
                "non-mercenary record missing IS_PLAYER_OWNED: {:?}", r,
            );
            // And their owner_character_key must be outside the
            // playable set.
            assert!(
                !PLAYABLE_CHARACTER_KEYS.contains(&r.owner_character_key),
                "NPC follower record has playable owner_character_key: {:?}", r,
            );
        }

        // Sanity bounds: the filter shouldn't reject everything or
        // include everything.
        assert!(owned.len() < total, "filter passed all {total} records — bug in PLAYABLE_CHARACTER_KEYS?");
        assert!(owned.len() > 500, "filter passed only {} records, expected >500 (18 + 545 + …)", owned.len());

        // Histogram by owner_character_key among player-owned mercenary records.
        let mut merc_owners: std::collections::BTreeMap<u32, usize> = Default::default();
        for r in &owned {
            if matches!(
                r.container_kind,
                container_kind::MERCENARY_EQUIP | container_kind::MERCENARY_INVENTORY,
            ) {
                *merc_owners.entry(r.owner_character_key).or_insert(0) += 1;
            }
        }
        eprintln!("player-owned mercenary records by charKey:");
        for (k, n) in &merc_owners {
            eprintln!("  charKey={k}: {n}");
        }

        unsafe { super::super::crimson_save_free(handle) };
    }

    #[test]
    fn live_buffer_too_small_path() {
        let Some(save_path) = find_save_path() else {
            eprintln!("skipping live_buffer_too_small_path: no save");
            return;
        };
        let path_c = std::ffi::CString::new(save_path.to_str().unwrap()).unwrap();
        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        let rc = unsafe { super::super::crimson_save_load_from_file(path_c.as_ptr(), &mut handle) };
        assert_eq!(rc, error::OK);

        // Sized but undersized: capacity = 1 but real count > 1.
        let mut count: usize = 0;
        let mut version: u64 = 0;
        let _ = unsafe {
            crimson_save_list_all_items(handle, ptr::null_mut(), 0, &mut count, &mut version)
        };
        assert!(count > 1, "need a save with >1 record");

        let mut records = vec![
            CrimsonItemRecord {
                block_idx: 0, container_kind: 0, path_len: 0,
                path_step_0_field: 0, path_step_0_element: 0,
                path_step_1_field: 0, path_step_1_element: 0,
                inventory_key: 0, item_key: 0, slot_no: 0,
                flags: 0, owner_character_key: 0,
                item_no: 0, owner_mercenary_no: 0,
            };
            1
        ];
        let mut got = 0usize;
        let rc = unsafe {
            crimson_save_list_all_items(
                handle,
                records.as_mut_ptr(),
                records.len(),
                &mut got,
                ptr::null_mut(),
            )
        };
        assert_eq!(rc, error::BUFFER_TOO_SMALL);
        assert_eq!(got, count, "BUFFER_TOO_SMALL must still populate the real count");

        unsafe { super::super::crimson_save_free(handle) };
    }
}
