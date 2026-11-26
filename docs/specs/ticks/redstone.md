# Redstone: index, shared conventions and cross-component tables (26.3)

- Verified by suites: `redstone`
- Sources: local reference reading of the 26.3 server/client code (2026-11-26, reader session), see the three detail files; everything is in our own words.
- Detail specs (split because of length): [`redstone-power.md`](redstone-power.md) (power model, level helpers, signal table, redstone wire, experimental evaluator),
  [`redstone-components.md`](redstone-components.md) (torches, repeaters, comparators, observers, switches, plates, tripwire, sensors, sculk sensors, every consumer, rails as consumers),
  [`pistons.md`](pistons.md) (pistons, sticky pistons, push resolver, moving-piston block entity).
- Framework specs this relies on: [`server-tick.md`](server-tick.md), [`scheduled-ticks-and-block-events.md`](scheduled-ticks-and-block-events.md), [`neighbor-and-shape-updates.md`](neighbor-and-shape-updates.md),
  [`set-block-flags-and-sequence.md`](set-block-flags-and-sequence.md), [`block-hooks.md`](block-hooks.md), [`collection-orders.md`](collection-orders.md), [`../java/hash-collections.md`](../java/hash-collections.md),
  [`../blocks/state-facts.md`](../blocks/state-facts.md), [`../blocks/block-table.md`](../blocks/block-table.md), [`../blocks/shapes.md`](../blocks/shapes.md).

## Summary

Redstone behaviour in 26.3 is the sum of (a) the pull-style signal model, (b) per-component reactions to neighbour / shape updates, scheduled ticks and block events, and (c) the exact order in which
the framework delivers those updates. Parity suites compare whole contraptions tick by tick, so the framework orders matter as much as the per-block rules. This file lists what is shared: the
write flags each component uses, the places where **randomness** or **hash order** leaks in, the order of one level tick as seen by redstone, and the consolidated oracle / open-question lists.

## Inputs and state

None of its own. See the detail files.

## Behavior (ordered rules)

### 1. Where redstone sits in a level tick (from `server-tick.md` §5, restated for contraption timing)

For each dimension, per tick: (6) block scheduled ticks (all collected first, then executed in collection order; ticks scheduled meanwhile run next tick), then fluid ticks; (8) chunk source tick (includes
random ticks); (9) **block events** (pistons, note blocks, bells, chests …), loop until the queue is empty, including events queued while it runs; (10) `isHandlingTick` becomes false; (12b) entities (this is when
`entityInside` hooks of plates, buttons, tripwires, detector rails, hoppers fire — entity milestone); (12c) block entities (moving pistons, comparators have none that tick, sculk sensors, daylight detectors, hoppers, crafters, bells …).
Consequences used throughout: a component that reacts through a scheduled tick sees its change in the **next** level tick at the earliest (delay 0 → next tick); a piston reacts at the **end** of the tick in which it was notified; entity-driven triggers act after scheduled ticks and events of the same tick.

### 2. Write-flag cheat sheet (who writes with which flags)

| flags | meaning | users |
|---|---|---|
| 2 | send to clients; **no** neighbour notification (but shape updates and `onPlace` run) | repeater / comparator state changes, observer, lamp, hopper, dispenser, crafter, doors, trapdoors, fence gates, plates, wire power |
| 3 (`setBlockAndUpdate`) | clients + neighbours (old block as source) + shape updates | torch, lever, button, note block, target block, bell, lectern, tripwire, daylight detector, sculk sensor phases, lightning rod, copper bulb, detector rail, powered rail |
| 18 (16 + 2) | no shape updates, no neighbour notification | observer / target / sculk `onPlace` repairs, piston destruction of `POPPED` blocks |
| 67 (64 + 2 + 1) | piston-moved placement | piston base after extension; final placement of moved blocks |
| 82 (64 + 16 + 2) | piston vacated cell | pistons |
| 260 (256 + 4) | no block-entity side effects, invisible | disarming a tripwire with shears |
| 276 (256 + 16 + 4) | piston arm removal, base → moving piston | pistons |
| 324 (256 + 64 + 4) | moving-piston writes | pistons |
| 340 (256 + 64 + 16 + 4) | moved block that cannot stay | moving block entity |
| 128 (experimental wire only) | skip shape updates on wire blocks | experimental evaluator |

### 3. Where level randomness is consumed (level `random`, server)

One float each, in the order the code reaches them: piston extend (`nextFloat`) and retract (`nextFloat`) sounds; door, trapdoor, fence gate open/close sounds; tripwire-hook **detach** sound; sculk-sensor click and stop-click sounds;
big-dripleaf tilt sounds; dispenser / dropper slot choice (`nextInt(k)` per non-empty slot, `k = 1, 2, …`); and (**only with the `redstone_experiments` flag**) one `nextInt(48)` per `initialOrientation` call, drawn at every call site that
builds an orientation before notifying neighbours. Other level-random users of the tick (random ticks etc.) share the same stream, so these draws shift them (sound `playSound` seeds come from a **different** generator and do not).

### 4. Hash-order dependencies

1. The wire's 7-position `HashSet` (power §6.8.3b). 2. The piston's `deleteAfterMove` `HashMap` (pistons §5.8). Everything else (chained updater, block events, tick queue) has orders defined in the framework specs.

### 5. Experimental-orientation call sites (only with `redstone_experiments`)

`ServerLevel.updateNeighborsAt(pos, block)` (2-argument form), diode `updateNeighborsInFront`, observer `updateNeighborsInFront`, lever / button `updateNeighbours`, redstone torch `notifyNeighbors`, lightning rod `updateNeighbours`,
tripwire hook `notify`, lectern `updateBelow`, trapped chest open-count change, moving-piston final placement (`initialOrientation(push dir)`), piston `moveBlocks`, wire player-toggle, wire evaluator (random orientation when the caller passed none).
Every one of them is `null` and draws nothing in vanilla.

## Edge cases and known quirks

See the detail files; the most parity-relevant ones: quasi-connectivity of pistons and dispensers (power §4), repeater pulse stretching and `willTickThisTick` lost pulses (components §2.7, Edge 2), observer 2+2 pulse (components §4),
piston event timing and the three-tick moving entity (pistons §6, §9), wire neighbour-update order from the `HashSet` (power §6.8), torch burnout list (components §1.4).

## Constants and IDs

See the detail files (each ends with its constants table). Global: signal 0–15; priority values −3 … 3; scheduled tick limit 65,536 per type per tick; chained updater limit 1,000,000; shape update depth 512.

## Oracle probe points

Consolidated lists are in each detail file. A single **tick-by-tick dump** format for the `redstone` suite is recommended: per game tick, the ordered list of
`(phase, hook, position, block, state-before, state-after, args)` for `tick`, `neighborChanged` (via `BlockStateBase#handleNeighborChanged`), `updateShape`, `onPlace`, `affectNeighborsAfterRemoval`, `triggerEvent`,
block-entity ticks, and every `Level#setBlock(BlockPos, BlockState, int, int)` (flags included); plus the scheduled-tick queue (`LevelChunkTicks` contents) at the end of the tick.

## Open questions

Consolidated (details and probes in the files):
1. **[oracle]** wire `HashSet` order; **[oracle]** piston `HashMap` order; **[oracle]** same-tick first tick of the moving-piston entity; **[oracle]** duplicates in `toPush`; **[oracle]** `willTickThisTick` lost pulses; **[oracle]** repeater prioritisation; **[oracle]** observer placement facing;
   **[oracle]** sculk listener order / selector / occlusion; **[oracle]** daylight `SUN_ANGLE`; **[oracle]** chest opener recheck; **[oracle]** dispenser slot draws; **[oracle]** headless pistons; **[oracle]** event id 2 conditions; **[oracle]** experimental evaluator; **[oracle]** conductor overrides; **[oracle]** wire `getSignal` between writes; **[oracle]** entity pushing by pistons.
2. **(unresolved)** `RailState` (rail shape solver, connection order) — a separate reading is needed for plain rails and for the `updatePowerToConnected` order of detector rails.
3. **(unresolved, other milestones)** entity-side triggers (`entityInside` order), item dispense behaviours, hopper transfer, crafter recipes, command-block execution, big-dripleaf tilt timings, bell raider resonance, lightning-bolt rod selection.

## Parity status

Not yet run.
