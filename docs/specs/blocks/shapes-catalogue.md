# Blocks: shape catalogue — every block family (26.3)

- Verified by suites: `block-facts` (ZBF1 section 6 "Shapes", section 7 "Face facts").
- Sources: local reference reading of the 26.3 client (2026-11-25, reader session); data-generator
  `reports/blocks.json` (state lists). Written in our own words; all numbers are facts needed for parity.
- Related specs: [`shapes.md`](shapes.md) (the shape model, helpers `box`/`column`/`cube`/`boxZ`, rotations, face
  shapes, sturdiness, contexts, offsets), [`state-facts.md`](state-facts.md), [`block-table.md`](block-table.md)
  (assigns every block id to one family key below and gives its flags: `nc` = no collision, `no` = cannot occlude).

## Summary

Each section below is one **shape family** (the key in the `shape` column of `block-table.md`). It lists the six
per-state shapes:

- **Outline** — `getShape` with the given context (the empty context unless stated).
- **Collision** — if not stated: the outline **evaluated with the empty context** when the block has collision (no
  `nc` flag), else EMPTY. A family that states a collision rule uses it **regardless** of the `nc` flag (the flag only
  feeds the default rule).
- **Visual** — if not stated: equal to the family's collision rule (evaluated, not cached).
- **Support** — if not stated: the collision rule with the empty context.
- **Interaction** — if not stated: EMPTY.
- **Occlusion** — if not stated: the outline at the origin with the empty context. It is *stored* only when the
  block can occlude (no `nc`/`no` flag); otherwise the stored occlusion shape and all face occlusion shapes are
  EMPTY (`state-facts.md` §2).

Coordinates are **pixels** (divide by 16), written `(x1,y1,z1)–(x2,y2,z2)`. "∪" is the shape union (`or`), "−" the
ONLY_FIRST join. A shape given "for NORTH" is rotated to other horizontal facings with the `horizontal` family of
`shapes.md` §3.3 (NORTH → itself, EAST = Y90, SOUTH = Y180, WEST = Y270); where the rotated boxes are listed, they are
the result of that rotation. Quarter names: **NW** = x 0–8, z 0–8; **NE** = x 8–16, z 0–8; **SE** = x 8–16,
z 8–16; **SW** = x 0–8, z 8–16. Properties that are not mentioned do not influence shapes (in particular
`waterlogged` never does; several families compute their shape table with `waterlogged` fixed, which is equivalent).

## Inputs and state

Block states (registry §3) and, for the offset families, the block position (shapes.md §5.4). The parity suite uses
the empty getter and empty collision context; the context- and getter-dependent families are listed in shapes.md
§5.3.

## Behavior (ordered rules) — the families

### A. Full cubes and invisible / empty blocks

#### `cube`
Classes: plain blocks and every class without a shape override (see the generated membership list at the end).
- Outline BLOCK; collision BLOCK, or EMPTY for `nc` blocks (cobweb, end gateway); visual = collision; support =
  collision; occlusion BLOCK when the block can occlude (barrier, beacon, ice, frosted ice, slime block, spawner, trial
  spawner, vault, test instance block and mangrove roots cannot).

#### `cube_glass`
Classes `TransparentBlock`, `StainedGlassBlock`, `TintedGlassBlock`, `WaterloggedTransparentBlock` (glass, stained
glass, tinted glass, copper grates, …). As `cube` but **visual = EMPTY**. All are `no` (cannot occlude).

#### `leaves`
Every leaves block. As `cube` (collision BLOCK) but **support = EMPTY**; cannot occlude.

#### `cube_plant`
Two-block plants without their own outline (sunflower, lilac, rose bush, peony, tall grass, large fern, pitcher plant)
and the firefly bush: outline BLOCK (not offset even though most have an offset type), no collision.

#### `empty`
Air, void air, cave air, bubble column: outline EMPTY, collision EMPTY.

#### `fluid`
Water, lava (all 16 `level` values): outline EMPTY; collision per shapes.md §5.3 (EMPTY with the empty context);
visual = that collision rule (EMPTY); support EMPTY.

#### `moving_piston`
Outline EMPTY; collision from the block entity (EMPTY without one); visual EMPTY; support EMPTY.

#### `light`
Light block: outline BLOCK when the context holds the light item, else EMPTY. It has the collision property, but the
default collision rule uses the empty-context outline, so collision (and visual, support) is always EMPTY.

#### `structure_void`
Outline `cube(6)` = (5,5,5)–(11,11,11); no collision.

#### `shulker_box`
All 17 shulker boxes (dynamic shape). Outline and collision: the block entity's current box when one exists, BLOCK
otherwise (always BLOCK in the suite). Support: when a block entity exists and is not closed, the thin face sheet on
the side **opposite** the facing (the `all` rotation family of `boxZ(16, 0, 1)`, i.e. NORTH sheet (0,0,0)–(16,16,1)
etc.); otherwise BLOCK. Cannot occlude.

### B. Slabs, stairs and other "building" families

#### `slab`
- type=bottom: (0,0,0)–(16,8,16); type=top: (0,8,0)–(16,16,16); type=double: BLOCK.

#### `stairs`
Outline = a half-height slab plus the listed quarters of the other half:
- half=bottom: slab (0,0,0)–(16,8,16), quarters at y 8–16; half=top: slab (0,8,0)–(16,16,16), quarters at y 0–8.
- Quarters by `shape` × `facing`:

| shape | north | east | south | west |
|---|---|---|---|---|
| straight | NW, NE | NE, SE | SE, SW | SW, NW |
| outer_left | NW | NE | SE | SW |
| outer_right | NE | SE | SW | NW |
| inner_right | NW, NE, SE | NE, SE, SW | SE, SW, NW | SW, NW, NE |
| inner_left | SW, NW, NE | NW, NE, SE | NE, SE, SW | SE, SW, NW |

(Construction: an "outer" piece = slab + NW quarter; "straight" = outer ∪ Y90(outer); "inner" = straight ∪
Y90(straight); the top-half versions apply INVERT_Y before the horizontal rotation; straight uses the facing's entry,
outer_left the facing's entry, outer_right the clockwise neighbour's entry, inner_right the facing's entry, inner_left
the counter-clockwise neighbour's entry.)

#### `fence`
Booleans `north/east/south/west` select arms.
- Outline: post (6,0,6)–(10,16,10) ∪ arms: N (6,0,0)–(10,16,8), E (8,0,6)–(16,16,10), S (6,0,8)–(10,16,16),
  W (0,0,6)–(8,16,10).
- Collision (and support): the same footprints with height 24: post (6,0,6)–(10,24,10), arms N (6,0,0)–(10,24,8),
  E (8,0,6)–(16,24,10), S (6,0,8)–(10,24,16), W (0,0,6)–(8,24,10).
- Visual: the outline.
- Occlusion (override): post (6,0,6)–(10,16,10) ∪ thin arms N (7,6,0)–(9,15,8), E (8,6,7)–(16,15,9),
  S (7,6,8)–(9,15,16), W (0,6,7)–(8,15,9).

#### `pane`
Iron bars, glass panes, stained glass panes, copper bars (all weathering stages). Outline and collision identical:
post (7,0,7)–(9,16,9) ∪ arms N (7,0,0)–(9,16,8), E (8,0,7)–(16,16,9), S (7,0,8)–(9,16,16), W (0,0,7)–(8,16,9).
**Visual EMPTY.** Cannot occlude.

#### `wall`
`up` (boolean) and `north/east/south/west` ∈ {none, low, tall}.
- Outline: post (if up) (4,0,4)–(12,16,12); each low side: N (5,0,0)–(11,14,11), E (5,0,5)–(16,14,11),
  S (5,0,5)–(11,14,16), W (0,0,5)–(11,14,11); each tall side: the same footprint with y 0–16. up=false with all sides
  none gives **EMPTY**.
- Collision (also visual and support): post (4,0,4)–(12,24,12) (if up) and every non-none side with the low/tall
  footprint but y 0–24 (low and tall are identical in collision).

#### `fence_gate`
`facing` (axis), `open`, `in_wall`.
- Outline: facing north/south (Z axis): (0,0,6)–(16,16,10); east/west (X axis): (6,0,0)–(10,16,16); with in_wall the
  top is 13 instead of 16.
- Collision (and visual): open → EMPTY; closed → Z: (0,0,6)–(16,24,10), X: (6,0,0)–(10,24,16).
- Support (override): open → EMPTY; closed → Z: (0,5,6)–(16,24,10), X: (6,5,0)–(10,24,16).
- Occlusion (override): Z: (0,5,7)–(2,16,9) ∪ (14,5,7)–(16,16,9); X: (7,5,0)–(9,16,2) ∪ (7,5,14)–(9,16,16); with
  in_wall both posts are 3 pixels lower: y 2–13.

#### `door`
`facing`, `open`, `hinge`. Pick a direction: closed → facing; open with hinge=right → facing rotated
counter-clockwise; open with hinge=left → facing rotated clockwise. Shape by that direction: N (0,0,13)–(16,16,16),
E (0,0,0)–(3,16,16), S (0,0,0)–(16,16,3), W (13,0,0)–(16,16,16). Both halves identical; cannot occlude.

#### `trapdoor`
`facing`, `open`, `half`. Open → by facing: N (0,0,13)–(16,16,16), E (0,0,0)–(3,16,16), S (0,0,0)–(16,16,3),
W (13,0,0)–(16,16,16). Closed and half=bottom → (0,0,0)–(16,3,16); closed and half=top → (0,13,0)–(16,16,16).

#### `ladder`
By facing: N (0,0,13)–(16,16,16), E (0,0,0)–(3,16,16), S (0,0,0)–(16,16,3), W (13,0,0)–(16,16,16). Has collision.

#### `chain`
By `axis`: Y (6.5,0,6.5)–(9.5,16,9.5); X (0,6.5,6.5)–(16,9.5,9.5); Z (6.5,6.5,0)–(9.5,9.5,16).

#### `rod`
End rod and lightning rods, by the facing's axis: Y (6,0,6)–(10,16,10); X (0,6,6)–(16,10,10); Z (6,6,0)–(10,10,16).

#### `carpet`
Wool carpets and moss carpet: (0,0,0)–(16,1,16) (has collision).

#### `pale_moss_carpet`
`bottom` plus `north/east/south/west` ∈ {none, low, tall}.
- Outline: union of: bottom → (0,0,0)–(16,1,16); each low side: N (0,0,0)–(16,10,1), E (15,0,0)–(16,10,16),
  S (0,0,15)–(16,10,16), W (0,0,0)–(1,10,16); each tall side: same sheets with y 0–16. If the union is empty →
  **BLOCK**.
- Collision (also visual, support): bottom=true → (0,0,0)–(16,1,16) (the default state's outline, independent of the
  sides); bottom=false → EMPTY.

#### `snow_layer`
`layers` L ∈ 1..8.
- Outline, visual and support: (0,0,0)–(16,2L,16) (L = 8 is BLOCK).
- Collision: one layer less: (0,0,0)–(16,2(L−1),16); EMPTY for L = 1.

### C. Furniture, containers, signs

#### `bed`
Coloured beds. Let d = facing for part=head, the opposite of facing for part=foot. Outline = mattress
(0,3,0)–(16,9,16) ∪ two legs on the d side: d=N: (0,0,0)–(3,3,3), (13,0,0)–(16,3,3); E: (13,0,0)–(16,3,3),
(13,0,13)–(16,3,16); S: (13,0,13)–(16,3,16), (0,0,13)–(3,3,16); W: (0,0,0)–(3,3,3), (0,0,13)–(3,3,16).

#### `straw_bed`
Foot part: (0,0,0)–(16,4,16). Head part: that base ∪ a pillow 5 pixels high on the d side (d = facing for the head):
N (0,0,0)–(16,5,8), E (8,0,0)–(16,5,16), S (0,0,8)–(16,5,16), W (0,0,0)–(8,5,16).

#### `chest`
Chest, trapped chest, copper chests. type=single: (1,0,1)–(15,14,15). type=left/right: by the connected direction
c (left → facing rotated clockwise, right → counter-clockwise): c=N (1,0,0)–(15,14,15), E (1,0,1)–(16,14,15),
S (1,0,1)–(15,14,16), W (0,0,1)–(15,14,15).

#### `ender_chest`
(1,0,1)–(15,14,15).

#### `sign`
Standing signs (all `rotation`): (4,0,4)–(12,16,12); no collision.

#### `wall_sign`
By facing: N (0,4.5,14)–(16,12.5,16), E (0,4.5,0)–(2,12.5,16), S (0,4.5,0)–(16,12.5,2), W (14,4.5,0)–(16,12.5,16).

#### `ceiling_hanging_sign`
By `rotation`: 0 or 8 → (1,0,7)–(15,10,9); 4 or 12 → (7,0,1)–(9,10,15); every other rotation → (3,0,3)–(13,16,13).
No collision. **Support = the outline.**

#### `wall_hanging_sign`
By the facing's axis. Outline: Z (north/south): (0,14,6)–(16,16,10) ∪ (1,0,7)–(15,10,9); X: (6,14,0)–(10,16,16) ∪
(7,0,1)–(9,10,15). **Collision (and therefore visual) = the plank only**: Z (0,14,6)–(16,16,10), X
(6,14,0)–(10,16,16) — this override ignores the block's no-collision flag. **Support = the outline.**

#### `banner` / `wall_banner`
Standing banner: (4,0,4)–(12,16,12). Wall banner by facing: N (0,0,14)–(16,12.5,16), E (0,0,0)–(2,12.5,16),
S (0,0,0)–(16,12.5,2), W (14,0,0)–(16,12.5,16). No collision.

#### `shelf`
Wooden shelves, by facing, NORTH: (0,12,11)–(16,16,13) ∪ (0,0,13)–(16,16,16) ∪ (0,0,11)–(16,4,13).

#### `lectern`
- Outline, NORTH (then horizontal rotation): (0,10,1)–(16,14,5.333333) ∪ (0,12,5.333333)–(16,16,9.666667) ∪
  (0,14,9.666667)–(16,18,14) ∪ base (0,0,0)–(16,2,16) ∪ post (4,2,4)–(12,14,12). The two decimal literals are divided
  by 16 in double precision; rotated variants use the flip expression of shapes.md §3.2 rule 4.
- Collision, visual and occlusion (override): base ∪ post, independent of facing.

#### `bell`
By `attachment` and facing. Bell body = (5,6,5)–(11,13,11) ∪ (4,4,4)–(12,6,12).
- floor: Z axis (0,0,4)–(16,16,12); X axis (4,0,0)–(12,16,16).
- ceiling: body ∪ (7,13,7)–(9,16,9).
- single_wall: body ∪ an arm toward the facing: N (7,13,0)–(9,15,13) (rotated with the facing: E (3,13,7)–(16,15,9),
  S (7,13,3)–(9,15,16), W (0,13,7)–(13,15,9)).
- double_wall: Z axis body ∪ (7,13,0)–(9,15,16); X axis body ∪ (0,13,7)–(16,15,9).
- Collision = outline.

#### `cauldron`, `cauldron(layered)`, `cauldron(lava)`
- Outline (all four cauldrons): BLOCK − [(0,0,4)–(16,3,12) ∪ (4,0,0)–(12,3,16) ∪ (2,0,2)–(14,3,14) ∪
  (2,4,2)–(14,16,14)] (L-shaped corner legs, a 1-pixel floor at y 3–4, 2-pixel walls).
- Interaction: (2,4,2)–(14,16,14).
- Entity-inside shape (not dumped by ZBF1): water / powder-snow cauldron with level n ∈ 1..3: outline ∪
  (2,4,2)–(14, 6+3n, 14) (tops 9, 12, 15); lava cauldron: outline ∪ (2,4,2)–(14,15,14); the empty cauldron keeps the
  default (BLOCK).

#### `composter`
`level` L ∈ 0..8: BLOCK − (2,h,2)–(14,16,14) with h = 2, 3, 5, 7, 9, 11, 13, 15, 15 for L = 0..8 (h = clamp(1 + 2L,
2, 16), and level 8 reuses level 7). Collision (and visual, support): the level-0 shape for every level. Interaction:
BLOCK.

#### `hopper`
- Outline: bowl = [(0,10,0)–(16,16,16) ∪ (4,4,4)–(12,10,12)] − (2,11,2)–(14,16,14), ∪ the spout for `facing` clipped
  to the block: down (6,0,6)–(10,6,10) (the rotated spout (6,−2,6)–(10,6,10) clipped), north (6,4,0)–(10,8,8), east
  (8,4,6)–(16,8,10), south (6,4,8)–(10,8,16), west (0,4,6)–(8,8,10). (`enabled` ignored.) The spout family is the
  `all` rotation of north (6,4,0)–(10,8,8) about the pivot (8,6,8) px.
- Interaction: facing down → (2,11,2)–(14,16,14); horizontal facing → (2,11,2)–(14,16,14) ∪ the horizontal rotation
  of (6,8,0)–(10,10,4): N (6,8,0)–(10,10,4), E (12,8,6)–(16,10,10), S (6,8,12)–(10,10,16), W (0,8,6)–(4,10,10).

#### `decorated_pot`, `heavy_core`, `copper_golem_statue`, `conduit`
Decorated pot (1,0,1)–(15,16,15); heavy core (4,0,4)–(12,8,12); copper golem statues (3,0,3)–(13,14,13); conduit
(5,5,5)–(11,11,11).

#### `skull(type)` / `wall_skull(type)` / `piglin_wall_head`
- Standing heads: outline and collision (4,0,4)–(12,8,12); piglin head (3,0,3)–(13,8,13); dragon head outline
  (4,0,4)–(12,8.5,12) but collision (4,0,4)–(12,8,12).
- Wall heads by facing: N (4,4,8)–(12,12,16), E (0,4,4)–(8,12,12), S (4,4,0)–(12,12,8), W (8,4,4)–(16,12,12) (the
  dragon wall head uses this too). Piglin wall head: N (3,4,8)–(13,12,16), E (0,4,3)–(8,12,13), S (3,4,0)–(13,12,8),
  W (8,4,3)–(16,12,13).

#### `anvil`
By the facing's axis. Z axis (north/south): (2,0,2)–(14,4,14) ∪ (4,4,3)–(12,5,13) ∪ (6,5,4)–(10,10,12) ∪
(3,10,0)–(13,16,16). X axis: (2,0,2)–(14,4,14) ∪ (3,4,4)–(13,5,12) ∪ (4,5,6)–(12,10,10) ∪ (0,10,3)–(16,16,13).

#### `grindstone`
`face` and `facing`. Model (wall, NORTH): wheel (4,2,0)–(12,14,12) ∪ legs (2,6,7)–(4,10,16), (2,5,3)–(4,11,9) and
their mirror images (12,6,7)–(14,10,16), (12,5,3)–(14,11,9). Rotated with the `attachFace` family (shapes.md §3.3).
For reference: floor NORTH = (4,4,2)–(12,16,14) ∪ (2,0,6)–(4,9,10) ∪ (2,7,5)–(4,13,11) ∪ the x 12–14 mirrors;
ceiling NORTH = (4,0,2)–(12,12,14) ∪ (2,7,6)–(4,16,10) ∪ (2,3,5)–(4,9,11) ∪ the x 12–14 mirrors. Collision = outline.

#### `stonecutter`, `enchanting_table`, `campfire`, `daylight_detector`, `sculk_sensor`, `diode`
Stonecutter (0,0,0)–(16,9,16); enchanting table (0,0,0)–(16,12,16); campfire and soul campfire (0,0,0)–(16,7,16);
daylight detector (0,0,0)–(16,6,16); sculk sensor and calibrated sculk sensor (0,0,0)–(16,8,16); repeater and
comparator (0,0,0)–(16,2,16).

#### `sculk_shrieker`
Outline BLOCK; collision, visual, support and occlusion (0,0,0)–(16,8,16).

#### `end_portal_frame`
eye=false: (0,0,0)–(16,13,16); eye=true: that ∪ (4,13,4)–(12,16,12).

#### `brewing_stand`
(1,0,1)–(15,2,15) ∪ (7,2,7)–(9,14,9).

#### `cake` / `candle_cake`
Cake by `bites` b ∈ 0..6: (1+2b,0,1)–(15,8,15). Candle cakes: (1,0,1)–(15,8,15) ∪ (7,8,7)–(9,14,9).

#### `candle`
By `candles`: 1 → (7,0,7)–(9,6,9); 2 → (5,0,6)–(11,6,9); 3 → (5,0,6)–(10,6,11); 4 → (5,0,5)–(11,6,10).

#### `lantern`
Lantern, soul lantern, copper lanterns. hanging=false: (5,0,5)–(11,7,11) ∪ (6,7,6)–(10,9,10); hanging=true: both
boxes one pixel higher: (5,1,5)–(11,8,11) ∪ (6,8,6)–(10,10,10).

#### `dragon_egg`
(1,0,1)–(15,16,15).

#### `scaffolding`
- Stable shape: top (0,14,0)–(16,16,16) ∪ four posts (0,0,0)–(2,16,2), (14,0,0)–(16,16,2), (14,0,14)–(16,16,16),
  (0,0,14)–(2,16,16). Unstable shape: stable ∪ bottom (0,0,0)–(16,2,16) (∪ four bottom edge bars that lie inside it).
- Outline: BLOCK when the context holds scaffolding; else bottom=true → unstable, bottom=false → stable.
- Collision (context rules in shapes.md §5.3): with the empty context **stable for every state**. Visual = that
  rule (stable); support = stable. Interaction BLOCK.

#### `piston` / `piston_head`
- Piston and sticky piston: extended=false → BLOCK; extended=true → by facing: N (0,0,4)–(16,16,16), E
  (0,0,0)–(12,16,16), S (0,0,0)–(16,16,12), W (4,0,0)–(16,16,16), UP (0,0,0)–(16,12,16), DOWN (0,4,0)–(16,16,16).
- Piston head: platform ∪ arm, by facing (`type` ignored). short=true arm reaches the block edge; short=false arm
  extends 4 pixels into the neighbouring block:

| facing | platform | arm, short=true | arm, short=false |
|---|---|---|---|
| north | (0,0,0)–(16,16,4) | (6,6,4)–(10,10,16) | (6,6,4)–(10,10,20) |
| east | (12,0,0)–(16,16,16) | (0,6,6)–(12,10,10) | (−4,6,6)–(12,10,10) |
| south | (0,0,12)–(16,16,16) | (6,6,0)–(10,10,12) | (6,6,−4)–(10,10,12) |
| west | (0,0,0)–(4,16,16) | (4,6,6)–(16,10,10) | (4,6,6)–(20,10,10) |
| up | (0,12,0)–(16,16,16) | (6,0,6)–(10,12,10) | (6,−4,6)–(10,12,10) |
| down | (0,0,0)–(16,4,16) | (6,4,6)–(10,16,10) | (6,4,6)–(10,20,10) |

### D. Thin redstone and floor pieces

#### `button`
`face`, `facing`, `powered`. Wall NORTH unpressed (5,6,14)–(11,10,16), pressed (5,6,15)–(11,10,16); wall E
(0,6,5)–(2,10,11) / (0,6,5)–(1,10,11); wall S (5,6,0)–(11,10,2) / (5,6,0)–(11,10,1); wall W (14,6,5)–(16,10,11) /
(15,6,5)–(16,10,11). Floor, facing N/S: (5,0,6)–(11,2,10) / (5,0,6)–(11,1,10); facing E/W: (6,0,5)–(10,2,11) /
(6,0,5)–(10,1,11). Ceiling, facing N/S: (5,14,6)–(11,16,10) / (5,15,6)–(11,16,10); facing E/W:
(6,14,5)–(10,16,11) / (6,15,5)–(10,16,11). (Construction: the attach-face rotations of (5,6,8)–(11,10,16), minus
`cube(12)` unpressed or `cube(14)` pressed.)

#### `lever`
`face`, `facing` (`powered` ignored). Wall: N (5,4,10)–(11,12,16), E (0,4,5)–(6,12,11), S (5,4,0)–(11,12,6),
W (10,4,5)–(16,12,11). Floor: N/S (5,0,4)–(11,6,12), E/W (4,0,5)–(12,6,11). Ceiling: N/S (5,10,4)–(11,16,12),
E/W (4,10,5)–(12,16,11).

#### `pressure_plate`
Signal > 0 (powered=true, or power > 0 for weighted plates) → (1,0,1)–(15,0.5,15); otherwise (1,0,1)–(15,1,15).

#### `redstone_wire`
Dot (3,0,3)–(13,1,13) always, ∪ for each side that is `side` or `up`: floor arm N (3,0,0)–(13,1,8), E
(8,0,3)–(16,1,13), S (3,0,8)–(13,1,16), W (0,0,3)–(8,1,13); and for `up` also the wall strip N (3,0,0)–(13,16,1),
E (15,0,3)–(16,16,13), S (3,0,15)–(13,16,16), W (0,0,3)–(1,16,13). (`power` ignored.)

#### `rail`
Rail, powered rail, detector rail, activator rail: ascending shapes → (0,0,0)–(16,8,16); others (0,0,0)–(16,2,16).

#### `tripwire` / `tripwire_hook`
Tripwire: attached=true (0,1,0)–(16,2.5,16); false (0,0,0)–(16,8,16). Hook by facing: N (5,0,10)–(11,10,16),
E (0,0,5)–(6,10,11), S (5,0,0)–(11,10,6), W (10,0,5)–(16,10,11).

#### `torch` / `wall_torch`
Torch (all four: torch, soul torch, copper torch, redstone torch): (6,0,6)–(10,10,10). Wall torches (incl. redstone):
N (5.5,3,11)–(10.5,13,16), E (0,3,5.5)–(5,13,10.5), S (5.5,3,0)–(10.5,13,5), W (11,3,5.5)–(16,13,10.5).

#### `fire` / `soul_fire`
Fire: union of the sheets for each true face among north/east/south/west/up — N (0,0,0)–(16,16,1), E
(15,0,0)–(16,16,16), S (0,0,15)–(16,16,16), W (0,0,0)–(1,16,16), UP (0,15,0)–(16,16,16); none → (0,0,0)–(16,1,16).
Soul fire: (0,0,0)–(16,1,16).

#### `nether_portal` / `end_portal`
Nether portal: axis=x (0,0,6)–(16,16,10); axis=z (6,0,0)–(10,16,16). End portal: (0,6,0)–(16,12,16).

#### `farmland`, `dirt_path`
(0,0,0)–(16,15,16).

#### `sinking_block`
Soul sand, mud: outline BLOCK, collision (0,0,0)–(16,14,16), **visual BLOCK, support BLOCK**.

#### `honey_block`
Outline BLOCK; collision (1,0,1)–(15,15,15).

#### `powder_snow`
Outline BLOCK; collision per shapes.md §5.3 (EMPTY with the empty context); **visual EMPTY**; support EMPTY.

#### `flower_pot`
All 40 pots: (5,0,5)–(11,6,11).

### E. Plants

Offset rule: where a family says "moved by offset", the shape is translated by the state's offset at the queried
position (shapes.md §5.4). Families not saying so never move, even if the block reports an offset type.

#### `sapling`, `seagrass`, `short_plant`, `short_dry_grass`, `tall_dry_grass`, `bush`, `tall_seagrass`
Saplings (2,0,2)–(14,12,14); seagrass (2,0,2)–(14,12,14); short plants (short grass, fern, dead bush, crimson roots,
warped roots) (2,0,2)–(14,13,14); short dry grass (2,0,2)–(14,10,14); tall dry grass (1,0,1)–(15,16,15); bush and red
shrub (0,0,0)–(16,13,16); tall seagrass (both halves) (2,0,2)–(14,16,14).

#### `mangrove_propagule`
Let a = age if hanging=true, else 4. Outline (7, m, 7)–(9,16,9) with m = 13, 10, 7, 3, 0 for a = 0..4, moved by offset.

#### `flower`
Flowers (incl. golden dandelion, torchflower, wither rose, open/closed eyeblossom): (5,0,5)–(11,10,11) moved by offset.

#### `mushroom`, `fungus`, `nether_sprouts`, `cactus_flower`
Mushrooms (5,0,5)–(11,6,11); fungi (4,0,4)–(12,9,12); nether sprouts (2,0,2)–(14,3,14); cactus flower
(1,0,1)–(15,12,15).

#### `crop`, `crop_small`, `beetroot`, `torchflower_crop`
Wheat age a ∈ 0..7: (0,0,0)–(16,2+2a,16) (a = 7 is BLOCK). Carrots, potatoes age a ∈ 0..7: (0,0,0)–(16,2+a,16).
Beetroots age a ∈ 0..3: (0,0,0)–(16,2+2a,16). Torchflower crop age a ∈ 0..1: (5,0,5)–(11,6+4a,11).

#### `pitcher_crop`
- Outline: lower half: age 0 (5,−1,5)–(11,3,11); age 1 (3,−1,3)–(13,14,13); ages 2–4 (3,−1,3)–(13,16,13). Upper
  half: ages 0–2 EMPTY; age 3 (3,0,3)–(13,11,13); age 4 (3,0,3)–(13,15,13).
- Collision (override, ignores `nc`): lower age 0 (5,−1,5)–(11,3,11); lower ages 1–4 (3,−1,3)–(13,5,13); upper EMPTY.

#### `stem` / `attached_stem`
Stems age a: (7,0,7)–(9,2+2a,9). Attached stems by facing: N (6,0,0)–(10,10,10), E (6,0,6)–(16,10,10),
S (6,0,6)–(10,10,16), W (0,0,6)–(10,10,10).

#### `cocoa`
By age and facing:

| age | north | east | south | west |
|---|---|---|---|---|
| 0 | (6,7,1)–(10,12,5) | (11,7,6)–(15,12,10) | (6,7,11)–(10,12,15) | (1,7,6)–(5,12,10) |
| 1 | (5,5,1)–(11,12,7) | (9,5,5)–(15,12,11) | (5,5,9)–(11,12,15) | (1,5,5)–(7,12,11) |
| 2 | (4,3,1)–(12,12,9) | (7,3,4)–(15,12,12) | (4,3,7)–(12,12,15) | (1,3,4)–(9,12,12) |

#### `nether_wart`, `sweet_berry_bush`, `sugar_cane`, `cactus`, `lily_pad`
Nether wart age a: (0,0,0)–(16,5+3a,16). Sweet berry bush: age 0 (3,0,3)–(13,8,13); ages 1–2 (1,0,1)–(15,16,15); age 3
BLOCK. Sugar cane (2,0,2)–(14,16,14). Cactus outline (1,0,1)–(15,16,15), collision (1,0,1)–(15,15,15). Lily pad
(1,0,1)–(15,1.5,15) (has collision).

#### Vines and hanging plants
- `kelp` (0,0,0)–(16,9,16); `kelp_plant` BLOCK.
- `weeping_vines` (4,9,4)–(12,16,12); `weeping_vines_plant` (1,0,1)–(15,16,15).
- `twisting_vines` (4,0,4)–(12,15,12); `twisting_vines_plant` (4,0,4)–(12,16,12).
- `cave_vines` (head and body) (1,0,1)–(15,16,15).
- `hanging_roots` (2,10,2)–(14,16,14); `spore_blossom` (2,13,2)–(14,16,14).
- `hanging_moss` (pale hanging moss): tip=true (1,2,1)–(15,16,15); tip=false (1,0,1)–(15,16,15).

#### `vine` / `multiface`
Union of the face sheets that are present — DOWN (0,0,0)–(16,1,16) (multiface only), UP (0,15,0)–(16,16,16),
N (0,0,0)–(16,16,1), E (15,0,0)–(16,16,16), S (0,0,15)–(16,16,16), W (0,0,0)–(1,16,16); no face → **BLOCK**. Vine has
properties up/north/east/south/west; multiface blocks (glow lichen, sculk vein, resin clump) all six.

#### `azalea`
Azalea, flowering azalea: (0,8,0)–(16,16,16) ∪ (6,0,6)–(10,8,10).

#### `segmented(h)`
Pink petals and wildflowers (h = 3), leaf litter (h = 1). Take `amount` (or `segment_amount`) c quarters starting at
the facing's quarter and turning counter-clockwise (N → NW, then W → SW, then S → SE, then E → NE; E → NE, N → NW, …),
and return the **bounding box** of their union with y 0–h: c = 1 → that quarter; c = 2 → N: (0,0,0)–(8,h,16),
E: (0,0,0)–(16,h,8), S: (8,0,0)–(16,h,16), W: (0,0,8)–(16,h,16); c ≥ 3 → (0,0,0)–(16,h,16).

#### `big_dripleaf` / `big_dripleaf_stem` / `small_dripleaf`
- Big dripleaf: leaf by tilt: none, unstable → (0,11,0)–(16,15,16); partial → (0,11,0)–(16,13,16); full → EMPTY.
  Stem by facing: N (5,0,9)–(11,13,15), E (1,0,5)–(7,13,11), S (5,0,1)–(11,13,7), W (9,0,5)–(15,13,11). Outline =
  leaf ∪ stem; **collision (visual, support) = the leaf only**.
- Big dripleaf stem: N (5,0,9)–(11,16,15), E (1,0,5)–(7,16,11), S (5,0,1)–(11,16,7), W (9,0,5)–(15,16,11); no
  collision.
- Small dripleaf (both halves): (2,0,2)–(14,13,14); not moved by its XYZ offset.

#### `bamboo_sapling` / `bamboo`
Bamboo sapling (4,0,4)–(12,12,12) moved by offset. Bamboo (dynamic shape): outline leaves=large → (3,0,3)–(13,16,13),
otherwise (5,0,5)–(11,16,11), moved by offset; collision (and visual, support) (6.5,0,6.5)–(9.5,16,9.5) moved by
offset; **collision-full-block forced false**.

#### `speleothem`
Pointed dripstone and sulfur spike (dynamic shape), by thickness: tip_merge (5,0,5)–(11,16,11); tip with
vertical_direction=down (5,5,5)–(11,16,11); tip with up (5,0,5)–(11,11,11); frustum (4,0,4)–(12,16,12); middle
(3,0,3)–(13,16,13); base (2,0,2)–(14,16,14); all moved by offset (max horizontal offset 0.125). Collision = outline
(moved). **Collision-full-block forced false.**

#### `coral_plant` / `coral_fan` / `coral_wall_fan`
Coral plants (live and dead) (2,0,2)–(14,15,14); coral fans (2,0,2)–(14,4,14); wall fans by facing:
N (0,4,5)–(16,12,16), E (0,4,0)–(11,12,16), S (0,4,0)–(16,12,11), W (5,4,0)–(16,12,16).

#### `sea_pickle`, `turtle_egg`, `sniffer_egg`, `dried_ghast`, `frogspawn`
Sea pickle by `pickles`: 1 (6,0,6)–(10,6,10); 2 (3,0,3)–(13,6,13); 3 (2,0,2)–(14,6,14); 4 (2,0,2)–(14,7,14). Turtle
egg: eggs=1 (3,0,3)–(12,7,12), else (1,0,1)–(15,7,15). Sniffer egg (1,0,2)–(15,16,14). Dried ghast
(3,0,3)–(13,10,13). Frogspawn (0,0,0)–(16,1.5,16).

#### `amethyst_cluster(h, w)`
Amethyst cluster (h 7, w 10), large bud (5, 10), medium bud (4, 10), small bud (3, 8). By facing (the side the
crystal points to), with a = 8 − w/2, b = 8 + w/2: up (a,0,a)–(b,h,b); down (a,16−h,a)–(b,16,b); north
(a,a,16−h)–(b,b,16); south (a,a,0)–(b,b,h); east (0,a,a)–(h,b,b); west (16−h,a,a)–(16,b,b).

#### `shelf_mushroom`
By age and facing; NORTH: age 0 (3,9,9)–(13,11,16) ∪ (5,8,12)–(11,9,16); age 1 (1,8,6)–(15,11,16) ∪
(4,6,10)–(12,8,16); other facings by horizontal rotation.

#### `chorus_plant` / `chorus_flower`
Chorus plant: core (3,3,3)–(13,13,13) ∪ an arm for each true direction: N (3,3,0)–(13,13,8), S (3,3,8)–(13,13,16),
E (8,3,3)–(16,13,13), W (0,3,3)–(8,13,13), UP (3,8,3)–(13,16,13), DOWN (3,0,3)–(13,8,13). Chorus flower: outline
BLOCK, **support (1,0,1)–(15,15,15)**.

## Edge cases and known quirks

1. Overrides that ignore the no-collision flag: scaffolding (stable shape), wall hanging signs (plank), pitcher crop
   (bulb/crop boxes), and the context families of shapes.md §5.3. Their visual shapes follow the same rule.
2. Empty fallbacks: vine, multiface and pale moss carpet with no faces have a **full-block** outline; fire with no
   faces has the 1-pixel floor sheet; a wall with up=false and no sides has an **empty** outline (and collision).
3. Shapes leaving the unit cube: fence / wall / closed gate collision (to 24 px), piston head long arm, pitcher crop
   (from −1 px), lectern outline (to 18 px). Their collision shapes give *large collision shape* = true
   (state-facts §2) where the collision is concerned (not the lectern, whose collision is inside the cube).
4. Wheat at age 7, sweet berry bush at age 3, kelp plant, double slabs, extended=false pistons, full snow (8 layers)
   and several others have a full-block outline; they count as full blocks for every face test.
5. Segmented plants use the bounding box, so two petals facing north report the whole west half.

## Constants and IDs

All coordinates above; rotation names and the quarter convention in Summary.

## Oracle probe points

`Block.getShape` / `getCollisionShape` / `getVisualShape` / `getInteractionShape` / `getOcclusionShape` /
`getBlockSupportShape` overrides on the classes named in `block-table.md`; ZBF1 records all of them for every state.

## Open questions

None for the shapes themselves; the global probes are in shapes.md "Open questions".

## Parity status

Not yet run.

## Family membership (generated from the registry; registry order)

Every block id of 26.3 appears exactly once. Ids are without the `minecraft:` namespace; order is registry order.

- **`cube`** (396): stone, granite, polished_granite, diorite, polished_diorite, andesite, polished_andesite, grass_block, dirt, coarse_dirt, podzol, cobblestone, oak_planks, spruce_planks, birch_planks, jungle_planks, acacia_planks, cherry_planks, dark_oak_planks, pale_oak_wood, pale_oak_planks, mangrove_planks, poplar_planks, bamboo_planks, bamboo_mosaic, bedrock, sand, suspicious_sand, red_sand, gravel, suspicious_gravel, gold_ore, deepslate_gold_ore, iron_ore, deepslate_iron_ore, coal_ore, deepslate_coal_ore, nether_gold_ore, oak_log, spruce_log, birch_log, jungle_log, acacia_log, cherry_log, dark_oak_log, pale_oak_log, mangrove_log, poplar_log, mangrove_roots, muddy_mangrove_roots, bamboo_block, stripped_spruce_log, stripped_birch_log, stripped_jungle_log, stripped_acacia_log, stripped_cherry_log, stripped_dark_oak_log, stripped_pale_oak_log, stripped_oak_log, stripped_mangrove_log, stripped_poplar_log, stripped_bamboo_block, oak_wood, spruce_wood, birch_wood, jungle_wood, acacia_wood, cherry_wood, dark_oak_wood, mangrove_wood, poplar_wood, stripped_oak_wood, stripped_spruce_wood, stripped_birch_wood, stripped_jungle_wood, stripped_acacia_wood, stripped_cherry_wood, stripped_dark_oak_wood, stripped_pale_oak_wood, stripped_mangrove_wood, stripped_poplar_wood, sponge, wet_sponge, lapis_ore, deepslate_lapis_ore, lapis_block, dispenser, sandstone, chiseled_sandstone, cut_sandstone, note_block, cobweb, white_wool, orange_wool, magenta_wool, light_blue_wool, yellow_wool, lime_wool, pink_wool, gray_wool, light_gray_wool, cyan_wool, purple_wool, blue_wool, brown_wool, green_wool, red_wool, black_wool, gold_block, iron_block, bricks, tnt, bookshelf, chiseled_bookshelf, mossy_cobblestone, obsidian, spawner, creaking_heart, diamond_ore, deepslate_diamond_ore, diamond_block, crafting_table, furnace, redstone_ore, deepslate_redstone_ore, ice, snow_block, clay, jukebox, netherrack, soul_soil, basalt, polished_basalt, glowstone, carved_pumpkin, jack_o_lantern, stone_bricks, mossy_stone_bricks, cracked_stone_bricks, chiseled_stone_bricks, packed_mud, mud_bricks, infested_stone, infested_cobblestone, infested_stone_bricks, infested_mossy_stone_bricks, infested_cracked_stone_bricks, infested_chiseled_stone_bricks, brown_mushroom_block, red_mushroom_block, mushroom_stem, pumpkin, melon, mycelium, resin_block, resin_bricks, chiseled_resin_bricks, nether_bricks, end_stone, redstone_lamp, emerald_ore, deepslate_emerald_ore, emerald_block, command_block, beacon, redstone_block, nether_quartz_ore, quartz_block, chiseled_quartz_block, quartz_pillar, dropper, white_terracotta, orange_terracotta, magenta_terracotta, light_blue_terracotta, yellow_terracotta, lime_terracotta, pink_terracotta, gray_terracotta, light_gray_terracotta, cyan_terracotta, purple_terracotta, blue_terracotta, brown_terracotta, green_terracotta, red_terracotta, black_terracotta, slime_block, barrier, prismarine, prismarine_bricks, dark_prismarine, sea_lantern, hay_block, terracotta, coal_block, packed_ice, red_sandstone, chiseled_red_sandstone, cut_red_sandstone, smooth_stone, smooth_sandstone, smooth_quartz, smooth_red_sandstone, purpur_block, purpur_pillar, end_stone_bricks, end_gateway, repeating_command_block, chain_command_block, frosted_ice, magma_block, nether_wart_block, red_nether_bricks, bone_block, observer, white_glazed_terracotta, orange_glazed_terracotta, magenta_glazed_terracotta, light_blue_glazed_terracotta, yellow_glazed_terracotta, lime_glazed_terracotta, pink_glazed_terracotta, gray_glazed_terracotta, light_gray_glazed_terracotta, cyan_glazed_terracotta, purple_glazed_terracotta, blue_glazed_terracotta, brown_glazed_terracotta, green_glazed_terracotta, red_glazed_terracotta, black_glazed_terracotta, white_concrete, orange_concrete, magenta_concrete, light_blue_concrete, yellow_concrete, lime_concrete, pink_concrete, gray_concrete, light_gray_concrete, cyan_concrete, purple_concrete, blue_concrete, brown_concrete, green_concrete, red_concrete, black_concrete, white_concrete_powder, orange_concrete_powder, magenta_concrete_powder, light_blue_concrete_powder, yellow_concrete_powder, lime_concrete_powder, pink_concrete_powder, gray_concrete_powder, light_gray_concrete_powder, cyan_concrete_powder, purple_concrete_powder, blue_concrete_powder, brown_concrete_powder, green_concrete_powder, red_concrete_powder, black_concrete_powder, dried_kelp_block, dead_tube_coral_block, dead_brain_coral_block, dead_bubble_coral_block, dead_fire_coral_block, dead_horn_coral_block, tube_coral_block, brain_coral_block, bubble_coral_block, fire_coral_block, horn_coral_block, blue_ice, loom, barrel, smoker, blast_furnace, cartography_table, fletching_table, smithing_table, warped_stem, stripped_warped_stem, warped_hyphae, stripped_warped_hyphae, warped_nylium, warped_wart_block, crimson_stem, stripped_crimson_stem, crimson_hyphae, stripped_crimson_hyphae, crimson_nylium, shroomlight, crimson_planks, warped_planks, structure_block, jigsaw, test_block, test_instance_block, target, bee_nest, beehive, honeycomb_block, netherite_block, ancient_debris, crying_obsidian, respawn_anchor, lodestone, blackstone, polished_blackstone, polished_blackstone_bricks, cracked_polished_blackstone_bricks, chiseled_polished_blackstone, gilded_blackstone, chiseled_nether_bricks, cracked_nether_bricks, quartz_bricks, amethyst_block, budding_amethyst, tuff, polished_tuff, chiseled_tuff, tuff_bricks, chiseled_tuff_bricks, sulfur, potent_sulfur, polished_sulfur, sulfur_bricks, chiseled_sulfur, cinnabar, polished_cinnabar, cinnabar_bricks, chiseled_cinnabar, calcite, sculk, sculk_catalyst, copper_block, exposed_copper, weathered_copper, oxidized_copper, waxed_copper_block, waxed_exposed_copper, waxed_weathered_copper, waxed_oxidized_copper, copper_ore, deepslate_copper_ore, cut_copper, exposed_cut_copper, weathered_cut_copper, oxidized_cut_copper, waxed_cut_copper, waxed_exposed_cut_copper, waxed_weathered_cut_copper, waxed_oxidized_cut_copper, chiseled_copper, exposed_chiseled_copper, weathered_chiseled_copper, oxidized_chiseled_copper, waxed_chiseled_copper, waxed_exposed_chiseled_copper, waxed_weathered_chiseled_copper, waxed_oxidized_chiseled_copper, copper_bulb, exposed_copper_bulb, weathered_copper_bulb, oxidized_copper_bulb, waxed_copper_bulb, waxed_exposed_copper_bulb, waxed_weathered_copper_bulb, waxed_oxidized_copper_bulb, dripstone_block, moss_block, rooted_dirt, deepslate, cobbled_deepslate, polished_deepslate, deepslate_tiles, deepslate_bricks, chiseled_deepslate, cracked_deepslate_bricks, cracked_deepslate_tiles, infested_deepslate, smooth_basalt, raw_iron_block, raw_copper_block, raw_gold_block, ochre_froglight, verdant_froglight, pearlescent_froglight, reinforced_deepslate, crafter, trial_spawner, vault, pale_moss_block
- **`amethyst_cluster(h=3,w=8)`** (1): small_amethyst_bud
- **`amethyst_cluster(h=4,w=10)`** (1): medium_amethyst_bud
- **`amethyst_cluster(h=5,w=10)`** (1): large_amethyst_bud
- **`amethyst_cluster(h=7,w=10)`** (1): amethyst_cluster
- **`anvil`** (3): anvil, chipped_anvil, damaged_anvil
- **`attached_stem`** (2): attached_pumpkin_stem, attached_melon_stem
- **`azalea`** (2): azalea, flowering_azalea
- **`bamboo`** (1): bamboo
- **`bamboo_sapling`** (1): bamboo_sapling
- **`banner`** (16): white_banner, orange_banner, magenta_banner, light_blue_banner, yellow_banner, lime_banner, pink_banner, gray_banner, light_gray_banner, cyan_banner, purple_banner, blue_banner, brown_banner, green_banner, red_banner, black_banner
- **`bed`** (16): white_bed, orange_bed, magenta_bed, light_blue_bed, yellow_bed, lime_bed, pink_bed, gray_bed, light_gray_bed, cyan_bed, purple_bed, blue_bed, brown_bed, green_bed, red_bed, black_bed
- **`beetroot`** (1): beetroots
- **`bell`** (1): bell
- **`big_dripleaf`** (1): big_dripleaf
- **`big_dripleaf_stem`** (1): big_dripleaf_stem
- **`brewing_stand`** (1): brewing_stand
- **`bush`** (2): bush, red_shrub
- **`button`** (15): stone_button, oak_button, spruce_button, birch_button, jungle_button, acacia_button, cherry_button, dark_oak_button, pale_oak_button, mangrove_button, poplar_button, bamboo_button, crimson_button, warped_button, polished_blackstone_button
- **`cactus`** (1): cactus
- **`cactus_flower`** (1): cactus_flower
- **`cake`** (1): cake
- **`campfire`** (2): campfire, soul_campfire
- **`candle`** (17): candle, white_candle, orange_candle, magenta_candle, light_blue_candle, yellow_candle, lime_candle, pink_candle, gray_candle, light_gray_candle, cyan_candle, purple_candle, blue_candle, brown_candle, green_candle, red_candle, black_candle
- **`candle_cake`** (17): candle_cake, white_candle_cake, orange_candle_cake, magenta_candle_cake, light_blue_candle_cake, yellow_candle_cake, lime_candle_cake, pink_candle_cake, gray_candle_cake, light_gray_candle_cake, cyan_candle_cake, purple_candle_cake, blue_candle_cake, brown_candle_cake, green_candle_cake, red_candle_cake, black_candle_cake
- **`carpet`** (17): white_carpet, orange_carpet, magenta_carpet, light_blue_carpet, yellow_carpet, lime_carpet, pink_carpet, gray_carpet, light_gray_carpet, cyan_carpet, purple_carpet, blue_carpet, brown_carpet, green_carpet, red_carpet, black_carpet, moss_carpet
- **`cauldron`** (1): cauldron
- **`cauldron(lava)`** (1): lava_cauldron
- **`cauldron(layered)`** (2): water_cauldron, powder_snow_cauldron
- **`cave_vines`** (2): cave_vines, cave_vines_plant
- **`ceiling_hanging_sign`** (13): oak_hanging_sign, spruce_hanging_sign, birch_hanging_sign, acacia_hanging_sign, cherry_hanging_sign, jungle_hanging_sign, dark_oak_hanging_sign, pale_oak_hanging_sign, crimson_hanging_sign, warped_hanging_sign, mangrove_hanging_sign, poplar_hanging_sign, bamboo_hanging_sign
- **`chain`** (9): iron_chain, copper_chain, exposed_copper_chain, weathered_copper_chain, oxidized_copper_chain, waxed_copper_chain, waxed_exposed_copper_chain, waxed_weathered_copper_chain, waxed_oxidized_copper_chain
- **`chest`** (10): chest, trapped_chest, copper_chest, exposed_copper_chest, weathered_copper_chest, oxidized_copper_chest, waxed_copper_chest, waxed_exposed_copper_chest, waxed_weathered_copper_chest, waxed_oxidized_copper_chest
- **`chorus_flower`** (1): chorus_flower
- **`chorus_plant`** (1): chorus_plant
- **`cocoa`** (1): cocoa
- **`composter`** (1): composter
- **`conduit`** (1): conduit
- **`copper_golem_statue`** (8): copper_golem_statue, exposed_copper_golem_statue, weathered_copper_golem_statue, oxidized_copper_golem_statue, waxed_copper_golem_statue, waxed_exposed_copper_golem_statue, waxed_weathered_copper_golem_statue, waxed_oxidized_copper_golem_statue
- **`coral_fan`** (10): dead_tube_coral_fan, dead_brain_coral_fan, dead_bubble_coral_fan, dead_fire_coral_fan, dead_horn_coral_fan, tube_coral_fan, brain_coral_fan, bubble_coral_fan, fire_coral_fan, horn_coral_fan
- **`coral_plant`** (10): dead_tube_coral, dead_brain_coral, dead_bubble_coral, dead_fire_coral, dead_horn_coral, tube_coral, brain_coral, bubble_coral, fire_coral, horn_coral
- **`coral_wall_fan`** (10): dead_tube_coral_wall_fan, dead_brain_coral_wall_fan, dead_bubble_coral_wall_fan, dead_fire_coral_wall_fan, dead_horn_coral_wall_fan, tube_coral_wall_fan, brain_coral_wall_fan, bubble_coral_wall_fan, fire_coral_wall_fan, horn_coral_wall_fan
- **`crop`** (1): wheat
- **`crop_small`** (2): carrots, potatoes
- **`cube_glass`** (26): glass, white_stained_glass, orange_stained_glass, magenta_stained_glass, light_blue_stained_glass, yellow_stained_glass, lime_stained_glass, pink_stained_glass, gray_stained_glass, light_gray_stained_glass, cyan_stained_glass, purple_stained_glass, blue_stained_glass, brown_stained_glass, green_stained_glass, red_stained_glass, black_stained_glass, tinted_glass, copper_grate, exposed_copper_grate, weathered_copper_grate, oxidized_copper_grate, waxed_copper_grate, waxed_exposed_copper_grate, waxed_weathered_copper_grate, waxed_oxidized_copper_grate
- **`cube_plant`** (8): sunflower, lilac, rose_bush, peony, tall_grass, large_fern, pitcher_plant, firefly_bush
- **`daylight_detector`** (1): daylight_detector
- **`decorated_pot`** (1): decorated_pot
- **`diode`** (2): repeater, comparator
- **`dirt_path`** (1): dirt_path
- **`door`** (22): oak_door, iron_door, spruce_door, birch_door, jungle_door, acacia_door, cherry_door, dark_oak_door, pale_oak_door, mangrove_door, poplar_door, bamboo_door, crimson_door, warped_door, copper_door, exposed_copper_door, weathered_copper_door, oxidized_copper_door, waxed_copper_door, waxed_exposed_copper_door, waxed_weathered_copper_door, waxed_oxidized_copper_door
- **`dragon_egg`** (1): dragon_egg
- **`dried_ghast`** (1): dried_ghast
- **`empty`** (4): air, void_air, cave_air, bubble_column
- **`enchanting_table`** (1): enchanting_table
- **`end_portal`** (1): end_portal
- **`end_portal_frame`** (1): end_portal_frame
- **`ender_chest`** (1): ender_chest
- **`farmland`** (1): farmland
- **`fence`** (14): oak_fence, nether_brick_fence, spruce_fence, birch_fence, jungle_fence, acacia_fence, cherry_fence, dark_oak_fence, pale_oak_fence, mangrove_fence, poplar_fence, bamboo_fence, crimson_fence, warped_fence
- **`fence_gate`** (13): oak_fence_gate, spruce_fence_gate, birch_fence_gate, jungle_fence_gate, acacia_fence_gate, cherry_fence_gate, dark_oak_fence_gate, pale_oak_fence_gate, mangrove_fence_gate, poplar_fence_gate, bamboo_fence_gate, crimson_fence_gate, warped_fence_gate
- **`fire`** (1): fire
- **`flower`** (17): dandelion, golden_dandelion, torchflower, poppy, blue_orchid, allium, azure_bluet, red_tulip, orange_tulip, white_tulip, pink_tulip, oxeye_daisy, cornflower, wither_rose, lily_of_the_valley, open_eyeblossom, closed_eyeblossom
- **`flower_pot`** (40): flower_pot, potted_torchflower, potted_oak_sapling, potted_spruce_sapling, potted_birch_sapling, potted_jungle_sapling, potted_acacia_sapling, potted_cherry_sapling, potted_dark_oak_sapling, potted_pale_oak_sapling, potted_poplar_sapling, potted_mangrove_propagule, potted_fern, potted_dandelion, potted_golden_dandelion, potted_poppy, potted_blue_orchid, potted_allium, potted_azure_bluet, potted_red_tulip, potted_orange_tulip, potted_white_tulip, potted_pink_tulip, potted_oxeye_daisy, potted_cornflower, potted_lily_of_the_valley, potted_wither_rose, potted_red_mushroom, potted_brown_mushroom, potted_dead_bush, potted_cactus, potted_bamboo, potted_crimson_fungus, potted_warped_fungus, potted_crimson_roots, potted_warped_roots, potted_azalea_bush, potted_flowering_azalea_bush, potted_open_eyeblossom, potted_closed_eyeblossom
- **`fluid`** (2): water, lava
- **`frogspawn`** (1): frogspawn
- **`fungus`** (2): warped_fungus, crimson_fungus
- **`grindstone`** (1): grindstone
- **`hanging_moss`** (1): pale_hanging_moss
- **`hanging_roots`** (1): hanging_roots
- **`heavy_core`** (1): heavy_core
- **`honey_block`** (1): honey_block
- **`hopper`** (1): hopper
- **`kelp`** (1): kelp
- **`kelp_plant`** (1): kelp_plant
- **`ladder`** (1): ladder
- **`lantern`** (10): lantern, soul_lantern, copper_lantern, exposed_copper_lantern, weathered_copper_lantern, oxidized_copper_lantern, waxed_copper_lantern, waxed_exposed_copper_lantern, waxed_weathered_copper_lantern, waxed_oxidized_copper_lantern
- **`leaves`** (14): oak_leaves, spruce_leaves, birch_leaves, jungle_leaves, acacia_leaves, cherry_leaves, dark_oak_leaves, pale_oak_leaves, mangrove_leaves, red_poplar_leaves, orange_poplar_leaves, yellow_poplar_leaves, azalea_leaves, flowering_azalea_leaves
- **`lectern`** (1): lectern
- **`lever`** (1): lever
- **`light`** (1): light
- **`lily_pad`** (1): lily_pad
- **`mangrove_propagule`** (1): mangrove_propagule
- **`moving_piston`** (1): moving_piston
- **`multiface`** (3): glow_lichen, resin_clump, sculk_vein
- **`mushroom`** (2): brown_mushroom, red_mushroom
- **`nether_portal`** (1): nether_portal
- **`nether_sprouts`** (1): nether_sprouts
- **`nether_wart`** (1): nether_wart
- **`pale_moss_carpet`** (1): pale_moss_carpet
- **`pane`** (26): iron_bars, copper_bars, exposed_copper_bars, weathered_copper_bars, oxidized_copper_bars, waxed_copper_bars, waxed_exposed_copper_bars, waxed_weathered_copper_bars, waxed_oxidized_copper_bars, glass_pane, white_stained_glass_pane, orange_stained_glass_pane, magenta_stained_glass_pane, light_blue_stained_glass_pane, yellow_stained_glass_pane, lime_stained_glass_pane, pink_stained_glass_pane, gray_stained_glass_pane, light_gray_stained_glass_pane, cyan_stained_glass_pane, purple_stained_glass_pane, blue_stained_glass_pane, brown_stained_glass_pane, green_stained_glass_pane, red_stained_glass_pane, black_stained_glass_pane
- **`piglin_wall_head`** (1): piglin_wall_head
- **`piston`** (2): sticky_piston, piston
- **`piston_head`** (1): piston_head
- **`pitcher_crop`** (1): pitcher_crop
- **`powder_snow`** (1): powder_snow
- **`pressure_plate`** (17): stone_pressure_plate, oak_pressure_plate, spruce_pressure_plate, birch_pressure_plate, jungle_pressure_plate, acacia_pressure_plate, cherry_pressure_plate, dark_oak_pressure_plate, pale_oak_pressure_plate, mangrove_pressure_plate, poplar_pressure_plate, bamboo_pressure_plate, light_weighted_pressure_plate, heavy_weighted_pressure_plate, crimson_pressure_plate, warped_pressure_plate, polished_blackstone_pressure_plate
- **`rail`** (4): powered_rail, detector_rail, rail, activator_rail
- **`redstone_wire`** (1): redstone_wire
- **`rod`** (9): end_rod, lightning_rod, exposed_lightning_rod, weathered_lightning_rod, oxidized_lightning_rod, waxed_lightning_rod, waxed_exposed_lightning_rod, waxed_weathered_lightning_rod, waxed_oxidized_lightning_rod
- **`sapling`** (9): oak_sapling, spruce_sapling, birch_sapling, jungle_sapling, acacia_sapling, cherry_sapling, dark_oak_sapling, pale_oak_sapling, poplar_sapling
- **`scaffolding`** (1): scaffolding
- **`sculk_sensor`** (2): sculk_sensor, calibrated_sculk_sensor
- **`sculk_shrieker`** (1): sculk_shrieker
- **`sea_pickle`** (1): sea_pickle
- **`seagrass`** (1): seagrass
- **`segmented(h=1)`** (1): leaf_litter
- **`segmented(h=3)`** (2): pink_petals, wildflowers
- **`shelf`** (13): acacia_shelf, bamboo_shelf, birch_shelf, cherry_shelf, crimson_shelf, dark_oak_shelf, jungle_shelf, mangrove_shelf, oak_shelf, pale_oak_shelf, poplar_shelf, spruce_shelf, warped_shelf
- **`shelf_mushroom`** (1): shelf_mushroom
- **`short_dry_grass`** (1): short_dry_grass
- **`short_plant`** (5): short_grass, fern, dead_bush, warped_roots, crimson_roots
- **`shulker_box`** (17): shulker_box, white_shulker_box, orange_shulker_box, magenta_shulker_box, light_blue_shulker_box, yellow_shulker_box, lime_shulker_box, pink_shulker_box, gray_shulker_box, light_gray_shulker_box, cyan_shulker_box, purple_shulker_box, blue_shulker_box, brown_shulker_box, green_shulker_box, red_shulker_box, black_shulker_box
- **`sign`** (13): oak_sign, spruce_sign, birch_sign, acacia_sign, cherry_sign, jungle_sign, dark_oak_sign, pale_oak_sign, mangrove_sign, poplar_sign, bamboo_sign, crimson_sign, warped_sign
- **`sinking_block`** (2): soul_sand, mud
- **`skull(creeper)`** (1): creeper_head
- **`skull(dragon)`** (1): dragon_head
- **`skull(piglin)`** (1): piglin_head
- **`skull(player)`** (1): player_head
- **`skull(skeleton)`** (1): skeleton_skull
- **`skull(wither_skeleton)`** (1): wither_skeleton_skull
- **`skull(zombie)`** (1): zombie_head
- **`slab`** (101): white_wool_slab, orange_wool_slab, magenta_wool_slab, light_blue_wool_slab, yellow_wool_slab, lime_wool_slab, pink_wool_slab, gray_wool_slab, light_gray_wool_slab, cyan_wool_slab, purple_wool_slab, blue_wool_slab, brown_wool_slab, green_wool_slab, red_wool_slab, black_wool_slab, resin_brick_slab, prismarine_slab, prismarine_brick_slab, dark_prismarine_slab, oak_slab, spruce_slab, birch_slab, jungle_slab, acacia_slab, cherry_slab, dark_oak_slab, pale_oak_slab, mangrove_slab, poplar_slab, bamboo_slab, bamboo_mosaic_slab, stone_slab, sandstone_slab, cut_sandstone_slab, petrified_oak_slab, cobblestone_slab, brick_slab, stone_brick_slab, mud_brick_slab, nether_brick_slab, quartz_slab, red_sandstone_slab, cut_red_sandstone_slab, smooth_stone_slab, purpur_slab, white_concrete_slab, orange_concrete_slab, magenta_concrete_slab, light_blue_concrete_slab, yellow_concrete_slab, lime_concrete_slab, pink_concrete_slab, gray_concrete_slab, light_gray_concrete_slab, cyan_concrete_slab, purple_concrete_slab, blue_concrete_slab, brown_concrete_slab, green_concrete_slab, red_concrete_slab, black_concrete_slab, polished_granite_slab, smooth_red_sandstone_slab, mossy_stone_brick_slab, polished_diorite_slab, mossy_cobblestone_slab, end_stone_brick_slab, smooth_sandstone_slab, smooth_quartz_slab, granite_slab, andesite_slab, red_nether_brick_slab, polished_andesite_slab, diorite_slab, crimson_slab, warped_slab, blackstone_slab, polished_blackstone_brick_slab, polished_blackstone_slab, tuff_slab, polished_tuff_slab, tuff_brick_slab, sulfur_slab, polished_sulfur_slab, sulfur_brick_slab, cinnabar_slab, polished_cinnabar_slab, cinnabar_brick_slab, cut_copper_slab, exposed_cut_copper_slab, weathered_cut_copper_slab, oxidized_cut_copper_slab, waxed_cut_copper_slab, waxed_exposed_cut_copper_slab, waxed_weathered_cut_copper_slab, waxed_oxidized_cut_copper_slab, cobbled_deepslate_slab, polished_deepslate_slab, deepslate_tile_slab, deepslate_brick_slab
- **`small_dripleaf`** (1): small_dripleaf
- **`sniffer_egg`** (1): sniffer_egg
- **`snow_layer`** (1): snow
- **`soul_fire`** (1): soul_fire
- **`speleothem`** (2): pointed_dripstone, sulfur_spike
- **`spore_blossom`** (1): spore_blossom
- **`stairs`** (97): white_wool_stairs, orange_wool_stairs, magenta_wool_stairs, light_blue_wool_stairs, yellow_wool_stairs, lime_wool_stairs, pink_wool_stairs, gray_wool_stairs, light_gray_wool_stairs, cyan_wool_stairs, purple_wool_stairs, blue_wool_stairs, brown_wool_stairs, green_wool_stairs, red_wool_stairs, black_wool_stairs, oak_stairs, cobblestone_stairs, brick_stairs, stone_brick_stairs, mud_brick_stairs, resin_brick_stairs, nether_brick_stairs, sandstone_stairs, spruce_stairs, birch_stairs, jungle_stairs, quartz_stairs, acacia_stairs, cherry_stairs, dark_oak_stairs, pale_oak_stairs, mangrove_stairs, poplar_stairs, bamboo_stairs, bamboo_mosaic_stairs, prismarine_stairs, prismarine_brick_stairs, dark_prismarine_stairs, red_sandstone_stairs, purpur_stairs, white_concrete_stairs, orange_concrete_stairs, magenta_concrete_stairs, light_blue_concrete_stairs, yellow_concrete_stairs, lime_concrete_stairs, pink_concrete_stairs, gray_concrete_stairs, light_gray_concrete_stairs, cyan_concrete_stairs, purple_concrete_stairs, blue_concrete_stairs, brown_concrete_stairs, green_concrete_stairs, red_concrete_stairs, black_concrete_stairs, polished_granite_stairs, smooth_red_sandstone_stairs, mossy_stone_brick_stairs, polished_diorite_stairs, mossy_cobblestone_stairs, end_stone_brick_stairs, stone_stairs, smooth_sandstone_stairs, smooth_quartz_stairs, granite_stairs, andesite_stairs, red_nether_brick_stairs, polished_andesite_stairs, diorite_stairs, crimson_stairs, warped_stairs, blackstone_stairs, polished_blackstone_brick_stairs, polished_blackstone_stairs, tuff_stairs, polished_tuff_stairs, tuff_brick_stairs, sulfur_stairs, polished_sulfur_stairs, sulfur_brick_stairs, cinnabar_stairs, polished_cinnabar_stairs, cinnabar_brick_stairs, cut_copper_stairs, exposed_cut_copper_stairs, weathered_cut_copper_stairs, oxidized_cut_copper_stairs, waxed_cut_copper_stairs, waxed_exposed_cut_copper_stairs, waxed_weathered_cut_copper_stairs, waxed_oxidized_cut_copper_stairs, cobbled_deepslate_stairs, polished_deepslate_stairs, deepslate_tile_stairs, deepslate_brick_stairs
- **`stem`** (2): pumpkin_stem, melon_stem
- **`stonecutter`** (1): stonecutter
- **`straw_bed`** (1): straw_bed
- **`structure_void`** (1): structure_void
- **`sugar_cane`** (1): sugar_cane
- **`sweet_berry_bush`** (1): sweet_berry_bush
- **`tall_dry_grass`** (1): tall_dry_grass
- **`tall_seagrass`** (1): tall_seagrass
- **`torch`** (4): torch, redstone_torch, soul_torch, copper_torch
- **`torchflower_crop`** (1): torchflower_crop
- **`trapdoor`** (22): oak_trapdoor, spruce_trapdoor, birch_trapdoor, jungle_trapdoor, acacia_trapdoor, cherry_trapdoor, dark_oak_trapdoor, pale_oak_trapdoor, mangrove_trapdoor, poplar_trapdoor, bamboo_trapdoor, iron_trapdoor, crimson_trapdoor, warped_trapdoor, copper_trapdoor, exposed_copper_trapdoor, weathered_copper_trapdoor, oxidized_copper_trapdoor, waxed_copper_trapdoor, waxed_exposed_copper_trapdoor, waxed_weathered_copper_trapdoor, waxed_oxidized_copper_trapdoor
- **`tripwire`** (1): tripwire
- **`tripwire_hook`** (1): tripwire_hook
- **`turtle_egg`** (1): turtle_egg
- **`twisting_vines`** (1): twisting_vines
- **`twisting_vines_plant`** (1): twisting_vines_plant
- **`vine`** (1): vine
- **`wall`** (32): resin_brick_wall, cobblestone_wall, mossy_cobblestone_wall, brick_wall, prismarine_wall, red_sandstone_wall, mossy_stone_brick_wall, granite_wall, stone_brick_wall, mud_brick_wall, nether_brick_wall, andesite_wall, red_nether_brick_wall, sandstone_wall, end_stone_brick_wall, diorite_wall, blackstone_wall, polished_blackstone_brick_wall, polished_blackstone_wall, tuff_wall, polished_tuff_wall, tuff_brick_wall, sulfur_wall, polished_sulfur_wall, sulfur_brick_wall, cinnabar_wall, polished_cinnabar_wall, cinnabar_brick_wall, cobbled_deepslate_wall, polished_deepslate_wall, deepslate_tile_wall, deepslate_brick_wall
- **`wall_banner`** (16): white_wall_banner, orange_wall_banner, magenta_wall_banner, light_blue_wall_banner, yellow_wall_banner, lime_wall_banner, pink_wall_banner, gray_wall_banner, light_gray_wall_banner, cyan_wall_banner, purple_wall_banner, blue_wall_banner, brown_wall_banner, green_wall_banner, red_wall_banner, black_wall_banner
- **`wall_hanging_sign`** (13): oak_wall_hanging_sign, spruce_wall_hanging_sign, birch_wall_hanging_sign, acacia_wall_hanging_sign, cherry_wall_hanging_sign, jungle_wall_hanging_sign, dark_oak_wall_hanging_sign, pale_oak_wall_hanging_sign, mangrove_wall_hanging_sign, poplar_wall_hanging_sign, crimson_wall_hanging_sign, warped_wall_hanging_sign, bamboo_wall_hanging_sign
- **`wall_sign`** (13): oak_wall_sign, spruce_wall_sign, birch_wall_sign, acacia_wall_sign, cherry_wall_sign, jungle_wall_sign, dark_oak_wall_sign, pale_oak_wall_sign, mangrove_wall_sign, poplar_wall_sign, bamboo_wall_sign, crimson_wall_sign, warped_wall_sign
- **`wall_skull(creeper)`** (1): creeper_wall_head
- **`wall_skull(dragon)`** (1): dragon_wall_head
- **`wall_skull(player)`** (1): player_wall_head
- **`wall_skull(skeleton)`** (1): skeleton_wall_skull
- **`wall_skull(wither_skeleton)`** (1): wither_skeleton_wall_skull
- **`wall_skull(zombie)`** (1): zombie_wall_head
- **`wall_torch`** (4): wall_torch, redstone_wall_torch, soul_wall_torch, copper_wall_torch
- **`weeping_vines`** (1): weeping_vines
- **`weeping_vines_plant`** (1): weeping_vines_plant
