# Lighting: per-chunk sky-light sources (`ChunkSkyLightSources`), 26.3

- Verified by suites: `light-engine` (section `sky-sources`: per-column lowest-source dumps after generation, after load and
  after single block changes)
- Sources: local reference reading of the 26.3 decompile (2026-11-25), written in our own words.
- Related specs: [`light-engine.md`](light-engine.md) (the sky engine consumes this), [`chunk-steps.md`](chunk-steps.md)
  (when it is filled and updated), [`data-layers.md`](data-layers.md), `../blocks/state-facts.md` §3.2 (light dampening),
  `../blocks/shapes.md` §4.4 (`faceShapeOccludes`).

## Summary

Every chunk (proto chunk and level chunk, server and client) owns a 16×16 table of integers, one per column: the
**lowest source Y** — the Y of the lowest block of the column that receives full sky light 15 straight from above without any
attenuation. Everything at or above that Y in the column is a sky source; below it light comes only from sideways spread.
The table is **derived data**: it is never saved or sent; it is rebuilt from the block states when a chunk reaches the
`initialize_light` status (server) or when chunk data arrives (client), and is updated incrementally by every block change
that changes light properties.

## Inputs and state

- `minY = worldMinY − 1` (Overworld: −65), `maxY = worldMaxY + 1` (Overworld: 320). The table stores
  `value − minY` in a packed array of 256 entries of `ceil(log2(maxY − minY + 1))` bits (Overworld: 9 bits); the packing is
  not observable except through the accessors below. Index of column `(x, z)` (each 0..15) = `x + z·16`.
- A freshly created table is **all zero raw values**, i.e. every column reads as `minY` ("reaches below the world").
- Public accessors:
  - `getLowestSourceY(x, z)`: `raw + minY`, mapped: **a value equal to `minY` is returned as `i32::MIN`**
    ("sources reach below the world"); any other value is returned as is.
  - `getHighestLowestSourceY()`: `max` over the 256 raw values, plus `minY`, with the same `minY → i32::MIN` mapping. For
    an all-open chunk this is `i32::MIN`.
- Edge-occlusion predicate between a block `top` directly above a block `bottom` (both `BlockState`s):
  `isEdgeOccluded(top, bottom)` = `dampening(bottom) != 0`, **or** `faceShapeOccludes(lightShape(top, DOWN), lightShape(bottom, UP))`
  (`light-engine.md` Inputs A for `lightShape` / `emptyShape`). Only the **lower** block's dampening counts; leaves (1),
  water (1) and every solid block end a source column; blocks with `useShapeForLightOcclusion` can also end it through their
  face shapes.
- All reads of block states go through the chunk itself (never a neighbour); a position above `worldMaxY` or below `worldMinY`
  reads as air.

## Behavior (ordered rules)

### 1. Full (re)computation: `fillFrom(chunk)`

Run at the `initialize_light` task on the server (`chunk-steps.md` §2) and by the client when a chunk's data arrives
(`chunk-steps.md` §9).

1. `top = ` index of the highest section that is **not** "only air" (sections ordered bottom to top); if there is none, set
   every column's lowest source to `minY` and stop.
2. For `z` 0..15 (outer), `x` 0..15 (inner): `value = max(findLowestSourceY(chunk, top, x, z), minY)`; store it.

`findLowestSourceY(chunk, topIndex, x, z)` — walk the column downward, remembering the **upper block of the current edge**:
1. `topEdgeY = (sectionY(topIndex) + 1) · 16` (the Y just above the top block of the highest filled section); the block
   under test is the one at `topEdgeY − 1`; `topState = AIR`.
2. For `sectionIndex` from `topIndex` down to 0:
   - If the section is "only air": `topState = AIR`; `topEdgeY = ` the section's minimum block Y; the block under test becomes
     `topEdgeY − 1`. (The air section contributes no edge by itself.)
   - Otherwise for local `y` from 15 down to 0: `bottomState = block (x, y, z)` of that section; if
     `isEdgeOccluded(topState, bottomState)` **return `topEdgeY`**; else `topState = bottomState`, `topEdgeY = ` Y of that
     block, and the block under test moves one down.
3. If nothing occludes, return `minY`.

So the result is the Y of the first non-occluding block **above** the first occluding block, counted from the top.

### 2. Incremental update: `update(chunk, x, y, z)` → boolean "changed"

Called after a block at local column `(x, z)` and world `y` changed and `hasDifferentLightProperties` held
(`chunk-steps.md` §7). `current` = the stored lowest source (the raw un-mapped value `raw + minY`).

1. `upperEdgeY = y + 1`. If `upperEdgeY < current` return false (the change is far below the source line).
2. `topState = block(x, y+1, z)`, `middleState = block(x, y, z)`. Try `updateEdge(top = (y+1, topState), bottom = (y, middleState))`;
   if it reports a change return true.
3. Otherwise `bottomState = block(x, y−1, z)` and try `updateEdge(top = (y, middleState), bottom = (y−1, bottomState))`; return its result.

`updateEdge(topY, topState, bottomPos, bottomState)` with `checkedEdgeY = topY`:
- If `isEdgeOccluded(topState, bottomState)`: if `checkedEdgeY > current` store `checkedEdgeY` and return true; else false.
- Else (open edge): if `checkedEdgeY == current` store `findLowestSourceBelow(start = bottomPos, startState = bottomState)` and
  return true; else false.

`findLowestSourceBelow(startPos, startState)`: `topY = startPos.y`, `topState = startState`; for `bottomY` from `startPos.y − 1`
down to and including `minY`: `bottomState = block (x, bottomY, z)`; if `isEdgeOccluded(topState, bottomState)` return `topY`;
else `topState = bottomState`, `topY = bottomY`. If the loop ends return `minY`.

Only the **first** of the two edges that changes the value is applied per call (at most one write per call).

## Edge cases and known quirks

1. `getLowestSourceY` never returns exactly `minY`; "reaches below the world" is `i32::MIN`. Consumers compare with `<`, `<=`,
   `==` against real Y values, so `i32::MIN` makes `y == lowest` false and `y >= lowest` true for every `y`.
2. A chunk with no filled section is "open to the bottom" in every column (raw 0), exactly like a table that was never filled.
3. The table of a chunk that has not reached `initialize_light` is all zero (open); the sky engine reads other chunks' tables
   through the chunk-for-lighting accessor (`chunk-steps.md` §2), which returns null (treated as open) until the neighbour's
   `features` step has completed.
4. `update` trusts the old stored value; it is not a recomputation. A sequence of changes that alters occluders without
   passing through `update` (for example bulk edits with no light notification) leaves a stale table until the next
   `fillFrom`.
5. The aliasing of the scratch positions inside the original `update` / `findLowestSourceBelow` has no observable effect.

## Constants and IDs

`minY = worldMinY − 1`, `maxY = worldMaxY + 1`, 256 entries, index `x + 16·z`; "below the world" sentinel `i32::MIN`;
"unknown" sentinel used by the engine `i32::MAX` (not stored here).

## Oracle probe points

- `net.minecraft.world.level.chunk.ChunkAccess#getSkyLightSources()` → `ChunkSkyLightSources#getLowestSourceY(int,int)` (all 256
  columns) and `#getHighestLowestSourceY()`; call after each of: chunk status `initialize_light` completes, `full`, and after a
  scripted `ServerLevel#setBlock` (compare before/after tables).
- `ChunkAccess#initializeLightSources()` (re-runs `fillFrom`); `ChunkSkyLightSources#fillFrom(ChunkAccess)`, `update(BlockGetter,int,int,int)`
  (the boolean result matters).
- Block-state sweep: place each state of interest as the only block on a flat floor and record the resulting column value
  (covers leaves, water, slabs, stairs, snow layers, ice, glass, tinted glass, vines, farmland).

## Open questions

1. **[oracle] Table value for states with `dampening == 0` but non-empty face shapes** (bottom / top slabs, stairs in each
   half and facing, snow layers 1..8, farmland, dirt path): confirm the face-shape rule of `shapes.md` §4.4 yields the
   same `getLowestSourceY` as the game by the block-state sweep above.
2. **[oracle] Chunk edge reads.** `update` and `findLowestSourceBelow` read only the chunk's own states; confirm there is no
   neighbour-dependent behaviour by sweeping a change at local x/z = 0 and 15.

## Parity status

Not yet run.
