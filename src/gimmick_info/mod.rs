//! `gimmickinfo.pabgb` parser — anchor-scan based.
//!
//! Resolves `GimmickInfoKey (u32)` — the gamedata key stored as the
//! `_gimmickInfoKey` sibling of every `FieldGimmickSaveData` block.
//! Pairs with `_fieldGimmickSaveDataKey` (the save-local slot index)
//! to identify a placed gimmick instance; `_gimmickInfoKey` is the
//! one with a gamedata entry.
//!
//! Schema-wise this is the same `[u32 key][u32 name_len][name][...body]`
//! row header as mission/quest/stage/iteminfo, but with two
//! complications the simpler parsers don't have:
//!
//! 1. **Cat-byte in the row key.** Gimmick row keys span multiple
//!    hi-bytes: `0x00`, `0x01`, `0x07`, `0x08`, `0x09` (region or
//!    gimmick-category marker baked into the key). The
//!    `(key >> 24) == 0` constraint other parsers rely on would
//!    miss those — about 12% of real rows. The scanner here uses
//!    `(key >> 24) < 0x10` as a looser cap (covers everything
//!    observed in 1.06 with headroom).
//!
//! 2. **Body-byte noise.** Gimmick rows have bodies dense with
//!    embedded references that occasionally satisfy the anchor
//!    shape — e.g. `0x01000000` (lo24=0) reading as
//!    `"UnnamedTrigger_0"` (the no-trigger placeholder embed), or
//!    `0x7475706e` (= ASCII `"tupn"`) reading as `"GimmickOnExitState"`
//!    (a state-enum byte sequence that happens to look like a real
//!    anchor). Filtered out via two extra checks: `(key & 0xFFFFFF)
//!    != 0` (no real row has a zero lo24) and `(key >> 24) < 0x10`
//!    (the noise's hi-byte exceeds the observed cat-byte range).
//!
//! No PALOC entries are produced by this parser — the bridge
//! consumes the `(key, internal_name)` pair and lets the c_abi
//! `lookup_display_name` chain into PALOC at the caller's lo32
//! (default `0x200` for the gimmick display label, `0x19202` for
//! description).
//!
//! Coverage on the live 1.06 install: 527/530 (99.4%) of the editor's
//! sample save `_gimmickInfoKey` values resolve through this parser.

/// One parsed gimmickinfo row, reduced to what the bridge consumes.
#[derive(Debug, Clone)]
pub struct GimmickInfoEntry {
    /// `GimmickInfoKey` as stored in save blocks
    /// (`FieldGimmickSaveData._gimmickInfoKey`). Can have a non-zero
    /// hi-byte indicating gimmick category / region.
    pub key: u32,
    /// ASCII internal name (e.g. `"gimmick_cart_middle_03"`,
    /// `"Background_Lamp_16"`, `"mine_copper"`).
    pub name: String,
}

/// Lossy anchor-scan parse of an in-memory `gimmickinfo.pabgb` blob.
///
/// Validation rules:
///
/// - `key != 0 && (key >> 24) < 0x10` — real rows use hi-byte 0-9
///   plus headroom. Filters body-byte garbage whose first 4 bytes
///   look like ASCII ('tupn' = 0x7475706e etc.) and other large-key
///   embeds.
/// - `(key & 0xFFFFFF) != 0` — no real row has a zero lo24. Filters
///   the `0x01000000 UnnamedTrigger_0` placeholder embed.
/// - `name_len ∈ [2, 128]` — actual range ~5–60.
/// - First byte ASCII letter; all bytes identifier-shape.
///
/// Returns entries in the order they appear in the file. The bridge's
/// loader uses first-wins HashMap insertion so the real row (which
/// appears first in the file) wins over any later body-byte
/// collision that happens to share a key.
pub fn parse_gimmick_info_lossy(data: &[u8]) -> Vec<GimmickInfoEntry> {
    let mut entries = Vec::new();
    let mut cursor = 0;
    while let Some(start) = scan_next_anchor(data, cursor) {
        let slen = u32::from_le_bytes([
            data[start + 4],
            data[start + 5],
            data[start + 6],
            data[start + 7],
        ]) as usize;
        let key = u32::from_le_bytes([
            data[start],
            data[start + 1],
            data[start + 2],
            data[start + 3],
        ]);
        let name_bytes = &data[start + 8..start + 8 + slen];
        // Scanner already validated valid UTF-8 — the unwrap is sound.
        let name = std::str::from_utf8(name_bytes).unwrap().to_owned();
        entries.push(GimmickInfoEntry { key, name });
        cursor = start + 8 + slen;
    }
    entries
}

fn scan_next_anchor(data: &[u8], from: usize) -> Option<usize> {
    let n = data.len();
    let mut o = from;
    while o + 12 < n {
        let key = u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
        if key != 0
            && (key >> 24) < 0x10
            && (key & 0x00FF_FFFF) != 0
        {
            let slen = u32::from_le_bytes([
                data[o + 4],
                data[o + 5],
                data[o + 6],
                data[o + 7],
            ]) as usize;
            if (2..=128).contains(&slen) && o + 8 + slen <= n {
                let bytes = &data[o + 8..o + 8 + slen];
                if bytes[0].is_ascii_alphabetic()
                    && bytes.contains(&b'_')
                    && bytes.iter().all(|&b| is_ident_byte(b))
                    && std::str::from_utf8(bytes).is_ok()
                {
                    return Some(o);
                }
            }
        }
        o += 1;
    }
    None
}

#[inline]
fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b' ' || b >= 0x80
}

#[cfg(test)]
mod tests {
    //! Live-install integration test against the real
    //! `gimmickinfo.pabgb`. Pins a handful of mappings drawn from the
    //! Save Editor handoff bundle to ensure scanner + cat-byte
    //! handling stays correct across patches.

    use super::*;
    use std::path::PathBuf;

    /// (GimmickInfoKey, expected internal_name) — verified via the
    /// 2026-05-14 probe pass against the handoff bundle. The cat-byte
    /// 0x09 sample exercises the loose hi-byte rule (would be missed
    /// by an `(key >> 24) == 0` constraint).
    const KNOWN: &[(u32, &str)] = &[
        // cat=0x09 — cart category
        (0x094c_861e, "gimmick_cart_middle_03"),
        (0x094c_861f, "gimmick_cart_middle_03_hellhound"),
        (0x094c_8621, "gimmick_cart_waterbottle_01"),
        // cat=0x00 — generic gimmicks (the bulk)
        (0x000f_6265, "gimmick_tool_dryingrope_herb_0002"),
        (0x000f_4ebc, "abyssruins_useartifact_01_part"),
        (0x000f_569e, "gimmick_trap_rope_03m_0001"),
        (0x000f_4288, "Background_DisplayStand_04"),
        (0x0089_a265, "box_close_01"),
        // cat=0x01 — secondary category
        (0x0103_db72, "mine_copper"),
        // cat=0x00, large lo24
        (0x0053_9e50, "Background_Lamp_16"),
    ];

    fn find_gimmickinfo_bytes() -> Option<Vec<u8>> {
        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let pamt_path = game_root.join("0008").join("0.pamt");
        if !pamt_path.is_file() {
            return None;
        }
        let pamt_data = std::fs::read(&pamt_path).ok()?;
        let pamt = crate::binary::pamt::PackMeta::parse(&pamt_data, None).ok()?;
        let dir = pamt
            .directories
            .iter()
            .find(|d| d.path == "gamedata/binary__/client/bin")?;
        let file = dir.files.iter().find(|f| f.name == "gimmickinfo.pabgb")?;
        let group_dir = game_root.join("0008");
        crate::binary::paz::extract_file(
            &group_dir,
            file,
            "gamedata/binary__/client/bin",
            &pamt.header.encrypt_info.encrypt_info,
        )
        .ok()
    }

    #[test]
    fn gimmick_info_lossy_live() {
        let Some(data) = find_gimmickinfo_bytes() else {
            eprintln!("skipping gimmick_info_lossy_live: no game install");
            return;
        };
        let entries = parse_gimmick_info_lossy(&data);
        println!(
            "parsed {} gimmickinfo entries from {} bytes",
            entries.len(),
            data.len()
        );
        // 1.06 has ~10k+ real gimmick rows after the strict scanner.
        // Pin to >2000 for plausibility.
        assert!(
            entries.len() > 2_000,
            "expected >2000 gimmick entries, got {}",
            entries.len()
        );

        // First-wins to mirror bridge behaviour.
        let mut by_key: std::collections::HashMap<u32, &str> = Default::default();
        for e in &entries {
            by_key.entry(e.key).or_insert(e.name.as_str());
        }
        for &(key, expected) in KNOWN {
            assert_eq!(
                by_key.get(&key).copied(),
                Some(expected),
                "GimmickInfoKey 0x{:08x} ({}) mismatch",
                key,
                key,
            );
        }

        // Sanity: no entry leads with a digit, and no entry has a
        // hi-byte ≥ 0x10 (the noise filter from the scanner).
        for e in &entries {
            assert!(
                e.name.bytes().next().is_some_and(|b| b.is_ascii_alphabetic()),
                "non-letter-leading name: key=0x{:08x}, name={:?}",
                e.key,
                e.name,
            );
            assert!(
                (e.key >> 24) < 0x10,
                "implausible hi-byte: key=0x{:08x}, name={:?}",
                e.key,
                e.name,
            );
            assert_ne!(
                e.key & 0x00FF_FFFF,
                0,
                "zero lo24: key=0x{:08x}, name={:?}",
                e.key,
                e.name,
            );
        }
    }
}
