# Ticks: fluids — state model, flow algorithm, water / lava, waterlogging, bubble columns (26.3)

- Verified by suites: `fluids`; `server-tick`, `block-updates` (tick framework it plugs into)
- Sources: local reference reading of the 26.3 client/server code (reader session): the fluid base class and its flowing
  / water / lava / empty subclasses, the fluid-state wrapper and the fluid registry, the liquid block, the bubble-column
  block, the waterlogging / bucket-pickup / liquid-container interfaces and every block that implements them, the bucket
  item's world side, the sponge's water removal, the chunk post-processing loop, the world-generation call sites that
  schedule or mark fluids, the block / fluid tag providers, and the game-rule and environment-attribute registries.
  Tag membership was read from the data generators (tags are data: Zenith loads them from the install at runtime).
  Everything is in our own words.
- Related specs: [`scheduled-ticks-and-block-events.md`](scheduled-ticks-and-block-events.md) (the fluid tick container,
  drain order, dedup, post-processing §5.5), [`set-block-flags-and-sequence.md`](set-block-flags-and-sequence.md),
  [`neighbor-and-shape-updates.md`](neighbor-and-shape-updates.md), [`block-hooks.md`](block-hooks.md),
  [`server-tick.md`](server-tick.md) (random-tick loop §10), [`collection-orders.md`](collection-orders.md),
  [`../blocks/state-facts.md`](../blocks/state-facts.md) (fluid state of each block state §3.1, legacy-solid, replaceable),
  [`../blocks/shapes.md`](../blocks/shapes.md) (collision shapes, `mergedFaceOccludes`, BLOCK / EMPTY identity),
  [`../worldgen/aquifers-and-ore-veins.md`](../worldgen/aquifers-and-ore-veins.md) (when the aquifer asks for a fluid update),
  [`../worldgen/structures-placement-and-jigsaw.md`](../worldgen/structures-placement-and-jigsaw.md) §4 (template waterlogging),
  [`../math/random.md`](../math/random.md).

## Summary

Water and lava are *flowing fluids*: a grid cell holds a *fluid state* (type, amount 1–8, falling flag, source or not) that
is derived from its block state (a *liquid block* with a 0–15 `level` property, or a waterlogged / always-wet block).
A fluid cell acts only when a **scheduled fluid tick** (the fluid half of [`scheduled-ticks-and-block-events.md`](scheduled-ticks-and-block-events.md))
fires. That tick (1) re-computes the cell's own state from its neighbours (non-source cells only), (2) spreads: first
downwards, then — depending on a short search for the nearest "hole" — into some of the four horizontal neighbours by writing
new liquid blocks. Everything else (obsidian, cobblestone, basalt, the fizz event, scheduling the next tick, bubble columns)
is done by block hooks of the liquid block and of waterloggable blocks reacting to those writes.

Randomness: water draws **nothing**. Lava draws in exactly two places (a spread-delay roll, §9.3, and the random-tick fire
spread, §9.5). Drops of blocks destroyed by flowing water use the loot / drop code (another spec).

## Inputs and state

### Fluid types and fluid states

Five fluid types, registry order (ids as `fluid_id` of ZBF1): `empty`, `flowing_water`, `water`, `flowing_lava`, `lava`.
"Water family" = `water` + `flowing_water`; "lava family" likewise. `isSame(a, b)` for water / lava: true iff both are in the
same family; for the empty fluid: identity. `is(x)` on a fluid state with a **fluid type** is **identity** (so *source* water ≠ *flowing*
water); `is(tag)` is membership of the fluid tag; tags: `#water` = {water, flowing_water}, `#lava` = {lava, flowing_lava},
`#bubble_column_can_occupy` = {water} only (not flowing_water).

A **fluid state** is interned (compare by `(type, amount/level, falling)`, i.e. identity):

| type | properties | states |
|---|---|---|
| `empty` | none | 1 (amount 0, height 0) |
| `water`, `lava` (source) | `falling` (bool) | 2 each; amount always **8**; `isSource` true |
| `flowing_water`, `flowing_lava` | `falling` (bool), `level` 1–8 (= amount) | 16 each; `isSource` false |

- **amount** of a source = 8; of a flowing state = `level`. `isFull` = amount 8 (true for every source and for flowing level 8).
- **own height** = `amount / 9` in **f32** (source 0.8888889, flowing 8 → 0.8888889, 4 → 0.44444445, 3 → 0.33333334).
  **height at a position** = 1.0 if the fluid state of the cell **above** has the **same fluid type family** (`isSame` with this fluid's
  type; for the empty fluid: identity), else own height. (Used by `canBeReplacedWith` of lava, lava spread delay, entities, rendering.)
- Water and lava constants (the only per-fluid differences):

| | water | lava, normal dimension | lava, "fast lava" dimension |
|---|---|---|---|
| tick delay | 5 | 30 | 10 |
| drop-off per cell | 1 | 2 | 1 |
| slope search depth (`getSlopeFindDistance`) | 4 | 2 | 4 |
| infinite-source game rule | `water_source_conversion` (default **true**) | `lava_source_conversion` (default **false**) | same |
| randomly ticking fluid | no | **yes** | yes |
| explosion resistance | 100 | 100 | 100 |

- **Fast lava** = the dimension-wide environment attribute `gameplay/fast_lava` (default false; true only in the Nether's built-in
  dimension type; no timeline modifies it). Read it from the dimension type data at runtime. Every "lava" delay / depth / drop-off
  query is evaluated for the level (dimension) of the cell being processed.
- The **game rules** are server-wide values read when a fluid tick runs; not per dimension.

### Liquid blocks and the legacy `level` property

Two liquid blocks, `water` and `lava`, each with the integer property `level` 0–15 (default 0). Both are: replaceable, no collision,
not a block entity, empty visual shape, no loot, legacy `liquid` flag true, `propagates skylight down` false, destroy strength 100.
**Lava additionally** emits light 15 and is a *randomly ticking block* (it is also a randomly ticking *fluid*, so the section's block and
fluid counters both include it; see §9.5).

**Block → fluid state** (`getFluidState`): index = `min(level, 8)`: 0 → source (falling false); 1…7 → flowing, amount `8 − level`, falling false;
8…15 → flowing amount 8, **falling true**.
**Fluid state → block** (`createLegacyBlock`, used by every flow write): the block of the state's family with
`level = (source ? 0 : 8 − min(amount, 8) + (falling ? 8 : 0))`; the empty fluid → plain air. Consequences: a source (falling or not) ↔ level 0;
flowing amount 8 **not falling** would also encode as level 0 (a source) — the algorithm never produces it (flowing amounts it produces are ≤ 7
unless falling, and a falling state is always amount 8). Levels 9–15 decode like level 8; writing them back normalises to 8.

### Other blocks that hold fluid

- **Waterlogged blocks** (a `waterlogged` boolean property): fluid state = `water` source, **falling false**, iff waterlogged; **copper grates** use
  the `water` source with **falling true**; otherwise empty. (Also see [`../blocks/state-facts.md`](../blocks/state-facts.md) §3.1.)
- **Always-wet blocks** (water source, falling false, regardless of state): seagrass, tall seagrass, kelp, kelp plant, bubble column.
- Any other block: the empty fluid.

### Tags and predicates the algorithm reads (data, from the install)

- Block tag **`washed_away_by_fluids`** — blocks a fluid may replace (in vanilla: fire, saplings, corals and coral fans, air kinds, flower pots,
  buttons, wool carpets, candles, rails, skulls, copper golem statues, `water`, `lava`, tall seagrass, wall torches of every kind, attached stems,
  kelp plant, the vine plants, big dripleaf stem, wall skulls / heads and more — read the tag, do not hard-code).
- Block tag **`blocks_fluid_flow`** — blocks whose side shows "no flow" for the `getFlow` vector (§12): the `blocks_motion` tags plus all signs.
- Block tag **`blocks_lava_fire_spread`** — stops lava's fire-spread walk (= the `blocks_motion` tag).
- Block tags **`enables_bubble_column_push_up`** (soul sand) and **`enables_bubble_column_drag_down`** (magma block); **`cannot_support_kelp`** (magma block).
- State predicates: `isSolid` (legacy solid), `isAir`, `ignitedByLava`, `canBeReplaced` (replaceable flag), `hasDynamicShape` (position-dependent shape),
  collision shape ([`../blocks/state-facts.md`](../blocks/state-facts.md), [`../blocks/shapes.md`](../blocks/shapes.md)).

### Direction orders used here

`Direction` ordinals: DOWN 0, UP 1, NORTH 2, SOUTH 3, WEST 4, EAST 5.
- **Horizontal search order** (`Plane.HORIZONTAL`): **NORTH, EAST, SOUTH, WEST** — used by every loop below *except* §6.5 step 2.
- **Spread-map application order** = the map iterates by ordinal: **NORTH, SOUTH, WEST, EAST** (§6.5 step 2).
- Liquid-block neighbour probe list for lava conversion: DOWN, SOUTH, NORTH, EAST, WEST (§10.1).
- Standard neighbour / shape update orders: [`neighbor-and-shape-updates.md`](neighbor-and-shape-updates.md).

## Behavior (ordered rules)

### 1. When a fluid tick is scheduled (all callers)

A fluid tick is a scheduled tick of **type = a fluid** (identity: `water` ≠ `flowing_water`), default priority `NORMAL`, per the container rules
(one per `(fluid, pos)`; a later schedule of an existing pair is **dropped**, whatever its delay; see
[`scheduled-ticks-and-block-events.md`](scheduled-ticks-and-block-events.md) §1). The delay always comes from the *fluid's* tick delay evaluated for the level
(water 5; lava 30 / 10). Callers:

1. **Liquid block `onPlace`** (new state of a liquid block, any write without flag 512): if the block does *not* turn itself to stone first (§10.1) →
   schedule `(fluid type of the new state, pos)` with the block's fluid delay. (The block's own fluid, not the state's: a water block always uses 5.)
   Then the bubble-column check of §11.1.
2. **Liquid block `neighborChanged`** (any neighbour update, no filtering of which neighbour): identical to 1 (including the lava conversion test
   and the bubble-column check).
3. **Liquid block `updateShape`** (a shape update from any direction): if the block's own fluid state is a source **or** the neighbour state's
   fluid state is a source → schedule `(block's current fluid type, pos)` with the block's delay; then, if the update comes from **below**
   and the block is an occupiable water source (§11.1) → bubble check on the neighbour state. Returns the state unchanged.
4. **Waterlogged / always-wet blocks' `updateShape`** — the common pattern: if the block currently holds water (waterlogged or always-wet),
   schedule `(water **source** type, pos)` with delay **5**, then the block's own shape logic (which may return a different state or air).
   Per-class variants (§8.3). Note the tick type is `water` (the *source* fluid) for a waterlogged block; it fires only while the cell's fluid is
   that exact type.
5. **`placeLiquid` of a waterloggable block** (§8.4): after setting the block waterlogged, schedule `(fluid type of the placed fluid state, pos)`
   with that type's delay (5 for water).
6. **`FlowingFluid.tick` itself** (§3 step 3 → the new state differs): schedule `(new fluid type, pos)` with the spread delay. This request is almost always
   **dropped as a duplicate** — see §9.3 and Edge case 1.
7. **World generation** (§13): features / structure pieces schedule `(fluid type, pos)` with delay 0 on the generation region (stored with delay 0 anyway).
8. **Trapdoor toggles / powered updates** while waterlogged: schedule `water` at delay 5 (after the state write).

### 2. Framework side of a fluid tick

When the level's fluid-tick action runs for `(pos, type)` ([`scheduled-ticks-and-block-events.md`](scheduled-ticks-and-block-events.md) §3.3): read the block state `B` at `pos`,
`F = B.fluidState`; **only if `F.type` is identical to `type`** call `F.tick(level, pos, B)` (→ §3). Otherwise the tick is dropped. Fluid ticks drain
**after** all block ticks of the level tick (separate container, same 65,536 limit). Ticks scheduled by a fluid tick run no earlier than the next level tick.
Chunk post-processing calls the same hook directly (§13).

### 3. `FlowingFluid.tick(level, pos, B, F)` — the fluid tick

1. If `F` is a **source**: skip steps 2–3 (a source never changes by itself).
2. Otherwise: `newF = getNewLiquid(pos, current block state at pos)` (§4). `delay = spreadDelay(old F, newF)` (§9.3 for lava — **may draw a random number even if the result is not used**; water: 5).
3. Branches (on the identity of the interned states):
   a. `newF` is **empty**: `F := empty`, `B := air`; write `air` at `pos` with `setBlockAndUpdate` (flags **3**; no drops). The fluid then stops.
   b. else if `newF ≠ F` (different type, amount or falling flag): `F := newF`, `B := newF.createLegacyBlock()`; write it with flags **3**; then schedule `(F.type, pos, delay)` (usually a duplicate of the tick that `onPlace` just scheduled → rejected, §1.6).
   c. else (unchanged): nothing is written.
4. **Always** (also for sources and for an unchanged flowing cell): `spread(pos, B, F)` (§6) using the **local** `B` / `F` values from steps 1–3 — **not re-read** from the world. (If the write of 3b triggered a re-entrant change — e.g. lava turning to cobblestone, §10.1 — the spread still runs with the stale lava state.)

Order of effects of one write in 3a / 3b: the write's full block-change sequence of [`set-block-flags-and-sequence.md`](set-block-flags-and-sequence.md) §2–§3: `onPlace` of the new block (schedules its own tick, §1.1) → neighbour-changed notification of the six neighbours (W,E,D,U,N,S; liquid neighbours schedule their ticks, §1.2) → six shape updates (W,E,N,S,D,U; liquid neighbours / waterlogged neighbours schedule ticks, §1.3–1.4).

### 4. `getNewLiquid(pos, blockState)` — the state a cell *should* have

Evaluated for the cell's own block state `S` (used for wall checks). Let `canPassWall(d, …)` be §5.1.

1. For each horizontal direction `d` (N, E, S, W) with neighbour `n` at `pos + d` and `Fn` = fluid state of the block at `n`: if `Fn`'s type is in **this fluid's family** *and* `canPassWall(d, pos, S, n, state(n))` → count it: `sources += 1` if `Fn` is a source; `highest = max(highest, Fn.amount)`. (Falling / non-falling make no difference to the count.)
2. **Source conversion:** if `sources ≥ 2` and the fluid's game rule allows conversion: let `below` = block state at `pos − Y`; if `below.isSolid` (legacy solid) **or** the fluid state of `below` is a **source of this family** (`isSame` family and isSource) → return the **source, falling false**.
3. **Falling from above:** `a` = block state at `pos + Y`, `Fa` its fluid state. If `Fa` is non-empty, in this family, and `canPassWall(UP, pos, S, pos + Y, a)` → return **flowing amount 8, falling true**.
4. `amount = highest − dropOff`. If `amount ≤ 0` → the **empty** fluid state; else **flowing(amount, falling false)**.

(Edge: step 1 ignores diagonal / vertical neighbours; step 2 only looks below, never above; the result's falling flag is true **only** from step 3.)

### 5. Passage predicates

**5.1 `canPassWall(direction, sourcePos, sourceState, targetPos, targetState)`** — "may fluid cross the face between two cells":
1. (Debug constants of the game are off; ignore.)
2. `T` = collision shape of `targetState` at `targetPos`, `Sh` = collision shape of `sourceState` at `sourcePos` (the *empty-context* collision shapes; liquid blocks give EMPTY; position-dependent shapes use the position). If `T` is the **BLOCK** singleton → false. If `Sh` is BLOCK → false. If **both** are EMPTY → true.
3. Else: result = **not** `mergedFaceOccludes(Sh, T, direction)` ([`../blocks/shapes.md`](../blocks/shapes.md) §4: with `first` = the shape on the negative side of the face along the direction's axis, `second` = the other; each is discarded (treated as EMPTY) if it does not reach 1 (resp. 0) on that axis; occludes iff the union of the two face layers covers the unit square). The memoisation of the game (a 200-entry LRU keyed by the two states and the direction, disabled when either block has a position-dependent shape) has no observable effect.
   (Structural vs identity BLOCK: a shape that is structurally a full cube but not the singleton still occludes in `mergedFaceOccludes`, so the result is the same; keep the singletons anyway.)
   The predicate is **symmetric** (swapping source / target and reversing the direction gives the same answer).

**5.2 `canHoldAnyFluid(state)`**: true if the block is a *liquid container* (§8.1) or the state is in block tag `washed_away_by_fluids`.
**5.3 `canHoldSpecific(state, pos, fluid)`**: if the block is a liquid container: its `canPlaceLiquid(none, …, fluid)` (§8.1: default `fluid == water SOURCE type`); else true. **Note** the argument is a fluid *type*: the flowing types never satisfy a waterloggable block (only the source type `water` does).
**5.4 `canHold(state, pos, fluid)`** = 5.2 ∧ 5.3.
**5.5 `isSourceOfThisType(fluidState)`** = family match ∧ isSource.
**5.6 `canMaybePassThrough(sourcePos, sourceState, d, testPos, testState, testFluidState)`** = **not** 5.5(`testFluidState`) ∧ 5.2(`testState`) ∧ 5.1(`d`, source→test).
**5.7 `canPassThrough(…, fluid = flowing type, …)`** = 5.6 ∧ 5.3(`testState`, flowing type of this family).
**5.8 `isWaterHole(topPos, topState, bottomPos, bottomState)`** = 5.1(DOWN, top→bottom) ∧ (bottom's fluid type is in this family **(any state, source included)** ∨ 5.4(bottom, flowing type)). The name is historical; it is used for both fluids.
**5.9 Replacement by a fluid state** (`FluidState.canBeReplacedWith(level, pos, otherFluid, direction)`; `this` = the fluid state of the target cell):
- empty fluid → **true**; water family → `direction == DOWN ∧ other ∉ #water`; lava family → `height(this at pos) ≥ 0.44444445 ∧ other ∈ #water` (any direction; flowing lava amount ≥ 4, or a source, or anything under lava, qualifies; amounts 1–3 do not).
- So flowing water never overwrites another water cell sideways, water *can* overwrite lava of height ≥ 4/9, lava only overwrites water **downwards** and then via the stone rule (§9.4).

### 6. `spread(pos, B, F)` — down first, then sideways

1. If `F` is empty → return.
2. `belowPos = pos − Y`, `belowState`, `belowFluid`. If `canMaybePassThrough(pos, B, DOWN, belowPos, belowState, belowFluid)` (§5.6):
   a. `newBelow = getNewLiquid(belowPos, belowState)` (§4). If `belowFluid.canBeReplacedWith(belowPos, newBelow.type, DOWN)` **and** `canHoldSpecific(belowState, belowPos, newBelow.type)`:
      `spreadTo(belowPos, belowState, DOWN, newBelow)` (§7); then **if the number of horizontal neighbours (N,E,S,W) that are sources of this family ≥ 3** → `spreadToSides` (§6.3); **return**.
3. (Reached when the downward move did not happen.) If `F` is a **source** **or** `isWaterHole(pos, B, belowPos, belowState)` is **false** → `spreadToSides`. (A flowing cell sitting directly over a hole does not spread sideways.)

**6.3 `spreadToSides(pos, F, B)`:** `reach = F.amount − dropOff`; **if `F.falling`: `reach := 7`** (regardless of fluid, amount or drop-off). If `reach > 0`: `spreads = getSpread(pos, B)` (§6.4); for each entry in the map's iteration order **NORTH, SOUTH, WEST, EAST**: `spreadTo(pos + dir, block state **read now**, dir, entry fluid state)` (§7). The new states in the map were all computed **before** the first write; the writes happen one by one, each with its full side effects, which may already change cells that later entries use.
Water source amount 8 → reach 7; flowing 1 → 0 (stops); lava normal: source 6, amount 2 → 0, so a lava flow reaches at most 3 cells from a source (reach 6, 4, 2, 0 → amounts 6, 4, 2 on cells at distance 1, 2, 3).

**6.4 `getSpread(pos, B)`** → ordered map `Direction → new fluid state`:
1. `lowest = 1000`; map empty; the **search context** (§6.5) is created lazily at the first use, with origin = `pos`.
2. For `d` in N, E, S, W: `testPos`, `testState`, `testFluid`. Only if `canMaybePassThrough(pos, B, d, testPos, testState, testFluid)` (§5.6):
   a. `nf = getNewLiquid(testPos, testState)`; only if `canHoldSpecific(testState, testPos, nf.type)`:
   b. `distance = 0` if `context.isHole(testPos)`; else `slope(testPos, pass 1, from = d.opposite, testState)` (§6.5).
   c. If `distance < lowest` → **clear the map**. If `distance ≤ lowest`: if `testFluid.canBeReplacedWith(testPos, nf.type, d)` → `map[d] = nf`; then **`lowest = distance`** (also when the entry was *not* added).
   (So a neighbour that is passable but not replaceable — e.g. an existing flowing-water cell next to a water cell — still shifts `lowest` and can wipe entries added for farther holes.)
3. Return the map.

**6.5 The slope search** (`getSlopeDistance(pos, pass, from, state, context)`):
1. `lowest = 1000`.
2. For `d` in N, E, S, W with `d ≠ from`: `testPos = pos + d`; `testState` from the context; if `canPassThrough(…, flowing type, pos, state, d, testPos, testState, testFluid)` (§5.7):
   - if `context.isHole(testPos)` → **return `pass` immediately** (earlier directions' results are discarded; later directions are not examined);
   - else if `pass < slopeFindDistance` → `v = slope(testPos, pass + 1, d.opposite, testState)`; `lowest = min(lowest, v)`.
3. Return `lowest` (1000 = "no hole within reach").
The **context** caches block states and hole results per `(dx, dz)` relative to the spread origin (a pure optimisation: every probed cell has the same Y as the origin, and the search never exceeds ±5 cells, so the cache never aliases and is unobservable). `isHole(p)` = §5.8 with top = `p` and its state, bottom = `p − Y`.
Depth semantics: `slopeFindDistance` = 4 (water, fast lava) or 2 (normal lava): a hole is found if it is reachable in at most that many horizontal steps measured as in the recursion (the neighbour itself is pass 1, so with depth 4 holes up to 4 cells from the cell next to the source are seen; the immediate neighbour being a hole yields distance 0 in §6.4 b).

### 7. `spreadTo(pos, targetState, direction, newFluidState)` — writing the flow

1. If the target block is a **liquid container** (§8.1) → call its `placeLiquid(level, pos, targetState, newFluidState)` and stop (result ignored). (For a waterloggable block this only succeeds for the water **source** type — in practice a flowing state never reaches here because §5.3 already refused it.)
2. Else: if the target block state is **not air** → run the fluid's `beforeDestroyingBlock(pos, targetState)`:
   - **water:** drop the block's loot as if broken without a tool or entity (server only): block-entity contents read only if the block has one, then the block's `spawnAfterBreak(…, empty tool, dropExperience = true)`. (Liquid blocks themselves drop nothing.) Random draws belong to the loot / item-entity code.
   - **lava:** emit the **fizz** level event (§10.2); **no drops**.
3. Write `newFluidState.createLegacyBlock()` at `pos` with `setBlockAndUpdate` (flags 3). (An empty `newFluidState` — not produced by §6 — would write air.)
4. **Lava override** (`LavaFluid.spreadTo`, before the generic rule): if `direction == DOWN` **and** the *current* fluid at `pos` is in `#water`: if the target block is a **liquid block** (`water`) → write **stone** at `pos` (flags 3); then (liquid block or not) emit fizz and **stop** (no lava is placed; a waterlogged block just fizzes and nothing changes). Otherwise the generic steps 1–3.

### 8. Waterlogging

**8.1 The container contract** (`LiquidBlockContainer`): `canPlaceLiquid(user, level, pos, state, fluidType)` and `placeLiquid(level, pos, state, fluidState)`; **`SimpleWaterloggedBlock`** adds `pickupBlock` and a pickup sound. Defaults of `SimpleWaterloggedBlock`:
- `canPlaceLiquid`: `fluidType == water` (the source type; flowing water and lava are refused).
- `placeLiquid`: if the state is not waterlogged **and** the fluid state's type is `water` (identity): on the server: write `state[waterlogged = true]` with `setBlockAndUpdate` (flags 3), then schedule `(water source type, pos)` with delay 5 (§1.5); return true. Else return false. (On a generation region the write is a plain proto-chunk write and the tick is stored with delay 0.)
- `pickupBlock(user, level, pos, state)`: if waterlogged: write `state[waterlogged = false]` with `setBlockAndUpdate` (flags 3); then **if the (old, still waterlogged) `state` cannot survive at `pos`** → `destroyBlock(pos, drop = true)` (a block whose survival needs water; drops, notifies); return a water bucket. If not waterlogged: return empty (nothing happens).
- Pickup sound = the water bucket-fill sound.

**8.2 Which blocks are containers.** Waterloggable (`SimpleWaterloggedBlock`): amethyst clusters / buds, barrier, all coral plants, fans and wall fans, rails (all four rail kinds), big and small dripleaf and big dripleaf stem, campfire, candles, chains, chests (single / double), conduit, copper golem statue, decorated pot, dried ghast, ender chest, hanging roots, heavy core, ladder, lanterns, leaves, light block, lightning rod, mangrove propagule and roots, multiface blocks (glow lichen, sculk vein and the other classes built on the multiface base), scaffolding, sculk sensor / calibrated sensor / shrieker, sea pickle, shelves, signs (standing, wall, ceiling hanging, wall hanging), slabs, speleothems (dripstone), stairs, trapdoors, walls, fences, iron bars / glass panes / copper bars (the cross-collision classes), and the copper grate family (`WaterloggedTransparentBlock`). Always-wet **containers that refuse all liquids** (`canPlaceLiquid` and `placeLiquid` always false): kelp, kelp plant, seagrass, tall seagrass. (Bubble column is *not* a container.) Treat the class list as data: the block's state facts say whether it has the property; the behaviours below are per class.

**8.3 Per-class overrides and the `updateShape` pattern.**
- Common pattern (§1.4): `if waterlogged → schedule (water, pos, 5)` first, then the block's own shape logic. Used by all classes of §8.2 with a `waterlogged` property. Variants:
  - **seagrass**: runs its own plant logic first and, **only if the result is not air**, schedules `(water, pos, 5)` (always-wet, so unconditional).
  - **kelp head / kelp plant** (growing-plant blocks with the "schedule fluid ticks" flag set): the schedule happens only on the paths that fall through to the default (not when the block converts head ↔ body, and not for the survival-check tick); see the plant reader.
  - **multiface** blocks: schedule first, then return air if no face is left.
  - **leaves**: schedule, then (independently) schedule its own block tick with delay 1 for the distance update.
  - **trapdoor**: schedule in `updateShape`, and also right after every `toggle` and powered update write while waterlogged (§1.8).
  - **bubble column**: §11.
- `placeLiquid` / `canPlaceLiquid` overrides: **barrier** — only when `user` is a creative-mode player (otherwise refuses); **slab** — refuses when `type = double`; **campfire** — if lit: emit the extinguish sound (server) and the douse game event, then write `waterlogged = true, lit = false` with flags 3 and schedule water (5); **candle** — if lit, extinguish through the candle's own helper (which writes the waterlogged-unlit state), else a flags-3 write; then schedule `(type, 5)`; **dried ghast** — server only: flags-3 write, schedule, play the "place in water" sound. Kelp family / seagrass: always refuse.
- `getStateForPlacement` of waterloggable blocks sets `waterlogged` iff the fluid state at the placement cell has the type `water` (the *source* type, identity); big dripleaf and its stem use "is a water source" (same result). Placement belongs to the item / block-placement milestone.

**8.4 Flowing out of a waterlogged block.** A waterlogged cell's fluid state is the water *source*. A scheduled `(water, pos)` tick on it (from §1.4 / §1.5) runs the normal `tick`: step 1 skips the state change; step 4 `spread` runs with the block's **real** state `B` and `F` = the source: downward move into the cell below if passable (§5.1 uses `B`'s collision shape for the top face; e.g. a bottom slab or a stair leaks, a full block cannot hold water since the block's own shape is BLOCK), then sideways per §6.3 with `B`'s shape. A waterlogged block therefore leaks water into adjacent air / washable cells exactly like a source block, subject to the face-occlusion test. A **falling** source (copper grate) uses `reach = 7` in §6.3.

**8.5 Replacing a fluid cell by a non-liquid block.** `removeBlock` / `destroyBlock` leave `fluid.createLegacyBlock()` (a water block from a waterlogged source) instead of air ([`set-block-flags-and-sequence.md`](set-block-flags-and-sequence.md) §1.2–1.3). **Double plants** (`preventDropFromBottomPart`, creative / player break of the upper half) write water (flags 35 = 1+2+32) or air at the lower half depending on whether the lower half's fluid is water.

### 9. Water and lava specifics

**9.1 Water.** Drop-off 1, delay 5, slope depth 4; source conversion by game rule `water_source_conversion` (default true); `canBeReplacedWith` per §5.9; no random ticks; no special spreadTo; `beforeDestroyingBlock` = drop loot (§7.2). Pickup sound = bucket fill.

**9.2 Lava** (§Inputs table): delay 30 (10 fast), drop-off 2 (1 fast), slope depth 2 (4 fast), conversion rule `lava_source_conversion` (default **false**), randomly ticking.

**9.3 Lava spread delay (`getSpreadDelay`)** — evaluated at the top of §3 step 2 for every non-source fluid tick of lava:
`delay = tickDelay` (30 / 10). **Then**, if **all** of: old state non-empty; new state non-empty; old not falling; new not falling; `height(newF at pos) > height(F at pos)` (an *increase*; the height of a state of the lava family is 1.0 when lava is above); then **draw `nextInt(4)` from the level's random**, and if the draw is **non-zero** multiply the delay by 4. The conditions are evaluated left to right and the draw happens only when all preceding ones hold (`&&` short-circuit). Water: no rule. The delay is passed to the explicit schedule of §3 step 3b; see Edge case 1 for why it is normally discarded.

**9.4 Lava ↔ water in the flow code:** (a) lava spreads **down** into water → stone + fizz (§7.4); (b) lava spreads **sideways** into water: refused (§5.9, water not replaceable sideways); (c) water spreads into lava of height ≥ 4/9 (§5.9) as a normal water write (§7.2 water branch: lava's loot is empty, **no** fizz) — the liquid-block hooks of the neighbouring lava (§10.1) then convert them; (d) lava flowing into a cell adjacent to water converts *itself* in its own `onPlace` (§10.1).

**9.5 Lava random tick** (`randomTick`; the call site is [`server-tick.md`](server-tick.md) §10.2: **two** calls per selection — see Edge case 2):
1. If fire may spread around `pos` (`canSpreadFireAround`): the game rule `fire_spread_radius_around_player` is −1 (always allowed) or some player is "close enough" to `pos` for that radius (the chunk map's own distance test; exact metric: Open question 8) — else do nothing, **without any draw**.
2. `passes = random.nextInt(3)` (0, 1 or 2).
3. **If `passes > 0`:** `t = pos`; repeat `passes` times:
   a. `t = t + (nextInt(3) − 1, +1, nextInt(3) − 1)` — the **x draw first, then the z draw**; the vertical step is always +1 (so each pass climbs one block *from the previous pass's position*).
   b. If `t` is not in a loaded chunk (`isLoaded`: inside the world's valid bounds and a chunk exists) → **return** (end).
   c. `s` = block state at `t`. If air: if `hasFlammableNeighbour(t)` (any of the six neighbours `isFlammable`, §below, in DOWN, UP, N, S, W, E order — order is unobservable) → write `fireState(t)` at `t` with `setBlockAndUpdate` (flags 3) and **return**. (If air without a flammable neighbour: continue with the next pass.) Else if `s` is in block tag `blocks_lava_fire_spread` → **return**. (Other non-air states: continue.)
4. **If `passes == 0`:** repeat **3 times**: `t = pos + (nextInt(3) − 1, 0, nextInt(3) − 1)` (x draw first); if `t` not loaded → **return**; if the block **above** `t` is air (`isEmptyBlock`) **and** `isFlammable(t)` → write `fireState(t)` — computed for **`t`** (the flammable block's position), but written at **`t + Y`** — with flags 3. (No early exit; the next iteration continues.)
- `isFlammable(p)`: if `p`'s Y is inside the build height **and** no chunk exists at `p` → false; else the block state at `p` is `ignitedByLava` (outside the build height reads void air → false).
- `fireState(p)`: soul fire if the block below `p` is in tag `soul_fire_base_blocks`; else the fire block's placement state at `p` (default fire; when the block below is neither burnable nor has a sturdy top face, the per-side fire flags are set from which of the six neighbours of `p` are burnable). The fire block reader owns the exact state.
- Draw count: 0 (spread disabled), or 1 + 2·(passes made) [+ stops early on return], or 1 + 6 for the `passes == 0` branch (3 × two draws) unless `isLoaded` fails earlier.

**9.6 Lava's other interactions:** entities inside lava (§12); lava ignites flammable blocks only through §9.5; lava destroying blocks by flowing → fizz (§7.2).

### 10. Liquid-block hooks: obsidian, cobblestone, basalt, fizz

**10.1 `shouldSpreadLiquid(pos, state)`** (called by `onPlace` and `neighborChanged`; returns whether the block should schedule its fluid tick):
- Water block: always true.
- **Lava block:** `soulSoil` = the block at `pos − Y` is soul soil. For `d` in **DOWN, SOUTH, NORTH, EAST, WEST** (in that order), `n = pos + d.opposite` — i.e. the probed neighbours are **UP, NORTH, SOUTH, WEST, EAST** (the cell below is **never** probed):
  1. if the fluid state at `n` is in `#water`: write **obsidian** if the lava cell's fluid is a **source** (level 0 block), else **cobblestone**, at `pos` with `setBlockAndUpdate` (flags 3); emit fizz; **return false**.
  2. else if `soulSoil` and the block at `n` is **blue ice**: write **basalt** (flags 3); emit fizz; return false.
  (Per probe the water test comes first; the first probe in the list order that satisfies either test decides.)
  Otherwise true.
- The write inside the hook is re-entrant: from `onPlace` it is a nested `setBlock` whose full sequence runs inside the outer one; the outer write then notices its state was replaced (step 2.3 of the set-block spec) and skips its remaining steps. A converted position has no fluid and **schedules nothing**.

**10.2 The fizz event** = level event **1501** with data 0 at the position, broadcast to players within **64 blocks**. The server draws no random; sound and smoke particles are client-side. Emitted by: §10.1 (after the write), §7.2 (lava destroys a non-air block), §7.4 (lava onto water).

**10.3 `tick` (block tick) of a liquid block:** only bubble columns (§11.1). **`pickupBlock`** (bucket): if `level == 0` (a source): write air with flags **11** (1+2+8) and return the fluid's bucket; else return empty.

### 11. Bubble columns (fluid-related part)

The bubble column block (`drag` boolean; default drag-down true) is *always-wet* (water source falling false), replaceable, no collision, no shape, pickup = water bucket (writes **air**, flags 11). Soul sand and magma blocks have **no update hooks** here: they only matter through the tags `enables_bubble_column_push_up` / `enables_bubble_column_drag_down`; both are `postProcess` "above" blocks, i.e. during generation the cell above them is marked for post-processing (§13.2) so that generated underwater soul sand / magma get columns.

**11.1 Liquid-block side.** `occupiable(state)` = fluid state ∈ `#bubble_column_can_occupy` (type `water`) ∧ source ∧ full (amount 8) — i.e. a plain water block with level 0 (waterlogged blocks never get here because only liquid-block hooks call it).
- `onPlace` / `neighborChanged` (after the spreading check of §1): if `occupiable(state)`: `below` = block state at `pos − Y`; if `below` is in `enables_bubble_column_drag_down` **or** `…_push_up` → schedule a **block** tick `(water block, pos)` with delay **20**.
- `updateShape` from direction **DOWN** with `occupiable(state)`: same check on the **neighbour (below) state** → schedule block tick delay 20.
- Block `tick` of the water block (also called by post-processing, §13): if `occupiable(state)`: `updateColumn(column block, level, pos, below state)` (§11.2).

**11.2 `updateColumn(columnBlock, level, occupyAt, [occupyState], belowState)`** (the occupy state defaults to the current state at `occupyAt`):
1. `canOccupy(s)` = `s` is the column block, **or** (`s`'s fluid ∈ `#bubble_column_can_occupy` ∧ `s`'s block is a **liquid block** ∧ source ∧ amount ≥ 8). If `occupyState` can't be occupied → nothing.
2. `col = columnState(belowState, occupyState)`: if `belowState` is the column block → **that same state** (copy of the drag flag below); else if `belowState` ∈ `enables_bubble_column_push_up` → column with **drag false**; else if `belowState` ∈ `enables_bubble_column_drag_down` → column with **drag true**; else → (`occupyState` is a column ? plain water block level 0 : `occupyState` unchanged).
3. `setBlock(occupyAt, col, flags 2)` (result ignored). Then for `p = occupyAt + Y, +2Y, …` while `canOccupy(state at p)`: `setBlock(p, col, flags 2)`; **if that write returns false (identical state / out of bounds) → return**. So a column fills upward through every connected occupiable water source cell. Flags 2: clients notified, **no neighbour notification**, but shape updates and `onPlace` run (so a restored water block re-runs §1.1; bubble columns trigger their neighbours' shape updates, §11.3).

**11.3 Bubble-column block hooks.**
- `updateShape` (any direction): schedule `(water, pos, 5)` fluid tick; then if the column **cannot survive** (the block below is neither a column nor in either enabler tag) **or** the direction is DOWN, **or** the direction is UP and the neighbour above is **not** a column but `canOccupy` → schedule a block tick `(column, pos)` with delay **5**. Returns the state unchanged (survival failure does *not* delete it directly).
- Block `tick`: `updateColumn(column, level, pos, current state, block below)` — if the block below is no longer an enabler the column cells convert to water blocks (step 2 last case) from the bottom up in one call.
- Fluid tick of the column cell: normal water source tick (§3) with `B` = the column state (EMPTY collision shape).
- Entity hooks (`entityInside`, `onAboveBubbleColumn`, `onInsideBubbleColumn`) are entity-milestone; the "nothing above" test = the block above has an empty collision shape ∧ empty fluid.

### 12. Pure fluid queries other milestones need (hooks and `getFlow`)

Entity milestone (not specified here): `Entity.updateFluidInteraction` / `EntityFluidInteraction.update` (fluid height, current push, `isInWater`, `isUnderWater`, swimming), `Fluid.entityInside` (water: extinguish effect; lava: clear freeze, ignite, then lava damage), `LiquidBlock.getCollisionShape` (standable-fluid shapes for contexts that stand on fluid; empty with the empty context), `BubbleColumnBlock.entityInside`, `FluidState.getShape / getAABB / getHeight`, `FluidState.getHeightForCamera`, `ShapeContext.canStandOnFluid`. The world-side data they read: §Inputs (height, amounts) and:

**`getFlow(level, pos, fluidState)` → Vec3 (pure, deterministic):** `flowX = flowZ = 0`. For each horizontal `d` (N, E, S, W) with neighbour fluid state `fn` at `pos + d`: only if `fn` is **empty** or of this family: `h = fn.ownHeight`; `dist = 0`; if `h == 0` (empty neighbour): if the neighbour's block is **not** in `blocks_fluid_flow`, let `fb` = fluid state at `pos + d − Y`; if `fb` is empty-or-this-family and `fb.ownHeight > 0`: `dist = own.ownHeight − (fb.ownHeight − 0.8888889)`. Else if `h > 0`: `dist = own.ownHeight − h`. If `dist ≠ 0`: `flow += d.step · dist` (x, z). Then if the state is **falling**: for each horizontal `d` (N,E,S,W): if `isSolidFace(pos + d, d)` or `isSolidFace(pos + d + Y, d)` → `flow = normalize(flow) + (0, −6, 0)` and **stop the loop**. Return `normalize(flow)` (zero vector stays zero). `isSolidFace(p, d)`: false if the fluid at `p` is this family; true if `d == UP`; false for ice-type blocks; else the block's face `d` is **sturdy** (full support type). (`f32` arithmetic where the game uses float.)

### 13. World generation: fluid marks, fluid ticks, promotion

1. **During generation nothing flows.** Fluid ticks are never executed on a proto chunk; liquid blocks' `onPlace` / `neighborChanged` / `updateShape` do not run for generation-region writes ([`block-hooks.md`](block-hooks.md) §4.5, [`set-block-flags-and-sequence.md`](set-block-flags-and-sequence.md) §4). Two kinds of record are produced instead:
   - **Scheduled fluid ticks** (stored with **delay 0**; the requested delay and sub-tick order are dropped; priority kept) by: the *spring* feature (after writing the fluid block, `(its fluid type, pos, 0)`, once per placed spring), the *geode* feature (for every non-empty fluid cell face-adjacent to a cell that a crack carved to air: `(fluid type, neighbour, 0)`, directions in the geode's own direction list order), *structure pieces* (`StructurePiece.placeBlock`: after the write, if the cell's fluid is non-empty `(type, pos, 0)`; the nether-fortress lava well schedules `(lava, pos, 0)` for its own cell), and waterlogged-structure `placeLiquid` (the container's own tick request, delay 5 → stored as 0). An imposter (already `full`) chunk drops them.
   - **Post-process marks** (packed offsets in the chunk's `PostProcessing` section lists, [`scheduled-ticks-and-block-events.md`](scheduled-ticks-and-block-events.md) §5.5): the noise filler marks every cell it writes whose fluid is non-empty **iff the aquifer says "schedule fluid update"** for that cell ([`../worldgen/aquifers-and-ore-veins.md`](../worldgen/aquifers-and-ore-veins.md)); the carver step does the same for its aquifer substance writes and for the grass-adjacent dirt top material; the surface system's block-column writes mark every non-empty-fluid cell they write; the terrain-blending border pass marks every non-empty-fluid cell (and leaves) of the border columns; `Feature.markAboveForPostProcessing` marks up to two cells above lake / disk / fallen-tree placements until it meets air; soul sand and magma block mark the cell above themselves (§11).
2. **Promotion** (first block-ticking promotion of a chunk): post-processing runs **before** the saved ticks are unpacked (see [`scheduled-ticks-and-block-events.md`](scheduled-ticks-and-block-events.md) §5.3 / §5.5): for each marked position in section order and insertion order: if the fluid state is non-empty → call the **fluid tick hook immediately** (§3, with real scheduling from now on: onPlace-driven ticks use the normal delays, and flowing cells at an edge start flowing right here); then if the block is a **liquid block** → call its block `tick` (bubble columns, §11.1); else recompute from neighbour shapes and write with flags 276. The fluid ticks stored with delay 0 are unpacked afterwards with trigger = `gameTime + 0` and run on the next level tick's fluid phase (so each such cell ticks **twice** in quick succession: once in post-processing, once as the stored tick, unless the first run's writes changed its fluid type).
   Because the pending ticks seed the container's `(type, pos)` set, a fluid tick that post-processing (or its `onPlace` side effects) tries to schedule for a pair that is **still pending** is rejected; ticks for other pairs enter the queue *before* the unpack (so they sort after the unpacked ones on equal trigger, [`scheduled-ticks-and-block-events.md`](scheduled-ticks-and-block-events.md) §5.3.4).
3. A feature writing water or lava **without** any mark or tick leaves a static pool that only starts to flow when a neighbour update reaches a liquid block (or a player / game update touches it).

### 14. Other consumers of fluid state (hooks only; owned by other readers)

Bucket item (`emptyContents` / pickup: the place position is the clicked block itself when it is a container and the bucket is water, else the adjacent cell; for a *water* bucket where the dimension attribute `water_evaporates` is true the water is **not** placed — sound + smoke, with two `nextFloat` draws from the level random, in order `a − b`, for the sound pitch — otherwise a container takes `placeLiquid(flowing fluid's **source** state)`, else a replaceable / air cell is destroyed with drops (server, only if it is replaceable and not a liquid) and the fluid's legacy block with `level 0` is written with flags **11**; pickup calls the block's `pickupBlock`), dispensers, sponge (breadth-first search ≤ 65 cells, radius 6, picks up / clears water, wet-sponge write flags 2), concrete powder, farmland moisture, sugar cane / lily pad / frogspawn support, cauldrons, dripstone drips, coral death, fire, sculk, pointed dripstone, sea pickles, conduits, snow golem / melting rules. Their world-side effects go through the writes and hooks above.

## Edge cases and known quirks

1. **Lava's "×4" spread delay is normally dead.** In §3 step 3b the new block is written **first** (its `onPlace` schedules the same `(fluid, pos)` with the plain 30 / 10 delay) and the explicit reschedule with the spread delay comes **after**, so it is dropped as a duplicate. Only the **random draw** (and the `nextInt(4)` stream advance) is observable. **[oracle]** Probe in Open questions 1.
2. **Lava random-ticks twice.** The lava *block* is randomly ticking (its hook forwards to the fluid's random tick) **and** its fluid is randomly ticking, so each random-tick selection of a lava cell runs §9.5 **twice** back to back, with independent draws (the fluid hook uses the pre-tick state captured before the block hook). **[oracle]**.
3. **Stale locals after a self-conversion.** If the write of §3b makes the lava cell convert itself (§10.1), `spread` still runs from the old lava `B` / `F` (§3 step 4), and the dropped reschedule of step 3b is a stale-type tick. Spreading from a cell that is now cobblestone / obsidian / basalt can still write lava into neighbours.
4. **Search order vs apply order differ:** distances / `lowest` are evaluated in N, E, S, W order; the final writes go N, S, W, E (§6.3). A passable but non-replaceable neighbour can reset the map (§6.4 step 2c).
5. **Flowing water never waterlogs.** Only the *source* water type satisfies a container, so a waterloggable block is filled only by a source (from conversion §4.2, a bucket, a structure fill, or a source cell's tick whose `getNewLiquid` returns a source). Flowing water around a slab / stair does not enter it; it flows around.
6. **A flowing cell directly above a hole never spreads sideways** (§6.2 step 3) unless the downward move did not happen *and* the cell is a source (sources always reach step 3's sideways branch when the downward move fails). When the downward move **did** happen, sideways spreading follows only if at least 3 of the 4 horizontal neighbours are sources of this family (§6.2 step 2a).
7. **`falling` flag semantics:** `falling` appears only on cells produced by §4 step 3 (always amount 8) and on copper-grate water; it forces `reach = 7` in §6.3 for any fluid (lava included) and disables the lava ×4 draw.
8. **Source conversion needs only the cell below** (solid, or a source of the same family); it does not look at the neighbours' walls beyond `canPassWall`. Two sources side by side over a solid floor therefore create a third source in the cell between them (when the rule is on). Lava: off by default.
9. **`isSolid` is the legacy solid flag** (`state-facts` §2.13), not "full block": e.g. glass, slabs and stairs may or may not count; use the tabulated fact.
10. **Pickup writes air, not water,** for liquid sources and bubble columns (flags 11); the flow code then refills.
11. **`removeBlock` / `destroyBlock` of a waterlogged block leave water**, not air ([`set-block-flags-and-sequence.md`](set-block-flags-and-sequence.md) Edge 5).
12. **Tick type identity:** a pending `(water, pos)` tick is ignored if the cell's fluid is `flowing_water` when it fires (and vice versa); a cell changing between source and flowing between scheduling and firing loses the tick, but the write that changed it scheduled the new type.
13. **Post-processing runs both hooks** for liquid cells (fluid tick, then block tick) and only the fluid tick for waterlogged non-liquid cells, whose block is then re-derived from neighbour shapes (§13.2).
14. **Fluid spread reads collision shapes with the empty context**, so lava / water (EMPTY collision) are transparent to the wall test, while waterlogged blocks block or leak by their real collision shape.
15. **`getNewLiquid` returns `flowing(8, falling)` from above even when the fluid above is a lone source or itself falling,** and does not check `canHoldFluid` for the cell itself (the caller does, in §6.4 2a / §6.2).

## Constants and IDs

Fluid ids (ZBF1 `fluid_id` order): empty, flowing_water, water, flowing_lava, lava. Tick delays: water 5, lava 30 (fast 10), bubble-column block tick 5 (shape-update) / 20 (liquid scheduling), liquid water-source tick 5. Drop-off: water 1, lava 2 (fast 1). Slope depth: water 4, lava 2 (fast 4). Reach cap for falling = 7. Slope "no hole" value 1000. Amount range 1–8 (flowing), source 8; height = amount / 9 (f32); lava replaceable threshold height 0.44444445 (f32) = amount ≥ 4. Legacy `level` 0–15; liquid-state index `min(level, 8)`. Level event **1501** (fizz), broadcast radius **64**. Write flags: flow / conversion / waterlog **3**; liquid pickup **11**; bubble column **2**; post-process fix-up **276**. Game rules `water_source_conversion` (true), `lava_source_conversion` (false), `fire_spread_radius_around_player`. Attribute `gameplay/fast_lava` (default false; nether true). Random draws: lava spread-delay `nextInt(4)`; lava random tick `nextInt(3)`, `nextInt(3)` pairs; both from the level random. Tags: `washed_away_by_fluids`, `blocks_fluid_flow`, `blocks_lava_fire_spread`, `enables_bubble_column_drag_down` (magma), `enables_bubble_column_push_up` (soul sand), `cannot_support_kelp`, `soul_fire_base_blocks`, fluid tags `water`, `lava`, `bubble_column_can_occupy`.

## Worked examples (sanity checks for the implementer)

1. **Water source on a flat stone floor, air around.** t₀: source block placed with flags 3 → `onPlace` schedules `(water, pos, 5)`. t₀+5: tick → `spread`: below is stone (not holdable) → skip; source → `spreadToSides`, reach 7; `getSpread`: all four neighbours passable, `getNewLiquid` = flowing 7; no holes within depth 4 → each `distance = 1000`; `1000 < 1000` false, `1000 ≤ 1000` true → all four added; writes in order N, S, W, E, each water level-1 block (`level = 8 − 7 = 1`) schedules its own tick at t₀+10. Next ring: reach 6, … until amount 1 (level 7), whose reach is 0.
2. **Water source one block above a hole at a corner.** The slope search finds the hole at distance d: only the direction(s) with the smallest `distance` (hole next to the cell = 0) survive in the map; a hole straight below the source takes the downward branch instead (§6.2).
3. **Lava source flowing over a ledge in the overworld:** delay 30, drop-off 2, depth 2; in the Nether: delay 10, drop-off 1, depth 4 — evaluated for the dimension of the cell.
4. **Lava meets a water source on its side:** the lava cell's `neighborChanged` fires after the water write: a lava **source** becomes obsidian, flowing lava becomes cobblestone (the water above or beside decides; water only below does not).

## Oracle probe points

- Fluid tick: `FluidState#tick(ServerLevel, BlockPos, BlockState)` (framework entry), `FlowingFluid#tick(ServerLevel, BlockPos, BlockState, FluidState)` (public), `#getNewLiquid`, `#spread`, `#getSpread`, `#getSlopeDistance`, `#spreadTo`, `#canPassThrough…` (private: log through the public wrappers and the writes), `LavaFluid#getSpreadDelay(Level, BlockPos, FluidState, FluidState)`, `LavaFluid#randomTick`, `FluidState#randomTick(ServerLevel, BlockPos, RandomSource)`.
- Scheduling: `ScheduledTickAccess#scheduleTick(BlockPos, Fluid, int)` and `LevelTickAccess#schedule` (log `(gameTime, type, pos, delay, accepted)`), `LevelChunk#getFluidTicks()` → `LevelChunkTicks#getAll()`, `ServerLevel#getFluidTicks()`; compare the accepted / rejected sequence per tick.
- Block hooks: `LiquidBlock#onPlace`, `#neighborChanged`, `#updateShape`, `#tick`, `#pickupBlock`, `SimpleWaterloggedBlock#placeLiquid` / `#pickupBlock` / `#canPlaceLiquid`, `BubbleColumnBlock#updateColumn(Block, LevelAccessor, BlockPos, BlockState)` and `#updateShape` / `#tick`; wrap `Level#setBlock(BlockPos, BlockState, int, int)` and log every write with its flags.
- Events: `Level#levelEvent(int, BlockPos, int)` (id 1501).
- Randomness: `Level#getRandom()` draws (count and values) around a lava tick / random tick; level `random` value dump.
- State: `FluidState#getAmount`, `#isSource`, `#getValue(FlowingFluid.FALLING)`, `#getHeight(BlockGetter, BlockPos)`, `#getFlow(BlockGetter, BlockPos)`, `LiquidBlock#getFluidState(BlockState)`; `Fluids#WATER` etc.
- Game data: `GameRules#WATER_SOURCE_CONVERSION` / `LAVA_SOURCE_CONVERSION`, `EnvironmentAttributes#FAST_LAVA`, tags `BlockTags#WASHED_AWAY_BY_FLUIDS`, `#BLOCKS_FLUID_FLOW`, `#BLOCKS_LAVA_FIRE_SPREAD`, `#ENABLES_BUBBLE_COLUMN_DRAG_DOWN` / `_PUSH_UP`.
- World: a "drive" harness: build a scenario (blocks around a source), advance `ServerLevel#tick(BooleanSupplier)`, dump all blocks in a box and the fluid-tick queue after each tick, compare with Zenith tick by tick. Include the post-processing path: `LevelChunk#postProcessGeneration(ServerLevel)`.

## Open questions

1. **[oracle] Lava ×4 delay is discarded (Edge 1).** Probe: in the overworld and the Nether, pour a lava source onto a slope and log every `scheduleTick(…, lava types, …)` call with its delay, `accepted` flag and the `nextInt(4)` draws; confirm that the explicit reschedule after the write is always rejected and that the level random still advances.
2. **[oracle] Double random tick of lava (Edge 2).** Probe: wrap `FluidState#randomTick` and `LiquidBlock#randomTick`; count calls and `Level#getRandom` draws per selection for one lava cell.
3. **[oracle] Stale-state spread after a self-conversion (Edge 3).** Probe: arrange flowing lava with a water source placed adjacent in the same tick as the lava tick fires; log the block at the cell before the `spread` call, the `spreadTo` writes that follow and the final world.
4. **[oracle] Spread-map apply order (N, S, W, E) vs search order (N, E, S, W).** Probe: log `spreadTo` calls and the `getSpread` result on a flat plane (all four) and on a plane with one hole.
5. **[oracle] Slope-search depth and "return `pass` on first hole".** Probe: put a hole at each distance 0–5 in each direction from a source for water, normal lava and Nether lava; log the chosen directions.
6. **[oracle] Loot / item-entity random draws for blocks destroyed by water.** Which stream (level random vs the loot context's seed) and how many draws before the item entity's own `nextDouble` offsets (e.g. a flower or torch washed away). Probe: log `Level#getRandom` before / after a flowing-water tick that destroys a torch.
7. **[oracle] `isLoaded` for the lava fire walk.** Whether the chunk must be `full` / entity-ticking or merely present at any status. Probe: lava at a chunk border with the neighbouring chunk at `features` status; log whether `pos + (dx, 1, dz)` in that chunk returns.
8. **[oracle] `fire_spread_radius_around_player` distance metric** (`anyPlayerCloseEnoughTo`): horizontal-only vs 3D, squared vs linear, rounding. Probe with players at known offsets and rule values 0, 1, 128.
9. **[oracle] Structural vs identity BLOCK shape in `canPassWall` (§5.1).** Probe for every block state: compare the collision shape's identity against the BLOCK singleton and the result of `canPassThroughWall` towards an air cell; confirm they agree with the shape catalogue's "full cube" classification.
10. **[oracle] Copper-grate falling source** (§8.3 / Edge 7): confirm `getFluidState` is the falling source and that its tick uses `reach = 7`. Probe: place a waterlogged grate next to air and log the written levels.
11. **[oracle] Post-process double tick (§13.2):** confirm both the immediate post-process fluid tick and the stored delay-0 tick fire, in that order, for a spring / structure fluid cell. Probe: generate a chunk with a spring and log fluid-tick hook calls per `gameTime`.
12. **[oracle] `fast_lava` attribute evaluation for non-vanilla dimension types** (`getDimensionValue` with timeline / modifier layers): confirm it is the plain dimension-type value for all vanilla dimensions.
13. (implementer) Fluid states may be represented by a single byte (type 0–4, falling bit, amount 1–8) with an interning table of 37 states; identity comparisons of the game are equality of that triple.
14. (implementer) The search-context cache of the game is unobservable (§6.5); implement the search without it or with any cache that keys on the relative position.
15. (unresolved, other readers) Exact loot behaviour of §7.2, the fire block placement state of §9.5, the extinguish helper of candles / campfires (§8.3), bucket / dispenser / sponge / concrete-powder reactions (§14) and entity fluid physics (§12) are not specified here.

## Parity status

Not yet run.
