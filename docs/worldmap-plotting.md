# World-map NPC plotting — investigation notes (2026-05-17)

Captured for the next session. Investigation is largely complete on
the math side; what's left is a single C ABI surface (a positioned-
entity enumerator) and one piece of user calibration cleanup.

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

## What the next session should ship

### Primary task: position enumerator C ABI

Same shape as `crimson_save_list_all_items`, but for positioned
entities. Yields one record per `(_spawnPosition, _spawnFieldInfoKey)`
pair across these container classes:

| Container | slot103 count | Position field |
|---|---:|---|
| `MercenarySaveData` | 96 | `_spawnPosition: float3` |
| `FieldNPCSaveData` | 228 | TBD — probe expected to confirm |
| `FieldGimmickSaveData` | 4,260 | TBD |
| `TransformFieldSaveData` | 1 (per visited field) | `_position: float3` (the active char) |
| `GameData_GimmickPointData` | 857 | TBD |

Per-record fields (suggested `repr(C)`, ~48 bytes):

```rust
#[repr(C)]
pub struct CrimsonPositionedEntityRecord {
    pub block_idx: u32,
    pub kind: u32,                // enum: MERCENARY / FIELD_NPC / GIMMICK / ACTIVE_CHAR / ...
    pub field_info_key: u32,      // _spawnFieldInfoKey for client-side region filtering
    pub character_key: u32,       // for MERCENARY / FIELD_NPC; 0 otherwise
    pub gimmick_info_key: u32,    // for GIMMICK; 0 otherwise
    pub mercenary_no: u64,        // for MERCENARY; 0 otherwise
    pub pos_x: f32,               // _spawnPosition[0] (already global)
    pub pos_y: f32,               // _spawnPosition[1] — height (ignored for top-down plotting)
    pub pos_z: f32,               // _spawnPosition[2] (already global)
    pub yaw: f32,                 // _spawnYaw (for directional markers)
}
```

C# editor pipeline:
1. `crimson_save_list_field_positions(handle, …)` → record array.
2. (Optional) Filter by `field_info_key` if showing one region at a time.
3. Apply the affine: `(px, py) = (0.432044*pos_x + 5937.50, -0.433071*pos_z + 1864.08)`.
4. Plot at `(px, py)` on the basemap. Use `yaw` for facing direction if rendering arrows.

The affine coefficients can be exposed as constants in the docs, or
embedded in the editor — they're stable per-basemap. If a different
basemap is used, the editor re-calibrates client-side via numpy /
its own least-squares helper.

### Secondary / nice-to-have (not blocking the editor)

- **Field-origin lookup table from gamedata.** The chunk origins are
  a multiple-of-1000 grid. There's likely a `fieldinfo.pabgb` or
  similar bridge that maps `FieldInfoKey → (origin_x, origin_y,
  origin_z)`. Finding it would let downstream tools convert
  CE-local coords → global without per-field re-calibration. Hunt
  pattern: PAMT scan for `fieldinfo*` / `fieldsublevel*` /
  `sublevel*` in group 0008.

- **Re-verify Hernand Town TP coord.** Single CE read in-game.

- **Extend `MercenarySaveData`-tagged ABI** with the same global
  `_spawnPosition` so the C# editor can hover a mount marker and
  see "this is Damine's horse".

## Cross-references

- `src/c_abi/character_info.rs::tests::_probe_transform_save_data_slot102` — schema of `TransformFieldSaveData` (field 2 = `_position`).
- `src/c_abi/character_info.rs::tests::_extract_worldmap_dds` — refresh map texture extraction.
- `scripts/worldmap_tp_fit.py` — re-run the affine fit if more calibration points land.
- `docs/dye-editor-scope.md` — sibling investigation; same data-shape pattern (palette positions matched via byte-order swap).
