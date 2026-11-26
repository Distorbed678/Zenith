# Worldgen: structure placement, jigsaw assembly, and structure templates (26.3)

- Verified by suites: `golden-structures`
- Sources: data-pack JSON schemas for `minecraft:worldgen/structure_set`, `minecraft:worldgen/structure`, `minecraft:worldgen/template_pool`, and `minecraft:worldgen/processor_list`; `.nbt` structure template format; `docs/specs/worldgen/density-functions.md` (beardifier and terrain fill); `docs/specs/worldgen/biomes-and-climate.md` (climate sampling, biome sources, spawn finding); `docs/specs/worldgen/surface-and-carvers.md` (chunk pyramid and vertical anchors); `docs/specs/math/random.md` and `docs/specs/math/java-math-helpers.md` (RNGs, hashing, integer/float helpers); `docs/specs/nbt/binary-format.md`; `docs/specs/codec/datapacks-and-codecs.md`; `docs/specs/registry/identifiers-registries-tags.md` (§3.3 block-state serialized form); and local reference reading (2026-11-25) for 26.3 exact placement math, RNG draw order, jigsaw assembly, template processing, and chunk-step integration. Everything below is written in our own words.
- Related specs:
  - [`worldgen/density-functions.md`](./density-functions.md) — `Beardifier` kernel/evaluation (§8), `NoiseChunk`, `getBaseHeight` / `iterateNoiseColumn` (§7)
  - [`worldgen/biomes-and-climate.md`](./biomes-and-climate.md) — `BiomeSource`, `Climate.Sampler`, `NoiseSpawnFinder` / `ChunkGenerator.getOrigin` (§4)
  - [`worldgen/surface-and-carvers.md`](./surface-and-carvers.md) — `VerticalAnchor` (§1.2), `WorldGenerationContext`, `ChunkPyramid.GENERATION_PYRAMID` (§4)
  - [`math/random.md`](../math/random.md) — legacy 48-bit LCG, xoroshiro128++, positional factories, `Mth.getSeed` position hash
  - [`math/java-math-helpers.md`](../math/java-math-helpers.md) — `Mth` helpers (`floorDiv`, `floorMod`, `inverseLerp`, `clampedLerp`, `nextInt`, `randomBetweenInclusive`), `ChunkPos.pack`, `QuartPos`, `SectionPos`, `AABB`
  - [`nbt/binary-format.md`](../nbt/binary-format.md) — gzipped named-root NBT and byte-accounting quotas
  - [`codec/datapacks-and-codecs.md`](../codec/datapacks-and-codecs.md) — strict JSON, record/either/dispatch/lenient codecs, pack stack resource lookup
  - [`registry/identifiers-registries-tags.md`](../registry/identifiers-registries-tags.md) — dynamic registry numeric ID order (§2.4), holders/holder sets (§2.6), block-state JSON/NBT form (§3.3)

---

## Summary

Structure generation in 26.3 spans four chunk-generation statuses (`structure_starts`, `structure_references`, `terrain`, and `features`) and five interacting subsystems:

1. **Structure sets and placements (`minecraft:worldgen/structure_set`)**: Each structure set pairs a weighted list of structures with one placement rule—`random_spread` (spaced chunk grid), `concentric_rings` (precomputed biome-biased ring positions), or 26.3's new `dimension_origin` (exact match on the dimension's spawn-finder origin chunk)—augmented on spreading placements by frequency reduction, exclusion zones, and locate offsets.
2. **Start planning and cross-chunk referencing (`structure_starts`, `structure_references`)**: In `structure_starts`, every candidate chunk tests all biome-compatible structure sets, selects at most one structure per set by weighted sampling without replacement, verifies the generation point's biome before lazily building pieces, and stores valid starts (those with at least one piece). In `structure_references`, every chunk scans the `17 × 17` neighborhood (`±8` chunks) and records references to every valid start whose (possibly beardifier-inflated) bounding box overlaps its `16 × 16` XZ footprint.
3. **Common structure definitions and the jigsaw assembler (`minecraft:worldgen/structure`, `minecraft:worldgen/template_pool`)**: Every structure carries biomes, decoration step, mob spawn overrides, and terrain adaptation. The `minecraft:jigsaw` type resolves pool aliases, anchors and vertically places a start piece, and expands children through a priority-ordered BFS (`SequencedPriorityIterator`), matching jigsaw block orientations and names, colliding against free-space `VoxelShape`s with a `0.25`-block deflation margin, applying fallback pools, and recording `JigsawJunction`s.
4. **Structure templates and processors (`data/<ns>/structure/**/*.nbt`, `minecraft:worldgen/processor_list`)**: Gzipped NBT templates hold one or more block-state palettes, categorized/sorted block lists, and entities. Placement transforms positions and states by mirror and rotation around a pivot, filters and mutates blocks through an ordered pipeline of 11 built-in processors (including `rule` with 10 rule tests, 3 position tests, and 4 block-entity modifiers, plus whole-piece `capped` processing), handles waterlogging and fluid flood-fill, populates block entities, and spawns entities.
5. **Decoration placement and `Beardifier` handoff (`terrain`, `features`)**: During `terrain`, `Beardifier` consumes referenced structures' rigid pieces, ground-level deltas, terrain-adaptation modes, and jigsaw junctions to shape terrain density before blocks are placed. During `features`, structures run **before** biome features within each decoration step, post-processing every piece that intersects the chunk's writable bounding box.

All vanilla structure sets, structures, template pools, processor lists, and `.nbt` templates live inside the user's game jar and active data packs (`data/<namespace>/worldgen/{structure_set,structure,template_pool,processor_list}/**/*.json` and `data/<namespace>/structure/**/*.nbt`) and must be read at run time; none are copied into the repository.

---

## Inputs and state

### Arithmetic, coordinate, and RNG conventions
- Integer arithmetic uses two's-complement wrapping `i32` and `i64`; `/` and `%` on integers truncate toward zero unless explicitly written `floorDiv` / `floorMod` (which floor toward $-\infty$).
- Floating-point arithmetic uses IEEE-754 binary32 (`f32`) and binary64 (`f64`), round-to-nearest-even, without fused multiply-add; chains associate left to right unless parenthesized.
- `Math.round(f64) -> i64` rounds ties toward $+\infty$ (exact $\lfloor x + 0.5 \rfloor$, saturating).
- Coordinate helpers (per [`java-math-helpers.md`](../math/java-math-helpers.md)):
  - `ChunkPos.minBlockX = chunkX << 4`, `middleBlockX = (chunkX << 4) + 8`, `maxBlockX = (chunkX << 4) + 15` (likewise Z).
  - `ChunkPos.pack(x, z) = (i64(x) & 0xFFFFFFFF) | ((i64(z) & 0xFFFFFFFF) << 32)`; `unpack(k)` extracts low 32 bits as `x` and arithmetic-shifted high 32 bits as `z`.
  - `QuartPos.fromBlock(b) = b >> 2`, `QuartPos.toBlock(q) = q << 2`.
  - `BlockPos.distManhattan(a, b) = |a.x - b.x| + |a.y - b.y| + |a.z - b.z|`.
- **`WorldgenRandom` seeding formulas** (always backed by the specified underlying source—legacy 48-bit LCG or xoroshiro128++—per [`random.md`](../math/random.md)):
  - `setLargeFeatureSeed(seed: i64, chunkX: i32, chunkZ: i32)`:
    1. `setSeed(seed)`
    2. `xScale = nextLong()`, `zScale = nextLong()` (in that order)
    3. `setSeed((i64(chunkX) * xScale) ^ (i64(chunkZ) * zScale) ^ seed)`
  - `setLargeFeatureWithSalt(seed: i64, x: i32, z: i32, salt: i32)`:
    - `setSeed(i64(x) * 341873128712 + i64(z) * 132897987541 + seed + i64(salt))` (64-bit wrapping).
  - `setDecorationSeed(seed: i64, blockX: i32, blockZ: i32) -> i64`:
    1. `setSeed(seed)`
    2. `xScale = nextLong() | 1`, `zScale = nextLong() | 1`
    3. `result = (i64(blockX) * xScale + i64(blockZ) * zScale) ^ seed`
    4. `setSeed(result)` and return `result`.
  - `setFeatureSeed(decorationSeed: i64, index: i32, step: i32)`:
    - `setSeed(decorationSeed + i64(index) + i64(10000 * step))` (note `10000 * step` is `i32` multiply widened to `i64`).
- **Standard shuffle (`Util.shuffle` / `Util.shuffledCopy` / `Util.toShuffledList`)**: backward Fisher-Yates on a list of length $n$:
  - For $i = n$ down to $2$ inclusive: draw $j = \text{random.nextInt}(i)$, swap element $i - 1$ with element $j$.
  - When $n \le 1$, zero RNG draws are consumed.
- **`WeightedList<T>` sampling (`getRandom` / `getRandomOrThrow`)**:
  - JSON schema: array of objects `{ "data": <T>, "weight": <i32 >= 0> }` (or map-flattened when used with a `MapCodec`). `nonEmptyCodec` rejects lists whose total weight is `0`.
  - Total weight $W = \sum_k w_k$. If $W = 0$, empty / throws. Otherwise draw $s = \text{random.nextInt}(W)$ and return the first item $k$ for which $s < \sum_{m \le k} w_m$ (sequential subtraction `s -= w_k; if s < 0 return item_k`).

### Axis-aligned integer bounding box (`BoundingBox`)
A `BoundingBox` is an **inclusive** 6-tuple `(minX, minY, minZ, maxX, maxY, maxZ)` of `i32` with `min* <= max*` (if constructed with inverted bounds, min and max are swapped per axis):
- Spans: `xSpan = maxX - minX + 1`, `ySpan = maxY - minY + 1`, `zSpan = maxZ - minZ + 1`.
- `getCenter()`: `(minX + xSpan / 2, minY + ySpan / 2, minZ + zSpan / 2)` (Java truncating `/ 2`).
- `isInside(x, y, z)`: inclusive on all 6 faces (`minX <= x <= maxX`, etc.).
- `intersects(other)`: inclusive 3D overlap (`maxX >= other.minX && minX <= other.maxX` on X, Z, Y).
- `intersects(minX, minZ, maxX, maxZ)`: inclusive 2D XZ overlap.
- `inflatedBy(r)`: `(minX - r, minY - r, minZ - r, maxX + r, maxY + r, maxZ + r)`.
- `AABB.of(box)`: half-open `f64` box `[minX, maxX + 1] × [minY, maxY + 1] × [minZ, maxZ + 1]`.
- `encapsulate(pos)` / `encapsulate(other)`: component-wise min/max expansion.
- Serialization (`BoundingBox.CODEC`): 6-element `IntArray` `[minX, minY, minZ, maxX, maxY, maxZ]`.

### Registries and per-dimension state
- **World-layer dynamic registries** (numeric ID order follows [`identifiers-registries-tags.md`](../registry/identifiers-registries-tags.md) §2.4—ascending by full file identifier `<ns>:<registry_path>/<element_path>.json`):
  - `minecraft:worldgen/structure_set`
  - `minecraft:worldgen/structure`
  - `minecraft:worldgen/template_pool`
  - `minecraft:worldgen/processor_list`
- **Built-in dispatch registries**: `minecraft:worldgen/structure_placement`, `minecraft:worldgen/structure_type`, `minecraft:worldgen/structure_piece`, `minecraft:worldgen/structure_pool_element`, `minecraft:worldgen/pool_alias_binding`, `minecraft:worldgen/structure_processor`, `minecraft:worldgen/rule_test`, `minecraft:worldgen/pos_rule_test`, `minecraft:worldgen/rule_block_entity_modifier`, `minecraft:worldgen/height_provider_type`.
- **`ChunkGeneratorStructureState`**: created once per dimension from `(randomState, levelSeed, origin, biomeSource, structureSets)`:
  - **Normal worlds** (`createForNormal`): takes `structureSets.listElements()` in dynamic-registry numeric ID order, retains only sets where at least one structure has at least one biome present in `biomeSource.possibleBiomes()`, and sets `concentricRingsSeed = levelSeed`.
  - **Flat worlds** (`createForFlat`): uses `FlatLevelGeneratorSettings.structureOverrides` if present (else all `structureSets`), applies the same biome filter, and sets **`concentricRingsSeed = 0L`**.
  - **`origin`**: `chunkGenerator.getOrigin(randomState)` (for `NoiseBasedChunkGenerator`, computed by `NoiseSpawnFinder` per [`biomes-and-climate.md`](biomes-and-climate.md) §4; for `FlatLevelSource`, `DebugLevelSource`, or empty `spawn_target`, `ChunkPos(0, 0)`).

---

## Behavior (ordered rules)

### 1. Structure sets and placement

#### 1.1 `StructureSet` schema (`minecraft:worldgen/structure_set`)
JSON object:
- `structures`: required list of `StructureSelectionEntry` objects, each `{ "structure": <Holder<Structure>>, "weight": <i32 >= 1> }`.
- `placement`: required `StructurePlacement`, dispatched on `"type"` via `minecraft:worldgen/structure_placement`.

#### 1.2 Shared spreading placement schema (`AbstractSpreadingStructurePlacement`)
Shared by `minecraft:random_spread` and `minecraft:concentric_rings` (and **not** by `minecraft:dimension_origin`):

| JSON field | Type / range | Required | Default |
|---|---|---|---|
| `locate_offset` | `[x, y, z]` (`Vec3i`, each component in `[-16, 16]`) | no | `[0, 0, 0]` |
| `frequency_reduction_method` | `"default"` \| `"legacy_type_1"` \| `"legacy_type_2"` \| `"legacy_type_3"` | no | `"default"` |
| `frequency` | `f32` in `[0.0, 1.0]` | no | `1.0` |
| `salt` | `i32 >= 0` | yes | — |
| `exclusion_zone` | `{ "other_set": <reference-only Holder<StructureSet>>, "chunk_count": <i32 in [1, 16]> }` | no | absent |

Note: `exclusion_zone.other_set` disallows inline definitions (`allowInline = false`); it must be a registry identifier string in `minecraft:worldgen/structure_set`.

#### 1.3 Candidate-chunk evaluation order (`isStructureChunk`)
For any spreading placement at chunk `(sourceX, sourceZ)`, three predicates execute strictly in order with short-circuit `&&`:
1. **`isPlacementChunk(state, sourceX, sourceZ)`** (placement-specific rule below).
2. **Frequency reduction (`applyAdditionalChunkRestrictions(sourceX, sourceZ, levelSeed)`)**:
   - If `!(frequency < 1.0f)` (i.e. `frequency == 1.0f`), returns `true` without creating an RNG.
   - Otherwise instantiates a fresh `WorldgenRandom(LegacyRandomSource(0L))` and runs `frequency_reduction_method`:
     - **`default`**: calls `random.setLargeFeatureWithSalt(levelSeed, salt, sourceX, sourceZ)` (**note parameter order**: `x = salt`, `z = sourceX`, `blend = sourceZ`!) and returns `random.nextFloat() < frequency`.
     - **`legacy_type_1`** (pillager outpost reducer): sets `cx = sourceX >> 4`, `cz = sourceZ >> 4`; computes 32-bit `cx ^ (cz << 4)`, sign-extends to `i64`, XORs with `levelSeed`, and calls `random.setSeed(i64(cx ^ (cz << 4)) ^ levelSeed)`; calls and discards one `random.nextInt()`; returns `random.nextInt(trunc_i32(1.0f / frequency)) == 0`.
     - **`legacy_type_2`** (arbitrary-salt reducer): calls `random.setLargeFeatureWithSalt(levelSeed, sourceX, sourceZ, 10387320)` and returns `random.nextFloat() < frequency`.
     - **`legacy_type_3`** (legacy double reducer): calls `random.setLargeFeatureSeed(levelSeed, sourceX, sourceZ)` and returns `random.nextDouble() < f64(frequency)`.
3. **Exclusion zone (`applyInteractionsWithOtherStructures(state, sourceX, sourceZ)`)**:
   - If `exclusion_zone` is absent, returns `true`.
   - Otherwise calls `state.hasStructureChunkInRange(other_set, sourceX, sourceZ, chunk_count)`: loops `testX = sourceX - chunk_count ..= sourceX + chunk_count` (outer), `testZ = sourceZ - chunk_count ..= sourceZ + chunk_count` (inner); if `other_set.placement().isStructureChunk(state, testX, testZ)` is `true` for any tested chunk, placement is **forbidden** (returns `false`).

#### 1.4 Placement type 1: `minecraft:random_spread`
Additional JSON fields:
- `spacing`: `i32` in `[0, 4096]` (required).
- `separation`: `i32` in `[0, 4096]` (required).
- `spread_type`: `"linear"` | `"triangular"` (optional, default `"linear"`).
- Codec validation: rejects `spacing <= separation`.

Candidate chunk calculation `getPotentialStructureChunk(levelSeed, sourceX, sourceZ)`:
1. `spacedGridX = floorDiv(sourceX, spacing)`, `spacedGridZ = floorDiv(sourceZ, spacing)`.
2. Construct `random = WorldgenRandom(LegacyRandomSource(0L))` and call `random.setLargeFeatureWithSalt(levelSeed, spacedGridX, spacedGridZ, salt)`.
3. `limit = spacing - separation` ($\ge 1$).
4. Draw `spreadX`, then `spreadZ` (in exact X-then-Z order):
   - `linear`: `random.nextInt(limit)`.
   - `triangular`: `(random.nextInt(limit) + random.nextInt(limit)) / 2` (2 draws per axis, integer division by 2).
5. Potential chunk is `(spacedGridX * spacing + spreadX, spacedGridZ * spacing + spreadZ)`.
6. `isPlacementChunk` returns `true` iff this potential chunk equals `(sourceX, sourceZ)`.

#### 1.5 Placement type 2: `minecraft:concentric_rings`
Additional JSON fields:
- `distance`: `i32` in `[0, 1023]` (required).
- `spread`: `i32` in `[0, 1023]` (required).
- `count`: `i32` in `[1, 4095]` (required).
- `preferred_biomes`: `HolderSet<Biome>` (required).

Ring position precomputation (`ChunkGeneratorStructureState.generateRingPositions`, executed lazily on first query via `ensureStructuresGenerated` only for sets that have at least one biome-compatible structure in `biomeSource.possibleBiomes()`):
1. If `count == 0`, return empty list.
2. Construct a **legacy 48-bit LCG** `random = LegacyRandomSource(concentricRingsSeed)` (`concentricRingsSeed` is `levelSeed` in normal worlds, `0L` in flat worlds).
3. Initialize:
   - `angle = random.nextDouble() * Math.PI * 2.0` (`f64`, left-to-right: `(nextDouble() * PI) * 2.0`)
   - `positionInCircle = 0`
   - `circle = 0`
   - `curSpread = placement.spread`
4. Loop `i = 0 .. count - 1`:
   1. Compute ring distance in `f64`:
      $$\text{dist} = \text{f64}\big((4 \cdot_{32} \text{distance}) +_{32} ((\text{distance} \cdot_{32} \text{circle}) \cdot_{32} 6)\big) + (\text{random.nextDouble}() - 0.5) \cdot (\text{f64}(\text{distance}) \cdot 2.5)$$
   2. Compute initial chunk coordinates (using JVM `Math.cos` / `Math.sin`, `Math.round(f64) -> i64`, narrowed to `i32`):
      $$\text{initialX} = \text{trunc}_{i32}\big(\text{round}_{i64}(\cos(\text{angle}) \cdot \text{dist})\big), \qquad \text{initialZ} = \text{trunc}_{i32}\big(\text{round}_{i64}(\sin(\text{angle}) \cdot \text{dist})\big)$$
   3. Fork a per-ring-slot legacy RNG **sequentially before advancing the loop**: `biomeSearchRng = random.fork()` (draws one `nextLong()` from `random`).
   4. **Biome-biased position search** (`biomeSource.findBiomeHorizontal` with `originX = (initialX << 4) + 8`, `originY = 0`, `originZ = (initialZ << 4) + 8`, `searchRadius = 112`, `skipSteps = 1`, `findClosest = false`, `allowed = preferred_biomes::contains`):
      - Convert to quart coordinates: `noiseCenterX = originX >> 2 = (initialX << 2) + 2`, `noiseCenterZ = (initialZ << 2) + 2`, `noiseY = 0`, `noiseRadius = 112 >> 2 = 28`.
      - Create a fresh caching point `BiomeResolver` (`biomeSource.createCachingResolver(randomState)`).
      - Initialize `result = null`, `found = 0`.
      - Loop **`z = -28 ..= +28` (outer)**, **`x = -28 ..= +28` (inner)** (step `1`, 3,249 quarts):
        - Query `biome = resolver.getNoiseBiome(noiseCenterX + x, 0, noiseCenterZ + z)`.
        - If `preferred_biomes.contains(biome)`:
          - If `result == null || biomeSearchRng.nextInt(found + 1) == 0` (**short-circuit**: the first matching quart sets `result` without calling `nextInt(1)`!):
            - Set `result = BlockPos((noiseCenterX + x) << 2, 0, (noiseCenterZ + z) << 2)`.
          - Increment `found += 1`.
      - Final chunk for slot `i`: if `result != null`, `ChunkPos(result.x >> 4, result.z >> 4)`; else fallback `ChunkPos(initialX, initialZ)`.
   5. Advance ring state:
      - `angle += (Math.PI * 2.0) / f64(curSpread)`
      - Increment `positionInCircle += 1`.
      - If `positionInCircle == curSpread`:
        - `circle += 1`
        - `positionInCircle = 0`
        - `curSpread += (2 * curSpread) / (circle + 1)` (truncating `i32` division)
        - `curSpread = min(curSpread, count - i)` (uses current loop index `i`, **not** `count - i - 1`!)
        - `angle += random.nextDouble() * Math.PI * 2.0`
5. `isPlacementChunk(state, sourceX, sourceZ)` returns `true` iff `ChunkPos(sourceX, sourceZ)` is contained in the generated list of `count` chunk positions.

#### 1.6 Placement type 3: `minecraft:dimension_origin` *(new in 26.3)*
- JSON fields: none (unit codec `{ "type": "minecraft:dimension_origin" }`). Does **not** support `locate_offset`, `frequency`, `frequency_reduction_method`, `salt`, or `exclusion_zone`.
- `isStructureChunk(state, sourceX, sourceZ)`: returns `sourceX == origin.x && sourceZ == origin.z`, where `origin = state.getDimensionOrigin()`.
- `applyAdditionalChunkRestrictions` is always `true`; `locateOffset` is `(0, 0, 0)`.

#### 1.7 Locate offset
For any placement, `getLocatePos(chunkPos)` returns `BlockPos((chunkPos.x << 4) + locateOffset.x, locateOffset.y, (chunkPos.z << 4) + locateOffset.z)`. This affects `/locate structure` and explorer map targets only; it does not alter block placement.

#### 1.8 Choosing a structure within a set
When `placement.isStructureChunk(state, chunkX, chunkZ)` is `true` for a `StructureSet` with entries `structures`:
- **Single entry (`structures.size() == 1`)**: call `tryGenerateStructure` on `structures[0]` directly (no RNG allocated or consumed).
- **Multiple entries (`structures.size() > 1`)**:
  1. Copy `structures` into a mutable list `options`.
  2. Construct `random = WorldgenRandom(LegacyRandomSource(0L))` and call `random.setLargeFeatureSeed(levelSeed, chunkX, chunkZ)`.
  3. Compute `total = sum(option.weight for option in options)`.
  4. Loop while `!options.isEmpty()`:
     1. Draw `choice = random.nextInt(total)`.
     2. Scan `options` from index `0`: subtract `options[index].weight` from `choice`; stop at the first `index` where `choice < 0`.
     3. Let `selected = options[index]`. Call `tryGenerateStructure(selected, ...)`.
     4. If it returns `true`, stop immediately (at most one structure per set succeeds in a chunk).
     5. Otherwise remove `options[index]` (preserving relative order of remaining entries), subtract `selected.weight` from `total`, and continue.

---

### 2. Chunk steps: `structure_starts` and `structure_references`

In `ChunkPyramid.GENERATION_PYRAMID` (see [`surface-and-carvers.md`](surface-and-carvers.md) §4):
- Step 1 **`structure_starts`**: neighbor requirements = none, write radius = `0`.
- Step 2 **`structure_references`**: neighbor requirements = `structure_starts` at radius `8` (`17 × 17` chunks), write radius = `0`.

#### 2.1 `structure_starts` execution (`ChunkStatusTasks.generateStructureStarts` → `ChunkGenerator.createStructures`)
1. If world options disable structures (`!generateStructures`) or `DEBUG_DISABLE_STRUCTURES` is set, skip generation and jump to step 5.
2. Create one per-chunk `climateSampler = randomState.createClimateSampler(SamplerContext.builder().enableCaches().build())` shared across all structure sets in this chunk.
3. Iterate `state.possibleStructureSets()` in list order (dynamic-registry numeric ID order of `minecraft:worldgen/structure_set`, or override order in flat worlds). For each `StructureSet`:
   1. **Existing valid start guard**: iterate every entry in `set.structures()`; if `centerChunk.getStartForStructure(entry.structure)` is non-null AND `existingStart.isValid()`, skip the entire set.
   2. Evaluate `set.placement().isStructureChunk(state, chunkX, chunkZ)`. If `false`, continue to the next set.
   3. Select and attempt structures per Section 1.8.
4. For each attempted structure (`tryGenerateStructure` → `Structure.generate`):
   1. Preserve any prior reference count: `references = (prevStart != null) ? prevStart.getReferences() : 0`.
   2. Construct `Structure.GenerationContext`:
      - `biomeResolver = biomeSource.createResolver(climateSampler)` (point resolver backed by the chunk's caching `climateSampler`).
      - `random = WorldgenRandom(LegacyRandomSource(0L))`, seeded via `random.setLargeFeatureSeed(levelSeed, chunkX, chunkZ)`.
      - `validBiome = structure.biomes()::contains`.
   3. Call `structure.findValidGenerationPoint(context)`, which evaluates `structure.findGenerationPoint(context).filter(context::isValidBiome)`:
      - `findGenerationPoint` returns `Optional<GenerationStub(position, pieceGenerator)>`. (Pieces are typically supplied as a lazy callback so expensive piece assembly does not run if the biome check fails.)
      - **Biome check (`context.isValidBiome(stub)`)**: converts `stub.position` to quart coordinates `(pos.x >> 2, pos.y >> 2, pos.z >> 2)`, queries `biomeResolver.getNoiseBiome(qx, qy, qz)`, and checks `structure.biomes().contains(biome)`.
      - **26.3 column pre-check helper (`context.couldStructureExistInColumn(blockX, blockZ, minBlockY, maxBlockY)`)**: used by `JigsawPlacement` (when `project_start_to_heightmap` is set) and `Structure.onTopOfChunkCenter` to reject impossible columns before calling `getFirstFreeHeight` / `getFirstOccupiedHeight`. Queries `biomeSource.createResolverForChunk(climateSampler, blockX >> 2, minBlockY >> 2, blockZ >> 2, 1, (maxBlockY >> 2) - (minBlockY >> 2) + 1, 1)` and returns `true` iff any `quartY` in `[minBlockY >> 2, maxBlockY >> 2]` satisfies `validBiome`. **[oracle]**
   4. If a stub survived the biome check:
      - Materialize `StructurePiecesBuilder` by invoking the stub's generator callback (or using its prebuilt builder).
      - Construct `testStart = StructureStart(structure, chunkPos, references, builder.build())`.
      - **Validity rule (`StructureStart.isValid()`)**: `true` iff `!pieceContainer.isEmpty()` (at least one `StructurePiece`).
      - If valid: store `centerChunk.setStartForStructure(structure, testStart)` and return `true`.
        - *Retrogen edge case (`ProtoChunk.setStartForStructure`)*: if `chunk.getBelowZeroRetrogen() != null`, a valid start is silently discarded if `testStart.getBoundingBox().minY < heightAccessor.minY || testStart.getBoundingBox().maxY > heightAccessor.maxY`.
   5. If generation failed or produced zero pieces, `Structure.generate` returns `StructureStart.INVALID_START` and `tryGenerateStructure` returns `false` without storing `INVALID_START` on the chunk.
5. Call `level.onStructureStartsAvailable(chunk)`, updating `StructureCheck`'s in-memory map of valid starts and their reference counts for `chunkPos`.

#### 2.2 `structure_references` execution (`ChunkGenerator.createReferences`)
Given target chunk `(targetX, targetZ)` with block footprint `[targetBlockX, targetBlockZ, targetBlockX + 15, targetBlockZ + 15]`:
1. Loop **`sourceX = targetX - 8 ..= targetX + 8` (outer)**, **`sourceZ = targetZ - 8 ..= targetZ + 8` (inner)** (289 chunks).
2. Compute `sourceChunkKey = ChunkPos.pack(sourceX, sourceZ)`.
3. For every `StructureStart` in `level.getChunk(sourceX, sourceZ).getAllStarts().values()`:
   - Check `start.isValid()` AND `start.getBoundingBox().intersects(targetBlockX, targetBlockZ, targetBlockX + 15, targetBlockZ + 15)` (2D XZ inclusive overlap; Y is ignored).
   - **Adjusted bounding box (`StructureStart.getBoundingBox()`)**: lazily computes `structure.adjustBoundingBox(pieceContainer.calculateBoundingBox())`. For any structure with `terrain_adaptation != "none"`, `Structure.adjustBoundingBox` **inflates the union bounding box of all pieces by `12` blocks in all 6 directions** (`min - 12`, `max + 12`). When `terrain_adaptation == "none"`, it returns the uninflated piece union.
   - If overlapping: call `centerChunk.addReferenceForStructure(start.getStructure(), sourceChunkKey)`, inserting `sourceChunkKey` into that structure's `LongSet` (`LongOpenHashSet`) in `centerChunk.structureReferences`.

#### 2.3 Saved and loaded starts and references (`SerializableChunkData`)
In chunk NBT under root key `"structures"` (`CompoundTag`):
- `"starts"` (`CompoundTag`): maps each structure's canonical identifier string (`"<ns>:<path>"`) to a `StructureStart` compound:
  - If `start.isValid()`: `{ "id": "<structure_id>", "ChunkX": i32, "ChunkZ": i32, "references": i32, "Children": List<CompoundTag> }`.
  - If `!start.isValid()`: `{ "id": "INVALID" }`.
- `"References"` (`CompoundTag`, capital `R`): maps each structure's canonical identifier string (only when its reference set is non-empty) to a `LongArrayTag` of packed `ChunkPos` longs.

Loading rules:
1. **`unpackStructureStart`**: iterates keys of `"starts"`. Unknown structure IDs are logged and skipped. For each known structure, `StructureStart.loadStaticStart(context, startTag, worldSeed)`:
   - If `startTag["id"] == "INVALID"`, returns `StructureStart.INVALID_START`.
   - Looks up structure by `Identifier.parse(startTag["id"])`; if unknown, returns `null` (omitted).
   - Reads `ChunkX`, `ChunkZ` (default `0`), `references` (default `0`), and `Children` list.
   - Loads `PiecesContainer`: for each child tag, lowercases `"id"` (`Locale.ROOT`), parses as `Identifier`, applies legacy piece-ID renames (`minecraft:nvi`, `minecraft:pcp`, `minecraft:bastionremnant`, `minecraft:runtime` $\to$ `minecraft:jigsaw`), and loads via `BuiltInRegistries.STRUCTURE_PIECE`. Unknown piece IDs or per-piece exceptions log and skip that piece.
   - Special case: if the structure is an `OceanMonumentStructure`, calls `OceanMonumentStructure.regeneratePiecesAfterLoad(chunkPos, worldSeed, pieces)`.
2. **Common `StructurePiece` NBT fields**: `"id"` (`String`), `"BB"` (`IntArray[6]`), `"O"` (`i32` horizontal 2D data value or `-1` for `null`), `"GD"` (`i32` generation depth).
3. **`PoolElementStructurePiece` (`minecraft:jigsaw`) additional NBT fields**:
   - `"PosX"`, `"PosY"`, `"PosZ"` (`i32`), `"ground_level_delta"` (`i32`), `"pool_element"` (`StructurePoolElement.CODEC` via registry-backed `NbtOps`), `"rotation"` (`Rotation.LEGACY_CODEC`: uppercase enum names `"NONE"`, `"CLOCKWISE_90"`, `"CLOCKWISE_180"`, `"COUNTERCLOCKWISE_90"`), `"junctions"` (`List<CompoundTag>`, each `{ "source_x": i32, "source_ground_y": i32, "source_z": i32, "delta_y": i32, "dest_proj": "rigid"|"terrain_matching" }`), and optional `"liquid_settings"` (omitted on save when equal to `"apply_waterlogging"`, defaulted to `"apply_waterlogging"` on load).
   - **On load**, `PoolElementStructurePiece` overwrites its `boundingBox` with `element.getBoundingBox(templateManager, position, rotation)` rather than trusting `"BB"` (so any expansion-hack inflation is **not** preserved across save/reload).
4. **`unpackStructureReferences`**: iterates `"References"`; unknown structures are discarded; any packed `ChunkPos` whose chessboard distance `max(|rx - cx|, |rz - cz|) > 8` is logged and filtered out.
5. In `ChunkPyramid.LOADING_PYRAMID`, `ChunkStatusTasks.loadStructureStarts` calls `level.onStructureStartsAvailable(chunk)` on loaded chunks so `StructureCheck` sees their persisted starts.

---

### 3. Common structure schema and the jigsaw structure type

#### 3.1 Common structure registry schema (`minecraft:worldgen/structure`)
Every entry in `minecraft:worldgen/structure` is a JSON object dispatched on `"type"` via `minecraft:worldgen/structure_type` (16 types in 26.3; see **Constants and IDs**), flattening the four common `StructureSettings` fields into the root object:

| JSON field | Type | Required | Default | Meaning |
|---|---|---|---|---|
| `type` | `Identifier` in `minecraft:worldgen/structure_type` | yes | — | Structure implementation type |
| `biomes` | `HolderSet<Biome>` (`#tag`, single ID, or list) | yes | — | Allowed biomes at the generation point |
| `spawn_overrides` | map `{ "<MobCategory>": StructureSpawnOverride }` | yes | — | Per-category mob spawn replacements (may be `{}`) |
| `step` | `GenerationStep.Decoration` string enum | yes | — | Decoration stage during `FEATURES` |
| `terrain_adaptation` | `"none"` \| `"bury"` \| `"beard_thin"` \| `"beard_box"` \| `"encapsulate"` | no | `"none"` | `Beardifier` adaptation mode and `±12` box inflation |

- **`MobCategory` keys**: `"monster"`, `"creature"`, `"ambient"`, `"axolotls"`, `"underground_water_creature"`, `"water_creature"`, `"water_ambient"`, `"misc"`.
- **`StructureSpawnOverride`**: `{ "bounding_box": "piece" | "full", "spawns": WeightedList<SpawnerData> }`. (`"piece"` matches any individual piece box; `"full"` matches `StructureStart.getBoundingBox()`.)

#### 3.2 `minecraft:jigsaw` (`JigsawStructure`) schema
All four common fields plus:

| JSON field | Type / range | Required | Default |
|---|---|---|---|
| `start_pool` | `Holder<StructureTemplatePool>` | yes | — |
| `start_jigsaw_name` | `Identifier` | no | absent |
| `size` | `i32` in `[0, 20]` (`maxDepth`) | yes | — |
| `start_height` | `HeightProvider` (Section 3.3) | yes | — |
| `use_expansion_hack` | `bool` | yes | — |
| `project_start_to_heightmap` | `Heightmap.Types` (`"WORLD_SURFACE_WG"`, `"WORLD_SURFACE"`, `"OCEAN_FLOOR_WG"`, `"OCEAN_FLOOR"`, `"MOTION_BLOCKING"`, `"MOTION_BLOCKING_NO_LEAVES"`) | no | absent |
| `max_distance_from_center` | `i32` in `[1, 128]` (sets both `horizontal` and `vertical`) **or** `{ "horizontal": i32 in [1, 128], "vertical": i32 in [1, 4064] (opt, default 4064) }` | yes | — |
| `pool_aliases` | `List<PoolAliasBinding>` (Section 3.4) | no | `[]` |
| `dimension_padding` | `i32 >= 0` (sets both `bottom` and `top`) **or** `{ "bottom": i32 >= 0 (lenient opt, default 0), "top": i32 >= 0 (lenient opt, default 0) }` | no | `0` (`{bottom: 0, top: 0}`) |
| `liquid_settings` | `"ignore_waterlogging"` \| `"apply_waterlogging"` | no | `"apply_waterlogging"` |

- **Codec validation (`verifyRange`)**: let `edgeNeeded = (terrain_adaptation == "none") ? 0 : 12`. Decode fails if `max_distance_from_center.horizontal + edgeNeeded > 128`.

#### 3.3 `HeightProvider` schema (`minecraft:worldgen/height_provider_type`)
Accepts either an inline `VerticalAnchor` (shorthand for `minecraft:constant`; see [`surface-and-carvers.md`](surface-and-carvers.md) §1.2) or an object dispatched on `"type"`:

| Type ID | JSON fields | Exact sampling rule on `(random, context)` |
|---|---|---|
| `minecraft:constant` | `value`: `VerticalAnchor` | Returns `value.resolveY(context)` (0 RNG draws). |
| `minecraft:uniform` | `min_inclusive`, `max_inclusive`: `VerticalAnchor` | Resolve `min`, `max`; if `min > max`, log once and return `min` (0 draws); else `Mth.randomBetweenInclusive(random, min, max)` (`random.nextInt(max - min + 1) + min`; unconditionally draws even when `min == max`). |
| `minecraft:biased_to_bottom` | `min_inclusive`, `max_inclusive`: `VerticalAnchor`, `inner`: `i32 >= 1` (opt, default `1`) | If `max - min - inner + 1 <= 0`, return `min` (0 draws); else `limit = random.nextInt(max - min - inner + 1)`, return `random.nextInt(limit + inner) + min`. |
| `minecraft:very_biased_to_bottom` | `min_inclusive`, `max_inclusive`: `VerticalAnchor`, `inner`: `i32 >= 1` (opt, default `1`) | If `max - min - inner + 1 <= 0`, return `min` (0 draws); else `u = Mth.nextInt(random, min + inner, max)`, `b = Mth.nextInt(random, min, u - 1)`, return `Mth.nextInt(random, min, b - 1 + inner)`. (Note: `Mth.nextInt(r, lo, hi)` draws only when `lo < hi`!) |
| `minecraft:trapezoid` | `min_inclusive`, `max_inclusive`: `VerticalAnchor`, `plateau`: `i32` (opt, default `0`) | If `min > max`, return `min` (0 draws). Let `range = max - min`. If `plateau >= range`, return `Mth.randomBetweenInclusive(random, min, max)`. Else `pStart = (range - plateau) / 2` (truncating), `pEnd = range - pStart`, return `min + Mth.randomBetweenInclusive(random, 0, pEnd) + Mth.randomBetweenInclusive(random, 0, pStart)`. |
| `minecraft:weighted_list` | `distribution`: non-empty `WeightedList<HeightProvider>` | `distribution.getRandomOrThrow(random).sample(random, context)`. |

#### 3.4 Pool aliases (`PoolAliasBinding`, `minecraft:worldgen/pool_alias_binding`)
Dispatched on `"type"`:
- `minecraft:direct`: `{ "alias": <TemplatePool ID>, "target": <TemplatePool ID> }` — binds `alias -> target` (0 RNG draws).
- `minecraft:random`: `{ "alias": <TemplatePool ID>, "targets": <non-empty WeightedList<TemplatePool ID>> }` — binds `alias -> targets.getRandomOrThrow(random)`.
- `minecraft:random_group`: `{ "groups": <non-empty WeightedList<List<PoolAliasBinding>>> }` — draws one group via `groups.getRandomOrThrow(random)`, then recursively resolves each binding in that group in list order using the same `random`.

Resolution (`PoolAliasLookup.create(poolAliases, startPos, worldSeed)`):
1. If `poolAliases` is empty, return the identity lookup without touching any RNG.
2. Otherwise construct a **legacy 48-bit LCG positional RNG**: `random = LegacyRandomSource(worldSeed).forkPositional().at(startPos.x, startPos.y, startPos.z)`.
3. Resolve every binding in `poolAliases` order into an immutable map (duplicate alias keys fail). Lookups return the mapped target key if present, else the original key unchanged.

#### 3.5 Template pool schema (`minecraft:worldgen/template_pool`) and pool elements
A `StructureTemplatePool` JSON object has two required fields:
- `fallback`: `Holder<StructureTemplatePool>` (recursive/lazy codec).
- `elements`: list of `{ "element": <StructurePoolElement>, "weight": <i32 in [1, 150]> }`.
  - On load, builds a flat weight-expanded list `templates` by appending each `element` `weight` times in input order.
  - `getRandomTemplate(random)`: if `templates` is empty, returns `EmptyPoolElement.INSTANCE` (0 draws); else `templates[random.nextInt(templates.size())]`.
  - `getShuffledTemplates(random)`: `Util.shuffledCopy(templates, random)`.
  - `getMaxSize(templateManager)` (cached lazily): maximum of `e.getBoundingBox(templateManager, ZERO, NONE).ySpan` across all non-`EmptyPoolElement` entries in `templates` (or `0` if none).

**Projection enum (`StructureTemplatePool.Projection`)**:
- `"rigid"`: extra processors = `[]`.
- `"terrain_matching"`: extra processors = `[GravityProcessor(WORLD_SURFACE_WG, -1)]`.

**Pool element types** (`minecraft:worldgen/structure_pool_element`, dispatched on **`"element_type"`**, not `"type"`!):

| `element_type` | JSON fields | Behavior |
|---|---|---|
| `minecraft:single_pool_element` | `location` (`Identifier`, req), `processors` (`Holder<StructureProcessorList>`, req), `projection` (req), `override_liquid_settings` (`LiquidSettings`, opt) | Loads template `location`. `getGroundLevelDelta() = 1`. `getShuffledJigsawBlocks` transforms palette jigsaws, runs `Util.shuffle(jigsaws, random)`, then stably sorts by `selection_priority` **descending**. `getSettings` sets `boundingBox = chunkBB`, `rotation`, `knownShape = true`, `ignoreEntities = false`, `finalizeEntities = true`, `liquidSettings = override_liquid_settings.orElse(structureLiquidSettings)`, and appends processors in exact order: `[BlockIgnoreProcessor.STRUCTURE_BLOCK]`, then `[JigsawReplacementProcessor.INSTANCE]` (if `!keepJigsaws`), then `processors.list()`, then `projection.getProcessors()`. Places with `updateMode = 18` (`UPDATE_CLIENTS \| UPDATE_KNOWN_SHAPE`). |
| `minecraft:legacy_single_pool_element` | Same 4 fields as `single_pool_element` | Identical to `single_pool_element`, except `getSettings` removes `BlockIgnoreProcessor.STRUCTURE_BLOCK` from the start of the processor list and appends `BlockIgnoreProcessor.STRUCTURE_AND_AIR` at the **end** (after `processors` and `projection.getProcessors()`). |
| `minecraft:feature_pool_element` | `feature` (`Holder<PlacedFeature>`, req), `projection` (req) | `getSize = (0,0,0)`; `getBoundingBox` is 1×1×1 box at `position`; `getShuffledJigsawBlocks` returns a singleton list (0 RNG draws) with synthetic jigsaw at `position`, orientation `DOWN_SOUTH`, `jointType = ROLLABLE`, `name = null` (matches any source `target`), `pool = minecraft:empty`, `target = minecraft:empty`, `placementPriority = 0`, `selectionPriority = 0`. `place` calls `feature.place(level, generator, random, position)`. |
| `minecraft:list_pool_element` | `elements` (non-empty `List<StructurePoolElement>`, req), `projection` (req) | Overwrites every child's projection to its own `projection`. `getSize` takes per-axis max of children. `getBoundingBox` encapsulates all non-empty children's boxes. **`getShuffledJigsawBlocks` delegates ONLY to `elements[0]`** (subsequent elements' jigsaws are never queried). `place` places children in order, stopping early and returning `false` if any child returns `false`. |
| `minecraft:empty_pool_element` | *(none)* | Singleton (`EmptyPoolElement.INSTANCE`), projection `TERRAIN_MATCHING`, `getSize = (0,0,0)`, empty jigsaw list (0 draws), `place` returns `true`. Calling `getBoundingBox` throws (must be filtered out). |

#### 3.6 Exact jigsaw assembly algorithm (`JigsawStructure` + `JigsawPlacement`)

##### Phase A — Start piece selection, anchoring, and vertical placement (`JigsawPlacement.addPieces`)
Inputs: `context` (with `context.random` already seeded via `setLargeFeatureSeed(worldSeed, chunkX, chunkZ)`), `start_pool`, `start_jigsaw_name`, `size` (`maxDepth`), `start_height`, `use_expansion_hack`, `project_start_to_heightmap`, `max_distance_from_center`, `pool_aliases`, `dimension_padding`, `liquid_settings`.

1. Sample `height = start_height.sample(context.random, WorldGenerationContext(chunkGenerator, heightAccessor))`. Let `position = BlockPos(chunkX << 4, height, chunkZ << 4)`.
2. Build `poolAliasLookup = PoolAliasLookup.create(pool_aliases, position, worldSeed)` (using its own positional RNG at `position`—before anchoring or heightmap projection!).
3. Draw `centerRotation = Rotation.getRandom(context.random)` (`random.nextInt(4)` indexing `[NONE, CLOCKWISE_90, CLOCKWISE_180, COUNTERCLOCKWISE_90]`).
4. Resolve `centerPool`: if `start_pool` has a registry key `k`, look up `poolAliasLookup.lookup(k)` in `minecraft:worldgen/template_pool`, falling back to `start_pool.value()` if absent; otherwise use `start_pool.value()`.
5. Draw `centerElement = centerPool.getRandomTemplate(context.random)`. If `centerElement == EmptyPoolElement.INSTANCE`, abort (return empty).
6. **Start jigsaw anchor**:
   - If `start_jigsaw_name` is present: call `centerElement.getShuffledJigsawBlocks(templateManager, position, centerRotation, context.random)` and take the **first** jigsaw whose `name` equals `start_jigsaw_name`. If none exists, abort (return empty). Set `anchoredPosition = jigsaw.pos`.
   - Else: `anchoredPosition = position`.
   - Compute `localAnchor = anchoredPosition - position` and `adjustedPosition = position - localAnchor`.
7. Create `centerPiece = PoolElementStructurePiece(templateManager, centerElement, adjustedPosition, centerElement.getGroundLevelDelta(), centerRotation, box, liquid_settings)` where `box = centerElement.getBoundingBox(templateManager, adjustedPosition, centerRotation)`.
8. Compute horizontal center with truncating division: `centerX = (box.maxX + box.minX) / 2`, `centerZ = (box.maxZ + box.minZ) / 2`.
9. Compute `bottomY`:
   - If `project_start_to_heightmap` is absent: `bottomY = adjustedPosition.y`.
   - If present (`hmType`):
     1. Pre-check `context.couldStructureExistInColumn(centerX, centerZ, heightAccessor.minY, heightAccessor.maxY)`. If `false`, abort (return empty).
     2. Set `bottomY = position.y + chunkGenerator.getFirstFreeHeight(centerX, centerZ, hmType, heightAccessor, randomState)` (adds unanchored `position.y`, not `adjustedPosition.y`; for `NoiseBasedChunkGenerator`, `getFirstFreeHeight` equals `getBaseHeight`, i.e., `highestOpaqueY + 1` or `minY` if column is empty).
10. Shift `centerPiece` vertically: `oldAbsoluteGroundY = box.minY + centerPiece.groundLevelDelta`; call `centerPiece.move(0, bottomY - oldAbsoluteGroundY, 0)` (updates both `centerPiece.position` and `box` in place).
11. **Dimension padding check**: if `dimension_padding != ZERO`, abort (return empty) if `box.minY < heightAccessor.minY + dimension_padding.bottom` OR `box.maxY > heightAccessor.maxY - dimension_padding.top`.
12. Set `centerY = bottomY + localAnchor.y`. Return `GenerationStub(BlockPos(centerX, centerY, centerZ), lazyAssemblyCallback)`.
13. **`Structure.findValidGenerationPoint` now tests `context.isValidBiome` at `(centerX, centerY, centerZ)`.** Only if the biome matches does Phase B execute!

##### Phase B — Priority-BFS child expansion (`Placer.tryPlacingChildren`)
1. Initialize `pieces = [centerPiece]`.
2. **If `maxDepth <= 0` (`size == 0`)**: return without adding `centerPiece` to `builder`! (Because `pieces.forEach(builder::addPiece)` sits inside `if (maxDepth > 0)`, a `size == 0` jigsaw structure always produces 0 pieces and an invalid start.)
3. Build the outer allowed volume `outerAABB`:
   - `x ∈ [centerX - maxDist.horizontal, centerX + maxDist.horizontal + 1]`
   - `y ∈ [max(centerY - maxDist.vertical, heightAccessor.minY + dimension_padding.bottom), min(centerY + maxDist.vertical + 1, heightAccessor.maxY + 1 - dimension_padding.top)]`
   - `z ∈ [centerZ - maxDist.horizontal, centerZ + maxDist.horizontal + 1]`
4. Build initial global free space `VoxelShape`: `shape = Shapes.join(Shapes.create(outerAABB), Shapes.create(AABB.of(box)), BooleanOp.ONLY_FIRST)` (`outerAABB \ AABB.of(centerPiece.boundingBox)`).
5. Create `Placer` holding a **`SequencedPriorityIterator<PieceState>`** (`placing`):
   - Maintains a FIFO deque per integer priority key.
   - `next()` always pops from the front of the **highest** non-empty integer priority deque (so higher `placement_priority` values expand first; equal priorities expand in FIFO insertion order).
6. Call `tryPlacingChildren(centerPiece, MutableObject(shape), depth = 0, ...)`, then while `placing` is non-empty, pop `state` and call `tryPlacingChildren(state.piece, state.free, state.depth, ...)`.
7. Append every piece in `pieces` to `builder`.

##### Exact rules and RNG order inside `tryPlacingChildren(sourcePiece, contextFree, depth)`
Let `sourceElement = sourcePiece.element`, `sourceBoxPos = sourcePiece.position`, `sourceRotation = sourcePiece.rotation`, `sourceProjection = sourceElement.projection`, `sourceRigid = (sourceProjection == RIGID)`, `sourceBB = sourcePiece.boundingBox`, `sourceBoxY = sourceBB.minY`, and a fresh local `sourceFree = MutableObject(null)`.

Fetch `sourceJigsaws = sourceElement.getShuffledJigsawBlocks(templateManager, sourceBoxPos, sourceRotation, random)` (consumes shuffle RNG for `sourcePiece`).

For each `sourceJigsaw` in `sourceJigsaws` (in shuffled + `selection_priority`-descending order):
1. `sourceDir = JigsawBlock.getFrontFacing(sourceJigsaw.state)`, `sourceJigsawPos = sourceJigsaw.pos`, `targetJigsawPos = sourceJigsawPos.relative(sourceDir)`.
2. `sourceJigsawLocalY = sourceJigsawPos.y - sourceBoxY`; initialize lazy height cache `sourceJigsawBaseHeight = i32::MIN`.
3. Resolve `poolKey = poolAliasLookup.lookup(sourceJigsaw.pool)`. Look up `targetPool` in `TEMPLATE_POOL`. Skip this jigsaw if missing, or if `targetPool.size() == 0 && !targetPool.is(minecraft:empty)`.
4. Let `fallback = targetPool.getFallback()`. Skip this jigsaw if `fallback.size() == 0 && !fallback.is(minecraft:empty)`.
5. **Free-space scope (`childrenFree`)**:
   - If `sourceBB.isInside(targetJigsawPos)` (the target block sits **inside** `sourceBB`, inclusive): use `sourceFree`, lazily initializing `sourceFree` to `Shapes.create(AABB.of(sourceBB))` on first use. (Internal sub-pieces collide against `sourceBB` and each other, not against `contextFree`.)
   - Otherwise: use `contextFree`.
6. **Candidate template list (`targetPieces`)** — exact RNG consumption:
   - Initialize empty `targetPieces`.
   - If `depth != maxDepth`: append `targetPool.getShuffledTemplates(random)`.
   - Always append `fallback.getShuffledTemplates(random)` (even when `depth < maxDepth`; skipped only if `fallback` has `size <= 1` which consumes 0 RNG).
7. Let `placementPriority = sourceJigsaw.placementPriority`.
8. For each `targetElement` in `targetPieces`:
   - If `targetElement == EmptyPoolElement.INSTANCE`: **`break`** out of the `targetPieces` loop (stops trying remaining pool and fallback entries for this `sourceJigsaw`!).
   - Draw shuffled rotation order `Rotation.getShuffled(random)` (3 draws: `nextInt(4)`, `nextInt(3)`, `nextInt(2)` on `[NONE, CLOCKWISE_90, CLOCKWISE_180, COUNTERCLOCKWISE_90]`).
   - For each `targetRotation` in that shuffled list:
     1. Call `targetJigsaws = targetElement.getShuffledJigsawBlocks(templateManager, BlockPos.ZERO, targetRotation, random)`.
     2. **Expansion hack**: compute `hackBox = targetElement.getBoundingBox(templateManager, BlockPos.ZERO, targetRotation)`.
        - If `use_expansion_hack && hackBox.ySpan <= 16`: for each `tj` in `targetJigsaws`, if `hackBox.isInside(tj.pos.relative(front(tj.state)))`, resolve `childPool = pools.get(poolAliasLookup.lookup(tj.pool))` and compute `max(childPool.getMaxSize(), childPool.fallback.getMaxSize())` (else `0`); set `expandTo` to the maximum across `targetJigsaws` (or `0` if none).
        - Else `expandTo = 0`.
     3. For each `targetJigsaw` in `targetJigsaws`:
        1. **Attachment match (`JigsawBlock.canAttach(sourceJigsaw, targetJigsaw)`)**: requires ALL three:
           - `front(sourceJigsaw.state) == front(targetJigsaw.state).opposite()`
           - `sourceJigsaw.jointType == ROLLABLE || top(sourceJigsaw.state) == top(targetJigsaw.state)`
           - `targetJigsaw.name == null || sourceJigsaw.target.equals(targetJigsaw.name)`
           *(Jigsaw NBT defaults when parsing a template's `minecraft:jigsaw` block: `name`, `target`, `pool` default to `minecraft:empty`; `placement_priority`, `selection_priority` default to `0`; `joint` defaults to `ALIGNED` if `front` is horizontal (`N/S/E/W`), or `ROLLABLE` if `front` is vertical (`UP/DOWN`).)*
        2. If attached, compute tentative position and box:
           - `rawTargetBoxPos = targetJigsawPos - targetJigsaw.pos`
           - `rawTargetBB = targetElement.getBoundingBox(templateManager, rawTargetBoxPos, targetRotation)`
           - `rawTargetY = rawTargetBB.minY`
           - `targetProjection = targetElement.projection`, `targetRigid = (targetProjection == RIGID)`
           - `targetJigsawLocalY = targetJigsaw.pos.y`
           - `deltaY = sourceJigsawLocalY - targetJigsawLocalY + sourceDir.stepY`
           - If `sourceRigid && targetRigid`: `targetBoxY = sourceBoxY + deltaY`.
           - Else: if `sourceJigsawBaseHeight == i32::MIN`, compute `sourceJigsawBaseHeight = chunkGenerator.getFirstFreeHeight(sourceJigsawPos.x, sourceJigsawPos.z, WORLD_SURFACE_WG, heightAccessor, randomState)` (cached for this `sourceJigsaw`); set `targetBoxY = sourceJigsawBaseHeight - targetJigsawLocalY`.
           - `yOffset = targetBoxY - rawTargetY`
           - `targetBB = rawTargetBB.moved(0, yOffset, 0)`
           - `targetBoxPos = rawTargetBoxPos.offset(0, yOffset, 0)`
           - If `expandTo > 0`: `newSize = max(expandTo + 1, targetBB.maxY - targetBB.minY)`; expand `targetBB` to include `(targetBB.minX, targetBB.minY + newSize, targetBB.minZ)`.
        3. **Collision test**: test `!Shapes.joinIsNotEmpty(childrenFree, Shapes.create(AABB.of(targetBB).deflate(0.25)), BooleanOp.ONLY_SECOND)`.
           - Geometrically: the candidate's block-volume AABB `[minX, maxX + 1] × [minY, maxY + 1] × [minZ, maxZ + 1]` shrunk inward by `0.25` on every face (`[minX + 0.25, maxX + 0.75] × ...`) must have **zero** volume outside `childrenFree` (within VoxelShape's `1.0e-7` coordinate tolerance). If any part pokes outside `childrenFree`, reject and try the next `targetJigsaw`.
        4. **Accept piece**:
           1. Subtract the **un-deflated** `AABB.of(targetBB)` from `childrenFree`: `childrenFree = Shapes.joinUnoptimized(childrenFree, Shapes.create(AABB.of(targetBB)), BooleanOp.ONLY_FIRST)`.
           2. `sourceGroundLevelDelta = sourcePiece.groundLevelDelta`.
           3. `targetGroundLevelDelta = targetRigid ? (sourceGroundLevelDelta - deltaY) : targetElement.getGroundLevelDelta()`.
           4. Construct `targetPiece = PoolElementStructurePiece(templateManager, targetElement, targetBoxPos, targetGroundLevelDelta, targetRotation, targetBB, liquidSettings)`.
           5. Compute `junctionY`:
              - If `sourceRigid`: `junctionY = sourceBoxY + sourceJigsawLocalY`.
              - Else if `targetRigid`: `junctionY = targetBoxY + targetJigsawLocalY`.
              - Else: lazily compute `sourceJigsawBaseHeight` if still `i32::MIN`, then `junctionY = sourceJigsawBaseHeight + deltaY / 2` (truncating `i32` division).
           6. Record two `JigsawJunction`s:
              - On `sourcePiece`: `(sourceX = targetJigsawPos.x, sourceGroundY = junctionY - sourceJigsawLocalY + sourceGroundLevelDelta, sourceZ = targetJigsawPos.z, deltaY = +deltaY, destProjection = targetProjection)`.
              - On `targetPiece`: `(sourceX = sourceJigsawPos.x, sourceGroundY = junctionY - targetJigsawLocalY + targetGroundLevelDelta, sourceZ = sourceJigsawPos.z, deltaY = -deltaY, destProjection = sourceProjection)`.
           7. Append `targetPiece` to `pieces`.
           8. If `depth + 1 <= maxDepth`: enqueue `PieceState(targetPiece, childrenFree, depth + 1)` into `placing` at priority `placementPriority`.
           9. Advance immediately to the next `sourceJigsaw` (stop testing remaining jigsaws, rotations, and target elements for this `sourceJigsaw`).

---

### 4. Structure templates, transforms, and processors

#### 4.1 Template discovery and `.nbt` binary format
`StructureTemplateManager` resolves template identifier `<ns>:<path>` through an ordered source chain (first hit wins, cached in memory and cleared on `/reload`):
1. World-generated folder: `<world>/generated/<ns>/structure/<path>.nbt`.
2. Active data-pack stack (`ResourceManager`): `data/<ns>/structure/<path>.nbt` (one-winner lookup per [`datapacks-and-codecs.md`](../codec/datapacks-and-codecs.md) §5.4).

Each `.nbt` file is a gzip-compressed named-root compound read with **unlimited** NBT heap quota (`NbtAccounter.unlimitedHeap()`) and data-fixed from its `DataVersion` (default `500` if absent). Root compound keys:

| Key | NBT type | Meaning |
|---|---|---|
| `DataVersion` | `Int` | Format version (26.3 = `5023`; `< 5006` triggers block-state `Name`/`Properties` $\to$ `id`/`properties` upgrade) |
| `size` | `List<Int>[3]` | `[sizeX, sizeY, sizeZ]` (missing elements default to `0`) |
| `palettes` | `List<List<Compound>>` | Multiple alternate block-state palettes; if present, **`palette` is ignored** |
| `palette` | `List<Compound>` | Single block-state palette (used when `palettes` is absent) |
| `blocks` | `List<Compound>` | Each entry: `pos: List<Int>[3]`, `state: Int` (palette index, default `0`; out-of-range maps to `minecraft:air`), optional `nbt: Compound` |
| `entities` | `List<Compound>` | Each entry: `pos: List<Double>[3]`, `blockPos: List<Int>[3]`, `nbt: Compound` (entries missing `nbt` are skipped) |

- **Palette block-state reading (`NbtUtils.readBlockState`)**: reads `"id"` (block identifier; unknown/missing $\to$ `minecraft:air`) and optional `"properties"` compound of string values (matching [`identifiers-registries-tags.md`](../registry/identifiers-registries-tags.md) §3.3; invalid/unknown property names or values keep the block's default property value).
- **Mandatory block partitioning and sorting on load**: for every palette, `blocks` is partitioned into three lists:
  1. `blockEntitiesList`: entries with `nbt != null`.
  2. `fullBlockList`: entries with `nbt == null`, `!block.hasDynamicShape()`, and `state.isCollisionShapeFullBlock(EmptyBlockGetter, ZERO)`.
  3. `otherBlocksList`: all remaining entries.
  Each list is stably sorted by **`Y` ascending, then `X` ascending, then `Z` ascending**, then concatenated as **`fullBlockList` + `otherBlocksList` + `blockEntitiesList`**.
  - This exact order determines both `Palette.jigsaws()` order (all jigsaws have NBT, so they lie in `blockEntitiesList` in Y-X-Z order) and `placeInWorld` / `processBlockInfos` iteration order.

#### 4.2 Transforms: mirror, rotation, pivot, and bounding box
Mirror is applied **first**, then Rotation around `pivot = (px, py, pz)`:

- **Integer `BlockPos(x, y, z)`**:
  1. Mirror: `NONE` $\to (x, y, z)$; `LEFT_RIGHT` $\to (x, y, -z)$; `FRONT_BACK` $\to (-x, y, z)$.
  2. Rotate around `(px, pz)`:
     - `NONE`: $(x, y, z)$
     - `CLOCKWISE_90`: $(px + pz - z,\; y,\; pz - px + x)$
     - `CLOCKWISE_180`: $(px + px - x,\; y,\; pz + pz - z)$
     - `COUNTERCLOCKWISE_90`: $(px - pz + z,\; y,\; px + pz - x)$
- **Continuous `Vec3(x, y, z)`** (entities):
  1. Mirror: `LEFT_RIGHT` $\to z = 1.0 - z$; `FRONT_BACK` $\to x = 1.0 - x$.
  2. Rotate around `(px, pz)`:
     - `NONE`: $(x, y, z)$
     - `CLOCKWISE_90`: $(px + pz + 1 - z,\; y,\; pz - px + x)$
     - `CLOCKWISE_180`: $(px + px + 1 - x,\; y,\; pz + pz + 1 - z)$
     - `COUNTERCLOCKWISE_90`: $(px - pz + z,\; y,\; px + pz + 1 - x)$
- **Rotated size**: `CLOCKWISE_90` and `COUNTERCLOCKWISE_90` swap X and Z (`(sizeZ, sizeY, sizeX)`); `NONE` and `CLOCKWISE_180` keep `(sizeX, sizeY, sizeZ)`.
- **Bounding box**: let `delta = (sizeX - 1, sizeY - 1, sizeZ - 1)`, `c1 = transform((0,0,0), mirror, rotation, pivot)`, `c2 = transform(delta, mirror, rotation, pivot)`; result is `BoundingBox.fromCorners(c1, c2).moved(position)`.
- **Palette selection (`settings.getRandomPalette(palettes, position)`)**:
  - `settings.getRandom(pos)` returns `settings.random` if explicitly set; else if `pos != null`, a fresh `LegacyRandomSource(Mth.getSeed(pos.x, pos.y, pos.z))`; else `LegacyRandomSource(currentTimeMillis)`.
  - Draws `palettes[settings.getRandom(position).nextInt(palettes.size())]` (always calls `nextInt`, even when `palettes.size() == 1`, advancing `settings.random` if one was explicitly supplied).

#### 4.3 Processor list schema and pipeline (`processBlockInfos`)
`minecraft:worldgen/processor_list` entries accept either `{ "processors": [ <StructureProcessor>, ... ] }` or a bare JSON array `[ <StructureProcessor>, ... ]`. Each processor is dispatched on **`"processor_type"`** via `minecraft:worldgen/structure_processor`.

`StructureTemplate.processBlockInfos(level, position, referencePos, settings, blockInfoList)` executes in two passes:
1. **Whole-piece scan flag**: check whether any processor in `settings.processors` returns `evaluatesEntirePieceState() == true` (in 26.3, **only** `CappedProcessor`). Set `processOnlyInCurrentChunk = !anyWholePiece`.
2. **Pass 1 (per-block `processBlock`)**: iterate `blockInfo` in `blockInfoList` order:
   - Compute `worldPos = transform(blockInfo.pos, mirror, rotation, pivot) + position`.
   - If `processOnlyInCurrentChunk && chunkBB != null && !chunkBB.isInside(worldPos)`: skip completely.
   - Build `cur = StructureBlockInfo(worldPos, blockInfo.state, blockInfo.nbt != null ? blockInfo.nbt.copy() : null)`. (**Important**: `cur.state` is **unmirrored and unrotated** during all processors!)
   - Run every processor's `processBlock(level, position, referencePos, blockInfo.pos, cur, settings)` in list order; stop early if any returns `null`.
   - If `cur != null`, append `cur` to `processedList` and `blockInfo` to `originalList`.
3. **Pass 2 (`finalizeProcessing`)**: for each processor in `settings.processors` order, set `processedList = processor.finalizeProcessing(level, position, referencePos, originalList, processedList, settings)`.

#### 4.4 All 11 built-in structure processor types (`minecraft:worldgen/structure_processor`)

1. **`minecraft:nop`**: no fields; returns block unchanged.
2. **`minecraft:block_ignore`**:
   - Fields: `blocks`: list of `BlockState` (decoded via `BlockState.CODEC`, then reduced to `Block` by discarding properties).
   - Behavior: returns `null` if `processedBlockInfo.state.block` is in `blocks`, else unchanged.
3. **`minecraft:block_rot`**:
   - Fields: `rottable_blocks` (`HolderSet<Block>`, optional), `integrity` (`f32` in `[0.0, 1.0]`, required).
   - Behavior: obtains `random = settings.getRandom(processedBlockInfo.pos)`. If `rottable_blocks` is absent OR `processedBlockInfo.state` is in `rottable_blocks`, draws `random.nextFloat()` and drops (`null`) unless `nextFloat() <= integrity`. (Short-circuits without drawing RNG when the block is not in `rottable_blocks`.)
4. **`minecraft:gravity`**:
   - Fields: `heightmap` (`Heightmap.Types`, opt, default `"WORLD_SURFACE_WG"`), `offset` (`i32`, opt, default `0`).
   - Behavior: if `level` is a live `ServerLevel` (not `WorldGenRegion`), remaps `WORLD_SURFACE_WG -> WORLD_SURFACE` and `OCEAN_FLOOR_WG -> OCEAN_FLOOR`. Computes `height = level.getHeight(hm, pos.x, pos.z) + offset` (`getHeight` returns `firstAvailable = highestBlockY + 1`), and replaces `pos.y` with `height + templateRelativePos.y` (preserving X, Z, state, and NBT).
5. **`minecraft:rule`**:
   - Fields: `rules`: list of `ProcessorRule` objects.
   - Behavior: creates a fresh **`LegacyRandomSource(Mth.getSeed(processedBlockInfo.pos))`** (ignoring `settings.random`!). Iterates `rules` in order, sharing that single `random` across all rules for this block. For the **first** rule whose `test` returns `true`, returns `StructureBlockInfo(pos, rule.output_state, rule.block_entity_modifier.apply(random, processedBlockInfo.nbt))`. If no rule matches, returns `processedBlockInfo` unchanged.
   - **`ProcessorRule` fields**:
     - `input_predicate`: `RuleTest` (required) — tested against `processedBlockInfo.state` (unrotated template state).
     - `location_predicate`: `RuleTest` (required) — tested against existing world block `level.getBlockState(worldPos)` (`AlwaysTrueTest` short-circuits without reading the world).
     - `position_predicate`: `PosRuleTest` (lenient optional, default `minecraft:always_true`).
     - `output_state`: `BlockState` (required).
     - `block_entity_modifier`: `RuleBlockEntityModifier` (lenient optional, default `minecraft:passthrough`).
     - Short-circuit evaluation order: `input_predicate` $\to$ `location_predicate` $\to$ `position_predicate`.
   - **All 10 `RuleTest` types** (`minecraft:rule_test`, dispatched on `"predicate_type"`):

     | `predicate_type` | Fields | Rule on `(state, worldPos, random)` |
     |---|---|---|
     | `minecraft:always_true` | *(none)* | `true` (does not read world block for `location_predicate`) |
     | `minecraft:block_match` | `block` (`Block` ID) | `state.is(block)` |
     | `minecraft:blockstate_match` | `block_state` (`BlockState`) | exact state equality `state == block_state` |
     | `minecraft:tag_match` | `tag` (bare block tag ID, **no `#` prefix**) | `state.is(tag)` |
     | `minecraft:random_block_match` | `block` (`Block` ID), `probability` (`f32`) | `state.is(block) && random.nextFloat() < probability` |
     | `minecraft:random_blockstate_match` | `block_state` (`BlockState`), `probability` (`f32`) | `state == block_state && random.nextFloat() < probability` |
     | `minecraft:height_match` *(26.3)* | `min_inclusive` (`i32`), `max_inclusive` (`i32`) | `min_inclusive <= worldPos.y && worldPos.y <= max_inclusive` |
     | `minecraft:all_of` *(26.3)* | `rules` (`List<RuleTest>`) | short-circuit AND of `rules` in order (`true` if empty) |
     | `minecraft:any_of` *(26.3)* | `rules` (`List<RuleTest>`) | short-circuit OR of `rules` in order (`false` if empty) |
     | `minecraft:not` *(26.3)* | `rule` (`RuleTest`) | `!rule.test(state, worldPos, random)` |

   - **All 3 `PosRuleTest` types** (`minecraft:pos_rule_test`, dispatched on `"predicate_type"`):

     | `predicate_type` | Fields | Rule on `(inTemplatePos, worldPos, worldReference, random)` |
     |---|---|---|
     | `minecraft:always_true` | *(none)* | `true` |
     | `minecraft:linear_pos` | `min_chance` (`f32`, opt `0.0`), `max_chance` (`f32`, opt `0.0`), `min_dist` (`i32`, opt `0`), `max_dist` (`i32`, opt `0`); requires `min_dist < max_dist` | `dist = worldPos.distManhattan(worldReference)`; returns `random.nextFloat() <= Mth.clampedLerp(Mth.inverseLerp(f32(dist), f32(min_dist), f32(max_dist)), min_chance, max_chance)` |
     | `minecraft:axis_aligned_linear_pos` | same 4 fields + `axis` (`"x"`\|`"y"`\|`"z"`, opt `"y"`); requires `min_dist < max_dist` | `dist = trunc_i32(abs(f32(worldPos[axis] - worldReference[axis])))`; same `nextFloat() <= clampedLerp(inverseLerp(...))` check |

   - **All 4 `RuleBlockEntityModifier` types** (`minecraft:rule_block_entity_modifier`, dispatched on `"type"`):

     | `type` | Fields | Rule on `(random, existingTag)` |
     |---|---|---|
     | `minecraft:passthrough` | *(none)* | returns `existingTag` unchanged |
     | `minecraft:clear` | *(none)* | returns a fresh **empty** `CompoundTag()` (non-null!) |
     | `minecraft:append_static` | `data` (`CompoundTag`) | returns `data.copy()` if `existingTag == null`, else `existingTag.merge(data)` (recursive compound merge, overwriting keys in `existingTag`) |
     | `minecraft:append_loot` | `loot_table` (`ResourceKey<LootTable>`) | copies `existingTag` (or creates new `CompoundTag()`), sets `"LootTable"` to canonical ID string and `"LootTableSeed"` to `random.nextLong()` |

6. **`minecraft:protected_blocks`**:
   - Fields: `value` (`HolderSet<Block>`, required; `#tag`, single ID, or list).
   - Behavior: drops (`null`) if `level.getBlockState(processedBlockInfo.pos).is(value)`.
7. **`minecraft:capped`**:
   - Fields: `delegate` (`StructureProcessor`, required), `limit` (`IntProvider` validated in `[1, i32::MAX]`, required).
   - `evaluatesEntirePieceState()` returns **`true`**. `processBlock` is a no-op.
   - `finalizeProcessing`:
     1. If `limit.maxInclusive == 0` or `processedList.isEmpty()` or `originalList.size() != processedList.size()`, return `processedList` unchanged.
     2. Construct `random = SingleThreadedRandomSource(level.getLevel().getSeed()).forkPositional().at(position)` (legacy 48-bit LCG positional factory at the piece's placement `position`).
     3. Sample `maxToReplace = min(limit.sample(random), processedList.size())`. If `maxToReplace < 1`, return unchanged.
     4. Build shuffled index list `indices = Util.toShuffledList(0..processedList.size(), random)` (backward Fisher-Yates).
     5. Initialize `replaced = 0`. Walk `idx` in `indices` while `replaced < maxToReplace`:
        - Call `maybeAltered = delegate.processBlock(level, position, referencePos, originalList[idx].pos, processedList[idx], settings)`.
        - If `maybeAltered != null && !processedList[idx].equals(maybeAltered)`: increment `replaced` and set `processedList[idx] = maybeAltered`.
     6. Return `processedList`.
8. **`minecraft:jigsaw_replacement`**:
   - Fields: none.
   - Behavior: if `!state.is(minecraft:jigsaw)` or `DEBUG_KEEP_JIGSAW_BLOCKS`, return unchanged. If `nbt == null`, log warning and return unchanged. Read `stateString = nbt.getStringOr("final_state", "minecraft:air")` and parse using command block-state syntax (`<block_id>[prop=val,...]`, accepting block states only). On syntax error, log error and return `null`. If parsed block is `minecraft:structure_void`, return `null`; otherwise return `StructureBlockInfo(pos, parsedState, null)` (clearing NBT).
9. **`minecraft:lava_submerged_block`**:
   - Fields: none.
   - Behavior: if `level.getBlockState(pos).is(minecraft:lava)` AND `!Block.isShapeFullBlock(processedBlockInfo.state.getShape(level, pos))`, replace state with `minecraft:lava` default state (retaining `nbt`); else unchanged.
10. **`minecraft:block_age`**:
    - Fields: `mossiness` (`f32`, required).
    - Behavior: obtains `random = settings.getRandom(processedBlockInfo.pos)` and dispatches by input state (exact RNG draw order is mandatory):
      - `stone_bricks`, `stone`, or `chiseled_stone_bricks`: if `random.nextFloat() < 0.5f`:
        1. Build `nonMossyStairs` = `stone_brick_stairs` with `FACING = HORIZONTAL[random.nextInt(4)]` (`[NORTH, EAST, SOUTH, WEST]`) and `HALF = Half[random.nextInt(2)]` (`[TOP, BOTTOM]`).
        2. Build `mossyStairs` = `mossy_stone_brick_stairs` with `FACING = HORIZONTAL[random.nextInt(4)]` and `HALF = Half[random.nextInt(2)]`. *(Both stairs are rolled unconditionally before checking `mossiness`!)*
        3. Draw `isMossy = random.nextFloat() < mossiness`, then `pick = random.nextInt(2)`.
        4. Select `[mossy_stone_bricks, mossyStairs][pick]` if `isMossy`, else `[cracked_stone_bricks, nonMossyStairs][pick]`.
      - Else if in `#minecraft:stairs`: if `random.nextFloat() < 0.5f`:
        - Draw `isMossy = random.nextFloat() < mossiness`, then `pick = random.nextInt(2)`.
        - Select `[mossy_stone_brick_stairs.withPropertiesOf(state), mossy_stone_brick_slab][pick]` if `isMossy`, else `[stone_slab, stone_brick_slab][pick]` (default states).
      - Else if in `#minecraft:slabs`: if `random.nextFloat() < mossiness`, replace with `mossy_stone_brick_slab.withPropertiesOf(state)`.
      - Else if in `#minecraft:walls`: if `random.nextFloat() < mossiness`, replace with `mossy_stone_brick_wall.withPropertiesOf(state)`.
      - Else if `minecraft:obsidian`: if `random.nextFloat() < 0.15f`, replace with `minecraft:crying_obsidian` default state.
      - (`withPropertiesOf(src)` copies every property shared between `src` and the replacement block, keeping defaults for any others.)
11. **`minecraft:blackstone_replace`**:
    - Fields: none.
    - Behavior: looks up `state.block` in the fixed 23-entry stone-to-blackstone replacement map (see **Constants and IDs**; notably `minecraft:iron_bars -> minecraft:iron_chain` in 26.3). If absent, returns unchanged. Otherwise starts from `newBlock.defaultBlockState()` and copies **only** `StairBlock.FACING`, `StairBlock.HALF`, and `SlabBlock.TYPE` if present on `oldState` (all other properties—including stair `SHAPE`, `WATERLOGGED`, and wall side connections—reset to defaults).

#### 4.5 Block placement, liquid handling, block entities, and entities (`StructureTemplate.placeInWorld`)
Inputs: `(level, position, referencePos, settings, random, updateMode)`:
1. If `palettes` is empty, return `false`.
2. Select `blockInfoList = settings.getRandomPalette(palettes, position).blocks()`.
3. If `(blockInfoList.isEmpty() && (settings.isIgnoreEntities() || entityInfoList.isEmpty())) || size.x < 1 || size.y < 1 || size.z < 1`, return `false`.
4. Run `processedList = processBlockInfos(level, position, referencePos, settings, blockInfoList)`.
5. **Block placement loop** — iterate `blockInfo` in `processedList` order; skip if `boundingBox != null && !boundingBox.isInside(blockInfo.pos)`:
   1. If `settings.shouldApplyWaterlogging()` (`liquid_settings == "apply_waterlogging"`), record `previousFluidState = level.getFluidState(blockPos)`.
   2. Transform state: `state = blockInfo.state.mirror(settings.mirror).rotate(settings.rotation)`.
   3. If `blockInfo.nbt != null`: first place `minecraft:barrier` with flags `820` (`UPDATE_INVISIBLE | UPDATE_KNOWN_SHAPE | UPDATE_SUPPRESS_DROPS | UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS | UPDATE_SKIP_ON_PLACE`) to clear any pre-existing block entity cleanly without dropping contents.
   4. Place `state` via `level.setBlock(blockPos, state, updateMode)` (for `SinglePoolElement`, `updateMode = 18 = UPDATE_CLIENTS | UPDATE_KNOWN_SHAPE`).
   5. If `setBlock` returns `true`:
      - Expand placed extents `(minX..maxZ)` and record `(blockPos, blockInfo.nbt)` in `placed`.
      - **Block entity loading** (if `blockInfo.nbt != null`): fetch `blockEntity = level.getBlockEntity(blockPos)` (in `WorldGenRegion`, lazily instantiates from the `"DUMMY"` placeholder created by `setBlock`). If non-null:
        - If `!DEBUG_STRUCTURE_EDIT_MODE` and `blockEntity` implements `RandomizableContainer` (`Barrel`, `Chest`/`TrappedChest`, `Crafter`, `DecoratedPot`, `Dispenser`/`Dropper`, `Hopper`, `ShulkerBox`—**not** `BrushableBlockEntity`!), write `blockInfo.nbt["LootTableSeed"] = random.nextLong()` using `placeInWorld`'s `random` parameter (overwriting any `LootTableSeed` previously set by `AppendLoot`).
        - Call `blockEntity.loadWithComponents(blockInfo.nbt)`.
      - **Initial waterlogging** (if `previousFluidState != null`):
        - If `state.getFluidState().isSource()`: add `blockPos` to `lockedFluids` (prevents structure-placed fluid sources from flooding adjacent waterloggable blocks in step 6).
        - Else if `state.block` implements `LiquidBlockContainer` (`SimpleWaterloggedBlock`, `KelpBlock`, `KelpPlantBlock`, `SeagrassBlock`, `TallSeagrassBlock`):
          - Call `placeLiquid(level, blockPos, state, previousFluidState)` (for `SimpleWaterloggedBlock`, sets `WATERLOGGED = true` and schedules a water tick iff `!waterlogged && previousFluidState.is(minecraft:water)`; `SlabBlock` refuses `TYPE == DOUBLE`).
          - If `!previousFluidState.isSource()`: add `blockPos` to `toFill`.
6. **Fluid flood-fill pass** (runs while `filled && !toFill.isEmpty()`, only populated when `APPLY_WATERLOGGING` is active):
   - Directions tested in exact order: **`[UP, NORTH, EAST, SOUTH, WEST]`** (`DOWN` is never checked).
   - For each `pos` in `toFill` (in insertion order): read `toPlace = level.getFluidState(pos)`. While `!toPlace.isSource()`, inspect each neighbor in the 5 directions; the first neighbor whose `FluidState` is a source AND is **not** in `lockedFluids` becomes `toPlace`. If `toPlace.isSource()` and `level.getBlockState(pos).block` is a `LiquidBlockContainer`, call `placeLiquid(level, pos, state, toPlace)`, set `filled = true`, and remove `pos` from `toFill`. Repeat until a full pass fills no blocks.
7. **Shape updates and dirty marking** (if at least one block was placed, `minX <= maxX`):
   - If `!settings.getKnownShape()` (skipped by `SinglePoolElement`, which sets `knownShape = true`): build a `BitSetDiscreteVoxelShape` of all placed positions, run `updateShapeAtEdge` on boundary faces (`updateMode & ~1`), then for each placed position run `Block.updateFromNeighbourShapes` (setting changed states with `(updateMode & ~1) | 16`) and `level.updateNeighborsAt`.
   - For every entry in `placed` with `nbt != null`: call `blockEntity.setChanged()`.
8. **Entity placement** (if `!settings.isIgnoreEntities()`):
   - For each `entityInfo` in `entityInfoList`: compute `blockPos = transform(entityInfo.blockPos, mirror, rotation, pivot) + position`. Skip if `boundingBox != null && !boundingBox.isInside(blockPos)`.
   - Copy `entityInfo.nbt`; compute `pos = transform(entityInfo.pos, mirror, rotation, pivot) + (position.x, position.y, position.z)`; overwrite `"Pos"` to `[pos.x, pos.y, pos.z]`; remove `"UUID"`; if `"block_pos"` is present, overwrite it with `blockPos`.
   - Instantiate entity with `EntitySpawnReason.STRUCTURE` (silently skipping on exception), compute `yRot = entity.rotate(rotation) + entity.mirror(mirror) - entity.getYRot()`, `snapTo(pos, yRot, xRot)`, set body/head yaw to `yRot`, call `mob.finalizeSpawn(level, difficulty, STRUCTURE, null)` if `finalizeEntities && entity instanceof Mob`, and add via `level.addFreshEntityWithPassengers(entity)` (which serializes non-passenger entities into `ProtoChunk.entities` during worldgen).
9. `SinglePoolElement.place` post-step: filters `DATA`-mode `minecraft:structure_block`s through `processBlockInfos` and invokes `handleDataMarker` (a no-op on `SinglePoolElement`, and `BlockIgnoreProcessor.STRUCTURE_BLOCK` already removes them).

---

### 5. Structure placement in the `features` step and `Beardifier` contract

#### 5.1 `features` step execution (`ChunkStatusTasks.generateFeatures` → `ChunkGenerator.applyBiomeDecoration`)
`ChunkStatus.FEATURES` requires `STRUCTURE_STARTS` (radius `8`) and `TERRAIN` (radius `1`), with `writeRadius = 1` (`3 × 3` chunks writable via `WorldGenRegion`).

1. Skip if `DEBUG_DISABLE_FEATURES` or `debugVoidTerrain(centerPos)`.
2. Compute `origin = BlockPos(chunkX << 4, level.minSectionY << 4, chunkZ << 4)`.
3. Group all registered structures from `minecraft:worldgen/structure` (iterated in **dynamic-registry numeric ID order**) into `structuresByStep[structure.step().ordinal()]`.
4. Instantiate decoration RNG: **`WorldgenRandom(XoroshiroRandomSource(uniqueSeed))`** (always xoroshiro128++ regardless of dimension settings) and immediately overwrite its seed with `decorationSeed = random.setDecorationSeed(level.getSeed(), origin.x, origin.z)`.
5. Compute the chunk's `writableArea` (`ChunkGenerator.getWritableArea`):
   $$\text{writableArea} = \text{BoundingBox}\big(\text{chunkMinX},\; \text{heightAccessor.minY} + 1,\; \text{chunkMinZ},\; \text{chunkMinX} + 15,\; \text{heightAccessor.maxY},\; \text{chunkMinZ} + 15\big)$$
   Note the **`minY + 1`** lower bound (the very bottom layer `minY` is excluded).
6. Loop `stepIndex = 0 .. max(11, featureStepCount) - 1`:
   1. **Structures for `stepIndex` execute FIRST** (before biome features of the same `stepIndex`):
      - If `structureManager.shouldGenerateStructures()`:
      - Initialize `index = 0`. For each `structure` in `structuresByStep[stepIndex]` (in registry numeric ID order):
        1. Call `random.setFeatureSeed(decorationSeed, index, stepIndex)`.
        2. Fetch `starts = structureManager.startsForStructure(chunkX, chunkZ, structure)`, which iterates `chunk.getReferencesForStructure(structure)` (`LongOpenHashSet` of start chunk keys) and resolves each valid `StructureStart` from its origin chunk.
        3. For each `start` in `starts`: call `start.placeInChunk(level, structureManager, generator, random, writableArea, centerPos)`:
           - If `start.pieces` is empty, return.
           - Compute `referencePos` from `pieces[0].boundingBox`: `center = pieces[0].boundingBox.getCenter()`, `referencePos = BlockPos(center.x, pieces[0].boundingBox.minY, center.z)`.
           - For each `piece` in `start.pieces` in list order: if `piece.boundingBox.intersects(writableArea)` (3D inclusive), call `piece.postProcess(level, structureManager, generator, random, writableArea, centerPos, referencePos)`.
           - Call `structure.afterPlace(level, structureManager, generator, random, writableArea, centerPos, pieceContainer)` (no-op for `JigsawStructure`).
        4. Increment `index += 1`.
   2. **Biome features for `stepIndex` execute SECOND** (collecting feature indices from the `3 × 3` neighborhood's biomes, sorting ascending, seeding `random.setFeatureSeed(decorationSeed, globalIndex, stepIndex)` per feature).
7. Run `Blender.generateBorderTicks(region, chunk)`.

#### 5.2 Data contract between structures and `Beardifier`
`Beardifier`'s exact kernel and `f32` evaluation formulas are specified in [`worldgen/density-functions.md`](./density-functions.md) §8. During `ChunkStatus.TERRAIN`, `Beardifier.forStructuresInChunk(structureManager, chunkPos)` consumes the following data produced by `STRUCTURE_STARTS` and `STRUCTURE_REFERENCES`:

1. **Start lookup**: queries `structureManager.startsForStructure(chunkX, chunkZ, s -> s.terrainAdaptation() != NONE)`, which iterates `chunk.getAllReferences()` (a `HashMap<Structure, LongSet>`) and resolves every valid start referenced by this chunk. Because `StructureStart.getBoundingBox()` inflates piece bounds by `±12` when `terrain_adaptation != "none"`, every piece within 12 blocks horizontally of the chunk is guaranteed to be referenced in `chunk.structureReferences`.
2. **Piece filter**: for each `start` and each `piece` in `start.getPieces()` (in piece list order), skips `piece` unless `piece.boundingBox` intersects `[cx - 12, cz - 12, cx + 27, cz + 27]` (inclusive on all four horizontal edges, where `cx = chunkX << 4`, `cz = chunkZ << 4`).
3. **Emitted `Rigid` records**:
   - If `piece` is a `PoolElementStructurePiece`:
     - Emits `Rigid(box = piece.boundingBox, terrainAdjustment = start.structure.terrainAdaptation, groundLevelDelta = piece.groundLevelDelta)` **only if `piece.element.projection == RIGID`**.
     - `TERRAIN_MATCHING` pool pieces emit **no** `Rigid` record.
   - If `piece` is any other `StructurePiece` subclass: emits `Rigid(box = piece.boundingBox, terrainAdjustment = start.structure.terrainAdaptation, groundLevelDelta = 0)`.
4. **Emitted `JigsawJunction` records**:
   - For every `PoolElementStructurePiece` that passed step 2 (regardless of whether its projection is `RIGID` or `TERRAIN_MATCHING`), iterates `piece.getJunctions()` in insertion order and emits every junction satisfying **`sourceX > cx - 12 && sourceZ > cz - 12 && sourceX < cx + 27 && sourceZ < cz + 27`** (**strict** inequalities, unlike step 2's inclusive test!).
   - `Beardifier` reads only `(sourceX, sourceGroundY, sourceZ)` from each `JigsawJunction`; `deltaY` and `destProjection` are persisted in NBT but unused by `Beardifier`.
5. **`affectedBox` construction**: encapsulates every emitted `Rigid.box` and every emitted junction's 1×1×1 box `(sourceX, sourceGroundY, sourceZ)`, then inflates by `24` blocks on all 6 sides (`inflatedBy(24)`). If nothing was emitted, returns `Beardifier.EMPTY`. **[oracle]**

---

## Edge cases and known quirks

1. **Swapped arguments in `default` frequency reducer**: `AbstractSpreadingStructurePlacement.probabilityReducer` calls `setLargeFeatureWithSalt(seed, salt, sourceX, sourceZ)` (putting `salt` in the `x` slot, `sourceX` in `z`, and `sourceZ` in `blend`), whereas `legacy_type_2` calls `setLargeFeatureWithSalt(seed, sourceX, sourceZ, 10387320)`. Using `(sourceX, sourceZ, salt)` for `default` breaks placement parity.
2. **32-bit sign extension in `legacy_type_1` frequency reducer**: `cx ^ (cz << 4)` is evaluated in 32-bit `i32` before widening to `i64` and XORing with `seed`, and one `nextInt()` is drawn and discarded before `nextInt(trunc_i32(1.0f / probability)) == 0`.
3. **`ConcentricRings` ring-spread `count - i` off-by-one and `findBiomeHorizontal` reservoir short-circuit**:
   - When advancing to a new ring, `spread` clamps to `min(spread, count - i)` using the current loop index `i` (before `i++`), not remaining count `count - i - 1`.
   - In `BiomeSource.findBiomeHorizontal`, `result == null || random.nextInt(found + 1) == 0` skips `nextInt(1)` on the first matching quart (`found == 0`).
   - Flat worlds always use `concentricRingsSeed = 0L`, ignoring `levelSeed`.
4. **`JigsawStructure` with `size == 0` always fails**: in `JigsawPlacement.addPieces`, `pieces.forEach(builder::addPiece)` sits inside `if (maxDepth > 0)`. Thus `size: 0` builds a `centerPiece`, never adds it to `builder`, and produces `INVALID_START`.
5. **`JigsawPlacement` center truncation and unanchored `position.y`**:
   - `centerX = (box.maxX + box.minX) / 2` and `centerZ = (box.maxZ + box.minZ) / 2` use truncating integer division toward zero (not `floorDiv`), affecting negative coordinates when `min + max` is odd.
   - When `project_start_to_heightmap` is set, `bottomY = position.y + getFirstFreeHeight(...)` adds unanchored `position.y` rather than `adjustedPosition.y`.
   - `PoolAliasLookup` is seeded at unanchored, unprojected `(chunkMinX, startHeightSample, chunkMinZ)`.
6. **`JigsawPlacement` `EmptyPoolElement` breaks out of fallback too**: hitting `EmptyPoolElement.INSTANCE` while scanning `targetPieces` executes `break`, abandoning all remaining entries in both the primary pool and the fallback pool for that source jigsaw.
7. **`JigsawPlacement` fallback pool is shuffled even when `depth < maxDepth`**: `fallback.getShuffledTemplates(random)` is appended unconditionally before trying any element, consuming RNG draws whenever `fallback.size() > 1` even if a primary-pool piece succeeds immediately.
8. **Jigsaw pieces at `depth == maxDepth` are still enqueued**: the enqueue condition is `depth + 1 <= maxDepth`, so pieces at `depth == maxDepth` are popped and expand using **only** their fallback pools (skipping primary pools).
9. **Jigsaw `ListPoolElement` only exposes `elements[0]`'s jigsaws**: `ListPoolElement.getShuffledJigsawBlocks` ignores elements `1..k-1`.
10. **`LegacySinglePoolElement` moves `BlockIgnoreProcessor` to the end**: removing `STRUCTURE_BLOCK` and appending `STRUCTURE_AND_AIR` places air/structure-block ignoring **after** all custom pool processors and `GravityProcessor`, so custom processors still see template `minecraft:air` blocks (and can replace them before `STRUCTURE_AND_AIR` drops remaining air).
11. **`PoolElementStructurePiece` recomputes `boundingBox` on chunk reload**: loading from NBT discards `"BB"` and recomputes from `element.getBoundingBox`, dropping any `use_expansion_hack` vertical inflation.
12. **Processors see unrotated, unmirrored template block states**: `StructureTemplate.processBlockInfos` runs before `state.mirror(...).rotate(...)`, so `input_predicate` in `RuleProcessor` and block checks in `BlockAgeProcessor`/`BlackstoneReplaceProcessor` inspect template-local states (whereas `location_predicate`, `ProtectedBlockProcessor`, and `LavaSubmergedBlockProcessor` inspect already-placed world states at the transformed `worldPos`).
13. **`BlockAgeProcessor` unconditionally rolls both stair states before testing `mossiness`**: for full stone/stone-brick blocks that pass the `0.5f` check, `getRandomFacingStairs` is called twice (4 RNG draws: 2 directions + 2 halves) before `random.nextFloat() < mossiness` and `random.nextInt(2)`.
14. **`AppendLoot` vs `RandomizableContainer` seed overwrite**: in `StructureTemplate.placeInWorld`, any block entity implementing `RandomizableContainer` has its `LootTableSeed` overwritten with `placeInWorld`'s `random.nextLong()` (the `FEATURES` decoration xoroshiro RNG), overriding any `LootTableSeed` written by `AppendLoot` (which used `RuleProcessor`'s legacy positional RNG). Non-`RandomizableContainer` block entities such as `BrushableBlockEntity` retain `AppendLoot`'s seed.
15. **`Clear` block-entity modifier returns empty `CompoundTag`, not `null`**: because `nbt != null`, `placeInWorld` still performs the `minecraft:barrier` pre-clear and `loadWithComponents` with an empty compound.
16. **`StructureTemplate.placeInWorld` waterlogging `lockedFluids` and 5-direction flood-fill**: structure-placed source fluids are locked so they never flood neighboring waterloggable blocks in the same template; the flood-fill checks `[UP, NORTH, EAST, SOUTH, WEST]` (never `DOWN`) and repeats to a fixed point.
17. **`CappedProcessor` forces whole-piece processing across chunk borders**: because `CappedProcessor.evaluatesEntirePieceState()` is `true`, `processBlockInfos` runs Pass 1 on **every** block in the template even outside `chunkBB`, ensuring `CappedProcessor`'s shuffled index space and replacement cap are invariant across every chunk overlapping the piece. **[oracle]**
18. **`writableArea` excludes `minY`**: `ChunkGenerator.getWritableArea` sets `minY = heightAccessor.minY + 1`, so structure post-processing cannot place blocks on the bottommost world layer (`-64` in Overworld, `0` in Nether/End).
19. **`Beardifier` inclusive piece check vs strict junction check**: `piece.isCloseToChunk(chunkPos, 12)` uses inclusive `<=` / `>=` on `[cx - 12, cx + 27]`, while junction filtering uses strict `<` / `>` (`junctionX > cx - 12 && junctionX < cx + 27`).
20. **`DimensionOriginStructurePlacement` is ignored by `findNearestMapStructure`**: in 26.3, `ChunkGenerator.findNearestMapStructure` only handles `ConcentricRingsStructurePlacement` and `RandomSpreadStructurePlacement` (it does not branch on `DimensionOriginStructurePlacement`).

---

## Constants and IDs

### Registry IDs
- Dynamic registries: `minecraft:worldgen/structure_set`, `minecraft:worldgen/structure`, `minecraft:worldgen/template_pool`, `minecraft:worldgen/processor_list`.
- Built-in dispatch registries:
  - `minecraft:worldgen/structure_placement` (3): `minecraft:concentric_rings`, `minecraft:dimension_origin`, `minecraft:random_spread`.
  - `minecraft:worldgen/structure_type` (16): `minecraft:buried_treasure`, `minecraft:desert_pyramid`, `minecraft:end_city`, `minecraft:fortress`, `minecraft:igloo`, `minecraft:jigsaw`, `minecraft:jungle_temple`, `minecraft:mineshaft`, `minecraft:nether_fossil`, `minecraft:ocean_monument`, `minecraft:ocean_ruin`, `minecraft:ruined_portal`, `minecraft:shipwreck`, `minecraft:stronghold`, `minecraft:swamp_hut`, `minecraft:woodland_mansion`.
  - `minecraft:worldgen/structure_pool_element` (5): `minecraft:single_pool_element`, `minecraft:list_pool_element`, `minecraft:feature_pool_element`, `minecraft:empty_pool_element`, `minecraft:legacy_single_pool_element`.
  - `minecraft:worldgen/pool_alias_binding` (3): `minecraft:random`, `minecraft:random_group`, `minecraft:direct`.
  - `minecraft:worldgen/structure_processor` (11): `minecraft:blackstone_replace`, `minecraft:block_age`, `minecraft:block_ignore`, `minecraft:block_rot`, `minecraft:capped`, `minecraft:gravity`, `minecraft:jigsaw_replacement`, `minecraft:lava_submerged_block`, `minecraft:nop`, `minecraft:protected_blocks`, `minecraft:rule`.
  - `minecraft:worldgen/rule_test` (10): `minecraft:all_of`, `minecraft:always_true`, `minecraft:any_of`, `minecraft:block_match`, `minecraft:blockstate_match`, `minecraft:not`, `minecraft:tag_match`, `minecraft:height_match`, `minecraft:random_block_match`, `minecraft:random_blockstate_match`.
  - `minecraft:worldgen/pos_rule_test` (3): `minecraft:always_true`, `minecraft:linear_pos`, `minecraft:axis_aligned_linear_pos`.
  - `minecraft:worldgen/rule_block_entity_modifier` (4): `minecraft:clear`, `minecraft:passthrough`, `minecraft:append_static`, `minecraft:append_loot`.
  - `minecraft:worldgen/height_provider_type` (6): `minecraft:constant`, `minecraft:uniform`, `minecraft:biased_to_bottom`, `minecraft:very_biased_to_bottom`, `minecraft:trapezoid`, `minecraft:weighted_list`.

### `GenerationStep.Decoration` ordinals (0..10)
`0: raw_generation`, `1: lakes`, `2: local_modifications`, `3: underground_structures`, `4: surface_structures`, `5: strongholds`, `6: underground_ores`, `7: underground_decoration`, `8: fluid_springs`, `9: vegetal_decoration`, `10: top_layer_modification`.

### Numeric constants
| Constant | Value | Where used |
|---|---|---|
| `setLargeFeatureWithSalt` X / Z multipliers | `341873128712L`, `132897987541L` | `RandomSpread`, frequency reducers |
| `HIGHLY_ARBITRARY_RANDOM_SALT` | `10387320` | `legacy_type_2` frequency reducer |
| `setFeatureSeed` step multiplier | `10000` | `applyBiomeDecoration` |
| `ConcentricRings` biome search radius | `112` blocks (`28` quarts) | `generateRingPositions` |
| `STRUCTURE_REFERENCES` scan radius | `8` chunks (`17 × 17`) | `createReferences`, `unpackStructureReferences` |
| `Beardifier` box inflation & search margin | `12` blocks (`affectedBox` inflation: `24`) | `Structure.adjustBoundingBox`, `Beardifier` |
| `JigsawStructure` max total horizontal range | `128` blocks (`horizontal + edgeNeeded <= 128`) | `JigsawStructure.verifyRange` |
| `JigsawStructure` `size` / `maxDepth` range | `[0, 20]` | `JigsawStructure.CODEC` |
| `JigsawPlacement` collision AABB deflation | `0.25` blocks | `Placer.tryPlacingChildren` |
| `JigsawPlacement` expansion hack max `ySpan` | `16` blocks | `Placer.tryPlacingChildren` |
| `StructureTemplate` barrier pre-clear flags | `820` (`4 \| 16 \| 32 \| 256 \| 512`) | `StructureTemplate.placeInWorld` |
| `SinglePoolElement` block update flags | `18` (`2 \| 16`) | `SinglePoolElement.place` |
| `BlockAgeProcessor` replacement probabilities | full stone `0.5f`, stairs `0.5f`, obsidian `0.15f` | `BlockAgeProcessor` |
| Block-state `id`/`properties` rename DataVersion | `5006` (26.3 current = `5023`) | `StructureTemplate` / `NbtUtils` |

### `BlackstoneReplaceProcessor` 23-entry block mapping (all in `minecraft:` namespace)
| Input block | Output block | Input block | Output block |
|---|---|---|---|
| `cobblestone` | `blackstone` | `mossy_cobblestone` | `blackstone` |
| `stone` | `polished_blackstone` | `stone_bricks` | `polished_blackstone_bricks` |
| `mossy_stone_bricks` | `polished_blackstone_bricks` | `chiseled_stone_bricks` | `chiseled_polished_blackstone` |
| `cracked_stone_bricks` | `cracked_polished_blackstone_bricks` | `iron_bars` | `iron_chain` |
| `cobblestone_stairs` | `blackstone_stairs` | `mossy_cobblestone_stairs` | `blackstone_stairs` |
| `stone_stairs` | `polished_blackstone_stairs` | `stone_brick_stairs` | `polished_blackstone_brick_stairs` |
| `mossy_stone_brick_stairs` | `polished_blackstone_brick_stairs` | `cobblestone_slab` | `blackstone_slab` |
| `mossy_cobblestone_slab` | `blackstone_slab` | `smooth_stone_slab` | `polished_blackstone_slab` |
| `stone_slab` | `polished_blackstone_slab` | `stone_brick_slab` | `polished_blackstone_brick_slab` |
| `mossy_stone_brick_slab` | `polished_blackstone_brick_slab` | `cobblestone_wall` | `blackstone_wall` |
| `mossy_cobblestone_wall` | `blackstone_wall` | `stone_brick_wall` | `polished_blackstone_brick_wall` |
| `mossy_stone_brick_wall` | `polished_blackstone_brick_wall` | | |

---

## Oracle probe points

Public classes and methods a black-box parity harness can call to capture intermediate structure placement, jigsaw assembly, template processing, and chunk output:

- `net.minecraft.world.level.chunk.ChunkGeneratorStructureState`:
  - `createForNormal(...)`, `createForFlat(...)`, `ensureStructuresGenerated()`
  - `getRingPositionsFor(ConcentricRingsStructurePlacement)` $\to$ `List<ChunkPos>`
  - `getDimensionOrigin()` $\to$ `ChunkPos`
  - `hasStructureChunkInRange(Holder<StructureSet>, int, int, int)` $\to$ `boolean`
- `net.minecraft.world.level.levelgen.structure.placement.StructurePlacement` / `RandomSpreadStructurePlacement` / `ConcentricRingsStructurePlacement` / `DimensionOriginStructurePlacement`:
  - `isStructureChunk(ChunkGeneratorStructureState, int, int)` $\to$ `boolean`
  - `applyAdditionalChunkRestrictions(int, int, long)` $\to$ `boolean`
  - `RandomSpreadStructurePlacement.getPotentialStructureChunk(long, int, int)` $\to$ `ChunkPos`
- `net.minecraft.world.level.chunk.ChunkGenerator`:
  - `createStructures(registryAccess, state, structureManager, centerChunk, templateManager, dimension)`
  - `createReferences(level, structureManager, centerChunk)`
  - `applyBiomeDecoration(level, chunk, structureManager)`
- `net.minecraft.world.level.levelgen.structure.Structure` & `StructureStart`:
  - `Structure.generate(...)` $\to$ `StructureStart`
  - `StructureStart.getPieces()`, `StructureStart.getBoundingBox()`, `StructureStart.isValid()`, `StructureStart.createTag(...)`, `StructureStart.loadStaticStart(...)`
- `net.minecraft.world.level.levelgen.structure.pools.JigsawPlacement`:
  - `addPieces(context, startPool, startJigsaw, maxDepth, pos, useExpansionHack, projectStartToHeightmap, maxDistance, poolAliasLookup, dimensionPadding, liquidSettings)` $\to$ `Optional<Structure.GenerationStub>`
- `net.minecraft.world.level.levelgen.structure.pools.alias.PoolAliasLookup`:
  - `create(List<PoolAliasBinding>, BlockPos, long)` $\to$ `PoolAliasLookup`
- `net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplate`:
  - `load(HolderGetter<Block>, CompoundTag)`, `save(CompoundTag)`
  - `getJigsaws(BlockPos, Rotation)`, `getBoundingBox(StructurePlaceSettings, BlockPos)`
  - `processBlockInfos(level, position, referencePos, settings, blockInfoList)`
  - `placeInWorld(level, position, referencePos, settings, random, updateMode)`
- `net.minecraft.world.level.levelgen.Beardifier`:
  - `forStructuresInChunk(StructureManager, ChunkPos)` $\to$ `Beardifier`

---

## Open questions

1. **[oracle] 26.3 column biome pre-check (`couldStructureExistInColumn`) volume-vs-point boundary sensitivity**: `JigsawPlacement.addPieces` (when `project_start_to_heightmap` is set) calls `context.couldStructureExistInColumn(centerX, centerZ, minY, maxY)`, which evaluates biomes via `createResolverForChunk` (`sampleVolume`, 1×N×1 quart column), whereas `context.isValidBiome(stub)` evaluates the biome at `(centerX, centerY, centerZ)` via `createResolver` (`sampleValue`, single point). Because `sampleVolume` and `sampleValue` can differ by up to 1 ULP on non-dyadic noise scales (see [`density-functions.md`](./density-functions.md) §2.1), a biome boundary sitting within 1 ULP of a quart could theoretically pass one check and fail the other. **Probe**: Across 10,000 candidate village and abandoned-camp chunks on multiple seeds, compare the set of valid `StructureStart`s against vanilla's `ChunkGenerator.createStructures` output to verify 100% agreement on both column pre-check rejection and final `isValidBiome` acceptance.
2. **[oracle] `CappedProcessor` world reads outside `WorldGenRegion` write radius**: Because `CappedProcessor.evaluatesEntirePieceState()` disables the `chunkBB.isInside(blockPos)` filter during `processBlockInfos`, any delegate processor (such as a `RuleProcessor` with a non-`always_true` `location_predicate`) may call `level.getBlockState(worldPos)` on blocks belonging to chunks adjacent to the center chunk being decorated (and outside `writeRadius = 1` for very large pieces). **Probe**: Generate chunks containing Trial Chambers and Trail Ruins (both of which use `capped` processors spanning multiple chunks), dump `StructureTemplate.processBlockInfos` outputs and final chunk block states across every chunk overlapping a capped piece, and verify that chunk-order-independent block replacement matches vanilla block for block.
3. **[oracle] Iteration order of overlapping structures in `Beardifier` and `applyBiomeDecoration`**: `ChunkAccess.structureReferences` is a `HashMap<Structure, LongSet>` (with `LongOpenHashSet` values). When **two or more** structures with `terrain_adaptation != "none"` (or two instances of the same structure) overlap a single chunk, `Beardifier` sums their `f32` density contributions in map/set iteration order (which can differ by 1 ULP in `f32`), and `applyBiomeDecoration` calls `start.placeInChunk` for multiple starts of the same `Structure` in `LongOpenHashSet` iteration order (sharing the same `WorldgenRandom` stream!). **Probe**: Locate a seed and chunk where two starts of the same jigsaw structure (or two terrain-adapting structures) overlap, dump `Beardifier.forStructuresInChunk` rigid/junction order and final chunk blocks after `TERRAIN` and `FEATURES`, and determine whether Zenith needs to match fastutil `LongOpenHashSet` iteration order for overlapping same-structure starts.
4. **(implementer) 26.3 `BlockState.CODEC` bare string shorthand in `processor_list` JSON**: In 26.3, `worldgen/processor_list` entries (e.g. `minecraft:ancient_city_generic_degradation`) encode `output_state` using a bare block identifier string (e.g. `"minecraft:deepslate_bricks"`) as shorthand for the default block state, in addition to `{ "id": ..., "properties": ... }`. `zenith-structures` accepts both forms.
5. **(implementer) `dimension_origin` placement parity**: None of the 21 built-in 26.3 `structure_set` files uses `minecraft:dimension_origin` (all 21 use `random_spread` or `concentric_rings`). `compute_dimension_origin` and `StructurePlacement::DimensionOrigin` are implemented per §1.6 from `NoiseSpawnFinder`, and would benefit from a custom datapack parity probe when datapack loading parity is added.
6. **(implementer) Beardifier summation order (answers [oracle] item 3 for the terrain step)**: `Beardifier` input starts are gathered in
   ascending source-chunk raster order (`x` outer, `z` inner, over the 17×17 neighbourhood) and, within one start chunk, in
   `structure_starts` order; pieces and junctions keep piece-list order. With this order `golden-terrain` in structures-on mode
   (`ZENITH_GOLDEN_STRUCTURES=1`) matched vanilla bit for bit over 12 seeds × 3 world presets (normal Overworld/Nether/End,
   large_biomes, amplified): every terrain-adapting structure type found (villages, pillager outposts, abandoned camps, ancient
   cities, trail ruins, trial chambers, strongholds, nether fossils) in all four adjustment modes, with chunks where two or more
   starts overlap (including starts of different structure types, which the corpus searches for explicitly). No `f32` sum-order
   difference was observed, so no emulation of `HashMap` / `LongOpenHashSet` iteration order is implemented for the beardifier. The
   `applyBiomeDecoration` half of item 3 (shared decoration RNG across same-structure starts) is not exercised by this suite.
   Note for suite authors: the vanilla server fully generates (features included) the chunks around the world spawn, so
   `terrain`-status comparisons must stay out of roughly `|chunk| <= 32` around the origin.

7. **(implementer) Per-block `mirror`/`rotate` is not specified**: §4.5 applies `state.mirror(m).rotate(r)` but no spec lists the per-block-class rules. `zenith-structures::block_xform` implements them from the block classes' behaviour: any horizontal `facing` rotates / mirrors (up/down unchanged); `orientation` via `FrontAndTop`; `axis` swaps `x`/`z` on quarter turns; the 16-step `rotation` of signs, banners and skulls (`+4/+8/+12`; mirror `LEFT_RIGHT: (8 - r) mod 16`, `FRONT_BACK: (16 - r) mod 16` on the signed step); `north/east/south/west` of fences, panes, bars, walls, redstone wire, tripwire, vines, multiface blocks, moss carpet and huge mushrooms; stairs (axis-matching mirror turns the stair 180° and swaps `inner/outer_left/right`); doors (any mirror cycles `hinge`); rail shapes via the standard tables; chests keep `type` on mirror. **Probe**: a `golden-structures` run over every template with all 12 mirror/rotation combinations, comparing placed block states.
8. **(implementer) `knownShape = false` shape updates are not implemented**: §4.5 step 7 (`updateShapeAtEdge` / `updateFromNeighbourShapes`) runs for template pieces that do not set `knownShape` (igloo, shipwreck, ocean ruin, ruined portal, nether fossil, end city, mansion). `place_template` skips it, so connection properties (fences, panes, walls, stairs shapes) of those pieces stay exactly as stored in the template after mirror/rotation. Jigsaw pieces (`knownShape = true`) are unaffected. **Probe**: compare fence / pane / stair states of shipwrecks and end cities against vanilla.
9. **(implementer) Placement API shape**: `StructureStart::place_in_chunk(&mut self, env, world, random, writable_area: &mut BoundingBox)` takes `&mut self` because vanilla pieces keep state between chunk calls (snapped scattered-feature heights, placed-chest flags, shifted template positions, ocean-ruin Y) and `&mut BoundingBox` because ruined portals and nether fossils encapsulate their box into the shared writable area for the rest of the chunk's decoration. `FeaturePoolElement` placement is delegated to `StructureWorld::place_feature`; block entities and entities are exchanged as NBT (`set_block_entity` merges keys into the entity at the position, `add_entity` records an entity compound with `id`/`Pos`; spawning itself is not performed).

---

## Parity status
`structure-starts` (2026-11-25): **828/828** checks over 567 chunks and 4 seeds (overworld, nether, end): every
structure start (structure, start chunk, reference count, validity, bounds, every piece's type, bounds, orientation
and depth in list order) and every reference, for all 52 structure types of 26.3. `golden-terrain-structures`
(terrain with structures on, beardifier from the real starts per §5.2): **101,830/101,830** over 12 seeds, all four
terrain-adjustment modes. Piece block placement in the features step is not verified yet (`golden-structures`).
