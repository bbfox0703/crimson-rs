//! Main-quest chapter rollup — C ABI surface.
//!
//! Static curated `(chapter, arc, mission)` table sourced from
//! [`docs/main-quest-list.md`](../../docs/main-quest-list.md). Quest
//! chapter rollups ("Prologue: Dead of Night", "Chapter 1: The First
//! Encounter", …) are **not present in any RE'd gamedata table** —
//! [`docs/save-editor-keys-plan.md`](../../docs/save-editor-keys-plan.md)
//! records the "never located" status — so this bridge ships the
//! curated wiki-style breakdown as a static lookup. No file load, no
//! handle.
//!
//! The arc layer corresponds to the `lo32 = 0x100` (256) titles that
//! `questinfo.pabgb` rows resolve to (e.g. "Trials of Kindness",
//! "Hernand in Chaos") — same display strings produced by
//! [`super::quest_info::crimson_questinfo_lookup_display_name`]. The
//! mission layer corresponds to `missioninfo.pabgb` display titles
//! at `lo32 = 0x101` (e.g. "Where Rumors Gather"). The chapter layer
//! is curated.
//!
//! ## Lookup shape
//!
//! - [`crimson_main_quest_chapter_for_arc`] — arc title (the bold
//!   bullets in the source MD) → chapter heading. Arcs are unique
//!   across the curated set, so this is 1:1.
//! - [`crimson_main_quest_chapter_for_mission`] — mission title → its
//!   chapter heading. Three mission titles repeat across chapters
//!   ("In Ashes", "Reclamation", "The Counterattack"); first match by
//!   table order is documented behaviour. Callers needing
//!   disambiguation should pair with the arc title (which is unique).
//! - [`crimson_main_quest_arc_for_mission`] — mission title → arc
//!   title. Prologue missions return an empty string (the Prologue has
//!   no arcs).
//! - [`crimson_main_quest_table_entry_count`] +
//!   [`crimson_main_quest_table_get_entry`] — full enumeration.
//!
//! Stateless: backing data is a `const` table; lookup indices are
//! lazily built on first call via `OnceLock`. No load / free pair.

use std::collections::HashMap;
use std::os::raw::{c_char, c_int};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::OnceLock;

use super::error;

/// `(chapter_heading, arc_title, mission_title)`. `arc_title` is
/// `None` for Prologue entries (the Prologue has no arc layer).
type Row = (&'static str, Option<&'static str>, &'static str);

const ROWS: &[Row] = &[
    // ── Prologue: Dead of Night ──────────────────────────────────────
    ("Prologue: Dead of Night", None, "Ambush"),
    ("Prologue: Dead of Night", None, "Unfamiliar Lands"),
    ("Prologue: Dead of Night", None, "In Ashes"),
    ("Prologue: Dead of Night", None, "Unknown Space"),
    ("Prologue: Dead of Night", None, "Realm of Uncertainty"),
    ("Prologue: Dead of Night", None, "New Journey"),
    // ── Chapter 1: The First Encounter ───────────────────────────────
    // Trials of Kindness
    (
        "Chapter 1: The First Encounter",
        Some("Trials of Kindness"),
        "Where Rumors Gather",
    ),
    (
        "Chapter 1: The First Encounter",
        Some("Trials of Kindness"),
        "Mysterious Man",
    ),
    (
        "Chapter 1: The First Encounter",
        Some("Trials of Kindness"),
        "True Wisdom in Kindness",
    ),
    (
        "Chapter 1: The First Encounter",
        Some("Trials of Kindness"),
        "Actions Speak Louder than Words",
    ),
    (
        "Chapter 1: The First Encounter",
        Some("Trials of Kindness"),
        "Heart Beyond Borders",
    ),
    // Trace
    (
        "Chapter 1: The First Encounter",
        Some("Trace"),
        "Mystical Key",
    ),
    (
        "Chapter 1: The First Encounter",
        Some("Trace"),
        "Polar Opposites",
    ),
    (
        "Chapter 1: The First Encounter",
        Some("Trace"),
        "Abyss Without Balance",
    ),
    (
        "Chapter 1: The First Encounter",
        Some("Trace"),
        "Woman in White",
    ),
    // ── Chapter 2: Golden Greed ──────────────────────────────────────
    // Unexpected Gift
    (
        "Chapter 2: Golden Greed",
        Some("Unexpected Gift"),
        "Where the Light Leads",
    ),
    (
        "Chapter 2: Golden Greed",
        Some("Unexpected Gift"),
        "Memory Fragment",
    ),
    (
        "Chapter 2: Golden Greed",
        Some("Unexpected Gift"),
        "Reunion",
    ),
    // Hernand in Chaos
    (
        "Chapter 2: Golden Greed",
        Some("Hernand in Chaos"),
        "For Honor",
    ),
    (
        "Chapter 2: Golden Greed",
        Some("Hernand in Chaos"),
        "Awestruck",
    ),
    (
        "Chapter 2: Golden Greed",
        Some("Hernand in Chaos"),
        "Shadow Cast Over the River",
    ),
    (
        "Chapter 2: Golden Greed",
        Some("Hernand in Chaos"),
        "Where Misery Gathers",
    ),
    (
        "Chapter 2: Golden Greed",
        Some("Hernand in Chaos"),
        "Trial After Trial",
    ),
    (
        "Chapter 2: Golden Greed",
        Some("Hernand in Chaos"),
        "The Man Trapped in the Mire",
    ),
    (
        "Chapter 2: Golden Greed",
        Some("Hernand in Chaos"),
        "Missing Companion",
    ),
    (
        "Chapter 2: Golden Greed",
        Some("Hernand in Chaos"),
        "Secrets Hidden in the Dark",
    ),
    // The End of Greed
    (
        "Chapter 2: Golden Greed",
        Some("The End of Greed"),
        "The Dark Veil",
    ),
    (
        "Chapter 2: Golden Greed",
        Some("The End of Greed"),
        "The Flames of Greed",
    ),
    (
        "Chapter 2: Golden Greed",
        Some("The End of Greed"),
        "Kidnapped Healer",
    ),
    (
        "Chapter 2: Golden Greed",
        Some("The End of Greed"),
        "Rebellion or Revolution",
    ),
    (
        "Chapter 2: Golden Greed",
        Some("The End of Greed"),
        "Cheers Echoing From the Edge",
    ),
    // ── Chapter 3: Howling Hill ──────────────────────────────────────
    // Homestead
    (
        "Chapter 3: Howling Hill",
        Some("Homestead"),
        "Old Friend",
    ),
    (
        "Chapter 3: Howling Hill",
        Some("Homestead"),
        "First Step to Rebuilding",
    ),
    (
        "Chapter 3: Howling Hill",
        Some("Homestead"),
        "A Fresh Start",
    ),
    (
        "Chapter 3: Howling Hill",
        Some("Homestead"),
        "Reward for Their Sweat",
    ),
    (
        "Chapter 3: Howling Hill",
        Some("Homestead"),
        "Return of the Comrade",
    ),
    (
        "Chapter 3: Howling Hill",
        Some("Homestead"),
        "Familiar Curses",
    ),
    // The Face Behind the Mask
    (
        "Chapter 3: Howling Hill",
        Some("The Face Behind the Mask"),
        "Return",
    ),
    (
        "Chapter 3: Howling Hill",
        Some("The Face Behind the Mask"),
        "Traces in the Manor",
    ),
    (
        "Chapter 3: Howling Hill",
        Some("The Face Behind the Mask"),
        "Nonhuman",
    ),
    (
        "Chapter 3: Howling Hill",
        Some("The Face Behind the Mask"),
        "Seed of Unease",
    ),
    (
        "Chapter 3: Howling Hill",
        Some("The Face Behind the Mask"),
        "Dance with the Devil",
    ),
    // Pioneering
    (
        "Chapter 3: Howling Hill",
        Some("Pioneering"),
        "Hope After the Draught",
    ),
    (
        "Chapter 3: Howling Hill",
        Some("Pioneering"),
        "Scattered Comrades",
    ),
    (
        "Chapter 3: Howling Hill",
        Some("Pioneering"),
        "Rumors from the Sawmill",
    ),
    (
        "Chapter 3: Howling Hill",
        Some("Pioneering"),
        "A Gentle Touch",
    ),
    (
        "Chapter 3: Howling Hill",
        Some("Pioneering"),
        "Bustling Hill",
    ),
    (
        "Chapter 3: Howling Hill",
        Some("Pioneering"),
        "Greymanes Reunited",
    ),
    // ── Chapter 4: The Price of Knowledge ────────────────────────────
    // Mysterious Iron Pot
    (
        "Chapter 4: The Price of Knowledge",
        Some("Mysterious Iron Pot"),
        "Kilnden Workshop",
    ),
    (
        "Chapter 4: The Price of Knowledge",
        Some("Mysterious Iron Pot"),
        "Kiln Repair at the Kilnden Workshop",
    ),
    (
        "Chapter 4: The Price of Knowledge",
        Some("Mysterious Iron Pot"),
        "The Mysterious Pot",
    ),
    (
        "Chapter 4: The Price of Knowledge",
        Some("Mysterious Iron Pot"),
        "The Iron Pot's Usage",
    ),
    // Daily Life
    (
        "Chapter 4: The Price of Knowledge",
        Some("Daily Life"),
        "Disturbance at the Arena",
    ),
    (
        "Chapter 4: The Price of Knowledge",
        Some("Daily Life"),
        "Skilled in Archery",
    ),
    // Forbidden Knowledge
    (
        "Chapter 4: The Price of Knowledge",
        Some("Forbidden Knowledge"),
        "The Words of Alustin",
    ),
    (
        "Chapter 4: The Price of Knowledge",
        Some("Forbidden Knowledge"),
        "Scholastone",
    ),
    (
        "Chapter 4: The Price of Knowledge",
        Some("Forbidden Knowledge"),
        "On the Right Path",
    ),
    (
        "Chapter 4: The Price of Knowledge",
        Some("Forbidden Knowledge"),
        "Gate to the Otherworld",
    ),
    (
        "Chapter 4: The Price of Knowledge",
        Some("Forbidden Knowledge"),
        "Spire of the Stars",
    ),
    (
        "Chapter 4: The Price of Knowledge",
        Some("Forbidden Knowledge"),
        "Obsession and Madness",
    ),
    (
        "Chapter 4: The Price of Knowledge",
        Some("Forbidden Knowledge"),
        "Casted Shadow",
    ),
    // ── Chapter 5: Guest Unbidden ────────────────────────────────────
    // Uninvited Guest
    (
        "Chapter 5: Guest Unbidden",
        Some("Uninvited Guest"),
        "Double-sided Invitation",
    ),
    (
        "Chapter 5: Guest Unbidden",
        Some("Uninvited Guest"),
        "Unwelcomed Guests",
    ),
    (
        "Chapter 5: Guest Unbidden",
        Some("Uninvited Guest"),
        "Demenissian Delegation",
    ),
    (
        "Chapter 5: Guest Unbidden",
        Some("Uninvited Guest"),
        "Exposed Plot",
    ),
    // Black and White
    (
        "Chapter 5: Guest Unbidden",
        Some("Black and White"),
        "The Missing Seal",
    ),
    (
        "Chapter 5: Guest Unbidden",
        Some("Black and White"),
        "Crowcaller",
    ),
    (
        "Chapter 5: Guest Unbidden",
        Some("Black and White"),
        "The Crow's Warning",
    ),
    (
        "Chapter 5: Guest Unbidden",
        Some("Black and White"),
        "Bloodwind",
    ),
    (
        "Chapter 5: Guest Unbidden",
        Some("Black and White"),
        "Secret at the Church",
    ),
    (
        "Chapter 5: Guest Unbidden",
        Some("Black and White"),
        "Toward the Nest (Spire of Soaring)",
    ),
    // ── Chapter 6: Cracks in the Shield ──────────────────────────────
    // Blazing Beacon
    (
        "Chapter 6: Cracks in the Shield",
        Some("Blazing Beacon"),
        "News",
    ),
    (
        "Chapter 6: Cracks in the Shield",
        Some("Blazing Beacon"),
        "To the Battlefield",
    ),
    (
        "Chapter 6: Cracks in the Shield",
        Some("Blazing Beacon"),
        "The Counterattack",
    ),
    // Under the Banner
    (
        "Chapter 6: Cracks in the Shield",
        Some("Under the Banner"),
        "Pike Again",
    ),
    // Cradle of Defense
    (
        "Chapter 6: Cracks in the Shield",
        Some("Cradle of Defense"),
        "The Touch of Deliverance",
    ),
    (
        "Chapter 6: Cracks in the Shield",
        Some("Cradle of Defense"),
        "Fire on the Frontlines",
    ),
    // Turning Tides
    (
        "Chapter 6: Cracks in the Shield",
        Some("Turning Tides"),
        "Fire Support",
    ),
    (
        "Chapter 6: Cracks in the Shield",
        Some("Turning Tides"),
        "In Ashes",
    ),
    (
        "Chapter 6: Cracks in the Shield",
        Some("Turning Tides"),
        "Hidden Fangs",
    ),
    (
        "Chapter 6: Cracks in the Shield",
        Some("Turning Tides"),
        "Reclamation",
    ),
    // The Undying Shields
    (
        "Chapter 6: Cracks in the Shield",
        Some("The Undying Shields"),
        "A Thousand Troops",
    ),
    (
        "Chapter 6: Cracks in the Shield",
        Some("The Undying Shields"),
        "Traitor",
    ),
    (
        "Chapter 6: Cracks in the Shield",
        Some("The Undying Shields"),
        "All Quiet on the Front",
    ),
    (
        "Chapter 6: Cracks in the Shield",
        Some("The Undying Shields"),
        "News of Victory",
    ),
    (
        "Chapter 6: Cracks in the Shield",
        Some("The Undying Shields"),
        "Return Home",
    ),
    // ── Chapter 7: Homecoming ────────────────────────────────────────
    // Dawn Mist
    (
        "Chapter 7: Homecoming",
        Some("Dawn Mist"),
        "Ashes of Treachery",
    ),
    ("Chapter 7: Homecoming", Some("Dawn Mist"), "Trust Lost"),
    ("Chapter 7: Homecoming", Some("Dawn Mist"), "Bared Fang"),
    (
        "Chapter 7: Homecoming",
        Some("Dawn Mist"),
        "Rekindled Hope",
    ),
    (
        "Chapter 7: Homecoming",
        Some("Dawn Mist"),
        "Podium of Resolve",
    ),
    // Dawnrise
    (
        "Chapter 7: Homecoming",
        Some("Dawnrise"),
        "Shadows Over Pailune",
    ),
    (
        "Chapter 7: Homecoming",
        Some("Dawnrise"),
        "Driving out the Shadows",
    ),
    (
        "Chapter 7: Homecoming",
        Some("Dawnrise"),
        "Lurking Wolves",
    ),
    ("Chapter 7: Homecoming", Some("Dawnrise"), "Reclamation"),
    (
        "Chapter 7: Homecoming",
        Some("Dawnrise"),
        "Lonely Jackals",
    ),
    ("Chapter 7: Homecoming", Some("Dawnrise"), "Resolution"),
    // Decisive Battle
    (
        "Chapter 7: Homecoming",
        Some("Decisive Battle"),
        "The Counterattack",
    ),
    (
        "Chapter 7: Homecoming",
        Some("Decisive Battle"),
        "Unleashed Fury",
    ),
    (
        "Chapter 7: Homecoming",
        Some("Decisive Battle"),
        "The Final Bridge",
    ),
    (
        "Chapter 7: Homecoming",
        Some("Decisive Battle"),
        "Broken Claws",
    ),
    (
        "Chapter 7: Homecoming",
        Some("Decisive Battle"),
        "Battle at Silverwolf Mountain",
    ),
    (
        "Chapter 7: Homecoming",
        Some("Decisive Battle"),
        "Incomplete Victory",
    ),
    // Twisted Fate
    (
        "Chapter 7: Homecoming",
        Some("Twisted Fate"),
        "Ludvig's Whereabouts",
    ),
    (
        "Chapter 7: Homecoming",
        Some("Twisted Fate"),
        "Time to Face Justice",
    ),
    // ── Chapter 8: Blood Coronation ──────────────────────────────────
    // Ashen Steps
    (
        "Chapter 8: Blood Coronation",
        Some("Ashen Steps"),
        "Healing Pailune",
    ),
    (
        "Chapter 8: Blood Coronation",
        Some("Ashen Steps"),
        "A Bond",
    ),
    (
        "Chapter 8: Blood Coronation",
        Some("Ashen Steps"),
        "Ritual Preparations",
    ),
    (
        "Chapter 8: Blood Coronation",
        Some("Ashen Steps"),
        "Where the Wind Guides You",
    ),
    // To Demeniss
    (
        "Chapter 8: Blood Coronation",
        Some("To Demeniss"),
        "Chasing a Shadow",
    ),
    (
        "Chapter 8: Blood Coronation",
        Some("To Demeniss"),
        "Blazing Fire",
    ),
    (
        "Chapter 8: Blood Coronation",
        Some("To Demeniss"),
        "Whispering Shadows",
    ),
    (
        "Chapter 8: Blood Coronation",
        Some("To Demeniss"),
        "Bloodied Invitation",
    ),
    (
        "Chapter 8: Blood Coronation",
        Some("To Demeniss"),
        "Resolve Amidst a Storm",
    ),
    (
        "Chapter 8: Blood Coronation",
        Some("To Demeniss"),
        "Preparations for Advance",
    ),
    (
        "Chapter 8: Blood Coronation",
        Some("To Demeniss"),
        "Rebel Suppression",
    ),
    (
        "Chapter 8: Blood Coronation",
        Some("To Demeniss"),
        "The Cursed Knight",
    ),
    (
        "Chapter 8: Blood Coronation",
        Some("To Demeniss"),
        "The Blood Coronation",
    ),
    // Traitor (arc — distinct from the Ch6 "Traitor" mission)
    (
        "Chapter 8: Blood Coronation",
        Some("Traitor"),
        "Clue",
    ),
    (
        "Chapter 8: Blood Coronation",
        Some("Traitor"),
        "A Fleeting Dream",
    ),
    // ── Chapter 9: The Sage of the Desert ────────────────────────────
    // The Calling
    (
        "Chapter 9: The Sage of the Desert",
        Some("The Calling"),
        "An Unknown Voice",
    ),
    (
        "Chapter 9: The Sage of the Desert",
        Some("The Calling"),
        "Cloister of Enlightenment",
    ),
    // Shattered Ties
    (
        "Chapter 9: The Sage of the Desert",
        Some("Shattered Ties"),
        "Mark of the Scar",
    ),
    (
        "Chapter 9: The Sage of the Desert",
        Some("Shattered Ties"),
        "Shackles of Fate",
    ),
    // Thinning Blade
    (
        "Chapter 9: The Sage of the Desert",
        Some("Thinning Blade"),
        "Crossing Point",
    ),
    (
        "Chapter 9: The Sage of the Desert",
        Some("Thinning Blade"),
        "Unwavering Steps",
    ),
    // Six Pensive Statues and the Evil Spirit
    (
        "Chapter 9: The Sage of the Desert",
        Some("Six Pensive Statues and the Evil Spirit"),
        "Morning Fog",
    ),
    (
        "Chapter 9: The Sage of the Desert",
        Some("Six Pensive Statues and the Evil Spirit"),
        "Jijeong Temple in Chaos",
    ),
    (
        "Chapter 9: The Sage of the Desert",
        Some("Six Pensive Statues and the Evil Spirit"),
        "Path to Enlightenment",
    ),
    (
        "Chapter 9: The Sage of the Desert",
        Some("Six Pensive Statues and the Evil Spirit"),
        "Path of the Disciple",
    ),
    (
        "Chapter 9: The Sage of the Desert",
        Some("Six Pensive Statues and the Evil Spirit"),
        "True Strength",
    ),
    (
        "Chapter 9: The Sage of the Desert",
        Some("Six Pensive Statues and the Evil Spirit"),
        "Face the Inner Self",
    ),
    // Veiled Witch
    (
        "Chapter 9: The Sage of the Desert",
        Some("Veiled Witch"),
        "Fragments of Darkness",
    ),
    (
        "Chapter 9: The Sage of the Desert",
        Some("Veiled Witch"),
        "Pursuit Beyond the Veil",
    ),
    (
        "Chapter 9: The Sage of the Desert",
        Some("Veiled Witch"),
        "Black Witch",
    ),
    // Enlightenment
    (
        "Chapter 9: The Sage of the Desert",
        Some("Enlightenment"),
        "The Cloister of Enlightenment",
    ),
    (
        "Chapter 9: The Sage of the Desert",
        Some("Enlightenment"),
        "The Sage of the Desert",
    ),
    (
        "Chapter 9: The Sage of the Desert",
        Some("Enlightenment"),
        "New Perspectives",
    ),
    (
        "Chapter 9: The Sage of the Desert",
        Some("Enlightenment"),
        "Lust for Power",
    ),
    // ── Chapter 10: Counterattack ────────────────────────────────────
    // Secret Weapon
    (
        "Chapter 10: Counterattack",
        Some("Secret Weapon"),
        "Untouchable",
    ),
    (
        "Chapter 10: Counterattack",
        Some("Secret Weapon"),
        "The Gate of War",
    ),
    (
        "Chapter 10: Counterattack",
        Some("Secret Weapon"),
        "Master of the Ironworks",
    ),
    (
        "Chapter 10: Counterattack",
        Some("Secret Weapon"),
        "Hidden Ace",
    ),
    (
        "Chapter 10: Counterattack",
        Some("Secret Weapon"),
        "Clockwork Insect Clash",
    ),
    // Greater Firepower
    (
        "Chapter 10: Counterattack",
        Some("Greater Firepower"),
        "Beating Heart",
    ),
    (
        "Chapter 10: Counterattack",
        Some("Greater Firepower"),
        "Invaders from the East",
    ),
    (
        "Chapter 10: Counterattack",
        Some("Greater Firepower"),
        "Frozen Hearted Predator",
    ),
    (
        "Chapter 10: Counterattack",
        Some("Greater Firepower"),
        "Lingering Shadow",
    ),
    // ── Chapter 11: Truth and Reality ────────────────────────────────
    // Brave New World
    (
        "Chapter 11: Truth and Reality",
        Some("Brave New World"),
        "The City of Steel",
    ),
    (
        "Chapter 11: Truth and Reality",
        Some("Brave New World"),
        "Crossroads",
    ),
    (
        "Chapter 11: Truth and Reality",
        Some("Brave New World"),
        "Strange Manor",
    ),
    (
        "Chapter 11: Truth and Reality",
        Some("Brave New World"),
        "Fortress Keys",
    ),
    (
        "Chapter 11: Truth and Reality",
        Some("Brave New World"),
        "Truth and Lies",
    ),
    // Foreboding Shadow
    (
        "Chapter 11: Truth and Reality",
        Some("Foreboding Shadow"),
        "Master of a Forgotten Land",
    ),
    (
        "Chapter 11: Truth and Reality",
        Some("Foreboding Shadow"),
        "Whispers in the Wind",
    ),
    (
        "Chapter 11: Truth and Reality",
        Some("Foreboding Shadow"),
        "Cloud Fortress Orbian",
    ),
    // ── Chapter 12: The Abyss ────────────────────────────────────────
    // The Final Battle
    (
        "Chapter 12: The Abyss",
        Some("The Final Battle"),
        "Precise Execution",
    ),
    (
        "Chapter 12: The Abyss",
        Some("The Final Battle"),
        "Deferred Advance",
    ),
    (
        "Chapter 12: The Abyss",
        Some("The Final Battle"),
        "Departure of the Brave",
    ),
    (
        "Chapter 12: The Abyss",
        Some("The Final Battle"),
        "Forbidden Gate",
    ),
    // The Void
    (
        "Chapter 12: The Abyss",
        Some("The Void"),
        "A Shadow in the Void",
    ),
    (
        "Chapter 12: The Abyss",
        Some("The Void"),
        "Blinding Darkness",
    ),
    // ── Epilogue: Journey's End ──────────────────────────────────────
    // Journey's End
    (
        "Epilogue: Journey's End",
        Some("Journey's End"),
        "A New Beginning",
    ),
    (
        "Epilogue: Journey's End",
        Some("Journey's End"),
        "Peace in Hernand",
    ),
    (
        "Epilogue: Journey's End",
        Some("Journey's End"),
        "The Unyielding Shields",
    ),
    (
        "Epilogue: Journey's End",
        Some("Journey's End"),
        "The Heart of Pywel",
    ),
    (
        "Epilogue: Journey's End",
        Some("Journey's End"),
        "The Enduring Flame",
    ),
    (
        "Epilogue: Journey's End",
        Some("Journey's End"),
        "Evolving City",
    ),
    (
        "Epilogue: Journey's End",
        Some("Journey's End"),
        "The Desert's Edge",
    ),
    (
        "Epilogue: Journey's End",
        Some("Journey's End"),
        "New Horizons",
    ),
];

/// Lookup index: arc title → first row index in [`ROWS`].
fn arc_index() -> &'static HashMap<&'static str, usize> {
    static IDX: OnceLock<HashMap<&'static str, usize>> = OnceLock::new();
    IDX.get_or_init(|| {
        let mut m: HashMap<&'static str, usize> = HashMap::new();
        for (i, row) in ROWS.iter().enumerate() {
            if let Some(arc) = row.1 {
                m.entry(arc).or_insert(i);
            }
        }
        m
    })
}

/// Lookup index: mission title → first row index in [`ROWS`]. Three
/// titles collide ("In Ashes", "Reclamation", "The Counterattack");
/// `or_insert` keeps the earliest in declared table order.
fn mission_index() -> &'static HashMap<&'static str, usize> {
    static IDX: OnceLock<HashMap<&'static str, usize>> = OnceLock::new();
    IDX.get_or_init(|| {
        let mut m: HashMap<&'static str, usize> = HashMap::with_capacity(ROWS.len());
        for (i, row) in ROWS.iter().enumerate() {
            m.entry(row.2).or_insert(i);
        }
        m
    })
}

// ── Enumeration ────────────────────────────────────────────────────────────

/// Total number of `(chapter, arc, mission)` rows in the curated
/// table. Stable across runs of the same library build.
///
/// # Safety
/// `out_count` must be non-null and writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_main_quest_table_entry_count(out_count: *mut u32) -> c_int {
    if out_count.is_null() {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        unsafe { *out_count = ROWS.len() as u32 };
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

/// Read the row at `idx`. Each of the three string outputs uses the
/// standard two-call sizing pattern: pass `*_buf = null, *_buf_len = 0`
/// to query the required byte count (including the trailing NUL).
///
/// For Prologue rows the arc string is empty (`*arc_required == 1` —
/// a single NUL byte). All other rows return a non-empty arc.
///
/// Returns [`error::OUT_OF_RANGE`] if `idx >=
/// crimson_main_quest_table_entry_count`. Returns
/// [`error::BUFFER_TOO_SMALL`] if any of the three output buffers is
/// shorter than its corresponding `*_required` value (probe with
/// `buf_len = 0` first).
///
/// # Safety
/// `chapter_required`, `arc_required`, and `mission_required` must
/// all be non-null. Each `*_buf` may be null iff its `*_buf_len == 0`.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn crimson_main_quest_table_get_entry(
    idx: u32,
    chapter_buf: *mut u8,
    chapter_buf_len: usize,
    chapter_required: *mut usize,
    arc_buf: *mut u8,
    arc_buf_len: usize,
    arc_required: *mut usize,
    mission_buf: *mut u8,
    mission_buf_len: usize,
    mission_required: *mut usize,
) -> c_int {
    if chapter_required.is_null() || arc_required.is_null() || mission_required.is_null() {
        return error::NULL_ARG;
    }
    if (chapter_buf.is_null() && chapter_buf_len != 0)
        || (arc_buf.is_null() && arc_buf_len != 0)
        || (mission_buf.is_null() && mission_buf_len != 0)
    {
        return error::NULL_ARG;
    }
    unsafe {
        *chapter_required = 0;
        *arc_required = 0;
        *mission_required = 0;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let Some(row) = ROWS.get(idx as usize) else {
            return error::OUT_OF_RANGE;
        };
        let chapter = row.0;
        let arc = row.1.unwrap_or("");
        let mission = row.2;

        // Probe-or-fill each slot independently. The two-step pattern is
        // standard across this ABI: query sizes with all three buf_lens=0
        // first, then allocate, then call again.
        let rc_a = write_str_to_buf(chapter, chapter_buf, chapter_buf_len, chapter_required);
        let rc_b = write_str_to_buf(arc, arc_buf, arc_buf_len, arc_required);
        let rc_c = write_str_to_buf(mission, mission_buf, mission_buf_len, mission_required);
        // If any slot was too small, surface that. The `required` outputs
        // are populated regardless so the caller can resize and retry.
        if rc_a == error::BUFFER_TOO_SMALL
            || rc_b == error::BUFFER_TOO_SMALL
            || rc_c == error::BUFFER_TOO_SMALL
        {
            return error::BUFFER_TOO_SMALL;
        }
        error::OK
    }))
    .unwrap_or(error::PANIC)
}

// ── Lookups ────────────────────────────────────────────────────────────────

/// Resolve a quest arc display title (the bold bullets in the source
/// MD — e.g. "Trials of Kindness", "Hernand in Chaos", "Journey's End")
/// to its chapter heading.
///
/// `arc_title` must be NUL-terminated UTF-8. Returns
/// [`error::NOT_FOUND`] if the arc isn't in the curated set; otherwise
/// fills `buf` per the standard two-call pattern.
///
/// # Safety
/// `arc_title` must point to a valid NUL-terminated UTF-8 C string.
/// `required` must be non-null. `buf` may be null iff `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_main_quest_chapter_for_arc(
    arc_title: *const c_char,
    buf: *mut u8,
    buf_len: usize,
    required: *mut usize,
) -> c_int {
    lookup_string_to_string(arc_title, buf, buf_len, required, |key| {
        arc_index().get(key).map(|&i| ROWS[i].0)
    })
}

/// Resolve a mission display title (e.g. "Where Rumors Gather",
/// "Unfamiliar Lands") to its chapter heading.
///
/// Three titles repeat across chapters ("In Ashes", "Reclamation",
/// "The Counterattack"); first match by table order wins. Callers that
/// need disambiguation should pair the mission with its arc title
/// (resolved via the existing [`super::quest_info`] /
/// [`super::mission_info`] bridges) and call
/// [`crimson_main_quest_chapter_for_arc`] instead.
///
/// # Safety
/// `mission_title` must point to a valid NUL-terminated UTF-8 C
/// string. `required` must be non-null. `buf` may be null iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_main_quest_chapter_for_mission(
    mission_title: *const c_char,
    buf: *mut u8,
    buf_len: usize,
    required: *mut usize,
) -> c_int {
    lookup_string_to_string(mission_title, buf, buf_len, required, |key| {
        mission_index().get(key).map(|&i| ROWS[i].0)
    })
}

/// Resolve a mission display title to its quest arc title (e.g.
/// "Where Rumors Gather" → "Trials of Kindness").
///
/// Prologue missions have no arc — they resolve to the empty string
/// (`*required == 1` for the NUL terminator, single-NUL byte written
/// once the caller passes a non-zero buffer). Returns
/// [`error::NOT_FOUND`] if the mission isn't in the curated set.
///
/// Same first-match disambiguation behaviour as
/// [`crimson_main_quest_chapter_for_mission`] for the three repeated
/// titles.
///
/// # Safety
/// `mission_title` must point to a valid NUL-terminated UTF-8 C
/// string. `required` must be non-null. `buf` may be null iff
/// `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_main_quest_arc_for_mission(
    mission_title: *const c_char,
    buf: *mut u8,
    buf_len: usize,
    required: *mut usize,
) -> c_int {
    lookup_string_to_string(mission_title, buf, buf_len, required, |key| {
        mission_index().get(key).map(|&i| ROWS[i].1.unwrap_or(""))
    })
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn lookup_string_to_string(
    key_c: *const c_char,
    buf: *mut u8,
    buf_len: usize,
    required: *mut usize,
    resolve: impl FnOnce(&str) -> Option<&'static str>,
) -> c_int {
    if required.is_null() || key_c.is_null() {
        return error::NULL_ARG;
    }
    if buf.is_null() && buf_len != 0 {
        return error::NULL_ARG;
    }
    unsafe { *required = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let key = match unsafe { std::ffi::CStr::from_ptr(key_c) }.to_str() {
            Ok(s) => s,
            Err(_) => return error::INVALID_PATH,
        };
        let Some(value) = resolve(key) else {
            return error::NOT_FOUND;
        };
        write_str_to_buf(value, buf, buf_len, required)
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
    //! Tests cover four axes:
    //!
    //! 1. Curated-table integrity — no row has an empty mission, every
    //!    non-Prologue row has a non-empty arc, no row has an empty
    //!    chapter.
    //! 2. Forward / reverse lookups — the documented "trace" cases
    //!    from `docs/main-quest-list.md` resolve in both directions.
    //! 3. Collision behaviour — the three repeated mission titles
    //!    resolve to the *first* declared chapter, and the arc lookup
    //!    surfaces the corresponding arc.
    //! 4. ABI hygiene — NULL args / OOR index / buffer too small /
    //!    NOT_FOUND for unknown strings.
    //!
    //! These are pure-Rust tests with no live-install dependency, so
    //! they run on every CI build.
    use super::*;
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

    fn call_chapter_for_arc(arc: &str) -> Result<String, i32> {
        let c = CString::new(arc).unwrap();
        let mut req: usize = 0;
        let rc = unsafe {
            crimson_main_quest_chapter_for_arc(c.as_ptr(), ptr::null_mut(), 0, &mut req)
        };
        if rc == error::NOT_FOUND {
            return Err(rc);
        }
        Ok(fill(rc, req, |b, n, r| unsafe {
            crimson_main_quest_chapter_for_arc(c.as_ptr(), b, n, r)
        }))
    }

    fn call_chapter_for_mission(mission: &str) -> Result<String, i32> {
        let c = CString::new(mission).unwrap();
        let mut req: usize = 0;
        let rc = unsafe {
            crimson_main_quest_chapter_for_mission(c.as_ptr(), ptr::null_mut(), 0, &mut req)
        };
        if rc == error::NOT_FOUND {
            return Err(rc);
        }
        Ok(fill(rc, req, |b, n, r| unsafe {
            crimson_main_quest_chapter_for_mission(c.as_ptr(), b, n, r)
        }))
    }

    fn call_arc_for_mission(mission: &str) -> Result<String, i32> {
        let c = CString::new(mission).unwrap();
        let mut req: usize = 0;
        let rc = unsafe {
            crimson_main_quest_arc_for_mission(c.as_ptr(), ptr::null_mut(), 0, &mut req)
        };
        if rc == error::NOT_FOUND {
            return Err(rc);
        }
        // Prologue missions: the resolved value is "" so needed=1 and
        // the probe (buf_len=0) returns BUFFER_TOO_SMALL with req=1 —
        // same shape as any other value. The fill helper handles it.
        Ok(fill(rc, req, |b, n, r| unsafe {
            crimson_main_quest_arc_for_mission(c.as_ptr(), b, n, r)
        }))
    }

    #[test]
    fn curated_table_integrity() {
        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_main_quest_table_entry_count(&mut count) },
            error::OK
        );
        assert_eq!(count as usize, ROWS.len());
        assert!(count > 100, "expected >100 curated quests, got {count}");
        for (i, row) in ROWS.iter().enumerate() {
            assert!(!row.0.is_empty(), "row {i}: empty chapter");
            assert!(!row.2.is_empty(), "row {i}: empty mission");
            if !row.0.starts_with("Prologue") {
                let arc = row.1.unwrap_or_else(|| panic!("row {i}: non-Prologue with no arc"));
                assert!(!arc.is_empty(), "row {i}: empty arc string");
            }
        }
    }

    #[test]
    fn chapter_for_arc_known_cases() {
        // Sample across the structure: Ch1 first arc, Ch8 ambiguous
        // "Traitor" arc, Epilogue arc (also matches its chapter title).
        assert_eq!(
            call_chapter_for_arc("Trials of Kindness").unwrap(),
            "Chapter 1: The First Encounter"
        );
        assert_eq!(
            call_chapter_for_arc("Hernand in Chaos").unwrap(),
            "Chapter 2: Golden Greed"
        );
        // "Traitor" exists as both an arc (Ch8) AND a mission (Ch6).
        // The arc lookup is unambiguous: it's the Ch8 arc.
        assert_eq!(
            call_chapter_for_arc("Traitor").unwrap(),
            "Chapter 8: Blood Coronation"
        );
        assert_eq!(
            call_chapter_for_arc("Journey's End").unwrap(),
            "Epilogue: Journey's End"
        );
        // Unknown arc
        assert_eq!(call_chapter_for_arc("No Such Arc"), Err(error::NOT_FOUND));
    }

    #[test]
    fn chapter_for_mission_known_cases() {
        // Prologue mission (no arc)
        assert_eq!(
            call_chapter_for_mission("Unfamiliar Lands").unwrap(),
            "Prologue: Dead of Night"
        );
        // Mid-game arc mission
        assert_eq!(
            call_chapter_for_mission("Where Rumors Gather").unwrap(),
            "Chapter 1: The First Encounter"
        );
        assert_eq!(
            call_chapter_for_mission("The Crow's Warning").unwrap(),
            "Chapter 5: Guest Unbidden"
        );
        // Mission with an apostrophe — "Ludvig's Whereabouts"
        assert_eq!(
            call_chapter_for_mission("Ludvig's Whereabouts").unwrap(),
            "Chapter 7: Homecoming"
        );
        // Unknown mission
        assert_eq!(
            call_chapter_for_mission("No Such Mission"),
            Err(error::NOT_FOUND)
        );
    }

    #[test]
    fn collision_first_match_is_table_order() {
        // "In Ashes" appears in Prologue first, then Ch6/Turning Tides.
        // First-match returns the earlier declaration.
        assert_eq!(
            call_chapter_for_mission("In Ashes").unwrap(),
            "Prologue: Dead of Night",
        );
        assert_eq!(call_arc_for_mission("In Ashes").unwrap(), "");

        // "Reclamation" first appears in Ch6/Turning Tides, then Ch7/Dawnrise.
        assert_eq!(
            call_chapter_for_mission("Reclamation").unwrap(),
            "Chapter 6: Cracks in the Shield",
        );
        assert_eq!(call_arc_for_mission("Reclamation").unwrap(), "Turning Tides");

        // "The Counterattack" first in Ch6/Blazing Beacon, then Ch7/Decisive Battle.
        assert_eq!(
            call_chapter_for_mission("The Counterattack").unwrap(),
            "Chapter 6: Cracks in the Shield",
        );
        assert_eq!(call_arc_for_mission("The Counterattack").unwrap(), "Blazing Beacon");

        // "Traitor" appears as a mission in Ch6/The Undying Shields (the
        // arc lookup separately surfaces Ch8 — see chapter_for_arc_known_cases).
        assert_eq!(
            call_chapter_for_mission("Traitor").unwrap(),
            "Chapter 6: Cracks in the Shield",
        );
        assert_eq!(call_arc_for_mission("Traitor").unwrap(), "The Undying Shields");
    }

    #[test]
    fn arc_for_mission_prologue_returns_empty_string() {
        // Prologue missions have no arc → resolves to "" so the probe
        // (buf_len=0) returns BUFFER_TOO_SMALL with required=1 (one
        // byte for the NUL terminator) — same shape as any other
        // value. Filling into a sized buffer then returns OK with a
        // single NUL byte.
        let c = CString::new("Ambush").unwrap();
        let mut req: usize = 0;
        let rc = unsafe {
            crimson_main_quest_arc_for_mission(c.as_ptr(), ptr::null_mut(), 0, &mut req)
        };
        assert_eq!(rc, error::BUFFER_TOO_SMALL);
        assert_eq!(req, 1, "expected required=1 for empty arc string");

        // Fill into a 1-byte buffer should succeed and produce exactly
        // a NUL.
        let mut out = [0xFFu8; 4];
        let mut req2: usize = 0;
        let rc = unsafe {
            crimson_main_quest_arc_for_mission(c.as_ptr(), out.as_mut_ptr(), out.len(), &mut req2)
        };
        assert_eq!(rc, error::OK);
        assert_eq!(req2, 1);
        assert_eq!(out[0], 0);
    }

    #[test]
    fn enumeration_round_trip() {
        // Fetch every row via get_entry and rebuild a HashSet — then
        // verify every direct ROWS entry is recoverable. Mainly a
        // smoke test that the two-call buffer pattern works for all
        // three string outputs and the OUT_OF_RANGE boundary is solid.
        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_main_quest_table_entry_count(&mut count) },
            error::OK
        );

        for idx in 0..count {
            // Probe sizes
            let (mut chap_req, mut arc_req, mut mis_req) = (0usize, 0usize, 0usize);
            let rc = unsafe {
                crimson_main_quest_table_get_entry(
                    idx,
                    ptr::null_mut(),
                    0,
                    &mut chap_req,
                    ptr::null_mut(),
                    0,
                    &mut arc_req,
                    ptr::null_mut(),
                    0,
                    &mut mis_req,
                )
            };
            assert_eq!(rc, error::BUFFER_TOO_SMALL);
            assert!(chap_req >= 1);
            assert!(arc_req >= 1);
            assert!(mis_req >= 1);

            let mut chap_buf = vec![0u8; chap_req];
            let mut arc_buf = vec![0u8; arc_req];
            let mut mis_buf = vec![0u8; mis_req];
            let rc = unsafe {
                crimson_main_quest_table_get_entry(
                    idx,
                    chap_buf.as_mut_ptr(),
                    chap_buf.len(),
                    &mut chap_req,
                    arc_buf.as_mut_ptr(),
                    arc_buf.len(),
                    &mut arc_req,
                    mis_buf.as_mut_ptr(),
                    mis_buf.len(),
                    &mut mis_req,
                )
            };
            assert_eq!(rc, error::OK);

            let chap = std::str::from_utf8(&chap_buf[..chap_req - 1]).unwrap();
            let arc = std::str::from_utf8(&arc_buf[..arc_req - 1]).unwrap();
            let mis = std::str::from_utf8(&mis_buf[..mis_req - 1]).unwrap();

            let row = &ROWS[idx as usize];
            assert_eq!(chap, row.0);
            assert_eq!(arc, row.1.unwrap_or(""));
            assert_eq!(mis, row.2);
        }

        // Out-of-range guard
        let (mut a, mut b, mut c) = (0usize, 0usize, 0usize);
        let rc = unsafe {
            crimson_main_quest_table_get_entry(
                count,
                ptr::null_mut(),
                0,
                &mut a,
                ptr::null_mut(),
                0,
                &mut b,
                ptr::null_mut(),
                0,
                &mut c,
            )
        };
        assert_eq!(rc, error::OUT_OF_RANGE);
    }

    #[test]
    fn null_args() {
        // entry_count: null out
        assert_eq!(
            unsafe { crimson_main_quest_table_entry_count(ptr::null_mut()) },
            error::NULL_ARG
        );
        // get_entry: null required
        assert_eq!(
            unsafe {
                crimson_main_quest_table_get_entry(
                    0,
                    ptr::null_mut(),
                    0,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    0,
                    &mut 0usize,
                    ptr::null_mut(),
                    0,
                    &mut 0usize,
                )
            },
            error::NULL_ARG
        );
        // Lookups: null key
        let mut req: usize = 0;
        assert_eq!(
            unsafe {
                crimson_main_quest_chapter_for_arc(
                    ptr::null(),
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            },
            error::NULL_ARG
        );
        assert_eq!(
            unsafe {
                crimson_main_quest_chapter_for_mission(
                    ptr::null(),
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            },
            error::NULL_ARG
        );
        assert_eq!(
            unsafe {
                crimson_main_quest_arc_for_mission(
                    ptr::null(),
                    ptr::null_mut(),
                    0,
                    &mut req,
                )
            },
            error::NULL_ARG
        );
        // Lookups: null required
        let key = CString::new("Trials of Kindness").unwrap();
        assert_eq!(
            unsafe {
                crimson_main_quest_chapter_for_arc(
                    key.as_ptr(),
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
        // Lookups: undersized buf returns BUFFER_TOO_SMALL with the
        // proper `required` size, leaving the caller a chance to
        // resize and retry.
        let key = CString::new("Trials of Kindness").unwrap();
        let mut tiny = [0u8; 4];
        let mut req: usize = 0;
        let rc = unsafe {
            crimson_main_quest_chapter_for_arc(
                key.as_ptr(),
                tiny.as_mut_ptr(),
                tiny.len(),
                &mut req,
            )
        };
        assert_eq!(rc, error::BUFFER_TOO_SMALL);
        assert_eq!(req, "Chapter 1: The First Encounter".len() + 1);
    }
}
