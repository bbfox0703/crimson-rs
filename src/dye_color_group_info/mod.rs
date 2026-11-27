//! `dyecolorgroupinfo.pabgb` parser — PABGH-indexed.
//!
//! Resolves `DyeColorGroupInfoKey (u32)` — the gamedata key stored as
//! `ItemDyeSaveData._dyeColorGroupInfoKey` in the save's
//! `_itemDyeDataList`. The 10 rows in 1.07 are the **themed color
//! palettes** the in-game NPC dye menu offers (e.g.
//! `Her_Color_Group_I` = 埃爾南德, `Por_Color_Group_I` = 波羅琳).
//!
//! ## Schema (verified against the live 2.04 install, every row, byte-perfect)
//!
//! 2.04 moved `key` after the name and `row_hash` (formerly the row's last
//! 4 bytes) up in front of `color_count`. Against 2.03, all 10 rows carry
//! the same flag, hash, palette and trailer.
//!
//! ```text
//! Row {
//!     u32 name_len,
//!     u8 name[name_len],        // ASCII, e.g. "Her_Color_Group_I"
//!     u32 key,                  // matches PABGH key
//!     u8 flag,                  // always 0
//!     u8 row_hash[4],           // per-row hash value
//!     u32 color_count,          // always 109
//!     u8 color_data[8 * color_count],  // palette (BGRA + 4-byte tail per record; 22-byte records from 2.06)
//!     // trailer:
//!     u8 trailer_marker[5],     // 23 31 02 00 00 — constant across rows
//!     u32 key_copy,             // = key
//!     u32 numeric_id_len,       // 16..=20
//!     u8 numeric_id[numeric_id_len],  // ASCII decimal digits (u64 hash serialized as text)
//! }
//! ```
//!
//! ## 2.06: 22-byte color records
//!
//! The 4-byte tail of a color record is the condition that unlocks the
//! color. Through 2.05.01 it was a `ConditionInfoKey`, e.g. 1008623 =
//! `CheckKnowledge(Knowledge_Grey_DyeColor_1)`; each row uses six (the
//! `Grey_DyeColor_<n>` one plus five of its own). 2.06 inlined every such
//! anonymous condition (see "Inlined conditions (2.06)" in `scripts/CLAUDE.md`):
//! the tail is now `01` + the condition's compiled bytecode,
//! `01 03 24 00 <u32 KnowledgeKey> 01 00 00 00 00 00 00 00 01` (18 bytes,
//! records 8 → 22), and the condition rows left `conditioninfo`. All 1,090
//! records (10 rows × 109) equal `01` + the 2.05.01 condition's bytecode, and the
//! key reads `Knowledge_Grey_DyeColor_1` (1001287) etc. So a color is unlocked
//! by a knowledge entry. Head and trailer are unchanged, and on all 10 rows the
//! trailer marker sits right after `color_count` 22-byte records, which is how
//! [`parse_row_after_key`] finds the records' extent. A different
//! condition would compile to a different length, so the parser does not
//! assume one size: it locates the trailer (marker + the row key), walks the
//! records between one by one, reading each condition as `00` (none) or `01` +
//! a `CheckKnowledge` bytecode, and only when that fails splits them into equal
//! sizes (the 8-byte records through 2.05.01). A row that fits neither gets an
//! empty palette rather than a misread one.
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

//! ## Which knowledge unlocks each color, and the theme's name (2.06)
//!
//! Each 2.06 color record's condition is `CheckKnowledge(<KnowledgeKey>)`
//! ([`PaletteCondition::Knowledge`]). On all 10 rows the layout is the same:
//! positions 0–8 (the gray ramp) need the row's own `Knowledge_Grey_DyeColor_<n>`
//! (Her 1, Dem I 2, Dem II 3, Dem III 4, Kwe 5, Del 6, Cal 7, Por 8, Tom 9,
//! Bar 10), and positions 9–108 come in five blocks of 20, each needing
//! `Knowledge_<Theme>_DyeColor_<1..5>` (`Dem` rows add the `_I` / `_II` /
//! `_III` group). So the 1,090 records name 60 knowledge rows in all: the 50
//! theme colors and the 10 grays. No `Knowledge_Collection_Prop_Dyewater_*`
//! row is referenced.
//!
//! The row head after the key is `u8 0` and a `StringInfoKey` of the theme's
//! map icon (`cd_icon_map_quest_sub_icon_01`; the three Dem rows share one),
//! not a condition. The trailer is a localizable string: `23`, `u32 0x231`,
//! the row key, and a `CString` holding `(row key << 32) | 0x231` in decimal,
//! the PALOC key of the theme's display name ("Hernandian Dye", "Demenissian
//! Dye" on all three Dem rows, "Pailunese Dye" for `Kwe`)
//! ([`DyeColorGroupInfoEntry::display_name_paloc_key`]).
//!
//! **Not established**: what makes a theme appear at a dyer. The theme
//! knowledge rows `Knowledge_<Theme>_DyeColor` (2147483445..454, titled with
//! the dyer's name) are not referenced by this table; `conditioninfo` keeps a
//! `CheckKnowledge(Knowledge_<Theme>_DyeColor)` row for each, but a byte scan
//! cannot show which table uses those (their small keys collide with other
//! `u32` values everywhere).

use crate::binary::row_head::read_row_head;

/// The unlock condition of one palette color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteCondition {
    /// No condition (`00`): always available.
    None,
    /// 2.06: an inlined `CheckKnowledge(<KnowledgeKey>)`.
    Knowledge(u32),
    /// Through 2.05.01: a `ConditionInfoKey` (the condition row itself is not
    /// read here).
    ConditionInfoKey(u32),
    /// A condition this parser does not decode.
    Unknown,
}

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
    /// The unlock condition of each palette position, parallel to `palette`.
    pub conditions: Vec<PaletteCondition>,
    /// The PALOC key (decimal) of the theme's display name, from the row's
    /// trailer: `(key << 32) | 0x231` on every 2.06 row.
    pub display_name_paloc_key: Option<String>,
}

/// Parse `dyecolorgroupinfo.pabgb` using its `.pabgh` index.
///
/// Returns entries in PABGH on-disk order. Skips any row whose body
/// is too short to contain `[u32 name_len][name_len bytes][u32 key]`
/// or whose name bytes are not valid UTF-8 — both treated as anchor
/// noise rather than panics so a future patch with new row shapes
/// doesn't break the build.
pub fn parse_dye_color_group_info_lossy(
    pabgb: &[u8],
    pabgh: &[u8],
) -> Vec<DyeColorGroupInfoEntry> {
    // 2.06 moved row names into the header; rebuild the pre-2.06 layout.
    let (pabgb, pabgh) = crate::binary::static_header::legacy_pair(pabgb, pabgh);
    let (pabgb, pabgh): (&[u8], &[u8]) = (&pabgb, &pabgh);
    let Ok(index) = crate::skill_info::parse_pabgh(pabgh) else {
        return Vec::new();
    };
    let ranges = crate::skill_info::entry_ranges(&index, pabgb.len());
    let mut out = Vec::with_capacity(index.len());
    for (entry, (start, end)) in index.iter().zip(ranges.iter()) {
        let Some(body) = pabgb.get(*start..*end) else {
            continue;
        };
        let Some(head) = read_row_head(body, 4) else {
            continue;
        };
        if head.key != entry.key || !(1..=128).contains(&head.name.len()) {
            // PABGH/PABGB mismatch — refuse the row rather than emit
            // garbage. A future patch could change the row prefix; the
            // bridge surface treats this as "row missing".
            continue;
        }

        // Parse the 109-record palette body.
        // Layout after the key: `u8 flag + 4-byte hash + u32 color_count +
        // 8 * color_count bytes`.
        // Bytes per record: [B, G, R, A, tail0..tail3]; we keep only the
        // logical RGBA (post-swap) so palette comparisons against the
        // save's u8 _dyeColorR/G/B/A are direct.
        let row = parse_row_after_key(body, head.body_at).unwrap_or_default();

        out.push(DyeColorGroupInfoEntry {
            key: head.key,
            name: head.name.to_owned(),
            palette: row.palette,
            conditions: row.conditions,
            display_name_paloc_key: row.display_name_paloc_key,
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

/// Parse the post-key palette body. Returns `None` when the body is
/// too short or `color_count` is unreasonable — both are treated as
/// anchor noise (row stays in the entry list but with an empty
/// palette, mirroring the parser's "lossy" contract).
#[cfg(test)]
fn parse_palette_after_key(body: &[u8], after_key_off: usize) -> Option<Vec<[u8; 4]>> {
    parse_row_after_key(body, after_key_off).map(|r| r.palette)
}

/// What [`parse_row_after_key`] reads after the row key.
#[derive(Debug, Default)]
struct RowBody {
    palette: Vec<[u8; 4]>,
    conditions: Vec<PaletteCondition>,
    display_name_paloc_key: Option<String>,
}

fn parse_row_after_key(body: &[u8], after_key_off: usize) -> Option<RowBody> {
    // `u8 flag + 4-byte row hash`.
    let count_off = after_key_off + 5;
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
    // The trailer opens with a constant marker and the row key again; the
    // records fill everything between `data_off` and it.
    let key = body.get(after_key_off.checked_sub(4)?..after_key_off)?;
    let mut needle = TRAILER_MARKER.to_vec();
    needle.extend_from_slice(key);
    let trailer = data_off
        + body
            .get(data_off..)?
            .windows(needle.len())
            .position(|w| w == needle.as_slice())?;
    let (starts, walked) = record_starts(body, data_off, trailer, color_count)?;
    // Stored order: [B, G, R, A, ...]. Swap to logical (R, G, B, A).
    let palette = starts.iter().map(|&o| [body[o + 2], body[o + 1], body[o], body[o + 3]]).collect();
    let stride = if walked || starts.len() < 2 { 0 } else { starts[1] - starts[0] };
    let u32_at = |o: usize| body.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let conditions = starts
        .iter()
        .map(|&o| match (walked, body.get(o + 4)) {
            (true, Some(0)) => PaletteCondition::None,
            (true, Some(1)) => u32_at(o + 9).map_or(PaletteCondition::Unknown, PaletteCondition::Knowledge),
            (false, _) if stride == 8 => u32_at(o + 4).map_or(PaletteCondition::Unknown, PaletteCondition::ConditionInfoKey),
            _ => PaletteCondition::Unknown,
        })
        .collect();
    // Trailer: marker (`23` + `u32 0x231`), the row key, then a CString.
    let name_at = trailer + needle.len();
    let display_name_paloc_key = u32_at(name_at).and_then(|len| {
        let s = body.get(name_at + 4..name_at + 4 + len as usize)?;
        (!s.is_empty() && s.iter().all(u8::is_ascii_digit)).then(|| String::from_utf8_lossy(s).into_owned())
    });
    Some(RowBody { palette, conditions, display_name_paloc_key })
}

/// First bytes of a dye-color trailer: `23 31 02 00 00`, then the row key.
const TRAILER_MARKER: [u8; 5] = [0x23, 0x31, 0x02, 0x00, 0x00];

/// The start of each of the `count` color records in `body[from..to]`: a
/// record-by-record walk over `BGRA` + an inline condition (2.06) first, then
/// equal sizes (the 8-byte records through 2.05.01, or any one size). `None`
/// when neither fits. The flag says whether the walk (inline conditions)
/// placed them.
fn record_starts(body: &[u8], from: usize, to: usize, count: usize) -> Option<(Vec<usize>, bool)> {
    let span = to.checked_sub(from)?;
    if count == 0 {
        return (span == 0).then(|| (Vec::new(), false));
    }
    let walk = || {
        let mut starts = Vec::with_capacity(count);
        let mut at = from;
        for _ in 0..count {
            starts.push(at);
            at += 4 + inline_condition_len(body.get(at + 4..to)?)?;
        }
        (at == to).then_some(starts)
    };
    walk().map(|s| (s, true)).or_else(|| {
        (span % count == 0 && span / count >= 4)
            .then(|| ((0..count).map(|i| from + i * (span / count)).collect(), false))
    })
}

/// Length of a 2.06 inline condition at the start of `b`: `00` (none) or `01`
/// followed by the 17-byte `CheckKnowledge` bytecode
/// `01 03 24 00 <u32 KnowledgeKey> 01 00 00 00 00 00 00 00 01`, the only form
/// the dye table uses. Other bytecode is not decoded (`None`).
fn inline_condition_len(b: &[u8]) -> Option<usize> {
    const TAIL: [u8; 9] = [1, 0, 0, 0, 0, 0, 0, 0, 1];
    match b.first()? {
        0 => Some(1),
        1 if b.get(1..5) == Some(&[1, 3, 0x24, 0][..]) && b.get(9..18) == Some(&TAIL[..]) => Some(18),
        _ => None,
    }
}

#[cfg(test)]
mod record_tests {
    use super::*;

    /// A row in the shape the parser reads: name len 0, key, flag, hash, count,
    /// records, trailer.
    fn row(records: &[Vec<u8>]) -> (Vec<u8>, usize) {
        let key = 0xc882_11f5u32.to_le_bytes();
        let mut b = 0u32.to_le_bytes().to_vec();
        b.extend_from_slice(&key);
        let after_key = b.len();
        b.push(0);
        b.extend_from_slice(&[0x0e, 0x62, 0x0e, 0x6d]);
        b.extend_from_slice(&(records.len() as u32).to_le_bytes());
        for r in records {
            b.extend_from_slice(r);
        }
        b.extend_from_slice(&TRAILER_MARKER);
        b.extend_from_slice(&key);
        b.extend_from_slice(&[2, 0, 0, 0, b'4', b'2']);
        (b, after_key)
    }

    fn knowledge(bgra: [u8; 4], key: u32) -> Vec<u8> {
        let mut r = bgra.to_vec();
        r.extend_from_slice(&[1, 1, 3, 0x24, 0]);
        r.extend_from_slice(&key.to_le_bytes());
        r.extend_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0, 1]);
        r
    }

    #[test]
    fn reads_uniform_and_mixed_records() {
        // 2.06: every record 22 bytes.
        let (b, k) = row(&[knowledge([3, 2, 1, 255], 1_001_287), knowledge([6, 5, 4, 255], 1_000_294)]);
        assert_eq!(parse_palette_after_key(&b, k).unwrap(), [[1, 2, 3, 255], [4, 5, 6, 255]]);
        // Pre-2.06: 8-byte records (BGRA + u32 ConditionInfoKey).
        let (b, k) = row(&[vec![3, 2, 1, 255, 0xef, 0x63, 0x0f, 0], vec![6, 5, 4, 255, 0xf5, 0x63, 0x0f, 0]]);
        assert_eq!(parse_palette_after_key(&b, k).unwrap(), [[1, 2, 3, 255], [4, 5, 6, 255]]);
        // Mixed sizes: a color without a condition (`00`) next to two gated ones.
        let (b, k) = row(&[
            knowledge([3, 2, 1, 255], 1),
            vec![9, 8, 7, 255, 0],
            knowledge([6, 5, 4, 255], 2),
        ]);
        assert_eq!(
            parse_palette_after_key(&b, k).unwrap(),
            [[1, 2, 3, 255], [7, 8, 9, 255], [4, 5, 6, 255]]
        );
    }

    #[test]
    fn reads_conditions_and_name_key() {
        // 2.06: a gated color, an ungated one.
        let (b, k) = row(&[knowledge([3, 2, 1, 255], 1_001_287), vec![9, 8, 7, 255, 0]]);
        let r = parse_row_after_key(&b, k).unwrap();
        assert_eq!(r.conditions, [PaletteCondition::Knowledge(1_001_287), PaletteCondition::None]);
        assert_eq!(r.display_name_paloc_key.as_deref(), Some("42"));
        // Pre-2.06: 8-byte records carry a ConditionInfoKey.
        let (b, k) = row(&[vec![3, 2, 1, 255, 0xef, 0x63, 0x0f, 0], vec![6, 5, 4, 255, 0xf5, 0x63, 0x0f, 0]]);
        let r = parse_row_after_key(&b, k).unwrap();
        assert_eq!(
            r.conditions,
            [PaletteCondition::ConditionInfoKey(0x000f_63ef), PaletteCondition::ConditionInfoKey(0x000f_63f5)]
        );
    }

    #[test]
    fn refuses_a_condition_it_cannot_walk() {
        let mut other = knowledge([6, 5, 4, 255], 2);
        other[6] = 0x27; // a different function: not walked
        other.push(0); // and a size that no longer splits evenly
        let (b, k) = row(&[knowledge([3, 2, 1, 255], 1), other, vec![9, 8, 7, 255, 0]]);
        assert_eq!(parse_palette_after_key(&b, k), None);
    }
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
            let head = read_row_head(body, 4).expect("row head");
            assert_eq!(head.key, target_key);
            // After key + 1-byte flag + 4-byte hash we hit color_count then 8*N bytes.
            let flag = body[head.body_at];
            let color_count_off = head.body_at + 5;
            let color_count = u32::from_le_bytes([
                body[color_count_off],
                body[color_count_off + 1],
                body[color_count_off + 2],
                body[color_count_off + 3],
            ]) as usize;
            let color_data_off = color_count_off + 4;
            eprintln!(
                "\n[{target_name}] key=0x{target_key:08x} flag={flag} color_count={color_count} color_data_off={color_data_off}",
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
