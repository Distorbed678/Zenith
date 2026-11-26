# Redstone: pistons, sticky pistons, the push resolver and the moving-piston block entity (26.3)

- Verified by suites: `redstone`
- Sources: local reference reading of the 26.3 server code (2026-11-26, reader session): the piston base block (shared by normal and sticky pistons), the piston head,
  the moving-piston block and its block entity, the push-structure resolver, the movement helper, and the level's block-event loop, `isHandlingTick` and block-entity
  ticker registration as far as pistons depend on them. Everything is in our own words.
- Related specs: [`redstone.md`](redstone.md), [`redstone-power.md`](redstone-power.md) (§4 the piston's input rule, §2 helpers),
  [`redstone-components.md`](redstone-components.md); [`scheduled-ticks-and-block-events.md`](scheduled-ticks-and-block-events.md) §6 (block events),
  [`server-tick.md`](server-tick.md) §5 (order inside a level tick), §8 (block-entity tickers), [`set-block-flags-and-sequence.md`](set-block-flags-and-sequence.md)
  (flags 64 / 256 / 4 and the hooks they gate), [`neighbor-and-shape-updates.md`](neighbor-and-shape-updates.md), [`../java/hash-collections.md`](../java/hash-collections.md)
  (the `HashMap` order of §5.8), [`../blocks/block-table.md`](../blocks/block-table.md) (`push:` reaction, hardness, block entities).

## Summary

A piston base reads its input when it is notified, and — instead of acting at once — queues a **block event**. The event is processed at the end of the same level tick
(after all scheduled and random ticks). Extension and retraction replace the moved blocks by **moving-piston** blocks backed by block entities that advance 0 → 0.5 → 1 in
two ticks and put the real block down on the third. The *push structure* (which blocks move, with the limit of 12 and the exact list order) is computed by a resolver that
walks lines of blocks and the sticky (slime / honey) blocks' side branches.

## Inputs and state

- **Piston base** (`piston`, `sticky_piston`; the same class, a flag distinguishes sticky): `FACING` (any of 6, default NORTH), `EXTENDED` (default false). Never a redstone conductor (so also
  not a strong-power carrier); push reaction `IMMOVEABLE` as a block (but a *retracted* piston can still be pushed, §4.2); hardness 1.5; it has **no** block entity (so it is pushable by the rule "no block entity").
- **Piston head** (`piston_head`): `FACING`, `TYPE` (`normal` / `sticky`), `SHORT` (default false); push reaction `IMMOVEABLE`; hardness 1.5; no loot table.
- **Moving piston** (`moving_piston`): `FACING`, `TYPE`; indestructible (hardness −1), dynamic shape, not a conductor, not suffocating, push reaction `IMMOVEABLE`; its block entity carries all data. Its `getTicker` always exists (the entity's `tick`).
- **Event ids** (`blockEvent(pos, pistonBlock, id, facing3D)`): `0` extend, `1` retract (pulls the block in front for a sticky piston), `2` retract **without** pulling ("drop"). `facing3D` = `Direction.get3DDataValue` (DOWN 0, UP 1, NORTH 2, SOUTH 3, WEST 4, EAST 5).
- **Moving block entity** (`PistonMovingBlockEntity`): `movedState` (a block state), `direction` (the **piston's facing**, not the movement direction), `extending`, `isSourcePiston`, `progress` (f32, 0..1), `progressO` (f32, previous), `lastTicked` (i64 game time of its last tick). NBT: `blockState` (state), `facing` (3D data value int), `progress` (saved from `progressO`; both fields restored equal), `extending`, `source` (bools). Movement direction `M = extending ? direction : direction.opposite`.
- **Constants:** push limit 12; progress step 0.5 per entity tick (so 2 ticks to reach 1); entity push offset 0.01; flags used: 2, 18, 67, 82, 276, 324, 340 (§5, §6).
- **Direction orders:** `Direction.values()` = DOWN, UP, NORTH, SOUTH, WEST, EAST.

## Behavior (ordered rules)

### 1. When a piston looks at its input — `checkIfExtend(pos, state)`

Called from: `neighborChanged` (server), `setPlacedBy` (server), and `onPlace` only if the old block was **not** a piston **and** there is no block entity at `pos` (so a piston being put down by its own moving block,
whose entity was just removed, re-checks; a piston placed over a moving piston whose entity still exists does not). Also the piston head forwards its neighbour updates to the base (§7.3).
1. `facing = FACING`; `power = getNeighborSignal(pos, facing)` (§2).
2. **Extend case** `power && !EXTENDED`: construct the resolver for extension (`pos`, `facing`, `extending = true`) and call `resolve()` (§4). If it succeeds: `blockEvent(pos, piston, 0, facing3D)`. If it fails: nothing (the piston silently stays retracted).
3. **Retract case** `!power && EXTENDED`: `p2 = pos + facing·2`, `s2 = state(p2)`. Event id = **2** if **all** of: `s2` is a moving piston with `FACING = facing`, the block entity at `p2` is a moving-piston entity that is **extending**, **and** (its `getProgress(0)` — i.e. `progressO` — is `< 0.5`, **or** `gameTime == its lastTicked`, **or** the level `isHandlingTick()`); otherwise **1**. `blockEvent(pos, piston, id, facing3D)`.
   (`isHandlingTick` is true throughout `ServerLevel.tick` steps 1–9 of server-tick §5 — scheduled ticks, random ticks, block events — and false during the entity and block-entity phases and outside the level tick, e.g. while a player's packet is processed.)
4. Otherwise nothing.

### 2. The input — `getNeighborSignal(pos, facing)` (power §4)

True iff **any** of the following holds, tested in this order (result is a boolean; order only matters for the number of hook calls):
1. for `d` in `Direction.values()` with `d ≠ facing`: `hasSignal(pos + d, d)`;
2. `hasSignal(pos, DOWN)` (the piston's own position; a piston is not a conductor, so this is practically false);
3. with `a = pos + UP`: for `d` in `Direction.values()` with `d ≠ DOWN`: `hasSignal(a + d, d)`.
So a piston is powered by any signal that would power the block **above** it — quasi-connectivity — except from below that block, and by every neighbour except the one it pushes towards.

### 3. Event processing — `triggerEvent(state, pos, id, data)`

Runs from the block-event loop (scheduled-ticks §6.3: only for positions in block-ticking range, only if the block there is still this piston type; `true` ⇒ client packet). `facing = FACING`; `extendedState = state with EXTENDED = true`.
1. **Server re-check:** `power = getNeighborSignal(pos, facing)` evaluated **now**.
   - If `power` and `id ∈ {1, 2}`: `setBlock(pos, extendedState, flags 2)` and **return false** (the retract request is cancelled because power came back; the piston stays / becomes extended without moving anything; no extension animation happens).
   - If `!power` and `id = 0`: **return false** (the extension is cancelled: the 0-tick case).
2. **`id = 0` (extend):** `moveBlocks(pos, facing, extending = true)` (§5); if it returns false → return false. Else `setBlock(pos, extendedState, flags 67)` (`1|2|64`: neighbours notified with old block = piston; shape updates; movedByPiston = true so `onPlace` of the piston gets `movedByPiston = true`, and its `affectNeighborsAfterRemoval` is **not** called because the block type is unchanged); extend sound (volume 0.5, pitch `random.nextFloat() × 0.25 + 0.6` — **one level-random float**); game event `block_activate`. Return true.
3. **`id ∈ {1, 2}` (retract):**
   a. If the block entity at `pos + facing` is a moving-piston entity: call its `finalTick()` (§6.3) — this completes an in-flight extension arm at once.
   b. `movingState = moving_piston(FACING = facing, TYPE = sticky ? sticky : normal)`; `setBlock(pos, movingState, flags 276)` (`256|16|4`: no neighbour notification, no shape update, no block-entity side effects); `setBlockEntity(MovingPiston(pos, movingState, movedState = piston base with FACING = Direction.from3DDataValue(data & 7) and EXTENDED = false, direction = facing, extending = false, isSourcePiston = true))`; `updateNeighborsAt(pos, moving_piston block)`; `movingState.updateNeighbourShapes(pos, flags 2)` (limit 512).
   c. **Sticky piston:** `p2 = pos + facing·2`; `s2 = state(p2)`. If `s2` is a moving piston whose entity has `direction = facing` and is extending: `finalTick()` on it and treat the piece as handled (**no** pulling). Otherwise: if `id ≠ 1`, or `s2` is air, or `isPushable(s2, p2, facing.opposite, allowDestroyable = false, connection = facing)` is false, or (`s2`'s push reaction is not `PUSH_PULL` **and** `s2` is not a piston / sticky piston): `removeBlock(pos + facing, false)` (the head, flags 3). Else (`id = 1`, pullable): `moveBlocks(pos, facing, extending = false)` (§5; it removes the head itself).
   d. **Non-sticky:** `removeBlock(pos + facing, false)` (removes the head: `affectNeighborsAfterRemoval` of the head finds no fitting base because the base is now a moving piston).
   e. Retract sound (volume 0.5, pitch `random.nextFloat() × 0.15 + 0.6` — one level-random float); game event `block_deactivate`. Return true.

### 4. The push structure — `PistonStructureResolver(level, pistonPos, facing, extending)`

Fields: `pushDir = extending ? facing : facing.opposite`; `start = extending ? pistonPos + facing : pistonPos + facing·2`; ordered lists `toPush` and `toDestroy`.
**4.1 `isPushable(state, pos, moveDir, allowDestroyable, connectionDir)`** (public static; used by the resolver and by §3.3c), in this order:
1. false if `pos.y < minY` or `pos.y > maxY` or the position is outside the world border.
2. true if the state is air.
3. false if `moveDir = DOWN` and `pos.y = minY`; false if `moveDir = UP` and `pos.y = maxY`.
4. If the state is **not** a piston / sticky piston: false if its destroy speed is −1 (hardness −1, e.g. bedrock, barrier, moving piston); then by push reaction: `IMMOVEABLE` → false; `POPPED` → `allowDestroyable`; `PUSH` → `moveDir == connectionDir`; `PUSH_PULL` and `IGNORE_ENTITY` fall through. If it **is** a piston: false if `EXTENDED`.
5. Result `!state.hasBlockEntity()` (any block with a block entity is immovable, including chests, furnaces, hoppers, beacons, banners, signs, skulls, comparators, daylight detectors …).
**4.2** A retracted piston is pushable (its reaction `IMMOVEABLE` is skipped by the "piston" branch) and, once pushed, **still has `EXTENDED = false`**; an extended piston or any head is not.
**4.3 `resolve()`:**
1. Clear both lists. `s = state(start)`.
2. If `!isPushable(s, start, pushDir, false, facing)`: if `extending` and `s`'s push reaction is `POPPED`: `toDestroy = [start]` and **return true**; otherwise return false (a non-pushable block directly in front blocks an extension; during retraction §3.3c only starts a pull when the block is pushable).
3. Else: if `!addBlockLine(start, pushDir)` return false. Then walk `i = 0, 1, 2 …` over `toPush` **as it grows**: for `pos = toPush[i]`: if its block is slime or honey and `!addBranchingBlocks(pos)` → return false. Return true.
**4.4 `isSticky(state)`** = slime block or honey block. **`canStickToEachOther(a, b)`**: false for (honey, slime) in either order; otherwise true iff at least one of `a`, `b` is sticky. (So slime / honey stick to **any** neighbouring pushable block, but slime and honey do not stick to each other.)
**4.5 `addBlockLine(start, connectionDir)`** — returns false only when the structure is invalid:
1. `s = state(start)`; if air → true. If `!isPushable(s, start, pushDir, false, connectionDir)` → **true** (this block is simply not part of the structure; the caller proceeds).
2. If `start == pistonPos` → true. If `start ∈ toPush` (already present) → true.
3. `count = 1`; if `count + |toPush| > 12` → false.
4. **Backwards walk** while the current block (initially `s`) is sticky: `pos = start − pushDir·count` (one step further back), `prev = current`, `current = state(pos)`; stop the walk if `current` is air, or `!canStickToEachOther(prev, current)`, or `!isPushable(current, pos, pushDir, false, connection = pushDir.opposite)`, or `pos == pistonPos`; otherwise `count += 1` and, if `count + |toPush| > 12` → return false.
5. **Append** `count` blocks to `toPush`: for `i = count − 1` down to 0: `start − pushDir·i` (the **rearmost first**, ending with `start`); `added = count`.
6. **Forward walk**, `i = 1, 2, …`: `pos = start + pushDir·i`.
   - If `pos` is already in `toPush` (at index `c`): `reorder(added, c)`; then for `j = 0 … c + added` (inclusive): `b = toPush[j]`; if `b` is sticky and `!addBranchingBlocks(b)` → return false; return true.
   - `n = state(pos)`: if air → return true. If `!isPushable(n, pos, pushDir, allowDestroyable = true, connection = pushDir)` or `pos == pistonPos` → **return false**. If `n`'s push reaction is `POPPED` → `toDestroy.add(pos)`; return true. If `|toPush| ≥ 12` → return false. Else `toPush.add(pos)`, `added += 1`, continue.
**4.6 `reorder(added, c)`** (collision with an earlier line): split `toPush` into `head = toPush[0 … c)`, `tail = the last 'added' elements`, `middle = toPush[c … size − added)`; new list = `head ++ tail ++ middle`.
**4.7 `addBranchingBlocks(from)`:** `fs = state(from)`; for `d` in `Direction.values()` **excluding every direction on the axis of `pushDir`**: `np = from + d`; if `canStickToEachOther(state(np), fs)` and `!addBlockLine(np, d)` → return false. Return true.
**4.8 Facts:** (a) the 12-limit counts `toPush.size()` including duplicates; the resolver does **not** de-duplicate positions that enter `toPush` through the backward walk of a *different* line (only a line's `start` is checked against the list), so a position can appear twice and counts twice — keep the plain list **[oracle]**; (b) `toDestroy` receives at most the one block that terminates a line; (c) the order of `toPush` is the order in which blocks will be processed *backwards* by `moveBlocks` (§5.5); (d) blocks with `POPPED` reaction (torches, plants, redstone wire, buttons, levers, repeaters, pressure plates, …) at the head of a line are destroyed (dropped) instead of pushed; a `POPPED` block elsewhere in a push line, when reached by a branch from a slime block with `allowDestroyable = false`, is **not** pushable and the branch is ignored.

### 5. Moving the blocks — `moveBlocks(pistonPos, facing, extending)`

`armPos = pistonPos + facing`; `M = extending ? facing : facing.opposite`.
1. If `!extending` and `state(armPos)` is a piston head: `setBlock(armPos, air, flags 276)`.
2. Resolve (a new resolver); if it fails return **false** (nothing has been changed except step 1).
3. `toPush` / `toDestroy` from the resolver. `deleteAfterMove` = a `java.util.HashMap<BlockPos, BlockState>`: for each `pos` in `toPush` **in list order**: `toPushStates.add(state(pos))` and `deleteAfterMove.put(pos, that state)`. An array `toUpdate` of size `|toPush| + |toDestroy|`, index 0.
4. **Destroyed blocks**, for `i` = last … first over `toDestroy`: `st = state(pos)`; drop its resources (`dropResources` with its block entity, no tool; server loot); `setBlock(pos, air, flags 18)` (`16|2`: no neighbour notification, no shape updates); game event `block_destroy`; `toUpdate[idx++] = st`.
5. **Pushed blocks**, for `i` = last … first over `toPush`: `st = state(toPush[i])` (current); `np = toPush[i] + M`; `deleteAfterMove.remove(np)`; `moving = moving_piston(FACING = facing, TYPE = normal)`; `setBlock(np, moving, flags 324)` (`256|64|4`: no neighbour notification; shape updates run; `movedByPiston` = true: the old state at `np` gets `affectNeighborsAfterRemoval(…, true)`); `setBlockEntity(MovingPiston(np, moving, movedState = toPushStates[i], direction = facing, extending, isSourcePiston = false))`; `toUpdate[idx++] = st`.
   (Far end first: each block's moving-piston write lands on the cell of the next block in the line, which has already been processed, or on an empty cell for the foremost block.)
6. **Head (extension only):** `headState = piston_head(FACING = facing, TYPE = sticky ? sticky : normal)` (SHORT false); `deleteAfterMove.remove(armPos)`; `moving = moving_piston(FACING = facing, TYPE = type)`; `setBlock(armPos, moving, flags 324)`; `setBlockEntity(MovingPiston(armPos, moving, headState, facing, extending = true, isSourcePiston = true))`.
7. **Cells that were pushed away and not re-filled** (`deleteAfterMove` now holds only the *vacated* positions): first, for every key in the map's **key iteration order**: `setBlock(pos, air, flags 82)` (`64|16|2`: no neighbour notification, no shape update, `movedByPiston` true so the old state's `affectNeighborsAfterRemoval(…, true)` runs — most blocks do nothing when moved by a piston).
8. **Order of that map:** the map is a plain `HashMap` holding at most 12 entries, so its table stays at 16 buckets with no tree bins. Iteration order = ascending `bucket`, `bucket = (h ^ (h >>> 16)) & 15` with `h = (y + z·31)·31 + x` (wrapping 32-bit, absolute block coordinates, `>>>` unsigned), and within one bucket the **insertion order of §5.3** (removals do not reorder). Entries removed in steps 5–6 are gone from the iteration.
9. Then, for every entry of the map in that same iteration order: `oldState.updateIndirectNeighbourShapes(pos, flags 2)`; `air.updateNeighbourShapes(pos, flags 2)`; `air.updateIndirectNeighbourShapes(pos, flags 2)` (all with limit 512; these three run **per entry** before the next entry).
10. `orientation = initialOrientation(level, resolver.pushDirection, none)` (null in vanilla; draws one `nextInt(48)` with the experiment).
11. **Neighbour notifications:** reset `idx = 0`. For `i` = last … first over `toDestroy`: `st = toUpdate[idx++]`; (server) `st.affectNeighborsAfterRemoval(pos, movedByPiston = false)`; `st.updateIndirectNeighbourShapes(pos, flags 2)`; `updateNeighborsAt(pos, st.block, orientation)`.
12. For `i` = last … first over `toPush`: `updateNeighborsAt(toPush[i], toUpdate[idx++].block, orientation)` (at the **old** positions, source block = the pushed block's type).
13. If `extending`: `updateNeighborsAt(armPos, piston_head, orientation)`.
14. Return true.
**Consequences:** destroyed blocks' drops are spawned before anything moves; every position that holds a moving piston has been written **without** neighbour notification (flag 1 clear) and the notification comes in steps 11–13 after *all* writes, in reverse list order; the vacated cells are written to air before any notification.

### 6. The moving-piston block entity

**6.1 Registration timeline.** The entity is attached with `setBlockEntity` during block-event processing (server-tick §5 step 9). The ticker wrapper is appended to the level's ticker list (not mid-walk), so the entity is ticked by the **same level tick's** block-entity phase (step 12c), `emptyTime < 300` permitting. **[oracle]** confirm the same-tick first tick.
**6.2 `tick(level, pos, state, entity)`** (server and client; the server path):
1. `lastTicked = gameTime`; `progressO = progress`.
2. If `progressO ≥ 1`: **finish** — `removeBlockEntity(pos)` (marks it removed); if the block at `pos` is still a moving piston: `ns = Block.updateFromNeighbourShapes(movedState, level, pos)` (the six pure shape reads, neighbour spec §4); if `ns` is **air**: `setBlock(pos, movedState, flags 340)` (`256|64|16|4`) then `Block.updateOrDestroy(movedState, air, level, pos, 3)` ⇒ the block is destroyed with drops (e.g. a plant that cannot stand there); else: if `ns` has `WATERLOGGED` = true, set it false; `setBlock(pos, ns, flags 67)` (`1|2|64`: neighbours notified with source = moving piston; shape updates; `onPlace` of the new state with `movedByPiston = true`); then a Simple neighbour update `neighborChanged(pos, ns.block, orientation(push dir))` on the block itself.
3. Else (progress < 1): `np = progress + 0.5f`; `moveCollidedEntities(np)`; `moveStuckEntities(np)`; `progress = np`; if `progress ≥ 1`: `progress = 1`.
So the entity needs **three ticks** after creation: 0 → 0.5, 0.5 → 1, then the finish tick (`progressO = 1`).
**6.3 `finalTick()`** (called by a retracting / sticky piston on a neighbouring moving entity and by block removal `preRemoveSideEffects`): if the level exists and (`progressO < 1` or client): `progress = progressO = 1`; remove and mark the entity removed; if the block at the position is still a moving piston: `ns` = **air if `isSourcePiston`** (the head or retracting base vanishes) else `Block.updateFromNeighbourShapes(movedState, …)`; `setBlockAndUpdate(pos, ns)` (flags 3); then Simple update `neighborChanged(pos, ns.block, orientation(push dir))`.
**6.4 Block removal** of a moving piston by other code runs `finalTick` through the entity's `preRemoveSideEffects`. Right-clicking a moving piston with no entity removes it.
**6.5 Collision shape of the moving block (for entities and `getCollisionShape`):** if not extending **and** source **and** the moved state is a piston base: base part = the extended base's collision shape; else empty. While `progress < 1` and the thread-local "no-clip direction" equals the movement direction: only the base part. Otherwise: `st` = (source ? a piston head with `FACING = direction`, `SHORT = (extending ≠ ((1 − progress) < 0.25))` : `movedState`); shape = `st`'s collision shape translated by `direction × extendedProgress`, united with the base part, where `extendedProgress = extending ? progress − 1 : 1 − progress`. The ordinary `getShape` of the moving-piston block is empty.
**6.6 `getProgress(a)`** = `lerp(min(a, 1), progressO, progress)`; offsets = `direction.step × extendedProgress(getProgress(a))`.

### 7. The piston head and the arm

**7.1 Head state:** `FACING` equals the base's facing; `TYPE` equals the base type (normal ↔ piston, sticky ↔ sticky piston). **7.2** `canSurvive`: the block behind (`pos + FACING.opposite`) is a *fitting base* — a piston of the matching type with `EXTENDED = true` and the same `FACING` — **or** a moving piston with the same `FACING`. `updateShape`: for the neighbour behind: air if `!canSurvive`.
**7.3 `neighborChanged`:** if `canSurvive`: Simple update `neighborChanged(pos + FACING.opposite, sourceBlock, orientation.withFront(FACING.opposite))` — the base is notified of everything that happens next to its head.
**7.4 `affectNeighborsAfterRemoval`:** if the base behind is a fitting base: `destroyBlock(basePos, dropResources = true)` (the piston goes with its head; flags 3 via `destroyBlock`). **7.5 `playerWillDestroy`:** in creative the fitting base is destroyed **without** drops.
**7.6 Moving-piston `destroy` hook** (called after a player breaks it): if the block behind it (`pos + FACING.opposite`) is an *extended* piston: `removeBlock(thatPos, false)`.

### 8. Entities (summary; the numerical movement belongs to the entity-physics milestone)

`moveCollidedEntities(newProgress)`: `Δ = newProgress − progress`; the shape is the moving block's collision-related state's collision shape (head for a retracting source base); if non-empty: boxes translated by `extendedProgress` at the *old* progress; entity search area = the swept area of the shape bounds along `M` by `Δ` united with the box; for each entity whose piston push reaction ≠ `IGNORE_ENTITY`: if the moved state is **slime** and the entity simulates movement: set that velocity component to `M.step` (the others unchanged); compute the maximal overlap `delta` over the shape's boxes (`getMovement` per axis, early-out when `delta ≥ Δ`); if `delta > 0`: `delta = min(delta, Δ) + 0.01`; move the entity by `delta × M` with mover type `piston` (with the thread-local no-clip direction set to `M` during the move, then block-effect checks for the moved path); if retracting a source base: additionally fix entities left inside the base (`fixEntityWithinPistonBase`: push out along `M.opposite` by `min(delta, Δ) + 0.01` when the overlap with the base cube differs from the intersected overlap by less than 0.01).
`moveStuckEntities(newProgress)`: only for **honey** and only for horizontal movement: entities in the box `[0, stickyTop, 0]–[1, 1.500001, 1]` (above the moved block, `stickyTop` = the block's top collision y) shifted by the current offset, with push reaction `PUSH_PULL`, on the ground, and (supported by the block position or horizontally inside the box): moved by `Δ` along `M` (no extra 0.01). **[oracle]** both routines (needs entity milestone).

### 9. Timelines (game ticks; `T` = the tick whose step 9 processes the event)

- **Extend** — the input change is noticed (a neighbour update in any phase) and the event is queued; it runs at the **next** step 9 of a level tick (`T`): if the update happened in the scheduled-tick, random-tick or event phases of tick `t` (before step 9) then `T = t`; if it happened later in tick `t` (entity or block-entity phase) or between ticks, `T` is the following level tick. At `T`: the structure is resolved again; moving blocks and the moving head are placed; the base becomes `EXTENDED` (flags 67). The moving entities tick at `T` (progress 0.5), `T+1` (1.0) and `T+2` (final blocks and head placed).
- **Retract:** same shape; base replaced by a retracting moving piston at `T`; head removed (non-sticky) or pulled block begins moving; at `T+2` the base reappears (flags 67 ⇒ `onPlace` ⇒ `checkIfExtend` again) and the pulled block is placed.
- **0-tick pulse (cancelled extension):** if power drops before step 9 of the same tick, `triggerEvent` returns false for `id = 0`: the piston never moves and `EXTENDED` stays false; if power then returns the next neighbour update starts over.
- **Power returns before a retract event runs:** `id ∈ {1, 2}` with power back at event time: the base is set `EXTENDED = true` (flags 2) and nothing moves (§3.1); the head, if still present, stays.
- **Headless piston** (an extended base with no head, e.g. after `/setblock` or structure placement of the base alone): while powered nothing happens; when power is removed `checkIfExtend` still queues a retract event; `triggerEvent` finds no moving entity at `pos + facing`, turns the base into a retracting moving piston and — for a **sticky** piston — `moveBlocks(false)` finds no head to remove and may pull the block two cells in front (a headless sticky piston can pull).
- **Event id 2** is used when an extension arm is still in the first half of its animation (`progressO < 0.5`) or has already been ticked this game tick or the level is inside its tick: the piston retracts but, if sticky, **does not pull** the block (the "drop block" behaviour that makes fast pulses leave the block in place).
- **Moved piston bases** re-check their input on arrival (`onPlace` ⇒ `checkIfExtend` with no entity) — "piston moving a piston" chains.
- **Block events queued during the event loop** (a freshly placed block's `onPlace` / neighbour updates may queue more piston events) are processed in the same loop after the ones already queued (scheduled-ticks §6.4).

## Edge cases and known quirks

1. **Quasi-connectivity** (§2.3) and the **`!power && id = 0` cancellation** (§3.1) are the two behaviours players rely on most; both are deterministic from the rules above.
2. **Unpowered re-check at event time:** the piston re-reads its input when the event runs, so a signal shorter than "the rest of the tick" cancels extension but a signal that *returns* before retraction cancels retraction (state flips to `EXTENDED` with no animation).
3. **`isPushable` allows pushing an immovable block's neighbour but not the block itself:** a non-pushable block inside a *branch* of a slime structure is ignored (the branch simply doesn't move it), while a non-pushable block in the main push direction makes the whole move fail (§4.5.6).
4. **`POPPED` blocks are destroyed only at the end of a line** and only one per line; a destroyed block is not counted against the limit of 12.
5. **Limit counting:** the main line counts each appended block; the backwards slime walk counts before appending; the forward walk refuses at `|toPush| ≥ 12`; the branch recursion shares the same list, so a structure of exactly 12 moves, 13 fails.
6. **Duplicates in `toPush`** (§4.8a) count twice and are written twice; the second write replaces the moving state of the first with the same data.
7. **Flag 324 writes run shape updates but no neighbour notification** (§5.5); the neighbour notification is deferred to steps 11–13 and uses the **old** block type as source. Observers watching a moved block see the move through the shape updates of flag-324 / 82 writes (82 has flag 16: no shape updates; 324 has no 16: shape updates run).
8. **The vacated-cell order** (§5.8) is a `HashMap` order; one of two hash-order dependencies of redstone.
9. **Entity-side-effects:** sound `random` consumption (§3.2, §3.3e) happens on the server level random.
10. **The moving block entity of a piston-pushed block** has `direction = piston facing` for both extension and retraction; the movement direction is derived (`M`).

## Constants and IDs

Push limit 12; progress per tick 0.5; entity offset 0.01; sticky top box height 1.500001; event ids 0 / 1 / 2; flags: moving block writes 324, vacated cells 82, destroyed 18, arm removal 276, piston base after extend 67, final placement 67 (340 when the block cannot stay), base to moving piston on retract 276.
`PushReaction`: `PUSH_PULL` (normal), `PUSH`, `POPPED`, `IMMOVEABLE`, `IGNORE_ENTITY`. Sounds: extend `piston.extend` (vol 0.5, pitch `r·0.25 + 0.6`), contract `piston.contract` (vol 0.5, pitch `r·0.15 + 0.6`).

## Oracle probe points

- `PistonBaseBlock#triggerEvent(BlockState, Level, BlockPos, int, int)`, `#isPushable(BlockState, Level, BlockPos, Direction, boolean, Direction)` (public static), private `#moveBlocks`, `#checkIfExtend`, `#getNeighborSignal`.
- `PistonStructureResolver#resolve()`, `#getToPush()`, `#getToDestroy()` (public); dump both lists for a catalogue of structures (long lines, 12 / 13 blocks, slime 3×3 sheets, slime + honey, slime touching a non-pushable block, POPPED terminators, lines that hit the piston itself, toPush duplicates).
- `PistonMovingBlockEntity#tick(Level, BlockPos, BlockState, PistonMovingBlockEntity)` (public static), `#finalTick()`, `#getProgress(float)`, `#isExtending()`, `#getLastTicked()`; read `progress` / `progressO` through reflection each game tick.
- Block events: `ServerLevel#blockEvent(BlockPos, Block, int, int)` and the private queue; log `(gameTime, pos, id, data)` before/after the event loop. Level tick hooks: `ServerLevel#isHandlingTick()`.
- Vacated-cell order: log `Level#setBlock(BlockPos, BlockState, int, int)` calls with flags 82 during `moveBlocks` for pushes at many coordinates (all 16 buckets, negative coordinates).

## Open questions

1. **[oracle] Same-tick first tick of a newly created moving block entity** (§6.1): log `gameTime` at creation vs the first `tick` call.
2. **[oracle] Duplicate entries in `toPush`** (§4.8a): build a 2×2 slime sheet pushed along its normal and compare the resolver list with the rules.
3. **[oracle] `HashMap` iteration order of `deleteAfterMove`** (§5.8) with many vacated cells whose hashes collide (positions 31 apart in x with y one lower; 961 apart in x with z one lower).
4. **[oracle] Event id 2 conditions** (§1.3): pulse a sticky piston with a block attached at 1, 2, 3 tick pulses from a clock inside block-event processing and from an entity-phase source (dropper-fed hopper clock), log the chosen id and the position of the pulled block afterwards.
5. **[oracle] Headless-piston behaviour** (§9): remove the head by `/setblock` and probe retraction / pulling.
6. **[oracle] Entity pushing** (§8): needs the entity milestone; probe with an item entity and a player on slime and honey, log the position per tick.
7. **[oracle] The `PUSH` reaction case** (§4.1.4) — find the blocks that use it (see `block-table.md` `push:PUSH`) and probe each in a line.
8. (implementer) The moved block's **block-entity-bearing neighbours** never move; `hasBlockEntity` is the property of the *state's block* (`state-facts.md` item 23).

## Parity status

Not yet run.
