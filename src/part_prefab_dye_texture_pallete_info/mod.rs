//! `partprefabdyetexturepalleteinfo.pabgb` parser — custom PABGH.
//!
//! Resolves `PartPrefabDyeTexturePalleteKey (u16)` — the gamedata key
//! stored as `ItemDyeSaveData._texturePalleteKey` in the save's
//! `_itemDyeDataList`. There are 11 rows in 1.07 (`key ∈ 0..=10`);
//! each row defines a "palette" — a set of 2 or 3 materials (cloth /
//! leather / metal etc.) with paired icon + texture DDS paths.
//!
//! The PA/Crimson Desert in-game dye UI groups these as "material
//! variants" — choosing a different `_texturePalleteKey` swaps the
//! texture tier visually (Cloth 5001 → Cloth 5005 etc.). The bridge
//! exposes the full per-row sub-record list so the C# editor can
//! render a richer "palette tier" dropdown than a flat material name
//! would allow.
//!
//! **PABGH layout** — custom 6-byte entries (does NOT match the
//! standard skill / dye_color_group `u16 count + (u32 key, u32 offset)*`
//! 10-byte shape):
//!
//! ```text
//! u16 count, then count × { u16 key, u32 offset }
//! ```
//!
//! For 11 entries: 2 + 11 × 6 = 68 bytes (matches the live file's
//! 68-byte `.pabgh`). The u16 key matches the save's `_texturePalleteKey`
//! u16 type 1:1.
//!
//! **PABGB row layout** (verified byte-exact on all 11 rows in 1.07):
//!
//! ```text
//! Row {
//!     u32 key,                 // PABGH key extended to u32
//!     u8 pad[3],               // = 00 00 00
//!     u32 key_copy,            // = key
//!     u32 sub_count,           // 2 for key=0, 3 for key=1..10
//!     Sub sub[sub_count],
//! }
//! Sub {
//!     CString material_name,   // "cloth"/"leather"/"metal"/"wool"/"velvet"/"silk"
//!     CString icon_path,       // "ui/.../itemicon_*.dds" — on key=0 this is a
//!                              //   duplicate of texture_path (no UI icon)
//!     CString texture_path,    // "character/texture/cd_texturelayer_*.dds"
//!     CString variant_name,    // empty by default, or "wool"/"velvet"/"silk"
//!     f32 variant_value,       // -1.0 default; positive when variant_name set
//! }
//! ```
//!
//! Each `CString` here is `[u32 len][len bytes]` with NO trailing NUL —
//! the project's standard `CString` shape used by iteminfo.pabgb.

/// One sub-record inside a palette row.
#[derive(Debug, Clone)]
pub struct PaletteSub {
    /// Material identifier — e.g. `"cloth"`, `"leather"`, `"metal"`.
    pub material_name: String,
    /// Path to the UI icon DDS. For `key=0` (the "default" palette)
    /// this is the same string as `texture_path` — the game falls
    /// back to the texture itself when no icon is provided.
    pub icon_path: String,
    /// Path to the material texture DDS used at runtime.
    pub texture_path: String,
    /// Optional variant label inside the material — e.g. `"wool"`
    /// inside a cloth sub, `"velvet"` / `"silk"` inside cloth rows
    /// for higher palette tiers. Empty string when absent.
    pub variant_name: String,
    /// Variant strength as a float. `-1.0` is the "no variant"
    /// sentinel; positive values (around `0.1..0.4`) appear together
    /// with a non-empty `variant_name`.
    pub variant_value: f32,
}

/// One palette row.
#[derive(Debug, Clone)]
pub struct PartPrefabDyeTexturePalleteEntry {
    /// `PartPrefabDyeTexturePalleteKey` as stored in
    /// `ItemDyeSaveData._texturePalleteKey` (u16 in the save; u32 here
    /// to match the PABGB row header — the high bits are always zero).
    pub key: u32,
    /// Sub-records describing each material variant within the palette.
    pub subs: Vec<PaletteSub>,
}

/// PABGH-equivalent index entry for `partprefabdyetexturepalleteinfo`.
/// Distinct from `SkillIndexEntry` because the keys here are u16, not u32.
#[derive(Debug, Clone, Copy)]
pub struct PaletteIndexEntry {
    pub key: u16,
    pub offset: u32,
}

/// Parse the custom 6-byte-per-entry PABGH layout. Returns `None`
/// when the file's count header would overflow the buffer.
pub fn parse_palette_pabgh(pabgh: &[u8]) -> Option<Vec<PaletteIndexEntry>> {
    if pabgh.len() < 2 {
        return None;
    }
    let count = u16::from_le_bytes([pabgh[0], pabgh[1]]) as usize;
    let needed = 2 + count * 6;
    if pabgh.len() < needed {
        return None;
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let o = 2 + i * 6;
        let key = u16::from_le_bytes([pabgh[o], pabgh[o + 1]]);
        let offset = u32::from_le_bytes([
            pabgh[o + 2],
            pabgh[o + 3],
            pabgh[o + 4],
            pabgh[o + 5],
        ]);
        out.push(PaletteIndexEntry { key, offset });
    }
    Some(out)
}

/// Compute `(start, end)` body ranges for each palette row, given
/// the index. End-of-row is taken from the *next-by-offset* row's
/// offset (or the PABGB length for the last row on disk).
fn palette_entry_ranges(
    entries: &[PaletteIndexEntry],
    pabgb_len: usize,
) -> Vec<(usize, usize)> {
    let mut by_offset: Vec<usize> = (0..entries.len()).collect();
    by_offset.sort_by_key(|&i| entries[i].offset);
    let mut ends = vec![pabgb_len; entries.len()];
    for win in by_offset.windows(2) {
        ends[win[0]] = entries[win[1]].offset as usize;
    }
    entries
        .iter()
        .enumerate()
        .map(|(i, e)| (e.offset as usize, ends[i]))
        .collect()
}

/// Read a `[u32 len][len bytes]` CString from `data` starting at
/// `*cursor`, advancing the cursor past the consumed bytes. Returns
/// `None` if the buffer is too short.
fn read_cstring(data: &[u8], cursor: &mut usize) -> Option<String> {
    if *cursor + 4 > data.len() {
        return None;
    }
    let len = u32::from_le_bytes([
        data[*cursor],
        data[*cursor + 1],
        data[*cursor + 2],
        data[*cursor + 3],
    ]) as usize;
    *cursor += 4;
    if *cursor + len > data.len() {
        return None;
    }
    let s = std::str::from_utf8(&data[*cursor..*cursor + len]).ok()?.to_owned();
    *cursor += len;
    Some(s)
}

/// Parse one palette row's body. Returns `None` if the body deviates
/// from the verified 1.07 schema (e.g. truncated CString, wrong header
/// shape, sub_count overflow).
fn parse_row(body: &[u8], expected_key: u32) -> Option<PartPrefabDyeTexturePalleteEntry> {
    if body.len() < 15 {
        return None;
    }
    let key = u32::from_le_bytes([body[0], body[1], body[2], body[3]]);
    if key != expected_key {
        return None;
    }
    // bytes 4..7 are padding zeros — we don't enforce, just skip.
    let key_copy = u32::from_le_bytes([body[7], body[8], body[9], body[10]]);
    if key_copy != key {
        return None;
    }
    let sub_count = u32::from_le_bytes([body[11], body[12], body[13], body[14]]) as usize;
    if sub_count == 0 || sub_count > 16 {
        return None;
    }

    let mut cursor = 15;
    let mut subs = Vec::with_capacity(sub_count);
    for _ in 0..sub_count {
        let material_name = read_cstring(body, &mut cursor)?;
        let icon_path = read_cstring(body, &mut cursor)?;
        let texture_path = read_cstring(body, &mut cursor)?;
        let variant_name = read_cstring(body, &mut cursor)?;
        if cursor + 4 > body.len() {
            return None;
        }
        let variant_value = f32::from_le_bytes([
            body[cursor],
            body[cursor + 1],
            body[cursor + 2],
            body[cursor + 3],
        ]);
        cursor += 4;
        subs.push(PaletteSub {
            material_name,
            icon_path,
            texture_path,
            variant_name,
            variant_value,
        });
    }
    // Spec says cursor should == body.len() now. Trailing bytes would
    // signal schema drift — return None so callers see "row missing"
    // rather than silently-truncated data.
    if cursor != body.len() {
        return None;
    }

    Some(PartPrefabDyeTexturePalleteEntry { key, subs })
}

/// Parse `partprefabdyetexturepalleteinfo.pabgb` using its `.pabgh`
/// index. Returns entries in PABGH on-disk order. Rows that fail to
/// parse against the verified schema are silently dropped (the bridge
/// surfaces them as `NOT_FOUND` on lookup).
pub fn parse_part_prefab_dye_texture_pallete_info_lossy(
    pabgb: &[u8],
    pabgh: &[u8],
) -> Vec<PartPrefabDyeTexturePalleteEntry> {
    let Some(index) = parse_palette_pabgh(pabgh) else {
        return Vec::new();
    };
    let ranges = palette_entry_ranges(&index, pabgb.len());
    let mut out = Vec::with_capacity(index.len());
    for (entry, (start, end)) in index.iter().zip(ranges.iter()) {
        let Some(body) = pabgb.get(*start..*end) else {
            continue;
        };
        if let Some(parsed) = parse_row(body, entry.key as u32) {
            out.push(parsed);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    //! Live-install integration test. Pins enough invariants to catch
    //! schema drift in future patches: every key 0..=10 must parse;
    //! `key=0` has 2 subs; `key=1..=10` have 3 subs each; the materials
    //! in `key=1` are `cloth/leather/metal` in that order; the variant
    //! `"wool"` on `key=1` cloth sub has a positive variant_value.

    use super::*;
    use std::path::PathBuf;

    fn extract_pair() -> Option<(Vec<u8>, Vec<u8>)> {
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
        let pabgb_file = dir
            .files
            .iter()
            .find(|f| f.name == "partprefabdyetexturepalleteinfo.pabgb")?;
        let pabgh_file = dir
            .files
            .iter()
            .find(|f| f.name == "partprefabdyetexturepalleteinfo.pabgh")?;
        let group_dir = game_root.join("0008");
        let pabgb = crate::binary::paz::extract_file(
            &group_dir,
            pabgb_file,
            "gamedata/binary__/client/bin",
            &pamt.header.encrypt_info.encrypt_info,
        )
        .ok()?;
        let pabgh = crate::binary::paz::extract_file(
            &group_dir,
            pabgh_file,
            "gamedata/binary__/client/bin",
            &pamt.header.encrypt_info.encrypt_info,
        )
        .ok()?;
        Some((pabgb, pabgh))
    }

    #[test]
    fn palette_pabgh_parses_custom_six_byte_entries() {
        // Mirror of the live file's 68-byte .pabgh.
        let pabgh = vec![
            0x0b, 0x00, // count = 11
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // key=0, off=0
            0x01, 0x00, 0xfb, 0x00, 0x00, 0x00, // key=1, off=251
            0x02, 0x00, 0x84, 0x02, 0x00, 0x00, // key=2, off=644
            0x03, 0x00, 0x0d, 0x04, 0x00, 0x00, // key=3, off=1037
            0x04, 0x00, 0x96, 0x05, 0x00, 0x00, // key=4, off=1430
            0x05, 0x00, 0x1f, 0x07, 0x00, 0x00, // key=5, off=1823
            0x06, 0x00, 0xaa, 0x08, 0x00, 0x00, // key=6, off=2218
            0x07, 0x00, 0x35, 0x0a, 0x00, 0x00, // key=7, off=2613
            0x08, 0x00, 0xc0, 0x0b, 0x00, 0x00, // key=8, off=3008
            0x09, 0x00, 0x49, 0x0d, 0x00, 0x00, // key=9, off=3401
            0x0a, 0x00, 0xd2, 0x0e, 0x00, 0x00, // key=10, off=3794
        ];
        let entries = parse_palette_pabgh(&pabgh).expect("parse pabgh");
        assert_eq!(entries.len(), 11);
        assert_eq!(entries[0].key, 0);
        assert_eq!(entries[0].offset, 0);
        assert_eq!(entries[10].key, 10);
        assert_eq!(entries[10].offset, 3794);
    }

    #[test]
    fn part_prefab_dye_texture_pallete_info_lossy_live() {
        let Some((pabgb, pabgh)) = extract_pair() else {
            eprintln!(
                "skipping part_prefab_dye_texture_pallete_info_lossy_live: no game install"
            );
            return;
        };
        let entries = parse_part_prefab_dye_texture_pallete_info_lossy(&pabgb, &pabgh);
        println!(
            "parsed {} palette entries from {} pabgb bytes",
            entries.len(),
            pabgb.len()
        );
        // 1.07 has exactly 11 rows; pin >=11 since the schema is so
        // narrow we expect any patch to keep all of them.
        assert_eq!(
            entries.len(),
            11,
            "expected 11 palette entries (keys 0..=10), got {}",
            entries.len()
        );

        // Key invariants: keys are exactly 0..=10 in on-disk order.
        for (i, e) in entries.iter().enumerate() {
            assert_eq!(
                e.key, i as u32,
                "palette entry at index {} has key {}, expected {}",
                i, e.key, i
            );
        }

        // key=0 has 2 sub-records.
        assert_eq!(entries[0].subs.len(), 2, "key=0 should have 2 subs");
        // key=0's icon_path == texture_path (the "no UI icon" fallback).
        assert_eq!(
            entries[0].subs[0].icon_path, entries[0].subs[0].texture_path,
            "key=0 sub[0] icon_path should fall back to texture_path",
        );

        // key=1..=10 have 3 sub-records each (cloth / leather / metal).
        for (i, e) in entries.iter().enumerate().skip(1) {
            assert_eq!(e.subs.len(), 3, "key={i} should have 3 subs");
            assert_eq!(
                e.subs[0].material_name, "cloth",
                "key={i} sub[0] should be cloth",
            );
            assert_eq!(
                e.subs[1].material_name, "leather",
                "key={i} sub[1] should be leather",
            );
            assert_eq!(
                e.subs[2].material_name, "metal",
                "key={i} sub[2] should be metal",
            );
        }

        // Spot-check the variant: key=1 sub[0] (cloth) has variant_name
        // "wool" with positive variant_value; sub[1] (leather) has no
        // variant (empty name, value -1.0).
        let k1 = &entries[1];
        assert_eq!(k1.subs[0].variant_name, "wool");
        assert!(
            k1.subs[0].variant_value > 0.0 && k1.subs[0].variant_value < 1.0,
            "key=1 sub[0] (cloth+wool) variant_value should be (0,1); got {}",
            k1.subs[0].variant_value
        );
        assert_eq!(k1.subs[1].variant_name, "");
        assert!(
            (k1.subs[1].variant_value + 1.0).abs() < 1e-6,
            "key=1 sub[1] (leather) variant_value should be ~-1.0; got {}",
            k1.subs[1].variant_value
        );

        // Spot-check texture_path shape: each non-zero key references
        // `cd_texturelayer_*.dds` files under `character/texture/`.
        for e in &entries[1..] {
            for s in &e.subs {
                assert!(
                    s.texture_path.starts_with("character/texture/cd_texturelayer_"),
                    "key={} sub material={} texture_path looks wrong: {:?}",
                    e.key, s.material_name, s.texture_path,
                );
                assert!(
                    s.texture_path.ends_with(".dds"),
                    "key={} sub texture_path should end .dds: {:?}",
                    e.key, s.texture_path,
                );
            }
        }
    }
}
