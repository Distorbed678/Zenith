# Tick: fire (26.3)

- Verified by suites: `random-ticks`, `block-ticks`
- Sources: local reference reading of the 26.3 fire block classes (2026-11-26, reader session): the regular fire block with its burn / ignite tables, the shared fire base block and the soul fire block, the level's fire-spread-radius predicate, the player-distance helper, rain queries on `Level`, the difficulty enum, and the TNT prime hook.
  Everything is in our own words. The numeric table below is a set of rule constants (odds), not game data files.
- Related specs: [`tick-index.md`](tick-index.md), [`tick-growth.md`](tick-growth.md) (conventions), [`tick-decay-weathering.md`](tick-decay-weathering.md) §Inputs (`isRainingAt`), [`../server-tick.md`](../server-tick.md) §10 (random ticks do **not** drive fire: fire is a
  *scheduled* tick), [`../scheduled-ticks-and-block-events.md`](../scheduled-ticks-and-block-events.md), [`../set-block-flags-and-sequence.md`](../set-block-flags-and-sequence.md), [`../block-hooks.md`](../block-hooks.md), [`../../math/random.md`](../../math/random.md).

## Summary

Fire (`fire`) is **not randomly ticking**. It owns a scheduled tick of its own, created when it is placed (`onPlace`) and re-created at the start of every tick. Soul fire has **no tick at all** (it never ages, spreads or burns out by itself; only its survival hooks act).
Each fire tick: reschedules itself (one draw), may be extinguished by rain, ages, may burn out, may destroy flammable face neighbours, and may ignite nearby air. Every draw and its order matter; the ordering below is the code's.

## Inputs and state

- `age` 0..15 and five boolean face properties `north, east, south, west, up` (the faces attached to flammable neighbours when there is no sturdy floor).
- `random` = the level random (all draws). `Direction.values()` order = `[down, up, north, south, west, east]`.
- Game rule `fire_spread_radius_around_player` (int, default 128, `−1` = unlimited, minimum −1): `canSpreadFireAround(p)` = the rule is `−1`, or some **non-spectator** player has `distance(player position, (p.x, p.y, p.z)) < radius` (Euclidean, on the player's exact position vs the integer corner of `p` — **no +0.5**).
- `infiniburn` = the block tag named by the **dimension type** (`infiniburn` field; vanilla: a tag per dimension): `onInfiniburn = blockBelow ∈ tag`.
- `isRaining()`, `isRainingAt(p)`: `tick-decay-weathering.md` Inputs. `isNearRain(p)` = `isRainingAt(p) || isRainingAt(p + west) || isRainingAt(p + east) || isRainingAt(p + north) || isRainingAt(p + south)` in this order (short-circuit, no draws).
- `difficulty` = the level difficulty id: peaceful 0, easy 1, normal 2, hard 3.
- `increasedBurnout` = environment attribute `gameplay/increased_fire_burnout` at `p` (default false; vanilla data: see Open question 2).
- **Burn table** (per block): `igniteOdds(b)` (how easily the *neighbour of air* catches: used to spread into air) and `burnOdds(b)` (how easily the block is destroyed by adjacent fire). A block state with `waterlogged = true` always has both = 0. A block is **flammable** (`canBurn`) iff its `igniteOdds > 0`.
  Everything not in the table: both 0. The table (ignite, burn):

| (ignite, burn) | blocks |
|---|---|
| (5, 20) | planks, slabs, fences, fence gates, stairs of oak, spruce, birch, jungle, acacia, cherry, dark oak, pale oak, poplar, mangrove and bamboo wood types; bamboo mosaic, bamboo mosaic slab, bamboo mosaic stairs; mangrove roots; composter; beehive |
| (5, 5) | logs, stripped logs, woods, stripped woods of oak, spruce, birch, jungle, acacia, cherry, pale oak, poplar, dark oak, mangrove; bamboo block, stripped bamboo block; coal block |
| (30, 60) | all leaves (oak, spruce, birch, jungle, acacia, cherry, dark oak, pale oak, mangrove, red / orange / yellow poplar, azalea, flowering azalea); azalea, flowering azalea; dried kelp block; hanging roots; straw bed; all 16 wool blocks, wool stairs, wool slabs |
| (30, 20) | bookshelf, lectern, bee nest, the shelf blocks of all ten non-bamboo wood types and the bamboo shelf |
| (15, 100) | tnt, vine, glow lichen |
| (60, 100) | short grass, fern, dead bush, short / tall dry grass, sunflower, lilac, rose bush, peony, red shrub, tall grass, large fern, dandelion, golden dandelion, poppy, open / closed eyeblossom, blue orchid, allium, azure bluet, the four tulips, oxeye daisy, cornflower, lily of the valley, torchflower, pitcher plant, wither rose, pink petals, wildflowers, leaf litter, cactus flower, sweet berry bush, spore blossom, big dripleaf, big dripleaf stem, small dripleaf, firefly bush, bush |
| (60, 20) | hay block; all 16 carpets |
| (15, 20) | target block |
| (5, 100) | pale moss block, pale moss carpet, pale hanging moss |
| (60, 60) | bamboo (the plant), scaffolding |
| (15, 60) | cave vines, cave vines plant |

  (Which exact "wood-type" members exist is given by the block registry; doors, trapdoors, signs, buttons, pressure plates and the nether woods are **not** in the table.)

## Behavior (ordered rules)

### 1. Placement (`onPlace`)

1. (shared base, only if the old state was not fire): in a dimension that supports portals (overworld and nether): if the position can form an empty nether-portal frame, create it (hook; nothing is drawn here), and **stop** that part; else if the fire cannot survive (`canSurvive` false) → `removeBlock(p, false)`.
2. **Always** (even when step 1 removed the fire): `scheduleTick(p, fire, 30 + nextInt(10))` — one draw. The scheduling is subject to the "one tick per (block, pos)" rule, so a removed position schedules a tick for a block that is no longer there; the tick is discarded when it is due (scheduled-tick spec §3.3).
`canSurvive(p)` = the block below has a sturdy **top** face, or any of the 6 neighbours is flammable (`isValidFireLocation`). `getStateWithAge(p, age)` (also used by spread) = `soul fire` if the block below is in tag `soul_fire_base_blocks`, else a `fire` state with its face properties: if the block below is **not** flammable **and** not top-sturdy, each face property (`north`, `east`, `south`, `west`, `up`) = the neighbour on that side is flammable; otherwise all faces false; then `age = age` (fire only).

### 2. Scheduled tick (`tick`) — in this order

1. `scheduleTick(p, fire, 30 + nextInt(10))` — **always the first draw**, before every test below.
2. If `canSpreadFireAround(p)` is false → **stop** (the fire neither ages nor spreads while no player is within the radius; it keeps rescheduling).
3. If `!state.canSurvive(level, p)` → `removeBlock(p, false)`. **Execution continues** (no return): the rest of the tick uses the old `state` / `age` and may still act, although the fire is gone.
4. `onInfiniburn`; `age = state.age`.
5. **Rain extinguish**: if `!onInfiniburn && isRaining() && isNearRain(p)` and then a draw `nextFloat() < 0.2f + age · 0.03f` (f32: `0.2f + (float)age · 0.03f`; the draw only when the three previous tests hold): `removeBlock(p, false)`; the tick ends. If any of those tests fail (including the draw), continue with step 6.
6. **Aging**: `newAge = min(15, age + nextInt(3) / 2)` (one draw `nextInt(3)` ∈ {0,1,2}; integer division: 0, 0, 1). If `newAge ≠ age`: `state = state with age = newAge`; `setBlock(p, state, flags 260)`. (The local variable `age` keeps the **old** age for all later formulas.)
   **Hidden draw:** this write changes the state, and flag 512 is not set, so the new state's `onPlace` hook runs (`set-block-flags-and-sequence.md` §3.10 runs it for property-only changes too). Fire's `onPlace` (§1 step 2) draws `nextInt(10)` and requests a scheduled tick; the request is **discarded** because the tick of step 1 is already pending,
   but the draw is consumed. So an aging tick draws `nextInt(3)` **then** `nextInt(10)` back to back.
7. If `!onInfiniburn`:
   a. If `!isValidFireLocation(p)` (no flammable neighbour): `below = p − up`; if the block below does **not** have a sturdy top face **or** `age > 3`: `removeBlock(p, false)`. **Return** in every case.
   b. If `age == 15` and then a draw `nextInt(4) == 0` (only when age is 15) and the block below is **not** flammable: `removeBlock(p, false)`; **return**.
8. `extra = increasedBurnout ? −50 : 0`.
9. **Burn-out of face neighbours**, six calls in this order, each `checkBurnOut(position, chance, age)`:
   `east: 300 + extra`, `west: 300 + extra`, `down: 250 + extra`, `up: 250 + extra`, `north: 300 + extra`, `south: 300 + extra`.
   `checkBurnOut(q, chance, age)`:
   1. `odds = burnOdds(state at q)`; draw `nextInt(chance)` (**always**, even when `odds = 0`; `chance` is positive for the standard values); if `value < odds`:
   2. `oldBlock = block at q`. Draw `nextInt(age + 10)`; if `value < 5` **and** `!isRainingAt(q)`: `newAge = min(age + nextInt(5) / 4, 15)` (second draw `nextInt(5)`, only in this branch; `/4` is nonzero only for 4); `setBlockAndUpdate(q, getStateWithAge(q, newAge))` (the neighbour becomes fire / soul fire);
      else (`value ≥ 5`, or it rains at `q`): `removeBlock(q, false)`.
   3. If `oldBlock` was **TNT**: prime it (`TntBlock.prime(level, q)`: entity hook; the TNT block itself was already replaced in step 2).
10. **Spread into air**: for `xx = −1..1` (outer), `zz = −1..1` (middle), `yy = −1..4` (inner), skipping `(0,0,0)`:
    - `rate = 100`; if `yy > 1`: `rate += (yy − 1) · 100` (so `yy = 2 → 200`, `3 → 300`, `4 → 400`).
    - `t = p + (xx, yy, zz)`; `ig = igniteOdds around t`: **0 unless the block at `t` is air**; else the **maximum** over the six neighbours of `t` (in `Direction.values()` order; order irrelevant for a max) of their `igniteOdds` (waterlogged → 0).
    - If `ig > 0`: `odds = (ig + 40 + difficulty · 7) / (age + 30)` (integer division, old age); if `increasedBurnout`: `odds /= 2`.
      If `odds > 0` **and** (draw `nextInt(rate) ≤ odds`) **and** (`!isRaining()` **or** `!isNearRain(t)`): `spreadAge = min(15, age + nextInt(5) / 4)` (draw `nextInt(5)`); `setBlockAndUpdate(t, getStateWithAge(t, spreadAge))`.
      The `nextInt(rate)` draw happens exactly when `ig > 0` **and** `odds > 0`; the `nextInt(5)` draw only after the three conditions hold.
    Each spread write is a full flags-3 write that re-enters the world pipeline (shape updates, neighbour notifications, `onPlace` → a new scheduled tick for the new fire with a fresh `30 + nextInt(10)` draw) **before the next loop iteration**: the draws of newly placed fires interleave into this stream.

### 3. Other facts the fire relies on

- **Every state-changing write of a fire block runs `onPlace`** and therefore draws `30 + nextInt(10)` once (§1 step 2): aging (step 6), face-property recomputation by the `updateShape` hook (`getStateWithAge`), spread writes into air (new fire blocks), burn-out writes that turn a flammable block into fire. A write that leaves the state identical does nothing (no draw).

- **Soul fire** (`soul_fire`): no scheduled tick, no random tick. `canSurvive`: block below in `soul_fire_base_blocks`. It is replaced by regular fire only through `getStateWithAge` (spread). Entities inside it burn longer (entity hook).
- `FireBlock.updateShape` (reaction file): `canSurvive ? getStateWithAge(p, state.age) : air` — recomputes the face booleans.
- `entityInside`: clears freeze and ignites the entity (entity hook; players get `nextInt(1, 3)` extra fire ticks from the level random — one draw `1 + nextInt(2)`).
- `lightning bolt` strikes ignite fire around the strike (entity hook).
- Water extinguishing (water placement) is a fluid / replaceable-block rule (`fluids.md`).

## Edge cases and known quirks

0. **Hidden `onPlace` draws** (see §3 first bullet): aging adds one `nextInt(10)`; every new fire adds one. Parity logs must count them.
1. **First draw first.** The reschedule draw (`30 + nextInt(10)`) happens before the player-distance test; a fire far from every player consumes one draw per tick and nothing else.
2. **After a failed survival check the tick keeps going** (step 3 has no return). The stale `state` is used; subsequent steps may remove blocks, write neighbours and draw.
3. **`age == 15` branch** (step 7b) draws only when the age is 15, and the "block below is flammable" test comes **last**, after the draw.
4. **Burn-out checks always draw** `nextInt(chance)` for each of the six neighbours, even air (so 6 guaranteed draws per full fire tick unless it returned earlier).
5. **Spread loop draws** `nextInt(rate)` only for air positions that have a flammable neighbour (the `odds > 0` test excludes cases where the integer division yields 0: `(ig + 40 + 7d) < age + 30`, possible for high ages and low ignite odds).
6. **Increased burnout** (attribute) changes both the burn-out chances (`−50`) and halves the spread odds.
7. **Rain tests** use the *current* `isRaining()` (global weather) and `isRainingAt` (needs sky access, height-map, biome precipitation = rain).
8. **Write flags**: aging uses 260 (no neighbour update, no block-entity side effects, shape updates still run); all other writes are `setBlockAndUpdate` (3) or `removeBlock` (3).
9. **Fire on an infiniburn block** is never rain-extinguished and never burns out for lack of fuel (steps 5 and 7 are skipped); it still ages, burns face neighbours and spreads, and it is still removed by step 3 when it cannot survive.

## Constants and IDs

Reschedule delay `30 + nextInt(10)` (30..39); aging `min(15, age + nextInt(3)/2)`; rain extinguish `0.2f + age · 0.03f`; age-15 burn-out `1/4`; burn-out chances 300 (E, W, N, S) / 250 (down, up) (minus 50 with increased burnout);
burn-out removal vs spread: `nextInt(age + 10) < 5`; spread loop `xx, zz ∈ [−1, 1]`, `yy ∈ [−1, 4]`, `rate = 100 + max(0, yy − 1) · 100`, `odds = (ig + 40 + 7 · difficulty) / (age + 30)`. `max age` 15. Level event / sound ids are not part of the world state.
`fire_spread_radius_around_player` default 128.

## Oracle probe points

- `net.minecraft.world.level.block.FireBlock#tick(BlockState, ServerLevel, BlockPos, RandomSource)` (protected; reflection / agent), `#checkBurnOut` (private), `#getIgniteOdds(LevelReader, BlockPos)` (private), `#getBurnOdds`/`#getIgniteOdds(BlockState)` (private; read the two `Object2IntMap` fields `igniteOdds` / `burnOdds` after `FireBlock.bootStrap()` to confirm the table).
- `ServerLevel#canSpreadFireAround(BlockPos)`; `Level#isRainingAt(BlockPos)`; `BaseFireBlock#getState(BlockGetter, BlockPos)` (public static).
- A counting `RandomSource` proxy per fire tick (`(draw index, bound)` list) is the dump format.

## Open questions

1. **[oracle] Burn table completeness.** Dump `FireBlock.igniteOdds` / `burnOdds` (both `Object2IntMap<Block>`, private; reflection) and compare with the table above (the wool / carpet families are 16 colours each; stairs and slabs of wool are included).
2. **[oracle] Increased-burnout attribute and `infiniburn` tags per dimension.** Probe `EnvironmentAttributes.INCREASED_FIRE_BURNOUT` at a position in each vanilla dimension; `DimensionType#infiniburn()` for each. (Expected from vanilla data: tags `infiniburn_overworld` / `infiniburn_nether` / `infiniburn_end`; contents are registry data.)
3. **[oracle] `age` after a removed fire.** Step 3 keeps executing after `removeBlock`: confirm with a draw log that a fire without support still performs steps 5–10 in the same tick (e.g. in an area with flammable neighbours the survival check passes anyway; the case needs a fire whose `canSurvive` is false while still existing, e.g. after a piston move).
4. **[oracle] Portal-frame creation at `onPlace`.** The exact frame search (`PortalShape.findEmptyPortalShape`) belongs to the nether-portal spec; it consumes no random draws.
5. (implementer) Players' "within radius" check uses the player entity position at the time of the tick (entity milestone input).

## Parity status

Not yet run.
