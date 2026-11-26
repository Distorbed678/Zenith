# Ticks: block reactions — connection blocks: fences, walls, panes and bars, stairs, chests, shelves, vines and multiface, mushroom blocks (26.3)

- Verified by suites: `block-updates`
- Sources: local reference reading of the 26.3 block classes (2026-11-26, reader session): the cross-collision base (fences, bars, panes), fence gate, wall, stairs, chest family (plain, trapped, ender, copper), the huge-mushroom
  block, the pipe base, the multiface base with glow lichen / sculk vein / resin clump, vine, the shelf with its side-chain interface, the tripwire pair. Everything is in our own words.
- Related specs: [`README.md`](README.md), [`plants.md`](plants.md) (chorus plant connections §7.14, hanging moss §7.5, pale moss carpet §10), [`multiblock.md`](multiblock.md), [`../redstone-components.md`](../redstone-components.md) §6.6–6.7 (tripwire, hook), §8.13 (shelf power),
  [`../neighbor-and-shape-updates.md`](../neighbor-and-shape-updates.md), [`../fluids.md`](../fluids.md) §8, [`../../blocks/shapes.md`](../../blocks/shapes.md) (face shapes, `getFaceShape`, full-block test), [`../../blocks/state-transforms.md`](../../blocks/state-transforms.md) (sturdiness),
  [`../../blocks/state-facts.md`](../../blocks/state-facts.md).
- Scope: the *state recomputation* from neighbours (`updateShape`) and the one-time effects of removal / placement of these blocks. Placement-time connection (`getStateForPlacement`) is listed only where the reaction reuses the same predicate.

## Summary

Connection blocks have no survival rule; their `updateShape` recomputes **only the property of the notified direction** (fences, bars, tripwire, chorus) or **a function of the neighbours** (stairs, walls). The recomputation uses the **`neighbourState` captured by the shape
update** for the notified direction (so a stale capture is observable) but **live world reads** for everything else. Wall and stair updates also read other cells live.

Common helpers:

- `isException(state)` (`Block.isExceptionForConnection`): the block is a **leaves block** (any `LeavesBlock` subclass), or `barrier`, `carved_pumpkin`, `jack_o_lantern`, `melon`, `pumpkin`, or in `tag#shulker_boxes`. Such blocks never count as "solid face" connections.
- `faceSolid(nb, nbPos, d)` := `nb.isFaceSturdy(nbPos, d)` with the default FULL support type; `d` = the direction of the face of the neighbour that points back at the connecting block.
- `waterTick` as in [`plants.md`](plants.md) (water fluid tick at the fluid tick delay, 5).
- `PROPERTY_BY_DIRECTION`: N → `NORTH`, E → `EAST`, S → `SOUTH`, W → `WEST` (+ `UP`, `DOWN` for pipe-like blocks).

## Behavior (ordered rules)

### 1. Fences (`FenceBlock`: all fences and nether-brick fences; props `NORTH/EAST/SOUTH/WEST`, `WATERLOGGED`)

1. **`updateShape`**: (a) if `WATERLOGGED` → `waterTick`; (b) if `dir` is **horizontal**: return `state` with the property of `dir` set to `connectsTo(nb, faceSolid(nb, nbPos, dir.opposite), dir.opposite)`; (c) else (UP/DOWN) return the state.
2. **`connectsTo(nb, faceSolid, d)`** (`d` = direction from the neighbour toward this fence = `dir.opposite`):
   `connectsTo = (!isException(nb) && faceSolid) || sameFence(nb) || (nb is a fence gate block && gateConnects(nb, d))`.
   - `sameFence(nb)` := `nb` is in `tag#fences` **and** (`nb` in `tag#wooden_fences`) == (this fence's own default state in `tag#wooden_fences`). So wooden fences join wooden fences, the nether-brick fence joins only non-wooden fences.
   - `gateConnects(gate, d)` := `gate.FACING.axis == d.clockWise.axis` (the gate's opening axis is **perpendicular** to the line to the fence; i.e. the gate's hinge line is along the connection).
3. Placement note: all four sides evaluated with the same predicate on live neighbours.
4. Fences have no `neighborChanged`, no `onPlace`, no removal hook.

### 2. Fence gates (`FenceGateBlock`; props `FACING`, `OPEN`, `POWERED`, `IN_WALL`)

1. **`updateShape`** (`axis` = axis of `dir`): if `state.FACING.clockWise.axis != axis` → state unchanged (no tick scheduling, no water: gates are not waterloggable). Else: `inWall = isWall(nb) || isWall(world state at pos + dir.opposite)` (the **second** operand is read **live**, not the captured state) →
   return `state` with `IN_WALL = inWall`. (`isWall` := block in `tag#walls`.) So only the two neighbours along the gate's **line** matter, and a shape update from either end re-reads both.
2. Power handling (`neighborChanged`, flags 2 write) in redstone-components §8.8.
3. Placement note: `IN_WALL` from the two lateral neighbours (`WEST/EAST` for a Z-axis gate, `NORTH/SOUTH` for X-axis).

### 3. Iron bars, copper bars, glass panes (`IronBarsBlock`, `StainedGlassPaneBlock`; the cross-collision props)

1. **`updateShape`**: `waterTick` first if waterlogged; if `dir` horizontal → state with the property of `dir` set to `attachesTo(nb, faceSolid(nb, nbPos, dir.opposite))`; else the state.
2. **`attachesTo(nb, faceSolid)`** := `(!isException(nb) && faceSolid) || nb is any IronBarsBlock instance (bars, copper bars, any pane, stained-glass pane) || nb in tag#walls`.
   (Gates, fences and chains do **not** attach by themselves, only through a solid face.)
3. **Chains** (`ChainBlock`, all metal variants; props `AXIS`, `WATERLOGGED`), **lanterns**: see [`survival-and-supports.md`](survival-and-supports.md); chains only schedule the water tick.
4. **Copper grates / glass-like waterlogged blocks** (`WaterloggedTransparentBlock`): water tick only.

### 4. Walls (`WallBlock`; props `UP`, `NORTH/EAST/SOUTH/WEST` ∈ {NONE, LOW, TALL}, `WATERLOGGED`)

1. **`updateShape`**: (a) `waterTick` if waterlogged; (b) `dir == DOWN` → return the state unchanged; (c) `dir == UP` → `topUpdate`; (d) horizontal → `sideUpdate`.
2. **`wallConnects(nb, faceSolid, d)`** := `nb in tag#walls` **or** (`!isException(nb) && faceSolid`) **or** `nb is an IronBarsBlock` **or** (`nb` is a fence gate that connects toward `d`: `gate.FACING.axis == d.clockWise.axis`).
3. **`topUpdate(state, topPos, topNb)`**: `n,e,s,w` = "current side != NONE" for the four properties of the **current** state; then `shapeFor(state, topPos, topNb, n, e, s, w)` (§4.5).
4. **`sideUpdate(state, dir, nb)`**: for each of N, E, S, W: connected = if that direction equals `dir`: `wallConnects(nb, faceSolid(nb, nbPos, dir.opposite), dir.opposite)`; else "current side != NONE". Then `above = pos + UP`, `aboveState` = live read of `above`; `shapeFor(state, above, aboveState, n, e, s, w)`.
5. **`shapeFor(state, topPos, topNb, n, e, s, w)`**: `aboveShape = topNb.getCollisionShape(topPos).getFaceShape(DOWN)` (the bottom-layer slice of the above block's collision shape, [`../../blocks/shapes.md`](../../blocks/shapes.md)).
   `s' = updateSides(state, n, e, s, w, aboveShape)`; result = `s'` with `UP = shouldRaisePost(s', topNb, aboveShape)`.
6. **`updateSides`**: each side property = `NONE` if not connected, else `TALL` if `isCovered(aboveShape, testShape[side])` else `LOW`. `isCovered(a, t)` := `t` minus `a` is empty (the above block's bottom slice contains the whole test column).
   Test columns (pixels, y 0–16): NORTH `x 7–9, z 0–9`; SOUTH `x 7–9, z 7–16`; WEST `x 0–9, z 7–9`; EAST `x 7–16, z 7–9` (a 2-wide column from the centre to the edge, height 16). Post test column: `x 7–9, z 7–9`.
7. **`shouldRaisePost(s', topNb, aboveShape)`**: (a) if `topNb` is a wall block **and** its `UP` is true → **true**. (b) Let `xNone` = (side x of `s'` is NONE). `hasCorner = (all four NONE) || (N-none != S-none) || (W-none != E-none)` → if true → **true**.
   (c) `hasHighWall = (N == TALL && S == TALL) || (E == TALL && W == TALL)` → if true → **false**. (d) else `topNb in tag#wall_post_override || isCovered(aboveShape, post test column)`.
8. Walls have no other reactions. Placement uses the same helpers with the live top neighbour.

### 5. Stairs (`StairBlock` and every stair variant; props `FACING`, `HALF`, `SHAPE`, `WATERLOGGED`)

1. **`updateShape`**: `waterTick` if waterlogged; if `dir` is **horizontal** → state with `SHAPE = stairsShape(state, pos)` (**ignores the captured neighbour; reads live**); else the state. (UP/DOWN updates do nothing.)
2. **`stairsShape(state, pos)`**: `F = FACING`, `H = HALF`.
   a. `front = state at pos + F`. If `front` is a stair block (`instanceof StairBlock`) with `HALF == H`: `fF = front.FACING`; if `fF.axis != F.axis` **and** `canTakeShape(state, pos, fF.opposite)` → return `OUTER_LEFT` if `fF == F.counterClockWise` else `OUTER_RIGHT`.
   b. `back = state at pos + F.opposite`. If `back` is a stair with `HALF == H`: `bF = back.FACING`; if `bF.axis != F.axis` **and** `canTakeShape(state, pos, bF)` → return `INNER_LEFT` if `bF == F.counterClockWise` else `INNER_RIGHT`.
   c. `STRAIGHT`.
3. **`canTakeShape(state, pos, d)`**: `n = state at pos + d`; true iff `n` is not a stair, **or** `n.FACING != state.FACING`, **or** `n.HALF != state.HALF`.
   (So a stair does not form a corner if the cell on the corner side already holds a stair of the same facing and half that "owns" the corner.)
4. No `neighborChanged`, no `onPlace`. Stairs are not "supported" blocks.

### 6. Slabs (`SlabBlock`): the only reaction is `waterTick` in `updateShape` if `WATERLOGGED`. `canPlaceLiquid` / `placeLiquid` refuse `TYPE == DOUBLE` (fluids.md §8.3). Merging two slabs is placement.

### 7. Huge mushroom blocks (`HugeMushroomBlock`: red / brown cap, stem; props `NORTH/EAST/SOUTH/WEST/UP/DOWN`, default all true)

`updateShape`: if the neighbour (captured state, in direction `dir`) **is this same block** → state with the property of `dir` set to **false**; else the state. It never sets a face back to true when a neighbour disappears (placement sets true where the neighbour is not the same block).

### 8. Chests (`ChestBlock`: `chest`, `trapped_chest`, the eight copper chests; `EnderChestBlock` separately; props `FACING`, `TYPE` ∈ {SINGLE, LEFT, RIGHT}, `WATERLOGGED`)

1. **Connected direction** `cd(state)`: `TYPE == LEFT` → `FACING.clockWise`; **otherwise (RIGHT and SINGLE)** → `FACING.counterClockWise`.
2. `canConnect(nb)`: ordinary and trapped chests: `nb` is **this same block** (a trapped chest only pairs with a trapped chest; a chest never with a trapped one). Copper chests: `nb in tag#copper_chests` and has the `TYPE` property (any oxidation / wax stage pairs).
3. **`updateShape`** (`dir`, `nb`):
   a. `waterTick` if waterlogged.
   b. If `canConnect(nb)` **and** `dir` is horizontal: `nt = nb.TYPE`; if `state.TYPE == SINGLE` **and** `nt != SINGLE` **and** `state.FACING == nb.FACING` **and** `cd(nb) == dir.opposite` → return `state` with `TYPE = nt.opposite` (LEFT ↔ RIGHT).
      (Otherwise fall through to "return the state".)
   c. Else (if the condition of (b) as a whole — can-connect-and-horizontal — is false): if `cd(state) == dir` → return `state` with `TYPE = SINGLE`. (For a SINGLE chest this is a no-op write because `cd` = counter-clockwise side and `TYPE` is already SINGLE.)
   d. Otherwise the state.
4. **Copper chest extra**: after the §8.3 result `r`: if `canConnect(nb)`: if `r.TYPE != SINGLE` **and** `cd(r) == dir` → return **`nb`'s block** with `r`'s properties (`nb.block.withPropertiesOf(r)`) — the connected half adopts the neighbour's weathering / wax stage. Else `r`.
5. **`affectNeighborsAfterRemoval`** (`ChestBlock` and its subclasses — trapped and copper chests; the ender chest has none): `updateNeighbourForOutputSignal(pos, thisBlock)` (comparator notification of neighbour-update §2.6). `onPlace`: none.
   Block entity side: `shouldChangedStateKeepBlockEntity(oldState)` for **copper chests**: true iff the old state is in `tag#copper_chests` (so weathering / waxing between copper chest blocks keeps the inventory); every other chest uses the default (drop on any block change).
6. **Ender chest**: `waterTick` only (no pairing); `tick` (recheck open) is tick-side. **Trapped chest** adds the redstone signal (redstone-power §5).
7. Placement note (item side): `TYPE` from the neighbours' `candidatePartnerFacing` rule; copper chests use the least-oxidised of the pair.

### 9. Shelves (`ShelfBlock`: wooden shelves; props `FACING`, `POWERED`, `SIDE_CHAIN_PART` ∈ {UNCONNECTED, RIGHT, CENTER, LEFT}, `WATERLOGGED`, block entity)

1. **`updateShape`**: `waterTick` if waterlogged; state.
2. **`neighborChanged`** (server; redstone-components §8.13): `signal = hasNeighborSignal(pos)`; if `signal != POWERED`: `newState = state with POWERED = signal`, and if `!signal` also `SIDE_CHAIN_PART = UNCONNECTED`; `setBlockAndUpdate(pos, newState)`; sound + game event.
3. **`onPlace`** (server; runs **on every state write of a shelf**, same-block writes included — no `oldState.is(block)` guard): if `POWERED` → `connectOnPowerUp(pos, state, oldState)`; else `disconnectNeighbours(pos, state)`.
4. **`affectNeighborsAfterRemoval`** (any removal, also state → other block): `updateNeighbourForOutputSignal(pos, thisBlock)`; then `disconnectNeighbours(pos, state)` with the **old** state.
5. **Chain definitions.** `connectable(state)` := block in `tag#wooden_shelves`, has `POWERED`, and `POWERED` is true. `facing(state)` = `FACING`. `maxChain = 3`. For a shelf at `c` with facing `f`: `left(k) = c + f.clockWise·k`, `right(k) = c + f.counterClockWise·k`.
   A *neighbour record* for position `p`: if `state(p)` is `connectable` **and** `facing(state(p)) == f` → a chain neighbour with its current `SIDE_CHAIN_PART`; otherwise an *empty* neighbour (not connectable, always a chain end).
   Part algebra: `isConnected` := ≠ UNCONNECTED; `isChainEnd` := ≠ CENTER; `connectsToward(end)` := CENTER or `== end`;
   `whenConnectedRight`: UNCONNECTED→LEFT, LEFT→LEFT, RIGHT→CENTER, CENTER→CENTER; `whenConnectedLeft`: UNCONNECTED→RIGHT, RIGHT→RIGHT, LEFT→CENTER, CENTER→CENTER;
   `whenDisconnectedRight`: UNCONNECTED→UNCONNECTED, LEFT→UNCONNECTED, RIGHT→RIGHT, CENTER→RIGHT; `whenDisconnectedLeft`: UNCONNECTED→UNCONNECTED, RIGHT→UNCONNECTED, CENTER→LEFT, LEFT→LEFT.
   (`LEFT` = the left end of a chain as seen with facing `f`: it is connected to its **right** neighbour.)
6. **`chainOf(p)`** (list of the positions of the chain through `p`; empty if `state(p)` is not `connectable`): start with `[p]`; scanning to the left: for `k = 1 … maxChain − 1 (= 2)`: take `left(k)` of `p`'s record set; if it `connectsToward(LEFT)` (CENTER or LEFT) → prepend its position; if it `isUnconnectableOrChainEnd` (empty, or part ≠ CENTER) → stop. Same to the right with `connectsToward(RIGHT)`, append, stop likewise. Size of the list is what §9.7 uses.
7. **`connectOnPowerUp(pos, state, oldState)`** (only if `connectable(state)`): skip if `isBeingUpdatedByNeighbor` := `state.SIDE_CHAIN_PART.isConnected()` **or** (`oldState` is connectable and its part is connected). Else:
   `L = left neighbour record, R = right neighbour record`; `eL = L.connectable ? |chainOf(L.pos)| : 0`; `eR` likewise; `len = 1`; `part = UNCONNECTED`.
   If `eL > 0 && len + eL ≤ 3`: `part = part.whenConnectedLeft()` (→ RIGHT); `L.connectToRight()` (write `L.pos` with its part `whenConnectedRight`, via `setBlockAndUpdate` **only if the part changes**); `len += eL`.
   If `eR > 0 && len + eR ≤ 3`: `part = part.whenConnectedRight()`; `R.connectToLeft()` (write `R.pos` with `whenConnectedLeft`).
   Finally `setPart(pos, part)` (write `setBlockAndUpdate` of the shelf with the new part **only if different**). Each of those writes re-enters `onPlace` (§9.3) and is stopped by `isBeingUpdatedByNeighbor`.
   Order of writes: left neighbour, right neighbour, then self.
8. **`disconnectNeighbours(pos, state)`**: `L.disconnectFromRight()` (write `whenDisconnectedRight` part if changed) then `R.disconnectFromLeft()` (`whenDisconnectedLeft`). Empty neighbours are ignored. Only the two adjacent shelves are touched; a CENTER neighbour becomes an end (RIGHT / LEFT), an end becomes UNCONNECTED.
9. Sounds, block-entity inventory and the item-display logic are interaction side.

### 10. Multiface blocks (`MultifaceBlock`: `resin_clump`; `MultifaceSpreadeableBlock`: `glow_lichen`, `sculk_vein`; props `NORTH/EAST/SOUTH/WEST/UP/DOWN` booleans, `WATERLOGGED`)

1. **`attachable(level, dir, nbPos, nb)`** (`MultifaceBlock.canAttachTo`): `dir` = direction **from the multiface block to the neighbour**; true iff the neighbour's **block-support shape** has a *full* face toward `dir.opposite` **or** its **collision shape** has a full face toward `dir.opposite`
   (a face is full iff `getFaceShape(face)` equals the full block, shapes.md). (State-facts sturdiness is **not** used: leaves and other blocks with empty support shapes qualify only through their collision shape.)
2. **`canSurvive`**: at least one face is set **and** every set face satisfies `attachable` (live read of the neighbour at `pos + face`).
3. **`updateShape`**: (a) `waterTick` if waterlogged; (b) if **no** face is set → `AIR`; (c) if the state has the face `dir` **and** `!attachable(level, dir, nbPos, nb)` (captured neighbour) → remove that face (set false); if that leaves no face → `AIR`; else the state with the face removed; (d) else the state.
   (Only the face toward the notifying neighbour is re-evaluated; others stay.)
4. Spreading, bonemeal and the sculk charge system are tick-side. `sculk_vein`: no extra reaction hooks (its discharge / regrow helpers are called by the sculk spreader; they write with `setBlockAndUpdate`).
5. **`pale_hanging_moss`, `pale_moss_carpet`**: [`plants.md`](plants.md) §7.5, §10 (they reuse `attachable`).

### 11. Vines (`VineBlock`; props `UP`, `NORTH/EAST/SOUTH/WEST`; **no** `DOWN`, no water)

1. `acceptable(nbPos, d)` := `attachable(level, d, nbPos, state at nbPos)` of §10.1 (`d` = direction from the vine toward the neighbour).
2. **`canSupportAtFace(pos, d)`**: `false` if `d == DOWN`; true if `acceptable(pos + d, d)`; **else if `d` is vertical → false**; else true iff the block **above** (`pos + UP`) is a vine whose property for `d` is true (a vine can hang on the vine above).
3. **`updatedState(state, pos)`** (`UP` first, then the horizontal faces in N, E, S, W order):
   - If `state.UP`: set `UP = acceptable(pos + UP, **DOWN**)` — note the direction argument is `DOWN` (not UP), so the face tested is the **top** face (`UP` side) of the block above rather than its bottom face. **[oracle]**
   - For each horizontal `d` in N, E, S, W with the face set: `c = canSupportAtFace(pos, d)`; if `!c`: `c = (block above is a vine with its d face true)` (the above state read lazily at most once); set the face to `c`.
   Faces that were not set stay unset.
4. **`canSurvive`** := the result of `updatedState` has at least one of the five faces set.
5. **`updateShape`**: `dir == DOWN` → state; else `new = updatedState(state, pos)`; if `new` has no face → `AIR`; else `new`.
6. Placement note (item side): uses `canSupportAtFace` for the nearest looking directions except DOWN.

### 12. Tripwire strings and hooks (shape part only)

- `TripWireBlock.updateShape`: horizontal `dir` → state with the property of `dir` = `shouldConnectTo(nb, dir)`: `nb` is the paired hook block ? `nb.FACING == dir.opposite` : `nb` is this tripwire block. Vertical → state.
- Everything else (support of the hook, the update scan along the wire, power) is in redstone-components §6.6–6.7; those specs fix `updateSource`, `calculateState` and the removal behaviour.

### 13. Chorus plant

[`plants.md`](plants.md) §7.14: `updateShape` returns the state unchanged (+ `sched(plant, 1)`) when it cannot survive, otherwise sets only the property of `dir` from "neighbour is the plant, a chorus flower, or (dir == DOWN and `tag#supports_chorus_plant`)".

### 14. Pipe-like and cross-collision shapes (`PipeBlock`, `CrossCollisionBlock`) define shapes only; no reactions of their own.

## Edge cases and known quirks

1. **Captured vs live reads.** Fences, bars, tripwire, mushroom blocks, chests (the dir neighbour) use the **captured** `nb`; stairs use only live reads; walls use the captured neighbour for the notified side and live reads for the block above; fence gates use the captured neighbour **and** one live read.
2. **Fence `sameFence` compares wooden-ness**, not block identity, so an oak fence and a spruce fence connect; a nether-brick fence does not connect to an oak fence.
3. **Stairs ignore vertical updates**; a stair above or below can never change the shape. The shape is recomputed from **live** neighbours at the moment the shape update runs.
4. **Chest `TYPE` inversion:** the update that links a new second chest changes the **existing** chest (`TYPE = nt.opposite`), the placed one already has its own `TYPE` from placement; both must agree through their `FACING` and `cd`.
5. **Copper chest synchronisation** replaces the block (weathering / wax stage) of one half with the neighbour's; it does not preserve the inventory unless `shouldChangedStateKeepBlockEntity` returns true (it does within `tag#copper_chests`).
6. **Shelf `onPlace` on every write.** Powering up with a neighbour that changes part triggers a short cascade of `onPlace` calls that terminates through `isBeingUpdatedByNeighbor`; the cascade order is left, right, self.
7. **Huge mushroom** faces only ever turn off through updates.
8. **Vine `UP`** uses the *top* face of the block above (§11.3) — a vine hanging under a bottom slab or an upside-down stair can survive/vanish differently from the "natural" rule.
9. **Wall `UP` depends on the block above's collision shape bottom slice** and on the post-override tag; the update from DOWN does nothing.
10. **Bars / panes attach to walls, fences gates do not**, and bars of any material attach to each other and to glass panes.

## Constants and IDs

- Chorus / vine / multiface use the pipe-like boolean property set; wall side values `NONE`, `LOW`, `TALL`; stairs shape values `STRAIGHT, INNER_LEFT, INNER_RIGHT, OUTER_LEFT, OUTER_RIGHT`; chest types `SINGLE, LEFT, RIGHT`; side-chain parts `UNCONNECTED, RIGHT, CENTER, LEFT`; `maxChain = 3`.
- Tags: `fences`, `wooden_fences`, `walls`, `wall_post_override`, `shulker_boxes`, `copper_chests`, `wooden_shelves`, `speleothems`.

## Oracle probe points

- `FenceBlock#updateShape`, `FenceBlock#connectsTo`, `FenceGateBlock#updateShape`, `WallBlock#updateShape` (with `topUpdate` / `sideUpdate`), `StairBlock#updateShape`, `StairBlock#getStairsShape` (private; probe via `updateShape`), `ChestBlock#updateShape`, `CopperChestBlock#updateShape`,
  `MultifaceBlock#updateShape`, `MultifaceBlock#canAttachTo(BlockGetter, Direction, BlockPos, BlockState)`, `VineBlock#updateShape`, `HugeMushroomBlock#updateShape`, `ShelfBlock#onPlace`, `SideChainPartBlock#updateSelfAndNeighborsOnPoweringUp`, `SideChainPartBlock#getAllBlocksConnectedTo`, `Block#isExceptionForConnection`.

## Open questions

1. **[oracle] Vine `UP` face direction** (§11.3): place a vine under a block with a full top but a partial bottom (e.g. a slab placed upside-down on top of a slab) and trigger a shape update.
2. **[oracle] Wall test columns** (§4.6): confirm the numeric extents (x/z ranges) of the four test columns against a probe of `WallBlock.TEST_SHAPES_WALL` (public-to-reflection via static field), as read from `boxZ(2, 16, 0, 9)` rotated.
3. **[oracle] Chest §8.3c/d fall-through**: confirm via probe that a SINGLE chest receiving a non-chest update from its `FACING.counterClockWise` side returns the identical state object (no write).
4. **[oracle] Shelf chain with mixed facings** (§9.5): two powered shelves adjacent but with different `FACING`; confirm they are empty neighbours of each other.
5. **[oracle] Stair corner ownership** (§5.3): probe `canTakeShape` with three stairs around a corner.
6. (implementer) `getFaceShape` and the "full face" test are the shapes.md versions; `isFaceSturdy` is the state-transforms version.

## Parity status

Not yet run.
