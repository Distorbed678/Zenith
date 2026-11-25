# Blocks: the voxel-shape model and per-state shapes (26.3)

- Verified by suites: `block-facts` (ZBF1 sections 6 "Shapes" and 7 "Face facts", and the shape-derived flags of
  section 2; see `docs/specs/parity/block-facts.md`). Later: entity movement and lighting suites reuse the same model.
- Sources: local reference reading of the 26.3 client (2026-11-25, reader session); data-generator
  `reports/blocks.json` for the block list, property order and state ids (1286 blocks, 35 723 states in 26.3);
  `docs/specs/parity/block-facts.md` for the exact probe positions and the empty getter / empty collision context
  used by the oracle. Everything below is written in our own words.
- Related specs: [`shapes-catalogue.md`](shapes-catalogue.md) (the shape of every block family),
  [`state-facts.md`](state-facts.md) (every other per-state fact, including the flags derived from shapes),
  [`block-table.md`](block-table.md) (every block: class, shape family, map colour, strength, sound, flags),
  [`sound-types.md`](sound-types.md); `docs/specs/registry/identifiers-registries-tags.md` §3 (state ids and
  property order); `docs/specs/math/random.md` (the position seed used by random offsets);
  `docs/specs/worldgen/features.md` §3.1 (`has_sturdy_face`, `solid`, `replaceable` predicates);
  `docs/specs/worldgen/structures-legacy-pieces.md` (sturdy-face / centre-support checks);
  `docs/specs/worldgen/density-functions.md` §7 (heightmap predicates).

## Summary

Every block state owns up to six shapes: the **outline** (selection) shape, the **collision** shape, the **visual**
shape, the **interaction** shape, the **occlusion** shape and the **block-support** shape, plus six per-direction
**face occlusion shapes** cut from the occlusion shape. All of them are values of one geometric type, the *voxel
shape*: a finite union of axis-aligned boxes stored as a 3-D grid of on/off cells over three sorted coordinate lists.
Shapes are built from boxes given in 1/16-block units, combined with boolean operations, simplified ("optimized")
back into a short list of maximal boxes, rotated by the 24/48 cube symmetries, translated, and sliced into face
shapes. Sturdiness ("can a torch/rail/etc. be supported by this face") is a pure geometric test of a face shape
against three fixed templates.

This document specifies (1) the shape model and every operation exactly enough to reproduce the box lists and bounds
bit for bit, (2) face extraction and the derived predicates, (3) the six per-state shapes, their defaults, collision
contexts and the random position offset. `shapes-catalogue.md` gives the concrete shapes of every block family.

## Inputs and state

### Units and axes

- Block-local coordinates run from 0 to 1 on each axis; "pixel" coordinates are 16× larger (0..16). The helpers
  below take pixels and divide by 16 in double precision (the division is exact for every pixel value used in 26.3
  except the lectern's two decimal literals, see the catalogue).
- Axis order is X, Y, Z (ordinal 0, 1, 2). Direction order (ordinal) is DOWN (−Y), UP (+Y), NORTH (−Z),
  SOUTH (+Z), WEST (−X), EAST (+X). Horizontal clockwise order (seen from above) is NORTH → EAST → SOUTH → WEST.
- All arithmetic is IEEE-754 binary64 with round-to-nearest-even, no fused multiply-add. Two tolerances appear:
  **ε = 1·10⁻⁷** (`EPSILON`) and 1·10⁻⁶ (`BIG_EPSILON`, unused by block shapes).

### The representation (normative for exact output)

A voxel shape *S* is the triple (grid, X-list, Y-list, Z-list):

1. **Grid.** Sizes (nx, ny, nz) ≥ 0 and a set of *full* cells (i, j, k) with 0 ≤ i < nx etc. The grid also caches
   a cell-space bounding box [x0, x1) × [y0, y1) × [z0, z1) (first full index, one past the last full index per
   axis). For grids built by filling cells the cache is the true bounding box of the full cells; the one exception is
   described in §1.2 (boxes created directly).
2. **Coordinate lists.** For each axis a non-decreasing list of nx+1 (ny+1, nz+1) doubles. Cell i on the X axis spans
   [X[i], X[i+1]). A list may be (a) a *regular* list "k/n for k = 0..n" (n ≥ 1) or (b) an explicit list of doubles.
   The distinction matters only for which merge strategy a join takes (§2.2) and for the shortcut in
   §2.2 rule 4; values are what the outputs use.
3. The shape's point set is the union of the cells' boxes. Two different triples can describe the same point set;
   all observable outputs used by the parity suite (emptiness, bounds, box list in order, face shapes, every
   predicate) depend only on the point set **and on the exact double values of its boundary coordinates**, except
   where a rule below says otherwise (the slice-bounds quirk of §4.2 depends on the *parent's* bounding box, which is
   also point-set determined).
4. **Distinguished shapes.** *EMPTY*: grid 0×0×0 with lists [0], [0], [0]. *BLOCK*: grid 1×1×1 with its single cell
   full and regular lists [0, 1] on all axes. Several algorithms take shortcuts when an argument *is* BLOCK or
   EMPTY (identity, not equality). Identity never changes a dumped value, but implementations should keep the two
   singletons so that shortcuts give the same answers in the tolerance edge cases.

## Behavior (ordered rules)

### 1. Building shapes

#### 1.1 `box(min, max)` from block-local doubles
1. If any min > max on an axis, the call is an error (never happens for 26.3 block shapes).
2. If, on any axis, max − min < ε, the result is EMPTY.
3. For each axis find the smallest *b* in {0, 1, 2, 3} such that both min·2ᵇ and max·2ᵇ are within ε·2ᵇ of an
   integer (rounding half up), provided min ≥ −ε and max ≤ 1 + ε; otherwise the axis has no *b*.
4. If every axis has a *b*:
   - if all three are 0 the result is BLOCK (the box is the unit cube);
   - otherwise the result is a regular-grid shape of sizes (2^bx, 2^by, 2^bz) whose full cells are the cell range
     [round(min·n), round(max·n)) on each axis, with regular coordinate lists k/n.
5. Otherwise (some axis has fractional coordinates finer than 1/8 or lies outside [0, 1]) the result is a 1×1×1 grid
   with the cell full and **explicit** lists [minX, maxX], [minY, maxY], [minZ, maxZ].

Consequence: boxes on 1/16 pixels that are not multiples of 2 pixels are explicit-list shapes; boxes on even pixels
are regular-grid shapes. Both describe the same point set; the representation only affects merging strategy.

#### 1.2 Pixel helpers (used by every block family; values in pixels, divided by 16)
- `box(x1, y1, z1, x2, y2, z2)` = 1.1 applied to each coordinate / 16.
- `column(w, y1, y2)` = `column(w, w, y1, y2)`; `column(wx, wz, y1, y2)` = box centred on the block's vertical axis:
  x from 8 − wx/2 to 8 + wx/2, z from 8 − wz/2 to 8 + wz/2, y from y1 to y2.
- `cube(s)` = `cube(s, s, s)`; `cube(sx, sy, sz)` = `column(sx, sz, 8 − sy/2, 8 + sy/2)` (centred on all axes).
- `boxZ(s, z1, z2)` = `boxZ(s, s, z1, z2)`; `boxZ(sx, sy, z1, z2)` = `boxZ(sx, 8 − sy/2, 8 + sy/2, z1, z2)`;
  `boxZ(sx, y1, y2, z1, z2)` = box with x from 8 − sx/2 to 8 + sx/2, y from y1 to y2, z from z1 to z2 (a bar along Z,
  centred in X).
- `boxes(n, f)` = the array f(0), f(1), …, f(n) (n + 1 entries).
- Helper arguments that are single-precision constants in the game (e.g. `16.0F − height`) are computed in single
  precision first and then widened; for all 26.3 values this is exact.

#### 1.3 Translation (`move(dx, dy, dz)`)
EMPTY stays EMPTY. Otherwise the grid is shared and every coordinate becomes *value + d* (one double addition per
coordinate, computed when read). Translation never re-merges anything.

### 2. Reading and combining shapes

#### 2.1 Basic queries
1. **Empty test.** A shape is empty iff its grid has no full cell (for grids with an explicit cell set) or, for a
   *slice* (§4.1), iff its cached bounding box is degenerate on some axis.
2. **min(axis)** = the coordinate at the grid's first full index on that axis (+∞ if none); **max(axis)** = the
   coordinate at one-past-the-last full index (−∞ if none).
3. **bounds()** = (minX, minY, minZ, maxX, maxY, maxZ); undefined for an empty shape (the parity dump writes no
   bounds for empty shapes).
4. **singleEncompassing()** = EMPTY if empty, else `box(bounds)` (rule 1.1 applied to the bounds, so the result can
   be BLOCK).
5. **Cell lookup for a coordinate** on an axis (`findIndex`): for a regular list with n cells it is
   floor(clamp(c·n, −1, n)); for an explicit list it is (the first index i in 0..n with c < list[i]) − 1, i.e. −1
   if c is below the first coordinate and n if c is at or above the last.

#### 2.2 Box enumeration (`forAllBoxes` / `toAabbs`) — the yield order is normative
Work on a scratch copy of the full-cell set.
1. For y-index j = 0 … ny−1, for x-index i = 0 … nx−1, scan z-index k = 0 … nz (k = nz counts as empty):
   - while scanning, remember the start k₀ of the current run of full cells;
   - when a run ends at k (cell k empty or k = nz): clear cells (i, j, k₀…k−1); then extend in +X while the whole
     z-run (i′, j, k₀…k−1) is full at i′ = i+1, i+2, … (clearing each such strip); let the last such i′ be i₁; then
     extend in +Y while the whole rectangle (i…i₁, j′, k₀…k−1) is full at j′ = j+1, … (clearing it); let the last
     be j₁; yield the box (X[i], Y[j], Z[k₀]) – (X[i₁+1], Y[j₁+1], Z[k]); continue the z-scan after k.
2. The yield order and box boundaries of this greedy merge are invariant under refining the grid (adding redundant
   coordinates), so an implementation may run it on any grid that represents the same point set with the same
   boundary values.

Worked example (north-facing bottom straight stairs, §stairs of the catalogue): yields (0,0,0,1,½,1) then
(0,½,0,1,1,½). The top (upside-down) variant yields (0,0,0,1,1,½) then (0,½,½,1,1,1) — the first box merges the
lower half-row and the slab vertically because the +Y extension succeeds.

#### 2.3 Boolean operations
The sixteen two-input boolean functions exist; block shapes use OR, AND, ONLY_FIRST (a ∧ ¬b), ONLY_SECOND (¬a ∧ b)
and NOT_SAME (a ≠ b). A join with an operation that is true for (false, false) is an error.

**Unoptimized join `joinUnoptimized(A, B, op)`** (ordered):
1. If A and B are the same object: result is A if op(true, true) else EMPTY.
2. Let *keepA* = op(true, false), *keepB* = op(false, true). If A is empty: result B if keepB else EMPTY. If B is empty:
   result A if keepA else EMPTY.
3. Build a merged coordinate list per axis (X first, then Y, then Z) with an *index merger*:
   - **Regular × regular** (both lists regular with nA, nB cells): let L = lcm(nA, nB). Let *cost* be 1 for X,
     (merged X cells) for Y, (merged X cells)·(merged Y cells) for Z. If cost·L ≤ 256, use the regular merger: the
     result list is regular with L cells and result cell r maps to A-cell ⌊r/(L/nA)⌋ and B-cell ⌊r/(L/nB)⌋.
   - Else, if A's last coordinate < B's first − ε: the **non-overlapping** merger: result list = A's list followed by
     B's list; result cells 0 … |A|−1 map to (A-cell r, no B cell) — note the last of these is the gap cell between
     the two lists and maps to an out-of-range A index, i.e. empty — then cells |A| … map to (no A cell, B-cell
     r − |A|). Symmetrically when B lies entirely below A (the roles are swapped back when mapping).
   - Else, if both lists have the same length and are element-wise equal: the **identical** merger (cell r ↦ r, r).
   - Else the **indirect** merger. Keep counters a and b (values consumed from A and from B, both starting at 0) and
     a "last emitted value" that starts as NaN (so the first comparison fails). Repeat until both lists are consumed:
     1. Take from A if A is not exhausted and (B is exhausted or A[a] < B[b] + ε); otherwise take from B. Record
        whether the *other* list was exhausted or not yet started (b = 0, resp. a = 0) **before** incrementing the
        counter of the list taken from.
     2. Skip rule: a value taken from A is dropped (nothing emitted, nothing updated) when ¬keepA **and** B was not
        started or was exhausted; a value taken from B is dropped when ¬keepB **and** A was not started or was
        exhausted. (Dropping only removes boundaries in regions where the result is necessarily empty.)
     3. Otherwise compare the value with the last emitted value: if last ≥ value − ε, no new coordinate is made and
        the most recent result cell's index pair is overwritten with (a − 1, b − 1); else the value is appended to the
        merged list with the index pair (a − 1, b − 1) and becomes the last emitted value.
     The merged list keeps at least one element; result cell r covers [list[r], list[r+1]) and evaluates A at the
     A index of pair r and B at its B index (an index of −1 or past the end means "not full").
4. Result grid: cell (rx, ry, rz) is full iff op(A full at its mapped cell, B full at its mapped cell). The cached
   bounding box is computed from the full cells. If all three mergers were regular mergers the result has regular
   lists; otherwise explicit lists (a regular merger on one axis still yields a regular list on that axis).

**Join `join(A, B, op)`** = `optimize(joinUnoptimized(A, B, op))`. **`or(A, B, …)`** folds left with OR.

**Optimize.** Start from EMPTY and fold the boxes of §2.2, in yield order, with `joinUnoptimized(acc, box(b), OR)`,
where each box goes through rule 1.1 (so a full unit cube becomes BLOCK and the first fold returns the box itself by
rule 2).

**Non-empty test `joinIsNotEmpty(A, B, op)`** (does op(A, B) contain any point?):
1. Error if op(false, false).
2. If either is empty: return op(A non-empty, B non-empty).
3. If A and B are the same object: return op(true, true).
4. For each axis X, Y, Z: if A.max < B.min − ε or B.max < A.min − ε, return keepA ∨ keepB.
5. Otherwise build the three mergers as in the join and return true as soon as a merged cell satisfies op.

**Derived tests.** `equal(A, B)` = ¬joinIsNotEmpty(A, B, NOT_SAME). **Full-block test** `isShapeFullBlock(S)` =
¬joinIsNotEmpty(BLOCK, S, NOT_SAME) (the point set equals the unit cube up to the ε merges). The game memoises this
in a cache keyed by shape identity; the cache has no observable effect.

### 3. Rotations

#### 3.1 The elements used by block shapes
Rotations act about the block centre (½, ½, ½) unless a pivot is given. As point maps (block-local):

| Name | Map (x, y, z) ↦ | Effect on a "north" shape |
|---|---|---|
| Y90 | (1 − z, y, x) | north → east |
| Y180 | (1 − x, y, 1 − z) | north → south |
| Y270 | (z, y, 1 − x) | north → west |
| X90 | (x, z, 1 − y) | north → down |
| X270 | (x, 1 − z, y) | north → up |
| INVERT_Y | (x, 1 − y, z) | mirror top/bottom |
| INVERT_X | (1 − x, y, z) | mirror left/right |

"Q after P" means apply P first, then Q. Compositions are evaluated as one combined permutation-with-sign-flips and
applied in a single step (equivalent to sequential application for the centre pivot).

#### 3.2 Applying a rotation to a shape
1. The identity returns the shape unchanged.
2. The grid is rotated cell by cell: each full cell's integer index vector is permuted and sign-flipped, then shifted
   so indices are again ≥ 0 (a flipped axis of size n maps index i to n − 1 − i). The new grid's bounding box is that
   of the moved cells.
3. If the shape had regular lists and the pivot is the block centre, the result keeps regular lists (sizes permuted).
4. Otherwise each new axis takes the coordinate list of the old axis that maps onto it. If that axis is flipped, the
   list is reversed and each value v becomes −(v − p_new) + p_old, computed exactly in that order (subtract, negate,
   add), where p_new is the pivot coordinate on the *source* axis and p_old the pivot coordinate on the *target*
   axis; if it is not flipped but the two pivot coordinates differ, v becomes (v − p_new) + p_old; otherwise the list
   is reused.

#### 3.3 Rotation families (maps from a direction/axis to a shape)
Given a shape *N* modelled for NORTH (and optionally an initial rotation I applied first):
- **horizontal(N, I)**: NORTH ↦ rotate(N, I) **about the block centre even when a pivot was supplied** (quirk; no
  26.3 family supplies a pivot to this form), EAST ↦ Y90 after I, SOUTH ↦ Y180 after I, WEST ↦ Y270 after I.
- **all(N, I, pivot)**: the four horizontal entries as above (EAST/SOUTH/WEST about the pivot, NORTH via rotate(N, I)
  about the centre), UP ↦ X270 after I, DOWN ↦ X90 after I (about the pivot).
- **horizontalAxis(N)**: Z ↦ N, X ↦ Y90(N).
- **allAxis(N)**: Z ↦ N, X ↦ Y90(N), Y ↦ X90(N).
- **attachFace(N)** (buttons, levers, grindstone): WALL ↦ horizontal(N); FLOOR ↦ horizontal(N, X270);
  CEILING ↦ horizontal(N, Y180 after X90). So a floor shape facing d is "X270, then the horizontal rotation of d"; a
  ceiling shape facing NORTH is "X90 then Y180", SOUTH is "X90" alone, EAST "X90 then Y270", WEST "X90 then Y90".

### 4. Face shapes

#### 4.1 `faceShape(S, d)` (the 2-D cross-section of S at face d, extruded to the full depth)
1. If S is empty, or S *is* BLOCK, return S.
2. Let *a* be d's axis. If S's list on *a* has exactly two entries and they are within ε of 0 and 1, return S itself
   (the shape is "cube-like along a": every cross-section is the same).
3. Otherwise find the cell index on *a* containing 0.9999999 (positive directions) or 1·10⁻⁷ (negative directions)
   using §2.1 rule 5. Form the **slice**: the one-cell layer of S's grid at that index (cells outside the grid read as
   empty), with coordinate list [0, 1] on *a* and S's own lists on the other two axes. Its cached bounding box is
   S's bounding box clamped to the layer: on *a* it is [0, 1) if S's cell-space box covers the layer index and
   degenerate otherwise; on the other two axes it is **S's whole bounding box**.
4. If the slice is empty (degenerate box on some axis) return EMPTY.
5. If S's lists on the two other axes are each exactly two entries within ε of 0 and 1, return BLOCK.
6. Otherwise return the slice.

#### 4.2 Consequences that show up in dumps
- A face shape spans [0, 1] along its own axis, whatever the source depth was.
- **Slice-bounds quirk:** the `bounds()` of a slice face are the source shape's full bounds on the two in-plane axes,
  not the bounds of the face's own cells. Example: the UP face of the north-facing bottom stair has the single box
  (0,0,0,1,1,½) but bounds (0,0,0,1,1,1).
- A source shape that is cube-like along the face axis returns itself (e.g. every horizontal face of a bottom slab is
  the slab, y 0..½).
- If the cell containing the probe coordinate is inside the bounding box but has no full cells, the slice is reported
  non-empty with zero boxes. No 26.3 block shape has a hollow probe layer inside its bounding box on the probed axis,
  so this case does not occur in the suite (it would need a gap at y≈1 or ≈0 with material on both sides).

#### 4.3 Derived face predicates
- **Face full** `isFaceFull(S, d)` = isShapeFullBlock(faceShape(S, d)), i.e. the cross-section at the face covers the
  whole unit square.
- **Support templates** (block-local; all three are fixed shapes):
  - FULL: the face shape must be full (as above).
  - CENTER: template = `column(2, 0, 10)` = x, z ∈ [7/16, 9/16], y ∈ [0, 10/16]. The face is sturdy iff no point of
    the template lies outside faceShape(S, d) (¬joinIsNotEmpty(face, template, ONLY_SECOND)).
  - RIGID: template = unit cube minus `column(12, 0, 16)` (a 2-pixel ring: x or z within 2/16 of the block edge, all
    y). Same test.
  - The templates are **not rotated** per direction. Because a face shape always spans the full depth of its own
    axis, this gives:
    - UP/DOWN: CENTER needs the 2×2-pixel centre column (7..9, 7..9) covered; RIGID needs the 2-pixel border ring
      covered.
    - NORTH/SOUTH (slice along Z): CENTER needs x ∈ [7, 9], y ∈ [0, 10] (pixels) covered; RIGID needs the whole
      face covered (equivalent to FULL).
    - WEST/EAST: CENTER needs z ∈ [7, 9], y ∈ [0, 10] covered; RIGID equivalent to FULL.
  - An empty source shape is never sturdy for any type.
- **isFaceSturdy(state, d, type)** = the test above applied to the state's **block-support shape** (§5.6) evaluated
  at the empty getter and the origin. States with a per-state cache (§5.1) precompute all 18 answers; the others
  (dynamic-shape blocks) compute them on demand with the same inputs, so the dumped answers are identical in form.
- **canSupportRigidBlock(below)** = isFaceSturdy(below state, UP, RIGID).
- **canSupportCenter(pos, d)** = false if d is DOWN and the state at pos is in tag `#minecraft:unstable_bottom_center`;
  otherwise isFaceSturdy(state, d, CENTER).
- **collision face full** (ZBF1 `collision_face_full_mask`) = isFaceFull(collision shape at P0, d).

#### 4.4 Occlusion helpers (renderer and light; not dumped directly, specified for completeness)
- **shouldRenderFace(state, neighbour, d)**: let O = neighbour's face occlusion shape on the side facing us
  (opposite of d). If O *is* BLOCK → false. Else if the block-specific "skip rendering" rule says so → false. Else if
  O *is* EMPTY → true. Else let F = our face occlusion shape for d; if F *is* EMPTY → true; else true iff F has a
  point not in O (joinIsNotEmpty(F, O, ONLY_FIRST)). A 256-entry per-thread memo keyed by the two shapes' identities
  has no observable effect.
- **blockOccludes(S, O, d)**: true if both are BLOCK; false if O is empty; else with (first, second) = (S, O) for
  positive d and (O, S) for negative d: requires first.max(a) within ε of 1, second.min(a) within ε of 0, and no
  point of first's top layer (on axis a) that is not in second's bottom layer (both as slices with [0,1] depth).
- **mergedFaceOccludes(S, O, d)**: true if either is BLOCK; otherwise, after discarding first (resp. second) when it
  does not reach 1 (resp. 0) on the face axis, true iff the union of the two boundary layers covers the unit square.
- **faceShapeOccludes(S, O)**: true if either is BLOCK; false if both empty; else true iff S ∪ O covers the unit cube.

### 5. The per-state shapes

All shape queries take a block getter, a position and (for some) a collision context. The parity suite evaluates
them with the **empty getter** (every position reads air / empty fluid / no block entity) and the **empty collision
context** at P0 = (0,0,0), P1 = (1,0,3), P2 = (−7,64,13) (`parity/block-facts.md`).

#### 5.1 Defaults (when a block class does not override)
1. **Outline** (`getShape(getter, pos, ctx)`): BLOCK.
2. **Collision**: if the block's properties have collision, the outline evaluated with the **empty** context (the
   context passed to the collision query is ignored by this default), else EMPTY.
3. **Visual** (`getVisualShape`): the collision shape with the same context (the class's own collision rule, not a
   cached value).
4. **Interaction** (`getInteractionShape`): EMPTY.
5. **Block support** (`getBlockSupportShape`): the collision shape with the **empty** context.
6. **Occlusion** (`getOcclusionShape`, position-independent): the outline evaluated with the empty getter, the
   origin and the empty context. The state's stored occlusion shape is this value if the properties say the block
   *can occlude*, otherwise EMPTY (see `state-facts.md` §2 "can_occlude").
7. **Face occlusion shapes**: if the stored occlusion shape is empty, all six are EMPTY; else if it is a full block
   (solid render), all six are BLOCK; else faceShape(occlusion, d) for each d.
8. **Entity-inside shape** (used for "is the entity inside this block" effects; not dumped): BLOCK.

#### 5.2 The per-state cache and dynamic shapes
At bootstrap every state (unless its block is flagged *dynamic shape*) computes once, with the empty getter, origin
and empty context: its collision shape; *large collision shape* = some axis has min < 0 or max > 1; the 18 sturdiness
answers; and *collision full block* = isShapeFullBlock(collision). A non-dynamic block whose collision shape is
non-empty while it has a random offset is a bootstrap error (so every offset block with collision is dynamic: bamboo,
pointed dripstone, sulfur spike). For dynamic-shape blocks (moving piston, all 17 shulker boxes, bamboo, scaffolding,
powder snow, pointed dripstone, sulfur spike) nothing is cached: *large collision shape* reports **true**
unconditionally, and the other queries are computed on demand from the getter/position actually passed.

#### 5.3 Collision contexts
A collision context answers: is the entity descending (sneak); is a given shape *below* the entity ("is above"); is
the entity holding a given item; should fluids always collide; can the entity stand on a fluid; is this a block
placement test; and it maps (state, getter, pos) to a collision shape (normally the state's collision shape with this
context).
- **Empty context** (the one the suite uses; also used for support shapes and caches): not descending; *is above*
  returns the caller-supplied default; holds no item; fluids do not always collide; cannot stand on fluid; not a
  placement; entity bottom −∞ (only relevant to subclasses); it **is** an entity context with no entity.
- **Entity context**: descending = entity sneaking state; *is above(S, pos)* = entity bottom Y > pos.y + S.max(Y) −
  1·10⁻⁵ (the single-precision constant 1e-5F widened, ≈ 9.999999747·10⁻⁶); holding = main-hand item; stand on fluid =
  living entity that can stand on that fluid and the fluid above is not the same fluid.
- **Minecart context** (experimental minecart movement only): like the entity context but the block under the
  current rail and, on slopes, the block the rail ascends towards collide as EMPTY.
- **Position context** (used by fluid-aware queries): only *is above* against a given Y; everything else false.
- **Placement context**: an entity context with *placement* = true.

Context-dependent shapes in 26.3 (all other shapes ignore the context):

| Family | Outline | Collision | Empty-context result |
|---|---|---|---|
| scaffolding | BLOCK if holding scaffolding, else stable/unstable shape | EMPTY when placement; stable shape when *is above* BLOCK and not descending; bottom strip when distance ≠ 0, bottom = true and *is above* "one block below"; else EMPTY | outline = stable (bottom=false) / unstable (bottom=true); collision = stable for every state |
| light | BLOCK when holding the light item, else EMPTY | default rule, i.e. the empty-context outline: always EMPTY | EMPTY |
| liquids (water, lava) | EMPTY | BLOCK when fluids always collide; EMPTY when level ≠ 0; at level 0 the living entity's "liquid collision shape" when it is above that shape and can stand on the fluid; else EMPTY | EMPTY |
| powder snow | BLOCK | EMPTY for placement or non-entity contexts; for an entity: falling > 2.5 blocks → box (0,0,0)-(1, 0.9F, 1) (0.9F widened ≈ 0.899999976); a falling block entity, or an entity that can walk on powder snow that is above the block and not descending → BLOCK; else EMPTY | EMPTY |
| moving piston | EMPTY | from the block entity (EMPTY when there is none) | EMPTY |
| shulker box | from the block entity's current box (BLOCK when there is none) | same | BLOCK |

#### 5.4 Random position offsets
Blocks whose properties carry an offset type move some of their shapes by a per-column pseudo-random vector.
1. Let s = positionSeed(x, 0, z) (`docs/specs/math/random.md`, the `Mth.getSeed` hash; note **y = 0**, so the offset
   is the same for the whole column).
2. Let h = the block's maximum horizontal offset (default 0.25 single precision; 0.125 for pointed dripstone and
   sulfur spike, which use (float) of their base shape's min X = 2/16) and v = the maximum vertical offset (default
   0.2 single precision; 0.1 for small dripleaf).
3. For each of the low 4-bit nibbles n_x = s & 15 and n_z = (s >> 8) & 15: compute q = n/15 in **single** precision
   (single division), then in double: offset = clamp((q − 0.5) · 0.5, −h, h) (h widened to double).
4. XYZ type only: n_y = (s >> 4) & 15, q = n_y/15 in single precision, offset_y = (q − 1.0) · v in double (v widened);
   XZ type: offset_y = 0.
5. The offset is applied (by `move`, §1.3) only where a family says so (flowers, propagule, bamboo, bamboo sapling,
   speleothems; see the catalogue). Plants with an offset type but no offset in their shape rule (e.g. short grass,
   tall grass, sunflower) still report the offset type and vectors but their shapes do not move.
6. Reference values (for test vectors): at P0 every nibble is 0 → (−0.25, 0, −0.25) with h = 0.25, (−0.125, 0,
   −0.125) with h = 0.125, y = −0.20000000298023224 (v = 0.2F) or −0.10000000149011612 (v = 0.1F). At P1 the nibbles
   are (x 7, y 5, z 1) and at P2 (x 6, y 3, z 4).

#### 5.5 How the suite's position variation arises
Only the offset (5.4) and the getter/block-entity-dependent families make shapes position-dependent. With the empty
getter the latter are constant, so the ZBF1 `shape_pos_diff_mask` is non-zero exactly for states whose outline (and
hence visual / collision / support when those derive from it) moves with the offset.

#### 5.6 Block-support shape overrides (summary; details per family in the catalogue)
EMPTY for leaves and for open fence gates; the *outline* for ceiling and wall hanging signs; the closed-gate support
bar for closed fence gates; BLOCK for soul sand and mud; `column(14, 0, 15)` for chorus flowers; one layer *more* than
the collision for snow; BLOCK (no block entity) for shulker boxes; otherwise the default (collision with the empty
context).

### 6. Bootstrap order (normative where a fact depends on another)
Blocks are registered in registry order; for each block, each state in state-id order computes (1) its fluid state,
(2) its random-tick flag, (3) the cache of §5.2 (unless dynamic), (4) the legacy *solid* flag (needs the cached
collision shape), (5) the occlusion shape (if it can occlude), (6) solid render = isShapeFullBlock(occlusion), (7)
the face occlusion shapes, (8) propagates-skylight-down, (9) light dampening. See `state-facts.md` §1.

## Edge cases and known quirks

1. **Slice bounds** (§4.2): face shapes report their source's in-plane bounds. Dumped in ZBF1 `face_occlusion_shapes`.
2. **Unrotated support templates** (§4.3): CENTER on horizontal faces tests the lower 10 pixels of the middle 2-pixel
   strip; RIGID on horizontal faces equals FULL.
3. **horizontal(N, I, pivot)** rotates the NORTH entry about the centre, ignoring the pivot; only the hopper uses a
   pivot and it goes through `all(N, IDENTITY, pivot)` whose NORTH entry is the unrotated shape, so no 26.3 output
   is affected.
4. **Empty-run grids.** `box()` of a zero-thickness box (snow layer 0, pitcher-crop upper halves at ages 0–2) is EMPTY;
   unions that stay empty fall back to the family's "empty fallback" (BLOCK for vines/multiface/pale moss carpet,
   the floor sheet for fire).
5. **Large shapes.** Fence/wall/gate collision (y up to 1.5), the piston head's long arm (up to 1.25 / down to −0.25),
   pitcher-crop collision and outline (y from −1/16), the lectern outline (y up to 18/16) and the hopper's spout (before
   clipping) leave the unit cube; such shapes use explicit coordinate lists (rule 1.1 step 5).
6. **Lectern decimals.** 5.333333 and 9.666667 pixels are decimal literals; divided by 16 they are not dyadic, so the
   lectern's rotated shapes depend on the exact flip expression of §3.2 rule 4.
7. **Identity shortcuts** (BLOCK / EMPTY / same object) are observationally equivalent to the general path for every
   26.3 shape; keep them anyway.
8. **Offset at P0 is not zero** (§5.4), so every offset family's P0 outline is displaced.

## Constants and IDs

| Name | Value |
|---|---|
| ε (merge / cube-like / bit-detection tolerance) | 1.0E-7 |
| face probe coordinates | 0.9999999 (positive), 1.0E-7 (negative) |
| regular-merger cost limit | 256 |
| maximum grid subdivision exponent in `box()` | 3 (1/8 block) |
| CENTER template | `column(2, 0, 10)` px |
| RIGID template | unit cube minus `column(12, 0, 16)` px |
| entity *is above* tolerance | 1.0E-5F widened |
| powder-snow falling collision top | 0.9F widened |
| default max horizontal / vertical offset | 0.25F / 0.2F |
| speleothem max horizontal offset | (float) 0.125 |
| small dripleaf max vertical offset | 0.1F |
| legacy solid thresholds (state-facts) | average extent ≥ 0.7291666666666666 or height ≥ 1.0 |

## Oracle probe points

Public names in the 26.3 client (for the oracle agent only; implementations must not depend on them):
`net.minecraft.world.phys.shapes.Shapes` (`box`, `create`, `join`, `joinUnoptimized`, `joinIsNotEmpty`, `or`,
`rotate`, `rotateHorizontal`, `rotateAll`, `rotateAttachFace`, `rotateHorizontalAxis`, `rotateAllAxis`,
`blockOccludes`, `mergedFaceOccludes`, `faceShapeOccludes`, `block`, `empty`), `VoxelShape` (`bounds`, `toAabbs`,
`forAllBoxes`, `getFaceShape`, `optimize`, `move`, `min`, `max`, `singleEncompassing`, `isEmpty`),
`net.minecraft.world.level.block.Block` (`box`, `column`, `cube`, `boxZ`, `boxes`, `isFaceFull`,
`isShapeFullBlock`, `canSupportCenter`, `canSupportRigidBlock`, `shouldRenderFace`), `SupportType`,
`BlockBehaviour.BlockStateBase` (`getShape`, `getCollisionShape`, `getVisualShape`, `getInteractionShape`,
`getOcclusionShape`, `getBlockSupportShape`, `getFaceOcclusionShape`, `isFaceSturdy`, `isCollisionShapeFullBlock`,
`hasLargeCollisionShape`, `getOffset`, `hasOffsetFunction`), `CollisionContext` (`empty`, `of`, `placementContext`),
`com.mojang.math.OctahedralGroup`. The `BlockFacts` oracle (ZBF1) already dumps every value this spec defines.

## Open questions

1. **[oracle] Tolerance merges.** Confirm that no 26.3 state hits a merge of two distinct coordinates within ε: run the
   ZBF1 dump and check that every box coordinate equals a value producible from the catalogue formulas (the lectern
   rotations, offsets at P1/P2, and the hopper spout are the only non-trivial values). Expected: no discrepancies.
2. **[oracle] Hollow probe layer** (§4.2 last bullet): confirm no state's face occlusion shape is "non-empty with zero
   boxes" (`box_count = 0` while the dump wrote a shape) — expected none.

## Parity status

Not yet run.
