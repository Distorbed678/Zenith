# Ticks: block reactions — attachables and support rules (torches, signs, buttons, levers, ladders, lanterns, bells, carpets, plates, clusters …) and the waterlogging table (26.3)

- Verified by suites: `block-updates`
- Sources: local reference reading of the 26.3 block classes (2026-11-26, reader session): torch bases, the face-attached base (lever, button, grindstone), ladder, the sign family (standing, wall, ceiling hanging, wall hanging), banners, skulls, lantern, bell,
  chain, rods, amethyst clusters, conduit, candles and cakes, carpets, pressure plates, scaffolding, flower pots, plus every `updateShape` that only schedules the water tick. Everything is in our own words.
- Related specs: [`README.md`](README.md) (index), [`plants.md`](plants.md) (vegetation, vines, corals), [`connections.md`](connections.md), [`multiblock.md`](multiblock.md), [`rails.md`](rails.md), [`misc-reactions.md`](misc-reactions.md),
  [`../redstone-components.md`](../redstone-components.md) (power behaviour of torches, levers, buttons, plates, lightning rods), [`../fluids.md`](../fluids.md) §8, [`../neighbor-and-shape-updates.md`](../neighbor-and-shape-updates.md),
  [`../../blocks/state-transforms.md`](../../blocks/state-transforms.md) (`isFaceSturdy` per support type; `isSolid`), [`../../blocks/state-facts.md`](../../blocks/state-facts.md) (legacy "solid" flag, `isRedstoneConductor`, `isSignalSource`).

## Summary

An **attachable** block remembers which face it hangs from (a property, or a fixed rule such as "the block below") and has a *support predicate* evaluated live. Its `updateShape` turns it into air when **the update arrives from the support side** and the predicate fails;
the framework then drops it (neighbor-and-shape-updates §3.3d). There are **four support predicates** (plus the raw centre test) in the game, and which one a block uses is the key fact:

| predicate | meaning (live read of the support cell `s`) | used by |
|---|---|---|
| `sturdy(s, face)` | `s.isFaceSturdy(sPos, face)` with the default FULL support type | wall torch, ladder, lever, buttons, wall fan / amethyst, tripwire hook, hanging roots, pointed dripstone, ... |
| `center(s, face)` | `Block.canSupportCenter(level, sPos, face)` = `s.isFaceSturdy(sPos, face, CENTER)` (**and** false when `face == DOWN` and `s` is in `tag#unstable_bottom_center`) | torch, lantern, candle, bell (ceiling), spore blossom, pressure plate (alternative) |
| `centerRaw(s, face)` | `s.isFaceSturdy(sPos, face, CENTER)` without the tag exception | ceiling hanging sign, cactus flower (UP) |
| `solid(s)` | `s.isSolid()` = the **legacy solid flag** of `state-facts.md` (not sturdiness): the state's shape/material-derived "solid" boolean | standing sign, wall sign, banner, wall banner, cake, candle cake |
| `rigid(s)` | `s.isFaceSturdy(UP, RIGID)` (`Block.canSupportRigidBlock`) | rails, pressure plates |

`isFaceSturdy(pos, face[, type])` is exact in Zenith already (state-transforms.md); the order of the direction names below tells which face of the **support** cell is tested ("toward the attached block").

## Inputs and state

Hook inputs as in `block-hooks.md` §1 rows 8, 12. `dir` = `directionToNeighbour` (see `plants.md`). `AIR` = return the air state. Water tick = `scheduleTick(pos, WATER, 5)` (fluids.md). **Whether the water tick happens before or after the AIR decision is block-specific and listed in §Waterlogging.**

## Behavior (ordered rules)

### 1. Torches

- **`torch`, `soul_torch`, `copper_torch`, `redstone_torch`** (standing; `BaseTorchBlock`): `canSurvive` := `center(below, UP)`. `updateShape`: `dir == DOWN && !canSurvive` → `AIR`; else the state. (Other directions: nothing.)
- **`wall_torch`, `soul_wall_torch`, `copper_wall_torch`, `redstone_wall_torch`** (`FACING` = the direction the torch points **away from the wall**): `canSurvive` := `sturdy(pos + FACING.opposite, FACING)`. `updateShape`: `dir.opposite == FACING && !canSurvive` → `AIR`; else the state.
- Redstone torches additionally: `onPlace` notifies, `affectNeighborsAfterRemoval` (not moved by a piston) notifies all six neighbours, `neighborChanged` schedules the re-evaluation: [`../redstone-components.md`](../redstone-components.md) §1. Torches have no waterlogging.

### 2. Face-attached levers and buttons (`FaceAttachedHorizontalDirectionalBlock`: `lever`, all buttons, `grindstone`)

Props `FACE` ∈ {FLOOR, WALL, CEILING}, `FACING` (horizontal). **Connected direction** `A`: `CEILING` → DOWN, `FLOOR` → UP, `WALL` → `FACING`. The support cell is `pos + A.opposite`; `canSurvive` := `sturdy(pos + A.opposite, A)`... precisely: `canAttach(level, pos, A.opposite)` = the cell at `pos + A.opposite` has `isFaceSturdy(A)`.
(`FLOOR`: support below, its **UP** face; `CEILING`: support above, its **DOWN** face; `WALL`: support at `pos + FACING.opposite`, its `FACING` face.)

- **`updateShape`**: `A.opposite == dir && !canSurvive` → `AIR`; else the state.
- **Grindstone** overrides `canSurvive` to **always true** → it never pops from shape updates (it uses the same `updateShape`).
- Buttons / lever redstone parts (`press`, `pull`, `tick`, `affectNeighborsAfterRemoval`, `updateNeighbours` of `pos` and `pos + A.opposite`... the neighbour-update targets): [`../redstone-components.md`](../redstone-components.md) §5. Their removal hook runs only if `!movedByPiston && POWERED`.

### 3. Ladders (`LadderBlock`; props `FACING`, `WATERLOGGED`)

`canSurvive` := `sturdy(pos + FACING.opposite, FACING)`. `updateShape`: (1) `dir.opposite == FACING && !canSurvive` → `AIR` (**before** any water tick); (2) if `WATERLOGGED` → `waterTick`; (3) the state.

### 4. Signs, hanging signs, banners, skulls

| block (class) | `canSurvive` | `updateShape` |
|---|---|---|
| `*_sign` standing (`StandingSignBlock`; `ROTATION`, `WATERLOGGED`) | `solid(below)` | `dir == DOWN && !canSurvive` → `AIR`; else the sign base: water tick if waterlogged, state |
| `*_wall_sign` (`WallSignBlock`; `FACING`) | `solid(pos + FACING.opposite)` | `dir.opposite == FACING && !canSurvive` → `AIR`; else water tick if waterlogged, state |
| `*_hanging_sign` ceiling (`CeilingHangingSignBlock`; `ROTATION`, `ATTACHED`) | `above.isFaceSturdy(DOWN, CENTER)` **directly** (no `unstable_bottom_center` exception, unlike `center()`) | `dir == UP && !canSurvive` → `AIR`; else water tick if waterlogged, state |
| `*_wall_hanging_sign` (`WallHangingSignBlock`; `FACING`) | **default true** (no override; the "can place" test is only used at placement) | `dir.axis == FACING.clockWise.axis && !canSurvive` → AIR — **never fires** because `canSurvive` is true; else water tick if waterlogged, state **[oracle]** |
| `*_banner` standing (`BannerBlock`; `ROTATION`) | `solid(below)` | `dir == DOWN && !canSurvive` → `AIR`; else the state (no waterlogging) |
| `*_wall_banner` (`WallBannerBlock`; `FACING`) | `solid(pos + FACING.opposite)` | `dir == FACING.opposite && !canSurvive` → `AIR`; else the state |
| heads / skulls (`SkullBlock`, `WallSkullBlock`, player / piglin / wither variants) | **none** | none (a wall head never pops; only the `POWERED` mirror of redstone-components §8.12) |

The sign family has a block entity (text); removal drops it through the block entity side effects, not through hooks here.

### 5. Lanterns (`LanternBlock`; `HANGING`, `WATERLOGGED`)

Support direction `D` = UP if `HANGING` else DOWN. `canSurvive` := `center(pos + D, D.opposite)` (so a hanging lantern needs the block above to support the **centre of its bottom face**, a standing one the centre of the top face of the block below).
`updateShape`: (1) if `WATERLOGGED` → `waterTick` (**first**); (2) `dir == D && !canSurvive` → `AIR`; else the state.

### 6. Bells (`BellBlock`; `FACING`, `ATTACHMENT` ∈ {FLOOR, CEILING, SINGLE_WALL, DOUBLE_WALL}, `POWERED`)

Support direction `S` (the direction **from the bell to its support**): `FLOOR` → DOWN, `CEILING` → UP, `SINGLE_WALL` / `DOUBLE_WALL` → `FACING` (for wall bells `FACING` points **toward the wall** the bell hangs on: the support cell is `pos + FACING`).
`canSurvive`: if `S == UP` → `center(pos + UP, DOWN)`; else `canAttach(pos, S)` = the cell at `pos + S` has `isFaceSturdy(S.opposite)` FULL (floor bell: the block below's UP face; wall bell: the wall's face toward the bell).

`updateShape` (`dir`, `nb` captured), `S` as above:
1. If `dir == S` **and** `!canSurvive` **and** `ATTACHMENT != DOUBLE_WALL` → `AIR`.
2. If `dir.axis == FACING.axis`:
   - `ATTACHMENT == DOUBLE_WALL` **and** `!nb.isFaceSturdy(nbPos, dir)` (**note: the face tested is `dir`, the face pointing *away* from the bell, not `dir.opposite`**) → `state` with `ATTACHMENT = SINGLE_WALL` and `FACING = dir.opposite`.
   - `ATTACHMENT == SINGLE_WALL` **and** `dir == S.opposite` (the update comes from the side opposite to the support) **and** `nb.isFaceSturdy(nbPos, FACING)` → `state` with `ATTACHMENT = DOUBLE_WALL`.
3. Otherwise the state. `neighborChanged` (ring on rising power; flags 3 write of `POWERED`): redstone-components §8.10.
Note: a double-wall bell has two supports; losing one converts it to a single-wall bell facing away from the lost side and **never** returns air from step 1.

### 7. Chains, rods, conduits

- `*_chain` (all metals; `AXIS`, `WATERLOGGED`), `conduit` (`WATERLOGGED`, default true), `lightning_rod` (all copper stages; `FACING`, `POWERED`, `WATERLOGGED`): `updateShape` = water tick if waterlogged, state. No survival rule.
- `end_rod`: no reactions at all.
- `lightning_rod`: `onPlace` (old block differs): if `POWERED` and no tick scheduled → `scheduleTick(pos, rod, 8)`; `affectNeighborsAfterRemoval`: if `POWERED` → update the neighbours behind the rod; see redstone-components §7.3.
- `conduit`: block-entity logic is tick-side.

### 8. Amethyst clusters (`amethyst_cluster`, `large_/medium_/small_amethyst_bud`; `FACING` any of 6, `WATERLOGGED`)

`canSurvive` := `sturdy(pos + FACING.opposite, FACING)`. `updateShape`: (1) `WATERLOGGED` → `waterTick` (**first**); (2) `dir == FACING.opposite && !canSurvive` → `AIR`; else the state.

### 9. Candles and cakes

- **`*_candle`** (`CandleBlock`; `CANDLES` 1–4, `LIT`, `WATERLOGGED`): `canSurvive` := `center(below, UP)` — **but `updateShape` only schedules the water tick (if waterlogged) and returns the state**: there is **no removal hook**, so a candle never pops from shape updates **[oracle]** (checked only at placement and by the water pickup rule).
  `placeLiquid` (fluids.md §8.3) extinguishes (`setLit false` via flags 11 write) when lit.
- **`*_candle_cake`** (`CandleCakeBlock`; `LIT`): `canSurvive` := `solid(below)`; `updateShape`: `dir == DOWN && !canSurvive` → `AIR`; else the state.
- **`cake`** (`BITES` 0–6): `canSurvive` := `solid(below)`; `updateShape` `dir == DOWN && !canSurvive` → `AIR`; else the state.
- Lighting / extinguishing writes use flags **11** (`1 + 2 + 8`): neighbours notified, clients updated, shape updates run.

### 10. Carpets (`CarpetBlock`: `moss_carpet`; `WoolCarpetBlock`: the sixteen carpets)

`canSurvive` := the block below is **not air** (`isEmptyBlock(pos.below())` false — any non-air block, even a fluid block counts as supporting). `updateShape` (**any dir**): `!canSurvive` → `AIR`; else the state.
(`pale_moss_carpet` is a different class: [`plants.md`](plants.md) §10.)

### 11. Pressure plates (`PressurePlateBlock`, `WeightedPressurePlateBlock`)

`canSurvive` := `rigid(below) || center(below, UP)`. `updateShape`: `dir == DOWN && !canSurvive` → `AIR`; else the state. Pressing, release timing, neighbour updates: redstone-components §6.1–6.3.

### 12. Scaffolding (`ScaffoldingBlock`; `DISTANCE` 0–7 default 7, `BOTTOM`, `WATERLOGGED`)

1. `getDistance(pos)`: `b = pos + DOWN`; `d = 7`; if the block at `b` is scaffolding: `d = its DISTANCE`; else if `b.isFaceSturdy(UP)` (FULL) → return **0**. Then for the four horizontal directions in N, E, S, W order (`HORIZONTAL`): if the block at `pos + dir` is scaffolding: `d = min(d, its DISTANCE + 1)`; if `d == 1` stop.
   (The loop **starts after** the below test; a scaffolding below contributes its own `DISTANCE` (not +1).)
2. `isBottom(pos, d)` := `d > 0 && block below is not scaffolding`.
3. **`onPlace`** (server): `scheduleTick(pos, scaffolding, 1)`. **`updateShape`**: (1) `WATERLOGGED` → `waterTick`; (2) (server) `scheduleTick(pos, scaffolding, 1)`; return the state **unchanged** (every shape update on a server schedules the tick; the tick is ignored if one is pending).
4. **`tick`**: `d = getDistance(pos)`; `new = state with DISTANCE = d, BOTTOM = isBottom(pos, d)`. If `new.DISTANCE == 7`: if the **old** `DISTANCE` was already 7 → `FallingBlockEntity.fall(level, pos, new)` (entity falls; the cell becomes the legacy fluid block, flags 3) else → `destroyBlock(pos, drops = true)`. Else if `state != new` → `setBlockAndUpdate(pos, new)` (flags 3).
   So an unstable scaffold first breaks with drops if it *was* stable (old distance < 7 and new = 7), and **falls** if it was already marked 7.
5. `canSurvive` := `getDistance < 7` (used only at placement / pickup rules). `canBeReplaced` by scaffolding items (item side).
   Because each step writes with flags 3, neighbouring scaffolds are notified and recompute: a distance propagation proceeds one block per game tick.

### 13. Flower pots (`FlowerPotBlock`, `potted_*`)

`updateShape`: `dir == DOWN && !canSurvive` → `AIR` — `canSurvive` is the default **true** (no override), so a pot **never** pops. Eyeblossom pots flip open / closed by random tick (tick-side reader).

### 14. Blocks whose `updateShape` is only the water tick

`light`, `barrier`, `heavy_core`, `decorated_pot`, `sculk_sensor`, `calibrated_sculk_sensor`, `sculk_shrieker`, `dried_ghast`, `copper_golem_statue` (all weathering / wax stages), `campfire` / `soul_campfire` (plus the signal-fire rule of misc-reactions §9.1),
`conduit`, `chain` family, copper grates, `trapdoor` family (22 blocks: `updateShape` = water tick; the powered-state writes also re-schedule the water tick while waterlogged: redstone-components §8.7), `ender_chest`, `mangrove_roots`, slabs, stairs, fences, walls, panes and bars (their other rules are in connections.md).
All use "W-first" (see the table below). They have no survival rule and are never removed by shape updates.

## Waterlogging — which blocks schedule the water tick and when (hook into fluids.md §8)

`fluids.md` §8.2 lists the waterloggable blocks; this is the order inside each `updateShape`. "**W-first**" = the `WATERLOGGED` check and tick happen before any other rule (an AIR result still leaves the water tick scheduled);
"**AIR-first**" = the survival check may return AIR before the water tick (no tick scheduled then).

| class | order |
|---|---|
| slab, stair, fence, wall, bars / glass panes, chain, trapdoor, chest (+ trapped, copper), ender chest, scaffolding, barrier, light, conduit, lightning rod, heavy core, decorated pot, sculk sensor, calibrated sensor, sculk shrieker, shelf, dried ghast, copper golem statue, campfire, copper grates (`WaterloggedTransparentBlock`), rail family, candle, speleothems (dripstone, sulfur spike) | **W-first** (water tick, then the class rule or nothing) |
| multiface (lichen, sculk vein, resin clump) | W-first, then AIR check |
| lantern, amethyst cluster / buds | W-first, then AIR check |
| ladder, hanging roots, big dripleaf (leaf), sea pickle, standing / wall / ceiling signs, alive coral plant / fan / wall fan (water **after** the AIR check, wall fan: AIR, water, die-tick), seagrass | **AIR-first** |
| dead coral plant / fan / wall fan | W-first |
| small dripleaf, mangrove propagule, mangrove roots | W-first (dripleaf: before the double-plant rule) |
| big dripleaf stem | stem tick scheduling first, then water tick |
| `leaves` | water tick, then a block tick (delay 1) if the distance may change (misc-reactions §Leaves) |

Per-block details with all conditions: plants.md §5, §8; connections.md; this file §3–§9. Every case uses the fluid's tick delay for the **level** (5 for water).

## Edge cases and known quirks

1. **`solid(s)` is not sturdiness.** Signs, banners, cake and candle cakes use the legacy `isSolid` flag, so e.g. a stair, slab, glass or leaf block may or may not count regardless of face shape — read the flag from state-facts (`solid+` / `solid-` columns of block-table).
2. **Candles never pop** from shape updates (§9) and wall hanging signs never pop (§4); both are visible gameplay quirks.
3. **Flower pots and grindstones** also never pop from `updateShape`.
4. **Bell wall rules read the wrong face** in the `DOUBLE_WALL` check (§6.2) — tested face is `dir`, i.e. the neighbour's face pointing away from the bell.
5. **Wall torch `updateShape` does not call the base** (returns the plain state otherwise), equivalent because the base default returns the state.
6. **Standing sign / banner removal on `dir == DOWN` only**; horizontal or UP updates never re-check support.
7. **Water tick placement relative to AIR** changes the scheduled-tick set of the level after a break; parity dumps include fluid ticks, so keep the order.

## Constants and IDs

Water tick delay 5; carpet / plate / torch have no delayed removal (immediate AIR). Scaffolding tick 1; scaffolding max distance 7. Flags 11 for candle lighting; flags 3 for tick-driven writes.
Tags: `unstable_bottom_center`, `copper_golem_statues` (hooks listed in misc-reactions).

## Oracle probe points

- Wrap `BlockBehaviour$BlockStateBase#updateShape` and `#canSurvive` for: `BaseTorchBlock`, `WallTorchBlock`, `FaceAttachedHorizontalDirectionalBlock`, `LadderBlock`, `StandingSignBlock`, `WallSignBlock`, `CeilingHangingSignBlock`, `WallHangingSignBlock`, `BannerBlock`, `WallBannerBlock`,
  `LanternBlock`, `BellBlock`, `AmethystClusterBlock`, `CandleBlock`, `CandleCakeBlock`, `CakeBlock`, `CarpetBlock`, `BasePressurePlateBlock`, `ScaffoldingBlock`, `FlowerPotBlock`.
- Scripted probes: remove each support cell with `setBlock(flags 3)` and with `setBlock(flags 2)` and compare the dump of changed blocks + scheduled ticks.

## Open questions

1. **[oracle] Wall hanging signs and candles never pop** (§4, §9): remove the supporting block of an attached wall hanging sign and of a candle (flags 3) and confirm they stay.
2. **[oracle] `isSolid` of specific blocks** used by signs / banners / cake (glass, leaves, slabs, stairs): compare with the `solid` column.
3. **[oracle] Bell double-wall conversion face** (§6.2) with a one-sided supporting block (a bottom slab on the removed side).
4. **[oracle] Scaffold tick order**: with a column of five scaffolds whose base is removed, the order in which they break / fall (one per tick vs all at once).
5. **[oracle] Torch above a fence / wall**: `center(below, UP)` for fences (CENTER support type) — confirm with the state-transforms table.

## Parity status

Not yet run.
