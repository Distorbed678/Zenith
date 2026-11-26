# Redstone: the power model and redstone wire (26.3)

- Verified by suites: `redstone`
- Sources: local reference reading of the 26.3 server/client code (2026-11-26, reader session): the signal-getter interface
  (all level helpers), the block-behaviour signal hooks and their defaults, the redstone-wire block, both wire evaluators
  (default and experimental), the orientation type and its utility class, and the `own/weak/strong` overrides of every
  signal-source block. Everything is in our own words.
- Related specs: [`redstone.md`](redstone.md) (index, flags cheat sheet, timing table), [`redstone-components.md`](redstone-components.md),
  [`pistons.md`](pistons.md); [`neighbor-and-shape-updates.md`](neighbor-and-shape-updates.md) (the chained updater, direction
  orders, orientation algebra §7), [`set-block-flags-and-sequence.md`](set-block-flags-and-sequence.md) (flags; `onPlace` runs on **every**
  write without flag 512), [`scheduled-ticks-and-block-events.md`](scheduled-ticks-and-block-events.md), [`block-hooks.md`](block-hooks.md),
  [`../java/hash-collections.md`](../java/hash-collections.md) (the wire's 7-position `HashSet` order, §6.4 here),
  [`../blocks/state-facts.md`](../blocks/state-facts.md) (is-signal-source, redstone-conductor, has-analog-output),
  [`../blocks/block-table.md`](../blocks/block-table.md) (`cond:never|always`, push reaction), [`../blocks/shapes.md`](../blocks/shapes.md)
  (face sturdiness: FULL / CENTER / RIGID).

## Summary

Redstone power is an integer 0–15 that is **pulled** by the consumer from its six neighbours; nothing is pushed. A neighbour answers in
two flavours: a *weak* signal (`getSignal`, what a block shows towards one side) and a *strong / direct* signal (`getDirectSignal`,
what a block is willing to inject *into a conductor block* so that the conductor itself powers its other neighbours). The level helpers
(§2) combine the two. Blocks are notified that something changed only by the neighbour-update machinery of the tick specs; the
algorithms below say what a block re-reads when it is notified.

## Inputs and state

- **Signal range:** `0 … 15` (4 bits; `SIGNAL_MIN` 0, `SIGNAL_MAX` 15, `SIGNAL_NONE` 0).
- **Direction conventions (very important).** In `getSignal(pos, d)` and `getDirectSignal(pos, d)` the direction `d` is the direction **from the
  asking position towards `pos`** (the neighbour). So the asker sits at `pos − d`. A block that "powers its back" answers non-zero
  for the `d` that points *at it from its back*.
- **`Direction.values()` order** = DOWN, UP, NORTH, SOUTH, WEST, EAST. **`Plane.HORIZONTAL`** = NORTH, EAST, SOUTH, WEST.
  **`Plane.VERTICAL` = UP, DOWN** (note: UP first). `getClockWise` of a horizontal direction: N→E→S→W→N.
- **Block hooks (state wrappers)** and defaults: `isSignalSource(state)` false; `ownSignal(state, level, pos)` 0; `getSignal(state, level, pos, d)` =
  `ownSignal`; `getDirectSignal(state, level, pos, d)` 0; `shouldRedstoneWireConnectTo(state, level, pos, d | none)` = `isSignalSource && d ≠ none`;
  `hasAnalogOutputSignal(state)` false; `getAnalogOutputSignal(state, level, pos, d)` 0; `isRedstoneConductor(state, level, pos)` = the property
  predicate (default: the collision shape is a full block; overrides per block in `block-table.md` `cond:`).
- **Wire state:** `NORTH/EAST/SOUTH/WEST` ∈ {`none`, `side`, `up`} (`isConnected` = side or up), `POWER` 0–15. Default: all `none`, power 0.
  Block-level mutable switch `shouldSignal` (initially true; shared by the single wire block, see §6.7).
- **Feature flag** `redstone_experiments` (off in vanilla): selects the experimental wire evaluator and makes every `initialOrientation` call draw a random
  orientation (§7). With the flag off, **all orientation arguments are `null` and nothing draws randomness**.

## Behavior (ordered rules)

### 1. Block-level signal hooks

For a block the three questions are independent; the table of §5 lists the answers of every signal block. Generic reading rules:

1. `getSignal(state, pos, d)` is the *weak* signal shown towards the asker (default: the block's own signal in every direction).
2. `getDirectSignal(state, pos, d)` is the *strong* signal: only these values are copied into a conductor (§2.2). Default 0.
3. `isSignalSource` marks blocks that wire may connect to (default `shouldRedstoneWireConnectTo`), that comparators / diodes treat as a control input (§2.6)
   and that `getBestOwnOrNeighbourSignal` counts.

### 2. Level helpers (all read-only, evaluation order as listed)

1. **`getSignal(pos, d)`**: `s = state(pos).getSignal(level, pos, d)`; if `state(pos)` is a redstone conductor: result `max(s, getDirectSignalTo(pos))`,
   otherwise `s`. (So a conductor shows the strongest strong-signal injected into it, to every side, even the side it came from.)
2. **`getDirectSignal(pos, d)`** = `state(pos).getDirectSignal(level, pos, d)`.
3. **`getDirectSignalTo(pos)`**: maximum of `getDirectSignal(pos + o, o)` over `o` in the order **DOWN, UP, NORTH, SOUTH, WEST, EAST**, returning immediately
   when a value `≥ 15` is seen. (Observable only through the early exit not evaluating later neighbours; results are identical.)
4. **`hasSignal(pos, d)`** = `getSignal(pos, d) > 0`.
5. **`getBestNeighborSignal(pos)`**: `best = 0`; for `d` in `Direction.values()`: `s = getSignal(pos + d, d)`; return 15 as soon as `s ≥ 15`; else keep the maximum.
6. **`hasNeighborSignal(pos)`**: true iff any of `getSignal(pos+DOWN, DOWN)`, `getSignal(pos+UP, UP)`, `getSignal(pos+NORTH, NORTH)`, `getSignal(pos+SOUTH, SOUTH)`,
   `getSignal(pos+WEST, WEST)`, `getSignal(pos+EAST, EAST)` is `> 0` (that order, short-circuit).
7. **`getBestOwnOrNeighbourSignal(pos)`** = `max(getBestNeighborSignal(pos), isSignalSource(state(pos)) ? state(pos).ownSignal : 0)` (used by the sulfur cube; wire does not use it).
8. **`getControlInputSignal(pos, d, onlyDiodes)`** (side inputs of repeaters / comparators, §components 3): if `onlyDiodes`: `isDiode(state(pos)) ? getDirectSignal(pos, d) : 0`
   (`isDiode` = the block is a repeater or comparator). Else: redstone block → 15; redstone wire → its `POWER` (regardless of connections or direction); else if
   `isSignalSource(state)` → `getDirectSignal(pos, d)`; else 0.

### 3. The conductor rule

1. A block is a **redstone conductor** iff its `isRedstoneConductor` predicate holds (default: collision shape is a full cube; `cond:never` / `cond:always` per
   block in `block-table.md`; known `never` blocks include pistons, the moving piston, TNT and leaves — **use the table, do not re-derive**).
2. Only conductors forward strong signals (§2.1). Non-conductors answer only their own `getSignal`.
3. Wire connectivity reads the predicate (§6.2); comparators look through a conductor to the block behind it (components §3.4).
4. A conductor reports exactly the maximum strong signal injected into it (0–15, no attenuation), to every side including the side the power came from.

### 4. Quasi-connectivity and the other "reach" patterns

"Quasi-connectivity" is not a rule but the result of consumers asking about positions other than their own. The complete list in 26.3:

| consumer | positions asked (all through `getSignal` / `hasSignal`, §2) |
|---|---|
| **Pistons** (both) | `hasSignal(pos+d, d)` for `d` in `Direction.values()` **except the push (facing) direction**; then `hasSignal(pos, DOWN)` (the piston's own block, effectively always false because pistons are never conductors); then, with `a = pos + UP`, `hasSignal(a + d, d)` for `d` in `Direction.values()` **except DOWN** (the five neighbours of the block above) — the quasi-connectivity part |
| **Dispenser / dropper** | `hasNeighborSignal(pos)` **or** `hasNeighborSignal(pos + UP)` |
| **Doors** | `hasNeighborSignal(pos)` **or** `hasNeighborSignal` of the *other half* (`pos+UP` for the lower half, `pos+DOWN` for the upper half) — a door sees power around either half, not around the block above the upper half |
| **Door placement** (state for placement) | `hasNeighborSignal(pos)` or `hasNeighborSignal(pos+UP)` |
| everything else that asks `hasNeighborSignal` (note block, TNT, lamp, bell, copper bulb, trapdoor, fence gate, hopper, crafter, command / structure block, skulls, shelf, big dripleaf, rails) | only `hasNeighborSignal(pos)` — **no** quasi-connectivity |
| **Redstone torch** | standing: `hasSignal(pos+DOWN, DOWN)`; wall: `hasSignal(pos + FACING.opposite, FACING.opposite)` |
| **Repeater / comparator** | input: `getSignal(pos + FACING, FACING)` (+ wire special case, components §2.1); sides: §2.8 |
| **Calibrated sculk sensor** | `getSignal(pos + FACING.opposite, FACING.opposite)` as "back signal" |

### 5. Signal table — every block that answers `getSignal` / `getDirectSignal`

Notation: `P` = the block's own signal; "weak" = `getSignal(d)`; "strong" = `getDirectSignal(d)`; `d` as in the direction convention. "attached dir" for
face-attached blocks (lever, buttons) = floor → UP, ceiling → DOWN, wall → FACING.

| block | `isSignalSource` | own `P` | weak (`getSignal`) | strong (`getDirectSignal`) | wire connects (`shouldRedstoneWireConnectTo`) |
|---|---|---|---|---|---|
| redstone block | yes | 15 | `P` all `d` (default) | 0 | default (yes if `d` given) |
| lever, all buttons | yes | 15 if POWERED | `P` all `d` | 15 if POWERED and `d` = attached dir | default |
| pressure plates (both) | yes | stone-style: 15 if POWERED; weighted: `POWER` | `P` all `d` | `P` if `d = UP`, else 0 (i.e. the block **below** the plate) | default |
| detector rail | yes | 15 if POWERED | `P` all `d` | 15 if POWERED and `d = UP` | default |
| standing redstone torch | yes | 15 if LIT | `P` for `d ≠ UP` (nothing into the block below it) | `P` (via `getSignal(DOWN)`) only if `d = DOWN` (the block **above** the torch) | default |
| wall redstone torch | yes | 15 if LIT | `P` for `d ≠ FACING` (nothing towards the wall block) | like standing: only `d = DOWN` | default |
| repeater / comparator | yes | POWERED ? output : 0 (repeater 15; comparator its stored output) | `P` only if `d = FACING`. FACING points at the block's **input** side, so `d = FACING` means the asker sits at `pos − FACING`, the **output** side | same as weak | repeater: only if `d = FACING` or `FACING.opposite`; comparator: default (any horizontal `d`) |
| observer | yes | 15 if POWERED | `P` only if `d = FACING` (asker at the back) | same as weak | only if `d = FACING` |
| target block | yes | `POWER` (0–15) | `P` all `d` | 0 | default |
| daylight detector | yes | `POWER` | `P` all `d` | 0 | default |
| tripwire hook | yes | 15 if POWERED | `P` all `d` | 15 if POWERED and `d = FACING` | default |
| lightning rods (all weathering states) | yes | 15 if POWERED | `P` all `d` | 15 if POWERED and `d = FACING` | default |
| sculk sensor | yes | `POWER` | `P` all `d` | `P` if `d = UP` | default |
| calibrated sculk sensor | yes | `POWER` | `P` for `d ≠ FACING`, 0 for `d = FACING` | `P` if `d = UP` (from the base class) | default |
| lectern | yes | 15 if POWERED | `P` all `d` | 15 if POWERED and `d = UP` | default |
| trapped chest | yes | `clamp(opener count of this chest half, 0, 15)` | `P` all `d` | `P` if `d = UP` | default |
| jukebox | yes | 15 while its song player is playing | `P` all `d` | 0 | default |
| redstone wire | `shouldSignal` (§6.7) | `POWER` | §6.6 | same function as weak, gated by `shouldSignal` | always yes |

Everything else answers 0 / false.

### 6. Redstone wire

**6.1 Support.** Wire can survive iff the block below has a *full* sturdy UP face (`isFaceSturdy(UP)` with the default FULL support type) **or** is a hopper.
It is removed (with drops, `removeBlock` flags 3 semantic) from `neighborChanged` when it cannot survive, and turns to air in `updateShape` when the DOWN
neighbour no longer supports it (drops via `destroyBlock` of the shape-update path).

**6.2 Connection of one side — `connectingSide(pos, d, canConnectUp)`** where `canConnectUp` = the block at `pos+UP` is **not** a redstone conductor
(evaluated *with that position* as the asker). `r = pos + d`, `rs = state(r)`:
1. If `canConnectUp`: `placeable = rs is a trapdoor block  OR  canSurviveOn(r, rs)` (wire could stand on `rs`). If `placeable` **and** the block at `r + UP` accepts
   wire (`shouldRedstoneWireConnectTo(…, none)`: only wire itself, or any block overriding the hook to ignore the direction; repeater/observer return false for `none`) →
   result **UP** if `rs.isFaceSturdy(r, d.opposite)` (default FULL), else **SIDE**.
2. Otherwise: result **NONE** if `rs` does not accept wire from `d` (`rs.shouldRedstoneWireConnectTo(…, d)`) **and** (`rs` is a redstone conductor **or** the block at
   `r + DOWN` does not accept wire with direction `none`); else **SIDE**.
   (So: wire steps down onto a lower wire only past a non-conductor; powered components connect sideways via their own hook.)

**6.3 `connectionState(state, pos)`** (dust / dot rules). `wasDot` = no side of the **input** `state` is connected.
1. `new` = default wire state with `POWER` copied from the input; for each horizontal `d` in N, E, S, W: `new[d] = connectingSide(pos, d, canConnectUp)` (all four recomputed; the
   `canConnectUp` flag is evaluated once).
2. If `wasDot` and `new` is again a dot (no side connected): return `new` (a dot stays a dot).
3. Otherwise let `n, s, e, w` = whether each side of `new` is connected, `nsEmpty = !n && !s`, `ewEmpty = !e && !w` (all computed **before** any change below).
   In this order: if `!w && nsEmpty` → `WEST = side`; if `!e && nsEmpty` → `EAST = side`; if `!n && ewEmpty` → `NORTH = side`; if `!s && ewEmpty` → `SOUTH = side`.
   Return. (Consequences: no connection at all → a full cross ("+"); a single connected side → a straight line through the block; connections on two perpendicular sides
   → a corner with no stubs.)
4. **Placement** (`getStateForPlacement`) = `connectionState(crossState-with-power-0, pos)`; the input is a cross so `wasDot` is false.
5. **Player toggle** (empty-hand use, building allowed): only if the state is a full cross or a dot: new = `connectionState(copy of power with (cross → default, dot → cross), pos)`;
   if it differs: `setBlockAndUpdate(pos, new)` (flags 3), then for horizontal `d` in N, E, S, W: if `connected(old, d) ≠ connected(new, d)` **and** the block at `pos + d` is a redstone
   conductor: `updateNeighborsAtExceptFromFacing(pos + d, wireBlock, d.opposite, orientation)` (orientation `null` in vanilla; with the experiment one
   `initialOrientation(none, UP)` is drawn first, and each call gets `withFront(d)`).

**6.4 Shape update (`updateShape`)** for a neighbour in direction `d` (state returned, never written here):
1. `d = DOWN`: the neighbour supports the wire (§6.1) ? unchanged : **air**.
2. `d = UP`: `connectionState(state, pos)`.
3. Horizontal `d`: `side = connectingSide(pos, d)` (recomputing `canConnectUp` from the block above). If `side.connected == state[d].connected` **and** the state is **not** a full cross:
   return `state` with `state[d] = side` (this is how side ↔ up flips without re-deriving the other sides).
   Else return `connectionState(crossState with POWER = state.POWER and [d] = side, pos)` (the cross is only a vehicle for the power, `wasDot` is false).

**6.5 Indirect shape updates (`updateIndirectNeighbourShapes(state, pos, flags, limit)`)** — started by `setBlock` for the old and new state (set-block §2.7). For each
horizontal `d` (N, E, S, W): `side = state[d]`; if `side ≠ none` and `state(pos + d)` is **not** wire:
1. `lower = pos + d + DOWN`: if it is wire: queue a shape update on `lower` with direction `d.opposite`, neighbour position `lower + d.opposite` (= `pos + DOWN`), neighbour state
   **read now**, with the supplied `flags` and `limit` (via `neighborShapeChanged`; chained updater, neighbour spec §3.2).
2. `upper = pos + d + UP`: likewise if it is wire, neighbour position `upper + d.opposite` (= `pos + UP`).
These reach the wires diagonally below / above that cannot be reached by the six direct shape updates.

**6.6 What a wire shows (`getSignal` / `getDirectSignal`)**
1. If `shouldSignal` is false or `d = DOWN`: 0.
2. `p = POWER`; if `p = 0`: 0.
3. If `d = UP`: `p`.
4. Otherwise (horizontal): the wire shows `p` towards the asker only if the **freshly recomputed** connection state (`connectionState(state, pos)`, not the stored state) has the side `d.opposite`
   connected (that side faces the asker); else 0.
`getDirectSignal` = the same value when `shouldSignal`. `isSignalSource` = `shouldSignal`; `ownSignal` = `POWER`; `shouldRedstoneWireConnectTo` = true.

**6.7 `shouldSignal` toggling.** `getBlockSignal(pos)` (the "signal from non-wire sources") sets `shouldSignal = false`, evaluates `getBestNeighborSignal(pos)`, sets it back to true. While false, **every**
wire answers 0 / not-a-source, so wires never read each other through `getSignal` here; wire-to-wire power comes only from §6.8.2. The flag is a single switch of the single wire block; no writes happen
while it is false.

**6.8 Default evaluator (the one used in vanilla): `updatePowerStrength(pos, state, orientation, skipShapeUpdates)`** (both extra arguments are ignored):
1. `blockSignal = getBlockSignal(pos)`. Target strength = `blockSignal` if it is 15, else `max(blockSignal, incomingWire(pos))`.
2. `incomingWire(pos)`: `w = 0`; for horizontal `d` in N, E, S, W with `n = pos + d`, `ns = state(n)`: `w = max(w, wirePower(n))` (wire power = `POWER` if wire else 0);
   then, with `a = pos + UP`: if `ns` is a conductor **and** `state(a)` is not a conductor → `w = max(w, wirePower(n + UP))`; **else if** `ns` is not a conductor → `w = max(w, wirePower(n + DOWN))`.
   Result `max(0, w − 1)`.
3. If `state.POWER ≠ target` (the `state` argument, i.e. what the caller read):
   a. if `state(pos)` is still **the very same state** as the argument: `setBlock(pos, state with POWER = target, flags 2)` (no neighbour notification of its own, but flag 16 is clear: the six shape updates and both
      indirect shape updates run; `onPlace` runs and is a no-op for wire because the block did not change);
   b. build an **insertion-ordered list** of 7 positions: `pos`, then `pos + o` for `o` in `Direction.values()`; put them in a `java.util.HashSet<BlockPos>` and iterate it: the **iteration order** of that
      set decides the order in which the 7 `updateNeighborsAt(p, wireBlock)` calls are issued. The set never grows beyond 7 elements, so the table has 16 buckets and no tree bins: iteration order = ascending
      `bucket(p)` where `bucket = (h ^ (h >>> 16)) & 15`, `h = (y + z·31)·31 + x` in wrapping 32-bit arithmetic (`>>>` is the unsigned shift; use the absolute block coordinates), ties
      (equal bucket) in insertion order (`pos`, DOWN, UP, NORTH, SOUTH, WEST, EAST);
   c. for each `p` in that order: `updateNeighborsAt(p, wireBlock)` (a Multi update, neighbour spec §2.2; with the chain running they queue, preserving this order, and execute depth-first).
4. If `state.POWER == target` nothing happens (no write, no notification).
5. **Callers:** `neighborChanged` (below), `onPlace`, `affectNeighborsAfterRemoval`.

**6.9 Wire hooks**
1. **`neighborChanged(state, pos, sourceBlock, …)`** (server): (default evaluator) **always** — also when the source block is wire — if the wire can survive: `updatePowerStrength(pos, state, …, false)`; else drop its resources and
   `removeBlock(pos, false)`.
2. **`onPlace(state, pos, oldState, movedByPiston)`**: only if the old block was **not** wire and this is the server: (a) `updatePowerStrength(pos, state, null, true)`; (b) for `d` in **UP, DOWN**
   (`Plane.VERTICAL`): `updateNeighborsAt(pos + d, wireBlock)`; (c) `updateNeighborsOfNeighboringWires(pos)` (§6.10).
3. **`affectNeighborsAfterRemoval(state, pos, movedByPiston)`**: if **not** `movedByPiston`: (a) for `d` in `Direction.values()`: `updateNeighborsAt(pos + d, wireBlock)`; (b) `updatePowerStrength(pos, state, null, false)` — note the world no
   longer holds the wire, so the identity check of §6.8.3a fails and only the notifications of §6.8.3b/c can happen (if the computed target differs from the removed wire's power); (c) `updateNeighborsOfNeighboringWires(pos)`.
4. **`updateShape`** §6.4, **`updateIndirectNeighbourShapes`** §6.5, **`canSurvive`** §6.1.

**6.10 `updateNeighborsOfNeighboringWires(pos)`**: (1) for horizontal `d` in N, E, S, W: `cornerCheck(pos + d)`; (2) for horizontal `d` in N, E, S, W: `t = pos + d`; if `state(t)` is a conductor:
`cornerCheck(t + UP)` else `cornerCheck(t + DOWN)`. `cornerCheck(p)`: if `state(p)` is wire: `updateNeighborsAt(p, wireBlock)` then for `o` in `Direction.values()`: `updateNeighborsAt(p + o, wireBlock)` (**no** `HashSet` here: fixed order p, then D, U, N, S, W, E).

**6.11 Write flags used by wire:** power change `2`; player toggle `3` (`setBlockAndUpdate`); experimental evaluator `2` (plus `128` except for the first wire when the caller asked for shape updates around the initial position, §7.5).

### 7. Experimental wire evaluator and orientation (only with `redstone_experiments`)

Not part of the default game; specified for completeness so the flag can later be supported. All rules below apply **only** when the level's enabled features contain `redstone_experiments`.

**7.1 Orientation type** (the algebra is in `neighbor-and-shape-updates.md` §7.2–7.3): 48 values `(up, front, sideBias)`; index `((up.ordinal·4 + frontKey)·2 + bias)`; `random(rand)` = `ORIENTATIONS[rand.nextInt(48)]`.
Extra operations used here: `withFrontPreserveUp(f)` = same orientation if `f` is on the up axis, else `withFront(f)`; `withUp(u)`; `withSideBias(b)`; `getDirections()` = `[front.opposite, front, side, side.opposite, up.opposite, up]`;
`getHorizontalDirections()` = those of the list whose axis is **not** the up axis (list order kept); `getVerticalDirections()` = those on the up axis (list order kept).
`side` = the direction nearest to `front × up` if bias RIGHT, its opposite if LEFT.
**7.2 `initialOrientation(level, front, up)`** (the single entry point): flag off → `null` (no RNG). Flag on: `o = random(level.random).withSideBias(LEFT)`; if `up` non-null `o = o.withUp(up)`; if `front` non-null `o = o.withFront(front)`; one `nextInt(48)` is consumed per call.

**7.3 Evaluator entry** `updatePowerStrength(initialPos, state, orientation, shapeUpdateAroundInitial)`: a **fresh evaluator object per call** holding `wiresToTurnOff` (FIFO deque), `wiresToTurnOn` (FIFO deque) and `updatedWires` (insertion-ordered map position → packed `(orientationIndex << 4) | power`).
`o0 = (orientation if non-null else random(level.random)).withUp(UP).withSideBias(LEFT)`.

**7.4 Phase A — computing changes**
1. If `state(initialPos)` is wire: record `setPower(initialPos, its current POWER, o0)` and append the position to `wiresToTurnOff`. Else `propagate(initialPos, 0, o0, allowTurningOff = true)`.
2. **Turn-off pass:** while `wiresToTurnOff` non-empty: `p` = removeFirst; `packed = updatedWires[p]`, `o` = its orientation, `old` = its power; `blockPower = getBlockSignal(p)`; `wirePower = incomingWire(p)` (§6.8.2, but `wirePower(q)` consults `updatedWires` first, then the world);
   `new = max(blockPower, wirePower)`. If `new < old`: if `blockPower > 0` and `p ∉ wiresToTurnOn` append it to `wiresToTurnOn`; `toSet = 0`; else `toSet = new`. If `toSet ≠ old` record `setPower(p, toSet, o)`. Then `propagate(p, toSet, o, allowTurningOff = (old > new))`.
3. **Turn-on pass:** while `wiresToTurnOn` non-empty: `p` = removeFirst, `old` from the map, `new = max(getBlockSignal(p), incomingWire(p))`, `o` from the map; if `new > old` record the new power; if `new < old` → internal error (cannot happen); then `propagate(p, new, o, false)`.
4. `setPower(p, v, o)`: if `p` is absent: store `(o, v)`, else keep the **stored** orientation and replace the power.
5. `propagate(p, power, o, allowTurningOff)`:
   a. for `d` in `o.horizontalDirections` (in list order): `enqueue(p + d, power, o.withFront(d), allowTurningOff)`;
   b. for `v` in `o.verticalDirections` (list order): `vp = p + v`, `solid = state(vp)` is a conductor; for each `d` in `o.horizontalDirections`: if `v = UP` and not `solid` → `enqueue(vp + d, …, o.withFront(d), …)`; else if `v = DOWN` and `state(p + d)` is **not** a conductor → `enqueue(vp + d, …)`.
6. `enqueue(q, fromPower, o, allowTurningOff)`: only if `state(q)` is wire: `to = wirePower(q)` (map first, then world). If `to < fromPower − 1` and `q ∉ wiresToTurnOn`: append to `wiresToTurnOn` and `setPower(q, to, o)`. If `allowTurningOff` and `to > fromPower` and `q ∉ wiresToTurnOff`: append to `wiresToTurnOff` and `setPower(q, to, o)`.

**7.5 Phase B — writing.** Iterate `updatedWires` in insertion order with an "is initial wire" flag true for the first entry only: `newPower` = packed low 4 bits; if `state(q)` is wire and its `POWER ≠ newPower`: write it with flags `2`, plus `128` (skip shape updates **on wires**) unless `(shapeUpdateAroundInitial && this is the first entry)`; else **remove** the entry from the map.
**7.6 Phase C — neighbour notifications.** For every remaining entry in insertion order: `o` = its stored orientation, `st = state(q)`; for `d` in `o.directions` (all six): if `isConnected(st, d)` (horizontal: that side is connected; DOWN: always true; UP: false): `n = q + d`, `ns = state(n)`, `no = o.withFrontPreserveUp(d)`;
issue a Full neighbour update `neighborChanged(ns, n, wireBlock, no, false)`; then if `ns` is a conductor: for each `e` in `no.directions` with `e ≠ d.opposite`: Simple update `neighborChanged(n + e, wireBlock, no.withFrontPreserveUp(e))`.
**7.7 In experimental mode** `neighborChanged` of a wire ignores updates whose **source block is wire** (it only reacts to other sources); the evaluator itself handles wire-to-wire propagation.

## Edge cases and known quirks

1. **Wire is a strong source towards the block it points at** (the "up" and horizontal rules of §6.6 apply to `getDirectSignal` too), so a conductor next to powered wire passes it on; but wire power *into* wire never uses §2.
2. **Wire shows the recomputed shape**, not the stored one (§6.6.4): a wire whose neighbour changed but whose state has not yet been refreshed by the shape update still answers by the fresh geometry.
3. **Power of `hasNeighborSignal` ignores the source's identity**: a conductor next to a powered wire that sits *above* it reports that wire's power; a conductor above a wire pointing at it likewise.
4. **`getDirectSignalTo` early exit** and the helper orders (§2) never change results; only the number of block-hook calls (relevant to the probe counters, not to parity).
5. **The `HashSet` order** of §6.8.3b depends on absolute coordinates; two wires one block apart in x notify their 7 positions in different orders. This order is one of the two places (the other: the piston `deleteAfterMove` map, pistons §5.8) where vanilla's behaviour depends on a hash order.
6. **Wire removal with `movedByPiston`** (the wire being moved is a non-pushable POPPED block, so this only occurs for `flags & 64` removals by other code) skips every notification.
7. **A wire whose power is unchanged is silent** even when a neighbour changed — all the chain traffic of a wire network comes from actual power changes.
8. **Dot stays dot (§6.3.2)** only when the *input* was a dot; a wire that loses all connections becomes a cross, a wire with one connection a straight line.
9. **Sulfur cube** is the only reader of `getBestOwnOrNeighbourSignal` (entity milestone).

## Constants and IDs

Signal 0–15; `shouldSignal` default true; evaluator write flags 2 (+128 experimental); chained-updater `HashSet` table 16 buckets, hash `(y + z·31)·31 + x`, spread `h ^ (h >>> 16)`; orientation count 48; wire power loss per block 1;
`getDirectSignalTo` order D, U, N, S, W, E; `Plane.VERTICAL` order UP, DOWN; `Plane.HORIZONTAL` order N, E, S, W.

## Oracle probe points

- Signal queries: `net.minecraft.world.level.SignalGetter#getSignal(BlockPos, Direction)`, `#getDirectSignal`, `#getDirectSignalTo`, `#getControlInputSignal(BlockPos, Direction, boolean)`, `#hasSignal`, `#hasNeighborSignal`, `#getBestNeighborSignal`,
  `#getBestOwnOrNeighbourSignal`; per-block: `BlockBehaviour$BlockStateBase#getSignal(BlockGetter, BlockPos, Direction)`, `#getDirectSignal`, `#getOwnSignal`, `#isSignalSource`, `#shouldRedstoneWireConnectTo`, `#isRedstoneConductor`.
  Dump format: for every block state and every neighbour configuration of a 3×3×3 test volume, log the six `getSignal` / `getDirectSignal` answers.
- Wire: `net.minecraft.world.level.block.RedstoneWireBlock` (public `getBlockSignal(Level, BlockPos)`, `getColorForPower(int)`; private evaluator entry reachable through `DefaultRedstoneWireEvaluator#updatePowerStrength(Level, BlockPos, BlockState, Orientation, boolean)`);
  log the sequence of `Level#updateNeighborsAt(BlockPos, Block)` calls made from that method (order of the 7 positions) to verify §6.8.3b at coordinates covering all 16 buckets and negative values.
- Experimental: `ExperimentalRedstoneWireEvaluator#updatePowerStrength`, `Orientation#random`, `ExperimentalRedstoneUtils#initialOrientation(Level, Direction, Direction)`; enable by data-pack feature flag `redstone_experiments`.

## Open questions

1. **[oracle] The 7-position `HashSet` order** (§6.8.3b) — probe with wires at (x, y, z) covering hash-collision cases (positions where two of the 7 land in the same bucket) and negative coordinates; compare the logged order with the formula.
2. **[oracle] Conductor overrides** — the list of blocks with `cond:never` / `always` comes from `block-table.md`; verify against `BlockStateBase#isRedstoneConductor` for every state of a few families (leaves, glass, pistons, TNT, sculk sensor, shulker box, soul sand).
3. **[oracle] `getSignal` of wire towards the asker** (§6.6.4) after a neighbour change that has not yet produced a shape update — probe with a piston that moves a block next to a wire and log `getSignal` between the two writes.
4. **[oracle] Experimental evaluator** (§7) — all of it is derived from reading only; probe with the flag enabled: a 3-wire line + torch; compare the order of `neighborChanged` calls (Phase C) with the rules.
5. (implementer) Sound / particle / client-only branches (wire particles, dust colours) are out of scope; the dust colour table is `COLORS[p]` computed from `p/15` by the formulas of the client spec (not repeated here).
6. (unresolved, other milestone) `RailState` rail-shape solving is not part of the redstone reading (see components §9).

## Parity status

Not yet run.
