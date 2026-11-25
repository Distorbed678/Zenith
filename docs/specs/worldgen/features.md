# Worldgen: features, placement modifiers, and the features chunk step (26.3)

- Verified by suites: `golden-features`
- Sources: Minecraft: Java Edition 26.3 data-pack schemas (`minecraft:worldgen/feature`, `minecraft:worldgen/placed_feature`, `minecraft:worldgen/block_state_provider`, `minecraft:worldgen/biome`), `docs/specs/math/random.md`, `docs/specs/math/noise.md`, `docs/specs/math/java-math-helpers.md`, `docs/specs/codec/datapacks-and-codecs.md`, `docs/specs/registry/identifiers-registries-tags.md`, `docs/specs/worldgen/biomes-and-climate.md`, `docs/specs/worldgen/surface-and-carvers.md`, and local reference reading (2026-11-25) for 26.3 exact schemas, RNG draw order, write-zone bounds, feature sorting, and edge-case quirks. Everything below is written in our own words.
- Related specs:
  - [`docs/specs/math/random.md`](../math/random.md) (legacy 48-bit LCG, xoroshiro128++, positional random factories, Marsaglia Gaussian)
  - [`docs/specs/math/noise.md`](../math/noise.md) (normal/double-Perlin noise, simplex noise, `BIOME_INFO_NOISE`, `TEMPERATURE_NOISE`)
  - [`docs/specs/math/java-math-helpers.md`](../math/java-math-helpers.md) (`Mth` math/random helpers, `BlockPos`/`SectionPos`/`ChunkPos`, trig LUT)
  - [`docs/specs/codec/datapacks-and-codecs.md`](../codec/datapacks-and-codecs.md) (strict JSON, codec coercion, `either`/`xor`, dispatch)
  - [`docs/specs/registry/identifiers-registries-tags.md`](../registry/identifiers-registries-tags.md) (registries, holders, holder sets, block-state codec §3.3, tags)
  - [`docs/specs/worldgen/biomes-and-climate.md`](./biomes-and-climate.md) (biome palettes, `BiomeManager` Voronoi zoom, `BiomeSource`)
  - [`docs/specs/worldgen/surface-and-carvers.md`](./surface-and-carvers.md) (`VerticalAnchor`, heightmap predicates, chunk-step pyramid §4, biome snow/freeze temperature)
  - [`docs/specs/worldgen/features-trees.md`](./features-trees.md) (`minecraft:tree` feature internals: trunk/foliage/root placers, feature sizes, tree decorators)
  - [`docs/specs/worldgen/structures-legacy-pieces.md`](./structures-legacy-pieces.md) (structure piece placement interleaved during the `features` chunk step)

---

## Summary

The `minecraft:features` chunk generation step decorates each chunk after `minecraft:terrain` completes by interleaving **structure piece placement** and **biome-driven placed features** across 11 ordered decoration steps (`raw_generation` through `top_layer_modification`). Decoration runs inside a `3 × 3` writable chunk window (`writeRadius = 1`, with read-only access to `STRUCTURE_STARTS` up to radius 8) using a single reusable `WorldgenRandom` wrapping a `XoroshiroRandomSource` seeded per chunk and re-seeded per `(step, index)`.

Version 26.3 introduces major architectural changes to the feature pipeline relative to earlier versions:

1. **Direct type-dispatched `minecraft:worldgen/feature` registry**: There is no `ConfiguredFeature` wrapper (`{ "type": ..., "config": { ... } }`). Every entry in `minecraft:worldgen/feature` is a flat type-dispatched object (`"type": "<feature_type_id>"` plus its fields directly at top level).
2. **Dynamic `minecraft:worldgen/block_state_provider` registry**: Block state providers are registered in a synchronized world-layer dynamic registry, accept a holder reference string or an inline `BlockState.FULL_CODEC` / type-dispatched object, and use shortened type IDs (`simple`, `weighted`, `noise`, `dual_noise`, `noise_threshold`, `rotated`, `randomized_int`, `rule_based`, `copy_properties`, `random_block`).
3. **Depth-first `FeaturePlacer`**: Placed-feature evaluation uses an explicit stack rather than Java streams, evaluating placement modifiers depth-first while preserving left-to-right position order.
4. **Generalized and refactored features**: Version 26.3 registers exactly **58** feature types and **18** placement modifier types. Several legacy single-purpose feature types (such as `random_patch`, `flower`, `ice_spike`, `forest_rock`, `basalt_columns`, `basalt_pillar`, `glowstone_blob`, `kelp`, `seagrass`, `sea_pickle`, `desert_well`, `coral_mushroom`, `pointed_dripstone`, `dripstone_cluster`, `weeping_vines`, `twisting_vines`, `nether_forest_vegetation`) have been replaced or generalized into composable primitives (`overlay`, `sequence`, `weighted_random_selector`, `template`, `spike`, `block_blob`, `stepped_column_cluster`, `single_block_pillar`, `random_neighbor_spread`, `projected_random_patchy_square`, `speleothem`, `speleothem_cluster`, `cuboid` placement, `offset` placement, `random_chance` placement, `randomly_selected` placement). `CarvingMaskPlacement` no longer exists.

This specification covers:
1. The `FEATURES` chunk step, `WorldGenRegion` bounds, `FeatureSorter` global ordering and cycle detection, decoration seeding, and structure interleaving.
2. `PlacedFeature`, `FeaturePlacer`, all **18** placement modifiers, and all **6** height providers.
3. Shared helpers: all **16** block predicates, all **10** block state providers, all **8** int providers, all **4** float providers, all **10** rule tests, `WeightedList`, and `Column.scan`.
4. All **58** registered feature types (with `minecraft:tree` internals delegated to [`features-trees.md`](./features-trees.md), while covering `fallen_tree`, all selector/combinator features that invoke trees, and all block-side helpers such as chorus growth, sculk spreading, multiface spreading, double-plant/mossy-carpet placement, and speleothem utilities).

---

## Inputs and state

### Arithmetic, coordinate, and iteration conventions
- **Floating-point arithmetic**: IEEE-754 binary32 (`f32`) and binary64 (`f64`), round-to-nearest-even, no FMA; left-to-right associativity unless parenthesized. Integer arithmetic is Java wrapping two's-complement (`i32` / `i64`), `/` truncates toward zero, `>>` is arithmetic shift, `>>>` is logical shift.
- **Math helpers** (defined in [`java-math-helpers.md`](../math/java-math-helpers.md)):
  - `Mth.floor(v)`, `Mth.ceil(v)`: saturating `i32` floor / ceil.
  - `Mth.sqrt(x: f32) -> f32`: `f32(sqrt(f64(x)))`.
  - `Mth.invSqrt(x: f64) -> f64`: `1.0 / sqrt(x)`.
  - `Mth.sin(f64) -> f32`, `Mth.cos(f64) -> f32`: 65,536-entry LUT.
  - `Mth.nextInt(rng, min, max)`: if `min >= max` returns `min` **without drawing RNG**; else `rng.nextInt(max - min + 1) + min`.
  - `Mth.randomBetweenInclusive(rng, min, max)`: **unconditionally** draws `rng.nextInt(max - min + 1) + min` (even when `min == max`).
  - `Mth.randomBetween(rng, min: f32, max: f32)`: **unconditionally** draws `rng.nextFloat() * (max - min) + min`.
  - `Mth.normal(rng, mean: f32, dev: f32)`: `mean + f32(rng.nextGaussian()) * dev`.
  - `Mth.clampedMap(v, f0, f1, t0, t1)`: `clampedLerp(inverseLerp(v, f0, f1), t0, t1)`.
- **`Direction` enum order** (`Direction.values()`): `[DOWN(0), UP(1), NORTH(2), SOUTH(3), WEST(4), EAST(5)]`.
  - `Direction.getRandom(rng)` picks `values()[rng.nextInt(6)]`.
  - `Direction.Plane.HORIZONTAL` order: `[NORTH, EAST, SOUTH, WEST]`. `Plane.HORIZONTAL.getRandomDirection(rng)` picks from those 4 in that order.
  - `Direction.from2DDataValue(i)`: `[SOUTH(0), WEST(1), NORTH(2), EAST(3)]` (note: different order from `Plane.HORIZONTAL`!).
  - `Direction.VERTICAL_CODEC`: accepts `"down"` or `"up"` only.
- **`Rotation` enum order** (`Rotation.values()`): `[NONE("none"), CLOCKWISE_90("clockwise_90"), CLOCKWISE_180("180"), COUNTERCLOCKWISE_90("counterclockwise_90")]`.
  - `Rotation.getRandom(rng)` picks `values()[rng.nextInt(4)]`.
- **Shuffling (`Util.shuffle`, `Util.shuffledCopy`, `Util.toShuffledList`)**:
  - Backward Fisher-Yates shuffle on a list/array of length `n`:
    For `i = n` down to `2` inclusive: draw `swapTo = rng.nextInt(i)` and swap elements `i - 1` and `swapTo`.
  - Note: unlike noise permutation shuffling (which runs forward `0..255` and calls `nextInt(1)` at the end), `Util.shuffle` runs backward `n..2` and stops **before** `i = 1` (so a 1-element list consumes 0 RNG draws).
  - `Direction.allShuffled(rng)` runs `Util.shuffledCopy(Direction.values(), rng)`.
  - `Direction.Plane.HORIZONTAL.shuffledCopy(rng)` runs `Util.shuffledCopy([NORTH, EAST, SOUTH, WEST], rng)`.
- **Distance metrics on `BlockPos` / `Vec3i`**:
  - `a.distSqr(b)` = `a.distToLowCornerSqr(b.x, b.y, b.z)` = `(a.x - b.x)^2 + (a.y - b.y)^2 + (a.z - b.z)^2` in `f64`.
  - `a.closerThan(b, d)` = `a.distSqr(b) < d * d` (strict `<`).
  - `a.distManhattan(b)` = `trunc_i32(f32(|b.x - a.x|) + f32(|b.y - a.y|) + f32(|b.z - a.z|))`.
- **`BlockPos` iteration orders (CRITICAL — features draw RNG inside these loops)**:
  1. **`BlockPos.betweenClosed(a, b)`** (normalizes `[minX..maxX]`, `[minY..maxY]`, `[minZ..maxZ]` inclusive; `width = maxX - minX + 1`, `height = maxY - minY + 1`, `depth = maxZ - minZ + 1`):
     - Iterates `index = 0 .. width * height * depth - 1` with `x = index % width`, `y = (index / width) % height`, `z = (index / width) / height`.
     - Equivalent nested loop order: **`z` outer (ascending), `y` middle (ascending), `x` inner (ascending)**.
  2. **`BlockPos.withinBoxByManhattanDistance(origin, reachX, reachY, reachZ)`**:
     - Iterates Manhattan distance shells `d = 0 .. reachX + reachY + reachZ` inclusive.
     - Within each shell `d`:
       - `x` from `-min(reachX, d)` to `+min(reachX, d)` ascending.
       - For each `x`, `y` from `-min(reachY, d - |x|)` to `+min(reachY, d - |x|)` ascending.
       - Let `z = d - |x| - |y|`. If `z <= reachZ`: emit `(originX + x, originY + y, originZ + z)`; then if `z != 0`, immediately emit `(originX + x, originY + y, originZ - z)`.
  3. **`BlockPos.randomBetweenClosed(rng, limit, minX, minY, minZ, maxX, maxY, maxZ)`**:
     - Let `w = maxX - minX + 1`, `h = maxY - minY + 1`, `d = maxZ - minZ + 1`.
     - Yields `limit` positions lazily; each position draws in exact order:
       `x = minX + rng.nextInt(w)`, `y = minY + rng.nextInt(h)`, `z = minZ + rng.nextInt(d)`.
       (Note: because it is a lazy iterator, draws occur one position per loop iteration, interleaved with any RNG draws inside the loop body!)
  4. **`BoundingBox.forAllCorners(consumer)`**: visits the 8 corners in exact order:
     `(maxX,maxY,maxZ)`, `(minX,maxY,maxZ)`, `(maxX,minY,maxZ)`, `(minX,minY,maxZ)`, `(maxX,maxY,minZ)`, `(minX,maxY,minZ)`, `(maxX,minY,minZ)`, `(minX,minY,minZ)`.

### Critical RNG semantics: `WorldgenRandom` wrapping `XoroshiroRandomSource`

Every call to `ChunkGenerator.applyBiomeDecoration` constructs:
`WorldgenRandom random = new WorldgenRandom(new XoroshiroRandomSource(uniqueSeed))`
and immediately overwrites its seed via `setDecorationSeed` and `setFeatureSeed`.

**CRITICAL PARITY FACT**: `WorldgenRandom` subclasses `LegacyRandomSource` (which implements `BitRandomSource`) and overrides ONLY `next(bits)`, `setSeed(seed)`, `fork()`, and `forkPositional()`. All other `RandomSource` methods inherit `BitRandomSource`'s default implementations rather than calling `XoroshiroRandomSource`'s native methods!

When `WorldgenRandom` wraps a `XoroshiroRandomSource` (`X`):

| Method on `WorldgenRandom` | Exact behavior when backing source is `XoroshiroRandomSource` `X` | Xoroshiro steps consumed |
|---|---|---|
| `setSeed(s)` | Calls `X.setSeed(s)` (128-bit Mix13 upgrade + resets `X`'s unused Gaussian cache). **Does NOT reset `WorldgenRandom`'s own inherited Gaussian cache!** **[oracle]** | 0 |
| `next(bits)` | Increments internal counter; returns `trunc_i32(X.nextLong() >>> (64 - bits))` (top `bits` of one 64-bit Xoroshiro step) | 1 |
| `nextInt()` | `next(32)` = top 32 bits of `X.nextLong()` (**NOT** low 32 bits!) | 1 |
| `nextInt(bound)` | `BitRandomSource` algorithm using `next(31)` (power-of-two fast path `(i64(bound) * next(31)) >> 31`, else modulo rejection `sample - modulo + (bound - 1) < 0`). **NOT** Xoroshiro's multiply-and-reject! | 1+ |
| `nextLong()` | `(i64(next(32)) << 32) + i64(next(32))` (second `i32` sign-extended before add!). Consumes **2** Xoroshiro steps! | 2 |
| `nextBoolean()` | `next(1) != 0` = bit 63 (MSB) of `X.nextLong()` (**NOT** bit 0!) | 1 |
| `nextFloat()` | `f32(next(24)) * 5.9604645e-8_f` (`2^-24`, top 24 bits) | 1 |
| `nextDouble()` | `f64((i64(next(26)) << 27) + i64(next(27))) * 2^-53`. Consumes **2** Xoroshiro steps! | 2 |
| `nextGaussian()` | Marsaglia polar method (see [`random.md`](../math/random.md)) using `WorldgenRandom`'s own `nextDouble()` (4+ Xoroshiro steps per pair) and **its own Gaussian cache that is NOT cleared by `setSeed`!** **[oracle]** | 0 (cache hit) or 4+ |
| `consumeCount(n)` | `n` calls to `nextInt()` (`n` calls to `next(32)`) | `n` |
| `fork()` | `X.fork()` → returns a raw `XoroshiroRandomSource` (NOT wrapped in `WorldgenRandom`) | 2 |
| `forkPositional()` | `X.forkPositional()` → returns `XoroshiroPositionalRandomFactory` | 2 |

*(When `WorldgenRandom` wraps a `LegacyRandomSource`, `next(bits)` forwards directly to `LegacyRandomSource.next(bits)`, so behavior matches standard 48-bit LCG except that `setSeed` still fails to reset `WorldgenRandom`'s own inherited Gaussian cache.)*

#### `WorldgenRandom` seeding methods (64-bit wrapping arithmetic)
1. **`setDecorationSeed(worldSeed: i64, chunkBlockX: i32, chunkBlockZ: i32) -> i64`**:
   1. `this.setSeed(worldSeed)`
   2. `xScale = this.nextLong() | 1L` *(note: calls `WorldgenRandom.nextLong()`, consuming 2 Xoroshiro steps!)*
   3. `zScale = this.nextLong() | 1L` *(2 more Xoroshiro steps!)*
   4. `result = (i64(chunkBlockX) * xScale + i64(chunkBlockZ) * zScale) ^ worldSeed`
   5. `this.setSeed(result)`
   6. Return `result`.
2. **`setFeatureSeed(decorationSeed: i64, index: i32, step: i32)`**:
   - `this.setSeed(decorationSeed + i64(index) + i64(10000 * step))` *(note `10000 * step` is computed in 32-bit `i32`, then sign-extended and added in 64-bit)*.
3. **`setLargeFeatureSeed(seed: i64, chunkX: i32, chunkZ: i32)`**:
   - `this.setSeed(seed)`; `a = this.nextLong()`; `b = this.nextLong()`; `this.setSeed((i64(chunkX) * a) ^ (i64(chunkZ) * b) ^ seed)`.
4. **`setLargeFeatureWithSalt(seed: i64, x: i32, z: i32, salt: i32)`**:
   - `this.setSeed(i64(x) * 341873128712L + i64(z) * 132897987541L + seed + i64(salt))`.

#### `WorldGenRegion.getRandom()`
Separate from the `WorldgenRandom` passed to `Feature.place`: `WorldGenRegion` initializes its own field `this.random = randomState.getOrCreateRandomFactory("minecraft:worldgen_region_random").at(centerChunk.minBlockX, 0, centerChunk.minBlockZ)` (using the dimension's normal `RandomState` positional factory: Xoroshiro for Overworld/End, legacy LCG if `legacy_random_source = true` as in Nether). Only a few feature code paths read `level.getRandom()` (notably `SimpleBlockFeature` when placing `MossyCarpetBlock`).

### Block writing, update flags, and `Feature` default helpers
- **`level.setBlock(pos, state, flags)`** (with default `updateLimit = 512`):
  - `flags = 2` (`UPDATE_CLIENTS`): standard silent placement during worldgen; triggers post-processing marking if `state.getPostProcessPos(level, pos) != null`.
  - `flags = 3` (`UPDATE_ALL` = `UPDATE_NEIGHBORS | UPDATE_CLIENTS`): used by `level.setBlockAndUpdate(pos, state)` and `Feature.setBlock(level, pos, state)`. Inside `WorldGenRegion`, `flags = 2` and `flags = 3` have identical behavior (neither sends neighbor updates during worldgen; both run post-processing checks because bit `16` is clear).
  - `flags = 19` (`UPDATE_ALL | UPDATE_KNOWN_SHAPE`): bit `16` (`UPDATE_KNOWN_SHAPE`) is set, suppressing `WorldGenRegion`'s automatic `getPostProcessPos` marking. Used by tree/fallen-tree placement.
  - `flags = 260` (`UPDATE_NONE` = `UPDATE_INVISIBLE | UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS`): bit `16` is clear (so post-processing still runs), bit `256` skips block-entity side effects. Used by `BlockPileFeature`, `FossilFeature`, `HugeFungusFeature`.
  - `flags = 820` (`UPDATE_INVISIBLE | UPDATE_KNOWN_SHAPE | UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS | UPDATE_SKIP_ON_PLACE`): used by `StructureTemplate.placeInWorld` when pre-placing a temporary `BARRIER` before block-entity blocks.
- **`Feature.setBlock(level, pos, state)`**: calls `level.setBlockAndUpdate(pos, state)` (`flags = 3`).
- **`Feature.safeSetBlock(level, pos, state, canReplace)`**: reads `current = level.getBlockState(pos)`; if `canReplace.test(current)` is `true`, calls `level.setBlock(pos, state, 2)`. Otherwise no-op.
- **`Feature.markAboveForPostProcessing(level, placePos)`**:
  - Starts a mutable pos at `placePos` and loops `i = 0, 1` (at most 2 blocks above `placePos`):
    1. Move up 1 (`y + 1`, then `y + 2`).
    2. If `level.getBlockState(pos).isAir()`, return immediately.
    3. Call `level.getChunk(pos).markPosForPostProcessing(pos)`.

### Registries and codecs
- **Dynamic world-layer registries** (loaded from data packs per [`identifiers-registries-tags.md`](../registry/identifiers-registries-tags.md) §2.4):
  - `minecraft:worldgen/feature` — direct `Feature` entries (dispatched via `minecraft:worldgen/feature_type`).
  - `minecraft:worldgen/placed_feature` — `PlacedFeature` records `{ "feature": Holder<Feature>, "placement": [PlacementModifier, ...] }`.
  - `minecraft:worldgen/block_state_provider` — synchronized dynamic registry of `BlockStateProvider` entries (dispatched via `minecraft:worldgen/block_state_provider_type`, with `BlockState.FULL_CODEC` inline shorthand).
- **Static built-in dispatch registries**:
  - `minecraft:worldgen/feature_type` (58 entries)
  - `minecraft:worldgen/placement_modifier_type` (18 entries)
  - `minecraft:worldgen/height_provider_type` (6 entries)
  - `minecraft:worldgen/block_predicate_type` (16 entries)
  - `minecraft:worldgen/block_state_provider_type` (10 entries)
  - `minecraft:int_provider_type` (8 entries)
  - `minecraft:float_provider_type` (4 entries)
  - `minecraft:rule_test` (10 entries)

---

## Behavior (ordered rules)

### 1. The `FEATURES` chunk step

#### 1.1 Generation region (`WorldGenRegion`) and bounds
In `ChunkPyramid.GENERATION_PYRAMID` (see [`surface-and-carvers.md`](./surface-and-carvers.md) §4), step 5 (`ChunkStatus.FEATURES`) executes `ChunkStatusTasks.generateFeatures`:

1. **Dependencies**:
   - `ChunkStatus.STRUCTURE_STARTS` in Chebyshev radius **8** (`17 × 17` chunks around `center`).
   - `ChunkStatus.TERRAIN` in Chebyshev radius **1** (`3 × 3` chunks around `center`).
   - `blockStateWriteRadius = 1` (`3 × 3` chunks: `|chunkX - centerChunkX| <= 1 && |chunkZ - centerChunkZ| <= 1`).
2. **Region construction**: wraps the dependency cache into a `WorldGenRegion(serverLevel, chunks, step, centerChunk)`.
   - `region.getChunk(cx, cz, targetStatus)` checks Chebyshev distance `d = max(|cx - centerX|, |cz - centerZ|)` against `step.directDependencies()`:
     - For `d <= 1`: max allowed status is `TERRAIN` (the `3 × 3` neighborhood has full terrain, biomes, and `FINAL` heightmaps).
     - For `2 <= d <= 8`: max allowed status is `STRUCTURE_STARTS` (only structure starts are guaranteed; terrain blocks may be ungenerated!).
     - For `d > 8`, or if `targetStatus` is after the max allowed status: throws `ReportedException` ("Requested chunk unavailable during world generation").
3. **Reads outside the write zone**:
   - `getBlockState(pos)`, `getFluidState(pos)`, `getHeight(type, x, z)`, and `findBlocksIn(from, to)` check `isWithinWriteZone(chunkX, chunkZ)` (`|cx - centerX| <= 1 && |cz - centerZ| <= 1`).
   - If outside the `3 × 3` write zone (distance `2..8`), `WorldGenRegion` logs a warning ("Detected unsafe terrain read during worldgen") and **still performs the read** from the cached chunk (if distance `> 8`, `getChunk` throws).
4. **Writes outside the write zone (`ensureCanWrite` & `setBlock`)**:
   - `ensureCanWrite(pos)`:
     1. If `!isWithinWriteZone(pos)` (i.e., `|pos.x >> 4 - centerChunkX| > 1 || |pos.z >> 4 - centerChunkZ| > 1`): logs `"Detected setBlock in a far chunk ..."` and returns **`false`** (does **not** crash, does **not** write any block or block entity).
     2. Else if `center.isUpgrading()` (`BelowZeroRetrogen != null`): returns `!center.getHeightAccessorForGeneration().isOutsideBuildHeight(pos.y)`.
     3. Else: returns **`true`**.
   - `setBlock(pos, blockState, updateFlags, updateLimit)`:
     1. If `!ensureCanWrite(pos)` return `false`.
     2. Calls `oldState = chunk.setBlockState(pos, blockState, updateFlags)`. (Updates all present heightmaps on that chunk—`MOTION_BLOCKING`, `MOTION_BLOCKING_NO_LEAVES`, `OCEAN_FLOOR`, `WORLD_SURFACE`—live as blocks are placed!)
     3. Updates POIs if `oldState != null`.
     4. Block-entity lifecycle:
        - If `blockState.hasBlockEntity()`: on a `LevelChunk`, instantiates `newBlockEntity(pos, blockState)` immediately; on a `ProtoChunk` (normal worldgen), writes a placeholder `DUMMY` NBT compound `{x, y, z, id: "DUMMY"}` onto the chunk (lazily instantiated by `getBlockEntity(pos)` when a feature such as `MonsterRoomFeature`, `BonusChestFeature`, or `EndGatewayFeature` fetches it).
        - Else if `oldState != null && oldState.hasBlockEntity()`: removes the block entity at `pos`.
     5. Post-processing: if `(updateFlags & 16) == 0` (`UPDATE_KNOWN_SHAPE` clear), queries `postProcessPos = blockState.getPostProcessPos(this, pos)`; if non-null, calls `getChunk(postProcessPos).markPosForPostProcessing(postProcessPos)`.
     6. Returns `true`.
   - `getHeight(type, x, z)`: returns `getChunk(x >> 4, z >> 4).getHeight(type, x & 15, z & 15) + 1` (the Y coordinate **above** the highest matching block, or `minY` if the column is empty).
   - `getBrightness(LightLayer.BLOCK, pos)`: during `FEATURES` (before `INITIALIZE_LIGHT`/`LIGHT`), block light is `0` for newly generated proto-chunks, so `level.getBrightness(LightLayer.BLOCK, pos) < 10` is `true` unless the chunk was already lit. **[oracle]**
   - `getEntities(...)` / `players()` on `WorldGenRegion` always return empty lists; `addFreshEntity(entity)` attaches the entity to `getChunk(entity.blockX >> 4, entity.blockZ >> 4)`.
5. **Post-decoration blending**: After `generator.applyBiomeDecoration(region, chunk, structureManager)` returns (skipped if `DEBUG_DISABLE_FEATURES`), `ChunkStatusTasks.generateFeatures` calls `Blender.generateBorderTicks(region, chunk)` (no-op for fresh worlds without old-chunk blending borders).

#### 1.2 Decoration steps (`GenerationStep.Decoration`)
There are **11** decoration steps, indexed `0..10` in enum declaration order:

| Ordinal | Serialized name | Typical contents |
|---|---|---|
| `0` | `raw_generation` | End islands, void start platform |
| `1` | `lakes` | Lava lakes |
| `2` | `local_modifications` | Geodes, icebergs, forest rocks/block blobs, basalt columns/deltas |
| `3` | `underground_structures` | Monster rooms, fossils, mineshafts, strongholds |
| `4` | `surface_structures` | Desert wells, villages, temples, shipwrecks, end spikes/gateways |
| `5` | `strongholds` | Stronghold structures |
| `6` | `underground_ores` | Ores, gravel/granite/diorite/andesite blobs, disks, underwater magma |
| `7` | `underground_decoration` | Glowstone, nether ores, dripstone/speleothem clusters, sculk patches, multiface |
| `8` | `fluid_springs` | Water and lava springs |
| `9` | `vegetal_decoration` | Trees, fallen trees, huge mushrooms/fungi, flowers, grass, kelp, coral, bamboo, vines, seagrass |
| `10` | `top_layer_modification` | `freeze_top_layer`, `fill_layer` (flat worlds) |

#### 1.3 Global feature sorting and cycle detection (`FeatureSorter.buildFeaturesPerStep`)
Every `ChunkGenerator` memoizes `featuresPerStep: List<StepFeatureData>` once from:
- `featureSources = List.copyOf(biomeSource.possibleBiomes())` (preserving the iteration order of the biome source's `Set<Holder<Biome>>`!).
- `featureGetter = biome -> generationSettingsGetter.apply(biome).features()` (each biome supplies `List<HolderSet<PlacedFeature>>`, indexed by decoration step `0..k-1`).
- `tryReducingError = true`.

**Exact algorithm**:
1. Maintain an **identity map** `featureIndex: Reference2IntMap<PlacedFeature>` (compares `PlacedFeature` object identity, **not** value equality!) and counter `nextFeatureIndex = 0`.
2. Define record `FeatureData(featureIndex: i32, step: i32, feature: PlacedFeature)` with total order `comparator`: compare `step` ascending, then `featureIndex` ascending.
3. Allocate `edges: TreeMap<FeatureData, TreeSet<FeatureData>>` sorted by `comparator`, and `maxStep = 0`.
4. For each `biome` in `featureSources` in order:
   1. Let `featuresForStep = featureGetter(biome)`; update `maxStep = max(maxStep, featuresForStep.size())`.
   2. Build flat list `featureList`: for `step = 0 .. featuresForStep.size() - 1`, for each `holder` in `featuresForStep.get(step)` in order:
      - Let `pf = holder.value()`.
      - Assign `idx = featureIndex.computeIfAbsent(pf, nextFeatureIndex++)`.
      - Append `FeatureData(idx, step, pf)` to `featureList`.
   3. For `i = 0 .. featureList.size() - 1`:
      - Ensure `edges` has a `TreeSet(comparator)` entry for `featureList[i]`.
      - If `i < featureList.size() - 1`: insert directed edge `featureList[i] -> featureList[i + 1]` into `edges[featureList[i]]`.
      *(Notice: edges are added **across** step boundaries too—from the last feature of step `s` to the first feature of step `s + 1` in the same biome! Because `FeatureData` compares `(step, featureIndex)`, same-`PlacedFeature` appearing in two different steps creates two distinct graph vertices `(idx, s1, pf)` and `(idx, s2, pf)`.)*
5. **Topological DFS (`Graph.depthFirstSearch`)**:
   - Allocate `discovered = TreeSet(comparator)`, `currentlyVisiting = TreeSet(comparator)`, `sortedFeatures = ArrayList`.
   - Iterate `v` over `edges.keySet()` in ascending `(step, featureIndex)` order:
     - If `v` is not in `discovered`, run recursive DFS:
       1. If `v ∈ discovered`: return `false` (no cycle).
       2. If `v ∈ currentlyVisiting`: return **`true`** (cycle detected!).
       3. Add `v` to `currentlyVisiting`.
       4. For each `next` in `edges[v]` (in ascending `(step, featureIndex)` order): if DFS(`next`) returns `true`, immediately return `true`.
       5. Remove `v` from `currentlyVisiting`, add `v` to `discovered`, append `v` to `sortedFeatures`.
       6. Return `false`.
   - **Cycle error handling**: if DFS returns `true`:
     - If `!tryReducingError`: throw `IllegalStateException("Feature order cycle found")`.
     - Otherwise iteratively minimize `reducedSources` (copy of `featureSources`) by greedily removing each source whose removal still triggers a cycle when re-running `buildFeaturesPerStep(reducedSources, featureGetter, false)` until a fixed point is reached, then throw `IllegalStateException("Feature order cycle found, involved sources: " + reducedSources)`.
6. Reverse `sortedFeatures` in place (producing valid topological order).
7. For each `step = 0 .. maxStep - 1`:
   - Filter `sortedFeatures` for entries with `entry.step == step` (preserving reversed-DFS order), extract their `PlacedFeature` references into `featuresInStep`, and construct `StepFeatureData(featuresInStep, identityIndexLookup(featuresInStep))` (where `indexMapping` maps each `PlacedFeature` by **reference identity** to its `0`-based index within `featuresInStep`).
8. Return the list of `maxStep` `StepFeatureData` entries.

**Critical consequence for RNG parity**: The global index `globalIndexOfFeature` assigned to every `PlacedFeature` in step `s` depends on the **exact iteration order** of `biomeSource.possibleBiomes()` (which sets the `featureIndex` tie-breaking keys in `TreeMap`/`TreeSet`)! **[oracle]** (See Open questions #1.)

#### 1.4 Exact execution of `ChunkGenerator.applyBiomeDecoration`
Skipped completely if `SharedConstants.debugVoidTerrain(centerPos)` is true.
Note generator overrides:
- **`NoiseBasedChunkGenerator`**: inherits `ChunkGenerator.applyBiomeDecoration`.
- **`FlatLevelSource`**: inherits `ChunkGenerator.applyBiomeDecoration`, but supplies a custom `generationSettingsGetter` (`FlatLevelGeneratorSettings.adjustGenerationSettings`): for the configured flat biome, conditionally prepends `lakes` in step `LAKES` (if `addLakes`), copies biome features except steps `UNDERGROUND_STRUCTURES`, `SURFACE_STRUCTURES`, and (when `addLakes`) `LAKES` (if `decoration && (!voidGen || biome == minecraft:the_void)`), and converts every non-`MOTION_BLOCKING` flat layer `i` into an inline `FillLayerFeature(i, layerState)` in step `TOP_LAYER_MODIFICATION`.
- **`DebugLevelSource`**: **overrides** `applyBiomeDecoration` to place `minecraft:barrier` at `y = 60` and all registered block states in a 2-block-spaced grid at `y = 70` (positive odd `worldX`, `worldZ`), skipping normal features and structures entirely.

For normal `ChunkGenerator.applyBiomeDecoration(level, chunk, structureManager)`:
1. `centerPos = chunk.getPos()`.
2. `sectionPos = SectionPos.of(centerPos, level.getMinSectionY())`.
3. `origin = sectionPos.origin()` = `BlockPos(centerPos.minBlockX, level.minSectionY << 4, centerPos.minBlockZ)` = `(centerX * 16, minY, centerZ * 16)`.
4. Collect all registered structures from `Registries.STRUCTURE` in **registry numeric ID order** (`structuresRegistry.stream()`) and group them into `structuresByStep: Map<stepOrdinal, List<Structure>>` preserving registry order within each step.
5. Obtain `featureList = this.featuresPerStep.get()` (`featureStepCount = featureList.size()`).
6. Instantiate `random = new WorldgenRandom(new XoroshiroRandomSource(uniqueSeed))`.
7. Compute chunk decoration seed:
   `decorationSeed = random.setDecorationSeed(level.getSeed(), origin.x, origin.z)`
   *(uses `origin.x = centerX * 16` and `origin.z = centerZ * 16` in block coordinates, NOT chunk coordinates!)*.
8. Collect `possibleBiomes` present in the **`3 × 3` chunk neighborhood**:
   - Initialize empty `Set<Holder<Biome>> possibleBiomes`.
   - Iterate `ChunkPos.rangeClosed(centerPos, 1)` (9 chunks, `dz = -1..1`, `dx = -1..1`).
   - For each chunk, for each `LevelChunkSection`, add every biome present in that section's biome palette (`section.getBiomes().getAll(possibleBiomes::add)`).
   - Intersect with the biome source's possible biomes: `possibleBiomes.retainAll(biomeSource.possibleBiomes())`.
     *(Important: any biome stored in a chunk palette that is NOT in `biomeSource.possibleBiomes()`—for example from chunk blending or retrogen—is excluded here!)*
9. Instantiate `placer = new FeaturePlacer(level, this)`.
10. Let `generationSteps = max(11, featureStepCount)`.
11. Loop `stepIndex = 0 .. generationSteps - 1`:
    1. **Structures for this step** (only if `structureManager.shouldGenerateStructures()` is `true`, i.e., `WorldOptions.generateStructures` is enabled):
       - Initialize `index = 0`.
       - For each `structure` in `structuresByStep.getOrDefault(stepIndex, empty)` (in `Registries.STRUCTURE` ID order):
         1. `random.setFeatureSeed(decorationSeed, index, stepIndex)`.
         2. Look up all valid `StructureStart`s referenced by `centerPos` for `structure`:
            `structureManager.startsForStructure(centerPos.x, centerPos.z, structure)` (reads `centerChunk.getReferencesForStructure(structure)` — a `LongSet` of packed start `ChunkPos`es — and fetches each start from its start chunk at status `STRUCTURE_STARTS`).
         3. For each valid `start`: call `start.placeInChunk(level, structureManager, this, random, getWritableArea(chunk), centerPos)`, where `getWritableArea(chunk)` is `BoundingBox(minBlockX, minY + 1, minBlockZ, minBlockX + 15, maxY, minBlockZ + 15)` (notice `minY + 1`, protecting the bottommost Y layer such as bedrock!).
            *(Structure piece contents and internal RNG usage are specified in the structure specs, e.g. [`structures-legacy-pieces.md`](./structures-legacy-pieces.md).)*
         4. `index += 1` (incremented for **every** registered structure in `stepIndex`, even if zero starts intersect `centerPos`!).
    2. **Features for this step** (only if `stepIndex < featureStepCount`):
       - Build an `IntSet possibleFeaturesThisStep`:
         - For each `biome` in `possibleBiomes`:
           - Let `featuresInBiome = generationSettingsGetter.apply(biome).features()`.
           - If `stepIndex < featuresInBiome.size()`:
             - For each `holder` in `featuresInBiome.get(stepIndex)`:
               - Look up `globalIdx = featureList[stepIndex].indexMapping(holder.value())` (by reference identity) and insert `globalIdx` into `possibleFeaturesThisStep`.
       - Copy `possibleFeaturesThisStep` to an `int[]` array and sort ascending (`Arrays.sort`).
       - For each `globalIndexOfFeature` in that sorted array:
         1. `placedFeature = featureList[stepIndex].features().get(globalIndexOfFeature)`.
         2. `random.setFeatureSeed(decorationSeed, globalIndexOfFeature, stepIndex)`.
            *(Notice: structures use `index = 0, 1, ...` and features ALSO start at `globalIndexOfFeature = 0, 1, ...` for the SAME `stepIndex`, so structure `k` and feature `k` in the same step receive the same initial seed—this is intentional vanilla behavior!)*
         3. Call `placer.placeWithBiomeCheck(placedFeature, random, origin)`.
            *(Features whose biome check fails via `BiomeFilter` still execute every placement modifier listed **before** `minecraft:biome`, consuming RNG, and are simply dropped when `BiomeFilter` returns `false`!)*

---

### 2. Placed features, `FeaturePlacer`, placement modifiers, and height providers

#### 2.1 `PlacedFeature` and `FeaturePlacer` execution
- **Schema (`minecraft:worldgen/placed_feature`)**:
  - `feature`: `Holder<Feature>` (required; registry ID string or inline `Feature` object).
  - `placement`: `List<PlacementModifier>` (required; may be empty `[]`).
- **Context (`PlacementContext extends WorldGenerationContext`)**:
  - `minGenY = max(level.minY, generator.minY)`, `genDepth = min(level.height, generator.genDepth)`, `seaLevel = generator.seaLevel`.
  - `topFeature`: `Optional.of(placedFeature)` when invoked via `placeWithBiomeCheck` (from `applyBiomeDecoration`); `Optional.empty()` when invoked via `place` (from sub-features such as `random_selector`, `overlay`, `sequence`, `vegetation_patch`, `root_system`, `coral_*`, `single_block_pillar`).
- **Exact execution (`FeaturePlacer.place(placedFeature, random, origin, biomeCheck)`)**:
  1. If `placement.isEmpty()`: call `feature.place(level, generator, random, origin)` directly and return its boolean result.
  2. Otherwise, initialize `placedAny = false`, a LIFO position stack `positions = [origin]`, a parallel LIFO index stack `modifierIndices = [0]`, and a temporary list `modifiedPositions = []`.
  3. While `positions` is non-empty:
     1. Pop `pos = positions.pop()` and `modifierIndex = modifierIndices.pop()`.
     2. Run `placement[modifierIndex].modify(context, random, pos, modifiedPositions::add)`.
     3. Let `nextModifierIndex = modifierIndex + 1`.
     4. If `nextModifierIndex < placement.size()`:
        - Push elements of `modifiedPositions` onto `positions` (with `nextModifierIndex` onto `modifierIndices`) in **reverse order** (`i = modifiedPositions.size() - 1` down to `0`).
        - Because it is a LIFO stack, the **first** emitted position is popped next, producing **left-to-right depth-first** evaluation!
     5. Else (final modifier in the chain):
        - For each `nextPos` in `modifiedPositions` in forward order:
          `placedAny |= feature.place(level, generator, random, nextPos)`.
     6. Clear `modifiedPositions`.
  4. Return `placedAny`.

**Why depth-first vs breadth-first matters for RNG**: Suppose modifier 0 is `count(4)` and modifier 1 is `in_square`, followed by `height_range` and `feature.place`. Depth-first evaluates `(in_square -> height_range -> feature.place)` for copy #1, then `(in_square -> height_range -> feature.place)` for copy #2, etc. This matches pre-26.3 lazy Java `Stream.flatMap` execution order exactly, **except** when a single modifier emits multiple positions (`count`, `count_on_every_layer`, `cuboid`, `fixed_placement`), all of that one modifier's outputs for a given input position are collected into `modifiedPositions` before the next modifier runs on the first output.

#### 2.2 All 18 placement modifier types (`minecraft:worldgen/placement_modifier_type`)

Filters implement `PlacementFilter`: emit `origin` unchanged if `shouldPlace(...)` returns `true`, else emit nothing.
Repeaters implement `RepeatingPlacement`: compute `n = count(random, origin)` and emit `origin` `n` times (emitting nothing if `n <= 0`).

1. **`minecraft:biome` (`BiomeFilter`)** — *filter*:
   - **JSON fields**: none.
   - **Behavior**: unwraps `topFeature = context.topFeature()` (throws `IllegalStateException` if absent—so `biome` filter may only appear on top-level chunk-decorated placed features, never inside sub-features invoked via `PlacedFeature.place`). Looks up `biome = level.getBiome(origin)` (using `BiomeManager` 3D Voronoi zoom at `origin`). Passes iff `generator.getBiomeGenerationSettings(biome).hasFeature(topFeature)` (using `PlacedFeature` `Set.contains`, i.e., record value equality).
   - **RNG**: none.

2. **`minecraft:block_predicate_filter` (`BlockPredicateFilter`)** — *filter*:
   - **JSON fields**: `predicate`: `BlockPredicate` (required).
   - **Behavior**: passes iff `predicate.test(level, origin)`.
   - **RNG**: none.

3. **`minecraft:count` (`CountPlacement`)** — *repeater*:
   - **JSON fields**: `count`: `IntProvider` validated in `[0, 4096]` (required; note upper bound increased from 256 to 4096 in 26.3).
   - **Behavior**: `n = count.sample(random)`, emits `origin` `n` times.
   - **RNG**: 1 sample of `count`.

4. **`minecraft:count_on_every_layer` (`CountOnEveryLayerPlacement`, deprecated)**:
   - **JSON fields**: `count`: `IntProvider` validated in `[0, 256]` (required).
   - **Behavior**:
     1. Set `layer = 0`.
     2. Loop (`do { ... } while (foundAny)`):
        - Set `foundAny = false`.
        - Loop `i = 0`; **on every loop condition test**, evaluate `i < this.count.sample(random)`! *(CRITICAL QUIRK: `this.count.sample(random)` is re-evaluated on every iteration including the terminating check, NOT cached once per layer!)*
          1. `x = random.nextInt(16) + origin.x`
          2. `z = random.nextInt(16) + origin.z`
          3. `startY = context.getHeight(MOTION_BLOCKING, x, z)`
          4. `y = findOnGroundYPosition(context, x, startY, z, layer)`
          5. If `y != i32::MAX`: emit `(x, y, z)`, set `foundAny = true`.
        - `layer += 1`.
   - **`findOnGroundYPosition(context, x, yStart, z, targetLayer)`**:
     - Define `isEmpty(s) = s.isAir() || s.is(minecraft:water) || s.is(minecraft:lava)`.
     - Set `currentLayer = 0`, `currentBlock = getBlockState(x, yStart, z)`.
     - For `y = yStart` down to `context.minY + 1` inclusive:
       - `belowBlock = getBlockState(x, y - 1, z)`.
       - If `!isEmpty(belowBlock) && isEmpty(currentBlock) && !belowBlock.is(minecraft:bedrock)`:
         - If `currentLayer == targetLayer`: return `y` (= `(y - 1) + 1`).
         - `currentLayer += 1`.
       - `currentBlock = belowBlock`.
     - Return `i32::MAX`.

5. **`minecraft:cuboid` (`CuboidPlacement`)** — *new in 26.3*:
   - **JSON fields**:
     - `xz_size`: `IntProvider` in `[1, 16]` (required).
     - `y_size`: `IntProvider` in `[1, 16]` (required).
     - `include_edges`: `bool` (optional, default `true`).
     - `include_interior`: `bool` (optional, default `true`).
   - **RNG draw order**:
     1. `height = y_size.sample(random)`
     2. `width = xz_size.sample(random)`
     3. `length = xz_size.sample(random)`
   - **Emission order** (inclusive bounds `0..=width`, `0..=height`, `0..=length` — note total box dimensions are `(width+1) × (height+1) × (length+1)`!):
     - Outer `x = 0 ..= width`, middle `y = 0 ..= height`, inner `z = 0 ..= length`:
       - Let `onX = (x == 0 || x == width)`, `onY = (y == 0 || y == height)`, `onZ = (z == 0 || z == length)`.
       - Emit `(origin.x + x, origin.y + y, origin.z + z)` iff:
         - `(include_edges || !(onX && onY)) && (include_edges || !(onZ && onY)) && (include_edges || !(onX && onZ))` (when `include_edges == false`, excludes the 12 edges and 8 corners where 2+ coordinates lie on the boundary), AND
         - `(include_interior || onX || onY || onZ)` (when `include_interior == false`, excludes strictly interior cells).

6. **`minecraft:environment_scan` (`EnvironmentScanPlacement`)**:
   - **JSON fields**:
     - `direction_of_search`: `Direction` (`"up"` or `"down"` only, required).
     - `target_condition`: `BlockPredicate` (required).
     - `allowed_search_condition`: `BlockPredicate` (optional, default `true`).
     - `max_steps`: `i32` in `[1, 32]` (required).
   - **Behavior**:
     1. Set `pos = origin`.
     2. If `!allowed_search_condition.test(level, pos)`: emit nothing and return.
     3. Loop `i = 0 .. max_steps - 1`:
        - If `target_condition.test(level, pos)`: emit `pos` and return.
        - `pos = pos.relative(direction_of_search)`.
        - If `level.isOutsideBuildHeight(pos.y)`: emit nothing and return.
        - If `!allowed_search_condition.test(level, pos)`: break loop.
     4. After loop (either `max_steps` reached or `allowed_search_condition` failed on the moved `pos`): if `target_condition.test(level, pos)`, emit `pos`.
   - **RNG**: none.

7. **`minecraft:fixed_placement` (`FixedPlacement`)**:
   - **JSON fields**: `positions`: `List<BlockPos>` (required; each `BlockPos` is a 3-int JSON array `[x, y, z]`).
   - **Behavior**: let `cx = origin.x >> 4`, `cz = origin.z >> 4`. For each `p` in `positions` in order, if `(p.x >> 4) == cx && (p.z >> 4) == cz`, emit `p`.
   - **RNG**: none.

8. **`minecraft:height_range` (`HeightRangePlacement`)**:
   - **JSON fields**: `height`: `HeightProvider` (required).
   - **Behavior**: `y = height.sample(random, context)`; emit `(origin.x, y, origin.z)`.
   - **RNG**: whatever `height` consumes.

9. **`minecraft:heightmap` (`HeightmapPlacement`)**:
   - **JSON fields**: `heightmap`: `Heightmap.Types` (`"WORLD_SURFACE_WG"`, `"WORLD_SURFACE"`, `"OCEAN_FLOOR_WG"`, `"OCEAN_FLOOR"`, `"MOTION_BLOCKING"`, `"MOTION_BLOCKING_NO_LEAVES"`, required).
   - **Behavior**: `h = context.getHeight(heightmap, origin.x, origin.z)`. If `h > context.minY` (strict `>`), emit `(origin.x, h, origin.z)`. Otherwise emit nothing.
   - **RNG**: none.

10. **`minecraft:in_square` (`InSquarePlacement`)**:
    - **JSON fields**: none.
    - **Behavior**: `x = random.nextInt(16) + origin.x`; `z = random.nextInt(16) + origin.z`; emit `(x, origin.y, z)`.
    - **RNG**: `nextInt(16)` (X), then `nextInt(16)` (Z).

11. **`minecraft:noise_based_count` (`NoiseBasedCountPlacement`)** — *repeater*:
    - **JSON fields**:
      - `noise_to_count_ratio`: `i32` (required).
      - `noise_factor`: `f64` (required).
      - `noise_offset`: `f64` (optional, default `0.0`).
    - **Behavior**:
      - `flowerNoise = f64(Biome.BIOME_INFO_NOISE.get(f64(origin.x) / noise_factor, f64(origin.z) / noise_factor))` *(note `BIOME_INFO_NOISE` is single-octave simplex noise seeded with `LegacyRandomSource(2345L)` and discarded offsets $S = 0.0$; see [`noise.md`](../math/noise.md))*.
      - `n = trunc_i32(ceil((flowerNoise + noise_offset) * f64(noise_to_count_ratio)))`.
      - Emit `origin` `n` times (0 times if `n <= 0`).
    - **RNG**: none.

12. **`minecraft:noise_threshold_count` (`NoiseThresholdCountPlacement`)** — *repeater*:
    - **JSON fields**:
      - `noise_level`: `f64` (required).
      - `below_noise`: `i32` (required).
      - `above_noise`: `i32` (required).
    - **Behavior**:
      - `flowerNoise = f64(Biome.BIOME_INFO_NOISE.get(f64(origin.x) / 200.0, f64(origin.z) / 200.0))`.
      - `n = (flowerNoise < noise_level) ? below_noise : above_noise`.
      - Emit `origin` `n` times.
    - **RNG**: none.

13. **`minecraft:offset` (`OffsetPlacement`)** — *replaces pre-26.3 `random_offset`*:
    - **JSON fields**: `x`, `y`, `z`: each `IntProvider` validated in `[-16, 16]` (all 3 required).
    - **Behavior**: draw `dx = x.sample(random)`, `dy = y.sample(random)`, `dz = z.sample(random)` in exact **`x`, then `y`, then `z`** order; emit `origin.offset(dx, dy, dz)`.

14. **`minecraft:random_chance` (`RandomChancePlacement`)** — *filter, new in 26.3*:
    - **JSON fields**: `chance`: `f32` in `[0.0, 1.0]` (required).
    - **Behavior**: passes iff `random.nextFloat() < chance`.

15. **`minecraft:randomly_selected` (`RandomlySelectedPlacement`)** — *new in 26.3*:
    - **JSON fields**: `placements`: non-empty `List<PlacementModifier>` (required).
    - **Behavior**: draws `idx = random.nextInt(placements.size())` and delegates to `placements[idx].modify(context, random, origin, output)`.

16. **`minecraft:rarity_filter` (`RarityFilter`)** — *filter*:
    - **JSON fields**: `chance`: positive `i32` (`>= 1`, required).
    - **Behavior**: passes iff `random.nextFloat() < 1.0_f32 / f32(chance)` *(exact `f32` division `1.0F / (float)chance`)*.

17. **`minecraft:surface_relative_threshold_filter` (`SurfaceRelativeThresholdFilter`)** — *filter*:
    - **JSON fields**:
      - `heightmap`: `Heightmap.Types` (required).
      - `min_inclusive`: `i32` (optional, default `i32::MIN`).
      - `max_inclusive`: `i32` (optional, default `i32::MAX`).
    - **Behavior**: using **`i64`** arithmetic to avoid overflow: `surfaceY = i64(context.getHeight(heightmap, origin.x, origin.z))`; passes iff `surfaceY + i64(min_inclusive) <= i64(origin.y) && i64(origin.y) <= surfaceY + i64(max_inclusive)`.
    - **RNG**: none.

18. **`minecraft:surface_water_depth_filter` (`SurfaceWaterDepthFilter`)** — *filter*:
    - **JSON fields**: `max_water_depth`: `i32` (required).
    - **Behavior**: `ocean = context.getHeight(OCEAN_FLOOR, origin.x, origin.z)`; `surface = context.getHeight(WORLD_SURFACE, origin.x, origin.z)`; passes iff `surface - ocean <= max_water_depth`.
    - **RNG**: none.

#### 2.3 All 6 `HeightProvider` types (`minecraft:height_provider_type`)

Decoding uses `Codec.either(VerticalAnchor.CODEC, dispatch)` — a bare `VerticalAnchor` object (`{"absolute": y}`, `{"above_bottom": n}`, `{"below_top": n}`, `{"relative_to_sea_level": n}`; see [`surface-and-carvers.md`](./surface-and-carvers.md) §1.2) decodes as `ConstantHeight`.

| Type ID | JSON fields | Exact sampling rule (`sample(random, context) -> i32`) |
|---|---|---|
| `minecraft:constant` | `value`: `VerticalAnchor` (required) | Returns `value.resolveY(context)`. No RNG. |
| `minecraft:uniform` | `min_inclusive`, `max_inclusive`: `VerticalAnchor` (required) | Resolve `min`, `max`. If `min > max`, log warning once per `(min, max)` pair and return `min` (no RNG). Else return `Mth.randomBetweenInclusive(random, min, max)` (1 `nextInt(max - min + 1)` draw even if `min == max`). |
| `minecraft:biased_to_bottom` | `min_inclusive`, `max_inclusive`: `VerticalAnchor` (required); `inner`: `i32 >= 1` (optional, default `1`) | Resolve `min`, `max`. If `max - min - inner + 1 <= 0`, warn and return `min` (no RNG). Else `limit = random.nextInt(max - min - inner + 1)`; return `random.nextInt(limit + inner) + min` (2 `nextInt` draws). |
| `minecraft:very_biased_to_bottom` | `min_inclusive`, `max_inclusive`: `VerticalAnchor` (required); `inner`: `i32 >= 1` (optional, default `1`) | Resolve `min`, `max`. If `max - min - inner + 1 <= 0`, warn and return `min` (no RNG). Else: `u = Mth.nextInt(random, min + inner, max)`; `b = Mth.nextInt(random, min, u - 1)`; return `Mth.nextInt(random, min, b - 1 + inner)`. *(Note `Mth.nextInt` skips RNG when `lo >= hi`!)* |
| `minecraft:trapezoid` | `min_inclusive`, `max_inclusive`: `VerticalAnchor` (required); `plateau`: `i32` (optional, default `0`) | Resolve `min`, `max`. If `min > max`, warn and return `min` (no RNG). Let `range = max - min`. If `plateau >= range`: return `Mth.randomBetweenInclusive(random, min, max)` (1 draw). Else: `pStart = (range - plateau) / 2` (truncating `i32` div), `pEnd = range - pStart`; return `min + Mth.randomBetweenInclusive(random, 0, pEnd) + Mth.randomBetweenInclusive(random, 0, pStart)` (2 draws, `pEnd` first, then `pStart`). |
| `minecraft:weighted_list` | `distribution`: non-empty `WeightedList<HeightProvider>` (required) | `distribution.getRandomOrThrow(random).sample(random, context)`. |

---

### 3. Shared helpers

#### 3.1 All 16 `BlockPredicate` types (`minecraft:block_predicate_type`)
`offset` (where present) uses `Vec3i.offsetCodec(16)` (3-int array `[x, y, z]`, each `|c| <= 16`, optional, default `[0, 0, 0]`), except `unobstructed` which uses unconstrained `Vec3i.CODEC`.

| Type ID | JSON fields | Exact evaluation (`test(level, origin) -> bool`) |
|---|---|---|
| `minecraft:all_of` | `predicates`: `List<BlockPredicate>` (required) | Short-circuiting logical AND in list order (`true` if empty). |
| `minecraft:any_of` | `predicates`: `List<BlockPredicate>` (required) | Short-circuiting logical OR in list order (`false` if empty). |
| `minecraft:not` | `predicate`: `BlockPredicate` (required) | `!predicate.test(level, origin)`. |
| `minecraft:true` | *(none)* | Always `true`. |
| `minecraft:matching_blocks` | `offset`?, `blocks`: `HolderSet<Block>` (required) | `level.getBlockState(origin + offset).is(blocks)`. |
| `minecraft:matching_block_tag` | `offset`?, `tag`: bare `TagKey<Block>` (no `#`, required) | `level.getBlockState(origin + offset).is(tag)`. |
| `minecraft:matching_fluids` | `offset`?, `fluids`: `HolderSet<Fluid>` (required) | `level.getBlockState(origin + offset).getFluidState().is(fluids)`. |
| `minecraft:matching_biomes` | `biomes`: `HolderSet<Biome>` (required) | `biomes.contains(level.getBiome(origin))` *(new in 26.3; no `offset` field)*. |
| `minecraft:has_sturdy_face` | `offset`?, `direction`: `Direction` (required) | Let `p = origin + offset`; `level.getBlockState(p).isFaceSturdy(level, p, direction)`. |
| `minecraft:solid` *(deprecated)* | `offset`? | `level.getBlockState(origin + offset).isSolid()`. |
| `minecraft:replaceable` | `offset`? | `level.getBlockState(origin + offset).canBeReplaced()`. |
| `minecraft:would_survive` | `offset`?, `state`: `BlockState` (required) | `state.canSurvive(level, origin + offset)`. |
| `minecraft:inside_world_bounds` | `offset`? | `level.isInsideBuildHeight(origin + offset)`. |
| `minecraft:unobstructed` | `offset`: `Vec3i` (optional, default `[0,0,0]`) | Returns `level.isUnobstructed(null, Shapes.block().move(origin))`. **QUIRK**: `offset` is decoded and stored but **completely ignored** at test time! |
| `minecraft:height_range` | `min_inclusive`, `max_inclusive`: `VerticalAnchor` (required) | *(New in 26.3.)* Constructs `ctx = WorldGenerationContext.of(level)` (when `level` is `WorldGenLevel`, uses `level.getLevel().getChunkSource().getGenerator()` + `level`); resolves `min`, `max`; returns `origin.y >= min && origin.y <= max`. |
| `minecraft:volume_match` | `min`, `max`: `Vec3i` in `[-16, 16]` (validated `min.x <= max.x && min.y <= max.y && min.z <= max.z`), `match`: `BlockPredicate` (all required) | *(New in 26.3.)* Loops `ox = min.x..=max.x`, `oz = min.z..=max.z`, `oy = min.y..=max.y`: returns `false` on the first offset where `!match.test(level, origin + (ox, oy, oz))`; else `true`. |

#### 3.2 All 10 `BlockStateProvider` types (`minecraft:worldgen/block_state_provider_type`)
In 26.3, `BlockStateProvider` is a synchronized dynamic registry (`minecraft:worldgen/block_state_provider`).
- **Codec (`BlockStateProvider.CODEC`)**: `RegistryCodecs.holder(minecraft:worldgen/block_state_provider, DIRECT_CODEC)`.
  - A JSON string is resolved as a registry reference ID in `minecraft:worldgen/block_state_provider`.
  - A JSON object decodes via `DIRECT_CODEC = Codec.xor(BlockState.FULL_CODEC, TYPED_CODEC)`:
    - If it matches `BlockState.FULL_CODEC` (`{"id": "<block_id>", "properties": {...}}`, no `"type"` key), wraps it in `SimpleStateProvider`.
    - Otherwise dispatches on `"type"` via `minecraft:worldgen/block_state_provider_type`.
- **Two query methods**:
  - `getState(level, random, pos) -> BlockState`: always returns a non-null `BlockState`.
  - `getOptionalState(level, random, pos) -> BlockState?`: defaults to `getState`, but `rule_based` and `random_block` may return `null` (which `DiskFeature`, `SimpleBlockFeature`, and `ProjectedRandomPatchySquare` skip without placing a block).
- **Noise-based providers (`noise`, `dual_noise`, `noise_threshold`)** share base fields:
  - `seed`: `i64` (required)
  - `noise`: inline `NormalNoise` parameters (`NormalNoise.DIRECT_CODEC`; see [`noise.md`](../math/noise.md) §7, required)
  - `scale`: positive `f32` (`> 0.0`, required)
  - At construction, instantiates `noise = parameters.create(new WorldgenRandom(new LegacyRandomSource(seed)))`.
  - `getNoiseValue(pos, scale_f64)` = `noise.get(f64(pos.x) * scale_f64, f64(pos.y) * scale_f64, f64(pos.z) * scale_f64)` (returns `f32`). Note `this.scale` (`f32`) widens to `f64` when passed as `scale_f64`!

All 10 types:

1. **`minecraft:simple` (`SimpleStateProvider`)**:
   - **Fields**: `state`: `BlockState` (required).
   - **Behavior**: returns `state`. No RNG.

2. **`minecraft:weighted` (`WeightedStateProvider`)**:
   - **Fields**: `entries`: non-empty `WeightedList<BlockState>` (required).
   - **Behavior**: returns `entries.getRandomOrThrow(random)`.

3. **`minecraft:noise` (`NoiseProvider`)**:
   - **Fields**: base noise fields (`seed`, `noise`, `scale`) + `states`: non-empty `List<BlockState>` (required).
   - **Behavior**:
     - `v = getNoiseValue(pos, f64(scale))` (`f32`).
     - `p = Mth.clamp((1.0_f32 + v) / 2.0_f32, 0.0_f32, 0.9999_f32)`.
     - Return `states[trunc_i32(p * f32(states.size()))]`. No RNG.

4. **`minecraft:dual_noise` (`DualNoiseProvider`)**:
   - **Fields**: `variety`: `InclusiveRange<i32>` in `[1, 64]` (JSON `{min_inclusive, max_inclusive}` or `[min, max]`, required), `slow_noise`: inline `NormalNoise` (required), `slow_scale`: positive `f32` (required), plus all `noise` fields (`seed`, `noise`, `scale`, `states`).
   - **Construction**: builds `slowNoise = slowNoiseParameters.create(new WorldgenRandom(new LegacyRandomSource(seed)))` (using the **same** `seed` as the fast noise!).
   - **Behavior**:
     1. `slowVal = f64(slowNoise.get(f64(pos.x) * f64(slowScale), f64(pos.y) * f64(slowScale), f64(pos.z) * f64(slowScale)))`.
     2. `localVariety = trunc_i32(Mth.clampedMap(slowVal, -1.0, 1.0, f64(variety.min), f64(variety.max + 1)))`.
     3. Build `possibleStates` of length `localVariety`: for `i = 0 .. localVariety - 1`, sample `s = slowNoise` at `pos.offset(i * 54545, 0, i * 34234)` with `slowScale`, and append `getRandomState(states, s)` (same `(1 + s)/2` clamp `[0, 0.9999f]` index formula).
     4. Return `getRandomState(possibleStates, pos, f64(scale))` using the fast noise. No RNG.

5. **`minecraft:noise_threshold` (`NoiseThresholdProvider`)**:
   - **Fields**: base noise fields (`seed`, `noise`, `scale`) + `threshold`: `f32` in `[-1.0, 1.0]`, `high_chance`: `f32` in `[0.0, 1.0]`, `default_state`: `BlockState`, `low_states`: non-empty `List<BlockState>`, `high_states`: non-empty `List<BlockState>` (all required).
   - **Behavior**:
     - `v = f64(getNoiseValue(pos, f64(scale)))`.
     - If `v < f64(threshold)`: return `low_states[random.nextInt(low_states.size())]`.
     - Else: if `random.nextFloat() < high_chance`, return `high_states[random.nextInt(high_states.size())]`; else return `default_state`.

6. **`minecraft:rotated` (`RotatedBlockProvider`)**:
   - **Fields**: `state`: `Holder<BlockStateProvider>` (required), `direction`: `Direction` (optional).
   - **Behavior**:
     1. If `direction` is absent, draw `dir = Direction.getRandom(random)` (1 `nextInt(6)` draw) **BEFORE** querying `this.state`!
     2. Query `s = this.state.value().getState(level, random, pos)`.
     3. Apply `trySetValue(AXIS, dir.axis)` and `trySetValue(FACING, dir)`.
     4. If `dir.axis.isHorizontal()`: apply `trySetValue(HORIZONTAL_FACING, dir)`.
     5. Return resulting state.

7. **`minecraft:randomized_int` (`RandomizedIntStateProvider`)**:
   - **Fields**: `source`: `Holder<BlockStateProvider>` (required), `property`: `String` (required), `values`: `IntProvider` (required).
   - **Behavior**:
     1. `s = source.value().getState(level, random, pos)`.
     2. Look up an `IntegerProperty` named `property` on `s` (cached on the provider instance if `s` has it). If none exists, return `s` **without** sampling `values`.
     3. Otherwise return `s.setValue(prop, values.sample(random))`.

8. **`minecraft:rule_based` (`RuleBasedStateProvider`)**:
   - **Fields**:
     - `fallback`: `Holder<BlockStateProvider>` (optional; may be omitted / null).
     - `rules`: `List<{ "if_true": BlockPredicate, "then": Holder<BlockStateProvider> }>` (required).
   - **Behavior**:
     - `getOptionalState(level, random, pos)`: iterate `rules` in order; for the first rule where `if_true.test(level, pos)` is `true` AND `then.value().getOptionalState(level, random, pos)` returns non-null, return that state (if `then` returns null, continue checking later rules!). If no rule produces a state, return `fallback == null ? null : fallback.value().getOptionalState(level, random, pos)`.
     - `getState(level, random, pos)`: call `getOptionalState`; if null, return `level.getBlockState(pos)`.

9. **`minecraft:copy_properties` (`CopyPropertiesProvider`)** — *new in 26.3*:
   - **Fields**: `source`: `Holder<BlockStateProvider>` (required).
   - **Behavior**: return `source.value().getState(level, random, pos).withPropertiesOf(level.getBlockState(pos))` (copies every property shared between the generated block and the existing block at `pos`).

10. **`minecraft:random_block` (`RandomBlockProvider`)** — *new in 26.3*:
    - **Fields**: `blocks`: `HolderSet<Block>` (required).
    - **Behavior**:
      - `getOptionalState`: if `blocks` is empty, return `null` (no RNG); else pick `blocks[random.nextInt(blocks.size())].defaultBlockState()`.
      - `getState`: call `getOptionalState`; if null, return `level.getBlockState(pos)`.

#### 3.3 `IntProvider`, `FloatProvider`, and `WeightedList`

**`WeightedList<E>`** (JSON array of `{ "data": <E>, "weight": <i32 >= 0> }`; `nonEmptyCodec` requires total weight `> 0`):
- `totalWeight = sum(entry.weight)`. If `totalWeight == 0`, `isEmpty()` is `true` (`getRandom` returns `Optional.empty()` without drawing RNG; `getRandomOrThrow` throws).
- Otherwise draws `selection = random.nextInt(totalWeight)` and returns the unique entry whose cumulative weight interval `[prevSum, prevSum + weight)` contains `selection` (both `Flat` for `< 64` and `Compact` for `>= 64` implement this identical prefix-sum selection).

**All 8 `IntProvider` types** (`minecraft:int_provider_type`; bare JSON `i32` decodes as `minecraft:constant`):

| Type ID | JSON fields & validation | `minInclusive` / `maxInclusive` | Exact `sample(random)` |
|---|---|---|---|
| `minecraft:constant` | `value`: `i32` | `value` / `value` | `value` (no RNG) |
| `minecraft:uniform` | `min_inclusive`, `max_inclusive`: `i32` (`max >= min`) | `min` / `max` | `Mth.randomBetweenInclusive(random, min, max)` (1 draw even if `min == max`) |
| `minecraft:biased_to_bottom` | `min_inclusive`, `max_inclusive`: `i32` (`max >= min`) | `min` / `max` | `min + random.nextInt(random.nextInt(max - min + 1) + 1)` (2 draws, inner first) |
| `minecraft:very_biased_to_bottom` | `min_inclusive`, `max_inclusive`: `i32` (`max >= min`) | `min` / `max` | `min + random.nextInt(random.nextInt(random.nextInt(max - min + 1) + 1) + 1)` (3 draws, innermost first) |
| `minecraft:clamped` | `source`: `IntProvider`, `min_inclusive`, `max_inclusive`: `i32` (`max >= min`) | `max(min, source.min)` / `min(max, source.max)` | `Mth.clamp(source.sample(random), min, max)` |
| `minecraft:clamped_normal` | `mean`, `deviation`: `f32`; `min_inclusive`, `max_inclusive`: `i32` (`max >= min`) | `min` / `max` | `trunc_i32(Mth.clamp(Mth.normal(random, mean, deviation), f32(min), f32(max)))` |
| `minecraft:weighted_list` | `distribution`: non-empty `WeightedList<IntProvider>` | min of mins / max of maxes | `distribution.getRandomOrThrow(random).sample(random)` |
| `minecraft:trapezoid` | `min`, `max`, `plateau`: `i32` (`max >= min`, `plateau <= max - min`) | `min` / `max` | If `plateau == 0 && max == -min`: `random.nextInt(max + 1) - random.nextInt(max + 1)`. Else if `plateau == max - min`: `Mth.randomBetweenInclusive(random, min, max)` (1 draw). Else: `r = max - min`, `pStart = (r - plateau) / 2`, `pEnd = r - pStart`, return `min + Mth.randomBetweenInclusive(random, 0, pEnd) + Mth.randomBetweenInclusive(random, 0, pStart)`. |

**All 4 `FloatProvider` types** (`minecraft:float_provider_type`; bare JSON `f32` decodes as `minecraft:constant`):

| Type ID | JSON fields & validation | Exact `sample(random)` |
|---|---|---|
| `minecraft:constant` | `value`: `f32` | `value` (no RNG) |
| `minecraft:uniform` | `min_inclusive`, `max_exclusive`: `f32` (`max > min`) | `Mth.randomBetween(random, min, max)` = `random.nextFloat() * (max - min) + min` |
| `minecraft:clamped_normal` | `mean`, `deviation`, `min`, `max`: `f32` (`max >= min`) | `Mth.clamp(Mth.normal(random, mean, deviation), min, max)` |
| `minecraft:trapezoid` | `min`, `max`, `plateau`: `f32` (`max >= min`, `plateau <= max - min`) | `r = max - min`; `pStart = (r - plateau) / 2.0_f32`; `pEnd = r - pStart`; return `min + random.nextFloat() * pEnd + random.nextFloat() * pStart` (2 draws, left-to-right `f32`) |

#### 3.4 All 10 `RuleTest` types (`minecraft:rule_test`) and `BlockReplacement`
- `RuleTest.CODEC` dispatches on key **`"predicate_type"`** (not `"type"`!).
- `BlockReplacement`: `{ "target": RuleTest, "state": BlockState }`.

| `predicate_type` | JSON fields | `test(state, pos, random) -> bool` |
|---|---|---|
| `minecraft:always_true` | *(none)* | Always `true`. |
| `minecraft:all_of` | `rules`: `List<RuleTest>` | Short-circuiting AND in list order. |
| `minecraft:any_of` | `rules`: `List<RuleTest>` | Short-circuiting OR in list order. |
| `minecraft:not` | `rule`: `RuleTest` | `!rule.test(state, pos, random)`. |
| `minecraft:block_match` | `block`: block ID | `state.is(block)`. |
| `minecraft:blockstate_match` | `block_state`: `BlockState` | Exact state equality (`state == block_state`). |
| `minecraft:tag_match` | `tag`: bare `TagKey<Block>` (no `#`) | `state.is(tag)`. |
| `minecraft:height_match` | `min_inclusive`, `max_inclusive`: `i32` | `min_inclusive <= pos.y && pos.y <= max_inclusive`. |
| `minecraft:random_block_match` | `block`: block ID, `probability`: `f32` | `state.is(block) && random.nextFloat() < probability` (short-circuits RNG if block doesn't match). |
| `minecraft:random_blockstate_match` | `block_state`: `BlockState`, `probability`: `f32` | `state == block_state && random.nextFloat() < probability` (short-circuits RNG). |

#### 3.5 Vertical column scanner (`Column.scan`)
Used by `large_dripstone`, `speleothem_cluster`, and `underwater_magma`.
Given `(level, pos, searchRange, insideColumn, validEdge)`:
1. If `!insideColumn(level.getBlockState(pos))`: return `Optional.empty()`.
2. Scan upward (`scanDirection(..., Direction.UP)`): start `m = pos`, loop `i = 1 .. searchRange - 1` while `insideColumn(state(m))` moving `m` up 1 each step. After loop, if `validEdge(state(m))` record `ceiling = Some(m.y)` else `None`.
   *(Note: because the loop starts at `i = 1` and checks `i < searchRange`, it moves at most `searchRange - 1` blocks!)*
3. Scan downward (`scanDirection(..., Direction.DOWN)`): identical downward from `pos`, recording `floor = Some(m.y)` or `None`.
4. Return `Column`:
   - Both `floor` and `ceiling`: `Column.Range(floor, ceiling)` with `height = ceiling - floor - 1`.
   - Only `floor`: `Column.Ray(floor, pointingUp = true)` (no ceiling, no height).
   - Only `ceiling`: `Column.Ray(ceiling, pointingUp = false)` (no floor, no height).
   - Neither: `Column.Line` (no floor, no ceiling, no height).

---

### 4. All 58 feature types (`minecraft:worldgen/feature_type`)

Every entry in `minecraft:worldgen/feature` decodes directly via `"type": "<id>"` and the type's flat fields.

#### Alphabetical registry index (all 58 registered types in 26.3)

| # | Type ID | Section | # | Type ID | Section |
|---|---|---|---|---|---|
| 1 | `minecraft:bamboo` | §4.22 | 30 | `minecraft:netherrack_replace_blobs` | §4.11 |
| 2 | `minecraft:block_blob` | §4.14 | 31 | `minecraft:no_op` | §4.1 |
| 3 | `minecraft:block_column` | §4.15 | 32 | `minecraft:ore` | §4.8 |
| 4 | `minecraft:block_pile` | §4.16 | 33 | `minecraft:overlay` | §4.6 |
| 5 | `minecraft:blue_ice` | §4.27 | 34 | `minecraft:projected_random_patchy_square` | §4.18 |
| 6 | `minecraft:bonus_chest` | §4.44 | 35 | `minecraft:random_boolean_selector` | §4.2 |
| 7 | `minecraft:chorus_plant` | §4.34 | 36 | `minecraft:random_neighbor_spread` | §4.19 |
| 8 | `minecraft:coral_claw` | §4.36 | 37 | `minecraft:random_selector` | §4.3 |
| 9 | `minecraft:coral_tree` | §4.37 | 38 | `minecraft:replace_single_block` | §4.10 |
| 10 | `minecraft:delta_feature` | §4.39 | 39 | `minecraft:root_system` | §4.31 |
| 11 | `minecraft:disk` | §4.12 | 40 | `minecraft:scattered_ore` | §4.9 |
| 12 | `minecraft:end_gateway` | §4.46 | 41 | `minecraft:sculk_patch` | §4.41 |
| 13 | `minecraft:end_island` | §4.47 | 42 | `minecraft:sequence` | §4.7 |
| 14 | `minecraft:end_platform` | §4.48 | 43 | `minecraft:simple_block` | §4.13 |
| 15 | `minecraft:end_podium` | §4.49 | 44 | `minecraft:simple_random_selector` | §4.4 |
| 16 | `minecraft:end_spike` | §4.50 | 45 | `minecraft:single_block_pillar` | §4.20 |
| 17 | `minecraft:fallen_tree` | §4.24 | 46 | `minecraft:speleothem` | §4.32 |
| 18 | `minecraft:fill_layer` | §4.17 | 47 | `minecraft:speleothem_cluster` | §4.33 |
| 19 | `minecraft:fossil` | §4.42 | 48 | `minecraft:spike` | §4.28 |
| 20 | `minecraft:freeze_top_layer` | §4.26 | 49 | `minecraft:spring_feature` | §4.21 |
| 21 | `minecraft:geode` | §4.29 | 50 | `minecraft:stepped_column_cluster` | §4.38 |
| 22 | `minecraft:huge_brown_mushroom` | §4.52 | 51 | `minecraft:template` | §4.43 |
| 23 | `minecraft:huge_fungus` | §4.54 | 52 | `minecraft:tree` | §4.23 *(see `features-trees.md`)* |
| 24 | `minecraft:huge_red_mushroom` | §4.53 | 53 | `minecraft:underwater_magma` | §4.40 |
| 25 | `minecraft:iceberg` | §4.25 | 54 | `minecraft:vegetation_patch` | §4.55 |
| 26 | `minecraft:lake` | §4.51 | 55 | `minecraft:vines` | §4.35 |
| 27 | `minecraft:large_dripstone` | §4.30 | 56 | `minecraft:void_start_platform` | §4.57 |
| 28 | `minecraft:monster_room` | §4.45 | 57 | `minecraft:waterlogged_vegetation_patch` | §4.56 |
| 29 | `minecraft:multiface_growth` | §4.58 | 58 | `minecraft:weighted_random_selector` | §4.5 |

---

#### Group A: Meta, selector, and combinator features (§4.1–§4.7)

All selector/combinator features invoke child `PlacedFeature`s via `PlacedFeature.place(...)` (or a shared `FeaturePlacer.place(...)`, which sets `topFeature = Optional.empty()`, so `minecraft:biome` cannot appear on sub-features) and expose their children via `getSubFeatures()` (used by `BiomeGenerationSettings.getBoneMealFeatures()`).

##### 4.1 `minecraft:no_op` (`NoOpFeature`)
- **Fields**: none.
- **Behavior**: does nothing, consumes no RNG, returns **`true`**.

##### 4.2 `minecraft:random_boolean_selector` (`RandomBooleanSelectorFeature`)
- **Fields**: `feature_true`: `Holder<PlacedFeature>`, `feature_false`: `Holder<PlacedFeature>` (both required).
- **Behavior**: draws `b = random.nextBoolean()`; places `(b ? feature_true : feature_false)` at `origin` and returns its result.

##### 4.3 `minecraft:random_selector` (`RandomSelectorFeature`, deprecated)
- **Fields**:
  - `features`: `List<{ "feature": Holder<PlacedFeature>, "chance": f32 in [0.0, 1.0] }>` (required).
  - `default`: `Holder<PlacedFeature>` (required).
- **Behavior**: iterates `features` in order; for each entry draws `random.nextFloat() < entry.chance`; on the first match places `entry.feature` at `origin` and returns its result. If none match, places `default` at `origin` and returns its result.

##### 4.4 `minecraft:simple_random_selector` (`SimpleRandomSelectorFeature`)
- **Fields**: `features`: non-empty `HolderSet<PlacedFeature>` (required).
- **Behavior**: draws `idx = random.nextInt(features.size())`; places `features[idx]` at `origin` and returns its result.

##### 4.5 `minecraft:weighted_random_selector` (`WeightedRandomSelectorFeature`) — *new in 26.3*
- **Fields**: `features`: `WeightedList<Holder<PlacedFeature>>` (required; may be empty).
- **Behavior**: calls `features.getRandom(random)`; if present, places that `PlacedFeature` at `origin` and returns its result; if empty, consumes no RNG and returns `false`.

##### 4.6 `minecraft:overlay` (`OverlayFeature`) — *new in 26.3*
- **Fields**: `features`: non-empty `HolderSet<PlacedFeature>` (required).
- **Behavior**: instantiates one `FeaturePlacer`, sets `placedAny = false`, and places **every** feature in `features` in order at `origin` (no short-circuiting), accumulating `placedAny |= placer.place(f, random, origin)`. Returns `placedAny`.

##### 4.7 `minecraft:sequence` (`SequenceFeature`) — *new in 26.3*
- **Fields**: `features`: non-empty `HolderSet<PlacedFeature>` (required).
- **Behavior**: instantiates one `FeaturePlacer` and iterates `features` in order, calling `placer.place(f, random, origin)`. Stops immediately and returns **`false`** on the first child returning `false`; returns **`true`** if all succeed.

---

#### Group B: Ores, replacements, blobs, disks, columns, and simple blocks (§4.8–§4.21)

##### Shared ore helper (`AbstractOreFeature`)
- **Shared fields**:
  - `targets`: `List<BlockReplacement>` (required; each `{ "target": RuleTest, "state": BlockState }`).
  - `size`: `i32` in `[0, 64]` (required).
  - `discard_chance_on_air_exposure`: `f32` in `[0.0, 1.0]` (required).
- **`canPlaceOre(state, blockGetter, random, target, pos) -> bool`**:
  1. If `!target.target.test(state, pos, random)`: return `false`.
  2. Evaluate `skipAirCheck`:
     - If `discard_chance_on_air_exposure <= 0.0_f32`: `true` (no RNG!).
     - Else if `discard_chance_on_air_exposure >= 1.0_f32`: `false` (no RNG!).
     - Else: `random.nextFloat() >= discard_chance_on_air_exposure`.
  3. If `skipAirCheck`: return `true`.
  4. Otherwise check all 6 neighbors in `Direction.values()` order (`DOWN, UP, NORTH, SOUTH, WEST, EAST`): return `false` if any neighbor satisfies `blockGetter(pos.relative(dir)).isAir()`, else `true`.

##### 4.8 `minecraft:ore` (`OreFeature`)
- **Fields**: shared ore fields (`targets`, `size`, `discard_chance_on_air_exposure`).
- **Behavior**:
  1. Draw in exact order:
     - `dir = random.nextFloat() * f32(PI)` (`3.1415927_f32`)
     - `spreadXY = f32(size) / 8.0_f32`
     - `maxRadius = Mth.ceil((f32(size) / 16.0_f32 * 2.0_f32 + 1.0_f32) / 2.0_f32)`
     - `x0 = f64(origin.x) + Math.sin(f64(dir)) * f64(spreadXY)` *(uses `Math.sin`/`Math.cos`, NOT `Mth` LUT!)*
     - `x1 = f64(origin.x) - Math.sin(f64(dir)) * f64(spreadXY)`
     - `z0 = f64(origin.z) + Math.cos(f64(dir)) * f64(spreadXY)`
     - `z1 = f64(origin.z) - Math.cos(f64(dir)) * f64(spreadXY)`
     - `y0 = f64(origin.y + random.nextInt(3) - 2)`
     - `y1 = f64(origin.y + random.nextInt(3) - 2)`
  2. Compute bounding box:
     - `xStart = origin.x - Mth.ceil(spreadXY) - maxRadius`
     - `yStart = origin.y - 2 - maxRadius`
     - `zStart = origin.z - Mth.ceil(spreadXY) - maxRadius`
     - `sizeXZ = 2 * (Mth.ceil(spreadXY) + maxRadius)`
     - `sizeY = 2 * (2 + maxRadius)`
  3. **Surface pre-check**: loop `xp = xStart ..= xStart + sizeXZ`, `zp = zStart ..= zStart + sizeXZ`; as soon as `yStart <= level.getHeight(OCEAN_FLOOR_WG, xp, zp)`, proceed to `doPlace`. If no column passes, return `false` immediately.
  4. **`doPlace`**:
     1. Allocate `BitSet tested(sizeXZ * sizeY * sizeXZ)` and `data[size * 4]` (`f64`).
     2. For `i = 0 .. size - 1`:
        - `step = f32(i) / f32(size)`
        - `xx = Mth.lerp(f64(step), x0, x1)`, `yy = Mth.lerp(f64(step), y0, y1)`, `zz = Mth.lerp(f64(step), z0, z1)`
        - `ss = random.nextDouble() * f64(size) / 16.0`
        - `r = (f64(Mth.sin(f64(3.1415927_f32 * step)) + 1.0_f32) * ss + 1.0) / 2.0` *(uses **`Mth.sin`** LUT!)*
        - Store `(xx, yy, zz, r)` into `data[i*4 .. i*4+3]`.
     3. **Contained-sphere culling**: for `i1 = 0 .. size - 2`, if `data[i1*4+3] > 0.0`, for `i2 = i1 + 1 .. size - 1`, if `data[i2*4+3] > 0.0`:
        - `dx = x1 - x2`, `dy = y1 - y2`, `dz = z1 - z2`, `dr = r1 - r2`.
        - If `dr * dr > dx * dx + dy * dy + dz * dz`: set `data[i2*4+3] = -1.0` if `dr > 0.0`, else `data[i1*4+3] = -1.0`.
     4. Open `BulkSectionAccess(level)` (note: `BulkSectionAccess.getBlockState` returns `minecraft:air` if a position is outside build height, and **`section.setBlockState(..., false)` bypasses heightmap updates and post-processing!**).
     5. For `i = 0 .. size - 1`:
        - If `r < 0.0` skip.
        - `xMin = max(Mth.floor(xx - r), xStart)`, `yMin = max(Mth.floor(yy - r), yStart)`, `zMin = max(Mth.floor(zz - r), zStart)`.
        - `xMax = max(Mth.floor(xx + r), xMin)`, `yMax = max(Mth.floor(yy + r), yMin)`, `zMax = max(Mth.floor(zz + r), zMin)`.
        - Loop `x = xMin..=xMax`, `y = yMin..=yMax`, `z = zMin..=zMax` (with early-out `xd*xd < 1.0` and `xd*xd + yd*yd < 1.0`, where `xd = (x + 0.5 - xx) / r`, etc.):
          - If `xd*xd + yd*yd + zd*zd < 1.0 && !level.isOutsideBuildHeight(y)`:
            - `bitIdx = (x - xStart) + (y - yStart) * sizeXZ + (z - zStart) * sizeXZ * sizeY`.
            - If `!tested[bitIdx]`:
              - Set `tested[bitIdx] = true`.
              - If `level.ensureCanWrite(orePos)` and `section != null`:
                - Read `blockState = section.getBlockState(x & 15, y & 15, z & 15)`.
                - Iterate `targets` in order: on the first `target` where `canPlaceOre(blockState, sectionGetter::getBlockState, random, target, orePos)` is `true`: write `section.setBlockState(x & 15, y & 15, z & 15, target.state, false)`, increment `placed`, and break.
     6. Return `placed > 0`.

##### 4.9 `minecraft:scattered_ore` (`ScatteredOreFeature`)
- **Fields**: shared ore fields (`targets`, `size`, `discard_chance_on_air_exposure`).
- **Behavior**:
  1. `numberOfTries = random.nextInt(size + 1)`.
  2. For `i = 0 .. numberOfTries - 1`:
     - `maxDist = min(i, 7)`.
     - Draw 3 axis offsets in exact `X, Y, Z` order, each `round_i32((random.nextFloat() - random.nextFloat()) * f32(maxDist))` *(note: 6 `nextFloat()` draws per try, even when `maxDist == 0` at `i == 0`!)*.
     - `targetPos = origin.offset(xd, yd, zd)`.
     - Read `blockState = level.getBlockState(targetPos)`.
     - Iterate `targets` in order: on the first `target` passing `canPlaceOre(blockState, level::getBlockState, random, target, targetPos)`: `level.setBlock(targetPos, target.state, 2)` and break.
  3. Always returns **`true`**.

##### 4.10 `minecraft:replace_single_block` (`ReplaceBlockFeature`)
- **Fields**: `targets`: `List<BlockReplacement>` (required).
- **Behavior**: iterates `targets` in order; on the first entry where `target.test(level.getBlockState(origin), origin, random)` is `true`, calls `level.setBlock(origin, replacement.state, 2)` and breaks. Always returns **`true`**.

##### 4.11 `minecraft:netherrack_replace_blobs` (`ReplaceBlobsFeature`)
- **Fields**: `target`: `BlockState` (only its `Block` is compared), `state`: `BlockState`, `radius`: `IntProvider` in `[0, 12]` (all required).
- **Behavior**:
  1. Clamp `origin.y` to `[level.minY + 1, level.maxY]`.
  2. Scan downward while `cursor.y > level.minY + 1`: if `level.getBlockState(cursor).is(target.block)`, stop with `centerPos = cursor`; else move down 1. If none found, return `false` (no RNG).
  3. Sample `radiusX = radius.sample(random)`, `radiusY = radius.sample(random)`, `radiusZ = radius.sample(random)` in exact `X, Y, Z` order. Let `maxR = max(radiusX, max(radiusY, radiusZ))`.
  4. Iterate `BlockPos.withinBoxByManhattanDistance(centerPos, radiusX, radiusY, radiusZ)`:
     - If `pos.distManhattan(centerPos) > maxR`: break.
     - If `level.getBlockState(pos).is(target.block)`: `setBlock(level, pos, state)` (`flags = 3`), `replacedAny = true`.
  5. Return `replacedAny`.

##### 4.12 `minecraft:disk` (`DiskFeature`)
- **Fields**:
  - `state_provider`: `Holder<BlockStateProvider>` (required; uses `getOptionalState`!).
  - `target`: `BlockPredicate` (required).
  - `radius`: `IntProvider` in `[0, 8]` (required).
  - `half_height`: `i32` in `[0, 4]` (required).
- **Behavior**:
  1. `top = origin.y + half_height`, `bottom = origin.y - half_height - 1` (so columns cover `y = top` down to `origin.y - half_height` inclusive!).
  2. `r = radius.sample(random)`.
  3. Iterate `columnPos` in `BlockPos.betweenClosed(origin.offset(-r, 0, -r), origin.offset(r, 0, r))` (`z` outer, `x` inner):
     - If `(columnPos.x - origin.x)^2 + (columnPos.z - origin.z)^2 <= r * r`:
       - Set `placedAbove = false`.
       - For `y = top` down to `bottom + 1` inclusive:
         - If `target.test(level, (columnPos.x, y, columnPos.z))`:
           - `state = state_provider.getOptionalState(level, random, pos)`.
           - If `state != null`:
             - `level.setBlock(pos, state, 2)`.
             - If `!placedAbove`: `markAboveForPostProcessing(level, pos)`.
             - `placedAny = true`, `placedAbove = true`.
         - Else: `placedAbove = false`.
  4. Return `placedAny`.

##### 4.13 `minecraft:simple_block` (`SimpleBlockFeature`)
- **Fields**:
  - `to_place`: `Holder<BlockStateProvider>` (required; uses `getOptionalState`).
  - `schedule_tick`: `bool` (optional, default `false`).
- **Behavior**:
  1. `state = to_place.getOptionalState(level, random, origin)`. If `state == null`, return `false`.
  2. If `!state.canSurvive(level, origin)`, return `false`.
  3. Dispatch on `state.getBlock()`:
     - **`DoublePlantBlock`**: check `above = level.getBlockState(origin.above())`. If `!above.isAir() && (state.getFluidState() != above.getFluidState() || !above.canBeReplaced())`, return `false`. Otherwise call `DoublePlantBlock.placeAt(level, state, origin, 2)`:
       - Places `copyWaterloggedFrom(level, origin, state.setValue(HALF, LOWER))` at `origin` (`flags = 2`), then `copyWaterloggedFrom(level, origin.above(), state.setValue(HALF, UPPER))` at `origin.above()` (`flags = 2`), where `copyWaterloggedFrom` sets `WATERLOGGED = level.isWaterAt(p)` if the state has `WATERLOGGED`.
     - **`MossyCarpetBlock`**: calls `MossyCarpetBlock.placeAt(level, origin, level.getRandom(), 2)` (**NOTE**: uses **`level.getRandom()`** — `WorldGenRegion`'s random — NOT the feature's `random`!):
       1. `adjusted = getUpdatedState(PALE_MOSS_CARPET.defaultState, level, origin, createSides = true)` (for each horizontal direction `NORTH, EAST, SOUTH, WEST`: if `MultifaceBlock.canAttachTo(level, origin, dir)` set `LOW` — upgraded to `TALL` if `origin.above()` is already `PALE_MOSS_CARPET` with `BASE=false` and that side `!= NONE` — else `NONE`).
       2. `level.setBlock(origin, adjusted, 2)`.
       3. Compute topper at `above = origin.above()`: if `abovePrev` is `PALE_MOSS_CARPET` with `BASE=false` OR (`!isMossyCarpetAbove && abovePrev.canBeReplaced()`):
          - Compute `aboveState = getUpdatedState(PALE_MOSS_CARPET(BASE=false), level, above, true)` (for each horizontal side: `LOW` only if `canAttachTo` at `above` AND below's `adjusted` side `!= NONE`, else `NONE`).
          - For each `dir` in `[NORTH, EAST, SOUTH, WEST]`: if `aboveState[dir] != NONE`, draw `level.getRandom().nextBoolean()`; if `false`, set that side to `NONE`.
          - If `aboveState` has at least one non-`NONE` side and `aboveState != abovePrev`: place `aboveState` at `above` (`flags = 2`), then recompute `getUpdatedState(adjusted, level, origin, true)` (promoting sides beneath the topper to `TALL`) and place it at `origin` (`flags = 2`).
     - **Any other block**: `level.setBlock(origin, state, 2)`.
  4. If `schedule_tick`: `level.scheduleTick(origin, level.getBlockState(origin).getBlock(), 1)`.
  5. Return **`true`**.

##### 4.14 `minecraft:block_blob` (`BlockBlobFeature`) — *generalizes pre-26.3 `forest_rock`*
- **Fields**: `state`: `BlockState` (required), `can_place_on`: `BlockPredicate` (required).
- **Behavior**:
  1. Scan downward: while `origin.y > level.minY + 3 && !can_place_on.test(level, origin.below())`, set `origin = origin.below()`.
  2. If `origin.y <= level.minY + 3`: return `false`.
  3. Loop `c = 0, 1, 2` (3 overlapping ellipsoids):
     - Draw `xr = random.nextInt(2)`, `yr = random.nextInt(2)`, `zr = random.nextInt(2)`.
     - `tr = f32(xr + yr + zr) * 0.333_f32 + 0.5_f32`.
     - For `pos` in `BlockPos.betweenClosed(origin.offset(-xr, -yr, -zr), origin.offset(xr, yr, zr))`:
       - If `pos.distSqr(origin) <= f64(tr * tr)` (`tr * tr` in `f32`, widened to `f64`): `level.setBlockAndUpdate(pos, state)`.
     - Move `origin = origin.offset(-1 + random.nextInt(2), -random.nextInt(2), -1 + random.nextInt(2))` (note: drawn even on final iteration `c = 2`).
  4. Return **`true`**.

##### 4.15 `minecraft:block_column` (`BlockColumnFeature`)
- **Fields**:
  - `layers`: `List<{ "height": IntProvider (>= 0), "provider": Holder<BlockStateProvider> }>` (required).
  - `direction`: `Direction` (required).
  - `allowed_placement`: `BlockPredicate` (required).
  - `prioritize_tip`: `bool` (required).
- **Behavior**:
  1. Sample `layerHeights[i] = layers[i].height.sample(random)` for `i = 0 .. layerCount - 1` in order; sum into `totalHeight`.
  2. If `totalHeight == 0`, return `false`.
  3. **Truncation scan**: start `nextPos = origin.relative(direction)` (**NOTE**: checks starting 1 step in `direction`, NOT `origin` itself!). For `y = 0 .. totalHeight - 1`:
     - If `!allowed_placement.test(level, nextPos)`: truncate `layerHeights` from `totalHeight` down to `y`:
       - `amountToRemove = totalHeight - y`.
       - If `prioritize_tip == true`: trim from `i = 0` upward; if `false`: trim from `i = layerCount - 1` downward, subtracting `min(layerHeights[i], amountToRemove)` at each layer until `amountToRemove == 0`.
       - Break.
     - `nextPos.move(direction)`.
  4. **Placement**: start `placePos = origin`. For `i = 0 .. layerCount - 1`, repeat `layerHeights[i]` times:
     - `level.setBlock(placePos, layers[i].provider.getState(level, random, placePos), 2)`.
     - `placePos.move(direction)`.
  5. Return **`true`**.

##### 4.16 `minecraft:block_pile` (`BlockPileFeature`)
- **Fields**: `state_provider`: `Holder<BlockStateProvider>` (required).
- **Behavior**:
  1. If `origin.y < level.minY + 5`: return `false`.
  2. Draw `xr = 2 + random.nextInt(2)`, `zr = 2 + random.nextInt(2)`.
  3. For `pos` in `BlockPos.betweenClosed(origin.offset(-xr, 0, -zr), origin.offset(xr, 1, zr))` (`z` outer, `y` middle `0..1`, `x` inner):
     - `xd = origin.x - pos.x`, `zd = origin.z - pos.z`.
     - If `f32(xd * xd + zd * zd) <= random.nextFloat() * 10.0_f32 - random.nextFloat() * 6.0_f32`: `tryPlaceBlock(pos)`.
     - Else if `f64(random.nextFloat()) < 0.031`: `tryPlaceBlock(pos)`.
     - **`tryPlaceBlock(pos)`**: if `level.isEmptyBlock(pos)` AND `mayPlaceOn(pos)` (where `below = level.getBlockState(pos.below())`: if `below.is(minecraft:dirt_path)` draw `random.nextBoolean()`, else `below.isFaceSturdy(level, pos.below(), UP)`):
       - `level.setBlock(pos, state_provider.getState(level, random, pos), 260)`.
  4. Return **`true`**.

##### 4.17 `minecraft:fill_layer` (`FillLayerFeature`)
- **Fields**: `height`: `i32` in `[0, 4064]` (`DimensionType.Y_SIZE`), `state`: `BlockState` (both required).
- **Behavior**: loop `dx = 0..15`, `dz = 0..15`: at `(origin.x + dx, level.minY + height, origin.z + dz)`, if `isAir()`, `level.setBlock(pos, state, 2)`. No RNG. Returns **`true`**.

##### 4.18 `minecraft:projected_random_patchy_square` (`ProjectedRandomPatchySquare`) — *new in 26.3*
- **Fields**:
  - `block`: `Holder<BlockStateProvider>` (required; uses `getOptionalState`).
  - `project_through`: `BlockPredicate` (required).
  - `size`: `IntProvider` in `[1, 16]` (required).
  - `max_projection_height`: non-negative `i32` (`>= 0`, required).
- **Behavior**:
  1. `sz = size.sample(random)`; `bound = sz * sz + 1`.
  2. Loop `dx = -sz ..= sz` (outer), `dz = -sz ..= sz` (inner):
     - `prob = |dx| * |dz|`.
     - If `random.nextInt(bound) < bound - prob`:
       - `basePos = origin.offset(dx, 0, dz)`.
       - `drop = max_projection_height`.
       - While `project_through.test(level, basePos.below())`:
         - Move `basePos` down 1.
         - If `--drop <= 0`: break.
         *(Note: when `max_projection_height == 0`, if `project_through` passes at `basePos.below()`, it STILL moves down 1 before `--drop` becomes `-1 <= 0` and breaks!)*
       - `state = block.getOptionalState(level, random, basePos)`.
       - If `state != null`: `level.setBlock(basePos, state, 2)`.
  3. Always returns **`true`**.

##### 4.19 `minecraft:random_neighbor_spread` (`RandomNeighborSpreadFeature`) — *new in 26.3, generalizes glowstone blob*
- **Fields**:
  - `block`: `Holder<BlockStateProvider>` (required).
  - `accepted_neighbors`: `HolderSet<Block>` (required).
  - `can_replace`: `BlockPredicate` (required).
  - `attempts`: `IntProvider` in `[1, 3000]` (required).
  - `xz_offset`: `IntProvider` in `[-16, 16]` (required).
  - `y_offset`: `IntProvider` in `[-16, 16]` (required).
- **Behavior**:
  1. Unconditionally place `level.setBlock(origin, block.getState(level, random, origin), 2)`.
  2. `n = attempts.sample(random)`.
  3. Loop `i = 0 .. n - 1`:
     - Draw offsets in exact `X, Y, Z` order: `dx = xz_offset.sample(random)`, `dy = y_offset.sample(random)`, `dz = xz_offset.sample(random)`.
     - `placePos = origin.offset(dx, dy, dz)`.
     - If `can_replace.test(level, placePos)`:
       - Count 6-connected neighbors (in `Direction.values()` order) matching `accepted_neighbors`, breaking early if `neighbours > 1`.
       - If `neighbours == 1`: `level.setBlock(placePos, block.getState(level, random, placePos), 2)`.
  4. Always returns **`true`**.

##### 4.20 `minecraft:single_block_pillar` (`SingleBlockPillarFeature`) — *new in 26.3, generalizes kelp / basalt pillar / nether vines*
- **Fields**:
  - `block`: `Holder<BlockStateProvider>` (required).
  - `can_replace`: `BlockPredicate` (optional, default `true`).
  - `direction`: vertical `Direction` (`"up"` or `"down"`, required).
  - `chance_to_continue`: `f32` in `[0.0, 1.0]` (optional, default `1.0`).
  - `cap_feature`: `Optional<Holder<PlacedFeature>>` (optional).
- **Behavior**:
  1. Set `pos = origin`.
  2. While `can_replace.test(level, pos) && random.nextFloat() < chance_to_continue && !level.isOutsideBuildHeight(pos)` *(short-circuiting left-to-right!)*:
     - `level.setBlock(pos, block.getState(level, random, pos), 2)`.
     - `pos.move(direction)`.
  3. Move `pos` back 1 step: `pos.move(direction.getOpposite())`.
  4. If `cap_feature` is present: call `cap_feature.value().place(level, chunkGenerator, random, pos)` (note: even if 0 pillar blocks were placed, `cap_feature` still runs at `origin.relative(direction.getOpposite())`!).
  5. Always returns **`true`**.

##### 4.21 `minecraft:spring_feature` (`SpringFeature`)
- **Fields**:
  - `state`: `FluidState` (JSON `{ "id": "<fluid_id>", "properties": {...} }`, required).
  - `requires_block_below`: `bool` (optional, default `true`).
  - `rock_count`: `i32` (optional, default `4`).
  - `hole_count`: `i32` (optional, default `1`).
  - `valid_blocks`: `HolderSet<Block>` (required).
- **Behavior** (no RNG):
  1. If `!getBlockState(origin.above()).is(valid_blocks)`: return `false`.
  2. If `requires_block_below && !getBlockState(origin.below()).is(valid_blocks)`: return `false`.
  3. Let `cur = getBlockState(origin)`. If `!cur.isAir() && !cur.is(valid_blocks)`: return `false`.
  4. Over the 5 neighbors `(west, east, north, south, below)`:
     - `rocks` = count whose block matches `valid_blocks`.
     - `holes` = count where `level.isEmptyBlock(p)`.
  5. If `rocks == rock_count && holes == hole_count`:
     - `level.setBlock(origin, state.createLegacyBlock(), 2)`.
     - `level.scheduleTick(origin, state.getType(), 0)`.
     - Return `true`.
  6. Return `false`.


---

#### Group C: Bamboo, trees, and fallen trees (§4.22–§4.24)

##### 4.22 `minecraft:bamboo` (`BambooFeature`)
- **Fields**: `probability`: `f32` in `[0.0, 1.0]` (required).
- **Pre-defined block states**:
  - `BAMBOO_TRUNK`: `minecraft:bamboo[age=1,leaves=none,stage=0]`
  - `BAMBOO_TOP_SMALL`: `minecraft:bamboo[age=1,leaves=small,stage=0]`
  - `BAMBOO_TOP_LARGE`: `minecraft:bamboo[age=1,leaves=large,stage=0]`
  - `BAMBOO_FINAL_LARGE`: `minecraft:bamboo[age=1,leaves=large,stage=1]`
- **Behavior**:
  1. Check `level.isEmptyBlock(origin)`. If `false`, consume no RNG and return **`false`**.
  2. If `minecraft:bamboo` `canSurvive` at `origin` (i.e. `level.getBlockState(origin.below()).is(#minecraft:supports_bamboo)`):
     1. `height = random.nextInt(12) + 5` (in `[5, 16]`).
     2. **Podzol patch**: if `random.nextFloat() < probability`:
        - `r = random.nextInt(4) + 1` (in `[1, 4]`).
        - Loop `xx = origin.x - r ..= origin.x + r` (outer), `zz = origin.z - r ..= origin.z + r` (inner):
          - If `(xx - origin.x)^2 + (zz - origin.z)^2 <= r * r`:
            - `y = level.getHeight(WORLD_SURFACE, xx, zz) - 1`.
            - If `level.getBlockState((xx, y, zz)).is(#minecraft:beneath_bamboo_podzol_replaceable)`: `level.setBlock((xx, y, zz), minecraft:podzol, 2)`.
     3. **Stalk**: set cursor `bambooPos = origin`. Loop `i = 0 .. height - 1` while `level.isEmptyBlock(bambooPos)`:
        - `level.setBlock(bambooPos, BAMBOO_TRUNK, 2)`.
        - Move `bambooPos` up 1.
     4. **Top foliage**: if `bambooPos.y - origin.y >= 3`:
        - `level.setBlock(bambooPos, BAMBOO_FINAL_LARGE, 2)` *(note: placed 1 block above the last trunk block without checking `isEmptyBlock` — total stalk height is `placedTrunk + 1`!)*
        - Move `bambooPos` down 1: `level.setBlock(bambooPos, BAMBOO_TOP_LARGE, 2)`.
        - Move `bambooPos` down 1: `level.setBlock(bambooPos, BAMBOO_TOP_SMALL, 2)`.
  3. Return **`true`** *(quirk: returns `true` whenever step 1 passed, even if `canSurvive` was `false` and nothing was placed)*.

##### 4.23 `minecraft:tree` (`TreeFeature`)
- **Fields**:
  - `trunk_provider`: `Holder<BlockStateProvider>` (required)
  - `trunk_placer`: `TrunkPlacer` (required)
  - `foliage_provider`: `Holder<BlockStateProvider>` (required)
  - `foliage_placer`: `FoliagePlacer` (required)
  - `root_placer`: `Optional<RootPlacer>` (optional)
  - `minimum_size`: `FeatureSize` (required)
  - `decorators`: `List<TreeDecorator>` (required)
  - `ignore_vines`: `bool` (optional, default `false`)
  - `below_trunk_provider`: `Holder<BlockStateProvider>` (required)
- **Shared static helpers** (also used by `fallen_tree`):
  - `TreeFeature.validTreePos(level, pos)`: `state.isAir() || state.is(#minecraft:replaceable_by_trees)`.
  - `TreeFeature.isAirOrLeaves(level, pos)`: `state.isAir() || state.is(#minecraft:leaves)`.
  - `TreeFeature.isVine(level, pos)`: `state.is(minecraft:vine)`.
- **Behavior**: Full algorithm, trunk/foliage/root placers, minimum feature sizes, tree decorators, and leaves-distance voxel flood-fill are specified in [`docs/specs/worldgen/features-trees.md`](features-trees.md).

##### 4.24 `minecraft:fallen_tree` (`FallenTreeFeature`)
- **Fields**:
  - `trunk_provider`: `Holder<BlockStateProvider>` (required).
  - `log_length`: `IntProvider` in `[0, 16]` (required).
  - `stump_decorators`: `List<TreeDecorator>` (required).
  - `log_decorators`: `List<TreeDecorator>` (required).
- **Helpers**:
  - `isOverSolidGround(level, pos)`: `level.getBlockState(pos.below()).isFaceSturdy(level, pos, Direction.UP)` *(quirk: passes `pos` rather than `pos.below()` to `isFaceSturdy`!)*.
  - `mayPlaceOn(level, pos)`: `TreeFeature.validTreePos(level, pos) && isOverSolidGround(level, pos)`.
  - `placeLogBlock(level, random, pos, modifier)`: computes `state = modifier(trunk_provider.getState(level, random, pos))`, calls `level.setBlockAndUpdate(pos, state)` (`flags = 3`) and `markAboveForPostProcessing(level, pos)`, returns immutable `pos`.
  - `decorateLogs(level, random, logs, decorators)`: if `!decorators.isEmpty()`, constructs `TreeDecorator.Context(level, (p, s) -> level.setBlock(p, s, 19), random, logs, emptySet, emptySet)` and runs each decorator in order (see `features-trees.md`).
- **Behavior**:
  1. **Stump**: `stump = placeLogBlock(level, random, origin, identity)`. Run `decorateLogs(level, random, Set.of(stump), stump_decorators)`.
  2. **Fallen log geometry**:
     - `dir = Direction.Plane.HORIZONTAL.getRandomDirection(random)` (`[NORTH, EAST, SOUTH, WEST]`, 1 `nextInt(4)`).
     - `logLen = log_length.sample(random) - 2` (may be `<= 0`).
     - `logStart = origin.relative(dir, 2 + random.nextInt(2))`.
  3. **Ground snapping**: move `logStart` up 1, then loop `i = 0 .. 5` (6 checks): if `mayPlaceOn(level, logStart)`, stop; else move `logStart` down 1. (If none pass, `logStart` stays at `origin.y - 5`.)
  4. **Validation (`canPlaceEntireFallenLog`)**: set `gapInGround = 0`. Loop `i = 0 .. logLen - 1`:
     - If `!TreeFeature.validTreePos(level, logStart)`: abort log placement.
     - If `!isOverSolidGround(level, logStart)`: increment `gapInGround`; if `gapInGround > 2`, abort log placement. Else reset `gapInGround = 0`.
     - Move `logStart` 1 step in `dir`.
     - If loop completes, rewind `logStart` by `logLen` steps in `dir.getOpposite()`.
  5. **Log placement**: if validation passed, create `fallenLog = HashSet<BlockPos>()` *(note: iteration order inside `log_decorators` follows Java `HashSet<BlockPos>` order — see **[oracle]** item in Open questions)*. Loop `i = 0 .. logLen - 1`:
     - `fallenLog.add(placeLogBlock(level, random, logStart, s -> s.trySetValue(RotatedPillarBlock.AXIS, dir.getAxis())))`.
     - Move `logStart` 1 step in `dir`.
     - Run `decorateLogs(level, random, fallenLog, log_decorators)`.
  6. Always returns **`true`**.

---

#### Group D: Ice, snow, spikes, geodes, and cave speleothems (§4.25–§4.33)

##### 4.25 `minecraft:iceberg` (`IcebergFeature`)
- **Fields**: `state`: `BlockState` (required — main ice block, e.g. `packed_ice` or `blue_ice`).
- **Helper predicates & geometry**:
  - `isIcebergState(s)`: `s.is(packed_ice) || s.is(snow_block) || s.is(blue_ice)`.
  - `signedDistanceCircle(xo, zo, radius, random)` (all `f64` except `off` in `f32`):
    - `off = 10.0_f32 * clamp_f32(random.nextFloat(), 0.2_f32, 0.8_f32) / f32(radius)` *(if `radius == 0`, IEEE-754 `f32` division yields `+Infinity` so `signedDist < 0.0` is `false`)*
    - Return `f64(off) + f64(xo)^2 + f64(zo)^2 - f64(radius)^2`.
  - `signedDistanceEllipse(xo, zo, center, a, c, angle)` (`f64`, using `Math.cos`/`Math.sin`):
    - `rx = (f64(xo - center.x) * Math.cos(angle) - f64(zo - center.z) * Math.sin(angle)) / f64(a)`
    - `rz = (f64(xo - center.x) * Math.sin(angle) + f64(zo - center.z) * Math.cos(angle)) / f64(c)`
    - Return `rx * rx + rz * rz - 1.0`.
  - `getEllipseC(yOff, height, shapeEllipseC)`: if `yOff > 0 && height - yOff <= 3`, return `shapeEllipseC - (4 - (height - yOff))`, else `shapeEllipseC`.
  - `heightDependentRadiusRound(random, yOff, height, width)`:
    - `k = 3.5_f32 - random.nextFloat()`
    - `scale = (1.0_f32 - f32(f64(yOff)^2) / (f32(height) * k)) * f32(width)`
    - If `height > 15 + random.nextInt(5)`:
      - `tempYOff = (yOff < 3 + random.nextInt(6)) ? yOff / 2 : yOff`
      - `scale = (1.0_f32 - f32(tempYOff) / (f32(height) * k * 0.4_f32)) * f32(width)`
    - Return `Mth.ceil(scale / 2.0_f32)`.
  - `heightDependentRadiusEllipse(yOff, height, width)`: `Mth.ceil(((1.0_f32 - f32(f64(yOff)^2) / f32(height)) * f32(width)) / 2.0_f32)`.
  - `heightDependentRadiusSteep(random, yOff, height, width)`: `k = 1.0_f32 + random.nextFloat() / 2.0_f32`; return `Mth.ceil(((1.0_f32 - f32(yOff) / (f32(height) * k)) * f32(width)) / 2.0_f32)`.
  - `generateIcebergBlock(level, random, origin, height, xo, yOff, zo, radius, a, isEllipse, shapeEllipseC, shapeAngle, snowOnTop, mainState)`:
    1. `signedDist = isEllipse ? signedDistanceEllipse(xo, zo, ZERO, a, getEllipseC(yOff, height, shapeEllipseC), shapeAngle) : signedDistanceCircle(xo, zo, radius, random)`.
    2. If `signedDist < 0.0`:
       - `pos = origin.offset(xo, yOff, zo)`.
       - `compareVal = isEllipse ? -0.5 : f64(-6 - random.nextInt(3))`.
       - If `signedDist > compareVal && random.nextDouble() > 0.9`: return (skip block).
       - Call `setIcebergBlock(pos, level, random, hDiff = height - yOff, height, isEllipse, snowOnTop, mainState)`:
         - Read `cur = level.getBlockState(pos)`.
         - If `cur.isAir() || cur.is(snow_block) || cur.is(ice) || cur.is(water)`:
           - `randomness = !isEllipse || random.nextDouble() > 0.05`.
           - `divisor = isEllipse ? 3 : 2`.
           - If `snowOnTop && !cur.is(water) && f64(hDiff) <= f64(random.nextInt(max(1, height / divisor))) + f64(height) * 0.6 && randomness` *(note left-to-right short-circuiting: `random.nextInt` is drawn BEFORE checking `randomness`, whereas `randomness` was already computed above!)*:
             - `setBlock(level, pos, minecraft:snow_block)` (`flags = 3`).
           - Else:
             - `setBlock(level, pos, mainState)` (`flags = 3`).
- **Behavior**:
  1. Replace `origin` with `(origin.x, chunkGenerator.getSeaLevel(), origin.z)` (ignores `origin.y`).
  2. Initial RNG draws in exact order:
     - `snowOnTop = random.nextDouble() > 0.7`
     - `shapeAngle = random.nextDouble() * 2.0 * PI`
     - `shapeEllipseA = 11 - random.nextInt(5)` (in `[7, 11]`)
     - `shapeEllipseC = 3 + random.nextInt(3)` (in `[3, 5]`)
     - `isEllipse = random.nextDouble() > 0.7`
     - `overWaterHeight = isEllipse ? random.nextInt(6) + 6 : random.nextInt(15) + 3`
     - If `!isEllipse && random.nextDouble() > 0.9`: `overWaterHeight += random.nextInt(19) + 7`
     - `underWaterHeight = min(overWaterHeight + random.nextInt(11), 18)`
     - `width = min(overWaterHeight + random.nextInt(7) - random.nextInt(5), 11)`
     - `a = isEllipse ? shapeEllipseA : 11`.
  3. **Above-water body**: loop `xo = -a .. a - 1`, `zo = -a .. a - 1`, `yOff = 0 .. overWaterHeight - 1`:
     - `radius = isEllipse ? heightDependentRadiusEllipse(yOff, overWaterHeight, width) : heightDependentRadiusRound(random, yOff, overWaterHeight, width)`.
     - If `isEllipse || xo < radius` *(quirk: `xo < radius`, not `|xo| < radius`)*: call `generateIcebergBlock(...)`.
  4. **Smooth pass**: let `sa = isEllipse ? shapeEllipseA : width / 2`. Loop `x = -sa ..= sa`, `z = -sa ..= sa`, `yOff = 0 ..= overWaterHeight`:
     - At `pos = origin.offset(x, yOff, z)`, if `isIcebergState(state) || state.is(minecraft:snow)`:
       - If `level.getBlockState(pos.below()).isAir()`: set `pos` to `air` (`flags = 3`) AND set `pos.above()` to `air` (`flags = 3`).
       - Else if `isIcebergState(state)`: count how many of `[west, east, north, south]` are NOT `isIcebergState`; if `count >= 3`, set `pos` to `air` (`flags = 3`).
  5. **Under-water body**: loop `xo = -a .. a - 1`, `zo = -a .. a - 1`, `yOff = -1` down to `-(underWaterHeight - 1)`:
     - `newA = isEllipse ? Mth.ceil(f32(a) * (1.0_f32 - f32(f64(yOff)^2) / (f32(underWaterHeight) * 8.0_f32))) : a`.
     - `radius = heightDependentRadiusSteep(random, -yOff, underWaterHeight, width)`.
     - If `xo < radius`: call `generateIcebergBlock(..., underWaterHeight, ..., newA, ...)`.
  6. **Cut-out pass**: `doCutOut = isEllipse ? random.nextDouble() > 0.1 : random.nextDouble() > 0.7`. If `doCutOut`:
     - `signX = random.nextBoolean() ? -1 : 1`, `signZ = random.nextBoolean() ? -1 : 1`.
     - `xOff = random.nextInt(max(width / 2 - 2, 1))`; if `random.nextBoolean()`: `xOff = width / 2 + 1 - random.nextInt(max(width - width / 2 - 1, 1))`.
     - `zOff = random.nextInt(max(width / 2 - 2, 1))`; if `random.nextBoolean()`: `zOff = width / 2 + 1 - random.nextInt(max(width - width / 2 - 1, 1))`.
     - If `isEllipse`: `xOff = zOff = random.nextInt(max(shapeEllipseA - 5, 1))` *(note: previous `xOff`/`zOff` RNG calls still ran!)*.
     - `localOrigin = (signX * xOff, 0, signZ * zOff)`.
     - `angle = isEllipse ? shapeAngle + PI / 2.0 : random.nextDouble() * 2.0 * PI`.
     - Above water: for `yOff = 0 .. overWaterHeight - 4`: `r = heightDependentRadiusRound(random, yOff, overWaterHeight, width)`, `carve(r, yOff, origin, level, false, angle, localOrigin, shapeEllipseA, shapeEllipseC)`.
     - Under water: for `yOff = -1; yOff > -overWaterHeight + random.nextInt(5); yOff--` *(quirk: `random.nextInt(5)` is re-drawn on EVERY loop condition check, including the final failing check!)*: `r = heightDependentRadiusSteep(random, -yOff, overWaterHeight, width)`, `carve(r, yOff, origin, level, true, angle, localOrigin, shapeEllipseA, shapeEllipseC)`.
     - **`carve`**: `ca = r + 1 + shapeEllipseA / 3`, `cc = min(r - 3, 3) + shapeEllipseC / 2 - 1`. Loop `xo = -ca .. ca - 1`, `zo = -ca .. ca - 1`: if `signedDistanceEllipse(xo, zo, localOrigin, ca, cc, angle) < 0.0`, at `pos = origin.offset(xo, yOff, zo)`: if `isIcebergState(s) || s.is(snow_block)`: if `underWater` set `minecraft:water` (`flags = 3`); else set `minecraft:air` (`flags = 3`) and if `pos.above().is(minecraft:snow)` set `pos.above()` to `minecraft:air` (`flags = 3`).
  7. Always returns **`true`**.

##### 4.26 `minecraft:freeze_top_layer` (`SnowAndFreezeFeature`)
- **Fields**: none.
- **Behavior** (no RNG):
  1. Loop `dx = 0 .. 15` (outer), `dz = 0 .. 15` (inner):
     - `x = origin.x + dx`, `z = origin.z + dz`.
     - `y = level.getHeight(MOTION_BLOCKING, x, z)`.
     - `topPos = (x, y, z)`, `belowPos = (x, y - 1, z)`.
     - Look up `biome = level.getBiome(topPos)`.
     - **Freeze check** (`biome.shouldFreeze(level, belowPos, checkNeighbors = false)`):
       - True iff:
         1. `biome.getTemperature(belowPos, level.getSeaLevel()) < 0.15_f32` (not `warmEnoughToRain`; see `biomes-and-climate.md`),
         2. `level.isInsideBuildHeight(belowPos.y)` AND `level.getBrightness(LightLayer.BLOCK, belowPos) < 10` *(note: during `FEATURES`, `LIGHT` has not yet run — see **[oracle]** note in Open questions)*,
         3. `level.getFluidState(belowPos).is(Fluids.WATER)` (still water source, level 8) AND `level.getBlockState(belowPos).getBlock() instanceof LiquidBlock`.
       - If true: `level.setBlock(belowPos, minecraft:ice, 2)`.
     - **Snow check** (`biome.shouldSnow(level, topPos)`):
       - True iff:
         1. `biome.getPrecipitationAt(topPos, level.getSeaLevel()) == SNOW` (biome `has_precipitation == true` and `getTemperature(topPos, seaLevel) < 0.15_f32`),
         2. `level.isInsideBuildHeight(topPos.y)` AND `level.getBrightness(LightLayer.BLOCK, topPos) < 10`,
         3. `state(topPos)` is air or `minecraft:snow`, AND `minecraft:snow[layers=1].canSurvive(level, topPos)` (below block is not in `#minecraft:snow_layer_cannot_survive_on`, and either is in `#minecraft:snow_layer_can_survive_on` OR has a full top collision face OR is `minecraft:snow[layers=8]`).
       - If true:
         - `level.setBlock(topPos, minecraft:snow[layers=1], 2)`.
         - Re-read `belowState = level.getBlockState(belowPos)`: if it has boolean property `snowy`, `level.setBlock(belowPos, belowState.setValue(snowy, true), 2)`.
  2. Always returns **`true`**.

##### 4.27 `minecraft:blue_ice` (`BlueIceFeature`)
- **Fields**: none.
- **Behavior**:
  1. If `origin.y > level.getSeaLevel() - 1`: return `false`.
  2. If `!state(origin).is(water) && !state(origin.below()).is(water)`: return `false`.
  3. Scan `Direction.values()` skipping `DOWN`: if no neighbor is `minecraft:packed_ice`, return `false`.
  4. `level.setBlock(origin, minecraft:blue_ice, 2)`.
  5. Loop `i = 0 .. 199` (200 iterations):
     - `yOff = random.nextInt(5) - random.nextInt(6)`.
     - `xzDiff = 3 + (yOff < 2 ? yOff / 2 : 0)` *(Java integer division toward zero)*.
     - If `xzDiff >= 1`:
       - `dx = random.nextInt(xzDiff) - random.nextInt(xzDiff)`.
       - `dz = random.nextInt(xzDiff) - random.nextInt(xzDiff)`.
       - `placePos = origin.offset(dx, yOff, dz)`.
       - `s = level.getBlockState(placePos)`.
       - If `s.isAir() || s.is(water) || s.is(packed_ice) || s.is(ice)`:
         - Check all 6 `Direction.values()` neighbors of `placePos`: on the first neighbor that is `minecraft:blue_ice`, `level.setBlock(placePos, minecraft:blue_ice, 2)` and break.
  6. Return **`true`**.

##### 4.28 `minecraft:spike` (`SpikeFeature`) — *generalizes pre-26.3 `ice_spike`*
- **Fields**: `state`: `BlockState`, `can_place_on`: `BlockPredicate`, `can_replace`: `BlockPredicate` (all required).
- **Behavior**:
  1. Scan downward: while `level.isEmptyBlock(origin) && origin.y > level.minY + 2`, set `origin = origin.below()`.
  2. If `!can_place_on.test(level, origin)`: return `false`.
  3. Draw in exact order:
     - `origin = origin.above(random.nextInt(4))`.
     - `height = random.nextInt(4) + 7` (in `[7, 10]`).
     - `width = height / 4 + random.nextInt(2)`.
     - If `width > 1 && random.nextInt(60) == 0`: `origin = origin.above(10 + random.nextInt(30))`.
  4. **Double cone**: loop `yOff = 0 .. height - 1`:
     - `scale = (1.0_f32 - f32(yOff) / f32(height)) * f32(width)`; `newWidth = Mth.ceil(scale)`.
     - Loop `xo = -newWidth ..= newWidth`, `zo = -newWidth ..= newWidth`:
       - `dx = f32(|xo|) - 0.25_f32`, `dz = f32(|zo|) - 0.25_f32`.
       - Check `(xo == 0 && zo == 0) || !(dx * dx + dz * dz > scale * scale)`.
       - If that holds AND either `(|xo| < newWidth && |zo| < newWidth)` OR `!(random.nextFloat() > 0.75_f32)` *(note: RNG only drawn on the outer perimeter `|xo| == newWidth || |zo| == newWidth`)*:
         - At `posUp = origin.offset(xo, yOff, zo)`: if `state(posUp).isAir() || can_replace.test(level, posUp)`: `setBlock(level, posUp, state)` (`flags = 3`).
         - If `yOff != 0 && newWidth > 1`: at `posDown = origin.offset(xo, -yOff, zo)`: if `state(posDown).isAir() || can_replace.test(level, posDown)`: `setBlock(level, posDown, state)` (`flags = 3`).
  5. **Root pillar**: `pillarWidth = clamp(width - 1, 0, 1)`. Loop `xo = -pillarWidth ..= pillarWidth`, `zo = -pillarWidth ..= pillarWidth`:
     - `cursor = origin.offset(xo, -1, zo)`.
     - `runLength = (|xo| == 1 && |zo| == 1) ? random.nextInt(5) : 50`.
     - While `cursor.y > 50` *(hardcoded world Y = 50!)*:
       - `cur = level.getBlockState(cursor)`.
       - If `!cur.isAir() && !can_replace.test(level, cursor) && cur != state`: break *(exact `BlockState` identity/equality comparison `!= state`!)*.
       - `setBlock(level, cursor, state)` (`flags = 3`).
       - `cursor = cursor.below()`.
       - If `--runLength <= 0`: `cursor = cursor.below(random.nextInt(5) + 1)`; `runLength = random.nextInt(5)`.
  6. Return **`true`**.

##### 4.29 `minecraft:geode` (`GeodeFeature`)
- **Fields**:
  - `blocks` (`GeodeBlockSettings`, required):
    - `filling_provider`, `inner_layer_provider`, `alternate_inner_layer_provider`, `middle_layer_provider`, `outer_layer_provider`: `Holder<BlockStateProvider>` (all required)
    - `inner_placements`: non-empty `List<BlockState>` (required)
    - `cannot_replace`, `invalid_blocks`: `HolderSet<Block>` (both required)
  - `layers` (`GeodeLayerSettings`, required; each `f64` in `[0.01, 50.0]`):
    - `filling` (default `1.7`), `inner_layer` (default `2.2`), `middle_layer` (default `3.2`), `outer_layer` (default `4.2`)
  - `crack` (`GeodeCrackSettings`, required):
    - `generate_crack_chance` (`f64` in `[0.0, 1.0]`, default `1.0`), `base_crack_size` (`f64` in `[0.0, 5.0]`, default `2.0`), `crack_point_offset` (`i32` in `[0, 10]`, default `2`)
  - `use_potential_placements_chance`: `f64` in `[0.0, 1.0]` (default `0.35`)
  - `use_alternate_layer0_chance`: `f64` in `[0.0, 1.0]` (default `0.0`)
  - `placements_require_layer0_alternate`: `bool` (default `true`)
  - `outer_wall_distance`: `IntProvider` in `[1, 20]` (default `uniform(4, 5)`)
  - `distribution_points`: `IntProvider` in `[1, 20]` (default `uniform(3, 4)`)
  - `point_offset`: `IntProvider` in `[0, 10]` (default `uniform(1, 2)`)
  - `min_gen_offset`: `i32` (default `-16`), `max_gen_offset`: `i32` (default `16`)
  - `noise_multiplier`: `f64` in `[0.0, 1.0]` (default `0.05`)
  - `invalid_blocks_threshold`: `i32` (required).
- **Behavior**:
  1. `numPoints = distribution_points.sample(random)`.
  2. Construct world-seeded 3D `NormalNoise` with first octave `-4`, amplitudes `[1.0]` (parity variant, see `docs/specs/math/noise.md`), seeded by `new WorldgenRandom(new LegacyRandomSource(level.getSeed()))`.
  3. Compute inverse-square-root thresholds (`f64`):
     - `adj = f64(numPoints) / f64(outer_wall_distance.maxInclusive())`.
     - `innerAir = 1.0 / sqrt(layers.filling)`
     - `innermostBlockLayer = 1.0 / sqrt(layers.inner_layer + adj)`
     - `innerCrust = 1.0 / sqrt(layers.middle_layer + adj)`
     - `outerCrust = 1.0 / sqrt(layers.outer_layer + adj)`
     - `crackSize = 1.0 / sqrt(crack.base_crack_size + random.nextDouble() / 2.0 + (numPoints > 3 ? adj : 0.0))`
     - `shouldGenerateCrack = f64(random.nextFloat()) < crack.generate_crack_chance` *(note `nextFloat()` promoted to `f64`!)*.
  4. **Sample distribution points**: loop `i = 0 .. numPoints - 1`:
     - Sample `x = outer_wall_distance.sample(random)`, `y = outer_wall_distance.sample(random)`, `z = outer_wall_distance.sample(random)` in `X, Y, Z` order.
     - `pos = origin.offset(x, y, z)`; read `s = level.getBlockState(pos)`.
     - If `s.isAir() || s.is(blocks.invalid_blocks)`: increment `numInvalidPoints`; if `numInvalidPoints > invalid_blocks_threshold`, return **`false`** immediately.
     - Append `(pos, point_offset.sample(random))` to `points`.
  5. **Crack points**: if `shouldGenerateCrack`:
     - `offsetIndex = random.nextInt(4)`; `co = numPoints * 2 + 1`.
     - Add 3 points to `crackPoints` at Y offsets `7, 5, 1`:
       - `0`: `(co, y, 0)`; `1`: `(0, y, co)`; `2`: `(co, y, co)`; `3`: `(0, y, 0)`.
  6. **Shell placement**: let `canReplace(s) = !s.is(blocks.cannot_replace)`. Allocate list `potentialCrystalPlacements`.
     Iterate `p` in `BlockPos.betweenClosed(origin + (min_gen_offset, min_gen_offset, min_gen_offset), origin + (max_gen_offset, max_gen_offset, max_gen_offset))` (`z` outer, `y` mid, `x` inner):
     - `noiseOffset = noise.get(p.x, p.y, p.z) * noise_multiplier`.
     - `distSumShell = sum over (pt, off) in points of (invSqrt(p.distSqr(pt) + off) + noiseOffset)` *(note: `noiseOffset` is added once per point! `invSqrt(x) = 1.0 / sqrt(x)` in `f64`)*.
     - If `!(distSumShell < outerCrust)`:
       - If `distSumShell >= innerAir`: `safeSetBlock(level, p, filling_provider.getState(level, random, p), canReplace)` (`flags = 3`).
       - Else:
         - `distSumCrack = sum over cp in crackPoints of (invSqrt(p.distSqr(cp) + crack.crack_point_offset) + noiseOffset)`.
         - If `shouldGenerateCrack && distSumCrack >= crackSize`:
           - `safeSetBlock(level, p, minecraft:air, canReplace)`.
           - For each `dir` in `Direction.values()`: let `adjFluid = level.getFluidState(p.relative(dir))`; if `!adjFluid.isEmpty()`, `level.scheduleTick(p.relative(dir), adjFluid.getType(), 0)`.
         - Else if `distSumShell >= innermostBlockLayer`:
           - `useAlt = f64(random.nextFloat()) < use_alternate_layer0_chance`.
           - Place via `alternate_inner_layer_provider` if `useAlt`, else `inner_layer_provider` (`safeSetBlock`).
           - If `(!placements_require_layer0_alternate || useAlt) && f64(random.nextFloat()) < use_potential_placements_chance`: append `p.immutable()` to `potentialCrystalPlacements`.
         - Else if `distSumShell >= innerCrust`: `safeSetBlock` with `middle_layer_provider`.
         - Else if `distSumShell >= outerCrust`: `safeSetBlock` with `outer_layer_provider`.
  7. **Inner crystal placements**: for each `crystalPos` in `potentialCrystalPlacements`:
     - `blockState = inner_placements[random.nextInt(inner_placements.size())]`.
     - For each `dir` in `Direction.values()` (`DOWN, UP, NORTH, SOUTH, WEST, EAST`):
       - If `blockState` has `FACING`, set `FACING = dir`.
       - `placePos = crystalPos.relative(dir)`; `placeState = level.getBlockState(placePos)`.
       - If `blockState` has `WATERLOGGED`, set `WATERLOGGED = placeState.getFluidState().isSource()`.
       - If `BuddingAmethystBlock.canClusterGrowAtState(placeState)` (i.e. `placeState.isAir() || (placeState.is(minecraft:water) && placeState.getFluidState().isFull())`):
         - `safeSetBlock(level, placePos, blockState, canReplace)` and break.
  8. Return **`true`**.

##### 4.30 `minecraft:large_dripstone` (`LargeDripstoneFeature`)
- **Fields**:
  - `replaceable_blocks`: `HolderSet<Block>` (required)
  - `floor_to_ceiling_search_range`: `i32` in `[1, 512]` (optional, default `30`)
  - `column_radius`: `IntProvider` in `[1, 16]` (required — only `minInclusive()` and `maxInclusive()` are used!)
  - `height_scale`: `FloatProvider` in `[0.0, 20.0]` (required)
  - `max_column_radius_to_cave_height_ratio`: `f32` in `[0.1, 1.0]` (required)
  - `stalactite_bluntness`, `stalagmite_bluntness`: `FloatProvider` in `[0.1, 10.0]` (required)
  - `wind_speed`: `FloatProvider` in `[0.0, 2.0]` (required)
  - `min_radius_for_wind`: `i32` in `[0, 100]` (required)
  - `min_bluntness_for_wind`: `f32` in `[0.0, 5.0]` (required).
- **Shared `SpeleothemUtils` helpers**:
  - `isEmptyOrWater(s)`: `s.isAir() || s.is(water)`.
  - `isEmptyOrWaterOrLava(s)`: `s.isAir() || s.is(water) || s.is(lava)`.
  - `isBase(s, baseBlock, replaceable)`: `s.is(baseBlock) || s.is(replaceable)`.
  - `isBaseOrLava(s, baseBlock, replaceable)`: `isBase(...) || s.is(lava)`.
  - `getSpeleothemHeight(xzDist, radius, scale, bluntness)` (all `f64`):
    - `d = max(xzDist, bluntness)`
    - `r = d / radius * 0.384`
    - `part1 = 0.75 * r^(4.0/3.0)` (exact double `1.3333333333333333`)
    - `part2 = r^(2.0/3.0)` (exact double `0.6666666666666666`)
    - `part3 = (1.0/3.0) * ln(r)` (exact double `0.3333333333333333`)
    - `hRel = max(scale * (part1 - part2 - part3), 0.0)`
    - Return `hRel / 0.384 * radius`.
  - `isCircleMostlyEmbeddedInStone(level, center, xzRadius)`:
    - If `isEmptyOrWaterOrLava(state(center))`: return `false`.
    - `angleInc = 6.0_f32 / f32(xzRadius)`.
    - Loop `angle = 0.0_f32` while `angle < f32(2.0 * PI)` (`6.2831855_f32`), `angle += angleInc`:
      - `dx = (i32)(Mth.cos(angle) * f32(xzRadius))`, `dz = (i32)(Mth.sin(angle) * f32(xzRadius))`.
      - If `isEmptyOrWaterOrLava(state(center.offset(dx, 0, dz)))`: return `false`.
    - Return `true`.
- **Behavior**:
  1. If `!isEmptyOrWater(level, origin)`: return `false`.
  2. Call `Column.scan(level, origin, floor_to_ceiling_search_range, isEmptyOrWater, s -> isBaseOrLava(s, dripstone_block, replaceable_blocks))`. Must return a `Column.Range` with `height >= 4`, else return `false`.
  3. Compute radius:
     - `maxByHeight = (i32)(f32(range.height) * max_column_radius_to_cave_height_ratio)` (truncation toward zero).
     - `maxColR = clamp(maxByHeight, column_radius.minInclusive(), column_radius.maxInclusive())`.
     - `radius = Mth.randomBetweenInclusive(random, column_radius.minInclusive(), maxColR)` (`random.nextInt(maxColR - min + 1) + min`).
  4. Construct `stalactite` then `stalagmite` in exact order:
     - `stalactite`: `root = origin.atY(range.ceiling - 1)`, `pointingUp = false`, `radius`, `bluntness = stalactite_bluntness.sample(random)`, `scale = height_scale.sample(random)`.
     - `stalagmite`: `root = origin.atY(range.floor + 1)`, `pointingUp = true`, `radius`, `bluntness = stalagmite_bluntness.sample(random)`, `scale = height_scale.sample(random)`.
  5. **Wind**: if both `stalactite` and `stalagmite` have `radius >= min_radius_for_wind && bluntness >= f64(min_bluntness_for_wind)`:
     - `speed = wind_speed.sample(random)`.
     - `dir = Mth.randomBetween(random, 0.0_f32, f32(PI))` (`random.nextFloat() * 3.1415927_f32`).
     - `windVec = (f64(Mth.cos(dir) * speed), 0.0, f64(Mth.sin(dir) * speed))`, `maxOffset = 16 - radius`.
     - `wind.offset(pos)`: `dy = origin.y - pos.y`; `dx = clamp(floor(windVec.x * dy), -maxOffset, maxOffset)`; `dz = clamp(floor(windVec.z * dy), -maxOffset, maxOffset)`; return `pos.offset(dx, 0, dz)`.
     - Else: no wind (`wind.offset(pos) = pos`).
  6. **Embed bases**: run `moveBackUntilBaseIsInsideStoneAndShrinkRadiusIfNecessary` on `stalactite`, then `stalagmite`:
     - While `this.radius > 1`:
       - `newRoot = this.root`; `maxTries = min(10, (i32)getSpeleothemHeight(0.0, this.radius, this.scale, this.bluntness))`.
       - For `i = 0 .. maxTries - 1`:
         - If `level.getBlockState(newRoot).is(lava)`: return `false`.
         - If `isCircleMostlyEmbeddedInStone(level, wind.offset(newRoot), this.radius)`: set `this.root = newRoot` and return `true`.
         - Move `newRoot` `DOWN` (if `pointingUp`) or `UP` (if `!pointingUp`).
       - `this.radius /= 2`.
     - Return `false`.
  7. **Place blocks**: if `stalactite` embedded, run `stalactite.placeBlocks`; then if `stalagmite` embedded, run `stalagmite.placeBlocks`:
     - Loop `dx = -radius ..= radius` (outer), `dz = -radius ..= radius` (inner):
       - `curR = Mth.sqrt(f32(dx * dx + dz * dz))`; if `curR > f32(radius)` skip.
       - `h = (i32)getSpeleothemHeight(f64(curR), f64(radius), scale, bluntness)`.
       - If `h > 0`:
         - If `f64(random.nextFloat()) < 0.2`: `h = (i32)(f32(h) * (random.nextFloat() * 0.2_f32 + 0.8_f32))`.
         - `pos = root.offset(dx, 0, dz)`; `hasBeenOutOfStone = false`.
         - `maxY = pointingUp ? level.getHeight(WORLD_SURFACE_WG, pos.x, pos.z) : i32::MAX`.
         - For `i = 0 .. h - 1` while `pos.y < maxY`:
           - `wPos = wind.offset(pos)`.
           - If `isEmptyOrWaterOrLava(level, wPos)`: `hasBeenOutOfStone = true`; `level.setBlock(wPos, minecraft:dripstone_block, 2)`.
           - Else if `hasBeenOutOfStone && level.getBlockState(wPos).is(#minecraft:base_stone_overworld)`: break.
           - Move `pos` `UP` (if `pointingUp`) or `DOWN` (if `!pointingUp`).
  8. Return **`true`**.

##### 4.31 `minecraft:root_system` (`RootSystemFeature`)
- **Fields**:
  - `feature`: `Holder<PlacedFeature>` (required)
  - `required_vertical_space_for_tree`: `i32` in `[1, 64]`
  - `level_test_distance`: `i32` in `[0, 16]`
  - `max_level_deviation`: `i32` in `[0, 64]`
  - `root_radius`: `i32` in `[1, 64]`
  - `root_replaceable`: `HolderSet<Block>`
  - `root_state_provider`: `Holder<BlockStateProvider>`
  - `root_placement_attempts`: `i32` in `[1, 256]`
  - `root_column_max_height`: `i32` in `[1, 4096]`
  - `hanging_root_radius`: `i32` in `[1, 64]`
  - `hanging_roots_vertical_span`: `i32` in `[1, 16]`
  - `hanging_root_state_provider`: `Holder<BlockStateProvider>`
  - `hanging_root_placement_attempts`: `i32` in `[1, 256]`
  - `allowed_vertical_water_for_tree`: `i32` in `[1, 64]`
  - `allowed_tree_position`: `BlockPredicate`.
- **Behavior**:
  1. If `!level.getBlockState(origin).isAir()`: return **`false`**.
  2. **`placeDirtAndTree`**: set `workingPos = origin`. Loop `y = 0 .. root_column_max_height - 1`:
     - Move `workingPos` UP 1.
     - If `level.getHeight(WORLD_SURFACE, workingPos.x, workingPos.z) < workingPos.y`: stop loop (`placed = false`).
     - If `allowed_tree_position.test(level, workingPos)` AND `spaceForTree(level, workingPos)`:
       - `spaceForTree`:
         1. For `i = 1 ..= required_vertical_space_for_tree`: inspect `workingPos.above(i)`. Pass iff `s.isAir() || (i + 1 <= allowed_vertical_water_for_tree && s.getFluidState().is(#minecraft:water))`.
         2. If `level_test_distance > 0`: for `i = 0 .. 3` (`Direction.from2DDataValue(i)`: `SOUTH, WEST, NORTH, EAST`), let `corner = workingPos.relative(dir, level_test_distance)`; fail if `state(corner.below(max_level_deviation)).isAir() || !state(corner.above(max_level_deviation)).isAir()`.
       - Check `belowPos = workingPos.below()`: if `level.getFluidState(belowPos).is(#minecraft:lava) || !level.getBlockState(belowPos).isSolid()`: stop loop (`placed = false`).
       - Call `feature.place(level, chunkGenerator, random, workingPos)`: if `true`:
         - **`placeDirt`**: for `yy = origin.y .. origin.y + y - 1` *(note: stops 1 below `workingPos.y = origin.y + y + 1`, wait: `targetHeight = origin.y + y`, so `yy < origin.y + y`, stopping 2 below `workingPos.y`!)*:
           - For `i = 0 .. root_placement_attempts - 1`:
             - `dx = random.nextInt(root_radius) - random.nextInt(root_radius)`.
             - `dz = random.nextInt(root_radius) - random.nextInt(root_radius)`.
             - `p = (origin.x + dx, yy, origin.z + dz)`.
             - If `level.getBlockState(p).is(root_replaceable)`: `level.setBlock(p, root_state_provider.getState(level, random, p), 2)`.
         - Mark `placed = true` and break.
  3. **`placeRoots`** (only if `placeDirtAndTree` returned `true`):
     - Loop `i = 0 .. hanging_root_placement_attempts - 1`:
       - `dx = random.nextInt(hanging_root_radius) - random.nextInt(hanging_root_radius)`.
       - `dy = random.nextInt(hanging_roots_vertical_span) - random.nextInt(hanging_roots_vertical_span)`.
       - `dz = random.nextInt(hanging_root_radius) - random.nextInt(hanging_root_radius)`.
       - `p = origin.offset(dx, dy, dz)`.
       - If `level.isEmptyBlock(p)`:
         - `targetState = hanging_root_state_provider.getState(level, random, p)`.
         - If `targetState.canSurvive(level, p)` AND `level.getBlockState(p.above()).isFaceSturdy(level, p, Direction.DOWN)` *(quirk: passes `p` rather than `p.above()` to `isFaceSturdy`!)*:
           - `level.setBlock(p, targetState, 2)`.
  4. Always returns **`true`** (whenever `origin` was air in step 1, even if `placeDirtAndTree` returned `false`!).

##### 4.32 `minecraft:speleothem` (`SpeleothemFeature`) — *replaces pre-26.3 `pointed_dripstone`*
- **Fields**:
  - `base_block`: `BlockState` (only its `Block` is used, required)
  - `pointed_block`: `BlockState` (only its `Block` is used, required)
  - `replaceable_blocks`: `HolderSet<Block>` (required)
  - `chance_of_taller_generation`: `f32` in `[0.0, 1.0]` (default `0.2`)
  - `chance_of_directional_spread`: `f32` in `[0.0, 1.0]` (default `0.7`)
  - `chance_of_spread_radius2`: `f32` in `[0.0, 1.0]` (default `0.5`)
  - `chance_of_spread_radius3`: `f32` in `[0.0, 1.0]` (default `0.5`).
- **Shared `SpeleothemUtils.growSpeleothem(level, startPos, tipDir, height, mergedTip, baseBlock, pointedBlock, replaceable)`**:
  - Only runs if `isBase(level.getBlockState(startPos.relative(tipDir.getOpposite())), baseBlock, replaceable)`.
  - Produces `height` states from base to tip:
    - If `height >= 3`: one `BASE`, then `height - 3` `MIDDLE`.
    - If `height >= 2`: one `FRUSTUM`.
    - If `height >= 1`: one `TIP_MERGE` (if `mergedTip`) or `TIP` (else).
  - For each state in order along `pos = startPos`, stepping in `tipDir`: sets `TIP_DIRECTION = tipDir`, `THICKNESS = ...`, `WATERLOGGED = level.isWaterAt(pos)`, then `level.setBlock(pos, state, 2)`.
- **Behavior**:
  1. **Determine `tipDirection`**:
     - `canAbove = isBase(state(origin.above()), base_block.block, replaceable_blocks)`.
     - `canBelow = isBase(state(origin.below()), base_block.block, replaceable_blocks)`.
     - Both true: `random.nextBoolean() ? DOWN : UP`; only `canAbove`: `DOWN`; only `canBelow`: `UP`; neither: return **`false`**.
  2. **Patch of base blocks** centered at `rootPos = origin.relative(tipDirection.getOpposite())`:
     - `placeBaseBlockIfPossible(rootPos)` (if `state.is(replaceable_blocks)`, `level.setBlock(p, base_block.block.defaultState, 2)`).
     - For each `dir` in `Direction.Plane.HORIZONTAL` (`NORTH, EAST, SOUTH, WEST`):
       - If `!(random.nextFloat() > chance_of_directional_spread)` (i.e. `<=`):
         - `pos1 = rootPos.relative(dir)`; `placeBaseBlockIfPossible(pos1)`.
         - If `!(random.nextFloat() > chance_of_spread_radius2)`:
           - `pos2 = pos1.relative(Direction.getRandom(random))`; `placeBaseBlockIfPossible(pos2)`.
           - If `!(random.nextFloat() > chance_of_spread_radius3)`:
             - `pos3 = pos2.relative(Direction.getRandom(random))`; `placeBaseBlockIfPossible(pos3)`.
  3. **Height**: `height = (random.nextFloat() < chance_of_taller_generation && isEmptyOrWater(state(origin.relative(tipDirection)))) ? 2 : 1` *(short-circuit: `nextFloat()` is always drawn first)*.
  4. Call `growSpeleothem(level, origin, tipDirection, height, false, base_block.block, pointed_block.block, replaceable_blocks)`.
  5. Return **`true`**.

##### 4.33 `minecraft:speleothem_cluster` (`SpeleothemClusterFeature`) — *replaces pre-26.3 `dripstone_cluster`*
- **Fields**:
  - `base_block`, `pointed_block`: `BlockState` (only their `Block`s used, required)
  - `replaceable_blocks`: `HolderSet<Block>` (required)
  - `floor_to_ceiling_search_range`: `i32` in `[1, 512]`
  - `height`: `IntProvider` in `[1, 128]`
  - `radius`: `IntProvider` in `[1, 128]`
  - `max_stalagmite_stalactite_height_diff`: `i32` in `[0, 64]`
  - `height_deviation`: `i32` in `[1, 64]`
  - `speleothem_block_layer_thickness`: `IntProvider` in `[0, 128]`
  - `density`, `wetness`: `FloatProvider` in `[0.0, 2.0]`
  - `chance_of_speleothem_at_max_distance_from_center`: `f32` in `[0.0, 1.0]`
  - `max_distance_from_edge_affecting_chance_of_speleothem`: `i32` in `[1, 64]`
  - `max_distance_from_center_affecting_height_bias`: `i32` in `[1, 64]`.
- **Behavior**:
  1. If `!isEmptyOrWater(level, origin)`: return **`false`**.
  2. Sample cluster parameters in exact order:
     - `clusterHeight = height.sample(random)`
     - `wet = wetness.sample(random)`
     - `dens = density.sample(random)`
     - `xRadius = radius.sample(random)`
     - `zRadius = radius.sample(random)`.
  3. Loop `dx = -xRadius ..= xRadius` (outer), `dz = -zRadius ..= zRadius` (inner):
     - `distFromEdge = min(xRadius - |dx|, zRadius - |dz|)`.
     - `chance = f64(clampedMap_f32(f32(distFromEdge), 0.0_f32, f32(max_distance_from_edge_affecting_chance_of_speleothem), chance_of_speleothem_at_max_distance_from_center, 1.0_f32))`.
     - Scan `baseCol = Column.scan(level, origin.offset(dx, 0, dz), floor_to_ceiling_search_range, isEmptyOrWater, isNeitherEmptyNorWater)`.
     - If `baseCol` is empty, or both `ceiling` and `baseFloor` are absent: continue.
     - **Pool**: `wantPool = random.nextFloat() < wet`. If `wantPool && baseFloor.isPresent() && canPlacePool(level, pos.atY(baseFloor))`:
       - `canPlacePool(p)`: `state(p)` is not `water`, `base_block`, or `pointed_block`; `state(p.above())` fluid is not `#minecraft:water`; all 4 horizontal neighbors and `p.below()` satisfy `s.is(#minecraft:base_stone_overworld) || s.getFluidState().is(#minecraft:water)`.
       - Set `column = baseCol.withFloor(baseFloor - 1)` and `level.setBlock(pos.atY(baseFloor), minecraft:water, 2)`.
     - Let `floor = column.getFloor()`.
     - **Stalactite**: `wantStalactite = random.nextDouble() < chance` *(always drawn, even when `ceiling` is absent!)*.
       - If `ceiling.isPresent() && wantStalactite && !state(pos.atY(ceiling)).is(lava)`:
         - `ceilingThickness = speleothem_block_layer_thickness.sample(random)`; replace up to `ceilingThickness` consecutive blocks moving `UP` from `ceiling` via `placeBaseBlockIfPossible` (stopping on first failure).
         - `maxH = floor.isPresent() ? min(clusterHeight, ceiling - floor) : clusterHeight` *(note: `ceiling - floor`, not `ceiling - floor - 1`)*.
         - `stalactiteHeight = sampleHeight(random, dx, dz, dens, maxH)`: if `random.nextFloat() > dens` return `0`; else `dist = |dx| + |dz|`, `mean = f32(clampedMap_f64(dist, 0.0, max_distance_from_center_affecting_height_bias, maxH / 2.0, 0.0))`, return `(i32)ClampedNormalFloat.sample(random, mean, f32(height_deviation), 0.0_f32, f32(maxH))`.
       - Else `stalactiteHeight = 0`.
     - **Stalagmite**: `wantStalagmite = random.nextDouble() < chance` *(always drawn!)*.
       - If `floor.isPresent() && wantStalagmite && !state(pos.atY(floor)).is(lava)`:
         - `floorThickness = speleothem_block_layer_thickness.sample(random)`; replace up to `floorThickness` blocks moving `DOWN` from `floor`.
         - If `ceiling.isPresent()`: `stalagmiteHeight = max(0, stalactiteHeight + Mth.randomBetweenInclusive(random, -max_stalagmite_stalactite_height_diff, max_stalagmite_stalactite_height_diff))`.
         - Else: `stalagmiteHeight = sampleHeight(random, dx, dz, dens, clusterHeight)`.
       - Else `stalagmiteHeight = 0`.
     - **Collision resolution**: if `ceiling.isPresent() && floor.isPresent() && ceiling - stalactiteHeight <= floor + stalagmiteHeight`:
       - `lowBot = max(ceiling - stalactiteHeight, floor + 1)`.
       - `highTop = min(floor + stalagmiteHeight, ceiling - 1)`.
       - `actualBot = Mth.randomBetweenInclusive(random, lowBot, highTop + 1)`.
       - `actualStalactiteHeight = ceiling - actualBot`, `actualStalagmiteHeight = (actualBot - 1) - floor`.
     - Else keep raw heights.
     - **Merge check**: `mergeTips = random.nextBoolean() && actualStalactiteHeight > 0 && actualStalagmiteHeight > 0 && column.height.isPresent() && actualStalactiteHeight + actualStalagmiteHeight == column.height` *(quirk: `random.nextBoolean()` is ALWAYS drawn first, even when both heights are 0!)*.
     - If `ceiling.isPresent()`: `growSpeleothem(level, pos.atY(ceiling - 1), DOWN, actualStalactiteHeight, mergeTips, ...)`.
     - If `floor.isPresent()`: `growSpeleothem(level, pos.atY(floor + 1), UP, actualStalagmiteHeight, mergeTips, ...)`.
  4. Return **`true`**.

---

#### Group E: Plants, corals, Nether formations, underwater magma, and sculk (§4.34–§4.41)

##### 4.34 `minecraft:chorus_plant` (`ChorusPlantFeature`)
- **Fields**: none.
- **Behavior**:
  1. If `!level.isEmptyBlock(origin) || !level.getBlockState(origin.below()).is(#minecraft:supports_chorus_plant)`: return **`false`**.
  2. Call `ChorusFlowerBlock.generatePlant(level, origin, random, maxHorizontalSpread = 8)`:
     - Place `ChorusPlantBlock.getStateWithConnections(level, origin, chorus_plant)` at `origin` (`flags = 2`), where `getStateWithConnections` sets `DOWN = down.is(chorus_plant) || down.is(chorus_flower) || down.is(#minecraft:supports_chorus_plant)` and each of `UP, NORTH, EAST, SOUTH, WEST` to `neighbor.is(chorus_plant) || neighbor.is(chorus_flower)`.
     - Recurse `growTreeRecursive(level, current = origin, random, startPos = origin, maxHorizontalSpread = 8, depth = 0)`:
       1. `height = random.nextInt(4) + 1 + (depth == 0 ? 1 : 0)`.
       2. For `i = 0 .. height - 1`:
          - `target = current.above(i + 1)`.
          - If any of the 4 horizontal neighbors of `target` is non-empty (`!isEmptyBlock`): return immediately (without placing a flower!).
          - Place `getStateWithConnections(level, target, chorus_plant)` at `target` (`flags = 2`).
          - Re-place `getStateWithConnections(level, target.below(), chorus_plant)` at `target.below()` (`flags = 2`).
       3. `placedStem = false`. If `depth < 4`:
          - `stems = random.nextInt(4) + (depth == 0 ? 1 : 0)`.
          - Loop `i = 0 .. stems - 1`:
            - `dir = Direction.Plane.HORIZONTAL.getRandomDirection(random)`.
            - `target = current.above(height).relative(dir)`.
            - If `|target.x - startPos.x| < 8 && |target.z - startPos.z| < 8 && isEmptyBlock(target) && isEmptyBlock(target.below())` AND all 3 horizontal neighbors of `target` other than `dir.getOpposite()` are empty:
              - `placedStem = true`.
              - Place `getStateWithConnections` at `target` (`flags = 2`) and at `target.relative(dir.getOpposite())` (`flags = 2`).
              - Recurse `growTreeRecursive(level, target, random, startPos, 8, depth + 1)`.
       4. If `!placedStem`: place `minecraft:chorus_flower[age=5]` at `current.above(height)` (`flags = 2`).
  3. Return **`true`**.

##### 4.35 `minecraft:vines` (`VinesFeature`)
- **Fields**: none.
- **Behavior** (no RNG):
  1. If `!level.isEmptyBlock(origin)`: return **`false`**.
  2. Iterate `dir` in `Direction.values()` (`DOWN, UP, NORTH, SOUTH, WEST, EAST`), skipping `DOWN`:
     - If `VineBlock.isAcceptableNeighbour(level, origin.relative(dir), dir)` (equivalent to `MultifaceBlock.canAttachTo(level, origin.relative(dir), dir.getOpposite())`, i.e. neighbor's opposite face is full square collision face and block is not in the non-attachable set — `Block.isFaceFull(collisionShape, dir.getOpposite())`):
       - `level.setBlock(origin, minecraft:vine[<dir>=true], 2)` and return **`true`**.
  3. Return **`false`**.

##### 4.36 `minecraft:coral_claw` (`CoralClawFeature`) — *refactored in 26.3*
- **Fields**: `feature`: `Holder<PlacedFeature>` (required).
- **Behavior**:
  1. Call `feature.place(level, chunkGenerator, random, origin)`. If `false`, return **`false`**.
  2. `clawDir = Direction.Plane.HORIZONTAL.getRandomDirection(random)`.
  3. `nBranches = random.nextInt(2) + 2` (2 or 3).
  4. `possibleDirs = Util.toShuffledList([clawDir, clawDir.getClockWise(), clawDir.getCounterClockWise()], random)`.
  5. For each `branchDir` in `possibleDirs[0 .. nBranches - 1]`:
     - `mutPos = origin`; `sidewayLength = random.nextInt(2) + 1` (1 or 2); `mutPos.move(branchDir)`.
     - If `branchDir == clawDir`: `segmentDir = clawDir`; `inwayLength = random.nextInt(3) + 2`.
     - Else: `mutPos.move(UP)`; `segmentDir = random.nextInt(2) == 0 ? branchDir : UP`; `inwayLength = random.nextInt(3) + 3`.
     - Loop `i = 0 .. sidewayLength - 1`: if `!feature.place(level, chunkGenerator, random, mutPos)` break; `mutPos.move(segmentDir)`.
     - `mutPos.move(segmentDir.getOpposite())`; `mutPos.move(UP)`.
     - Loop `i = 0 .. inwayLength - 1`:
       - `mutPos.move(clawDir)`.
       - If `!feature.place(level, chunkGenerator, random, mutPos)` break.
       - If `random.nextFloat() < 0.25_f32`: `mutPos.move(UP)`.
  6. Return **`true`**.

##### 4.37 `minecraft:coral_tree` (`CoralTreeFeature`) — *refactored in 26.3*
- **Fields**: `feature`: `Holder<PlacedFeature>` (required).
- **Behavior**:
  1. `mutPos = origin`; `trunkHeight = random.nextInt(3) + 1` (in `[1, 3]`).
  2. Loop `i = 0 .. trunkHeight - 1`:
     - If `!feature.place(level, chunkGenerator, random, mutPos)`: return **`true`** *(quirk: returns `true` on trunk failure!)*.
     - `mutPos.move(UP)`.
  3. `trunkTop = mutPos.immutable()`.
  4. `nBranches = random.nextInt(3) + 2` (in `[2, 4]`).
  5. `dirs = Direction.Plane.HORIZONTAL.shuffledCopy(random)`.
  6. For each `branchDir` in `dirs[0 .. nBranches - 1]`:
     - `mutPos = trunkTop.relative(branchDir)`.
     - `branchHeight = random.nextInt(5) + 2`; `segLen = 0`.
     - Loop `j = 0 .. branchHeight - 1`:
       - If `!feature.place(level, chunkGenerator, random, mutPos)` break.
       - `segLen++`; `mutPos.move(UP)`.
       - If `j == 0 || (segLen >= 2 && random.nextFloat() < 0.25_f32)`: `mutPos.move(branchDir)`; `segLen = 0`.
  7. Return **`true`**.

##### 4.38 `minecraft:stepped_column_cluster` (`SteppedColumnClusterFeature`) — *new in 26.3, generalizes `basalt_columns`*
- **Fields**:
  - `block`: `Holder<BlockStateProvider>` (required)
  - `continue_through`: `BlockPredicate` (required)
  - `can_replace`: `BlockPredicate` (required)
  - `cannot_place_on`: `HolderSet<Block>` (required)
  - `cluster_reach`: `IntProvider` in `[0, 13]` (required)
  - `column_count`: `IntProvider` in `[1, 150]` (required)
  - `column_reach`: `IntProvider` in `[0, 3]` (required)
  - `height`: `IntProvider` in `[1, 10]` (required).
- **Helpers**:
  - `canPlaceAt(level, pos)`: `can_replace.test(level, pos)` AND `!state(pos.below()).isAir() && !state(pos.below()).is(cannot_place_on)`.
  - `findSurface(level, cursor, limit)`: while `cursor.y > level.minY + 1 && limit > 0`: decrement `limit`; if `canPlaceAt(level, cursor)` return `cursor`; move `cursor` DOWN 1. Else return `null`. *(Note: `lavaSeaLevel = chunkGenerator.getSeaLevel()` is fetched in `place` and passed as a dead parameter to `findSurface`.)*
  - `findAir(level, cursor, limit)`: while `cursor.y <= level.maxY && limit > 0`: decrement `limit`; if `state(cursor).is(cannot_place_on)` return `null`; if `state(cursor).isAir()` return `cursor`; move `cursor` UP 1. Else return `null`.
- **Behavior**:
  1. If `!canPlaceAt(level, origin)`: return **`false`**.
  2. Sample in exact order:
     - `columnHeight = height.sample(random)`
     - `cReach = min(columnHeight, cluster_reach.sample(random))`
     - `count = column_count.sample(random)`.
  3. Iterate `pos` lazily from `BlockPos.randomBetweenClosed(random, count, origin.x - cReach, origin.y, origin.z - cReach, origin.x + cReach, origin.y, origin.z + cReach)` (each iteration draws `x, y, z` via `Mth.randomBetweenInclusive`, interleaved with the loop body!):
     - `blocksToPlaceY = columnHeight - pos.distManhattan(origin)`.
     - If `blocksToPlaceY >= 0`:
       - `reach = column_reach.sample(random)`.
       - Iterate `colPos` in `BlockPos.betweenClosed(pos.x - reach, pos.y, pos.z - reach, pos.x + reach, pos.y, pos.z + reach)` (`z` outer, `x` inner):
         - `stepLimit = colPos.distManhattan(pos)` *(quirk: when `colPos == pos`, `stepLimit == 0`, so both `findSurface` and `findAir` return `null` immediately and the sub-column center is skipped!)*.
         - `start = can_replace.test(level, colPos) ? findSurface(level, colPos, stepLimit) : findAir(level, colPos, stepLimit)`.
         - If `start != null`:
           - `blocksY = blocksToPlaceY - stepLimit / 2`; `cursor = start`.
           - While `blocksY >= 0`:
             - If `can_replace.test(level, cursor)`: `setBlock(level, cursor, block.getState(level, random, cursor))` (`flags = 3`), `cursor.move(UP)`, `placed = true`.
             - Else if `continue_through.test(level, cursor)`: `cursor.move(UP)`.
             - Else: break.
             - `blocksY--`.
  4. Return `placed`.

##### 4.39 `minecraft:delta_feature` (`DeltaFeature`)
- **Fields**: `contents`: `BlockState`, `rim`: `BlockState`, `size`: `IntProvider` in `[0, 16]`, `rim_size`: `IntProvider` in `[0, 16]` (all required).
- **Hardcoded `CANNOT_REPLACE`** (7 blocks): `bedrock`, `nether_bricks`, `nether_brick_fence`, `nether_brick_stairs`, `nether_wart`, `chest`, `spawner`.
- **Helper `isClear(level, pos)`**: `s = state(pos)`; false if `s.is(contents.block)` or `s.block` in `CANNOT_REPLACE`; then for each `d` in `Direction.values()`: `air = state(pos.relative(d)).isAir()`; false if `(d != UP && air) || (d == UP && !air)`. Else `true`.
- **Behavior**:
  1. `spawnRim = random.nextDouble() < 0.9`.
  2. `rimX = spawnRim ? rim_size.sample(random) : 0`; `rimZ = spawnRim ? rim_size.sample(random) : 0`.
  3. `hasRim = spawnRim && rimX != 0 && rimZ != 0`.
  4. `radiusX = size.sample(random)`, `radiusZ = size.sample(random)`, `radiusLimit = max(radiusX, radiusZ)`.
  5. Iterate `pos` in `BlockPos.withinBoxByManhattanDistance(origin, radiusX, 0, radiusZ)`:
     - If `pos.distManhattan(origin) > radiusLimit`: break.
     - If `isClear(level, pos)`:
       - If `hasRim`: `anyPlaced = true`; `setBlock(level, pos, rim)` (`flags = 3`).
       - `posOffset = pos.offset(rimX, 0, rimZ)`.
       - If `isClear(level, posOffset)`: `anyPlaced = true`; `setBlock(level, posOffset, contents)` (`flags = 3`).
  6. Return `anyPlaced`.

##### 4.40 `minecraft:underwater_magma` (`UnderwaterMagmaFeature`)
- **Fields**: `floor_search_range`: `i32` in `[0, 512]`, `placement_radius_around_floor`: `i32` in `[0, 64]`, `placement_probability_per_valid_position`: `f32` in `[0.0, 1.0]` (all required).
- **Behavior**:
  1. Scan `Column.scan(level, origin, floor_search_range, s -> s.is(water), s -> !s.is(water))`. Extract `floorY = col.getFloor()`. If empty, return **`false`**.
  2. `floorPos = origin.atY(floorY)`; `r = placement_radius_around_floor`.
  3. Iterate `pos` in `BlockPos.betweenClosed(floorPos.offset(-r, -r, -r), floorPos.offset(r, r, r))` (`z` outer, `y` mid, `x` inner):
     - **Filter 1 (RNG FIRST)**: if `!(random.nextFloat() < placement_probability_per_valid_position)` skip *(RNG is drawn for EVERY cell in the cube BEFORE checking validity!)*.
     - **Filter 2 (`isValidPlacement`)**: `state(pos)` is neither `water` nor air, AND `!isVisibleFromOutside(pos.below(), UP)`, AND for all 4 horizontal `dir` (`NORTH, EAST, SOUTH, WEST`), `!isVisibleFromOutside(pos.relative(dir), dir.getOpposite())`, where `isVisibleFromOutside(p, face)` is true iff `state(p).getFaceOcclusionShape(face)` is empty or not a full 1×1 face.
     - If both pass: `level.setBlock(pos, minecraft:magma_block, 2)` and increment `placed`.
  4. Return `placed > 0`.

##### 4.41 `minecraft:sculk_patch` (`SculkPatchFeature`)
- **Fields** *(26.3 schema — pre-26.3 `catalyst_chance` and `extra_rare_growths` are removed)*:
  - `charge_count`: `i32` in `[1, 32]`
  - `amount_per_charge`: `i32` in `[1, 500]`
  - `spread_attempts`: `i32` in `[1, 64]`
  - `growth_rounds`: `i32` in `[0, 8]`
  - `spread_rounds`: `i32` in `[0, 8]`.
- **Precondition `canSpreadFrom(level, origin)`**: true iff `state(origin)`'s block implements `SculkBehaviour` (`minecraft:sculk` or `minecraft:sculk_vein`), OR (`state(origin).isAir() || (state(origin).is(water) && fluidState.isSource())`) AND at least one of the 6 `Direction.values()` neighbors has `isCollisionShapeFullBlock(level, neighborPos)`. If `false`, return **`false`** (no RNG).
- **Worldgen `SculkSpreader` parameters**:
  - `isWorldGeneration = true`, `replaceableBlocks = #minecraft:sculk_replaceable_world_gen`, `growthSpawnCost = 50`, `noGrowthRadius = 1`, `chargeDecayRate = 5`, `additionalDecayRate = 10`, `MAX_CURSORS = 32`, `MAX_CHARGE = 1000`, `MAX_WORLDGEN_SPREAD = 12` (`dx^2 + dz^2 <= 144`), `MAX_CURSOR_DISTANCE = 1024`.
- **Round loop**:
  1. `totalRounds = spread_rounds + growth_rounds`.
  2. For `round = 0 .. totalRounds - 1`:
     - Repeat `charge_count` times: `addCursors(origin, amount_per_charge)` (splits charge into chunks of `<= 1000`, appends each `ChargeCursor(pos=origin, charge, decayDelay=1, updateDelay=0, facings=null)` while `cursors.size() < 32`).
     - `spreadVeins = (round < spread_rounds)`.
     - Repeat `spread_attempts` times: `spreader.updateCursors(level, origin, random, spreadVeins)`.
     - `spreader.clear()`.
  3. Return **`true`**.
- **Exact `updateCursors` and `ChargeCursor.update` step (worldgen mode)**:
  - For each `cursor` in `cursors` in list order:
    1. Skip if `cursor.pos.distChessboard(origin) > 1024`.
    2. If `cursor.charge <= 0`: skip.
    3. If `cursor.updateDelay > 0`: decrement `cursor.updateDelay` and keep cursor in `processedCursors`.
    4. Else (`updateDelay == 0`):
       - Read `curState = level.getBlockState(cursor.pos)`; `beh = blockBehaviour(curState)` (`SculkBlock`, `SculkVeinBlock`, or `SculkBehaviour.DEFAULT`; all other blocks including `minecraft:sculk_catalyst` use `DEFAULT`).
       - **Vein spread**: if `spreadVeins`: call `beh.attemptSpreadVein(level, cursor.pos, curState, cursor.facings, postProcess = true)`:
         - `SculkBehaviour.DEFAULT`: if `facings == null`, run `sculk_vein.sameSpaceSpreader.spreadAll(level.getBlockState(pos), level, pos, true) > 0`; else if `!facings.isEmpty()`, if `curState.isAir() || curState.getFluidState().is(Fluids.WATER)` call `SculkVeinBlock.regrow(level, pos, curState, facings)` (places `sculk_vein` with attachable faces in `facings`, `WATERLOGGED` if `!curState.getFluidState().isEmpty()`, `flags = 3`); else call `veinSpreader.spreadAll(...) > 0`.
         - `SculkBlock` / `SculkVeinBlock` (default interface method): `sculk_vein.veinSpreader.spreadAll(curState, level, pos, true) > 0`.
         - If spread succeeded and `beh.canChangeBlockStateOnSpread()` (`true` for `DEFAULT` and `SculkVeinBlock`, `false` for `SculkBlock`): re-read `curState = level.getBlockState(cursor.pos)` and `beh = blockBehaviour(curState)`.
       - **Use charge**: `cursor.charge = beh.attemptUseCharge(cursor, level, origin, random, spreader, spreadVeins)`:
         - **`SculkBehaviour.DEFAULT`**: `cursor.decayDelay > 0 ? cursor.charge : 0`.
         - **`SculkVeinBlock`**:
           - If `spreadVeins && attemptPlaceSculk`: iterate `support` in `Direction.allShuffled(random)` (`Direction.values()` Fisher-Yates shuffled); on first `support` where `hasFace(state, support)` AND `state(pos.relative(support)).is(#minecraft:sculk_replaceable_world_gen)`:
             - `level.setBlockAndUpdate(supportPos, minecraft:sculk)` (`flags = 3`).
             - `veinSpreader.spreadAll(sculk, level, supportPos, true)`.
             - For each `dir` in `Direction.values()` except `support.getOpposite()`: if `state(supportPos.relative(dir)).is(sculk_vein)`, call `SculkVeinBlock.onDischarged` on that neighbor (clears faces touching `sculk`; if no faces remain replaces with `air` or `water` per `getFluidState().isEmpty()`, `flags = 3`).
             - Return `cursor.charge - 1`.
           - Else: `random.nextInt(5) == 0 ? Mth.floor(f32(cursor.charge) * 0.5_f32) : cursor.charge`.
         - **`SculkBlock`**:
           - If `charge != 0 && random.nextInt(5) == 0`:
             - `close = cursor.pos.closerThan(origin, 1.0)` (Euclidean dist `< 1.0`, i.e. `cursor.pos == origin`).
             - If `!close && canPlaceGrowth(level, cursor.pos)` (`above` is air or still `water`, AND at most 2 blocks in `[pos + (-4, 0, -4) .. pos + (4, 2, 4)]` match `#minecraft:sculk_growth_inhibitors`):
               - If `random.nextInt(50) < charge`:
                 - Choose growth state: `random.nextInt(11) == 0 ? sculk_shrieker[can_summon=true] : sculk_sensor`; if `!level.getFluidState(pos.above()).isEmpty()`, set `WATERLOGGED = true`; `level.setBlockAndUpdate(pos.above(), growthState)` (`flags = 3`).
               - Return `max(0, charge - 50)`.
             - Else: if `random.nextInt(10) != 0` return `charge`; else if `close` return `charge - 1`; else `outerDistSq = square(f32(sqrt(pos.distSqr(origin))) - 1.0_f32)`, `factor = min(1.0_f32, outerDistSq / 529.0_f32)` (`23^2 = 529`), return `charge - max(1, (i32)(f32(charge) * factor * 0.5_f32))`.
           - Else return `charge`.
       - **Discharge / movement**:
         - If `cursor.charge <= 0`: call `beh.onDischarged(level, curState, cursor.pos, random)`.
         - Else find `transferPos = getValidMovementPos(level, cursor.pos, random, origin, spreader)`:
           - Shuffle the 18 non-corner 3×3×3 neighbor offsets (constructed in `BlockPos.betweenClosedStream((-1,-1,-1), (1,1,1))` order filtered to `(x==0 || y==0 || z==0) && !(x==0 && y==0 && z==0)`) via `Util.shuffledCopy(NON_CORNER_NEIGHBOURS, random)`.
           - Iterate shuffled offsets: for `neighbour = cursor.pos + offset`:
             - Require `(origin.x - neighbour.x)^2 + (origin.z - neighbour.z)^2 <= 144`.
             - Require `state(neighbour)`'s block implements `SculkBehaviour` AND `isMovementUnobstructed(level, cursor.pos, neighbour)` (always true for Manhattan dist 1; for Manhattan dist 2 with two non-zero axes `A, B`, true iff at least one of the two intermediate orthogonal neighbors fails `isFaceSturdy(level, testPos, dir.getOpposite())`).
             - Record `sculkPosition = neighbour`. If `SculkVeinBlock.hasSubstrateAccess(level, state(neighbour), neighbour)` (true iff `state.is(sculk_vein)` and any attached face borders a block in **`#minecraft:sculk_replaceable`** — *quirk: uses `#minecraft:sculk_replaceable`, NOT `#minecraft:sculk_replaceable_world_gen`!*): break immediately.
           - If `transferPos != null`: call `beh.onDischarged`, move `cursor.pos = transferPos`, re-read `curState`.
           - Else (worldgen mode): call `beh.onDischarged`, set `cursor.charge = 0`, return.
         - If `curState` implements `SculkBehaviour`: `cursor.facings = MultifaceBlock.availableFaces(curState)`.
         - `cursor.decayDelay = beh.updateDecayDelay(cursor.decayDelay)` (`max(d - 1, 0)` for `DEFAULT`, `1` for `SculkBlock`/`SculkVeinBlock`).
         - `cursor.updateDelay = 1`.
    5. **Cursor retention**: if `cursor.charge > 0`, append to `processedCursors` (in worldgen mode, cursors at the same position are **never merged**!).
  - Replace `this.cursors = processedCursors`.
- **MultifaceSpreader details for `SculkVeinBlock`**:
  - `spreadAll(state, level, pos, postProcess)` iterates `face` in `Direction.values()` where `isOtherBlockValidAsSource(state) || hasFace(state, face)` (for `SculkVeinSpreaderConfig`, `isOtherBlockValidAsSource(s) = !s.is(sculk_vein)`), and for each `face` iterates `spreadDir` in `Direction.values()` with `spreadDir.axis != face.axis`.
  - For each `(face, spreadDir)`, tries `SpreadType`s in order (`[SAME_POSITION, SAME_PLANE, WRAP_AROUND]` for `veinSpreader`; `[SAME_POSITION]` for `sameSpaceSpreader`):
    - `SAME_POSITION`: `(pos, spreadDir)`
    - `SAME_PLANE`: `(pos.relative(spreadDir), face)`
    - `WRAP_AROUND`: `(pos.relative(spreadDir).relative(face), spreadDir.getOpposite())`
  - First type passing `canSpreadInto` (for `SculkVeinSpreaderConfig`: `againstState = state(placementPos.relative(placementDir))` is NOT `sculk`, `sculk_catalyst`, or `moving_piston`; if `sourcePos.distManhattan(placementPos) == 2`, `sourcePos.relative(placementDir.getOpposite())` is not face-sturdy on `placementDir`; `existingState` fluid is empty or `Fluids.WATER`; not `#minecraft:fire`; `existingState.canBeReplaced() || isAir || is(sculk_vein) || (is(water) && isSource)`; AND `sculk_vein.isValidStateForPlacement`) places the new `sculk_vein` state (`flags = 2`, marking chunk pos for post-processing if `postProcess`) and stops trying further `SpreadType`s for that `(face, spreadDir)`.


---

#### Group F: Templates, fossils, bonus chest, and monster rooms (§4.42–§4.45)

*(Note: NBT structure template loading, block/entity placement, transform math, and `StructureProcessor` pipelines are specified in the structure template specs; below we specify the exact feature-side selection, bounds, origin math, processor list wiring, RNG order, and block-update flags.)*

##### 4.42 `minecraft:fossil` (`FossilFeature`)
- **Fields**:
  - `fossil_structures`: non-empty `List<Identifier>` (required)
  - `overlay_structures`: non-empty `List<Identifier>` (required; codec validation requires `fossil_structures.size() == overlay_structures.size()`)
  - `fossil_processors`: `Holder<StructureProcessorList>` (required)
  - `overlay_processors`: `Holder<StructureProcessorList>` (required)
  - `max_empty_corners_allowed`: `i32` in `[0, 7]` (required).
- **Behavior**:
  1. Draw in exact order:
     - `rotation = Rotation.getRandom(random)` (`[NONE, CLOCKWISE_90, CLOCKWISE_180, COUNTERCLOCKWISE_90]`, 1 `nextInt(4)`).
     - `fossilIndex = random.nextInt(fossil_structures.size())`.
  2. Look up `fossilBase = templates[fossil_structures[fossilIndex]]` and `fossilOverlay = templates[overlay_structures[fossilIndex]]`.
  3. Compute the 3×3-chunk clipping `BoundingBox` around `chunkPos = ChunkPos.containing(origin)`:
     `[chunkPos.minBlockX - 16 ..= chunkPos.maxBlockX + 16] × [level.minY ..= level.maxY] × [chunkPos.minBlockZ - 16 ..= chunkPos.maxBlockZ + 16]`.
  4. Construct `StructurePlaceSettings` with `rotation`, `boundingBox`, and `random`.
  5. Let `size = fossilBase.getSize(rotation)` and `lowCorner = origin.offset(-size.x / 2, 0, -size.z / 2)` *(Java integer division toward zero)*.
  6. Scan `xscan = 0 .. size.x - 1` (outer), `zscan = 0 .. size.z - 1` (inner):
     - `lowestSurfaceY = min(lowestSurfaceY, level.getHeight(OCEAN_FLOOR_WG, lowCorner.x + xscan, lowCorner.z + zscan))` (initialized to `origin.y`).
  7. `targetY = max(lowestSurfaceY - 15 - random.nextInt(10), level.minY + 10)`.
  8. `targetPos = fossilBase.getZeroPositionWithTransform(lowCorner.atY(targetY), Mirror.NONE, rotation)`.
  9. **Empty corner check**: compute `bounds = fossilBase.getBoundingBox(settings, targetPos)` and count how many of its 8 corners (`(minX|maxX, minY|maxY, minZ|maxZ)`) have `state.isAir() || state.is(lava) || state.is(water)`. If `count > max_empty_corners_allowed`: return **`false`**.
  10. Place `fossilBase` at `(targetPos, targetPos)` with `fossil_processors` and **`flags = 260`** (`UPDATE_KNOWN_SHAPE | UPDATE_INVISIBLE`? Specifically bit 2 `UPDATE_INVISIBLE=4` + bit 8 `UPDATE_SKIP_ALL_SIDEEFFECTS=256`, no neighbor updates).
  11. Replace processors on `settings` with `overlay_processors` and place `fossilOverlay` at `(targetPos, targetPos)` with **`flags = 260`**.
  12. Return **`true`**.

##### 4.43 `minecraft:template` (`TemplateFeature`) — *new in 26.3, replaces e.g. `desert_well` and `coral_mushroom`*
- **Fields**:
  - `templates`: `WeightedList<TemplateEntry>` (required), where each `TemplateEntry` has:
    - `id`: `Identifier` (required — structure template ID)
    - `rotations`: `List<Rotation>` (optional, default all 4 rotations `[none, clockwise_90, clockwise_180, counterclockwise_90]`)
  - `processors`: `Optional<Holder<StructureProcessorList>>` (optional).
- **Behavior**:
  1. Select `entry = templates.getRandomOrThrow(random)` (`WeightedList` selection).
  2. Select `rotation = entry.rotations[random.nextInt(entry.rotations.size())]`.
  3. Look up `template = templates[entry.id]`.
  4. Center XZ around `origin` accounting for rotation:
     - `offsetX = rotation.rotate(Direction.WEST).unitVec3i * (template.size.x / 2)`
     - `offsetZ = rotation.rotate(Direction.NORTH).unitVec3i * (template.size.z / 2)`
     - `pos = origin + offsetX + offsetZ`.
  5. Construct `StructurePlaceSettings` with `rotation`, `random`, and (if present) all processors in `processors` in list order. (No explicit `boundingBox` clip beyond `WorldGenRegion` write bounds.)
  6. Call `template.placeInWorld(level, pos, pos, settings, random, flags = 3)` and return its boolean result.

##### 4.44 `minecraft:bonus_chest` (`BonusChestFeature`)
- **Fields**: none.
- **Behavior**:
  1. Let `chunkPos = ChunkPos.containing(origin)`.
  2. Shuffle `[chunkPos.minBlockX ..= chunkPos.maxBlockX]` via `Util.toShuffledList(..., random)` into `xPoses`.
  3. Shuffle `[chunkPos.minBlockZ ..= chunkPos.maxBlockZ]` via `Util.toShuffledList(..., random)` into `zPoses`.
  4. Loop `x` in `xPoses` (outer), `z` in `zPoses` (inner):
     - `chestPos = level.getHeightmapPos(MOTION_BLOCKING_NO_LEAVES, (x, 0, z))`.
     - If `level.isEmptyBlock(chestPos) || level.getBlockState(chestPos).getCollisionShape(level, chestPos).isEmpty()`:
       - `level.setBlock(chestPos, minecraft:chest, 2)`.
       - `RandomizableContainer.setBlockEntityLootTable(level, random, chestPos, minecraft:chests/spawn_bonus_chest)` — sets the chest block entity's `LootTable` and `LootTableSeed = random.nextLong()` (2 Xoroshiro steps via `WorldgenRandom`!).
       - For each `dir` in `Direction.Plane.HORIZONTAL` (`NORTH, EAST, SOUTH, WEST`): if `minecraft:torch.canSurvive(level, chestPos.relative(dir))`, `level.setBlock(chestPos.relative(dir), minecraft:torch, 2)`.
       - Return **`true`**.
  5. If no position matched, return **`false`**.

##### 4.45 `minecraft:monster_room` (`MonsterRoomFeature`)
- **Fields**: none.
- **Constants**:
  - `MOBS = [minecraft:skeleton, minecraft:zombie, minecraft:zombie, minecraft:spider]` (50% zombie, 25% skeleton, 25% spider).
  - `AIR = minecraft:cave_air`.
  - `canReplace(s) = !s.is(#minecraft:features_cannot_replace)`.
- **Behavior**:
  1. Draw room half-extents: `xr = random.nextInt(2) + 2` (2 or 3), `zr = random.nextInt(2) + 2` (2 or 3).
     Let `minX = -xr - 1`, `maxX = xr + 1`, `minZ = -zr - 1`, `maxZ = zr + 1`.
  2. **Validation scan**: `holeCount = 0`. Loop `dx = minX ..= maxX`, `dy = -1 ..= 4`, `dz = minZ ..= maxZ`:
     - `p = origin.offset(dx, dy, dz)`; `solid = level.getBlockState(p).isSolid()`.
     - If `dy == -1 && !solid`: return **`false`**.
     - If `dy == 4 && !solid`: return **`false`**.
     - If `(dx == minX || dx == maxX || dz == minZ || dz == maxZ) && dy == 0 && level.isEmptyBlock(p) && level.isEmptyBlock(p.above())`: `holeCount++`.
  3. Require `1 <= holeCount <= 5`; otherwise return **`false`**.
  4. **Carve room & place cobble**: loop `dx = minX ..= maxX`, `dy = 3` down to `-1` *(note `dy = 4` is not modified!)*, `dz = minZ ..= maxZ`:
     - `p = origin.offset(dx, dy, dz)`; `s = level.getBlockState(p)`.
     - If on boundary (`dx == minX || dx == maxX || dz == minZ || dz == maxZ || dy == -1 || dy == 4`):
       - If `p.y >= level.minY && !level.getBlockState(p.below()).isSolid()`: `level.setBlock(p, minecraft:cave_air, 2)` *(bypasses `safeSetBlock`!)*.
       - Else if `s.isSolid() && !s.is(minecraft:chest)`:
         - If `dy == -1 && random.nextInt(4) != 0`: `safeSetBlock(level, p, minecraft:mossy_cobblestone, canReplace)` (`flags = 3`).
         - Else: `safeSetBlock(level, p, minecraft:cobblestone, canReplace)` (`flags = 3`).
     - Else (interior): if `!s.is(minecraft:chest) && !s.is(minecraft:spawner)`: `safeSetBlock(level, p, minecraft:cave_air, canReplace)`.
  5. **Chests**: repeat `cc = 0 .. 1` (2 chests):
     - Repeat `i = 0 .. 2` (up to 3 attempts):
       - `xc = origin.x + random.nextInt(xr * 2 + 1) - xr`, `yc = origin.y`, `zc = origin.z + random.nextInt(zr * 2 + 1) - zr`.
       - If `level.isEmptyBlock((xc, yc, zc))`:
         - Count horizontal neighbors (`NORTH, EAST, SOUTH, WEST`) where `isSolid()`.
         - If `wallCount == 1`:
           - Orient chest via `StructurePiece.reorient(level, chestPos, minecraft:chest[facing=north])`:
             1. Scan `Direction.Plane.HORIZONTAL` (`NORTH, EAST, SOUTH, WEST`): if any neighbor is `minecraft:chest`, return unchanged; track `solidNeighbor` for `isSolidRender()` neighbors (if 2+ have `isSolidRender()`, clear `solidNeighbor = null` and break).
             2. If unique `solidNeighbor != null`, face `solidNeighbor.getOpposite()`.
             3. Else start from `lockDir = state.FACING` (`NORTH`): if `isSolidRender()` in `lockDir`, flip to `Opposite`; if still `isSolidRender()`, turn `ClockWise`; if still `isSolidRender()`, flip to `Opposite`.
           - `safeSetBlock(level, chestPos, orientedChest, canReplace)`.
           - `RandomizableContainer.setBlockEntityLootTable(level, random, chestPos, minecraft:chests/simple_dungeon)` (draws `random.nextLong()` if chest block entity exists).
           - Break inner attempt loop.
  6. **Spawner**: `safeSetBlock(level, origin, minecraft:spawner, canReplace)`. If `level.getBlockEntity(origin)` is a `SpawnerBlockEntity`, draw `mob = MOBS[random.nextInt(4)]` and set its `SpawnData.entity.id` to `mob` (no further RNG consumed by `setEntityId` because `spawnPotentials` is empty).
  7. Return **`true`**.

---

#### Group G: End features (§4.46–§4.50)

##### 4.46 `minecraft:end_gateway` (`EndGatewayFeature`)
- **Fields**: `exit`: `Optional<BlockPos>` (optional), `exact`: `bool` (required).
- **Behavior** (no RNG):
  1. Iterate `pos` in `BlockPos.betweenClosed(origin.offset(-1, -2, -1), origin.offset(1, 2, 1))`:
     - Let `sameX = (pos.x == origin.x)`, `sameY = (pos.y == origin.y)`, `sameZ = (pos.z == origin.z)`, `end = (|pos.y - origin.y| == 2)`.
     - If `sameX && sameY && sameZ`: `setBlock(level, pos, minecraft:end_gateway)` (`flags = 3`); if `exit` is present and block entity is `TheEndGatewayBlockEntity`, set `exitPosition = exit` and `exactTeleport = exact`.
     - Else if `sameY`: `setBlock(level, pos, minecraft:air)`.
     - Else if `end && sameX && sameZ`: `setBlock(level, pos, minecraft:bedrock)`.
     - Else if `(sameX || sameZ) && !end`: `setBlock(level, pos, minecraft:bedrock)`.
     - Else: `setBlock(level, pos, minecraft:air)`.
  2. Return **`true`**.

##### 4.47 `minecraft:end_island` (`EndIslandFeature`)
- **Fields**: none.
- **Behavior**:
  1. `size = f32(random.nextInt(3)) + 4.0_f32` (`4.0`, `5.0`, or `6.0`).
  2. Loop `y = 0, -1, -2, ...` while `size > 0.5_f32`:
     - Loop `x = Mth.floor(-size) ..= Mth.ceil(size)` (outer), `z = Mth.floor(-size) ..= Mth.ceil(size)` (inner):
       - If `f32(x * x + z * z) <= (size + 1.0_f32) * (size + 1.0_f32)`: `setBlock(level, origin.offset(x, y, z), minecraft:end_stone)` (`flags = 3`).
     - `size -= f32(random.nextInt(2)) + 0.5_f32`.
  3. Return **`true`**.

##### 4.48 `minecraft:end_platform` (`EndPlatformFeature`)
- **Fields**: none.
- **Behavior** (no RNG):
  1. Loop `dz = -2 ..= 2`, `dx = -2 ..= 2`, `dy = -1 ..= 2`:
     - `block = (dy == -1) ? minecraft:obsidian : minecraft:air`.
     - At `origin.offset(dx, dy, dz)`: if `!state.is(block)`, `level.setBlockAndUpdate(pos, block.defaultState)` (`flags = 3`; `dropResources = false` during feature placement).
  2. Return **`true`**.

##### 4.49 `minecraft:end_podium` (`EndPodiumFeature`)
- **Fields**: `active`: `bool` (optional, default `false`).
- **Behavior** (no RNG):
  1. Iterate `pos` in `BlockPos.betweenClosed(origin.offset(-4, -1, -4), origin.offset(4, 32, 4))`:
     - `insideRim = pos.closerThan(origin, 2.5)` (Euclidean distance `< 2.5`, i.e. `distSqr < 6.25`).
     - If `insideRim || pos.closerThan(origin, 3.5)` (`distSqr < 12.25`):
       - If `pos.y < origin.y`:
         - If `insideRim`: `setBlock(level, pos, minecraft:bedrock)` (`flags = 3`).
         - Else: place `minecraft:end_stone` (if `active`, only if `!state.is(end_stone)`, calling `destroyBlock` first then `setBlock`).
       - Else if `pos.y > origin.y`: place `minecraft:air` (if `active`, destroy first if not air).
       - Else (`pos.y == origin.y`):
         - If `!insideRim`: `setBlock(level, pos, minecraft:bedrock)`.
         - Else if `active`: destroy-and-set `minecraft:end_portal`.
         - Else: `setBlock(level, pos, minecraft:air)`.
  2. Central pillar: for `y = 0 .. 3`, `setBlock(level, origin.above(y), minecraft:bedrock)`.
  3. Torches: for each `face` in `Direction.Plane.HORIZONTAL`, `setBlock(level, origin.above(2).relative(face), minecraft:wall_torch[facing=<face>])`.
  4. Return **`true`**.

##### 4.50 `minecraft:end_spike` (`EndSpikeFeature`)
- **Fields**:
  - `spikes`: `List<EndSpike>` (required; may be empty to auto-generate the 10 world-seeded End pillars). Each `EndSpike` has optional fields `centerX` (`0`), `centerZ` (`0`), `radius` (`0`), `height` (`0`), `guarded` (`false`).
  - `crystal_invulnerable`: `bool` (optional, default `false`).
  - `crystal_beam_target`: `Optional<BlockPos>` (optional).
- **World-seeded 10-pillar layout (`getSpikesForLevel`)** (when `spikes` is empty):
  1. Seed `r1 = SingleThreadedRandomSource(level.getSeed())` (Legacy 48-bit LCG).
  2. `key = r1.nextLong() & 0xFFFF_i64` *(only 65,536 distinct pillar permutations exist across all world seeds!)*.
  3. Seed `r2 = SingleThreadedRandomSource(key)` and shuffle `[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]` via `Util.toShuffledList(..., r2)` into `sizes`.
  4. For `i = 0 .. 9`:
     - `angle = 2.0 * (-PI + (PI / 10.0) * f64(i))` (standard `Math.cos`/`Math.sin`, `f64`).
     - `x = Mth.floor(42.0 * Math.cos(angle))`, `z = Mth.floor(42.0 * Math.sin(angle))`.
     - `size = sizes[i]`, `radius = 2 + size / 3`, `height = 76 + size * 3`, `guarded = (size == 1 || size == 2)`.
- **Placement (`place`)**:
  1. Resolve `spikes` (from config if non-empty, else `getSpikesForLevel(level)`).
  2. For each `spike` in `spikes`: if `SectionPos.blockToSectionCoord(origin.x) == SectionPos.blockToSectionCoord(spike.centerX)` AND same for `Z`:
     1. **Obsidian cylinder + air clearance**: iterate `pos` in `BlockPos.betweenClosed((centerX - radius, level.minY, centerZ - radius), (centerX + radius, height + 10, centerZ + radius))`:
        - If `(pos.x - centerX)^2 + (pos.z - centerZ)^2 <= f64(radius * radius + 1) && pos.y < height` *(note `distToLowCornerSqr(centerX, pos.y, centerZ)` has `dy = 0`)*: `setBlock(level, pos, minecraft:obsidian)` (`flags = 3`).
        - Else if `pos.y > 65`: `setBlock(level, pos, minecraft:air)`.
     2. **Iron bars cage** (if `spike.guarded`): loop `dx = -2 ..= 2`, `dz = -2 ..= 2`, `dy = 0 ..= 3`:
        - If `|dx| == 2 || |dz| == 2 || dy == 3`:
          - `xEdge = (dx == -2 || dx == 2 || dy == 3)`, `zEdge = (dz == -2 || dz == 2 || dy == 3)`.
          - Place `minecraft:iron_bars[north = xEdge && dz != -2, south = xEdge && dz != 2, west = zEdge && dx != -2, east = zEdge && dx != 2]` at `(centerX + dx, height + dy, centerZ + dz)` (`flags = 3`).
     3. **End crystal, bedrock, and fire**:
        - Spawn `minecraft:end_crystal` at `(centerX + 0.5, height + 1, centerZ + 0.5)` with yaw `random.nextFloat() * 360.0_f32`, pitch `0.0`, `beamTarget = crystal_beam_target`, `invulnerable = crystal_invulnerable`.
        - At `crystalPos = (centerX, height + 1, centerZ)`:
          - `setBlock(level, crystalPos.below(), minecraft:bedrock)` (`flags = 3`).
          - `setBlock(level, crystalPos, BaseFireBlock.getState(level, crystalPos))` (on bedrock, `minecraft:fire` default state, `flags = 3`).
  3. Return **`true`**.

---

#### Group H: Lakes, huge mushrooms/fungi, vegetation patches, void platform, and multiface growth (§4.51–§4.58)

##### 4.51 `minecraft:lake` (`LakeFeature`, deprecated)
- **Fields** *(updated in 26.3 with 3 explicit `BlockPredicate`s)*:
  - `fluid`: `Holder<BlockStateProvider>` (required)
  - `barrier`: `Holder<BlockStateProvider>` (required)
  - `can_place_feature`: `BlockPredicate` (required)
  - `can_replace_with_air_or_fluid`: `BlockPredicate` (required)
  - `can_replace_with_barrier`: `BlockPredicate` (required).
- **Behavior**:
  1. If `origin.y <= level.minY + 4`: return **`false`**.
  2. Shift `origin = origin.offset(-8, -4, -8)`.
  3. Allocate `grid[2048]` (`bool`, indexed `(xx * 16 + zz) * 8 + yy` for `xx in 0..15, zz in 0..15, yy in 0..7`).
  4. `spots = random.nextInt(4) + 4` (in `[4, 7]`). For `i = 0 .. spots - 1`:
     - Draw 6 `nextDouble()` values in exact order:
       - `xr = nextDouble() * 6.0 + 3.0`, `yr = nextDouble() * 4.0 + 2.0`, `zr = nextDouble() * 6.0 + 3.0`
       - `xp = nextDouble() * (16.0 - xr - 2.0) + 1.0 + xr / 2.0`
       - `yp = nextDouble() * (8.0 - yr - 4.0) + 2.0 + yr / 2.0`
       - `zp = nextDouble() * (16.0 - zr - 2.0) + 1.0 + zr / 2.0`.
     - Mark `grid[(xx*16+zz)*8+yy] = true` for `xx in 1..14, zz in 1..14, yy in 1..6` where `((xx - xp)/(xr/2))^2 + ((yy - yp)/(yr/2))^2 + ((zz - zp)/(zr/2))^2 < 1.0`.
  5. Evaluate `fluidState = fluid.getState(level, random, origin)` *(once, at shifted `origin`, BEFORE boundary check!)*.
  6. **Boundary check**: define `isBoundary(xx, yy, zz)` as `!grid[idx]` AND at least one in-bounds 6-neighbor (`xx+1<16`, `xx>0`, `zz+1<16`, `zz>0`, `yy+1<8`, `yy>0`) has `grid == true`.
     Loop `xx = 0..15`, `zz = 0..15`, `yy = 0..7`: if `isBoundary`:
     - `offsetPos = origin.offset(xx, yy, zz)`; `s = level.getBlockState(offsetPos)`.
     - If `yy >= 4 && s.liquid()`: return **`false`**.
     - If `yy < 4 && !s.isSolid() && s != fluidState`: return **`false`**.
     - If `!can_place_feature.test(level, offsetPos)`: return **`false`**.
  7. **Carve & fill**: loop `xx = 0..15`, `zz = 0..15`, `yy = 0..7`: if `grid[idx]`:
     - `p = origin.offset(xx, yy, zz)`.
     - If `can_replace_with_air_or_fluid.test(level, p)`:
       - If `yy >= 4`: `level.setBlock(p, minecraft:cave_air, 2)`, `level.scheduleTick(p, minecraft:cave_air, 0)`, `markAboveForPostProcessing(level, p)`.
       - Else: `level.setBlock(p, fluidState, 2)`.
  8. **Barrier shell**: evaluate `barrierState = barrier.getState(level, random, origin)` *(once, at shifted `origin`)*. If `!barrierState.isAir()`:
     - Loop `xx = 0..15`, `zz = 0..15`, `yy = 0..7`:
       - If `isBoundary && (yy < 4 || random.nextInt(2) != 0)` *(RNG only drawn when `yy >= 4`!)*:
         - `p = origin.offset(xx, yy, zz)`.
         - If `level.getBlockState(p).isSolid() && can_replace_with_barrier.test(level, p)`: `level.setBlock(p, barrierState, 2)` and `markAboveForPostProcessing(level, p)`.
  9. **Surface freeze**: if `fluidState.getFluidState().is(#minecraft:water)`:
     - Loop `xx = 0..15`, `zz = 0..15`: at `p = origin.offset(xx, 4, zz)`:
       - If `level.getBiome(p).shouldFreeze(level, p, false) && can_replace_with_air_or_fluid.test(level, p)`: `level.setBlock(p, minecraft:ice, 2)`.
  10. Return **`true`**.

##### 4.52 `minecraft:huge_brown_mushroom` (`HugeBrownMushroomFeature`) & 4.53 `minecraft:huge_red_mushroom` (`HugeRedMushroomFeature`)
- **Shared fields** (`AbstractHugeMushroomFeature`, identical schema for both types):
  - `cap_provider`: `Holder<BlockStateProvider>` (required)
  - `stem_provider`: `Holder<BlockStateProvider>` (required)
  - `foliage_radius`: `i32` (optional, default `2`)
  - `can_place_on`: `BlockPredicate` (required — new in 26.3 replacing hardcoded dirt/mycelium check).
- **Shared behavior (`AbstractHugeMushroomFeature.place`)**:
  1. `treeHeight = random.nextInt(3) + 4`; if `random.nextInt(12) == 0`: `treeHeight *= 2`.
  2. **Validation (`isValidPosition`)**:
     - Require `origin.y >= level.minY + 1 && origin.y + treeHeight + 1 <= level.maxY`.
     - Require `can_place_on.test(level, origin.below())`.
     - Loop `dy = 0 ..= treeHeight`:
       - `r = getTreeRadiusForHeight(trunkHeight = -1, treeHeight = -1, foliage_radius, yo = dy)`:
         - **Brown (`4.52`)**: `dy <= 3 ? 0 : foliage_radius`.
         - **Red (`4.53`)**: `(dy < -1 && dy >= -4) || dy == -1 ? foliage_radius : 0` — **QUIRK**: because `isValidPosition` passes `treeHeight = -1` and `dy >= 0`, `HugeRedMushroomFeature.getTreeRadiusForHeight` **ALWAYS returns `0`** during validation, checking only the `1×1` center column!
       - Loop `dx = -r ..= r`, `dz = -r ..= r`: fail (`return false`) if `state(origin + (dx, dy, dz))` is neither air nor `#minecraft:leaves`.
  3. **`makeCap`** (runs BEFORE `placeTrunk`):
     - Helper `placeMushroomBlock(level, pos, state)`: only places (`setBlock`, `flags = 3`) if `cur.isAir() || cur.is(#minecraft:replaceable_by_mushrooms)`.
     - **Important quirk**: BOTH `cap_provider.getState(level, random, origin)` and `stem_provider.getState(level, random, origin)` are called with **`origin`** (NOT `blockPos`)!
     - **4.52 Brown cap** (single flat layer at `dy = treeHeight`):
       - Loop `dx = -foliage_radius ..= foliage_radius`, `dz = -foliage_radius ..= foliage_radius`:
         - Skip the 4 corners (`|dx| == foliage_radius && |dz| == foliage_radius`).
         - Sample `state = cap_provider.getState(level, random, origin)`.
         - If `state` has `WEST, EAST, NORTH, SOUTH`, set:
           - `WEST = (dx == -r) || (|dz| == r && dx == 1 - r)`
           - `EAST = (dx == r) || (|dz| == r && dx == r - 1)`
           - `NORTH = (dz == -r) || (|dx| == r && dz == 1 - r)`
           - `SOUTH = (dz == r) || (|dx| == r && dz == r - 1)`
           (leaves `UP` and `DOWN` at provider's values).
         - `placeMushroomBlock(level, origin + (dx, treeHeight, dz), state)`.
     - **4.53 Red cap** (4 layers `dy = treeHeight - 3 ..= treeHeight`):
       - Let `r = (dy < treeHeight) ? foliage_radius : foliage_radius - 1`; `center = foliage_radius - 2`.
       - Loop `dx = -r ..= r`, `dz = -r ..= r`:
         - Include iff `dy >= treeHeight || ((|dx| == r) != (|dz| == r))` (top layer is solid square; lower 3 layers are hollow perimeter minus corners).
         - Sample `state = cap_provider.getState(level, random, origin)`.
         - If `state` has `WEST, EAST, NORTH, SOUTH, UP`, set: `UP = (dy >= treeHeight - 1)`, `WEST = (dx < -center)`, `EAST = (dx > center)`, `NORTH = (dz < -center)`, `SOUTH = (dz > center)`.
         - `placeMushroomBlock(level, origin + (dx, dy, dz), state)`.
  4. **`placeTrunk`**: loop `dy = 0 .. treeHeight - 1`: `placeMushroomBlock(level, origin.above(dy), stem_provider.getState(level, random, origin))`.
  5. Return **`true`**.

##### 4.54 `minecraft:huge_fungus` (`HugeFungusFeature`)
- **Fields**:
  - `valid_base_block`: `BlockState` (only its `Block` is compared)
  - `stem_state`, `hat_state`, `decor_state`: `BlockState`
  - `replaceable_blocks`: `BlockPredicate`
  - `planted`: `bool` (optional, default `false`).
- **Helper `isReplaceable(level, pos, checkPlants)`**: `state(pos).canBeReplaced() || (checkPlants && replaceable_blocks.test(level, pos))`.
- **Behavior**:
  1. If `!level.getBlockState(origin.below()).is(valid_base_block.block)`: return **`false`**.
  2. `totalHeight = Mth.nextInt(random, 4, 13)` (`random.nextInt(10) + 4`). If `random.nextInt(12) == 0`: `totalHeight *= 2`.
  3. If `!planted && origin.y + totalHeight + 1 >= chunkGenerator.getGenDepth()`: return **`false`**.
  4. `isHuge = !planted && random.nextFloat() < 0.06_f32`.
  5. Clear origin: `level.setBlock(origin, minecraft:air, 260)`.
  6. **`placeStem`**: `stemRadius = isHuge ? 1 : 0`. Loop `dx = -stemRadius ..= stemRadius`, `dz = -stemRadius ..= stemRadius`, `dy = 0 .. totalHeight - 1`:
     - `corner = isHuge && |dx| == stemRadius && |dz| == stemRadius`.
     - If `isReplaceable(level, pos, true)`:
       - If `planted`: if `!state(pos.below()).isAir()` call `destroyBlock(pos, true)`; `level.setBlockAndUpdate(pos, stem_state)`.
       - Else if `corner`: if `random.nextFloat() < 0.1_f32`: `setBlock(level, pos, stem_state)` (`flags = 3`).
       - Else: `setBlock(level, pos, stem_state)` (`flags = 3`).
  7. **`placeHat`**:
     - `placeVines = hat_state.is(minecraft:nether_wart_block)`.
     - `hatHeight = min(random.nextInt(1 + totalHeight / 3) + 5, totalHeight)`; `hatStartY = totalHeight - hatHeight`.
     - Loop `dy = hatStartY ..= totalHeight`:
       - `radius = (dy < totalHeight - random.nextInt(3)) ? 2 : 1`.
       - If `hatHeight > 8 && dy < hatStartY + 4`: `radius = 3`.
       - If `isHuge`: `radius++`.
       - Loop `dx = -radius ..= radius`, `dz = -radius ..= radius`:
         - `isEdgeX = (|dx| == radius)`, `isEdgeZ = (|dz| == radius)`, `inside = !isEdgeX && !isEdgeZ && dy != totalHeight`, `corner = isEdgeX && isEdgeZ`, `isHatBottom = dy < hatStartY + 3`.
         - If `isReplaceable(level, pos, false)`:
           - If `planted && !state(pos.below()).isAir()`: `destroyBlock(pos, true)`.
           - If `isHatBottom`: if `!inside`: **`placeHatDropBlock`**:
             - If `state(pos.below()).is(hat_state.block)`: `setBlock(level, pos, hat_state)`.
             - Else if `f64(random.nextFloat()) < 0.15`: `setBlock(level, pos, hat_state)`; if `placeVines && random.nextInt(11) == 0`: `tryPlaceWeepingVines(pos)` *(short-circuits when `!placeVines`)*.
           - Else: **`placeHatBlock(decorProb, hatProb, vinesProb)`**:
             - `inside`: `(0.1F, 0.2F, placeVines ? 0.1F : 0.0F)`
             - `corner`: `(0.01F, 0.7F, placeVines ? 0.083F : 0.0F)`
             - other (edge/top): `(5.0e-4F, 0.98F, placeVines ? 0.07F : 0.0F)`
             - Draw `random.nextFloat() < decorProb` -> place `decor_state`; else `random.nextFloat() < hatProb` -> place `hat_state` and draw **`random.nextFloat() < vinesProb`** *(quirk: ALWAYS draws this `nextFloat()` even when `vinesProb == 0.0F`!)* -> if true, `tryPlaceWeepingVines(pos)`.
     - **`tryPlaceWeepingVines(hatPos)`**: let `p = hatPos.below()`. If `level.isEmptyBlock(p)`:
       - `goalH = Mth.nextInt(random, 1, 5)` (`random.nextInt(5) + 1`); if `random.nextInt(7) == 0`: `goalH *= 2`.
       - Loop `h = 0 ..= goalH`: if `level.isEmptyBlock(p)`: if `h == goalH || !level.isEmptyBlock(p.below())` place `minecraft:weeping_vines[age = Mth.nextInt(random, 23, 25)]` (`flags = 2`) and break; else place `minecraft:weeping_vines_plant` (`flags = 2`). Move `p` DOWN 1.
  8. Return **`true`**.

##### 4.55 `minecraft:vegetation_patch` (`VegetationPatchFeature`)
- **Fields**:
  - `replaceable`: `HolderSet<Block>`
  - `ground_state`: `Holder<BlockStateProvider>`
  - `vegetation_feature`: `Holder<PlacedFeature>`
  - `surface`: `CaveSurface` (`"floor"` -> inwards `DOWN`, or `"ceiling"` -> inwards `UP`)
  - `depth`: `IntProvider` in `[1, 128]`
  - `extra_bottom_block_chance`: `f32` in `[0.0, 1.0]`
  - `vertical_range`: `i32` in `[1, 256]`
  - `vegetation_chance`: `f32` in `[0.0, 1.0]`
  - `xz_radius`: `IntProvider` (unbounded)
  - `extra_edge_column_chance`: `f32` in `[0.0, 1.0]`.
- **Behavior**:
  1. `xRadius = xz_radius.sample(random) + 1`, `zRadius = xz_radius.sample(random) + 1`.
  2. **`placeGroundPatch`**: let `inwards = surface.direction`, `outwards = inwards.opposite`. Allocate **`HashSet<BlockPos> surface`**.
     Loop `dx = -xRadius ..= xRadius` (outer), `dz = -zRadius ..= zRadius` (inner):
     - Skip the 4 corners (`|dx| == xRadius && |dz| == zRadius`).
     - If on a non-corner edge (`|dx| == xRadius || |dz| == zRadius`): skip if `extra_edge_column_chance == 0.0F` OR `random.nextFloat() > extra_edge_column_chance` *(short-circuits RNG when `== 0.0F`)*.
     - Start `pos = origin.offset(dx, 0, dz)`:
       - Move `inwards` up to `vertical_range` times while `state(pos).isAir()`.
       - Move `outwards` up to `vertical_range` times while `!state(pos).isAir()`.
     - Let `belowPos = pos.relative(inwards)`.
     - If `level.isEmptyBlock(pos) && state(belowPos).isFaceSturdy(level, belowPos, outwards)`:
       - `d = depth.sample(random) + (extra_bottom_block_chance > 0.0F && random.nextFloat() < extra_bottom_block_chance ? 1 : 0)`.
       - `groundPos = belowPos.immutable()`.
       - **`placeGround`**: loop `i = 0 .. d - 1`:
         - `stateToPlace = ground_state.getState(level, random, belowPos)`; `cur = level.getBlockState(belowPos)`.
         - If `!stateToPlace.is(cur.block)`:
           - If `!cur.is(replaceable)`: stop with `groundPlaced = (i != 0)`.
           - `level.setBlock(belowPos, stateToPlace, 2)`.
           - `belowPos.move(inwards)`.
         *(Note: if `stateToPlace.is(cur.block)`, `belowPos` does NOT move `inwards` on that iteration!)*
         - If loop finishes: `groundPlaced = true`.
       - If `groundPlaced`: `surface.add(groundPos)`.
  3. **`distributeVegetation`**: iterate `surfacePos` in **`surface` (`HashSet<BlockPos>` iteration order!)** **[oracle]**:
     - If `vegetation_chance > 0.0F && random.nextFloat() < vegetation_chance`:
       - Call `placeVegetation(level, chunkGenerator, random, surfacePos)` -> `vegetation_feature.place(level, chunkGenerator, random, surfacePos.relative(outwards))`.
  4. Return `!surface.isEmpty()`.

##### 4.56 `minecraft:waterlogged_vegetation_patch` (`WaterloggedVegetationPatchFeature`)
- **Fields**: identical to `minecraft:vegetation_patch` (§4.55).
- **Differences from `vegetation_patch`**:
  1. **`placeGroundPatch`**: runs `super.placeGroundPatch(...)` to get `surface`, then allocates a second **`HashSet<BlockPos> waterSurface`** **[oracle]**.
     - Iterates `surfacePos` in `surface`: adds `surfacePos` to `waterSurface` iff none of `[NORTH, EAST, SOUTH, WEST, DOWN]` is exposed (where a direction `d` is exposed iff `!state(surfacePos.relative(d)).isFaceSturdy(level, surfacePos.relative(d), d.getOpposite())`).
     - Iterates `surfacePos` in `waterSurface`: `level.setBlock(surfacePos, minecraft:water, 2)` (replaces the top ground block with water).
     - Returns `waterSurface` as the set iterated by `distributeVegetation` and checked for `!isEmpty()`.
  2. **`placeVegetation(placementPos)`**: calls `super.placeVegetation(placementPos.below())` (so for `surface = "floor"`, `vegetation_feature` is placed at `placementPos.below().above() == placementPos`, inside the water block!). If that returns `true`, re-reads `placed = level.getBlockState(placementPos)`: if it has `WATERLOGGED == false`, sets `WATERLOGGED = true` (`flags = 2`).

##### 4.57 `minecraft:void_start_platform` (`VoidStartPlatformFeature`)
- **Fields**: none.
- **Behavior** (no RNG):
  1. Let `currentChunk = ChunkPos.containing(origin)`. Platform center is `(8, origin.y + 3, 8)` in chunk `(0, 0)`.
  2. If `max(|currentChunk.x|, |currentChunk.z|) > 1`: return **`true`**.
  3. Loop `z = currentChunk.minBlockZ ..= currentChunk.maxBlockZ` (outer), `x = currentChunk.minBlockX ..= currentChunk.maxBlockX` (inner):
     - If `max(|8 - x|, |8 - z|) <= 16`:
       - At `(x, origin.y + 3, z)`: place `minecraft:cobblestone` if `x == 8 && z == 8`, else `minecraft:stone` (`flags = 2`).
  4. Return **`true`**.

##### 4.58 `minecraft:multiface_growth` (`MultifaceGrowthFeature`)
- **Fields**:
  - `block`: block ID (required; validated to be a `MultifaceSpreadeableBlock`, e.g. `minecraft:glow_lichen`, `minecraft:sculk_vein`, `minecraft:resin_clump`)
  - `search_range`: `i32` in `[1, 64]` (optional, default `10`)
  - `can_place_on_floor`: `bool` (optional, default `false`)
  - `can_place_on_ceiling`: `bool` (optional, default `false`)
  - `can_place_on_wall`: `bool` (optional, default `false`)
  - `chance_of_spreading`: `f32` in `[0.0, 1.0]` (optional, default `0.5`)
  - `can_be_placed_on`: `HolderSet<Block>` (required).
- **Valid directions list (`validDirections`)** (exact build order before shuffling):
  - If `can_place_on_ceiling`: add `UP`.
  - If `can_place_on_floor`: add `DOWN`.
  - If `can_place_on_wall`: add `NORTH, EAST, SOUTH, WEST` (`Direction.Plane.HORIZONTAL`).
- **Helper `placeGrowthIfPossible(placerBlock, level, pos, oldState, random, placementDirs)`**:
  - For each `dir` in `placementDirs`:
    - If `level.getBlockState(pos.relative(dir)).is(can_be_placed_on)`:
      - `newState = placerBlock.getStateForPlacement(oldState, level, pos, dir)` (returns `null` if `!isValidStateForPlacement` — i.e., `oldState` already has `dir`, or `!canAttachTo(level, pos, dir)`; otherwise merges `dir=true` onto `oldState` if `oldState.is(placerBlock)` else `defaultState`, setting `WATERLOGGED = true` if `oldState.is(water) && fluidState.isSource()`).
      - If `newState == null`: return **`false`** immediately (does NOT try remaining `placementDirs`!).
      - `level.setBlockAndUpdate(pos, newState)` (`flags = 3`).
      - `level.getChunk(pos).markPosForPostProcessing(pos)`.
      - If `random.nextFloat() < chance_of_spreading`: call `placerBlock.getSpreader().spreadFromFaceTowardRandomDirection(newState, level, pos, dir, random, postProcess = true)` (shuffles all 6 `Direction.values()` via `Direction.allShuffled(random)` and spreads to the first direction where one of `[SAME_POSITION, SAME_PLANE, WRAP_AROUND]` passes `canSpreadInto`, writing with `flags = 2` and marking post-processing).
      - Return **`true`**.
  - Return `false`.
- **Behavior**:
  1. Require `state(origin)` is air or `minecraft:water` (and `block` is a `MultifaceSpreadeableBlock`); else return **`false`**.
  2. `dirs = Util.shuffledCopy(validDirections(), random)`.
  3. If `placeGrowthIfPossible(placerBlock, level, origin, state(origin), random, dirs)`: return **`true`**.
  4. Loop `searchDir` in `dirs`:
     - `placementDirs = Util.toShuffledList(validDirections() without searchDir.getOpposite(), random)`.
     - Loop `i = 0 .. search_range - 1`:
       - **VANILLA BUG (MUST REPRODUCE FOR PARITY)**: sets `pos = origin.relative(searchDir)` on **every** iteration (NOT `pos.move(searchDir)`!), so it re-tests the exact same 1-block neighbor `origin + searchDir` up to `search_range` times!
       - Read `s = level.getBlockState(pos)`. If `!isAirOrWater(s) && !s.is(block)`: break.
       - If `placeGrowthIfPossible(placerBlock, level, pos, s, random, placementDirs)`: return **`true`**.
  5. Return **`false`**.

---

## Edge cases and known quirks

1. **`WorldgenRandom` wrapping `XoroshiroRandomSource` (`BitRandomSource` adapter)**: `WorldgenRandom` inherits `BitRandomSource` from `LegacyRandomSource` and only overrides `next(bits)` to take the top `bits` of `XoroshiroRandomSource.nextLong()`. Consequently `nextInt(bound)` uses `BitRandomSource`'s LCG-style modulo rejection, `nextLong()` and `nextDouble()` each consume **two** 64-bit Xoroshiro outputs, and `setSeed` does **not** clear `WorldgenRandom`'s inherited Gaussian cache (**[oracle]**).
2. **`FeatureSorter` tie-breaking depends on `biomeSource.possibleBiomes()` order**: Topological cycle breaking and `TreeMap`/`TreeSet` priority ordering depend on the integer `featureIndex` assigned during the first pass over `biomeSource.possibleBiomes()` (**[oracle]**).
3. **Structures execute inside `FEATURES` and share the step's RNG stream**: Structure pieces are interleaved per decoration step **before** that step's features, with `random.setFeatureSeed(decorationSeed, structureIndex, stepIndex)`. Each chunk only runs pieces whose `BoundingBox` intersects the chunk's `16×16` XZ column.
4. **Biome filter runs only on top-level features**: `PlacedFeature.placeWithBiomeCheck` sets `topFeature = Optional.of(this)`; child `PlacedFeature`s invoked via `PlacedFeature.place` or `FeaturePlacer.place` set `topFeature = Optional.empty()`, so `minecraft:biome` throws `IllegalStateException` if placed on a sub-feature.
5. **Writes outside `WorldGenRegion` write radius (`writeRadiusCutoff = 1`, 3×3 chunks)**: Silently dropped with a rate-limited log error (`setBlock` returns `false`, `ensureCanWrite` returns `false`). Reads work across the full `5×5` region (`center ± 2` chunks); reads outside throw.
6. **`WorldGenRegion.getRandom()` shared state**: `SimpleBlockFeature` placing `MossyCarpetBlock` calls `level.getRandom()` (which is a chunk-local `LegacyRandomSource` on `WorldGenRegion`, NOT the feature's `WorldgenRandom`).
7. **`OreFeature` bypasses heightmaps**: `OreFeature` writes via `BulkSectionAccess` directly into `LevelChunkSection.setBlockState(..., false)`, skipping heightmap updates and chunk post-processing flags.
8. **`HeightmapPlacement` uses `<` against `minY`**: Fails when `height <= level.minY`, not `< level.minY`.
9. **`CountOnEveryLayerPlacement` and `NoiseBasedCountPlacement` ordering quirks**: `CountOnEveryLayerPlacement` starts at `layer = 0` and loops while at least one column in the chunk yields a valid `findOnGroundYPosition`; `NoiseBasedCountPlacement` evaluates `Biome.BIOME_INFO_NOISE` (Simplex, seed `2345`).
10. **`SurfaceRelativeThresholdFilter` checks `WORLD_SURFACE_WG` (or configured heightmap) with `min_inclusive`/`max_inclusive` offsets**: Uses `origin.y <= height + max_inclusive && origin.y >= height + min_inclusive`.
11. **`BlockColumnFeature` truncation starts at `origin + direction`**: It never tests `allowed_placement` at `origin` itself.
12. **`ProjectedRandomPatchySquare` with `max_projection_height == 0`**: Tests `project_through` at `basePos.below()` before decrementing `drop`, so it still moves down 1 block when `max_projection_height == 0` and `project_through` passes.
13. **`SingleBlockPillarFeature` cap position on 0-length pillar**: Moves back 1 step (`direction.getOpposite()`) unconditionally before invoking `cap_feature`, so a 0-block pillar runs `cap_feature` at `origin.relative(direction.getOpposite())`.
14. **`BambooFeature` return value & stalk height**: Returns `true` whenever `origin` is empty even if `canSurvive` is `false`; top `BAMBOO_FINAL_LARGE` is placed 1 block above the `height` trunk blocks without checking `isEmptyBlock`.
15. **`FallenTreeFeature` & `RootSystemFeature` `isFaceSturdy` wrong-position bug**: `FallenTreeFeature.isOverSolidGround` checks `state(pos.below()).isFaceSturdy(level, pos, UP)` (passes `pos`, not `pos.below()`); `RootSystemFeature.placeRoots` checks `state(p.above()).isFaceSturdy(level, p, DOWN)` (passes `p`, not `p.above()`).
16. **`IcebergFeature` quirks**: Ignores `origin.y` (uses `seaLevel`); checks `xo < radius` (not `|xo| < radius`); `generateCutOut` re-draws `random.nextInt(5)` on every iteration of its underwater `for` loop condition; `heightDependentRadiusRound` draws RNG per `(xo, zo, yOff)` voxel.
17. **`SpeleothemClusterFeature` unconditional RNG**: Draws `wantStalactite`, `wantStalagmite`, and `mergeTips = random.nextBoolean()` unconditionally even when ceiling/floor or heights are absent/zero.
18. **`SteppedColumnClusterFeature` dead center & dead parameter**: At `colPos == pos`, `stepLimit == 0`, causing both `findSurface` and `findAir` to return `null` immediately (center column of every sub-column is skipped); `lavaSeaLevel` is unused.
19. **`UnderwaterMagmaFeature` RNG-before-validity filter**: Draws `random.nextFloat() < prob` for every block in the cube before testing `isValidPlacement`.
20. **`SculkVeinBlock.hasSubstrateAccess` tag mismatch**: Checks `#minecraft:sculk_replaceable` (not `#minecraft:sculk_replaceable_world_gen`) even during worldgen.
21. **`HugeRedMushroomFeature` validation bug**: `isValidPosition` passes `treeHeight = -1` into `getTreeRadiusForHeight`, so red mushrooms always validate radius `0` (only the 1×1 center column). Both huge mushroom features pass `origin` (not `blockPos`) to `cap_provider` and `stem_provider`.
22. **`HugeFungusFeature` unconditional vine RNG**: `placeHatBlock` always calls `random.nextFloat() < vinesProbability` even when `vinesProbability == 0.0F` (warped fungus).
23. **`VegetationPatchFeature` / `WaterloggedVegetationPatchFeature` / `FallenTreeFeature` `HashSet<BlockPos>` iteration order**: `distributeVegetation` and `log_decorators` iterate a Java `HashSet<BlockPos>`, so RNG consumption order depends on `BlockPos.hashCode() = ((y + z * 31) * 31) + x` and Java `HashMap` table capacity/resizing (**[oracle]**).
24. **`MultifaceGrowthFeature` search-loop reset bug**: Resets `pos` to `origin + searchDirection` every iteration instead of advancing along `searchDirection`.
25. **`getOptionalState` vs `getState`**: `rule_based` (when no rule matches and `fallback` is absent/null) and `random_block` (when `blocks` is empty) return `null` from `getOptionalState` and fall back to `level.getBlockState(pos)` in `getState`. Only `SimpleBlockFeature`, `DiskFeature`, and `ProjectedRandomPatchySquare` call `getOptionalState` (skipping placement on `null`); all other features call `getState`.

---

## Constants and IDs

### Registries
| Registry ID | Entry count (26.3) | Notes |
|---|---|---|
| `minecraft:worldgen/feature_type` | 58 | Built-in static registry |
| `minecraft:worldgen/feature` | data-driven | Flat `Feature` objects (no `ConfiguredFeature` wrapper in 26.3) |
| `minecraft:worldgen/placed_feature` | data-driven | `{ "feature": Holder<Feature>, "placement": List<PlacementModifier> }` |
| `minecraft:worldgen/placement_modifier_type` | 18 | Built-in static registry |
| `minecraft:worldgen/block_state_provider_type` | 10 | Built-in static registry |
| `minecraft:worldgen/block_state_provider` | data-driven | Dynamic synced registry (new in 26.3) |
| `minecraft:block_predicate_type` | 16 | Built-in static registry |
| `minecraft:height_provider_type` | 6 | Built-in static registry |
| `minecraft:int_provider_type` | 8 | Built-in static registry |
| `minecraft:float_provider_type` | 4 | Built-in static registry |
| `minecraft:rule_test` | 10 | Built-in static registry |

### Decoration steps (`GenerationStep.Decoration`, indices 0..10)
`0: raw_generation`, `1: lakes`, `2: local_modifications`, `3: underground_structures`, `4: surface_structures`, `5: strongholds`, `6: underground_ores`, `7: underground_decoration`, `8: fluid_springs`, `9: vegetal_decoration`, `10: top_layer_modification`.

### All 18 placement modifier IDs (`minecraft:worldgen/placement_modifier_type`)
`minecraft:biome`, `minecraft:block_predicate_filter`, `minecraft:count`, `minecraft:count_on_every_layer`, `minecraft:cuboid`, `minecraft:environment_scan`, `minecraft:fixed_placement`, `minecraft:height_range`, `minecraft:heightmap`, `minecraft:in_square`, `minecraft:noise_based_count`, `minecraft:noise_threshold_count`, `minecraft:offset`, `minecraft:random_chance`, `minecraft:randomly_selected`, `minecraft:rarity_filter`, `minecraft:surface_relative_threshold_filter`, `minecraft:surface_water_depth_filter`.

### All 16 block predicate IDs (`minecraft:block_predicate_type`)
`minecraft:all_of`, `minecraft:any_of`, `minecraft:has_sturdy_face`, `minecraft:height_range`, `minecraft:inside_world_bounds`, `minecraft:matching_biomes`, `minecraft:matching_block_tag`, `minecraft:matching_blocks`, `minecraft:matching_fluids`, `minecraft:not`, `minecraft:replaceable`, `minecraft:solid`, `minecraft:true`, `minecraft:unobstructed`, `minecraft:volume_match`, `minecraft:would_survive`.

### All 10 block state provider IDs (`minecraft:worldgen/block_state_provider_type`)
`minecraft:copy_properties`, `minecraft:dual_noise`, `minecraft:noise`, `minecraft:noise_threshold`, `minecraft:random_block`, `minecraft:randomized_int`, `minecraft:rotated`, `minecraft:rule_based`, `minecraft:simple`, `minecraft:weighted`.

### All 6 height provider IDs (`minecraft:height_provider_type`)
`minecraft:biased_to_bottom`, `minecraft:constant`, `minecraft:trapezoid`, `minecraft:uniform`, `minecraft:very_biased_to_bottom`, `minecraft:weighted_list`.

### All 8 int provider & 4 float provider IDs
- **Int (`minecraft:int_provider_type`)**: `minecraft:biased_to_bottom`, `minecraft:clamped`, `minecraft:clamped_normal`, `minecraft:constant`, `minecraft:trapezoid`, `minecraft:uniform`, `minecraft:very_biased_to_bottom`, `minecraft:weighted_list` *(+ inline constant integer)*.
- **Float (`minecraft:float_provider_type`)**: `minecraft:clamped_normal`, `minecraft:constant`, `minecraft:trapezoid`, `minecraft:uniform` *(+ inline constant float)*.

### All 10 rule test IDs (`minecraft:rule_test`)
`minecraft:all_of`, `minecraft:always_true`, `minecraft:any_of`, `minecraft:block_match`, `minecraft:blockstate_match`, `minecraft:height_match`, `minecraft:not`, `minecraft:random_block_match`, `minecraft:random_blockstate_match`, `minecraft:tag_match`.

### Tags and loot tables referenced directly by features
- **Block tags**: `#minecraft:base_stone_overworld`, `#minecraft:beneath_bamboo_podzol_replaceable`, `#minecraft:features_cannot_replace`, `#minecraft:leaves`, `#minecraft:replaceable_by_mushrooms`, `#minecraft:replaceable_by_trees`, `#minecraft:sculk_growth_inhibitors`, `#minecraft:sculk_replaceable`, `#minecraft:sculk_replaceable_world_gen`, `#minecraft:snow_layer_can_survive_on`, `#minecraft:snow_layer_cannot_survive_on`, `#minecraft:supports_bamboo`, `#minecraft:supports_chorus_flower`, `#minecraft:supports_chorus_plant`.
- **Fluid tags**: `#minecraft:lava`, `#minecraft:water`.
- **Feature tags** (`FeatureTags`): `#minecraft:overworld_trees`, etc. (used by datapack holder sets).
- **Loot tables**: `minecraft:chests/spawn_bonus_chest` (`BonusChestFeature`), `minecraft:chests/simple_dungeon` (`MonsterRoomFeature`).

### Pre-26.3 feature types removed or generalized in 26.3
| Pre-26.3 type ID | 26.3 replacement / status |
|---|---|
| `random_patch`, `flower`, `no_bonemeal_flower`, `nether_forest_vegetation`, `seagrass`, `sea_pickle` | Removed; expressed via placement modifiers (`count`, `random_offset`, `in_square`) + `simple_block` / `projected_random_patchy_square` / `overlay` / `sequence` |
| `ice_spike` | Generalized to `minecraft:spike` (§4.28) |
| `forest_rock` | Generalized to `minecraft:block_blob` (§4.14) |
| `basalt_columns` | Generalized to `minecraft:stepped_column_cluster` (§4.38) |
| `basalt_pillar`, `kelp`, `twisting_vines`, `weeping_vines` | Generalized to `minecraft:single_block_pillar` (§4.20) |
| `glowstone_blob` | Generalized to `minecraft:random_neighbor_spread` (§4.19) |
| `desert_well`, `coral_mushroom` | Generalized to `minecraft:template` (§4.43) |
| `pointed_dripstone` | Renamed/generalized to `minecraft:speleothem` (§4.32) |
| `dripstone_cluster` | Renamed/generalized to `minecraft:speleothem_cluster` (§4.33) |
| `carving_mask` (placement modifier) | Removed in 26.3 |

---

## Oracle probe points

Public reference entry points suitable for black-box oracle testing (`golden-features`):
- **Chunk step & orchestration**:
  - `net.minecraft.world.level.chunk.ChunkGenerator#applyBiomeDecoration(WorldGenLevel, ChunkAccess, StructureManager)`
  - `net.minecraft.world.level.chunk.status.ChunkStatusTasks#generateFeatures(...)`
  - `net.minecraft.world.level.biome.FeatureSorter#buildFeaturesPerStep(List<T>, Function<T, List<HolderSet<PlacedFeature>>>, boolean)`
  - `net.minecraft.server.level.WorldGenRegion#setBlock(BlockPos, BlockState, int, int)` and `#ensureCanWrite(BlockPos)`
- **RNG**:
  - `net.minecraft.world.level.levelgen.WorldgenRandom` (`setDecorationSeed`, `setFeatureSeed`, `setSeed`, `nextInt`, `nextLong`, `nextDouble`, `nextGaussian`)
  - `net.minecraft.world.level.levelgen.XoroshiroRandomSource`
- **Placement**:
  - `net.minecraft.world.level.levelgen.placement.PlacedFeature#placeWithBiomeCheck(...)` and `#place(...)`
  - `net.minecraft.world.level.levelgen.placement.FeaturePlacer#place(...)`
  - `net.minecraft.world.level.levelgen.placement.PlacementModifier#getPositions(PlacementContext, RandomSource, BlockPos)`
  - `net.minecraft.world.level.levelgen.heightproviders.HeightProvider#sample(RandomSource, WorldGenerationContext)`
- **Shared helpers**:
  - `net.minecraft.world.level.levelgen.blockpredicates.BlockPredicate#test(WorldGenLevel, BlockPos)`
  - `net.minecraft.world.level.levelgen.feature.stateproviders.BlockStateProvider#getState(WorldGenLevel, RandomSource, BlockPos)` and `#getOptionalState(...)`
  - `net.minecraft.world.level.levelgen.structure.templatesystem.RuleTest#test(BlockState, BlockPos, RandomSource)`
  - `net.minecraft.world.level.levelgen.Column#scan(...)`
- **Features**:
  - `net.minecraft.world.level.levelgen.feature.Feature#place(WorldGenLevel, ChunkGenerator, RandomSource, BlockPos)` on every registered implementation in `FeatureTypes`
  - `net.minecraft.world.level.levelgen.feature.EndSpikeFeature#getSpikesForLevel(WorldGenLevel)`
  - `net.minecraft.world.level.block.SculkSpreader#createWorldGenSpreader()` and `#updateCursors(...)`
  - `net.minecraft.world.level.block.ChorusFlowerBlock#generatePlant(...)`

---

## Open questions

1. **`FeatureSorter` tie-breaking & `biomeSource.possibleBiomes()` iteration order** (`[oracle]` in §1.3):
   - *Question*: Does `MultiNoiseBiomeSource` / `TheEndBiomeSource` / `FixedBiomeSource` construct `possibleBiomes()` in a fixed deterministic order (e.g. `ObjectLinkedOpenHashSet` from the biome parameter list order), and does any cycle exist in vanilla 26.3 Overworld/Nether/End biome feature lists?
   - *Black-box probe*: Invoke `FeatureSorter.buildFeaturesPerStep` on the 26.3 vanilla Overworld, Nether, and End biome sources and dump the resulting `(step, globalIndex, featureId)` table; compare against Zenith's sorted table for exact index-for-index equality.
2. **`WorldgenRandom.setSeed` not resetting inherited Gaussian cache** (`[oracle]` in Inputs & State):
   - *Question*: In `applyBiomeDecoration`, the same `WorldgenRandom` instance is reused across every `(step, feature)` via `random.setFeatureSeed(...)`. If feature `A` draws an odd number of `nextGaussian()` calls (e.g. via `ClampedNormalInt` or `ClampedNormalFloat`, such as `speleothem_cluster` or `clamped_normal` int/float providers), does the cached second Gaussian survive `setFeatureSeed` and get consumed by feature `B` in the same chunk?
   - *Black-box probe*: Construct `WorldgenRandom(XoroshiroRandomSource(0))`, call `setFeatureSeed(seed, 0, 0)`, draw 1 `nextGaussian()`, call `setFeatureSeed(seed, 1, 0)`, draw 1 `nextGaussian()`, and compare against a fresh `WorldgenRandom` seeded directly with `(seed, 1, 0)`.
3. **`WorldGenRegion.getBrightness(LightLayer.BLOCK, pos)` during `FEATURES`** (`[oracle]` in Inputs & State and §4.26):
   - *Question*: During `FEATURES` (before `INITIALIZE_LIGHT` and `LIGHT`), does `WorldGenRegion.getBrightness(LightLayer.BLOCK, pos)` always return `0` for newly generated proto-chunks, or can a light-emitting block placed earlier in `FEATURES` (e.g. lava spring, glowstone, torch) cause a non-zero block light reading in `SnowAndFreezeFeature` or `LakeFeature`?
   - *Black-box probe*: Place a `minecraft:glowstone` or `minecraft:lava` block inside a `WorldGenRegion` at `FEATURES` status in a cold biome and probe `level.getBrightness(LightLayer.BLOCK, pos)` and `SnowAndFreezeFeature.place` at and adjacent to that position.
4. **Java `HashSet<BlockPos>` iteration order in `VegetationPatchFeature`, `WaterloggedVegetationPatchFeature`, and `FallenTreeFeature`** (`[oracle]` in §4.24, §4.55, §4.56):
   - *Question*: Exact parity of `distributeVegetation` (and `FallenTreeFeature`'s `log_decorators` when multiple log blocks exist) depends on iterating `HashSet<BlockPos>` in Java 21's `HashMap` bucket order (`h = key.hashCode(); hash = h ^ (h >>> 16); bucket = hash & (cap - 1)`, initial capacity 16, load factor 0.75, doubling on `size > threshold`, chaining in insertion order within a bucket unless treeified at bin length >= 8 & capacity >= 64).
   - *Black-box probe*: Run `VegetationPatchFeature.place` and `WaterloggedVegetationPatchFeature.place` on a lush-cave test chunk (fixed seed + fixed origin) and verify every placed clay/moss/azalea/dripleaf block matches Java bit-for-bit when Zenith replicates Java `HashSet<BlockPos>` iteration order.
5. **(implementer) `zenith-worldgen::TerrainChunk` mutator API for `FEATURES` step**:
   - *Context*: `TerrainChunk::set_block_and_update_heightmaps` only live-updates `OCEAN_FLOOR_WG` and `WORLD_SURFACE_WG` (which must remain frozen during `FEATURES`), and `TerrainChunk` has no public mutators for the 4 `FINAL` heightmaps, scheduled block/fluid ticks, block entities, or entities. `zenith-features` currently works around this by copying into `RegionChunk::from_terrain_chunk`. Adding `FINAL`-heightmap live-update and tick/BE mutators to `TerrainChunk` (or a shared mutable proto-chunk type) would eliminate the per-chunk copy.
6. **(implementer) `ProtoChunk` heightmaps and post-processing marks during `FEATURES`**:
   - *Observed in `golden-features` parity*: (a) `ProtoChunk.setBlockState` updates only the 4 `FINAL` heightmaps (`OCEAN_FLOOR`, `WORLD_SURFACE`, `MOTION_BLOCKING`, `MOTION_BLOCKING_NO_LEAVES`), leaving `OCEAN_FLOOR_WG` and `WORLD_SURFACE_WG` frozen at their `TERRAIN`-step values (verified on End `(72, 12)` chorus plant); (b) `ProtoChunk.markPosForPostProcessing` appends packed offsets without per-section deduplication (verified on Nether `(-307, -12)` and `(228, 240)`). Clarifying both explicitly in §1.1 would help future readers.
   **Answered (reader, 2026-11-25):** both confirmed. A proto chunk's set-block updates the heightmaps listed for its
   *persisted* status; while the features step runs, every chunk of the region is at `terrain`, whose list is the four
   final heightmaps (`OCEAN_FLOOR`, `WORLD_SURFACE`, `MOTION_BLOCKING`, `MOTION_BLOCKING_NO_LEAVES`), so the two
   `*_WG` heightmaps stay frozen; a listed heightmap the chunk does not have yet is created and primed on first use.
   Post-processing marks are appended to a plain per-section list (created on first use), with no de-duplication.
   Question 5 (an API change in `zenith-worldgen`) is a design note, not a spec question; the copy is fine for now.

---

## Parity status

`golden-features`: not yet run.
