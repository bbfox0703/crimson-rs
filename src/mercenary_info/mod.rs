//! `mercenaryinfo.pabgb` parser — custom-PABGH (u8-key variant).
//!
//! Resolves `MercenaryKey (u8-widened-u32)` — the **template** ID of
//! every mercenary / mount / vehicle in the game. Distinct from
//! `MercenaryNo`, which is the per-save instance ID; the Save Editor's
//! "Rename Mercenary" UI uses this bridge to surface the template
//! lineage of each instance (e.g. "this rename targets a Vehicle_Horse
//! template").
//!
//! 2.06 ships **21 rows** — `Mercenary_Main` (player avatar template)
//! plus mount / vehicle variants (`Vehicle_Horse`, `Vehicle_Dragon`,
//! `Vehicle_WarMachine`, `Vehicle_WarMachine_Raptor`, …).
//!
//! **PABGH layout** (pre-2.06; a 2.06 header is rebuilt into it in
//! memory by `static_header::legacy_pair`) — custom 5-byte entries
//! (`u16 count + (u8 key, u32 offset)*`). New variant for this codebase;
//! other shapes are `(u16, u32)` (factionrelationgroup, storeinfo,
//! partprefabdyetexturepalleteinfo) and `(u32, u32)` (skill, dye color
//! group, etc.). `count` entries × 5 + 2 header bytes.
//!
//! **PABGB row layout** — bridge consumes the leading
//! `(u32 name_len, name, u8 key)` triple (2.04 swapped key and name). The
//! rest of each row
//! (~58 bytes of template body — version tag, sentinel bytes, anchor
//! marker, then a hash trailer) is left untouched.
//!
//! ```text
//! Row {
//!     u32 name_len,
//!     u8  name[name_len],   // ASCII template name, e.g. "Mercenary_Main"
//!     u8  key,              // matches PABGH key
//!     [variable body — version + flags + trailing hash, ~58 B/row,
//!      length determined by the next row's offset]
//! }
//! ```
//!
//! `CString`-shape name (no trailing NUL), matching `iteminfo.pabgb`.
//!
//! **PALOC chain**: not probed. As with the other internal-name-only
//! tables (FactionNode, SubLevel, QuestGauge) the localized name a
//! player sees is resolved through a separate gamedata path; the
//! template internal name is the save-layer identifier and is what
//! this bridge surfaces.

use crate::binary::row_head::read_row_head;

/// One parsed mercenaryinfo row, reduced to what the bridge consumes.
#[derive(Debug, Clone)]
pub struct MercenaryInfoEntry {
    /// `MercenaryKey` (u8 in the on-disk table, widened to u32 here).
    pub key: u32,
    /// ASCII internal name (e.g. `"Mercenary_Main"`, `"Vehicle_Horse"`).
    pub name: String,
}

/// Parse `mercenaryinfo.pabgb` using its custom-shape `.pabgh` index.
///
/// Returns entries in PABGH on-disk order. Skips any row whose body is
/// truncated or whose body key disagrees with the PABGH key.
pub fn parse_mercenary_info_lossy(
    pabgb: &[u8],
    pabgh: &[u8],
) -> Vec<MercenaryInfoEntry> {
    // 2.06 moved row names into the header; rebuild the pre-2.06 layout.
    let (pabgb, pabgh) = crate::binary::static_header::legacy_pair(pabgb, pabgh);
    let (pabgb, pabgh): (&[u8], &[u8]) = (&pabgb, &pabgh);
    if pabgh.len() < 2 {
        return Vec::new();
    }
    let count = u16::from_le_bytes([pabgh[0], pabgh[1]]) as usize;
    if pabgh.len() != 2 + count * 5 {
        return Vec::new();
    }
    let mut entries: Vec<(u8, u32)> = Vec::with_capacity(count);
    for i in 0..count {
        let off = 2 + i * 5;
        let k = pabgh[off];
        let o = u32::from_le_bytes([
            pabgh[off + 1],
            pabgh[off + 2],
            pabgh[off + 3],
            pabgh[off + 4],
        ]);
        entries.push((k, o));
    }
    let mut out = Vec::with_capacity(count);
    for &(key8, off) in &entries {
        let start = off as usize;
        let Some(body) = pabgb.get(start..) else {
            continue;
        };
        let Some(head) = read_row_head(body, 1) else {
            continue;
        };
        if head.key != u32::from(key8) || !(1..=64).contains(&head.name.len()) {
            continue;
        }
        out.push(MercenaryInfoEntry {
            key: head.key,
            name: head.name.to_owned(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    //! Live-install integration test against the real
    //! `mercenaryinfo.pabgb` + `.pabgh`. Pins five known rows and the row
    //! count.

    use crate::binary::gamedata_layout;
    use super::*;

    /// Five known `(key, name)` rows; the live test also pins the row
    /// count.
    const KNOWN: &[(u32, &str)] = &[
        (64, "Mercenary_Main"),
        (78, "Vehicle_Horse"),
        (79, "Vehicle_Dragon"),
        (80, "Vehicle_WarMachine"),
        (82, "Vehicle_WarMachine_Raptor"),
    ];

    fn find_table_bytes() -> Option<(Vec<u8>, Vec<u8>)> {
        // None only when the install (or the table) is absent; an install
        // that is there but unreadable fails the test (audit I-02).
        Some((
            gamedata_layout::extract_bin(&gamedata_layout::body("mercenaryinfo"))?,
            gamedata_layout::extract_bin(&gamedata_layout::header("mercenaryinfo"))?,
        ))
    }

    /// Body-dump probe — reads every 1.07 mercenaryinfo row and prints
    /// the full body (everything after `(u32 name_len, name, u8 key)`)
    /// as hex + repr + interpretation hints. Used to RE the per-row
    /// body schema (price / cooldown / faction / requirements).
    ///
    /// 18 rows × ~58 bytes/row should be small enough to identify
    /// field boundaries by eye. Comparison vs in-game UI values
    /// (recruitment cost, summon cooldown, required level) settles
    /// type guesses.
    #[test]
    #[ignore = "investigation only — dump mercenaryinfo bodies for body-schema RE"]
    fn _probe_mercenary_body_dump() {
        let Some((pabgb, pabgh)) = find_table_bytes() else {
            eprintln!("skipping: no game install");
            return;
        };
        eprintln!("pabgb total: {} bytes", pabgb.len());

        // Re-parse the PABGH index to get sorted (key, offset) so we
        // can compute each row's exact length from the next offset.
        let count = u16::from_le_bytes([pabgh[0], pabgh[1]]) as usize;
        assert_eq!(count, 21);
        let mut idx: Vec<(u8, u32)> = (0..count)
            .map(|i| {
                let off = 2 + i * 5;
                let k = pabgh[off];
                let o = u32::from_le_bytes([
                    pabgh[off + 1], pabgh[off + 2], pabgh[off + 3], pabgh[off + 4],
                ]);
                (k, o)
            })
            .collect();
        // PABGH entries are sorted by offset on-disk; preserve that
        // order so consecutive (offset, next_offset) define row ends.
        idx.sort_by_key(|(_k, o)| *o);

        for (i, &(key, off)) in idx.iter().enumerate() {
            let end = if i + 1 < idx.len() {
                idx[i + 1].1 as usize
            } else {
                pabgb.len()
            };
            let row = &pabgb[off as usize..end];
            let head = read_row_head(row, 1).expect("row head");
            let name = head.name;
            let body = &row[head.body_at..];
            eprintln!(
                "\n=== key={} ({:32}) offset={} row_len={} body_len={} ===",
                key, name, off, row.len(), body.len(),
            );
            // Hex dump in 8-byte rows so eyeball matching is easier.
            for (j, chunk) in body.chunks(8).enumerate() {
                let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
                let ascii: String = chunk.iter()
                    .map(|&b| if (0x20..=0x7e).contains(&b) { b as char } else { '.' })
                    .collect();
                eprintln!(
                    "  {:04x}: {:<24}  |{}|",
                    j * 8, hex.join(" "), ascii,
                );
            }
            // Type-guess pass: try every offset as u32, u16, u8 and flag
            // plausible values (small positive ints, recognisable patterns).
            eprintln!("  --- numeric-field probes ---");
            for o in (0..body.len().saturating_sub(4)).step_by(1) {
                let u32_le = u32::from_le_bytes([body[o], body[o+1], body[o+2], body[o+3]]);
                // Look for values that "feel like" game data:
                // - small u32 (<= 1_000_000) → could be level / price / hash
                // - typed u8 / u16 → flags
                // We just print the "small u32 LE" interpretations to
                // help spot price / cooldown fields.
                if u32_le > 0 && u32_le < 10_000_000 {
                    eprintln!("    @{o:>2}  u32_le={u32_le}");
                }
            }
        }
    }

    #[test]
    fn mercenary_info_lossy_live() {
        let Some((pabgb, pabgh)) = find_table_bytes() else {
            eprintln!("skipping mercenary_info_lossy_live: no game install");
            return;
        };
        let entries = parse_mercenary_info_lossy(&pabgb, &pabgh);
        println!(
            "parsed {} mercenaryinfo entries from {} byte pabgb",
            entries.len(),
            pabgb.len()
        );
        assert_eq!(
            entries.len(),
            21,
            "expected exactly 21 rows in 1.16 (was 19 in 1.13-1.15, 18 through 1.12)"
        );
        let by_key: std::collections::HashMap<u32, &str> =
            entries.iter().map(|e| (e.key, e.name.as_str())).collect();
        for &(key, expected) in KNOWN {
            assert_eq!(
                by_key.get(&key).copied(),
                Some(expected),
                "MercenaryKey {key} mismatch",
            );
        }
        // Sanity: every name is non-empty ASCII alphanumeric +
        // underscore (template-name shape). 1.07 includes both the
        // suffixed `Vehicle_Horse` family and the bare-template rows
        // `Vehicle`, `Wagon`, `Mercenary`, ... so no shared prefix check.
        for e in &entries {
            assert!(!e.name.is_empty(), "empty name for key {}", e.key);
            assert!(
                e.name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_'),
                "name has non-ASCII-alphanumeric byte: key={}, name={:?}",
                e.key,
                e.name,
            );
        }
        println!("all 18 mercenaryinfo rows:");
        for e in &entries {
            println!("  {} → {}", e.key, e.name);
        }
    }
}
