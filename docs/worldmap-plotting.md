# World-map NPC plotting — investigation notes (2026-05-17)

Investigation complete. The position enumerator C ABI
(`crimson_save_list_field_positions`) shipped on 2026-05-17 — see
[the "Shipped" section below](#shipped-position-enumerator-c-abi-2026-05-17).
Coordinate model, affine fit, and asset extraction are pinned. Only
the Hernand Town TP-coord re-read remains as user calibration
cleanup; everything else is optional follow-on.

## Goal

Render every save-side positioned entity (active char, mounts,
mercenaries, world NPCs, gimmicks, …) as a marker on the in-game
world map. The user supplies a basemap image; we supply the
coordinate transform + the per-entity position list.

## Confirmed coordinate model

### Three coordinate frames, related by integer-multiple-of-1000 offsets

| Frame | Source | Used for |
|---|---|---|
| **Global world (TP / save)** | `_position` / `_spawnPosition` in save body. Also matches the "CE TP marker on map" values the in-game teleport system uses. | Cross-region plotting, the affine fit below |
| **Field-local** | What the in-game CE injection reports for the active character — position **relative to the current sublevel chunk's origin**. | In-game-only; **do NOT use for plotting** |
| **Map pixel** | (x, y) on the user's 5178×5240 web-fetched basemap (`crimson-desert-full-world-map.jpg`) | Final render coords |

### The chunk grid

The world is partitioned into **1000×1000 game-unit chunks** on an
integer grid. Each chunk has a global origin at integer-multiple-of-
1000 coords. The in-game CE-read position is `global - chunk_origin`;
the save's `_position` and the TP marker are the **global** value
(chunk origin already baked in). Verified across 9 landmarks:

| Landmark | Chunk origin (X, Z) |
|---|---|
| Char + Abyss Nexus Howling Hill | (−10000, −4000) |
| Abyss Nexus Witch Woods | (−11000, −4000) |
| Abyss Cresset Five Finger Mtn | (−11000, 0) |
| Abyss Nexus Coast Windmill | (−4000, −4000) |
| Abyss Cresset Trivana Sound | (−6000, +1000) |
| Abyss Cresset Frozen Souls Mtn | (−11000, −6000) |
| Abyss Nexus Vellua | (−10000, −6000) |
| Abyss Nexus Three Brother's Cliff | (−6000, −5000) |

The slot102 active character: save `_position` = `(-10502.729, 610.6218, -4373.9663)`, chunk origin `(-10000, 0, -4000)`, in-game CE = `(-502.7, 610.6, -373.97)`. The math closes within floating-point precision.

**Practical implication for the C# editor**: the save's `_position` /
`_spawnPosition` fields are **already in global frame** — no
conversion needed before applying the affine. The in-game CE
injection's local-relative reading is what caused the original
calibration to fail with 1286 px RMSE; once we switched to TP-marker
coords, RMSE dropped to 6.4 px.

**Possibly explains in-game teleport failures**: if a TP destination
is written in CE-local coords without the chunk-origin offset, the
target lands in the wrong chunk. User mentioned seeing this in-game.

## The affine fit (for the user's web basemap)

Map: 5178 × 5240 pixels. Image source: web-fetched
`crimson-desert-full-world-map.jpg`.

### Calibration points

9 landmarks with verified TP-marker coords + map pixel coords. Hernand
Town was excluded — its TP-marker value was copy/pasted from Abyss
Nexus HH in the user's data; needs a re-check before reuse.

### Coefficients (least-squares fit, RMSE 6.4 px, max 15.1 px)

```text
map_px =  0.432044 * X + ~0          * Z + 5937.50
map_py = ~0         * X + -0.433071  * Z + 1864.08
```

Off-diagonal terms are ~0.0001 — essentially a **diagonal matrix with
a Z-axis flip**. World X → map +x (east), world Z → map −y (Z positive
= north → smaller pixel y). No rotation, no shear. Uniform scale
**~0.432 px / world unit**, equivalent to **2.31 world units per
pixel**. Continent spans ~12000 world units east-west.

Each 1000-unit chunk = 432 pixels on the map. The continent is
roughly 12 × 12 chunks across.

### Inverse (clicked pixel → world)

```text
X =  2.314577 * map_px + -0.002129 * map_py + -13738.83
Z = -0.000652 * map_px + -2.309090 * map_py +   4308.19
```

### Per-pair scale sanity

After the TP-marker swap: min 0.411, median 0.433, max 0.448 px/unit
(1.09× ratio). Before: 0.46 to 20.00 (43× ratio). Confirms global
frame consistency.

## Data inputs / known gaps

### What we have

- **9 well-calibrated TP-marker points** spread across 8 chunks (Hernand cluster + NW + NE + E + SW + Vellua + Three Brother's).
- **In-game CE coords** for the same 9 points (field-local; used to derive the per-chunk origin table above).
- **The web basemap** with hand-marked pixel coords for the 9 points.

### What's missing / needs re-verification

1. **Hernand Town TP-marker value** — user's data had it copy-pasted from Abyss Nexus HH. Re-read in-game.
2. **Witch Woods** — fits the global affine cleanly (3.9 px residual), but was a 365 px outlier in the earlier (broken) per-field fit. Either accept as fine or re-verify.
3. **Silver Wolf Mountain** — user skipped, location forgotten. Skip permanently or re-locate.

## Extracted assets

Run `cargo test --lib --features c_abi _extract_worldmap_dds -- --ignored --nocapture` to refresh.
Output: `out/worldmap/*.dds` and (via `scripts/decode_worldmap_dds.py`) `out/worldmap/*.png`.

| File | Size | What it is |
|---|---|---|
| `cd_global_map_navigator_guide_00.dds/.png` | 1024² | **Game's in-engine world map** (faction-colored, chunk grid visible). Not user-facing — the web jpg is preferred. |
| `cd_uitexture_worldmap_00..04.dds/.png` | 1024² × 5 | Map UI panel sprites (banners, masks). Not the basemap itself. |
| `cd_uitexture_worldmap_marker_00.dds/.png` | 1024² | Single marker icon |
| `cd_icon_worldmap_00.dds/.png` | 1024² | Diamond POI atlas (towns, shops, quest, …) |
| `cd_worldmap_*_pattern.dds/.png` | 512² × 4 | Tile fill patterns (land, sea, paper, cloud) |

DDS decode uses `crimsonforge/core/dds_reader.py` (handles type-1 self-compressed DDS + DXT1/3/5 + BC4/5/6/7 + uncompressed BGRA/RGBA). Pillow writes the PNGs.

## Probes added this session

All in `src/c_abi/character_info.rs`, all `#[ignore]`'d (don't affect CI test count). Re-runnable with `cargo test --lib --features c_abi <name> -- --ignored --nocapture`:

| Probe | Purpose |
|---|---|
| `_probe_active_char_position_slot102` | Scan slot102 for any float3 matching the CE target. Helped identify that in-game CE coords ARE field-local (no save field matched). |
| `_probe_transform_save_data_slot102` | Dump `TransformSaveData` + `TransformFieldSaveData` schemas. Located the active char's `_position` field (TransformFieldSaveData field 2, type `float3`). |
| `_probe_worldmap_assets` | Scan every PAMT group for map-like asset paths. Found `ui/texture/cd_uitexture_worldmap_*.dds` in group 0012 + `object/texture/cd_global_map_navigator_guide_00.dds` in group 0000. |
| `_extract_worldmap_dds` | Extract every map texture to `out/worldmap/`. |

Plus three Python scripts under `scripts/`:
- `decode_worldmap_dds.py` — DDS → PNG batch decoder.
- `worldmap_affine_fit.py` — original failed fit (with in-game CE coords).
- `worldmap_affine_diagnose.py` — diagnostic (per-pair scale, leave-one-out, subsets) — produced the per-pair-scale-varies-43× evidence.
- `worldmap_tp_fit.py` — successful fit with TP-marker coords.
- `worldmap_hernand_only.py` — Hernand-cluster-only fit (pre-TP-coord realization).

## Shipped: position enumerator C ABI (2026-05-17)

`crimson_save_list_field_positions` lives in
[`src/c_abi/positions.rs`](../src/c_abi/positions.rs). Same shape as
`crimson_save_list_all_items` — two-call sizing-then-fill, fills a
caller-provided buffer of 56-byte `CrimsonPositionedEntityRecord`
rows, populates an `out_version` stamp for snapshot staleness
detection, and panics route to `error::PANIC`.

### Final container coverage (slot103 baseline)

| Kind | Container | Position source | Records |
|---|---|---|---:|
| `ACTIVE_CHAR` | `TransformSaveData._fieldSaveDataList[0]` → `TransformFieldSaveData._position` | F32x3 direct | 1 |
| `MERCENARY` | `MercenaryClanSaveData._mercenaryDataList[N]._spawnPosition` | F32x3 direct | 76 (of 96 — 20 mercs have no present `_spawnPosition`) |
| `GIMMICK` | `FieldSaveData._fieldGimmickSaveDataList[N]._transform` ∥ `_originSpawnTransform` | 40-byte `Transform` scalar — scale[3] + quat[4] + pos[3], decoded from `ScalarValue::Bytes` | 3,240 (of 4,260 — 24% are state-only, no transform) |

**Total: 3,317 records in slot103**.

### What changed vs the original spec

- **`FieldNPCSaveData` dropped from scope.** The 12 fields don't
  include a position — NPC positions live in gamedata level data,
  not the save. Plotting NPCs needs a separate gamedata-level data
  bridge.
- **`GameData_GimmickPointData` dropped from scope.** 857 instances
  exist at the TOC level, but `_transform` is universally absent in
  slot103. Revisit if a future probe finds present transforms.
- **`FieldGimmickSaveData` lives nested**, not at TOC. The host is
  `FieldSaveData._fieldGimmickSaveDataList` (count=4260 at `toc[873]`
  in slot103). The walker enumerates direct children only — nested
  child gimmicks (in `_fieldGimmickSaveData_AutoSpawnChildList` etc.)
  co-locate with their parent and are not enumerated separately.
- **Transform decoding**: the 40-byte `Transform` type lands in
  `ScalarValue::Bytes(Vec<u8>)`, NOT `FieldValue::InlineBytes`.
  Schema is `scale(0..12) + quaternion(12..28) + position(28..40)`.
- **Yaw**: mercenaries have `_spawnYaw` (direct f32); active char
  and gimmicks derive yaw from the quaternion as
  `atan2(2(wy + xz), 1 - 2(y² + z²))`. Every observed transform is
  a pure Y-axis rotation (qx ≈ qz ≈ 0), so the formula reduces to
  `2·atan2(qy, qw)`.
- **Final record size: 56 bytes** (12 u32/f32 + 1 u64), 8-aligned.
  Layout pinned by `record_layout_is_stable` test.

### C# editor pipeline (unchanged)

1. `crimson_save_list_field_positions(handle, …)` → record array.
2. (Optional) Filter by `field_info_key` if showing one region at a time.
3. Apply the affine: `(px, py) = (0.432044*pos_x + 5937.50, -0.433071*pos_z + 1864.08)`.
4. Plot at `(px, py)` on the basemap. Use `yaw` for facing direction if rendering arrows.

Live test pinned by `live_affine_lands_in_basemap`: slot103's
active char projects to pixel `(1399.9, 3758.3)` on the 5178×5240
basemap — comfortably in-bounds, matches manual placement.

### Basemap tile discovery + extraction (shipped 2026-05-17, session 7)

The player-facing world map is **procedurally rendered at runtime** —
the game has no single rasterised basemap file. Source ingredients
live in two places:

| Group / dir | What's there | Per-file resolution |
|---|---|---|
| `0015/leveldata/rootlevel/terrain/global/global_colormap.dds` | Pre-stitched whole-world colormap | **2048×2048** |
| `0015/leveldata/rootlevel/terrain/color/` | 785 per-tile colormaps `terrain_X_Y_color_c.dds` (X, Y ∈ ~[-19,+19]) | 512×512 per tile, ~14,332² stitched |
| `0015/leveldata/rootlevel/terrain/height16f/` | 785 16-bit float heightmap tiles | 512×512 per tile |
| `0015/leveldata/rootlevel/terrain/normal/` | 784 surface-normal tiles | 512×512 per tile |
| `0015/leveldata/rootlevel/terrain/region/` | 784 per-pixel region-ID tiles | 512×512 per tile |
| `0012/ui/texture/image/worldmap/cd_worldmap_blur_height.dds` | UI-side heightmap (relief shading) | **8192×8192** |
| `0012/ui/texture/image/worldmap/cd_worldmap_road_sdf_32768x32768.dds` | UI-side road SDF (every road in the world) | 8192×8192 (filename's 32768 is the SDF distance range, not pixel dims) |
| `0012/ui/texture/image/worldmapregiontitle/` | 234 region-title decals (HERNAND / CRIMSON DESERT / etc.) | 1024×1024 each |

**Tile ↔ chunk relationship**: each `terrain_X_Y_*` tile covers
**1000×1000 game units** — exactly one chunk on the chunk grid pinned
earlier in this doc. Per-pixel scale ≈ 0.5 px / game unit.

### Shipped ABI: `crimson_paz_list_dir`

Lists every file in a PAMT directory as 272-byte `repr(C)`
`CrimsonPazFileEntry` records (filename + sizes + flags). Pairs with
the existing `crimson_paz_extract_file` for the full
discover → extract pipeline. C# editor workflow:

```csharp
string pamt = Path.Combine(gameRoot, "0015", "0.pamt");
var tiles = CrimsonPaz.ListDir(pamt, "leveldata/rootlevel/terrain/color");
// → 785 entries; 784 are 174,904 B, one edge tile is 43,832 B
foreach (var entry in tiles) {
    string cachePath = Path.Combine(localCache, entry.Name);
    if (File.Exists(cachePath)) continue;
    byte[] dds = CrimsonPaz.ExtractFile(pamt, "leveldata/rootlevel/terrain/color", entry.Name);
    File.WriteAllBytes(cachePath, dds);   // editor decodes/composites locally
}
```

See `src/c_abi/paz.rs::crimson_paz_list_dir`. Live regression
`c_abi_paz_list_dir_terrain_color_live` pins the 785-tile count +
174,904 B dominant size on slot103. The 1.07 game install must be on
the host running the test, otherwise the test skips cleanly.

**Scope split (decision 2026-05-17)**:
- **Rust side**: tile / asset extraction from PAZ (existing
  `crimson_paz_extract_file`) + directory listing (new
  `crimson_paz_list_dir`).
- **C# side**: DDS decoding (TBD whether to add `crimson_dds_decode`
  later — the user's editor will try existing DDS libs first), local
  cache management, GPU-side compositing of layers (heightmap +
  roads + colormap + region titles), POI marker overlay via the
  existing `crimson_save_list_field_positions` data + affine fit.

The two-call sizing-then-fill shape matches every other listing ABI
in this crate. NOT yet exposed (defer to a future PR if needed):
DDS decode (`crimson_dds_decode_to_rgba`), top-level directory
enumeration across an entire PAMT, or batch extraction.

### Secondary / nice-to-have (not blocking the editor)

- **Field-origin lookup table from gamedata.** The chunk origins are
  a multiple-of-1000 grid. There's likely a `fieldinfo.pabgb` or
  similar bridge that maps `FieldInfoKey → (origin_x, origin_y,
  origin_z)`. Finding it would let downstream tools convert
  CE-local coords → global without per-field re-calibration. Hunt
  pattern: PAMT scan for `fieldinfo*` / `fieldsublevel*` /
  `sublevel*` in group 0008.

- **Re-verify Hernand Town TP coord.** Single CE read in-game.

- **Recursive gimmick walker.** Today's enumerator stops at
  `_fieldGimmickSaveDataList[N]`. The five `_fieldGimmickSaveData_*ChildList`
  sublists carry ~1,020 nested gimmicks that share their parent's
  approximate position. Add a variant ABI (`*_list_all_field_positions_deep`)
  if the editor grows a "drill into multi-slot container" feature.

- **NPC positions from level data.** `FieldNPCSaveData` carries
  `_spawnFieldInfoKey` + `_characterKey` but no position — to plot
  NPCs, load the matching `levelinfo*.pabgb` or equivalent gamedata
  and join against the save's NPC presence.

## Cross-references

- [`src/c_abi/positions.rs`](../src/c_abi/positions.rs) — the shipped ABI surface.
- `src/c_abi/character_info.rs::tests::_probe_positioned_entity_hosts` — schema discovery probe (run with `--ignored --nocapture` for cross-version verification).
- `src/c_abi/character_info.rs::tests::_probe_transform_save_data_slot102` — schema of `TransformFieldSaveData` (field 2 = `_position`).
- `src/c_abi/character_info.rs::tests::_extract_worldmap_dds` — refresh map texture extraction.
- `scripts/worldmap_tp_fit.py` — re-run the affine fit if more calibration points land.
- `docs/dye-editor-scope.md` — sibling investigation; same data-shape pattern (palette positions matched via byte-order swap).
