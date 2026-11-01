//! `skill.pabgb` + `skill.pabgh` parser for Crimson Desert.
//!
//! Field layout reverse-engineered from IDA decompilation of
//! `SkillInfo::readEntryFields` (`sub_1410F8680`) and the `BuffData`
//! factory (`sub_1419D8660`); ground-truth ported from the GameMods
//! `skillinfo_parser.py` (35 KB pure Python, validated 100% roundtrip
//! across game versions 1.03 / 1.04 / 1.05).
//!
//! Scope notes
//! -----------
//!
//! - The `BuffData` subclass tail size depends on `type_id` (0..=119)
//!   and is **not statically known** for every value. The loader
//!   brute-forces sizes 0..500 and caches per-type_id; entries whose
//!   subclass cannot be resolved fall back to a raw blob path that
//!   preserves bytes for roundtrip but loses semantic structure.
//! - Format flag `field_58` is present in 1.04+ format and absent in
//!   1.03. We auto-detect by probing the first non-null buff entry.
//! - 2.04 lays an entry out `[len][name][key][head][buff matrix][tail]`, where
//!   head and tail are the post-buff fields split around the matrix (see
//!   `post_buff`). Nothing inside `BuffData` moved: 1,976 of the 2,068 entries
//!   carried over from 2.03 keep their exact length.
//! - Cross-version probe (May 2026) showed 11 type_id sizes drift
//!   between 1.03 / 1.04 / 1.05, so the brute-force probe must stay —
//!   hard-coding sizes would silently break older or future versions.

mod buff_data;
mod pabgh;
mod post_buff;

pub use buff_data::{BuffData, BuffDataBody, SkillFormat, TailSizeCache};
pub use pabgh::{SkillIndexEntry, entry_ranges, parse_pabgh, write_pabgh};
pub use post_buff::{EntryHead, Graph, PostBuff, ResourceItem, ResourceStat};

use std::io;

use crate::binary::{BinaryRead, BinaryWrite};

const PROBE_MAX_TAIL: usize = 500;

/// One skill entry. Carries enough to round-trip the original bytes.
#[derive(Debug, Clone)]
pub struct SkillEntry {
    pub key: u32,
    /// Raw name bytes (`u32 len + len bytes`, no terminator).
    pub name_bytes: Vec<u8>,
    pub unk_post_key: u8,
    pub is_blocked: u8,
    pub pad_01: [u8; 3],
    /// Decoded buff matrix; `None` when the raw fallback is used.
    pub buff_level_list: Option<Vec<Vec<BuffData>>>,
    /// Raw buff blob preserved for entries the probe could not resolve.
    /// Mutually exclusive with `buff_level_list`. The bytes run from the end
    /// of the entry head up to (but not including) the post-buff tail.
    pub buff_raw_fallback: Option<Vec<u8>>,
    pub post_buff: PostBuff,
}

/// Parsed `skill.pabgh` + `skill.pabgb` pair.
#[derive(Debug, Clone)]
pub struct SkillData {
    pub entries: Vec<SkillEntry>,
    pub format: SkillFormat,
    /// On-disk PABGH order — entries are stored in this same order.
    /// Kept for `write` to reproduce the exact byte layout.
    pub index_order: Vec<SkillIndexEntry>,
}

/// Result of [`probe_entry_failures`].
///
/// Test-only: `skill_info` is a private module, so a diagnostic that only the
/// `_probe_skill_entry_failures` test calls would otherwise trip the crate's
/// `-D dead_code` clippy gate under `--features c_abi,python`.
#[cfg(test)]
#[derive(Debug, Clone)]
pub struct EntryFailureReport {
    pub total: usize,
    /// `(entry_index, key, error message)` for every entry that failed.
    pub failures: Vec<(usize, u32, String)>,
}

/// Walk every entry independently and collect the ones that fail to parse.
///
/// [`SkillData::parse`] aborts on the first bad entry, so on a new game
/// patch it cannot tell "one odd new skill" from "systematic schema
/// drift". This keeps going, which is the first question to answer before
/// starting any skill RE. Errors only for problems that make per-entry
/// probing impossible at all (bad PABGH, no format detected).
#[cfg(test)]
pub fn probe_entry_failures(pabgh: &[u8], pabgb: &[u8]) -> io::Result<EntryFailureReport> {
    let index = parse_pabgh(pabgh)?;
    let ranges = entry_ranges(&index, pabgb.len());
    let format = detect_format(pabgb, &index, &ranges)?;

    let mut cache = TailSizeCache::new();
    let mut failures = Vec::new();
    for (i, &(start, end)) in ranges.iter().enumerate() {
        if start > end || end > pabgb.len() {
            failures.push((i, index[i].key, format!("bad range [{start}, {end})")));
            continue;
        }
        if let Err(e) = parse_skill_entry(&pabgb[start..end], format, &mut cache) {
            failures.push((i, index[i].key, e.to_string()));
        }
    }
    Ok(EntryFailureReport { total: ranges.len(), failures })
}

impl SkillData {
    /// Parse both files, returning entries in PABGH on-disk order.
    pub fn parse(pabgh: &[u8], pabgb: &[u8]) -> io::Result<Self> {
        let index = parse_pabgh(pabgh)?;
        let ranges = entry_ranges(&index, pabgb.len());

        let format = detect_format(pabgb, &index, &ranges)?;

        let mut cache = TailSizeCache::new();
        let mut entries = Vec::with_capacity(index.len());
        for (i, &(start, end)) in ranges.iter().enumerate() {
            if start > end || end > pabgb.len() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "entry {} (key={}): bad range [{}, {}) vs len {}",
                        i, index[i].key, start, end, pabgb.len()
                    ),
                ));
            }
            let entry = parse_skill_entry(&pabgb[start..end], format, &mut cache)
                .map_err(|e| {
                    io::Error::new(
                        e.kind(),
                        format!("entry {} (key={}): {}", i, index[i].key, e),
                    )
                })?;
            entries.push(entry);
        }

        Ok(SkillData {
            entries,
            format,
            index_order: index,
        })
    }

    /// Serialise back to `(pabgh_bytes, pabgb_bytes)`. The PABGH on-disk
    /// order is preserved — pass an unmodified `SkillData` from `parse`
    /// for byte-identical roundtrip.
    pub fn write(&self) -> io::Result<(Vec<u8>, Vec<u8>)> {
        // Serialise each entry, recompute its offset within pabgb in the
        // on-disk index order. (For real game files the on-disk order is
        // already offset-sorted, so this just lays bytes out sequentially.)
        if self.entries.len() != self.index_order.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "entries len {} != index_order len {}",
                    self.entries.len(),
                    self.index_order.len()
                ),
            ));
        }

        let mut pabgb = Vec::new();
        let mut new_index = Vec::with_capacity(self.entries.len());
        for (entry, idx) in self.entries.iter().zip(self.index_order.iter()) {
            let offset = pabgb.len() as u32;
            write_skill_entry(&mut pabgb, entry, self.format)?;
            new_index.push(SkillIndexEntry {
                key: idx.key,
                offset,
            });
        }
        let pabgh = write_pabgh(&new_index)?;
        Ok((pabgh, pabgb))
    }
}

// ── Per-entry parse / write ───────────────────────────────────────────────

/// Parse a single skill entry from a `[start, end)` slice of the pabgb.
fn parse_skill_entry(
    entry_bytes: &[u8],
    format: SkillFormat,
    cache: &mut TailSizeCache,
) -> io::Result<SkillEntry> {
    let mut p: usize = 0;
    let name_bytes = read_name_bytes(entry_bytes, &mut p)?;
    let key = u32::read_from(entry_bytes, &mut p)?;

    let body = &entry_bytes[p..];
    let (head, buff_level_list, buff_raw_fallback, post_buff) = parse_body(body, format, cache)?;

    Ok(SkillEntry {
        key,
        name_bytes,
        unk_post_key: head.unk_post_key,
        is_blocked: head.is_blocked,
        pad_01: head.pad_01,
        buff_level_list,
        buff_raw_fallback,
        post_buff,
    })
}

fn write_skill_entry<W: io::Write>(
    w: &mut W,
    entry: &SkillEntry,
    format: SkillFormat,
) -> io::Result<()> {
    write_name_bytes(w, &entry.name_bytes)?;
    entry.key.write_to(w)?;

    let head = EntryHead {
        unk_post_key: entry.unk_post_key,
        is_blocked: entry.is_blocked,
        pad_01: entry.pad_01,
    };
    post_buff::write_entry_head(w, &head, &entry.post_buff)?;

    if let Some(raw) = &entry.buff_raw_fallback {
        // Raw path: write the preserved blob (already includes level_count
        // + per-level/per-buff bytes).
        w.write_all(raw)?;
    } else if let Some(levels) = &entry.buff_level_list {
        (levels.len() as u32).write_to(w)?;
        for level in levels {
            (level.len() as u32).write_to(w)?;
            for bd in level {
                buff_data::write_buff_data(w, bd)?;
            }
        }
    } else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "entry has neither buff_level_list nor buff_raw_fallback",
        ));
    }

    post_buff::write_post_buff_tail(w, &entry.post_buff)?;
    let _ = format; // currently unused at the entry level; kept for symmetry.
    Ok(())
}

// ── Body parser (the messy bit: probing + fallback) ───────────────────────

#[allow(clippy::type_complexity)]
fn parse_body(
    body: &[u8],
    format: SkillFormat,
    cache: &mut TailSizeCache,
) -> io::Result<(
    EntryHead,
    Option<Vec<Vec<BuffData>>>,
    Option<Vec<u8>>,
    PostBuff,
)> {
    let body_end = body.len();
    let mut head_end = 0;
    let (head, mut post_buff) = post_buff::read_entry_head(body, &mut head_end)?;

    let parse_attempt = try_parse_buff_levels(body, head_end, format, cache);

    let (buff_level_list, buff_raw_fallback, mut p_after_buffs) = match parse_attempt {
        Ok((levels, after)) => (Some(levels), None, after),
        Err(_) => {
            // Brute-force: find a position where the post-buff tail parses
            // cleanly to body_end.
            let pb_start = find_post_buff_start(body, head_end + 4).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "could not locate post-buff boundary for raw fallback",
                )
            })?;
            (None, Some(body[head_end..pb_start].to_vec()), pb_start)
        }
    };

    // Try the tail from the cursor we ended at. If that doesn't reach
    // body_end exactly, fall back to brute-force scan.
    if read_post_buff_strict(body, &mut p_after_buffs, body_end, &mut post_buff).is_err() {
        let pb_start = find_post_buff_start(body, head_end + 4).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "post-buff parse failed and brute-force scan found no boundary",
            )
        })?;
        // If we already had a decoded buff list, demote to raw fallback.
        let raw = body[head_end..pb_start].to_vec();
        let mut p2 = pb_start;
        read_post_buff_strict(body, &mut p2, body_end, &mut post_buff).map_err(|e| {
            io::Error::new(e.kind(), format!("post-buff at fallback start failed: {}", e))
        })?;
        return Ok((head, None, Some(raw), post_buff));
    }

    Ok((head, buff_level_list, buff_raw_fallback, post_buff))
}

fn read_post_buff_strict(
    body: &[u8],
    p: &mut usize,
    body_end: usize,
    post_buff: &mut PostBuff,
) -> io::Result<()> {
    let saved = *p;
    post_buff::read_post_buff_tail(body, p, post_buff)?;
    if *p != body_end {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "post-buff consumed {} bytes (started at {}), body_end={}",
                *p - saved,
                saved,
                body_end
            ),
        ));
    }
    Ok(())
}

/// Walk every buff in every level using the cache + per-type_id probing.
/// Returns the decoded matrix and the cursor position after the buff list.
fn try_parse_buff_levels(
    body: &[u8],
    mut p: usize,
    format: SkillFormat,
    cache: &mut TailSizeCache,
) -> io::Result<(Vec<Vec<BuffData>>, usize)> {
    let body_end = body.len();
    let level_count = u32::read_from(body, &mut p)? as usize;
    if level_count > 100 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("implausible level_count={}", level_count),
        ));
    }
    let mut levels = Vec::with_capacity(level_count);
    for lev in 0..level_count {
        let buff_count = u32::read_from(body, &mut p)? as usize;
        if buff_count > 200 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("implausible buff_count={}", buff_count),
            ));
        }
        let mut buffs = Vec::with_capacity(buff_count);
        for bi in 0..buff_count {
            let flag = u8::read_from(body, &mut p)?;
            if flag != 0 {
                buffs.push(BuffData { flag, body: None });
                continue;
            }
            let mut body_bd = buff_data::read_common_base(body, &mut p, format)?;
            let tid = body_bd.type_id;

            let tail_size = if let Some(&sz) = cache.get(&tid) {
                sz
            } else {
                let remaining_buffs = buff_count - bi - 1;
                let remaining_levels = level_count - lev - 1;
                let found = probe_subclass_tail(
                    body, p, remaining_buffs, remaining_levels, body_end, format, cache,
                )
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("subclass-tail probe failed for type_id={}", tid),
                    )
                })?;
                cache.insert(tid, found);
                found
            };

            if p + tail_size > body_end {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    format!(
                        "subclass tail of {} bytes for type_id={} would overrun body",
                        tail_size, tid
                    ),
                ));
            }
            body_bd.subclass_tail = body[p..p + tail_size].to_vec();
            p += tail_size;

            buffs.push(BuffData {
                flag: 0,
                body: Some(body_bd),
            });
        }
        levels.push(buffs);
    }
    Ok((levels, p))
}

/// Try sizes 0..=PROBE_MAX_TAIL for the current buff's subclass tail. For
/// each candidate, walk the *remaining* buffs (cache-only — won't recurse
/// into another probe) and verify the post-buff section parses to body_end.
fn probe_subclass_tail(
    body: &[u8],
    p_after_common: usize,
    remaining_buffs: usize,
    remaining_levels: usize,
    body_end: usize,
    format: SkillFormat,
    cache: &TailSizeCache,
) -> Option<usize> {
    for try_sz in 0..=PROBE_MAX_TAIL {
        let test_p = p_after_common + try_sz;
        if test_p > body_end {
            return None;
        }
        if try_parse_remaining_cache_only(
            body,
            test_p,
            remaining_buffs,
            remaining_levels,
            body_end,
            format,
            cache,
        ) {
            return Some(try_sz);
        }
    }
    None
}

/// Walk the remaining buffs + levels using only the existing cache (no
/// nested probing), then check post-buff parses to body_end exactly.
/// Returns true on a clean parse.
fn try_parse_remaining_cache_only(
    body: &[u8],
    mut p: usize,
    remaining_buffs: usize,
    remaining_levels: usize,
    body_end: usize,
    format: SkillFormat,
    cache: &TailSizeCache,
) -> bool {
    // Remaining buffs in current level
    for _ in 0..remaining_buffs {
        if p >= body_end {
            return false;
        }
        let flag = body[p];
        p += 1;
        if flag != 0 {
            continue;
        }
        let mut p_mut = p;
        let bd = match buff_data::read_common_base(body, &mut p_mut, format) {
            Ok(v) => v,
            Err(_) => return false,
        };
        p = p_mut;
        let tid = bd.type_id;
        let Some(&sz) = cache.get(&tid) else {
            return false;
        };
        p += sz;
        if p > body_end {
            return false;
        }
    }
    // Remaining levels
    for _ in 0..remaining_levels {
        if p + 4 > body_end {
            return false;
        }
        let bc = u32::from_le_bytes([body[p], body[p + 1], body[p + 2], body[p + 3]]) as usize;
        p += 4;
        for _ in 0..bc {
            if p >= body_end {
                return false;
            }
            let flag = body[p];
            p += 1;
            if flag != 0 {
                continue;
            }
            let mut p_mut = p;
            let bd = match buff_data::read_common_base(body, &mut p_mut, format) {
                Ok(v) => v,
                Err(_) => return false,
            };
            p = p_mut;
            let tid = bd.type_id;
            let Some(&sz) = cache.get(&tid) else {
                return false;
            };
            p += sz;
            if p > body_end {
                return false;
            }
        }
    }
    post_buff::try_parse_post_buff_end(body, p) == Some(body_end)
}

/// Brute-force scan for the post-buff start position. Tries every offset
/// from `min_start` upward; the first one whose `try_parse_post_buff_end`
/// hits `body.len()` AND whose strict parse succeeds is returned.
fn find_post_buff_start(body: &[u8], min_start: usize) -> Option<usize> {
    let body_end = body.len();
    for start in min_start..body_end {
        if post_buff::try_parse_post_buff_end(body, start) != Some(body_end) {
            continue;
        }
        let mut p = start;
        let mut scratch = PostBuff::default();
        match post_buff::read_post_buff_tail(body, &mut p, &mut scratch) {
            Ok(()) if p == body_end => return Some(start),
            _ => continue,
        }
    }
    None
}

// ── Format auto-detection ─────────────────────────────────────────────────

/// Detect the BuffData format by probing the first entry that has a
/// non-null first buff. Mirrors Python `_detect_format`: under each
/// candidate format, read the first buff's common base, then brute-force
/// subclass-tail sizes 0..=PROBE_MAX_TAIL looking for one where
/// `try_parse_post_buff_end` reaches body_end exactly. The format that
/// admits any such size wins. Ties resolve to `WithField58` (the order
/// of iteration), which is the modern format and the right default.
fn detect_format(
    pabgb: &[u8],
    index: &[SkillIndexEntry],
    ranges: &[(usize, usize)],
) -> io::Result<SkillFormat> {
    for &candidate in &[SkillFormat::WithField58, SkillFormat::NoField58] {
        for ((start, end), _idx) in ranges.iter().zip(index.iter()) {
            let entry = &pabgb[*start..*end];
            let Some(body) = entry_body(entry) else { continue };
            let mut head_end = 0;
            if post_buff::read_entry_head(body, &mut head_end).is_err() {
                continue;
            }
            let mut p = head_end;
            let Ok(level_count) = u32::read_from(body, &mut p) else { continue };
            if level_count == 0 {
                continue;
            }
            let Ok(buff_count) = u32::read_from(body, &mut p) else { continue };
            if buff_count == 0 {
                continue;
            }
            let Ok(flag) = u8::read_from(body, &mut p) else { continue };
            if flag != 0 {
                // First buff is null — can't probe with this entry.
                break;
            }
            // Read common base under `candidate` starting after the flag
            // byte. We don't propagate errors — this is just a probe.
            if buff_data::read_common_base(body, &mut p, candidate).is_err() {
                break; // candidate is wrong shape; try the other candidate
            }
            // Try every plausible subclass tail size.
            for try_sz in 0..=PROBE_MAX_TAIL {
                let test_p = p + try_sz;
                if test_p > body.len() {
                    break;
                }
                if post_buff::try_parse_post_buff_end(body, test_p) == Some(body.len()) {
                    return Ok(candidate);
                }
            }
            // This entry didn't admit a size under this format; stop and
            // try the other candidate (the Python heuristic only probes
            // the first non-null-buff entry it finds).
            break;
        }
    }
    Ok(SkillFormat::WithField58)
}

/// Slice off the body of an entry: skips `u32 name_len + name + u32 key`.
/// Returns `None` if the header is malformed.
fn entry_body(entry: &[u8]) -> Option<&[u8]> {
    entry.get(crate::binary::row_head::read_row_head(entry, 4)?.body_at..)
}

// ── Header helpers ────────────────────────────────────────────────────────

/// Read `u32 len + len bytes`.
pub(crate) fn read_name_bytes(data: &[u8], offset: &mut usize) -> io::Result<Vec<u8>> {
    let len = u32::read_from(data, offset)? as usize;
    if *offset + len > data.len() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            format!(
                "name_bytes: need {} bytes at {}, have {}",
                len,
                *offset,
                data.len() - *offset
            ),
        ));
    }
    let bytes = data[*offset..*offset + len].to_vec();
    *offset += len;
    Ok(bytes)
}

pub(crate) fn write_name_bytes<W: io::Write>(w: &mut W, bytes: &[u8]) -> io::Result<()> {
    (bytes.len() as u32).write_to(w)?;
    w.write_all(bytes)
}
