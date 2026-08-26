//! `globalgameevent.pabgb` parser — custom-PABGH (u16-key).
//!
//! Resolves `GlobalGameEventInfoKey (u16-widened-u32)` — server-side
//! global event templates (`Drought_Varnian`, `Flood_Demenissian`,
//! `Typhoon_Delesyian`, …). 103 rows in 1.07. The save layer
//! references these as active-event identifiers.
//!
//! `u16 count + (u16 key, u32 offset)*` PABGH. Body: `[u16 key][u32
//! name_len][name]` followed by per-event payload (timing, action list,
//! cross-references). See [`docs/archive/globalgameevent-body-re.md`](../../docs/archive/globalgameevent-body-re.md)
//! for the full body-schema analysis.
//!
//! ## Bridge surface (v1)
//!
//! - `key + name` — universal, 100% coverage (was the v0 surface).
//! - `group_key` (u32, widened from u16) — universal, 100% coverage.
//!   Cross-references the 7-row `globalgameeventgroup` table (e.g.
//!   `WeatherEventGroup`, `FactionBlockEventGroup`).
//! - `paloc_key` (u64, parsed from a 14-char ASCII decimal string
//!   embedded in the body) — present on most rows but absent on the
//!   `RoyalSupply` + `FactionBlockEvent_*` groups. Returned as 0 when
//!   absent.
//!
//! Per-group action lists and per-row payload schemas are NOT exposed
//! yet — see the docs above for what's been mapped vs deferred.

#[derive(Debug, Clone)]
pub struct GlobalGameEventInfoEntry {
    /// `GlobalGameEventInfoKey` (u16 on disk, widened to u32).
    pub key: u32,
    /// ASCII internal name (e.g. `"Drought_Varnian"`).
    pub name: String,
    /// `GlobalGameEventGroupKey` cross-reference — pulled from
    /// `body[1..3]` as u16 LE, widened to u32. Always present.
    pub group_key: u32,
    /// PALOC localization key for the event's display name, parsed
    /// from the 14-char ASCII decimal field at body offset ~0x1F.
    /// `0` when absent (RoyalSupply / FactionBlockEvent_* groups
    /// lack the embedded `PalocStringRef` structure).
    pub paloc_key: u64,
}

pub fn parse_global_game_event_info_lossy(
    pabgb: &[u8],
    pabgh: &[u8],
) -> Vec<GlobalGameEventInfoEntry> {
    if pabgh.len() < 2 {
        return Vec::new();
    }
    let count = u16::from_le_bytes([pabgh[0], pabgh[1]]) as usize;
    if pabgh.len() != 2 + count * 6 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let pos = 2 + i * 6;
        let key16 = u16::from_le_bytes([pabgh[pos], pabgh[pos + 1]]);
        let off = u32::from_le_bytes([
            pabgh[pos + 2],
            pabgh[pos + 3],
            pabgh[pos + 4],
            pabgh[pos + 5],
        ]) as usize;
        let Some(body) = pabgb.get(off..) else { continue };
        if body.len() < 6 {
            continue;
        }
        if u16::from_le_bytes([body[0], body[1]]) != key16 {
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
        // Per-row payload (everything after the name) — used to extract
        // group_key + paloc_key per the body schema in
        // docs/archive/globalgameevent-body-re.md.
        let payload = &body[6 + name_len..];
        let group_key = extract_group_key(payload).unwrap_or(0);
        let paloc_key = extract_paloc_key(payload).unwrap_or(0);
        out.push(GlobalGameEventInfoEntry {
            key: u32::from(key16),
            name: name.to_owned(),
            group_key,
            paloc_key,
        });
    }
    out
}

/// Pull `group_key` from `payload[1..3]` (u16 LE → u32). Returns `None`
/// when the payload is too short. Universal across all 103 rows in
/// 1.07 — the 7 distinct values match the 7 rows in
/// `globalgameeventgroup.pabgb`.
fn extract_group_key(payload: &[u8]) -> Option<u32> {
    if payload.len() < 3 {
        return None;
    }
    Some(u32::from(u16::from_le_bytes([payload[1], payload[2]])))
}

/// Pull `paloc_key` from the embedded `PalocStringRef` structure that
/// sits at `payload[0x12..]` on most rows. Layout:
///
/// ```text
/// payload[0x12..0x16]: u32 class_tag = 0x0002C12C (signals PalocStringRef)
/// payload[0x16]:       u8 zero
/// payload[0x17..0x19]: u16 key_echo (= the row's key)
/// payload[0x19..0x1B]: u16 zero
/// payload[0x1B..0x1F]: u32 name_len = 14 (always, for the PALOC numeric key)
/// payload[0x1F..0x2D]: 14 ASCII decimal digits — the u64 PALOC key
/// ```
///
/// Returns `None` (caller maps to 0) when:
/// - payload is shorter than `0x2D`
/// - `class_tag` doesn't match — signals the row uses a different
///   payload shape (RoyalSupply / FactionBlockEvent_*)
/// - the 14 bytes aren't valid ASCII digits / don't parse as u64
const PALOC_STRING_REF_TAG: u32 = 0x0002_C12C;
const PALOC_STRING_REF_TAG_OFFSET: usize = 0x12;
const PALOC_STRING_REF_LEN_OFFSET: usize = 0x1B;
const PALOC_STRING_REF_NAME_OFFSET: usize = 0x1F;
/// The numeric PALOC key is always rendered as exactly 14 ASCII
/// decimal digits — `(hi32 = event_key) << 32 + lo32 = namespace`
/// always fits in 14 digits because `event_key < 0x4300` and the
/// lo32 namespace is small (the observed lo32 is in the 0..=0x3FF
/// range).
const PALOC_NUMERIC_LEN: usize = 14;
fn extract_paloc_key(payload: &[u8]) -> Option<u64> {
    if payload.len() < PALOC_STRING_REF_NAME_OFFSET + PALOC_NUMERIC_LEN {
        return None;
    }
    let tag = u32::from_le_bytes([
        payload[PALOC_STRING_REF_TAG_OFFSET],
        payload[PALOC_STRING_REF_TAG_OFFSET + 1],
        payload[PALOC_STRING_REF_TAG_OFFSET + 2],
        payload[PALOC_STRING_REF_TAG_OFFSET + 3],
    ]);
    if tag != PALOC_STRING_REF_TAG {
        return None;
    }
    let name_len = u32::from_le_bytes([
        payload[PALOC_STRING_REF_LEN_OFFSET],
        payload[PALOC_STRING_REF_LEN_OFFSET + 1],
        payload[PALOC_STRING_REF_LEN_OFFSET + 2],
        payload[PALOC_STRING_REF_LEN_OFFSET + 3],
    ]) as usize;
    if name_len != PALOC_NUMERIC_LEN {
        // Defensive: the field-length should always be 14 in 1.07.
        // If a future patch changes it, return None rather than risk
        // mis-reading.
        return None;
    }
    let bytes = &payload[PALOC_STRING_REF_NAME_OFFSET
        ..PALOC_STRING_REF_NAME_OFFSET + PALOC_NUMERIC_LEN];
    let s = std::str::from_utf8(bytes).ok()?;
    s.parse::<u64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const KNOWN: &[(u32, &str)] = &[
        (0x4258, "Drought_Varnian"),
        (0x426b, "Flood_Demenissian"),
        (0x426c, "Typhoon_Delesyian"),
    ];

    /// Pinned (key, group_key, paloc_key) for 1.07. The group_key
    /// matches a `globalgameeventgroup` row; the paloc_key resolves
    /// the localized display name via the PALOC table.
    ///
    /// The `RoyalSupply_*` (group 0x4241) + `FactionBlockEvent_*`
    /// (group 0x4244) rows have `paloc_key = 0` because their body
    /// shape lacks the embedded PalocStringRef.
    ///
    /// 2.00 split the single `RoyalSupply` row (0x424a) into four
    /// per-faction rows (0x4308–0x430b), which is the whole of that
    /// patch's 188 → 191 delta (−1 +4). All four keep group 0x4241 and
    /// the absent paloc, so they replace 0x424a here one-for-four.
    const KNOWN_BODY: &[(u32, u32, u64)] = &[
        // (key,    group_key, paloc_key)
        (0x4258, 0x4240, 72_945_724_555_969),  // Drought_Varnian
        (0x426b, 0x4240, 73_027_328_934_593),  // Flood_Demenissian
        (0x426c, 0x4240, 73_031_623_901_889),  // Typhoon_Delesyian
        (0x4308, 0x4241, 0),                   // RoyalSupply_Her (no paloc ref)
        (0x4309, 0x4241, 0),                   // RoyalSupply_Dem (no paloc ref)
        (0x430a, 0x4241, 0),                   // RoyalSupply_Del (no paloc ref)
        (0x430b, 0x4241, 0),                   // RoyalSupply_Var (no paloc ref)
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
        let group_dir = game_root.join("0008");
        let pabgb = crate::binary::paz::extract_file(
            &group_dir,
            dir.files.iter().find(|f| f.name == "globalgameevent.pabgb")?,
            "gamedata/binary__/client/bin",
            &pamt.header.encrypt_info.encrypt_info,
        )
        .ok()?;
        let pabgh = crate::binary::paz::extract_file(
            &group_dir,
            dir.files.iter().find(|f| f.name == "globalgameevent.pabgh")?,
            "gamedata/binary__/client/bin",
            &pamt.header.encrypt_info.encrypt_info,
        )
        .ok()?;
        Some((pabgb, pabgh))
    }

    /// Body-dump probe — reads all 103 globalgameevent rows, prints
    /// each body's bytes + numeric-field probes. Body content is
    /// near-uniform per probe data (row sizes 164/166/170 bytes —
    /// small variance suggests fixed-length core + a variable-length
    /// trailing field like an ASCII number string).
    #[test]
    #[ignore = "investigation only — dump globalgameevent bodies for body-schema RE"]
    fn _probe_global_game_event_body_dump() {
        let Some((pabgb, pabgh)) = find_table_bytes() else {
            eprintln!("skipping: no game install");
            return;
        };
        eprintln!("pabgb total: {} bytes", pabgb.len());

        let count = u16::from_le_bytes([pabgh[0], pabgh[1]]) as usize;
        let mut idx: Vec<(u16, u32)> = (0..count)
            .map(|i| {
                let pos = 2 + i * 6;
                let k = u16::from_le_bytes([pabgh[pos], pabgh[pos + 1]]);
                let o = u32::from_le_bytes([
                    pabgh[pos + 2], pabgh[pos + 3],
                    pabgh[pos + 4], pabgh[pos + 5],
                ]);
                (k, o)
            })
            .collect();
        idx.sort_by_key(|(_k, o)| *o);

        // First pass: histogram of body sizes so we see if it's truly uniform.
        let mut size_hist: std::collections::BTreeMap<usize, u32> = Default::default();
        for (i, &(_k, off)) in idx.iter().enumerate() {
            let end = if i + 1 < idx.len() { idx[i + 1].1 as usize } else { pabgb.len() };
            let row = &pabgb[off as usize..end];
            let name_len = u32::from_le_bytes([row[2], row[3], row[4], row[5]]) as usize;
            let body_len = row.len() - 6 - name_len;
            *size_hist.entry(body_len).or_insert(0) += 1;
        }
        eprintln!("\nbody-size histogram (after `[u16 key][u32 name_len][name]`):");
        for (size, count) in &size_hist {
            eprintln!("  {size} bytes: {count} rows");
        }

        // Second pass: print the first 6 rows in full + every distinct
        // body-size class.
        eprintln!("\n=== Sample row dumps ===");
        let mut printed_sizes: std::collections::BTreeSet<usize> = Default::default();
        for (i, &(key, off)) in idx.iter().enumerate() {
            let end = if i + 1 < idx.len() { idx[i + 1].1 as usize } else { pabgb.len() };
            let row = &pabgb[off as usize..end];
            let name_len = u32::from_le_bytes([row[2], row[3], row[4], row[5]]) as usize;
            let name = std::str::from_utf8(&row[6..6 + name_len]).unwrap_or("<bad>");
            let body = &row[6 + name_len..];

            // Print: first 8 rows ALWAYS; then one example per distinct size class.
            let should_print = i < 8 || printed_sizes.insert(body.len());
            if !should_print { continue }

            eprintln!(
                "\n--- row {} key=0x{key:04x} name={:32} body_len={} ---",
                i, name, body.len(),
            );
            for (j, chunk) in body.chunks(16).enumerate() {
                let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
                let ascii: String = chunk.iter()
                    .map(|&b| if (0x20..=0x7e).contains(&b) { b as char } else { '.' })
                    .collect();
                eprintln!(
                    "  {:04x}: {:<48}  |{}|",
                    j * 16, hex.join(" "), ascii,
                );
            }
        }

        // Third pass: scan for repeating fields across rows.
        // For each offset, collect the value across all rows; if they
        // cluster tightly (e.g. all the same, or always small u32), that's
        // a likely field.
        eprintln!("\n=== Per-offset cross-row analysis (first 80 bytes of body) ===");
        let mut by_offset: std::collections::BTreeMap<usize, Vec<u32>> = Default::default();
        let mut first_bodies: Vec<Vec<u8>> = Vec::new();
        for (i, &(_k, off)) in idx.iter().enumerate() {
            let end = if i + 1 < idx.len() { idx[i + 1].1 as usize } else { pabgb.len() };
            let row = &pabgb[off as usize..end];
            let name_len = u32::from_le_bytes([row[2], row[3], row[4], row[5]]) as usize;
            let body = row[6 + name_len..].to_vec();
            first_bodies.push(body.clone());
            for o in (0..body.len().saturating_sub(4)).step_by(1) {
                let v = u32::from_le_bytes([body[o], body[o+1], body[o+2], body[o+3]]);
                by_offset.entry(o).or_default().push(v);
            }
        }
        for o in 0..80 {
            let Some(vals) = by_offset.get(&o) else { continue };
            let distinct: std::collections::BTreeSet<&u32> = vals.iter().collect();
            let max = *vals.iter().max().unwrap_or(&0);
            let min = *vals.iter().min().unwrap_or(&0);
            let label = if distinct.len() == 1 {
                format!("CONSTANT 0x{max:08x}")
            } else if distinct.len() <= 8 {
                let vs: Vec<String> = distinct.iter().map(|v| format!("0x{v:08x}")).collect();
                format!("{} distinct: {{ {} }}", distinct.len(), vs.join(", "))
            } else if max < 100_000 {
                format!("{} distinct, range [{min}, {max}]", distinct.len())
            } else {
                format!("{} distinct, hi-magnitude", distinct.len())
            };
            eprintln!("  @{o:>2} u32 LE: {label}");
        }
    }

    #[test]
    fn global_game_event_info_lossy_live() {
        let Some((pabgb, pabgh)) = find_table_bytes() else {
            eprintln!("skipping global_game_event_info_lossy_live: no game install");
            return;
        };
        let entries = parse_global_game_event_info_lossy(&pabgb, &pabgh);
        assert_eq!(entries.len(), 191, "expected 191 rows in 2.00 (was 188 in 1.08-1.18)");
        let by_key: std::collections::HashMap<u32, &str> =
            entries.iter().map(|e| (e.key, e.name.as_str())).collect();
        for &(k, expected) in KNOWN {
            assert_eq!(by_key.get(&k).copied(), Some(expected), "key 0x{k:04x}");
        }
    }

    /// Live test pinning the new body fields (`group_key`, `paloc_key`).
    /// Skips when the game install isn't present.
    #[test]
    fn global_game_event_info_body_fields_live() {
        let Some((pabgb, pabgh)) = find_table_bytes() else {
            eprintln!("skipping: no game install");
            return;
        };
        let entries = parse_global_game_event_info_lossy(&pabgb, &pabgh);
        assert_eq!(entries.len(), 191);

        let by_key: std::collections::HashMap<u32, &GlobalGameEventInfoEntry> =
            entries.iter().map(|e| (e.key, e)).collect();
        for &(key, expected_group, expected_paloc) in KNOWN_BODY {
            let e = by_key.get(&key).unwrap_or_else(|| panic!("missing key 0x{key:04x}"));
            assert_eq!(
                e.group_key, expected_group,
                "key 0x{key:04x} group_key mismatch",
            );
            assert_eq!(
                e.paloc_key, expected_paloc,
                "key 0x{key:04x} paloc_key mismatch",
            );
        }

        // Every row's group_key must be in the 12 known
        // GlobalGameEventGroupKey range — pinned in 1.08 (7 in 1.07; 1.08
        // added 0x424a–0x424e, corresponding to the new `FactionBlockEvent_*`
        // groups Pearl Abyss added for per-node faction blocking).
        let known_groups: std::collections::HashSet<u32> = [
            0x4240, 0x4241, 0x4244, 0x4246, 0x4247, 0x4248, 0x4249,
            0x424a, 0x424b, 0x424c, 0x424d, 0x424e,
        ].into_iter().collect();
        for e in &entries {
            assert!(
                known_groups.contains(&e.group_key),
                "key 0x{:04x} ({}) has unexpected group_key 0x{:04x}",
                e.key, e.name, e.group_key,
            );
        }

        // PalocStringRef coverage: most rows have a non-zero
        // paloc_key. Per the body-RE doc the absent set is the
        // RoyalSupply + FactionBlockEvent_* (23) groups — 24/103 missing
        // when RoyalSupply was one row, 27 missing since 2.00 split it
        // into four. ~76% should have a paloc_key.
        let with_paloc = entries.iter().filter(|e| e.paloc_key != 0).count();
        eprintln!("paloc_key coverage: {}/{}", with_paloc, entries.len());
        assert!(
            with_paloc >= 70,
            "paloc_key coverage dropped below 70: got {with_paloc}/{}",
            entries.len(),
        );

        // Cross-check: when paloc_key is present, its hi32 should
        // equal the event key (the PALOC `(hi32, lo32)` convention).
        for e in &entries {
            if e.paloc_key == 0 { continue }
            let hi32 = (e.paloc_key >> 32) as u32;
            assert_eq!(
                hi32, e.key,
                "key 0x{:04x} ({}) has paloc_key hi32 mismatch: paloc={} hi32=0x{:08x}",
                e.key, e.name, e.paloc_key, hi32,
            );
        }
    }

    /// Pure-Rust unit test for the body extractors. Doesn't require
    /// a game install — synthetic payloads pin the exact byte layout
    /// the parser expects.
    #[test]
    fn extract_group_key_handles_short_payload() {
        assert_eq!(extract_group_key(&[]), None);
        assert_eq!(extract_group_key(&[0]), None);
        assert_eq!(extract_group_key(&[0, 0x40, 0x42]), Some(0x4240));
        assert_eq!(extract_group_key(&[0, 0x41, 0x42, 0x80]), Some(0x4241));
    }

    #[test]
    fn extract_paloc_key_round_trip() {
        // Build a synthetic payload with the WeatherEventGroup-style
        // PalocStringRef at offset 0x12. Anything before that is
        // arbitrary header content.
        let mut payload = vec![0u8; 0x12];
        payload.extend_from_slice(&PALOC_STRING_REF_TAG.to_le_bytes()); // 0x12..0x16
        payload.push(0);                                                 // 0x16
        payload.extend_from_slice(&0x4258u16.to_le_bytes());            // 0x17..0x19 (key echo)
        payload.extend_from_slice(&[0, 0]);                             // 0x19..0x1B (u16 zero)
        payload.extend_from_slice(&14u32.to_le_bytes());                // 0x1B..0x1F (name_len = 14)
        // 0x1F..0x2D: "72945724555969" (= 14 ASCII digits)
        payload.extend_from_slice(b"72945724555969");
        assert_eq!(extract_paloc_key(&payload), Some(72_945_724_555_969));
    }

    #[test]
    fn extract_paloc_key_rejects_wrong_tag() {
        // Wrong tag → None (signals a different body shape, like
        // RoyalSupply / FactionBlockEvent_*).
        let mut payload = vec![0u8; 0x12];
        payload.extend_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        payload.extend(std::iter::repeat_n(0, 32));
        assert_eq!(extract_paloc_key(&payload), None);
    }

    #[test]
    fn extract_paloc_key_rejects_wrong_length() {
        // Tag matches but name_len != 14 → None (defensive guard
        // against schema drift).
        let mut payload = vec![0u8; 0x12];
        payload.extend_from_slice(&PALOC_STRING_REF_TAG.to_le_bytes());
        payload.push(0);
        payload.extend_from_slice(&0x4258u16.to_le_bytes());
        payload.extend_from_slice(&[0, 0]);
        payload.extend_from_slice(&13u32.to_le_bytes()); // wrong: 13 not 14
        payload.extend_from_slice(b"7294572455596"); // 13 chars
        assert_eq!(extract_paloc_key(&payload), None);
    }
}
