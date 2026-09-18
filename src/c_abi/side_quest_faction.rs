//! Side-quest faction rollup — C ABI surface.
//!
//! Static curated `(quest_title, faction_name)` table sourced from
//! [`docs/ref-gamedata/side-quest-list.md`](../../docs/ref-gamedata/side-quest-list.md). Side
//! quests in Crimson Desert are organized by **faction** rather than
//! the Chapter / Arc structure used for the main story (see the
//! [sibling `main_quest_chapter` bridge](super::main_quest_chapter)
//! for that one). The list follows the in-game journal, so most entries
//! are **missions** — `MissionKey` display titles at PALOC `lo32 = 0x101`
//! (e.g. `Mission_GreymaneCamp_Carl → 1_001_073 → "Carl's Request"`) —
//! and 20 are quests, `QuestKey` titles at `lo32 = 0x100` (e.g.
//! `Quest_Node_Her_GreymaneCamp_Contents → 1_000_881 → "Record of the
//! Greymanes"`). Every row records that key, so
//! [`crimson_side_quest_faction_for_mission_key`] /
//! [`crimson_side_quest_faction_for_quest_key`] answer from what a save
//! stores and survive a retitle; the faction column is curated and ships
//! as static data. Titles were reconciled against 2.02 (9 had drifted —
//! see the source MD) and follow the live strings since (2.03 retitled
//! one); `curated_titles_match_live_install` fails the next time a key's
//! live title stops matching.
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
//! - [`crimson_side_quest_faction_for_mission_key`] /
//!   [`crimson_side_quest_faction_for_quest_key`] — game key → faction
//!   name. The two key spaces overlap numerically, hence two functions.
//! - [`crimson_side_quest_table_entry_count`] +
//!   [`crimson_side_quest_table_get_entry`] (strings) /
//!   [`crimson_side_quest_table_get_entry_key`] (key) — full enumeration.
//!
//! Stateless: backing data is a `const` table; lookup indices are
//! lazily built on first call via `OnceLock`. No load / free pair.

use std::collections::HashMap;
use std::os::raw::{c_char, c_int};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::OnceLock;

use super::error;
use super::main_quest_chapter::Entry::{self, Mission, Quest};

/// `(quest_title, faction_name, entry)`. `entry` is the game row the title
/// comes from: a [`Quest`] (PALOC `lo32 = 0x100`) or — for most of the
/// table, which the in-game journal lists by mission — a [`Mission`]
/// (`lo32 = 0x101`). Where a title is both, the row carries the quest.
type Row = (&'static str, &'static str, Entry);

/// Titles are the live English display strings (reconciled against Crimson
/// Desert 2.02, updated for 2.03's one retitle — see the source MD);
/// `curated_titles_match_live_install` fails as soon as a key's live title
/// stops matching its row.
const ROWS: &[Row] = &[
    // ── Scattered Embers ──────────────────────────────────────────────
    ("Record of the Greymanes", "Scattered Embers", Quest(1_000_881)),
    ("Strongbox with Wheels", "Scattered Embers", Quest(1_000_157)),
    ("Brightening the Spirits", "Scattered Embers", Quest(1_000_350)),
    ("Chance to Make a Fortune", "Scattered Embers", Quest(1_000_404)),
    // was "To the Rescue"
    ("Rescuing the Pailunese Refugees", "Scattered Embers", Mission(1_001_412)),
    ("The Greymanes' New Fangs", "Scattered Embers", Quest(1_000_330)),
    ("The Nag and the Stubborn One", "Scattered Embers", Mission(1_001_205)),
    ("A Chunk of Meat", "Scattered Embers", Mission(1_001_210)),
    ("Fang Without a Master", "Scattered Embers", Mission(1_001_217)),
    ("Letter at the Shrine", "Scattered Embers", Mission(1_001_409)),
    ("Running Loot", "Scattered Embers", Mission(1_000_422)),
    ("Empty Wagon", "Scattered Embers", Mission(1_000_516)),
    ("Gloomy Gray", "Scattered Embers", Mission(1_000_987)),
    ("Vibrant Dye", "Scattered Embers", Mission(1_000_997)),
    ("A Fresh Color", "Scattered Embers", Mission(1_000_998)),
    ("Shattered Charmed Life", "Scattered Embers", Mission(1_000_999)),
    ("A Move on the Table", "Scattered Embers", Mission(1_001_000)),
    ("Liquor and Memories", "Scattered Embers", Mission(1_001_001)),
    ("Trembling Hands", "Scattered Embers", Mission(1_001_002)),
    ("White Wood Bow", "Scattered Embers", Mission(1_001_003)),
    ("The New Archers", "Scattered Embers", Mission(1_001_004)),
    ("Face on the Bounty Notice", "Scattered Embers", Mission(1_001_211)),
    ("Plenty of Bounty", "Scattered Embers", Mission(1_001_218)),
    ("The Cost of the Tab", "Scattered Embers", Mission(1_001_216)),
    ("Logging Without an Axe", "Scattered Embers", Mission(1_001_220)),
    ("Quarrel on Horseback", "Scattered Embers", Mission(1_000_146)),
    ("Showdown in the Saddles", "Scattered Embers", Mission(1_001_403)),
    ("Scent of Gold", "Scattered Embers", Mission(1_001_005)),
    // ── Grounds of the Sunrise ────────────────────────────────────────
    ("Embers of Return", "Grounds of the Sunrise", Quest(1_000_304)),
    ("Reuniting with Comrades", "Grounds of the Sunrise", Quest(1_000_195)),
    ("For a Better Tomorrow", "Grounds of the Sunrise", Quest(1_000_937)),
    // ── Greymane Commissions ──────────────────────────────────────────
    ("Carl's Request", "Greymane Commissions", Mission(1_001_073)),
    ("Ronnie's Request", "Greymane Commissions", Mission(1_001_076)),
    ("Ross's Request", "Greymane Commissions", Mission(1_001_080)),
    ("Tranan's Request", "Greymane Commissions", Mission(1_001_081)),
    ("Brice's Request", "Greymane Commissions", Mission(1_001_082)),
    ("Ronald's Request", "Greymane Commissions", Mission(1_001_112)),
    ("Pierce's Request", "Greymane Commissions", Mission(1_001_123)),
    // ── House Celeste ─────────────────────────────────────────────────
    // was "Bounty Target: Jeffrey"
    ("Bounty Notice - Jeffrey", "House Celeste", Mission(1_000_833)),
    // was "Bounty Target: Bianca"
    ("Bounty Notice - Bianca", "House Celeste", Mission(1_000_349)),
    // was "Bounty Target: Simon de Montfort"
    ("Bounty Notice - Simon de Montfort", "House Celeste", Mission(1_000_344)),
    // was "Bounty Target: Alessio"
    ("Bounty Notice - Alessio", "House Celeste", Mission(1_000_347)),
    // ── House Roberts ─────────────────────────────────────────────────
    ("Estate in Dismay", "House Roberts", Quest(1_000_016)),
    ("Continuing Concern", "House Roberts", Quest(1_000_397)),
    ("Boulder from the Sky", "House Roberts", Quest(1_000_630)),
    // ── Hernand Commissions ───────────────────────────────────────────
    ("Serge's Request", "Hernand Commissions", Mission(1_000_688)),
    // 2.03 retitle (was "Breaking in the Grindstone")
    ("Break in the grindstone", "Hernand Commissions", Mission(1_000_015)),
    ("Lunchbox of Love", "Hernand Commissions", Mission(1_000_231)),
    ("The Weight of Knowledge", "Hernand Commissions", Quest(1_000_290)),
    ("Rhett's Request", "Hernand Commissions", Mission(1_000_578)),
    ("Renee's Request", "Hernand Commissions", Mission(1_000_149)),
    ("Turnali's Request", "Hernand Commissions", Mission(1_000_663)),
    ("Prox's Request", "Hernand Commissions", Mission(1_000_745)),
    ("Tina's Request", "Hernand Commissions", Mission(1_000_585)),
    ("Bruna's Request", "Hernand Commissions", Mission(1_000_669)),
    ("Ugmon's Request", "Hernand Commissions", Mission(1_000_241)),
    // ── Hernand Requests ──────────────────────────────────────────────
    ("Goddess of Abundance", "Hernand Requests", Mission(1_000_569)),
    ("Path that Connects to House of Healing", "Hernand Requests", Mission(1_000_570)),
    ("Wolf Protecting Hernand", "Hernand Requests", Mission(1_000_571)),
    ("A Favor for Hernand", "Hernand Requests", Quest(1_000_163)),
    ("Bells Ringing Again", "Hernand Requests", Mission(1_000_568)),
    // ── Other factions (one or two quests each) ───────────────────────
    // was "The Trembling Woods"
    ("Trembling Woods", "Pororin Forest Guardians", Quest(1_000_159)),
    ("House of Spears", "House Alfonso", Quest(1_000_874)),
    ("Lord Amidst the Ruins", "House Serkis", Quest(1_000_894)),
    ("Deathchime", "House Wells", Quest(1_000_278)),
    // was "Mushrooms Growing Among Poisons"
    ("Mushrooms Growing Among Poison", "Demeniss Commissions", Quest(1_000_615)),
    ("Crossroads of Succession", "Pailune Militia", Quest(1_000_282)),
    ("Antumbra's Sword", "Antumbra Order", Mission(1_000_757)),
    ("The Witch of Wisdom", "Antumbra Order", Quest(1_000_335)),
    ("Veil of the Yard", "Giant's Yard", Mission(1_002_129)),
    // Source MD spells it "Encirlement" — preserve as-is so this matches
    // whatever the QuestKey display title actually resolves to. If the
    // PALOC strings use the standard "Encirclement" spelling, the bridge
    // will need a one-row fix-up; flag during the live-cross-check pass.
    // was "Encirlement on the Cliff"
    ("Encirclement on the Cliff", "Giant's Yard", Mission(1_002_130)),
    ("Dangerous Saltroad", "Goldenscales on the Saltroad", Mission(1_000_118)),
    (
        "Siege of the Abandoned Castle Ruins",
        "Hunters of the Abandoned Castle Ruins",
        Mission(1_002_131),
    ),
    (
        "Veil of the Abandoned Castle Ruins",
        "Hunters of the Abandoned Castle Ruins",
        Mission(1_001_423),
    ),
    // was "The Fangs that Devoured the Village"
    ("The Fangs That Devoured the Village", "The Fangs Beneath the Rock", Mission(1_000_130)),
    ("The Gorge Under Siege", "Those Who Constrict the Research Expedition", Mission(1_002_133)),
    ("Rainforest Gorge", "Those Who Constrict the Research Expedition", Mission(1_002_132)),
    ("The Missing Desert Melons", "Harvest of Greed", Mission(1_000_617)),
    ("A Village of Growing Suspicion", "Tales of the Crimson Desert Merchants", Mission(1_001_465)),
    ("Thomas's Request", "Tales of the Crimson Desert Merchants", Mission(1_001_079)),
    ("Between Drinks and Cheers", "Tales of the Crimson Desert Residents", Mission(1_001_100)),
    ("Friend's Whereabouts", "Tales of the Crimson Desert Residents", Mission(1_001_113)),
    ("Dirty Marauders", "Tales from the Corners of Crimson Desert", Mission(1_001_646)),
    ("Futile Goodwill", "Tales from the Corners of Crimson Desert", Mission(1_001_679)),
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

/// Lookup index: `MissionKey` → row index. Keys are unique per kind
/// (asserted by `entry_keys_are_unique`).
fn mission_key_index() -> &'static HashMap<u32, usize> {
    static IDX: OnceLock<HashMap<u32, usize>> = OnceLock::new();
    IDX.get_or_init(|| {
        ROWS.iter()
            .enumerate()
            .filter_map(|(i, row)| match row.2 {
                Mission(k) => Some((k, i)),
                _ => None,
            })
            .collect()
    })
}

/// Lookup index: `QuestKey` → row index.
fn quest_key_index() -> &'static HashMap<u32, usize> {
    static IDX: OnceLock<HashMap<u32, usize>> = OnceLock::new();
    IDX.get_or_init(|| {
        ROWS.iter()
            .enumerate()
            .filter_map(|(i, row)| match row.2 {
                Quest(k) => Some((k, i)),
                _ => None,
            })
            .collect()
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

/// Resolve a `MissionKey` — the save-side key — to its faction name,
/// independent of the display title, so it survives a retitle. Returns
/// [`error::NOT_FOUND`] when no curated row is that mission.
///
/// # Safety
/// `required` must be non-null. `buf` may be null iff `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_side_quest_faction_for_mission_key(
    mission_key: u32,
    buf: *mut u8,
    buf_len: usize,
    required: *mut usize,
) -> c_int {
    faction_for_key(buf, buf_len, required, || {
        mission_key_index().get(&mission_key).copied()
    })
}

/// Resolve a `QuestKey` to its faction name. Same contract as
/// [`crimson_side_quest_faction_for_mission_key`].
///
/// # Safety
/// `required` must be non-null. `buf` may be null iff `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_side_quest_faction_for_quest_key(
    quest_key: u32,
    buf: *mut u8,
    buf_len: usize,
    required: *mut usize,
) -> c_int {
    faction_for_key(buf, buf_len, required, || {
        quest_key_index().get(&quest_key).copied()
    })
}

/// Read the game key of the row at `idx` — the companion of
/// [`crimson_side_quest_table_get_entry`], which returns its strings.
/// `*out_kind` is `1` for a `MissionKey` and `2` for a `QuestKey` (`0`
/// would mean unresolved; every side-quest row resolves).
///
/// Returns [`error::OUT_OF_RANGE`] if `idx >=
/// crimson_side_quest_table_entry_count`.
///
/// # Safety
/// `out_kind` and `out_key` must be non-null and writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_side_quest_table_get_entry_key(
    idx: u32,
    out_kind: *mut u32,
    out_key: *mut u32,
) -> c_int {
    if out_kind.is_null() || out_key.is_null() {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let Some(row) = ROWS.get(idx as usize) else {
            return error::OUT_OF_RANGE;
        };
        unsafe {
            *out_kind = row.2.kind_code();
            *out_key = row.2.key();
        }
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn faction_for_key(
    buf: *mut u8,
    buf_len: usize,
    required: *mut usize,
    row: impl FnOnce() -> Option<usize>,
) -> c_int {
    if required.is_null() {
        return error::NULL_ARG;
    }
    if buf.is_null() && buf_len != 0 {
        return error::NULL_ARG;
    }
    unsafe { *required = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let Some(i) = row() else {
            return error::NOT_FOUND;
        };
        write_str_to_buf(ROWS[i].1, buf, buf_len, required)
    }))
    .unwrap_or(error::PANIC)
}

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
    //! 5. Keys — unique per kind, the key lookups, and
    //!    `curated_titles_match_live_install`, which checks every row's key
    //!    against the live install's title (skips cleanly without one).
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
        // Multi-word faction (2.02 title; the wiki had "Bounty Target: …")
        assert_eq!(
            call_faction_for_quest("Bounty Notice - Simon de Montfort").unwrap(),
            "House Celeste"
        );
        // Singleton faction (one quest in the curated set)
        assert_eq!(
            call_faction_for_quest("Trembling Woods").unwrap(),
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
        assert_eq!(pororin, vec!["Trembling Woods".to_string()]);
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

    fn call_by_key(
        f: unsafe extern "C" fn(u32, *mut u8, usize, *mut usize) -> c_int,
        key: u32,
    ) -> Result<String, i32> {
        let mut req: usize = 0;
        let rc = unsafe { f(key, ptr::null_mut(), 0, &mut req) };
        if rc == error::NOT_FOUND {
            return Err(rc);
        }
        Ok(fill(rc, req, |b, n, r| unsafe { f(key, b, n, r) }))
    }

    #[test]
    fn entry_keys_are_unique() {
        let mut seen: HashSet<(u32, u32)> = HashSet::with_capacity(ROWS.len());
        for (i, row) in ROWS.iter().enumerate() {
            assert_ne!(row.2.kind_code(), 0, "row {i}: every side-quest row resolves");
            assert!(seen.insert((row.2.kind_code(), row.2.key())), "row {i}: {:?} repeats", row.2);
        }
    }

    #[test]
    fn key_lookups_known_cases() {
        let m = crimson_side_quest_faction_for_mission_key;
        let q = crimson_side_quest_faction_for_quest_key;
        // "Record of the Greymanes" (quest) / "Carl's Request" (mission)
        assert_eq!(call_by_key(q, 1_000_881).unwrap(), "Scattered Embers");
        assert_eq!(call_by_key(m, 1_001_073).unwrap(), "Greymane Commissions");
        // Re-paired in the 2.02 reconciliation.
        assert_eq!(call_by_key(m, 1_000_344).unwrap(), "House Celeste");
        assert_eq!(call_by_key(q, 1_000_159).unwrap(), "Pororin Forest Guardians");
        // The key spaces overlap: 1_000_157 is the quest "Strongbox with
        // Wheels" here, and also a mission (Mission_Intro_Tutorial_I) that
        // this table does not list.
        assert_eq!(call_by_key(q, 1_000_157).unwrap(), "Scattered Embers");
        assert_eq!(call_by_key(m, 1_000_157), Err(error::NOT_FOUND));
        assert_eq!(call_by_key(m, 0), Err(error::NOT_FOUND));
    }

    #[test]
    fn entry_key_round_trip() {
        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_side_quest_table_entry_count(&mut count) },
            error::OK
        );
        let (mut kind, mut key) = (9u32, 9u32);
        for (idx, row) in ROWS.iter().enumerate() {
            let rc = unsafe { crimson_side_quest_table_get_entry_key(idx as u32, &mut kind, &mut key) };
            assert_eq!(rc, error::OK);
            assert_eq!((kind, key), (row.2.kind_code(), row.2.key()), "row {idx}");
        }
        let rc = unsafe { crimson_side_quest_table_get_entry_key(count, &mut kind, &mut key) };
        assert_eq!(rc, error::OUT_OF_RANGE);
        let rc = unsafe { crimson_side_quest_table_get_entry_key(0, ptr::null_mut(), &mut key) };
        assert_eq!(rc, error::NULL_ARG);
        let mut req: usize = 0;
        let rc = unsafe {
            crimson_side_quest_faction_for_mission_key(1_001_073, ptr::null_mut(), 4, &mut req)
        };
        assert_eq!(rc, error::NULL_ARG);
        let rc = unsafe {
            crimson_side_quest_faction_for_quest_key(1_000_881, ptr::null_mut(), 0, ptr::null_mut())
        };
        assert_eq!(rc, error::NULL_ARG);
    }

    #[test]
    fn curated_titles_match_live_install() {
        let Some(live) = crate::c_abi::live_titles::LiveTitles::load() else {
            eprintln!("skipping curated_titles_match_live_install: no game install");
            return;
        };
        let drift: Vec<String> = ROWS
            .iter()
            .filter_map(|&(title, _, entry)| {
                let got = match entry {
                    Mission(k) => live.mission(k),
                    Quest(k) => live.quest(k),
                    Entry::Unresolved => None,
                };
                (got != Some(title)).then(|| format!("{entry:?}: live {got:?}, table {title:?}"))
            })
            .collect();
        assert!(
            drift.is_empty(),
            "curated side-quest titles drifted from the live install:\n{}",
            drift.join("\n")
        );
    }
}
