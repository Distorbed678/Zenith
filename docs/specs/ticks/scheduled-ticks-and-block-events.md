# Ticks: scheduled block / fluid ticks and block events (26.3)

- Verified by suites: `server-tick`, `block-updates`
- Sources: local reference reading of the 26.3 client/server code (2026-11-26, reader session): the scheduled-tick
  containers, the level-wide tick processor, the saved-tick record, the proto-chunk and world-generation tick access,
  the chunk load / save / promotion path, the level's block-event queue, and the ticking-chunk promotion; the heap and hash
  orders were checked against the public JDK / fastutil behaviour ([`collection-orders.md`](collection-orders.md)).
  Everything is in our own words.
- Related specs: [`server-tick.md`](server-tick.md) (where in the tick this runs, chunk ranges),
  [`collection-orders.md`](collection-orders.md), [`set-block-flags-and-sequence.md`](set-block-flags-and-sequence.md),
  [`block-hooks.md`](block-hooks.md), [`../storage/region-and-level.md`](../storage/region-and-level.md) (region files),
  [`../registry/identifiers-registries-tags.md`](../registry/identifiers-registries-tags.md) (block / fluid ids),
  [`../worldgen/features.md`](../worldgen/features.md) §1 (block writes during generation),
  [`../lighting/chunk-steps.md`](../lighting/chunk-steps.md) (chunk step order).

## Summary

Blocks and fluids ask the world to call them back later (*scheduled ticks*: a `(block or fluid, position, trigger time,
priority)` record, at most one per `(type, position)`), and blocks ask the world to deliver a small integer message to a
position's block at the end of the tick (*block events*: pistons, note blocks, bells, chests, …). This file specifies
the data, the exact drain order of scheduled ticks (which is not a plain sort), the per-tick limit, what happens to ticks
scheduled while ticks run, how containers are created / saved / loaded / upgraded, and the block-event queue.

## Inputs and state

- **Level counters:** `gameTime` (i64), `subTickCount` (i64, per dimension, starts at 0, never saved; every created
  scheduled tick takes the current value and increments it — *including* ticks that are then rejected as duplicates).
  A world-generation region keeps its own counter, also starting at 0.
- **Tick record** `ScheduledTick`: `type` (a block, for the block container, or a fluid, for the fluid container),
  `pos` (immutable block position), `triggerTick` (i64), `priority`, `subTickOrder` (i64). **Equality for the
  one-per-position rule is `(type identity, pos)` only.**
- **Priority** (`TickPriority`) values and order (drain order = ascending value): `EXTREMELY_HIGH` −3, `VERY_HIGH` −2,
  `HIGH` −1, `NORMAL` 0 (the default), `LOW` 1, `VERY_LOW` 2, `EXTREMELY_LOW` 3. Decoding an integer: exact match, else
  below −3 → `EXTREMELY_HIGH`, above 3 → `EXTREMELY_LOW`.
- **Per-chunk container for a loaded chunk** (`LevelChunkTicks<T>`), one for blocks and one for fluids: a priority queue of
  scheduled ticks ordered by `(triggerTick, priority, subTickOrder)` ascending, a set of the `(type, pos)` pairs it holds,
  an optional **pending list** of saved ticks not yet unpacked (§5), and an optional "tick added" callback owned by the
  level.
- **Per-chunk container while a chunk is being generated** (`ProtoChunkTicks<T>`): an ordered list of saved-tick records
  plus the `(type, pos)` set (§5.2).
- **Level processor** (`LevelTicks<T>`, one for blocks and one for fluids): `allContainers` (chunk key → container),
  `nextTickForContainer` (fastutil `Long2LongOpenHashMap`: chunk key → trigger time of that container's head tick, default
  constructor), the heap `containersToTick` (JDK `PriorityQueue` ordered by the head tick's `(priority, subTickOrder)` —
  *without* the trigger time), the FIFO `toRunThisTick`, the list `alreadyRunThisTick`, the lazily built set
  `toRunThisTickSet`, and the predicate "chunk may tick" (§3.2).
- **Block-event queue** (`ServerLevel.blockEvents`, an insertion-ordered set of `(pos, block, paramA, paramB)` records) and the
  reschedule list.
- **Constant:** `MAX_SCHEDULED_TICKS_PER_TICK = 65,536`.

## Behavior (ordered rules)

### 1. Creating and scheduling a tick

1. `scheduleTick(pos, type, delay[, priority])` creates a tick with `triggerTick = gameTime + delay` (i64; the delay is an
   i32 and may be 0 or negative) and `subTickOrder = subTickCount++`; the priority defaults to `NORMAL`. The same call
   exists for blocks and for fluids (two containers).
2. **Server level** → `LevelTicks.schedule(tick)`: find the container of the tick position's chunk; **if the chunk has no
   registered container the tick is silently dropped** (a log line only). Otherwise `LevelChunkTicks.schedule`.
3. `LevelChunkTicks.schedule(tick)`: **if a tick for the same `(type, pos)` is already in the container (queued or still
   pending-unpacked) the new tick is discarded — even if it is earlier or has a better priority.** Otherwise it is queued; if
   a "tick added" callback is registered and the new tick is now the container's head tick, the level's
   `nextTickForContainer[chunk]` is set to its trigger time. (The sub-tick number was consumed either way.)
4. **World-generation region** (`WorldGenRegion`, also used for features and structures; its `scheduleTick` goes to the chunk
   that holds the position, which must be inside the region's chunk cache): the tick goes to that chunk's container —
   `ProtoChunkTicks` for a chunk still being generated: it is stored as a saved-tick record with **delay 0** (the requested
   delay and the sub-tick number are *dropped*; only type, position and priority survive); `hasScheduledTick` answers from the
   same set; `willTickThisTick` is always false. For a chunk that is already `full` (wrapped in a read-only "imposter" chunk)
   the container is a **black hole**: scheduling is ignored and `hasScheduledTick` is false.
5. `hasScheduledTick(pos, type)` (level): the container of the position's chunk reports whether its `(type, pos)` set holds
   the pair (so a tick collected for the *current* run but not yet executed no longer counts; a tick still *pending
   unpack* does count).
6. `count()` of a level processor = sum over containers of `queued + pending`.

### 2. The per-chunk container

1. Order inside one container is a total order `(triggerTick, priority, subTickOrder)`; `peek` / `poll` return the
   smallest. Within one chunk two ticks never tie on all three (sub-tick numbers are unique), so any ordered structure is
   equivalent.
2. `poll` also removes the pair from the `(type, pos)` set; so while a tick is waiting in `toRunThisTick` or running,
   the same `(type, pos)` **can be scheduled again** (it goes into the container for a later tick).
3. `removeIf(predicate)` removes matching ticks from queue and pair set (used by `clearArea`, §7).
4. `pack(currentTick)` (saving): the result starts with the still-pending saved ticks (if the chunk was never unpacked), in
   their stored order, followed by the queued ticks **sorted ascending by `subTickOrder` only** (not by trigger time), each
   converted to a saved tick with `delay = (int)(triggerTick − currentTick)` (i32 truncation; can be 0 or negative for
   overdue ticks) and the same priority.
5. `unpack(currentTick)` (see §5.3): numbering of the pending ticks.

### 3. The level-wide processor: one call per container type per level tick

`tick(currentTick = gameTime, maxTicks = 65536, action)`:

**3.1 Collect** (`collectTicks`):
1. **Sort containers.** Walk `nextTickForContainer` in its fastutil iteration order
   ([`collection-orders.md`](collection-orders.md) §2.7), skipping entries whose stored time is `> currentTick`. For an entry
   with stored time `<= currentTick`: look up the container; **if the container is gone** or **has no head tick** → remove
   the entry (iterator removal, [`collection-orders.md`](collection-orders.md) rule 2.8); else if the head tick's trigger time is `> currentTick` → store that time in the entry
   (no structural change); else if the chunk may tick (§3.2) → remove the entry and **add the container to
   `containersToTick`** (heap insertion in this walk order); otherwise leave the entry as is (it is re-examined every tick;
   the ticks stay queued and become overdue).
2. **Drain.** While fewer than `maxTicks` ticks are collected and the heap is non-empty:
   a. poll the top container (smallest head by `(priority, subTickOrder)`); poll its head tick and append it to
      `toRunThisTick`.
   b. **Greedy run on that container:** let `next` be the head tick of the heap's new top container (or none, evaluated
      **once**, here). While fewer than `maxTicks` are collected: look at this container's head tick `h`; stop if there is none, or
      `h.triggerTick > currentTick`, or `next` exists and `h` compares **greater** than `next` by `(priority, subTickOrder)`
      (equal does *not* stop); else poll `h` and append it.
   c. Afterwards, if the container still has a head tick `h`: if `h.triggerTick <= currentTick` **and** fewer than `maxTicks`
      are collected → put the container back into the heap (insertion at its current head); else store `h.triggerTick` in
      `nextTickForContainer[chunk]` (a put; an absent key is inserted).
   (A container that became empty has no entry.)
3. **Reschedule leftovers:** for every container still in the heap (array order), store its head tick's trigger time in
   `nextTickForContainer[chunk]`.

**3.2 "Chunk may tick"** (the predicate): the chunk's entity section data is loaded **and** the chunk source reports the
position as ticking: the chunk is in *block-ticking range* (simulation level ≤ 32, [`server-tick.md`](server-tick.md) §9) and
its holder has a completed ticking-chunk future. (Chunks in range but not yet promoted are skipped.)

**3.3 Run** (`runCollectedTicks`): take `toRunThisTick` in FIFO order; for each tick: remove it from `toRunThisTickSet` if the
set is non-empty; add it to `alreadyRunThisTick`; call the action with `(pos, type)`. **All ticks of a call are collected
before the first one runs**, so ticks scheduled during the run (even with delay 0, even for another chunk) are **never**
executed in the same call; they are collected by the next level tick's call. For blocks the action is:
read the block state at `pos`; **only if it is still a state of that block** (`state.is(type)`) call the block's *tick hook*
`(state, level, pos, level.random)`. For fluids: read the state at `pos`; **only if its fluid state is of that fluid type**
call the fluid's tick hook `(level, pos, blockState)`. A tick whose block changed meanwhile is silently dropped.

**3.4 Cleanup:** clear `toRunThisTick`, `containersToTick`, `alreadyRunThisTick`, `toRunThisTickSet`.

**3.5 Limit.** `maxTicks` limits only the *collection*: at most 65,536 ticks run per call. Containers and ticks beyond the
limit stay queued (their `nextTickForContainer` entries are refreshed in 3.1.2c / 3.1.3) and are collected next tick in the
same order rules. Blocks and fluids have separate limits (two calls).

**3.6 `willTickThisTick(pos, type)`** (used by blocks to avoid scheduling something that is about to run): builds
`toRunThisTickSet` from the *current* contents of `toRunThisTick` the first time it is asked while the set is empty and the
queue is not, then tests `(type, pos)` membership. Entries leave the set as they run; ticks that were *collected and already
run* are not found. Outside the run phase of a call the queue is empty, so it answers false.

**3.7 Order summary.** The drain order is **not** `(triggerTick, priority, subTickOrder)` globally: within one container it
is; across containers it is "by head tick's `(priority, subTickOrder)`", so a tick with better priority in another chunk can
run before a tick with an earlier trigger time in a different chunk. Ticks with `triggerTick > currentTick` never run. The
*run* order of one call is the *collect* order.

### 4. Ticks scheduled by the ticks themselves

1. Tick hooks run in the order above; each may place blocks (neighbour / shape updates run inline, [`neighbor-and-shape-updates.md`](neighbor-and-shape-updates.md)),
   schedule further ticks, and queue block events. Any tick scheduled now has `triggerTick = gameTime + delay` and the next
   sub-tick number, and runs on the first later call for which it is `<= currentTick` — for a delay of 0 that is the **next level
   tick**.
2. Because `poll` released the `(type, pos)` pair (§2.2), a tick may reschedule *itself* from inside its own hook.
3. A hook that causes a `setBlock` of a different block at a position with a collected-but-unrun tick makes that tick a no-op
   (§3.3 state check) — it is *not* removed from `toRunThisTick`.

### 5. Container lifecycle: creation, saving, loading, promotion, upgrade

**5.1 Persisted form.** In a chunk's NBT, `block_ticks` and `fluid_ticks` are lists of compounds `{ i: <registry id of the
block / fluid>, x, y, z: <absolute integer position>, t: <i32 delay>, p: <i32 priority value> }` (a missing list is empty;
when reading, entries whose position is **outside the chunk** are discarded; a decode failure of the list makes it empty
**[oracle]**). `PostProcessing` (list per section of i16 packed offsets `x | y<<4 | z<<8`) is stored alongside (§5.5).

**5.2 Generation phase.** A chunk of status below `full` owns `ProtoChunkTicks`: `schedule` appends a saved-tick record
(delay 0) unless the `(type, pos)` pair is already present; `pack` returns the list **as is** (so a proto chunk saved to disk
keeps scheduling order, delay 0, and re-loading keeps whatever delays the file has); `load(list)` re-adds records in order,
dropping later duplicates of a pair.

**5.3 Promotion to a level chunk** (`full` chunk status task, then the ticking promotion):
1. When a proto chunk becomes a level chunk, its record list becomes the **pending list** of a fresh `LevelChunkTicks`, and the
   `(type, pos)` set is seeded with those pairs. A chunk loaded from disk as an already-`full` chunk gets its saved list as the
   pending list the same way.
2. The `full` task registers both containers with the level (`addContainer`): a head tick is none yet (everything is pending) so
   no `nextTickForContainer` entry is made; the callback is installed.
3. **Ticking promotion** (`prepareTickingChunk`, requires every chunk within distance 1 to be `full`; order inside):
   (i) **post-processing** of the chunk (§5.5); (ii) **`unpack(gameTime)`**: let `n` = number of pending records; record `k`
   (0-based, in list order) becomes a queued tick with `type, pos, priority` from the record,
   **`triggerTick = gameTime + delay`**, **`subTickOrder = −n + k`** (so the chunk's own numbering is `−n … −1`); each is
   queued with the normal callback; the pending list is dropped.
4. Consequences: (a) on equal trigger time and priority, pending ticks sort *before* every tick scheduled since the level started (non-negative sub-ticks); (b) two
   chunks both unpacked from saves reuse the same negative numbers, so **ties between containers are normal** and are decided
   by §3.1.1's walk order and the heap rule of [`collection-orders.md`](collection-orders.md) §3; (c) a `scheduleTick` for a
   `(type, pos)` that is pending is **rejected** (§1.3) and the ticks that post-processing (step i) schedules enter the queue
   *before* the unpack, so on equal `(trigger, priority)` they sort *after* the unpacked ones (non-negative vs negative).
5. **Unload** (chunk saved, then `unregisterTickContainerFromLevel`): `removeContainer` drops the `allContainers` and
   `nextTickForContainer` entries and the callback; the container's queued ticks leave memory (they were already packed into
   the saved chunk by `pack(gameTime)` with the sub-tick-sorted order of §2.4). A chunk that is `full` but not yet ticking
   still serializes its pending list unchanged.
6. **Save** at any time: `pack(gameTime)` with the **current** `gameTime` (autosave, unload, `/save-all`): delays are relative
   to the save time.

**5.4 Reload ordering rule (what a saved world looks like after load).** Saved order = pending ticks first (never
unpacked), then queue sorted by sub-tick; on load, record `k` gets `−n + k`; so relative order of equal `(trigger, priority)`
ticks of one chunk survives a save / load; the *global* tie pattern between chunks does not (all chunks restart from `−n`).

**5.5 Post-processing at ticking promotion** (`postProcessGeneration`; also the place where some "fix-up ticks" are born):
for every section index ascending, for every packed offset in the section's list in insertion order: unpack to a position;
`state` = block at that position; if its fluid state is non-empty call the fluid's **tick hook immediately**
`(level, pos, state)`; then if the block is a *liquid block* call the block's **tick hook** `(state, level, pos, level.random)`;
otherwise recompute the state from its neighbours' shapes (`Block.updateFromNeighbourShapes`) and if it changed
set it with **flags 276** (`256 | 16 | 4`: no block-entity side effects, known shape, no re-render). Then clear the section's list.
After all sections: every pending block-entity NBT is promoted in the iteration order of that JDK `HashMap`
([`../java/hash-collections.md`](../java/hash-collections.md)) (the keys are copied first), the pending map is cleared, and
the chunk's `UpgradeData.upgrade` runs (§5.6). Lists are filled during generation by `markPosForPostProcessing`
(set-block flag rule in [`set-block-flags-and-sequence.md`](set-block-flags-and-sequence.md) §5) and persisted in `PostProcessing`.
Blocks that request post-processing of their own position: brown mushroom, red mushroom; of the position **above**: soul sand,
magma block.

**5.6 Legacy upgrade data** (only for chunks that carry `UpgradeData`, i.e. converted legacy worlds; none in a fresh 26.3
world). `upgrade(chunk)` in order: (1) `upgradeInside`: for each section ascending, for each stored index (packed
`x | z<<4 | y<<8`) in order: for each of the six directions (`Direction.values()` order) inside the same chunk, run the block's
fixer on the state; commit with `Block.updateOrDestroy(old, new, level, pos, flags 18)`. (2) the eight *sides* in `Direction8`
order whose bit is set (cleared as processed): for every position of that edge/corner strip (`BlockPos.betweenClosed` order,
z outermost) apply the fixer for each of the six directions, commit with flags 18. (3) re-schedule saved neighbour ticks:
each `neighbor_block_ticks` / `neighbor_fluid_ticks` record is scheduled through the level with its own delay and priority (an
"air" / "empty" type means "whatever block / fluid is there now"). (4) *chunky fixers* (the leaf-distance fixer) run once per
level. Fixers: the default fixer calls the block's own `updateShape`; a blacklist (observer, nether portal, concrete powders, anvils,
dragon egg, gravel, sand, red sand, all signs / hanging signs) returns the state unchanged; chests join into double chests (swapping
contents for north / east facing); leaves collect updated positions per leaf distance and fix distances breadth-first by distance
2..6; melon / pumpkin stems at age 7 with the fruit beside them become the attached stem facing it. **Not specified in detail
(legacy worlds only): Open questions 7.**

### 6. Block events

1. **Queueing** (`blockEvent(pos, block, a, b)` on a server level): append a record `(pos, block, a, b)` to the queue **unless an
   equal record is already queued** (the set keeps the first one's place). The position object is stored as given (callers pass
   an immutable position; a mutable position passed by a caller would change later — Zenith must copy). No range / chunk check.
2. **When it runs:** once per level tick at §5.9 of [`server-tick.md`](server-tick.md) (after chunk-source work), only when the tick
   runs normally (`runsNormally`). While the game is frozen the queue just grows.
3. **Processing loop:** clear the reschedule list; repeat until the queue is empty: remove the **first** record; **if its position
   is in block-ticking range** ([`server-tick.md`](server-tick.md) §9, level ≤ 32): read the block state at the position; **if it is a
   state of the event's block**, call the *event hook* `triggerEvent(state, level, pos, a, b)`; **if the hook returns true**, a
   `ClientboundBlockEventPacket(pos, block, a, b)` is broadcast to players of that dimension within 64 blocks. **Else (position not
   ticking)** append the record to the reschedule list. A record whose position now holds a different block is dropped silently
   (no packet).
4. **Events queued while the loop runs** (e.g. the piston hook queues a follow-up) are appended to the queue and **processed in
   the same loop**, after everything already queued. A new event equal to one still waiting is dropped; equal to one already
   processed is accepted (the processed one left the set).
5. **After the loop** the reschedule list is appended to the queue in order (duplicates with newly queued equals are dropped), so
   non-ticking-position events wait for a later level tick, **behind** all events queued this tick.
6. **Hooks that queue events** (parameters `(a, b)` are hook-private): note block `(0, 0)`; potent sulfur block `(0, 0)`; piston base
   `(event type 0 = extend / 1 = retract / 2 = drop-in-place, facing 3D data value)`; chest / ender chest / shulker box `(1, open
   count)`; bell `(1, click direction 3D value)`; decorated pot `(1, wobble style ordinal)`; spawner `(event id, 0)`; end gateway
   `(1, 0)`. Block entities forward `triggerEvent` to their block entity (`BaseEntityBlock`) and return its boolean. The client
   applies the hook directly (no queue) when it receives the packet.
7. `clearBlockEvents(box)` (test / command utility) removes queued events whose position is inside a bounding box.

### 7. `clearArea` / `copyArea` (clone, structure moves; level-wide processors)

1. `clearArea(box)`: for each chunk container intersecting the box (chunk x outer, z inner, ascending): remember the head tick,
   `removeIf(position inside box)`; if the head tick object changed: if a new head exists set the `nextTickForContainer` time to
   its trigger, else remove the entry. Also remove matching ticks from `alreadyRunThisTick` and `toRunThisTick`.
2. `copyArea(box, offset)` (`copyAreaFrom(source, …)`): gather, in this order, the matching ticks of `source.alreadyRunThisTick`,
   then `source.toRunThisTick`, then each source container intersecting the box (chunk x outer, z inner; each container's queue
   order); let `minSub`/`maxSub` be the min / max `subTickOrder` of the gathered ticks; schedule each copy at `pos + offset` with the
   same type, trigger time and priority and `subTickOrder' = subTickOrder − minSub + maxSub + 1` — **using the supplied sub-tick
   (not a fresh counter value)**, through the normal `schedule` (dedup, drop if no container).

## Edge cases and known quirks

1. **One tick per `(type, pos)`:** a later `scheduleTick` for the same pair is *ignored* (not merged, not re-timed), even for an
   earlier time. Blocks avoid this by checking `hasScheduledTick` / `willTickThisTick` first; the framework must not "improve" it.
2. **Delay-0 ticks run next tick**, never the same tick (§3.3 / §4.1) — observable in zero-tick pulse behaviour.
3. **The drain order is per container**, so a lower-priority tick in chunk A with an earlier trigger can run **after** a better-priority
   tick of chunk B with a later trigger (§3.7). Within one chunk it is `(trigger, priority, sub-tick)`.
4. **Greedy run uses `>` on the neighbour's head** (§3.1.2b): ties let the current container keep draining.
5. **World-generation ticks have delay 0 and lose their sub-tick order** (only scheduling order survives); water and lava placed by
   features are therefore ticked on the first level tick after the chunk starts ticking (together with the post-processing of §5.5).
6. **Duplicates in the pending list are not merged** when the list comes from a saved `full` chunk (the seed of the pair set absorbs
   them but the list keeps both; both unpack, the second one then runs in addition).
7. **A save with overdue ticks** writes `delay ≤ 0`; they run on the first tick after load (trigger `gameTime + delay ≤ gameTime`).
8. **Priority 3 / −3 clamping** applies to saved values only.
9. The queue limit of §3.5 is per container type per level tick; nothing is dropped.
10. **`gameTime` for sub-tick numbers survives nothing:** after a restart sub-tick numbering restarts at 0 and pending ticks use
    negative numbers; a tick scheduled in the very first tick after load therefore sorts after loaded ones on equal
    `(trigger, priority)`.
11. **Block events are not saved.** They are lost on shutdown / chunk unload; events for a chunk that leaves the ticking range stay queued.

## Constants and IDs

`MAX_SCHEDULED_TICKS_PER_TICK` 65,536; priorities −3…3; post-processing packing `x | y<<4 | z<<8` (i16); saved fields `i, x, y, z, t, p`
in lists `block_ticks` / `fluid_ticks`; copy flags used by post-processing 276 (`256 | 16 | 4`); legacy-upgrade flag 18 (`16 | 2`); block-event
broadcast radius 64.0; block-event queue / reschedule list: insertion-ordered set / list.

## Oracle probe points

- Dump every container tick-by-tick: for each loaded `LevelChunk`, `LevelChunk#getBlockTicks()` / `getFluidTicks()` → `LevelChunkTicks#getAll()`
  (stream of `ScheduledTick`: `type()`, `pos()`, `triggerTick()`, `priority()`, `subTickOrder()`), `#count()`, `#peek()`,
  `#hasScheduledTick(BlockPos, T)`, and `#pack(long currentTick)` → `SavedTick#delay()` / `#priority()`. Pending (not yet unpacked) ticks are in the
  private `pendingTicks` field (agent-only).
- Level side: `ServerLevel#getBlockTicks()` / `#getFluidTicks()` → `LevelTicks#count()`, `#hasScheduledTick`, `#willTickThisTick`; agent-only fields
  `LevelTicks.nextTickForContainer`, `containersToTick`, `toRunThisTick`, `alreadyRunThisTick`.
- Order of execution: wrap `BlockBehaviour$BlockStateBase#tick(ServerLevel, BlockPos, RandomSource)` and `FluidState#tick(ServerLevel, BlockPos, BlockState)`
  and log `(gameTime, pos, block)`; compare the full sequence per tick.
- Drive: `ServerLevel#scheduleTick(BlockPos, Block, int, TickPriority)` (via `ScheduledTickAccess`), `ServerLevel#tick(BooleanSupplier)`,
  `Level#nextSubTickCount()` (mutating).
- Save path: `SerializableChunkData#copyOf(ServerLevel, ChunkAccess)` then `#write()` → inspect `block_ticks` / `fluid_ticks` / `PostProcessing`.
- Block events: `ServerLevel#blockEvent(BlockPos, Block, int, int)`; the queue `blockEvents` and `blockEventsToReschedule` (agent-only reflection);
  wrap `BlockBehaviour$BlockStateBase#triggerEvent(Level, BlockPos, int, int)`; `ServerLevel#clearBlockEvents(BoundingBox)`.

## Open questions

1. **[oracle] Container tie order after a reload.** Probe: place single scheduled ticks (`scheduleTick` with delay 5) in 3 different chunks, `/save-all`,
   restart, then log execution order for the tick on which they fire; compare with the rules of §3.1 / §5.3 (negative sub-ticks, fastutil walk,
   heap ties).
2. **[oracle] Heap tie behaviour** of `PriorityQueue` with equal comparator keys (§3 of the collection spec) — probe with 6+ containers whose head
   ticks are all `(NORMAL, −1)`; log the poll order.
3. **[oracle] `nextTickForContainer` iteration with iterator removal and wrap-around** (collection spec rule 2.8) under real chunk keys: dump the
   map before and after each `LevelTicks#tick` for a world with 200 chunks holding ticks.
4. **[oracle] Decode failure of a tick list** (unknown block id inside `block_ticks`): whether the whole list or only the entry is lost. Probe: hand-edit
   a chunk's `block_ticks` with one bad id and load it.
5. **[oracle] Duplicate `(type, pos)` records in a saved `full` chunk** (Edge case 6): both run? Probe: hand-edit a chunk with two equal records.
6. **[oracle] `ImposterProtoChunk` black hole**: confirm that features placed from a neighbour region into a `full` chunk neither write blocks nor schedule
   ticks (set-block spec Edge 4). Probe: generate a chunk next to an existing full chunk with a feature that overlaps; check block ticks and blocks.
7. **(unresolved, legacy only)** The remaining details of §5.6 (the exact loop bodies of the side upgrade strips and the full fixer table beyond the
   five named ones) were summarised, not specified. Zenith's target worlds are created by 26.3; implement only if legacy-world import is scheduled.
8. (implementer) The "chunk may tick" predicate (§3.2) needs the chunk-lifecycle spec's holder promotion; for a single-threaded engine treat "entities
   loaded" as true and "ticking future complete" as "all 8 neighbours are `full` and the chunk was promoted".

## Parity status

Not yet run.
