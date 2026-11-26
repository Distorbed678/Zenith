//! `FeatureDecorator` executing the full 26.3 `minecraft:features` chunk step
//! (`ChunkGenerator.applyBiomeDecoration`, `docs/specs/worldgen/features.md` §1.4).

use std::collections::HashSet;
use std::sync::Arc;

use zenith_biome::{BiomeSource, obfuscate_seed};
use zenith_math::{LegacyRandom, RandomSource, XoroshiroRandom};
use zenith_worldgen::{BiomeId, PositionalFactory, TerrainChunk};

use crate::block_ops::BlockOps;
use crate::data::FeatureData;
use crate::features::{
    FeatureContext, NoopStructureHook, NoopTemplateHook, StructureInterleaveHook,
    TemplateFeatureHook, TreeFeatureHook, tree::VanillaTreeHook,
};
use crate::placement::place_feature_with_modifiers;
use crate::region::{GenerationRegion, RegionChunk};
use crate::rng::{BlockPos, WorldgenRandom};
use crate::sorter::{FeatureCycleError, StepFeatureData, build_features_per_step};

/// Number of built-in decoration steps (`GenerationStep.Decoration`: `0..=10`).
pub const DECORATION_STEP_COUNT: usize = 11;

/// Identifier used to derive `WorldGenRegion.random` from `RandomState.rootRandom`.
pub const WORLDGEN_REGION_RANDOM_ID: &str = "minecraft:worldgen_region_random";

/// Applies the `minecraft:features` chunk step to a `3 × 3` [`GenerationRegion`]
/// (`docs/specs/worldgen/features.md` §1.4).
#[derive(Debug)]
pub struct FeatureDecorator {
    data: Arc<FeatureData>,
    ops: Arc<BlockOps>,
    features_per_step: Vec<StepFeatureData>,
    possible_biome_ids: HashSet<BiomeId>,
    world_seed: i64,
    obfuscated_seed: i64,
    gen_min_y: i32,
    gen_depth: i32,
    sea_level: i32,
    region_random_factory: PositionalFactory,
    tree_hook: Arc<dyn TreeFeatureHook>,
    template_hook: Arc<dyn TemplateFeatureHook>,
    structure_hook: Arc<dyn StructureInterleaveHook>,
}

impl FeatureDecorator {
    /// Constructs a [`FeatureDecorator`] for a dimension by sorting all biome features across
    /// `biome_source.possible_biomes()` via `FeatureSorter.buildFeaturesPerStep` (§1.3).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        data: Arc<FeatureData>,
        ops: Arc<BlockOps>,
        biome_source: &BiomeSource,
        world_seed: i64,
        gen_min_y: i32,
        gen_depth: i32,
        sea_level: i32,
        legacy_random_source: bool,
    ) -> Result<Self, FeatureCycleError> {
        let source_biomes = biome_source.possible_biomes();
        let mut ordered_biome_ids = Vec::with_capacity(source_biomes.len());
        let mut possible_biome_ids = HashSet::with_capacity(source_biomes.len());
        for id in &source_biomes {
            if let Some(&bid) = data.biome_ids.get(id) {
                ordered_biome_ids.push(bid);
                possible_biome_ids.insert(bid);
            }
        }

        let features_per_step = {
            let biomes = data.biomes.as_slice();
            build_features_per_step(
                &ordered_biome_ids,
                |&bid| biomes[bid.0 as usize].features.as_slice(),
                true,
            )?
        };

        let root_random = if legacy_random_source {
            PositionalFactory::Legacy(LegacyRandom::new(world_seed).fork_positional())
        } else {
            PositionalFactory::Xoroshiro(XoroshiroRandom::new(world_seed).fork_positional())
        };
        let region_random_factory = root_random.fork_named(WORLDGEN_REGION_RANDOM_ID);

        Ok(Self {
            data,
            ops,
            features_per_step,
            possible_biome_ids,
            world_seed,
            obfuscated_seed: obfuscate_seed(world_seed),
            gen_min_y,
            gen_depth,
            sea_level,
            region_random_factory,
            tree_hook: Arc::new(VanillaTreeHook),
            template_hook: Arc::new(NoopTemplateHook),
            structure_hook: Arc::new(NoopStructureHook),
        })
    }

    /// Installs a custom `minecraft:tree` & `fallen_tree` decorator hook.
    pub fn with_tree_hook(mut self, hook: Arc<dyn TreeFeatureHook>) -> Self {
        self.tree_hook = hook;
        self
    }

    /// Installs a custom structure-template hook for `minecraft:fossil` and `minecraft:template`.
    pub fn with_template_hook(mut self, hook: Arc<dyn TemplateFeatureHook>) -> Self {
        self.template_hook = hook;
        self
    }

    /// Installs a custom structure-piece interleaving hook (for `generate-structures = true`).
    pub fn with_structure_hook(mut self, hook: Arc<dyn StructureInterleaveHook>) -> Self {
        self.structure_hook = hook;
        self
    }

    /// Returns the globally sorted feature lists per decoration step (from `FeatureSorter`).
    #[inline]
    pub fn features_per_step(&self) -> &[StepFeatureData] {
        &self.features_per_step
    }

    /// Shared [`BlockOps`].
    #[inline]
    pub fn ops(&self) -> &Arc<BlockOps> {
        &self.ops
    }

    /// Builds a `3 × 3` [`GenerationRegion`] centered on `(center_x, center_z)` from 9
    /// `terrain`-status [`TerrainChunk`]s in `(dz = -1..=1, dx = -1..=1)` order.
    pub fn make_region(
        &self,
        center_x: i32,
        center_z: i32,
        terrain_chunks: [&TerrainChunk; 9],
    ) -> GenerationRegion {
        let chunks =
            std::array::from_fn(|i| RegionChunk::from_terrain_chunk(terrain_chunks[i], &self.ops));
        self.make_region_from_chunks(center_x, center_z, chunks)
    }

    /// Builds a `3 × 3` [`GenerationRegion`] from already-built chunks (`(dz + 1) * 3 + (dx + 1)`
    /// order), e.g. chunks an earlier decoration of a neighbouring chunk has written into.
    pub fn make_region_from_chunks(
        &self,
        center_x: i32,
        center_z: i32,
        chunks: [RegionChunk; 9],
    ) -> GenerationRegion {
        let region_random = self
            .region_random_factory
            .at(center_x << 4, 0, center_z << 4);
        GenerationRegion::new(
            center_x,
            center_z,
            chunks,
            self.world_seed,
            self.obfuscated_seed,
            self.gen_min_y,
            self.gen_depth,
            self.sea_level,
            region_random,
            Arc::clone(&self.ops),
            self.data.biome_ids.clone(),
        )
    }

    /// Executes `ChunkGenerator.applyBiomeDecoration` for `(chunk_x, chunk_z)` over `region`
    /// (`docs/specs/worldgen/features.md` §1.4).
    pub fn decorate(&self, region: &mut GenerationRegion, chunk_x: i32, chunk_z: i32) {
        debug_assert_eq!(region.center_x(), chunk_x);
        debug_assert_eq!(region.center_z(), chunk_z);

        let origin = BlockPos::new(chunk_x << 4, region.min_section_y() << 4, chunk_z << 4);
        let mut rng = WorldgenRandom::new_xoroshiro(0);
        let decoration_seed = rng.set_decoration_seed(region.world_seed(), origin.x, origin.z);

        // Step 8: collect biomes present across the 3×3 neighborhood intersected with biomeSource.possibleBiomes().
        let possible_biomes: Vec<BiomeId> = region
            .collect_region_biomes()
            .into_iter()
            .filter(|bid| self.possible_biome_ids.contains(bid))
            .collect();

        let feature_step_count = self.features_per_step.len();
        let generation_steps = DECORATION_STEP_COUNT.max(feature_step_count);

        let ctx = FeatureContext {
            biome_settings: &self.data.biomes,
            tree_hook: self.tree_hook.as_ref(),
            template_hook: self.template_hook.as_ref(),
        };

        let mut step_indices: Vec<i32> = Vec::new();
        for step_index in 0..generation_steps {
            // 11.1 Interleaved structure placement
            self.structure_hook.place_structures_for_step(
                &ctx,
                step_index,
                decoration_seed,
                region,
                &mut rng,
            );

            // 11.2 Biome features for this step
            if step_index < feature_step_count {
                step_indices.clear();
                let step_data = &self.features_per_step[step_index];
                for &bid in &possible_biomes {
                    let biome_features = &self.data.biomes[bid.0 as usize].features;
                    if step_index < biome_features.len() {
                        for pf in &biome_features[step_index] {
                            if let Some(global_idx) = step_data.index_of(pf.id) {
                                step_indices.push(global_idx);
                            }
                        }
                    }
                }
                step_indices.sort_unstable();
                step_indices.dedup();

                for &global_idx in &step_indices {
                    let placed = &step_data.features[global_idx as usize];
                    rng.set_feature_seed(decoration_seed, global_idx, step_index as i32);
                    place_feature_with_modifiers(placed, &ctx, region, &mut rng, origin, true);
                }
            }
        }
    }
}
