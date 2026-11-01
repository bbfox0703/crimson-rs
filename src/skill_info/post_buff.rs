//! Post-buff skill fields. ~25 named fields that followed the buff-level
//! matrix through 2.03, ending with `_videoPath: u32`. Field names mirror the
//! IDA-decomp / Python parser names.
//!
//! 2.04 kept the struct and reordered it around the matrix: the u8 flags and
//! several u32s and lists moved ahead of it (see [`read_entry_head`]), the rest
//! stayed behind it in a new order (see [`read_post_buff_tail`]). The name still
//! says "post-buff", which is now only half true.
//!
//! This module also exposes a *try-parse* validator
//! (`try_parse_post_buff_end`) used by the BuffData subclass-tail probe:
//! it walks the post-buff layout from a candidate offset and returns the
//! resulting end position iff the parse hits the body end exactly. The
//! validator tolerates errors (returns `None`) instead of propagating
//! them — that's the contract the probing loop needs.

use std::io;

use crate::binary::{BinaryRead, BinaryWrite};

#[derive(Debug, Clone, Default)]
pub struct ResourceStat {
    pub stat_type: u8,
    pub stat_hash: u32,
    pub flag: u8,
    pub value: i64,
    pub hash2: u32,
    pub hash3: u32,
}

#[derive(Debug, Clone, Default)]
pub struct Graph {
    pub val0: i64,
    pub val1: i64,
    pub val2: i64,
    pub val3: u32,
}

#[derive(Debug, Clone, Default)]
pub struct ResourceItem {
    pub item_hash: u32,
    pub count: i64,
}

#[derive(Debug, Clone, Default)]
pub struct PostBuff {
    pub skill_group_key: u32,
    pub parent_skill: u32,
    pub learn_level: u32,
    pub apply_type: u8,
    pub icon_path: u32,
    pub need_upgrade_item_info: u32,
    pub need_upgrade_item_count_graph: Graph,
    pub need_upgrade_experience_graph: Graph,
    pub usable_character_info_list: Vec<u32>,
    pub usable_condition: Vec<u32>,
    pub learn_knowledge_info: u32,
    pub faction_info: u32,
    pub use_resource_stat_list: Vec<ResourceStat>,
    pub use_resource_item_list: Vec<ResourceItem>,
    pub use_driver_resource_stat_list: Vec<ResourceStat>,
    pub use_battery_stat: i64,
    pub is_ui_use_allowed: u8,
    pub is_learn_use_artifact: u8,
    pub allow_skill_with_low_resource: u8,
    pub is_use_child_pattern_description_buff_data: u8,
    /// Added in Crimson Desert 1.16: a new `u8` between
    /// `is_use_child_pattern_description_buff_data` and `damage_type`.
    ///
    /// Every entry gains exactly one byte (per-entry delta +1 on 1,977 of the
    /// 1,997 keys common to 1.15 and 1.16), and a tandem byte-walk against the
    /// 1.15 `skill.pabgb` puts the insert on the `damage_type` boundary in
    /// 796 of 800 sampled entries. Reads 0x00 on every sampled entry, so the
    /// semantic role is unknown — named for its position, matching the
    /// iteminfo `unk_pre_*` convention. Before this field was added, 589 of
    /// the 2,013 1.16 entries failed to parse.
    pub unk_pre_damage_type: u8,
    pub damage_type: u8,
    pub ui_type: u8,
    pub reserve_slot_info_list: Vec<u32>,
    pub max_level: u32,
    pub skill_group_key_list: Vec<u16>,
    pub buff_sustain_flag: u32,
    /// `u32 len + len bytes` — Korean UTF-8 in real data.
    pub dev_skill_name: Vec<u8>,
    pub dev_skill_desc: Vec<u8>,
    pub video_path: u32,
}

fn read_resource_stat(data: &[u8], offset: &mut usize) -> io::Result<ResourceStat> {
    Ok(ResourceStat {
        stat_type: u8::read_from(data, offset)?,
        stat_hash: u32::read_from(data, offset)?,
        flag: u8::read_from(data, offset)?,
        value: i64::read_from(data, offset)?,
        hash2: u32::read_from(data, offset)?,
        hash3: u32::read_from(data, offset)?,
    })
}

fn write_resource_stat<W: io::Write>(w: &mut W, rs: &ResourceStat) -> io::Result<()> {
    rs.stat_type.write_to(w)?;
    rs.stat_hash.write_to(w)?;
    rs.flag.write_to(w)?;
    rs.value.write_to(w)?;
    rs.hash2.write_to(w)?;
    rs.hash3.write_to(w)?;
    Ok(())
}

fn read_graph(data: &[u8], offset: &mut usize) -> io::Result<Graph> {
    Ok(Graph {
        val0: i64::read_from(data, offset)?,
        val1: i64::read_from(data, offset)?,
        val2: i64::read_from(data, offset)?,
        val3: u32::read_from(data, offset)?,
    })
}

fn write_graph<W: io::Write>(w: &mut W, g: &Graph) -> io::Result<()> {
    g.val0.write_to(w)?;
    g.val1.write_to(w)?;
    g.val2.write_to(w)?;
    g.val3.write_to(w)?;
    Ok(())
}

fn read_list_u32(data: &[u8], offset: &mut usize) -> io::Result<Vec<u32>> {
    let cnt = u32::read_from(data, offset)? as usize;
    let mut out = Vec::with_capacity(cnt);
    for _ in 0..cnt {
        out.push(u32::read_from(data, offset)?);
    }
    Ok(out)
}

fn write_list_u32<W: io::Write>(w: &mut W, list: &[u32]) -> io::Result<()> {
    (list.len() as u32).write_to(w)?;
    for v in list {
        v.write_to(w)?;
    }
    Ok(())
}

fn read_var_bytes(data: &[u8], offset: &mut usize) -> io::Result<Vec<u8>> {
    let len = u32::read_from(data, offset)? as usize;
    if *offset + len > data.len() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            format!(
                "var_bytes: need {} bytes at {}, have {}",
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

fn write_var_bytes<W: io::Write>(w: &mut W, bytes: &[u8]) -> io::Result<()> {
    (bytes.len() as u32).write_to(w)?;
    w.write_all(bytes)?;
    Ok(())
}

/// The part of an entry ahead of the buff matrix: the fields below that 2.04
/// moved there, plus the three bytes [`PostBuff`] does not hold.
#[derive(Debug, Clone, Default)]
pub struct EntryHead {
    /// Sat right after the name through 2.03, where it was taken for a NUL
    /// terminator; reads 0 on every entry.
    pub unk_post_key: u8,
    pub is_blocked: u8,
    pub pad_01: [u8; 3],
}

fn read_resource_stats(data: &[u8], offset: &mut usize) -> io::Result<Vec<ResourceStat>> {
    let cnt = u32::read_from(data, offset)? as usize;
    let mut out = Vec::with_capacity(cnt.min(data.len()));
    for _ in 0..cnt {
        out.push(read_resource_stat(data, offset)?);
    }
    Ok(out)
}

fn write_resource_stats<W: io::Write>(w: &mut W, list: &[ResourceStat]) -> io::Result<()> {
    (list.len() as u32).write_to(w)?;
    for rs in list {
        write_resource_stat(w, rs)?;
    }
    Ok(())
}

/// Read what 2.04 stores ahead of the buff matrix: everything between the key
/// and `level_count`. The rest of `PostBuff` is [`read_post_buff_tail`].
///
/// `learn_level`, `faction_info` and `need_upgrade_item_info` read 0 on (nearly)
/// every entry, and three all-zero u32 slots here hold them, so which is which
/// keeps the order they had rather than anything the data says.
pub fn read_entry_head(data: &[u8], offset: &mut usize) -> io::Result<(EntryHead, PostBuff)> {
    let mut p = PostBuff::default();
    let mut head = EntryHead {
        unk_post_key: u8::read_from(data, offset)?,
        ..EntryHead::default()
    };
    p.is_learn_use_artifact = u8::read_from(data, offset)?;
    p.allow_skill_with_low_resource = u8::read_from(data, offset)?;
    p.ui_type = u8::read_from(data, offset)?;
    p.is_ui_use_allowed = u8::read_from(data, offset)?;
    p.apply_type = u8::read_from(data, offset)?;
    p.damage_type = u8::read_from(data, offset)?;
    p.unk_pre_damage_type = u8::read_from(data, offset)?;
    p.is_use_child_pattern_description_buff_data = u8::read_from(data, offset)?;
    p.learn_level = u32::read_from(data, offset)?;
    p.icon_path = u32::read_from(data, offset)?;
    p.parent_skill = u32::read_from(data, offset)?;
    p.learn_knowledge_info = u32::read_from(data, offset)?;
    p.faction_info = u32::read_from(data, offset)?;
    p.buff_sustain_flag = u32::read_from(data, offset)?;
    p.max_level = u32::read_from(data, offset)?;
    p.skill_group_key = u32::read_from(data, offset)?;
    p.need_upgrade_item_info = u32::read_from(data, offset)?;
    // Small non-zero values (3, 5, 10) on a few entries, so the name was always
    // a guess and "pad" is really the high bytes of one u32.
    head.is_blocked = u8::read_from(data, offset)?;
    for b in &mut head.pad_01 {
        *b = u8::read_from(data, offset)?;
    }
    Ok((head, p))
}

/// Read the fields behind the buff matrix into `p`.
pub fn read_post_buff_tail(data: &[u8], offset: &mut usize, p: &mut PostBuff) -> io::Result<()> {
    p.need_upgrade_item_count_graph = read_graph(data, offset)?;
    p.need_upgrade_experience_graph = read_graph(data, offset)?;
    p.usable_character_info_list = read_list_u32(data, offset)?;
    p.usable_condition = read_list_u32(data, offset)?;
    p.use_resource_stat_list = read_resource_stats(data, offset)?;

    let cnt = u32::read_from(data, offset)? as usize;
    p.use_resource_item_list = Vec::with_capacity(cnt.min(data.len()));
    for _ in 0..cnt {
        let item_hash = u32::read_from(data, offset)?;
        let count = i64::read_from(data, offset)?;
        p.use_resource_item_list.push(ResourceItem { item_hash, count });
    }

    p.use_driver_resource_stat_list = read_resource_stats(data, offset)?;
    p.use_battery_stat = i64::read_from(data, offset)?;
    p.reserve_slot_info_list = read_list_u32(data, offset)?;

    let cnt = u32::read_from(data, offset)? as usize;
    p.skill_group_key_list = Vec::with_capacity(cnt.min(data.len()));
    for _ in 0..cnt {
        p.skill_group_key_list.push(u16::read_from(data, offset)?);
    }

    p.dev_skill_name = read_var_bytes(data, offset)?;
    p.dev_skill_desc = read_var_bytes(data, offset)?;
    p.video_path = u32::read_from(data, offset)?;
    Ok(())
}

pub fn write_entry_head<W: io::Write>(w: &mut W, h: &EntryHead, p: &PostBuff) -> io::Result<()> {
    h.unk_post_key.write_to(w)?;
    p.is_learn_use_artifact.write_to(w)?;
    p.allow_skill_with_low_resource.write_to(w)?;
    p.ui_type.write_to(w)?;
    p.is_ui_use_allowed.write_to(w)?;
    p.apply_type.write_to(w)?;
    p.damage_type.write_to(w)?;
    p.unk_pre_damage_type.write_to(w)?;
    p.is_use_child_pattern_description_buff_data.write_to(w)?;
    p.learn_level.write_to(w)?;
    p.icon_path.write_to(w)?;
    p.parent_skill.write_to(w)?;
    p.learn_knowledge_info.write_to(w)?;
    p.faction_info.write_to(w)?;
    p.buff_sustain_flag.write_to(w)?;
    p.max_level.write_to(w)?;
    p.skill_group_key.write_to(w)?;
    p.need_upgrade_item_info.write_to(w)?;
    h.is_blocked.write_to(w)?;
    w.write_all(&h.pad_01)
}

pub fn write_post_buff_tail<W: io::Write>(w: &mut W, p: &PostBuff) -> io::Result<()> {
    write_graph(w, &p.need_upgrade_item_count_graph)?;
    write_graph(w, &p.need_upgrade_experience_graph)?;
    write_list_u32(w, &p.usable_character_info_list)?;
    write_list_u32(w, &p.usable_condition)?;
    write_resource_stats(w, &p.use_resource_stat_list)?;
    (p.use_resource_item_list.len() as u32).write_to(w)?;
    for ri in &p.use_resource_item_list {
        ri.item_hash.write_to(w)?;
        ri.count.write_to(w)?;
    }
    write_resource_stats(w, &p.use_driver_resource_stat_list)?;
    p.use_battery_stat.write_to(w)?;
    write_list_u32(w, &p.reserve_slot_info_list)?;
    (p.skill_group_key_list.len() as u32).write_to(w)?;
    for v in &p.skill_group_key_list {
        v.write_to(w)?;
    }
    write_var_bytes(w, &p.dev_skill_name)?;
    write_var_bytes(w, &p.dev_skill_desc)?;
    p.video_path.write_to(w)
}

/// Bounded validator used by the subclass-tail probe, over the fields behind
/// the buff matrix. Mirrors the Python `_try_parse_post_buff` heuristics:
/// per-list element count caps + a UTF-8 plausibility check on the dev_* strings.
///
/// Returns the position the parse ended at (which the caller compares to
/// `body_end`), or `None` on any failure.
pub fn try_parse_post_buff_end(data: &[u8], mut p: usize) -> Option<usize> {
    let body_len = data.len();

    // Counted list, capped; `size` bytes per element.
    let skip_list = |p: usize, size: usize| -> Option<usize> {
        let cnt = read_u32_at(data, p)?;
        if cnt > 50 {
            return None;
        }
        let end = p.checked_add(4 + (cnt as usize) * size)?;
        (end <= body_len).then_some(end)
    };
    // `u32 len + len bytes`: cap 2000, must be valid UTF-8 if non-empty.
    let skip_text = |p: usize| -> Option<usize> {
        let slen = read_u32_at(data, p)?;
        if slen > 2000 {
            return None;
        }
        let end = p.checked_add(4 + slen as usize)?;
        if end > body_len || std::str::from_utf8(&data[end - slen as usize..end]).is_err() {
            return None;
        }
        Some(end)
    };

    // _needUpgradeItemCountGraph + _needUpgradeExperienceGraph
    p = p.checked_add(28 + 28)?;
    if p > body_len {
        return None;
    }

    p = skip_list(p, 4)?; // _usableCharacterInfoList
    p = skip_list(p, 4)?; // _usableCondition
    p = skip_list(p, 22)?; // _useResourceStatList
    p = skip_list(p, 12)?; // _useResourceItemList
    p = skip_list(p, 22)?; // _useDriverResourceStatList

    p = p.checked_add(8)?; // _useBatteryStat
    if p > body_len {
        return None;
    }

    p = skip_list(p, 4)?; // _reserveSlotInfoList
    p = skip_list(p, 2)?; // _skillGroupKeyList

    p = skip_text(p)?; // _devSkillName
    p = skip_text(p)?; // _devSkillDesc

    p = p.checked_add(4)?; // _videoPath
    if p > body_len {
        return None;
    }

    Some(p)
}

#[inline]
fn read_u32_at(data: &[u8], at: usize) -> Option<u32> {
    if at + 4 > data.len() {
        return None;
    }
    Some(u32::from_le_bytes([
        data[at],
        data[at + 1],
        data[at + 2],
        data[at + 3],
    ]))
}
