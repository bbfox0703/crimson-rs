//! `storeinfo.pabgb` parser — custom-PABGH.
//!
//! Resolves `StoreKey (u16-widened-u32)` — the template ID of every
//! store / vendor / merchant in the game (general stores, blacksmiths,
//! basecamp traders, fishing stalls, …). The save layer references
//! these by `u16`, so the same bridge surfaces the value widened to
//! `u32` to keep the C ABI uniform across the project.
//!
//! 1.07 ships **292 rows** ranging across every store family: general
//! stores (`Store_Her_General` / `Store_Cal_General`), per-region
//! suppliers, blacksmiths, fishing vendors, basecamp shops, and the
//! various non-standard formats (BlackMarket, StreetVendor, Church
//! camp, etc.) the hexpat-RE pass in [`references/store_info.hexpat`]
//! catalogued.
//!
//! **PABGH layout** — custom 6-byte entries (`u16 count + (u16 key, u32 offset)*`),
//! same shape as `factionrelationgroup.pabgh` and
//! `partprefabdyetexturepalleteinfo.pabgh`. 292 entries × 6 + 2 header =
//! 1,754 bytes (matches the live file).
//!
//! **PABGB row layout** — the bridge only consumes the leading
//! `(u16 key, u32 name_len, name)` triple. The rest of each row (~4 KB
//! of price / item / format-tag body) is left untouched; the hexpat
//! pattern at `references/store_info.hexpat` documents the per-format
//! layouts for callers that want to RE the embedded item list. Per-row
//! body decode (with per-item `(item_key, buy_price, sell_price,
//! purchase_limit)` extraction) is **deferred** — the hexpat-claimed
//! "105-byte uniform items" turns out to be wrong: items are
//! variable-length in 1.07 saves (the first item of Store_Her_General
//! is 123 bytes, item 1 has a different size, stride is not constant).
//! See [`docs/save-editor-keys-plan.md`](../../docs/save-editor-keys-plan.md)
//! "What's deferred" for the RE roadmap and the
//! `_probe_store_variant_distribution` test below for the classified-
//! by-`(field_e, format_tag)` row landscape.
//!
//! ```text
//! Row {
//!     u16 key,              // matches PABGH key
//!     u32 name_len,
//!     u8  name[name_len],   // ASCII template name, e.g. "Store_Her_General"
//!     [variable body — store header + items + tail, format-specific,
//!      length determined by the next row's offset]
//! }
//! ```
//!
//! `CString`-shape (no trailing NUL) name field, matching `iteminfo.pabgb`.
//!
//! **PALOC chain**: not probed yet. The user-facing localized name a
//! player sees in-game ("General Store of Herennen") is resolved through
//! a different gamedata path; the internal template name is the
//! save-layer's only stable identifier and is what this bridge surfaces.

/// One parsed storeinfo row, reduced to what the bridge consumes.
#[derive(Debug, Clone)]
pub struct StoreInfoEntry {
    /// `StoreKey` (u16 in the on-disk table, widened to u32 here).
    pub key: u32,
    /// ASCII internal name (e.g. `"Store_Her_General"`,
    /// `"Store_BlackMarket"`, `"Store_StreetVendor_Cal"`).
    pub name: String,
}

/// Parse `storeinfo.pabgb` using its custom-shape `.pabgh` index.
///
/// Returns entries in PABGH on-disk order. Skips any row whose body is
/// truncated or whose body key disagrees with the PABGH key — both
/// treated as anchor noise rather than panics so a future patch with a
/// new row shape doesn't break the build.
pub fn parse_store_info_lossy(pabgb: &[u8], pabgh: &[u8]) -> Vec<StoreInfoEntry> {
    if pabgh.len() < 2 {
        return Vec::new();
    }
    let count = u16::from_le_bytes([pabgh[0], pabgh[1]]) as usize;
    if pabgh.len() != 2 + count * 6 {
        return Vec::new();
    }
    let mut entries: Vec<(u16, u32)> = Vec::with_capacity(count);
    for i in 0..count {
        let off = 2 + i * 6;
        let k = u16::from_le_bytes([pabgh[off], pabgh[off + 1]]);
        let o = u32::from_le_bytes([
            pabgh[off + 2],
            pabgh[off + 3],
            pabgh[off + 4],
            pabgh[off + 5],
        ]);
        entries.push((k, o));
    }
    let mut out = Vec::with_capacity(count);
    for &(key16, off) in &entries {
        let start = off as usize;
        // Row needs at least `u16 key + u32 name_len = 6` bytes to be
        // parseable. We don't need the end-of-row offset for a
        // name-only bridge — the `name_len` field bounds the read.
        let Some(body) = pabgb.get(start..) else {
            continue;
        };
        if body.len() < 6 {
            continue;
        }
        let body_key = u16::from_le_bytes([body[0], body[1]]);
        if body_key != key16 {
            continue;
        }
        let name_len =
            u32::from_le_bytes([body[2], body[3], body[4], body[5]]) as usize;
        if !(1..=128).contains(&name_len) || 6 + name_len > body.len() {
            continue;
        }
        let Ok(name) = std::str::from_utf8(&body[6..6 + name_len]) else {
            continue;
        };
        out.push(StoreInfoEntry {
            key: u32::from(key16),
            name: name.to_owned(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    //! Live-install integration test against the real storeinfo
    //! `.pabgb` / `.pabgh` pair. Pins a handful of mappings from the
    //! 2026-05-16 RE pass. Skips cleanly when the game install isn't
    //! present.

    use super::*;
    use std::path::PathBuf;

    /// (StoreKey, expected internal_name). Taken from the
    /// `_probe_store_mercenary_partprefab` output — the first two
    /// entries by on-disk PABGH order. The 1.07 file has 292 rows.
    const KNOWN: &[(u32, &str)] = &[
        (3101, "Store_Her_General"),
        (3201, "Store_Cal_General"),
    ];

    fn find_table_bytes() -> Option<(Vec<u8>, Vec<u8>)> {
        let game_root = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(r"D:\SteamLibrary\steamapps\common\Crimson Desert")
            });
        let pamt_path = game_root.join("0008").join("0.pamt");
        if !pamt_path.is_file() {
            return None;
        }
        let pamt_bytes = std::fs::read(&pamt_path).ok()?;
        let pamt = crate::binary::pamt::PackMeta::parse(&pamt_bytes, None).ok()?;
        let dir = pamt
            .directories
            .iter()
            .find(|d| d.path == "gamedata/binary__/client/bin")?;
        let pabgb_file = dir.files.iter().find(|f| f.name == "storeinfo.pabgb")?;
        let pabgh_file = dir.files.iter().find(|f| f.name == "storeinfo.pabgh")?;
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
    fn store_info_lossy_live() {
        let Some((pabgb, pabgh)) = find_table_bytes() else {
            eprintln!("skipping store_info_lossy_live: no game install");
            return;
        };
        let entries = parse_store_info_lossy(&pabgb, &pabgh);
        println!(
            "parsed {} storeinfo entries from {} byte pabgb",
            entries.len(),
            pabgb.len()
        );
        assert!(
            entries.len() >= 250,
            "expected at least 250 storeinfo rows in 1.07, got {}",
            entries.len()
        );
        let by_key: std::collections::HashMap<u32, &str> =
            entries.iter().map(|e| (e.key, e.name.as_str())).collect();
        for &(key, expected) in KNOWN {
            assert_eq!(
                by_key.get(&key).copied(),
                Some(expected),
                "StoreKey {key} mismatch",
            );
        }
        // Sanity: every name starts with "Store_" (template prefix).
        for e in &entries {
            assert!(
                e.name.starts_with("Store_"),
                "store name does not start with Store_: key={}, name={:?}",
                e.key,
                e.name,
            );
        }
    }

    /// Forensic probe for the next-session per-row body RE pass.
    /// Classifies every row by `(field_e, format_tag)` (the
    /// common-header discriminators at offsets +21 and +26..+27) and
    /// dumps the first 96 bytes of body for each variant group.
    ///
    /// 1.07 distribution (2026-05-17 baseline):
    /// - `field_e=0, format_tag=0x01FD/0x03FC/0x83FC`: 181 rows (standard family;
    ///   items table follows the 23-byte std-family continuation)
    /// - `field_e=1, format_tag=0x01FD`: 10 rows (Trade — variable-length items)
    /// - `field_e=2, format_tag=0x01FD`: 9 rows (Short-tail — 15-byte tail variant)
    /// - `field_e=0, format_tag=0x0600/0x83FC`: 6 rows (Camp anomaly + leather variants)
    /// - `field_e ∈ {14, 1536, 1537, 1538}`: 86 rows (Camp / BlackMarket /
    ///   Furniture / TradeManager etc. — completely different body layout,
    ///   prefix-list-then-std-family structure; layout pending full RE)
    ///
    /// The hexpat `references/store_info.hexpat` claims a uniform 105-byte
    /// `StoreItemEntry`, but the probe revealed that item bytes are
    /// actually **variable-length** even within the standard family
    /// (Store_Her_General's first item is 123 bytes; subsequent items
    /// have different sizes). A full per-item parser needs either:
    /// - a variable-length walker that scans for the `0xFFFF` separator
    ///   + `item_key_dup` match to find item boundaries, or
    /// - deeper RE of the `item_data` sub-fields to compute per-item
    ///   length from the leading `sentinel_2` bitmask.
    ///
    /// Re-run with:
    /// ```text
    /// cargo test --lib --features c_abi _probe_store_variant_distribution -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore]
    fn _probe_store_variant_distribution() {
        let Some((pabgb, pabgh)) = find_table_bytes() else {
            eprintln!("skipping: no game install");
            return;
        };
        let count = u16::from_le_bytes([pabgh[0], pabgh[1]]) as usize;
        assert_eq!(pabgh.len(), 2 + count * 6, "PABGH shape mismatch");
        let mut entries: Vec<(u16, u32)> = Vec::with_capacity(count);
        for i in 0..count {
            let off = 2 + i * 6;
            let k = u16::from_le_bytes([pabgh[off], pabgh[off + 1]]);
            let o = u32::from_le_bytes([
                pabgh[off + 2],
                pabgh[off + 3],
                pabgh[off + 4],
                pabgh[off + 5],
            ]);
            entries.push((k, o));
        }
        let mut offsets: Vec<u32> = entries.iter().map(|&(_, o)| o).collect();
        offsets.sort();
        offsets.push(pabgb.len() as u32);

        use std::collections::BTreeMap;
        type VariantSample = (u16, u32, u32, String);
        let mut by_variant: BTreeMap<(u32, u8, u8), Vec<VariantSample>> = BTreeMap::new();
        for &(key, start) in &entries {
            let row_end = offsets
                .iter()
                .find(|&&o| o > start)
                .copied()
                .unwrap_or(pabgb.len() as u32);
            let len = row_end - start;
            let body = &pabgb[start as usize..row_end as usize];
            if body.len() < 6 {
                continue;
            }
            let name_len = u32::from_le_bytes([body[2], body[3], body[4], body[5]]) as usize;
            if 6 + name_len > body.len() {
                continue;
            }
            let name = std::str::from_utf8(&body[6..6 + name_len])
                .unwrap_or("<bad utf8>")
                .to_owned();
            let common_start = 6 + name_len;
            if common_start + 28 > body.len() {
                continue;
            }
            let field_e = u32::from_le_bytes([
                body[common_start + 21],
                body[common_start + 22],
                body[common_start + 23],
                body[common_start + 24],
            ]);
            let fta = body[common_start + 26];
            let ftb = body[common_start + 27];
            by_variant
                .entry((field_e, fta, ftb))
                .or_default()
                .push((key, start, len, name));
        }

        println!("\n=== Store variant distribution ({count} total rows) ===");
        for ((field_e, fta, ftb), rows) in &by_variant {
            println!(
                "  field_e={} format_tag=0x{:02X}{:02X}: {} rows  (sample: key={} name={:?} len={})",
                field_e,
                fta,
                ftb,
                rows.len(),
                rows[0].0,
                rows[0].3,
                rows[0].2,
            );
        }

        println!("\n=== First-sample body dumps (96 bytes after name) ===");
        for ((field_e, fta, ftb), rows) in &by_variant {
            let (key, start, len, name) = &rows[0];
            let body = &pabgb[*start as usize..(*start as usize + *len as usize)];
            let name_len =
                u32::from_le_bytes([body[2], body[3], body[4], body[5]]) as usize;
            let after_name = 6 + name_len;
            let head = &body[after_name..(after_name + 96).min(body.len())];
            print!(
                "  variant=(field_e={}, 0x{:02X}{:02X}) key={} name={:?} body_len={} :",
                field_e, fta, ftb, key, name, len
            );
            for (i, b) in head.iter().enumerate() {
                if i % 16 == 0 {
                    println!();
                    print!("    {:04X}:", after_name + i);
                }
                print!(" {:02X}", b);
            }
            println!();
        }
    }
}
