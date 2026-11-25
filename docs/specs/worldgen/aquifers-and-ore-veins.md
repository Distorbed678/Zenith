# Worldgen: aquifers, ore veins, and terrain fill (26.3)

- Verified by suites: `golden-terrain`
- Sources: Minecraft: Java Edition 26.3 data-pack schemas (`minecraft:worldgen/noise_settings`, `minecraft:worldgen/material_rule`, `minecraft:worldgen/density_function`), `docs/specs/math/noise.md`, `docs/specs/math/random.md`, `docs/specs/math/java-math-helpers.md`, `docs/specs/codec/datapacks-and-codecs.md`, `docs/specs/registry/identifiers-registries-tags.md`, and local reference reading (2026-11-25). Everything below is written in our own words.
- Related specs: [`math/noise.md`](../math/noise.md), [`math/random.md`](../math/random.md), [`math/java-math-helpers.md`](../math/java-math-helpers.md), [`codec/datapacks-and-codecs.md`](../codec/datapacks-and-codecs.md), [`registry/identifiers-registries-tags.md`](../registry/identifiers-registries-tags.md), [`worldgen/density-functions.md`](./density-functions.md), [`worldgen/biomes-and-climate.md`](./biomes-and-climate.md), [`worldgen/surface-and-carvers.md`](./surface-and-carvers.md).

## Summary

Terrain generation in 26.3 populates solid rock, air, fluids, and large ore veins across two phases of the single `terrain` chunk-generation step:

1. **Aquifers (during density fill and carver application)**: Every block where `final_density <= 0.0` (and every carved voxel during carver mask application, evaluated with `density = 0.0`) queries an `Aquifer` to determine whether the voxel becomes solid terrain (`null`, replaced by `default_block` during fill or left untouched during carving), a fluid block (typically `minecraft:water` or `minecraft:lava`), or `minecraft:air`.
   - In 26.3, aquifers are configured by an **optional** `"aquifers"` object inside `NoiseGeneratorSettings` rather than a boolean flag plus `NoiseRouter` fields. When `"aquifers"` is absent, a disabled aquifer falls back directly to the global fluid picker.
   - When `"aquifers"` is present, a Voronoi-cell noise-based aquifer searches 12 jittered 3D grid cells (`16 × 12 × 16` blocks), determines per-cell fluid surface levels and water-vs-lava types, blends pressure barriers across the three closest cells (using a shared lazy barrier-noise sample), and reports whether the resulting fluid voxel must be marked for post-processing so it flows once the chunk promotes to `full`.
2. **Ore veins (during the material-rule / surface pass)**: In 26.3, ore veins are **no longer** evaluated inside `NoiseChunk` during density fill and are no longer fields on `NoiseRouter`. Instead, ore veins are data-driven material rules of type `minecraft:ore_vein` evaluated inside `MaterialSystem` during `buildSurface` (and during single-block `topMaterial` fix-up in carver application). The built-in Overworld material rule places the Copper and Iron ore-vein rules after bedrock floor/roof and before surface and underground rules, driven by registered density functions under `minecraft:overworld/ore_vein/*`.
3. **Terrain-fill integration**: `NoiseBasedChunkGenerator.buildTerrain` constructs one `NoiseChunk` per chunk (holding the chunk `DensityVolume`, shared `DensitySamplerSet`, and `Aquifer`), runs density fill (`doFill`), then `buildSurface` (which executes material rules including `minecraft:ore_vein`), then `generateCarvers` (which reuses the same `NoiseChunk`'s `Aquifer` and caching samplers).

---

## Inputs and state

### Arithmetic and coordinate conventions
- Floating-point arithmetic is IEEE-754 binary32 (`f32`) and binary64 (`f64`), round-to-nearest-even, without fused multiply-add; chains associate left to right unless parenthesized.
- Integer arithmetic is two's-complement wrapping (`i32`, `i64`); `>>` is arithmetic shift and `>>>` is logical shift.
- Floor division `floorDiv(a, b)` and Euclidean modulo `floorMod(a, b)` follow Java `Math.floorDiv` / `Math.floorMod`; saturating floor `floor_i32(v)` follows [`java-math-helpers.md`](../math/java-math-helpers.md).
- `DensitySampler.sampleValue(x, y, z)` and `DensityBuffer.get(index)` produce **`f32`** values in 26.3; whenever formulas below compute in `f64`, the `f32` sample is widened to `f64` at the exact point noted.
- `QuartPos`: `fromBlock(b) = b >> 2`, `toBlock(q) = q << 2`.
- `SectionPos`: `sectionToBlockCoord(s) = s << 4`, `blockToSectionCoord(b) = b >> 4`, `sectionRelative(b) = b & 15`.
- `BlockPos.asLong(x, y, z)` and `ChunkPos.pack(x, z)` use the exact 64-bit bitfields defined in [`java-math-helpers.md`](../math/java-math-helpers.md).
- `DimensionType` vertical bounds:
  - `MIN_Y = -2032`, `MAX_Y = 2031`, `Y_SIZE = 4064`.
  - `WAY_BELOW_MIN_Y = MIN_Y << 4 = -32512`.
  - `WAY_ABOVE_MAX_Y = MAX_Y << 4 = 32496`.

### `NoiseGeneratorSettings` aquifer-relevant fields (`minecraft:worldgen/noise_settings`)
Full noise-settings and density-function schemas live in [`density-functions.md`](./density-functions.md). The fields directly consumed by aquifers and terrain fill are:

| JSON field | Type | Required | Notes |
|---|---|---|---|
| `noise` | `{ "min_y": i32, "height": i32 }` | yes | Both multiples of 16; `min_y` in `[-2032, 2031]`, `height` in `[0, 4064]`, `min_y + height <= 2032`. Clamped to chunk generation height accessor before fill. |
| `default_block` | `BlockState` | yes | Solid terrain block placed when `aquifer.computeSubstance` returns `null`. |
| `default_fluid` | `BlockState` | yes | Global sea-level fluid block. |
| `noise_router` | `NoiseRouter` | yes | Provides `final_density` (terrain fill) and `chunk_surface_level` (surface rules); in 26.3, `NoiseRouter` has **no** aquifer or ore-vein fields. |
| `material_rule` | `Holder<MaterialRule>` | yes | Registry ID or inline `MaterialRule` (contains `minecraft:ore_vein` rules in Overworld presets). |
| `sea_level` | `i32` | yes | Global fluid surface Y threshold. |
| `aquifers` | `Aquifer.Config` | no (optional) | If present, enables `NoiseBasedAquifer`; if absent/null, uses the disabled aquifer variant. |
| `legacy_random_source` | `bool` | yes | Selects `LEGACY` (48-bit LCG) vs `XOROSHIRO` (xoroshiro128++) for `RandomState`'s root positional random factory. |

### `Aquifer.Config` JSON schema
Every field inside `"aquifers"` is a required `DensityFunction` (ID string or inline density function; see [`density-functions.md`](./density-functions.md)):

| JSON field | Role |
|---|---|
| `barrier` | 3D noise controlling solid rock barriers between neighboring aquifers with different fluid levels. |
| `fluid_level_floodedness` | 3D noise deciding whether an aquifer cell is fully flooded, partially flooded (randomized level), or dry. |
| `fluid_level_spread` | 3D noise (sampled at coarse cell coordinates) offsetting randomized fluid surface heights. |
| `lava` | 3D noise (sampled at coarse cell coordinates) converting deep aquifers (`fluid_level <= -10`) into lava. |
| `exclusion` | 3D density function suppressing aquifer flooding wherever `value > 0.0` (Deep Dark exclusion in Overworld). |
| `surface_level` | 2D density function (sampled at `y = 0`) giving the unadjusted preliminary surface Y for each column. |

### `RandomState` seeding for aquifers and ore veins
From the dimension's `worldSeed` and `legacy_random_source`:
1. Root positional factory: `rootPositional = randomAlgorithm.newInstance(worldSeed).forkPositional()`.
2. Named sub-factory cache (`getOrCreateRandomFactory(id)`): `rootPositional.fromHashOf(id.toString()).forkPositional()`.
   - Aquifer positional factory ID: `"minecraft:aquifer"`.
   - Ore-vein positional factory ID: `"minecraft:ore"`.
   - Note: `fromHashOf` passes the canonical `"namespace:path"` string (see [`random.md`](../math/random.md)).

### `DensityVolume` layout
A `DensityVolume(sizeX, sizeY, sizeZ, minBlockX, minBlockY, minBlockZ, stepX, stepY, stepZ)` (default step `1, 1, 1`) maps grid index `(ix, iy, iz)` to:
- World block coordinates: `blockX(ix) = minBlockX + ix * stepX`, `blockY(iy) = minBlockY + iy * stepY`, `blockZ(iz) = minBlockZ + iz * stepZ`.
- Inclusive max block coordinates: `maxBlockX = minBlockX + sizeX * stepX - 1` (and identically for Y, Z).
- Flat buffer index (Z-major, X-middle, Y-inner): `index(ix, iy, iz) = iy + (ix + iz * sizeX) * sizeY`.
- Reverse lookup `indexOfBlock(bx, by, bz)`: returns `-1` if out of bounds or not aligned to `(stepX, stepY, stepZ)`, otherwise `index(floorDiv(bx - minBlockX, stepX), ...)`.

---

## Behavior (ordered rules)

### 1. Global fluid picker and disabled aquifer

#### 1.1 Global fluid picker (`NoiseBasedChunkGenerator.createFluidPicker`)
Constructed once per generator from `NoiseGeneratorSettings` (memoized). It holds three `FluidStatus(fluidLevel: i32, fluidType: BlockState)` records:
- `lavaStatus = FluidStatus(-54, minecraft:lava)` (default fluid block state, `level=0`).
- `seaStatus = FluidStatus(settings.sea_level, settings.default_fluid)`.
- `emptyStatus = FluidStatus(DimensionType.MIN_Y * 2, minecraft:air)` = `FluidStatus(-4064, minecraft:air)`.

Evaluating `FluidStatus.at(blockY)`:
- If `blockY < fluidLevel`: returns `fluidType`.
- Otherwise (`blockY >= fluidLevel`): returns `minecraft:air`.

Evaluating `globalFluidPicker.computeFluid(blockX, blockY, blockZ)` (X and Z are unused):
1. If debug flag `DEBUG_DISABLE_FLUID_GENERATION` is set (always `false` in normal gameplay): return `emptyStatus`.
2. Let `lavaCutoff = min(-54, settings.sea_level)`.
3. If `blockY < lavaCutoff`: return `lavaStatus`.
4. Otherwise: return `seaStatus`.

Consequences for built-in presets:
- **Overworld / Large Biomes / Amplified** (`sea_level = 63`, `default_fluid = water`): `y < -54` gets lava (`fluidLevel = -54`); `y >= -54` gets water below `y = 63` and air at `y >= 63`.
- **Nether** (`sea_level = 32`, `default_fluid = lava`, `min_y = 0`): `y >= 0` is never `< -54`, so `seaStatus` applies everywhere (`lava` for `y < 32`, `air` for `y >= 32`).
- **End** (`sea_level = 0`, `default_fluid = air`, `min_y = 0`): `seaStatus` produces `air` everywhere.
- **Caves** (`sea_level = 32`, `default_fluid = water`, `min_y = -64`): `y < -54` gets lava, `-54 <= y < 32` gets water, `y >= 32` gets air.
- **Floating Islands** (`sea_level = -64`, `default_fluid = water`, `min_y = 0`): `lavaCutoff = -64`, so all `y >= 0` get `seaStatus`, which yields `air` (`y >= -64`).

#### 1.2 Disabled aquifer (`Aquifer.createDisabled`)
Used whenever `NoiseGeneratorSettings.aquifers` is absent:
- `computeSubstance(blockX, blockY, blockZ, density: f64)`:
  - If `density > 0.0`: return `null` (solid terrain).
  - Otherwise (`density <= 0.0`): return `globalFluidPicker.computeFluid(blockX, blockY, blockZ).at(blockY)`.
- `shouldScheduleFluidUpdate()`: always returns `false`.

---

### 2. Noise-based aquifer (`Aquifer.NoiseBasedAquifer`)

Created when `NoiseGeneratorSettings.aquifers` is present, given `cachingSamplers`, `Aquifer.Config`, `positionalRandomFactory` (`minecraft:aquifer`), chunk `DensityVolume`, and `globalFluidPicker`.

#### 2.1 Grid coordinates and cache initialization
Grid coordinate conversion helpers:
- `gridX(bx) = bx >> 4` (spacing `16`), `fromGridX(gx, ox) = (gx << 4) + ox`.
- `gridY(by) = floorDiv(by, 12)` (spacing `12`), `fromGridY(gy, oy) = gy * 12 + oy`.
- `gridZ(bz) = bz >> 4` (spacing `16`), `fromGridZ(gz, oz) = (gz << 4) + oz`.

Constructor sequence:
1. Bind the 6 `Aquifer.Config` density functions against `cachingSamplers`: `barrier`, `fluid_level_floodedness`, `fluid_level_spread`, `lava`, `exclusion`, `surface_level`.
2. Compute grid cache bounds from `volume` using anchor offsets `(-5, +1, -5)` and cell search window `x1 ∈ [0, 1]`, `y1 ∈ [-1, 1]`, `z1 ∈ [0, 1]`:
   - `minGridX = gridX(volume.minBlockX - 5) + 0`
   - `maxGridX = gridX(volume.maxBlockX - 5) + 1`
   - `gridSizeX = maxGridX - minGridX + 1`
   - `minGridY = gridY(volume.minBlockY + 1) - 1`
   - `maxGridY = gridY(volume.maxBlockY + 1) + 1`
   - `gridSizeY = maxGridY - minGridY + 1`
   - `minGridZ = gridZ(volume.minBlockZ - 5) + 0`
   - `maxGridZ = gridZ(volume.maxBlockZ - 5) + 1`
   - `gridSizeZ = maxGridZ - minGridZ + 1`
   - `totalGridSize = gridSizeX * gridSizeY * gridSizeZ`
3. Allocate caches:
   - `aquiferCache`: `FluidStatus?[totalGridSize]`, initialized to `null`.
   - `aquiferLocationCache`: `i64[totalGridSize]`, filled with `i64::MAX` (`0x7FFF_FFFF_FFFF_FFFF`, sentinel for uncomputed).
   - `surfaceLevelCache`: `Map<i64, i32>` keyed by `ChunkPos.pack(quantizedX, quantizedZ)`.
   - Grid cache index formula:
     $$\text{index}(g_x, g_y, g_z) = ((g_y - \text{minGridY}) \cdot \text{gridSizeZ} + (g_z - \text{minGridZ})) \cdot \text{gridSizeX} + (g_x - \text{minGridX})$$
4. **Pre-populate `surfaceLevelCache` and compute `skipSamplingAboveY`**:
   - Compute horizontal block span covered by grid cells' jittered points:
     `minBx = fromGridX(minGridX, 0)`, `minBz = fromGridZ(minGridZ, 0)`, `maxBx = fromGridX(maxGridX, 9)`, `maxBz = fromGridZ(maxGridZ, 9)`.
   - Quantize to quart grid: `minQx = minBx >> 2`, `maxQx = maxBx >> 2`, `minQz = minBz >> 2`, `maxQz = maxBz >> 2`.
   - Construct 4-block-strided 2D volume:
     `DensityVolume(maxQx - minQx + 1, 1, maxQz - minQz + 1, minQx << 2, 0, minQz << 2, 4, 1, 4)`.
   - Evaluate `surface_level` over that volume (`sampleVolume`), loop `iz = 0 .. sizeZ - 1` (outer), `ix = 0 .. sizeX - 1` (inner):
     - `sl = floor_i32(buffer.get(index(ix, 0, iz)))` (floor of `f32`).
     - Store `surfaceLevelCache[ChunkPos.pack(vol.blockX(ix), vol.blockZ(iz))] = sl`.
     - Track `maxY = max(maxY, sl)` (starting from `i32::MIN`).
   - Compute:
     - `maxAdjustedSurfaceLevel = adjustSurfaceLevel(maxY) = maxY + 8`.
     - `skipSamplingAboveGridY = gridY(maxAdjustedSurfaceLevel + 12) + 1`.
     - `skipSamplingAboveY = fromGridY(skipSamplingAboveGridY, 11) - 1 = skipSamplingAboveGridY * 12 + 10`.

#### 2.2 Per-block `computeSubstance(blockX, blockY, blockZ, density: f64)`
Executed in exact order:

1. **Solid terrain fast path**:
   - If `density > 0.0`: set `shouldScheduleFluidUpdate = false` and return `null`.
2. **Global fluid lookup and upper sky fast path**:
   - `globalFluid = globalFluidPicker.computeFluid(blockX, blockY, blockZ)`.
   - If `blockY > skipSamplingAboveY`: set `shouldScheduleFluidUpdate = false` and return `globalFluid.at(blockY)`.
3. **Deep global lava fast path**:
   - If `globalFluid.at(blockY)` is `minecraft:lava`: set `shouldScheduleFluidUpdate = false` and return `minecraft:lava` (or `minecraft:air` if `DEBUG_DISABLE_FLUID_GENERATION`).
4. **Nearest-4 Voronoi cell search**:
   - Compute cell anchors:
     `xAnchor = gridX(blockX - 5)`, `yAnchor = gridY(blockY + 1)`, `zAnchor = gridZ(blockZ - 5)`.
   - Initialize `distanceSqr1 = distanceSqr2 = distanceSqr3 = distanceSqr4 = i32::MAX` and `closestIndex1 = closestIndex2 = closestIndex3 = closestIndex4 = 0`.
   - Iterate in exact nested order: `x1 ∈ {0, 1}` (outer), `y1 ∈ {-1, 0, 1}` (middle), `z1 ∈ {0, 1}` (inner) — 12 cells total:
     1. `gx = xAnchor + x1`, `gy = yAnchor + y1`, `gz = zAnchor + z1`.
     2. `index = getIndex(gx, gy, gz)`.
     3. Read `location = aquiferLocationCache[index]`. If `location == i64::MAX`:
        - `rng = positionalRandomFactory.at(gx, gy, gz)`.
        - Draw in exact order: `ox = rng.nextInt(10)`, `oy = rng.nextInt(9)`, `oz = rng.nextInt(10)`.
        - `location = BlockPos.asLong(fromGridX(gx, ox), fromGridY(gy, oy), fromGridZ(gz, oz))`.
        - Store `aquiferLocationCache[index] = location`.
     4. Unpack `(lx, ly, lz)` from `location` and compute 32-bit wrapping squared Euclidean distance:
        `dx = lx - blockX`, `dy = ly - blockY`, `dz = lz - blockZ`,
        `newDistance = dx * dx + dy * dy + dz * dz`.
     5. Insert `(newDistance, index)` into the top-4 list using **`>=`** comparisons (ties are broken in favor of **later** cells in `(x1, y1, z1)` loop order):
        - If `distanceSqr1 >= newDistance`: shift `3→4, 2→3, 1→2`, set `1 = (newDistance, index)`.
        - Else if `distanceSqr2 >= newDistance`: shift `3→4, 2→3`, set `2 = (newDistance, index)`.
        - Else if `distanceSqr3 >= newDistance`: shift `3→4`, set `3 = (newDistance, index)`.
        - Else if `distanceSqr4 >= newDistance`: set `4 = (newDistance, index)`.
5. **Closest cell status and similarity**:
   - Define in `f64`:
     $$\text{similarity}(d_1, d_2) = 1.0 - \frac{\text{f64}(d_2 -_{32} d_1)}{25.0}$$
     and constant:
     $$\text{FLOWING\_UPDATE\_SIMILARITY} = \text{similarity}(100, 144) = 1.0 - \frac{44.0}{25.0} = -0.76$$
   - Fetch `closestStatus1 = getAquiferStatus(closestIndex1)` (Section 2.4).
   - Compute `similarity12 = similarity(distanceSqr1, distanceSqr2)`.
   - `fluidState = closestStatus1.at(blockY)`.
   - (`actualFluidState` is `fluidState` unless `DEBUG_DISABLE_FLUID_GENERATION` overrides it to `minecraft:air`.)
6. **Non-overlapping cell branch (`similarity12 <= 0.0`, i.e., `distanceSqr2 - distanceSqr1 >= 25`)**:
   - If `similarity12 >= FLOWING_UPDATE_SIMILARITY` (i.e., `25 <= d2 - d1 <= 44`):
     - Fetch `closestStatus2 = getAquiferStatus(closestIndex2)`.
     - Set `shouldScheduleFluidUpdate = !closestStatus1.equals(closestStatus2)` (compares both `fluidLevel` and `fluidType`).
   - Else:
     - Set `shouldScheduleFluidUpdate = false`.
   - Return `actualFluidState`.
7. **Border / barrier branch (`similarity12 > 0.0`, i.e., `distanceSqr2 - distanceSqr1 < 25`)**:
   1. **Water-above-global-lava override**:
      - If `fluidState` is `minecraft:water` AND `globalFluidPicker.computeFluid(blockX, blockY - 1, blockZ).at(blockY - 1)` is `minecraft:lava`:
        - Set `shouldScheduleFluidUpdate = true` and return `actualFluidState` immediately (skips all barrier checks so water meets lava and triggers cobblestone/obsidian fluid interaction).
   2. Initialize `barrierNoiseCache = f64::NAN` (shared across up to three `calculatePressure` calls for this voxel).
   3. **Pair (1, 2) barrier**:
      - Fetch `closestStatus2 = getAquiferStatus(closestIndex2)`.
      - `barrier12 = similarity12 * calculatePressure(blockX, blockY, blockZ, &mut barrierNoiseCache, closestStatus1, closestStatus2)` (in `f64`).
      - If `density + barrier12 > 0.0`: set `shouldScheduleFluidUpdate = false` and return `null`.
   4. **Pair (1, 3) barrier**:
      - Fetch `closestStatus3 = getAquiferStatus(closestIndex3)` (note: fetched unconditionally before checking `similarity13 > 0.0`, populating `aquiferCache[closestIndex3]` even if `similarity13 <= 0.0`).
      - `similarity13 = similarity(distanceSqr1, distanceSqr3)`.
      - If `similarity13 > 0.0`:
        - `barrier13 = (similarity12 * similarity13) * calculatePressure(blockX, blockY, blockZ, &mut barrierNoiseCache, closestStatus1, closestStatus3)` (left-to-right `f64`).
        - If `density + barrier13 > 0.0`: set `shouldScheduleFluidUpdate = false` and return `null`.
   5. **Pair (2, 3) barrier**:
      - `similarity23 = similarity(distanceSqr2, distanceSqr3)`.
      - If `similarity23 > 0.0`:
        - `barrier23 = (similarity12 * similarity23) * calculatePressure(blockX, blockY, blockZ, &mut barrierNoiseCache, closestStatus2, closestStatus3)` (left-to-right `f64`).
        - If `density + barrier23 > 0.0`: set `shouldScheduleFluidUpdate = false` and return `null`.
   6. **Fluid-update flag decision**:
      - `mayFlow12 = !closestStatus1.equals(closestStatus2)`
      - `mayFlow23 = similarity23 >= FLOWING_UPDATE_SIMILARITY && !closestStatus2.equals(closestStatus3)`
      - `mayFlow13 = similarity13 >= FLOWING_UPDATE_SIMILARITY && !closestStatus1.equals(closestStatus3)`
      - If `mayFlow12 || mayFlow23 || mayFlow13`:
        - `shouldScheduleFluidUpdate = true`.
      - Else:
        - `shouldScheduleFluidUpdate = (similarity13 >= FLOWING_UPDATE_SIMILARITY) && (similarity(distanceSqr1, distanceSqr4) >= FLOWING_UPDATE_SIMILARITY) && !closestStatus1.equals(getAquiferStatus(closestIndex4))` (short-circuited left to right).
   7. Return `actualFluidState`.

#### 2.3 Barrier pressure calculation (`calculatePressure`)
Given `(blockX, blockY, blockZ, &mut barrierNoiseCache, status1, status2)`:
1. Let `type1 = status1.at(blockY)`, `type2 = status2.at(blockY)`.
2. **Water/lava boundary**: if (`type1` is `lava` and `type2` is `water`) OR (`type1` is `water` and `type2` is `lava`):
   - Return `2.0` (`f64`).
3. **Equal fluid level**: let `fluidYDiff = abs(status1.fluidLevel - status2.fluidLevel)`.
   - If `fluidYDiff == 0`: return `0.0`.
4. **Piecewise linear vertical gradient** (all `f64`):
   - `averageFluidY = 0.5 * f64(status1.fluidLevel + status2.fluidLevel)` (note: `i32` addition before widening to `f64`).
   - `howFarAboveAverage = (f64(blockY) + 0.5) - averageFluidY`.
   - `baseValue = f64(fluidYDiff) / 2.0`.
   - `distanceFromEdge = baseValue - abs(howFarAboveAverage)`.
   - If `howFarAboveAverage > 0.0` (upper half of the barrier):
     - `centerPoint = 0.0 + distanceFromEdge`
     - `gradient = (centerPoint > 0.0) ? (centerPoint / 1.5) : (centerPoint / 2.5)`
   - Else (`howFarAboveAverage <= 0.0`, lower half of the barrier):
     - `centerPoint = 3.0 + distanceFromEdge`
     - `gradient = (centerPoint > 0.0) ? (centerPoint / 3.0) : (centerPoint / 10.0)`
5. **Lazy barrier noise**:
   - If `!(gradient < -2.0) && !(gradient > 2.0)` (i.e., `-2.0 <= gradient <= 2.0`):
     - If `barrierNoiseCache.is_nan()`:
       - Sample `v = f64(barrierNoise.sampleValue(blockX, blockY, blockZ))` and store in `barrierNoiseCache`.
     - `noiseValue = barrierNoiseCache`.
   - Else:
     - `noiseValue = 0.0` (does **not** sample `barrierNoise`).
6. Return `2.0 * (noiseValue + gradient)` in `f64`.

#### 2.4 Aquifer cell fluid status (`getAquiferStatus` and `computeFluid`)
`getAquiferStatus(index)` returns `aquiferCache[index]` if non-null; otherwise unpacks `(x, y, z)` from `aquiferLocationCache[index]`, computes `computeFluid(x, y, z)`, caches it in `aquiferCache[index]`, and returns it.

Helper `surfaceLevel(bx, bz)`:
- `qx = (bx >> 2) << 2`, `qz = (bz >> 2) << 2`.
- Look up `ChunkPos.pack(qx, qz)` in `surfaceLevelCache`. On cache miss, evaluate `floor_i32(surfaceLevel.sampleValue(qx, 0, qz))` (point evaluation, `f32` floor), insert into `surfaceLevelCache`, and return it. **[oracle]**

Helper `adjustSurfaceLevel(preliminarySurfaceLevel) = preliminarySurfaceLevel + 8`.

`computeFluid(x, y, z)`:
1. `globalFluid = globalFluidPicker.computeFluid(x, y, z)`.
2. Initialize `lowestPreliminarySurface = i32::MAX`, `topOfCell = y + 12`, `bottomOfCell = y - 12`, `surfaceAtCenterUnderGlobalFluid = false`.
3. Iterate the 13 chunk-offset pairs `(ox, oz)` in `SURFACE_SAMPLING_OFFSETS_IN_CHUNKS` in exact order:
   `(0, 0), (-2, -1), (-1, -1), (0, -1), (1, -1), (-3, 0), (-2, 0), (-1, 0), (1, 0), (-2, 1), (-1, 1), (0, 1), (1, 1)`:
   1. `sampleX = x + (ox << 4)`, `sampleZ = z + (oz << 4)`.
   2. `sl = surfaceLevel(sampleX, sampleZ)`.
   3. `adjustedSl = sl + 8`.
   4. `isStart = (ox == 0 && oz == 0)` (first iteration only).
   5. If `isStart && bottomOfCell > adjustedSl`: return `globalFluid` immediately.
   6. `topPokesAbove = (topOfCell > adjustedSl)`.
   7. If `topPokesAbove || isStart`:
      - `globalFluidAtSurface = globalFluidPicker.computeFluid(sampleX, adjustedSl, sampleZ)`.
      - If `!globalFluidAtSurface.at(adjustedSl).isAir()`:
        - If `isStart`: set `surfaceAtCenterUnderGlobalFluid = true`.
        - If `topPokesAbove`: return `globalFluidAtSurface` immediately.
   8. `lowestPreliminarySurface = min(lowestPreliminarySurface, sl)` (unadjusted `sl`).
4. Compute fluid surface level (`computeSurfaceLevel`):
   - Check exclusion: if `f64(exclusion.sampleValue(x, y, z)) > 0.0`:
     - `partiallyFloodedness = -1.0`, `fullyFloodedness = -1.0`.
   - Else:
     - `distanceBelowSurface = (lowestPreliminarySurface + 8) - y`.
     - `floodednessFactor = surfaceAtCenterUnderGlobalFluid ? Mth.clampedMap(f64(distanceBelowSurface), 0.0, 64.0, 1.0, 0.0) : 0.0`.
     - `floodednessNoiseVal = Mth.clamp(f64(fluidLevelFloodednessNoise.sampleValue(x, y, z)), -1.0, 1.0)`.
     - `fullyFloodedThreshold = Mth.map(floodednessFactor, 1.0, 0.0, -0.3, 0.8)`.
     - `partiallyFloodedThreshold = Mth.map(floodednessFactor, 1.0, 0.0, -0.8, 0.4)`.
     - `partiallyFloodedness = floodednessNoiseVal - partiallyFloodedThreshold`.
     - `fullyFloodedness = floodednessNoiseVal - fullyFloodedThreshold`.
   - Select `fluidSurfaceLevel`:
     - If `fullyFloodedness > 0.0`: `fluidSurfaceLevel = globalFluid.fluidLevel`.
     - Else if `partiallyFloodedness > 0.0`: `fluidSurfaceLevel = computeRandomizedFluidSurfaceLevel(x, y, z, lowestPreliminarySurface)`:
       1. `cellX = floorDiv(x, 16)`, `cellY = floorDiv(y, 40)`, `cellZ = floorDiv(z, 16)`.
       2. `cellMiddleY = cellY * 40 + 20`.
       3. `spread = f64(fluidLevelSpreadNoise.sampleValue(cellX, cellY, cellZ) * 10.0_f32)` — **multiplied by `10.0f` in `f32` before widening to `f64`**.
       4. `spreadQuantized = Mth.quantize(spread, 3) = floor_i32(spread / 3.0) * 3`.
       5. `fluidSurfaceLevel = min(lowestPreliminarySurface, cellMiddleY + spreadQuantized)`.
     - Else: `fluidSurfaceLevel = DimensionType.WAY_BELOW_MIN_Y` (`-32512`).
5. Compute fluid type (`computeFluidType`):
   - Start with `fluidType = globalFluid.fluidType`.
   - If `fluidSurfaceLevel <= -10 && fluidSurfaceLevel != DimensionType.WAY_BELOW_MIN_Y && fluidType != minecraft:lava`:
     - `typeCellX = floorDiv(x, 64)`, `typeCellY = floorDiv(y, 40)`, `typeCellZ = floorDiv(z, 64)`.
     - `lavaVal = f64(lavaNoise.sampleValue(typeCellX, typeCellY, typeCellZ))`.
     - If `abs(lavaVal) > 0.3` (`0.3` in `f64`): `fluidType = minecraft:lava`.
6. Return `FluidStatus(fluidSurfaceLevel, fluidType)`.

#### 2.5 Effect of `shouldScheduleFluidUpdate` on the chunk
`Aquifer` itself never mutates chunk state. Instead, its callers (`NoiseBasedChunkGenerator.doFill` and `NoiseBasedChunkGenerator.applyCarvingMask`) check:
- `if aquifer.shouldScheduleFluidUpdate() && !state.getFluidState().isEmpty()`:
  - Call `chunk.markPosForPostProcessing(blockPos)`.
- In `ProtoChunk.markPosForPostProcessing(pos)`:
  - If `pos.y` is inside build height (`minY <= y <= maxY`), packs the section-relative offset into a 16-bit integer:
    $$\text{packed} = (x \mathbin{\&} 15) \mid ((y \mathbin{\&} 15) \ll 4) \mid ((z \mathbin{\&} 15) \ll 8)$$
    and appends it to `postProcessing[chunk.getSectionIndex(y)]` (a `ShortList`, **not** deduplicated and **not** inserted into `ProtoChunk`'s `fluidTicks`).
- Later, when the chunk is promoted to `FULL` and `LevelChunk.postProcessGeneration(serverLevel)` executes (invoked from `ChunkMap`), every packed coordinate in `postProcessing` is unpacked in section-index order and list insertion order:
  - If the current block at `blockPos` has a non-empty `FluidState`, `fluidState.tick(level, blockPos, blockState)` is invoked (which schedules flowing fluid updates).
  - If the block is a `LiquidBlock`, `blockState.tick(level, blockPos, level.getRandom())` is invoked; otherwise `Block.updateFromNeighbourShapes` is applied.

---

### 3. Ore veins (`minecraft:ore_vein` material rule)

In 26.3, ore veins are **not** part of `NoiseChunk` or `NoiseRouter`. They are a `MaterialRule` variant (`OreVeinRule`, type ID `minecraft:ore_vein`) evaluated during the material/surface pass (`MaterialSystem.buildSurface`, and also when `MaterialSystem.topMaterial` runs during carver grass fix-up).

#### 3.1 Data-pack JSON schema (`minecraft:ore_vein`)
All seven fields are required:

| JSON field | Type / range | Role |
|---|---|---|
| `ore_block` | `BlockState` | Regular ore block (e.g. `minecraft:copper_ore`, `minecraft:deepslate_iron_ore`). |
| `raw_ore_block` | `BlockState` | Rare raw-ore block (e.g. `minecraft:raw_copper_block`, `minecraft:raw_iron_block`). |
| `filler_block` | `BlockState` | Vein rock filler (e.g. `minecraft:granite`, `minecraft:tuff`). |
| `raw_ore_chance` | `f32` in `[0.0, 1.0]` | Probability that a chosen ore block becomes `raw_ore_block` instead of `ore_block`. |
| `density` | `DensityFunction` | Controls vein presence and solidness probability (`<= 0.0f` means outside vein). |
| `richness` | `DensityFunction` | Controls ore-vs-filler probability inside the vein. |
| `filler_gap` | `DensityFunction` | Controls gap suppression of ore (`< 0.0f` allows ore; `>= 0.0f` forces filler). |

#### 3.2 Compilation (`OreVeinRule.compile(MaterialRuleContext)`)
When compiled for a `MaterialRuleContext`:
1. If debug flag `DEBUG_DISABLE_ORE_VEINS` is enabled: returns a no-op evaluator returning `null`.
2. Binds three `DensityGetter` samplers from `context.getDensitiesInChunk(fn, prefill)`:
   - `densitySampler`: **`prefill = true`** (samples `density` into an unpooled `DensityBuffer` covering `context.expectedVolume` once at compile time; falls back to point `sampleValue` only if `(blockX, blockY, blockZ)` is outside `expectedVolume`).
   - `richnessSampler`: **`prefill = true`** (same prefill behavior over `expectedVolume`).
   - `fillerGapSampler`: **`prefill = false`** (always evaluates `sampleValue(blockX, blockY, blockZ)` lazily per block).
3. Acquires `randomFactory = context.getOrCreateRandomFactory("minecraft:ore")` (`rootPositional.fromHashOf("minecraft:ore").forkPositional()`).
4. (Debug flag `DEBUG_ORE_VEINS`, always `false` normally, substitutes `minecraft:air` for `null` and `minecraft:oak_button` for `filler_block`.)

#### 3.3 Per-block evaluation (`RuleEvaluator.tryApply(blockX, blockY, blockZ)`)
Evaluated on solid (non-air, non-fluid) blocks in `MaterialSystem` in exact order:
1. Evaluate `d = densitySampler.get()` (`f32`).
2. If `d <= 0.0_f`: return `null` (no RNG created or drawn).
3. Create positional RNG: `rng = randomFactory.at(blockX, blockY, blockZ)`.
4. **RNG draw 1**: draw `r1 = rng.nextFloat()`. If `r1 > d`: return `null`.
5. Evaluate `richness = richnessSampler.get()` (`f32`).
6. **RNG draw 2**: draw `r2 = rng.nextFloat()`.
   - Evaluate `r2 < richness && fillerGapSampler.get() < 0.0_f` (short-circuit `&&`: `fillerGapSampler` is evaluated **only** when `r2 < richness`).
   - If **both** are true:
     - **RNG draw 3**: draw `r3 = rng.nextFloat()`.
     - If `r3 < raw_ore_chance`: return `raw_ore_block`.
     - Else: return `ore_block`.
   - Else:
     - Return `filler_block`.

Notice the exact RNG draw count per solid block inside a vein (`d > 0.0f`):
- Outside probability `d`: 1 `nextFloat()`.
- Inside vein, filler chosen: 2 `nextFloat()`.
- Inside vein, ore chosen: 3 `nextFloat()`.

#### 3.4 Built-in Overworld ore vein definitions (26.3)
In vanilla 26.3, `OverworldMaterialRules` registers two `minecraft:ore_vein` rules in sequence order (`copper_ore_vein` first, `iron_ore_vein` second) for `overworld`, `overworld_caves`, and `overworld_floating_islands`:

| Vein type | Material rule ID | `ore_block` | `raw_ore_block` | `filler_block` | `raw_ore_chance` | Y range `[minY, maxY]` | Toggle sign |
|---|---|---|---|---|---|---|---|
| Copper | `minecraft:overworld/copper_ore_vein` | `minecraft:copper_ore` | `minecraft:raw_copper_block` | `minecraft:granite` | `0.02_f` | `[0, 50]` | positive (`+toggle`) |
| Iron | `minecraft:overworld/iron_ore_vein` | `minecraft:deepslate_iron_ore` | `minecraft:raw_iron_block` | `minecraft:tuff` | `0.02_f` | `[-60, -8]` | negative (`-toggle`) |

The density functions registered under `minecraft:overworld/ore_vein/*` that feed these two rules are constructed as follows (all interpolated functions use cell size `4 × 8` in XZ × Y, and span `interpolatedVeinMinY = floorDiv(-60, 8) * 8 = -64` through `interpolatedVeinMaxY = (floorDiv(50, 8) + 1) * 8 = 56`, with `rangeChoice(y, minY, maxY + 1, ...)` i.e. `[-64, 57)`):
- `overworld/ore_vein/toggle`: `cache(interpolated(rangeChoice(y, -64, 57, noise(ore_veininess, 1.5, 1.5), 0), 4, 8))`.
- `overworld/ore_vein/richness`: `clampedMap(abs(toggle), 0.4_f, 0.6_f, 0.1_f, 0.3_f)`.
- `overworld/ore_vein/mask`: `cache(rangeChoice(toggle, -0.4_f, 0.4_f, -1.0_f, 0.08_f - max(veinA, veinB)))`, where:
  - `veinA = abs(interpolated(rangeChoice(y, -64, 57, noise(ore_vein_a, 4.0, 4.0), 1), 4, 8))`
  - `veinB = abs(interpolated(rangeChoice(y, -64, 57, noise(ore_vein_b, 4.0, 4.0), 1), 4, 8))`
- `overworld/ore_vein/copper_density` / `iron_density` (for each vein type with `[minY, maxY]`):
  - `distanceFromEdge = min(maxY - y, y - minY)`
  - `edgeRoundoff = clampedMap(distanceFromEdge, 0.0_f, 20.0_f, -0.2_f, 0.0_f)`
  - `veininess = toggle` (copper) or `-toggle` (iron)
  - Nested `rangeChoice`: if `y ∈ [minY, maxY)` (note `rangeChoice` upper bound is exclusive — see [`density-functions.md`](./density-functions.md)!) AND `mask ∈ [0.0_f, 1000000.0_f)` AND `veininess - 0.4_f + edgeRoundoff ∈ [0.0_f, 1000000.0_f)`, yields `0.7_f` (`VEIN_SOLIDNESS`); otherwise `-1.0_f`.
- `overworld/ore_vein/gap`: `-0.3_f - noise(ore_gap, 1.0, 1.0)` (so `gap < 0.0_f` iff `noise(ore_gap) > -0.3_f`).

---

### 4. Combining density, aquifers, and ore veins in `NoiseChunk` and `NoiseBasedChunkGenerator`

#### 4.1 `NoiseChunk` construction
For a chunk and `NoiseSettings` (clamped to `chunk.getHeightAccessorForGeneration()`):
1. `volume = DensityVolume(16, noiseSettings.height, 16, chunkPos.minBlockX, noiseSettings.minY, chunkPos.minBlockZ)` (step `1, 1, 1`).
2. `beardifier = Beardifier.forStructuresInChunk(structureManager, chunkPos)`.
3. Build `SamplerContext` with `Beardifier`, optional `Blender` (`alpha`/`offset` buffers if `!blender.isEmpty()`), a pooled `DensityBufferPool`, and caches enabled (`enableCaches()`).
4. Create `cachingSamplers = randomState.samplersWithContext(samplerContext)`.
5. Create `aquifer`:
   - If `settings.aquifers` is present: `Aquifer.NoiseBasedAquifer(cachingSamplers, config, randomState.getOrCreateRandomFactory("minecraft:aquifer"), volume, globalFluidPicker)`.
   - Else: `Aquifer.createDisabled(globalFluidPicker)`.

#### 4.2 Exact call order inside `NoiseBasedChunkGenerator.buildTerrain`
If `noiseSettings.height <= 0` (after clamping) or `debugVoidTerrain(chunkPos)`, `buildTerrain` returns immediately without modifying the chunk. Otherwise, using a single `NoiseChunk` instance:

1. **Step 1 — Density fill (`doFill`)**:
   - Acquires chunk sections from `topSectionIndex = chunk.getSectionIndex(volume.maxBlockY)` down to `bottomSectionIndex = chunk.getSectionIndex(volume.minBlockY)`.
   - Fetches unprimed `OCEAN_FLOOR_WG` and `WORLD_SURFACE_WG` heightmaps.
   - Evaluates `settings.noiseRouter.finalDensity` over `volume` in one batch (`sampleVolume(volume)`).
   - Iterates voxels in exact order: **`z = 0..15` (outer), `x = 0..15` (middle), `y = sizeY - 1` down to `0` (inner, top-down)**:
     1. `density = densityBuffer.get(index(x, y, z))` (`f32`, widened to `f64`).
     2. `state = aquifer.computeSubstance(blockX, blockY, blockZ, f64(density))`.
     3. If `state == null`: `state = settings.default_block`.
     4. If `state != minecraft:air` (default state identity check):
        - Write directly into section (`section.setBlockState(x, blockY & 15, z, state, false)`).
        - Update `OCEAN_FLOOR_WG` and `WORLD_SURFACE_WG`.
        - If `aquifer.shouldScheduleFluidUpdate() && !state.getFluidState().isEmpty()`: call `chunk.markPosForPostProcessing(blockPos)`.
   - Releases chunk sections.
   - **Ore veins do NOT run in `doFill`.**

2. **Step 2 — Surface / material rules (`buildSurface`)**:
   - Invokes `randomState.surfaceSystem().buildSurface(...)` with `settings.material_rule`, passing the same `noiseChunk` (sharing its `cachingSamplers` and `volume`).
   - In Overworld presets, the `material_rule` sequence runs in exact order for each solid (non-air, non-fluid) block:
     1. `bedrock_roof` (only in `overworld_caves`)
     2. `bedrock_floor` (in `overworld` and `overworld_caves`)
     3. **`overworld/copper_ore_vein`**
     4. **`overworld/iron_ore_vein`**
     5. `overworld/surface` (gated by `above_preliminary_surface` in `overworld`)
     6. `overworld/underground` (sulfur cave bands, deepslate gradient)
   - Full `MaterialSystem` rules and column-walk algorithm are specified in [`surface-and-carvers.md`](./surface-and-carvers.md).

3. **Step 3 — Carvers (`generateCarvers` → `applyCarvingMask`)**:
   - Runs all carvers in the `17 × 17` chunk neighborhood into a `CarvingMask`, then applies the mask top-down per contiguous carved run using the **same** `noiseChunk.aquifer()` (`computeSubstance(worldX, worldY, worldZ, 0.0)`) and `surfaceSystem.topMaterial(...)` (which re-evaluates `material_rule` including ore veins on exposed dirt below grass/mycelium). See [`surface-and-carvers.md`](./surface-and-carvers.md).

#### 4.3 Column queries (`getBaseHeight` and `getBaseColumn`)
`NoiseBasedChunkGenerator.iterateNoiseColumn` (used by `getBaseHeight` and `getBaseColumn`, e.g. for structure placement) builds a `1 × noiseSettings.height × 1` `NoiseChunk` with `beardifier = null` and `Blender.empty()`, samples `final_density` over that column, walks top-down `y = sizeY - 1 .. 0`, and evaluates `aquifer.computeSubstance(blockX, blockY, blockZ, f64(density))` with `null -> default_block`. It does **not** run `buildSurface`, ore veins, or carvers. For `getBaseHeight`, the first voxel matching `heightmapType.isOpaque()` returns `blockY + 1` (falling back to `heightAccessor.getMinY()`).

---

## Edge cases and known quirks

1. **26.3 architectural shift — ore veins live in `MaterialRule`, not `NoiseChunk`**: In 26.3, `NoiseRouter` has no `vein_toggle`, `vein_ridged`, or `vein_gap` fields, and `doFill` never places ore veins. Because `OreVeinRule` runs in `buildSurface`, ore veins only replace solid non-air non-fluid blocks (and can be overwritten by bedrock, which precedes them, while taking precedence over surface soil and deepslate, which follow them).
2. **`fluid_level_spread` multiplication in `f32`**: `fluidLevelSpreadNoise.sampleValue(...) * 10.0F` multiplies in `f32` before widening to `f64` for `Mth.quantize(..., 3)`. Performing `* 10.0` in `f64` can flip `floor(spread / 3.0)` near multiples of 3.
3. **Nearest-4 Voronoi tie-breaking favors later cells**: Because the insertion chain uses `>=`, when two grid cells have identical squared distance `newDistance`, the cell visited **later** in `(x1 ∈ 0..1, y1 ∈ -1..1, z1 ∈ 0..1)` order takes the closer slot.
4. **32-bit wrapping in Voronoi distance**: `dx * dx + dy * dy + dz * dz` is evaluated in `i32` (never overflows in normal worlds since `|dx|, |dy|, |dz| < 32`).
5. **Unconditional `closestStatus3` evaluation in border branch**: In `computeSubstance`, once `density + barrier12 <= 0.0`, `getAquiferStatus(closestIndex3)` is called **before** checking `similarity13 > 0.0`, which populates `aquiferCache[closestIndex3]` even if no `(1, 3)` barrier check runs. (This has no RNG side effect because `computeFluid` only evaluates pure density functions and caches, but affects probe caches.)
6. **13 asymmetric `SURFACE_SAMPLING_OFFSETS_IN_CHUNKS`**: The offset list includes `(-3, 0)` and omits `(2, 0)` and `(-2, -1)`'s mirror `(2, 1)`; `(-3, 0)` probes 48 blocks west of the cell point, outside the constructor's `maxSurfaceLevel` pre-fill box, triggering a `surfaceLevelCache` miss and point evaluation.
7. **`surfaceLevelCache` volume vs. point evaluation**: The constructor pre-fills `surfaceLevelCache` via `surfaceLevel.sampleVolume` (step 4), while misses outside that rectangle call `surfaceLevel.sampleValue` (point path). See [`noise.md`](../math/noise.md) Edge Case 14 for the 1-ULP volume-vs-point associativity rule. **[oracle]**
8. **`i32` sum in `calculatePressure`'s `averageFluidY`**: `0.5 * (status1.fluidLevel + status2.fluidLevel)` sums the two `i32` levels in `i32` before widening to `f64`. Note that if one status is dry (`WAY_BELOW_MIN_Y = -32512`), `fluidLevel` is still well within `i32` range (no overflow, but `fluidYDiff` is ~32500, making `distanceFromEdge` huge and `gradient > 2.0`, so `barrierNoise` is skipped and `calculatePressure` returns `2.0 * gradient`).
9. **Water-over-lava short-circuit skips barriers**: In the `similarity12 > 0.0` branch, if `closestStatus1.at(blockY)` is `minecraft:water` and the global fluid at `blockY - 1` is `minecraft:lava`, `computeSubstance` immediately returns water with `shouldScheduleFluidUpdate = true` without checking any barriers.
10. **Lava check uses `globalFluid.fluidType != Blocks.LAVA.defaultBlockState()`**: In `computeFluidType`, a deep aquifer (`fluidSurfaceLevel <= -10`) only samples `lavaNoise` if `fluidSurfaceLevel != -32512` and `globalFluid.fluidType` is not already `minecraft:lava`; the threshold check `abs(lavaVal) > 0.3` uses `f64` `0.3` (`0x3FD3333333333333`).
11. **`FluidStatus.equals` compares raw `(fluidLevel, fluidType)`**: `shouldScheduleFluidUpdate` compares `FluidStatus` records directly rather than their `at(blockY)` outputs; two aquifers with different levels that both resolve to water at `blockY` still trigger `shouldScheduleFluidUpdate = true`.
12. **`OreVeinRule` `filler_gap` short-circuit**: In `OreVeinRule`, `fillerGapSampler.get()` is only called when `r2 < richness`, and `OreVeinRule` uses `nextFloat() > density` (strictly greater exits, so `r1 == density` enters the vein) while `r2 < richness` and `r3 < raw_ore_chance` use strict `<`.
13. **`OreVeinRule` Y-range upper bound in vanilla density functions**: `NoiseRouterData.createOreVeinDensity` uses `rangeChoice(y, type.minY, type.maxY, ...)`. Because `rangeChoice` is half-open `[min_inclusive, max_exclusive)`, copper veins cover `y ∈ [0, 49]` (`y = 50` excluded) and iron veins cover `y ∈ [-60, -9]` (`y = -8` excluded).
14. **Post-processing list is not deduplicated**: `ProtoChunk.markPosForPostProcessing` appends to a `ShortList` per section; if both `doFill` and `applyCarvingMask` (or `buildSurface`) mark the same block, it appears twice in `postProcessing`.
15. **`NoiseBasedAquifer` `skipSamplingAboveY` formula**: `skipSamplingAboveGridY = floorDiv(maxSurfaceLevel + 8 + 12, 12) + 1`, and `skipSamplingAboveY = skipSamplingAboveGridY * 12 + 11 - 1 = skipSamplingAboveGridY * 12 + 10`. Blocks at `blockY == skipSamplingAboveY` still run the full aquifer search; only `blockY > skipSamplingAboveY` short-circuit.

---

## Constants and IDs

### Aquifer constants
| Constant | Type | Value | Notes |
|---|---|---|---|
| `X_SPACING`, `Z_SPACING` | `i32` | `16` (`1 << 4`) | Horizontal aquifer grid cell width |
| `Y_SPACING` | `i32` | `12` | Vertical aquifer grid cell height |
| `X_RANGE`, `Z_RANGE` | `i32` | `10` (`nextInt(10)` → `0..=9`) | Horizontal cell jitter range (`X_SEPARATION = 6`) |
| `Y_RANGE` | `i32` | `9` (`nextInt(9)` → `0..=8`) | Vertical cell jitter range (`Y_SEPARATION = 3`) |
| `SAMPLE_OFFSET_X`, `SAMPLE_OFFSET_Z` | `i32` | `-5` | Block-to-grid anchor offset in X and Z |
| `SAMPLE_OFFSET_Y` | `i32` | `+1` | Block-to-grid anchor offset in Y |
| Cell search bounds | `i32` | `x1 ∈ [0, 1]`, `y1 ∈ [-1, 1]`, `z1 ∈ [0, 1]` | 12 candidate cells per block |
| Similarity divisor | `f64` | `25.0` | `similarity(d1, d2) = 1.0 - (d2 - d1) / 25.0` |
| `FLOWING_UPDATE_SIMILARITY` | `f64` | `-0.76` (`0xBFE851EB851EB852`) | `similarity(10^2, 12^2) = 1.0 - 44.0 / 25.0` |
| Global lava level | `i32` | `-54` | `lavaCutoff = min(-54, sea_level)` |
| Surface level adjustment | `i32` | `+8` | `adjustSurfaceLevel(sl) = sl + 8` |
| Floodedness max depth | `f64` | `64.0` | `clampedMap(dist, 0.0, 64.0, 1.0, 0.0)` |
| Fully / partially flooded thresholds | `f64` | `[-0.3, 0.8]` / `[-0.8, 0.4]` | Mapped from `floodednessFactor` in `[1.0, 0.0]` |
| Dry fluid level sentinel | `i32` | `-32512` | `DimensionType.WAY_BELOW_MIN_Y = -2032 << 4` |
| Fluid spread cell size (X, Y, Z) | `i32` | `(16, 40, 16)` | Middle Y offset `+20`, spread `* 10.0f`, quantize `3` |
| Lava noise max surface level | `i32` | `-10` | `fluidSurfaceLevel <= -10` |
| Lava type cell size (X, Y, Z) | `i32` | `(64, 40, 64)` | Threshold `abs(lavaNoise) > 0.3` (`f64`) |
| Pressure upper half bias / divisors | `f64` | bias `0.0`, holes `1.5`, rocks `2.5` | `howFarAboveAverage > 0.0` |
| Pressure lower half bias / divisors | `f64` | bias `3.0`, holes `3.0`, rocks `10.0` | `howFarAboveAverage <= 0.0` |
| Pressure noise clamp & amplitude | `f64` | `[-2.0, 2.0]`, amplitude `2.0` | Water/lava pair returns `2.0` directly |
| Location cache empty sentinel | `i64` | `Long.MAX_VALUE` (`0x7FFF_FFFF_FFFF_FFFF`) | Initial fill of `aquiferLocationCache` |
| Aquifer RNG factory ID | `Identifier` | `minecraft:aquifer` | Forked via `getOrCreateRandomFactory` |

### `SURFACE_SAMPLING_OFFSETS_IN_CHUNKS` (exact order, 13 entries)
`(0, 0)`, `(-2, -1)`, `(-1, -1)`, `(0, -1)`, `(1, -1)`, `(-3, 0)`, `(-2, 0)`, `(-1, 0)`, `(1, 0)`, `(-2, 1)`, `(-1, 1)`, `(0, 1)`, `(1, 1)`.

### Built-in Overworld aquifer `Aquifer.Config` definitions
Used by `minecraft:overworld`, `minecraft:large_biomes`, and `minecraft:amplified` (absent in `nether`, `end`, `caves`, `floating_islands`):

| `Aquifer.Config` field | Built-in density function | Noise parameters (`NoiseData`) |
|---|---|---|
| `barrier` | `noise(minecraft:aquifer_barrier, xz=1.0, y=0.5)` | `firstOctave = -3`, `amplitudes = [1.0]` |
| `fluid_level_floodedness` | `noise(minecraft:aquifer_fluid_level_floodedness, xz=1.0, y=0.67)` | `firstOctave = -7`, `amplitudes = [1.0]` |
| `fluid_level_spread` | `noise(minecraft:aquifer_fluid_level_spread, xz=1.0, y=0.7142857142857143)` (`5/7`) | `firstOctave = -5`, `amplitudes = [1.0]` |
| `lava` | `noise(minecraft:aquifer_lava, xz=1.0, y=1.0)` | `firstOctave = -1`, `amplitudes = [1.0]` |
| `exclusion` | `min(-0.225_f - erosion, max(depth - 0.9_f, 0.0_f))` (`OverworldBiomeBuilder.deepDarkRegion`) | Uses preset's `erosion` & `depth` |
| `surface_level` | Preset's `preliminary_surface_level` (`overworld/`, `overworld_large_biomes/`, or `overworld_amplified/`) | — |

### Ore vein constants and IDs
| Constant / ID | Value |
|---|---|
| Material rule type ID | `minecraft:ore_vein` (in `minecraft:worldgen/material_rule_type`) |
| Ore RNG factory ID | `minecraft:ore` |
| `VEININESS_THRESHOLD` | `0.4_f` |
| `EDGE_ROUNDOFF_BEGIN` | `20` |
| `MAX_EDGE_ROUNDOFF` | `0.2_f` (mapped to `-0.2_f .. 0.0_f`) |
| `VEIN_SOLIDNESS` | `0.7_f` |
| `MIN_RICHNESS` / `MAX_RICHNESS` | `0.1_f` / `0.3_f` |
| `MAX_RICHNESS_THRESHOLD` | `0.6_f` |
| `CHANCE_OF_RAW_ORE_BLOCK` | `0.02_f` |
| `SKIP_ORE_IF_GAP_NOISE_IS_BELOW` | `-0.3_f` |
| `ORE_THICKNESS` / `VEININESS_FREQUENCY` / ridge frequency | `0.08_f` / `1.5` / `4.0` |
| Ore noises (`NoiseData`) | `minecraft:ore_veininess` (`-8, [1.0]`), `minecraft:ore_vein_a` (`-7, [1.0]`), `minecraft:ore_vein_b` (`-7, [1.0]`), `minecraft:ore_gap` (`-5, [1.0]`) |
| Built-in density function IDs | `minecraft:overworld/ore_vein/{toggle,richness,mask,copper_density,iron_density,gap}` |
| Built-in material rule IDs | `minecraft:overworld/{copper_ore_vein,iron_ore_vein}` |

---

## Oracle probe points

Public classes and methods a black-box parity tool can call to dump intermediate aquifer and ore-vein state:
- `net.minecraft.world.level.levelgen.NoiseChunk`:
  - `new NoiseChunk(randomState, beardifier, settings, globalFluidPicker, blender, volume)`
  - `aquifer()` → `Aquifer`
  - `cachingSamplers()` → `DensitySamplerSet`
- `net.minecraft.world.level.levelgen.Aquifer`:
  - `computeSubstance(int blockX, int blockY, int blockZ, double density)` → `@Nullable BlockState`
  - `shouldScheduleFluidUpdate()` → `boolean`
  - `Aquifer.Config.create(cachingSamplers, positionalRandomFactory, volume, fluidRule)` → `Aquifer`
  - `Aquifer.FluidPicker.computeFluid(int blockX, int blockY, int blockZ)` → `Aquifer.FluidStatus` (`fluidLevel()`, `fluidType()`, `at(int blockY)`)
- `net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator`:
  - `getBaseColumn(int x, int z, LevelHeightAccessor, RandomState)` → `NoiseColumn` (exercises `final_density` + `Aquifer` without surface/carvers)
  - `getBaseHeight(int x, int z, Heightmap.Types, LevelHeightAccessor, RandomState)` → `int`
  - `buildTerrain(chunk, blender, randomState, structureManager, biomeManager, carverBiomeRegion, possibleBiomes)` → `CompletableFuture<ChunkAccess>` (full fill + surface + ore veins + carvers)
- `net.minecraft.world.level.levelgen.material.rule.OreVeinRule`:
  - `compile(MaterialRuleContext)` → `RuleEvaluator` (`tryApply(int blockX, int blockY, int blockZ)` → `@Nullable BlockState`)
- `net.minecraft.world.level.chunk.ChunkAccess`:
  - `getPostProcessing()` → `@Nullable ShortList[]` (verifies exact fluid-update post-processing coordinates queued by `doFill`, `buildSurface`, and `applyCarvingMask`).

---

## Open questions

1. **[oracle] Volume vs. point sampling 1-ULP boundary on `Aquifer.Config.surface_level` cache misses**: In `NoiseBasedAquifer`, the constructor populates `surfaceLevelCache` via `surfaceLevel.sampleVolume` over the 4-block-strided bounding box of the chunk's grid cells, whereas chunk-shifted probes outside that box (notably `(-3, 0)`) fall back to `surfaceLevel.sampleValue`. **Probe**: Compare `Aquifer.computeSubstance` outputs and `surfaceLevelCache` entries across 64 chunks spanning high-gradient coastlines and mountain slopes against a pure point-sampled implementation to confirm whether volume-prefill parity is observable in `golden-terrain`.
2. **[oracle] `OreVeinRule` prefill vs. single-block `topMaterial` evaluation during carver fix-up**: During `buildSurface`, `OreVeinRule` prefills `density` and `richness` over `narrowedVolume`, whereas during `applyCarvingMask` → `MaterialSystem.topMaterial`, `OreVeinRule` compiles on a `1 × 1 × 1` volume. **Probe**: In chunks where a cave carves through grass/mycelium above dirt inside `[0, 49]` where copper ore-vein density is positive, verify via `buildTerrain` dump that `topMaterial` evaluates `OreVeinRule` first and can replace the exposed dirt block with granite/copper ore, and confirm whether `1 × 1 × 1` volume sampling matches chunk-wide prefill bit-for-bit.

---

## Parity status

`golden-terrain`: not yet run.
