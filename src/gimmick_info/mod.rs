//! `gimmickinfo.pabgb` parser — anchor-scan based.
//!
//! Resolves `GimmickInfoKey (u32)` — the gamedata key stored as the
//! `_gimmickInfoKey` sibling of every `FieldGimmickSaveData` block.
//! Pairs with `_fieldGimmickSaveDataKey` (the save-local slot index)
//! to identify a placed gimmick instance; `_gimmickInfoKey` is the
//! one with a gamedata entry.
//!
//! Schema-wise this is the same `[u32 name_len][name][u32 key][...body]`
//! row head as the other named tables, but with two complications the
//! simpler parsers don't have:
//!
//! 1. **Cat-byte in the row key.** Gimmick row keys span multiple
//!    hi-bytes: `0x00`, `0x01`, `0x07`, `0x08`, `0x09` (region or
//!    gimmick-category marker baked into the key). The
//!    `(key >> 24) == 0` constraint other parsers rely on would
//!    miss those — a sizeable share of real rows. The scanner here uses
//!    `(key >> 24) < 0x10` as a looser cap (covers every observed
//!    cat-byte with headroom).
//!
//! 2. **Body-byte noise.** Gimmick rows have bodies dense with
//!    embedded `[len][name][u32]` elements, many of which pass the key and
//!    name rules alone, and some carry a real row's key (`InspectSocket_0` +
//!    the key of the gimmick it sockets) ahead of that row. What settles it:
//!    every indexed row continues `[u8 0x00][u32 len]["/object/..."]`, and
//!    none of the noise does, so the scanner requires that. It also checks
//!    `(key & 0xFFFFFF) != 0` (no real row has a zero lo24) and
//!    `(key >> 24) < 0x10` (the noise's hi-byte exceeds the observed
//!    cat-byte range).
//!
//! No PALOC entries are produced by this parser — the bridge
//! consumes the `(key, internal_name)` pair and lets the c_abi
//! `lookup_display_name` chain into PALOC at the caller's lo32
//! (default `0x200` for the gimmick display label, `0x19202` for
//! description).

use crate::binary::row_head::{is_row_name_byte, read_row_head};

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

/// 2.06+: row names live in the `.staticinfoheader`, which lists every row,
/// so with a 2.06 header the rows come from it, in header order (the anchor
/// scan drops rows its key and name rules do not expect). A pre-2.06 pair,
/// or a header that is not the 2.06 shape, scans `data`.
pub fn parse_gimmick_info_lossy_with_header(data: &[u8], header: &[u8]) -> Vec<GimmickInfoEntry> {
    match crate::binary::static_header::header_row_names(data, header) {
        Some(rows) => rows.into_iter().map(|(key, name)| GimmickInfoEntry { key, name }).collect(),
        None => parse_gimmick_info_lossy(data),
    }
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
/// - The key is followed by `0x00` and a non-empty `/`-rooted asset path.
///
/// Returns entries in the order they appear in the file. The bridge's
/// loader uses first-wins HashMap insertion as a backstop.
pub fn parse_gimmick_info_lossy(data: &[u8]) -> Vec<GimmickInfoEntry> {
    let mut entries = Vec::new();
    let mut cursor = 0;
    while let Some(start) = scan_next_anchor(data, cursor) {
        // Scanner already validated the head — the unwrap is sound.
        let head = read_row_head(&data[start..], 4).unwrap();
        entries.push(GimmickInfoEntry {
            key: head.key,
            name: head.name.to_owned(),
        });
        cursor = start + head.body_at;
    }
    entries
}

/// Does a `[u32 len]["/..."]` asset path start at `at`? Every real row's
/// does; see the module docs.
fn opens_with_path(data: &[u8], at: usize) -> bool {
    let Some(head) = data.get(at..at + 5) else {
        return false;
    };
    head[..4] != [0; 4] && head[4] == b'/'
}

fn scan_next_anchor(data: &[u8], from: usize) -> Option<usize> {
    let n = data.len();
    let mut o = from;
    while o + 12 < n {
        let slen = u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]) as usize;
        // Length, name, key, then the 0x00 that opens every row body in the
        // pre-2.06 layout — the byte that rejects most body strings followed
        // by a small number.
        if (2..=128).contains(&slen) && o + 9 + slen <= n && data[o + 8 + slen] == 0 {
            let bytes = &data[o + 4..o + 4 + slen];
            let key = u32::from_le_bytes([
                data[o + 4 + slen],
                data[o + 5 + slen],
                data[o + 6 + slen],
                data[o + 7 + slen],
            ]);
            // `key` rules: see the parser's doc comment. The `/object/...` path is
            // what separates a row from an element that names another gimmick.
            if key != 0
                && (key >> 24) < 0x10
                && (key & 0x00FF_FFFF) != 0
                && bytes[0].is_ascii_alphabetic()
                && bytes.contains(&b'_')
                && bytes.iter().all(|&b| is_row_name_byte(b))
                && std::str::from_utf8(bytes).is_ok()
                && opens_with_path(data, o + 9 + slen)
            {
                return Some(o);
            }
        }
        o += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    //! Live-install integration test against the real
    //! `gimmickinfo.pabgb`. Pins a handful of mappings drawn from the
    //! Save Editor handoff bundle to ensure scanner + cat-byte
    //! handling stays correct across patches.

    use crate::binary::gamedata_layout;
    use super::*;

    /// (GimmickInfoKey, expected internal_name), drawn from the Save Editor
    /// handoff bundle. The cat-byte
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
        // Rows an earlier `InspectSocket_0` element names by key: only the
        // scanner's asset-path check keeps the element from taking the key.
        (0x000f_431b, "gimmick_equip_hp_summon_soulknight"),
        (0x000f_44a2, "gimmick_equip_rocket_backpack"),
        (0x0089_a265, "box_close_01"),
        // cat=0x01 — secondary category
        (0x0103_db72, "mine_copper"),
        // cat=0x00, large lo24
        (0x0053_9e50, "Background_Lamp_16"),
    ];

    fn find_gimmickinfo_bytes() -> Option<Vec<u8>> {
        gamedata_layout::extract_bin(&gamedata_layout::body("gimmickinfo"))
    }

    /// 2.06+: the row names live here. Empty when absent (an older install
    /// needs none, and `parse_gimmick_info_lossy_with_header` then scans the body alone).
    fn find_gimmickinfo_header() -> Vec<u8> {
        gamedata_layout::extract_bin(&gamedata_layout::header("gimmickinfo")).expect("the 2.06 header next to the table body (install present but header missing: drift?)")
    }

    #[test]
    fn gimmick_info_lossy_live() {
        let Some(data) = find_gimmickinfo_bytes() else {
            eprintln!("skipping gimmick_info_lossy_live: no game install");
            return;
        };
        let header = find_gimmickinfo_header();
        let entries = parse_gimmick_info_lossy_with_header(&data, &header);
        // A 2.06 header lists every row, including rows the scanner rules
        // drop, so the name and key rules below hold for the anchor scan alone.
        let scanned = crate::binary::static_header::header_row_names(&data, &header).is_none();
        println!(
            "parsed {} gimmickinfo entries from {} bytes",
            entries.len(),
            data.len()
        );
        // A plausibility floor well under the real row count.
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
        for e in entries.iter().filter(|_| scanned) {
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
