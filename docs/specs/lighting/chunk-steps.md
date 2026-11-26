# Lighting: chunk steps, task batching, load / save / unload, live block changes, client (26.3)

- Verified by suites: `light-engine` (sections `steps`, `live-updates`, `save-load`, `client`), and the chunk-step cases of the
  terrain suites (`parity/terrain-chunks.md`) for the `initialize_light` and `light` statuses
- Sources: local reference reading of the 26.3 decompile (2026-11-25), written in our own words.
- Related specs: [`light-engine.md`](light-engine.md), [`data-layers.md`](data-layers.md), [`sky-light-sources.md`](sky-light-sources.md),
  `../worldgen/surface-and-carvers.md` §4 (status table), `../worldgen/density-functions.md` §7 (heightmaps),
  `../storage/region-and-level.md` (chunk NBT envelope, unload and save flow), `../blocks/state-facts.md`.

## Summary

Lighting enters the chunk pipeline at two consecutive statuses, **`initialize_light`** (register the chunk's non-empty sections
with the engines and build the sky-source table) and **`light`** (seed the chunk's light sources and let the engines propagate),
after `features` and before `spawn`. Every engine call from chunk code is wrapped in a **task** and executed in batches by a
"threaded" level light engine on its own executor: tasks are **PRE_UPDATE** (change engine state) or **POST_UPDATE**
(completion actions); one batch runs the PRE tasks, then one engine `runLightUpdates` (block, then sky), then the POST tasks.
After `full`, every block change that alters light properties enqueues a check; changed sections are reported to the chunk holder
and broadcast to clients. The client runs the same engines synchronously from queued packet data and from locally predicted block
changes.

## Inputs and state

- Status order (26.3): `empty`, `structure_starts`, `structure_references`, `biomes`, `terrain`, `features`, **`initialize_light`**,
  **`light`**, `spawn`, `full`. Light-relevant dependency rows: `initialize_light` — only its parent (`features`) at radius 0;
  `light` — `initialize_light` at radius **1** (all 8 neighbours must have completed `initialize_light`) plus the parent
  at radius 0. Neither step writes block states (`blockStateWriteRadius` unset). `spawn` additionally needs `biomes` radius 1 (not
  light related).
- Both the generation pyramid and the loading pyramid contain both light steps with the same tasks.
- `ChunkStep.apply`: if the chunk's **persisted status is before** the step's target, the task runs and afterwards the persisted
  status is set to the target (if still before it); otherwise (a chunk loaded at a later status) the task still runs, **without**
  changing the status. This is how a chunk loaded from disk gets registered with the (empty) engine.
- `isLighted(chunk)` = `persistedStatus >= light` **and** `chunk.isLightCorrect()`.
- Per chunk flag `lightCorrect` (NBT key `isLightOn`): initial false; set false when the `light` task starts; set true by the
  `light` task's POST completion; copied from the proto chunk into the level chunk at `full`; written to / read from NBT.
- Server level light engine = block engine (always) + sky engine (only if the dimension type has sky light), `Threaded…` wrapper
  with: `lightTasks` ordered list of `(type, action)`, batch size **1000**, a "scheduled" flag, and a task dispatcher
  (see §3).

## Behavior (ordered rules)

### 1. The chunk-for-lighting accessor (server vs client)

The engines read neighbours' block states and sky-source tables through `getChunkForLighting(cx, cz)`:
- **Server:** the chunk of the visible holder at that position **whose `features` future has completed** (i.e. any chunk that
  finished `features`, in whatever later status or class: proto chunk, imposter, level chunk); null if the holder does not exist
  or `features` is not finished. (The accessor asks for `initialize_light`'s *parent* status.)
- **Client:** the loaded level chunk at that position, or null.

### 2. The `initialize_light` task

For the chunk `C` (server):
1. `C.initializeLightSources()` → `fillFrom` (`sky-light-sources.md` §1). Runs immediately on the pipeline thread, before any
   engine task.
2. Attach the level light engine to `C` (proto chunks use it to report block changes from now on, §7).
3. `lighted = isLighted(C)`.
4. Submit PRE task `initializeLight: pos`: for each section index 0..sectionsCount−1 **in ascending order**, if the section is
   not "only air": `levelEngine.updateSectionStatus(sectionPos, false)` (both engines; `data-layers.md` §4).
5. The step's future completes only after a POST task has run: `setLightEnabled(pos, lighted)` on both engines (block: toggle;
   sky: toggle + fill rule `light-engine.md` rule 20) and `retainData(pos, false)`; then the chunk is returned.

Note the sky fill in step 5 runs **after** the batch's engine run, so for a lighted (loaded) chunk the queued NBT layers have
already been applied (`data-layers.md` §7.5) and only layers that are still empty are filled.

### 3. Task batching: `ThreadedLevelLightEngine`

**Task kinds and priority.** Every engine call from chunk code becomes a task; the "queue level" decides which chunk's tasks the
dispatcher serves first (smaller = earlier):

| Call | Type | Queue level |
| --- | --- | --- |
| `checkBlock(pos)` | PRE | the chunk's current holder level (maximum if no holder) |
| `updateSectionStatus`, `queueSectionData`, `retainData`, `updateChunkStatus` (unload) | PRE | **0** (highest) |
| `propagateLightSources`, threaded `setLightEnabled`, the PRE half of `initializeLight` / `lightChunk` | PRE | holder level |
| completion of `initializeLight` / `lightChunk` | POST | holder level |

**Logical batching** (this is what a single-threaded implementation must reproduce):
1. An accepted task is **appended** to `lightTasks`. If the list now holds **≥ 1000** tasks, `runUpdate` runs immediately.
2. Otherwise `runUpdate` runs when `tryScheduleUpdate` fires: it schedules one `runUpdate` (if none is pending) whenever the list is
   non-empty **or** an engine has light work (queues / inconsistencies). It is called from the main thread's task polling
   (`ServerChunkCache` executor, every poll that is not preceded by a distance-manager update) and after a chunk has been unloaded.
3. **`runUpdate`:** `N = min(size, 1000)`. (a) Run, in list order, the **PRE** tasks among the first `N`. (b) Run the level engine's
   `runLightUpdates` (block engine, then sky engine). (c) For the same first `N` tasks in list order: run the **POST** ones;
   remove every one of the `N` (PRE and POST). Tasks appended meanwhile stay for the next batch.
4. The threaded wrapper does **not** allow a direct `runLightUpdates` call.

**Dispatcher ordering (for fidelity, not required for value parity).** Tasks reach the list through a priority dispatcher: tasks
are grouped per chunk and per queue level (levels 0..N, ties served FIFO by first submission of that chunk at that level); the
lowest non-empty level is popped, **all tasks of one chunk at that level together**, in submission order. Cross-chunk interleaving
and the relative order of two tasks of the same chunk with different queue levels (for example `checkBlock` at the holder level vs
`updateSectionStatus` at 0) can therefore differ from submission order. The resulting stored light is order independent for the
cases in `light-engine.md` Open question 1; queue traces and run counts are not. **Zenith's deterministic model:** serve tasks in
global submission order and batch as above; flush (run `runUpdate` until no task and no engine work remain) at the points listed
in §11.

### 4. The `light` task

For the chunk `C` (server): `lighted = isLighted(C)`; `C.setLightCorrect(false)`; submit PRE task `lightChunk`: **if not
`lighted`**, `levelEngine.propagateLightSources(pos)` — block engine first (`light-engine.md` rule 9), then sky engine (rule 21) —
(nothing happens for an already lighted chunk); the future completes after a POST task that sets `C.setLightCorrect(true)`.
Because the step needs all 8 neighbours at `initialize_light`, the neighbours' sections are registered (their layers exist) before
any seeding, so light spreads across chunk borders in whichever order the `light` steps run.

### 5. Loading a chunk (NBT → engine)

On `SerializableChunkData.read` (server), in this order:
1. Parse per section tag: `Y` (byte), optional `block_states` / `biomes` (a section tag with a `Y` outside the world range
   yields no chunk section but its light arrays are still read), `BlockLight`, `SkyLight` (byte arrays; each must be exactly
   2048 bytes, else an error). `isLightOn` (byte, default false) → `lightCorrect`.
2. For each section data entry in list order: if it has a block-light array, or (sky engine exists **and** it has a sky array): on
   the first such entry call `retainData(pos, true)` once; then `queueSectionData(BLOCK, sectionPos, DataLayer(array))` and/or
   `queueSectionData(SKY, …)`. The arrays are **adopted**, not copied.
3. Chunk-class rules: statuses at or after `initialize_light` attach the light engine to the proto chunk at load.
4. The chunk later passes through the loading pyramid, whose `initialize_light` / `light` tasks run as in §2 and §4 with
   `lighted = (status >= light && isLightOn)`.

Consequences: **(a)** with `isLightOn` true, stored arrays are authoritative; sky fill (§2 step 5) only fills still-empty layers in
the fully-lit range; no seeding runs. **(b)** With `isLightOn` false (never lit, or lit by an old / external tool), stored
arrays are still adopted, `setLightEnabled(false)` is applied and `light` runs `propagateLightSources`, which only **raises**
values: stale values that are too high are not removed.

### 6. Saving and unloading

**Save (`SerializableChunkData.copyOf` / `write`):** for `sectionY` from `minLightSection` up to but excluding `maxLightSection`
(Overworld: −5..20): `blockLight = engine.getDataLayerData(sectionPos)` (queued-or-visible) of the block engine,
`skyLight` likewise; each is **kept only if non-null and not `isEmpty()`**, and is copied. A section entry exists if the chunk has a
section at that Y, or any kept light layer. `write` emits, per entry: `block_states` / `biomes` (if a section exists), `BlockLight`
bytes (materialised 2048-byte array), `SkyLight` bytes, then `Y`; an entry with no content is dropped. The chunk tag gets
`isLightOn = 1b` only when `lightCorrect`. Section order: ascending `Y`. (Compound key order inside tags follows the NBT writer's
rules, `../nbt/binary-format.md`.)

**Unload (`updateChunkStatus`, after the chunk was saved)**: submit PRE task (level 0): `retainData(pos, false)`;
`setLightEnabled(pos, false)` (both engines); for `sectionY` in the light range: `queueSectionData(BLOCK, …, null)` and
`queueSectionData(SKY, …, null)` (drops still-queued data only); then for every real section `minSectionY..maxSectionY`
ascending: `updateSectionStatus(sectionPos, true)`. Then `tryScheduleUpdate`. Layers that no neighbour keeps alive are removed in
the following run's end-of-run pass; layers still needed by neighbouring chunks stay with their data (`data-layers.md` Edge 2).

### 7. Live block changes (the enqueue rules)

`LevelChunk.setBlockState(pos, newState, flags)` (server, after `full`, and the same code on the client):
1. Section `S` of `pos.y`; `wasEmpty = S.hasOnlyAir()`. If `wasEmpty && newState.isAir()` → return (no light work).
2. Write the state into the section; if the returned old state **is** `newState` → return (no light work).
3. Update the four heightmaps.
4. `isEmpty = S.hasOnlyAir()`; if `isEmpty != wasEmpty`: `lightEngine.updateSectionStatus(pos, isEmpty)`
   (`SectionPos.of(pos)`, both engines; threaded: PRE task level 0) and the chunk source's emptiness hook.
5. If `hasDifferentLightProperties(old, new)`: `skyLightSources.update(chunk, localX, y, localZ)` (**immediately, on the calling
   thread**, `sky-light-sources.md` §2) and then `lightEngine.checkBlock(pos)` (a PRE task at the holder level that adds the position
   to both engines' check sets).
6. Later: block-entity / neighbour / `onPlace` handling (not lighting).

`ProtoChunk.setBlockState` (only for statuses at or after `initialize_light`; before that no light notification happens at all):
returns early with the new state when the section was only air and the new block is the plain air block; otherwise writes, then
**if the chunk's current status is at or after `initialize_light`**: steps 4 and 5 above (without the "old is new" early return;
`hasDifferentLightProperties` is false for identical states), then heightmaps. `ImposterProtoChunk` forwards to its wrapped level
chunk.

Engine side: the next batch runs `checkBlock` → `checkNode` for each queued position (hash-set order), then the queues drain, then
the end-of-run pass (`light-engine.md` Behavior rule 2).

### 8. Telling clients: change notification

At the end of a run the storage calls `chunkSource.onLightUpdate(layer, sectionPos)` for every affected section
(`data-layers.md` §7). On the server this schedules, on the main thread, `holder = visible holder of the section's chunk`;
`holder.sectionLightChanged(layer, sectionY)`:
1. null if the holder has no chunk at `initialize_light` yet → false. Else mark the chunk **unsaved**.
2. If the holder has no ticking (full) chunk → false.
3. If `minLightSection <= sectionY <= maxLightSection`: set bit `sectionY − minLightSection` in the SKY or BLOCK change filter; return
   true when the bit was newly set (then the holder joins the broadcast set).
`ChunkHolder.broadcastChanges` (called from `ServerChunkCache.broadcastChangedChunks` inside the per-tick chunk work, which runs even
when the tick rate is frozen): if either filter is non-empty and players are in range (including the border ring), send one
`ClientboundLightUpdatePacket` carrying `ClientboundLightUpdatePacketData(pos, engine, skyFilter, blockFilter)`; always clear both
filters.

**Packet data layout** (`ClientboundLightUpdatePacketData`; also embedded in the full-chunk packet with both filters null = all
sections): for `index` 0..lightSectionCount−1 ascending: sky first, then block, each if its filter bit is set (or the filter is
null): `data = engine.getDataLayerData(minLightSection + index)`; if null → nothing; if `data.isEmpty()` → set bit `index` in the
layer's "empty" mask; else set bit `index` in the layer's "data" mask and append a 2048-byte copy (materialised). Wire order:
sky data mask, block data mask, sky empty mask, block empty mask (as bit sets), then the list of sky arrays, then the list of block
arrays (each array length-prefixed, max 2048). Chunk sends read the light state at send time, via the same accessor; the server
makes chunk sends wait for pending light tasks only in special cases (`ChunkMap.waitForLightBeforeSending`, used by the End dragon
fight exit portal).

### 9. Client engines

The client builds `LevelLightEngine(clientChunkSource, blockLight = true, skyLight = dimension.has_skylight)` (not threaded).
- **Chunk with light packet:** (1) chunk data is applied immediately: sections are read and `initializeLightSources` (`fillFrom`)
  runs. (2) The light payload is **queued** (a FIFO of closures) and applied by the polling rule below: for each layer (sky first, then
  block) and each `index` ascending: if the data bit is set → `queueSectionData(layer, section, DataLayer(clone of bytes))`;
  else if the empty bit is set → `queueSectionData(layer, section, new empty DataLayer)`; (a light-update packet additionally marks
  the section and its neighbours dirty for rendering); then `setLightEnabled(chunkPos, true)` for both engines. (3) For a full chunk
  packet only, after step 2: for each section index ascending `updateSectionStatus(sectionPos, section.hasOnlyAir())`
  (this registers non-empty sections and creates their layers, consuming the queued ones through `createDataLayer`), and the
  surrounding render sections are marked dirty.
- **Forget chunk packet:** queued task: `setLightEnabled(pos, false)`, then `queueSectionData(…, null)` for both layers over the
  light range, then `updateSectionStatus(…, true)` for every real section ascending.
- **Polling rule** (`pollLightUpdates`): let `size` = queue length; run `size < 1000 ? max(10, size / 10) : size` queued closures
  (integer division), stopping early when the queue is empty. Then, once per rendered frame while game time advances (only after the
  game finished loading), `ClientLevel.update` calls `pollLightUpdates` followed by `runLightUpdates` on the client engine
  (block, then sky) — **one engine run per frame**, regardless of the number of packets applied.
- **Local prediction:** a block change on the client calls the same `LevelChunk.setBlockState` code (§7), whose `checkBlock` /
  `updateSectionStatus` calls act directly on the client engine (no tasks, no batching); the next frame's run recomputes light for
  that position. Server light packets later replace the layers (queued layer objects replace the updating layers in the end-of-run
  pass).
- Client unload of a chunk object (`ClientLevel.unload`) calls `setLightEnabled(pos, false)`.
- Client `onLightUpdate(layer, section)` marks the render section dirty (rendering only).

### 10. Light values exposed to game logic

All reads are of the **visible** layers (`data-layers.md` §8) through these definitions:
- `getBrightness(layer, pos)` = the layer's visible value (0 block / 0 sky for an absent engine; the sky value of a position with
  no layers is 15).
- `getRawBrightness(pos, darkening)` = `max(block, sky − darkening)` (`light-engine.md` rule 22; sky term 0 without a sky engine).
- `getMaxLocalRawBrightness(pos)` = `getMaxLocalRawBrightness(pos, skyDarken)`, where for a position with `x` or `z` outside
  `[−30000000, 30000000)` the answer is **15**, otherwise `getRawBrightness(pos, darkening)`.
- `getEffectiveSkyBrightness(pos)` = `max(sky(pos) − skyDarken, 0)`.
- `canSeeSky(pos)` = `sky(pos) >= 15`.
- `skyDarken` (per level, integer) = `trunc(15.0f − SKY_LIGHT_LEVEL)` where `SKY_LIGHT_LEVEL` is the float environment attribute
  `gameplay/sky_light_level` (default 15.0, range 0..15, not positional); it is recomputed at level construction and at the end of
  every level tick (server: after the world-border / weather / sleep handling and before the time advance; client: first thing in its tick). The
  attribute's value over the day comes from the environment-attribute system (outside this spec).
- `isBrightOutside()` = the dimension has no fixed time **and** `skyDarken < 4`.
- `getLightLevelDependentMagicValue(pos)`: `v = getMaxLocalRawBrightness(pos) / 15.0f` (f32); `curved = v / (4.0f − 3.0f·v)`;
  result `lerp(dimension.ambientLight, curved, 1.0f)`; the pathfinding cost uses that value minus 0.5.
- Block-level light emission seen by logic (`BlockGetter.getLightEmission(pos)`) is `emission(blockState at pos)`, independent of the
  engines.

### 11. When light is "settled" (for parity and for Zenith's own scheduler)

In vanilla, all engine runs happen on the light thread at times that depend on scheduling; the state visible to game logic can lag by
a few ticks. A parity probe must therefore compare **after quiescence**: no pending `lightTasks`, `hasLightWork()` false for the level
engine, and no queued closures on the client. Zenith's single-threaded flush points: (a) after the pipeline thread has submitted the
tasks of `initialize_light` and of `light` for a chunk (the step completes only after the flush), (b) after unload tasks, (c) before the
per-tick change broadcast, (d) before serialising a chunk for saving or sending, (e) any time `lightTasks.size() >= 1000`.

## Edge cases and known quirks

1. `lightCorrect` is cleared at the **start** of the `light` task, so a chunk saved between the task start and its completion is
   written without `isLightOn` and is relit when loaded.
2. The `light` step does nothing but flip `lightCorrect` for chunks that were saved as lighted; `initialize_light` is what registers them.
3. Worldgen features (earlier statuses) read brightness through the same engine (`WorldGenRegion.getLightEngine` is the level's
   engine). Block light at positions of chunks that have not been registered is 0 and sky light is 15 (or the value of the first
   layer above, `data-layers.md` §8); neighbour chunks that were already lit can contribute real values. Snow / ice placement uses
   `getBrightness(BLOCK, pos) < 10`; this is a timing-dependent read in vanilla (neighbour status at that moment).
4. `sectionLightChanged` accepts `sectionY == maxLightSection` (one past the valid index range); the extra bit is never read.
5. A chunk whose holder has no ticking chunk marks itself unsaved on any light change but sends nothing.
6. The server never runs `runLightUpdates` on the wrapper directly; the client calls it directly once per frame.
7. Loading a lit chunk whose neighbours are not loaded still creates layers in the neighbours' (unregistered) border sections
   (`data-layers.md` Edge 1).
8. Sky light in dimensions with `has_skylight = false` does not exist at all; stored `SkyLight` arrays in such a dimension's chunks
   are ignored on load and never written.

## Constants and IDs

Batch size 1000; client polling `size < 1000 ? max(10, size/10) : size`; light range padding 1; chunk NBT keys `isLightOn`,
`sections[].BlockLight`, `sections[].SkyLight`, `sections[].Y`; status names `initialize_light`, `light`; packet payload order sky mask,
block mask, empty sky mask, empty block mask, sky arrays, block arrays; light-engine task queue levels 0 (status / data tasks) and holder
level (everything else).

## Oracle probe points

- Chunk statuses: `net.minecraft.world.level.chunk.status.ChunkStatusTasks#initializeLight`, `#light` (hook entry / exit);
  `ThreadedLevelLightEngine#initializeLight(ChunkAccess, boolean)`, `#lightChunk(ChunkAccess, boolean)`, `#updateChunkStatus(ChunkPos)`,
  `#tryScheduleUpdate()`, `#waitForPendingTasks(int,int)`; `ChunkAccess#isLightCorrect()`, `#setLightCorrect(boolean)`.
- Per-chunk dumps after `light` completes and after quiescence: for all light sections of the chunk and its 8 neighbours, both layers:
  `LevelLightEngine#getLayerListener(layer).getDataLayerData(SectionPos)` (null / empty flag / 2048 bytes), plus
  `getLightValue(BlockPos)` for sampled positions; and `ChunkAccess#getSkyLightSources()` tables.
- Single block change driver: `ServerLevel#setBlock(BlockPos, BlockState, 3)`; settle by polling `ThreadedLevelLightEngine` via
  `ServerChunkCache#getLightEngine().hasLightWork()` and an empty `lightTasks` (agent: `ThreadedLevelLightEngine.lightTasks` size).
- Save form: `SerializableChunkData#copyOf(ServerLevel, ChunkAccess)` + `#write()`; load form: `SerializableChunkData#parse` + `#read`.
- Packet form: `new ClientboundLightUpdatePacketData(ChunkPos, LevelLightEngine, null, null)` and its `STREAM_CODEC`.
- Client: `ClientLevel#queueLightUpdate`, `#pollLightUpdates`, `#update`; `ClientPacketListener#handleLevelChunkWithLight`,
  `#handleLightUpdatePacket`, `#handleForgetLevelChunk`.
- Helpers: `LevelReader#getMaxLocalRawBrightness(BlockPos[, int])`, `#getEffectiveSkyBrightness`, `BlockAndLightGetter#canSeeSky`,
  `Level#getSkyDarken`, `Level#isBrightOutside`.

## Open questions

1. **[oracle] Exact content of a freshly generated chunk's saved light** (which light sections carry arrays; padding sections −5 and
   20; null-vs-materialised) — probe: generate a small area from a fixed seed, run to quiescence, `copyOf(...).write()` for the centre
   chunk and its neighbours; this also fixes the creation-order effect in `data-layers.md` §9.
2. **[oracle] Does the visible state depend on generation order of neighbours?** Probe: generate the same 5×5 chunk area twice with
   different ticket orders (centre first vs corners first), compare all dumps after quiescence.
3. **[oracle] Quiescence criterion on a live server.** Probe: after a scripted `setBlock`, record `hasLightWork()`, `lightTasks.size()`
   and the visible value each tick until stable; measure how many ticks vanilla needs for a single emitter, to document the
   observable lag against Zenith's flush model.
4. **[oracle] Unload with a lit neighbour.** Probe: load two adjacent chunks, unload one, dump border layers of the survivor and of
   the unloaded chunk's border sections (expected: layers and data survive, `data-layers.md` Edge 2).
5. **[oracle] `isLightOn` false relight.** Probe: load a chunk with `isLightOn` removed and stale high values planted in `BlockLight`;
   confirm the stale values persist (§5 consequence b).
6. **[oracle] Client first-chunk order** (§9 step 3): confirm that the queued sky arrays are the ones that end up in the layers when
   section status registration happens after `setLightEnabled`, by dumping client layers right after the first run.
7. **Open (outside this spec):** the value of `SKY_LIGHT_LEVEL` over time belongs to the environment-attribute / time spec.

## Parity status

Not yet run.
