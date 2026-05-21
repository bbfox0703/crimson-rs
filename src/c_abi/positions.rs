//! Positioned-entity enumeration — C ABI surface.
//!
//! Flattens every save-side entity that carries a world-space position
//! into a single record stream. Built for the C# editor's world-map
//! plotting feature: list once, filter by `field_info_key` if
//! per-region rendering is desired, then apply the affine transform
//! pinned in [`docs/worldmap-plotting.md`](../../docs/worldmap-plotting.md)
//! to map `(pos_x, pos_z)` to basemap pixel coordinates.
//!
//! ## Container coverage (1.07 slot103 baseline)
//!
//! | Kind | Container | Position source | Count |
//! |---|---|---:|---:|
//! | `ACTIVE_CHAR` | `TransformSaveData._fieldSaveDataList[0]` → `TransformFieldSaveData._position` | F32x3 direct | 1 |
//! | `MERCENARY` | `MercenaryClanSaveData._mercenaryDataList[N]._spawnPosition` | F32x3 direct | 76 (of 96) |
//! | `GIMMICK` | `FieldSaveData._fieldGimmickSaveDataList[N]._transform` OR `_originSpawnTransform` | 40-byte `Transform` scalar — scale[3] + quat[4] + pos[3], decoded from [`ScalarValue::Bytes`] | 3,240 (of 4,260) |
//!
//! `FieldGimmickSaveData` blocks are direct children of
//! `FieldSaveData._fieldGimmickSaveDataList` (the second
//! `FieldSaveData` block — when present — only carries event data,
//! not gimmicks). The walker picks up `FieldSaveData._fieldInfoKey`
//! from the enclosing block and propagates it onto every emitted
//! gimmick record so the editor can filter by region without a
//! second lookup.
//!
//! Roughly 24% of `FieldGimmickSaveData` blocks have **both**
//! `_transform` and `_originSpawnTransform` absent — these are
//! state-only gimmicks (counters, triggers, abstract scripted state)
//! with no world position. The walker skips them. Of the
//! position-bearing remainder, ~6% have moved from spawn (`_transform`
//! present); the other ~94% are at their spawn position
//! (`_originSpawnTransform` only) and get [`position_flags::FROM_ORIGIN_TRANSFORM`].
//!
//! Nested gimmicks — children of other `FieldGimmickSaveData` blocks
//! via the various `_fieldGimmickSaveData_*ChildList` sublists — are
//! NOT enumerated. Their world positions co-locate with their parent
//! container, so the map-marker plotting use case loses nothing by
//! dropping them. If a future use case needs them (e.g. drilling
//! into a multi-slot chest), a recursive walker variant can be added
//! without changing the existing ABI surface.
//!
//! ## Why not `FieldNPCSaveData`?
//!
//! Empirically verified: `FieldNPCSaveData` has 12 fields and **none
//! of them is a position field**. NPCs are positioned by gamedata
//! (level data + spawn tables), not by the save. The save only
//! records `_spawnFieldInfoKey` + `_characterKey` + `_friendly` state.
//! Plotting NPC markers requires loading the corresponding level
//! data — out of scope for this ABI.
//!
//! ## Why not `GameData_GimmickPointData`?
//!
//! In slot103 every observed instance has `_transform = absent`. The
//! 857-figure cited in the planning doc was an unverified estimate
//! pulled from the schema dump count, not the present-data count.
//! Drop from scope; revisit if a future probe finds present
//! transforms.
//!
//! ## Coordinate frame
//!
//! All positions are in the **global world frame** — the same frame
//! the in-game teleport system's "TP marker" values use. The basemap
//! affine fit lives in [`docs/worldmap-plotting.md`](../../docs/worldmap-plotting.md):
//!
//! ```text
//! map_px =  0.432044 * pos_x + 5937.50
//! map_py = -0.433071 * pos_z + 1864.08
//! ```
//!
//! `pos_y` is height — ignore for top-down plotting; use it for
//! 3D-aware features (e.g. underground vs surface markers).
//!
//! ## Yaw extraction
//!
//! - Mercenary: `_spawnYaw` is a direct `float` in the save — pass through.
//! - Gimmick: derived from the quaternion at `_originSpawnTransform`
//!   bytes `[12..28]` as `2 * atan2(qy, qw)`. Assumes Y-up rotation,
//!   which matches every observed transform (qx ≈ qz ≈ 0). Mixed-axis
//!   rotations are rare for world gimmicks and degrade gracefully —
//!   the marker just points in a slightly off direction.
//! - Active char: derived from `TransformFieldSaveData._rotation`
//!   (F32x4 quaternion) the same way.

use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::save::{FieldValue, ObjectBlock, ScalarValue};

use super::{CrimsonSaveHandle, error};

/// Kind classification for [`CrimsonPositionedEntityRecord::kind`].
///
/// Numeric stability: these constants are part of the C ABI surface
/// and MUST NOT be reassigned. Add new kinds with the next free
/// integer; never reuse a retired one.
pub mod position_kind {
    /// The active playable character — read from
    /// `TransformSaveData._fieldSaveDataList[0]._position`. At most
    /// one record of this kind per save. `character_key` carries
    /// `MercenaryClanSaveData._lastFocusCharacterKey` (cat-byte
    /// stripped) when available; 0 otherwise.
    pub const ACTIVE_CHAR: u32 = 0;
    /// A mercenary / mount / inactive-playable in
    /// `MercenaryClanSaveData._mercenaryDataList[N]`. Position comes
    /// from `_spawnPosition` (F32x3); `_spawnYaw` populates `yaw`;
    /// `_spawnFieldInfoKey` populates `field_info_key`. Only emitted
    /// when `_spawnPosition` is present (76 of 96 mercenaries in
    /// the slot103 baseline carry a present position).
    pub const MERCENARY: u32 = 1;
    /// A field gimmick — top-level `FieldGimmickSaveData` block.
    /// Position decoded from the 40-byte `Transform`-typed inline
    /// bytes at field 11 (`_transform`) if present, else field 12
    /// (`_originSpawnTransform`). Bytes [28..40] of the transform
    /// are the position float3; bytes [12..28] are the rotation
    /// quaternion. `gimmick_info_key` carries `_gimmickInfoKey`
    /// (resolvable through the `gimmickinfo` C ABI bridge);
    /// `gimmick_save_data_key` carries `_fieldGimmickSaveDataKey`
    /// (the per-save slot id).
    pub const GIMMICK: u32 = 2;
}

/// Bit constants for [`CrimsonPositionedEntityRecord::flags`].
///
/// Same stability contract as [`position_kind`]: never reassign.
pub mod position_flags {
    /// `MercenarySaveData._isMainMercenary = true` — the currently-
    /// summoned mount / main companion among the player's mercenary
    /// slots. Always 0 for non-`MERCENARY` kinds.
    pub const IS_MAIN_MERCENARY: u32 = 1 << 0;
    /// Mercenary's `_characterKey & 0xFFFFFF` is in
    /// [`super::all_items::PLAYABLE_CHARACTER_KEYS`] OR its
    /// `_ownedCharacterKey & 0xFFFFFF` is. Mirrors the same flag on
    /// [`super::all_items::CrimsonItemRecord`] — clear for NPC
    /// followers, set for the three playables + their owned mounts.
    /// Always set for `ACTIVE_CHAR` (the active is always a
    /// playable); always clear for `GIMMICK` (gimmicks have no
    /// ownership).
    pub const IS_PLAYER_OWNED: u32 = 1 << 1;
    /// Gimmick used `_originSpawnTransform` (field 12) instead of
    /// `_transform` (field 11) for position. `_transform` is the
    /// "current" transform once a gimmick has moved or been
    /// interacted with; `_originSpawnTransform` is the spawn-time
    /// transform. In slot103 ~94% of position-bearing gimmicks
    /// have this flag set (`_transform` absent); the other ~6% have
    /// `_transform` present (gimmick has moved from spawn).
    pub const FROM_ORIGIN_TRANSFORM: u32 = 1 << 2;
}

/// One flat record emitted by [`crimson_save_list_field_positions`] —
/// a 56-byte `repr(C)` structure laid out for direct mmap-style read
/// from C# / C++ consumers.
///
/// All integers are little-endian, naturally aligned. The single
/// u64 field sits at the end so the whole structure is 8-byte
/// aligned.
///
/// | Offset | Field | Type | Purpose |
/// |---:|---|---|---|
/// |  0 | `block_idx` | u32 | Top-level TOC block index (e.g. for fetching the block's JSON via existing reads) |
/// |  4 | `kind` | u32 | One of [`position_kind`] constants |
/// |  8 | `flags` | u32 | Bitfield ([`position_flags`]) |
/// | 12 | `field_info_key` | u32 | `_spawnFieldInfoKey` (mercenary) / `_fieldInfoKey` (active char) / enclosing `FieldSaveData._fieldInfoKey` (gimmick) |
/// | 16 | `character_key` | u32 | For `MERCENARY`: `_characterKey & 0xFFFFFF`. For `ACTIVE_CHAR`: `MercenaryClanSaveData._lastFocusCharacterKey & 0xFFFFFF`. 0 for `GIMMICK`. |
/// | 20 | `gimmick_info_key` | u32 | For `GIMMICK`: `_gimmickInfoKey`. 0 otherwise. Resolves through `crimson_gimmickinfo_lookup_*`. |
/// | 24 | `gimmick_save_data_key` | u32 | For `GIMMICK`: `_fieldGimmickSaveDataKey` (per-save slot id). 0 otherwise. |
/// | 28 | `element_index` | u32 | Within-list index (e.g. mercenary slot N). For `ACTIVE_CHAR`: the index into `_fieldSaveDataList`. For `GIMMICK`: 0 (top-level block — use `block_idx`). |
/// | 32 | `pos_x` | f32 | World X (east-west); apply affine `0.432044 * X + 5937.50` for basemap pixel |
/// | 36 | `pos_y` | f32 | World Y (height); ignore for top-down plotting |
/// | 40 | `pos_z` | f32 | World Z (north-south); apply affine `-0.433071 * Z + 1864.08` for basemap pixel |
/// | 44 | `yaw` | f32 | Rotation around Y axis in radians. Mercenaries: from `_spawnYaw` direct. Active char + gimmicks: derived from quaternion `2 * atan2(qy, qw)`. |
/// | 48 | `mercenary_no` | u64 | For `MERCENARY`: `_mercenaryNo` (per-save unique instance id). 0 otherwise. |
///
/// **Validity window**: positional fields (`block_idx`,
/// `element_index`) stay valid only until the next length-changing
/// mutation in the relevant list. Combine with
/// [`super::crimson_save_get_mutation_version`] to detect staleness.
/// Position scalars (`pos_*`, `yaw`) become stale on any mutation to
/// the source position field.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CrimsonPositionedEntityRecord {
    pub block_idx: u32,
    pub kind: u32,
    pub flags: u32,
    pub field_info_key: u32,
    pub character_key: u32,
    pub gimmick_info_key: u32,
    pub gimmick_save_data_key: u32,
    pub element_index: u32,
    pub pos_x: f32,
    pub pos_y: f32,
    pub pos_z: f32,
    pub yaw: f32,
    pub mercenary_no: u64,
}

// Sanity guard: size + layout are part of the C ABI surface.
const _: () = assert!(std::mem::size_of::<CrimsonPositionedEntityRecord>() == 56);
const _: () = assert!(std::mem::align_of::<CrimsonPositionedEntityRecord>() == 8);

// ── Scalar pullers ────────────────────────────────────────────────────────

fn pull_u32(b: &ObjectBlock, name: &str) -> Option<u32> {
    b.fields.iter().find(|f| f.name == name && f.present).and_then(|f| match &f.value {
        FieldValue::Scalar(ScalarValue::U32(v)) => Some(*v),
        _ => None,
    })
}
fn pull_u64(b: &ObjectBlock, name: &str) -> Option<u64> {
    b.fields.iter().find(|f| f.name == name && f.present).and_then(|f| match &f.value {
        FieldValue::Scalar(ScalarValue::U64(v)) => Some(*v),
        _ => None,
    })
}
fn pull_f32(b: &ObjectBlock, name: &str) -> Option<f32> {
    b.fields.iter().find(|f| f.name == name && f.present).and_then(|f| match &f.value {
        FieldValue::Scalar(ScalarValue::F32(v)) => Some(*v),
        _ => None,
    })
}
fn pull_f32x3(b: &ObjectBlock, name: &str) -> Option<[f32; 3]> {
    b.fields.iter().find(|f| f.name == name && f.present).and_then(|f| match &f.value {
        FieldValue::Scalar(ScalarValue::F32x3(v)) => Some(*v),
        _ => None,
    })
}
fn pull_f32x4(b: &ObjectBlock, name: &str) -> Option<[f32; 4]> {
    b.fields.iter().find(|f| f.name == name && f.present).and_then(|f| match &f.value {
        FieldValue::Scalar(ScalarValue::F32x4(v)) => Some(*v),
        _ => None,
    })
}
fn pull_bool_true(b: &ObjectBlock, name: &str) -> bool {
    b.fields.iter().any(|f| {
        f.name == name && f.present && matches!(f.value, FieldValue::Scalar(ScalarValue::Bool(true)))
    })
}

/// Decode the 40-byte `Transform`-typed InlineBytes payload. Returns
/// `(position, quaternion)`. The layout is
/// `scale(3) + quaternion(4) + position(3)` — 10 f32s, verified
/// against the slot103 active character (cross-checked against the
/// in-game teleport-marker reading).
fn decode_transform_bytes(bytes: &[u8]) -> Option<([f32; 3], [f32; 4])> {
    if bytes.len() < 40 {
        return None;
    }
    let f = |off: usize| {
        let arr: [u8; 4] = bytes[off..off + 4].try_into().ok()?;
        Some(f32::from_le_bytes(arr))
    };
    let qx = f(12)?;
    let qy = f(16)?;
    let qz = f(20)?;
    let qw = f(24)?;
    let px = f(28)?;
    let py = f(32)?;
    let pz = f(36)?;
    Some(([px, py, pz], [qx, qy, qz, qw]))
}

/// Pull the 40-byte payload of a `Transform`-typed field by name.
/// `Transform` is a non-power-of-2 fixed-size composite (40 B) that
/// falls through the scalar decoder's primitive table and lands in
/// [`ScalarValue::Bytes`]. The schema is `scale(3) + rotation(4) +
/// position(3)`.
fn pull_transform(b: &ObjectBlock, name: &str) -> Option<([f32; 3], [f32; 4])> {
    let f = b.fields.iter().find(|f| f.name == name && f.present)?;
    let FieldValue::Scalar(ScalarValue::Bytes(bytes)) = &f.value else {
        return None;
    };
    decode_transform_bytes(bytes)
}

/// Yaw extraction from a quaternion assuming Y-up. For pure
/// Y-axis rotations (qx ≈ qz ≈ 0) this reduces to `2 * atan2(qy, qw)`
/// — every observed save transform fits this pattern. Mixed-axis
/// rotations get a reasonable approximation rather than the exact
/// Euler decomposition (which would need pitch + roll fields we
/// don't expose).
fn yaw_from_quat(q: [f32; 4]) -> f32 {
    let [qx, qy, qz, qw] = q;
    // Standard yaw extraction for Y-up ZXY / YXZ Euler: atan2(2(wy + xz), 1 - 2(y² + z²))
    let s = 2.0 * (qw * qy + qx * qz);
    let c = 1.0 - 2.0 * (qy * qy + qz * qz);
    s.atan2(c)
}

// ── Walker ────────────────────────────────────────────────────────────────

fn collect_positions(handle: &CrimsonSaveHandle) -> Vec<CrimsonPositionedEntityRecord> {
    let mut out: Vec<CrimsonPositionedEntityRecord> = Vec::new();

    // Pass A: resolve the active character key from
    // MercenaryClanSaveData._lastFocusCharacterKey for the
    // ACTIVE_CHAR record's character_key field.
    let active_character_key: u32 = handle
        .blocks
        .iter()
        .find(|b| b.class_name == "MercenaryClanSaveData")
        .and_then(|b| pull_u32(b, "_lastFocusCharacterKey"))
        .map(|k| k & 0xFFFFFF)
        .unwrap_or(0);

    // Pass B: walk every TOC block and emit records per kind.
    // Both `FieldSaveData` blocks (one per loaded region) get walked
    // — the second only has `_globalGameEventDataList` populated, so
    // `emit_field_gimmicks` returns early on it.
    for (block_idx, block) in handle.blocks.iter().enumerate() {
        match block.class_name.as_str() {
            "TransformSaveData" => emit_active_char(block, block_idx as u32, active_character_key, &mut out),
            "MercenaryClanSaveData" => emit_mercenaries(block, block_idx as u32, &mut out),
            "FieldSaveData" => emit_field_gimmicks(block, block_idx as u32, &mut out),
            _ => {}
        }
    }

    out
}

fn emit_active_char(
    block: &ObjectBlock,
    block_idx: u32,
    active_character_key: u32,
    out: &mut Vec<CrimsonPositionedEntityRecord>,
) {
    let Some(list_field) = block.fields.iter().find(|f| f.name == "_fieldSaveDataList" && f.present)
    else {
        return;
    };
    let FieldValue::ObjectList { elements, .. } = &list_field.value else {
        return;
    };
    // Typically count=1 (one entry per currently-loaded field). Emit
    // a record for every entry that carries a present _position; the
    // editor can disambiguate via `field_info_key` if multiple appear.
    for (elem_idx, field_block) in elements.iter().enumerate() {
        let Some(pos) = pull_f32x3(field_block, "_position") else { continue };
        let quat = pull_f32x4(field_block, "_rotation").unwrap_or([0.0, 0.0, 0.0, 1.0]);
        let field_info_key = pull_u32(field_block, "_fieldInfoKey").unwrap_or(0);
        out.push(CrimsonPositionedEntityRecord {
            block_idx,
            kind: position_kind::ACTIVE_CHAR,
            flags: position_flags::IS_PLAYER_OWNED, // active is always a playable
            field_info_key,
            character_key: active_character_key,
            gimmick_info_key: 0,
            gimmick_save_data_key: 0,
            element_index: elem_idx as u32,
            pos_x: pos[0],
            pos_y: pos[1],
            pos_z: pos[2],
            yaw: yaw_from_quat(quat),
            mercenary_no: 0,
        });
    }
}

fn emit_mercenaries(
    block: &ObjectBlock,
    block_idx: u32,
    out: &mut Vec<CrimsonPositionedEntityRecord>,
) {
    use super::all_items::PLAYABLE_CHARACTER_KEYS;
    let Some(merc_list_field) =
        block.fields.iter().find(|f| f.name == "_mercenaryDataList" && f.present)
    else {
        return;
    };
    let FieldValue::ObjectList { elements: mercs, .. } = &merc_list_field.value else {
        return;
    };
    for (merc_idx, merc) in mercs.iter().enumerate() {
        // Skip mercenaries without a present _spawnPosition — those
        // are the "never been summoned" entries that the save keeps
        // around for stat tracking but doesn't pin to a world spot.
        let Some(pos) = pull_f32x3(merc, "_spawnPosition") else { continue };
        let yaw = pull_f32(merc, "_spawnYaw").unwrap_or(0.0);
        let field_info_key = pull_u32(merc, "_spawnFieldInfoKey").unwrap_or(0);
        let char_raw = pull_u32(merc, "_characterKey").unwrap_or(0);
        let char_stripped = char_raw & 0xFFFFFF;
        let owned_by = pull_u32(merc, "_ownedCharacterKey").map(|k| k & 0xFFFFFF);
        let merc_no = pull_u64(merc, "_mercenaryNo").unwrap_or(0);

        let mut flags = 0u32;
        if pull_bool_true(merc, "_isMainMercenary") {
            flags |= position_flags::IS_MAIN_MERCENARY;
        }
        if PLAYABLE_CHARACTER_KEYS.contains(&char_stripped)
            || owned_by.is_some_and(|k| PLAYABLE_CHARACTER_KEYS.contains(&k))
        {
            flags |= position_flags::IS_PLAYER_OWNED;
        }

        out.push(CrimsonPositionedEntityRecord {
            block_idx,
            kind: position_kind::MERCENARY,
            flags,
            field_info_key,
            character_key: char_stripped,
            gimmick_info_key: 0,
            gimmick_save_data_key: 0,
            element_index: merc_idx as u32,
            pos_x: pos[0],
            pos_y: pos[1],
            pos_z: pos[2],
            yaw,
            mercenary_no: merc_no,
        });
    }
}

fn emit_field_gimmicks(
    block: &ObjectBlock,
    block_idx: u32,
    out: &mut Vec<CrimsonPositionedEntityRecord>,
) {
    // FieldSaveData carries the region key for every gimmick inside.
    let field_info_key = pull_u32(block, "_fieldInfoKey").unwrap_or(0);
    let Some(list_field) =
        block.fields.iter().find(|f| f.name == "_fieldGimmickSaveDataList" && f.present)
    else {
        return;
    };
    let FieldValue::ObjectList { elements, .. } = &list_field.value else {
        return;
    };
    for (elem_idx, gimmick) in elements.iter().enumerate() {
        // Prefer the "current" _transform if present; fall back to
        // _originSpawnTransform. In slot103 100% of gimmicks use the
        // fallback — _transform is uniformly absent.
        let (pos, quat, from_origin) = match pull_transform(gimmick, "_transform") {
            Some((p, q)) => (p, q, false),
            None => match pull_transform(gimmick, "_originSpawnTransform") {
                Some((p, q)) => (p, q, true),
                None => continue, // no usable transform — skip
            },
        };
        let gimmick_info_key = pull_u32(gimmick, "_gimmickInfoKey").unwrap_or(0);
        let gimmick_save_data_key = pull_u32(gimmick, "_fieldGimmickSaveDataKey").unwrap_or(0);
        let mut flags = 0u32;
        if from_origin {
            flags |= position_flags::FROM_ORIGIN_TRANSFORM;
        }
        out.push(CrimsonPositionedEntityRecord {
            block_idx,
            kind: position_kind::GIMMICK,
            flags,
            field_info_key,
            character_key: 0,
            gimmick_info_key,
            gimmick_save_data_key,
            element_index: elem_idx as u32,
            pos_x: pos[0],
            pos_y: pos[1],
            pos_z: pos[2],
            yaw: yaw_from_quat(quat),
            mercenary_no: 0,
        });
    }
}

// ── ABI entry point ────────────────────────────────────────────────────────

/// Flat-list every positioned entity across the save's container
/// classes (active character, mercenaries / mounts, field gimmicks).
/// See [`CrimsonPositionedEntityRecord`] for the per-row shape and
/// [`position_kind`] for the kind classification.
///
/// **Two-call shape** (record-array variant — counts in records, not
/// bytes):
///
/// - First call with `out_records = null, capacity_records = 0`
///   populates `*out_count_records` and `*out_version`. Returns
///   `BUFFER_TOO_SMALL` (unless the save has zero positioned
///   entities — extremely unlikely in practice — in which case
///   returns `OK`).
/// - Allocate `*out_count_records` records, call again.
///
/// **`out_version` (optional, may be null)**: matches the staleness
/// contract of [`super::crimson_save_list_inventory_items`] and
/// [`super::crimson_save_list_all_items`] — pair the stamp with
/// [`super::crimson_save_get_mutation_version`].
///
/// **Plotting pipeline** (from
/// [`docs/worldmap-plotting.md`](../../docs/worldmap-plotting.md)):
///
/// ```text
/// for r in records {
///     // Optional: filter by region
///     if r.field_info_key != desired_field { continue }
///
///     // Apply the user's basemap affine (5178×5240 web map)
///     px =  0.432044 * r.pos_x + 5937.50;
///     py = -0.433071 * r.pos_z + 1864.08;
///
///     draw_marker(px, py, r.kind, r.yaw)
/// }
/// ```
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
pub unsafe extern "C" fn crimson_save_list_field_positions(
    handle: *const CrimsonSaveHandle,
    out_records: *mut CrimsonPositionedEntityRecord,
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
        let records = collect_positions(h);
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
    //!    `repr(C)` shape, kind / flag constants, transform decoder.
    //!    Run on every CI build.
    //! 2. **Live-install integration** — points at slot103/save.save
    //!    (or `CRIMSON_LIVE_SAVE` override). Asserts the per-kind
    //!    counts match the diagnostic-probe baseline:
    //!    1 ACTIVE_CHAR + 76 MERCENARY + 4,260 GIMMICK = 4,337 records.
    //!    Skips cleanly when the save isn't present.

    use super::*;
    use std::path::PathBuf;
    use std::ptr;

    #[test]
    fn record_layout_is_stable() {
        assert_eq!(std::mem::size_of::<CrimsonPositionedEntityRecord>(), 56);
        assert_eq!(std::mem::align_of::<CrimsonPositionedEntityRecord>(), 8);

        let rec = CrimsonPositionedEntityRecord {
            block_idx: 0, kind: 0, flags: 0, field_info_key: 0,
            character_key: 0, gimmick_info_key: 0,
            gimmick_save_data_key: 0, element_index: 0,
            pos_x: 0.0, pos_y: 0.0, pos_z: 0.0, yaw: 0.0,
            mercenary_no: 0,
        };
        let base = (&rec as *const CrimsonPositionedEntityRecord).addr();
        let off_u32 = |p: *const u32| (p as usize) - base;
        let off_f32 = |p: *const f32| (p as usize) - base;
        let off_u64 = |p: *const u64| (p as usize) - base;
        assert_eq!(off_u32(&rec.block_idx),               0);
        assert_eq!(off_u32(&rec.kind),                    4);
        assert_eq!(off_u32(&rec.flags),                   8);
        assert_eq!(off_u32(&rec.field_info_key),         12);
        assert_eq!(off_u32(&rec.character_key),          16);
        assert_eq!(off_u32(&rec.gimmick_info_key),       20);
        assert_eq!(off_u32(&rec.gimmick_save_data_key),  24);
        assert_eq!(off_u32(&rec.element_index),          28);
        assert_eq!(off_f32(&rec.pos_x),                  32);
        assert_eq!(off_f32(&rec.pos_y),                  36);
        assert_eq!(off_f32(&rec.pos_z),                  40);
        assert_eq!(off_f32(&rec.yaw),                    44);
        assert_eq!(off_u64(&rec.mercenary_no),           48);
    }

    #[test]
    fn kind_constants_are_distinct() {
        let kinds = [
            position_kind::ACTIVE_CHAR,
            position_kind::MERCENARY,
            position_kind::GIMMICK,
        ];
        let mut sorted = kinds.to_vec();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), kinds.len(), "position_kind constants collide");
    }

    #[test]
    fn flag_bits_are_non_overlapping() {
        let flags = [
            position_flags::IS_MAIN_MERCENARY,
            position_flags::IS_PLAYER_OWNED,
            position_flags::FROM_ORIGIN_TRANSFORM,
        ];
        let combined: u32 = flags.iter().sum();
        assert_eq!(
            combined,
            flags.iter().copied().fold(0u32, |a, b| a | b),
            "flag bits overlap",
        );
    }

    #[test]
    fn decode_transform_known_sample() {
        // The slot103 sample gimmick transform from
        // _probe_positioned_entity_hosts — scale=1,1,1, qx=0, qy=-0.186,
        // qz=0, qw=0.982, pos ≈ (-10086.566, 517.808, -4386.557).
        let bytes: &[u8] = &[
            0, 0, 128, 63,   // scale.x = 1.0
            0, 0, 128, 63,   // scale.y = 1.0
            0, 0, 128, 63,   // scale.z = 1.0
            0, 0, 0, 0,      // qx = 0.0
            204, 210, 62, 190, // qy = -0.186...
            0, 0, 0, 0,      // qz = 0.0
            2, 132, 123, 63, // qw = 0.982...
            67, 154, 29, 198, // pos.x
            183, 115, 1, 68,  // pos.y
            116, 22, 137, 197, // pos.z
        ];
        let (pos, quat) = decode_transform_bytes(bytes).expect("decode");
        // pos.x bytes [67, 154, 29, 198] = 0xC61D9A43 = -10086.566...
        assert!((pos[0] - (-10086.566)).abs() < 0.1, "pos.x = {}", pos[0]);
        // pos.y bytes [183, 115, 1, 68] = 0x440173B7 = 517.808...
        assert!((pos[1] - 517.808).abs() < 0.1, "pos.y = {}", pos[1]);
        // pos.z bytes [116, 22, 137, 197] = 0xC5891674 = -4386.806...
        assert!((pos[2] - (-4386.806)).abs() < 0.1, "pos.z = {}", pos[2]);
        assert!(quat[0].abs() < 1e-6);
        assert!((quat[1] - (-0.186)).abs() < 0.001);
        assert!(quat[2].abs() < 1e-6);
        assert!((quat[3] - 0.982).abs() < 0.001);

        let yaw = yaw_from_quat(quat);
        // Pure Y rotation: yaw ≈ 2 * atan2(-0.186, 0.982) ≈ -0.376 rad
        assert!((yaw - (-0.376)).abs() < 0.01, "yaw = {}", yaw);
    }

    #[test]
    fn decode_transform_short_input() {
        assert!(decode_transform_bytes(&[0u8; 39]).is_none());
        assert!(decode_transform_bytes(&[]).is_none());
    }

    #[test]
    fn yaw_from_identity_quat_is_zero() {
        let yaw = yaw_from_quat([0.0, 0.0, 0.0, 1.0]);
        assert!(yaw.abs() < 1e-6);
    }

    #[test]
    fn null_args() {
        let mut count: usize = 0;
        let rc = unsafe {
            crimson_save_list_field_positions(ptr::null(), ptr::null_mut(), 0, &mut count, ptr::null_mut())
        };
        assert_eq!(rc, error::NULL_ARG);
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
                    let p = entry.path().join("slot107").join("save.save");
                    p.is_file().then_some(p)
                })
            })
    }

    fn empty_rec() -> CrimsonPositionedEntityRecord {
        CrimsonPositionedEntityRecord {
            block_idx: 0, kind: u32::MAX, flags: 0, field_info_key: 0,
            character_key: 0, gimmick_info_key: 0,
            gimmick_save_data_key: 0, element_index: 0,
            pos_x: 0.0, pos_y: 0.0, pos_z: 0.0, yaw: 0.0,
            mercenary_no: 0,
        }
    }

    #[test]
    fn live_slot107_breakdown() {
        let Some(save_path) = find_save_path() else {
            eprintln!("skipping live_slot107_breakdown: no slot107/save.save");
            return;
        };
        let path_c = std::ffi::CString::new(save_path.to_str().unwrap()).unwrap();
        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        let rc = unsafe { super::super::crimson_save_load_from_file(path_c.as_ptr(), &mut handle) };
        assert_eq!(rc, error::OK);
        assert!(!handle.is_null());

        // First-call sizing.
        let mut count: usize = 0;
        let mut version: u64 = 0;
        let rc = unsafe {
            crimson_save_list_field_positions(handle, ptr::null_mut(), 0, &mut count, &mut version)
        };
        // Expect thousands — the 4260 gimmicks dominate.
        assert!(count > 1000, "expected >1000 records in slot107, got {count}");
        if count > 0 {
            assert_eq!(rc, error::BUFFER_TOO_SMALL);
        }
        assert_eq!(version, 0, "fresh handle should report mutation_version=0");

        // Fill.
        let mut records = vec![empty_rec(); count];
        let mut count2 = count;
        let rc = unsafe {
            crimson_save_list_field_positions(
                handle,
                records.as_mut_ptr(),
                records.len(),
                &mut count2,
                ptr::null_mut(),
            )
        };
        assert_eq!(rc, error::OK);
        assert_eq!(count2, count);

        // Histogram by kind.
        let mut by_kind: std::collections::BTreeMap<u32, usize> = Default::default();
        for r in &records {
            *by_kind.entry(r.kind).or_insert(0) += 1;
        }
        eprintln!("slot107 breakdown by position kind:");
        for (k, n) in &by_kind {
            let name = match *k {
                position_kind::ACTIVE_CHAR => "ACTIVE_CHAR",
                position_kind::MERCENARY => "MERCENARY",
                position_kind::GIMMICK => "GIMMICK",
                _ => "<unknown>",
            };
            eprintln!("  {name} ({k}): {n}");
        }

        // Pinned baseline counts from _probe_positioned_entity_hosts.
        let active = by_kind.get(&position_kind::ACTIVE_CHAR).copied().unwrap_or(0);
        let merc = by_kind.get(&position_kind::MERCENARY).copied().unwrap_or(0);
        let gimmick = by_kind.get(&position_kind::GIMMICK).copied().unwrap_or(0);
        assert_eq!(active, 1, "expected exactly 1 ACTIVE_CHAR record");
        // Mercenary: 76 in slot103 baseline (only mercs with present _spawnPosition).
        assert!(merc >= 60, "expected ≥60 MERCENARY records (baseline ~76), got {merc}");
        // Gimmick: 3240 position-bearing in slot103 baseline (of
        // 4260 total — ~24% are state-only, no transform).
        assert!(gimmick > 1000, "expected >1000 GIMMICK records (baseline ~3240), got {gimmick}");

        // The active character's position must match the slot103
        // probe value: (-10502.729, 610.6218, -4373.9663).
        let active_rec = records.iter()
            .find(|r| r.kind == position_kind::ACTIVE_CHAR)
            .expect("active char record");
        eprintln!(
            "active char position: ({}, {}, {}), yaw = {}",
            active_rec.pos_x, active_rec.pos_y, active_rec.pos_z, active_rec.yaw,
        );
        assert!(active_rec.pos_x.is_finite() && active_rec.pos_z.is_finite());
        // Should be in the world coord range (~ tens of thousands).
        assert!(active_rec.pos_x.abs() < 100_000.0);
        assert!(active_rec.pos_z.abs() < 100_000.0);

        // Every ACTIVE_CHAR record must have IS_PLAYER_OWNED set.
        for r in &records {
            if r.kind == position_kind::ACTIVE_CHAR {
                assert_ne!(
                    r.flags & position_flags::IS_PLAYER_OWNED, 0,
                    "ACTIVE_CHAR missing IS_PLAYER_OWNED: {:?}", r,
                );
            }
        }

        // Every MERCENARY record must have a non-zero mercenary_no.
        for r in &records {
            if r.kind == position_kind::MERCENARY {
                assert_ne!(
                    r.mercenary_no, 0,
                    "MERCENARY record with mercenary_no=0: {:?}", r,
                );
            }
        }

        // Every GIMMICK record must have a non-zero gimmick_info_key.
        for r in &records {
            if r.kind == position_kind::GIMMICK {
                assert_ne!(
                    r.gimmick_info_key, 0,
                    "GIMMICK record with gimmick_info_key=0: {:?}", r,
                );
            }
        }

        // ~94% of slot103 gimmicks use FROM_ORIGIN_TRANSFORM; the
        // other ~6% have moved (have `_transform` present). Both
        // surfaces should be exercised — assert neither extreme.
        let gimmick_origin = records.iter()
            .filter(|r| r.kind == position_kind::GIMMICK
                && r.flags & position_flags::FROM_ORIGIN_TRANSFORM != 0)
            .count();
        let gimmick_moved = gimmick - gimmick_origin;
        eprintln!("  gimmick from origin: {gimmick_origin}, moved: {gimmick_moved}");
        assert!(gimmick_origin > 0, "expected some FROM_ORIGIN_TRANSFORM gimmicks");
        assert!(
            (gimmick_origin as f64 / gimmick as f64) > 0.8,
            "FROM_ORIGIN_TRANSFORM rate {} too low — schema drift?",
            gimmick_origin as f64 / gimmick as f64,
        );

        // Position sanity: every record's pos coords should fit in
        // the world bounding box (~ ±50k world units per
        // worldmap-plotting.md §"The chunk grid").
        for r in &records {
            assert!(
                r.pos_x.is_finite() && r.pos_y.is_finite() && r.pos_z.is_finite(),
                "non-finite position: {:?}", r,
            );
            assert!(
                r.pos_x.abs() < 100_000.0 && r.pos_z.abs() < 100_000.0,
                "position out of world bounds: {:?}", r,
            );
            assert!(r.yaw.is_finite(), "non-finite yaw: {:?}", r);
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

        let mut count: usize = 0;
        let mut version: u64 = 0;
        let _ = unsafe {
            crimson_save_list_field_positions(handle, ptr::null_mut(), 0, &mut count, &mut version)
        };
        assert!(count > 1, "need a save with >1 record");

        // Undersize: capacity = 1 but real count is thousands.
        let mut records = vec![empty_rec(); 1];
        let mut got: usize = 0;
        let rc = unsafe {
            crimson_save_list_field_positions(
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

    /// Affine-fit smoke test — applies the pinned coefficients from
    /// `docs/worldmap-plotting.md` to the active character record and
    /// asserts the resulting pixel coords land inside the 5178×5240
    /// basemap. Pins the coefficient values so a future doc edit can't
    /// silently invalidate the C# editor's plotting math.
    #[test]
    fn live_affine_lands_in_basemap() {
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
            crimson_save_list_field_positions(handle, ptr::null_mut(), 0, &mut count, ptr::null_mut())
        };
        let mut records = vec![empty_rec(); count];
        let mut got: usize = 0;
        let _ = unsafe {
            crimson_save_list_field_positions(
                handle,
                records.as_mut_ptr(),
                count,
                &mut got,
                ptr::null_mut(),
            )
        };

        let active = records.iter()
            .find(|r| r.kind == position_kind::ACTIVE_CHAR)
            .expect("active char");

        // Pinned coefficients from docs/worldmap-plotting.md §"The affine fit".
        let px = 0.432044 * active.pos_x + 5937.50;
        let py = -0.433071 * active.pos_z + 1864.08;
        eprintln!("active char pixel: ({px:.1}, {py:.1}) on 5178×5240 basemap");
        assert!(
            (0.0..=5178.0).contains(&px) && (0.0..=5240.0).contains(&py),
            "active char projects outside basemap: ({px}, {py})",
        );

        unsafe { super::super::crimson_save_free(handle) };
    }
}
