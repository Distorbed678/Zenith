# Redstone: components — torches, diodes, comparators, observers, switches, sensors and consumers (26.3)

- Verified by suites: `redstone`
- Sources: local reference reading of the 26.3 server/client code (2026-11-26, reader session): the diode base, repeater, comparator (+ its block entity),
  redstone torches (standing, wall), observer, lever, buttons, pressure plates (basic, weighted), tripwire and tripwire hook, daylight detector, target block,
  lightning rod, sculk sensor / calibrated sculk sensor (+ block entities and the vibration system), lectern, trapped chest (+ opener counter), redstone lamp, note block,
  dispenser, dropper, hopper, crafter (+ block entity), doors, trapdoors, fence gates, TNT, bell, copper bulb, skulls, shelf, command block, structure block, big dripleaf,
  powered / detector rails, the block-set types and every analog-output override. Everything is in our own words.
- Related specs: [`redstone.md`](redstone.md), [`redstone-power.md`](redstone-power.md) (helpers §2, signal table §5), [`pistons.md`](pistons.md);
  [`neighbor-and-shape-updates.md`](neighbor-and-shape-updates.md), [`set-block-flags-and-sequence.md`](set-block-flags-and-sequence.md),
  [`scheduled-ticks-and-block-events.md`](scheduled-ticks-and-block-events.md) (priorities, one tick per (block, pos), `willTickThisTick`, block events),
  [`block-hooks.md`](block-hooks.md), [`server-tick.md`](server-tick.md) §5 / §8 (order inside a level tick, block-entity tickers),
  [`../blocks/shapes.md`](../blocks/shapes.md) (RIGID / CENTER / FULL sturdiness), [`../blocks/state-facts.md`](../blocks/state-facts.md).

## Summary

Every component below is described by: its state properties, the hook that reacts (neighbour update, shape update, scheduled tick, block event, entity-inside),
the scheduled delay and **priority**, the write flags it uses, and the neighbour notifications it issues. A reminder that matters everywhere
(set-block §3.10): **`onPlace` runs on every write that does not carry flag 512, also when only a property changed** — several components (diodes, torches)
rely on it to notify neighbours.

## Inputs and state

- Tick priorities: `EXTREMELY_HIGH` −3, `VERY_HIGH` −2, `HIGH` −1, `NORMAL` 0 (default). Scheduling a tick for a `(block, pos)` that already has one is **ignored** (scheduled-ticks §1.3).
- `willTickThisTick(pos, block)` = "that tick is already collected for the current run" (scheduled-ticks §3.6): true only while ticks of the current level tick are executing.
- Block-set types (per door / trapdoor / button / plate family): `canOpenByHand`, `canButtonBeActivatedByArrows`, `pressurePlateSensitivity`:

| set type | by hand | button by arrows | plate sensitivity |
|---|---|---|---|
| iron | no | no | everything |
| gold | no | no | everything |
| copper | yes | no | everything |
| stone, polished_blackstone | yes | no | **mobs** (living entities only) |
| all wood sets, cherry, bamboo, crimson, warped (+ the new `poplar`) | yes | yes | everything |

  Stone buttons and the polished-blackstone button both use the **stone** set; the polished-blackstone *plate* uses the polished-blackstone set (same data as stone). Button hold times: stone family and polished blackstone **20**, every wood-like family **30**
  (crimson, warped and bamboo included). Weighted plates: light (gold set) max weight **15**, heavy (iron set) **150**.

### 1. Redstone torch (standing and wall)

State: `LIT` (default true); wall torch also `FACING` (horizontal; points **away** from the wall). Constants: burnout window **60** ticks, burnout threshold **8**, restart delay **160**, normal delay **2**.
Support: standing = the block below supports the **centre** of its top (shapes.md `canSupportCenter`); wall = the attached block's face towards the torch is sturdy (FULL). Standing `updateShape` for `DOWN` and wall `updateShape` for the attached side → air when no longer supported.
**1.1 Input** `hasNeighborSignal`: standing `hasSignal(pos+DOWN, DOWN)`; wall `hasSignal(pos + FACING.opposite, FACING.opposite)`.
**1.2 `neighborChanged`:** if `LIT == hasNeighborSignal` (i.e. lit with input, or unlit without) **and** `!willTickThisTick(pos, block)`: `scheduleTick(pos, block, 2)` (NORMAL). (Duplicates are dropped by the one-tick rule; a pending 160-tick burnout tick therefore blocks this.)
**1.3 `onPlace` and `affectNeighborsAfterRemoval`** (the latter skipped when moved by a piston): `notifyNeighbors`: for `d` in `Direction.values()`: `updateNeighborsAt(pos + d, torchBlock, orientation withFront d)` (orientation `null` in vanilla; with the experiment `initialOrientation(front = wall: FACING.opposite / standing: none, up = UP)` drawn once). `onPlace` fires on **every** write, so each LIT toggle notifies the neighbours of the six neighbours first and (flag 1 of the toggle write) the neighbours of the torch itself afterwards.
**1.4 `tick`** (level random unused):
1. `signal = hasNeighborSignal`. Prune the per-level **recent-toggle list** (kept in memory per level, chronological, entries `(pos, gameTime)`; **not saved**): while the list is non-empty and `gameTime − first.when > 60`: remove the first.
2. If `LIT`: if `signal`: `setBlockAndUpdate(pos, LIT = false)` (flags 3); then `toggledTooFrequently(add = true)`: append `(pos, gameTime)`; count entries with this `pos`; true if the count is **≥ 8**. If true: level event 1502 (burnout sound / smoke) and `scheduleTick(pos, <block currently at pos>, 160)` (NORMAL).
3. If not `LIT`: if `!signal` **and** `!toggledTooFrequently(add = false)` (count entries for `pos` ≥ 8 ⇒ true): `setBlockAndUpdate(pos, LIT = true)` (no entry recorded for turning on).
**1.5 Signal:** table in power §5 (own 15 when lit; weak everywhere except towards the support; strong only to the block above, `d = DOWN`).
**1.6 Timeline:** input changes at tick `t` → tick at `t+2`; output changes at `t+2`; neighbours see it in the same tick (flags 3). Burnout: the 8th turn-off within 60 ticks is followed by no relight until at least 160 ticks later and then only if `< 8` of that position's toggles remain in the 60-tick window (they will have expired).

### 2. Diodes — shared behaviour (repeater and comparator)

State: `FACING` (horizontal; **the input side**: the block is placed facing the player's opposite), `POWERED`; repeater also `DELAY` 1–4 and `LOCKED`; comparator also `MODE`. Support: the block below must have a **RIGID** sturdy UP face (shapes.md RIGID: the 2-pixel border ring); `updateShape(DOWN)` → air when lost; `neighborChanged` removes the diode (with drops, for the comparator its block entity too) and notifies `updateNeighborsAt(pos + d, block)` for **all six** `d` in `Direction.values()` when it cannot survive.
**2.1 Input signal** `getInputSignal(pos, state)`: `t = pos + FACING`; `i = getSignal(t, FACING)` (power §2.1, so conductors pass strong signals); if `i ≥ 15` return it; else `max(i, POWER of the block at t if it is redstone wire else 0)` (a wire directly in front always counts with its full POWER, whatever its shape).
**2.2 Side inputs** `getAlternateSignal`: for `cw = FACING.clockWise`, `ccw = FACING.counterClockWise`: `max(controlInput(pos+cw, cw, sideOnlyDiodes), controlInput(pos+ccw, ccw, sideOnlyDiodes))` where `controlInput` is power §2.8 and `sideOnlyDiodes` is **true for the repeater, false for the comparator**.
**2.3 Output** (`ownSignal`): `POWERED ? outputSignal : 0`, repeater 15, comparator = its block entity's stored output; the signal table of power §5 applies (only towards the output side).
**2.4 `neighborChanged`** (only if the block at `pos` is still this diode): if it can survive → `checkTickOnNeighbor`; else remove as in the support rule above.
**2.5 Base `checkTickOnNeighbor`** (repeater): if **not locked** (`isLocked` recomputed live): `on = POWERED`, `want = shouldTurnOn` (input > 0); if `on ≠ want` and `!willTickThisTick`: priority `EXTREMELY_HIGH` if `shouldPrioritize`, else `VERY_HIGH` if `on`, else `HIGH`; `scheduleTick(pos, block, delay, priority)`.
**2.6 `shouldPrioritize`:** let `o = pos + FACING.opposite` (the block the output feeds); true iff `o` is a diode **and** that diode's `FACING ≠ FACING.opposite` (of this block). (A repeater feeding the side of another diode, or head-on into a diode that faces away, is prioritised.)
**2.7 Base `tick`:** if not locked: `on = POWERED`, `want = shouldTurnOn`. If `on && !want`: `setBlock(pos, POWERED=false, 2)`. Else if `!on`: `setBlock(pos, POWERED=true, 2)`; then, **if `!want`**: `scheduleTick(pos, block, delay, VERY_HIGH)` (the output pulse is stretched to a full delay).
**2.8 Notification of the output:** `updateNeighborsInFront(pos, state)`: `front = pos + FACING.opposite`; (1) a Simple neighbour update `neighborChanged(front, thisBlock, orientation)` (notifies only the block in front), then (2) `updateNeighborsAtExceptFromFacing(front, thisBlock, skip = FACING, orientation)` (the other five neighbours of `front`; `FACING` direction from `front` points back at the diode). Called from `onPlace` (every write of the diode without flag 512), from `affectNeighborsAfterRemoval` (unless moved by a piston), and by the comparator's own refresh. Orientation: `initialOrientation(front = FACING.opposite, up = UP)` (`null` in vanilla).
**2.9 `setPlacedBy`** (placement by an item): if `shouldTurnOn`: `scheduleTick(pos, block, 1)` (NORMAL).
**2.10 Repeater specifics**
- `getDelay = DELAY × 2` game ticks (2, 4, 6, 8).
- **Locked:** `isLocked` = `getAlternateSignal > 0` computed **live** on every use; the `LOCKED` property is merely a mirrored cache: it is updated only in `updateShape` for neighbours **not on the facing axis** (sides and up/down; `DOWN` first checks support) as `LOCKED = isLocked(...)` on the server, and at placement. A locked repeater ignores `tick` and `neighborChanged` entirely (no scheduling); when the lock ends nothing is re-scheduled — the next neighbour update of the repeater re-evaluates (the side diode that stopped signalling notifies it in the same update batch).
- Interaction (empty-hand use): `setBlockAndUpdate(pos, DELAY cycled 1→2→3→4→1)` (flags 3; `onPlace` and flag 1 both notify).
- Placement: `LOCKED` computed immediately.
- Wire connection: only along the facing axis.
**2.11 Worked timeline** (delay setting 1 = 2 ticks, unlocked, input rises at tick `t`): `neighborChanged` at `t` → tick at `t+2`, priority `HIGH` → at `t+2` the repeater turns on (flags 2, `onPlace` ⇒ notify front) and, if the input has already fallen, schedules `t+4` (`VERY_HIGH`) → at `t+4` it turns off. Input falls at `u` while on: tick at `u+2` (`VERY_HIGH`), turns off at `u+2`.

### 3. Comparator

State: `FACING` (input side), `POWERED`, `MODE` (`compare` default, `subtract`). Block entity `ComparatorBlockEntity` with one i32 `OutputSignal` (default 0, NBT key `OutputSignal`); created in the set-block entity step (set-block §3.11), i.e. *after* `onPlace`. Delay 2 (fixed).
**3.1 Input** `getInputSignal`: `r = diode input (2.1)`; `t = pos + FACING`, `ts = state(t)`:
1. if `ts` has an analog output signal: `r = ts.getAnalogOutputSignal(level, t, FACING.opposite)` (**replaces** `r`, even a smaller one);
2. else if `r < 15` and `ts` is a redstone conductor: `t2 = t + FACING`, `s2 = state(t2)`; `frame` = the item frame in the unit box of `t2` whose facing direction **equals `FACING`**, only if **exactly one** such frame exists; candidate `= max(frame ? frame.analogOutput : −∞, s2.hasAnalogOutput ? s2.getAnalogOutputSignal(level, t2, FACING.opposite) : −∞)`; if the candidate is not −∞ it **replaces** `r`.
   Item-frame analog output = `0` if the frame holds no item, else `(rotation % 8) + 1`.
**3.2 Output** `calculateOutputSignal`: `in = getInputSignal`; `0` if `in = 0`; `side = getAlternateSignal`; `0` if `side > in`; else `in − side` in `subtract` mode, `in` in `compare` mode.
**3.3 `shouldTurnOn`:** `in = getInputSignal`; false if `in = 0`; else `in > side`, or (`in = side` **and** mode = compare).
**3.4 `checkTickOnNeighbor`** (overrides the diode one; **ignores locking** — the comparator has none): if `!willTickThisTick`: `out = calculateOutputSignal`, `old = block entity OutputSignal` (0 if missing); if `out ≠ old` **or** `POWERED ≠ shouldTurnOn`: `scheduleTick(pos, block, 2, HIGH if shouldPrioritize else NORMAL)`.
**3.5 `tick` and the refresh** `refreshOutputState`:
1. `out = calculateOutputSignal`; `old` = entity output; store `out` in the entity (no `setChanged` side effects beyond the field write).
2. If `old ≠ out` **or** mode = compare: `src = shouldTurnOn`, `on = POWERED`; if `on && !src` → `setBlock(pos, POWERED=false, 2)`; else if `!on && src` → `setBlock(pos, POWERED=true, 2)` (each write triggers `onPlace` ⇒ §2.8); **then** `updateNeighborsInFront` once more (always, in this branch).
So in compare mode the front is re-notified on every comparator tick; in subtract mode only when the output value changed.
**3.6 Mode toggle by hand:** `cycle MODE`, click sound, `setBlock(pos, state, 2)` (⇒ `onPlace` ⇒ §2.8) and, if the block is still the comparator, `refreshOutputState` immediately with the new state.
**3.7 Notifications from containers:** `updateNeighbourForOutputSignal(pos, changedBlock)` (neighbour spec §2.6) reaches comparators at distance 1 and, through a conductor, at distance 2 as a Full neighbour update ⇒ §2.4.
**3.8 `triggerEvent`:** forwards to the block entity (no events are queued by vanilla code).
**3.9 Analog output of the readable blocks** (`hasAnalogOutputSignal` true — list identical to `state-facts.md` item 22; value of `getAnalogOutputSignal(...)`):

| block | value |
|---|---|
| any container-style block (barrel, furnace / smoker / blast furnace, brewing stand, decorated pot, dispenser, dropper, hopper, shulker boxes, chest family via its combined container) | **container fill formula** (below) of the block entity (chest: the double-chest combined container; ender chests have no analog output) |
| crafter | number of crafter slots that are non-empty **or disabled** (0–9) |
| beehive / bee nest | `HONEY_LEVEL` |
| cake | `(7 − bites) × 2`; candle cakes always 14 |
| layered cauldrons (water, powder snow) | `LEVEL`; lava cauldron 3 |
| composter | `LEVEL` |
| chiseled bookshelf | `lastInteractedSlot + 1` (0 on the client) |
| copper bulb | 15 if `LIT` else 0 |
| copper golem statue | `POSE.ordinal + 1` |
| creaking heart | 0 if uprooted; else the block entity's stored output (recomputed as `15 − floor(clamp(distance to the protector creaking, 0, 32)/32 × 15)` when it has a protector, 0 otherwise) |
| command block | its success count |
| end portal frame | 15 if it has an eye |
| jukebox | the `comparator_output` of the playing record's song (0 if none) |
| lectern | if it has a book: `floor(pageProgress × 14) + 1` with `pageProgress = page/(pageCount − 1)` (1 when pageCount ≤ 1); else 0 |
| respawn anchor | `floor(CHARGE / 4 × 15)` (CHARGE 0–4) |
| sculk sensors (both) | the last vibration frequency if the phase is **active**, else 0 |
| shelf | only when asked with direction = `FACING.opposite`: bit0 = slot 0 non-empty, bit1 = slot 1, bit2 = slot 2 (0 on the client / other directions) |
| detector rail | if POWERED: the success count of the first command-block minecart in its search box, else the container fill formula of the first container minecart; else 0 (§9.4) |
| hopper | container fill formula |

**Container fill formula** (`getRedstoneSignalFromContainer`; f32 arithmetic): `total = Σ over slots of count / maxStackSize(stack)` (each non-empty slot, `maxStackSize` of **that stack in this container**); `total /= containerSize`; result `lerpDiscrete(total, 0, 15) = 0 + floor(total × 14) + (total > 0 ? 1 : 0)`. So any item gives ≥ 1 and a completely full container gives 15.
**3.10 Timeline** (compare mode, input rises at `t`): tick scheduled for `t+2` (NORMAL; HIGH if prioritised), at `t+2`: output stored, turns on, notifies the front twice (once from `onPlace`, once explicitly).

### 4. Observer

State: `FACING` (the **observed** face; the output is at the back, `FACING.opposite`), `POWERED`.
**4.1 Trigger — `updateShape`:** called for every shape update of the neighbour in direction `d`. If `d = FACING` **and** the observer is not powered: on the server, if the block has no scheduled tick: `scheduleTick(pos, block, 2)` (NORMAL; goes through the scheduled-tick access of the caller; dropped in the generation region by its own rule). The state is returned unchanged. Because it hooks shape updates, the observer reacts to **any state change of the observed position that starts shape updates** (the written state must differ from the old one — `setBlockState` returns early for the identical state — and the write must not carry flag 16): block placement / removal, property changes with flags ≠ 16-bit (flags 2, 3, 67 …), piston moves; **not** writes made with flag 16 (e.g. 18, 816) and not block-entity-only changes.
**4.2 `tick`:** if `POWERED`: `setBlock(pos, POWERED=false, 2)`; else `setBlock(pos, POWERED=true, 2)` **then** `scheduleTick(pos, block, 2)`. In both cases afterwards `updateNeighborsInFront(pos, state)` (Simple update on the block behind + Multi update around it skipping the observer's own direction; orientation `initialOrientation(front = FACING.opposite, up = none)`).
**4.3 `onPlace`:** only if the old block differs: on the server, if `POWERED` and no tick is scheduled: `setBlock(pos, POWERED=false, 18)` then `updateNeighborsInFront` (this repairs observers copied / pushed in the powered state).
**4.4 `affectNeighborsAfterRemoval`:** if `POWERED` **and** a tick is scheduled: `updateNeighborsInFront(pos, state with POWERED=false)` (the output is withdrawn when the observer is removed mid-pulse).
**4.5 Signal:** power §5 (back only, weak = strong).
**4.6 Pulse timeline:** change seen at tick `t` ⇒ tick scheduled for `t+2` ⇒ on at `t+2` (+ second tick for `t+4`) ⇒ off at `t+4`: the output lasts **2 game ticks** and a change at the observed block while a tick is pending or while powered is ignored.
**4.7 Placement:** FACING = the nearest looking direction of the placer, then `opposite().opposite()` in the source (net: **the placer's look direction**) **[oracle]** — confirm with the placement probe.

### 5. Switches: lever and buttons

Both are face-attached: `FACE` ∈ {floor, wall, ceiling}, `FACING` (horizontal), `POWERED`. **Connected direction** `A`: floor → `UP`, ceiling → `DOWN`, wall → `FACING`; the supporting block is at `pos + A.opposite`. Support: `canAttach`: the block at `pos + A.opposite` has a sturdy face towards the switch (`isFaceSturdy(A)`, FULL). `updateShape` for the neighbour in direction `A.opposite` → air when unsupported.
**5.1 Lever** `pull`: `state = cycle POWERED`; `setBlockAndUpdate(pos, state)` (flags 3); then `updateNeighbours`: `front = A.opposite` (the supporting block's direction); orientation `initialOrientation(front, up = (front horizontal ? UP : FACING))` (null in vanilla); `updateNeighborsAt(pos, block)`; `updateNeighborsAt(pos + front, block)`. Sound / game event (`block_activate` when turned on, `block_deactivate` off). Explosions that can trigger blocks also pull it. `affectNeighborsAfterRemoval`: if powered and not moved by a piston: `updateNeighbours`.
**5.2 Button** `press`: `setBlockAndUpdate(pos, POWERED = true)` (flags 3); `updateNeighbours` (same as the lever); `scheduleTick(pos, block, holdTicks)` (20 stone-like, 30 wood-like); sound; game event. Pressing a powered button does nothing (returns "consume").
**5.3 Button `tick`:** if `POWERED`: `checkPressed`.
**5.4 `checkPressed`:** `arrow` = the first `AbstractArrow` entity (in whatever order `getEntitiesOfClass` returns — stream `findFirst`) whose bounding box intersects the button's current **shape bounds** moved to `pos`, only if the set type has `canButtonBeActivatedByArrows`; `want = arrow ≠ none`; `was = POWERED`. If `want ≠ was`: `setBlockAndUpdate(pos, POWERED = want)`, `updateNeighbours`, sound, game event. If `want`: `scheduleTick(pos, block, holdTicks)` again (a stuck arrow keeps the button pressed and re-checks every `holdTicks`).
**5.5 `entityInside`** (server, hook from the entity milestone): if the set type allows arrows and `!POWERED`: `checkPressed`.
**5.6** The pressed shape is lower (14 → thickness 1 vs 2): the **shape bounds** used in 5.4 depend on `POWERED` (shapes.md / shapes-catalogue).
**5.7 Timeline:** press at `t` ⇒ powered from `t` (flags 3) ⇒ unpowered at `t + hold` (tick) unless an arrow keeps it.

### 6. Pressure plates, tripwire

**6.1 Basic plates** (`POWERED`) and **weighted plates** (`POWER` 0–15). Support: below has a RIGID sturdy UP face **or** a CENTER-sturdy UP face. `updateShape(DOWN)` → air when lost. Detection box `TOUCH_AABB`: x, z ∈ [1/16, 15/16], y ∈ [0, 4/16] moved to `pos`.
- Count of entities: `getEntitiesOfClass(cls, box, selector)` with `cls` = living entities for the **mobs** sensitivity (stone, polished blackstone) else all entities; selector = not a spectator **and** `!isIgnoringBlockTriggers`.
- Basic: signal = 15 if any entity else 0. Weighted: `n = min(count, maxWeight)`; signal = 0 if `n = 0` else `ceil((float)n / maxWeight × 15.0f)` (f32; `ceil` as `Mth.ceil`).
- Hold time (`getPressedTime`): basic 20, weighted **10** ticks.
**6.2 `checkPressed(source, level, pos, state, oldSignal)`:** `s = getSignalStrength`; if `s ≠ oldSignal`: `setBlock(pos, newState(s), 2)` then `updateNeighbours` (= `updateNeighborsAt(pos, block)` then `updateNeighborsAt(pos + DOWN, block)`; plain overload: orientation computed via the 2-argument entry, i.e. `null`); the click sound / game event on 0→>0 and >0→0; if `s > 0`: `scheduleTick(pos, block, hold)`.
**6.3 Hooks:** `entityInside` (server): if the current signal is 0: `checkPressed(entity, …, oldSignal 0)`. `tick`: if the current signal > 0: `checkPressed(null, …, current)`. `affectNeighborsAfterRemoval`: if signal > 0 and not moved by a piston: `updateNeighbours`.
**6.4 Signals:** power §5 (weak everywhere, strong to the block **below** only).
**6.5 Timeline:** entity enters at `t` (during the entity phase of the tick) ⇒ on immediately; tick at `t + hold` re-counts; stays on while entities remain, re-scheduling every `hold` ticks.

**6.6 Tripwire** (`TripWireBlock`, paired with its hook block): state `POWERED`, `ATTACHED`, `DISARMED`, sides `NORTH/EAST/SOUTH/WEST` booleans (cosmetic, set from neighbours: a side is true iff the neighbour is a tripwire, or the paired hook **facing back** along that side). Shape depends on `ATTACHED` (attached: 16-wide column y 1..2.5 px; not attached: y 0..8 px); the detection box is the **bounds of that shape** moved to `pos`.
- `updateShape` for horizontal `d`: sets the side `d` from the neighbour; vertical neighbours: default.
- `onPlace` (old block differs): `updateSource(pos, state)`. `affectNeighborsAfterRemoval` (not moved by a piston): `updateSource(pos, state with POWERED = true)`.
- `playerWillDestroy` with shears in the main hand: `setBlock(pos, DISARMED = true, 260)` + game event `shear`.
- `entityInside` (server): if `!POWERED` and no scheduled tick: `checkPressedWith([entity])`.
- `tick`: if the block at `pos` is `POWERED`: `checkPressed` with all entities (`getEntities` in the shape bounds).
- `checkPressed(entities)`: `want` = some entity in the list has `!isIgnoringBlockTriggers`; if `want ≠ POWERED`: `setBlockAndUpdate(pos, POWERED = want)` then `updateSource(pos, newState)`; then `if want: scheduleTick(pos, block, 10) else if wasPowered: scheduleTick(pos, block, 1)`.
- `updateSource(pos, state)`: for `dir` in **SOUTH, WEST**: for `i = 1 … 41`: `p = pos + dir·i`, `s = state(p)`; if `s` is the paired hook: if its `FACING = dir.opposite` → `calculateState(p, s, isBeingDestroyed=false, canUpdate=true, wireSource = i, wireSourceState = state)`; **stop the scan**. Else if `s` is not this tripwire block: stop.
**6.7 Tripwire hook** (`FACING` horizontal: the direction **towards the wire**; `POWERED`, `ATTACHED`): support: the block at `pos + FACING.opposite` has a sturdy face towards `FACING`; `updateShape` (neighbour in `FACING.opposite`) → air when unsupported. Placement: first of the player's nearest horizontal looking directions `L` such that `FACING = L.opposite` survives.
- `setPlacedBy`: `calculateState(pos, state, false, false, wireSource −1, none)`. `tick`: `calculateState(pos, state, false, true, −1, none)`.
- **`calculateState(hookPos, hookState, isBeingDestroyed, canUpdate, wireSource, wireSourceState)`:**
  1. `dir = FACING`; `wasAttached`, `wasPowered` from `hookState`; `attached = !isBeingDestroyed`; `powered = false`; `receiverDist = 0`; array `wires[1..41]`.
  2. For `i = 1 … 41`: `p = hookPos + dir·i`; `s = state(p)`. If `s` is a hook block: if its `FACING = dir.opposite` then `receiverDist = i`; **break** (either way). Else if `s` is not a tripwire and `i ≠ wireSource`: `wires[i] = none`, `attached = false`. Else: if `i = wireSource` and `wireSourceState` non-null: `s = wireSourceState`; `armed = !s.DISARMED`; `powered |= armed && s.POWERED`; `wires[i] = s`; if `i = wireSource`: `scheduleTick(hookPos, hookBlock, 10)` and `attached &= armed`.
  3. `attached &= (receiverDist > 1)`; `powered &= attached`. `newState` = the hook block's default with `ATTACHED = attached`, `POWERED = powered`.
  4. If `receiverDist > 0`: `q = hookPos + dir·receiverDist`; `setBlockAndUpdate(q, newState with FACING = dir.opposite)`; `notify(q, dir.opposite)`; if the block at `hookPos` is no longer a hook: `onRemoved(newState, hookPos)` and **return**; `emitSound(q, attached, powered, wasAttached, wasPowered)`.
  5. `emitSound(hookPos, …)`.
  6. If `!isBeingDestroyed`: `setBlockAndUpdate(hookPos, newState with FACING = dir)`; if `canUpdate`: `notify(hookPos, dir)`.
  7. If `wasAttached ≠ attached`: for `i = 1 … receiverDist − 1`: if `wires[i]` is non-null and the block at that position is currently a tripwire or hook: `setBlockAndUpdate(p, wires[i] with ATTACHED = attached)`.
- `notify(p, d)`: `updateNeighborsAt(p, hookBlock, orientation)` then `updateNeighborsAt(p + d.opposite, hookBlock, orientation)` (orientation `initialOrientation(front = d.opposite, up = UP)`).
- `onRemoved(state, pos)`: if `ATTACHED` or `POWERED`: `calculateState(pos, state, true, false, −1, none)`; then if `POWERED`: `notify(pos, state.FACING)`. `affectNeighborsAfterRemoval` (not moved): `onRemoved`.
- `emitSound`: first matching of: newly powered → click-on (pitch 0.6); newly unpowered → click-off (pitch 0.5); newly attached → attach (0.7); newly detached → detach (pitch `1.2 / (random.nextFloat() × 0.2 + 0.9)`: **one `nextFloat` of the level random is consumed only in this last case**), each with its game event (`block_activate`, `block_deactivate`, `block_attach`, `block_detach`).
- Signals: power §5. Max wire span: 40 blocks between the hooks (`i` up to 41).

### 7. Sensors and special sources

**7.1 Daylight detector** (`POWER`, `INVERTED`; block entity with no data). The block entity ticker exists only on a server level whose dimension **has skylight**; it runs `updateSignalStrength` **only when `gameTime % 20 == 0`**.
`updateSignalStrength(state, pos)`: `t = max(skyBrightness(pos) − skyDarken, 0)` (`getEffectiveSkyBrightness`); if `INVERTED`: `t = 15 − t`; else if `t > 0`: `a = SUN_ANGLE(pos) (degrees, environment attribute) × (π/180)` in f32; `off = (a < π) ? 0 : 2π` (f32); `a += (off − a) × 0.2f`; `t = round(t × cos(a))` (`Math.round` of the f32 product). `t = clamp(t, 0, 15)`; if `POWER ≠ t`: `setBlockAndUpdate(pos, POWER = t)`.
Hand-use (builders only): `cycle INVERTED`, `setBlock(pos, state, 2)`, then `updateSignalStrength(newState)`. **[oracle]** the `SUN_ANGLE` attribute values (they come from the world-clock timelines).
**7.2 Target block** (`POWER` 0–15): `onProjectileHit`: `strength` from the exact hit point: for the hit face axis, distances `dx, dy, dz = |frac(coord) − 0.5|`; `dist = max` of the two components **not** on the face axis (Y face: dx, dz; Z face: dx, dy; X face: dy, dz); `strength = max(1, ceil(15 × clamp((0.5 − dist)/0.5, 0, 1)))`. Duration = **20** ticks if the projectile is an arrow-like (`AbstractArrow`) else **8**. Only if the block has no scheduled tick: `setBlockAndUpdate(pos, POWER = strength)` then `scheduleTick(pos, block, duration)`. `tick`: if `POWER ≠ 0`: `setBlockAndUpdate(pos, POWER = 0)`. `onPlace` (old block differs, server): if `POWER > 0` and no tick scheduled: `setBlock(pos, POWER = 0, 18)`.
**7.3 Lightning rod** (`FACING` = the face it sticks out of, `POWERED`, `WATERLOGGED`; all copper stages): `onLightningStrike`: `setBlockAndUpdate(pos, POWERED)`; `updateNeighbours`; `scheduleTick(pos, block, 8)`; level event 3002. `updateNeighbours`: `updateNeighborsAt(pos + FACING.opposite, block, orientation)` — **only the supporting block's position**, not the rod's own. `tick`: `setBlockAndUpdate(pos, POWERED = false)` + `updateNeighbours`. `onPlace` (old block differs): if `POWERED` and no tick: `scheduleTick(pos, block, 8)`. `affectNeighborsAfterRemoval`: if `POWERED`: `updateNeighbours` (**also when moved by a piston**). Waterlogging: shape updates schedule the water fluid tick.
**7.4 Lectern** (`POWERED`, `HAS_BOOK`, `FACING`): page change (`signalPageChange`): `changePowered(true)`; `scheduleTick(pos, block, 2)`; level event 1043. `changePowered(p)`: `setBlockAndUpdate(pos, POWERED = p)` **and** `updateBelow` (`updateNeighborsAt(pos + DOWN, block, orientation)`); `tick`: `changePowered(false)`. `affectNeighborsAfterRemoval`: if `POWERED`: `updateBelow`. Signals: power §5; analog: §3.9.
**7.5 Trapped chest:** weak signal = `clamp(opener count, 0, 15)` of **this** half; the opener count changes through the container-opener counter (increment on open: if it was 0 → `onOpen` + `scheduleTick(pos, block, 5)` recheck; every count change queues the block event `(1, newCount)` and (trapped chest) `updateNeighborsAt(pos, block)` and `updateNeighborsAt(pos + DOWN, block)` when `previous ≠ current`). The 5-tick recheck re-counts the openers by scanning entities with this container open within `maxInteractionRange + 4` blocks (box inflated around the block) and, if the count changed, runs the same open / close handling (`recheckOpeners`). The chest block's `tick` calls `recheckOpen`; `recheckOpeners` always calls `openerCountChanged(previous, current)` (so a trapped chest re-issues its block event; its neighbour update only fires when `previous ≠ current`) and, **if the count is > 0, re-schedules the 5-tick recheck**. Orientation for the experiment: `initialOrientation(front = FACING.opposite, up = UP)`.
**7.6 Sculk sensors** (`PHASE` inactive/active/cooldown, `POWER`, `WATERLOGGED`; calibrated also `FACING`): active ticks **30** (calibrated **10**), cooldown ticks **10**, listener range **8** blocks (calibrated **16**).
- **Vibration reception** (block-entity ticker, server): `VibrationSystem.Ticker.tick` each tick: if no current vibration, take the selector's chosen candidate **only if it was recorded on an earlier game tick** than now (so a vibration is never delivered the tick it arrives); the travel time = `floor(distance)` ticks (distance = Euclidean from the source position to the sensor's **block centre**, f32); each tick decrement; at 0 `receive`: requires the sensor's chunk and **all 8 neighbouring chunks** to be block-ticking and loaded (else the vibration waits); `onReceiveVibration(distance)` with `distance = sqrt(distSqr(sourceBlock, sensorBlock))` of the **block positions** (f32).
- **Accepting a game event** (`handleGameEvent`, immediate on dispatch, listeners visited in registry order of their chunk sections; delivery mode unspecified, **not** sorted by distance): reject if a vibration is already current; reject if not `isValidVibration` (event not in the `vibrations` event tag; source entity is a spectator; sneaking source for events in the `ignore_vibrations_sneaking` tag; entity that dampens vibrations; the affected block state is in `dampens_vibrations`); reject if `canReceiveVibration` is false (frequency 0 events; the event happens at the sensor's own position and is `block_destroy` / `block_place`; the sensor is **not inactive**; calibrated: if the back input signal `b ≠ 0` then the event's frequency must equal `b`); reject if **occluded** (line from the source block centre to the sensor block centre blocked by blocks in `occludes_vibration_signals`, tested from the centre nudged by 1e-5 in each of the six directions; the vibration passes if **any** of the six rays is free); else `addCandidate(info(event, distance, origin, entity), gameTime)`.
- **Selector:** a new candidate replaces the stored one only if none is stored, or **the stored one was recorded the same game tick** and the new one is nearer, or equally near with a **higher frequency**.
- **Frequencies (redstone-relevant event table, `getGameEventFrequency`):** step, swim, flap = 1; projectile_land, hit_ground, splash, bounce = 2; item_interact_finish, projectile_shoot, instrument_play = 3; entity_action, elytra_glide, unequip = 4; entity_dismount, equip = 5; entity_interact, shear, entity_mount = 6; entity_damage = 7; drink, eat = 8; container_close, block_close, block_deactivate, block_detach = 9; container_open, block_open, block_activate, block_attach, prime_fuse, note_block_play = 10; block_change = 11; block_destroy, fluid_pickup = 12; block_place, fluid_place = 13; entity_place, lightning_strike, teleport = 14; entity_die, explode = 15; the 15 *resonate* events `resonate_n` = `n`; everything else 0 (not detected).
- **Power from distance:** `power = max(1, 15 − floor((15 / listenerRange) × distance))` (f64 scale, `distance` f32 above).
- **`onReceiveVibration`** (only if the sensor is still inactive): store `lastVibrationFrequency = frequency`; `activate(sourceEntity, pos, state, power, frequency)`.
- **`activate`:** `setBlockAndUpdate(pos, PHASE = active, POWER = power)`; `scheduleTick(pos, block, activeTicks)` (30 / 10); `updateNeighbours` (`updateNeighborsAt(pos, block)`, `updateNeighborsAt(pos + DOWN, block)`); resonance: for `d` in `Direction.values()`: if the block at `pos + d` is in `vibration_resonators` post game event `resonate_<frequency>` at that position and play the amethyst resonate sound; game event `sculk_sensor_tendrils_clicking`; if not waterlogged click sound (pitch `random.nextFloat() × 0.2 + 0.8`, **one level-random float**).
- **`tick`:** phase ≠ active: if cooldown → `setBlockAndUpdate(pos, PHASE = inactive)` (+ stop-click sound with one random float if not waterlogged); else (active) → `deactivate`: `setBlockAndUpdate(pos, PHASE = cooldown, POWER = 0)`, `scheduleTick(pos, block, 10)`, `updateNeighbours`.
- **Step-on** (`stepOn`, server): if inactive, not a warden, and `canReceiveVibration(step)`: **force-schedule** a `step` vibration from the entity position (bypasses the occlusion and "already has a vibration" checks).
- `onPlace` (old block differs): if `POWER > 0` and no tick pending: `setBlock(pos, POWER = 0, 18)`. `affectNeighborsAfterRemoval`: if phase = active: `updateNeighbours`.
- Signals / analog: power §5, §3.9. The block entity persists `last_vibration_frequency` and the listener data (current vibration, selector, remaining travel time).

### 8. Consumers

Unless stated, "powered" = `hasNeighborSignal(pos)` (power §2.6, **no** quasi-connectivity).
**8.1 Redstone lamp** (`LIT`): placement: `LIT = hasNeighborSignal`. `neighborChanged` (server): `lit = LIT`; if `lit ≠ hasNeighborSignal`: if `lit`: `scheduleTick(pos, block, 4)` (NORMAL) else `setBlock(pos, LIT cycled → true, 2)`. `tick`: if `LIT` and not powered: `setBlock(pos, LIT = false, 2)`. (On: immediate; off: 4-tick delay. No `onPlace`.)
**8.2 Note block** (`INSTRUMENT`, `NOTE` 0–24, `POWERED`): `neighborChanged`: `s = hasNeighborSignal`; if `s ≠ POWERED`: if `s`: `playNote(null)`; then `setBlockAndUpdate(pos, POWERED = s)`. `playNote`: if the instrument "works above a note block" **or** the block above is air: `blockEvent(pos, block, 0, 0)` + game event `note_block_play`. `triggerEvent` (block event): sound / particle only (returns true; `getPitch = 2^((note − 12)/12)`; head-sound lookup for custom instruments may return false). `updateShape` for vertical neighbours recomputes `INSTRUMENT` (from the block above if it works above a note block, else from below, mapping "works above" instruments to harp). Hand use: `cycle NOTE`, `setBlockAndUpdate`, `playNote`.
**8.3 Dispenser / dropper** (`FACING` any of 6, `TRIGGERED`, block entity container 9 slots): `neighborChanged`: `want = hasNeighborSignal(pos) || hasNeighborSignal(pos + UP)`; if `want && !TRIGGERED`: **first** `scheduleTick(pos, block, 4)` (NORMAL) **then** `setBlock(pos, TRIGGERED = true, 2)`; else if `!want && TRIGGERED`: `setBlock(pos, TRIGGERED = false, 2)`. `tick` → `dispenseFrom`.
- `getRandomSlot(random)`: scans slots in order; for every **non-empty** slot, `if random.nextInt(k) == 0: chosen = slot` with `k` starting at 1 and incremented after **each** non-empty slot (reservoir sampling; one level-random draw per non-empty slot). `-1` if empty. (Loot-table unpack first.)
- Dispenser: slot < 0 → level event 1001 (empty click) + game event; else the dispense behaviour chosen by item (registry; items with an equippable component use the equip behaviour; sulfur-cube swallowable items, spawn eggs …) replaces the slot content. Dropper: slot < 0 → level event 1001; else if the slot has an item: target = container at `pos + FACING` (hopper-style lookup); no container → default drop (item entity); container → try to insert **one** item with `addItem(… from FACING.opposite)`; on success the slot loses one item, on failure the slot is unchanged.
- Analog output = container fill; `affectNeighborsAfterRemoval` → `updateNeighbourForOutputSignal(pos, block)`.
**8.4 Hopper** (`FACING` DOWN or horizontal, `ENABLED`): `onPlace` (old block differs) and `neighborChanged`: `want = !hasNeighborSignal(pos)`; if `want ≠ ENABLED`: `setBlock(pos, ENABLED = want, 2)`. Transfer logic (block entity ticker, cooldown **8** ticks, `ENABLED` gate) belongs to the inventory milestone.
**8.5 Crafter** (`ORIENTATION`, `TRIGGERED`, `CRAFTING`; block entity mirrors `triggered`): placement `TRIGGERED = hasNeighborSignal`; `setPlacedBy`: if `TRIGGERED` → `scheduleTick(pos, block, 4)`. `neighborChanged`: `want = hasNeighborSignal(pos)`: rising: `scheduleTick(pos, block, 4)`; `setBlock(pos, TRIGGERED = true, 2)`; entity `triggered = true`. Falling: `setBlock(pos, TRIGGERED = false, CRAFTING = false, 2)`; entity `triggered = false`. `tick` → `dispenseFrom`: no recipe → level event 1050; else `craftingTicksRemaining = 6`, `setBlock(pos, CRAFTING = true, 2)`, items spawn / insert into the container in front (`FRONT` of `ORIENTATION`); every non-empty slot loses 1. Block-entity ticker: `rem = craftingTicksRemaining − 1`; if `rem ≥ 0`: store; if `rem = 0`: `setBlockAndUpdate(pos, CRAFTING = false)`. Analog: §3.9.
**8.6 Doors** (`HALF`, `FACING`, `HINGE`, `OPEN`, `POWERED`): `neighborChanged(sourceBlock)`: `s = hasNeighborSignal(pos) || hasNeighborSignal(pos + (lower ? UP : DOWN))`; **if the source block is not this door's own block type** and `s ≠ POWERED`: if `s ≠ OPEN`: sound (`random.nextFloat() × 0.1 + 0.9` pitch — **one level-random float**) + game event; `setBlock(pos, POWERED = s, OPEN = s, 2)`. Both halves react independently to updates addressed to them. Iron doors cannot be opened by hand (only power); copper doors can. `setOpen` (hand / wind charge): flags 10.
**8.7 Trapdoors:** `neighborChanged` (server): `s = hasNeighborSignal(pos)`; if `s ≠ POWERED`: if `OPEN ≠ s`: `OPEN = s` and sound (one random float, game event); `setBlock(pos, POWERED = s, 2)`; if waterlogged `scheduleTick(pos, water, 5)`. Placement: powered ⇒ `OPEN` and `POWERED` true.
**8.8 Fence gates:** `neighborChanged` (server): `p = hasNeighborSignal(pos)`; if `p ≠ POWERED`: `setBlock(pos, POWERED = p, OPEN = p, 2)`; then if the **old** `OPEN ≠ p`: sound (one random float) + game event. Placement: `OPEN = POWERED = hasNeighborSignal`.
**8.9 TNT** (`UNSTABLE`): `onPlace` (old block differs) and `neighborChanged`: if `hasNeighborSignal(pos)` and `prime(pos)` succeeds (server, `tnt_explodes` rule on): spawn the primed-TNT entity at `(x+0.5, y, z+0.5)` (fuse default 80), play the prime sound, game event `prime_fuse`, then `removeBlock(pos, false)` (flags 3). TNT is **not** a redstone conductor.
**8.10 Bell** (`POWERED`, …): `neighborChanged`: `s = hasNeighborSignal`; if `s ≠ POWERED`: if `s`: `attemptToRing(direction = none → FACING)`: block entity `onHit` queues the block event `(1, direction 3D value)`, sound, game event `block_change`; then `setBlockAndUpdate(pos, POWERED = s)`. Ring animation / raider resonance are block-entity details (ticks ≥ 5 resonance start, 40 resonance ticks, shake ends at 50).
**8.11 Copper bulb** (`LIT`, `POWERED`; all weathering stages): `onPlace` (old block differs, server) and `neighborChanged` (server): `s = hasNeighborSignal`; if `s ≠ POWERED`: if the bulb was not powered, also flip `LIT` (+ sound); `setBlockAndUpdate(pos, POWERED = s [, LIT flipped])`. So the light toggles on every **rising** edge. Analog: 15 when lit.
**8.12 Skulls / heads** (`POWERED`): placement sets it from `hasNeighborSignal`; `neighborChanged` (server): if `≠`: `setBlock(pos, POWERED, 2)` (mouth animation only).
**8.13 Shelf** (`POWERED`, `SIDE_CHAIN_PART`): `neighborChanged` (server): `s = hasNeighborSignal`; if `≠ POWERED`: new state with `POWERED = s` (and `SIDE_CHAIN_PART = unconnected` when off); `setBlockAndUpdate`; activate / deactivate sound and game event.
**8.14 Command block:** `neighborChanged` (server, with block entity): `p = hasNeighborSignal`; if it differs from the stored `powered`: store it; if now powered and the block is **not** automatic and not in `sequence` mode: `markConditionMet()` and `scheduleTick(pos, block, 1)`. `tick`: mode `auto`: `markConditionMet`; if the condition was met → execute; else if conditional → success count 0; if powered or automatic → `scheduleTick(pos, block, 1)` again; mode `redstone`: execute if the condition was met (else success count 0 for conditionals); finally `updateNeighbourForOutputSignal(pos, block)`. Chain execution (sequence blocks follow `FACING`, limit = game rule `max_command_sequence_length`) is the command milestone's.
**8.15 Structure block:** `neighborChanged` (server, with block entity): `p = hasNeighborSignal`; rising edge: store powered and run the mode action (save / load / corner → unload / data → nothing); falling edge: store not powered.
**8.16 Big dripleaf:** `neighborChanged`: if `hasNeighborSignal(pos)`: reset tilt (write `NONE` with flags 2 and, if it was not `NONE`, play the tilt-up sound with a random pitch `randomBetween(0.8, 1.2)`); `tick`: if `hasNeighborSignal(pos)`: reset; else advance `UNSTABLE → PARTIAL → FULL` (each `scheduleTick` per the tilt delays of the plant spec) and `FULL → NONE`.

### 9. Rails as redstone consumers

**9.1 Powered rail and activator rail** (the **same** class; `SHAPE` straight shapes only, `POWERED`, `WATERLOGGED`):
- `neighborChanged` (server, block still this rail): if the rail must be removed (`shouldBeRemoved`: the block below is not RIGID-supporting, or for an ascending shape the block on the rising side is not RIGID-supporting): drop + `removeBlock(pos, movedByPiston)`; else `updateState(state, pos, sourceBlock)`.
- **`updateState`** (powered / activator): `isPowered = POWERED`; `should = hasNeighborSignal(pos) || search(pos, forward = true, depth 0) || search(pos, forward = false, depth 0)`; if `should ≠ isPowered`: `setBlockAndUpdate(pos, POWERED = should)`; `updateNeighborsAt(pos + DOWN, block)`; if the shape is a slope: `updateNeighborsAt(pos + UP, block)`.
- **`search(pos, state, forward, depth)`:** false if `depth ≥ 8`. Step table (coordinates `x, y, z` of `pos`; "compat" = the axis family used to reject crossing rails; `checkBelow` = also test one block lower):

| shape | forward | backward | compat | checkBelow |
|---|---|---|---|---|
| north_south | `z+1` | `z−1` | NS | yes |
| east_west | `x−1` | `x+1` | EW | yes |
| ascending_east | `x−1` | `x+1, y+1` (no below) | EW | forward yes / backward **no** |
| ascending_west | `x−1, y+1` (no below) | `x+1` | EW | forward **no** / backward yes |
| ascending_north | `z+1` | `z−1, y+1` (no below) | NS | forward yes / backward **no** |
| ascending_south | `z+1, y+1` (no below) | `z−1` | NS | forward **no** / backward yes |

  Result = `sameRailPowered(target, forward, depth, compat)` or (`checkBelow` and the same at `y−1`).
- **`sameRailPowered(p, forward, depth, compat)`:** the block at `p` must be **this same block** (powered rails only chain with powered rails, activator with activator). Reject if `compat = EW` and its shape is NS / ascending_north / ascending_south, or `compat = NS` and its shape is EW / ascending_east / ascending_west. Else it must be `POWERED`; then `true` if `hasNeighborSignal(p)` else `search(p, thatState, forward, depth + 1)` (`forward` keeps its meaning relative to **each** rail's own shape).
- `onPlace` (old block differs) and the generic rail `updateState(state, pos, movedByPiston)`: first the **rail-shape solver** (`RailState.place`, **not specified here**, see Open questions) then, for straight rails, a Full neighbour update of the rail to itself (`neighborChanged(state, pos, this, null, movedByPiston)`), which runs the powered logic above. `affectNeighborsAfterRemoval` (not moved): slope → `updateNeighborsAt(pos + UP, block)`; straight → `updateNeighborsAt(pos, block)` and `updateNeighborsAt(pos + DOWN, block)`.
**9.2 Detector rail** (`POWERED`; `SHAPE` straight): signals / analog: power §5, §3.9. Search box: x, z ∈ [0.2, 0.8], y ∈ [0, 0.8] (all of `pos` inset by 0.2 except the bottom) for minecarts (`AbstractMinecart`; command-block minecarts and container minecarts for the analog value).
- `entityInside` (server): if `!POWERED`: `checkPressed`. `tick`: if `POWERED`: `checkPressed`. `onPlace` (old block differs): `updateState(…)` (§9.1 generic) then `checkPressed(updatedState)`.
- `checkPressed(state)`: only if the rail can still survive (RIGID support below) and the block at `pos` is still this rail: `want` = any minecart in the box; `was = POWERED`. If `want && !was`: `setBlockAndUpdate(pos, POWERED = true)`; `updatePowerToConnected`; `updateNeighborsAt(pos, block)`; `updateNeighborsAt(pos + DOWN, block)`. If `!want && was`: same with `false`. If `want`: `scheduleTick(pos, block, 20)`. **Always** (also when nothing changed): `updateNeighbourForOutputSignal(pos, block)`.
- `updatePowerToConnected`: for every rail position connected to this one by the rail-shape solver, in the solver's order: a Full neighbour update `neighborChanged(state(conn), conn, thatBlock, null, false)`.
**9.3 `getAnalogOutputSignal` of the detector rail:** §3.9.

### 10. Delay and priority summary (game ticks)

| component | trigger → action | delay | priority |
|---|---|---|---|
| redstone torch | neighbour change (lit == input) → tick | 2 | NORMAL |
| torch burnout | 8th toggle in 60 ticks → tick | 160 | NORMAL |
| repeater | input change → tick (rising) | 2·DELAY | HIGH (EXTREMELY_HIGH if prioritised) |
| repeater | input change → tick (falling) | 2·DELAY | VERY_HIGH (EXTREMELY_HIGH if prioritised) |
| repeater | after turning on with input already gone | 2·DELAY | VERY_HIGH |
| repeater | `setPlacedBy` with input | 1 | NORMAL |
| comparator | input / output differs → tick | 2 | NORMAL (HIGH if prioritised) |
| observer | observed change → tick; second tick after turning on | 2 and 2 | NORMAL |
| lamp (turn off) | unpowered → tick | 4 | NORMAL |
| dispenser / dropper / crafter | rising edge → tick | 4 | NORMAL |
| stone / wood button | press → tick | 20 / 30 | NORMAL |
| basic / weighted plate | pressed → re-check | 20 / 10 | NORMAL |
| tripwire | pressed → re-check; unpressed → final tick | 10; 1 | NORMAL |
| tripwire hook | wire change → recalculation tick | 10 | NORMAL |
| target block | hit → tick | 20 (arrow) / 8 | NORMAL |
| lightning rod | strike → tick | 8 | NORMAL |
| lectern | page turn → tick | 2 | NORMAL |
| sculk sensor | activate → tick; cooldown → tick | 30 / 10 (calibrated 10 / 10) | NORMAL |
| trapped chest recheck | while opened | 5 | NORMAL |
| detector rail | minecart present → recheck | 20 | NORMAL |
| command block (auto / redstone) | edge → tick | 1 | NORMAL |
| piston | event queued at the end of the level tick (not a scheduled tick) | 0 (same tick, after all scheduled ticks) | — |

## Edge cases and known quirks

1. **0-tick and short pulses through diodes:** a repeater stretches any pulse shorter than its delay to exactly its delay (2.7); a torch's 2-tick delay filters pulses shorter than 2 ticks only if the input drops before the tick runs (the tick re-reads the input).
2. **`willTickThisTick` suppression** (diodes, torches, comparator): an input change that arrives *while the component's own tick is already collected for this level tick* does not schedule a new one; the running tick re-reads the input, but a change that happens **after** it has run in the same tick is **lost** until the next neighbour update (a classic dropped-pulse quirk).
3. **One tick per `(block, pos)`:** a new schedule is ignored even if earlier or of better priority; the repeater's `HIGH`/`VERY_HIGH` choice therefore only matters when no tick is pending.
4. **Repeater `LOCKED` is cosmetic**; the logic recomputes the lock live. A locked repeater holds its state because it skips `tick` and `checkTickOnNeighbor`.
5. **Comparator reading through a conductor** reads the analog block / item frame **behind** the conductor and overrides the direct reading; a conductor in front of the comparator that is itself powered up to 15 disables the look-through (`r < 15` condition).
6. **Comparator subtract/compare quirk:** the front is notified on every tick in compare mode even if nothing changed (3.5), but a comparator tick is scheduled only when output or `POWERED` is about to change.
7. **Dispenser quasi-connectivity** (8.3) and piston quasi-connectivity (power §4) are intentional; doors check the other half only.
8. **Observer ignores changes while powered** and while a tick is pending; shape updates issued with flag 16 never trigger it.
9. **Trapdoor / fence gate / door sounds consume one `nextFloat` from the level random** per state change by power; wood types and iron types alike. Also: sculk sensor click and stop-click, tripwire-hook detach, big dripleaf tilt.
10. **Lightning rod removal while moved by a piston still notifies**; most other components guard with `!movedByPiston`.
11. **Redstone torch toggle list** is per level and in memory only: a restart forgives burnout; unloading chunks does not clear it.
12. **Daylight detector** only updates once per second (20 ticks), and only in dimensions with skylight.
13. **`hasNeighborSignal` consumers fire on state *edges*** (they compare against a stored `POWERED` / `TRIGGERED` / `LIT`), so a pulse shorter than the delay between notifications can be missed if both edges occur before the component is notified.

## Constants and IDs

Torch: 60 / 8 / 160 / 2. Repeater delay `2·DELAY`, comparator delay 2. Observer 2 + 2. Lamp off delay 4. Dispenser / dropper / crafter 4. Crafter crafting ticks 6. Buttons 20 / 30. Plates 20 / 10; weighted max 15 / 150. Tripwire recheck 10, unpressed final 1; wire span 41. Target 20 / 8. Lightning rod 8. Lectern 2. Sculk: active 30 (cal. 10), cooldown 10, range 8 (cal. 16), power `max(1, 15 − floor(15/range × d))`. Trapped chest recheck 5. Detector rail 20; search box inset 0.2. Rail power search depth 8. Hopper cooldown 8. Block-event ids: note block `(0, 0)`, bell `(1, dir)`, chest `(1, count)`.

## Oracle probe points

- Diodes: `DiodeBlock#getInputSignal(Level, BlockPos, BlockState)`, `#getAlternateSignal(SignalGetter, BlockPos, BlockState)`, `#isLocked`, `#shouldPrioritize`, `#checkTickOnNeighbor`, `#tick`, `#updateNeighborsInFront`; `RepeaterBlock#getDelay`; `ComparatorBlock#calculateOutputSignal`, `#shouldTurnOn`, `#refreshOutputState`, `#getInputSignal`, `ComparatorBlockEntity#getOutputSignal / #setOutputSignal`. Log `(gameTime, pos, hook, args)`; compare ordered logs per tick.
- Scheduled ticks: wrap `ServerLevel#scheduleTick(BlockPos, Block, int, TickPriority)` and log delay and priority per call; dump `LevelTicks#count()` / `#willTickThisTick`.
- Torch: `RedstoneTorchBlock#tick`, `RedstoneTorchBlock#isToggledTooFrequently` (private; agent reflection) and the static `RECENT_TOGGLES` map.
- Observer: `ObserverBlock#tick`, `#updateShape`; `ButtonBlock#press / #checkPressed`; `LeverBlock#pull`; `BasePressurePlateBlock#checkPressed / #getSignalStrength`; `WeightedPressurePlateBlock#getSignalStrength`; `TripWireHookBlock#calculateState` (public static); `TripWireBlock#checkPressed`; `TargetBlock#onProjectileHit`; `LightningRodBlock#onLightningStrike`; `DaylightDetectorBlock#updateSignalStrength` (private static; reflection).
- Sculk: `SculkSensorBlock#activate / #deactivate`, `VibrationSystem$Listener#handleGameEvent`, `VibrationSystem$Ticker#tick`, `VibrationSystem#getRedstoneStrengthForDistance(float, int)`, `VibrationSystem#getGameEventFrequency(Holder)`.
- Consumers: `DispenserBlock#neighborChanged / #dispenseFrom`, `DispenserBlockEntity#getRandomSlot`, `HopperBlock#checkPoweredState`, `CrafterBlock#neighborChanged / #dispenseFrom`, `DoorBlock#neighborChanged`, `TrapDoorBlock#neighborChanged`, `FenceGateBlock#neighborChanged`, `NoteBlock#neighborChanged / #playNote`, `RedstoneLampBlock#neighborChanged`, `TntBlock#prime`, `BellBlock#neighborChanged`, `CopperBulbBlock#checkAndFlip`.
- Rails: `PoweredRailBlock#updateState / #findPoweredRailSignal`, `DetectorRailBlock#checkPressed`.
- Analog: `BlockBehaviour$BlockStateBase#getAnalogOutputSignal(Level, BlockPos, Direction)`, `AbstractContainerMenu#getRedstoneSignalFromContainer(Container)`, `Mth#lerpDiscrete(float, int, int)`.

## Open questions

1. **[oracle] `willTickThisTick` lost-pulse quirk** (Edge 2): probe two inputs changing the same tick on a repeater and a torch; log which scheduled ticks exist.
2. **[oracle] Observer placement facing** (4.7).
3. **[oracle] Repeater prioritisation** (2.6): probe repeater-into-repeater head-on, side-on, and into a comparator; log the priority of the scheduled tick.
4. **[oracle] Sculk sensor listener order and selector** (7.6): probe two sensors and one event; confirm same-tick candidate replacement and the "recorded on an earlier tick" delivery rule; probe occlusion with wool vs glass; probe the frequency map for events routed through `Level#gameEvent`.
5. **[oracle] Daylight detector formula** (7.1), including `SUN_ANGLE` values at several clock times in the overworld, and the f32 rounding of `Math.round`.
6. **[oracle] Container openers recheck** (7.5): confirm the 5-tick recheck chain and the block-event stream `(1, count)` while a player keeps a trapped chest open.
7. **[oracle] Dispenser slot selection draws** (8.3): confirm one `nextInt(k)` per non-empty slot including `k = 1`.
8. **[oracle] Vibration travel time / distance units** (7.6): `calculateTravelTimeInTicks = floor(distance)` with `distance` the f32 Euclidean distance between the **origin vector and the listener position source** in the selector (not the block-position distance used for power).
9. **(unresolved)** `RailState` (rail shape solver and `getConnections` order) is outside this reading; needed for plain / powered / detector rail placement shapes and `updatePowerToConnected`.
10. **(unresolved)** Entity-side triggers (`entityInside` sweep order and timing for plates, buttons, detector rails, tripwires) belong to the entity-physics milestone; the block-side responses here are complete.
11. **(unresolved, other milestone)** Dispense behaviours of individual items, hopper item transfer, crafter recipe matching, bell raider resonance, big dripleaf tilt delays, command execution.

## Parity status

Not yet run.
