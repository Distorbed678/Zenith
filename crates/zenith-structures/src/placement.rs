//! Structure set filtering, placement evaluation (`random_spread`, `concentric_rings`,
//! `dimension_origin`, frequency reduction, exclusion zones), and `NoiseSpawnFinder`
//! (`docs/specs/worldgen/structures-placement-and-jigsaw.md` §1).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use zenith_biome::{BiomeSource, EndErosionClimate, PointClimate, SearchState};
use zenith_math::helpers::intrinsics::{java_math_cos, java_math_sin};
use zenith_math::helpers::rounding::{java_floor_div_i32, java_round_f64, square_i64};
use zenith_math::{LegacyRandom, RandomSource};
use zenith_registry::Identifier;
use zenith_worldgen::{
    ClimateParameter, CompiledSampler, DensityVolume, RandomState, RouterField, SamplerContext,
    SpawnTargetPoint, WorldgenData,
};

use crate::data::{
    FrequencyReductionMethod, RandomSpreadType, SpreadingPlacement, StructureData,
    StructurePlacement, StructureSet,
};
use crate::geom::{set_large_feature_seed, set_large_feature_with_salt};

/// `NoiseSpawnFinder` distance weight (`2048^2 = 4,194,304`).
const SPAWN_FITNESS_WEIGHT: i64 = 4_194_304;

/// `HIGHLY_ARBITRARY_RANDOM_SALT` used by `legacy_type_2` frequency reduction (§1.3).
const HIGHLY_ARBITRARY_RANDOM_SALT: i32 = 10_387_320;

/// Evaluates a single noise biome at `(qx, qy, qz)` via the point path (`createResolver`),
/// reusing `ctx` (caching `SamplerContext`) and `search_state` (`RTree` warm-start).
pub fn sample_noise_biome_point(
    biome_source: &BiomeSource,
    rs: &RandomState,
    ctx: &mut SamplerContext,
    search_state: &mut SearchState,
    qx: i32,
    qy: i32,
    qz: i32,
) -> Identifier {
    match biome_source {
        BiomeSource::Fixed(src) => src.noise_biome(),
        BiomeSource::Checkerboard(src) => src.noise_biome(qx, qz),
        BiomeSource::TheEnd(src) => {
            let cell = std::cell::RefCell::new(ctx);
            let sampler = EndErosionClimate::new(|bx, by, bz| {
                rs.sample_router_with_context(
                    &mut cell.borrow_mut(),
                    RouterField::Erosion,
                    bx,
                    by,
                    bz,
                )
            });
            src.noise_biome(qx, qy, qz, &sampler)
        }
        BiomeSource::MultiNoise(src) => {
            let cell = std::cell::RefCell::new(ctx);
            let sampler = PointClimate::new(|bx, by, bz| {
                let mut c = cell.borrow_mut();
                [
                    rs.sample_router_with_context(&mut c, RouterField::Temperature, bx, by, bz),
                    rs.sample_router_with_context(&mut c, RouterField::Vegetation, bx, by, bz),
                    rs.sample_router_with_context(&mut c, RouterField::Continents, bx, by, bz),
                    rs.sample_router_with_context(&mut c, RouterField::Erosion, bx, by, bz),
                    rs.sample_router_with_context(&mut c, RouterField::Depth, bx, by, bz),
                    rs.sample_router_with_context(&mut c, RouterField::Ridges, bx, by, bz),
                ]
            });
            src.noise_biome(qx, qy, qz, &sampler, search_state)
        }
    }
}

/// 26.3 column biome pre-check (`context.couldStructureExistInColumn`, spec §2.1 & legacy §1.2):
/// queries `createResolverForChunk` over a `1 × qh × 1` quart column and returns `true` iff any
/// `quartY` in `[min_block_y >> 2, max_block_y >> 2]` satisfies `allowed`.
pub fn could_structure_exist_in_column(
    biome_source: &BiomeSource,
    rs: &RandomState,
    block_x: i32,
    block_z: i32,
    min_block_y: i32,
    max_block_y: i32,
    allowed: &HashSet<Identifier>,
) -> bool {
    let qx = block_x >> 2;
    let qz = block_z >> 2;
    let min_qy = min_block_y >> 2;
    let max_qy = max_block_y >> 2;
    if min_qy > max_qy {
        return false;
    }
    let qh = (max_qy - min_qy + 1) as usize;
    match biome_source {
        BiomeSource::Fixed(src) => allowed.contains(&src.biome),
        BiomeSource::Checkerboard(src) => allowed.contains(&src.noise_biome(qx, qz)),
        BiomeSource::TheEnd(src) => {
            let mut ctx = rs.new_sampler_context(true);
            for dy in 0..qh {
                let qy = min_qy + dy as i32;
                let id = src.noise_biome_with_erosion(qx, qy, qz, |bx, by, bz| {
                    rs.sample_router_with_context(&mut ctx, RouterField::Erosion, bx, by, bz)
                });
                if allowed.contains(&id) {
                    return true;
                }
            }
            false
        }
        BiomeSource::MultiNoise(src) => {
            let vol = DensityVolume::new([1, qh, 1], [qx << 2, min_qy << 2, qz << 2], [4, 4, 4])
                .expect("1 x qh x 1 climate volume is valid");
            let channels = rs.sample_climate_volume(vol);
            let climate_vol = zenith_biome::ClimateVolume::new(qx, min_qy, qz, 1, qh, 1, channels)
                .expect("climate volume dimensions match");
            let param_list = src.parameter_list();
            let mut state = SearchState::new();
            for dy in 0..qh {
                let qy = min_qy + dy as i32;
                let target = zenith_biome::ClimateSampler::sample(&climate_vol, qx, qy, qz);
                if allowed.contains(param_list.find_value(target, &mut state)) {
                    return true;
                }
            }
            false
        }
    }
}

/// Evaluates `NoiseSpawnFinder.findSpawnPosition` (`biomes-and-climate.md` §4) to compute
/// `ChunkGenerator.getOrigin(randomState)` for `minecraft:dimension_origin`.
pub fn compute_dimension_origin(
    targets: &[SpawnTargetPoint],
    worldgen_data: &WorldgenData,
    seed: i64,
    use_legacy_random: bool,
) -> (i32, i32) {
    if targets.is_empty() {
        return (0, 0);
    }

    // Compile each unique density function referenced by `spawn_target`.
    let mut samplers: HashMap<Identifier, (CompiledSampler, SamplerContext)> = HashMap::new();
    for target in targets {
        for (fn_id, _) in &target.entries {
            if !samplers.contains_key(fn_id)
                && let Some(df) = worldgen_data.density_function(fn_id)
                && let Ok((sampler, cache_count)) =
                    RandomState::compile_function(df, seed, use_legacy_random)
            {
                let ctx = SamplerContext::caching(cache_count);
                samplers.insert(fn_id.clone(), (sampler, ctx));
            }
        }
    }

    let mut eval = |block_x: i32, block_z: i32| -> ((i32, i32), i64) {
        let qx = (block_x >> 2) << 2;
        let qz = (block_z >> 2) << 2;
        let mut min_fitness = i64::MAX;
        for target in targets {
            let mut sum: i64 = 0;
            for (fn_id, param) in &target.entries {
                let val = match samplers.get_mut(fn_id) {
                    Some((sampler, ctx)) => sampler.sample_value(ctx, qx, 0, qz),
                    None => 0.0_f32,
                };
                let q = ClimateParameter::quantize(val);
                sum = sum.wrapping_add(square_i64(param.distance(q)));
            }
            if sum < min_fitness {
                min_fitness = sum;
            }
        }
        let total_fitness = min_fitness
            .wrapping_mul(SPAWN_FITNESS_WEIGHT)
            .wrapping_add(square_i64(i64::from(block_x)))
            .wrapping_add(square_i64(i64::from(block_z)));
        ((block_x, block_z), total_fitness)
    };

    let mut best = eval(0, 0);
    radial_search(&mut best, 2048.0_f32, 512.0_f32, &mut eval);
    radial_search(&mut best, 512.0_f32, 32.0_f32, &mut eval);
    (best.0.0 >> 4, best.0.1 >> 4)
}

fn radial_search(
    best: &mut ((i32, i32), i64),
    max_radius: f32,
    step: f32,
    eval: &mut impl FnMut(i32, i32) -> ((i32, i32), i64),
) {
    let origin = best.0;
    let mut angle: f32 = 0.0_f32;
    let mut radius: f32 = step;

    while radius <= max_radius {
        let x = origin
            .0
            .wrapping_add((java_math_sin(f64::from(angle)) * f64::from(radius)) as i32);
        let z = origin
            .1
            .wrapping_add((java_math_cos(f64::from(angle)) * f64::from(radius)) as i32);
        let cand = eval(x, z);
        if cand.1 < best.1 {
            *best = cand;
        }
        angle += step / radius;
        if f64::from(angle) > 2.0_f64 * std::f64::consts::PI {
            angle = 0.0_f32;
            radius += step;
        }
    }
}

/// Per-dimension structure placement state (`ChunkGeneratorStructureState`, §1).
pub struct StructurePlacementState {
    level_seed: i64,
    concentric_rings_seed: i64,
    origin: (i32, i32),
    structure_data: Arc<StructureData>,
    possible_sets: Vec<Arc<StructureSet>>,
    ring_positions: RwLock<HashMap<Identifier, Vec<(i32, i32)>>>,
    ring_position_sets: RwLock<HashMap<Identifier, HashSet<(i32, i32)>>>,
}

impl std::fmt::Debug for StructurePlacementState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StructurePlacementState")
            .field("level_seed", &self.level_seed)
            .field("concentric_rings_seed", &self.concentric_rings_seed)
            .field("origin", &self.origin)
            .field("possible_sets_count", &self.possible_sets.len())
            .finish()
    }
}

impl StructurePlacementState {
    /// Creates placement state for a normal noise-based dimension (`createForNormal`).
    pub fn for_normal(
        level_seed: i64,
        origin: (i32, i32),
        biome_source: &BiomeSource,
        structure_data: Arc<StructureData>,
    ) -> Self {
        let possible_biomes: HashSet<Identifier> =
            biome_source.possible_biomes().into_iter().collect();
        let possible_sets = filter_sets_by_biomes(
            structure_data.structure_sets().iter().cloned(),
            &structure_data,
            &possible_biomes,
        );
        Self {
            level_seed,
            concentric_rings_seed: level_seed,
            origin,
            structure_data,
            possible_sets,
            ring_positions: RwLock::new(HashMap::new()),
            ring_position_sets: RwLock::new(HashMap::new()),
        }
    }

    /// Creates placement state for a flat world (`createForFlat`, `concentricRingsSeed = 0L`).
    pub fn for_flat(
        level_seed: i64,
        override_sets: Option<&[Identifier]>,
        biome_source: &BiomeSource,
        structure_data: Arc<StructureData>,
    ) -> Self {
        let possible_biomes: HashSet<Identifier> =
            biome_source.possible_biomes().into_iter().collect();
        let candidates: Vec<Arc<StructureSet>> = match override_sets {
            Some(ids) => ids
                .iter()
                .filter_map(|id| structure_data.structure_set(id).cloned())
                .collect(),
            None => structure_data.structure_sets().to_vec(),
        };
        let possible_sets =
            filter_sets_by_biomes(candidates.into_iter(), &structure_data, &possible_biomes);
        Self {
            level_seed,
            concentric_rings_seed: 0,
            origin: (0, 0),
            structure_data,
            possible_sets,
            ring_positions: RwLock::new(HashMap::new()),
            ring_position_sets: RwLock::new(HashMap::new()),
        }
    }

    /// Level seed.
    #[inline]
    pub const fn level_seed(&self) -> i64 {
        self.level_seed
    }

    /// Dimension origin chunk (`ChunkGenerator.getOrigin`).
    #[inline]
    pub const fn origin(&self) -> (i32, i32) {
        self.origin
    }

    /// Filtered biome-compatible structure sets in dynamic-registry numeric ID order.
    #[inline]
    pub fn possible_structure_sets(&self) -> &[Arc<StructureSet>] {
        &self.possible_sets
    }

    /// Computed dimension origin chunk (`(chunk_x, chunk_z)`) for `dimension_origin` placement.
    #[inline]
    pub const fn dimension_origin(&self) -> (i32, i32) {
        self.origin
    }

    /// Lazily computes and returns all `concentric_rings` chunk positions for `set` (§1.5).
    pub fn ring_positions_for(
        &self,
        set: &StructureSet,
        biome_source: &BiomeSource,
        rs: &RandomState,
    ) -> Vec<(i32, i32)> {
        if let Ok(guard) = self.ring_positions.read()
            && let Some(pos) = guard.get(&set.id)
        {
            return pos.clone();
        }
        let positions = match &set.placement {
            StructurePlacement::ConcentricRings {
                distance,
                spread,
                count,
                preferred_biomes,
                ..
            } => generate_ring_positions(
                self.concentric_rings_seed,
                *distance,
                *spread,
                *count,
                preferred_biomes,
                biome_source,
                rs,
            ),
            _ => Vec::new(),
        };
        let pos_set: HashSet<(i32, i32)> = positions.iter().copied().collect();
        if let Ok(mut guard) = self.ring_positions.write() {
            guard.insert(set.id.clone(), positions.clone());
        }
        if let Ok(mut guard) = self.ring_position_sets.write() {
            guard.insert(set.id.clone(), pos_set);
        }
        positions
    }

    /// Evaluates `placement.isStructureChunk(state, sourceX, sourceZ)` (§1.3).
    pub fn is_structure_chunk(
        &self,
        set: &StructureSet,
        source_x: i32,
        source_z: i32,
        biome_source: &BiomeSource,
        rs: &RandomState,
    ) -> bool {
        match &set.placement {
            StructurePlacement::DimensionOrigin => {
                source_x == self.origin.0 && source_z == self.origin.1
            }
            StructurePlacement::RandomSpread {
                common,
                spacing,
                separation,
                spread_type,
            } => {
                let pot = potential_random_spread_chunk(
                    self.level_seed,
                    source_x,
                    source_z,
                    *spacing,
                    *separation,
                    *spread_type,
                    common.salt,
                );
                if pot != (source_x, source_z) {
                    return false;
                }
                if !apply_frequency_reduction(common, source_x, source_z, self.level_seed) {
                    return false;
                }
                self.check_exclusion_zone(common, source_x, source_z, biome_source, rs)
            }
            StructurePlacement::ConcentricRings { common, .. } => {
                // Ensure ring positions are generated.
                let is_ring = {
                    if let Ok(guard) = self.ring_position_sets.read()
                        && let Some(pos_set) = guard.get(&set.id)
                    {
                        pos_set.contains(&(source_x, source_z))
                    } else {
                        let _ = self.ring_positions_for(set, biome_source, rs);
                        self.ring_position_sets
                            .read()
                            .ok()
                            .and_then(|g| g.get(&set.id).map(|s| s.contains(&(source_x, source_z))))
                            .unwrap_or(false)
                    }
                };
                if !is_ring {
                    return false;
                }
                if !apply_frequency_reduction(common, source_x, source_z, self.level_seed) {
                    return false;
                }
                self.check_exclusion_zone(common, source_x, source_z, biome_source, rs)
            }
        }
    }

    fn check_exclusion_zone(
        &self,
        common: &SpreadingPlacement,
        source_x: i32,
        source_z: i32,
        biome_source: &BiomeSource,
        rs: &RandomState,
    ) -> bool {
        let Some(ez) = &common.exclusion_zone else {
            return true;
        };
        let Some(other) = self.structure_data.structure_set(&ez.other_set) else {
            return true;
        };
        for tx in (source_x - ez.chunk_count)..=(source_x + ez.chunk_count) {
            for tz in (source_z - ez.chunk_count)..=(source_z + ez.chunk_count) {
                if self.is_structure_chunk(other, tx, tz, biome_source, rs) {
                    return false;
                }
            }
        }
        true
    }
}

fn filter_sets_by_biomes(
    sets: impl Iterator<Item = Arc<StructureSet>>,
    data: &StructureData,
    possible_biomes: &HashSet<Identifier>,
) -> Vec<Arc<StructureSet>> {
    sets.filter(|set| {
        set.structures.iter().any(|entry| {
            data.resolve_structure(&entry.structure).is_some_and(|s| {
                s.settings
                    .biomes
                    .iter()
                    .any(|b| possible_biomes.contains(b))
            })
        })
    })
    .collect()
}

/// `RandomSpreadStructurePlacement.getPotentialStructureChunk` (§1.4).
pub fn potential_random_spread_chunk(
    level_seed: i64,
    source_x: i32,
    source_z: i32,
    spacing: i32,
    separation: i32,
    spread_type: RandomSpreadType,
    salt: i32,
) -> (i32, i32) {
    let gx = java_floor_div_i32(source_x, spacing);
    let gz = java_floor_div_i32(source_z, spacing);
    let mut rng = LegacyRandom::new(0);
    set_large_feature_with_salt(&mut rng, level_seed, gx, gz, salt);
    let limit = spacing - separation;
    let (sx, sz) = match spread_type {
        RandomSpreadType::Linear => (rng.next_int_bounded(limit), rng.next_int_bounded(limit)),
        RandomSpreadType::Triangular => {
            let x = (rng.next_int_bounded(limit) + rng.next_int_bounded(limit)) / 2;
            let z = (rng.next_int_bounded(limit) + rng.next_int_bounded(limit)) / 2;
            (x, z)
        }
    };
    (gx * spacing + sx, gz * spacing + sz)
}

/// `AbstractSpreadingStructurePlacement.applyAdditionalChunkRestrictions` (§1.3).
fn apply_frequency_reduction(
    common: &SpreadingPlacement,
    source_x: i32,
    source_z: i32,
    level_seed: i64,
) -> bool {
    if common.frequency >= 1.0_f32 || common.frequency.is_nan() {
        return true;
    }
    let mut rng = LegacyRandom::new(0);
    match common.frequency_reduction_method {
        FrequencyReductionMethod::Default => {
            // Exact vanilla parameter order: x = salt, z = sourceX, blend = sourceZ!
            set_large_feature_with_salt(&mut rng, level_seed, common.salt, source_x, source_z);
            rng.next_float() < common.frequency
        }
        FrequencyReductionMethod::LegacyType1 => {
            let cx = source_x >> 4;
            let cz = source_z >> 4;
            let mixed = i64::from(cx ^ (cz << 4)) ^ level_seed;
            rng.set_seed(mixed);
            let _ = rng.next_int();
            let bound = (1.0_f32 / common.frequency) as i32;
            rng.next_int_bounded(bound) == 0
        }
        FrequencyReductionMethod::LegacyType2 => {
            set_large_feature_with_salt(
                &mut rng,
                level_seed,
                source_x,
                source_z,
                HIGHLY_ARBITRARY_RANDOM_SALT,
            );
            rng.next_float() < common.frequency
        }
        FrequencyReductionMethod::LegacyType3 => {
            set_large_feature_seed(&mut rng, level_seed, source_x, source_z);
            rng.next_double() < f64::from(common.frequency)
        }
    }
}

/// `ChunkGeneratorStructureState.generateRingPositions` (§1.5).
pub fn generate_ring_positions(
    concentric_rings_seed: i64,
    distance: i32,
    spread: i32,
    count: i32,
    preferred_biomes: &HashSet<Identifier>,
    biome_source: &BiomeSource,
    rs: &RandomState,
) -> Vec<(i32, i32)> {
    if count <= 0 {
        return Vec::new();
    }
    let mut rng = LegacyRandom::new(concentric_rings_seed);
    let mut angle = (rng.next_double() * std::f64::consts::PI) * 2.0;
    let mut position_in_circle = 0_i32;
    let mut circle = 0_i32;
    let mut cur_spread = spread;
    let mut result = Vec::with_capacity(count as usize);

    for i in 0..count {
        let base_dist = 4_i32
            .wrapping_mul(distance)
            .wrapping_add(distance.wrapping_mul(circle).wrapping_mul(6));
        let dist = f64::from(base_dist) + (rng.next_double() - 0.5) * (f64::from(distance) * 2.5);
        let initial_x = java_round_f64(java_math_cos(angle) * dist) as i32;
        let initial_z = java_round_f64(java_math_sin(angle) * dist) as i32;

        // Fork per-ring-slot legacy RNG sequentially before advancing the loop.
        let mut biome_search_rng = rng.fork();

        let origin_x = (initial_x << 4) + 8;
        let origin_z = (initial_z << 4) + 8;
        let noise_cx = origin_x >> 2;
        let noise_cz = origin_z >> 2;
        let noise_r = 28_i32; // 112 >> 2

        let mut ctx = rs.new_sampler_context(true);
        let mut search_state = SearchState::new();
        let mut chosen: Option<(i32, i32)> = None;
        let mut found = 0_i32;

        // Exact loop order: z outer (-28..=28), x inner (-28..=28).
        for dz in -noise_r..=noise_r {
            for dx in -noise_r..=noise_r {
                let qx = noise_cx + dx;
                let qz = noise_cz + dz;
                let biome = sample_noise_biome_point(
                    biome_source,
                    rs,
                    &mut ctx,
                    &mut search_state,
                    qx,
                    0,
                    qz,
                );
                if preferred_biomes.contains(&biome) {
                    if chosen.is_none() || biome_search_rng.next_int_bounded(found + 1) == 0 {
                        chosen = Some((qx << 2, qz << 2));
                    }
                    found += 1;
                }
            }
        }

        let final_chunk = match chosen {
            Some((bx, bz)) => (bx >> 4, bz >> 4),
            None => (initial_x, initial_z),
        };
        result.push(final_chunk);

        angle += (std::f64::consts::PI * 2.0) / f64::from(cur_spread);
        position_in_circle += 1;
        if position_in_circle == cur_spread {
            circle += 1;
            position_in_circle = 0;
            cur_spread += (2 * cur_spread) / (circle + 1);
            cur_spread = cur_spread.min(count - i);
            angle += (rng.next_double() * std::f64::consts::PI) * 2.0;
        }
    }

    result
}
