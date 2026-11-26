# Parity: tick-by-tick scenarios (`ZSC1` in, `ZSO1` out)

- Used by the M10 suites (`server-tick`, `block-updates`, redstone, fluids, pistons); specs they check:
  [`../ticks/server-tick.md`](../ticks/server-tick.md), [`../ticks/scheduled-ticks-and-block-events.md`](../ticks/scheduled-ticks-and-block-events.md),
  [`../ticks/set-block-flags-and-sequence.md`](../ticks/set-block-flags-and-sequence.md), [`../ticks/neighbor-and-shape-updates.md`](../ticks/neighbor-and-shape-updates.md),
  [`../ticks/fluids.md`](../ticks/fluids.md), [`../ticks/redstone.md`](../ticks/redstone.md), [`../ticks/pistons.md`](../ticks/pistons.md).
- Tooling (own original code): `tools/oracle/src/dev/zenith/oracle/Scenario*.java` (agent side),
  `xtask/src/oracle.rs` (`run_scenario`, `ScenarioSpec`, `parse_scenario_output`, `cargo xtask oracle scenario`).
- Everything is big-endian. `str` = `u16` byte length + UTF-8. Nothing here contains game data; block states are
  global state ids and registry ids from the user's own ingested tables (`~/.cache/zenith/versions/<v>/tables.bin`).

## 1. What a scenario run is

The user's own vanilla dedicated server is started with the oracle agent (`scenario=<ZSC1 file>,scenario_out=<ZSO1 file>`).
The agent takes over the server's tick method and runs this protocol on the server thread:

1. **First server tick (setup).** Find the scenario dimension; apply the game rules; (optionally) raise the tick rate so
   the run is not paced at 20 ticks per second; add a *forced-chunk ticket* for every chunk of the chunk box, **x
   ascending (outer), z ascending (inner)**, each call also loading the chunk to FULL (blocking).
2. **Loading ticks.** The server runs normally (empty world, no players) until every chunk of the box is *entity ticking*
   (it appears in the level's random-tick chunk walk) and every chunk of the one-chunk ring around the box is block ticking
   (`isPositionTickingWithEntitiesLoaded`). How many ticks this takes is wall-clock dependent (about 50) and **not
   observable**: everything below is pinned after it.
3. **Pin.** At the start of the next tick (this becomes *timeline tick 0*): overworld game time := **0**; the level's
   sub-tick counter := **0**; the level random and the random-tick LCG := the values from the file (§3.2).
4. **Timeline.** Timeline ticks `k = 0 .. totalTicks-1`. For each: the operations with `tick == k` and phase *Start* run
   (file order) → light sync → **the vanilla server tick body runs untouched** → light sync → the operations with
   `tick == k` and phase *End* run. After the last tick the output is written and the server is halted. A failure writes the
   output with a non-zero status and a message.

Semantics worth stating exactly:

- **Phase *Start* of tick `k`** = after tick `k-1` has completed, before any part of tick `k` ran (`gameTime == k`).
  **Phase *End* of tick `k`** = after the whole tick body, including the block-event drain and the block-entity phase
  (`gameTime == k+1`). A *Start* dump of tick `k+1` therefore equals the *End* dump of tick `k` plus whatever operations
  ran in between (none by default). Use Start for "what did my operations just queue", End for "state after the tick".
- Operations are the *only* source of changes besides the game itself; they run as if from a command / plugin: no player,
  no chunk-loading side effects (every position must be inside the loaded chunks).
- Scheduled ticks created at *Start* of tick `k` with delay `d` get `trigger = k + d`; the tick processor of tick `k`
  runs ticks with `trigger <= k+1` (game time is incremented first, [`server-tick.md`](../ticks/server-tick.md) §5.5–5.6),
  so a delay-1 tick scheduled at *Start* of tick `k` **runs in tick `k`**, one scheduled at *End* of tick `k` runs in tick `k+1`.
- There are no players and no entities unless an operation makes the game create them (falling sand creates a falling-block
  entity). Entity behaviour is an entity-milestone concern; the examples that create entities say so.

### 1.1 World and server configuration (chosen by `xtask`, `ScenarioWorld::default`)

`level-type=minecraft:flat` with `generator-settings={"layers":[{"block":"minecraft:bedrock","height":1}],"biome":"minecraft:plains","features":false,"lakes":false,"structure_overrides":[]}`:
**one bedrock layer at the very bottom (y = −64), air above, biome plains everywhere, no structures, no features, no
lakes.** (The game accepts a custom flat generator through `server.properties`; no data pack is needed.) Scenarios build
everything else with operations, so terrain never interferes; the scenarios' stone floors live at y = 0. Other server
properties: `generate-structures=false`, `difficulty=normal`, `spawn-monsters/animals/npcs=false`, `allow-nether=false`,
`pause-when-empty-seconds=0` (**required**: otherwise the empty server stops ticking after 60 s), `view/simulation-distance=3`,
`max-tick-time=-1`, `level-seed=0`. The biome matters for random ticks: plains is warm enough that no precipitation
or freezing logic acts in a dry world (§3.4).

## 2. File formats

### 2.1 `ZSC1` scenario (input)

```
"ZSC1"
str   dimension                      "minecraft:overworld" (others work, but the pins below are for this dimension only)
i32   chunkMinX, chunkMinZ, chunkMaxX, chunkMaxZ     inclusive chunk box held loaded + ticking
u8    flags                          bit0 light_sync (recommended), bit1 fast_ticks (recommended; tick rate 5000, no gameplay effect)
u16   ruleCount;  ruleCount x ( str name, str value )   game rules, short id ("random_tick_speed") and command text ("3", "false")
u8    rngMode                        0 = leave Level.random, 1 = setSeed(rngValue), 2 = raw 48-bit state = rngValue & (2^48-1)
i64   rngValue
u8    hasRandValue;  i32 randValue   the random-tick LCG value (Level.randValue)
i32   totalTicks                     timeline ticks to run (>= 1)
i32   opCount;  opCount x op
op:   i32 tick;  u8 phase (0 = Start, 1 = End);  u8 code;  payload
```

Operations are sorted (stable) by `(tick, phase)` by the agent; ties keep file order.

| code | op | payload |
|---|---|---|
| 1 | `SET_BLOCK` | `i32 x,y,z, state, flags` — `Level.setBlock(pos, Block.stateById(state), flags)` (the 3-argument form: update limit 512) |
| 2 | `FILL` | `i32 x1,y1,z1, x2,y2,z2, state, flags` — inclusive box (corners may be in any order); one `setBlock` per cell, **y ascending (outer), z, x ascending (inner)** |
| 3 | `SCHEDULE_BLOCK` | `i32 x,y,z, blockRegistryId, delay, priority` — `scheduleTick(pos, block, delay, priority)` (priority −3..3, `TickPriority` values; out-of-range clamps) |
| 4 | `SCHEDULE_FLUID` | same with a fluid registry id |
| 5 | `BLOCK_EVENT` | `i32 x,y,z, blockRegistryId, a, b` — `Level.blockEvent(pos, block, a, b)` (queued; processed at the block-event step of the current tick) |
| 6 | `PIN_RNG` | `u8 mode, i64 value, u8 hasRandValue, i32 randValue` — same meaning as the header fields, applied now (also clears the cached Gaussian) |
| 7 | `DUMP` | `i32 minX,minY,minZ, maxX,maxY,maxZ; u8 sections` — write a dump record of the box (§2.2); sections bits: 1 blocks, 2 scheduled ticks, 4 block events, 8 block entities |
| 8 | `GAMERULE` | `str name, str value` — change a game rule mid-run |

`state` is a *global block-state id* (`Block.getId`), registry ids are raw registry ids (`BuiltInRegistries.BLOCK/FLUID.getId`; these equal
the `protocol_id` of the data generator's registries report). `flags` are the `Block.UPDATE_*` bits of
[`set-block-flags-and-sequence.md`](../ticks/set-block-flags-and-sequence.md) (3 = neighbour + client, 2 = client only, 16 = no
neighbour reactions, 18 = silent build step, 512 = skip `onPlace`). A dump box must lie inside the loaded chunks
(chunk box plus the one-chunk ring); anything else fails the run with a message.

### 2.2 `ZSO1` output

```
"ZSO1"
str   dimension
i32   plannedTicks
records until END:
  u8 = 1  DUMP
    i32 timelineTick;  u8 phase (0 Start / 1 End)
    i64 gameTime                                   Level.getGameTime()
    i64 randomState                                raw 48-bit internal state of Level.random (0 .. 2^48-1)
    u8  gaussianCached;  f64 gaussianValue         cached second Marsaglia-polar value (0.0 when none)
    i32 randValue                                  random-tick LCG (Level.randValue)
    i64 subTickCount                               next sub-tick number the level will hand out
    i32 levelBlockTicks, levelFluidTicks           LevelTicks.count() of the whole level (queued + pending-unpack)
    i32 chunkCount;  chunkCount x (i32 cx, i32 cz) chunks in the order the level walks them for random ticks
    i32 minX,minY,minZ, maxX,maxY,maxZ             the dump box, normalised (min <= max)
    u8  sections
    [bit0]  i32 n (= volume);  n x i32 stateId     y ascending, z, x ascending (x fastest)
    [bit1]  block ticks, then fluid ticks, each:  i32 n;  n x ( i32 x,y,z; i32 typeId; str typeName; i64 triggerTick; i32 priority; i64 subTickOrder )
            sorted by (triggerTick, priority, subTickOrder) ascending; only ticks whose position is in the box; queued ticks only
    [bit2]  i32 n;  n x ( i32 x,y,z; i32 blockId; str blockName; i32 a; i32 b )   pending block events inside the box, in queue order
    [bit3]  i32 n;  n x ( i32 x,y,z; i32 len; len bytes )   block entities in the box sorted by (y, z, x);
            the bytes are the entity's saved NBT as `NbtIo.writeAnyTag` writes it (tag-type byte, then the unnamed root compound);
            len = -1 if the entity cannot be saved
  u8 = 255  END     (always last)
    u8 status (0 = the timeline finished)
    str message       (empty on success; the failure otherwise)
    i32 ticksRun      completed timeline ticks
```

The output file is written once, at the end, and also from a shutdown hook, so a killed server still yields an `END` record
with status 1.

## 3. What Zenith has to pin and assume (the Zenith-side obligations)

A Zenith run of a scenario must be driven so the same observable inputs hold. Per scenario:

### 3.1 Clock

- Timeline tick `k` has game time `k` at its start and `k+1` at its end (overworld). Trigger times in the output are absolute
  game times on this scale. The game time is pinned to 0 even though the vanilla server has already run ~50 loading ticks.
- The level's **sub-tick counter** starts at 0 at timeline tick 0 and increments for every scheduled tick *created* —
  including ones rejected as duplicates and ones for unloaded positions ([`scheduled-ticks-and-block-events.md`](../ticks/scheduled-ticks-and-block-events.md) §1).
  It is part of the output (`subTickCount`, and `subTickOrder` of every tick), so drift shows immediately.
- Weather and the world clocks are frozen by the recommended game rules (`advance_weather=false`, `advance_time=false`).
  The world starts clear with no rain, and weather does not advance, so neither the weather random draws nor rain / snow logic run.
- `emptyTime` stays 0 (the forced tickets keep the dimension active), so entities and block entities tick every tick.

### 3.2 Random sources — **neither is seeded from the world seed; the harness must set both**

| source | vanilla field | how the oracle pins it | what the dump reports | Zenith obligation |
|---|---|---|---|---|
| **level random** | `Level.random` (a legacy 48-bit linear congruential source, `docs/specs/math/random.md`) | mode 1: `setSeed(v)` → state `(v ^ 0x5DEECE66D) & (2^48-1)` and the cached Gaussian is cleared; mode 2: write the raw 48-bit state | `randomState` = the raw 48-bit state **after** the tick / at the dump point (the value the next draw starts from), plus the Gaussian cache | construct the level random with the same state: `RngPin::Seed(v)` ⇒ `Random::from_seed(v)`; `RngPin::Raw(s)` ⇒ state `s`. One state step is `s = (s * 0x5DEECE66D + 0xB) & (2^48-1)`; `next(bits) = (int)(s >> (48 - bits))`. `setSeed(0)` therefore starts at state `0x5DEECE66D` (the first dump of a no-draw scenario shows exactly that) |
| **random-tick LCG** | `Level.randValue` (an `int`) | set to `randValue` | `randValue` after the tick | `rv = rv * 3 + 1013904223` (wrapping `i32`) once per `randomPos` call, `pos = (xo + (v & 15), yo + ((v >> 16) & 15), zo + ((v >> 8) & 15))` with `v = rv >> 2` ([`server-tick.md`](../ticks/server-tick.md) §10) |

`PIN_RNG` can re-pin either at any tick boundary (useful to realign after a long run or to start an experiment at a known
state); the header pin is applied at timeline tick 0. Vanilla's own initial values (a clock-derived seed and a thread-local random int) are
unrelated and never used. Other randomness a scenario can reach (an entity's own random, the loot/sound seeds) does not
move these two and is outside the dump; a block that draws from the level random (piston and torch sounds, fluid tie-breakers,
random ticks) does move `randomState`, which is how `piston-12` shows the draws.

### 3.3 Game rules the examples use

`advance_time=false`, `advance_weather=false`, `spawn_mobs=false`, `spawn_monsters=false`, `raids=false`, `random_tick_speed=0`
(set to a positive value only by scenarios that test random ticks). Anything else stays at its default (`fire_spread_radius_around_player`
128: fire logic is gated by a nearby player, so a scenario testing fire must set it to a negative value to disable the check). With
no player there is no natural spawning anyway, so the natural-spawn chunk shuffle draws nothing from the level random. Rule changes inside a
tick take effect from the next point the game reads them (`random_tick_speed` is read once per tick).

### 3.4 Chunk-ticking assumptions

- The chunk box is held by **forced-chunk tickets** (level 31): box chunks are *entity ticking*
  (random ticks, scheduled ticks, block events, block entities, entities), the one-chunk ring around the box is only
  *block ticking* (scheduled ticks, block events and block entities run, **random ticks do not**), the second ring is loaded but inert
  ([`server-tick.md`](../ticks/server-tick.md) §9). Zenith must mirror this: loaded set = box + 2 rings, random ticks = box.
  Keep scenario structures inside the box if they need random ticks; ring chunks also tick scheduled ticks, so a fluid can spill into them.
- **Random-tick chunk order** is the order the level's simulation tracker lists them; with the forced tickets added x-then-z ascending the oracle reports
  it in every dump (`chunkCount` + coordinates). Zenith's order (the fastutil-order rules of [`collection-orders.md`](../ticks/collection-orders.md)) must reproduce
  *that list* — the list in the output is the check. (If a scenario needs only one random-ticking chunk the question disappears.)
- Chunk contents at the start are the flat world (bedrock layer, air). The biome is plains, so a dry world has no ice / snow logic and `tickPrecipitation`
  draws (`nextInt(48)` per tick-speed step per chunk) still happen (they are part of the level-random trace).
- Light is part of the world: Zenith's light engine must reach the same values. The oracle waits for the vanilla light engine
  to settle before every tick body, after it, and before every dump (flag `light_sync`); changes of light *inside* one tick body can be seen by
  later phases of the same tick at an arbitrary stage in vanilla, so scenarios must not depend on that (keep light conditions static during the
  run, or change them only from operations and look at the next tick).
- Chunks never unload (forced tickets) and the autosave (every 6000 server ticks, including ~50 loading ticks) does not change observable
  state; scenarios longer than ~5900 ticks are fine.

## 4. Usage (the `xtask` side)

```bash
cargo xtask oracle scenario --example torch-clock          # 2 runs, byte-compare, print a summary of run 1
cargo xtask oracle scenario --example water-spread --runs 3 --out /tmp/water.zso
cargo xtask oracle scenario --file my.zsc --quiet          # any ZSC1 file you built
```

Examples (`SCENARIO_EXAMPLES`): `torch-clock`, `water-spread`, `piston-12`, `sand-fall`, `crop-random-tick`, `ops-smoke`.
The command exits non-zero when the runs differ or the timeline did not finish. Rust API (`xtask/src/oracle.rs`):

```rust
let ids = ScenarioIds::load("26.3")?;                       // state / block / fluid ids from tables.bin
let mut s = ScenarioSpec::new([4, 4, 5, 5], 40);            // chunk box, timeline ticks; recommended rules + RngPin::Seed(0)
s.op(0, Phase::Start, ScenarioOp::SetBlock { pos: [80, 1, 80], state: ids.state("water", &[("level", "0")])?, flags: 3 });
s.op(39, Phase::End, ScenarioOp::Dump { min: [70, 1, 70], max: [90, 1, 90], sections: SECTION_ALL });
let raw = run_scenario("26.3", &ScenarioWorld::default(), &s.to_bytes())?;   // fresh world per call, ~8 s
let out = parse_scenario_output(&raw)?;                     // ScenarioOutput { dumps: Vec<ScenarioDumpRecord>, status, .. }
```

`run_scenario` starts a fresh world and deletes the run folder after reading (set `ZENITH_KEEP_ORACLE_RUN=1` to keep the
server log in the user's own cache for debugging). It requires the EULA marker like the other oracle runs.
Agent arguments (when starting a server by hand): `-javaagent:zenith-oracle.jar=scenario=<in>,scenario_out=<out>,version=26.3`
(`scenario` is a *label* in the default tick-dump mode; it is a file path as soon as `scenario_out` is present; `out` and `ticks`
are then unused). The default ZPD tick dump and the `ZTC` chunk dump are unchanged.

Writing a Zenith-side test: build the same `ScenarioSpec`, apply it to the Zenith world exactly as in §1 (pin, then per timeline tick: Start ops → one engine
tick → End ops, producing the same dump records) and compare record by record with `first_scenario_difference`-style output: the first mismatching
field (game time, `randomState`, `randValue`, sub-tick counter, blocks, ticks, events, block entities) names the first divergence.

## 5. Observations from the built-in examples (26.3, checked on the owner's install)

- **Runs are deterministic**: every example is byte-identical across repeated runs (≥ 2 each). Entries of `ZSO1` contain nothing that depends on wall clock.
- `torch-clock` (wall torch + conductor + dust loop): the torch toggles every 2 ticks (period 4); each toggle schedules the next delay-2 torch tick
  at `trigger = now + 2`, `priority 0`, with a fresh sub-tick number (7 numbers consumed per cycle).
- `water-spread`: a source placed with flags 3 schedules a fluid tick at +5; flowing water of level *n* appears 5 ticks apart on the plane; level 7 is reached
  after 35 ticks, each ring leaves behind fluid ticks at the next multiple of 5.
- `piston-12`: a piston pushes the maximum 12 blocks; the moving-piston block entities exist from the event tick and finish two ticks later;
  the level random is advanced by the piston's sound draws (visible in `randomState`).
- `sand-fall`: block tick on placement, the falling entity appears next tick, and the sand lands about 17 ticks later (entity physics: entity milestone).
- `crop-random-tick`: with `random_tick_speed=3000` the level random advances by thousands of draws per tick and wheat ages by random ticks; the whole
  trace is reproducible only because both random sources are pinned (§3.2).

## 6. Limits and open points

- **Light inside a tick body** (above) is the one known race in vanilla itself; the oracle cannot remove it.
- **Identity-hash collections**: vanilla collections keyed by `Block` / `Fluid` objects iterate in identity-hash order, which differs between JVM runs.
  None showed up in the examples; if a scenario's output ever differs between runs, look for that first.
- Entities that scenarios create are not dumped yet (entity milestone); only the blocks, ticks, events and block entities they cause are.
- One timeline per server start (~8 s): batch as many dump points as possible into one scenario.
