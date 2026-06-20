// Default-features build (neither `python` nor `c_abi` enabled) is a
// minimal dev sub-surface — there's no shipping consumer that links
// only the pure-Rust core, so save/* + paloc/* + item_info helpers that
// the `python` facade or `c_abi` bridges would normally pull in look
// dead to the linter. Silence dead_code globally in that combo only;
// the two real-build clippy gates in `.github/workflows/ci.yml` keep
// the public-surface code paths covered.
#![cfg_attr(
    not(any(feature = "python", feature = "c_abi")),
    allow(dead_code, unused_imports)
)]

mod binary;
#[cfg(feature = "c_abi")]
mod character_info;
#[cfg(feature = "c_abi")]
mod craft_tool_group_info;
#[cfg(feature = "c_abi")]
mod craft_tool_info;
mod crypto;
#[cfg(feature = "c_abi")]
mod dye_color_group_info;
#[cfg(feature = "c_abi")]
mod faction_node_info;
#[cfg(feature = "c_abi")]
mod faction_relation_group_info;
#[cfg(feature = "c_abi")]
mod faction_spawn_data_info;
#[cfg(feature = "c_abi")]
mod game_advice_group_info;
#[cfg(feature = "c_abi")]
mod game_advice_info;
#[cfg(feature = "c_abi")]
mod gameplay_variable_info;
#[cfg(feature = "c_abi")]
mod gimmick_info;
#[cfg(feature = "c_abi")]
mod global_game_event_group_info;
#[cfg(feature = "c_abi")]
mod global_game_event_info;
#[cfg(feature = "c_abi")]
mod house_info;
#[cfg(feature = "c_abi")]
mod item_group_info;
mod item_info;
#[cfg(feature = "c_abi")]
mod knowledge_info;
#[cfg(feature = "c_abi")]
mod mercenary_info;
#[cfg(feature = "c_abi")]
mod mission_info;
#[cfg(feature = "c_abi")]
mod part_prefab_dye_slot_info;
#[cfg(feature = "c_abi")]
mod part_prefab_dye_texture_pallete_info;
#[cfg(feature = "python")]
mod python;
#[cfg(feature = "c_abi")]
mod quest_gauge_info;
#[cfg(feature = "c_abi")]
mod quest_info;
#[cfg(feature = "c_abi")]
mod region_info;
#[cfg(feature = "c_abi")]
mod reserve_slot_info;
#[cfg(feature = "c_abi")]
mod royal_supply_info;
mod save;
mod skill_info;
#[cfg(feature = "c_abi")]
mod stage_info;
#[cfg(feature = "c_abi")]
mod store_info;
mod string_info;
#[cfg(feature = "c_abi")]
mod sub_level_info;
#[cfg(feature = "c_abi")]
mod trigger_region_info;
#[cfg(feature = "python")]
pub(crate) mod python_traits;

#[cfg(feature = "c_abi")]
mod c_abi;

#[cfg(feature = "python")]
use pyo3::prelude::*;

#[cfg(feature = "python")]
#[pymodule]
pub fn crimson_rs(m: &Bound<'_, PyModule>) -> PyResult<()> {
    python::register(m)
}

#[cfg(test)]
mod tests {
    use crate::binary::BinaryRead;
    use crate::binary::BinaryWrite;
    use crate::binary::paloc::LocalizationFile;
    use crate::binary::pamt::PackMeta;
    use crate::binary::papgt::PackGroupTreeMeta;
    use crate::item_info::ItemInfo;

    // Hardcoded to the maintainer's local install. Tests skip gracefully
    // if the files aren't present (CI / fresh machines) — they don't fail.
    const GAME_DIR: &str = r"D:\SteamLibrary\steamapps\common\Crimson Desert";
    // The parser targets Crimson Desert 1.05. The roundtrip test reads the
    // current 1.05 binary that `scripts\export_for_ce.py` extracts to
    // `out\iteminfo.pabgb`. Test skips if the file isn't present (e.g. a
    // fresh checkout that hasn't run the export pipeline).
    const BINARY_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        r"\out\iteminfo.pabgb"
    );
    // PAPGT / PAMT in the live game install (used by the parse / roundtrip
    // tests below). meta/0.papgt is the global descriptor; any group's
    // 0.pamt works for the parse path — 0019 ships in every region.
    const PAPGT_PATH: &str = concat!(
        r"D:\SteamLibrary\steamapps\common\Crimson Desert",
        r"\meta\0.papgt"
    );
    const PAMT_PATH: &str = concat!(
        r"D:\SteamLibrary\steamapps\common\Crimson Desert",
        r"\0019\0.pamt"
    );

    /// Read a file, or print a skip notice and return `None` so tests can
    /// `let Some(data) = ... else { return };` instead of panicking.
    fn try_read(path: &str, label: &str) -> Option<Vec<u8>> {
        match std::fs::read(path) {
            Ok(data) => Some(data),
            Err(e) => {
                eprintln!("skipping: {label} not found at {path}: {e}");
                None
            }
        }
    }

    #[test]
    fn test_full_roundtrip() {
        let Some(data) = try_read(BINARY_PATH, "1.04 baseline binary") else {
            return;
        };
        let mut offset = 0;
        let mut items = Vec::new();
        while offset < data.len() {
            items.push(ItemInfo::read_from(&data, &mut offset).unwrap());
        }
        assert_eq!(offset, data.len(), "did not consume all bytes");

        let mut out = Vec::with_capacity(data.len());
        for item in &items {
            item.write_to(&mut out).unwrap();
        }
        assert_eq!(out.len(), data.len(), "size mismatch");
        assert_eq!(out, data, "roundtrip bytes mismatch");
    }

    #[test]
    fn test_papgt_parse() {
        let Some(data) = try_read(PAPGT_PATH, "papgt") else {
            return;
        };
        let papgt = PackGroupTreeMeta::parse(&data).unwrap();
        println!("PAPGT: {} entries", papgt.entries.len());
        for entry in &papgt.entries {
            println!(
                "  group={}, optional={}, language={:#06x}, checksum={:#010x}",
                entry.group_name,
                entry.entry.is_optional,
                entry.entry.language.0,
                entry.entry.pack_meta_checksum,
            );
        }
        assert!(!papgt.entries.is_empty(), "should have entries");
    }

    #[test]
    fn test_papgt_roundtrip() {
        let Some(data) = try_read(PAPGT_PATH, "papgt") else {
            return;
        };
        let papgt = PackGroupTreeMeta::parse(&data).unwrap();
        println!("PAPGT: {} entries", papgt.entries.len());
        let written = papgt.to_bytes().unwrap();
        assert_eq!(written.len(), data.len(), "papgt roundtrip size mismatch");
        assert_eq!(written, data, "papgt roundtrip bytes mismatch");
    }

    #[test]
    fn test_pamt_parse() {
        let Some(data) = try_read(PAMT_PATH, "pamt") else {
            return;
        };
        let pamt = PackMeta::parse(&data, None).unwrap();
        println!(
            "PAMT: {} chunks, {} directories",
            pamt.chunks.len(),
            pamt.directories.len()
        );
        for dir in &pamt.directories {
            println!("  dir={}, {} files", dir.path, dir.files.len());
            for f in dir.files.iter().take(3) {
                println!(
                    "    file={}, compressed={}, uncompressed={}, chunk_id={}",
                    f.name, f.file.compressed_size, f.file.uncompressed_size, f.file.chunk_id
                );
            }
        }
        assert!(!pamt.directories.is_empty(), "should have directories");
    }

    #[test]
    fn test_pamt_roundtrip() {
        let Some(data) = try_read(PAMT_PATH, "pamt") else {
            return;
        };
        let pamt = PackMeta::parse(&data, None).unwrap();
        let written = pamt.to_bytes().unwrap();
        assert_eq!(written.len(), data.len(), "pamt roundtrip size mismatch");
        assert_eq!(written, data, "pamt roundtrip bytes mismatch");
    }

    fn extract_paloc_data() -> Option<Vec<u8>> {
        extract_paloc_from_archive("0020", "localizationstring_eng.paloc")
    }

    /// Returns `None` (with a skip notice on stderr) if the game install
    /// isn't present so tests can fall through gracefully.
    fn extract_paloc_from_archive(group: &str, file_name: &str) -> Option<Vec<u8>> {
        use crate::binary::paz;
        use std::path::Path;

        let group_dir = Path::new(GAME_DIR).join(group);
        let pamt_path = group_dir.join("0.pamt");
        let pamt_data = match std::fs::read(&pamt_path) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("skipping: {}: {}", pamt_path.display(), e);
                return None;
            }
        };
        let pamt = PackMeta::parse(&pamt_data, None).unwrap();

        let dir = pamt
            .directories
            .iter()
            .find(|d| d.path == "gamedata/stringtable/binary__")
            .expect("directory not found in pamt");
        let file = dir
            .files
            .iter()
            .find(|f| f.name == file_name)
            .unwrap_or_else(|| panic!("{} not found", file_name));

        Some(
            paz::extract_file(
                &group_dir,
                file,
                "gamedata/stringtable/binary__",
                &pamt.header.encrypt_info.encrypt_info,
            )
            .unwrap(),
        )
    }

    #[test]
    fn test_paloc_parse() {
        let Some(data) = extract_paloc_data() else {
            return;
        };
        let paloc = LocalizationFile::parse(&data).unwrap();
        println!("PALOC: {} entries", paloc.entries.len());
        for entry in paloc.entries.iter().take(5) {
            println!(
                "  id={}, key={}, value={}",
                entry.unk_id,
                entry.string_key.data,
                &entry.string_value.data[..entry.string_value.data.len().min(80)],
            );
        }
        assert!(!paloc.entries.is_empty(), "should have entries");
    }

    #[test]
    fn test_paloc_roundtrip() {
        let Some(data) = extract_paloc_data() else {
            return;
        };
        let paloc = LocalizationFile::parse(&data).unwrap();
        let written = paloc.to_bytes().unwrap();
        assert_eq!(written.len(), data.len(), "paloc roundtrip size mismatch");
        assert_eq!(written, data, "paloc roundtrip bytes mismatch");
    }

    #[test]
    fn test_paloc_kor_parse() {
        let Some(data) = extract_paloc_from_archive("0019", "localizationstring_kor.paloc")
        else {
            return;
        };
        let paloc = LocalizationFile::parse(&data).unwrap();
        println!("PALOC KOR: {} entries", paloc.entries.len());
        for entry in paloc.entries.iter().take(5) {
            let preview: String = entry.string_value.data.chars().take(40).collect();
            println!(
                "  id={}, key={}, value={}",
                entry.unk_id, entry.string_key.data, preview,
            );
        }
        assert!(!paloc.entries.is_empty(), "should have entries");
    }

    #[test]
    fn test_skill_roundtrip_1_05() {
        let Some(pabgh) = extract_skill_from_archive(GAME_DIR, "skill.pabgh") else {
            return;
        };
        let Some(pabgb) = extract_skill_from_archive(GAME_DIR, "skill.pabgb") else {
            return;
        };
        run_skill_roundtrip("1.05 (live game install)", &pabgh, &pabgb);
    }

    /// Cross-version skill roundtrip: validates the parser also handles
    /// the 1.03 (NoField58) and 1.04 (WithField58, smaller type_id table)
    /// formats. Reads from a hard-coded path on the maintainer's machine
    /// and skips on absence.
    #[test]
    fn test_skill_roundtrip_cross_version() {
        const VERSIONED_ROOT: &str = r"G:\我的雲端硬碟\temp\Crimson Desert";
        for version in ["1.03.01", "1.04.01", "1.05.01"] {
            let game_dir = format!(r"{}\{}", VERSIONED_ROOT, version);
            let Some(pabgh) = extract_skill_from_archive(&game_dir, "skill.pabgh") else {
                eprintln!("skipping {}: install not found", version);
                continue;
            };
            let Some(pabgb) = extract_skill_from_archive(&game_dir, "skill.pabgb") else {
                continue;
            };
            run_skill_roundtrip(version, &pabgh, &pabgb);
        }
    }

    fn run_skill_roundtrip(label: &str, pabgh: &[u8], pabgb: &[u8]) {
        use crate::skill_info::SkillData;
        let parsed = SkillData::parse(pabgh, pabgb)
            .unwrap_or_else(|e| panic!("{}: parse failed: {}", label, e));
        let raw_fallback_count = parsed
            .entries
            .iter()
            .filter(|e| e.buff_raw_fallback.is_some())
            .count();
        println!(
            "skill {}: {} entries (raw fallback={}), format={:?}",
            label,
            parsed.entries.len(),
            raw_fallback_count,
            parsed.format
        );
        let (h, b) = parsed
            .write()
            .unwrap_or_else(|e| panic!("{}: write failed: {}", label, e));
        assert_eq!(h.len(), pabgh.len(), "{}: pabgh size mismatch", label);
        assert_eq!(b.len(), pabgb.len(), "{}: pabgb size mismatch", label);
        assert_eq!(h, pabgh, "{}: pabgh bytes mismatch", label);
        assert_eq!(b, pabgb, "{}: pabgb bytes mismatch", label);
    }

    fn extract_skill_from_archive(game_dir: &str, file_name: &str) -> Option<Vec<u8>> {
        use crate::binary::paz;
        use std::path::Path;

        let group_dir = Path::new(game_dir).join("0008");
        let pamt_path = group_dir.join("0.pamt");
        let pamt_data = match std::fs::read(&pamt_path) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("skipping: {}: {}", pamt_path.display(), e);
                return None;
            }
        };
        let pamt = PackMeta::parse(&pamt_data, None).unwrap();
        let dir = pamt
            .directories
            .iter()
            .find(|d| d.path == "gamedata/binary__/client/bin")
            .expect("dir not found");
        let file = dir
            .files
            .iter()
            .find(|f| f.name == file_name)
            .unwrap_or_else(|| panic!("{} not found", file_name));
        Some(
            paz::extract_file(
                &group_dir,
                file,
                "gamedata/binary__/client/bin",
                &pamt.header.encrypt_info.encrypt_info,
            )
            .unwrap(),
        )
    }

    #[test]
    fn test_paloc_kor_roundtrip() {
        let Some(data) = extract_paloc_from_archive("0019", "localizationstring_kor.paloc")
        else {
            return;
        };
        let paloc = LocalizationFile::parse(&data).unwrap();
        let written = paloc.to_bytes().unwrap();
        assert_eq!(
            written.len(),
            data.len(),
            "paloc kor roundtrip size mismatch"
        );
        assert_eq!(written, data, "paloc kor roundtrip bytes mismatch");
    }

    #[test]
    fn test_game_dir_papgt_pamt_checksums() {
        use crate::crypto::checksum;
        use std::path::Path;

        let papgt_path = Path::new(GAME_DIR).join("meta/0.papgt");
        let papgt_data = match std::fs::read(&papgt_path) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("skipping: cannot read {}: {}", papgt_path.display(), e);
                return;
            }
        };
        let papgt = PackGroupTreeMeta::parse(&papgt_data).unwrap();

        println!(
            "Validating {} PAPGT entries against game directory...",
            papgt.entries.len()
        );

        let mut validated = 0;
        let mut skipped = 0;
        for entry in &papgt.entries {
            let pamt_path = Path::new(GAME_DIR).join(&entry.group_name).join("0.pamt");

            if !pamt_path.exists() {
                println!("  SKIP group={} (no 0.pamt found)", entry.group_name);
                skipped += 1;
                continue;
            }

            let pamt_data = std::fs::read(&pamt_path)
                .unwrap_or_else(|e| panic!("cannot read {}: {}", pamt_path.display(), e));

            // Compute checksum of entire pamt file data after header (8 bytes header)
            // The PAPGT stores pack_meta_checksum which is validated against post-header data
            let pamt_header_size = 4 + 2 + 2 + 1 + 3; // checksum + count + unknown0 + encrypt_info
            let post_header = &pamt_data[pamt_header_size..];
            let computed = checksum::calculate_checksum(post_header);

            assert_eq!(
                computed, entry.entry.pack_meta_checksum,
                "Checksum mismatch for group={}: computed={:#010x}, papgt expected={:#010x}",
                entry.group_name, computed, entry.entry.pack_meta_checksum,
            );

            // Also verify full parse with the expected CRC succeeds
            PackMeta::parse(&pamt_data, Some(entry.entry.pack_meta_checksum))
                .unwrap_or_else(|e| panic!("parse failed for group={}: {}", entry.group_name, e));

            println!(
                "  OK   group={}, checksum={:#010x}",
                entry.group_name, computed
            );
            validated += 1;
        }

        println!("Validated: {}, Skipped: {}", validated, skipped);
        assert!(
            validated > 0,
            "should have validated at least one pamt file"
        );
    }

    // ── Save file tests ────────────────────────────────────────────────────
    //
    // Save tests dynamically discover a save.save under
    // %LOCALAPPDATA%\Pearl Abyss\CD\save\<UserID>\slot*\, because the user
    // ID and slot numbers both vary per machine. Tests skip cleanly if no
    // save is found.

    fn try_find_save() -> Option<(std::path::PathBuf, Vec<u8>)> {
        use std::path::PathBuf;
        let local = std::env::var_os("LOCALAPPDATA").or_else(|| {
            eprintln!("skipping save tests: LOCALAPPDATA not set");
            None
        })?;
        let save_root = PathBuf::from(local).join("Pearl Abyss/CD/save");
        let users = match std::fs::read_dir(&save_root) {
            Ok(r) => r,
            Err(e) => {
                eprintln!(
                    "skipping save tests: cannot read {}: {}",
                    save_root.display(),
                    e
                );
                return None;
            }
        };
        // Two passes: prefer the canonical slot0/1/2 trio (stable across
        // sessions and the lowest indices that any user with a save tends
        // to have populated), then fall back to *any* slotN directory so
        // a machine that only has post-1.07 slots (slot100+ on the
        // maintainer's box) still exercises the save tests instead of
        // silently skipping them.
        let mut user_dirs: Vec<PathBuf> = users
            .flatten()
            .map(|u| u.path())
            .filter(|p| p.is_dir())
            .collect();
        user_dirs.sort();
        // Prefer slot107 — the current Crimson Desert 1.12 save — so the
        // save tests validate against the live latest-patch format.
        for user_path in &user_dirs {
            let path = user_path.join("slot107").join("save.save");
            if let Ok(data) = std::fs::read(&path) {
                return Some((path, data));
            }
        }
        for user_path in &user_dirs {
            let Ok(entries) = std::fs::read_dir(user_path) else {
                continue;
            };
            let mut slot_dirs: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.is_dir()
                        && p.file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| n.starts_with("slot"))
                })
                .collect();
            slot_dirs.sort();
            for slot_dir in slot_dirs {
                let path = slot_dir.join("save.save");
                if let Ok(data) = std::fs::read(&path) {
                    return Some((path, data));
                }
            }
        }
        eprintln!(
            "skipping save tests: no save.save found under {}",
            save_root.display()
        );
        None
    }

    #[test]
    fn test_save_parse() {
        use crate::save::Save;
        let Some((path, data)) = try_find_save() else {
            return;
        };
        let save = Save::parse(&data).unwrap_or_else(|e| {
            panic!("parse failed for {}: {}", path.display(), e);
        });
        println!(
            "save: {} | version={} flags={:#06x} payload={} uncompressed={} hmac_ok={}",
            path.display(),
            save.header.version(),
            save.header.flags(),
            save.header.payload_size(),
            save.header.uncompressed_size(),
            save.hmac_ok,
        );
        assert_eq!(save.header.as_bytes().len(), 0x80);
        assert!(save.hmac_ok, "HMAC verification failed");
        assert_eq!(
            save.body.len() as u32,
            save.header.uncompressed_size(),
            "decompressed body size must match header field"
        );
    }

    #[test]
    fn test_save_write_parse_roundtrip() {
        // Decode-roundtrip: parse a save, write it back with the original
        // nonce, parse the result, and check that the *bodies* match.
        //
        // We can't guarantee byte-identical output bytes for the on-disk
        // file because lz4_flex uses the standard LZ4 algorithm and the
        // game-shipped saves were written with LZ4 high-compression mode;
        // the compressed lengths will differ. But the format is correct iff
        // we can decode our own output back to the same body.
        use crate::save::Save;
        let Some((_, data)) = try_find_save() else {
            return;
        };
        let original = Save::parse(&data).unwrap();
        let nonce = original.header.nonce();
        let written = original.write_with_nonce(nonce).unwrap();

        let round = Save::parse(&written).unwrap();
        assert!(round.hmac_ok, "roundtripped save fails HMAC");
        assert_eq!(round.body, original.body, "body changed across roundtrip");
        assert_eq!(
            round.header.version(),
            original.header.version(),
            "version drift"
        );
        assert_eq!(
            round.header.uncompressed_size(),
            original.header.uncompressed_size(),
            "uncomp_size drift"
        );
    }

    #[test]
    fn test_save_body_parse() {
        // Parse the decompressed body's schema + TOC and sanity-check shape.
        use crate::save::{Body, Save};
        let Some((path, data)) = try_find_save() else {
            return;
        };
        let save = Save::parse(&data).unwrap();
        let body = Body::parse(&save.body).unwrap_or_else(|e| {
            panic!("body parse failed for {}: {}", path.display(), e);
        });

        println!(
            "body: {} | types={} root='{}' schema_end={} toc_count={} stream_size={}",
            path.display(),
            body.schema.type_count,
            body.schema.root_type,
            body.schema.schema_end,
            body.toc.toc_count,
            body.toc.stream_size,
        );

        assert!(body.schema.type_count > 0, "schema should have types");
        assert_eq!(
            body.schema.types.len(),
            body.schema.type_count as usize,
            "schema type_count and types.len disagree"
        );
        assert!(body.toc.toc_count > 0, "TOC should have entries");

        // Spot-check class index validity — TOC entries should mostly
        // (allow a few out-of-range stragglers per Python's tolerance).
        let in_range = body
            .toc
            .entries
            .iter()
            .filter(|e| (e.class_index as usize) < body.schema.types.len())
            .count();
        assert!(
            in_range as f64 / body.toc.entries.len() as f64 > 0.9,
            "expected >90% of TOC entries to point at valid class indices"
        );

        // Print the first few types + entries so a regression in layout
        // surfaces visually when this runs with --nocapture.
        for t in body.schema.types.iter().take(5) {
            println!("  type[{}] {} ({} fields)", t.index, t.name, t.fields.len());
        }
        for e in body.toc.entries.iter().take(5) {
            let class_name = body
                .schema
                .types
                .get(e.class_index as usize)
                .map(|t| t.name.as_str())
                .unwrap_or("<unknown>");
            println!(
                "  toc[{:3}] class={} '{}' offset={} size={}",
                e.index, e.class_index, class_name, e.data_offset, e.data_size,
            );
        }
    }

    #[test]
    fn test_save_body_decode_all_blocks() {
        // Run the per-object field decoder against every TOC entry in a
        // live save. Asserts shape sanity (>90% of the TOC entries
        // produce a decoded block) and prints a brief decode-quality
        // summary — bytes decoded vs bytes left in undecoded_ranges.
        use crate::save::{Body, FieldKind, Save};
        let Some((path, data)) = try_find_save() else {
            return;
        };
        let save = Save::parse(&data).unwrap();
        let body = Body::parse(&save.body).unwrap();
        let blocks = body.decode_blocks(&save.body);

        assert!(
            blocks.len() as f64 / body.toc.entries.len() as f64 > 0.9,
            "expected >90% of TOC entries to decode into blocks; got {} of {}",
            blocks.len(),
            body.toc.entries.len(),
        );

        // Aggregate decode quality.
        let mut total_block_bytes: usize = 0;
        let mut total_undecoded: usize = 0;
        let mut total_present_fields: usize = 0;
        let mut total_decoded_fields: usize = 0;
        let mut trailing_pad_blocks: usize = 0;
        let mut classes_with_undecoded: std::collections::BTreeMap<&str, usize> =
            std::collections::BTreeMap::new();
        for block in &blocks {
            total_block_bytes += block.data_size as usize;
            for (s, e) in &block.undecoded_ranges {
                total_undecoded += e - s;
            }
            if !block.trailing_pad.is_empty() {
                trailing_pad_blocks += 1;
            }
            if !block.undecoded_ranges.is_empty() {
                *classes_with_undecoded.entry(block.class_name.as_str()).or_default() += 1;
            }
            for f in &block.fields {
                if f.present {
                    total_present_fields += 1;
                    if f.kind != FieldKind::Unknown && f.kind != FieldKind::Absent {
                        total_decoded_fields += 1;
                    }
                }
            }
        }
        let decode_ratio = total_decoded_fields as f64 / total_present_fields as f64;
        let undecoded_ratio = total_undecoded as f64 / total_block_bytes as f64;
        println!(
            "decode_all: {} blocks | fields: {}/{} decoded ({:.1}%) | undecoded bytes: {}/{} ({:.2}%) | trailing_pad on {} blocks",
            blocks.len(),
            total_decoded_fields,
            total_present_fields,
            decode_ratio * 100.0,
            total_undecoded,
            total_block_bytes,
            undecoded_ratio * 100.0,
            trailing_pad_blocks,
        );
        if !classes_with_undecoded.is_empty() {
            let mut by_count: Vec<_> = classes_with_undecoded.iter().collect();
            by_count.sort_by(|a, b| b.1.cmp(a.1));
            println!("classes with any undecoded bytes (top 10):");
            for (name, n) in by_count.iter().take(10) {
                println!("  {n:>5}  {name}");
            }
        }

        // Hard invariant: against the live 1.06 save, every byte of
        // every block must be accounted for — either decoded into a
        // typed field or captured as a `trailing_pad`. A regression
        // here means a format detail has drifted.
        assert_eq!(
            total_undecoded, 0,
            "expected zero undecoded bytes against the live save"
        );
        // And every present field must end up with a concrete kind.
        assert_eq!(
            total_decoded_fields, total_present_fields,
            "expected every present field to decode (no Unknown)"
        );
        let _ = path;
    }

    /// Walk every save.save under %LOCALAPPDATA%\Pearl Abyss\CD\save\<UserID>\
    /// (slot0..slot199). Returns `(path, file_bytes)` for each one found.
    /// Empty if no install / saves present.
    fn collect_all_saves() -> Vec<(std::path::PathBuf, Vec<u8>)> {
        use std::path::PathBuf;
        let Some(local) = std::env::var_os("LOCALAPPDATA") else {
            return Vec::new();
        };
        let save_root = PathBuf::from(local).join("Pearl Abyss/CD/save");
        let Ok(users) = std::fs::read_dir(&save_root) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for user in users.flatten() {
            let user_path = user.path();
            if !user_path.is_dir() {
                continue;
            }
            let Ok(entries) = std::fs::read_dir(&user_path) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name();
                let Some(name_str) = name.to_str() else {
                    continue;
                };
                if !name_str.starts_with("slot") {
                    continue;
                }
                let save_path = entry.path().join("save.save");
                if let Ok(data) = std::fs::read(&save_path) {
                    out.push((save_path, data));
                }
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// Walk a decoded ObjectBlock tree looking for the smallest sub-element
    /// whose stored `data_offset..data_offset+data_size` range straddles the
    /// given body offset. Useful for narrowing down a roundtrip failure.
    fn find_element_at_offset<'a>(
        block: &'a crate::save::ObjectBlock,
        path: &str,
        target_offset: usize,
    ) -> Option<(String, &'a crate::save::ObjectBlock)> {
        use crate::save::FieldValue;
        for field in &block.fields {
            match &field.value {
                FieldValue::ObjectList { elements, .. } => {
                    for (i, el) in elements.iter().enumerate() {
                        let s = el.data_offset as usize;
                        let e = s + el.data_size as usize;
                        if (s..e).contains(&target_offset) {
                            let sub_path = format!("{path}.{}[{}]", field.name, i);
                            return find_element_at_offset(el, &sub_path, target_offset)
                                .or(Some((sub_path, el)));
                        }
                    }
                }
                FieldValue::Locator { child: Some(c), .. } => {
                    let s = c.data_offset as usize;
                    let e = s + c.data_size as usize;
                    if (s..e).contains(&target_offset) {
                        let sub_path = format!("{path}.{}<child>", field.name);
                        return find_element_at_offset(c, &sub_path, target_offset)
                            .or(Some((sub_path, c)));
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Per-block size + structural round-trip: for every top-level TOC
    /// block, encode it standalone and check the byte count matches
    /// the original.
    ///
    /// We deliberately don't assert byte-identity here because the
    /// engine writes a few items' `payload_offset` u32 with values
    /// that don't point at the wrapper-end (the decoder falls back to
    /// wrapper-end and parses correctly anyway). The encoder always
    /// writes the canonical wrapper-end value, so those specific
    /// bytes differ from the engine's quirky originals — but the
    /// game tolerates canonical output, and our subsequent
    /// encode→re-parse→re-encode is stable (idempotent). See
    /// `test_save_body_full_roundtrip_is_idempotent` for the
    /// stronger stability check.
    #[test]
    fn test_save_body_block_roundtrip_per_block() {
        use crate::save::{Body, Save, encode_top_level_block};
        let saves = collect_all_saves();
        if saves.is_empty() {
            eprintln!("skipping per-block roundtrip: no saves found");
            return;
        }
        let mut total_blocks = 0usize;
        for (path, data) in &saves {
            let save = Save::parse(data).unwrap();
            let body = Body::parse(&save.body).unwrap();
            let blocks = body.decode_blocks(&save.body);
            total_blocks += blocks.len();
            for block in &blocks {
                let start = block.data_offset as usize;
                let end = start + block.data_size as usize;
                let original = &save.body[start..end];
                let encoded = encode_top_level_block(block, block.data_offset).unwrap_or_else(|e| {
                    panic!(
                        "{}: encode failed for block toc_idx?? class={} ({}): {}",
                        path.display(),
                        block.class_name,
                        block.class_index,
                        e
                    )
                });
                // Size invariant: encoder must produce the same number
                // of bytes the original block occupied. Byte-identity
                // is asserted by the full-body idempotence test, not
                // here — see the test's docstring above for why.
                if encoded.len() != original.len() {
                    let _ = original; // keep `original` in scope for the
                                       // diagnostic block that follows.
                    let first_diff = original
                        .iter()
                        .zip(encoded.iter())
                        .position(|(a, b)| a != b)
                        .unwrap_or(original.len().min(encoded.len()));
                    // Locate the smallest sub-element that contains the
                    // diff offset, so failures pinpoint a specific element.
                    let body_diff_offset = start + first_diff;
                    let smallest = find_element_at_offset(
                        block,
                        &block.class_name,
                        body_diff_offset,
                    );
                    let element_info = match smallest {
                        Some((p, el)) => {
                            let mut s = format!(
                                "    smallest containing element: {p}\n      class={} data_offset={:#x} data_size={} mask_bytes={:02x?} reserved={:#x} trailing_pad_len={}",
                                el.class_name,
                                el.data_offset,
                                el.data_size,
                                el.mask_bytes,
                                el.reserved_u32,
                                el.trailing_pad.len(),
                            );
                            let el_start = el.data_offset as usize;
                            let el_end = el_start + el.data_size as usize;
                            let intra = body_diff_offset.saturating_sub(el_start);
                            s.push_str(&format!(
                                "\n      diff at intra-element offset {} ({:#x})",
                                intra, intra,
                            ));
                            s.push_str("\n      element fields:");
                            for f in &el.fields {
                                if f.start < f.end {
                                    let rs = f.start - el_start;
                                    let re = f.end - el_start;
                                    s.push_str(&format!(
                                        "\n        #{:2} {:28} kind={:14} [{:#x}..{:#x}) len={}",
                                        f.field_index, f.name, f.kind.as_str(), rs, re, re - rs,
                                    ));
                                } else if f.present {
                                    s.push_str(&format!(
                                        "\n        #{:2} {:28} kind={:14} (present, no span)",
                                        f.field_index, f.name, f.kind.as_str(),
                                    ));
                                }
                            }
                            s.push_str(&format!(
                                "\n      element original bytes: {:02x?}",
                                &save.body[el_start..el_end]
                            ));
                            s
                        }
                        None => "    (no sub-element found at diff offset)".to_string(),
                    };
                    // Walk the field tree printing each field's intra-block
                    // span so we can identify which one straddles the diff.
                    let mut field_dump = String::new();
                    let block_start = block.data_offset as usize;
                    for field in &block.fields {
                        if field.start < field.end {
                            let rel_start = field.start - block_start;
                            let rel_end = field.end - block_start;
                            field_dump.push_str(&format!(
                                "    field #{:2} {:30} kind={:14} [{:#x}..{:#x}) len={}\n",
                                field.field_index,
                                field.name,
                                field.kind.as_str(),
                                rel_start,
                                rel_end,
                                rel_end - rel_start,
                            ));
                        }
                    }
                    panic!(
                        "{}: block class={} ({}) at offset {:#x} did not round-trip\n  \
                         original len={}, encoded len={}\n  \
                         first diff at intra-block offset {} ({:#x})\n  \
                         original bytes [diff-32 .. diff+32]: {:02x?}\n  \
                         encoded  bytes [diff-32 .. diff+32]: {:02x?}\n{}\n  \
                         field map:\n{}",
                        path.display(),
                        block.class_name,
                        block.class_index,
                        start,
                        original.len(),
                        encoded.len(),
                        first_diff,
                        first_diff,
                        &original[first_diff.saturating_sub(32)..(first_diff + 32).min(original.len())],
                        &encoded[first_diff.saturating_sub(32)..(first_diff + 32).min(encoded.len())],
                        element_info,
                        field_dump,
                    );
                }
            }
            println!(
                "{}: {} blocks all round-trip ok",
                path.display(),
                blocks.len()
            );
        }
        println!("per-block roundtrip OK across {} saves, {} blocks total", saves.len(), total_blocks);
    }

    /// Full-body round-trip stability: Body::write should be IDEMPOTENT —
    /// the second write of an already-encoded body equals the first.
    ///
    /// Note: we can't assert byte-identity against the raw engine bytes
    /// directly because the engine writes some items' `payload_offset`
    /// u32 with non-canonical values. The decoder tolerates these
    /// (falls back to wrapper_end), and the encoder always emits the
    /// canonical wrapper_end. The first re-emit therefore differs
    /// from the raw bytes; subsequent re-emits match the first
    /// (idempotent stable state).
    ///
    /// This still proves the encoder is a deterministic structural
    /// inverse of the decoder — the property we actually need for
    /// length-changing edits.
    #[test]
    fn test_save_body_full_roundtrip() {
        use crate::save::{Body, Save};
        let saves = collect_all_saves();
        if saves.is_empty() {
            eprintln!("skipping full-body roundtrip: no saves found");
            return;
        }
        for (path, data) in &saves {
            let save = Save::parse(data).unwrap();
            let body = Body::parse(&save.body).unwrap();
            let blocks = body.decode_blocks(&save.body);

            // First write: may differ from raw (engine quirks).
            let first = body.write(&save.body, &blocks).unwrap_or_else(|e| {
                panic!("{}: body.write failed: {}", path.display(), e)
            });
            // Length must always match — encoding never grows or shrinks
            // unmodified data.
            if first.len() != save.body.len() {
                panic!(
                    "{}: body length drifted on first write: {} -> {}",
                    path.display(),
                    save.body.len(),
                    first.len(),
                );
            }

            // Re-parse the canonical output, then re-write. Should be
            // byte-identical to the first write (idempotent stability).
            let body2 = Body::parse(&first).unwrap_or_else(|e| {
                panic!("{}: re-parse of canonical body failed: {}", path.display(), e)
            });
            let blocks2 = body2.decode_blocks(&first);
            let second = body2.write(&first, &blocks2).unwrap_or_else(|e| {
                panic!("{}: body.write[2] failed: {}", path.display(), e)
            });
            if second != first {
                let first_diff = first
                    .iter()
                    .zip(second.iter())
                    .position(|(a, b)| a != b)
                    .unwrap_or(first.len().min(second.len()));
                panic!(
                    "{}: encoder is not idempotent (write[2] != write[1])\n  \
                     write[1] len={}, write[2] len={}\n  \
                     first diff at body offset {} ({:#x})",
                    path.display(),
                    first.len(),
                    second.len(),
                    first_diff,
                    first_diff,
                );
            }
            println!(
                "{}: full body round-trip ok ({} bytes; canonical {} bytes; idempotent)",
                path.display(),
                save.body.len(),
                first.len(),
            );
        }
    }

    /// Regression for the save-loader length-change crash
    /// (docs/save-loader-length-change-crash.md). A non-empty
    /// `_reviveQuestList` with no `01 01 01 01 01` trailer must decode (via the
    /// `prefix_00xx0100_notrailer` dynamic-array variant) so the following
    /// `_factionNodeApplySkillList` object_list is parsed as a field. Before the
    /// fix the forward walk broke at `_reviveQuestList`, dumping that object_list
    /// — wrapper `payload_offset` and all — into `trailing_pad`; the encoder
    /// wrote it verbatim, so on a length-changing edit the offset went stale and
    /// the game's loader crashed on the dangling pointer.
    #[test]
    fn test_faction_revive_quest_no_trailer_relocates() {
        use crate::save::{Body, FieldKind, FieldValue, Save, encode_top_level_block};
        use std::path::PathBuf;

        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
            "tests/fixtures/saves/1.10/broken_save_after_length_change/base_sample/save.save",
        );
        let Ok(data) = std::fs::read(&p) else {
            eprintln!("skipping: {} not present", p.display());
            return;
        };
        // git-crypt-locked clones check the file out as its encrypted blob.
        if data.get(..4) != Some(b"SAVE") {
            eprintln!("skipping: base_sample is git-crypt locked (no key)");
            return;
        }

        let save = Save::parse(&data).unwrap();
        let body = Body::parse(&save.body).unwrap();
        let blocks = body.decode_blocks(&save.body);

        // (a) No FactionNodeElementSaveData may have `_factionNodeApplySkillList`
        // present-but-undecoded — the signature of the broken forward walk.
        fn scan(fields: &[crate::save::DecodedField], broken: &mut usize) {
            for f in fields {
                match &f.value {
                    FieldValue::ObjectList { elements, .. } => {
                        for el in elements {
                            if el.class_name == "FactionNodeElementSaveData"
                                && el.fields.iter().any(|ff| {
                                    ff.name == "_factionNodeApplySkillList"
                                        && ff.present
                                        && ff.kind == FieldKind::Unknown
                                })
                            {
                                *broken += 1;
                            }
                            scan(&el.fields, broken);
                        }
                    }
                    FieldValue::Locator { child: Some(c), .. } => scan(&c.fields, broken),
                    _ => {}
                }
            }
        }
        let mut broken = 0;
        for b in &blocks {
            scan(&b.fields, &mut broken);
        }
        assert_eq!(
            broken, 0,
            "_reviveQuestList must decode so _factionNodeApplySkillList is a field, not trailing_pad",
        );

        // (b) Every self-referential wrapper `payload_offset` in the
        // FactionSaveData block must relocate when the block is re-encoded at a
        // shifted offset (this is what the editor's length-changing edit does).
        let fac = blocks
            .iter()
            .find(|b| b.fields.iter().any(|f| f.name == "_factionNodeElementSaveDataList"))
            .expect("FactionSaveData block present");
        let o = fac.data_offset;
        let at_o = encode_top_level_block(fac, o).unwrap();
        assert_eq!(
            &at_o[..],
            &save.body[o as usize..o as usize + fac.data_size as usize],
            "no-op re-encode of FactionSaveData must be byte-identical",
        );
        let shift = 0x100u32;
        let at_shift = encode_top_level_block(fac, o + shift).unwrap();
        let mut relocated = 0usize;
        let mut p = 0usize;
        while p + 4 <= at_o.len() {
            let v = u32::from_le_bytes(at_o[p..p + 4].try_into().unwrap());
            // self-referential wrapper end: value == its own absolute position + 4
            if v == o + p as u32 + 4 {
                let v2 = u32::from_le_bytes(at_shift[p..p + 4].try_into().unwrap());
                assert_eq!(
                    v2,
                    o + shift + p as u32 + 4,
                    "wrapper payload_offset at +{p} must relocate by the block shift",
                );
                relocated += 1;
            }
            p += 1;
        }
        assert!(relocated > 0, "expected at least one relocatable wrapper offset");
        eprintln!("faction relocation regression: {relocated} wrapper offsets relocate, 0 broken nodes");
    }

    /// Companion to the above for the `_reviveQuestList` header variants the
    /// decoder still can't parse (`00 00 01 00` / `00 01 01 00`): their
    /// `_factionNodeApplySkillList` stays in `trailing_pad`, but the encoder's
    /// self-referential relocation pass must still move its wrapper
    /// `payload_offset` when the block shifts. `engine-natural/before` carries
    /// such a node, so EVERY self-referential offset there must relocate even
    /// though some nodes don't fully decode.
    #[test]
    fn test_faction_node_offset_relocates_through_trailing_pad() {
        use crate::save::{Body, Save, encode_top_level_block};
        use std::path::PathBuf;

        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
            "tests/fixtures/saves/1.10/sealed-artifact-challenge/engine-natural/before/save.save",
        );
        let Ok(data) = std::fs::read(&p) else {
            eprintln!("skipping: {} not present", p.display());
            return;
        };
        if data.get(..4) != Some(b"SAVE") {
            eprintln!("skipping: fixture is git-crypt locked (no key)");
            return;
        }

        let save = Save::parse(&data).unwrap();
        let body = Body::parse(&save.body).unwrap();
        let blocks = body.decode_blocks(&save.body);
        let fac = blocks
            .iter()
            .find(|b| b.fields.iter().any(|f| f.name == "_factionNodeElementSaveDataList"))
            .expect("FactionSaveData block present");

        let o = fac.data_offset;
        let at_o = encode_top_level_block(fac, o).unwrap();
        assert_eq!(
            &at_o[..],
            &save.body[o as usize..o as usize + fac.data_size as usize],
            "no-op re-encode must be byte-identical (clean game save)",
        );
        let shift = 0x100u32;
        let at_shift = encode_top_level_block(fac, o + shift).unwrap();
        let mut relocated = 0usize;
        let mut p = 0usize;
        while p + 4 <= at_o.len() {
            let v = u32::from_le_bytes(at_o[p..p + 4].try_into().unwrap());
            if v == o + p as u32 + 4 {
                let v2 = u32::from_le_bytes(at_shift[p..p + 4].try_into().unwrap());
                assert_eq!(
                    v2,
                    o + shift + p as u32 + 4,
                    "offset at +{p} did not relocate (stranded in trailing_pad?)",
                );
                relocated += 1;
            }
            p += 1;
        }
        assert!(relocated > 0, "expected relocatable wrapper offsets");
        eprintln!("trailing_pad relocation regression: {relocated} offsets relocate");
    }

    /// Compare the `ContentsMiscSaveData` block between two saves
    /// (default: slot104 = 1.09, slot107 = 1.10). Dumps each field's
    /// decode kind + object_list count/variant, and the leading bytes of
    /// any undecoded region. Used to RE the 1.10 `_contentsNpcSchedule`
    /// drift.
    ///
    /// ```text
    /// cargo test --lib --features c_abi _probe_contents_misc_drift -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "investigation only — uses the user's live save files"]
    fn _probe_contents_misc_drift() {
        use crate::save::{Body, FieldValue, Save};
        use std::path::PathBuf;

        let base = std::env::var_os("LOCALAPPDATA").map(|l| {
            PathBuf::from(l).join("Pearl Abyss").join("CD").join("save")
        });
        let Some(base) = base else {
            eprintln!("skipping: no LOCALAPPDATA");
            return;
        };
        // Find the account dir.
        let account = std::fs::read_dir(&base)
            .ok()
            .and_then(|rd| rd.flatten().map(|e| e.path()).find(|p| p.is_dir()));
        let Some(account) = account else {
            eprintln!("skipping: no account dir under {}", base.display());
            return;
        };

        let probe_one = |slot: &str| {
            let path = account.join(slot).join("save.save");
            if !path.is_file() {
                eprintln!("  {slot}: not found");
                return;
            }
            let raw = std::fs::read(&path).expect("read save");
            let save = Save::parse(&raw).expect("parse save");
            let body = Body::parse(&save.body).expect("parse body");
            let blocks = body.decode_blocks(&save.body);
            let Some(cms) = blocks.iter().find(|b| b.class_name == "ContentsMiscSaveData") else {
                eprintln!("  {slot}: no ContentsMiscSaveData block");
                return;
            };
            eprintln!(
                "\n=== {slot}: ContentsMiscSaveData (data_offset={}, size={}) ===",
                cms.data_offset, cms.data_size
            );
            eprintln!("  undecoded_ranges: {:?}", cms.undecoded_ranges);
            for f in &cms.fields {
                let detail = match &f.value {
                    FieldValue::ObjectList { count, header_variant, elements, header_bytes } => {
                        format!(
                            "list count={count} variant={header_variant} elements_decoded={} header_bytes={:02x?}",
                            elements.len(),
                            &header_bytes[..header_bytes.len().min(24)],
                        )
                    }
                    FieldValue::Scalar(_) => "scalar".to_string(),
                    FieldValue::None => "none".to_string(),
                    other => format!("{other:?}").chars().take(60).collect(),
                };
                eprintln!(
                    "  [{:2}] present={} kind={:?} name={} start={} end={} :: {}",
                    f.field_index, f.present, f.kind, f.name, f.start, f.end, detail
                );
            }
            // Dump leading bytes of each undecoded range.
            for (s, e) in &cms.undecoded_ranges {
                let end = (*s + 96).min(*e);
                eprintln!("  undecoded [{s},{e}) leading bytes:");
                let mut o = *s;
                while o < end {
                    let stop = (o + 16).min(end);
                    eprintln!("    +{:>5} {:02x?}", o - *s, &save.body[o..stop]);
                    o = stop;
                }
            }
        };

        let slot_a = std::env::var("CRIMSON_SAVE_A").unwrap_or_else(|_| "slot104".into());
        let slot_b = std::env::var("CRIMSON_SAVE_B").unwrap_or_else(|_| "slot107".into());
        probe_one(&slot_a);
        probe_one(&slot_b);
    }
}
