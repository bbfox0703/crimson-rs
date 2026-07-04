//! `iteminfo.pabgb` bridge — C ABI surface.
//!
//! Parses an in-memory `iteminfo.pabgb` blob (after PAZ extraction
//! from group `0008`, directory `gamedata/binary__/client/bin/`) and
//! exposes a single primitive the downstream editor needs: map an
//! `ItemKey (u32)` to its `string_key (String)`. The string key then
//! feeds into the PALOC catalog from [`super::paloc`] to yield a
//! localized display name.
//!
//! Memory cost: ~6,400 items in 1.06 × ~30 bytes per string key ≈
//! 200 KB resident plus HashMap overhead. The full `ItemInfo` parse
//! runs once at load time, but only the `(key, string_key)` pair is
//! retained — the other 100+ fields are dropped. A future PR can
//! expose richer queries (icon path, item description, …) without
//! reshaping this handle.

use std::collections::HashMap;
use std::io;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::error;
use crate::binary::BinaryRead;
use crate::item_info::ItemInfo;

// ── Static-flag bitmask layout (CrimsonItemInfoFlags) ──────────────────────
//
// `crimson_iteminfo_lookup_flags` (and the `flags` field of
// `CrimsonItemInfoSummary`) returns a u32 bitmask of the static-boolean
// fields the gamedata defines per `ItemInfo` row. Bit indices are stable —
// new flags get appended (next available is bit 28); the lower bits never
// move. Mirror in C# as `[Flags] enum CrimsonItemInfoFlags : uint`.
pub const CRIMSON_ITEMINFO_FLAG_IS_BLOCKED: u32 = 1 << 0;
pub const CRIMSON_ITEMINFO_FLAG_IS_DYEABLE: u32 = 1 << 1;
pub const CRIMSON_ITEMINFO_FLAG_IS_DESTROY_WHEN_BROKEN: u32 = 1 << 2;
pub const CRIMSON_ITEMINFO_FLAG_IS_HOUSING_ONLY: u32 = 1 << 3;
/// Added in Crimson Desert 1.08. Set on every weapon / armor /
/// accessory / tool-slot tool (949 of 6,314 items). See
/// `src/item_info/item.rs` for the value-distribution analysis behind
/// the naming guess.
pub const CRIMSON_ITEMINFO_FLAG_IS_EQUIP_QUICK_SLOT_VISIBLE: u32 = 1 << 4;
pub const CRIMSON_ITEMINFO_FLAG_IS_IMPORTANT_ITEM: u32 = 1 << 5;
pub const CRIMSON_ITEMINFO_FLAG_IS_SHIELD_ITEM: u32 = 1 << 6;
pub const CRIMSON_ITEMINFO_FLAG_IS_TOWER_SHIELD_ITEM: u32 = 1 << 7;
pub const CRIMSON_ITEMINFO_FLAG_IS_WILD: u32 = 1 << 8;
pub const CRIMSON_ITEMINFO_FLAG_HIDE_FROM_INVENTORY_ON_POP_ITEM: u32 = 1 << 9;
pub const CRIMSON_ITEMINFO_FLAG_DISCARDABLE: u32 = 1 << 10;
pub const CRIMSON_ITEMINFO_FLAG_IS_REGISTER_TRADE_MARKET: u32 = 1 << 11;
pub const CRIMSON_ITEMINFO_FLAG_IS_EDITOR_USABLE: u32 = 1 << 12;
pub const CRIMSON_ITEMINFO_FLAG_IS_EDITABLE_GRIME: u32 = 1 << 13;
pub const CRIMSON_ITEMINFO_FLAG_USE_IMMEDIATELY: u32 = 1 << 14;
pub const CRIMSON_ITEMINFO_FLAG_APPLY_MAX_STACK_CAP: u32 = 1 << 15;
pub const CRIMSON_ITEMINFO_FLAG_IS_BLOCKED_STORE_SELL: u32 = 1 << 16;
pub const CRIMSON_ITEMINFO_FLAG_IS_PREORDER_ITEM: u32 = 1 << 17;
pub const CRIMSON_ITEMINFO_FLAG_IS_HAS_ITEM_USE_DATA_INVENTORY_BUFF: u32 = 1 << 18;
pub const CRIMSON_ITEMINFO_FLAG_IS_PRESERVED_ON_EXTRACT: u32 = 1 << 19;
pub const CRIMSON_ITEMINFO_FLAG_ENABLE_ALERT_SYSTEM_TO_UI: u32 = 1 << 20;
pub const CRIMSON_ITEMINFO_FLAG_IS_SAVE_GAME_DATA_AT_USE_ITEM: u32 = 1 << 21;
pub const CRIMSON_ITEMINFO_FLAG_IS_LOGOUT_AT_USE_ITEM: u32 = 1 << 22;
pub const CRIMSON_ITEMINFO_FLAG_ENABLE_EQUIP_IN_CLONE_ACTOR: u32 = 1 << 23;
pub const CRIMSON_ITEMINFO_FLAG_CAN_DISASSEMBLE: u32 = 1 << 24;
pub const CRIMSON_ITEMINFO_FLAG_IS_ALL_GIMMICK_SEALABLE: u32 = 1 << 25;
pub const CRIMSON_ITEMINFO_FLAG_DELETE_BY_GIMMICK_UNLOCK: u32 = 1 << 26;
pub const CRIMSON_ITEMINFO_FLAG_USE_DROP_SET_TARGET: u32 = 1 << 27;
// bits 28..31 reserved for future flags — set to 0 by the encoder.

fn pack_iteminfo_flags(item: &ItemInfo) -> u32 {
    let mut f = 0u32;
    macro_rules! set_if {
        ($field:expr, $mask:expr) => {
            if $field != 0 {
                f |= $mask;
            }
        };
    }
    set_if!(item.is_blocked, CRIMSON_ITEMINFO_FLAG_IS_BLOCKED);
    set_if!(item.is_dyeable, CRIMSON_ITEMINFO_FLAG_IS_DYEABLE);
    set_if!(
        item.is_destroy_when_broken,
        CRIMSON_ITEMINFO_FLAG_IS_DESTROY_WHEN_BROKEN
    );
    set_if!(item.is_housing_only, CRIMSON_ITEMINFO_FLAG_IS_HOUSING_ONLY);
    set_if!(
        item.is_equip_quick_slot_visible,
        CRIMSON_ITEMINFO_FLAG_IS_EQUIP_QUICK_SLOT_VISIBLE
    );
    set_if!(
        item.is_important_item,
        CRIMSON_ITEMINFO_FLAG_IS_IMPORTANT_ITEM
    );
    set_if!(item.is_shield_item, CRIMSON_ITEMINFO_FLAG_IS_SHIELD_ITEM);
    set_if!(
        item.is_tower_shield_item,
        CRIMSON_ITEMINFO_FLAG_IS_TOWER_SHIELD_ITEM
    );
    set_if!(item.is_wild, CRIMSON_ITEMINFO_FLAG_IS_WILD);
    set_if!(
        item.hide_from_inventory_on_pop_item,
        CRIMSON_ITEMINFO_FLAG_HIDE_FROM_INVENTORY_ON_POP_ITEM
    );
    set_if!(item.discardable, CRIMSON_ITEMINFO_FLAG_DISCARDABLE);
    set_if!(
        item.is_register_trade_market,
        CRIMSON_ITEMINFO_FLAG_IS_REGISTER_TRADE_MARKET
    );
    set_if!(
        item.is_editor_usable,
        CRIMSON_ITEMINFO_FLAG_IS_EDITOR_USABLE
    );
    set_if!(
        item.is_editable_grime,
        CRIMSON_ITEMINFO_FLAG_IS_EDITABLE_GRIME
    );
    set_if!(item.use_immediately, CRIMSON_ITEMINFO_FLAG_USE_IMMEDIATELY);
    set_if!(
        item.apply_max_stack_cap,
        CRIMSON_ITEMINFO_FLAG_APPLY_MAX_STACK_CAP
    );
    set_if!(
        item.is_blocked_store_sell,
        CRIMSON_ITEMINFO_FLAG_IS_BLOCKED_STORE_SELL
    );
    set_if!(
        item.is_preorder_item,
        CRIMSON_ITEMINFO_FLAG_IS_PREORDER_ITEM
    );
    set_if!(
        item.is_has_item_use_data_inventory_buff,
        CRIMSON_ITEMINFO_FLAG_IS_HAS_ITEM_USE_DATA_INVENTORY_BUFF
    );
    set_if!(
        item.is_preserved_on_extract,
        CRIMSON_ITEMINFO_FLAG_IS_PRESERVED_ON_EXTRACT
    );
    set_if!(
        item.enable_alert_system_to_ui,
        CRIMSON_ITEMINFO_FLAG_ENABLE_ALERT_SYSTEM_TO_UI
    );
    set_if!(
        item.is_save_game_data_at_use_item,
        CRIMSON_ITEMINFO_FLAG_IS_SAVE_GAME_DATA_AT_USE_ITEM
    );
    set_if!(
        item.is_logout_at_use_item,
        CRIMSON_ITEMINFO_FLAG_IS_LOGOUT_AT_USE_ITEM
    );
    set_if!(
        item.enable_equip_in_clone_actor,
        CRIMSON_ITEMINFO_FLAG_ENABLE_EQUIP_IN_CLONE_ACTOR
    );
    set_if!(item.can_disassemble, CRIMSON_ITEMINFO_FLAG_CAN_DISASSEMBLE);
    set_if!(
        item.is_all_gimmick_sealable,
        CRIMSON_ITEMINFO_FLAG_IS_ALL_GIMMICK_SEALABLE
    );
    set_if!(
        item.delete_by_gimmick_unlock,
        CRIMSON_ITEMINFO_FLAG_DELETE_BY_GIMMICK_UNLOCK
    );
    set_if!(
        item.use_drop_set_target,
        CRIMSON_ITEMINFO_FLAG_USE_DROP_SET_TARGET
    );
    f
}

// ── CrimsonItemInfoSummary (one-shot static metadata struct) ───────────────
//
// `crimson_iteminfo_lookup_summary` populates this struct from the cached
// per-key metadata. Field ordering picks the largest scalars first so the
// repr(C) layout has no implicit padding mid-struct; the trailing
// `_reserved: [u8; 5]` brings the size up to a clean 72 bytes
// (`size_of::<CrimsonItemInfoSummary>()` is asserted in the unit tests).
//
// The summary intentionally omits variable-length data (string_key,
// icon_path full string, the various Lists). Callers that need those
// take the per-key dedicated lookups (`lookup_string_key`,
// `lookup_socket_caps`, …).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CrimsonItemInfoSummary {
    pub max_stack_count: u64,
    pub cooltime: i64,
    pub respawn_time_seconds: i64,
    pub key: u32,
    pub flags: u32,
    pub icon_path_hash: u32,
    pub equip_type_info: u32,
    pub equipable_hash: u32,
    pub equipable_level: u32,
    pub knowledge_info: u32,
    pub material_key: u32,
    pub gimmick_info: u32,
    pub category_info: u16,
    pub inventory_info: u16,
    pub minimum_extract_enchant_level: u16,
    pub max_endurance: u16,
    pub item_type: u8,
    pub item_tier: u8,
    pub quick_slot_index: u8,
    pub item_charge_type: u8,
    pub usable_alert_type: u8,
    pub knowledge_obtain_type: u8,
    pub apply_drop_stat_type: u8,
    pub _reserved: u8,
}

const _: () = {
    // Catch unexpected padding at build time — the struct is part of the
    // C ABI surface and downstream C# matches its size with
    // [StructLayout(LayoutKind.Sequential)] + Marshal.SizeOf.
    //
    // Layout under repr(C):
    //   3×u64 + 9×u32 + 4×u16 + 8×u8 = 24+36+8+8 = 76 bytes raw,
    //   rounded up to 80 by the u64 alignment requirement (the trailing
    //   `_reserved: u8` sits at offset 71; bytes 72..79 are pad bytes
    //   the compiler adds to make the struct's total size a multiple
    //   of its 8-byte alignment).
    assert!(std::mem::size_of::<CrimsonItemInfoSummary>() == 80);
    assert!(std::mem::align_of::<CrimsonItemInfoSummary>() == 8);
};

/// Opaque handle exposing lean per-item lookups against the loaded
/// iteminfo: the `string_key` (internal id) and the `max_stack_count`
/// (stack cap). The full ItemInfo parse runs once; only the bits the
/// downstream editor needs are retained.
#[repr(C)]
pub struct CrimsonItemInfoHandle {
    by_key: HashMap<u32, String>,
    max_stack_by_key: HashMap<u32, u64>,
    /// First entry of `item_icon_list[0].icon_path` (a `StringInfoKey`
    /// hash) per item key. Items with no icon list aren't inserted —
    /// the lookup returns `NOT_FOUND` instead of writing 0.
    icon_path_by_key: HashMap<u32, u32>,
    /// Per-item `look_detail_mission_info` (a `MissionKey` u32). Only
    /// items with a non-zero value are inserted — that maps cleanly to
    /// the editor's "does this artifact item belong to a catalog
    /// challenge?" predicate. Quest-reward items (most notably the
    /// Sealed Abyss Artifact series) point at the catalog mission key
    /// of the challenge that rewards them; regular items leave this
    /// field at 0 and don't show up in the map.
    ///
    /// Verified across the live 1.07 install: 141 items carry a
    /// non-zero value, all 141 missions they point at have internal
    /// names matching `Challenge_SealedArtifact_*`. The mapping is
    /// **1:1** (no mission is the target of more than one artifact),
    /// so the reverse map below is a clean inverse.
    look_detail_mission_by_key: HashMap<u32, u32>,
    /// Inverse of `look_detail_mission_by_key`: mission_key → item_key.
    /// Lets the editor answer "which artifact starts THIS challenge?"
    /// in O(1). For missions absent from the map, no artifact
    /// triggers them — they're started by other means (e.g. dialogue,
    /// kill counters, geographic discovery).
    ///
    /// **Invariant**: the iteminfo's forward map is verified 1:1 in
    /// 1.07, so this inverse is unambiguous.
    artifact_by_mission: HashMap<u32, u32>,
    /// Per-item gamedata socket caps:
    /// `(use_socket, socket_valid_count)`.
    /// - `use_socket = 0` → item is not socket-capable in vanilla.
    /// - `use_socket != 0` → socket-capable; `socket_valid_count` is
    ///   the in-game max number of sockets the item supports.
    ///
    /// Every item key is inserted (even non-socket items) so callers
    /// can distinguish "item exists but not socket-capable" from
    /// "item key not in iteminfo at all".
    ///
    /// Source: [`crate::item_info::structs::DropDefaultData`] (parsed
    /// from `iteminfo.pabgb`, field `socket_valid_count` and
    /// `use_socket`).
    socket_caps_by_key: HashMap<u32, (u8, u8)>,
    /// Per-item allowed-gem itemkey list (from
    /// `DropDefaultData::socket_item_list`). Only items with a
    /// non-empty list are inserted — non-socket items have empty
    /// lists and don't show up here, so the editor surfaces "no
    /// allowed gems" as `count = 0`. The list preserves on-disk
    /// order so enumerators are deterministic.
    socket_allowed_gems_by_key: HashMap<u32, Vec<u32>>,
    /// The "canonical gem set" — sorted-ascending list of every
    /// itemkey whose iteminfo row has `item_type == 74` AND
    /// `category_info == CategoryKey(2501)`. This is the gamedata's
    /// own definition of a gem (verified against the user's slot104
    /// reference set — all 43 observed gems match this rule; the
    /// two host items 1000316/1002285 do not).
    ///
    /// **Why not "union of all socket_item_list"?** Because
    /// `DropDefaultData::socket_item_list` is the per-weapon
    /// VENDOR / CRAFTING display list — it's a narrow subset of the
    /// gems the engine will actually accept in that weapon's
    /// socket. The classification `(item_type=74, category=2501)` is
    /// the engine's gem-row marker and captures the full gem
    /// catalog.
    ///
    /// Computed once at handle-load time. The editor is free to
    /// ignore this list and let the user pick any u32 itemkey in
    /// CE-style freeform mode — see
    /// [`crimson_iteminfo_socket_allows_gem`] for the per-weapon
    /// advisory.
    canonical_gem_list: Vec<u32>,
    /// `(key, string_key)` in file order so the caller can enumerate
    /// via [`crimson_iteminfo_get_entry`].
    entries: Vec<(u32, String)>,
    /// Pre-built one-shot static-metadata snapshot per item, populated
    /// once at load time so [`crimson_iteminfo_lookup_summary`] and the
    /// individual scalar / flag getters all read from a hash lookup
    /// instead of re-parsing the underlying bytes. The full ItemInfo
    /// struct is dropped after extracting these — only the snapshot is
    /// retained alongside the other lean per-field caches above. See
    /// [`CrimsonItemInfoSummary`] for the field layout.
    summary_by_key: HashMap<u32, CrimsonItemInfoSummary>,
}

impl CrimsonItemInfoHandle {
    fn from_bytes(data: &[u8]) -> io::Result<Self> {
        // Walk the input the same way `test_full_roundtrip` in lib.rs
        // does. Each `ItemInfo::read_from` advances `offset`; we keep
        // going until the buffer is consumed.
        let mut offset = 0usize;
        let mut entries: Vec<(u32, String)> = Vec::new();
        let mut max_stack_by_key: HashMap<u32, u64> = HashMap::new();
        let mut icon_path_by_key: HashMap<u32, u32> = HashMap::new();
        let mut look_detail_mission_by_key: HashMap<u32, u32> = HashMap::new();
        let mut socket_caps_by_key: HashMap<u32, (u8, u8)> = HashMap::new();
        let mut socket_allowed_gems_by_key: HashMap<u32, Vec<u32>> = HashMap::new();
        // Accumulate gem itemkeys: any row with item_type=74 +
        // category_info=2501 is a gem in the gamedata's own sense.
        let mut canonical_gems_set: std::collections::BTreeSet<u32> =
            std::collections::BTreeSet::new();
        let mut summary_by_key: HashMap<u32, CrimsonItemInfoSummary> = HashMap::new();
        while offset < data.len() {
            let item = ItemInfo::read_from(data, &mut offset)?;
            entries.push((item.key.0, item.string_key.data.to_owned()));
            max_stack_by_key.insert(item.key.0, item.max_stack_count);
            // Build the one-shot static-metadata snapshot before we
            // start dropping field references — this is the source the
            // `lookup_summary` and `lookup_flags` / `lookup_item_tier`
            // / etc. getters all read from.
            summary_by_key.insert(
                item.key.0,
                CrimsonItemInfoSummary {
                    max_stack_count: item.max_stack_count,
                    cooltime: item.cooltime,
                    respawn_time_seconds: item.respawn_time_seconds,
                    key: item.key.0,
                    flags: pack_iteminfo_flags(&item),
                    icon_path_hash: item
                        .item_icon_list
                        .items
                        .first()
                        .map(|ic| ic.icon_path.0)
                        .unwrap_or(0),
                    equip_type_info: item.equip_type_info.0,
                    equipable_hash: item.equipable_hash,
                    equipable_level: item.equipable_level,
                    knowledge_info: item.knowledge_info.0,
                    material_key: item.material_key,
                    gimmick_info: item.gimmick_info.0,
                    category_info: item.category_info.0,
                    inventory_info: item.inventory_info.0,
                    minimum_extract_enchant_level: item.minimum_extract_enchant_level,
                    max_endurance: item.max_endurance,
                    item_type: item.item_type,
                    item_tier: item.item_tier,
                    quick_slot_index: item.quick_slot_index,
                    item_charge_type: item.item_charge_type,
                    usable_alert_type: item.usable_alert_type,
                    knowledge_obtain_type: item.knowledge_obtain_type,
                    apply_drop_stat_type: item.apply_drop_stat_type,
                    _reserved: 0,
                },
            );
            // Only capture the first per-item icon. Items without an
            // `item_icon_list` entry are intentionally skipped — the
            // lookup surface reports those as NOT_FOUND, mirroring the
            // string-key lookup's contract for dev-only items.
            if let Some(first) = item.item_icon_list.items.first() {
                let hash = first.icon_path.0;
                if hash != 0 {
                    icon_path_by_key.insert(item.key.0, hash);
                }
            }
            // 0 means "no associated mission" and is the overwhelming
            // majority case (vanilla items / weapons / consumables).
            // Skip those so the lookup's NOT_FOUND signal is meaningful.
            if item.look_detail_mission_info.0 != 0 {
                look_detail_mission_by_key
                    .insert(item.key.0, item.look_detail_mission_info.0);
            }
            // Socket caps — insert for every item so non-socket items
            // round-trip through the lookup with `use_socket=0` rather
            // than `NOT_FOUND`. The editor needs to distinguish
            // "item missing from iteminfo" (NOT_FOUND) from "item
            // exists but is not socket-capable" (`use_socket=0`).
            let dd = &item.drop_default_data;
            socket_caps_by_key.insert(item.key.0, (dd.use_socket, dd.socket_valid_count));
            // Allowed-gem list = union of `socket_item_list` (explicit
            // allow list, often used for vendor/crafting display) and
            // `add_socket_material_item_list` (the broader set of
            // gem-shaped materials the item's socket slot will accept
            // at insertion time — captures gems missed by the
            // explicit list, including 1002979 "爆走的力量審判").
            // Insertion-order is preserved via Vec; duplicates are
            // de-duplicated with a tracking set so the per-weapon
            // enumeration stays clean.
            let mut union: Vec<u32> = Vec::new();
            let mut seen: std::collections::HashSet<u32> = Default::default();
            for k in &dd.socket_item_list.items {
                if seen.insert(k.0) {
                    union.push(k.0);
                }
            }
            for m in &dd.add_socket_material_item_list.items {
                if seen.insert(m.item.0) {
                    union.push(m.item.0);
                }
            }
            if !union.is_empty() {
                socket_allowed_gems_by_key.insert(item.key.0, union);
            }
            // Gem classifier: verified across the user's 43-gem
            // slot104 reference set. Both fields must match.
            if item.item_type == 74 && item.category_info.0 == 2501 {
                canonical_gems_set.insert(item.key.0);
            }
        }
        if offset != data.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "iteminfo parse ended at 0x{:X} but file is 0x{:X} bytes",
                    offset,
                    data.len()
                ),
            ));
        }
        let by_key = entries.iter().cloned().collect();
        // Canonical gem set is the (item_type=74, category=2501) bucket
        // — collected during the parse loop above.
        let canonical_gem_list: Vec<u32> = canonical_gems_set.into_iter().collect();
        // Inverse of look_detail_mission_by_key. The forward map is
        // verified 1:1 against the live 1.07 install, so this inverse
        // is unambiguous; if a future patch broke the 1:1 invariant
        // we'd silently overwrite earlier entries — surface a warning
        // in that case so we know to revisit the schema.
        let mut artifact_by_mission: HashMap<u32, u32> =
            HashMap::with_capacity(look_detail_mission_by_key.len());
        for (&item_key, &mission_key) in &look_detail_mission_by_key {
            if artifact_by_mission.insert(mission_key, item_key).is_some() {
                eprintln!(
                    "warn: look_detail_mission_info 1:1 invariant broken — \
                     mission {mission_key} pointed at by multiple artifacts; \
                     keeping last-write-wins"
                );
            }
        }
        Ok(CrimsonItemInfoHandle {
            by_key,
            max_stack_by_key,
            icon_path_by_key,
            look_detail_mission_by_key,
            artifact_by_mission,
            socket_caps_by_key,
            socket_allowed_gems_by_key,
            canonical_gem_list,
            entries,
            summary_by_key,
        })
    }
}

// ── Load / free ────────────────────────────────────────────────────────────

/// Parse an `iteminfo.pabgb` blob from disk.
///
/// The file must be **already-decrypted, raw `.pabgb` bytes** — the
/// wrapped copy under `0008/0.paz` needs to come through PAZ
/// extraction first (see [`super::paz::crimson_paz_extract_file`]).
///
/// On success `*out_handle` receives an owned
/// [`CrimsonItemInfoHandle`] that the caller must release via
/// [`crimson_iteminfo_free`].
///
/// # Safety
/// `path` must be a NUL-terminated UTF-8 string and `out_handle` must
/// point at writable memory for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_iteminfo_load_from_file(
    path: *const c_char,
    out_handle: *mut *mut CrimsonItemInfoHandle,
) -> i32 {
    if path.is_null() || out_handle.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_handle = std::ptr::null_mut() };
    catch_unwind(AssertUnwindSafe(|| {
        let path_str = match unsafe { std::ffi::CStr::from_ptr(path) }.to_str() {
            Ok(s) => s,
            Err(_) => return error::INVALID_PATH,
        };
        let bytes = match std::fs::read(path_str) {
            Ok(b) => b,
            Err(_) => return error::IO,
        };
        let handle = match CrimsonItemInfoHandle::from_bytes(&bytes) {
            Ok(h) => h,
            Err(_) => return error::BODY_PARSE,
        };
        unsafe { *out_handle = Box::into_raw(Box::new(handle)) };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Parse iteminfo bytes already in memory (preferred — the editor pulls
/// them through PAZ extraction first).
///
/// # Safety
/// `data` must point to `data_len` readable bytes; `out_handle` must
/// be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_iteminfo_load_from_bytes(
    data: *const u8,
    data_len: usize,
    out_handle: *mut *mut CrimsonItemInfoHandle,
) -> i32 {
    if out_handle.is_null() {
        return error::NULL_ARG;
    }
    if data.is_null() && data_len != 0 {
        return error::NULL_ARG;
    }
    unsafe { *out_handle = std::ptr::null_mut() };
    catch_unwind(AssertUnwindSafe(|| {
        let slice = if data_len == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(data, data_len) }
        };
        let handle = match CrimsonItemInfoHandle::from_bytes(slice) {
            Ok(h) => h,
            Err(_) => return error::BODY_PARSE,
        };
        unsafe { *out_handle = Box::into_raw(Box::new(handle)) };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Free a handle returned by either loader.
///
/// # Safety
/// `handle` must be null or a pointer previously returned by one of
/// the loaders and not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_iteminfo_free(handle: *mut CrimsonItemInfoHandle) {
    if handle.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(handle);
    }
}

// ── Scalar getters ─────────────────────────────────────────────────────────

/// Total number of items in the loaded `iteminfo.pabgb`.
///
/// # Safety
/// `handle` must be live; `out_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_iteminfo_entry_count(
    handle: *const CrimsonItemInfoHandle,
    out_count: *mut u32,
) -> i32 {
    if handle.is_null() || out_count.is_null() {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        unsafe { *out_count = h.entries.len() as u32 };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

// ── Lookup ─────────────────────────────────────────────────────────────────

/// Look up the `string_key` for a given `ItemKey (u32)` and write it
/// into `buf` (NUL-terminated UTF-8). Two-call pattern, identical
/// shape to the PALOC catalog:
///
/// - First call with `buf = null, buf_len = 0` returns
///   `BUFFER_TOO_SMALL` and sets `*required` (includes trailing NUL).
/// - Allocate, call again to receive the bytes and `OK`.
///
/// Returns `NOT_FOUND` when `item_key` doesn't match any item in the
/// loaded table.
///
/// # Safety
/// `handle` and `required` must be non-null; `buf` may be null iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_iteminfo_lookup_string_key(
    handle: *const CrimsonItemInfoHandle,
    item_key: u32,
    buf: *mut u8,
    buf_len: usize,
    required: *mut usize,
) -> i32 {
    if handle.is_null() || required.is_null() {
        return error::NULL_ARG;
    }
    if buf.is_null() && buf_len != 0 {
        return error::NULL_ARG;
    }
    unsafe { *required = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some(name) = h.by_key.get(&item_key) else {
            return error::NOT_FOUND;
        };
        let needed = name.len() + 1;
        unsafe { *required = needed };
        if buf_len < needed {
            return error::BUFFER_TOO_SMALL;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(name.as_ptr(), buf, name.len());
            *buf.add(name.len()) = 0;
        }
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Look up the first `item_icon_list[0].icon_path` for a given `ItemKey
/// (u32)` and write the resulting `StringInfoKey` (u32 hash) into
/// `*out_hash`. The downstream icon-extraction pipeline pipes the hash
/// through the stringinfo bridge to obtain a texture name like
/// `ItemIcon_Prefab_cd_phm_04_arw_0020`, lowercases it, appends
/// `.dds`, and PAZ-extracts the texture from group `0012`'s
/// `ui/texture/icon/` directory.
///
/// Returns `NOT_FOUND` when the key isn't in the loaded table OR the
/// item ships without an `item_icon_list` entry OR the first entry's
/// `icon_path` is 0 (interpreted as "no icon"). On `NOT_FOUND`,
/// `*out_hash` is set to 0.
///
/// # Safety
/// `handle` and `out_hash` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_iteminfo_lookup_icon_path_hash(
    handle: *const CrimsonItemInfoHandle,
    item_key: u32,
    out_hash: *mut u32,
) -> i32 {
    if handle.is_null() || out_hash.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_hash = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some(hash) = h.icon_path_by_key.get(&item_key) else {
            return error::NOT_FOUND;
        };
        unsafe { *out_hash = *hash };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Look up the `max_stack_count` for a given `ItemKey (u32)` and write
/// it into `*out_max_stack`. Returns `NOT_FOUND` when the key isn't
/// in the loaded table, `OK` otherwise. The downstream editor uses
/// this to drive a "Set to max stack" action that fills a save's
/// item-count field with the game's own per-item cap (so the user
/// gets a maxed stack without exceeding what the game considers
/// valid).
///
/// # Safety
/// `handle` and `out_max_stack` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_iteminfo_lookup_max_stack(
    handle: *const CrimsonItemInfoHandle,
    item_key: u32,
    out_max_stack: *mut u64,
) -> i32 {
    if handle.is_null() || out_max_stack.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_max_stack = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some(max) = h.max_stack_by_key.get(&item_key) else {
            return error::NOT_FOUND;
        };
        unsafe { *out_max_stack = *max };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Look up the `look_detail_mission_info` (a `MissionKey` u32) for a
/// given `ItemKey (u32)` and write it into `*out_mission_key`. Returns
/// `NOT_FOUND` when the key isn't in the loaded table OR the item ships
/// without a mission link (the field is 0). On `NOT_FOUND`,
/// `*out_mission_key` is set to 0.
///
/// Use case: the editor's "Mark Challenge Complete" UI only enables when
/// the in-focus catalog `MissionStateData` challenge has a corresponding
/// quest-reward item (e.g. a Sealed Abyss Artifact) currently in the
/// player's inventory. Walking inventory + matching this lookup is the
/// fast path for that gate.
///
/// # Safety
/// `handle` and `out_mission_key` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_iteminfo_lookup_look_detail_mission_info(
    handle: *const CrimsonItemInfoHandle,
    item_key: u32,
    out_mission_key: *mut u32,
) -> i32 {
    if handle.is_null() || out_mission_key.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_mission_key = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some(mk) = h.look_detail_mission_by_key.get(&item_key) else {
            return error::NOT_FOUND;
        };
        unsafe { *out_mission_key = *mk };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Reverse of [`crimson_iteminfo_lookup_look_detail_mission_info`]:
/// given a `MissionKey (u32)`, return the `ItemKey (u32)` of the
/// artifact whose pickup triggers that challenge.
///
/// Use case: the editor's catalog UI starts from a focused mission
/// (challenge); to highlight "you need item X to start this", it
/// reads the artifact key here. Combined with the inventory walker
/// and PALOC, the editor can also tell the user "you already own /
/// have used the artifact for this challenge".
///
/// **Coverage**: 141 missions in 1.07 — all named
/// `Challenge_SealedArtifact_*`. Returns `NOT_FOUND` for the
/// remaining ~3,900 missions; those are challenges that start by
/// other triggers (dialogue, kills, exploration) and need no
/// artifact. The editor uses this to gate the "Sealed Artifact
/// required" badge.
///
/// On `NOT_FOUND`, `*out_item_key` is set to 0.
///
/// # Safety
/// `handle` and `out_item_key` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_iteminfo_lookup_artifact_for_mission(
    handle: *const CrimsonItemInfoHandle,
    mission_key: u32,
    out_item_key: *mut u32,
) -> i32 {
    if handle.is_null() || out_item_key.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_item_key = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some(ik) = h.artifact_by_mission.get(&mission_key) else {
            return error::NOT_FOUND;
        };
        unsafe { *out_item_key = *ik };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

// ── Socket caps + gem-allow cross-check (advisory) ─────────────────────────
//
// These four entry points expose gamedata facts the save's
// `_socketSaveDataList` cannot tell on its own. They are PURE QUERIES
// — the editor decides how to surface violations (warn / red flag /
// log) and is **always free to write any mutation regardless**.
// CE-modified saves with `_validSocketCount > socket_valid_count`,
// or gems outside the allowed list, load cleanly in the game (just
// with some NPC-UI quirks) — the ABI mirrors that permissiveness.

/// Look up the gamedata socket caps for `item_key`. Writes
/// `*out_use_socket` (0 = non-socket item, non-zero = socket-capable)
/// and `*out_valid_count` (the in-game max number of sockets the
/// item supports — only meaningful when `use_socket != 0`).
///
/// Returns `OK` if the item exists in iteminfo (regardless of
/// socket-capability), `NOT_FOUND` if `item_key` is not in the
/// loaded table. The editor compares the returned `out_valid_count`
/// against the save's `_validSocketCount` to detect CE-bumped
/// overflows (`save_valid > gamedata_valid`).
///
/// # Safety
/// `handle`, `out_use_socket`, and `out_valid_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_iteminfo_lookup_socket_caps(
    handle: *const CrimsonItemInfoHandle,
    item_key: u32,
    out_use_socket: *mut u8,
    out_valid_count: *mut u8,
) -> i32 {
    if handle.is_null() || out_use_socket.is_null() || out_valid_count.is_null() {
        return error::NULL_ARG;
    }
    unsafe {
        *out_use_socket = 0;
        *out_valid_count = 0;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some((use_flag, valid)) = h.socket_caps_by_key.get(&item_key) else {
            return error::NOT_FOUND;
        };
        unsafe {
            *out_use_socket = *use_flag;
            *out_valid_count = *valid;
        }
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Check whether `gem_key` is in `item_key`'s gamedata-defined
/// allowed-gem list (`DropDefaultData::socket_item_list`). Writes
/// `*out_allowed` as 1 if allowed, 0 if not.
///
/// **Advisory only**: returns `OK` in both the allowed and
/// not-allowed cases — the editor decides whether to warn the user.
/// CE-bypassed gem placements (gem not in the list) load cleanly in
/// the game; some NPC interfaces may not display them correctly.
///
/// Returns `NOT_FOUND` only when `item_key` itself is missing from
/// iteminfo. An `item_key` that exists but is non-socket-capable
/// (or has an empty allowed-gem list) returns `OK` with
/// `*out_allowed = 0` — the caller should pair this with
/// [`crimson_iteminfo_lookup_socket_caps`] to distinguish "no sockets
/// at all" from "wrong gem for an otherwise socket-capable item".
///
/// # Safety
/// `handle` and `out_allowed` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_iteminfo_socket_allows_gem(
    handle: *const CrimsonItemInfoHandle,
    item_key: u32,
    gem_key: u32,
    out_allowed: *mut u8,
) -> i32 {
    if handle.is_null() || out_allowed.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_allowed = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        // Validate that the item exists at all so the caller can tell
        // "no iteminfo row" from "iteminfo says no allowed gems".
        if !h.by_key.contains_key(&item_key) {
            return error::NOT_FOUND;
        }
        let allowed = h
            .socket_allowed_gems_by_key
            .get(&item_key)
            .is_some_and(|list| list.contains(&gem_key));
        unsafe { *out_allowed = u8::from(allowed) };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Number of entries in `item_key`'s gamedata allowed-gem list.
/// Items that aren't socket-capable (or are missing from the
/// allowed-gem index entirely) return `OK` with `*out_count = 0`.
///
/// Returns `NOT_FOUND` only when `item_key` is missing from iteminfo.
///
/// # Safety
/// `handle` and `out_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_iteminfo_socket_allowed_gem_count(
    handle: *const CrimsonItemInfoHandle,
    item_key: u32,
    out_count: *mut u32,
) -> i32 {
    if handle.is_null() || out_count.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_count = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        if !h.by_key.contains_key(&item_key) {
            return error::NOT_FOUND;
        }
        let c = h
            .socket_allowed_gems_by_key
            .get(&item_key)
            .map_or(0, |v| v.len() as u32);
        unsafe { *out_count = c };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Read the allowed-gem itemkey at insertion index `idx` for
/// `item_key`. Order matches `DropDefaultData::socket_item_list` on
/// disk so enumerations are deterministic.
///
/// Returns `NOT_FOUND` when `item_key` is missing from iteminfo,
/// `OUT_OF_RANGE` when `idx >= allowed_gem_count(item_key)`,
/// `OK` otherwise.
///
/// # Safety
/// `handle` and `out_gem_key` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_iteminfo_socket_allowed_gem_at(
    handle: *const CrimsonItemInfoHandle,
    item_key: u32,
    idx: u32,
    out_gem_key: *mut u32,
) -> i32 {
    if handle.is_null() || out_gem_key.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_gem_key = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        if !h.by_key.contains_key(&item_key) {
            return error::NOT_FOUND;
        }
        let Some(list) = h.socket_allowed_gems_by_key.get(&item_key) else {
            return error::OUT_OF_RANGE;
        };
        let Some(g) = list.get(idx as usize) else {
            return error::OUT_OF_RANGE;
        };
        unsafe { *out_gem_key = *g };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

// ── Canonical gem set (the gem-picker dropdown source) ─────────────────────
//
// The "canonical gem set" = sorted-ascending union of every item's
// `socket_item_list`. Every itemkey that ANY weapon accepts as a gem
// is a gem by this definition. Computed once at handle-load time,
// available as a flat enumeration so the C# editor can drive a
// gem-picker dropdown without reinventing the gem catalog itself.
//
// The editor is free to:
// - Show this list as the default gem picker (sensible vanilla UX),
// - Filter it further (e.g. only gems the focused weapon accepts via
//   the per-weapon `socket_allowed_gem_count/_at` advisory),
// - Or skip it entirely and let the user enter any u32 itemkey for
//   CE-style freeform mode (the save format and runtime accept it).

/// Number of itemkeys in the canonical gem set (sorted-ascending
/// union of every item's `socket_item_list`).
///
/// Returns `OK` (always — empty iteminfo just gives 0).
///
/// # Safety
/// `handle` and `out_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_iteminfo_canonical_gem_count(
    handle: *const CrimsonItemInfoHandle,
    out_count: *mut u32,
) -> i32 {
    if handle.is_null() || out_count.is_null() {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        unsafe { *out_count = h.canonical_gem_list.len() as u32 };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Read the canonical gem itemkey at sorted-ascending index `idx`.
/// Indexes are stable for the lifetime of a single handle (re-sorted
/// only on re-load).
///
/// Returns `OUT_OF_RANGE` when `idx >= canonical_gem_count`.
///
/// # Safety
/// `handle` and `out_gem_key` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_iteminfo_canonical_gem_at(
    handle: *const CrimsonItemInfoHandle,
    idx: u32,
    out_gem_key: *mut u32,
) -> i32 {
    if handle.is_null() || out_gem_key.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_gem_key = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some(g) = h.canonical_gem_list.get(idx as usize) else {
            return error::OUT_OF_RANGE;
        };
        unsafe { *out_gem_key = *g };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

// ── Static-metadata lookups (granular getters, share `summary_by_key`) ─────
//
// Each function below resolves `item_key` against the per-key
// `CrimsonItemInfoSummary` cache built once at load time, then writes a
// single scalar / bitmask out. Lookups are O(1) HashMap hits — no
// re-parsing of the underlying bytes. All return `NOT_FOUND` for unknown
// keys and `NULL_ARG` for null pointers; none of them touch `*out_*`
// until they've successfully resolved the key.

/// Read the static-flag bitmask for `item_key`. Bit definitions are the
/// `CRIMSON_ITEMINFO_FLAG_*` constants at the top of this module. The
/// returned value is also exposed as `CrimsonItemInfoSummary::flags`
/// when using the one-shot summary lookup.
///
/// # Safety
/// `handle` must be live, `out_flags` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_iteminfo_lookup_flags(
    handle: *const CrimsonItemInfoHandle,
    item_key: u32,
    out_flags: *mut u32,
) -> i32 {
    if handle.is_null() || out_flags.is_null() {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some(s) = h.summary_by_key.get(&item_key) else {
            return error::NOT_FOUND;
        };
        unsafe { *out_flags = s.flags };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

macro_rules! impl_iteminfo_scalar_lookup {
    ($fn_name:ident, $out_ty:ty, $field:ident) => {
        /// O(1) scalar lookup against the per-key summary cache.
        ///
        /// # Safety
        /// `handle` must be live; `out` must be non-null.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $fn_name(
            handle: *const CrimsonItemInfoHandle,
            item_key: u32,
            out: *mut $out_ty,
        ) -> i32 {
            if handle.is_null() || out.is_null() {
                return error::NULL_ARG;
            }
            catch_unwind(AssertUnwindSafe(|| {
                let h = unsafe { &*handle };
                let Some(s) = h.summary_by_key.get(&item_key) else {
                    return error::NOT_FOUND;
                };
                unsafe { *out = s.$field };
                error::OK
            }))
            .unwrap_or(error::PANIC)
        }
    };
}

impl_iteminfo_scalar_lookup!(crimson_iteminfo_lookup_item_type, u8, item_type);
impl_iteminfo_scalar_lookup!(crimson_iteminfo_lookup_item_tier, u8, item_tier);
impl_iteminfo_scalar_lookup!(
    crimson_iteminfo_lookup_quick_slot_index,
    u8,
    quick_slot_index
);
impl_iteminfo_scalar_lookup!(
    crimson_iteminfo_lookup_item_charge_type,
    u8,
    item_charge_type
);
impl_iteminfo_scalar_lookup!(
    crimson_iteminfo_lookup_usable_alert_type,
    u8,
    usable_alert_type
);
impl_iteminfo_scalar_lookup!(
    crimson_iteminfo_lookup_knowledge_obtain_type,
    u8,
    knowledge_obtain_type
);
impl_iteminfo_scalar_lookup!(
    crimson_iteminfo_lookup_apply_drop_stat_type,
    u8,
    apply_drop_stat_type
);
impl_iteminfo_scalar_lookup!(crimson_iteminfo_lookup_category_info, u16, category_info);
impl_iteminfo_scalar_lookup!(
    crimson_iteminfo_lookup_inventory_info,
    u16,
    inventory_info
);
impl_iteminfo_scalar_lookup!(crimson_iteminfo_lookup_max_endurance, u16, max_endurance);
impl_iteminfo_scalar_lookup!(
    crimson_iteminfo_lookup_minimum_extract_enchant_level,
    u16,
    minimum_extract_enchant_level
);
impl_iteminfo_scalar_lookup!(
    crimson_iteminfo_lookup_equip_type_info,
    u32,
    equip_type_info
);
impl_iteminfo_scalar_lookup!(crimson_iteminfo_lookup_equipable_hash, u32, equipable_hash);
impl_iteminfo_scalar_lookup!(
    crimson_iteminfo_lookup_equipable_level,
    u32,
    equipable_level
);
impl_iteminfo_scalar_lookup!(crimson_iteminfo_lookup_knowledge_info, u32, knowledge_info);
impl_iteminfo_scalar_lookup!(crimson_iteminfo_lookup_material_key, u32, material_key);
impl_iteminfo_scalar_lookup!(crimson_iteminfo_lookup_gimmick_info, u32, gimmick_info);
impl_iteminfo_scalar_lookup!(crimson_iteminfo_lookup_cooltime, i64, cooltime);
impl_iteminfo_scalar_lookup!(
    crimson_iteminfo_lookup_respawn_time_seconds,
    i64,
    respawn_time_seconds
);

// ── CrimsonItemInfoSummary (one-shot summary lookup) ───────────────────────

/// One-shot lookup that fills the entire static-metadata snapshot for
/// `item_key`. Saves a round-trip per scalar for callers that need most
/// fields at once (e.g. an editor refreshing its detail panel when the
/// user selects an inventory entry).
///
/// `*out_summary` is written only on success (`error::OK`); on
/// `NOT_FOUND` it's left untouched. The struct layout is frozen — see
/// `CrimsonItemInfoSummary` above; downstream consumers mirror it as a
/// `[StructLayout(LayoutKind.Sequential)]` C# struct.
///
/// # Safety
/// `handle` must be live; `out_summary` must be a writable pointer to
/// at least `sizeof(CrimsonItemInfoSummary) = 80` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_iteminfo_lookup_summary(
    handle: *const CrimsonItemInfoHandle,
    item_key: u32,
    out_summary: *mut CrimsonItemInfoSummary,
) -> i32 {
    if handle.is_null() || out_summary.is_null() {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some(s) = h.summary_by_key.get(&item_key) else {
            return error::NOT_FOUND;
        };
        unsafe { *out_summary = *s };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

// ── Enumeration ────────────────────────────────────────────────────────────

/// Get the `(item_key, string_key)` pair at insertion index `idx`.
/// Two-call pattern over `buf`; the `out_key` u32 is always written.
///
/// Returns `OUT_OF_RANGE` when `idx >= entry_count`.
///
/// # Safety
/// `handle`, `out_key`, and `required` must be non-null; `buf` may
/// be null iff `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_iteminfo_get_entry(
    handle: *const CrimsonItemInfoHandle,
    idx: u32,
    out_key: *mut u32,
    buf: *mut u8,
    buf_len: usize,
    required: *mut usize,
) -> i32 {
    if handle.is_null() || out_key.is_null() || required.is_null() {
        return error::NULL_ARG;
    }
    if buf.is_null() && buf_len != 0 {
        return error::NULL_ARG;
    }
    unsafe {
        *out_key = 0;
        *required = 0;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some((key, name)) = h.entries.get(idx as usize) else {
            return error::OUT_OF_RANGE;
        };
        unsafe { *out_key = *key };
        let needed = name.len() + 1;
        unsafe { *required = needed };
        if buf_len < needed {
            return error::BUFFER_TOO_SMALL;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(name.as_ptr(), buf, name.len());
            *buf.add(name.len()) = 0;
        }
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

#[cfg(test)]
mod tests {
    //! Live-install integration tests against the real iteminfo.pabgb.
    //! Skip cleanly when no Steam install is present. Synthesizing
    //! `iteminfo.pabgb` from scratch is impractical (each item is
    //! ~600 B with 100+ fields) so we rely on round-tripping the
    //! real bytes through PAZ extraction + the new C ABI.
    //!
    //! Coverage for the C ABI's error paths (NULL_ARG, BODY_PARSE on
    //! garbage, OUT_OF_RANGE, NOT_FOUND) uses synthetic inputs that
    //! exercise the wrappers without needing valid item bytes.

    use super::*;
    use crate::c_abi::paz::crimson_paz_extract_file;
    use std::ffi::CString;
    use std::path::PathBuf;
    use std::ptr;

    fn find_pamt_for_iteminfo() -> Option<PathBuf> {
        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let p = game_root.join("0008").join("0.pamt");
        p.is_file().then_some(p)
    }

    /// Pull iteminfo.pabgb via the standard PAZ path, returns its bytes.
    fn extract_iteminfo_bytes(pamt: &CStr) -> Vec<u8> {
        let dir = CString::new("gamedata/binary__/client/bin").unwrap();
        let name = CString::new("iteminfo.pabgb").unwrap();
        let mut needed: usize = 0;
        let rc = unsafe {
            crimson_paz_extract_file(
                pamt.as_ptr(),
                dir.as_ptr(),
                name.as_ptr(),
                ptr::null_mut(),
                0,
                &mut needed,
            )
        };
        assert_eq!(rc, error::BUFFER_TOO_SMALL, "first call should query size");
        let mut buf = vec![0u8; needed];
        let rc = unsafe {
            crimson_paz_extract_file(
                pamt.as_ptr(),
                dir.as_ptr(),
                name.as_ptr(),
                buf.as_mut_ptr(),
                buf.len(),
                &mut needed,
            )
        };
        assert_eq!(rc, error::OK);
        buf.truncate(needed);
        buf
    }

    use std::ffi::CStr;

    #[test]
    fn c_abi_iteminfo_live_roundtrip() {
        let Some(pamt_path) = find_pamt_for_iteminfo() else {
            eprintln!(
                "skipping c_abi_iteminfo_live_roundtrip: no 0008/0.pamt in game install"
            );
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let bytes = extract_iteminfo_bytes(pamt.as_c_str());

        let mut handle: *mut CrimsonItemInfoHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_iteminfo_load_from_bytes(bytes.as_ptr(), bytes.len(), &mut handle) },
            error::OK
        );
        assert!(!handle.is_null());

        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_iteminfo_entry_count(handle, &mut count) },
            error::OK
        );
        // 1.05 had ~6,236; 1.06 has ~6,400. Just assert plausibly populated.
        assert!(count > 5_000, "expected >5k items, got {count}");

        // ── Round-trip: pick entry 0, then look up its string_key by
        // the u32 we got back. Must match.
        let mut out_key: u32 = 0;
        let mut req: usize = 0;
        let rc = unsafe {
            crimson_iteminfo_get_entry(
                handle,
                0,
                &mut out_key,
                ptr::null_mut(),
                0,
                &mut req,
            )
        };
        assert_eq!(rc, error::BUFFER_TOO_SMALL);
        let mut buf = vec![0u8; req];
        let rc = unsafe {
            crimson_iteminfo_get_entry(
                handle,
                0,
                &mut out_key,
                buf.as_mut_ptr(),
                buf.len(),
                &mut req,
            )
        };
        assert_eq!(rc, error::OK);
        let enum_name = std::str::from_utf8(&buf[..req - 1]).unwrap().to_string();
        assert!(!enum_name.is_empty(), "item 0's string_key should be non-empty");

        // Now lookup by the u32 we just read.
        let mut req2: usize = 0;
        let rc = unsafe {
            crimson_iteminfo_lookup_string_key(
                handle,
                out_key,
                ptr::null_mut(),
                0,
                &mut req2,
            )
        };
        assert_eq!(rc, error::BUFFER_TOO_SMALL);
        let mut buf2 = vec![0u8; req2];
        let rc = unsafe {
            crimson_iteminfo_lookup_string_key(
                handle,
                out_key,
                buf2.as_mut_ptr(),
                buf2.len(),
                &mut req2,
            )
        };
        assert_eq!(rc, error::OK);
        let lookup_name = std::str::from_utf8(&buf2[..req2 - 1]).unwrap();
        assert_eq!(
            lookup_name, enum_name,
            "get_entry and lookup must agree for the same key"
        );

        // NOT_FOUND on a definitely-invalid item key (0 might be valid
        // depending on the data, so use a value picked to be outside
        // the range — u32::MAX is safely out of bounds).
        let mut req3: usize = 0;
        let rc = unsafe {
            crimson_iteminfo_lookup_string_key(
                handle,
                u32::MAX,
                ptr::null_mut(),
                0,
                &mut req3,
            )
        };
        assert_eq!(rc, error::NOT_FOUND);

        // ── max_stack lookup: round-trip against entry 0's key, then
        // assert u32::MAX is NOT_FOUND. Don't pin the value of any
        // specific item — the schema is the contract, not "Camp Funds
        // caps at 999999".
        let mut max_stack: u64 = 999;
        let rc = unsafe { crimson_iteminfo_lookup_max_stack(handle, out_key, &mut max_stack) };
        assert_eq!(rc, error::OK);

        let mut bogus: u64 = 0;
        let rc = unsafe { crimson_iteminfo_lookup_max_stack(handle, u32::MAX, &mut bogus) };
        assert_eq!(rc, error::NOT_FOUND);
        assert_eq!(bogus, 0, "out_max_stack should be reset on NOT_FOUND");

        // ── icon_path_hash lookup: at least one item in any real game
        // install ships an icon. Walk the table and assert there's at
        // least one hit, then assert u32::MAX is NOT_FOUND.
        let mut found_at_least_one_icon = false;
        for i in 0..count.min(2000) {
            let mut ik: u32 = 0;
            let mut req: usize = 0;
            let _ = unsafe {
                crimson_iteminfo_get_entry(
                    handle, i, &mut ik, ptr::null_mut(), 0, &mut req,
                )
            };
            let mut icon_hash: u32 = 0;
            let rc = unsafe {
                crimson_iteminfo_lookup_icon_path_hash(handle, ik, &mut icon_hash)
            };
            if rc == error::OK && icon_hash != 0 {
                found_at_least_one_icon = true;
                break;
            }
        }
        assert!(
            found_at_least_one_icon,
            "expected at least one item in the first 2000 to have an icon"
        );

        let mut bogus_icon: u32 = 0;
        let rc = unsafe {
            crimson_iteminfo_lookup_icon_path_hash(handle, u32::MAX, &mut bogus_icon)
        };
        assert_eq!(rc, error::NOT_FOUND);
        assert_eq!(bogus_icon, 0, "out_hash should be reset on NOT_FOUND");

        // OUT_OF_RANGE on get_entry past the end.
        let rc = unsafe {
            crimson_iteminfo_get_entry(
                handle,
                u32::MAX,
                &mut out_key,
                ptr::null_mut(),
                0,
                &mut req,
            )
        };
        assert_eq!(rc, error::OUT_OF_RANGE);

        unsafe { crimson_iteminfo_free(handle) };
        // free(null) is a no-op.
        unsafe { crimson_iteminfo_free(ptr::null_mut()) };
    }

    #[test]
    fn c_abi_iteminfo_garbage_bytes_returns_body_parse() {
        // 32 zero bytes won't parse as an ItemInfo (the leading u32
        // ItemKey reads as 0; the next CString reads a length of 0
        // then 0 bytes; then the next field tries to read more bytes
        // than remain → InvalidData → BODY_PARSE).
        let garbage = [0u8; 32];
        let mut handle: *mut CrimsonItemInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_iteminfo_load_from_bytes(garbage.as_ptr(), garbage.len(), &mut handle)
        };
        assert_eq!(rc, error::BODY_PARSE);
        assert!(handle.is_null());
    }

    #[test]
    fn c_abi_iteminfo_null_args() {
        let mut handle: *mut CrimsonItemInfoHandle = ptr::null_mut();
        assert_eq!(
            unsafe {
                crimson_iteminfo_load_from_bytes(ptr::null(), 16, &mut handle)
            },
            error::NULL_ARG
        );
        assert_eq!(
            unsafe {
                crimson_iteminfo_load_from_bytes([0u8; 1].as_ptr(), 1, ptr::null_mut())
            },
            error::NULL_ARG
        );

        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_iteminfo_entry_count(ptr::null(), &mut count) },
            error::NULL_ARG
        );

        let mut req: usize = 0;
        assert_eq!(
            unsafe {
                crimson_iteminfo_lookup_string_key(
                    ptr::null(),
                    0,
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            },
            error::NULL_ARG
        );
    }

    #[test]
    fn c_abi_iteminfo_load_bad_path_returns_io() {
        let bad = CString::new("Z:\\definitely\\does\\not\\exist.pabgb").unwrap();
        let mut handle: *mut CrimsonItemInfoHandle = ptr::null_mut();
        let rc = unsafe { crimson_iteminfo_load_from_file(bad.as_ptr(), &mut handle) };
        assert_eq!(rc, error::IO);
        assert!(handle.is_null());
    }

    /// Live-install integration test for the socket cross-check ABIs.
    /// Asserts the contract (advisory only — never blocks), then
    /// dumps the gamedata socket caps for the user's CE-modified
    /// reference items (1002285 / 1002284 / 1000316) so a regression
    /// in either the parser or our HashMap population is caught
    /// immediately. Skips cleanly when no game install is present.
    #[test]
    fn c_abi_iteminfo_socket_caps_and_gem_allow_live() {
        let Some(pamt_path) = find_pamt_for_iteminfo() else {
            eprintln!("skipping c_abi_iteminfo_socket_caps_live: no game install");
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let bytes = extract_iteminfo_bytes(pamt.as_c_str());

        let mut handle: *mut CrimsonItemInfoHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_iteminfo_load_from_bytes(bytes.as_ptr(), bytes.len(), &mut handle) },
            error::OK
        );
        assert!(!handle.is_null());

        // ── Lookup the 3 user-flagged CE-modified items. Print their
        // vanilla socket caps so the editor's allow/deny prompts can
        // contrast against the CE-mutated save state.
        let ce_items: &[(u32, &str)] = &[
            (1002285, "嘟嘟鳥放電盔甲 (armor)"),
            (1002284, "嘟嘟鳥馬羅尼雷射頭盔 (helmet)"),
            (1000316, "嘟嘟鳥里西的鞋子 (shoes)"),
        ];
        for &(key, label) in ce_items {
            let mut use_socket: u8 = 99;
            let mut valid_count: u8 = 99;
            let rc = unsafe {
                crimson_iteminfo_lookup_socket_caps(
                    handle, key, &mut use_socket, &mut valid_count,
                )
            };
            assert_eq!(rc, error::OK, "key {key} ({label}) must be in iteminfo");
            let mut gem_count: u32 = 0;
            assert_eq!(
                unsafe {
                    crimson_iteminfo_socket_allowed_gem_count(
                        handle, key, &mut gem_count,
                    )
                },
                error::OK
            );
            eprintln!(
                "  {} (key={}): use_socket={} valid_count={} allowed_gems={}",
                label, key, use_socket, valid_count, gem_count,
            );
        }

        // ── Find any item key that IS socket-capable and has a
        // non-empty allowed-gem list. Use it to exercise the
        // allow/disallow paths. We don't pin a specific item key here
        // because the table changes per patch.
        let mut socket_item_key: Option<u32> = None;
        let mut socket_allowed_gem: Option<u32> = None;
        unsafe {
            let h = &*handle;
            for (k, gems) in &h.socket_allowed_gems_by_key {
                if let Some(g) = gems.first() {
                    socket_item_key = Some(*k);
                    socket_allowed_gem = Some(*g);
                    break;
                }
            }
        }
        let Some(item_with_gems) = socket_item_key else {
            eprintln!(
                "no item with a non-empty allowed-gem list found — \
                 cannot exercise allow/disallow paths"
            );
            unsafe { crimson_iteminfo_free(handle) };
            return;
        };
        let allowed_gem = socket_allowed_gem.unwrap();
        eprintln!(
            "allow/deny exemplar: item_key={item_with_gems} allowed_gem={allowed_gem}",
        );

        // socket_allows_gem: allowed gem → out=1.
        let mut out_allowed: u8 = 99;
        let rc = unsafe {
            crimson_iteminfo_socket_allows_gem(
                handle, item_with_gems, allowed_gem, &mut out_allowed,
            )
        };
        assert_eq!(rc, error::OK);
        assert_eq!(out_allowed, 1, "the gem we pulled from the list MUST report as allowed");

        // socket_allows_gem: definitely-not-allowed gem → out=0,
        // but still OK (advisory). Use u32::MAX which can't be a real
        // gem itemkey.
        let mut out_disallowed: u8 = 99;
        let rc = unsafe {
            crimson_iteminfo_socket_allows_gem(
                handle, item_with_gems, u32::MAX, &mut out_disallowed,
            )
        };
        assert_eq!(rc, error::OK, "disallowed gem must still return OK — advisory only");
        assert_eq!(out_disallowed, 0);

        // socket_allowed_gem_count + _at: enumerate the allowed list,
        // confirm each entry maps back through socket_allows_gem.
        let mut count: u32 = 0;
        assert_eq!(
            unsafe {
                crimson_iteminfo_socket_allowed_gem_count(
                    handle, item_with_gems, &mut count,
                )
            },
            error::OK
        );
        assert!(count > 0, "allow-list should be non-empty");
        for i in 0..count {
            let mut g: u32 = 0;
            assert_eq!(
                unsafe {
                    crimson_iteminfo_socket_allowed_gem_at(
                        handle, item_with_gems, i, &mut g,
                    )
                },
                error::OK
            );
            let mut a: u8 = 0;
            assert_eq!(
                unsafe {
                    crimson_iteminfo_socket_allows_gem(
                        handle, item_with_gems, g, &mut a,
                    )
                },
                error::OK
            );
            assert_eq!(a, 1, "enumerated gem {g} must report as allowed");
        }
        // OUT_OF_RANGE past the end.
        let mut g: u32 = 0;
        assert_eq!(
            unsafe {
                crimson_iteminfo_socket_allowed_gem_at(
                    handle, item_with_gems, count, &mut g,
                )
            },
            error::OUT_OF_RANGE
        );

        // ── NOT_FOUND: every entry point on a missing item key.
        let mut use_s: u8 = 0;
        let mut vc: u8 = 0;
        assert_eq!(
            unsafe {
                crimson_iteminfo_lookup_socket_caps(
                    handle, u32::MAX, &mut use_s, &mut vc,
                )
            },
            error::NOT_FOUND
        );
        let mut a: u8 = 0;
        assert_eq!(
            unsafe {
                crimson_iteminfo_socket_allows_gem(
                    handle, u32::MAX, allowed_gem, &mut a,
                )
            },
            error::NOT_FOUND
        );
        let mut c: u32 = 0;
        assert_eq!(
            unsafe {
                crimson_iteminfo_socket_allowed_gem_count(
                    handle, u32::MAX, &mut c,
                )
            },
            error::NOT_FOUND
        );
        let mut g2: u32 = 0;
        assert_eq!(
            unsafe {
                crimson_iteminfo_socket_allowed_gem_at(
                    handle, u32::MAX, 0, &mut g2,
                )
            },
            error::NOT_FOUND
        );

        // ── Non-socket-capable item: lookup_socket_caps returns OK
        // with use_socket=0. socket_allows_gem returns OK with
        // out_allowed=0. socket_allowed_gem_count returns OK with 0.
        // Find any item with use_socket=0.
        let mut non_socket_key: Option<u32> = None;
        unsafe {
            let h = &*handle;
            for (k, (use_flag, _)) in &h.socket_caps_by_key {
                if *use_flag == 0 {
                    non_socket_key = Some(*k);
                    break;
                }
            }
        }
        if let Some(k) = non_socket_key {
            let mut us: u8 = 99;
            let mut vc: u8 = 99;
            assert_eq!(
                unsafe {
                    crimson_iteminfo_lookup_socket_caps(handle, k, &mut us, &mut vc)
                },
                error::OK
            );
            assert_eq!(us, 0, "use_socket should be 0 for non-socket items");

            let mut a: u8 = 99;
            assert_eq!(
                unsafe {
                    crimson_iteminfo_socket_allows_gem(handle, k, allowed_gem, &mut a)
                },
                error::OK
            );
            assert_eq!(a, 0, "non-socket items disallow every gem");

            let mut c: u32 = 99;
            assert_eq!(
                unsafe {
                    crimson_iteminfo_socket_allowed_gem_count(handle, k, &mut c)
                },
                error::OK
            );
            assert_eq!(c, 0, "non-socket items have empty allowed-gem list");
        }

        // NULL_ARG paths.
        let mut us: u8 = 0;
        let mut vc: u8 = 0;
        assert_eq!(
            unsafe {
                crimson_iteminfo_lookup_socket_caps(ptr::null(), 0, &mut us, &mut vc)
            },
            error::NULL_ARG
        );
        assert_eq!(
            unsafe {
                crimson_iteminfo_socket_allows_gem(ptr::null(), 0, 0, &mut a)
            },
            error::NULL_ARG
        );
        assert_eq!(
            unsafe {
                crimson_iteminfo_socket_allowed_gem_count(ptr::null(), 0, &mut c)
            },
            error::NULL_ARG
        );
        assert_eq!(
            unsafe {
                crimson_iteminfo_socket_allowed_gem_at(ptr::null(), 0, 0, &mut g2)
            },
            error::NULL_ARG
        );

        unsafe { crimson_iteminfo_free(handle) };
    }

    /// Diagnostic — surveys the artifact ↔ challenge mapping carried
    /// by `iteminfo.look_detail_mission_info`. Resolves item display
    /// names (via PALOC `(itemkey<<32)|0x70`) and mission internal
    /// names (via missioninfo) so we can see the actual mapping data
    /// and decide what to ship as an API surface.
    ///
    /// What we want to learn from this probe:
    ///   - How many items carry a non-zero look_detail_mission_info?
    ///   - What naming pattern do those mission internal_names have
    ///     (e.g. "Challenge_*", "Catalog_*", "Mission_*Artifact*")?
    ///   - Is the mapping 1:1 (one artifact per mission) or 1:N
    ///     (multiple artifacts can start the same challenge)?
    ///   - What fraction of missions have NO artifact pointing at them
    ///     (those are challenges that start by other triggers)?
    #[test]
    #[ignore = "investigation only — artifact ↔ catalog-challenge mapping survey"]
    fn _probe_artifact_challenge_mapping() {
        use crate::mission_info::parse_mission_info_lossy;

        let Some(pamt_path) = find_pamt_for_iteminfo() else {
            eprintln!("skipping: no game install");
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let iteminfo_bytes = extract_iteminfo_bytes(pamt.as_c_str());

        // Pull missioninfo too.
        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let pamt_data = std::fs::read(game_root.join("0008").join("0.pamt"))
            .expect("read 0008 pamt");
        let pamt_parsed = crate::binary::pamt::PackMeta::parse(&pamt_data, None)
            .expect("parse pamt");
        let dir = pamt_parsed.directories.iter()
            .find(|d| d.path == "gamedata/binary__/client/bin")
            .expect("dir");
        let mi_file = dir.files.iter().find(|f| f.name == "missioninfo.pabgb")
            .expect("missioninfo");
        let missioninfo_bytes = crate::binary::paz::extract_file(
            &game_root.join("0008"),
            mi_file,
            "gamedata/binary__/client/bin",
            &pamt_parsed.header.encrypt_info.encrypt_info,
        ).expect("extract missioninfo");
        let mission_entries = parse_mission_info_lossy(&missioninfo_bytes);
        let mission_name_by_key: std::collections::HashMap<u32, String> = mission_entries
            .iter()
            .map(|e| (e.key, e.name.clone()))
            .collect();

        // Build iteminfo handle.
        let mut h: *mut CrimsonItemInfoHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_iteminfo_load_from_bytes(iteminfo_bytes.as_ptr(), iteminfo_bytes.len(), &mut h) },
            error::OK
        );
        let handle = unsafe { &*h };

        // Pull eng PALOC for item display names.
        let paloc_pamt = game_root.join("0020").join("0.pamt");
        let paloc_handle = if paloc_pamt.is_file() {
            let pamt_c = CString::new(paloc_pamt.to_str().unwrap()).unwrap();
            let dir_c = CString::new("gamedata/stringtable/binary__").unwrap();
            let name_c = CString::new("localizationstring_eng.paloc").unwrap();
            let mut needed: usize = 0;
            let _ = unsafe {
                crimson_paz_extract_file(
                    pamt_c.as_ptr(), dir_c.as_ptr(), name_c.as_ptr(),
                    ptr::null_mut(), 0, &mut needed,
                )
            };
            let mut buf = vec![0u8; needed];
            unsafe {
                crimson_paz_extract_file(
                    pamt_c.as_ptr(), dir_c.as_ptr(), name_c.as_ptr(),
                    buf.as_mut_ptr(), buf.len(), &mut needed,
                );
            }
            buf.truncate(needed);
            let mut ph: *mut crate::c_abi::paloc::CrimsonPalocHandle = ptr::null_mut();
            unsafe {
                crate::c_abi::paloc::crimson_paloc_load_from_bytes(
                    buf.as_ptr(), buf.len(), &mut ph,
                );
            }
            Some(ph)
        } else {
            None
        };

        // Walk every item carrying a non-zero look_detail_mission_info.
        // Group: mission_key → Vec<(item_key, item_display)>.
        let mut by_mission: std::collections::BTreeMap<u32, Vec<(u32, String)>> = Default::default();
        for (item_key, mission_key) in &handle.look_detail_mission_by_key {
            let display = if let Some(ph) = paloc_handle {
                let u64_key = (u64::from(*item_key) << 32) | 0x70u64;
                let decimal = format!("{u64_key}");
                let p = unsafe { &*ph };
                p.lookup_str(&decimal).unwrap_or("<no display>").to_string()
            } else {
                "<paloc unavailable>".to_string()
            };
            by_mission.entry(*mission_key).or_default().push((*item_key, display));
        }

        // ── Top-level stats ────────────────────────────────────────
        eprintln!("\n=== iteminfo → mission mapping survey ===");
        eprintln!("  total items in iteminfo:                  {}", handle.entries.len());
        eprintln!("  items with look_detail_mission_info != 0: {}",
            handle.look_detail_mission_by_key.len());
        eprintln!("  distinct missions pointed at:             {}", by_mission.len());

        let with_multi = by_mission.values().filter(|v| v.len() > 1).count();
        eprintln!("  missions with >1 artifact pointing at them: {}", with_multi);

        // ── Mission-name pattern stats — which prefixes show up most?
        let mut prefix_hist: std::collections::BTreeMap<String, u32> = Default::default();
        let mut missing_in_missioninfo = 0u32;
        for mission_key in by_mission.keys() {
            match mission_name_by_key.get(mission_key) {
                Some(name) => {
                    // First 2 path segments of the internal name.
                    let prefix: String = name.split('_').take(2).collect::<Vec<_>>().join("_");
                    *prefix_hist.entry(prefix).or_insert(0) += 1;
                }
                None => missing_in_missioninfo += 1,
            }
        }
        eprintln!("\n  mission-name prefix histogram (top 15):");
        let mut sorted_prefixes: Vec<_> = prefix_hist.iter().collect();
        sorted_prefixes.sort_by(|a, b| b.1.cmp(a.1));
        for (prefix, count) in sorted_prefixes.iter().take(15) {
            eprintln!("    {:6} × {}", count, prefix);
        }
        eprintln!("  missions pointed-at but not found in missioninfo: {}",
            missing_in_missioninfo);

        // ── Sample dump: 25 (mission, [items]) entries
        eprintln!("\n  sample (first 25 missions; mission_name → [items]):");
        for (i, (mission_key, items)) in by_mission.iter().take(25).enumerate() {
            let mname = mission_name_by_key.get(mission_key)
                .map(|s| s.as_str())
                .unwrap_or("<not in missioninfo>");
            let items_str: Vec<String> = items.iter()
                .map(|(ik, n)| format!("{ik}:{n}"))
                .collect();
            eprintln!("    [{:2}] mission={} ({}) → {} item(s):", i, mission_key, mname, items.len());
            for it in &items_str {
                eprintln!("           {it}");
            }
        }

        // ── Multi-artifact missions — interesting case
        eprintln!("\n  missions with multiple artifacts (1:N case):");
        let mut multi_count = 0;
        for (mission_key, items) in by_mission.iter().filter(|(_, v)| v.len() > 1) {
            let mname = mission_name_by_key.get(mission_key)
                .map(|s| s.as_str())
                .unwrap_or("<not in missioninfo>");
            eprintln!("    mission={} ({}) → {} items: {:?}",
                mission_key, mname, items.len(),
                items.iter().map(|(k, n)| format!("{k}:{n}")).collect::<Vec<_>>());
            multi_count += 1;
            if multi_count >= 10 { break; }
        }

        // ── Universe — total missions vs missions-with-artifacts
        eprintln!("\n  universe context:");
        eprintln!("    total missions in missioninfo: {}", mission_entries.len());
        let with_artifact = by_mission.len() as f64;
        let total_missions = mission_entries.len() as f64;
        eprintln!(
            "    fraction of missions started by an artifact: {:.1}% ({}/{})",
            (with_artifact / total_missions) * 100.0,
            by_mission.len(), mission_entries.len(),
        );

        // Cleanup
        unsafe { crimson_iteminfo_free(h) };
        if let Some(ph) = paloc_handle {
            unsafe { crate::c_abi::paloc::crimson_paloc_free(ph) };
        }
    }

    /// Diagnostic — dumps the iteminfo fields for known gem itemkeys
    /// to figure out which field reliably marks something as a gem.
    /// Items investigated: 1002979 (爆走的力量審判, durability gem
    /// the user verified), 1002972/3/4 (the gem trio in 嘟嘟鳥盔甲),
    /// 1002815 + 1002848 (no-durability gems in the same armor),
    /// 1000316 (one of the host items, for contrast).
    #[test]
    #[ignore = "investigation only — find the iteminfo field that classifies a gem"]
    fn _probe_iteminfo_gem_classification() {
        use crate::binary::BinaryRead;
        use crate::item_info::ItemInfo;

        let Some(pamt_path) = find_pamt_for_iteminfo() else {
            eprintln!("skipping: no game install");
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let bytes = extract_iteminfo_bytes(pamt.as_c_str());

        // Targets: known gems + a host item for contrast.
        let targets: std::collections::HashSet<u32> = [
            1002979, // 爆走的力量審判 (durability gem)
            1002972, 1002973, 1002974, // gem trio in helmet/armor/shoes
            1002815, 1002848, // no-durability gems in armor
            1000316, // shoes (host item)
            1002285, // armor (host item)
        ].into_iter().collect();

        let mut offset = 0usize;
        let mut found = std::collections::HashMap::<u32, ItemInfo>::new();
        while offset < bytes.len() {
            let item = ItemInfo::read_from(&bytes, &mut offset).unwrap();
            if targets.contains(&item.key.0) {
                found.insert(item.key.0, item);
            }
        }

        eprintln!("\n=== iteminfo field dump for target items ===");
        for key in [
            1002979u32, 1002972, 1002973, 1002974, 1002815, 1002848, 1000316, 1002285,
        ] {
            let Some(item) = found.get(&key) else {
                eprintln!("  key {key}: NOT FOUND");
                continue;
            };
            eprintln!(
                "\n  key={} string_key={:?}",
                key, item.string_key.data
            );
            eprintln!("    item_type={}", item.item_type);
            eprintln!("    category_info={:?}", item.category_info);
            eprintln!("    equip_type_info={:?}", item.equip_type_info);
            eprintln!("    item_tier={}", item.item_tier);
            eprintln!("    filter_type={:?}", item.filter_type.data);
            eprintln!("    item_tag_list={:?}", item.item_tag_list.items);
            eprintln!("    is_dyeable={} is_destroy_when_broken={}",
                item.is_dyeable, item.is_destroy_when_broken);
            eprintln!("    max_stack={} apply_max_stack_cap={}",
                item.max_stack_count, item.apply_max_stack_cap);
            eprintln!(
                "    drop_default_data: use_socket={} valid_count={} \
                 socket_item_list_count={} add_socket_material_item_list_count={}",
                item.drop_default_data.use_socket,
                item.drop_default_data.socket_valid_count,
                item.drop_default_data.socket_item_list.items.len(),
                item.drop_default_data.add_socket_material_item_list.items.len(),
            );
        }
    }

    /// Live-install test for the canonical gem-set ABIs. Confirms the
    /// list is non-empty, strictly sorted ascending, every entry
    /// appears in at least one weapon's allowed-gem list, and the 43
    /// gem itemkeys we observed in slot104's saves are all present.
    /// Also dumps the first 20 entries for visibility.
    #[test]
    fn c_abi_iteminfo_canonical_gem_set_live() {
        let Some(pamt_path) = find_pamt_for_iteminfo() else {
            eprintln!("skipping c_abi_iteminfo_canonical_gem_set_live: no game install");
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let bytes = extract_iteminfo_bytes(pamt.as_c_str());

        let mut handle: *mut CrimsonItemInfoHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_iteminfo_load_from_bytes(bytes.as_ptr(), bytes.len(), &mut handle) },
            error::OK
        );

        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_iteminfo_canonical_gem_count(handle, &mut count) },
            error::OK
        );
        assert!(count > 30, "expected >30 canonical gems, got {count}");

        // Walk + assert sorted-ascending.
        let mut prev: u32 = 0;
        let mut all: Vec<u32> = Vec::with_capacity(count as usize);
        for i in 0..count {
            let mut g: u32 = 0;
            assert_eq!(
                unsafe { crimson_iteminfo_canonical_gem_at(handle, i, &mut g) },
                error::OK
            );
            assert!(
                g > prev,
                "canonical_gem_list must be strictly ascending; got {prev} then {g}"
            );
            prev = g;
            all.push(g);
        }
        eprintln!(
            "canonical gem count: {} | first 20: {:?}",
            count,
            &all[..20.min(all.len())]
        );

        // OUT_OF_RANGE past the end.
        let mut g: u32 = 0;
        assert_eq!(
            unsafe { crimson_iteminfo_canonical_gem_at(handle, count, &mut g) },
            error::OUT_OF_RANGE
        );

        // Cross-check: pin the 43 gem itemkeys observed in slot104's
        // saves — every one of them MUST be in the canonical set.
        // (If a future patch drops a gem from all allowed lists, the
        // canonical set would shrink — this test surfaces that.)
        const SLOT104_GEMS: &[u32] = &[
            1001426, 1002467, 1002499, 1002509, 1002523, 1002539, 1002548, 1002552,
            1002554, 1002569, 1002580, 1002751, 1002785, 1002791, 1002794, 1002796,
            1002797, 1002807, 1002810, 1002815, 1002848, 1002862, 1002898, 1002907,
            1002913, 1002953, 1002969, 1002970, 1002972, 1002973, 1002974, 1002976,
            1002977, 1002978, 1002979, 1002980, 1003290, 1003686, 1003702, 1003714,
            1003718, 1003761, 1003767,
        ];
        let gem_set: std::collections::HashSet<u32> = all.iter().copied().collect();
        let mut missing: Vec<u32> = Vec::new();
        for &g in SLOT104_GEMS {
            if !gem_set.contains(&g) {
                missing.push(g);
            }
        }
        assert!(
            missing.is_empty(),
            "{} gems observed in slot104 are missing from the canonical set: {:?}",
            missing.len(),
            missing,
        );

        // Cross-check #2: the canonical set must be a SUPERSET of the
        // union of per-weapon socket_item_list + add_socket_material_item_list
        // (every itemkey shown as "vendor allowed" must also be
        // gem-classified in iteminfo — otherwise the schema would be
        // self-inconsistent). The reverse isn't true: many gems exist
        // in the canonical set that no specific weapon's vendor list
        // names.
        let canonical_set: std::collections::HashSet<u32> = all.iter().copied().collect();
        let union_from_lists: std::collections::HashSet<u32> = unsafe {
            (*handle)
                .socket_allowed_gems_by_key
                .values()
                .flatten()
                .copied()
                .collect()
        };
        let vendor_not_canonical: Vec<u32> =
            union_from_lists.difference(&canonical_set).copied().collect();
        // Allow a small slack: vendor lists sometimes include
        // pseudo-itemkeys (like literal `1`) that aren't real gems.
        // We tolerate up to a handful of these; the important
        // invariant is "almost-every vendor entry is gem-classified".
        let slack = (union_from_lists.len() / 20).max(5);
        assert!(
            vendor_not_canonical.len() <= slack,
            "{} per-weapon vendor entries are not in the canonical gem set \
             (slack threshold = {}): {:?}",
            vendor_not_canonical.len(),
            slack,
            vendor_not_canonical,
        );

        // NULL_ARG paths.
        let mut c: u32 = 0;
        assert_eq!(
            unsafe { crimson_iteminfo_canonical_gem_count(ptr::null(), &mut c) },
            error::NULL_ARG
        );
        let mut g2: u32 = 0;
        assert_eq!(
            unsafe { crimson_iteminfo_canonical_gem_at(ptr::null(), 0, &mut g2) },
            error::NULL_ARG
        );

        unsafe { crimson_iteminfo_free(handle) };
    }

    /// Validates the artifact ↔ challenge bidirectional mapping is
    /// 1:1 and round-trips correctly. The phase-3 probe
    /// `_probe_artifact_challenge_mapping` confirmed:
    /// - 141 items carry a non-zero `look_detail_mission_info`
    /// - All 141 missions they point at are `Challenge_SealedArtifact_*`
    /// - Mapping is 1:1 (no mission target by >1 artifact)
    ///
    /// This test asserts the same invariants via the C ABI surface
    /// and pins a handful of (item, mission) tuples from the probe.
    /// Pinned tuples catch both schema regression (parser misses the
    /// field) and Pearl Abyss rebinding (an artifact is reassigned to
    /// a different challenge).
    #[test]
    fn c_abi_iteminfo_artifact_challenge_roundtrip_live() {
        let Some(pamt_path) = find_pamt_for_iteminfo() else {
            eprintln!("skipping c_abi_iteminfo_artifact_challenge_roundtrip_live: no game install");
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let bytes = extract_iteminfo_bytes(pamt.as_c_str());

        let mut handle: *mut CrimsonItemInfoHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_iteminfo_load_from_bytes(bytes.as_ptr(), bytes.len(), &mut handle) },
            error::OK
        );

        // ── Pinned (artifact_item_key, challenge_mission_key) pairs from
        // the probe output, covering several weapon-mastery tracks +
        // hunting + challenge-and-change. If PA rebinds any of these
        // the test fires.
        let pins: &[(u32, u32, &str)] = &[
            (1002038, 1000138, "Hunting_XI"),
            (1001984, 1000143, "Mastery_Battle_X"),
            (1002035, 1000144, "ChallengeAndChange_VII"),
            (1000826, 1000209, "Mastery_OneHandSword_I"),
            (1002008, 1000214, "Mastery_OneHandSword_II"),
            (1000850, 1000217, "Mastery_OneHandSword_III"),
            (1001987, 1000602, "Mastery_Bow_I"),
            (1002004, 1000605, "Mastery_Battle_I"),
        ];

        for &(item_key, mission_key, label) in pins {
            // Forward: item → mission.
            let mut got_mission: u32 = 0;
            let rc = unsafe {
                crimson_iteminfo_lookup_look_detail_mission_info(
                    handle, item_key, &mut got_mission,
                )
            };
            assert_eq!(rc, error::OK, "forward lookup failed for {label} (item {item_key})");
            assert_eq!(
                got_mission, mission_key,
                "{label}: forward mapping drifted (got mission {got_mission}, expected {mission_key})"
            );

            // Reverse: mission → item.
            let mut got_item: u32 = 0;
            let rc = unsafe {
                crimson_iteminfo_lookup_artifact_for_mission(
                    handle, mission_key, &mut got_item,
                )
            };
            assert_eq!(rc, error::OK, "reverse lookup failed for {label} (mission {mission_key})");
            assert_eq!(
                got_item, item_key,
                "{label}: reverse mapping drifted (got item {got_item}, expected {item_key})"
            );
        }

        // ── Full-table consistency: every forward entry must have a
        // matching reverse entry, and vice versa (1:1 invariant).
        unsafe {
            let h = &*handle;
            assert_eq!(
                h.look_detail_mission_by_key.len(),
                h.artifact_by_mission.len(),
                "forward and reverse maps disagree on entry count — 1:1 invariant broken \
                 (this would mean ≥2 artifacts point at the same mission)"
            );
            for (item_key, mission_key) in &h.look_detail_mission_by_key {
                let reverse_item = h.artifact_by_mission.get(mission_key)
                    .unwrap_or_else(|| panic!(
                        "forward says item {item_key} → mission {mission_key}, but reverse map missing"
                    ));
                assert_eq!(
                    reverse_item, item_key,
                    "round-trip broken: item {item_key} → mission {mission_key} → item {reverse_item}"
                );
            }
        }

        // ── NOT_FOUND paths.
        let mut zero: u32 = 99;
        let rc = unsafe {
            crimson_iteminfo_lookup_artifact_for_mission(
                handle, u32::MAX, &mut zero,
            )
        };
        assert_eq!(rc, error::NOT_FOUND);
        assert_eq!(zero, 0, "out_item_key must be reset on NOT_FOUND");

        // ── NULL_ARG paths.
        assert_eq!(
            unsafe {
                crimson_iteminfo_lookup_artifact_for_mission(
                    ptr::null(), 0, &mut zero,
                )
            },
            error::NULL_ARG
        );
        assert_eq!(
            unsafe {
                crimson_iteminfo_lookup_artifact_for_mission(
                    handle, 0, ptr::null_mut(),
                )
            },
            error::NULL_ARG
        );

        unsafe { crimson_iteminfo_free(handle) };
    }

    /// Validates the full editor-side pipeline for the gem-picker:
    /// `canonical_gem_count/_at → iteminfo string_key → PALOC display
    /// name at lo32=0x70`. Asserts every one of the 190 canonical
    /// gems resolves to a non-empty display name (so the C# editor
    /// can build the entire gem-picker dropdown end-to-end at startup
    /// without any hardcoded mapping).
    ///
    /// Cross-references a handful of (itemkey, expected display name)
    /// pairs from the user's hand-verified gem list so a regression
    /// in either iteminfo classification or PALOC lookup gets caught
    /// immediately.
    #[test]
    fn c_abi_iteminfo_canonical_gems_resolve_to_paloc_display_names() {
        let Some(pamt_path) = find_pamt_for_iteminfo() else {
            eprintln!(
                "skipping c_abi_iteminfo_canonical_gems_resolve_to_paloc_display_names: no game install"
            );
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let bytes = extract_iteminfo_bytes(pamt.as_c_str());

        let mut iteminfo_handle: *mut CrimsonItemInfoHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_iteminfo_load_from_bytes(bytes.as_ptr(), bytes.len(), &mut iteminfo_handle) },
            error::OK
        );

        // ── Pull eng PALOC from 0020/0.pamt
        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let paloc_pamt = game_root.join("0020").join("0.pamt");
        let Some(paloc_pamt) = paloc_pamt.is_file().then_some(paloc_pamt) else {
            eprintln!("skipping: no 0020/0.pamt for PALOC");
            unsafe { crimson_iteminfo_free(iteminfo_handle) };
            return;
        };
        let paloc_pamt_c = CString::new(paloc_pamt.to_str().unwrap()).unwrap();
        let paloc_dir = CString::new("gamedata/stringtable/binary__").unwrap();
        let paloc_name = CString::new("localizationstring_eng.paloc").unwrap();
        let mut needed: usize = 0;
        let _ = unsafe {
            crimson_paz_extract_file(
                paloc_pamt_c.as_ptr(),
                paloc_dir.as_ptr(),
                paloc_name.as_ptr(),
                ptr::null_mut(),
                0,
                &mut needed,
            )
        };
        let mut paloc_buf = vec![0u8; needed];
        assert_eq!(
            unsafe {
                crimson_paz_extract_file(
                    paloc_pamt_c.as_ptr(),
                    paloc_dir.as_ptr(),
                    paloc_name.as_ptr(),
                    paloc_buf.as_mut_ptr(),
                    paloc_buf.len(),
                    &mut needed,
                )
            },
            error::OK
        );
        paloc_buf.truncate(needed);

        use crate::c_abi::paloc::{CrimsonPalocHandle, crimson_paloc_free, crimson_paloc_load_from_bytes};
        let mut paloc_handle: *mut CrimsonPalocHandle = ptr::null_mut();
        assert_eq!(
            unsafe {
                crimson_paloc_load_from_bytes(paloc_buf.as_ptr(), paloc_buf.len(), &mut paloc_handle)
            },
            error::OK
        );

        // ── Walk every canonical gem; resolve display via PALOC.
        let mut gem_count: u32 = 0;
        assert_eq!(
            unsafe { crimson_iteminfo_canonical_gem_count(iteminfo_handle, &mut gem_count) },
            error::OK
        );

        let paloc = unsafe { &*paloc_handle };
        let mut resolved = 0u32;
        let mut missing: Vec<u32> = Vec::new();
        let mut sample: Vec<(u32, String)> = Vec::new();
        for i in 0..gem_count {
            let mut gem_key: u32 = 0;
            assert_eq!(
                unsafe {
                    crimson_iteminfo_canonical_gem_at(iteminfo_handle, i, &mut gem_key)
                },
                error::OK
            );
            let paloc_u64 = (u64::from(gem_key) << 32) | 0x70u64;
            let decimal = format!("{paloc_u64}");
            match paloc.lookup_str(&decimal) {
                Some(display) if !display.is_empty() => {
                    resolved += 1;
                    if sample.len() < 5 {
                        sample.push((gem_key, display.to_owned()));
                    }
                }
                _ => missing.push(gem_key),
            }
        }

        eprintln!(
            "\ncanonical gem PALOC pipeline: {}/{} resolved",
            resolved, gem_count
        );
        eprintln!("  sample: {:?}", sample);
        if !missing.is_empty() {
            eprintln!("  missing display names ({}): {:?}", missing.len(), missing);
        }
        // Pin: every canonical gem MUST resolve through PALOC. If a
        // future patch adds a gem-classified item with no display
        // name, that's worth catching as a heads-up (the C# editor
        // would show an empty row in the dropdown).
        assert_eq!(
            missing.len(), 0,
            "{} canonical gems have no eng PALOC display name — \
             editor's gem-picker would show blank rows for these",
            missing.len(),
        );

        // Cross-check a handful of (itemkey, display_name) from the
        // user's hand-verified list. If PA renames a gem, this
        // catches it.
        let pins: &[(u32, &str)] = &[
            (1002785, "Destruction I"),
            (1002787, "Destruction III"),
            (1002796, "Fortification III"),
            (1002979, "Greater Malicebane"),
            (1002972, "Greater Flameward"),
            (1002862, "Greater Destruction"),
            (1001424, "Flameward I"),
            (1001426, "Flameward III"),
            (1003290, "Spirit Transference"),
            (1001067, "Relentless"),
        ];
        for &(key, expected) in pins {
            let paloc_u64 = (u64::from(key) << 32) | 0x70u64;
            let decimal = format!("{paloc_u64}");
            let display = paloc.lookup_str(&decimal)
                .unwrap_or_else(|| panic!("gem {key} ({expected}) has no PALOC entry"));
            assert_eq!(
                display, expected,
                "gem {key} display name drifted (PA renamed?)",
            );
        }

        unsafe {
            crimson_paloc_free(paloc_handle);
            crimson_iteminfo_free(iteminfo_handle);
        }
    }

    // ── Static-metadata lookup tests (Option A + Option B) ────────────────
    //
    // Pin the granular flag / scalar getters and the one-shot summary
    // struct against two known items in 1.08:
    //   - Pyeonjeon_Arrow (key=2200): a consumable arrow with
    //     item_type=0, item_tier=0, and is_equip_quick_slot_visible=0
    //     (it's a quiver/consumable, not equipment).
    //   - Marni_Devotee_PlateArmor_Helm (key=14510): equipment with
    //     item_type=24, is_equip_quick_slot_visible=1, populated
    //     docking_child_data, and a real `equip_type_info`.
    #[test]
    fn c_abi_iteminfo_static_lookups_live() {
        let Some(pamt_path) = find_pamt_for_iteminfo() else {
            eprintln!("skipping c_abi_iteminfo_static_lookups_live: no game install");
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let bytes = extract_iteminfo_bytes(pamt.as_c_str());

        let mut handle: *mut CrimsonItemInfoHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_iteminfo_load_from_bytes(bytes.as_ptr(), bytes.len(), &mut handle) },
            error::OK
        );
        assert!(!handle.is_null());

        // ── Pyeonjeon_Arrow (key=2200) ──────────────────────────────
        let mut item_type: u8 = 0xff;
        let mut item_tier: u8 = 0xff;
        let mut qs_idx: u8 = 0xff;
        let mut flags: u32 = 0;
        assert_eq!(
            unsafe { crimson_iteminfo_lookup_item_type(handle, 2200, &mut item_type) },
            error::OK
        );
        assert_eq!(
            unsafe { crimson_iteminfo_lookup_item_tier(handle, 2200, &mut item_tier) },
            error::OK
        );
        assert_eq!(
            unsafe { crimson_iteminfo_lookup_quick_slot_index(handle, 2200, &mut qs_idx) },
            error::OK
        );
        assert_eq!(
            unsafe { crimson_iteminfo_lookup_flags(handle, 2200, &mut flags) },
            error::OK
        );
        // 1.13 remapped the ammo item_type: Pyeonjeon_Arrow read 0 through 1.12,
        // now reads 23 (item_type==0 no longer occurs in 1.13). item_tier /
        // quick_slot_index / flags below are unchanged, confirming the field is
        // correctly aligned (a genuine game-side enum change, not a parse drift).
        assert_eq!(item_type, 23, "Pyeonjeon_Arrow item_type drifted (0 in ≤1.12, 23 in 1.13)");
        assert_eq!(item_tier, 0, "Pyeonjeon_Arrow item_tier drifted");
        assert_eq!(qs_idx, 1, "Pyeonjeon_Arrow quick_slot_index drifted");
        // Consumable arrow — should NOT be on the new equip-quick-bar
        // (only weapons / armor / accessories / tool-slot tools are).
        assert_eq!(
            flags & CRIMSON_ITEMINFO_FLAG_IS_EQUIP_QUICK_SLOT_VISIBLE,
            0,
            "Pyeonjeon_Arrow should not have IS_EQUIP_QUICK_SLOT_VISIBLE set",
        );
        assert_eq!(
            flags & CRIMSON_ITEMINFO_FLAG_IS_HOUSING_ONLY,
            0,
            "Pyeonjeon_Arrow should not be housing-only",
        );

        // ── Marni_Devotee_PlateArmor_Helm (key=14510) ───────────────
        assert_eq!(
            unsafe { crimson_iteminfo_lookup_item_type(handle, 14510, &mut item_type) },
            error::OK
        );
        assert_eq!(
            unsafe { crimson_iteminfo_lookup_flags(handle, 14510, &mut flags) },
            error::OK
        );
        assert_eq!(item_type, 24, "Marni helm item_type drifted (was 24 in 1.08)");
        // Equipment — must have the new 1.08 quick-slot-visible bit set.
        assert!(
            flags & CRIMSON_ITEMINFO_FLAG_IS_EQUIP_QUICK_SLOT_VISIBLE != 0,
            "Marni helm missing IS_EQUIP_QUICK_SLOT_VISIBLE; flags=0x{flags:08x}",
        );
        // Helms are not housing-only.
        assert_eq!(
            flags & CRIMSON_ITEMINFO_FLAG_IS_HOUSING_ONLY,
            0,
            "Marni helm should not be housing-only",
        );

        // ── Summary one-shot lookup ────────────────────────────────
        let mut summary = CrimsonItemInfoSummary {
            max_stack_count: 0,
            cooltime: 0,
            respawn_time_seconds: 0,
            key: 0,
            flags: 0,
            icon_path_hash: 0,
            equip_type_info: 0,
            equipable_hash: 0,
            equipable_level: 0,
            knowledge_info: 0,
            material_key: 0,
            gimmick_info: 0,
            category_info: 0,
            inventory_info: 0,
            minimum_extract_enchant_level: 0,
            max_endurance: 0,
            item_type: 0,
            item_tier: 0,
            quick_slot_index: 0,
            item_charge_type: 0,
            usable_alert_type: 0,
            knowledge_obtain_type: 0,
            apply_drop_stat_type: 0,
            _reserved: 0,
        };
        assert_eq!(
            unsafe { crimson_iteminfo_lookup_summary(handle, 14510, &mut summary) },
            error::OK
        );
        assert_eq!(summary.key, 14510);
        assert_eq!(summary.item_type, 24);
        assert_eq!(summary.flags, flags, "summary.flags must match per-key flag lookup");
        // Summary's max_stack_count should agree with the dedicated
        // max-stack lookup — different code paths reading the same
        // cache.
        let mut max_stack: u64 = 0;
        assert_eq!(
            unsafe { crimson_iteminfo_lookup_max_stack(handle, 14510, &mut max_stack) },
            error::OK
        );
        assert_eq!(summary.max_stack_count, max_stack);

        // ── Cross-check: count of items with IS_EQUIP_QUICK_SLOT_VISIBLE.
        // Was 949 on 1.08/1.09; 1.10 adds one more equip item → 950.
        // A regression here flags either a parser bug or a game-content
        // drift large enough to need re-validation.
        let mut count = 0u32;
        assert_eq!(
            unsafe { crimson_iteminfo_entry_count(handle, &mut count) },
            error::OK
        );
        let mut visible = 0u32;
        for i in 0..count {
            let mut k: u32 = 0;
            let mut required: usize = 0;
            let _ = unsafe {
                crimson_iteminfo_get_entry(handle, i, &mut k, ptr::null_mut(), 0, &mut required)
            };
            let mut f: u32 = 0;
            if unsafe { crimson_iteminfo_lookup_flags(handle, k, &mut f) } == error::OK
                && f & CRIMSON_ITEMINFO_FLAG_IS_EQUIP_QUICK_SLOT_VISIBLE != 0
            {
                visible += 1;
            }
        }
        assert_eq!(
            visible, 1005,
            "IS_EQUIP_QUICK_SLOT_VISIBLE count drifted (1.13 baseline 1005; 970 on 1.12, 950 on 1.10/1.11, 949 on 1.08/1.09)",
        );

        // ── Negative path: unknown key → NOT_FOUND ─────────────────
        assert_eq!(
            unsafe { crimson_iteminfo_lookup_flags(handle, u32::MAX, &mut flags) },
            error::NOT_FOUND
        );
        assert_eq!(
            unsafe { crimson_iteminfo_lookup_summary(handle, u32::MAX, &mut summary) },
            error::NOT_FOUND
        );

        unsafe { crimson_iteminfo_free(handle) };
    }

    /// NULL_ARG coverage for the new static-metadata getters.
    #[test]
    fn c_abi_iteminfo_static_lookups_null_args() {
        // Pass null handle on each getter and verify NULL_ARG comes back
        // before any out-pointer touch.
        let mut u8_out: u8 = 0;
        let mut u16_out: u16 = 0;
        let mut u32_out: u32 = 0;
        let mut i64_out: i64 = 0;
        let mut summary = unsafe {
            // Default-zeroed; safe since the function returns before reading.
            std::mem::zeroed::<CrimsonItemInfoSummary>()
        };
        assert_eq!(
            unsafe { crimson_iteminfo_lookup_flags(ptr::null(), 0, &mut u32_out) },
            error::NULL_ARG,
        );
        assert_eq!(
            unsafe { crimson_iteminfo_lookup_item_type(ptr::null(), 0, &mut u8_out) },
            error::NULL_ARG,
        );
        assert_eq!(
            unsafe { crimson_iteminfo_lookup_category_info(ptr::null(), 0, &mut u16_out) },
            error::NULL_ARG,
        );
        assert_eq!(
            unsafe { crimson_iteminfo_lookup_cooltime(ptr::null(), 0, &mut i64_out) },
            error::NULL_ARG,
        );
        assert_eq!(
            unsafe { crimson_iteminfo_lookup_summary(ptr::null(), 0, &mut summary) },
            error::NULL_ARG,
        );
    }
}
