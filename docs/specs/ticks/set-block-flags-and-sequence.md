# Ticks: block-set flags and the exact sequence of a block change (26.3)

- Verified by suites: `server-tick`, `block-updates`
- Sources: local reference reading of the 26.3 client/server code (2026-11-26, reader session): the level's set-block /
  remove / destroy entry points, the loaded-chunk and proto-chunk `setBlockState`, the generation-region write path, the
  read-only "imposter" chunk wrapper, the chunk section counters, the block-entity registration code, and the POI update hook.
  Everything is in our own words.
- Related specs: [`neighbor-and-shape-updates.md`](neighbor-and-shape-updates.md) (steps 7–8 below),
  [`block-hooks.md`](block-hooks.md) (every hook named here), [`scheduled-ticks-and-block-events.md`](scheduled-ticks-and-block-events.md),
  [`server-tick.md`](server-tick.md); [`../lighting/chunk-steps.md`](../lighting/chunk-steps.md) §7 (light enqueue),
  [`../lighting/sky-light-sources.md`](../lighting/sky-light-sources.md); [`../worldgen/features.md`](../worldgen/features.md) §1
  (block writing during generation: this file is its framework side), [`../worldgen/structures-placement-and-jigsaw.md`](../worldgen/structures-placement-and-jigsaw.md) §4–§5;
  [`../blocks/state-facts.md`](../blocks/state-facts.md) (state predicates such as air, light properties, block-entity flag),
  [`../blocks/shapes.md`](../blocks/shapes.md) (collision shapes compared in step 6), [`../blocks/state-transforms.md`](../blocks/state-transforms.md).

## Summary

Every world write goes through `setBlock(pos, state, flags, updateLimit)`. The *flags* decide which side effects follow; the
*sequence* is fixed and mixes chunk-internal bookkeeping (section counters, heightmaps, light, block entities), block
hooks (`affectNeighborsAfterRemoval`, `onPlace`), client notification, neighbour notification, shape updates and POI upkeep.
Three different implementations exist and they differ a lot: the **level** (a loaded chunk, all side effects), the
**generation region** (proto chunks: almost no side effects), and the **imposter** wrapper (already-`full` chunks seen from
generation: writes are discarded). Another chunk implementation, the client's, shares the loaded-chunk code.

## Inputs and state

- `pos` (block position), `state` (the new block state), `flags` (i32 bit set, below), `updateLimit` (i32 recursion budget,
  default **512** = `Block.UPDATE_LIMIT`).
- The chunk (section array, `nonEmptyBlockCount` / `tickingBlockCount` / `tickingFluidCount` per section, four heightmaps),
  its block-entity map and ticker table, the light engine, the POI manager.

### Update flags (bit values; the same bits are used everywhere)

| bit | name | server-side meaning |
|---|---|---|
| 1 | `UPDATE_NEIGHBORS` | after the write: notify neighbours (`neighborChanged`), the analog-signal comparator notification, and (loaded chunks) run `affectNeighborsAfterRemoval` of the removed state |
| 2 | `UPDATE_CLIENTS` | send the change to clients (only if the chunk is block-ticking) |
| 4 | `UPDATE_INVISIBLE` | client: do not re-render; **no server effect** |
| 8 | `UPDATE_IMMEDIATE` | client: re-render on the main thread; **no server effect** |
| 16 | `UPDATE_KNOWN_SHAPE` | skip *all* shape updates of this write (and, in the generation region, skip post-process marking) |
| 32 | `UPDATE_SUPPRESS_DROPS` | read only by `Block.updateOrDestroy` (a neighbour update that turns a block into air then does not drop items); cleared before it is passed on |
| 64 | `UPDATE_MOVE_BY_PISTON` | the removal / placement is a piston move: forwarded as the `movedByPiston` argument of the hooks; also makes `affectNeighborsAfterRemoval` run when bit 1 is clear |
| 128 | `UPDATE_SKIP_SHAPE_UPDATE_ON_WIRE` | shape updates of this write skip redstone-wire blocks (`neighbor-and-shape-updates.md` §3) |
| 256 | `UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS` | skip `preRemoveSideEffects` of the removed block entity (container contents are not dropped) |
| 512 | `UPDATE_SKIP_ON_PLACE` | skip the new state's `onPlace` hook |

Composite constants: `UPDATE_NONE = 260` (4 + 256), `UPDATE_ALL = 3`, `UPDATE_ALL_IMMEDIATE = 11`,
`UPDATE_SKIP_ALL_SIDEEFFECTS = 816` (16 + 32 + 256 + 512). Neighbour-shape updates started by a write use
`flags & ~(1 | 32)` (clear bits 1 and 32, mask `−34`).

## Behavior (ordered rules)

### 1. Entry points

1. `setBlockAndUpdate(pos, state)` = `setBlock(pos, state, 3)`; `setBlock(pos, state, flags)` = `setBlock(pos, state, flags, 512)`.
2. `removeBlock(pos, movedByPiston)`: `fluid = fluidState(pos)`; `setBlock(pos, fluid.createLegacyBlock(), 3 | (movedByPiston ? 64 : 0))`
   (a block over water leaves a water block, not air). The generation region's `removeBlock` instead places plain **air** with flags 3.
3. `destroyBlock(pos, dropResources, breaker, limit)` (level): `state = blockState(pos)`; if it is air → return false; call the block's
   *destroy-particles* hook (client effect); if `dropResources`: look up the block entity (only if the state has one) and run
   `Block.dropResources(state, level, pos, blockEntity, breaker, empty tool)` **before** the removal; `fluid = fluidState(pos)`;
   `ok = setBlock(pos, fluid.createLegacyBlock(), 3, limit)`; if `ok` post the game event `block_destroy` with `(breaker, old state)`;
   return `ok`. (Generation region: no drops, `setBlock(AIR, 3, limit)`.)
4. `Block.updateOrDestroy(old, new, level, pos, flags[, limit])`: if `new == old` nothing; else if `new.isAir()`: on the server run
   `destroyBlock(pos, dropResources = (flags & 32) == 0, no breaker, limit)`; else `setBlock(pos, new, flags & ~32, limit)`. (The client does
   nothing for the air case.)
5. `ServerLevel.updateNeighboursOnBlockSet(pos, oldState)` (commands `/setblock`, `/fill`, `/clone`, which write with bit 1 clear and then
   notify): `state = blockState(pos)`; if the **block type** differs from `oldState`'s: `oldState.affectNeighborsAfterRemoval(level, pos, false)`;
   then `updateNeighborsAt(pos, state.block)` (the **new** block, unlike step 7 below); then if the state has an analog output signal:
   `updateNeighbourForOutputSignal(pos, newBlock)`.

### 2. The level's `setBlock` — ordered steps (server, loaded chunk)

1. **Bounds.** If `pos` is not *valid* (outside the dimension's build height `[minY, maxY]`, or its chunk coordinates exceed the valid chunk
   range) return **false**. If this is a server level in a debug-world mode return **false**.
2. **Chunk write** (§3): `old = chunk.setBlockState(pos, state, flags)`; the chunk of `pos` is fetched with load-or-generate to `full`
   (a blocking call). If `old` is *null* return **false**.
3. `new = blockState(pos)` (re-read). **Steps 4–9 run only if `new == state`** (identity of the interned state); otherwise (a hook of step 3.x
   replaced the block again) they are all skipped and the call still returns **true**.
4. If `old != new`: `setBlocksDirty` (client re-render hint; no server effect).
5. **Client notification**: if bit 2 is set and (client: bit 4 clear) / (server: the chunk's full status is at least *block ticking*):
   `sendBlockUpdated(pos, old, state, flags)` — server: mark the position changed for the chunk's next broadcast, invalidate the path-type cache
   at `pos`, then if `old`'s collision shape and `state`'s collision shape differ (the symmetric difference is non-empty) ask every navigating
   mob (identity set; hook, entity milestone) whether it must re-compute its path for `pos`, and re-compute those.
6. **Neighbour notification**: if bit 1 is set: `updateNeighborsAt(pos, old.block)` (the **old** block is the "source block" passed to
   `neighborChanged`), then if this is a server and `state.hasAnalogOutputSignal()`: `updateNeighbourForOutputSignal(pos, state.block)`.
   Details: [`neighbor-and-shape-updates.md`](neighbor-and-shape-updates.md) §1–§2.
7. **Shape updates**: if bit 16 is clear **and** `updateLimit > 0`: `nf = flags & ~(1 | 32)`; in this order: `old.updateIndirectNeighbourShapes(level, pos, nf, limit − 1)`;
   `state.updateNeighbourShapes(level, pos, nf, limit − 1)`; `state.updateIndirectNeighbourShapes(level, pos, nf, limit − 1)`
   ([`neighbor-and-shape-updates.md`](neighbor-and-shape-updates.md) §3–§4).
8. **POI upkeep** (server): for the point-of-interest type of `old` and of `state` (`none` for most blocks): if the two differ: if `old` had a
   type, run "remove POI at `pos`" on the server's task queue; if `state` has a type, run "add POI at `pos`". Each runs **immediately** when the
   caller is the server thread and not inside a queued task, otherwise it is queued to run later in queue order (generation threads queue).
9. Return **true**.

### 3. `LevelChunk.setBlockState(pos, state, flags)` — the chunk-internal sequence (loaded chunk; server and client)

1. `section` = the section containing `y`; `wasEmpty = section.hasOnlyAir()`. If `wasEmpty` and `state.isAir()` → return *null* (nothing changes, not even
   between two air kinds such as air and cave air).
2. Write the state into the section; the section updates its counters: when the old state was not air: `nonEmpty −= 1`, if it is randomly ticking
   `tickingBlock −= 1`, and if its fluid state is non-empty `fluid −= 1` and (if the fluid is randomly ticking) `tickingFluid −= 1`; then the same
   increments for the new state if it is not air. If the previous state **is** the new state (same object) return *null* — nothing below runs.
3. Update the four heightmaps with the new state, in order `MOTION_BLOCKING`, `MOTION_BLOCKING_NO_LEAVES`, `OCEAN_FLOOR`, `WORLD_SURFACE`
   (rule of `Heightmap.update`: if `y <= firstAvailable − 2` nothing; if the new state is opaque for that map and `y >= firstAvailable` set the height to
   `y + 1`; else if `y == firstAvailable − 1` scan downward for the first opaque block (or the world bottom)).
4. If the section's emptiness changed: tell the light engine (`updateSectionStatus(pos, isEmpty)`) and the chunk source's section-emptiness hook.
5. If the old and new states have different light properties (dampening, emission, or either uses its shape for occlusion): update the chunk's sky-light
   sources at the column, then enqueue a light check at `pos` ([`../lighting/chunk-steps.md`](../lighting/chunk-steps.md) §7).
6. `blockChanged = !old.is(newBlock)`; `movedByPiston = (flags & 64) != 0`; `sideEffects = (flags & 256) == 0`.
7. **Block-entity removal:** if `blockChanged` and `old` has a block entity and `!state.shouldChangedStateKeepBlockEntity(old)`: on the server and if
   `sideEffects`: look up the block entity at `pos` (an immediate lookup: a missing one is created only if the *new* state itself has a block entity, which is rare here) and, if present, call its **`preRemoveSideEffects(pos, old)`** (containers drop their
   contents here); then remove the block entity and its ticker from the chunk (mark it removed).
8. **Removal hook:** if `(blockChanged || newBlock is a rail block)` and this is a server level and `(flags & 1) != 0 || movedByPiston`:
   call **`old.affectNeighborsAfterRemoval(level, pos, movedByPiston)`**. (Rails call it even when the block type did not change, so shape changes of
   a rail notify.)
9. **Guard:** if the block now stored at the position is no longer `newBlock` (the hook of step 8 replaced it) → return *null*.
10. **Placement hook:** if this is a server level and `(flags & 512) == 0`: call **`state.onPlace(level, pos, old, movedByPiston)`**.
11. **Block-entity (re)creation:** if `state` has a block entity and the stored block is still `newBlock` (`onPlace` may have replaced it): look up an existing
    block entity at `pos` without creating one; if one exists and its type is not valid for `state`: log and remove it, treat as absent; if absent: create via
    the block's `newBlockEntity(pos, state)` and register it (§ block-entity registration of [`server-tick.md`](server-tick.md) §8); else set its block state to
    `state` and refresh its ticker.
12. Mark the chunk unsaved; return `old`.

**Client chunks** run the same code but: no `preRemoveSideEffects`, no `affectNeighborsAfterRemoval`, no `onPlace`; block entities are still created.

### 4. Generation phase: proto chunks and the generation region

**4.1 `WorldGenRegion.setBlock(pos, state, flags, limit)`** (used by every feature, structure, carver step after `terrain`):
1. If `pos` is outside the *write zone* (|chunkX − centerX| and |chunkZ − centerZ| must be ≤ the generating step's block-state write radius:
   `0` for `terrain`, `1` for `features`) → log and return **false**. If the centre chunk is a *retrogen* upgrade chunk, also require `pos` to be inside the
   chunk's old build height.
2. `old = chunk.setBlockState(pos, state, flags)` on the region's chunk (a `ProtoChunk` or an imposter). If `old` is non-null, run the **POI hook** of §2 step 8.
3. **Block entity:** if `state` has a block entity: if the chunk is a *level-chunk type* (imposter) create and set the block entity; else store a **`DUMMY`** pending
   NBT `{x, y, z, id: "DUMMY"}` at the position (kept only if no block entity exists there); the real block entity is created on promotion. Else if `old`
   had a block entity: remove it (and its pending NBT).
4. **Post-process marking:** if `(flags & 16) == 0`: `p = state.getPostProcessPos(region, pos)`; if non-null mark `p` in its chunk: **appended to the section's list as a
   packed offset (`x | y<<4 | z<<8`), duplicates allowed, ignored if outside the build height** (an imposter ignores the mark).
5. Return **true** — also when the chunk returned null (empty section + air, identical state, or a discarded imposter write).
6. **No other effect:** no neighbour notification (bit 1 ignored), no shape updates (`updateLimit` ignored), no `onPlace`, no `affectNeighborsAfterRemoval`, no
   client notification, no drops. Region `destroyBlock` = `setBlock(air, 3)`, region `removeBlock` = `setBlockAndUpdate(air)`.
7. **Randomness:** the region exposes its own random source: a positional random of the factory `worldgen_region_random` taken at the generating chunk's
   world position; it is what `updateShape` receives during generation.

**4.2 `ProtoChunk.setBlockState(pos, state, flags)`** (chunk below `full`):
1. If `y` is outside the build height → return **void air** (not null).
2. If the section is empty and `state` is the plain air block → return `state` (a non-null result).
3. Write the section. **If the chunk's status is at least `initialize_light`:** tell the light engine if the section's emptiness changed, and if the old / new states differ in
   light properties update the sky-light sources and enqueue the light check. **Before `initialize_light` no light work happens.**
4. Heightmaps: for every heightmap the chunk's status keeps (`heightmapsAfter` of its persisted status) that already exists: `update`; those that do not exist yet are
   created by scanning the column ("priming") once.
5. Return the old state — **even when it equals the new state** (no early exit). No section counters beyond the section's own, no block-entity handling, no hooks.

**4.3 Imposter chunk** (a `full` chunk viewed during generation): `setBlockState` returns *null* (write discarded), block-entity writes and post-process marks are ignored,
the tick containers are black holes. Reads go to the wrapped chunk. Features that overlap an already-`full` neighbour therefore write nothing there.

**4.4 Promotion:** when a proto chunk becomes a level chunk (`full` task) its blocks, block-entity NBT, post-process lists and ticks are carried over (Behavior §5 of
[`scheduled-ticks-and-block-events.md`](scheduled-ticks-and-block-events.md)); the first time the chunk becomes block-ticking its post-process list runs.

### 5. Which flags the common callers use (reference table; the callers themselves belong to other specs)

| caller | flags | note |
|---|---|---|
| `setBlockAndUpdate`, commands default, most block logic | 3 | neighbours + clients, shape updates on |
| piston moves, falling blocks landing, etc. | per block (readers of those blocks) | |
| `Feature.setBlock` | 3 (in the region identical to 2) | no neighbour updates during generation |
| tree trunks / fallen trees | 19 (3 + 16) | known shape: no post-process marking |
| block piles, fossils, huge fungus | 260 | no re-render, no block-entity side effects; post-process marking still on |
| template placement (barrier pre-clear) | 820 (4 + 16 + 32 + 256 + 512) | |
| jigsaw single-pool pieces | 18 (2 + 16) | |
| post-process fix-up of a changed state | 276 (4 + 16 + 256) | |
| legacy upgrade fixers | 18 | |
| `Block.updateOrDestroy` inside shape updates | caller's flags with bit 32 removed | |
| neighbour-shape updates started by a write | write flags minus bits 1 and 32 | |

## Edge cases and known quirks

1. **Source block of the neighbour notification is the old block** (step 2.6), while `/setblock`-style notification (`updateNeighboursOnBlockSet`) uses the new one.
2. **Steps 2.4–2.9 are skipped when the new state is replaced inside `onPlace` / the removal hook**, yet `setBlock` returns true. The inner write ran its own full sequence.
3. **`chunk.setBlockState` can return null after side effects** (step 3.9): heightmaps, light, block-entity removal and `affectNeighborsAfterRemoval` already happened.
4. **Region writes to a `full` neighbour are silently dropped** but `WorldGenRegion.setBlock` still returns true and still runs the POI hook only when the chunk returned a non-null old state (so
   not for dropped writes).
5. **`removeBlock` leaves water** for a waterlogged position in the level but not in the generation region.
6. **POI changes are deferred when written off the server thread** (generation) or from inside a queued server task, and immediate otherwise — the order of POI adds / removes within a tick is
   therefore queue order, not call order, in those cases. Villager job-site queries in the same tick can see stale POI data.
7. **Flag 16 in generation only suppresses post-process marking**; flag 1 never has an effect there.
8. **`ProtoChunk.setBlockState` returns the old state even for an identical write**; callers that treat "non-null" as "changed" (the region's POI hook) will fire the POI hook for no-op writes.
9. **Block-entity ticker moves to the end** when a block entity is replaced (step 3.7 + 3.11); a state change that keeps the block entity keeps its ticker position.
10. **`hasOnlyAir` (section) vs `state.isAir()`:** the early-out of step 3.1 uses the air-ness of the *block class* (air, cave air, void air), the proto chunk's early-out uses the single air block.

## Constants and IDs

Flag bits 1, 2, 4, 8, 16, 32, 64, 128, 256, 512 and composites 260, 3, 11, 816 (above); default update limit 512; heightmap order `MOTION_BLOCKING`,
`MOTION_BLOCKING_NO_LEAVES`, `OCEAN_FLOOR`, `WORLD_SURFACE`; post-process packing `x | y<<4 | z<<8`; DUMMY block-entity tag `{x, y, z, id: "DUMMY"}`;
generation write radius: `terrain` 0, `features` 1; region random factory name `worldgen_region_random`.

## Oracle probe points

- Drive a change: `Level#setBlock(BlockPos, BlockState, int flags, int updateLimit)`, `Level#setBlock(BlockPos, BlockState, int)`, `Level#removeBlock(BlockPos, boolean)`,
  `Level#destroyBlock(BlockPos, boolean, Entity, int)`, `ServerLevel#updateNeighboursOnBlockSet(BlockPos, BlockState)`, `Block.updateOrDestroy(...)`.
- Observe the sequence: instrument (agent) the public wrappers on `BlockBehaviour$BlockStateBase`: `onPlace(Level, BlockPos, BlockState, boolean)`,
  `affectNeighborsAfterRemoval(ServerLevel, BlockPos, boolean)`, `handleNeighborChanged(Level, BlockPos, Block, Orientation, boolean)`,
  `updateShape(LevelReader, ScheduledTickAccess, BlockPos, Direction, BlockPos, BlockState, RandomSource)`, `updateNeighbourShapes(...)`, `updateIndirectNeighbourShapes(...)`,
  plus `BlockEntity#preRemoveSideEffects(BlockPos, BlockState)`, `LevelChunkSection#setBlockState`, `LevelLightEngine#checkBlock(BlockPos)`, `PoiManager#add` / `#remove`;
  log `(call index, method, pos, args)`; compare per scenario.
- Generation: `WorldGenRegion#setBlock(BlockPos, BlockState, int, int)` (construct a region around a `ProtoChunk`) and `ChunkAccess#getPostProcessing()`.
- Heightmaps and counters: `ChunkAccess#getHeight(Heightmap.Types, int, int)`, `LevelChunkSection#isRandomlyTickingBlocks()` / `#isRandomlyTickingFluids()` / `#hasOnlyAir()`.
- Flag matrix: for each of the flag combinations {0, 1, 2, 3, 16, 19, 32, 64, 128, 256, 512, 816} place a fixed set of blocks (stone, redstone wire, sand over air, door, chest with items, piston, water) over
  fixed old states; dump the world state and the hook log.

## Open questions

1. **[oracle] Generation write radius for the `terrain` step** (0) and `features` step (1) are taken from the step table; confirm no other step writes blocks (carvers, `surface`). Probe: instrument
   `WorldGenRegion#setBlock` per chunk status for a fresh chunk and list the statuses that call it.
2. **[oracle] `ProtoChunk` POI hook on no-op writes** (Edge 8): confirm that POI add / remove only fires when the POI type differs (it does by the type test) — the open point is whether the test sees a *different* old state for a repeated write; probe with a double write of a bed.
3. **[oracle] Deferred POI ordering inside a command** (`/fill` over beds): the order of POI adds relative to `neighborChanged` effects. Probe: log `PoiManager#add` order for a 10×1×1 `/fill` of beds.
4. **[oracle] Navigation recompute** (step 2.5) order and trigger: depends on the identity-hash set of navigating mobs (nondeterministic); parity runs without mobs.
5. (implementer) `sendBlockUpdated`'s client part is not needed for headless parity; keep the collision-shape comparison only if a navigation model is implemented.
6. (implementer) The client-side `setBlock` (prediction) shares steps 3.1–3.7 and 3.11; Zenith's client engine reuses the same code with the `isClientSide` switches of §3.

## Parity status

Not yet run.
