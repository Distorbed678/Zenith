# Tick: spreading blocks — grass / mycelium, nylium, mushrooms, vines, sculk (26.3)

- Verified by suites: `random-ticks`, `block-ticks`
- Sources: local reference reading of the 26.3 block classes (2026-11-26, reader session): the snowy / spreading-snowy block family (grass block,
  mycelium), nylium, the small-mushroom block, the vine block, the multiface block family with its spreader helper, the sculk behaviour
  interface, sculk block, sculk vein, sculk catalyst (block and block entity), sculk shrieker (block and block entity), the sculk spreader with its charge
  cursor, the block-scan helpers, direction and utility helpers (random direction, shuffle). Everything is in our own words.
- Related specs: [`tick-index.md`](tick-index.md), [`tick-growth.md`](tick-growth.md) (conventions, light notation, flags), [`../server-tick.md`](../server-tick.md) §7 / §8 / §10,
  [`../block-hooks.md`](../block-hooks.md), [`../scheduled-ticks-and-block-events.md`](../scheduled-ticks-and-block-events.md), [`../redstone-components.md`](../redstone-components.md) §7.6 (sculk sensor
  phases and vibration listening), [`../../lighting/light-engine.md`](../../lighting/light-engine.md) (`lightDampeningInto`), [`../../math/random.md`](../../math/random.md),
  [`../../blocks/state-facts.md`](../../blocks/state-facts.md).

## Summary

Spreading is driven by **random ticks** (grass, mycelium, nylium, mushrooms, vines), by **a block entity ticker** (sculk catalysts advance charge cursors every tick) and by **scheduled ticks** (catalyst pulse, shrieker). Every
draw is from the level `random`; the order below is the order of the calls. Conventions (`raw`, `local`, flags, "left-to-right short-circuit", random direction arrays) are those of [`tick-growth.md`](tick-growth.md) *Inputs and state*.

## Inputs and state

- `Direction` constants and orders used here: `VALUES = [down, up, north, south, west, east]` (this is the order of `Direction.getRandom`, `Direction.stream()` and of `allShuffled` before shuffling);
  horizontal array `[north, east, south, west]`; clockwise of north / east / south / west = east / south / west / north; counter-clockwise = west / north / east / south.
- **Shuffle** `Util.shuffle(list, random)`: for `i = size` down to `2`: `j = nextInt(i)`; swap elements `i−1` and `j`. `shuffledCopy` shuffles a copy; `allShuffled` = shuffled copy of `VALUES`.
- **Box scan** (`findBlocksIn(a, b)`): all positions of the axis-aligned box (inclusive corners), clipped to the world's section range; positions above / below the world count as **air**. Iteration order (only when something observable depends on it): section by section
  (x outer, z, y inner in section units), inside a section `y` outermost, then `z`, then `x` innermost. `atMostMatched(n)` = "at most `n` positions match" (counting stops after `n+1`).
- `isFull(fluid)`: the fluid state's `isFull` = a source or a full-height flowing level (see `../fluids.md`).
- Game rule `spread_vines` (bool, default true).
- `lightDampeningInto(from, to, UP, opacity(to))` as defined in `../../lighting/light-engine.md` ("returns 16 if the merged face occludes else the simple opacity of the upper state").

## Behavior (ordered rules)

### 1. Grass block and mycelium (spreading snowy block) — random tick

Both blocks have property `snowy`; the "base" block (what they revert to) is **dirt**. Both are randomly ticking in all states.
Helper `canStayAlive(state, p)` (`above = p + up`, `a` its state):
1. if `a` is `snow` with `layers == 1` → true;
2. else if the fluid of `a` is a **full** fluid (`isFull`) → false;
3. else `lightDampeningInto(state, a, UP, a.lightDampening) < 15`.
Helper `canPropagate(state, p)` = `canStayAlive(state, p)` **and** the fluid at `p + up` is not in tag `water`.
Helper `isSnowySetting(a)` = `a` in tag `snow` (state-tag; snow layers / snow block / powder snow per the registry).
**Random tick**:
1. If not `canStayAlive(state, p)`: `setBlockAndUpdate(p, dirt default)` and stop (no draws).
2. Else if `local(p + up) ≥ 9`: `s = this.defaultBlockState()` (the plain grass / mycelium state, `snowy = false`); repeat **4 times** (`i = 0..3`):
   `t = p + (nextInt(3) − 1, nextInt(5) − 3, nextInt(3) − 1)` (the three draws in the order x, y, z); if the block at `t` is **dirt** (block identity, not coarse dirt / podzol / rooted dirt)
   **and** `canPropagate(s, t)` (with the *default* state `s` as the "from" state): `setBlockAndUpdate(t, s with snowy = isSnowySetting(block at t + up))`.
3. Otherwise (light too low) nothing, no draws.
Each tick therefore consumes 0 draws when decaying or dark, else exactly 12 draws.

### 2. Nylium (crimson and warped) — random tick

If `lightDampeningInto(state, above, UP, above.lightDampening) ≥ 15` (the same dampening test as above, **without** the snow / fluid special cases): `setBlockAndUpdate(p, netherrack default)`. No draws, no spread. (Bone meal spreads nylium vegetation: not tick-driven.)

### 3. Brown and red mushroom — random tick (spread)

1. Draw `nextInt(25)`; if not 0, stop.
2. If `canSpread(p)` is false stop. `canSpread(p)` = at most **4** blocks of the same kind as `this` in the box `p + (−4, −1, −4)` .. `p + (4, 1, 4)` (the block itself counts) — no draws.
3. `o = p + (nextInt(3) − 1, nextInt(2) − nextInt(2), nextInt(3) − 1)`, the draws in this order: x, then y-first, y-second, then z (**four** draws).
4. Repeat 4 times: if `isEmpty(o)` **and** `state.canSurvive(level, o)` (mushroom survival: block below in tag `overrides_mushroom_light_requirement`, or `raw(o) < 13` with a solid-render block below): `p = o` (the walker moves).
   Then `o = p + (nextInt(3) − 1, nextInt(2) − nextInt(2), nextInt(3) − 1)` (four draws), whether or not the walker moved.
5. After the loop: if `isEmpty(o)` and `canSurvive(state, o)`: `setBlock(o, state, flags 2)`.
Total draws: 1 (success path ends here for 24/25 of ticks) or `1 + 4 + 16 = 21` (a full attempt reaches step 5 unless step 2 stops it after 1 draw).
Huge-mushroom growth is **not** a random-tick behaviour (only bone meal: `growMushroom` hook).

### 4. Vine — random tick

Requires game rule `spread_vines` (if false no draw, nothing happens). Faces: `up, north, east, south, west` booleans. Randomly ticking in all states. Let `faceProp(d)` be the boolean property of horizontal / up face `d`.
Definitions: `acceptable(q, d)` = the block at `q` can be attached to from direction `d` (`MultifaceBlock.canAttachTo(level, d, q, state)`: the neighbour's *support shape* or collision shape has a full face toward `opposite(d)`);
`canSpread(p)` = at most 4 vine blocks in the box `p + (−4, −1, −4)..p + (4, 1, 4)` (as mushroom §3.2).
Sequence:
1. Draw `nextInt(4)`; if not 0 stop.
2. `d = Direction.getRandom` (draw `nextInt(6)` over `VALUES`).
3. **Branch H** — `d` is horizontal **and** the vine has no face `d`:
   a. If not `canSpread(p)` → stop.
   b. `t = p + d`, `e` = block at `t`.
   c. If `e` is air: `cw`, `ccw` = clockwise / counter-clockwise of `d`; `hasCw = state has face cw`, `hasCcw = state has face ccw`; `cwT = t + cw`, `ccwT = t + ccw`.
      - if `hasCw && acceptable(cwT, cw)` → `setBlock(t, vine with only face cw, flags 2)`;
      - else if `hasCcw && acceptable(ccwT, ccw)` → `setBlock(t, vine with only face ccw, 2)`;
      - else with `o = opposite(d)`: if `hasCw && isEmpty(cwT) && acceptable(p + cw, o)` → `setBlock(cwT, vine with only face o, 2)`;
        else if `hasCcw && isEmpty(ccwT) && acceptable(p + ccw, o)` → `setBlock(ccwT, vine with only face o, 2)`;
        else (**draw** `nextFloat()`): if `value < 0.05` (double compare after widening) **and** `acceptable(t + up, UP)` → `setBlock(t, vine with only face up, 2)`. The draw happens only when all previous branches failed.
   d. Else if `e` is not air and `acceptable(t, d)`: `setBlock(p, state with face d set, flags 2)`.
4. **Branch V** — otherwise (`d` is up / down, or `d` is horizontal and the vine already has face `d`):
   a. If `d == UP` and `p.y < level.maxY`:
      - if `canSupportAtFace(p, UP)` (see below) → `setBlock(p, state with face up, 2)` and **stop**;
      - else if `isEmpty(p + up)`: if not `canSpread(p)` → stop; `s = state`; for each horizontal `h` in `[N, E, S, W]`: **draw `nextBoolean()`**; if it is true **or** (short-circuit: the boolean draw happens first, always) not `acceptable(p + up + h, h)`: `s = s with face h = false`.
        If `s` has any horizontal face: `setBlock(p + up, s, flags 2)`. **Stop** in any case.
      (The 4 `nextBoolean` draws happen in order N, E, S, W; `acceptable` is evaluated only when the draw is false.)
   b. Then (still branch V, when not stopped): if `p.y > level.minY`: `b` = block at `p − up`; if `b` is air **or** vine: `before = (b is air) ? default vine : b`; `after = copyRandomFaces(state, before)`:
      for each horizontal `h` in `[N, E, S, W]`: draw `nextBoolean()`; if true and `state` has face `h` then `to.face(h) = true` (**the draw is made for every `h`**, in this order, regardless of whether the state has the face).
      If `after != before` (state identity) and `after` has a horizontal face: `setBlock(p − up, after, flags 2)`.
   `canSupportAtFace(p, d)`: false for `down`; if `acceptable(p + d, d)` → true; for `d` on the Y axis false; for horizontal `d`: the block above `p` is a vine **with face d**.
Vine support loss / face loss is the `updateShape` hook (reaction file); a vine with zero faces is removed there.

### 5. Multiface spreading framework (shared by sculk veins; glow lichen spreads only through bone meal)

`MultifaceSpreader` with a *config* (spread types in order: `SAME_POSITION`, `SAME_PLANE`, `WRAP_AROUND`; for sculk veins the "same-space" spreader uses only `SAME_POSITION`).
- Spread position for `(pos, spreadDirection, fromFace)`: `SAME_POSITION` → `(pos, spreadDirection)` (a new face on the same block); `SAME_PLANE` → `(pos + spreadDirection, fromFace)`; `WRAP_AROUND` → `(pos + spreadDirection + fromFace, opposite(spreadDirection))`.
- `getSpreadFromFaceTowardDirection(state, pos, fromFace, spreadDirection)`: empty if `spreadDirection` and `fromFace` are on the same axis; else, if `config.isOtherBlockValidAsSource(state)` **or** (`state` has `fromFace` and not `spreadDirection`),
  take the first spread type (in order) whose spread position `sp` satisfies `canSpreadInto(sourcePos = pos, sp)`.
- `canSpreadInto` (default config): `stateCanBeReplaced(…)` — the existing block at `sp.pos` is air, or the same multiface block, or a water **source** block — and `block.isValidStateForPlacement(…)`: the face at `sp.face` is supported by the block and `oldState` does not already have it, and
  the neighbour at `sp.pos + sp.face` can be attached to. Sculk-vein config **replaces** `stateCanBeReplaced`: false if the block *behind* the new face (`sp.pos + sp.face`) is sculk, sculk catalyst or moving piston; false if `manhattan(pos, sp.pos) == 2` and the block at `pos + opposite(sp.face)` has a sturdy face toward `sp.face`;
  false if the existing fluid is non-empty and not water; false if the existing block is in tag `fire`; otherwise true if `existing.canBeReplaced()` (the state's replaceable property) or the default test above.
- `placeBlock`: `newState = block.getStateForPlacement(oldState, level, sp.pos, sp.face)` (valid → the old multiface state or default / waterlogged-by-water-source state with the face set; invalid → none); if none, fail; else (generation: mark for post-processing) `setBlock(sp.pos, newState, flags 2)`; success = the write reports success.
- `spreadAll(state, pos, postProcess)`: for each `faceDirection` in `VALUES` order for which `config.canSpreadFrom(state, faceDirection)` (other-block-valid-as-source, or the state has that face): for each `spreadDirection` in `VALUES` order: `getSpread…` then `placeBlock`; counts successes. **The `state` argument stays the one passed in** even though earlier placements in the loop changed the world.
  Returns the number of successful placements (`> 0` means "spread something").
- `spreadFromRandomFaceTowardRandomDirection` (bone meal / lichen hook): shuffle the 6 directions → for each (face) with `canSpreadFrom`: shuffle again and take the first spread that places; stop at the first face that produced one.

### 6. Sculk spreader (`SculkSpreader`) — the cursor engine

One spreader per catalyst block entity (level spreader) and per world-generation feature. Parameters:

| | level spreader (catalysts) | world-gen spreader |
|---|---|---|
| `replaceableBlocks` tag | `sculk_replaceable` | `sculk_replaceable_world_gen` |
| `growthSpawnCost` | 10 | 50 |
| `noGrowthRadius` | 4 | 1 |
| `chargeDecayRate` | 10 | 5 |
| `additionalDecayRate` | 5 | 10 |
| `isWorldGeneration` | false | true |

Fixed: at most **32** cursors; max cursor charge 1000; max distance of a cursor from the origin `1024` (Chebyshev; a cursor farther than that is not processed and is **dropped** from the list, Edge 6); `MAX_GROWTH_RATE_RADIUS` 24; shrieker placement rate 11.
**Cursor** (`ChargeCursor`): `pos`, `charge` (0..1000), `updateDelay` (starts 0), `decayDelay` (starts 1), `facings` (optional set of directions, absent at creation).
**`addCursors(startPos, charge)`**: split `charge` into chunks of at most 1000 (first chunk first); each becomes a cursor (charge as given, delays 0 / 1, no facings) **if fewer than 32 cursors exist**; surplus is discarded.
**`updateCursors(level, originPos, random, spreadVeins)`** — executed once per tick per catalyst by the block-entity ticker (§7), with `spreadVeins = true`:
1. If there are no cursors stop. Else start empty lists; for each cursor **in list order**:
   a. If `chebyshev(cursor.pos, originPos) > 1024` skip it entirely (it is dropped from the new list: the new list only receives cursors that are processed and still charged).
   b. `cursor.update(…)` (below). If `cursor.charge ≤ 0`: level event 3006 at the cursor (discharge particle); the cursor is **dropped**.
   c. Else: let `q = cursor.pos`; if no cursor is yet recorded at `q` in this pass: record it and append it to the new list. Else if not world-gen and `cursor.charge + existing.charge ≤ 1000`: **merge** into the existing cursor (`existing.charge += cursor.charge`, `cursor.charge = 0`, `existing.updateDelay = min(both)`); the merged cursor is **not** appended. Else append the cursor, and if its charge is lower than the recorded one it becomes the recorded cursor.
   (Particle level events 3006 with packed data are sent afterwards: no world effect.)
2. `cursors = new list`.
**`cursor.update(level, originPos, random, spreader, spreadVeins)`**:
1. Proceed only if `charge > 0` and (world-gen, or the **origin** `originPos` is in a block-ticking-range chunk of the server level: simulation level ≤ 32).
2. If `updateDelay > 0`: `updateDelay -= 1`; end.
3. `state` = block at `pos`, `behaviour` = the state's block if it implements the sculk behaviour (only the **sculk block** and the **sculk vein** do; every other block, including the catalyst, uses the default behaviour).
4. If `spreadVeins` and `behaviour.attemptSpreadVein(level, pos, state, facings, worldGen)` is true: if `behaviour.canChangeBlockStateOnSpread()` re-read `state` and `behaviour` at `pos`; play sound `SCULK_BLOCK_SPREAD` at `pos`.
5. `charge = behaviour.attemptUseCharge(cursor, level, originPos, random, spreader, spreadVeins)` (draws: see the behaviour table).
6. If `charge ≤ 0`: `behaviour.onDischarged(level, state, pos, random)`; end of update.
7. Else: `transfer = getValidMovementPos(...)`: if present: `behaviour.onDischarged(...)` at the old pos (with the pre-move state), `pos = transfer`, `state = block at transfer`; else if world-gen: `onDischarged`, `charge = 0`, end.
   If the block at the (possibly new) pos implements the sculk behaviour: `facings = availableFaces(state)` (the set of faces that the multiface state has; a sculk **block** is not multiface → empty set).
   `decayDelay = behaviour.updateDecayDelay(decayDelay)` (the behaviour object in use at this point: the one read in step 3, re-read in step 4 only when that behaviour may change state); `updateDelay = behaviour.getSculkSpreadDelay()` (1 for all).
**`getValidMovementPos`**: `candidates = shuffledCopy(NON_CORNER_NEIGHBOURS, random)` — the 18 offsets `(dx, dy, dz) ∈ {−1,0,1}³` with at least one zero coordinate, excluding `(0,0,0)`, listed with `x` fastest, then `y`, then `z` slowest (z = −1: y = −1: x = −1, 0, 1 minus the corner; …) before shuffling (one draw per shuffle step, 17 draws).
   `result = pos`. For each offset in shuffled order: `n = pos + offset`; skip if world-gen and `(origin.x − n.x)² + (origin.z − n.z)² > 144`; require the block at `n` to implement the sculk behaviour (sculk or sculk vein) **and** movement unobstructed (`isMovementUnobstructed(pos, n)`);
   then `result = n` and **if** `hasSubstrateAccess(n)` (the block at `n` is a sculk vein with a face whose adjacent block is in tag `sculk_replaceable`) stop scanning; otherwise **keep scanning** (a later candidate overrides). Return `result` if `result ≠ pos`, else none.
   `isMovementUnobstructed(from, to)`: true if `manhattan(from, to) == 1`; else with `delta = to − from` and the unit directions `dx, dy, dz` toward `to` per axis (positive when the delta component ≥ 0): if `delta.x == 0` → `free(dy) or free(dz)`; else if `delta.y == 0` → `free(dx) or free(dz)`; else `free(dx) or free(dy)`,
   where `free(dir)` = the block at `from + dir` has **no** sturdy face toward `opposite(dir)`.

**Behaviours** (`attemptSpreadVein`, `attemptUseCharge`, `onDischarged`, `updateDecayDelay`, `canChangeBlockStateOnSpread`):

| behaviour | `attemptSpreadVein(pos, state, facings)` | `attemptUseCharge` | `onDischarged` | `updateDecayDelay(age)` | can change state |
|---|---|---|---|---|---|
| **default** (any other block under a cursor: air, water, stone…) | `facings == null`: the vein *same-space* spreader `spreadAll(state at pos, pos)` succeeded `> 0`; `facings` non-empty: if `state` is neither air nor water → false, else **regrow** (§6.1); `facings` empty: the generic `spreadAll(state, …)` of the vein block's regular spreader `> 0` | `decayDelay > 0 ? charge : 0` | none | `max(age − 1, 0)` | yes |
| **sculk block** | generic: `spreadAll` of the regular vein spreader on the sculk block's state (`facings` is not consulted) `> 0` | §6.2 | none | 1 | **no** |
| **sculk vein** | generic as the sculk block | §6.3 | §6.4 | 1 | yes |

(The sculk block and the vein use the interface defaults: the generic vein spreader — whose config treats any non-vein block as a valid *source for every face*, so a sculk block spreads veins to all six sides — `updateDecayDelay = 1`, `getSculkSpreadDelay = 1`.)

**6.1 `SculkVeinBlock.regrow(pos, existing, faces)`**: `ns = sculk vein default`; for each direction `f` in the given collection order: if `MultifaceBlock.canAttachTo(level, pos, f)` set face `f`, mark found. If none found → false. If the existing block's fluid is non-empty → `waterlogged = true`. `setBlockAndUpdate(pos, ns)` (flags 3); true.
**6.2 Sculk block `attemptUseCharge(cursor, originPos, random, spreader, spreadVeins)`**: `c = cursor.charge`. If `c == 0` or the draw `nextInt(chargeDecayRate) ≠ 0` (drawn only when `c ≠ 0`): return `c` unchanged. Otherwise:
   `close = distance(cursor.pos, originPos) < noGrowthRadius` (strict Euclidean `closerThan`, f64 compare of squared distance `< radius²`).
   If **not** `close` and `canPlaceGrowth(cursor.pos)`: draw `nextInt(growthSpawnCost)`; if `< c`: `gp = cursor.pos + up`; `growthState = randomGrowthState`: draw `nextInt(11)`; if 0 a **sculk shrieker** with `can_summon = isWorldGeneration`, else a **sculk sensor**; if the chosen state has `waterlogged` and the fluid at `gp` is non-empty it becomes waterlogged;
   `setBlockAndUpdate(gp, growthState)`; play that block's place sound at `cursor.pos`. Return `max(0, c − growthSpawnCost)` (also when the inner draw failed).
   Else (close or cannot place): draw `nextInt(additionalDecayRate)`; if it is **not** 0 return `c`; else return `c − (close ? 1 : decayPenalty)` where
   `decayPenalty = max(1, trunc_i32(c · factor · 0.5f))`, `factor = min(1.0f, outer² / maxReach²)`, `outer = sqrt(distSqr(cursor.pos, originPos)) − noGrowthRadius` (f32; `(float)Math.sqrt(...)` of the f64 squared distance, minus the radius, squared in f32), `maxReach = (24 − noGrowthRadius)²` (int).
   `canPlaceGrowth(q)`: the block at `q + up` is air, or the water block with a water fluid state; **and** at most **2** blocks in tag `sculk_growth_inhibitors` in the box `q + (−4, 0, −4)..q + (4, 2, 4)`.
**6.3 Sculk vein `attemptUseCharge`**: if `spreadVeins` and `attemptPlaceSculk(spreader, cursor.pos)` → return `charge − 1`. Else draw `nextInt(chargeDecayRate)`: if 0 return `floor_f32(charge · 0.5f)` else `charge`. (When `spreadVeins` is false the first test is skipped and no extra draw is made.)
   `attemptPlaceSculk(pos)`: `state` = block at `pos`; for each `support` in `allShuffled(random)` (6 draws, the shuffle happens even if no face matches): if `state` has face `support`: `sp = pos + support`; if the block at `sp` is in the spreader's replaceable tag:
   `setBlockAndUpdate(sp, sculk)`; push entities up (`Block.pushEntitiesUp(old, sculk, level, sp)`); sound; `veinSpreader.spreadAll(sculk default, sp, worldGen)` (the regular spreader, from the sculk block); then for every `v` in `VALUES` except `opposite(support)`: if the block at `sp + v` is a sculk vein: `onDischarged` of that vein (§6.4, **no `random` use**). Return true (stop the loop). Return false if no support matched.
**6.4 Sculk vein `onDischarged(state, pos)`**: only if the state is a sculk vein: for each `dir` in `VALUES`: if the vein has face `dir` and the block at `pos + dir` is **sculk**: remove that face. If it has no face left: the block becomes water if the fluid at `pos` is non-empty, else air. `setBlockAndUpdate(pos, state)` (flags 3, **always** written, even when unchanged).

### 7. Sculk catalyst — block entity ticker and scheduled tick

- **Ticker** (`serverTick`, every server tick while the block entity ticks, `server-tick.md` §8): `spreader.updateCursors(level, pos, level.random, spreadVeins = true)` — the *catalyst position* is the origin.
- **Cursor creation** (game event `entity_die` within the listener radius 8, delivery by distance): hook of the vibration / game-event milestone; it calls `addCursors(floor(dying mob position + 0.5 up), experienceReward)` and a **bloom**:
  `setBlockAndUpdate(catalyst, bloom = true)`, `scheduleTick(catalyst, block, 8)`, particles, sound pitch `0.6f + nextFloat() · 0.4f` (**one `nextFloat` from the level random**).
- **Scheduled tick** (delay 8): if `bloom` is true → `setBlockAndUpdate(pos, bloom = false)`.
- The spreader state (cursors) is saved in the block entity (`cursors` list: pos, charge, decay delay, update delay, facings).
- A catalyst has **no random tick**; its block entity ticker exists only on the server.

### 8. Sculk shrieker — scheduled tick

- `shriek` (called by the vibration listener hook and by `stepOn`): `setBlock(pos, shrieking = true, flags 2)`; `scheduleTick(pos, block, 90)`; level event 3007; game event `shriek`.
- **Scheduled tick**: if `shrieking`: `setBlockAndUpdate(pos, shrieking = false)`; then the block entity's `tryRespond`: only if `can_summon` and difficulty ≠ peaceful and game rule `spawn_wardens` and `warningLevel > 0`:
  try to spawn a warden if `warningLevel ≥ 4` (entity hook: up to 20 attempts around the shrieker); if not spawned: play the warning sound for the level (levels 1..4 only): **three draws** `nextInt(21) − 10` for x, y, z (in that order) from the level random; then darkness is applied around (entity hook).
- The shrieker's own vibration ticker (`VibrationSystem.Ticker`) is the redstone-components §7.6 mechanism.

## Edge cases and known quirks

1. **Grass decay vs spread**: the decay test (`canStayAlive`) is evaluated on the *current* state; spreading requires `local(p + up) ≥ 9` with sky darkening — grass spreads less at night.
2. **Spread targets are dirt only** (not coarse dirt, podzol, mud, rooted dirt); the `snowy` property of the new block is derived from the block above the *target*.
3. **Mushrooms**: the walker only moves when the target is empty **and** survivable; the final target `o` is a fresh offset from the *last* position, so up to 4 hops (`1 + 4 + 16 = 21` draws).
4. **Vine branch V when `d` is a horizontal direction the vine already has** falls into the vertical section: it can still grow downwards (4 `nextBoolean` for faces) — with `d` horizontal, step 4a is skipped (it needs `d == UP`).
5. **Vine copy faces** makes four `nextBoolean` draws even when the target would be unchanged; the identity test `after != before` is on the interned state objects (equality of properties).
6. **Cursors beyond 1024** are skipped but, because only processed cursors are re-added, **they disappear** from the list (the new list is built from the processed ones). Worth probing.
7. **Order of movement vs spread**: the vein spread attempt (step 4) happens on the *old* position before the charge is used and before moving; `onDischarged` of the old position is called only when the cursor moves or runs out.
8. **Merging** keeps the earlier cursor; because merging happens in list order, a cursor appended after a non-mergeable same-position cursor can become the "recorded" one if it has a lower charge.
9. **`closerThan`** is strict (`<`) on the squared integer-coordinate distance against `radius²`: a cursor exactly `noGrowthRadius` away is **not** close.
10. **Sculk catalysts tick only while their chunk is block-ticking** (`shouldTickBlocksAt(origin)`): cursors persist but do nothing otherwise.
11. **Bloom flag** is cleared 8 ticks after the last bloom write because every bloom schedules (a duplicate schedule request for the same block / position is discarded per scheduled-tick rules: a second death within 8 ticks does not reset the timer).

## Constants and IDs

Light thresholds: grass spread `local(p+up) ≥ 9`; mushroom survive `raw < 13`; dampening 15. Grass tries 4, offsets `nextInt(3)−1, nextInt(5)−3, nextInt(3)−1`. Mushroom chance 1/25, box ±4 × ±1 × ±4, limit 4 (also vine). Vine chance 1/4; extra 0.05f. Sculk: max cursors 32, max charge 1000, level spreader (10, 4, 10, 5), worldgen spreader (50, 1, 5, 10), shrieker chance 1/11 (`nextInt(11) == 0`), catalyst bloom 8 ticks,
shrieker 90 ticks, level events 3006 (cursor particles), 3007 (shriek), distance limit 1024, growth inhibitor limit 2 in box `(−4,0,−4)..(4,2,4)`, decay formula `0.5f`, max reach `24 − noGrowthRadius`.

## Oracle probe points

- `SpreadingSnowyBlock#randomTick`, `NyliumBlock#randomTick`, `MushroomBlock#randomTick`, `VineBlock#randomTick`, `MultifaceSpreader#spreadAll`, `#spreadFromFaceTowardDirection`, `SculkSpreader#updateCursors`, `SculkSpreader$ChargeCursor#update`, `SculkBlock#attemptUseCharge`, `SculkVeinBlock#attemptUseCharge`, `#onDischarged`,
  `SculkCatalystBlockEntity#serverTick`, `SculkCatalystBlock#tick`, `SculkShriekerBlock#tick`, `SculkShriekerBlockEntity#tryRespond`; `SculkSpreader#getCursors()` is public (dump every tick); `SculkSpreader$ChargeCursor#getPos/#getCharge/#getDecayDelay/#getFacingData` public.
- `ServerLevel#shouldTickBlocksAt(BlockPos)`.

## Open questions

1. **[oracle] Dropped far cursors (Edge 6).** Probe: add a cursor at `catalyst + 1100` (reflection into the cursor list) and call `updateCursors`; read the list size afterwards.
2. **[oracle] `allShuffled(random)` source.** `Direction.allShuffled` shuffles a copy of `values()` — verify that the shuffled array is `VALUES` (`down, up, north, south, west, east`) by drawing from a seeded `LegacyRandomSource` and comparing with §Inputs shuffle.
3. **[oracle] Box-scan counting for `atMostMatched` near the world top / bottom** (positions outside the world count as air in the scan): irrelevant for mushrooms and vines (they never match air), relevant for `sculk_growth_inhibitors` only if air is a member (it is not).
4. **[oracle] Vine `acceptable` for transparent neighbours** uses the support shape *and* the collision shape of the neighbour state (`MultifaceBlock.canAttachTo`): probe with leaves, glass, stairs, slabs, fences.
5. **[oracle] `Mth.floor` vs `trunc` for the vein charge halving**: `floor(charge · 0.5f)` of a positive value; confirmed positive-only (cursor charge > 0) so trunc = floor.
6. (implementer) Sculk and veins write with `setBlockAndUpdate` (flags 3) from inside the block-entity ticker; the cascading updates are in `../neighbor-and-shape-updates.md`.

## Parity status

Not yet run.
