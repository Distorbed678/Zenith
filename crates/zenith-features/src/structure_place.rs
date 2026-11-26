//! Structure pieces and NBT templates inside the `features` step: the [`StructureWorld`] adapter
//! over a [`GenerationRegion`], the structure-interleave hook (§1.4 of `features.md`, §5.1 of
//! `structures-placement-and-jigsaw.md`) and the `fossil` / `template` feature hooks (§4.42,
//! §4.43), all on top of `zenith-structures`.

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use zenith_codec::json::Json;
use zenith_math::RandomSource;
use zenith_nbt::Compound;
use zenith_registry::{BlockStateTable, Identifier};
use zenith_structures as zs;
use zenith_structures::{
    BoundingBox, BoundingBoxExt, Mirror, PlacementEnv, StructureGenerator, StructurePlaceSettings,
    StructureStart, StructureTemplate, StructureWorld, TemplateManager,
};
use zenith_worldgen::{BlockTags, HeightmapKind, SeededRandom};

use crate::data::FeatureData;
use crate::features::{
    FeatureContext, FossilConfig, StructureInterleaveHook, TemplateConfig, TemplateFeatureHook,
};
use crate::placement::place_feature_with_modifiers;
use crate::region::GenerationRegion;
use crate::rng::{BlockPos, Rotation, WorldgenRandom};

// ---------------------------------------------------------------------------
// Conversions
// ---------------------------------------------------------------------------

#[inline]
fn fpos(p: zs::BlockPos) -> BlockPos {
    BlockPos::new(p.x, p.y, p.z)
}

#[inline]
fn zrot(r: Rotation) -> zs::Rotation {
    match r {
        Rotation::None => zs::Rotation::None,
        Rotation::Clockwise90 => zs::Rotation::Clockwise90,
        Rotation::Clockwise180 => zs::Rotation::Clockwise180,
        Rotation::Counterclockwise90 => zs::Rotation::Counterclockwise90,
    }
}

// ---------------------------------------------------------------------------
// The region's own random (`WorldGenRegion.getRandom()`)
// ---------------------------------------------------------------------------

/// `WorldGenRegion.random` as a dynamic random for the structure adapter.
#[derive(Debug, Clone)]
pub struct RegionRandom(pub SeededRandom);

macro_rules! delegate {
    ($self:ident, $m:ident $(, $a:expr)*) => {
        match &mut $self.0 {
            SeededRandom::Legacy(r) => zenith_math::RandomSource::$m(r $(, $a)*),
            SeededRandom::Xoroshiro(r) => zenith_math::RandomSource::$m(r $(, $a)*),
        }
    };
}

impl zs::WorldgenRandom for RegionRandom {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn next_int(&mut self) -> i32 {
        delegate!(self, next_int)
    }
    fn next_int_bounded(&mut self, bound: i32) -> i32 {
        delegate!(self, next_int_bounded, bound)
    }
    fn next_long(&mut self) -> i64 {
        delegate!(self, next_long)
    }
    fn next_boolean(&mut self) -> bool {
        delegate!(self, next_boolean)
    }
    fn next_float(&mut self) -> f32 {
        delegate!(self, next_float)
    }
    fn next_double(&mut self) -> f64 {
        delegate!(self, next_double)
    }
    fn next_gaussian(&mut self) -> f64 {
        delegate!(self, next_gaussian)
    }
}

// ---------------------------------------------------------------------------
// StructureWorld adapter
// ---------------------------------------------------------------------------

/// What a `feature_pool_element` needs to run a placed feature inside a structure piece.
#[derive(Debug, Clone, Copy)]
pub struct PlaceFeatureEnv<'a> {
    /// Shared feature context (hooks, per-biome settings).
    pub ctx: &'a FeatureContext<'a>,
    /// Loaded feature registries (placed features by id, biome names).
    pub data: &'a FeatureData,
}

/// [`StructureWorld`] over a [`GenerationRegion`] (`WorldGenRegion` while decorating one chunk).
pub struct RegionStructureWorld<'a> {
    region: &'a mut GenerationRegion,
    features: Option<PlaceFeatureEnv<'a>>,
}

impl std::fmt::Debug for RegionStructureWorld<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegionStructureWorld")
            .finish_non_exhaustive()
    }
}

impl<'a> RegionStructureWorld<'a> {
    /// Wraps `region`. `features` enables `feature_pool_element` placement and biome lookups.
    pub fn new(region: &'a mut GenerationRegion, features: Option<PlaceFeatureEnv<'a>>) -> Self {
        Self { region, features }
    }
}

impl StructureWorld for RegionStructureWorld<'_> {
    fn world_seed(&self) -> i64 {
        self.region.world_seed()
    }
    fn min_y(&self) -> i32 {
        self.region.min_y()
    }
    fn max_y(&self) -> i32 {
        self.region.min_y() + self.region.height() - 1
    }
    fn sea_level(&self) -> i32 {
        self.region.sea_level()
    }
    fn blocks(&self) -> &BlockStateTable {
        &self.region.ops().blocks
    }
    fn block_tags(&self) -> &BlockTags {
        &self.region.ops().tags
    }
    fn facts(&self) -> &zenith_blocks::BlockFacts {
        &self.region.ops().facts
    }
    fn get_block_state(&self, pos: zs::BlockPos) -> u32 {
        self.region.get_block(fpos(pos))
    }
    fn set_block_state(&mut self, pos: zs::BlockPos, state: u32, flags: u32) -> bool {
        self.region.set_block(fpos(pos), state, flags)
    }
    fn get_height(&self, kind: HeightmapKind, x: i32, z: i32) -> i32 {
        self.region.get_height(kind, x, z)
    }
    fn schedule_fluid_tick(&mut self, pos: zs::BlockPos, delay: i32) {
        let fluid = self.region.ops().fluid(self.region.get_block(fpos(pos)));
        self.region
            .schedule_fluid_tick(fpos(pos), fluid.kind, delay);
    }
    fn mark_pos_for_post_processing(&mut self, pos: zs::BlockPos) {
        self.region.mark_pos_for_post_processing(fpos(pos));
    }
    fn set_block_entity(&mut self, pos: zs::BlockPos, nbt: Compound) {
        self.region.merge_block_entity_data(fpos(pos), nbt);
    }
    fn add_entity(&mut self, nbt: Compound) {
        self.region.add_nbt_entity(nbt);
    }
    fn level_random(&mut self) -> &mut dyn zs::WorldgenRandom {
        self.region.region_random_dyn()
    }
    fn place_feature(
        &mut self,
        feature: &Json,
        pos: zs::BlockPos,
        random: &mut dyn zs::WorldgenRandom,
    ) -> bool {
        let Some(env) = self.features else {
            return false;
        };
        let Some(id) = feature.as_str().and_then(|s| Identifier::parse(s).ok()) else {
            return false;
        };
        let Some(placed) = env.data.placed_features.get(&id) else {
            return false;
        };
        let Some(rng) = random.as_any_mut().downcast_mut::<WorldgenRandom>() else {
            return false;
        };
        // `PlacedFeature.place(level, generator, random, pos)`: no biome check.
        place_feature_with_modifiers(placed, env.ctx, self.region, rng, fpos(pos), false)
    }
    fn biome_at(&self, pos: zs::BlockPos) -> Option<Identifier> {
        let env = self.features?;
        let bid = self.region.get_biome(fpos(pos));
        env.data.biomes.get(bid.0 as usize).map(|b| b.id.clone())
    }
}

// ---------------------------------------------------------------------------
// Structure pieces (§1.4 / §5.1)
// ---------------------------------------------------------------------------

/// Chunk-keyed cache of `StructureStart`s with the mutable piece state vanilla keeps in memory
/// between chunk calls.
pub type StartStore = HashMap<(i32, i32), Vec<StructureStart>>;

/// Places the pieces of every structure start referenced by the chunk being decorated, once per
/// decoration step before that step's biome features.
pub struct StructurePlacementHook {
    generator: Arc<StructureGenerator>,
    data: Arc<FeatureData>,
    /// Structures per decoration step in registry order.
    by_step: Vec<Vec<Identifier>>,
    starts: Mutex<StartStore>,
}

impl std::fmt::Debug for StructurePlacementHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StructurePlacementHook")
            .finish_non_exhaustive()
    }
}

impl StructurePlacementHook {
    /// Builds the hook for `generator`'s dimension.
    pub fn new(generator: Arc<StructureGenerator>, data: Arc<FeatureData>) -> Self {
        let mut by_step: Vec<Vec<Identifier>> = vec![Vec::new(); 11];
        for def in generator.structure_data().structures() {
            by_step[def.settings.step.ordinal()].push(def.id.clone());
        }
        Self {
            generator,
            data,
            by_step,
            starts: Mutex::new(HashMap::new()),
        }
    }

    /// The structure generator (starts, references, beardifier) the hook places from.
    pub fn generator(&self) -> &Arc<StructureGenerator> {
        &self.generator
    }

    /// Runs `f` with the generator and the shared start store (the starts hold the piece state
    /// placement mutates, like vanilla's in-memory starts).
    pub fn with_starts<R>(&self, f: impl FnOnce(&StructureGenerator, &mut StartStore) -> R) -> R {
        let mut store = self.starts.lock().unwrap_or_else(PoisonError::into_inner);
        f(&self.generator, &mut store)
    }

    /// The terrain `Beardifier` of `(chunk_x, chunk_z)` from the shared starts.
    pub fn beardifier_for_chunk(&self, chunk_x: i32, chunk_z: i32) -> zenith_worldgen::Beardifier {
        self.with_starts(|g, store| g.beardifier_for_chunk_with_cache(chunk_x, chunk_z, store))
    }
}

impl StructureInterleaveHook for StructurePlacementHook {
    fn place_structures_for_step(
        &self,
        ctx: &FeatureContext<'_>,
        step_index: usize,
        decoration_seed: i64,
        region: &mut GenerationRegion,
        rng: &mut WorldgenRandom,
    ) {
        let Some(structures) = self.by_step.get(step_index) else {
            return;
        };
        if structures.is_empty() {
            return;
        }
        let (cx, cz) = (region.center_x(), region.center_z());
        let mut store = self.starts.lock().unwrap_or_else(PoisonError::into_inner);
        let refs = self
            .generator
            .structure_references_packed_with_cache(cx, cz, &mut store);
        let refs: HashMap<&Identifier, &Vec<i64>> = refs.iter().map(|(id, v)| (id, v)).collect();
        let env = PlacementEnv {
            data: self.generator.structure_data(),
            templates: self.generator.template_manager(),
        };

        for (index, id) in structures.iter().enumerate() {
            rng.set_feature_seed(decoration_seed, index as i32, step_index as i32);
            let Some(packed) = refs.get(id) else {
                continue;
            };
            // Vanilla iterates a `LongOpenHashSet` filled in `x`-major, `z`-minor order.
            let mut insertion: Vec<i64> = packed.to_vec();
            insertion.sort_by_key(|&p| (p as i32, (p >> 32) as i32));
            for p in crate::fastutil::long_open_hash_set_order(&insertion) {
                let key = (p as i32, (p >> 32) as i32);
                let Some(start) = store
                    .get_mut(&key)
                    .and_then(|v| v.iter_mut().find(|s| s.structure_id == *id && s.is_valid()))
                else {
                    continue;
                };
                let mut world = RegionStructureWorld::new(
                    &mut *region,
                    Some(PlaceFeatureEnv {
                        ctx,
                        data: &self.data,
                    }),
                );
                let mut writable = zs::writable_area(&world, cx, cz);
                start.place_in_chunk(&env, &mut world, &mut *rng, &mut writable);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// `fossil` and `template` features (§4.42 / §4.43)
// ---------------------------------------------------------------------------

/// `minecraft:fossil` and `minecraft:template` on top of the structure template placement.
pub struct StructureTemplateHook {
    data: Arc<zs::StructureData>,
    templates: Arc<TemplateManager>,
}

impl std::fmt::Debug for StructureTemplateHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StructureTemplateHook")
            .finish_non_exhaustive()
    }
}

impl StructureTemplateHook {
    /// Builds the hook from the structure registries and template loader.
    pub fn new(data: Arc<zs::StructureData>, templates: Arc<TemplateManager>) -> Self {
        Self { data, templates }
    }

    fn processors(&self, json: &Json) -> Vec<zs::StructureProcessor> {
        self.data
            .decode_processor_list_holder(json)
            .unwrap_or_default()
    }
}

impl TemplateFeatureHook for StructureTemplateHook {
    fn place_fossil(
        &self,
        config: &FossilConfig,
        region: &mut GenerationRegion,
        rng: &mut WorldgenRandom,
        origin: BlockPos,
    ) -> bool {
        let rotation = Rotation::get_random(rng);
        let rot = zrot(rotation);
        let idx = rng.next_int_bounded(config.fossil_structures.len() as i32) as usize;
        let Ok(base) = self.templates.get(&config.fossil_structures[idx]) else {
            return false;
        };
        let Some(overlay_id) = config.overlay_structures.get(idx) else {
            return false;
        };
        let Ok(overlay) = self.templates.get(overlay_id) else {
            return false;
        };

        let (chunk_x, chunk_z) = (origin.x >> 4, origin.z >> 4);
        let min_y = region.min_y();
        let max_y = min_y + region.height() - 1;
        let clip = BoundingBox::from_coords(
            (chunk_x << 4) - 16,
            min_y,
            (chunk_z << 4) - 16,
            (chunk_x << 4) + 31,
            max_y,
            (chunk_z << 4) + 31,
        );
        let mut settings = StructurePlaceSettings {
            rotation: rot,
            bounding_box: Some(clip),
            shared_random: true,
            processors: self.processors(&config.fossil_processors),
            ..StructurePlaceSettings::default()
        };

        let size = base.rotated_size(rot);
        let low_corner = origin.offset(-(size.x / 2), 0, -(size.z / 2));
        let mut lowest = origin.y;
        for xscan in 0..size.x {
            for zscan in 0..size.z {
                lowest = lowest.min(region.get_height(
                    HeightmapKind::OceanFloorWg,
                    low_corner.x + xscan,
                    low_corner.z + zscan,
                ));
            }
        }
        let target_y = (lowest - 15 - rng.next_int_bounded(10)).max(min_y + 10);
        let target = zs::geom::get_zero_position_with_transform(
            zs::BlockPos::new(low_corner.x, target_y, low_corner.z),
            Mirror::None,
            rot,
            base.size.x,
            base.size.z,
        );

        let bounds = base.bounding_box_with_settings(&settings, target);
        let mut empty_corners = 0;
        for &x in &[bounds.min_x, bounds.max_x] {
            for &y in &[bounds.min_y, bounds.max_y] {
                for &z in &[bounds.min_z, bounds.max_z] {
                    let state = region.get_block(BlockPos::new(x, y, z));
                    let ops = region.ops();
                    if ops.is_air(state) || ops.is_lava(state) || ops.is_water(state) {
                        empty_corners += 1;
                    }
                }
            }
        }
        if empty_corners > config.max_empty_corners_allowed {
            return false;
        }

        let mut world = RegionStructureWorld::new(region, None);
        zs::template_place::place_template(
            &mut world,
            &base,
            target,
            target,
            &mut settings,
            &mut *rng,
            260,
        );
        settings.processors = self.processors(&config.overlay_processors);
        zs::template_place::place_template(
            &mut world,
            &overlay,
            target,
            target,
            &mut settings,
            &mut *rng,
            260,
        );
        true
    }

    fn place_template(
        &self,
        config: &TemplateConfig,
        region: &mut GenerationRegion,
        rng: &mut WorldgenRandom,
        origin: BlockPos,
    ) -> bool {
        let entry = config.templates.get_random_or_throw(rng);
        let rotation = entry.rotations[rng.next_int_bounded(entry.rotations.len() as i32) as usize];
        let rot = zrot(rotation);
        let Ok(template): Result<Arc<StructureTemplate>, _> = self.templates.get(&entry.id) else {
            return false;
        };
        let (wx, _, wz) = zs::geom::dir_step(rot.rotate_dir(zs::Direction::West));
        let (nx, _, nz) = zs::geom::dir_step(rot.rotate_dir(zs::Direction::North));
        let pos = zs::BlockPos::new(
            origin.x + wx * (template.size.x / 2) + nx * (template.size.z / 2),
            origin.y,
            origin.z + wz * (template.size.x / 2) + nz * (template.size.z / 2),
        );
        let processors = config
            .processors
            .as_ref()
            .map(|j| self.processors(j))
            .unwrap_or_default();
        let mut settings = StructurePlaceSettings {
            rotation: rot,
            shared_random: true,
            processors,
            ..StructurePlaceSettings::default()
        };
        let mut world = RegionStructureWorld::new(region, None);
        zs::template_place::place_template(
            &mut world,
            &template,
            pos,
            pos,
            &mut settings,
            &mut *rng,
            3,
        )
    }
}
