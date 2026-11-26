//! All 58 feature types registered in `minecraft:worldgen/feature_type` (`features.md` §4),
//! plus trait hooks for `tree`, `fallen_tree` decorators, `fossil`/`template` placement, and
//! structure-piece interleaving.

pub mod combinators;
pub mod ice_caves;
pub mod lakes_mushrooms_patches;
pub mod ores_blobs;
pub mod plants_nether_sculk;
pub mod structures_end;
pub mod tree;
pub mod trees_bamboo;

use std::fmt;
use std::sync::Arc;

use zenith_blocks::{Direction, FluidState};
use zenith_codec::json::Json;
use zenith_math::RandomSource;
use zenith_registry::Identifier;
use zenith_worldgen::{BiomeId, CaveSurface};

use crate::block_ops::BlockBitSet;
use crate::data::BiomeFeatureSettings;
use crate::placement::{PlacedFeature, PlacedFeatureId, place_feature_with_modifiers};
use crate::providers::{
    BlockPredicate, BlockReplacement, BlockStateProvider, FloatProvider, IntProvider, WeightedList,
};
use crate::region::GenerationRegion;
use crate::rng::{BlockPos, Rotation, WorldgenRandom};

// ---------------------------------------------------------------------------
// External hooks (trees, templates, structures)
// ---------------------------------------------------------------------------

/// Hook for `minecraft:tree` placement and `fallen_tree` decorators
/// (`docs/specs/worldgen/features-trees.md`). The default is [`tree::VanillaTreeHook`].
pub trait TreeFeatureHook: fmt::Debug + Send + Sync {
    /// Places a decoded `minecraft:tree` feature.
    fn place_tree(
        &self,
        ctx: &FeatureContext<'_>,
        config: &tree::TreeConfig,
        region: &mut GenerationRegion,
        rng: &mut WorldgenRandom,
        origin: BlockPos,
    ) -> bool;

    /// Runs `fallen_tree` stump or log decorators on `log_positions` (iterated in Java `HashSet` order).
    fn run_tree_decorators(
        &self,
        ctx: &FeatureContext<'_>,
        decorators: &[tree::TreeDecorator],
        region: &mut GenerationRegion,
        rng: &mut WorldgenRandom,
        log_positions: &[BlockPos],
    );
}

/// No-op tree hook (every tree placement fails; useful for isolating other features).
#[derive(Debug, Default)]
pub struct NoopTreeHook;

impl TreeFeatureHook for NoopTreeHook {
    fn place_tree(
        &self,
        _ctx: &FeatureContext<'_>,
        _config: &tree::TreeConfig,
        _region: &mut GenerationRegion,
        _rng: &mut WorldgenRandom,
        _origin: BlockPos,
    ) -> bool {
        false
    }

    fn run_tree_decorators(
        &self,
        _ctx: &FeatureContext<'_>,
        _decorators: &[tree::TreeDecorator],
        _region: &mut GenerationRegion,
        _rng: &mut WorldgenRandom,
        _log_positions: &[BlockPos],
    ) {
    }
}

/// Hook for NBT structure-template placement (`minecraft:fossil` and `minecraft:template`),
/// which lives in the structures crate.
pub trait TemplateFeatureHook: fmt::Debug + Send + Sync {
    /// Places a `minecraft:fossil` feature (`features.md` §4.42).
    fn place_fossil(
        &self,
        config: &FossilConfig,
        region: &mut GenerationRegion,
        rng: &mut WorldgenRandom,
        origin: BlockPos,
    ) -> bool;

    /// Places a `minecraft:template` feature (`features.md` §4.43).
    fn place_template(
        &self,
        config: &TemplateConfig,
        region: &mut GenerationRegion,
        rng: &mut WorldgenRandom,
        origin: BlockPos,
    ) -> bool;
}

/// Default no-op template hook (advances RNG for rotation/template selection only if needed;
/// returns `false` when no template store is plugged in).
#[derive(Debug, Default)]
pub struct NoopTemplateHook;

impl TemplateFeatureHook for NoopTemplateHook {
    fn place_fossil(
        &self,
        config: &FossilConfig,
        _region: &mut GenerationRegion,
        rng: &mut WorldgenRandom,
        _origin: BlockPos,
    ) -> bool {
        let _rot = Rotation::get_random(rng);
        let _idx = rng.next_int_bounded(config.fossil_structures.len() as i32);
        false
    }

    fn place_template(
        &self,
        config: &TemplateConfig,
        _region: &mut GenerationRegion,
        rng: &mut WorldgenRandom,
        _origin: BlockPos,
    ) -> bool {
        let entry = config.templates.get_random_or_throw(rng);
        let _rot = entry.rotations[rng.next_int_bounded(entry.rotations.len() as i32) as usize];
        false
    }
}

/// Hook for structure piece placement interleaved before each decoration step (§1.4 step 11.1).
pub trait StructureInterleaveHook: fmt::Debug + Send + Sync {
    /// Places all structure pieces intersecting `(center_x, center_z)` for decoration `step_index`.
    fn place_structures_for_step(
        &self,
        step_index: usize,
        decoration_seed: i64,
        region: &mut GenerationRegion,
        rng: &mut WorldgenRandom,
    );
}

/// Default no-op structure hook (equivalent to `generate-structures = false`).
#[derive(Debug, Default)]
pub struct NoopStructureHook;

impl StructureInterleaveHook for NoopStructureHook {
    fn place_structures_for_step(
        &self,
        _step_index: usize,
        _decoration_seed: i64,
        _region: &mut GenerationRegion,
        _rng: &mut WorldgenRandom,
    ) {
    }
}

// ---------------------------------------------------------------------------
// Runtime feature context
// ---------------------------------------------------------------------------

/// Shared context passed through placement modifiers and feature `place` methods.
pub struct FeatureContext<'a> {
    /// Per-biome feature settings indexed by `BiomeId.0`.
    pub biome_settings: &'a [BiomeFeatureSettings],
    /// Tree hook.
    pub tree_hook: &'a dyn TreeFeatureHook,
    /// Template hook.
    pub template_hook: &'a dyn TemplateFeatureHook,
}

impl fmt::Debug for FeatureContext<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FeatureContext")
            .field("biome_count", &self.biome_settings.len())
            .finish()
    }
}

impl FeatureContext<'_> {
    /// Returns `true` if `biome`'s generation settings contain `feature_id`.
    #[inline]
    pub fn biome_has_feature(&self, biome: BiomeId, feature_id: PlacedFeatureId) -> bool {
        self.biome_settings
            .get(biome.0 as usize)
            .is_some_and(|s| s.has_feature(feature_id))
    }

    /// Returns biome climate/feature settings for `biome`.
    #[inline]
    pub fn biome(&self, biome: BiomeId) -> Option<&BiomeFeatureSettings> {
        self.biome_settings.get(biome.0 as usize)
    }

    /// Places a sub-feature (`topFeature = None`, `biome_check = false`).
    #[inline]
    pub fn place_sub_feature(
        &self,
        placed: &PlacedFeature,
        region: &mut GenerationRegion,
        rng: &mut WorldgenRandom,
        origin: BlockPos,
    ) -> bool {
        place_feature_with_modifiers(placed, self, region, rng, origin, false)
    }
}

// ---------------------------------------------------------------------------
// Feature config structs for larger features
// ---------------------------------------------------------------------------

/// Shared config for `minecraft:ore` and `minecraft:scattered_ore` (§4.8, §4.9).
#[derive(Debug, Clone)]
pub struct OreConfig {
    /// Target replacement rules.
    pub targets: Vec<BlockReplacement>,
    /// Vein size (`0..=64`).
    pub size: i32,
    /// Air-exposure discard probability (`0.0..=1.0`).
    pub discard_chance_on_air_exposure: f32,
}

/// One layer of `minecraft:block_column` (§4.15).
#[derive(Debug, Clone)]
pub struct BlockColumnLayer {
    /// Layer height provider.
    pub height: IntProvider,
    /// Layer block state provider.
    pub provider: Arc<BlockStateProvider>,
}

/// Config for `minecraft:geode` (§4.29).
#[derive(Debug, Clone)]
pub struct GeodeConfig {
    /// Filling state provider.
    pub filling_provider: Arc<BlockStateProvider>,
    /// Inner layer state provider.
    pub inner_layer_provider: Arc<BlockStateProvider>,
    /// Alternate inner layer state provider.
    pub alternate_inner_layer_provider: Arc<BlockStateProvider>,
    /// Middle layer state provider.
    pub middle_layer_provider: Arc<BlockStateProvider>,
    /// Outer layer state provider.
    pub outer_layer_provider: Arc<BlockStateProvider>,
    /// Candidate crystal placements.
    pub inner_placements: Vec<u32>,
    /// Blocks that cannot be replaced.
    pub cannot_replace: BlockBitSet,
    /// Blocks counting toward `invalid_blocks_threshold`.
    pub invalid_blocks: BlockBitSet,
    /// Layer thresholds (`filling`, `inner_layer`, `middle_layer`, `outer_layer`).
    pub filling: f64,
    pub inner_layer: f64,
    pub middle_layer: f64,
    pub outer_layer: f64,
    /// Crack settings.
    pub generate_crack_chance: f64,
    pub base_crack_size: f64,
    pub crack_point_offset: i32,
    /// Placement chances.
    pub use_potential_placements_chance: f64,
    pub use_alternate_layer0_chance: f64,
    pub placements_require_layer0_alternate: bool,
    /// Geometry providers.
    pub outer_wall_distance: IntProvider,
    pub distribution_points: IntProvider,
    pub point_offset: IntProvider,
    pub min_gen_offset: i32,
    pub max_gen_offset: i32,
    pub noise_multiplier: f64,
    pub invalid_blocks_threshold: i32,
}

/// Config for `minecraft:large_dripstone` (§4.30).
#[derive(Debug, Clone)]
pub struct LargeDripstoneConfig {
    pub replaceable_blocks: BlockBitSet,
    pub floor_to_ceiling_search_range: i32,
    pub column_radius: IntProvider,
    pub height_scale: FloatProvider,
    pub max_column_radius_to_cave_height_ratio: f32,
    pub stalactite_bluntness: FloatProvider,
    pub stalagmite_bluntness: FloatProvider,
    pub wind_speed: FloatProvider,
    pub min_radius_for_wind: i32,
    pub min_bluntness_for_wind: f32,
}

/// Config for `minecraft:root_system` (§4.31).
#[derive(Debug, Clone)]
pub struct RootSystemConfig {
    pub feature: Arc<PlacedFeature>,
    pub required_vertical_space_for_tree: i32,
    pub level_test_distance: i32,
    pub max_level_deviation: i32,
    pub root_radius: i32,
    pub root_replaceable: BlockBitSet,
    pub root_state_provider: Arc<BlockStateProvider>,
    pub root_placement_attempts: i32,
    pub root_column_max_height: i32,
    pub hanging_root_radius: i32,
    pub hanging_roots_vertical_span: i32,
    pub hanging_root_state_provider: Arc<BlockStateProvider>,
    pub hanging_root_placement_attempts: i32,
    pub allowed_vertical_water_for_tree: i32,
    pub allowed_tree_position: BlockPredicate,
}

/// Config for `minecraft:speleothem` (§4.32).
#[derive(Debug, Clone)]
pub struct SpeleothemConfig {
    pub base_block_id: u32,
    pub base_default_state: u32,
    pub pointed_block_id: u32,
    pub pointed_default_state: u32,
    pub replaceable_blocks: BlockBitSet,
    pub chance_of_taller_generation: f32,
    pub chance_of_directional_spread: f32,
    pub chance_of_spread_radius2: f32,
    pub chance_of_spread_radius3: f32,
}

/// Config for `minecraft:speleothem_cluster` (§4.33).
#[derive(Debug, Clone)]
pub struct SpeleothemClusterConfig {
    pub base_block_id: u32,
    pub base_default_state: u32,
    pub pointed_block_id: u32,
    pub pointed_default_state: u32,
    pub replaceable_blocks: BlockBitSet,
    pub floor_to_ceiling_search_range: i32,
    pub height: IntProvider,
    pub radius: IntProvider,
    pub max_stalagmite_stalactite_height_diff: i32,
    pub height_deviation: i32,
    pub speleothem_block_layer_thickness: IntProvider,
    pub density: FloatProvider,
    pub wetness: FloatProvider,
    pub chance_of_speleothem_at_max_distance_from_center: f32,
    pub max_distance_from_edge_affecting_chance_of_speleothem: i32,
    pub max_distance_from_center_affecting_height_bias: i32,
}

/// Config for `minecraft:stepped_column_cluster` (§4.38).
#[derive(Debug, Clone)]
pub struct SteppedColumnClusterConfig {
    pub block: Arc<BlockStateProvider>,
    pub continue_through: BlockPredicate,
    pub can_replace: BlockPredicate,
    pub cannot_place_on: BlockBitSet,
    pub cluster_reach: IntProvider,
    pub column_count: IntProvider,
    pub column_reach: IntProvider,
    pub height: IntProvider,
}

/// Config for `minecraft:fossil` (§4.42).
#[derive(Debug, Clone)]
pub struct FossilConfig {
    pub fossil_structures: Vec<Identifier>,
    pub overlay_structures: Vec<Identifier>,
    pub fossil_processors: Json,
    pub overlay_processors: Json,
    pub max_empty_corners_allowed: i32,
}

/// Entry in `TemplateConfig::templates`.
#[derive(Debug, Clone)]
pub struct TemplateEntry {
    pub id: Identifier,
    pub rotations: Vec<Rotation>,
}

/// Config for `minecraft:template` (§4.43).
#[derive(Debug, Clone)]
pub struct TemplateConfig {
    pub templates: WeightedList<TemplateEntry>,
    pub processors: Option<Json>,
}

/// Explicit pillar definition in `minecraft:end_spike` (§4.50).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndSpike {
    pub center_x: i32,
    pub center_z: i32,
    pub radius: i32,
    pub height: i32,
    pub guarded: bool,
}

/// Shared config for `minecraft:huge_brown_mushroom` and `minecraft:huge_red_mushroom` (§4.52, §4.53).
#[derive(Debug, Clone)]
pub struct HugeMushroomConfig {
    pub cap_provider: Arc<BlockStateProvider>,
    pub stem_provider: Arc<BlockStateProvider>,
    pub foliage_radius: i32,
    pub can_place_on: BlockPredicate,
}

/// Shared config for `minecraft:vegetation_patch` and `minecraft:waterlogged_vegetation_patch` (§4.55, §4.56).
#[derive(Debug, Clone)]
pub struct VegetationPatchConfig {
    pub replaceable: BlockBitSet,
    pub ground_state: Arc<BlockStateProvider>,
    pub vegetation_feature: Arc<PlacedFeature>,
    pub surface: CaveSurface,
    pub depth: IntProvider,
    pub extra_bottom_block_chance: f32,
    pub vertical_range: i32,
    pub vegetation_chance: f32,
    pub xz_radius: IntProvider,
    pub extra_edge_column_chance: f32,
}

// ---------------------------------------------------------------------------
// All 58 feature types
// ---------------------------------------------------------------------------

/// All 58 registered 26.3 feature types (`minecraft:worldgen/feature_type`, `features.md` §4).
#[derive(Debug, Clone)]
pub enum Feature {
    // Group A: combinators (§4.1–§4.7)
    NoOp,
    RandomBooleanSelector {
        feature_true: Arc<PlacedFeature>,
        feature_false: Arc<PlacedFeature>,
    },
    RandomSelector {
        features: Vec<(Arc<PlacedFeature>, f32)>,
        default: Arc<PlacedFeature>,
    },
    SimpleRandomSelector {
        features: Vec<Arc<PlacedFeature>>,
    },
    WeightedRandomSelector {
        features: WeightedList<Arc<PlacedFeature>>,
    },
    Overlay {
        features: Vec<Arc<PlacedFeature>>,
    },
    Sequence {
        features: Vec<Arc<PlacedFeature>>,
    },

    // Group B: ores, blobs, disks, columns, simple blocks (§4.8–§4.21)
    Ore(OreConfig),
    ScatteredOre(OreConfig),
    ReplaceSingleBlock {
        targets: Vec<BlockReplacement>,
    },
    NetherrackReplaceBlobs {
        target_block_id: u32,
        state: u32,
        radius: IntProvider,
    },
    Disk {
        state_provider: Arc<BlockStateProvider>,
        target: BlockPredicate,
        radius: IntProvider,
        half_height: i32,
    },
    SimpleBlock {
        to_place: Arc<BlockStateProvider>,
        schedule_tick: bool,
    },
    BlockBlob {
        state: u32,
        can_place_on: BlockPredicate,
    },
    BlockColumn {
        layers: Vec<BlockColumnLayer>,
        direction: Direction,
        allowed_placement: BlockPredicate,
        prioritize_tip: bool,
    },
    BlockPile {
        state_provider: Arc<BlockStateProvider>,
    },
    FillLayer {
        height: i32,
        state: u32,
    },
    ProjectedRandomPatchySquare {
        block: Arc<BlockStateProvider>,
        project_through: BlockPredicate,
        size: IntProvider,
        max_projection_height: i32,
    },
    RandomNeighborSpread {
        block: Arc<BlockStateProvider>,
        accepted_neighbors: BlockBitSet,
        can_replace: BlockPredicate,
        attempts: IntProvider,
        xz_offset: IntProvider,
        y_offset: IntProvider,
    },
    SingleBlockPillar {
        block: Arc<BlockStateProvider>,
        can_replace: BlockPredicate,
        direction: Direction,
        chance_to_continue: f32,
        cap_feature: Option<Arc<PlacedFeature>>,
    },
    SpringFeature {
        fluid_state: FluidState,
        legacy_block_state: u32,
        requires_block_below: bool,
        rock_count: i32,
        hole_count: i32,
        valid_blocks: BlockBitSet,
    },

    // Group C: bamboo, trees, fallen trees (§4.22–§4.24)
    Bamboo {
        probability: f32,
    },
    Tree {
        config: Box<tree::TreeConfig>,
    },
    FallenTree {
        trunk_provider: Arc<BlockStateProvider>,
        log_length: IntProvider,
        stump_decorators: Vec<tree::TreeDecorator>,
        log_decorators: Vec<tree::TreeDecorator>,
    },

    // Group D: ice, snow, spikes, geodes, speleothems (§4.25–§4.33)
    Iceberg {
        state: u32,
    },
    FreezeTopLayer,
    BlueIce,
    Spike {
        state: u32,
        can_place_on: BlockPredicate,
        can_replace: BlockPredicate,
    },
    Geode(Box<GeodeConfig>),
    LargeDripstone(Box<LargeDripstoneConfig>),
    RootSystem(Box<RootSystemConfig>),
    Speleothem(Box<SpeleothemConfig>),
    SpeleothemCluster(Box<SpeleothemClusterConfig>),

    // Group E: plants, corals, nether, underwater magma, sculk (§4.34–§4.41)
    ChorusPlant,
    Vines,
    CoralClaw {
        feature: Arc<PlacedFeature>,
    },
    CoralTree {
        feature: Arc<PlacedFeature>,
    },
    SteppedColumnCluster(Box<SteppedColumnClusterConfig>),
    DeltaFeature {
        contents: u32,
        rim: u32,
        size: IntProvider,
        rim_size: IntProvider,
    },
    UnderwaterMagma {
        floor_search_range: i32,
        placement_radius_around_floor: i32,
        placement_probability_per_valid_position: f32,
    },
    SculkPatch {
        charge_count: i32,
        amount_per_charge: i32,
        spread_attempts: i32,
        growth_rounds: i32,
        spread_rounds: i32,
    },

    // Group F & G: templates, fossils, chests, monster rooms, end (§4.42–§4.50)
    Fossil(Box<FossilConfig>),
    Template(Box<TemplateConfig>),
    BonusChest,
    MonsterRoom,
    EndGateway {
        exit: Option<BlockPos>,
        exact: bool,
    },
    EndIsland,
    EndPlatform,
    EndPodium {
        active: bool,
    },
    EndSpike {
        spikes: Vec<EndSpike>,
        crystal_invulnerable: bool,
        crystal_beam_target: Option<BlockPos>,
    },

    // Group H: lakes, mushrooms, fungi, vegetation patches, void, multiface (§4.51–§4.58)
    Lake {
        fluid: Arc<BlockStateProvider>,
        barrier: Arc<BlockStateProvider>,
        can_place_feature: BlockPredicate,
        can_replace_with_air_or_fluid: BlockPredicate,
        can_replace_with_barrier: BlockPredicate,
    },
    HugeBrownMushroom(Box<HugeMushroomConfig>),
    HugeRedMushroom(Box<HugeMushroomConfig>),
    HugeFungus {
        valid_base_block_id: u32,
        stem_state: u32,
        hat_state: u32,
        decor_state: u32,
        replaceable_blocks: BlockPredicate,
        planted: bool,
    },
    VegetationPatch(Box<VegetationPatchConfig>),
    WaterloggedVegetationPatch(Box<VegetationPatchConfig>),
    VoidStartPlatform,
    MultifaceGrowth {
        block_id: u32,
        search_range: i32,
        can_place_on_floor: bool,
        can_place_on_ceiling: bool,
        can_place_on_wall: bool,
        chance_of_spreading: f32,
        can_be_placed_on: BlockBitSet,
    },
}

impl Feature {
    /// Executes `Feature.place(level, generator, random, origin)`.
    pub fn place(
        &self,
        ctx: &FeatureContext<'_>,
        region: &mut GenerationRegion,
        rng: &mut WorldgenRandom,
        origin: BlockPos,
    ) -> bool {
        match self {
            // Group A
            Self::NoOp => true,
            Self::RandomBooleanSelector {
                feature_true,
                feature_false,
            } => combinators::place_random_boolean_selector(
                ctx,
                region,
                rng,
                origin,
                feature_true,
                feature_false,
            ),
            Self::RandomSelector { features, default } => {
                combinators::place_random_selector(ctx, region, rng, origin, features, default)
            }
            Self::SimpleRandomSelector { features } => {
                combinators::place_simple_random_selector(ctx, region, rng, origin, features)
            }
            Self::WeightedRandomSelector { features } => {
                combinators::place_weighted_random_selector(ctx, region, rng, origin, features)
            }
            Self::Overlay { features } => {
                combinators::place_overlay(ctx, region, rng, origin, features)
            }
            Self::Sequence { features } => {
                combinators::place_sequence(ctx, region, rng, origin, features)
            }

            // Group B
            Self::Ore(cfg) => ores_blobs::place_ore(region, rng, origin, cfg),
            Self::ScatteredOre(cfg) => ores_blobs::place_scattered_ore(region, rng, origin, cfg),
            Self::ReplaceSingleBlock { targets } => {
                ores_blobs::place_replace_single_block(region, rng, origin, targets)
            }
            Self::NetherrackReplaceBlobs {
                target_block_id,
                state,
                radius,
            } => ores_blobs::place_replace_blobs(
                region,
                rng,
                origin,
                *target_block_id,
                *state,
                radius,
            ),
            Self::Disk {
                state_provider,
                target,
                radius,
                half_height,
            } => ores_blobs::place_disk(
                region,
                rng,
                origin,
                state_provider,
                target,
                radius,
                *half_height,
            ),
            Self::SimpleBlock {
                to_place,
                schedule_tick,
            } => ores_blobs::place_simple_block(region, rng, origin, to_place, *schedule_tick),
            Self::BlockBlob {
                state,
                can_place_on,
            } => ores_blobs::place_block_blob(region, rng, origin, *state, can_place_on),
            Self::BlockColumn {
                layers,
                direction,
                allowed_placement,
                prioritize_tip,
            } => ores_blobs::place_block_column(
                region,
                rng,
                origin,
                layers,
                *direction,
                allowed_placement,
                *prioritize_tip,
            ),
            Self::BlockPile { state_provider } => {
                ores_blobs::place_block_pile(region, rng, origin, state_provider)
            }
            Self::FillLayer { height, state } => {
                ores_blobs::place_fill_layer(region, origin, *height, *state)
            }
            Self::ProjectedRandomPatchySquare {
                block,
                project_through,
                size,
                max_projection_height,
            } => ores_blobs::place_projected_random_patchy_square(
                region,
                rng,
                origin,
                block,
                project_through,
                size,
                *max_projection_height,
            ),
            Self::RandomNeighborSpread {
                block,
                accepted_neighbors,
                can_replace,
                attempts,
                xz_offset,
                y_offset,
            } => ores_blobs::place_random_neighbor_spread(
                region,
                rng,
                origin,
                block,
                accepted_neighbors,
                can_replace,
                attempts,
                xz_offset,
                y_offset,
            ),
            Self::SingleBlockPillar {
                block,
                can_replace,
                direction,
                chance_to_continue,
                cap_feature,
            } => ores_blobs::place_single_block_pillar(
                ctx,
                region,
                rng,
                origin,
                block,
                can_replace,
                *direction,
                *chance_to_continue,
                cap_feature.as_deref(),
            ),
            Self::SpringFeature {
                fluid_state,
                legacy_block_state,
                requires_block_below,
                rock_count,
                hole_count,
                valid_blocks,
            } => ores_blobs::place_spring(
                region,
                origin,
                *fluid_state,
                *legacy_block_state,
                *requires_block_below,
                *rock_count,
                *hole_count,
                valid_blocks,
            ),

            // Group C
            Self::Bamboo { probability } => {
                trees_bamboo::place_bamboo(region, rng, origin, *probability)
            }
            Self::Tree { config } => ctx.tree_hook.place_tree(ctx, config, region, rng, origin),
            Self::FallenTree {
                trunk_provider,
                log_length,
                stump_decorators,
                log_decorators,
            } => trees_bamboo::place_fallen_tree(
                ctx,
                region,
                rng,
                origin,
                trunk_provider,
                log_length,
                stump_decorators,
                log_decorators,
            ),

            // Group D
            Self::Iceberg { state } => ice_caves::place_iceberg(region, rng, origin, *state),
            Self::FreezeTopLayer => ice_caves::place_freeze_top_layer(ctx, region, origin),
            Self::BlueIce => ice_caves::place_blue_ice(region, rng, origin),
            Self::Spike {
                state,
                can_place_on,
                can_replace,
            } => ice_caves::place_spike(region, rng, origin, *state, can_place_on, can_replace),
            Self::Geode(cfg) => ice_caves::place_geode(region, rng, origin, cfg),
            Self::LargeDripstone(cfg) => ice_caves::place_large_dripstone(region, rng, origin, cfg),
            Self::RootSystem(cfg) => ice_caves::place_root_system(ctx, region, rng, origin, cfg),
            Self::Speleothem(cfg) => ice_caves::place_speleothem(region, rng, origin, cfg),
            Self::SpeleothemCluster(cfg) => {
                ice_caves::place_speleothem_cluster(region, rng, origin, cfg)
            }

            // Group E
            Self::ChorusPlant => plants_nether_sculk::place_chorus_plant(region, rng, origin),
            Self::Vines => plants_nether_sculk::place_vines(region, origin),
            Self::CoralClaw { feature } => {
                plants_nether_sculk::place_coral_claw(ctx, region, rng, origin, feature)
            }
            Self::CoralTree { feature } => {
                plants_nether_sculk::place_coral_tree(ctx, region, rng, origin, feature)
            }
            Self::SteppedColumnCluster(cfg) => {
                plants_nether_sculk::place_stepped_column_cluster(region, rng, origin, cfg)
            }
            Self::DeltaFeature {
                contents,
                rim,
                size,
                rim_size,
            } => plants_nether_sculk::place_delta_feature(
                region, rng, origin, *contents, *rim, size, rim_size,
            ),
            Self::UnderwaterMagma {
                floor_search_range,
                placement_radius_around_floor,
                placement_probability_per_valid_position,
            } => plants_nether_sculk::place_underwater_magma(
                region,
                rng,
                origin,
                *floor_search_range,
                *placement_radius_around_floor,
                *placement_probability_per_valid_position,
            ),
            Self::SculkPatch {
                charge_count,
                amount_per_charge,
                spread_attempts,
                growth_rounds,
                spread_rounds,
            } => plants_nether_sculk::place_sculk_patch(
                region,
                rng,
                origin,
                *charge_count,
                *amount_per_charge,
                *spread_attempts,
                *growth_rounds,
                *spread_rounds,
            ),

            // Group F & G
            Self::Fossil(cfg) => ctx.template_hook.place_fossil(cfg, region, rng, origin),
            Self::Template(cfg) => ctx.template_hook.place_template(cfg, region, rng, origin),
            Self::BonusChest => structures_end::place_bonus_chest(region, rng, origin),
            Self::MonsterRoom => structures_end::place_monster_room(region, rng, origin),
            Self::EndGateway { exit, exact } => {
                structures_end::place_end_gateway(region, origin, *exit, *exact)
            }
            Self::EndIsland => structures_end::place_end_island(region, rng, origin),
            Self::EndPlatform => structures_end::place_end_platform(region, origin),
            Self::EndPodium { active } => structures_end::place_end_podium(region, origin, *active),
            Self::EndSpike {
                spikes,
                crystal_invulnerable,
                crystal_beam_target,
            } => structures_end::place_end_spike(
                region,
                rng,
                origin,
                spikes,
                *crystal_invulnerable,
                *crystal_beam_target,
            ),

            // Group H
            Self::Lake {
                fluid,
                barrier,
                can_place_feature,
                can_replace_with_air_or_fluid,
                can_replace_with_barrier,
            } => lakes_mushrooms_patches::place_lake(
                ctx,
                region,
                rng,
                origin,
                fluid,
                barrier,
                can_place_feature,
                can_replace_with_air_or_fluid,
                can_replace_with_barrier,
            ),
            Self::HugeBrownMushroom(cfg) => {
                lakes_mushrooms_patches::place_huge_mushroom(region, rng, origin, cfg, false)
            }
            Self::HugeRedMushroom(cfg) => {
                lakes_mushrooms_patches::place_huge_mushroom(region, rng, origin, cfg, true)
            }
            Self::HugeFungus {
                valid_base_block_id,
                stem_state,
                hat_state,
                decor_state,
                replaceable_blocks,
                planted,
            } => lakes_mushrooms_patches::place_huge_fungus(
                region,
                rng,
                origin,
                *valid_base_block_id,
                *stem_state,
                *hat_state,
                *decor_state,
                replaceable_blocks,
                *planted,
            ),
            Self::VegetationPatch(cfg) => lakes_mushrooms_patches::place_vegetation_patch(
                ctx, region, rng, origin, cfg, false,
            ),
            Self::WaterloggedVegetationPatch(cfg) => {
                lakes_mushrooms_patches::place_vegetation_patch(ctx, region, rng, origin, cfg, true)
            }
            Self::VoidStartPlatform => {
                lakes_mushrooms_patches::place_void_start_platform(region, origin)
            }
            Self::MultifaceGrowth {
                block_id,
                search_range,
                can_place_on_floor,
                can_place_on_ceiling,
                can_place_on_wall,
                chance_of_spreading,
                can_be_placed_on,
            } => lakes_mushrooms_patches::place_multiface_growth(
                region,
                rng,
                origin,
                *block_id,
                *search_range,
                *can_place_on_floor,
                *can_place_on_ceiling,
                *can_place_on_wall,
                *chance_of_spreading,
                can_be_placed_on,
            ),
        }
    }
}
