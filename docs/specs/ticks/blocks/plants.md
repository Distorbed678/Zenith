# Ticks: block reactions — plants, crops, vines, corals, eggs (26.3)

- Verified by suites: `block-updates`
- Sources: local reference reading of the 26.3 block classes (2026-11-26, reader session): the vegetation base class and
  every subclass, the growing-plant family (kelp, twisting / weeping / cave vines), double plants, crops and stems,
  farmland, cactus / sugar cane, bamboo, dripleaf family, chorus, corals, sea pickle, eggs and spawn, mangrove propagule,
  moss / hanging moss / leaf litter / flower bed (segmentable), mushrooms. Tag names come from the game's built-in block
  tags (data, read by Zenith from the install at runtime). Everything is in our own words.
- Related specs: [`README.md`](README.md) (index: block → file), [`../block-hooks.md`](../block-hooks.md) (hook table),
  [`../neighbor-and-shape-updates.md`](../neighbor-and-shape-updates.md) (shape-update delivery, `updateOrDestroy`),
  [`../set-block-flags-and-sequence.md`](../set-block-flags-and-sequence.md), [`../scheduled-ticks-and-block-events.md`](../scheduled-ticks-and-block-events.md)
  (one tick per `(block, pos)`), [`../fluids.md`](../fluids.md) (waterlogging, fluid tick delay), [`multiblock.md`](multiblock.md)
  (doors, beds, pointed dripstone), [`misc-reactions.md`](misc-reactions.md), [`../../blocks/state-facts.md`](../../blocks/state-facts.md),
  [`../../blocks/block-table.md`](../../blocks/block-table.md) (block → class).
- Scope: the **world reaction** side only (shape updates, survival checks, `onPlace`, the delayed `tick` that completes a
  survival check). Random-tick growth and spreading are the tick-behaviour reader's; item placement (`getStateForPlacement`)
  is out of scope except where it feeds a state the reactions depend on (marked "placement note").

## Summary

Almost every plant has the same pattern: *a block that needs a support re-checks it whenever **any** neighbour's shape
update arrives* and answers with air if the support is gone (the framework then drops the block through
`destroyBlock`, flags 3, with loot). Two variants exist:

- **Immediate removal:** `updateShape` returns air. The removal happens inside the shape-update chain of the originating
  `setBlock`, in `SHAPE_ORDER` position of the notifying direction.
- **Delayed removal:** `updateShape` only schedules a block tick one game tick later (`scheduleTick(pos, thisBlock, 1)`);
  the block's `tick` repeats the check and calls `destroyBlock(pos, drops = true)` (flags 3, no breaker).
  The delayed variant is what makes sugar cane, cactus, bamboo, chorus, kelp, twisting / weeping / cave vines, big dripleaf stems,
  hanging moss, farmland and others survive a piston push for one tick. Both patterns are visible in tick-by-tick parity.

Vocabulary used below:

- `AIR` = "return the air state from `updateShape`" (framework: `destroyBlock(pos, dropResources, flags 3)`; see
  neighbor-and-shape-updates §3.3d).
- `sched(B, d)` = `scheduleTick(pos, B, d)` for the block B at this position (one per `(block, pos)`; a second call while one
  is pending is ignored).
- `waterTick` = `scheduleTick(pos, WATER fluid, tickDelay(WATER))`; water's tick delay is 5 ([`../fluids.md`](../fluids.md) §constants).
  Only blocks whose state is waterlogged (or that always hold water) do this.
- `below` = `pos + DOWN`, `above` = `pos + UP`; `isFaceSturdy(pos, dir[, type])` is the support-type check of
  [`../../blocks/state-transforms.md`](../../blocks/state-transforms.md) (default type FULL).
- "**dir**" always means `directionToNeighbour` of the hook (where the changed neighbour is **relative to this block**);
  "any dir" = the rule ignores the direction.
- `tag#X` = membership of the block in the built-in block tag `minecraft:X` (read from the installed game's data).

## Inputs and state

Hook inputs are exactly those of `block-hooks.md` §1 rows 7, 8, 12. All rules below read the world *now* (survival checks
re-read neighbours; only `neighbourState`, `directionToNeighbour` come from the update).

## Behavior (ordered rules)

### 1. The vegetation base (`VegetationBlock` family)

1. `canSurvive`: the block **below** is accepted by `mayPlaceOn(belowState, pos.below())` (per-class table in §1.3).
   No light, fluid or other neighbour is read.
2. `updateShape` (**any dir**): if `canSurvive` is false → `AIR`; else the state is returned unchanged. No tick scheduling.
   Waterlogging is **not** handled here (these blocks have no water property; see §5 for the ones that do).
3. `mayPlaceOn` by block / class:

| class (blocks) | `mayPlaceOn(below state)` |
|---|---|
| `BushBlock` base (`bush`, `red_shrub`), `FlowerBlock` (all small flowers, torchflower, open/closed eyeblossom), `TallGrassBlock` (`short_grass`, `fern`), `SaplingBlock` (all saplings), `SweetBerryBushBlock`, `FireflyBushBlock`, `DoublePlantBlock` family default (`tall_grass`, `large_fern`, `sunflower`, `lilac`, `rose_bush`, `peony`) | `tag#supports_vegetation` |
| `DryVegetationBlock` (`dead_bush`, `short_dry_grass`, `tall_dry_grass`) | `tag#supports_dry_vegetation` |
| `WitherRoseBlock` | `tag#supports_wither_rose` |
| `CropBlock` (wheat, carrots, potatoes, beetroots, torchflower crop) | `tag#supports_crops`; plus the light rule of §2.1 |
| `PitcherCropBlock` | `tag#supports_crops` (lower half), §3.3 |
| `StemBlock` (pumpkin / melon stem) | the stem's own support tag (constructor argument; pumpkin and melon stems use the farmland-type crop support tag registered with the block) |
| `AttachedStemBlock` | the attached-stem support tag (constructor argument; same tag as the stem's) |
| `MushroomBlock` | `state.isSolidRender()` of the below state (**full opaque cube**), §2.4 |
| `NetherWartBlock` | `tag#supports_nether_wart` |
| `NetherRootsBlock` (`crimson_roots`, `warped_roots`) | the block's own tag (constructor argument; the nylium/roots support tag of its colour) |
| `NetherSproutsBlock` | `tag#supports_nether_sprouts` |
| `NetherFungusBlock` (`crimson_fungus`, `warped_fungus`) | the block's own tag (constructor argument) |
| `AzaleaBlock` (`azalea`, `flowering_azalea`) | `tag#supports_azalea` |
| `LilyPadBlock` | §4.1 (fluid-aware) |
| `SeagrassBlock`, `TallSeagrassBlock` | below `isFaceSturdy(UP)` **and** not `tag#cannot_support_seagrass` |
| `SeaPickleBlock` | below has a **non-empty collision face on UP** *or* `isFaceSturdy(UP)` |
| `CactusFlowerBlock` | below in `tag#support_override_cactus_flower`, or below `isFaceSturdy(UP, CENTER)` |
| `MangrovePropaguleBlock` (standing) | `tag#supports_mangrove_propagule` |
| `SmallDripleafBlock` | `tag#supports_small_dripleaf`, **or** (the fluid at the *dripleaf's own position*, i.e. `pos.above()` of the support, is a **source of water** **and** the double-plant default `tag#supports_vegetation` accepts the support) |
| `LeafLitterBlock`, `FlowerBedBlock` (`pink_petals`, `wildflowers`) | leaf litter: below `isFaceSturdy(UP)` (own `canSurvive`, not `mayPlaceOn`); flower bed uses the vegetation default `tag#supports_vegetation` |

   (Which concrete block ids use which class is in `README.md` and `block-table.md`.)
4. `propagatesSkylightDown` for vegetation = "fluid state is empty" — a state fact, already in `state-facts.md`.

### 2. Crops, stems, mushrooms, farmland

1. **`CropBlock.canSurvive`**: `rawBrightness(pos, 0) >= 8` **and** the vegetation rule (§1.1). `rawBrightness(pos, 0)` is the
   combined sky/block light at the crop position with **zero skylight subtraction** (it ignores time of day). So crops pop
   when their own cell becomes dark (a light change does **not** trigger a shape update by itself: the check runs the next time
   any neighbour shape update reaches the crop). `TorchflowerCropBlock`, `PotatoBlock`, `CarrotBlock`, `BeetrootBlock` inherit it.
2. **`StemBlock`** (pumpkin / melon stems): only the vegetation rule (no light rule). **`AttachedStemBlock.updateShape`**:
   if `directionToNeighbour == state.FACING` **and** the neighbour state is **not** the fruit block of this stem pair → return the
   *unattached stem* of the pair with `AGE = 7` (the attached stem reverts when its fruit disappears; no drops
   difference — a state change, not air); otherwise the vegetation rule (§1.2). Order: fruit check first, then support.
   The fruit block is the pumpkin (resp. melon) of the pair; "is" tests the block type only.
3. **`PitcherCropBlock`** — see §3.3 (double).
4. **`MushroomBlock`** (`brown_mushroom`, `red_mushroom`) `canSurvive`: if the block below is in `tag#overrides_mushroom_light_requirement`
   (mycelium, podzol, nylium family) → **true**; otherwise `rawBrightness(pos, 0) < 13` **and** `below.isSolidRender()`.
   The same method is used by the random-tick spreading (spec of the other reader).
5. **`FarmlandBlock`**:
   - `canSurvive`: the block above is **not** `isSolid()` (the legacy-solid flag of `state-facts.md`), **or** the block above is in
     `tag#maintains_farmland`.
   - `updateShape`: if `directionToNeighbour == UP` **and** `!canSurvive` → `sched(farmland, 1)`; the state is returned
     unchanged (no AIR). Other directions: nothing.
   - `tick` (the scheduled tick): `if !canSurvive` → *turn to base block* (dirt): compute the new state `dirt`, push every entity in the
     collision difference up (`pushEntitiesUp`: entities overlapping the *new* shape part that the old shape did not occupy are
     teleported up by `1 + collide-offset` — entity side), write it with `setBlockAndUpdate` (flags 3), then emit game event `BLOCK_CHANGE`.
   - Placement note: placing farmland where `canSurvive` is false produces dirt instead.
   - Trampling (`fallOn`) and moisture are entity / random-tick side; they end in the same *turn to base block* routine.
6. **`NetherWartBlock`**, **`SweetBerryBushBlock`**, **`CocoaBlock`**: `CocoaBlock.canSurvive`: the block at `pos + FACING` (the log it hangs on) is in
   `tag#supports_cocoa`; `updateShape`: if `directionToNeighbour == FACING` and `!canSurvive` → `AIR`, else the state (other
   directions do not re-check). Nether wart and the berry bush use only §1.

### 3. Double plants and their halves

1. **`DoublePlantBlock.updateShape`** (state has `HALF`; applies to `tall_grass`, `large_fern`, `sunflower`, `lilac`, `rose_bush`, `peony`, `pitcher_plant`
   (a `DoublePlantBlock`, no crop), `tall_seagrass`, `small_dripleaf`, `pitcher_crop`):
   a. If `directionToNeighbour` is **vertical** (UP/DOWN) **and** this half *faces its partner* — i.e. (`half == LOWER` and dir == UP) or
      (`half == UPPER` and dir == DOWN) — **and** the neighbour state is **not** "this same block with the *other* half" (i.e. the neighbour is not `this` block,
      or it is `this` block with the **same** half) → return `AIR` (partner missing or wrong).
   b. Otherwise: if `half == LOWER` and `dir == DOWN` and `!canSurvive` → `AIR`; else the vegetation rule §1.2 (which calls `canSurvive` again and
      returns `AIR` if false, else the state).
   Net effect: the lower half dies when the block below is bad (checked in (b) **and** again by the super rule); the upper half dies when it has no lower half of the same
   block *below it* (via (a) when the update comes from DOWN, and via `canSurvive` of (b)→super for any direction).
2. **`canSurvive`**: `UPPER`: the block below is **this block** with `HALF == LOWER`. `LOWER`: the vegetation rule of §1 for the block (its `mayPlaceOn`).
   `TallSeagrassBlock`: `UPPER` as above; `LOWER`: vegetation rule **and** the fluid at `pos` is **water and full** (`isFull`, source/level-8 height).
   `SmallDripleafBlock`: `UPPER` → `DoublePlantBlock` rule; `LOWER` → `mayPlaceOn(below)` of the dripleaf (§1.3).
3. **`PitcherCropBlock`** (overrides the shape update and `canSurvive`):
   - `updateShape`: if `age >= 3` (the *double* stage; both halves exist) → the `DoublePlantBlock` rule (§3.1); else (age 0–2, a single lower block) →
     `canSurvive ? state : AIR` (direction ignored).
   - `canSurvive`: if the state is the **lower** half **and** `rawBrightness(pos, 0) < 8` → false; otherwise the `DoublePlantBlock.canSurvive` (§3.2 with `tag#supports_crops`).
     The upper half is never checked against light.
   - `canBeReplaced` is false and `setPlacedBy` places nothing (the upper half is created by growth: `setBlock(lower, age, flags 2)` then
     `setBlockAndUpdate(upper, state with UPPER, flags 3)`).
4. **Waterlogged copies:** when the engine places both halves (`DoublePlantBlock.placeAt`, `setPlacedBy`) the `WATERLOGGED` property of each half (if the block has it:
   `small_dripleaf`) is set from `isWaterAt(thatPos)` for each half separately. `setPlacedBy` writes the upper half with `setBlockAndUpdate` (flags 3).
5. `TallSeagrassBlock` is always a water-holding block (fluid state = water source) and refuses liquid placement (`canPlaceLiquid` false, `placeLiquid` false).
   It never schedules fluid ticks in its own `updateShape`; see §5.2 for `SeagrassBlock` / `kelp`.

### 4. Aquatic and wet-surface plants

1. **`LilyPadBlock.mayPlaceOn`** (called with the support position): the fluid at the **support** is in `fluid_tag#supports_lily_pad` **or** the support block is in
   `tag#supports_lily_pad`, **and** the fluid at `support + UP` (the lily pad's own cell) is **empty** (`Fluids.EMPTY`).
   `updateShape` = §1.2 (any dir; AIR).
2. **`SeagrassBlock`** (`seagrass`, always water):
   - `updateShape`: apply §1.2; **if the result is not air** also `waterTick` (the fluid at the cell is a water source: the block's fluid state is always water).
   - It has a `canSurvive` = §1 with its `mayPlaceOn`. (Bonemeal growth places `tall_seagrass`, tick-side.)
3. **Sea pickle** (`SeaPickleBlock`, props `PICKLES 1–4`, `WATERLOGGED`):
   - `canSurvive`: `mayPlaceOn(below)` (§1.3).
   - `updateShape` (**any dir**): `!canSurvive` → `AIR`; else if `WATERLOGGED` → `waterTick`; return the state.
   - A pickle is "dead" when not waterlogged (`isDead`); it keeps its states otherwise. Placement note: `WATERLOGGED` is set from whether the replaced fluid is **a water source**.
4. **`KelpBlock` / `KelpPlantBlock`** — see §6 (growing plants).
5. **Frogspawn** (`FrogspawnBlock`):
   - `canSurvive`: the **support** (`pos.below()`) satisfies `mayPlaceOn`: (fluid at support in `fluid_tag#supports_frogspawn` **or** the support block in `tag#supports_frogspawn`) **and** the fluid at `support + UP` is empty.
   - `onPlace` (every write, no old-block guard): `scheduleTick(pos, frogspawn, rand)` with `rand = 3600 + level.random.nextInt(8400)` (the bounded `nextInt(origin 3600, bound 12000)`; one draw from the **level** random on every call, even if a tick is already pending).
   - `updateShape` (**any dir**): `!canSurvive` → `AIR`, else the state.
   - `tick`: if `!canSurvive` → `destroyBlock(pos, drops = false)`; else hatch: `destroyBlock(pos, false)`, hatch sound, then tadpoles (entity side). (The destroy happens **before** the sound and the spawns.)

### 5. Waterlogged-plant tick scheduling (hook into fluids.md)

Blocks with a `WATERLOGGED` property schedule the water fluid tick from their own `updateShape` whenever `WATERLOGGED` is true, in addition to their survival rule.
The *order* inside each hook is listed here because the AIR early-exit decides whether the water tick is scheduled:

| block | order in `updateShape` |
|---|---|
| `hanging_roots` | (1) `dir == UP` and `!canSurvive` → AIR; (2) if waterlogged → `waterTick`; (3) state |
| `small_dripleaf` | (1) waterlogged → `waterTick` (**before** the double-plant logic); (2) `DoublePlantBlock.updateShape` of §3.1 |
| `big_dripleaf` | (1) `dir == DOWN` and `!canSurvive` → AIR; (2) waterlogged → `waterTick`; (3) `dir == UP` and neighbour is a big dripleaf → the **stem** with the same properties (§7.1); else state |
| `big_dripleaf_stem` | (1) `dir` is UP **or** DOWN and `!canSurvive` → `sched(stem, 1)`; (2) waterlogged → `waterTick`; (3) state |
| `sea_pickle` | §4.3 |
| `mangrove_propagule` | (1) waterlogged → `waterTick`; (2) `dir == UP` and `!canSurvive` → AIR; (3) the vegetation/sapling rule (§1.2 → AIR if standing and `!canSurvive`) — **both** the (2) and (3) checks use the same `canSurvive` |
| `mangrove_roots` | waterlogged → `waterTick`; state |
| corals (all) | §8 |
| `seagrass`, `kelp`, `kelp_plant` | §4.2, §6 (always water; condition is "result not air" / `scheduleFluidTicks = true` flag, not a property) |
| `tall_seagrass` | no fluid tick of its own (it is a `DoublePlantBlock` without the water hook); relies on neighbours' water (the fluid state is always source water) |
| other waterlogged non-plants | see `survival-and-supports.md` §Waterlogging table |

All fluid-state facts (`getFluidState` = water source when `WATERLOGGED`) are already in `state-facts.md`.

### 6. Growing plants: kelp, twisting vines, weeping vines, cave vines

Two block kinds per plant: a **head** (has `AGE` 0–25, grows) and a **body** (plant block, no age). Parameter table:

| plant | head block | body block | growth dir | `scheduleFluidTicks` | head support tag / `canAttachTo` |
|---|---|---|---|---|---|
| kelp | `kelp` | `kelp_plant` | UP | **true** | `canAttachTo(state) = !tag#cannot_support_kelp` |
| twisting vines | `twisting_vines` | `twisting_vines_plant` | UP | false | any |
| weeping vines | `weeping_vines` | `weeping_vines_plant` | DOWN | false | any |
| cave vines | `cave_vines` (also `BERRIES`) | `cave_vines_plant` (also `BERRIES`) | DOWN | false | any |

(`canAttachTo` is asked of the **support** block; the body block asks the head block's version.)

1. **`canSurvive`** (head and body, `GrowingPlantBlock`): let `s = pos + growthDir.opposite` (the cell the plant hangs from / stands on) and `S = state at s`.
   False if `!canAttachTo(S)`. Else true if `S` is the head block **or** the body block of this plant; else true iff `S.isFaceSturdy(s, growthDir)` (the face of `S`
   that points toward the plant must be sturdy, FULL type).
2. **Head `updateShape`** (growth dir G):
   a. If `dir == G.opposite` (the support side): if `!canSurvive` → `sched(head, 1)`; **else** (survives): let `n = state at pos + G`; if `n` is the head block *or* the body block
      → return the **body state** (the head becomes a body when something grows above it): `bodyDefault`, then `updateBodyAfterConvertedFromHead` — for cave vines the body copies
      the head's `BERRIES`; for the others it is the plain default body state (no other property is copied; the `AGE` is dropped).
   b. Then: if `dir != G` **or** the neighbour is neither the head nor the body: if `scheduleFluidTicks` → `waterTick`; return `super` (the state unchanged).
   c. Else (dir == G and the neighbour *is* head/body): return the body state as in (a).
   Net: the head turns into body when the cell in growth direction holds head/body (either from a shape update coming from G or from the support-side shape update that
   re-checks that cell).
3. **Body `updateShape`**: (a) if `dir == G.opposite` and `!canSurvive` → `sched(body, 1)`. (b) if `dir == G` and the neighbour is neither this body nor the head → return a **head state**:
   `headBlock.getStateForPlacement(random)` = default head with `AGE = random.nextInt(25)` — **one draw from the update's random source** (the level random on the server; this draw
   is consumed even when the final state equals the old one — it cannot) — with `updateHeadAfterConvertedFromBody` (cave vines: head copies the body's `BERRIES`; others: plain).
   (c) else if `scheduleFluidTicks` → `waterTick`; return the state. (a) happens in addition to (b)/(c): after (a), execution continues to (b), then (c).
4. **`tick`** (both): `!canSurvive` → `destroyBlock(pos, true)` (flags 3, drops).
5. Kelp and kelp plant are always waterlogged (`getFluidState` = water source) and reject liquid placement. `kelp` head: placement note `AGE` random 0–24 via `getStateForPlacement(random)`.
6. Berry harvest and growth of the cave vines head are tick / interaction side.

### 7. Dripleaf family and other special survival rules

1. **`big_dripleaf`** (props `FACING`, `TILT`, `WATERLOGGED`): `canSurvive`: below is a big dripleaf, a big-dripleaf **stem**, or in `tag#supports_big_dripleaf`.
   `updateShape` order: §5 table. The conversion in step (3): `dir == UP` and the neighbour is also `big_dripleaf` → state of `big_dripleaf_stem` carrying the **same** shared property values
   (`FACING`, `WATERLOGGED`; `TILT` is dropped) — i.e. a leaf that gets another leaf grown on top becomes a stem.
   `neighborChanged`: if the block has a redstone signal (`hasNeighborSignal(pos)`: any neighbour provides power, quasi-connectivity **not** included) → `resetTilt`: write `TILT = NONE`
   (flags 2) and, if the state before had `TILT != NONE`, play the tilt-up sound (note: `setTilt` is called first, then the sound test reads the **old** `state` argument). `tick`: same signal test first
   (→ reset), else advance `UNSTABLE→PARTIAL` (10 ticks later `PARTIAL→FULL` (100) …): `UNSTABLE → PARTIAL` + `scheduleTick(pos, leaf, 10)`; `PARTIAL → FULL` + `scheduleTick(.., 100)`;
   `FULL → NONE`; setting a tilt writes the state with flags 2 and emits `BLOCK_CHANGE` game event if the new tilt `causesVibration` and differs from the old one.
   (The entity-triggered tilt-start is entity side.)
2. **`big_dripleaf_stem`** (props `FACING`, `WATERLOGGED`): `canSurvive`: (below is a stem **or** in `tag#supports_big_dripleaf`) **and** (above is a stem **or** a big dripleaf). Shape / tick as §5; `tick`: `!canSurvive` → `destroyBlock(pos, true)`.
   Unlike the leaf, the stem checks both vertical directions and delays one tick.
3. **`spore_blossom`**: `canSurvive`: `canSupportCenter(above, DOWN)` — the block above supports the CENTER of its bottom face, except that blocks in `tag#unstable_bottom_center` never do for DOWN — **and** `!isWaterAt(pos)` (any water in the cell, including flowing).
   `updateShape`: `dir == UP` and `!canSurvive` → `AIR`; else the state (the water check is therefore only re-evaluated by an UP-direction update).
4. **`hanging_roots`**: `canSurvive`: the block above `isFaceSturdy(DOWN)` (FULL). §5 table for the shape update.
5. **`pale_hanging_moss`** (`TIP` property): `canStayAtPosition`: the block above can host (`MultifaceBlock.canAttachTo(level, UP, abovePos, aboveState)`: face of the above block toward DOWN is sturdy per the multiface rule, see
   `connections.md` §Multiface) **or** the block above is the hanging moss itself. `canSurvive` = that.
   `updateShape` (**any dir**): `if !canStayAtPosition` → `sched(moss, 1)`; then **return the state with `TIP = !(block below is hanging moss)`** (unconditionally recomputed; may change the state and cause a write without
   neighbour notification). `tick`: `!canStayAtPosition` → `destroyBlock(pos, true)`.
6. **`pale_moss_carpet`** (`MossyCarpetBlock`; props `BASE` bool, `NORTH/EAST/SOUTH/WEST` ∈ {NONE, LOW, TALL}): see §10.
7. **`moss_carpet`**, other carpets: [`survival-and-supports.md`](survival-and-supports.md).
8. **`leaf_litter`** (segmentable, `FACING`, `SEGMENT_AMOUNT` 1–4): `canSurvive`: below `isFaceSturdy(UP)` (FULL), via the vegetation `updateShape` (§1.2, any dir → AIR).
   **`pink_petals`, `wildflowers`** (`FlowerBedBlock`, `FACING`, `FLOWER_AMOUNT` 1–4): the vegetation rule with `tag#supports_vegetation`.
9. **`cactus`**: `canSurvive`: for each horizontal direction `d` (N, E, S, W order, any returns false): the neighbour at `pos + d` `isSolid()` (legacy solid flag) **or** its fluid is in `fluid_tag#lava` → false;
   then true iff (below is a cactus **or** `tag#supports_cactus`) **and** the block above is **not** `liquid()` (legacy liquid flag: water/lava/bubble column).
   `updateShape` (**any dir**): `!canSurvive` → `sched(cactus, 1)`; state returned. `tick`: `!canSurvive` → `destroyBlock(pos, true)`.
10. **`cactus_flower`**: vegetation rule (§1.1/1.2) with the CENTER-support `mayPlaceOn` of the table.
11. **`sugar_cane`**: `canSurvive`: below is sugar cane → true; else if below in `tag#supports_sugar_cane`: for horizontal `d` in N,E,S,W of the **below** position `b = pos.below()`: the fluid at `b + d` in
    `fluid_tag#supports_sugar_cane_adjacently` **or** the block at `b + d` in `tag#supports_sugar_cane_adjacently` → true; else false. `updateShape`/`tick` pattern exactly as cactus (delayed removal).
    Do **not** confuse with the lily pad: sugar cane does not check the cell above.
12. **`bamboo_sapling`**: `canSurvive`: below in `tag#supports_bamboo`. `updateShape` (**immediate**): `!canSurvive` → `AIR`; else if `dir == UP` and the neighbour is `bamboo` → the **bamboo** default state (the sapling becomes the first stalk); else state.
13. **`bamboo`** (props `AGE` 0–1, `LEAVES`, `STAGE`): `canSurvive` as the sapling. `updateShape`: first, if `!canSurvive` → `sched(bamboo, 1)`; then `dir == UP` and the neighbour is `bamboo` with `AGE` > this `AGE` → `state.cycle(AGE)` (0→1; a thick stalk above makes this thick); else the state. `tick`: `!canSurvive` → `destroyBlock(pos, true)`.
14. **`chorus_plant`** (`PipeBlock`: `NORTH/EAST/SOUTH/WEST/UP/DOWN` booleans) — connection and survival:
    - `canSurvive`: let `blockAboveOrBelow = (above is not air) and (below is not air)`. For `d` in N, E, S, W: if `pos + d` is a chorus plant: if `blockAboveOrBelow` → **false**; else if the block below `pos + d` is a chorus plant or in `tag#supports_chorus_plant` → **true**. After the loop: below is a chorus plant or in `tag#supports_chorus_plant`.
      (Note the loop returns on the *first* qualifying horizontal neighbour in N,E,S,W order.)
    - `updateShape`: if `!canSurvive` → `sched(plant, 1)`; return the state **unchanged** (no connection update in that case). Else: `connect = neighbour is chorus plant or chorus flower, or (dir == DOWN and neighbour in tag#supports_chorus_plant)`; return `state.setValue(property of dir, connect)` (only the property of the notified direction changes).
    - `tick`: `!canSurvive` → `destroyBlock(pos, true)`.
    - Placement connection helper `getStateWithConnections(level, pos, defaultState)`: `DOWN` = below is the plant / a chorus flower / in `tag#supports_chorus_plant`; the other five = the neighbour is the plant or a chorus flower.
15. **`chorus_flower`** (`AGE` 0–5): `canSurvive`: below is the chorus plant or in `tag#supports_chorus_flower` → true; else (if below is not air → false) count horizontal neighbours: each neighbour must be a chorus plant or air; **exactly one** chorus plant neighbour → true; zero or ≥ 2 → false.
    `updateShape`: if `dir != UP` and `!canSurvive` → `sched(flower, 1)`; return the state. `tick`: `!canSurvive` → `destroyBlock(pos, true)`.

16. **`shelf_mushroom`** (`ShelfMushroomBlock`; `FACING` horizontal = the direction the fungus points **away from** its support, `AGE` 0–1): `canSurvive` := the block at `pos + FACING.opposite` has `isFaceSturdy(FACING)` (FULL). `updateShape`: `dir.opposite == FACING` **and** `!canSurvive` → `AIR`; else the state.
    (Growth by random tick / bonemeal is tick-side; the fall-on bounce is entity side.)

### 8. Corals (`BaseCoralPlantTypeBlock` family)

Properties `WATERLOGGED` (default **true**); wall fans also `FACING`. Alive and dead variants:

| alive class (blocks) | dead class (blocks) |
|---|---|
| `CoralPlantBlock` (`<c>_coral`) | `BaseCoralPlantBlock` (`dead_<c>_coral`) |
| `CoralFanBlock` (`<c>_coral_fan`) | `BaseCoralFanBlock` (`dead_<c>_coral_fan`) |
| `CoralWallFanBlock` (`<c>_coral_wall_fan`) | `BaseCoralWallFanBlock` (`dead_<c>_coral_wall_fan`) |
| `CoralBlock` (`<c>_coral_block`) | `Block` (`dead_<c>_coral_block`) — no reactions |

`scanForWater(state, pos)`: true if `WATERLOGGED`, else true iff any of the **six** neighbours' fluid state is in `fluid_tag#water` (order irrelevant). For the **coral block** (cube, no property) the same scan without the property.

1. **`canSurvive`** (plant, fan, standing): `below.isFaceSturdy(UP)` (FULL). **Wall fans**: `pos + FACING.opposite` `isFaceSturdy(FACING)` (the face pointing toward the fan).
2. **Dead plant / dead fan (standing)** `updateShape`: (1) if `WATERLOGGED` → `waterTick`; (2) if `dir == DOWN` and `!canSurvive` → `AIR`; else the state.
3. **Alive plant / alive standing fan** `updateShape`: (1) `dir == DOWN` and `!canSurvive` → `AIR`; (2) `tryScheduleDieTick`: if `!scanForWater` → `scheduleTick(pos, thisBlock, 60 + random.nextInt(40))` (**one draw** of the update's random source, only when there is no water); (3) if `WATERLOGGED` → `waterTick`; state.
4. **Dead wall fan** `updateShape`: (1) `WATERLOGGED` → `waterTick`; (2) `dir.opposite == FACING` and `!canSurvive` → `AIR`; else the state.
5. **Alive wall fan** `updateShape`: (1) `dir.opposite == FACING` and `!canSurvive` → `AIR`; (2) `WATERLOGGED` → `waterTick`; (3) `tryScheduleDieTick` (**after** the water tick); state.
6. **`onPlace`** (alive plant, standing fan, wall fan): `tryScheduleDieTick` with the level random (draw only if no water).
7. **`tick`** (alive plant / fan / wall fan): if `!scanForWater` → `setBlock(pos, deadVariant.default with WATERLOGGED = false [and FACING copied for wall fans], flags 2)`. Dead variants have no `tick`.
   (`setBlock` flags 2 = client sync only, no neighbour notification, shape updates run (bit 16 clear).)
8. **Alive coral block** `updateShape` (any dir): if `!scanForWater` → `scheduleTick(pos, block, 60 + random.nextInt(40))` (draw only when dry); state. `tick`: if `!scanForWater` → `setBlock(pos, deadBlock default, flags 2)`.
   Placement note: placing a coral block with no adjacent water schedules the same tick (level random draw).
9. A coral (alive or dead) never reacts to an UP neighbour except through the always-run water / die ticks.

### 9. Eggs

1. **`turtle_egg`** (props `HATCH` 0–2, `EGGS` 1–4): **no** `canSurvive`, **no** `updateShape`, **no** `neighborChanged`. `onPlace` (**every write**, server): if the block below is in `tag#sand`, `levelEvent(2012, pos, 15)` (happy-villager effect) — so each hatch-stage or egg-count write repeats the event. Hatching and stomping are tick / entity side.
2. **`sniffer_egg`** (`HATCH` 0–2): no survival rule. `onPlace` (**every state write**, no old-block guard): `boosted = below in tag#sniffer_egg_hatch_boost`; server: if `boosted` → `levelEvent(3009, pos, 0)`; game event `BLOCK_PLACE` (context: the new state, no entity);
   then `scheduleTick(pos, egg, hatchTime/3 + level.random.nextInt(300))` with `hatchTime = boosted ? 12000 : 24000` (i.e. 4000 or 8000 plus a draw 0–299). Order: event → game event → draw+schedule. The scheduled `tick` advances `HATCH` with a flags-2 write (tick-side reader); **that write's `onPlace` schedules the next stage** — this is how the three stages chain, each stage re-drawing `nextInt(300)` and re-emitting the boost event and the `BLOCK_PLACE` game event.
3. **`dried_ghast`**: see `misc-reactions.md`.

### 10. Pale moss carpet (`MossyCarpetBlock`; the only block of the class)

State: `BASE` (bottom slab present) and the four side properties ∈ {NONE, LOW, TALL}.

1. `canSurvive`: if `BASE`: the block below is **not air** (any block); else (a side-only cell): the block below is the pale moss carpet **with `BASE` true**.
2. `updateShape` (**any dir**): (a) `!canSurvive` → `AIR`; (b) compute `newState = getUpdatedState(state, level, pos, createSides = false)`; (c) if `newState` has no faces (`BASE` false **and** all four sides NONE) → `AIR`; else return `newState`.
3. `getUpdatedState(state, pos, createSides)`: `createSides |= state.BASE`. For each horizontal `d` in N, E, S, W order: `canSupportAtFace(d)` = the neighbour at `pos + d` can be attached to on its face toward the carpet (`MultifaceBlock.canAttachTo(level, pos, d)` for a horizontal d; UP never). `side = canSupportAtFace ? (createSides ? LOW : currentSide(d)) : NONE`.
   Then if `side == LOW`: (i) if the block **above** is the pale moss carpet **and** its `d`-side is not NONE **and** the block above has `BASE == false` → `side = TALL`; (ii) if this state has `BASE == false` and the block **below** is a pale moss carpet whose `d`-side is NONE → `side = NONE`. Set the property, continue. (The above/below cell states are read lazily, once per call.)
   Summary of the per-side rule: with `createSides` false a side that is currently NONE stays NONE; a side that is currently **LOW** is re-tested by (i) and (ii) and may become TALL or NONE; a side that is currently **TALL** is *not* re-tested by (i) or (ii) (`side` already differs from LOW), so it stays TALL while `canSupportAtFace` holds. A side whose support is gone becomes NONE. With `createSides` true (placement, or `BASE == true`) every supportable side starts from LOW and then goes through (i)/(ii). The block above / below are each read lazily at most once per call.
4. Placement-only helpers (`setPlacedBy`, `placeAt`, `createTopperWithSideChance`) are item / worldgen side.

### 11. Segmentable and flower-bed blocks

`leaf_litter`, `pink_petals`, `wildflowers` share the segment-amount rule (`1..4`) for **replacement by the same item** (placement) only. They have no reaction beyond §1.

## Edge cases and known quirks

1. **Direction-agnostic rules fire for every neighbour.** The plain vegetation rule re-checks survival on *all six* shape updates of a neighbour change. A crop in the dark pops when *any* neighbour block changes, not only the soil.
2. **Dark crops do not pop on their own** (light changes send no shape update); they pop at the next unrelated neighbour change or when a random tick sees `rawBrightness < 8` is **not** checked — the random tick only grows when `>= 9`.
3. **Waterlogged ordering matters for tick queues.** `small_dripleaf` schedules the water tick *before* it may return AIR; `hanging_roots` and `big_dripleaf` only after the AIR early-exit; `sea_pickle` after. A fluid tick scheduled and then followed by AIR is harmless but visible in the scheduled-tick state dump.
4. **Delayed vs immediate removal** differs in drops timing: delayed (cactus / sugar cane / bamboo / chorus / kelp / vines / farmland / stems of dripleaf / hanging moss) removal runs next tick; a piston that pushes the support and the plant back in the same tick restores survival before the tick runs.
5. **Head → body conversion consumes no RNG, body → head consumes `nextInt(25)`** from the shape-update random (level random on the server). The draw happens whenever the condition (dir == G, neighbour not body/head) holds, which includes a fresh body with air above it **at every such update** (the result is a *new head state* different from the body state, so the write always happens).
6. **Kelp / seagrass `waterTick` on any update that leaves a block**, including updates that arrive from the vertical neighbours, so they never stay stale without a fluid tick pending.
7. **`FarmlandBlock.updateShape` ignores non-UP directions** and never returns AIR; `canSurvive` is only enforced by the tick.
8. **`Cocoa` and `ShelfMushroom` checks are direction-specific**; the others in this file are not.
9. **Pitcher crop upper half** has no light rule; the lower half's light rule lives in `canSurvive` and is therefore evaluated by `updateShape` only for ages 0–2 (single block) and via the double-plant path (the `canSurvive` of the *lower* half is evaluated for a LOWER half in b of §3.1 only when `dir == DOWN`, and again in the super call for any direction).
10. **`MushroomBlock.canSurvive` reads `isSolidRender`** (a derived cache fact), not `isFaceSturdy`; slabs, stairs, glass never support mushrooms; mycelium/podzol/nylium bypass both light and shape.

## Constants and IDs

- Delayed-removal delay 1 (`sched(B, 1)`); coral die delay `60 + nextInt(40)`; frogspawn hatch `nextInt(3600, 12000)`; sniffer-egg `(12000|24000)/3 + nextInt(300)`; kelp / vine head age bound `nextInt(25)`; water tick via fluid tick delay (5).
- Tags used (all `minecraft:` block tags unless noted): `supports_vegetation`, `supports_dry_vegetation`, `supports_wither_rose`, `supports_crops`, `supports_nether_wart`, `supports_nether_sprouts`, `supports_azalea`, `supports_cocoa`,
  `supports_bamboo`, `supports_cactus`, `supports_sugar_cane`, `supports_sugar_cane_adjacently`, `supports_small_dripleaf`, `supports_big_dripleaf`, `supports_mangrove_propagule`, `supports_hanging_mangrove_propagule`, `supports_chorus_plant`, `supports_chorus_flower`,
  `supports_lily_pad`, `supports_frogspawn`, `support_override_cactus_flower`, `overrides_mushroom_light_requirement`, `maintains_farmland`, `cannot_support_kelp`, `cannot_support_seagrass`, `unstable_bottom_center`, `sand`, `sniffer_egg_hatch_boost`.
  Fluid tags: `water`, `lava`, `supports_lily_pad`, `supports_frogspawn`, `supports_sugar_cane_adjacently`.
  (The tag membership files are data the engine reads from the install; Zenith must not copy them.)

## Oracle probe points

- Hook wrappers per `block-hooks.md` §Oracle probe points; log `(block, pos, dir, neighbourState) → returned state` for each block listed above.
- Classes: `VegetationBlock#updateShape` / `#canSurvive` / `#mayPlaceOn`, `DoublePlantBlock#updateShape`, `GrowingPlantHeadBlock#updateShape`, `GrowingPlantBodyBlock#updateShape`, `GrowingPlantBlock#canSurvive`,
  `BaseCoralPlantTypeBlock#updateShape` and subclasses, `MossyCarpetBlock#getUpdatedState` (private; probe through `updateShape`), `ChorusPlantBlock#canSurvive`, `CropBlock#canSurvive`, `FarmlandBlock#updateShape` / `#tick`.
- Tick queue: `ServerLevel#scheduleTick(BlockPos, Block, int)` / `#scheduleTick(BlockPos, Fluid, int)` log (to confirm which hooks schedule what).
- RNG: wrap `RandomSource#nextInt(int)` of the level random around `updateShape` for coral / vine bodies to confirm draw counts.

## Open questions

1. **[oracle] Pale moss carpet TALL re-evaluation** (§10.3): probe `updateShape` on a carpet whose side is already TALL when the carpet above is replaced by air: does the side stay TALL or return to LOW? From the rule as read, a TALL side survives as TALL.
2. **[oracle] `rawBrightness(pos, 0)` semantics for crops** at night with a block light of 8–9: confirm `getRawBrightness(pos, 0)` returns `max(blockLight, skyLight - 0)` (the second argument is the *sky darkening subtraction*, 0 here).
3. **[oracle] Tall-plant upper half on `dir == UP` update** with a non-plant block above: confirm no AIR (rule §3.1a only fires for the partner direction).
4. **[oracle] Stem support tags**: confirm that pumpkin and melon stems share the same support tag as farmland-type crops and the attached stems use the same tag (the constructor receives them from the registration code, not visible in the class).
5. **[oracle] `FlowerPotBlock`** (class `FlowerPotBlock`) has an `updateShape` with `canSurvive` default true → never air; probe to confirm a potted plant cannot be removed by a missing support.
6. **[oracle] Sea-pickle `mayPlaceOn` collision-face check** uses the support's *collision shape face on UP being non-empty*; probe with soul sand / honey / lily pad / carpets as supports.
7. **[oracle] Order inside `MangrovePropaguleBlock.updateShape`** (water tick then AIR) vs `SaplingBlock`'s inherited vegetation rule; probe a waterlogged propagule whose support is broken.
8. (implementer) Keep the **draw order** of any `random` use inside `updateShape` (corals, vine bodies) identical; the dump compares RNG state.

## Parity status

Not yet run.
