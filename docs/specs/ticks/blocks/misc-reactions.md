# Ticks: block reactions — snow, leaves, ice, gravity blocks, sponge, fire, portal, containers and other one-off reactions (26.3)

- Verified by suites: `block-updates`
- Sources: local reference reading of the 26.3 block classes (2026-11-26, reader session): snow layer and the snowy / spreading-snowy bases, leaves and their particle subclasses, ice and frosted ice, the falling-block base with sand / gravel / concrete powder / anvil / brushable blocks,
  the note block, path block, sponge and wet sponge, fire / soul fire, the nether portal block and its shape helper, campfire, beehive, composter, carved pumpkin (golem patterns), potent sulfur, creaking heart, target block, the container blocks' removal hooks, plus the
  breadth-first traversal helper. Everything is in our own words.
- Related specs: [`README.md`](README.md) (index), [`plants.md`](plants.md), [`survival-and-supports.md`](survival-and-supports.md), [`connections.md`](connections.md), [`multiblock.md`](multiblock.md), [`rails.md`](rails.md),
  [`../redstone-components.md`](../redstone-components.md) (powered consumers: lamp, note block power, dispenser, hopper, TNT, command blocks, …), [`../fluids.md`](../fluids.md) (water tick, bubble column, waterlogging),
  [`../pistons.md`](../pistons.md), [`../scheduled-ticks-and-block-events.md`](../scheduled-ticks-and-block-events.md), [`../neighbor-and-shape-updates.md`](../neighbor-and-shape-updates.md), [`../set-block-flags-and-sequence.md`](../set-block-flags-and-sequence.md),
  `tick-index.md` and `tick-growth.md` / `tick-spread.md` in this folder (random-tick and scheduled-tick behaviour, written by the other reader).
- Scope: reactions to the *world*: shape updates, neighbour updates, `onPlace`, removal hooks and the delayed ticks they schedule. The random-tick halves are summarised only where the reaction itself needs them.

## Summary

This file collects the blocks whose reactions do not fit the families of the other files:

- **State-follows-neighbour blocks** (snowy grass, leaves distance, note block instrument, campfire signal, potent sulfur, concrete powder).
- **Gravity blocks:** every shape update schedules a tick; the tick makes the block fall.
- **Absorbing / replacing blocks:** sponge, wet sponge, carved pumpkin (golems), fire and the portal block.
- **Removal-hook only blocks:** all containers (comparator notification), beehive, shelves.

Vocabulary as in [`plants.md`](plants.md): `AIR`, `sched(B, d)`, `waterTick`.

## Behavior (ordered rules)

### 1. Snowy blocks and snow layers

1. **`SnowyBlock`** (`podzol`; bases of `grass_block` and `mycelium` via `SpreadingSnowyBlock`; prop `SNOWY`): `updateShape`: `dir == UP` → state with `SNOWY = (nb in tag#snow)`; every other direction → the state. Placement sets `SNOWY` from the block above in the same way.
   (The random-tick revert-to-dirt and spreading are in tick-spread.md; the rule `canStayAlive` there: the block above is `snow` with exactly 1 layer → true; else if the above fluid is a **full** fluid → false; else the light dampening from above into this block `< 15`.)
2. **`snow`** (layers `LAYERS` 1–8; `SnowLayerBlock`): `canSurvive`: the block below in `tag#cannot_support_snow_layer` → **false**; else in `tag#support_override_snow_layer` → **true**; else (the below block's collision shape has a **full UP face**) **or** (the below block is `snow` with `LAYERS == 8`).
   `updateShape` (**any dir**): `!canSurvive` → `AIR`; else the state. (Melting by block light > 11 is a random tick: `dropResources` then `removeBlock(pos, false)` + game event.)
3. **`powder_snow`**: no hooks (only bucket pickup: `setBlock(pos, air, flags 11)` + level event 2001). **Layered cauldrons / snow block drips**: tick-side.

### 2. Leaves (all `*_leaves`; props `DISTANCE` 1–7, `PERSISTENT`, `WATERLOGGED`)

1. `distanceAt(state)`: `0` if the block is in `tag#prevents_nearby_leaf_decay` (logs); else its `DISTANCE` if it has the property (leaves); else **7** (no property).
2. **`updateShape`**: (a) if `WATERLOGGED` → `waterTick`; (b) `d = distanceAt(nb) + 1` using the **captured** neighbour; **if `d != 1` or `state.DISTANCE != d`** → `sched(leaves, 1)`; (c) return the state **unchanged** (the distance is rewritten by the tick, never in `updateShape`).
   (So when a log is adjacent and `DISTANCE` is already 1 there is no tick; every other neighbour — including a non-leaf block, `d = 8` — schedules one.)
3. **`tick`**: `setBlockAndUpdate(pos, state with DISTANCE = recomputed)` (flags 3; unconditional call, no-op if identical). **Recompute**: start `d = 7`; for each direction in `Direction.values()` order (DOWN, UP, NORTH, SOUTH, WEST, EAST): `d = min(d, distanceAt(neighbour at pos + dir) + 1)` (live reads); **stop early when `d == 1`**. Result ∈ 1…7.
   Because the write has flag 1, neighbouring leaves get `neighborChanged` (no hook) and **shape updates** (flag 16 clear) → they schedule their own ticks: distance propagates one block per game tick.
4. **Decay** (random tick; here for completeness): if `!PERSISTENT && DISTANCE == 7` → `dropResources` then `removeBlock(pos, false)`.
5. Placement: `PERSISTENT = true`, `WATERLOGGED` from the replaced fluid, then the distance recomputation of §2.3.
6. Mangrove leaves are the same class family (no extra reaction); particle logic is client-side.

### 3. Ice and frosted ice

1. **`ice`**, **`frosted_ice`** share `melt(state, pos)`: if the environment attribute `WATER_EVAPORATES` at `pos` is true → `removeBlock(pos, false)`; else `setBlockAndUpdate(pos, water)`, then **`level.neighborChanged(pos, water block, null)`** (a Simple neighbour update delivered to the water block itself at `pos` with the source block = water), then game event `BLOCK_DESTROY` (old state). (`ice` melts in its random tick when `blockLight > 11 − lightDampening`; `blue_ice`, `packed_ice` have no behaviour.)
2. **`frosted_ice`** (`AGE` 0–3):
   - `onPlace` (**every write**, no old-block guard): `scheduleTick(pos, frosted, Mth.nextInt(level.random, 60, 120))` = `level.random.nextInt(61) + 60` (one draw).
   - `neighborChanged(sourceBlock)`: if `sourceBlock`'s default state **is frosted ice** and `fewerNeighboursThan(pos, 2)` → `melt`. (Then the base `neighborChanged`, a no-op.) `fewerNeighboursThan(pos, k)`: counts the six neighbours that are frosted ice; false as soon as the count reaches `k`.
   - `tick`: `if random.nextInt(3) == 0 || fewerNeighboursThan(pos, 4)`: `brightness` = (dimension is The End ? block light at `pos` : `getMaxLocalRawBrightness(pos)`); `if brightness > 11 − AGE − lightDampening` **and** `slightlyMelt(state, pos)` → for each of the six neighbours (`Direction.values()` order): if it is frosted ice and `!slightlyMelt(neighbour)` → `sched(neighbour, nextInt(20, 40))`; **return**. Otherwise (any other path) `sched(pos, frosted, nextInt(random, 20, 40))`.
     `slightlyMelt(state, pos)`: `AGE < 3` → `setBlock(pos, AGE + 1, flags 2)`, return false; else `melt`, return true. (Note: the neighbours' `slightlyMelt` ages them as a side effect of the condition.)
   - Placement by frost walker is entity/enchant side.

### 4. Falling blocks and gravity (`FallingBlock`: `sand`, `red_sand`, `gravel`, concrete powders, `anvil` / `chipped_` / `damaged_anvil`, `dragon_egg`; `BrushableBlock`: `suspicious_sand`, `suspicious_gravel`; speleothems and scaffolding, see their own sections)

1. **`onPlace`** (every write): `scheduleTick(pos, this, 2)`. **`updateShape`** (**any dir**): `sched(this, 2)`; return the state. (Only one tick per `(block, pos)`; extras are ignored.)
2. **`tick`**: if `isFree(below)` and `pos.y >= level.minY`: `FallingBlockEntity.fall(level, pos, state)` (creates the entity with the state — `WATERLOGGED` cleared if present —, **then** `setBlockAndUpdate(pos, state.fluidState.createLegacyBlock())`, then adds the entity), then the class's `falling(entity)` hook (anvil: hurts entities with 2.0 per block, max 40).
   `isFree(s)` := air, **or** `tag#fire`, **or** the legacy liquid flag, **or** the replaceable flag.
3. **Brushable blocks** (`suspicious_*`): `onPlace` and `updateShape` use delay **2** the same way; `tick`: block entity `checkReset`, then if `isFree(below)` and `y >= minY`: `fall(...)` and `entity.disableDrop()` (drops disabled).
4. **`concrete_powder`** (16 colours): `updateShape` (**any dir**): `touchesLiquid(pos)` → **the concrete block's default state** (a block change through the shape-update write); else the falling-block rule (`sched(self, 2)`, state).
   `touchesLiquid(pos)`: iterate `Direction.values()` in order (DOWN, UP, N, S, W, E): for `DOWN` the step is entered only if the powder's own cell has water (never in practice); for every other direction: `nb = state at pos + dir`; if `nb`'s fluid state is **water** and **not** `nb.isFaceSturdy(pos, dir.opposite)` (the position passed is the powder's own, irrelevant for ordinary shapes) → **true**. So water on a side or above solidifies it unless that neighbour presents a sturdy face; water **below** never does.
   Landing (`onLand` of the falling entity): `shouldSolidify(pos, replacedBlock)` := the replaced block's fluid is water, or `touchesLiquid(pos)` → `setBlockAndUpdate(pos, concrete)`.
5. **`anvil`** has no shape-update rule of its own (falls through §4.1).
6. **`dragon_egg`**: same `FallingBlock` base; its teleport-on-click logic is interaction side.

### 5. Note block (`INSTRUMENT`, `NOTE`, `POWERED`)

- **`updateShape`**: only when `dir` is **vertical** (UP or DOWN): return `state` with `INSTRUMENT` recomputed from **live** reads (the captured neighbour is ignored): `above = state at pos + UP`; if `above.instrument.worksAboveNoteBlock` (i.e. its instrument is a **mob-head or custom-head** kind) → that instrument; else `below.instrument`; if it `worksAboveNoteBlock` → `HARP`, else it.
  Horizontal updates change nothing. `NOTE` and `POWERED` are untouched.
- `neighborChanged` / `playNote`: redstone-components §8.2 (the above-air test there).
- `state.instrument` is the per-block instrument fact of `block-table.md` (`instrument` column).

### 6. Dirt path and farmland

- **`dirt_path`**: `canSurvive`: the block above is **not** `isSolid()` **or** is a fence gate block. `updateShape`: `dir == UP && !canSurvive` → `sched(path, 1)`; the state. `tick`: **unconditionally** *turn to base block* (dirt): `pushEntitiesUp`, `setBlockAndUpdate(pos, dirt)`, game event `BLOCK_CHANGE` — it does **not** re-check `canSurvive` (so a scheduled tick converts even if the block above went away again).
- **`farmland`**: [`plants.md`](plants.md) §2.5 (the tick re-checks).

### 7. Sponge and wet sponge

1. **`sponge`**: `onPlace` (only if the **old block is not sponge**): `tryAbsorbWater(pos)`. **`neighborChanged`** (any neighbour change): `tryAbsorbWater(pos)`.
2. **`tryAbsorbWater(pos)`**: run `absorb(pos)` (below); if it returns true → `setBlock(pos, wet_sponge, flags 2)` and the absorb sound.
3. **`absorb(start)`** — breadth-first traversal of the six-neighbourhood with **max depth 6** and **max count 65**, visiting in a FIFO queue, expanding neighbours in `Direction.values()` order (DOWN, UP, NORTH, SOUTH, WEST, EAST), each position visited at most once:
   Node processing for position `p` (the start position is always *accepted* without checks):
   - if `p`'s **fluid state is not in `fluid_tag#water`** → `SKIP` (not counted, not expanded);
   - else if the block at `p` is a bucket-pickup block **and** `pickupBlock(null, level, p, state)` returns a non-empty item stack → `ACCEPT` (this call **already performed the removal**: for a water source block `setBlock(p, air, flags 11)`; for a waterlogged block it clears `WATERLOGGED` through fluids.md §8.1, which may destroy the block with drops if it cannot survive without water);
   - else if the block is a liquid block (flowing water, level ≠ 0) → `setBlockAndUpdate(p, air)` (flags 3) → `ACCEPT`;
   - else if the block is **not** `kelp`, `kelp_plant`, `seagrass` or `tall_seagrass` → `SKIP`; else `dropResources` (with the block entity if any), `setBlockAndUpdate(p, air)` (flags 3) → `ACCEPT`.
   `ACCEPT` increments the count; if `count >= 65` the traversal stops and returns the count; nodes at depth 6 are processed but not expanded. `absorb` returns `count > 1` (the start itself counts 1).
   Removed water cells are written **during** the search: later nodes see the earlier removals. Every write has flag 1, so neighbouring blocks (including the sponge, which re-enters `neighborChanged` → another `tryAbsorbWater`) are notified **inside** the traversal through the chained updater.
4. **`wet_sponge`**: `onPlace` (**every write**): if `WATER_EVAPORATES` at `pos` → `setBlockAndUpdate(pos, sponge)`, level event 2009, a dry-out sound with one `nextFloat` draw: `(1 + r·0.2) × 0.7` pitch (`r` from the level random).

### 8. Fire and portal

1. **`fire`** (`FireBlock`; `AGE` 0–15, `NORTH/EAST/SOUTH/WEST/UP` booleans) — `canBurn(state)` := the (tick-side) ignite-odds table value for the state's block is `> 0`, and **0 for any waterlogged state**.
   - `canSurvive`: below `isFaceSturdy(UP)` **or** `isValidFireLocation(pos)` (some of the six neighbours `canBurn`).
   - `updateShape` (**any dir**): `canSurvive ? stateWithAge(pos, state.AGE) : AIR`; `stateWithAge(pos, age)` = `getState(pos)` (soul fire if the block below is in `tag#soul_fire_base_blocks`, else the fire placement state) with the age applied **only if the result is fire**. So a shape update **rewrites the side properties** (`NORTH…UP`) from the neighbours' `canBurn` (when the below block is neither burnable nor sturdy) or resets them to the default (below burnable or sturdy), and turns fire into soul fire when it now stands on a soul base block.
   - fire placement state: if the below block `!canBurn` and is not UP-sturdy: default state with each of the five side properties = `canBurn(neighbour)`; else the default state.
   - `onPlace` (**old block differs for the portal part only**): the `BaseFireBlock` hook: if the old block is **not** the same block: **(a)** if the dimension is the Overworld or The Nether: `findEmptyPortalShape(pos, preferredAxis = X)` (§8.3); if present → `createPortalBlocks` and **return** (the fire cell itself is overwritten by the portal block); **(b)** else if `!canSurvive` → `removeBlock(pos, false)`.
     Then (fire only) **always** `scheduleTick(pos, fire, 30 + level.random.nextInt(10))` — the delay is drawn from the level random on **every** `onPlace` call (every state write of a fire block, including `AGE` changes), even when a tick is already pending and the new one is ignored; this also happens when the portal branch returned early.
   - `tick` / burning / spreading: tick-side (tick-index).
2. **`soul_fire`**: `canSurvive` := the block below in `tag#soul_fire_base_blocks`; `updateShape` (**any dir**): `canSurvive ? soul fire default : AIR`; `onPlace`: the `BaseFireBlock` hook only (portal check, `canSurvive` removal), **no** scheduled tick (soul fire does not tick).
3. **`PortalShape`** (rectangle of `nether_portal_frame`-tagged blocks around empty cells; used by fire `onPlace` and by the portal block's shape update). For an axis `a` ∈ {X, Z}: `rightDir` = WEST for X, SOUTH for Z.
   - *empty cell* := air, a block in `tag#fire`, or a `nether_portal` block. *frame* := block in `tag#nether_portal_frame`.
   - `bottomLeft`: from `pos`, move **down** while the cell below is empty and `y > max(minY, pos.y − 21)`; then `edge = distanceUntilEdgeAboveFrame(pos, leftDir = rightDir.opposite) − 1`; if `edge < 0` → no shape; else `bottomLeft = pos + leftDir·edge`.
   - `distanceUntilEdgeAboveFrame(p, dir)`: for `w = 0..21`: `q = p + dir·w`; if `q` is not empty: return `w` if it is a frame else stop (0); else if the block **below** `q` is not a frame → stop (0). If the loop ends → 0.
   - `width = distanceUntilEdgeAboveFrame(bottomLeft, rightDir)`, valid iff `2 ≤ width ≤ 21`, else 0.
   - `height` (with a count of existing portal blocks): for `h = 0..20`: row cells `bottomLeft + UP·h + rightDir·(−1)` and `…·width` must be frames, else return `h`; each of the `width` cells must be empty (else return `h`) and each `nether_portal` among them increments `portalCount`; if the loop completes return 21. Accept iff `3 ≤ height ≤ 21` **and** the top row (`h = height`, all `width` cells) is frame; else height = 0.
   - `isValid` := `2 ≤ width ≤ 21 && 3 ≤ height ≤ 21`; `isComplete` := valid and `portalCount == width·height`; *empty shape* := valid and `portalCount == 0`.
   - `findEmptyPortalShape(pos, preferredAxis)`: try `findAnyShape(preferredAxis)`; if it is an empty valid shape use it; else try the other axis.
   - `createPortalBlocks`: for every cell of the rectangle (`bottomLeft` to `bottomLeft + UP·(height−1) + rightDir·(width−1)`, in `BlockPos.betweenClosed` order) `setBlock(cell, nether_portal[AXIS = axis], flags 18 = 2 + 16)`: client sync, **no shape updates and no neighbour notification**.
4. **`nether_portal`** (`AXIS` ∈ {X, Z}): **`updateShape`**: `uAxis = dir.axis`; `wrongAxis = AXIS != uAxis && uAxis is horizontal`. If `!wrongAxis` **and** the neighbour (captured) is **not** a nether portal **and** `!findAnyShape(pos, AXIS).isComplete()` → `AIR`; else the state.
   (So the portal collapses when a frame block or an interior cell changes along its plane, and a vertical update with a non-portal neighbour re-checks the whole frame. Updates from the perpendicular horizontal axis are ignored.)

### 9. Campfire, beehive, composter, target, potent sulfur, creaking heart, carved pumpkin

1. **`campfire` / `soul_campfire`** (`LIT`, `SIGNAL_FIRE`, `WATERLOGGED`, `FACING`): `updateShape`: (1) `waterTick` if waterlogged; (2) `dir == DOWN` → state with `SIGNAL_FIRE = (nb is hay_block)` (captured neighbour); else the state.
2. **`beehive`, `bee_nest`**: `updateShape` (any dir): if the **captured neighbour block is a `fire` block** (`FireBlock` instance) and the hive has a block entity → release all bees in the *emergency* mode (entity side); return the state.
3. **`composter`** (`LEVEL` 0–8): `onPlace` (every write): if `LEVEL == 7` → `scheduleTick(pos, composter, 20)`. `tick`: if `LEVEL == 7` → `setBlockAndUpdate(pos, LEVEL cycled → 8)` + sound.
4. **`target`** (`OUTPUT_POWER`): `onPlace` (**old block differs**, server): if `OUTPUT_POWER > 0` and no tick scheduled → `setBlock(pos, OUTPUT_POWER = 0, flags 18)`. `tick`: if `OUTPUT_POWER != 0` → `setBlockAndUpdate(pos, OUTPUT_POWER = 0)`.
5. **`potent_sulfur`** (`STATE` ∈ {DRY, WET, DORMANT, ERUPTING, CONTINUOUS}; block entity): **`updateShape`** (**any dir**) returns `valid(state, pos)`:
   - if the fluid at `pos + UP` is **not a water source** → `DRY`;
   - else with `below = pos + DOWN`: if `below` is in `tag#causes_continuous_geyser_eruptions` and (if it holds a fluid, that fluid is a source: `isSourceIfFluid`) → `CONTINUOUS`;
   - else if `below` is in `tag#causes_periodic_geyser_eruptions` and (same fluid test): **if the current `STATE` is neither `ERUPTING` nor `DORMANT`** and the block entity exists → `resetCountdown()` on it (a side effect inside `updateShape`); result = the state itself if it is `ERUPTING`, else `DORMANT`;
   - else → `WET`.
   `onPlace` (every write): base `onPlace`; if `STATE` is `ERUPTING` or `CONTINUOUS`: `blockEvent(pos, block, 0, 0)`, the start sound (continuous vs eruption), game event `BLOCK_ACTIVATE`.
6. **`creaking_heart`**: `updateShape` (any dir): `sched(heart, 1)`; state. `tick`: `new = updateState(state, pos)`; if `new != state` → `setBlockAndUpdate(pos, new)`: `updateState`: if the required logs around it are present **and** `STATE == UPROOTED` → `AWAKE` if the `CREAKING_ACTIVE` attribute is true at `pos`, else `DORMANT`; else unchanged. (`removal hook`: comparator notification.)
7. **`carved_pumpkin`, `jack_o_lantern`**: `onPlace` (**old block differs**): `trySpawnGolem(pos)`: patterns are matched with the **placed pumpkin as the top**: snow golem (`pumpkin`/carved above two `snow_block`s), iron golem (the T of four `iron_block`s with a pumpkin on top and air in the four corners of the 3×3), copper golem
   (carved pumpkin above one `tag#copper` block; the copper block is replaced by the corresponding copper **chest** with `setBlock(…, flags 2)`); order tried: snow, iron, copper; on a match the pattern blocks are cleared and the entity is created (entity milestone). **[oracle]** clear order / flags.
8. **`wither_skeleton_skull`, `soul` heads** / **`dragon_egg`**: no reaction here.

### 10. Containers and removal hooks

These blocks do nothing on neighbour updates but call `updateNeighbourForOutputSignal(pos, thisBlock)` (neighbor-and-shape-updates §2.6) in **`affectNeighborsAfterRemoval`** (any block change at `pos`, not only removal) and have their contents dropped by the block entity's `preRemoveSideEffects` (set-block §3.7):

`furnace`, `blast_furnace`, `smoker`, `barrel`, `hopper`, `chiseled_bookshelf`, `jukebox`, `brewing_stand`, `shulker_box` (all 17), `decorated_pot`, `creaking_heart`, `copper_golem_statue` (all 8; also `shouldChangedStateKeepBlockEntity` for `tag#copper_golem_statues`), chests (connections.md §8), shelves (connections.md §9), `crafter` / `dispenser` / `dropper` (redstone-components §8.3 / §8.5), `lectern` (if `POWERED`: update below, redstone-components §7.4), `sculk_sensor` / calibrated (if `ACTIVE` → update neighbours; redstone-components §7.6).
`barrel` / chests / ender chests also have a block `tick` (open-count recheck, entity-side).

### 11. Bubble columns, liquid blocks, pistons, observers, command blocks

- `bubble_column`: [`../fluids.md`](../fluids.md) §11 (its `updateShape` always schedules the water tick; it schedules a block tick of delay 5 when it cannot survive, or on updates from DOWN, or from UP with a non-column that can hold a column).
- `water`, `lava`: fluids.md §3, §10.
- `piston_base`, `piston_head`, `moving_piston`: pistons.md.
- `observer`: redstone-components §4. `command_block` & variants, `structure_block`, `test_block`: redstone-components §8.14–8.15.

## Edge cases and known quirks

1. **Leaves never change `DISTANCE` in `updateShape`**; the write is in `tick` and always uses flags 3, so a chain of leaves updates one layer per tick and each write notifies neighbours.
2. **Concrete powder** changes block during `updateShape` (not in a tick); it solidifies on the very update chain that places a neighbouring water. Water below the powder does not count.
3. **Falling blocks schedule a tick on every shape update** (including from *horizontal* neighbours), so a sand block next to a changing block will tick (and then not fall); parity dumps show the tick queue.
4. **Sponge `neighborChanged` runs the absorption search on every neighbour change** — also from the sponge's own writes in the traversal (re-entrant through the chained updater: queued if a chain is running).
5. **Fire's `updateShape` can convert fire ↔ soul fire** and rewrites side flags; it returns AIR only when it cannot survive at all.
6. **Frosted ice** ages neighbours as a side effect of the melt test (§3.2).
7. **Path blocks** convert on their tick even if the block above has been removed again.
8. **`potent_sulfur.updateShape` has a side effect** on the block entity (`resetCountdown`).
9. **`wet_sponge.onPlace` runs on every state write** (it has no old-block guard) — harmless because the state has no properties.
10. **Containers' removal hook runs also when the block is replaced by another block of the same container class** only if the block type changes (set-block §3.8): same-type state changes do not call it.

## Constants and IDs

Falling tick delay 2; fire delay `30 + nextInt(10)`; frosted ice delays `nextInt(60..120)` (onPlace) and `nextInt(20..40)`; sponge BFS depth 6, count cap 65; leaves `DISTANCE` 7 = decay; portal shape width 2–21, height 3–21, search up to 21; golem patterns; composter tick delay 20.
Tags: `snow`, `cannot_support_snow_layer`, `support_override_snow_layer`, `prevents_nearby_leaf_decay`, `fire`, `soul_fire_base_blocks`, `nether_portal_frame`, `copper`, `causes_continuous_geyser_eruptions`, `causes_periodic_geyser_eruptions`, `copper_golem_statues`, `wooden_shelves`.

## Oracle probe points

- Wrap `LeavesBlock#updateShape` / `#tick`, `SnowyBlock#updateShape`, `SnowLayerBlock#updateShape`, `FallingBlock#updateShape` / `#tick`, `ConcretePowderBlock#updateShape`, `NoteBlock#updateShape`, `PathBlock#updateShape` / `#tick`, `SpongeBlock#tryAbsorbWater` / `#removeWaterBreadthFirstSearch`, `FrostedIceBlock#tick` / `#slightlyMelt`,
  `FireBlock#updateShape` / `#canSurvive`, `BaseFireBlock#onPlace`, `PortalShape#findAnyShape` / `#isComplete`, `NetherPortalBlock#updateShape`, `PotentSulfurBlock#updateShape`, `CampfireBlock#updateShape`, `CarvedPumpkinBlock#trySpawnGolem`, `BlockPos#breadthFirstTraversal`.
- Probes: sponge in a 3-D pocket of water of > 65 cells (the cut-off cell order); leaves with all combinations of log / leaf / air neighbours; concrete powder beside water / waterlogged stairs / water below; portal frame with one frame block removed and with one interior block placed.

## Open questions

1. **[oracle] Sponge BFS cut-off** (§7.3): with more than 65 reachable water cells, which cells are removed (queue order) and whether the sponge then still turns wet.
2. **[oracle] Frosted-ice neighbour ageing** (§3.2): confirm that `slightlyMelt(neighbour)` in the loop condition is evaluated (and ages the neighbour) for every neighbouring frosted ice block.
3. **[oracle] Portal collapse** (§8.4): the order of `findAnyShape` checks and whether `isComplete` ever sees `portalCount` of **fire** cells (fire counts as empty but not as a portal block).
4. **[oracle] Golem clearing order and flags** (§9.7): which cells are cleared first and with which flags (the block-pattern helper was not read in full).
5. **[oracle] Leaves tick scheduling** (§2.2): the case `d == 1` and `state.DISTANCE == 1` with a *waterlogged* leaf.
6. **[oracle] Note block above-instrument** (§5): a note block under a mob head, then the head is removed.

## Parity status

Not yet run.
