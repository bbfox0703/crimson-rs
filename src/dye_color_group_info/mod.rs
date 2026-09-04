//! `dyecolorgroupinfo.pabgb` parser — PABGH-indexed.
//!
//! Resolves `DyeColorGroupInfoKey (u32)` — the gamedata key stored as
//! `ItemDyeSaveData._dyeColorGroupInfoKey` in the save's
//! `_itemDyeDataList`. The 10 rows in 1.07 are the **themed color
//! palettes** the in-game NPC dye menu offers (e.g.
//! `Her_Color_Group_I` = 埃爾南德, `Por_Color_Group_I` = 波羅琳).
//!
//! ## Schema (verified against the live 1.07 install, every row, byte-perfect)
//!
//! ```text
//! Row {
//!     u32 key,                  // matches PABGH key
//!     u32 name_len,
//!     u8 name[name_len],        // ASCII, e.g. "Her_Color_Group_I"
//!     u8 flag,                  // always 0
//!     u32 color_count,          // always 109 in 1.07
//!     u8 color_data[8 * color_count],  // palette (BGRA + 4-byte tail per record, see below)
//!     // 37-byte trailer:
//!     u8 trailer_marker[5],     // 23 31 02 00 00 — constant across rows
//!     u32 key_copy,             // = key
//!     u32 numeric_id_len,       // always 20
//!     u8 numeric_id[20],        // ASCII decimal digits (u64 hash serialized as text)
//!     u8 trailing_hash[4],      // per-row hash value
//! }
//! ```
//!
//! ## Byte order — **BGRA, not RGBA** (verified 2026-05-17)
//!
//! Each 8-byte color record is `[B, G, R, A, tail0, tail1, tail2, tail3]`.
//! The save's `_dyeColorR/G/B/A` are u8 scalars stored in logical RGB
//! order, so the gradient bytes MUST be swapped before comparison
//! (`gradient_record[0]` = B, `[1]` = G, `[2]` = R, `[3]` = A).
//!
//! The parser stores positions in **logical RGBA order** (post-swap)
//! to match the save side. Cross-checking the
//! [slot103 probe](../c_abi/character_info.rs) (11 observed RGBs from
//! the user's applied dyes — 6 Hernand reds + 5 Pororin olives)
//! found every observed RGB hits an exact gradient position after
//! the swap, e.g. `#f22121` = position 17 in `Her_Color_Group_I`
//! (stored as `21 21 f2 ff`).
//!
//! ## Palette layout
//!
//! 109 positions per theme, organized as:
//! - **Positions 0-8** (9 records): grayscale ramp — varying lightness
//!   of the same hue (Hernand starts white, Pororin starts dark gray).
//! - **Positions 9-108** (100 records): 10 chromatic rows × 10 columns.
//!   Each row is a lightness tier (`0xf2 → 0xd9 → 0xbf → 0xa6 → 0x8c →
//!   0x73 → 0x59 → 0x40 → 0x26 → 0x1a` — the **R** channel after swap,
//!   which is the dominant chromatic channel in red-tinted themes
//!   like Hernand; for cyan-tinted themes like Pororin it's the **G/B**
//!   pair). Each column varies the secondary channel from pale
//!   (column 0) to fully saturated (column 9).
//!
//! ## What this implies for the dye editor
//!
//! The C# editor's dye picker should render the 109 positions per
//! theme as a visual grid (1 row of 9 grays + 10 rows × 10 chromatic).
//! User picks a cell; editor writes the cell's R/G/B back into the
//! save's `_dyeColorR/G/B` u8 scalars. The reverse lookup
//! ([`palette_position_for_rgb`]) lets the editor highlight which
//! cell a currently-applied dye came from. The freeform RGB sliders
//! in the PyQt5 reference editor were misleading — the in-game UI
//! is a discrete picker, not a continuous one. Off-grid RGB values
//! aren't reachable through normal gameplay.
//!
//! The 4-byte tail per record carries a per-row lightness key (byte 0)
//! plus a constant `e1 ff ff` (bytes 1-3). The lightness key bytes
//! observed in 1.07: `d4..cf` for Pororin, `fe..f9` for Hernand. Not
//! exposed by the bridge — included in the per-record dump for
//! diagnostic purposes only.
//!
//! The file ships with a matching `dyecolorgroupinfo.pabgh` so the
//! row offsets are explicit — we don't need anchor-scanning here. The
//! parser uses `skill_info::parse_pabgh` since the layout is identical
//! (`u16 count + (u32 key, u32 offset)*`).

/// One parsed dye color group row.
#[derive(Debug, Clone)]
pub struct DyeColorGroupInfoEntry {
    /// `DyeColorGroupInfoKey` as stored in `ItemDyeSaveData._dyeColorGroupInfoKey`.
    pub key: u32,
    /// ASCII internal name (e.g. `"Her_Color_Group_I"`,
    /// `"Dem_Color_Group_III"`). The dye UI shows a localized version
    /// of this — extending PALOC lookup is a v2 task; for v1 the C#
    /// editor can render the raw internal name.
    pub name: String,
    /// Logical-RGBA palette (109 records in 1.07). Stored as
    /// `(R, G, B, A)` post-swap from the on-disk BGRA byte order, so
    /// each entry can be compared directly against the save's u8
    /// `_dyeColorR/G/B/A` scalars. See module docs for the layout
    /// (positions 0-8 grayscale + 9-108 ten chromatic rows × ten
    /// columns) and how the editor should render this as a picker.
    pub palette: Vec<[u8; 4]>,
}

/// Parse `dyecolorgroupinfo.pabgb` using its `.pabgh` index.
///
/// Returns entries in PABGH on-disk order. Skips any row whose body
/// is too short to contain `[u32 key][u32 name_len][name_len bytes]`
/// or whose name bytes are not valid UTF-8 — both treated as anchor
/// noise rather than panics so a future patch with new row shapes
/// doesn't break the build.
pub fn parse_dye_color_group_info_lossy(
    pabgb: &[u8],
    pabgh: &[u8],
) -> Vec<DyeColorGroupInfoEntry> {
    let Ok(index) = crate::skill_info::parse_pabgh(pabgh) else {
        return Vec::new();
    };
    let ranges = crate::skill_info::entry_ranges(&index, pabgb.len());
    let mut out = Vec::with_capacity(index.len());
    for (entry, (start, end)) in index.iter().zip(ranges.iter()) {
        let Some(body) = pabgb.get(*start..*end) else {
            continue;
        };
        if body.len() < 8 {
            continue;
        }
        let key = u32::from_le_bytes([body[0], body[1], body[2], body[3]]);
        if key != entry.key {
            // PABGH/PABGB mismatch — refuse the row rather than emit
            // garbage. A future patch could change the row prefix; the
            // bridge surface treats this as "row missing".
            continue;
        }
        let name_len = u32::from_le_bytes([body[4], body[5], body[6], body[7]]) as usize;
        if 8 + name_len > body.len() || !(1..=128).contains(&name_len) {
            continue;
        }
        let Ok(name) = std::str::from_utf8(&body[8..8 + name_len]) else {
            continue;
        };

        // Parse the 109-record palette body.
        // Layout after the name: `u8 flag + u32 color_count + 8 * color_count bytes`.
        // Bytes per record: [B, G, R, A, tail0..tail3]; we keep only the
        // logical RGBA (post-swap) so palette comparisons against the
        // save's u8 _dyeColorR/G/B/A are direct.
        let palette = parse_palette_after_name(body, 8 + name_len).unwrap_or_default();

        out.push(DyeColorGroupInfoEntry {
            key,
            name: name.to_owned(),
            palette,
        });
    }
    out
}

/// Test helper — extract the `dyecolorgroupinfo` PABGB + PABGH pair
/// from the live game install (PAMT-resolved). Exposed at module
/// scope so the sibling `c_abi/dye_color_group_info.rs` bridge tests
/// can reuse the extraction logic; `#[cfg(test)]` keeps it out of the
/// shipped library binary.
#[cfg(test)]
pub(crate) fn extract_pair_for_tests() -> Option<(Vec<u8>, Vec<u8>)> {
    let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
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
        .find(|d| d.path == crate::binary::gamedata_layout::bin_dir())?;
    let pabgb_file = dir.files.iter().find(|f| f.name == crate::binary::gamedata_layout::body("dyecolorgroupinfo"))?;
    let pabgh_file = dir.files.iter().find(|f| f.name == crate::binary::gamedata_layout::header("dyecolorgroupinfo"))?;
    let group_dir = game_root.join("0008");
    let pabgb = crate::binary::paz::extract_file(
        &group_dir,
        pabgb_file,
        crate::binary::gamedata_layout::bin_dir(),
        &pamt.header.encrypt_info.encrypt_info,
    )
    .ok()?;
    let pabgh = crate::binary::paz::extract_file(
        &group_dir,
        pabgh_file,
        crate::binary::gamedata_layout::bin_dir(),
        &pamt.header.encrypt_info.encrypt_info,
    )
    .ok()?;
    Some((pabgb, pabgh))
}

/// Parse the post-name palette body. Returns `None` when the body is
/// too short or `color_count` is unreasonable — both are treated as
/// anchor noise (row stays in the entry list but with an empty
/// palette, mirroring the parser's "lossy" contract).
fn parse_palette_after_name(body: &[u8], after_name_off: usize) -> Option<Vec<[u8; 4]>> {
    let flag_off = after_name_off;
    let count_off = flag_off + 1;
    if count_off + 4 > body.len() {
        return None;
    }
    let color_count = u32::from_le_bytes([
        body[count_off],
        body[count_off + 1],
        body[count_off + 2],
        body[count_off + 3],
    ]) as usize;
    if color_count > 1024 {
        return None;
    }
    let data_off = count_off + 4;
    if data_off + 8 * color_count > body.len() {
        return None;
    }
    let mut palette = Vec::with_capacity(color_count);
    for i in 0..color_count {
        let off = data_off + 8 * i;
        // Stored order: [B, G, R, A, ...]. Swap to logical (R, G, B, A).
        palette.push([body[off + 2], body[off + 1], body[off], body[off + 3]]);
    }
    Some(palette)
}

#[cfg(test)]
mod tests {
    //! Live-install integration test against the real
    //! `dyecolorgroupinfo.pabgb` + `.pabgh`. Pins the 10 row keys
    //! verified during the 2026-05-16 RE pass so a future patch
    //! changing the row layout is caught immediately. Skips cleanly
    //! when the game install isn't present.

    use super::*;

    /// (key, expected internal name) — all 10 rows from the live 1.07
    /// install. Index order matches the on-disk PABGH order.
    const KNOWN: &[(u32, &str)] = &[
        (0xc88211f5, "Her_Color_Group_I"),
        (0xdc274476, "Dem_Color_Group_I"),
        (0x068f0cce, "Dem_Color_Group_II"),
        (0x40707e94, "Dem_Color_Group_III"),
        (0x001835e0, "Kwe_Color_Group_I"),
        (0xa7ec4d9b, "Del_Color_Group_I"),
        (0x2d0517c9, "Cal_Color_Group_I"),
        (0x2a85f874, "Por_Color_Group_I"),
        (0x4f40e9d2, "Tom_Color_Group_I"),
        (0x47564f94, "Bar_Color_Group_I"),
    ];

    fn extract_pair() -> Option<(Vec<u8>, Vec<u8>)> {
        super::extract_pair_for_tests()
    }

    /// Dye consumable → applied RGBA investigation, phase 1.
    ///
    /// Hypothesis: the 109-position gradient stored on each
    /// `Color_Group` row is exactly the set of selectable colors the
    /// in-game NPC dye menu offers under that theme. If true, the
    /// observed slot103 RGBs (the user's applied dyes) must hit
    /// *exact* positions inside the gradient — no interpolation.
    ///
    /// This probe extracts the raw color_data bytes for
    /// `Her_Color_Group_I` + `Por_Color_Group_I`, walks the 109
    /// records, and checks whether each of the 11 slot103-observed
    /// RGBs (6 Hernand-reds + 5 Pororin-olives, from the
    /// `_probe_item_dye_data_with_mercenary_resolution` dump) lands
    /// on an exact position. Successful match → the gradient is the
    /// dye-picker source-of-truth.
    ///
    /// Run: `cargo test --lib --features c_abi
    ///       dye_gradient_vs_slot103_rgbs -- --ignored --nocapture`
    /// (no c_abi gate strictly needed — it's a pure parser probe.)
    #[test]
    #[ignore = "investigation only — dye gradient vs slot103 observed RGBs"]
    fn dye_gradient_vs_slot103_rgbs() {
        let Some((pabgb, pabgh)) = extract_pair() else {
            eprintln!("skipping: no game install");
            return;
        };
        // Re-walk the PABGH index manually so we can pull the
        // gradient bytes for individual rows.
        let index = crate::skill_info::parse_pabgh(&pabgh).expect("parse pabgh");
        let ranges = crate::skill_info::entry_ranges(&index, pabgb.len());

        // Target keys + their slot103-observed RGBs from the dye probe.
        type Target = (u32, &'static str, &'static [(u8, u8, u8)]);
        let targets: &[Target] = &[
            (
                0xc88211f5,
                "Her_Color_Group_I",
                &[
                    (0xa6, 0x57, 0x57),  // #a65757
                    (0xf2, 0x21, 0x21),  // #f22121
                    (0xd9, 0x85, 0x85),  // #d98585
                    (0xd9, 0x99, 0x99),  // #d99999
                    (0x59, 0x44, 0x44),  // #594444
                    (0xa6, 0x48, 0x48),  // #a64848
                ],
            ),
            (
                0x2a85f874,
                "Por_Color_Group_I",
                &[
                    (0x73, 0x6e, 0x3f),  // #736e3f
                    (0x40, 0x39, 0x13),  // #403913
                    (0x59, 0x54, 0x2a),  // #59542a
                    (0x73, 0x6a, 0x15),  // #736a15
                    (0x8c, 0x85, 0x30),  // #8c8530
                ],
            ),
        ];

        for &(target_key, target_name, observed_rgbs) in targets {
            // Find the row body for target_key.
            let Some((_, (start, end))) = index.iter().zip(ranges.iter()).find(|(e, _)| e.key == target_key)
            else {
                eprintln!("\n[{target_name}] row not found");
                continue;
            };
            let body = &pabgb[*start..*end];
            if body.len() < 12 { continue; }
            let key = u32::from_le_bytes([body[0], body[1], body[2], body[3]]);
            assert_eq!(key, target_key);
            let name_len = u32::from_le_bytes([body[4], body[5], body[6], body[7]]) as usize;
            // After name + 1-byte flag we hit color_count then 8*N bytes.
            let after_name = 8 + name_len;
            let flag = body[after_name];
            let color_count_off = after_name + 1;
            let color_count = u32::from_le_bytes([
                body[color_count_off],
                body[color_count_off + 1],
                body[color_count_off + 2],
                body[color_count_off + 3],
            ]) as usize;
            let color_data_off = color_count_off + 4;
            eprintln!(
                "\n[{target_name}] key=0x{key:08x} name_len={name_len} flag={flag} color_count={color_count} color_data_off={color_data_off}",
            );

            // Walk every position and check exact match.
            for (rgb_idx, (r, g, b)) in observed_rgbs.iter().enumerate() {
                let mut found_at: Option<usize> = None;
                for i in 0..color_count {
                    let off = color_data_off + i * 8;
                    if off + 4 > body.len() { break; }
                    let pr = body[off];
                    let pg = body[off + 1];
                    let pb = body[off + 2];
                    if pr == *r && pg == *g && pb == *b {
                        found_at = Some(i);
                        break;
                    }
                }
                match found_at {
                    Some(i) => {
                        // Dump the full 8-byte record for context.
                        let off = color_data_off + i * 8;
                        let rec = &body[off..off + 8];
                        eprintln!(
                            "  observed[{rgb_idx}] #{r:02x}{g:02x}{b:02x} -> position {i:>3} (record bytes {rec:02x?})",
                        );
                    }
                    None => {
                        // Find the closest position by L1 distance for diagnostic.
                        let mut best = (usize::MAX, u32::MAX, [0u8; 3]);
                        for i in 0..color_count {
                            let off = color_data_off + i * 8;
                            if off + 3 > body.len() { break; }
                            let pr = body[off];
                            let pg = body[off + 1];
                            let pb = body[off + 2];
                            let d = (pr as i32 - *r as i32).unsigned_abs()
                                + (pg as i32 - *g as i32).unsigned_abs()
                                + (pb as i32 - *b as i32).unsigned_abs();
                            if d < best.1 { best = (i, d, [pr, pg, pb]); }
                        }
                        eprintln!(
                            "  observed[{rgb_idx}] #{r:02x}{g:02x}{b:02x} -> NO EXACT MATCH (closest position {} L1={} RGB=#{:02x}{:02x}{:02x})",
                            best.0, best.1, best.2[0], best.2[1], best.2[2],
                        );
                    }
                }
            }

            // Dump every gradient position so we can eyeball what
            // the 109 records actually contain.
            eprintln!("  all {} gradient positions:", color_count);
            for i in 0..color_count {
                let off = color_data_off + i * 8;
                if off + 8 > body.len() { break; }
                let rec = &body[off..off + 8];
                eprintln!(
                    "    [{i:>3}] RGBA=#{:02x}{:02x}{:02x}{:02x}  tail={:02x?}",
                    rec[0], rec[1], rec[2], rec[3], &rec[4..],
                );
            }
        }
    }

    #[test]
    fn dye_color_group_info_lossy_live() {
        let Some((pabgb, pabgh)) = extract_pair() else {
            eprintln!("skipping dye_color_group_info_lossy_live: no game install");
            return;
        };
        let entries = parse_dye_color_group_info_lossy(&pabgb, &pabgh);
        println!(
            "parsed {} dyecolorgroupinfo entries from {} pabgb bytes",
            entries.len(),
            pabgb.len()
        );
        // 1.07 has exactly 10 rows. Pin >= 5 as a loose floor so a
        // patch adding new color groups doesn't false-fail.
        assert!(
            entries.len() >= 5,
            "expected >=5 dyecolorgroupinfo entries, got {}",
            entries.len()
        );

        let by_key: std::collections::HashMap<u32, &str> = entries
            .iter()
            .map(|e| (e.key, e.name.as_str()))
            .collect();
        for &(key, expected) in KNOWN {
            assert_eq!(
                by_key.get(&key).copied(),
                Some(expected),
                "DyeColorGroupInfoKey 0x{key:08x} mismatch",
            );
        }
    }
}
