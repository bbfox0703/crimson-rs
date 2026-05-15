# Crimson Desert Archive Format

## File Hierarchy

```
game_dir/
├── meta/
│   └── 0.papgt          # Master index — lists all pack groups
├── 0000/
│   ├── 0.pamt           # VFS index for this group (trie + file metadata)
│   ├── 0.paz            # Concatenated compressed/encrypted file data
│   ├── 1.paz
│   └── ...
├── 0001/
│   ├── 0.pamt
│   └── *.paz
└── ...
```

## PAPGT (Pack Group Tree Meta) — `meta/0.papgt`

Master index listing all pack groups in the game.

### Header (12 bytes)
| Offset | Type | Field |
|--------|------|-------|
| 0 | u32 | unknown0 |
| 4 | u32 | checksum (Jenkins hashlittle2 of post-header data) |
| 8 | u8 | entry_count |
| 9 | u8 | unknown1 |
| 10 | u16 | unknown2 |

### Entry (repeated `entry_count` times)
| Offset | Type | Field |
|--------|------|-------|
| 0 | u8 | is_optional |
| 1 | u16 | language (bitmask, 0x3FFF = ALL) |
| 3 | u8 | always_zero |
| 4 | u32 | group_name_offset (into group_names_buffer) |
| 8 | u32 | pack_meta_checksum (checksum of group's 0.pamt post-header) |

### Group Names Buffer
- i32 length prefix
- Null-terminated C strings, referenced by entries via offset

### Load Order
The game reads entries front-to-back. First match wins for file resolution. This is how mods override game files — see [Mod Loading](#mod-loading-overlay-approach).

---

## PAMT (Pack Meta) — `{group}/0.pamt`

Virtual filesystem index for a single pack group. Maps directory paths and file names to offsets within `.paz` chunk files.

### Header (12 bytes)
| Offset | Type | Field |
|--------|------|-------|
| 0 | u32 | checksum (Jenkins hashlittle2 of post-header data) |
| 4 | u16 | count |
| 6 | u8 | unknown |
| 7 | u8[3] | encrypt_info |
| 10 | u8[2] | padding |

### Post-Header Structure
1. **Chunks array** — `(id: u32, checksum: u32, size: u32)` per `.paz` file
2. **Dir names trie buffer** — trie-encoded directory paths (i32 length prefix)
3. **File names trie buffer** — trie-encoded file names (i32 length prefix)
4. **Directories array** — `(name_checksum: u32, name_offset: i32, file_start_index: u32, file_count: u32)`
5. **Files array** — `(name_offset: i32, chunk_offset: u32, compressed_size: u32, uncompressed_size: u32, chunk_id: u16, flags: u8, unknown0: u8)`

### File Flags Byte
- Bits 0-3: compression type (0=None, 2=LZ4, 3=Zlib, 4=QuickLZ)
- Bits 4-7: crypto type (0=None, 1=ICE, 2=AES, 3=ChaCha20)

---

## PAZ (Pack Archive) — `{group}/{n}.paz`

Headerless concatenated file data. Each file's raw bytes (after compression and optional encryption) are written sequentially. File locations are tracked by the PAMT index via `chunk_id` (which `.paz` file) and `chunk_offset` (byte offset within that file).

### Processing Pipeline
1. Read raw file data
2. Compress (LZ4, Zlib, or None)
3. Encrypt (ChaCha20, AES, ICE, or None)
4. Append to current chunk; split to new `.paz` when `max_chunk_size` exceeded

---

## Trie Buffer Format

Used by PAMT for both directory names and file names. A compact prefix-sharing encoding.

### Entry Format
| Offset | Type | Field |
|--------|------|-------|
| 0 | i32 (LE) | parent_offset (-1 for root entries) |
| 4 | u8 | string_length |
| 5 | u8[string_length] | string_data |

### Encoding Rules
1. Paths are split on `/` into directory segments
2. Non-root segments get `/` prepended (e.g., `"/binary__"`, `"/client"`)
3. Siblings at each trie level are radix-compressed (byte-level prefix sharing)

### Example
For paths `gamedata/binary__` and `gamedata/binarygimmickchart__`:
```
offset=0   parent=-1  data="gamedata"
offset=13  parent=0   data="/binary"           # shared prefix
offset=25  parent=13  data="__"                # completes "binary__"
offset=32  parent=13  data="gimmickchart__"    # completes "binarygimmickchart__"
```

To reconstruct a full string, walk parent pointers to root and concatenate.

---

## PABGB / PABGH (Game Data Tables — `0008/gamedata/binary__/client/bin/*.pabg{b,h}`)

PAPGT / PAMT / PAZ are the **archive layer**. The files actually packed inside the archives — `iteminfo.pabgb`, `missioninfo.pabgb`, `skill.pabgb` + `skill.pabgh`, `characterappearanceindexinfo.pabgb` + `.pabgh`, etc. — are the **game-data layer** and share a couple of well-trodden patterns documented here.

### Two layouts

Some `.pabgb` files ship alone with **anchor-scannable rows**:

```text
[u32 key][u32 name_len][name_len bytes ASCII identifier][...variable body]
[u32 key][u32 name_len][name_len bytes ASCII identifier][...variable body]
...
```

Used by `iteminfo`, `missioninfo`, `questinfo`, `stageinfo`, `gimmickinfo`, `characterinfo`, `sublevelinfo`, `knowledgeinfo`, `questgaugeinfo`. Parsers under `src/<table>_info/mod.rs` walk these byte-by-byte with validation rules:

- `key != 0 && (key >> 24) == 0` (some tables loosen this — see `gimmick_info` for the `< 0x10` variant covering cat-byte rows)
- `name_len ∈ [2, 128]`
- name bytes ASCII alphanumeric / `_` / ` ` / UTF-8 high bytes
- name parses as valid UTF-8
- some tables add table-specific filters (e.g. characterinfo rejects all-digit names — those are embedded stringinfo refs)

Other `.pabgb` files ship a **PABGH index sibling** for offset-based lookup:

```text
foo.pabgh:  [count][count × (key, offset)]            ← index
foo.pabgb:  [entry_0 bytes][entry_1 bytes][...]      ← bodies at the offsets the index points to
```

Two header shapes observed so far:

| Pattern | Used by | Where |
|---|---|---|
| `[u16 count][count × (u32 key, u32 offset)]` | `skill.pabgh` | `src/skill_info/pabgh.rs` |
| `[u32 count][count × (u64 key, u32 offset)]` | `characterappearanceindexinfo.pabgh` | Pinned 2026-05-15 in `docs/save-editor-keys-plan.md` §9; **bridge not yet shipped** — investigation deferred until the 21-byte PABGB body schema is RE'd and the 87% save-side miss path is located |

PABGB entry bodies have **no standard layout** — each table's body is schema-per-file. The PABGH variant exists for tables where bodies are variable-size and need offset indexing (skill.pabgb's BuffData tail-size drift across versions, for example) or where the lookup key is wider than u32 (CharacterAppearanceIndexKey's u64).

### When to use which

- **Anchor-scan parser** when the rows are uniform-shape `[key, name, body]` and a byte-by-byte scan can find them reliably (every standalone `.pabgb` we've shipped a bridge for so far).
- **PABGH-indexed parser** when (a) bodies are variable-size and you need O(1) lookup, or (b) the key type is wider than u32 and anchor-scan's u32 key constraints don't fit.

---

## Checksum

Jenkins hashlittle2 with constant seed `0xDEBA1DCD`.

Used in:
- PAMT header (covers post-header data)
- PAPGT header (covers post-header data)
- PAZ chunk verification
- Directory name hashing in PAMT

---

## Mod Loading (Overlay Approach)

The game resolves files by scanning PAPGT entries front-to-back. First match wins.

### How It Works
1. **Create a new pack group** (e.g., `0036/`) containing modified files packed into `.paz` + `0.pamt`
2. **Insert the mod entry at the front** of both the PAPGT entries list and the group_names buffer
3. **Replace `meta/0.papgt`** with the updated version

The original game archives are never modified. When the game looks up a file, it finds the mod's version first (because the mod's entry is at index 0), effectively overlaying the original.

This is the same approach used by other Crimson Desert mod loaders.

### Pipeline (automated by `pack_mod()`)
```
mod files on disk
    → compress (LZ4/Zlib) + optional encrypt
    → write .paz chunks
    → build trie buffers for dir/file names
    → create 0.pamt with checksums
    → load original 0.papgt
    → insert mod entry at front (upsert)
    → write updated 0.papgt
```
