//! `dyecolorgroupinfo.pabgb` parser — PABGH-indexed.
//!
//! Resolves `DyeColorGroupInfoKey (u32)` — the gamedata key stored as
//! `ItemDyeSaveData._dyeColorGroupInfoKey` in the save's
//! `_itemDyeDataList`. The 10 rows in 1.07 are the "named dye families"
//! that the in-game dye UI exposes alongside the freeform RGB picker
//! (e.g. `Her_Color_Group_I`, `Dem_Color_Group_I/II/III`).
//!
//! Schema (verified against the live 1.07 install, every row, byte-perfect):
//!
//! ```text
//! Row {
//!     u32 key,                  // matches PABGH key
//!     u32 name_len,
//!     u8 name[name_len],        // ASCII, e.g. "Her_Color_Group_I"
//!     u8 flag,                  // always 0
//!     u32 color_count,          // always 109 in 1.07
//!     u8 color_data[8 * color_count],  // palette gradient (RGBA + 4 more bytes)
//!     // 37-byte trailer:
//!     u8 trailer_marker[5],     // 23 31 02 00 00 — constant across rows
//!     u32 key_copy,             // = key
//!     u32 numeric_id_len,       // always 20
//!     u8 numeric_id[20],        // ASCII decimal digits (u64 hash serialized as text)
//!     u8 trailing_hash[4],      // per-row hash value
//! }
//! ```
//!
//! The bridge only needs (key, name) — the 909-byte palette body is the
//! gradient data the dye UI displays. Other consumers (e.g. a future
//! palette-preview UI) can re-parse the body from the same handle.
//!
//! The file ships with a matching `dyecolorgroupinfo.pabgh` so the
//! row offsets are explicit — we don't need anchor-scanning here. The
//! parser uses `skill_info::parse_pabgh` since the layout is identical
//! (`u16 count + (u32 key, u32 offset)*`).

/// One parsed dye color group row, reduced to what the bridge consumes.
#[derive(Debug, Clone)]
pub struct DyeColorGroupInfoEntry {
    /// `DyeColorGroupInfoKey` as stored in `ItemDyeSaveData._dyeColorGroupInfoKey`.
    pub key: u32,
    /// ASCII internal name (e.g. `"Her_Color_Group_I"`,
    /// `"Dem_Color_Group_III"`). The dye UI shows a localized version
    /// of this — extending PALOC lookup is a v2 task; for v1 the C#
    /// editor can render the raw internal name.
    pub name: String,
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
        out.push(DyeColorGroupInfoEntry {
            key,
            name: name.to_owned(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    //! Live-install integration test against the real
    //! `dyecolorgroupinfo.pabgb` + `.pabgh`. Pins the 10 row keys
    //! verified during the 2026-05-16 RE pass so a future patch
    //! changing the row layout is caught immediately. Skips cleanly
    //! when the game install isn't present.

    use super::*;
    use std::path::PathBuf;

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
        let pabgb_file = dir.files.iter().find(|f| f.name == "dyecolorgroupinfo.pabgb")?;
        let pabgh_file = dir.files.iter().find(|f| f.name == "dyecolorgroupinfo.pabgh")?;
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
