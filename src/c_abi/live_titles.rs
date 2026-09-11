//! Live display titles for the curated-table drift tests (test-only).
//!
//! Resolves a `MissionKey` / `QuestKey` to its current English display title
//! through the chain the bridges use — key → internal name → `hashlittle2` →
//! `(hash << 32) | lo32` → PALOC — read straight from the live install.
//! [`LiveTitles::load`] returns `None` when the install is absent, so the
//! tests built on it skip cleanly on CI — and panics when the install is
//! there but something fails to load, so a broken pipeline cannot pass as a
//! skip.

use std::collections::HashMap;

use crate::binary::gamedata_layout;
use crate::binary::paloc::LocalizationFile;
use crate::crypto::checksum::calculate_checksum;

pub(crate) struct LiveTitles {
    paloc: HashMap<String, String>,
    missions: HashMap<u32, String>,
    quests: HashMap<u32, String>,
}

impl LiveTitles {
    /// `None` when there is no install; panics when there is one but it
    /// does not load.
    pub(crate) fn load() -> Option<Self> {
        if !gamedata_layout::game_root().join("0008").join("0.pamt").is_file() {
            return None;
        }
        Some(Self::load_present().expect(
            "live install present, but missioninfo / questinfo / the eng PALOC failed to load",
        ))
    }

    fn load_present() -> Option<Self> {
        let body = |stem: &str| gamedata_layout::extract_bin(&gamedata_layout::body(stem));
        let header = |stem: &str| gamedata_layout::extract_bin(&gamedata_layout::header(stem));
        let missions = crate::mission_info::parse_mission_info_lossy(&body("missioninfo")?)
            .into_iter()
            .map(|e| (e.key, e.name))
            .collect();
        let quests =
            crate::quest_info::parse_quest_info_via_pabgh(&header("questinfo")?, &body("questinfo")?)
                .into_iter()
                .map(|e| (e.key, e.name))
                .collect();
        let paloc_bytes = gamedata_layout::paloc_bytes("0020", "eng")?;
        let paloc = LocalizationFile::parse(&paloc_bytes)
            .ok()?
            .entries
            .iter()
            .map(|e| {
                (
                    String::from_utf8_lossy(e.string_key.as_bytes()).into_owned(),
                    String::from_utf8_lossy(e.string_value.as_bytes()).into_owned(),
                )
            })
            .collect();
        Some(Self { paloc, missions, quests })
    }

    fn title(&self, name: &str, lo32: u64) -> Option<&str> {
        let key = (u64::from(calculate_checksum(name.as_bytes())) << 32) | lo32;
        self.paloc.get(&key.to_string()).map(String::as_str)
    }

    /// Live title of a `missioninfo` row (PALOC `lo32 = 0x101`).
    pub(crate) fn mission(&self, key: u32) -> Option<&str> {
        self.title(self.missions.get(&key)?, 0x101)
    }

    /// Live title of a `questinfo` row (PALOC `lo32 = 0x100`).
    pub(crate) fn quest(&self, key: u32) -> Option<&str> {
        self.title(self.quests.get(&key)?, 0x100)
    }
}
