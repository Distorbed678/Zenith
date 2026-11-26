//! `PlacedFeature`, depth-first `FeaturePlacer`, and all 18 placement modifiers
//! (`docs/specs/worldgen/features.md` §2).

use std::sync::{Arc, LazyLock};

use zenith_blocks::Direction;
use zenith_math::RandomSource;
use zenith_math::noise::{SimplexNoise, biome_info_noise};
use zenith_registry::Identifier;
use zenith_worldgen::{HeightmapKind, WorldGenerationContext};

use crate::features::{Feature, FeatureContext};
use crate::providers::{BlockPredicate, HeightProvider, IntProvider};
use crate::region::GenerationRegion;
use crate::rng::{BlockPos, WorldgenRandom};

static BIOME_INFO_NOISE: LazyLock<SimplexNoise> = LazyLock::new(biome_info_noise);

/// Unique identity handle for a [`PlacedFeature`] (reproducing Java object reference identity
/// in `FeatureSorter`'s `Reference2IntMap<PlacedFeature>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PlacedFeatureId(pub u32);

/// A placed feature (`minecraft:worldgen/placed_feature`, §2.1).
#[derive(Debug, Clone)]
pub struct PlacedFeature {
    /// Unique reference-identity ID assigned when loaded/constructed.
    pub id: PlacedFeatureId,
    /// Optional registry identifier (for diagnostics).
    pub name: Option<Identifier>,
    /// Underlying configured feature.
    pub feature: Arc<Feature>,
    /// Ordered chain of placement modifiers.
    pub placement: Vec<PlacementModifier>,
}

/// All 18 `minecraft:worldgen/placement_modifier_type` implementations (`features.md` §2.2).
#[derive(Debug, Clone)]
pub enum PlacementModifier {
    /// 1. `minecraft:biome` (`BiomeFilter`).
    Biome,
    /// 2. `minecraft:block_predicate_filter`.
    BlockPredicateFilter(BlockPredicate),
    /// 3. `minecraft:count`.
    Count(IntProvider),
    /// 4. `minecraft:count_on_every_layer` (deprecated).
    CountOnEveryLayer(IntProvider),
    /// 5. `minecraft:cuboid` (new in 26.3).
    Cuboid {
        /// Horizontal size provider in `[1, 16]`.
        xz_size: IntProvider,
        /// Vertical size provider in `[1, 16]`.
        y_size: IntProvider,
        /// Whether to include edges & corners (default `true`).
        include_edges: bool,
        /// Whether to include strictly interior cells (default `true`).
        include_interior: bool,
    },
    /// 6. `minecraft:environment_scan`.
    EnvironmentScan {
        /// Vertical search direction (`Up` or `Down`).
        direction_of_search: Direction,
        /// Target stop condition.
        target_condition: BlockPredicate,
        /// Allowed search continuation condition (default `True`).
        allowed_search_condition: BlockPredicate,
        /// Maximum steps (`1..=32`).
        max_steps: i32,
    },
    /// 7. `minecraft:fixed_placement`.
    FixedPlacement(Vec<BlockPos>),
    /// 8. `minecraft:height_range`.
    HeightRange(HeightProvider),
    /// 9. `minecraft:heightmap`.
    Heightmap(HeightmapKind),
    /// 10. `minecraft:in_square`.
    InSquare,
    /// 11. `minecraft:noise_based_count`.
    NoiseBasedCount {
        /// Ratio multiplier.
        noise_to_count_ratio: i32,
        /// Coordinate divisor.
        noise_factor: f64,
        /// Noise offset (default `0.0`).
        noise_offset: f64,
    },
    /// 12. `minecraft:noise_threshold_count`.
    NoiseThresholdCount {
        /// Noise threshold.
        noise_level: f64,
        /// Count when `noise < noise_level`.
        below_noise: i32,
        /// Count when `noise >= noise_level`.
        above_noise: i32,
    },
    /// 13. `minecraft:offset` (replaces pre-26.3 `random_offset`).
    Offset {
        /// X offset provider (`-16..=16`).
        x: IntProvider,
        /// Y offset provider (`-16..=16`).
        y: IntProvider,
        /// Z offset provider (`-16..=16`).
        z: IntProvider,
    },
    /// 14. `minecraft:random_chance` (new in 26.3).
    RandomChance(f32),
    /// 15. `minecraft:randomly_selected` (new in 26.3).
    RandomlySelected(Vec<PlacementModifier>),
    /// 16. `minecraft:rarity_filter`.
    RarityFilter(i32),
    /// 17. `minecraft:surface_relative_threshold_filter`.
    SurfaceRelativeThresholdFilter {
        /// Heightmap type.
        heightmap: HeightmapKind,
        /// Minimum inclusive offset (default `i32::MIN`).
        min_inclusive: i32,
        /// Maximum inclusive offset (default `i32::MAX`).
        max_inclusive: i32,
    },
    /// 18. `minecraft:surface_water_depth_filter`.
    SurfaceWaterDepthFilter(i32),
}

impl PlacementModifier {
    /// Appends emitted positions for `origin` into `out` (`features.md` §2.2).
    pub fn modify(
        &self,
        ctx: &FeatureContext<'_>,
        region: &GenerationRegion,
        rng: &mut WorldgenRandom,
        origin: BlockPos,
        top_feature: Option<PlacedFeatureId>,
        out: &mut Vec<BlockPos>,
    ) {
        match self {
            Self::Biome => {
                let top_id = top_feature.expect(
                    "minecraft:biome placement modifier used without top_feature (sub-feature)",
                );
                let biome = region.get_biome(origin);
                if ctx.biome_has_feature(biome, top_id) {
                    out.push(origin);
                }
            }
            Self::BlockPredicateFilter(pred) => {
                if pred.test(region, origin) {
                    out.push(origin);
                }
            }
            Self::Count(count) => {
                let n = count.sample(rng);
                for _ in 0..n {
                    out.push(origin);
                }
            }
            Self::CountOnEveryLayer(count) => {
                let mut layer = 0;
                loop {
                    let mut found_any = false;
                    let mut i = 0;
                    // CRITICAL QUIRK: `count.sample(rng)` is re-evaluated on EVERY iteration check!
                    while i < count.sample(rng) {
                        let x = rng.next_int_bounded(16) + origin.x;
                        let z = rng.next_int_bounded(16) + origin.z;
                        let start_y = region.get_height(HeightmapKind::MotionBlocking, x, z);
                        let y = find_on_ground_y_position(region, x, start_y, z, layer);
                        if y != i32::MAX {
                            out.push(BlockPos::new(x, y, z));
                            found_any = true;
                        }
                        i += 1;
                    }
                    layer += 1;
                    if !found_any {
                        break;
                    }
                }
            }
            Self::Cuboid {
                xz_size,
                y_size,
                include_edges,
                include_interior,
            } => {
                // Exact RNG order: height, width, length.
                let height = y_size.sample(rng);
                let width = xz_size.sample(rng);
                let length = xz_size.sample(rng);
                for x in 0..=width {
                    let on_x = x == 0 || x == width;
                    for y in 0..=height {
                        let on_y = y == 0 || y == height;
                        if !*include_edges && on_x && on_y {
                            continue;
                        }
                        for z in 0..=length {
                            let on_z = z == 0 || z == length;
                            if !*include_edges && ((on_z && on_y) || (on_x && on_z)) {
                                continue;
                            }
                            if !*include_interior && !on_x && !on_y && !on_z {
                                continue;
                            }
                            out.push(origin.offset(x, y, z));
                        }
                    }
                }
            }
            Self::EnvironmentScan {
                direction_of_search,
                target_condition,
                allowed_search_condition,
                max_steps,
            } => {
                let mut pos = origin;
                if !allowed_search_condition.test(region, pos) {
                    return;
                }
                for _ in 0..*max_steps {
                    if target_condition.test(region, pos) {
                        out.push(pos);
                        return;
                    }
                    pos = pos.relative(*direction_of_search);
                    if region.is_outside_build_height(pos.y) {
                        return;
                    }
                    if !allowed_search_condition.test(region, pos) {
                        break;
                    }
                }
                if target_condition.test(region, pos) {
                    out.push(pos);
                }
            }
            Self::FixedPlacement(positions) => {
                let cx = origin.chunk_x();
                let cz = origin.chunk_z();
                for &p in positions {
                    if p.chunk_x() == cx && p.chunk_z() == cz {
                        out.push(p);
                    }
                }
            }
            Self::HeightRange(hp) => {
                let gen_ctx = WorldGenerationContext::new(
                    region.min_y().max(region.gen_min_y()),
                    region.height().min(region.gen_depth()),
                    region.sea_level(),
                );
                let y = hp.sample(rng, gen_ctx);
                out.push(origin.at_y(y));
            }
            Self::Heightmap(kind) => {
                let h = region.get_height(*kind, origin.x, origin.z);
                if h > region.min_y() {
                    out.push(origin.at_y(h));
                }
            }
            Self::InSquare => {
                let x = rng.next_int_bounded(16) + origin.x;
                let z = rng.next_int_bounded(16) + origin.z;
                out.push(BlockPos::new(x, origin.y, z));
            }
            Self::NoiseBasedCount {
                noise_to_count_ratio,
                noise_factor,
                noise_offset,
            } => {
                let flower_noise = f64::from(BIOME_INFO_NOISE.sample_2d(
                    f64::from(origin.x) / *noise_factor,
                    f64::from(origin.z) / *noise_factor,
                ));
                let n = ((flower_noise + *noise_offset) * f64::from(*noise_to_count_ratio)).ceil()
                    as i32;
                for _ in 0..n {
                    out.push(origin);
                }
            }
            Self::NoiseThresholdCount {
                noise_level,
                below_noise,
                above_noise,
            } => {
                let flower_noise = f64::from(
                    BIOME_INFO_NOISE
                        .sample_2d(f64::from(origin.x) / 200.0, f64::from(origin.z) / 200.0),
                );
                let n = if flower_noise < *noise_level {
                    *below_noise
                } else {
                    *above_noise
                };
                for _ in 0..n {
                    out.push(origin);
                }
            }
            Self::Offset { x, y, z } => {
                let dx = x.sample(rng);
                let dy = y.sample(rng);
                let dz = z.sample(rng);
                out.push(origin.offset(dx, dy, dz));
            }
            Self::RandomChance(chance) => {
                if rng.next_float() < *chance {
                    out.push(origin);
                }
            }
            Self::RandomlySelected(placements) => {
                let idx = rng.next_int_bounded(placements.len() as i32) as usize;
                placements[idx].modify(ctx, region, rng, origin, top_feature, out);
            }
            Self::RarityFilter(chance) => {
                if rng.next_float() < 1.0_f32 / (*chance as f32) {
                    out.push(origin);
                }
            }
            Self::SurfaceRelativeThresholdFilter {
                heightmap,
                min_inclusive,
                max_inclusive,
            } => {
                let surface_y = i64::from(region.get_height(*heightmap, origin.x, origin.z));
                let y = i64::from(origin.y);
                if surface_y + i64::from(*min_inclusive) <= y
                    && y <= surface_y + i64::from(*max_inclusive)
                {
                    out.push(origin);
                }
            }
            Self::SurfaceWaterDepthFilter(max_water_depth) => {
                let ocean = region.get_height(HeightmapKind::OceanFloor, origin.x, origin.z);
                let surface = region.get_height(HeightmapKind::WorldSurface, origin.x, origin.z);
                if surface - ocean <= *max_water_depth {
                    out.push(origin);
                }
            }
        }
    }
}

fn find_on_ground_y_position(
    region: &GenerationRegion,
    x: i32,
    y_start: i32,
    z: i32,
    target_layer: i32,
) -> i32 {
    let ops = region.ops();
    let is_empty = |s: u32| ops.is_air(s) || ops.is_water(s) || ops.is_lava(s);
    let mut current_layer = 0;
    let mut current_block = region.get_block(BlockPos::new(x, y_start, z));
    let min_y_plus_1 = region.min_y() + 1;
    let mut y = y_start;
    while y >= min_y_plus_1 {
        let below_block = region.get_block(BlockPos::new(x, y - 1, z));
        if !is_empty(below_block)
            && is_empty(current_block)
            && ops.block_id(below_block) != ops.bid_bedrock
        {
            if current_layer == target_layer {
                return y;
            }
            current_layer += 1;
        }
        current_block = below_block;
        y -= 1;
    }
    i32::MAX
}

/// Depth-first `FeaturePlacer` (§2.1) with explicit LIFO stacks.
pub fn place_feature_with_modifiers(
    placed: &PlacedFeature,
    ctx: &FeatureContext<'_>,
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    biome_check: bool,
) -> bool {
    if placed.placement.is_empty() {
        return placed.feature.place(ctx, region, rng, origin);
    }

    let top_feature = biome_check.then_some(placed.id);
    let mut placed_any = false;
    let mut positions = Vec::with_capacity(16);
    let mut modifier_indices = Vec::with_capacity(16);
    let mut modified_positions = Vec::with_capacity(16);

    positions.push(origin);
    modifier_indices.push(0usize);

    while let (Some(pos), Some(mod_idx)) = (positions.pop(), modifier_indices.pop()) {
        modified_positions.clear();
        placed.placement[mod_idx].modify(
            ctx,
            region,
            rng,
            pos,
            top_feature,
            &mut modified_positions,
        );
        let next_mod_idx = mod_idx + 1;
        if next_mod_idx < placed.placement.len() {
            for &next_pos in modified_positions.iter().rev() {
                positions.push(next_pos);
                modifier_indices.push(next_mod_idx);
            }
        } else {
            for &next_pos in &modified_positions {
                placed_any |= placed.feature.place(ctx, region, rng, next_pos);
            }
        }
    }

    placed_any
}
