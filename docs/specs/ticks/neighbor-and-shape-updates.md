# Ticks: neighbour updates, shape updates and the chained updater (26.3)

- Verified by suites: `server-tick`, `block-updates`
- Sources: local reference reading of the 26.3 client/server code (2026-11-26, reader session): the neighbour-updater
  interface and its collecting implementation, the shape-update executor, the block-state wrappers that start shape updates,
  the set-block entry that calls them, the output-signal notification, the structure-template edge update and its voxel-face
  walk, the default shape-update path for non-level accessors, and the redstone orientation type. Everything is in our own words.
- Related specs: [`set-block-flags-and-sequence.md`](set-block-flags-and-sequence.md) (steps 2.6–2.7 call into this file),
  [`block-hooks.md`](block-hooks.md) (the hook signatures), [`scheduled-ticks-and-block-events.md`](scheduled-ticks-and-block-events.md),
  [`server-tick.md`](server-tick.md); [`../blocks/shapes.md`](../blocks/shapes.md) (direction order), [`../blocks/state-transforms.md`](../blocks/state-transforms.md);
  [`../worldgen/structures-placement-and-jigsaw.md`](../worldgen/structures-placement-and-jigsaw.md) §4 step 7 (where the structure edge update is used),
  [`../worldgen/features.md`](../worldgen/features.md) (`Direction` ordinals).

## Summary

When a block changes, the world tells its neighbours in two different ways:

1. **Neighbour changed** (`neighborChanged`): "the block at position X changed, re-evaluate yourself" — delivered to the six neighbours
   in the order W, E, down, up, N, S. A neighbour may react by changing blocks, scheduling ticks, etc.
2. **Shape update** (`updateShape`): "your neighbour in direction D changed, return your new state" — a *pure function* of the
   current state and the neighbour's state that returns the new state of the notified block (fence connections, stair shapes, plants that
   pop off, water that schedules a tick). Delivered in the order W, E, N, S, down, up.

Both are not run recursively: they go through a **chained updater** that keeps an explicit stack so that deep chains never overflow, runs
every consequence of an update **depth-first before the next sibling**, and caps the total number of updates of one chain (default
1,000,000) and the nesting depth of shape-driven writes (512).

## Inputs and state

- **Direction orders** (all use the `Direction` names; `docs/specs/worldgen/features.md` fixes ordinals DOWN 0, UP 1, NORTH 2, SOUTH 3, WEST 4, EAST 5):
  - `NEIGHBOUR_ORDER` (`neighborChanged`): **WEST, EAST, DOWN, UP, NORTH, SOUTH**.
  - `SHAPE_ORDER` (`updateShape`): **WEST, EAST, NORTH, SOUTH, DOWN, UP**.
  - Output-signal scan: the four horizontal directions **NORTH, EAST, SOUTH, WEST**.
- **Chained updater state** (one per dimension, `CollectingNeighborUpdater`): a stack (LIFO), an ordered list `addedThisLayer`, an integer `count` (number of
  updates *added* in the current chain, including dropped ones) and the maximum `maxChain` (server property `max-chained-neighbor-updates`, default
  **1,000,000**; a negative value means unlimited).
- **Update records** (the kinds of stack entries):
  - *Shape update* `(direction, neighbourState, pos, neighbourPos, flags, limit)`: runs one step.
  - *Simple neighbour update* `(pos, sourceBlock, orientation)`: runs one step; the notified state is **read when the step runs**.
  - *Full neighbour update* `(state, pos, sourceBlock, orientation, movedByPiston)`: runs one step with the **captured state**.
  - *Multi neighbour update* `(sourcePos, sourceBlock, orientation, skipDirection)`: up to six steps, one per direction of `NEIGHBOUR_ORDER` except `skipDirection`.
- **Redstone orientation**: an optional extra argument (`null` in vanilla; §7).

## Behavior (ordered rules)

### 1. The chained updater

`addAndRun(update)` (called by every entry point of §2 and §3):
1. `running = count > 0`; `tooMany = maxChain ≥ 0 and count ≥ maxChain`; `count += 1`.
2. If not `tooMany`: if `running`, append the update to `addedThisLayer`; else push it onto the stack. If `tooMany`, drop it (only the first dropped update of a chain
   logs an error).
3. If not `running`: run the updates (rule 4). Otherwise return (the running loop will pick the update up).
4. **Run loop** — repeat while the stack is non-empty or `addedThisLayer` is non-empty:
   a. Move every update of `addedThisLayer` onto the stack **in reverse list order** (so the first-added ends up on top), then clear the list.
   b. `top` = the stack's top. Repeat while `addedThisLayer` is empty: run **one step** of `top`; if that step reports "no more steps", **pop** `top` and leave this
      inner loop.
   c. (If steps of `top` added updates, the inner loop ends at once, `top` stays on the stack *below* them, and the outer loop moves the new updates on top: they, and
      everything they add, run to completion **before `top` continues** with its next step.)
5. When the loop ends (also on an exception) the stack and `addedThisLayer` are cleared and `count` is reset to 0.
So: updates added by an update run **after that step finishes, in the order they were added, each fully (with its own descendants) before the next sibling**, and the
remaining steps of the parent run afterwards. An update added when **nothing is running** runs **immediately and completely** before the call returns, and the `count` and
the stack are fresh for the next top-level call (each of the six shape updates of an idle `setBlock` is therefore its own chain).

**Worked example.** Chain idle; `setBlock` at P with bit 1 set queues a Multi update M (steps W, E, D, U, N, S) and runs it. Step W calls `neighborChanged` of the block at
P+W; suppose that block does `setBlock` at P+W with bit 1: this queues Multi M2 (running → added to the list). Step W of M ends; the loop moves M2 on top and runs *all six
steps of M2* (and everything M2's steps add), then pops M2; only then does M run step E.

### 2. Neighbour-changed entry points and parameters

1. **`updateNeighborsAt(pos, sourceBlock)`**: orientation = `initialOrientation(level, none, none)` (§7; `null` in vanilla), then
   `updateNeighborsAtExceptFromFacing(pos, sourceBlock, skipDirection = none, orientation)`.
2. **`updateNeighborsAtExceptFromFacing(pos, sourceBlock, skipDirection, orientation)`**: adds one **Multi** update. Its steps, in order: for `d` in `NEIGHBOUR_ORDER`
   except `skipDirection`: `nbPos = sourcePos + d`; `state = blockState(nbPos)` (**read when the step runs**, so changes made by earlier steps are visible);
   `orientation' = null` (or, in the redstone experiment, the orientation with its front set to `d`); run the **neighbour-changed hook** of `state` (§2.5).
   (`skipDirection` is skipped at the start and between steps.)
3. **`neighborChanged(pos, sourceBlock, orientation)`** (Simple): one step: `state = blockState(pos)` at run time; run the hook with `movedByPiston = false`.
4. **`neighborChanged(state, pos, sourceBlock, orientation, movedByPiston)`** (Full): one step with the supplied state (not re-read).
5. **The hook call** (`handleNeighborChanged`): `block.neighborChanged(state, level, pos, sourceBlock, orientation, movedByPiston)` where `state` is the state of the notified
   block, `pos` is **its** position, `sourceBlock` is the block *type* that changed — **not** a position — and `movedByPiston` is false except for the explicit Full call by pistons.
   **There is no "from position" argument in 26.3**; blocks that need it recompute it from the six neighbours.
6. **`updateNeighbourForOutputSignal(pos, changedBlock)`** (comparator notification; called by `setBlock` when the new state has an analog output, by `BlockEntity.setChanged`
   when the block is not air, and by container code): for `d` in the horizontal order N, E, S, W: `n = pos + d`; if `n` is in a loaded chunk: `s = blockState(n)`;
   if `s` is a **comparator**: Full update `(s, n, changedBlock, null, false)`; else if `s` is a **redstone conductor** at `n`: `n2 = n + d`; `s2 = blockState(n2)`;
   if `s2` is a comparator: Full update `(s2, n2, changedBlock, null, false)`. (The Full updates go through the chained updater, so they obey rule 1 and run *after* the
   current step if a chain is running, or immediately if idle.)
7. **Callers' source block:** `Level.setBlock` passes the **old** block as `sourceBlock`; `/setblock`-style code passes the new one
   ([`set-block-flags-and-sequence.md`](set-block-flags-and-sequence.md) §1.5, §2.6).

### 3. Shape updates: who starts them, in which order, with which arguments

1. **Starting point** (`BlockState.updateNeighbourShapes(level, pos, flags, limit)`): for `d` in `SHAPE_ORDER` (W, E, N, S, down, up): `nb = pos + d`;
   call `level.neighborShapeChanged(d.opposite, nb, pos, thisState, flags, limit)`. Meaning of the arguments for the neighbour at `nb`: "the neighbour in direction
   `d.opposite` (from `nb`'s point of view the changed block is on that side) changed to `thisState`, you are at `nb`".
2. **On a `Level`** (`neighborShapeChanged`): adds a **Shape update** `(direction = d.opposite, neighbourState = thisState (captured now), pos = nb, neighbourPos = pos, flags,
   limit)` to the chained updater (§1) — **limit unchanged**. **On any other level accessor** (the generation region, a plain accessor): the update is executed **immediately**
   and **not queued**, with `limit − 1`.
3. **Executing one shape update** (`executeShapeUpdate(direction, pos, neighbourPos, neighbourState, flags, limit)`):
   a. `current = blockState(pos)` (read when it runs).
   b. If `(flags & 128) != 0` and `current` is redstone wire: **do nothing** (skip).
   c. `new = current.updateShape(level, level, pos, direction, neighbourPos, neighbourState, level.random)` — the **shape-update hook**
      (§ [`block-hooks.md`](block-hooks.md)); `neighbourState` is the **captured** state from step 1, which may be stale by now; the random source is the
      level's `random` (the generation region's positional random there).
   d. `Block.updateOrDestroy(current, new, level, pos, flags, limit)`: if `new == current` nothing; else if `new` is air: server: `destroyBlock(pos, dropResources = (flags & 32) == 0,
      no breaker, limit)` (drops the old block's loot — the survival-check path of plants, torches, …); else `setBlock(pos, new, flags & ~32, limit)`.
4. **The recursion limit.** `setBlock(pos, state, flags, limit)` starts shape updates only if `(flags & 16) == 0` and `limit > 0`, and passes `limit − 1` down; a shape update
   that changes a block calls `setBlock(…, limit)` with the limit it carries, so each nested shape-driven write consumes one level: **at most 512 nested shape-driven writes**
   per top-level `setBlock`; at `limit = 0` the block is still written but its shape updates are not started. Neighbour-changed updates have no depth limit other than `maxChain`.
5. **Which write flags the nested write carries:** the originating `setBlock`'s flags with bits 1 and 32 cleared (shape-update flags), and bit 32 cleared again by `updateOrDestroy`.
   Because bit 1 is cleared, **a shape-driven replacement never notifies neighbours** (no `neighborChanged`, no `affectNeighborsAfterRemoval` unless moved by a piston); it does
   send to clients (bit 2 if the original had it), start its own shape updates (bit 16 clear) and run `onPlace`. The **air** case is different: it calls `destroyBlock` with flags **3**,
   which does notify neighbours.
6. **Indirect updates** (`updateIndirectNeighbourShapes(level, pos, flags, limit)`): a block hook with an empty default, started by `setBlock` for the **old** state, then
   (after the direct shape updates) for the **new** state, and by a few block-specific code paths; it is how redstone wire and rails reach blocks two steps away. It receives
   the masked flags and `limit − 1`. The per-block behaviour belongs to the redstone / rail readers.
7. **Order inside one `setBlock`** (bit 16 clear, `limit > 0`): `old.updateIndirectNeighbourShapes` → six shape updates of the new state in `SHAPE_ORDER` → `new.updateIndirectNeighbourShapes`.
   If the chain is idle each of the six runs to completion before the next is created; if a chain is running they are queued and run in order after the current step.

### 4. `Block.updateFromNeighbourShapes(state, level, pos)` (pure reads; no writes)

For `d` in `SHAPE_ORDER`: `state = state.updateShape(level, level, pos, d, pos + d, blockState(pos + d), level.random)` — note the direction argument is the direction
**towards** the neighbour here (`d`), and the state is threaded through all six. Used by block placement helpers, post-processing of generated chunks
([`scheduled-ticks-and-block-events.md`](scheduled-ticks-and-block-events.md) §5.5), structure placement and legacy upgrades. The result is not written by this function.

### 5. The shape-update hook contract (summary; details per block elsewhere)

`updateShape(state, level, scheduledTickAccess, pos, directionToNeighbour, neighbourPos, neighbourState, random) → newState`:
- `state`: the notified block's current state; `pos`: its position;
- `directionToNeighbour`: where the changed neighbour is relative to `pos`; `neighbourPos`, `neighbourState`: its position and (new, possibly stale) state;
- `scheduledTickAccess`: the level itself, to **schedule block / fluid ticks** (e.g. water schedules a fluid tick, falling blocks schedule themselves, plants that
  can no longer survive return air);
- the return value replaces the state: returning the same object means no write; returning air means the block is destroyed with drops (unless bit 32).
The hook must not write blocks itself (it may schedule ticks).

### 6. Structure placement: `updateShapeAtEdge`

Used by template placement when the piece's `knownShape` is false ([`../worldgen/structures-placement-and-jigsaw.md`](../worldgen/structures-placement-and-jigsaw.md) §4 step 7), with
`updateMode` = the piece's flags and `shape` = a voxel set of all placed positions (box-relative, origin at the box minimum). It walks the **faces of the voxel set**:
1. **Face walk order** (`forAllFaces`): three passes. In each pass an outer loop `a` (ascending), a middle loop `b` (ascending) and an inner scan along `c` (ascending, from 0 to
   the size along `c`, inclusive of one step past the end) that finds maximal *runs* of filled cells. Faces are emitted in scan order: the **first-face** of a run is emitted
   when the run's first cell is reached (at that cell); the **end-face** of a run is emitted when the first non-filled cell (or the end of the line) is reached (at the run's last cell):
   - pass 1: `a = x`, `b = y`, `c = z`; first-face **NORTH**, end-face **SOUTH**;
   - pass 2: `a = z`, `b = x`, `c = y`; first-face **DOWN**, end-face **UP**;
   - pass 3: `a = y`, `b = z`, `c = x`; first-face **WEST**, end-face **EAST**.
   Each emitted face carries the box-relative `(x, y, z)` of the filled cell it belongs to.
2. For each emitted face `(direction, x, y, z)`: `pos = start + (x, y, z)`; `nb = pos + direction`; read `state = blockState(pos)`, `nbState = blockState(nb)`;
   `new = state.updateShape(level, level, pos, direction, nb, nbState, level.random)`; if `new != state`: `setBlock(pos, new, updateMode & ~1)`;
   then `newNb = nbState.updateShape(level, level, nb, direction.opposite, pos, new, level.random)` — **using `new` (the possibly just-changed state) as the neighbour state** —
   and if `newNb != nbState`: `setBlock(nb, newNb, updateMode & ~1)`. (In the generation region these writes are plain chunk writes: no further updates; flag 16 decides post-process marking.)
3. After the faces, for every placed block (placement order): if `knownShape` is false: `state = blockState`; `new = Block.updateFromNeighbourShapes(state, level, pos)`;
   if changed `setBlock(pos, new, (updateMode & ~1) | 16)`; then `level.updateNeighborsAt(pos, new.block)` (a no-op in the generation region); then the block-entity
   `setChanged` for pieces that carry NBT.

### 7. Redstone orientation (only when the `redstone_experiments` feature flag is enabled; `null` otherwise)

1. `initialOrientation(level, front, up)`: **if the flag is off return `null` and draw nothing.** Otherwise draw `i = level.random.nextInt(48)`; take orientation number `i`; set its side
   bias to LEFT; if `up` is non-null set up; if `front` is non-null set front; return it.
2. An orientation is `(up, front, sideBias)` with `up` and `front` on different axes; **index = ((up.ordinal · 4 + frontKey) · 2 + bias)**, `frontKey = frontAxisKey · 2 + (front's axis direction
   ordinal: POSITIVE 0, NEGATIVE 1)`, `frontAxisKey` = (if `up` is on the Y axis: 1 when `front` is on X else 0; otherwise: 1 when `front` is on Y else 0), `bias` LEFT 0 / RIGHT 1.
   Valid indices 0–47; `ordinal` is the DOWN 0 … EAST 5 order.
3. `withFront(f)`: new front `f`; new up = `front.opposite` if `f == up`, `front` if `f == up.opposite`, else `up`. `withUp(u)`: new up `u`; new front = `up.opposite` if `u == front`,
   `up` if `u == front.opposite`, else `front`. `withSideBias(b)` changes only the bias. `side` = the direction nearest to `front × up` (cross product) for RIGHT, its opposite for LEFT.
   Neighbour order of an orientation: `[front.opposite, front, side, side.opposite, up.opposite, up]`; horizontal / vertical sub-lists filter by the up axis.
4. In a Multi update the orientation handed to step `d` is `orientation.withFront(d)`; if the Multi was created with a `null` orientation it lazily creates
   `initialOrientation(level, skipDirection.opposite or none, none)` at its first step (one `nextInt(48)` draw) and keeps it.
5. The experiment is not part of the default game; the redstone reader specifies what blocks do with it.

## Edge cases and known quirks

1. **Depth-first, not breadth-first:** a block that reacts to a neighbour update by changing blocks gets *its* neighbours notified before the original multi-update continues with its next direction.
2. **The notified state is re-read for Multi / Simple updates but captured for Full / Shape updates** — shape updates therefore can act on a stale neighbour state.
3. **`neighborChanged` source block is the old block** for `setBlock`; there is **no source position**.
4. **Different direction orders** for neighbour updates (W E D U N S) and shape updates (W E N S D U); the output-signal scan uses N E S W.
5. **Shape-driven writes never notify neighbours** (bit 1 is masked) — a shape change of a fence does not wake redstone around it, but a block turned to air does call `destroyBlock`
   with flags 3, which *does* notify (it passes bit 1).
6. **`count` is per chain:** 1,000,000 updates in one chain is the limit; the first excess update is logged and the rest dropped silently; unlimited when the property is negative.
7. **Idle `setBlock` shape updates** are six independent chains (rule 1 end), so the `count` budget does not accumulate across the six directions.
8. **Generation-region accessors run shape updates immediately and with one less budget** (§3.2) — relevant to `Block.updateOrDestroy` calls by structure and feature code through the region.
9. **`updateOrDestroy` air case on the client does nothing** (the server decides).
10. **Orientation draws** (only with the experiment): one `nextInt(48)` per `updateNeighborsAt` call, consumed *before* the neighbours are notified.

## Constants and IDs

`NEIGHBOUR_ORDER` = W, E, DOWN, UP, N, S; `SHAPE_ORDER` = W, E, N, S, DOWN, UP; comparator scan = N, E, S, W; shape recursion limit 512; chain limit 1,000,000 (`max-chained-neighbor-updates`);
bit masks used: `~(1|32)` (= −34) for shape-update flags, `~32` (= −33) in `updateOrDestroy`, 128 skips wire; orientation index count 48; `nextInt(48)` draw.

## Oracle probe points

- Order of neighbour updates: set the public debug listener `CollectingNeighborUpdater#setDebugListener(Consumer<BlockPos>)` (called with every position of an entry each time that entry
  becomes the current top of the stack, including when it is resumed) — or instrument `BlockBehaviour$BlockStateBase#handleNeighborChanged(Level, BlockPos, Block, Orientation, boolean)` and
  `NeighborUpdater#executeUpdate` / `#executeShapeUpdate` (public static) and log `(seq, pos, sourceBlock, state)`.
- Shape updates: instrument `BlockBehaviour$BlockStateBase#updateShape(LevelReader, ScheduledTickAccess, BlockPos, Direction, BlockPos, BlockState, RandomSource)` and
  `updateNeighbourShapes(LevelAccessor, BlockPos, int, int)` / `updateIndirectNeighbourShapes(...)`, log `(seq, pos, direction, neighbourPos, neighbourState)`.
- Drive: `Level#setBlock(BlockPos, BlockState, int, int)`, `Level#updateNeighborsAt(BlockPos, Block)`, `Level#updateNeighborsAtExceptFromFacing(BlockPos, Block, Direction, Orientation)`,
  `Level#neighborChanged(...)`, `Level#updateNeighbourForOutputSignal(BlockPos, Block)`, `BlockState#updateNeighbourShapes`, `Block.updateFromNeighbourShapes`, `StructureTemplate#updateShapeAtEdge`.
- Chain limit: construct a long chain (a 2000-long observer / redstone-lamp line or a command-block loop) with `max-chained-neighbor-updates` set to 50; log the first skipped position (error log line).
- Experiment: `Level#enabledFeatures()` and `ExperimentalRedstoneUtils#initialOrientation(Level, Direction, Direction)`.

## Open questions

1. **[oracle] Chain dropping position and count semantics** (rule 1.1–1.2): verify that dropped updates count towards `count` and that the next top-level call starts a fresh count.
   Probe: limit 3; one `setBlock` of a block with six reacting neighbours; log which neighbours receive the hook.
2. **[oracle] Debug-listener positions as an order probe** — confirm that the listener reports Multi positions *before* the Multi's first step and again when resumed; if so, de-duplicate.
3. **[oracle] Stale neighbour state** (Edge 2): probe with two stairs side by side where the first shape update changes the neighbour; log the `neighbourState` argument vs the world state at that moment.
4. **[oracle] Face-walk order** (§6.1) on an L-shaped voxel set: probe `DiscreteVoxelShape#forAllFaces` through a `BitSetDiscreteVoxelShape` and compare the `(direction, x, y, z)` sequence with Zenith's.
5. **[oracle] The "bit 1 masked" claim** (§3.5) for the shape-driven `destroyBlock` flags: the written value is `3` for the air case (passes bit 1) and `flags & ~32 & ~1` for the replace case; probe with a fence next to a door that breaks.
6. (implementer) The comparator notification (§2.6) uses `isRedstoneConductor` (a block-state predicate listed in `blocks/state-facts.md`).
7. (implementer) `InstantNeighborUpdater` exists in the game but no level in 26.3 constructs it; it needs no implementation.

## Parity status

Not yet run.
