mod binary;
mod crypto;
mod item_info;
mod python;
pub(crate) mod python_traits;

use pyo3::prelude::*;

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
}
