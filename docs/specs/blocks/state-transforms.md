# Blocks: how every block state is rotated and mirrored (26.3)

- Verified by suites: `block-transforms` (ZBT1, see `docs/specs/parity/block-transforms.md`): for every one of the
  35,723 states, the state id after each of the 12 (mirror, rotation) combinations.
- Sources: local reference reading of the 26.3 client (2026-11-25, reader session): the rotation and mirror
  enumerations, the per-block `rotate` / `mirror` rules of every block class that has one, the rail and stair tables,
  the octahedral-group helper used by the two block-orientation properties, and the three places that apply a rotation
  and a mirror to a block state when a structure is placed (template placement, legacy piece placement, jigsaw
  connector lookup). The block list, property names and state ids come from the data-generator `reports/blocks.json`
  (1286 blocks, 35,723 states). The assignment of every block to a rule (§6, Appendix) was produced by asking the game
  which class supplies each block's rotate and mirror behaviour, and the rules themselves were re-implemented
  independently and compared with the game for all 35,723 × 12 results (0 differences). Everything is in our own words.
- Related specs: [`shapes-catalogue.md`](shapes-catalogue.md) (block families), [`state-facts.md`](state-facts.md),
  [`block-table.md`](block-table.md) (block → class), [`shapes.md`](shapes.md) (direction order, the same 24/48-element
  symmetry group used for shapes); `docs/specs/registry/identifiers-registries-tags.md` §3 (state ids, property order);
  `docs/specs/worldgen/structures-legacy-pieces.md` (legacy pieces that call the same transforms);
  `docs/specs/worldgen/features.md`; `docs/specs/parity/block-transforms.md` (the oracle dump format).

## Summary

A block state can be *rotated* about the vertical axis by 0, 90, 180 or 270 degrees and *mirrored* across a vertical
plane (either of the two horizontal axes). Every block decides what that means for its own state: most blocks have no
orientation and ignore both; most others carry a `facing` direction (or a rotation index, an axis, a set of side
connections, a rail shape, …) and rewrite that single property; a few rewrite more than one property or have
asymmetries that are not geometrically "correct" (a mirrored double chest keeps its left/right type, a mirrored door
flips its hinge on both mirrors, a stair mirrored along the wrong axis stays put, an anvil ignores mirrors, …). All of
it is a pure function from (state, rotation, mirror) to a state of the **same block** with every other property
unchanged.

This document specifies: (1) the primitive direction, axis, index and shape maps; (2) the ten rules T0–T9 and exactly
which properties each rewrites; (3) which rule every one of the 1286 blocks follows; (4) the order in which a
structure placement applies mirror and rotation (mirror first, then rotation); and (5) the quirks. Together with the
block-state tables this is enough to derive the 12 results for every state without further data.

## Inputs and state

### The two enumerations

- **Rotation** (clockwise as seen from above, i.e. looking down the −Y axis): `none`, `clockwise_90`, `clockwise_180`,
  `counterclockwise_90`, indices 0–3 in that order. Clockwise 90 sends NORTH → EAST → SOUTH → WEST → NORTH.
  Composition is the cyclic group of order 4 on the index (a ∘ b = (a + b) mod 4). Every rule below satisfies
  `rotate(rotate(s, a), b) == rotate(s, a + b)`; this was checked for all states.
- **Mirror**: `none`, `left_right`, `front_back`, in that order. `left_right` flips the **Z** axis, so it exchanges
  NORTH and SOUTH and leaves EAST/WEST alone. `front_back` flips the **X** axis, so it exchanges EAST and WEST and
  leaves NORTH/SOUTH alone. Neither touches UP/DOWN. Every rule below is an involution under each mirror
  (`mirror(mirror(s, m), m) == s`); checked for all states.
- For the block-orientation property (T9) each enumeration also names a symmetry of the cube: rotation 90 clockwise
  is the quarter turn about Y that sends NORTH to EAST, rotation 180 the half turn about Y, rotation 270 the quarter
  turn that sends NORTH to WEST; `left_right` is the reflection in Z, `front_back` the reflection in X.

### Direction facts used below

- Horizontal directions in clockwise order: N, E, S, W. UP and DOWN are *vertical*.
- Axes: X (east–west), Y (up–down), Z (north–south).
- A state's property values are looked up by name; a rule rewrites some named properties and copies all the others
  verbatim (waterlogged, powered, open, lit, age, power, half, type, …).

### The properties a rule can rewrite

`facing` (six-valued direction, or four horizontal values), `axis` (x/y/z or x/z), `rotation` (integer 0–15),
`north`/`east`/`south`/`west` (boolean, wall-side enum, or redstone-side enum, depending on block), `shape` (stair
shape or rail shape), `hinge` (left/right), `orientation` (one of 12 front-and-top combinations). The block's other
properties never change.

## Behavior (ordered rules)

### 1. Shape of the operation

1. `rotate(state, r)` and `mirror(state, m)` look up the state's block and apply that block's rule (rule table in §6).
   The block never changes; only named properties do; the result is always a registered state of the same block.
2. A block with no rule (T0) returns the state unchanged for every rotation and every mirror.
3. `none` rotation and `none` mirror return the state unchanged for every block (rules with a `none` special case or a
   fall-through both give identity).
4. Different rotation and mirror values are independent calls; when both are wanted the order is fixed (§5).

### 2. Direction primitives

- **rotateDir(d, r)** for a direction d: UP and DOWN are returned unchanged; a horizontal d is moved one step
  clockwise (90), to its opposite (180) or one step counter-clockwise (270) in the order N → E → S → W; `none` returns d.
- **mirrorDir(d, m)**: `front_back` returns the opposite of an X-axis direction (E ↔ W); `left_right` returns the
  opposite of a Z-axis direction (N ↔ S); everything else (including UP and DOWN) is unchanged.
- **mirrorAsRotation(d, m)**: the rotation that realises mirrorDir for a direction: `clockwise_180` when m is
  `front_back` and d is on the X axis, or m is `left_right` and d is on the Z axis; otherwise `none`. For every
  horizontal or vertical d, `rotateDir(d, mirrorAsRotation(d, m)) == mirrorDir(d, m)`.
- **rotate-an-index(v, r, n)** for an index v in [0, n): `clockwise_90` → (v + n/4) mod n; `clockwise_180` →
  (v + n/2) mod n; `counterclockwise_90` → (v + 3n/4) mod n; `none` → v. Used with n = 16.
- **mirror-an-index(v, m, n)**: let h = n/2 and c = v − n if v > h, else v. `left_right` → (h − c + n) mod n;
  `front_back` → (n − c) mod n; `none` → v. Because c ≡ v (mod n) this equals (h − v) mod n for `left_right` and
  (−v) mod n for `front_back`. With n = 16: `left_right` v → (8 − v) mod 16, `front_back` v → (16 − v) mod 16.
- **Side permutation** (used by every "four-sided" block, T8). Let the four horizontal side properties be N, E, S, W.
  Writing `new.X = old.Y` for "the value of property X after the operation is the value Y had before":
  - `clockwise_90`: new.N = old.W, new.E = old.N, new.S = old.E, new.W = old.S.
  - `clockwise_180`: new.N = old.S, new.E = old.W, new.S = old.N, new.W = old.E.
  - `counterclockwise_90`: new.N = old.E, new.E = old.S, new.S = old.W, new.W = old.N.
  - `left_right`: new.N = old.S, new.S = old.N; E and W unchanged.
  - `front_back`: new.E = old.W, new.W = old.E; N and S unchanged.
  All four values are read from the *old* state and written to the new one (a true permutation; the values themselves,
  whatever their type, move as a whole).
- **Front-and-top orientation** (T9). The property has 12 values written `front_top`: front DOWN or UP with top one of
  N/E/S/W (eight values: `down_east`, `down_north`, `down_south`, `down_west`, `up_east`, `up_north`, `up_south`,
  `up_west`), and front one of W/E/N/S with top UP (four values: `west_up`, `east_up`, `north_up`, `south_up`).
  A rotation or mirror is applied to the front direction and to the top direction **independently**, each by the
  cube symmetry of the rotation or mirror: rotations map a horizontal direction as in rotateDir and fix UP and DOWN;
  `left_right` exchanges N and S, `front_back` exchanges E and W, UP and DOWN fixed. The resulting pair is the new
  value (the pair is always one of the 12 values). Examples: `down_east` → (cw90) `down_south`, (cw180) `down_west`,
  (ccw90) `down_north`; `north_up` → (cw90) `east_up`, (`left_right`) `south_up`; `up_west` → (`front_back`) `up_east`.

### 3. Rail shapes

Rail shape has ten values: `north_south`, `east_west`, `ascending_east`, `ascending_west`, `ascending_north`,
`ascending_south`, `south_east`, `south_west`, `north_west`, `north_east`. The ascending name is the direction of the
rising end. All maps are exact tables (not derivable by a single formula; the tables below are normative):

| old shape | clockwise_90 | clockwise_180 | counterclockwise_90 | left_right | front_back |
|---|---|---|---|---|---|
| `north_south` | `east_west` | `north_south` | `east_west` | `north_south` | `north_south` |
| `east_west` | `north_south` | `east_west` | `north_south` | `east_west` | `east_west` |
| `ascending_east` | `ascending_south` | `ascending_west` | `ascending_north` | `ascending_east` | `ascending_west` |
| `ascending_west` | `ascending_north` | `ascending_east` | `ascending_south` | `ascending_west` | `ascending_east` |
| `ascending_north` | `ascending_east` | `ascending_south` | `ascending_west` | `ascending_south` | `ascending_north` |
| `ascending_south` | `ascending_west` | `ascending_north` | `ascending_east` | `ascending_north` | `ascending_south` |
| `south_east` | `south_west` | `north_west` | `north_east` | `north_east` | `south_west` |
| `south_west` | `north_west` | `north_east` | `south_east` | `north_west` | `south_east` |
| `north_west` | `north_east` | `south_east` | `south_west` | `south_west` | `north_east` |
| `north_east` | `south_east` | `south_west` | `north_west` | `south_east` | `north_west` |

(The curved names list the two connected sides; a curve is a pair {N or S} + {E or W}. For curves,
`left_right` exchanges the N/S member and `front_back` the E/W member, and the three rotations move the pair as the
side permutation of §2 would. For slopes, `left_right` only changes `ascending_north` ↔ `ascending_south`, and
`front_back` only `ascending_east` ↔ `ascending_west`.)

### 4. Stair mirror

Stairs rotate like any facing block (T5 rule below) but mirror with a hand-written table that changes `facing` and
`shape` together. `shape` has five values: `straight`, `inner_left`, `inner_right`, `outer_left`, `outer_right`.

| mirror | facing axis | effect |
|---|---|---|
| `left_right` | Z (north or south) | facing becomes its opposite; shape: `outer_left` ↔ `outer_right`, `inner_left` ↔ `inner_right`, `straight` stays |
| `left_right` | X (east or west) | **nothing changes** (state returned unchanged, whatever the shape) |
| `front_back` | X (east or west) | facing becomes its opposite; shape: `outer_left` ↔ `outer_right`, `inner_left` stays `inner_left`, `inner_right` stays `inner_right`, `straight` stays |
| `front_back` | Z (north or south) | **nothing changes** |

(The two mirrors treat the inner shapes differently: under `left_right` the inner pair exchanges, under `front_back`
each inner shape is kept. This asymmetry is real and is reproduced exactly.)

### 5. Order of application when a block is placed from a template

1. **Template placement** (structure templates, including jigsaw pieces): for every block of the template, after all
   structure processors have run, the state is **mirrored first, then rotated**: `rotate(mirror(state, m), r)` with the
   placement settings' mirror m and rotation r. The processors therefore see (and may replace) the untransformed
   state; whatever state they return is the one that is mirrored and rotated. Block entity data is applied to the
   result.
2. The block **position** follows the same order: the template-local x/z is mirrored first (`left_right`: z → −z;
   `front_back`: x → −x), then rotated about the pivot (px, pz): clockwise 90 → (px + pz − z, pz − px + x);
   counter-clockwise 90 → (px − pz + z, px + pz − x); 180 → (2·px − x, 2·pz − z); `none` leaves the mirrored position.
3. **Legacy (hard-coded) pieces** apply the piece's mirror (skipped when `none`) and then its rotation (skipped when
   `none`) to every block they place from a state value; skipping `none` is the same as applying it. A piece's
   orientation fixes the pair: no orientation → (`none`, `none`); NORTH → (`none`, `none`); SOUTH → (`left_right`,
   `none`); WEST → (`left_right`, `clockwise_90`); EAST → (`none`, `clockwise_90`).
4. **Jigsaw connector lookup** (template jigsaw blocks seen from a placement with a given rotation) applies the
   **rotation only** to the connector's state; mirror is not used. Listing blocks of one kind from a template at a
   rotation likewise applies the rotation only.
5. The order matters: for many blocks `rotate(mirror(s, m), r) ≠ mirror(rotate(s, r), m)`. Example: a door facing north,
   hinge left; `left_right` then `clockwise_90` gives facing west, hinge right; rotating first and then mirroring gives
   facing east, hinge right. In 26.3, 28,864 of the 35,723 states are order-sensitive for at least one (mirror, rotation)
   pair. Implementations must therefore compose exactly as in item 1, or use the 12-column table.

### 6. The rules

Every block follows exactly one rule; the list of blocks per rule is in the Appendix (Constants and IDs). "Left
untouched" below means the property is not read or written by the rule at all, even when it looks directional.

**T0 — identity.** No property is changed by any rotation or mirror. (699 blocks, 4,575 states.) This includes blocks
whose properties *look* directional but are deliberately not handled: fire (`north`/`east`/`south`/`west`/`up`),
chorus plant (six sides), pointed dripstone and sulfur spike (`vertical_direction`), double plants and pitcher
crop (`half`), slabs (`type`), structure block and test block (`mode`).

**T1 — axis (pillar-like).** Rotation by 90 or 270 exchanges the `axis` values x ↔ z (y unchanged); rotation by 0 or
180, and **every mirror**, leave it unchanged. Applies to logs, woods, stems, pillars, bone block, hay bales, chains
and similar, the creaking heart, the infested deepslate, and the nether portal (whose axis only has x and z). (73
blocks, 260 states.)

**T2 — `facing` (the generic directional rule).** `facing` becomes rotateDir(facing, r) under rotation and
mirrorDir(facing, m) under mirror (identical to rotating by mirrorAsRotation). Vertical facings (UP, DOWN) are never
changed by either operation. No other property changes. Applies to every "facing"-style block not listed in T3–T9: the
horizontal-facing family (beds, trapdoors, fence gates, buttons and levers, glazed terracotta, comparators and
repeaters, cocoa, carved pumpkins, loom, big dripleaf and its stem, dried ghast, straw bed, shelf mushrooms), the six-way
facing family (dispenser, dropper, pistons and piston head and moving piston, observer, shulker boxes, hopper, rods,
amethyst clusters, command blocks), and every wall-mounted or hanging block (wall torches, wall signs, wall hanging
signs, wall skulls and heads, wall banners, coral wall fans), the furnaces, chests (all kinds), ender chest, barrel,
lectern, stonecutter, grindstone, bell, campfires, beehive and bee nest, vault, decorated pot, copper golem statues,
flower beds (pink petals and wildflowers), leaf litter, small dripleaf, ladder, tripwire hook, attached stems, end
portal frame, shelves, chiseled bookshelf, calibrated sculk sensor. (255 blocks, 5,986 states.) The exact block list
is in the Appendix, T2. The properties that stay put, even though they look related, include: chest `type`
(single/left/right), bed `part` (head/foot), lever/button/grindstone `face` (floor/wall/ceiling), bell `attachment`,
trapdoor `half`, shelf `side_chain`, tripwire hook `attached`, piston `extended`/`type`/`short`.

**T3 — anvil.** `facing` follows the rotation exactly as in T2, but **mirrors do nothing** (all three mirror values
return the state unchanged). Applies to anvil, chipped anvil, damaged anvil. (3 blocks, 12 states.)

**T4 — door.** Rotation: `facing` rotated as in T2. Mirror (either): `facing` becomes mirrorDir(facing, m) (so it flips
only when the mirror axis matches the facing's axis), **and `hinge` flips between left and right regardless of
facing**. `half`, `open`, `powered` unchanged. Applies to every wooden, iron and copper door. (22 blocks, 1,408 states.)

**T5 — stair.** Rotation: `facing` rotated as in T2, `shape` and `half` unchanged. Mirror: §4 table. `waterlogged`
unchanged. Applies to every stairs block. (97 blocks, 7,760 states.)

**T6 — rail.** Rotation and mirror replace `shape` per the §3 table; `powered` and `waterlogged` unchanged. Applies to
rail, powered rail, activator rail, detector rail. The rails whose shape set has only the six non-curved values
(powered, activator, detector) never meet a curved row. (4 blocks, 92 states.)

**T7 — rotation index (16 steps).** `rotation` (0–15) becomes rotate-an-index(v, r, 16) under rotation and
mirror-an-index(v, m, 16) under mirror. Applies to standing signs, ceiling hanging signs (whose `attached` and
`waterlogged` are unchanged), standing banners and floor skulls/heads (whose `powered` is unchanged). (49 blocks, 1,728
states.) Table of the five non-identity operations for a few indices (the formulas of §2 give all 16):

| v | cw90 | cw180 | ccw90 | left_right | front_back |
|---|---|---|---|---|---|
| 0 | 4 | 8 | 12 | 8 | 0 |
| 1 | 5 | 9 | 13 | 7 | 15 |
| 3 | 7 | 11 | 15 | 5 | 13 |
| 4 | 8 | 12 | 0 | 4 | 12 |
| 5 | 9 | 13 | 1 | 3 | 11 |
| 8 | 12 | 0 | 4 | 0 | 8 |
| 12 | 0 | 4 | 8 | 12 | 4 |
| 15 | 3 | 7 | 11 | 9 | 1 |

**T8 — four horizontal sides.** The four properties `north`, `east`, `south`, `west` are permuted per §2 "side
permutation". Under rotation the permutation is the one for that rotation; under mirror, `left_right` exchanges `north`
and `south`, `front_back` exchanges `east` and `west`. The properties `up` and `down` (where the block has them), the
`waterlogged`, `power`, `attached`, `disarmed`, `powered`, `bottom` flags and every other property are unchanged. The
side values move whole (a redstone wire's `side`/`up`/`none`, a wall's `none`/`low`/`tall`, a moss carpet's wall-side
values, or a boolean). Applies to: fences, glass panes, iron and copper bars (all the cross-collision blocks), walls,
redstone wire, tripwire, vine, pale moss carpet, huge mushroom blocks (the two caps and the stem, which also have `up`
and `down` that stay), and the multi-face blocks (glow lichen, resin clump, sculk vein — which have six faces
and, in 26.3, always rotate and mirror because every one of the four horizontal faces exists on them). (82 blocks,
13,842 states.)

**T9 — front-and-top orientation.** `orientation` is transformed per §2 "front-and-top". Applies to the jigsaw block
and the crafter (whose `triggered` and `crafting` are unchanged). (2 blocks, 60 states.)

### 7. Composition in the 12-column table

The oracle (and a Rust implementation's table) is indexed by (mirror, rotation) with mirror ∈ {none, left_right,
front_back} and rotation ∈ {none, clockwise_90, clockwise_180, counterclockwise_90}, column = mirror × 4 + rotation.
Column 0 is the identity. The nine columns with a non-`none` mirror and any rotation, plus the three rotation-only
columns, plus the two mirror-only columns, are the 11 non-identity combinations; each is `rotate(mirror(s, m), r)`.

## Edge cases and known quirks

1. **Chest type is never swapped.** A mirrored double chest keeps `type` left or right; only `facing` changes (and only
   for the matching axis). Single chests have `type = single`. The same holds for trapped chests and copper chests.
2. **Bed part, door half, trapdoor half, stair half, lever face, grindstone face, bell attachment** are never touched.
3. **Doors flip the hinge on *both* mirrors**, even when the mirror does not change the facing (a north-facing door
   under `front_back` keeps facing north but its hinge flips).
4. **Stairs ignore the mirror whose axis is perpendicular to their facing** (§4), and the two mirrors treat inner shapes
   differently. `left_right` followed by `front_back` on a stair is not the same as `clockwise_180` in general
   (4,656 stair states and 12 anvil states differ).
5. **Anvils ignore mirrors**, **pillars and the nether portal ignore mirrors** (axis x/z does not change), and only
   rotations of 90/270 change an axis.
6. **Mirror of a rotation-16 value** is a reflection about index 0 for `front_back` (`v → −v`; 0 and 8 are fixed) and
   about index 4 for `left_right` (`v → 8 − v`; 4 and 12 are fixed, 0 ↔ 8). Quarter-turn indices therefore map: under
   `left_right` 0 ↔ 8, 1 ↔ 7, 2 ↔ 6, 3 ↔ 5, 9 ↔ 15, 10 ↔ 14, 11 ↔ 13; under `front_back` 1 ↔ 15, 2 ↔ 14, … 7 ↔ 9.
7. **Vertical facings never change**: dispenser/dropper/piston/observer/shulker box/rod/amethyst/hopper facing UP or
   DOWN are fixed by every rotation and mirror, and a hopper's DOWN is fixed too.
8. **Vertical and non-side properties of side-connected blocks are fixed**: `up`/`down` booleans on vines, walls, chorus
   plants, huge mushrooms and multi-face blocks; a vine's top flag; a wall's post.
9. **Blocks that only look directional stay identity**: fire (its side flags), chorus plant, pointed dripstone and
   sulfur spike, slabs, double plants, structure and test blocks (T0).
10. **Rails**: straight shapes are unchanged under 180 and under both mirrors; rotation by 90 or 270 swaps the two
    straight shapes. Ascending shapes change under 180 and under the mirror that lies along their rise axis (`left_right`
    for north/south slopes, `front_back` for east/west slopes). Curves change under every non-`none` operation.
11. **Only registered states can result.** The result of every operation is a registered state of the same block, so a
    table of state ids is closed under the operations.
12. **Non-template callers.** Entities (item frames, paintings) use the same enumerations on yaw and position; that is
    separate from this block-state spec.

## Constants and IDs

Counts verified against the game (35,723 states):

| Fact | Value |
|---|---|
| blocks / states | 1,286 / 35,723 |
| table columns per state | 12 (mirror ∈ {none, left_right, front_back}) × (rotation ∈ {none, cw90, cw180, ccw90}) |
| states changed by `cw90` / `cw180` / `ccw90` (alone) | 30,206 / 28,876 / 30,206 |
| states changed by `left_right` / `front_back` (alone) | 18,668 / 18,668 |
| states changed by `left_right` then `cw90` / `cw180` / `ccw90` | 28,858 / 21,784 / 28,858 |
| states changed by `front_back` then `cw90` / `cw180` / `ccw90` | 28,858 / 20,232 / 28,858 |
| T0 / T1 / T2 / T3 / T4 / T5 / T6 / T7 / T8 / T9 (blocks) | 699 / 73 / 255 / 3 / 22 / 97 / 4 / 49 / 82 / 2 |
| T0 … T9 (states) | 4,575 / 260 / 5,986 / 12 / 1,408 / 7,760 / 92 / 1,728 / 13,842 / 60 |
| identity column mismatches | 0 |

### Appendix: rule assignment for every block

Below, each group lists the exact block ids (without the namespace) that follow the rule. "Left untouched" lists the
property names of those blocks that no operation touches. Group headings give the base behaviour name in the game; they
carry no extra meaning beyond grouping.

**T0 — 699 blocks** (no `rotate` and no `mirror` override; no property is touched):

air, stone, granite, polished_granite, diorite, polished_diorite, andesite, polished_andesite, grass_block, dirt, coarse_dirt, podzol, cobblestone, oak_planks, spruce_planks, birch_planks, jungle_planks, acacia_planks, cherry_planks, dark_oak_planks, pale_oak_planks, mangrove_planks, poplar_planks, bamboo_planks, bamboo_mosaic, oak_sapling, spruce_sapling, birch_sapling, jungle_sapling, acacia_sapling, cherry_sapling, dark_oak_sapling, pale_oak_sapling, mangrove_propagule, poplar_sapling, bedrock, water, lava, sand, suspicious_sand, red_sand, gravel, suspicious_gravel, gold_ore, deepslate_gold_ore, iron_ore, deepslate_iron_ore, coal_ore, deepslate_coal_ore, nether_gold_ore, mangrove_roots, oak_leaves, spruce_leaves, birch_leaves, jungle_leaves, acacia_leaves, cherry_leaves, dark_oak_leaves, pale_oak_leaves, mangrove_leaves, red_poplar_leaves, orange_poplar_leaves, yellow_poplar_leaves, azalea_leaves, flowering_azalea_leaves, sponge, wet_sponge, glass, lapis_ore, deepslate_lapis_ore, lapis_block, sandstone, chiseled_sandstone, cut_sandstone, note_block, cobweb, short_grass, fern, dead_bush, bush, red_shrub, short_dry_grass, tall_dry_grass, seagrass, tall_seagrass, white_wool, orange_wool, magenta_wool, light_blue_wool, yellow_wool, lime_wool, pink_wool, gray_wool, light_gray_wool, cyan_wool, purple_wool, blue_wool, brown_wool, green_wool, red_wool, black_wool, white_wool_slab, orange_wool_slab, magenta_wool_slab, light_blue_wool_slab, yellow_wool_slab, lime_wool_slab, pink_wool_slab, gray_wool_slab, light_gray_wool_slab, cyan_wool_slab, purple_wool_slab, blue_wool_slab, brown_wool_slab, green_wool_slab, red_wool_slab, black_wool_slab, dandelion, golden_dandelion, torchflower, poppy, blue_orchid, allium, azure_bluet, red_tulip, orange_tulip, white_tulip, pink_tulip, oxeye_daisy, cornflower, wither_rose, lily_of_the_valley, brown_mushroom, red_mushroom, gold_block, iron_block, bricks, tnt, bookshelf, mossy_cobblestone, obsidian, torch, fire, soul_fire, spawner, diamond_ore, deepslate_diamond_ore, diamond_block, crafting_table, wheat, farmland, stone_pressure_plate, oak_pressure_plate, spruce_pressure_plate, birch_pressure_plate, jungle_pressure_plate, acacia_pressure_plate, cherry_pressure_plate, dark_oak_pressure_plate, pale_oak_pressure_plate, mangrove_pressure_plate, poplar_pressure_plate, bamboo_pressure_plate, redstone_ore, deepslate_redstone_ore, redstone_torch, snow, ice, snow_block, cactus, cactus_flower, clay, sugar_cane, jukebox, netherrack, soul_sand, soul_soil, soul_torch, copper_torch, glowstone, cake, white_stained_glass, orange_stained_glass, magenta_stained_glass, light_blue_stained_glass, yellow_stained_glass, lime_stained_glass, pink_stained_glass, gray_stained_glass, light_gray_stained_glass, cyan_stained_glass, purple_stained_glass, blue_stained_glass, brown_stained_glass, green_stained_glass, red_stained_glass, black_stained_glass, stone_bricks, mossy_stone_bricks, cracked_stone_bricks, chiseled_stone_bricks, packed_mud, mud_bricks, infested_stone, infested_cobblestone, infested_stone_bricks, infested_mossy_stone_bricks, infested_cracked_stone_bricks, infested_chiseled_stone_bricks, pumpkin, melon, pumpkin_stem, melon_stem, mycelium, lily_pad, resin_block, resin_bricks, resin_brick_slab, chiseled_resin_bricks, nether_bricks, nether_wart, enchanting_table, brewing_stand, cauldron, water_cauldron, lava_cauldron, powder_snow_cauldron, end_portal, end_stone, dragon_egg, redstone_lamp, emerald_ore, deepslate_emerald_ore, emerald_block, beacon, flower_pot, potted_torchflower, potted_oak_sapling, potted_spruce_sapling, potted_birch_sapling, potted_jungle_sapling, potted_acacia_sapling, potted_cherry_sapling, potted_dark_oak_sapling, potted_pale_oak_sapling, potted_poplar_sapling, potted_mangrove_propagule, potted_fern, potted_dandelion, potted_golden_dandelion, potted_poppy, potted_blue_orchid, potted_allium, potted_azure_bluet, potted_red_tulip, potted_orange_tulip, potted_white_tulip, potted_pink_tulip, potted_oxeye_daisy, potted_cornflower, potted_lily_of_the_valley, potted_wither_rose, potted_red_mushroom, potted_brown_mushroom, potted_dead_bush, potted_cactus, carrots, potatoes, light_weighted_pressure_plate, heavy_weighted_pressure_plate, daylight_detector, redstone_block, nether_quartz_ore, quartz_block, chiseled_quartz_block, white_terracotta, orange_terracotta, magenta_terracotta, light_blue_terracotta, yellow_terracotta, lime_terracotta, pink_terracotta, gray_terracotta, light_gray_terracotta, cyan_terracotta, purple_terracotta, blue_terracotta, brown_terracotta, green_terracotta, red_terracotta, black_terracotta, slime_block, barrier, light, prismarine, prismarine_bricks, dark_prismarine, prismarine_slab, prismarine_brick_slab, dark_prismarine_slab, sea_lantern, white_carpet, orange_carpet, magenta_carpet, light_blue_carpet, yellow_carpet, lime_carpet, pink_carpet, gray_carpet, light_gray_carpet, cyan_carpet, purple_carpet, blue_carpet, brown_carpet, green_carpet, red_carpet, black_carpet, terracotta, coal_block, packed_ice, sunflower, lilac, rose_bush, peony, tall_grass, large_fern, red_sandstone, chiseled_red_sandstone, cut_red_sandstone, oak_slab, spruce_slab, birch_slab, jungle_slab, acacia_slab, cherry_slab, dark_oak_slab, pale_oak_slab, mangrove_slab, poplar_slab, bamboo_slab, bamboo_mosaic_slab, stone_slab, sandstone_slab, cut_sandstone_slab, petrified_oak_slab, cobblestone_slab, brick_slab, stone_brick_slab, mud_brick_slab, nether_brick_slab, quartz_slab, red_sandstone_slab, cut_red_sandstone_slab, smooth_stone, smooth_stone_slab, smooth_sandstone, smooth_quartz, smooth_red_sandstone, chorus_plant, chorus_flower, purpur_block, purpur_slab, end_stone_bricks, torchflower_crop, pitcher_crop, pitcher_plant, beetroots, dirt_path, end_gateway, frosted_ice, magma_block, nether_wart_block, red_nether_bricks, structure_void, white_concrete, orange_concrete, magenta_concrete, light_blue_concrete, yellow_concrete, lime_concrete, pink_concrete, gray_concrete, light_gray_concrete, cyan_concrete, purple_concrete, blue_concrete, brown_concrete, green_concrete, red_concrete, black_concrete, white_concrete_slab, orange_concrete_slab, magenta_concrete_slab, light_blue_concrete_slab, yellow_concrete_slab, lime_concrete_slab, pink_concrete_slab, gray_concrete_slab, light_gray_concrete_slab, cyan_concrete_slab, purple_concrete_slab, blue_concrete_slab, brown_concrete_slab, green_concrete_slab, red_concrete_slab, black_concrete_slab, white_concrete_powder, orange_concrete_powder, magenta_concrete_powder, light_blue_concrete_powder, yellow_concrete_powder, lime_concrete_powder, pink_concrete_powder, gray_concrete_powder, light_gray_concrete_powder, cyan_concrete_powder, purple_concrete_powder, blue_concrete_powder, brown_concrete_powder, green_concrete_powder, red_concrete_powder, black_concrete_powder, kelp, kelp_plant, dried_kelp_block, turtle_egg, sniffer_egg, dead_tube_coral_block, dead_brain_coral_block, dead_bubble_coral_block, dead_fire_coral_block, dead_horn_coral_block, tube_coral_block, brain_coral_block, bubble_coral_block, fire_coral_block, horn_coral_block, dead_tube_coral, dead_brain_coral, dead_bubble_coral, dead_fire_coral, dead_horn_coral, tube_coral, brain_coral, bubble_coral, fire_coral, horn_coral, dead_tube_coral_fan, dead_brain_coral_fan, dead_bubble_coral_fan, dead_fire_coral_fan, dead_horn_coral_fan, tube_coral_fan, brain_coral_fan, bubble_coral_fan, fire_coral_fan, horn_coral_fan, sea_pickle, blue_ice, conduit, bamboo_sapling, bamboo, potted_bamboo, void_air, cave_air, bubble_column, polished_granite_slab, smooth_red_sandstone_slab, mossy_stone_brick_slab, polished_diorite_slab, mossy_cobblestone_slab, end_stone_brick_slab, smooth_sandstone_slab, smooth_quartz_slab, granite_slab, andesite_slab, red_nether_brick_slab, polished_andesite_slab, diorite_slab, scaffolding, cartography_table, fletching_table, smithing_table, lantern, soul_lantern, copper_lantern, exposed_copper_lantern, weathered_copper_lantern, oxidized_copper_lantern, waxed_copper_lantern, waxed_exposed_copper_lantern, waxed_weathered_copper_lantern, waxed_oxidized_copper_lantern, sweet_berry_bush, warped_nylium, warped_fungus, warped_wart_block, warped_roots, nether_sprouts, crimson_nylium, crimson_fungus, shroomlight, weeping_vines, weeping_vines_plant, twisting_vines, twisting_vines_plant, crimson_roots, crimson_planks, warped_planks, crimson_slab, warped_slab, crimson_pressure_plate, warped_pressure_plate, structure_block, test_block, test_instance_block, composter, target, honey_block, honeycomb_block, netherite_block, ancient_debris, crying_obsidian, respawn_anchor, potted_crimson_fungus, potted_warped_fungus, potted_crimson_roots, potted_warped_roots, lodestone, blackstone, blackstone_slab, polished_blackstone, polished_blackstone_bricks, cracked_polished_blackstone_bricks, chiseled_polished_blackstone, polished_blackstone_brick_slab, gilded_blackstone, polished_blackstone_slab, polished_blackstone_pressure_plate, chiseled_nether_bricks, cracked_nether_bricks, quartz_bricks, candle, white_candle, orange_candle, magenta_candle, light_blue_candle, yellow_candle, lime_candle, pink_candle, gray_candle, light_gray_candle, cyan_candle, purple_candle, blue_candle, brown_candle, green_candle, red_candle, black_candle, candle_cake, white_candle_cake, orange_candle_cake, magenta_candle_cake, light_blue_candle_cake, yellow_candle_cake, lime_candle_cake, pink_candle_cake, gray_candle_cake, light_gray_candle_cake, cyan_candle_cake, purple_candle_cake, blue_candle_cake, brown_candle_cake, green_candle_cake, red_candle_cake, black_candle_cake, amethyst_block, budding_amethyst, tuff, tuff_slab, polished_tuff, polished_tuff_slab, chiseled_tuff, tuff_bricks, tuff_brick_slab, chiseled_tuff_bricks, sulfur, potent_sulfur, sulfur_slab, polished_sulfur, polished_sulfur_slab, sulfur_bricks, sulfur_brick_slab, chiseled_sulfur, cinnabar, cinnabar_slab, polished_cinnabar, polished_cinnabar_slab, cinnabar_bricks, cinnabar_brick_slab, chiseled_cinnabar, calcite, tinted_glass, powder_snow, sculk_sensor, sculk, sculk_catalyst, sculk_shrieker, copper_block, exposed_copper, weathered_copper, oxidized_copper, waxed_copper_block, waxed_exposed_copper, waxed_weathered_copper, waxed_oxidized_copper, copper_ore, deepslate_copper_ore, cut_copper, exposed_cut_copper, weathered_cut_copper, oxidized_cut_copper, waxed_cut_copper, waxed_exposed_cut_copper, waxed_weathered_cut_copper, waxed_oxidized_cut_copper, chiseled_copper, exposed_chiseled_copper, weathered_chiseled_copper, oxidized_chiseled_copper, waxed_chiseled_copper, waxed_exposed_chiseled_copper, waxed_weathered_chiseled_copper, waxed_oxidized_chiseled_copper, cut_copper_slab, exposed_cut_copper_slab, weathered_cut_copper_slab, oxidized_cut_copper_slab, waxed_cut_copper_slab, waxed_exposed_cut_copper_slab, waxed_weathered_cut_copper_slab, waxed_oxidized_cut_copper_slab, copper_grate, exposed_copper_grate, weathered_copper_grate, oxidized_copper_grate, waxed_copper_grate, waxed_exposed_copper_grate, waxed_weathered_copper_grate, waxed_oxidized_copper_grate, copper_bulb, exposed_copper_bulb, weathered_copper_bulb, oxidized_copper_bulb, waxed_copper_bulb, waxed_exposed_copper_bulb, waxed_weathered_copper_bulb, waxed_oxidized_copper_bulb, dripstone_block, pointed_dripstone, sulfur_spike, cave_vines, cave_vines_plant, spore_blossom, azalea, flowering_azalea, moss_carpet, moss_block, hanging_roots, rooted_dirt, mud, cobbled_deepslate, cobbled_deepslate_slab, polished_deepslate, polished_deepslate_slab, deepslate_tiles, deepslate_tile_slab, deepslate_bricks, deepslate_brick_slab, chiseled_deepslate, cracked_deepslate_bricks, cracked_deepslate_tiles, smooth_basalt, raw_iron_block, raw_copper_block, raw_gold_block, potted_azalea_bush, potted_flowering_azalea_bush, frogspawn, reinforced_deepslate, trial_spawner, heavy_core, pale_moss_block, pale_hanging_moss, open_eyeblossom, closed_eyeblossom, potted_open_eyeblossom, potted_closed_eyeblossom, firefly_bush

**T1 / `RotatedPillarBlock` — 70 blocks**; rewritten: `axis`; left untouched: `waterlogged`

pale_oak_wood, oak_log, spruce_log, birch_log, jungle_log, acacia_log, cherry_log, dark_oak_log, pale_oak_log, mangrove_log, poplar_log, muddy_mangrove_roots, bamboo_block, stripped_spruce_log, stripped_birch_log, stripped_jungle_log, stripped_acacia_log, stripped_cherry_log, stripped_dark_oak_log, stripped_pale_oak_log, stripped_oak_log, stripped_mangrove_log, stripped_poplar_log, stripped_bamboo_block, oak_wood, spruce_wood, birch_wood, jungle_wood, acacia_wood, cherry_wood, dark_oak_wood, mangrove_wood, poplar_wood, stripped_oak_wood, stripped_spruce_wood, stripped_birch_wood, stripped_jungle_wood, stripped_acacia_wood, stripped_cherry_wood, stripped_dark_oak_wood, stripped_pale_oak_wood, stripped_mangrove_wood, stripped_poplar_wood, basalt, polished_basalt, iron_chain, copper_chain, exposed_copper_chain, weathered_copper_chain, oxidized_copper_chain, waxed_copper_chain, waxed_exposed_copper_chain, waxed_weathered_copper_chain, waxed_oxidized_copper_chain, quartz_pillar, hay_block, purpur_pillar, bone_block, warped_stem, stripped_warped_stem, warped_hyphae, stripped_warped_hyphae, crimson_stem, stripped_crimson_stem, crimson_hyphae, stripped_crimson_hyphae, deepslate, ochre_froglight, verdant_froglight, pearlescent_froglight

**T1 / `CreakingHeartBlock` — 1 block**; rewritten: `axis`; left untouched: `creaking_heart_state, natural`

creaking_heart

**T1 / `NetherPortalBlock` — 1 block**; rewritten: `axis`; left untouched: —

nether_portal

**T1 / `InfestedRotatedPillarBlock` — 1 block**; rewritten: `axis`; left untouched: —

infested_deepslate

**T2 / `DispenserBlock` — 2 blocks**; rewritten: `facing`; left untouched: `triggered`

dispenser, dropper

**T2 / `HorizontalDirectionalBlock` — 94 blocks**; rewritten: `facing`; left untouched: `age, delay, face, half, hydration, in_wall, locked, mode, occupied, open, part, powered, tilt, waterlogged`

white_bed, orange_bed, magenta_bed, light_blue_bed, yellow_bed, lime_bed, pink_bed, gray_bed, light_gray_bed, cyan_bed, purple_bed, blue_bed, brown_bed, green_bed, red_bed, black_bed, straw_bed, lever, stone_button, carved_pumpkin, jack_o_lantern, repeater, oak_trapdoor, spruce_trapdoor, birch_trapdoor, jungle_trapdoor, acacia_trapdoor, cherry_trapdoor, dark_oak_trapdoor, pale_oak_trapdoor, mangrove_trapdoor, poplar_trapdoor, bamboo_trapdoor, oak_fence_gate, cocoa, shelf_mushroom, oak_button, spruce_button, birch_button, jungle_button, acacia_button, cherry_button, dark_oak_button, pale_oak_button, mangrove_button, poplar_button, bamboo_button, comparator, iron_trapdoor, spruce_fence_gate, birch_fence_gate, jungle_fence_gate, acacia_fence_gate, cherry_fence_gate, dark_oak_fence_gate, pale_oak_fence_gate, mangrove_fence_gate, poplar_fence_gate, bamboo_fence_gate, white_glazed_terracotta, orange_glazed_terracotta, magenta_glazed_terracotta, light_blue_glazed_terracotta, yellow_glazed_terracotta, lime_glazed_terracotta, pink_glazed_terracotta, gray_glazed_terracotta, light_gray_glazed_terracotta, cyan_glazed_terracotta, purple_glazed_terracotta, blue_glazed_terracotta, brown_glazed_terracotta, green_glazed_terracotta, red_glazed_terracotta, black_glazed_terracotta, dried_ghast, loom, crimson_trapdoor, warped_trapdoor, crimson_fence_gate, warped_fence_gate, crimson_button, warped_button, polished_blackstone_button, copper_trapdoor, exposed_copper_trapdoor, weathered_copper_trapdoor, oxidized_copper_trapdoor, waxed_copper_trapdoor, waxed_exposed_copper_trapdoor, waxed_weathered_copper_trapdoor, waxed_oxidized_copper_trapdoor, big_dripleaf, big_dripleaf_stem

**T2 / `PistonBaseBlock` — 2 blocks**; rewritten: `facing`; left untouched: `extended`

sticky_piston, piston

**T2 / `PistonHeadBlock` — 1 block**; rewritten: `facing`; left untouched: `short, type`

piston_head

**T2 / `MovingPistonBlock` — 1 block**; rewritten: `facing`; left untouched: `type`

moving_piston

**T2 / `ChiseledBookShelfBlock` — 1 block**; rewritten: `facing`; left untouched: `slot_0_occupied, slot_1_occupied, slot_2_occupied, slot_3_occupied, slot_4_occupied, slot_5_occupied`

chiseled_bookshelf

**T2 / `ShelfBlock` — 13 blocks**; rewritten: `facing`; left untouched: `powered, side_chain, waterlogged`

acacia_shelf, bamboo_shelf, birch_shelf, cherry_shelf, crimson_shelf, dark_oak_shelf, jungle_shelf, mangrove_shelf, oak_shelf, pale_oak_shelf, poplar_shelf, spruce_shelf, warped_shelf

**T2 / `WallTorchBlock` — 3 blocks**; rewritten: `facing`; left untouched: —

wall_torch, soul_wall_torch, copper_wall_torch

**T2 / `ChestBlock` — 10 blocks**; rewritten: `facing`; left untouched: `type, waterlogged`

chest, trapped_chest, copper_chest, exposed_copper_chest, weathered_copper_chest, oxidized_copper_chest, waxed_copper_chest, waxed_exposed_copper_chest, waxed_weathered_copper_chest, waxed_oxidized_copper_chest

**T2 / `AbstractFurnaceBlock` — 3 blocks**; rewritten: `facing`; left untouched: `lit`

furnace, smoker, blast_furnace

**T2 / `LadderBlock` — 1 block**; rewritten: `facing`; left untouched: `waterlogged`

ladder

**T2 / `WallSignBlock` — 13 blocks**; rewritten: `facing`; left untouched: `waterlogged`

oak_wall_sign, spruce_wall_sign, birch_wall_sign, acacia_wall_sign, cherry_wall_sign, jungle_wall_sign, dark_oak_wall_sign, pale_oak_wall_sign, mangrove_wall_sign, poplar_wall_sign, bamboo_wall_sign, crimson_wall_sign, warped_wall_sign

**T2 / `WallHangingSignBlock` — 13 blocks**; rewritten: `facing`; left untouched: `waterlogged`

oak_wall_hanging_sign, spruce_wall_hanging_sign, birch_wall_hanging_sign, acacia_wall_hanging_sign, cherry_wall_hanging_sign, jungle_wall_hanging_sign, dark_oak_wall_hanging_sign, pale_oak_wall_hanging_sign, mangrove_wall_hanging_sign, poplar_wall_hanging_sign, crimson_wall_hanging_sign, warped_wall_hanging_sign, bamboo_wall_hanging_sign

**T2 / `RedstoneWallTorchBlock` — 1 block**; rewritten: `facing`; left untouched: `lit`

redstone_wall_torch

**T2 / `AttachedStemBlock` — 2 blocks**; rewritten: `facing`; left untouched: —

attached_pumpkin_stem, attached_melon_stem

**T2 / `EndPortalFrameBlock` — 1 block**; rewritten: `facing`; left untouched: `eye`

end_portal_frame

**T2 / `EnderChestBlock` — 1 block**; rewritten: `facing`; left untouched: `waterlogged`

ender_chest

**T2 / `TripWireHookBlock` — 1 block**; rewritten: `facing`; left untouched: `attached, powered`

tripwire_hook

**T2 / `CommandBlock` — 3 blocks**; rewritten: `facing`; left untouched: `conditional`

command_block, repeating_command_block, chain_command_block

**T2 / `WallSkullBlock` — 7 blocks**; rewritten: `facing`; left untouched: `powered`

skeleton_wall_skull, wither_skeleton_wall_skull, zombie_wall_head, player_wall_head, creeper_wall_head, dragon_wall_head, piglin_wall_head

**T2 / `HopperBlock` — 1 block**; rewritten: `facing`; left untouched: `enabled`

hopper

**T2 / `WallBannerBlock` — 16 blocks**; rewritten: `facing`; left untouched: —

white_wall_banner, orange_wall_banner, magenta_wall_banner, light_blue_wall_banner, yellow_wall_banner, lime_wall_banner, pink_wall_banner, gray_wall_banner, light_gray_wall_banner, cyan_wall_banner, purple_wall_banner, blue_wall_banner, brown_wall_banner, green_wall_banner, red_wall_banner, black_wall_banner

**T2 / `RodBlock` — 9 blocks**; rewritten: `facing`; left untouched: `powered, waterlogged`

end_rod, lightning_rod, exposed_lightning_rod, weathered_lightning_rod, oxidized_lightning_rod, waxed_lightning_rod, waxed_exposed_lightning_rod, waxed_weathered_lightning_rod, waxed_oxidized_lightning_rod

**T2 / `ObserverBlock` — 1 block**; rewritten: `facing`; left untouched: `powered`

observer

**T2 / `ShulkerBoxBlock` — 17 blocks**; rewritten: `facing`; left untouched: —

shulker_box, white_shulker_box, orange_shulker_box, magenta_shulker_box, light_blue_shulker_box, yellow_shulker_box, lime_shulker_box, pink_shulker_box, gray_shulker_box, light_gray_shulker_box, cyan_shulker_box, purple_shulker_box, blue_shulker_box, brown_shulker_box, green_shulker_box, red_shulker_box, black_shulker_box

**T2 / `BaseCoralWallFanBlock` — 10 blocks**; rewritten: `facing`; left untouched: `waterlogged`

dead_tube_coral_wall_fan, dead_brain_coral_wall_fan, dead_bubble_coral_wall_fan, dead_fire_coral_wall_fan, dead_horn_coral_wall_fan, tube_coral_wall_fan, brain_coral_wall_fan, bubble_coral_wall_fan, fire_coral_wall_fan, horn_coral_wall_fan

**T2 / `BarrelBlock` — 1 block**; rewritten: `facing`; left untouched: `open`

barrel

**T2 / `GrindstoneBlock` — 1 block**; rewritten: `facing`; left untouched: `face`

grindstone

**T2 / `LecternBlock` — 1 block**; rewritten: `facing`; left untouched: `has_book, powered`

lectern

**T2 / `StonecutterBlock` — 1 block**; rewritten: `facing`; left untouched: —

stonecutter

**T2 / `BellBlock` — 1 block**; rewritten: `facing`; left untouched: `attachment, powered`

bell

**T2 / `CampfireBlock` — 2 blocks**; rewritten: `facing`; left untouched: `lit, signal_fire, waterlogged`

campfire, soul_campfire

**T2 / `BeehiveBlock` — 2 blocks**; rewritten: `facing`; left untouched: `honey_level`

bee_nest, beehive

**T2 / `AmethystClusterBlock` — 4 blocks**; rewritten: `facing`; left untouched: `waterlogged`

amethyst_cluster, large_amethyst_bud, medium_amethyst_bud, small_amethyst_bud

**T2 / `CalibratedSculkSensorBlock` — 1 block**; rewritten: `facing`; left untouched: `power, sculk_sensor_phase, waterlogged`

calibrated_sculk_sensor

**T2 / `CopperGolemStatueBlock` — 8 blocks**; rewritten: `facing`; left untouched: `copper_golem_pose, waterlogged`

copper_golem_statue, exposed_copper_golem_statue, weathered_copper_golem_statue, oxidized_copper_golem_statue, waxed_copper_golem_statue, waxed_exposed_copper_golem_statue, waxed_weathered_copper_golem_statue, waxed_oxidized_copper_golem_statue

**T2 / `FlowerBedBlock` — 2 blocks**; rewritten: `facing`; left untouched: `flower_amount`

pink_petals, wildflowers

**T2 / `LeafLitterBlock` — 1 block**; rewritten: `facing`; left untouched: `segment_amount`

leaf_litter

**T2 / `SmallDripleafBlock` — 1 block**; rewritten: `facing`; left untouched: `half, waterlogged`

small_dripleaf

**T2 / `DecoratedPotBlock` — 1 block**; rewritten: `facing`; left untouched: `cracked, waterlogged`

decorated_pot

**T2 / `VaultBlock` — 1 block**; rewritten: `facing`; left untouched: `ominous, vault_state`

vault

**T3 / `AnvilBlock` — 3 blocks**; rewritten: `facing`; left untouched: —

anvil, chipped_anvil, damaged_anvil

**T4 / `DoorBlock` — 22 blocks**; rewritten: `facing, hinge`; left untouched: `half, open, powered`

oak_door, iron_door, spruce_door, birch_door, jungle_door, acacia_door, cherry_door, dark_oak_door, pale_oak_door, mangrove_door, poplar_door, bamboo_door, crimson_door, warped_door, copper_door, exposed_copper_door, weathered_copper_door, oxidized_copper_door, waxed_copper_door, waxed_exposed_copper_door, waxed_weathered_copper_door, waxed_oxidized_copper_door

**T5 / `StairBlock` — 97 blocks**; rewritten: `facing, shape`; left untouched: `half, waterlogged`

white_wool_stairs, orange_wool_stairs, magenta_wool_stairs, light_blue_wool_stairs, yellow_wool_stairs, lime_wool_stairs, pink_wool_stairs, gray_wool_stairs, light_gray_wool_stairs, cyan_wool_stairs, purple_wool_stairs, blue_wool_stairs, brown_wool_stairs, green_wool_stairs, red_wool_stairs, black_wool_stairs, oak_stairs, cobblestone_stairs, brick_stairs, stone_brick_stairs, mud_brick_stairs, resin_brick_stairs, nether_brick_stairs, sandstone_stairs, spruce_stairs, birch_stairs, jungle_stairs, quartz_stairs, acacia_stairs, cherry_stairs, dark_oak_stairs, pale_oak_stairs, mangrove_stairs, poplar_stairs, bamboo_stairs, bamboo_mosaic_stairs, prismarine_stairs, prismarine_brick_stairs, dark_prismarine_stairs, red_sandstone_stairs, purpur_stairs, white_concrete_stairs, orange_concrete_stairs, magenta_concrete_stairs, light_blue_concrete_stairs, yellow_concrete_stairs, lime_concrete_stairs, pink_concrete_stairs, gray_concrete_stairs, light_gray_concrete_stairs, cyan_concrete_stairs, purple_concrete_stairs, blue_concrete_stairs, brown_concrete_stairs, green_concrete_stairs, red_concrete_stairs, black_concrete_stairs, polished_granite_stairs, smooth_red_sandstone_stairs, mossy_stone_brick_stairs, polished_diorite_stairs, mossy_cobblestone_stairs, end_stone_brick_stairs, stone_stairs, smooth_sandstone_stairs, smooth_quartz_stairs, granite_stairs, andesite_stairs, red_nether_brick_stairs, polished_andesite_stairs, diorite_stairs, crimson_stairs, warped_stairs, blackstone_stairs, polished_blackstone_brick_stairs, polished_blackstone_stairs, tuff_stairs, polished_tuff_stairs, tuff_brick_stairs, sulfur_stairs, polished_sulfur_stairs, sulfur_brick_stairs, cinnabar_stairs, polished_cinnabar_stairs, cinnabar_brick_stairs, cut_copper_stairs, exposed_cut_copper_stairs, weathered_cut_copper_stairs, oxidized_cut_copper_stairs, waxed_cut_copper_stairs, waxed_exposed_cut_copper_stairs, waxed_weathered_cut_copper_stairs, waxed_oxidized_cut_copper_stairs, cobbled_deepslate_stairs, polished_deepslate_stairs, deepslate_tile_stairs, deepslate_brick_stairs

**T6 / `PoweredRailBlock` — 2 blocks**; rewritten: `shape`; left untouched: `powered, waterlogged`

powered_rail, activator_rail

**T6 / `DetectorRailBlock` — 1 block**; rewritten: `shape`; left untouched: `powered, waterlogged`

detector_rail

**T6 / `RailBlock` — 1 block**; rewritten: `shape`; left untouched: `waterlogged`

rail

**T7 / `StandingSignBlock` — 13 blocks**; rewritten: `rotation`; left untouched: `waterlogged`

oak_sign, spruce_sign, birch_sign, acacia_sign, cherry_sign, jungle_sign, dark_oak_sign, pale_oak_sign, mangrove_sign, poplar_sign, bamboo_sign, crimson_sign, warped_sign

**T7 / `CeilingHangingSignBlock` — 13 blocks**; rewritten: `rotation`; left untouched: `attached, waterlogged`

oak_hanging_sign, spruce_hanging_sign, birch_hanging_sign, acacia_hanging_sign, cherry_hanging_sign, jungle_hanging_sign, dark_oak_hanging_sign, pale_oak_hanging_sign, crimson_hanging_sign, warped_hanging_sign, mangrove_hanging_sign, poplar_hanging_sign, bamboo_hanging_sign

**T7 / `SkullBlock` — 7 blocks**; rewritten: `rotation`; left untouched: `powered`

skeleton_skull, wither_skeleton_skull, zombie_head, player_head, creeper_head, dragon_head, piglin_head

**T7 / `BannerBlock` — 16 blocks**; rewritten: `rotation`; left untouched: —

white_banner, orange_banner, magenta_banner, light_blue_banner, yellow_banner, lime_banner, pink_banner, gray_banner, light_gray_banner, cyan_banner, purple_banner, blue_banner, brown_banner, green_banner, red_banner, black_banner

**T8 / `RedstoneWireBlock` — 1 block**; rewritten: `north, east, south, west`; left untouched: `power`

redstone_wire

**T8 / `CrossCollisionBlock` — 40 blocks**; rewritten: `north, east, south, west`; left untouched: `waterlogged`

oak_fence, iron_bars, copper_bars, exposed_copper_bars, weathered_copper_bars, oxidized_copper_bars, waxed_copper_bars, waxed_exposed_copper_bars, waxed_weathered_copper_bars, waxed_oxidized_copper_bars, glass_pane, nether_brick_fence, white_stained_glass_pane, orange_stained_glass_pane, magenta_stained_glass_pane, light_blue_stained_glass_pane, yellow_stained_glass_pane, lime_stained_glass_pane, pink_stained_glass_pane, gray_stained_glass_pane, light_gray_stained_glass_pane, cyan_stained_glass_pane, purple_stained_glass_pane, blue_stained_glass_pane, brown_stained_glass_pane, green_stained_glass_pane, red_stained_glass_pane, black_stained_glass_pane, spruce_fence, birch_fence, jungle_fence, acacia_fence, cherry_fence, dark_oak_fence, pale_oak_fence, mangrove_fence, poplar_fence, bamboo_fence, crimson_fence, warped_fence

**T8 / `HugeMushroomBlock` — 3 blocks**; rewritten: `north, east, south, west`; left untouched: `down, up`

brown_mushroom_block, red_mushroom_block, mushroom_stem

**T8 / `VineBlock` — 1 block**; rewritten: `north, east, south, west`; left untouched: `up`

vine

**T8 / `MultifaceBlock` — 3 blocks**; rewritten: `north, east, south, west`; left untouched: `down, up, waterlogged`

glow_lichen, resin_clump, sculk_vein

**T8 / `WallBlock` — 32 blocks**; rewritten: `north, east, south, west`; left untouched: `up, waterlogged`

resin_brick_wall, cobblestone_wall, mossy_cobblestone_wall, brick_wall, prismarine_wall, red_sandstone_wall, mossy_stone_brick_wall, granite_wall, stone_brick_wall, mud_brick_wall, nether_brick_wall, andesite_wall, red_nether_brick_wall, sandstone_wall, end_stone_brick_wall, diorite_wall, blackstone_wall, polished_blackstone_brick_wall, polished_blackstone_wall, tuff_wall, polished_tuff_wall, tuff_brick_wall, sulfur_wall, polished_sulfur_wall, sulfur_brick_wall, cinnabar_wall, polished_cinnabar_wall, cinnabar_brick_wall, cobbled_deepslate_wall, polished_deepslate_wall, deepslate_tile_wall, deepslate_brick_wall

**T8 / `TripWireBlock` — 1 block**; rewritten: `north, east, south, west`; left untouched: `attached, disarmed, powered`

tripwire

**T8 / `MossyCarpetBlock` — 1 block**; rewritten: `north, east, south, west`; left untouched: `bottom`

pale_moss_carpet

**T9 / `JigsawBlock` — 1 block**; rewritten: `orientation`; left untouched: —

jigsaw

**T9 / `CrafterBlock` — 1 block**; rewritten: `orientation`; left untouched: `crafting, triggered`

crafter


## Oracle probe points

Public names in the 26.3 client (for the oracle only; implementations must not depend on them):
`net.minecraft.world.level.block.Rotation` (`NONE`, `CLOCKWISE_90`, `CLOCKWISE_180`, `COUNTERCLOCKWISE_90`),
`net.minecraft.world.level.block.Mirror` (`NONE`, `LEFT_RIGHT`, `FRONT_BACK`), `BlockBehaviour.BlockStateBase#rotate(Rotation)` and
`#mirror(Mirror)`, `Block.BLOCK_STATE_REGISTRY` (`IdMapper#byId`, `#getId`, `#size`),
`net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplate` (placement order),
`net.minecraft.world.level.levelgen.structure.StructurePiece#setOrientation`.
The `BlockTransforms` oracle (ZBT1, `docs/specs/parity/block-transforms.md`) dumps, for each state id, the id of the
state after each of the 12 (mirror, rotation) pairs, mirror applied first.

Spot checks (values read from the game, usable as unit tests):

- `oak_stairs[facing=north,half=bottom,shape=straight,waterlogged=false]` → `clockwise_90`: `facing=east`;
  `clockwise_180`: `south`; `counterclockwise_90`: `west`; `left_right`: `south` (shape unchanged); `front_back`:
  unchanged (z-axis stair, §4).
- `rail[shape=north_east,waterlogged=false]` → `left_right`: `south_east`; `front_back`: `north_west`; `clockwise_90`:
  `south_east`; `clockwise_180`: `south_west`; `counterclockwise_90`: `north_west`;
  `left_right` then `clockwise_90`: `south_west`.
- `chest[facing=north,type=left,waterlogged=false]` → `left_right`: `facing=south`, `type=left` (type kept);
  `chest[facing=east,type=right]` → `front_back`: `facing=west`, `type=right`; `left_right`: unchanged.
- `oak_sign[rotation=r]`: `cw90` r+4, `cw180` r+8, `ccw90` r+12 (mod 16); `left_right` 8−r (mod 16), `front_back` −r (mod 16):
  e.g. r=1 → 5, 9, 13, 7, 15; r=3 → 7, 11, 15, 5, 13; r=15 → 3, 7, 11, 9, 1.
- `red_bed[facing=north,part=head]` → `cw90`: `facing=east`, `part=head`; `left_right`: `facing=south`, `part=head`;
  `red_bed[facing=east,part=foot]` → `front_back`: `facing=west`, `part=foot`; `left_right`: unchanged.
- `oak_door[facing=north,half=lower,hinge=left,open=false,powered=false]` → `left_right`: `facing=south`, `hinge=right`;
  `front_back`: `facing=north`, `hinge=right`; `left_right` then `clockwise_90`: `facing=west`, `hinge=right`.
- `jigsaw[orientation=down_east]` → `cw90`: `down_south`; `front_back`: `down_west`.

## Open questions

1. **[oracle] Whole-table equality.** *Resolved:* the `block-transforms` suite compares all 35,723 × 12 results with
   Zenith's rule application (`zenith-blocks` `BlockTransforms`) and passes 428,676 / 428,676 checks, with no
   difference in any of the ten rules, so §2–§6 and the Appendix are complete for 26.3.
2. **[oracle] Multi-face flags.** The multi-face rule has "can rotate / can mirror" guards that depend on which of the six
   faces a block supports; glow lichen, resin clump and sculk vein all support all six, so the guards never trigger in
   26.3. A later multi-face block that omits a horizontal face would behave differently (no rotation; mirror only when
   an even number of X- or Z-axis faces exists). The guard path is not exercised by any 26.3 state.
3. **[oracle] Placement order outside the three call sites of §5.** Other structure code (the woodland mansion pieces,
   some legacy pieces that build states from facings by hand) never calls this operation on a state; whether any such
   caller applies rotation before mirror cannot be checked by this suite. The structure suites with rotated and
   mirrored templates are the check.
4. **[oracle] Processor interaction.** Item 1 of §5 says processors run on the untransformed state and mirror+rotate act
   on their output. This is about call order in the placement code; only the structure suites with processors that
   replace blocks (jigsaw final-state, rules processors) can confirm it end to end.
5. **(implementer)** *Done:* Zenith computes the table from §2–§6 at start-up (rule per block from the Appendix, which
   is embedded and parsed at compile time) and keeps flat arrays; `rotate`, `mirror` and `transform` (mirror, then
   rotate) are array reads. A block that the Appendix does not list makes the build fail instead of silently being
   treated as T0, so a later game version must extend the Appendix before it can be loaded.

## Parity status
`block-transforms` (2026-11-25): **428,676/428,676** — every state of 26.3 under all 12 mirror × rotation
combinations identical to the game; the per-combination changed-state counts equal §7's. Placement call order (§5)
and processor interaction are verified by the structure suites.
