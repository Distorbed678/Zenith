# Worldgen: legacy (non-jigsaw) structure types and piece generators (26.3)

- Verified by suites: `golden-structures`
- Sources: Minecraft: Java Edition 26.3 data-pack schemas (`minecraft:worldgen/structure`, `minecraft:worldgen/structure_type`, `minecraft:worldgen/structure_piece`), `docs/specs/math/random.md`, `docs/specs/worldgen/density-functions.md` §8, `docs/specs/worldgen/surface-and-carvers.md` §4, `docs/specs/registry/identifiers-registries-tags.md` §3.3, and local reference reading (2026-11-25). Everything below is written in our own words.
- Related specs:
  - [`worldgen/structures-placement-and-jigsaw.md`](./structures-placement-and-jigsaw.md) — structure set placement, start/reference chunk statuses, jigsaw assembly, `StructureTemplate` loading/placement, and structure processors
  - [`math/random.md`](../math/random.md) — legacy 48-bit LCG, xoroshiro128++, positional random factories, position hash
  - [`worldgen/density-functions.md`](./density-functions.md) — §8 `Beardifier` terrain adaptation rules and `getBaseHeight`/`getBaseColumn`
  - [`worldgen/surface-and-carvers.md`](./surface-and-carvers.md) — §4 chunk-step order and heightmap predicates
  - [`registry/identifiers-registries-tags.md`](../registry/identifiers-registries-tags.md) — §3.3 serialized `BlockState` format, identifiers, registries, tags

---

## Summary

Version 26.3 registers **16** structure types in built-in registry `minecraft:worldgen/structure_type`. Exactly **1** (`minecraft:jigsaw`) uses the generic jigsaw pool assembler and is **out of scope** here (specified in `structures-placement-and-jigsaw.md`). The remaining **15** types are legacy structures whose piece graphs, procedural geometry, or bespoke template assemblies are specified in full below.

### Complete `minecraft:worldgen/structure_type` registry (26.3)

| # | Structure type ID | Scope | Piece assembly model | Piece type count (`structure_piece`) |
|---|---|---|---|---|
| 1 | `minecraft:jigsaw` | **Out of scope** (see `structures-placement-and-jigsaw.md`) | Generic jigsaw pool assembler (villages, pillager outpost, bastion remnant, ancient city, trail ruins, trial chambers, 18 abandoned camps) | 1 (`jigsaw`) |
| 2 | `minecraft:mineshaft` | **In scope** (§2) | Procedural recursive piece tree (normal & mesa variants) | 4 |
| 3 | `minecraft:stronghold` | **In scope** (§3) | Procedural weighted BFS piece graph with retry loop | 13 |
| 4 | `minecraft:fortress` | **In scope** (§4) | Procedural dual-pool weighted BFS piece graph | 15 |
| 5 | `minecraft:ocean_monument` | **In scope** (§5) | Procedural 5×3×5 room-graph fitting + fixed monumental shell | 12 |
| 6 | `minecraft:woodland_mansion` | **In scope** (§6) | Procedural 11×11 3-floor grid planner + `.nbt` room/wall/roof templates | 1 |
| 7 | `minecraft:desert_pyramid` | **In scope** (§7) | Procedural single piece + archaeology `afterPlace` pass | 1 |
| 8 | `minecraft:jungle_temple` | **In scope** (§8) | Procedural single piece with redstone traps & puzzle | 1 |
| 9 | `minecraft:swamp_hut` | **In scope** (§9) | Procedural single piece with witch & cat spawn | 1 |
| 10 | `minecraft:igloo` | **In scope** (§10) | Bespoke `.nbt` template stack (top + optional shaft & basement) | 1 |
| 11 | `minecraft:shipwreck` | **In scope** (§11) | Single `.nbt` template (beached or ocean pool) with terrain height fit | 1 |
| 12 | `minecraft:ocean_ruin` | **In scope** (§12) | Single or clustered `.nbt` templates (warm single-layer or cold 3-layer) | 1 |
| 13 | `minecraft:buried_treasure` | **In scope** (§13) | Procedural single chest buried under ocean floor | 1 |
| 14 | `minecraft:end_city` | **In scope** (§14) | Recursive section-generator tree of `.nbt` templates | 1 |
| 15 | `minecraft:nether_fossil` | **In scope** (§15) | Single `.nbt` template on Nether solid floor + optional dried ghast | 1 |
| 16 | `minecraft:ruined_portal` | **In scope** (§16) | Single `.nbt` template with custom processor stack & netherrack spread | 1 |

> **Runtime game data rule**: Zenith never bundles game `.nbt` structure templates or loot-table JSON files. For every template-backed structure (§6, §10, §11, §12, §14, §15, §16), implementations load templates at runtime from the user's own Minecraft: Java Edition installation (data packs / client jar under `data/<namespace>/structure/<path>.nbt`) and attach loot tables solely by `ResourceKey<LootTable>` identifier and 64-bit seed.

### Built-in 26.3 legacy `minecraft:worldgen/structure` defaults (terrain adaptation, generation step, spawn overrides)

Terrain adaptation is consumed by `Beardifier` during `ChunkStatus.TERRAIN` (`density-functions.md` §8). For every non-pool piece, `Beardifier` inserts a `Rigid(piece.boundingBox, structure.terrainAdaptation, groundLevelDelta = 0)` whenever `terrainAdaptation != NONE` and the piece's horizontal box intersects `[chunkMinX - 12, chunkMinZ - 12, chunkMinX + 27, chunkMinZ + 27]`.

| Built-in structure ID | Type | Generation step | Terrain adaptation | Spawn overrides (box type & spawns) |
|---|---|---|---|---|
| `minecraft:mineshaft` | `mineshaft` (`normal`) | `underground_structures` | `none` | none |
| `minecraft:mineshaft_mesa` | `mineshaft` (`mesa`) | `underground_structures` | `none` | none |
| `minecraft:stronghold` | `stronghold` | `surface_structures` | **`bury`** | none |
| `minecraft:fortress` | `fortress` | `underground_decoration` | `none` | `monster` (`piece`): blaze wt 10 (2–3), zombified_piglin wt 5 (4), wither_skeleton wt 8 (5), skeleton wt 2 (5), magma_cube wt 3 (4) |
| `minecraft:ocean_monument` | `ocean_monument` | `surface_structures` | `none` | `monster` (`full`): guardian wt 1 (2–4); `underground_water_creature` (`full`): empty; `axolotls` (`full`): empty |
| `minecraft:woodland_mansion` | `woodland_mansion` | `surface_structures` | `none` | none |
| `minecraft:desert_pyramid` | `desert_pyramid` | `surface_structures` | `none` | none |
| `minecraft:jungle_pyramid` | `jungle_temple` | `surface_structures` | `none` | none |
| `minecraft:swamp_hut` | `swamp_hut` | `surface_structures` | `none` | `monster` (`piece`): witch wt 1 (1); `creature` (`piece`): cat wt 1 (1) |
| `minecraft:igloo` | `igloo` | `surface_structures` | `none` | none |
| `minecraft:shipwreck` | `shipwreck` (`is_beached=false`) | `surface_structures` | `none` | none |
| `minecraft:shipwreck_beached` | `shipwreck` (`is_beached=true`) | `surface_structures` | `none` | none |
| `minecraft:ocean_ruin_cold` | `ocean_ruin` (`cold`, `0.3`, `0.9`) | `surface_structures` | `none` | none |
| `minecraft:ocean_ruin_warm` | `ocean_ruin` (`warm`, `0.3`, `0.9`) | `surface_structures` | `none` | none |
| `minecraft:buried_treasure` | `buried_treasure` | `underground_structures` | `none` | none |
| `minecraft:end_city` | `end_city` | `surface_structures` | `none` | none |
| `minecraft:nether_fossil` | `nether_fossil` (`uniform[32..below_top(2)]`) | `underground_decoration` | **`beard_thin`** | none |
| `minecraft:ruined_portal` | `ruined_portal` (standard, 2 setups) | `surface_structures` | `none` | none |
| `minecraft:ruined_portal_desert` | `ruined_portal` (desert, 1 setup) | `surface_structures` | `none` | none |
| `minecraft:ruined_portal_jungle` | `ruined_portal` (jungle, 1 setup) | `surface_structures` | `none` | none |
| `minecraft:ruined_portal_swamp` | `ruined_portal` (swamp, 1 setup) | `surface_structures` | `none` | none |
| `minecraft:ruined_portal_mountain` | `ruined_portal` (mountain, 2 setups) | `surface_structures` | `none` | none |
| `minecraft:ruined_portal_ocean` | `ruined_portal` (ocean, 1 setup) | `surface_structures` | `none` | none |
| `minecraft:ruined_portal_nether` | `ruined_portal` (nether, 1 setup) | `surface_structures` | `none` | none |

*(Note: in data packs, `step`, `terrain_adaptation`, `biomes`, and `spawn_overrides` are configurable on any structure definition via `StructureSettings`; the table above records the vanilla 26.3 definitions. Only `stronghold` (`bury`) and `nether_fossil` (`beard_thin`) use non-`none` terrain adaptation in vanilla among legacy structures.)*

---

## Inputs and state

### Pipeline integration (`surface-and-carvers.md` §4)
1. **`structure_starts` status**: For each candidate chunk selected by a `StructureSet` placement, `Structure.generate` constructs a `GenerationContext` whose RNG is a **legacy 48-bit LCG** `WorldgenRandom` seeded via `setLargeFeatureSeed(worldSeed, chunkX, chunkZ)` (`surface-and-carvers.md` §3.2). It calls `findGenerationPoint(context)`, validates the stub position's noise biome against `validBiome` (via `biomeResolver.getNoiseBiome(x >> 2, y >> 2, z >> 2)`), invokes the piece builder to materialize all `StructurePiece` instances, and stores the `StructureStart` on the chunk if `!pieces.isEmpty()`.
2. **`structure_references` status**: Copies start references within radius 8 chunks (`structures-placement-and-jigsaw.md`).
3. **`terrain` status**: `Beardifier` reads every overlapping start with `terrainAdaptation != NONE` and adds density contributions during terrain fill (`density-functions.md` §8).
4. **`features` status**: `ChunkGenerator.applyBiomeDecoration` iterates `GenerationStep.Decoration` indices `0..N-1`. For each structure assigned to that step, it reseeds the decoration RNG (`random.setFeatureSeed(decorationSeed, index, stepIndex)` — see `structures-placement-and-jigsaw.md`) and invokes `start.placeInChunk(level, structureManager, generator, random, chunkBB, chunkPos)` for every start overlapping the chunk.
   - `chunkBB` (`getWritableArea(chunk)`) is `[chunkMinX, heightAccessor.minY + 1, chunkMinZ]` through `[chunkMinX + 15, heightAccessor.maxY, chunkMinZ + 15]` inclusive. Note the bottom floor is **`minY + 1`**, protecting the lowest bedrock layer.
   - `level.getRandom()` inside `WorldGenRegion` is **deterministic per chunk**: `randomState.getOrCreateRandomFactory("minecraft:worldgen_region_random").at(centerChunk.minBlockX, 0, centerChunk.minBlockZ)` (using the dimension's random algorithm: xoroshiro in Overworld, legacy LCG in Nether/End). Several pieces (`DesertPyramidPiece`, `WoodlandMansionPiece`) draw from `level.getRandom()` instead of the `postProcess` `random` parameter.

### Common JSON schema: `StructureSettings`
Every structure type codec embeds `StructureSettings` inline (flat keys alongside the type's own fields):
- `biomes`: `HolderSet<Biome>` (required; `#tag`, single ID, or array of IDs).
- `spawn_overrides`: map from `MobCategory` string (`monster`, `creature`, `ambient`, `axolotls`, `underground_water_creature`, `water_creature`, `water_ambient`, `misc`) to `{ "bounding_box": "piece" | "full", "spawns": WeightedList<SpawnerData> }` (required; may be `{}`).
- `step`: `GenerationStep.Decoration` enum string (required; e.g. `"surface_structures"`, `"underground_structures"`, `"underground_decoration"`).
- `terrain_adaptation`: `"none"` | `"bury"` | `"beard_thin"` | `"beard_box"` | `"encapsulate"` (optional, default `"none"`).

### Coordinate, direction, rotation, and RNG conventions
- Integer division `/` truncates toward zero; `i32` / `i64` arithmetic wraps two's complement; floats are IEEE-754 `f32`/`f64`.
- **`ChunkPos` helpers**: `minBlockX = chunkX << 4`, `blockX(o) = (chunkX << 4) + o`, `middleBlockX = (chunkX << 4) + 8`, `worldPosition = (minBlockX, 0, minBlockZ)` (likewise Z).
- **Heightmap conventions**:
  - `chunkGenerator.getBaseHeight(x, z, type, heightAccessor, randomState)` returns the Y of the **first empty block above** the highest block matching `type` (or `minY` if empty).
  - `chunkGenerator.getFirstOccupiedHeight(x, z, type, ...)` returns `getBaseHeight(...) - 1`.
  - `WorldGenRegion.getHeight(type, x, z)` and `level.getHeightmapPos(type, pos).getY()` return `chunk.getHeight(type, x & 15, z & 15) + 1` (the **first available** Y, matching `getBaseHeight`).
- **`Direction` numbering and ordering**:
  - `Direction.values()` (3D data value order): `0 = DOWN (0,-1,0)`, `1 = UP (0,+1,0)`, `2 = NORTH (0,0,-1)`, `3 = SOUTH (0,0,+1)`, `4 = WEST (-1,0,0)`, `5 = EAST (+1,0,0)`.
  - `2D data value` (`get2DDataValue` / `from2DDataValue`): `0 = SOUTH`, `1 = WEST`, `2 = NORTH`, `3 = EAST` (`DOWN`/`UP` have `-1`).
  - `Direction.Plane.HORIZONTAL` iteration & random-choice order: `[NORTH, EAST, SOUTH, WEST]` (`getRandomHorizontalDirection(random)` calls `random.nextInt(4)` indexing this array!).
  - Clockwise horizontal cycle: `NORTH → EAST → SOUTH → WEST → NORTH`.
- **`Rotation` enum order** (`Rotation.values()`): `0 = NONE`, `1 = CLOCKWISE_90`, `2 = CLOCKWISE_180`, `3 = COUNTERCLOCKWISE_90`. `Rotation.getRandom(random)` indexes this 4-element array via `random.nextInt(4)`.
- **`Mirror` enum**: `NONE` (identity), `LEFT_RIGHT` (inverts Z: `z -> -z`, flips `NORTH ↔ SOUTH`), `FRONT_BACK` (inverts X: `x -> -x`, flips `WEST ↔ EAST`).
- **Default block states**: Unless explicitly noted with property overrides, every named block uses its registered default state (`identifiers-registries-tags.md` §3.3). Notably: `*_stairs`, `*_door`, `wall_torch`, `chest`, `dried_ghast` default to `facing=north`; `rail` defaults to `shape=north_south`; `*_slab` defaults to `type=bottom`; `*_fence` / `iron_bars` default to all connections `false`; `lever` defaults to `face=wall, facing=north, powered=false`.
- **Standalone legacy RNG helper**: `RandomSource.createThreadLocalInstance(seed)` constructs a `SingleThreadedRandomSource(seed)` — always the **legacy 48-bit LCG** (`math/random.md`), whose `forkPositional()` draws one `nextLong()` seed `P` and returns a legacy positional factory (`at(x, y, z)` seeds a legacy LCG with `positionHash(x, y, z) ^ P`).
- **RNG helpers**:
  - `Mth.randomBetweenInclusive(rng, min, max)` = `rng.nextInt(max - min + 1) + min` (unconditionally consumes RNG even if `min == max`).
  - `RandomSource.nextIntBetweenInclusive(min, max)` = `rng.nextInt(max - min + 1) + min`.
  - `Mth.nextInt(rng, min, max)` = if `min >= max` return `min` (no RNG draw!), else `rng.nextInt(max - min + 1) + min`.
  - `RandomSource.nextInt(origin, bound)` = `origin + rng.nextInt(bound - origin)` (requires `origin < bound`).
  - `Util.getRandom(array, rng)` = `array[rng.nextInt(array.length)]`.
  - `Util.shuffle(list, rng)`: reverse Fisher-Yates for `i = list.size()` down to `2`: `swapTo = rng.nextInt(i)`; swap indices `i - 1` and `swapTo`.
- **`BlockPos.betweenClosed(minX, minY, minZ, maxX, maxY, maxZ)`** (and `betweenClosedStream`): iterates `index = 0 .. width*height*depth - 1` with **`x` fastest (innermost), `y` middle, `z` slowest (outermost)**: `x = minX + (index % width)`, `y = minY + ((index / width) % height)`, `z = minZ + (index / (width * height))`. When called with two `BlockPos(a, b)`, min/max are normalized per axis first.
- **`Vec3i.compareTo`**: compares **Y ascending first, Z ascending second, X ascending third**.
- **`SpawnerBlockEntity.setEntityId(entityType, random)`**: for a newly placed spawner whose `nextSpawnData == null` and `spawnPotentials` is empty, `getOrCreateNextSpawnData` sees an empty weighted list and creates a fresh `SpawnData` **without consuming any RNG draws**, then writes `id = entityType`'s registry identifier string into `SpawnData.entity`.

---

## Behavior (ordered rules)

### 1. Shared structure and piece helpers

#### 1.1 `BoundingBox` geometry
An axis-aligned inclusive integer box `[minX..maxX] × [minY..maxY] × [minZ..maxZ]`. If constructed with inverted bounds (`max < min`), min/max are swapped per axis.
- `getXSpan = maxX - minX + 1` (likewise Y, Z).
- `getCenter = (minX + (maxX - minX + 1) / 2, minY + (maxY - minY + 1) / 2, minZ + (maxZ - minZ + 1) / 2)` (truncating integer division).
- `isInside(x, y, z)`: `minX <= x <= maxX && minY <= y <= maxY && minZ <= z <= maxZ`.
- `intersects(other)`: inclusive overlap on all 3 axes (`this.maxX >= other.minX && this.minX <= other.maxX`, etc.); 2D `intersects(minX, minZ, maxX, maxZ)` checks X and Z only.
- `inflatedBy(d)`: `[minX - d, minY - d, minZ - d, maxX + d, maxY + d, maxZ + d]`.
- `Structure.adjustBoundingBox(box)`: returns `box.inflatedBy(12)` when `terrainAdaptation != NONE`, else `box` unchanged.
- `makeBoundingBox(x, y, z, dir, width, height, depth)`:
  - If `dir` is `NORTH` or `SOUTH` (Z axis): `[x, y, z] .. [x + width - 1, y + height - 1, z + depth - 1]`.
  - If `dir` is `WEST` or `EAST` (X axis): `[x, y, z] .. [x + depth - 1, y + height - 1, z + width - 1]`.
- `BoundingBox.orientBox(footX, footY, footZ, offX, offY, offZ, width, height, depth, dir)`:
  - `SOUTH` (or default): `[footX + offX, footY + offY, footZ + offZ] .. [footX + width - 1 + offX, footY + height - 1 + offY, footZ + depth - 1 + offZ]`.
  - `NORTH`: `[footX + offX, footY + offY, footZ - depth + 1 + offZ] .. [footX + width - 1 + offX, footY + height - 1 + offY, footZ + offZ]`.
  - `WEST`: `[footX - depth + 1 + offZ, footY + offY, footZ + offX] .. [footX + offZ, footY + height - 1 + offY, footZ + width - 1 + offX]`.
  - `EAST`: `[footX + offZ, footY + offY, footZ + offX] .. [footX + depth - 1 + offZ, footY + height - 1 + offY, footZ + width - 1 + offX]`.

#### 1.2 `Structure` start-point and height helpers
- `couldValidBiomeExistInTerrainColumn(bx, bz)`: scans every quart Y from `(heightAccessor.minY - 1) >> 2` through `heightAccessor.maxY >> 2` inclusive in column `(bx >> 2, bz >> 2)` using `biomeSource.createResolverForChunk`; returns `true` if any quart matches `validBiome`.
- `couldValidBiomeExistOnTopOfChunkCenter()`: calls `couldValidBiomeExistInTerrainColumn(chunk.middleBlockX, chunk.middleBlockZ)`.
- `onTopOfChunkCenterWithoutBiomeCheck(context, heightmap, generator)`: computes `y = chunkGenerator.getFirstOccupiedHeight(chunk.middleBlockX, chunk.middleBlockZ, heightmap, ...)` and returns stub at `(middleBlockX, y, middleBlockZ)`.
- `onTopOfChunkCenter(context, heightmap, generator)`: returns empty if `!couldValidBiomeExistOnTopOfChunkCenter()`, else calls `onTopOfChunkCenterWithoutBiomeCheck`.
- `getCornerHeights(context, minX, sizeX, minZ, sizeZ)`: samples `getFirstOccupiedHeight(..., WORLD_SURFACE_WG)` at the 4 points `(minX, minZ)`, `(minX, minZ + sizeZ)`, `(minX + sizeX, minZ)`, `(minX + sizeX, minZ + sizeZ)`.
- `getMeanFirstOccupiedHeight(context, minX, sizeX, minZ, sizeZ)`: truncating integer average `(h0 + h1 + h2 + h3) / 4` of `getCornerHeights`.
- `getLowestY(context, minX, minZ, sizeX, sizeZ)`: `min(h0, h1, h2, h3)` of `getCornerHeights`. The 2-arg overload `getLowestY(context, sizeX, sizeZ)` uses `minX = chunk.minBlockX`, `minZ = chunk.minBlockZ`.
- `getLowestYIn5by5Box(context, blockX, blockZ, rotation)`: sets `(ox, oz)` to `(+5, +5)` for `NONE`, `(-5, +5)` for `CLOCKWISE_90`, `(-5, -5)` for `CLOCKWISE_180`, `(+5, -5)` for `COUNTERCLOCKWISE_90`; returns `BlockPos(blockX, getLowestY(context, blockX, blockZ, ox, oz), blockZ)`.
- **`SinglePieceStructure(constructor, width, depth, settings)`**:
  1. If `!context.couldValidBiomeExistOnTopOfChunkCenter()`, return empty.
  2. If `getLowestY(context, width, depth) < chunkGenerator.getSeaLevel()`, return empty.
  3. Return `onTopOfChunkCenterWithoutBiomeCheck(context, WORLD_SURFACE_WG, builder -> builder.addPiece(constructor(context.random(), chunk.minBlockX, chunk.minBlockZ)))`.

#### 1.3 `StructurePiecesBuilder` helpers
- `findCollisionPiece(box)`: returns the first piece in `pieces` whose `boundingBox.intersects(box)` is true, or `null`.
- `offsetPiecesVertically(dy)`: calls `piece.move(0, dy, 0)` on every piece.
- `moveBelowSeaLevel(seaLevel, minY, random, offset)`:
  1. `maxY = seaLevel - offset`; `bb = encapsulating bounding box of all pieces`.
  2. `y1Pos = bb.getYSpan() + minY + 1`.
  3. If `y1Pos < maxY`: `y1Pos += random.nextInt(maxY - y1Pos)`.
  4. `dy = y1Pos - bb.maxY()`; call `offsetPiecesVertically(dy)`; return `dy`.
- `moveInsideHeights(random, lowestAllowed, highestAllowed)`:
  1. `bb = encapsulating bounding box of all pieces`.
  2. `span = highestAllowed - lowestAllowed + 1 - bb.getYSpan()`.
  3. `y0Pos = (span > 1) ? lowestAllowed + random.nextInt(span) : lowestAllowed`.
  4. `dy = y0Pos - bb.minY()`; call `offsetPiecesVertically(dy)`.

#### 1.4 `StructurePiece` orientation, coordinate transform, and block placement
Every `StructurePiece` stores `boundingBox`, `genDepth`, `orientation: Option<Direction>`, and derived `(mirror, rotation)`:

| `orientation` | `mirror` | `rotation` | `getWorldX(x, z)` | `getWorldY(y)` | `getWorldZ(x, z)` |
|---|---|---|---|---|---|
| `null` | `NONE` | `NONE` | `x` | `y` | `z` |
| `NORTH` | `NONE` | `NONE` | `bb.minX + x` | `bb.minY + y` | `bb.maxZ - z` |
| `SOUTH` | `LEFT_RIGHT` | `NONE` | `bb.minX + x` | `bb.minY + y` | `bb.minZ + z` |
| `WEST` | `LEFT_RIGHT` | `CLOCKWISE_90` | `bb.maxX - z` | `bb.minY + y` | `bb.minZ + x` |
| `EAST` | `NONE` | `CLOCKWISE_90` | `bb.minX + z` | `bb.minY + y` | `bb.minZ + x` |

- **Critical**: When `orientation == null` (used by `MineShaftRoom`, `MineShaftCrossing`, `BuriedTreasurePiece`), `(x, y, z)` passed to `placeBlock`/`generateBox` are **world coordinates directly** and no mirror/rotation is applied to placed block states.
- **`placeBlock(level, blockState, x, y, z, chunkBB)`**:
  1. Compute `pos = getWorldPos(x, y, z)`. If `!chunkBB.isInside(pos)`, return immediately.
  2. If `!canBeReplaced(level, x, y, z, chunkBB)` (default `true`; overridden by `MineShaftPiece`), return.
  3. Apply `blockState = blockState.mirror(this.mirror)` if `mirror != NONE`, then `blockState = blockState.rotate(this.rotation)` if `rotation != NONE` (exact order: **mirror first, then rotate**).
  4. `level.setBlock(pos, blockState, 2)`.
  5. If `!level.getFluidState(pos).isEmpty()`: `level.scheduleTick(pos, fluidState.getType(), 0)`.
  6. If `blockState.getBlock()` is in `SHAPE_CHECK_BLOCKS` = `{nether_brick_fence, torch, wall_torch, oak_fence, spruce_fence, dark_oak_fence, pale_oak_fence, acacia_fence, birch_fence, jungle_fence, ladder, iron_bars}`: mark `pos` on its chunk for post-processing (`markPosForPostProcessing`).
- **`getBlock(level, x, y, z, chunkBB)`**: computes `pos = getWorldPos(x, y, z)`; if `!chunkBB.isInside(pos)` returns `minecraft:air` (default state), else `level.getBlockState(pos)`.
- **`isInterior(level, x, y, z, chunkBB)`**: computes `pos = getWorldPos(x, y + 1, z)`; returns `false` if `!chunkBB.isInside(pos)`, else `pos.y < level.getHeight(OCEAN_FLOOR_WG, pos.x, pos.z)`.
- **`isReplaceableByStructures(state)`**: `state.isAir() || state.liquid() || state.is(glow_lichen) || state.is(seagrass) || state.is(tall_seagrass)`.
- **`fillColumnDown(level, state, x, startY, z, chunkBB)`**: computes `pos = getWorldPos(x, startY, z)`. If `chunkBB.isInside(pos)`: while `isReplaceableByStructures(level.getBlockState(pos)) && pos.y > level.getMinY() + 1`, call `level.setBlock(pos, state, 2)` (note: bypasses `placeBlock`, so no mirror/rotate) and decrement `pos.y`.

#### 1.5 `StructurePiece` volume, selector, and container helpers
All box helpers iterate in exact triple-loop order **`y = y0..=y1` (outer), `x = x0..=x1` (middle), `z = z0..=z1` (inner)**. A cell is an *edge* iff `y == y0 || y == y1 || x == x0 || x == x1 || z == z0 || z == z1`.
- **`generateAirBox(level, chunkBB, x0, y0, z0, x1, y1, z1)`**: calls `placeBlock(..., minecraft:air, x, y, z, chunkBB)` for every cell.
- **`generateBox(..., x0..z1, edgeBlock, fillBlock, skipAir)`**: for each cell, if `!skipAir || !getBlock(level, x, y, z, chunkBB).isAir()`, places `edgeBlock` on edges and `fillBlock` strictly inside.
- **`generateBox(..., x0..z1, skipAir, random, selector)`**: for each cell, if `!skipAir || !getBlock(level, x, y, z, chunkBB).isAir()`, calls `selector.next(random, x, y, z, isEdge)` and places `selector.getNext()`.
  - **RNG parity rule**: Because `getBlock` returns `air` outside `chunkBB`, when `skipAir == true`, `selector.next` is **skipped** for cells outside `chunkBB` (and for air cells inside `chunkBB`). When `skipAir == false`, `selector.next` is **always called** for every cell in `[x0..x1]×[y0..y1]×[z0..z1]` regardless of `chunkBB`.
- **`generateMaybeBox(level, chunkBB, random, prob, x0..z1, edgeBlock, fillBlock, skipAir, hasToBeInside)`**: for each cell:
  1. Draw `random.nextFloat()`; skip cell if `> prob` (strict `>`, so `<= prob` passes). **Always consumes 1 `nextFloat()` per cell in the box regardless of `chunkBB`!**
  2. If `skipAir` and `getBlock(x, y, z).isAir()`, skip.
  3. If `hasToBeInside` and `!isInterior(x, y, z)`, skip.
  4. Place `edgeBlock` on edges, `fillBlock` strictly inside.
- **`maybeGenerateBlock(level, chunkBB, random, prob, x, y, z, state)`**: draws `random.nextFloat()`; if `< prob` (strict `<`), calls `placeBlock`. **Always consumes 1 `nextFloat()` regardless of `chunkBB`.**
- **`generateUpperHalfSphere(level, chunkBB, x0..z1, fillBlock, skipAir)`**:
  - `dx = f32(x1 - x0 + 1)`, `dy = f32(y1 - y0 + 1)`, `dz = f32(z1 - z0 + 1)`.
  - `cx = f32(x0) + dx / 2.0f`, `cz = f32(z0) + dz / 2.0f`.
  - Loop `y = y0..=y1`, `x = x0..=x1`, `z = z0..=z1`:
    - `ny = f32(y - y0) / dy`, `nx = (f32(x) - cx) / (dx * 0.5f)`, `nz = (f32(z) - cz) / (dz * 0.5f)`.
    - If `!skipAir || !getBlock(x, y, z).isAir()`, compute `d = (nx*nx + ny*ny) + nz*nz` in `f32`; if `d <= 1.05f`, place `fillBlock`.
- **`reorient(level, pos, blockState)`** (auto-facing for placed chests):
  1. Iterate `dir` in `Direction.Plane.HORIZONTAL` (`NORTH, EAST, SOUTH, WEST`): read `neighbor = level.getBlockState(pos.relative(dir))`. If `neighbor` is `minecraft:chest`, return `blockState` unchanged. If `neighbor.isSolidRender()`, record `solidNeighbor = dir` if first solid neighbor seen, else clear `solidNeighbor = null` and break.
  2. If `solidNeighbor != null`: return `blockState` with `facing = solidNeighbor.getOpposite()`.
  3. Else let `lockDir = blockState.facing` (defaults to `NORTH`). Test `pos.relative(lockDir).isSolidRender()`:
     - If solid: `lockDir = lockDir.getOpposite()`.
     - If new `pos.relative(lockDir)` is solid: `lockDir = lockDir.getClockWise()`.
     - If new `pos.relative(lockDir)` is solid: `lockDir = lockDir.getOpposite()`.
     Return `blockState` with `facing = lockDir`.
- **`createChest(level, chunkBB, random, pos, lootTable, blockState)`**:
  - Only runs if `chunkBB.isInside(pos) && !level.getBlockState(pos).is(minecraft:chest)`.
  - If `blockState == null`, computes `blockState = reorient(level, pos, minecraft:chest[facing=north])`.
  - Places `level.setBlock(pos, blockState, 2)` (note: no piece mirror/rotate applied when called directly!) and sets `chestBlockEntity.setLootTable(lootTable, random.nextLong())`. Returns `true` if placed, `false` otherwise (consuming no RNG when `false`).
  - The local-coordinate overload calls `createChest(..., getWorldPos(x, y, z), lootTable, null)`.
- **`createDispenser(level, chunkBB, random, x, y, z, facing, lootTable)`**:
  - Computes `pos = getWorldPos(x, y, z)`. If `chunkBB.isInside(pos) && !level.getBlockState(pos).is(minecraft:dispenser)`: calls `placeBlock(level, minecraft:dispenser[facing=facing], x, y, z, chunkBB)` (which DOES apply piece mirror/rotate), sets `dispenserBlockEntity.setLootTable(lootTable, random.nextLong())`, and returns `true`; else `false`.

#### 1.6 `ScatteredFeaturePiece` height snapping
Stores `(width, height, depth)` and mutable `heightPosition` (initially `-1`, serialized as `"HPos"`). Constructor sets `boundingBox = makeBoundingBox(west, floor, north, direction, width, height, depth)` and `setOrientation(direction)`.
- **`updateAverageGroundHeight(level, chunkBB, offset)`**:
  - If `heightPosition >= 0`: return `true`.
  - Loop `z = bb.minZ..=bb.maxZ` (outer), `x = bb.minX..=bb.maxX` (inner): test `chunkBB.isInside(x, 64, z)` (note hardcoded Y = `64`!). If inside, add `level.getHeight(MOTION_BLOCKING_NO_LEAVES, x, z)` to `total` and increment `count`.
  - If `count == 0`, return `false`. Else `heightPosition = total / count` (truncating int division), call `bb.move(0, heightPosition - bb.minY + offset, 0)`, return `true`.
- **`updateHeightPositionToLowestGroundHeight(level, offset)`**:
  - If `heightPosition >= 0`: return `true`.
  - Loop `z = bb.minZ..=bb.maxZ`, `x = bb.minX..=bb.maxX`: take `lowest = min(lowest, level.getHeight(MOTION_BLOCKING_NO_LEAVES, x, z))` across the **entire** piece footprint (NOT restricted to `chunkBB`!).
  - Set `heightPosition = lowest`, call `bb.move(0, heightPosition - bb.minY + offset, 0)`, return `true`.

#### 1.7 `TemplateStructurePiece` & template transform math
Full `StructureTemplate.placeInWorld` and processor execution are specified in `structures-placement-and-jigsaw.md`. The geometric transform primitives used by legacy template pieces are:
- **`StructureTemplate.transform(pos, mirror, rotation, pivot)`**:
  1. Mirror: `LEFT_RIGHT` maps `z = -z`; `FRONT_BACK` maps `x = -x`; `NONE` leaves `(x, z)`.
  2. Rotate around `(px, pz) = (pivot.x, pivot.z)`:
     - `NONE`: `(x, y, z)`
     - `CLOCKWISE_90`: `(px + pz - z, y, pz - px + x)`
     - `CLOCKWISE_180`: `(2*px - x, y, 2*pz - z)`
     - `COUNTERCLOCKWISE_90`: `(px - pz + z, y, px + pz - x)`
- **`calculateRelativePosition(settings, pos)`**: `transform(pos, settings.mirror, settings.rotation, settings.rotationPivot)`.
- **`calculateConnectedPosition(settings1, conn1, settings2, conn2)`**: `calculateRelativePosition(settings1, conn1) - calculateRelativePosition(settings2, conn2)`.
- **`getZeroPositionWithTransform(zeroPos, mirror, rotation, sizeX, sizeZ)`**: let `sx = sizeX - 1`, `sz = sizeZ - 1`, `mx = (mirror == FRONT_BACK) ? sx : 0`, `mz = (mirror == LEFT_RIGHT) ? sz : 0`:
  - `NONE`: `zeroPos + (mx, 0, mz)`
  - `CLOCKWISE_90`: `zeroPos + (sz - mz, 0, mx)`
  - `CLOCKWISE_180`: `zeroPos + (sx - mx, 0, sz - mz)`
  - `COUNTERCLOCKWISE_90`: `zeroPos + (mz, 0, sx - mx)`
- **`template.getBoundingBox(position, rotation, pivot, mirror)`**: let `d = size - (1, 1, 1)`, `c1 = transform((0,0,0), mirror, rotation, pivot)`, `c2 = transform(d, mirror, rotation, pivot)`, return `BoundingBox.fromCorners(c1, c2).moved(position)`.
- **`TemplateStructurePiece.postProcess`**:
  1. Sets `placeSettings.boundingBox = chunkBB` and recomputes `this.boundingBox = template.getBoundingBox(placeSettings, templatePosition)`.
  2. Calls `template.placeInWorld(level, templatePosition, referencePos, placeSettings, random, 2)`.
  3. If `placeInWorld` returns `true`:
     - Iterates all `minecraft:structure_block` entries in the template filtered by `placeSettings` (which restricts to `chunkBB`); for each with NBT `mode == "DATA"`, calls `handleDataMarker(metadata, blockPos, level, random, chunkBB)`.
     - Iterates all `minecraft:jigsaw` entries in the template filtered by `placeSettings`; parses `final_state` (default `"minecraft:air"`) and calls `level.setBlockAndUpdate(pos, targetState)`.
- **`TemplateStructurePiece.move(dx, dy, dz)`**: shifts both `boundingBox` and `templatePosition` by `(dx, dy, dz)`.

#### 1.8 `StructureStart.placeInChunk`
Given non-empty `pieces`:
1. `centerBB = pieces[0].boundingBox`; `c = centerBB.getCenter()`; `referencePos = BlockPos(c.x, centerBB.minY, c.z)`.
2. For each `piece` in `pieces` in list order: if `piece.boundingBox.intersects(chunkBB)`, call `piece.postProcess(level, structureManager, generator, random, chunkBB, chunkPos, referencePos)`.
3. Call `structure.afterPlace(level, structureManager, generator, random, chunkBB, chunkPos, pieceContainer)` (overridden only by `DesertPyramidStructure` and `WoodlandMansionStructure`).

---

### 2. Mineshaft (`minecraft:mineshaft`)

#### 2.1 Configuration schema
- `StructureSettings` fields, plus:
- `mineshaft_type`: `"normal"` | `"mesa"` (required).
  - `normal`: `wood = minecraft:oak_log`, `planks = minecraft:oak_planks`, `fence = minecraft:oak_fence`.
  - `mesa`: `wood = minecraft:dark_oak_log`, `planks = minecraft:dark_oak_planks`, `fence = minecraft:dark_oak_fence`.

#### 2.2 Start generation and vertical positioning
In `findGenerationPoint(context)`:
1. **Discard one double**: calls `context.random().nextDouble()` (legacy relic; result unused, MUST still advance RNG).
2. Let `startPos = (chunk.middleBlockX, 50, chunk.minBlockZ)` *(note: X is `middleBlockX`, Z is `minBlockZ`!)*.
3. Construct `MineShaftRoom(genDepth = 0, random, chunk.blockX(2), chunk.blockZ(2), type)`, add it to a local `StructurePiecesBuilder`, and call `room.addChildren(room, builder, random)` (depth-first recursion).
4. Shift all pieces vertically:
   - **`mesa`**: let `center = builder.getBoundingBox().getCenter()`; `surfaceY = chunkGenerator.getBaseHeight(center.x, center.z, WORLD_SURFACE_WG, ...)`. `targetY = (surfaceY <= seaLevel) ? seaLevel : Mth.randomBetweenInclusive(random, seaLevel, surfaceY)`. `dy = targetY - center.y`; call `builder.offsetPiecesVertically(dy)`.
   - **`normal`**: `dy = builder.moveBelowSeaLevel(seaLevel, chunkGenerator.getMinY(), random, 10)`.
5. Return `GenerationStub(startPos.offset(0, dy, 0), prebuiltBuilder)`. (Biome validation then checks the shifted `startPos`.)

#### 2.3 Piece-generation recursion
Shared `generateAndAddPiece(startPiece, builder, random, footX, footY, footZ, dir, depth)`:
1. Reject (`null`) if `depth > 8` or `|footX - startPiece.bb.minX| > 80` or `|footZ - startPiece.bb.minZ| > 80`.
2. Draw `sel = random.nextInt(100)`:
   - `sel >= 80` (20%): try `MineShaftCrossing.findCrossing` → `MineShaftCrossing(depth + 1, box, dir, type)`.
   - `sel >= 70` (10%): try `MineShaftStairs.findStairs` → `MineShaftStairs(depth + 1, box, dir, type)`.
   - `sel < 70` (70%): try `MineShaftCorridor.findCorridorSize` → `MineShaftCorridor(depth + 1, random, box, dir, type)`.
3. If non-null: `builder.addPiece(newPiece)` and **immediately** recurse `newPiece.addChildren(startPiece, builder, random)` (pure DFS, not BFS!).

#### 2.4 Shared `MineShaftPiece` helpers
- **`canBeReplaced(level, x, y, z, chunkBB)`**: returns `false` if the existing block at `(x, y, z)` is `type.planks`, `type.wood`, `type.fence`, or `minecraft:iron_chain` (protects existing mineshaft timber/chains from being carved out by overlapping pieces).
- **`isSupportingBox(level, chunkBB, x0, x1, y1, z0)`**: returns `true` iff for every `x ∈ [x0..x1]`, `!getBlock(level, x, y1 + 1, z0, chunkBB).isAir()`.
- **`isInInvalidLocation(level, chunkBB)`**:
  1. Clamped expanded box: `x0 = max(bb.minX - 1, chunkBB.minX)`, `y0 = max(bb.minY - 1, chunkBB.minY)`, `z0 = max(bb.minZ - 1, chunkBB.minZ)`, `x1 = min(bb.maxX + 1, chunkBB.maxX)`, `y1 = min(bb.maxY + 1, chunkBB.maxY)`, `z1 = min(bb.maxZ + 1, chunkBB.maxZ)`.
  2. Test biome at `((x0 + x1) / 2, (y0 + y1) / 2, (z0 + z1) / 2)` (truncating int div); if in `#minecraft:mineshaft_blocking`, return `true`.
  3. Test all 6 faces of `[x0..x1] × [y0..y1] × [z0..z1]` for any block with `state.liquid() == true`:
     - `x = x0..x1, z = z0..z1`: check `y0` and `y1`.
     - `x = x0..x1, y = y0..y1`: check `z0` and `z1`.
     - `z = z0..z1, y = y0..y1`: check `x0` and `x1`.
     If any face block is liquid, return `true`; else `false`.
- **`setPlanksBlock(level, chunkBB, planks, x, y, z)`**: if `isInterior(level, x, y, z, chunkBB)`, read `pos = getWorldPos(x, y, z)`; if `!level.getBlockState(pos).isFaceSturdy(level, pos, Direction.UP)`, call `level.setBlock(pos, planks, 2)`.

#### 2.5 Mineshaft pieces (4 types)

##### A. `MineShaftRoom` (`msroom`, `orientation = null` — world-space coords)
- **Construction**: RNG draws in exact order:
  - `xSpanExtra = random.nextInt(6)`, `ySpanExtra = random.nextInt(6)`, `zSpanExtra = random.nextInt(6)`.
  - `boundingBox = [west, 50, north] .. [west + 7 + xSpanExtra, 54 + ySpanExtra, north + 7 + zSpanExtra]`.
- **`addChildren`**:
  - `heightSpace = max(1, bb.getYSpan() - 4)`.
  - Runs 4 loops in exact order **NORTH, SOUTH, WEST, EAST**:
    1. **NORTH**: `pos = 0`; while `pos < bb.getXSpan()`: `pos += random.nextInt(bb.getXSpan())`; if `pos + 3 > bb.getXSpan()` break; `child = generateAndAddPiece(..., bb.minX + pos, bb.minY + random.nextInt(heightSpace) + 1, bb.minZ - 1, NORTH, depth)`; if `child != null`, record entrance box `[child.minX, child.minY, bb.minZ] .. [child.maxX, child.maxY, bb.minZ + 1]`; `pos += 4`.
    2. **SOUTH**: identical over `bb.getXSpan()`, attaching at `bb.maxZ + 1` facing `SOUTH`; entrance box `[child.minX, child.minY, bb.maxZ - 1] .. [child.maxX, child.maxY, bb.maxZ]`.
    3. **WEST**: identical over `bb.getZSpan()`, attaching at `(bb.minX - 1, bb.minY + random.nextInt(heightSpace) + 1, bb.minZ + pos)` facing `WEST`; entrance box `[bb.minX, child.minY, child.minZ] .. [bb.minX + 1, child.maxY, child.maxZ]`.
    4. **EAST**: identical over `bb.getZSpan()`, attaching at `bb.maxX + 1` facing `EAST`; entrance box `[bb.maxX - 1, child.minY, child.minZ] .. [bb.maxX, child.maxY, child.maxZ]`.
  - `move(dx, dy, dz)` also moves every stored entrance box.
- **`postProcess`**: if `isInInvalidLocation`, return.
  1. `generateBox(bb.minX, bb.minY + 1, bb.minZ, bb.maxX, min(bb.minY + 3, bb.maxY), bb.maxZ, cave_air, cave_air, false)`.
  2. For each stored entrance box `eb`: `generateBox(eb.minX, eb.maxY - 2, eb.minZ, eb.maxX, eb.maxY, eb.maxZ, cave_air, cave_air, false)`.
  3. `generateUpperHalfSphere(bb.minX, bb.minY + 4, bb.minZ, bb.maxX, bb.maxY, bb.maxZ, cave_air, false)`.

##### B. `MineShaftCorridor` (`mscorridor`, oriented)
- **`findCorridorSize`**: draws `corridorLength = random.nextInt(3) + 2` (`2..4`). Loops `corridorLength` down to `1`: `blockLength = corridorLength * 5`; creates relative box `NORTH:[0,0,-(L-1), 2,2,0]`, `SOUTH:[0,0,0, 2,2,L-1]`, `WEST:[-(L-1),0,0, 0,2,2]`, `EAST:[0,0,0, L-1,2,2]`, moves by `(footX, footY, footZ)`; returns first non-colliding box, or `null`.
- **Construction**: `hasRails = (random.nextInt(3) == 0)`; `spiderCorridor = !hasRails && (random.nextInt(23) == 0)` (short-circuit: `nextInt(23)` not called if `hasRails`); `numSections = (axis == Z ? bb.getZSpan() : bb.getXSpan()) / 5`.
- **`addChildren`**:
  1. Draw `endSelection = random.nextInt(4)`. Let `y = bb.minY - 1 + random.nextInt(3)`. Attach one end piece at `depth` (NOT `depth + 1` — `generateAndAddPiece` increments to `depth + 1`):
     - `NORTH`: `<=1` → `(minX, y, minZ - 1, NORTH)`; `==2` → `(minX - 1, y, minZ, WEST)`; `==3` → `(maxX + 1, y, minZ, EAST)`.
     - `SOUTH`: `<=1` → `(minX, y, maxZ + 1, SOUTH)`; `==2` → `(minX - 1, y, maxZ - 3, WEST)`; `==3` → `(maxX + 1, y, maxZ - 3, EAST)`.
     - `WEST`: `<=1` → `(minX - 1, y, minZ, WEST)`; `==2` → `(minX, y, minZ - 1, NORTH)`; `==3` → `(minX, y, maxZ + 1, SOUTH)`.
     - `EAST`: `<=1` → `(maxX + 1, y, minZ, EAST)`; `==2` → `(maxX - 3, y, minZ - 1, NORTH)`; `==3` → `(maxX - 3, y, maxZ + 1, SOUTH)`.
  2. Side branches (only if `depth < 8`, passing `depth + 1` to `generateAndAddPiece` so children get `genDepth = depth + 2`):
     - If `EAST` or `WEST`: for `x = bb.minX + 3` while `x + 3 <= bb.maxX` step `+5`: `sel = random.nextInt(5)`; `0` → `(x, minY, minZ - 1, NORTH)`, `1` → `(x, minY, maxZ + 1, SOUTH)`.
     - If `NORTH` or `SOUTH`: for `z = bb.minZ + 3` while `z + 3 <= bb.maxZ` step `+5`: `sel = random.nextInt(5)`; `0` → `(minX - 1, minY, z, WEST)`, `1` → `(maxX + 1, minY, z, EAST)`.
- **`createChest` override** (places a **chest minecart** on a rail, not a chest block):
  - Checks `chunkBB.isInside(pos) && level.getBlockState(pos).isAir() && !level.getBlockState(pos.below()).isAir()`.
  - If true: draws `random.nextBoolean()` (`true` → `rail[shape=north_south]`, `false` → `rail[shape=east_west]`), calls `placeBlock`, creates `minecraft:chest_minecart` at `(pos.x + 0.5, pos.y + 0.5, pos.z + 0.5)`, calls `setLootTable(lootTable, random.nextLong())`, spawns entity, returns `true`.
- **`postProcess`**: if `isInInvalidLocation`, return. Let `length = numSections * 5 - 1`.
  1. `generateBox(0, 0, 0, 2, 1, length, cave_air, cave_air, false)`.
  2. `generateMaybeBox(random, 0.8f, 0, 2, 0, 2, 2, length, cave_air, cave_air, false, false)`.
  3. If `spiderCorridor`: `generateMaybeBox(random, 0.6f, 0, 0, 0, 2, 1, length, cobweb, cave_air, false, true)`.
  4. For `section = 0 .. numSections - 1` (`z = 2 + section * 5`):
     - **Support beam** (`placeSupport(0, 0, z, 2, 2, random)`): if `isSupportingBox(0, 2, 2, z)`:
       - `generateBox(0, 0, z, 0, 1, z, fence[west=true], cave_air, false)`.
       - `generateBox(2, 0, z, 2, 1, z, fence[east=true], cave_air, false)`.
       - If `random.nextInt(4) == 0`: place `planks` at `(0, 2, z)` and `(2, 2, z)` (damaged beam).
       - Else: `generateBox(0, 2, z, 2, 2, z, planks, cave_air, false)`; `maybeGenerateBlock(0.05f, 1, 2, z - 1, wall_torch[facing=south])`; `maybeGenerateBlock(0.05f, 1, 2, z + 1, wall_torch[facing=north])`.
     - **Corner cobwebs** (`maybePlaceCobWeb(prob, x, y, z)`: short-circuits `isInterior(x, y, z) && random.nextFloat() < prob && hasSturdyNeighbours(x, y, z, 2)` — checks 6 adjacent `Direction.values()` inside `chunkBB` for `isFaceSturdy(..., dir.opposite)` >= 2):
       - `0.1f` at `(0,2,z-1)`, `(2,2,z-1)`, `(0,2,z+1)`, `(2,2,z+1)`.
       - `0.05f` at `(0,2,z-2)`, `(2,2,z-2)`, `(0,2,z+2)`, `(2,2,z+2)`.
     - **Minecart chests**:
       - If `random.nextInt(100) == 0`: `createChest(2, 0, z - 1, minecraft:chests/abandoned_mineshaft)`.
       - If `random.nextInt(100) == 0`: `createChest(0, 0, z + 1, minecraft:chests/abandoned_mineshaft)`.
     - **Cave spider spawner**: if `spiderCorridor && !hasPlacedSpider`:
       - `newZ = z - 1 + random.nextInt(3)` (unconditionally drawn while `!hasPlacedSpider`!).
       - `pos = getWorldPos(1, 0, newZ)`. If `chunkBB.isInside(pos) && isInterior(1, 0, newZ)`: set `hasPlacedSpider = true`, `level.setBlock(pos, minecraft:spawner, 2)`, `spawner.setEntityId(minecraft:cave_spider, random)`.
  5. **Bridge planks under floor**: loop `x = 0..2`, `z = 0..length`: `setPlanksBlock(planks, x, -1, z)`.
  6. **Wood pillars down / iron chains up**: call `placeDoubleLowerOrUpperSupport(0, -1, 2)`, and if `numSections > 1`, also at `(0, -1, length - 2)`.
     - For `px ∈ {x, x + 2}`: if `getBlock(px, y, z)` is `type.planks`: run `fillPillarDownOrChainUp(type.wood, px, y, z)`:
       - Only if `pos = getWorldPos(px, y, z)` is inside `chunkBB`. Let `worldY = pos.y`.
       - Loop `dist = 1, 2, ...` while `checkBelow || checkAbove` (both initially `true`):
         - If `checkBelow`: inspect `below = level.getBlockState(x, worldY - dist, z)`. `emptyBelow = isReplaceableByStructures(below) && !below.is(lava)`. If `!emptyBelow && below.isFaceSturdy(..., UP)`: fill `type.wood` at `y = worldY - dist + 1 .. worldY - 1` inclusive, then return. Update `checkBelow = (dist <= 20) && emptyBelow && (worldY - dist > level.minY + 1)`.
         - If `checkAbove`: inspect `above = level.getBlockState(x, worldY + dist, z)`. `emptyAbove = isReplaceableByStructures(above)`. If `!emptyAbove && Block.canSupportCenter(..., DOWN) && !(above.block instanceof FallingBlock)`: place `type.fence` at `worldY + 1` and `minecraft:iron_chain` at `y = worldY + 2 .. worldY + dist - 1` inclusive, then return. Update `checkAbove = (dist <= 50) && emptyAbove && (worldY + dist < level.maxY)`.
  7. **Rails**: if `hasRails`: for `z = 0..length`: `floor = getBlock(1, -1, z)`; if `!floor.isAir() && floor.isSolidRender()`: `prob = isInterior(1, 0, z) ? 0.7f : 0.9f`; `maybeGenerateBlock(prob, 1, 0, z, minecraft:rail[shape=north_south])`.

##### C. `MineShaftCrossing` (`mscrossing`, `orientation = null` — world-space coords)
Stores its own `direction` field (`"D"` in NBT) and `isTwoFloored` (`"tf"`, true iff `bb.getYSpan() > 3`).
- **`findCrossing`**: `y1 = (random.nextInt(4) == 0) ? 6 : 2`. Relative box: `NORTH:[-1,0,-4, 3,y1,0]`, `SOUTH:[-1,0,0, 3,y1,4]`, `WEST:[-4,0,-1, 0,y1,3]`, `EAST:[0,0,-1, 4,y1,3]`; move by `(footX, footY, footZ)`; return `box` if no collision, else `null`.
- **`addChildren`**:
  1. Lower floor (always 3 exits, excluding entry side, at `y = bb.minY` and `depth`):
     - `NORTH`: `(minX+1, minY, minZ-1, NORTH)`, `(minX-1, minY, minZ+1, WEST)`, `(maxX+1, minY, minZ+1, EAST)`.
     - `SOUTH`: `(minX+1, minY, maxZ+1, SOUTH)`, `(minX-1, minY, minZ+1, WEST)`, `(maxX+1, minY, minZ+1, EAST)`.
     - `WEST`: `(minX+1, minY, minZ-1, NORTH)`, `(minX+1, minY, maxZ+1, SOUTH)`, `(minX-1, minY, minZ+1, WEST)`.
     - `EAST`: `(minX+1, minY, minZ-1, NORTH)`, `(minX+1, minY, maxZ+1, SOUTH)`, `(maxX+1, minY, minZ+1, EAST)`.
  2. Upper floor (only if `isTwoFloored`, at `y = bb.minY + 4`, 4 independent `random.nextBoolean()` checks in exact order **NORTH, WEST, EAST, SOUTH**):
     - `NORTH`: `(minX+1, minY+4, minZ-1, NORTH)`
     - `WEST`: `(minX-1, minY+4, minZ+1, WEST)`
     - `EAST`: `(maxX+1, minY+4, minZ+1, EAST)`
     - `SOUTH`: `(minX+1, minY+4, maxZ+1, SOUTH)`
- **`postProcess`**: if `isInInvalidLocation`, return. All boxes use world coords:
  1. If `isTwoFloored`: carve 5 `cave_air` boxes:
     - `[minX+1, minY, minZ]..[maxX-1, minY+2, maxZ]`
     - `[minX, minY, minZ+1]..[maxX, minY+2, maxZ-1]`
     - `[minX+1, maxY-2, minZ]..[maxX-1, maxY, maxZ]`
     - `[minX, maxY-2, minZ+1]..[maxX, maxY, maxZ-1]`
     - `[minX+1, minY+3, minZ+1]..[maxX-1, minY+3, maxZ-1]`
     Else (single floor): carve 2 `cave_air` boxes:
     - `[minX+1, minY, minZ]..[maxX-1, maxY, maxZ]`
     - `[minX, minY, minZ+1]..[maxX, maxY, maxZ-1]`
  2. Four corner support pillars at `(minX+1, minZ+1)`, `(minX+1, maxZ-1)`, `(maxX-1, minZ+1)`, `(maxX-1, maxZ-1)`: if `!getBlock(px, maxY + 1, pz).isAir()`, fill `[px, minY, pz]..[px, maxY, pz]` with `type.planks`.
  3. Floor planks: for `x = minX..maxX`, `z = minZ..maxZ`: `setPlanksBlock(type.planks, x, minY - 1, z)`.

##### D. `MineShaftStairs` (`msstairs`, oriented)
- **`findStairs`**: relative box `NORTH:[0,-5,-8, 2,2,0]`, `SOUTH:[0,-5,0, 2,2,8]`, `WEST:[-8,-5,0, 0,2,2]`, `EAST:[0,-5,0, 8,2,2]`; move by `(footX, footY, footZ)`; return `box` if no collision, else `null`.
- **`addChildren`**: attaches 1 child at the bottom end (`y = bb.minY`, same `orientation`, `depth`):
  - `NORTH`: `(minX, minY, minZ - 1)`, `SOUTH`: `(minX, minY, maxZ + 1)`, `WEST`: `(minX - 1, minY, minZ)`, `EAST`: `(maxX + 1, minY, minZ)`.
- **`postProcess`**: if `isInInvalidLocation`, return.
  1. `generateBox(0, 5, 0, 2, 7, 1, cave_air, cave_air, false)`.
  2. `generateBox(0, 0, 7, 2, 2, 8, cave_air, cave_air, false)`.
  3. For `i = 0..4`: `generateBox(0, 5 - i - (i < 4 ? 1 : 0), 2 + i, 2, 7 - i, 2 + i, cave_air, cave_air, false)`.

---
### 3. Stronghold (`minecraft:stronghold`)

#### 3.1 Configuration schema & terrain adaptation
- Schema: `StructureSettings` only (no extra fields).
- Vanilla defaults: `step = surface_structures`, `terrain_adaptation = bury`, no `spawn_overrides`.

#### 3.2 Start generation and retry loop
In `findGenerationPoint(context)`: returns `GenerationStub(chunkPos.worldPosition, generator)` where `worldPosition = (chunkMinX, 0, chunkMinZ)` (note biome check runs at Y = 0!).
Inside `generatePieces(builder, context)`:
1. Initialize `tries = 0`.
2. Repeat (`do ... while`):
   1. `builder.clear()`.
   2. Reseed `context.random()` via `setLargeFeatureSeed(context.seed + tries, chunkX, chunkZ)` and increment `tries`.
   3. Reset piece pool: restore all 11 entries of `STRONGHOLD_PIECE_WEIGHTS` with `placeCount = 0`, and set `imposedPiece = null`.
   4. Construct `StartPiece(random, chunk.blockX(2), chunk.blockZ(2))` (draws `getRandomHorizontalDirection(random)`). Add to `builder`.
   5. Call `startRoom.addChildren(startRoom, builder, random)`.
   6. While `!startRoom.pendingChildren.isEmpty()`:
      - Draw `idx = random.nextInt(pendingChildren.size())`.
      - Remove and pop `piece = pendingChildren.remove(idx)`.
      - Call `piece.addChildren(startRoom, builder, random)`.
   7. Call `builder.moveBelowSeaLevel(seaLevel, chunkGenerator.minY, random, 10)`.
3. Loop while `builder.isEmpty() || startRoom.portalRoomPiece == null`.

#### 3.3 Piece weights, selection, and recursion
| Order | Piece class | Weight | `maxPlaceCount` (`0` = unlimited) | Extra `doPlace(depth)` guard |
|---|---|---|---|---|
| 0 | `Straight` | 40 | 0 | — |
| 1 | `PrisonHall` | 5 | 5 | — |
| 2 | `LeftTurn` | 20 | 0 | — |
| 3 | `RightTurn` | 20 | 0 | — |
| 4 | `RoomCrossing` | 10 | 6 | — |
| 5 | `StraightStairsDown` | 5 | 5 | — |
| 6 | `StairsDown` | 5 | 5 | — |
| 7 | `FiveCrossing` | 5 | 4 | — |
| 8 | `ChestCorridor` | 5 | 4 | — |
| 9 | `Library` | 10 | 2 | `depth > 4` |
| 10 | `PortalRoom` | 20 | 1 | `depth > 5` |

- **`isOkBox(box)`**: `box.minY > 10`.
- **`generateAndAddPiece(start, builder, random, footX, footY, footZ, dir, depth)`**:
  - Reject if `depth > 50` or `|footX - start.bb.minX| > 112` or `|footZ - start.bb.minZ| > 112`.
  - Calls `generatePieceFromSmallDoor(..., depth + 1)`. If non-null, appends to both `builder` and `start.pendingChildren`.
- **`generatePieceFromSmallDoor(..., depth)`**:
  1. Recompute `totalWeight = sum(p.weight)` over `currentPieces` and `hasAnyLimited = any(p.maxPlaceCount > 0 && p.placeCount < p.maxPlaceCount)`. If `!hasAnyLimited`, return `null`.
  2. If `imposedPiece != null`: attempt `createPiece` for `imposedPiece` and clear `imposedPiece = null`. If non-null, return it immediately *(does NOT increment `placeCount` or update `start.previousPiece`!)*.
  3. Up to `5` attempts (`numAttempts = 1..5`):
     - Draw `w = random.nextInt(totalWeight)`.
     - Walk `currentPieces` in list order, subtracting `p.weight` until `w < 0`:
       - If `!p.doPlace(depth) || p == start.previousPiece`: break out of the `for` loop (counts as 1 failed attempt).
       - Else call `p`'s `createPiece(...)`. If non-null: `p.placeCount++`, `start.previousPiece = p`, if `p.maxPlaceCount > 0 && p.placeCount >= p.maxPlaceCount` remove `p` from `currentPieces`, and return the piece. *(Note: if `createPiece` returns `null`, the inner `for` loop continues subtracting remaining weights from the already-negative `w`, trying every subsequent eligible piece in `currentPieces` on the same attempt!)*
  4. If all 5 attempts fail: call `FillerCorridor.findPieceBox(builder, random, footX, footY, footZ, dir)`. If non-null and `box.minY > 1`, return `FillerCorridor(depth, box, dir)`; else `null`.

- **Door-child attachment helpers** (all pass `this.genDepth` into `generateAndAddPiece`):
  - `generateSmallDoorChildForward(xOff, yOff)`:
    - `NORTH`: `(minX + xOff, minY + yOff, minZ - 1, NORTH)`
    - `SOUTH`: `(minX + xOff, minY + yOff, maxZ + 1, SOUTH)`
    - `WEST`: `(minX - 1, minY + yOff, minZ + xOff, WEST)`
    - `EAST`: `(maxX + 1, minY + yOff, minZ + xOff, EAST)`
  - `generateSmallDoorChildLeft(yOff, zOff)`:
    - `NORTH` / `SOUTH`: `(minX - 1, minY + yOff, minZ + zOff, WEST)`
    - `WEST` / `EAST`: `(minX + zOff, minY + yOff, minZ - 1, NORTH)`
  - `generateSmallDoorChildRight(yOff, zOff)`:
    - `NORTH` / `SOUTH`: `(maxX + 1, minY + yOff, minZ + zOff, EAST)`
    - `WEST` / `EAST`: `(minX + zOff, minY + yOff, maxZ + 1, SOUTH)`

#### 3.4 Shared `StrongholdPiece` helpers
- **`SmoothStoneSelector`** (`BlockSelector`):
  - If `!isEdge`: `minecraft:cave_air` (NO RNG draw).
  - If `isEdge`: draw `s = random.nextFloat()`:
    - `< 0.2f` → `minecraft:cracked_stone_bricks`
    - `< 0.5f` → `minecraft:mossy_stone_bricks`
    - `< 0.55f` → `minecraft:infested_stone_bricks`
    - else → `minecraft:stone_bricks`
- **`randomSmallDoor(random)`**: draws `random.nextInt(5)`: `0, 1` → `OPENING`, `2` → `WOOD_DOOR`, `3` → `GRATES`, `4` → `IRON_DOOR`.
- **`generateSmallDoor(level, random, chunkBB, doorType, fx, fy, fz)`**:
  - `OPENING`: `generateBox(fx, fy, fz, fx+2, fy+2, fz, cave_air, cave_air, false)`.
  - `WOOD_DOOR`: stone-brick frame at `(fx, fy..fy+2, fz)`, `(fx+1, fy+2, fz)`, `(fx+2, fy..fy+2, fz)`; `oak_door[half=lower]` at `(fx+1, fy, fz)` and `oak_door[half=upper]` at `(fx+1, fy+1, fz)` (both `facing=north`).
  - `GRATES`: `cave_air` at `(fx+1, fy, fz)` and `(fx+1, fy+1, fz)`; `iron_bars[west=true]` at `(fx, fy..fy+1, fz)`; `iron_bars[east=true, west=true]` at `(fx..fx+2, fy+2, fz)`; `iron_bars[east=true]` at `(fx+2, fy..fy+1, fz)`.
  - `IRON_DOOR`: same stone-brick frame as `WOOD_DOOR`; `iron_door[half=lower]` at `(fx+1, fy, fz)`, `iron_door[half=upper]` at `(fx+1, fy+1, fz)`; `stone_button[facing=north]` at `(fx+2, fy+1, fz+1)` and `stone_button[facing=south]` at `(fx+2, fy+1, fz-1)`.

#### 3.5 Stronghold pieces (13 types)

##### A. `StairsDown` (`shsd`) & `StartPiece` (`shstart`)
- **`StartPiece`**: extends `StairsDown` with `isSource = true`, `genDepth = 0`, `makeBoundingBox(west, 64, north, dir, 5, 11, 5)`, `entryDoor = OPENING`. `getLocatorPosition()` returns `portalRoomPiece.getLocatorPosition()` when `portalRoomPiece != null`, else own center.
- **`StairsDown.createPiece`**: `orientBox(footX, footY, footZ, -1, -7, 0, 5, 11, 5, dir)`; checks `isOkBox && noCollision`; sets `isSource = false`, `entryDoor = randomSmallDoor(random)`.
- **`addChildren`**: if `isSource`, sets `StrongholdPieces.imposedPiece = FiveCrossing.class`. Calls `generateSmallDoorChildForward(1, 1)`.
- **`postProcess`**:
  1. `generateBox(0, 0, 0, 4, 10, 4, skipAir=true, random, SMOOTH_STONE_SELECTOR)`.
  2. `generateSmallDoor(entryDoor, 1, 7, 0)` and `generateSmallDoor(OPENING, 1, 1, 4)`.
  3. Spiral staircase blocks (`stone_bricks` = SB, `smooth_stone_slab` = SL):
     - SB: `(2,6,1)`, `(1,5,1)`, `(1,5,2)`, `(1,4,3)`, `(2,4,3)`, `(3,3,3)`, `(3,3,2)`, `(3,2,1)`, `(2,2,1)`, `(1,1,1)`, `(1,1,2)`.
     - SL: `(1,6,1)`, `(1,5,3)`, `(3,4,3)`, `(3,3,1)`, `(1,2,1)`, `(1,1,3)`.

##### B. `Straight` (`shs`)
- **`createPiece`**: `orientBox(-1, -1, 0, 5, 5, 7, dir)`; draws `entryDoor = randomSmallDoor(random)`, `leftChild = (random.nextInt(2) == 0)`, `rightChild = (random.nextInt(2) == 0)`.
- **`addChildren`**: `Forward(1, 1)`; if `leftChild` `Left(1, 2)`; if `rightChild` `Right(1, 2)`.
- **`postProcess`**:
  1. `generateBox(0, 0, 0, 4, 4, 6, true, random, SMOOTH_STONE_SELECTOR)`.
  2. `generateSmallDoor(entryDoor, 1, 1, 0)`; `generateSmallDoor(OPENING, 1, 1, 6)`.
  3. `maybeGenerateBlock(0.1f, ...)` in exact order: `(1,2,1)` `wall_torch[facing=east]`, `(3,2,1)` `wall_torch[facing=west]`, `(1,2,5)` `wall_torch[facing=east]`, `(3,2,5)` `wall_torch[facing=west]`.
  4. If `leftChild`: `generateBox(0, 1, 2, 0, 3, 4, cave_air, cave_air, false)`.
  5. If `rightChild`: `generateBox(4, 1, 2, 4, 3, 4, cave_air, cave_air, false)`.

##### C. `ChestCorridor` (`shcc`)
- **`createPiece`**: `orientBox(-1, -1, 0, 5, 5, 7, dir)`; draws `entryDoor = randomSmallDoor(random)`.
- **`addChildren`**: `Forward(1, 1)`.
- **`postProcess`**:
  1. `generateBox(0, 0, 0, 4, 4, 6, true, random, SMOOTH_STONE_SELECTOR)`.
  2. `generateSmallDoor(entryDoor, 1, 1, 0)`; `generateSmallDoor(OPENING, 1, 1, 6)`.
  3. `generateBox(3, 1, 2, 3, 1, 4, stone_bricks, stone_bricks, false)`.
  4. `stone_brick_slab` at `(3,1,1)`, `(3,1,5)`, `(3,2,2)`, `(3,2,4)`, and `(2,1,2..4)`.
  5. If `!hasPlacedChest && chunkBB.isInside(getWorldPos(3, 2, 3))`: `hasPlacedChest = true`, `createChest(3, 2, 3, minecraft:chests/stronghold_corridor)`.

##### D. `FillerCorridor` (`shfc`)
- **`findPieceBox`**: tests `box = orientBox(-1, -1, 0, 5, 5, 4, dir)` for a colliding piece `c`. If `c == null` or `c.bb.minY != box.minY`, return `null`. Else for `d = 2` down to `1`: let `b = orientBox(-1, -1, 0, 5, 5, d, dir)`; if `!c.bb.intersects(b)`, return `orientBox(-1, -1, 0, 5, 5, d + 1, dir)`. Else `null`.
- **`steps`**: `ZSpan` if `NORTH/SOUTH`, else `XSpan`. No `addChildren`.
- **`postProcess`**: for `i = 0 .. steps - 1`: `stone_bricks` at `(0..4, 0, i)` and `(0..4, 4, i)`; for `y = 1..3`, `stone_bricks` at `(0, y, i)` and `(4, y, i)`, `cave_air` at `(1..3, y, i)`.

##### E. `LeftTurn` (`shlt`) & F. `RightTurn` (`shrt`)
- **`createPiece`**: `orientBox(-1, -1, 0, 5, 5, 5, dir)`; draws `entryDoor = randomSmallDoor(random)`.
- **`addChildren`**:
  - `LeftTurn`: if `orientation ∈ {NORTH, EAST}` → `Left(1, 1)`, else `Right(1, 1)`.
  - `RightTurn`: if `orientation ∈ {NORTH, EAST}` → `Right(1, 1)`, else `Left(1, 1)`.
- **`postProcess`**:
  1. `generateBox(0, 0, 0, 4, 4, 4, true, random, SMOOTH_STONE_SELECTOR)`.
  2. `generateSmallDoor(entryDoor, 1, 1, 0)`.
  3. Carve `cave_air` doorway:
     - `LeftTurn`: `[0,1,1..0,3,3]` if `orientation ∈ {NORTH, EAST}`, else `[4,1,1..4,3,3]`.
     - `RightTurn`: `[4,1,1..4,3,3]` if `orientation ∈ {NORTH, EAST}`, else `[0,1,1..0,3,3]`.

##### G. `PrisonHall` (`shph`)
- **`createPiece`**: `orientBox(-1, -1, 0, 9, 5, 11, dir)`; draws `entryDoor = randomSmallDoor(random)`.
- **`addChildren`**: `Forward(1, 1)`.
- **`postProcess`**:
  1. `generateBox(0, 0, 0, 8, 4, 10, true, random, SMOOTH_STONE_SELECTOR)`.
  2. `generateSmallDoor(entryDoor, 1, 1, 0)`.
  3. `generateBox(1, 1, 10, 3, 3, 10, cave_air, cave_air, false)`.
  4. Four cell dividers with `SMOOTH_STONE_SELECTOR` (`skipAir=false`): `[4,1,1..4,3,1]`, `[4,1,3..4,3,3]`, `[4,1,7..4,3,7]`, `[4,1,9..4,3,9]`.
  5. For `y = 1..3`: `iron_bars[north=true, south=true]` at `(4, y, 4)` and `(4, y, 6)`; `iron_bars[north=true, south=true, east=true]` at `(4, y, 5)`; `iron_bars[west=true, east=true]` at `(5, y, 5)`, `(6, y, 5)`, `(7, y, 5)`.
  6. `iron_bars[north=true, south=true]` at `(4, 3, 2)` and `(4, 3, 8)`.
  7. Two iron doors (`facing=west`): lower at `(4, 1, 2)` & `(4, 1, 8)`, upper at `(4, 2, 2)` & `(4, 2, 8)`.

##### H. `RoomCrossing` (`shrc`)
- **`createPiece`**: `orientBox(-4, -1, 0, 11, 7, 11, dir)`; draws `entryDoor = randomSmallDoor(random)`, then `type = random.nextInt(5)` (`0..4`).
- **`addChildren`**: `Forward(4, 1)`, `Left(1, 4)`, `Right(1, 4)`.
- **`postProcess`**:
  1. `generateBox(0, 0, 0, 10, 6, 10, true, random, SMOOTH_STONE_SELECTOR)`.
  2. `generateSmallDoor(entryDoor, 4, 1, 0)`.
  3. Three `cave_air` openings: `[4,1,10..6,3,10]`, `[0,1,4..0,3,6]`, `[10,1,4..10,3,6]`.
  4. Room interior by `type`:
     - **`0` (torch pillar)**: `stone_bricks` at `(5, 1..3, 5)`; `wall_torch` at `(4,3,5)[west]`, `(6,3,5)[east]`, `(5,3,4)[south]`, `(5,3,6)[north]`; `smooth_stone_slab` at the 8 ring positions `(4..6, 1, 4..6) \ {(5,1,5)}`.
     - **`1` (fountain)**: `stone_bricks` square rim `i = 0..4` at `(3,1,3+i)`, `(7,1,3+i)`, `(3+i,1,3)`, `(3+i,1,7)`, center pillar `(5, 1..3, 5)`, and `minecraft:water` at `(5, 4, 5)`.
     - **`2` (storeroom)**: `cobblestone` border at `y = 3` (`(1,3,1..9)`, `(9,3,1..9)`, `(1..9,3,1)`, `(1..9,3,9)`), center shaft walls `cobblestone` at `(5,1,4)`, `(5,1,6)`, `(5,3,4)`, `(5,3,6)`, `(4,1,5)`, `(6,1,5)`, `(4,3,5)`, `(6,3,5)`, 4 pillars `(4,1..3,4)`, `(6,1..3,4)`, `(4,1..3,6)`, `(6,1..3,6)`; `wall_torch[facing=north]` (default!) at `(5, 3, 5)`; `oak_planks` upper floor at `y = 3` for `z = 2..8`: `x ∈ {2,3,7,8}` always and `x ∈ {4,5,6}` when `z <= 3 || z >= 7`; `ladder[facing=west]` at `(9, 1..3, 3)`; `createChest(3, 4, 8, minecraft:chests/stronghold_crossing)`.
     - **`3` or `4`**: empty room (no interior blocks).

##### I. `StraightStairsDown` (`shssd`)
- **`createPiece`**: `orientBox(-1, -7, 0, 5, 11, 8, dir)`; draws `entryDoor = randomSmallDoor(random)`.
- **`addChildren`**: `Forward(1, 1)`.
- **`postProcess`**:
  1. `generateBox(0, 0, 0, 4, 10, 7, true, random, SMOOTH_STONE_SELECTOR)`.
  2. `generateSmallDoor(entryDoor, 1, 7, 0)`; `generateSmallDoor(OPENING, 1, 1, 7)`.
  3. For `i = 0..5`: `cobblestone_stairs[facing=south]` at `(1..3, 6 - i, 1 + i)`; and if `i < 5`, `stone_bricks` at `(1..3, 5 - i, 1 + i)`.

##### J. `FiveCrossing` (`sh5c`)
- **`createPiece`**: `orientBox(-4, -3, 0, 10, 9, 11, dir)`; draws in order: `entryDoor = randomSmallDoor(random)`, `leftLow = nextBoolean()`, `leftHigh = nextBoolean()`, `rightLow = nextBoolean()`, `rightHigh = (nextInt(3) > 0)`.
- **`addChildren`**: let `zOffA = 3`, `zOffB = 5`; if `orientation ∈ {WEST, NORTH}`, set `zOffA = 5`, `zOffB = 3`. Call `Forward(5, 1)`; if `leftLow` `Left(zOffA, 1)`; if `leftHigh` `Left(zOffB, 7)`; if `rightLow` `Right(zOffA, 1)`; if `rightHigh` `Right(zOffB, 7)`.
- **`postProcess`**:
  1. `generateBox(0, 0, 0, 9, 8, 10, true, random, SMOOTH_STONE_SELECTOR)`.
  2. `generateSmallDoor(entryDoor, 4, 3, 0)`.
  3. Carve `cave_air`: if `leftLow` `[0,3,1..0,5,3]`; if `rightLow` `[9,3,1..9,5,3]`; if `leftHigh` `[0,5,7..0,7,9]`; if `rightHigh` `[9,5,7..9,7,9]`; always `[5,1,10..7,3,10]`.
  4. Stone-brick structures (`skipAir=false`, `SMOOTH_STONE_SELECTOR`) in exact order: `[1,2,1..8,2,6]`, `[4,1,5..4,4,9]`, `[8,1,5..8,4,9]`, `[1,4,7..3,4,9]`, `[1,3,5..3,3,6]`.
  5. `smooth_stone_slab`: `[1,3,4..3,3,4]`, `[1,4,6..3,4,6]`.
  6. `SMOOTH_STONE_SELECTOR` (`skipAir=false`): `[5,1,7..7,1,8]`.
  7. `smooth_stone_slab`: `[5,1,9..7,1,9]`, `[5,2,7..7,2,7]`, `[4,5,7..4,5,9]`, `[8,5,7..8,5,9]`.
  8. `smooth_stone_slab[type=double]`: `[5,5,7..7,5,9]`.
  9. `wall_torch[facing=south]` at `(6, 5, 6)`.

##### K. `Library` (`shli`)
- **`createPiece`**: first tests `orientBox(-4, -1, 0, 14, 11, 15, dir)`; if invalid or colliding, falls back to `orientBox(-4, -1, 0, 14, 6, 15, dir)`. Draws `entryDoor = randomSmallDoor(random)`. `isTall = (bb.getYSpan() > 6)`. No `addChildren`.
- **`postProcess`**: let `H = isTall ? 11 : 6`.
  1. `generateBox(0, 0, 0, 13, H - 1, 14, true, random, SMOOTH_STONE_SELECTOR)`.
  2. `generateSmallDoor(entryDoor, 4, 1, 0)`.
  3. `generateMaybeBox(random, 0.07f, 2, 1, 1, 11, 4, 13, cobweb, cobweb, false, false)`.
  4. Side bookcases/pillars (`d = 1..13`):
     - If `(d - 1) % 4 == 0`: `oak_planks` at `[1,1,d..1,4,d]` and `[12,1,d..12,4,d]`; `wall_torch[east]` at `(2,3,d)`, `wall_torch[west]` at `(11,3,d)`; if `isTall`, `oak_planks` at `[1,6,d..1,9,d]` and `[12,6,d..12,9,d]`.
     - Else: `bookshelf` at `[1,1,d..1,4,d]` and `[12,1,d..12,4,d]`; if `isTall`, `bookshelf` at `[1,6,d..1,9,d]` and `[12,6,d..12,9,d]`.
  5. Freestanding shelves (`d = 3, 5, 7, 9, 11`): `bookshelf` at `[3,1,d..4,3,d]`, `[6,1,d..7,3,d]`, `[9,1,d..10,3,d]`.
  6. If `isTall`:
     - Balcony `oak_planks` at `y = 5`: `[1,5,1..3,5,13]`, `[10,5,1..12,5,13]`, `[4,5,1..9,5,2]`, `[4,5,12..9,5,13]`, `(9,5,11)`, `(8,5,11)`, `(9,5,10)`.
     - Railings (`oak_fence`) at `y = 6`: `[3,6,3..3,6,11]` (`ns`), `[10,6,3..10,6,9]` (`ns`), `[4,6,2..9,6,2]` (`we`), `[4,6,12..7,6,12]` (`we`), `(3,6,2)` (`ne`), `(3,6,12)` (`se`), `(10,6,2)` (`nw`); for `i = 0..2`: `(8+i, 6, 12-i)` (`sw`) and if `i != 2`: `(8+i, 6, 11-i)` (`ne`).
     - `ladder[facing=south]` at `(10, 1..7, 13)`.
     - Chandelier (`oak_fence`): `(6,9,7)` & `(6,8,7)` `[east]`; `(7,9,7)` & `(7,8,7)` `[west]`; `(6,7,7)` & `(7,7,7)` `[nswe]`; `(5,7,7)` `[east]`; `(8,7,7)` `[west]`; `(6,7,6)` `[ne]`; `(6,7,8)` `[se]`; `(7,7,6)` `[nw]`; `(7,7,8)` `[sw]`; `torch` at `(5,8,7)`, `(8,8,7)`, `(6,8,6)`, `(6,8,8)`, `(7,8,6)`, `(7,8,8)`.
  7. Chests: `createChest(3, 3, 5, minecraft:chests/stronghold_library)`; if `isTall`, place `cave_air` at `(12, 9, 1)` and `createChest(12, 8, 1, minecraft:chests/stronghold_library)`.

##### L. `PortalRoom` (`shpr`)
- **`createPiece`**: `orientBox(-4, -1, 0, 11, 8, 16, dir)`; checks `isOkBox && noCollision`. No RNG drawn in constructor.
- **`addChildren`**: sets `startPiece.portalRoomPiece = this`.
- **`postProcess`**:
  1. `generateBox(0, 0, 0, 10, 7, 15, skipAir=false, random, SMOOTH_STONE_SELECTOR)` *(note `skipAir = false`, unlike other stronghold rooms!)*.
  2. `generateSmallDoor(GRATES, 4, 1, 0)`.
  3. Upper ledges (`skipAir=false`, `SMOOTH_STONE_SELECTOR`): `[1,6,1..1,6,14]`, `[9,6,1..9,6,14]`, `[2,6,1..8,6,2]`, `[2,6,14..8,6,14]`.
  4. Side lava pools: `SMOOTH_STONE_SELECTOR` (`skipAir=false`) `[1,1,1..2,1,4]` and `[8,1,1..9,1,4]`; `lava` at `[1,1,1..1,1,3]` and `[9,1,1..9,1,3]`.
  5. Center lava pit: `SMOOTH_STONE_SELECTOR` (`skipAir=false`) `[3,1,8..7,1,12]`; `lava` at `[4,1,9..6,1,11]`.
  6. Wall iron bars: for `z = 3, 5, 7, 9, 11, 13`: `iron_bars[n,s]` at `[0,3,z..0,4,z]` and `[10,3,z..10,4,z]`; for `x = 2, 4, 6, 8`: `iron_bars[w,e]` at `[x,3,15..x,4,15]`.
  7. Stairs platform (`skipAir=false`, `SMOOTH_STONE_SELECTOR`): `[4,1,5..6,1,7]`, `[4,2,6..6,2,7]`, `[4,3,7..6,3,7]`; `stone_brick_stairs[facing=north]` for `x = 4..6` at `(x,1,4)`, `(x,2,5)`, `(x,3,6)`.
  8. **12 End Portal Frames**: draw 12 booleans in order `eyes[i] = (random.nextFloat() > 0.9f)` (`i = 0..11`), tracking `allEyes = AND(eyes)`. Place `end_portal_frame[facing=..., eye=eyes[i]]`:
     - `0..2`: `facing=north` at `(4,3,8)`, `(5,3,8)`, `(6,3,8)`
     - `3..5`: `facing=south` at `(4,3,12)`, `(5,3,12)`, `(6,3,12)`
     - `6..8`: `facing=east` at `(3,3,9)`, `(3,3,10)`, `(3,3,11)`
     - `9..11`: `facing=west` at `(7,3,9)`, `(7,3,10)`, `(7,3,11)`
     - If `allEyes`: place `minecraft:end_portal` in the 3×3 square `(4..6, 3, 9..11)` (row-major z=9,10,11 then x=4,5,6).
  9. **Silverfish spawner**: if `!hasPlacedSpawner` and `pos = getWorldPos(5, 3, 6)` is inside `chunkBB`: set `hasPlacedSpawner = true`, `level.setBlock(pos, minecraft:spawner, 2)` (overwriting the stair at `(5,3,6)`), `spawner.setEntityId(minecraft:silverfish, random)`.

---

### 4. Nether Fortress (`minecraft:fortress`)

#### 4.1 Configuration schema, spawn overrides & terrain adaptation
- Schema: `StructureSettings` only.
- Vanilla defaults: `step = underground_decoration`, `terrain_adaptation = none`, `spawn_overrides = { monster: { bounding_box: "piece", spawns: [blaze wt 10 (2–3), zombified_piglin wt 5 (4), wither_skeleton wt 8 (5), skeleton wt 2 (5), magma_cube wt 3 (4)] } }`.

#### 4.2 Start generation and vertical positioning
In `findGenerationPoint(context)`: returns `GenerationStub((chunkMinX, 64, chunkMinZ), generator)`.
Inside `generatePieces(builder, context)`:
1. Construct `StartPiece(random, chunk.blockX(2), chunk.blockZ(2))` (extends `BridgeCrossing`, draws `getRandomHorizontalDirection(random)`, resets both weight tables to `placeCount = 0`). Add to `builder`.
2. Call `start.addChildren(start, builder, random)`.
3. While `!start.pendingChildren.isEmpty()`: pop `start.pendingChildren.remove(random.nextInt(size))` and call `piece.addChildren(start, builder, random)`.
4. Call `builder.moveInsideHeights(random, 48, 70)`.

#### 4.3 Piece weights, pools, and recursion
Two separate weight tables (`isCastle == false` uses bridge pool; `isCastle == true` uses castle pool):

**Bridge pool (`BRIDGE_PIECE_WEIGHTS`)**:
| Order | Piece class | Weight | `maxPlaceCount` | `allowInRow` |
|---|---|---|---|---|
| 0 | `BridgeStraight` | 30 | 0 | `true` |
| 1 | `BridgeCrossing` | 10 | 4 | `false` |
| 2 | `RoomCrossing` | 10 | 4 | `false` |
| 3 | `StairsRoom` | 10 | 3 | `false` |
| 4 | `MonsterThrone` | 5 | 2 | `false` |
| 5 | `CastleEntrance` | 5 | 1 | `false` |

**Castle pool (`CASTLE_PIECE_WEIGHTS`)**:
| Order | Piece class | Weight | `maxPlaceCount` | `allowInRow` |
|---|---|---|---|---|
| 0 | `CastleSmallCorridorPiece` | 25 | 0 | `true` |
| 1 | `CastleSmallCorridorCrossingPiece` | 15 | 5 | `false` |
| 2 | `CastleSmallCorridorRightTurnPiece` | 5 | 10 | `false` |
| 3 | `CastleSmallCorridorLeftTurnPiece` | 5 | 10 | `false` |
| 4 | `CastleCorridorStairsPiece` | 10 | 3 | `true` |
| 5 | `CastleCorridorTBalconyPiece` | 7 | 2 | `false` |
| 6 | `CastleStalkRoom` | 5 | 2 | `false` |

- **`isOkBox(box)`**: `box.minY > 10`.
- **`generateAndAddPiece(start, builder, random, footX, footY, footZ, dir, depth, isCastle)`**:
  - If `|footX - start.bb.minX| <= 112 && |footZ - start.bb.minZ| <= 112`: calls `generatePiece(..., depth + 1)`; if non-null, adds to `builder` and `start.pendingChildren`.
  - **Otherwise** (out of horizontal bounds): calls `BridgeEndFiller.createPiece(builder, random, footX, footY, footZ, dir, depth)` and returns it **without** adding it to `builder` or `pendingChildren` (still consumes `selfSeed = random.nextInt()` if `createPiece` succeeds!).
- **`generatePiece(start, pool, builder, random, footX, footY, footZ, dir, depth)`**:
  1. Compute `totalWeight` across `pool`; if no piece has `maxPlaceCount > 0 && placeCount < maxPlaceCount`, `totalWeight = -1`.
  2. If `totalWeight > 0 && depth <= 30`: up to `5` attempts:
     - Draw `w = random.nextInt(totalWeight)`.
     - Walk `pool` subtracting `p.weight` until `w < 0`:
       - If `!p.doPlace(depth) || (p == start.previousPiece && !p.allowInRow)`: break (failed attempt).
       - Call `p`'s `createPiece`. If non-null: `p.placeCount++`, `start.previousPiece = p`, remove `p` from `pool` if `!p.isValid()`, return piece. *(Same fall-through quirk on `null` as Stronghold.)*
  3. Fallback if all 5 attempts fail (or `totalWeight <= 0` or `depth > 30`): return `BridgeEndFiller.createPiece(builder, random, footX, footY, footZ, dir, depth)`.
- **`generateChildForward / Left / Right`**: identical coordinate mapping to Stronghold's `generateSmallDoorChild*`, forwarding `isCastle`.

#### 4.4 Nether Fortress pieces (15 types)
Shorthand below: `NB = minecraft:nether_bricks`, `NBF = minecraft:nether_brick_fence`, `NS = [north=true, south=true]`, `WE = [west=true, east=true]`. All `generateBox` calls use `edge = fill` and `skipAir = false`.

##### A. `StartPiece` (`nestart`) & B. `BridgeCrossing` (`nebcr`)
- **Size**: `19 × 10 × 19`. `StartPiece` uses `makeBoundingBox(west, 64, north, dir, 19, 10, 19)`; `BridgeCrossing.createPiece` uses `orientBox(-8, -3, 0, 19, 10, 19, dir)`.
- **`addChildren`**: `Forward(8, 3, false)`, `Left(3, 8, false)`, `Right(3, 8, false)`.
- **`postProcess`**:
  1. Deck NB: `[7,3,0..11,4,18]`, `[0,3,7..18,4,11]`; `air`: `[8,5,0..10,7,18]`, `[0,5,8..18,7,10]`.
  2. Parapets NB (`y=5`): `[7,5,0..7,5,7]`, `[7,5,11..7,5,18]`, `[11,5,0..11,5,7]`, `[11,5,11..11,5,18]`, `[0,5,7..7,5,7]`, `[11,5,7..18,5,7]`, `[0,5,11..7,5,11]`, `[11,5,11..18,5,11]`.
  3. Under-arches & legs NB: `[7,2,0..11,2,5]`, `[7,2,13..11,2,18]`, `[7,0,0..11,1,3]`, `[7,0,15..11,1,18]`; `fillColumnDown(NB)` at `y=-1` for `x=7..11, z∈{0..2, 16..18}`.
  4. Cross under-arches & legs NB: `[0,2,7..5,2,11]`, `[13,2,7..18,2,11]`, `[0,0,7..3,1,11]`, `[15,0,7..18,1,11]`; `fillColumnDown(NB)` at `y=-1` for `x∈{0..2, 16..18}, z=7..11`.

##### C. `BridgeEndFiller` (`nebef`)
- **`createPiece`**: `orientBox(-1, -3, 0, 5, 10, 8, dir)`; draws `selfSeed = random.nextInt()`. No `addChildren`.
- **`postProcess`**: creates `selfRng = RandomSource.createThreadLocalInstance(selfSeed)` (legacy 48-bit LCG).
  1. For `x = 0..4`, `y = 3..4`: `z = selfRng.nextInt(8)`, fill NB `[x,y,0..x,y,z]`.
  2. `z = selfRng.nextInt(8)`, fill NB `[0,5,0..0,5,z]`; `z = selfRng.nextInt(8)`, fill NB `[4,5,0..4,5,z]`.
  3. For `x = 0..4`: `z = selfRng.nextInt(5)`, fill NB `[x,2,0..x,2,z]`.
  4. For `x = 0..4`, `y = 0..1`: `z = selfRng.nextInt(3)`, fill NB `[x,y,0..x,y,z]`.

##### D. `BridgeStraight` (`nebs`)
- **`createPiece`**: `orientBox(-1, -3, 0, 5, 10, 19, dir)`. **`addChildren`**: `Forward(1, 3, false)`.
- **`postProcess`**:
  1. NB `[0,3,0..4,4,18]`; `air` `[1,5,0..3,7,18]`; NB `[0,5,0..0,5,18]`, `[4,5,0..4,5,18]`, `[0,2,0..4,2,5]`, `[0,2,13..4,2,18]`, `[0,0,0..4,1,3]`, `[0,0,15..4,1,18]`.
  2. `fillColumnDown(NB, x, -1, z)` for `x=0..4, z∈{0..2, 16..18}`.
  3. Side fence ribs: `NBF[n,s,e]` at `[0,1,1..0,4,1]`, `[0,3,4..0,4,4]`, `[0,3,14..0,4,14]`, `[0,1,17..0,4,17]`; `NBF[n,s,w]` at `[4,1,1..4,4,1]`, `[4,3,4..4,4,4]`, `[4,3,14..4,4,14]`, `[4,1,17..4,4,17]`.

##### E. `RoomCrossing` (`nerc`)
- **`createPiece`**: `orientBox(-2, 0, 0, 7, 9, 7, dir)`. **`addChildren`**: `Forward(2, 0, false)`, `Left(0, 2, false)`, `Right(0, 2, false)`.
- **`postProcess`**:
  1. NB `[0,0,0..6,1,6]`; `air` `[0,2,0..6,7,6]`.
  2. Corner columns NB (`y=2..6`): `[0..1, z=0]`, `[0..1, z=6]`, `[5..6, z=0]`, `[5..6, z=6]`, `[x=0, 0..1]`, `[x=0, 5..6]`, `[x=6, 0..1]`, `[x=6, 5..6]`.
  3. Arch lintels & grates: NB `[2,6,0..4,6,0]`, `NBF[WE]` `[2,5,0..4,5,0]`, NB `[2,6,6..4,6,6]`, `NBF[WE]` `[2,5,6..4,5,6]`, NB `[0,6,2..0,6,4]`, `NBF[NS]` `[0,5,2..0,5,4]`, NB `[6,6,2..6,6,4]`, `NBF[NS]` `[6,5,2..6,5,4]`.
  4. `fillColumnDown(NB, x, -1, z)` for `x=0..6, z=0..6`.

##### F. `StairsRoom` (`nesr`)
- **`createPiece`**: `orientBox(-2, 0, 0, 7, 11, 7, dir)`. **`addChildren`**: `Right(6, 2, false)`.
- **`postProcess`**:
  1. NB `[0,0,0..6,1,6]`; `air` `[0,2,0..6,10,6]`.
  2. Walls NB (`y=2..8`): `[0,2,0..1,8,0]`, `[5,2,0..6,8,0]`, `[0,2,1..0,8,6]`, `[6,2,1..6,8,6]`, `[1,2,6..5,8,6]`.
  3. Windows `NBF[NS]`: `[0,3,2..0,5,4]`, `[6,3,2..6,5,2]`, `[6,3,4..6,5,4]`.
  4. Step blocks NB: `(5,2,5)`, `[4,2,5..4,3,5]`, `[3,2,5..3,4,5]`, `[2,2,5..2,5,5]`, `[1,2,5..1,6,5]`; upper landing `[1,7,1..5,7,4]`; doorway `air` `[6,8,2..6,8,4]`; entry arch NB `[2,6,0..4,8,0]` and `NBF[WE]` `[2,5,0..4,5,0]`.
  5. `fillColumnDown(NB, x, -1, z)` for `x=0..6, z=0..6`.

##### G. `MonsterThrone` (`nemt`)
- **`createPiece`**: `orientBox(-2, 0, 0, 7, 8, 9, dir)`. No `addChildren`.
- **`postProcess`**:
  1. `air` `[0,2,0..6,7,7]`; stepped base NB `[1,0,0..5,1,7]`, `[1,2,1..5,2,7]`, `[1,3,2..5,3,7]`, `[1,4,3..5,4,7]`; side steps NB `[1,2,0..1,4,2]`, `[5,2,0..5,4,2]`, `[1,5,2..1,5,3]`, `[5,5,2..5,5,3]`; upper rim NB `[0,5,3..0,5,8]`, `[6,5,3..6,5,8]`, `[1,5,8..5,5,8]`.
  2. Fence parapet (`NBF`): `(1,6,3)[w]`, `(5,6,3)[e]`, `(0,6,3)[e,n]`, `(6,6,3)[w,n]`, `[0,6,4..0,6,7][NS]`, `[6,6,4..6,6,7][NS]`, `(0,6,8)[e,s]`, `(6,6,8)[w,s]`, `[1,6,8..5,6,8][WE]`, `(1,7,8)[e]`, `[2,7,8..4,7,8][WE]`, `(5,7,8)[w]`, `(2,8,8)[e]`, `(3,8,8)[WE]`, `(4,8,8)[w]`.
  3. **Blaze spawner**: if `!hasPlacedSpawner` and `pos = getWorldPos(3, 5, 5)` is inside `chunkBB`: set `hasPlacedSpawner = true`, `level.setBlock(pos, minecraft:spawner, 2)`, `spawner.setEntityId(minecraft:blaze, random)`.
  4. `fillColumnDown(NB, x, -1, z)` for `x=0..6, z=0..6` *(note `z <= 6`, leaving `z = 7..8` unsupported)*.

##### H. `CastleEntrance` (`nece`)
- **`createPiece`**: `orientBox(-5, -3, 0, 13, 14, 13, dir)`. **`addChildren`**: `Forward(5, 3, true)`.
- **`postProcess`**:
  1. Base NB `[0,3,0..12,4,12]`; `air` `[0,5,0..12,13,12]`.
  2. Thick walls & ceiling NB: `[0,5,0..1,12,12]`, `[11,5,0..12,12,12]`, `[2,5,11..4,12,12]`, `[8,5,11..10,12,12]`, `[5,9,11..7,12,12]`, `[2,5,0..4,12,1]`, `[8,5,0..10,12,1]`, `[5,9,0..7,12,1]`, `[2,11,2..10,12,10]`.
  3. Entry portcullis: `NBF` (unconnected default!) `[5,8,0..7,8,0]`.
  4. Battlements (`i = 1, 3, 5, 7, 9, 11`): `NBF[WE]` at `[i,10,0..i,11,0]` & `[i,10,12..i,11,12]`; `NBF[NS]` at `[0,10,i..0,11,i]` & `[12,10,i..12,11,i]`; NB at `(i,13,0)`, `(i,13,12)`, `(0,13,i)`, `(12,13,i)`; if `i != 11`, `NBF[WE]` at `(i+1,13,0)` & `(i+1,13,12)`, `NBF[NS]` at `(0,13,i+1)` & `(12,13,i+1)`. Corners `y=13`: `(0,13,0)[n,e]`, `(0,13,12)[s,e]`, `(12,13,12)[s,w]`, `(12,13,0)[n,w]`.
  5. Inner wall windows (`z = 3, 5, 7, 9`): `NBF[n,s,w]` at `[1,7,z..1,8,z]`; `NBF[n,s,e]` at `[11,7,z..11,8,z]`.
  6. Under-arches & legs NB: `[4,2,0..8,2,12]`, `[0,2,4..12,2,8]`, `[4,0,0..8,1,3]`, `[4,0,9..8,1,12]`, `[0,0,4..3,1,8]`, `[9,0,4..12,1,8]`; `fillColumnDown(NB, x, -1, z)` for (`x=4..8, z∈{0..2, 10..12}`) and (`x∈{0..2, 10..12}, z=4..8`).
  7. Center lava well: NB `[5,5,5..7,5,7]`, `air` `[6,1,6..6,4,6]`, NB `(6,0,6)`, `minecraft:lava` at `(6,5,6)`, and explicitly `level.scheduleTick(getWorldPos(6,5,6), Fluids.LAVA, 0)` if inside `chunkBB`.

##### I. `CastleSmallCorridorPiece` (`nesc`)
- **`createPiece`**: `orientBox(-1, 0, 0, 5, 7, 5, dir)`. **`addChildren`**: `Forward(1, 0, true)`.
- **`postProcess`**: NB `[0,0,0..4,1,4]`; `air` `[0,2,0..4,5,4]`; walls NB `[0,2,0..0,5,4]`, `[4,2,0..4,5,4]`; windows `NBF[NS]` at `[0,3,1..0,4,1]`, `[0,3,3..0,4,3]`, `[4,3,1..4,4,1]`, `[4,3,3..4,4,3]`; roof NB `[0,6,0..4,6,4]`; `fillColumnDown(NB, 0..4, -1, 0..4)`.

##### J. `CastleSmallCorridorCrossingPiece` (`nescsc`)
- **`createPiece`**: `orientBox(-1, 0, 0, 5, 7, 5, dir)`. **`addChildren`**: `Forward(1, 0, true)`, `Left(0, 1, true)`, `Right(0, 1, true)`.
- **`postProcess`**: NB `[0,0,0..4,1,4]`; `air` `[0,2,0..4,5,4]`; 4 corner pillars NB `[0,2,0..0,5,0]`, `[4,2,0..4,5,0]`, `[0,2,4..0,5,4]`, `[4,2,4..4,5,4]`; roof NB `[0,6,0..4,6,4]`; `fillColumnDown(NB, 0..4, -1, 0..4)`.

##### K. `CastleSmallCorridorLeftTurnPiece` (`nesclt`) & L. `CastleSmallCorridorRightTurnPiece` (`nescrt`)
- **`createPiece`**: `orientBox(-1, 0, 0, 5, 7, 5, dir)`; draws `isNeedingChest = (random.nextInt(3) == 0)`.
- **`addChildren`**: LeftTurn calls `Left(0, 1, true)`; RightTurn calls `Right(0, 1, true)`.
- **`postProcess`**:
  - Floor NB `[0,0,0..4,1,4]`; `air` `[0,2,0..4,5,4]`.
  - **LeftTurn**: right wall NB `[4,2,0..4,5,4]` with `NBF[NS]` `[4,3,1..4,4,1]` & `[4,3,3..4,4,3]`; corner NB `[0,2,0..0,5,0]`; back wall NB `[0,2,4..3,5,4]` with `NBF[WE]` `[1,3,4..1,4,4]` & `[3,3,4..3,4,4]`; if `isNeedingChest && chunkBB.isInside(getWorldPos(3,2,3))`: `isNeedingChest = false`, `createChest(3, 2, 3, minecraft:chests/nether_bridge)`.
  - **RightTurn**: mirror on X — left wall NB `[0,2,0..0,5,4]` with `NBF[NS]` `[0,3,1..0,4,1]` & `[0,3,3..0,4,3]`; corner NB `[4,2,0..4,5,0]`; back wall NB `[1,2,4..4,5,4]` with `NBF[WE]` `[1,3,4..1,4,4]` & `[3,3,4..3,4,4]`; if `isNeedingChest && chunkBB.isInside(getWorldPos(1,2,3))`: `isNeedingChest = false`, `createChest(1, 2, 3, minecraft:chests/nether_bridge)`.
  - Roof NB `[0,6,0..4,6,4]`; `fillColumnDown(NB, 0..4, -1, 0..4)`.

##### M. `CastleCorridorStairsPiece` (`neccs`)
- **`createPiece`**: `orientBox(-1, -7, 0, 5, 14, 10, dir)`. **`addChildren`**: `Forward(1, 0, true)`.
- **`postProcess`**: for `step = 0..9` (`z = step`):
  - `floor = max(1, 7 - step)`, `roof = min(max(floor + 5, 14 - step), 13)`.
  - NB `[0,0,z..4,floor,z]`; `air` `[1,floor+1,z..3,roof-1,z]`.
  - If `step <= 6`: `nether_brick_stairs[facing=south]` at `(1..3, floor + 1, z)`.
  - Roof NB `[0,roof,z..4,roof,z]`; side walls NB `[0,floor+1,z..0,roof-1,z]` and `[4,floor+1,z..4,roof-1,z]`.
  - If `(step & 1) == 0`: `NBF[NS]` at `[0,floor+2,z..0,floor+3,z]` and `[4,floor+2,z..4,floor+3,z]`.
  - `fillColumnDown(NB, 0..4, -1, z)`.

##### N. `CastleCorridorTBalconyPiece` (`nectb`)
- **`createPiece`**: `orientBox(-3, 0, 0, 9, 7, 9, dir)`.
- **`addChildren`**: `zOff = (orientation ∈ {WEST, NORTH}) ? 5 : 1`; `Left(0, zOff, random.nextInt(8) > 0)`; `Right(0, zOff, random.nextInt(8) > 0)` (each draws `nextInt(8)` independently!).
- **`postProcess`**:
  1. NB `[0,0,0..8,1,8]`; `air` `[0,2,0..8,5,8]`; roof NB `[0,6,0..8,6,5]`; front walls NB `[0,2,0..2,5,0]` & `[6,2,0..8,5,0]`; front windows `NBF[WE]` `[1,3,0..1,4,0]` & `[7,3,0..7,4,0]`.
  2. Balcony floor NB `[0,2,4..8,2,8]`; openings `air` `[1,1,4..2,2,4]` & `[6,1,4..7,2,4]`.
  3. Balcony railing (`y=3`): `NBF[WE]` `[1,3,8..7,3,8]`, `(0,3,8)[e,s]`, `(8,3,8)[w,s]`, `[0,3,6..0,3,7][NS]`, `[8,3,6..8,3,7][NS]`.
  4. Back corridor walls NB `[0,3,4..0,5,5]`, `[8,3,4..8,5,5]`, `[1,3,5..2,5,5]`, `[6,3,5..7,5,5]`; windows `NBF[WE]` `[1,4,5..1,5,5]`, `[7,4,5..7,5,5]`.
  5. `fillColumnDown(NB, 0..8, -1, 0..5)`.

##### O. `CastleStalkRoom` (`necsr`)
- **`createPiece`**: `orientBox(-5, -3, 0, 13, 14, 13, dir)`. **`addChildren`**: `Forward(5, 3, true)`, then `Forward(5, 11, true)` (two stacked forward exits!).
- **`postProcess`**:
  1. Identical outer walls, ceiling, battlements, and inner wall windows to `CastleEntrance` (except **no** entry portcullis at `[5,8,0..7,8,0]`).
  2. Central staircase (`i = 0..6`, `z = i + 4`): `nether_brick_stairs[facing=north]` at `(5..7, 5 + i, z)`; under-stairs NB `[5,5,z..7,i+4,z]` if `5 <= z <= 8`, else `[5,8,z..7,i+4,z]` if `9 <= z <= 10`; if `i >= 1`, `air` `[5,6+i,z..7,9+i,z]`. Top step: `nether_brick_stairs[facing=north]` at `(5..7, 12, 11)`.
  3. Under-stair fence pillars: `NBF[n,s,e]` `[5,6,7..5,7,7]`, `NBF[n,s,w]` `[7,6,7..7,7,7]`; top rear doorway `air` `[5,13,12..7,13,12]`.
  4. Nether wart beds border NB (`y=5`): `[2,5,2..3,5,3]`, `[2,5,9..3,5,10]`, `[2,5,4..2,5,8]`, `[9,5,2..10,5,3]`, `[9,5,9..10,5,10]`, `[10,5,4..10,5,8]`; `nether_brick_stairs[west]` at `(4,5,2)`, `(4,5,3)`, `(4,5,9)`, `(4,5,10)`; `nether_brick_stairs[east]` at `(8,5,2)`, `(8,5,3)`, `(8,5,9)`, `(8,5,10)`.
  5. Planting beds: `minecraft:soul_sand` at `[3,4,4..4,4,8]` & `[8,4,4..9,4,8]`; `minecraft:nether_wart` (`age=0`) at `[3,5,4..4,5,8]` & `[8,5,4..9,5,8]`.
  6. Under-arches, legs, and `fillColumnDown` identical to `CastleEntrance` step 6.

---
### 5. Ocean Monument (`minecraft:ocean_monument`)

#### 5.1 Configuration schema, spawn overrides & terrain adaptation
- Schema: `StructureSettings` only.
- Vanilla defaults: `step = surface_structures`, `terrain_adaptation = none`, `spawn_overrides`:
  - `monster` (`full`): `minecraft:guardian` wt 1 (`UniformInt(2, 4)`).
  - `underground_water_creature` (`full`): empty list.
  - `axolotls` (`full`): empty list.

#### 5.2 Start generation, biome check & post-load regeneration
In `findGenerationPoint(context)`:
1. Compute `offsetX = chunk.blockX(9)`, `offsetZ = chunk.blockZ(9)`.
2. Query `context.biomeResolver().getBiomesWithin(offsetX, chunkGenerator.seaLevel, offsetZ, 29)` (radius 29 blocks around `(offsetX, seaLevel, offsetZ)`). If **any** biome in that set is NOT in `#minecraft:required_ocean_monument_surrounding`, return empty.
3. Call `onTopOfChunkCenter(context, OCEAN_FLOOR_WG, builder -> builder.addPiece(createTopPiece(chunkPos, random)))`.
   - `createTopPiece`: `west = chunk.minBlockX - 29`, `north = chunk.minBlockZ - 29`, `dir = Direction.Plane.HORIZONTAL.getRandomDirection(random)`, constructs a **single** top-level piece `MonumentBuilding(random, west, north, dir)`.
   - All 11 child room pieces live inside `MonumentBuilding.childPieces` (not in `PiecesContainer`).
   - **Save/load regeneration quirk**: `MonumentBuilding` does NOT serialize `childPieces` to NBT. On `StructureStart.loadStaticStart`, when `structure` is `OceanMonumentStructure`, `regeneratePiecesAfterLoad(chunkPos, seed, savedPieces)` creates a legacy `WorldgenRandom` seeded with `setLargeFeatureSeed(seed, chunkX, chunkZ)`, reads `(west, north)` from `savedPieces[0].boundingBox.minX/minZ`, draws `defaultDir = HORIZONTAL.getRandomDirection(random)`, keeps `savedPieces[0].orientation` if non-null else `defaultDir`, and reconstructs `MonumentBuilding(random, west, north, dir)`.

#### 5.3 Shared `OceanMonumentPiece` constants & helpers
- Block shorthands: `G = minecraft:prismarine` (`BASE_GRAY`), `L = minecraft:prismarine_bricks` (`BASE_LIGHT` / `DOT_DECO_DATA`), `D = minecraft:dark_prismarine` (`BASE_BLACK`), `SL = minecraft:sea_lantern` (`LAMP_BLOCK`), `W = minecraft:water` (`FILL_BLOCK`).
- `FILL_KEEP = { ice, packed_ice, blue_ice, water }`.
- Grid constants: room grid is `5 × 3 × 5` (`GRID_SIZE = 75`), index `getRoomIndex(rx, ry, rz) = ry * 25 + rz * 5 + rx`. Special indices: `SOURCE = (2,0,0) = 2`, `TOP_CONNECT = (2,2,0) = 52`, `LEFTWING_CONNECT = (0,1,0) = 25`, `RIGHTWING_CONNECT = (4,1,0) = 29`; virtual rooms `LEFTWING = 1001`, `RIGHTWING = 1002`, `PENTHOUSE = 1003`.
- **Room piece bounding box** (`roomWidth, roomHeight, roomDepth` in grid cells; each cell is `8 × 4 × 8` blocks):
  - `rx = idx % 5`, `rz = (idx / 5) % 5`, `ry = idx / 25`.
  - Start with `makeBoundingBox(0, 0, 0, dir, roomWidth * 8, roomHeight * 4, roomDepth * 8)`, then move by:
    - `NORTH`: `(rx * 8, ry * 4, -(rz + roomDepth) * 8 + 1)`
    - `SOUTH`: `(rx * 8, ry * 4, rz * 8)`
    - `WEST`: `(-(rz + roomDepth) * 8 + 1, ry * 4, rx * 8)`
    - `EAST`: `(rz * 8, ry * 4, rx * 8)`
- **`generateWaterBox(x0, y0, z0, x1, y1, z1)`**: loops `y, x, z`: reads `block = getBlock(x, y, z)`. If `block.getBlock() ∉ FILL_KEEP`: if `getWorldY(y) >= level.seaLevel && block != W`, places `minecraft:air`; else places `W`.
- **`generateBoxOnFillOnly(x0..z1, targetBlock)`**: loops `y, x, z`: places `targetBlock` iff `getBlock(x, y, z) == W` (exact default `water` state equality).
- **`generateDefaultFloor(xOff, zOff, downOpening)`**:
  - If `!downOpening`: fill `G` over `[xOff..xOff+7, 0, zOff..zOff+7]`.
  - If `downOpening`: fill `G` over `[xOff..xOff+2, 0, zOff..zOff+7]`, `[xOff+5..xOff+7, 0, zOff..zOff+7]`, `[xOff+3..xOff+4, 0, zOff..zOff+2]`, `[xOff+3..xOff+4, 0, zOff+5..zOff+7]`; trim `L` around the 2×2 hole at `[xOff+3..xOff+4, 0, zOff+2]`, `[xOff+3..xOff+4, 0, zOff+5]`, `[xOff+2, 0, zOff+3..zOff+4]`, `[xOff+5, 0, zOff+3..zOff+4]`.
- **`chunkIntersects(chunkBB, x0, z0, x1, z1)`**: transforms `(x0, z0)` and `(x1, z1)` to world `(wx0, wz0)` and `(wx1, wz1)` and tests 2D `chunkBB.intersects(min(wx0,wx1), min(wz0,wz1), max(wx0,wx1), max(wz0,wz1))`.
- **`spawnElder(x, y, z)`**: if `pos = getWorldPos(x, y, z)` is inside `chunkBB`, spawns `minecraft:elder_guardian` at `(pos.x + 0.5, pos.y, pos.z + 0.5)` healed to max health, `finalizeSpawn(..., STRUCTURE, null)`, added via `addFreshEntityWithPassengers`.

#### 5.4 `MonumentBuilding` (`omb`) room graph & fitter assembly
`boundingBox = makeBoundingBox(west, 39, north, dir, 58, 23, 58)` (Y spans `39..61`).
1. **Build 46-room grid (`generateRoomGraph(random)`)**:
   - Allocate `roomGrid[75]`:
     - Floor 0 (`y = 0`): `x = 0..4, z = 0..3` (20 rooms).
     - Floor 1 (`y = 1`): `x = 0..4, z = 0..3` (20 rooms).
     - Floor 2 (`y = 2`): `x = 1..3, z = 0..1` (6 rooms).
   - `sourceRoom = roomGrid[2]` (`(2, 0, 0)`).
   - Connect all adjacent present rooms (`x=0..4, z=0..4, y=0..2`, `Direction.values()`): if `neighZ == z`, connect in `dir`; **else (`neighZ != z`) connect in `dir.getOpposite()`** (flips NORTH/SOUTH relative to grid Z so grid `+Z` maps to `NORTH` in `RoomDefinition.connections`!).
   - Attach special claimed rooms: `roofRoom (1003)` on `roomGrid[52]` `UP`; `leftWing (1001)` on `roomGrid[25]` `SOUTH`; `rightWing (1002)` on `roomGrid[29]` `SOUTH`. Mark all three `claimed = true`.
   - Mark `sourceRoom.isSource = true`.
   - Pick **core room**: `coreRoom = roomGrid[getRoomIndex(random.nextInt(4), 0, 2)]` (`rx ∈ 0..3, ry = 0, rz = 2`). Mark the 2×2×2 block `{core, +E, +N, +E+N, +U, +E+U, +N+U, +E+N+U}` as `claimed = true`.
   - Collect all 46 non-null `roomGrid` entries in index order `0..74` into `roomDefs`, calling `updateOpenings()` on each (and on `roofRoom`).
   - Shuffle `roomDefs` with `Util.shuffle(roomDefs, random)`.
   - **Random wall closing** (`scanIndex = 1`): iterate `def` in shuffled `roomDefs`:
     - `closeCount = 0`, `attemptCount = 0`.
     - While `closeCount < 2 && attemptCount < 5`: `attemptCount++`; `f = random.nextInt(6)`. If `def.hasOpening[f]`: temporarily close edge `f` and its opposite on `def.connections[f]`; test `def.findSource(scanIndex++) && def.connections[f].findSource(scanIndex++)` (DFS over open edges to `sourceRoom`, note short-circuit: second `scanIndex++` only runs if first returns `true`). If both reach `sourceRoom`, keep closed and `closeCount++`; else reopen both.
   - Append `roofRoom`, `leftWing`, `rightWing` to `roomDefs` and return.
2. **Fit rooms into pieces**:
   - Mark `sourceRoom.claimed = true`. Add `OceanMonumentEntryRoom(dir, sourceRoom)` and `OceanMonumentCoreRoom(dir, coreRoom)` to `childPieces`.
   - Iterate `def` in `roomDefs`: if `!def.claimed && !def.isSpecial()` (`index < 75`), test the 7 fitters in exact priority order and add the first match:
     1. `FitDoubleXYRoom` (2×2×1): open & unclaimed `+E`, `+U`, and `(+E).+U`. Claims all 4.
     2. `FitDoubleYZRoom` (1×2×2): open & unclaimed `+N`, `+U`, and `(+N).+U`. Claims all 4.
     3. `FitDoubleZRoom` (1×1×2): open & unclaimed `+N` (note: `create` has a fallback to `SOUTH` if `+N` unavailable, never triggered after `fits`). Claims 2.
     4. `FitDoubleXRoom` (2×1×1): open & unclaimed `+E`. Claims 2.
     5. `FitDoubleYRoom` (1×2×1): open & unclaimed `+U`. Claims 2.
     6. `FitSimpleTopRoom` (1×1×1): NO openings in `W, E, N, S, U` (only `DOWN` open). Claims 1.
     7. `FitSimpleRoom` (1×1×1): always fits; constructor draws `mainDesign = random.nextInt(3)`. Claims 1.
   - Move every child piece in `childPieces` by `offset = this.getWorldPos(9, 0, 22)`.
3. **Add wings & penthouse**:
   - `leftWingBB = fromCorners(getWorldPos(1,1,1), getWorldPos(23,8,21))`
   - `rightWingBB = fromCorners(getWorldPos(34,1,1), getWorldPos(56,8,21))`
   - `penthouseBB = fromCorners(getWorldPos(22,13,22), getWorldPos(35,17,35))`
   - Draw `wingRandom = random.nextInt()`.
   - Add `OceanMonumentWingRoom(dir, leftWingBB, wingRandom)` (`mainDesign = wingRandom & 1`), `OceanMonumentWingRoom(dir, rightWingBB, wingRandom + 1)` (`mainDesign = (wingRandom + 1) & 1`), and `OceanMonumentPenthouse(dir, penthouseBB)`.

#### 5.5 `MonumentBuilding.postProcess` exterior shell
1. `waterHeight = max(level.seaLevel, 64) - bb.minY`. Call `generateWaterBox(0, 0, 0, 58, waterHeight, 58)`.
2. **Left & right wings** (`generateWing(isFlipped=false, xoff=0)` then `generateWing(isFlipped=true, xoff=33)`): guarded by `chunkIntersects(xoff, 0, xoff+23, 20)`:
   - Base `G` `[xoff,0,0..xoff+24,0,20]`; `generateWaterBox(xoff,1,0, xoff+24,10,20)`.
   - Stepped ridges (`i = 0..3`, `L`): `[xoff+i,i+1,i..xoff+i,i+1,20]`, `[xoff+i+7,i+5,i+7..xoff+i+7,i+5,20]`, `[xoff+17-i,i+5,i+7..xoff+17-i,i+5,20]`, `[xoff+24-i,i+1,i..xoff+24-i,i+1,20]`, `[xoff+i+1,i+1,i..xoff+23-i,i+1,i]`, `[xoff+i+8,i+5,i+7..xoff+16-i,i+5,i+7]`.
   - Decks `G`: `[xoff+4,4,4..xoff+6,4,20]`, `[xoff+7,4,4..xoff+17,4,6]`, `[xoff+18,4,4..xoff+20,4,20]`, `[xoff+11,8,11..xoff+13,8,20]`.
   - Trim studs `L`: `(xoff+12, 9, z)` for `z∈{12,15,18}`; `leftPos = xoff + (isFlipped ? 19 : 5)`, `rightPos = xoff + (isFlipped ? 5 : 19)`: `(leftPos, 5, z)` for `z=20,17,14,11,8,5`; `(rightPos, 5, z)` for `z=19,16,13,10,7`; for `i=0..3`, `(isFlipped ? xoff+24-(17-3i) : xoff+17-3i, 5, 5)`; `(rightPos, 5, 5)`.
   - Center cross-pillar `G`: `[xoff+11,1,12..xoff+13,7,12]` and `[xoff+12,1,11..xoff+12,7,13]`.
3. **Entrance arches** (`chunkIntersects(22, 5, 35, 17)`): `generateWaterBox(25,0,0, 32,8,20)`; for `i = 0..3` (`z = 5 + 4i`): `L` `[24,2,z..24,4,z]`, `[22,4,z..23,4,z]`, `(25,5,z)`, `(26,6,z)`, `SL` `(26,5,z)`, `L` `[33,2,z..33,4,z]`, `[34,4,z..35,4,z]`, `(32,5,z)`, `(31,6,z)`, `SL` `(31,5,z)`, `G` `[27,6,z..30,6,z]`.
4. **Entrance wall** (`chunkIntersects(15, 20, 42, 21)`): all at `z = 21` unless noted:
   - `G`: `[15,0..42,0]`, `[21,12..36,12]`, `[17,11..40,11]`, `[16,10..41,10]`, `[15,7..42,9]`, `[16,6..41,6]`, `[17,5..40,5]`, `[21,4..36,4]`, `[22,3..26,3]`, `[31,3..35,3]`, `[23,2..25,2]`, `[32,2..34,2]`.
   - `generateWaterBox(26,1,21, 31,3,21)`.
   - Archway trim `L`: `[28,4,20..29,4,21]`, `(27,3,21)`, `(30,3,21)`, `(26,2,21)`, `(31,2,21)`, `(25,1,21)`, `(32,1,21)`.
   - Chevron relief `D`: for `i=0..6`: `(28-i,6+i,21)`, `(29+i,6+i,21)`; for `i=0..3`: `(28-i,9+i,21)`, `(29+i,9+i,21)`; `(28,12,21)`, `(29,12,21)`; for `i=0..2`: `(22-2i,8..9,21)`, `(35+2i,8..9,21)`.
   - Water cuts around triangle: `[15,13..42,15]`, `[15,1..15,6]`, `[16,1..16,5]`, `[17,1..20,4]`, `[21,1..21,3]`, `[22,1..22,2]`, `[23,1..24,1]`, `[42,1..42,6]`, `[41,1..41,5]`, `[37,1..40,4]`, `[36,1..36,3]`, `[33,1..34,1]`, `[35,1..35,2]`.
5. **Roof crown** (`chunkIntersects(21, 21, 36, 36)`): `G` `[21,0,22..36,0,36]`; `generateWaterBox(21,1,22, 36,23,36)`; for `i=0..3` (`y=13+i`), 4-sided `L` ring `[21+i..36-i, y, 21+i]`, `[21+i..36-i, y, 36-i]`, `[21+i, y, 22+i..35-i]`, `[36-i, y, 22+i..35-i]`; cap `G` `[25,16,25..32,16,32]`; 4 corner pillars `L` `(25|32, 17..19, 25|32)`; 4 diagonal finials `(26,20,26)+(27,21,27)+SL(27,20,27)` and 3 symmetric corners `(26/27, 31/30)`, `(31/30, 31/30)`, `(31/30, 26/27)`; crown rim `G` `[28..29,21,27]`, `[27,21,28..29]`, `[28..29,21,30]`, `[30,21,28..29]`.
6. **Lower wall**:
   - West (`chunkIntersects(0, 21, 6, 58)`): `G` `[0,0,21..6,0,57]`, water `[0,1,21..6,7,57]`, `G` `[4,4,21..6,4,53]`, steps `i=0..3` `L` `[i,i+1,21..i,i+1,57-i]` (run twice), studs `L` `(5,5,23..50 step 3)` + `(5,5,52)`, rear corner pillar `G` `[4,1,52..6,3,52]` & `[5,1,51..5,3,53]`.
   - East (`chunkIntersects(51, 21, 58, 58)`): mirror at `x = 51..57`, steps `[57-i,i+1,21..57-i,i+1,57-i]`, studs `(52,5,z)`, pillar `[51,1,52..53,3,52]` & `[52,1,51..52,3,53]`.
   - Back (`chunkIntersects(0, 51, 57, 57)`): `G` `[7,0,51..50,0,57]`, water `[7,1,51..50,10,57]`, steps `i=0..3` `L` `[i+1,i+1,57-i..56-i,i+1,57-i]`.
7. **Middle wall**:
   - West (`chunkIntersects(7, 21, 13, 50)`): `G` `[7,0,21..13,0,50]`, water `[7,1,21..13,10,50]`, `G` `[11,8,21..13,8,53]`, steps `i=0..3` `L` `[i+7,i+5,21..i+7,i+5,54]`, studs `L` `(12,9,21..45 step 3)`.
   - East (`chunkIntersects(44, 21, 50, 54)`): `G` `[44,0,21..50,0,50]`, water `[44,1,21..50,10,50]`, `G` `[44,8,21..46,8,53]`, steps `i=0..3` `L` `[50-i,i+5,21..50-i,i+5,54]`, studs `L` `(45,9,21..45 step 3)`.
   - Back (`chunkIntersects(8, 44, 49, 54)`): `G` `[14,0,44..43,0,50]`, water `[14,1,44..43,10,50]`; for `x = 12..45 step 3`: `L` at `(x,9,45)` & `(x,9,52)`, and when `x ∈ {12,18,24,33,39,45}` also arch pattern `L` at `(x,9,47)`, `(x,9,50)`, `(x,10,45..46)`, `(x,10,51..52)`, `(x,11,47)`, `(x,11,50)`, `(x,12,48..49)`; steps `i=0..2` `G` `[8+i,5+i,54..49-i,5+i,54]`; `L` `[11,8,54..46,8,54]`; `G` `[14,8,44..43,8,53]`.
8. **Upper wall**:
   - West (`chunkIntersects(14, 21, 20, 43)`): `G` `[14,0,21..20,0,43]`, water `[14,1,22..20,14,43]`, `G` `[18,12,22..20,12,39]`, `L` `[18,12,21..20,12,21]`, steps `i=0..3` `L` `[i+14,i+9,21..i+14,i+9,43-i]`, studs `L` `(19,13,23..39 step 3)`.
   - East (`chunkIntersects(37, 21, 43, 43)`): `G` `[37,0,21..43,0,43]`, water `[37,1,22..43,14,43]`, `G` `[37,12,22..39,12,39]`, `L` `[37,12,21..39,12,21]`, steps `i=0..3` `L` `[43-i,i+9,21..43-i,i+9,43-i]`, studs `L` `(38,13,23..39 step 3)`.
   - Back (`chunkIntersects(15, 37, 42, 43)`): `G` `[21,0,37..36,0,43]`, water `[21,1,37..36,14,43]`, `G` `[21,12,37..36,12,39]`, steps `i=0..3` `L` `[15+i,i+9,43-i..42-i,i+9,43-i]`, studs `L` `(21..36 step 3, 13, 38)`.
9. **Foundation pillars**: 23 perimeter 4×4 `L` legs (`pillarX = 0..6`, `pillarZ = 0..6` on perimeter only, skipping `(3, 0)`): at `bx = pillarX * 9`, `bz = pillarZ * 9`, for `w=0..3, d=0..3`: place `L` at `(bx+w, 0, bz+d)` and `fillColumnDown(L, bx+w, -1, bz+d)`.
10. **Surrounding sloped water moat** (`i = 0..4`): `generateWaterBox` on all 4 sides around monument: `[-1-i, 2i, -1-i .. -1-i, 23, 58+i]`, `[58+i, 2i, -1-i .. 58+i, 23, 58+i]`, `[-i, 2i, -1-i .. 57+i, 23, -1-i]`, `[-i, 2i, 58+i .. 57+i, 23, 58+i]`.
11. **Child pieces**: for each `child` in `childPieces` in insertion order, if `child.boundingBox.intersects(chunkBB)`, invoke `child.postProcess(...)`.

#### 5.6 Ocean Monument child pieces (11 types)
All interior rooms use piece-local coordinates (`0..7` per X/Z cell, `0..3` per Y cell; openings test `roomDefinition.hasOpening[dir.3d]`, where in local coords `SOUTH` is `z=0`, `NORTH` is `z=maxZ`, `WEST` is `x=0`, `EAST` is `x=maxX`).

##### A. `OceanMonumentEntryRoom` (`omentry`, 1×1×1)
- Walls `L`: `[0,3,0..2,3,7]`, `[5,3,0..7,3,7]`, `[0,2,0..1,2,7]`, `[6,2,0..7,2,7]`, `[0,1,0..0,1,7]`, `[7,1,0..7,1,7]`, `[0,1,7..7,3,7]`, `[1,1,0..2,3,0]`, `[5,1,0..6,3,0]`.
- Doorways (water): if `NORTH` `[3,1,7..4,2,7]`; if `WEST` `[0,1,3..1,2,4]`; if `EAST` `[6,1,3..7,2,4]`.

##### B. `OceanMonumentCoreRoom` (`omcr`, 2×2×2 — Gold Treasure Chamber)
- Ceiling `generateBoxOnFillOnly(1,8,0, 14,8,14, G)`; upper border `L` at `y=7`: `[0,7,0..0,7,15]`, `[15,7,0..15,7,15]`, `[1,7,0..15,7,0]`, `[1,7,15..14,7,15]`.
- Walls (`y=1..6`, `block = (y==2 || y==6) ? G : L`): on `x ∈ {0,15}` fill `z∈{0..1, 6..9, 14..15}`; on `z=0` fill `x∈{1, 6..9, 14}`; on `z=15` fill `x=1..14`.
- **Treasure core**: `D` outer cube `[6,3,6..9,6,9]`; **`minecraft:gold_block` 2×2×2 inner cube `[7,4,7..8,5,8]`** (8 blocks); `SL` at the 8 outer corners `(6|9, 3|6, 6|9)`.
- Pillars & overhead beams `L`: legs `[5,1..2,6]`, `[5,1..2,9]`, `[10,1..2,6]`, `[10,1..2,9]`, `[6,1..2,5]`, `[9,1..2,5]`, `[6,1..2,10]`, `[9,1..2,10]`; corner posts `[5|10, 2..6, 5|10]`; ceiling beams (`y=7`) `[5|10, 7, 1..6]`, `[5|10, 7, 9..14]`, `[1..6, 7, 5|10]`, `[9..14, 7, 5|10]`; 4 corner floor accents (`y=1`): `(2,1,2..3)+(3,1,2)`, `(13,1,2..3)+(12,1,2)`, `(2,1,12..13)+(3,1,13)`, `(13,1,12..13)+(12,1,13)`.

##### C. `OceanMonumentDoubleXRoom` (`omdxr`, 2×1×1)
- If `index / 25 > 0`: `generateDefaultFloor(8, 0, east.open[DOWN])`, `generateDefaultFloor(0, 0, west.open[DOWN])`.
- Ceilings `generateBoxOnFillOnly(..., G)`: if `west.conn[UP] == null` `[1,4,1..7,4,6]`; if `east.conn[UP] == null` `[8,4,1..14,4,6]`.
- Perimeter walls (`x=0`, `x=15`, `z=0` `x=1..15`, `z=7` `x=1..14`): `L` at `y=1,3`, `G` at `y=2`.
- Central alcove: `L` `[5,1,0..10,1,4]` & `[5,3,0..10,3,4]`, `G` `[6,2,0..9,2,3]`, `SL` `(6,2,3)` & `(9,2,3)`.
- Water doorways: `west`: `S` `[3,1,0..4,2,0]`, `N` `[3,1,7..4,2,7]`, `W` `[0,1,3..0,2,4]`; `east`: `S` `[11,1,0..12,2,0]`, `N` `[11,1,7..12,2,7]`, `E` `[15,1,3..15,2,4]`.

##### D. `OceanMonumentDoubleYRoom` (`omdyr`, 1×2×1)
- If `index / 25 > 0`: `generateDefaultFloor(0, 0, def.open[DOWN])`.
- If `above.conn[UP] == null`: `generateBoxOnFillOnly(1,8,1, 6,8,6, G)`.
- Mid-level balcony `L` at `y=4`: perimeter `[0,4,0..0,4,7]`, `[7,4,0..7,4,7]`, `[1,4,0..6,4,0]`, `[1,4,7..6,4,7]` plus 4 corner brackets `(2,4,1..2)+(1,4,2)`, `(5,4,1..2)+(6,4,2)`, `(2,4,5..6)+(1,4,5)`, `(5,4,5..6)+(6,4,5)`.
- Two levels (`y = 1` using `def`, then `y = 5` using `above`): on each of `S(z=0)`, `N(z=7)`, `W(x=0)`, `E(x=7)`: if open, place `L` doorframe pillars at `2` & `5` (`y..y+2`) and lintel `3..4` (`y+2`); else solid wall `L` (`y..y+2`) with middle band `G` (`y+1`).

##### E. `OceanMonumentDoubleZRoom` (`omdzr`, 1×1×2)
- If `index / 25 > 0`: `generateDefaultFloor(0, 8, north.open[DOWN])`, `generateDefaultFloor(0, 0, south.open[DOWN])`.
- Ceilings `generateBoxOnFillOnly(..., G)`: if `south.conn[UP] == null` `[1,4,1..6,4,7]`; if `north.conn[UP] == null` `[1,4,8..6,4,14]`.
- Perimeter walls (`x=0,7`, `z=0` `x=1..7`, `z=15` `x=1..6`): `L` at `y=1,3`, `G` at `y=2`.
- End corner buttresses `L`: `(1|6, 1|3, 1..2)` and `(1|6, 1|3, 13..14)`.
- Center arch & lamps: `L` pillars `(2|5, 1..3, 6|9)`, cross-bars `(3..4, 2, 6|9)` & `(2|5, 2, 7..8)`, `SL` at `(2|5, 2, 5|10)`, `L` above lamps at `(2|5, 3, 5|10)`.
- Water doorways: `south`: `S` `[3,1,0..4,2,0]`, `E` `[7,1,3..7,2,4]`, `W` `[0,1,3..0,2,4]`; `north`: `N` `[3,1,15..4,2,15]`, `W` `[0,1,11..0,2,12]`, `E` `[7,1,11..7,2,12]`.

##### F. `OceanMonumentDoubleXYRoom` (`omdxyr`, 2×2×1)
- If `index / 25 > 0`: `generateDefaultFloor(8, 0, east.open[DOWN])`, `generateDefaultFloor(0, 0, west.open[DOWN])`.
- Ceilings `generateBoxOnFillOnly(..., G)`: `westUp.conn[UP]==null` `[1,8,1..7,8,6]`, `eastUp.conn[UP]==null` `[8,8,1..14,8,6]`.
- Perimeter walls (`y=1..7`, `G` at `y=2,6`, else `L`): `x=0`, `x=15`, `z=0` (`1..15`), `z=7` (`1..14`).
- Interior pillars & centerpiece `L`: `[2,1..7,3..4]`, `[3..4,1..7,2]`, `[3..4,1..7,5]`, `[13,1..7,3..4]`, `[11..12,1..7,2]`, `[11..12,1..7,5]`, `[5,1..3,3..4]`, `[10,1..3,3..4]`, `[5..10,7,2..5]`, `[5|10,5..7,2|5]`, `(6|9,6,2|5)`, `[5..6,4,3..4]`, `[9..10,4,3..4]`; `SL` at `(5|10, 4, 2|5)`.
- Water doorways (12 checks): lower `west` (`S,N,W`) & `east` (`S,N,E`) at `y=1..2`; upper `westUp` (`S,N,W`) & `eastUp` (`S,N,E`) at `y=5..6`.

##### G. `OceanMonumentDoubleYZRoom` (`omdyzr`, 1×2×2)
- If `index / 25 > 0`: `generateDefaultFloor(0, 8, north.open[DOWN])`, `generateDefaultFloor(0, 0, south.open[DOWN])`.
- Ceilings `generateBoxOnFillOnly(..., G)`: `southUp.conn[UP]==null` `[1,8,1..6,8,7]`, `northUp.conn[UP]==null` `[1,8,8..6,8,14]`.
- Perimeter walls (`y=1..7`, `G` at `y=2,6`, else `L`): `x=0`, `x=7`, `z=0` (`1..6`), `z=15` (`1..6`).
- Central 2×7×2 column (`[3,y,7..4,y,8]`): `SL` at `y=2,6`, `D` at `y=1,3,4,5,7`.
- Water doorways: lower `south` (`S,E,W`) & `north` (`N,W,E`) at `y=1..2`; upper `southUp` `S` `[3,5,0..4,6,0]`, `northUp` `N` `[3,5,15..4,6,15]`; upper side balconies (when open, carve doorway + place `L` balcony platform at `y=4` and two `y=1..3` support posts):
  - `southUp.E`: water `[7,5,3..7,6,4]`, `L` `[5,4,2..6,4,5]`, `[6,1..3,2]`, `[6,1..3,5]`.
  - `southUp.W`: water `[0,5,3..0,6,4]`, `L` `[1,4,2..2,4,5]`, `[1,1..3,2]`, `[1,1..3,5]`.
  - `northUp.W`: water `[0,5,11..0,6,12]`, `L` `[1,4,10..2,4,13]`, `[1,1..3,10]`, `[1,1..3,13]`.
  - `northUp.E`: water `[7,5,11..7,6,12]`, `L` `[5,4,10..6,4,13]`, `[6,1..3,10]`, `[6,1..3,13]`.

##### H. `OceanMonumentSimpleRoom` (`omsimple`, 1×1×1)
`mainDesign ∈ {0, 1, 2}` drawn at construction.
- If `index / 25 > 0`: `generateDefaultFloor(0, 0, def.open[DOWN])`.
- If `def.conn[UP] == null`: `generateBoxOnFillOnly(1,4,1, 6,4,6, G)`.
- **Center pillar roll**: `centerPillar = (mainDesign != 0) && random.nextBoolean() && !def.open[DOWN] && !def.open[UP] && def.countOpenings() > 1` (short-circuit: `random.nextBoolean()` is ONLY called when `mainDesign != 0`!).
- **Design `0`** (4 corner lantern pods): 4 corners `(0..2, 0..2)`, `(5..7, 0..2)`, `(0..2, 5..7)`, `(5..7, 5..7)` each with `L` at `y=1,3`, `G` outer L-walls at `y=2`, and `SL` at `(1,2,1)`, `(6,2,1)`, `(1,2,6)`, `(6,2,6)`. Each side (`S, N, W, E`): if open, 2-block `L` lintel at `y=3`; else 2-thick `L` at `y=1,3` and outer `G` at `y=2`.
- **Design `1`** (4 freestanding lamp pillars): 4 pillars `L` at `(2|5, 1..3, 2|5)` with `SL` at `y=2`; 4 corner `L` walls with `G` at `y=2` (`(1,2,0)`, `(0,2,1)`, `(1,2,7)`, `(0,2,6)`, `(6,2,7)`, `(7,2,6)`, `(6,2,0)`, `(7,2,1)`); closed sides filled with `L`/`G`/`L` at `y=1/2/3`.
- **Design `2`** (dark prismarine bands): full perimeter `L`/`D`/`L` at `y=1/2/3`, plus `D` at the 4 doorway midpoints (`y=1..2`), then carve water doorways on any open side (`S, N, W, E`).
- If `centerPillar`: place `[3,1,3..4,1,4]` `L`, `[3,2,3..4,2,4]` `G`, `[3,3,3..4,3,4]` `L`.

##### I. `OceanMonumentSimpleTopRoom` (`omsimplet`, 1×1×1 — Sponge Room)
- If `index / 25 > 0`: `generateDefaultFloor(0, 0, def.open[DOWN])`.
- If `def.conn[UP] == null`: `generateBoxOnFillOnly(1,4,1, 6,4,6, G)`.
- **Wet sponges**: loop `x = 1..6` (outer), `z = 1..6` (inner): if `random.nextInt(3) != 0` (2/3 chance): `y0 = 2 + (random.nextInt(4) == 0 ? 0 : 1)` (height 2 with 25% chance, height 1 with 75% chance); place `minecraft:wet_sponge` at `[x, y0, z .. x, 3, z]`.
- Perimeter walls identical to `SimpleRoom` design `2` (`L`/`D`/`L` + `D` doorway markers); then only if `def.open[SOUTH]` (never true in vanilla because `FitSimpleTopRoom` rejects `SOUTH` openings), carves water at `[3,1,0..4,2,0]`.

##### J. `OceanMonumentWingRoom` (`omwr`, 23×8×21)
`mainDesign = randomValue & 1` (`0` or `1`; left and right wings always get opposite designs because `wingRandom` increments by 1!).
- **Design `0`**: stepped entrance (`i=0..3`) `L` `[10-i,3-i,20-i..12+i,3-i,20]`; hall `L` `[7,0,6..15,0,16]`, `[6,0,6..6,3,20]`, `[16,0,6..16,3,20]`, `[7,1,7..7,1,20]`, `[15,1,7..15,1,20]`, `[7,1,6..9,3,6]`, `[13,1,6..15,3,6]`, `[8,1,7..9,1,7]`, `[13,1,7..14,1,7]`, `[9,0,5..13,0,5]`; dark strips `D` `[10,0,7..12,0,7]`, `[8,0,10..8,0,12]`, `[14,0,10..14,0,12]`; wall lamps `SL` at `(6|16, 3, 18..7 step -3)`; floor lamps `SL` `(10|12, 0, 10|12)`; front lamps `SL` `(8|14, 3, 6)`; 4 corner pedestals `(4|18, 0..2, 4|18)` (`L, SL, L`); upper markers `L` `(9|13, 7, 20)`; back piers `L` `[6,0,21..7,4,21]` & `[15,0,21..16,4,21]`; `spawnElder(11, 2, 16)`.
- **Design `1`**: entry platform `L` `[9,3,18..13,3,20]`, `[9|13, 0..2, 18]`, two lamp pillars `(9|13, 4..6, 20)` (`L, SL, L`); center ceiling `L` `[7,3,7..15,3,14]`; 4 tall columns `(10|12, 0..6, 10|12)` `L` with `SL` at `y=0` and `y=4`; 4 short posts `(8|14, 0..2, 7|14)` `L`; roof strips `D` `[8|14, 3, 8..13]`; `spawnElder(11, 5, 13)`.

##### K. `OceanMonumentPenthouse` (`ompenthouse`, 14×5×14)
- Sub-floor (`y=-1`): `L` `[2,-1,2..11,-1,11]`; `G` border `[0..1, 0..11]`, `[12..13, 0..11]`, `[2..11, 0..1]`, `[2..11, 12..13]`.
- Rim (`y=0`): `L` `[0,0,0..0,0,13]`, `[13,0,0..13,0,13]`, `[1,0,0..12,0,0]`, `[1,0,13..12,0,13]`; `SL` at `i ∈ {2,5,8,11}` on `(0,0,i)`, `(13,0,i)`, `(i,0,0)`.
- Interior dais (`y=0`): `L` `[2,0,3..4,0,9]`, `[9,0,3..11,0,9]`, `[4,0,9..9,0,11]`, `(5,0,8)`, `(8,0,8)`, `(10,0,10)`, `(3,0,10)`; `D` `[3,0,3..3,0,7]`, `[10,0,3..10,0,7]`, `[6,0,10..7,0,10]`; 6 pillars `L` (`y=0..2`) at `(3|10, z ∈ {2,5,8})`, `(5, 10)`, `(8, 10)`; floor accent `D` `[6,-1,7..7,-1,8]`; entry shaft water `[6,-1,3..7,-1,4]`; `spawnElder(6, 1, 6)`.

---

### 6. Woodland Mansion (`minecraft:woodland_mansion`)

#### 6.1 Configuration schema, start check & terrain adaptation
- Schema: `StructureSettings` only. Vanilla defaults: `step = surface_structures`, `terrain_adaptation = none`, no `spawn_overrides`.
- `findGenerationPoint(context)`:
  1. `blockX = chunk.blockX(7)`, `blockZ = chunk.blockZ(7)`. If `!couldValidBiomeExistInTerrainColumn(blockX, blockZ)`, return empty.
  2. `rotation = Rotation.getRandom(context.random())`.
  3. `startPos = getLowestYIn5by5Box(context, blockX, blockZ, rotation)`.
  4. If `startPos.y < 60`, return empty. Else return stub at `startPos` invoking `WoodlandMansionPieces.generateMansion(templateManager, startPos, rotation, pieces, random)`.

#### 6.2 `MansionGrid` 11×11 floorplan planner
Uses `SimpleGrid(11, 11, valueIfOutside = 5)` (initial cells `0 = CLEAR`, `1 = CORRIDOR`, `2 = ROOM`, `3 = START_ROOM`, `4 = TEST_ROOM`, `5 = BLOCKED`). `set(x0, y0, x1, y1, val)` writes inclusive ranges (ignoring out-of-bounds). `edgesTo(x, y, v)` checks 4 neighbors `(x-1,y), (x+1,y), (x,y+1), (x,y-1)`. `isHouse(grid, x, y)` is true iff `value ∈ {1, 2, 3, 4}`.

Room-grid bitfields (`floorRooms[0..2]`):
- `ROOM_ID_MASK = 0xFFFF`, `ROOM_1x1 = 0x10000`, `ROOM_1x2 = 0x20000`, `ROOM_2x2 = 0x40000`, `ROOM_TYPE_MASK = 0xF0000`
- `ROOM_ORIGIN_FLAG = 0x100000`, `ROOM_DOOR_FLAG = 0x200000`, `ROOM_STAIRS_FLAG = 0x400000`, `ROOM_CORRIDOR_FLAG = 0x800000`.

Ordered construction in `MansionGrid(random)`:
1. Initialize `entranceX = 7`, `entranceY = 4`. Set `baseGrid` cells:
   - `[7..8, 4..5] = 3` (`START_ROOM`), `[6, 4..5] = 2` (`ROOM`), `[9..10, 2..7] = 5` (`BLOCKED`).
   - `[8, 2..3] = 1`, `[8, 6..7] = 1`, `(6, 3) = 1`, `(6, 6) = 1`.
   - `[0..11, 0..1] = 5` (top 2 rows blocked), `[0..11, 9..11] = 5` (bottom 2 rows blocked).
2. Carve 4 recursive corridors in exact order:
   - `recursiveCorridor(baseGrid, 7, 2, WEST, 6)`
   - `recursiveCorridor(baseGrid, 7, 7, WEST, 6)`
   - `recursiveCorridor(baseGrid, 5, 3, WEST, 3)`
   - `recursiveCorridor(baseGrid, 5, 6, WEST, 3)`
   - **`recursiveCorridor(grid, x, y, heading, depth)`**: if `depth <= 0` return. Set `(x, y) = 1`, `setif(x+hx, y+hz, 0, 1)`. Up to `8` attempts: draw `nextDir = Direction.from2DDataValue(random.nextInt(4))` (`0=S, 1=W, 2=N, 3=E`); if `nextDir != heading.opposite && (nextDir != EAST || !random.nextBoolean())`: let `(nx, ny) = (x+hx, y+hz)`; if `grid(nx+dx, ny+dz) == 0 && grid(nx+2dx, ny+2dz) == 0`, recurse `(nx+dx, ny+dz, nextDir, depth - 1)` and break. Then set `0 -> 2` (`ROOM`) at the 7 flanking cells: `±cw`, `±ccw`, `heading + cw`, `heading + ccw`, `2*heading`, `2*cw`, `2*ccw`.
3. Repeat `cleanEdges(baseGrid)` until it returns `false`: scans `y = 0..10` (outer), `x = 0..10` (inner); any `0` cell with `>= 3` cardinal `isHouse` neighbors OR (`== 2` cardinal AND `<= 1` diagonal `isHouse` neighbors) becomes `2` (`ROOM`).
4. Run `identifyRooms(baseGrid, floorRooms[0])`, then `identifyRooms(baseGrid, floorRooms[1])`.
   - **`identifyRooms(fromGrid, roomGrid)`**:
     1. Collect all `(x, y)` with `fromGrid(x, y) == 2` in `y=0..10, x=0..10` order; shuffle with `Util.shuffle(list, random)`.
     2. Initialize `roomId = 10`. For each `(x, y)` with `roomGrid(x, y) == 0`:
        - Try expanding in exact priority order (checking both `roomGrid == 0` and `fromGrid == 2`):
          1. 2×2 to `(+1,+1)` (`x1++, y1++`)
          2. 2×2 to `(-1,+1)` (`x0--, y1++`)
          3. 2×2 to `(-1,-1)` (`x0--, y0--`) *(note `(+1,-1)` is not checked)*
          4. 1×2 to `(+1,0)`
          5. 1×2 to `(0,+1)`
          6. 1×2 to `(-1,0)`
          7. 1×2 to `(0,-1)`
          8. Else 1×1.
        - Pick door corner: `doorX = random.nextBoolean() ? x0 : x1`, `doorY = random.nextBoolean() ? y0 : y1`, `doorFlag = ROOM_DOOR_FLAG`. Test `fromGrid.edgesTo(doorX, doorY, 1)` across up to 4 corners by flipping `(X&Y)`, then `(Y)`, then `(X&Y)` again; if none touches a corridor, set `doorFlag = 0, doorX = x0, doorY = y0` (secret room!).
        - Write `type | roomId` to `[x0..x1, y0..y1]`, ORing `ROOM_ORIGIN_FLAG | doorFlag` at `(doorX, doorY)`. Increment `roomId`.
5. Mark entrance corridor flags: `floorRooms[0].set(8, 4, 8, 5, ROOM_CORRIDOR_FLAG)` and `floorRooms[1].set(8, 4, 8, 5, ROOM_CORRIDOR_FLAG)`.
6. **`setupThirdFloor()`**:
   - Collect all `(x, y)` in `y=0..10, x=0..10` order where `floorRooms[1](x, y)` has `type == ROOM_1x2` and `ROOM_DOOR_FLAG` set.
   - If empty: fill entire `thirdFloorGrid` with `5` (`BLOCKED`).
   - Else: pick `roomPos = potentialRooms[random.nextInt(size)]`; OR `ROOM_STAIRS_FLAG` onto `floorRooms[1](roomPos)`. Find `roomDir = get1x2RoomDirection` (first `dir` in `HORIZONTAL` `[N, E, S, W]` whose neighbor shares `roomId`), `roomEnd = roomPos + roomDir`.
   - Copy mask to `thirdFloorGrid`: `!isHouse(baseGrid)` → `5`; `roomPos` → `3`; `roomEnd` → `3` and `floorRooms[2](roomEnd) = ROOM_CORRIDOR_FLAG` (remaining house cells stay `0`).
   - Collect all `dir ∈ HORIZONTAL` with `thirdFloorGrid(roomEnd + dir) == 0`. If empty: fill `thirdFloorGrid` with `5` and clear `ROOM_STAIRS_FLAG` on `floorRooms[1](roomPos)`. Else pick `corridorDir = potentialCorridors[random.nextInt(size)]`, run `recursiveCorridor(thirdFloorGrid, roomEnd + corridorDir, corridorDir, 4)`, then `while (cleanEdges(thirdFloorGrid))`.
7. Run `identifyRooms(thirdFloorGrid, floorRooms[2])`.

#### 6.3 `MansionPiecePlacer` piece emission order & template selection
Every `WoodlandMansionPiece` (`wmp`) loads `minecraft:woodland_mansion/<name>` with `StructurePlaceSettings`: `ignoreEntities = true`, `rotation`, `mirror`, processor `BlockIgnoreProcessor.STRUCTURE_BLOCK`.
Grid origin anchor: `startX = entranceX + 1 = 8`, `startY = entranceY + 1 = 5`. Grid cell `(x, y)` maps to horizontal world offset `relative(rot(SOUTH), 8 + (y - startY) * 8).relative(rot(EAST), (x - startX) * 8)`.

Exact emission order in `createMansion(origin, rotation, pieces, mansion)`:
1. **Entrance**: emits `"entrance"` at `origin.relative(rot(WEST), 9)` with `rotation`; advances `data.position` by `rot(SOUTH) * 16`.
2. **Outer walls (floors 1 & 2)**: `traverseOuterWalls` starting at `(8, 5)` heading `SOUTH`, stopping when back at `(8, 4)` heading `SOUTH`:
   - Floor 1: `wallType = "wall_flat"`, starting at `data.position` (`origin + 16*SOUTH`).
   - Floor 2: `wallType = "wall_window"`, starting at `data.position.above(8)`.
   - Perimeter walk step (`do ... while (gx != endX || gy != endY || dir != startDir)`):
     - If `!isHouse(gx + dir)`: emit `"wall_corner"` at `pos - 1*SOUTH`, move `pos += -7*SOUTH - 6*WEST` (in current `data.rotation`), rotate `data.rotation` CW90 and `dir` CW90; if not at end state, emit `wallType` at `pos + 7*EAST` and advance `pos += 8*SOUTH`.
     - Else if `isHouse(gx + dir) && isHouse(gx + dir + ccw)`: inner corner — move `pos += 6*SOUTH + 8*EAST`, rotate `data.rotation` CCW90, advance `(gx, gy) += dir`, `dir = dir.ccw()`.
     - Else: straight — advance `(gx, gy) += dir`; if not at end state, emit `wallType` at `pos + 7*EAST` and advance `pos += 8*SOUTH`.
3. **Outer walls (floor 3)**: `thirdData` at `data.position.above(19)`, `wallType = "wall_window"`. Scan `y = 0..10` (outer), `x = 10..0` (inner) for the first `isHouse(thirdGrid, x, y)`: shift `thirdData.position` by `(8 + (y - startY)*8)*SOUTH + (x - startX)*8*EAST`, emit one `traverseWallPiece`, then `traverseOuterWalls(..., x, y, x, y)` and stop.
4. **Roofs**: `createRoof(origin.above(16), rotation, baseGrid, thirdGrid)` then `createRoof(origin.above(27), rotation, thirdGrid, null)`:
   - Pass 1 (`y=0..10, x=0..10`): for each `isHouse(grid, x, y)` NOT covered by `aboveGrid`: emit `"roof"` at `pos.above(3)` (`rot`), plus `"roof_front"` on each non-house side: `+X` at `+6E` (`rot`), `-X` at `+7S` (`rot+180`), `-Y` at `+1W` (`rot+CCW90`), `+Y` at `+6E+6S` (`rot+CW90`).
   - Pass 2 (only if `aboveGrid != null`, `y=0..10, x=0..10`): for each cell in BOTH `grid` and `aboveGrid`: emit `"small_wall"` on non-house sides of `grid` (`+X`: `+7E, rot`; `-X`: `+1W+6S, rot+180`; `-Y`: `+1N, rot+CCW90`; `+Y`: `+6E+7S, rot+CW90`), and `"small_wall_corner"` at outer corners (`+X&-Y`: `+7E+2N, rot`; `+X&+Y`: `+8E+7S, rot+CW90`; `-X&-Y`: `+2W+1N, rot+CCW90`; `-X&+Y`: `+1W+8S, rot+180`).
   - Pass 3 (`y=0..10, x=0..10`): for each `isHouse(grid)` not covered by `aboveGrid`: emit `"roof_corner"` / `"roof_inner_corner"` on `+X` (`+6E`) and `-X` (`+0`) edges depending on `y±1` and diagonal `(x±1, y±1)` neighbors (exact offsets: `+X&!+Y` → `corner` `+6E+6S, rot`; `+X&+Y&(+1,+1)` → `inner` `+6E+5S, rot`; `+X&!-Y` → `corner` `+6E, rot+CCW90`; `+X&-Y&(+1,-1)` → `inner` `+9E+2N, rot+CW90`; `-X&!+Y` → `corner` `+6S, rot+CW90`; `-X&+Y&(-1,+1)` → `inner` `+8S+3W, rot+CCW90`; `-X&!-Y` → `corner` `+0, rot+180`; `-X&-Y&(-1,-1)` → `inner` `+1S, rot+180`).
5. **Interior floors (`floorNum = 0, 1, 2`)**: `floorOrigin = origin.above(8 * floorNum + (floorNum == 2 ? 3 : 0))`, `grid = (floorNum == 2) ? thirdGrid : baseGrid`.
   - **Corridors** (`y=0..10, x=0..10`, where `grid(x, y) == 1`): emit `"corridor_floor"` at `pos` (`rot`). Check 4 neighbors for `grid == 1 || (rooms & ROOM_CORRIDOR_FLAG) != 0`:
     - `(x, y-1)`: `"carpet_north"` at `pos + 1E + 1U`
     - `(x+1, y)`: `"carpet_east"` at `pos + 1S + 5E + 1U`
     - `(x, y+1)`: `"carpet_south_1"` (floor 0) / `"carpet_south_2"` (floors 1,2) at `pos + 5S + 1W`
     - `(x-1, y)`: `"carpet_west_1"` (floor 0) / `"carpet_west_2"` (floors 1,2) at `pos + 1W + 1N`
   - **Rooms & interior walls/doors** (`y=0..10, x=0..10`, where `grid(x, y) == 2 || (floorNum == 2 && grid(x, y) == 3)`):
     - `wallPiece = floorNum==0 ? "indoors_wall_1" : "indoors_wall_2"`, `doorPiece = floorNum==0 ? "indoors_door_1" : "indoors_door_2"`.
     - If `(roomData & ROOM_DOOR_FLAG) != 0`, collect all `dir ∈ HORIZONTAL` with `grid(x+dx, y+dz) == 1`. If non-empty, `doorDir = doorDirs[random.nextInt(size)]`; else if `(roomData & ROOM_ORIGIN_FLAG) != 0`, `doorDir = UP`; else `doorDir = null`.
     - `roomPos = floorOrigin + (8 + (y - startY)*8)*SOUTH + (-1 + (x - startX)*8)*EAST`.
     - Emit interior wall or door on 4 sides (note `thirdFloorStartRoom` suppresses `+X` and `-Y` corridor walls when `floorNum==2 && grid==3 && ROOM_CORRIDOR_FLAG`):
       - `x-1` (if `isHouse` and different `roomId`): at `roomPos` (`rot`), `doorPiece` if `doorDir == WEST`.
       - `x+1` (if `grid == 1 && !thirdFloorStartRoom`): at `roomPos + 8E` (`rot`), `doorPiece` if `doorDir == EAST`.
       - `y+1` (if `isHouse` and different `roomId`): at `roomPos + 7S + 7E` (`rot+CW90`), `doorPiece` if `doorDir == SOUTH`.
       - `y-1` (if `grid == 1 && !thirdFloorStartRoom`): at `roomPos + 1N + 7E` (`rot+CW90`), `doorPiece` if `doorDir == NORTH`.
     - **Emit room template**:
       - `1x1`: **always draws `rooms.get1x1(random)` first!** Then rotates `pieceRot` (`EAST -> NONE`, `NORTH -> CCW90`, `WEST -> 180`, `SOUTH -> CW90`), and if `doorDir` is none of those 4 (i.e. `UP` secret room), **also draws `rooms.get1x1Secret(random)`**! Computes `off = getZeroPositionWithTransform((1,0,0), NONE, pieceRot, 7, 7).rotate(rot)`, places at `roomPos + (off.x, 0, off.z)` with `pieceRot.getRotated(rot)`.
       - `1x2` (only when `doorDir != null`): gets `roomDir = get1x2RoomDirection`, `isStairs = (roomData & ROOM_STAIRS_FLAG) != 0`:
         - 8 side-entrance combinations (`doorDir ⊥ roomDir`): calls `get1x2SideEntrance(random, isStairs)` with `(offset, rot_delta, mirror)`:
           - `(E,S)`: `+1E, 0, NONE`; `(E,N)`: `+1E+6S, 0, LR`; `(W,N)`: `+7E+6S, 180, NONE`; `(W,S)`: `+7E, 0, FB`; `(S,E)`: `+1E, CW90, LR`; `(S,W)`: `+7E, CW90, NONE`; `(N,W)`: `+7E+6S, CW90, FB`; `(N,E)`: `+1E+6S, CCW90, NONE`.
         - 4 front-entrance combinations (`doorDir == -roomDir`): calls `get1x2FrontEntrance(random, isStairs)`:
           - `(S,N)`: `+1E+8N, 0, NONE`; `(N,S)`: `+7E+14S, 180, NONE`; `(W,E)`: `+15E, CW90, NONE`; `(E,W)`: `+7W+6S, CCW90, NONE`.
         - 2 secret combinations (`doorDir == UP`): calls `get1x2Secret(random)`:
           - `(UP,E)`: `+15E, CW90, NONE`; `(UP,S)`: `+1E, 0, NONE`.
       - `2x2` (`doorDir ∉ {null, UP}`): `roomDir = doorDir.getClockWise()`; if `!isRoomId(x+dx, y+dz)` flip `roomDir = roomDir.getOpposite()`. Calls `get2x2(random)` with `(east, south, rot_delta, mirror)`:
         - `(E,S)`: `(-7,0,0,NONE)`; `(E,N)`: `(-7,6,0,LR)`; `(N,E)`: `(1,14,CCW90,NONE)`; `(N,W)`: `(7,14,CCW90,LR)`; `(S,W)`: `(7,-8,CW90,NONE)`; `(S,E)`: `(1,-8,CW90,LR)`; `(W,N)`: `(15,6,180,NONE)`; `(W,S)`: `(15,0,0,FB)`.
       - `2x2` secret (`doorDir == UP`): calls `get2x2Secret(random)`, places at `roomPos + 1E` with `(rot, NONE)`.

**Room template collections (RNG draw per call)**:
| Method | Floor 1 (`FirstFloorRoomCollection`) | Floors 2 & 3 (`SecondFloorRoomCollection` / `ThirdFloorRoomCollection`) |
|---|---|---|
| `get1x1` | `"1x1_a" + (nextInt(5) + 1)` | `"1x1_b" + (nextInt(5) + 1)` |
| `get1x1Secret` | `"1x1_as" + (nextInt(4) + 1)` | `"1x1_as" + (nextInt(4) + 1)` |
| `get1x2SideEntrance` | `"1x2_a" + (nextInt(9) + 1)` | if `isStairsRoom`: `"1x2_c_stairs"` (no RNG); else `"1x2_c" + (nextInt(4) + 1)` |
| `get1x2FrontEntrance` | `"1x2_b" + (nextInt(5) + 1)` | if `isStairsRoom`: `"1x2_d_stairs"` (no RNG); else `"1x2_d" + (nextInt(5) + 1)` |
| `get1x2Secret` | `"1x2_s" + (nextInt(2) + 1)` | `"1x2_se" + (nextInt(1) + 1)` *(still calls `nextInt(1)`!)* |
| `get2x2` | `"2x2_a" + (nextInt(4) + 1)` | `"2x2_b" + (nextInt(5) + 1)` |
| `get2x2Secret` | `"2x2_s1"` (no RNG) | `"2x2_s1"` (no RNG) |

#### 6.4 Data markers & `afterPlace` cobblestone foundation
- **`handleDataMarker`**:
  - Starts with `"Chest"`: sets chest facing `rot.rotate(WEST/EAST/SOUTH/NORTH)` for `"ChestWest"/"ChestEast"/"ChestSouth"/"ChestNorth"` (else default `NORTH`) and calls `createChest(level, chunkBB, random, pos, minecraft:chests/woodland_mansion, chestState)`.
  - `"Mage"`: spawns 1 persistent `minecraft:evoker`.
  - `"Warrior"`: spawns 1 persistent `minecraft:vindicator`.
  - `"Group of Allays"`: draws **`numberOfAllays = level.getRandom().nextInt(3) + 1`** (`1..3`, from `WorldGenRegion`'s `minecraft:worldgen_region_random`!) and spawns that many persistent `minecraft:allay`s.
  - For each spawned mob: `setPersistenceRequired()`, `snapTo(pos, 0, 0)`, `finalizeSpawn(..., STRUCTURE, null)`, `addFreshEntityWithPassengers`, and `level.setBlock(pos, minecraft:air, 2)`.
- **`WoodlandMansionStructure.afterPlace`**: let `bb = pieces.calculateBoundingBox()`, `yStart = bb.minY`, `minY = level.minY`. Loop `x = chunkBB.minX..chunkBB.maxX` (outer), `z = chunkBB.minZ..chunkBB.maxZ` (inner): if `!level.isEmptyBlock(x, yStart, z) && bb.isInside(x, yStart, z) && pieces.isInsidePiece(x, yStart, z)`: scan `y = yStart - 1` down to `minY + 1` (strictly `y > minY`); stop on the first block that is `!isEmptyBlock && !liquid()`, else place `minecraft:cobblestone` (`flags = 2`).

---
### 7. Desert Pyramid (`minecraft:desert_pyramid`)

#### 7.1 Configuration schema, start generation & terrain adaptation
- Schema: `StructureSettings` only. Vanilla defaults: `step = surface_structures`, `terrain_adaptation = none`, no `spawn_overrides`.
- Extends `SinglePieceStructure(DesertPyramidPiece::new, width = 21, depth = 21)` (§1.2).
- `DesertPyramidPiece` (`tedp`): `ScatteredFeaturePiece(west, 64, north, width = 21, height = 15, depth = 21, getRandomHorizontalDirection(random))`. Tracks `hasPlacedChest[4]` (indexed by `Direction.get2DDataValue()`), ephemeral `potentialSuspiciousSandWorldPositions` list, and ephemeral `randomCollapsedRoofPos` (default `(0,0,0)`).

#### 7.2 `DesertPyramidPiece.postProcess`
Shorthand: `SS = minecraft:sandstone`, `CSS = minecraft:cut_sandstone`, `HSS = minecraft:chiseled_sandstone`, `OT = minecraft:orange_terracotta`, `BT = minecraft:blue_terracotta`.
1. **Height snap**: calls `updateHeightPositionToLowestGroundHeight(level, -random.nextInt(3))` *(note: `random.nextInt(3)` is ALWAYS evaluated as the argument BEFORE checking `heightPosition >= 0`!)* If false, return.
2. **Base & stepped pyramid**:
   - `SS` `[0,-4,0..20,0,20]`.
   - For `p = 1..9`: `SS` `[p,p,p..20-p,p,20-p]`, then `air` `[p+1,p,p+1..19-p,p,19-p]`.
   - For `x = 0..20`, `z = 0..20`: `fillColumnDown(SS, x, -5, z)`.
3. **Twin front towers** (`sandstone_stairs` N/S/E/W):
   - West tower: shell `SS` / fill `air` `[0,0,0..4,9,4]`; roof `SS` `[1,10,1..3,10,3]`; cap stairs `(2,10,0)[N]`, `(2,10,4)[S]`, `(0,10,2)[E]`, `(4,10,2)[W]`.
   - East tower: shell `SS` / fill `air` `[16,0,0..20,9,4]`; roof `SS` `[17,10,1..19,10,3]`; cap stairs `(18,10,0)[N]`, `(18,10,4)[S]`, `(16,10,2)[E]`, `(20,10,2)[W]`.
4. **Main entrance & hallways**:
   - Entryway: `SS`/`air` `[8,0,0..12,4,4]`, `air` `[9,1,0..11,3,4]`, arch `CSS` at `(9,1..3,1)`, `(10,3,1)`, `(11,1..3,1)`.
   - Side halls: `SS`/`air` `[4,1,1..8,3,3]` + `air` `[4,1,2..8,2,2]`; `SS`/`air` `[12,1,1..16,3,3]` + `air` `[12,1,2..16,2,2]`.
5. **Interior floor, pillars, balconies & tower stairs**:
   - 2nd floor `SS` `[5,4,5..15,4,15]` with center opening `air` `[9,4,9..11,4,11]`.
   - 4 central pillars `CSS`: `(8|12, 1..3, 8|12)`.
   - Side solid blocks `SS`: `[1,1,5..4,4,11]`, `[16,1,5..19,4,11]`; upper ledges `SS` `[6,7,9..6,7,11]`, `[14,7,9..14,7,11]`; upper frames `CSS` `[5,5,9..5,7,11]`, `[15,5,9..15,7,11]` with `air` openings `(5,5..6,10)`, `(6,6,10)`, `(15,5..6,10)`, `(14,6,10)`.
   - Tower doors `air` `[2,4,4..2,6,4]` & `[18,4,4..18,6,4]`; steps `stairs[N]` at `(2,4,5)`, `(2,3,4)`, `(18,4,5)`, `(18,3,4)`.
   - Tower interior staircases: `SS` `[1,1,3..2,2,3]` & `[18,1,3..19,2,3]`; `SS` `(1,1,2)` & `(19,1,2)`; `sandstone_slab` `(1,2,2)` & `(19,2,2)`; `stairs[W]` `(2,1,2)`, `stairs[E]` `(18,1,2)`.
   - Side colonnades: `SS` `[4,3,5..4,3,17]` & `[16,3,5..16,3,17]`; `air` `[3,1,5..4,2,16]` & `[15,1,5..16,2,16]`; for `z = 5..17 step 2`: `CSS` at `(4,1,z)` & `(16,1,z)`, `HSS` at `(4,2,z)` & `(16,2,z)`.
6. **Floor terracotta mosaic (`y = 0`)**: `BT` at `(10,0,10)`; `OT` at `(10,0,7)`, `(10,0,8)`, `(9,0,9)`, `(11,0,9)`, `(8,0,10)`, `(12,0,10)`, `(7,0,10)`, `(13,0,10)`, `(9,0,11)`, `(11,0,11)`, `(10,0,12)`, `(10,0,13)`.
7. **Tower ankh facades**:
   - Side faces (`x ∈ {0, 20}`, `z = 1..3`, `y = 2..8`): rows `y=2,3,5`: `(CSS, OT, CSS)`; `y=4,6`: `(OT, HSS, OT)`; `y=7`: `(OT, OT, OT)`; `y=8`: `(CSS, CSS, CSS)`.
   - Front faces (`x ∈ {2, 18}`, `x-1..x+1`, `z = 0`, `y = 2..8`): identical 3×7 pattern.
   - Front lintel: `CSS` `[8,4,0..12,6,0]`, `air` at `(8,6,0)` & `(12,6,0)`, `OT` at `(9,5,0)` & `(11,5,0)`, `HSS` at `(10,5,0)`.
8. **Hidden treasure chamber & TNT trap**:
   - Shaft shell: `CSS` `[8,-14,8..12,-11,12]`, `HSS` `[8,-10,8..12,-10,12]`, `CSS` `[8,-9,8..12,-9,12]`, `SS` `[8,-8,8..12,-1,12]`, interior `air` `[9,-11,9..11,-1,11]`.
   - `minecraft:stone_pressure_plate` at `(10, -11, 10)`.
   - `generateBox(9, -13, 9, 11, -13, 11, minecraft:tnt, air, false)` (all 9 cells are edges since `y0 == y1`, placing 9 TNT blocks).
   - 4 chest alcoves (`(8,10)/(7,10)`, `(12,10)/(13,10)`, `(10,8)/(10,7)`, `(10,12)/(10,13)`): inner wall `air` at `y=-11,-10`; outer wall `HSS` at `y=-10`, `CSS` at `y=-11`.
   - **4 chests**: iterate `dir` in `Direction.Plane.HORIZONTAL` (`[N, E, S, W]`): if `!hasPlacedChest[dir.get2DDataValue()]`: `hasPlacedChest[...] = createChest(10 + 2*dir.dx, -11, 10 + 2*dir.dz, minecraft:chests/desert_pyramid)`.
9. **Archaeology cellar (`addCellar`)** centered at `roomCenter = (16, -4, 13)` (`x=16, y=-4, z=13`):
   - **`addCellarStairs`**:
     - 3 stairs (`sandstone_stairs[facing=west]`, from `default.rotate(CCW90)`): `(13,-1,17)`, `(14,-2,17)`, `(15,-3,17)`.
     - Draws **`variant = level.getRandom().nextBoolean()`** (uses `WorldGenRegion`'s `minecraft:worldgen_region_random`!).
     - Places `sand` at `(12..16, 0, 17)`, `(14, -1, 17)`, `(15, -2, 17)`, `(16, -3, 17)`; `SS` at `(16, -2, 17)`; at `(15, -1, 17)` places `variant ? sand : SS`; at `(16, -1, 17)` places `!variant ? sand : SS`.
   - **`addCellarRoom`**:
     - Walls (`skipAir = true`!): `CSS` at `y=-3`, `HSS` at `y=-2`, `CSS` at `y=-1` on 4 boxes: `[13,Y,10..13,Y,15]`, `[19,Y,10..19,Y,15]`, `[13,Y,10..19,Y,11]`, `[13,Y,16..19,Y,16]`.
     - Record 5×3×5 `placeSandBox(14, -3, 11, 18, -1, 15)` (world positions appended to `potentialSuspiciousSandWorldPositions` in `y -> x -> z` order).
     - **`placeCollapsedRoof(14, 0, 11, 18, 15)`**:
       - Loop `rx = 14..18` (outer), `rz = 11..15` (inner): draws **`level.getRandom().nextFloat() < 0.33f`** → `SS`, else `sand`, placing at `(rx, 0, rz)`.
       - Creates standalone legacy positional RNG `r = RandomSource.createThreadLocalInstance(level.getSeed()).forkPositional().at(getWorldPos(14, 0, 11))`; draws `roofPosX = r.nextIntBetweenInclusive(14, 18)`, `roofPosZ = r.nextIntBetweenInclusive(11, 15)`; sets `randomCollapsedRoofPos = getWorldPos(roofPosX, 0, roofPosZ)`.
     - Cellar floor terracotta (`y = -4`): `BT` `(16,-4,13)`; `OT` `(17,-4,12)`, `(17,-4,14)`, `(15,-4,12)`, `(15,-4,14)`, `(18,-4,13)`, `(14,-4,13)`, `(16,-4,15)`, `(16,-4,11)`, `(19,-4,13)`, `(13,-4,13)`, `(16,-4,16)`, `(16,-4,10)`.
     - 4 alcove sand pairs (appended to `potentialSuspiciousSandWorldPositions`) & back walls: `(19,-3..-2,13)` + `CSS(20,-3,13), HSS(20,-2,13)`; `(13,-3..-2,13)` + `CSS(12,-3,13), HSS(12,-2,13)`; `(16,-3..-2,16)`; `(16,-3..-2,10)` + `CSS(16,-3,9), HSS(16,-2,9)` *(hardcoded `-2` in reference equals `y + 2` since `y = -4`)*.

#### 7.3 `DesertPyramidStructure.afterPlace` (suspicious sand)
1. Collects all `potentialSuspiciousSandWorldPositions` across all `DesertPyramidPiece`s into a `SortedArraySet` ordered by `Vec3i::compareTo` (Y, then Z, then X), and immediately places `minecraft:suspicious_sand` at each piece's `randomCollapsedRoofPos` (if inside `chunkBB`, setting `brushable_block` loot table `minecraft:archaeology/desert_pyramid` with seed `blockPos.asLong()`).
2. Copies sorted set into a list, creates standalone legacy positional RNG `posRng = RandomSource.createThreadLocalInstance(level.getSeed()).forkPositional().at(pieces.calculateBoundingBox().getCenter())`, shuffles via `Util.shuffle(list, posRng)`, and draws `count = min(list.size(), posRng.nextInt(5, 8))` (`5`, `6`, or `7` blocks).
3. Iterates the shuffled list: for the first `count` positions, places `suspicious_sand` (with loot table `minecraft:archaeology/desert_pyramid`, seed `blockPos.asLong()`) if inside `chunkBB`; for all remaining positions, places `minecraft:sand` (`flags = 2`) if inside `chunkBB`.

---

### 8. Jungle Temple (`minecraft:jungle_temple`)

#### 8.1 Configuration schema, start generation & terrain adaptation
- Schema: `StructureSettings` only. Vanilla defaults: `step = surface_structures`, `terrain_adaptation = none`, no `spawn_overrides`.
- Extends `SinglePieceStructure(JungleTemplePiece::new, width = 12, depth = 15)` (§1.2).
- `JungleTemplePiece` (`tejp`): `ScatteredFeaturePiece(west, 64, north, width = 12, height = 10, depth = 15, getRandomHorizontalDirection(random))`. Tracks booleans `placedMainChest`, `placedHiddenChest`, `placedTrap1`, `placedTrap2`.
- **`MossStoneSelector`**: `random.nextFloat() < 0.4f` → `minecraft:cobblestone`, else `minecraft:mossy_cobblestone` (ignores `isEdge`).

#### 8.2 `JungleTemplePiece.postProcess`
All `STONE_SELECTOR` calls below use `skipAir = false` (so every cell in every box consumes 1 `nextFloat()` in `y -> x -> z` order regardless of `chunkBB`).
1. If `!updateAverageGroundHeight(level, chunkBB, 0)`, return.
2. **Outer stone shell** (`STONE_SELECTOR`, exact call order):
   - `[0,-4,0..11,0,14]`, `[2,1,2..9,2,2]`, `[2,1,12..9,2,12]`, `[2,1,3..2,2,11]`, `[9,1,3..9,2,11]`, `[1,3,1..10,6,1]`, `[1,3,13..10,6,13]`, `[1,3,2..1,6,12]`, `[10,3,2..10,6,12]`, `[2,3,2..9,3,12]`, `[2,6,2..9,6,12]`, `[3,7,3..8,7,11]`, `[4,8,4..7,8,10]`.
3. **Interior air voids** (`generateAirBox`): `[3,1,3..8,2,11]`, `[4,3,6..7,3,9]`, `[2,4,2..9,5,12]`, `[4,6,5..7,6,9]`, `[5,7,6..6,7,8]`, `[5,1,2..6,2,2]`, `[5,2,12..6,2,12]`, `[5,5,1..6,5,1]`, `[5,5,13..6,5,13]`, plus `air` at `(1,5,5)`, `(10,5,5)`, `(1,5,9)`, `(10,5,9)`.
4. **Exterior pilasters & roof battlements** (`STONE_SELECTOR`, exact order):
   - For `z ∈ {0, 14}`: `[2,4,z..2,5,z]`, `[4,4,z..4,5,z]`, `[7,4,z..7,5,z]`, `[9,4,z..9,5,z]`.
   - `[5,6,0..6,6,0]`.
   - For `x ∈ {0, 11}`: for `z = 2..12 step 2` `[x,4,z..x,5,z]`, then `[x,6,5..x,6,5]`, `[x,6,9..x,6,9]`.
   - Roof merlons: `[2,7,2..2,9,2]`, `[9,7,2..9,9,2]`, `[2,7,12..2,9,12]`, `[9,7,12..9,9,12]`, `[4,9,4]`, `[7,9,4]`, `[4,9,10]`, `[7,9,10]`, `[5,9,7..6,9,7]`.
5. **Stairs & landings** (`cobblestone_stairs` E/W/S/N):
   - Roof: `(5..6, 9, 6)[N]`, `(5..6, 9, 8)[S]`. Entry: `(4..7, 0, 0)[N]`. Upper stairs: `(4,1,8)[N]`, `(4,2,9)[N]`, `(4,3,10)[N]`, `(7,1,8)[N]`, `(7,2,9)[N]`, `(7,3,10)[N]`.
   - Under-stair supports (`STONE_SELECTOR`): `[4,1,9]`, `[7,1,9]`, `[4,1,10..7,2,10]`, `[5,4,5..6,4,5]`. Upper trim: `(4,4,5)[E]`, `(7,4,5)[W]`.
   - Basement descent (`i = 0..3`): `stairs[S]` at `(5..6, -i, 6+i)`, `generateAirBox(5, -i, 7+i, 6, -i, 9+i)`.
6. **Basement corridors & ribs**:
   - `generateAirBox`: `[1,-3,12..10,-1,13]`, `[1,-3,1..3,-1,13]`, `[1,-3,1..9,-1,5]`.
   - `STONE_SELECTOR`: for `z = 1..13 step 2` `[1,-3,z..1,-2,z]`; for `z = 2..12 step 2` `[1,-1,z..3,-1,z]`; then `[2,-2,1..5,-2,1]`, `[7,-2,1..9,-2,1]`, `[6,-3,1]`, `[6,-1,1]`.
7. **Arrow trap 1**:
   - `tripwire_hook[attached=true]`: `(1,-3,8)[east]`, `(4,-3,8)[west]`.
   - `tripwire[east=true, west=true, attached=true]`: `(2,-3,8)`, `(3,-3,8)`.
   - `redstone_wire`: `(5,-3,2..7)` `[north=side, south=side]`, `(5,-3,1)` `[north=side, west=side]`, `(4,-3,1)` `[east=side, west=side]`.
   - `mossy_cobblestone` `(3,-3,1)`; if `!placedTrap1`, `placedTrap1 = createDispenser(3, -2, 1, NORTH, minecraft:chests/jungle_temple_dispenser)`; `vine[south=true]` `(3,-2,2)`.
8. **Arrow trap 2 & main chest**:
   - `tripwire_hook[attached=true]`: `(7,-3,1)[north]`, `(7,-3,5)[south]`.
   - `tripwire[north=true, south=true, attached=true]`: `(7,-3,2..4)`.
   - `redstone_wire`: `(8,-3,6)` `[east=side, west=side]`, `(9,-3,6)` `[west=side, south=side]`, `(9,-3,5)` `[north=side, south=up]`; `mossy_cobblestone` `(9,-3,4)`; `redstone_wire[n=side, s=side]` `(9,-2,4)`.
   - If `!placedTrap2`, `placedTrap2 = createDispenser(9, -2, 3, WEST, minecraft:chests/jungle_temple_dispenser)`; `vine[east=true]` at `(8,-1,3)` & `(8,-2,3)`.
   - If `!placedMainChest`, `placedMainChest = createChest(8, -3, 3, minecraft:chests/jungle_temple)`.
   - `mossy_cobblestone` at `(9,-3,2)`, `(8,-3,1)`, `(4,-3,5)`, `(5,-2,5)`, `(5,-1,5)`, `(6,-3,5)`, `(7,-2,5)`, `(7,-1,5)`, `(8,-3,5)`.
   - `STONE_SELECTOR`: `[9,-1,1..9,-1,5]`.
9. **Lever puzzle & hidden chest**:
   - `generateAirBox(8, -3, 8, 10, -1, 10)`.
   - `chiseled_stone_bricks` at `(8..10, -2, 11)`; `lever[facing=north, face=wall]` at `(8..10, -2, 12)`.
   - `STONE_SELECTOR`: `[8,-3,8..8,-3,10]`, `[10,-3,8..10,-3,10]`.
   - `mossy_cobblestone` `(10,-2,9)`; `redstone_wire[n=side, s=side]` `(8,-2,9..10)`; `redstone_wire[n=side, s=side, e=side, w=side]` `(10,-1,9)`.
   - `sticky_piston`: `(9,-2,8)[up]`, `(10,-2,8)[west]`, `(10,-1,8)[west]`.
   - `repeater[facing=north]` `(10,-2,10)`.
   - If `!placedHiddenChest`, `placedHiddenChest = createChest(9, -3, 10, minecraft:chests/jungle_temple)`.

---

### 9. Swamp Hut (`minecraft:swamp_hut`)

#### 9.1 Configuration schema, start generation & spawn overrides
- Schema: `StructureSettings` only. Vanilla defaults: `step = surface_structures`, `terrain_adaptation = none`, `spawn_overrides = { monster(piece): witch wt 1 (1), creature(piece): cat wt 1 (1) }`.
- `findGenerationPoint`: `onTopOfChunkCenter(context, WORLD_SURFACE_WG, builder -> builder.addPiece(SwampHutPiece(random, chunk.minBlockX, chunk.minBlockZ)))`.
- `SwampHutPiece` (`tesh`): `ScatteredFeaturePiece(west, 64, north, width = 7, height = 7, depth = 9, getRandomHorizontalDirection(random))`. Tracks `spawnedWitch` (`"Witch"`) and `spawnedCat` (`"Cat"`).

#### 9.2 `SwampHutPiece.postProcess`
1. If `!updateAverageGroundHeight(level, chunkBB, 0)`, return.
2. `spruce_planks`: floor `[1,1,1..5,1,7]`, ceiling `[1,4,2..5,4,7]`, porch `[2,1,0..4,1,0]`, walls `[2,2,2..3,3,2]`, `[1,2,3..1,3,6]`, `[5,2,3..5,3,6]`, `[2,2,7..4,3,7]`.
3. Corner stilts `oak_log`: `[1,0,2..1,3,2]`, `[5,0,2..5,3,2]`, `[1,0,7..1,3,7]`, `[5,0,7..5,3,7]`.
4. Details: `oak_fence` `(2,3,2)`, `(3,3,7)`, `(1,2,1)`, `(5,2,1)`; `air` `(1,3,4)`, `(5,3,4)`, `(5,3,5)`; `potted_red_mushroom` `(1,3,5)`; `crafting_table` `(3,2,6)`; `cauldron` `(4,2,6)`.
5. Roof trim (`spruce_stairs`): `[0,4,1..6,4,1][N]`, `[0,4,2..0,4,7][E]`, `[6,4,2..6,4,7][W]`, `[0,4,8..6,4,8][S]`; outer corners: `(0,4,1)[N, outer_right]`, `(6,4,1)[N, outer_left]`, `(0,4,8)[S, outer_left]`, `(6,4,8)[S, outer_right]`.
6. Stilts down: for `z ∈ {2, 7}`, `x ∈ {1, 5}`: `fillColumnDown(oak_log, x, -1, z)`.
7. **Entity spawns** (both at local `(2, 2, 5)` → `(worldPos.x + 0.5, worldPos.y, worldPos.z + 0.5)` if inside `chunkBB`, persistent, `finalizeSpawn(..., STRUCTURE, null)`, `addFreshEntityWithPassengers`):
   - If `!spawnedWitch`: set `spawnedWitch = true`, spawn `minecraft:witch`.
   - If `!spawnedCat`: set `spawnedCat = true`, spawn `minecraft:cat` (note: cat variant selection reads `SwampHut` context during `finalizeSpawn` to become black).

---

### 10. Igloo (`minecraft:igloo`)

#### 10.1 Configuration schema, start generation & template assembly
- Schema: `StructureSettings` only. Vanilla defaults: `step = surface_structures`, `terrain_adaptation = none`, no `spawn_overrides`.
- `findGenerationPoint`: `onTopOfChunkCenter(context, WORLD_SURFACE_WG, ...)`:
  1. `startPos = (chunk.minBlockX, 90, chunk.minBlockZ)`, `rotation = Rotation.getRandom(random)`.
  2. **50% basement**: if `random.nextDouble() < 0.5`:
     - `depth = random.nextInt(8) + 4` (`4..11`).
     - Add `IglooPiece("minecraft:igloo/bottom", startPos, rotation, depth * 3)`.
     - For `i = 0 .. depth - 2`: add `IglooPiece("minecraft:igloo/middle", startPos, rotation, i * 3)`.
  3. Always add `IglooPiece("minecraft:igloo/top", startPos, rotation, 0)`.

#### 10.2 `IglooPiece` (`iglu`) settings, dynamic Y & markers
- Templates (loaded from user's jar):
  - `minecraft:igloo/top`: pivot `(3, 5, 5)`, offset `(0, 0, 0)`.
  - `minecraft:igloo/middle`: pivot `(1, 3, 1)`, offset `(2, -3, 4)`.
  - `minecraft:igloo/bottom`: pivot `(3, 6, 7)`, offset `(0, -3, -2)`.
- `templatePosition = startPos + offset - (0, depthOffset, 0)`.
- `StructurePlaceSettings`: `rotation`, `mirror = NONE`, `rotationPivot = PIVOTS[template]`, processor `BlockIgnoreProcessor.STRUCTURE_BLOCK`, `liquidSettings = IGNORE_WATERLOGGING`.
- **`postProcess`**:
  1. Compute `entrancePos = templatePosition + calculateRelativePosition(settings, (3 - offset.x, 0, -offset.z))`.
  2. `height = level.getHeight(WORLD_SURFACE_WG, entrancePos.x, entrancePos.z)`.
  3. Temporarily shift `templatePosition` by `(0, height - 90 - 1, 0)` and invoke `TemplateStructurePiece.postProcess`.
  4. If this piece is `minecraft:igloo/top`: compute `trapDoorPos = templatePosition + calculateRelativePosition(settings, (3, 0, 5))`; read `below = level.getBlockState(trapDoorPos.below())`; if `!below.isAir() && !below.is(minecraft:ladder)`, replace `trapDoorPos` with `minecraft:snow_block` (`setBlockAndUpdate`).
  5. Restore `templatePosition` to its original (unshifted) value!
- **`handleDataMarker`**: if `"chest"`: set `pos` to `minecraft:air` (`setBlockAndUpdate`), and if `pos.below()` is a `ChestBlockEntity`, set loot table `minecraft:chests/igloo_chest` with `random.nextLong()`.

---

### 11. Shipwreck (`minecraft:shipwreck`)

#### 11.1 Configuration schema & template pools
- Schema: `StructureSettings` + `is_beached: bool` (required).
- Vanilla defaults: `step = surface_structures`, `terrain_adaptation = none`, no `spawn_overrides`.
- Templates (all under `minecraft:shipwreck/<name>`, loaded from user's jar, pivot `(4, 0, 15)`):
  - **`STRUCTURE_LOCATION_BEACHED` (11)**: `with_mast`, `sideways_full`, `sideways_fronthalf`, `sideways_backhalf`, `rightsideup_full`, `rightsideup_fronthalf`, `rightsideup_backhalf`, `with_mast_degraded`, `rightsideup_full_degraded`, `rightsideup_fronthalf_degraded`, `rightsideup_backhalf_degraded`.
  - **`STRUCTURE_LOCATION_OCEAN` (20)**: `with_mast`, `upsidedown_full`, `upsidedown_fronthalf`, `upsidedown_backhalf`, `sideways_full`, `sideways_fronthalf`, `sideways_backhalf`, `rightsideup_full`, `rightsideup_fronthalf`, `rightsideup_backhalf`, `with_mast_degraded`, `upsidedown_full_degraded`, `upsidedown_fronthalf_degraded`, `upsidedown_backhalf_degraded`, `sideways_full_degraded`, `sideways_fronthalf_degraded`, `sideways_backhalf_degraded`, `rightsideup_full_degraded`, `rightsideup_fronthalf_degraded`, `rightsideup_backhalf_degraded`.

#### 11.2 Start generation, height fitting & markers (`shipwreck`)
- `findGenerationPoint`: `onTopOfChunkCenter(context, isBeached ? WORLD_SURFACE_WG : OCEAN_FLOOR_WG, ...)`:
  1. `rotation = Rotation.getRandom(random)`, `offset = (chunk.minBlockX, 90, chunk.minBlockZ)`.
  2. Pick `templateId = Util.getRandom(isBeached ? BEACHED : OCEAN, random)`. Construct `ShipwreckPiece` with `mirror = NONE`, `pivot = (4, 0, 15)`, processor `BlockIgnoreProcessor.STRUCTURE_AND_AIR`, `heightAdjusted = false`.
  3. Check `piece.isTooBigToFitInWorldGenRegion()`: **`template.size.x > 32 || template.size.y > 32`** *(quirk: checks `Y`, not `Z`, against 32!)*. If true (for oversized data-pack templates):
     - If `isBeached`: `minY = Structure.getLowestY(context, bb.minX, bb.getXSpan(), bb.minZ, bb.getZSpan())`, `h = minY - template.size.y / 2 - random.nextInt(3)`.
     - Else: `h = Structure.getMeanFirstOccupiedHeight(context, bb.minX, bb.getXSpan(), bb.minZ, bb.getZSpan())`.
     - Set `templatePosition.y = h`, `heightAdjusted = true`.
- **`ShipwreckPiece.postProcess`**:
  - If `!heightAdjusted && !isTooBigToFitInWorldGenRegion()`:
    - Let `hm = isBeached ? WORLD_SURFACE_WG : OCEAN_FLOOR_WG`, `sz = template.size`, `baseSize = sz.x * sz.z`.
    - Note: iterates **unrotated** `(templatePosition) .. (templatePosition + (sz.x - 1, 0, sz.z - 1))` via `BlockPos.betweenClosed`, accumulating `mean` and `minY` from `level.getHeight(hm, p.x, p.z)`. (If `baseSize == 0`, uses single point `templatePosition`.)
    - `mean /= baseSize` (truncating int div).
    - `newY = isBeached ? (minY - sz.y / 2 - random.nextInt(3)) : mean`; set `templatePosition.y = newY`, `heightAdjusted = true`.
  - Invoke `TemplateStructurePiece.postProcess`.
- **`handleDataMarker`**: calls `RandomizableContainer.setBlockEntityLootTable(level, random, pos.below(), lootTable)` for:
  - `"map_chest"` → `minecraft:chests/shipwreck_map`
  - `"treasure_chest"` → `minecraft:chests/shipwreck_treasure`
  - `"supply_chest"` → `minecraft:chests/shipwreck_supply`

---

### 12. Ocean Ruins (`minecraft:ocean_ruin`)

#### 12.1 Configuration schema & template pools
- Schema: `StructureSettings` + `biome_temp: "warm" | "cold"`, `large_probability: f32 ∈ [0,1]`, `cluster_probability: f32 ∈ [0,1]` (all required).
- Vanilla defaults: `step = surface_structures`, `terrain_adaptation = none`, no `spawn_overrides`. Both `ocean_ruin_cold` and `ocean_ruin_warm` use `large_probability = 0.3f`, `cluster_probability = 0.9f`.
- Templates (all under `minecraft:underwater_ruin/<name>`, loaded from user's jar):
  - `WARM_RUINS` (8): `warm_1..warm_8`; `BIG_WARM_RUINS` (4): `big_warm_4..big_warm_7`.
  - `RUINS_BRICK` (8): `brick_1..brick_8`; `RUINS_CRACKED` (8): `cracked_1..cracked_8`; `RUINS_MOSSY` (8): `mossy_1..mossy_8`.
  - `BIG_RUINS_BRICK` (4): `big_brick_1, big_brick_2, big_brick_3, big_brick_8`; `BIG_RUINS_CRACKED` (4): `big_cracked_1,2,3,8`; `BIG_RUINS_MOSSY` (4): `big_mossy_1,2,3,8`.

#### 12.2 Start generation, cluster placement & piece stacking
- `findGenerationPoint`: `onTopOfChunkCenter(context, OCEAN_FLOOR_WG, ...)`:
  1. `offset = (chunk.minBlockX, 90, chunk.minBlockZ)`, `rotation = Rotation.getRandom(random)`.
  2. `isLarge = (random.nextFloat() <= structure.largeProbability)`; `baseIntegrity = isLarge ? 0.9f : 0.8f`.
  3. Call `addPiece(offset, rotation, isLarge, baseIntegrity)`:
     - **`WARM`**: pick 1 template (`Util.getRandom(isLarge ? BIG_WARM_RUINS : WARM_RUINS, random)`), add 1 `OceanRuinPiece(..., baseIntegrity, WARM, isLarge)`.
     - **`COLD`**: draw `idx = random.nextInt(isLarge ? 4 : 8)` and add **3 overlaid `OceanRuinPiece`s** at the exact same `(offset, rotation)`: `bricks[idx]` (`integrity = baseIntegrity`), `cracked[idx]` (`integrity = 0.7f`), `mossy[idx]` (`integrity = 0.5f`).
  4. **Cluster ruins**: if `isLarge && random.nextFloat() <= structure.clusterProbability`:
     - `parentPos = (offset.x, 90, offset.z)`, `parentCorner = transform((15,0,15), NONE, rotation, ZERO) + parentPos`, `parentBB = fromCorners(parentPos, parentCorner)`, `parentBL = (min(px,cx), 90, min(pz,cz))`.
     - Build 8 candidate positions (`allPositions`, exact RNG order using `Mth.nextInt(random, lo, hi)`):
       1. `(-16 + [1..8], 0, +16 + [1..7])`
       2. `(-16 + [1..8], 0, [1..7])`
       3. `(-16 + [1..8], 0, -16 + [4..8])`
       4. `([1..7], 0, +16 + [1..7])`
       5. `([1..7], 0, -16 + [4..6])`
       6. `(+16 + [1..7], 0, +16 + [3..8])`
       7. `(+16 + [1..7], 0, [1..7])`
       8. `(+16 + [1..7], 0, -16 + [4..8])`
     - Draw `ruins = Mth.nextInt(random, 4, 8)`. Loop `i = 0 .. ruins - 1`:
       - Remove `pos = allPositions.remove(random.nextInt(allPositions.size()))`.
       - `nextRot = Rotation.getRandom(random)`.
       - `nextCorner = transform((5,0,6), NONE, nextRot, ZERO) + pos`, `nextBB = fromCorners(pos, nextCorner)`.
       - If `!nextBB.intersects(parentBB)`: call `addPiece(pos, nextRot, false, 0.8f)`.

#### 12.3 `OceanRuinPiece` (`orp`) processors, height adjustment & markers
- `StructurePlaceSettings`: `rotation`, `mirror = NONE`, processors in exact order:
  1. `BlockRotProcessor(integrity)`
  2. `BlockIgnoreProcessor.STRUCTURE_AND_AIR`
  3. Archaeology processor: `CappedProcessor(RuleProcessor([ProcessorRule(BlockMatchTest(candidate), AlwaysTrueTest, PosAlwaysTrueTest, replacement, AppendLoot(lootTable))]), ConstantInt(5))`:
     - `WARM`: `sand` → `suspicious_sand`, loot `minecraft:archaeology/ocean_ruin_warm`.
     - `COLD`: `gravel` → `suspicious_gravel`, loot `minecraft:archaeology/ocean_ruin_cold`.
- **`postProcess`**:
  1. `h = level.getHeight(OCEAN_FLOOR_WG, tp.x, tp.z)`; set `tp.y = h`.
  2. `corner = transform((size.x - 1, 0, size.z - 1), NONE, rot, ZERO) + tp`.
  3. Adjust `tp.y = getHeight(tp, level, corner)`:
     - Let `newY = tp.y`, `minY = 512`, `topY = newY - 1`, `area = 0`.
     - For each `p` in `BlockPos.betweenClosed(tp, corner)`: start `floorY = tp.y - 1`; while `(state.isAir() || fluid.is(#water) || state.is(#ice)) && floorY > level.minY + 1`, decrement `floorY`. Update `minY = min(minY, floorY)`; if `floorY < topY - 2`, `area++`.
     - `width = |tp.x - corner.x|`. If `topY - minY > 2 && area > width - 2`, return `minY + 1`; else `newY`.
  4. Invoke `TemplateStructurePiece.postProcess`.
- **`handleDataMarker`**:
  - `"chest"`: places `minecraft:chest[waterlogged = fluid.is(#water)]` (`flags = 2`), sets loot table `isLarge ? minecraft:chests/underwater_ruin_big : minecraft:chests/underwater_ruin_small` with `random.nextLong()`.
  - `"drowned"`: spawns persistent `minecraft:drowned` at `pos` (`finalizeSpawn(..., STRUCTURE, null)`, `addFreshEntityWithPassengers`), then replaces `pos` with `minecraft:air` if `pos.y > level.seaLevel` else `minecraft:water` (`flags = 2`).

---

### 13. Buried Treasure (`minecraft:buried_treasure`)

#### 13.1 Configuration schema, start generation & placement (`btp`)
- Schema: `StructureSettings` only. Vanilla defaults: `step = underground_structures`, `terrain_adaptation = none`, no `spawn_overrides`.
- `findGenerationPoint`: `onTopOfChunkCenter(context, OCEAN_FLOOR_WG, builder -> builder.addPiece(BuriedTreasurePiece((chunk.blockX(9), 90, chunk.blockZ(9)))))`.
- `BuriedTreasurePiece` (`btp`): `orientation = null`, initial `boundingBox = BoundingBox(offset)` (1×1×1 at `(chunkX+9, 90, chunkZ+9)`).
- **`postProcess`**:
  1. `y = level.getHeight(OCEAN_FLOOR_WG, bb.minX, bb.minZ)`; `pos = (bb.minX, y, bb.minZ)`.
  2. While `pos.y > level.minY`:
     - `cur = level.getBlockState(pos)`, `below = level.getBlockState(pos.below())`.
     - If `below` is one of `{sandstone, stone, andesite, granite, diorite}`:
       - `softState = (!cur.isAir() && !cur.is(water) && !cur.is(lava)) ? cur : minecraft:sand`.
       - For each `dir` in `Direction.values()` (`DOWN, UP, NORTH, SOUTH, WEST, EAST`):
         - `rel = pos.relative(dir)`, `relState = level.getBlockState(rel)`.
         - If `relState.isAir() || relState.is(water) || relState.is(lava)`:
           - `belowRelState = level.getBlockState(rel.below())`.
           - If `(belowRelState.isAir() || belowRelState.is(water) || belowRelState.is(lava)) && dir != UP`: `level.setBlockAndUpdate(rel, below)`.
           - Else: `level.setBlockAndUpdate(rel, softState)`.
         *(Note: neighbor writes do NOT check `chunkBB.isInside`, though `chunkX+9, chunkZ+9` is always well inside the 16×16 chunk.)*
       - Update `this.boundingBox = BoundingBox(pos)`.
       - Call `createChest(level, chunkBB, random, pos, minecraft:chests/buried_treasure, null)` and return.
     - Else `pos.y -= 1`.

---

### 14. End City (`minecraft:end_city`)

#### 14.1 Configuration schema, start check & terrain adaptation
- Schema: `StructureSettings` only. Vanilla defaults: `step = surface_structures`, `terrain_adaptation = none`, no `spawn_overrides`.
- `findGenerationPoint(context)`:
  1. `blockX = chunk.blockX(7)`, `blockZ = chunk.blockZ(7)`. If `!couldValidBiomeExistInTerrainColumn(blockX, blockZ)`, return empty.
  2. `rotation = Rotation.getRandom(random)`; `startPos = getLowestYIn5by5Box(context, blockX, blockZ, rotation)`.
  3. If `startPos.y < 60`, return empty. Else return stub at `startPos` calling `EndCityPieces.startHouseTower(templateManager, startPos, rotation, pieces, random)`.

#### 14.2 Recursive section-generator tree (`MAX_GEN_DEPTH = 8`)
All `EndCityPiece` (`ecp`) templates load `minecraft:end_city/<name>` with `StructurePlaceSettings`: `ignoreEntities = true`, `rotation`, processor `overwrite ? BlockIgnoreProcessor.STRUCTURE_BLOCK : BlockIgnoreProcessor.STRUCTURE_AND_AIR` (`"OW"` in NBT).
Helper `addPiece(parent, offset, name, rot, overwrite)`: constructs child at `parent.templatePosition`, computes `delta = parent.template.calculateConnectedPosition(parent.placeSettings, offset, child.placeSettings, ZERO)`, moves child by `delta`.

**`recursiveChildren(generator, genDepth, parent, offset, pieces, random)`**:
1. If `genDepth > 8`, return `false`.
2. Allocate empty `childPieces` list; invoke `generator.generate(genDepth, parent, offset, childPieces, random)`.
3. If it returns `true`:
   - Draw `childTag = random.nextInt()`.
   - For each `child` in `childPieces`: set `child.genDepth = childTag`; check `col = StructurePiece.findCollisionPiece(pieces, child.boundingBox)`; if `col != null && col.genDepth != parent.genDepth`, mark `collision = true` and break *(note: `genDepth` is overwritten with `childTag` for grouping, so siblings in the same section share `genDepth` and are allowed to overlap their immediate parent's group!)*.
   - If `!collision`: append all `childPieces` to `pieces` and return `true`.
4. Return `false`.

**Entry point `startHouseTower`**: resets `shipCreated = false` on `TOWER_BRIDGE_GENERATOR`. Appends:
1. `"base_floor"` at `origin` (`rot, ow=true`, `genDepth=0`)
2. `"second_floor_1"` at `(-1, 0, -1)` (`rot, ow=false`)
3. `"third_floor_1"` at `(-1, 4, -1)` (`rot, ow=false`)
4. `"third_roof"` at `(-1, 8, -1)` (`rot, ow=true`)
5. Calls `recursiveChildren(TOWER_GENERATOR, 1, lastPiece, null, pieces, random)`.

**4 Section Generators**:
1. **`HOUSE_TOWER_GENERATOR`**: if `genDepth > 8` return `false`. Adds `"base_floor"` at `offset` (`rot, ow=true`). Draws `numFloors = random.nextInt(3)`:
   - `0`: adds `"base_roof"` at `(-1, 4, -1)` (`ow=true`).
   - `1`: `"second_floor_2"` `(-1,0,-1, ow=false)`, `"second_roof"` `(-1,8,-1, ow=false)`, recurses `TOWER_GENERATOR` (`genDepth + 1`).
   - `2`: `"second_floor_2"` `(-1,0,-1, ow=false)`, `"third_floor_2"` `(-1,4,-1, ow=false)`, `"third_roof"` `(-1,8,-1, ow=true)`, recurses `TOWER_GENERATOR` (`genDepth + 1`).
   Returns `true`.
2. **`TOWER_GENERATOR`**:
   - Adds `"tower_base"` at `(3 + random.nextInt(2), -3, 3 + random.nextInt(2))` (`ow=true`), then `"tower_piece"` at `(0, 7, 0)` (`ow=true`).
   - `bridgePiece = (random.nextInt(3) == 0) ? lastPiece : null`.
   - `towerHeight = 1 + random.nextInt(3)` (`1..3`). For `i = 0 .. towerHeight - 1`: adds `"tower_piece"` at `(0, 4, 0)` (`ow=true`); if `i < towerHeight - 1 && random.nextBoolean()`, sets `bridgePiece = lastPiece`.
   - If `bridgePiece != null`: iterate 4 `TOWER_BRIDGES` in order `[(NONE,(1,-1,0)), (CW90,(6,-1,1)), (CCW90,(0,-1,5)), (180,(5,-1,6))]`: if `random.nextBoolean()`, adds `"bridge_end"` at bridge offset with `rot.getRotated(bridgeRot)` (`ow=true`) and recurses `TOWER_BRIDGE_GENERATOR` (`genDepth + 1`). Then adds `"tower_top"` at `(-1, 4, -1)` (`ow=true`). Returns `true`.
   - Else if `genDepth != 7`: returns `recursiveChildren(FAT_TOWER_GENERATOR, genDepth + 1, lastPiece, null, pieces, random)`.
   - Else (`genDepth == 7`): adds `"tower_top"` at `(-1, 4, -1)` (`ow=true`), returns `true`.
3. **`TOWER_BRIDGE_GENERATOR`** (mutable `shipCreated` flag):
   - `bridgeLength = random.nextInt(4) + 1` (`1..4`). Adds `"bridge_piece"` at `(0, 0, -4)` (`ow=true`), sets its `genDepth = -1`; `nextY = 0`.
   - For `i = 0 .. bridgeLength - 1`: if `random.nextBoolean()` adds `"bridge_piece"` at `(0, nextY, -4)` and sets `nextY = 0`; else if `random.nextBoolean()` adds `"bridge_steep_stairs"` at `(0, nextY, -4)` and sets `nextY = 4`, else adds `"bridge_gentle_stairs"` at `(0, nextY, -8)` and sets `nextY = 4`.
   - **End Ship check**: if `!shipCreated && random.nextInt(10 - genDepth) == 0`: adds `"ship"` at `(-8 + random.nextInt(8), nextY, -70 + random.nextInt(10))` (`ow=true`), sets `shipCreated = true`.
   - Else: calls `recursiveChildren(HOUSE_TOWER_GENERATOR, genDepth + 1, lastPiece, (-3, nextY + 1, -11), pieces, random)`; if `false`, returns `false`!
   - Adds `"bridge_end"` at `(4, nextY, 0)` with `rot.getRotated(180)` (`ow=true`), sets its `genDepth = -1`, returns `true`.
4. **`FAT_TOWER_GENERATOR`**:
   - Adds `"fat_tower_base"` at `(-3, 4, -3)` (`ow=true`), `"fat_tower_middle"` at `(0, 4, 0)` (`ow=true`).
   - For `i = 0; i < 2 && random.nextInt(3) != 0; i++`: adds `"fat_tower_middle"` at `(0, 8, 0)` (`ow=true`); iterates 4 `FAT_TOWER_BRIDGES` `[(NONE,(4,-1,0)), (CW90,(12,-1,4)), (CCW90,(0,-1,8)), (180,(8,-1,12))]`: if `random.nextBoolean()`, adds `"bridge_end"` at bridge offset with `rot.getRotated(bridgeRot)` (`ow=true`) and recurses `TOWER_BRIDGE_GENERATOR` (`genDepth + 1`).
   - Adds `"fat_tower_top"` at `(-2, 8, -2)` (`ow=true`), returns `true`.

#### 14.3 `EndCityPiece.handleDataMarker`
- Starts with `"Chest"`: if `chunkBB.isInside(pos.below())`, `RandomizableContainer.setBlockEntityLootTable(level, random, pos.below(), minecraft:chests/end_city_treasure)`.
- Else if `chunkBB.isInside(pos) && Level.isInSpawnableBounds(pos)`:
  - Starts with `"Sentry"`: creates `minecraft:shulker` at `(pos.x + 0.5, pos.y, pos.z + 0.5)` (`addFreshEntity`, no `finalizeSpawn`!).
  - Starts with `"Elytra"`: creates `ItemFrame` at `pos` facing `rot.rotate(SOUTH)` holding `minecraft:elytra` (`setItem(elytra, false)`), `addFreshEntity`.

---

### 15. Nether Fossil (`minecraft:nether_fossil`)

#### 15.1 Configuration schema, start generation & terrain adaptation
- Schema: `StructureSettings` + `height: HeightProvider` (required).
- Vanilla defaults: `step = underground_decoration`, **`terrain_adaptation = beard_thin`**, no `spawn_overrides`, `height = uniform(absolute(32), below_top(2))`.
- `findGenerationPoint(context)`:
  1. `blockX = chunk.minBlockX + random.nextInt(16)`, `blockZ = chunk.minBlockZ + random.nextInt(16)`.
  2. `y = height.sample(random, WorldGenerationContext(chunkGenerator, heightAccessor))`.
  3. Read `column = chunkGenerator.getBaseColumn(blockX, blockZ, ...)`.
  4. Loop while `y > seaLevel`: `cur = column.getBlock(y)`, `below = column.getBlock(--y)`; break if `cur.isAir() && (below.is(minecraft:soul_sand) || below.isFaceSturdy(EmptyBlockGetter, (blockX, y, blockZ), UP))`.
  5. If `y <= seaLevel`, return empty.
  6. Return stub at `(blockX, y, blockZ)`: draws `rot = Rotation.getRandom(random)` then `template = Util.getRandom(FOSSILS, random)` from the 14 templates `minecraft:nether_fossils/fossil_1` .. `fossil_14`.

#### 15.2 `NetherFossilPiece` (`nefos`) placement & dried ghast
- `StructurePlaceSettings`: `rotation`, `mirror = NONE`, processor `BlockIgnoreProcessor.STRUCTURE_AND_AIR`.
- **`postProcess`**:
  1. Compute `fossilBB = template.getBoundingBox(placeSettings, templatePosition)`.
  2. **Mutates `chunkBB`**: calls `chunkBB.encapsulate(fossilBB)` (expanding the passed bounding box so the entire fossil places in a single chunk pass!).
  3. Calls `TemplateStructurePiece.postProcess`.
  4. **Dried ghast placement (`placeDriedGhast`)**:
     - Creates standalone legacy positional RNG `posRng = RandomSource.createThreadLocalInstance(level.getSeed()).forkPositional().at(fossilBB.getCenter())`.
     - If `posRng.nextFloat() < 0.5f`:
       - `x = fossilBB.minX + posRng.nextInt(fossilBB.getXSpan())`, `y = fossilBB.minY`, `z = fossilBB.minZ + posRng.nextInt(fossilBB.getZSpan())`.
       - If `level.getBlockState(x, y, z).isAir() && chunkBB.isInside(x, y, z)`: places `minecraft:dried_ghast` rotated by `Rotation.getRandom(posRng)` (`flags = 2`).

---

### 16. Ruined Portal (`minecraft:ruined_portal`)

#### 16.1 Configuration schema & vanilla setups
- Schema: `StructureSettings` + `setups`: non-empty list of `Setup`:
  - `placement`: `"on_land_surface"` | `"partly_buried"` | `"on_ocean_floor"` | `"in_mountain"` | `"underground"` | `"in_nether"`
  - `air_pocket_probability`: `f32 ∈ [0,1]`
  - `mossiness`: `f32 ∈ [0,1]`
  - `overgrown`: `bool`
  - `vines`: `bool`
  - `can_be_cold`: `bool`
  - `replace_with_blackstone`: `bool`
  - `weight`: `f32 > 0`
- Vanilla 26.3 presets (all `step = surface_structures`, `terrain_adaptation = none`, no `spawn_overrides`):
  - `ruined_portal` (standard): `[underground(air=1.0, moss=0.2, cold=true, wt=0.5), on_land_surface(air=0.0, moss=0.2, cold=true, wt=0.5)]`
  - `ruined_portal_desert`: `[partly_buried(air=0.0, moss=0.0, all false, wt=1.0)]`
  - `ruined_portal_jungle`: `[on_land_surface(air=0.0, moss=0.8, overgrown=true, vines=true, wt=1.0)]`
  - `ruined_portal_swamp`: `[on_ocean_floor(air=0.0, moss=0.5, vines=true, wt=1.0)]`
  - `ruined_portal_mountain`: `[in_mountain(air=1.0, moss=0.2, cold=true, wt=0.5), on_land_surface(air=0.0, moss=0.2, cold=true, wt=0.5)]`
  - `ruined_portal_ocean`: `[on_ocean_floor(air=0.0, moss=0.8, cold=true, wt=1.0)]`
  - `ruined_portal_nether`: `[in_nether(air=0.0, moss=0.0, replace_with_blackstone=true, wt=1.0)]`

#### 16.2 Start generation, RNG order & `findSuitableY`
Templates (loaded from user's jar):
- Normal (10): `minecraft:ruined_portal/portal_1` .. `portal_10`.
- Giant (3): `minecraft:ruined_portal/giant_portal_1` .. `giant_portal_3`.

Exact RNG order in `findGenerationPoint(context)`:
1. **Choose `Setup`**: if `setups.size() > 1`, sum `total = sum(s.weight)` (`f32`), draw `pick = random.nextFloat()`, iterate `s` subtracting `s.weight / total` (`f32`) until `pick < 0.0f`. If `setups.size() == 1`, takes `setups[0]` with **no RNG draw**.
2. **`airPocket`**: if `airPocketProbability == 0.0f` → `false` (no RNG); if `1.0f` → `true` (no RNG); else `random.nextFloat() < prob`.
3. **Template**: if `random.nextFloat() < 0.05f`, pick `GIANT[random.nextInt(3)]`; else `PORTALS[random.nextInt(10)]`.
4. **Transform**: `rotation = Rotation.getRandom(random)`; `mirror = (random.nextFloat() < 0.5f) ? Mirror.NONE : Mirror.FRONT_BACK`; `pivot = (template.size.x / 2, 0, template.size.z / 2)`.
5. **Height selection**: `basePos = (chunkMinX, 0, chunkMinZ)`, `bb = template.getBoundingBox(basePos, rotation, pivot, mirror)`, `center = bb.getCenter()`.
   - `hmType = (placement == ON_OCEAN_FLOOR) ? OCEAN_FLOOR_WG : WORLD_SURFACE_WG`.
   - `surfaceY = chunkGenerator.getBaseHeight(center.x, center.z, hmType, ...) - 1`.
   - **`findSuitableY`**: `minY = heightAccessor.minY + 15`. Initial `newY`:
     - `IN_NETHER`: if `airPocket` → `randomBetweenInclusive(32, 100)`; else if `random.nextFloat() < 0.5f` → `randomBetweenInclusive(27, 29)`; else `randomBetweenInclusive(29, 100)`.
     - `IN_MOUNTAIN`: `maxY = surfaceY - bb.getYSpan()`; `newY = (70 < maxY) ? randomBetweenInclusive(70, maxY) : maxY`.
     - `UNDERGROUND`: `maxY = surfaceY - bb.getYSpan()`; `newY = (minY < maxY) ? randomBetweenInclusive(minY, maxY) : maxY`.
     - `PARTLY_BURIED`: `newY = surfaceY - bb.getYSpan() + randomBetweenInclusive(2, 8)`.
     - `ON_LAND_SURFACE` / `ON_OCEAN_FLOOR`: `newY = surfaceY`.
   - Sample 4 `NoiseColumn`s at `(bb.minX, bb.minZ)`, `(bb.maxX, bb.minZ)`, `(bb.minX, bb.maxZ)`, `(bb.maxX, bb.maxZ)`. Scan `projectedY = newY` down while `projectedY > minY`: count corners where `hmType.isOpaque().test(column.getBlock(projectedY))`; return immediately when count reaches `3`. If loop ends, return `projectedY` (`minY`).
6. `origin = (basePos.x, projectedY, basePos.z)`. Inside piece builder, compute `cold = setup.canBeCold && biomeAt(origin >> 2).coldEnoughToSnow(origin, seaLevel)` (`surface-and-carvers.md` §1.8), and add single `RuinedPortalPiece` (`rupo`).

#### 16.3 `RuinedPortalPiece` (`rupo`) processor stack & post-processing
- **Exact processor chain** (in `StructurePlaceSettings` with `rotation`, `mirror`, `pivot`):
  1. `airPocket ? BlockIgnoreProcessor.STRUCTURE_BLOCK : BlockIgnoreProcessor.STRUCTURE_AND_AIR`
  2. `RuleProcessor` with rules:
     - `gold_block` (prob `0.3f`) → `air`
     - `lava`: if `ON_OCEAN_FLOOR` → `magma_block` (prob 1.0); else if `cold` → `netherrack` (prob 1.0); else `magma_block` (prob `0.2f`)
     - If `!cold`: `netherrack` (prob `0.07f`) → `magma_block`
  3. `BlockAgeProcessor(mossiness)` — uses `settings.getRandom(blockPos)`. Full stone blocks (`stone_bricks`, `stone`, `chiseled_stone_bricks`) have 50% chance to replace; **note**: when replacing, it unconditionally draws 2 random stair facings + 2 random `Half` values (4 RNG draws) to build both the non-mossy (`cracked_stone_bricks` / `stone_brick_stairs`) and mossy (`mossy_stone_bricks` / `mossy_stone_brick_stairs`) 2-element arrays before drawing `nextFloat() < mossiness` and `nextInt(2)`! Stairs (`#stairs`): 50% chance to replace with non-mossy `{stone_slab, stone_brick_slab}` or mossy `{mossy_stone_brick_stairs(withPropertiesOf), mossy_stone_brick_slab}`. Slabs (`#slabs`) and walls (`#walls`): `mossiness` chance to convert to mossy stone brick variant (`withPropertiesOf`). `obsidian`: 15% chance to become `crying_obsidian`.
  4. `ProtectedBlockProcessor(#minecraft:features_cannot_replace)`
  5. `LavaSubmergedBlockProcessor` (if world block is `lava` and placed block is not full cube, replaces with `lava`)
  6. If `replaceWithBlackstone`: `BlackstoneReplaceProcessor` (maps cobblestone/mossy_cobblestone → `blackstone`, stone → `polished_blackstone`, stone_bricks/mossy_stone_bricks → `polished_blackstone_bricks`, matching stairs/slabs/walls preserving `FACING`/`HALF`/`TYPE`, chiseled → `chiseled_polished_blackstone`, cracked → `cracked_polished_blackstone_bricks`, `iron_bars` → `iron_chain`).
  *(Loot chests inside portal templates already carry loot table `minecraft:chests/ruined_portal` in their template block-entity NBT.)*
- **`postProcess`**:
  1. Compute `bb = template.getBoundingBox(placeSettings, templatePosition)`.
  2. **Center-in-chunk guard**: runs ONLY if `chunkBB.isInside(bb.getCenter())`! (So the entire portal places once, in the chunk containing its center.)
  3. **Mutates `chunkBB`**: calls `chunkBB.encapsulate(bb)`, then invokes `TemplateStructurePiece.postProcess`.
  4. **`spreadNetherrack(random, level)`**:
     - `followGround = placement ∈ {ON_LAND_SURFACE, ON_OCEAN_FLOOR}`.
     - `center = bb.getCenter()`. `probTable[0..13] = [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.9, 0.9, 0.8, 0.7, 0.6, 0.4, 0.2]` (`maxDist = 14`).
     - `avgWidth = (bb.getXSpan() + bb.getZSpan()) / 2`; `distAdj = random.nextInt(max(1, 8 - avgWidth / 2))`.
     - Loop `x = center.x - 14 .. center.x + 14` (outer), `z = center.z - 14 .. center.z + 14` (inner):
       - `adjDist = max(0, |x - center.x| + |z - center.z| + distAdj)`.
       - If `adjDist < 14 && random.nextDouble() < f64(probTable[adjDist])`:
         - `surfY = level.getHeight(hmType, x, z) - 1`.
         - `y = followGround ? surfY : min(bb.minY, surfY)`.
         - If `|y - bb.minY| <= 3 && canReplaceNetherrack(state)` (`!air && !obsidian && !#features_cannot_replace && (placement == IN_NETHER || !lava)`):
           - `placeNetherrackOrMagma`: if `!cold && random.nextFloat() < 0.07f` → `magma_block`, else `netherrack` (`setBlockAndUpdate`).
           - If `overgrown`: `maybeAddLeavesAbove(x, y, z)` (`random.nextFloat() < 0.5f && state(pos)==netherrack && state(pos.above)==air` → `jungle_leaves[persistent=true]`).
           - `addNetherrackDripColumn(x, y - 1, z)`: calls `placeNetherrackOrMagma` at start pos; then `rem = 8`; while `rem > 0 && random.nextFloat() < 0.5f`, move down 1, `rem--`, `placeNetherrackOrMagma`.
  5. **`addNetherrackDripColumnsBelowPortal(random, level)`**: loops `x = bb.minX + 1 .. bb.maxX - 1` (outer), `z = bb.minZ + 1 .. bb.maxZ - 1` (inner): if `level.getBlockState(x, bb.minY, z)` is `netherrack`, calls `addNetherrackDripColumn(x, bb.minY - 1, z)`.
  6. **Vines & overgrown leaves**: if `vines || overgrown`: streams `BlockPos.betweenClosedStream(bb)` (`x` fastest, `y` mid, `z` slowest):
     - If `vines`: if `!state.isAir() && !state.is(vine)`: draws `dir = getRandomHorizontalDirection(random)`; if `neighbor(pos+dir)` is air and `Block.isFaceFull(state.collisionShape, dir)`, places `vine[dir.opposite = true]` at `pos + dir`.
     - If `overgrown`: calls `maybeAddLeavesAbove(pos)`.

---
## Edge cases and known quirks

1. **`StructurePiece` `skipAir` vs. RNG consumption**:
   - In `generateBox(..., selector)`, `skipAir = false` calls `selector.next(random, ...)` for **every** cell in the box regardless of `chunkBB` (used by `JungleTemplePiece` and `StrongholdPieces.PortalRoom` / `FiveCrossing` / `PrisonHall` sub-boxes). `skipAir = true` skips `selector.next` for every cell outside `chunkBB` (because `getBlock` returns `air` outside `chunkBB`) AND for existing air cells inside `chunkBB` (used by most `StrongholdPiece` outer shells).
   - `generateMaybeBox` and `maybeGenerateBlock` **always** consume `random.nextFloat()` for every cell regardless of `chunkBB`, `skipAir`, or `hasToBeInside`.
   - Conversely, `MineShaftCorridor.maybePlaceCobWeb` short-circuits `isInterior` before `random.nextFloat()`, and `MineShaftCorridor` rails check `!floor.isAir() && floor.isSolidRender()` before `maybeGenerateBlock`, so neither consumes RNG outside `chunkBB`.
2. **Null-orientation world-space pieces**: `MineShaftRoom`, `MineShaftCrossing`, and `BuriedTreasurePiece` never call `setOrientation`, leaving `orientation = null`. Consequently `getWorldX/Y/Z` return `(x, y, z)` unchanged (world coordinates) and `placeBlock` applies no mirror or rotation.
3. **Mineshaft discarded `nextDouble()` and asymmetric start position**: `MineshaftStructure.findGenerationPoint` unconditionally consumes and discards `context.random().nextDouble()`, and sets the biome-validation start position to `(chunk.middleBlockX, 50 + dy, chunk.minBlockZ)` (middle X, min Z).
4. **Stronghold & Nether Fortress weighted-selection fall-through on `null`**: Inside the 5-attempt loop, if the chosen `PieceWeight` passes `doPlace` and `!= previousPiece` but `createPiece` returns `null` (collision or `minY <= 10`), the inner `for` loop does **not** `break` — because `weightSelection` is already `< 0`, it falls through and tries every remaining eligible `PieceWeight` in the list during the same attempt!
5. **Stronghold `imposedPiece` bypasses counters**: When `StartPiece.addChildren` sets `imposedPiece = FiveCrossing.class`, the forced `FiveCrossing` does **not** increment `FiveCrossing`'s `placeCount` and does **not** set `startPiece.previousPiece`.
6. **Stronghold `PortalRoom` spawner overwrites stair & `RoomCrossing` unused types**: The silverfish spawner at `(5, 3, 6)` overwrites the middle stone-brick stair placed immediately beforehand. `RoomCrossing` types `3` and `4` (40% of crossings) are empty rooms. `RoomCrossing` type `2` places a default `wall_torch[facing=north]` at `(5, 3, 5)`.
7. **Nether Fortress out-of-bounds `BridgeEndFiller` leak**: When `|footX - start.minX| > 112` or `|footZ - start.minZ| > 112`, `generateAndAddPiece` still calls `BridgeEndFiller.createPiece` (consuming `selfSeed = random.nextInt()` if non-colliding) but returns it **without** adding it to `builder` or `pendingChildren`.
8. **Ocean Monument grid Z-axis flip, save/load regeneration, & per-chunk child-room RNG** **[oracle]**: `MonumentBuilding.generateRoomGraph` flips `NORTH`/`SOUTH` connections when linking rooms along grid Z so local `z = 0` is `SOUTH` (entrance side) and `z = maxZ` is `NORTH`. `OceanMonumentSimpleTopRoom` checks `hasOpening[SOUTH]` even though `FitSimpleTopRoom` requires `SOUTH` to be closed. Because `MonumentBuilding` does not serialize `childPieces`, reloading a chunk regenerates them deterministically via `OceanMonumentStructure.regeneratePiecesAfterLoad`. Additionally, `MonumentBuilding.postProcess` passes the current chunk's `random` into each intersecting child room (`SimpleRoom` / `SimpleTopRoom`), so rooms spanning chunk borders draw from each chunk's independent `random` stream.
9. **Woodland Mansion double RNG draw for secret 1×1 rooms & `nextInt(1)`**: `addRoom1x1` unconditionally calls `rooms.get1x1(random)` **before** checking `doorDir` and calling `rooms.get1x1Secret(random)` for secret rooms (`doorDir == UP`). `SecondFloorRoomCollection.get1x2Secret` computes `"1x2_se" + (random.nextInt(1) + 1)`, advancing RNG by one `nextInt(1)` draw.
10. **Desert Pyramid multi-chunk cellar RNG & `afterPlace` interaction** **[oracle]**:
    - `DesertPyramidPiece.postProcess` unconditionally evaluates `-random.nextInt(3)` as an argument to `updateHeightPositionToLowestGroundHeight` on **every** intersecting chunk call, even after `heightPosition >= 0`.
    - `addCellarStairs` and `placeCollapsedRoofPiece` draw from `level.getRandom()` (`minecraft:worldgen_region_random` seeded per **current chunk**), so a cellar straddling a chunk border sees different `level.getRandom()` streams per chunk.
    - `potentialSuspiciousSandWorldPositions` is an ephemeral list populated during `postProcess` and not saved to NBT; `afterPlace` deduplicates it with `SortedArraySet(Vec3i::compareTo)` and shuffles with a deterministic legacy positional RNG seeded at `pieces.calculateBoundingBox().getCenter()`, so suspicious sand positions are chunk-order-invariant once `heightPosition` is fixed.
11. **Shipwreck `isTooBigToFitInWorldGenRegion` checks `Y` instead of `Z` and unrotated height loop**: `isTooBigToFitInWorldGenRegion` tests `size.x > 32 || size.y > 32` (never true for vanilla shipwrecks). When computing height in `postProcess`, `ShipwreckPiece` iterates `(templatePosition) .. (templatePosition + (size.x - 1, 0, size.z - 1))` **without** applying `rotation` or `PIVOT`.
12. **Ocean Ruin repeated height recomputation across chunks** **[oracle]**: Unlike `ShipwreckPiece`, `OceanRuinPiece` has no `heightAdjusted` flag and overwrites `this.templatePosition.y` from `OCEAN_FLOOR_WG` + `getHeight(...)` on **every** intersecting chunk's `postProcess` call (which can shift Y if an earlier chunk already placed blocks at `(tp.x, tp.z)`!).
13. **Igloo `templatePosition` save/restore**: `IglooPiece.postProcess` shifts `templatePosition.y` by `height - 90 - 1` before calling `super.postProcess`, then restores `templatePosition` to its original `y = 90 - depth` afterward so every intersecting chunk computes the same offset from the entrance column's `WORLD_SURFACE_WG`.
14. **`NetherFossilPiece` and `RuinedPortalPiece` mutate `chunkBB`** **[oracle]**:
    - Both call `chunkBB.encapsulate(pieceBB)`, expanding the writable box beyond the current 16×16 chunk into the surrounding `WorldGenRegion` write radius.
    - `RuinedPortalPiece` guards `postProcess` with `if (chunkBB.isInside(bb.getCenter()))`, ensuring it executes exactly once across all intersecting chunks.
    - `NetherFossilPiece` has **no** center-in-chunk guard, so if a fossil spans multiple chunks, `postProcess` (and `placeDriedGhast`) runs in each intersecting chunk (though `placeDriedGhast` checks `level.getBlockState(randomPos).isAir()`, preventing double placement once placed, and uses a deterministic positional RNG at `fossilBB.getCenter()`).
15. **`BlockAgeProcessor` unconditional stair RNG consumption**: In `RuinedPortalPiece`, when a full stone block passes the 50% replacement check, `BlockAgeProcessor` always allocates both the non-mossy and mossy replacement arrays — calling `getRandomFacingStairs` twice (4 RNG draws: horizontal direction + `Half`, twice) even when `mossiness == 0.0f` or `1.0f`.
16. **Static mutable state in `StrongholdPieces` and `EndCityPieces`**: Vanilla stores `currentPieces` / `imposedPiece` (Stronghold) and `TOWER_BRIDGE_GENERATOR.shipCreated` (End City) in static fields. Zenith must keep these per-structure-generation-call (thread-local / stack-local) so parallel chunk generation is race-free while remaining bit-identical to single-threaded vanilla generation.
17. **Chunk generation order and block-predicate parity (`isFaceSturdy`, `canSupportCenter`, `isSolidRender`, `isFaceFull`)** **[oracle]**: Several legacy pieces inspect surrounding terrain/earlier-structure blocks (`MineShaftCorridor` pillars/chains/rails/cobwebs, `NetherFossilStructure` floor check, `StructurePiece.reorient` chest facing, `RuinedPortalPiece.maybeAddVines`), so exact block-shape predicates must match 26.3.

---

## Constants and IDs

### Registered `minecraft:worldgen/structure_type` IDs (all 16 in 26.3)
- **Out of scope (jigsaw)**: `minecraft:jigsaw`
- **In scope (15 legacy types)**:
  `minecraft:buried_treasure`, `minecraft:desert_pyramid`, `minecraft:end_city`, `minecraft:fortress`, `minecraft:igloo`, `minecraft:jungle_temple`, `minecraft:mineshaft`, `minecraft:nether_fossil`, `minecraft:ocean_monument`, `minecraft:ocean_ruin`, `minecraft:ruined_portal`, `minecraft:shipwreck`, `minecraft:stronghold`, `minecraft:swamp_hut`, `minecraft:woodland_mansion`.

### Registered non-jigsaw `minecraft:worldgen/structure_piece` IDs (55 total, lowercased on registration)
- **Mineshaft (4)**: `mscorridor`, `mscrossing`, `msroom`, `msstairs`
- **Nether Fortress (15)**: `nebcr`, `nebef`, `nebs`, `neccs`, `nectb`, `nece`, `nescsc`, `nesclt`, `nesc`, `nescrt`, `necsr`, `nemt`, `nerc`, `nesr`, `nestart`
- **Stronghold (13)**: `shcc`, `shfc`, `sh5c`, `shlt`, `shli`, `shpr`, `shph`, `shrt`, `shrc`, `shsd`, `shstart`, `shs`, `shssd`
- **Ocean Monument (12)**: `omb`, `omcr`, `omdxr`, `omdxyr`, `omdyr`, `omdyzr`, `omdzr`, `omentry`, `ompenthouse`, `omsimple`, `omsimplet`, `omwr`
- **Single-piece & template structures (11)**: `tedp` (desert pyramid), `tejp` (jungle temple), `tesh` (swamp hut), `btp` (buried treasure), `iglu` (igloo), `shipwreck` (shipwreck), `orp` (ocean ruin), `rupo` (ruined portal), `ecp` (end city), `wmp` (woodland mansion), `nefos` (nether fossil)
- *(Excluded jigsaw piece type: `jigsaw`, making 56 total registered piece types.)*

### Loot table IDs referenced (loaded from user's data packs; contents never copied)
- Chests & dispensers:
  - `minecraft:chests/abandoned_mineshaft`
  - `minecraft:chests/buried_treasure`
  - `minecraft:chests/desert_pyramid`
  - `minecraft:chests/end_city_treasure`
  - `minecraft:chests/igloo_chest`
  - `minecraft:chests/jungle_temple`
  - `minecraft:chests/jungle_temple_dispenser`
  - `minecraft:chests/nether_bridge`
  - `minecraft:chests/ruined_portal` (referenced inside portal template NBT)
  - `minecraft:chests/shipwreck_map`
  - `minecraft:chests/shipwreck_supply`
  - `minecraft:chests/shipwreck_treasure`
  - `minecraft:chests/stronghold_corridor`
  - `minecraft:chests/stronghold_crossing`
  - `minecraft:chests/stronghold_library`
  - `minecraft:chests/underwater_ruin_big`
  - `minecraft:chests/underwater_ruin_small`
  - `minecraft:chests/woodland_mansion`
- Archaeology (`brushable_block`):
  - `minecraft:archaeology/desert_pyramid`
  - `minecraft:archaeology/ocean_ruin_cold`
  - `minecraft:archaeology/ocean_ruin_warm`

### Spawner & directly spawned entity IDs
- **Mob spawners (`minecraft:spawner`)**: `minecraft:cave_spider` (Mineshaft), `minecraft:silverfish` (Stronghold), `minecraft:blaze` (Nether Fortress).
- **Direct entity spawns**: `minecraft:chest_minecart` (Mineshaft), `minecraft:witch` & `minecraft:cat` (Swamp Hut), `minecraft:elder_guardian` (Ocean Monument: 2 wings + 1 penthouse = 3 total), `minecraft:drowned` (Ocean Ruins), `minecraft:shulker` & `minecraft:item_frame` holding `minecraft:elytra` (End City), `minecraft:evoker`, `minecraft:vindicator`, & `minecraft:allay` (Woodland Mansion).

### Tags & special RNG identifiers referenced
- Biome tags: `#minecraft:mineshaft_blocking`, `#minecraft:required_ocean_monument_surrounding`, plus `#minecraft:has_structure/*` tags in `minecraft:worldgen/structure` JSON.
- Block tags: `#minecraft:features_cannot_replace`, `#minecraft:ice`, `#minecraft:stairs`, `#minecraft:slabs`, `#minecraft:walls`.
- Fluid tags: `#minecraft:water`.
- `WorldGenRegion` positional random name: `minecraft:worldgen_region_random`.

### Template namespace directories (loaded at runtime from user's jar/data packs)
`end_city/*`, `igloo/*`, `nether_fossils/*`, `ruined_portal/*`, `shipwreck/*`, `underwater_ruin/*`, `woodland_mansion/*`.

---

## Oracle probe points

Public Java classes and methods a black-box parity harness for `golden-structures` can invoke to capture piece lists, bounding boxes, block states, block-entity loot tables/spawners, and entity spawns:

1. **Start & piece-graph parity (`STRUCTURE_STARTS`)**:
   - `net.minecraft.world.level.levelgen.structure.Structure.generate(...)` → `StructureStart`
   - `net.minecraft.world.level.levelgen.structure.StructureStart.getPieces()` and `StructurePiece.createTag(StructurePieceSerializationContext)` — dump ordered list of `(pieceTypeId, BB, O, GD, extra NBT fields)` for every one of the 15 legacy structure types across multiple seeds & chunks.
   - `net.minecraft.world.level.levelgen.structure.structures.OceanMonumentStructure.regeneratePiecesAfterLoad(ChunkPos, long, PiecesContainer)` — verify post-load deterministic reconstruction.
   - `net.minecraft.world.level.levelgen.structure.structures.WoodlandMansionPieces.generateMansion(StructureTemplateManager, BlockPos, Rotation, List, RandomSource)` — dump exact ordered piece list (`templateName`, `templatePosition`, `rotation`, `mirror`).
   - `net.minecraft.world.level.levelgen.structure.structures.EndCityPieces.startHouseTower(StructureTemplateManager, BlockPos, Rotation, List, RandomSource)` — dump exact ordered piece list (`templateName`, `templatePosition`, `rotation`, `overwrite`, `genDepth`).
2. **Per-chunk placement & block/block-entity/entity parity (`FEATURES`)**:
   - `net.minecraft.world.level.levelgen.structure.StructureStart.placeInChunk(WorldGenLevel, StructureManager, ChunkGenerator, RandomSource, BoundingBox, ChunkPos)` — dump chunk block-state global IDs (`Block.getId(state)`), block-entity NBT (`LootTable`, `LootTableSeed`, `SpawnData`), scheduled fluid ticks, post-processing marks, and newly spawned entity list `(type, pos, rotation, persistent, item)` before and after `placeInChunk`.
   - `net.minecraft.world.level.levelgen.structure.Structure.afterPlace(...)` on `DesertPyramidStructure` and `WoodlandMansionStructure` — isolate suspicious sand positions/loot seeds and cobblestone foundation columns.
   - `net.minecraft.world.level.levelgen.structure.templatesystem.BlockAgeProcessor.processBlock(...)`, `BlackstoneReplaceProcessor.processBlock(...)`, `LavaSubmergedBlockProcessor.processBlock(...)` — verify per-block RNG consumption and property preservation.
3. **Beardifier integration (`TERRAIN`)**:
   - `net.minecraft.world.level.levelgen.Beardifier.forStructuresInChunk(StructureManager, ChunkPos)` on chunks overlapping `StrongholdStructure` (`bury`) and `NetherFossilStructure` (`beard_thin`) to verify chunk terrain parity with legacy starts present.

---

## Open questions

1. **[oracle] Desert Pyramid multi-chunk cellar `level.getRandom()` and `afterPlace` chunk-order parity**: `DesertPyramidPiece.addCellarStairs` and `placeCollapsedRoofPiece` draw from `WorldGenRegion.getRandom()` (seeded per center chunk), and `postProcess` evaluates `-random.nextInt(3)` even on subsequent chunks while appending to `potentialSuspiciousSandWorldPositions`. **Probe**: Locate a seed/chunk where a Desert Pyramid's cellar (`local [12..20, -4..0, 9..17]`) crosses a chunk boundary; run `golden-structures` on both chunks and dump every block in the cellar, every `suspicious_sand` coordinate + `LootTableSeed`, and confirm exact match against vanilla (including whether chunk generation order affects `level.getRandom()` draws in `addCellarStairs` / `placeCollapsedRoofPiece`).
2. **[oracle] `OceanRuinPiece` repeated height recomputation across chunk borders**: `OceanRuinPiece.postProcess` has no `heightAdjusted` guard and recomputes `templatePosition.y` from `OCEAN_FLOOR_WG` + `getHeight(...)` on every intersecting chunk. **Probe**: Find a large Ocean Ruin (especially a cold 3-layer ruin whose `bricks` piece writes blocks into the chunk before `cracked` and `mossy` run, or a ruin spanning two chunks) and dump `(pieceIndex, chunkPos, computedY)` and final block states to verify whether earlier-placed ruin blocks alter `OCEAN_FLOOR_WG` or `getHeight` for subsequent layers/chunks.
3. **[oracle] `NetherFossilPiece` multi-chunk `chunkBB.encapsulate` and `placeDriedGhast` behavior**: Unlike `RuinedPortalPiece`, `NetherFossilPiece.postProcess` has no `chunkBB.isInside(center)` guard before calling `chunkBB.encapsulate(fossilBB)` and `placeDriedGhast`. **Probe**: Pick a Nether Fossil spanning a chunk boundary and dump final chunk blocks (including `minecraft:dried_ghast` position and facing) across both chunks, verifying whether `WorldGenRegion`'s write radius permits the cross-chunk write cleanly and produces identical blocks when both chunks generate.
4. **[oracle] `OceanMonumentPiece` per-chunk child-room RNG consumption in `OceanMonumentSimpleRoom` and `OceanMonumentSimpleTopRoom`**: `MonumentBuilding.postProcess` passes the chunk's `random` into each intersecting child room in `childPieces` order, where `SimpleRoom` draws `nextBoolean()` (when `mainDesign != 0`) and `SimpleTopRoom` draws wet-sponge RNGs per `(x, z)` regardless of `chunkBB`. **Probe**: Dump block states across all chunks intersecting an Ocean Monument (especially a `SimpleRoom` or `SimpleTopRoom` split across a chunk border) to confirm whether vanilla generates different sponge patterns / center pillars on opposite sides of a chunk border when `random` differs per chunk, and confirm Zenith replicates that exact per-chunk RNG stream.
5. **[oracle] Block-shape predicate parity (`isFaceSturdy`, `canSupportCenter`, `isSolidRender`, `isFaceFull`) during structure placement**: `MineShaftCorridor` pillars/chains/rails/cobwebs, `NetherFossilStructure` floor detection, `StructurePiece.reorient` chest facing, and `RuinedPortalPiece.maybeAddVines` depend on exact block collision/occlusion shape predicates. **Probe**: Across a `golden-structures` sample covering Mineshaft (normal + mesa), Stronghold, Nether Fortress, Nether Fossil, and Ruined Portal (jungle + swamp), verify zero mismatches on chain vs. pillar selection, cobweb placement, chest facing, and vine attachment.

---

## Parity status

`golden-structures`: not yet run.
