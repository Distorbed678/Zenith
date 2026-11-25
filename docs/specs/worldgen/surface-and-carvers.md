# Worldgen: surface rules, special surface passes, carvers, and terrain chunk-step order (26.3)

- Verified by suites: `golden-terrain`
- Sources: Minecraft: Java Edition 26.3 data-pack schemas (`minecraft:worldgen/material_rule`, `minecraft:worldgen/material_condition`, `minecraft:worldgen/carver`, `minecraft:worldgen/biome`), `docs/specs/math/noise.md`, `docs/specs/math/random.md`, `docs/specs/math/java-math-helpers.md`, `docs/specs/codec/datapacks-and-codecs.md`, `docs/specs/registry/identifiers-registries-tags.md`, and local reference reading (2026-11-25). Everything below is written in our own words.
- Related specs: [`math/noise.md`](../math/noise.md), [`math/random.md`](../math/random.md), [`math/java-math-helpers.md`](../math/java-math-helpers.md), [`codec/datapacks-and-codecs.md`](../codec/datapacks-and-codecs.md), [`registry/identifiers-registries-tags.md`](../registry/identifiers-registries-tags.md), [`worldgen/density-functions.md`](./density-functions.md), [`worldgen/biomes-and-climate.md`](./biomes-and-climate.md), [`worldgen/aquifers-and-ore-veins.md`](./aquifers-and-ore-veins.md).

## Summary

In 26.3, terrain noise fill, surface material replacement, and cave/canyon carving are unified inside a single chunk status (`minecraft:terrain`, `NoiseBasedChunkGenerator.buildTerrain`), executing three sequential passes over one `NoiseChunk`:

1. **Density fill (`doFill`)**: Populates solid blocks and aquifer fluids/air, updating `OCEAN_FLOOR_WG` and `WORLD_SURFACE_WG` (see [`aquifers-and-ore-veins.md`](./aquifers-and-ore-veins.md)).
2. **Surface & material replacement (`MaterialSystem.buildSurface`)**: Replaces the pre-26.3 `SurfaceSystem` / `SurfaceRules` with data-driven registries `minecraft:worldgen/material_rule` and `minecraft:worldgen/material_condition`. For each `(x, z)` column in a chunk (`x` outer, `z` inner):
   - Runs the **eroded badlands pillar** pre-pass if the column's surface biome is `minecraft:eroded_badlands`.
   - Walks downward from `WORLD_SURFACE_WG + 1` to `chunk.minY`, tracking stone depth from floor (`stoneDepthAbove`), stone depth from ceiling (`stoneDepthBelow`), and overlying fluid surface height (`waterHeight`), evaluating the compiled `MaterialRule` tree (which in 26.3 also contains `minecraft:ore_vein` rules) on every solid non-air non-fluid voxel.
   - Runs the **frozen ocean iceberg** post-pass if the column's surface biome is `minecraft:frozen_ocean` or `minecraft:deep_frozen_ocean`.
3. **Carvers (`NoiseBasedChunkGenerator.generateCarvers` → `applyCarvingMask`)**: In 26.3, there are only two carver types (`minecraft:cave` and `minecraft:canyon`; `minecraft:nether_cave` is a configured `cave` carver). Carvers from all `17 × 17` neighboring chunks (`dx, dz ∈ [-8, +8]`) write carved coordinates into an ephemeral per-chunk `CarvingMask` bitset. A second pass visits each contiguous vertical carved run top-down, skips `#minecraft:uncarvable` blocks, resolves replacement blocks via `noiseChunk.aquifer().computeSubstance(x, y, z, 0.0)`, and fixes exposed `minecraft:dirt` beneath carved `minecraft:grass_block` / `minecraft:mycelium` using `MaterialSystem.topMaterial`.
4. **Post-terrain finalization**: Applies `BelowZeroRetrogen` bedrock fixes (if upgrading an older chunk) and primes the four final heightmaps (`MOTION_BLOCKING`, `MOTION_BLOCKING_NO_LEAVES`, `OCEAN_FLOOR`, `WORLD_SURFACE`).

---

## Inputs and state

### Arithmetic and coordinate conventions
- IEEE-754 `f32` and `f64`, round-to-nearest-even, no FMA; left-to-right associativity unless parenthesized.
- Two's-complement wrapping `i32` / `i64`; Java `%` is truncated remainder; `floorDiv` / `floorMod` follow Java `Math`; `trunc_i32` truncates toward zero; `floor_i32` / `round_i32` follow [`java-math-helpers.md`](../math/java-math-helpers.md).
- `Mth.sin(f64)` and `Mth.cos(f64)` use the 65,536-entry `f32` lookup table specified in [`java-math-helpers.md`](../math/java-math-helpers.md).
- `Noise.get(x, y, z)` and `Noise.get(x, y)` return `f32` ([`noise.md`](../math/noise.md)); `DensitySampler.sampleValue` returns `f32`. Exact widening points from `f32` to `f64` are called out in every formula below.
- `ChunkPos`: `minBlockX = chunkX << 4`, `blockX(offset) = (chunkX << 4) + offset`, `middleBlockX = (chunkX << 4) + 8` (and identically for Z).
- `ChunkAccess.getHeight(type, localX, localZ)` returns `heightmap.getFirstAvailable(localX & 15, localZ & 15) - 1` (the Y of the highest matching block, or `minY - 1` if empty). Therefore `getHeight(...) + 1` equals `getFirstAvailable(...)`.
- `WorldGenerationContext` for a generator and height accessor holds:
  - `minGenY = max(heightAccessor.minY, generator.minY)`
  - `genDepth = min(heightAccessor.height, generator.genDepth)`
  - `seaLevel = generator.seaLevel`

### `MaterialSystem` persistent state (constructed once inside `RandomState`)
Constructed with `(randomState, defaultBlock, seaLevel, router.chunkSurfaceLevel, rootPositionalRandom)`:
- Stores `defaultBlock`, `seaLevel`, `preliminarySurfaceFunction = router.chunkSurfaceLevel`, and `noiseRandom = rootPositionalRandom` (the world's root positional factory `randomAlgorithm.newInstance(worldSeed).forkPositional()`, **not** a named sub-factory).
- Generates the 192-entry `clayBands` array from `noiseRandom.fromHashOf("minecraft:clay_bands")` (Section 2.1).
- Instantiates 9 normal noises via `randomState.getOrCreateNoise(...)` (each seeded via `rootPositionalRandom.fromHashOf(noiseId)`):
  - `minecraft:clay_bands_offset`, `minecraft:surface`, `minecraft:surface_secondary`
  - `minecraft:badlands_pillar`, `minecraft:badlands_pillar_roof`, `minecraft:badlands_surface`
  - `minecraft:iceberg_pillar`, `minecraft:iceberg_pillar_roof`, `minecraft:iceberg_surface`

---

## Behavior (ordered rules)

### 1. Material rules and conditions (`MaterialRule`, `MaterialCondition`)

#### 1.1 Registries and holder encoding
26.3 introduces two data-driven registries (`world` layer; see [`identifiers-registries-tags.md`](../registry/identifiers-registries-tags.md)) and two built-in dispatch registries:
- Dynamic registries: `minecraft:worldgen/material_rule`, `minecraft:worldgen/material_condition`.
- Static type-codec registries: `minecraft:worldgen/material_rule_type`, `minecraft:worldgen/material_condition_type`.
- Anywhere a `MaterialRule` or `MaterialCondition` is expected in JSON, the codec accepts **either** a registry reference identifier string (e.g. `"minecraft:on_floor"`, `"minecraft:bedrock_floor"`) **or** an inline type-dispatched object (`{ "type": "<id>", ... }`).

#### 1.2 Shared helper schemas

**`VerticalAnchor`** (untagged `xor` of four single-key objects; every integer offset is validated in `[DimensionType.MIN_Y, DimensionType.MAX_Y] = [-2032, 2031]`):

| JSON form | Resolved Y (`resolveY(context)`) |
|---|---|
| `{ "absolute": y }` | `y` |
| `{ "above_bottom": offset }` | `context.minGenY + offset` |
| `{ "below_top": offset }` | `context.genDepth - 1 + context.minGenY - offset` |
| `{ "relative_to_sea_level": offset }` | `context.seaLevel + offset` *(new in 26.3)* |

**`CaveSurface`** (string enum): `"floor"` (downward / from above) or `"ceiling"` (upward / from below).

#### 1.3 `MaterialRule` types (`minecraft:worldgen/material_rule_type`)
All 5 built-in rule types:

| Type ID | JSON fields | Compiled behavior (`tryApply(blockX, blockY, blockZ) -> BlockState?`) |
|---|---|---|
| `minecraft:block` | `result_state`: `BlockState` (required) | Always returns `result_state`. |
| `minecraft:bandlands` | *(none)* | Returns `context.getBand(blockX, blockY, blockZ)` (Section 2.1). |
| `minecraft:sequence` | `sequence`: list of `MaterialRule` (required) | Compiles each child rule in order (single-element list compiles directly to that child). At runtime, iterates compiled children in order and returns the first non-null `BlockState`, or `null` if all return `null`. |
| `minecraft:condition` | `if_true`: `MaterialCondition` (required), `then_run`: `MaterialRule` (required) | Compiles `if_true` then `then_run`. At runtime, evaluates `if_true.test()`; if `false`, returns `null` without calling `then_run`; if `true`, returns `then_run.tryApply(blockX, blockY, blockZ)`. |
| `minecraft:ore_vein` | `ore_block`, `raw_ore_block`, `filler_block`, `raw_ore_chance`, `density`, `richness`, `filler_gap` | Evaluates 3D ore-vein placement with positional RNG `minecraft:ore`. Full specification in [`aquifers-and-ore-veins.md`](./aquifers-and-ore-veins.md) Section 3. |

#### 1.4 `MaterialCondition` types (`minecraft:worldgen/material_condition_type`)
All 11 built-in condition types (and their caching scope in `MaterialRuleContext`):

| Type ID | JSON fields | Cache scope | Evaluation rule (`test() -> bool`) |
|---|---|---|---|
| `minecraft:biome` | `biome_is`: `HolderSet<Biome>` (`#tag`, single ID, or list of IDs; required) | Compile-time prune + `LazyY` | At compile time, if `context.possibleBiomes != null`: if intersection with `biome_is` is empty → constant `false`; else if `possibleBiomes ⊆ biome_is` → constant `true`. Otherwise lazy per Y: `biome_is.contains(context.getBiome())`. |
| `minecraft:noise_threshold` | `noise`: `ResourceKey<NormalNoise>` (required), `min_threshold`: `f64` (required), `max_threshold`: `f64` (required), `is_3d`: `bool` (optional, default `false`) | Shared per-noise cache (`XZ` if 2D, `Y` if 3D) | Samples `v = f64(noise.get(blockX, is_3d ? blockY : 0.0, blockZ))`; returns `v >= min_threshold && v <= max_threshold`. *(Note: `is_3d` is new in 26.3.)* |
| `minecraft:vertical_gradient` | `random_name`: `Identifier` (required), `true_at_and_below`: `VerticalAnchor` (required), `false_at_and_above`: `VerticalAnchor` (required) | `LazyY` | Resolves `lo = resolveY(true_at_and_below)` and `hi = resolveY(false_at_and_above)` and acquires `factory = getOrCreateRandomFactory(random_name)` at compile time. At runtime: if `blockY <= lo` return `true`; if `blockY >= hi` return `false`; else `p = Mth.map(f64(blockY), f64(lo), f64(hi), 1.0, 0.0)`, return `f64(factory.at(blockX, blockY, blockZ).nextFloat()) < p`. |
| `minecraft:y_above` | `anchor`: `VerticalAnchor` (required), `surface_depth_multiplier`: `i32` in `[-20, 20]` (required), `add_stone_depth`: `bool` (required) | `LazyY` | Returns `blockY + (add_stone_depth ? stoneDepthAbove : 0) >= resolveY(anchor) + surfaceDepth * surface_depth_multiplier`. |
| `minecraft:water` | `offset`: `i32` (required), `surface_depth_multiplier`: `i32` in `[-20, 20]` (required), `add_stone_depth`: `bool` (required) | `LazyY` | Returns `waterHeight == i32::MIN \|\| (blockY + (add_stone_depth ? stoneDepthAbove : 0) >= waterHeight + offset + surfaceDepth * surface_depth_multiplier)`. |
| `minecraft:temperature` | *(none)* | Uncached (biome cached per Y; biome temperature has thread-local 1024-entry LRU cache) | Returns `context.getBiome().coldEnoughToSnow(blockPos, context.getSeaLevel())` (Section 1.7). |
| `minecraft:steep` | *(none)* | `LazyXZ` | Returns `context.surfaceGradientX <= -4 \|\| context.surfaceGradientZ >= 4`. |
| `minecraft:not` | `invert`: `MaterialCondition` (required) | Delegates to child | Returns `!compiledChild.test()`. |
| `minecraft:hole` | *(none)* | `LazyXZ` | Returns `context.surfaceDepth <= 0`. |
| `minecraft:above_preliminary_surface` | *(none)* | `minSurfaceLevel` cached per XZ | Returns `context.blockY >= context.getMinSurfaceLevel()`. |
| `minecraft:stone_depth` | `offset`: `i32` (required), `add_surface_depth`: `bool` (required), `secondary_depth_range`: `i32` (required), `surface_type`: `CaveSurface` (required) | `LazyY` | `stoneDepth = (surface_type == CEILING) ? stoneDepthBelow : stoneDepthAbove`; `surf = add_surface_depth ? surfaceDepth : 0`; `sec = (secondary_depth_range == 0) ? 0 : trunc_i32(Mth.map(context.getSurfaceSecondary(), -1.0, 1.0, 0.0, f64(secondary_depth_range)))`; returns `stoneDepth <= 1 + offset + surf + sec`. |

#### 1.5 `MaterialRuleContext` state and caching
`MaterialRuleContext` holds monotonic counters `lastUpdateXZ` and `lastUpdateY` (both initialized to `i64::MIN + 1 = -9223372036854775807`):
- `updateXZ(blockX, blockZ, gradientX, gradientZ)`:
  1. Increments `lastUpdateXZ` and `lastUpdateY`.
  2. Stores `blockX`, `blockZ`, `surfaceGradientX`, `surfaceGradientZ`.
  3. Computes `surfaceDepth` immediately (`MaterialSystem.getSurfaceDepth`):
     $$\text{surfaceDepth} = \text{trunc}_{i32}\!\left(\text{f64}(\text{surfaceNoise.get}(x, 0.0, z)) \cdot 2.75 + 3.0 + \text{noiseRandom.at}(x, 0, z).\text{nextDouble}() \cdot 0.25\right)$$
     Note: uses **truncation toward zero** (`trunc_i32`), not floor, and queries `noiseRandom` (the world root positional factory).
- `updateY(stoneDepthAbove, stoneDepthBelow, waterHeight, blockY)`:
  1. Increments `lastUpdateY`.
  2. Clears cached `biome = null`.
  3. Stores `blockY`, `waterHeight`, `stoneDepthBelow`, `stoneDepthAbove`.
- Lazy per-XZ caches (recomputed when `lastUpdate != lastUpdateXZ`):
  - `getSurfaceSecondary()`: `f64(surfaceSecondaryNoise.get(blockX, 0.0, blockZ))`.
  - `getMinSurfaceLevel()`:
    - Checks `preliminarySurfaceVolume = DensityVolume(expectedVolume.sizeX, 1, expectedVolume.sizeZ, expectedVolume.minBlockX, 0, expectedVolume.minBlockZ)`.
    - If `(blockX, 0, blockZ)` is inside `preliminarySurfaceVolume`: on first access, allocates an unpooled `DensityBuffer` of size `sizeX * sizeZ` and evaluates `preliminarySurfaceFunction` (`router.chunkSurfaceLevel`) over the entire 2D volume; reads `preliminarySurfaceLevel = buffer.get(index)` (`f32`).
    - Otherwise: point-samples `densitySamplers.sampleValue(preliminarySurfaceFunction, blockX, 0, blockZ)` (`f32`).
    - Computes `minSurfaceLevel = floor_i32(preliminarySurfaceLevel) + surfaceDepth - 8`.
  - `LazyXZCondition` subclasses (`HoleCondition`, `SteepCondition`): cache `bool` per `lastUpdateXZ`.
  - 2D noise samplers (`noiseSamplers2d`, keyed by `ResourceKey<NormalNoise>` identity): cache `f64(noise.get(blockX, 0.0, blockZ))` per `lastUpdateXZ`.
- Lazy per-Y caches (recomputed when `lastUpdate != lastUpdateY`):
  - `getBiome()`: calls `biomeGetter.apply(BlockPos(blockX, blockY, blockZ))` once per Y.
  - `LazyYCondition` subclasses (`BiomeCondition`, `StoneDepthCondition`, `VerticalGradientCondition`, `WaterCondition`, `YCondition`): cache `bool` per `lastUpdateY`.
  - 3D noise samplers (`noiseSamplers3d`, keyed by `ResourceKey<NormalNoise>` identity): cache `f64(noise.get(blockX, blockY, blockZ))` per `lastUpdateY`.

#### 1.6 Per-column surface algorithm (`MaterialSystem.buildSurface`)
Inputs: `randomState`, `biomeManager`, `generationContext`, `protoChunk`, `noiseChunk`, `materialRule`, `possibleBiomes`.

1. **Height bounds and narrowed volume**:
   - `heightAccessor = protoChunk.getHeightAccessorForGeneration()`; `minY = heightAccessor.minY`, `maxY = heightAccessor.maxY`.
   - `highestFilledSection = protoChunk.getHighestFilledSectionIndex()` (scanning sections top-down for first `!section.hasOnlyAir()`, or `-1` if all air).
   - `maxFilledY = protoChunk.getSectionYFromSectionIndex(highestFilledSection) * 16 + 15`. (If `highestFilledSection == -1`, `maxFilledY = protoChunk.minY - 1`.)
   - `narrowedVolume = DensityVolume(fullVolume.sizeX, max(maxFilledY - fullVolume.minBlockY + 1, 1), fullVolume.sizeZ, fullVolume.minBlockX, fullVolume.minBlockY, fullVolume.minBlockZ)`.
   - Construct `MaterialRuleContext` with `narrowedVolume`, `noiseChunk.cachingSamplers()`, `biomeManager::getBiome`, `generationContext`, `possibleBiomes`.
   - Compile `rule = materialRule.compile(context)` (triggers `OreVeinRule` prefill buffers over `narrowedVolume`).
2. **BlockColumn write wrapper**:
   - `column.getBlock(y)` calls `protoChunk.getBlockState(BlockPos(blockX, y, blockZ))` (returns `minecraft:void_air` outside build height).
   - `column.setBlock(y, state)`: if `minY <= y <= maxY`, calls `protoChunk.setBlockState(BlockPos(blockX, y, blockZ), state)` (which **immediately updates `OCEAN_FLOOR_WG` and `WORLD_SURFACE_WG`** because `protoChunk`'s persisted status is `BIOMES`!) and if `!state.getFluidState().isEmpty()`, calls `protoChunk.markPosForPostProcessing(pos)`.
3. **Column iteration** — exact loop order **`x = 0..15` (outer), `z = 0..15` (inner)**:
   1. `blockX = minBlockX + x`, `blockZ = minBlockZ + z`.
   2. `startingHeight = protoChunk.getHeight(WORLD_SURFACE_WG, x, z) + 1` (= `firstAvailable` Y above highest non-air block).
   3. `surfaceBiome = biomeManager.getBiome(BlockPos(blockX, startingHeight, blockZ))`.
   4. **Pre-pass (eroded badlands)**: if `surfaceBiome` is `minecraft:eroded_badlands`, run `erodedBadlandsExtension(column, blockX, blockZ, startingHeight, protoChunk)` (Section 2.2).
   5. **Re-read height and compute live gradients**:
      - `height = protoChunk.getHeight(WORLD_SURFACE_WG, x, z) + 1`.
      - `gradientX = protoChunk.getHeight(WORLD_SURFACE_WG, min(x + 1, 15), z) - protoChunk.getHeight(WORLD_SURFACE_WG, max(x - 1, 0), z)`.
      - `gradientZ = protoChunk.getHeight(WORLD_SURFACE_WG, x, min(z + 1, 15)) - protoChunk.getHeight(WORLD_SURFACE_WG, x, max(z - 1, 0))`.
      - Call `context.updateXZ(blockX, blockZ, gradientX, gradientZ)`.
   6. **Downward column walk**: initialize `stoneAboveDepth = 0`, `waterHeight = i32::MIN`, `nextCeilingStoneY = i32::MAX`, `endY = protoChunk.getMinY()`.
      For `y = height` down to `endY` (inclusive):
      - `old = column.getBlock(y)`.
      - If `old.isAir()`:
        - `stoneAboveDepth = 0`; `waterHeight = i32::MIN`.
      - Else if `!old.getFluidState().isEmpty()`:
        - If `waterHeight == i32::MIN`: `waterHeight = y + 1`.
      - Else (solid non-air non-fluid block, i.e., `isStone(old)`):
        1. If `nextCeilingStoneY >= y`:
           - Reset `nextCeilingStoneY = DimensionType.WAY_BELOW_MIN_Y` (`-32512`).
           - Scan `lookaheadY = y - 1` down to `endY - 1` (inclusive):
             - If `!isStone(column.getBlock(lookaheadY))` (note: at `lookaheadY == endY - 1`, `getBlock` returns `void_air`, which is air, so `!isStone` is always true):
               - Set `nextCeilingStoneY = lookaheadY + 1` and break.
        2. `stoneAboveDepth += 1`.
        3. `stoneBelowDepth = y - nextCeilingStoneY + 1`.
        4. Call `context.updateY(stoneAboveDepth, stoneBelowDepth, waterHeight, y)`.
        5. If `minY <= y <= maxY`:
           - `state = rule.tryApply(blockX, y, blockZ)`.
           - If `state != null`: `column.setBlock(y, state)`.
   7. **Post-pass (frozen ocean)**: if `surfaceBiome` is `minecraft:frozen_ocean` or `minecraft:deep_frozen_ocean`, run `frozenOceanExtension(context.getMinSurfaceLevel(), surfaceBiome, column, blockPos, blockX, blockZ, startingHeight)` (Section 2.3). Notice this passes `startingHeight` (not `height`, though both are equal in frozen oceans) and triggers `context.getMinSurfaceLevel()`.

#### 1.7 Single-block surface query (`MaterialSystem.topMaterial`)
Used exclusively by carver grass/mycelium fix-up (`NoiseBasedChunkGenerator.applyCarvingMask`):
1. Constructs a `1 × 1 × 1` `DensityVolume(1, 1, 1, pos.x, pos.y, pos.z)`.
2. Constructs `MaterialRuleContext` with `possibleBiomes = null` (disabling compile-time biome pruning) and compiles `materialRule`.
3. Computes `gradientX` and `gradientZ` from the chunk's `WORLD_SURFACE_WG` at `(pos.x & 15, pos.z & 15)`.
4. Calls `context.updateXZ(pos.x, pos.z, gradientX, gradientZ)`.
5. Calls `context.updateY(1, 1, underFluid ? (pos.y + 1) : i32::MIN, pos.y)`.
6. Returns `Optional.ofNullable(rule.tryApply(pos.x, pos.y, pos.z))`.

#### 1.8 Biome temperature evaluation (`coldEnoughToSnow` and `shouldMeltFrozenOceanIcebergSlightly`)
Used by `TemperatureCondition` (`coldEnoughToSnow`) and `frozenOceanExtension` (`shouldMeltFrozenOceanIcebergSlightly`), using the fixed-seed simplex noises specified in [`noise.md`](../math/noise.md):
1. Apply the biome's `temperature_modifier` to `baseTemperature` (`f32`):
   - `"none"`: `adjusted = baseTemperature`.
   - `"frozen"`:
     - `largeVar = f64(FROZEN_TEMPERATURE_NOISE.get(f64(x) * 0.05, f64(z) * 0.05) * 7.0_f32)` (`f32` multiply by `7.0f`, widened to `f64`).
     - `edgeVar = f64(BIOME_INFO_NOISE.get(f64(x) * 0.2, f64(z) * 0.2))`.
     - If `largeVar + edgeVar < 0.3`:
       - `smallVar = f64(BIOME_INFO_NOISE.get(f64(x) * 0.09, f64(z) * 0.09))`.
       - If `smallVar < 0.8`: `adjusted = 0.2_f32`; else `adjusted = baseTemperature`.
     - Else: `adjusted = baseTemperature`.
2. Height adjustment (`snowLevel = seaLevel + 17`):
   - If `y > snowLevel`:
     - `v = TEMPERATURE_NOISE.get(f64(f32(x) / 8.0_f32), f64(f32(z) / 8.0_f32)) * 8.0_f32` (note: `x / 8.0f` and `z / 8.0f` are **`f32` divisions** widened to `f64`; output multiplied by `8.0f` in `f32`).
     - `temp = adjusted - (((v + f32(y)) - f32(snowLevel)) * 0.05_f32) / 40.0_f32` (all `f32`, left-to-right).
   - Else: `temp = adjusted`.
3. Predicates:
   - `coldEnoughToSnow(pos, seaLevel)`: `temp < 0.15_f32` (i.e. `!(temp >= 0.15_f32)`).
   - `shouldMeltFrozenOceanIcebergSlightly(pos, seaLevel)`: `temp > 0.1_f32`.

---

### 2. Special surface passes

#### 2.1 Badlands terracotta clay bands (`generateBands` and `getBand`)
Constructed once per `MaterialSystem` using `rng = noiseRandom.fromHashOf("minecraft:clay_bands")`:
1. Allocate `clayBands[192]` filled with `minecraft:terracotta`.
2. **Orange terracotta bands**: loop `i = 0` while `i < 192` (incrementing `i += 1` at loop step):
   - `i += rng.nextInt(5) + 1`.
   - If `i < 192`: `clayBands[i] = minecraft:orange_terracotta`.
   *(Note: the loop increments `i` both inside the body by `nextInt(5) + 1` AND via the loop header `i++`, so the net step between consecutive orange bands is `nextInt(5) + 2 ∈ [2, 6]`.)*
3. **Helper `makeBands(rng, clayBands, baseWidth, state)`**:
   - `bandCount = rng.nextIntBetweenInclusive(6, 15)` (= `rng.nextInt(10) + 6`).
   - Repeat `bandCount` times:
     - `width = baseWidth + rng.nextInt(3)`.
     - `start = rng.nextInt(192)`.
     - For `p = 0 .. width - 1` while `start + p < 192`: `clayBands[start + p] = state`.
4. Call `makeBands` three times in exact order:
   1. `baseWidth = 1`, `minecraft:yellow_terracotta`
   2. `baseWidth = 2`, `minecraft:brown_terracotta`
   3. `baseWidth = 1`, `minecraft:red_terracotta`
5. **White and light-gray bands**:
   - `whiteBandCount = rng.nextIntBetweenInclusive(9, 15)` (= `rng.nextInt(7) + 9`).
   - Loop `k = 0`, `start = 0` while `k < whiteBandCount && start < 192`:
     - `clayBands[start] = minecraft:white_terracotta`.
     - If `start - 1 > 0 && rng.nextBoolean()`: `clayBands[start - 1] = minecraft:light_gray_terracotta` *(strict `start - 1 > 0`, so index `0` is never overwritten and `rng.nextBoolean()` is NOT called when `start <= 1`!)*.
     - If `start + 1 < 192 && rng.nextBoolean()`: `clayBands[start + 1] = minecraft:light_gray_terracotta`.
     - `k += 1`; `start += rng.nextInt(16) + 4` *(note: drawn even on the final iteration `k == whiteBandCount`, though unused afterward because `rng` is discarded)*.

Lookup `getBand(worldX, y, worldZ)`:
1. `offset = round_i32(clayBandsOffsetNoise.get(f64(worldX), 0.0, f64(worldZ)) * 4.0_f32)` (`f32` multiply by `4.0f`, then Java `Math.round(float)`).
2. Return `clayBands[(y + offset + 192) % 192]` (Java truncated `%`).

#### 2.2 Eroded badlands pillars (`erodedBadlandsExtension`)
Runs in `buildSurface` **before** the rule-evaluation walk whenever `surfaceBiome` is `minecraft:eroded_badlands`:
1. Compute `pillarBuffer` in `f64`:
   - `a = abs(f64(badlandsSurfaceNoise.get(blockX, 0.0, blockZ)) * 8.25)` (`f64` multiply).
   - `b = f64(badlandsPillarNoise.get(blockX * 0.2, 0.0, blockZ * 0.2) * 15.0_f32)` (**`f32` multiply by `15.0f`**, then widened to `f64`).
   - `pillarBuffer = min(a, b)`.
2. If `pillarBuffer <= 0.0`: return.
3. `pillarFloor = abs(f64(badlandsPillarRoofNoise.get(blockX * 0.75, 0.0, blockZ * 0.75)) * 1.5)` (`f64` multiply).
4. `extensionTop = 64.0 + min(pillarBuffer * pillarBuffer * 2.5, ceil(pillarFloor * 50.0) + 24.0)`.
5. `startY = floor_i32(extensionTop)`.
6. If `startingHeight > startY`: return.
7. **Water check**: scan `y = startY` down to `protoChunk.minY`:
   - If `column.getBlock(y)` is `defaultBlock.getBlock()`: break (solid rock reached before water).
   - If `column.getBlock(y)` is `minecraft:water`: return immediately (do not build pillar over water).
8. **Pillar fill**: scan `y = startY` down to `protoChunk.minY` while `column.getBlock(y).isAir()`:
   - `column.setBlock(y, defaultBlock)` (updates `WORLD_SURFACE_WG` and `OCEAN_FLOOR_WG`).

#### 2.3 Frozen ocean icebergs (`frozenOceanExtension`)
Runs in `buildSurface` **after** the rule-evaluation walk whenever `surfaceBiome` is `minecraft:frozen_ocean` or `minecraft:deep_frozen_ocean`:
1. Compute `iceberg` in `f64`:
   - `a = abs(f64(icebergSurfaceNoise.get(blockX, 0.0, blockZ)) * 8.25)` (`f64` multiply).
   - `b = f64(icebergPillarNoise.get(blockX * 1.28, 0.0, blockZ * 1.28) * 15.0_f32)` (**`f32` multiply by `15.0f`**, then widened to `f64`).
   - `iceberg = min(a, b)`.
2. If `iceberg <= 1.8`: return.
3. `icebergRoof = abs(f64(icebergPillarRoofNoise.get(blockX * 1.17, 0.0, blockZ * 1.17)) * 1.5)`.
4. `top = min(iceberg * iceberg * 1.2, ceil(icebergRoof * 40.0) + 14.0)`.
5. If `surfaceBiome.shouldMeltFrozenOceanIcebergSlightly(BlockPos(blockX, seaLevel, blockZ), seaLevel)`:
   - `top -= 2.0`.
6. If `top <= 2.0`: return.
7. `extensionBottom = f64(seaLevel) - top - 7.0`; `extensionTop = top + f64(seaLevel)`.
8. `rng = noiseRandom.at(blockX, 0, blockZ)` (world root positional factory at `y = 0`).
9. Draw in exact order:
   - `maxSnowDepth = 2 + rng.nextInt(4)` (`2..=5`).
   - `minSnowHeight = seaLevel + 18 + rng.nextInt(10)` (`seaLevel + 18 .. seaLevel + 27`).
   - `snowDepth = 0`.
10. Scan `y = max(startingHeight, trunc_i32(extensionTop) + 1)` down to `minSurfaceLevel` (inclusive):
    - Evaluate the short-circuited placement condition (note exact `trunc_i32` casts and RNG consumption!):
      - Branch A: `column.getBlock(y).isAir() && y < trunc_i32(extensionTop) && rng.nextDouble() > 0.01`
      - Branch B (only evaluated if Branch A is `false`): `column.getBlock(y).is(minecraft:water) && y > trunc_i32(extensionBottom) && y < seaLevel && rng.nextDouble() > 0.15`
    - If Branch A or Branch B succeeds:
      - If `snowDepth <= maxSnowDepth && y > minSnowHeight`:
        - `column.setBlock(y, minecraft:snow_block)`
        - `snowDepth += 1` *(note `<=`, so up to `maxSnowDepth + 1` snow blocks are placed: `3..=6` blocks!)*
      - Else:
        - `column.setBlock(y, minecraft:packed_ice)`

---

### 3. Carvers (`WorldCarver`, `CaveWorldCarver`, `CanyonWorldCarver`, `CarvingMask`)

#### 3.1 Data-pack schemas (26.3)
In 26.3, carvers are registered in dynamic registry `minecraft:worldgen/carver`, dispatched via `minecraft:worldgen/carver_type` (`"type"` field). Legacy pre-26.3 fields (`config` wrapper object, `lava_level`, `replaceable`, `debug_settings`, `aquifers`, and the `nether_cave` carver type) **do not exist in 26.3**. Biomes list carvers directly in `"carvers"` as a flat `HolderSet<WorldCarver>` (a `#tag`, single ID/inline object, or list), with no air/liquid step map.

**Type `minecraft:cave` (`CaveWorldCarver`)**:

| JSON field | Type / range | Required | Default |
|---|---|---|---|
| `probability` | `f32` in `[0.0, 1.0]` | yes | — |
| `y` | `HeightProvider` | yes | — |
| `count` | `IntProvider` (`min >= 0`) | yes | — |
| `thickness` | `FloatProvider` (`min >= 0.0`) | yes | — |
| `weird_thickness_bias` | `bool` | no | `false` |
| `room_vertical_radius_multiplier` | `FloatProvider` | yes | — |
| `horizontal_radius_multiplier` | `FloatProvider` | yes | — |
| `vertical_radius_multiplier` | `FloatProvider` | yes | — |
| `start_vertical_radius_multiplier` | `FloatProvider` | no | `1.0` (`ConstantFloat`) |
| `floor_level` | `FloatProvider` in `[-1.0, 1.0]` | yes | — |

**Type `minecraft:canyon` (`CanyonWorldCarver`)**:

| JSON field | Type / range | Required |
|---|---|---|
| `probability` | `f32` in `[0.0, 1.0]` | yes |
| `y` | `HeightProvider` | yes |
| `vertical_rotation` | `FloatProvider` | yes |
| `shape` | `Shape` object (below) | yes |

**`CanyonWorldCarver.Shape` object**:

| JSON field | Type / range | Required |
|---|---|---|
| `distance_factor` | `FloatProvider` | yes |
| `thickness` | `FloatProvider` | yes |
| `width_smoothness` | `i32 >= 1` (`POSITIVE_INT`) | yes |
| `horizontal_radius_factor` | `FloatProvider` | yes |
| `vertical_radius_default_factor` | `f32` | yes |
| `vertical_radius_center_factor` | `f32` | yes |
| `y_scale` | `FloatProvider` | yes |

**Value & height providers used by carvers** (inline constant or type-dispatched object):
- **`HeightProvider`**: inline `VerticalAnchor` (shorthand for `minecraft:constant`), or `type` in `{constant, uniform, biased_to_bottom, very_biased_to_bottom, trapezoid, weighted_list}`.
  - `uniform`: `{ min_inclusive: VerticalAnchor, max_inclusive: VerticalAnchor }`. Resolves `min`, `max`; if `min > max` returns `min` without drawing RNG; else `Mth.randomBetweenInclusive(rng, min, max)` (= `rng.nextInt(max - min + 1) + min`, unconditionally draws even when `min == max`).
- **`FloatProvider`**: inline `f32` (shorthand for `minecraft:constant`), or `type` in `{constant, uniform, clamped_normal, trapezoid}`:
  - `uniform`: `{ min_inclusive: f32, max_exclusive: f32 }` (`max > min`). Samples `rng.nextFloat() * (max - min) + min` (`Mth.randomBetween`).
  - `trapezoid`: `{ min: f32, max: f32, plateau: f32 }` (`max >= min`, `plateau <= max - min`). Let `range = max - min`, `pStart = (range - plateau) / 2.0f`, `pEnd = range - pStart`; returns `min + rng.nextFloat() * pEnd + rng.nextFloat() * pStart` (2 `nextFloat()` draws, left-to-right `f32`).
- **`IntProvider`**: inline `i32` (shorthand for `minecraft:constant`), or `type` in `{constant, uniform, biased_to_bottom, very_biased_to_bottom, clamped, weighted_list, clamped_normal, trapezoid}`:
  - `very_biased_to_bottom`: `{ min_inclusive: i32, max_inclusive: i32 }` (`max >= min`). Samples `min + rng.nextInt(rng.nextInt(rng.nextInt(max - min + 1) + 1) + 1)` (3 nested `nextInt` draws, innermost first).

#### 3.2 Carving step (`NoiseBasedChunkGenerator.generateCarvers`)
Skipped if `DEBUG_DISABLE_CARVERS` or `debugVoidTerrain(chunkPos)`.

1. Create `biomeResolver = biomeSource.createUncachedResolver(randomState)`.
2. Create one reusable `WorldgenRandom` backed by `LegacyRandomSource` (**always legacy 48-bit LCG**, regardless of `settings.legacy_random_source`).
3. Compute `CarvingMask` vertical bounds from `context = WorldGenerationContext(this, chunk.getHeightAccessorForGeneration())`:
   - `protectedBlocksOnTop = chunk.isUpgrading() ? 0 : 7` (`isUpgrading()` is `chunk.getBelowZeroRetrogen() != null`).
   - `maskMinY = context.minGenY + 1`.
   - `maskMaxY = context.minGenY + context.genDepth - 1 - protectedBlocksOnTop`.
   - Allocate `mask = CarvingMask(maskMinY, maskMaxY)`.
4. Iterate source chunks in the **`17 × 17` neighborhood** (`range = 8`) in exact order: **`dx = -8 .. +8` (outer), `dz = -8 .. +8` (inner)**:
   1. `sourcePos = ChunkPos(chunkPos.x + dx, chunkPos.z + dz)`.
   2. Obtain `sourceBiomeGenerationSettings` for `sourcePos`:
      - Queries `biomeResolver.getNoiseBiome(sourcePos.minBlockX >> 2, 0, sourcePos.minBlockZ >> 2)` (quart Y = `0`, uncached biome source), cached per chunk via `carverCenterChunk.carverBiome(...)` when `carverBiomeRegion != null`. **[oracle]**
   3. Iterate `carver` in `sourceBiomeGenerationSettings.getCarvers()` with 0-based integer `index = 0, 1, 2, ...`:
      - Seed `random` via `setLargeFeatureSeed(randomState.seed() + index, sourcePos.x, sourcePos.z)`:
        1. `random.setSeed(seed)` where `seed = worldSeed + index` (64-bit wrapping).
        2. `xScale = random.nextLong()` (2 LCG steps).
        3. `zScale = random.nextLong()` (2 LCG steps).
        4. `combined = (i64(sourcePos.x) * xScale) ^ (i64(sourcePos.z) * zScale) ^ seed`.
        5. `random.setSeed(combined)`.
      - Test `carver.isStartChunk(random)`: draws `random.nextFloat() <= carver.probability`.
      - If `true`, call `carver.carve(context, random, chunkPos, sourcePos, mask)` using the same `random`.
5. If `!mask.isEmpty()`:
   - Build `correctBiomeManager = biomeManager.withDifferentSource(biomeResolver)` (retains the obfuscated seed zoom, backed by the uncached resolver).
   - Call `applyCarvingMask(chunk, mask, randomState, materialRule, context, noiseChunk, correctBiomeManager::getBiome, blender.getCarvingFilter())` (Section 3.6).

#### 3.3 Shared carver geometry (`WorldCarver.canReach` and `carveEllipsoid`)
Default `getRange() = 4` for all carvers, giving `maxDistance = (4 * 2 - 1) * 16 = 112`.

**`canReach(chunkPos, x: f64, z: f64, currentStep: i32, totalSteps: i32, thickness: f32) -> bool`**:
- `xd = x - f64(chunkPos.middleBlockX)`, `zd = z - f64(chunkPos.middleBlockZ)`.
- `remaining = f64(totalSteps - currentStep)`.
- `rr = f64((thickness + 2.0_f32) + 16.0_f32)` (`f32` additions before widening to `f64`).
- Return `(xd * xd + zd * zd) - remaining * remaining <= rr * rr` (`f64`).

**`carveEllipsoid(chunkPos, x, y, z, horizontalRadius, verticalRadius, output, skipChecker)`**:
1. Bounding check: `maxDelta = 16.0 + horizontalRadius * 2.0`. If `abs(x - middleBlockX) > maxDelta || abs(z - middleBlockZ) > maxDelta`: return.
2. Compute clamped local X/Z and world Y bounds:
   - `minX = max(floor_i32(x - horizontalRadius) - minBlockX - 1, 0)`
   - `maxX = min(floor_i32(x + horizontalRadius) - minBlockX, 15)` *(note: `- 1` on min, NO `+ 1` on max!)*
   - `minY = max(floor_i32(y - verticalRadius) - 1, output.minY)`
   - `maxY = min(floor_i32(y + verticalRadius) + 1, output.maxY)`
   - `minZ = max(floor_i32(z - horizontalRadius) - minBlockZ - 1, 0)`
   - `maxZ = min(floor_i32(z + horizontalRadius) - minBlockZ, 15)`
3. Loop `xIndex = minX ..= maxX`, `zIndex = minZ ..= maxZ`:
   - `xd = (f64(chunkPos.blockX(xIndex)) + 0.5 - x) / horizontalRadius`
   - `zd = (f64(chunkPos.blockZ(zIndex)) + 0.5 - z) / horizontalRadius`
   - If `xd * xd + zd * zd < 1.0`:
     - Loop `worldY = maxY` down to `minY + 1` (**strictly `worldY > minY`**!):
       - `yd = (f64(worldY) - 0.5 - y) / verticalRadius` *(note **`- 0.5`** for Y, opposite of `+ 0.5` for X and Z!)*
       - If `!skipChecker.shouldSkip(xd, yd, zd, worldY)`: `output.carve(xIndex, worldY, zIndex)`.

#### 3.4 Cave tunnel algorithm (`CaveWorldCarver`)
In `carve(context, random, chunkPos, sourceChunkPos, output)`:
1. `maxDistance = (getRange() * 2 - 1) << 4 = 112`.
2. Draw `caveCount = count.sample(random)`.
3. For `cave = 0 .. caveCount - 1`, draw in exact order:
   1. `x = f64(sourceChunkPos.blockX(random.nextInt(16)))`
   2. `y = f64(yProvider.sample(random, context))`
   3. `z = f64(sourceChunkPos.blockZ(random.nextInt(16)))`
   4. `hMult = f64(horizontalRadiusMultiplier.sample(random))`
   5. `vMult = f64(verticalRadiusMultiplier.sample(random))`
   6. `startVMult = f64(startVerticalRadiusMultiplier.sample(random))`
   7. `floorLevel = f64(floorLevelProvider.sample(random))`
   8. Skip predicate for this cave: `shouldSkip(xd, yd, zd, worldY) = (yd <= floorLevel) || ((xd * xd + yd * yd + zd * zd) >= 1.0)`.
   9. `tunnels = 1`.
   10. If `random.nextInt(4) == 0` (25% room):
       - `roomYScale = f64(roomVerticalRadiusMultiplier.sample(random))`.
       - `roomThickness = 1.0_f32 + random.nextFloat() * 6.0_f32`.
       - `createRoom`: `hRad = 1.5 + f64(Mth.sin(f64(1.5707964_f32)) * roomThickness)` (where `1.5707964f = f32(PI / 2)`), `vRad = hRad * roomYScale`, calls `carveEllipsoid(chunkPos, x + 1.0, y, z, hRad, vRad, output, skipChecker)` *(note **`x + 1.0`** offset!)*.
       - `tunnels += random.nextInt(4)`.
   11. For `i = 0 .. tunnels - 1`, draw in exact order:
       - `hRot = random.nextFloat() * 6.2831855_f32` (`f32(2 * PI)`).
       - `vRot = (random.nextFloat() - 0.5_f32) / 4.0_f32`.
       - `thickness = thicknessProvider.sample(random)`; if `weirdThicknessBias && random.nextInt(10) == 0`, `thickness *= random.nextFloat() * random.nextFloat() * 3.0_f32 + 1.0_f32`.
       - `distance = maxDistance - random.nextInt(maxDistance / 4)` (`112 - nextInt(28) ∈ [85, 112]`).
       - `tunnelSeed = random.nextLong()`.
       - Call `createTunnel(chunkPos, tunnelSeed, x, y, z, hMult, vMult, thickness, hRot, vRot, 0, distance, startVMult, output, skipChecker)`.

**`createTunnel`**:
1. Create a fresh **legacy 48-bit LCG** `rng = SingleThreadedRandomSource(tunnelSeed)`.
2. Draw in exact order:
   - `splitPoint = rng.nextInt(dist / 2) + dist / 4`.
   - `steep = (rng.nextInt(6) == 0)`.
3. Initialize `yRota = 0.0_f32`, `xRota = 0.0_f32`.
4. Loop `currentStep = step` while `currentStep < dist`:
   1. `hRad = 1.5 + f64(Mth.sin(f64((3.1415927_f32 * f32(currentStep)) / f32(dist))) * thickness)` (where `3.1415927f = f32(PI)`).
   2. `vRad = hRad * yScale`.
   3. `cosX = Mth.cos(f64(vRot))` (`f32`).
   4. `x += f64(Mth.cos(f64(hRot)) * cosX)`; `y += f64(Mth.sin(f64(vRot)))`; `z += f64(Mth.sin(f64(hRot)) * cosX)`.
   5. Update rotations in `f32`:
      - `vRot *= steep ? 0.92_f32 : 0.7_f32`
      - `vRot += xRota * 0.1_f32`
      - `hRot += yRota * 0.1_f32`
      - `xRota *= 0.9_f32`
      - `yRota *= 0.75_f32`
      - `xRota += ((rng.nextFloat() - rng.nextFloat()) * rng.nextFloat()) * 2.0_f32`
      - `yRota += ((rng.nextFloat() - rng.nextFloat()) * rng.nextFloat()) * 4.0_f32`
   6. **Branching**: if `currentStep == splitPoint && thickness > 1.0_f32`:
      - Draw `seed1 = rng.nextLong()`, `thick1 = rng.nextFloat() * 0.5_f32 + 0.5_f32`, and recurse `createTunnel(..., seed1, x, y, z, hMult, vMult, thick1, hRot - 1.5707964_f32, vRot / 3.0_f32, currentStep, dist, 1.0, output, skipChecker)`.
      - Draw `seed2 = rng.nextLong()`, `thick2 = rng.nextFloat() * 0.5_f32 + 0.5_f32`, and recurse `createTunnel(..., seed2, x, y, z, hMult, vMult, thick2, hRot + 1.5707964_f32, vRot / 3.0_f32, currentStep, dist, 1.0, output, skipChecker)`.
      - Return immediately (does **not** carve at `splitPoint` and does not continue the parent tunnel).
   7. **Carve step**: if `rng.nextInt(4) != 0` (75% of steps):
      - If `!canReach(chunkPos, x, z, currentStep, dist, thickness)`: return immediately.
      - Call `carveEllipsoid(chunkPos, x, y, z, hRad * hMult, vRad * vMult, output, skipChecker)`.

#### 3.5 Canyon algorithm (`CanyonWorldCarver`)
In `carve(context, random, chunkPos, sourceChunkPos, output)`:
1. `maxDistance = (getRange() * 2 - 1) * 16 = 112`.
2. Draw in exact order:
   1. `x = f64(sourceChunkPos.blockX(random.nextInt(16)))`
   2. `y = yProvider.sample(random, context)` (converted to `f64`)
   3. `z = f64(sourceChunkPos.blockZ(random.nextInt(16)))`
   4. `hRot = random.nextFloat() * 6.2831855_f32` (`f32(2 * PI)`)
   5. `vRot = verticalRotation.sample(random)`
   6. `yScale = f64(shape.yScale.sample(random))`
   7. `thickness = shape.thickness.sample(random)`
   8. `distance = trunc_i32(f32(maxDistance) * shape.distanceFactor.sample(random))` (`f32` multiply, truncated toward zero)
   9. `tunnelSeed = random.nextLong()`
3. Call `doCarve(context, chunkPos, tunnelSeed, x, y, z, thickness, hRot, vRot, 0, distance, yScale, output)`.

**`doCarve`**:
1. Create a fresh **legacy 48-bit LCG** `rng = SingleThreadedRandomSource(tunnelSeed)`.
2. **Initialize height width factors (`initWidthFactors`)**:
   - Allocate `widthFactorPerHeight[context.genDepth]`; `wf = 1.0_f32`.
   - For `yIndex = 0 .. context.genDepth - 1`:
     - If `yIndex == 0 || rng.nextInt(shape.widthSmoothness) == 0` (short-circuit: `nextInt` is **not** called at `yIndex == 0`!):
       - `wf = 1.0_f32 + rng.nextFloat() * rng.nextFloat()`.
     - `widthFactorPerHeight[yIndex] = wf * wf`.
3. Initialize `yRota = 0.0_f32`, `xRota = 0.0_f32`.
4. Loop `currentStep = step` while `currentStep < distance`:
   1. `hRad = 1.5 + f64(Mth.sin(f64((f32(currentStep) * 3.1415927_f32) / f32(distance))) * thickness)`.
   2. `vRad = hRad * yScale`.
   3. `hRad *= f64(shape.horizontalRadiusFactor.sample(rng))`.
   4. Update vertical radius (`updateVerticalRadius`):
      - `vMult = 1.0_f32 - abs(0.5_f32 - f32(currentStep) / f32(distance)) * 2.0_f32`.
      - `factor = shape.verticalRadiusDefaultFactor + shape.verticalRadiusCenterFactor * vMult`.
      - `vRad = (f64(factor) * vRad) * f64(Mth.randomBetween(rng, 0.75_f32, 1.0_f32))`.
   5. `xc = Mth.cos(f64(vRot))`, `xs = Mth.sin(f64(vRot))`.
   6. `x += f64(Mth.cos(f64(hRot)) * xc)`; `y += f64(xs)`; `z += f64(Mth.sin(f64(hRot)) * xc)`.
   7. Update rotations in `f32`:
      - `vRot *= 0.7_f32`; `vRot += xRota * 0.05_f32`; `hRot += yRota * 0.05_f32`.
      - `xRota *= 0.8_f32`; `yRota *= 0.5_f32`.
      - `xRota += ((rng.nextFloat() - rng.nextFloat()) * rng.nextFloat()) * 2.0_f32`.
      - `yRota += ((rng.nextFloat() - rng.nextFloat()) * rng.nextFloat()) * 4.0_f32`.
   8. If `rng.nextInt(4) != 0`:
      - If `!canReach(chunkPos, x, z, currentStep, distance, thickness)`: return immediately.
      - Call `carveEllipsoid(chunkPos, x, y, z, hRad, vRad, output, skipChecker)`, where:
        - `yIndex = worldY - context.minGenY`
        - `shouldSkip = ((xd * xd + zd * zd) * f64(widthFactorPerHeight[yIndex - 1]) + (yd * yd) / 6.0) >= 1.0`.
        *(Note: `carveEllipsoid` enforces `worldY >= mask.minY + 1 = minGenY + 2`, so `yIndex - 1 >= 1` always.)*

#### 3.6 `CarvingMask` layout and per-block carve application (`applyCarvingMask`)
`CarvingMask(minY, maxY)` stores a `BitSet` of `256 * height` bits (`height = maxY - minY + 1`):
- Bit index for local `(x, z) ∈ [0, 15]^2` and world `y ∈ [minY, maxY]`:
  $$\text{bit}(x, y, z) = (y - \text{minY}) + (z + (x \ll 4)) \cdot \text{height}$$
- `visit(visitor)` scans contiguous set-bit runs `[startIndex, endIndex]` in ascending bit-index order (i.e. `x` ascending, `z` ascending, `y` ascending), splits any run crossing column boundaries, and invokes `visitor.visitColumn(columnX, columnZ, bottomY, topY)` for each contiguous vertical carved interval `[bottomY, topY]` within a column.

For each contiguous vertical run `(x, z, bottomY, topY)` visited by `mask.visit(...)`, `applyCarvingMask` executes:
1. Initialize `hasGrass = false` (**scoped per contiguous vertical carved run**, not per whole column).
2. `worldX = chunkPos.blockX(x)`, `worldZ = chunkPos.blockZ(z)`.
3. Loop **`worldY = topY` down to `bottomY`** (inclusive, top-down):
   1. If `blenderCarvingFilter != null && !blenderCarvingFilter.test(x, worldY, z)`: skip.
   2. Read `blockState = chunk.getBlockState(BlockPos(worldX, worldY, worldZ))`.
   3. If `blockState.is(#minecraft:uncarvable)`: skip completely.
   4. If `blockState` is `minecraft:grass_block` OR `minecraft:mycelium`: set `hasGrass = true`.
   5. Query `state = noiseChunk.aquifer().computeSubstance(worldX, worldY, worldZ, 0.0)` (**`density = 0.0`**).
   6. If `state != null`:
      - `chunk.setBlockState(blockPos, state)` (updates `OCEAN_FLOOR_WG` and `WORLD_SURFACE_WG`).
      - If `aquifer.shouldScheduleFluidUpdate() && !state.getFluidState().isEmpty()`: `chunk.markPosForPostProcessing(blockPos)`.
      - **Grass/mycelium/dirt surface fix-up**: if `hasGrass` is `true` AND `chunk.getBlockState(BlockPos(worldX, worldY - 1, worldZ))` is `minecraft:dirt`:
        - Call `surfaceSystem.topMaterial(materialRule, randomState, context, correctBiomeManager::getBiome, chunk, noiseChunk.cachingSamplers(), downPos, !state.getFluidState().isEmpty())` (Section 1.7).
        - If non-empty (`topMaterial`):
          - `chunk.setBlockState(downPos, topMaterial)`.
          - If `!topMaterial.getFluidState().isEmpty()`: `chunk.markPosForPostProcessing(downPos)`.

*(Note: when `aquifer` is disabled, `computeSubstance(..., 0.0)` returns `globalFluidPicker.computeFluid(x, y, z).at(y)`, which automatically places `minecraft:lava` below `min(-54, sea_level)`, `default_fluid` below `sea_level`, and `minecraft:air` at/above `sea_level`—replacing the pre-26.3 per-carver `lava_level` logic.)*

---

### 4. Chunk-step order around terrain generation (26.3)

In 26.3, the pre-26.3 statuses `NOISE`, `SURFACE`, and `CARVERS` have been collapsed into a single `ChunkStatus.TERRAIN`. The full `GENERATION_PYRAMID` order is:

| # | `ChunkStatus` | Neighbor requirements (status, radius) | Write radius | Heightmaps after status | What it reads / writes |
|---|---|---|---|---|---|
| 0 | `empty` | — | 0 | `WORLDGEN` (`OCEAN_FLOOR_WG`, `WORLD_SURFACE_WG`) | Empty `ProtoChunk` |
| 1 | `structure_starts` | — | 0 | `WORLDGEN` | Computes & writes `structureStarts` |
| 2 | `structure_references` | `structure_starts` (8) | 0 | `WORLDGEN` | Reads neighbor `structureStarts`, writes `structureReferences` |
| 3 | `biomes` | `structure_starts` (8) | 0 | `WORLDGEN` | Writes section biome palettes via `BiomeSource` (+ `Blender` / `BelowZeroRetrogen`) |
| 4 | **`terrain`** | `structure_starts` (8), `biomes` (1) | 0 | **`FINAL`** (`OCEAN_FLOOR`, `WORLD_SURFACE`, `MOTION_BLOCKING`, `MOTION_BLOCKING_NO_LEAVES`) | Runs `ChunkStatusTasks.buildTerrain` (below) |
| 5 | `features` | `structure_starts` (8), `terrain` (1) | 1 | `FINAL` | Places features & structures, generates `Blender` border ticks |
| 6 | `initialize_light` | — | 0 | `FINAL` | Initializes sky-light sources |
| 7 | `light` | `initialize_light` (1) | 0 | `FINAL` | Computes block & sky light |
| 8 | `spawn` | `biomes` (1) | 0 | `FINAL` | Initial mob spawning (`NaturalSpawner`) |
| 9 | `full` | — | 0 | `FINAL` | Promotes `ProtoChunk` → `LevelChunk`, runs `postLoad` (and `ChunkMap` runs `postProcessGeneration` for queued fluid/block updates) |

Exact sequence inside **`ChunkStatusTasks.buildTerrain`** (step 4):
1. Collect `possibleBiomes` from the `3 × 3` chunk neighborhood (`radius = 1`, all at least `BIOMES` status) by unioning every section's biome palette (`collectBiomesInPalette`).
2. Call `NoiseBasedChunkGenerator.buildTerrain`:
   1. `doFill`: reads `final_density`, `Beardifier` (from `structureStarts`/`structureReferences`), `Blender`, `Aquifer`; writes chunk sections, `OCEAN_FLOOR_WG`, `WORLD_SURFACE_WG`, and `postProcessing`.
   2. `buildSurface`: reads `WORLD_SURFACE_WG`, `BiomeManager` (3D Voronoi zoom over the chunk + radius-1 neighbor biome palettes), `possibleBiomes`, `router.chunkSurfaceLevel`, `MaterialRule` (including `OreVeinRule`); writes chunk sections (via `protoChunk.setBlockState`, live-updating `OCEAN_FLOOR_WG` and `WORLD_SURFACE_WG`) and `postProcessing`.
   3. `generateCarvers` → `applyCarvingMask`: reads uncached `BiomeResolver` at `quartY = 0` for `17 × 17` source chunks (cached on each chunk's `carverBiomeSettings`), `#minecraft:uncarvable`, `Blender.getCarvingFilter()`, `noiseChunk.aquifer()`, `MaterialRule` (`topMaterial` with `BiomeManager` over uncached `BiomeResolver`), `WORLD_SURFACE_WG`; populates an ephemeral `CarvingMask` (not persisted on `ProtoChunk`); writes chunk sections (live-updating `OCEAN_FLOOR_WG` and `WORLD_SURFACE_WG`) and `postProcessing`.
3. If `ProtoChunk.getBelowZeroRetrogen() != null`: runs `BelowZeroRetrogen.replaceOldBedrock` and (if `hasBedrockHoles()`) `applyBedrockMask`.
4. Calls `Heightmap.primeHeightmaps` for the four `FINAL` heightmaps: `MOTION_BLOCKING`, `MOTION_BLOCKING_NO_LEAVES`, `OCEAN_FLOOR`, `WORLD_SURFACE`.
   - Heightmap predicates in 26.3:
     - `WORLD_SURFACE` / `WORLD_SURFACE_WG`: `!state.isAir()`
     - `OCEAN_FLOOR` / `OCEAN_FLOOR_WG`: `state.is(#minecraft:blocks_motion_in_heightmap)`
     - `MOTION_BLOCKING`: `state.is(#minecraft:blocks_motion_in_heightmap) || !state.getFluidState().isEmpty()`
     - `MOTION_BLOCKING_NO_LEAVES`: `state.is(#minecraft:blocks_motion_in_heightmap_no_leaves) || !state.getFluidState().isEmpty()`

---

## Edge cases and known quirks

1. **Live heightmap mutation affects `SteepCondition` across columns**: `buildSurface` loops `x = 0..15` (outer), `z = 0..15` (inner), and `column.setBlock` immediately updates `WORLD_SURFACE_WG`. Consequently, eroded badlands pillars, frozen ocean holes (`AIR` placed on floor), and frozen ocean icebergs change `gradientX` / `gradientZ` seen by subsequent columns `(x, z+1)` and `(x+1, z)`. **[oracle]**
2. **Asymmetric `SteepCondition` gradient signs**: `gradientX = h(min(x+1,15), z) - h(max(x-1,0), z)` and `gradientZ = h(x, min(z+1,15)) - h(x, max(z-1,0))`, and `SteepCondition` checks `gradientX <= -4 || gradientZ >= 4` (steeply sloping down toward +X **or** up toward +Z—i.e., facing East or North only, never West or South). At chunk edges `x ∈ {0, 15}` or `z ∈ {0, 15}`, one endpoint clamps to the edge column (1-block difference instead of 2-block difference).
3. **`getSurfaceDepth` uses truncation toward zero, not floor**: `(int)(noise * 2.75 + 3.0 + rng.nextDouble() * 0.25)` truncates negative values toward `0` (so `-0.9` becomes `0`, triggering `HoleCondition` (`surfaceDepth <= 0`), whereas `-1.1` becomes `-1`).
4. **Surface ceiling lookahead terminates at `endY - 1`**: Scanning down to `protoChunk.minY - 1` hits `void_air` (`!isStone`), setting `nextCeilingStoneY = minY`, so the bottommost solid block at `y = minY` has `stoneDepthBelow = 1`.
5. **Badlands terracotta band RNG quirks**:
   - Orange band loop increments `i` by `nextInt(5) + 1` in the body **and** `i++` in the loop header (step `2..6`).
   - White band loop checks `start - 1 > 0` (strictly `> 0`, so `start == 1` skips `clayBands[0]` and does **not** consume `nextBoolean()`).
   - `getBand` uses Java truncated `% 192` on `y + offset + 192` (always non-negative for `y >= -184`; would index negatively if `y + offset < -192`).
6. **Mixed `f32` vs `f64` scaling in badlands pillars, icebergs, and biome temperature**:
   - `badlandsPillarNoise * 15.0F`, `icebergPillarNoise * 15.0F`, `clayBandsOffsetNoise * 4.0F`, and `FROZEN_TEMPERATURE_NOISE * 7.0F` multiply in **`f32`** before widening to `f64`.
   - `badlandsSurfaceNoise * 8.25`, `badlandsPillarRoofNoise * 1.5`, `icebergSurfaceNoise * 8.25`, and `icebergPillarRoofNoise * 1.5` widen to **`f64`** before multiplying.
   - `Biome.getHeightAdjustedTemperature` divides `x / 8.0F` and `z / 8.0F` in **`f32`** before widening to `f64`.
7. **Frozen ocean iceberg truncation and snow cap**: `(int)extensionTop` and `(int)extensionBottom` truncate toward zero; `snowDepth <= maxSnowDepth` places up to `maxSnowDepth + 1` (`3..6`) snow blocks; the `nextDouble()` calls in the air/water check short-circuit left-to-right so RNG state depends on the exact block states left by the rule walk.
8. **`Biome.temperatureCache` ignores `seaLevel` in its cache key**: The thread-local 1024-entry cache keys solely on `BlockPos.asLong(pos)` (not `seaLevel`). In single-dimension generation `seaLevel` is constant, so this is transparent unless the same `Biome` instance is queried across two dimensions with different sea levels on one thread.
9. **Carvers ALWAYS use legacy 48-bit LCG**: Both the outer `WorldgenRandom` in `generateCarvers` (`setLargeFeatureSeed`) and the inner per-tunnel `SingleThreadedRandomSource(tunnelSeed)` use the 48-bit legacy LCG even in xoroshiro dimensions (Overworld).
10. **Carver ellipsoid asymmetric bounds and Y half-voxel offset**:
    - `minX`/`minZ` subtract `1` before clamping to `0`, whereas `maxX`/`maxZ` do **not** add `1`.
    - `worldY` iterates `maxY` down to `minY + 1` (strict `worldY > minY`), so `y == output.minY` (i.e. `minGenY + 1`) is **never** carved—the lowest carveable Y is `minGenY + 2` (`-62` in Overworld).
    - `xd` and `zd` use `+ 0.5`, while `yd` uses **`- 0.5`**.
11. **Cave room X offset**: `CaveWorldCarver.createRoom` carves at `(x + 1.0, y, z)` rather than `(x, y, z)`, and computes `Mth.sin(f64(f32(PI / 2)))` through the 65,536-entry sine table (`SIN[16384] == 1.0f`).
12. **Cave branching skips carving at `splitPoint` and resets `yScale = 1.0`**: When `currentStep == splitPoint && thickness > 1.0f`, `createTunnel` spawns two branches with `yScale = 1.0` (discarding `start_vertical_radius_multiplier`) and returns immediately without carving or advancing the parent.
13. **Canyon `initWidthFactors` short-circuit and `yIndex - 1` indexing**: At `yIndex == 0`, `random.nextInt(widthSmoothness)` is skipped by short-circuit `||`. `shouldSkip` indexes `widthFactorPerHeight[worldY - minGenY - 1]`.
14. **`applyCarvingMask` `hasGrass` resets per contiguous vertical carved run**: Because `CarvingMask.visit` invokes the visitor once per contiguous vertical run `[bottomY, topY]`, `hasGrass` resets to `false` across any uncarved block separating two carved segments in the same column. Once `true`, `hasGrass` remains `true` for the rest of that vertical run.
15. **Carver grass fix-up runs the full `materialRule` (including ore veins) with uncached `BiomeResolver`**: `applyCarvingMask` queries `surfaceSystem.topMaterial` using `biomeManager.withDifferentSource(uncachedBiomeResolver)` and `possibleBiomes = null`, on a `1 × 1 × 1` volume, with `stoneDepthAbove = 1`, `stoneDepthBelow = 1`.
16. **Uncached biome lookup at `quartY = 0` for carver selection**: `generateCarvers` selects which carvers start in each of the 289 neighboring chunks solely from the uncached biome at `(sourceChunk.minBlockX >> 2, 0, sourceChunk.minBlockZ >> 2)`. **[oracle]**
17. **`CarvingMask` is ephemeral in 26.3**: Unlike earlier versions, 26.3 does not persist `CarvingMask` on `ProtoChunk` or serialize it to chunk NBT.

---

## Constants and IDs

### Registry and tag IDs
| Kind | Identifier |
|---|---|
| Dynamic registries | `minecraft:worldgen/material_rule`, `minecraft:worldgen/material_condition`, `minecraft:worldgen/carver` |
| Built-in dispatch registries | `minecraft:worldgen/material_rule_type`, `minecraft:worldgen/material_condition_type`, `minecraft:worldgen/carver_type` |
| Material rule types (5) | `minecraft:block`, `minecraft:bandlands`, `minecraft:sequence`, `minecraft:condition`, `minecraft:ore_vein` |
| Material condition types (11) | `minecraft:biome`, `minecraft:noise_threshold`, `minecraft:vertical_gradient`, `minecraft:y_above`, `minecraft:water`, `minecraft:temperature`, `minecraft:steep`, `minecraft:not`, `minecraft:hole`, `minecraft:above_preliminary_surface`, `minecraft:stone_depth` |
| Carver types (2) | `minecraft:cave`, `minecraft:canyon` |
| Built-in configured carvers (4) | `minecraft:cave`, `minecraft:cave_extra_underground`, `minecraft:canyon`, `minecraft:nether_cave` |
| Uncarvable block tag | `#minecraft:uncarvable` |
| Heightmap block tags | `#minecraft:blocks_motion_in_heightmap`, `#minecraft:blocks_motion_in_heightmap_no_leaves` |
| Clay bands RNG hash name | `minecraft:clay_bands` (hashed on world root positional factory) |
| Vertical gradient RNG hash names (vanilla) | `minecraft:bedrock_floor`, `minecraft:bedrock_roof`, `minecraft:deepslate` |

### Surface & biome numeric constants
| Constant | Type | Value |
|---|---|---|
| Preliminary surface offset below surface | `i32` | `8` (`floor(preliminarySurface) + surfaceDepth - 8`) |
| `getSurfaceDepth` scale / base / jitter | `f64` | `2.75`, `3.0`, `0.25` |
| `SteepCondition` thresholds | `i32` | `gradientX <= -4 \|\| gradientZ >= 4` |
| `clayBands` table length | `i32` | `192` |
| `clayBandsOffsetNoise` multiplier | `f32` | `4.0_f` |
| Eroded badlands pillar scales / cap | `f64` / `f32` | surface `* 8.25`, pillar coord `* 0.2` value `* 15.0_f`, roof coord `* 0.75` value `* 1.5`, base `64.0`, `min(buf^2 * 2.5, ceil(floor * 50.0) + 24.0)` |
| Frozen ocean iceberg scales / thresholds | `f64` / `f32` | surface `* 8.25`, pillar coord `* 1.28` value `* 15.0_f`, cutoff `1.8`, roof coord `* 1.17` value `* 1.5`, `min(ice^2 * 1.2, ceil(roof * 40.0) + 14.0)`, melt `-2.0`, top cutoff `2.0`, bottom `seaLevel - top - 7.0`, air prob `> 0.01`, water prob `> 0.15` |
| Biome snow height offset & lapse rate | `i32` / `f32` | `snowLevel = seaLevel + 17`, noise coord `/ 8.0_f`, noise `* 8.0_f`, lapse `* 0.05_f / 40.0_f` |
| Biome temperature thresholds | `f32` | `coldEnoughToSnow`: `< 0.15_f`; `shouldMeltFrozenOceanIcebergSlightly`: `> 0.1_f` |
| `FROZEN` temperature modifier constants | `f64` / `f32` | coords `* 0.05`, `* 0.2`, `* 0.09`; large scale `* 7.0_f`; cutoffs `< 0.3`, `< 0.8`; override `0.2_f` |

### Surface noises (`NoiseData`)
| Noise ID | `firstOctave` | `amplitudes` |
|---|---|---|
| `minecraft:surface` | `-6` | `[1.0, 1.0, 1.0]` |
| `minecraft:surface_secondary` | `-6` | `[1.0, 1.0, 0.0, 1.0]` |
| `minecraft:clay_bands_offset` | `-8` | `[1.0]` |
| `minecraft:badlands_pillar` | `-2` | `[1.0, 1.0, 1.0, 1.0]` |
| `minecraft:badlands_pillar_roof` | `-8` | `[1.0]` |
| `minecraft:badlands_surface` | `-6` | `[1.0, 1.0, 1.0]` |
| `minecraft:iceberg_pillar` | `-6` | `[1.0, 1.0, 1.0, 1.0]` |
| `minecraft:iceberg_pillar_roof` | `-3` | `[1.0]` |
| `minecraft:iceberg_surface` | `-6` | `[1.0, 1.0, 1.0]` |
| `minecraft:sulfur_cave_gradient` | `-5` | `[1.0, 0.0, 1.0]` |

### Carver numeric constants and built-in presets
| Constant | Type | Value |
|---|---|---|
| Chunk search radius (`generateCarvers`) | `i32` | `8` (`17 × 17 = 289` source chunks) |
| `WorldCarver.getRange()` / `maxDistance` | `i32` | `4` / `112` blocks |
| `protectedBlocksOnTop` (normal / upgrading) | `i32` | `7` / `0` |
| `f32(PI)` / `f32(PI / 2)` / `f32(2 * PI)` | `f32` | `3.1415927_f` (`0x40490FDB`) / `1.5707964_f` (`0x3FC90FDB`) / `6.2831855_f` (`0x40C90FDB`) |
| Cave tunnel steep decay / normal decay | `f32` | `0.92_f` / `0.7_f` (prob `1/6`) |
| Cave `xRota` / `yRota` decay & step scale | `f32` | `0.9_f`, `0.75_f`; angle step `* 0.1_f`; jitter `* 2.0_f`, `* 4.0_f` |
| Canyon `vRot` / `xRota` / `yRota` decay & step scale | `f32` | `0.7_f`, `0.8_f`, `0.5_f`; angle step `* 0.05_f`; jitter `* 2.0_f`, `* 4.0_f`; `vRad` random factor `[0.75_f, 1.0_f)` |
| Canyon skip Y divisor | `f64` | `6.0` |

Built-in configured carvers (`Carvers`):
- `minecraft:cave` (`cave`): `probability = 0.15f`, `y = uniform(above_bottom(8), absolute(180))`, `count = very_biased_to_bottom(0, 14)`, `thickness = trapezoid(0.0, 3.0, 1.0)`, `weird_thickness_bias = true`, `room_vertical_radius_multiplier = uniform(0.1, 0.9)`, `horizontal_radius_multiplier = uniform(0.7, 1.4)`, `vertical_radius_multiplier = uniform(0.8, 1.3)`, `start_vertical_radius_multiplier = 1.0`, `floor_level = uniform(-1.0, -0.4)`.
- `minecraft:cave_extra_underground` (`cave`): identical to `minecraft:cave` except `probability = 0.07f` and `y = uniform(above_bottom(8), absolute(47))`.
- `minecraft:canyon` (`canyon`): `probability = 0.01f`, `y = uniform(absolute(10), absolute(67))`, `vertical_rotation = uniform(-0.125, 0.125)`, `shape = { distance_factor: uniform(0.75, 1.0), thickness: trapezoid(0.0, 6.0, 2.0), width_smoothness: 3, horizontal_radius_factor: uniform(0.75, 1.0), vertical_radius_default_factor: 1.0, vertical_radius_center_factor: 0.0, y_scale: 3.0 }`.
- `minecraft:nether_cave` (`cave`): `probability = 0.2f`, `y = uniform(absolute(0), below_top(1))`, `count = very_biased_to_bottom(0, 9)`, `thickness = trapezoid(0.0, 6.0, 2.0)`, `weird_thickness_bias = false`, `room_vertical_radius_multiplier = 0.5`, `horizontal_radius_multiplier = 1.0`, `vertical_radius_multiplier = 1.0`, `start_vertical_radius_multiplier = 5.0`, `floor_level = -0.7`.

---

## Oracle probe points

Public classes and methods a black-box parity tool can call to dump intermediate surface and carver state:
- `net.minecraft.world.level.levelgen.material.MaterialSystem`:
  - `buildSurface(randomState, biomeManager, generationContext, protoChunk, noiseChunk, ruleSource, possibleBiomes)`
  - `topMaterial(ruleSource, randomState, generationContext, biomeGetter, chunk, densitySamplers, pos, underFluid)` → `Optional<BlockState>`
  - `preliminarySurfaceFunction()` → `DensityFunction`
- `net.minecraft.world.level.levelgen.material.rule.MaterialRule` / `RuleEvaluator`:
  - `compile(MaterialRuleContext)` → `RuleEvaluator` (`tryApply(int blockX, int blockY, int blockZ)` → `@Nullable BlockState`)
- `net.minecraft.world.level.levelgen.carver.WorldCarver`:
  - `isStartChunk(RandomSource)` → `boolean`
  - `carve(WorldGenerationContext, RandomSource, ChunkPos chunkPos, ChunkPos sourceChunkPos, CarverOutput)` → `boolean`
- `net.minecraft.world.level.chunk.CarvingMask`:
  - `new CarvingMask(int minY, int maxY)` (implements `CarverOutput`; dump carved bits via `visit(CarvingMask.Visitor)`)
- `net.minecraft.world.level.biome.Biome`:
  - `coldEnoughToSnow(BlockPos, int seaLevel)` → `boolean`
  - `shouldMeltFrozenOceanIcebergSlightly(BlockPos, int seaLevel)` → `boolean`
- `net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator`:
  - `buildTerrain(chunk, blender, randomState, structureManager, biomeManager, carverBiomeRegion, possibleBiomes)` → `CompletableFuture<ChunkAccess>`
- `net.minecraft.world.level.chunk.status.ChunkStatusTasks`:
  - `buildTerrain(context, step, chunks, chunk)` → `CompletableFuture<ChunkAccess>` (verifies post-terrain heightmap priming and `BelowZeroRetrogen` handling).

---

## Open questions

1. **[oracle] Live heightmap gradient coupling across `(x, z)` columns in `MaterialSystem.buildSurface`**: Because `buildSurface` mutates `WORLD_SURFACE_WG` via `protoChunk.setBlockState` as each `(x, z)` column finishes (`x` outer, `z` inner), eroded badlands pillars, frozen ocean holes, and frozen ocean icebergs change `gradientX` / `gradientZ` for subsequent columns in the same chunk. **Probe**: Pick a seed/chunk containing an `eroded_badlands` pillar or `frozen_ocean` iceberg bordering a `frozen_peaks` / `snowy_slopes` / `jagged_peaks` biome and verify that `SteepCondition` matches vanilla block-for-block only when `WORLD_SURFACE_WG` is updated live in `(x, z)` order.
2. **[oracle] Uncached `BiomeResolver` at `quartY = 0` in `generateCarvers` across `17 × 17` chunks**: `TERRAIN` only requires `BIOMES` at radius 1 (and `STRUCTURE_STARTS` at radius 8), so `generateCarvers` populates `carverCenterChunk.carverBiome(...)` from `biomeSource.createUncachedResolver(randomState)` at `quartY = 0` rather than reading stored chunk biome palettes. **Probe**: Dump the `(sourcePos, index, isStartChunk)` list and resulting `CarvingMask` for chunks spanning Overworld and Nether biome borders (including radius 2..8 neighbors) to confirm exact agreement with uncached `quartY = 0` lookups.
3. **[oracle] `MaterialRuleContext.possibleBiomes` pruning under 3D Voronoi biome zoom**: `ChunkStatusTasks.buildTerrain` builds `possibleBiomes` from the `3 × 3` chunk biome palettes (`radius = 1`). **Probe**: Verify across `golden-terrain` chunks that `buildSurface` with `possibleBiomes` set produces bit-identical blocks to `buildSurface` with `possibleBiomes = null` (confirming `BiomeManager`'s Voronoi offset never reaches outside the `3 × 3` chunk neighborhood).

---

## Parity status
`golden-terrain` (full chunks after the terrain step, structures off): **1,320/1,320** checks identical on the default
corpus (132 chunks: 3 seeds × normal overworld + nether + end, large-biomes and amplified overworlds, including 2×2
adjacent blocks, far chunks and targeted eroded-badlands, frozen-ocean, sulfur-caves and peak chunks).
Scaled run (`ZENITH_GOLDEN_SEEDS=24 ZENITH_GOLDEN_RANDOM=40`, 2026-11-25): **58,540/58,540** identical over 5,854
chunks. 50-seed soak (2026-11-25): seeds 1–24 with 40 random chunks per dimension plus seeds 25–50 with 60: **147,980/147,980**
checks over 14,798 chunks. An earlier 1,966-chunk run found 3 edge-of-chunk surface blocks that differed because the neighbourhood biomes
came from one shared climate volume instead of each neighbour's own (now `biomes-and-climate.md` edge case 6a).
