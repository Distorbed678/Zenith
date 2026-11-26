# Tick: index of every randomly-ticking and scheduled-ticking block (26.3)

- Verified by suites: `random-ticks`, `block-ticks`
- Sources: local reference reading of the 26.3 block classes and registrations (2026-11-26, reader session): every block class that overrides the random-tick, scheduled-tick or ticker hooks, the block-property registrations that request random ticks, and the block-table of this repo (`../../blocks/block-table.md`, flag `rt` and class column) cross-checked per block id.
  Everything is in our own words.
- Related specs (the other time-driven files): [`tick-growth.md`](tick-growth.md) (**G**), [`tick-spread.md`](tick-spread.md) (**S**), [`tick-decay-weathering.md`](tick-decay-weathering.md) (**D**), [`tick-fire.md`](tick-fire.md) (**F**), [`tick-gravity-and-misc.md`](tick-gravity-and-misc.md) (**M**);
  external: [`../fluids.md`](../fluids.md) (**FL**: water, lava, bubble columns, waterlogged fluid ticks), [`../redstone-components.md`](../redstone-components.md) and [`../redstone.md`](../redstone.md) (**R**), [`../pistons.md`](../pistons.md) (**P**);
  the per-block *reaction* hooks (`updateShape`, `canSurvive`, `onPlace` placement rules, `neighborChanged`) are the other reader's files in this folder; they are referenced here only by hook name.

## Summary

A block is **randomly ticking** when its state's random-tick flag is set (block-table flag `rt`, or a class rule in `state-facts.md` §2.10). It is **scheduled-ticking** when some code path calls `scheduleTick(pos, block, delay)` for it. This file assigns each such block, by registry id, to the section that specifies its behaviour. Sections are written
`G§n`, `S§n`, `D§n`, `F§n`, `M§n` (file letter + section number inside that file). Blocks that merely schedule a *fluid* tick for themselves (every waterloggable block: amethyst clusters, signs, fences, slabs, stairs, walls, trapdoors, bars, chains, lanterns, candles, sea pickles, seagrass, coral, shelves, conduits, light blocks, decorated pots, …) are in `FL` and are not listed again.

## Conventions

- "RT" = randomly ticking (property or class rule); "ST" = receives scheduled ticks of its own block type; "BE" = block-entity ticker (runs once per server tick for block entities in a block-ticking chunk; `server-tick.md` §8); "hook" = the draws / writes are specified here but the *request* for the scheduled tick comes from a reaction hook.
- Which states random-tick: state-facts §2.10 (age < max for crops and growing plants, `distance == 7 && !persistent` for leaves, `lit` for redstone ore, `stage == 0` for bamboo, copper stages before oxidized, potted eyeblossoms only, …).
- A random tick runs only if `server-tick.md` §10 selects the position and the state read at that moment is randomly ticking.

## Index

### Plants and growth (`tick-growth.md`)

| block ids | kind | section |
|---|---|---|
| `wheat`, `carrots`, `potatoes` | RT | G§2 (growth speed G§1) |
| `beetroots` | RT | G§2 |
| `torchflower_crop` | RT (ages 0 and 1; becomes `torchflower`) | G§2 |
| `pitcher_crop` | RT (lower half, age < 4) | G§3 |
| `pumpkin_stem`, `melon_stem` | RT (all ages; fruit at age 7) | G§4 |
| `attached_pumpkin_stem`, `attached_melon_stem` | none (no tick) | — (reaction file) |
| `cocoa`, `sweet_berry_bush`, `nether_wart` | RT | G§5 |
| `oak_sapling`, `spruce_sapling`, `birch_sapling`, `jungle_sapling`, `acacia_sapling`, `cherry_sapling`, `dark_oak_sapling`, `pale_oak_sapling`, `poplar_sapling` | RT | G§6, tree grower G§6.1 |
| `mangrove_propagule` | RT | G§6 |
| `azalea`, `flowering_azalea` | none (bone meal only) | — |
| `sugar_cane` | RT + ST (survival) | G§7 |
| `cactus` | RT + ST (survival); `cactus_flower`: none | G§8 |
| `bamboo`, `bamboo_sapling` | RT (`bamboo`: stage 0 only) + ST (survival, `bamboo`) | G§9 |
| `kelp`, `weeping_vines`, `twisting_vines`, `cave_vines` (heads) | RT (age < 25) + ST (survival) | G§10 |
| `kelp_plant`, `weeping_vines_plant`, `twisting_vines_plant`, `cave_vines_plant` (bodies) | ST (survival) | G§10 |
| `chorus_flower` | RT (age < 5) + ST (survival) | G§11 |
| `chorus_plant` | ST (survival) | G§11 |
| `brown_mushroom`, `red_mushroom` | RT (spread) | S§3 |
| huge mushroom blocks, `glow_lichen`, `moss_block`, `short_grass`, flowers, `dripleaf`, … | none (bone meal / hook only) | — |

### Spreading and sculk (`tick-spread.md`)

| block ids | kind | section |
|---|---|---|
| `grass_block`, `mycelium` | RT | S§1 |
| `crimson_nylium`, `warped_nylium` | RT (decay only) | S§2 |
| `vine` | RT | S§4 |
| `sculk_vein`, `sculk` | driven by the spreader (cursor behaviours); no tick of their own | S§5, S§6 |
| `sculk_catalyst` | BE (spreader) + ST (bloom reset, delay 8) | S§6, S§7 |
| `sculk_shrieker` | ST (shriek end, delay 90) | S§8 |
| `sculk_sensor`, `calibrated_sculk_sensor` | ST (phases) + BE (vibration) | R: `redstone-components.md` §7.6 |

### Decay, melting, farmland, coral, copper (`tick-decay-weathering.md`)

| block ids | kind | section |
|---|---|---|
| all 14 leaf blocks: `oak_leaves`, `spruce_leaves`, `birch_leaves`, `jungle_leaves`, `acacia_leaves`, `cherry_leaves`, `dark_oak_leaves`, `pale_oak_leaves`, `mangrove_leaves`, `red_poplar_leaves`, `orange_poplar_leaves`, `yellow_poplar_leaves`, `azalea_leaves`, `flowering_azalea_leaves` | RT (decay) + ST (distance update, delay 1) | D§1 |
| `ice` | RT | D§2 |
| `frosted_ice` | ST (+ `onPlace` hidden schedules) | D§2 |
| `snow` (layers) | RT | D§2; formation D§6 |
| `farmland` | RT + ST (delay 1) | D§3 |
| `tube_coral_block`, `brain_coral_block`, `bubble_coral_block`, `fire_coral_block`, `horn_coral_block` | ST (die, delay 60..99) | D§4 |
| live coral plants, fans, wall fans of the same five types (`tube_coral`, `*_coral_fan`, `*_coral_wall_fan`) | ST (die) | D§4 |
| dead corals (blocks, plants, fans, wall fans) | none | — |
| 60 unwaxed copper blocks: `copper_block`, `exposed_copper`, `weathered_copper`, `oxidized_copper`; `cut_copper`, `chiseled_copper`, `cut_copper_stairs`, `cut_copper_slab`, `copper_door`, `copper_trapdoor`, `copper_bars`, `copper_grate`, `copper_bulb`, `copper_lantern`, `copper_chest`, `copper_golem_statue`, `lightning_rod`, `copper_chain` — each with the `exposed_` / `weathered_` / `oxidized_` prefix — | RT for the first three stages (not `oxidized_*`) | D§5 |
| all `waxed_*` copper blocks | none (the chests, lightning rods, doors etc. keep their other behaviours: R / BE) | — |
| `cauldron`, `water_cauldron`, `powder_snow_cauldron`, `lava_cauldron` | ST (dripstone fill) + precipitation hook | D§7, D§6 |
| `composter` | ST (delay 20) | D§8 |
| precipitation / ice / snow formation (level tick, not a block) | level roll | D§6, `server-tick.md` §10–§11 |

### Fire (`tick-fire.md`)

| block ids | kind | section |
|---|---|---|
| `fire` | ST (30..39) | F§1–F§3 |
| `soul_fire` | none | F§3 |

### Falling blocks, supports, drips, eggs, misc (`tick-gravity-and-misc.md`)

| block ids | kind | section |
|---|---|---|
| `sand`, `red_sand`, `gravel`, the 16 `*_concrete_powder`, `anvil`, `chipped_anvil`, `damaged_anvil` | ST (delay 2) | M§1 |
| `dragon_egg` | ST (delay 5) | M§1 |
| `suspicious_sand`, `suspicious_gravel` | ST (2) + BE data | M§1 |
| `scaffolding` | ST (1) | M§1 |
| `dirt_path` | ST (1) | M§2 |
| `pale_hanging_moss` | ST (1) | M§2 |
| `big_dripleaf` | ST (tilt: 10 / 10 / 100) | M§3 |
| `big_dripleaf_stem` | ST (1) | M§2 |
| `dried_ghast` | RT + ST (5000) | M§4 |
| `pointed_dripstone`, `sulfur_spike` | RT + ST (fall / support) | M§5 |
| `turtle_egg` | RT | M§6 |
| `sniffer_egg` | ST (hatch stages) | M§6 |
| `frogspawn` | ST (3600..11999) | M§6 |
| `budding_amethyst` | RT | M§7 |
| `open_eyeblossom`, `closed_eyeblossom` | RT + ST (switch) | M§8 |
| `potted_open_eyeblossom`, `potted_closed_eyeblossom` | RT | M§8 |
| other flower pots (36), `firefly_bush` | none (the firefly bush has no random tick) | — |
| `redstone_ore`, `deepslate_redstone_ore` | RT (lit only) | M§9 |
| `nether_portal` | RT (piglins) | M§9 |
| `command_block`, `repeating_command_block`, `chain_command_block` | ST (1) | M§9 |
| `campfire`, `soul_campfire` | BE (cook / cool-down) | M§9 |
| `creaking_heart` | ST (1) + BE | M§10 |
| `potent_sulfur` | BE (geyser) | M§11 |
| lightning strike block effects (entity) | entity | M§12 |
| `test_block` | ST (reset; game-test framework only) | — (technical block, not gameplay) |

### Specified elsewhere

| block ids | where |
|---|---|
| `water`, `lava` (lava is RT: fluid random tick), `bubble_column`; all fluid ticks of waterlogged blocks; `magma_block` / `soul_sand` bubble-column feeding | [`../fluids.md`](../fluids.md) |
| `redstone_torch`, `redstone_wall_torch`, `repeater`, `comparator`, `observer`, `target`, `redstone_lamp`, `lectern`, `lightning_rod` (8, power pulse 8), the 15 buttons, 15 pressure plates, 2 weighted plates, `tripwire`, `tripwire_hook`, `detector_rail`, `dispenser`, `dropper`, `crafter`, chest-like openers (`chest`, `trapped_chest`, `ender_chest`, `barrel`, 8 copper chests), sculk sensors | [`../redstone-components.md`](../redstone-components.md), [`../redstone.md`](../redstone.md) |
| `piston`, `sticky_piston`, `moving_piston` (BE), piston heads | [`../pistons.md`](../pistons.md) |
| other block-entity tickers (furnaces, hoppers, spawners, beacons, bells, banners, signs, skulls, brewing stands, jukebox, conduit, enchanting table, end gateway, vault, trial spawner, daylight detector, shulker boxes, beehives): not "random / scheduled tick" blocks; belong to the block-entity milestone | — |

## Behavior (cross-cutting rules every section relies on)

1. **Random-tick selection** and the draw streams: `../server-tick.md` §10. A block's random tick receives the **level random** (the same object that the scheduled-tick code receives).
2. **Scheduled-tick draining order, delay semantics, one tick per `(block, pos)`:** `../scheduled-ticks-and-block-events.md` §1–§3. A tick that fires finds its block at the position or is dropped.
3. **`onPlace` runs for every state-changing write** (flag 512 clear), not only for placements of a different block (`../set-block-flags-and-sequence.md` §3.10, step 2: identical states return early). Consequence for time-driven blocks: *any* write that changes a property of fire, frosted ice, sniffer eggs, scaffolding, brushable blocks, composters, coral plants and fans, frogspawn,
   turtle eggs, … re-runs its `onPlace` side effects (schedule requests *with their random draws*; a request for an already pending `(block, pos)` is discarded but its delay draw has been made). Each section lists those draws where they exist.
4. **Draws:** the order inside one hook is fixed by the order of evaluation in the section; short-circuit evaluation hides draws behind failed tests; cosmetic sounds and particles that use the *level* random count (pitches, eyeblossom trail particles); client-only effects do not.
5. **Light reads** are of the visible light layers (`../server-tick.md` §13 and Open question 4 there).
6. **Flags** of writes are given per block (2 = clients only, 3 = full update, 260 = no neighbours / no block-entity side effects, 818 = skip shape updates and side effects for sapling removal).

## Edge cases and known quirks

1. `firefly_bush` is **not** randomly ticking in 26.3 (it is a plain vegetation block with client particles); do not tick it. Every block flagged `rt` has a random-tick behaviour in the sections above.
2. `lava` is the only fluid that random-ticks, via the fluid hook after the block hook (`server-tick.md` §10.2d).
3. Block types that appear in two sections (cactus, sugar cane, bamboo, chorus, kelp, vines, coral, leaves, farmland, frosted ice, dried ghast, eyeblossoms, pointed dripstone) are specified in one file; the cross-reference is the table above.
4. **Torchflower crop** is RT in both of its states, **potted eyeblossoms** in both, **bamboo** only while `stage = 0`, **leaves** only at distance 7 (non-persistent), **crops** only below max age, **copper** only before the last stage.

## Constants and IDs

Counts: **67** block ids carry the property flag `rt` in `block-table.md`: grass block, 9 saplings, mangrove propagule, lava, **14** leaf blocks, 2 mushrooms, wheat, farmland, 2 redstone ores, snow, ice, cactus, sugar cane, nether portal, 2 stems, vine, mycelium, nether wart, cocoa, carrots, potatoes, chorus flower, torchflower crop, pitcher crop, beetroots, kelp,
turtle egg, dried ghast, bamboo sapling, bamboo, sweet berry bush, 2 nylium, weeping and twisting vines, budding amethyst, pointed dripstone, sulfur spike, cave vines, 2 eyeblossoms, 2 potted eyeblossoms. The 60 unwaxed copper ids are randomly ticking by a **class rule** (state-facts §2.10): 45 of them (the three non-oxidized stages of 15 families); the property flag is not set for them.
Delays (scheduled-tick blocks): leaves / farmland / path / moss / stem plants / support checks 1; falling blocks 2 (dragon egg 5); scaffolding 1; fire 30..39; coral 60..99; frosted ice 60..120 then 20..40; frogspawn 3600..11999; sniffer egg `hatchTime/3 + nextInt(300)`; dried ghast 5000; big dripleaf 10 / 10 / 100; composter 20; sculk catalyst 8; shrieker 90; dripstone→cauldron `50 + fall`.

## Oracle probe points

- Enumerate randomly-ticking states: `BlockBehaviour$BlockStateBase#isRandomlyTicking()` over `Block.BLOCK_STATE_REGISTRY` (public static); compare with this index and `state-facts.md` §2.10.
- Enumerate blocks that override scheduling: `Class#getDeclaredMethod("tick", BlockState.class, ServerLevel.class, BlockPos.class, RandomSource.class)` per registered block class; diff with the "ST" rows.
- Record every `ServerLevel#scheduleTick(BlockPos, Block, int)` call with `(block, pos, delay, accepted)` — the `block-ticks` dump format.

## Open questions

1. **[oracle] Exhaustive RT / ST set.** Run the two enumerations above and list differences with this index (blocks added by data packs are out of scope).
2. (implementer) Leaves, farmland, coral, frosted ice, fire, scaffolding and the falling blocks are the blocks whose schedule requests can come from several hooks (`onPlace`, `updateShape`, `neighborChanged`, block entities); implement the request sites from the reaction files and the draws from the time-driven files exactly once each.

## Parity status

Not yet run.
