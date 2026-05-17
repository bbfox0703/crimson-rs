//! Side-quest faction rollup — C ABI surface.
//!
//! Static curated `(quest_title, faction_name)` table sourced from
//! [`docs/ref-gamedata/side-quest-list.md`](../../docs/ref-gamedata/side-quest-list.md). Side
//! quests in Crimson Desert are organized by **faction** rather than
//! the Chapter / Arc structure used for the main story (see the
//! [sibling `main_quest_chapter` bridge](super::main_quest_chapter)
//! for that one). Each quest title here is a `QuestKey` display title
//! resolved by [`super::quest_info::crimson_questinfo_lookup_display_name`]
//! at `lo32 = 0x100` (e.g. `Quest_Node_Her_GreymaneCamp_Contents → key
//! 1_000_881 → "Record of the Greymanes"`); the faction column is
//! curated and ships as static data.
//!
//! The source MD also has Traditional-Chinese annotations in the
//! section headings — those are informational only and don't appear
//! anywhere in the bridge data.
//!
//! ## Lookup shape
//!
//! - [`crimson_side_quest_faction_for_quest`] — quest title →
//!   faction name. 1:1 (every curated quest has exactly one faction).
//! - [`crimson_side_quest_quest_count_for_faction`] +
//!   [`crimson_side_quest_quest_at_for_faction`] — faction name →
//!   ordered list of quest titles in that faction. Mirrors the
//!   `lookup_related_count` / `_at` pattern from
//!   [`super::faction_relation_group_info`]. Useful for the C# editor's
//!   "show all side quests for faction X" UI.
//! - [`crimson_side_quest_table_entry_count`] +
//!   [`crimson_side_quest_table_get_entry`] — full enumeration.
//!
//! Stateless: backing data is a `const` table; lookup indices are
//! lazily built on first call via `OnceLock`. No load / free pair.

use std::collections::HashMap;
use std::os::raw::{c_char, c_int};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::OnceLock;

use super::error;

/// `(quest_title, faction_name)`.
type Row = (&'static str, &'static str);

const ROWS: &[Row] = &[
    // ── Scattered Embers ──────────────────────────────────────────────
    ("Record of the Greymanes", "Scattered Embers"),
    ("Strongbox with Wheels", "Scattered Embers"),
    ("Brightening the Spirits", "Scattered Embers"),
    ("Chance to Make a Fortune", "Scattered Embers"),
    ("To the Rescue", "Scattered Embers"),
    ("The Greymanes' New Fangs", "Scattered Embers"),
    ("The Nag and the Stubborn One", "Scattered Embers"),
    ("A Chunk of Meat", "Scattered Embers"),
    ("Fang Without a Master", "Scattered Embers"),
    ("Letter at the Shrine", "Scattered Embers"),
    ("Running Loot", "Scattered Embers"),
    ("Empty Wagon", "Scattered Embers"),
    ("Gloomy Gray", "Scattered Embers"),
    ("Vibrant Dye", "Scattered Embers"),
    ("A Fresh Color", "Scattered Embers"),
    ("Shattered Charmed Life", "Scattered Embers"),
    ("A Move on the Table", "Scattered Embers"),
    ("Liquor and Memories", "Scattered Embers"),
    ("Trembling Hands", "Scattered Embers"),
    ("White Wood Bow", "Scattered Embers"),
    ("The New Archers", "Scattered Embers"),
    ("Face on the Bounty Notice", "Scattered Embers"),
    ("Plenty of Bounty", "Scattered Embers"),
    ("The Cost of the Tab", "Scattered Embers"),
    ("Logging Without an Axe", "Scattered Embers"),
    ("Quarrel on Horseback", "Scattered Embers"),
    ("Showdown in the Saddles", "Scattered Embers"),
    ("Scent of Gold", "Scattered Embers"),
    // ── Grounds of the Sunrise ────────────────────────────────────────
    ("Embers of Return", "Grounds of the Sunrise"),
    ("Reuniting with Comrades", "Grounds of the Sunrise"),
    ("For a Better Tomorrow", "Grounds of the Sunrise"),
    // ── Greymane Commissions ──────────────────────────────────────────
    ("Carl's Request", "Greymane Commissions"),
    ("Ronnie's Request", "Greymane Commissions"),
    ("Ross's Request", "Greymane Commissions"),
    ("Tranan's Request", "Greymane Commissions"),
    ("Brice's Request", "Greymane Commissions"),
    ("Ronald's Request", "Greymane Commissions"),
    ("Pierce's Request", "Greymane Commissions"),
    // ── House Celeste ─────────────────────────────────────────────────
    ("Bounty Target: Jeffrey", "House Celeste"),
    ("Bounty Target: Bianca", "House Celeste"),
    ("Bounty Target: Simon de Montfort", "House Celeste"),
    ("Bounty Target: Alessio", "House Celeste"),
    // ── House Roberts ─────────────────────────────────────────────────
    ("Estate in Dismay", "House Roberts"),
    ("Continuing Concern", "House Roberts"),
    ("Boulder from the Sky", "House Roberts"),
    // ── Hernand Commissions ───────────────────────────────────────────
    ("Serge's Request", "Hernand Commissions"),
    ("Breaking in the Grindstone", "Hernand Commissions"),
    ("Lunchbox of Love", "Hernand Commissions"),
    ("The Weight of Knowledge", "Hernand Commissions"),
    ("Rhett's Request", "Hernand Commissions"),
    ("Renee's Request", "Hernand Commissions"),
    ("Turnali's Request", "Hernand Commissions"),
    ("Prox's Request", "Hernand Commissions"),
    ("Tina's Request", "Hernand Commissions"),
    ("Bruna's Request", "Hernand Commissions"),
    ("Ugmon's Request", "Hernand Commissions"),
    // ── Hernand Requests ──────────────────────────────────────────────
    ("Goddess of Abundance", "Hernand Requests"),
    ("Path that Connects to House of Healing", "Hernand Requests"),
    ("Wolf Protecting Hernand", "Hernand Requests"),
    ("A Favor for Hernand", "Hernand Requests"),
    ("Bells Ringing Again", "Hernand Requests"),
    // ── Other factions (one or two quests each) ───────────────────────
    ("The Trembling Woods", "Pororin Forest Guardians"),
    ("House of Spears", "House Alfonso"),
    ("Lord Amidst the Ruins", "House Serkis"),
    ("Deathchime", "House Wells"),
    ("Mushrooms Growing Among Poisons", "Demeniss Commissions"),
    ("Crossroads of Succession", "Pailune Militia"),
    ("Antumbra's Sword", "Antumbra Order"),
    ("The Witch of Wisdom", "Antumbra Order"),
    ("Veil of the Yard", "Giant's Yard"),
    // Source MD spells it "Encirlement" — preserve as-is so this matches
    // whatever the QuestKey display title actually resolves to. If the
    // PALOC strings use the standard "Encirclement" spelling, the bridge
    // will need a one-row fix-up; flag during the live-cross-check pass.
    ("Encirlement on the Cliff", "Giant's Yard"),
    ("Dangerous Saltroad", "Goldenscales on the Saltroad"),
    (
        "Siege of the Abandoned Castle Ruins",
        "Hunters of the Abandoned Castle Ruins",
    ),
    (
        "Veil of the Abandoned Castle Ruins",
        "Hunters of the Abandoned Castle Ruins",
    ),
    (
        "The Fangs that Devoured the Village",
        "The Fangs Beneath the Rock",
    ),
    (
        "The Gorge Under Siege",
        "Those Who Constrict the Research Expedition",
    ),
    (
        "Rainforest Gorge",
        "Those Who Constrict the Research Expedition",
    ),
    ("The Missing Desert Melons", "Harvest of Greed"),
    (
        "A Village of Growing Suspicion",
        "Tales of the Crimson Desert Merchants",
    ),
    (
        "Thomas's Request",
        "Tales of the Crimson Desert Merchants",
    ),
    (
        "Between Drinks and Cheers",
        "Tales of the Crimson Desert Residents",
    ),
    (
        "Friend's Whereabouts",
        "Tales of the Crimson Desert Residents",
    ),
    (
        "Dirty Marauders",
        "Tales from the Corners of Crimson Desert",
    ),
    (
        "Futile Goodwill",
        "Tales from the Corners of Crimson Desert",
    ),
];

/// Lookup index: quest title → row index in [`ROWS`]. The curated
/// table has no duplicate quest titles (asserted by
/// `curated_table_integrity`), so this is 1:1.
fn quest_index() -> &'static HashMap<&'static str, usize> {
    static IDX: OnceLock<HashMap<&'static str, usize>> = OnceLock::new();
    IDX.get_or_init(|| {
        let mut m: HashMap<&'static str, usize> = HashMap::with_capacity(ROWS.len());
        for (i, row) in ROWS.iter().enumerate() {
            // `or_insert` here is defensive — the integrity test
            // catches duplicates at build time. If a future revision
            // accidentally introduces one, lookup falls back to the
            // first declaration in MD order.
            m.entry(row.0).or_insert(i);
        }
        m
    })
}

/// Lookup index: faction name → ordered list of row indices for that
/// faction. Order matches declaration order in [`ROWS`] (which mirrors
/// the section order in the source MD).
fn faction_quests_index() -> &'static HashMap<&'static str, Vec<usize>> {
    static IDX: OnceLock<HashMap<&'static str, Vec<usize>>> = OnceLock::new();
    IDX.get_or_init(|| {
        let mut m: HashMap<&'static str, Vec<usize>> = HashMap::new();
        for (i, row) in ROWS.iter().enumerate() {
            m.entry(row.1).or_default().push(i);
        }
        m
    })
}

// ── Enumeration ────────────────────────────────────────────────────────────

/// Total number of `(quest, faction)` rows in the curated table.
///
/// # Safety
/// `out_count` must be non-null and writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_side_quest_table_entry_count(out_count: *mut u32) -> c_int {
    if out_count.is_null() {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        unsafe { *out_count = ROWS.len() as u32 };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Read the row at `idx`. Each of the two string outputs uses the
/// standard two-call sizing pattern.
///
/// # Safety
/// `quest_required` and `faction_required` must be non-null. Each
/// `*_buf` may be null iff its `*_buf_len == 0`.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn crimson_side_quest_table_get_entry(
    idx: u32,
    quest_buf: *mut u8,
    quest_buf_len: usize,
    quest_required: *mut usize,
    faction_buf: *mut u8,
    faction_buf_len: usize,
    faction_required: *mut usize,
) -> c_int {
    if quest_required.is_null() || faction_required.is_null() {
        return error::NULL_ARG;
    }
    if (quest_buf.is_null() && quest_buf_len != 0)
        || (faction_buf.is_null() && faction_buf_len != 0)
    {
        return error::NULL_ARG;
    }
    unsafe {
        *quest_required = 0;
        *faction_required = 0;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let Some(row) = ROWS.get(idx as usize) else {
            return error::OUT_OF_RANGE;
        };
        let rc_q = write_str_to_buf(row.0, quest_buf, quest_buf_len, quest_required);
        let rc_f = write_str_to_buf(row.1, faction_buf, faction_buf_len, faction_required);
        if rc_q == error::BUFFER_TOO_SMALL || rc_f == error::BUFFER_TOO_SMALL {
            return error::BUFFER_TOO_SMALL;
        }
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

// ── Lookups ────────────────────────────────────────────────────────────────

/// Resolve a side-quest display title (the `quest:` value in the
/// source MD — e.g. "Record of the Greymanes", "Carl's Request") to
/// its faction name.
///
/// `quest_title` must be NUL-terminated UTF-8. Returns
/// [`error::NOT_FOUND`] if the quest isn't in the curated set;
/// otherwise fills `buf` per the standard two-call pattern.
///
/// # Safety
/// `quest_title` must point to a valid NUL-terminated UTF-8 C string.
/// `required` must be non-null. `buf` may be null iff `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_side_quest_faction_for_quest(
    quest_title: *const c_char,
    buf: *mut u8,
    buf_len: usize,
    required: *mut usize,
) -> c_int {
    if required.is_null() || quest_title.is_null() {
        return error::NULL_ARG;
    }
    if buf.is_null() && buf_len != 0 {
        return error::NULL_ARG;
    }
    unsafe { *required = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let key = match unsafe { std::ffi::CStr::from_ptr(quest_title) }.to_str() {
            Ok(s) => s,
            Err(_) => return error::INVALID_PATH,
        };
        let Some(&i) = quest_index().get(key) else {
            return error::NOT_FOUND;
        };
        write_str_to_buf(ROWS[i].1, buf, buf_len, required)
    }))
    .unwrap_or(error::PANIC)
}

/// Number of side quests in the curated set that belong to
/// `faction_name`. Returns [`error::NOT_FOUND`] when no curated quest
/// references that faction.
///
/// # Safety
/// `faction_name` must point to a valid NUL-terminated UTF-8 C string.
/// `out_count` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_side_quest_quest_count_for_faction(
    faction_name: *const c_char,
    out_count: *mut u32,
) -> c_int {
    if faction_name.is_null() || out_count.is_null() {
        return error::NULL_ARG;
    }
    unsafe { *out_count = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let key = match unsafe { std::ffi::CStr::from_ptr(faction_name) }.to_str() {
            Ok(s) => s,
            Err(_) => return error::INVALID_PATH,
        };
        let Some(rows) = faction_quests_index().get(key) else {
            return error::NOT_FOUND;
        };
        unsafe { *out_count = rows.len() as u32 };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// The `idx`-th side quest title in `faction_name`'s curated list.
/// Order matches declaration order in the source MD (and the bridge's
/// internal `ROWS` table — see the file head).
///
/// Returns [`error::NOT_FOUND`] when the faction isn't in the curated
/// set, [`error::OUT_OF_RANGE`] when the faction exists but `idx` is
/// past its quest count, or [`error::BUFFER_TOO_SMALL`] under the
/// standard two-call sizing pattern.
///
/// # Safety
/// `faction_name` must point to a valid NUL-terminated UTF-8 C string.
/// `required` must be non-null. `buf` may be null iff `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_side_quest_quest_at_for_faction(
    faction_name: *const c_char,
    idx: u32,
    buf: *mut u8,
    buf_len: usize,
    required: *mut usize,
) -> c_int {
    if required.is_null() || faction_name.is_null() {
        return error::NULL_ARG;
    }
    if buf.is_null() && buf_len != 0 {
        return error::NULL_ARG;
    }
    unsafe { *required = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let key = match unsafe { std::ffi::CStr::from_ptr(faction_name) }.to_str() {
            Ok(s) => s,
            Err(_) => return error::INVALID_PATH,
        };
        let Some(rows) = faction_quests_index().get(key) else {
            return error::NOT_FOUND;
        };
        let Some(&row_idx) = rows.get(idx as usize) else {
            return error::OUT_OF_RANGE;
        };
        write_str_to_buf(ROWS[row_idx].0, buf, buf_len, required)
    }))
    .unwrap_or(error::PANIC)
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn write_str_to_buf(src: &str, buf: *mut u8, buf_len: usize, required: *mut usize) -> c_int {
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

#[cfg(test)]
mod tests {
    //! Tests:
    //!
    //! 1. Curated-table integrity — no duplicate quest titles, no row
    //!    has an empty quest or faction.
    //! 2. Forward `quest → faction` lookup — known mappings.
    //! 3. Reverse `faction → [quests]` enumeration — known faction
    //!    sizes + sample first / last entries.
    //! 4. ABI hygiene — NULL args, OUT_OF_RANGE, NOT_FOUND, buffer
    //!    sizing.
    //!
    //! Pure-Rust tests; no live-install dependency.
    use super::*;
    use std::collections::HashSet;
    use std::ffi::CString;
    use std::ptr;

    fn fill(rc_first: i32, required: usize, run: impl FnOnce(*mut u8, usize, *mut usize) -> i32) -> String {
        assert_eq!(rc_first, error::BUFFER_TOO_SMALL);
        let mut out = vec![0u8; required];
        let mut req2: usize = 0;
        let rc = run(out.as_mut_ptr(), out.len(), &mut req2);
        assert_eq!(rc, error::OK);
        std::str::from_utf8(&out[..req2 - 1]).unwrap().to_owned()
    }

    fn call_faction_for_quest(quest: &str) -> Result<String, i32> {
        let c = CString::new(quest).unwrap();
        let mut req: usize = 0;
        let rc = unsafe {
            crimson_side_quest_faction_for_quest(c.as_ptr(), ptr::null_mut(), 0, &mut req)
        };
        if rc == error::NOT_FOUND {
            return Err(rc);
        }
        Ok(fill(rc, req, |b, n, r| unsafe {
            crimson_side_quest_faction_for_quest(c.as_ptr(), b, n, r)
        }))
    }

    fn faction_quest_titles(faction: &str) -> Vec<String> {
        let c = CString::new(faction).unwrap();
        let mut count: u32 = 0;
        let rc = unsafe {
            crimson_side_quest_quest_count_for_faction(c.as_ptr(), &mut count)
        };
        assert_eq!(rc, error::OK, "faction not found: {faction:?}");
        (0..count)
            .map(|i| {
                let mut req: usize = 0;
                let rc = unsafe {
                    crimson_side_quest_quest_at_for_faction(
                        c.as_ptr(),
                        i,
                        ptr::null_mut(),
                        0,
                        &mut req,
                    )
                };
                fill(rc, req, |b, n, r| unsafe {
                    crimson_side_quest_quest_at_for_faction(c.as_ptr(), i, b, n, r)
                })
            })
            .collect()
    }

    #[test]
    fn curated_table_integrity() {
        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_side_quest_table_entry_count(&mut count) },
            error::OK
        );
        assert_eq!(count as usize, ROWS.len());
        assert!(count > 50, "expected >50 curated side quests, got {count}");

        // No duplicate quest titles (the source MD is a flat list).
        let mut seen: HashSet<&str> = HashSet::with_capacity(ROWS.len());
        for (i, row) in ROWS.iter().enumerate() {
            assert!(!row.0.is_empty(), "row {i}: empty quest title");
            assert!(!row.1.is_empty(), "row {i}: empty faction name");
            assert!(
                seen.insert(row.0),
                "row {i}: duplicate quest title {:?} — index expects 1:1",
                row.0
            );
        }
    }

    #[test]
    fn faction_for_quest_known_cases() {
        // First entry in source MD
        assert_eq!(
            call_faction_for_quest("Record of the Greymanes").unwrap(),
            "Scattered Embers"
        );
        // Apostrophe handling — Carl's Request
        assert_eq!(
            call_faction_for_quest("Carl's Request").unwrap(),
            "Greymane Commissions"
        );
        // Multi-word faction
        assert_eq!(
            call_faction_for_quest("Bounty Target: Simon de Montfort").unwrap(),
            "House Celeste"
        );
        // Singleton faction (one quest in the curated set)
        assert_eq!(
            call_faction_for_quest("The Trembling Woods").unwrap(),
            "Pororin Forest Guardians"
        );
        // Two-quest faction with apostrophe in faction name
        assert_eq!(
            call_faction_for_quest("Antumbra's Sword").unwrap(),
            "Antumbra Order"
        );
        assert_eq!(
            call_faction_for_quest("Veil of the Yard").unwrap(),
            "Giant's Yard"
        );
        // Last entry in source MD
        assert_eq!(
            call_faction_for_quest("Futile Goodwill").unwrap(),
            "Tales from the Corners of Crimson Desert"
        );
        // Unknown quest
        assert_eq!(
            call_faction_for_quest("No Such Quest"),
            Err(error::NOT_FOUND)
        );
    }

    #[test]
    fn faction_to_quests_known_cases() {
        // Biggest faction — Scattered Embers (28 quests)
        let scattered = faction_quest_titles("Scattered Embers");
        assert_eq!(scattered.len(), 28);
        assert_eq!(scattered.first().map(String::as_str), Some("Record of the Greymanes"));
        assert_eq!(scattered.last().map(String::as_str), Some("Scent of Gold"));

        // Grounds of the Sunrise — 3 quests
        let sunrise = faction_quest_titles("Grounds of the Sunrise");
        assert_eq!(sunrise, vec![
            "Embers of Return".to_string(),
            "Reuniting with Comrades".to_string(),
            "For a Better Tomorrow".to_string(),
        ]);

        // Hunters of the Abandoned Castle Ruins — 2 quests (one of the
        // multi-quest "Other Factions" cases)
        let hunters = faction_quest_titles("Hunters of the Abandoned Castle Ruins");
        assert_eq!(hunters.len(), 2);

        // Singleton faction
        let pororin = faction_quest_titles("Pororin Forest Guardians");
        assert_eq!(pororin, vec!["The Trembling Woods".to_string()]);
    }

    #[test]
    fn faction_lookups_unknown_and_oor() {
        // Unknown faction → NOT_FOUND for both count and _at
        let bogus = CString::new("Not A Faction").unwrap();
        let mut count: u32 = 99;
        assert_eq!(
            unsafe {
                crimson_side_quest_quest_count_for_faction(bogus.as_ptr(), &mut count)
            },
            error::NOT_FOUND
        );
        assert_eq!(count, 0, "out_count should reset to 0 on NOT_FOUND");

        let mut req: usize = 0;
        assert_eq!(
            unsafe {
                crimson_side_quest_quest_at_for_faction(
                    bogus.as_ptr(),
                    0,
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            },
            error::NOT_FOUND
        );

        // Known faction, out-of-range idx → OUT_OF_RANGE (separate
        // from NOT_FOUND so the caller can distinguish "faction
        // doesn't exist" from "faction exists but idx too big").
        let sunrise = CString::new("Grounds of the Sunrise").unwrap();
        assert_eq!(
            unsafe {
                crimson_side_quest_quest_at_for_faction(
                    sunrise.as_ptr(),
                    99,
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            },
            error::OUT_OF_RANGE
        );
    }

    #[test]
    fn enumeration_round_trip() {
        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_side_quest_table_entry_count(&mut count) },
            error::OK
        );

        for idx in 0..count {
            let (mut q_req, mut f_req) = (0usize, 0usize);
            let rc = unsafe {
                crimson_side_quest_table_get_entry(
                    idx,
                    ptr::null_mut(),
                    0,
                    &mut q_req,
                    ptr::null_mut(),
                    0,
                    &mut f_req,
                )
            };
            assert_eq!(rc, error::BUFFER_TOO_SMALL);
            assert!(q_req >= 1);
            assert!(f_req >= 1);

            let mut q_buf = vec![0u8; q_req];
            let mut f_buf = vec![0u8; f_req];
            let rc = unsafe {
                crimson_side_quest_table_get_entry(
                    idx,
                    q_buf.as_mut_ptr(),
                    q_buf.len(),
                    &mut q_req,
                    f_buf.as_mut_ptr(),
                    f_buf.len(),
                    &mut f_req,
                )
            };
            assert_eq!(rc, error::OK);
            let q = std::str::from_utf8(&q_buf[..q_req - 1]).unwrap();
            let f = std::str::from_utf8(&f_buf[..f_req - 1]).unwrap();
            let row = &ROWS[idx as usize];
            assert_eq!(q, row.0);
            assert_eq!(f, row.1);
        }

        // OOR guard
        let (mut a, mut b) = (0usize, 0usize);
        let rc = unsafe {
            crimson_side_quest_table_get_entry(
                count,
                ptr::null_mut(),
                0,
                &mut a,
                ptr::null_mut(),
                0,
                &mut b,
            )
        };
        assert_eq!(rc, error::OUT_OF_RANGE);
    }

    #[test]
    fn null_args() {
        assert_eq!(
            unsafe { crimson_side_quest_table_entry_count(ptr::null_mut()) },
            error::NULL_ARG
        );
        let mut req: usize = 0;
        assert_eq!(
            unsafe {
                crimson_side_quest_faction_for_quest(
                    ptr::null(),
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            },
            error::NULL_ARG
        );
        let mut count: u32 = 0;
        assert_eq!(
            unsafe {
                crimson_side_quest_quest_count_for_faction(ptr::null(), &mut count)
            },
            error::NULL_ARG
        );
        let key = CString::new("Scattered Embers").unwrap();
        assert_eq!(
            unsafe {
                crimson_side_quest_quest_count_for_faction(key.as_ptr(), ptr::null_mut())
            },
            error::NULL_ARG
        );
        assert_eq!(
            unsafe {
                crimson_side_quest_quest_at_for_faction(
                    ptr::null(),
                    0,
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            },
            error::NULL_ARG
        );
        assert_eq!(
            unsafe {
                crimson_side_quest_quest_at_for_faction(
                    key.as_ptr(),
                    0,
                    ptr::null_mut(),
                    0,
                    ptr::null_mut(),
                )
            },
            error::NULL_ARG
        );
    }

    #[test]
    fn buffer_too_small_paths() {
        let key = CString::new("Record of the Greymanes").unwrap();
        let mut tiny = [0u8; 4];
        let mut req: usize = 0;
        let rc = unsafe {
            crimson_side_quest_faction_for_quest(
                key.as_ptr(),
                tiny.as_mut_ptr(),
                tiny.len(),
                &mut req,
            )
        };
        assert_eq!(rc, error::BUFFER_TOO_SMALL);
        assert_eq!(req, "Scattered Embers".len() + 1);
    }
}
