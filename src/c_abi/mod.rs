//! C ABI surface for native consumers.
//!
//! Built only when the `c_abi` Cargo feature is enabled. Exposes a
//! handle-based API: load a save once, then call zero-allocation getters
//! to read header / schema / TOC / per-block info. UTF-8 strings come out
//! through a "query size, then fill buffer" pattern (no callbacks, no
//! borrowed pointers held across calls).
//!
//! Layout mirrors the existing Rust API:
//!
//! - `crimson_save_load_from_file` parses the file with [`Save::parse`],
//!   then [`Body::parse`] and [`Body::decode_blocks`], and caches all
//!   three on the heap behind an opaque handle.
//! - Getters are pure reads into pre-existing fields — never re-parse.
//! - The handle is freed only by `crimson_save_free`. Aliasing or
//!   double-free is undefined behaviour, same as any C library.
//!
//! All entry points wrap their bodies in [`catch_unwind`] so a Rust
//! panic surfaces as [`error::PANIC`] instead of unwinding into C.

use std::ffi::CStr;
use std::fmt::Write;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::save::{
    Body, DecodedField, FieldKind, FieldValue, ObjectBlock, Save, SaveError, ScalarValue,
    scalar_from_bytes,
};

pub mod all_items;
pub mod character_info;
pub mod checksum;
pub mod craft_tool_group_info;
pub mod craft_tool_info;
pub mod dye_color_group_info;
pub mod faction_node_info;
pub mod faction_relation_group_info;
pub mod faction_spawn_data_info;
pub mod game_advice_group_info;
pub mod game_advice_info;
pub mod gameplay_variable_info;
pub mod gimmick_info;
pub mod global_game_event_group_info;
pub mod global_game_event_info;
pub mod house_info;
pub mod item_group_info;
pub mod item_part_prefab;
pub mod iteminfo;
pub mod knowledge_info;
pub mod main_quest_chapter;
pub mod mercenary_info;
pub mod mission_info;
pub mod paloc;
pub mod part_prefab_dye_slot_info;
pub mod part_prefab_dye_texture_pallete_info;
pub mod paver;
pub mod paz;
pub mod positions;
pub mod quest_gauge_info;
pub mod quest_info;
pub mod region_info;
pub mod reserve_slot_info;
pub mod royal_supply_info;
pub mod side_quest_faction;
pub mod skill_info;
pub mod stage_info;
pub mod store_info;
pub mod string_info;
pub mod sub_level_info;
pub mod trigger_region_info;

/// Stable error codes returned by every fallible C entry point.
///
/// Reserved space: `[-99, -1]`. New error categories add a new negative
/// constant; never reuse a number.
pub mod error {
    pub const OK: i32 = 0;
    pub const NULL_ARG: i32 = -1;
    pub const INVALID_PATH: i32 = -2;
    pub const IO: i32 = -3;
    pub const TOO_SMALL: i32 = -4;
    pub const BAD_MAGIC: i32 = -5;
    pub const UNSUPPORTED_VERSION: i32 = -6;
    pub const PAYLOAD_OUT_OF_RANGE: i32 = -7;
    pub const DECOMPRESS: i32 = -8;
    pub const BODY_PARSE: i32 = -9;
    pub const OUT_OF_RANGE: i32 = -10;
    pub const BUFFER_TOO_SMALL: i32 = -11;
    /// Field is not a fixed-size scalar (`fixed_prefix` / `fixed_suffix`).
    /// Returned by `crimson_save_set_scalar_field` when the caller targets
    /// a list / inline-bytes / locator / absent field.
    pub const NOT_SCALAR: i32 = -12;
    /// `bytes_len` doesn't match the field's recorded byte range.
    /// Length-changing edits are not supported by `set_scalar_field`.
    pub const LENGTH_MISMATCH: i32 = -13;
    /// `crimson_save_write_to_file` failed downstream of a successful
    /// re-serialize (filesystem error, permissions, etc.).
    pub const WRITE_FAILED: i32 = -14;
    /// A mid-path navigation step in `crimson_save_set_scalar_field_path`
    /// targeted a field whose kind is not navigable (only `ObjectLocator`
    /// with a resolved child and `ObjectList` permit descent). Distinct
    /// from `NOT_SCALAR`, which only fires on the leaf.
    pub const NOT_NAVIGABLE: i32 = -15;
    /// `crimson_paloc_lookup` could not find the requested key in the
    /// loaded localization table. Distinct from `BUFFER_TOO_SMALL`
    /// (which means "key found, but caller's buffer is too small").
    pub const NOT_FOUND: i32 = -16;
    /// A length-changing list mutation targeted an `object_list` whose
    /// `header_variant` is none of the count-patchable shapes
    /// (`zero1_count_u24`, `zero4_count_u32`, `ones_then_count`,
    /// `one_count_u16be`, `marker_run_plus_zeros`). Reserved for any
    /// future header variant whose count position we can't yet locate.
    pub const LIST_VARIANT_UNSUPPORTED: i32 = -17;
    /// A length-changing mutation targeted a field whose schema
    /// `meta_kind` isn't a fixed-size scalar (0 or 2). For example,
    /// flipping the mask bit of an `object_list` field via
    /// `crimson_save_set_scalar_field_present` is rejected — only
    /// scalar fields are supported by that entry point.
    pub const NOT_SCALAR_FIELD_KIND: i32 = -18;
    /// A length-changing mutation produced bytes the parser can't read
    /// back (e.g. `Body::write` errored, or the re-parse failed). The
    /// handle's state is restored to what it was before the call.
    pub const MUTATION_INVALID: i32 = -19;
    /// A length-changing inline-bytes mutation targeted a field whose
    /// schema `meta_kind` isn't `1` (InlineBytes). For example, calling
    /// `crimson_save_set_inline_bytes_field` against a fixed-size scalar
    /// field is rejected — the caller should use
    /// `crimson_save_set_scalar_field_present` (or
    /// `crimson_save_set_scalar_field`) for those.
    pub const NOT_INLINE_BYTES: i32 = -20;
    /// A `crimson_save_begin_deferred_redecode` call would have nested,
    /// or `crimson_save_write_to_file` was called while a deferred batch
    /// was still open. The caller MUST end / abort the open batch first.
    pub const BATCH_IN_PROGRESS: i32 = -21;
    /// `crimson_save_end_deferred_redecode` /
    /// `crimson_save_abort_deferred_redecode` was called but no batch
    /// is currently open. Pairing is begin → (end | abort).
    pub const BATCH_NOT_OPEN: i32 = -22;
    /// `crimson_save_set_object_list_present` targeted a field whose
    /// schema `meta_kind` isn't `6` or `7` (ObjectList). Used to surface
    /// the type mismatch separately from `NOT_SCALAR_FIELD_KIND` so the
    /// caller can route to the right toggle entry point.
    pub const NOT_OBJECT_LIST: i32 = -23;
    /// `crimson_save_transplant_list_element` could not map a class name
    /// carried by the source element to a type in the TARGET save's
    /// schema (the target save has never serialized that type, so it has
    /// no index for it). The transplant is rejected; the target is
    /// untouched.
    pub const TRANSPLANT_TYPE_MISSING: i32 = -24;
    pub const PANIC: i32 = -99;
}

/// Build a slice from a `(ptr, len)` pair without tripping Rust 2024's
/// stricter `slice::from_raw_parts` safety preconditions. The C ABI
/// contract says a null pointer with `len == 0` is a valid
/// representation of an empty buffer, but `from_raw_parts` requires
/// non-null even when `len == 0` (since the 2024 edition tightened
/// the unsafe precondition). This helper returns `&[]` for the empty
/// case, side-stepping the UB.
///
/// All `extern "C"` entry points that build a slice from a caller-
/// provided buffer should go through this helper instead of
/// `slice::from_raw_parts` directly. Callers that always pre-check
/// `ptr.is_null() && len != 0` are still vulnerable to the null+0
/// case — the helper closes that gap.
///
/// # Safety
/// When `len != 0`, `ptr` MUST be non-null and point to `len`
/// readable elements aligned for `T`. When `len == 0`, `ptr` may be
/// anything (null or arbitrary).
#[inline]
unsafe fn slice_from_raw_or_empty<'a, T>(ptr: *const T, len: usize) -> &'a [T] {
    if len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(ptr, len) }
    }
}

/// Shared two-call-pattern string writer. `required` always reports
/// `src.len() + 1` (the NUL terminator) — callers query with
/// `buf_len = 0` first, then provide a sized buffer.
///
/// # Safety
/// `buf` may be null iff `buf_len == 0`. `required` must be non-null.
/// The existing per-bridge helpers (in `faction_node_info`, `store_info`,
/// etc.) predate this and remain inlined; new bridges should use this
/// shared copy.
pub(crate) fn write_str_to_buf(
    src: &str,
    buf: *mut u8,
    buf_len: usize,
    required: *mut usize,
) -> i32 {
    let needed = src.len() + 1;
    unsafe { *required = needed };
    if buf_len < needed {
        return error::BUFFER_TOO_SMALL;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(src.as_ptr(), buf, src.len());
        *buf.add(src.len()) = 0;
    }
    error::OK
}

/// Two-call-pattern **raw bytes** writer — the no-NUL sibling of
/// [`write_str_to_buf`]. `required` reports `src.len()` exactly (no
/// terminator); the payload is copied verbatim. Used by
/// [`crimson_save_get_inline_bytes_field`], whose payload is arbitrary
/// bytes (length-prefixed UTF-8 etc.) the caller already knows the
/// length of.
///
/// # Safety
/// `buf` may be null iff `buf_len == 0`. `required` must be non-null.
pub(crate) fn write_bytes_to_buf(
    src: &[u8],
    buf: *mut u8,
    buf_len: usize,
    required: *mut usize,
) -> i32 {
    let needed = src.len();
    unsafe { *required = needed };
    if buf_len < needed {
        return error::BUFFER_TOO_SMALL;
    }
    if needed > 0 {
        unsafe {
            std::ptr::copy_nonoverlapping(src.as_ptr(), buf, needed);
        }
    }
    error::OK
}

/// Macro for the "name-only key resolver" bridge family — issues the
/// standard handle struct + six `extern "C"` functions (load_from_file,
/// load_from_bytes, free, entry_count, lookup_string_key, get_entry).
///
/// Used by the 2026-05-16 niche-bridge batch (HouseKey, RoyalSupplyKey,
/// CraftToolKey, ... ). The earlier bridges (store_info, faction_node_info,
/// etc.) predate the macro and remain hand-written; they can be migrated
/// later if a cross-cutting ABI change ever lands.
///
/// Usage:
///
/// ```ignore
/// crate::impl_name_only_bridge! {
///     handle = CrimsonHouseInfoHandle,
///     parser = crate::house_info::parse_house_info_lossy,
///     entry_ty = crate::house_info::HouseInfoEntry,
///     load_from_file = crimson_house_info_load_from_file,
///     load_from_bytes = crimson_house_info_load_from_bytes,
///     free = crimson_house_info_free,
///     entry_count = crimson_house_info_entry_count,
///     lookup_string_key = crimson_house_info_lookup_string_key,
///     get_entry = crimson_house_info_get_entry,
///     key_param = house_key,
/// }
/// ```
#[macro_export]
macro_rules! impl_name_only_bridge {
    (
        handle = $Handle:ident,
        parser = $parse_fn:path,
        entry_ty = $EntryTy:path,
        load_from_file = $load_file:ident,
        load_from_bytes = $load_bytes:ident,
        free = $free:ident,
        entry_count = $entry_count:ident,
        lookup_string_key = $lookup:ident,
        get_entry = $get_entry:ident,
        key_param = $key_param:ident,
    ) => {
        #[repr(C)]
        pub struct $Handle {
            by_key: std::collections::HashMap<u32, String>,
            entries: Vec<(u32, String)>,
        }

        impl $Handle {
            fn from_bytes(pabgb: &[u8], pabgh: &[u8]) -> Self {
                let raw: Vec<$EntryTy> = $parse_fn(pabgb, pabgh);
                let mut by_key: std::collections::HashMap<u32, String> =
                    std::collections::HashMap::with_capacity(raw.len());
                let mut entries: Vec<(u32, String)> = Vec::with_capacity(raw.len());
                for e in raw {
                    if let std::collections::hash_map::Entry::Vacant(v) =
                        by_key.entry(e.key)
                    {
                        v.insert(e.name.clone());
                        entries.push((e.key, e.name));
                    }
                }
                $Handle { by_key, entries }
            }
        }

        /// # Safety
        /// Both path arguments must be NUL-terminated UTF-8 strings.
        /// `out_handle` must be non-null.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $load_file(
            pabgb_path: *const std::os::raw::c_char,
            pabgh_path: *const std::os::raw::c_char,
            out_handle: *mut *mut $Handle,
        ) -> i32 {
            if pabgb_path.is_null() || pabgh_path.is_null() || out_handle.is_null() {
                return $crate::c_abi::error::NULL_ARG;
            }
            unsafe { *out_handle = std::ptr::null_mut() };
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let pabgb_str =
                    match unsafe { std::ffi::CStr::from_ptr(pabgb_path) }.to_str() {
                        Ok(s) => s,
                        Err(_) => return $crate::c_abi::error::INVALID_PATH,
                    };
                let pabgh_str =
                    match unsafe { std::ffi::CStr::from_ptr(pabgh_path) }.to_str() {
                        Ok(s) => s,
                        Err(_) => return $crate::c_abi::error::INVALID_PATH,
                    };
                let pabgb = match std::fs::read(pabgb_str) {
                    Ok(b) => b,
                    Err(_) => return $crate::c_abi::error::IO,
                };
                let pabgh = match std::fs::read(pabgh_str) {
                    Ok(b) => b,
                    Err(_) => return $crate::c_abi::error::IO,
                };
                let handle = $Handle::from_bytes(&pabgb, &pabgh);
                unsafe { *out_handle = Box::into_raw(Box::new(handle)) };
                $crate::c_abi::error::OK
            }))
            .unwrap_or($crate::c_abi::error::PANIC)
        }

        /// # Safety
        /// `pabgb`/`pabgh` may be null iff length is 0; `out_handle`
        /// must be non-null.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $load_bytes(
            pabgb: *const u8,
            pabgb_len: usize,
            pabgh: *const u8,
            pabgh_len: usize,
            out_handle: *mut *mut $Handle,
        ) -> i32 {
            if out_handle.is_null() {
                return $crate::c_abi::error::NULL_ARG;
            }
            if (pabgb.is_null() && pabgb_len != 0)
                || (pabgh.is_null() && pabgh_len != 0)
            {
                return $crate::c_abi::error::NULL_ARG;
            }
            unsafe { *out_handle = std::ptr::null_mut() };
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let pabgb_slice = if pabgb_len == 0 {
                    &[][..]
                } else {
                    unsafe { std::slice::from_raw_parts(pabgb, pabgb_len) }
                };
                let pabgh_slice = if pabgh_len == 0 {
                    &[][..]
                } else {
                    unsafe { std::slice::from_raw_parts(pabgh, pabgh_len) }
                };
                let handle = $Handle::from_bytes(pabgb_slice, pabgh_slice);
                unsafe { *out_handle = Box::into_raw(Box::new(handle)) };
                $crate::c_abi::error::OK
            }))
            .unwrap_or($crate::c_abi::error::PANIC)
        }

        /// # Safety
        /// `handle` must be null or a pointer previously returned by
        /// one of the loaders and not yet freed.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $free(handle: *mut $Handle) {
            if handle.is_null() {
                return;
            }
            unsafe {
                let _ = Box::from_raw(handle);
            }
        }

        /// # Safety
        /// `handle` must be live; `out_count` must be non-null.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $entry_count(
            handle: *const $Handle,
            out_count: *mut u32,
        ) -> i32 {
            if handle.is_null() || out_count.is_null() {
                return $crate::c_abi::error::NULL_ARG;
            }
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let h = unsafe { &*handle };
                unsafe { *out_count = h.entries.len() as u32 };
                $crate::c_abi::error::OK
            }))
            .unwrap_or($crate::c_abi::error::PANIC)
        }

        /// # Safety
        /// `handle` and `required` must be non-null; `buf` may be null
        /// iff `buf_len == 0`.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $lookup(
            handle: *const $Handle,
            $key_param: u32,
            buf: *mut u8,
            buf_len: usize,
            required: *mut usize,
        ) -> i32 {
            if handle.is_null() || required.is_null() {
                return $crate::c_abi::error::NULL_ARG;
            }
            if buf.is_null() && buf_len != 0 {
                return $crate::c_abi::error::NULL_ARG;
            }
            unsafe { *required = 0 };
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let h = unsafe { &*handle };
                let Some(name) = h.by_key.get(&$key_param) else {
                    return $crate::c_abi::error::NOT_FOUND;
                };
                $crate::c_abi::write_str_to_buf(name, buf, buf_len, required)
            }))
            .unwrap_or($crate::c_abi::error::PANIC)
        }

        /// # Safety
        /// `handle`, `out_key`, and `required` must be non-null; `buf`
        /// may be null iff `buf_len == 0`.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $get_entry(
            handle: *const $Handle,
            idx: u32,
            out_key: *mut u32,
            buf: *mut u8,
            buf_len: usize,
            required: *mut usize,
        ) -> i32 {
            if handle.is_null() || out_key.is_null() || required.is_null() {
                return $crate::c_abi::error::NULL_ARG;
            }
            if buf.is_null() && buf_len != 0 {
                return $crate::c_abi::error::NULL_ARG;
            }
            unsafe {
                *out_key = 0;
                *required = 0;
            }
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let h = unsafe { &*handle };
                let Some((key, name)) = h.entries.get(idx as usize) else {
                    return $crate::c_abi::error::OUT_OF_RANGE;
                };
                unsafe { *out_key = *key };
                $crate::c_abi::write_str_to_buf(name, buf, buf_len, required)
            }))
            .unwrap_or($crate::c_abi::error::PANIC)
        }
    };
}

/// Opaque handle handed out across the ABI boundary. C side only sees
/// `CrimsonSaveHandle*` and uses it as a token.
///
/// `mutation_version` is a monotonic counter bumped by every mutating
/// entry point ([`crimson_save_set_scalar_field`],
/// [`crimson_save_list_remove_element`], the `*_batch` variants, etc.).
/// Pure read paths ([`crimson_save_get_block_json`],
/// [`crimson_save_list_inventory_items`], …) DO NOT bump it. Snapshot
/// readers (the canonical example is
/// [`crimson_save_list_inventory_items`], which returns positional
/// `(block_idx, element_idx)` paths that become stale after
/// length-changing mutations) stamp the version at read time via
/// [`crimson_save_get_mutation_version`] and re-walk when it changes.
/// See that function's doc for the canonical pattern.
#[repr(C)]
pub struct CrimsonSaveHandle {
    save: Save,
    body: Body,
    blocks: Vec<ObjectBlock>,
    /// Monotonic counter — bumped exactly once per successful mutation.
    /// Uses wrapping add; 64 bits is enough headroom that the wrap is
    /// purely defensive (584 years at 1 GHz of mutations). Initialised
    /// to 0 by [`crimson_save_load_from_file`].
    mutation_version: u64,
    /// `Some` only between matched
    /// [`crimson_save_begin_deferred_redecode`] +
    /// [`crimson_save_end_deferred_redecode`] /
    /// [`crimson_save_abort_deferred_redecode`] pairs. During a deferred
    /// batch every mutation entry point skips the encode + re-parse +
    /// decode_blocks tail; the cost lands once in `end_*`. The
    /// snapshot captured here lets `abort_*` (and a failing `end_*`)
    /// restore the pre-begin state.
    ///
    /// While a batch is open, `save.body` is the pre-begin byte image
    /// and `blocks` is the in-progress in-memory tree — they're
    /// intentionally inconsistent until `end_*` re-emits.
    deferred_state: Option<DeferredState>,
}

/// Per-batch rollback snapshot for the deferred-redecode mode.
///
/// Only `blocks` is mutated during a batch (every mutation operates
/// on the in-memory tree); `save.body` + `body` stay at their
/// pre-batch state and don't need snapshotting until `end_*` re-emits.
struct DeferredState {
    blocks_backup: Vec<ObjectBlock>,
    version_at_begin: u64,
}

impl CrimsonSaveHandle {
    /// Bump the mutation counter. Call exactly once per successful
    /// mutation, from inside the [`catch_unwind`] body, AFTER the
    /// underlying state change has been committed (so a rollback path
    /// doesn't leak a phantom bump).
    fn bump_version(&mut self) {
        self.mutation_version = self.mutation_version.wrapping_add(1);
    }

    /// True while a [`crimson_save_begin_deferred_redecode`] batch is
    /// open. Mutation entry points use this to skip their
    /// `decode_blocks` tail; reads work as normal — `blocks` is always
    /// the in-progress tree.
    fn is_deferred(&self) -> bool {
        self.deferred_state.is_some()
    }
}

/// Per-block summary returned by [`crimson_save_get_block_info`].
///
/// `fields_present`: count of fields whose presence bit was set in the
/// block's mask.
/// `fields_decoded`: subset of those that resolved to a concrete
/// [`FieldKind`] (excludes `Absent` and `Unknown`). When parsing a save
/// from a supported game version the two are expected to match.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct CrimsonBlockInfo {
    pub class_index: u32,
    pub data_offset: u32,
    pub data_size: u32,
    pub fields_present: u32,
    pub fields_decoded: u32,
}

/// One step of a descent path used by
/// [`crimson_save_set_scalar_field_path`].
///
/// Each step says: "from the current block, look up `field_idx`; if that
/// field is an `ObjectList`, descend into element `element_idx`; if it's
/// an `ObjectLocator` with a resolved child, descend into the child
/// (`element_idx` is ignored)". Anything else fails with `NOT_NAVIGABLE`.
///
/// The terminal field index (the scalar being written) is passed
/// separately from the path so the caller can address either a top-level
/// block field (empty path) or a deeply-nested one with the same API.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct CrimsonPathStep {
    pub field_idx: u32,
    pub element_idx: u32,
}

/// One element of a scalar batch mutation, passed to
/// [`crimson_save_set_scalar_fields_batch`].
///
/// Each op fully describes one scalar write: target block, descent path,
/// leaf field index, and replacement bytes. The path and byte buffers are
/// borrowed for the duration of the batch call only — the caller owns and
/// keeps them alive across the FFI boundary.
///
/// Layout matches the argument list of
/// [`crimson_save_set_scalar_field_path`] so the same caller-side
/// validation rules apply per-op.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CrimsonScalarBatchOp {
    pub block_idx: u32,
    pub field_idx: u32,
    pub path: *const CrimsonPathStep,
    pub path_len: usize,
    pub bytes: *const u8,
    pub bytes_len: usize,
}

/// One element of a scalar-presence batch mutation, passed to
/// [`crimson_save_set_scalar_fields_present_batch`].
///
/// Each op fully describes one mask-bit toggle: target block, descent
/// path, leaf field index, the new presence flag, and (when making the
/// field present) the initialization bytes for the scalar value. The
/// path and byte buffers are borrowed for the duration of the batch
/// call only — the caller owns and keeps them alive across the FFI
/// boundary.
///
/// Layout matches the argument list of
/// [`crimson_save_set_scalar_field_present`] so the same caller-side
/// validation rules apply per-op. `make_present` is non-zero to make
/// the field present, zero to make it absent. When zero, `bytes` /
/// `bytes_len` are ignored and may be NULL / 0.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CrimsonScalarPresentBatchOp {
    pub block_idx: u32,
    pub field_idx: u32,
    pub path: *const CrimsonPathStep,
    pub path_len: usize,
    pub make_present: i32,
    pub bytes: *const u8,
    pub bytes_len: usize,
}

/// One element of a list-element removal batch, passed to
/// [`crimson_save_list_remove_elements_batch`].
///
/// Each op fully describes one removal: target block, descent path,
/// leaf list field index, and the element index to drop. Same per-op
/// validation rules as [`crimson_save_list_remove_element`].
///
/// **Caller is responsible for pre-sorting** ops targeting the same
/// list by descending `element_idx` so that earlier removes don't
/// shift later ones out from under their indexes. Ops are applied in
/// input order; if a later op's `element_idx` is out of range after
/// earlier removes, that op fails with `OUT_OF_RANGE` and the whole
/// batch is rolled back.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CrimsonListRemoveBatchOp {
    pub block_idx: u32,
    pub field_idx: u32,
    pub path: *const CrimsonPathStep,
    pub path_len: usize,
    pub element_idx: u32,
}

/// One flat record emitted by [`crimson_save_list_inventory_items`] —
/// a `repr(C)` 48-byte structure laid out for direct mmap-style read
/// from C# / C++ consumers.
///
/// Field layout (all little-endian, naturally aligned):
///
/// | Offset | Field                       | Type | Purpose |
/// |--------|-----------------------------|------|---------|
/// |  0     | `block_idx`                 | u32  | Top-level `InventorySaveData` block index in the save |
/// |  4     | `inventory_element_idx`     | u32  | Position in `_inventorylist[N]` (the container index, 0..17) |
/// |  8     | `item_element_idx`          | u32  | Position in `_itemList[M]` (the item index within the container) |
/// | 12     | `inventory_key`             | u32  | `InventoryKey` value from the container — the category id the C# `LocalizationProvider.cs` table labels (e.g. `2` = Backpack, `5` = Quest Artifacts, …). u16 widened to u32 for alignment. |
/// | 16     | `item_key`                  | u32  | `ItemKey` for this item slot — the gamedata key consumers search by. |
/// | 20     | `transferred_item_key`      | u32  | `_transferredItemKey` (origin item key when this slot was transferred from another). 0 when absent. |
/// | 24     | `slot_no`                   | u32  | `_slotNo` — visual slot within the container. u16 widened. |
/// | 28     | `flags`                     | u32  | Bitfield: bit 0 = `_isLocked`, bit 1 = `_isNewMark`. Other bits reserved 0. |
/// | 32     | `item_no`                   | u64  | `_itemNo` — per-save unique instance id (stable across mutations until the item is removed). |
/// | 40     | `stack_count`               | u64  | `_stackCount` — current stack size. |
///
/// `inventory_element_idx` + `item_element_idx` form the **descent
/// path** the C ABI uses to address this exact slot from
/// `crimson_save_set_scalar_field_path` and friends. Specifically:
/// `block_idx = record.block_idx`, `path = [(field=0 (_inventorylist),
/// element=record.inventory_element_idx), (field=2 (_itemList),
/// element=record.item_element_idx)]`, `field_idx = <ItemSaveData
/// scalar field index>`.
///
/// **Validity window**: positional fields stay valid only until the
/// next length-changing mutation in the relevant inventory list.
/// Combine with [`crimson_save_get_mutation_version`] to detect
/// staleness. See that function's doc.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CrimsonInventoryItemRecord {
    pub block_idx: u32,
    pub inventory_element_idx: u32,
    pub item_element_idx: u32,
    pub inventory_key: u32,
    pub item_key: u32,
    pub transferred_item_key: u32,
    pub slot_no: u32,
    pub flags: u32,
    pub item_no: u64,
    pub stack_count: u64,
}

/// Bit constants for [`CrimsonInventoryItemRecord::flags`].
pub mod inventory_item_flags {
    /// `_isLocked` field was present and `true`.
    pub const LOCKED: u32 = 1 << 0;
    /// `_isNewMark` field was present and `true`.
    pub const NEW_MARK: u32 = 1 << 1;
}

// ── Load / free ────────────────────────────────────────────────────────────

/// Load and fully decode a `.save` file.
///
/// `path` must point to a NUL-terminated UTF-8 string. On success the
/// caller owns `*out_handle` and must release it via
/// [`crimson_save_free`].
///
/// # Safety
/// `path` must be a valid pointer to a NUL-terminated UTF-8 string for
/// the duration of the call. `out_handle` must be a writable pointer to
/// `*mut CrimsonSaveHandle`. On any error (`!= OK`) the function does
/// not write through `out_handle`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_load_from_file(
    path: *const c_char,
    out_handle: *mut *mut CrimsonSaveHandle,
) -> i32 {
    if path.is_null() || out_handle.is_null() {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let path_str = match unsafe { CStr::from_ptr(path) }.to_str() {
            Ok(s) => s,
            Err(_) => return error::INVALID_PATH,
        };

        let bytes = match std::fs::read(path_str) {
            Ok(b) => b,
            Err(_) => return error::IO,
        };

        let save = match Save::parse(&bytes) {
            Ok(s) => s,
            Err(e) => return save_error_code(&e),
        };
        let body = match Body::parse(&save.body) {
            Ok(b) => b,
            Err(_) => return error::BODY_PARSE,
        };
        let blocks = body.decode_blocks(&save.body);

        let boxed = Box::new(CrimsonSaveHandle {
            save,
            body,
            blocks,
            mutation_version: 0,
            deferred_state: None,
        });
        unsafe {
            *out_handle = Box::into_raw(boxed);
        }
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Free a handle returned by [`crimson_save_load_from_file`]. Passing a
/// NULL pointer is a no-op. Passing any other pointer is undefined
/// behaviour, exactly like `free()`.
///
/// # Safety
/// `handle` must either be NULL or a pointer previously returned by
/// [`crimson_save_load_from_file`] that has not already been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_free(handle: *mut CrimsonSaveHandle) {
    if handle.is_null() {
        return;
    }
    // catch_unwind because Drop on user-supplied data could in principle
    // panic; we still want the C side to see a normal return.
    let _ = catch_unwind(AssertUnwindSafe(|| {
        unsafe {
            drop(Box::from_raw(handle));
        }
    }));
}

// ── Inventory enumeration ─────────────────────────────────────────────────

/// Flat-list every item slot across every `InventorySaveData` block in
/// the save. Output is a contiguous array of
/// [`CrimsonInventoryItemRecord`] values (48 bytes each, `repr(C)`),
/// suitable for direct `MemoryMarshal.Cast` / `Span<T>` consumption on
/// the C# side. Saves a downstream editor from walking 18
/// `_inventorylist[N]` × `_itemList[M]` nesting in N+1 FFI calls — one
/// call returns the whole picture.
///
/// **What's in each record** — block + path indices for FFI mutations
/// targeting that exact slot, plus the search-relevant fields
/// (`inventory_key`, `item_key`, `transferred_item_key`, `slot_no`,
/// `item_no`, `stack_count`, `flags`). See
/// [`CrimsonInventoryItemRecord`] for the precise byte layout.
///
/// **Two-call shape** (record-array variant — counts in records, not
/// bytes, unlike `crimson_paz_list_*` which uses bytes):
///
/// - First call with `out_records = null, capacity_records = 0`
///   returns `BUFFER_TOO_SMALL` (or `OK` if the save has zero items)
///   and populates `*out_count_records` and `*out_version`.
/// - Allocate `*out_count_records` records, call again.
///
/// **`out_version` (optional, may be null)**: receives the value of
/// the save handle's mutation counter at read time. Pair the snapshot
/// with this value so subsequent
/// [`crimson_save_get_mutation_version`] calls can detect staleness in
/// O(1) — the canonical C# pattern is:
///
/// ```text
/// // First call: query size + version.
/// size_t count = 0; uint64_t v = 0;
/// crimson_save_list_inventory_items(h, NULL, 0, &count, &v);
/// CrimsonInventoryItemRecord* records = malloc(count * sizeof(*records));
/// crimson_save_list_inventory_items(h, records, count, &count, &v);
///
/// // Later: cheap staleness check.
/// uint64_t now;
/// crimson_save_get_mutation_version(h, &now);
/// if (now != v) {
///     // Free + re-list. crimson_save_set_*/list_* fired between calls.
/// }
/// ```
///
/// **What this function reads** (no mutation):
/// - Every `InventorySaveData` block (typically 1 per save, but the
///   ABI doesn't assume — multi-block saves work the same).
/// - The `_inventorylist` ObjectList (18 elements in 1.07; ABI doesn't
///   pin the count). Each element is an `InventoryElementSaveData`
///   with `_inventoryKey: InventoryKey (u16)` + `_itemList: ObjectList`.
/// - Every `_itemList` element is an `ItemSaveData` row — pulls
///   `_itemKey`, `_slotNo`, `_stackCount`, `_itemNo`,
///   `_transferredItemKey`, `_isLocked`, `_isNewMark` into the record.
///   Other 18 ItemSaveData fields (sockets / endurance / charge data /
///   dye lists / etc.) are NOT in the record — fetch via
///   [`crimson_save_get_block_json`] when needed for a specific slot.
///
/// **Performance**: O(total items × small constant). For the 1.07
/// reference save (543 items across 18 containers), end-to-end cost
/// is under 1 ms. No allocation beyond the output buffer the caller
/// owns.
///
/// Return codes:
/// - `OK` — list written. `*out_count_records` and `*out_version` are
///   populated. When the save has zero inventory items, this is the
///   first-call return (rather than `BUFFER_TOO_SMALL`).
/// - `BUFFER_TOO_SMALL` — `capacity_records < *out_count_records`.
///   `*out_count_records` is populated so the caller can allocate.
///   `*out_version` is populated so the caller doesn't lose the
///   version stamp between calls.
/// - `NULL_ARG` — any required pointer is null (see Safety).
///
/// # Safety
/// `handle` must be a live handle from
/// [`crimson_save_load_from_file`]. `out_count_records` must point to
/// writable `usize` memory. `out_records` may be null iff
/// `capacity_records == 0`. `out_version` may be null (the version is
/// then dropped).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_list_inventory_items(
    handle: *const CrimsonSaveHandle,
    out_records: *mut CrimsonInventoryItemRecord,
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
        let _ffi_guard = save_ffi_lock();
        let h = unsafe { &*handle };
        if !out_version.is_null() {
            unsafe { *out_version = h.mutation_version };
        }

        // Single pass: collect into a Vec first so we know the total
        // count before deciding whether the caller's buffer fits.
        let mut records: Vec<CrimsonInventoryItemRecord> = Vec::new();
        for (block_idx, block) in h.blocks.iter().enumerate() {
            if block.class_name != "InventorySaveData" {
                continue;
            }
            for inv_list_field in &block.fields {
                if !inv_list_field.name.eq_ignore_ascii_case("_inventorylist") {
                    continue;
                }
                let FieldValue::ObjectList { elements: containers, .. } =
                    &inv_list_field.value
                else {
                    continue;
                };
                for (inv_idx, container) in containers.iter().enumerate() {
                    let inv_key: u32 = container
                        .fields
                        .iter()
                        .find(|f| f.name.eq_ignore_ascii_case("_inventoryKey"))
                        .and_then(|f| match &f.value {
                            FieldValue::Scalar(ScalarValue::U16(v)) => Some(u32::from(*v)),
                            FieldValue::Scalar(ScalarValue::U32(v)) => Some(*v),
                            _ => None,
                        })
                        .unwrap_or(0);

                    for f in &container.fields {
                        if !f.name.eq_ignore_ascii_case("_itemList") {
                            continue;
                        }
                        let FieldValue::ObjectList { elements: items, .. } = &f.value else {
                            continue;
                        };
                        for (item_idx, item) in items.iter().enumerate() {
                            let mut rec = CrimsonInventoryItemRecord {
                                block_idx: block_idx as u32,
                                inventory_element_idx: inv_idx as u32,
                                item_element_idx: item_idx as u32,
                                inventory_key: inv_key,
                                item_key: 0,
                                transferred_item_key: 0,
                                slot_no: 0,
                                flags: 0,
                                item_no: 0,
                                stack_count: 0,
                            };
                            for itf in &item.fields {
                                if !itf.present {
                                    continue;
                                }
                                match (itf.name.as_str(), &itf.value) {
                                    ("_itemKey", FieldValue::Scalar(ScalarValue::U32(v))) => {
                                        rec.item_key = *v;
                                    }
                                    (
                                        "_transferredItemKey",
                                        FieldValue::Scalar(ScalarValue::U32(v)),
                                    ) => {
                                        rec.transferred_item_key = *v;
                                    }
                                    ("_slotNo", FieldValue::Scalar(ScalarValue::U16(v))) => {
                                        rec.slot_no = u32::from(*v);
                                    }
                                    ("_itemNo", FieldValue::Scalar(ScalarValue::U64(v))) => {
                                        rec.item_no = *v;
                                    }
                                    ("_stackCount", FieldValue::Scalar(ScalarValue::U64(v))) => {
                                        rec.stack_count = *v;
                                    }
                                    (
                                        "_isLocked",
                                        FieldValue::Scalar(ScalarValue::Bool(b)),
                                    ) if *b != 0 => {
                                        rec.flags |= inventory_item_flags::LOCKED;
                                    }
                                    (
                                        "_isNewMark",
                                        FieldValue::Scalar(ScalarValue::Bool(b)),
                                    ) if *b != 0 => {
                                        rec.flags |= inventory_item_flags::NEW_MARK;
                                    }
                                    _ => {}
                                }
                            }
                            records.push(rec);
                        }
                    }
                }
            }
        }

        unsafe { *out_count_records = records.len() };
        if records.is_empty() {
            return error::OK;
        }
        if records.len() > capacity_records {
            return error::BUFFER_TOO_SMALL;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(records.as_ptr(), out_records, records.len());
        }
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

// ── CharacterKey reference enumeration ────────────────────────────────────

/// One flat record emitted by [`crimson_save_list_character_refs`] —
/// a `repr(C)` 16-byte structure laid out for direct mmap-style read
/// from C# / C++ consumers.
///
/// Field layout (little-endian, naturally aligned):
///
/// | Offset | Field             | Type | Purpose |
/// |--------|-------------------|------|---------|
/// |  0     | `block_idx`       | u32  | Top-level block index containing this reference. Pass to [`crimson_save_get_block_json`] for the full block payload. |
/// |  4     | `character_key`   | u32  | The `CharacterKey` value. Feed through the gamedata-side `crimson_characterinfo_lookup_string_key` / `_lookup_display_name` to resolve to "Greymane" / "灰鬃" etc. |
/// |  8     | `class_index`     | u32  | Schema class index of the top-level block — coarse hint for where in the save tree this reference lives (e.g. `CharacterStatusSaveData` vs `NPCScheduleStageManagerSaveData`). Resolve to a class_name via [`crimson_save_get_block_info`]. |
/// | 12     | `reserved0`       | u32  | Reserved for future use; always 0. |
///
/// **Coverage**: every present field whose declared `type_name ==
/// "CharacterKey"` is emitted, regardless of nesting depth. Fixed-size
/// scalar fields (`meta_kind` 0 / 2) produce one record per field;
/// `CharacterKey` dynamic-array fields produce one record per element.
/// Absent fields and unrelated u32 fields that happen to hold a
/// character key by coincidence are NOT included — only fields the
/// schema declares as `CharacterKey`.
///
/// **Duplicates**: the same character may appear many times across
/// different blocks (e.g. Greymane referenced from a `_characterKey`
/// scalar AND from an ObjectList of party members). The enumerator
/// emits one record per **field occurrence**, not per distinct key.
/// Callers wanting "every character referenced in this save" dedupe
/// on `character_key` themselves.
///
/// **Validity window**: `block_idx` stays valid only until the next
/// length-changing mutation. Combine with
/// [`crimson_save_get_mutation_version`] for staleness detection (same
/// pattern as [`crimson_save_list_inventory_items`]).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CrimsonCharacterRefRecord {
    pub block_idx: u32,
    pub character_key: u32,
    pub class_index: u32,
    pub reserved0: u32,
}

/// Flat-list every present `CharacterKey` field across every block in
/// the save (top-level + nested via ObjectList / Locator). Output is a
/// contiguous array of [`CrimsonCharacterRefRecord`] values (16 bytes
/// each, `repr(C)`).
///
/// Use to answer: "which characters does this save reference, and where?"
/// — closes the gap noted in the Save Editor's character browser
/// (screenshot, 2026-05-17).
///
/// **Two-call shape** (records, not bytes):
///
/// - First call with `out_records = null, capacity_records = 0`
///   returns `BUFFER_TOO_SMALL` (or `OK` if the save has zero refs).
///   Populates `*out_count_records` and `*out_version`.
/// - Allocate `*out_count_records` records, call again.
///
/// **`out_version`** (may be null) — handle's mutation counter at read
/// time. See [`crimson_save_list_inventory_items`] for the snapshot /
/// staleness pattern.
///
/// **Performance**: O(blocks × fields × nesting_depth). For the 1.07
/// reference save this scans ~10k blocks × ~30 fields each in well
/// under 100 ms. Single allocation up front; no per-record allocs.
///
/// Return codes:
/// - `OK` — written. Populates `*out_count_records` and `*out_version`.
/// - `BUFFER_TOO_SMALL` — `capacity_records < *out_count_records`.
///   `*out_count_records` and `*out_version` are populated.
/// - `NULL_ARG` — any required pointer is null.
///
/// # Safety
/// `handle` must be a live handle. `out_count_records` must point to
/// writable `usize` memory. `out_records` may be null iff
/// `capacity_records == 0`. `out_version` may be null (then dropped).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_list_character_refs(
    handle: *const CrimsonSaveHandle,
    out_records: *mut CrimsonCharacterRefRecord,
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
        let _ffi_guard = save_ffi_lock();
        let h = unsafe { &*handle };
        if !out_version.is_null() {
            unsafe { *out_version = h.mutation_version };
        }

        let mut records: Vec<CrimsonCharacterRefRecord> = Vec::new();
        for (block_idx, block) in h.blocks.iter().enumerate() {
            walk_for_character_refs(block, block_idx as u32, block.class_index, &mut records);
        }

        unsafe { *out_count_records = records.len() };
        if records.is_empty() {
            return error::OK;
        }
        if records.len() > capacity_records {
            return error::BUFFER_TOO_SMALL;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(records.as_ptr(), out_records, records.len());
        }
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Recursive walker for [`crimson_save_list_character_refs`]. Emits one
/// record per present `CharacterKey` field occurrence and descends into
/// ObjectList elements + Locator children. `top_block_idx` and
/// `top_class_index` always reference the OUTER top-level block — they
/// don't change when we descend into nested elements, since the
/// canonical record's `block_idx` always points at the top-level block.
fn walk_for_character_refs(
    block: &ObjectBlock,
    top_block_idx: u32,
    top_class_index: u32,
    out: &mut Vec<CrimsonCharacterRefRecord>,
) {
    for f in &block.fields {
        if !f.present {
            // Don't descend into ObjectList children when the list
            // itself is absent — there are no decoded elements to walk.
            continue;
        }
        if f.type_name == "CharacterKey" {
            match &f.value {
                FieldValue::Scalar(ScalarValue::U32(v)) => {
                    out.push(CrimsonCharacterRefRecord {
                        block_idx: top_block_idx,
                        character_key: *v,
                        class_index: top_class_index,
                        reserved0: 0,
                    });
                }
                // DynamicArray of CharacterKey (meta_kind == 3) — each
                // element is a u32. None seen in the 1.07 sample save
                // but the survey probe doesn't rule it out, so handle
                // it defensively.
                FieldValue::DynamicArray { bytes, count, .. } if f.meta_size == 4 => {
                    let n = (*count as usize).min(bytes.len() / 4);
                    for i in 0..n {
                        let off = i * 4;
                        let v = u32::from_le_bytes([
                            bytes[off],
                            bytes[off + 1],
                            bytes[off + 2],
                            bytes[off + 3],
                        ]);
                        out.push(CrimsonCharacterRefRecord {
                            block_idx: top_block_idx,
                            character_key: v,
                            class_index: top_class_index,
                            reserved0: 0,
                        });
                    }
                }
                _ => {}
            }
        }
        // Descend regardless of type — a CharacterKey may live deep
        // inside an unrelated parent (e.g. inside a ReflectObjectPtr
        // sub-list).
        match &f.value {
            FieldValue::ObjectList { elements, .. } => {
                for e in elements {
                    walk_for_character_refs(e, top_block_idx, top_class_index, out);
                }
            }
            FieldValue::Locator { child: Some(c), .. } => {
                walk_for_character_refs(c, top_block_idx, top_class_index, out);
            }
            _ => {}
        }
    }
}

// ── Mutation-version counter ──────────────────────────────────────────────

/// Read the handle's mutation counter — a monotonic `u64` that bumps by
/// exactly 1 on every successful mutation through the C ABI surface
/// ([`crimson_save_set_scalar_field`],
/// [`crimson_save_list_remove_element`], the batch variants, etc.).
/// Pure read entry points DO NOT bump it.
///
/// **Purpose**: cache-coherency for snapshot readers. APIs like
/// [`crimson_save_list_inventory_items`] return positional
/// `(block_idx, element_idx)` paths that become stale after any
/// length-changing mutation. The canonical pattern is:
///
/// ```text
/// uint64_t v = crimson_save_get_mutation_version(handle);
/// crimson_save_list_inventory_items(handle, …, &v_out);  // v_out == v
/// // … hold the snapshot, do stuff, possibly mutate via crimson_save_set_*
/// if (crimson_save_get_mutation_version(handle) != v) {
///     // snapshot is stale — re-list
/// }
/// ```
///
/// Cost: one pointer dereference + one `u64` read. The C ABI surface is
/// stable across the lifetime of a handle; no cache invalidation
/// beyond the version comparison is needed.
///
/// `out_version` must be non-null. Returns `OK` on success;
/// `NULL_ARG` if either pointer is null.
///
/// # Safety
/// `handle` must be a live handle from
/// [`crimson_save_load_from_file`]. `out_version` must point to
/// writable memory for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_get_mutation_version(
    handle: *const CrimsonSaveHandle,
    out_version: *mut u64,
) -> i32 {
    if handle.is_null() || out_version.is_null() {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let _ffi_guard = save_ffi_lock();
        let h = unsafe { &*handle };
        unsafe { *out_version = h.mutation_version };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

// ── Scalar getters ─────────────────────────────────────────────────────────

macro_rules! scalar_getter {
    ($name:ident, $out_ty:ty, $expr:expr) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(
            handle: *const CrimsonSaveHandle,
            out: *mut $out_ty,
        ) -> i32 {
            with_handle(handle, out, |h, out| {
                let v: $out_ty = ($expr)(h);
                unsafe {
                    *out = v;
                }
                error::OK
            })
        }
    };
}

scalar_getter!(crimson_save_get_version, u16, |h: &CrimsonSaveHandle| h
    .save
    .header
    .version());
scalar_getter!(crimson_save_get_flags, u16, |h: &CrimsonSaveHandle| h
    .save
    .header
    .flags());
scalar_getter!(
    crimson_save_get_payload_size,
    u32,
    |h: &CrimsonSaveHandle| h.save.header.payload_size()
);
scalar_getter!(
    crimson_save_get_uncompressed_size,
    u32,
    |h: &CrimsonSaveHandle| h.save.header.uncompressed_size()
);
scalar_getter!(
    crimson_save_get_schema_type_count,
    u32,
    |h: &CrimsonSaveHandle| h.body.schema.type_count as u32
);
scalar_getter!(
    crimson_save_get_toc_entry_count,
    u32,
    |h: &CrimsonSaveHandle| h.body.toc.toc_count
);
scalar_getter!(crimson_save_get_block_count, u32, |h: &CrimsonSaveHandle| h
    .blocks
    .len() as u32);

/// HMAC verification result. Writes `1` if verified, `0` otherwise.
///
/// # Safety
/// `handle` must be a live handle. `out` must be a writable `*mut i32`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_get_hmac_ok(
    handle: *const CrimsonSaveHandle,
    out: *mut i32,
) -> i32 {
    with_handle(handle, out, |h, out| {
        unsafe {
            *out = if h.save.hmac_ok { 1 } else { 0 };
        }
        error::OK
    })
}

// ── Block info ─────────────────────────────────────────────────────────────

/// Fill `out` with the summary of the block at TOC `index`.
///
/// # Safety
/// `handle` must be a live handle. `out` must be a writable
/// `*mut CrimsonBlockInfo`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_get_block_info(
    handle: *const CrimsonSaveHandle,
    index: u32,
    out: *mut CrimsonBlockInfo,
) -> i32 {
    with_handle(handle, out, |h, out| {
        let Some(block) = h.blocks.get(index as usize) else {
            return error::OUT_OF_RANGE;
        };
        let (present, decoded) = count_fields(block);
        unsafe {
            *out = CrimsonBlockInfo {
                class_index: block.class_index,
                data_offset: block.data_offset,
                data_size: block.data_size,
                fields_present: present,
                fields_decoded: decoded,
            };
        }
        error::OK
    })
}

/// Write the class name of the block at TOC `index` into `buf` as a
/// NUL-terminated UTF-8 string.
///
/// `out_required` (optional, may be NULL) is always set to the buffer
/// size needed including the NUL terminator.
///
/// Return value:
/// - `OK` when the name fit and `buf` holds a valid C string.
/// - `BUFFER_TOO_SMALL` when the buffer is too small. Caller should
///   read `*out_required`, reallocate, and call again.
/// - other negative codes for invalid arguments / out-of-range index /
///   panic.
///
/// # Safety
/// `handle` must be a live handle. If `buf` is non-NULL it must be
/// writable for at least `buf_len` bytes. If `out_required` is non-NULL
/// it must be a writable `*mut usize`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_get_block_class_name(
    handle: *const CrimsonSaveHandle,
    index: u32,
    buf: *mut u8,
    buf_len: usize,
    out_required: *mut usize,
) -> i32 {
    if handle.is_null() {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let _ffi_guard = save_ffi_lock();
        let h = unsafe { &*handle };
        let Some(block) = h.blocks.get(index as usize) else {
            return error::OUT_OF_RANGE;
        };
        let name = block.class_name.as_bytes();
        let required = name.len() + 1;
        if !out_required.is_null() {
            unsafe {
                *out_required = required;
            }
        }
        if buf.is_null() || buf_len < required {
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

/// Serialize the full per-field decode of the block at TOC `index` as a
/// JSON document. Used by the UI to populate the field-detail pane.
///
/// Shape:
/// ```json
/// {
///   "class_index": u32,
///   "data_offset": u32,
///   "data_size":   u32,
///   "mask_bytes_hex":    "hex",   // empty when mask is empty
///   "trailing_pad_hex":  "hex",   // empty when no trailing pad
///   "fields": [
///     { "field_index", "name", "type_name",
///       "meta_kind", "meta_size", "meta_aux",
///       "present", "kind", "value", "start", "end", "note" }
///   ],
///   "undecoded_ranges": [[start, end], ...]
/// }
/// ```
///
/// `value` is a pre-formatted human string mirroring
/// `tools/inspect/inspect_save_section.py --pretty`. Empty for fields
/// whose `present` is false (or `kind` is `absent`).
///
/// Uses the standard two-call pattern: pass `buf=NULL, buf_len=0` to
/// learn the required size (including NUL), then allocate and call
/// again. `out_required` is always populated when non-NULL.
///
/// # Safety
/// `handle` must be a live handle. If `buf` is non-NULL it must be
/// writable for at least `buf_len` bytes. If `out_required` is non-NULL
/// it must be a writable `*mut usize`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_get_block_json(
    handle: *const CrimsonSaveHandle,
    index: u32,
    buf: *mut u8,
    buf_len: usize,
    out_required: *mut usize,
) -> i32 {
    if handle.is_null() {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let _ffi_guard = save_ffi_lock();
        let h = unsafe { &*handle };
        let Some(block) = h.blocks.get(index as usize) else {
            return error::OUT_OF_RANGE;
        };
        let json = format_block_json(block);
        let bytes = json.as_bytes();
        let required = bytes.len() + 1;
        if !out_required.is_null() {
            unsafe {
                *out_required = required;
            }
        }
        if buf.is_null() || buf_len < required {
            return error::BUFFER_TOO_SMALL;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
            *buf.add(bytes.len()) = 0;
        }
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

// ── Mutation ───────────────────────────────────────────────────────────────

/// Resolve `(block_idx, path, field_idx)` to the leaf scalar's byte range
/// in the body buffer, validating every navigation step.
///
/// On success returns `(start, end)` such that `start..end` is exactly the
/// slice to overwrite. Pure read of `blocks`; the caller does the write.
///
/// Shared between the single-op setters and the batch entry point so the
/// three surfaces stay in lockstep — same path semantics, same error
/// codes, same `bytes_len` invariant.
fn resolve_leaf_range(
    blocks: &[ObjectBlock],
    body_len: usize,
    block_idx: u32,
    path: &[CrimsonPathStep],
    field_idx: u32,
    bytes_len: usize,
) -> Result<(usize, usize), i32> {
    let Some(top) = blocks.get(block_idx as usize) else {
        return Err(error::OUT_OF_RANGE);
    };
    let mut current: &ObjectBlock = top;
    for step in path {
        let Some(field) = current.fields.get(step.field_idx as usize) else {
            return Err(error::OUT_OF_RANGE);
        };
        current = match &field.value {
            FieldValue::Locator {
                child: Some(child), ..
            } => child.as_ref(),
            FieldValue::ObjectList { elements, .. } => {
                let Some(el) = elements.get(step.element_idx as usize) else {
                    return Err(error::OUT_OF_RANGE);
                };
                el
            }
            _ => return Err(error::NOT_NAVIGABLE),
        };
    }
    let Some(leaf) = current.fields.get(field_idx as usize) else {
        return Err(error::OUT_OF_RANGE);
    };
    if !matches!(leaf.kind, FieldKind::FixedPrefix | FieldKind::FixedSuffix) {
        return Err(error::NOT_SCALAR);
    }
    let expected = leaf.end.saturating_sub(leaf.start);
    if bytes_len != expected {
        return Err(error::LENGTH_MISMATCH);
    }
    if leaf.end > body_len {
        // Defensive guard: decoder produced offsets into the same body
        // buffer it parsed, so leaf.end > body_len should be unreachable.
        return Err(error::OUT_OF_RANGE);
    }
    Ok((leaf.start, leaf.end))
}

/// Mutable counterpart to [`resolve_leaf_range`] used by the length-
/// changing edit surface (Phase B.2).
///
/// Navigates `(block_idx, path[])` and returns a `&mut DecodedField` at
/// `field_idx` in the deepest reachable block. Same path semantics +
/// error codes as `resolve_leaf_range`, but produces a mutable handle
/// so the caller can swap the field's `FieldValue` / kind / mask.
fn navigate_mut_to_field<'a>(
    blocks: &'a mut [ObjectBlock],
    block_idx: u32,
    path: &[CrimsonPathStep],
    field_idx: u32,
) -> Result<&'a mut DecodedField, i32> {
    let parent = navigate_mut_to_parent(blocks, block_idx, path)?;
    parent
        .fields
        .get_mut(field_idx as usize)
        .ok_or(error::OUT_OF_RANGE)
}

/// Walk `path[]` from the top-level block at `block_idx`, returning a
/// `&mut ObjectBlock` to the deepest block the path reaches. Path steps
/// can descend through `Locator { child: Some(_) }` or `ObjectList`
/// elements; anything else fails with `NOT_NAVIGABLE`.
fn navigate_mut_to_parent<'a>(
    blocks: &'a mut [ObjectBlock],
    block_idx: u32,
    path: &[CrimsonPathStep],
) -> Result<&'a mut ObjectBlock, i32> {
    let mut current = blocks
        .get_mut(block_idx as usize)
        .ok_or(error::OUT_OF_RANGE)?;
    for step in path {
        let field = current
            .fields
            .get_mut(step.field_idx as usize)
            .ok_or(error::OUT_OF_RANGE)?;
        current = match &mut field.value {
            FieldValue::Locator { child: Some(child), .. } => child.as_mut(),
            FieldValue::ObjectList { elements, .. } => elements
                .get_mut(step.element_idx as usize)
                .ok_or(error::OUT_OF_RANGE)?,
            _ => return Err(error::NOT_NAVIGABLE),
        };
    }
    Ok(current)
}

/// Apply a closure that mutates the decoded block tree, then re-emit
/// the body via [`Body::write`], replace the cached `save.body`, and
/// re-parse + re-decode so subsequent reads see the new layout.
///
/// On any error (including encode / re-parse failures) the handle is
/// left fully untouched — the closure mutates `h.blocks` in place, but
/// if the re-emit fails we restore the original blocks from the
/// pre-mutation snapshot.
///
/// **Deferred-redecode fast path**: when a
/// [`crimson_save_begin_deferred_redecode`] batch is open, the
/// per-call encode + re-parse + decode_blocks tail is skipped — the
/// mutator just modifies `h.blocks` in place. The cost lands once in
/// the matching `crimson_save_end_deferred_redecode`. A mutator that
/// errors out mid-batch leaves the tree partially mutated; the caller
/// is expected to call `crimson_save_abort_deferred_redecode` to roll
/// back to the snapshot captured by `begin_*`.
fn apply_length_changing_mutation<F>(h: &mut CrimsonSaveHandle, mutator: F) -> i32
where
    F: FnOnce(&mut Vec<ObjectBlock>) -> Result<(), i32>,
{
    if h.is_deferred() {
        // Deferred path: the begin_* snapshot already captured the
        // pre-batch state, so we don't pay a per-op clone. A failed
        // mutator leaves `h.blocks` partially mutated; abort_* will
        // restore. Caller's contract.
        return match mutator(&mut h.blocks) {
            Ok(()) => error::OK,
            Err(code) => code,
        };
    }

    // Snapshot for rollback. Cloning the blocks tree is O(N); acceptable
    // for the user-facing edit cadence (single-digit edits per second
    // tops), and saves us from leaving the handle in a half-baked state
    // if `Body::write` or `Body::parse` fails on bytes we produced.
    let blocks_backup = h.blocks.clone();

    if let Err(code) = mutator(&mut h.blocks) {
        // Mutator already gave up; restore and return.
        h.blocks = blocks_backup;
        return code;
    }

    let new_body = match h.body.write(&h.save.body, &h.blocks) {
        Ok(b) => b,
        Err(_) => {
            h.blocks = blocks_backup;
            return error::MUTATION_INVALID;
        }
    };
    let new_body_parsed = match Body::parse(&new_body) {
        Ok(b) => b,
        Err(_) => {
            h.blocks = blocks_backup;
            return error::MUTATION_INVALID;
        }
    };
    let new_blocks = new_body_parsed.decode_blocks(&new_body);

    h.save.body = new_body;
    h.body = new_body_parsed;
    h.blocks = new_blocks;
    h.bump_version();
    error::OK
}

/// Update a fixed-size scalar field's `ScalarValue` inside the in-memory
/// `blocks` tree, no byte-level patch + no `decode_blocks` refresh.
///
/// Used by the deferred-redecode path on scalar mutations: validates the
/// same way `resolve_leaf_range` does, then converts `src` bytes into a
/// `ScalarValue` via [`scalar_from_bytes`] and replaces the field's
/// `value`. The encoder at `end_deferred_redecode` time reads the
/// `ScalarValue` back, so the change persists across the batch commit.
fn apply_scalar_mutation_in_blocks(
    blocks: &mut [ObjectBlock],
    block_idx: u32,
    path: &[CrimsonPathStep],
    field_idx: u32,
    src: &[u8],
) -> Result<(), i32> {
    let field = navigate_mut_to_field(blocks, block_idx, path, field_idx)?;
    if !matches!(field.kind, FieldKind::FixedPrefix | FieldKind::FixedSuffix) {
        return Err(error::NOT_SCALAR);
    }
    let expected = field.end.saturating_sub(field.start);
    if src.len() != expected {
        return Err(error::LENGTH_MISMATCH);
    }
    let new_value = scalar_from_bytes(src, &field.type_name, field.meta_size as usize);
    field.value = FieldValue::Scalar(new_value);
    Ok(())
}

/// Rewrite the count bytes of an `object_list` variant header in place.
///
/// Fixed-size variants locate the count as
/// `(header_bytes.len() - fixed_size) + variant_offset`, where
/// `header_bytes.len() - fixed_size` is the heuristic-skip padding the
/// decoder captured at the front. `marker_run_plus_zeros` has a
/// variable-length leading `01` run so that rule can't place its count;
/// its header is fixed at the TAIL instead (see below).
fn update_object_list_count_in_header(
    header_bytes: &mut [u8],
    header_variant: &str,
    new_count: u32,
) -> Result<(), i32> {
    enum Endian {
        LeU24,
        LeU32,
        BeU16,
    }
    // `marker_run_plus_zeros` carries a variable-length leading run of
    // `01` marker bytes, so the `pad + fixed_offset` rule used below
    // can't locate its count. The header is fixed at the tail instead —
    // `[01 …][00][u32 count LE][13 zero bytes]` (the decoder records
    // `header_size = run + 1 + 4 + 13`) — so the count is always the u32
    // sitting 17 bytes before the end of `header_bytes`, independent of
    // the run length and the decoder's 0..=3 probe pad. Patch it there.
    if header_variant == "marker_run_plus_zeros" {
        let len = header_bytes.len();
        if len < 17 {
            return Err(error::OUT_OF_RANGE);
        }
        let off = len - 17;
        header_bytes[off..off + 4].copy_from_slice(&new_count.to_le_bytes());
        return Ok(());
    }
    // (variant_name, fixed_header_size, count_offset_from_body_cursor,
    //  count_endian)
    let (fixed_size, count_offset, count_endian) = match header_variant {
        "zero1_count_u24" => (18usize, 1usize, Endian::LeU24),
        "zero4_count_u32" => (18, 4, Endian::LeU32),
        "ones_then_count" => (21, 4, Endian::LeU32),
        "one_count_u16be" => (19, 1, Endian::BeU16),
        _ => return Err(error::LIST_VARIANT_UNSUPPORTED),
    };
    if header_bytes.len() < fixed_size {
        return Err(error::OUT_OF_RANGE);
    }
    let pad_len = header_bytes.len() - fixed_size;
    let off = pad_len + count_offset;
    match count_endian {
        Endian::LeU24 => {
            if new_count > 0xFF_FFFF {
                return Err(error::OUT_OF_RANGE);
            }
            header_bytes[off] = (new_count & 0xFF) as u8;
            header_bytes[off + 1] = ((new_count >> 8) & 0xFF) as u8;
            header_bytes[off + 2] = ((new_count >> 16) & 0xFF) as u8;
        }
        Endian::LeU32 => {
            header_bytes[off..off + 4].copy_from_slice(&new_count.to_le_bytes());
        }
        Endian::BeU16 => {
            if new_count > 0xFFFF {
                return Err(error::OUT_OF_RANGE);
            }
            let bytes = (new_count as u16).to_be_bytes();
            header_bytes[off..off + 2].copy_from_slice(&bytes);
        }
    }
    Ok(())
}

/// Decide whether a (present) scalar field at `field_idx` should be
/// emitted as `FixedPrefix` (forward pass) or `FixedSuffix` (reverse
/// pass) — replicates the decoder's reverse-pass rule.
///
/// Reverse pass walks fields backward from the end, peeling present
/// scalars as `FixedSuffix` until it hits a present non-scalar field;
/// at that point it stops. So a scalar at index `field_idx` is
/// `FixedSuffix` iff every higher-index field is either absent or
/// (also peelable, i.e. a present scalar). Equivalently: iff
/// `field_idx > last_index_of_present_non_scalar`.
fn classify_scalar_after_mask_toggle(parent: &ObjectBlock, field_idx: usize) -> FieldKind {
    let mut last_non_scalar: i64 = -1;
    for (i, f) in parent.fields.iter().enumerate() {
        if i == field_idx {
            // The toggled field is scalar (caller validates); it doesn't
            // count as a non-scalar boundary.
            continue;
        }
        if f.present && !matches!(f.meta_kind, 0 | 2) {
            last_non_scalar = i as i64;
        }
    }
    if (field_idx as i64) > last_non_scalar {
        FieldKind::FixedSuffix
    } else {
        FieldKind::FixedPrefix
    }
}

/// Overwrite the bytes of a fixed-size scalar field with `bytes`.
///
/// Constraints:
/// - `(block_idx, field_idx)` must resolve to a field whose
///   [`FieldKind`] is `FixedPrefix` or `FixedSuffix` — list, inline-byte,
///   locator, and absent fields are rejected with `NOT_SCALAR`. The
///   set is a same-size byte-level replacement.
/// - `bytes_len` must equal the field's recorded byte range
///   (`field.end - field.start`); otherwise `LENGTH_MISMATCH`.
///
/// On success the cached body bytes are patched in place and **every**
/// block is re-decoded so subsequent
/// [`crimson_save_get_block_json`] / `_info` calls see the new value.
/// The re-decode is O(toc_count) but completes in milliseconds on the
/// 1112-block save.
///
/// # Safety
/// `handle` must be a live, exclusive handle. `bytes` must be a valid
/// pointer to `bytes_len` readable bytes for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_set_scalar_field(
    handle: *mut CrimsonSaveHandle,
    block_idx: u32,
    field_idx: u32,
    bytes: *const u8,
    bytes_len: usize,
) -> i32 {
    if handle.is_null() {
        return error::NULL_ARG;
    }
    if bytes.is_null() && bytes_len != 0 {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let _ffi_guard = save_ffi_lock();
        let h = unsafe { &mut *handle };
        let src = unsafe { slice_from_raw_or_empty(bytes, bytes_len) };
        if h.is_deferred() {
            // Deferred path: update the field's ScalarValue in place;
            // the encode at end_deferred_redecode emits the new bytes.
            // No body patch, no decode_blocks, no version bump (the
            // batch's end_* bumps once for the whole transaction).
            return match apply_scalar_mutation_in_blocks(
                &mut h.blocks,
                block_idx,
                &[],
                field_idx,
                src,
            ) {
                Ok(()) => error::OK,
                Err(code) => code,
            };
        }
        let (dst_start, dst_end) =
            match resolve_leaf_range(&h.blocks, h.save.body.len(), block_idx, &[], field_idx, bytes_len) {
                Ok(range) => range,
                Err(code) => return code,
            };
        h.save.body[dst_start..dst_end].copy_from_slice(src);
        // Refresh decoded blocks so consumers see the new value on the
        // next get_block_json. Re-parsing the body is cheap (schema/TOC
        // unchanged); decode_blocks is the only meaningful work.
        h.blocks = h.body.decode_blocks(&h.save.body);
        h.bump_version();
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Path-addressed variant of [`crimson_save_set_scalar_field`].
///
/// Mutates a fixed-size scalar field reachable through a chain of inline
/// children and list elements. The `(block_idx, path[], field_idx)`
/// triple uniquely identifies any decoded scalar in the save tree:
///
/// - `block_idx` picks a top-level TOC block.
/// - `path` is a sequence of [`CrimsonPathStep`] descents from that
///   block. Each step's `field_idx` selects a nested-bearing field of
///   the current block (must resolve to `ObjectLocator` with an inline
///   child, or `ObjectList`). For lists, `element_idx` picks the element.
/// - `field_idx` (the leaf) is the scalar to write inside the block we
///   arrive at after `path_len` descents. With `path_len == 0` this
///   behaves identically to [`crimson_save_set_scalar_field`].
///
/// All other invariants match the top-level setter:
/// - leaf must be `FixedPrefix` / `FixedSuffix` → otherwise `NOT_SCALAR`
/// - `bytes_len` must equal the leaf's recorded byte range → otherwise
///   `LENGTH_MISMATCH`
/// - on success the body is patched in place and every block re-decoded
///
/// Errors:
/// - `OUT_OF_RANGE` for any bad index along the path or at the leaf
/// - `NOT_NAVIGABLE` when a mid-path field isn't a locator-with-child
///   or a list (e.g. a scalar in the middle of the chain)
/// - `NULL_ARG` on null `handle`, null `bytes` with non-zero length, or
///   null `path` with non-zero `path_len`
///
/// # Safety
/// `handle` must be a live, exclusive handle. `path` must point to
/// `path_len` readable [`CrimsonPathStep`] values, and `bytes` to
/// `bytes_len` readable bytes, both for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_set_scalar_field_path(
    handle: *mut CrimsonSaveHandle,
    block_idx: u32,
    path: *const CrimsonPathStep,
    path_len: usize,
    field_idx: u32,
    bytes: *const u8,
    bytes_len: usize,
) -> i32 {
    if handle.is_null() {
        return error::NULL_ARG;
    }
    if bytes.is_null() && bytes_len != 0 {
        return error::NULL_ARG;
    }
    if path.is_null() && path_len != 0 {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let _ffi_guard = save_ffi_lock();
        let h = unsafe { &mut *handle };
        let steps: &[CrimsonPathStep] = if path_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(path, path_len) }
        };
        let src = unsafe { slice_from_raw_or_empty(bytes, bytes_len) };
        if h.is_deferred() {
            return match apply_scalar_mutation_in_blocks(
                &mut h.blocks,
                block_idx,
                steps,
                field_idx,
                src,
            ) {
                Ok(()) => error::OK,
                Err(code) => code,
            };
        }
        let (dst_start, dst_end) =
            match resolve_leaf_range(&h.blocks, h.save.body.len(), block_idx, steps, field_idx, bytes_len) {
                Ok(range) => range,
                Err(code) => return code,
            };
        h.save.body[dst_start..dst_end].copy_from_slice(src);
        h.blocks = h.body.decode_blocks(&h.save.body);
        h.bump_version();
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

// ── Typed composite-scalar setters (ergonomic wrappers) ──────────────
//
// Thin wrappers around `crimson_save_set_scalar_field_path` /
// `crimson_save_set_scalar_field_present` that pack typed values
// (f32 / u32 triples / quadruples) into the LE byte buffer the raw
// setters expect. They emit nothing the raw API can't already produce
// — purely an ergonomic convenience so the C# editor doesn't have to
// hand-pack 12/16-byte buffers for every float3 / float4 / uint4
// edit. Mirrors the typed read side added in 2026-05-17
// (`ScalarValue::F32x3` etc.).
//
// Mutation rule: each composite is atomic — there's no path to make
// individual components absent. Either the whole vector is present
// (with all components carrying values) or the whole vector is absent.
// The `_present` variant flips between those two states.

/// Set the value of an already-present `float3` (12-byte / 3 × f32) field.
///
/// Equivalent to `crimson_save_set_scalar_field_path` with a 12-byte
/// LE payload packed from `(x, y, z)`. Validation rules identical:
/// the leaf field must be a fixed-size scalar of size 12 (`NOT_SCALAR`
/// / `LENGTH_MISMATCH` otherwise).
///
/// # Safety
/// Same as [`crimson_save_set_scalar_field_path`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_set_float3_field_path(
    handle: *mut CrimsonSaveHandle,
    block_idx: u32,
    path: *const CrimsonPathStep,
    path_len: usize,
    field_idx: u32,
    x: f32,
    y: f32,
    z: f32,
) -> i32 {
    let mut buf = [0u8; 12];
    buf[0..4].copy_from_slice(&x.to_le_bytes());
    buf[4..8].copy_from_slice(&y.to_le_bytes());
    buf[8..12].copy_from_slice(&z.to_le_bytes());
    unsafe {
        crimson_save_set_scalar_field_path(
            handle, block_idx, path, path_len, field_idx, buf.as_ptr(), buf.len(),
        )
    }
}

/// Toggle the presence of a `float3` field. When `present_flag != 0`,
/// the field becomes present with the supplied `(x, y, z)` packed into
/// 12 LE bytes; when `present_flag == 0` the field becomes absent and
/// `x` / `y` / `z` are ignored.
///
/// Equivalent to `crimson_save_set_scalar_field_present` with the
/// packed init bytes; see that function for full validation semantics.
///
/// # Safety
/// Same as [`crimson_save_set_scalar_field_present`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_set_float3_field_present(
    handle: *mut CrimsonSaveHandle,
    block_idx: u32,
    path: *const CrimsonPathStep,
    path_len: usize,
    field_idx: u32,
    present_flag: i32,
    x: f32,
    y: f32,
    z: f32,
) -> i32 {
    if present_flag == 0 {
        return unsafe {
            crimson_save_set_scalar_field_present(
                handle,
                block_idx,
                path,
                path_len,
                field_idx,
                0,
                std::ptr::null(),
                0,
            )
        };
    }
    let mut buf = [0u8; 12];
    buf[0..4].copy_from_slice(&x.to_le_bytes());
    buf[4..8].copy_from_slice(&y.to_le_bytes());
    buf[8..12].copy_from_slice(&z.to_le_bytes());
    unsafe {
        crimson_save_set_scalar_field_present(
            handle, block_idx, path, path_len, field_idx, 1, buf.as_ptr(), buf.len(),
        )
    }
}

/// Set the value of an already-present `float4` / `quaternion`
/// (16-byte / 4 × f32) field. See [`crimson_save_set_float3_field_path`].
///
/// # Safety
/// Same as [`crimson_save_set_scalar_field_path`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_set_float4_field_path(
    handle: *mut CrimsonSaveHandle,
    block_idx: u32,
    path: *const CrimsonPathStep,
    path_len: usize,
    field_idx: u32,
    x: f32,
    y: f32,
    z: f32,
    w: f32,
) -> i32 {
    let mut buf = [0u8; 16];
    buf[0..4].copy_from_slice(&x.to_le_bytes());
    buf[4..8].copy_from_slice(&y.to_le_bytes());
    buf[8..12].copy_from_slice(&z.to_le_bytes());
    buf[12..16].copy_from_slice(&w.to_le_bytes());
    unsafe {
        crimson_save_set_scalar_field_path(
            handle, block_idx, path, path_len, field_idx, buf.as_ptr(), buf.len(),
        )
    }
}

/// Toggle the presence of a `float4` / `quaternion` field. See
/// [`crimson_save_set_float3_field_present`] for the presence-flag
/// contract.
///
/// # Safety
/// Same as [`crimson_save_set_scalar_field_present`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_set_float4_field_present(
    handle: *mut CrimsonSaveHandle,
    block_idx: u32,
    path: *const CrimsonPathStep,
    path_len: usize,
    field_idx: u32,
    present_flag: i32,
    x: f32,
    y: f32,
    z: f32,
    w: f32,
) -> i32 {
    if present_flag == 0 {
        return unsafe {
            crimson_save_set_scalar_field_present(
                handle,
                block_idx,
                path,
                path_len,
                field_idx,
                0,
                std::ptr::null(),
                0,
            )
        };
    }
    let mut buf = [0u8; 16];
    buf[0..4].copy_from_slice(&x.to_le_bytes());
    buf[4..8].copy_from_slice(&y.to_le_bytes());
    buf[8..12].copy_from_slice(&z.to_le_bytes());
    buf[12..16].copy_from_slice(&w.to_le_bytes());
    unsafe {
        crimson_save_set_scalar_field_present(
            handle, block_idx, path, path_len, field_idx, 1, buf.as_ptr(), buf.len(),
        )
    }
}

/// Set the value of an already-present `uint4` (16-byte / 4 × u32) field.
/// `uint4` is the on-disk shape of 128-bit IDs like `SceneObjectUuid`.
///
/// # Safety
/// Same as [`crimson_save_set_scalar_field_path`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_set_uint4_field_path(
    handle: *mut CrimsonSaveHandle,
    block_idx: u32,
    path: *const CrimsonPathStep,
    path_len: usize,
    field_idx: u32,
    a: u32,
    b: u32,
    c: u32,
    d: u32,
) -> i32 {
    let mut buf = [0u8; 16];
    buf[0..4].copy_from_slice(&a.to_le_bytes());
    buf[4..8].copy_from_slice(&b.to_le_bytes());
    buf[8..12].copy_from_slice(&c.to_le_bytes());
    buf[12..16].copy_from_slice(&d.to_le_bytes());
    unsafe {
        crimson_save_set_scalar_field_path(
            handle, block_idx, path, path_len, field_idx, buf.as_ptr(), buf.len(),
        )
    }
}

/// Toggle the presence of a `uint4` field. See
/// [`crimson_save_set_float3_field_present`] for the presence-flag contract.
///
/// # Safety
/// Same as [`crimson_save_set_scalar_field_present`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_set_uint4_field_present(
    handle: *mut CrimsonSaveHandle,
    block_idx: u32,
    path: *const CrimsonPathStep,
    path_len: usize,
    field_idx: u32,
    present_flag: i32,
    a: u32,
    b: u32,
    c: u32,
    d: u32,
) -> i32 {
    if present_flag == 0 {
        return unsafe {
            crimson_save_set_scalar_field_present(
                handle,
                block_idx,
                path,
                path_len,
                field_idx,
                0,
                std::ptr::null(),
                0,
            )
        };
    }
    let mut buf = [0u8; 16];
    buf[0..4].copy_from_slice(&a.to_le_bytes());
    buf[4..8].copy_from_slice(&b.to_le_bytes());
    buf[8..12].copy_from_slice(&c.to_le_bytes());
    buf[12..16].copy_from_slice(&d.to_le_bytes());
    unsafe {
        crimson_save_set_scalar_field_present(
            handle, block_idx, path, path_len, field_idx, 1, buf.as_ptr(), buf.len(),
        )
    }
}

/// Apply many scalar mutations in one FFI round trip, sharing a single
/// post-batch re-decode.
///
/// Semantics:
///
/// 1. **Validate everything first.** Each op's `(block_idx, path[],
///    field_idx, bytes_len)` is resolved through the same rules as
///    [`crimson_save_set_scalar_field_path`] (`NOT_SCALAR`,
///    `LENGTH_MISMATCH`, `OUT_OF_RANGE`, `NOT_NAVIGABLE`). On any error
///    the call returns immediately with no mutation applied — the save
///    body is left exactly as it was before the call.
/// 2. **Patch all ops in order.** Identical to running N
///    `crimson_save_set_scalar_field_path` calls sequentially, except no
///    re-decode happens between ops.
/// 3. **One re-decode at the end.** The decoded `blocks` are refreshed
///    a single time after the last patch, regardless of `op_count`.
///
/// This is the high-throughput path for bulk edits — e.g. "fill every
/// item's stack to max", which performs 168 writes today and would
/// otherwise pay 168 × `decode_blocks` cost. Wall-time goes from
/// O(N · block_count) to O(N + block_count).
///
/// `out_failed_op_index` (optional, may be NULL):
/// - On error, written with the index of the op whose validation failed
///   so the caller can pinpoint the offending mutation.
/// - On success, written with `usize::MAX` as a sentinel meaning
///   "no failure". Callers can either ignore the value on `OK` or check
///   it explicitly.
///
/// # Safety
/// `handle` must be a live, exclusive handle. `ops` must point to
/// `op_count` readable [`CrimsonScalarBatchOp`] values for the duration
/// of the call. Each op's `path` and `bytes` pointers must point to
/// `path_len` / `bytes_len` readable bytes respectively, for the
/// duration of the call. `out_failed_op_index` may be NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_set_scalar_fields_batch(
    handle: *mut CrimsonSaveHandle,
    ops: *const CrimsonScalarBatchOp,
    op_count: usize,
    out_failed_op_index: *mut usize,
) -> i32 {
    if handle.is_null() {
        return error::NULL_ARG;
    }
    if ops.is_null() && op_count != 0 {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        // Trivial batch: no work, no re-decode, no failure index to set.
        if op_count == 0 {
            if !out_failed_op_index.is_null() {
                unsafe {
                    *out_failed_op_index = usize::MAX;
                }
            }
            return error::OK;
        }

        let _ffi_guard = save_ffi_lock();
        let h = unsafe { &mut *handle };
        let ops_slice: &[CrimsonScalarBatchOp] =
            unsafe { slice_from_raw_or_empty(ops, op_count) };

        // Per-op NULL_ARG pre-check (path / bytes pointers). Mirrors
        // the single-op setter's invariants without rolling them into
        // resolve_leaf_range, which works on a typed slice.
        for (i, op) in ops_slice.iter().enumerate() {
            if op.path.is_null() && op.path_len != 0 {
                if !out_failed_op_index.is_null() {
                    unsafe {
                        *out_failed_op_index = i;
                    }
                }
                return error::NULL_ARG;
            }
            if op.bytes.is_null() && op.bytes_len != 0 {
                if !out_failed_op_index.is_null() {
                    unsafe {
                        *out_failed_op_index = i;
                    }
                }
                return error::NULL_ARG;
            }
        }

        // Deferred path: each op updates the in-memory ScalarValue in
        // turn; no body patch, no decode_blocks. Validation is per-op
        // (same code as the single-op setter); the first failing op
        // reports its index and leaves earlier ops applied — the
        // batch's abort_* call will roll back if the caller wants
        // all-or-nothing semantics.
        if h.is_deferred() {
            for (i, op) in ops_slice.iter().enumerate() {
                let steps: &[CrimsonPathStep] = if op.path_len == 0 {
                    &[]
                } else {
                    unsafe { std::slice::from_raw_parts(op.path, op.path_len) }
                };
                let src = unsafe { slice_from_raw_or_empty(op.bytes, op.bytes_len) };
                if let Err(code) = apply_scalar_mutation_in_blocks(
                    &mut h.blocks,
                    op.block_idx,
                    steps,
                    op.field_idx,
                    src,
                ) {
                    if !out_failed_op_index.is_null() {
                        unsafe {
                            *out_failed_op_index = i;
                        }
                    }
                    return code;
                }
            }
            if !out_failed_op_index.is_null() {
                unsafe {
                    *out_failed_op_index = usize::MAX;
                }
            }
            return error::OK;
        }

        // Phase 1 — validate every op against the (immutable) decoded
        // tree. Collect each op's resolved byte range. On the first
        // failure we abort without writing anything.
        let mut ranges: Vec<(usize, usize)> = Vec::with_capacity(op_count);
        for (i, op) in ops_slice.iter().enumerate() {
            let steps: &[CrimsonPathStep] = if op.path_len == 0 {
                &[]
            } else {
                unsafe { std::slice::from_raw_parts(op.path, op.path_len) }
            };
            match resolve_leaf_range(
                &h.blocks,
                h.save.body.len(),
                op.block_idx,
                steps,
                op.field_idx,
                op.bytes_len,
            ) {
                Ok(range) => ranges.push(range),
                Err(code) => {
                    if !out_failed_op_index.is_null() {
                        unsafe {
                            *out_failed_op_index = i;
                        }
                    }
                    return code;
                }
            }
        }

        // Phase 2 — apply every patch in input order. Pure memcpy over
        // already-validated ranges; no validation errors possible here.
        // Note: overlapping ranges between ops are not detected. Last
        // write wins, exactly as if the caller had run N sequential
        // single-op setters.
        for (op, (dst_start, dst_end)) in ops_slice.iter().zip(ranges) {
            let src = unsafe { slice_from_raw_or_empty(op.bytes, op.bytes_len) };
            h.save.body[dst_start..dst_end].copy_from_slice(src);
        }

        // Phase 3 — one re-decode covers all mutations.
        h.blocks = h.body.decode_blocks(&h.save.body);

        if !out_failed_op_index.is_null() {
            unsafe {
                *out_failed_op_index = usize::MAX;
            }
        }
        h.bump_version();
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

// ── Length-changing edits (Phase B.2) ──────────────────────────────────────

/// Remove element `element_idx` from the `object_list` field reached by
/// `(block_idx, path[], field_idx)`. The list's `count` is decremented,
/// the variant header's count bytes are rewritten, and the body is
/// re-encoded + re-parsed so subsequent reads see the new layout.
///
/// Validation:
/// - The leaf field must be `FieldKind::ObjectList`; else `NOT_SCALAR`
///   (overloaded — "not a list either"; the existing code maps non-list
///   leaves to this).
/// - `element_idx` must be `< current count`; else `OUT_OF_RANGE`.
/// - The list's `header_variant` must be one whose count we can patch:
///   `zero1_count_u24`, `zero4_count_u32`, `ones_then_count`,
///   `one_count_u16be` (fixed-size headers) or `marker_run_plus_zeros`
///   (variable-length `01` run, but its count is a fixed u32 sitting 17
///   bytes from the header's end); else `LIST_VARIANT_UNSUPPORTED`.
///
/// On success the in-memory body is fully replaced and the cached
/// decoded blocks are refreshed. On any error the handle is left
/// untouched.
///
/// # Safety
/// `handle` must be a live, exclusive handle. `path` must point to
/// `path_len` readable [`CrimsonPathStep`] values for the duration of
/// the call (or be NULL with `path_len == 0`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_list_remove_element(
    handle: *mut CrimsonSaveHandle,
    block_idx: u32,
    path: *const CrimsonPathStep,
    path_len: usize,
    field_idx: u32,
    element_idx: u32,
) -> i32 {
    if handle.is_null() {
        return error::NULL_ARG;
    }
    if path.is_null() && path_len != 0 {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let _ffi_guard = save_ffi_lock();
        let h = unsafe { &mut *handle };
        let steps: &[CrimsonPathStep] = if path_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(path, path_len) }
        };
        apply_length_changing_mutation(h, |blocks| {
            let field = navigate_mut_to_field(blocks, block_idx, steps, field_idx)?;
            let FieldValue::ObjectList {
                count,
                header_variant,
                header_bytes,
                elements,
            } = &mut field.value
            else {
                return Err(error::NOT_SCALAR);
            };
            if (element_idx as usize) >= elements.len() {
                return Err(error::OUT_OF_RANGE);
            }
            elements.remove(element_idx as usize);
            *count = elements.len() as u32;
            update_object_list_count_in_header(header_bytes, header_variant, *count)?;
            Ok(())
        })
    }))
    .unwrap_or(error::PANIC)
}

/// Drop many `object_list` elements in one FFI round trip, sharing a
/// single post-batch re-emit + re-decode. Each op describes one removal
/// in the same shape as [`crimson_save_list_remove_element`].
///
/// Semantics:
/// 1. **Apply ops in input order.** Each op is validated and applied
///    against the freshly-mutated tree, exactly as if the caller had
///    run N sequential single-op removes. Earlier removes shift later
///    indexes in the same list, so the caller is expected to pre-sort
///    ops targeting the same list by descending `element_idx`.
/// 2. **All-or-nothing on failure.** On the first op that fails any
///    of the same checks as the single-op API (`NOT_SCALAR` /
///    `OUT_OF_RANGE` / `LIST_VARIANT_UNSUPPORTED` / `NOT_NAVIGABLE`),
///    the in-memory body is rolled back to its pre-batch state and
///    the call returns that op's index via `out_failed_op_index`.
/// 3. **One re-emit + re-decode at the end.** The body is serialized
///    via [`Body::write`] exactly once after the last successful
///    removal — a single cost regardless of `op_count`. This is the
///    high-throughput path for bulk drops (e.g. trimming dozens of
///    quest-reward items in one Tools-menu action).
///
/// `out_failed_op_index` (optional, may be NULL):
/// - On error, written with the index of the failing op.
/// - On success, written with `usize::MAX` as a "no failure" sentinel.
///
/// # Safety
/// `handle` must be a live, exclusive handle. `ops` must point to
/// `op_count` readable [`CrimsonListRemoveBatchOp`] values for the
/// duration of the call. Each op's `path` pointer must point to
/// `path_len` readable [`CrimsonPathStep`] values (or be NULL with
/// `path_len == 0`). `out_failed_op_index` may be NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_list_remove_elements_batch(
    handle: *mut CrimsonSaveHandle,
    ops: *const CrimsonListRemoveBatchOp,
    op_count: usize,
    out_failed_op_index: *mut usize,
) -> i32 {
    if handle.is_null() {
        return error::NULL_ARG;
    }
    if ops.is_null() && op_count != 0 {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        if op_count == 0 {
            if !out_failed_op_index.is_null() {
                unsafe {
                    *out_failed_op_index = usize::MAX;
                }
            }
            return error::OK;
        }

        let ops_slice: &[CrimsonListRemoveBatchOp] =
            unsafe { slice_from_raw_or_empty(ops, op_count) };

        // Per-op NULL_ARG pre-check (path pointer). Mirrors the
        // single-op API's invariants before we begin mutating.
        for (i, op) in ops_slice.iter().enumerate() {
            if op.path.is_null() && op.path_len != 0 {
                if !out_failed_op_index.is_null() {
                    unsafe {
                        *out_failed_op_index = i;
                    }
                }
                return error::NULL_ARG;
            }
        }

        let _ffi_guard = save_ffi_lock();
        let h = unsafe { &mut *handle };
        let mut failed_op: Option<usize> = None;
        let rc = apply_length_changing_mutation(h, |blocks| {
            for (i, op) in ops_slice.iter().enumerate() {
                let steps: &[CrimsonPathStep] = if op.path_len == 0 {
                    &[]
                } else {
                    unsafe { std::slice::from_raw_parts(op.path, op.path_len) }
                };
                if let Err(code) =
                    remove_one_list_element_in_place(blocks, op.block_idx, steps, op.field_idx, op.element_idx)
                {
                    failed_op = Some(i);
                    return Err(code);
                }
            }
            Ok(())
        });

        if !out_failed_op_index.is_null() {
            unsafe {
                *out_failed_op_index = failed_op.unwrap_or(usize::MAX);
            }
        }
        rc
    }))
    .unwrap_or(error::PANIC)
}

/// Shared mutator for both the single-op and batch list-remove entry
/// points. Resolves the list field, bounds-checks `element_idx`, drops
/// the element, and rewrites the variant header's count.
fn remove_one_list_element_in_place(
    blocks: &mut [ObjectBlock],
    block_idx: u32,
    path: &[CrimsonPathStep],
    field_idx: u32,
    element_idx: u32,
) -> Result<(), i32> {
    let field = navigate_mut_to_field(blocks, block_idx, path, field_idx)?;
    let FieldValue::ObjectList {
        count,
        header_variant,
        header_bytes,
        elements,
    } = &mut field.value
    else {
        return Err(error::NOT_SCALAR);
    };
    if (element_idx as usize) >= elements.len() {
        return Err(error::OUT_OF_RANGE);
    }
    elements.remove(element_idx as usize);
    *count = elements.len() as u32;
    update_object_list_count_in_header(header_bytes, header_variant, *count)
}

/// Clone element `src_element_idx` of an `object_list` and insert the
/// copy at `dst_element_idx`. The list's `count` is incremented and the
/// variant header's count bytes are rewritten.
///
/// `dst_element_idx` must be in `0..=new_count` (i.e. `0..=count + 1`
/// after the clone). `dst_element_idx == count + 1` would be illegal
/// — pass `dst_element_idx == count` to append.
///
/// After cloning, the new element is byte-identical to the source.
/// Callers typically follow up with
/// [`crimson_save_set_scalar_field_path`] to patch fields (`_itemKey`,
/// `_stackCount`, etc.) so the clone represents a distinct entity.
///
/// Validation:
/// - The leaf field must be `FieldKind::ObjectList`; else `NOT_SCALAR`.
/// - `src_element_idx` must be `< current count`; else `OUT_OF_RANGE`.
/// - `dst_element_idx` must be `<= count + 1`; else `OUT_OF_RANGE`.
///   (`<= count` is also OK; the comparison uses the post-clone count.)
/// - The list's `header_variant` must be count-patchable — a fixed-size
///   variant or `marker_run_plus_zeros`; else `LIST_VARIANT_UNSUPPORTED`.
///
/// # Safety
/// `handle` must be a live, exclusive handle. `path` must point to
/// `path_len` readable [`CrimsonPathStep`] values for the duration of
/// the call (or be NULL with `path_len == 0`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_list_clone_element(
    handle: *mut CrimsonSaveHandle,
    block_idx: u32,
    path: *const CrimsonPathStep,
    path_len: usize,
    field_idx: u32,
    src_element_idx: u32,
    dst_element_idx: u32,
) -> i32 {
    if handle.is_null() {
        return error::NULL_ARG;
    }
    if path.is_null() && path_len != 0 {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let _ffi_guard = save_ffi_lock();
        let h = unsafe { &mut *handle };
        let steps: &[CrimsonPathStep] = if path_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(path, path_len) }
        };
        apply_length_changing_mutation(h, |blocks| {
            let field = navigate_mut_to_field(blocks, block_idx, steps, field_idx)?;
            let FieldValue::ObjectList {
                count,
                header_variant,
                header_bytes,
                elements,
            } = &mut field.value
            else {
                return Err(error::NOT_SCALAR);
            };
            if (src_element_idx as usize) >= elements.len() {
                return Err(error::OUT_OF_RANGE);
            }
            // Post-clone count is elements.len() + 1; dst must be <= that.
            if (dst_element_idx as usize) > elements.len() {
                return Err(error::OUT_OF_RANGE);
            }
            let cloned = elements[src_element_idx as usize].clone();
            elements.insert(dst_element_idx as usize, cloned);
            *count = elements.len() as u32;
            update_object_list_count_in_header(header_bytes, header_variant, *count)?;
            Ok(())
        })
    }))
    .unwrap_or(error::PANIC)
}

/// Recursively retarget every embedded schema type-index in a decoded
/// element so it is valid under `target_name_to_index` (the TARGET save's
/// `class name -> schema index` map). The decoded tree carries class
/// NAMES verbatim — `ObjectBlock::class_name` (also the locator-wrapper's
/// type) and `FieldValue::Locator::child_type_name` — so the remap is
/// purely name-keyed and needs no source schema. Returns the first class
/// name the target schema doesn't define.
fn remap_block_type_indices(
    block: &mut ObjectBlock,
    target_name_to_index: &std::collections::HashMap<String, u32>,
) -> Result<(), String> {
    let idx = *target_name_to_index
        .get(block.class_name.as_str())
        .ok_or_else(|| block.class_name.clone())?;
    block.class_index = idx;
    if let Some(w) = block.locator_wrapper.as_mut() {
        // A list element's / inline child's wrapper type IS the block class.
        w.type_index = idx as u16;
    }
    for field in &mut block.fields {
        match &mut field.value {
            FieldValue::Locator {
                child_type_index,
                child_type_name,
                child,
                ..
            } => {
                let ci = *target_name_to_index
                    .get(child_type_name.as_str())
                    .ok_or_else(|| child_type_name.clone())?;
                *child_type_index = ci as u16;
                if let Some(c) = child.as_mut() {
                    remap_block_type_indices(c, target_name_to_index)?;
                }
            }
            FieldValue::ObjectList { elements, .. } => {
                for el in elements.iter_mut() {
                    remap_block_type_indices(el, target_name_to_index)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Transplant one `object_list` element from a SOURCE save into a TARGET
/// save's list, retargeting the element's embedded schema type-indices
/// from the source's numbering to the target's (keyed by class name).
///
/// Cross-save counterpart to [`crimson_save_list_clone_element`] (which
/// duplicates within one save): lifts e.g. a fully-formed mount element
/// out of a save that owns it and grafts it into a save that doesn't. The
/// element's scalar values (charKey, level, spawn data, …) copy verbatim;
/// only type-indices are retargeted. Callers typically follow with
/// [`crimson_save_set_scalar_field_path`] to make instance-unique fields
/// distinct (e.g. `_mercenaryNo`).
///
/// Both saves must share the same field DEFINITIONS for every class the
/// element references (same game version) — only the per-save type-index
/// numbering may differ. On a definition mismatch the post-insert
/// re-decode rejects the bytes with `MUTATION_INVALID` and the target is
/// rolled back.
///
/// Errors: `NULL_ARG`, `OUT_OF_RANGE` (bad block/path/element idx, or
/// `insert_at > count`), `NOT_OBJECT_LIST` (source/target field isn't a
/// list), `TRANSPLANT_TYPE_MISSING` (target schema lacks a needed class),
/// `LIST_VARIANT_UNSUPPORTED`, `MUTATION_INVALID`.
///
/// # Safety
/// `target_handle` and `source_handle` must be live and **distinct**
/// handles (aliasing one save as both is UB). Each `*_path` must point to
/// `*_path_len` readable [`CrimsonPathStep`]s (or NULL with len 0).
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn crimson_save_transplant_list_element(
    target_handle: *mut CrimsonSaveHandle,
    target_block_idx: u32,
    target_path: *const CrimsonPathStep,
    target_path_len: usize,
    target_field_idx: u32,
    insert_at: u32,
    source_handle: *const CrimsonSaveHandle,
    source_block_idx: u32,
    source_path: *const CrimsonPathStep,
    source_path_len: usize,
    source_field_idx: u32,
    source_element_idx: u32,
) -> i32 {
    if target_handle.is_null() || source_handle.is_null() {
        return error::NULL_ARG;
    }
    if (target_path.is_null() && target_path_len != 0)
        || (source_path.is_null() && source_path_len != 0)
    {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        // One global lock covers both handles — no two-lock ordering /
        // deadlock concern, and the C# wrapper already rejects target ==
        // source so the `&mut`/`&` below never alias the same save.
        let _ffi_guard = save_ffi_lock();
        let target = unsafe { &mut *target_handle };
        let source = unsafe { &*source_handle };
        let t_steps: &[CrimsonPathStep] = if target_path_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(target_path, target_path_len) }
        };
        let s_steps: &[CrimsonPathStep] = if source_path_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(source_path, source_path_len) }
        };

        // 1) Lift + clone the source element (read-only).
        let src_parent = match navigate_to_parent_ref(&source.blocks, source_block_idx, s_steps) {
            Ok(p) => p,
            Err(e) => return e,
        };
        let Some(src_field) = src_parent.fields.get(source_field_idx as usize) else {
            return error::OUT_OF_RANGE;
        };
        let FieldValue::ObjectList { elements: src_elems, .. } = &src_field.value else {
            return error::NOT_OBJECT_LIST;
        };
        let Some(src_el) = src_elems.get(source_element_idx as usize) else {
            return error::OUT_OF_RANGE;
        };
        let mut transplanted = src_el.clone();

        // 2) Retarget type-indices to the target schema (owned-key map so
        //    it doesn't borrow `target` across the &mut mutation below).
        let target_map: std::collections::HashMap<String, u32> = target
            .body
            .schema
            .types
            .iter()
            .map(|t| (t.name.clone(), t.index))
            .collect();
        if remap_block_type_indices(&mut transplanted, &target_map).is_err() {
            return error::TRANSPLANT_TYPE_MISSING;
        }

        // 3) Splice into the target list. The re-encode + re-decode inside
        //    apply_length_changing_mutation validates the grafted bytes;
        //    a definition mismatch surfaces as MUTATION_INVALID + rollback.
        apply_length_changing_mutation(target, move |blocks| {
            let field =
                navigate_mut_to_field(blocks, target_block_idx, t_steps, target_field_idx)?;
            let FieldValue::ObjectList {
                count,
                header_variant,
                header_bytes,
                elements,
            } = &mut field.value
            else {
                return Err(error::NOT_OBJECT_LIST);
            };
            if insert_at as usize > elements.len() {
                return Err(error::OUT_OF_RANGE);
            }
            elements.insert(insert_at as usize, transplanted);
            *count = elements.len() as u32;
            update_object_list_count_in_header(header_bytes, header_variant, *count)?;
            Ok(())
        })
    }))
    .unwrap_or(error::PANIC)
}

/// Flip the mask bit of a fixed-size scalar field, inserting (or
/// removing) the corresponding bytes in the enclosing block's payload.
///
/// `present_flag == 1` makes the field present:
/// - The mask bit at `field_idx` in the enclosing block is set.
/// - `init_bytes` (length `init_len`) must equal the field's
///   `meta_size` and is decoded into a `ScalarValue` per the schema's
///   `type_name` heuristic (`bool`, `u8..u64`, `i8..i64`, `f32`, `f64`,
///   or raw `bytes`).
/// - The field is classified as `FixedPrefix` or `FixedSuffix` per the
///   decoder's reverse-pass rule (suffix iff no present non-scalar
///   field exists at a higher index).
///
/// `present_flag == 0` makes the field absent:
/// - The mask bit is cleared.
/// - The field's `kind` becomes `Absent`, its `value` becomes `None`.
/// - `init_bytes` is ignored (pass NULL + 0).
///
/// Validation:
/// - The field's schema `meta_kind` must be `0` or `2` (fixed scalar);
///   else `NOT_SCALAR_FIELD_KIND`. Toggling list / locator / inline
///   field presence requires the template-builder ABI (Phase B.3).
/// - When `present_flag == 1`, `init_len` must equal the field's
///   `meta_size`; else `LENGTH_MISMATCH`.
/// - When `present_flag == 0` AND the field is already absent (or
///   `present_flag == 1` AND already present), the call is a no-op
///   that still re-emits the body. Cheap, but the caller can short-
///   circuit by checking `crimson_save_get_block_json` first.
///
/// # Safety
/// `handle` must be a live, exclusive handle. `path` must point to
/// `path_len` readable [`CrimsonPathStep`] values (or NULL with
/// `path_len == 0`). When `present_flag == 1`, `init_bytes` must point
/// to `init_len` readable bytes (or be NULL only if `init_len == 0`,
/// which only applies to zero-size fields — unusual).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_set_scalar_field_present(
    handle: *mut CrimsonSaveHandle,
    block_idx: u32,
    path: *const CrimsonPathStep,
    path_len: usize,
    field_idx: u32,
    present_flag: i32,
    init_bytes: *const u8,
    init_len: usize,
) -> i32 {
    if handle.is_null() {
        return error::NULL_ARG;
    }
    if path.is_null() && path_len != 0 {
        return error::NULL_ARG;
    }
    let make_present = present_flag != 0;
    if make_present && init_bytes.is_null() && init_len != 0 {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let _ffi_guard = save_ffi_lock();
        let h = unsafe { &mut *handle };
        let steps: &[CrimsonPathStep] = if path_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(path, path_len) }
        };
        // Copy init_bytes out of the raw pointer up front so the closure
        // below is fully owned.
        let init: Vec<u8> = if make_present {
            unsafe { slice_from_raw_or_empty(init_bytes, init_len) }.to_vec()
        } else {
            Vec::new()
        };
        apply_length_changing_mutation(h, |blocks| {
            toggle_one_scalar_presence_in_place(blocks, block_idx, steps, field_idx, make_present, &init)
        })
    }))
    .unwrap_or(error::PANIC)
}

/// Shared mutator for both the single-op and batch
/// `set_scalar_field_present` entry points. Validates the field, flips
/// the mask bit, sets `kind` / `value` to reflect the new presence,
/// and runs the FixedPrefix/FixedSuffix classification rule for the
/// "make present" case.
fn toggle_one_scalar_presence_in_place(
    blocks: &mut [ObjectBlock],
    block_idx: u32,
    path: &[CrimsonPathStep],
    field_idx: u32,
    make_present: bool,
    init_bytes: &[u8],
) -> Result<(), i32> {
    let parent = navigate_mut_to_parent(blocks, block_idx, path)?;
    let target_idx = field_idx as usize;
    let Some(field) = parent.fields.get(target_idx) else {
        return Err(error::OUT_OF_RANGE);
    };
    let meta_kind = field.meta_kind;
    let meta_size = field.meta_size as usize;
    let type_name = field.type_name.clone();
    if !matches!(meta_kind, 0 | 2) || meta_size == 0 {
        return Err(error::NOT_SCALAR_FIELD_KIND);
    }
    if make_present && init_bytes.len() != meta_size {
        return Err(error::LENGTH_MISMATCH);
    }

    // Toggle the mask bit at `target_idx`.
    let byte_idx = target_idx / 8;
    let bit_idx = target_idx % 8;
    if byte_idx >= parent.mask_bytes.len() {
        return Err(error::OUT_OF_RANGE);
    }
    if make_present {
        parent.mask_bytes[byte_idx] |= 1 << bit_idx;
    } else {
        parent.mask_bytes[byte_idx] &= !(1 << bit_idx);
    }

    // Decide forward vs reverse classification for the new state.
    let new_kind = if make_present {
        classify_scalar_after_mask_toggle(parent, target_idx)
    } else {
        FieldKind::Absent
    };

    // Patch the field's decoded shape. The encoder picks up these
    // values on the next encode pass.
    let field_mut = parent
        .fields
        .get_mut(target_idx)
        .expect("field bounds checked above");
    field_mut.present = make_present;
    field_mut.kind = new_kind;
    if make_present {
        let value = crate::save::scalar_from_bytes(init_bytes, &type_name, meta_size);
        field_mut.value = FieldValue::Scalar(value);
        // start/end are stale but the encoder ignores them for
        // scalar emission; they'll be refreshed by the re-decode.
    } else {
        field_mut.value = FieldValue::None;
        field_mut.start = 0;
        field_mut.end = 0;
    }
    Ok(())
}

/// Build the 18-byte `zero1_count_u24` header for an `object_list`
/// field with the given non-zero count. We pin this variant because
/// the decoder's body_offset-probing loop (see
/// [`decode_object_list`](crate::save::body::decoder)) accepts any of
/// {0,1,2,3} byte leading skip and picks the "furthest reaching"
/// success — so an all-zero header (count=0) is genuinely ambiguous
/// and round-trips as `marker_run_plus_zeros`, which the encoder
/// can't re-emit with a different count. Lists with count >= 1 in
/// `zero1_count_u24` disambiguate via the count u24 bytes themselves,
/// so the round-trip is well-defined; this is why
/// `set_object_list_present(make_present=1)` always materializes the
/// list with `count=1` + a default empty element rather than `count=0`.
fn build_zero1_count_u24_header(count: u32) -> Result<Vec<u8>, i32> {
    if count == 0 {
        // Defensive: callers must seed count >= 1 to keep the round-trip
        // unambiguous. The decoder's body_offset probing eats 1-3 bytes
        // past the header when count=0, contaminating the next field.
        return Err(error::LIST_VARIANT_UNSUPPORTED);
    }
    if count > 0xFF_FFFF {
        return Err(error::OUT_OF_RANGE);
    }
    let mut bytes = vec![0u8; 18];
    bytes[1] = (count & 0xFF) as u8;
    bytes[2] = ((count >> 8) & 0xFF) as u8;
    bytes[3] = ((count >> 16) & 0xFF) as u8;
    Ok(bytes)
}

/// Flip the mask bit of an `object_list` field, creating (or removing)
/// the corresponding list payload in the enclosing block.
///
/// `present_flag == 1` makes the field present:
/// - The mask bit at `field_idx` in the enclosing block is set.
/// - The field is initialised as a `count = 1` [`FieldValue::ObjectList`]
///   containing one default-empty element of the field's element class
///   (per the schema's `meta_aux`). This is the natural flow for the
///   dye-editor use case ("add the first dye element") and keeps the
///   byte layout unambiguous for the decoder's body-offset probing — a
///   `count = 0` header would be greedily reclassified as
///   `marker_run_plus_zeros` and steal bytes from subsequent fields.
/// - Caller follows up with
///   [`crimson_save_set_scalar_field_present`] /
///   [`crimson_save_set_scalar_field_path`] to populate the element's
///   scalar fields (RGBA, material, color group, …).
///
/// `present_flag == 0` makes the field absent:
/// - The mask bit is cleared.
/// - The field's `kind` becomes `Absent`, its `value` becomes `None`.
/// - Any existing elements are discarded. The encoder emits nothing
///   for an absent field, so the body shrinks back exactly the way it
///   was before the matching `make_present == 1` call — `present(1)
///   → present(0)` is byte-identical to the original.
///
/// Validation:
/// - The field's schema `meta_kind` must be `6` or `7` (ObjectList);
///   else `NOT_OBJECT_LIST`. Scalar / inline-bytes / dynamic-array
///   presence toggles route through
///   [`crimson_save_set_scalar_field_present`] /
///   [`crimson_save_set_inline_bytes_field`] /
///   [`crimson_save_dynamic_array_set_u32_elements`] respectively.
/// - When `present_flag == 1` and the schema can't build the default
///   element bytes (e.g. an unknown element class index), the call is
///   rolled back and returns `BODY_PARSE`.
///
/// # Safety
/// `handle` must be a live, exclusive handle. `path` must point to
/// `path_len` readable [`CrimsonPathStep`] values (or NULL with
/// `path_len == 0`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_set_object_list_present(
    handle: *mut CrimsonSaveHandle,
    block_idx: u32,
    path: *const CrimsonPathStep,
    path_len: usize,
    field_idx: u32,
    present_flag: i32,
) -> i32 {
    if handle.is_null() {
        return error::NULL_ARG;
    }
    if path.is_null() && path_len != 0 {
        return error::NULL_ARG;
    }
    let make_present = present_flag != 0;
    catch_unwind(AssertUnwindSafe(|| {
        let _ffi_guard = save_ffi_lock();
        let h = unsafe { &mut *handle };
        let steps: &[CrimsonPathStep] = if path_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(path, path_len) }
        };
        // Pre-build the default element bytes (and decode them with the
        // schema) outside the length-changing closure. The closure only
        // has access to `blocks`, so we hand it a ready-to-insert
        // `ObjectBlock`. For the absent-toggle path this is unused.
        let default_element: Option<ObjectBlock> = if make_present {
            let element_class = match resolve_object_list_element_class(
                &h.blocks,
                block_idx,
                steps,
                field_idx,
            ) {
                Ok(c) => c,
                Err(code) => return code,
            };
            let bytes = match build_empty_element_bytes(element_class, &h.body) {
                Ok(b) => b,
                Err(code) => return code,
            };
            let schema_clone = h.body.schema.clone();
            match crate::save::decode_one_list_element_bytes(&bytes, &schema_clone) {
                Ok(el) => Some(el),
                Err(_) => return error::BODY_PARSE,
            }
        } else {
            None
        };
        apply_length_changing_mutation(h, move |blocks| {
            toggle_one_object_list_presence_in_place(
                blocks,
                block_idx,
                steps,
                field_idx,
                make_present,
                default_element,
            )
        })
    }))
    .unwrap_or(error::PANIC)
}

/// Resolve the element `class_index` for an ObjectList field whose
/// current instance is **absent**.
///
/// The schema's `meta_aux` is opaque for `meta_kind ∈ {6, 7}` (the
/// element class is encoded per-element on the wrapper, not on the
/// field). So we discover it by scanning the whole save tree for any
/// block of the same parent class with the field present-and-non-empty,
/// then copying that element's `class_index`. Returns `NOT_FOUND` when
/// no template exists.
fn resolve_object_list_element_class(
    blocks: &[ObjectBlock],
    block_idx: u32,
    path: &[CrimsonPathStep],
    field_idx: u32,
) -> Result<u32, i32> {
    // Step 1: walk to the target parent block so we know its class_name
    // + verify the field is meta_kind 6/7.
    let mut current = blocks
        .get(block_idx as usize)
        .ok_or(error::OUT_OF_RANGE)?;
    for step in path {
        let field = current
            .fields
            .get(step.field_idx as usize)
            .ok_or(error::OUT_OF_RANGE)?;
        current = match &field.value {
            FieldValue::Locator { child: Some(child), .. } => child.as_ref(),
            FieldValue::ObjectList { elements, .. } => elements
                .get(step.element_idx as usize)
                .ok_or(error::OUT_OF_RANGE)?,
            _ => return Err(error::NOT_NAVIGABLE),
        };
    }
    let field = current
        .fields
        .get(field_idx as usize)
        .ok_or(error::OUT_OF_RANGE)?;
    if !matches!(field.meta_kind, 6 | 7) {
        return Err(error::NOT_OBJECT_LIST);
    }
    let parent_class = current.class_name.clone();
    let target_idx = field_idx as usize;

    // Step 2: scan the tree for any block of the same parent class with
    // the same field present-and-non-empty. Copy the first element's
    // class_index.
    fn scan(
        block: &ObjectBlock,
        target_class: &str,
        target_idx: usize,
    ) -> Option<u32> {
        if block.class_name == target_class
            && let Some(field) = block.fields.get(target_idx)
            && let FieldValue::ObjectList { elements, .. } = &field.value
            && let Some(first) = elements.first()
        {
            return Some(first.class_index);
        }
        for f in &block.fields {
            match &f.value {
                FieldValue::Locator { child: Some(child), .. } => {
                    if let Some(v) = scan(child, target_class, target_idx) {
                        return Some(v);
                    }
                }
                FieldValue::ObjectList { elements, .. } => {
                    for el in elements {
                        if let Some(v) = scan(el, target_class, target_idx) {
                            return Some(v);
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }
    for top in blocks {
        if let Some(c) = scan(top, &parent_class, target_idx) {
            return Ok(c);
        }
    }
    Err(error::NOT_FOUND)
}

/// Shared mutator behind [`crimson_save_set_object_list_present`].
/// `default_element` is required for the make-present path and ignored
/// on make-absent.
fn toggle_one_object_list_presence_in_place(
    blocks: &mut [ObjectBlock],
    block_idx: u32,
    path: &[CrimsonPathStep],
    field_idx: u32,
    make_present: bool,
    default_element: Option<ObjectBlock>,
) -> Result<(), i32> {
    let parent = navigate_mut_to_parent(blocks, block_idx, path)?;
    let target_idx = field_idx as usize;
    let Some(field) = parent.fields.get(target_idx) else {
        return Err(error::OUT_OF_RANGE);
    };
    if !matches!(field.meta_kind, 6 | 7) {
        return Err(error::NOT_OBJECT_LIST);
    }
    if target_idx / 8 >= parent.mask_bytes.len() {
        return Err(error::OUT_OF_RANGE);
    }

    let byte_idx = target_idx / 8;
    let bit_idx = target_idx % 8;
    if make_present {
        parent.mask_bytes[byte_idx] |= 1 << bit_idx;
    } else {
        parent.mask_bytes[byte_idx] &= !(1 << bit_idx);
    }

    let field_mut = parent
        .fields
        .get_mut(target_idx)
        .expect("field bounds checked above");
    field_mut.present = make_present;
    if make_present {
        let element = default_element.ok_or(error::NULL_ARG)?;
        let header_bytes = build_zero1_count_u24_header(1)?;
        field_mut.kind = FieldKind::ObjectList;
        field_mut.value = FieldValue::ObjectList {
            count: 1,
            header_variant: "zero1_count_u24",
            header_bytes,
            elements: vec![element],
        };
        // start/end are stale but the encoder uses header_bytes +
        // elements directly; the re-decode pass refreshes the range.
    } else {
        field_mut.kind = FieldKind::Absent;
        field_mut.value = FieldValue::None;
        field_mut.start = 0;
        field_mut.end = 0;
    }
    Ok(())
}

/// Apply many [`CrimsonScalarPresentBatchOp`] mutations in one FFI
/// round trip, sharing a single post-batch re-emit + re-decode.
///
/// Semantics:
/// 1. **Apply ops in input order.** Each op is validated and applied
///    against the freshly-mutated tree, exactly as if the caller had
///    run N sequential [`crimson_save_set_scalar_field_present`]
///    calls. When multiple ops target the same parent block, each
///    successive `classify_scalar_after_mask_toggle` call sees the
///    earlier toggles reflected — same end state as running N single
///    ops back-to-back.
/// 2. **All-or-nothing on failure.** On the first op that fails any
///    of the single-op API's checks (`NOT_SCALAR_FIELD_KIND` /
///    `LENGTH_MISMATCH` / `OUT_OF_RANGE` / `NOT_NAVIGABLE`), the
///    in-memory body is rolled back to its pre-batch state and the
///    call returns that op's index via `out_failed_op_index`.
/// 3. **One re-emit + re-decode at the end.** The body is serialized
///    via [`Body::write`] exactly once after the last successful
///    toggle. This is the high-throughput path for bulk presence
///    flips — e.g. "promote `_completedTime` from absent to present
///    on 1300 `MissionStateData` blocks", which would otherwise pay
///    1300 × encode + decode cost (~20 minutes on a 1100-block save)
///    and instead amortizes to a single re-emit (~seconds).
///
/// `out_failed_op_index` (optional, may be NULL):
/// - On error, written with the index of the failing op.
/// - On success, written with `usize::MAX` as a "no failure" sentinel.
///
/// # Safety
/// `handle` must be a live, exclusive handle. `ops` must point to
/// `op_count` readable [`CrimsonScalarPresentBatchOp`] values for the
/// duration of the call. Each op's `path` and `bytes` pointers must
/// point to `path_len` / `bytes_len` readable elements respectively
/// for the duration of the call (NULL is allowed when the
/// corresponding length is 0, and `bytes` is also allowed to be NULL
/// when `make_present == 0`, in which case it is ignored).
/// `out_failed_op_index` may be NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_set_scalar_fields_present_batch(
    handle: *mut CrimsonSaveHandle,
    ops: *const CrimsonScalarPresentBatchOp,
    op_count: usize,
    out_failed_op_index: *mut usize,
) -> i32 {
    if handle.is_null() {
        return error::NULL_ARG;
    }
    if ops.is_null() && op_count != 0 {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        if op_count == 0 {
            if !out_failed_op_index.is_null() {
                unsafe {
                    *out_failed_op_index = usize::MAX;
                }
            }
            return error::OK;
        }

        let ops_slice: &[CrimsonScalarPresentBatchOp] =
            unsafe { slice_from_raw_or_empty(ops, op_count) };

        // Per-op NULL_ARG pre-check. `path` may be NULL only when
        // `path_len == 0`; `bytes` may be NULL only when `bytes_len
        // == 0` OR `make_present == 0` (in which case bytes are
        // ignored entirely).
        for (i, op) in ops_slice.iter().enumerate() {
            if op.path.is_null() && op.path_len != 0 {
                if !out_failed_op_index.is_null() {
                    unsafe {
                        *out_failed_op_index = i;
                    }
                }
                return error::NULL_ARG;
            }
            if op.make_present != 0
                && op.bytes.is_null()
                && op.bytes_len != 0
            {
                if !out_failed_op_index.is_null() {
                    unsafe {
                        *out_failed_op_index = i;
                    }
                }
                return error::NULL_ARG;
            }
        }

        // Copy init bytes out of raw pointers up front so the closure
        // below is fully owned and doesn't carry raw pointers across
        // the encode boundary. For absent ops we keep an empty Vec.
        let init_bufs: Vec<Vec<u8>> = ops_slice
            .iter()
            .map(|op| {
                if op.make_present != 0 {
                    unsafe { slice_from_raw_or_empty(op.bytes, op.bytes_len) }.to_vec()
                } else {
                    Vec::new()
                }
            })
            .collect();

        let _ffi_guard = save_ffi_lock();
        let h = unsafe { &mut *handle };
        let mut failed_op: Option<usize> = None;
        let rc = apply_length_changing_mutation(h, |blocks| {
            for (i, op) in ops_slice.iter().enumerate() {
                let steps: &[CrimsonPathStep] = if op.path_len == 0 {
                    &[]
                } else {
                    unsafe { std::slice::from_raw_parts(op.path, op.path_len) }
                };
                let make_present = op.make_present != 0;
                if let Err(code) = toggle_one_scalar_presence_in_place(
                    blocks,
                    op.block_idx,
                    steps,
                    op.field_idx,
                    make_present,
                    &init_bufs[i],
                ) {
                    failed_op = Some(i);
                    return Err(code);
                }
            }
            Ok(())
        });

        if !out_failed_op_index.is_null() {
            unsafe {
                *out_failed_op_index = failed_op.unwrap_or(usize::MAX);
            }
        }
        rc
    }))
    .unwrap_or(error::PANIC)
}

/// Produce the minimal valid bytes for a list element of `class_index`.
///
/// The emitted element has:
/// - Locator wrapper: `mbc` (= `ceil(field_count / 8)` clamped to `1..=16`)
///   mask bytes (all zero, so every field is absent), `type_index =
///   class_index`, all sentinels and `payload_offset` zero.
/// - Inline payload: `u32 reserved = 0`, no field bytes, `u32
///   trailing_size = 4`.
///
/// Total size: wrapper (`mbc + 17`) + payload (`8`) = `mbc + 25` bytes.
/// For a class with a 4-byte mask (≤32 fields), the element is 29 bytes.
///
/// Uses the standard two-call pattern: pass `buf=NULL, buf_len=0` to
/// learn the required size, then allocate and call again. `out_required`
/// is always populated when non-NULL.
///
/// The returned bytes parse via the decoder as a valid list element of
/// `class_index` with every field marked absent. Callers typically:
/// 1. Call this to get an "empty shell" for the desired class.
/// 2. Call [`crimson_save_list_insert_element`] to add the shell to a
///    list.
/// 3. Call [`crimson_save_set_scalar_field_present`] (and
///    [`crimson_save_set_scalar_field_path`]) for each field they want
///    to populate.
///
/// Errors:
/// - `OUT_OF_RANGE` when `class_index` doesn't resolve to a schema
///   type, or `field_count > 128` (would need mbc > 16, which the
///   decoder rejects).
/// - `BUFFER_TOO_SMALL` when `buf_len` is too small.
///
/// # Safety
/// `handle` must be a live handle. If `buf` is non-NULL it must be
/// writable for at least `buf_len` bytes. If `out_required` is non-NULL
/// it must be a writable `*mut usize`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_make_empty_element_bytes(
    handle: *const CrimsonSaveHandle,
    class_index: u32,
    buf: *mut u8,
    buf_len: usize,
    out_required: *mut usize,
) -> i32 {
    if handle.is_null() {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let _ffi_guard = save_ffi_lock();
        let h = unsafe { &*handle };
        let bytes = match build_empty_element_bytes(class_index, &h.body) {
            Ok(b) => b,
            Err(code) => return code,
        };
        let required = bytes.len();
        if !out_required.is_null() {
            unsafe {
                *out_required = required;
            }
        }
        if buf.is_null() || buf_len < required {
            return error::BUFFER_TOO_SMALL;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
        }
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Insert a caller-supplied list-element bytes blob into an
/// `object_list` field at `insert_at`.
///
/// `bytes[0..bytes_len]` must be the raw bytes of a complete list
/// element: wrapper + inline payload, exactly as
/// [`crimson_save_make_empty_element_bytes`] produces (or as you'd
/// extract from `raw[element.data_offset .. element.data_offset +
/// element.data_size]` of an existing decoded element).
///
/// Validation:
/// - The leaf field must be `FieldKind::ObjectList`; else `NOT_SCALAR`.
/// - `bytes` must parse as a valid list element of a class known to
///   the schema; else `BODY_PARSE`.
/// - `insert_at` must be `<= current count`; else `OUT_OF_RANGE`.
/// - The list's `header_variant` must be count-patchable — a fixed-size
///   variant or `marker_run_plus_zeros`; else `LIST_VARIANT_UNSUPPORTED`.
///
/// On success the list grows by one element, the variant header's
/// count bytes are rewritten, and the body is re-encoded + re-parsed
/// so subsequent reads see the new layout. On any failure the handle
/// is left untouched.
///
/// # Safety
/// `handle` must be a live, exclusive handle. `path` must point to
/// `path_len` readable [`CrimsonPathStep`] values (or NULL with
/// `path_len == 0`). `bytes` must point to `bytes_len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_list_insert_element(
    handle: *mut CrimsonSaveHandle,
    block_idx: u32,
    path: *const CrimsonPathStep,
    path_len: usize,
    field_idx: u32,
    insert_at: u32,
    bytes: *const u8,
    bytes_len: usize,
) -> i32 {
    if handle.is_null() {
        return error::NULL_ARG;
    }
    if path.is_null() && path_len != 0 {
        return error::NULL_ARG;
    }
    if bytes.is_null() && bytes_len != 0 {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let _ffi_guard = save_ffi_lock();
        let h = unsafe { &mut *handle };
        let steps: &[CrimsonPathStep] = if path_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(path, path_len) }
        };
        // Copy the element bytes + clone the schema up front so the
        // closure below doesn't borrow `h` twice.
        let bytes_vec: Vec<u8> =
            unsafe { slice_from_raw_or_empty(bytes, bytes_len) }.to_vec();
        let schema_clone = h.body.schema.clone();
        let parsed = match crate::save::decode_one_list_element_bytes(&bytes_vec, &schema_clone) {
            Ok(el) => el,
            Err(_) => return error::BODY_PARSE,
        };
        apply_length_changing_mutation(h, move |blocks| {
            let field = navigate_mut_to_field(blocks, block_idx, steps, field_idx)?;
            let FieldValue::ObjectList {
                count,
                header_variant,
                header_bytes,
                elements,
            } = &mut field.value
            else {
                return Err(error::NOT_SCALAR);
            };
            if (insert_at as usize) > elements.len() {
                return Err(error::OUT_OF_RANGE);
            }
            elements.insert(insert_at as usize, parsed);
            *count = elements.len() as u32;
            update_object_list_count_in_header(header_bytes, header_variant, *count)?;
            Ok(())
        })
    }))
    .unwrap_or(error::PANIC)
}

/// Construct the minimal valid bytes for a list element of
/// `class_index`. See [`crimson_save_make_empty_element_bytes`] for the
/// exposed C ABI shape; this is the Rust helper that does the work.
fn build_empty_element_bytes(class_index: u32, body: &Body) -> Result<Vec<u8>, i32> {
    let type_def = body
        .schema
        .types
        .iter()
        .find(|t| t.index == class_index)
        .ok_or(error::OUT_OF_RANGE)?;
    let field_count = type_def.fields.len();
    // The decoder accepts mbc in 1..=16, so cap at 16 (= 128 fields max).
    // Decoder rule mirrored from object/decoder: `expected_mask_bytes =
    // type_def.fields.len().div_ceil(8).max(1)`.
    let mbc = field_count.div_ceil(8).max(1);
    if mbc > 16 {
        return Err(error::OUT_OF_RANGE);
    }

    // Wrapper layout: u16 mbc (2) | u8[mbc] mask | u16 type_index (2)
    //   | u8 reserved (1) | u32 sent1 (4) | u32 sent2 (4)
    //   | u32 payload_offset (4)
    //   = 2 + mbc + 2 + 1 + 4 + 4 + 4 = mbc + 17 bytes.
    // Inline payload: u32 reserved (4) | (no fields) | u32 trailing_size (4)
    //   = 8 bytes.
    let wrapper_size = mbc + 17;
    let payload_size = 4 + 4;
    let total = wrapper_size + payload_size;
    let mut out = Vec::with_capacity(total);

    // Wrapper bytes.
    out.extend_from_slice(&(mbc as u16).to_le_bytes());
    out.extend(std::iter::repeat_n(0u8, mbc));
    out.extend_from_slice(&(class_index as u16).to_le_bytes());
    out.push(0); // child_reserved
    out.extend_from_slice(&0u32.to_le_bytes()); // sentinel1
    out.extend_from_slice(&0u32.to_le_bytes()); // sentinel2
    out.extend_from_slice(&0u32.to_le_bytes()); // payload_offset (advisory)

    // Inline payload bytes. `trailing_size = 4` because the size u32
    // sits 4 bytes after `payload_start` (just the `reserved` u32).
    out.extend_from_slice(&0u32.to_le_bytes()); // payload reserved
    out.extend_from_slice(&4u32.to_le_bytes()); // payload trailing_size

    debug_assert_eq!(
        out.len(),
        total,
        "empty element bytes len mismatch ({total} expected, got {})",
        out.len()
    );
    Ok(out)
}

/// Serialize the in-memory save back to `path` using the original nonce.
///
/// Wholesale-replace the contents of a `dynamic_array<u32>` field. The
/// existing element bytes are dropped and replaced with
/// `[new_elements[0..new_count]]` (each element written little-endian);
/// the variant header's count slot is rewritten to match. Body re-emit
/// + re-decode runs once at the end.
///
/// Use cases:
/// - Append/insert tags: read the current elements, build the desired
///   sequence in the caller, hand the whole sequence in.
/// - Empty an array: pass `new_count = 0`.
///
/// Validation:
/// - The leaf field must be `FieldKind::DynamicArray`; else `NOT_SCALAR`.
/// - The field's `meta_size` must be 4 (u32 element); else
///   `LENGTH_MISMATCH`. (Other element widths can be added when a real
///   need surfaces.)
/// - The header's `header_variant` must be one of the known fixed-shape
///   variants (`prefix_00xx0100` / `marker_prefix` / `compact` /
///   `generic`); else `BODY_PARSE`.
/// - `new_count` must fit the variant's count slot
///   (`< 0x10000` for `compact` / `prefix_00xx0100` / `marker_prefix`;
///   `<= u32::MAX` for `generic`); else `OUT_OF_RANGE`.
///
/// # Safety
/// `handle` must be a live, exclusive handle. `path` must point to
/// `path_len` readable [`CrimsonPathStep`] values for the duration of
/// the call (or be NULL with `path_len == 0`). `new_elements` must
/// point to `new_count` readable `u32` values for the duration of the
/// call (or be NULL with `new_count == 0`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_dynamic_array_set_u32_elements(
    handle: *mut CrimsonSaveHandle,
    block_idx: u32,
    path: *const CrimsonPathStep,
    path_len: usize,
    field_idx: u32,
    new_elements: *const u32,
    new_count: usize,
) -> i32 {
    if handle.is_null() {
        return error::NULL_ARG;
    }
    if path.is_null() && path_len != 0 {
        return error::NULL_ARG;
    }
    if new_elements.is_null() && new_count != 0 {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let _ffi_guard = save_ffi_lock();
        let h = unsafe { &mut *handle };
        let steps: &[CrimsonPathStep] = if path_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(path, path_len) }
        };
        // Snapshot the new elements out of the raw pointer up front so the
        // closure below is fully owned and doesn't carry raw pointers
        // across the encode boundary.
        let new_elems: Vec<u32> = if new_count == 0 {
            Vec::new()
        } else {
            unsafe { slice_from_raw_or_empty(new_elements, new_count) }.to_vec()
        };
        if new_elems.len() > u32::MAX as usize {
            return error::OUT_OF_RANGE;
        }
        let new_count_u32 = new_elems.len() as u32;

        apply_length_changing_mutation(h, |blocks| {
            let field = navigate_mut_to_field(blocks, block_idx, steps, field_idx)?;
            if !matches!(field.kind, FieldKind::DynamicArray) {
                return Err(error::NOT_SCALAR);
            }
            // Restrict to u32 element width for now. Other widths can be
            // added by mirroring this function for u8/u16/u64 if needed.
            if field.meta_size != 4 {
                return Err(error::LENGTH_MISMATCH);
            }
            let FieldValue::DynamicArray {
                count,
                bytes,
                header_variant,
                header_bytes,
                ..
            } = &mut field.value
            else {
                return Err(error::NOT_SCALAR);
            };
            update_dynamic_array_count_in_header(header_bytes, header_variant, new_count_u32)?;
            // Replace the data bytes with the new elements (LE).
            let mut new_bytes = Vec::with_capacity(new_elems.len() * 4);
            for &e in &new_elems {
                new_bytes.extend_from_slice(&e.to_le_bytes());
            }
            *bytes = new_bytes;
            *count = new_count_u32;
            Ok(())
        })
    }))
    .unwrap_or(error::PANIC)
}

/// Read the contents of a `dynamic_array<u32>` field as a flat
/// little-endian `u32` sequence. Uses the standard two-call buffer
/// pattern: pass `out_buf = NULL, buf_len = 0` to learn the required
/// element count via `out_required`, then allocate and call again.
///
/// Validation:
/// - The leaf field must be `FieldKind::DynamicArray`; else `NOT_SCALAR`.
/// - The field's `meta_size` must be 4 (u32 element); else
///   `LENGTH_MISMATCH`.
///
/// `out_required` is always populated (when non-NULL) with the
/// element count, regardless of whether the buffer was big enough.
/// Returns `BUFFER_TOO_SMALL` when the caller's buffer can't fit
/// every element.
///
/// # Safety
/// `handle` must be a live handle. `path` must point to `path_len`
/// readable [`CrimsonPathStep`] values for the duration of the call
/// (or be NULL with `path_len == 0`). When non-NULL, `out_buf` must be
/// writable for at least `buf_len` `u32` values. `out_required` may
/// be NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_dynamic_array_get_u32_elements(
    handle: *const CrimsonSaveHandle,
    block_idx: u32,
    path: *const CrimsonPathStep,
    path_len: usize,
    field_idx: u32,
    out_buf: *mut u32,
    buf_len: usize,
    out_required: *mut usize,
) -> i32 {
    if handle.is_null() {
        return error::NULL_ARG;
    }
    if path.is_null() && path_len != 0 {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let _ffi_guard = save_ffi_lock();
        let h = unsafe { &*handle };
        let steps: &[CrimsonPathStep] = if path_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(path, path_len) }
        };
        // Read-only navigation reuses the schema-aware `resolve_leaf_range`
        // path used by other read-side getters, but here we need the
        // FieldValue, not just byte offsets. Walk a borrowed view of
        // h.blocks instead.
        let parent = match navigate_to_parent_ref(&h.blocks, block_idx, steps) {
            Ok(b) => b,
            Err(code) => return code,
        };
        let field = match parent.fields.get(field_idx as usize) {
            Some(f) => f,
            None => return error::OUT_OF_RANGE,
        };
        if !matches!(field.kind, FieldKind::DynamicArray) {
            return error::NOT_SCALAR;
        }
        if field.meta_size != 4 {
            return error::LENGTH_MISMATCH;
        }
        let FieldValue::DynamicArray { bytes, count, .. } = &field.value else {
            return error::NOT_SCALAR;
        };
        let elem_count = *count as usize;
        // Sanity: bytes.len() should equal elem_count * 4.
        if bytes.len() != elem_count * 4 {
            return error::BODY_PARSE;
        }
        if !out_required.is_null() {
            unsafe {
                *out_required = elem_count;
            }
        }
        if out_buf.is_null() || buf_len < elem_count {
            return error::BUFFER_TOO_SMALL;
        }
        // Write each element as native-host u32 (LE on x86/ARM little).
        // The on-disk bytes are LE; copy directly via from_le_bytes.
        for i in 0..elem_count {
            let off = i * 4;
            let v = u32::from_le_bytes([
                bytes[off],
                bytes[off + 1],
                bytes[off + 2],
                bytes[off + 3],
            ]);
            unsafe {
                *out_buf.add(i) = v;
            }
        }
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Wholesale-replace the contents of an `inline_bytes` field (the
/// schema's `meta_kind == 1` shape: a `u32 count` header followed by
/// `count * meta_size` payload bytes). Used for length-changing edits
/// to string-like fields — `_mercenaryName` is the motivating case
/// (length-prefixed UTF-8) but the same surface works for any
/// homogeneous element-width inline array.
///
/// Validation:
/// - The leaf field's `meta_kind` must be `1`; else `NOT_INLINE_BYTES`.
///   Use [`crimson_save_set_scalar_field_present`] for fixed-size
///   scalars (`meta_kind` 0 / 2) and the dynamic-array setters for
///   `meta_kind == 3`.
/// - `new_bytes_len` must be a multiple of the field's `meta_size`
///   (so the `count = new_bytes_len / meta_size` math is exact); else
///   `LENGTH_MISMATCH`.
/// - The computed `count` must fit in a `u32`; else `OUT_OF_RANGE`.
///
/// Semantics:
/// - When the field was absent (mask bit cleared), this call promotes
///   it to present and writes the new bytes — exactly mirrors the
///   absent → present path on [`crimson_save_set_scalar_field_present`].
/// - When the field was already present, the existing bytes are
///   dropped and replaced with `new_bytes`. The encoder's re-emit
///   shifts every downstream offset (TOC, locator wrappers,
///   `payload_offset`s) to match the new length.
/// - Passing `new_bytes_len == 0` writes a zero-length string (count
///   slot becomes 0, payload becomes empty). The field stays present
///   — to fully clear the field's bytes from the block, use a future
///   "make-absent" entry point (not yet exposed for inline_bytes).
///
/// # Safety
/// `handle` must be a live, exclusive handle. `path` must point to
/// `path_len` readable [`CrimsonPathStep`] values for the duration of
/// the call (or be NULL with `path_len == 0`). `new_bytes` must point
/// to `new_bytes_len` readable bytes for the duration of the call (or
/// be NULL with `new_bytes_len == 0`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_set_inline_bytes_field(
    handle: *mut CrimsonSaveHandle,
    block_idx: u32,
    path: *const CrimsonPathStep,
    path_len: usize,
    field_idx: u32,
    new_bytes: *const u8,
    new_bytes_len: usize,
) -> i32 {
    if handle.is_null() {
        return error::NULL_ARG;
    }
    if path.is_null() && path_len != 0 {
        return error::NULL_ARG;
    }
    if new_bytes.is_null() && new_bytes_len != 0 {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let _ffi_guard = save_ffi_lock();
        let h = unsafe { &mut *handle };
        let steps: &[CrimsonPathStep] = if path_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(path, path_len) }
        };
        // Snapshot the bytes out of the raw pointer up front so the
        // closure below is fully owned and doesn't carry raw pointers
        // across the encode boundary.
        let bytes: Vec<u8> = if new_bytes_len == 0 {
            Vec::new()
        } else {
            unsafe { slice_from_raw_or_empty(new_bytes, new_bytes_len) }.to_vec()
        };
        apply_length_changing_mutation(h, |blocks| {
            write_inline_bytes_in_place(blocks, block_idx, steps, field_idx, &bytes)
        })
    }))
    .unwrap_or(error::PANIC)
}

/// Read the raw payload bytes of an `inline_bytes` field — the read
/// counterpart to [`crimson_save_set_inline_bytes_field`]. Two-call
/// pattern: probe with `buf_len == 0` to learn the byte count via
/// `out_required`, then call again with a sized buffer.
///
/// Unlike the name-resolver string getters, the payload is copied
/// **verbatim with no NUL terminator** — it's arbitrary bytes (e.g. the
/// length-prefixed UTF-8 of `_mercenaryName`) and the caller already
/// knows the length. Decode the bytes caller-side.
///
/// Validation mirrors the setter: the leaf field's `meta_kind` must be
/// `1`, else `NOT_INLINE_BYTES`. A present-but-empty (or absent-value)
/// inline_bytes field reads back as zero bytes (`OK`, `out_required == 0`).
///
/// # Safety
/// `handle` must be a live handle and `out_required` non-null. `path`
/// must point to `path_len` readable [`CrimsonPathStep`] values for the
/// call (or be NULL with `path_len == 0`). `buf` may be NULL iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_get_inline_bytes_field(
    handle: *const CrimsonSaveHandle,
    block_idx: u32,
    path: *const CrimsonPathStep,
    path_len: usize,
    field_idx: u32,
    buf: *mut u8,
    buf_len: usize,
    out_required: *mut usize,
) -> i32 {
    if handle.is_null() || out_required.is_null() {
        return error::NULL_ARG;
    }
    if path.is_null() && path_len != 0 {
        return error::NULL_ARG;
    }
    if buf.is_null() && buf_len != 0 {
        return error::NULL_ARG;
    }
    unsafe { *out_required = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let _ffi_guard = save_ffi_lock();
        let h = unsafe { &*handle };
        let steps: &[CrimsonPathStep] = if path_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(path, path_len) }
        };
        let parent = match navigate_to_parent_ref(&h.blocks, block_idx, steps) {
            Ok(p) => p,
            Err(e) => return e,
        };
        let Some(field) = parent.fields.get(field_idx as usize) else {
            return error::OUT_OF_RANGE;
        };
        if field.meta_kind != 1 {
            return error::NOT_INLINE_BYTES;
        }
        let payload: &[u8] = match &field.value {
            FieldValue::InlineBytes { bytes, .. } => bytes,
            // present meta_kind==1 but a non-inline value (absent etc.) →
            // read back as empty rather than erroring.
            _ => &[],
        };
        write_bytes_to_buf(payload, buf, buf_len, out_required)
    }))
    .unwrap_or(error::PANIC)
}

/// Shared mutator for [`crimson_save_set_inline_bytes_field`]. Walks
/// the path, validates the leaf field's `meta_kind == 1`, sets the
/// mask bit, and overwrites the `FieldValue::InlineBytes` payload with
/// the new bytes. `count` is derived from `new_bytes.len() / meta_size`.
fn write_inline_bytes_in_place(
    blocks: &mut [ObjectBlock],
    block_idx: u32,
    path: &[CrimsonPathStep],
    field_idx: u32,
    new_bytes: &[u8],
) -> Result<(), i32> {
    let parent = navigate_mut_to_parent(blocks, block_idx, path)?;
    let target_idx = field_idx as usize;
    let Some(field) = parent.fields.get(target_idx) else {
        return Err(error::OUT_OF_RANGE);
    };
    if field.meta_kind != 1 {
        return Err(error::NOT_INLINE_BYTES);
    }
    let meta_size = field.meta_size as usize;
    if meta_size == 0 {
        // Defensive: the decoder shouldn't emit zero-width inline
        // arrays, but the field-shape invariant lives in metadata so
        // guard explicitly.
        return Err(error::NOT_INLINE_BYTES);
    }
    if !new_bytes.len().is_multiple_of(meta_size) {
        return Err(error::LENGTH_MISMATCH);
    }
    let count_usize = new_bytes.len() / meta_size;
    if count_usize > u32::MAX as usize {
        return Err(error::OUT_OF_RANGE);
    }
    let count = count_usize as u32;

    // Flip the mask bit on if the field was previously absent — same
    // shape as the absent → present path in `set_scalar_field_present`.
    let byte_idx = target_idx / 8;
    let bit_idx = target_idx % 8;
    if byte_idx >= parent.mask_bytes.len() {
        return Err(error::OUT_OF_RANGE);
    }
    parent.mask_bytes[byte_idx] |= 1 << bit_idx;

    let field_mut = parent
        .fields
        .get_mut(target_idx)
        .expect("field bounds checked above");
    field_mut.present = true;
    field_mut.kind = FieldKind::InlineBytes;
    field_mut.value = FieldValue::InlineBytes {
        count,
        bytes: new_bytes.to_vec(),
    };
    // start / end will be refreshed on re-decode after the encode pass.
    Ok(())
}

/// Read-only counterpart to [`navigate_mut_to_parent`]. Walks the
/// `path[]` from the top-level block at `block_idx` and returns a
/// `&ObjectBlock` to the deepest reachable block, threading borrows
/// instead of mutable references so multiple read paths can call it
/// concurrently.
fn navigate_to_parent_ref<'a>(
    blocks: &'a [ObjectBlock],
    block_idx: u32,
    path: &[CrimsonPathStep],
) -> Result<&'a ObjectBlock, i32> {
    let mut current = blocks
        .get(block_idx as usize)
        .ok_or(error::OUT_OF_RANGE)?;
    for step in path {
        let field = current
            .fields
            .get(step.field_idx as usize)
            .ok_or(error::OUT_OF_RANGE)?;
        current = match &field.value {
            FieldValue::Locator { child: Some(child), .. } => child.as_ref(),
            FieldValue::ObjectList { elements, .. } => elements
                .get(step.element_idx as usize)
                .ok_or(error::OUT_OF_RANGE)?,
            _ => return Err(error::NOT_NAVIGABLE),
        };
    }
    Ok(current)
}

/// Patch the count slot in a dynamic-array `header_bytes` blob to
/// `new_count`, dispatched by `header_variant`. Mirrors the layouts the
/// decoder produces in [`super::super::save::body::decoder::decode_dynamic_array`].
fn update_dynamic_array_count_in_header(
    header_bytes: &mut [u8],
    header_variant: &str,
    new_count: u32,
) -> Result<(), i32> {
    match header_variant {
        // 9 bytes: `00 00 XX 01 00 <u32 count LE>`. Count constrained
        // to `< 0x10000` by the decoder's matcher.
        "prefix_00xx0100" => {
            if header_bytes.len() != 9 {
                return Err(error::BODY_PARSE);
            }
            if new_count >= 0x10000 {
                return Err(error::OUT_OF_RANGE);
            }
            header_bytes[5..9].copy_from_slice(&new_count.to_le_bytes());
            Ok(())
        }
        // Variable: `01..01 00 <u32 count LE>` (N markers + 0 + count).
        // Decoder constrains count to `< 0x10000`.
        "marker_prefix" => {
            let zero_pos = header_bytes
                .iter()
                .position(|&b| b == 0)
                .ok_or(error::BODY_PARSE)?;
            if header_bytes.len() < zero_pos + 5 {
                return Err(error::BODY_PARSE);
            }
            if new_count >= 0x10000 {
                return Err(error::OUT_OF_RANGE);
            }
            header_bytes[zero_pos + 1..zero_pos + 5]
                .copy_from_slice(&new_count.to_le_bytes());
            Ok(())
        }
        // 6 bytes: `00 00 <u16 count LE> 00 00`.
        "compact" => {
            if header_bytes.len() != 6 {
                return Err(error::BODY_PARSE);
            }
            let count_u16: u16 = new_count.try_into().map_err(|_| error::OUT_OF_RANGE)?;
            header_bytes[2..4].copy_from_slice(&count_u16.to_le_bytes());
            Ok(())
        }
        // 5 bytes: `<u8 prefix> <u32 count LE>`. No count limit beyond
        // u32 itself.
        "generic" => {
            if header_bytes.len() != 5 {
                return Err(error::BODY_PARSE);
            }
            header_bytes[1..5].copy_from_slice(&new_count.to_le_bytes());
            Ok(())
        }
        _ => Err(error::BODY_PARSE),
    }
}

/// Uses `Save::write_with_nonce` so the on-disk layout matches what the
/// game produced — HMAC re-computed, ChaCha20 re-applied. The header's
/// `uncompressed_size` and `payload_size` get patched to match the
/// current body / freshly-compressed payload.
///
/// # Safety
/// `handle` must be a live handle. `path` must be a NUL-terminated
/// UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_write_to_file(
    handle: *const CrimsonSaveHandle,
    path: *const c_char,
) -> i32 {
    if handle.is_null() || path.is_null() {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let _ffi_guard = save_ffi_lock();
        let h = unsafe { &*handle };
        if h.is_deferred() {
            // Writing during an open batch would emit the pre-batch
            // bytes (h.save.body hasn't been refreshed yet) — silently
            // dropping every mutation in the batch. Force the caller
            // to end or abort the batch first.
            return error::BATCH_IN_PROGRESS;
        }
        let path_str = match unsafe { CStr::from_ptr(path) }.to_str() {
            Ok(s) => s,
            Err(_) => return error::INVALID_PATH,
        };
        let nonce = h.save.header.nonce();
        let bytes = match h.save.write_with_nonce(nonce) {
            Ok(b) => b,
            Err(e) => return save_error_code(&e),
        };
        match std::fs::write(path_str, &bytes) {
            Ok(()) => error::OK,
            Err(_) => error::WRITE_FAILED,
        }
    }))
    .unwrap_or(error::PANIC)
}

// ── Deferred-redecode batch ────────────────────────────────────────────────

/// Open a deferred-redecode batch on the save handle.
///
/// In normal mode every length-changing mutation
/// (`crimson_save_list_insert_element`,
/// `crimson_save_set_scalar_field_present`,
/// `crimson_save_dynamic_array_set_u32_elements`, …) runs the
/// `Body::write` + `Body::parse` + `decode_blocks` cycle (~25 ms on a
/// typical 1.07 save). Each scalar mutation also runs `decode_blocks`.
/// Workflows that need many mutations in sequence (e.g.
/// "complete all 141 held abyss-artifact challenges", which fires
/// **3 length-changing calls per challenge × 141 ≈ 423 re-decodes**)
/// pay 10+ seconds of `decode_blocks` time.
///
/// While a batch is open, every mutation entry point mutates the
/// in-memory `blocks` tree directly and skips the re-decode tail. The
/// matching `crimson_save_end_deferred_redecode` runs a **single**
/// `encode + parse + decode_blocks` pass for the whole batch.
///
/// Read entry points keep working — `blocks` is always the
/// in-progress tree, so [`crimson_save_get_block_json`],
/// [`crimson_save_list_inventory_items`], etc. see the latest state.
/// `crimson_save_write_to_file` is rejected with `BATCH_IN_PROGRESS`
/// while a batch is open (`h.save.body` is stale until end).
///
/// `mutation_version` is bumped exactly **once** by a successful
/// `crimson_save_end_deferred_redecode` (regardless of how many
/// mutations ran inside the batch). `crimson_save_abort_deferred_redecode`
/// does not bump it.
///
/// Returns:
/// - `OK` on success.
/// - `BATCH_IN_PROGRESS` if a batch is already open. Pairing rule:
///   one outstanding `begin_*` allowed; nest by ending the outer
///   batch first.
/// - `NULL_ARG` on null handle.
///
/// # Safety
/// `handle` must be a live, exclusive handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_begin_deferred_redecode(
    handle: *mut CrimsonSaveHandle,
) -> i32 {
    if handle.is_null() {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let _ffi_guard = save_ffi_lock();
        let h = unsafe { &mut *handle };
        if h.is_deferred() {
            return error::BATCH_IN_PROGRESS;
        }
        h.deferred_state = Some(DeferredState {
            blocks_backup: h.blocks.clone(),
            version_at_begin: h.mutation_version,
        });
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Commit the deferred-redecode batch opened by
/// [`crimson_save_begin_deferred_redecode`].
///
/// Runs **one** `Body::write` + `Body::parse` + `decode_blocks` pass
/// across the accumulated in-memory tree. On success the handle's
/// `save.body`, `body`, and `blocks` are replaced atomically and
/// `mutation_version` bumps exactly once.
///
/// On encode / re-parse failure the batch is rolled back: `blocks` is
/// restored from the snapshot captured by `begin_*`, the mutation
/// counter is left at its pre-begin value, and the call returns
/// `MUTATION_INVALID`. Callers that hit `MUTATION_INVALID` should
/// treat the batch as if it had been aborted — `begin_*` again to
/// retry with different ops.
///
/// Returns:
/// - `OK` on success.
/// - `BATCH_NOT_OPEN` if no batch is currently open.
/// - `MUTATION_INVALID` if the accumulated tree fails to encode or
///   re-parse; the handle is rolled back to its pre-batch state.
/// - `NULL_ARG` on null handle.
///
/// # Safety
/// `handle` must be a live, exclusive handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_end_deferred_redecode(
    handle: *mut CrimsonSaveHandle,
) -> i32 {
    if handle.is_null() {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let _ffi_guard = save_ffi_lock();
        let h = unsafe { &mut *handle };
        let Some(state) = h.deferred_state.take() else {
            return error::BATCH_NOT_OPEN;
        };
        // Try to commit: encode_body(h.blocks) → fresh bytes → parse →
        // decode. On any step's failure, restore blocks from the
        // snapshot and surface MUTATION_INVALID.
        let new_body = match h.body.write(&h.save.body, &h.blocks) {
            Ok(b) => b,
            Err(_) => {
                h.blocks = state.blocks_backup;
                return error::MUTATION_INVALID;
            }
        };
        let new_body_parsed = match Body::parse(&new_body) {
            Ok(b) => b,
            Err(_) => {
                h.blocks = state.blocks_backup;
                return error::MUTATION_INVALID;
            }
        };
        let new_blocks = new_body_parsed.decode_blocks(&new_body);
        h.save.body = new_body;
        h.body = new_body_parsed;
        h.blocks = new_blocks;
        h.bump_version();
        let _ = state.version_at_begin; // captured but unused on the success path
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Abort the deferred-redecode batch opened by
/// [`crimson_save_begin_deferred_redecode`], discarding every
/// in-memory mutation since `begin_*` and restoring the snapshot.
///
/// `mutation_version` is reset to its pre-begin value (an aborted
/// batch is observationally identical to one that never opened).
///
/// Returns:
/// - `OK` on success.
/// - `BATCH_NOT_OPEN` if no batch is open.
/// - `NULL_ARG` on null handle.
///
/// # Safety
/// `handle` must be a live, exclusive handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_abort_deferred_redecode(
    handle: *mut CrimsonSaveHandle,
) -> i32 {
    if handle.is_null() {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let _ffi_guard = save_ffi_lock();
        let h = unsafe { &mut *handle };
        let Some(state) = h.deferred_state.take() else {
            return error::BATCH_NOT_OPEN;
        };
        h.blocks = state.blocks_backup;
        h.mutation_version = state.version_at_begin;
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Returns `1` (in `*out_open`) if a deferred-redecode batch is
/// currently open on the handle, `0` otherwise. Useful for editor
/// code that re-enters from an event loop and needs to find out
/// whether a partial batch is still around.
///
/// # Safety
/// `handle` must be a live, exclusive handle. `out_open` must point
/// at a writable `i32`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_save_is_deferred_redecode_open(
    handle: *const CrimsonSaveHandle,
    out_open: *mut i32,
) -> i32 {
    if handle.is_null() || out_open.is_null() {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let _ffi_guard = save_ffi_lock();
        let h = unsafe { &*handle };
        unsafe { *out_open = if h.is_deferred() { 1 } else { 0 } };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

// ── Internal helpers ───────────────────────────────────────────────────────

/// Process-global serialization lock for the `crimson_save_*` C ABI.
///
/// The opaque `*mut CrimsonSaveHandle` the C side holds carries no interior
/// synchronization, and the handle's `&mut`/`&` borrows are formed straight
/// from that raw pointer. If two threads enter the save ABI on the same
/// handle at once — e.g. the C# app's background bulk-mutation worker
/// overlapping a UI-thread block read — they would hold a `&mut` and a `&`
/// to the same `Save`/`Body`/`Vec<ObjectBlock>` simultaneously, which is
/// undefined behaviour (and can free the blocks vec out from under an
/// in-flight reader).
///
/// Every save entry point takes this lock for the duration of its
/// `&mut`/`&` access via [`save_ffi_lock`], so those borrows are never live
/// on two threads at once. The lock is process-global rather than
/// per-handle: it lives outside the handle data (so it can be held *before*
/// the raw pointer is dereferenced — a per-handle field would have to be
/// reached through the very `&*handle` it is meant to guard), and in
/// practice the app keeps a single save open, so global ≈ per-handle. The
/// read-only catalog bridges (iteminfo/missioninfo/…) deliberately do NOT
/// take it — they are immutable after load and must not stall behind a save
/// bulk-op.
static SAVE_FFI_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Acquire [`SAVE_FFI_LOCK`], recovering from poison.
///
/// A panic mid-mutation is already surfaced to the caller as `PANIC` (the
/// `catch_unwind` wrapper), and the length-changing mutations roll their
/// state back on error, so a poisoned lock should not wedge every later
/// call — recover the guard and carry on.
fn save_ffi_lock() -> std::sync::MutexGuard<'static, ()> {
    SAVE_FFI_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn with_handle<T, F>(handle: *const CrimsonSaveHandle, out: *mut T, body: F) -> i32
where
    F: FnOnce(&CrimsonSaveHandle, *mut T) -> i32,
{
    if handle.is_null() || out.is_null() {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let _ffi_guard = save_ffi_lock();
        let h = unsafe { &*handle };
        body(h, out)
    }))
    .unwrap_or(error::PANIC)
}

fn count_fields(block: &ObjectBlock) -> (u32, u32) {
    let mut present = 0u32;
    let mut decoded = 0u32;
    for f in &block.fields {
        if f.present {
            present += 1;
            if !matches!(f.kind, FieldKind::Absent | FieldKind::Unknown) {
                decoded += 1;
            }
        }
    }
    (present, decoded)
}

fn save_error_code(e: &SaveError) -> i32 {
    match e {
        SaveError::Io(_) => error::IO,
        SaveError::TooSmall { .. } => error::TOO_SMALL,
        SaveError::BadMagic(_) => error::BAD_MAGIC,
        SaveError::PayloadOutOfRange { .. } => error::PAYLOAD_OUT_OF_RANGE,
        SaveError::UnsupportedVersion(_) => error::UNSUPPORTED_VERSION,
        SaveError::DecompressSizeMismatch { .. } | SaveError::Lz4(_) => error::DECOMPRESS,
    }
}

// ── JSON serialization ─────────────────────────────────────────────────────
//
// Hand-rolled to avoid pulling serde + serde_json into the cdylib. The
// shape is small and stable; if it grows past a few dozen lines, swap to
// serde_json under a feature.

fn format_block_json(b: &ObjectBlock) -> String {
    let mut s = String::with_capacity(256 + b.fields.len() * 96);
    write_block_json(&mut s, b);
    s
}

fn write_block_json(s: &mut String, b: &ObjectBlock) {
    s.push('{');
    write!(s, "\"class_index\":{},", b.class_index).unwrap();
    s.push_str("\"class_name\":");
    write_json_string(s, &b.class_name);
    s.push(',');
    write!(s, "\"data_offset\":{},", b.data_offset).unwrap();
    write!(s, "\"data_size\":{},", b.data_size).unwrap();

    s.push_str("\"mask_bytes_hex\":");
    write_json_hex(s, &b.mask_bytes);
    s.push(',');

    s.push_str("\"trailing_pad_hex\":");
    write_json_hex(s, &b.trailing_pad);
    s.push(',');

    s.push_str("\"fields\":[");
    for (i, f) in b.fields.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        write_field_json(s, f);
    }
    s.push_str("],");

    s.push_str("\"undecoded_ranges\":[");
    for (i, (start, end)) in b.undecoded_ranges.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        write!(s, "[{start},{end}]").unwrap();
    }
    s.push(']');

    s.push('}');
}

fn write_field_json(s: &mut String, f: &DecodedField) {
    s.push('{');
    write!(s, "\"field_index\":{},", f.field_index).unwrap();

    s.push_str("\"name\":");
    write_json_string(s, &f.name);
    s.push(',');

    s.push_str("\"type_name\":");
    write_json_string(s, &f.type_name);
    s.push(',');

    write!(s, "\"meta_kind\":{},", f.meta_kind).unwrap();
    write!(s, "\"meta_size\":{},", f.meta_size).unwrap();
    write!(s, "\"meta_aux\":{},", f.meta_aux).unwrap();
    write!(s, "\"present\":{},", f.present).unwrap();

    s.push_str("\"kind\":");
    write_json_string(s, f.kind.as_str());
    s.push(',');

    s.push_str("\"value\":");
    let formatted = format_field_value(f);
    write_json_string(s, &formatted);
    s.push(',');

    write!(s, "\"start\":{},", f.start).unwrap();
    write!(s, "\"end\":{},", f.end).unwrap();

    s.push_str("\"note\":");
    write_json_string(s, &f.note);

    // Nested-block payload. `child` is populated when the field is an
    // inline object_locator with a resolvable inline child; `elements`
    // is populated for object_list fields (possibly empty). Both are
    // `null` otherwise so the C# side can rely on a stable shape.
    s.push_str(",\"child\":");
    match &f.value {
        FieldValue::Locator { child: Some(c), .. } => write_block_json(s, c),
        _ => s.push_str("null"),
    }

    s.push_str(",\"elements\":");
    match &f.value {
        FieldValue::ObjectList { elements, .. } => {
            s.push('[');
            for (i, el) in elements.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                write_block_json(s, el);
            }
            s.push(']');
        }
        _ => s.push_str("null"),
    }

    s.push('}');
}

/// Pre-formatted, human-readable value mirroring Python's
/// `format_field_value` in `inspect_save_section.py`. Empty when the
/// field is absent / unknown.
fn format_field_value(f: &DecodedField) -> String {
    match (&f.kind, &f.value) {
        (FieldKind::FixedPrefix | FieldKind::FixedSuffix, FieldValue::Scalar(v)) => {
            let (val, ty) = format_scalar(v);
            format!("{val} <{ty}>")
        }
        (FieldKind::InlineBytes, FieldValue::InlineBytes { count, bytes }) => {
            format!("<{count} items, {} bytes>", bytes.len())
        }
        (FieldKind::DynamicArray, FieldValue::DynamicArray { count, bytes, header_variant, .. }) => {
            // Render contents inline when the element width is a simple
            // primitive (4-byte u32 or 8-byte u64). Sizes outside that
            // set fall back to the legacy `<N items, X bytes, variant>`
            // summary plus a short hex preview so the editor can still
            // see something. This is a display-only convenience — the
            // typed read API (`crimson_save_dynamic_array_get_u32_elements`)
            // remains the canonical accessor.
            format_dynamic_array(*count, bytes, header_variant, f.meta_size)
        }
        (FieldKind::ObjectLocator, FieldValue::Locator { child_type_name, child_payload_offset, child, .. }) => {
            match child {
                Some(c) => format!(
                    "-> {child_type_name} (offset {child_payload_offset}) inline -> {} fields",
                    c.fields.len()
                ),
                None => format!("-> {child_type_name} (offset {child_payload_offset})"),
            }
        }
        (FieldKind::ObjectList, FieldValue::ObjectList { count, header_variant, .. }) => {
            format!("[{count} elements, variant={header_variant}]")
        }
        (FieldKind::Absent, _) => "(absent)".to_string(),
        (FieldKind::Unknown, _) => "<unknown>".to_string(),
        _ => String::new(),
    }
}

/// Render a `DynamicArray` value as a human-readable string with its
/// element contents inlined when the element type is a simple primitive.
///
/// Display contract (matches the existing `format_scalar` `value <kind>`
/// shape):
///
/// - `meta_size == 4` → `[v1, v2, …] <u32_dynamic_array, variant>`. Up
///   to 12 elements rendered; longer arrays get a `…` continuation
///   marker plus the total count.
/// - `meta_size == 8` → `[v1, v2, …] <u64_dynamic_array, variant>`.
/// - Other sizes → legacy summary `<N items, X bytes, variant>` plus a
///   hex preview of the first 24 payload bytes.
///
/// Caveat: the typed read API
/// [`crimson_save_dynamic_array_get_u32_elements`] remains the canonical
/// accessor — the display string here is best-effort and not part of
/// the stable JSON value contract. Callers parsing JSON should still
/// rely on the field's `bytes`/`count` payload for round-trip safety.
fn format_dynamic_array(
    count: u32,
    bytes: &[u8],
    header_variant: &'static str,
    meta_size: u16,
) -> String {
    const MAX_INLINE: usize = 12;
    match meta_size {
        4 if bytes.len() == count as usize * 4 => {
            let mut s = String::with_capacity(64);
            s.push('[');
            let n = (count as usize).min(MAX_INLINE);
            for i in 0..n {
                if i > 0 {
                    s.push_str(", ");
                }
                let off = i * 4;
                let v = u32::from_le_bytes([
                    bytes[off],
                    bytes[off + 1],
                    bytes[off + 2],
                    bytes[off + 3],
                ]);
                write!(&mut s, "{v}").unwrap();
            }
            if (count as usize) > MAX_INLINE {
                let rest = count as usize - MAX_INLINE;
                write!(&mut s, ", … ({rest} more)").unwrap();
            }
            s.push(']');
            write!(&mut s, " <u32_dynamic_array, {header_variant}>").unwrap();
            s
        }
        8 if bytes.len() == count as usize * 8 => {
            let mut s = String::with_capacity(80);
            s.push('[');
            let n = (count as usize).min(MAX_INLINE);
            for i in 0..n {
                if i > 0 {
                    s.push_str(", ");
                }
                let off = i * 8;
                let v = u64::from_le_bytes([
                    bytes[off],
                    bytes[off + 1],
                    bytes[off + 2],
                    bytes[off + 3],
                    bytes[off + 4],
                    bytes[off + 5],
                    bytes[off + 6],
                    bytes[off + 7],
                ]);
                write!(&mut s, "{v}").unwrap();
            }
            if (count as usize) > MAX_INLINE {
                let rest = count as usize - MAX_INLINE;
                write!(&mut s, ", … ({rest} more)").unwrap();
            }
            s.push(']');
            write!(&mut s, " <u64_dynamic_array, {header_variant}>").unwrap();
            s
        }
        _ => {
            // Unknown / unusual element width — keep the legacy summary
            // line and tack on a short hex preview for diagnostics.
            let preview_n = bytes.len().min(24);
            let mut preview = String::with_capacity(preview_n * 2 + 4);
            for b in &bytes[..preview_n] {
                write!(&mut preview, "{b:02x}").unwrap();
            }
            if bytes.len() > preview_n {
                preview.push('…');
            }
            format!(
                "<{count} items, {} bytes, {header_variant}, hex={preview}>",
                bytes.len()
            )
        }
    }
}

fn format_scalar(v: &ScalarValue) -> (String, &'static str) {
    match v {
        ScalarValue::Bool(b) => ((*b != 0).to_string(), "bool"),
        ScalarValue::U8(n)   => (n.to_string(), "u8"),
        ScalarValue::U16(n)  => (n.to_string(), "u16"),
        ScalarValue::U32(n)  => (n.to_string(), "u32"),
        ScalarValue::U64(n)  => (n.to_string(), "u64"),
        ScalarValue::I8(n)   => (n.to_string(), "i8"),
        ScalarValue::I16(n)  => (n.to_string(), "i16"),
        ScalarValue::I32(n)  => (n.to_string(), "i32"),
        ScalarValue::I64(n)  => (n.to_string(), "i64"),
        ScalarValue::F32(n)  => (format!("{n}"), "f32"),
        ScalarValue::F64(n)  => (format!("{n}"), "f64"),
        ScalarValue::F32x3(xs) => (
            format!("[{}, {}, {}]", xs[0], xs[1], xs[2]),
            "f32x3",
        ),
        ScalarValue::F32x4(xs) => (
            format!("[{}, {}, {}, {}]", xs[0], xs[1], xs[2], xs[3]),
            "f32x4",
        ),
        ScalarValue::U32x4(xs) => (
            format!(
                "[0x{:08x}, 0x{:08x}, 0x{:08x}, 0x{:08x}]",
                xs[0], xs[1], xs[2], xs[3]
            ),
            "u32x4",
        ),
        ScalarValue::Bytes(b) => (format!("{} bytes", b.len()), "bytes"),
    }
}

fn write_json_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"'  => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                write!(out, "\\u{:04x}", c as u32).unwrap();
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write_json_hex(out: &mut String, bytes: &[u8]) {
    out.push('"');
    for b in bytes {
        write!(out, "{b:02x}").unwrap();
    }
    out.push('"');
}

#[cfg(test)]
mod format_dynamic_array_tests {
    use super::format_dynamic_array;

    #[test]
    fn empty_u32_array() {
        let s = format_dynamic_array(0, &[], "prefix_00xx0100", 4);
        assert_eq!(s, "[] <u32_dynamic_array, prefix_00xx0100>");
    }

    #[test]
    fn three_u32_elements_inlined() {
        // [1, 2, 3] LE → 12 bytes
        let bytes = [
            1, 0, 0, 0, // 1
            2, 0, 0, 0, // 2
            3, 0, 0, 0, // 3
        ];
        let s = format_dynamic_array(3, &bytes, "marker_prefix", 4);
        assert_eq!(s, "[1, 2, 3] <u32_dynamic_array, marker_prefix>");
    }

    #[test]
    fn truncates_at_max_inline_with_count_marker() {
        // 14 u32 elements — should inline 12, then "… (2 more)".
        let mut bytes = Vec::with_capacity(14 * 4);
        for i in 0u32..14 {
            bytes.extend_from_slice(&i.to_le_bytes());
        }
        let s = format_dynamic_array(14, &bytes, "marker_prefix", 4);
        assert!(
            s.starts_with("[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, … (2 more)]"),
            "got {s:?}"
        );
        assert!(s.ends_with("<u32_dynamic_array, marker_prefix>"));
    }

    #[test]
    fn u64_array_inlined() {
        let bytes = [
            // u64 = 0x0000000100000000 = 4294967296
            0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
        ];
        let s = format_dynamic_array(1, &bytes, "marker_prefix", 8);
        assert_eq!(s, "[4294967296] <u64_dynamic_array, marker_prefix>");
    }

    #[test]
    fn unusual_meta_size_falls_back_to_hex() {
        // meta_size = 16 (e.g. uint4 SceneObjectUuid in a dynamic array).
        let bytes = (0u8..16).collect::<Vec<u8>>();
        let s = format_dynamic_array(1, &bytes, "marker_prefix", 16);
        assert!(
            s.starts_with("<1 items, 16 bytes, marker_prefix, hex=000102030405060708090a0b0c0d0e0f"),
            "got {s:?}"
        );
    }

    #[test]
    fn corrupt_count_falls_back_to_legacy_summary() {
        // bytes.len() doesn't match count * meta_size → don't try to
        // decode, surface the legacy summary so the editor sees the
        // anomaly rather than reading past the buffer.
        let bytes = [1u8, 2, 3]; // 3 bytes claimed as 1 u32
        let s = format_dynamic_array(1, &bytes, "marker_prefix", 4);
        assert!(s.starts_with("<1 items, 3 bytes, marker_prefix, hex="), "got {s:?}");
    }
}

#[cfg(test)]
mod tests {
    //! End-to-end smoke test that drives the C ABI exactly as a native
    //! caller would: load → query → enumerate → free. Skips cleanly when
    //! no live save file is present (CI / fresh machines).

    use super::*;
    use std::ffi::CString;
    use std::path::PathBuf;
    use std::ptr;

    fn find_save() -> Option<PathBuf> {
        // The save-test fixture is slot107 — the current Crimson Desert 1.12
        // save. Pinning every save test to it keeps the mutation / round-trip
        // suite validating against the live latest-patch save format.
        let local = std::env::var_os("LOCALAPPDATA")?;
        let root = PathBuf::from(local).join("Pearl Abyss/CD/save");
        for user in std::fs::read_dir(&root).ok()?.flatten() {
            let p = user.path().join("slot107").join("save.save");
            if p.is_file() {
                return Some(p);
            }
        }
        None
    }

    #[test]
    fn c_abi_smoke() {
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_smoke: no live save under %LOCALAPPDATA%");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();

        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        let rc = unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) };
        assert_eq!(rc, error::OK, "load failed with code {rc}");
        assert!(!handle.is_null());

        let mut version: u16 = 0;
        assert_eq!(unsafe { crimson_save_get_version(handle, &mut version) }, error::OK);
        assert_eq!(version, 2, "expected save version 2");

        let mut hmac_ok: i32 = -1;
        assert_eq!(unsafe { crimson_save_get_hmac_ok(handle, &mut hmac_ok) }, error::OK);
        assert_eq!(hmac_ok, 1, "expected HMAC ok");

        let mut block_count: u32 = 0;
        assert_eq!(
            unsafe { crimson_save_get_block_count(handle, &mut block_count) },
            error::OK
        );
        assert!(block_count > 0, "expected at least one block");

        // First block: read info + class name (with the two-call pattern).
        let mut info = CrimsonBlockInfo::default();
        assert_eq!(
            unsafe { crimson_save_get_block_info(handle, 0, &mut info) },
            error::OK
        );
        assert_eq!(info.fields_present, info.fields_decoded);

        let mut needed: usize = 0;
        let rc = unsafe {
            crimson_save_get_block_class_name(handle, 0, ptr::null_mut(), 0, &mut needed)
        };
        assert_eq!(rc, error::BUFFER_TOO_SMALL);
        assert!(needed > 1);

        let mut buf = vec![0u8; needed];
        let rc = unsafe {
            crimson_save_get_block_class_name(
                handle,
                0,
                buf.as_mut_ptr(),
                buf.len(),
                ptr::null_mut(),
            )
        };
        assert_eq!(rc, error::OK);
        assert_eq!(*buf.last().unwrap(), 0, "expected NUL terminator");
        let name = std::str::from_utf8(&buf[..needed - 1]).unwrap();
        assert!(!name.is_empty());

        // Block JSON: query size, then read, sanity-check content.
        let mut needed: usize = 0;
        let rc = unsafe {
            crimson_save_get_block_json(handle, 0, ptr::null_mut(), 0, &mut needed)
        };
        assert_eq!(rc, error::BUFFER_TOO_SMALL);
        assert!(needed > 16, "expected non-trivial JSON, got {needed} bytes");
        let mut json_buf = vec![0u8; needed];
        let rc = unsafe {
            crimson_save_get_block_json(
                handle,
                0,
                json_buf.as_mut_ptr(),
                json_buf.len(),
                ptr::null_mut(),
            )
        };
        assert_eq!(rc, error::OK);
        assert_eq!(*json_buf.last().unwrap(), 0);
        let json = std::str::from_utf8(&json_buf[..needed - 1]).unwrap();
        assert!(json.starts_with('{') && json.ends_with('}'), "json shape: {json:.120}…");
        for needle in [
            "\"class_index\":",
            "\"class_name\":",
            "\"data_offset\":",
            "\"mask_bytes_hex\":",
            "\"trailing_pad_hex\":",
            "\"fields\":[",
            "\"undecoded_ranges\":[",
            "\"child\":",
            "\"elements\":",
        ] {
            assert!(json.contains(needle), "missing {needle:?} in {json:.200}…");
        }

        // Block JSON out-of-range -> proper error code.
        let rc = unsafe {
            crimson_save_get_block_json(handle, u32::MAX, ptr::null_mut(), 0, &mut needed)
        };
        assert_eq!(rc, error::OUT_OF_RANGE);

        // Out-of-range index on block_info returns the right code.
        let rc = unsafe { crimson_save_get_block_info(handle, u32::MAX, &mut info) };
        assert_eq!(rc, error::OUT_OF_RANGE);

        // NULL arg validation.
        let rc = unsafe { crimson_save_get_version(ptr::null(), &mut version) };
        assert_eq!(rc, error::NULL_ARG);

        unsafe { crimson_save_free(handle) };
    }

    /// Drives the new mutation API end-to-end against a live save:
    /// mutate block 0 field 0 (a fixed_suffix u32 in
    /// CharacterStatusSaveData), assert the in-memory JSON reflects the
    /// new value, write to a tempfile, reload that tempfile, and assert
    /// the value survived the encrypt → LZ4 → HMAC → decrypt round-trip.
    /// Then exercises every error path on the same handle.
    #[test]
    fn c_abi_mutate_and_write_roundtrip() {
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_mutate_and_write_roundtrip: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();

        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );

        // Confirm block 0 field 0 is the fixed_suffix u32 (_characterKey)
        // we're about to overwrite. If the schema ever drifts this assert
        // will tell us we picked the wrong target.
        let json = read_block_json(handle, 0);
        assert!(
            json.contains("\"field_index\":0") && json.contains("\"kind\":\"fixed_suffix\""),
            "expected block 0 field 0 to be fixed_suffix; got: {json:.200}…"
        );

        // 0x01EFCDAB = 32_492_971. Specific enough to be distinguishable
        // from any plausible original value.
        let sentinel: [u8; 4] = [0xAB, 0xCD, 0xEF, 0x01];
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_field(handle, 0, 0, sentinel.as_ptr(), sentinel.len())
            },
            error::OK
        );
        // After the set, get_block_json must reflect the new decoded value.
        let after = read_block_json(handle, 0);
        assert!(
            after.contains("\"value\":\"32492971 <u32>\""),
            "expected sentinel value in JSON after set; got: {after:.300}…"
        );

        // Write the modified handle to a temp file and reload it. The
        // tempfile crate's NamedTempFile deletes the file on drop, so we
        // keep `_tmp` alive until the reload completes.
        let _tmp = tempfile::NamedTempFile::new().unwrap();
        let tmp_path_str = _tmp.path().to_str().unwrap().to_owned();
        let tmp_path = CString::new(tmp_path_str.clone()).unwrap();
        assert_eq!(
            unsafe { crimson_save_write_to_file(handle, tmp_path.as_ptr()) },
            error::OK
        );

        unsafe { crimson_save_free(handle) };

        let mut reloaded: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(tmp_path.as_ptr(), &mut reloaded) },
            error::OK
        );
        let mut hmac: i32 = 0;
        assert_eq!(
            unsafe { crimson_save_get_hmac_ok(reloaded, &mut hmac) },
            error::OK
        );
        assert_eq!(hmac, 1, "HMAC must verify on reload of a write_to_file output");

        let reloaded_json = read_block_json(reloaded, 0);
        assert!(
            reloaded_json.contains("\"value\":\"32492971 <u32>\""),
            "mutation must persist across write+reload; got: {reloaded_json:.300}…"
        );
        unsafe { crimson_save_free(reloaded) };

        // ── Error paths ────────────────────────────────────────────────
        let mut h2: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut h2) },
            error::OK
        );
        let dummy = [0u8; 4];

        // NOT_SCALAR: target an absent field. Block 0 field 3 (_experience)
        // is absent in any low-level character.
        assert_eq!(
            unsafe { crimson_save_set_scalar_field(h2, 0, 3, dummy.as_ptr(), 0) },
            error::NOT_SCALAR
        );

        // LENGTH_MISMATCH: field 0 is 4 bytes; send 5.
        let dummy5 = [0u8; 5];
        assert_eq!(
            unsafe { crimson_save_set_scalar_field(h2, 0, 0, dummy5.as_ptr(), 5) },
            error::LENGTH_MISMATCH
        );

        // OUT_OF_RANGE on both axes.
        assert_eq!(
            unsafe { crimson_save_set_scalar_field(h2, 0, u32::MAX, dummy.as_ptr(), 4) },
            error::OUT_OF_RANGE
        );
        assert_eq!(
            unsafe { crimson_save_set_scalar_field(h2, u32::MAX, 0, dummy.as_ptr(), 4) },
            error::OUT_OF_RANGE
        );

        // NULL_ARG on handle and on bytes.
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_field(ptr::null_mut(), 0, 0, dummy.as_ptr(), 4)
            },
            error::NULL_ARG
        );
        assert_eq!(
            unsafe { crimson_save_set_scalar_field(h2, 0, 0, ptr::null(), 4) },
            error::NULL_ARG
        );

        // write_to_file with a NULL path is also NULL_ARG.
        assert_eq!(
            unsafe { crimson_save_write_to_file(h2, ptr::null()) },
            error::NULL_ARG
        );

        unsafe { crimson_save_free(h2) };
    }

    fn read_block_json(handle: *mut CrimsonSaveHandle, idx: u32) -> String {
        let mut needed: usize = 0;
        let _ = unsafe {
            crimson_save_get_block_json(handle, idx, ptr::null_mut(), 0, &mut needed)
        };
        let mut buf = vec![0u8; needed];
        let rc = unsafe {
            crimson_save_get_block_json(handle, idx, buf.as_mut_ptr(), buf.len(), ptr::null_mut())
        };
        assert_eq!(rc, error::OK);
        String::from_utf8(buf[..needed - 1].to_vec()).unwrap()
    }

    /// Locate a (block, path, leaf, current_value, scalar_byte_len) tuple
    /// pointing at a u32-shaped scalar reachable via a one-step descent
    /// (either through a Locator's inline child or the first element of
    /// an ObjectList). Returns `None` if nothing in the save matches —
    /// the live-save assertion below handles that case explicitly.
    fn find_nested_u32_scalar(
        handle: *mut CrimsonSaveHandle,
    ) -> Option<(u32, CrimsonPathStep, u32, u32, usize)> {
        let h = unsafe { &*handle };
        for (block_idx, block) in h.blocks.iter().enumerate() {
            for (parent_field_idx, parent_field) in block.fields.iter().enumerate() {
                // Inline locator → walk the child.
                if let FieldValue::Locator {
                    child: Some(child), ..
                } = &parent_field.value
                    && let Some(leaf) = pick_scalar_field(child)
                {
                    return Some((
                        block_idx as u32,
                        CrimsonPathStep {
                            field_idx: parent_field_idx as u32,
                            element_idx: 0,
                        },
                        leaf.0,
                        leaf.1,
                        leaf.2,
                    ));
                }
                // ObjectList → walk the first element.
                if let FieldValue::ObjectList { elements, .. } = &parent_field.value
                    && let Some(first) = elements.first()
                    && let Some(leaf) = pick_scalar_field(first)
                {
                    return Some((
                        block_idx as u32,
                        CrimsonPathStep {
                            field_idx: parent_field_idx as u32,
                            element_idx: 0,
                        },
                        leaf.0,
                        leaf.1,
                        leaf.2,
                    ));
                }
            }
        }
        None
    }

    /// Pick a u32-shaped FixedPrefix/FixedSuffix field from `block`.
    /// Returns (field_idx, current_u32_value, byte_len). Restricting to
    /// u32 keeps the test independent of which exact field we land on —
    /// we just need predictable bit width for the sentinel.
    fn pick_scalar_field(block: &ObjectBlock) -> Option<(u32, u32, usize)> {
        for (idx, f) in block.fields.iter().enumerate() {
            if !matches!(f.kind, FieldKind::FixedPrefix | FieldKind::FixedSuffix) {
                continue;
            }
            let len = f.end.saturating_sub(f.start);
            if len != 4 {
                continue;
            }
            if let FieldValue::Scalar(ScalarValue::U32(v)) = &f.value {
                return Some((idx as u32, *v, len));
            }
        }
        None
    }

    #[test]
    fn c_abi_set_scalar_field_path() {
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_set_scalar_field_path: no live save under %LOCALAPPDATA%");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();

        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );

        // ── Empty-path parity: path_len=0 must behave identically to
        // the top-level setter. Block 0 field 0 is the _characterKey
        // fixed_suffix u32 the existing tests already exercise.
        let sentinel: [u8; 4] = [0xAB, 0xCD, 0xEF, 0x01];
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_field_path(
                    handle,
                    0,
                    ptr::null(),
                    0,
                    0,
                    sentinel.as_ptr(),
                    sentinel.len(),
                )
            },
            error::OK
        );
        let after_top = read_block_json(handle, 0);
        assert!(
            after_top.contains("\"value\":\"32492971 <u32>\""),
            "empty-path mutation must equal the top-level setter; got: {after_top:.300}…"
        );

        // ── Find a nested scalar and mutate it.
        let Some((block_idx, step, leaf_idx, original, len)) = find_nested_u32_scalar(handle)
        else {
            // Any non-trivial save has at least one InventorySaveData /
            // EquipmentSaveData with nested scalars. If we hit this we
            // want to know — fail loudly rather than silently skip.
            unsafe { crimson_save_free(handle) };
            panic!("expected a nested u32 scalar in a live save; schema or fixture drifted");
        };
        assert_eq!(len, 4, "find_nested_u32_scalar contract");

        // Pick a sentinel guaranteed to differ from the original value.
        let nested_sentinel: u32 = original.wrapping_add(0x0BAD_F00D);
        let nested_bytes = nested_sentinel.to_le_bytes();
        let steps = [step];
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_field_path(
                    handle,
                    block_idx,
                    steps.as_ptr(),
                    steps.len(),
                    leaf_idx,
                    nested_bytes.as_ptr(),
                    nested_bytes.len(),
                )
            },
            error::OK,
            "nested-path mutation must succeed (block={block_idx}, parent_field={}, leaf={leaf_idx}, original=0x{:08x})",
            step.field_idx, original
        );

        // The mutation must round-trip through write_to_file + reload —
        // i.e. survive HMAC / ChaCha20 / LZ4 re-emission.
        let _tmp = tempfile::NamedTempFile::new().unwrap();
        let tmp_path_str = _tmp.path().to_str().unwrap().to_owned();
        let tmp_path = CString::new(tmp_path_str).unwrap();
        assert_eq!(
            unsafe { crimson_save_write_to_file(handle, tmp_path.as_ptr()) },
            error::OK
        );
        unsafe { crimson_save_free(handle) };

        let mut reloaded: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(tmp_path.as_ptr(), &mut reloaded) },
            error::OK
        );
        let mut hmac: i32 = 0;
        assert_eq!(
            unsafe { crimson_save_get_hmac_ok(reloaded, &mut hmac) },
            error::OK
        );
        assert_eq!(hmac, 1, "HMAC must verify on reload after nested-path mutation");

        // Confirm the reloaded copy carries the nested sentinel where
        // we left it. Re-discover the path so this test stays robust to
        // any schema drift (we relocate the same logical position from
        // the in-memory tree, not from cached indices).
        let reloaded_target = find_nested_u32_scalar(reloaded);
        unsafe { crimson_save_free(reloaded) };
        let (_, _, _, post_value, _) = reloaded_target.expect("nested scalar still findable post-reload");
        assert_eq!(
            post_value, nested_sentinel,
            "reloaded save must carry the nested sentinel"
        );

        // ── Error paths ────────────────────────────────────────────────
        let mut h2: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut h2) },
            error::OK
        );
        let dummy = [0u8; 4];

        // NOT_NAVIGABLE: target block 0 field 0 (a scalar) as a mid-path
        // step. Walking *into* a scalar is the canonical "not navigable"
        // failure.
        let bad_steps = [CrimsonPathStep {
            field_idx: 0,
            element_idx: 0,
        }];
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_field_path(
                    h2,
                    0,
                    bad_steps.as_ptr(),
                    bad_steps.len(),
                    0,
                    dummy.as_ptr(),
                    dummy.len(),
                )
            },
            error::NOT_NAVIGABLE
        );

        // OUT_OF_RANGE on a path step's field_idx.
        let oor_step = [CrimsonPathStep {
            field_idx: u32::MAX,
            element_idx: 0,
        }];
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_field_path(
                    h2,
                    0,
                    oor_step.as_ptr(),
                    oor_step.len(),
                    0,
                    dummy.as_ptr(),
                    dummy.len(),
                )
            },
            error::OUT_OF_RANGE
        );

        // NULL_ARG: null handle.
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_field_path(
                    ptr::null_mut(),
                    0,
                    ptr::null(),
                    0,
                    0,
                    dummy.as_ptr(),
                    dummy.len(),
                )
            },
            error::NULL_ARG
        );

        // NULL_ARG: null path with non-zero path_len.
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_field_path(
                    h2,
                    0,
                    ptr::null(),
                    1,
                    0,
                    dummy.as_ptr(),
                    dummy.len(),
                )
            },
            error::NULL_ARG
        );

        // NULL_ARG: null bytes with non-zero bytes_len.
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_field_path(
                    h2,
                    0,
                    ptr::null(),
                    0,
                    0,
                    ptr::null(),
                    4,
                )
            },
            error::NULL_ARG
        );

        unsafe { crimson_save_free(h2) };
    }

    /// Collect every top-level u32 FixedPrefix/FixedSuffix scalar in the
    /// save, paired with its current value. Returned as
    /// `(block_idx, field_idx, current_u32)`. Used by the batch tests to
    /// build many-op fixtures without inventing schema knowledge.
    fn collect_top_level_u32_scalars(
        handle: *mut CrimsonSaveHandle,
    ) -> Vec<(u32, u32, u32)> {
        let h = unsafe { &*handle };
        let mut out = Vec::new();
        for (block_idx, block) in h.blocks.iter().enumerate() {
            for (field_idx, f) in block.fields.iter().enumerate() {
                if !matches!(f.kind, FieldKind::FixedPrefix | FieldKind::FixedSuffix) {
                    continue;
                }
                let len = f.end.saturating_sub(f.start);
                if len != 4 {
                    continue;
                }
                if let FieldValue::Scalar(ScalarValue::U32(v)) = &f.value {
                    out.push((block_idx as u32, field_idx as u32, *v));
                }
            }
        }
        out
    }

    /// Drives the batch entry point with a mix of top-level + one nested
    /// op against a live save: apply, verify each value reflected in the
    /// decoded JSON, write to a tempfile, reload, and confirm every
    /// sentinel survived HMAC / ChaCha20 / LZ4 re-emission.
    #[test]
    fn c_abi_set_scalar_fields_batch_smoke() {
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_set_scalar_fields_batch_smoke: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();

        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );

        // Grab 5 top-level u32 scalars + 1 nested op. The nested op
        // exercises the path-traversal branch alongside the empty-path
        // ones so the batch covers both code paths in resolve_leaf_range.
        let mut top = collect_top_level_u32_scalars(handle);
        assert!(
            top.len() >= 5,
            "expected ≥5 top-level u32 scalars in live save; got {}",
            top.len()
        );
        top.truncate(5);
        let nested = find_nested_u32_scalar(handle)
            .expect("expected a nested u32 scalar in live save");

        // Build a stable mapping (block, path[], field) → sentinel bytes
        // that owns its buffers across the batch call. The ops slice
        // borrows from this storage.
        struct OpFixture {
            block_idx: u32,
            field_idx: u32,
            steps: Vec<CrimsonPathStep>,
            bytes: [u8; 4],
            expected_decoded: u32,
        }
        let mut fixtures: Vec<OpFixture> = Vec::new();
        for (i, (b, f, original)) in top.iter().enumerate() {
            let sentinel = original.wrapping_add(0x5EED_0000 + i as u32);
            fixtures.push(OpFixture {
                block_idx: *b,
                field_idx: *f,
                steps: Vec::new(),
                bytes: sentinel.to_le_bytes(),
                expected_decoded: sentinel,
            });
        }
        let (nb, nstep, nleaf, noriginal, _) = nested;
        let nested_sentinel = noriginal.wrapping_add(0xBADD_F00D);
        fixtures.push(OpFixture {
            block_idx: nb,
            field_idx: nleaf,
            steps: vec![nstep],
            bytes: nested_sentinel.to_le_bytes(),
            expected_decoded: nested_sentinel,
        });

        let ops: Vec<CrimsonScalarBatchOp> = fixtures
            .iter()
            .map(|fx| CrimsonScalarBatchOp {
                block_idx: fx.block_idx,
                field_idx: fx.field_idx,
                path: if fx.steps.is_empty() {
                    ptr::null()
                } else {
                    fx.steps.as_ptr()
                },
                path_len: fx.steps.len(),
                bytes: fx.bytes.as_ptr(),
                bytes_len: fx.bytes.len(),
            })
            .collect();

        let mut failed_idx: usize = 0;
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_fields_batch(
                    handle,
                    ops.as_ptr(),
                    ops.len(),
                    &mut failed_idx,
                )
            },
            error::OK
        );
        assert_eq!(
            failed_idx,
            usize::MAX,
            "OK return must write usize::MAX sentinel to out_failed_op_index"
        );

        // Every top-level op must be reflected in the decoded JSON.
        for fx in fixtures.iter().filter(|fx| fx.steps.is_empty()) {
            let json = read_block_json(handle, fx.block_idx);
            let needle = format!("\"value\":\"{} <u32>\"", fx.expected_decoded);
            assert!(
                json.contains(&needle),
                "top-level batch op (block={}, field={}) not visible in JSON: {json:.300}…",
                fx.block_idx,
                fx.field_idx
            );
        }

        // Round-trip via write_to_file + reload — every sentinel must
        // survive HMAC / ChaCha20 / LZ4 re-emission.
        let _tmp = tempfile::NamedTempFile::new().unwrap();
        let tmp_path = CString::new(_tmp.path().to_str().unwrap()).unwrap();
        assert_eq!(
            unsafe { crimson_save_write_to_file(handle, tmp_path.as_ptr()) },
            error::OK
        );
        unsafe { crimson_save_free(handle) };

        let mut reloaded: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(tmp_path.as_ptr(), &mut reloaded) },
            error::OK
        );
        let mut hmac: i32 = 0;
        assert_eq!(
            unsafe { crimson_save_get_hmac_ok(reloaded, &mut hmac) },
            error::OK
        );
        assert_eq!(hmac, 1, "HMAC must verify on reload after batch mutation");

        for fx in fixtures.iter().filter(|fx| fx.steps.is_empty()) {
            let json = read_block_json(reloaded, fx.block_idx);
            let needle = format!("\"value\":\"{} <u32>\"", fx.expected_decoded);
            assert!(
                json.contains(&needle),
                "reloaded JSON missing sentinel for block={}, field={}: {json:.300}…",
                fx.block_idx,
                fx.field_idx
            );
        }

        // Empty-batch: zero ops returns OK + writes the sentinel.
        let mut sentinel_slot: usize = 0;
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_fields_batch(
                    reloaded,
                    ptr::null(),
                    0,
                    &mut sentinel_slot,
                )
            },
            error::OK
        );
        assert_eq!(sentinel_slot, usize::MAX);

        unsafe { crimson_save_free(reloaded) };
    }

    /// Validate the all-or-nothing contract: if any op in the batch fails
    /// validation, the body must be left exactly as it was on entry, and
    /// `out_failed_op_index` must point at the offending op.
    #[test]
    fn c_abi_set_scalar_fields_batch_atomicity() {
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_set_scalar_fields_batch_atomicity: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();

        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );

        // Snapshot the entire body so we can compare bytes after the
        // failed batch and prove zero mutation happened.
        let body_before: Vec<u8> = {
            let h = unsafe { &*handle };
            h.save.body.clone()
        };

        let scalars = collect_top_level_u32_scalars(handle);
        assert!(
            scalars.len() >= 2,
            "expected ≥2 top-level u32 scalars for atomicity test"
        );
        let (b0, f0, v0) = scalars[0];
        let (b1, f1, _) = scalars[1];

        // Op 0: valid mutation. Op 1: same target but with bytes_len=3
        // — guaranteed LENGTH_MISMATCH because the field is u32 (4 bytes).
        let sentinel0 = v0.wrapping_add(0xDEAD_BEEF).to_le_bytes();
        let bad_bytes: [u8; 3] = [0, 0, 0];

        let ops = [
            CrimsonScalarBatchOp {
                block_idx: b0,
                field_idx: f0,
                path: ptr::null(),
                path_len: 0,
                bytes: sentinel0.as_ptr(),
                bytes_len: sentinel0.len(),
            },
            CrimsonScalarBatchOp {
                block_idx: b1,
                field_idx: f1,
                path: ptr::null(),
                path_len: 0,
                bytes: bad_bytes.as_ptr(),
                bytes_len: bad_bytes.len(),
            },
        ];

        let mut failed_idx: usize = 0;
        let rc = unsafe {
            crimson_save_set_scalar_fields_batch(
                handle,
                ops.as_ptr(),
                ops.len(),
                &mut failed_idx,
            )
        };
        assert_eq!(rc, error::LENGTH_MISMATCH);
        assert_eq!(failed_idx, 1, "failed_op_index must pinpoint the offending op");

        // The body must be byte-identical to entry — op 0 must NOT have
        // been applied just because validation reached it first.
        let body_after: &[u8] = unsafe { &(*handle).save.body };
        assert_eq!(
            body_after.len(),
            body_before.len(),
            "body length must not change on failed batch"
        );
        assert!(
            body_after == body_before.as_slice(),
            "body must be byte-identical to pre-batch on validation failure"
        );

        // Different error: NOT_NAVIGABLE via path that walks into a scalar.
        let bad_step = [CrimsonPathStep {
            field_idx: f0,
            element_idx: 0,
        }];
        let dummy: [u8; 4] = [0, 0, 0, 0];
        let ops2 = [CrimsonScalarBatchOp {
            block_idx: b0,
            field_idx: 0,
            path: bad_step.as_ptr(),
            path_len: bad_step.len(),
            bytes: dummy.as_ptr(),
            bytes_len: dummy.len(),
        }];
        let mut failed_idx2: usize = 0;
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_fields_batch(
                    handle,
                    ops2.as_ptr(),
                    ops2.len(),
                    &mut failed_idx2,
                )
            },
            error::NOT_NAVIGABLE
        );
        assert_eq!(failed_idx2, 0);

        // NULL_ARG: per-op null bytes with non-zero len must surface
        // through out_failed_op_index at the right index.
        let good_bytes = sentinel0;
        let ops3 = [
            CrimsonScalarBatchOp {
                block_idx: b0,
                field_idx: f0,
                path: ptr::null(),
                path_len: 0,
                bytes: good_bytes.as_ptr(),
                bytes_len: good_bytes.len(),
            },
            CrimsonScalarBatchOp {
                block_idx: b1,
                field_idx: f1,
                path: ptr::null(),
                path_len: 0,
                bytes: ptr::null(),
                bytes_len: 4,
            },
        ];
        let mut failed_idx3: usize = 0;
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_fields_batch(
                    handle,
                    ops3.as_ptr(),
                    ops3.len(),
                    &mut failed_idx3,
                )
            },
            error::NULL_ARG
        );
        assert_eq!(failed_idx3, 1);

        // NULL handle: top-level NULL_ARG; out_failed_op_index is not
        // touched (handle is null so we can't write back through it
        // safely, and the per-op pre-check never runs).
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_fields_batch(
                    ptr::null_mut(),
                    ops3.as_ptr(),
                    ops3.len(),
                    ptr::null_mut(),
                )
            },
            error::NULL_ARG
        );

        unsafe { crimson_save_free(handle) };
    }

    /// Equivalence check at scale: applying N ops through the batch
    /// entry point produces the byte-identical body that running the same
    /// N ops one-at-a-time through the single-op setter would. This is
    /// the soundness invariant the perf optimization rests on.
    #[test]
    fn c_abi_set_scalar_fields_batch_matches_single_op() {
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_set_scalar_fields_batch_matches_single_op: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();

        let mut handle_batch: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle_batch) },
            error::OK
        );
        let mut handle_single: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle_single) },
            error::OK
        );

        // Collect candidates from the batch handle; both handles parsed
        // the same file so positions are identical. Cap at 200 to keep
        // the test fast — 200 ops is already 5× the fill-stacks use
        // case and is plenty to exercise the batch path.
        let mut scalars = collect_top_level_u32_scalars(handle_batch);
        assert!(
            scalars.len() >= 50,
            "expected ≥50 top-level u32 scalars in live save; got {}",
            scalars.len()
        );
        scalars.truncate(200);
        let n = scalars.len();

        // Build (sentinel_bytes_owned, op) pairs. Each sentinel is a
        // deterministic mutation of the original so two ops on the same
        // target wouldn't accidentally land on the same byte pattern.
        let sentinels: Vec<[u8; 4]> = scalars
            .iter()
            .enumerate()
            .map(|(i, (_, _, v))| v.wrapping_add(0xC0DE_0000 + i as u32).to_le_bytes())
            .collect();
        let ops: Vec<CrimsonScalarBatchOp> = scalars
            .iter()
            .zip(sentinels.iter())
            .map(|((b, f, _), bytes)| CrimsonScalarBatchOp {
                block_idx: *b,
                field_idx: *f,
                path: ptr::null(),
                path_len: 0,
                bytes: bytes.as_ptr(),
                bytes_len: bytes.len(),
            })
            .collect();

        // Apply via batch on handle A.
        let mut failed_idx: usize = 0;
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_fields_batch(
                    handle_batch,
                    ops.as_ptr(),
                    ops.len(),
                    &mut failed_idx,
                )
            },
            error::OK,
            "batch of {n} ops should apply cleanly"
        );

        // Apply the same N ops one-at-a-time on handle B via the
        // pre-existing single-op setter.
        for ((b, f, _), bytes) in scalars.iter().zip(sentinels.iter()) {
            assert_eq!(
                unsafe {
                    crimson_save_set_scalar_field(
                        handle_single,
                        *b,
                        *f,
                        bytes.as_ptr(),
                        bytes.len(),
                    )
                },
                error::OK
            );
        }

        // Both bodies must be byte-identical. This is the equivalence
        // proof: batch == N × single-op, with one re-decode instead of N.
        let body_batch: &[u8] = unsafe { &(*handle_batch).save.body };
        let body_single: &[u8] = unsafe { &(*handle_single).save.body };
        assert_eq!(
            body_batch.len(),
            body_single.len(),
            "body lengths must match between batch and single-op handles"
        );
        assert!(
            body_batch == body_single,
            "batch body must be byte-identical to N × single-op body"
        );

        unsafe { crimson_save_free(handle_batch) };
        unsafe { crimson_save_free(handle_single) };
    }

    // ── Length-changing edits (Phase B.2) ──────────────────────────────────

    /// Find the first top-level (block_idx, field_idx) whose value is an
    /// `ObjectList` with `header_variant == "zero1_count_u24"` and at
    /// least one element. Returns `None` if none — all live 1.06 saves
    /// have multiple matches, so this is mostly a CI-environment guard.
    fn find_object_list(
        blocks: &[ObjectBlock],
    ) -> Option<(u32, u32)> {
        for (b_idx, block) in blocks.iter().enumerate() {
            for (f_idx, field) in block.fields.iter().enumerate() {
                if let FieldValue::ObjectList {
                    elements,
                    header_variant,
                    ..
                } = &field.value
                    && !elements.is_empty()
                    && *header_variant == "zero1_count_u24"
                {
                    return Some((b_idx as u32, f_idx as u32));
                }
            }
        }
        None
    }

    /// Clone an existing list element to a new position, then remove the
    /// clone. The save body should be byte-identical to the original — a
    /// strong invariant that exercises the full encode → re-parse path.
    #[test]
    fn c_abi_list_clone_then_remove_roundtrip() {
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_list_clone_then_remove_roundtrip: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();

        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );

        let original_body = unsafe { (*handle).save.body.clone() };
        let (block_idx, field_idx) = unsafe { find_object_list(&(*handle).blocks) }
            .expect("expected a zero1_count_u24 object_list with elements in a live save");

        // Clone element 0 into slot 1 (shifts the rest down).
        let rc = unsafe {
            crimson_save_list_clone_element(
                handle,
                block_idx,
                ptr::null(),
                0,
                field_idx,
                0,
                1,
            )
        };
        assert_eq!(rc, error::OK, "clone failed with rc={rc}");
        let after_clone = unsafe { (*handle).save.body.clone() };
        assert_ne!(
            after_clone, original_body,
            "body should have changed after clone"
        );
        assert!(
            after_clone.len() > original_body.len(),
            "cloning should grow the body"
        );

        // Remove the clone at slot 1.
        let rc = unsafe {
            crimson_save_list_remove_element(
                handle,
                block_idx,
                ptr::null(),
                0,
                field_idx,
                1,
            )
        };
        assert_eq!(rc, error::OK, "remove failed with rc={rc}");
        let after_remove = unsafe { (*handle).save.body.clone() };
        assert_eq!(
            after_remove, original_body,
            "clone-then-remove must be byte-identical to the original body"
        );

        unsafe { crimson_save_free(handle) };
    }

    /// Clone-then-remove on a `marker_run_plus_zeros` list (e.g.
    /// `MercenaryClanSaveData._mercenaryDataList`) must round-trip
    /// byte-identically. Regression guard for the variant the
    /// length-changing ops used to reject with `LIST_VARIANT_UNSUPPORTED`
    /// (the blocker for save-side mount/mercenary insertion). Skips
    /// cleanly when no live save — or no marker-variant list — is present.
    #[test]
    fn c_abi_list_clone_then_remove_roundtrip_marker_variant() {
        let Some(path) = find_save() else {
            eprintln!(
                "skipping c_abi_list_clone_then_remove_roundtrip_marker_variant: no live save"
            );
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();

        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );

        // Find the first top-level block whose field is a
        // marker_run_plus_zeros object_list with at least one element.
        let target = {
            let h = unsafe { &*handle };
            let mut found: Option<(u32, u32)> = None;
            'outer: for (bi, block) in h.blocks.iter().enumerate() {
                for (fi, field) in block.fields.iter().enumerate() {
                    if let FieldValue::ObjectList {
                        elements,
                        header_variant,
                        ..
                    } = &field.value
                        && *header_variant == "marker_run_plus_zeros"
                        && !elements.is_empty()
                    {
                        found = Some((bi as u32, fi as u32));
                        break 'outer;
                    }
                }
            }
            found
        };
        let Some((block_idx, field_idx)) = target else {
            eprintln!(
                "skipping c_abi_list_clone_then_remove_roundtrip_marker_variant: \
                 no marker_run_plus_zeros list with elements in this save"
            );
            unsafe { crimson_save_free(handle) };
            return;
        };

        let original_body = unsafe { (*handle).save.body.clone() };

        // Clone element 0 into slot 1 (the count u32 lives 17 bytes from
        // the end of the variant header) …
        let rc = unsafe {
            crimson_save_list_clone_element(handle, block_idx, ptr::null(), 0, field_idx, 0, 1)
        };
        assert_eq!(rc, error::OK, "marker-variant clone failed with rc={rc}");
        let after_clone = unsafe { (*handle).save.body.clone() };
        assert!(
            after_clone.len() > original_body.len(),
            "cloning a marker-variant element should grow the body"
        );

        // … then remove it again; the body must return to byte-identity,
        // proving the count was incremented and decremented in place.
        let rc = unsafe {
            crimson_save_list_remove_element(handle, block_idx, ptr::null(), 0, field_idx, 1)
        };
        assert_eq!(rc, error::OK, "marker-variant remove failed with rc={rc}");
        let after_remove = unsafe { (*handle).save.body.clone() };
        assert_eq!(
            after_remove, original_body,
            "marker-variant clone-then-remove must be byte-identical to the original body"
        );

        unsafe { crimson_save_free(handle) };
    }

    /// Pure-logic guard for the `marker_run_plus_zeros` count patch: the
    /// count is the u32 sitting 17 bytes before the end of `header_bytes`,
    /// independent of the leading pad / run length — even when a pad byte
    /// is itself `0x01` and mimics the marker run.
    #[test]
    fn update_marker_run_count_patches_tail_u32() {
        // Layout: [pad][01 run][00][u32 count LE][13 zero bytes].
        // Case A: no pad, 3-byte run, count 5 -> 7.
        let mut h = vec![0x01u8, 0x01, 0x01, 0x00];
        h.extend_from_slice(&5u32.to_le_bytes());
        h.extend_from_slice(&[0u8; 13]);
        let original = h.clone();
        update_object_list_count_in_header(&mut h, "marker_run_plus_zeros", 7).unwrap();
        let off = h.len() - 17;
        assert_eq!(&h[off..off + 4], &7u32.to_le_bytes(), "count not patched");
        assert_eq!(&h[..off], &original[..off], "bytes before count changed");
        assert_eq!(&h[off + 4..], &original[off + 4..], "trailing zeros changed");

        // Case B: a leading `0x01` pad byte that looks like a marker —
        // the tail-anchored offset must still land on the count.
        let mut h2 = vec![0x01u8 /* pad */, 0x01, 0x01 /* run */, 0x00];
        h2.extend_from_slice(&9u32.to_le_bytes());
        h2.extend_from_slice(&[0u8; 13]);
        update_object_list_count_in_header(&mut h2, "marker_run_plus_zeros", 42).unwrap();
        let off2 = h2.len() - 17;
        assert_eq!(&h2[off2..off2 + 4], &42u32.to_le_bytes());

        // Too-short header is rejected, not panicked.
        let mut tiny = vec![0u8; 10];
        assert_eq!(
            update_object_list_count_in_header(&mut tiny, "marker_run_plus_zeros", 1),
            Err(error::OUT_OF_RANGE)
        );
    }

    /// Cross-handle element transplant: load the live save into two
    /// handles, lift a mercenary element from handle A's
    /// _mercenaryDataList into handle B's, and verify B's list grew by one
    /// with the source charKey. Same file => the type-index remap is an
    /// identity, so this exercises the lift + insert + re-encode +
    /// re-decode plumbing end-to-end (cross-schema remap is covered by the
    /// downstream C# dragon transplant). Skips cleanly with no live save.
    #[test]
    fn c_abi_transplant_list_element_same_file() {
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_transplant_list_element_same_file: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();

        let mut a: *mut CrimsonSaveHandle = ptr::null_mut();
        let mut b: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut a) }, error::OK);
        assert_eq!(unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut b) }, error::OK);

        // Find a marker_run_plus_zeros list with >=1 element + element 0's charKey.
        let (block_idx, field_idx, src_charkey) = {
            let h = unsafe { &*a };
            let mut found: Option<(u32, u32, u32)> = None;
            'outer: for (bi, block) in h.blocks.iter().enumerate() {
                for (fi, field) in block.fields.iter().enumerate() {
                    if let FieldValue::ObjectList { elements, header_variant, .. } = &field.value
                        && *header_variant == "marker_run_plus_zeros"
                        && !elements.is_empty()
                    {
                        let mut ck = 0u32;
                        for f in &elements[0].fields {
                            if f.name == "_characterKey"
                                && let FieldValue::Scalar(ScalarValue::U32(v)) = f.value
                            {
                                ck = v;
                            }
                        }
                        found = Some((bi as u32, fi as u32, ck));
                        break 'outer;
                    }
                }
            }
            match found {
                Some(t) => t,
                None => {
                    eprintln!("skipping c_abi_transplant_list_element_same_file: no marker list");
                    unsafe { crimson_save_free(a) };
                    unsafe { crimson_save_free(b) };
                    return;
                }
            }
        };

        let b_count_before = {
            let h = unsafe { &*b };
            let FieldValue::ObjectList { elements, .. } =
                &h.blocks[block_idx as usize].fields[field_idx as usize].value
            else { panic!("target field not a list"); };
            elements.len()
        };

        let rc = unsafe {
            crimson_save_transplant_list_element(
                b, block_idx, ptr::null(), 0, field_idx, b_count_before as u32,
                a, block_idx, ptr::null(), 0, field_idx, 0,
            )
        };
        assert_eq!(rc, error::OK, "transplant failed rc={rc}");

        let h = unsafe { &*b };
        let FieldValue::ObjectList { elements, .. } =
            &h.blocks[block_idx as usize].fields[field_idx as usize].value
        else { panic!("target field not a list after transplant"); };
        assert_eq!(elements.len(), b_count_before + 1, "target list should grow by one");
        let mut ck = 0u32;
        for f in &elements[b_count_before].fields {
            if f.name == "_characterKey"
                && let FieldValue::Scalar(ScalarValue::U32(v)) = f.value
            {
                ck = v;
            }
        }
        assert_eq!(ck, src_charkey, "transplanted element charKey mismatch");

        unsafe { crimson_save_free(a) };
        unsafe { crimson_save_free(b) };
    }

    /// `list_remove_element` then bring the same element back by cloning
    /// the new head is NOT byte-identical (different element repeated)
    /// — verified by negation. This locks in the per-element identity:
    /// clones are byte-equal copies, not "any element of the same class".
    #[test]
    fn c_abi_list_clone_distinct_source_changes_body() {
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_list_clone_distinct_source: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();
        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );

        // Need a list with >= 2 distinct elements (bytes differ).
        let target = unsafe {
            let h = &*handle;
            let mut chosen = None;
            for (b_idx, block) in h.blocks.iter().enumerate() {
                for (f_idx, field) in block.fields.iter().enumerate() {
                    if let FieldValue::ObjectList {
                        elements,
                        header_variant,
                        ..
                    } = &field.value
                        && elements.len() >= 2
                        && *header_variant == "zero1_count_u24"
                    {
                        let a_start = elements[0].data_offset as usize;
                        let a_end = a_start + elements[0].data_size as usize;
                        let b_start = elements[1].data_offset as usize;
                        let b_end = b_start + elements[1].data_size as usize;
                        let a = &h.save.body[a_start..a_end];
                        let b = &h.save.body[b_start..b_end];
                        if a != b {
                            chosen = Some((b_idx as u32, f_idx as u32));
                            break;
                        }
                    }
                }
                if chosen.is_some() {
                    break;
                }
            }
            chosen
        };
        let Some((block_idx, field_idx)) = target else {
            eprintln!(
                "skipping c_abi_list_clone_distinct_source: no list with two distinct elements"
            );
            unsafe { crimson_save_free(handle) };
            return;
        };

        let original_body = unsafe { (*handle).save.body.clone() };

        // Clone src=1 into dst=0, then remove the clone at 0. Body
        // should round-trip back (we didn't change anything net).
        assert_eq!(
            unsafe {
                crimson_save_list_clone_element(
                    handle, block_idx, ptr::null(), 0, field_idx, 1, 0,
                )
            },
            error::OK
        );
        let after_clone = unsafe { (*handle).save.body.clone() };
        assert_ne!(after_clone, original_body);
        assert_eq!(
            unsafe {
                crimson_save_list_remove_element(handle, block_idx, ptr::null(), 0, field_idx, 0)
            },
            error::OK
        );
        let after_remove = unsafe { (*handle).save.body.clone() };
        assert_eq!(after_remove, original_body);

        unsafe { crimson_save_free(handle) };
    }

    /// Toggle a present scalar field absent, then back to present with
    /// the same bytes. Body must be byte-identical to the original.
    /// Exercises the mask-bit edit, the FixedPrefix/FixedSuffix
    /// classification rule, and the encoder's reverse-pass ordering.
    #[test]
    fn c_abi_set_scalar_field_present_toggle_roundtrip() {
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_set_scalar_field_present_toggle_roundtrip: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();
        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );

        let original_body = unsafe { (*handle).save.body.clone() };

        // Find a top-level block with a present scalar field. Prefer
        // FixedSuffix so the classification round-trip is exercised; fall
        // back to FixedPrefix if no suffix is found.
        let found = unsafe {
            let h = &*handle;
            let mut prefer: Option<(u32, u32, Vec<u8>)> = None;
            let mut fallback: Option<(u32, u32, Vec<u8>)> = None;
            for (b_idx, block) in h.blocks.iter().enumerate() {
                for (f_idx, field) in block.fields.iter().enumerate() {
                    if !field.present || !matches!(field.meta_kind, 0 | 2) {
                        continue;
                    }
                    let bytes = h.save.body[field.start..field.end].to_vec();
                    let entry = (b_idx as u32, f_idx as u32, bytes);
                    match field.kind {
                        FieldKind::FixedSuffix if prefer.is_none() => prefer = Some(entry),
                        FieldKind::FixedPrefix if fallback.is_none() => fallback = Some(entry),
                        _ => {}
                    }
                    if prefer.is_some() && fallback.is_some() {
                        break;
                    }
                }
                if prefer.is_some() {
                    break;
                }
            }
            prefer.or(fallback)
        };
        let Some((block_idx, field_idx, original_bytes)) = found else {
            eprintln!("skipping c_abi_set_scalar_field_present_toggle_roundtrip: no scalar field");
            unsafe { crimson_save_free(handle) };
            return;
        };

        // Make the field absent.
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_field_present(
                    handle,
                    block_idx,
                    ptr::null(),
                    0,
                    field_idx,
                    0,
                    ptr::null(),
                    0,
                )
            },
            error::OK
        );
        let after_clear = unsafe { (*handle).save.body.clone() };
        assert!(
            after_clear.len() < original_body.len(),
            "clearing a present field should shrink the body"
        );

        // Make it present again with the original bytes.
        let rc = unsafe {
            crimson_save_set_scalar_field_present(
                handle,
                block_idx,
                ptr::null(),
                0,
                field_idx,
                1,
                original_bytes.as_ptr(),
                original_bytes.len(),
            )
        };
        assert_eq!(rc, error::OK, "re-set present failed with rc={rc}");
        let after_set = unsafe { (*handle).save.body.clone() };
        assert_eq!(
            after_set, original_body,
            "clear-then-set-with-original-bytes must be byte-identical"
        );

        unsafe { crimson_save_free(handle) };
    }

    /// `set_scalar_field_present` rejects non-scalar fields with
    /// `NOT_SCALAR_FIELD_KIND` and length mismatches with
    /// `LENGTH_MISMATCH`. Errors must leave the handle untouched.
    #[test]
    fn c_abi_set_scalar_field_present_validation() {
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_set_scalar_field_present_validation: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();
        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );

        // Find any object_list field index in any block — we'll point
        // set_scalar_field_present at it to confirm rejection.
        let (block_idx, list_field_idx) = unsafe {
            let h = &*handle;
            let mut found = None;
            for (b_idx, block) in h.blocks.iter().enumerate() {
                for (f_idx, field) in block.fields.iter().enumerate() {
                    if matches!(field.value, FieldValue::ObjectList { .. }) {
                        found = Some((b_idx as u32, f_idx as u32));
                        break;
                    }
                }
                if found.is_some() {
                    break;
                }
            }
            found.expect("expected at least one object_list field in a live save")
        };

        let snapshot = unsafe { (*handle).save.body.clone() };

        // Non-scalar field rejection.
        let dummy = [0u8; 8];
        let rc = unsafe {
            crimson_save_set_scalar_field_present(
                handle,
                block_idx,
                ptr::null(),
                0,
                list_field_idx,
                1,
                dummy.as_ptr(),
                dummy.len(),
            )
        };
        assert_eq!(rc, error::NOT_SCALAR_FIELD_KIND);
        assert_eq!(
            unsafe { (*handle).save.body.clone() },
            snapshot,
            "handle must be untouched on error"
        );

        // Length mismatch rejection. Pick a scalar field and lie about
        // the byte count.
        let (b2, f2, meta_size) = unsafe {
            let h = &*handle;
            let mut found = None;
            for (b_idx, block) in h.blocks.iter().enumerate() {
                for (f_idx, field) in block.fields.iter().enumerate() {
                    if matches!(field.meta_kind, 0 | 2) && field.meta_size > 0 {
                        found = Some((b_idx as u32, f_idx as u32, field.meta_size as usize));
                        break;
                    }
                }
                if found.is_some() {
                    break;
                }
            }
            found.expect("expected at least one scalar field schema in a live save")
        };
        let wrong_len = vec![0u8; meta_size + 1];
        let rc = unsafe {
            crimson_save_set_scalar_field_present(
                handle,
                b2,
                ptr::null(),
                0,
                f2,
                1,
                wrong_len.as_ptr(),
                wrong_len.len(),
            )
        };
        assert_eq!(rc, error::LENGTH_MISMATCH);
        assert_eq!(
            unsafe { (*handle).save.body.clone() },
            snapshot,
            "handle must be untouched on length error"
        );

        unsafe { crimson_save_free(handle) };
    }

    /// Cloning an element grows the body by exactly the source element's
    /// `data_size` (the wrapper + payload bytes). Locks the size delta
    /// observation from the slot100/slot101 RE pass.
    #[test]
    fn c_abi_list_clone_grows_body_by_element_size() {
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_list_clone_grows_body_by_element_size: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();
        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );

        let original_len = unsafe { (*handle).save.body.len() };
        let (block_idx, field_idx, expected_delta) = unsafe {
            let h = &*handle;
            let (b_idx, f_idx) =
                find_object_list(&h.blocks).expect("expected a zero1_count_u24 list");
            let element_size = match &h.blocks[b_idx as usize].fields[f_idx as usize].value {
                FieldValue::ObjectList { elements, .. } => elements[0].data_size as usize,
                _ => unreachable!(),
            };
            (b_idx, f_idx, element_size)
        };

        let rc = unsafe {
            crimson_save_list_clone_element(
                handle,
                block_idx,
                ptr::null(),
                0,
                field_idx,
                0,
                1,
            )
        };
        assert_eq!(rc, error::OK);
        let new_len = unsafe { (*handle).save.body.len() };
        assert_eq!(
            new_len - original_len,
            expected_delta,
            "clone must grow the body by exactly the element's data_size"
        );

        unsafe { crimson_save_free(handle) };
    }

    // ── Schema-aware element builder + list_insert_element (Phase B.3) ─────

    /// `make_empty_element_bytes` produces bytes that decode cleanly back
    /// to an `ObjectBlock` of the requested class with all fields absent.
    /// Two-call pattern: query size, then fill.
    #[test]
    fn c_abi_make_empty_element_bytes_decodes() {
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_make_empty_element_bytes_decodes: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();
        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );

        // Pick the class of any existing list element so we know the
        // class_index is valid and the produced bytes can sit alongside
        // the real elements in that list.
        let (class_index, expected_mbc) = unsafe {
            let h = &*handle;
            let (b_idx, f_idx) = find_object_list(&h.blocks)
                .expect("expected a zero1_count_u24 object_list");
            let element = match &h.blocks[b_idx as usize].fields[f_idx as usize].value {
                FieldValue::ObjectList { elements, .. } => &elements[0],
                _ => unreachable!(),
            };
            (element.class_index, element.mask_byte_count as usize)
        };

        // Two-call pattern: first call with NULL buf -> BUFFER_TOO_SMALL +
        // required size; second call with allocated buf -> OK.
        let mut required: usize = 0;
        let rc = unsafe {
            crimson_save_make_empty_element_bytes(
                handle,
                class_index,
                ptr::null_mut(),
                0,
                &mut required,
            )
        };
        assert_eq!(rc, error::BUFFER_TOO_SMALL);
        // total = wrapper (mbc + 17) + payload (4 + 4) = mbc + 25
        let expected_len = expected_mbc + 25;
        assert_eq!(
            required, expected_len,
            "empty element should be mbc({expected_mbc}) + 25 = {expected_len} bytes"
        );

        let mut buf = vec![0u8; required];
        let rc = unsafe {
            crimson_save_make_empty_element_bytes(
                handle,
                class_index,
                buf.as_mut_ptr(),
                buf.len(),
                ptr::null_mut(),
            )
        };
        assert_eq!(rc, error::OK);

        // Sanity-check the wrapper shape: u16 mbc, mbc zero mask bytes,
        // u16 type_index (= class_index low 16 bits).
        let parsed_mbc = u16::from_le_bytes([buf[0], buf[1]]) as usize;
        assert_eq!(parsed_mbc, expected_mbc);
        for i in 0..parsed_mbc {
            assert_eq!(buf[2 + i], 0, "mask byte {i} should be zero");
        }
        let parsed_type = u16::from_le_bytes([buf[2 + parsed_mbc], buf[3 + parsed_mbc]]);
        assert_eq!(parsed_type as u32, class_index);

        // The trailing u32 (last 4 bytes) is `trailing_size = 4` because
        // the size u32 sits 4 bytes after payload_start.
        let trailing = u32::from_le_bytes([
            buf[required - 4],
            buf[required - 3],
            buf[required - 2],
            buf[required - 1],
        ]);
        assert_eq!(trailing, 4, "trailing_size should be 4 for an empty payload");

        unsafe { crimson_save_free(handle) };
    }

    /// `make_empty_element_bytes` then `list_insert_element` adds an empty
    /// element; subsequent `list_remove_element` reverses it. The body
    /// must be byte-identical to the original.
    #[test]
    fn c_abi_insert_empty_then_remove_roundtrip() {
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_insert_empty_then_remove_roundtrip: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();
        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );

        let original_body = unsafe { (*handle).save.body.clone() };
        let (block_idx, field_idx, element_class_idx) = unsafe {
            let h = &*handle;
            let (b_idx, f_idx) = find_object_list(&h.blocks)
                .expect("expected a zero1_count_u24 object_list");
            let class = match &h.blocks[b_idx as usize].fields[f_idx as usize].value {
                FieldValue::ObjectList { elements, .. } => elements[0].class_index,
                _ => unreachable!(),
            };
            (b_idx, f_idx, class)
        };

        // Build empty element bytes for the list's element class.
        let mut required: usize = 0;
        unsafe {
            crimson_save_make_empty_element_bytes(
                handle,
                element_class_idx,
                ptr::null_mut(),
                0,
                &mut required,
            );
        }
        let mut empty = vec![0u8; required];
        assert_eq!(
            unsafe {
                crimson_save_make_empty_element_bytes(
                    handle,
                    element_class_idx,
                    empty.as_mut_ptr(),
                    empty.len(),
                    ptr::null_mut(),
                )
            },
            error::OK
        );

        // Insert at the head (index 0). Body should grow by exactly the
        // empty element's length.
        let rc = unsafe {
            crimson_save_list_insert_element(
                handle,
                block_idx,
                ptr::null(),
                0,
                field_idx,
                0,
                empty.as_ptr(),
                empty.len(),
            )
        };
        assert_eq!(rc, error::OK, "insert failed with rc={rc}");
        let after_insert = unsafe { (*handle).save.body.clone() };
        assert_ne!(after_insert, original_body);
        assert_eq!(
            after_insert.len() - original_body.len(),
            empty.len(),
            "insert must grow the body by exactly the element's len"
        );

        // Remove the inserted element (now at index 0).
        let rc = unsafe {
            crimson_save_list_remove_element(
                handle,
                block_idx,
                ptr::null(),
                0,
                field_idx,
                0,
            )
        };
        assert_eq!(rc, error::OK);
        let after_remove = unsafe { (*handle).save.body.clone() };
        assert_eq!(
            after_remove, original_body,
            "insert-empty then remove must round-trip"
        );

        unsafe { crimson_save_free(handle) };
    }

    /// `list_insert_element` rejects malformed template bytes
    /// (`BODY_PARSE`) without touching the handle. Out-of-range
    /// `insert_at` returns `OUT_OF_RANGE`.
    #[test]
    fn c_abi_list_insert_element_validation() {
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_list_insert_element_validation: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();
        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );

        let original_body = unsafe { (*handle).save.body.clone() };
        let (block_idx, field_idx) = unsafe {
            let h = &*handle;
            find_object_list(&h.blocks).expect("expected a zero1_count_u24 object_list")
        };

        // Garbage bytes — too short to be a valid wrapper.
        let garbage = [0u8; 4];
        let rc = unsafe {
            crimson_save_list_insert_element(
                handle,
                block_idx,
                ptr::null(),
                0,
                field_idx,
                0,
                garbage.as_ptr(),
                garbage.len(),
            )
        };
        assert_eq!(rc, error::BODY_PARSE);
        assert_eq!(
            unsafe { (*handle).save.body.clone() },
            original_body,
            "handle must be untouched on BODY_PARSE error"
        );

        // Out-of-range insert position. Build empty element first.
        let element_class_idx = unsafe {
            let h = &*handle;
            match &h.blocks[block_idx as usize].fields[field_idx as usize].value {
                FieldValue::ObjectList { elements, .. } => elements[0].class_index,
                _ => unreachable!(),
            }
        };
        let mut required: usize = 0;
        unsafe {
            crimson_save_make_empty_element_bytes(
                handle,
                element_class_idx,
                ptr::null_mut(),
                0,
                &mut required,
            );
        }
        let mut empty = vec![0u8; required];
        unsafe {
            crimson_save_make_empty_element_bytes(
                handle,
                element_class_idx,
                empty.as_mut_ptr(),
                empty.len(),
                ptr::null_mut(),
            );
        }
        let rc = unsafe {
            crimson_save_list_insert_element(
                handle,
                block_idx,
                ptr::null(),
                0,
                field_idx,
                u32::MAX,
                empty.as_ptr(),
                empty.len(),
            )
        };
        assert_eq!(rc, error::OUT_OF_RANGE);
        assert_eq!(
            unsafe { (*handle).save.body.clone() },
            original_body,
            "handle must be untouched on OUT_OF_RANGE error"
        );

        unsafe { crimson_save_free(handle) };
    }

    /// End-to-end: insert empty element, populate one scalar field via
    /// `set_scalar_field_present`, then `set_scalar_field_path` on a
    /// nested location. Verifies the full B.1 + B.2 + B.3 chain.
    #[test]
    fn c_abi_insert_then_populate_field() {
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_insert_then_populate_field: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();
        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );

        // Pick a list + element class. Find any scalar field schema in
        // that element class to populate.
        let (block_idx, field_idx, element_class_idx, target_scalar_field, scalar_size) =
            unsafe {
                let h = &*handle;
                let (b_idx, f_idx) = find_object_list(&h.blocks)
                    .expect("expected a zero1_count_u24 object_list");
                let element = match &h.blocks[b_idx as usize].fields[f_idx as usize].value {
                    FieldValue::ObjectList { elements, .. } => &elements[0],
                    _ => unreachable!(),
                };
                let class_idx = element.class_index;
                // Find a scalar field schema entry on the element. Use the
                // existing element's field list which has the schema-derived
                // (name, meta_kind, meta_size) information.
                let mut scalar = None;
                for f in &element.fields {
                    if matches!(f.meta_kind, 0 | 2) && f.meta_size > 0 {
                        scalar = Some((f.field_index, f.meta_size as usize));
                        break;
                    }
                }
                let (scalar_field, scalar_size) = scalar.expect("expected a scalar field");
                (b_idx, f_idx, class_idx, scalar_field, scalar_size)
            };

        // Build + insert empty element at the head.
        let mut required: usize = 0;
        unsafe {
            crimson_save_make_empty_element_bytes(
                handle,
                element_class_idx,
                ptr::null_mut(),
                0,
                &mut required,
            );
        }
        let mut empty = vec![0u8; required];
        unsafe {
            crimson_save_make_empty_element_bytes(
                handle,
                element_class_idx,
                empty.as_mut_ptr(),
                empty.len(),
                ptr::null_mut(),
            );
        }
        assert_eq!(
            unsafe {
                crimson_save_list_insert_element(
                    handle,
                    block_idx,
                    ptr::null(),
                    0,
                    field_idx,
                    0,
                    empty.as_ptr(),
                    empty.len(),
                )
            },
            error::OK
        );

        // Populate the chosen scalar field via set_scalar_field_present.
        // Use 0xAB-fill bytes so a path-set later can detect the value.
        let init = vec![0xAB_u8; scalar_size];
        let path_step = CrimsonPathStep {
            field_idx,
            element_idx: 0,
        };
        let rc = unsafe {
            crimson_save_set_scalar_field_present(
                handle,
                block_idx,
                &path_step,
                1,
                target_scalar_field,
                1,
                init.as_ptr(),
                init.len(),
            )
        };
        assert_eq!(rc, error::OK, "set_scalar_field_present failed with rc={rc}");

        // Now overwrite the same field with a different value via
        // set_scalar_field_path — confirms the new element is reachable
        // by the existing path-addressed setter too.
        let overwrite = vec![0xCD_u8; scalar_size];
        let rc = unsafe {
            crimson_save_set_scalar_field_path(
                handle,
                block_idx,
                &path_step,
                1,
                target_scalar_field,
                overwrite.as_ptr(),
                overwrite.len(),
            )
        };
        assert_eq!(rc, error::OK, "set_scalar_field_path failed with rc={rc}");

        // Verify by reading the new element's scalar.
        unsafe {
            let h = &*handle;
            let el = match &h.blocks[block_idx as usize].fields[field_idx as usize].value {
                FieldValue::ObjectList { elements, .. } => &elements[0],
                _ => unreachable!(),
            };
            let f = &el.fields[target_scalar_field as usize];
            assert!(f.present, "field must be present after set_field_present");
            assert_eq!(f.end - f.start, scalar_size);
            assert_eq!(
                &h.save.body[f.start..f.end],
                overwrite.as_slice(),
                "scalar bytes must match the overwrite value"
            );
        }

        unsafe { crimson_save_free(handle) };
    }

    /// Fixture history: this socket round-trip was first RE'd against slot104,
    /// which the user populated with 5 North Wind Tridents in a specific
    /// gem-distribution pattern for a known data shape. It now loads the
    /// current 1.12 save (slot107) via `find_save` and skips cleanly when no
    /// item with an empty opened socket is present.
    ///
    /// Insert a gem into an empty socket slot via
    /// `set_scalar_fields_present_batch`, then remove it. Body MUST be
    /// byte-identical to the pre-mutation state after the remove.
    ///
    /// This is the validation that the existing C ABI surface already
    /// supports the "absent → insert gem" and "gem → absent" cases the
    /// CrimsonAtomtic socket editor needs — without any new primitive.
    ///
    /// Schema being exercised (verified via `_probe_item_socket_data` in
    /// `src/c_abi/character_info.rs`):
    ///
    /// - `ItemSaveData._socketSaveDataList` is an ObjectList of
    ///   `ItemSocketSaveData` whose count is fixed to `_maxSocketCount`.
    /// - Each `ItemSocketSaveData` has 2 fields: `_currentEndurance` (u16)
    ///   + `_itemKey` (u32) and a 1-byte mask.
    /// - mask=[0x00] = empty slot (opened-empty OR not-yet-opened);
    ///   mask=[0x03] = filled slot.
    /// - Insert gem = flip both mask bits via two
    ///   `set_scalar_field_present(make_present=true)` ops with the
    ///   gem's endurance + itemkey as init bytes.
    /// - Remove gem = flip both mask bits via two
    ///   `set_scalar_field_present(make_present=false)` ops.
    ///
    /// Runs only when slot107 is present on the developer's machine.
    #[test]
    fn c_abi_socket_insert_then_remove_roundtrip_slot107() {
        let Some(path) = find_save() else {
            eprintln!(
                "skipping c_abi_socket_insert_then_remove_roundtrip_slot107: \
                 no slot107/save.save under %LOCALAPPDATA%"
            );
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();
        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );
        let original_body = unsafe { (*handle).save.body.clone() };

        // ── Find the first ItemSaveData with itemkey 310031 and at
        // least one mask=[0x00] socket among its first `_validSocketCount`
        // entries (i.e. an opened-empty slot we can fill).
        const WEAPON_KEY: u32 = 310031;
        let found = unsafe {
            let h = &*handle;
            let mut hit = None;
            'outer: for (block_idx, block) in h.blocks.iter().enumerate() {
                if block.class_name != "InventorySaveData" {
                    continue;
                }
                let Some(inv_list) = block
                    .fields
                    .iter()
                    .find(|f| f.name.eq_ignore_ascii_case("_inventorylist"))
                else { continue };
                let FieldValue::ObjectList { elements: containers, .. } = &inv_list.value
                else { continue };
                for container in containers {
                    let Some(item_list_field) = container
                        .fields
                        .iter()
                        .find(|f| f.name.eq_ignore_ascii_case("_itemList"))
                    else { continue };
                    let item_list_field_idx = item_list_field.field_index;
                    let inv_list_field_idx = inv_list.field_index;
                    let inv_idx = containers.iter().position(|c| std::ptr::eq(c, container)).unwrap();
                    let FieldValue::ObjectList { elements: items, .. } = &item_list_field.value
                    else { continue };
                    for (item_idx, item) in items.iter().enumerate() {
                        let item_key = item
                            .fields
                            .iter()
                            .find(|f| f.name == "_itemKey")
                            .and_then(|f| match &f.value {
                                FieldValue::Scalar(ScalarValue::U32(v)) => Some(*v),
                                _ => None,
                            })
                            .unwrap_or(0);
                        if item_key != WEAPON_KEY {
                            continue;
                        }
                        let valid_count = item
                            .fields
                            .iter()
                            .find(|f| f.name == "_validSocketCount")
                            .and_then(|f| match &f.value {
                                FieldValue::Scalar(ScalarValue::U8(v)) => Some(*v),
                                _ => None,
                            })
                            .unwrap_or(0) as usize;
                        let Some(socket_field) = item.fields.iter().find(|f| {
                            f.name == "_socketSaveDataList"
                        }) else { continue };
                        let socket_field_idx = socket_field.field_index;
                        let FieldValue::ObjectList { elements: sockets, .. } = &socket_field.value
                        else { continue };
                        for (socket_idx, socket) in sockets.iter().enumerate() {
                            if socket_idx >= valid_count {
                                break; // only consider opened slots
                            }
                            if socket.mask_bytes.first().copied() == Some(0x00) {
                                hit = Some((
                                    block_idx as u32,
                                    inv_list_field_idx,
                                    inv_idx as u32,
                                    item_list_field_idx,
                                    item_idx as u32,
                                    socket_field_idx,
                                    socket_idx as u32,
                                ));
                                break 'outer;
                            }
                        }
                    }
                }
            }
            hit
        };
        let Some((
            block_idx,
            inv_field_idx,
            inv_elem_idx,
            item_field_idx,
            item_elem_idx,
            socket_field_idx,
            socket_elem_idx,
        )) = found else {
            eprintln!(
                "skipping: no trident (key=310031) with an empty opened socket found in slot107"
            );
            unsafe { crimson_save_free(handle) };
            return;
        };
        eprintln!(
            "round-trip target: block={block_idx} inv_field={inv_field_idx} \
             inv_elem={inv_elem_idx} item_field={item_field_idx} item_elem={item_elem_idx} \
             socket_field={socket_field_idx} socket_elem={socket_elem_idx}"
        );

        // ── Build the descent path: InventorySaveData → _inventorylist[N]
        //    → _itemList[M] → _socketSaveDataList[K]. The leaf step
        //    targets a scalar inside the socket element, so the path
        //    has 3 descents (the socket field itself is named in
        //    field_idx, with element_idx pointing at K).
        let path = [
            CrimsonPathStep { field_idx: inv_field_idx, element_idx: inv_elem_idx },
            CrimsonPathStep { field_idx: item_field_idx, element_idx: item_elem_idx },
            CrimsonPathStep { field_idx: socket_field_idx, element_idx: socket_elem_idx },
        ];

        // Test gem: itemkey 1002979, endurance 88 (chosen to NOT match
        // the existing 99/100 values in the save so we can verify the
        // value made it in cleanly).
        const GEM_ITEM_KEY: u32 = 1002979;
        const GEM_ENDURANCE: u16 = 88;
        let endurance_bytes = GEM_ENDURANCE.to_le_bytes();
        let itemkey_bytes = GEM_ITEM_KEY.to_le_bytes();

        // ── Insert: set both _currentEndurance (field 0, u16) and
        //    _itemKey (field 1, u32) to present, with init bytes.
        let insert_ops = [
            CrimsonScalarPresentBatchOp {
                block_idx,
                field_idx: 0, // _currentEndurance
                path: path.as_ptr(),
                path_len: path.len(),
                make_present: 1,
                bytes: endurance_bytes.as_ptr(),
                bytes_len: endurance_bytes.len(),
            },
            CrimsonScalarPresentBatchOp {
                block_idx,
                field_idx: 1, // _itemKey
                path: path.as_ptr(),
                path_len: path.len(),
                make_present: 1,
                bytes: itemkey_bytes.as_ptr(),
                bytes_len: itemkey_bytes.len(),
            },
        ];
        let mut failed_idx: usize = 0;
        let rc = unsafe {
            crimson_save_set_scalar_fields_present_batch(
                handle,
                insert_ops.as_ptr(),
                insert_ops.len(),
                &mut failed_idx,
            )
        };
        assert_eq!(rc, error::OK, "insert batch failed with rc={rc} at op {failed_idx}");

        // ── Verify: the socket element now reports mask=[0x03] with
        //    the right values.
        unsafe {
            let h = &*handle;
            let inv_field = &h.blocks[block_idx as usize].fields[inv_field_idx as usize];
            let FieldValue::ObjectList { elements: containers, .. } = &inv_field.value
            else { panic!("inv field wrong shape") };
            let item_field = &containers[inv_elem_idx as usize].fields[item_field_idx as usize];
            let FieldValue::ObjectList { elements: items, .. } = &item_field.value
            else { panic!("item field wrong shape") };
            let socket_field = &items[item_elem_idx as usize].fields[socket_field_idx as usize];
            let FieldValue::ObjectList { elements: sockets, .. } = &socket_field.value
            else { panic!("socket field wrong shape") };
            let socket = &sockets[socket_elem_idx as usize];
            assert_eq!(
                socket.mask_bytes.as_slice(),
                &[0x03],
                "after insert, socket mask must be [0x03]"
            );
            let end_field = &socket.fields[0];
            let key_field = &socket.fields[1];
            assert!(end_field.present);
            assert!(key_field.present);
            if let FieldValue::Scalar(ScalarValue::U16(v)) = &end_field.value {
                assert_eq!(*v, GEM_ENDURANCE);
            } else {
                panic!("_currentEndurance wrong type after insert");
            }
            if let FieldValue::Scalar(ScalarValue::U32(v)) = &key_field.value {
                assert_eq!(*v, GEM_ITEM_KEY);
            } else {
                panic!("_itemKey wrong type after insert");
            }
        }

        // Body must have grown by exactly 6 bytes (u16 + u32).
        let after_insert_len = unsafe { (*handle).save.body.len() };
        assert_eq!(
            after_insert_len,
            original_body.len() + 6,
            "insert should grow body by 6 bytes (u16 + u32)"
        );

        // ── Remove: set both fields back to absent.
        let remove_ops = [
            CrimsonScalarPresentBatchOp {
                block_idx,
                field_idx: 0,
                path: path.as_ptr(),
                path_len: path.len(),
                make_present: 0,
                bytes: std::ptr::null(),
                bytes_len: 0,
            },
            CrimsonScalarPresentBatchOp {
                block_idx,
                field_idx: 1,
                path: path.as_ptr(),
                path_len: path.len(),
                make_present: 0,
                bytes: std::ptr::null(),
                bytes_len: 0,
            },
        ];
        let rc = unsafe {
            crimson_save_set_scalar_fields_present_batch(
                handle,
                remove_ops.as_ptr(),
                remove_ops.len(),
                &mut failed_idx,
            )
        };
        assert_eq!(rc, error::OK, "remove batch failed with rc={rc} at op {failed_idx}");

        // ── Verify: byte-identical to original. This is the core
        // contract — insert then remove must be a no-op.
        let after_remove = unsafe { (*handle).save.body.clone() };
        assert_eq!(
            after_remove.len(),
            original_body.len(),
            "after remove, body length must match original"
        );
        assert_eq!(
            after_remove, original_body,
            "after insert+remove, body bytes must be byte-identical to original"
        );

        unsafe { crimson_save_free(handle) };
    }

    /// EquipmentSaveData socket round-trip: the currently-equipped
    /// items live nested in `EquipmentSaveData._list[N]._item<child>`
    /// (a Locator wrapping an `ItemSaveData`), not at the top of an
    /// `InventorySaveData`. Confirms the C ABI path-descent handles
    /// the Locator-then-ObjectList chain cleanly via the same
    /// `set_scalar_fields_present_batch` entry point.
    ///
    /// Strategy: find any equipped item with a FILLED socket, snapshot
    /// the gem's endurance + itemkey, remove → re-insert with the
    /// same values, assert byte-identical to original. Works whether
    /// the user has empty opened slots or not (the user's CE-stuffed
    /// 1002285/1002284/1000316 are all full, but they still have
    /// filled slots we can round-trip on).
    #[test]
    fn c_abi_socket_remove_then_reinsert_roundtrip_equipment_slot107() {
        let Some(path) = find_save() else {
            eprintln!(
                "skipping c_abi_socket_remove_then_reinsert_roundtrip_equipment_slot107: \
                 no slot107/save.save"
            );
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();
        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );
        let original_body = unsafe { (*handle).save.body.clone() };

        // Find the first EquipmentSaveData._list[N]._item<child>.ItemSaveData
        // whose _socketSaveDataList has a FILLED socket. Capture the
        // gem's endurance + itemkey so we can re-insert the same values.
        type Target = (u32, u32, u32, u32, u32, u32, u16, u32);
        let found: Option<Target> = unsafe {
            let h = &*handle;
            let mut hit: Option<Target> = None;
            'outer: for (block_idx, block) in h.blocks.iter().enumerate() {
                if block.class_name != "EquipmentSaveData" {
                    continue;
                }
                let Some(list_field) = block.fields.iter().find(|f| f.name == "_list")
                else { continue };
                let list_field_idx = list_field.field_index;
                let FieldValue::ObjectList { elements: slots, .. } = &list_field.value
                else { continue };
                for (slot_idx, slot) in slots.iter().enumerate() {
                    let Some(item_field) = slot.fields.iter().find(|f| f.name == "_item")
                    else { continue };
                    let item_field_idx = item_field.field_index;
                    let FieldValue::Locator { child: Some(item), .. } = &item_field.value
                    else { continue };
                    let Some(socket_field) = item
                        .fields
                        .iter()
                        .find(|f| f.name == "_socketSaveDataList" && f.present)
                    else { continue };
                    let socket_field_idx = socket_field.field_index;
                    let FieldValue::ObjectList { elements: sockets, .. } = &socket_field.value
                    else { continue };
                    for (socket_idx, socket) in sockets.iter().enumerate() {
                        if socket.mask_bytes.first().copied() != Some(0x03) {
                            continue;
                        }
                        let end = socket.fields.iter()
                            .find(|sf| sf.name == "_currentEndurance" && sf.present)
                            .and_then(|sf| match &sf.value {
                                FieldValue::Scalar(ScalarValue::U16(v)) => Some(*v),
                                _ => None,
                            });
                        let key = socket.fields.iter()
                            .find(|sf| sf.name == "_itemKey" && sf.present)
                            .and_then(|sf| match &sf.value {
                                FieldValue::Scalar(ScalarValue::U32(v)) => Some(*v),
                                _ => None,
                            });
                        if let (Some(e), Some(k)) = (end, key) {
                            hit = Some((
                                block_idx as u32,
                                list_field_idx,
                                slot_idx as u32,
                                item_field_idx,
                                socket_field_idx,
                                socket_idx as u32,
                                e,
                                k,
                            ));
                            break 'outer;
                        }
                    }
                }
            }
            hit
        };
        let Some((
            block_idx,
            list_field_idx,
            slot_idx,
            item_field_idx,
            socket_field_idx,
            socket_elem_idx,
            saved_endurance,
            saved_gem_key,
        )) = found else {
            eprintln!(
                "skipping: no EquipmentSaveData slot with a filled socket — \
                 user has nothing equipped with gems in slot107"
            );
            unsafe { crimson_save_free(handle) };
            return;
        };
        eprintln!(
            "EquipmentSaveData round-trip: block={block_idx} list_field={list_field_idx} \
             slot={slot_idx} item_field={item_field_idx} socket_field={socket_field_idx} \
             socket_elem={socket_elem_idx} endurance={saved_endurance} gem={saved_gem_key}"
        );

        // Path: EquipmentSaveData → _list[N] → _item<Locator child> → _socketSaveDataList[K]
        // The Locator step uses element_idx=0 (Locator has no array dimension —
        // navigate_mut_to_parent dereferences the single child).
        let path = [
            CrimsonPathStep { field_idx: list_field_idx, element_idx: slot_idx },
            CrimsonPathStep { field_idx: item_field_idx, element_idx: 0 },
            CrimsonPathStep { field_idx: socket_field_idx, element_idx: socket_elem_idx },
        ];

        // ── Remove: clear both fields.
        let remove_ops = [
            CrimsonScalarPresentBatchOp {
                block_idx,
                field_idx: 0,
                path: path.as_ptr(),
                path_len: path.len(),
                make_present: 0,
                bytes: std::ptr::null(),
                bytes_len: 0,
            },
            CrimsonScalarPresentBatchOp {
                block_idx,
                field_idx: 1,
                path: path.as_ptr(),
                path_len: path.len(),
                make_present: 0,
                bytes: std::ptr::null(),
                bytes_len: 0,
            },
        ];
        let mut failed_idx: usize = 0;
        let rc = unsafe {
            crimson_save_set_scalar_fields_present_batch(
                handle,
                remove_ops.as_ptr(),
                remove_ops.len(),
                &mut failed_idx,
            )
        };
        assert_eq!(rc, error::OK, "equipment remove failed rc={rc} at op {failed_idx}");
        let after_remove_len = unsafe { (*handle).save.body.len() };
        assert_eq!(after_remove_len, original_body.len() - 6, "remove should shrink by 6 bytes");

        // ── Re-insert with the original values: write back endurance + itemkey.
        let end_bytes = saved_endurance.to_le_bytes();
        let key_bytes = saved_gem_key.to_le_bytes();
        let reinsert_ops = [
            CrimsonScalarPresentBatchOp {
                block_idx,
                field_idx: 0,
                path: path.as_ptr(),
                path_len: path.len(),
                make_present: 1,
                bytes: end_bytes.as_ptr(),
                bytes_len: end_bytes.len(),
            },
            CrimsonScalarPresentBatchOp {
                block_idx,
                field_idx: 1,
                path: path.as_ptr(),
                path_len: path.len(),
                make_present: 1,
                bytes: key_bytes.as_ptr(),
                bytes_len: key_bytes.len(),
            },
        ];
        let rc = unsafe {
            crimson_save_set_scalar_fields_present_batch(
                handle,
                reinsert_ops.as_ptr(),
                reinsert_ops.len(),
                &mut failed_idx,
            )
        };
        assert_eq!(rc, error::OK, "equipment reinsert failed rc={rc} at op {failed_idx}");

        let after_reinsert = unsafe { (*handle).save.body.clone() };
        assert_eq!(
            after_reinsert, original_body,
            "EquipmentSaveData remove+reinsert-with-original-values must be byte-identical \
             — proves the Locator-then-ObjectList descent + scalar present-toggle cycle \
             is fully reversible"
        );

        unsafe { crimson_save_free(handle) };
    }

    /// Round-trip test for [`crimson_save_set_object_list_present`]:
    /// pick an `ItemSaveData` whose `_itemDyeDataList` field is absent,
    /// toggle it present (auto-materializes count=1 + one empty
    /// `ItemDyeSaveData`) → toggle it back to absent → assert body
    /// bytes are byte-identical to the original.
    ///
    /// Closes the v2 "add dye to undyed item" path for `CrimsonAtomtic`'s
    /// dye editor (see [`docs/dye-editor-scope.md`](../../../docs/dye-editor-scope.md)).
    /// Skips cleanly when slot107 isn't present.
    #[test]
    fn c_abi_object_list_present_roundtrip_dye_data_list_slot107() {
        let Some(path) = find_save() else {
            eprintln!(
                "skipping c_abi_object_list_present_roundtrip_dye_data_list_slot107: \
                 no slot107/save.save"
            );
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();
        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );
        let original_body = unsafe { (*handle).save.body.clone() };

        // Find the first ItemSaveData under InventorySaveData →
        // _inventorylist[N] → _itemList[M] whose `_itemDyeDataList` is
        // ABSENT. Most non-dyed items match — the slot107 baseline has
        // hundreds of candidates.
        type Target = (u32, u32, u32, u32, u32, u32);
        let found: Option<Target> = unsafe {
            let h = &*handle;
            let mut hit: Option<Target> = None;
            'outer: for (block_idx, block) in h.blocks.iter().enumerate() {
                if block.class_name != "InventorySaveData" {
                    continue;
                }
                let Some(inv_list) = block
                    .fields
                    .iter()
                    .find(|f| f.name.eq_ignore_ascii_case("_inventorylist"))
                else { continue };
                let inv_list_field_idx = inv_list.field_index;
                let FieldValue::ObjectList { elements: containers, .. } = &inv_list.value
                else { continue };
                for (inv_idx, container) in containers.iter().enumerate() {
                    let Some(item_list_field) = container
                        .fields
                        .iter()
                        .find(|f| f.name.eq_ignore_ascii_case("_itemList"))
                    else { continue };
                    let item_list_field_idx = item_list_field.field_index;
                    let FieldValue::ObjectList { elements: items, .. } = &item_list_field.value
                    else { continue };
                    for (item_idx, item) in items.iter().enumerate() {
                        let Some(dye_field) = item.fields.iter().find(|f| f.name == "_itemDyeDataList")
                        else { continue };
                        if dye_field.present {
                            continue; // we want an absent dye list
                        }
                        if !matches!(dye_field.meta_kind, 6 | 7) {
                            continue; // schema sanity
                        }
                        hit = Some((
                            block_idx as u32,
                            inv_list_field_idx,
                            inv_idx as u32,
                            item_list_field_idx,
                            item_idx as u32,
                            dye_field.field_index,
                        ));
                        break 'outer;
                    }
                }
            }
            hit
        };
        let Some((block_idx, inv_field_idx, inv_elem_idx, item_field_idx, item_elem_idx, dye_field_idx)) = found
        else {
            eprintln!(
                "skipping: no ItemSaveData with absent _itemDyeDataList found in slot107"
            );
            unsafe { crimson_save_free(handle) };
            return;
        };
        eprintln!(
            "round-trip target: block={block_idx} inv_field={inv_field_idx} \
             inv_elem={inv_elem_idx} item_field={item_field_idx} \
             item_elem={item_elem_idx} dye_field={dye_field_idx}"
        );

        let path = [
            CrimsonPathStep { field_idx: inv_field_idx, element_idx: inv_elem_idx },
            CrimsonPathStep { field_idx: item_field_idx, element_idx: item_elem_idx },
        ];

        // ── Step 1: toggle the dye list from absent to present. The
        //    ABI auto-materializes count=1 with one default empty
        //    ItemDyeSaveData element (the byte-unambiguous shape that
        //    survives a re-decode).
        let rc = unsafe {
            crimson_save_set_object_list_present(
                handle,
                block_idx,
                path.as_ptr(),
                path.len(),
                dye_field_idx,
                1,
            )
        };
        assert_eq!(rc, error::OK, "set_object_list_present(true) failed rc={rc}");

        // ── Verify: dye field is now present with count=1 and one
        //    decoded element with every dye scalar absent.
        let after_present_len = unsafe {
            let h = &*handle;
            let inv_field = &h.blocks[block_idx as usize].fields[inv_field_idx as usize];
            let FieldValue::ObjectList { elements: containers, .. } = &inv_field.value
            else { panic!("inv field shape changed") };
            let item_field = &containers[inv_elem_idx as usize].fields[item_field_idx as usize];
            let FieldValue::ObjectList { elements: items, .. } = &item_field.value
            else { panic!("item field shape changed") };
            let item = &items[item_elem_idx as usize];
            let dye_field = &item.fields[dye_field_idx as usize];
            assert!(dye_field.present, "dye field must be present after toggle");
            assert_eq!(dye_field.kind, FieldKind::ObjectList, "kind must be ObjectList");
            let FieldValue::ObjectList { count, elements, .. } = &dye_field.value
            else { panic!("dye field value must be ObjectList") };
            assert_eq!(*count, 1, "make-present must materialize count=1");
            assert_eq!(elements.len(), 1, "must have exactly one default element");
            assert_eq!(
                elements[0].class_name, "ItemDyeSaveData",
                "default element class must be ItemDyeSaveData"
            );
            // Every field on the default element should be absent — the
            // caller follows up with set_scalar_field_present to add the
            // RGBA / material / color group values.
            assert!(
                elements[0].fields.iter().all(|f| !f.present),
                "default element fields must all be absent"
            );
            h.save.body.len()
        };
        assert!(
            after_present_len > original_body.len(),
            "body must grow when an absent ObjectList field becomes present \
             (was {} bytes, now {})",
            original_body.len(),
            after_present_len,
        );

        // ── Step 2: toggle the field back to absent. The encoder
        //    skips emission for absent fields, so the body shrinks back
        //    exactly the way it was before the make-present call.
        let rc = unsafe {
            crimson_save_set_object_list_present(
                handle,
                block_idx,
                path.as_ptr(),
                path.len(),
                dye_field_idx,
                0,
            )
        };
        assert_eq!(rc, error::OK, "set_object_list_present(false) failed rc={rc}");

        unsafe {
            let h = &*handle;
            let inv_field = &h.blocks[block_idx as usize].fields[inv_field_idx as usize];
            let FieldValue::ObjectList { elements: containers, .. } = &inv_field.value
            else { panic!("inv field shape changed") };
            let item_field = &containers[inv_elem_idx as usize].fields[item_field_idx as usize];
            let FieldValue::ObjectList { elements: items, .. } = &item_field.value
            else { panic!("item field shape changed") };
            let item = &items[item_elem_idx as usize];
            let dye_field = &item.fields[dye_field_idx as usize];
            assert!(!dye_field.present, "dye field must be absent after final toggle");
            assert_eq!(dye_field.kind, FieldKind::Absent);
        }
        let after_remove = unsafe { (*handle).save.body.clone() };
        assert_eq!(
            after_remove.len(),
            original_body.len(),
            "body length must match original after present→absent cycle"
        );
        assert_eq!(
            after_remove, original_body,
            "body bytes must be byte-identical to original after the full round-trip"
        );

        unsafe { crimson_save_free(handle) };
    }

    /// Schema-only validation: `set_object_list_present` must reject
    /// scalar fields (meta_kind 0/2) with `NOT_OBJECT_LIST`, mirroring
    /// the symmetric rejection `set_scalar_field_present` does for
    /// ObjectList fields.
    #[test]
    fn c_abi_object_list_present_rejects_scalar_field_slot107() {
        let Some(path) = find_save() else {
            eprintln!(
                "skipping c_abi_object_list_present_rejects_scalar_field_slot107: \
                 no slot107/save.save"
            );
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();
        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );

        // Find any block with a scalar field (meta_kind 0 or 2).
        let target = unsafe {
            let h = &*handle;
            h.blocks.iter().enumerate().find_map(|(b_idx, block)| {
                block.fields.iter().find_map(|f| {
                    if matches!(f.meta_kind, 0 | 2) {
                        Some((b_idx as u32, f.field_index))
                    } else {
                        None
                    }
                })
            })
        };
        let Some((block_idx, scalar_field_idx)) = target else {
            unsafe { crimson_save_free(handle) };
            panic!("no scalar field found in slot107 — unexpected save shape");
        };

        let rc = unsafe {
            crimson_save_set_object_list_present(
                handle,
                block_idx,
                ptr::null(),
                0,
                scalar_field_idx,
                1,
            )
        };
        assert_eq!(
            rc,
            error::NOT_OBJECT_LIST,
            "must reject scalar field with NOT_OBJECT_LIST"
        );

        unsafe { crimson_save_free(handle) };
    }

    // ── Length-changing batch entry points (Phase B.5) ─────────────────────

    /// Collect up to `cap` (block_idx, field_idx, original_bytes) triples
    /// of present scalar fields suitable for an absent→present round-trip
    /// test. Each entry is on a distinct parent block to keep the
    /// classification rule from coupling ops together — for the bulk
    /// challenge-completion use case the real ops are also one per block.
    fn collect_present_scalars(
        handle: *mut CrimsonSaveHandle,
        cap: usize,
    ) -> Vec<(u32, u32, Vec<u8>)> {
        let mut out = Vec::new();
        unsafe {
            let h = &*handle;
            for (b_idx, block) in h.blocks.iter().enumerate() {
                for (f_idx, field) in block.fields.iter().enumerate() {
                    if !field.present
                        || !matches!(field.meta_kind, 0 | 2)
                        || field.meta_size == 0
                    {
                        continue;
                    }
                    if field.end <= field.start || field.end > h.save.body.len() {
                        continue;
                    }
                    let bytes = h.save.body[field.start..field.end].to_vec();
                    out.push((b_idx as u32, f_idx as u32, bytes));
                    break; // one per block keeps classification independent
                }
                if out.len() >= cap {
                    break;
                }
            }
        }
        out
    }

    /// Equivalence: clearing N scalar fields via the batch entry point
    /// produces the byte-identical body that running N single-op
    /// `set_scalar_field_present(make_present=0)` calls would. Locks the
    /// "batch is a perf shortcut, not a behavior change" invariant.
    #[test]
    fn c_abi_set_scalar_fields_present_batch_matches_single_op() {
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_set_scalar_fields_present_batch_matches_single_op: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();

        let mut handle_batch: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle_batch) },
            error::OK
        );
        let mut handle_single: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle_single) },
            error::OK
        );

        let targets = collect_present_scalars(handle_batch, 30);
        if targets.is_empty() {
            eprintln!("skipping: no present scalar fields available");
            unsafe { crimson_save_free(handle_batch) };
            unsafe { crimson_save_free(handle_single) };
            return;
        }

        // Build clear-all batch (make_present = 0, bytes ignored).
        let ops: Vec<CrimsonScalarPresentBatchOp> = targets
            .iter()
            .map(|(b, f, _)| CrimsonScalarPresentBatchOp {
                block_idx: *b,
                field_idx: *f,
                path: ptr::null(),
                path_len: 0,
                make_present: 0,
                bytes: ptr::null(),
                bytes_len: 0,
            })
            .collect();
        let mut failed_idx: usize = 0;
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_fields_present_batch(
                    handle_batch,
                    ops.as_ptr(),
                    ops.len(),
                    &mut failed_idx,
                )
            },
            error::OK
        );
        assert_eq!(failed_idx, usize::MAX);

        // Same ops via the single-op API on handle B.
        for (b, f, _) in &targets {
            assert_eq!(
                unsafe {
                    crimson_save_set_scalar_field_present(
                        handle_single,
                        *b,
                        ptr::null(),
                        0,
                        *f,
                        0,
                        ptr::null(),
                        0,
                    )
                },
                error::OK
            );
        }

        let body_batch: &[u8] = unsafe { &(*handle_batch).save.body };
        let body_single: &[u8] = unsafe { &(*handle_single).save.body };
        assert_eq!(
            body_batch.len(),
            body_single.len(),
            "body lengths must match between batch and single-op handles"
        );
        assert!(
            body_batch == body_single,
            "batch body must be byte-identical to N × single-op body"
        );

        unsafe { crimson_save_free(handle_batch) };
        unsafe { crimson_save_free(handle_single) };
    }

    /// Round-trip: clear N scalar fields via the batch entry point, then
    /// re-promote them to present with their original bytes via a second
    /// batch. The body must end byte-identical to the pre-batch original.
    #[test]
    fn c_abi_set_scalar_fields_present_batch_clear_then_restore_roundtrip() {
        let Some(path) = find_save() else {
            eprintln!("skipping clear_then_restore_roundtrip: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();

        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );
        let original_body = unsafe { (*handle).save.body.clone() };

        let targets = collect_present_scalars(handle, 20);
        if targets.is_empty() {
            eprintln!("skipping: no present scalar fields available");
            unsafe { crimson_save_free(handle) };
            return;
        }

        // Phase 1: clear all targets in one batch.
        let clear_ops: Vec<CrimsonScalarPresentBatchOp> = targets
            .iter()
            .map(|(b, f, _)| CrimsonScalarPresentBatchOp {
                block_idx: *b,
                field_idx: *f,
                path: ptr::null(),
                path_len: 0,
                make_present: 0,
                bytes: ptr::null(),
                bytes_len: 0,
            })
            .collect();
        let mut failed_idx: usize = 0;
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_fields_present_batch(
                    handle,
                    clear_ops.as_ptr(),
                    clear_ops.len(),
                    &mut failed_idx,
                )
            },
            error::OK
        );
        let after_clear = unsafe { (*handle).save.body.clone() };
        assert!(
            after_clear.len() < original_body.len(),
            "clearing scalar fields should shrink the body"
        );

        // Phase 2: restore all targets in one batch with original bytes.
        let restore_ops: Vec<CrimsonScalarPresentBatchOp> = targets
            .iter()
            .map(|(b, f, bytes)| CrimsonScalarPresentBatchOp {
                block_idx: *b,
                field_idx: *f,
                path: ptr::null(),
                path_len: 0,
                make_present: 1,
                bytes: bytes.as_ptr(),
                bytes_len: bytes.len(),
            })
            .collect();
        let mut failed_idx2: usize = 0;
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_fields_present_batch(
                    handle,
                    restore_ops.as_ptr(),
                    restore_ops.len(),
                    &mut failed_idx2,
                )
            },
            error::OK
        );
        let after_restore = unsafe { (*handle).save.body.clone() };
        assert_eq!(
            after_restore, original_body,
            "clear-then-restore-with-original-bytes must be byte-identical"
        );

        unsafe { crimson_save_free(handle) };
    }

    /// All-or-nothing: a batch with one bad op (LENGTH_MISMATCH on
    /// make_present=1 with the wrong byte count) must leave the body
    /// byte-identical and surface the failing op index via
    /// `out_failed_op_index`. None of the earlier valid ops should be
    /// observable in the body.
    #[test]
    fn c_abi_set_scalar_fields_present_batch_atomicity() {
        let Some(path) = find_save() else {
            eprintln!("skipping set_scalar_fields_present_batch_atomicity: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();

        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );
        let body_before = unsafe { (*handle).save.body.clone() };

        let targets = collect_present_scalars(handle, 3);
        if targets.len() < 2 {
            eprintln!("skipping: need ≥2 present scalar fields for atomicity test");
            unsafe { crimson_save_free(handle) };
            return;
        }

        // Op 0: valid clear. Op 1: make_present=1 with a deliberately
        // wrong byte count → LENGTH_MISMATCH.
        let (b0, f0, _) = &targets[0];
        let (b1, f1, ref orig1) = targets[1];
        let bad_bytes = vec![0u8; orig1.len() + 1];
        let ops = [
            CrimsonScalarPresentBatchOp {
                block_idx: *b0,
                field_idx: *f0,
                path: ptr::null(),
                path_len: 0,
                make_present: 0,
                bytes: ptr::null(),
                bytes_len: 0,
            },
            CrimsonScalarPresentBatchOp {
                block_idx: b1,
                field_idx: f1,
                path: ptr::null(),
                path_len: 0,
                make_present: 1,
                bytes: bad_bytes.as_ptr(),
                bytes_len: bad_bytes.len(),
            },
        ];
        let mut failed_idx: usize = 0;
        let rc = unsafe {
            crimson_save_set_scalar_fields_present_batch(
                handle,
                ops.as_ptr(),
                ops.len(),
                &mut failed_idx,
            )
        };
        assert_eq!(rc, error::LENGTH_MISMATCH);
        assert_eq!(failed_idx, 1);
        let body_after = unsafe { &(*handle).save.body };
        assert_eq!(
            body_after.len(),
            body_before.len(),
            "body length must not change on failed batch"
        );
        assert!(
            body_after == &body_before,
            "body must be byte-identical to pre-batch on validation failure"
        );

        // Empty-batch sanity: zero ops returns OK + writes the sentinel.
        let mut sentinel_slot: usize = 0;
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_fields_present_batch(
                    handle,
                    ptr::null(),
                    0,
                    &mut sentinel_slot,
                )
            },
            error::OK
        );
        assert_eq!(sentinel_slot, usize::MAX);

        unsafe { crimson_save_free(handle) };
    }

    /// Equivalence: removing N list elements in descending index order
    /// via the batch entry point yields the byte-identical body as
    /// running the same N removes through the single-op
    /// `crimson_save_list_remove_element`. Mirrors the
    /// `set_scalar_fields_batch_matches_single_op` invariant for the
    /// list-shrink case.
    #[test]
    fn c_abi_list_remove_elements_batch_matches_single_op() {
        let Some(path) = find_save() else {
            eprintln!("skipping list_remove_elements_batch_matches_single_op: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();

        let mut handle_batch: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle_batch) },
            error::OK
        );
        let mut handle_single: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle_single) },
            error::OK
        );

        let (block_idx, field_idx, list_len) = unsafe {
            let h = &*handle_batch;
            let (b_idx, f_idx) = find_object_list(&h.blocks)
                .expect("expected a zero1_count_u24 object_list");
            let len = match &h.blocks[b_idx as usize].fields[f_idx as usize].value {
                FieldValue::ObjectList { elements, .. } => elements.len(),
                _ => unreachable!(),
            };
            (b_idx, f_idx, len)
        };
        // Drop up to the last 3 elements (descending order). Skip the
        // test if the list is too small to remove 2+ elements.
        if list_len < 3 {
            eprintln!("skipping: list too small for multi-remove (len={list_len})");
            unsafe { crimson_save_free(handle_batch) };
            unsafe { crimson_save_free(handle_single) };
            return;
        }
        let to_drop = [(list_len - 1) as u32, (list_len - 2) as u32, (list_len - 3) as u32];

        // Batch on handle A.
        let ops: Vec<CrimsonListRemoveBatchOp> = to_drop
            .iter()
            .map(|el| CrimsonListRemoveBatchOp {
                block_idx,
                field_idx,
                path: ptr::null(),
                path_len: 0,
                element_idx: *el,
            })
            .collect();
        let mut failed_idx: usize = 0;
        assert_eq!(
            unsafe {
                crimson_save_list_remove_elements_batch(
                    handle_batch,
                    ops.as_ptr(),
                    ops.len(),
                    &mut failed_idx,
                )
            },
            error::OK
        );
        assert_eq!(failed_idx, usize::MAX);

        // Same removes one-at-a-time on handle B.
        for el in to_drop {
            assert_eq!(
                unsafe {
                    crimson_save_list_remove_element(
                        handle_single,
                        block_idx,
                        ptr::null(),
                        0,
                        field_idx,
                        el,
                    )
                },
                error::OK
            );
        }

        let body_batch: &[u8] = unsafe { &(*handle_batch).save.body };
        let body_single: &[u8] = unsafe { &(*handle_single).save.body };
        assert_eq!(body_batch.len(), body_single.len());
        assert!(
            body_batch == body_single,
            "batch list-remove body must be byte-identical to N × single-op body"
        );

        unsafe { crimson_save_free(handle_batch) };
        unsafe { crimson_save_free(handle_single) };
    }

    /// All-or-nothing: a list-remove batch with one bad op (OUT_OF_RANGE
    /// via an element_idx past the current list length) must leave the
    /// body byte-identical and report the failing op index. The earlier
    /// valid removes must NOT be visible.
    #[test]
    fn c_abi_list_remove_elements_batch_atomicity() {
        let Some(path) = find_save() else {
            eprintln!("skipping list_remove_elements_batch_atomicity: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();

        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );
        let body_before = unsafe { (*handle).save.body.clone() };

        let (block_idx, field_idx, list_len) = unsafe {
            let h = &*handle;
            let (b_idx, f_idx) = find_object_list(&h.blocks)
                .expect("expected a zero1_count_u24 object_list");
            let len = match &h.blocks[b_idx as usize].fields[f_idx as usize].value {
                FieldValue::ObjectList { elements, .. } => elements.len(),
                _ => unreachable!(),
            };
            (b_idx, f_idx, len)
        };
        if list_len < 2 {
            eprintln!("skipping: list too small for atomicity test");
            unsafe { crimson_save_free(handle) };
            return;
        }

        // Op 0: valid remove of the tail. Op 1: bogus element_idx way
        // past the current end → OUT_OF_RANGE. The valid op 0 mutation
        // must be rolled back.
        let ops = [
            CrimsonListRemoveBatchOp {
                block_idx,
                field_idx,
                path: ptr::null(),
                path_len: 0,
                element_idx: (list_len - 1) as u32,
            },
            CrimsonListRemoveBatchOp {
                block_idx,
                field_idx,
                path: ptr::null(),
                path_len: 0,
                element_idx: 0xFFFF_FFFF,
            },
        ];
        let mut failed_idx: usize = 0;
        let rc = unsafe {
            crimson_save_list_remove_elements_batch(
                handle,
                ops.as_ptr(),
                ops.len(),
                &mut failed_idx,
            )
        };
        assert_eq!(rc, error::OUT_OF_RANGE);
        assert_eq!(failed_idx, 1);
        let body_after = unsafe { &(*handle).save.body };
        assert!(
            body_after == &body_before,
            "body must be byte-identical to pre-batch on OUT_OF_RANGE"
        );

        // Empty-batch sanity.
        let mut sentinel_slot: usize = 0;
        assert_eq!(
            unsafe {
                crimson_save_list_remove_elements_batch(
                    handle,
                    ptr::null(),
                    0,
                    &mut sentinel_slot,
                )
            },
            error::OK
        );
        assert_eq!(sentinel_slot, usize::MAX);

        unsafe { crimson_save_free(handle) };
    }

    /// `set_inline_bytes_field` round-trip: load a real save, locate
    /// any present `inline_bytes` field (`meta_kind == 1`), rewrite
    /// it with new bytes via the FFI, verify the body changes; then
    /// rewrite back to the original payload and verify byte-identity.
    /// Mirrors the assertion shape of `c_abi_list_clone_distinct_source`.
    #[test]
    fn c_abi_set_inline_bytes_field_roundtrip() {
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_set_inline_bytes_field_roundtrip: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();
        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );

        // Find any present inline_bytes field in a top-level block. Live
        // saves consistently carry several (e.g. _mercenaryName,
        // _accountUserName); pick the first one we encounter so the test
        // doesn't bind to a specific class shape.
        let target = unsafe {
            let h = &*handle;
            let mut chosen: Option<(u32, u32, Vec<u8>, u32)> = None;
            'outer: for (b_idx, block) in h.blocks.iter().enumerate() {
                for (f_idx, field) in block.fields.iter().enumerate() {
                    if field.meta_kind != 1 || !field.present {
                        continue;
                    }
                    if let FieldValue::InlineBytes { count, bytes } = &field.value {
                        chosen = Some((b_idx as u32, f_idx as u32, bytes.clone(), *count));
                        break 'outer;
                    }
                }
            }
            chosen
        };
        let Some((block_idx, field_idx, original_bytes, _original_count)) = target else {
            eprintln!(
                "skipping c_abi_set_inline_bytes_field_roundtrip: no present inline_bytes field"
            );
            unsafe { crimson_save_free(handle) };
            return;
        };

        let original_body = unsafe { (*handle).save.body.clone() };

        // Rewrite with new bytes the same length as the original — keeps
        // the body length stable so any change must come from payload
        // bytes themselves, not cascading offsets.
        let mut replacement = original_bytes.clone();
        for b in replacement.iter_mut() {
            *b = b.wrapping_add(1);
        }
        let rc = unsafe {
            crimson_save_set_inline_bytes_field(
                handle,
                block_idx,
                ptr::null(),
                0,
                field_idx,
                replacement.as_ptr(),
                replacement.len(),
            )
        };
        assert_eq!(rc, error::OK, "set_inline_bytes_field failed rc={rc}");
        let after_write = unsafe { (*handle).save.body.clone() };
        assert_ne!(
            after_write, original_body,
            "body should differ after the in-place inline_bytes overwrite"
        );

        // Restore exactly. Round-trip must match the original body
        // byte-for-byte once the encoder canonicalisation settles.
        let rc = unsafe {
            crimson_save_set_inline_bytes_field(
                handle,
                block_idx,
                ptr::null(),
                0,
                field_idx,
                original_bytes.as_ptr(),
                original_bytes.len(),
            )
        };
        assert_eq!(rc, error::OK, "second set_inline_bytes_field failed rc={rc}");
        let after_restore = unsafe { (*handle).save.body.clone() };
        assert_eq!(
            after_restore, original_body,
            "round-trip rewrite-to-original must reproduce the original body"
        );

        unsafe { crimson_save_free(handle) };
    }

    /// `get_inline_bytes_field` round-trip: load a real save, locate any
    /// present `inline_bytes` field, and assert the two-call getter
    /// returns exactly the bytes the decoder holds. Also checks the
    /// probe (`buf_len == 0`) reports the right `required` length and
    /// `BUFFER_TOO_SMALL` when the payload is non-empty.
    #[test]
    fn c_abi_get_inline_bytes_field_roundtrip() {
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_get_inline_bytes_field_roundtrip: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();
        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );

        let target = unsafe {
            let h = &*handle;
            let mut chosen: Option<(u32, u32, Vec<u8>)> = None;
            'outer: for (b_idx, block) in h.blocks.iter().enumerate() {
                for (f_idx, field) in block.fields.iter().enumerate() {
                    if field.meta_kind != 1 || !field.present {
                        continue;
                    }
                    if let FieldValue::InlineBytes { bytes, .. } = &field.value {
                        chosen = Some((b_idx as u32, f_idx as u32, bytes.clone()));
                        break 'outer;
                    }
                }
            }
            chosen
        };
        let Some((block_idx, field_idx, expected)) = target else {
            eprintln!("skipping c_abi_get_inline_bytes_field_roundtrip: no present inline_bytes field");
            unsafe { crimson_save_free(handle) };
            return;
        };

        // Probe: buf_len = 0 reports the byte count; non-empty payload
        // also yields BUFFER_TOO_SMALL.
        let mut required: usize = 0;
        let rc_probe = unsafe {
            crimson_save_get_inline_bytes_field(
                handle, block_idx, ptr::null(), 0, field_idx,
                ptr::null_mut(), 0, &mut required,
            )
        };
        assert_eq!(required, expected.len(), "probe required must equal payload length");
        if expected.is_empty() {
            assert_eq!(rc_probe, error::OK);
        } else {
            assert_eq!(rc_probe, error::BUFFER_TOO_SMALL);
        }

        // Fill: sized buffer returns OK and the exact bytes.
        let mut buf = vec![0u8; required];
        let mut req2: usize = 0;
        let rc_fill = unsafe {
            crimson_save_get_inline_bytes_field(
                handle, block_idx, ptr::null(), 0, field_idx,
                buf.as_mut_ptr(), buf.len(), &mut req2,
            )
        };
        assert_eq!(rc_fill, error::OK, "get_inline_bytes_field fill failed rc={rc_fill}");
        assert_eq!(req2, expected.len());
        assert_eq!(buf, expected, "returned bytes must match the decoder payload");

        unsafe { crimson_save_free(handle) };
    }

    /// `set_inline_bytes_field` rejects fixed-size scalar fields with
    /// `NOT_INLINE_BYTES` instead of corrupting them. Synthetic-no-save
    /// rejection of mis-kind targets is the kind of regression test
    /// that would have caught the case where a typo'd field index
    /// pointed at a scalar.
    #[test]
    fn c_abi_set_inline_bytes_field_rejects_non_inline_bytes_kind() {
        let Some(path) = find_save() else {
            eprintln!(
                "skipping c_abi_set_inline_bytes_field_rejects_non_inline_bytes_kind: no live save"
            );
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();
        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );

        // Find any fixed-size scalar (meta_kind 0 or 2) in any block.
        let target = unsafe {
            let h = &*handle;
            let mut chosen = None;
            'outer: for (b_idx, block) in h.blocks.iter().enumerate() {
                for (f_idx, field) in block.fields.iter().enumerate() {
                    if matches!(field.meta_kind, 0 | 2) && field.meta_size > 0 {
                        chosen = Some((b_idx as u32, f_idx as u32));
                        break 'outer;
                    }
                }
            }
            chosen
        };
        let Some((block_idx, field_idx)) = target else {
            eprintln!(
                "skipping c_abi_set_inline_bytes_field_rejects_non_inline_bytes_kind: no scalar"
            );
            unsafe { crimson_save_free(handle) };
            return;
        };

        let rc = unsafe {
            crimson_save_set_inline_bytes_field(
                handle,
                block_idx,
                ptr::null(),
                0,
                field_idx,
                [0u8; 4].as_ptr(),
                4,
            )
        };
        assert_eq!(
            rc,
            error::NOT_INLINE_BYTES,
            "scalar field must be rejected with NOT_INLINE_BYTES (got rc={rc})"
        );

        unsafe { crimson_save_free(handle) };
    }

    /// NULL-pointer arguments to `set_inline_bytes_field` must return
    /// `NULL_ARG` cleanly, not segfault.
    #[test]
    fn c_abi_set_inline_bytes_field_null_args() {
        // Handle null.
        let rc = unsafe {
            crimson_save_set_inline_bytes_field(ptr::null_mut(), 0, ptr::null(), 0, 0, ptr::null(), 0)
        };
        assert_eq!(rc, error::NULL_ARG);

        // path null but path_len > 0.
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_set_inline_bytes_field_null_args path-len: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();
        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );
        let rc = unsafe {
            crimson_save_set_inline_bytes_field(handle, 0, ptr::null(), 1, 0, ptr::null(), 0)
        };
        assert_eq!(rc, error::NULL_ARG);

        // bytes null but len > 0.
        let rc = unsafe {
            crimson_save_set_inline_bytes_field(handle, 0, ptr::null(), 0, 0, ptr::null(), 4)
        };
        assert_eq!(rc, error::NULL_ARG);

        unsafe { crimson_save_free(handle) };
    }

    // ── Mutation version + inventory enumeration ─────────────────────────

    /// Verifies the `mutation_version` counter contract:
    /// - starts at 0
    /// - bumps exactly once per successful mutation
    /// - doesn't bump on pure reads
    /// - doesn't bump on failed mutations
    /// - returns `NULL_ARG` on null pointers
    ///
    /// Drives the counter through a `crimson_save_set_scalar_field`
    /// mutation found by the existing `find_nested_u32_scalar`
    /// helper. Skips when no live save is present.
    #[test]
    fn c_abi_mutation_version_bumps_on_mutation_only() {
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_mutation_version_bumps_on_mutation_only: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();
        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );

        // ── Initial version is 0 ──────────────────────────────────
        let mut v0: u64 = u64::MAX;
        assert_eq!(
            unsafe { crimson_save_get_mutation_version(handle, &mut v0) },
            error::OK
        );
        assert_eq!(v0, 0, "fresh handle must start at version 0");

        // ── Pure reads must not bump ──────────────────────────────
        let mut block_count: u32 = 0;
        assert_eq!(
            unsafe { crimson_save_get_block_count(handle, &mut block_count) },
            error::OK
        );
        let mut info = CrimsonBlockInfo::default();
        assert_eq!(
            unsafe { crimson_save_get_block_info(handle, 0, &mut info) },
            error::OK
        );
        let _json = read_block_json(handle, 0);
        let mut count_records: usize = 0;
        let mut inv_version: u64 = 99;
        let _ = unsafe {
            crimson_save_list_inventory_items(
                handle,
                ptr::null_mut(),
                0,
                &mut count_records,
                &mut inv_version,
            )
        };
        assert_eq!(inv_version, 0, "list_inventory_items must not bump");
        let mut v_after_reads: u64 = u64::MAX;
        unsafe { crimson_save_get_mutation_version(handle, &mut v_after_reads) };
        assert_eq!(v_after_reads, 0, "read-only calls must not bump");

        // ── A failed mutation must not bump ───────────────────────
        let zeros = [0u8; 4];
        let rc = unsafe {
            crimson_save_set_scalar_field(handle, u32::MAX, 0, zeros.as_ptr(), zeros.len())
        };
        assert_eq!(rc, error::OUT_OF_RANGE);
        let mut v_after_fail: u64 = u64::MAX;
        unsafe { crimson_save_get_mutation_version(handle, &mut v_after_fail) };
        assert_eq!(v_after_fail, 0, "failed mutation must not bump");

        // ── A successful mutation bumps by exactly 1 ──────────────
        let Some((block_idx, step, leaf_idx, original, len)) = find_nested_u32_scalar(handle)
        else {
            unsafe { crimson_save_free(handle) };
            panic!("expected a u32 scalar in the live save; fixture drift");
        };
        assert_eq!(len, 4);
        // Write the original bytes back through the path-addressed
        // setter (find_nested_u32_scalar gives us a nested target).
        // Still counts as a mutation regardless of whether bytes
        // actually changed.
        let bytes = original.to_le_bytes();
        let steps = [step];
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_field_path(
                    handle,
                    block_idx,
                    steps.as_ptr(),
                    steps.len(),
                    leaf_idx,
                    bytes.as_ptr(),
                    bytes.len(),
                )
            },
            error::OK
        );
        let mut v1: u64 = 0;
        unsafe { crimson_save_get_mutation_version(handle, &mut v1) };
        assert_eq!(v1, 1, "successful mutation must bump version by exactly 1");

        // Another mutation bumps again.
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_field_path(
                    handle,
                    block_idx,
                    steps.as_ptr(),
                    steps.len(),
                    leaf_idx,
                    bytes.as_ptr(),
                    bytes.len(),
                )
            },
            error::OK
        );
        let mut v2: u64 = 0;
        unsafe { crimson_save_get_mutation_version(handle, &mut v2) };
        assert_eq!(v2, 2);

        // ── NULL_ARG paths ────────────────────────────────────────
        let mut sink: u64 = 0;
        assert_eq!(
            unsafe { crimson_save_get_mutation_version(ptr::null(), &mut sink) },
            error::NULL_ARG
        );
        assert_eq!(
            unsafe { crimson_save_get_mutation_version(handle, ptr::null_mut()) },
            error::NULL_ARG
        );

        unsafe { crimson_save_free(handle) };
    }

    /// Live-save integration: walk every InventorySaveData and confirm
    /// the flat list matches the per-container counts in the schema
    /// probe (`_probe_inventory_save_data_schema` baseline:
    /// 543 items across 18 containers in the user's 1.07 sample).
    /// Asserts record-shape invariants (inventory_key non-zero, no
    /// duplicate (inv, slot) pairs within a single container) and the
    /// staleness-detection contract (version stamp matches the
    /// handle's current version at read time).
    #[test]
    fn c_abi_list_inventory_items_live() {
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_list_inventory_items_live: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();
        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );

        // Sizing call.
        let mut count: usize = 0;
        let mut version: u64 = u64::MAX;
        let rc = unsafe {
            crimson_save_list_inventory_items(
                handle,
                ptr::null_mut(),
                0,
                &mut count,
                &mut version,
            )
        };
        // count > 0 → BUFFER_TOO_SMALL, count == 0 → OK directly.
        assert!(
            rc == error::OK || rc == error::BUFFER_TOO_SMALL,
            "first call returned {rc}"
        );
        assert_eq!(version, 0, "fresh handle: version stamp must be 0");
        if count == 0 {
            eprintln!("save has zero inventory items — skipping fill phase");
            unsafe { crimson_save_free(handle) };
            return;
        }
        assert!(count > 100, "expected >100 items in a live save, got {count}");

        // Fill phase.
        let mut buf: Vec<CrimsonInventoryItemRecord> =
            vec![unsafe { std::mem::zeroed() }; count];
        let mut count2: usize = 0;
        let mut version2: u64 = u64::MAX;
        let rc = unsafe {
            crimson_save_list_inventory_items(
                handle,
                buf.as_mut_ptr(),
                buf.len(),
                &mut count2,
                &mut version2,
            )
        };
        assert_eq!(rc, error::OK);
        assert_eq!(count2, count);
        assert_eq!(version2, version);

        // Record-shape invariants.
        let mut seen_inv_keys: std::collections::HashSet<u32> = Default::default();
        for r in &buf {
            // Every record must address a real save block.
            // inventory_key should be in the documented 1..20 range.
            assert!(
                r.inventory_key > 0 && r.inventory_key < 100,
                "inventory_key out of plausible range: {}",
                r.inventory_key
            );
            seen_inv_keys.insert(r.inventory_key);
        }
        // 1.07 sample has at least ~7 distinct container categories.
        assert!(
            seen_inv_keys.len() >= 3,
            "expected ≥3 distinct inventory_key values, got {} ({:?})",
            seen_inv_keys.len(),
            seen_inv_keys
        );

        // BUFFER_TOO_SMALL path: explicitly under-allocate.
        if count >= 2 {
            let mut small_buf: Vec<CrimsonInventoryItemRecord> =
                vec![unsafe { std::mem::zeroed() }; count - 1];
            let mut needed: usize = 0;
            let mut v: u64 = 0;
            let rc = unsafe {
                crimson_save_list_inventory_items(
                    handle,
                    small_buf.as_mut_ptr(),
                    small_buf.len(),
                    &mut needed,
                    &mut v,
                )
            };
            assert_eq!(rc, error::BUFFER_TOO_SMALL);
            assert_eq!(needed, count);
        }

        // Version-stamp staleness contract: a mutation invalidates
        // the snapshot. Drive a 4-byte scalar write into block 0 and
        // observe that the new list call returns a higher version.
        let bytes = [0u8; 4];
        let _ = unsafe {
            crimson_save_set_scalar_field(handle, 0, 0, bytes.as_ptr(), bytes.len())
        };
        let mut new_version: u64 = 0;
        let _ = unsafe {
            crimson_save_list_inventory_items(
                handle,
                ptr::null_mut(),
                0,
                &mut count2,
                &mut new_version,
            )
        };
        // We don't assert WHETHER the field-0 write succeeded — block 0
        // schemas differ across saves. We DO assert that *if* it
        // succeeded, the version bumped; *if* it failed, version stayed.
        let mut current_version: u64 = 0;
        unsafe { crimson_save_get_mutation_version(handle, &mut current_version) };
        assert_eq!(
            new_version, current_version,
            "list_inventory_items must report the live mutation_version"
        );

        // NULL_ARG paths.
        let mut sink: usize = 0;
        assert_eq!(
            unsafe {
                crimson_save_list_inventory_items(
                    ptr::null(),
                    ptr::null_mut(),
                    0,
                    &mut sink,
                    ptr::null_mut(),
                )
            },
            error::NULL_ARG
        );
        assert_eq!(
            unsafe {
                crimson_save_list_inventory_items(
                    handle,
                    ptr::null_mut(),
                    0,
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            },
            error::NULL_ARG
        );
        // Null buffer with non-zero capacity.
        assert_eq!(
            unsafe {
                crimson_save_list_inventory_items(
                    handle,
                    ptr::null_mut(),
                    1,
                    &mut sink,
                    ptr::null_mut(),
                )
            },
            error::NULL_ARG
        );

        unsafe { crimson_save_free(handle) };
    }

    /// End-to-end live-save smoke test for
    /// [`crimson_save_list_character_refs`]. Mirrors the
    /// `_inventory_items` test's two-call + NULL_ARG + buffer-too-small
    /// shape. Asserts that the flat list:
    ///
    /// - Returns a plausible row count (≥ 50 distinct character_key
    ///   values in a typical 1.07 save).
    /// - Resolves each `character_key` through the gamedata-side
    ///   `crimson_characterinfo_lookup_string_key` when the live install
    ///   is present — every emitted key MUST exist in the catalog (no
    ///   silent zeros / garbage).
    /// - Reports the same `class_index` for every record from the same
    ///   `block_idx` (the top-level class doesn't change mid-block).
    #[test]
    fn c_abi_list_character_refs_live() {
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_list_character_refs_live: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();
        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );

        // Sizing call.
        let mut count: usize = 0;
        let mut version: u64 = u64::MAX;
        let rc = unsafe {
            crimson_save_list_character_refs(
                handle,
                ptr::null_mut(),
                0,
                &mut count,
                &mut version,
            )
        };
        assert!(
            rc == error::OK || rc == error::BUFFER_TOO_SMALL,
            "first call returned {rc}"
        );
        assert_eq!(version, 0, "fresh handle: version stamp must be 0");
        if count == 0 {
            eprintln!("save has zero CharacterKey refs — surprising but legal");
            unsafe { crimson_save_free(handle) };
            return;
        }
        assert!(
            count >= 50,
            "expected ≥50 CharacterKey refs in a live save, got {count}",
        );

        // Fill phase.
        let mut buf: Vec<CrimsonCharacterRefRecord> =
            vec![unsafe { std::mem::zeroed() }; count];
        let mut count2: usize = 0;
        let mut version2: u64 = u64::MAX;
        let rc = unsafe {
            crimson_save_list_character_refs(
                handle,
                buf.as_mut_ptr(),
                buf.len(),
                &mut count2,
                &mut version2,
            )
        };
        assert_eq!(rc, error::OK);
        assert_eq!(count2, count);
        assert_eq!(version2, version);

        // Record-shape invariants.
        let mut by_block: std::collections::HashMap<u32, u32> = Default::default();
        let mut distinct_keys: std::collections::HashSet<u32> = Default::default();
        for r in &buf {
            // character_key 0 is illegal — gamedata catalog uses non-zero keys.
            assert_ne!(r.character_key, 0, "CharacterKey 0 leaked into record");
            assert_eq!(r.reserved0, 0, "reserved0 must be zero");
            distinct_keys.insert(r.character_key);
            // Top-level class_index is stable per block.
            if let Some(prev) = by_block.get(&r.block_idx) {
                assert_eq!(
                    *prev, r.class_index,
                    "class_index drift within block {}",
                    r.block_idx
                );
            } else {
                by_block.insert(r.block_idx, r.class_index);
            }
        }
        assert!(
            distinct_keys.len() >= 20,
            "expected ≥20 distinct character_key values, got {}",
            distinct_keys.len()
        );

        // BUFFER_TOO_SMALL path.
        if count >= 2 {
            let mut small_buf: Vec<CrimsonCharacterRefRecord> =
                vec![unsafe { std::mem::zeroed() }; count - 1];
            let mut needed: usize = 0;
            let mut v: u64 = 0;
            let rc = unsafe {
                crimson_save_list_character_refs(
                    handle,
                    small_buf.as_mut_ptr(),
                    small_buf.len(),
                    &mut needed,
                    &mut v,
                )
            };
            assert_eq!(rc, error::BUFFER_TOO_SMALL);
            assert_eq!(needed, count);
        }

        // NULL_ARG paths.
        let mut sink: usize = 0;
        assert_eq!(
            unsafe {
                crimson_save_list_character_refs(
                    ptr::null(),
                    ptr::null_mut(),
                    0,
                    &mut sink,
                    ptr::null_mut(),
                )
            },
            error::NULL_ARG
        );
        assert_eq!(
            unsafe {
                crimson_save_list_character_refs(
                    handle,
                    ptr::null_mut(),
                    0,
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            },
            error::NULL_ARG
        );
        assert_eq!(
            unsafe {
                crimson_save_list_character_refs(
                    handle,
                    ptr::null_mut(),
                    1,
                    &mut sink,
                    ptr::null_mut(),
                )
            },
            error::NULL_ARG
        );

        unsafe { crimson_save_free(handle) };
    }

    // ── Deferred-redecode batch ────────────────────────────────────────────

    /// Lifecycle smoke test: begin → mutate → end commits, version
    /// bumps exactly once for the whole batch, byte image matches a
    /// reference run of the same mutations in normal mode.
    #[test]
    fn c_abi_deferred_redecode_commits_and_matches_normal_mode() {
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_deferred_redecode_commits_and_matches_normal_mode: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();

        // ── Reference handle: run the mutations in normal mode ──────
        let mut href: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut href) },
            error::OK
        );
        let Some((block_idx, step, leaf_idx, original_u32, len)) =
            find_nested_u32_scalar(href)
        else {
            unsafe { crimson_save_free(href) };
            panic!("expected a u32 scalar in the live save; fixture drift");
        };
        assert_eq!(len, 4);
        // Pick a sentinel that differs from the original so we can prove
        // the mutation actually landed.
        let sentinel: u32 = original_u32.wrapping_add(0x0FAD_BEEF);
        let sentinel_bytes = sentinel.to_le_bytes();
        let steps = [step];
        // Normal mode: same 3 mutations, each triggers its own decode.
        for _ in 0..3 {
            assert_eq!(
                unsafe {
                    crimson_save_set_scalar_field_path(
                        href,
                        block_idx,
                        steps.as_ptr(),
                        steps.len(),
                        leaf_idx,
                        sentinel_bytes.as_ptr(),
                        sentinel_bytes.len(),
                    )
                },
                error::OK
            );
        }
        let ref_body = unsafe { &*href }.save.body.clone();
        let mut ref_version: u64 = 0;
        unsafe { crimson_save_get_mutation_version(href, &mut ref_version) };
        assert_eq!(ref_version, 3, "normal mode bumps once per call");
        unsafe { crimson_save_free(href) };

        // ── Batched handle: same 3 mutations, single end ───────────
        let mut hb: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut hb) },
            error::OK
        );

        // is_open getter starts false.
        let mut open: i32 = -1;
        assert_eq!(
            unsafe { crimson_save_is_deferred_redecode_open(hb, &mut open) },
            error::OK
        );
        assert_eq!(open, 0);

        assert_eq!(
            unsafe { crimson_save_begin_deferred_redecode(hb) },
            error::OK
        );
        assert_eq!(
            unsafe { crimson_save_is_deferred_redecode_open(hb, &mut open) },
            error::OK
        );
        assert_eq!(open, 1);

        // Nested begin rejected.
        assert_eq!(
            unsafe { crimson_save_begin_deferred_redecode(hb) },
            error::BATCH_IN_PROGRESS
        );

        // write_to_file rejected while a batch is open.
        let tmp_path = std::env::temp_dir().join("crimson_deferred_test.save");
        let c_tmp = CString::new(tmp_path.to_str().unwrap()).unwrap();
        assert_eq!(
            unsafe { crimson_save_write_to_file(hb, c_tmp.as_ptr()) },
            error::BATCH_IN_PROGRESS
        );

        // Three mutations inside the batch — same target, same bytes.
        for _ in 0..3 {
            assert_eq!(
                unsafe {
                    crimson_save_set_scalar_field_path(
                        hb,
                        block_idx,
                        steps.as_ptr(),
                        steps.len(),
                        leaf_idx,
                        sentinel_bytes.as_ptr(),
                        sentinel_bytes.len(),
                    )
                },
                error::OK
            );
        }
        // Mid-batch the version must NOT have bumped — single bump on end.
        let mut mid_version: u64 = u64::MAX;
        unsafe { crimson_save_get_mutation_version(hb, &mut mid_version) };
        assert_eq!(
            mid_version, 0,
            "mid-batch mutations must not bump the version counter"
        );

        // End commits.
        assert_eq!(unsafe { crimson_save_end_deferred_redecode(hb) }, error::OK);

        // Exactly one version bump for the whole batch.
        let mut end_version: u64 = u64::MAX;
        unsafe { crimson_save_get_mutation_version(hb, &mut end_version) };
        assert_eq!(end_version, 1, "end_deferred_redecode bumps version once");

        // Body image must equal the normal-mode reference (same ops,
        // same target, same bytes — encoder is deterministic).
        let batched_body = unsafe { &*hb }.save.body.clone();
        assert_eq!(
            batched_body, ref_body,
            "batched body bytes must match the normal-mode reference"
        );

        // is_open now false again.
        assert_eq!(
            unsafe { crimson_save_is_deferred_redecode_open(hb, &mut open) },
            error::OK
        );
        assert_eq!(open, 0);

        // end / abort without a batch open returns BATCH_NOT_OPEN.
        assert_eq!(
            unsafe { crimson_save_end_deferred_redecode(hb) },
            error::BATCH_NOT_OPEN
        );
        assert_eq!(
            unsafe { crimson_save_abort_deferred_redecode(hb) },
            error::BATCH_NOT_OPEN
        );

        unsafe { crimson_save_free(hb) };
    }

    /// Abort path: discards every mutation since begin, restores the
    /// pre-begin tree + version. Subsequent reads see the original
    /// scalar value, not the sentinel applied during the batch.
    #[test]
    fn c_abi_deferred_redecode_abort_restores_pre_begin() {
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_deferred_redecode_abort_restores_pre_begin: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();
        let mut handle: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut handle) },
            error::OK
        );
        let Some((block_idx, step, leaf_idx, original_u32, _)) =
            find_nested_u32_scalar(handle)
        else {
            unsafe { crimson_save_free(handle) };
            panic!("expected a u32 scalar in the live save; fixture drift");
        };

        // Bump once outside the batch so we can prove abort restores
        // to a non-zero version rather than just "version == 0".
        let sentinel_outside: u32 = original_u32.wrapping_add(1);
        let outside_bytes = sentinel_outside.to_le_bytes();
        let steps = [step];
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_field_path(
                    handle,
                    block_idx,
                    steps.as_ptr(),
                    steps.len(),
                    leaf_idx,
                    outside_bytes.as_ptr(),
                    outside_bytes.len(),
                )
            },
            error::OK
        );
        let mut v_before: u64 = 0;
        unsafe { crimson_save_get_mutation_version(handle, &mut v_before) };
        assert_eq!(v_before, 1);

        // Open a batch, apply a different sentinel, then abort.
        assert_eq!(
            unsafe { crimson_save_begin_deferred_redecode(handle) },
            error::OK
        );
        let sentinel_inside: u32 = original_u32.wrapping_add(0xABCD);
        let inside_bytes = sentinel_inside.to_le_bytes();
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_field_path(
                    handle,
                    block_idx,
                    steps.as_ptr(),
                    steps.len(),
                    leaf_idx,
                    inside_bytes.as_ptr(),
                    inside_bytes.len(),
                )
            },
            error::OK
        );
        assert_eq!(
            unsafe { crimson_save_abort_deferred_redecode(handle) },
            error::OK
        );

        // Version restored to its pre-begin value.
        let mut v_after_abort: u64 = u64::MAX;
        unsafe { crimson_save_get_mutation_version(handle, &mut v_after_abort) };
        assert_eq!(
            v_after_abort, v_before,
            "abort_deferred_redecode must restore the pre-begin version"
        );

        // Walk the same nested scalar and confirm it still holds the
        // outside-batch sentinel — the inside-batch mutation was rolled
        // back.
        let h = unsafe { &*handle };
        let parent = match &h.blocks[block_idx as usize].fields[step.field_idx as usize].value {
            FieldValue::Locator { child: Some(c), .. } => c.as_ref(),
            FieldValue::ObjectList { elements, .. } => &elements[step.element_idx as usize],
            _ => panic!("step doesn't navigate to a block"),
        };
        let leaf = &parent.fields[leaf_idx as usize];
        let FieldValue::Scalar(ScalarValue::U32(actual)) = leaf.value else {
            panic!("expected u32 scalar leaf");
        };
        assert_eq!(
            actual, sentinel_outside,
            "abort must restore the in-memory tree to its pre-begin state"
        );

        unsafe { crimson_save_free(handle) };
    }

    /// Mixed length-changing + scalar mutations inside a single batch
    /// commit to the same body bytes as running them sequentially in
    /// normal mode. Covers the real C# editor workflow shape
    /// (ListCloneElement + SetScalarFieldPresent + scalar setters in
    /// the same transaction).
    #[test]
    fn c_abi_deferred_redecode_mixed_length_change_matches_normal_mode() {
        let Some(path) = find_save() else {
            eprintln!("skipping c_abi_deferred_redecode_mixed_length_change_matches_normal_mode: no live save");
            return;
        };
        let c_path = CString::new(path.to_str().unwrap()).unwrap();

        // Locate a clonable list element + a sibling fixed-size u32
        // scalar inside the same parent block so the test can chain
        // a length-changing op and a scalar op against the same target.
        // The simplest reliable shape: pick any block whose first list
        // has at least one element with a scalar child.
        let mut href: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut href) },
            error::OK
        );
        let target = {
            let h = unsafe { &*href };
            let mut found: Option<(u32, u32, u32, u32, u32, u32)> = None;
            'outer: for (bi, block) in h.blocks.iter().enumerate() {
                for (fi, field) in block.fields.iter().enumerate() {
                    if let FieldValue::ObjectList {
                        elements,
                        header_variant,
                        ..
                    } = &field.value
                    {
                        if *header_variant == "marker_run_plus_zeros" {
                            // Prefer a fixed-size-header list here to keep
                            // this mixed-batch test's target simple; the
                            // marker variant has its own dedicated tests.
                            continue;
                        }
                        if let Some((eli, _el)) = elements.iter().enumerate().next() {
                            // Within this element, find a u32 leaf.
                            let el = &elements[eli];
                            for (lf, leaf) in el.fields.iter().enumerate() {
                                if matches!(
                                    leaf.kind,
                                    FieldKind::FixedPrefix | FieldKind::FixedSuffix
                                ) && leaf.end - leaf.start == 4
                                    && matches!(
                                        leaf.value,
                                        FieldValue::Scalar(ScalarValue::U32(_))
                                    )
                                {
                                    let FieldValue::Scalar(ScalarValue::U32(orig)) = leaf.value
                                    else {
                                        continue;
                                    };
                                    found = Some((
                                        bi as u32,
                                        fi as u32,
                                        eli as u32,
                                        lf as u32,
                                        orig,
                                        elements.len() as u32,
                                    ));
                                    break 'outer;
                                }
                            }
                        }
                    }
                }
            }
            found
        };
        let Some((block_idx, list_fi, el_idx, leaf_fi, original_u32, list_len_before)) = target
        else {
            unsafe { crimson_save_free(href) };
            eprintln!("skipping: no suitable list+scalar shape found in this save");
            return;
        };
        assert!(list_len_before >= 1);
        let sentinel: u32 = original_u32.wrapping_add(0xCAFE_F00D);
        let sentinel_bytes = sentinel.to_le_bytes();

        // ── Normal mode reference ──────────────────────────────────
        // 1. Clone element 0; 2. Set the cloned element's scalar.
        // dst_element_idx = list_len_before puts the clone at the
        // current end of the list, regardless of which element we
        // sourced from — keeps the path invariant for both the
        // reference + batched runs.
        let dst_idx = list_len_before;
        assert_eq!(
            unsafe {
                crimson_save_list_clone_element(
                    href,
                    block_idx,
                    ptr::null(),
                    0,
                    list_fi,
                    el_idx,
                    dst_idx,
                )
            },
            error::OK
        );
        let clone_path = [CrimsonPathStep {
            field_idx: list_fi,
            element_idx: dst_idx,
        }];
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_field_path(
                    href,
                    block_idx,
                    clone_path.as_ptr(),
                    clone_path.len(),
                    leaf_fi,
                    sentinel_bytes.as_ptr(),
                    sentinel_bytes.len(),
                )
            },
            error::OK
        );
        let ref_body = unsafe { &*href }.save.body.clone();
        unsafe { crimson_save_free(href) };

        // ── Batched run ─────────────────────────────────────────────
        let mut hb: *mut CrimsonSaveHandle = ptr::null_mut();
        assert_eq!(
            unsafe { crimson_save_load_from_file(c_path.as_ptr(), &mut hb) },
            error::OK
        );
        assert_eq!(
            unsafe { crimson_save_begin_deferred_redecode(hb) },
            error::OK
        );
        assert_eq!(
            unsafe {
                crimson_save_list_clone_element(
                    hb,
                    block_idx,
                    ptr::null(),
                    0,
                    list_fi,
                    el_idx,
                    dst_idx,
                )
            },
            error::OK
        );
        assert_eq!(
            unsafe {
                crimson_save_set_scalar_field_path(
                    hb,
                    block_idx,
                    clone_path.as_ptr(),
                    clone_path.len(),
                    leaf_fi,
                    sentinel_bytes.as_ptr(),
                    sentinel_bytes.len(),
                )
            },
            error::OK
        );

        // Mid-batch the in-memory tree already reflects the changes:
        // the list has one more element, and the cloned leaf holds
        // the sentinel.
        {
            let h = unsafe { &*hb };
            let FieldValue::ObjectList { elements, .. } =
                &h.blocks[block_idx as usize].fields[list_fi as usize].value
            else {
                panic!("list field disappeared");
            };
            assert_eq!(
                elements.len() as u32,
                list_len_before + 1,
                "clone must extend the list in deferred mode"
            );
            let clone_leaf = &elements[dst_idx as usize].fields[leaf_fi as usize];
            let FieldValue::Scalar(ScalarValue::U32(v)) = clone_leaf.value else {
                panic!("clone leaf isn't u32");
            };
            assert_eq!(v, sentinel, "deferred scalar mutation must be visible in-tree");
        }

        assert_eq!(unsafe { crimson_save_end_deferred_redecode(hb) }, error::OK);

        // Body image matches the normal-mode reference.
        let batched_body = unsafe { &*hb }.save.body.clone();
        assert_eq!(
            batched_body, ref_body,
            "batched body must match normal-mode reference byte-for-byte"
        );

        unsafe { crimson_save_free(hb) };
    }

    /// Null-arg paths for the new deferred-redecode entry points.
    #[test]
    fn c_abi_deferred_redecode_null_args() {
        let mut sink: i32 = -1;
        assert_eq!(
            unsafe { crimson_save_begin_deferred_redecode(ptr::null_mut()) },
            error::NULL_ARG
        );
        assert_eq!(
            unsafe { crimson_save_end_deferred_redecode(ptr::null_mut()) },
            error::NULL_ARG
        );
        assert_eq!(
            unsafe { crimson_save_abort_deferred_redecode(ptr::null_mut()) },
            error::NULL_ARG
        );
        assert_eq!(
            unsafe {
                crimson_save_is_deferred_redecode_open(ptr::null(), &mut sink)
            },
            error::NULL_ARG
        );
    }
}
