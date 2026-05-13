use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::pamt::*;
use super::trie::build_trie_buffer;
use crate::crypto::chacha20;
use crate::crypto::checksum;

// ── Compression ───────────────────────────────────────────────────────────

pub fn compress(data: &[u8], compression: Compression) -> io::Result<Vec<u8>> {
    match compression {
        Compression::None => Ok(data.to_vec()),
        Compression::Lz4 => Ok(lz4_flex::block::compress(data)),
        Compression::Zlib => {
            let mut encoder =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(data)?;
            encoder.finish()
        }
        _ => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("compression {:?} not supported for creation", compression),
        )),
    }
}

pub fn decompress(
    data: &[u8],
    compression: Compression,
    uncompressed_size: usize,
) -> io::Result<Vec<u8>> {
    match compression {
        Compression::None => Ok(data.to_vec()),
        Compression::Lz4 => lz4_flex::block::decompress(data, uncompressed_size)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
        Compression::Zlib => {
            use std::io::Read;
            let mut decoder = flate2::read::ZlibDecoder::new(data);
            let mut out = Vec::with_capacity(uncompressed_size);
            decoder.read_to_end(&mut out)?;
            Ok(out)
        }
        _ => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("decompression {:?} not supported", compression),
        )),
    }
}

/// Partial-compression scheme used by Pearl Abyss PAZ archives when
/// `raw_compression == 1`. The on-disk payload is one of:
///
/// 1. **Identity** — when LZ4 yielded no gain, the engine stores the file
///    verbatim and sets `compressed_size == uncompressed_size`. The
///    bytes ARE the file; no decoder needed.
/// 2. **Header + LZ4(prefix dict)** — the first `PARTIAL_HEADER_BYTES`
///    (128) bytes are stored verbatim, then the remainder is one LZ4
///    block. The decoder uses those 128 bytes as a prefix dictionary
///    so back-references can reach into the header. Covers every file
///    under `0012/ui/texture/icon/` in 1.06 (every item icon).
/// 3. **DDS per-mip table** — for DDS textures the engine can encode
///    each mip level independently. The on-disk size of mip *i* is
///    stored as a u32 in the DDS reserved area at `0x20 + 4*i` (11
///    slots, mips 0..10). A non-zero slot smaller than that mip's raw
///    size means LZ4-compressed; equal means raw; `0` means "all
///    remaining mips are stored raw, sequentially". Covers the
///    worldmap SDF tiles and large diffuse textures the simpler rule
///    misses. Strategy + offsets reverse-engineered by NattKh in the
///    CrimsonForge modding tool — see `core/compression_engine.py`
///    `_decompress_type1_dds_per_mip_sizes`.
///
/// **Not yet handled**: the PAR-container layout used by `.pam` /
/// `.pamlod` / `.pac` mesh assets in 0009/0015 (per-section LZ4 blocks
/// indexed by an 8-slot table at offset 0x10). Those return an
/// `InvalidData` error here so the caller can distinguish them from
/// outright PAZ corruption.
const PARTIAL_HEADER_BYTES: usize = 128;

pub(crate) fn decompress_partial(
    decrypted: &[u8],
    uncompressed_size: usize,
) -> io::Result<Vec<u8>> {
    if decrypted.len() == uncompressed_size {
        // Identity case — the engine declined LZ4 because the file
        // doesn't compress (already-block-compressed BC formats, etc.).
        return Ok(decrypted.to_vec());
    }
    if decrypted.len() <= PARTIAL_HEADER_BYTES
        || uncompressed_size <= PARTIAL_HEADER_BYTES
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "partial PAZ entry too short for header({})+lz4: decrypted={} u_size={}",
                PARTIAL_HEADER_BYTES,
                decrypted.len(),
                uncompressed_size,
            ),
        ));
    }

    // Strategy 2 — header(128) + LZ4(rest) with the header as a prefix
    // dictionary. Cheap to try first because no DDS parsing is required.
    if let Some(out) = try_partial_header_lz4(decrypted, uncompressed_size) {
        return Ok(out);
    }
    // Strategy 3 — DDS-only per-mip layout.
    if let Some(out) = try_partial_dds_per_mip(decrypted, uncompressed_size) {
        return Ok(out);
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "partial PAZ entry uses an unrecognised layout (decrypted={}, u_size={})",
            decrypted.len(),
            uncompressed_size
        ),
    ))
}

fn try_partial_header_lz4(decrypted: &[u8], uncompressed_size: usize) -> Option<Vec<u8>> {
    let dict = &decrypted[..PARTIAL_HEADER_BYTES];
    let body = lz4_flex::block::decompress_with_dict(
        &decrypted[PARTIAL_HEADER_BYTES..],
        uncompressed_size - PARTIAL_HEADER_BYTES,
        dict,
    )
    .ok()?;
    if body.len() + PARTIAL_HEADER_BYTES != uncompressed_size {
        return None;
    }
    let mut out = Vec::with_capacity(uncompressed_size);
    out.extend_from_slice(dict);
    out.extend_from_slice(&body);
    Some(out)
}

/// Per-mip DDS layout: the DDS reserved area carries up to 11 u32 slots
/// giving each mip's on-disk byte length. A non-zero slot smaller than
/// the mip's raw size means that mip is LZ4-compressed; equal means
/// raw; zero means "the remaining mips are stored raw, sequentially".
fn try_partial_dds_per_mip(decrypted: &[u8], uncompressed_size: usize) -> Option<Vec<u8>> {
    let info = DdsInfo::parse(decrypted)?;
    if info.expected_total_size()? != uncompressed_size {
        return None;
    }
    if info.mip_count == 0 {
        return None;
    }
    let raw_mip_sizes: Vec<usize> = (0..info.mip_count)
        .map(|lvl| {
            let mw = (info.width >> lvl).max(1);
            let mh = (info.height >> lvl).max(1);
            info.mip_payload_size(mw, mh)
        })
        .collect::<Option<Vec<_>>>()?;

    // Up to 11 slots at offset 0x20.
    let max_explicit = info.mip_count.min(11);
    let mut reserved = [0u32; 11];
    for (i, slot) in reserved.iter_mut().enumerate().take(max_explicit) {
        let off = 0x20 + i * 4;
        *slot = u32::from_le_bytes(decrypted[off..off + 4].try_into().ok()?);
    }
    // Sanity: every explicit value must fit its expected raw size (LZ4
    // never produces larger output in this pipeline). Bail if not, so
    // we don't munge non-per-mip layouts.
    for (i, &value) in reserved.iter().enumerate().take(max_explicit) {
        if value == 0 {
            continue;
        }
        if value as usize > raw_mip_sizes[i] + 16 {
            return None;
        }
    }

    let body = &decrypted[info.data_offset..];
    let mut pos = 0usize;
    let mut out = Vec::with_capacity(uncompressed_size);
    out.extend_from_slice(&decrypted[..info.data_offset]);

    for lvl in 0..info.mip_count {
        let on_disk = if lvl < max_explicit { reserved[lvl] as usize } else { 0 };
        if on_disk == 0 {
            // Trailing raw mips — the body holds the remaining mip
            // levels stored sequentially without further compression.
            for r in raw_mip_sizes.iter().take(info.mip_count).skip(lvl) {
                if pos + r > body.len() {
                    return None;
                }
                out.extend_from_slice(&body[pos..pos + r]);
                pos += r;
            }
            break;
        }

        if pos + on_disk > body.len() {
            return None;
        }
        let chunk = &body[pos..pos + on_disk];
        pos += on_disk;
        let expected_raw = raw_mip_sizes[lvl];
        if on_disk == expected_raw {
            out.extend_from_slice(chunk);
        } else {
            let decoded = lz4_flex::block::decompress(chunk, expected_raw).ok()?;
            if decoded.len() != expected_raw {
                return None;
            }
            out.extend_from_slice(&decoded);
        }
    }

    if pos != body.len() {
        // Leftover body bytes mean we picked the wrong strategy.
        return None;
    }
    if out.len() != uncompressed_size {
        return None;
    }
    Some(out)
}

/// Minimal DDS header reader, just enough for the per-mip partial
/// decompressor: width, height, mip count, header length, and per-mip
/// raw byte size given (width, height). Covers every format observed
/// in Crimson Desert 1.06's PAZ archives (DXT1/3/5, DX10-wrapped
/// BC1..BC7, packed RGB(A), single-channel luminance, DX10 RGBA8 /
/// R8 / R16F / etc).
#[derive(Debug, Clone, Copy)]
struct DdsInfo {
    width: usize,
    height: usize,
    mip_count: usize,
    data_offset: usize,
    /// Distinguishes between block-compressed (BC*/DXT*), packed BPP,
    /// or DX10 DXGI codes.
    body: DdsBody,
}

#[derive(Debug, Clone, Copy)]
enum DdsBody {
    /// 8-byte 4×4 blocks: BC1 / DXT1 / BC4.
    Block8,
    /// 16-byte 4×4 blocks: BC2/3/5/6/7 / DXT3 / DXT5.
    Block16,
    /// Plain pixels at N bits each.
    PixelsBpp(usize),
}

impl DdsInfo {
    fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 128 || &data[..4] != b"DDS " {
            return None;
        }
        // The standard header is 124 bytes after the 4-byte magic;
        // dwSize at offset 4 should be 124.
        let flags = u32::from_le_bytes(data[8..12].try_into().ok()?);
        let height = u32::from_le_bytes(data[12..16].try_into().ok()?) as usize;
        let width = u32::from_le_bytes(data[16..20].try_into().ok()?) as usize;
        let mip_count = if flags & 0x00020000 != 0 {
            u32::from_le_bytes(data[28..32].try_into().ok()?) as usize
        } else {
            1
        };
        // Pixel format at offset 0x4C.
        let pf_flags = u32::from_le_bytes(data[80..84].try_into().ok()?);
        let fourcc = &data[84..88];
        let bpp = u32::from_le_bytes(data[88..92].try_into().ok()?) as usize;

        const DDPF_ALPHAPIXELS: u32 = 0x1;
        const DDPF_FOURCC: u32 = 0x4;
        const DDPF_RGB: u32 = 0x40;
        const DDPF_LUMINANCE: u32 = 0x20000;
        let _ = DDPF_ALPHAPIXELS;

        let mut data_offset = 128usize;
        let body = if pf_flags & DDPF_FOURCC != 0 {
            match fourcc {
                b"DXT1" => DdsBody::Block8,
                b"DXT3" | b"DXT5" | b"BC5U" | b"ATI2" => DdsBody::Block16,
                b"BC4U" | b"ATI1" => DdsBody::Block8,
                b"DX10" => {
                    if data.len() < 148 {
                        return None;
                    }
                    data_offset = 148;
                    let dxgi = u32::from_le_bytes(data[128..132].try_into().ok()?);
                    dxgi_body(dxgi)?
                }
                _ => return None,
            }
        } else if pf_flags & (DDPF_RGB | DDPF_LUMINANCE) != 0 {
            DdsBody::PixelsBpp(bpp)
        } else {
            return None;
        };
        Some(DdsInfo {
            width,
            height,
            mip_count,
            data_offset,
            body,
        })
    }

    fn mip_payload_size(self, width: usize, height: usize) -> Option<usize> {
        match self.body {
            DdsBody::Block8 => {
                let bw = width.div_ceil(4).max(1);
                let bh = height.div_ceil(4).max(1);
                Some(bw * bh * 8)
            }
            DdsBody::Block16 => {
                let bw = width.div_ceil(4).max(1);
                let bh = height.div_ceil(4).max(1);
                Some(bw * bh * 16)
            }
            DdsBody::PixelsBpp(bpp) => {
                if bpp == 0 || bpp % 8 != 0 {
                    return None;
                }
                Some(width * height * (bpp / 8))
            }
        }
    }

    fn expected_total_size(self) -> Option<usize> {
        let mut total = self.data_offset;
        let (mut w, mut h) = (self.width.max(1), self.height.max(1));
        let mips = self.mip_count.max(1);
        for _ in 0..mips {
            total += self.mip_payload_size(w, h)?;
            w = (w / 2).max(1);
            h = (h / 2).max(1);
        }
        Some(total)
    }
}

/// DX10 DXGI_FORMAT → DdsBody. Codes per
/// https://learn.microsoft.com/en-us/windows/win32/api/dxgiformat/ne-dxgiformat-dxgi_format
/// trimmed to what Pearl Abyss actually ships in 1.06.
fn dxgi_body(dxgi: u32) -> Option<DdsBody> {
    Some(match dxgi {
        // RGBA8 / BGRA8 32-bit
        28..=31 | 87..=91 => DdsBody::PixelsBpp(32),
        // R10G10B10A2
        24 | 25 => DdsBody::PixelsBpp(32),
        // R16G16B16A16_FLOAT
        10 => DdsBody::PixelsBpp(64),
        // R32G32B32A32_FLOAT
        2 => DdsBody::PixelsBpp(128),
        // R16_FLOAT
        54 | 55 => DdsBody::PixelsBpp(16),
        // R32_FLOAT
        41 | 43 => DdsBody::PixelsBpp(32),
        // R8_UNORM / R8_UINT
        61 | 62 => DdsBody::PixelsBpp(8),
        // Block-compressed
        70..=72 => DdsBody::Block8,   // BC1
        73..=78 => DdsBody::Block16,  // BC2 + BC3
        79..=81 => DdsBody::Block8,   // BC4
        82..=84 => DdsBody::Block16,  // BC5
        94..=96 => DdsBody::Block16,  // BC6H
        97..=99 => DdsBody::Block16,  // BC7
        _ => return None,
    })
}

// ── File processing ───────────────────────────────────────────────────────

/// Process a single file: compress then optionally encrypt.
/// Returns (processed_data, flags_byte).
pub fn process_file(
    data: &[u8],
    compression: Compression,
    crypto: CryptoType,
    encrypt_info: &[u8; 3],
    file_path: &str,
) -> io::Result<(Vec<u8>, u8)> {
    let compressed = compress(data, compression)?;

    let processed = match crypto {
        CryptoType::ChaCha20 => chacha20::encrypt_pack_entry(&compressed, encrypt_info, file_path),
        CryptoType::None => compressed,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!("crypto {:?} not supported for creation", crypto),
            ));
        }
    };

    let flags = compression as u8 | ((crypto as u8) << 4);
    Ok((processed, flags))
}

// ── Pack Group Builder (streaming to disk) ────────────────────────────────

/// Metadata for a file that has been added to a chunk (no data kept in memory).
struct FileMeta {
    dir_path: String,
    file_name: String,
    chunk_id: u16,
    chunk_offset: u32,
    compressed_size: u32,
    uncompressed_size: u32,
    flags: u8,
}

/// Metadata for a completed chunk written to disk.
struct ChunkMeta {
    id: u32,
    checksum: u32,
    size: u32,
}

/// Builds a pack group by streaming .paz files to disk.
///
/// Only file metadata is kept in memory; compressed data is written
/// to `{output_dir}/{chunk_id}.paz` immediately.
pub struct PackGroupBuilder {
    output_dir: PathBuf,
    compression: Compression,
    crypto: CryptoType,
    encrypt_info: [u8; 3],
    max_chunk_size: u64,
    // Completed chunks
    finished_chunks: Vec<ChunkMeta>,
    // Current chunk being built (in memory, flushed when full)
    current_chunk_id: u32,
    current_chunk_data: Vec<u8>,
    // All file metadata (kept for PAMT generation)
    file_metas: Vec<FileMeta>,
}

impl PackGroupBuilder {
    pub fn new(
        output_dir: &Path,
        compression: Compression,
        crypto: CryptoType,
        encrypt_info: [u8; 3],
        max_chunk_size: u64,
    ) -> Self {
        PackGroupBuilder {
            output_dir: output_dir.to_path_buf(),
            compression,
            crypto,
            encrypt_info,
            max_chunk_size,
            finished_chunks: Vec::new(),
            current_chunk_id: 0,
            current_chunk_data: Vec::new(),
            file_metas: Vec::new(),
        }
    }

    /// Add a file by providing its raw (uncompressed, unencrypted) data.
    /// The data is compressed/encrypted and appended to the current .paz chunk.
    /// If the chunk exceeds max_chunk_size, it is flushed to disk first.
    pub fn add_file(&mut self, dir_path: &str, file_name: &str, data: &[u8]) -> io::Result<()> {
        let full_path = if dir_path.is_empty() {
            file_name.to_string()
        } else {
            format!("{}/{}", dir_path, file_name)
        };

        let (processed, flags) = process_file(
            data,
            self.compression,
            self.crypto,
            &self.encrypt_info,
            &full_path,
        )?;

        let compressed_size = processed.len() as u64;

        // Flush current chunk if adding this file would exceed max_chunk_size
        if !self.current_chunk_data.is_empty()
            && self.current_chunk_data.len() as u64 + compressed_size > self.max_chunk_size
        {
            self.flush_current_chunk()?;
        }

        let chunk_offset = self.current_chunk_data.len() as u32;
        self.current_chunk_data.extend_from_slice(&processed);

        self.file_metas.push(FileMeta {
            dir_path: dir_path.to_string(),
            file_name: file_name.to_string(),
            chunk_id: self.current_chunk_id as u16,
            chunk_offset,
            compressed_size: compressed_size as u32,
            uncompressed_size: data.len() as u32,
            flags,
        });

        Ok(())
    }

    /// Add a file by reading it from a path on disk.
    /// Avoids the caller needing to load the file into memory themselves
    /// (though we still load it here for compression).
    pub fn add_file_from_path(
        &mut self,
        dir_path: &str,
        file_name: &str,
        file_path: &Path,
    ) -> io::Result<()> {
        let data = std::fs::read(file_path)?;
        self.add_file(dir_path, file_name, &data)
    }

    /// Flush the current in-progress chunk to disk.
    fn flush_current_chunk(&mut self) -> io::Result<()> {
        if self.current_chunk_data.is_empty() {
            return Ok(());
        }

        let crc = checksum::calculate_checksum(&self.current_chunk_data);
        let size = self.current_chunk_data.len() as u32;

        // Write to disk
        let paz_path = self
            .output_dir
            .join(format!("{}.paz", self.current_chunk_id));
        std::fs::write(&paz_path, &self.current_chunk_data)?;

        self.finished_chunks.push(ChunkMeta {
            id: self.current_chunk_id,
            checksum: crc,
            size,
        });

        self.current_chunk_data.clear();
        self.current_chunk_id += 1;

        Ok(())
    }

    /// Finish building: flush remaining data, write 0.pamt, return PAMT bytes.
    pub fn finish(mut self) -> io::Result<Vec<u8>> {
        // Flush any remaining chunk data
        self.flush_current_chunk()?;

        if self.finished_chunks.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "no files were added",
            ));
        }

        // Build PAMT from metadata
        let pamt_bytes = self.build_pamt()?;

        // Write 0.pamt to disk
        let pamt_path = self.output_dir.join("0.pamt");
        std::fs::write(&pamt_path, &pamt_bytes)?;

        Ok(pamt_bytes)
    }

    fn build_pamt(&self) -> io::Result<Vec<u8>> {
        // Collect directories and their files
        let mut dir_order: Vec<String> = Vec::new();
        let mut dir_files: std::collections::HashMap<String, Vec<usize>> =
            std::collections::HashMap::new();

        for (i, meta) in self.file_metas.iter().enumerate() {
            if !dir_files.contains_key(&meta.dir_path) {
                dir_order.push(meta.dir_path.clone());
                dir_files.insert(meta.dir_path.clone(), Vec::new());
            }
            dir_files.get_mut(&meta.dir_path).unwrap().push(i);
        }

        dir_order.sort();

        // Build trie buffers
        let dir_strs: Vec<&str> = dir_order.iter().map(|s| s.as_str()).collect();
        let (dir_names_buffer, dir_offsets) = build_trie_buffer(&dir_strs);

        // File names in directory-sorted order
        let mut ordered_file_indices: Vec<usize> = Vec::new();
        for dir in &dir_order {
            ordered_file_indices.extend_from_slice(&dir_files[dir]);
        }

        let file_names: Vec<&str> = ordered_file_indices
            .iter()
            .map(|&i| self.file_metas[i].file_name.as_str())
            .collect();
        let (file_names_buffer, file_name_offsets) = build_trie_buffer(&file_names);

        // PAMT chunks
        let pamt_chunks: Vec<PackMetaChunk> = self
            .finished_chunks
            .iter()
            .map(|c| PackMetaChunk {
                id: c.id,
                checksum: c.checksum,
                size: c.size,
            })
            .collect();

        // Directories and files
        let mut raw_directories: Vec<PackMetaDirectory> = Vec::new();
        let mut raw_files: Vec<PackMetaFileRaw> = Vec::new();
        let mut file_index: u32 = 0;

        for (dir_idx, dir) in dir_order.iter().enumerate() {
            let dir_file_indices = &dir_files[dir];
            let file_count = dir_file_indices.len() as u32;

            raw_directories.push(PackMetaDirectory {
                name_checksum: checksum::calculate_checksum(dir.as_bytes()),
                name_offset: dir_offsets[dir_idx],
                file_start_index: file_index,
                file_count,
            });

            for (local_idx, &global_idx) in dir_file_indices.iter().enumerate() {
                let meta = &self.file_metas[global_idx];
                raw_files.push(PackMetaFileRaw {
                    name_offset: file_name_offsets[file_index as usize + local_idx] as u32,
                    chunk_offset: meta.chunk_offset,
                    compressed_size: meta.compressed_size,
                    uncompressed_size: meta.uncompressed_size,
                    chunk_id: meta.chunk_id,
                    flags: meta.flags,
                    unknown0: 0,
                });
            }

            file_index += file_count;
        }

        let pamt = PackMeta {
            header: PackMetaHeader {
                checksum: 0,
                count: pamt_chunks.len() as u16,
                unknown0: 0, // seen in real files, always the same, maybe a version or magic?
                encrypt_info: PackEncryptInfo {
                    unknown0: 50,
                    encrypt_info: [2, 14, 97],
                },
            },
            chunks: pamt_chunks,
            directories: Vec::new(),
            dir_names_buffer,
            file_names_buffer,
            raw_directories,
            raw_files,
        };

        pamt.to_bytes_with_checksum()
    }
}

// ── File extraction from existing archives ───────────────────────────────

/// Extract a single file from a pack group archive.
///
/// Reads the compressed/encrypted data from the `.paz` chunk file,
/// decrypts if needed, then decompresses.
///
/// - `group_dir`: path to the group folder (e.g., `game_dir/0008`)
/// - `file`: the resolved file entry from PAMT
/// - `dir_path`: full directory path (for ChaCha20 nonce derivation)
/// - `encrypt_info`: 3-byte encryption info from the PAMT header
pub fn extract_file(
    group_dir: &Path,
    file: &super::pamt::ResolvedFile,
    dir_path: &str,
    encrypt_info: &[u8; 3],
) -> io::Result<Vec<u8>> {
    use std::io::Read;

    let paz_path = group_dir.join(format!("{}.paz", file.file.chunk_id));
    let mut fh = std::fs::File::open(&paz_path)
        .map_err(|e| io::Error::new(e.kind(), format!("{}: {}", paz_path.display(), e)))?;

    // Seek to the file's offset within the chunk
    std::io::Seek::seek(
        &mut fh,
        std::io::SeekFrom::Start(file.file.chunk_offset as u64),
    )?;

    // Read the compressed/encrypted data
    let mut raw = vec![0u8; file.file.compressed_size as usize];
    fh.read_exact(&mut raw)?;

    // Decrypt if needed
    let decrypted = match file.file.crypto {
        CryptoType::ChaCha20 => {
            let full_path = if dir_path.is_empty() {
                file.name.clone()
            } else {
                format!("{}/{}", dir_path, file.name)
            };
            chacha20::decrypt_pack_entry(&raw, encrypt_info, &full_path)
        }
        CryptoType::None => raw,
        other => {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!("crypto {:?} not supported for extraction", other),
            ));
        }
    };

    if file.file.is_partial {
        return decompress_partial(&decrypted, file.file.uncompressed_size as usize);
    }
    decompress(
        &decrypted,
        file.file.compression,
        file.file.uncompressed_size as usize,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_tempdir() -> tempfile::TempDir {
        tempdir().expect("failed to create temp dir")
    }

    #[test]
    fn test_compress_decompress_none() {
        let data = b"hello world";
        let compressed = compress(data, Compression::None).unwrap();
        assert_eq!(compressed, data);
        let decompressed = decompress(&compressed, Compression::None, data.len()).unwrap();
        assert_eq!(decompressed, data);
    }

    // ── Partial-compression decoder ───────────────────────────────────

    #[test]
    fn partial_identity_roundtrips() {
        // When compressed_size == uncompressed_size, the engine stored
        // the file verbatim. The decoder must return exactly those
        // bytes, untouched.
        let mut payload = Vec::with_capacity(256);
        payload.extend_from_slice(b"DDS ");
        payload.extend(std::iter::repeat_n(0u8, 124));
        payload.extend((0..128u8).cycle().take(128));
        let out = decompress_partial(&payload, payload.len()).unwrap();
        assert_eq!(out, payload);
    }

    #[test]
    fn partial_header_plus_lz4_roundtrips() {
        // Build a synthetic partial-compressed payload: a 128-byte
        // verbatim header followed by an LZ4 block that, when decoded
        // with the header as a prefix dictionary, reproduces the
        // original tail. Validates that decompress_partial uses
        // decompress_with_dict the right way.
        let mut header = vec![0u8; 128];
        for (i, b) in header.iter_mut().enumerate() {
            *b = i as u8;
        }
        // Tail that repeats the header twice + a fresh literal block.
        let mut tail = Vec::new();
        tail.extend_from_slice(&header);
        tail.extend_from_slice(&header);
        tail.extend_from_slice(b"hello partial PAZ\0");
        let uncompressed_size = header.len() + tail.len();

        let encoded_tail = lz4_flex::block::compress_with_dict(&tail, &header);
        let mut on_disk = header.clone();
        on_disk.extend_from_slice(&encoded_tail);

        let decoded = decompress_partial(&on_disk, uncompressed_size).unwrap();
        let mut expected = header.clone();
        expected.extend_from_slice(&tail);
        assert_eq!(decoded, expected);
    }

    #[test]
    fn partial_too_short_returns_error() {
        // Decrypted payload shorter than the 128-byte header carve-out
        // is malformed.
        let err = decompress_partial(&[1, 2, 3, 4], 4096).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn partial_per_mip_synthetic_roundtrips() {
        // Build a synthetic 8×8 BC1 (DXT1) DDS with two mip levels and
        // pack it as a per-mip partial entry:
        //   - mip 0 (8×8 BC1 = 32 bytes) → LZ4-compressed
        //   - mip 1 (4×4 BC1 = 8 bytes)  → raw (slot == raw size)
        //   - mip 2+ (none — mip_count = 2)
        let mut header = [0u8; 128];
        header[..4].copy_from_slice(b"DDS ");
        header[4..8].copy_from_slice(&124u32.to_le_bytes()); // dwSize
        header[8..12].copy_from_slice(&0x00021007u32.to_le_bytes()); // caps + h + w + pf + mipcount
        header[12..16].copy_from_slice(&8u32.to_le_bytes()); // height
        header[16..20].copy_from_slice(&8u32.to_le_bytes()); // width
        header[28..32].copy_from_slice(&2u32.to_le_bytes()); // mip count
        header[76..80].copy_from_slice(&32u32.to_le_bytes()); // pf dwSize
        header[80..84].copy_from_slice(&0x4u32.to_le_bytes()); // pf flags = DDPF_FOURCC
        header[84..88].copy_from_slice(b"DXT1");

        // Mip 0: 32 bytes of recognisable pattern; compressible.
        let mip0: Vec<u8> = (0..32).map(|i| (i as u8) & 0x0F).collect();
        let mip0_lz4 = lz4_flex::block::compress(&mip0);
        // Mip 1: 8 bytes of distinct data; stored raw.
        let mip1: Vec<u8> = (0..8).map(|i| 0xA0 + i as u8).collect();

        // Per-mip slots at offset 0x20.
        header[0x20..0x24].copy_from_slice(&(mip0_lz4.len() as u32).to_le_bytes());
        header[0x24..0x28].copy_from_slice(&(mip1.len() as u32).to_le_bytes());

        let mut on_disk = Vec::from(&header[..]);
        on_disk.extend_from_slice(&mip0_lz4);
        on_disk.extend_from_slice(&mip1);

        let uncompressed_size = header.len() + mip0.len() + mip1.len();
        let decoded = decompress_partial(&on_disk, uncompressed_size).unwrap();

        let mut expected = Vec::from(&header[..]);
        expected.extend_from_slice(&mip0);
        expected.extend_from_slice(&mip1);
        assert_eq!(decoded.len(), expected.len());
        // Body must match byte-for-byte. The header is allowed to differ
        // only in the per-mip slots, which the decoder leaves untouched.
        assert_eq!(&decoded[128..], &expected[128..]);
    }

    #[test]
    fn partial_per_mip_trailing_raw_synthetic() {
        // Build a 16×16 BC1 DDS with 3 mips where only mip 0 is
        // LZ4-compressed; mip 1 and mip 2 fall under the "trailing raw"
        // rule (slot == 0).
        let mut header = [0u8; 128];
        header[..4].copy_from_slice(b"DDS ");
        header[4..8].copy_from_slice(&124u32.to_le_bytes());
        header[8..12].copy_from_slice(&0x00021007u32.to_le_bytes());
        header[12..16].copy_from_slice(&16u32.to_le_bytes());
        header[16..20].copy_from_slice(&16u32.to_le_bytes());
        header[28..32].copy_from_slice(&3u32.to_le_bytes());
        header[76..80].copy_from_slice(&32u32.to_le_bytes());
        header[80..84].copy_from_slice(&0x4u32.to_le_bytes());
        header[84..88].copy_from_slice(b"DXT1");

        // Raw sizes for BC1: 16×16 → 4×4 blocks × 8 = 128 bytes
        //                   8×8  → 2×2 blocks × 8 = 32 bytes
        //                   4×4  → 1×1 blocks × 8 = 8 bytes
        let mip0: Vec<u8> = (0..128).map(|i| (i as u8) & 0x07).collect();
        let mip1: Vec<u8> = (0..32).map(|i| 0x80 ^ (i as u8)).collect();
        let mip2: Vec<u8> = (0..8).map(|i| 0xF0 | (i as u8 & 0xF)).collect();
        let mip0_lz4 = lz4_flex::block::compress(&mip0);

        header[0x20..0x24].copy_from_slice(&(mip0_lz4.len() as u32).to_le_bytes());
        // slot[1] and slot[2] are zero → "remaining mips are raw".

        let mut on_disk = Vec::from(&header[..]);
        on_disk.extend_from_slice(&mip0_lz4);
        on_disk.extend_from_slice(&mip1);
        on_disk.extend_from_slice(&mip2);

        let uncompressed_size = 128 + 128 + 32 + 8;
        let decoded = decompress_partial(&on_disk, uncompressed_size).unwrap();

        let mut expected = Vec::from(&header[..]);
        expected.extend_from_slice(&mip0);
        expected.extend_from_slice(&mip1);
        expected.extend_from_slice(&mip2);
        assert_eq!(decoded.len(), expected.len());
        assert_eq!(&decoded[128..], &expected[128..]);
    }

    #[test]
    fn partial_garbled_lz4_returns_error() {
        // 128-byte header is fine, but the "LZ4" body is nonsense
        // bytes that won't decode to the claimed uncompressed_size.
        let mut on_disk = vec![0u8; 128];
        on_disk.extend_from_slice(&[0xFFu8; 64]);
        let err = decompress_partial(&on_disk, 200_000).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    /// Live-install smoke check: walk a handful of partial-compressed
    /// DDS files spanning every supported sub-format — icons (identity
    /// + header-LZ4) and worldmap SDFs (per-mip table).
    ///
    /// Each must round-trip through `extract_file` to bytes that start
    /// with the DDS magic and match the PAMT-declared length. Skips
    /// cleanly when the game isn't installed.
    #[test]
    fn live_install_extracts_partial_icons() {
        let game = std::env::var_os("CRIMSON_GAME_ROOT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(
                    r"D:\SteamLibrary\steamapps\common\Crimson Desert",
                )
            });
        let group_dir = game.join("0012");
        let pamt_path = group_dir.join("0.pamt");
        let Ok(pamt_bytes) = std::fs::read(&pamt_path) else {
            eprintln!(
                "skipping live_install_extracts_partial_icons: no {}",
                pamt_path.display()
            );
            return;
        };
        let pamt = PackMeta::parse(&pamt_bytes, None).expect("0012/0.pamt parses");

        let mut checked_identity = 0usize;
        let mut checked_lz4 = 0usize;
        let mut checked_per_mip = 0usize;
        for d in &pamt.directories {
            let is_icon = d.path.starts_with("ui/texture/icon");
            let is_worldmap = d.path.starts_with("ui/texture/image/worldmap");
            if !is_icon && !is_worldmap {
                continue;
            }
            for f in &d.files {
                if !f.file.is_partial {
                    continue;
                }
                if !f.name.to_ascii_lowercase().ends_with(".dds") {
                    continue;
                }
                let want_identity = f.file.compressed_size == f.file.uncompressed_size;
                if is_icon {
                    if (want_identity && checked_identity >= 4)
                        || (!want_identity && checked_lz4 >= 4)
                    {
                        continue;
                    }
                } else {
                    // Worldmap exercises strategy 3 (per-mip).
                    if checked_per_mip >= 4 {
                        continue;
                    }
                }
                let out = extract_file(
                    &group_dir,
                    f,
                    &d.path,
                    &pamt.header.encrypt_info.encrypt_info,
                )
                .unwrap_or_else(|e| {
                    panic!("extract {}/{} failed: {}", d.path, f.name, e)
                });
                assert_eq!(
                    out.len(),
                    f.file.uncompressed_size as usize,
                    "{}/{} size mismatch",
                    d.path,
                    f.name
                );
                assert_eq!(
                    &out[..4],
                    b"DDS ",
                    "{}/{} should be a valid DDS",
                    d.path,
                    f.name
                );
                if is_icon {
                    if want_identity {
                        checked_identity += 1;
                    } else {
                        checked_lz4 += 1;
                    }
                } else {
                    checked_per_mip += 1;
                }
            }
            if checked_identity >= 4 && checked_lz4 >= 4 && checked_per_mip >= 4 {
                return;
            }
        }
        eprintln!(
            "live_install_extracts_partial_icons: identity={} lz4={} per_mip={}",
            checked_identity, checked_lz4, checked_per_mip,
        );
    }


    #[test]
    fn test_compress_decompress_lz4() {
        let data = b"hello world hello world hello world";
        let compressed = compress(data, Compression::Lz4).unwrap();
        let decompressed = decompress(&compressed, Compression::Lz4, data.len()).unwrap();
        assert_eq!(decompressed, data.as_ref());
    }

    #[test]
    fn test_compress_decompress_zlib() {
        let data = b"hello world hello world hello world";
        let compressed = compress(data, Compression::Zlib).unwrap();
        let decompressed = decompress(&compressed, Compression::Zlib, data.len()).unwrap();
        assert_eq!(decompressed, data.as_ref());
    }

    #[test]
    fn test_pack_group_builder_basic() {
        let dir = make_tempdir();
        let mut builder = PackGroupBuilder::new(
            dir.path(),
            Compression::None,
            CryptoType::None,
            [0, 0, 0],
            1_000_000,
        );

        builder
            .add_file("textures", "test.dds", b"fake texture data")
            .unwrap();
        builder
            .add_file("textures", "test2.dds", b"more texture data")
            .unwrap();
        builder
            .add_file("models", "mesh.obj", b"fake mesh data")
            .unwrap();

        let pamt_bytes = builder.finish().unwrap();

        // .paz and .pamt should exist on disk
        assert!(dir.path().join("0.paz").exists());
        assert!(dir.path().join("0.pamt").exists());

        // PAMT should be parseable
        let pamt = PackMeta::parse(&pamt_bytes, None).unwrap();
        assert_eq!(pamt.directories.len(), 2);
        assert_eq!(pamt.chunks.len(), 1);

        let dir_names: Vec<&str> = pamt.directories.iter().map(|d| d.path.as_str()).collect();
        assert!(dir_names.contains(&"models"));
        assert!(dir_names.contains(&"textures"));

        let total_files: usize = pamt.directories.iter().map(|d| d.files.len()).sum();
        assert_eq!(total_files, 3);
    }

    #[test]
    fn test_pack_group_builder_chunk_splitting() {
        let dir = make_tempdir();
        let mut builder = PackGroupBuilder::new(
            dir.path(),
            Compression::None,
            CryptoType::None,
            [0, 0, 0],
            50, // very small max chunk size
        );

        builder.add_file("dir", "file1.dat", &[0u8; 30]).unwrap();
        builder.add_file("dir", "file2.dat", &[1u8; 30]).unwrap();
        builder.add_file("dir", "file3.dat", &[2u8; 30]).unwrap();

        let pamt_bytes = builder.finish().unwrap();

        // Should have multiple .paz files
        assert!(dir.path().join("0.paz").exists());
        assert!(dir.path().join("1.paz").exists());

        let pamt = PackMeta::parse(&pamt_bytes, None).unwrap();
        assert_eq!(pamt.directories.len(), 1);
        assert_eq!(pamt.directories[0].files.len(), 3);
        assert!(pamt.chunks.len() >= 2);
    }

    #[test]
    fn test_pack_group_builder_deep_paths() {
        // Matches the game's 0008 pack group structure
        let dir = make_tempdir();
        let mut builder = PackGroupBuilder::new(
            dir.path(),
            Compression::None,
            CryptoType::None,
            [0, 0, 0],
            1_000_000,
        );

        builder.add_file("gamedata", "f1.bin", b"d1").unwrap();
        builder
            .add_file("gamedata/binary__", "f2.bin", b"d2")
            .unwrap();
        builder
            .add_file("gamedata/binary__/client", "f3.bin", b"d3")
            .unwrap();
        builder
            .add_file("gamedata/binary__/client/bin", "f4.bin", b"d4")
            .unwrap();
        builder
            .add_file("gamedata/binary__/misc", "f5.bin", b"d5")
            .unwrap();
        builder
            .add_file("gamedata/binary__/misc/bin", "f6.bin", b"d6")
            .unwrap();
        builder
            .add_file("gamedata/binarygimmickchart__", "f7.bin", b"d7")
            .unwrap();
        builder
            .add_file("gamedata/binarygimmickchart__/bin", "f8.bin", b"d8")
            .unwrap();

        let pamt_bytes = builder.finish().unwrap();
        let pamt = PackMeta::parse(&pamt_bytes, None).unwrap();

        // All 8 directories should be present and resolved correctly
        assert_eq!(pamt.directories.len(), 8);
        let dir_names: Vec<&str> = pamt.directories.iter().map(|d| d.path.as_str()).collect();
        assert!(dir_names.contains(&"gamedata"));
        assert!(dir_names.contains(&"gamedata/binary__"));
        assert!(dir_names.contains(&"gamedata/binary__/client"));
        assert!(dir_names.contains(&"gamedata/binary__/client/bin"));
        assert!(dir_names.contains(&"gamedata/binary__/misc"));
        assert!(dir_names.contains(&"gamedata/binary__/misc/bin"));
        assert!(dir_names.contains(&"gamedata/binarygimmickchart__"));
        assert!(dir_names.contains(&"gamedata/binarygimmickchart__/bin"));

        // Verify the radix trie structure: "gamedata" at root, "/binary" shared
        let buf = &pamt.dir_names_buffer;
        let parent0 = i32::from_le_bytes(buf[0..4].try_into().unwrap());
        let len0 = buf[4] as usize;
        let data0 = std::str::from_utf8(&buf[5..5 + len0]).unwrap();
        assert_eq!(parent0, -1);
        assert_eq!(data0, "gamedata");

        // Second entry should be "/binary" (shared prefix of "binary__" and "binarygimmickchart__")
        let off1 = 5 + len0;
        let parent1 = i32::from_le_bytes(buf[off1..off1 + 4].try_into().unwrap());
        let len1 = buf[off1 + 4] as usize;
        let data1 = std::str::from_utf8(&buf[off1 + 5..off1 + 5 + len1]).unwrap();
        assert_eq!(parent1, 0);
        assert_eq!(data1, "/binary");
    }

    #[test]
    fn test_pack_group_builder_with_compression() {
        let dir = make_tempdir();
        let mut builder = PackGroupBuilder::new(
            dir.path(),
            Compression::Lz4,
            CryptoType::None,
            [0, 0, 0],
            1_000_000,
        );

        let data = vec![0xABu8; 1000]; // repetitive data compresses well
        builder.add_file("data", "big.bin", &data).unwrap();

        let pamt_bytes = builder.finish().unwrap();
        let pamt = PackMeta::parse(&pamt_bytes, None).unwrap();

        let file = &pamt.directories[0].files[0];
        assert_eq!(file.file.uncompressed_size, 1000);
        assert!(file.file.compressed_size < 1000); // should actually compress
        assert_eq!(file.file.compression, Compression::Lz4);
    }
}
