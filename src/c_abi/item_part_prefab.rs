//! `_itemKey → _partPrefabKey` bridge — combined handle.
//!
//! There is no `_partPrefabKey` field in `iteminfo.pabgb` or any sibling
//! table. Each `partprefabdyeslotinfo` row describes one **`.pac` mesh**:
//! its slots are the mesh's sub-meshes in order, the last slot's
//! `tail_name` is the `.pac` path, and the row key is the Jenkins
//! [`calculate_checksum`](crate::crypto::checksum::calculate_checksum) of
//! that path in lower case (1,703 / 1,703 rows on 2.06). An item names
//! *character prefabs*, and a prefab is a scene file that lists the `.pac`
//! meshes it is built from. So the join is:
//!
//! ```text
//! ItemKey
//!   → iteminfo.merged_prefab_visual_list[].prefab_names[]  (StringInfoKey)
//!   → stringinfo.pabgb                  → "cd_m0001_00_so_phm_ub_31067"
//!   → <name>.prefab (character PAZ group, 0009 on 2.06)
//!   → its .pac paths                    → ".../cd_m0001_00_so_phm_ub_31066.pac", ...
//!   → calculate_checksum(lower(path))   = PartPrefabKey (row key)
//! ```
//!
//! An item's dye slot numbers are the union of its rows' masks. One prefab
//! usually yields 2–5 rows (upper body, lower body, shoulder, accessories).
//!
//! Two loaders build the join:
//!
//! - `crimson_item_part_prefab_load_from_{bytes,file}_with_prefab_pamt`
//!   (2026-11-27) reads the prefabs from the caller's character PAMT and
//!   follows the chain above. On 2.06 it resolves 909 of the 917
//!   `is_dyeable` items.
//! - `crimson_item_part_prefab_load_from_{bytes,file}` (the original)
//!   matches the prefab *name* against a row's `prefab_name`, which is the
//!   mesh's **first sub-mesh** name. It resolves 70 dyeable items on 2.06,
//!   and five of those to the wrong mesh (sub-mesh names repeat across
//!   meshes: spear 1001163 lands on a horse-saddle hook). Kept for
//!   compatibility.
//!
//! ## Body variants
//!
//! Each `merged_prefab_visual_list` element is one body variant, tagged
//! with the tribe / genders that wear it (`tribe_gender_list`, e.g.
//! `player_1`, `human_female`, `bosshuman_male`). On 2.06, 526 dyeable
//! items have more than one variant with dye rows; 512 of them offer the
//! same dye slot numbers in every variant. In the other 14 the variants
//! differ (usually a male mesh offering more numbers than a female or boss
//! one), and 9 items name a dye slot number with different default
//! materials in different variants. The save keeps one dye list per item,
//! so the item-level lookups take an optional tribe / gender filter and
//! union every matching variant (all variants when the filter is null).
//!
//! ## Input requirements
//!
//! From the gamedata PAZ group (0008): `iteminfo.pabgb`,
//! `stringinfo.pabgb` and `partprefabdyeslotinfo.pabgb` + `.pabgh`
//! (extract them with [`super::paz`]). The prefab loaders also take the
//! path of the PAZ group's `0.pamt` that holds the character `.prefab`
//! files (0009 on 2.06); prefabs are found there by file name.

use std::collections::HashMap;
use std::ffi::CStr;
use std::io;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use super::error;
use crate::binary::BinaryRead;
use crate::part_prefab_dye_slot_info::PartPrefabDyeSlotInfoEntry;

/// One body variant of an item: one `merged_prefab_visual_list` element
/// that resolved to at least one `partprefabdyeslotinfo` row.
struct ItemPrefabVariant {
    /// `tribe_gender_list`, resolved through stringinfo (`"player_1"`, ...).
    tribe_genders: Vec<String>,
    /// Row keys, in prefab / mesh order, deduplicated within the variant.
    keys: Vec<u32>,
}

/// Opaque handle exposing precomputed `ItemKey → Vec<PartPrefabKey>`
/// lookups.
#[repr(C)]
pub struct CrimsonItemPartPrefabHandle {
    /// Joined map. Keys are item keys; values are the list of
    /// part-prefab keys resolvable for that item, in
    /// `iteminfo.merged_prefab_visual_list` traversal order, deduplicated.
    /// Empty entries are dropped from the map.
    part_prefab_keys_by_item: HashMap<u32, Vec<u32>>,
    /// Each part-prefab key's row `prefab_name` (its first sub-mesh name),
    /// parallel to `part_prefab_keys_by_item`, for debugging / display.
    prefab_names_by_item: HashMap<u32, Vec<String>>,
    /// The item's body variants, in list order (same item set as above).
    variants_by_item: HashMap<u32, Vec<ItemPrefabVariant>>,
    /// Every `partprefabdyeslotinfo` row by key.
    rows: HashMap<u32, PartPrefabDyeSlotInfoEntry>,
}

/// The `PartPrefabKey` of the `partprefabdyeslotinfo` row describing the
/// mesh at `pac_path`: the Jenkins checksum of the path in lower case.
pub(crate) fn part_prefab_key_for_pac(pac_path: &str) -> u32 {
    crate::crypto::checksum::calculate_checksum(pac_path.to_ascii_lowercase().as_bytes())
}

/// The `.pac` paths a character `.prefab` references, in file order. The
/// prefab is a reflection-serialized scene; every string in it is
/// `[u32 len][len bytes]`, and the mesh references are the ones ending in
/// `.pac`.
pub(crate) fn prefab_pac_paths(prefab: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(len_bytes) = prefab.get(i..i + 4) {
        let len = u32::from_le_bytes(len_bytes.try_into().unwrap()) as usize;
        if (5..=1024).contains(&len)
            && let Some(s) = prefab.get(i + 4..i + 4 + len)
            && s.ends_with(b".pac")
            && s.iter().all(u8::is_ascii_graphic)
        {
            out.push(String::from_utf8_lossy(s).into_owned());
            i += 4 + len;
        } else {
            i += 1;
        }
    }
    out
}

/// The character `.prefab` files of one PAZ group, by lower-case file stem.
struct PrefabArchive {
    group_dir: PathBuf,
    meta: crate::binary::pamt::PackMeta,
    /// stem → (directory index, file index); first occurrence wins.
    by_stem: HashMap<String, (usize, usize)>,
}

impl PrefabArchive {
    fn open(pamt_path: &Path) -> io::Result<Self> {
        let meta = crate::binary::pamt::PackMeta::parse(&std::fs::read(pamt_path)?, None)?;
        let group_dir = pamt_path
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "PAMT path has no parent"))?
            .to_path_buf();
        let mut by_stem = HashMap::new();
        for (di, d) in meta.directories.iter().enumerate() {
            for (fi, f) in d.files.iter().enumerate() {
                let name = f.name.to_ascii_lowercase();
                if let Some(stem) = name.strip_suffix(".prefab") {
                    by_stem.entry(stem.to_string()).or_insert((di, fi));
                }
            }
        }
        Ok(PrefabArchive { group_dir, meta, by_stem })
    }

    /// The `.pac` paths of prefab `name`, or `None` when the group has no
    /// such prefab or it cannot be extracted.
    fn pac_paths(&self, name: &str) -> Option<Vec<String>> {
        let &(di, fi) = self.by_stem.get(&name.to_ascii_lowercase())?;
        let d = &self.meta.directories[di];
        let bytes = crate::binary::paz::extract_file(
            &self.group_dir,
            &d.files[fi],
            &d.path,
            &self.meta.header.encrypt_info.encrypt_info,
        )
        .ok()?;
        Some(prefab_pac_paths(&bytes))
    }
}

impl CrimsonItemPartPrefabHandle {
    /// The original join: an item prefab name matches a row whose
    /// `prefab_name` (first sub-mesh name) equals it.
    fn from_bytes(
        iteminfo_pabgb: &[u8],
        stringinfo_pabgb: &[u8],
        partprefab_pabgb: &[u8],
        partprefab_pabgh: &[u8],
    ) -> io::Result<Self> {
        let rows = crate::part_prefab_dye_slot_info::parse_part_prefab_dye_slot_info_lossy(
            partprefab_pabgb,
            partprefab_pabgh,
        );
        let mut key_by_name: HashMap<String, u32> = HashMap::with_capacity(rows.len());
        for e in &rows {
            // First occurrence wins on a name collision.
            key_by_name.entry(e.prefab_name.clone()).or_insert(e.key);
        }
        Self::build(iteminfo_pabgb, stringinfo_pabgb, rows, |name| {
            key_by_name.get(name).copied().into_iter().collect()
        })
    }

    /// The prefab join: an item prefab name → its `.prefab` in `prefabs`
    /// → the `.pac` paths it lists → the rows keyed by their checksums.
    fn from_bytes_with_prefabs(
        iteminfo_pabgb: &[u8],
        stringinfo_pabgb: &[u8],
        partprefab_pabgb: &[u8],
        partprefab_pabgh: &[u8],
        prefabs: &PrefabArchive,
    ) -> io::Result<Self> {
        let rows = crate::part_prefab_dye_slot_info::parse_part_prefab_dye_slot_info_lossy(
            partprefab_pabgb,
            partprefab_pabgh,
        );
        let known: std::collections::HashSet<u32> = rows.iter().map(|e| e.key).collect();
        let mut cache: HashMap<String, Vec<u32>> = HashMap::new();
        Self::build(iteminfo_pabgb, stringinfo_pabgb, rows, |name| {
            cache
                .entry(name.to_string())
                .or_insert_with(|| {
                    prefabs
                        .pac_paths(name)
                        .unwrap_or_default()
                        .iter()
                        .map(|p| part_prefab_key_for_pac(p))
                        .filter(|k| known.contains(k))
                        .collect()
                })
                .clone()
        })
    }

    /// Walk iteminfo and resolve every prefab name of every variant to
    /// row keys with `resolve`.
    fn build(
        iteminfo_pabgb: &[u8],
        stringinfo_pabgb: &[u8],
        rows: Vec<PartPrefabDyeSlotInfoEntry>,
        mut resolve: impl FnMut(&str) -> Vec<u32>,
    ) -> io::Result<Self> {
        let si_entries = crate::string_info::StringInfoData::parse_pabgb(stringinfo_pabgb)?;
        let mut si_by_hash: HashMap<u32, String> = HashMap::with_capacity(si_entries.len());
        for e in &si_entries {
            si_by_hash.entry(e.hash).or_insert_with(|| e.value.clone());
        }
        let rows: HashMap<u32, PartPrefabDyeSlotInfoEntry> =
            rows.into_iter().map(|e| (e.key, e)).collect();

        let mut keys_by_item: HashMap<u32, Vec<u32>> = HashMap::new();
        let mut names_by_item: HashMap<u32, Vec<String>> = HashMap::new();
        let mut variants_by_item: HashMap<u32, Vec<ItemPrefabVariant>> = HashMap::new();
        let mut offset = 0usize;
        while offset < iteminfo_pabgb.len() {
            let item = crate::item_info::ItemInfo::read_from(iteminfo_pabgb, &mut offset)?;
            let mut keys: Vec<u32> = Vec::new();
            let mut variants: Vec<ItemPrefabVariant> = Vec::new();
            for pd in &item.merged_prefab_visual_list.items {
                let mut vkeys: Vec<u32> = Vec::new();
                for sik in &pd.prefab_names.items {
                    let Some(name) = si_by_hash.get(&sik.0) else {
                        continue;
                    };
                    for k in resolve(name) {
                        if !vkeys.contains(&k) {
                            vkeys.push(k);
                        }
                    }
                }
                if vkeys.is_empty() {
                    continue;
                }
                for &k in &vkeys {
                    if !keys.contains(&k) {
                        keys.push(k);
                    }
                }
                let tribe_genders = pd
                    .tribe_gender_list
                    .items
                    .iter()
                    .filter_map(|k| si_by_hash.get(&k.0).cloned())
                    .collect();
                variants.push(ItemPrefabVariant { tribe_genders, keys: vkeys });
            }
            if !keys.is_empty() {
                let names = keys.iter().map(|k| rows[k].prefab_name.clone()).collect();
                keys_by_item.insert(item.key.0, keys);
                names_by_item.insert(item.key.0, names);
                variants_by_item.insert(item.key.0, variants);
            }
        }
        Ok(CrimsonItemPartPrefabHandle {
            part_prefab_keys_by_item: keys_by_item,
            prefab_names_by_item: names_by_item,
            variants_by_item,
            rows,
        })
    }

    /// The rows of the item's variants that `tribe_gender` selects (every
    /// variant when `None`), in variant then mesh order. `None` when the
    /// item has no rows or no variant lists `tribe_gender`.
    fn selected_rows(
        &self,
        item_key: u32,
        tribe_gender: Option<&str>,
    ) -> Option<Vec<&PartPrefabDyeSlotInfoEntry>> {
        let variants = self.variants_by_item.get(&item_key)?;
        let mut out: Vec<&PartPrefabDyeSlotInfoEntry> = Vec::new();
        for v in variants {
            if tribe_gender.is_some_and(|t| !v.tribe_genders.iter().any(|g| g == t)) {
                continue;
            }
            for k in &v.keys {
                if let Some(r) = self.rows.get(k)
                    && !out.iter().any(|o| o.key == r.key)
                {
                    out.push(r);
                }
            }
        }
        (!out.is_empty()).then_some(out)
    }

    /// Union of the selected rows' dye slot bits (bit `n` = `_dyeSlotNo` `n`).
    fn dye_slot_bits(&self, item_key: u32, tribe_gender: Option<&str>) -> Option<u32> {
        let rows = self.selected_rows(item_key, tribe_gender)?;
        Some(rows.iter().fold(0, |a, r| a | r.dye_slot_bits()))
    }

    /// Union of the selected rows' channel numbers, dyeable or not.
    fn channel_dye_slot_bits(&self, item_key: u32, tribe_gender: Option<&str>) -> Option<u32> {
        let rows = self.selected_rows(item_key, tribe_gender)?;
        Some(rows.iter().fold(0, |a, r| a | r.channel_dye_slot_bits()))
    }

    /// Default material of `dye_slot_no` over the selected rows: the first
    /// non-empty name in variant then mesh order. `Ok(Some(""))` when the
    /// number is used but unnamed, `Ok(None)` when no selected row uses it,
    /// `Err(())` when nothing is selected.
    fn dye_slot_material(
        &self,
        item_key: u32,
        tribe_gender: Option<&str>,
        dye_slot_no: u8,
    ) -> Result<Option<&str>, ()> {
        let rows = self.selected_rows(item_key, tribe_gender).ok_or(())?;
        let mut used = None;
        for r in rows {
            match r.dye_slot_material(dye_slot_no) {
                Some(m) if !m.is_empty() => return Ok(Some(m)),
                Some(m) => used = used.or(Some(m)),
                None => {}
            }
        }
        Ok(used)
    }
}

// ── Load / free ────────────────────────────────────────────────────────────

/// Parse the three required tables from disk (the original name join;
/// prefer [`crimson_item_part_prefab_load_from_file_with_prefab_pamt`],
/// see the module docs).
///
/// # Safety
/// All four path arguments must be NUL-terminated UTF-8 strings.
/// `out_handle` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_item_part_prefab_load_from_file(
    iteminfo_pabgb_path: *const c_char,
    stringinfo_pabgb_path: *const c_char,
    partprefab_pabgb_path: *const c_char,
    partprefab_pabgh_path: *const c_char,
    out_handle: *mut *mut CrimsonItemPartPrefabHandle,
) -> i32 {
    if iteminfo_pabgb_path.is_null()
        || stringinfo_pabgb_path.is_null()
        || partprefab_pabgb_path.is_null()
        || partprefab_pabgh_path.is_null()
        || out_handle.is_null()
    {
        return error::NULL_ARG;
    }
    unsafe { *out_handle = std::ptr::null_mut() };
    catch_unwind(AssertUnwindSafe(|| {
        let paths = [
            iteminfo_pabgb_path,
            stringinfo_pabgb_path,
            partprefab_pabgb_path,
            partprefab_pabgh_path,
        ];
        let mut buffers: [Vec<u8>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
        for (i, p) in paths.iter().enumerate() {
            let s = match unsafe { CStr::from_ptr(*p) }.to_str() {
                Ok(s) => s,
                Err(_) => return error::INVALID_PATH,
            };
            buffers[i] = match std::fs::read(s) {
                Ok(b) => b,
                Err(_) => return error::IO,
            };
        }
        let [it, si, pp_b, pp_h] = buffers;
        match CrimsonItemPartPrefabHandle::from_bytes(&it, &si, &pp_b, &pp_h) {
            Ok(h) => {
                unsafe { *out_handle = Box::into_raw(Box::new(h)) };
                error::OK
            }
            Err(_) => error::BODY_PARSE,
        }
    }))
    .unwrap_or(error::PANIC)
}

/// Parse the three required tables from in-memory bytes.
///
/// # Safety
/// Each of the four `*_bytes` pointers must point to `*_len` readable
/// bytes (may be null iff length is 0). `out_handle` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_item_part_prefab_load_from_bytes(
    iteminfo_pabgb: *const u8,
    iteminfo_pabgb_len: usize,
    stringinfo_pabgb: *const u8,
    stringinfo_pabgb_len: usize,
    partprefab_pabgb: *const u8,
    partprefab_pabgb_len: usize,
    partprefab_pabgh: *const u8,
    partprefab_pabgh_len: usize,
    out_handle: *mut *mut CrimsonItemPartPrefabHandle,
) -> i32 {
    if out_handle.is_null() {
        return error::NULL_ARG;
    }
    let pairs = [
        (iteminfo_pabgb, iteminfo_pabgb_len),
        (stringinfo_pabgb, stringinfo_pabgb_len),
        (partprefab_pabgb, partprefab_pabgb_len),
        (partprefab_pabgh, partprefab_pabgh_len),
    ];
    for &(p, n) in &pairs {
        if p.is_null() && n != 0 {
            return error::NULL_ARG;
        }
    }
    unsafe { *out_handle = std::ptr::null_mut() };
    catch_unwind(AssertUnwindSafe(|| {
        let slice = |p: *const u8, n: usize| -> &[u8] {
            if n == 0 {
                &[][..]
            } else {
                unsafe { std::slice::from_raw_parts(p, n) }
            }
        };
        match CrimsonItemPartPrefabHandle::from_bytes(
            slice(iteminfo_pabgb, iteminfo_pabgb_len),
            slice(stringinfo_pabgb, stringinfo_pabgb_len),
            slice(partprefab_pabgb, partprefab_pabgb_len),
            slice(partprefab_pabgh, partprefab_pabgh_len),
        ) {
            Ok(h) => {
                unsafe { *out_handle = Box::into_raw(Box::new(h)) };
                error::OK
            }
            Err(_) => error::BODY_PARSE,
        }
    }))
    .unwrap_or(error::PANIC)
}

/// Open the character PAMT for the prefab loaders. `NOT_FOUND` when it
/// lists no `.prefab` file (the caller passed another group).
fn open_prefab_archive(prefab_pamt_path: *const c_char) -> Result<PrefabArchive, i32> {
    let s = unsafe { CStr::from_ptr(prefab_pamt_path) }
        .to_str()
        .map_err(|_| error::INVALID_PATH)?;
    let archive = PrefabArchive::open(Path::new(s)).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied => error::IO,
        _ => error::BODY_PARSE,
    })?;
    if archive.by_stem.is_empty() {
        return Err(error::NOT_FOUND);
    }
    Ok(archive)
}

/// Build the join through the character prefabs (the 2026-11-27 join; see
/// the module docs). Same three tables as
/// [`crimson_item_part_prefab_load_from_bytes`], plus the path of the PAZ
/// group `0.pamt` that holds the character `.prefab` files (0009 on 2.06,
/// `character/bin__/prefab/...`). Prefabs are looked up there by file name;
/// their `.prefab` files are read from that group's `.paz` chunks.
///
/// Returns `IO` when the PAMT cannot be read, `BODY_PARSE` when it or a
/// table does not parse, `NOT_FOUND` when the PAMT lists no `.prefab` file.
///
/// # Safety
/// As [`crimson_item_part_prefab_load_from_bytes`]; `prefab_pamt_path` must
/// be a NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_item_part_prefab_load_from_bytes_with_prefab_pamt(
    iteminfo_pabgb: *const u8,
    iteminfo_pabgb_len: usize,
    stringinfo_pabgb: *const u8,
    stringinfo_pabgb_len: usize,
    partprefab_pabgb: *const u8,
    partprefab_pabgb_len: usize,
    partprefab_pabgh: *const u8,
    partprefab_pabgh_len: usize,
    prefab_pamt_path: *const c_char,
    out_handle: *mut *mut CrimsonItemPartPrefabHandle,
) -> i32 {
    if out_handle.is_null() || prefab_pamt_path.is_null() {
        return error::NULL_ARG;
    }
    let pairs = [
        (iteminfo_pabgb, iteminfo_pabgb_len),
        (stringinfo_pabgb, stringinfo_pabgb_len),
        (partprefab_pabgb, partprefab_pabgb_len),
        (partprefab_pabgh, partprefab_pabgh_len),
    ];
    for &(p, n) in &pairs {
        if p.is_null() && n != 0 {
            return error::NULL_ARG;
        }
    }
    unsafe { *out_handle = std::ptr::null_mut() };
    catch_unwind(AssertUnwindSafe(|| {
        let slice = |p: *const u8, n: usize| -> &[u8] {
            if n == 0 {
                &[][..]
            } else {
                unsafe { std::slice::from_raw_parts(p, n) }
            }
        };
        let archive = match open_prefab_archive(prefab_pamt_path) {
            Ok(a) => a,
            Err(rc) => return rc,
        };
        match CrimsonItemPartPrefabHandle::from_bytes_with_prefabs(
            slice(iteminfo_pabgb, iteminfo_pabgb_len),
            slice(stringinfo_pabgb, stringinfo_pabgb_len),
            slice(partprefab_pabgb, partprefab_pabgb_len),
            slice(partprefab_pabgh, partprefab_pabgh_len),
            &archive,
        ) {
            Ok(h) => {
                unsafe { *out_handle = Box::into_raw(Box::new(h)) };
                error::OK
            }
            Err(_) => error::BODY_PARSE,
        }
    }))
    .unwrap_or(error::PANIC)
}

/// File-path form of
/// [`crimson_item_part_prefab_load_from_bytes_with_prefab_pamt`].
///
/// # Safety
/// All five path arguments must be NUL-terminated UTF-8 strings.
/// `out_handle` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_item_part_prefab_load_from_file_with_prefab_pamt(
    iteminfo_pabgb_path: *const c_char,
    stringinfo_pabgb_path: *const c_char,
    partprefab_pabgb_path: *const c_char,
    partprefab_pabgh_path: *const c_char,
    prefab_pamt_path: *const c_char,
    out_handle: *mut *mut CrimsonItemPartPrefabHandle,
) -> i32 {
    let paths = [
        iteminfo_pabgb_path,
        stringinfo_pabgb_path,
        partprefab_pabgb_path,
        partprefab_pabgh_path,
    ];
    if paths.iter().any(|p| p.is_null()) || prefab_pamt_path.is_null() || out_handle.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_handle = std::ptr::null_mut() };
    catch_unwind(AssertUnwindSafe(|| {
        let mut buffers: [Vec<u8>; 4] = Default::default();
        for (i, p) in paths.iter().enumerate() {
            let Ok(s) = unsafe { CStr::from_ptr(*p) }.to_str() else {
                return error::INVALID_PATH;
            };
            buffers[i] = match std::fs::read(s) {
                Ok(b) => b,
                Err(_) => return error::IO,
            };
        }
        let archive = match open_prefab_archive(prefab_pamt_path) {
            Ok(a) => a,
            Err(rc) => return rc,
        };
        let [it, si, pp_b, pp_h] = buffers;
        match CrimsonItemPartPrefabHandle::from_bytes_with_prefabs(&it, &si, &pp_b, &pp_h, &archive)
        {
            Ok(h) => {
                unsafe { *out_handle = Box::into_raw(Box::new(h)) };
                error::OK
            }
            Err(_) => error::BODY_PARSE,
        }
    }))
    .unwrap_or(error::PANIC)
}

/// Free a handle returned by any loader.
///
/// # Safety
/// `handle` must be null or a pointer previously returned by one of
/// the loaders and not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_item_part_prefab_free(
    handle: *mut CrimsonItemPartPrefabHandle,
) {
    if handle.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(handle);
    }
}

// ── Lookups ────────────────────────────────────────────────────────────────

/// Number of items with at least one resolvable partprefab key. Useful
/// for diagnostics — divide by `iteminfo`'s total item count to get a
/// coverage estimate.
///
/// # Safety
/// `handle` must be live; `out_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_item_part_prefab_resolved_item_count(
    handle: *const CrimsonItemPartPrefabHandle,
    out_count: *mut u32,
) -> i32 {
    if handle.is_null() || out_count.is_null() {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        unsafe { *out_count = h.part_prefab_keys_by_item.len() as u32 };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Number of partprefab keys resolvable for a given item. Returns
/// `NOT_FOUND` (and writes 0) when the item has no resolvable keys.
///
/// # Safety
/// `handle` and `out_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_item_part_prefab_lookup_count(
    handle: *const CrimsonItemPartPrefabHandle,
    item_key: u32,
    out_count: *mut u32,
) -> i32 {
    if handle.is_null() || out_count.is_null() {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        match h.part_prefab_keys_by_item.get(&item_key) {
            Some(v) => {
                unsafe { *out_count = v.len() as u32 };
                error::OK
            }
            None => {
                unsafe { *out_count = 0 };
                error::NOT_FOUND
            }
        }
    }))
    .unwrap_or(error::PANIC)
}

/// Get the partprefab key at insertion order `idx` for the given item.
///
/// Returns `NOT_FOUND` when the item has no resolvable keys,
/// `OUT_OF_RANGE` when `idx >= count`.
///
/// # Safety
/// `handle` and `out_key` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_item_part_prefab_lookup_key_at(
    handle: *const CrimsonItemPartPrefabHandle,
    item_key: u32,
    idx: u32,
    out_key: *mut u32,
) -> i32 {
    if handle.is_null() || out_key.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_key = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let Some(v) = h.part_prefab_keys_by_item.get(&item_key) else {
            return error::NOT_FOUND;
        };
        let Some(&k) = v.get(idx as usize) else {
            return error::OUT_OF_RANGE;
        };
        unsafe { *out_key = k };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Get the prefab name string corresponding to the partprefab key at
/// position `idx`. Same indexing as
/// [`crimson_item_part_prefab_lookup_key_at`]. Two-call pattern.
///
/// Returns `NOT_FOUND` when the item has no resolvable keys,
/// `OUT_OF_RANGE` when `idx >= count`.
///
/// # Safety
/// `handle` and `required` must be non-null; `buf` may be null iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_item_part_prefab_lookup_prefab_name_at(
    handle: *const CrimsonItemPartPrefabHandle,
    item_key: u32,
    idx: u32,
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
        let Some(v) = h.prefab_names_by_item.get(&item_key) else {
            return error::NOT_FOUND;
        };
        let Some(name) = v.get(idx as usize) else {
            return error::OUT_OF_RANGE;
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

/// The optional tribe / gender filter of the item-level dye lookups.
/// `Ok(None)` for a null pointer.
fn tribe_gender_arg<'a>(tribe_gender: *const c_char) -> Result<Option<&'a str>, i32> {
    if tribe_gender.is_null() {
        return Ok(None);
    }
    unsafe { CStr::from_ptr(tribe_gender) }
        .to_str()
        .map(Some)
        .map_err(|_| error::INVALID_PATH)
}

/// The dye slot numbers an item offers: the union of the dye slot bits of
/// every `partprefabdyeslotinfo` row of its body variants (bit `n` =
/// `ItemDyeSaveData._dyeSlotNo` `n`, absent = 0). Equals OR-ing
/// `crimson_part_prefab_dye_slot_info_lookup_dye_slot_bits` over the item's
/// keys when `tribe_gender` is null.
///
/// `tribe_gender` (nullable, NUL-terminated) keeps only the variants whose
/// `tribe_gender_list` names it, e.g. `"player_1"` or `"human_female"`.
///
/// Returns `NOT_FOUND` (and writes 0) when the item has no rows, or no
/// variant lists `tribe_gender`; `INVALID_PATH` when `tribe_gender` is not
/// UTF-8.
///
/// # Safety
/// `handle` and `out_bits` must be non-null; `tribe_gender` must be null or
/// a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_item_part_prefab_lookup_dye_slot_bits(
    handle: *const CrimsonItemPartPrefabHandle,
    item_key: u32,
    tribe_gender: *const c_char,
    out_bits: *mut u32,
) -> i32 {
    if handle.is_null() || out_bits.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_bits = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let tg = match tribe_gender_arg(tribe_gender) {
            Ok(t) => t,
            Err(rc) => return rc,
        };
        let Some(bits) = h.dye_slot_bits(item_key, tg) else {
            return error::NOT_FOUND;
        };
        unsafe { *out_bits = bits };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Every dye slot number a material channel of the item's rows carries
/// (`mat_indices`), whether or not the current mask enables it: a superset
/// of [`crimson_item_part_prefab_lookup_dye_slot_bits`]. Saves written
/// before 2.01 hold `_dyeSlotNo` values in this set but outside the 2.06
/// masks (1.10 fixture: 200914 slot 3, 2111103 slot 2, 103023 slot 1), so
/// an editor should keep such elements rather than treat them as invalid.
/// Same arguments and return codes as `lookup_dye_slot_bits`.
///
/// # Safety
/// As [`crimson_item_part_prefab_lookup_dye_slot_bits`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_item_part_prefab_lookup_channel_dye_slot_bits(
    handle: *const CrimsonItemPartPrefabHandle,
    item_key: u32,
    tribe_gender: *const c_char,
    out_bits: *mut u32,
) -> i32 {
    if handle.is_null() || out_bits.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_bits = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let h = unsafe { &*handle };
        let tg = match tribe_gender_arg(tribe_gender) {
            Ok(t) => t,
            Err(rc) => return rc,
        };
        let Some(bits) = h.channel_dye_slot_bits(item_key, tg) else {
            return error::NOT_FOUND;
        };
        unsafe { *out_bits = bits };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Default material name of dye slot number `dye_slot_no` on an item: the
/// first non-empty name over its rows, in variant then mesh order. Variants
/// can disagree (9 items on 2.06); pass `tribe_gender` to pick one. Writes
/// `""` when the number is used but no channel names a material. Two-call
/// pattern.
///
/// Returns `NOT_FOUND` as [`crimson_item_part_prefab_lookup_dye_slot_bits`],
/// `OUT_OF_RANGE` when no selected row uses `dye_slot_no`.
///
/// # Safety
/// `handle` and `required` must be non-null; `buf` may be null iff
/// `buf_len == 0`; `tribe_gender` must be null or a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_item_part_prefab_lookup_dye_slot_material(
    handle: *const CrimsonItemPartPrefabHandle,
    item_key: u32,
    tribe_gender: *const c_char,
    dye_slot_no: u8,
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
        let tg = match tribe_gender_arg(tribe_gender) {
            Ok(t) => t,
            Err(rc) => return rc,
        };
        let name = match h.dye_slot_material(item_key, tg, dye_slot_no) {
            Err(()) => return error::NOT_FOUND,
            Ok(None) => return error::OUT_OF_RANGE,
            Ok(Some(m)) => m,
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

/// Constants for `out_resolve_source` of
/// [`crimson_item_part_prefab_resolve_dye_slot_count`].
///
/// Numeric stability: these constants are part of the C ABI surface
/// and MUST NOT be reassigned. Add new states with the next free
/// integer; never reuse a retired one.
pub mod resolve_dye_slot_count_source {
    /// Slot count came from a direct
    /// `(item_key → partprefab[0] → slot_count)` chain. The
    /// `out_slot_count` value is authoritative.
    pub const DIRECT: u32 = 0;
    /// `item_key` has no entry in the `item_part_prefab` join — its
    /// mesh prefab(s) aren't in `partprefabdyeslotinfo`. Typically a
    /// body-type variant (goblin / dwarf / tribe mesh) that shares
    /// the human male's dye-slot layout but isn't itself listed.
    /// 76% of `is_dyeable=1` items hit this in 1.07 (387/507).
    /// With a handle from the `_with_prefab_pamt` loaders, 8 of the 917
    /// 2.06 dyeable items hit this (909 resolve).
    /// Editor should fall back to a curated default (e.g. the
    /// human-male equip-slot prefab's slot count) or display
    /// "unknown" with a warning. `out_slot_count` is 0.
    pub const NOT_RESOLVED_NO_PARTPREFAB: u32 = 1;
    /// `item_key` mapped to a partprefab via the join, but that
    /// partprefab key isn't in `partprefabdyeslotinfo`. This
    /// shouldn't happen given how the join is built (it only emits
    /// keys that exist in the slot-info table) but the guard stays
    /// in place for defense-in-depth — a future patch could split
    /// the tables. `out_slot_count` is 0.
    pub const NOT_RESOLVED_NO_SLOT_INFO: u32 = 2;
}

/// One-shot "how many dye slots does this item have?" — chains the
/// existing `item_part_prefab` join and `partprefabdyeslotinfo`
/// lookups into a single call. Saves the C# editor from orchestrating
/// the 3-call sequence
/// (`lookup_count` → `lookup_key_at(0)` →
/// `crimson_part_prefab_dye_slot_info_lookup_slot_count`) for every
/// dyeable item it surfaces.
///
/// **Resolution policy**: always uses `partprefab[0]` — the first
/// resolved prefab in iteminfo's `prefab_data_list` traversal order.
/// Multi-prefab items (rare — mainly mesh variants) typically share a
/// dye-slot layout across all variants; if a future use case needs
/// per-variant resolution, switch to the manual chain via
/// [`crimson_item_part_prefab_lookup_key_at`].
///
/// **Return shape**: always returns `OK` (or `NULL_ARG` / `PANIC` for
/// fatal call errors). The "did it resolve?" question is communicated
/// via `out_resolve_source`:
///
/// - `DIRECT` (0): `out_slot_count` is authoritative.
/// - `NOT_RESOLVED_NO_PARTPREFAB` (1): item has no partprefab mapping;
///   editor should fall back to a default or display "unknown".
/// - `NOT_RESOLVED_NO_SLOT_INFO` (2): partprefab present but missing
///   from the slot-info table (shouldn't happen given how the join
///   is built; defensive).
///
/// In all non-`DIRECT` cases `*out_slot_count = 0`.
///
/// # Safety
/// All four pointer arguments must be non-null. Both handles must be
/// live (returned by their respective loaders and not yet freed).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_item_part_prefab_resolve_dye_slot_count(
    item_part_prefab_handle: *const CrimsonItemPartPrefabHandle,
    slot_info_handle: *const super::part_prefab_dye_slot_info::CrimsonPartPrefabDyeSlotInfoHandle,
    item_key: u32,
    out_slot_count: *mut u32,
    out_resolve_source: *mut u32,
) -> i32 {
    if item_part_prefab_handle.is_null()
        || slot_info_handle.is_null()
        || out_slot_count.is_null()
        || out_resolve_source.is_null()
    {
        return error::NULL_ARG;
    }
    unsafe {
        *out_slot_count = 0;
        *out_resolve_source = resolve_dye_slot_count_source::NOT_RESOLVED_NO_PARTPREFAB;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let ipp = unsafe { &*item_part_prefab_handle };

        // Step 1: ItemKey → partprefab[0].
        let Some(prefab_keys) = ipp.part_prefab_keys_by_item.get(&item_key) else {
            return error::OK; // NOT_RESOLVED_NO_PARTPREFAB (already written)
        };
        let Some(&pp_key) = prefab_keys.first() else {
            // Empty Vec wouldn't be in the map (the join drops empties),
            // but guard defensively.
            return error::OK;
        };

        // Step 2: PartPrefabKey → slot_count via the slot-info bridge.
        let mut count: u32 = 0;
        let rc = unsafe {
            super::part_prefab_dye_slot_info::crimson_part_prefab_dye_slot_info_lookup_slot_count(
                slot_info_handle,
                pp_key,
                &mut count,
            )
        };
        match rc {
            x if x == error::OK => {
                unsafe {
                    *out_slot_count = count;
                    *out_resolve_source = resolve_dye_slot_count_source::DIRECT;
                }
                error::OK
            }
            x if x == error::NOT_FOUND => {
                unsafe {
                    *out_resolve_source =
                        resolve_dye_slot_count_source::NOT_RESOLVED_NO_SLOT_INFO;
                }
                error::OK
            }
            // Pass through any other error (PANIC / NULL_ARG from the
            // slot-info bridge — shouldn't happen given our guards, but
            // don't swallow it silently).
            other => other,
        }
    }))
    .unwrap_or(error::PANIC)
}

#[cfg(test)]
mod tests {
    use crate::binary::gamedata_layout;
    use super::*;
    use crate::c_abi::paz::crimson_paz_extract_file;
    use std::ffi::{CStr, CString};
    use std::path::PathBuf;
    use std::ptr;

    fn find_pamt() -> Option<PathBuf> {
        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let p = game_root.join("0008").join("0.pamt");
        p.is_file().then_some(p)
    }

    fn extract_file(pamt: &CStr, dir: &str, name: &str) -> Vec<u8> {
        let dir_c = CString::new(dir).unwrap();
        let name_c = CString::new(name).unwrap();
        let mut needed: usize = 0;
        let rc = unsafe {
            crimson_paz_extract_file(
                pamt.as_ptr(),
                dir_c.as_ptr(),
                name_c.as_ptr(),
                ptr::null_mut(),
                0,
                &mut needed,
            )
        };
        assert_eq!(rc, error::BUFFER_TOO_SMALL);
        let mut buf = vec![0u8; needed];
        let rc = unsafe {
            crimson_paz_extract_file(
                pamt.as_ptr(),
                dir_c.as_ptr(),
                name_c.as_ptr(),
                buf.as_mut_ptr(),
                buf.len(),
                &mut needed,
            )
        };
        assert_eq!(rc, error::OK);
        buf.truncate(needed);
        buf
    }

    #[test]
    fn c_abi_item_part_prefab_live() {
        let Some(pamt_path) = find_pamt() else {
            eprintln!("skipping c_abi_item_part_prefab_live: no game install");
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let item_b = extract_file(
            pamt.as_c_str(),
            gamedata_layout::bin_dir(),
            &gamedata_layout::body("iteminfo"),
        );
        let si_b = extract_file(
            pamt.as_c_str(),
            gamedata_layout::bin_dir(),
            &gamedata_layout::body("stringinfo"),
        );
        let pp_b = extract_file(
            pamt.as_c_str(),
            gamedata_layout::bin_dir(),
            &gamedata_layout::body("partprefabdyeslotinfo"),
        );
        let pp_h = extract_file(
            pamt.as_c_str(),
            gamedata_layout::bin_dir(),
            &gamedata_layout::header("partprefabdyeslotinfo"),
        );

        let mut h: *mut CrimsonItemPartPrefabHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_item_part_prefab_load_from_bytes(
                item_b.as_ptr(),
                item_b.len(),
                si_b.as_ptr(),
                si_b.len(),
                pp_b.as_ptr(),
                pp_b.len(),
                pp_h.as_ptr(),
                pp_h.len(),
                &mut h,
            )
        };
        assert_eq!(rc, error::OK);
        assert!(!h.is_null());

        let mut resolved_items: u32 = 0;
        assert_eq!(
            unsafe { crimson_item_part_prefab_resolved_item_count(h, &mut resolved_items) },
            error::OK
        );
        // Probe pass observed ~120 items with at least one resolvable
        // key — assert ≥50 to leave headroom for cross-version drift.
        assert!(
            resolved_items >= 50,
            "expected ≥50 items with partprefab keys, got {resolved_items}"
        );

        // NOT_FOUND for an unknown item key.
        let mut c: u32 = 0;
        assert_eq!(
            unsafe { crimson_item_part_prefab_lookup_count(h, u32::MAX, &mut c) },
            error::NOT_FOUND
        );
        assert_eq!(c, 0);

        // Enumerate the first 5 resolved items and pin invariants:
        // each item's keys + names must be non-empty and same length;
        // every returned partprefab key must round-trip its prefab_name.
        // This guards against accidental desync between the two
        // parallel HashMap entries.
        // We have to read the entries through the public lookups; do a
        // brute scan for items with resolved keys by trying a known
        // dyeable item from the linkage probe.
        // Marni_Devotee_PlateArmor_Helm (item_key=14510) showed 0 partprefab
        // hits in the v3 probe — none of its 5 body-type variants live
        // in partprefabdyeslotinfo. So instead, find any item that has
        // a resolved keys by scanning known partprefab prefab_name
        // candidates that DO appear in iteminfo (per the v3 probe's
        // 50 direct-hit count).
        //
        // Simpler: iterate item_keys 1..50000 looking for any with a
        // resolved count. Cap the scan to avoid pathological runtime.
        let mut found_item: Option<u32> = None;
        for k in 1..200000u32 {
            let mut n: u32 = 0;
            if unsafe { crimson_item_part_prefab_lookup_count(h, k, &mut n) } == error::OK
                && n > 0
            {
                found_item = Some(k);
                break;
            }
        }
        let item = found_item.expect("at least one item must have a resolved partprefab key");
        let mut cnt: u32 = 0;
        assert_eq!(
            unsafe { crimson_item_part_prefab_lookup_count(h, item, &mut cnt) },
            error::OK
        );
        assert!(cnt > 0);
        let mut pk: u32 = 0;
        assert_eq!(
            unsafe { crimson_item_part_prefab_lookup_key_at(h, item, 0, &mut pk) },
            error::OK
        );
        assert!(pk != 0, "resolved partprefab key must be non-zero");

        // Prefab name round-trip — two-call pattern.
        let mut req: usize = 0;
        let rc = unsafe {
            crimson_item_part_prefab_lookup_prefab_name_at(
                h,
                item,
                0,
                ptr::null_mut(),
                0,
                &mut req,
            )
        };
        assert_eq!(rc, error::BUFFER_TOO_SMALL);
        assert!(req > 1);
        let mut buf = vec![0u8; req];
        let mut req2: usize = 0;
        let rc = unsafe {
            crimson_item_part_prefab_lookup_prefab_name_at(
                h,
                item,
                0,
                buf.as_mut_ptr(),
                buf.len(),
                &mut req2,
            )
        };
        assert_eq!(rc, error::OK);
        let name = std::str::from_utf8(&buf[..req2 - 1]).unwrap();
        // Prefab names all start with `cd_` per the partprefab schema.
        assert!(
            name.starts_with("cd_"),
            "resolved prefab name does not start with cd_: {name:?}"
        );

        // OUT_OF_RANGE for an idx past the end.
        assert_eq!(
            unsafe { crimson_item_part_prefab_lookup_key_at(h, item, cnt, &mut pk) },
            error::OUT_OF_RANGE
        );

        unsafe { crimson_item_part_prefab_free(h) };
    }

    #[test]
    fn c_abi_item_part_prefab_null_args() {
        let mut sh: *mut CrimsonItemPartPrefabHandle = ptr::null_mut();
        assert_eq!(
            unsafe {
                crimson_item_part_prefab_load_from_bytes(
                    ptr::null(),
                    16,
                    ptr::null(),
                    16,
                    ptr::null(),
                    16,
                    ptr::null(),
                    16,
                    &mut sh,
                )
            },
            error::NULL_ARG,
        );
        assert_eq!(
            unsafe {
                crimson_item_part_prefab_load_from_bytes(
                    [0u8; 1].as_ptr(),
                    1,
                    [0u8; 1].as_ptr(),
                    1,
                    [0u8; 1].as_ptr(),
                    1,
                    [0u8; 1].as_ptr(),
                    1,
                    ptr::null_mut(),
                )
            },
            error::NULL_ARG,
        );
    }

    /// Live test for the dye-slot-count wrapper. Walks the
    /// resolved-items set, asserts every directly-resolved item gives
    /// a non-zero slot count in [1, 8]. Also pins:
    /// - NOT_RESOLVED_NO_PARTPREFAB for an unknown item key.
    /// - DIRECT resolution chains through both bridges correctly
    ///   (slot count matches a manual `lookup_key_at(0)` →
    ///   `lookup_slot_count` chain).
    #[test]
    fn c_abi_item_part_prefab_resolve_dye_slot_count_live() {
        use super::super::part_prefab_dye_slot_info::*;
        let Some(pamt_path) = find_pamt() else {
            eprintln!("skipping: no game install");
            return;
        };
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let item_b =
            extract_file(pamt.as_c_str(), gamedata_layout::bin_dir(), &gamedata_layout::body("iteminfo"));
        let si_b =
            extract_file(pamt.as_c_str(), gamedata_layout::bin_dir(), &gamedata_layout::body("stringinfo"));
        let pp_b = extract_file(
            pamt.as_c_str(),
            gamedata_layout::bin_dir(),
            &gamedata_layout::body("partprefabdyeslotinfo"),
        );
        let pp_h = extract_file(
            pamt.as_c_str(),
            gamedata_layout::bin_dir(),
            &gamedata_layout::header("partprefabdyeslotinfo"),
        );

        // Build both handles.
        let mut ipp_handle: *mut CrimsonItemPartPrefabHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_item_part_prefab_load_from_bytes(
                item_b.as_ptr(), item_b.len(),
                si_b.as_ptr(), si_b.len(),
                pp_b.as_ptr(), pp_b.len(),
                pp_h.as_ptr(), pp_h.len(),
                &mut ipp_handle,
            )
        };
        assert_eq!(rc, error::OK);

        let mut slot_handle: *mut CrimsonPartPrefabDyeSlotInfoHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_part_prefab_dye_slot_info_load_from_bytes(
                pp_b.as_ptr(), pp_b.len(),
                pp_h.as_ptr(), pp_h.len(),
                &mut slot_handle,
            )
        };
        assert_eq!(rc, error::OK);

        // Unknown item key → NOT_RESOLVED_NO_PARTPREFAB.
        let mut count: u32 = 99;
        let mut source: u32 = 99;
        let rc = unsafe {
            crimson_item_part_prefab_resolve_dye_slot_count(
                ipp_handle, slot_handle, u32::MAX,
                &mut count, &mut source,
            )
        };
        assert_eq!(rc, error::OK);
        assert_eq!(count, 0, "unknown key should yield 0 slot count");
        assert_eq!(
            source, resolve_dye_slot_count_source::NOT_RESOLVED_NO_PARTPREFAB,
            "unknown key should report NOT_RESOLVED_NO_PARTPREFAB",
        );

        // Find a directly-resolvable item by scanning the resolved-items set.
        let mut found_item: Option<u32> = None;
        for k in 1..1_000_000u32 {
            let mut n: u32 = 0;
            if unsafe { crimson_item_part_prefab_lookup_count(ipp_handle, k, &mut n) }
                == error::OK
                && n > 0
            {
                found_item = Some(k);
                break;
            }
        }
        let item = found_item.expect("at least one item must have a resolved partprefab key");

        // Resolve via the wrapper.
        let mut wrapper_count: u32 = 0;
        let mut wrapper_source: u32 = 99;
        let rc = unsafe {
            crimson_item_part_prefab_resolve_dye_slot_count(
                ipp_handle, slot_handle, item,
                &mut wrapper_count, &mut wrapper_source,
            )
        };
        assert_eq!(rc, error::OK);
        assert_eq!(
            wrapper_source, resolve_dye_slot_count_source::DIRECT,
            "resolved item should report DIRECT",
        );
        // Slot counts are positive small ints — typical range 1..=16
        // observed in 1.07 (some equipment has many independently-dyeable
        // parts). 32 is a generous defensive cap to flag schema drift.
        assert!(
            (1..=32).contains(&wrapper_count),
            "slot_count {wrapper_count} out of plausible range [1, 32] for item {item}",
        );

        // Manual chain: lookup_key_at(0) → lookup_slot_count.
        let mut manual_pp: u32 = 0;
        assert_eq!(
            unsafe { crimson_item_part_prefab_lookup_key_at(ipp_handle, item, 0, &mut manual_pp) },
            error::OK,
        );
        let mut manual_count: u32 = 0;
        assert_eq!(
            unsafe {
                crimson_part_prefab_dye_slot_info_lookup_slot_count(
                    slot_handle, manual_pp, &mut manual_count,
                )
            },
            error::OK,
        );
        // The wrapper must agree with the manual chain.
        assert_eq!(
            wrapper_count, manual_count,
            "wrapper and manual chain disagree on slot_count for item {item}",
        );

        // Full sweep: every resolved item should give DIRECT + [1, 8] count.
        let mut direct_count = 0u32;
        let mut not_resolved_no_partprefab = 0u32;
        let mut not_resolved_no_slot_info = 0u32;
        for k in 1..1_000_000u32 {
            let mut n: u32 = 0;
            if unsafe { crimson_item_part_prefab_lookup_count(ipp_handle, k, &mut n) }
                != error::OK
            {
                continue;
            }
            if n == 0 {
                continue;
            }
            let mut c: u32 = 0;
            let mut s: u32 = 0;
            assert_eq!(
                unsafe {
                    crimson_item_part_prefab_resolve_dye_slot_count(
                        ipp_handle, slot_handle, k, &mut c, &mut s,
                    )
                },
                error::OK,
            );
            match s {
                x if x == resolve_dye_slot_count_source::DIRECT => {
                    assert!(
                        (1..=32).contains(&c),
                        "item {k} slot_count {c} out of plausible range [1, 32]",
                    );
                    direct_count += 1;
                }
                x if x == resolve_dye_slot_count_source::NOT_RESOLVED_NO_PARTPREFAB => {
                    not_resolved_no_partprefab += 1;
                }
                x if x == resolve_dye_slot_count_source::NOT_RESOLVED_NO_SLOT_INFO => {
                    not_resolved_no_slot_info += 1;
                }
                other => panic!("unexpected source {other} for item {k}"),
            }
        }
        eprintln!(
            "resolve_dye_slot_count sweep: direct={direct_count}, \
             no_partprefab={not_resolved_no_partprefab}, no_slot_info={not_resolved_no_slot_info}",
        );
        // 1.07 baseline: ~45 items in the 1..1_000_000 key range
        // resolve directly. Allow ≥30 for cross-version drift and to
        // keep the test stable when Pearl Abyss reshuffles item-key
        // ranges. The sibling `c_abi_item_part_prefab_live` test
        // pins the broader `resolved_items >= 50` invariant via
        // `crimson_item_part_prefab_resolved_item_count`, which
        // doesn't rely on a key-range scan.
        assert!(
            direct_count >= 30,
            "expected ≥30 DIRECT resolutions in the 1..1M key range, got {direct_count}",
        );
        // NO_PARTPREFAB shouldn't fire from this loop — the loop only
        // visits items WITH a partprefab mapping (lookup_count != 0).
        // If we hit it here, the wrapper's branch logic is wrong.
        assert_eq!(
            not_resolved_no_partprefab, 0,
            "wrapper emitted NO_PARTPREFAB for items in the join — branch logic bug",
        );

        unsafe {
            crimson_part_prefab_dye_slot_info_free(slot_handle);
            crimson_item_part_prefab_free(ipp_handle);
        }
    }

    #[test]
    fn c_abi_item_part_prefab_resolve_dye_slot_count_null_args() {
        // Every required pointer must trigger NULL_ARG.
        let mut count: u32 = 0;
        let mut source: u32 = 0;
        let rc = unsafe {
            crimson_item_part_prefab_resolve_dye_slot_count(
                ptr::null(), ptr::null(), 9102, &mut count, &mut source,
            )
        };
        assert_eq!(rc, error::NULL_ARG);

        // out_slot_count null
        let rc = unsafe {
            crimson_item_part_prefab_resolve_dye_slot_count(
                ptr::null(), ptr::null(), 9102, ptr::null_mut(), &mut source,
            )
        };
        assert_eq!(rc, error::NULL_ARG);

        // out_resolve_source null
        let rc = unsafe {
            crimson_item_part_prefab_resolve_dye_slot_count(
                ptr::null(), ptr::null(), 9102, &mut count, ptr::null_mut(),
            )
        };
        assert_eq!(rc, error::NULL_ARG);
    }

    /// Constants must be distinct and stable. Layout check on the
    /// `resolve_dye_slot_count_source` module.
    #[test]
    fn resolve_dye_slot_count_source_constants_are_distinct() {
        let vals = [
            resolve_dye_slot_count_source::DIRECT,
            resolve_dye_slot_count_source::NOT_RESOLVED_NO_PARTPREFAB,
            resolve_dye_slot_count_source::NOT_RESOLVED_NO_SLOT_INFO,
        ];
        let mut sorted = vals.to_vec();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), vals.len(), "source constants collide");
        // Pin the numeric values so a future renumbering breaks loud.
        assert_eq!(resolve_dye_slot_count_source::DIRECT, 0);
        assert_eq!(resolve_dye_slot_count_source::NOT_RESOLVED_NO_PARTPREFAB, 1);
        assert_eq!(resolve_dye_slot_count_source::NOT_RESOLVED_NO_SLOT_INFO, 2);
    }

    #[test]
    fn c_abi_item_part_prefab_empty_bytes_yields_empty_handle() {
        // Empty iteminfo + empty stringinfo + empty partprefab must
        // produce a valid (empty) handle without panicking.
        let mut sh: *mut CrimsonItemPartPrefabHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_item_part_prefab_load_from_bytes(
                ptr::null(),
                0,
                ptr::null(),
                0,
                ptr::null(),
                0,
                ptr::null(),
                0,
                &mut sh,
            )
        };
        assert_eq!(rc, error::OK);
        assert!(!sh.is_null());
        let mut c: u32 = 0;
        assert_eq!(
            unsafe { crimson_item_part_prefab_resolved_item_count(sh, &mut c) },
            error::OK
        );
        assert_eq!(c, 0);
        unsafe { crimson_item_part_prefab_free(sh) };
    }

    /// One `_itemDyeDataList` in a decoded save: the owning item's key and
    /// its `ItemDyeSaveData` elements.
    type DyeList<'a> = (u32, &'a [crate::save::ObjectBlock]);

    fn save_scalar(b: &crate::save::ObjectBlock, n: &str) -> Option<i64> {
        use crate::save::{FieldValue, ScalarValue};
        b.fields.iter().find(|f| f.name == n && f.present).and_then(|f| match &f.value {
            FieldValue::Scalar(ScalarValue::U8(v)) => Some(i64::from(*v)),
            FieldValue::Scalar(ScalarValue::I8(v)) => Some(i64::from(*v)),
            FieldValue::Scalar(ScalarValue::U16(v)) => Some(i64::from(*v)),
            FieldValue::Scalar(ScalarValue::U32(v)) => Some(i64::from(*v)),
            FieldValue::Scalar(ScalarValue::Bool(v)) => Some(i64::from(*v)),
            _ => None,
        })
    }

    /// Every `_itemDyeDataList` under `b`, whatever container holds the item.
    fn dye_lists<'a>(b: &'a crate::save::ObjectBlock, out: &mut Vec<DyeList<'a>>) {
        use crate::save::FieldValue;
        for f in &b.fields {
            if f.name == "_itemDyeDataList"
                && f.present
                && let FieldValue::ObjectList { elements, .. } = &f.value
            {
                let key = save_scalar(b, "_itemKey").unwrap_or(-1) as u32;
                out.push((key, elements));
            }
        }
        for f in &b.fields {
            match &f.value {
                FieldValue::ObjectList { elements, .. } => {
                    for e in elements {
                        dye_lists(e, out);
                    }
                }
                FieldValue::Locator { child: Some(c), .. } => dye_lists(c, out),
                _ => {}
            }
        }
    }

    /// The live join handle plus the slot rows by key, or `None` without an
    /// install.
    fn live_join() -> Option<(
        CrimsonItemPartPrefabHandle,
        HashMap<u32, crate::part_prefab_dye_slot_info::PartPrefabDyeSlotInfoEntry>,
    )> {
        let pamt_path = find_pamt()?;
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let x = |n: String| extract_file(pamt.as_c_str(), gamedata_layout::bin_dir(), &n);
        let item_b = x(gamedata_layout::body("iteminfo"));
        let si_b = x(gamedata_layout::body("stringinfo"));
        let pp_b = x(gamedata_layout::body("partprefabdyeslotinfo"));
        let pp_h = x(gamedata_layout::header("partprefabdyeslotinfo"));
        let h = CrimsonItemPartPrefabHandle::from_bytes(&item_b, &si_b, &pp_b, &pp_h).unwrap();
        let rows = crate::part_prefab_dye_slot_info::parse_part_prefab_dye_slot_info_lossy(&pp_b, &pp_h)
            .into_iter()
            .map(|e| (e.key, e))
            .collect();
        Some((h, rows))
    }

    /// The save's `_dyeSlotNo` is a dye slot number of the item's prefab —
    /// the bit index of the 2.01+ 12-byte mask, not a slot index. Items
    /// 121013 and 131013 were dyed in-game on 1.10; each has one prefab with
    /// one slot, yet three and two dye entries, numbered exactly like the
    /// prefab's dye slots on live 2.06. Skips without the private fixture
    /// repo (`crate::test_fixtures`).
    #[test]
    fn dye_slot_no_is_a_prefab_dye_slot_in_the_1_10_fixture() {
        use crate::save::{Body, Save};
        let Some(raw) =
            crate::test_fixtures::save("1.10/broken_save_after_length_change/base_sample/save.save")
        else {
            return;
        };
        let Some((h, rows)) = live_join() else {
            eprintln!("skipping: no game install");
            return;
        };
        let save = Save::parse(&raw).unwrap();
        let body = Body::parse(&save.body).unwrap();
        let blocks = body.decode_blocks(&save.body);
        let mut lists = Vec::new();
        for b in &blocks {
            dye_lists(b, &mut lists);
        }
        let mut resolved = HashMap::new();
        for (item, elements) in lists {
            let Some(keys) = h.part_prefab_keys_by_item.get(&item) else {
                continue;
            };
            // Absent `_dyeSlotNo` is slot 0.
            let used = elements
                .iter()
                .map(|e| save_scalar(e, "_dyeSlotNo").unwrap_or(0))
                .fold(0u32, |a, n| a | (1 << n));
            let offered = keys.iter().filter_map(|k| rows.get(k)).fold(0, |a, r| a | r.dye_slot_bits());
            assert_eq!(used & !offered, 0, "item {item}: _dyeSlotNo outside the prefab's dye slots");
            resolved.insert(item, (keys.clone(), used));
        }
        assert_eq!(resolved.get(&121013), Some(&(vec![0x8dfa1dd5], 0b111)));
        assert_eq!(resolved.get(&131013), Some(&(vec![0x5cfb1da4], 0b11)));
        assert_eq!(rows[&0x8dfa1dd5].slot_count(), 1);
        assert_eq!(rows[&0x8dfa1dd5].dye_slot_bits(), 0b111);
        assert_eq!(rows[&0x5cfb1da4].dye_slot_bits(), 0b11);
    }

    /// Joins every `_itemDyeDataList` element in the live saves to the
    /// item's `partprefabdyeslotinfo` rows, one block per dyed item, so the
    /// save's `_dyeSlotNo` can be compared against the slot masks. Reads
    /// every `save.save` under `%LOCALAPPDATA%\Pearl Abyss\CD\save`
    /// (override: `CRIMSON_DYE_JOIN_ROOT`, e.g. the private fixture `saves` folder).
    #[test]
    #[ignore]
    fn _probe_dye_save_slot_join() {
        use crate::save::{Body, Save};
        let Some((h, rows)) = live_join() else {
            eprintln!("skipping: no game install");
            return;
        };
        let root = std::env::var_os("CRIMSON_DYE_JOIN_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default())
                    .join("Pearl Abyss")
                    .join("CD")
                    .join("save")
            });
        let mut paths = Vec::new();
        let mut stack = vec![root];
        while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.file_name().is_some_and(|n| n == "save.save") {
                    paths.push(p);
                }
            }
        }
        paths.sort();
        for p in paths {
            let Ok(raw) = std::fs::read(&p) else { continue };
            let (Ok(save), true) = (Save::parse(&raw), raw.get(..4) == Some(b"SAVE")) else {
                println!("SAVE {} unreadable", p.display());
                continue;
            };
            let Ok(body) = Body::parse(&save.body) else {
                continue;
            };
            let blocks = body.decode_blocks(&save.body);
            let mut lists = Vec::new();
            for b in &blocks {
                dye_lists(b, &mut lists);
            }
            println!("SAVE {} dyed_items={}", p.display(), lists.len());
            for (key, elements) in lists {
                println!("ITEM {key}");
                for e in elements {
                    let s = |n| save_scalar(e, n);
                    println!(
                        "  DYE no={:?} rgba={:?},{:?},{:?},{:?} grime={:?} grp={:?} pal={:?} nosym={:?}",
                        s("_dyeSlotNo"),
                        s("_dyeColorR"),
                        s("_dyeColorG"),
                        s("_dyeColorB"),
                        s("_dyeColorA"),
                        s("_grimeOpacity"),
                        s("_dyeColorGroupInfoKey").map(|v| format!("{v:x}")),
                        s("_texturePalleteKey"),
                        s("_disableSymbol"),
                    );
                }
                let keys = h.part_prefab_keys_by_item.get(&key).into_iter().flatten();
                let names = h.prefab_names_by_item.get(&key).into_iter().flatten();
                for (pk, name) in keys.zip(names) {
                    let Some(r) = rows.get(pk) else { continue };
                    println!(
                        "  PREFAB {pk:08x} {name} slots={} dye_slot_bits={:012b}",
                        r.slots.len(),
                        r.dye_slot_bits()
                    );
                    for (i, s) in r.slots.iter().enumerate() {
                        let m: String = s.mask.iter().map(|b| char::from(b'0' + b)).collect();
                        println!(
                            "    SLOT {i} mi={:?} mats={:?} mask={m} active={:?} extra={}",
                            s.mat_indices,
                            s.default_materials,
                            s.active_channels(),
                            s.extra_layers.len()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn prefab_pac_paths_reads_length_prefixed_pac_strings() {
        let s = |v: &str| {
            let mut b = (v.len() as u32).to_le_bytes().to_vec();
            b.extend_from_slice(v.as_bytes());
            b
        };
        let mut blob = b"\x01\x02junk".to_vec();
        blob.extend(s("Upperbody"));
        blob.extend(s("character/model/a/b_ub.pac"));
        blob.push(b'y');
        blob.extend(s("character/model/a/b_ub.pac_xml"));
        blob.extend(s("character/model/a/b_lb.pac"));
        assert_eq!(
            prefab_pac_paths(&blob),
            ["character/model/a/b_ub.pac", "character/model/a/b_lb.pac"]
        );
        assert!(prefab_pac_paths(&[]).is_empty());
        assert!(prefab_pac_paths(&[5, 0, 0]).is_empty());
    }

    /// The live 2.06 tables, the character PAMT next to the gamedata group,
    /// and the prefab-join handle built through the C ABI.
    struct LivePrefabJoin {
        item_b: Vec<u8>,
        rows: Vec<crate::part_prefab_dye_slot_info::PartPrefabDyeSlotInfoEntry>,
        h: *mut CrimsonItemPartPrefabHandle,
    }

    impl Drop for LivePrefabJoin {
        fn drop(&mut self) {
            unsafe { crimson_item_part_prefab_free(self.h) };
        }
    }

    fn live_prefab_join() -> Option<LivePrefabJoin> {
        let pamt_path = find_pamt()?;
        let prefab_pamt = pamt_path.parent()?.parent()?.join("0009").join("0.pamt");
        if !prefab_pamt.is_file() {
            return None;
        }
        let pamt = CString::new(pamt_path.to_str().unwrap()).unwrap();
        let x = |n: String| extract_file(pamt.as_c_str(), gamedata_layout::bin_dir(), &n);
        let item_b = x(gamedata_layout::body("iteminfo"));
        let si_b = x(gamedata_layout::body("stringinfo"));
        let pp_b = x(gamedata_layout::body("partprefabdyeslotinfo"));
        let pp_h = x(gamedata_layout::header("partprefabdyeslotinfo"));
        let prefab_c = CString::new(prefab_pamt.to_str().unwrap()).unwrap();
        let mut h: *mut CrimsonItemPartPrefabHandle = ptr::null_mut();
        let rc = unsafe {
            crimson_item_part_prefab_load_from_bytes_with_prefab_pamt(
                item_b.as_ptr(),
                item_b.len(),
                si_b.as_ptr(),
                si_b.len(),
                pp_b.as_ptr(),
                pp_b.len(),
                pp_h.as_ptr(),
                pp_h.len(),
                prefab_c.as_ptr(),
                &mut h,
            )
        };
        assert_eq!(rc, error::OK);
        let rows = crate::part_prefab_dye_slot_info::parse_part_prefab_dye_slot_info_lossy(&pp_b, &pp_h);
        Some(LivePrefabJoin { item_b, rows, h })
    }

    fn keys_of(h: *const CrimsonItemPartPrefabHandle, item: u32) -> Vec<u32> {
        let mut n = 0u32;
        if unsafe { crimson_item_part_prefab_lookup_count(h, item, &mut n) } != error::OK {
            return Vec::new();
        }
        (0..n)
            .map(|i| {
                let mut k = 0u32;
                assert_eq!(unsafe { crimson_item_part_prefab_lookup_key_at(h, item, i, &mut k) }, error::OK);
                k
            })
            .collect()
    }

    fn bits_of(h: *const CrimsonItemPartPrefabHandle, item: u32, tg: Option<&str>) -> (i32, u32) {
        let tg = tg.map(|t| CString::new(t).unwrap());
        let mut bits = u32::MAX;
        let rc = unsafe {
            crimson_item_part_prefab_lookup_dye_slot_bits(
                h,
                item,
                tg.as_ref().map_or(ptr::null(), |t| t.as_ptr()),
                &mut bits,
            )
        };
        (rc, bits)
    }

    fn material_of(h: *const CrimsonItemPartPrefabHandle, item: u32, tg: Option<&str>, n: u8) -> Result<String, i32> {
        let tg = tg.map(|t| CString::new(t).unwrap());
        let tgp = tg.as_ref().map_or(ptr::null(), |t| t.as_ptr());
        let mut req = 0usize;
        let rc = unsafe {
            crimson_item_part_prefab_lookup_dye_slot_material(h, item, tgp, n, ptr::null_mut(), 0, &mut req)
        };
        if rc != error::BUFFER_TOO_SMALL {
            return Err(rc);
        }
        let mut buf = vec![0u8; req];
        let rc = unsafe {
            crimson_item_part_prefab_lookup_dye_slot_material(h, item, tgp, n, buf.as_mut_ptr(), buf.len(), &mut req)
        };
        assert_eq!(rc, error::OK);
        Ok(String::from_utf8(buf[..req - 1].to_vec()).unwrap())
    }

    /// Every `partprefabdyeslotinfo` row is one `.pac` mesh, keyed by the
    /// checksum of its path (the last slot's `tail_name`).
    #[test]
    fn part_prefab_key_is_the_pac_path_checksum_live() {
        let Some(j) = live_prefab_join() else {
            eprintln!("skipping: no game install");
            return;
        };
        assert!(j.rows.len() > 1000);
        for r in &j.rows {
            let pac = &r.slots.last().expect("row has slots").tail_name;
            assert!(pac.ends_with(".pac"), "{:08x}: last tail {pac:?}", r.key);
            assert_eq!(part_prefab_key_for_pac(pac), r.key, "{pac}");
        }
        assert_eq!(
            part_prefab_key_for_pac("character/model/1_pc/1_phm/armor/9_upperbody/cd_phm_00_ub_00_0374.pac"),
            0xe3634591
        );
    }

    /// The prefab join on live 2.06: body-type armor resolves through its
    /// `.prefab`'s meshes, the slot numbers dyed in game on 2.06 (111147 in
    /// save slot102: `_dyeSlotNo` 0, 1, 7) are among them, and the join
    /// covers nearly every dyeable item.
    #[test]
    fn c_abi_item_part_prefab_with_prefab_pamt_live() {
        let Some(j) = live_prefab_join() else {
            eprintln!("skipping: no game install");
            return;
        };
        let h = j.h;

        // 111147 Demeniss Honor Armor: prefab cd_m0001_00_so_phm_ub_31067
        // → ub / lb / lb_belt / sho _31066 meshes.
        assert_eq!(keys_of(h, 111147), [0xf407029b, 0x4170373c, 0x86db33f9, 0xf736011d]);
        let (rc, bits) = bits_of(h, 111147, None);
        assert_eq!(rc, error::OK);
        assert_eq!(bits & 0b1000_0011, 0b1000_0011, "slot102 dyed numbers 0, 1, 7");
        assert_eq!(bits, 0b1111_1111);
        assert_eq!(bits_of(h, 111147, Some("player_1")), (error::OK, 0b1111_1111));
        assert_eq!(bits_of(h, 111147, Some("human_female")).0, error::NOT_FOUND);
        // 9102 Hernand ceremonial guard armor: cd_phm_00_ub_00_0374.
        assert_eq!(keys_of(h, 9102), [0xe3634591, 0xed21cd67, 0x30ffb974, 0x86db33f9, 0xbd4d4886]);
        assert_eq!(bits_of(h, 9102, None), (error::OK, 0b0010_1100_1101));
        // The name join sent this spear to a horse-saddle hook mesh.
        assert_eq!(
            keys_of(h, 1001163),
            [part_prefab_key_for_pac("character/model/1_pc/1_phm/weapon/2_twohandweapon/cd_phm_02_spear_0029.pac")]
        );

        // Variants that differ: 133172's male mesh offers 0..4, the female 0..2.
        assert_eq!(bits_of(h, 133172, None), (error::OK, 0b11111));
        assert_eq!(bits_of(h, 133172, Some("player_1")), (error::OK, 0b11111));
        assert_eq!(bits_of(h, 133172, Some("human_female")), (error::OK, 0b111));
        assert_eq!(material_of(h, 133172, Some("player_1"), 0).as_deref(), Ok("cloth"));
        assert_eq!(material_of(h, 133172, Some("human_female"), 0).as_deref(), Ok("leather"));
        assert_eq!(material_of(h, 133172, None, 0).as_deref(), Ok("cloth"));
        assert_eq!(material_of(h, 111147, None, 11), Err(error::OUT_OF_RANGE));
        assert_eq!(material_of(h, u32::MAX, None, 0), Err(error::NOT_FOUND));
        assert_eq!(bits_of(h, u32::MAX, None), (error::NOT_FOUND, 0));

        // Coverage: is_dyeable items with at least one row (909 / 917 on 2.06).
        let (mut dyeable, mut resolved) = (0u32, 0u32);
        let mut off = 0;
        while off < j.item_b.len() {
            let it = crate::item_info::ItemInfo::read_from(&j.item_b, &mut off).unwrap();
            if it.is_dyeable != 0 {
                dyeable += 1;
                resolved += u32::from(bits_of(h, it.key.0, None).0 == error::OK);
            }
        }
        assert!(dyeable >= 900, "dyeable {dyeable}");
        assert!(resolved * 100 >= dyeable * 98, "resolved {resolved} / {dyeable}");
    }

    #[test]
    fn c_abi_item_part_prefab_with_prefab_pamt_null_args() {
        let mut h: *mut CrimsonItemPartPrefabHandle = ptr::null_mut();
        let p = CString::new("x").unwrap();
        unsafe {
            assert_eq!(
                crimson_item_part_prefab_load_from_bytes_with_prefab_pamt(
                    ptr::null(), 0, ptr::null(), 0, ptr::null(), 0, ptr::null(), 0, ptr::null(), &mut h
                ),
                error::NULL_ARG
            );
            assert_eq!(
                crimson_item_part_prefab_load_from_bytes_with_prefab_pamt(
                    ptr::null(), 1, ptr::null(), 0, ptr::null(), 0, ptr::null(), 0, p.as_ptr(), &mut h
                ),
                error::NULL_ARG
            );
            assert_eq!(
                crimson_item_part_prefab_load_from_bytes_with_prefab_pamt(
                    ptr::null(), 0, ptr::null(), 0, ptr::null(), 0, ptr::null(), 0, p.as_ptr(), &mut h
                ),
                error::IO
            );
            assert!(h.is_null());
            assert_eq!(
                crimson_item_part_prefab_load_from_file_with_prefab_pamt(
                    p.as_ptr(), p.as_ptr(), p.as_ptr(), p.as_ptr(), ptr::null(), &mut h
                ),
                error::NULL_ARG
            );
            assert_eq!(
                crimson_item_part_prefab_load_from_file_with_prefab_pamt(
                    p.as_ptr(), p.as_ptr(), p.as_ptr(), p.as_ptr(), p.as_ptr(), &mut h
                ),
                error::IO
            );
            let mut bits = 0u32;
            let mut req = 0usize;
            assert_eq!(
                crimson_item_part_prefab_lookup_dye_slot_bits(ptr::null(), 1, ptr::null(), &mut bits),
                error::NULL_ARG
            );
            assert_eq!(
                crimson_item_part_prefab_lookup_channel_dye_slot_bits(ptr::null(), 1, ptr::null(), &mut bits),
                error::NULL_ARG
            );
            assert_eq!(
                crimson_item_part_prefab_lookup_dye_slot_material(
                    ptr::null(), 1, ptr::null(), 0, ptr::null_mut(), 0, &mut req
                ),
                error::NULL_ARG
            );
        }
    }

    /// The 1.10 fixtures hold dyed body-type armor the name join could not
    /// resolve (200914, 1000313, 2111103, 103023). Through the prefab join
    /// every `_dyeSlotNo` they carry is one of the item's channel numbers on
    /// live 2.06; three of them are numbers whose 2.06 mask bit is clear.
    /// Skips without the private fixtures or an install.
    #[test]
    fn dye_slot_no_is_an_item_dye_slot_via_prefabs_in_the_1_10_fixture() {
        use crate::save::{Body, Save};
        let Some(raw) =
            crate::test_fixtures::save("1.10/broken_save_after_length_change/base_sample/save.save")
        else {
            return;
        };
        let Some(j) = live_prefab_join() else {
            eprintln!("skipping: no game install");
            return;
        };
        let save = Save::parse(&raw).unwrap();
        let body = Body::parse(&save.body).unwrap();
        let blocks = body.decode_blocks(&save.body);
        let mut lists = Vec::new();
        for b in &blocks {
            dye_lists(b, &mut lists);
        }
        let mut used_by_item = HashMap::new();
        for (item, elements) in lists {
            let used = elements
                .iter()
                .map(|e| save_scalar(e, "_dyeSlotNo").unwrap_or(0))
                .fold(0u32, |a, n| a | (1 << n));
            let (rc, offered) = bits_of(j.h, item, None);
            assert_eq!(rc, error::OK, "item {item} does not resolve");
            let mut channels = 0u32;
            assert_eq!(
                unsafe { crimson_item_part_prefab_lookup_channel_dye_slot_bits(j.h, item, ptr::null(), &mut channels) },
                error::OK
            );
            assert_eq!(offered & !channels, 0, "item {item}: a mask bit with no channel");
            assert_eq!(used & !channels, 0, "item {item}: used {used:b}, channels {channels:b}");
            used_by_item.insert(item, used & !offered);
        }
        // The numbers 1.10 wrote that the 2.06 masks no longer enable.
        let mut outside: Vec<(u32, u32)> = used_by_item.into_iter().collect();
        outside.sort_unstable();
        assert_eq!(
            outside,
            [(103023, 1 << 1), (121013, 0), (131013, 0), (200914, 1 << 3), (1000313, 0), (2111103, 1 << 2)]
        );
    }
}
