//! Fast O(1) block-state operations, tag lookups, property manipulation, `canSurvive`,
//! `MultifaceBlock` / `MultifaceSpreader`, `DoublePlantBlock`, `MossyCarpetBlock`, and
//! `ChorusFlowerBlock` helpers (`docs/specs/worldgen/features.md`).

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use zenith_blocks::{BlockFacts, Direction, FluidKind, SupportType, flag};
use zenith_math::RandomSource;
use zenith_registry::{BlockStateTable, Identifier};
use zenith_worldgen::{BlockPredicates, BlockTags};

use crate::region::GenerationRegion;
use crate::rng::{BlockPos, direction_all_shuffled, direction_horizontal_random};

/// Dense bitset over `block_protocol_id` for O(1) tag and holder-set membership checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockBitSet {
    words: Vec<u64>,
}

impl BlockBitSet {
    /// Creates an empty bitset sized for `block_count` blocks.
    pub fn new(block_count: usize) -> Self {
        Self {
            words: vec![0u64; block_count.div_ceil(64).max(1)],
        }
    }

    /// Inserts a block protocol ID.
    #[inline]
    pub fn insert(&mut self, block_id: u32) {
        let idx = block_id as usize;
        let word = idx >> 6;
        if word >= self.words.len() {
            self.words.resize(word + 1, 0);
        }
        self.words[word] |= 1u64 << (idx & 63);
    }

    /// Tests whether `block_id` is present.
    #[inline]
    pub fn contains(&self, block_id: u32) -> bool {
        let idx = block_id as usize;
        self.words
            .get(idx >> 6)
            .is_some_and(|w| (w & (1u64 << (idx & 63))) != 0)
    }
}

/// Precomputed block-state metadata, fast property lookup/mutation, and common block/tag IDs.
#[derive(Debug)]
pub struct BlockOps {
    /// Global block-state table.
    pub blocks: Arc<BlockStateTable>,
    /// Per-state static facts and voxel shapes (bit-exact vs 26.3).
    pub facts: Arc<BlockFacts>,
    /// Resolved block tags.
    pub tags: Arc<BlockTags>,
    /// Heightmap / air / fluid predicates.
    pub predicates: Arc<BlockPredicates>,
    /// Precomputed bitsets for every loaded block tag.
    tag_bitsets: HashMap<Identifier, BlockBitSet>,
    /// Precomputed strides for every block's properties (for O(1) property mutation).
    prop_strides: Vec<Vec<u32>>,
    /// Lazily resolved tree constants (`features/tree`).
    tree_cache: std::sync::OnceLock<crate::features::tree::TreeCache>,

    // Common block protocol IDs (u32::MAX if absent in synthetic tests).
    pub bid_air: u32,
    pub bid_cave_air: u32,
    pub bid_water: u32,
    pub bid_lava: u32,
    pub bid_bedrock: u32,
    pub bid_stone: u32,
    pub bid_cobblestone: u32,
    pub bid_mossy_cobblestone: u32,
    pub bid_obsidian: u32,
    pub bid_end_stone: u32,
    pub bid_end_portal: u32,
    pub bid_end_gateway: u32,
    pub bid_spawner: u32,
    pub bid_chest: u32,
    pub bid_torch: u32,
    pub bid_wall_torch: u32,
    pub bid_fire: u32,
    pub bid_iron_bars: u32,
    pub bid_ice: u32,
    pub bid_packed_ice: u32,
    pub bid_blue_ice: u32,
    pub bid_snow: u32,
    pub bid_snow_block: u32,
    pub bid_podzol: u32,
    pub bid_dirt_path: u32,
    pub bid_bamboo: u32,
    pub bid_vine: u32,
    pub bid_magma_block: u32,
    pub bid_dripstone_block: u32,
    pub bid_pointed_dripstone: u32,
    pub bid_sculk: u32,
    pub bid_sculk_vein: u32,
    pub bid_sculk_catalyst: u32,
    pub bid_sculk_sensor: u32,
    pub bid_sculk_shrieker: u32,
    pub bid_moving_piston: u32,
    pub bid_chorus_plant: u32,
    pub bid_chorus_flower: u32,
    pub bid_nether_wart_block: u32,
    pub bid_weeping_vines: u32,
    pub bid_weeping_vines_plant: u32,
    pub bid_pale_moss_carpet: u32,
    pub bid_glow_lichen: u32,
    pub bid_resin_clump: u32,
    pub bid_barrier: u32,
    pub bid_grass_block: u32,
    pub bid_mycelium: u32,
    pub bid_dirt: u32,
    pub bid_sand: u32,
    pub bid_red_sand: u32,
    pub bid_soul_sand: u32,
    pub bid_soul_soil: u32,
    pub bid_cactus: u32,
    pub bid_sugar_cane: u32,
    pub bid_farmland: u32,
    pub bid_kelp: u32,
    pub bid_kelp_plant: u32,
    pub bid_twisting_vines: u32,
    pub bid_twisting_vines_plant: u32,
    pub bid_cave_vines: u32,
    pub bid_cave_vines_plant: u32,
    pub bid_big_dripleaf: u32,
    pub bid_big_dripleaf_stem: u32,
    pub bid_pale_hanging_moss: u32,
    pub bid_nether_bricks: u32,
    pub bid_nether_brick_fence: u32,
    pub bid_nether_brick_stairs: u32,
    pub bid_nether_wart: u32,

    // Common state IDs.
    pub sid_air: u32,
    pub sid_cave_air: u32,
    pub sid_water: u32,
    pub sid_lava: u32,
    pub sid_bedrock: u32,
    pub sid_stone: u32,
    pub sid_cobblestone: u32,
    pub sid_mossy_cobblestone: u32,
    pub sid_obsidian: u32,
    pub sid_end_stone: u32,
    pub sid_end_portal: u32,
    pub sid_end_gateway: u32,
    pub sid_spawner: u32,
    pub sid_chest: u32,
    pub sid_torch: u32,
    pub sid_fire: u32,
    pub sid_ice: u32,
    pub sid_blue_ice: u32,
    pub sid_snow_1: u32,
    pub sid_snow_8: u32,
    pub sid_snow_block: u32,
    pub sid_podzol: u32,
    pub sid_bamboo_trunk: u32,
    pub sid_bamboo_top_small: u32,
    pub sid_bamboo_top_large: u32,
    pub sid_bamboo_final_large: u32,
    pub sid_magma_block: u32,
    pub sid_dripstone_block: u32,
    pub sid_sculk: u32,
    pub sid_sculk_vein: u32,
    pub sid_sculk_sensor: u32,
    pub sid_sculk_shrieker_summon: u32,
    pub sid_chorus_plant: u32,
    pub sid_chorus_flower_5: u32,
    pub sid_weeping_vines_plant: u32,
    pub sid_pale_moss_carpet: u32,
    pub sid_barrier: u32,

    // Precomputed tag bitsets for hot feature checks.
    pub tag_base_stone_overworld: BlockBitSet,
    pub tag_beneath_bamboo_podzol_replaceable: BlockBitSet,
    pub tag_features_cannot_replace: BlockBitSet,
    pub tag_leaves: BlockBitSet,
    pub tag_replaceable_by_mushrooms: BlockBitSet,
    pub tag_replaceable_by_trees: BlockBitSet,
    pub tag_sculk_growth_inhibitors: BlockBitSet,
    pub tag_sculk_replaceable: BlockBitSet,
    pub tag_sculk_replaceable_world_gen: BlockBitSet,
    pub tag_snow_layer_can_survive_on: BlockBitSet,
    pub tag_snow_layer_cannot_survive_on: BlockBitSet,
    pub tag_supports_bamboo: BlockBitSet,
    pub tag_supports_chorus_plant: BlockBitSet,
    pub tag_fire: BlockBitSet,
    pub tag_dirt: BlockBitSet,
    pub tag_sand: BlockBitSet,
    pub tag_dry_vegetation_may_place_on: BlockBitSet,
    pub tag_nylium: BlockBitSet,
    pub tag_mushroom_grow_block: BlockBitSet,
    pub tag_small_dripleaf_placeable: BlockBitSet,
    pub tag_big_dripleaf_placeable: BlockBitSet,
    pub tag_bamboo_plantable_on: BlockBitSet,
    pub tag_dead_bush_may_place_on: BlockBitSet,
    pub tag_supports_vegetation: BlockBitSet,
    pub tag_supports_dry_vegetation: BlockBitSet,
    pub tag_supports_nether_sprouts: BlockBitSet,
    pub tag_supports_nether_fungus: BlockBitSet,
    pub tag_supports_nether_roots: BlockBitSet,
    pub tag_supports_crimson_roots: BlockBitSet,
    pub tag_supports_warped_roots: BlockBitSet,
    pub tag_supports_sugar_cane: BlockBitSet,
    pub tag_supports_cactus: BlockBitSet,
    pub tag_supports_azalea: BlockBitSet,
    pub tag_supports_hanging_moss: BlockBitSet,
    pub tag_supports_small_dripleaf: BlockBitSet,
    pub tag_supports_big_dripleaf: BlockBitSet,
    pub tag_supports_lily_pad: BlockBitSet,
    pub tag_supports_nether_wart: BlockBitSet,
    pub tag_supports_cocoa: BlockBitSet,
    pub tag_supports_melon_stem: BlockBitSet,
    pub tag_supports_pumpkin_stem: BlockBitSet,
    pub tag_supports_crops: BlockBitSet,
    pub tag_supports_wither_rose: BlockBitSet,
    pub tag_supports_mangrove_propagule: BlockBitSet,
    pub tag_supports_frogspawn: BlockBitSet,
    pub tag_supports_hanging_roots: BlockBitSet,
    pub tag_supports_pale_hanging_moss: BlockBitSet,
    pub tag_supports_wildflowers: BlockBitSet,
    pub tag_supports_flower_bed: BlockBitSet,
    pub tag_supports_leaf_litter: BlockBitSet,
    pub tag_supports_bush: BlockBitSet,
    pub tag_supports_firefly_bush: BlockBitSet,
    pub tag_supports_cactus_flower: BlockBitSet,
    pub tag_cannot_support_seagrass: BlockBitSet,
    pub tag_cannot_support_kelp: BlockBitSet,
    pub tag_cannot_support_snow_layer: BlockBitSet,
    pub tag_supports_snow_layer: BlockBitSet,
    /// DeltaFeature hardcoded CANNOT_REPLACE set.
    pub delta_cannot_replace: BlockBitSet,
}

impl BlockOps {
    /// Builds [`BlockOps`] from `blocks`, `facts`, `tags`, and `predicates`.
    pub fn new(
        blocks: Arc<BlockStateTable>,
        facts: Arc<BlockFacts>,
        tags: Arc<BlockTags>,
        predicates: Arc<BlockPredicates>,
    ) -> Self {
        let block_count = blocks.blocks().len();

        let mut prop_strides = Vec::with_capacity(block_count);
        for entry in blocks.blocks() {
            let n = entry.properties.len();
            let mut strides = vec![1u32; n];
            let mut s = 1u32;
            for i in (0..n).rev() {
                strides[i] = s;
                s *= entry.properties[i].values().len() as u32;
            }
            prop_strides.push(strides);
        }

        let tag_bitsets = HashMap::new();
        let make_tag = |name: &str| -> BlockBitSet {
            let mut bs = BlockBitSet::new(block_count);
            if let Ok(id) = Identifier::with_default_namespace(name)
                && let Some(ids) = tags.block_ids(&id)
            {
                for &bid in ids {
                    bs.insert(bid);
                }
            }
            bs
        };

        let bid = |name: &str| -> u32 {
            Identifier::with_default_namespace(name)
                .ok()
                .and_then(|id| blocks.block(&id))
                .map_or(u32::MAX, |e| e.block_protocol_id)
        };
        let sid = |name: &str| -> u32 {
            Identifier::with_default_namespace(name)
                .ok()
                .and_then(|id| blocks.block(&id))
                .map_or(0, |e| e.default_state)
        };
        let sid_props = |name: &str, props: &[(&str, &str)]| -> u32 {
            let Ok(id) = Identifier::with_default_namespace(name) else {
                return 0;
            };
            let Some(entry) = blocks.block(&id) else {
                return 0;
            };
            let Some(def) = blocks.state_by_id_exact(entry.default_state as i32) else {
                return entry.default_state;
            };
            let mut offset = 0u32;
            for prop in &entry.properties {
                let wanted = props
                    .iter()
                    .find(|(k, _)| *k == prop.name())
                    .map(|(_, v)| *v)
                    .or_else(|| def.property_value(prop.name()))
                    .unwrap_or("");
                let idx = prop.values().iter().position(|v| v == wanted).unwrap_or(0) as u32;
                offset = offset * (prop.values().len() as u32) + idx;
            }
            entry.first_state + offset
        };

        let mut delta_cannot_replace = BlockBitSet::new(block_count);
        for name in [
            "bedrock",
            "nether_bricks",
            "nether_brick_fence",
            "nether_brick_stairs",
            "nether_wart",
            "chest",
            "spawner",
        ] {
            let b = bid(name);
            if b != u32::MAX {
                delta_cannot_replace.insert(b);
            }
        }

        let bid_air = bid("air");
        let bid_cave_air = bid("cave_air");
        let bid_water = bid("water");
        let bid_lava = bid("lava");
        let bid_bedrock = bid("bedrock");
        let bid_stone = bid("stone");
        let bid_cobblestone = bid("cobblestone");
        let bid_mossy_cobblestone = bid("mossy_cobblestone");
        let bid_obsidian = bid("obsidian");
        let bid_end_stone = bid("end_stone");
        let bid_end_portal = bid("end_portal");
        let bid_end_gateway = bid("end_gateway");
        let bid_spawner = bid("spawner");
        let bid_chest = bid("chest");
        let bid_torch = bid("torch");
        let bid_wall_torch = bid("wall_torch");
        let bid_fire = bid("fire");
        let bid_iron_bars = bid("iron_bars");
        let bid_ice = bid("ice");
        let bid_packed_ice = bid("packed_ice");
        let bid_blue_ice = bid("blue_ice");
        let bid_snow = bid("snow");
        let bid_snow_block = bid("snow_block");
        let bid_podzol = bid("podzol");
        let bid_dirt_path = bid("dirt_path");
        let bid_bamboo = bid("bamboo");
        let bid_vine = bid("vine");
        let bid_magma_block = bid("magma_block");
        let bid_dripstone_block = bid("dripstone_block");
        let bid_pointed_dripstone = bid("pointed_dripstone");
        let bid_sculk = bid("sculk");
        let bid_sculk_vein = bid("sculk_vein");
        let bid_sculk_catalyst = bid("sculk_catalyst");
        let bid_sculk_sensor = bid("sculk_sensor");
        let bid_sculk_shrieker = bid("sculk_shrieker");
        let bid_moving_piston = bid("moving_piston");
        let bid_chorus_plant = bid("chorus_plant");
        let bid_chorus_flower = bid("chorus_flower");
        let bid_nether_wart_block = bid("nether_wart_block");
        let bid_weeping_vines = bid("weeping_vines");
        let bid_weeping_vines_plant = bid("weeping_vines_plant");
        let bid_pale_moss_carpet = bid("pale_moss_carpet");
        let bid_glow_lichen = bid("glow_lichen");
        let bid_resin_clump = bid("resin_clump");
        let bid_barrier = bid("barrier");
        let bid_grass_block = bid("grass_block");
        let bid_mycelium = bid("mycelium");
        let bid_dirt = bid("dirt");
        let bid_sand = bid("sand");
        let bid_red_sand = bid("red_sand");
        let bid_soul_sand = bid("soul_sand");
        let bid_soul_soil = bid("soul_soil");
        let bid_cactus = bid("cactus");
        let bid_sugar_cane = bid("sugar_cane");
        let bid_farmland = bid("farmland");
        let bid_kelp = bid("kelp");
        let bid_kelp_plant = bid("kelp_plant");
        let bid_twisting_vines = bid("twisting_vines");
        let bid_twisting_vines_plant = bid("twisting_vines_plant");
        let bid_cave_vines = bid("cave_vines");
        let bid_cave_vines_plant = bid("cave_vines_plant");
        let bid_big_dripleaf = bid("big_dripleaf");
        let bid_big_dripleaf_stem = bid("big_dripleaf_stem");
        let bid_pale_hanging_moss = bid("pale_hanging_moss");
        let bid_nether_bricks = bid("nether_bricks");
        let bid_nether_brick_fence = bid("nether_brick_fence");
        let bid_nether_brick_stairs = bid("nether_brick_stairs");
        let bid_nether_wart = bid("nether_wart");

        let sid_air = sid("air");
        let sid_cave_air = sid("cave_air");
        let sid_water = sid("water");
        let sid_lava = sid("lava");
        let sid_bedrock = sid("bedrock");
        let sid_stone = sid("stone");
        let sid_cobblestone = sid("cobblestone");
        let sid_mossy_cobblestone = sid("mossy_cobblestone");
        let sid_obsidian = sid("obsidian");
        let sid_end_stone = sid("end_stone");
        let sid_end_portal = sid("end_portal");
        let sid_end_gateway = sid("end_gateway");
        let sid_spawner = sid("spawner");
        let sid_chest = sid("chest");
        let sid_torch = sid("torch");
        let sid_fire = sid("fire");
        let sid_ice = sid("ice");
        let sid_blue_ice = sid("blue_ice");
        let sid_snow_1 = sid_props("snow", &[("layers", "1")]);
        let sid_snow_8 = sid_props("snow", &[("layers", "8")]);
        let sid_snow_block = sid("snow_block");
        let sid_podzol = sid("podzol");
        let sid_bamboo_trunk = sid_props(
            "bamboo",
            &[("age", "1"), ("leaves", "none"), ("stage", "0")],
        );
        let sid_bamboo_top_small = sid_props(
            "bamboo",
            &[("age", "1"), ("leaves", "small"), ("stage", "0")],
        );
        let sid_bamboo_top_large = sid_props(
            "bamboo",
            &[("age", "1"), ("leaves", "large"), ("stage", "0")],
        );
        let sid_bamboo_final_large = sid_props(
            "bamboo",
            &[("age", "1"), ("leaves", "large"), ("stage", "1")],
        );
        let sid_magma_block = sid("magma_block");
        let sid_dripstone_block = sid("dripstone_block");
        let sid_sculk = sid("sculk");
        let sid_sculk_vein = sid("sculk_vein");
        let sid_sculk_sensor = sid("sculk_sensor");
        let sid_sculk_shrieker_summon = sid_props("sculk_shrieker", &[("can_summon", "true")]);
        let sid_chorus_plant = sid("chorus_plant");
        let sid_chorus_flower_5 = sid_props("chorus_flower", &[("age", "5")]);
        let sid_weeping_vines_plant = sid("weeping_vines_plant");
        let sid_pale_moss_carpet = sid("pale_moss_carpet");
        let sid_barrier = sid("barrier");

        let tag_base_stone_overworld = make_tag("base_stone_overworld");
        let tag_beneath_bamboo_podzol_replaceable = make_tag("beneath_bamboo_podzol_replaceable");
        let tag_features_cannot_replace = make_tag("features_cannot_replace");
        let tag_leaves = make_tag("leaves");
        let tag_replaceable_by_mushrooms = make_tag("replaceable_by_mushrooms");
        let tag_replaceable_by_trees = make_tag("replaceable_by_trees");
        let tag_sculk_growth_inhibitors = make_tag("sculk_growth_inhibitors");
        let tag_sculk_replaceable = make_tag("sculk_replaceable");
        let tag_sculk_replaceable_world_gen = make_tag("sculk_replaceable_world_gen");
        let tag_snow_layer_can_survive_on = make_tag("snow_layer_can_survive_on");
        let tag_snow_layer_cannot_survive_on = make_tag("snow_layer_cannot_survive_on");
        let tag_supports_bamboo = make_tag("supports_bamboo");
        let tag_supports_chorus_plant = make_tag("supports_chorus_plant");
        let tag_fire = make_tag("fire");
        let tag_dirt = make_tag("dirt");
        let tag_sand = make_tag("sand");
        let tag_dry_vegetation_may_place_on = make_tag("dry_vegetation_may_place_on");
        let tag_nylium = make_tag("nylium");
        let tag_mushroom_grow_block = make_tag("overrides_mushroom_light_requirement");
        let tag_small_dripleaf_placeable = make_tag("small_dripleaf_placeable");
        let tag_big_dripleaf_placeable = make_tag("big_dripleaf_placeable");
        let tag_bamboo_plantable_on = make_tag("bamboo_plantable_on");
        let tag_dead_bush_may_place_on = make_tag("dead_bush_may_place_on");
        let tag_supports_vegetation = make_tag("supports_vegetation");
        let tag_supports_dry_vegetation = make_tag("supports_dry_vegetation");
        let tag_supports_nether_sprouts = make_tag("supports_nether_sprouts");
        let tag_supports_nether_fungus = make_tag("supports_nether_fungus");
        let tag_supports_nether_roots = make_tag("supports_nether_roots");
        let tag_supports_crimson_roots = make_tag("supports_crimson_roots");
        let tag_supports_warped_roots = make_tag("supports_warped_roots");
        let tag_supports_sugar_cane = make_tag("supports_sugar_cane");
        let tag_supports_cactus = make_tag("supports_cactus");
        let tag_supports_azalea = make_tag("supports_azalea");
        let tag_supports_hanging_moss = make_tag("supports_hanging_moss");
        let tag_supports_small_dripleaf = make_tag("supports_small_dripleaf");
        let tag_supports_big_dripleaf = make_tag("supports_big_dripleaf");
        let tag_supports_lily_pad = make_tag("supports_lily_pad");
        let tag_supports_nether_wart = make_tag("supports_nether_wart");
        let tag_supports_cocoa = make_tag("supports_cocoa");
        let tag_supports_melon_stem = make_tag("supports_melon_stem");
        let tag_supports_pumpkin_stem = make_tag("supports_pumpkin_stem");
        let tag_supports_crops = make_tag("supports_crops");
        let tag_supports_wither_rose = make_tag("supports_wither_rose");
        let tag_supports_mangrove_propagule = make_tag("supports_mangrove_propagule");
        let tag_supports_frogspawn = make_tag("supports_frogspawn");
        let tag_supports_hanging_roots = make_tag("supports_hanging_roots");
        let tag_supports_pale_hanging_moss = make_tag("supports_pale_hanging_moss");
        let tag_supports_wildflowers = make_tag("supports_wildflowers");
        let tag_supports_flower_bed = make_tag("supports_flower_bed");
        let tag_supports_leaf_litter = make_tag("supports_leaf_litter");
        let tag_supports_bush = make_tag("supports_bush");
        let tag_supports_firefly_bush = make_tag("supports_firefly_bush");
        let tag_supports_cactus_flower = make_tag("supports_cactus_flower");
        let tag_cannot_support_seagrass = make_tag("cannot_support_seagrass");
        let tag_cannot_support_kelp = make_tag("cannot_support_kelp");
        let tag_cannot_support_snow_layer = make_tag("cannot_support_snow_layer");
        let tag_supports_snow_layer = make_tag("supports_snow_layer");

        Self {
            blocks,
            facts,
            tags,
            predicates,
            tag_bitsets,
            prop_strides,
            tree_cache: std::sync::OnceLock::new(),
            bid_air,
            bid_cave_air,
            bid_water,
            bid_lava,
            bid_bedrock,
            bid_stone,
            bid_cobblestone,
            bid_mossy_cobblestone,
            bid_obsidian,
            bid_end_stone,
            bid_end_portal,
            bid_end_gateway,
            bid_spawner,
            bid_chest,
            bid_torch,
            bid_wall_torch,
            bid_fire,
            bid_iron_bars,
            bid_ice,
            bid_packed_ice,
            bid_blue_ice,
            bid_snow,
            bid_snow_block,
            bid_podzol,
            bid_dirt_path,
            bid_bamboo,
            bid_vine,
            bid_magma_block,
            bid_dripstone_block,
            bid_pointed_dripstone,
            bid_sculk,
            bid_sculk_vein,
            bid_sculk_catalyst,
            bid_sculk_sensor,
            bid_sculk_shrieker,
            bid_moving_piston,
            bid_chorus_plant,
            bid_chorus_flower,
            bid_nether_wart_block,
            bid_weeping_vines,
            bid_weeping_vines_plant,
            bid_pale_moss_carpet,
            bid_glow_lichen,
            bid_resin_clump,
            bid_barrier,
            bid_grass_block,
            bid_mycelium,
            bid_dirt,
            bid_sand,
            bid_red_sand,
            bid_soul_sand,
            bid_soul_soil,
            bid_cactus,
            bid_sugar_cane,
            bid_farmland,
            bid_kelp,
            bid_kelp_plant,
            bid_twisting_vines,
            bid_twisting_vines_plant,
            bid_cave_vines,
            bid_cave_vines_plant,
            bid_big_dripleaf,
            bid_big_dripleaf_stem,
            bid_pale_hanging_moss,
            bid_nether_bricks,
            bid_nether_brick_fence,
            bid_nether_brick_stairs,
            bid_nether_wart,
            sid_air,
            sid_cave_air,
            sid_water,
            sid_lava,
            sid_bedrock,
            sid_stone,
            sid_cobblestone,
            sid_mossy_cobblestone,
            sid_obsidian,
            sid_end_stone,
            sid_end_portal,
            sid_end_gateway,
            sid_spawner,
            sid_chest,
            sid_torch,
            sid_fire,
            sid_ice,
            sid_blue_ice,
            sid_snow_1,
            sid_snow_8,
            sid_snow_block,
            sid_podzol,
            sid_bamboo_trunk,
            sid_bamboo_top_small,
            sid_bamboo_top_large,
            sid_bamboo_final_large,
            sid_magma_block,
            sid_dripstone_block,
            sid_sculk,
            sid_sculk_vein,
            sid_sculk_sensor,
            sid_sculk_shrieker_summon,
            sid_chorus_plant,
            sid_chorus_flower_5,
            sid_weeping_vines_plant,
            sid_pale_moss_carpet,
            sid_barrier,
            tag_base_stone_overworld,
            tag_beneath_bamboo_podzol_replaceable,
            tag_features_cannot_replace,
            tag_leaves,
            tag_replaceable_by_mushrooms,
            tag_replaceable_by_trees,
            tag_sculk_growth_inhibitors,
            tag_sculk_replaceable,
            tag_sculk_replaceable_world_gen,
            tag_snow_layer_can_survive_on,
            tag_snow_layer_cannot_survive_on,
            tag_supports_bamboo,
            tag_supports_chorus_plant,
            tag_fire,
            tag_dirt,
            tag_sand,
            tag_dry_vegetation_may_place_on,
            tag_nylium,
            tag_mushroom_grow_block,
            tag_small_dripleaf_placeable,
            tag_big_dripleaf_placeable,
            tag_bamboo_plantable_on,
            tag_dead_bush_may_place_on,
            tag_supports_vegetation,
            tag_supports_dry_vegetation,
            tag_supports_nether_sprouts,
            tag_supports_nether_fungus,
            tag_supports_nether_roots,
            tag_supports_crimson_roots,
            tag_supports_warped_roots,
            tag_supports_sugar_cane,
            tag_supports_cactus,
            tag_supports_azalea,
            tag_supports_hanging_moss,
            tag_supports_small_dripleaf,
            tag_supports_big_dripleaf,
            tag_supports_lily_pad,
            tag_supports_nether_wart,
            tag_supports_cocoa,
            tag_supports_melon_stem,
            tag_supports_pumpkin_stem,
            tag_supports_crops,
            tag_supports_wither_rose,
            tag_supports_mangrove_propagule,
            tag_supports_frogspawn,
            tag_supports_hanging_roots,
            tag_supports_pale_hanging_moss,
            tag_supports_wildflowers,
            tag_supports_flower_bed,
            tag_supports_leaf_litter,
            tag_supports_bush,
            tag_supports_firefly_bush,
            tag_supports_cactus_flower,
            tag_cannot_support_seagrass,
            tag_cannot_support_kelp,
            tag_cannot_support_snow_layer,
            tag_supports_snow_layer,
            delta_cannot_replace,
        }
    }

    /// Lazily resolved tree-feature constants (tags and states used by `features/tree`).
    #[inline]
    pub fn tree_cache(&self) -> &crate::features::tree::TreeCache {
        self.tree_cache
            .get_or_init(|| crate::features::tree::TreeCache::new(self))
    }

    /// Block protocol ID of `state_id`.
    #[inline]
    pub fn block_id(&self, state_id: u32) -> u32 {
        self.facts.state(state_id).block
    }

    /// Block class name of `state_id` (e.g. `"DoublePlantBlock"`).
    #[inline]
    pub fn block_class(&self, state_id: u32) -> &'static str {
        self.facts.block_of(state_id).info.class.as_str()
    }

    /// Block identifier path without namespace (e.g. `"stone"`).
    #[inline]
    pub fn block_path(&self, state_id: u32) -> &'static str {
        self.facts.block_of(state_id).info.id.as_str()
    }

    /// `state.isAir()`.
    #[inline]
    pub fn is_air(&self, state_id: u32) -> bool {
        self.facts.state(state_id).has(flag::IS_AIR)
    }

    /// `state.isSolid()` (legacy solid).
    #[inline]
    pub fn is_solid(&self, state_id: u32) -> bool {
        self.facts.state(state_id).has(flag::LEGACY_SOLID)
    }

    /// `state.isSolidRender()`.
    #[inline]
    pub fn is_solid_render(&self, state_id: u32) -> bool {
        self.facts.state(state_id).has(flag::SOLID_RENDER)
    }

    /// `state.canBeReplaced()`.
    #[inline]
    pub fn is_replaceable(&self, state_id: u32) -> bool {
        self.facts.state(state_id).has(flag::REPLACEABLE)
    }

    /// `state.liquid()`.
    #[inline]
    pub fn is_liquid(&self, state_id: u32) -> bool {
        self.facts.state(state_id).has(flag::LIQUID)
    }

    /// `state.isCollisionShapeFullBlock(...)`.
    #[inline]
    pub fn is_collision_shape_full_block(&self, state_id: u32) -> bool {
        self.facts
            .state(state_id)
            .has(flag::COLLISION_SHAPE_FULL_BLOCK)
    }

    /// `state.isFaceSturdy(level, pos, dir)` (`SupportType::Full`).
    #[inline]
    pub fn is_face_sturdy(&self, state_id: u32, dir: Direction) -> bool {
        self.facts
            .state(state_id)
            .is_face_sturdy(dir, SupportType::Full)
    }

    /// `Block.isFaceFull(state.getCollisionShape(...), dir)`.
    #[inline]
    pub fn is_collision_face_full(&self, state_id: u32, dir: Direction) -> bool {
        (self.facts.state(state_id).collision_face_full & (1 << dir.ordinal())) != 0
    }

    /// `state.is(Blocks.WATER)`.
    #[inline]
    pub fn is_water(&self, state_id: u32) -> bool {
        self.block_id(state_id) == self.bid_water
    }

    /// `state.is(Blocks.LAVA)`.
    #[inline]
    pub fn is_lava(&self, state_id: u32) -> bool {
        self.block_id(state_id) == self.bid_lava
    }

    /// `state.getFluidState()`.
    #[inline]
    pub fn fluid(&self, state_id: u32) -> zenith_blocks::FluidState {
        self.facts.state(state_id).fluid
    }

    /// `level.isWaterAt(pos)` (`state.getFluidState().is(FluidTags.WATER)`).
    #[inline]
    pub fn is_water_fluid(&self, state_id: u32) -> bool {
        self.facts.state(state_id).fluid.kind.is_water()
    }

    /// `state.getFluidState().is(FluidTags.LAVA)`.
    #[inline]
    pub fn is_lava_fluid(&self, state_id: u32) -> bool {
        self.facts.state(state_id).fluid.kind.is_lava()
    }

    /// `state.is(tag_id)` using [`BlockTags`].
    #[inline]
    pub fn is_in_tag(&self, state_id: u32, tag_id: &Identifier) -> bool {
        let bid = self.block_id(state_id);
        if let Some(bs) = self.tag_bitsets.get(tag_id) {
            return bs.contains(bid);
        }
        self.tags.contains_block(tag_id, bid)
    }

    /// Builds a [`BlockBitSet`] for a tag ID.
    pub fn bitset_for_tag(&self, tag_id: &Identifier) -> BlockBitSet {
        let mut bs = BlockBitSet::new(self.blocks.blocks().len());
        if let Some(ids) = self.tags.block_ids(tag_id) {
            for &id in ids {
                bs.insert(id);
            }
        }
        bs
    }

    /// Resolves a `(block_id, properties)` spec (data version >= 5006 lenient rules) into a state ID.
    pub fn resolve_state(
        &self,
        block_id: &Identifier,
        properties: &BTreeMap<String, String>,
    ) -> Option<u32> {
        let entry = self.blocks.block(block_id)?;
        if entry.properties.is_empty() || properties.is_empty() {
            return Some(entry.default_state);
        }
        let default_state = self.blocks.state_by_id_exact(entry.default_state as i32)?;
        let mut offset = 0u32;
        for prop in &entry.properties {
            let val_idx = properties
                .get(prop.name())
                .and_then(|v| prop.values().iter().position(|pv| pv == v))
                .unwrap_or_else(|| {
                    let dv = default_state.property_value(prop.name()).unwrap_or("");
                    prop.values().iter().position(|pv| pv == dv).unwrap_or(0)
                }) as u32;
            offset = offset * (prop.values().len() as u32) + val_idx;
        }
        Some(entry.first_state + offset)
    }

    /// Reads property `name` of `state_id`.
    #[inline]
    pub fn get_property(&self, state_id: u32, name: &str) -> Option<&str> {
        let bid = self.block_id(state_id) as usize;
        let entry = &self.blocks.blocks()[bid];
        let prop_idx = entry
            .properties
            .binary_search_by(|p| p.name().cmp(name))
            .ok()?;
        let rel = state_id - entry.first_state;
        let stride = self.prop_strides[bid][prop_idx];
        let card = entry.properties[prop_idx].values().len() as u32;
        let val_idx = (rel / stride) % card;
        Some(entry.properties[prop_idx].values()[val_idx as usize].as_str())
    }

    /// Returns whether `state_id`'s block has a property named `name`.
    #[inline]
    pub fn has_property(&self, state_id: u32, name: &str) -> bool {
        let bid = self.block_id(state_id) as usize;
        let entry = &self.blocks.blocks()[bid];
        entry
            .properties
            .binary_search_by(|p| p.name().cmp(name))
            .is_ok()
    }

    /// Sets property `name = value` on `state_id`, returning `None` if property or value is invalid.
    #[inline]
    pub fn set_property(&self, state_id: u32, name: &str, value: &str) -> Option<u32> {
        let bid = self.block_id(state_id) as usize;
        let entry = &self.blocks.blocks()[bid];
        let prop_idx = entry
            .properties
            .binary_search_by(|p| p.name().cmp(name))
            .ok()?;
        let prop = &entry.properties[prop_idx];
        let new_val_idx = prop.values().iter().position(|v| v == value)? as u32;
        let rel = state_id - entry.first_state;
        let stride = self.prop_strides[bid][prop_idx];
        let card = prop.values().len() as u32;
        let old_val_idx = (rel / stride) % card;
        let new_rel = rel - old_val_idx * stride + new_val_idx * stride;
        Some(entry.first_state + new_rel)
    }

    /// `state.trySetValue(prop, value)`: returns updated state if property & value exist, else `state_id`.
    #[inline]
    pub fn try_set_property(&self, state_id: u32, name: &str, value: &str) -> u32 {
        self.set_property(state_id, name, value).unwrap_or(state_id)
    }

    /// `target.withPropertiesOf(source)`: copies every matching property present on both blocks.
    pub fn with_properties_of(&self, mut target: u32, source: u32) -> u32 {
        let src_bid = self.block_id(source) as usize;
        let src_entry = &self.blocks.blocks()[src_bid];
        if src_entry.properties.is_empty() {
            return target;
        }
        let tgt_bid = self.block_id(target) as usize;
        let tgt_entry = &self.blocks.blocks()[tgt_bid];
        if tgt_entry.properties.is_empty() {
            return target;
        }
        let src_rel = source - src_entry.first_state;
        for (i, prop) in src_entry.properties.iter().enumerate() {
            let stride = self.prop_strides[src_bid][i];
            let card = prop.values().len() as u32;
            let val_idx = (src_rel / stride) % card;
            let val = &prop.values()[val_idx as usize];
            target = self.try_set_property(target, prop.name(), val);
        }
        target
    }

    /// Sets `waterlogged` property if present on `state_id`.
    #[inline]
    pub fn copy_waterlogged(&self, state_id: u32, waterlogged: bool) -> u32 {
        self.try_set_property(
            state_id,
            "waterlogged",
            if waterlogged { "true" } else { "false" },
        )
    }

    /// `MultifaceBlock.canAttachTo(level, dir, pos, state)`:
    /// `Block.isFaceFull(state.getCollisionShape(level, pos), dir.getOpposite())`
    /// where `pos` is the neighbor block position and `dir` is the direction FROM the multiface TO the neighbor.
    #[inline]
    pub fn can_attach_multiface_to(&self, neighbor_state: u32, dir_to_neighbor: Direction) -> bool {
        // Note: in vanilla, `MultifaceBlock.canAttachTo(level, dir, neighborPos, neighborState)`
        // returns `Block.isFaceFull(neighborState.getBlockSupportShape(level, neighborPos), dir.getOpposite())`
        // wait: `MultifaceBlock` checks `!isExceptionForConnection`? No, `Block.isFaceFull(collisionShape, opposite)`!
        // In BlockFacts: `is_collision_face_full(neighbor_state, dir_to_neighbor.opposite())`.
        self.is_collision_face_full(neighbor_state, dir_to_neighbor.opposite())
    }

    /// `VineBlock.isAcceptableNeighbour(level, neighborPos, dir)` (direction from vine to neighbor).
    #[inline]
    pub fn is_acceptable_vine_neighbour(
        &self,
        neighbor_state: u32,
        dir_to_neighbor: Direction,
    ) -> bool {
        self.is_collision_face_full(neighbor_state, dir_to_neighbor.opposite())
    }

    /// Returns whether `state_id` is a `MultifaceBlock` with `dir` face set to `"true"`.
    #[inline]
    pub fn multiface_has_face(&self, state_id: u32, dir: Direction) -> bool {
        self.get_property(state_id, dir.name()) == Some("true")
    }

    /// Returns all active faces on a `MultifaceBlock` state (bitmask by `Direction::ordinal()`).
    #[inline]
    pub fn multiface_available_faces(&self, state_id: u32) -> u8 {
        let mut mask = 0u8;
        for dir in Direction::ALL {
            if self.multiface_has_face(state_id, dir) {
                mask |= 1 << dir.ordinal();
            }
        }
        mask
    }

    /// Whether `state_id` is `SculkBlock` or `SculkVeinBlock` (`implements SculkBehaviour`).
    #[inline]
    pub fn is_sculk_behaviour(&self, state_id: u32) -> bool {
        let bid = self.block_id(state_id);
        bid == self.bid_sculk || bid == self.bid_sculk_vein
    }

    /// Whether `state_id` is `DoublePlantBlock` (or `TallFlowerBlock`, `TallSeagrassBlock`, `PitcherCropBlock`, `SmallDripleafBlock`).
    #[inline]
    pub fn is_double_plant(&self, state_id: u32) -> bool {
        matches!(
            self.block_class(state_id),
            "DoublePlantBlock" | "TallFlowerBlock" | "TallSeagrassBlock" | "SmallDripleafBlock"
        )
    }

    /// `TreeFeature.validTreePos`: `state.isAir() || state.is(#minecraft:replaceable_by_trees)`.
    #[inline]
    pub fn valid_tree_pos(&self, state_id: u32) -> bool {
        self.is_air(state_id)
            || self
                .tag_replaceable_by_trees
                .contains(self.block_id(state_id))
    }

    /// `BlockState.canSurvive(level, pos)` for worldgen feature blocks.
    pub fn can_survive(&self, region: &GenerationRegion, pos: BlockPos, state_id: u32) -> bool {
        let bid = self.block_id(state_id);
        let class = self.block_class(state_id);
        let path = self.block_path(state_id);
        let below_pos = pos.below();
        let below = region.get_block(below_pos);
        let below_bid = self.block_id(below);

        match class {
            "BambooStalkBlock" | "BambooSaplingBlock" => {
                self.tag_supports_bamboo.contains(below_bid)
                    || self.tag_bamboo_plantable_on.contains(below_bid)
            }
            "SnowLayerBlock" => {
                let can_not = self.tag_snow_layer_cannot_survive_on.contains(below_bid)
                    || self.tag_cannot_support_snow_layer.contains(below_bid);
                if can_not {
                    return false;
                }
                if self.tag_snow_layer_can_survive_on.contains(below_bid)
                    || self.tag_supports_snow_layer.contains(below_bid)
                {
                    return true;
                }
                self.is_collision_face_full(below, Direction::Up)
                    || (below_bid == self.bid_snow && below == self.sid_snow_8)
            }
            "DoublePlantBlock" | "TallFlowerBlock" => {
                if self.get_property(state_id, "half") == Some("upper") {
                    below_bid == bid && self.get_property(below, "half") == Some("lower")
                } else {
                    self.may_place_vegetation_on(below_bid)
                }
            }
            "TallSeagrassBlock" => {
                if self.get_property(state_id, "half") == Some("upper") {
                    below_bid == bid && self.get_property(below, "half") == Some("lower")
                } else {
                    self.can_support_seagrass(below, below_bid)
                }
            }
            "SeagrassBlock" => self.can_support_seagrass(below, below_bid),
            "KelpBlock" | "KelpPlantBlock" => {
                below_bid == self.bid_kelp
                    || below_bid == self.bid_kelp_plant
                    || (self.is_face_sturdy(below, Direction::Up)
                        && !self.tag_cannot_support_kelp.contains(below_bid)
                        && below_bid != self.bid_magma_block)
            }
            "SeaPickleBlock" => {
                !self
                    .facts
                    .face_occlusion_shape(below, Direction::Up)
                    .is_empty()
                    || self.is_face_sturdy(below, Direction::Up)
            }
            "TallGrassBlock"
            | "FlowerBlock"
            | "EyeblossomBlock"
            | "SaplingBlock"
            | "BushBlock"
            | "FireflyBushBlock"
            | "SweetBerryBushBlock" => self.may_place_vegetation_on(below_bid),
            "WitherRoseBlock" => {
                self.may_place_vegetation_on(below_bid)
                    || self.tag_supports_wither_rose.contains(below_bid)
            }
            "AzaleaBlock" => {
                self.may_place_vegetation_on(below_bid)
                    || self.tag_supports_azalea.contains(below_bid)
            }
            "ShortDryGrassBlock" | "TallDryGrassBlock" | "DryVegetationBlock" => {
                self.tag_supports_dry_vegetation.contains(below_bid)
                    || self.tag_dry_vegetation_may_place_on.contains(below_bid)
                    || self.tag_dead_bush_may_place_on.contains(below_bid)
            }
            "CactusFlowerBlock" => {
                self.tag_supports_cactus_flower.contains(below_bid)
                    || below_bid == self.bid_cactus
                    || below_bid == self.bid_farmland
                    || self.is_face_sturdy(below, Direction::Up)
            }
            "CactusBlock" => {
                for d in Direction::HORIZONTAL {
                    let ns = region.get_block(pos.relative(d));
                    if self.is_solid(ns) || self.is_lava_fluid(ns) {
                        return false;
                    }
                }
                (below_bid == self.bid_cactus
                    || self.tag_supports_cactus.contains(below_bid)
                    || self.tag_sand.contains(below_bid))
                    && !self.is_liquid(region.get_block(pos.above()))
            }
            "SugarCaneBlock" => {
                if below_bid == self.bid_sugar_cane {
                    return true;
                }
                let base_ok = self.tag_supports_sugar_cane.contains(below_bid)
                    || self.tag_dirt.contains(below_bid)
                    || self.tag_sand.contains(below_bid);
                if !base_ok {
                    return false;
                }
                for d in Direction::HORIZONTAL {
                    let adj = region.get_block(below_pos.relative(d));
                    if self.is_water_fluid(adj) || self.block_path(adj) == "frosted_ice" {
                        return true;
                    }
                }
                false
            }
            "MushroomBlock" => {
                if self.tag_mushroom_grow_block.contains(below_bid) {
                    return true;
                }
                // `getRawBrightness(pos, 0) < 13`: sky light reads 15 in unlit dimensions with sky light.
                region.raw_brightness() < 13 && self.is_solid_render(below)
            }
            "NetherRootsBlock" => {
                self.tag_supports_nether_roots.contains(below_bid)
                    || self.tag_supports_crimson_roots.contains(below_bid)
                    || self.tag_supports_warped_roots.contains(below_bid)
                    || self.tag_nylium.contains(below_bid)
                    || below_bid == self.bid_soul_soil
                    || self.may_place_vegetation_on(below_bid)
            }
            "NetherSproutsBlock" => {
                self.tag_supports_nether_sprouts.contains(below_bid)
                    || self.tag_nylium.contains(below_bid)
                    || below_bid == self.bid_soul_soil
                    || self.may_place_vegetation_on(below_bid)
            }
            "NetherFungusBlock" => {
                self.tag_supports_nether_fungus.contains(below_bid)
                    || self.tag_nylium.contains(below_bid)
                    || below_bid == self.bid_mycelium
                    || below_bid == self.bid_soul_soil
                    || self.may_place_vegetation_on(below_bid)
            }
            "MossyCarpetBlock" => {
                if self.get_property(state_id, "bottom") == Some("true") {
                    !self.is_air(below)
                } else {
                    below_bid == bid
                }
            }
            "CarpetBlock" => !self.is_air(below),
            "FlowerBedBlock" => {
                self.tag_supports_flower_bed.contains(below_bid)
                    || self.tag_supports_wildflowers.contains(below_bid)
                    || self.may_place_vegetation_on(below_bid)
            }
            "LeafLitterBlock" => {
                self.tag_supports_leaf_litter.contains(below_bid)
                    || self.is_face_sturdy(below, Direction::Up)
            }
            "WaterlilyBlock" | "LilyPadBlock" => {
                let fl = self.fluid(below);
                let above_fl = self.fluid(pos_state_above(region, below_pos));
                (fl.kind == FluidKind::Water || below_bid == self.bid_ice) && above_fl.is_empty()
            }
            "SmallDripleafBlock" => {
                if self.get_property(state_id, "half") == Some("upper") {
                    below_bid == bid && self.get_property(below, "half") == Some("lower")
                } else {
                    self.tag_supports_small_dripleaf.contains(below_bid)
                        || self.tag_small_dripleaf_placeable.contains(below_bid)
                        || (self.is_water_fluid(region.get_block(pos))
                            && self.may_place_vegetation_on(below_bid))
                }
            }
            "BigDripleafBlock" => {
                below_bid == self.bid_big_dripleaf
                    || below_bid == self.bid_big_dripleaf_stem
                    || self.tag_supports_big_dripleaf.contains(below_bid)
                    || self.tag_big_dripleaf_placeable.contains(below_bid)
            }
            "BigDripleafStemBlock" => {
                let above = region.get_block(pos.above());
                let above_bid = self.block_id(above);
                (above_bid == self.bid_big_dripleaf || above_bid == self.bid_big_dripleaf_stem)
                    && (below_bid == self.bid_big_dripleaf_stem
                        || self.tag_supports_big_dripleaf.contains(below_bid)
                        || self.tag_big_dripleaf_placeable.contains(below_bid))
            }
            "HangingRootsBlock" => {
                let above = region.get_block(pos.above());
                self.is_face_sturdy(above, Direction::Down)
            }
            "HangingMossBlock" => {
                let above = region.get_block(pos.above());
                let above_bid = self.block_id(above);
                above_bid == self.bid_pale_hanging_moss
                    || self.tag_supports_pale_hanging_moss.contains(above_bid)
                    || self.tag_supports_hanging_moss.contains(above_bid)
                    || self.is_face_sturdy(above, Direction::Down)
            }
            "SporeBlossomBlock" => {
                let above = region.get_block(pos.above());
                self.is_face_sturdy(above, Direction::Down) && !self.is_water(region.get_block(pos))
            }
            "WeepingVinesBlock" | "WeepingVinesPlantBlock" => {
                let above = region.get_block(pos.above());
                let above_bid = self.block_id(above);
                above_bid == self.bid_weeping_vines
                    || above_bid == self.bid_weeping_vines_plant
                    || self.is_face_sturdy(above, Direction::Down)
            }
            "TwistingVinesBlock" | "TwistingVinesPlantBlock" => {
                below_bid == self.bid_twisting_vines
                    || below_bid == self.bid_twisting_vines_plant
                    || self.is_face_sturdy(below, Direction::Up)
            }
            "CaveVinesBlock" | "CaveVinesPlantBlock" => {
                let above = region.get_block(pos.above());
                let above_bid = self.block_id(above);
                above_bid == self.bid_cave_vines
                    || above_bid == self.bid_cave_vines_plant
                    || self.is_face_sturdy(above, Direction::Down)
            }
            "CoralPlantBlock" | "CoralFanBlock" | "BaseCoralPlantBlock" | "BaseCoralFanBlock" => {
                self.is_face_sturdy(below, Direction::Up)
            }
            "CoralWallFanBlock" | "BaseCoralWallFanBlock" => {
                let facing = self
                    .get_property(state_id, "facing")
                    .and_then(Direction::from_name)
                    .unwrap_or(Direction::North);
                let against = region.get_block(pos.relative(facing.opposite()));
                self.is_face_sturdy(against, facing)
            }
            "TorchBlock" => self
                .facts
                .state(below)
                .is_face_sturdy(Direction::Up, SupportType::Center),
            "WallTorchBlock" => {
                let facing = self
                    .get_property(state_id, "facing")
                    .and_then(Direction::from_name)
                    .unwrap_or(Direction::North);
                let against = region.get_block(pos.relative(facing.opposite()));
                self.is_face_sturdy(against, facing)
            }
            "AmethystClusterBlock" => {
                let facing = self
                    .get_property(state_id, "facing")
                    .and_then(Direction::from_name)
                    .unwrap_or(Direction::Up);
                let against = region.get_block(pos.relative(facing.opposite()));
                self.is_face_sturdy(against, facing)
            }
            "PointedDripstoneBlock" | "SulfurSpikeBlock" => {
                let tip_dir = self
                    .get_property(state_id, "vertical_direction")
                    .and_then(Direction::from_name)
                    .unwrap_or(Direction::Up);
                let against = region.get_block(pos.relative(tip_dir.opposite()));
                self.is_face_sturdy(against, tip_dir)
                    || (self.block_id(against) == bid
                        && self.get_property(against, "vertical_direction") == Some(tip_dir.name()))
            }
            "GlowLichenBlock" | "SculkVeinBlock" | "MultifaceBlock" => {
                let mut any = false;
                for dir in Direction::ALL {
                    if self.multiface_has_face(state_id, dir) {
                        any = true;
                        let against = region.get_block(pos.relative(dir));
                        if !self.can_attach_multiface_to(against, dir) {
                            return false;
                        }
                    }
                }
                any
            }
            "VineBlock" => {
                let mut count = 0u32;
                for dir in Direction::ALL {
                    if dir == Direction::Down {
                        continue;
                    }
                    if self.get_property(state_id, dir.name()) == Some("true") {
                        let against = region.get_block(pos.relative(dir));
                        if self.is_acceptable_vine_neighbour(against, dir)
                            || (dir != Direction::Up
                                && self.block_id(region.get_block(pos.above())) == self.bid_vine
                                && self.get_property(region.get_block(pos.above()), dir.name())
                                    == Some("true"))
                        {
                            count += 1;
                        }
                    }
                }
                count > 0
            }
            "MangrovePropaguleBlock" => {
                if self.get_property(state_id, "hanging") == Some("true") {
                    let above = region.get_block(pos.above());
                    self.block_path(above) == "mangrove_leaves"
                } else {
                    self.may_place_vegetation_on(below_bid)
                        || self.tag_supports_mangrove_propagule.contains(below_bid)
                }
            }
            "ChorusPlantBlock" => {
                let above = region.get_block(pos.above());
                let above_bid = self.block_id(above);
                let has_vert = !self.is_air(above) && !self.is_air(below);
                for dir in Direction::HORIZONTAL {
                    let adj_pos = pos.relative(dir);
                    let adj = region.get_block(adj_pos);
                    if self.block_id(adj) == self.bid_chorus_plant {
                        if has_vert {
                            return false;
                        }
                        let adj_below = region.get_block(adj_pos.below());
                        let adj_below_bid = self.block_id(adj_below);
                        if adj_below_bid == self.bid_chorus_plant
                            || self.tag_supports_chorus_plant.contains(adj_below_bid)
                        {
                            return true;
                        }
                    }
                }
                let _ = above_bid;
                below_bid == self.bid_chorus_plant
                    || self.tag_supports_chorus_plant.contains(below_bid)
            }
            "ChorusFlowerBlock" => {
                if below_bid == self.bid_chorus_plant
                    || self.tag_supports_chorus_plant.contains(below_bid)
                {
                    return true;
                }
                if !self.is_air(below) {
                    return false;
                }
                let mut adj_plants = 0;
                for dir in Direction::HORIZONTAL {
                    let adj = region.get_block(pos.relative(dir));
                    if self.block_id(adj) == self.bid_chorus_plant {
                        adj_plants += 1;
                    } else if !self.is_air(adj) {
                        return false;
                    }
                }
                adj_plants == 1
            }
            "BaseFireBlock" | "FireBlock" => self.is_face_sturdy(below, Direction::Up),
            "SoulFireBlock" => below_bid == self.bid_soul_sand || below_bid == self.bid_soul_soil,
            "PumpkinBlock" | "MelonBlock" => true,
            _ => {
                // Fallback for any other plant-like blocks (e.g. nether wart, crops).
                match path {
                    "nether_wart" => {
                        below_bid == self.bid_soul_sand
                            || self.tag_supports_nether_wart.contains(below_bid)
                    }
                    "crimson_roots" | "warped_roots" => {
                        self.tag_nylium.contains(below_bid)
                            || below_bid == self.bid_soul_soil
                            || self.may_place_vegetation_on(below_bid)
                    }
                    _ => true,
                }
            }
        }
    }

    #[inline]
    fn may_place_vegetation_on(&self, below_bid: u32) -> bool {
        self.tag_supports_vegetation.contains(below_bid)
            || self.tag_dirt.contains(below_bid)
            || below_bid == self.bid_farmland
    }

    #[inline]
    fn can_support_seagrass(&self, below: u32, below_bid: u32) -> bool {
        self.is_face_sturdy(below, Direction::Up)
            && !self.tag_cannot_support_seagrass.contains(below_bid)
            && below_bid != self.bid_magma_block
    }

    /// `DoublePlantBlock.placeAt(level, state, pos, flags = 2)` (`features.md` §4.13).
    pub fn place_double_plant(
        &self,
        region: &mut GenerationRegion,
        state_id: u32,
        pos: BlockPos,
        flags: u32,
    ) {
        let lower = self.try_set_property(state_id, "half", "lower");
        let lower = self.copy_waterlogged(lower, self.is_water_fluid(region.get_block(pos)));
        region.set_block(pos, lower, flags);

        let above = pos.above();
        let upper = self.try_set_property(state_id, "half", "upper");
        let upper = self.copy_waterlogged(upper, self.is_water_fluid(region.get_block(above)));
        region.set_block(above, upper, flags);
    }

    /// `MossyCarpetBlock.placeAt(level, pos, level.getRandom(), flags = 2)` (`features.md` §4.13).
    pub fn place_mossy_carpet(&self, region: &mut GenerationRegion, pos: BlockPos, flags: u32) {
        let adjusted =
            self.mossy_carpet_updated_state(self.sid_pale_moss_carpet, region, pos, true);
        region.set_block(pos, adjusted, flags);

        let above = pos.above();
        let above_prev = region.get_block(above);
        let is_mossy_above = self.block_id(above_prev) == self.bid_pale_moss_carpet;
        let can_topper = (is_mossy_above
            && self.get_property(above_prev, "bottom") == Some("false"))
            || (!is_mossy_above && self.is_replaceable(above_prev));
        if can_topper {
            let base_false = self.try_set_property(self.sid_pale_moss_carpet, "bottom", "false");
            let mut above_state = self.mossy_carpet_updated_state(base_false, region, above, true);
            for dir in Direction::HORIZONTAL {
                if self.get_property(above_state, dir.name()) != Some("none")
                    && !region.region_random_mut().next_boolean()
                {
                    above_state = self.try_set_property(above_state, dir.name(), "none");
                }
            }
            let has_any_side = Direction::HORIZONTAL
                .iter()
                .any(|&d| self.get_property(above_state, d.name()) != Some("none"));
            if has_any_side && above_state != above_prev {
                region.set_block(above, above_state, flags);
                let promoted = self.mossy_carpet_updated_state(adjusted, region, pos, true);
                region.set_block(pos, promoted, flags);
            }
        }
    }

    pub(crate) fn mossy_carpet_updated_state(
        &self,
        mut state: u32,
        region: &GenerationRegion,
        pos: BlockPos,
        create_sides: bool,
    ) -> u32 {
        let is_bottom = self.get_property(state, "bottom") == Some("true");
        let above = region.get_block(pos.above());
        let above_is_topper = self.block_id(above) == self.bid_pale_moss_carpet
            && self.get_property(above, "bottom") == Some("false");
        let below = region.get_block(pos.below());
        let below_is_carpet = self.block_id(below) == self.bid_pale_moss_carpet;

        for dir in Direction::HORIZONTAL {
            let cur_side = self.get_property(state, dir.name()).unwrap_or("none");
            let against = region.get_block(pos.relative(dir));
            let can_attach = self.can_attach_multiface_to(against, dir);
            let new_side = if !can_attach {
                "none"
            } else if is_bottom {
                if above_is_topper && self.get_property(above, dir.name()) != Some("none") {
                    "tall"
                } else if create_sides || cur_side != "none" {
                    "low"
                } else {
                    "none"
                }
            } else if below_is_carpet && self.get_property(below, dir.name()) != Some("none") {
                if create_sides || cur_side != "none" {
                    "low"
                } else {
                    "none"
                }
            } else {
                "none"
            };
            state = self.try_set_property(state, dir.name(), new_side);
        }
        state
    }

    /// `ChorusPlantBlock.getStateWithConnections(level, pos, default_state)` (`features.md` §4.34).
    pub fn chorus_plant_connections(&self, region: &GenerationRegion, pos: BlockPos) -> u32 {
        let mut s = self.sid_chorus_plant;
        let down = region.get_block(pos.below());
        let down_bid = self.block_id(down);
        let down_conn = down_bid == self.bid_chorus_plant
            || down_bid == self.bid_chorus_flower
            || self.tag_supports_chorus_plant.contains(down_bid);
        s = self.try_set_property(s, "down", if down_conn { "true" } else { "false" });
        for dir in [
            Direction::Up,
            Direction::North,
            Direction::East,
            Direction::South,
            Direction::West,
        ] {
            let nbid = self.block_id(region.get_block(pos.relative(dir)));
            let conn = nbid == self.bid_chorus_plant || nbid == self.bid_chorus_flower;
            s = self.try_set_property(s, dir.name(), if conn { "true" } else { "false" });
        }
        s
    }

    /// `ChorusFlowerBlock.generatePlant(level, origin, random, maxHorizontalSpread = 8)` (§4.34).
    pub fn generate_chorus_plant(
        &self,
        region: &mut GenerationRegion,
        origin: BlockPos,
        rng: &mut impl RandomSource,
        max_horizontal_spread: i32,
    ) {
        let s = self.chorus_plant_connections(region, origin);
        region.set_block(origin, s, 2);
        self.grow_chorus_recursive(region, origin, rng, origin, max_horizontal_spread, 0);
    }

    fn grow_chorus_recursive(
        &self,
        region: &mut GenerationRegion,
        current: BlockPos,
        rng: &mut impl RandomSource,
        start_pos: BlockPos,
        max_horizontal_spread: i32,
        depth: i32,
    ) {
        let mut height = rng.next_int_bounded(4) + 1;
        if depth == 0 {
            height += 1;
        }
        for i in 0..height {
            let target = current.above_n(i + 1);
            if !self.all_horizontal_neighbors_empty(region, target, None) {
                return;
            }
            let st = self.chorus_plant_connections(region, target);
            region.set_block(target, st, 2);
            let sb = self.chorus_plant_connections(region, target.below());
            region.set_block(target.below(), sb, 2);
        }
        let mut placed_stem = false;
        if depth < 4 {
            let mut stems = rng.next_int_bounded(4);
            if depth == 0 {
                stems += 1;
            }
            for _ in 0..stems {
                let dir = direction_horizontal_random(rng);
                let target = current.above_n(height).relative(dir);
                if (target.x - start_pos.x).abs() < max_horizontal_spread
                    && (target.z - start_pos.z).abs() < max_horizontal_spread
                    && self.is_air(region.get_block(target))
                    && self.is_air(region.get_block(target.below()))
                    && self.all_horizontal_neighbors_empty(region, target, Some(dir.opposite()))
                {
                    placed_stem = true;
                    let st = self.chorus_plant_connections(region, target);
                    region.set_block(target, st, 2);
                    let so = self.chorus_plant_connections(region, target.relative(dir.opposite()));
                    region.set_block(target.relative(dir.opposite()), so, 2);
                    self.grow_chorus_recursive(
                        region,
                        target,
                        rng,
                        start_pos,
                        max_horizontal_spread,
                        depth + 1,
                    );
                }
            }
        }
        if !placed_stem {
            region.set_block(current.above_n(height), self.sid_chorus_flower_5, 2);
        }
    }

    #[inline]
    fn all_horizontal_neighbors_empty(
        &self,
        region: &GenerationRegion,
        pos: BlockPos,
        exclude: Option<Direction>,
    ) -> bool {
        for dir in Direction::HORIZONTAL {
            if Some(dir) == exclude {
                continue;
            }
            if !self.is_air(region.get_block(pos.relative(dir))) {
                return false;
            }
        }
        true
    }

    /// `MultifaceBlock.getStateForPlacement(oldState, level, pos, dir)` (`features.md` §4.58).
    pub fn multiface_state_for_placement(
        &self,
        placer_block_id: u32,
        old_state: u32,
        region: &GenerationRegion,
        pos: BlockPos,
        dir: Direction,
    ) -> Option<u32> {
        if !self.is_valid_multiface_placement(placer_block_id, old_state, region, pos, dir) {
            return None;
        }
        let base = if self.block_id(old_state) == placer_block_id {
            old_state
        } else {
            let def = self.blocks.blocks()[placer_block_id as usize].default_state;
            if self.is_water(old_state) && self.fluid(old_state).is_source {
                self.copy_waterlogged(def, true)
            } else {
                def
            }
        };
        self.set_property(base, dir.name(), "true")
    }

    /// `MultifaceBlock.isValidStateForPlacement` (`features.md` §4.58).
    pub fn is_valid_multiface_placement(
        &self,
        placer_block_id: u32,
        old_state: u32,
        region: &GenerationRegion,
        pos: BlockPos,
        dir: Direction,
    ) -> bool {
        if self.block_id(old_state) == placer_block_id && self.multiface_has_face(old_state, dir) {
            return false;
        }
        let against = region.get_block(pos.relative(dir));
        self.can_attach_multiface_to(against, dir)
    }

    /// `MultifaceSpreader.spreadFromFaceTowardRandomDirection` (`features.md` §4.58).
    #[allow(clippy::too_many_arguments)]
    pub fn multiface_spread_random_dir(
        &self,
        placer_block_id: u32,
        state: u32,
        region: &mut GenerationRegion,
        pos: BlockPos,
        face: Direction,
        rng: &mut impl RandomSource,
        post_process: bool,
    ) -> bool {
        let is_sculk_vein = placer_block_id == self.bid_sculk_vein;
        for spread_dir in direction_all_shuffled(rng) {
            if spread_dir.axis() == face.axis() {
                continue;
            }
            if self.multiface_try_spread_dir(
                placer_block_id,
                is_sculk_vein,
                state,
                region,
                pos,
                face,
                spread_dir,
                true,
                post_process,
            ) {
                return true;
            }
        }
        false
    }

    /// `SculkVeinBlock` `spreadAll` (`same_space_only = true` for `sameSpaceSpreader`, `false` for `veinSpreader`).
    pub fn sculk_vein_spread_all(
        &self,
        state: u32,
        region: &mut GenerationRegion,
        pos: BlockPos,
        same_space_only: bool,
        post_process: bool,
    ) -> i64 {
        let mut count = 0i64;
        let other_valid = self.block_id(state) != self.bid_sculk_vein;
        for face in Direction::ALL {
            if !other_valid && !self.multiface_has_face(state, face) {
                continue;
            }
            for spread_dir in Direction::ALL {
                if spread_dir.axis() == face.axis() {
                    continue;
                }
                if self.multiface_try_spread_dir(
                    self.bid_sculk_vein,
                    true,
                    state,
                    region,
                    pos,
                    face,
                    spread_dir,
                    !same_space_only,
                    post_process,
                ) {
                    count += 1;
                }
            }
        }
        count
    }

    #[allow(clippy::too_many_arguments)]
    fn multiface_try_spread_dir(
        &self,
        placer_block_id: u32,
        is_sculk_vein: bool,
        _source_state: u32,
        region: &mut GenerationRegion,
        source_pos: BlockPos,
        face: Direction,
        spread_dir: Direction,
        allow_plane_and_wrap: bool,
        post_process: bool,
    ) -> bool {
        let candidates: [(BlockPos, Direction); 3] = [
            // SAME_POSITION
            (source_pos, spread_dir),
            // SAME_PLANE
            (source_pos.relative(spread_dir), face),
            // WRAP_AROUND
            (
                source_pos.relative(spread_dir).relative(face),
                spread_dir.opposite(),
            ),
        ];
        let limit = if allow_plane_and_wrap { 3 } else { 1 };
        for &(place_pos, place_dir) in &candidates[..limit] {
            if self.multiface_can_spread_into(
                placer_block_id,
                is_sculk_vein,
                region,
                source_pos,
                place_pos,
                place_dir,
            ) {
                let existing = region.get_block(place_pos);
                if let Some(new_state) = self.multiface_state_for_placement(
                    placer_block_id,
                    existing,
                    region,
                    place_pos,
                    place_dir,
                ) {
                    region.set_block(place_pos, new_state, 2);
                    if post_process {
                        region.mark_pos_for_post_processing(place_pos);
                    }
                    return true;
                }
            }
        }
        false
    }

    fn multiface_can_spread_into(
        &self,
        placer_block_id: u32,
        is_sculk_vein: bool,
        region: &GenerationRegion,
        source_pos: BlockPos,
        place_pos: BlockPos,
        place_dir: Direction,
    ) -> bool {
        let existing = region.get_block(place_pos);
        if is_sculk_vein {
            let against = region.get_block(place_pos.relative(place_dir));
            let against_bid = self.block_id(against);
            if against_bid == self.bid_sculk
                || against_bid == self.bid_sculk_catalyst
                || against_bid == self.bid_moving_piston
            {
                return false;
            }
            if source_pos.dist_manhattan(place_pos) == 2 {
                let inter_pos = source_pos.relative(place_dir.opposite());
                let inter_state = region.get_block(inter_pos);
                if self.is_face_sturdy(inter_state, place_dir) {
                    return false;
                }
            }
            let fl = self.fluid(existing);
            if !fl.is_empty() && fl.kind != FluidKind::Water {
                return false;
            }
            if self.tag_fire.contains(self.block_id(existing)) {
                return false;
            }
            let can_replace_existing = self.is_replaceable(existing)
                || self.is_air(existing)
                || self.block_id(existing) == self.bid_sculk_vein
                || (self.is_water(existing) && fl.is_source);
            if !can_replace_existing {
                return false;
            }
        } else {
            // Default MultifaceSpreaderConfig: canReplace = isAir || is(placer) || (is(water) && isSource)
            let ok = self.is_air(existing)
                || self.block_id(existing) == placer_block_id
                || (self.is_water(existing) && self.fluid(existing).is_source);
            if !ok {
                return false;
            }
        }
        self.is_valid_multiface_placement(placer_block_id, existing, region, place_pos, place_dir)
    }

    /// `SculkVeinBlock.regrow` (`features.md` §4.41).
    pub fn sculk_vein_regrow(
        &self,
        region: &mut GenerationRegion,
        pos: BlockPos,
        cur_state: u32,
        facings_mask: u8,
    ) -> bool {
        let mut new_state = self.sid_sculk_vein;
        let mut any = false;
        for dir in Direction::ALL {
            if (facings_mask & (1 << dir.ordinal())) != 0 {
                let against = region.get_block(pos.relative(dir));
                if self.can_attach_multiface_to(against, dir) {
                    new_state = self.try_set_property(new_state, dir.name(), "true");
                    any = true;
                }
            }
        }
        if !any {
            return false;
        }
        if !self.fluid(cur_state).is_empty() {
            new_state = self.copy_waterlogged(new_state, true);
        }
        region.set_block(pos, new_state, 3);
        true
    }

    /// `SculkVeinBlock.onDischarged` (`features.md` §4.41).
    pub fn sculk_vein_on_discharged(
        &self,
        region: &mut GenerationRegion,
        pos: BlockPos,
        state: u32,
    ) {
        if self.block_id(state) != self.bid_sculk_vein {
            return;
        }
        let mut updated = state;
        for dir in Direction::ALL {
            if self.multiface_has_face(updated, dir)
                && self.block_id(region.get_block(pos.relative(dir))) == self.bid_sculk
            {
                updated = self.try_set_property(updated, dir.name(), "false");
            }
        }
        if updated != state {
            if self.multiface_available_faces(updated) == 0 {
                let replacement = if self.fluid(state).is_empty() {
                    self.sid_air
                } else {
                    self.sid_water
                };
                region.set_block(pos, replacement, 3);
            } else {
                region.set_block(pos, updated, 3);
            }
        }
    }

    /// `SculkVeinBlock.hasSubstrateAccess`: true iff `state` is `sculk_vein` and any attached face
    /// borders a block in `#minecraft:sculk_replaceable` (NOT `_world_gen`!).
    pub fn sculk_vein_has_substrate_access(
        &self,
        region: &GenerationRegion,
        state: u32,
        pos: BlockPos,
    ) -> bool {
        if self.block_id(state) != self.bid_sculk_vein {
            return false;
        }
        for dir in Direction::ALL {
            if self.multiface_has_face(state, dir) {
                let against = region.get_block(pos.relative(dir));
                if self.tag_sculk_replaceable.contains(self.block_id(against)) {
                    return true;
                }
            }
        }
        false
    }
}

#[inline]
fn pos_state_above(region: &GenerationRegion, pos: BlockPos) -> u32 {
    region.get_block(pos.above())
}
