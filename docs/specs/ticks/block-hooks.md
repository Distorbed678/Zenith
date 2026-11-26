# Ticks: the block, fluid and block-entity hook interface (26.3)

- Verified by suites: `server-tick`, `block-updates`
- Sources: local reference reading of the 26.3 client/server code (2026-11-26, reader session): the block behaviour base
  (every overridable hook and its default), the block-state wrappers that the engine calls, the entity-block interface, the
  fluid and fluid-state hooks, the chunk code that drives block-entity tickers, and every framework call site of each hook.
  Everything is in our own words.
- Related specs: [`set-block-flags-and-sequence.md`](set-block-flags-and-sequence.md), [`neighbor-and-shape-updates.md`](neighbor-and-shape-updates.md),
  [`scheduled-ticks-and-block-events.md`](scheduled-ticks-and-block-events.md), [`server-tick.md`](server-tick.md);
  [`../blocks/state-facts.md`](../blocks/state-facts.md) (state predicates and properties read through the state wrappers),
  [`../blocks/block-table.md`](../blocks/block-table.md) (block → behaviour class), [`../blocks/shapes.md`](../blocks/shapes.md),
  [`../blocks/state-transforms.md`](../blocks/state-transforms.md).

## Summary

The framework never contains block-specific logic: it calls *hooks* on the block (or fluid, or block entity) of a state. A per-block
implementation fills hooks; every hook has a default. This file lists every hook the tick framework and the world code invoke, **when** it
is called (relative to the other steps of a block change or tick), **with which arguments**, what it **may** do, and its default.
Block-specific rules (survival checks, plants, gravity, redstone components, fluids) are specified by other readers against this interface.

## Inputs and state

- A *state* is an interned block state (identity comparison is meaningful: "same state" means "same object"). Hooks are called through
  the state wrappers; the receiving object is the state's block (`state.getBlock()`).
- `level` is the `ServerLevel` for server hooks, a `Level` (server or client) for hooks that exist on both sides, a `LevelReader` /
  `BlockGetter` for pure queries, and a `LevelAccessor` for hooks that may run during generation.
- `random` is always a `RandomSource`; for server tick hooks it is the **level's** `random` (not a per-position random).

## Behavior (ordered rules)

### 1. The hooks of the block-change pipeline (called by `setBlock` and the updaters)

| # | hook (receiver = the state noted) | arguments | called when | may do | default |
|---|---|---|---|---|---|
| 1 | `shouldChangedStateKeepBlockEntity` (on the **new** state) | `(oldState)` → bool | chunk step 3.7, only if the block type changed and the old state has a block entity | — | false (the block entity is removed) |
| 2 | block entity `preRemoveSideEffects` | `(pos, oldState)` | chunk step 3.7, after the check above, server only, unless flag 256 | drop contents; may write blocks | none |
| 3 | `affectNeighborsAfterRemoval` (on the **old** state) | `(serverLevel, pos, movedByPiston)` | chunk step 3.8: block type changed *or* new block is a rail, **and** (flag 1 set or flag 64 set); also `updateNeighboursOnBlockSet` and the piston code | notify neighbours (`updateNeighborsAt`), update signals; may replace the block (then the write is aborted, set-block §3.9) | none |
| 4 | `onPlace` (on the **new** state) | `(level, pos, oldState, movedByPiston)` | chunk step 3.10, server only, unless flag 512 | schedule ticks (`scheduleTick`), write blocks, queue block events, notify neighbours | none |
| 5 | `newBlockEntity` (`EntityBlock`) | `(pos, state)` → block entity or null | chunk step 3.11 when the state has a block entity and none exists | — | — |
| 6 | `getAnalogOutputSignal` / `hasAnalogOutputSignal` | `(state, level, pos, direction)` / `(state)` | after the write if flag 1: `hasAnalogOutputSignal(new state)` decides the comparator notification; comparators read `getAnalogOutputSignal` later | pure | 0 / false |
| 7 | `neighborChanged` | `(state, level, pos, sourceBlock, orientation, movedByPiston)` | a Multi / Simple / Full neighbour update of [`neighbor-and-shape-updates.md`](neighbor-and-shape-updates.md) §2 | write blocks, schedule ticks, queue events, notify neighbours | none |
| 8 | `updateShape` | `(state, level, scheduledTickAccess, pos, directionToNeighbour, neighbourPos, neighbourState, random)` → new state | a shape update (§3), `Block.updateFromNeighbourShapes`, `updateShapeAtEdge`, legacy upgrade | **return a state** and/or *schedule ticks*; must not write blocks | returns `state` unchanged |
| 9 | `updateIndirectNeighbourShapes` | `(state, level, pos, flags, limit)` | `setBlock` step 2.7 for the old state, then the new state; piston code | start shape updates / writes two steps away (redstone wire, rails) | none |
| 10 | `getPostProcessPos` (property set on the block) | `(state, level, pos)` → pos or null | generation region `setBlock` when flag 16 is clear | pure | null (no marking) |
| 11 | `getFluidState` | `(state)` | counters (set-block §3.2), `removeBlock` / `destroyBlock` (`createLegacyBlock`), random-tick fluid hook, scheduled fluid ticks | pure | the empty fluid state, or a source for waterlogged states (per block) |
| 12 | `canSurvive` | `(state, level, pos)` → bool | **not called by the framework itself**: blocks call it from `updateShape`, `tick`, `neighborChanged`; `shouldSnow` calls it for snow; item placement calls it | pure | true |

### 2. The hooks of the tick pipeline

| # | hook | arguments | called when | notes |
|---|---|---|---|---|
| 13 | `tick` (block) | `(state, serverLevel, pos, random)` | scheduled block tick that is collected for this level tick, if the block at `pos` is still of the scheduled block ([`scheduled-ticks-and-block-events.md`](scheduled-ticks-and-block-events.md) §3.3); also called directly for liquid blocks during chunk post-processing | `random` = level random |
| 14 | `tick` (fluid) | `(serverLevel, pos, blockState, fluidState)` — the fluid-state wrapper is `fluidState.tick(level, pos, blockState)` | scheduled fluid tick, if the fluid at `pos` is of that fluid; chunk post-processing for every non-empty fluid state | |
| 15 | `randomTick` (block) | `(state, serverLevel, pos, random)` | random tick selection ([`server-tick.md`](server-tick.md) §10) when `state.isRandomlyTicking()` | `random` = level random |
| 16 | `randomTick` (fluid) | `(serverLevel, pos, random)` — via `fluidState.randomTick` | same loop, after the block hook, using the **pre-tick** state's fluid | |
| 17 | `isRandomlyTicking` (block state / fluid state) | cached at state initialisation | counted by the chunk section's counters on every write; a block is randomly ticking iff its properties request random ticks | the lava fluid ticks randomly, water does not |
| 18 | `triggerEvent` | `(state, level, pos, paramA, paramB)` → bool | block-event queue ([`scheduled-ticks-and-block-events.md`](scheduled-ticks-and-block-events.md) §6); `BaseEntityBlock` forwards to the block entity | return true to broadcast the event to clients |
| 19 | `handlePrecipitation` | `(state, level, pos, precipitation)` | the precipitation random tick of [`server-tick.md`](server-tick.md) §11, on the block *below* the surface | cauldrons fill |
| 20 | `getTicker` (block-entity block) | `(level, state, blockEntityType)` → ticker or null | when a block entity is registered or its state changes (chunk code); decides whether a ticker wrapper exists | ticker function signature `(level, pos, state, blockEntity)` |
| 21 | block-entity ticker | `(level, pos, state, blockEntity)` | block-entity step of [`server-tick.md`](server-tick.md) §8 | |
| 22 | block entity `setChanged` | `()` | called by block-entity code after data changes | marks the chunk unsaved; if the block is not air runs `updateNeighbourForOutputSignal(pos, block)` |
| 23 | block entity `triggerEvent` | `(paramA, paramB)` → bool | from hook 18 | |

### 3. Entity-driven and interaction hooks (called by code of other milestones; listed so the signatures are fixed)

| hook | arguments | called by (framework side) |
|---|---|---|
| `entityInside` | `(state, level, pos, entity, insideBlockEffectApplier, isPrecise)` | entity block-overlap check each entity tick (positions swept this tick); order of blocks is the entity milestone's |
| `stepOn` | `(level, pos, onState, entity)` | entity movement |
| `fallOn` / `bounceOn` | `(level, state, pos, entity, fallDistance)` | entity landing |
| `playerWillDestroy` | `(level, pos, state, player)` → state | player mining, **before** the removal write |
| `destroy` | `(levelAccessor, pos, state)` | player mining, after the removal write |
| `playerDestroy` | `(level, player, pos, state, blockEntity, tool)` | player mining, after `destroy` (drops, exhaustion) |
| `spawnAfterBreak` | `(state, serverLevel, pos, tool, dropExperience)` | `Block.dropResources` family |
| `setPlacedBy` | `(level, pos, state, placer, itemStack)` | item placement, after the placement write |
| `getStateForPlacement` | `(placeContext)` → state or null | item placement |
| `canBeReplaced` | `(state, placeContext)` / `(state, fluid)` | placement and fluid spread |
| `useItemOn` / `useWithoutItem` | `(item, state, level, pos, player, hand, hit)` / `(state, level, pos, player, hit)` → `InteractionResult` | right-click on a block |
| `attack` | `(state, level, pos, player)` | left-click start |
| `onExplosionHit` | `(state, serverLevel, pos, explosion, onHit)` | explosion block-damage pass (default: remove with flags 3 + `wasExploded` unless the explosion only triggers blocks) |
| `onProjectileHit` | `(level, state, blockHit, projectile)` | projectile collision |
| `getSignal` / `getDirectSignal` / `ownSignal` / `isSignalSource` / `shouldRedstoneWireConnectTo` | `(state, level, pos, direction)` … | redstone queries; defaults: `getSignal` → `ownSignal` (0), `getDirectSignal` 0, `isSignalSource` false, wire connects iff signal source and direction non-null |
| `isRedstoneConductor`, `isSuffocating`, `isFaceSturdy`, `getShape` / `getCollisionShape` / `getOcclusionShape`, `getLightEmission`, `getLightDampening`, `propagatesSkylightDown`, `isPathfindable` … | pure state queries | read everywhere; specified in [`../blocks/state-facts.md`](../blocks/state-facts.md) and [`../blocks/shapes.md`](../blocks/shapes.md) |
| `rotate` / `mirror` | `(state, rotation)` / `(state, mirror)` | structure placement ([`../blocks/state-transforms.md`](../blocks/state-transforms.md)) |

### 4. How hooks are expected to interact with the framework (conventions that every per-block spec relies on)

1. **A hook sees the world as it is *now*.** Neighbour updates read the notified state when they run; shape updates receive the neighbour's state *as captured when the
   update was queued* (possibly stale) — a block that cares must re-read.
2. **Hooks that change blocks run `setBlock` re-entrantly.** The re-entrant write completes its own full sequence (set-block §2) before the hook continues; the outer write
   notices the change at its step 2.3 and skips its own later steps.
3. **Scheduling.** A block that wants to act later calls `scheduleTick(pos, block, delay[, priority])` from `onPlace`, `updateShape`, `neighborChanged` or `tick`; the
   one-tick-per-`(block, pos)` rule applies, so blocks usually test `hasScheduledTick` / `willTickThisTick` first.
4. **A block that must be removed when its support disappears** does it either inside `updateShape` (return air → `destroyBlock` with drops, notifies neighbours) or by scheduling a
   tick from `updateShape` and removing itself in `tick`. Both patterns are visible in tick-by-tick parity.
5. **Generation:** during world generation only `getPostProcessPos`, `updateShape` (through `Block.updateFromNeighbourShapes` and structure edge updates), `tick` / fluid `tick`
   (through post-processing) and block-entity creation run. `onPlace`, `affectNeighborsAfterRemoval` and `neighborChanged` **never** run for generation writes.
6. **Exceptions inside a hook** abort the server with a crash report (not reproduced).

## Edge cases and known quirks

1. `canSurvive` has no framework call site: a block whose survival is not re-checked by its own `updateShape` / `neighborChanged` / `tick` survives indefinitely.
2. The random-tick fluid hook uses the fluid state of the block read *before* the block's own random tick ([`server-tick.md`](server-tick.md) §10.2d).
3. `getTicker` is evaluated whenever the block entity registers or its block state changes — a state change from "no ticker" to "ticker" creates a new wrapper at the **end** of the ticker order.
4. `shouldChangedStateKeepBlockEntity` is asked of the **new** state with the old one as argument; most blocks inherit the default and drop the block entity on any block-type change.
5. The `random` passed to shape updates during generation is the generation region's positional random, not the level random.

## Constants and IDs

Hook default table above; `Block.UPDATE_LIMIT` 512; pipeline step numbers refer to [`set-block-flags-and-sequence.md`](set-block-flags-and-sequence.md) §2–§3.

## Oracle probe points

- Wrap the public methods of `BlockBehaviour$BlockStateBase` (the engine always calls through them): `tick(ServerLevel, BlockPos, RandomSource)`, `randomTick(...)`, `onPlace(Level, BlockPos, BlockState, boolean)`,
  `affectNeighborsAfterRemoval(ServerLevel, BlockPos, boolean)`, `handleNeighborChanged(Level, BlockPos, Block, Orientation, boolean)`, `updateShape(...)`, `updateNeighbourShapes(...)`,
  `updateIndirectNeighbourShapes(...)`, `triggerEvent(Level, BlockPos, int, int)`, `entityInside(...)`, `canSurvive(LevelReader, BlockPos)`, `getTicker(Level, BlockEntityType)`,
  `shouldChangedStateKeepBlockEntity(BlockState)`; `FluidState#tick`, `#randomTick`, `#createLegacyBlock`.
- A per-call log of `(tick, sequence, hook, pos, state ids, args)` is the dump format for the `block-updates` suite; the Rust side logs the same tuples.

## Open questions

1. **[oracle] Per-block overrides.** The set of blocks overriding each hook is the per-block readers' job; for completeness probe `Class#getDeclaredMethods` of each registered block class for the hook names above (the
   `block-table.md` class list gives the classes) and diff against their specs.
2. **[oracle] `entityInside` order and precision flag** belong to the entity milestone (the framework does not call it from `setBlock`).
3. (implementer) Keep hook arguments in the exact order above; the parity logger relies on it.

## Parity status

Not yet run.
