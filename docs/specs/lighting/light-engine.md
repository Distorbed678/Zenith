# Lighting: the propagation engines (block light and sky light), 26.3

- Verified by suites: `light-engine` (light values per position, per-section layers, queue-order traces)
- Sources: local reference reading of the 26.3 decompile (2026-11-25), written in our own words. Block and shape facts come
  from the existing block specs; hash-order facts from a JDK + fastutil fixture.
- Related specs: [`data-layers.md`](data-layers.md) (storage, sections, visible / updating views, hash-set order),
  [`sky-light-sources.md`](sky-light-sources.md) (the per-column source line the sky engine reads),
  [`chunk-steps.md`](chunk-steps.md) (who calls the engine, in which order), `../blocks/state-facts.md` §2.5 / §2.6 / §3.2 /
  §3.3 (propagates-skylight-down, use-shape-for-light-occlusion, light dampening, light emission),
  `../blocks/shapes.md` §4.4 and §5.1 rule 7 (`faceShapeOccludes`, `mergedFaceOccludes`, face occlusion shapes).

## Summary

Two independent engines share one algorithm skeleton: a **decrease queue** and an **increase queue** of
`(block node, entry)` pairs processed FIFO, plus a hash set of block nodes whose neighbourhood must be re-examined
("check set"). A run is: examine every checked block (this seeds both queues), drain the decrease queue completely, drain
the increase queue completely, then do the storage end-of-run pass (`data-layers.md` §7).

- **Block light**: values 0..15 per block; emitters set their own level; light spreads to the six neighbours losing
  `max(1, dampening)` per step; removal is a decrease wave that re-enqueues stronger neighbours.
- **Sky light**: the same machinery plus **sources**: every block at or above its column's "lowest source Y" has value 15
  without losing anything going down; sources are set and cleared in whole column segments; vertical gaps made of sections
  that have no layer are bridged by special rules.

Per-direction blocking uses two things: the opacity number (`max(1, dampening)` of the **target** block) and, when a
block's light-occlusion flag is set, the **face shapes** of the two blocks on either side of the step.

## Inputs and state

### A. Block facts the engine reads (all from the existing block specs)

For a block state `s` (exact per state, equality of states is identity of the interned state):
`dampening(s)` = light block (state-facts §3.2); `emission(s)` = light emission (§3.3); `canOcclude(s)` (§2.3);
`useShapeForLightOcclusion(s)` (§2.6); `faceOcclusion(s, d)` = the face occlusion shape of `s` toward direction `d`
(shapes.md §5.1 rule 7); `occlusionShape(s)` (shapes.md §5.1 rule 6).

Derived helpers:
- `emptyShape(s) = !canOcclude(s) || !useShapeForLightOcclusion(s)`. (So almost every block is "empty-shaped" for light:
  stairs, slabs-not-double, farmland, snow layers, pistons extended etc. are the ones with real shapes.)
- `lightShape(s, d) = emptyShape(s) ? EMPTY : faceOcclusion(s, d)`.
- `opacity(s) = max(1, dampening(s))`.
- `shapeOccludes(from, to, d) = faceShapeOccludes(lightShape(from, d), lightShape(to, opposite(d)))` (shapes.md §4.4).
- `hasDifferentLightProperties(old, new)`: false if `new` **is** `old` (same interned state); otherwise true iff
  `dampening` differs, or `emission` differs, or `useShapeForLightOcclusion(new)`, or `useShapeForLightOcclusion(old)`.
  This is the only gate for "a block change affects light" (`chunk-steps.md` §7).
- `lightDampeningInto(from, to, d, simple)` (public helper also used by grass / nylium spreading rules): if both states are
  `emptyShape` return `simple`; else `fromShape = emptyShape(from) ? EMPTY : occlusionShape(from)` (the whole shape, **not**
  a face), `toShape` likewise; return **16** if `mergedFaceOccludes(fromShape, toShape, d)` else `simple`.
- State lookup: `getState(pos)` reads the chunk at the position's chunk coordinates through the chunk source's
  **chunk-for-lighting** accessor; a missing chunk reads as the **bedrock** default state (dampening 15, so nothing
  propagates into it). Out-of-height positions read as air.

### B. Engine state (`LightEngine`)

- `blockNodesToCheck`: fastutil `LongOpenHashSet(512, 0.5)` of block nodes (set semantics: a block checked twice is
  examined once). Iteration order: `data-layers.md` "Hash-set iteration order used by the engine".
- `decreaseQueue`, `increaseQueue`: two FIFO queues of 64-bit values; every logical entry is **two consecutive longs**
  `(blockNode, entryData)`, enqueued and dequeued as a pair.
- The storage object of `data-layers.md`; a 2-entry chunk cache (semantically transparent; cleared at the end of each run).
- The engine reads the **updating** layers (`getStoredLevel` / `setStoredLevel`), never the visible ones.

### C. Queue entry encoding (`QueueEntry`; 64-bit, only the low 12 bits are used)

| Bits | Meaning |
| --- | --- |
| 0..3 | level (0..15): the source level the entry describes |
| 4..9 | "propagate in direction" flags, bit `4 + ordinal(d)`: DOWN 16, UP 32, NORTH 64, SOUTH 128, WEST 256, EAST 512 (all six = 1008) |
| 10 (1024) | `fromEmptyShape`: the source block's light shape is empty, so the source state need not be loaded |
| 11 (2048) | `increaseFromEmission`: the entry may **raise** the stored level of its own block to its level before propagating |

Constructors (`withLevel` replaces the low four bits with `level & 15`):

| Name | Result |
| --- | --- |
| `decreaseAllDirections(l)` | `1008 | l` |
| `decreaseSkipOneDirection(l, d)` | `(1008 − bit(d)) | l` |
| `increaseLightFromEmission(l, fromEmpty)` | `1008 | 2048 | (fromEmpty ? 1024 : 0) | l` |
| `increaseSkipOneDirection(l, fromEmpty, d)` | `(1008 − bit(d)) | (fromEmpty ? 1024 : 0) | l` |
| `increaseOnlyOneDirection(l, fromEmpty, d)` | `bit(d) | (fromEmpty ? 1024 : 0) | l` |
| `increaseSkySourceInDirections(down, north, south, west, east)` | `15` plus the bit of each `true` argument (never UP, never 1024 / 2048) |

Named constants: `PULL_LIGHT_IN_ENTRY = decreaseAllDirections(1) = 1009`; sky: `REMOVE_TOP_SKY_SOURCE_ENTRY =
decreaseAllDirections(15) = 1023`; `REMOVE_SKY_SOURCE_ENTRY = decreaseSkipOneDirection(15, UP) = 991`;
`ADD_SKY_SOURCE_ENTRY = increaseSkipOneDirection(15, false, UP) = 991`.

Examples: a torch (emission 14, empty-shaped) checked by the block engine enqueues decrease `(node, 1009)` then increase
`(node, 4094)` (= 1008 + 2048 + 1024 + 14); a glowstone block (15, still empty-shaped for light) → `4095`.

## Behavior (ordered rules)

### 1. Entry points

- `checkBlock(pos)` → add `pos.asLong()` to `blockNodesToCheck`. (`LevelLightEngine.checkBlock` forwards to the block
  engine **then** the sky engine.)
- `updateSectionStatus`, `setLightEnabled`, `queueSectionData`, `retainData` forward to the storage (`data-layers.md` §4, §5);
  the sky engine's `setLightEnabled(enable = true)` additionally fills layers (rule 20).
- `propagateLightSources(chunk)` seeds a freshly generated chunk (block rule 9, sky rule 21); the level engine calls the
  block engine first, then the sky engine.
- `hasLightWork()` = storage has inconsistencies **or** `blockNodesToCheck` non-empty **or** either queue non-empty
  (level engine: sky or block).

### 2. `runLightUpdates` (one engine)

1. For every node in `blockNodesToCheck` in the set's iteration order: `checkNode(node)` (block rule 4 / sky rule 13).
2. Clear the set and trim its table to 1024 slots.
3. `propagateDecreases`: while the decrease queue is non-empty: dequeue `(fromNode, decreaseData)`, `propagateDecrease` it
   (this may enqueue more decreases and increases). Count each dequeued pair.
4. `propagateIncreases`: while the increase queue is non-empty: dequeue `(fromNode, increaseData)`; `fromLevel =
   getStoredLevel(fromNode)`; `target = level(increaseData)`; if the entry has `increaseFromEmission` and `fromLevel <
   target`: store `target` at `fromNode` and use `fromLevel = target`; then **only if `fromLevel == target`** call
   `propagateIncrease(fromNode, increaseData, fromLevel)`. Count each dequeued pair. (An entry whose stored level has
   since been raised above, or lowered below, its target is dropped silently.)
5. Clear the chunk cache; run the storage end-of-run pass (`markNewInconsistencies`, then `swapSectionMap`).
6. Return `decreaseCount + increaseCount`. The level engine runs the **block engine first, then the sky engine** and
   returns the sum.

There is no limit on queue entries processed per run; the number of tasks batched into one run is the chunk-source
concern of `chunk-steps.md` §3 (server) and §9 (client).

### 3. Opacity and the per-step test (shared by both engines)

A step from block `A` (level `a`) to the neighbour `B` in direction `d`, in `propagateIncrease`:
1. `B`'s section must store light (otherwise skip). `toLevel = stored(B)`.
2. Quick reject: if `a − 1 <= toLevel` skip.
3. `bState = getState(B)`; `newTo = a − opacity(bState)`; if `newTo <= toLevel` skip.
4. `aState` = the air default state when the entry's `fromEmptyShape` flag is set, otherwise `getState(A)` (computed once
   per `propagateIncrease` call, on first need; equivalent to computing it every time).
5. If `shapeOccludes(aState, bState, d)` skip. (With the flag set the from-shape is empty, so only the `to` face matters.)
6. Store `newTo` at `B`; if `newTo > 1` enqueue increase `(B, increaseSkipOneDirection(newTo, emptyShape(bState),
   opposite(d)))`. (Level-1 results are stored but never propagate further.)
(The sky engine additionally calls its empty-section hook after step 6, rule 18.)

Directions are always visited in the order DOWN, UP, NORTH, SOUTH, WEST, EAST, and only those whose flag is set in the entry.

### Block engine

4. **`checkNode(blockNode)`** (only if the block's section stores light): `state = getState(node)`;
   `emissionHere = emission(state)` if `emission(state) > 0` **and the column is light-enabled**, else 0 (`getEmission`);
   `old = stored(node)`.
   - If `emissionHere < old`: store 0 at `node` and enqueue decrease `(node, decreaseAllDirections(old))`.
   - Else: enqueue decrease `(node, PULL_LIGHT_IN_ENTRY)` (the node pulls light in from its neighbours).
   - If `emissionHere > 0`: enqueue increase `(node, increaseLightFromEmission(emissionHere, emptyShape(state)))`.
   (The decrease is enqueued **before** the increase, but all decreases drain before any increase runs.)
5. **`propagateIncrease`**: rule 3 with no extra hooks.
6. **`propagateDecrease(fromNode, data)`**: `old = level(data)`. For each flagged direction `d` in order: `to = offset(from, d)`;
   if `to`'s section does not store light skip; `toLevel = stored(to)`; if `toLevel == 0` skip. Then:
   - If `toLevel <= old − 1` (it was lit through `from`): `toState = getState(to)`; `toEmission = getEmission(to, toState)`;
     store 0 at `to`; if `toEmission < toLevel` enqueue decrease `(to, decreaseSkipOneDirection(toLevel, opposite(d)))`;
     if `toEmission > 0` enqueue increase `(to, increaseLightFromEmission(toEmission, emptyShape(toState)))`.
   - Else (`toLevel` is too bright to be explained by `from`): enqueue increase `(to, increaseOnlyOneDirection(toLevel,
     false, opposite(d)))` — it pushes its light back toward `from` only.
7. **Light-enabled gate:** `getEmission` returns 0 for a column that is not light-enabled, so blocks in a disabled column
   can have their light removed by decreases but are not re-seeded.
8. Level-1 `PULL_LIGHT_IN_ENTRY` entries have `old − 1 = 0`, so every non-zero neighbour takes the second branch (pushes back).
9. **`propagateLightSources(chunkPos)`** (block): `setLightEnabled(chunk, true)` (storage level, no fill). Then, if the
   chunk-for-lighting exists, scan it for blocks with `emission != 0` and, **in scan order**, enqueue increase
   `(node, increaseLightFromEmission(emission(state), emptyShape(state)))` for each. Scan order: sections from
   `minSectionY` up to `maxSectionY`; sections whose block palette cannot contain such a state are skipped; inside a section
   `y` 0..15 outer, `z` 0..15 middle, `x` 0..15 inner (node = section origin + (x,y,z)). No decrease entries are produced.

### Sky engine

10. **Constants / predicates:** a sky "source level" is exactly 15 (`isSourceLevel(v) = v == 15`).
    `lowestSourceY(x, z, default)` = the chunk's sky sources value (`sky-light-sources.md`) of the column, or `default` when
    the chunk for lighting is missing. The special value `i32::MIN` means "sources reach below the world".
11. **`getChunkSources(cx, cz)`**: sky sources of the chunk-for-lighting, or null. Where the algorithm below says "or empty
    sources" it uses a sources object whose every column is "sources reach below the world" (`i32::MIN`).
12. **`updateSourcesInColumn(x, z, lowest)`**: `worldBottomY = sectionToBlock(storage.getBottomSectionY())` (the lowest
    section ever stored); run `removeSourcesBelow(x, z, lowest, worldBottomY)` then `addSourcesAbove(x, z, lowest,
    worldBottomY)`.
13. **`checkNode(blockNode)`:** `(x, y, z)`; `lowest = lightOnInSection ? lowestSourceY(x, z, i32::MAX) : i32::MAX`.
    If `lowest != i32::MAX`: `updateSourcesInColumn(x, z, lowest)`. Then, if the node's section stores light:
    - if `y >= lowest` (it is a source): enqueue decrease `(node, REMOVE_SKY_SOURCE_ENTRY)` **then** increase
      `(node, ADD_SKY_SOURCE_ENTRY)`;
    - else: `old = stored(node)`; if `old > 0` store 0 and enqueue decrease `(node, decreaseAllDirections(old))`; else
      enqueue decrease `(node, PULL_LIGHT_IN_ENTRY)`.
    (So a checked block in a light-disabled column is treated as a non-source block.)
14. **`removeSourcesBelow(x, z, lowest, worldBottomY)`**: only if `lowest > worldBottomY`. `startY = lowest − 1`. For
    `sectionY` from `sectionOf(startY)` downward while `sectionY >= currentLowestY`: if that section stores light: for `y` from
    `min(sectionTopY, startY)` down to `sectionBottomY`: `node = (x, y, z)`; if `stored(node) != 15` **return from the whole
    function**; store 0; enqueue decrease `(node, y == lowest − 1 ? REMOVE_TOP_SKY_SOURCE_ENTRY : REMOVE_SKY_SOURCE_ENTRY)`.
    Sections that do not store light are skipped (the walk continues downward).
15. **`addSourcesAbove(x, z, lowest, worldBottomY)`**: `neighborLowest = max` over the four horizontal neighbours' columns of
    `lowestSourceY(·, ·, i32::MIN)` (order: x−1, x+1, z−1, z+1; a missing chunk gives `i32::MIN`). `startY =
    max(lowest, worldBottomY)`. For section nodes from the section of `(x, startY, z)` upward **while not `isAboveData`**:
    if the section stores light: for `y` from `max(sectionBottomY, startY)` up to `sectionTopY`: `node = (x, y, z)`;
    if `stored(node) == 15` **return from the whole function**; store 15; if `y < neighborLowest` **or** `y == lowest`
    enqueue increase `(node, ADD_SKY_SOURCE_ENTRY)`.
16. **`propagateIncrease(from, data, fromLevel)`:** `emptyBelow = countEmptySectionsBelowIfAtBorder(from)`; then rule 3 per
    direction; after step 6 (and only then) call `propagateFromEmptySections(to, d, newTo, increase = true, emptyBelow)`.
    (The hook runs even when `newTo == 1`.)
17. **`propagateDecrease(from, data)`:** `emptyBelow = countEmptySectionsBelowIfAtBorder(from)`; `old = level(data)`. For each
    flagged `d`: `to`'s section must store light; `toLevel = stored(to)`; skip when 0. If `toLevel <= old − 1`: store 0 at `to`;
    enqueue decrease `(to, decreaseSkipOneDirection(toLevel, opposite(d)))`; call `propagateFromEmptySections(to, d, toLevel,
    increase = false, emptyBelow)`. Else enqueue increase `(to, increaseOnlyOneDirection(toLevel, false, opposite(d)))`.
    (No emission handling: sky light has none.)
18. **`countEmptySectionsBelowIfAtBorder(node)`**: 0 unless the node's local `y == 0` **and** local `x ∈ {0, 15}` or local
    `z ∈ {0, 15}`. Otherwise count `k = 0`; while the section `(sx, sy − k − 1, sz)` does **not** store light **and**
    `hasLightDataAtOrBelow(sy − k − 1)`: `k += 1`. Return `k`.
    **`propagateFromEmptySections(to, d, level, increase, emptyBelow)`**: nothing if `emptyBelow == 0`. `crossedSectionEdge(d,
    localX, localZ of to)` must hold: NORTH → `localZ == 15`; SOUTH → `localZ == 0`; WEST → `localX == 15`; EAST → `localX == 0`;
    DOWN / UP never. Then for `sectionY` from `sectionOf(to.y) − 1` down to `sectionOf(to.y) − emptyBelow` (that many
    sections): if the section `(toSectionX, sectionY, toSectionZ)` stores light, for local `y` 15 down to 0 (`node = (to.x,
    sectionMinY + localY, to.z)`): increase: store `level` at `node` and, if `level > 1`, enqueue increase `(node,
    increaseSkipOneDirection(level, true, opposite(d)))`; decrease: store 0 and enqueue decrease `(node,
    decreaseSkipOneDirection(level, opposite(d)))`.
19. **Why the hook exists:** light moving sideways out of the bottom layer of a section whose column has no layers beneath it
    would otherwise never reach the layers of the neighbouring column below; the hook copies the value straight down the
    neighbour column's stored sections.
20. **`setLightEnabled(chunkPos, enable)` (sky):** storage `setLightEnabled`. If `enable`: `sources = chunk sources or empty`;
    `highestNonSource = sources.getHighestLowestSourceY() − 1` (**wrapping i32 arithmetic**: `i32::MIN − 1 = i32::MAX`);
    `lowestFullySourceSection = sectionOf(highestNonSource) + 1` (arithmetic shift, `i32::MAX >> 4 + 1 = 134217728`);
    `top = storage.getTopSectionY(zero)`; `bottom = max(storage.getBottomSectionY(), lowestFullySourceSection)`. For `sectionY`
    from `top − 1` down to `bottom`: `layer = getDataLayerToWrite(section)`; if it exists **and `isEmpty()`**, `fill(15)`.
    (A chunk whose columns all reach below the world makes `bottom` huge: nothing is filled.)
21. **`propagateLightSources(chunkPos)` (sky):** `storage.setLightEnabled(zero, true)` (the storage call: **no** fill).
    `S` = this chunk's sources or empty; `N, South, W, E` = the neighbour chunks' sources or empty. `top = getTopSectionY(zero)`,
    `bottom = getBottomSectionY()`. For `sectionY` from `top − 1` down to `bottom`: `layer = getDataLayerToWrite(section)`;
    if `layer == null` continue; else `minY = sectionY·16`, `maxY = minY + 15`, `sourcesBelow = false`; for `z` 0..15 (outer),
    `x` 0..15 (inner): `low = S.lowest(x, z)`; if `low <= maxY`: the four neighbour values `north = z == 0 ? N.lowest(x, 15) :
    S.lowest(x, z − 1)`, `south = z == 15 ? South.lowest(x, 0) : S.lowest(x, z + 1)`, `west = x == 0 ? W.lowest(15, z) :
    S.lowest(x − 1, z)`, `east = x == 15 ? E.lowest(0, z) : S.lowest(x + 1, z)`; `nb = max(north, south, west, east)`; for
    `y` from `maxY` down to `max(minY, low)`: `layer.set(x, y & 15, z, 15)` (direct write, **no** affected-section marking);
    if `y == low` **or** `y < nb`: enqueue increase `(blockNode, increaseSkySourceInDirections(y == low, y < north, y < south,
    y < west, y < east))`; afterwards if `low < minY` set `sourcesBelow = true`. After the column loops: `if !sourcesBelow`
    **stop** the section loop (break). The enqueue order is therefore: sections top→bottom; inside, z outer, x inner,
    y top→bottom; the `break` is evaluated only for sections that have a layer.

### 22. Level-engine helpers

- `getRawBrightness(pos, skyDampen)`: `sky = skyEngine == null ? 0 : skyEngine.value(pos) − skyDampen` (note: the dampen is
  subtracted only when the engine exists, and the result may be negative); `block = blockEngine.value(pos)`; return
  `max(block, sky)`. Both values are the **visible** values (`data-layers.md` §8).
- `lightOnInColumn(zero)` = no block engine, or (block engine enabled for that column **and** (no sky engine or sky engine
  enabled for it)).
- `getLayerListener(layer)` returns the engine, or the dummy listener (value 0, no data, ignores everything) when the engine
  does not exist (no sky engine in dimensions without sky light).

## Edge cases and known quirks

1. **Order matters at three places:** the hash-set order of `blockNodesToCheck` (Behavior rule 2, step 1), the fixed direction order
   (Behavior rule 3), and the enqueue order in `propagateLightSources` (rules 9, 21). Final values converge for most inputs, but queue
   traces, run counts and the creation / materialisation of layers depend on them, so reproduce all three.
2. An emitter reached by a block-light decrease wave is first set to 0; if its emission is `>=` its old level it is re-seeded
   only through an increase entry (no decrease entry is enqueued for it), otherwise it also continues the decrease wave.
3. `increaseFromEmission` raises a stored level **only** when it is below the entry's level; an emitter whose stored level is
   already higher keeps it (and the entry is then dropped because `fromLevel != target`).
4. Entries are validated at dequeue time (`stored == target`), so stale increase entries are harmless; stale decrease entries
   are not validated (they only remove light that is `<= level − 1`).
5. Light never crosses a position whose section has no layer (`storingLightForSection`), even if the chunk exists.
6. The empty-section hook (rule 18) uses **`countEmptySectionsBelowIfAtBorder` of the source block**, evaluated once per
   `propagateIncrease` / `propagateDecrease` call.
7. Block light inside a disabled column keeps its stored values but is not re-seeded on checks (rule 7); the block's own check
   still enqueues the pull-in decrease.
8. Chunk absence reads as bedrock for the block / sky step test, but **sky sources** read absence as "reaches below the
   world" (`i32::MIN`) for neighbours and as "unknown" (`i32::MAX`, column left untouched) for the checked column itself.
9. `lowestFullySourceSection` uses wrapping arithmetic (rule 20); do not widen to 64 bits.

## Constants and IDs

`MAX_LEVEL = 15`; `MIN_OPACITY = 1`; queue-entry masks 15 / 1008 / 1024 / 2048 (Inputs C); `PULL_LIGHT_IN_ENTRY = 1009`; sky entries
1023 / 991 / 991; initial check-set capacity 512, load factor 0.5, trim 512; direction order DOWN, UP, NORTH, SOUTH, WEST, EAST.

## Oracle probe points

- `net.minecraft.world.level.lighting.LightEngine` protected `enqueueDecrease(long,long)`, `enqueueIncrease(long,long)` (trace
  every logical entry with its order index), `checkNode(long)`, `propagateIncrease(long,long,int)`,
  `propagateDecrease(long,long)`; `runLightUpdates()` return value (processed-entry count); `hasLightWork()`.
- `LightEngine.QueueEntry.*` static constructors (pure functions; dump a table for all arguments).
- `LightEngine.hasDifferentLightProperties(BlockState, BlockState)`, `getLightDampeningInto(...)`, `getOcclusionShape(...)`
  (pure; dump over all state pairs of interest).
- `BlockLightEngine.propagateLightSources(ChunkPos)` / `SkyLightEngine.propagateLightSources(ChunkPos)` on a scratch engine
  with a hand-built `LightChunkGetter` to dump the exact seeded queue.
- `LevelLightEngine#getRawBrightness(BlockPos, int)`.
- Driving a single block change on a live server: `ServerLevel#setBlock(BlockPos, BlockState, int)`, then wait for
  `ThreadedLevelLightEngine#waitForPendingTasks(int,int)` / until `hasLightWork()` is false; read via the listener methods in
  `data-layers.md`.

## Open questions

1. **[oracle] Order-dependence of final values.** Probe: build the same set of changes (e.g. 50 random emitter placements /
   removals) and run them as (a) one batch of checks, (b) one check per run; compare per-position light and the number of
   processed entries. Needed to know whether a Zenith implementation may reorder checks within a batch without a visible
   difference; until answered, reproduce the exact order.
2. **[oracle] `getChunkForLighting` content during a run.** The server returns the chunk once its **features** step has
   completed (`chunk-steps.md` §2); confirm with a trace of `getChunk(int,int)` results (null vs chunk) while neighbour
   chunks are at `terrain` / `features`.
3. **[oracle] Asymmetric shape cases.** Probe all ordered pairs of light-shaped states (stairs, double / bottom / top slabs,
   snow layers 1..8, farmland, dirt path, pistons, end portal frame, enchanting table, daylight detector, lectern,
   stonecutter, sculk sensors, shelves) × the six directions with `shapeOccludes` and the end-to-end light after
   placing an emitter next to each; this validates the shape reading of `shapes.md` §4.4.
4. **[oracle] Count semantics.** Confirm that the run count includes decreases enqueued by `propagateFromEmptySections`
   and that stale increase entries are counted (probe: compare `runLightUpdates()` return values against the number of
   `enqueue*` pairs dequeued).

## Parity status

Not yet run.
