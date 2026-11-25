# Blocks: per-state static facts (26.3)

- Verified by suites: `block-facts` (every field of ZBF1 except the shapes themselves; see
  `docs/specs/parity/block-facts.md`).
- Sources: local reference reading of the 26.3 client (2026-11-25, reader session); data-generator
  `reports/blocks.json` (state ids, property order, default state); block tags from the game's built-in data pack
  (read at runtime by Zenith, never copied). Written in our own words.
- Related specs: [`shapes.md`](shapes.md), [`shapes-catalogue.md`](shapes-catalogue.md),
  [`block-table.md`](block-table.md) (per-block values of every property-level fact), [`sound-types.md`](sound-types.md);
  `docs/specs/registry/identifiers-registries-tags.md` (§3 state ids, §4 tags); `docs/specs/math/random.md` (position
  seed); `docs/specs/worldgen/density-functions.md` §7 and `features.md` §3.1 (consumers of these facts).

## Summary

A block's static facts come from three layers, applied in this order:

1. **Block properties** — a record filled in at registration (map colour function, strength, sound, light function,
   flags such as "no collision", "replaceable", push reaction, instrument, …). Values per block are in
   `block-table.md`; this document defines the record, its defaults and how copies work.
2. **Class behaviour** — the block's class can override any per-state query (shapes, fluid state, random ticking,
   skylight, light dampening, pathfinding, signal flags, …). This document lists every override in 26.3 by class;
   `block-table.md` names each block's class.
3. **Derived caches** — at bootstrap each state computes and stores a fixed set of facts from 1 and 2 (occlusion,
   solid render, sturdiness, legacy "solid", skylight, light dampening, …) in a fixed order (§1).

Several facts consult **block tags** (`#minecraft:causes_suffocation`, `#minecraft:blocks_motion_in_heightmap`,
`#minecraft:blocks_motion_in_heightmap_no_leaves`, `#minecraft:shulker_boxes`, `#minecraft:unstable_bottom_center`,
`#minecraft:water` for fluids, and others). Tags are data: Zenith loads them from the installed game at runtime
(registry spec §4). The parity oracle binds the built-in tags before dumping.

## Inputs and state

### The properties record (defaults)

| Field | Default | Set by (registration call) |
|---|---|---|
| map colour function | every state → NONE (id 0) | a constant colour, a dye colour (§4.2) or a per-state function |
| has collision | true | "no collision" also clears *can occlude* |
| can occlude | true | "no occlusion" |
| sound type | STONE | sound (block-set / wood-type classes override it, §3.13) |
| light emission function | 0 | per-state function (table column `light`) |
| destroy time (hardness) | 0 | strength(h) sets hardness h and resistance h; strength(h, r); destroyTime(h); "instabreak" = strength(0) |
| explosion resistance | 0 | stored as max(0, r) |
| requires correct tool | false | |
| random ticks | false | |
| friction / speed factor / jump factor | 0.6 / 1.0 / 1.0 (single precision) | |
| bounce restitution / fall-distance reduction | 0 / 0 | |
| dynamic shape | false | disables the per-state shape cache (shapes.md §5.2) |
| is air | false | the three air blocks |
| ignited by lava | false | |
| liquid (legacy flag) | false | water, lava, bubble column |
| force solid on / force solid off | false / false | |
| push reaction | PUSH_PULL | one of PUSH_PULL, PUSH, POPPED, IMMOVEABLE, IGNORE_ENTITY (the last is used by no block) |
| spawn terrain particles | true | false only for barrier and structure void |
| instrument | HARP | |
| replaceable | false | |
| valid-spawn predicate | top face sturdy (FULL) and light emission < 14 | not dumped |
| redstone-conductor predicate | "collision shape is a full block" | never / always |
| suffocation predicate | in `#causes_suffocation` **and** collision shape is a full block | never / always / shulker / piston |
| view-blocking predicate | "this record's suffocation predicate" (see §2.19) | near-plane test (stairs, slabs), snow ≥ 8 layers |
| post-process predicate | none | self (mushrooms) / the block above (soul sand, magma block) |
| emissive-rendering predicate | never | magma block (always), sculk sensors (phase active) |
| offset type | none | XZ or XYZ (shapes.md §5.4) |
| required features | the vanilla feature set | (every 26.3 block is in the vanilla set) |

### Copying another block's properties
- **Full copy** copies every field of the legacy copy plus jump factor, the conductor, valid-spawn, post-process,
  suffocation and view-blocking predicates, loot table and description. It does **not** copy fall-distance reduction.
- **Legacy copy** copies hardness, resistance, collision, random ticks, light function, map colour function, sound,
  friction, speed factor, bounce restitution, dynamic shape, can occlude, air, ignited-by-lava, liquid, force solid
  on/off, push reaction, requires tool, offset type, terrain particles, required features, emissive predicate,
  instrument and replaceable. It does not copy jump factor, fall-distance reduction, or any of the predicates (they
  revert to defaults).
- **Stairs** (`registerStair`) = full copy of the base block + near-plane view blocking; **legacy stairs** = legacy
  copy + near-plane view blocking; **slabs** = legacy copy + near-plane view blocking (+ optional destroy time or
  strength override); **walls** = legacy copy + force solid on. Copper stairs/slabs use a full copy of the matching
  cut copper + near-plane view blocking.
- **View-blocking default binding quirk:** the default view-blocking predicate evaluates the suffocation predicate of
  the record it was created in, read at query time. A full copy therefore keeps evaluating the *source* record's
  suffocation predicate even if the copy later changes its own. In 26.3 no block ends up with different answers
  because of this (verified for every full-copy block), so an implementation may use "own suffocation predicate".

### Constructor-level adjustments
- Doors, trapdoors, buttons, pressure plates take their **sound** from their block-set type; fence gates and standing /
  wall signs from their wood type's sound; hanging signs from the wood type's hanging-sign sound (§3.13).
- Infested blocks: hardness = (host's hardness) / 2 and resistance 0.75 (block-table already shows the result).
- Stairs report the **base block's** explosion resistance (equal to their own in 26.3).

## Behavior (ordered rules)

### 1. Bootstrap order (per state, registry order, state-id order)
1. fluid state ← class rule (§3.1);
2. random ticking ← class rule (§2.10);
3. unless the block has a dynamic shape: the shape cache (collision with the empty getter at the origin and the empty
   context; large-collision flag; the 18 sturdiness answers; collision-full-block) — shapes.md §5.2;
4. legacy *solid* (§2.13) — uses the cached collision shape;
5. occlusion shape: the class's occlusion rule if the block can occlude, else EMPTY;
6. solid render ← isShapeFullBlock(occlusion shape);
7. face occlusion shapes (shapes.md §5.1 rule 7);
8. propagates skylight down ← class rule (§2.5);
9. light dampening ← class rule (§3.2), which may read 6 and 8.
Light emission, map colour, push reaction, destroy speed, instrument, replaceable and the predicates are fixed when
the state object is created (before step 1); map colour and light are evaluated **per state** with the state's
property values.

### 2. Boolean facts (ZBF1 `flags`, bit order)

All position-taking queries are evaluated with the empty getter at P0 = (0,0,0) (`parity/block-facts.md`).

1. **is_default_state** — the block's default state (from `blocks.json`).
2. **is_air** — the "air" property (air, void air, cave air).
3. **can_occlude** — the property (false after "no collision" or "no occlusion"). Column `flags` in block-table:
   `nc` (no collision, implies cannot occlude) or `no`.
4. **solid_render** — the occlusion shape is a full block (bootstrap step 6).
5. **propagates_skylight_down** — default: the outline (empty getter, origin, empty context) is **not** a full block
   and the state's fluid is empty. Class overrides:
   - every `VegetationBlock` subclass (saplings, propagule, short/tall grass, ferns, dry grasses, bushes, seagrass,
     tall seagrass, all two-tall plants incl. small dripleaf and pitcher crop, flowers incl. wither rose and
     eyeblossoms, mushrooms, cactus flower, all crops, stems, lily pad, nether wart, sweet berry bush, fungi, roots,
     sprouts, sea pickle, azalea, flower beds, leaf litter, firefly bush): fluid empty;
   - glass-like `TransparentBlock` and subclasses (glass, stained glass, copper grates): true; **tinted glass: false**;
   - liquids: false; chorus plant (`PipeBlock`): false; shulker boxes: false;
   - fences, iron bars, glass panes, copper bars (cross-collision classes) and walls: not waterlogged;
   - barrier, light block, glow lichen: fluid empty;
   - vine, bamboo, pale moss carpet, pale hanging moss: true.
6. **use_shape_for_light_occlusion** — default false. True for: slabs unless type=double; all stairs; pistons when
   extended; piston heads; farmland; dirt path; snow layers; enchanting table; end portal frame; daylight detector;
   lectern; stonecutter; sculk sensor and calibrated sculk sensor; sculk shrieker; shelves.
7. **collision_shape_full_block** — isShapeFullBlock(collision shape at P0, empty context); bamboo, pointed dripstone
   and sulfur spike always report **false**.
8. **has_large_collision_shape** — true for dynamic-shape blocks (no cache); otherwise some axis of the cached
   collision shape has min < 0 or max > 1.
9. **has_dynamic_shape** — the property: moving piston, the 17 shulker boxes, bamboo, scaffolding, powder snow,
   pointed dripstone, sulfur spike.
10. **is_randomly_ticking** — default: the property (`rt` in block-table). Class overrides (they replace the property):
    - crops (wheat, carrots, potatoes, beetroots): age < max age (7, 7, 7, 3); **torchflower crop: its max age is 2
      while its age property only reaches 1, so both states tick**; pitcher crop: lower half and age < 4;
    - cocoa: age < 2; nether wart: age < 3; sweet berry bush: age < 3; chorus flower: age < 5;
    - growing-plant heads (kelp, weeping vines, twisting vines, cave vines): age < 25;
    - leaves (all, incl. mangrove/cherry/azalea/pale oak/poplar): distance = 7 and not persistent;
    - liquids: water never, lava always (the fluid's own random-tick flag);
    - redstone ore and deepslate redstone ore: lit;
    - bamboo: stage = 0;
    - flower pots: only the potted open eyeblossom and potted closed eyeblossom (always), every other pot never;
    - the **weathering** (unwaxed) copper blocks of all 15 copper families (block, cut, chiseled, cut stairs, cut slab,
      door, trapdoor, bars, grate, bulb, lantern, chest, golem statue, lightning rod, chain): true for the unaffected,
      exposed and weathered stages, false for oxidized; waxed copper never ticks.
11. **replaceable** — the property (`repl`).
12. **liquid** — the legacy property (`liq`).
13. **legacy_solid** (`isSolid`) — in this order: force solid on → true; force solid off → false; no shape cache
    (dynamic shape) → false; cached collision shape empty → false; with the collision bounds b: (b.xsize + b.ysize +
    b.zsize)/3 ≥ 0.7291666666666666 → true; b.ysize ≥ 1.0 → true; else false (double arithmetic).
14. **blocks_motion_in_heightmap**, **blocks_motion_in_heightmap_no_leaves** — tag membership (data).
15. **ignited_by_lava** — the property (`lava`).
16. **requires_correct_tool_for_drops** — the property (`tool`).
17. **is_redstone_conductor** — the predicate at P0: default = collision_shape_full_block; `cond:never` / `cond:always`.
18. **is_suffocating** — the predicate at P0: default = in `#causes_suffocation` and collision_shape_full_block;
    `suff:never`, `suff:always`; `suff:shulker` = the block entity is closed, **true when there is no block entity**
    (always true in the suite); `suff:piston` = not extended.
19. **is_view_blocking** (third argument: a world-space box; the suite uses the unit box at P0) — default: the
    suffocation predicate (see the binding quirk above); `view:plane` (stairs, slabs incl. copper/wool/concrete
    variants): true iff some box of the **stored occlusion shape**, translated to the block position, strictly
    intersects the given box (open-interval overlap on all three axes) — with the unit probe box this is "the stored
    occlusion shape is non-empty"; `view:snow8`: layers ≥ 8.
20. **emissive_rendering** — the predicate: `emis:always` (magma block), `emis:sensor-active` (sculk sensor and
    calibrated sculk sensor with phase = active); otherwise false.
21. **is_signal_source** — class: buttons, levers, pressure plates (both kinds), redstone torches (standing and wall),
    repeater and comparator, detector rail, jukebox, daylight detector, redstone block, tripwire hook, lectern, target,
    observer, sculk sensors, lightning rods (all), trapped chest, and redstone wire (its "should signal" switch is
    true outside of its own update, so true in the suite). Everything else false.
22. **has_analog_output_signal** — class: barrel, all cauldrons, furnace / smoker / blast furnace, beehive and bee
    nest, candle cakes, brewing stand, chiseled bookshelf, chests (chest, trapped chest, copper chests; **not** the
    ender chest), cake, copper bulbs, copper golem statues, command blocks, creaking heart, composter, decorated pot,
    end portal frame, crafter, dispenser and dropper, hopper, jukebox, lectern, detector rail, respawn anchor, sculk
    sensors, shelves, shulker boxes.
23. **has_block_entity** — the class is a block-entity class: banners (standing, wall), barrel, beacon, beehive / bee
    nest, bell, furnaces (3), brewing stand, suspicious sand/gravel (`BrushableBlock`), sculk sensors (2), campfires,
    hanging signs (ceiling, wall), signs (standing, wall), chiseled bookshelf, command blocks (3), comparator, conduit,
    copper golem statues, crafter, creaking heart, daylight detector, decorated pot, dispenser, dropper, enchanting
    table, end gateway, end portal, hopper, jigsaw, jukebox, lectern, moving piston, heads and skulls (standing and
    wall, all types), potent sulfur, sculk catalyst, sculk shrieker, shelves, shulker boxes, spawner, structure block,
    test block, test instance block, trial spawner, vault, chests (chest, trapped chest, copper chests, ender chest).
24. **spawn_terrain_particles** — the property (`noparticles` marks false).
25. **is_light_permeable** — ¬solid_render ∨ light_block = 0.
26. **pathfindable LAND / WATER / AIR** — default: LAND and AIR = ¬collision_shape_full_block, WATER = the state's
    fluid is in `#minecraft:water`. Overrides (types not listed are false):
    - doors and fence gates: LAND = AIR = open (WATER false); trapdoors: LAND = AIR = open, WATER = waterlogged;
    - slabs: LAND false, AIR false, WATER = fluid is water;
    - snow layers: LAND = layers < 5, others false;
    - vegetation classes (§2.5 list): AIR = true when the block has no collision, otherwise the default; the other
      types default — **except sea pickle and azalea, which override to all false**;
    - liquids: all three = the block's fluid is not in the fluid tag `#minecraft:lava` (water true, lava false);
      powder snow: all true;
    - shelves and copper golem statues: WATER = fluid is water, others false;
    - **all false**: stairs, beds, fences, walls, panes/bars, chains, lanterns, rods (end rod, lightning rods), cactus,
      cake, candle cakes, cauldrons, chests and ender chest, cocoa, shelf mushroom, conduit, decorated pot, dragon egg,
      dried ghast, enchanting table, end portal frame, farmland, dirt path, flower pots, grindstone, heads/skulls,
      heavy core, hopper, lectern, moving piston, piston, piston head, respawn anchor, sculk sensors, sniffer egg,
      soul sand, mud, speleothems, stonecutter, bell, campfires, anvils, brewing stand, wall hanging signs, composter,
      chorus plant, bamboo.
27. **possible_to_respawn_in** — default ¬legacy_solid ∧ ¬liquid; signs (all four kinds), banners (standing and
    wall) and pressure plates: true.
28. **exception_for_connection** — leaves (any leaves class), barrier, carved pumpkin, jack o'lantern, melon, pumpkin,
    or the state is in `#minecraft:shulker_boxes`.
29. **can_be_replaced_by_fluid (water)** — default: replaceable ∨ ¬legacy_solid; end portal and end gateway: false.

### 3. Scalars, enums and the fluid state

#### 3.1 Fluid state
- Default: the empty fluid (id `minecraft:empty`, amount 0, height 0, not source, empty).
- **Waterlogged blocks** (any class with a `waterlogged` property): waterlogged=true → water source with
  falling=false; **copper grates** (`WaterloggedTransparentBlock`) give the water source with **falling=true**.
- Always water source (falling=false): seagrass, tall seagrass, kelp, kelp plant, bubble column.
- Liquids: level 0 → source (falling false); level ℓ = 1..7 → flowing fluid with amount 8 − ℓ, falling false;
  level ≥ 8 → flowing fluid with amount 8, falling true. (Water: ids `minecraft:water` / `minecraft:flowing_water`;
  lava: `minecraft:lava` / `minecraft:flowing_lava`.)
- Amount: sources 8, flowing = its `level` property (1..8). Own height = amount / 9 in single precision (source
  0.8888889). Fluid ids as in `fluid_id` of ZBF1.

#### 3.2 light_block (light dampening)
Default: 15 if solid_render, else 0 if propagates_skylight_down, else 1. Overrides: leaves → always 1; tinted glass →
always 15.

#### 3.3 light_emission
The property function evaluated per state (block-table column `light`): a constant; `lit?N:0` (lit property);
`lit?3*candles:0` (candles); `berries?14:0` (cave vines); `anyface?7:0` (glow lichen: any face property true);
`=level` (light block); `waterlogged?3+3*pickles:0` (sea pickle; a non-waterlogged pickle is "dead"); respawn anchor
floor(charge/4·15) computed in single precision (charge 0..4 → 0, 3, 7, 11, 15); trial spawner by state (inactive 0,
waiting_for_players 4, active 8, waiting_for_reward_ejection 8, ejecting_reward 8, cooldown 0); vault by state
(inactive 6, active / unlocking / ejecting 12). Copper bulbs: lit → 15, 12, 8, 4 for unaffected, exposed, weathered,
oxidized (waxed alike).

#### 3.4 map_color (id, rgb)
The property function per state; block-table gives the colour name and any per-state rule. Per-state rules in 26.3:
logs / stems-like pillars by `axis` (y → top colour, x/z → side colour), beds (head → WOOL, foot → the dye colour),
wheat (age ≥ 6 → COLOR_YELLOW, else PLANT), barrier and light (waterlogged → WATER, else NONE). "X's default colour"
references were resolved to names in block-table. See §4 for the id/rgb table.

#### 3.5 push_reaction
The property (block-table column `push`); serialized as the enum name (PUSH_PULL, PUSH, POPPED, IMMOVEABLE,
IGNORE_ENTITY).

#### 3.6 instrument
The property (block-table column `instrument`), serialized lower-case: harp, basedrum, snare, hat, bass, flute, bell,
guitar, chime, xylophone, iron_xylophone, cow_bell, didgeridoo, bit, banjo, pling, trumpet, trumpet_exposed,
trumpet_oxidized, trumpet_weathered, zombie, skeleton, creeper, dragon, wither_skeleton, piglin, custom_head.

#### 3.7 render_shape
INVISIBLE for air, void air, cave air, water, lava, bubble column, moving piston, barrier, light, structure void, end
portal, end gateway; MODEL for every other state.

#### 3.8 offset_type and offsets
From the property (`off:xz`, `off:xyz`; shapes.md §5.4 for the vectors). XZ: mangrove propagule, tall seagrass, all
flowers (dandelion, golden dandelion, torchflower, poppy, blue orchid, allium, azure bluet, the four tulips, oxeye
daisy, cornflower, wither rose, lily of the valley, open/closed eyeblossom), sunflower, lilac, rose bush, peony, tall
grass, large fern, pitcher plant, bamboo sapling, bamboo, warped roots, nether sprouts, crimson roots, pointed
dripstone, sulfur spike, hanging roots. XYZ: short grass, fern, short dry grass, tall dry grass, small dripleaf.
Maximum offsets: horizontal 0.25 except speleothems 0.125; vertical 0.2 except small dripleaf 0.1.

#### 3.9 post-process position
None for every block except brown and red mushroom (the position itself) and soul sand and magma block (the position
above).

#### 3.10 destroy_speed, explosion_resistance
Hardness and resistance from block-table (`-1` = indestructible). Explosion resistance is stored as max(0, r).

#### 3.11 friction, speed_factor, jump_factor, bounce_restitution, fall_distance_reduction
Block-table flags `fr=`, `spd=`, `jmp=`, `bnc=`, `fdr=` (defaults 0.6, 1, 1, 0, 0, single precision). In 26.3: ice,
packed ice, frosted ice 0.98, blue ice 0.989, slime block 0.8; soul sand and honey block speed 0.4; honey block jump
0.5; slime block bounce 1; beds, straw bed and shelf mushroom bounce 0.75 with fall-distance reduction 0.5.

#### 3.12 shade_brightness
Default: 0.2 if collision_shape_full_block else 1.0. Overrides: snow layers 0.2 at 8 layers else 1.0; soul sand and mud
0.2; glass-like `TransparentBlock` family (glass, stained glass, tinted glass, copper grates), barrier, light, structure
void: 1.0.

#### 3.13 Sound type
The property, except: doors, trapdoors, buttons and pressure plates use their block-set sound (iron → IRON, copper →
COPPER, gold → METAL, stone and polished blackstone → STONE, cherry → CHERRY_WOOD, crimson and warped → NETHER_WOOD,
bamboo → BAMBOO_WOOD, every other wood → WOOD); fence gates and standing/wall signs use the wood type's sound (cherry
CHERRY_WOOD, crimson/warped NETHER_WOOD, bamboo BAMBOO_WOOD, else WOOD); ceiling and wall hanging signs use the hanging
sound (CHERRY_WOOD_HANGING_SIGN, NETHER_WOOD_HANGING_SIGN, BAMBOO_WOOD_HANGING_SIGN, else HANGING_SIGN); decorated pot:
cracked → DECORATED_POT_CRACKED else DECORATED_POT. Block-table shows the resolved name; `sound-types.md` maps each
name to volume, pitch and the five sound-event ids.

### 4. Map colours

#### 4.1 Id and rgb (`map_color_id`, `map_color_rgb` as 0xRRGGBB)

| id | name | rgb | id | name | rgb |
|---|---|---|---|---|---|
| 0 | NONE | 000000 | 31 | DIAMOND | 5CDBD5 |
| 1 | GRASS | 7FB238 | 32 | LAPIS | 4A80FF |
| 2 | SAND | F7E9A3 | 33 | EMERALD | 00D93A |
| 3 | WOOL | C7C7C7 | 34 | PODZOL | 815631 |
| 4 | FIRE | FF0000 | 35 | NETHER | 700200 |
| 5 | ICE | A0A0FF | 36 | TERRACOTTA_WHITE | D1B1A1 |
| 6 | METAL | A7A7A7 | 37 | TERRACOTTA_ORANGE | 9F5224 |
| 7 | PLANT | 007C00 | 38 | TERRACOTTA_MAGENTA | 95576C |
| 8 | SNOW | FFFFFF | 39 | TERRACOTTA_LIGHT_BLUE | 706C8A |
| 9 | CLAY | A4A8B8 | 40 | TERRACOTTA_YELLOW | BA8524 |
| 10 | DIRT | 976D4D | 41 | TERRACOTTA_LIGHT_GREEN | 677535 |
| 11 | STONE | 707070 | 42 | TERRACOTTA_PINK | A04D4E |
| 12 | WATER | 4040FF | 43 | TERRACOTTA_GRAY | 392923 |
| 13 | WOOD | 8F7748 | 44 | TERRACOTTA_LIGHT_GRAY | 876B62 |
| 14 | QUARTZ | FFFCF5 | 45 | TERRACOTTA_CYAN | 575C5C |
| 15 | COLOR_ORANGE | D87F33 | 46 | TERRACOTTA_PURPLE | 7A4958 |
| 16 | COLOR_MAGENTA | B24CD8 | 47 | TERRACOTTA_BLUE | 4C3E5C |
| 17 | COLOR_LIGHT_BLUE | 6699D8 | 48 | TERRACOTTA_BROWN | 4C3223 |
| 18 | COLOR_YELLOW | E5E533 | 49 | TERRACOTTA_GREEN | 4C522A |
| 19 | COLOR_LIGHT_GREEN | 7FCC19 | 50 | TERRACOTTA_RED | 8E3C2E |
| 20 | COLOR_PINK | F27FA5 | 51 | TERRACOTTA_BLACK | 251610 |
| 21 | COLOR_GRAY | 4C4C4C | 52 | CRIMSON_NYLIUM | BD3031 |
| 22 | COLOR_LIGHT_GRAY | 999999 | 53 | CRIMSON_STEM | 943F61 |
| 23 | COLOR_CYAN | 4C7F99 | 54 | CRIMSON_HYPHAE | 5C191D |
| 24 | COLOR_PURPLE | 7F3FB2 | 55 | WARPED_NYLIUM | 167E86 |
| 25 | COLOR_BLUE | 334CB2 | 56 | WARPED_STEM | 3A8E8C |
| 26 | COLOR_BROWN | 664C33 | 57 | WARPED_HYPHAE | 562C3E |
| 27 | COLOR_GREEN | 667F33 | 58 | WARPED_WART_BLOCK | 14B485 |
| 28 | COLOR_RED | 993333 | 59 | DEEPSLATE | 646464 |
| 29 | COLOR_BLACK | 191919 | 60 | RAW_IRON | D8AF93 |
| 30 | GOLD | FAEE4D | 61 | GLOW_LICHEN | 7FA796 |

#### 4.2 Dye colours
Dye → map colour (and → terracotta colour): white SNOW (TERRACOTTA_WHITE), orange COLOR_ORANGE, magenta COLOR_MAGENTA,
light_blue COLOR_LIGHT_BLUE, yellow COLOR_YELLOW, lime COLOR_LIGHT_GREEN, pink COLOR_PINK, gray COLOR_GRAY, light_gray
COLOR_LIGHT_GRAY, cyan COLOR_CYAN, purple COLOR_PURPLE, blue COLOR_BLUE, brown COLOR_BROWN, green COLOR_GREEN, red
COLOR_RED, black COLOR_BLACK (each terracotta colour is TERRACOTTA_<same suffix>; lime → TERRACOTTA_LIGHT_GREEN).
Exceptions resolved in block-table: the purple shulker box is TERRACOTTA_PURPLE, the white candle is WOOL, the
coloured terracotta blocks use the terracotta colour.

## Edge cases and known quirks

1. **Torchflower crop always ticks** (max age 2 vs property range 0..1).
2. **Copper grates' water is "falling"** (§3.1) — the only waterlogged blocks whose fluid has falling = true.
3. **Shulker boxes suffocate** when there is no block entity (predicate defaults to true) and report
   *large collision shape* = true (dynamic shape).
4. **Dynamic shapes are never "solid"** unless forced (shulker boxes, bamboo, the speleothems and the moving piston are
   forced on; scaffolding and powder snow are not, so they are not solid).
5. **Overriding classes ignore the no-collision flag** (shapes-catalogue): scaffolding, wall hanging signs and the
   pitcher crop have collision even though their properties say "no collision"; their *legacy solid*, conductor,
   suffocation, pathfinding and shade results follow the overridden collision.
6. **Vegetation AIR pathfinding** reads the *property* "has collision", not the actual collision shape (pitcher crop:
   AIR true).
7. **The view-blocking default** binds to the record it was created in (Inputs and state) — no observable effect.
8. **Stairs' explosion resistance** comes from the base block (no observable effect).
9. **Fall-distance reduction is never copied** by full or legacy copies.

## Constants and IDs

| Name | Value |
|---|---|
| legacy-solid average-extent threshold | 0.7291666666666666 |
| legacy-solid height threshold | 1.0 |
| default friction / speed / jump | 0.6F / 1.0F / 1.0F |
| fluid own height | amount / 9.0F |
| total blocks / states in 26.3 | 1286 / 35 723 |

## Oracle probe points

`net.minecraft.world.level.block.state.BlockBehaviour.BlockStateBase`: `isAir`, `canOcclude`, `isSolidRender`,
`propagatesSkylightDown`, `useShapeForLightOcclusion`, `isCollisionShapeFullBlock`, `hasLargeCollisionShape`,
`isRandomlyTicking`, `canBeReplaced`, `liquid`, `isSolid`, `is(TagKey)`, `ignitedByLava`,
`requiresCorrectToolForDrops`, `isRedstoneConductor`, `isSuffocating`, `isViewBlocking`, `emissiveRendering`,
`isSignalSource`, `hasAnalogOutputSignal`, `hasBlockEntity`, `shouldSpawnTerrainParticles`, `isLightPermeable`,
`isPathfindable`, `getLightDampening`, `getLightEmission`, `getMapColor`, `getPistonPushReaction`, `instrument`,
`getRenderShape`, `hasOffsetFunction`, `getOffset`, `getPostProcessPos`, `getDestroySpeed`, `getShadeBrightness`,
`getSoundType`, `getFluidState`; `Block`: `hasDynamicShape`, `getExplosionResistance`, `getFriction`,
`getSpeedFactor`, `getJumpFactor`, `getBounceRestitution`, `getFallDistanceReduction`, `isPossibleToRespawnInThis`,
`isExceptionForConnection`. All are dumped by the `BlockFacts` oracle (ZBF1).

## Open questions

1. **[oracle] Tag-dependent bits.** `blocks_motion_in_heightmap(_no_leaves)`, `is_suffocating` (default predicate),
   `is_view_blocking` (default predicate), `exception_for_connection` (shulker tag) and `pathfindable WATER` (water
   tag) depend on built-in tag contents that Zenith reads from the install. Probe: run ZBF1 twice, once with tags bound
   (the normal mode) and once without (`BlockFacts` before binding), and confirm the only differing bits are these
   five, so the implementer knows exactly which facts must wait for tag loading.
2. **[oracle] Map-colour ids.** The id/rgb table above was read from the colour definitions; confirm with ZBF1
   `map_color_id` / `map_color_rgb` for one block per colour (e.g. stone 11 / 0x707070, glow lichen 61 / 0x7FA796).
3. **(implementer) Class lists proven only where they change a dumped bit.** The class lists of §2 (vegetation for
   skylight and pathfinding, signal source, analog output, block entity, all-false pathfinding) are keyed by the class
   names of `block-table.md`. The suite is 100%, but a list entry that does not change any dumped bit is unproven: for
   example, removing `FlowerBlock` from the vegetation list changes no bit, because flowers get the same skylight answer
   from the default rule. The vegetation membership should be re-confirmed against the class hierarchy by the reader.
4. **(implementer) `pathfindable WATER` uses fluid ids, not a loaded fluid tag.** `zenith-blocks` treats the fluid as
   "in `#minecraft:water`" when it is `water` or `flowing_water` (and "in `#minecraft:lava`" for `lava` /
   `flowing_lava`); only the five block tags of §2 open question 1 are read from the install, as block tags. A data pack
   that changed the fluid tags would not be seen. Equivalent in the unmodified game.

## Parity status
`block-facts` (2026-11-25): **535,845/535,845** checks identical — all 35,723 states × 15 field groups (identity,
flags, numbers, sound, fluid, six shape kinds, position-dependent shapes, face sturdiness / fullness / occlusion).
Context-dependent collision (held scaffolding, powder snow, standable fluids) and collision sweeps are not covered yet.
