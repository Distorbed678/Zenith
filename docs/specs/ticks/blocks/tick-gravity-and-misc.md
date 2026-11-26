# Tick: falling blocks, support timers, drips, eggs, amethyst, geysers and other timed blocks (26.3)

- Verified by suites: `random-ticks`, `block-ticks`
- Sources: local reference reading of the 26.3 block classes (2026-11-26, reader session): the falling-block base class with sand / concrete powder / anvil / dragon egg / suspicious sand-gravel, scaffolding, path block, hanging moss, big dripleaf and its stem, the speleothem base class with the pointed-dripstone and sulfur-spike subclasses, cauldron blocks (drip receive),
  turtle egg, sniffer egg, frogspawn, budding amethyst, dried ghast, eyeblossom and potted eyeblossoms, redstone ore, nether portal, command block, campfire (block and block entity), creaking heart (block and block entity), potent sulfur (block and block entity), the lightning bolt entity's block effects, the falling-block entity's spawn / landing code,
  container drop helper, `Mth` random helpers, `BlockPos` iteration helpers. Everything is in our own words.
- Related specs: [`tick-index.md`](tick-index.md), [`tick-growth.md`](tick-growth.md) (conventions), [`tick-decay-weathering.md`](tick-decay-weathering.md) (cauldron drip receive, `isRainingAt`), [`tick-spread.md`](tick-spread.md) (box scans, shuffle, direction order), [`tick-fire.md`](tick-fire.md), [`../server-tick.md`](../server-tick.md),
  [`../scheduled-ticks-and-block-events.md`](../scheduled-ticks-and-block-events.md), [`../set-block-flags-and-sequence.md`](../set-block-flags-and-sequence.md), [`../block-hooks.md`](../block-hooks.md), [`../fluids.md`](../fluids.md) (water / lava / bubble columns), [`../redstone-components.md`](../redstone-components.md),
  [`../../math/random.md`](../../math/random.md) (positional random), [`../../blocks/state-facts.md`](../../blocks/state-facts.md).

## Summary

The remaining time-driven blocks: those that fall, those that collapse when unsupported, the dripstone family (growth, dripping into cauldrons, falling stalactites), eggs and spawn that hatch, budding amethyst, dried ghasts, sulfur geysers, creaking hearts, plus small ones (path blocks, hanging moss, big dripleaf, eyeblossom, redstone ore, portal piglins, command blocks, campfires,
lightning bolts' block effects). Fluids, bubble columns, redstone and pistons are in their own files. Conventions follow [`tick-growth.md`](tick-growth.md) *Inputs and state*.

## Inputs and state

- `Direction.values()` = `[down, up, north, south, west, east]`; `nextIntBetweenInclusive(a, b)` = `a + nextInt(b − a + 1)`; `nextInt(origin, bound)` (two-argument) = `origin + nextInt(bound − origin)`; `Mth.nextInt(r, a, b)` = `a` if `a ≥ b` else `a + nextInt(b − a + 1)`;
  `Mth.randomBetween(r, min, max)` = `nextFloat() · (max − min) + min` (f32, one draw); `Mth.randomBetweenInclusive(r, min, max)` = `nextInt(max − min + 1) + min`.
- **`FallingBlock.isFree(state)`** = the state is air, or in tag `fire`, or a legacy liquid, or `canBeReplaced()` (the replaceable property).
- **Item drop at a block position** `Containers.dropItemStack(level, x, y, z, stack)` (used by campfire, composter, chest breaking, …): draws, in order: `nextDouble()` ×3 (x, y, z offsets: `floor(coord) + r · (1 − w) (+ w/2 for x and z)`, `w = 0.25`), then **per split part**: `nextInt(21)` (part size `+ 10`, the stack is split until empty), then `triangle(0.0, 0.11485000171139836)`
  for x velocity, `triangle(0.2, …)` for y, `triangle(0.0, …)` for z, where `triangle(mean, spread) = mean + spread · (nextDouble() − nextDouble())` — two `nextDouble` per component, evaluated left to right. Total per part: 1 + 6 draws.
- Game rules read: `entity_drops`, `mob_griefing`, `spawn_wardens`, `fire_spread_radius_around_player`.
- Game time `gameTime` (long) is the overworld clock value used by block-entity tickers' `% n` tests (server-tick §6.2).

## Behavior (ordered rules)

### 1. Falling blocks: sand, red sand, gravel, concrete powder (16), anvils (anvil, chipped, damaged), dragon egg, scaffolding (special), suspicious sand / gravel, stalactites

All `FallingBlock`s (sand, red sand, gravel, the 16 coloured concrete powders, the three anvils, the dragon egg) share:
- **`onPlace`** and **`updateShape`** (any direction): `scheduleTick(p, block, delay)` with `delay = 2` (**5 for the dragon egg**). (One scheduled tick per `(block, pos)`: a request while one is pending is discarded.)
- **Scheduled tick**: if `isFree(block at p − up)` **and** `p.y ≥ level.minY`: `FallingBlockEntity.fall(level, p, state)`: create the entity at `(p.x + 0.5, p.y, p.z + 0.5)` carrying the state (with `waterlogged` forced to false if the property exists); `setBlockAndUpdate(p, legacy block of the fluid at p)` (water if waterlogged / in water, else air; flags 3, **before** the entity is added);
  `addFreshEntity(entity)`. Anvils then call `falling(entity)`: `setHurtsEntities(2.0f, 40)`. Nothing is drawn. (The entity's own motion and landing are the entity milestone: on landing it is placed with `setBlockAndUpdate` if the target is replaceable and the block can survive and the block below is not free (concrete powder in water keeps solidifying), then the block's `onLand` hook runs; otherwise it
  drops as an item (`entity_drops`) after `onBrokenAfterFall`; timeouts: 100 ticks outside the world height range, 600 ticks in any case.)
- **Concrete powder** (hooks, listed for the scheduled interplay): `shouldSolidify` = the block it replaces holds water, **or** any of the six neighbours (the downward one only if the block itself holds water) has a water fluid and is not sturdy toward the block; `getStateForPlacement` and `updateShape` return the concrete state when `touchesLiquid`; `onLand` places the concrete when `shouldSolidify`.
- **Dragon egg** extra: teleport on use / attack (not time-driven): up to 1000 attempts; each attempt draws, in order, `nextInt(16) − nextInt(16)` (dx: two draws), `nextInt(8) − nextInt(8)` (dy: two draws), `nextInt(16) − nextInt(16)` (dz: two draws) — 6 draws per attempt from the level random; accept the first target that is air, whose block below is not air, inside the world border and the build height;
  then level event 2015 with the packed delta, `setBlock(target, state, flags 2)`, `removeBlock(p, false)`.
- **Suspicious sand / gravel** (`brushable`): `onPlace` and `updateShape`: `scheduleTick(p, block, 2)`. **Scheduled tick**: (1) if a brushable block entity exists: `checkReset` (below); (2) if `isFree(block at p − up)` and `p.y ≥ minY`: `FallingBlockEntity.fall` with `disableDrop()` (no item on failure).
  `checkReset` (block entity): if `brushCount ≠ 0` and `gameTime ≥ brushCountResetsAtTick`: `brushCount = max(0, brushCount − 2)`; if the integer `dusted` stage (a function of `brushCount`: `0` for 0, `1` for 1..2, `2` for 3..5, `3` for 6..9 — the *completion state*) changed: `setBlockAndUpdate(p, dusted = stage)`;
  `brushCountResetsAtTick = gameTime + 4`. Then if `brushCount == 0`: clear hit direction, reset timers; else `scheduleTick(p, block, 2)` (the block entity's brushing code schedules the first one with delay 2 too). Brushing itself (10 brushes, 10-tick cooldown, 40-tick reset time) is an interaction hook; completion drops the loot item using
  `nextInt(21) + 10` items split from the stack (level random, one draw) — the item-entity milestone.
- **Pointed dripstone / sulfur spike falling** (§5): not a `FallingBlock`; same entity spawn rule, see §5.
- **Scaffolding**: `onPlace` (server only) and `updateShape` (server only): `scheduleTick(p, scaffolding, 1)`. **Scheduled tick**: `d = getDistance(p)`: look at the block below: if it is scaffolding: `d = its distance`; else if its top face is sturdy: `d = 0` and **return that** (so `getDistance` returns 0 immediately); else `d = 7` initially.
  Then for each horizontal direction in order `[north, east, south, west]`: if the neighbour is scaffolding: `d = min(d, neighbour.distance + 1)`; stop early at 1. `bottom = d > 0 && block below is not scaffolding`. `new = state with distance = d, bottom = bottom`.
  If `new.distance == 7`: if the old state's distance was also 7: `FallingBlockEntity.fall(level, p, new)` (the scaffolding falls) else `destroyBlock(p, drop = true)`; else if `state ≠ new`: `setBlockAndUpdate(p, new)`. No draws. `canSurvive` = `getDistance < 7`.

### 2. Path block, hanging moss, big dripleaf stem, other "support timers"

- **Dirt path**: scheduled tick (delay 1, requested by the `updateShape` hook when the block above is solid): `turnToBaseBlock`: `pushEntitiesUp(state, dirt default)`; `setBlockAndUpdate(p, dirt)`; game event `block_change`. (Same helper as farmland.) No condition re-check at the tick.
- **Hanging moss** (pale hanging moss): scheduled tick (delay 1 from `updateShape` when it cannot stay): if it cannot stay (block above has no full bottom face and is not the same block): `destroyBlock(p, true)`.
- **Big dripleaf stem**: scheduled tick (delay 1 from `updateShape` for direction up or down when `!canSurvive`): if `!canSurvive`: `destroyBlock(p, true)`.
- **Amethyst clusters, sea pickles, lanterns, signs, fences, bars, walls, stairs, slabs, trapdoors, candles, campfires, seagrass, shelves, chains, light blocks, conduit, heavy core, decorated pot, mangrove roots, …**: their `updateShape` only schedules the **water fluid tick** when waterlogged (`../fluids.md`); they have no block tick of their own.

### 3. Big dripleaf tilt (scheduled)

`tilt` ∈ `none, unstable, partial, full`. Delays for the **next** step: unstable 10, partial 10, full 100 ticks; none → no schedule. Entry points (not time-driven, hooks): `entityInside` (an entity whose `onGround` and `y > p.y + 0.6875f`, tilt `none`, no neighbour redstone signal): `setTiltAndScheduleTick(unstable, no sound)`; a projectile hit: `setTiltAndScheduleTick(full, tilt-down sound)`.
`setTiltAndScheduleTick(state, p, tilt, sound)`: `setTilt` = `setBlock(p, tilt, flags 2)` and, if the new tilt causes vibration (`none`, `partial` and `full` do; `unstable` does not) and differs from the old one: game event `block_change` (no source entity); then if `sound` ≠ none: play it with pitch `Mth.randomBetween(level.random, 0.8f, 1.2f)` — **one `nextFloat` draw**;
then `scheduleTick(p, block, delay for the new tilt)`.
**Scheduled tick**: if the block has a neighbour redstone signal (`hasNeighborSignal(p)`): `resetTilt`; else by the current tilt: `unstable → setTiltAndScheduleTick(partial, tilt-down sound)`; `partial → setTiltAndScheduleTick(full, tilt-down sound)`; `full → resetTilt`; `none → nothing`.
`resetTilt(state, p)`: `setTilt(none)` (flags 2), then if the **old** `state`'s tilt ≠ none: play the tilt-up sound with a `Mth.randomBetween(0.8, 1.2)` pitch draw (one `nextFloat`). `neighborChanged` (hook): a signal resets the tilt (same helper).
So a leaf stepped on: unstable (t) → 10 ticks → partial → 10 ticks → full → 100 ticks → none. Total draws: the unstable step has none; partial 1; full 1; reset 1.

### 4. Dried ghast

`hydration 0..3`, `waterlogged`, `facing`. Randomly ticking in every state. **Random tick**: if (`waterlogged` or `hydration > 0`) and **no** scheduled block tick is pending for this block at `p`: `scheduleTick(p, block, 5000)` (no draw).
**Scheduled tick**: if `waterlogged`: if `hydration ≠ 3`: transition sound; `setBlock(p, hydration + 1, flags 2)`; game event `block_change`; else (`hydration == 3`): `removeBlock(p, false)` then spawn a baby happy ghast at the bottom centre of `p` with the block's facing as yaw (entity hook; the spawn sound has no draws).
Else (not waterlogged): if `hydration > 0`: `setBlock(p, hydration − 1, flags 2)`; game event. **The tick does not reschedule itself**: every hydration step needs another random tick to schedule the next 5000-tick delay.

### 5. Pointed dripstone and sulfur spike (speleothems)

Both extend one base class. State: `tip_direction` up / down, `thickness` ∈ `tip, tip_merge, frustum, middle, base`, `waterlogged`. **Both are randomly ticking in every state.** Constants: the "block to grow on" is the dripstone block (pointed dripstone) / the sulfur block (sulfur spike); maximum growth search length `7` (pointed dripstone) / `2` (sulfur spike).
Helpers: `isStalactite(s)` = a speleothem block with `tip_direction == down`; `isStalagmite(s)` = `up`; `isTip(s, includeMerged)` = thickness `tip` (or `tip_merge` if `includeMerged`); `isFreeHangingStalactite(s)` = stalactite, thickness `tip`, not waterlogged.
`isStalactiteStartPos(state, p)` = `isStalactite(state)` and the block above `p` is **not the same block** (the topmost stalactite block of a column).
`findBlockVertical(start, axisDirection, pathPredicate, targetPredicate, maxSteps)`: for `i = 1 .. maxSteps − 1`: move one block in the direction; if `targetPredicate(state)` → that position; if outside the build height or `!pathPredicate(pos, state)` → none. (So it examines at most `maxSteps − 1` blocks; start excluded.)
`findTip(state, p, maxLen, includeMerged)`: if `isTip(state, includeMerged)`: `p`; else search from `p` in the tip direction with path = "same block and same tip direction", target = `isTip(·, includeMerged)`, steps `maxLen`.
`canDripThrough(pos, state)`: air → true; `isSolidRender` → false; non-empty fluid → false; else the collision shape does **not** intersect the 4 × 16 × 4 pixel column centred in the block (`column(4, 0, 16)`).

**Random tick**:
- *Pointed dripstone*: `r1 = nextFloat()`; `maybeTransferFluid(state, p, r1)`; then the base `randomTick`.
- *Sulfur spike*: the base `randomTick` only.
- **Base `randomTick`**: draw `r2 = nextFloat()`; if `r2 < 0.011377778f` **and** `isStalactiteStartPos(state, p)`: `growStalactiteOrStalagmiteIfPossible(state, p)`.
**`maybeTransferFluid(state, p, r)`** (pointed dripstone only): proceed only if `r ≤ 0.17578125f` (the code's two comparisons collapse to this) and `isStalactiteStartPos`:
1. `info = fluidAboveStalactite(p, state)` = if not a stalactite: none; else find the **root** by searching upwards (start excluded, at most 10 blocks, path = same block and same tip direction, target = a block that is not this speleothem block): `root`; `above = root + up`; `fluid` = **water** if the block at `above` is `mud` and not `water_evaporates` at `above`, else the *type* of the fluid at `above` (so flowing water is a different type from source water: only **sources** match below); `sourceState` = the block state at `above`. If no root is found within range → none.
2. If `fluid == water`: `probability = 0.17578125f`; else if `fluid == lava`: `0.05859375f`; else stop. Require `r < probability`.
3. `tip = findTip(state, p, 11, includeMerged = false)`. If none → stop.
4. If `sourceState` is mud and `fluid` is water: `setBlockAndUpdate(above, clay)`, `pushEntitiesUp(mud, clay, above)`, game event `block_change`, level event 1504 at the tip (drip sound); **else** `cauldron = findFillableCauldronBelowStalactiteTip(tip, fluid)`: search downwards from `tip` (start excluded, path = `canDripThrough`, target = a cauldron block that `canReceiveStalactiteDrip(fluid)` — empty cauldron: any of water / lava; water cauldron: water; others none; 11 → at most 10 blocks);
   if found: level event 1504 at the tip; `delay = 50 + (tip.y − cauldron.y)`; `scheduleTick(cauldron, cauldron's block, delay)` (then the cauldron's tick fills it: `tick-decay-weathering.md` §7).
**`growStalactiteOrStalagmiteIfPossible(startState, p)`**: `canGrow(p)` — pointed dripstone: the block above `p` is the dripstone block **and** the block two above has a **water source** fluid state; sulfur spike: only the first condition. If true: `tipPos = findTip(startState, p, maxLen, false)`; if found: `tipState`; if `isFreeHangingStalactite(tipState)` and `canTipGrow(tipState, tipPos)`:
   draw `nextBoolean()`: **true** → `grow(tipPos, down)`; **false** → `growStalagmiteBelow(tipPos)`.
   - `canTipGrow(tipState, tipPos)`: `g = tipPos + tip direction`; the block at `g` has **no** fluid, and is air or an *unmerged* tip of the same block facing the opposite direction.
   - `grow(from, dir)`: `t = from + dir`; if `t` holds an unmerged tip of this block facing `opposite(dir)`: `createMergedTips`: the upper block becomes a stalactite `tip_merge`, the lower a stalagmite `tip_merge` (each `setBlockAndUpdate`, waterlogged by water fluid at the position, stalactite first); else if the block at `t` is air or the water block: `setBlockAndUpdate(t, speleothem(tip_direction = dir, thickness = tip, waterlogged = fluid at t is water))`.
   - `growStalagmiteBelow(tipPos)`: `q = tipPos`; for up to 10 steps: `q −= up`; `s` = block at `q`: if `s` has a non-empty fluid → stop; if `s` is an unmerged upward tip of this block and `canTipGrow(s, q)` → `grow(q, up)`, stop; if a speleothem could be placed pointing up at `q` (`isValidSpeleothemPlacement(q, up)`: the block *behind* `q` — i.e. below it — has a sturdy top face, or is an upward speleothem of the same block) and `!isWaterAt(q − up)` → `grow(q − up, up)`, stop;
     if `blocksStalagmiteScan(q, s)` (pointed dripstone: `!canDripThrough`; sulfur spike: never) → stop.
- Total draws per random tick: pointed dripstone: `r1`, `r2` (2), +1 (`nextBoolean`) when growth is attempted; sulfur spike: `r2` (1), +1.

**Scheduled tick** (`tick`; scheduled by `updateShape` with delay **2** for a stalactite (tip down) whose support is lost, **1** for a stalagmite (a stalactite with a pending tick ignores further requests)): if `isStalagmite(state)` and `!canSurvive`: `destroyBlock(p, drop = true)`; **else** `spawnFallingStalactite(state, p)`: `q = p`, `s = state`; while `isStalactite(s)`: `FallingBlockEntity.fall(level, q, s)` (as §1: the block becomes its fluid's legacy block, flags 3, then entity added); if `isTip(s, includeMerged = true)`: `size = max(1 + p.y − q.y, 6)`; `setHurtsEntities(1.0f · size, 40)`; stop; else `q −= up`, `s = block at q`.
   (A stalactite's tick does **not** re-check support: a scheduled tick still drops it even if the support came back.)
`canSurvive(state)` = `isValidSpeleothemPlacement(p, tip_direction)`: the block *behind* (opposite the tip) has a sturdy face toward the tip direction, or is a speleothem of the same block with the same tip direction. Thickness recalculation is the `updateShape` hook.
Falling stalactites are single `FallingBlockEntity`s per column block (all spawned in the same tick); the landing sound level events are 1045 (dripstone) / 1052 (sulfur).

### 6. Eggs and spawn

- **Turtle egg** (`hatch 0..2`, `eggs 1..4`). **Randomly ticking in every state.** **Random tick**: `shouldUpdateHatchLevel(p)`: `chance = environment attribute gameplay/turtle_egg_hatch_chance at p` (f32, default 0.002); true iff `chance > 0` **and then** a draw `nextFloat() < chance` (the draw only when `chance > 0`); and then `onSand(p)` (the block below is in tag `sand`) — **evaluated after the draw**.
  If both hold: if `hatch < 2`: crack sound with pitch `0.9f + nextFloat() · 0.2f` (**one draw**); `setBlock(p, hatch + 1, flags 2)`; game event `block_change`. Else (`hatch == 2`): hatch sound with the same pitch formula (**one draw**); `removeBlock(p, false)`; game event `block_destroy`; for `i` in `0..eggs−1`: level event 2001 (block break particles), create a turtle (entity milestone: age −24000, home position `p`, at `(p.x + 0.3 + 0.2 i, p.y, p.z + 0.3)`).
  Breaking by stepping / falling (entity hooks): `stepOn`: 1/100, `fallOn`: 1/3 (non-zombie) via `nextInt(randomness) == 0` from the level random after the `canDestroyEgg` tests; `decreaseEggs`: sound pitch `0.9f + nextFloat() · 0.2f` (one draw); `eggs ≤ 1` → `destroyBlock(p, false)`; else `eggs − 1` (flags 2), game event, level event 2001.
- **Sniffer egg** (`hatch 0..2`): **not** randomly ticking; scheduled only. `onPlace` — which runs on placement **and on every state-changing write** — : `boosted` = block below in tag `sniffer_egg_hatch_boost`; if boosted (server): level event 3009; `hatchTime = boosted ? 12000 : 24000`; game event `block_place`; then `delay = hatchTime / 3 + nextInt(300)` (**one draw**, evaluated after the game event) and `scheduleTick(p, block, delay)`.
  **Scheduled tick**: if `hatch ≠ 2`: crack sound with pitch `0.9f + nextFloat() · 0.2f` (**one draw**, before the write); `setBlock(p, hatch + 1, flags 2)` — the write runs `onPlace`, which draws `nextInt(300)` and schedules the next stage (accepted: nothing is pending). Else (`hatch == 2`): hatch sound (one draw); `destroyBlock(p, drop = false)`; spawn a baby sniffer at the block centre with yaw `Mth.wrapDegrees(nextFloat() · 360.0f)` (one draw from the level random; entity hook).
  Draw sequence of a stage-0 / 1 tick: `nextFloat`, `nextInt(300)`; of the last tick: `nextFloat`, `nextFloat`.
- **Frogspawn**: not randomly ticking. `onPlace`: `scheduleTick(p, block, nextInt(3600, 12000))` = `3600 + nextInt(8400)` (one draw). **Scheduled tick**: if `!canSurvive` (the block below must be in tag `supports_frogspawn` or hold water in tag `supports_frogspawn` fluids, and the fluid above `p` must be empty): `destroyBlock(p, false)`; else hatch: `destroyBlock(p, false)`, hatch sound (no draw), then `n = nextInt(2, 6)` = `2 + nextInt(4)` tadpoles; for each: `x = p.x + clamp(nextDouble(), 0.2f, 0.7999999970197678)`, `z = p.z + clamp(nextDouble(), 0.2f, 0.7999999970197678)` (the `0.2f` is the f32 constant widened to double), `yRot = nextInt(1, 361)` = `1 + nextInt(360)`; spawn position `(x, p.y − 0.5, z)` (entity hook; draws in the order x, z, yRot per tadpole).
  Falling-block entities inside destroy it (entity hook).

### 7. Budding amethyst

Randomly ticking always. **Random tick**: draw `nextInt(5)`; if 0: `d = VALUES[nextInt(6)]` (`[down, up, north, south, west, east]`); `g = p + d`; `s` = block at `g`. Next stage: if `s` is air or a **full-height water block** (the `water` block whose fluid is full: any source) → `small_amethyst_bud`; else if `s` is a small bud facing `d` → medium; medium facing `d` → large; large facing `d` → `amethyst_cluster`; else none.
If a stage: `setBlockAndUpdate(g, stage default with facing = d, waterlogged = (fluid at g is water))`. Two draws when the first succeeds (at most), none else. (Facing property compare: the bud must already point away in the same direction.)

### 8. Eyeblossom and potted eyeblossom, firefly bush

- **Open / closed eyeblossom** (randomly ticking always) and **scheduled** ticks: `tryChangingState(state, p)`: `shouldBeOpen = eyeblossom_open attribute at p` as tri-state with the block's own openness as the fallback for "undefined"; if equal to the block's current openness → false. Else: `setBlockAndUpdate(p, other type default)`; game event `block_change` (old state); particle: the **four draws** `lifetime = 0.5 + nextDouble()`; then `nextDouble() − 0.5`, `nextDouble() + 1.0`, `nextDouble() − 0.5` (velocity x, y, z in this order) — **drawn from the level random even though the particle is cosmetic**;
  then for every position in the box `p + (−3, −2, −3)..p + (3, 2, 3)` holding a block **in exactly the old state** (iteration order = the section-wise scan of `tick-spread.md` §Inputs: `y` outermost inside a section, then `z`, then `x`; sections ordered `x`, `z`, `y`): `dist = sqrt(distSqr(p, q))`; `delay = nextIntBetweenInclusive(trunc(dist · 5.0), trunc(dist · 10.0))` (one draw each); `scheduleTick(q, old block, delay)`. Return true → a switch sound plays (no draw). Random tick: `tryChangingState` (long sound) then nothing; scheduled tick: `tryChangingState` (short sound).
- **Potted open / closed eyeblossom**: randomly ticking always. **Random tick**: `shouldBeOpen` by the same attribute (fallback: its own openness): if different: `setBlockAndUpdate(p, opposite potted state)`; the particle draws of 4 `nextDouble`s as above (`spawnTransformParticle`); long sound. No neighbour scheduling.
- **Firefly bush**: randomly ticking (property) but **no random-tick behaviour**; ambient particles are client-only.

### 9. Redstone ore, nether portal, command block, campfire

- **Redstone ore / deepslate redstone ore**: randomly ticking iff `lit`. **Random tick**: `setBlockAndUpdate(p, lit = false)`. (Lighting up is `interact` on step / attack / use.) No draws.
- **Nether portal block**: randomly ticking always. **Random tick**: `isSpawningMonsters()` (rules `spawn_mobs` and `spawn_monsters`) **and** difficulty ≠ peaceful **and** attribute `gameplay/nether_portal_spawns_piglins` at `p` **and then** a draw `nextInt(2000) < difficultyId` (easy 1, normal 2, hard 3; the draw only when the three tests hold) **and** any player close enough for spawning (`ChunkMap.anyPlayerCloseEnoughForSpawning`: the chunk's centre within 128 blocks of a non-spectator player: squared distance `< 16384`):
  walk `p` down while the block is a nether portal; if the block there is a valid spawn for a zombified piglin: spawn one at `p + up` (entity hook; sets portal cooldown on it and its vehicle).
- **Command block**: scheduled tick **1** (from `neighborChanged` when powered state changes, and from its own tick while in `auto` mode); the tick runs the command (hook of the command system) — mode `auto`: mark condition met, execute if the previous condition was met, reschedule with delay 1 while powered or `automatic`; mode `redstone`: execute if the condition was met; then `updateNeighbourForOutputSignal`. Not detailed further here.
- **Campfire**: no block tick. Block-entity tickers (server side): **lit** → `cookTick`: for each of the 4 slots holding an item: `cookingProgress += 1`; when `progress ≥ cookingTime` (the recipe's time): drop the cooked result at the campfire position (`dropItemStack` draws, Inputs), clear the slot, `sendBlockUpdated`, game event `block_change`; `setChanged` if any item was present.
  **unlit** → `cooldownTick`: `cookingProgress = clamp(progress − 2, 0, cookingTime)` for slots with progress > 0; `setChanged` if any. The client-only smoke / particle ticker draws from the **client** level random and does not matter for server parity.

### 10. Creaking heart (block entity ticker + scheduled tick)

Properties `axis`, `state` ∈ `uprooted, dormant, awake`, `natural`. The block entity ticker exists (server) only when `state ≠ uprooted`.
- **Scheduled tick** (delay **1**, requested by every `updateShape`): `updateState`: `hasLogs` = both neighbours along `axis` are pale-oak logs **with the same axis** (`pale_oak_logs` tag and `axis` equal); if `hasLogs` and the state is `uprooted`: `setBlockAndUpdate(p, state = awake if the attribute creaking_active at p is true else dormant)`.
- **Ticker** (`serverTick`, each server tick): `ticksExisted += 1`; recompute the comparator output (`updateNeighbourForOutputSignal` when it changed); if `emitter > 0`: every 10th tick of the countdown a hurt sound; `emitter −= 1`.
  `if (ticker-- < 0)`: (the test uses the value *before* the decrement; initial `ticker = 0`, so the first update happens on the 2nd tick of the entity's life and then every `ticker + 2` ticks) → `ticker = nextInt(5) + 20` (**one draw**, level random) and run: `newState = updateCreakingState`: `uprooted` if the heart has no logs and no creaking is registered, else `awake` / `dormant` by the attribute;
  `setBlockAndUpdate` if changed (return when it became `uprooted`). If there is no creaking registered and the heart is `awake`, spawning monsters is allowed, difficulty ≠ peaceful and a player is within 32 blocks: try to spawn the protector creaking (entity milestone: up to 5 attempts at range 16 × 8 via `SpawnUtil`). If one exists: remove it when the attribute says inactive and it is not persistent, or it is farther than 34 blocks, or it has a stuck player.
  `creakingHurt` (entity hook): when `emitter ≤ 0` and `awake`: `n = nextIntBetweenInclusive(2, 3)` resin clumps are spread by a breadth-first walk through adjacent pale-oak logs (depth 2, ≤ 64 nodes, `shuffledCopy(Direction.values())` per visited log) — a consumer of level-random draws (Open question 4).

### 11. Potent sulfur (geyser) block entity

State `dry, wet, dormant, erupting, continuous` (set by `updateShape`/placement from the fluid above and the block below: not water-source above → `dry`; below in `causes_continuous_geyser_eruptions` and (empty or source fluid) → `continuous`; below in `causes_periodic_geyser_eruptions` → `dormant` unless already `erupting`/`dormant` (`resetCountdown` sets the countdown to −1 on entering); else `wet`).
Server tickers by state (composed left to right): `dry`: none; `wet`: nausea; `dormant`: countdown, then nausea; `erupting`: launch, then countdown; `continuous`: launch.
- `findNoxiousGasSource(origin)`: starting at `origin + up`, go up at most to `origin.y + 5`: while the position holds a water **source** fluid and (is the water block or is geyser-passable: non-air non-water blocks with an empty collision shape for the geyser's collision context): move up; at the first other position: if it is air or passable → that position is the source (surface); else none. `waterBlocks = source.y − origin.y − 1`.
- **Countdown ticker** (only when `gameTime % 20 == 0`): `source = findNoxiousGasSource`; if found: if `countdown ≤ 0`: `r = positional random at the block position` of `new Xoroshiro(worldSeed XOR −904011478).forkPositional()` (a fresh generator each time: deterministic per seed and position; `random.md`); for `dormant`: `countdown = 10 · (waterBlocks − 1) + r.nextIntBetweenInclusive(15, 30)`; otherwise (erupting): `r.nextInt()` (discard one value) then `countdown = (waterBlocks − 1) + r.nextIntBetweenInclusive(1, 2)`.
  Then if `countdown > 0`: `countdown −= 1`; if `countdown == 0`: `setBlockAndUpdate(p, state = (dormant → erupting, else → dormant))`; game event `block_deactivate` when going to dormant. (The unit is one step per 20 ticks.)
- **Nausea ticker** (`gameTime % 10 == 0`): living, non-spectator entities in the box `source ± 2.5` horizontally (height of the source block) that can be reached by the gas get nausea 80 ticks (entity hook; reach test: eye position in a passable block within squared distance 9 of the source centre, water below it, and line of sight).
- **Launch ticker** (every tick): entities in the column above the sulfur block (height = number of unobstructed blocks up to `6 · waterBlocks`) with `velocity.y < 0.3f + waterBlocks · 0.1` get `+0.2f` upward velocity (entity hook).
- The `onPlace` hook sends a block event on `erupting` / `continuous` that stores `eruptionTick = gameTime` (client sound timing only).

### 12. Lightning bolt: block effects (entity-tick hook; the strike itself is `server-tick.md` §12)

The bolt entity (`life` starts 2, `flashes = nextInt(3) + 1` from the **entity's own random**, `seed = nextLong()` likewise) acts as follows on blocks:
- At tick when `life == 2` (server, not visual-only): if the difficulty is **normal or hard**: `spawnFire(4)`; then `powerLightningRod`: if the block at the strike position `(x, y − 1e-6, z)` is a lightning rod: its strike hook (redstone-components); `clearCopperOnLightningStrike(strike position)`; game event `lightning_strike`.
- Later flashes (when `life < −nextInt(10)` after a flash): `flashes −= 1`, `life = 1`, new seed, `spawnFire(0)`.
- `spawnFire(extra)`: if `canSpreadFireAround(block position)`: `f = BaseFireBlock.getState(level, pos)`; if the block at `pos` is air and `f.canSurvive`: `setBlockAndUpdate(pos, f)`; then `extra` times: `n = pos + (nextInt(3) − 1, nextInt(3) − 1, nextInt(3) − 1)` (**entity random**, x, y, z order); same test and write.
- `clearCopperOnLightningStrike(struckPos)`: `waxed` = the struck block is in the wax-off map; `weathering` = it is a weathering copper block. If either: if weathering: `setBlockAndUpdate(struckPos, first stage of that block)` (state properties kept); then (level random) `strikes = nextInt(3) + 3`; for each strike: `steps = nextInt(8) + 1`; random walk from `struckPos`: for each step:
  `randomStepCleaningCopper(pos)`: iterate up to **10** random positions inside the cube `pos ± 1` (`3 × 3 × 3`; each candidate draws three `nextInt(3)` in x, y, z order, lazily): the first one holding a weathering copper block: `setBlockAndUpdate(candidate, previous stage)` if there is one (waxed blocks are not weathering copper); level event 3002; the walk continues from that position; if none of the 10 candidates qualifies, the walk stops.
- Entities hit take damage / are ignited by `thunderHit` (entity milestone).

## Edge cases and known quirks

1. **Falling-block entity spawn replaces the block before the entity exists** (flags 3): neighbour updates caused by the removal (e.g. other falling blocks scheduling) run first.
2. **Scaffolding** collapses only after the state with `distance 7` has been seen twice: the first tick with 7 *destroys* (drops) a scaffold that had been stable; only a block that already carried distance 7 becomes a falling entity.
3. **Stalactite ticks do not re-check support** — any scheduled tick drops the whole hanging column to its tip.
4. **Pointed dripstone random tick consumes two `nextFloat`s even when nothing grows**, and growth only attempts when the second one is `< 0.011377778f` (about 5 % of a random tick-day-share: "average 5 days per growth").
5. **Dripstone fluid type**: only *source* fluid types match (`water`, `lava`); a stalactite fed by flowing water does nothing.
6. **Dried ghast** needs a new random tick after each 5000-tick step.
7. **Turtle egg hatch chance is time-of-day data driven**; the draw is consumed only while the chance is positive; the sand check comes after the draw.
8. **Eyeblossom transition** draws 4 `nextDouble`s for a purely visual particle; they are part of the parity stream. Nearby identical flowers are rescheduled in scan order with one draw each.
9. **Hidden `onPlace` schedules** (the hook runs for every state-changing write, `set-block-flags-and-sequence.md` §3.10): sniffer egg stages reschedule themselves with a fresh `nextInt(300)`; scaffolding re-schedules itself after every state change; brushable blocks reschedule on every `dusted` change; frogspawn, turtle egg and composter re-run their `onPlace` effects likewise. Always count those draws and requests.
10. **Command block** executes inside the scheduled-tick phase; commands can do anything (not specified here).
11. **Frogspawn / tadpole count**: `nextInt(2, 6)` is an exclusive upper bound (2..5).

## Constants and IDs

Delays: falling blocks 2 (dragon egg 5); brushable 2 / 40 reset; scaffolding 1; path 1; hanging moss 1; big dripleaf unstable 10 / partial 10 / full 100; dried ghast 5000; sniffer egg `12000/3` boosted or `24000/3` + `nextInt(300)`; frogspawn `3600 + nextInt(8400)`; dripstone→cauldron `50 + fall distance`; command block 1; creaking heart 1 (state), 20..24 + 2 (ticker period);
campfire cook time per recipe; geyser `% 20` / `% 10`. Probabilities: speleothem growth `0.011377778f`; water transfer `0.17578125f`; lava transfer `0.05859375f`; turtle egg `attribute` default `0.002f`; budding amethyst `1/5`; piglin `nextInt(2000) < difficulty`; turtle / sniffer pitch `0.9f + 0.2f·r`; dripleaf pitch `0.8f..1.2f`. Distances: dripstone searches 11 (10 blocks), growth length 7 (spike 2); eyeblossom box `±3, ±2, ±3`;
geyser source search up to 5 blocks, nausea radius 2.5, launch height `6 · waterBlocks`; creaking range 32 / leash 34. Level events: 1045, 1052 (stalactite landing), 1504 (drip), 2001 (block break), 2015 (dragon egg), 3002 (copper clean), 3009 (sniffer boost).

## Oracle probe points

- `FallingBlock#tick`, `FallingBlockEntity#fall(Level, BlockPos, BlockState)` (public static); `ScaffoldingBlock#tick`, `#getDistance(BlockGetter, BlockPos)`; `BigDripleafBlock#tick`, `#setTiltAndScheduleTick` (private) / `#resetTilt`; `DriedGhastBlock#tick`, `#randomTick`; `SpeleothemBlock#randomTick`, `#growStalactiteOrStalagmiteIfPossible` (public, test-visible),
  `PointedDripstoneBlock#maybeTransferFluid(BlockState, ServerLevel, BlockPos, float)` (public static), `#findStalactiteTipAboveCauldron`, `#getCauldronFillFluidType`; `TurtleEggBlock#randomTick`; `SnifferEggBlock#tick`, `#onPlace`; `FrogspawnBlock#tick`; `BuddingAmethystBlock#randomTick`; `EyeblossomBlock#tryChangingState` (private), `FlowerPotBlock#randomTick`;
  `NetherPortalBlock#randomTick`; `CreakingHeartBlockEntity#serverTick`; `PotentSulfurBlockEntity` static ticker fields (`SERVER_WAITING_COUNTDOWN_TICKER` …) and `#geyserPositional(ServerLevel, BlockPos)` (public static); `LightningBolt#tick`, `#spawnFire` (private), `#clearCopperOnLightningStrike` (private static); `Containers#dropItemStack`.
- Draw logging: wrap the level `RandomSource`; dump `(tick, class#method, bound or kind)` per draw.

## Open questions

1. **[oracle] Environment attribute values** (`turtle_egg_hatch_chance`, `eyeblossom_open`, `creaking_active`, `nether_portal_spawns_piglins`, `water_evaporates`): see `tick-decay-weathering.md` Open question 1.
2. **[oracle] `Entity.random` determinism.** The lightning bolt's `flashes`, `seed` and `spawnFire` offsets come from `Entity.random` (not the level random). Probe whether the vanilla server seeds it from the world (e.g. via `Level#getRandom` forks) or from the system clock; a parity harness must pin it (reflection on `Entity#random`) or ignore `spawnFire(4)` positions.
3. **[oracle] Sniffer egg stage timing.** Confirm the `onPlace`-based rescheduling of §6: log `scheduleTick` calls and the `nextInt(300)` draws across the three stages of a boosted and an unboosted egg.
4. **[oracle] Creaking heart resin spread** draw order (`BlockPos.breadthFirstTraversal` plus per-node `shuffledCopy`) and the `spawnProtector` placement attempts use the level random; probe with a draw log when a creaking is hurt.
5. **[oracle] Brushing details** (10 brushes, 10-tick cooldown, 40-tick reset, `checkReset` every 2 ticks while `brushCount > 0`): confirm the interplay of the reset schedule with the falling check by logging a suspicious-sand block over air.
6. **[oracle] Falling block landing details** (placement flags, concrete-in-water case, drop on failure) belong to the entity milestone; the world-visible effects are `setBlockAndUpdate` (flags 3) and the item spawn.
7. **[oracle] Geyser entity masks and the `getUnobstructedBlockCount` collision context** (`CollisionContext.positionContext(y)`) — probe `PotentSulfurBlockEntity#LAUNCH_ENTITY_TICKER` with entities of different heights.
8. (implementer) `Mth.clamp(nextDouble(), 0.2f, 0.7999999970197678)`: the lower bound is the f32 `0.2f` widened to `0.20000000298023224`.

## Parity status

Not yet run.
