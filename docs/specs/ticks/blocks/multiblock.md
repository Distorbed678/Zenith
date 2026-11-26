# Ticks: block reactions — multi-block structures: doors, beds, tall plants, speleothems, partners (26.3)

- Verified by suites: `block-updates`
- Sources: local reference reading of the 26.3 block classes (2026-11-26, reader session): the door, the bed base class and both bed classes, the double-plant family, the speleothem base class
  with its two subclasses (pointed dripstone, sulfur spike), the shelf side-chain interface, the piston head. Everything is in our own words.
- Related specs: [`README.md`](README.md), [`plants.md`](plants.md) §3 (double plants, pitcher crop), §6 (growing plants), [`../redstone-components.md`](../redstone-components.md) §8.6 (door redstone), [`../pistons.md`](../pistons.md) §7 (piston head),
  [`../neighbor-and-shape-updates.md`](../neighbor-and-shape-updates.md), [`../set-block-flags-and-sequence.md`](../set-block-flags-and-sequence.md), [`../fluids.md`](../fluids.md) §8.
- Scope: how the two (or more) parts of a multi-block structure keep each other consistent through **shape updates** and what happens when one part disappears. Item placement writes the partner in `setPlacedBy`
  (listed per block); loot is chosen by the block's loot table (data) from the state of the removed part.

## Summary

Partner-based blocks use the same trick: each part reacts to the shape update that arrives **from its partner's side** (a vertical or fixed horizontal direction), and either **mirrors the partner** or **turns to air**. Because a shape update that
returns air becomes `destroyBlock(…, drops = true)` with flags 3 (neighbor-and-shape-updates §3.3d), breaking one part always removes the other part within the **same** update chain, and the survivor's neighbours see a normal removal.

## Behavior (ordered rules)

### 1. Doors (`DoorBlock`, all door blocks; props `FACING`, `HALF`, `HINGE`, `OPEN`, `POWERED`)

1. **`canSurvive`**: `LOWER`: the block below `isFaceSturdy(UP)` (FULL support). `UPPER`: the block below is **this same door block** (identity of the block type; any state).
2. **`updateShape`** (`half`, `dir`, `nb` = the neighbour's state):
   - If `dir` is **not vertical**, **or** (`half == LOWER`) ≠ (`dir == UP`) (i.e. the update does *not* come from the partner's side): `half == LOWER && dir == DOWN && !canSurvive` → `AIR`; else the state is returned unchanged
     (no other direction, and no UP-to-lower / DOWN-to-upper update has any effect).
   - Otherwise (the update comes from the partner's side: lower half with `dir == UP`, upper half with `dir == DOWN`): if `nb` is a **door block of any type** (`instanceof` the door class) **and** `nb.HALF != half` → return
     **`nb` with its `HALF` replaced by this half** — i.e. **this half becomes a copy of the partner**: block type, `FACING`, `HINGE`, `OPEN`, `POWERED`, everything. Else → `AIR`.
   Consequences: (a) when a door is opened / closed by a write that changes only one half, the other half follows through this rule (that is how both halves stay equal; the write of one half with bit 1/2 flags then triggers the partner's shape update);
   (b) an iron door half next to a wooden door half of the other half is *converted* to the partner's block type; (c) a lone half whose partner is not a door (or is a door with the same half) turns to air.
   Worked chain: the block under a lower half is removed → the lower half gets the `dir == DOWN` update → not the partner side → `!canSurvive` → `AIR` (destroyed with drops); that removal's own shape updates reach the upper half with `dir == DOWN`
   → partner side → the neighbour (now air) is not a door → `AIR`. Removing only the upper half: the lower half gets `dir == UP` → partner side → air is not a door → `AIR`.
3. `neighborChanged` (redstone) and the open/close writes (`setOpen` flags 10 = 2 + 8): see redstone-components §8.6. `setPlacedBy`: `setBlockAndUpdate(pos + UP, state with HALF = UPPER)` (flags 3; the state copies every other property of the placed lower half including `OPEN`/`POWERED`/`HINGE`).
4. Placement note: requires `y < maxY` and `canBeReplaced` at `pos + UP`; `POWERED = OPEN = hasNeighborSignal(pos) || hasNeighborSignal(pos + UP)`; hinge from the placement geometry (item side).
5. `setOpen` writes with flags **10** = bit 2 (send to clients) + bit 8 (client re-render hint). Bit 1 is clear, so there is **no neighbour notification** (no `neighborChanged` around the door) and no `affectNeighborsAfterRemoval`; bit 16 is clear, so
   the six shape updates run, and the partner half mirrors the new state through §1.2. (Powered-state writes of the redstone path use flags 2: redstone-components §8.6.)

### 2. Beds (`AbstractBedBlock`: `BedBlock` × 16 colours, `StrawBedBlock`; props `FACING`, `PART` ∈ {FOOT, HEAD}, `OCCUPIED`)

1. The partner direction of a part: `FOOT` → `FACING`; `HEAD` → `FACING.opposite` (the head lies one block in the facing direction of the foot).
2. **`updateShape`** (**only** for `dir == partner direction`; everything else returns the state): if `nb` is **this same block** (block identity) **and** `nb.PART != state.PART` → `state.setValue(OCCUPIED, nb.OCCUPIED)` (copy the partner's occupied flag; **FACING is not compared** — a differently-oriented partner of the same block still counts);
   else → `AIR`. No `canSurvive`; beds survive on any floor (only the partner counts).
3. `setPlacedBy`: `setBlockAndUpdate(pos + FACING, placed state with PART = HEAD)` (flags 3) — super is called first.
4. Beds have no `neighborChanged`. The "bad respawn point" explosion / straw-bed destruction on leave are interaction side (`destroyOnUse`, `destroyOnLeave`: bed: removeBlock for both parts then explosion; straw bed: `setBlockAndUpdate(pos, air)` for the half that was slept in).
5. Placement note: the head cell must be replaceable and inside the world border.

### 3. Double plants and the pitcher crop

Fully specified in [`plants.md`](plants.md) §3 (`DoublePlantBlock.updateShape` / `canSurvive`, pitcher crop single-vs-double rule, `setPlacedBy`, waterlogged copies). Quick reference of the partner test (same as doors but **AIR instead of mirroring** and the partner must be this same block):
`dir` vertical **and** the update comes from the partner side (lower+UP, upper+DOWN) **and** not (`nb` is this block with the other half) → `AIR`.

### 4. Speleothems (`SpeleothemBlock`: `pointed_dripstone`, `sulfur_spike`; props `TIP_DIRECTION` ∈ {UP, DOWN}, `THICKNESS` ∈ {TIP, TIP_MERGE, FRUSTUM, MIDDLE, BASE}, `WATERLOGGED`)

Helper predicates: `isSpeleothem(state, dir)` := the block is in `tag#speleothems` **and** its `TIP_DIRECTION == dir`. `stalagmite` := `isSpeleothem(state, UP)`; `stalactite` := `isSpeleothem(state, DOWN)`.
`isTip(state, includeMerge)` := in `tag#speleothems` and `THICKNESS == TIP` (or `TIP_MERGE` when `includeMerge`).

1. **`canSurvive(state)`** with tip direction `T`: let `behind = pos + T.opposite`; true iff `behind.isFaceSturdy(T)` (the face of the behind block that points toward the speleothem, i.e. in direction `T`, FULL support)
   **or** (`behind` is a speleothem with `TIP_DIRECTION == T` **and** is **this same block type**).
2. **`updateShape`** (`dir`, `nb`) — in order:
   a. If `WATERLOGGED` → `waterTick`.
   b. If `dir` is horizontal → return the state.
   c. `T = TIP_DIRECTION`. If `T == DOWN` **and** a **block tick of this block is already scheduled at `pos`** (`hasScheduledTick(pos, this)`) → return the state (no thickness recompute).
   d. If `dir == T.opposite` (the support side) **and** `!canSurvive`: `scheduleTick(pos, this, T == DOWN ? 2 : 1)`; return the state **unchanged** (the block is removed by its tick, not now).
   e. Else: `merge = (state.THICKNESS == TIP_MERGE)`; `return state with THICKNESS = calculate(level, pos, T, merge)`.
3. **`calculate(pos, T, merge)`**: `B = T.opposite`; `front = state at pos + T`.
   - If `front` is a speleothem with `TIP_DIRECTION == B` **and** `front` is this same block: result = `TIP` if (`!merge` **and** `front.THICKNESS != TIP_MERGE`) else `TIP_MERGE`.
   - Else if `front` is **not** a speleothem with `TIP_DIRECTION == T`: `TIP`.
   - Else (front continues the column): if `front.THICKNESS` is `TIP` or `TIP_MERGE` → `FRUSTUM`; else look at `back = state at pos + B`: if `back` is **not** a speleothem with `TIP_DIRECTION == T` → `BASE`, else `MIDDLE`.
   (The first branch only counts the same block type; the others use the tag, so a pointed dripstone and a sulfur spike column mix.)
4. **`tick`**: if the state is a **stalagmite** and `!canSurvive` → `destroyBlock(pos, drops = true)` (flags 3). **Else** (a stalactite, or a stalagmite that still survives): *spawn falling stalactite*:
   `fallPos = pos`, `fallState = state`; while `fallState` is a stalactite: `entity = FallingBlockEntity.fall(level, fallPos, fallState)` which **first** creates the entity object (state copy with `WATERLOGGED = false` if the block has it), **then**
   `setBlockAndUpdate(fallPos, fallState.getFluidState().createLegacyBlock())` (water if it was waterlogged, else air; flags 3), **then** `addFreshEntity`; if `isTip(fallState, includeMerge = true)`: set the entity's hurt parameters
   (`damagePerDistance = max(1 + pos.y − fallPos.y, 6)` as float × 1.0, max damage 40) and **stop**; else `fallPos` moves **DOWN** and `fallState` is re-read **after** the previous write (so the earlier removal's shape-update/neighbour effects are already visible).
   Note a stalactite whose tick runs but which has since regained support still falls (the tick does not re-check `canSurvive` for stalactites).
5. Growth, dripping, cauldron filling and tridents are tick / entity side. Fall delays: stalactite support loss → tick after **2**, stalagmite → **1** (§4.2d).
6. `onPlace`: none. `affectNeighborsAfterRemoval`: none.

### 5. Pistons: the head / base pair

`PistonHeadBlock` (the extended arm) and `piston_base` partner rules are in [`../pistons.md`](../pistons.md) §7: head `updateShape` returns `AIR` when the update comes from the base side (`dir.opposite == FACING`) and `canSurvive` fails;
`canSurvive` = the block behind is a fitting base **or** a moving-piston block with the same `FACING`; head `affectNeighborsAfterRemoval`: if the block behind (at `pos + FACING.opposite`) is the fitting base → `destroyBlock(basePos, drops = true)`;
head `neighborChanged`: if `canSurvive`, forward a Simple `neighborChanged` to the base. (Listed here only as the partner pattern; do not duplicate.)

### 6. Other two-cell structures with a fixed partner

| structure | rule file |
|---|---|
| chests (single / left / right) | [`connections.md`](connections.md) §Chests |
| shelves side chain (up to 3 powered shelves) | [`connections.md`](connections.md) §Shelves |
| kelp / vines / bamboo stacks | [`plants.md`](plants.md) §6, §7 |
| big dripleaf leaf + stem column | [`plants.md`](plants.md) §5, §7 |
| nether portal frame interior | [`misc-reactions.md`](misc-reactions.md) §Portal |

## Edge cases and known quirks

1. **Doors copy the partner.** Writing one half with a different state (e.g. a command or a piston-moved half) makes the other half adopt it at the next shape update: the rule never checks that the partner is the *same* door block (§1.2).
2. **Beds ignore FACING of the partner**, so two beds of the same block whose halves line up in an inconsistent orientation still pair.
3. **Doors break with drops on both halves** (the loot table of the door gives the item only for the lower half); the AIR result goes through `destroyBlock(drops = true)` (flags 3) for the second half.
4. **A speleothem with a tick already pending** (stalactite, §4.2c) does not recompute thickness; the pending tick then drops the whole stalactite from `pos` downward, **ignoring** whether it regained support (§4.4).
5. **`FallingBlockEntity.fall` removes the block before the entity is added**; the write notifies neighbours (flags 3), so shape updates of the neighbouring stalactite parts (recomputing thickness above) run **between** the per-block falls.
6. **Door `canSurvive` for the upper half uses block identity** (`belowState.is(this)`); an iron door above a wooden door lower half is not "surviving" but the partner rule (§1.2) is evaluated first and converts or removes it.

## Constants and IDs

Door shape-update mask: flags 10 for open/close writes; `setPlacedBy` writes flags 3. Speleothem fall delays 2 / 1; fall-damage: `max(1 + dy, 6)`, cap 40. Tags: `speleothems`.

## Oracle probe points

- `DoorBlock#updateShape`, `AbstractBedBlock#updateShape`, `SpeleothemBlock#updateShape`, `#tick`, `FallingBlockEntity#fall(Level, BlockPos, BlockState)`, `Level#destroyBlock`. Log `(pos, dir, nb, returned)`.
- Probes: wooden lower door next to iron upper door (and vice versa); bed head replaced with a bed of another colour; stalactite column with the root removed vs the middle removed; a stalactite column whose root is removed and immediately replaced in the same tick.

## Open questions

1. **[oracle] Door partner copy with mismatched block types** (§1.2): confirm the converted half keeps the partner's `OPEN` / `POWERED` and that no extra write flags beyond the shape-update rules apply.
2. **[oracle] Bed partner with different `FACING`** (§2.2): confirm the pairing and that `OCCUPIED` is copied.
3. **[oracle] Speleothem stalactite re-support** (§4.4): confirm that a stalactite tick falls even when `canSurvive` has become true again.
4. **[oracle] Door write flags** (§1.5): confirm the exact flag value used by `setOpen` (10) and that the partner follows via shape update in the same chain.

## Parity status

Not yet run.
