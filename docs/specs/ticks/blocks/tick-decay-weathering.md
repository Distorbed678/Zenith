# Tick: decay and weathering — leaves, ice and snow, farmland, coral, copper, weather-driven changes (26.3)

- Verified by suites: `random-ticks`, `block-ticks`
- Sources: local reference reading of the 26.3 block classes (2026-11-26, reader session): leaves, ice, frosted ice, snow layer, farmland, the coral block / plant / fan / wall-fan family, the generic "change over time" interface and the copper weathering interface with every weathering copper block class, cauldron blocks,
  composter, the level's precipitation tick, the biome temperature / freeze / snow predicates, the block-scan helpers, `Level` weather queries and the environment-attribute registry. Everything is in our own words.
- Related specs: [`tick-index.md`](tick-index.md), [`tick-growth.md`](tick-growth.md) (conventions, light notation, flags), [`tick-spread.md`](tick-spread.md) (box scans, shuffle), [`../server-tick.md`](../server-tick.md) §10–§12 (random-tick selection, precipitation roll, thunder),
  [`../scheduled-ticks-and-block-events.md`](../scheduled-ticks-and-block-events.md), [`../set-block-flags-and-sequence.md`](../set-block-flags-and-sequence.md), [`../block-hooks.md`](../block-hooks.md), [`../../worldgen/surface-and-carvers.md`](../../worldgen/surface-and-carvers.md)
  (biome temperature with height adjustment), [`../../blocks/state-facts.md`](../../blocks/state-facts.md) (§2.10 randomly-ticking states, §3.2 light dampening), [`../../math/random.md`](../../math/random.md).

## Summary

Blocks that disappear, melt, dry out, die or oxidise over time. All of them react to a **random tick** or to a **scheduled tick** requested by a neighbour / placement hook; none of them looks at entities. For each block this file lists the exact conditions, the order of reads and draws, the writes and
their flags. Conventions are those of [`tick-growth.md`](tick-growth.md) *Inputs and state* (level `random`, `raw`, `local`, flags 2 / 3 / 260, left-to-right short-circuit). The weather-driven formation of ice and snow and cauldron filling are specified here from the block side; the
roll that triggers them is `server-tick.md` §10–§11.

## Inputs and state

- **Block light** `blockLight(p)` = the visible block-light layer value at `p` (`getBrightness(BLOCK, p)`); **light dampening** of a state = `state-facts` §3.2 (ice, frosted ice, snow layers: `1` for the full-cube ice blocks, leaves always 1).
- **Environment attributes** read here (positional, data-driven; the attribute system itself is outside this spec, Open question 1): `gameplay/water_evaporates` (bool, default false — vanilla data enables it in the Nether-type dimension), `gameplay/increased_fire_burnout`
  (bool; fire spec), `gameplay/turtle_egg_hatch_chance` (f32 in [0,1], default 0.002), `gameplay/eyeblossom_open` (tri-state), `gameplay/creaking_active`, `gameplay/nether_portal_spawns_piglins`. Values come from the dimension type / biome data and the world clock; Zenith reads them from the installed data.
- **Weather**: `isRaining()` (rain level > 0.2), `isRainingAt(p)`: raining **and** `canSeeSky(p)` (sky light at `p` is 15) **and** the `MOTION_BLOCKING` height-map value at the column is not above `p.y` (i.e. `p.y ≥` the top solid-or-liquid block height; the heightmap position's Y is "first free Y") **and** the biome's precipitation at `p` is rain
  (`Biome.getPrecipitationAt`: none if the biome has no precipitation; otherwise snow if its height-adjusted temperature at `p` is `< 0.15f` else rain; `surface-and-carvers.md`). Its sibling `precipitationAt(p)` returns none / rain / snow by the same tests.
- `isWaterAt(p)` = the fluid state at `p` is in tag `water`.
- `Block.pushEntitiesUp(old, new, level, pos)`: if the new state's collision shape is larger than the old one, entities overlapping the new part are moved up; a hook of the entity milestone (no random draws).
- `dropResources(state, level, pos)` = the block's loot table evaluated with the level random (entity milestone / loot milestone). **The loot evaluation consumes draws from the level random**; the counts are table dependent (Open question 2).
- `removeBlock(p, false)` = `setBlock(p, legacyBlock(fluid at p), 3)`; `destroyBlock(p, drops)` as in `set-block-flags-and-sequence.md` §1.3.

## Behavior (ordered rules)

### 1. Leaves (all leaf blocks, including azalea, cherry, mangrove, pale oak, poplar)

Properties: `distance 1..7` (default 7), `persistent` (default false; set true by player placement through the placement hook), `waterlogged`.
**Randomly ticking** iff `distance == 7 && !persistent` (state-facts §2.10).
**Random tick**: if `decaying` (`!persistent && distance == 7`): `dropResources(state, level, pos)` (loot), then `removeBlock(pos, false)`. No other draws of its own.
**Scheduled tick** (delay **1**, requested by the leaves' `updateShape` hook when `distanceFromNeighbour + 1 ≠ 1 || own distance ≠ distanceFromNeighbour + 1` — i.e. whenever a neighbour change could alter the distance; the request is the reaction file's): recompute
`newDistance = 7`; for `d` in `Direction.values()` order (`down, up, north, south, west, east`): `newDistance = min(newDistance, distanceAt(neighbour) + 1)`; stop as soon as it is 1. `distanceAt(state)` = 0 if the state is in tag `prevents_nearby_leaf_decay` (logs), else its `distance`
property if it has the **leaves** `distance` property (range 1..7; scaffolding's `distance` is a different property and does not count), else 7. Then `setBlockAndUpdate(pos, state with distance = newDistance)` (flags 3; an unchanged state is a no-op write).
So decay is a two-stage process: a tree's logs vanish → shape updates schedule leaf ticks → distances rise by one per propagation step (each step is a separate scheduled tick with delay 1) → leaves reaching 7 start random-ticking → each is removed on its first random tick.

### 2. Ice (`ice`), frosted ice, snow layer

**Ice** (only the plain `ice` block ticks; packed and blue ice do not): randomly ticking always. **Random tick**: if `blockLight(p) > 11 − lightDampening(state)` (dampening 1 → `blockLight > 10`): `melt`:
 if `gameplay/water_evaporates` at `p`: `removeBlock(p, false)`; else `setBlockAndUpdate(p, water default)`, `neighborChanged(p, water, no orientation)` (the level-wide neighbour notification for the position; see `neighbor-and-shape-updates.md`) and game event `block_destroy` (old state). No draws.
**Snow layer** (`snow`, layers 1..8): randomly ticking always. **Random tick**: if `blockLight(p) > 11`: `dropResources(state)`, `removeBlock(p, false)`, game event `block_destroy`.
**Frosted ice** (age 0..3): *not* randomly ticking; **scheduled** only.
- `onPlace` — runs on placement **and on every state-changing write, including each aging step** (`set-block-flags-and-sequence.md` §3.10): draw `nextInt(61)` and `scheduleTick(p, block, 60 + that)` (= `Mth.nextInt(random, 60, 120)`). The draw is always made; the request is discarded when a tick for `(frosted ice, p)` is already pending.
- **Scheduled tick** (`tick`; while it runs, its own `(block, pos)` entry is no longer pending, so a request made from inside is accepted):
  1. Draw `nextInt(3)`. If it is **not** 0, evaluate `fewerNeighboursThan(p, 4)`: count the six face neighbours that are frosted ice, stopping when the count reaches 4; true if fewer than 4. If the draw **was 0** the count is skipped. If neither (draw ≠ 0 **and** ≥ 4 neighbours) → go to step 4.
  2. `brightness`: in the End dimension `blockLight(p)`; otherwise `local(p)` (with sky darkening). If `brightness > 11 − age − lightDampening(state)` **and then** `slightlyMelt(state, p)` returns true → step 3. (If the light test fails, `slightlyMelt` is **not** called; if it is called and returns false because the block merely aged → step 4.)
  3. For each `d` in `Direction.values()` order: `n = p + d`; if the block at `n` is frosted ice **and `slightlyMelt(neighbour, n)` returns false**: `scheduleTick(n, block, nextInt(20, 40))` (= `20 + nextInt(21)`, one draw each). **Return** (this block was melted by step 2 and is not rescheduled).
  4. `scheduleTick(p, block, nextInt(20, 40))` (one draw: `20 + nextInt(21)`).
- `slightlyMelt(state, p)`: if `age < 3`: `setBlock(p, age + 1, flags 2)` — which runs `onPlace` (draw `nextInt(61)`, request accepted if nothing pending at `p`) — and return false; else `melt` (below) and return true.
  **Consequence (verify: Open question 3):** after an aging step the `onPlace` request (60..120 ticks) is accepted first, so the **later** request of step 4 / 3 (20..40 ticks) is discarded as a duplicate (its draw is still made): an aged frosted-ice block next ticks after `60..120` ticks, not `20..40`. A block whose age is unchanged (steps 1 / 2 failed) takes the `20..40` delay.
  Draw sequence of one aging tick (typical): `nextInt(3)` [step 1], `nextInt(61)` [onPlace], `nextInt(21)` [step 4]. Every neighbour aged in step 3 adds `nextInt(61)` then `nextInt(21)`.
- `neighborChanged` (hook): when a *frosted ice* block neighbour notifies and `fewerNeighboursThan(p, 2)`: `melt`. (Reaction file; stated here because it removes pending melts.)
Frosted ice is created by the frost-walker hook (`onPlace` above schedules the first tick).

### 3. Farmland

`moisture 0..7`, randomly ticking always. Helpers: `nearWater(p)` = any position in the box `p + (−4, 0, −4)..p + (4, 1, 4)` whose **fluid** state is in tag `water` (waterlogged blocks count); `maintained(p)` = block above `p` is in tag `maintains_farmland`.
**Random tick** (no draws):
1. `m = moisture`. If **not** `nearWater(p)` **and** **not** `isRainingAt(p + up)`:
   - if `m > 0`: `setBlock(p, moisture = m − 1, flags 2)`;
   - else if not `maintained(p)`: `turnToBaseBlock` (below);
2. else if `m < 7`: `setBlock(p, moisture = 7, flags 2)`.
**Scheduled tick** (delay 1, requested by the `updateShape` hook for direction up when the block above is solid): if `!canSurvive` (`above` is a legacy-solid block and not `maintained(p)`): `turnToBaseBlock`.
`turnToBaseBlock(state, p)`: `newState = pushEntitiesUp(state, dirt default, level, p)` (returns the dirt state); `setBlockAndUpdate(p, newState)`; game event `block_change`. (Trampling is an entity hook: `fallOn` draws one `nextFloat` from the level random per landing.)

### 4. Coral (blocks, plants, fans, wall fans)

Corals die when no water is adjacent. All dead variants are 1:1 per live block.
- **Coral block** (`coral_block` types): `scanForWater(p)` = any of the six neighbours has a fluid in tag `water`. **Scheduled tick**: if not `scanForWater` → `setBlock(p, dead coral block default, flags 2)`. Scheduling (reaction hook, listed for the random draw): on a shape update, if not `scanForWater`: `scheduleTick(p, this, 60 + nextInt(40))` (one draw of the shape-update's random source); `getStateForPlacement` does the same with the level random.
- **Coral plant, coral fan, coral wall fan**: `scanForWater(state, p)` = the state's `waterlogged` is true **or** any of the six neighbours has water fluid. **Scheduled tick**: if not → `setBlock(p, dead variant default with waterlogged = false (wall fans: also the same facing), flags 2)`. Scheduling: `onPlace` and `updateShape` (the latter for any direction except a failing support below, which removes the block instead) call `tryScheduleDieTick`: if not `scanForWater` → `scheduleTick(p, this, 60 + nextInt(40))`.
- `onPlace` of plants and fans runs on **every state-changing write** (a waterlogging change, for instance), so such a write draws `nextInt(40)` as well when no water is adjacent (a request is discarded if one is pending). The delay is `60..99` ticks. The scheduled-tick rule "one tick per `(block, pos)`; a later request while one is pending is discarded" means repeated shape updates do not extend the delay, but each request **still draws** from the random source.

### 5. Copper weathering (15 families)

Unwaxed copper blocks age; the families (block, cut, chiseled, cut stairs, cut slab, door, trapdoor, bars, grate, bulb, lantern, chest, golem statue, lightning rod, chain) each have four unwaxed stages `unaffected → exposed → weathered → oxidized` and four waxed ones (waxed blocks are plain blocks with no tick).
**Randomly ticking** iff the block has a next stage (oxidized is not randomly ticking).
**Random tick** = `changeOverTime(state, p)` for all families, **except**:
- *doors*: only when `half == lower` (the upper half's random tick does nothing, no draw);
- *chests*: only when the chest `type ≠ right` **and** the block entity has no open containers (no viewers); otherwise nothing, no draw;
- (other families: unconditional).
**`changeOverTime(state, p)`**:
1. Draw `nextFloat()`; if `value < 0.05688889f` (this is "every block oxidises about once per day") continue, else stop.
2. `getNextState`: `ownAge` = this block's stage ordinal (0..3). Scan every position `q ≠ p` with Manhattan distance `|dx| + |dy| + |dz| ≤ 4` of `p` (the order is irrelevant for the result); consider only blocks that are *weathering copper blocks of any family* (waxed ones excluded); for each: `a` = its stage ordinal:
   - if `a < ownAge`: **abort** (no change this tick; the scan stops; **no second draw**);
   - else if `a > ownAge`: `older += 1`; else `same += 1`.
3. `chance = f32(older + 1) / f32(older + same + 1)` (int sums, f32 division of the two converted values); `actual = chance · chance · modifier` where `modifier = 0.75f` if `ownAge == 0` (unaffected) else `1.0f` (f32 left to right).
4. Draw `nextFloat()`; if `value < actual` → the next stage's state (same properties copied, i.e. `withPropertiesOf`) → `setBlockAndUpdate(p, next)` (flags 3). The second draw happens iff the scan did not abort.
Notes: the neighbourhood counts blocks of all copper families, not just the same family. Waxed copper, via honeycomb, and scraping with an axe or lightning are interaction / entity hooks (`WeatheringCopper.getFirst / getPrevious / getNext` maps stage ↔ waxed; one-to-one by block id).

### 6. Weather-driven block changes (the roll is `server-tick.md` §10–§11)

`tickPrecipitation(p)` — after the `random.nextInt(48) == 0` roll and one `randValue` position draw. Block-side effects in order:
1. `top` = `(p.x, firstFreeY, p.z)` from the live `MOTION_BLOCKING` height-map; `below = top − up`; `biome` = the biome at `top`.
2. **Freeze**: if `biome.shouldFreeze(level, below)`: `setBlockAndUpdate(below, ice default)` (flags 3). `shouldFreeze(below)`: not `warmEnoughToRain(below)` (height-adjusted temperature at `below` `≥ 0.15f` means warm); and `below.y` inside the build height; and `blockLight(below) < 10`; and the block at `below` is a **liquid block** (`LiquidBlock`) whose fluid is water (so waterlogged blocks do not freeze); and **not** all four horizontal neighbours of `below` have a water fluid (`isWaterAt`) — a water block surrounded by water on four sides stays liquid.
3. If `isRaining()`:
   a. `maxH = game rule max_snow_accumulation_height` (0..8, default 1). If `maxH > 0` and `biome.shouldSnow(level, top)`: (`shouldSnow`: precipitation at `top` is snow; `top.y` inside build height; `blockLight(top) < 10`; the state at `top` is air or a snow layer; and a default snow-layer state at `top` can survive: below is not in `cannot_support_snow_layer`, and below is in `support_override_snow_layer`
      or its collision shape has a full top face or it is a snow layer with 8 layers):
      read `s` = state at `top`; if `s` is snow: `layers = s.layers`; if `layers < min(maxH, 8)`: `new = layers + 1`; `pushEntitiesUp(s, new)`; `setBlockAndUpdate(top, new)`; else (no snow): `setBlockAndUpdate(top, snow default)` (1 layer).
   b. `precip = biome.getPrecipitationAt(below, seaLevel)`; if not none: `handlePrecipitation(state at below, below, precip)` of the block there:
      - **empty cauldron**: `shouldHandle(precip)`: rain → draw `nextFloat() < 0.05f`; snow → draw `nextFloat() < 0.1f` (exactly one draw). If true: rain → `water_cauldron` (level 1), snow → `powder_snow_cauldron` (level 1); `setBlockAndUpdate`; game event `block_change` (no source).
      - **water / powder-snow cauldron** (layered, precipitation type rain / snow respectively): the draw `shouldHandle(precip)` (one `nextFloat` as above, **always made first**, even for a full cauldron or the other type), and then `level < 3` and `precip == this type`: `setBlockAndUpdate(p, level + 1)`; game event `block_change`.
      - lava cauldron and every other block: no effect, no draw.
   There are no other draws in this function. The two draws of cauldron rain handling use the level random after the `nextInt(48)` (and nothing else is drawn between them).

### 7. Cauldron fill by dripstone (scheduled tick; the schedule comes from `tick-gravity-and-misc.md` §5)

`tick` of any cauldron block: `tip = findStalactiteTipAboveCauldron(p)` (the first *free-hanging stalactite tip* found going up from `p` through at most 10 blocks, each of which must let a drip through; see misc §5 `canDripThrough`); if found: `fluid = getCauldronFillFluidType(tip)` (the fluid at the root of that stalactite
if it is water or lava, mud counts as water unless `water_evaporates`; else empty). If `fluid ≠ empty` and the cauldron can receive it: `receiveStalactiteDrip`:
- empty cauldron: water → `water_cauldron` (level 1) + level event 1047; lava → `lava_cauldron` + level event 1046;
- water cauldron (rain type): if `level < 3`: `level + 1`, level event 1047 (powder snow and lava cauldrons never receive: `canReceive` is false);
all with `setBlockAndUpdate` and game event `block_change`. No draws.

### 8. Composter (scheduled)

`level == 7` states schedule themselves (`onPlace`, which also runs on every level change, and again after filling — a duplicate request is discarded) with delay **20**. **Scheduled tick**: if `level == 7`: `setBlockAndUpdate(p, level = 8)` (cycle) and play the "composter ready" sound. No random draws.

## Edge cases and known quirks

1. **Leaf decay is distance-driven and slow**: the distance propagates **one block per tick** because each hop is a separate scheduled tick; leaves inside a decaying canopy can sit at distance 7 and start random-ticking while their neighbours still carry stale smaller values.
2. **Ice melting ignores sky light**; frosted ice uses sky-darkened raw light (`local`) outside the End: at night a frosted-ice block in daylight-lit areas melts more slowly.
3. **Frosted-ice neighbour aging**: a frosted-ice tick that melts a block also **ages** (or melts) every adjacent frosted-ice block — a chain reaction that is not preceded by any draw for the neighbours.
4. **Cauldron precipitation draw is consumed even for a full or wrong-type cauldron** (the static `shouldHandle` runs before the level / type checks).
5. **Farmland random tick draws nothing**; hydration reads water in a 9 × 2 × 9 box one layer above and the weather.
6. **Copper scan is sensitive to waxed neighbours**: waxed blocks are not part of the scan; unaffected blocks have a 0.75 factor; a block aborts whenever *any* younger copper block (any family) is within Manhattan 4.
7. **`changeOverTime` makes at most two draws per random tick** (1 when the first draw fails or the scan aborts).
8. **Coral shape updates draw on every request** even when the pending scheduled tick makes the new request a no-op.

## Constants and IDs

Light limits: ice `blockLight > 11 − dampening` (dampening 1 → `> 10`), snow layer `> 11`, frosted ice `> 11 − age − dampening` (End: block light, else local light), freeze / snow `< 10`. Probabilities: copper first draw `0.05688889f`, unaffected modifier `0.75f`; cauldron rain `0.05f`, snow `0.1f`. Delays: frosted ice first `60..120` (`60 + nextInt(61)`), reschedule `20..40`
(`20 + nextInt(21)`); coral `60 + nextInt(40)`; leaves 1; farmland 1; composter 20. Manhattan radius 4 (copper), farmland box `(−4, 0, −4)..(4, 1, 4)`, temperature threshold `0.15f`, snow layers max 8, `max_snow_accumulation_height` default 1. Flags: 2 (aging / coral / farmland), 3 (everything else here).
Level events: 1046 (lava fill), 1047 (water fill).

## Oracle probe points

- `net.minecraft.world.level.block.LeavesBlock#randomTick`, `#tick`; `IceBlock#randomTick`, `#melt`; `FrostedIceBlock#tick`, `#slightlyMelt`; `SnowLayerBlock#randomTick`; `FarmlandBlock#randomTick`; `CoralBlock#tick`, `BaseCoralPlantTypeBlock#tryScheduleDieTick`; `ChangeOverTimeBlock#changeOverTime`, `#getNextState` (default interface methods);
  `WeatheringCopper#getNext(Block)` for the stage map; `CauldronBlock#handlePrecipitation` / `LayeredCauldronBlock#handlePrecipitation`; `ServerLevel#tickPrecipitation(BlockPos)` (public); `Biome#shouldFreeze(LevelReader, BlockPos)`, `#shouldSnow`, `#getPrecipitationAt(BlockPos, int)`.
- `Level#isRainingAt(BlockPos)`, `Level#precipitationAt(BlockPos)`; `LevelReader#findBlocksInManhattan(BlockPos, int)`.

## Open questions

1. **[oracle] Environment attribute values.** `water_evaporates`, `turtle_egg_hatch_chance`, `eyeblossom_open`, `creaking_active`, `nether_portal_spawns_piglins`, `increased_fire_burnout`: read the final per-position value through `level.environmentAttributes().getValue(attribute, pos)` for several positions / times in each vanilla dimension to fix the data (time-of-day keyframes) before implementing the attribute system.
2. **[oracle] Loot draws at leaf / snow decay.** The loot-table evaluation for `dropResources` of oak leaves (sapling 1/20 style chances, sticks, apples), snow layers and other blocks consumes the level random: probe the number of draws per call of `Block#dropResources(BlockState, Level, BlockPos)` for oak / spruce / birch / jungle / azalea leaves with and without shears-less tool (empty tool), and for snow with 1..8 layers.
3. **[oracle] Frosted-ice `onPlace` rescheduling.** The spec reads: every aging write runs `onPlace`, which requests a `60..120`-tick tick (accepted) before the `20..40` request of the tick body (discarded). Probe: log `LevelTicks#schedule` calls (`(pos, delay, accepted)`) and the `nextInt` draws for one frosted-ice block under a torch across ages 0 → 3; and confirm the neighbour loop of §2.3 ages each neighbouring frosted ice exactly once per melting tick.
4. **[oracle] Copper scan inclusion.** Verify that the neighbour scan includes the **block entity** families (chests, golem statues) and that "same ageing type" holds across all 15 families (single enum class). Probe: place an exposed copper block with an unaffected copper chest within 3 blocks; the exposed block must never advance while the chest is within range.
5. **[oracle] `isRainingAt` heightmap condition.** `getHeightmapPos(MOTION_BLOCKING, pos).getY() > pos.getY()` — confirm the comparison uses the "first free Y" convention (equal means the position is the topmost free block and rain reaches it). Probe: rain at a position directly below a slab vs the free top block.
6. (implementer) The chunk-load upgrade of leaf distances and coral ticks (`UpgradeData`) is a storage / migration concern, not a tick rule.

## Parity status

Not yet run.
