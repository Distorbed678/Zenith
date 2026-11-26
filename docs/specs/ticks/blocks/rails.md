# Ticks: block reactions — rails and the rail-shape solver (26.3)

- Verified by suites: `block-updates`
- Sources: local reference reading of the 26.3 rail classes (2026-11-26, reader session): the rail base class, the shape-solver
  helper object (connection search), the plain / powered / detector rail classes, the rail-shape enum and the framework steps they plug into
  (set-block step 3.8 "rails notify on shape change", `Level.neighborChanged`). Everything is in our own words.
- Related specs: [`README.md`](README.md) (index), [`../redstone-components.md`](../redstone-components.md) §9 (the powered-rail search, the detector
  rail's minecart scan and analog output — **this file supplies the shape solver that that section left open**), [`../redstone-power.md`](../redstone-power.md),
  [`../set-block-flags-and-sequence.md`](../set-block-flags-and-sequence.md) (§2 steps 3, 6, 7; §3 steps 8–10), [`../neighbor-and-shape-updates.md`](../neighbor-and-shape-updates.md),
  [`../block-hooks.md`](../block-hooks.md), [`../fluids.md`](../fluids.md) §8 (rails are waterloggable), [`../../blocks/state-facts.md`](../../blocks/state-facts.md)
  (rail shape property value lists, sturdiness, `isSignalSource`), [`../../blocks/shapes.md`](../../blocks/shapes.md).
- Scope: *every reaction of a rail block to the world*: placement-time solving, neighbour-driven re-solving, removal when unsupported, waterlogging tick, and the
  update fan-out these writes cause. The cart movement along rails is the entity milestone's.

## Summary

Rails are the only blocks for which the engine gives a *two-hop, stateful* shape solver: a rail looks at the up-to-four horizontal neighbours (each probed at the
same height, one up and one down), picks its own shape, **and then rewrites its neighbours' shapes so that both ends agree**. All of it is expressed through
`setBlockAndUpdate` (flags 3) writes made *inside* hooks, so the order of observable updates follows the re-entrancy rules of `set-block-flags-and-sequence.md`
and `neighbor-and-shape-updates.md` §1. Rails are **not** removed by `updateShape`; they are removed by `neighborChanged` (support check with the RIGID support type).

There are four rail blocks, three classes:

| block | class | straight-only | shape property | extra properties |
|---|---|---|---|---|
| `rail` | `RailBlock` | no (10 shapes) | `SHAPE` ∈ NORTH_SOUTH, EAST_WEST, ASCENDING_EAST/WEST/NORTH/SOUTH, SOUTH_EAST, SOUTH_WEST, NORTH_WEST, NORTH_EAST | `WATERLOGGED` |
| `powered_rail`, `activator_rail` | `PoweredRailBlock` | yes (6 shapes: the first six of the list) | `SHAPE` straight subset | `POWERED`, `WATERLOGGED` |
| `detector_rail` | `DetectorRailBlock` | yes | straight subset | `POWERED`, `WATERLOGGED` |

`isRail(state)` := the block is in `tag#rails` **and** is an instance of the rail base class. `isRail(level, pos)` reads the block at `pos`.
`shape.isSlope()` := one of the four ASCENDING_* shapes.

## Inputs and state

- A **RailState** (solver object, not stored): `(level, pos, state, block, isStraight, connections)`. `connections` is an ordered list of positions
  (duplicates possible, §6.3), recomputed from a shape by `updateConnections(shape)` and **initialised from the rail's current shape in the constructor**.
- **`connections(shape)`** (the list for each shape; order matters for iteration): with `N = pos+NORTH`, `S = pos+SOUTH`, `W = pos+WEST`, `E = pos+EAST`, `↑` = +UP:

| shape | list |
|---|---|
| NORTH_SOUTH | N, S |
| EAST_WEST | W, E |
| ASCENDING_EAST | W, E↑ |
| ASCENDING_WEST | W↑, E |
| ASCENDING_NORTH | N↑, S |
| ASCENDING_SOUTH | N, S↑ |
| SOUTH_EAST | E, S |
| SOUTH_WEST | W, S |
| NORTH_WEST | W, N |
| NORTH_EAST | E, N |

- Direction conventions: NORTH = −Z, SOUTH = +Z, WEST = −X, EAST = +X. `ASCENDING_<D>` rises **toward D**.
- `hasNeighborSignal(pos)` (redstone-power §2.6; no quasi-connectivity) is used only to choose between corner shapes (§2.6).

## Behavior (ordered rules)

### 1. RailState primitives (pure unless noted)

1. **`hasConnection(p)`**: some entry of `connections` has the same **X and Z** as `p` (the Y coordinate is ignored).
2. **`connectsTo(other)`** := `hasConnection(other.pos)`.
3. **`getRail(p)`**: test the block at `p`, then at `p + UP`, then at `p + DOWN` (in that order); the first that satisfies `isRail` yields a new RailState for that position and state
   (built from the **world's current** state, so its connections come from that rail's current shape). None → null. (Consequence: a rail can "see" another rail one block up or one block down
   in each horizontal direction, with **UP having priority over DOWN**, same level having priority over both.)
4. **`hasRail(p)`** := `isRail(p) || isRail(p+UP) || isRail(p+DOWN)`.
5. **`countPotentialConnections()`** := number of the four horizontal directions `d` (N, E, S, W order; the count is order independent) with `hasRail(pos + d)`.
6. **`removeSoftConnections()`** (mutates the RailState only): for each index `i` of `connections` in order: `rail = getRail(connections[i])`; if `rail ≠ null` **and**
   `rail.connectsTo(this)` (the neighbour's own list contains a position with this rail's X,Z) → replace `connections[i]` with `rail.pos` (this fixes the Y of the entry); else delete the entry (and
   re-examine the same index). A *soft* connection is one the other side does not reciprocate.
7. **`canConnectTo(other)`** := `connectsTo(other) || connections.size() != 2` (a rail with **exactly two** unrelated connections is "full" and refuses a third).
8. **`hasNeighborRail(p)`**: `nb = getRail(p)`; null → false; else `nb.removeSoftConnections(); return nb.canConnectTo(this)`.

### 2. `place(hasSignal, first, defaultShape)` — choose this rail's shape and wire the neighbours

Called with `defaultShape` = the rail's **current** shape property (read from the block's state when the solve starts). Let `n, s, w, e` = `hasNeighborRail(N/S/W/E)` — **all four evaluated, in the
order N, S, W, E** (each runs the neighbour's `removeSoftConnections`, a pure operation), and let `nOrS = n || s`, `wOrE = w || e`.

1. `shape = none`. If `nOrS && !wOrE` → NORTH_SOUTH. If `wOrE && !nOrS` → EAST_WEST. (Both assignments are exclusive.)
2. If **not straight-only** (plain rail): corner shapes when exactly the two perpendicular neighbours exist: apply in this order (later wins, they are exclusive anyway):
   `s && e && !n && !w` → SOUTH_EAST; `s && w && !n && !e` → SOUTH_WEST; `n && w && !s && !e` → NORTH_WEST; `n && e && !s && !w` → NORTH_EAST.
3. If `shape` is still none:
   a. if `nOrS && wOrE` → `defaultShape`; else if `nOrS` → NORTH_SOUTH; else if `wOrE` → EAST_WEST.
   b. If not straight-only, apply the **override by signal** (this is where three-way junctions pick a curve):
      - `hasSignal` (the rail's own cell `hasNeighborSignal`): in this order, later wins: `s&&e` → SOUTH_EAST; `s&&w` → SOUTH_WEST; `n&&e` → NORTH_EAST; `n&&w` → NORTH_WEST. (Winner = **north-west** if available, then NE, SW, SE.)
      - no signal: in this order, later wins: `n&&w` → NORTH_WEST; `n&&e` → NORTH_EAST; `s&&w` → SOUTH_WEST; `s&&e` → SOUTH_EAST. (Winner = **south-east**, then SW, NE, NW.)
      (If none of the four pairs holds, `shape` keeps the value of 3.a.)
4. **Ascending upgrade** (applies to the result, straight-only rails included): if `shape == NORTH_SOUTH`: if `isRail(level, N + UP)` → ASCENDING_NORTH; **then** if `isRail(level, S + UP)` → ASCENDING_SOUTH
   (so with rails above both ends the **south** one wins). If `shape == EAST_WEST`: if `isRail(E + UP)` → ASCENDING_EAST; then if `isRail(W + UP)` → ASCENDING_WEST (**west** wins).
   Only a rail **exactly one block up** in the straight direction makes a slope; the check is `isRail(that position)` only, not `getRail`.
5. If `shape` is still none → `defaultShape`.
6. `updateConnections(shape)`; `state = state with SHAPE = shape`.
7. **Write and wire:** if `first` **or** the world's state at `pos` differs from `state` (value inequality): `setBlockAndUpdate(pos, state)` (flags 3; a write of the identical state is a no-op, see set-block §3.2),
   then for each `i` of `connections` in index order: `nb = getRail(connections[i])`; if non-null: `nb.removeSoftConnections()`; if `nb.canConnectTo(this)` → `nb.connectTo(this)` (§3).
   Otherwise (not first and unchanged) nothing is written and **no neighbour is wired**.
8. Return the RailState (its `state` is what the caller continues with).

### 3. `connectTo(other)` — a neighbour accepts this rail and re-derives its own shape

Executed on the **neighbour's** RailState (`this` below = the neighbour).

1. Append `other.pos` to `connections` (**no duplicate check**).
2. `n, s, w, e` = `hasConnection(N)`, `hasConnection(S)`, `hasConnection(W)`, `hasConnection(E)` of this rail's position (X,Z compare; the list now includes the new entry).
3. `shape`: if `n || s` → NORTH_SOUTH; if `w || e` → EAST_WEST (overrides). If not straight-only: `s&&e&&!n&&!w` → SOUTH_EAST; `s&&w&&!n&&!e` → SOUTH_WEST; `n&&w&&!s&&!e` → NORTH_WEST; `n&&e&&!s&&!w` → NORTH_EAST.
4. Ascending upgrade exactly as §2.4 (on NORTH_SOUTH: N↑ then S↑; on EAST_WEST: E↑ then W↑; `isRail` of the exact positions above the side neighbours of **this** rail).
5. If none → NORTH_SOUTH. `state = state with SHAPE = shape`; `setBlockAndUpdate(pos, state)` (flags 3, unconditional; no-op if identical). **The connection list is not recomputed from the final shape** (the object is discarded afterwards).
   Because `connectTo` chooses by presence of connections, **there is no use of `hasSignal` or `defaultShape` here**.
6. Note: the neighbour's write does **not** recurse into further wiring of *its* neighbours: only `place` wires. A chain of rails thus re-solves only one hop outward per placement.

### 4. The block hooks

1. **`onPlace(state, level, pos, oldState, movedByPiston)`** (server): only if `oldState`'s **block is different** from this block: `updateState(state, level, pos, movedByPiston)` (the four-argument form returning a state, §4.2).
   Detector rail additionally runs `checkPressed(level, pos, thatReturnedState)` right after (spec in redstone-components §9.2).
   (A change of shape of an existing rail — `oldState` the same block — does **not** re-solve.)
2. **`updateState(state, level, pos, movedByPiston)` → state** (call this `solveAndNotify`): `state = updateDir(level, pos, state, first = true)`; if the rail is straight-only:
   `level.neighborChanged(state, pos, thisBlock, null, movedByPiston)` — a **Full neighbour update to itself** with the captured `state` (so it runs through the chained updater: immediately if the chain is idle,
   else after the current step); return `state`.
3. **`updateDir(level, pos, state, first)`**: on a client level → return `state`. Else `current = state.SHAPE`; return `RailState(level, pos, state).place(hasNeighborSignal(pos), first, current).state`.
4. **`neighborChanged(state, level, pos, sourceBlock, orientation, movedByPiston)`** (server; `Level.neighborChanged` hook): only if `level.getBlockState(pos)` is **this block** (re-read; the notified `state` argument may be stale):
   a. `shape = state.SHAPE` (from the **argument**). **`shouldBeRemoved`**: true if the block below does **not** `canSupportRigidBlock` (its UP face has the RIGID support type — `state-transforms.md`); or the shape is
      ASCENDING_EAST and the block at `pos + EAST` is not a rigid UP-supporter (the same test applied to the cell at `pos + D`, not `pos + D + DOWN`; likewise ASCENDING_WEST `pos+WEST`, ASCENDING_NORTH `pos+NORTH`, ASCENDING_SOUTH `pos+SOUTH`).
   b. If `shouldBeRemoved`: **`dropResources(state, level, pos)`** (the block's loot for the state, with no tool and no entity) **then** `removeBlock(pos, movedByPiston)` (writes `fluid.createLegacyBlock()` — water for a waterlogged rail, else air — with flags 3, plus bit 64 when `movedByPiston`).
   c. Else `updateState(state, level, pos, sourceBlock)` — the **five-argument-free void overload keyed on the source block**:
      - `RailBlock` (plain rail): `if sourceBlock.defaultState.isSignalSource()` **and** `RailState(level, pos, state).countPotentialConnections() == 3` → `updateDir(level, pos, state, first = false)` (return value ignored).
        So a plain rail re-solves **only** when a neighbour block that is a *signal source* changes (redstone wire, torches, levers, detector rails, observers, … — any block whose `isSignalSource` of the default state is true; see redstone-power §5) and exactly three of the four sides have
        a rail candidate. Neighbouring rail changes do **not** re-solve (they are wired by `place`).
      - `PoweredRailBlock` (powered and activator): the power search of redstone-components §9.1 (sets `POWERED`).
      - `DetectorRailBlock`: nothing (the void overload is the empty base).
5. **`canSurvive(state, level, pos)`** = `canSupportRigidBlock(level, pos.below())` — **not** used by `updateShape`; used by `DetectorRailBlock.checkPressed` and by waterlogging pickup (fluids.md §8.1). A rail with its support removed by a **shape-update-only**
   write (flags without bit 1) is therefore *not* removed until the next `neighborChanged`.
6. **`updateShape`**: only the waterlogging tick: if `WATERLOGGED` → `waterTick`; return the state. (No support check, no shape change.)
7. **`affectNeighborsAfterRemoval(state, level, pos, movedByPiston)`** (`state` = the **old** state; called also on rail → rail shape changes, set-block §3.8): if **not** `movedByPiston`: if the **old** shape `isSlope` → `updateNeighborsAt(pos + UP, thisBlock)`;
   if the rail is straight-only → `updateNeighborsAt(pos, thisBlock)` then `updateNeighborsAt(pos + DOWN, thisBlock)`. (Slope notification precedes the straight ones; each is a Multi update in the order of neighbor-and-shape-updates §2.2: W, E, DOWN, UP, N, S.)
8. **`updateIndirectNeighbourShapes`**: not overridden (rails have no two-step shape updates).
9. **Placement state** (`getStateForPlacement`, item side): shape = EAST_WEST if the player's horizontal direction is EAST or WEST else NORTH_SOUTH; `WATERLOGGED` from the replaced fluid being water. The solver of §2 then runs from `onPlace`.

### 5. Detector rail's connected-update (spec of the shape-dependent part)

`updatePowerToConnected(level, pos, state, powered)`: `rs = RailState(level, pos, state)`; for each position `c` in `rs.connections` (list order of §Inputs for the rail's **current** shape): `cs = level.getBlockState(c)`;
Full neighbour update `level.neighborChanged(cs, c, cs.block, null, false)` (so the connected position receives `neighborChanged` with **its own block as the source**; the position may hold anything, including a non-rail block or air).

### 6. Update order of a placement — the complete fan-out (worked through)

For `setBlock(P, railState, 3)` over a non-rail old block (chain idle):

1. Chunk write; old block's removal hook (none for air / most blocks); **`onPlace`** → §4.2: `RailState.place(first = true)`:
   - If the solved shape differs from the placed one: a **nested** `setBlockAndUpdate(P, solvedState)`: its set-block step 3.8 calls the *placed* state's `affectNeighborsAfterRemoval` (rail → rail change, flags 1 set), i.e. §4.7 with the
     placed shape (neighbour Multi updates around `P+UP` / `P` / `P+DOWN`, **before** the nested write's own neighbour notification), then (no `onPlace`: same block), then the nested step 6 `updateNeighborsAt(P, railBlock)` and the nested
     shape updates (six, `SHAPE_ORDER`). All of this completes **before `place` continues**.
   - Then for each `connections[i]` (index order): the neighbour's `connectTo(P)` → its own nested `setBlockAndUpdate` (if the shape changes) with the same fan-out around **that** neighbour (old-shape removal hook, then `updateNeighborsAt`, then shape updates).
   - Finally (straight-only) the Full update `neighborChanged(P)`, which may start the powered search.
2. Back in the **outer** `setBlock(P, …)`: if `place` wrote a different state at `P`, the outer step 3 sees `new != state` and **skips** its own neighbour notification, shape updates and client notification (the nested write did them). If `place` wrote nothing (the solved
   shape equals the placed shape) the outer steps 4–9 run normally afterwards: client notification, `updateNeighborsAt(P, oldBlock)` (**old block**, e.g. air, as source), the six shape updates, the indirect shape updates.
3. Neighbour rails that were re-shaped had their neighbours notified with **the rail block as source** (nested writes use the previous state's rail block); `RailBlock.updateState` of those listeners acts only for signal-source sources (§4.4c).

**Worked example (checks the solver).** Plain rails, flat ground, no signals. Existing rail `A` at (0,0,0) NORTH_SOUTH. Player places a rail `B` at (0,0,1) (south of A) facing EAST (placed shape EAST_WEST).
`B.onPlace` → `place(hasSignal = false, first = true, default = EAST_WEST)`:
`hasNeighborRail(N = (0,0,0))`: `nb = RailState(A)` with `connections = [(0,0,-1), (0,0,1)]`; `removeSoftConnections`: entry (0,0,-1): no rail there, up or down → removed; entry (0,0,1): `getRail` finds `B` (its **placed**
state, EAST_WEST, connections `[(-1,0,1),(1,0,1)]`); `B.connectsTo(A)`? neither entry has (X,Z) = (0,0) → removed. `A.connections = []`; `A.canConnectTo(B)` → size ≠ 2 → true ⇒ `n = true`. `s, w, e`: no rails ⇒ false.
`nOrS && !wOrE` → NORTH_SOUTH. Not corner. Ascending: `isRail((0,1,0))`? no; `isRail((0,1,2))`? no. Shape NORTH_SOUTH, `connections = [(0,0,0),(0,0,2)]`; since `first`, `setBlockAndUpdate(B, NORTH_SOUTH)` (differs from the placed EAST_WEST → written, with the
fan-out of §6.1: B's old-state (EAST_WEST, non-slope, plain rail) removal hook does nothing (not straight) → then neighbour update `updateNeighborsAt(B, rail)`, then shape updates of B's six neighbours). Wiring loop: `i = 0`: `getRail((0,0,0))` = RailState(A): connections `[(0,0,-1),(0,0,1)]`; `removeSoftConnections`: (0,0,-1) removed; (0,0,1): `getRail` = B (now NORTH_SOUTH,
connections `[(0,0,0),(0,0,2)]`), `B.connectsTo(A)` yes → entry replaced by `B.pos`; `A.connections = [(0,0,1)]`; `canConnectTo(B)` → `connectsTo(B)` true ⇒ `A.connectTo(B)`: appends → `[(0,0,1),(0,0,1)]`; `s = true` ⇒ NORTH_SOUTH; no slope; `setBlockAndUpdate(A, NORTH_SOUTH)` = identical → no effect.
`i = 1`: `getRail((0,0,2))` → none. Result: A unchanged, B NORTH_SOUTH.

## Edge cases and known quirks

1. **Y is ignored by `hasConnection` / `connectsTo`**, but entries are *positions*: a slope's list contains the position one block **up** on the rising side; `removeSoftConnections` then relies on `getRail` finding the rail at ±1 Y; a stale Y gets corrected to the neighbour's real Y.
2. **Duplicate connection entries** occur (`connectTo` appends blindly; `removeSoftConnections` keeps duplicates). They only matter for `size() != 2` in `canConnectTo` of the *same* temporary object, and are discarded after the call.
3. **`place` with `first = false`** (the re-solve of a plain rail next to a signal source) writes and wires **only if the solved shape differs from the current world state**; otherwise it is a pure no-op.
4. **Priority of ascending upgrades**: south overrides north, west overrides east (assignments in sequence).
5. **Corner curve choice with three neighbours** depends on `hasNeighborSignal` of the rail's own cell: powered → NW preferred; unpowered → SE preferred (§2.3b). A plain rail with 3 neighbours and a neighbouring *signal source* re-solves when that source updates (§4.4c).
6. **Rails are removed from `neighborChanged`, not `updateShape`.** Consequences: (a) a piston-pushed or `/setblock`-with-bit-1-clear support does not remove the rail; (b) a rail whose neighbour changes **by a shape-only write** keeps standing; (c) the removal passes `movedByPiston` through, so the
   removal flag 64 skips neighbour notifications in the rail's `affectNeighborsAfterRemoval` when a piston is the cause.
7. **`shouldBeRemoved` reads the shape from the notified state argument** while the "is this block still here" test reads the world; with a stale argument the slope test may use an old shape.
8. **The neighbour `connectTo` writes never re-enter `place`**; only the freshly placed (or signal-re-solved) rail wires its neighbours, so re-wiring propagates exactly one hop.
9. **`RailBlock.updateState` guard `isSignalSource` uses the default state of the *source block*** (e.g. `redstone_wire`, `redstone_torch`, `lever`, `detector_rail`, `observer`) — not the actual changed state.
10. **Waterlogged rails**: `WATERLOGGED` is untouched by the solver (the solver rebuilds states by `setValue(SHAPE)` on the existing state, so `WATERLOGGED`, `POWERED` are carried over).
11. The rail → rail removal-hook call (set-block §3.8) happens on **every** shape change written by the solver; straight rails therefore send three Multi updates (`P`, `P+DOWN`, and `P+UP` for slopes) **before** the write's own `updateNeighborsAt(P, …)` — this is the "extra" update burst visible in parity dumps.

## Constants and IDs

- Rail shape list order: `north_south, east_west, ascending_east, ascending_west, ascending_north, ascending_south, south_east, south_west, north_west, north_east` (ordinals 0–9). Straight-only rails use ordinals 0–5 only.
- Powered-rail search depth limit 8 (redstone-components §9.1); detector rail recheck delay 20; potential-connection threshold 3; max rail connection list size considered "full": 2.
- `removeBlock` flags: 3 (+64 when moved by piston).

## Oracle probe points

- Wrap `RailState#place(boolean, boolean, RailShape)`, `RailState#connectTo(RailState)`, `RailState#getConnections()`, `RailState#countPotentialConnections()`, `BaseRailBlock#updateState(BlockState, Level, BlockPos, boolean)`, `BaseRailBlock#updateDir(Level, BlockPos, BlockState, boolean)`,
  `BaseRailBlock#neighborChanged` (via `BlockStateBase#handleNeighborChanged`), `BaseRailBlock#affectNeighborsAfterRemoval`, `Level#setBlock(BlockPos, BlockState, int, int)` and `Level#neighborChanged(BlockState, BlockPos, Block, Orientation, boolean)`; log `(seq, pos, shape in, shape out, connection list)`.
- Probes (drive with commands or `ServerLevel#setBlock`): all 16 combinations of rails at N/S/W/E, each in {absent, flat, one up, one down}; with and without a powered neighbour (lever) for the three-neighbour corner choice.

## Open questions

1. **[oracle] `removeSoftConnections` index handling** (§1.6): verify with a three-rail T-junction that the entry examined after a deletion is the one that shifted into the same index (the decrement of the loop counter).
2. **[oracle] Duplicate entries** (§Edge 2): confirm that `canConnectTo` can return false for a rail whose list is `[X, X]` (size 2, `connectsTo` true → still true) and that no observable shape depends on a duplicate.
3. **[oracle] Ordering of the four `hasNeighborRail` probes** (§2): they are side-effect free on the world; confirm no probe changes world state by log of `setBlock` during `place` before the write.
4. **[oracle] Nested write fan-out** (§6.1): verify with the neighbour-update debug listener that the removal-hook updates of the *placed* state precede `updateNeighborsAt(P, rail)` of the nested write, as derived from set-block §3.8 vs §2.6.
5. **[oracle] `dropResources` argument set** in `neighborChanged` (§4.4b): confirm the drop uses the pre-removal state's loot table with no tool (a rail drops itself in every case).
6. **[oracle] Rail-at-slope support**: confirm `canSupportRigidBlock(pos.east())` for ASCENDING_EAST uses the block **at** `pos.east()` (same Y), per the source reading (§4.4a).
7. (implementer) `Level.neighborChanged(state, pos, block, orientation, movedByPiston)` with `movedByPiston = true` is only produced by piston code; the rail passes it through unchanged.

## Parity status

Not yet run.
