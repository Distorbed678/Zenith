# Ticks: block reactions — index of every block of 26.3 and the rules that cover its world-reaction hooks

- Verified by suites: `block-updates`
- Sources: local reference reading of the 26.3 block classes (2026-11-26, reader session) — every class registered for a block, with its overrides of the reaction hooks `updateShape`, `canSurvive`, `neighborChanged`, `onPlace`,
  `affectNeighborsAfterRemoval`, `updateIndirectNeighbourShapes` and `shouldChangedStateKeepBlockEntity` (own and inherited) — and the class column of [`../../blocks/block-table.md`](../../blocks/block-table.md) for the block → class assignment of all 1286 blocks.
  Everything is in our own words.
- Related specs: the framework — [`../block-hooks.md`](../block-hooks.md), [`../neighbor-and-shape-updates.md`](../neighbor-and-shape-updates.md), [`../set-block-flags-and-sequence.md`](../set-block-flags-and-sequence.md),
  [`../scheduled-ticks-and-block-events.md`](../scheduled-ticks-and-block-events.md), [`../server-tick.md`](../server-tick.md); the already-specified families — [`../fluids.md`](../fluids.md), [`../pistons.md`](../pistons.md),
  [`../redstone.md`](../redstone.md), [`../redstone-power.md`](../redstone-power.md), [`../redstone-components.md`](../redstone-components.md); block facts — [`../../blocks/state-facts.md`](../../blocks/state-facts.md),
  [`../../blocks/shapes.md`](../../blocks/shapes.md), [`../../blocks/state-transforms.md`](../../blocks/state-transforms.md); the random-tick / scheduled-tick behaviour written by the other reader (`tick-index.md`, `tick-growth.md`, `tick-spread.md`, … in this folder).

## Summary

This folder specifies how every block **reacts to its surroundings**: shape-update recomputation, survival checks that turn a block into air (with drops), neighbour-driven writes, `onPlace` / removal side effects and the delayed ticks those hooks schedule.
It does **not** repeat the framework (delivery order of updates, flags, tick queue) nor the random / scheduled tick *behaviour* (growth, spread, decay, fire spreading, gravity falling) which belong to the tick-side files.

| file | families covered |
|---|---|
| [`plants.md`](plants.md) | every vegetation block, crops and stems, farmland, double plants, kelp / vines / growing plants, dripleaf family, sugar cane, cactus, bamboo, chorus, corals, sea pickle, frogspawn, turtle / sniffer eggs, moss carpets, mushrooms |
| [`survival-and-supports.md`](survival-and-supports.md) | torches, face-attached lever / buttons, ladders, signs and banners, lanterns, bells, amethyst clusters, candles and cakes, carpets, pressure plates, scaffolding, flower pots, and the **waterlogging order table** |
| [`connections.md`](connections.md) | fences and gates, walls, bars / panes, stairs, slabs, chests (single / double / copper), shelves and their side chain, huge mushroom blocks, multiface blocks (lichen, sculk vein, resin clump), vines, tripwire shape, chorus plant |
| [`multiblock.md`](multiblock.md) | doors, beds, double plants (pointer), speleothems (pointed dripstone, sulfur spike), piston head |
| [`rails.md`](rails.md) | the **rail-shape solver** (connection search, curves, slopes, powered-rail straight-only rule, update fan-out), rail removal and support |
| [`misc-reactions.md`](misc-reactions.md) | snow, snowy blocks, leaves distance, ice / frosted ice, gravity blocks and concrete powder, note block instrument, path block, sponge, fire / soul fire, nether portal frame, campfire, beehive, composter, target, potent sulfur, creaking heart, carved pumpkin, container removal hooks |

Redstone-driven reactions (power read, repeater / comparator timing, observers, pistons, redstone wire) are **not** repeated; this index points to their sections. Fluids (liquid blocks, bubble columns, waterlogging mechanics) likewise.

## Inputs and state

- **Rule notation** (all files): `AIR` = `updateShape` returns the air state → the framework destroys the block with drops (flags 3); `sched(B, d)` = `scheduleTick(pos, B, d)`; `waterTick` = water fluid tick at the water tick delay (5);
  `dir` = `directionToNeighbour` of the hook; `sturdy` / `center` / `solid` / `rigid` = the four support predicates of [`survival-and-supports.md`](survival-and-supports.md); `tag#x` = block tag `minecraft:x` read from the installed game's data (never copied into the repo).
- **Classes**: the class names are the public names used by [`block-table.md`](../../blocks/block-table.md); an index row says which **hooks the class (or an ancestor) overrides**. A class with an empty hook list has only the defaults of `block-hooks.md` §1.
- **Hook defaults** (all blocks): `updateShape` returns the state unchanged; `canSurvive` true; `neighborChanged`, `onPlace`, `affectNeighborsAfterRemoval`, `updateIndirectNeighbourShapes` do nothing; `shouldChangedStateKeepBlockEntity` false.

## Behavior (ordered rules)

### 1. Where in a block change these hooks run (reminder, not a re-specification)

For a write at `P` with bit 16 clear (set-block §2): chunk write → *removal hook of the old state* (when the block type changed, or for rails) → *`onPlace` of the new state* (every write, server) → client notification → neighbour update `updateNeighborsAt(P, oldBlock)` (bit 1) → indirect shape updates of the old
state → six shape updates of the new state in W, E, N, S, DOWN, UP order → indirect shape updates of the new state. A shape update that returns another state writes it with `flags & ~(1|32)`; a shape update that returns air calls `destroyBlock(flags 3)`.
**Therefore** an `AIR` result notifies neighbours (and fires the removed block's `affectNeighborsAfterRemoval`), while a state change by a shape update does not notify neighbours.

### 2. Which survival checks fire on which shape-update direction

| pattern | blocks |
|---|---|
| **any direction** re-checks the support | all `VegetationBlock` descendants, snow layer, carpets, frogspawn, sea pickle, leaf litter, vines (except `dir == DOWN`), multiface blocks (only the notifying face), leaves-distance tick, pale moss carpet |
| **only the support direction** | torches (DOWN), wall torches, buttons / levers / ladders / wall signs / banners / wall fans / amethyst / hook (`dir.opposite == FACING` or the connected direction), pressure plates (DOWN), standing signs and banners (DOWN), cake (DOWN), ceiling hanging sign (UP), lantern, bell, doors (lower half, DOWN), cocoa, shelf mushroom, mangrove propagule (UP), spore blossom (UP), hanging roots (UP), big dripleaf leaf (DOWN), wall torches, speleothems (vertical only; removal delayed) |
| **delayed removal** (`sched(B, 1)` then `destroyBlock` in `tick`) | sugar cane, cactus, bamboo, chorus plant / flower, kelp / weeping / twisting / cave vines, big dripleaf stem, hanging moss, farmland (to dirt), dirt path (to dirt), stalagmites (1) and stalactites (2) |
| **by `neighborChanged` instead of `updateShape`** | rails (`rails.md`), redstone wire, repeaters / comparators |
| **never pops** | flower pot, grindstone, wall hanging signs, candles, wall / floor skulls, turtle / sniffer eggs, chests, containers, fences, walls, stairs, slabs |

### 3. Index by class

One row per block class: the hook overrides found in the class chain, the rule reference that covers them, and **all blocks** of the class (registry order). A block listed under a class marked *default hooks only* needs **no** reaction code.
A class marked "(no reaction hooks)" may still have random-tick, tick or interaction behaviour covered by the tick-side readers.
- **`AirBlock`** (3) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: air, void_air, cave_air
- **`Block`** (194) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: stone, granite, polished_granite, diorite, polished_diorite, andesite, polished_andesite, dirt, coarse_dirt, cobblestone, oak_planks, spruce_planks, birch_planks, jungle_planks, acacia_planks, cherry_planks, dark_oak_planks, pale_oak_planks, mangrove_planks, poplar_planks, bamboo_planks, bamboo_mosaic, bedrock, lapis_block, sandstone, chiseled_sandstone, cut_sandstone, white_wool, orange_wool, magenta_wool, light_blue_wool, yellow_wool, lime_wool, pink_wool, gray_wool, light_gray_wool, cyan_wool, purple_wool, blue_wool, brown_wool, green_wool, red_wool, black_wool, gold_block, iron_block, bricks, bookshelf, mossy_cobblestone, obsidian, diamond_block, snow_block, clay, soul_soil, glowstone, stone_bricks, mossy_stone_bricks, cracked_stone_bricks, chiseled_stone_bricks, packed_mud, mud_bricks, melon, resin_block, resin_bricks, chiseled_resin_bricks, nether_bricks, end_stone, emerald_block, quartz_block, chiseled_quartz_block, white_terracotta, orange_terracotta, magenta_terracotta, light_blue_terracotta, yellow_terracotta, lime_terracotta, pink_terracotta, gray_terracotta, light_gray_terracotta, cyan_terracotta, purple_terracotta, blue_terracotta, brown_terracotta, green_terracotta, red_terracotta, black_terracotta, prismarine, prismarine_bricks, dark_prismarine, sea_lantern, terracotta, coal_block, packed_ice, red_sandstone, chiseled_red_sandstone, cut_red_sandstone, smooth_stone, smooth_sandstone, smooth_quartz, smooth_red_sandstone, purpur_block, end_stone_bricks, nether_wart_block, red_nether_bricks, white_concrete, orange_concrete, magenta_concrete, light_blue_concrete, yellow_concrete, lime_concrete, pink_concrete, gray_concrete, light_gray_concrete, cyan_concrete, purple_concrete, blue_concrete, brown_concrete, green_concrete, red_concrete, black_concrete, dried_kelp_block, dead_tube_coral_block, dead_brain_coral_block, dead_bubble_coral_block, dead_fire_coral_block, dead_horn_coral_block, fletching_table, warped_wart_block, shroomlight, crimson_planks, warped_planks, honeycomb_block, netherite_block, ancient_debris, lodestone, blackstone, polished_blackstone, polished_blackstone_bricks, cracked_polished_blackstone_bricks, chiseled_polished_blackstone, gilded_blackstone, chiseled_nether_bricks, cracked_nether_bricks, quartz_bricks, tuff, polished_tuff, chiseled_tuff, tuff_bricks, chiseled_tuff_bricks, sulfur, polished_sulfur, sulfur_bricks, chiseled_sulfur, cinnabar, polished_cinnabar, cinnabar_bricks, chiseled_cinnabar, calcite, copper_block, exposed_copper, weathered_copper, oxidized_copper, waxed_copper_block, waxed_exposed_copper, waxed_weathered_copper, waxed_oxidized_copper, cut_copper, exposed_cut_copper, weathered_cut_copper, oxidized_cut_copper, waxed_cut_copper, waxed_exposed_cut_copper, waxed_weathered_cut_copper, waxed_oxidized_cut_copper, chiseled_copper, exposed_chiseled_copper, weathered_chiseled_copper, oxidized_chiseled_copper, waxed_chiseled_copper, waxed_exposed_chiseled_copper, waxed_weathered_chiseled_copper, waxed_oxidized_chiseled_copper, dripstone_block, cobbled_deepslate, polished_deepslate, deepslate_tiles, deepslate_bricks, chiseled_deepslate, cracked_deepslate_bricks, cracked_deepslate_tiles, smooth_basalt, raw_iron_block, raw_copper_block, raw_gold_block, reinforced_deepslate
- **`GrassBlock`** (1) — hooks: updateShape — covered by: [misc](misc-reactions.md) §1.1 (SNOWY rule)
  blocks: grass_block
- **`SnowyBlock`** (1) — hooks: updateShape — covered by: [misc](misc-reactions.md) §1.1
  blocks: podzol
- **`RotatedPillarBlock`** (60) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: pale_oak_wood, oak_log, spruce_log, birch_log, jungle_log, acacia_log, cherry_log, dark_oak_log, pale_oak_log, mangrove_log, poplar_log, muddy_mangrove_roots, bamboo_block, stripped_spruce_log, stripped_birch_log, stripped_jungle_log, stripped_acacia_log, stripped_cherry_log, stripped_dark_oak_log, stripped_pale_oak_log, stripped_oak_log, stripped_mangrove_log, stripped_poplar_log, stripped_bamboo_block, oak_wood, spruce_wood, birch_wood, jungle_wood, acacia_wood, cherry_wood, dark_oak_wood, mangrove_wood, poplar_wood, stripped_oak_wood, stripped_spruce_wood, stripped_birch_wood, stripped_jungle_wood, stripped_acacia_wood, stripped_cherry_wood, stripped_dark_oak_wood, stripped_pale_oak_wood, stripped_mangrove_wood, stripped_poplar_wood, basalt, polished_basalt, quartz_pillar, purpur_pillar, bone_block, warped_stem, stripped_warped_stem, warped_hyphae, stripped_warped_hyphae, crimson_stem, stripped_crimson_stem, crimson_hyphae, stripped_crimson_hyphae, deepslate, ochre_froglight, verdant_froglight, pearlescent_froglight
- **`SaplingBlock`** (9) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §1 (vegetation rule, `supports_vegetation`)
  blocks: oak_sapling, spruce_sapling, birch_sapling, jungle_sapling, acacia_sapling, cherry_sapling, dark_oak_sapling, pale_oak_sapling, poplar_sapling
- **`MangrovePropaguleBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §1.3 (standing: `supports_mangrove_propagule`), §5 (water tick first, AIR from UP; hanging variant needs `supports_hanging_mangrove_propagule` above)
  blocks: mangrove_propagule
- **`LiquidBlock`** (2) — hooks: updateShape, neighborChanged, onPlace — covered by: [fluids](../fluids.md) §3, §10
  blocks: water, lava
- **`SandBlock`** (2) — hooks: updateShape, onPlace — covered by: [misc](misc-reactions.md) §4
  blocks: sand, red_sand
- **`BrushableBlock`** (2) — hooks: updateShape, onPlace — covered by: [misc](misc-reactions.md) §4.3
  blocks: suspicious_sand, suspicious_gravel
- **`ColoredFallingBlock`** (1) — hooks: updateShape, onPlace — covered by: [misc](misc-reactions.md) §4
  blocks: gravel
- **`DropExperienceBlock`** (16) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: gold_ore, deepslate_gold_ore, iron_ore, deepslate_iron_ore, coal_ore, deepslate_coal_ore, nether_gold_ore, lapis_ore, deepslate_lapis_ore, diamond_ore, deepslate_diamond_ore, emerald_ore, deepslate_emerald_ore, nether_quartz_ore, copper_ore, deepslate_copper_ore
- **`MangroveRootsBlock`** (1) — hooks: updateShape — covered by: [plants](plants.md) §5 (water tick only)
  blocks: mangrove_roots
- **`TintedParticleLeavesBlock`** (5) — hooks: updateShape — covered by: [misc](misc-reactions.md) §2
  blocks: oak_leaves, birch_leaves, jungle_leaves, acacia_leaves, dark_oak_leaves
- **`LeavesBlock`** (1) — hooks: updateShape — covered by: [misc](misc-reactions.md) §2
  blocks: spruce_leaves
- **`UntintedParticleLeavesBlock`** (7) — hooks: updateShape — covered by: [misc](misc-reactions.md) §2
  blocks: cherry_leaves, pale_oak_leaves, red_poplar_leaves, orange_poplar_leaves, yellow_poplar_leaves, azalea_leaves, flowering_azalea_leaves
- **`MangroveLeavesBlock`** (1) — hooks: updateShape — covered by: [misc](misc-reactions.md) §2
  blocks: mangrove_leaves
- **`SpongeBlock`** (1) — hooks: neighborChanged, onPlace — covered by: [misc](misc-reactions.md) §7
  blocks: sponge
- **`WetSpongeBlock`** (1) — hooks: onPlace — covered by: [misc](misc-reactions.md) §7.4
  blocks: wet_sponge
- **`TransparentBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: glass
- **`DispenserBlock`** (1) — hooks: neighborChanged, affectNeighborsAfterRemoval — covered by: redstone-components §8.3 + [misc](misc-reactions.md) §10
  blocks: dispenser
- **`NoteBlock`** (1) — hooks: updateShape, neighborChanged — covered by: [misc](misc-reactions.md) §5 + redstone-components §8.2
  blocks: note_block
- **`BedBlock`** (16) — hooks: updateShape — covered by: [multiblock](multiblock.md) §2
  blocks: white_bed, orange_bed, magenta_bed, light_blue_bed, yellow_bed, lime_bed, pink_bed, gray_bed, light_gray_bed, cyan_bed, purple_bed, blue_bed, brown_bed, green_bed, red_bed, black_bed
- **`StrawBedBlock`** (1) — hooks: updateShape — covered by: [multiblock](multiblock.md) §2
  blocks: straw_bed
- **`PoweredRailBlock`** (2) — hooks: updateShape, canSurvive, neighborChanged, onPlace, affectNeighborsAfterRemoval — covered by: [rails](rails.md) + redstone-components §9.1
  blocks: powered_rail, activator_rail
- **`DetectorRailBlock`** (1) — hooks: onPlace, updateShape, canSurvive, neighborChanged, affectNeighborsAfterRemoval — covered by: [rails](rails.md) + redstone-components §9.2
  blocks: detector_rail
- **`PistonBaseBlock`** (2) — hooks: neighborChanged, onPlace — covered by: [pistons](../pistons.md)
  blocks: sticky_piston, piston
- **`WebBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: cobweb
- **`TallGrassBlock`** (2) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §1 (+ bonemeal growth is tick-side)
  blocks: short_grass, fern
- **`DryVegetationBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §1.3 (`supports_dry_vegetation`)
  blocks: dead_bush
- **`BushBlock`** (2) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §1
  blocks: bush, red_shrub
- **`ShortDryGrassBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §1.3
  blocks: short_dry_grass
- **`TallDryGrassBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §1.3
  blocks: tall_dry_grass
- **`SeagrassBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §4.2
  blocks: seagrass
- **`TallSeagrassBlock`** (1) — hooks: canSurvive, updateShape — covered by: [plants](plants.md) §3.2, §3.5
  blocks: tall_seagrass
- **`PistonHeadBlock`** (1) — hooks: updateShape, canSurvive, neighborChanged, affectNeighborsAfterRemoval — covered by: [multiblock](multiblock.md) §5 + [pistons](../pistons.md) §7
  blocks: piston_head
- **`StairBlock`** (97) — hooks: updateShape — covered by: [connections](connections.md) §5
  blocks: white_wool_stairs, orange_wool_stairs, magenta_wool_stairs, light_blue_wool_stairs, yellow_wool_stairs, lime_wool_stairs, pink_wool_stairs, gray_wool_stairs, light_gray_wool_stairs, cyan_wool_stairs, purple_wool_stairs, blue_wool_stairs, brown_wool_stairs, green_wool_stairs, red_wool_stairs, black_wool_stairs, oak_stairs, cobblestone_stairs, brick_stairs, stone_brick_stairs, mud_brick_stairs, resin_brick_stairs, nether_brick_stairs, sandstone_stairs, spruce_stairs, birch_stairs, jungle_stairs, quartz_stairs, acacia_stairs, cherry_stairs, dark_oak_stairs, pale_oak_stairs, mangrove_stairs, poplar_stairs, bamboo_stairs, bamboo_mosaic_stairs, prismarine_stairs, prismarine_brick_stairs, dark_prismarine_stairs, red_sandstone_stairs, purpur_stairs, white_concrete_stairs, orange_concrete_stairs, magenta_concrete_stairs, light_blue_concrete_stairs, yellow_concrete_stairs, lime_concrete_stairs, pink_concrete_stairs, gray_concrete_stairs, light_gray_concrete_stairs, cyan_concrete_stairs, purple_concrete_stairs, blue_concrete_stairs, brown_concrete_stairs, green_concrete_stairs, red_concrete_stairs, black_concrete_stairs, polished_granite_stairs, smooth_red_sandstone_stairs, mossy_stone_brick_stairs, polished_diorite_stairs, mossy_cobblestone_stairs, end_stone_brick_stairs, stone_stairs, smooth_sandstone_stairs, smooth_quartz_stairs, granite_stairs, andesite_stairs, red_nether_brick_stairs, polished_andesite_stairs, diorite_stairs, crimson_stairs, warped_stairs, blackstone_stairs, polished_blackstone_brick_stairs, polished_blackstone_stairs, tuff_stairs, polished_tuff_stairs, tuff_brick_stairs, sulfur_stairs, polished_sulfur_stairs, sulfur_brick_stairs, cinnabar_stairs, polished_cinnabar_stairs, cinnabar_brick_stairs, cut_copper_stairs, exposed_cut_copper_stairs, weathered_cut_copper_stairs, oxidized_cut_copper_stairs, waxed_cut_copper_stairs, waxed_exposed_cut_copper_stairs, waxed_weathered_cut_copper_stairs, waxed_oxidized_cut_copper_stairs, cobbled_deepslate_stairs, polished_deepslate_stairs, deepslate_tile_stairs, deepslate_brick_stairs
- **`SlabBlock`** (101) — hooks: updateShape — covered by: [connections](connections.md) §6
  blocks: white_wool_slab, orange_wool_slab, magenta_wool_slab, light_blue_wool_slab, yellow_wool_slab, lime_wool_slab, pink_wool_slab, gray_wool_slab, light_gray_wool_slab, cyan_wool_slab, purple_wool_slab, blue_wool_slab, brown_wool_slab, green_wool_slab, red_wool_slab, black_wool_slab, resin_brick_slab, prismarine_slab, prismarine_brick_slab, dark_prismarine_slab, oak_slab, spruce_slab, birch_slab, jungle_slab, acacia_slab, cherry_slab, dark_oak_slab, pale_oak_slab, mangrove_slab, poplar_slab, bamboo_slab, bamboo_mosaic_slab, stone_slab, sandstone_slab, cut_sandstone_slab, petrified_oak_slab, cobblestone_slab, brick_slab, stone_brick_slab, mud_brick_slab, nether_brick_slab, quartz_slab, red_sandstone_slab, cut_red_sandstone_slab, smooth_stone_slab, purpur_slab, white_concrete_slab, orange_concrete_slab, magenta_concrete_slab, light_blue_concrete_slab, yellow_concrete_slab, lime_concrete_slab, pink_concrete_slab, gray_concrete_slab, light_gray_concrete_slab, cyan_concrete_slab, purple_concrete_slab, blue_concrete_slab, brown_concrete_slab, green_concrete_slab, red_concrete_slab, black_concrete_slab, polished_granite_slab, smooth_red_sandstone_slab, mossy_stone_brick_slab, polished_diorite_slab, mossy_cobblestone_slab, end_stone_brick_slab, smooth_sandstone_slab, smooth_quartz_slab, granite_slab, andesite_slab, red_nether_brick_slab, polished_andesite_slab, diorite_slab, crimson_slab, warped_slab, blackstone_slab, polished_blackstone_brick_slab, polished_blackstone_slab, tuff_slab, polished_tuff_slab, tuff_brick_slab, sulfur_slab, polished_sulfur_slab, sulfur_brick_slab, cinnabar_slab, polished_cinnabar_slab, cinnabar_brick_slab, cut_copper_slab, exposed_cut_copper_slab, weathered_cut_copper_slab, oxidized_cut_copper_slab, waxed_cut_copper_slab, waxed_exposed_cut_copper_slab, waxed_weathered_cut_copper_slab, waxed_oxidized_cut_copper_slab, cobbled_deepslate_slab, polished_deepslate_slab, deepslate_tile_slab, deepslate_brick_slab
- **`MovingPistonBlock`** (1) — hooks: — — covered by: [pistons](../pistons.md) §6 (no reaction hooks)
  blocks: moving_piston
- **`FlowerBlock`** (14) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §1
  blocks: dandelion, golden_dandelion, torchflower, poppy, blue_orchid, allium, azure_bluet, red_tulip, orange_tulip, white_tulip, pink_tulip, oxeye_daisy, cornflower, lily_of_the_valley
- **`WitherRoseBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §1.3 (`supports_wither_rose`)
  blocks: wither_rose
- **`MushroomBlock`** (2) — hooks: canSurvive, updateShape — covered by: [plants](plants.md) §2.4
  blocks: brown_mushroom, red_mushroom
- **`TntBlock`** (1) — hooks: neighborChanged, onPlace — covered by: [redstone-components](../redstone-components.md) §8.9
  blocks: tnt
- **`ChiseledBookShelfBlock`** (1) — hooks: affectNeighborsAfterRemoval — covered by: [misc](misc-reactions.md) §10
  blocks: chiseled_bookshelf
- **`ShelfBlock`** (13) — hooks: updateShape, neighborChanged, onPlace, affectNeighborsAfterRemoval — covered by: [connections](connections.md) §9 + redstone-components §8.13
  blocks: acacia_shelf, bamboo_shelf, birch_shelf, cherry_shelf, crimson_shelf, dark_oak_shelf, jungle_shelf, mangrove_shelf, oak_shelf, pale_oak_shelf, poplar_shelf, spruce_shelf, warped_shelf
- **`TorchBlock`** (3) — hooks: updateShape, canSurvive — covered by: [survival](survival-and-supports.md) §1
  blocks: torch, soul_torch, copper_torch
- **`WallTorchBlock`** (3) — hooks: updateShape, canSurvive — covered by: [survival](survival-and-supports.md) §1
  blocks: wall_torch, soul_wall_torch, copper_wall_torch
- **`FireBlock`** (1) — hooks: updateShape, canSurvive, onPlace — covered by: [misc](misc-reactions.md) §8.1
  blocks: fire
- **`SoulFireBlock`** (1) — hooks: updateShape, canSurvive, onPlace — covered by: [misc](misc-reactions.md) §8.2
  blocks: soul_fire
- **`SpawnerBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: spawner
- **`CreakingHeartBlock`** (1) — hooks: updateShape, affectNeighborsAfterRemoval — covered by: [misc](misc-reactions.md) §9.6
  blocks: creaking_heart
- **`ChestBlock`** (1) — hooks: updateShape, affectNeighborsAfterRemoval — covered by: [connections](connections.md) §8
  blocks: chest
- **`RedstoneWireBlock`** (1) — hooks: updateShape, canSurvive, neighborChanged, onPlace, affectNeighborsAfterRemoval, updateIndirectNeighbourShapes — covered by: [redstone-power](../redstone-power.md) §6
  blocks: redstone_wire
- **`CraftingTableBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: crafting_table
- **`CropBlock`** (1) — hooks: canSurvive, updateShape — covered by: [plants](plants.md) §2.1
  blocks: wheat
- **`FarmlandBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §2.5
  blocks: farmland
- **`FurnaceBlock`** (1) — hooks: affectNeighborsAfterRemoval — covered by: [misc](misc-reactions.md) §10
  blocks: furnace
- **`StandingSignBlock`** (13) — hooks: updateShape, canSurvive — covered by: [survival](survival-and-supports.md) §4
  blocks: oak_sign, spruce_sign, birch_sign, acacia_sign, cherry_sign, jungle_sign, dark_oak_sign, pale_oak_sign, mangrove_sign, poplar_sign, bamboo_sign, crimson_sign, warped_sign
- **`DoorBlock`** (22) — hooks: updateShape, canSurvive, neighborChanged — covered by: [multiblock](multiblock.md) §1 + redstone-components §8.6
  blocks: oak_door, iron_door, spruce_door, birch_door, jungle_door, acacia_door, cherry_door, dark_oak_door, pale_oak_door, mangrove_door, poplar_door, bamboo_door, crimson_door, warped_door, copper_door, exposed_copper_door, weathered_copper_door, oxidized_copper_door, waxed_copper_door, waxed_exposed_copper_door, waxed_weathered_copper_door, waxed_oxidized_copper_door
- **`LadderBlock`** (1) — hooks: updateShape, canSurvive — covered by: [survival](survival-and-supports.md) §3
  blocks: ladder
- **`RailBlock`** (1) — hooks: updateShape, canSurvive, neighborChanged, onPlace, affectNeighborsAfterRemoval — covered by: [rails](rails.md)
  blocks: rail
- **`WallSignBlock`** (13) — hooks: updateShape, canSurvive — covered by: [survival](survival-and-supports.md) §4
  blocks: oak_wall_sign, spruce_wall_sign, birch_wall_sign, acacia_wall_sign, cherry_wall_sign, jungle_wall_sign, dark_oak_wall_sign, pale_oak_wall_sign, mangrove_wall_sign, poplar_wall_sign, bamboo_wall_sign, crimson_wall_sign, warped_wall_sign
- **`CeilingHangingSignBlock`** (13) — hooks: updateShape, canSurvive — covered by: [survival](survival-and-supports.md) §4
  blocks: oak_hanging_sign, spruce_hanging_sign, birch_hanging_sign, acacia_hanging_sign, cherry_hanging_sign, jungle_hanging_sign, dark_oak_hanging_sign, pale_oak_hanging_sign, crimson_hanging_sign, warped_hanging_sign, mangrove_hanging_sign, poplar_hanging_sign, bamboo_hanging_sign
- **`WallHangingSignBlock`** (13) — hooks: updateShape — covered by: [survival](survival-and-supports.md) §4 (never pops)
  blocks: oak_wall_hanging_sign, spruce_wall_hanging_sign, birch_wall_hanging_sign, acacia_wall_hanging_sign, cherry_wall_hanging_sign, jungle_wall_hanging_sign, dark_oak_wall_hanging_sign, pale_oak_wall_hanging_sign, mangrove_wall_hanging_sign, poplar_wall_hanging_sign, crimson_wall_hanging_sign, warped_wall_hanging_sign, bamboo_wall_hanging_sign
- **`LeverBlock`** (1) — hooks: affectNeighborsAfterRemoval, updateShape, canSurvive — covered by: [survival](survival-and-supports.md) §2 + [redstone-components](../redstone-components.md) §5
  blocks: lever
- **`PressurePlateBlock`** (15) — hooks: updateShape, canSurvive, affectNeighborsAfterRemoval — covered by: [survival](survival-and-supports.md) §11 + redstone-components §6.1
  blocks: stone_pressure_plate, oak_pressure_plate, spruce_pressure_plate, birch_pressure_plate, jungle_pressure_plate, acacia_pressure_plate, cherry_pressure_plate, dark_oak_pressure_plate, pale_oak_pressure_plate, mangrove_pressure_plate, poplar_pressure_plate, bamboo_pressure_plate, crimson_pressure_plate, warped_pressure_plate, polished_blackstone_pressure_plate
- **`RedStoneOreBlock`** (2) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: redstone_ore, deepslate_redstone_ore
- **`RedstoneTorchBlock`** (1) — hooks: neighborChanged, onPlace, affectNeighborsAfterRemoval, updateShape, canSurvive — covered by: [survival](survival-and-supports.md) §1 + [redstone-components](../redstone-components.md) §1
  blocks: redstone_torch
- **`RedstoneWallTorchBlock`** (1) — hooks: updateShape, canSurvive, neighborChanged, onPlace, affectNeighborsAfterRemoval — covered by: [survival](survival-and-supports.md) §1 + [redstone-components](../redstone-components.md) §1
  blocks: redstone_wall_torch
- **`ButtonBlock`** (15) — hooks: affectNeighborsAfterRemoval, updateShape, canSurvive — covered by: [survival](survival-and-supports.md) §2 + [redstone-components](../redstone-components.md) §5
  blocks: stone_button, oak_button, spruce_button, birch_button, jungle_button, acacia_button, cherry_button, dark_oak_button, pale_oak_button, mangrove_button, poplar_button, bamboo_button, crimson_button, warped_button, polished_blackstone_button
- **`SnowLayerBlock`** (1) — hooks: updateShape, canSurvive — covered by: [misc](misc-reactions.md) §1.2
  blocks: snow
- **`IceBlock`** (1) — hooks: — — covered by: [misc](misc-reactions.md) §3.1 (no reaction hooks)
  blocks: ice
- **`CactusBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §7.9
  blocks: cactus
- **`CactusFlowerBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §1.3, §7.10
  blocks: cactus_flower
- **`SugarCaneBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §7.11
  blocks: sugar_cane
- **`JukeboxBlock`** (1) — hooks: affectNeighborsAfterRemoval — covered by: [misc](misc-reactions.md) §10
  blocks: jukebox
- **`FenceBlock`** (14) — hooks: updateShape — covered by: [connections](connections.md) §1
  blocks: oak_fence, nether_brick_fence, spruce_fence, birch_fence, jungle_fence, acacia_fence, cherry_fence, dark_oak_fence, pale_oak_fence, mangrove_fence, poplar_fence, bamboo_fence, crimson_fence, warped_fence
- **`NetherrackBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: netherrack
- **`SoulSandBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: soul_sand
- **`NetherPortalBlock`** (1) — hooks: updateShape — covered by: [misc](misc-reactions.md) §8.3–8.4
  blocks: nether_portal
- **`CarvedPumpkinBlock`** (2) — hooks: onPlace — covered by: [misc](misc-reactions.md) §9.7
  blocks: carved_pumpkin, jack_o_lantern
- **`CakeBlock`** (1) — hooks: updateShape, canSurvive — covered by: [survival](survival-and-supports.md) §9
  blocks: cake
- **`RepeaterBlock`** (1) — hooks: updateShape, canSurvive, neighborChanged, onPlace, affectNeighborsAfterRemoval — covered by: [redstone-components](../redstone-components.md) §2
  blocks: repeater
- **`StainedGlassBlock`** (16) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: white_stained_glass, orange_stained_glass, magenta_stained_glass, light_blue_stained_glass, yellow_stained_glass, lime_stained_glass, pink_stained_glass, gray_stained_glass, light_gray_stained_glass, cyan_stained_glass, purple_stained_glass, blue_stained_glass, brown_stained_glass, green_stained_glass, red_stained_glass, black_stained_glass
- **`TrapDoorBlock`** (22) — hooks: updateShape, neighborChanged — covered by: [survival](survival-and-supports.md) §14 + redstone-components §8.7
  blocks: oak_trapdoor, spruce_trapdoor, birch_trapdoor, jungle_trapdoor, acacia_trapdoor, cherry_trapdoor, dark_oak_trapdoor, pale_oak_trapdoor, mangrove_trapdoor, poplar_trapdoor, bamboo_trapdoor, iron_trapdoor, crimson_trapdoor, warped_trapdoor, copper_trapdoor, exposed_copper_trapdoor, weathered_copper_trapdoor, oxidized_copper_trapdoor, waxed_copper_trapdoor, waxed_exposed_copper_trapdoor, waxed_weathered_copper_trapdoor, waxed_oxidized_copper_trapdoor
- **`InfestedBlock`** (6) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: infested_stone, infested_cobblestone, infested_stone_bricks, infested_mossy_stone_bricks, infested_cracked_stone_bricks, infested_chiseled_stone_bricks
- **`HugeMushroomBlock`** (3) — hooks: updateShape — covered by: [connections](connections.md) §7
  blocks: brown_mushroom_block, red_mushroom_block, mushroom_stem
- **`IronBarsBlock`** (10) — hooks: updateShape — covered by: [connections](connections.md) §3
  blocks: iron_bars, copper_bars, exposed_copper_bars, weathered_copper_bars, oxidized_copper_bars, waxed_copper_bars, waxed_exposed_copper_bars, waxed_weathered_copper_bars, waxed_oxidized_copper_bars, glass_pane
- **`ChainBlock`** (9) — hooks: updateShape — covered by: [survival](survival-and-supports.md) §7
  blocks: iron_chain, copper_chain, exposed_copper_chain, weathered_copper_chain, oxidized_copper_chain, waxed_copper_chain, waxed_exposed_copper_chain, waxed_weathered_copper_chain, waxed_oxidized_copper_chain
- **`PumpkinBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: pumpkin
- **`AttachedStemBlock`** (2) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §2.2
  blocks: attached_pumpkin_stem, attached_melon_stem
- **`StemBlock`** (2) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §2.2
  blocks: pumpkin_stem, melon_stem
- **`VineBlock`** (1) — hooks: updateShape, canSurvive — covered by: [connections](connections.md) §11
  blocks: vine
- **`GlowLichenBlock`** (1) — hooks: updateShape, canSurvive — covered by: [connections](connections.md) §10
  blocks: glow_lichen
- **`MultifaceBlock`** (1) — hooks: updateShape, canSurvive — covered by: [connections](connections.md) §10
  blocks: resin_clump
- **`FenceGateBlock`** (13) — hooks: updateShape, neighborChanged — covered by: [connections](connections.md) §2 + redstone-components §8.8
  blocks: oak_fence_gate, spruce_fence_gate, birch_fence_gate, jungle_fence_gate, acacia_fence_gate, cherry_fence_gate, dark_oak_fence_gate, pale_oak_fence_gate, mangrove_fence_gate, poplar_fence_gate, bamboo_fence_gate, crimson_fence_gate, warped_fence_gate
- **`MyceliumBlock`** (1) — hooks: updateShape — covered by: [misc](misc-reactions.md) §1.1
  blocks: mycelium
- **`LilyPadBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §4.1
  blocks: lily_pad
- **`WallBlock`** (32) — hooks: updateShape — covered by: [connections](connections.md) §4
  blocks: resin_brick_wall, cobblestone_wall, mossy_cobblestone_wall, brick_wall, prismarine_wall, red_sandstone_wall, mossy_stone_brick_wall, granite_wall, stone_brick_wall, mud_brick_wall, nether_brick_wall, andesite_wall, red_nether_brick_wall, sandstone_wall, end_stone_brick_wall, diorite_wall, blackstone_wall, polished_blackstone_brick_wall, polished_blackstone_wall, tuff_wall, polished_tuff_wall, tuff_brick_wall, sulfur_wall, polished_sulfur_wall, sulfur_brick_wall, cinnabar_wall, polished_cinnabar_wall, cinnabar_brick_wall, cobbled_deepslate_wall, polished_deepslate_wall, deepslate_tile_wall, deepslate_brick_wall
- **`NetherWartBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §1.3
  blocks: nether_wart
- **`EnchantingTableBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: enchanting_table
- **`BrewingStandBlock`** (1) — hooks: affectNeighborsAfterRemoval — covered by: [misc](misc-reactions.md) §10
  blocks: brewing_stand
- **`CauldronBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: cauldron
- **`LayeredCauldronBlock`** (2) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: water_cauldron, powder_snow_cauldron
- **`LavaCauldronBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: lava_cauldron
- **`EndPortalBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: end_portal
- **`EndPortalFrameBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: end_portal_frame
- **`DragonEggBlock`** (1) — hooks: updateShape, onPlace — covered by: [misc](misc-reactions.md) §4.6
  blocks: dragon_egg
- **`RedstoneLampBlock`** (1) — hooks: neighborChanged — covered by: [redstone-components](../redstone-components.md) §8.1
  blocks: redstone_lamp
- **`CocoaBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §2.6
  blocks: cocoa
- **`ShelfMushroomBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §7.16
  blocks: shelf_mushroom
- **`EnderChestBlock`** (1) — hooks: updateShape — covered by: [connections](connections.md) §8.6
  blocks: ender_chest
- **`TripWireHookBlock`** (1) — hooks: updateShape, canSurvive, affectNeighborsAfterRemoval — covered by: [connections](connections.md) §12 + redstone-components §6.7
  blocks: tripwire_hook
- **`TripWireBlock`** (1) — hooks: updateShape, onPlace, affectNeighborsAfterRemoval — covered by: [connections](connections.md) §12 + redstone-components §6.6
  blocks: tripwire
- **`CommandBlock`** (3) — hooks: neighborChanged — covered by: [redstone-components](../redstone-components.md) §8.14
  blocks: command_block, repeating_command_block, chain_command_block
- **`BeaconBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: beacon
- **`FlowerPotBlock`** (40) — hooks: updateShape — covered by: [survival](survival-and-supports.md) §13 (never pops)
  blocks: flower_pot, potted_torchflower, potted_oak_sapling, potted_spruce_sapling, potted_birch_sapling, potted_jungle_sapling, potted_acacia_sapling, potted_cherry_sapling, potted_dark_oak_sapling, potted_pale_oak_sapling, potted_poplar_sapling, potted_mangrove_propagule, potted_fern, potted_dandelion, potted_golden_dandelion, potted_poppy, potted_blue_orchid, potted_allium, potted_azure_bluet, potted_red_tulip, potted_orange_tulip, potted_white_tulip, potted_pink_tulip, potted_oxeye_daisy, potted_cornflower, potted_lily_of_the_valley, potted_wither_rose, potted_red_mushroom, potted_brown_mushroom, potted_dead_bush, potted_cactus, potted_bamboo, potted_crimson_fungus, potted_warped_fungus, potted_crimson_roots, potted_warped_roots, potted_azalea_bush, potted_flowering_azalea_bush, potted_open_eyeblossom, potted_closed_eyeblossom
- **`CarrotBlock`** (1) — hooks: canSurvive, updateShape — covered by: [plants](plants.md) §2.1
  blocks: carrots
- **`PotatoBlock`** (1) — hooks: canSurvive, updateShape — covered by: [plants](plants.md) §2.1
  blocks: potatoes
- **`SkullBlock`** (5) — hooks: neighborChanged — covered by: [survival](survival-and-supports.md) §4 (no survival) + [redstone-components](../redstone-components.md) §8.12
  blocks: skeleton_skull, zombie_head, creeper_head, dragon_head, piglin_head
- **`WallSkullBlock`** (4) — hooks: neighborChanged — covered by: [survival](survival-and-supports.md) §4 + redstone §8.12
  blocks: skeleton_wall_skull, zombie_wall_head, creeper_wall_head, dragon_wall_head
- **`WitherSkullBlock`** (1) — hooks: neighborChanged — covered by: [survival](survival-and-supports.md) §4 + redstone §8.12
  blocks: wither_skeleton_skull
- **`WitherWallSkullBlock`** (1) — hooks: neighborChanged — covered by: [survival](survival-and-supports.md) §4 + redstone §8.12
  blocks: wither_skeleton_wall_skull
- **`PlayerHeadBlock`** (1) — hooks: neighborChanged — covered by: [survival](survival-and-supports.md) §4 + redstone §8.12
  blocks: player_head
- **`PlayerWallHeadBlock`** (1) — hooks: neighborChanged — covered by: [survival](survival-and-supports.md) §4 + redstone §8.12
  blocks: player_wall_head
- **`PiglinWallSkullBlock`** (1) — hooks: neighborChanged — covered by: [survival](survival-and-supports.md) §4 + redstone §8.12
  blocks: piglin_wall_head
- **`AnvilBlock`** (3) — hooks: updateShape, onPlace — covered by: [misc](misc-reactions.md) §4
  blocks: anvil, chipped_anvil, damaged_anvil
- **`TrappedChestBlock`** (1) — hooks: updateShape, affectNeighborsAfterRemoval — covered by: [connections](connections.md) §8
  blocks: trapped_chest
- **`WeightedPressurePlateBlock`** (2) — hooks: updateShape, canSurvive, affectNeighborsAfterRemoval — covered by: [survival](survival-and-supports.md) §11 + redstone-components §6.1
  blocks: light_weighted_pressure_plate, heavy_weighted_pressure_plate
- **`ComparatorBlock`** (1) — hooks: updateShape, canSurvive, neighborChanged, onPlace, affectNeighborsAfterRemoval — covered by: [redstone-components](../redstone-components.md) §2–3
  blocks: comparator
- **`DaylightDetectorBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: daylight_detector
- **`PoweredBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: redstone_block
- **`HopperBlock`** (1) — hooks: neighborChanged, onPlace, affectNeighborsAfterRemoval — covered by: [misc](misc-reactions.md) §10 + redstone-components §8.4
  blocks: hopper
- **`DropperBlock`** (1) — hooks: neighborChanged, affectNeighborsAfterRemoval — covered by: redstone-components §8.3 + [misc](misc-reactions.md) §10
  blocks: dropper
- **`StainedGlassPaneBlock`** (16) — hooks: updateShape — covered by: [connections](connections.md) §3
  blocks: white_stained_glass_pane, orange_stained_glass_pane, magenta_stained_glass_pane, light_blue_stained_glass_pane, yellow_stained_glass_pane, lime_stained_glass_pane, pink_stained_glass_pane, gray_stained_glass_pane, light_gray_stained_glass_pane, cyan_stained_glass_pane, purple_stained_glass_pane, blue_stained_glass_pane, brown_stained_glass_pane, green_stained_glass_pane, red_stained_glass_pane, black_stained_glass_pane
- **`SlimeBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: slime_block
- **`BarrierBlock`** (1) — hooks: updateShape — covered by: [survival](survival-and-supports.md) §14
  blocks: barrier
- **`LightBlock`** (1) — hooks: updateShape — covered by: [survival](survival-and-supports.md) §14
  blocks: light
- **`HayBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: hay_block
- **`WoolCarpetBlock`** (16) — hooks: updateShape, canSurvive — covered by: [survival](survival-and-supports.md) §10
  blocks: white_carpet, orange_carpet, magenta_carpet, light_blue_carpet, yellow_carpet, lime_carpet, pink_carpet, gray_carpet, light_gray_carpet, cyan_carpet, purple_carpet, blue_carpet, brown_carpet, green_carpet, red_carpet, black_carpet
- **`TallFlowerBlock`** (4) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §3.1–3.2
  blocks: sunflower, lilac, rose_bush, peony
- **`DoublePlantBlock`** (3) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §3.1–3.2, [multiblock](multiblock.md) §3
  blocks: tall_grass, large_fern, pitcher_plant
- **`BannerBlock`** (16) — hooks: updateShape, canSurvive — covered by: [survival](survival-and-supports.md) §4
  blocks: white_banner, orange_banner, magenta_banner, light_blue_banner, yellow_banner, lime_banner, pink_banner, gray_banner, light_gray_banner, cyan_banner, purple_banner, blue_banner, brown_banner, green_banner, red_banner, black_banner
- **`WallBannerBlock`** (16) — hooks: updateShape, canSurvive — covered by: [survival](survival-and-supports.md) §4
  blocks: white_wall_banner, orange_wall_banner, magenta_wall_banner, light_blue_wall_banner, yellow_wall_banner, lime_wall_banner, pink_wall_banner, gray_wall_banner, light_gray_wall_banner, cyan_wall_banner, purple_wall_banner, blue_wall_banner, brown_wall_banner, green_wall_banner, red_wall_banner, black_wall_banner
- **`EndRodBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: end_rod
- **`ChorusPlantBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §7.14, [connections](connections.md) §13
  blocks: chorus_plant
- **`ChorusFlowerBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §7.15
  blocks: chorus_flower
- **`TorchflowerCropBlock`** (1) — hooks: canSurvive, updateShape — covered by: [plants](plants.md) §2.1
  blocks: torchflower_crop
- **`PitcherCropBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §3.3
  blocks: pitcher_crop
- **`BeetrootBlock`** (1) — hooks: canSurvive, updateShape — covered by: [plants](plants.md) §2.1
  blocks: beetroots
- **`PathBlock`** (1) — hooks: updateShape, canSurvive — covered by: [misc](misc-reactions.md) §6
  blocks: dirt_path
- **`EndGatewayBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: end_gateway
- **`FrostedIceBlock`** (1) — hooks: neighborChanged, onPlace — covered by: [misc](misc-reactions.md) §3.2
  blocks: frosted_ice
- **`MagmaBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: magma_block
- **`StructureVoidBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: structure_void
- **`ObserverBlock`** (1) — hooks: updateShape, onPlace, affectNeighborsAfterRemoval — covered by: [redstone-components](../redstone-components.md) §4
  blocks: observer
- **`ShulkerBoxBlock`** (17) — hooks: affectNeighborsAfterRemoval — covered by: [misc](misc-reactions.md) §10
  blocks: shulker_box, white_shulker_box, orange_shulker_box, magenta_shulker_box, light_blue_shulker_box, yellow_shulker_box, lime_shulker_box, pink_shulker_box, gray_shulker_box, light_gray_shulker_box, cyan_shulker_box, purple_shulker_box, blue_shulker_box, brown_shulker_box, green_shulker_box, red_shulker_box, black_shulker_box
- **`GlazedTerracottaBlock`** (16) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: white_glazed_terracotta, orange_glazed_terracotta, magenta_glazed_terracotta, light_blue_glazed_terracotta, yellow_glazed_terracotta, lime_glazed_terracotta, pink_glazed_terracotta, gray_glazed_terracotta, light_gray_glazed_terracotta, cyan_glazed_terracotta, purple_glazed_terracotta, blue_glazed_terracotta, brown_glazed_terracotta, green_glazed_terracotta, red_glazed_terracotta, black_glazed_terracotta
- **`ConcretePowderBlock`** (16) — hooks: updateShape, onPlace — covered by: [misc](misc-reactions.md) §4.4
  blocks: white_concrete_powder, orange_concrete_powder, magenta_concrete_powder, light_blue_concrete_powder, yellow_concrete_powder, lime_concrete_powder, pink_concrete_powder, gray_concrete_powder, light_gray_concrete_powder, cyan_concrete_powder, purple_concrete_powder, blue_concrete_powder, brown_concrete_powder, green_concrete_powder, red_concrete_powder, black_concrete_powder
- **`KelpBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §6
  blocks: kelp
- **`KelpPlantBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §6
  blocks: kelp_plant
- **`TurtleEggBlock`** (1) — hooks: onPlace — covered by: [plants](plants.md) §9.1
  blocks: turtle_egg
- **`SnifferEggBlock`** (1) — hooks: onPlace — covered by: [plants](plants.md) §9.2
  blocks: sniffer_egg
- **`DriedGhastBlock`** (1) — hooks: updateShape — covered by: [survival](survival-and-supports.md) §14
  blocks: dried_ghast
- **`CoralBlock`** (5) — hooks: updateShape — covered by: [plants](plants.md) §8.8
  blocks: tube_coral_block, brain_coral_block, bubble_coral_block, fire_coral_block, horn_coral_block
- **`BaseCoralPlantBlock`** (5) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §8 (dead)
  blocks: dead_tube_coral, dead_brain_coral, dead_bubble_coral, dead_fire_coral, dead_horn_coral
- **`CoralPlantBlock`** (5) — hooks: updateShape, onPlace, canSurvive — covered by: [plants](plants.md) §8 (alive)
  blocks: tube_coral, brain_coral, bubble_coral, fire_coral, horn_coral
- **`BaseCoralFanBlock`** (5) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §8 (dead)
  blocks: dead_tube_coral_fan, dead_brain_coral_fan, dead_bubble_coral_fan, dead_fire_coral_fan, dead_horn_coral_fan
- **`CoralFanBlock`** (5) — hooks: updateShape, onPlace, canSurvive — covered by: [plants](plants.md) §8 (alive)
  blocks: tube_coral_fan, brain_coral_fan, bubble_coral_fan, fire_coral_fan, horn_coral_fan
- **`BaseCoralWallFanBlock`** (5) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §8 (dead)
  blocks: dead_tube_coral_wall_fan, dead_brain_coral_wall_fan, dead_bubble_coral_wall_fan, dead_fire_coral_wall_fan, dead_horn_coral_wall_fan
- **`CoralWallFanBlock`** (5) — hooks: updateShape, onPlace, canSurvive — covered by: [plants](plants.md) §8 (alive)
  blocks: tube_coral_wall_fan, brain_coral_wall_fan, bubble_coral_wall_fan, fire_coral_wall_fan, horn_coral_wall_fan
- **`SeaPickleBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §4.3
  blocks: sea_pickle
- **`HalfTransparentBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: blue_ice
- **`ConduitBlock`** (1) — hooks: updateShape — covered by: [survival](survival-and-supports.md) §7
  blocks: conduit
- **`BambooSaplingBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §7.12
  blocks: bamboo_sapling
- **`BambooStalkBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §7.13
  blocks: bamboo
- **`BubbleColumnBlock`** (1) — hooks: updateShape, canSurvive — covered by: [misc](misc-reactions.md) §11 + [fluids](../fluids.md) §11
  blocks: bubble_column
- **`ScaffoldingBlock`** (1) — hooks: updateShape, canSurvive, onPlace — covered by: [survival](survival-and-supports.md) §12
  blocks: scaffolding
- **`LoomBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: loom
- **`BarrelBlock`** (1) — hooks: affectNeighborsAfterRemoval — covered by: [misc](misc-reactions.md) §10
  blocks: barrel
- **`SmokerBlock`** (1) — hooks: affectNeighborsAfterRemoval — covered by: [misc](misc-reactions.md) §10
  blocks: smoker
- **`BlastFurnaceBlock`** (1) — hooks: affectNeighborsAfterRemoval — covered by: [misc](misc-reactions.md) §10
  blocks: blast_furnace
- **`CartographyTableBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: cartography_table
- **`GrindstoneBlock`** (1) — hooks: canSurvive, updateShape — covered by: [survival](survival-and-supports.md) §2 (canSurvive always true)
  blocks: grindstone
- **`LecternBlock`** (1) — hooks: affectNeighborsAfterRemoval — covered by: [misc](misc-reactions.md) §10 + redstone-components §7.4
  blocks: lectern
- **`SmithingTableBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: smithing_table
- **`StonecutterBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: stonecutter
- **`BellBlock`** (1) — hooks: updateShape, canSurvive, neighborChanged — covered by: [survival](survival-and-supports.md) §6 + redstone-components §8.10
  blocks: bell
- **`LanternBlock`** (10) — hooks: updateShape, canSurvive — covered by: [survival](survival-and-supports.md) §5
  blocks: lantern, soul_lantern, copper_lantern, exposed_copper_lantern, weathered_copper_lantern, oxidized_copper_lantern, waxed_copper_lantern, waxed_exposed_copper_lantern, waxed_weathered_copper_lantern, waxed_oxidized_copper_lantern
- **`CampfireBlock`** (2) — hooks: updateShape — covered by: [misc](misc-reactions.md) §9.1
  blocks: campfire, soul_campfire
- **`SweetBerryBushBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §1
  blocks: sweet_berry_bush
- **`NyliumBlock`** (2) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: warped_nylium, crimson_nylium
- **`NetherFungusBlock`** (2) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §1.3
  blocks: warped_fungus, crimson_fungus
- **`NetherRootsBlock`** (2) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §1.3
  blocks: warped_roots, crimson_roots
- **`NetherSproutsBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §1.3
  blocks: nether_sprouts
- **`WeepingVinesBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §6
  blocks: weeping_vines
- **`WeepingVinesPlantBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §6
  blocks: weeping_vines_plant
- **`TwistingVinesBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §6
  blocks: twisting_vines
- **`TwistingVinesPlantBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §6
  blocks: twisting_vines_plant
- **`StructureBlock`** (1) — hooks: neighborChanged — covered by: [redstone-components](../redstone-components.md) §8.15
  blocks: structure_block
- **`JigsawBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: jigsaw
- **`TestBlock`** (1) — hooks: neighborChanged — covered by: [redstone-components](../redstone-components.md) §8.15
  blocks: test_block
- **`TestInstanceBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: test_instance_block
- **`ComposterBlock`** (1) — hooks: onPlace — covered by: [misc](misc-reactions.md) §9.3
  blocks: composter
- **`TargetBlock`** (1) — hooks: onPlace — covered by: [misc](misc-reactions.md) §9.4 + redstone-components §7.2
  blocks: target
- **`BeehiveBlock`** (2) — hooks: updateShape — covered by: [misc](misc-reactions.md) §9.2
  blocks: bee_nest, beehive
- **`HoneyBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: honey_block
- **`CryingObsidianBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: crying_obsidian
- **`RespawnAnchorBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: respawn_anchor
- **`CandleBlock`** (17) — hooks: updateShape, canSurvive — covered by: [survival](survival-and-supports.md) §9
  blocks: candle, white_candle, orange_candle, magenta_candle, light_blue_candle, yellow_candle, lime_candle, pink_candle, gray_candle, light_gray_candle, cyan_candle, purple_candle, blue_candle, brown_candle, green_candle, red_candle, black_candle
- **`CandleCakeBlock`** (17) — hooks: updateShape, canSurvive — covered by: [survival](survival-and-supports.md) §9
  blocks: candle_cake, white_candle_cake, orange_candle_cake, magenta_candle_cake, light_blue_candle_cake, yellow_candle_cake, lime_candle_cake, pink_candle_cake, gray_candle_cake, light_gray_candle_cake, cyan_candle_cake, purple_candle_cake, blue_candle_cake, brown_candle_cake, green_candle_cake, red_candle_cake, black_candle_cake
- **`AmethystBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: amethyst_block
- **`BuddingAmethystBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: budding_amethyst
- **`AmethystClusterBlock`** (4) — hooks: updateShape, canSurvive — covered by: [survival](survival-and-supports.md) §8
  blocks: amethyst_cluster, large_amethyst_bud, medium_amethyst_bud, small_amethyst_bud
- **`PotentSulfurBlock`** (1) — hooks: updateShape, onPlace — covered by: [misc](misc-reactions.md) §9.5
  blocks: potent_sulfur
- **`TintedGlassBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: tinted_glass
- **`PowderSnowBlock`** (1) — hooks: — — covered by: [misc](misc-reactions.md) §1.3
  blocks: powder_snow
- **`SculkSensorBlock`** (1) — hooks: updateShape, onPlace, affectNeighborsAfterRemoval — covered by: [survival](survival-and-supports.md) §14 + redstone-components §7.6
  blocks: sculk_sensor
- **`CalibratedSculkSensorBlock`** (1) — hooks: updateShape, onPlace, affectNeighborsAfterRemoval — covered by: [survival](survival-and-supports.md) §14 + redstone-components §7.6
  blocks: calibrated_sculk_sensor
- **`SculkBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: sculk
- **`SculkVeinBlock`** (1) — hooks: updateShape, canSurvive — covered by: [connections](connections.md) §10
  blocks: sculk_vein
- **`SculkCatalystBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: sculk_catalyst
- **`SculkShriekerBlock`** (1) — hooks: updateShape — covered by: [survival](survival-and-supports.md) §14
  blocks: sculk_shrieker
- **`WaterloggedTransparentBlock`** (8) — hooks: updateShape — covered by: [survival](survival-and-supports.md) §14, [connections](connections.md) §3.4
  blocks: copper_grate, exposed_copper_grate, weathered_copper_grate, oxidized_copper_grate, waxed_copper_grate, waxed_exposed_copper_grate, waxed_weathered_copper_grate, waxed_oxidized_copper_grate
- **`CopperBulbBlock`** (8) — hooks: neighborChanged, onPlace — covered by: [redstone-components](../redstone-components.md) §8.11
  blocks: copper_bulb, exposed_copper_bulb, weathered_copper_bulb, oxidized_copper_bulb, waxed_copper_bulb, waxed_exposed_copper_bulb, waxed_weathered_copper_bulb, waxed_oxidized_copper_bulb
- **`CopperChestBlock`** (8) — hooks: updateShape, shouldChangedStateKeepBlockEntity, affectNeighborsAfterRemoval — covered by: [connections](connections.md) §8.4
  blocks: copper_chest, exposed_copper_chest, weathered_copper_chest, oxidized_copper_chest, waxed_copper_chest, waxed_exposed_copper_chest, waxed_weathered_copper_chest, waxed_oxidized_copper_chest
- **`CopperGolemStatueBlock`** (8) — hooks: updateShape, affectNeighborsAfterRemoval, shouldChangedStateKeepBlockEntity — covered by: [survival](survival-and-supports.md) §14 + [misc](misc-reactions.md) §10
  blocks: copper_golem_statue, exposed_copper_golem_statue, weathered_copper_golem_statue, oxidized_copper_golem_statue, waxed_copper_golem_statue, waxed_exposed_copper_golem_statue, waxed_weathered_copper_golem_statue, waxed_oxidized_copper_golem_statue
- **`LightningRodBlock`** (8) — hooks: updateShape, onPlace, affectNeighborsAfterRemoval — covered by: [survival](survival-and-supports.md) §7 + redstone-components §7.3
  blocks: lightning_rod, exposed_lightning_rod, weathered_lightning_rod, oxidized_lightning_rod, waxed_lightning_rod, waxed_exposed_lightning_rod, waxed_weathered_lightning_rod, waxed_oxidized_lightning_rod
- **`PointedDripstoneBlock`** (1) — hooks: updateShape, canSurvive — covered by: [multiblock](multiblock.md) §4
  blocks: pointed_dripstone
- **`SulfurSpikeBlock`** (1) — hooks: updateShape, canSurvive — covered by: [multiblock](multiblock.md) §4
  blocks: sulfur_spike
- **`CaveVinesBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §6
  blocks: cave_vines
- **`CaveVinesPlantBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §6
  blocks: cave_vines_plant
- **`SporeBlossomBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §7.3
  blocks: spore_blossom
- **`AzaleaBlock`** (2) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §1.3
  blocks: azalea, flowering_azalea
- **`CarpetBlock`** (1) — hooks: updateShape, canSurvive — covered by: [survival](survival-and-supports.md) §10
  blocks: moss_carpet
- **`FlowerBedBlock`** (2) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §1, §7.8
  blocks: pink_petals, wildflowers
- **`LeafLitterBlock`** (1) — hooks: canSurvive, updateShape — covered by: [plants](plants.md) §1, §7.8
  blocks: leaf_litter
- **`BonemealableFeaturePlacerBlock`** (2) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: moss_block, pale_moss_block
- **`BigDripleafBlock`** (1) — hooks: updateShape, canSurvive, neighborChanged — covered by: [plants](plants.md) §5, §7.1
  blocks: big_dripleaf
- **`BigDripleafStemBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §5, §7.2
  blocks: big_dripleaf_stem
- **`SmallDripleafBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §3, §5
  blocks: small_dripleaf
- **`HangingRootsBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §5, §7.4
  blocks: hanging_roots
- **`RootedDirtBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: rooted_dirt
- **`MudBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: mud
- **`InfestedRotatedPillarBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: infested_deepslate
- **`FrogspawnBlock`** (1) — hooks: updateShape, canSurvive, onPlace — covered by: [plants](plants.md) §4.5
  blocks: frogspawn
- **`DecoratedPotBlock`** (1) — hooks: updateShape, affectNeighborsAfterRemoval — covered by: [survival](survival-and-supports.md) §14 + [misc](misc-reactions.md) §10
  blocks: decorated_pot
- **`CrafterBlock`** (1) — hooks: neighborChanged, affectNeighborsAfterRemoval — covered by: redstone-components §8.5 + [misc](misc-reactions.md) §10
  blocks: crafter
- **`TrialSpawnerBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: trial_spawner
- **`VaultBlock`** (1) — hooks: — — covered by: default hooks only (no world reaction of its own)
  blocks: vault
- **`HeavyCoreBlock`** (1) — hooks: updateShape — covered by: [survival](survival-and-supports.md) §14
  blocks: heavy_core
- **`MossyCarpetBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §10
  blocks: pale_moss_carpet
- **`HangingMossBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §7.5
  blocks: pale_hanging_moss
- **`EyeblossomBlock`** (2) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §1
  blocks: open_eyeblossom, closed_eyeblossom
- **`FireflyBushBlock`** (1) — hooks: updateShape, canSurvive — covered by: [plants](plants.md) §1
  blocks: firefly_bush

## Edge cases and known quirks

1. **Hook presence is inherited**: e.g. every `FlowerBlock`, `SaplingBlock`, `BushBlock` inherits the vegetation `updateShape` / `canSurvive`; the index lists the effective hooks per block class, not the declaring class.
2. **Blocks that look attachable but have no removal rule** are listed in §2 ("never pops"); these are gameplay-visible and must be reproduced.
3. **Mixed hook ownership**: powered-state writes of doors, trapdoors, gates, bells, lamps, note blocks are in `redstone-components.md`; their *shape* reactions are here. A parity test of one of these blocks needs both files.
4. **Weathering variants**: the unwaxed copper blocks use `Weathering…` subclasses (bulb, chain, chest, golem statue, grate, bars, lantern, lightning rod, door, trapdoor, slab, stair, full block) that add **no** reaction hooks to their plain parents; the index lists them under the parent class.
5. **Index completeness**: every block of `block-table.md` appears exactly once below; classes absent from the hook analysis have only defaults. If a future version adds a hook override, extend the class's row and the referenced rule.

## Constants and IDs

Index counts: 1286 blocks in 255 classes. Hook set analysed: `updateShape`, `canSurvive`, `neighborChanged`, `onPlace`, `affectNeighborsAfterRemoval`, `updateIndirectNeighbourShapes` (only redstone wire overrides it), `shouldChangedStateKeepBlockEntity` (copper chests, copper golem statues).

## Oracle probe points

- For a full cross-check, reflect `Class#getDeclaredMethods` over every class of the table above and compare the set of overridden hook names with the "hooks" field of the index row (this is the source of the index; the first row discrepancy found is a reader error).
- The reaction suite (`block-updates`) drives every block through the three scripted changes *remove support*, *replace neighbour with a different block*, *replace neighbour with the same block of another state* in all six directions with flags 3 and 2, then dumps changed blocks, scheduled ticks and RNG state.

## Open questions

1. **[oracle] Hook reflection check** (see probe points): confirm there is no block class whose effective hooks differ from the index.
2. **[oracle] Per-block `isSolid` / sturdy / center values** used by the support predicates (see `survival-and-supports.md`) come from `block-facts` and are verified there.
3. (implementer) Keep **one function per hook per class**, dispatching on the class chain; do not special-case block ids. Tags and the ignite-odds table are data read at runtime.

## Parity status

Not yet run.
