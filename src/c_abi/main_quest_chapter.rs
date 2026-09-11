//! Main-quest chapter rollup — C ABI surface.
//!
//! Static curated `(chapter, arc, mission)` table sourced from
//! [`docs/ref-gamedata/main-quest-list.md`](../../docs/ref-gamedata/main-quest-list.md). Quest
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
//! ## Keys
//!
//! Every row also records the game row its title comes from — a
//! `MissionKey`, or for the Prologue's opening "Ambush" the `QuestKey` of
//! `Quest_Intro` — and every arc its `QuestKey` ("Cradle of Defense" is the
//! one arc heading that is a mission title). Those are the keys a save
//! stores, so the key lookups below keep answering when a patch retitles a
//! mission, where a title lookup would silently return `NOT_FOUND`. The
//! titles were last reconciled against 2.02, when 58 of them had drifted
//! from the live strings and were re-paired with their game rows (the
//! source MD's "Reconciliation against 2.02" lists each one with its
//! evidence); `curated_titles_match_live_install` fails the next time a
//! key's live title stops matching. Five wiki-only titles with no live
//! counterpart are kept as [`Entry::Unresolved`]: the title lookups still
//! answer for them, the key lookups cannot.
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
//!   disambiguation should use the key lookups, which are exact.
//! - [`crimson_main_quest_arc_for_mission`] — mission title → arc
//!   title. Prologue missions return an empty string (the Prologue has
//!   no arcs).
//! - [`crimson_main_quest_chapter_for_mission_key`] /
//!   [`crimson_main_quest_arc_for_mission_key`] — `MissionKey` → chapter
//!   heading / arc title. Keys are unique, so the repeated titles resolve
//!   exactly ("In Ashes" is 1000160 in the Prologue, 1000783 in Chapter 6).
//! - [`crimson_main_quest_chapter_for_quest_key`] — `QuestKey` of an arc,
//!   or of a quest-kind entry, → chapter heading.
//! - [`crimson_main_quest_table_entry_count`] +
//!   [`crimson_main_quest_table_get_entry`] (strings) /
//!   [`crimson_main_quest_table_get_entry_keys`] (keys) — full
//!   enumeration.
//!
//! Stateless: backing data is a `const` table; lookup indices are
//! lazily built on first call via `OnceLock`. No load / free pair.

use std::collections::HashMap;
use std::os::raw::{c_char, c_int};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::OnceLock;

use super::error;

/// Where a curated title lives in the game data. The key is what a save
/// actually stores, so the key lookups keep working when a patch retitles
/// a mission — only the title lookups depend on the display string.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Entry {
    /// A `missioninfo.pabgb` row; its display title sits at PALOC
    /// `lo32 = 0x101`.
    Mission(u32),
    /// A `questinfo.pabgb` row; its display title sits at PALOC
    /// `lo32 = 0x100`.
    Quest(u32),
    /// No counterpart in the current patch: a title from the original
    /// wiki transcription that matches no live mission, quest or stage
    /// title. Kept so the curated structure stays intact; key lookups
    /// never return it.
    Unresolved,
}

impl Entry {
    /// Kind code the `*_get_entry_key(s)` ABI reports: `0` unresolved,
    /// `1` mission, `2` quest.
    pub(crate) const fn kind_code(self) -> u32 {
        match self {
            Entry::Unresolved => 0,
            Entry::Mission(_) => 1,
            Entry::Quest(_) => 2,
        }
    }

    /// The game key; `0` for [`Entry::Unresolved`].
    pub(crate) const fn key(self) -> u32 {
        match self {
            Entry::Mission(k) | Entry::Quest(k) => k,
            Entry::Unresolved => 0,
        }
    }
}

use Entry::{Mission, Quest, Unresolved};

/// A curated arc heading and the game row its title comes from.
#[derive(Clone, Copy)]
struct Arc {
    title: &'static str,
    /// A [`Quest`] for every arc except "Cradle of Defense", whose heading
    /// is a mission title (`Mission_Silver_Armor_Boss_Occupation_Refinery`)
    /// rather than a quest.
    entry: Entry,
}

/// One curated row. `arc` is `None` for Prologue entries (the Prologue
/// has no arc layer).
struct Row {
    chapter: &'static str,
    arc: Option<Arc>,
    mission: &'static str,
    entry: Entry,
}

const fn arc(title: &'static str, entry: Entry) -> Option<Arc> {
    Some(Arc { title, entry })
}

const fn row(chapter: &'static str, arc: Option<Arc>, mission: &'static str, entry: Entry) -> Row {
    Row { chapter, arc, mission, entry }
}

// ── Chapter headings (curated) ───────────────────────────────────────────

const PROLOGUE: &str = "Prologue: Dead of Night";
const CH1: &str = "Chapter 1: The First Encounter";
const CH2: &str = "Chapter 2: Golden Greed";
const CH3: &str = "Chapter 3: Howling Hill";
const CH4: &str = "Chapter 4: The Price of Knowledge";
const CH5: &str = "Chapter 5: Guest Unbidden";
const CH6: &str = "Chapter 6: Cracks in the Shield";
const CH7: &str = "Chapter 7: Homecoming";
const CH8: &str = "Chapter 8: Blood Coronation";
const CH9: &str = "Chapter 9: The Sage of the Desert";
const CH10: &str = "Chapter 10: Counterattack";
const CH11: &str = "Chapter 11: Truth and Reality";
const CH12: &str = "Chapter 12: The Abyss";
const EPILOGUE: &str = "Epilogue: Journey's End";

// ── Arc headings, each tied to the game row its title comes from ─────────

const TRIALS_OF_KINDNESS: Option<Arc> = arc("Trials of Kindness", Quest(1_000_027));
// was "Trace"
const TRACES: Option<Arc> = arc("Traces", Quest(1_000_509));
const UNEXPECTED_GIFT: Option<Arc> = arc("Unexpected Gift", Quest(1_000_004));
const HERNAND_IN_CHAOS: Option<Arc> = arc("Hernand in Chaos", Quest(10_011_000));
const THE_END_OF_GREED: Option<Arc> = arc("The End of Greed", Quest(1_000_267));
const HOMESTEAD: Option<Arc> = arc("Homestead", Quest(11_006_000));
const THE_FACE_BEHIND_THE_MASK: Option<Arc> = arc("The Face Behind the Mask", Quest(10_029_000));
const PIONEERING: Option<Arc> = arc("Pioneering", Quest(1_001_048));
// was "Mysterious Iron Pot"
const MYSTERIOUS_POT: Option<Arc> = arc("Mysterious Pot", Quest(1_000_142));
const DAILY_LIFE: Option<Arc> = arc("Daily Life", Quest(1_001_216));
const FORBIDDEN_KNOWLEDGE: Option<Arc> = arc("Forbidden Knowledge", Quest(10_040_400));
const UNINVITED_GUEST: Option<Arc> = arc("Uninvited Guest", Quest(1_000_118));
const BLACK_AND_WHITE: Option<Arc> = arc("Black and White", Quest(1_000_724));
const BLAZING_BEACON: Option<Arc> = arc("Blazing Beacon", Quest(10_060_100));
// was "Under the Banner"
const BELOW_THE_BANNERS: Option<Arc> = arc("Below the Banners", Quest(1_000_781));
const CRADLE_OF_DEFENSE: Option<Arc> = arc("Cradle of Defense", Mission(1_001_231));
const TURNING_TIDES: Option<Arc> = arc("Turning Tides", Quest(1_000_281));
// was "The Undying Shields"
const THE_UNYIELDING_SHIELDS: Option<Arc> = arc("The Unyielding Shields", Quest(1_000_112));
// was "Dawn Mist"
const MORNING_MIST: Option<Arc> = arc("Morning Mist", Quest(1_000_579));
// was "Dawnrise"
const DAWN: Option<Arc> = arc("Dawn", Quest(1_000_597));
const DECISIVE_BATTLE: Option<Arc> = arc("Decisive Battle", Quest(1_000_099));
const TWISTED_FATE: Option<Arc> = arc("Twisted Fate", Quest(1_000_009));
const ASHEN_STEPS: Option<Arc> = arc("Ashen Steps", Quest(1_001_076));
// was "To Demeniss"
const DEMENISS_BOUND: Option<Arc> = arc("Demeniss Bound", Quest(1_000_725));
const TRAITOR: Option<Arc> = arc("Traitor", Quest(1_000_180));
const THE_CALLING: Option<Arc> = arc("The Calling", Quest(1_000_354));
const SHATTERED_TIES: Option<Arc> = arc("Shattered Ties", Quest(1_000_212));
const THINNING_BLADE: Option<Arc> = arc("Thinning Blade", Quest(1_000_292));
// was "Six Pensive Statues and the Evil Spirit"
const SIX_STATUES_AND_THE_BEAST: Option<Arc> = arc("Six Statues and the Beast", Quest(1_000_305));
const VEILED_WITCH: Option<Arc> = arc("Veiled Witch", Quest(1_000_319));
const ENLIGHTENMENT: Option<Arc> = arc("Enlightenment", Quest(1_000_355));
const SECRET_WEAPON: Option<Arc> = arc("Secret Weapon", Quest(1_000_767));
const GREATER_FIREPOWER: Option<Arc> = arc("Greater Firepower", Quest(1_000_776));
const BRAVE_NEW_WORLD: Option<Arc> = arc("Brave New World", Quest(1_000_741));
const FOREBODING_SHADOW: Option<Arc> = arc("Foreboding Shadow", Quest(1_000_135));
const THE_FINAL_BATTLE: Option<Arc> = arc("The Final Battle", Quest(1_000_139));
const THE_VOID: Option<Arc> = arc("The Void", Quest(1_000_726));
const JOURNEYS_END: Option<Arc> = arc("Journey's End", Quest(1_000_358));

/// The curated table in story order. Titles are the live English display
/// strings (last reconciled against Crimson Desert 2.02 — see the
/// "Reconciliation against 2.02" section of the source MD for every change
/// and its evidence); `curated_titles_match_live_install` fails as soon as
/// a key's live title stops matching its row.
const ROWS: &[Row] = &[
    // ── Prologue: Dead of Night ────────────────────────────────────────
    row(PROLOGUE, None, "Ambush", Quest(10_001)),
    // 2.01 retitle (was "Unfamiliar Lands").
    row(PROLOGUE, None, "Unfamiliar Land", Mission(1_000_157)),
    row(PROLOGUE, None, "In Ashes", Mission(1_000_160)),
    // no counterpart in 2.02 — kept from the original transcription
    row(PROLOGUE, None, "Unknown Space", Unresolved),
    row(PROLOGUE, None, "Realm of Uncertainty", Mission(1_000_620)),
    row(PROLOGUE, None, "New Journey", Mission(1_000_164)),
    // ── Chapter 1: The First Encounter ─────────────────────────────────
    row(CH1, TRIALS_OF_KINDNESS, "Where Rumors Gather", Mission(1_000_052)),
    // 2.01 retitle (was "Mysterious Man").
    row(CH1, TRIALS_OF_KINDNESS, "A Mysterious Beggar", Mission(1_000_053)),
    row(CH1, TRIALS_OF_KINDNESS, "True Wisdom in Kindness", Mission(1_000_294)),
    // was "Actions Speak Louder than Words"
    row(CH1, TRIALS_OF_KINDNESS, "Actions Speak Volumes", Mission(1_000_042)),
    // was "Heart Beyond Borders" (inferred)
    row(CH1, TRIALS_OF_KINDNESS, "A Transcendent Connection", Mission(1_000_051)),
    row(CH1, TRACES, "Mystical Key", Mission(1_000_043)),
    // was "Polar Opposites"
    row(CH1, TRACES, "Faced With the Truth", Mission(1_000_048)),
    // was "Abyss Without Balance"
    row(CH1, TRACES, "The Faltering Abyss", Mission(1_000_546)),
    row(CH1, TRACES, "Woman in White", Mission(1_000_166)),
    // ── Chapter 2: Golden Greed ────────────────────────────────────────
    row(CH2, UNEXPECTED_GIFT, "Where the Light Leads", Mission(1_000_510)),
    row(CH2, UNEXPECTED_GIFT, "Memory Fragment", Mission(1_001_705)),
    row(CH2, UNEXPECTED_GIFT, "Reunion", Mission(1_000_507)),
    row(CH2, HERNAND_IN_CHAOS, "For Honor", Mission(1_000_177)),
    row(CH2, HERNAND_IN_CHAOS, "Awestruck", Mission(1_000_178)),
    // was "Shadow Cast Over the River"
    row(CH2, HERNAND_IN_CHAOS, "Shadow Over the River", Mission(1_000_529)),
    // was "Where Misery Gathers"
    row(CH2, HERNAND_IN_CHAOS, "A Collection of Woes", Mission(1_000_517)),
    row(CH2, HERNAND_IN_CHAOS, "Trial After Trial", Mission(1_000_179)),
    // was "The Man Trapped in the Mire"
    row(CH2, HERNAND_IN_CHAOS, "Down in the Muck", Mission(1_000_180)),
    row(CH2, HERNAND_IN_CHAOS, "Missing Companion", Mission(1_000_181)),
    // was "Secrets Hidden in the Dark"
    row(CH2, HERNAND_IN_CHAOS, "Shadowed Secrets", Mission(1_000_183)),
    row(CH2, THE_END_OF_GREED, "The Dark Veil", Mission(1_000_185)),
    row(CH2, THE_END_OF_GREED, "The Flames of Greed", Mission(1_000_187)),
    row(CH2, THE_END_OF_GREED, "Kidnapped Healer", Mission(1_000_190)),
    row(CH2, THE_END_OF_GREED, "Rebellion or Revolution", Mission(1_000_191)),
    // was "Cheers Echoing From the Edge"
    row(CH2, THE_END_OF_GREED, "Resounding Victory", Mission(1_000_193)),
    // ── Chapter 3: Howling Hill ────────────────────────────────────────
    row(CH3, HOMESTEAD, "Old Friend", Mission(1_000_215)),
    row(CH3, HOMESTEAD, "First Step to Rebuilding", Mission(1_000_216)),
    row(CH3, HOMESTEAD, "A Fresh Start", Mission(1_000_218)),
    // was "Reward for Their Sweat"
    row(CH3, HOMESTEAD, "A Well-Earned Meal", Mission(1_000_219)),
    // was "Return of the Comrade"
    row(CH3, HOMESTEAD, "Comrade's Return", Mission(1_000_220)),
    // no counterpart in 2.02 — kept from the original transcription
    row(CH3, HOMESTEAD, "Familiar Curses", Unresolved),
    // was "Return" (inferred)
    row(CH3, THE_FACE_BEHIND_THE_MASK, "Homecoming", Mission(1_000_504)),
    row(CH3, THE_FACE_BEHIND_THE_MASK, "Traces in the Manor", Mission(1_000_282)),
    // was "Nonhuman"
    row(CH3, THE_FACE_BEHIND_THE_MASK, "Inhuman", Mission(1_000_235)),
    // was "Seed of Unease"
    row(CH3, THE_FACE_BEHIND_THE_MASK, "Seed of Dread", Mission(1_001_417)),
    row(CH3, THE_FACE_BEHIND_THE_MASK, "Dance with the Devil", Mission(1_000_284)),
    // was "Hope After the Draught" (inferred)
    row(CH3, PIONEERING, "The Storm Passes", Mission(1_001_018)),
    row(CH3, PIONEERING, "Scattered Comrades", Mission(1_000_373)),
    row(CH3, PIONEERING, "Rumors from the Sawmill", Mission(1_000_660)),
    row(CH3, PIONEERING, "A Gentle Touch", Mission(1_000_116)),
    // was "Bustling Hill" (inferred)
    row(CH3, PIONEERING, "Commotion at Howling Hill", Mission(1_000_852)),
    row(CH3, PIONEERING, "Greymanes Reunited", Mission(1_001_206)),
    // ── Chapter 4: The Price of Knowledge ──────────────────────────────
    row(CH4, MYSTERIOUS_POT, "Kilnden Workshop", Mission(1_000_012)),
    // was "Kiln Repair at the Kilnden Workshop"
    row(CH4, MYSTERIOUS_POT, "Kilnden Kiln Repair", Mission(1_000_013)),
    row(CH4, MYSTERIOUS_POT, "The Mysterious Pot", Mission(1_000_019)),
    // was "The Iron Pot's Usage"
    row(CH4, MYSTERIOUS_POT, "Pot Put to Good Use", Mission(1_000_225)),
    // was "Disturbance at the Arena"
    row(CH4, DAILY_LIFE, "Ruckus in the Arena", Mission(1_001_219)),
    row(CH4, DAILY_LIFE, "Skilled in Archery", Mission(1_001_389)),
    row(CH4, FORBIDDEN_KNOWLEDGE, "The Words of Alustin", Mission(1_000_281)),
    row(CH4, FORBIDDEN_KNOWLEDGE, "Scholastone", Mission(1_001_066)),
    row(CH4, FORBIDDEN_KNOWLEDGE, "On the Right Path", Mission(1_000_265)),
    row(CH4, FORBIDDEN_KNOWLEDGE, "Gate to the Otherworld", Mission(1_000_266)),
    row(CH4, FORBIDDEN_KNOWLEDGE, "Spire of the Stars", Mission(1_000_267)),
    row(CH4, FORBIDDEN_KNOWLEDGE, "Obsession and Madness", Mission(1_000_268)),
    // was "Casted Shadow"
    row(CH4, FORBIDDEN_KNOWLEDGE, "A Looming Shadow", Mission(1_000_269)),
    // ── Chapter 5: Guest Unbidden ──────────────────────────────────────
    // was "Double-sided Invitation"
    row(CH5, UNINVITED_GUEST, "Ulterior Motives", Mission(1_000_432)),
    row(CH5, UNINVITED_GUEST, "Unwelcomed Guests", Mission(1_000_240)),
    row(CH5, UNINVITED_GUEST, "Demenissian Delegation", Mission(1_000_242)),
    row(CH5, UNINVITED_GUEST, "Exposed Plot", Mission(1_000_243)),
    row(CH5, BLACK_AND_WHITE, "The Missing Seal", Mission(1_001_574)),
    row(CH5, BLACK_AND_WHITE, "Crowcaller", Mission(1_000_580)),
    row(CH5, BLACK_AND_WHITE, "The Crow's Warning", Mission(1_000_244)),
    // was "Bloodwind"
    row(CH5, BLACK_AND_WHITE, "Blood on the Wind", Mission(1_000_245)),
    // was "Secret at the Church"
    row(CH5, BLACK_AND_WHITE, "The Church's Hidden Secret", Mission(1_000_247)),
    // was "Toward the Nest (Spire of Soaring)"
    row(CH5, BLACK_AND_WHITE, "Approaching the Nest", Mission(1_000_248)),
    // ── Chapter 6: Cracks in the Shield ────────────────────────────────
    // was "News"
    row(CH6, BLAZING_BEACON, "News Arrives", Mission(1_000_137)),
    row(CH6, BLAZING_BEACON, "To the Battlefield", Mission(1_000_139)),
    row(CH6, BLAZING_BEACON, "The Counterattack", Mission(1_000_142)),
    // no counterpart in 2.02 — kept from the original transcription
    row(CH6, BELOW_THE_BANNERS, "Pike Again", Unresolved),
    // was "The Touch of Deliverance"
    row(CH6, CRADLE_OF_DEFENSE, "Hand of Deliverance", Mission(1_000_419)),
    // was "Fire on the Frontlines"
    row(CH6, CRADLE_OF_DEFENSE, "Fire on the Front Line", Mission(1_000_161)),
    row(CH6, TURNING_TIDES, "Fire Support", Mission(1_000_866)),
    row(CH6, TURNING_TIDES, "In Ashes", Mission(1_000_783)),
    row(CH6, TURNING_TIDES, "Hidden Fangs", Mission(1_000_784)),
    row(CH6, TURNING_TIDES, "Reclamation", Mission(1_001_397)),
    row(CH6, THE_UNYIELDING_SHIELDS, "A Thousand Troops", Mission(1_000_150)),
    row(CH6, THE_UNYIELDING_SHIELDS, "Traitor", Mission(1_000_151)),
    row(CH6, THE_UNYIELDING_SHIELDS, "All Quiet on the Front", Mission(1_000_152)),
    row(CH6, THE_UNYIELDING_SHIELDS, "News of Victory", Mission(1_001_398)),
    row(CH6, THE_UNYIELDING_SHIELDS, "Return Home", Mission(1_004_461)),
    // ── Chapter 7: Homecoming ──────────────────────────────────────────
    row(CH7, MORNING_MIST, "Ashes of Treachery", Mission(1_000_195)),
    row(CH7, MORNING_MIST, "Trust Lost", Mission(1_001_774)),
    // was "Bared Fang"
    row(CH7, MORNING_MIST, "Bared Fangs", Mission(1_001_692)),
    row(CH7, MORNING_MIST, "Rekindled Hope", Mission(1_000_163)),
    // was "Podium of Resolve"
    row(CH7, MORNING_MIST, "A Stand of Resolve", Mission(1_000_134)),
    row(CH7, DAWN, "Shadows Over Pailune", Mission(1_000_004)),
    row(CH7, DAWN, "Driving out the Shadows", Mission(1_000_207)),
    row(CH7, DAWN, "Lurking Wolves", Mission(1_000_208)),
    row(CH7, DAWN, "Reclamation", Mission(1_000_460)),
    row(CH7, DAWN, "Lonely Jackals", Mission(1_000_210)),
    row(CH7, DAWN, "Resolution", Mission(1_000_621)),
    row(CH7, DECISIVE_BATTLE, "The Counterattack", Mission(1_000_253)),
    row(CH7, DECISIVE_BATTLE, "Unleashed Fury", Mission(1_000_255)),
    row(CH7, DECISIVE_BATTLE, "The Final Bridge", Mission(1_000_256)),
    // no counterpart in 2.02 — kept from the original transcription
    row(CH7, DECISIVE_BATTLE, "Broken Claws", Unresolved),
    // was "Battle at Silverwolf Mountain"
    row(CH7, DECISIVE_BATTLE, "Battle at Silver Wolf Mountain", Mission(1_000_258)),
    // was "Incomplete Victory"
    row(CH7, DECISIVE_BATTLE, "Hollow Victory", Mission(1_000_259)),
    row(CH7, TWISTED_FATE, "Ludvig's Whereabouts", Mission(1_000_174)),
    row(CH7, TWISTED_FATE, "Time to Face Justice", Mission(1_000_400)),
    // ── Chapter 8: Blood Coronation ────────────────────────────────────
    // was "Healing Pailune" (inferred)
    row(CH8, ASHEN_STEPS, "The Unending Pursuit", Mission(1_000_092)),
    // was "A Bond" (inferred)
    row(CH8, ASHEN_STEPS, "Bonds", Mission(1_001_213)),
    row(CH8, ASHEN_STEPS, "Ritual Preparations", Mission(1_000_058)),
    row(CH8, ASHEN_STEPS, "Where the Wind Guides You", Mission(1_000_083)),
    row(CH8, DEMENISS_BOUND, "Chasing a Shadow", Mission(1_000_047)),
    row(CH8, DEMENISS_BOUND, "Blazing Fire", Mission(1_000_272)),
    // was "Whispering Shadows"
    row(CH8, DEMENISS_BOUND, "Murmurs in the Dark", Mission(1_000_212)),
    // was "Bloodied Invitation" (inferred)
    row(CH8, DEMENISS_BOUND, "Signed in Blood", Mission(1_000_388)),
    // was "Resolve Amidst a Storm"
    row(CH8, DEMENISS_BOUND, "Steadfast in the Storm", Mission(1_000_276)),
    // was "Preparations for Advance"
    row(CH8, DEMENISS_BOUND, "Preparing to Strike", Mission(1_000_135)),
    // was "Rebel Suppression"
    row(CH8, DEMENISS_BOUND, "Quelling the Uprising", Mission(1_001_390)),
    row(CH8, DEMENISS_BOUND, "The Cursed Knight", Mission(1_001_215)),
    row(CH8, DEMENISS_BOUND, "The Blood Coronation", Mission(1_000_283)),
    // was "Clue" (inferred)
    row(CH8, TRAITOR, "The Thread", Mission(1_000_271)),
    row(CH8, TRAITOR, "A Fleeting Dream", Mission(1_000_287)),
    // ── Chapter 9: The Sage of the Desert ──────────────────────────────
    row(CH9, THE_CALLING, "An Unknown Voice", Mission(1_000_060)),
    row(CH9, THE_CALLING, "Cloister of Enlightenment", Mission(1_002_627)),
    // was "Mark of the Scar"
    row(CH9, SHATTERED_TIES, "The Spear's Mark", Mission(1_000_159)),
    row(CH9, SHATTERED_TIES, "Shackles of Fate", Mission(1_002_821)),
    // was "Crossing Point"
    row(CH9, THINNING_BLADE, "The Crossroads", Mission(1_000_162)),
    row(CH9, THINNING_BLADE, "Unwavering Steps", Mission(1_002_833)),
    // was "Morning Fog"
    row(CH9, SIX_STATUES_AND_THE_BEAST, "The Shroud of Dawn", Mission(1_000_158)),
    row(CH9, SIX_STATUES_AND_THE_BEAST, "Jijeong Temple in Chaos", Mission(1_000_285)),
    row(CH9, SIX_STATUES_AND_THE_BEAST, "Path to Enlightenment", Mission(1_000_296)),
    row(CH9, SIX_STATUES_AND_THE_BEAST, "Path of the Disciple", Mission(1_000_297)),
    row(CH9, SIX_STATUES_AND_THE_BEAST, "True Strength", Mission(1_000_117)),
    // was "Face the Inner Self"
    row(CH9, SIX_STATUES_AND_THE_BEAST, "Confronting What Lies Within", Mission(1_002_834)),
    row(CH9, VEILED_WITCH, "Fragments of Darkness", Mission(1_000_683)),
    row(CH9, VEILED_WITCH, "Pursuit Beyond the Veil", Mission(1_001_224)),
    row(CH9, VEILED_WITCH, "Black Witch", Mission(1_000_026)),
    row(CH9, ENLIGHTENMENT, "The Cloister of Enlightenment", Mission(1_000_250)),
    row(CH9, ENLIGHTENMENT, "The Sage of the Desert", Mission(1_000_251)),
    row(CH9, ENLIGHTENMENT, "New Perspectives", Mission(1_000_252)),
    // no counterpart in 2.02 — kept from the original transcription
    row(CH9, ENLIGHTENMENT, "Lust for Power", Unresolved),
    // ── Chapter 10: Counterattack ──────────────────────────────────────
    // was "Untouchable" (inferred)
    row(CH10, SECRET_WEAPON, "A New Front", Mission(1_000_223)),
    row(CH10, SECRET_WEAPON, "The Gate of War", Mission(1_000_224)),
    row(CH10, SECRET_WEAPON, "Master of the Ironworks", Mission(1_000_411)),
    row(CH10, SECRET_WEAPON, "Hidden Ace", Mission(1_000_229)),
    row(CH10, SECRET_WEAPON, "Clockwork Insect Clash", Mission(1_001_404)),
    row(CH10, GREATER_FIREPOWER, "Beating Heart", Mission(1_000_418)),
    row(CH10, GREATER_FIREPOWER, "Invaders from the East", Mission(1_000_069)),
    // was "Frozen Hearted Predator"
    row(CH10, GREATER_FIREPOWER, "Cold-Hearted Hunter", Mission(1_000_232)),
    row(CH10, GREATER_FIREPOWER, "Lingering Shadow", Mission(1_000_233)),
    // ── Chapter 11: Truth and Reality ──────────────────────────────────
    row(CH11, BRAVE_NEW_WORLD, "The City of Steel", Mission(1_000_107)),
    // was "Crossroads"
    row(CH11, BRAVE_NEW_WORLD, "At a Crossroads", Mission(1_000_616)),
    row(CH11, BRAVE_NEW_WORLD, "Strange Manor", Mission(1_000_434)),
    row(CH11, BRAVE_NEW_WORLD, "Fortress Keys", Mission(1_000_110)),
    row(CH11, BRAVE_NEW_WORLD, "Truth and Lies", Mission(1_000_184)),
    row(CH11, FOREBODING_SHADOW, "Master of a Forgotten Land", Mission(1_000_111)),
    row(CH11, FOREBODING_SHADOW, "Whispers in the Wind", Mission(1_000_113)),
    // was "Cloud Fortress Orbian"
    row(CH11, FOREBODING_SHADOW, "Flying Fortress Orbian", Mission(1_000_114)),
    // ── Chapter 12: The Abyss ──────────────────────────────────────────
    row(CH12, THE_FINAL_BATTLE, "Precise Execution", Mission(1_001_226)),
    row(CH12, THE_FINAL_BATTLE, "Deferred Advance", Mission(1_001_227)),
    row(CH12, THE_FINAL_BATTLE, "Departure of the Brave", Mission(1_000_046)),
    row(CH12, THE_FINAL_BATTLE, "Forbidden Gate", Mission(1_000_041)),
    row(CH12, THE_VOID, "A Shadow in the Void", Mission(1_000_106)),
    row(CH12, THE_VOID, "Blinding Darkness", Mission(1_000_108)),
    // ── Epilogue: Journey's End ────────────────────────────────────────
    row(EPILOGUE, JOURNEYS_END, "A New Beginning", Mission(1_000_112)),
    // was "Peace in Hernand"
    row(EPILOGUE, JOURNEYS_END, "Peace Restored", Mission(1_000_531)),
    // was "The Unyielding Shields"
    row(EPILOGUE, JOURNEYS_END, "The Unyielding Shield", Mission(1_000_532)),
    row(EPILOGUE, JOURNEYS_END, "The Heart of Pywel", Mission(1_000_540)),
    row(EPILOGUE, JOURNEYS_END, "The Enduring Flame", Mission(1_000_536)),
    // was "Evolving City"
    row(EPILOGUE, JOURNEYS_END, "The Evolving City", Mission(1_001_690)),
    row(EPILOGUE, JOURNEYS_END, "The Desert's Edge", Mission(1_000_534)),
    row(EPILOGUE, JOURNEYS_END, "New Horizons", Mission(1_000_329)),
];

/// Lookup index: arc title → first row index in [`ROWS`].
fn arc_index() -> &'static HashMap<&'static str, usize> {
    static IDX: OnceLock<HashMap<&'static str, usize>> = OnceLock::new();
    IDX.get_or_init(|| {
        let mut m: HashMap<&'static str, usize> = HashMap::new();
        for (i, row) in ROWS.iter().enumerate() {
            if let Some(arc) = row.arc {
                m.entry(arc.title).or_insert(i);
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
            m.entry(row.mission).or_insert(i);
        }
        m
    })
}

/// Lookup index: `MissionKey` → row index. Keys are unique across the
/// table (asserted by `entry_keys_are_unique`).
fn mission_key_index() -> &'static HashMap<u32, usize> {
    static IDX: OnceLock<HashMap<u32, usize>> = OnceLock::new();
    IDX.get_or_init(|| {
        let mut m: HashMap<u32, usize> = HashMap::with_capacity(ROWS.len());
        for (i, row) in ROWS.iter().enumerate() {
            if let Mission(k) = row.entry {
                m.entry(k).or_insert(i);
            }
        }
        m
    })
}

/// Lookup index: `QuestKey` → first row index, over arc quests and
/// quest-kind entries (the Prologue's "Ambush" is `Quest_Intro`).
fn quest_key_index() -> &'static HashMap<u32, usize> {
    static IDX: OnceLock<HashMap<u32, usize>> = OnceLock::new();
    IDX.get_or_init(|| {
        let mut m: HashMap<u32, usize> = HashMap::new();
        for (i, row) in ROWS.iter().enumerate() {
            if let Some(Arc { entry: Quest(k), .. }) = row.arc {
                m.entry(k).or_insert(i);
            }
            if let Quest(k) = row.entry {
                m.entry(k).or_insert(i);
            }
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
        let chapter = row.chapter;
        let arc = row.arc.map_or("", |a| a.title);
        let mission = row.mission;

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
        arc_index().get(key).map(|&i| ROWS[i].chapter)
    })
}

/// Resolve a mission display title (e.g. "Where Rumors Gather",
/// "Unfamiliar Land") to its chapter heading.
///
/// Three titles repeat across chapters ("In Ashes", "Reclamation",
/// "The Counterattack"); first match by table order wins. Callers that
/// hold the `MissionKey` should call
/// [`crimson_main_quest_chapter_for_mission_key`] instead — keys are
/// unique, and they do not depend on the display string.
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
        mission_index().get(key).map(|&i| ROWS[i].chapter)
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
        mission_index().get(key).map(|&i| ROWS[i].arc.map_or("", |a| a.title))
    })
}

/// Resolve a `MissionKey` — the save-side key — to its chapter heading.
///
/// Unlike [`crimson_main_quest_chapter_for_mission`] this does not go
/// through the display title, so it survives a retitle and is exact for
/// the titles that repeat. Returns [`error::NOT_FOUND`] when the key is
/// not in the curated set; otherwise fills `buf` per the standard
/// two-call pattern.
///
/// # Safety
/// `required` must be non-null. `buf` may be null iff `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_main_quest_chapter_for_mission_key(
    mission_key: u32,
    buf: *mut u8,
    buf_len: usize,
    required: *mut usize,
) -> c_int {
    lookup_key_to_string(buf, buf_len, required, || {
        mission_key_index().get(&mission_key).map(|&i| ROWS[i].chapter)
    })
}

/// Resolve a `MissionKey` to its arc title — the empty string for
/// Prologue missions, as with [`crimson_main_quest_arc_for_mission`].
/// Returns [`error::NOT_FOUND`] when the key is not in the curated set.
///
/// # Safety
/// `required` must be non-null. `buf` may be null iff `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_main_quest_arc_for_mission_key(
    mission_key: u32,
    buf: *mut u8,
    buf_len: usize,
    required: *mut usize,
) -> c_int {
    lookup_key_to_string(buf, buf_len, required, || {
        mission_key_index()
            .get(&mission_key)
            .map(|&i| ROWS[i].arc.map_or("", |a| a.title))
    })
}

/// Resolve a `QuestKey` to its chapter heading: the key of an arc's quest
/// (e.g. 1000027 `Quest_MeetAlustain_Test`, "Trials of Kindness") or of a
/// quest-kind entry (10001 `Quest_Intro`, the Prologue's "Ambush").
/// Returns [`error::NOT_FOUND`] when the key is not in the curated set.
///
/// # Safety
/// `required` must be non-null. `buf` may be null iff `buf_len == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_main_quest_chapter_for_quest_key(
    quest_key: u32,
    buf: *mut u8,
    buf_len: usize,
    required: *mut usize,
) -> c_int {
    lookup_key_to_string(buf, buf_len, required, || {
        quest_key_index().get(&quest_key).map(|&i| ROWS[i].chapter)
    })
}

/// Read the game keys of the row at `idx` — the companion of
/// [`crimson_main_quest_table_get_entry`], which returns its strings.
///
/// Kind codes: `0` none / unresolved, `1` `MissionKey`, `2` `QuestKey`.
/// `*out_arc_kind` / `*out_arc_key` describe the row's arc — `0` / `0` for
/// Prologue rows, and kind `1` only for "Cradle of Defense", whose heading
/// is a mission title. `*out_entry_kind` / `*out_entry_key` describe the
/// row itself — `0` / `0` for the five unresolved wiki titles.
///
/// Returns [`error::OUT_OF_RANGE`] if `idx >=
/// crimson_main_quest_table_entry_count`.
///
/// # Safety
/// All four out pointers must be non-null and writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn crimson_main_quest_table_get_entry_keys(
    idx: u32,
    out_arc_kind: *mut u32,
    out_arc_key: *mut u32,
    out_entry_kind: *mut u32,
    out_entry_key: *mut u32,
) -> c_int {
    if out_arc_kind.is_null()
        || out_arc_key.is_null()
        || out_entry_kind.is_null()
        || out_entry_key.is_null()
    {
        return error::NULL_ARG;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let Some(row) = ROWS.get(idx as usize) else {
            return error::OUT_OF_RANGE;
        };
        let arc = row.arc.map_or(Unresolved, |a| a.entry);
        unsafe {
            *out_arc_kind = arc.kind_code();
            *out_arc_key = arc.key();
            *out_entry_kind = row.entry.kind_code();
            *out_entry_key = row.entry.key();
        }
        error::OK
    }))
    .unwrap_or(error::PANIC)
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

fn lookup_key_to_string(
    buf: *mut u8,
    buf_len: usize,
    required: *mut usize,
    resolve: impl FnOnce() -> Option<&'static str>,
) -> c_int {
    if required.is_null() {
        return error::NULL_ARG;
    }
    if buf.is_null() && buf_len != 0 {
        return error::NULL_ARG;
    }
    unsafe { *required = 0 };
    catch_unwind(AssertUnwindSafe(|| {
        let Some(value) = resolve() else {
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
    //!    from `docs/ref-gamedata/main-quest-list.md` resolve in both directions.
    //! 3. Collision behaviour — the three repeated mission titles
    //!    resolve to the *first* declared chapter, and the arc lookup
    //!    surfaces the corresponding arc.
    //! 4. ABI hygiene — NULL args / OOR index / buffer too small /
    //!    NOT_FOUND for unknown strings.
    //! 5. Keys — unique per kind, the known unresolved set, the key
    //!    lookups, and `curated_titles_match_live_install`, which checks
    //!    every row's key against the live install's title. That one skips
    //!    cleanly without an install, so everything here runs on CI.
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
            assert!(!row.chapter.is_empty(), "row {i}: empty chapter");
            assert!(!row.mission.is_empty(), "row {i}: empty mission");
            if !row.chapter.starts_with("Prologue") {
                let arc = row.arc.unwrap_or_else(|| panic!("row {i}: non-Prologue with no arc"));
                assert!(!arc.title.is_empty(), "row {i}: empty arc string");
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
            call_chapter_for_mission("Unfamiliar Land").unwrap(),
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

        // "Traitor" appears as a mission in Ch6/The Unyielding Shields (the
        // arc lookup separately surfaces Ch8 — see chapter_for_arc_known_cases).
        assert_eq!(
            call_chapter_for_mission("Traitor").unwrap(),
            "Chapter 6: Cracks in the Shield",
        );
        assert_eq!(call_arc_for_mission("Traitor").unwrap(), "The Unyielding Shields");
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
            assert_eq!(chap, row.chapter);
            assert_eq!(arc, row.arc.map_or("", |a| a.title));
            assert_eq!(mis, row.mission);
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
        let mut seen = std::collections::HashSet::new();
        for (i, row) in ROWS.iter().enumerate() {
            if row.entry != Unresolved {
                let k = (row.entry.kind_code(), row.entry.key());
                assert!(seen.insert(k), "row {i}: {:?} repeats", row.entry);
            }
        }
    }

    #[test]
    fn unresolved_rows_are_the_known_five() {
        let unresolved: Vec<&str> =
            ROWS.iter().filter(|r| r.entry == Unresolved).map(|r| r.mission).collect();
        assert_eq!(
            unresolved,
            ["Unknown Space", "Familiar Curses", "Pike Again", "Broken Claws", "Lust for Power"]
        );
    }

    #[test]
    fn key_lookups_known_cases() {
        let ch = crimson_main_quest_chapter_for_mission_key;
        let arc = crimson_main_quest_arc_for_mission_key;
        // "Where Rumors Gather"
        assert_eq!(call_by_key(ch, 1_000_052).unwrap(), "Chapter 1: The First Encounter");
        assert_eq!(call_by_key(arc, 1_000_052).unwrap(), "Trials of Kindness");
        // "In Ashes" twice: the title lookup can only return the first,
        // the keys tell the Prologue mission from the Chapter 6 one.
        assert_eq!(call_by_key(ch, 1_000_160).unwrap(), "Prologue: Dead of Night");
        assert_eq!(call_by_key(arc, 1_000_160).unwrap(), "");
        assert_eq!(call_by_key(ch, 1_000_783).unwrap(), "Chapter 6: Cracks in the Shield");
        assert_eq!(call_by_key(arc, 1_000_783).unwrap(), "Turning Tides");
        // Re-paired in the 2.02 reconciliation ("Bared Fang" → "Bared Fangs").
        assert_eq!(call_by_key(arc, 1_001_692).unwrap(), "Morning Mist");
        // Quest keys: an arc's quest, and the Prologue's quest-kind entry.
        let q = crimson_main_quest_chapter_for_quest_key;
        assert_eq!(call_by_key(q, 1_000_027).unwrap(), "Chapter 1: The First Encounter");
        assert_eq!(call_by_key(q, 1_000_305).unwrap(), "Chapter 9: The Sage of the Desert");
        assert_eq!(call_by_key(q, 10_001).unwrap(), "Prologue: Dead of Night");
        // Kinds don't mix: 10_001 is a quest entry, not a mission.
        assert_eq!(call_by_key(ch, 10_001), Err(error::NOT_FOUND));
        assert_eq!(call_by_key(ch, 0), Err(error::NOT_FOUND));
        assert_eq!(call_by_key(q, 4_000_000_000), Err(error::NOT_FOUND));
    }

    #[test]
    fn entry_keys_round_trip() {
        let mut count: u32 = 0;
        assert_eq!(
            unsafe { crimson_main_quest_table_entry_count(&mut count) },
            error::OK
        );
        let (mut ak, mut a, mut ek, mut e) = (9u32, 9u32, 9u32, 9u32);
        for (idx, row) in ROWS.iter().enumerate() {
            let rc = unsafe {
                crimson_main_quest_table_get_entry_keys(idx as u32, &mut ak, &mut a, &mut ek, &mut e)
            };
            assert_eq!(rc, error::OK);
            let arc = row.arc.map_or(Unresolved, |x| x.entry);
            assert_eq!((ak, a), (arc.kind_code(), arc.key()), "row {idx} arc");
            assert_eq!((ek, e), (row.entry.kind_code(), row.entry.key()), "row {idx} entry");
            if row.chapter.starts_with("Prologue") {
                assert_eq!((ak, a), (0, 0), "row {idx}: Prologue rows have no arc");
            }
        }
        // "Cradle of Defense" is the one arc heading that is a mission title.
        let cradle = ROWS
            .iter()
            .position(|r| r.arc.is_some_and(|x| x.title == "Cradle of Defense"))
            .unwrap();
        let rc = unsafe {
            crimson_main_quest_table_get_entry_keys(cradle as u32, &mut ak, &mut a, &mut ek, &mut e)
        };
        assert_eq!(rc, error::OK);
        assert_eq!((ak, a), (1, 1_001_231));
        // Out of range / null out-pointer.
        let rc = unsafe {
            crimson_main_quest_table_get_entry_keys(count, &mut ak, &mut a, &mut ek, &mut e)
        };
        assert_eq!(rc, error::OUT_OF_RANGE);
        let rc = unsafe {
            crimson_main_quest_table_get_entry_keys(0, ptr::null_mut(), &mut a, &mut ek, &mut e)
        };
        assert_eq!(rc, error::NULL_ARG);
    }

    #[test]
    fn key_lookups_null_args() {
        let mut req: usize = 0;
        let rc = unsafe {
            crimson_main_quest_chapter_for_mission_key(1_000_052, ptr::null_mut(), 0, ptr::null_mut())
        };
        assert_eq!(rc, error::NULL_ARG);
        let rc = unsafe {
            crimson_main_quest_arc_for_mission_key(1_000_052, ptr::null_mut(), 4, &mut req)
        };
        assert_eq!(rc, error::NULL_ARG);
        let rc = unsafe {
            crimson_main_quest_chapter_for_quest_key(1_000_027, ptr::null_mut(), 0, ptr::null_mut())
        };
        assert_eq!(rc, error::NULL_ARG);
    }

    #[test]
    fn curated_titles_match_live_install() {
        let Some(live) = crate::c_abi::live_titles::LiveTitles::load() else {
            eprintln!("skipping curated_titles_match_live_install: no game install");
            return;
        };
        let title = |e: Entry| match e {
            Mission(k) => live.mission(k),
            Quest(k) => live.quest(k),
            Unresolved => None,
        };
        let mut drift = Vec::new();
        for (i, row) in ROWS.iter().enumerate() {
            if row.entry != Unresolved && title(row.entry) != Some(row.mission) {
                drift.push(format!(
                    "row {i} {:?}: live {:?}, table {:?}",
                    row.entry,
                    title(row.entry),
                    row.mission
                ));
            }
        }
        let mut arcs_seen = std::collections::HashSet::new();
        for arc in ROWS.iter().filter_map(|r| r.arc) {
            if arcs_seen.insert(arc.title) && title(arc.entry) != Some(arc.title) {
                drift.push(format!(
                    "arc {:?}: live {:?}, table {:?}",
                    arc.entry,
                    title(arc.entry),
                    arc.title
                ));
            }
        }
        assert!(
            drift.is_empty(),
            "curated main-quest titles drifted from the live install:\n{}",
            drift.join("\n")
        );
    }
}
