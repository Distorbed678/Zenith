# Tick: plant growth — crops, stems, cocoa, berries, wart, saplings, canes, bamboo, kelp and vines, chorus (26.3)

- Verified by suites: `random-ticks`, `block-ticks`
- Sources: local reference reading of the 26.3 block classes (2026-11-26, reader session): the crop base class and its
  beetroot / potato / carrot / torchflower / pitcher subclasses, the stem and attached-stem classes, cocoa, sweet berry
  bush, nether wart, the sapling class with its tree-grower helper, the mangrove propagule, sugar cane, cactus and cactus
  flower, bamboo stalk and sapling, the growing-plant head / body classes with their kelp / weeping / twisting / cave-vine
  subclasses, the chorus flower and plant, the block-scan helpers, the weighted-list picker and the random-source default
  methods. Everything below is in our own words; tags are named, never listed.
- Related specs: [`tick-index.md`](tick-index.md) (which section covers which block), [`../server-tick.md`](../server-tick.md)
  §10 (how a random tick is selected, `randValue` stream, the shared `random`), [`../scheduled-ticks-and-block-events.md`](../scheduled-ticks-and-block-events.md),
  [`../set-block-flags-and-sequence.md`](../set-block-flags-and-sequence.md) (flag meanings), [`../block-hooks.md`](../block-hooks.md),
  [`../neighbor-and-shape-updates.md`](../neighbor-and-shape-updates.md), [`../../math/random.md`](../../math/random.md) (`nextInt`,
  `nextFloat`, `nextDouble`), [`../../lighting/chunk-steps.md`](../../lighting/chunk-steps.md) §10 (light reads),
  [`../../worldgen/features-trees.md`](../../worldgen/features-trees.md) (tree placement), [`../../blocks/state-facts.md`](../../blocks/state-facts.md) §2.10
  (which states are randomly ticking).

## Summary

Every plant here advances only when the block receives a **random tick** (`server-tick.md` §10: `tickSpeed` chances per section per tick
to pick a random position; a block ticks if its state *is randomly ticking* at the moment the position is read) or, for a few, when a
**scheduled tick** it asked for in a shape update arrives (support check). This file lists, per block, the exact draws from the level `random`
(the shared 48-bit legacy source), their order, the conditions that gate each draw, the writes (state and flags) that follow, and the
side effects that are hooks for other specs. A draw that is behind a failing condition is **not** made: all `&&` / `||` below are
left-to-right short-circuit, and the order of draws is the order written.

## Inputs and state

- `random`: the level random source. All draws in this file come from it, whether the block calls it `random` or `level.getRandom()`
  (the same object).
- `nextInt(n)` = uniform in `[0, n)` (one source step for the legacy generator; power-of-two bounds use the high bits, others the rejection loop of `math/random.md`);
  `nextFloat`, `nextDouble` likewise. `nextIntBetweenInclusive(a, b)` = `a + nextInt(b − a + 1)`.
- **Raw light** `raw(p)` = `getRawBrightness(p, 0)` = `max(blockLight(p), skyLight(p))` of the **visible** light layers (no sky darkening).
  **Local light** `local(p)` = `getMaxLocalRawBrightness(p)` = `max(blockLight, skyLight − skyDarken)`; 15 if `p.x` or `p.z` is outside
  `[−30000000, 30000000)` (`lighting/chunk-steps.md` §10). Light read lag: see Open question 1.
- `isEmpty(p)` = block at `p` is air (any of the three air blocks).
- `HORIZONTAL` direction order for "random direction": `[north, east, south, west]`; a random horizontal direction costs one `nextInt(4)` and indexes this array.
- Tags used (names only; contents come from the data-pack registry at run time): `supports_crops`, `grows_crops`, `supports_vegetation`,
  `supports_nether_wart`, `supports_cocoa`, `supports_sugar_cane`, `supports_sugar_cane_adjacently` (block and fluid), `supports_cactus`,
  `supports_bamboo`, `supports_chorus_flower`, `supports_chorus_plant`, `cannot_support_kelp`, `supports_pumpkin_stem` / `supports_melon_stem`,
  `supports_pumpkin_stem_fruit` / `supports_melon_stem_fruit`, `supports_mangrove_propagule`, `supports_hanging_mangrove_propagule`,
  `overrides_mushroom_light_requirement`, `flowers`.
- **Flags** used by the writes below (`set-block-flags-and-sequence.md`): `2` = clients only (shape updates still run, no neighbour notification);
  `3` = `setBlockAndUpdate`; `260` = `UPDATE_NONE` (4 + 256: no neighbour, no client-visible update bit, no block-entity side effects — but
  *shape updates run* and the client change broadcast still happens through the server change tracking); `818` = 2 + 16 + 32 + 256 + 512 (client update, skip shape updates, suppress drops, skip block-entity side
  effects, skip `onPlace`).

## Behavior (ordered rules)

### 1. Crop growth speed (shared helper `growthSpeed(block, p)`)

Used by every crop, stem and pitcher crop. All arithmetic is `f32`.
1. `speed = 1.0`. `below = p − (0,1,0)`.
2. For `xx = −1..1` (outer), `zz = −1..1` (inner): `s = 0.0`; look at the block state at `below + (xx, 0, zz)`: if it is in tag `grows_crops`:
   `s = 1.0`, and if that state has a `moisture` property greater than 0 (a missing property counts as 0) then `s = 3.0`. If `(xx, zz) ≠ (0, 0)`:
   `s = s / 4.0`. `speed = speed + s`.
3. Crowding: `W`, `E`, `N`, `S` = the four horizontal neighbours of `p`. `horizontal` = block at `W` **or** `E` is the **same block** as `block`
   (block identity, not state); `vertical` = `N` **or** `S` is the same block. If `horizontal && vertical`: `speed = speed / 2.0`. Else if any of the four diagonal positions
   (`W+N`, `E+N`, `E+S`, `W+S`) is the same block: `speed = speed / 2.0`.
4. The probability gate used by callers is `nextInt(trunc_i32(25.0f / speed) + 1) == 0` (`25.0f / speed` in f32, truncated toward zero).
   Typical: all-dry farmland → speed 1 + 8·0.25 + 1 = 3.0 → bound 9; all irrigated → speed 1 + 3 + 8·0.75 = 10.0 → bound 3.

### 2. Crops: wheat, carrots, potatoes, beetroots, torchflower crop

Block `CropBlock` family; the *age property* and *max age* per block: wheat / carrots / potatoes age 0..7, max 7; beetroots age 0..3, max 3; torchflower crop
property age 0..1 but its max age is **2** (see below). Randomly ticking iff `age < maxAge` (state-facts §2.10).
**Random tick** (wheat, carrots, potatoes — base behaviour):
1. If `raw(p) < 9`: stop (no draw).
2. `age = state.age`; if `age ≥ maxAge` stop (no draw).
3. `speed = growthSpeed(this, p)` (§1). Draw `nextInt(trunc(25.0f/speed) + 1)`; if it is 0: `setBlock(p, state with age+1, flags 2)`.
**Beetroots**: first draw `nextInt(3)`; if it is **not** 0 run the base behaviour above (which draws again); if it is 0 nothing else happens.
So per random tick there are 1 (skipped tick) or 1 + (0 or 1) draws, the second only when `raw(p) ≥ 9`.
**Torchflower crop**: same as beetroots (a first `nextInt(3)` draw, then the base behaviour on `≠ 0`), but its "max age" is 2 and the age property only reaches 1:
the base step computes the new state with `getStateForAge(age + 1)`; for `age + 1 == 2` the result is the **torchflower** flower block's default state (the
crop becomes a flower). A state with age 1 therefore still ticks (1 < 2) and ends the crop. Other ages are plain age writes (flags 2).
Survival of crops needs `raw(p) ≥ 8` and support `supports_crops`: that is the `canSurvive` hook (reaction file), not tick-driven here, but a crop in the dark is removed
only when a neighbour update / shape update asks.

### 3. Pitcher crop

Two-block plant, age 0..4 on both halves, half `lower` / `upper`. Randomly ticking iff `half == lower` and `age < 4`.
**Random tick** (state at the lower half):
1. `speed = growthSpeed(this, p)`; draw `nextInt(trunc(25.0f/speed) + 1)` (this draw happens **before** any light test); if it is not 0 stop.
2. `grow(p, state, +1)`: `newAge = min(age + 1, 4)`. It proceeds only if **all** of: age is not already 4; `raw(p) ≥ 8`; `p + up` is inside build height; and
   (`newAge < 3` **or** the block at `p + up` is air or pitcher crop). Then `setBlock(p, state age = newAge, flags 2)`; and if `newAge ≥ 3`: `setBlockAndUpdate(p + up, the same state with half = upper)` (flags 3).
No other draws.

### 4. Stems (pumpkin stem, melon stem)

Age 0..7; randomly ticking for every age (state-facts: stems tick always — the class overrides nothing; the `rt` property is set). Parameters per block: fruit block
(pumpkin / melon), attached-stem block, `stemSupport` tag, `fruitSupport` tag (`supports_*_stem_fruit`).
**Random tick**:
1. If `raw(p) < 9` stop (no draw).
2. `speed = growthSpeed(this, p)`; draw `nextInt(trunc(25.0f/speed) + 1)`; if not 0 stop.
3. If `age < 7`: `setBlock(p, age+1, flags 2)`; stop.
4. Else (age 7): draw a random horizontal direction `d` (`nextInt(4)` over `[N, E, S, W]`). `t = p + d`. If the block at `t` is air **and** the block at `t − (0,1,0)` is in `fruitSupport`:
   `setBlockAndUpdate(t, fruit default state)` **then** `setBlockAndUpdate(p, attached stem default state with facing = d)` (flags 3 both, in that order; the attached stem write triggers shape updates /
   neighbour notifications, which can remove or modify the fruit — the second write happens regardless).
Attached stems have no random tick. (Their `updateShape` reverting to an age-7 stem when the fruit disappears is a hook.)

### 5. Cocoa, sweet berry bush, nether wart

- **Cocoa** (age 0..2, facing): randomly ticking iff `age < 2`. Random tick: draw `nextInt(5)`; if 0 and `age < 2`: `setBlock(p, age+1, flags 2)`. **No light condition.**
- **Sweet berry bush** (age 0..3): randomly ticking iff `age < 3`. Random tick: `age < 3 && nextInt(5) == 0 && raw(p + up) ≥ 9` (order: the draw is made first, light only if the draw is 0). On success `setBlock(p, age+1, flags 2)`, then the
  `BLOCK_CHANGE` game event at `p` with the new state (event / vibration spec).
- **Nether wart** (age 0..3): randomly ticking iff `age < 3`. Random tick: `age < 3 && nextInt(10) == 0` → `setBlock(p, age+1, flags 2)`. No light condition. (Soul sand support is the `canSurvive` hook.)

### 6. Saplings and mangrove propagule

Saplings (oak, spruce, birch, jungle, acacia, cherry, dark oak, pale oak, poplar) have property `stage` 0..1 and a **tree grower** (§6.1); azalea and flowering azalea are not randomly ticking.
**Random tick** (sapling): `local(p + up) ≥ 9 && nextInt(7) == 0` (draw only if bright enough; note `local` uses sky darkening, unlike the crops) → `advanceTree(p, state)`:
- if `stage == 0`: `setBlock(p, stage = 1, flags 260)`;
- else: `treeGrower.growTree(...)` (§6.1) with the same `random`.
**Mangrove propagule** (extends the sapling; props `age 0..4`, `hanging`, `waterlogged`, `stage`): `randomTick`:
- if not `hanging`: `nextInt(7) == 0` → `advanceTree` (same as above, **no light test**);
- if `hanging`: if `age < 4` (not fully grown): `setBlock(p, age+1, flags 2)` with no draw (this hanging propagule matures **every random tick**). A fully grown hanging propagule does nothing.
(Dropping the grown propagule is the block's `updateShape` / entity hook.) The propagule is randomly ticking in every state (property `rt`).
**Bone meal** is not tick-driven and not in this file.

#### 6.1 Tree grower `growTree(level, pos, state, random)` (random draws and order)

Per grower: `trees` (weighted list), `megaTrees` (weighted list), `flowerTrees` (weighted list), all of tree-feature ids; the lists are:

| grower | trees (weights) | mega trees | flower trees |
|---|---|---|---|
| oak | oak 9, fancy oak 1 | – | oak-bees-005 9, fancy-oak-bees-005 1 (the `*_bees_005` feature variants, which add a bee nest with the feature's own chance) |
| spruce | spruce | mega spruce 1, mega pine 1 | – |
| mangrove (propagule) | mangrove 15, tall mangrove 85 | – | – |
| azalea | azalea tree | – | – |
| birch | birch | – | birch-with-bees |
| jungle | jungle tree without vines | mega jungle | – |
| acacia | acacia | – | – |
| cherry | cherry | – | cherry-with-bees |
| dark oak | – | dark oak | – |
| pale oak | – | pale-oak (bonemeal variant) | – |
| poplar | red poplar 1, orange poplar 1, yellow poplar 1 | – | – |

(The azalea grower is only reachable through bone meal; azalea blocks do not random tick. Feature names resolve through the feature registry; see `features-trees.md`.)
Picking from a weighted list = `nextInt(totalWeight)` then the first entry whose cumulative weight exceeds it (list order as in the table); an **empty** list draws nothing and yields "none".
Sequence:
1. **Mega pick**: draw from `megaTrees` (one draw if the list is non-empty, **always**, even if no 2×2 group exists). If it yielded a feature:
   find a 2×2 of the *same sapling block* containing `p`: scan `dx = 0, −1` (outer) × `dz = 0, −1` (inner); the 2×2 square has corners `(p.x+dx, p.z+dz)` .. `(p.x+dx+1, p.z+dz+1)` at `p.y`; the **first** square (in that scan order) in which all four blocks are
   the sapling's block wins. If one is found: set all four positions to `legacy block of their fluid` (water or air) with flags **818**; place the mega feature at `p + (dx, 0, dz)` with this level `random`
   (`Feature.place`); on success return true. On failure restore the four saved states with flags 260 and return false (**do not** fall through).
   If no 2×2 is found, continue with step 2.
2. **Normal pick**: `hasFlowers` = any block in the box `p + (−2, −1, −2)` .. `p + (2, 1, 2)` is in tag `flowers` (no draw). Draw from `flowerTrees` if `hasFlowers` and that list is non-empty, else from `trees`; a draw only if the chosen list is non-empty.
   If nothing resulted (empty list): return false (dark oak and pale oak without a 2×2 group do nothing; the mega draw of step 1 was already made).
3. Replace `p` by its fluid's legacy block with flags 818, then `Feature.place(level, generator, random, p)`; on success return true; on failure restore the sapling state with flags 260 and return false.
After a successful growth nothing else ticks. On failure the sapling stays at **stage 1** and retries on every later successful draw.
The placement itself (blocks, leaf distance, drops) is `features-trees.md` §2 with the **level random** instead of a worldgen random; the feature call here is the full wrapper used by `ConfiguredFeature` (the result bool decides success).

### 7. Sugar cane

Age 0..15. Randomly ticking in every state (property `rt`; `age` property exists).
**Random tick**: if the block above is air:
1. `height = 1`; while the block at `p − (0, height, 0)` is sugar cane: `height += 1`. (Counts the cane below including itself.)
2. If `height < 3`: if `age == 15`: `setBlockAndUpdate(p + up, sugar cane default (age 0))` **then** `setBlock(p, age = 0, flags 260)`; else `setBlock(p, age+1, flags 260)`.
No random draw at all. **Scheduled tick** (scheduled by the `updateShape` hook with delay 1 when the block cannot survive): if `canSurvive` is false → `destroyBlock(p, drop = true)`.

### 8. Cactus and cactus flower

Cactus: age 0..15, randomly ticking always. **Random tick**: if the block above is air (`above = p + up`):
1. `height = 1`, `age = state.age`; while the block at `p − (0, height, 0)` is cactus: `height += 1`, and **immediately** if the new `height == 3 && age == 15` → return (stop everything).
2. Branch A: `age == 8` **and** cactus `canSurvive(default cactus state, p + up)` (that is: no solid / lava horizontal neighbour of `above`; the block below `above` — i.e. `p` — is cactus; and the block two above is not a liquid):
   `chance = (height ≥ 3) ? 0.25 : 0.1`; draw `nextDouble()`; if `value ≤ chance` (f64 compare, `≤`): `setBlockAndUpdate(above, cactus flower default)`.
   Branch B (else): `age == 15 && height < 3`: `setBlockAndUpdate(above, cactus default)`; `s = state age 0`; `setBlock(p, s, 260)`; then call `neighborChanged(s, above, cactus, no orientation, false)` (cactus has no neighbour-changed
   behaviour, so this is a no-op, kept for hook order).
3. Then, **always** (even after branch A or B) `if age < 15: setBlock(p, age+1, flags 260)` using the age read at the start (so after branch B, `age == 15` is not incremented; after branch A with `age == 8` the state becomes age 9).
**Scheduled tick**: `canSurvive` false → `destroyBlock(p, true)`; scheduled with delay 1 from `updateShape`.
`CactusFlowerBlock` has no tick behaviour of its own (support is the `canSurvive` hook).

### 9. Bamboo (stalk and sapling)

Bamboo stalk: properties `age 0..1` (thin / thick), `leaves none|small|large`, `stage 0..1`. Randomly ticking iff `stage == 0`. Bamboo sapling: always randomly ticking.
**Sapling random tick**: `nextInt(3) == 0 && isEmpty(p + up) && raw(p + up) ≥ 9` → `setBlockAndUpdate(p + up, bamboo default with leaves = small)`.
**Stalk random tick** (`stage == 0`): `nextInt(3) == 0 && isEmpty(p + up) && raw(p + up) ≥ 9`: `height = heightBelow(p) + 1`; if `height < 16`: `growBamboo(state, p, height)`.
`heightBelow(p)` = count of consecutive bamboo stalks directly below `p`, up to 16. `heightAbove(p)` likewise upward, up to 16.
**`growBamboo(state, p, height)`** (draws in order):
1. `leaves = none`. If `height ≥ 1` (always true): if the block below `p` is not a bamboo, or is a bamboo with leaves `none`: `leaves = small`; else (it has leaves): `leaves = large`; and if the block two below `p` is bamboo:
   `setBlockAndUpdate(p − 1, below state with leaves = small)` **then** `setBlockAndUpdate(p − 2, two-below state with leaves = none)`.
2. `age = 1` if `state.age == 1` or the block two below `p` is bamboo; else `0`.
3. `stage`: if `height < 11` → 0 (**no draw**); else draw `nextFloat()`: stage 1 if `value < 0.25f` or `height == 15` (the draw is made for every `height ≥ 11`, including 15); else 0.
4. `setBlockAndUpdate(p + up, bamboo default with age, leaves, stage)` (flags 3).
A stage-1 stalk is "done growing" and stops randomly ticking. **Scheduled tick**: not `canSurvive` (block below not in `supports_bamboo`) → `destroyBlock(p, true)`; scheduled with delay 1 from `updateShape`.

### 10. Growing plants: kelp, weeping vines, twisting vines, cave vines (heads)

Head blocks (kelp growing up, twisting vines up, weeping vines down, cave vines down) have `age 0..25` (cave vines also `berries`). Randomly ticking iff `age < 25`. Bodies (`kelp_plant`, `weeping_vines_plant`, `twisting_vines_plant`, `cave_vines_plant`) do not random tick.
Per-block constants: `growProbability` kelp **0.14**, weeping **0.1**, twisting **0.1**, cave **0.1**; "can grow into" = kelp: the target block is the **water block** (block identity, any level); weeping / twisting / cave vines: the target is air.
**Random tick**: `age < 25 && nextDouble() < growProbability` (the draw is made whenever `age < 25`):
 1. `g = p + growthDirection`; if `canGrowInto(block at g)`: `setBlockAndUpdate(g, growInto(state))` where `growInto(state)` = `state` with `age + 1` (cycle; no wrap since `age < 25`); **cave vines additionally**: `berries = (nextFloat() < 0.11f)` — drawn only when the grow actually happens, after the `nextDouble`.
 2. Otherwise no further draw.
The head's `updateShape` hook converts the old head into a body block (its `age` is dropped) when the new head appears above/below; kelp heads always carry a water source fluid state ("waterlogged by definition").
**Scheduled tick** (all heads and bodies, scheduled with delay 1 by `updateShape` when the support goes away): if not `canSurvive` → `destroyBlock(p, true)`. For water-logged kinds (kelp, kelp plant) `updateShape` also schedules a **water fluid tick** with the fluid's tick delay; fluid details in `../fluids.md`.
Bone meal growth (`performBonemeal`, several blocks) is not tick-driven.

### 11. Chorus flower and plant

Chorus flower: age 0..5; randomly ticking iff `age < 5` (5 = dead). Parameter: the plant block is chorus plant. **Random tick**:
1. `above = p + up`. Proceed only if `isEmpty(above)` **and** `above.y ≤ level.maxY` (the highest valid block Y). Then `age = state.age`; if `age ≥ 5` stop.
2. `growUp = false`, `pillarOnSupport = false`; `below` = block at `p − up`:
   - `below` in tag `supports_chorus_flower` (end stone) → `growUp = true`;
   - else if `below` is chorus plant: `height = 1`; repeat 4 times: `t` = block at `p − (0, height+1, 0)`; if `t` is not chorus plant: if `t` is in `supports_chorus_flower` set `pillarOnSupport = true`; break; else `height += 1`.
     Then: `growUp = (height < 2) || (height ≤ nextInt(pillarOnSupport ? 5 : 4))` (the draw happens **only if `height ≥ 2`**);
   - else if `below` is air → `growUp = true`; (any other block: no growth direction chosen).
3. If `growUp` **and** all four horizontal neighbours of `above` are air **and** `isEmpty(p + (0,2,0))`:
   `setBlock(p, chorus plant state-with-connections at p, flags 2)` (the plant block's connection state is computed from the six neighbours: the `getStateWithConnections` helper, see the plant's `updateShape` hook spec);
   then `placeGrownFlower(above, age)`: `setBlock(above, chorus flower age = age, flags 2)` and `levelEvent 1033` (chorus flower grown sound) at `above`.
4. Else if `age < 4`: `branches = nextInt(4)`, plus 1 if `pillarOnSupport`. `created = false`. For `i` in `0..branches−1`: draw a random horizontal direction `d` (`nextInt(4)`); `t = p + d`;
   if `isEmpty(t)` **and** `isEmpty(t − up)` **and** every horizontal neighbour of `t` **except the one in direction `−d`** is air:
   `placeGrownFlower(t, age + 1)`; `created = true`. After the loop: if `created`: `setBlock(p, chorus plant state-with-connections at p, flags 2)`; else `placeDeadFlower(p)`.
5. Else (`age ≥ 4`, not growing up): `placeDeadFlower(p)`.
`placeDeadFlower(q)`: `setBlock(q, chorus flower age = 5, flags 2)` and `levelEvent 1034` (dead sound).
Note the use of the *state* `age` read at step 1 in both the `age < 4` test and the new flower ages. **Scheduled tick** (flower and plant; delay 1 from `updateShape`): not `canSurvive` → `destroyBlock(p, true)`
(the flower survives on its plant or end stone, or on air with exactly one plant horizontal neighbour and all other horizontal neighbours air; the plant's survival rule is a hook of the plant block). The flower's `updateShape` schedules this tick only for directions other than up.
`generatePlant` / `growTreeRecursive` (the world-generation chorus tree) is world generation, not a tick.

## Edge cases and known quirks

1. **Draw gating.** Crops, stems and pitcher crops draw only after their light tests (pitcher: draws first); bamboo and sweet berries draw first; saplings test light first.
2. **Torchflower crop** reaches its "max age" 2 without a state for it: only through the `getStateForAge` override becoming the flower. A torchflower crop `age = 1` is still randomly ticking.
3. **Beetroot / torchflower** consume `nextInt(3)` on every random tick (even in the dark), so the stream advances differently from wheat.
4. **Mega grower draw before the 2×2 test** (spruce, jungle, dark oak, pale oak): the draw is consumed even when no 2×2 exists. For dark oak / pale oak the normal tree list is empty, so a lone sapling consumes exactly one draw (`nextInt(1)` — one source step) and does nothing.
5. **Failed tree placement** restores the sapling at stage 1, the world otherwise unchanged except for whatever the feature wrote before failing (features check before writing in vanilla tree code; see `features-trees.md`).
6. **Cactus step 3** increments the age *after* branch A (flower chance) — a cactus at age 8 gets exactly one `nextDouble` per random tick while the top is free and `canSurvive` of the above position holds, and one at age 9 does not.
7. **Cactus height rule**: the early return at `height == 3 && age == 15` skips the age increment (age stays 15 forever for a 3rd-or-more cactus).
8. **Sugar cane / cactus counts** look only downward (`height` counts the column below and including `p`), never upward.
9. **Stem fruit placement** does not check `canSurvive` of the fruit; only air + support tag. The attached stem is written even if the fruit write was cancelled by shape updates.
10. **Stale state** (server-tick §10.2d): the `state` argument is the state read before the tick; blocks that modify themselves re-read nothing (all code above writes from that `state` snapshot, e.g. cactus `age + 1`).
11. **Kelp "can grow into water"** means the *block* is the water block (source or flowing, any level); a flowing-water block counts. A water-logged state of another block does not.

## Constants and IDs

Crop gates: light `raw ≥ 9` (wheat family, stems), `local(p+up) ≥ 9` (saplings), `raw(p+up) ≥ 9` (bamboo, sweet berry), `raw(p) ≥ 8` (crop survival / pitcher growth); probabilities: `25/speed` family, saplings `1/7`, bamboo `1/3`, cocoa `1/5`, sweet berry `1/5`, nether wart `1/10`,
beetroot / torchflower skip `1/3` of ticks (they act on `nextInt(3) ≠ 0`), kelp `0.14`, vines `0.1`, cave-vine berries `0.11f`, bamboo stage-1 `0.25f` (for `height ≥ 11`), cactus flower `0.1` / `0.25` (age 8). Max heights: cane 3, cactus 3, bamboo 16. Ages: wheat-like 7, beetroot 3, torchflower 1 (+flower), pitcher 4,
stem 7, cocoa 2, berry 3, wart 3, cactus / cane 15, kelp-like heads 25, chorus flower 5 (4 = last branching age), bamboo age 1 / stage 1. Flags: 2, 3, 260, 818. Level events: 1033, 1034.

## Oracle probe points

- `net.minecraft.world.level.block.CropBlock#getGrowthSpeed(Block, BlockGetter, BlockPos)` (protected static; reflection), `#randomTick`; `BeetrootBlock#randomTick`, `TorchflowerCropBlock#randomTick`, `PitcherCropBlock#randomTick`, `StemBlock#randomTick`,
  `CocoaBlock#randomTick`, `SweetBerryBushBlock#randomTick`, `NetherWartBlock#randomTick`, `SaplingBlock#randomTick` / `#advanceTree`, `MangrovePropaguleBlock#randomTick`, `TreeGrower#growTree`, `SugarCaneBlock#randomTick`, `CactusBlock#randomTick`, `BambooStalkBlock#randomTick` / `#growBamboo`, `BambooSaplingBlock#randomTick`,
  `GrowingPlantHeadBlock#randomTick`, `ChorusFlowerBlock#randomTick`.
- For a draw-level check wrap `RandomSource` with a counting proxy and log `(pos, block, draws)` for each `BlockState#randomTick` (`BlockBehaviour$BlockStateBase#randomTick(ServerLevel, BlockPos, RandomSource)`).

## Open questions

1. **[oracle] Light visible to random ticks.** Crop / bamboo / sapling gates read the *visible* light, which can lag recent changes (`server-tick.md` Open question 4). Probe: place and remove a torch next to a crop, call `getRawBrightness(pos, 0)` on the same tick and the next 3 ticks.
2. **[oracle] Kelp "water" test.** Confirm `canGrowInto` for kelp is `state.is(Blocks.WATER)` (the block) and whether a *falling* flowing-water block (level ≥ 8) counts. Probe: `KelpBlock#canGrowInto(BlockState)` (protected) on `water[level=0..15]`.
3. **[oracle] Weighted list index mapping.** The "flat" selector (total weight < 64) maps the draw to entries in listed order by cumulative weight; the "compact" selector (≥ 64 total, mangrove 15 + 85 = 100) may map differently. Probe: call `WeightedList#getRandom` with a seeded random (`TreeGrower.MANGROVE`'s list) 1000 times and compare against cumulative-weight order.
4. **[oracle] `Feature.place` for tree features during ticking** uses the level random with the *world* generator — confirm that no `WorldgenRandom` is wrapped (the `random` argument passes straight through). Probe: draw counts before/after `TreeGrower#growTree` on a plain oak sapling vs `TreeFeature#place`.
5. (implementer) The `canSurvive` conditions referenced here (support tags, light 8) belong to the hook specs.

## Parity status

Not yet run.
