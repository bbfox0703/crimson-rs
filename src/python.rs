use pyo3::exceptions::{PyIOError, PyKeyError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList};

use crate::binary::pamt::PackMeta;
use crate::binary::papgt::PackGroupTreeMeta;
use crate::binary::*;
use crate::item_info::ItemInfo;
use crate::skill_info::{
    BuffData, BuffDataBody, Graph, PostBuff, ResourceItem, ResourceStat, SkillData, SkillEntry,
    SkillFormat, SkillIndexEntry,
};

// ── Dict helpers ───────────────────────────────────────────────────────────

fn get<'py, T>(d: &Bound<'py, PyDict>, key: &str) -> PyResult<T>
where
    for<'a> T: FromPyObject<'a, 'py, Error = PyErr>,
{
    d.get_item(key)?
        .ok_or_else(|| PyKeyError::new_err(key.to_string()))?
        .extract()
}

fn get_obj<'py>(d: &Bound<'py, PyDict>, key: &str) -> PyResult<Bound<'py, PyAny>> {
    d.get_item(key)?
        .ok_or_else(|| PyKeyError::new_err(key.to_string()))
}

// ── ItemInfo Python conversion ─────────────────────────────────────────────

fn to_py_item<'py>(py: Python<'py>, v: &ItemInfo) -> PyResult<Bound<'py, PyDict>> {
    v.to_py_dict(py)
}

fn wr_item(w: &mut Vec<u8>, obj: &Bound<'_, PyAny>) -> PyResult<()> {
    let d = obj.cast::<PyDict>()?;
    ItemInfo::write_from_py_dict(w, d)
}

// ── Module functions ───────────────────────────────────────────────────────

#[pyfunction]
pub fn parse_iteminfo_from_file(py: Python<'_>, path: &str) -> PyResult<Py<PyAny>> {
    let data = std::fs::read(path).map_err(|e| PyIOError::new_err(e.to_string()))?;
    parse_iteminfo_from_bytes_inner(py, &data)
}

#[pyfunction]
pub fn parse_iteminfo_from_bytes(py: Python<'_>, data: &[u8]) -> PyResult<Py<PyAny>> {
    parse_iteminfo_from_bytes_inner(py, data)
}

pub fn parse_iteminfo_from_bytes_inner(py: Python<'_>, data: &[u8]) -> PyResult<Py<PyAny>> {
    let mut offset = 0;
    let mut items = Vec::new();
    while offset < data.len() {
        let item = ItemInfo::read_from(data, &mut offset).map_err(|e| {
            PyValueError::new_err(format!("parse error at offset 0x{:08X}: {}", offset, e))
        })?;
        items.push(to_py_item(py, &item)?);
    }
    Ok(PyList::new(py, items)?.into_any().unbind())
}

/// Try to find the next plausible item start at or after `from_offset`.
///
/// An item begins with `u32 key` then `u32 len` then `len` bytes of an ASCII
/// `string_key`. We scan byte-by-byte for a position where these three checks
/// hold:
///   - `key` is a 32-bit value with the high byte zero (game item keys are
///     comfortably below 2^24);
///   - `len` is between 2 and 64 (string keys are short identifiers);
///   - the next `len` bytes are printable ASCII followed by a NUL byte.
fn scan_next_item_start(data: &[u8], from_offset: usize) -> Option<usize> {
    let n = data.len();
    let mut o = from_offset;
    while o + 12 < n {
        let key = u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
        if key != 0 && (key >> 24) == 0 {
            let slen = u32::from_le_bytes([
                data[o + 4],
                data[o + 5],
                data[o + 6],
                data[o + 7],
            ]) as usize;
            if (2..=128).contains(&slen) && o + 8 + slen < n {
                let bytes = &data[o + 8..o + 8 + slen];
                let mut all_ident = true;
                for &b in bytes {
                    // ASCII word chars / space, OR any UTF-8 high byte
                    // (1.05 string_keys can contain Ⅲ/Ⅳ/Ⅵ etc.).
                    let ok = b.is_ascii_alphanumeric() || b == b'_' || b == b' ' || b >= 0x80;
                    if !ok {
                        all_ident = false;
                        break;
                    }
                }
                if all_ident && data[o + 8 + slen] == 0 {
                    return Some(o);
                }
            }
        }
        o += 1;
    }
    None
}

/// Lossy parser: like `parse_iteminfo_tracked`, but on error scans forward to
/// the next plausible item start and continues. Returns a dict with `items`,
/// `spans`, `errors` (list of `{item_start, fail_offset, recovered_at, ...}`).
#[pyfunction]
pub fn parse_iteminfo_lossy(py: Python<'_>, data: &[u8]) -> PyResult<Py<PyAny>> {
    use crate::binary::{BinaryReadTracked, FieldRange};

    let mut offset = 0;
    let mut py_items = Vec::new();
    let mut py_spans = Vec::new();
    let py_errors = PyList::empty(py);

    while offset + 12 < data.len() {
        let start = offset;
        let mut path_buf = String::new();
        let mut ranges: Vec<FieldRange> = Vec::new();

        match ItemInfo::read_tracked(data, &mut offset, &mut path_buf, &mut ranges) {
            Ok(item) => {
                py_items.push(to_py_item(py, &item)?);
                let span = PyDict::new(py);
                span.set_item("start", start)?;
                span.set_item("end", offset)?;
                span.set_item("size", offset - start)?;
                py_spans.push(span.into_any().unbind());
            }
            Err(e) => {
                let err = PyDict::new(py);
                err.set_item("item_start", start)?;
                err.set_item("fail_offset", offset)?;
                err.set_item("path", path_buf.clone())?;
                err.set_item("message", e.to_string())?;
                let next = scan_next_item_start(data, start + 1).unwrap_or(data.len());
                err.set_item("recovered_at", next)?;
                err.set_item("skipped_bytes", next - start)?;
                py_errors.append(err)?;
                offset = next;
                if offset >= data.len() {
                    break;
                }
            }
        }
    }

    let result = PyDict::new(py);
    result.set_item("items", PyList::new(py, py_items)?)?;
    result.set_item("spans", PyList::new(py, py_spans)?)?;
    result.set_item("errors", py_errors)?;
    Ok(result.into_any().unbind())
}

#[pyfunction]
pub fn parse_iteminfo_tracked(py: Python<'_>, data: &[u8]) -> PyResult<Py<PyAny>> {
    use crate::binary::{BinaryReadTracked, FieldRange};

    let mut offset = 0;
    let mut py_items = Vec::new();
    let mut py_spans = Vec::new();

    let mut error_msg: Option<String> = None;
    let mut error_span: Option<Py<PyAny>> = None;

    while offset + 8 < data.len() {
        let start = offset;
        let mut path_buf = String::new();
        let mut ranges: Vec<FieldRange> = Vec::new();

        match ItemInfo::read_tracked(data, &mut offset, &mut path_buf, &mut ranges) {
            Ok(item) => {
                py_items.push(to_py_item(py, &item)?);

                let span = PyDict::new(py);
                span.set_item("start", start)?;
                span.set_item("end", offset)?;
                span.set_item("size", offset - start)?;

                let py_ranges = PyList::empty(py);
                for r in &ranges {
                    let rd = PyDict::new(py);
                    rd.set_item("path", &r.path)?;
                    rd.set_item("start", r.start)?;
                    rd.set_item("end", r.end)?;
                    rd.set_item("ty", r.ty)?;
                    py_ranges.append(rd)?;
                }
                span.set_item("ranges", py_ranges)?;
                py_spans.push(span.into_any().unbind());
            }
            Err(e) => {
                // Capture partial ranges to help diagnose where parsing broke.
                let span = PyDict::new(py);
                span.set_item("start", start)?;
                span.set_item("end", offset)?;
                span.set_item("size", offset - start)?;
                span.set_item("path", path_buf.clone())?;
                let py_ranges = PyList::empty(py);
                for r in &ranges {
                    let rd = PyDict::new(py);
                    rd.set_item("path", &r.path)?;
                    rd.set_item("start", r.start)?;
                    rd.set_item("end", r.end)?;
                    rd.set_item("ty", r.ty)?;
                    py_ranges.append(rd)?;
                }
                span.set_item("ranges", py_ranges)?;
                error_msg = Some(format!("at offset 0x{:08X} (path={}): {}", offset, path_buf, e));
                error_span = Some(span.into_any().unbind());
                break;
            }
        }
    }

    let result = PyDict::new(py);
    result.set_item("items", PyList::new(py, py_items)?)?;
    result.set_item("spans", PyList::new(py, py_spans)?)?;
    if let Some(msg) = error_msg {
        result.set_item("error", msg)?;
    }
    if let Some(sp) = error_span {
        result.set_item("error_span", sp)?;
    }
    Ok(result.into_any().unbind())
}

/// Resolve `(entry_name, rel_offset, length)` byte patches to field-path
/// attributions on a vanilla iteminfo blob.
///
/// `vanilla_bytes` is a vanilla `iteminfo.pabgb`. `patches` is a list of
/// dicts shaped `{"entry": str, "rel_offset": int, "length": int?}`. For
/// each patch this returns a dict with the field whose `[start, end)`
/// covers `entry.start + rel_offset`, or `None` if the entry is missing
/// or the offset falls outside any tracked field. `length` is optional
/// and is echoed back as `hit_length` for the caller's bookkeeping —
/// this function only attributes the *start* of the patch, not the span.
///
/// Returned dict shape (per non-None entry):
///   - path: dotted field path (e.g. `"enchant_data_list.2.level"`)
///   - ty: Rust type name of the field
///   - abs_start, abs_end: absolute byte range of the field
///   - hit_offset: `abs_pos - abs_start` (offset into the field)
///   - hit_length: echo of input `length` (default 0 if not provided)
#[pyfunction]
pub fn inspect_legacy_patches(
    py: Python<'_>,
    vanilla_bytes: &[u8],
    patches: &Bound<'_, PyList>,
) -> PyResult<Py<PyAny>> {
    use crate::binary::{BinaryReadTracked, FieldRange};
    use std::collections::HashMap;

    // Parse vanilla once, recording (string_key, span_start, span_end, ranges)
    // per item. First occurrence wins on duplicate string_keys (none expected
    // in 1.05 but cheaper than panicking).
    let mut offset = 0;
    let mut entries: Vec<(String, usize, usize, Vec<FieldRange>)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();

    while offset + 12 < vanilla_bytes.len() {
        let span_start = offset;
        let mut path_buf = String::new();
        let mut ranges: Vec<FieldRange> = Vec::new();
        let item = ItemInfo::read_tracked(vanilla_bytes, &mut offset, &mut path_buf, &mut ranges)
            .map_err(|e| {
                PyValueError::new_err(format!(
                    "parse error at offset 0x{:08X}: {}",
                    offset, e
                ))
            })?;
        let key = item.string_key.data.to_string();
        let i = entries.len();
        index.entry(key.clone()).or_insert(i);
        entries.push((key, span_start, offset, ranges));
    }

    // Resolve each patch
    let result = PyList::empty(py);
    for patch in patches.iter() {
        let d = patch.cast::<PyDict>()?;
        let entry_name: String = get(d, "entry")?;
        let rel_offset: usize = get(d, "rel_offset")?;
        let length: usize = match d.get_item("length")? {
            Some(v) => v.extract()?,
            None => 0,
        };

        let Some(&item_idx) = index.get(&entry_name) else {
            result.append(py.None())?;
            continue;
        };
        let (_, span_start, span_end, ranges) = &entries[item_idx];
        let abs_pos = span_start.saturating_add(rel_offset);
        if abs_pos >= *span_end {
            result.append(py.None())?;
            continue;
        }

        // Ranges are sorted-by-start, contiguous, and disjoint (covering
        // every byte of the entry), so binary-search by `[start, end)`.
        let hit = ranges.binary_search_by(|r| {
            if abs_pos < r.start {
                std::cmp::Ordering::Greater
            } else if abs_pos >= r.end {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        });

        match hit {
            Ok(i) => {
                let r = &ranges[i];
                let dd = PyDict::new(py);
                dd.set_item("path", &r.path)?;
                dd.set_item("ty", r.ty)?;
                dd.set_item("abs_start", r.start)?;
                dd.set_item("abs_end", r.end)?;
                dd.set_item("hit_offset", abs_pos - r.start)?;
                dd.set_item("hit_length", length)?;
                result.append(dd)?;
            }
            Err(_) => result.append(py.None())?,
        }
    }

    Ok(result.into_any().unbind())
}

#[pyfunction]
pub fn write_iteminfo_to_file(items: &Bound<'_, PyList>, path: &str) -> PyResult<()> {
    let data = serialize_iteminfo_impl(items)?;
    std::fs::write(path, data).map_err(|e| PyIOError::new_err(e.to_string()))
}

#[pyfunction]
pub fn serialize_iteminfo(py: Python<'_>, items: &Bound<'_, PyList>) -> PyResult<Py<PyAny>> {
    let data = serialize_iteminfo_impl(items)?;
    Ok(PyBytes::new(py, &data).into_any().unbind())
}

pub fn serialize_iteminfo_impl(items: &Bound<'_, PyList>) -> PyResult<Vec<u8>> {
    let mut buf = Vec::new();
    for item in items.iter() {
        wr_item(&mut buf, &item)?;
    }
    Ok(buf)
}

// ── PAPGT to/from Python ───────────────────────────────────────────────────

pub fn to_py_papgt<'py>(
    py: Python<'py>,
    papgt: &PackGroupTreeMeta,
) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("unknown0", papgt.header.unknown0)?;
    d.set_item("checksum", papgt.header.checksum)?;
    d.set_item("unknown1", papgt.header.unknown1)?;
    d.set_item("unknown2", papgt.header.unknown2)?;

    let entries = PyList::empty(py);
    for entry in &papgt.entries {
        let ed = PyDict::new(py);
        ed.set_item("group_name", &entry.group_name)?;
        ed.set_item("is_optional", entry.entry.is_optional)?;
        ed.set_item("language", entry.entry.language.0)?;
        ed.set_item("always_zero", entry.entry.always_zero)?;
        ed.set_item("group_name_offset", entry.entry.group_name_offset)?;
        ed.set_item("pack_meta_checksum", entry.entry.pack_meta_checksum)?;
        entries.append(ed)?;
    }
    d.set_item("entries", entries)?;
    Ok(d)
}

pub fn wr_papgt_from_dict(d: &Bound<'_, PyDict>) -> PyResult<Vec<u8>> {
    // We need the original raw data for roundtrip. Since we preserve all raw offsets
    // and the group_names_buffer, we reconstruct the PackGroupTreeMeta from the dict.
    use crate::binary::papgt::*;

    let unknown0: u32 = get(d, "unknown0")?;
    let unknown1: u8 = get(d, "unknown1")?;
    let unknown2: u16 = get(d, "unknown2")?;
    let entries_list = get_obj(d, "entries")?.cast::<PyList>()?.clone();

    let mut entries = Vec::new();
    let mut group_names_buffer = Vec::new();

    for item in entries_list.iter() {
        let ed = item.cast::<PyDict>()?;
        let group_name: String = get(ed, "group_name")?;
        let is_optional: u8 = get(ed, "is_optional")?;
        let language: u16 = get(ed, "language")?;
        let always_zero: u8 = get(ed, "always_zero")?;
        let group_name_offset: u32 = get(ed, "group_name_offset")?;
        let pack_meta_checksum: u32 = get(ed, "pack_meta_checksum")?;

        // Write group name to buffer at the offset
        // For new entries, we'd need to append. For roundtrip, offsets are preserved.
        // Ensure the buffer is large enough
        let needed = group_name_offset as usize + group_name.len() + 1;
        if group_names_buffer.len() < needed {
            group_names_buffer.resize(needed, 0);
        }
        let off = group_name_offset as usize;
        group_names_buffer[off..off + group_name.len()].copy_from_slice(group_name.as_bytes());
        group_names_buffer[off + group_name.len()] = 0; // null terminator

        entries.push(ResolvedEntry {
            group_name,
            entry: PackGroupTreeMetaEntry {
                is_optional,
                language: LanguageType(language),
                always_zero,
                group_name_offset,
                pack_meta_checksum,
            },
        });
    }

    let papgt = PackGroupTreeMeta {
        header: PackGroupTreeMetaHeader {
            unknown0,
            checksum: 0, // will be recalculated by write()
            entry_count: entries.len() as u8,
            unknown1,
            unknown2,
        },
        entries,
        group_names_buffer,
    };

    papgt
        .to_bytes()
        .map_err(|e| PyIOError::new_err(e.to_string()))
}

#[pyfunction]
pub fn parse_papgt_file(py: Python<'_>, path: &str) -> PyResult<Py<PyAny>> {
    let data = std::fs::read(path).map_err(|e| PyIOError::new_err(e.to_string()))?;
    parse_papgt_bytes_inner(py, &data)
}

#[pyfunction]
pub fn parse_papgt_bytes(py: Python<'_>, data: &[u8]) -> PyResult<Py<PyAny>> {
    parse_papgt_bytes_inner(py, data)
}

pub fn parse_papgt_bytes_inner(py: Python<'_>, data: &[u8]) -> PyResult<Py<PyAny>> {
    let papgt = PackGroupTreeMeta::parse(data).map_err(|e| PyValueError::new_err(e.to_string()))?;
    Ok(to_py_papgt(py, &papgt)?.into_any().unbind())
}

#[pyfunction]
pub fn write_papgt_file(data: &Bound<'_, PyDict>, path: &str) -> PyResult<()> {
    let bytes = wr_papgt_from_dict(data)?;
    std::fs::write(path, bytes).map_err(|e| PyIOError::new_err(e.to_string()))
}

#[pyfunction]
pub fn serialize_papgt(py: Python<'_>, data: &Bound<'_, PyDict>) -> PyResult<Py<PyAny>> {
    let bytes = wr_papgt_from_dict(data)?;
    Ok(PyBytes::new(py, &bytes).into_any().unbind())
}

// ── PAMT to/from Python ───────────────────────────────────────────────────

pub fn to_py_pamt<'py>(py: Python<'py>, pamt: &PackMeta) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("checksum", pamt.header.checksum)?;
    d.set_item("unknown0", pamt.header.unknown0)?;

    // Encrypt info
    let ei = PyDict::new(py);
    ei.set_item("unknown0", pamt.header.encrypt_info.unknown0)?;
    let ei_bytes = PyBytes::new(py, &pamt.header.encrypt_info.encrypt_info);
    ei.set_item("encrypt_info", ei_bytes)?;
    d.set_item("encrypt_info", ei)?;

    // Chunks
    let chunks = PyList::empty(py);
    for chunk in &pamt.chunks {
        let cd = PyDict::new(py);
        cd.set_item("id", chunk.id)?;
        cd.set_item("checksum", chunk.checksum)?;
        cd.set_item("size", chunk.size)?;
        chunks.append(cd)?;
    }
    d.set_item("chunks", chunks)?;

    // Directories (resolved)
    let dirs = PyList::empty(py);
    for dir in &pamt.directories {
        let dd = PyDict::new(py);
        dd.set_item("path", &dir.path)?;
        dd.set_item("name_checksum", dir.raw.name_checksum)?;
        dd.set_item("name_offset", dir.raw.name_offset)?;
        dd.set_item("file_start_index", dir.raw.file_start_index)?;
        dd.set_item("file_count", dir.raw.file_count)?;

        let files = PyList::empty(py);
        for f in &dir.files {
            let fd = PyDict::new(py);
            fd.set_item("name", &f.name)?;
            fd.set_item("name_offset", f.file.name_offset)?;
            fd.set_item("chunk_offset", f.file.chunk_offset)?;
            fd.set_item("compressed_size", f.file.compressed_size)?;
            fd.set_item("uncompressed_size", f.file.uncompressed_size)?;
            fd.set_item("chunk_id", f.file.chunk_id)?;
            fd.set_item("flags", f.file.flags)?;
            fd.set_item("unknown0", f.file.unknown0)?;
            fd.set_item("compression", f.file.compression as u8)?;
            fd.set_item("crypto", f.file.crypto as u8)?;
            fd.set_item("is_partial", f.file.is_partial)?;
            files.append(fd)?;
        }
        dd.set_item("files", files)?;
        dirs.append(dd)?;
    }
    d.set_item("directories", dirs)?;

    // Raw trie buffers for roundtrip writing
    d.set_item(
        "_dir_names_buffer",
        PyBytes::new(py, &pamt.dir_names_buffer),
    )?;
    d.set_item(
        "_file_names_buffer",
        PyBytes::new(py, &pamt.file_names_buffer),
    )?;

    Ok(d)
}

#[pyfunction]
pub fn parse_pamt_file(py: Python<'_>, path: &str) -> PyResult<Py<PyAny>> {
    let data = std::fs::read(path).map_err(|e| PyIOError::new_err(e.to_string()))?;
    parse_pamt_bytes_inner(py, &data)
}

#[pyfunction]
pub fn parse_pamt_bytes(py: Python<'_>, data: &[u8]) -> PyResult<Py<PyAny>> {
    parse_pamt_bytes_inner(py, data)
}

pub fn parse_pamt_bytes_inner(py: Python<'_>, data: &[u8]) -> PyResult<Py<PyAny>> {
    let pamt = PackMeta::parse(data, None).map_err(|e| PyValueError::new_err(e.to_string()))?;
    Ok(to_py_pamt(py, &pamt)?.into_any().unbind())
}

#[pyfunction]
pub fn write_pamt_file(data: &Bound<'_, PyDict>, path: &str) -> PyResult<()> {
    let bytes = wr_pamt_from_dict(data)?;
    std::fs::write(path, bytes).map_err(|e| PyIOError::new_err(e.to_string()))
}

#[pyfunction]
pub fn serialize_pamt(py: Python<'_>, data: &Bound<'_, PyDict>) -> PyResult<Py<PyAny>> {
    let bytes = wr_pamt_from_dict(data)?;
    Ok(PyBytes::new(py, &bytes).into_any().unbind())
}

pub fn wr_pamt_from_dict(d: &Bound<'_, PyDict>) -> PyResult<Vec<u8>> {
    use crate::binary::pamt::*;

    let checksum: u32 = get(d, "checksum")?;
    let unknown0: u16 = get(d, "unknown0")?;

    let ei_obj = get_obj(d, "encrypt_info")?.cast::<PyDict>()?.clone();
    let ei_unknown0: u8 = get(&ei_obj, "unknown0")?;
    let ei_bytes: Vec<u8> = get(&ei_obj, "encrypt_info")?;
    let encrypt_info_arr: [u8; 3] = ei_bytes
        .try_into()
        .map_err(|_| PyValueError::new_err("encrypt_info must be 3 bytes"))?;

    let chunks_list = get_obj(d, "chunks")?.cast::<PyList>()?.clone();
    let mut chunks = Vec::new();
    for c in chunks_list.iter() {
        let cd = c.cast::<PyDict>()?;
        chunks.push(PackMetaChunk {
            id: get(cd, "id")?,
            checksum: get(cd, "checksum")?,
            size: get(cd, "size")?,
        });
    }

    let dirs_list = get_obj(d, "directories")?.cast::<PyList>()?.clone();
    let mut raw_directories = Vec::new();
    let mut raw_files = Vec::new();

    for dir_item in dirs_list.iter() {
        let dd = dir_item.cast::<PyDict>()?;
        let name_checksum: u32 = get(dd, "name_checksum")?;
        let name_offset: i32 = get(dd, "name_offset")?;
        let file_start_index: u32 = get(dd, "file_start_index")?;
        let file_count: u32 = get(dd, "file_count")?;

        raw_directories.push(PackMetaDirectory {
            name_checksum,
            name_offset,
            file_start_index,
            file_count,
        });

        let files_list = get_obj(dd, "files")?.cast::<PyList>()?.clone();
        for f_item in files_list.iter() {
            let fd = f_item.cast::<PyDict>()?;
            raw_files.push(PackMetaFileRaw {
                name_offset: get(fd, "name_offset")?,
                chunk_offset: get(fd, "chunk_offset")?,
                compressed_size: get(fd, "compressed_size")?,
                uncompressed_size: get(fd, "uncompressed_size")?,
                chunk_id: get(fd, "chunk_id")?,
                flags: get(fd, "flags")?,
                unknown0: get(fd, "unknown0")?,
            });
        }
    }

    // Get trie buffers for roundtrip
    let dir_names_buffer: Vec<u8> = get(d, "_dir_names_buffer")?;
    let file_names_buffer: Vec<u8> = get(d, "_file_names_buffer")?;

    let pamt = PackMeta {
        header: PackMetaHeader {
            checksum,
            count: chunks.len() as u16,
            unknown0,
            encrypt_info: PackEncryptInfo {
                unknown0: ei_unknown0,
                encrypt_info: encrypt_info_arr,
            },
        },
        chunks,
        directories: Vec::new(), // not needed for write()
        dir_names_buffer,
        file_names_buffer,
        raw_directories,
        raw_files,
    };

    pamt.to_bytes()
        .map_err(|e| PyIOError::new_err(e.to_string()))
}

// ── Localization to/from Python ────────────────────────────────────────────

fn to_py_paloc_entry<'py>(
    py: Python<'py>,
    entry: &crate::binary::paloc::LocalizationEntry,
) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("unk_id", entry.unk_id)?;
    d.set_item("string_key", entry.string_key.data)?;
    d.set_item("string_value", entry.string_value.data)?;
    Ok(d)
}

#[pyfunction]
pub fn parse_paloc_bytes(py: Python<'_>, data: &[u8]) -> PyResult<Py<PyAny>> {
    let paloc = crate::binary::paloc::LocalizationFile::parse(data)
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    let entries = PyList::empty(py);
    for entry in &paloc.entries {
        entries.append(to_py_paloc_entry(py, entry)?)?;
    }
    Ok(entries.into_any().unbind())
}

#[pyfunction]
pub fn serialize_paloc(py: Python<'_>, items: &Bound<'_, PyList>) -> PyResult<Py<PyAny>> {
    let data = serialize_paloc_impl(items)?;
    Ok(PyBytes::new(py, &data).into_any().unbind())
}

fn serialize_paloc_impl(items: &Bound<'_, PyList>) -> PyResult<Vec<u8>> {
    use crate::binary::BinaryWrite;

    let mut buf = Vec::new();
    for item in items.iter() {
        let d = item.cast::<PyDict>()?;
        let unk_id: u64 = get(d, "unk_id")?;
        let string_key: String = get(d, "string_key")?;
        let string_value: String = get(d, "string_value")?;

        unk_id
            .write_to(&mut buf)
            .map_err(|e| PyIOError::new_err(e.to_string()))?;
        (string_key.len() as u32)
            .write_to(&mut buf)
            .map_err(|e| PyIOError::new_err(e.to_string()))?;
        buf.extend_from_slice(string_key.as_bytes());
        (string_value.len() as u32)
            .write_to(&mut buf)
            .map_err(|e| PyIOError::new_err(e.to_string()))?;
        buf.extend_from_slice(string_value.as_bytes());
    }
    let count = items.len() as u32;
    count
        .write_to(&mut buf)
        .map_err(|e| PyIOError::new_err(e.to_string()))?;
    Ok(buf)
}

// ── Checksum ──────────────────────────────────────────────────────────────

#[pyfunction]
pub fn calculate_checksum(data: &[u8]) -> u32 {
    crate::crypto::checksum::calculate_checksum(data)
}

// ── Compression ──────────────────────────────────────────────────────────

#[pyfunction]
pub fn compress_data(py: Python<'_>, data: &[u8], compression: u8) -> PyResult<Py<PyAny>> {
    use crate::binary::pamt::Compression;
    use crate::binary::paz;

    let comp = match compression {
        0 => Compression::None,
        2 => Compression::Lz4,
        3 => Compression::Zlib,
        _ => {
            return Err(PyValueError::new_err(format!(
                "unsupported compression: {}",
                compression
            )));
        }
    };

    let result = paz::compress(data, comp).map_err(|e| PyIOError::new_err(e.to_string()))?;
    Ok(PyBytes::new(py, &result).into_any().unbind())
}

#[pyfunction]
pub fn decompress_data(
    py: Python<'_>,
    data: &[u8],
    compression: u8,
    uncompressed_size: usize,
) -> PyResult<Py<PyAny>> {
    use crate::binary::pamt::Compression;
    use crate::binary::paz;

    let comp = match compression {
        0 => Compression::None,
        2 => Compression::Lz4,
        3 => Compression::Zlib,
        _ => {
            return Err(PyValueError::new_err(format!(
                "unsupported compression: {}",
                compression
            )));
        }
    };

    let result = paz::decompress(data, comp, uncompressed_size)
        .map_err(|e| PyIOError::new_err(e.to_string()))?;
    Ok(PyBytes::new(py, &result).into_any().unbind())
}

// ── Pack Group Builder (streaming) ───────────────────────────────────────

fn parse_compression(compression: u8) -> PyResult<crate::binary::pamt::Compression> {
    use crate::binary::pamt::Compression;
    match compression {
        0 => Ok(Compression::None),
        2 => Ok(Compression::Lz4),
        3 => Ok(Compression::Zlib),
        _ => Err(PyValueError::new_err(format!(
            "unsupported compression: {}",
            compression
        ))),
    }
}

fn parse_crypto(crypto: u8) -> PyResult<crate::binary::pamt::CryptoType> {
    use crate::binary::pamt::CryptoType;
    match crypto {
        0 => Ok(CryptoType::None),
        3 => Ok(CryptoType::ChaCha20),
        _ => Err(PyValueError::new_err(format!(
            "unsupported crypto: {}",
            crypto
        ))),
    }
}

/// Streaming pack group builder that writes .paz files to disk incrementally.
///
/// Usage:
///     builder = PackGroupBuilder("/path/to/0036", compression=2)
///     builder.add_file("textures", "icon.dds", raw_bytes)
///     builder.add_file_from_path("models", "mesh.obj", "/path/to/mesh.obj")
///     pamt_bytes = builder.finish()  # writes .paz + 0.pamt to output_dir
#[pyclass(name = "PackGroupBuilder")]
pub struct PyPackGroupBuilder {
    inner: Option<crate::binary::paz::PackGroupBuilder>,
}

#[pymethods]
impl PyPackGroupBuilder {
    #[new]
    #[pyo3(signature = (output_dir, compression=2, crypto=0, encrypt_info=vec![0,0,0], max_chunk_size=500_000_000))]
    fn new(
        output_dir: &str,
        compression: u8,
        crypto: u8,
        encrypt_info: Vec<u8>,
        max_chunk_size: u64,
    ) -> PyResult<Self> {
        let comp = parse_compression(compression)?;
        let cry = parse_crypto(crypto)?;
        let ei: [u8; 3] = encrypt_info
            .try_into()
            .map_err(|_| PyValueError::new_err("encrypt_info must be 3 bytes"))?;

        // Create output directory if it doesn't exist
        std::fs::create_dir_all(output_dir).map_err(|e| PyIOError::new_err(e.to_string()))?;

        let builder = crate::binary::paz::PackGroupBuilder::new(
            std::path::Path::new(output_dir),
            comp,
            cry,
            ei,
            max_chunk_size,
        );

        Ok(PyPackGroupBuilder {
            inner: Some(builder),
        })
    }

    /// Add a file from raw bytes.
    fn add_file(&mut self, dir_path: &str, file_name: &str, data: &[u8]) -> PyResult<()> {
        let builder = self
            .inner
            .as_mut()
            .ok_or_else(|| PyValueError::new_err("builder already finished"))?;
        builder
            .add_file(dir_path, file_name, data)
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Add a file by reading from a path on disk.
    fn add_file_from_path(
        &mut self,
        dir_path: &str,
        file_name: &str,
        file_path: &str,
    ) -> PyResult<()> {
        let builder = self
            .inner
            .as_mut()
            .ok_or_else(|| PyValueError::new_err("builder already finished"))?;
        builder
            .add_file_from_path(dir_path, file_name, std::path::Path::new(file_path))
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    /// Finish building: flush remaining chunk, write 0.pamt.
    /// Returns the raw PAMT bytes (for computing checksum for PAPGT).
    fn finish(&mut self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let builder = self
            .inner
            .take()
            .ok_or_else(|| PyValueError::new_err("builder already finished"))?;
        let pamt_bytes = builder
            .finish()
            .map_err(|e| PyIOError::new_err(e.to_string()))?;
        Ok(PyBytes::new(py, &pamt_bytes).into_any().unbind())
    }
}

/// Add a new entry to a PAPGT dict.
///
/// Parses the PAPGT from the dict, adds the entry, re-serializes,
/// and returns the updated PAPGT as a new dict.
#[pyfunction]
pub fn add_papgt_entry(
    py: Python<'_>,
    papgt_data: &Bound<'_, PyDict>,
    group_name: &str,
    pack_meta_checksum: u32,
    is_optional: u8,
    language: u16,
) -> PyResult<Py<PyAny>> {
    // Reconstruct the PackGroupTreeMeta from the dict
    let bytes = wr_papgt_from_dict(papgt_data)?;
    let mut papgt =
        PackGroupTreeMeta::parse(&bytes).map_err(|e| PyValueError::new_err(e.to_string()))?;

    papgt.add_entry(group_name, pack_meta_checksum, is_optional, language);

    let new_bytes = papgt
        .to_bytes()
        .map_err(|e| PyIOError::new_err(e.to_string()))?;

    // Re-parse to get the dict representation
    let new_papgt =
        PackGroupTreeMeta::parse(&new_bytes).map_err(|e| PyValueError::new_err(e.to_string()))?;

    Ok(to_py_papgt(py, &new_papgt)?.into_any().unbind())
}

// ── File Extraction ───────────────────────────────────────────────────────

/// Extract a single file from a pack group archive to bytes.
///
/// Given a game directory, group name, directory path, and file name,
/// finds the file in the PAMT index and reads/decrypts/decompresses it.
#[pyfunction]
pub fn extract_file(
    py: Python<'_>,
    game_dir: &str,
    group_name: &str,
    dir_path: &str,
    file_name: &str,
) -> PyResult<Py<PyAny>> {
    use crate::binary::paz;
    use std::path::Path;

    let group_dir = Path::new(game_dir).join(group_name);
    let pamt_path = group_dir.join("0.pamt");

    let pamt_data = std::fs::read(&pamt_path)
        .map_err(|e| PyIOError::new_err(format!("{}: {}", pamt_path.display(), e)))?;
    let pamt =
        PackMeta::parse(&pamt_data, None).map_err(|e| PyValueError::new_err(e.to_string()))?;

    // Find the directory and file
    let dir = pamt
        .directories
        .iter()
        .find(|d| d.path == dir_path)
        .ok_or_else(|| {
            PyValueError::new_err(format!(
                "directory '{}' not found in {}/{}",
                dir_path, group_name, "0.pamt"
            ))
        })?;

    let file = dir
        .files
        .iter()
        .find(|f| f.name == file_name)
        .ok_or_else(|| {
            PyValueError::new_err(format!(
                "file '{}' not found in directory '{}'",
                file_name, dir_path
            ))
        })?;

    let encrypt_info = pamt.header.encrypt_info.encrypt_info;
    let raw = paz::extract_file(&group_dir, file, dir_path, &encrypt_info)
        .map_err(|e| PyIOError::new_err(e.to_string()))?;

    Ok(PyBytes::new(py, &raw).into_any().unbind())
}

/// Extract a single file by pointing at any `.paz` file in the group dir.
///
/// `paz_path` may name any `.paz` in the group directory (typically `0.paz`);
/// only its parent directory is used, to locate the sibling `0.pamt` and the
/// chunk file the PAMT routes to. `vfs_path` is the full VFS path inside the
/// archive (e.g. `gamedata/binary__/client/bin/iteminfo.pabgb`); it is split
/// on the last `/` into a directory and file name, and root files are accepted.
#[pyfunction]
pub fn extract_file_from_paz(
    py: Python<'_>,
    paz_path: &str,
    vfs_path: &str,
) -> PyResult<Py<PyAny>> {
    use crate::binary::paz;
    use std::path::Path;

    let paz = Path::new(paz_path);
    let group_dir = paz.parent().ok_or_else(|| {
        PyValueError::new_err(format!("paz_path has no parent directory: {}", paz_path))
    })?;
    let pamt_path = group_dir.join("0.pamt");

    let pamt_data = std::fs::read(&pamt_path)
        .map_err(|e| PyIOError::new_err(format!("{}: {}", pamt_path.display(), e)))?;
    let pamt =
        PackMeta::parse(&pamt_data, None).map_err(|e| PyValueError::new_err(e.to_string()))?;

    let (dir_path, file_name) = match vfs_path.rsplit_once('/') {
        Some((d, f)) => (d, f),
        None => ("", vfs_path),
    };

    let dir = pamt
        .directories
        .iter()
        .find(|d| d.path == dir_path)
        .ok_or_else(|| {
            PyValueError::new_err(format!(
                "directory '{}' not found in {}",
                dir_path,
                pamt_path.display()
            ))
        })?;

    let file = dir
        .files
        .iter()
        .find(|f| f.name == file_name)
        .ok_or_else(|| {
            PyValueError::new_err(format!(
                "file '{}' not found in directory '{}'",
                file_name, dir_path
            ))
        })?;

    let encrypt_info = pamt.header.encrypt_info.encrypt_info;
    let raw = paz::extract_file(group_dir, file, dir_path, &encrypt_info)
        .map_err(|e| PyIOError::new_err(e.to_string()))?;

    Ok(PyBytes::new(py, &raw).into_any().unbind())
}

// ── SkillInfo (skill.pabgb + skill.pabgh) ─────────────────────────────────

fn skill_format_to_str(f: SkillFormat) -> &'static str {
    match f {
        SkillFormat::WithField58 => "with_field_58",
        SkillFormat::NoField58 => "no_field_58",
    }
}

fn skill_format_from_str(s: &str) -> PyResult<SkillFormat> {
    match s {
        "with_field_58" => Ok(SkillFormat::WithField58),
        "no_field_58" => Ok(SkillFormat::NoField58),
        other => Err(PyValueError::new_err(format!(
            "unknown skill format '{}': expected 'with_field_58' or 'no_field_58'",
            other
        ))),
    }
}

fn graph_to_py<'py>(py: Python<'py>, g: &Graph) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("val0", g.val0)?;
    d.set_item("val1", g.val1)?;
    d.set_item("val2", g.val2)?;
    d.set_item("val3", g.val3)?;
    Ok(d)
}

fn graph_from_py(d: &Bound<'_, PyDict>) -> PyResult<Graph> {
    Ok(Graph {
        val0: get(d, "val0")?,
        val1: get(d, "val1")?,
        val2: get(d, "val2")?,
        val3: get(d, "val3")?,
    })
}

fn resource_stat_to_py<'py>(
    py: Python<'py>,
    rs: &ResourceStat,
) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("stat_type", rs.stat_type)?;
    d.set_item("stat_hash", rs.stat_hash)?;
    d.set_item("flag", rs.flag)?;
    d.set_item("value", rs.value)?;
    d.set_item("hash2", rs.hash2)?;
    d.set_item("hash3", rs.hash3)?;
    Ok(d)
}

fn resource_stat_from_py(d: &Bound<'_, PyDict>) -> PyResult<ResourceStat> {
    Ok(ResourceStat {
        stat_type: get(d, "stat_type")?,
        stat_hash: get(d, "stat_hash")?,
        flag: get(d, "flag")?,
        value: get(d, "value")?,
        hash2: get(d, "hash2")?,
        hash3: get(d, "hash3")?,
    })
}

fn resource_item_to_py<'py>(
    py: Python<'py>,
    ri: &ResourceItem,
) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("item_hash", ri.item_hash)?;
    d.set_item("count", ri.count)?;
    Ok(d)
}

fn resource_item_from_py(d: &Bound<'_, PyDict>) -> PyResult<ResourceItem> {
    Ok(ResourceItem {
        item_hash: get(d, "item_hash")?,
        count: get(d, "count")?,
    })
}

fn post_buff_to_py<'py>(py: Python<'py>, p: &PostBuff) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("skill_group_key", p.skill_group_key)?;
    d.set_item("parent_skill", p.parent_skill)?;
    d.set_item("learn_level", p.learn_level)?;
    d.set_item("apply_type", p.apply_type)?;
    d.set_item("icon_path", p.icon_path)?;
    d.set_item("need_upgrade_item_info", p.need_upgrade_item_info)?;
    d.set_item("need_upgrade_item_count_graph", graph_to_py(py, &p.need_upgrade_item_count_graph)?)?;
    d.set_item("need_upgrade_experience_graph", graph_to_py(py, &p.need_upgrade_experience_graph)?)?;
    d.set_item("usable_character_info_list", p.usable_character_info_list.clone())?;
    d.set_item("usable_condition", p.usable_condition.clone())?;
    d.set_item("learn_knowledge_info", p.learn_knowledge_info)?;
    d.set_item("faction_info", p.faction_info)?;
    let rsl = PyList::empty(py);
    for rs in &p.use_resource_stat_list {
        rsl.append(resource_stat_to_py(py, rs)?)?;
    }
    d.set_item("use_resource_stat_list", rsl)?;
    let ril = PyList::empty(py);
    for ri in &p.use_resource_item_list {
        ril.append(resource_item_to_py(py, ri)?)?;
    }
    d.set_item("use_resource_item_list", ril)?;
    let drsl = PyList::empty(py);
    for rs in &p.use_driver_resource_stat_list {
        drsl.append(resource_stat_to_py(py, rs)?)?;
    }
    d.set_item("use_driver_resource_stat_list", drsl)?;
    d.set_item("use_battery_stat", p.use_battery_stat)?;
    d.set_item("is_ui_use_allowed", p.is_ui_use_allowed)?;
    d.set_item("is_learn_use_artifact", p.is_learn_use_artifact)?;
    d.set_item("allow_skill_with_low_resource", p.allow_skill_with_low_resource)?;
    d.set_item(
        "is_use_child_pattern_description_buff_data",
        p.is_use_child_pattern_description_buff_data,
    )?;
    d.set_item("unk_pre_damage_type", p.unk_pre_damage_type)?;
    d.set_item("damage_type", p.damage_type)?;
    d.set_item("ui_type", p.ui_type)?;
    d.set_item("reserve_slot_info_list", p.reserve_slot_info_list.clone())?;
    d.set_item("max_level", p.max_level)?;
    d.set_item("skill_group_key_list", p.skill_group_key_list.clone())?;
    d.set_item("buff_sustain_flag", p.buff_sustain_flag)?;
    d.set_item("dev_skill_name", PyBytes::new(py, &p.dev_skill_name))?;
    d.set_item("dev_skill_desc", PyBytes::new(py, &p.dev_skill_desc))?;
    d.set_item("video_path", p.video_path)?;
    Ok(d)
}

fn post_buff_from_py(d: &Bound<'_, PyDict>) -> PyResult<PostBuff> {
    let mut rsl = Vec::new();
    for it in get_obj(d, "use_resource_stat_list")?.cast::<PyList>()?.iter() {
        rsl.push(resource_stat_from_py(it.cast::<PyDict>()?)?);
    }
    let mut ril = Vec::new();
    for it in get_obj(d, "use_resource_item_list")?.cast::<PyList>()?.iter() {
        ril.push(resource_item_from_py(it.cast::<PyDict>()?)?);
    }
    let mut drsl = Vec::new();
    for it in get_obj(d, "use_driver_resource_stat_list")?.cast::<PyList>()?.iter() {
        drsl.push(resource_stat_from_py(it.cast::<PyDict>()?)?);
    }
    Ok(PostBuff {
        skill_group_key: get(d, "skill_group_key")?,
        parent_skill: get(d, "parent_skill")?,
        learn_level: get(d, "learn_level")?,
        apply_type: get(d, "apply_type")?,
        icon_path: get(d, "icon_path")?,
        need_upgrade_item_info: get(d, "need_upgrade_item_info")?,
        need_upgrade_item_count_graph: graph_from_py(
            get_obj(d, "need_upgrade_item_count_graph")?.cast::<PyDict>()?,
        )?,
        need_upgrade_experience_graph: graph_from_py(
            get_obj(d, "need_upgrade_experience_graph")?.cast::<PyDict>()?,
        )?,
        usable_character_info_list: get(d, "usable_character_info_list")?,
        usable_condition: get(d, "usable_condition")?,
        learn_knowledge_info: get(d, "learn_knowledge_info")?,
        faction_info: get(d, "faction_info")?,
        use_resource_stat_list: rsl,
        use_resource_item_list: ril,
        use_driver_resource_stat_list: drsl,
        use_battery_stat: get(d, "use_battery_stat")?,
        is_ui_use_allowed: get(d, "is_ui_use_allowed")?,
        is_learn_use_artifact: get(d, "is_learn_use_artifact")?,
        allow_skill_with_low_resource: get(d, "allow_skill_with_low_resource")?,
        is_use_child_pattern_description_buff_data: get(
            d,
            "is_use_child_pattern_description_buff_data",
        )?,
        unk_pre_damage_type: get(d, "unk_pre_damage_type")?,
        damage_type: get(d, "damage_type")?,
        ui_type: get(d, "ui_type")?,
        reserve_slot_info_list: get(d, "reserve_slot_info_list")?,
        max_level: get(d, "max_level")?,
        skill_group_key_list: get(d, "skill_group_key_list")?,
        buff_sustain_flag: get(d, "buff_sustain_flag")?,
        dev_skill_name: get(d, "dev_skill_name")?,
        dev_skill_desc: get(d, "dev_skill_desc")?,
        video_path: get(d, "video_path")?,
    })
}

fn buff_data_body_to_py<'py>(
    py: Python<'py>,
    body: &BuffDataBody,
) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("type_id", body.type_id)?;
    d.set_item("field_12", body.field_12)?;
    d.set_item("field_16", body.field_16)?;
    d.set_item("field_20", body.field_20)?;
    d.set_item("field_21", body.field_21)?;
    d.set_item("field_24", body.field_24)?;
    d.set_item("field_32", body.field_32)?;
    d.set_item("field_40", body.field_40)?;
    d.set_item("field_48", PyBytes::new(py, &body.field_48))?;
    d.set_item("field_56", body.field_56)?;
    d.set_item("field_58", body.field_58)?;
    d.set_item("field_60", body.field_60)?;
    d.set_item("field_62", body.field_62)?;
    d.set_item("field_64", body.field_64)?;
    d.set_item("field_66", body.field_66)?;
    d.set_item("field_68", body.field_68)?;
    d.set_item("field_69", body.field_69)?;
    d.set_item("field_88", body.field_88)?;
    d.set_item("field_90", body.field_90)?;
    d.set_item("field_96_list", body.field_96_list.clone())?;
    d.set_item("field_128", body.field_128)?;
    d.set_item("field_72", body.field_72)?;
    d.set_item("field_76", body.field_76)?;
    d.set_item("field_80", body.field_80)?;
    d.set_item("field_84", body.field_84)?;
    d.set_item("field_112_list", body.field_112_list.clone())?;
    d.set_item("field_132", body.field_132)?;
    d.set_item("field_136", body.field_136)?;
    d.set_item("subclass_tail", PyBytes::new(py, &body.subclass_tail))?;
    Ok(d)
}

fn buff_data_body_from_py(d: &Bound<'_, PyDict>) -> PyResult<BuffDataBody> {
    Ok(BuffDataBody {
        type_id: get(d, "type_id")?,
        field_12: get(d, "field_12")?,
        field_16: get(d, "field_16")?,
        field_20: get(d, "field_20")?,
        field_21: get(d, "field_21")?,
        field_24: get(d, "field_24")?,
        field_32: get(d, "field_32")?,
        field_40: get(d, "field_40")?,
        field_48: get(d, "field_48")?,
        field_56: get(d, "field_56")?,
        field_58: match d.get_item("field_58")? {
            Some(v) if !v.is_none() => Some(v.extract()?),
            _ => None,
        },
        field_60: get(d, "field_60")?,
        field_62: get(d, "field_62")?,
        field_64: get(d, "field_64")?,
        field_66: get(d, "field_66")?,
        field_68: get(d, "field_68")?,
        field_69: get(d, "field_69")?,
        field_88: get(d, "field_88")?,
        field_90: get(d, "field_90")?,
        field_96_list: get(d, "field_96_list")?,
        field_128: get(d, "field_128")?,
        field_72: get(d, "field_72")?,
        field_76: get(d, "field_76")?,
        field_80: get(d, "field_80")?,
        field_84: get(d, "field_84")?,
        field_112_list: get(d, "field_112_list")?,
        field_132: get(d, "field_132")?,
        field_136: get(d, "field_136")?,
        subclass_tail: get(d, "subclass_tail")?,
    })
}

fn buff_data_to_py<'py>(py: Python<'py>, bd: &BuffData) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("flag", bd.flag)?;
    match &bd.body {
        Some(b) => d.set_item("body", buff_data_body_to_py(py, b)?)?,
        None => d.set_item("body", py.None())?,
    }
    Ok(d)
}

fn buff_data_from_py(d: &Bound<'_, PyDict>) -> PyResult<BuffData> {
    let flag: u8 = get(d, "flag")?;
    let body = match d.get_item("body")? {
        Some(v) if !v.is_none() => Some(buff_data_body_from_py(v.cast::<PyDict>()?)?),
        _ => None,
    };
    Ok(BuffData { flag, body })
}

fn skill_entry_to_py<'py>(py: Python<'py>, e: &SkillEntry) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("key", e.key)?;
    d.set_item("name_bytes", PyBytes::new(py, &e.name_bytes))?;
    d.set_item("name", String::from_utf8_lossy(&e.name_bytes).into_owned())?;
    d.set_item("is_blocked", e.is_blocked)?;
    d.set_item("pad_01", PyBytes::new(py, &e.pad_01))?;
    match &e.buff_level_list {
        Some(levels) => {
            let py_levels = PyList::empty(py);
            for level in levels {
                let py_level = PyList::empty(py);
                for bd in level {
                    py_level.append(buff_data_to_py(py, bd)?)?;
                }
                py_levels.append(py_level)?;
            }
            d.set_item("buff_level_list", py_levels)?;
        }
        None => d.set_item("buff_level_list", py.None())?,
    }
    match &e.buff_raw_fallback {
        Some(b) => d.set_item("buff_raw_fallback", PyBytes::new(py, b))?,
        None => d.set_item("buff_raw_fallback", py.None())?,
    }
    d.set_item("post_buff", post_buff_to_py(py, &e.post_buff)?)?;
    Ok(d)
}

fn skill_entry_from_py(d: &Bound<'_, PyDict>) -> PyResult<SkillEntry> {
    let pad_bytes: Vec<u8> = get(d, "pad_01")?;
    if pad_bytes.len() != 3 {
        return Err(PyValueError::new_err(format!(
            "pad_01 must be 3 bytes, got {}",
            pad_bytes.len()
        )));
    }
    let pad_01 = [pad_bytes[0], pad_bytes[1], pad_bytes[2]];

    let buff_level_list = match d.get_item("buff_level_list")? {
        Some(v) if !v.is_none() => {
            let mut levels = Vec::new();
            for level_obj in v.cast::<PyList>()?.iter() {
                let mut level = Vec::new();
                for bd_obj in level_obj.cast::<PyList>()?.iter() {
                    level.push(buff_data_from_py(bd_obj.cast::<PyDict>()?)?);
                }
                levels.push(level);
            }
            Some(levels)
        }
        _ => None,
    };
    let buff_raw_fallback = match d.get_item("buff_raw_fallback")? {
        Some(v) if !v.is_none() => Some(v.extract::<Vec<u8>>()?),
        _ => None,
    };

    Ok(SkillEntry {
        key: get(d, "key")?,
        name_bytes: get(d, "name_bytes")?,
        is_blocked: get(d, "is_blocked")?,
        pad_01,
        buff_level_list,
        buff_raw_fallback,
        post_buff: post_buff_from_py(get_obj(d, "post_buff")?.cast::<PyDict>()?)?,
    })
}

/// Parse `skill.pabgb` + `skill.pabgh` into a list of skill entries plus
/// the detected format flag and on-disk index order. The dict shape is
/// stable for `serialize_skillinfo`: pass it back unmodified for a
/// byte-identical roundtrip.
#[pyfunction]
pub fn parse_skillinfo_from_bytes(
    py: Python<'_>,
    skill_pabgb: &[u8],
    skill_pabgh: &[u8],
) -> PyResult<Py<PyAny>> {
    let parsed = SkillData::parse(skill_pabgh, skill_pabgb)
        .map_err(|e| PyValueError::new_err(e.to_string()))?;

    let entries = PyList::empty(py);
    for e in &parsed.entries {
        entries.append(skill_entry_to_py(py, e)?)?;
    }

    let index_order = PyList::empty(py);
    for ie in &parsed.index_order {
        let id = PyDict::new(py);
        id.set_item("key", ie.key)?;
        id.set_item("offset", ie.offset)?;
        index_order.append(id)?;
    }

    let result = PyDict::new(py);
    result.set_item("entries", entries)?;
    result.set_item("format", skill_format_to_str(parsed.format))?;
    result.set_item("index_order", index_order)?;
    Ok(result.into_any().unbind())
}

/// Serialise a parsed dict back to `(pabgh_bytes, pabgb_bytes)`. Pass the
/// dict returned by `parse_skillinfo_from_bytes` (optionally with edits)
/// — the on-disk PABGH order is taken from `index_order`.
#[pyfunction]
pub fn serialize_skillinfo(
    py: Python<'_>,
    data: &Bound<'_, PyDict>,
) -> PyResult<Py<PyAny>> {
    let format = skill_format_from_str(&get::<String>(data, "format")?)?;
    let entries_list = get_obj(data, "entries")?.cast::<PyList>()?.clone();
    let index_list = get_obj(data, "index_order")?.cast::<PyList>()?.clone();

    let mut entries = Vec::with_capacity(entries_list.len());
    for it in entries_list.iter() {
        entries.push(skill_entry_from_py(it.cast::<PyDict>()?)?);
    }
    let mut index_order = Vec::with_capacity(index_list.len());
    for it in index_list.iter() {
        let d = it.cast::<PyDict>()?;
        index_order.push(SkillIndexEntry {
            key: get(d, "key")?,
            offset: get(d, "offset")?,
        });
    }
    let sd = SkillData {
        entries,
        format,
        index_order,
    };
    let (pabgh, pabgb) = sd.write().map_err(|e| PyIOError::new_err(e.to_string()))?;
    let tup = pyo3::types::PyTuple::new(
        py,
        [
            PyBytes::new(py, &pabgh).into_any(),
            PyBytes::new(py, &pabgb).into_any(),
        ],
    )?;
    Ok(tup.into_any().unbind())
}

// ── Save file ──────────────────────────────────────────────────────────────

/// Parse a save file from its raw bytes.
///
/// Returns a dict with:
///   - `header`         : bytes (128)
///   - `version`        : int
///   - `flags`          : int
///   - `payload_size`   : int (compressed+encrypted body length on disk)
///   - `uncompressed_size`: int (decompressed body length)
///   - `nonce`          : bytes (16)
///   - `hmac`           : bytes (32, the stored tag from the header)
///   - `hmac_ok`        : bool (True iff the stored tag verifies)
///   - `body`           : bytes (the decompressed payload)
#[pyfunction]
pub fn parse_save_from_bytes(py: Python<'_>, data: &[u8]) -> PyResult<Py<PyAny>> {
    let save = crate::save::Save::parse(data)
        .map_err(|e| PyValueError::new_err(format!("parse_save: {e}")))?;
    save_to_py_dict(py, &save)
}

/// Parse a save file from a filesystem path.
#[pyfunction]
pub fn parse_save_from_file(py: Python<'_>, path: &str) -> PyResult<Py<PyAny>> {
    let data = std::fs::read(path).map_err(|e| PyIOError::new_err(e.to_string()))?;
    parse_save_from_bytes(py, &data)
}

/// Serialize a save with a caller-supplied nonce.
///
/// Inputs:
///   - `header`: 128-byte header bytes (typically the dict's `header` from a
///     prior `parse_save_*` call — preserves magic/version/flags/padding)
///   - `body`  : decompressed payload bytes (possibly edited)
///   - `nonce` : 16-byte ChaCha20 nonce. Pass the original nonce for a
///     decode-stable round trip; pass random bytes for a fresh write.
///
/// Returns the encoded file bytes.
#[pyfunction]
pub fn write_save_with_nonce(
    py: Python<'_>,
    header: &[u8],
    body: &[u8],
    nonce: &[u8],
) -> PyResult<Py<PyAny>> {
    use crate::save::{HEADER_SIZE, Save, SaveHeader};

    if header.len() != HEADER_SIZE {
        return Err(PyValueError::new_err(format!(
            "header must be {HEADER_SIZE} bytes, got {}",
            header.len()
        )));
    }
    if nonce.len() != 16 {
        return Err(PyValueError::new_err(format!(
            "nonce must be 16 bytes, got {}",
            nonce.len()
        )));
    }
    let mut hdr_arr = [0u8; HEADER_SIZE];
    hdr_arr.copy_from_slice(header);
    let parsed_header = SaveHeader::from_bytes(hdr_arr).map_err(|e| {
        PyValueError::new_err(format!("invalid header: {e}"))
    })?;
    let mut nonce_arr = [0u8; 16];
    nonce_arr.copy_from_slice(nonce);

    let save = Save {
        header: parsed_header,
        body: body.to_vec(),
        hmac_ok: true,
    };
    let bytes = save
        .write_with_nonce(nonce_arr)
        .map_err(|e| PyValueError::new_err(format!("write_save: {e}")))?;
    Ok(PyBytes::new(py, &bytes).into_any().unbind())
}

/// Parse the decompressed save body and return its schema + TOC as a dict.
///
/// Input `body` is the bytes produced by `parse_save_*`'s `body` field.
/// Returns:
///   - `prefix`: bytes (14, magic + 10 unknown header bytes)
///   - `schema`: dict with `header_tag, header_zero, type_count, root_type,
///     schema_end, types[]` where each type has
///     `index, name, start_offset, end_offset, fields[]`.
///   - `toc`: dict with `prefix_zero, toc_count, stream_size, entries[]`.
///
/// Per-object decoding (turning a TOC entry's data slice into items, stats,
/// quests, etc.) is the next layer and is not done here.
#[pyfunction]
pub fn parse_save_body_from_bytes(py: Python<'_>, body: &[u8]) -> PyResult<Py<PyAny>> {
    let parsed = crate::save::Body::parse(body)
        .map_err(|e| PyValueError::new_err(format!("parse_save_body: {e}")))?;
    body_to_py_dict(py, &parsed)
}

/// Decode every TOC entry of a save body into typed [`ObjectBlock`]s.
///
/// Input `body` is the bytes produced by `parse_save_*`'s `body` field.
/// Returns:
///   - `blocks`: list of dicts, one per decoded TOC entry. Each dict:
///     `class_index, class_name, data_offset, data_size, mask_byte_count,
///      mask_bytes, reserved_u32, fields[], undecoded_ranges[]`.
///   - `stats`: dict with `block_count, present_fields, decoded_fields,
///      total_block_bytes, undecoded_bytes`.
///
/// Each field dict carries `field_index, name, type_name, meta_kind,
/// meta_size, meta_aux, present, kind, start, end, note` plus
/// kind-specific keys:
///   - kind `fixed_prefix` / `fixed_suffix` → `value` (typed int/float/bool/None),
///     `value_type` (e.g. `"u32"`, `"f32"`, `"bool"`, `"bytes"`)
///   - kind `inline_bytes` / `dynamic_array` → `count`, `bytes` (raw),
///     plus `header_variant` for dynamic_array
///   - kind `object_locator` → `child_type_index`, `child_type_name`,
///     `child_payload_offset`, optional `child` (nested block dict)
///   - kind `object_list` → `count`, `header_variant`,
///     `elements` (list of nested block dicts)
#[pyfunction]
pub fn decode_save_body_blocks(py: Python<'_>, body: &[u8]) -> PyResult<Py<PyAny>> {
    let parsed = crate::save::Body::parse(body)
        .map_err(|e| PyValueError::new_err(format!("decode_save_body_blocks: {e}")))?;
    let blocks = parsed.decode_blocks(body);

    let out = PyDict::new(py);
    let block_list = PyList::empty(py);

    let mut total_block_bytes: usize = 0;
    let mut total_undecoded: usize = 0;
    let mut present_fields: usize = 0;
    let mut decoded_fields: usize = 0;

    for block in &blocks {
        total_block_bytes += block.data_size as usize;
        for (s, e) in &block.undecoded_ranges {
            total_undecoded += e - s;
        }
        for f in &block.fields {
            if f.present {
                present_fields += 1;
                match f.kind {
                    crate::save::FieldKind::Unknown | crate::save::FieldKind::Absent => {}
                    _ => decoded_fields += 1,
                }
            }
        }
        block_list.append(block_to_py(py, block)?)?;
    }
    out.set_item("blocks", block_list)?;

    let stats = PyDict::new(py);
    stats.set_item("block_count", blocks.len())?;
    stats.set_item("present_fields", present_fields)?;
    stats.set_item("decoded_fields", decoded_fields)?;
    stats.set_item("total_block_bytes", total_block_bytes)?;
    stats.set_item("undecoded_bytes", total_undecoded)?;
    out.set_item("stats", stats)?;

    Ok(out.into_any().unbind())
}

fn block_to_py<'py>(py: Python<'py>, block: &crate::save::ObjectBlock) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("class_index", block.class_index)?;
    d.set_item("class_name", &block.class_name)?;
    d.set_item("data_offset", block.data_offset)?;
    d.set_item("data_size", block.data_size)?;
    d.set_item("mask_byte_count", block.mask_byte_count)?;
    d.set_item("mask_bytes", PyBytes::new(py, &block.mask_bytes))?;
    d.set_item("reserved_u32", block.reserved_u32)?;
    if !block.trailing_pad.is_empty() {
        d.set_item("trailing_pad", PyBytes::new(py, &block.trailing_pad))?;
    }

    let fields = PyList::empty(py);
    for f in &block.fields {
        fields.append(field_to_py(py, f)?)?;
    }
    d.set_item("fields", fields)?;

    let undec = PyList::empty(py);
    for (s, e) in &block.undecoded_ranges {
        undec.append(pyo3::types::PyTuple::new(py, [*s, *e])?)?;
    }
    d.set_item("undecoded_ranges", undec)?;
    Ok(d)
}

fn field_to_py<'py>(py: Python<'py>, field: &crate::save::DecodedField) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("field_index", field.field_index)?;
    d.set_item("name", &field.name)?;
    d.set_item("type_name", &field.type_name)?;
    d.set_item("meta_kind", field.meta_kind)?;
    d.set_item("meta_size", field.meta_size)?;
    d.set_item("meta_aux", field.meta_aux)?;
    d.set_item("present", field.present)?;
    d.set_item("kind", field.kind.as_str())?;
    d.set_item("start", field.start)?;
    d.set_item("end", field.end)?;
    if !field.note.is_empty() {
        d.set_item("note", &field.note)?;
    }
    fill_field_value(py, &d, &field.value)?;
    Ok(d)
}

fn fill_field_value<'py>(
    py: Python<'py>,
    d: &Bound<'py, PyDict>,
    value: &crate::save::FieldValue,
) -> PyResult<()> {
    use crate::save::{FieldValue, ScalarValue};
    match value {
        FieldValue::None => Ok(()),
        FieldValue::Scalar(s) => {
            let (val_obj, val_type): (Py<PyAny>, &'static str) = match s {
                ScalarValue::Bool(b) => (
                    // Expose truthiness as a Python bool (the raw byte 0x01/0xff
                    // is an encoder round-trip detail). `bool::into_pyobject`
                    // returns a `Borrowed` (PyBool is a singleton); take
                    // ownership before chaining `into_any`.
                    (*b != 0).into_pyobject(py)?.to_owned().into_any().unbind(),
                    "bool",
                ),
                ScalarValue::U8(x) => (x.into_pyobject(py)?.into_any().unbind(), "u8"),
                ScalarValue::U16(x) => (x.into_pyobject(py)?.into_any().unbind(), "u16"),
                ScalarValue::U32(x) => (x.into_pyobject(py)?.into_any().unbind(), "u32"),
                ScalarValue::U64(x) => (x.into_pyobject(py)?.into_any().unbind(), "u64"),
                ScalarValue::I8(x) => (x.into_pyobject(py)?.into_any().unbind(), "i8"),
                ScalarValue::I16(x) => (x.into_pyobject(py)?.into_any().unbind(), "i16"),
                ScalarValue::I32(x) => (x.into_pyobject(py)?.into_any().unbind(), "i32"),
                ScalarValue::I64(x) => (x.into_pyobject(py)?.into_any().unbind(), "i64"),
                ScalarValue::F32(x) => (x.into_pyobject(py)?.into_any().unbind(), "f32"),
                ScalarValue::F64(x) => (x.into_pyobject(py)?.into_any().unbind(), "f64"),
                ScalarValue::F32x3(xs) => (
                    pyo3::types::PyList::new(py, xs)?.into_any().unbind(),
                    "f32x3",
                ),
                ScalarValue::F32x4(xs) => (
                    pyo3::types::PyList::new(py, xs)?.into_any().unbind(),
                    "f32x4",
                ),
                ScalarValue::U32x4(xs) => (
                    pyo3::types::PyList::new(py, xs)?.into_any().unbind(),
                    "u32x4",
                ),
                ScalarValue::Bytes(b) => (PyBytes::new(py, b).into_any().unbind(), "bytes"),
            };
            d.set_item("value", val_obj)?;
            d.set_item("value_type", val_type)?;
            Ok(())
        }
        FieldValue::InlineBytes { count, bytes } => {
            d.set_item("count", *count)?;
            d.set_item("bytes", PyBytes::new(py, bytes))?;
            Ok(())
        }
        FieldValue::DynamicArray { count, bytes, header_variant, .. } => {
            d.set_item("count", *count)?;
            d.set_item("bytes", PyBytes::new(py, bytes))?;
            d.set_item("header_variant", *header_variant)?;
            Ok(())
        }
        FieldValue::Locator {
            child_type_index,
            child_type_name,
            child_payload_offset,
            child,
            ..
        } => {
            d.set_item("child_type_index", *child_type_index)?;
            d.set_item("child_type_name", child_type_name)?;
            d.set_item("child_payload_offset", *child_payload_offset)?;
            if let Some(c) = child {
                d.set_item("child", block_to_py(py, c)?)?;
            }
            Ok(())
        }
        FieldValue::ObjectList { count, header_variant, elements, .. } => {
            d.set_item("count", *count)?;
            d.set_item("header_variant", *header_variant)?;
            let list = PyList::empty(py);
            for el in elements {
                list.append(block_to_py(py, el)?)?;
            }
            d.set_item("elements", list)?;
            Ok(())
        }
    }
}

fn body_to_py_dict<'py>(py: Python<'py>, body: &crate::save::Body) -> PyResult<Py<PyAny>> {
    let d = PyDict::new(py);
    d.set_item("prefix", PyBytes::new(py, &body.prefix))?;

    let schema = PyDict::new(py);
    schema.set_item("header_tag", body.schema.header_tag)?;
    schema.set_item("header_zero", body.schema.header_zero)?;
    schema.set_item("type_count", body.schema.type_count)?;
    schema.set_item("root_type", &body.schema.root_type)?;
    schema.set_item("schema_end", body.schema.schema_end)?;
    let types_list = PyList::empty(py);
    for t in &body.schema.types {
        let td = PyDict::new(py);
        td.set_item("index", t.index)?;
        td.set_item("name", &t.name)?;
        td.set_item("start_offset", t.start_offset)?;
        td.set_item("end_offset", t.end_offset)?;
        let fields_list = PyList::empty(py);
        for f in &t.fields {
            let fd = PyDict::new(py);
            fd.set_item("name", &f.name)?;
            fd.set_item("type_name", &f.type_name)?;
            fd.set_item("meta_kind", f.meta_kind)?;
            fd.set_item("meta_size", f.meta_size)?;
            fd.set_item("meta_aux", f.meta_aux)?;
            fd.set_item("start_offset", f.start_offset)?;
            fd.set_item("end_offset", f.end_offset)?;
            fields_list.append(fd)?;
        }
        td.set_item("fields", fields_list)?;
        types_list.append(td)?;
    }
    schema.set_item("types", types_list)?;
    d.set_item("schema", schema)?;

    let toc = PyDict::new(py);
    toc.set_item("prefix_zero", body.toc.prefix_zero)?;
    toc.set_item("toc_count", body.toc.toc_count)?;
    toc.set_item("stream_size", body.toc.stream_size)?;
    let entries_list = PyList::empty(py);
    for e in &body.toc.entries {
        let ed = PyDict::new(py);
        ed.set_item("index", e.index)?;
        ed.set_item("class_index", e.class_index)?;
        let class_name = body
            .schema
            .types
            .get(e.class_index as usize)
            .map(|t| t.name.clone())
            .unwrap_or_else(|| format!("<class_{}>", e.class_index));
        ed.set_item("class_name", class_name)?;
        ed.set_item("sentinel1", e.sentinel1)?;
        ed.set_item("sentinel2", e.sentinel2)?;
        ed.set_item("data_offset", e.data_offset)?;
        ed.set_item("data_size", e.data_size)?;
        ed.set_item("entry_offset", e.entry_offset)?;
        entries_list.append(ed)?;
    }
    toc.set_item("entries", entries_list)?;
    d.set_item("toc", toc)?;

    Ok(d.into_any().unbind())
}

fn save_to_py_dict<'py>(py: Python<'py>, save: &crate::save::Save) -> PyResult<Py<PyAny>> {
    let d = PyDict::new(py);
    d.set_item("header", PyBytes::new(py, save.header.as_bytes()))?;
    d.set_item("version", save.header.version())?;
    d.set_item("flags", save.header.flags())?;
    d.set_item("payload_size", save.header.payload_size())?;
    d.set_item("uncompressed_size", save.header.uncompressed_size())?;
    d.set_item("nonce", PyBytes::new(py, &save.header.nonce()))?;
    d.set_item("hmac", PyBytes::new(py, &save.header.hmac()))?;
    d.set_item("hmac_ok", save.hmac_ok)?;
    d.set_item("body", PyBytes::new(py, &save.body))?;
    Ok(d.into_any().unbind())
}

// ── Registration ───────────────────────────────────────────────────────────

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(parse_iteminfo_from_file, m)?)?;
    m.add_function(wrap_pyfunction!(parse_iteminfo_from_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(parse_iteminfo_tracked, m)?)?;
    m.add_function(wrap_pyfunction!(parse_iteminfo_lossy, m)?)?;
    m.add_function(wrap_pyfunction!(inspect_legacy_patches, m)?)?;
    m.add_function(wrap_pyfunction!(write_iteminfo_to_file, m)?)?;
    m.add_function(wrap_pyfunction!(serialize_iteminfo, m)?)?;
    m.add_function(wrap_pyfunction!(parse_papgt_file, m)?)?;
    m.add_function(wrap_pyfunction!(parse_papgt_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(write_papgt_file, m)?)?;
    m.add_function(wrap_pyfunction!(serialize_papgt, m)?)?;
    m.add_function(wrap_pyfunction!(parse_pamt_file, m)?)?;
    m.add_function(wrap_pyfunction!(parse_pamt_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(write_pamt_file, m)?)?;
    m.add_function(wrap_pyfunction!(serialize_pamt, m)?)?;
    m.add_function(wrap_pyfunction!(calculate_checksum, m)?)?;
    m.add_function(wrap_pyfunction!(compress_data, m)?)?;
    m.add_function(wrap_pyfunction!(decompress_data, m)?)?;
    m.add_class::<PyPackGroupBuilder>()?;
    m.add_function(wrap_pyfunction!(add_papgt_entry, m)?)?;
    m.add_function(wrap_pyfunction!(extract_file, m)?)?;
    m.add_function(wrap_pyfunction!(extract_file_from_paz, m)?)?;
    m.add_function(wrap_pyfunction!(parse_skillinfo_from_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(serialize_skillinfo, m)?)?;
    m.add_function(wrap_pyfunction!(parse_paloc_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(serialize_paloc, m)?)?;
    m.add_function(wrap_pyfunction!(parse_save_from_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(parse_save_from_file, m)?)?;
    m.add_function(wrap_pyfunction!(write_save_with_nonce, m)?)?;
    m.add_function(wrap_pyfunction!(parse_save_body_from_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(decode_save_body_blocks, m)?)?;
    Ok(())
}
