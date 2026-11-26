# Ticks: the server tick, the level tick, chunk ticking and random ticks (26.3)

- Verified by suites: `server-tick`, `block-updates`
- Sources: local reference reading of the 26.3 client/server code (2026-11-26, reader session): the server main loop
  and tick, the tick-rate manager, the level tick, the chunk source and chunk map tick, the distance manager and its
  chunk trackers, the ticket storage, the chunk holder promotion path, the world clock manager, weather, sleep status,
  world border, the level-chunk section counters, block-entity ticker bookkeeping, the autosave path, and the pure-math
  position generator used for random ticks; fastutil order rules were checked with black-box fixtures
  ([`collection-orders.md`](collection-orders.md)). Everything is in our own words.
- Related specs: [`collection-orders.md`](collection-orders.md) (hash / heap orders used below),
  [`scheduled-ticks-and-block-events.md`](scheduled-ticks-and-block-events.md) (step 5/12 of the level tick),
  [`set-block-flags-and-sequence.md`](set-block-flags-and-sequence.md), [`neighbor-and-shape-updates.md`](neighbor-and-shape-updates.md),
  [`block-hooks.md`](block-hooks.md); [`../math/random.md`](../math/random.md) (the legacy 48-bit generator behind `Level.random`),
  [`../lighting/chunk-steps.md`](../lighting/chunk-steps.md) §7–§11 (live light updates and their flush points),
  [`../lighting/light-engine.md`](../lighting/light-engine.md), [`../storage/region-and-level.md`](../storage/region-and-level.md)
  (saved data files: world clocks, weather, tickets), [`../worldgen/surface-and-carvers.md`](../worldgen/surface-and-carvers.md)
  (biome temperature / snow rule), [`../math/java-math-helpers.md`](../math/java-math-helpers.md) (`ChunkPos.pack`,
  `Mth`, `Util.shuffle`).

## Summary

One *server tick* advances the whole game by one step. The order is fixed and every part of it is observable
(random-number draws are shared, block changes cascade into neighbours, entities and block entities read what blocks
did earlier in the same tick). This file gives, in order: what a server tick is (§1–§4), what each dimension does in its
tick (§5), time / weather / sleep / border (§6), what the chunk source does (§7), block-entity ticking (§8), which chunks
tick and in which order (§9), and exactly how random ticks pick positions and which random draws they make (§10–§12).
Scheduled block / fluid ticks and block events (steps of the level tick) are in
[`scheduled-ticks-and-block-events.md`](scheduled-ticks-and-block-events.md). **Entities are another milestone:** every
place where entity code runs is listed here only as a *hook position* with the random draws it is known to make.

## Inputs and state

### Server-wide
- `tickCount` (i32, starts at 0, +1 per executed server tick), `emptyTicks`, `ticksUntilAutosave` (starts **6000**).
- **Tick-rate manager** (one per server, shared by all dimensions): `tickrate` (f32, default **20.0**, minimum 1.0),
  `nanosecondsPerTick = trunc(1e9 / tickrate)`, `isFrozen`, `frozenTicksToRun`, `runGameElements` (bool, recomputed
  every tick, see §4), plus the sprint fields (`remainingSprintTicks`, `scheduledCurrentSprintTicks`,
  `previousIsFrozen`).
- **World clocks** (`ServerClockManager`): per world clock `totalTicks` (i64), `partialTick` (f32), `rate` (f32, default
  1.0), `paused` (bool), plus named time markers; saved data `world_clocks`.
- **Weather** (one record shared by the whole server): `clearWeatherTime`, `rainTime`, `thunderTime` (i32),
  `raining`, `thundering` (booleans).
- Game rules read by the tick (defaults): `random_tick_speed` 3 (int ≥ 0), `advance_time` true, `advance_weather` true,
  `players_sleeping_percentage` 100, `max_snow_accumulation_height` 1 (0–8), `spawn_mobs` true, `raids` true.
- Server properties that change tick behaviour: dedicated `pause-when-empty-seconds` (default 60; 0 disables),
  `simulation-distance` (default 10), `max-chained-neighbor-updates` (default 1,000,000; see
  [`neighbor-and-shape-updates.md`](neighbor-and-shape-updates.md)). The integrated (single-player) server takes the
  simulation distance from the client option `max(2, option)` and pauses itself when the pause screen is open (§1).

### Per dimension (`ServerLevel`)
- `gameTime` (i64; the *overworld* owns it, other dimensions share it through a derived level-data object), the
  level's two random sources (§10): **`random`** (a legacy 48-bit LCG, `docs/specs/math/random.md`, seeded from a
  unique seed at level construction — *not* from the world seed and *not* saved) and **`randValue`** (an i32 used only
  for random-tick positions, initialised from a thread-local random int — also not saved).
- `subTickCount` (i64, +1 per scheduled tick created), `emptyTime` (i32), `rainLevel`/`oRainLevel`/`thunderLevel`/
  `oThunderLevel` (f32), `skyDarken`, the two tick containers, the block-event queue, the block-entity ticker list and
  its pending list, the sleep status, the entity tick list, the neighbour updater.
- Per chunk: one section per 16 blocks of build height (24 in the overworld), each with counters `nonEmptyBlockCount`, `fluidCount`, `tickingBlockCount`,
  `tickingFluidCount` (kept by `setBlockState`, [`set-block-flags-and-sequence.md`](set-block-flags-and-sequence.md)),
  `inhabitedTime` (i64), the chunk's holder level.

## Behavior (ordered rules)

### 1. One iteration of the server loop: is this a tick?

1. Dedicated server and the vanilla loop: wall-clock pacing only decides *when* the next tick runs; it never changes
   what a tick does, except through *sprinting* (rule 3) and the empty-server pause (rule 2). Zenith's parity runs
   drive ticks directly.
2. **Empty-server pause** (dedicated, `pause-when-empty-seconds` > 0): at the start of every tick attempt, if no
   player is online and the manager is not sprinting, `emptyTicks += 1`, else `emptyTicks = 0`. When
   `emptyTicks >= 20 · seconds`: on the attempt where it *equals* the threshold an autosave (§13) runs once; **no
   tick is executed** (`tickCount` does not advance; only the network connections are polled). This holds until a
   player joins.
3. **Sprint decision** (before each iteration): if the server is not paused (rule 4), sprinting is active, and
   `checkShouldSprintThisTick()` is true, then the tick's budget is 0 ns (the next tick starts immediately and
   `haveTime` is always false for chunk-unload budgeting); otherwise the budget is `nanosecondsPerTick`.
   `checkShouldSprintThisTick()`: false if `runGameElements` is false (the previous tick was frozen); else if
   `remainingSprintTicks > 0` → decrement and true; else finish the sprint (§4) and false.
4. **Integrated server pause**: the single-player server recomputes `paused = (pause screen open) or (no players)` at
   the start of each tick attempt. On the transition not-paused → paused it saves everything (§13). While paused it
   executes no tick body (only polls connections and awards stats). On the transition paused → not-paused it forces a
   game-time sync packet and then runs the tick normally.

### 2. `tickServer` (the body of one tick), in order

1. (Rules 1.2 / 1.4 above can skip the whole body.)
2. `tickCount += 1`.
3. `tickRateManager.tick()` (§4).
4. `tickChildren` (§3).
5. Status refresh (ping data; not world state).
6. `ticksUntilAutosave -= 1`; if `<= 0`, autosave (§13).
7. Tick-time bookkeeping (`tickCount % 100`, no world effect).

Queued network packets are processed **before** `tickServer` (at the top of `processPacketsAndTick`), so player
actions of tick *N* are applied before tick *N*'s world update.

### 3. `tickChildren`, in order

1. Suspend packet flushing for all players (network).
2. **Command functions tick**: **only if `runsNormally`**: on the first tick after a (re)load the `#minecraft:load` function
   tag runs first; then the `#minecraft:tick` function tag runs.
3. **If `runsNormally`**: tick the world clocks (§6.1).
4. If `tickCount % 20 == 0`: broadcast a game-time sync (network only).
5. Recompute the effective respawn data (world-border-adjusted spawn).
6. **For each dimension, in the server's level order**: `level.tick` (§5). The order is the overworld first, then the
   other dimensions in the dimension-stem registry's order (vanilla: the_nether, the_end — **[oracle]**
   `MinecraftServer#getAllLevels()`).
7. Connection tick; player-list tick; debug subscribers; **if `runsNormally`** game tests; the server GUI tickables;
   per player: send the next chunks and resume flushing.
8. Server activity monitor tick.

### 4. Tick-rate manager: freeze, step, sprint

1. Every server tick, *before* anything else in `tickChildren`: `runGameElements = (not frozen) or (frozenTicksToRun >
   0)`; then if `frozenTicksToRun > 0` it is decremented. So `/tick freeze` sets `isFrozen`; `/tick step n` sets
   `frozenTicksToRun = n` (only if frozen): the next *n* ticks have `runGameElements = true`, the (n+1)-th tick is
   frozen again.
2. **`runsNormally`** (= `runGameElements`) gates, in the level tick: world border, weather, time, scheduled ticks,
   raids, chunk random ticking and spawning, block events, the dragon fight, block-entity ticking, and the clocks /
   game tests in `tickChildren`. A **frozen** tick still does: the sleep check (§6.4), sky brightness, ticket purge is
   *skipped* (only when `tickChunks` is true and the manager does not run normally), chunk-source distance-manager
   updates, change broadcast to clients, unloading, **entity ticking for players and for any entity that carries a player
   passenger** (`isEntityFrozen` is true for every other entity), and the entity-management step.
3. **Sprint** (`/tick sprint n`): sets `scheduledCurrentSprintTicks = remainingSprintTicks = n`, remembers
   `previousIsFrozen`, unfreezes. While sprinting each loop iteration runs a tick with no waiting (rule 1.3). The
   autosave interval during a sprint uses the average tick time instead of the tick rate (§13). When the counter reaches
   zero the sprint finishes: counters cleared, `isFrozen = previousIsFrozen`, `onTickRateChanged` (autosave interval
   re-evaluated). **A world-state parity run is independent of wall-clock time**: only the *sequence of ticks* and
   their `runsNormally` flag matter.
4. `/tick rate r`: `tickrate = max(r, 1.0)`, `nanosecondsPerTick = trunc(1e9 / tickrate)`; recomputes the autosave
   interval (it can only *shorten* the countdown).

### 5. `ServerLevel.tick` — the order of one dimension's tick

Let `runs = tickRateManager.runsNormally()`. In order:

1. `handlingTick = true`; invalidate the environment-attribute tick cache.
2. **If `runs`**: world-border tick; then weather cycle (§6.3).
3. **Sleep check** (always, also when frozen): `percentage = players_sleeping_percentage`. If
   `areEnoughSleeping(percentage)` **and** `areEnoughDeepSleeping(percentage, players)` (§6.4): if `advance_time` is
   true and the dimension has a default clock, move that clock to its "wake up from sleep" marker; wake every sleeping
   player; if `advance_weather` and the level `isRaining()`, reset the weather cycle (§6.3).
4. Recompute `skyDarken` (§6.5) — always.
5. **If `runs`**: `tickTime` (§6.2: game time +1 for the overworld, then the scheduled-function queue).
6. **Pending ticks** — if the level is not a debug-world level **and** `runs`: with `tick = gameTime` (the value *after*
   step 5): process the **block** tick container, then the **fluid** tick container, each with a limit of **65,536**
   ticks per call ([`scheduled-ticks-and-block-events.md`](scheduled-ticks-and-block-events.md) §3).
7. **If `runs`**: raids tick (hook; entity milestone).
8. **Chunk source tick** `chunkSource.tick(haveTime, tickChunks = true)` (§7): includes natural spawning, thunder, the
   random ticks of §10, ticket purge, distance-manager updates, change broadcast, POI and unload work.
9. **If `runs`**: run the block events ([`scheduled-ticks-and-block-events.md`](scheduled-ticks-and-block-events.md) §6).
10. `handlingTick = false`.
11. `emptyTime`: if the chunk source has any ticket whose type keeps the dimension active (§9.3) then `emptyTime = 0`;
    then **if `runs`**: `emptyTime += 1`.
12. **If `emptyTime < 300`**: (a) if there is a dragon fight and `runs`: dragon fight tick (hook); (b) **every entity** of
    the entity tick list in list order (hook: skipped when removed or frozen; despawn check; only when the entity's chunk
    is in entity-ticking range or it is a player; vehicles / passengers rules): entity tick (hook, entity milestone);
    (c) **block entities**: `tickBlockEntities` (§8). A dimension with no keep-alive ticket for 300 ticks therefore stops
    ticking entities and block entities — but **scheduled ticks and random ticks keep running** (they depend on chunk
    levels only).
13. `entityManager.tick` (entity section lifecycle), debug synchronisers, `randomState.garbageCollect` (hook; no world
    effect).

The light engine is *not* a step of the level tick: it runs on its own scheduler; see §12.

### 6. Time, weather, sleep, border, sky brightness

**6.1 World clocks** (server-level, once per tick, only when `runsNormally`, before any level ticks): if the game rule
`advance_time` is true, for **every** world clock that is not paused: `partialTick += rate` (f32 add);
`full = floor(partialTick)` (saturating i32); `partialTick -= full` (f32); `totalTicks += full`. When
`advance_time` is false nothing moves (and the saved data is not marked dirty).

**6.2 Game time** (`tickTime`, only the overworld has the flag set; only when `runs`): `gameTime += 1`, then the
scheduled-function queue (`/schedule function`) is run for the **new** game time. The other dimensions see the
incremented value because they tick after the overworld. Game time advances regardless of `advance_time`.

**6.3 Weather cycle** — only for a dimension where `canHaveWeather()` is true (has skylight, no ceiling, and is not the
End; vanilla: the overworld only; a custom sky dimension would advance the *shared* weather record a second time per
tick). Steps, only when `runs`:
1. `wasRaining = isRaining()`.
2. If the game rule `advance_weather` is true:
   a. Read `clear`, `thunderTime`, `rainTime`, `thundering`, `raining` from the shared record.
   b. If `clear > 0`: `clear -= 1`; `thunderTime = thundering ? 0 : 1`; `rainTime = raining ? 0 : 1`; `thundering = false`;
      `raining = false`.
   c. Else: **thunder first**: if `thunderTime > 0`: `thunderTime -= 1` and if it became 0 toggle `thundering`; else if
      `thundering`: `thunderTime = sample(3600..15600)`; else `thunderTime = sample(12000..180000)`. **Then rain**
      likewise: if `rainTime > 0`: decrement, toggle `raining` on reaching 0; else if `raining`: `rainTime =
      sample(12000..24000)`; else `rainTime = sample(12000..180000)`.
   d. Write all five fields back.
   `sample(a..b)` = `random.nextInt(b − a + 1) + a` using the **level's `random`** (always draws once — even for the
   thunder branch when rain is also drawing; the thunder draw precedes the rain draw).
3. `oThunderLevel = thunderLevel`; `thunderLevel += 0.01` if the record says thundering else `−= 0.01`; clamp to
   [0, 1] (f32). `oRainLevel = rainLevel`; `rainLevel += 0.01` if raining else `−= 0.01`; clamp [0, 1] (f32). (Step 3 runs
   whether or not `advance_weather` is true.)
4. Packets (network): rain/thunder level changes, start/stop raining if `isRaining()` changed.
- `isRaining() = canHaveWeather() && rainLevel > 0.2`; `isThundering() = canHaveWeather() && thunderLevel · rainLevel >
  0.9` (both on the current-tick value, interpolation factor 1).
- Resetting the weather cycle sets `rainTime = 0`, `raining = false`, `thunderTime = 0`, `thundering = false`
  (clear time untouched).
- At level construction, if `canHaveWeather()` and the record says raining: `rainLevel = 1.0`, and if also thundering
  `thunderLevel = 1.0`.

**6.4 Sleep status.** `activePlayers` = non-spectator players, `sleepingPlayers` = those of them that are in bed
(updated by player code through `SleepStatus.update`, not by the level tick). `needed = max(1, ceil(active ·
percentage / 100.0f))`. `areEnoughSleeping` = `sleepingPlayers >= needed`; `areEnoughDeepSleeping` = the number of
players with "slept long enough" `>= needed`. `removeAllSleepers` zeroes `sleepingPlayers`.

**6.5 Sky brightness**: `skyDarken = trunc(15.0f − SKY_LIGHT_LEVEL)` (environment attribute, f32); recomputed at
level construction and in step 4 of §5.

**6.6 World border** (`runs` only): the border's extent is replaced by `extent.update()`. A static extent returns itself.
A moving extent does: `lerpProgress −= 1`; `previousSize = size`; `size = (progress < 1.0) ? lerp(progress, from, to) : to`
with `progress = (duration − lerpProgress) / duration` (f64); marks the border data dirty; when `lerpProgress <= 0` the
extent becomes a static extent of size `to`. (Border damage, warning and the block-entity ticking restriction that
uses `isWithinBounds` belong to the entity / BE hooks.)

### 7. Chunk source tick (`ServerChunkCache.tick(haveTime, tickChunks = true)`)

In order:
1. **Purge**: if `runsNormally` (or `tickChunks` is false): purge stale tickets (§9.4).
2. Distance-manager updates (`runDistanceManagerUpdates`): run the three trackers, the loading tracker, promote
   holders and schedule generation work (chunk-lifecycle spec).
3. **Chunks** (only when `tickChunks`): `tickChunks()`: if the level is not a debug level: (a) **only if
   `runsNormally`**: the ticking step below; (b) *always*: broadcast changed chunks (network); then the chunk map's own
   `tick()` (entity tracking, network).
4. **Unload**: `chunkMap.tick(haveTime)`: POI manager flush (time-budgeted), then process pending unloads
   (time-budgeted: at least `max(0, queueSize − 2000)` unload tasks run, more while `haveTime`). Unload timing is
   wall-clock dependent and therefore **not reproducible**; keep chunks loaded in parity runs (the unload *result* —
   a chunk saved then dropped with its tick container, §5 of
   [`scheduled-ticks-and-block-events.md`](scheduled-ticks-and-block-events.md) — is deterministic).
5. Clear the chunk cache.

**The ticking step** (`tickChunks(profiler)`, only when `runsNormally`), in order:
1. `chunkCount` = number of chunks in the natural-spawn tracker (running its pending updates first); build the spawn
   state (mob counts per category; hook, no random draws).
2. `doMobSpawning = spawn_mobs`; **`tickSpeed = random_tick_speed`, read once here** (a change during the tick does not
   apply to this tick); the spawning categories for this tick (the persistent categories only when `gameTime % 400 ==
   0`).
3. **Collect spawning chunks**: iterate the natural-spawn tracker's key set (fastutil order, [`collection-orders.md`](collection-orders.md)
   §2); for each key whose holder exists and has a *ticking chunk* and where some player is close enough for spawning,
   append the chunk.
4. **Shuffle** that list with the level's `random`: backward Fisher–Yates, for `i = size` down to 2 draw
   `nextInt(i)` and swap element `i − 1` with it (no draw for size ≤ 1). This is `Util.shuffle` of `docs/specs/worldgen/features.md`.
   The shuffle **always consumes `size − 1` draws** (when `size ≥ 2`) even if mob spawning is off.
5. For each chunk of the shuffled list: `inhabitedTime += 1`; if the chunk is in entity-ticking range: **thunder**
   (§12); if the spawning category list is non-empty and the level may spawn entities in that chunk (entity section
   ticking and chunk inside the border): natural spawning (hook; consumes `random`).
6. **Block-ticking chunks**: for each chunk the simulation tracker lists with level ≤ 31 (§9.1, in that map's
   iteration order) that has a ticking chunk object: `tickChunk(chunk, tickSpeed)` (§10–§11).
7. If `doMobSpawning`: the dimension's custom spawners (phantoms, patrols, cats, village siege, wandering traders; hook).

(§7.6 is the *only* place random ticks happen. Note it uses the *entity-ticking* level (≤ 31), not the block-ticking
level (≤ 32).)

### 8. Block-entity ticking order

1. Every block entity whose block has a ticker (`getTicker` non-null) owns a *ticker wrapper*. Wrappers live in
   `Level.blockEntityTickers` (an array list) in **registration order**. A wrapper is created, and appended, when the
   block entity is registered into a loaded chunk: (a) on chunk activation, for every block entity already in the
   chunk map, in that map's iteration order (`Object2ObjectOpenHashMap`, [`collection-orders.md`](collection-orders.md)
   §2); (b) for block entities still stored as NBT (`pendingBlockEntities`), when each is promoted — either lazily on
   first access, or during chunk post-processing in the iteration order of that JDK `HashMap`
   ([`../java/hash-collections.md`](../java/hash-collections.md)); (c) when a block with a block entity is placed in a
   loaded chunk (new wrapper at the end); (d) when a block entity's state changes and the block now has a ticker where
   it had none (new wrapper). If a wrapper already exists for that position it is *rebound* (keeps its list position).
2. A wrapper added while the list is being walked (`tickingBlockEntities` true) goes to a *pending* list; the pending
   list is appended to the main list at the **start of the next** `tickBlockEntities` call.
3. `tickBlockEntities` (once per level tick, §5.12c): set the guard; append pending wrappers; `runs = runsNormally`;
   walk the list in order: a wrapper whose block entity was removed is **deleted from the list**; otherwise, if `runs` and
   the position's chunk is in *block-ticking range* (simulation level ≤ 32, §9.1), tick it.
4. A bound ticker only calls the block entity's ticker function if: the block entity is not removed and has a level;
   the position is inside the world border; the chunk's full status is at least *block ticking* and its entity section
   data is loaded; and the block state currently at the position is valid for the block-entity type (otherwise a
   warning is logged once and nothing runs). The ticker function receives `(level, pos, state-at-pos, blockEntity)`.
5. Removing a block entity (block replaced, chunk unload) rebinds its wrapper to a dead ticker, which is dropped from
   the list at the next walk; **placing a new block entity at the same position makes a new wrapper at the end of the
   list** (its tick order moves after all existing ones).
6. Block entities are ticked **after** entities within a dimension and only while `emptyTime < 300` (§5.12).

### 9. Chunk ticking ranges, tickets and the simulation tracker

**9.1 Levels.** Every chunk has a *simulation level* derived from tickets with the "simulates" flag. Meaning of the
level `L` (smaller = more active): `L ≤ 31` entity ticking; `L = 32` block ticking; `L = 33` full (loaded, not ticking);
the tracker stores levels 0..32 only (levels ≥ 33 are removed from the map). Concretely:
- *Random ticks* run for chunks with **`L ≤ 31`** and a ticking chunk object (§7.6).
- *Scheduled block / fluid ticks* run for chunks with **`L ≤ 32`**, a ticking chunk object, and loaded entity data
  (`isPositionTickingWithEntitiesLoaded`).
- *Block events* run for positions with `L ≤ 32` (otherwise deferred, §6 of the scheduled-ticks spec).
- *Block-entity tickers* run for positions with `L ≤ 32` (and the extra checks of §8.4).
- *Entities* tick in chunks with `L ≤ 31` (hook).
A chunk's **ticking chunk object** exists once its holder reached block-ticking status: all chunks within distance 1
(Chebyshev) were at *full* status, then post-processing ran and the tick containers were unpacked (§5 of
[`scheduled-ticks-and-block-events.md`](scheduled-ticks-and-block-events.md)).

**9.2 Level propagation.** The level of chunk `c` is `min` over all simulating tickets `t` in chunks `s` of
`t.level + chebyshevDistance(c, s)` (the 8-neighbourhood, +1 per step).

**9.3 Tickets** (types and flags; `level` of a ticket in parentheses):
| type | simulates | loads | keeps dimension active | timeout (ticks) | persisted |
|---|---|---|---|---|---|
| `player_simulation` | yes | no | yes | none | no |
| `forced` (31) | yes | yes | yes | none | yes |
| `portal` | yes | yes | yes | 300 | yes |
| `ender_pearl` | yes | yes | yes | 40 | no |
| `dragon` | yes | yes | no | none | no |
| `player_loading` | no | yes | no | none | no |
| `player_spawn` | no | yes | no | 20 | no |
| `spawn_search` | no | yes | no | 1 | no |
| `unknown` | no | yes (may expire while unloaded) | no | 1 | no |
A player's `player_simulation` ticket is added at the player's chunk with level `max(0, 31 − simulationDistance)` (so
chunks within Chebyshev distance `simulationDistance` have `L ≤ 31`, those at `simulationDistance + 1` have `L = 32`).
The distance manager's default simulation distance is 10 until the server sets it. A ticket with the same type and level at
a chunk is a duplicate: it only resets the timeout. A `ticketLevel` for a *radius* ticket is `33 − radius`.
Forced chunks use the *forced* ticket of level 31 at the chunk (so simulation distance 0 around it plus propagation:
`L = 31 + d` ⇒ only the chunk itself is entity ticking, distance 1 is block ticking).

**9.4 Ticket expiry** (once per tick, in the chunk-source `purge` step, only when `runsNormally`): for every ticket with a
timeout whose chunk holder is ready for saving (or whose type can expire while unloaded): decrement `ticksLeft`; a
ticket is removed when `ticksLeft < 0`. Removal re-evaluates the chunk's remaining tickets (§9.5). Iteration order is that
of the ticket map (`Long2ObjectOpenHashMap`, [`collection-orders.md`](collection-orders.md) §2) and, inside a chunk, list
order.

**9.5 How the simulation map is updated (the order that fixes §7.6).** The simulation tracker is a "min fixed point"
solver. State: `levelCount = 34`; for each level `0..33` an insertion-ordered set of chunk keys (a FIFO queue without
duplicates); `computed`: a map key → pending level; the result map `chunks` (key → level 0..32). Ticket storage calls
`update(key, level, onlyDecreased)`:
- adding a simulating ticket whose level is lower than the chunk's previous best: `update(key, ticketLevel, true)`;
- removing a simulating ticket / expiring: `update(key, newBestTicketLevelAtChunk, false)` (with "no ticket" = 34 ⇒ clamped 33).
`update` registers an edge from the virtual source to `key` (`checkEdge`). **`checkEdge(from, to, levelFrom, onlyDecreased)`:**
1. Clamp `levelFrom` and the stored level `levelTo` of `to` to `[0, 33]`; `oldComputed` = the pending level of `to`
   if there is one, else `levelTo` (and remember "was consistent" = no pending level).
2. `newComputed` = `min(oldComputed, levelFrom)` if `onlyDecreased`; otherwise the full recomputation
   `getComputedLevel(to, from, levelFrom)` (clamped): start from `levelFrom`, then for each of the 9 offsets
   `(dx, dz)` with `dx = −1..1` outer and `dz = −1..1` inner, neighbour `n = pack(x+dx, z+dz)` (the node itself counts as
   the *source*): skip `n == from`; cost = (source: the best simulating-ticket level at the node, or a value above 33 if none) or
   (`level(n) + 1`); keep the minimum; stop early at 0.
3. `oldPriority = min(levelTo, oldComputed, 33)`. If `levelTo ≠ newComputed`: `newPriority = min(levelTo, newComputed, 33)`;
   if the priority changed and it was not consistent, remove `to` from the queue of `oldPriority`; append `to` to the queue
   of `newPriority`; `computed[to] = newComputed`. Else (equal) if it was not consistent: remove from the old queue and
   forget the pending level.
**`runUpdates`:** while any queue is non-empty: take the **first element of the lowest non-empty level queue**
(FIFO); `level` = its stored level (clamped), `computed` = remove its pending level. If `computed < level`: `setLevel(node,
computed)` and run `checkNeighborsAfterUpdate(node, computed, onlyDecrease = true)`. If `computed > level`: `setLevel(node, 33)`;
if `computed ≠ 33` re-queue the node at priority `min(33, computed)` with that pending level; then
`checkNeighborsAfterUpdate(node, level, false)`. (Equal: nothing.)
**`checkNeighborsAfterUpdate(node, level, onlyDecrease)`:** if `!onlyDecrease || level < 32`: for `dx = −1..1` (outer), `dz =
−1..1` (inner), neighbour ≠ node: `checkNeighbor(node, neighbour, level, onlyDecrease)`: `levelFrom = clamp(level + 1)`;
if `onlyDecrease`: `checkEdge(node, neighbour, levelFrom, true)`; else: with `stored` = the neighbour's pending level
(if any) and `oldComputed = stored` (or its clamped stored level when none): only if `levelFrom == oldComputed` call
`checkEdge(node, neighbour, 33, false)` (so it recomputes).
**`setLevel(node, level)`** on the simulation tracker: `level ≥ 33` → remove the key from `chunks`; else put `(key →
level)` (a *put* of an existing key keeps its slot; a put of a new key claims the first free slot of its probe path).
The *insertion/removal sequence* of `chunks` is therefore exactly the sequence of `setLevel` calls above, and the
iteration order of §7.6 is the fastutil order of that map ([`collection-orders.md`](collection-orders.md) §2). The same solver
(with other level counts) drives the loading tracker, the natural-spawn tracker (`levelCount = 10`, source level 0 at every
chunk containing a player, entries kept for levels ≤ 8) and the player-loading tracker (`levelCount = 34`); the natural-spawn
tracker's `chunks` map order feeds §7.3.

### 10. Random ticks: positions, draws, order

`tickChunk(chunk, tickSpeed)` for one block-ticking chunk (`minX`, `minZ` = chunk origin), in order:

1. **Ice / snow ("iceandsnow")**: repeat `tickSpeed` times: draw `random.nextInt(48)` (level `random`); if it is 0:
   position `p = randomPos(minX, 0, minZ, yMask = 15)` (advances `randValue`, rule 3), then `tickPrecipitation(p)` (§11).
   (No iteration when `tickSpeed == 0`. This phase runs for every ticking chunk, even if all its sections are empty.)
2. **Random block ticks**, only if `tickSpeed > 0`: for each section **in ascending section index** (lowest Y first):
   if `section.isRandomlyTicking()` (its `tickingBlockCount > 0` or `tickingFluidCount > 0`, evaluated *once* before the
   inner loop): repeat `tickSpeed` times:
   a. `p = randomPos(minX, sectionMinY, minZ, 15)` (rule 3) with `sectionMinY = sectionY · 16`.
   b. `state` = the block state at `p` (read from the section) — **read before any tick runs**.
   c. If `state.isRandomlyTicking()`: call the block's random-tick hook with `(state, level, p, level.random)`.
   d. `fluid = state.getFluidState()` (taken from the *same, possibly stale* `state` of step b — if the random tick changed
      the block, the fluid used is still the old one's); if `fluid.isRandomlyTicking()`: call the fluid's random-tick hook
      with `(level, p, level.random)`.
3. **Position generator** `randomPos(xo, yo, zo, yMask)`: `randValue = randValue · 3 + 1013904223` (wrapping i32);
   `v = randValue >> 2` (arithmetic shift); the position is `(xo + (v & 15), yo + ((v >> 16) & yMask), zo + ((v >> 8) &
   15))`. Every call advances `randValue` by one step, **whether or not the position is used**. `randValue` is a
   per-dimension field; it is never reseeded after construction and not saved.
4. Draw accounting: the `randValue` stream is advanced `tickSpeed · (1 + number of randomly-ticking sections)` times at
   most per chunk (precisely: once per precipitation hit and once per section iteration); the `random` stream is advanced
   by `tickSpeed` draws of `nextInt(48)` plus whatever the ticked blocks draw. Chunks are processed one after another, so the
   chunk **order** (§7.6) decides which positions each chunk gets.
5. A block's random-tick hook may place blocks, schedule ticks, spawn entities; those effects happen immediately and are
   visible to the *next* position of the same chunk.

### 11. Precipitation (`tickPrecipitation(p)`)

Let `top = heightmapPos(MOTION_BLOCKING, p)` = `(p.x, firstAvailableY(p.x, p.z), p.z)` where `firstAvailableY` is the
`MOTION_BLOCKING` heightmap value + 1 (a position outside any loaded chunk uses the dimension's minimum Y), and `below =
top − (0, 1, 0)`. Let `biome` = the biome at `top`.
1. If `biome.shouldFreeze(level, below)` → set `below` to `minecraft:ice` with **flags 3** (neighbour + client update).
   `shouldFreeze(level, pos, checkNeighbors = true)`: false if the biome is *warm enough to rain* (height-adjusted
   temperature `≥ 0.15`, see [`../worldgen/surface-and-carvers.md`](../worldgen/surface-and-carvers.md)); otherwise true iff `pos` is inside the build
   height, the **block light** there is `< 10`, the block *is a water liquid block* (`LiquidBlock` with the water fluid),
   and **not** all four horizontal neighbours are water (`isWaterAt`); a water source / flowing block with all four horizontal
   water neighbours does not freeze.
2. **If `isRaining()`** (current level's weather):
   a. `maxHeight = max_snow_accumulation_height`. If `maxHeight > 0` and `biome.shouldSnow(level, top)`:
      `shouldSnow` = precipitation at `top` is SNOW (biome has precipitation and is cold enough), `top` inside build
      height, block light at `top` `< 10`, the state at `top` is air or snow, **and** a default `snow` state `canSurvive` at `top`.
      Then: if the state at `top` is `snow` with `layers < min(maxHeight, 8)` → new state `layers + 1`, *push entities up*
      (`Block.pushEntitiesUp(old, new, level, top)`) and set it with flags 3; if it is not `snow` → set the default `snow`
      state at `top` with flags 3. (A snow block with `layers ≥ min(maxHeight, 8)` is left alone.)
   b. `precipitation = biome.getPrecipitationAt(below, seaLevel)`; if not NONE: call the **`handlePrecipitation` hook** of the
      block at `below` with `(state, level, below, precipitation)` (cauldrons fill, etc.).
3. `getHeightmapPos` and the freeze check use the **live** heightmaps (updated by every block change).

### 12. Thunder (called from the spawning-chunk loop, §7.5)

`tickThunder(chunk)` draws, in order and only on the path shown: (1) if `isRaining()` **and** `isThundering()`: draw `random.nextInt(100000)`
(short-circuit: *no draw* otherwise); if it is 0: (2) `p = randomPos(minX, 0, minZ, 15)` (advances `randValue`); `target =
findLightningTargetAround(p)`: height-map position (`MOTION_BLOCKING`) of `p`; the closest lightning-rod POI within 128 blocks whose Y is `WORLD_SURFACE − 1` →
its position + 1; else living entities that are alive and can see the sky inside the box from the target column's block to the dimension's top (`maxY + 1`) inflated by 3: if any, draw
`random.nextInt(count)` and use that entity's block position; else the heightmap position (`+2` if it equals `minY − 1`).
(3) If `isRainingAt(target)`: `isTrap = spawn_mobs && random.nextDouble() < effectiveDifficulty · 0.01 && !(state below target
is in #lightning_rods)` (left-to-right short-circuit: the double is drawn only when `spawn_mobs` is true; the block test only when
the draw succeeded); a skeleton horse (trap) is created first when `isTrap`, then the lightning bolt (hooks: entity milestone,
they consume further draws). Lightning-rod POI search and difficulty depend on the POI manager and the local difficulty model
(other specs).

### 13. Light, autosave, everything else positioned in the tick

- **Light.** `checkBlock` calls happen inside `setBlock` (§3 of [`set-block-flags-and-sequence.md`](set-block-flags-and-sequence.md));
  the light engine's batches run on their own schedule. For parity, compare **after quiescence**
  ([`../lighting/chunk-steps.md`](../lighting/chunk-steps.md) §11). Zenith's single-threaded flush points: before the change broadcast
  of §7.3b, and before saving or sending a chunk. Game logic that reads light *during* the tick (grass, crops, mob spawning)
  sees the *visible* values, which in vanilla can lag the queued updates by a scheduling-dependent amount — **[oracle]**:
  the lag is not deterministic; see Open question 4.
- **Autosave.** `ticksUntilAutosave` counts down once per executed tick (after the world tick). At `≤ 0` the server saves
  everything non-flushing: all players, then per dimension (in level order) the level data / saved data, the chunk source
  (every unsaved chunk, each serialising its tick containers with the **current game time**), the entity sections; then the
  global level data. After an autosave `ticksUntilAutosave = max(100, trunc(tickrate_f32 · 300.0f))` (6000 at 20 tps; when
  sprinting `tickrate` is replaced by `1e9 / (averageTickNanos + 1)`). Changing the tick rate sets the countdown to `min(old, new
  interval)`. Autosave never alters world state; it matters for saved-tick sub-order only through the pack rules of
  [`scheduled-ticks-and-block-events.md`](scheduled-ticks-and-block-events.md) §5.
- **Network** (chunk sends, block-change broadcast, light packets): no effect on world state.

## Edge cases and known quirks

1. **Two RNG streams.** Positions of random ticks come from `randValue` (rule 10.3), everything a block draws comes from
   `Level.random`. Both are seeded non-deterministically at level construction and are *not* part of any save. A parity
   harness must set both (Oracle probe points).
2. **Random-tick chunk order is not insertion order**: it is the fastutil table order of the simulation tracker, which depends
   on the tracker's update history (collisions, rehashes) — [`collection-orders.md`](collection-orders.md).
3. **Entity-ticking vs block-ticking level**: random ticks use ≤ 31, scheduled ticks / block events / block entities use ≤ 32
   — a one-chunk ring in which scheduled ticks and block entities run but random ticks do not.
4. **Stale fluid state** (rule 10.2d): when a random tick replaces a block, the fluid hook of the *old* state is still
   called.
5. **Sleep check runs while frozen**; weather, time, border and scheduled ticks do not.
6. **Weather advancement is per ticking dimension** but the weather record is global (§6.3).
7. **`tickSpeed == 0`**: no random ticks and no precipitation, but the spawning shuffle still consumes draws (§7.4) and
   thunder still happens.
8. **`emptyTime`** hides entity / block-entity ticking of unvisited dimensions but not chunk ticking (§5.12).
9. **Sub-tick ordering of the wake-up**: the order "sleep check → sky brightness → time" is observable: waking players happens
   *before* the time advance of that tick, and the clock move happens before `tickTime`.
10. **Autosave rounding**: the interval is computed in f32 (`tickrate · 300.0f`) then truncated.

## Constants and IDs

`ticksUntilAutosave` start 6000; autosave interval `max(100, trunc(tickrate · 300))`; scheduled ticks per tick per container
65,536; `emptyTime` threshold 300; ticket levels: entity ticking 31, block ticking 32, full 33; simulation tracker
`levelCount` 34; natural-spawn tracker `levelCount` 10 (max distance 8); player-loading tracker `levelCount` 34;
random-tick position LCG: multiplier 3, increment 1013904223, `v = randValue >> 2`, masks 15 / `yMask` 15; precipitation
draw `nextInt(48)`; thunder draw `nextInt(100000)`; weather: `RAIN_DELAY` and `THUNDER_DELAY` uniform 12000..180000,
`RAIN_DURATION` uniform 12000..24000, `THUNDER_DURATION` uniform 3600..15600; rain / thunder level step 0.01; rain
threshold 0.2, thunder threshold 0.9; freeze temperature 0.15; freeze / snow block-light limit 10; maximum snow layers 8;
spawning-persistent period 400 ticks; tick-rate floor 1.0; dedicated default simulation distance 10.

## Oracle probe points

(Public API of the 26.3 server, usable from a Java agent or a server-side probe.)

- Drive and observe ticks: `MinecraftServer#tickServer(BooleanSupplier)` (protected: reflection / agent), `#getTickCount()`,
  `#tickRateManager()` → `ServerTickRateManager#setFrozen(boolean)`, `#stepGameIfPaused(int)`, `#requestGameToSprint(int)`,
  `TickRateManager#runsNormally()`; `ServerLevel#tick(BooleanSupplier)`, `Level#getGameTime()`, `ServerLevel#getRandom()`.
- Seed the two RNG streams per dimension before a run: `Level#getRandom().setSeed(long)` and set the protected int field
  `Level#randValue` by reflection (it is a plain `int`, not final).
- Order of chunks: `ChunkMap#forEachBlockTickingChunk(Consumer<LevelChunk>)` (public), `DistanceManager#forEachEntityTickingChunk(LongConsumer)`,
  `DistanceManager#inEntityTickingRange(long)` / `#inBlockTickingRange(long)` / `#getChunkLevel(long, boolean)`,
  `ChunkMap#collectSpawningChunks(List<LevelChunk>)`.
- Drive one chunk's random ticks directly: `ServerLevel#tickChunk(LevelChunk, int tickSpeed)` (public); precipitation:
  `ServerLevel#tickPrecipitation(BlockPos)` (public, test-visible); thunder: `ServerLevel#tickThunder(LevelChunk)`.
- Weather: `ServerLevel#getWeatherData()`, `Level#getRainLevel(float)`, `#getThunderLevel(float)`, `ServerLevel#resetWeatherCycle()`.
- Block entities: `Level#blockEntityTickers` (protected list; agent) read through `TickingBlockEntity#getPos()` / `#getType()`; `Level#tickBlockEntities()`.
- Tickets: `ServerChunkCache#addTicketWithRadius`, `ServerLevel#setChunkForced(int, int, boolean)` (command path), `TicketStorage#getTickets(long)`.
- State dump per tick: after each `tickServer`, dump per ticking chunk: `LevelChunk#getBlockState` palettes, `LevelChunk#getBlockEntities()`,
  `ServerLevel#getBlockTicks()` / `getFluidTicks()` contents, the pending block events (`blockEvents` by reflection), `Level#randValue`, the
  level `random` state (a copy: `LegacyRandomSource#nextLong()` on a clone, or reading its seed field), `gameTime`, weather record.

## Open questions

1. **[oracle] Level order** of `MinecraftServer#getAllLevels()` for the vanilla dimensions (expected overworld, the_nether,
   the_end). Probe: print `server.getAllLevels()` dimension keys on a fresh world.
2. **[oracle] Initial `randValue` / `random`**: confirm both are re-initialised at construction and unaffected by any saved file.
   Probe: start the same world twice, read both before the first tick.
3. **[oracle] Tracker insertion order for the first chunk set.** The simulation map's slot layout depends on ticket-change order
   when a player joins (player-simulation ticket, then loading tickets). Probe: join a fresh world, dump
   `forEachBlockTickingChunk` order each tick for the first 40 ticks and compare with a Zenith replay of §9.5; also repeat after
   moving the player two chunks (removals + rehash).
4. **[oracle] Light visibility lag.** How many ticks does the visible block-light / sky-light value lag after a `setBlock` when
   the same tick's random tick reads light (grass spreading, crop growth, snow, mob spawning)? Probe: place a torch, call
   `getBrightness(BLOCK, p)` in the same tick, after 1, 2, 3 ticks, over 100 repetitions; if the lag is always 0 on the main thread
   after quiescence, record it.
5. **[oracle] Natural-spawn tracker contents for several players** (`FixedPlayerDistanceChunkTracker(8)`): the claim "entries kept for
   levels ≤ 8" and `levelCount = 10`. Probe: `DistanceManager#getNaturalSpawnChunkCount()` for one player at the origin
   (expected 17 × 17 = 289 chunks).
6. **[oracle] `isRaining` after load**: whether `rainLevel` is restored to exactly 1.0 (rule 6.3 last bullet) on dimension creation
   for a saved raining world, and the effect of a *second* ticking sky dimension. Probe: custom dimension with sky + no ceiling.
7. (implementer) Debug-world levels (`isDebug`) skip scheduled ticks, random ticking and chunk spawning; not specified further.
8. (implementer) The empty-server pause and the integrated-server pause are not part of the engine core; a headless parity run
   never triggers them.

## Parity status

Not yet run.
