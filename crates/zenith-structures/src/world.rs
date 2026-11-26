//! The world interface used when placing structure pieces during the `features` step
//! (`docs/specs/worldgen/structures-placement-and-jigsaw.md` §4.5 & §5.1,
//! `docs/specs/worldgen/structures-legacy-pieces.md` §1.4–§1.8).
//!
//! [`StructureWorld`] is deliberately small and owned by this crate, so the features crate can
//! adapt its generation region to it without a dependency cycle. Block entities and entities are
//! exchanged as NBT compounds ([`Compound`]) holding exactly the data the placement code writes
//! (loot-table id/seed, spawner `SpawnData`, sign text, template `nbt`); the adapter turns them
//! into whatever its chunk model stores. [`MemoryWorld`] is a small in-memory implementation used
//! by the unit tests (and handy for tooling).

use std::collections::HashMap;

use zenith_blocks::{BlockFacts, flag};
use zenith_codec::json::Json;
use zenith_math::RandomSource;
use zenith_nbt::Compound;
use zenith_registry::BlockStateTable;
use zenith_worldgen::{BlockTags, HeightmapKind};

use crate::geom::{BlockPos, BoundingBox, BoundingBoxExt};
use crate::template::ProcessorWorldView;

/// Object-safe subset of [`RandomSource`] used by piece placement (`WorldgenRandom` of the
/// decoration step, and `level.getRandom()`).
pub trait WorldgenRandom {
    /// `nextInt()`.
    fn next_int(&mut self) -> i32;
    /// `nextInt(bound)` (`bound > 0`).
    fn next_int_bounded(&mut self, bound: i32) -> i32;
    /// `nextLong()`.
    fn next_long(&mut self) -> i64;
    /// `nextBoolean()`.
    fn next_boolean(&mut self) -> bool;
    /// `nextFloat()`.
    fn next_float(&mut self) -> f32;
    /// `nextDouble()`.
    fn next_double(&mut self) -> f64;
    /// `nextGaussian()`.
    fn next_gaussian(&mut self) -> f64;
}

impl<R: RandomSource> WorldgenRandom for R {
    #[inline]
    fn next_int(&mut self) -> i32 {
        RandomSource::next_int(self)
    }
    #[inline]
    fn next_int_bounded(&mut self, bound: i32) -> i32 {
        RandomSource::next_int_bounded(self, bound)
    }
    #[inline]
    fn next_long(&mut self) -> i64 {
        RandomSource::next_long(self)
    }
    #[inline]
    fn next_boolean(&mut self) -> bool {
        RandomSource::next_boolean(self)
    }
    #[inline]
    fn next_float(&mut self) -> f32 {
        RandomSource::next_float(self)
    }
    #[inline]
    fn next_double(&mut self) -> f64 {
        RandomSource::next_double(self)
    }
    #[inline]
    fn next_gaussian(&mut self) -> f64 {
        RandomSource::next_gaussian(self)
    }
}

/// `Mth.nextInt(rng, min, max)` on a dynamic random (no draw when `min >= max`).
#[inline]
pub fn rng_next_int_between(rng: &mut dyn WorldgenRandom, min: i32, max: i32) -> i32 {
    if min >= max {
        min
    } else {
        rng.next_int_bounded(max - min + 1) + min
    }
}

/// `Mth.randomBetweenInclusive(rng, min, max)` on a dynamic random (always draws).
#[inline]
pub fn rng_between_inclusive(rng: &mut dyn WorldgenRandom, min: i32, max: i32) -> i32 {
    rng.next_int_bounded(max - min + 1) + min
}

/// `Util.shuffle(list, rng)` on a dynamic random.
pub fn rng_shuffle<T>(slice: &mut [T], rng: &mut dyn WorldgenRandom) {
    let n = slice.len();
    for i in (2..=n).rev() {
        let j = rng.next_int_bounded(i as i32) as usize;
        slice.swap(i - 1, j);
    }
}

/// `setBlock` flag: notify clients (`UPDATE_CLIENTS`).
pub const UPDATE_CLIENTS: u32 = 2;
/// `setBlock` flag: skip shape updates (`UPDATE_KNOWN_SHAPE`).
pub const UPDATE_KNOWN_SHAPE: u32 = 16;
/// `setBlock` flags used by the template pre-clear barrier (`820`).
pub const UPDATE_BARRIER_FLAGS: u32 = 820;

/// World access needed to place structure pieces. All positions are world coordinates.
///
/// Writes are only ever issued inside the chunk-bounded writable area handed to
/// [`crate::StructureStart::place_in_chunk`]; reads may go anywhere the implementation can
/// answer (return air outside it).
pub trait StructureWorld {
    /// Level seed (`level.getSeed()`), used by `capped` processors.
    fn world_seed(&self) -> i64;
    /// `heightAccessor.minY` (e.g. `-64` Overworld).
    fn min_y(&self) -> i32;
    /// `heightAccessor.maxY` (inclusive, e.g. `319` Overworld).
    fn max_y(&self) -> i32;
    /// `level.getSeaLevel()`.
    fn sea_level(&self) -> i32;
    /// Block-state table.
    fn blocks(&self) -> &BlockStateTable;
    /// Resolved block tags.
    fn block_tags(&self) -> &BlockTags;
    /// Per-state facts (shapes, sturdiness, replaceable, fluids, ...).
    fn facts(&self) -> &BlockFacts;
    /// `level.getBlockState(pos)` (global state id; air where unavailable).
    fn get_block_state(&self, pos: BlockPos) -> u32;
    /// `level.setBlock(pos, state, flags)`. Returns whether the block was written. The adapter
    /// owns block-entity placeholder bookkeeping and the automatic post-processing mark that
    /// `ProtoChunk` performs for `flags & 16 == 0`.
    fn set_block_state(&mut self, pos: BlockPos, state: u32, flags: u32) -> bool;
    /// `level.getHeight(kind, x, z)`: the first free Y above the highest matching block.
    fn get_height(&self, kind: HeightmapKind, x: i32, z: i32) -> i32;
    /// `level.scheduleTick(pos, fluid, delay)` for the fluid currently at `pos` (`delay` is `0`
    /// for legacy piece blocks and the fluid's own tick delay, e.g. `5` for water, when a
    /// template waterlogs a block).
    fn schedule_fluid_tick(&mut self, pos: BlockPos, delay: i32);
    /// `chunk.markPosForPostProcessing(pos)`.
    fn mark_pos_for_post_processing(&mut self, pos: BlockPos);
    /// Writes block-entity data for the block at `pos` (`loadWithComponents(nbt)` /
    /// `setLootTable` / `setEntityId`). The keys of `nbt` are **merged** into the block entity
    /// data already stored at `pos` (the entity is created if the block hosts one). Loot
    /// containers get `LootTable` (string) and `LootTableSeed` (long); spawners get
    /// `SpawnData.entity.id`; template block entities carry their stored `nbt` (with `x`/`y`/`z`
    /// omitted — the position is the argument).
    fn set_block_entity(&mut self, pos: BlockPos, nbt: Compound);
    /// `level.addFreshEntityWithPassengers(entity)`: the entity's NBT (`id`, `Pos`, `Rotation`, ...).
    fn add_entity(&mut self, nbt: Compound);
    /// `level.getRandom()` (the deterministic per-chunk region random).
    fn level_random(&mut self) -> &mut dyn WorldgenRandom;
    /// `PlacedFeature.place(level, generator, random, pos)` for `feature_pool_element`.
    /// `feature` is the element's feature reference (id string or inline JSON). The default
    /// implementation places nothing.
    fn place_feature(
        &mut self,
        feature: &Json,
        pos: BlockPos,
        random: &mut dyn WorldgenRandom,
    ) -> bool {
        let _ = (feature, pos, random);
        false
    }
    /// Noise biome at `pos` (`level.getBiome(pos)`), `None` when unknown. Used for biome-tag
    /// checks of mineshaft pieces.
    fn biome_at(&self, pos: BlockPos) -> Option<zenith_registry::Identifier> {
        let _ = pos;
        None
    }
    /// Whether this is a live `ServerLevel` (not a generation region): remaps `*_WG`
    /// heightmaps in `gravity` processors.
    fn is_live_server_level(&self) -> bool {
        false
    }
}

/// Adapter exposing a [`StructureWorld`] as a [`ProcessorWorldView`].
#[allow(missing_debug_implementations)]
pub struct ProcessorView<'a, W: StructureWorld + ?Sized>(pub &'a W);

impl<W: StructureWorld + ?Sized> ProcessorWorldView for ProcessorView<'_, W> {
    fn world_seed(&self) -> i64 {
        self.0.world_seed()
    }
    fn is_live_server_level(&self) -> bool {
        self.0.is_live_server_level()
    }
    fn get_block_state(&self, pos: BlockPos) -> u32 {
        self.0.get_block_state(pos)
    }
    fn get_height(&self, kind: HeightmapKind, x: i32, z: i32) -> i32 {
        self.0.get_height(kind, x, z)
    }
    fn blocks(&self) -> &BlockStateTable {
        self.0.blocks()
    }
    fn block_tags(&self) -> &BlockTags {
        self.0.block_tags()
    }
    fn block_facts(&self) -> Option<&BlockFacts> {
        Some(self.0.facts())
    }
}

/// `ChunkGenerator.getWritableArea(chunk)` for the chunk `(chunk_x, chunk_z)`:
/// `[chunkMinX, minY + 1, chunkMinZ] .. [chunkMinX + 15, maxY, chunkMinZ + 15]`.
pub fn writable_area(world: &dyn StructureWorld, chunk_x: i32, chunk_z: i32) -> BoundingBox {
    BoundingBox::from_coords(
        chunk_x << 4,
        world.min_y() + 1,
        chunk_z << 4,
        (chunk_x << 4) + 15,
        world.max_y(),
        (chunk_z << 4) + 15,
    )
}

/// A recorded `addEntity` call of [`MemoryWorld`].
pub type RecordedEntity = Compound;

/// Simple in-memory [`StructureWorld`]: a flat ground (`ground_block` up to and including
/// `ground_y - 1`, air above) with a sparse overlay of written blocks. Everything written is
/// recorded so tests can assert on it.
#[allow(missing_debug_implementations)]
pub struct MemoryWorld<'a> {
    blocks: &'a BlockStateTable,
    tags: &'a BlockTags,
    facts: &'a BlockFacts,
    seed: i64,
    min_y: i32,
    max_y: i32,
    ground_y: i32,
    ground_block: u32,
    air: u32,
    /// Blocks written so far (including air writes).
    pub overlay: HashMap<BlockPos, u32>,
    /// Block entities written so far.
    pub block_entities: HashMap<BlockPos, Compound>,
    /// Entities added so far.
    pub entities: Vec<RecordedEntity>,
    /// Positions marked for post-processing.
    pub post_process: Vec<BlockPos>,
    /// Fluid ticks scheduled.
    pub fluid_ticks: Vec<BlockPos>,
    /// Number of `place_feature` calls.
    pub features_placed: usize,
    /// Every `(pos, flags)` write, in order.
    pub writes: Vec<(BlockPos, u32, u32)>,
    random: zenith_math::LegacyRandom,
}

impl<'a> MemoryWorld<'a> {
    /// Creates a flat world. `ground_y` is the Y of the first air block above the ground.
    pub fn new(
        blocks: &'a BlockStateTable,
        tags: &'a BlockTags,
        facts: &'a BlockFacts,
        min_y: i32,
        max_y: i32,
        ground_y: i32,
    ) -> Self {
        let air = blocks.air_default_state().map_or(0, |s| s.global_id);
        let ground_block = crate::block_xform::default_state(blocks, "stone").unwrap_or(1);
        Self {
            blocks,
            tags,
            facts,
            seed: 0,
            min_y,
            max_y,
            ground_y,
            ground_block,
            air,
            overlay: HashMap::new(),
            block_entities: HashMap::new(),
            entities: Vec::new(),
            post_process: Vec::new(),
            fluid_ticks: Vec::new(),
            features_placed: 0,
            writes: Vec::new(),
            random: zenith_math::LegacyRandom::new(0),
        }
    }

    /// Sets the ground block.
    pub fn set_ground_block(&mut self, state: u32) {
        self.ground_block = state;
    }

    /// Ground level (first air Y).
    pub fn ground_y(&self) -> i32 {
        self.ground_y
    }

    /// Reads the current block (alias of [`StructureWorld::get_block_state`]).
    pub fn block_at(&self, pos: BlockPos) -> u32 {
        StructureWorld::get_block_state(self, pos)
    }

    /// Air state id.
    pub fn air(&self) -> u32 {
        self.air
    }

    /// Whether the block at `pos` is air.
    pub fn is_air_at(&self, pos: BlockPos) -> bool {
        self.facts.state(self.block_at(pos)).has(flag::IS_AIR)
    }

    /// Sets the level seed.
    pub fn with_seed(mut self, seed: i64) -> Self {
        self.seed = seed;
        self
    }
}

impl StructureWorld for MemoryWorld<'_> {
    fn world_seed(&self) -> i64 {
        self.seed
    }
    fn min_y(&self) -> i32 {
        self.min_y
    }
    fn max_y(&self) -> i32 {
        self.max_y
    }
    fn sea_level(&self) -> i32 {
        self.ground_y - 1
    }
    fn blocks(&self) -> &BlockStateTable {
        self.blocks
    }
    fn block_tags(&self) -> &BlockTags {
        self.tags
    }
    fn facts(&self) -> &BlockFacts {
        self.facts
    }
    fn get_block_state(&self, pos: BlockPos) -> u32 {
        if pos.y < self.min_y || pos.y > self.max_y {
            return self.air;
        }
        if let Some(s) = self.overlay.get(&pos) {
            return *s;
        }
        if pos.y < self.ground_y {
            self.ground_block
        } else {
            self.air
        }
    }
    fn set_block_state(&mut self, pos: BlockPos, state: u32, flags: u32) -> bool {
        if pos.y < self.min_y || pos.y > self.max_y {
            return false;
        }
        self.overlay.insert(pos, state);
        self.writes.push((pos, state, flags));
        if self.facts.state(state).has(flag::HAS_BLOCK_ENTITY) {
            self.block_entities.entry(pos).or_default();
        } else {
            self.block_entities.remove(&pos);
        }
        true
    }
    fn get_height(&self, kind: HeightmapKind, x: i32, z: i32) -> i32 {
        let mut y = self.max_y;
        while y >= self.min_y {
            let st = self
                .facts
                .state(self.get_block_state(BlockPos::new(x, y, z)));
            let blocks_motion = st.has(flag::BLOCKS_MOTION_IN_HEIGHTMAP);
            let no_leaves = st.has(flag::BLOCKS_MOTION_IN_HEIGHTMAP_NO_LEAVES);
            let fluid = !st.fluid.is_empty();
            let hit = match kind {
                HeightmapKind::WorldSurface | HeightmapKind::WorldSurfaceWg => {
                    !st.has(flag::IS_AIR)
                }
                HeightmapKind::OceanFloor | HeightmapKind::OceanFloorWg => blocks_motion,
                HeightmapKind::MotionBlocking => blocks_motion || fluid,
                HeightmapKind::MotionBlockingNoLeaves => no_leaves || fluid,
            };
            if hit {
                return y + 1;
            }
            y -= 1;
        }
        self.min_y
    }
    fn schedule_fluid_tick(&mut self, pos: BlockPos, _delay: i32) {
        self.fluid_ticks.push(pos);
    }
    fn mark_pos_for_post_processing(&mut self, pos: BlockPos) {
        self.post_process.push(pos);
    }
    fn set_block_entity(&mut self, pos: BlockPos, nbt: Compound) {
        let entry = self.block_entities.entry(pos).or_default();
        for (k, v) in nbt.iter() {
            entry.insert(k.clone(), v.clone());
        }
    }
    fn add_entity(&mut self, nbt: Compound) {
        self.entities.push(nbt);
    }
    fn level_random(&mut self) -> &mut dyn WorldgenRandom {
        &mut self.random
    }
    fn place_feature(
        &mut self,
        _feature: &Json,
        _pos: BlockPos,
        _random: &mut dyn WorldgenRandom,
    ) -> bool {
        self.features_placed += 1;
        true
    }
}

/// Whether `pos` is inside the box (free function used widely by piece code).
#[inline]
pub fn inside(bb: &BoundingBox, pos: BlockPos) -> bool {
    bb.is_inside(pos)
}

#[cfg(test)]
mod tests {
    use zenith_math::LegacyRandom;

    use super::*;

    #[test]
    fn dynamic_shuffle_matches_generic_shuffle() {
        let mut a: Vec<i32> = (0..20).collect();
        let mut b = a.clone();
        let mut r1 = LegacyRandom::new(42);
        let mut r2 = LegacyRandom::new(42);
        rng_shuffle(&mut a, &mut r1);
        crate::geom::shuffle_slice(&mut b, &mut r2);
        assert_eq!(a, b);
    }

    #[test]
    fn between_helpers_follow_spec_draw_rules() {
        let mut r = LegacyRandom::new(7);
        let mut r2 = LegacyRandom::new(7);
        // `Mth.nextInt` draws nothing when min >= max.
        assert_eq!(rng_next_int_between(&mut r, 5, 5), 5);
        assert_eq!(
            WorldgenRandom::next_int(&mut r),
            WorldgenRandom::next_int(&mut r2)
        );
        // `randomBetweenInclusive` always draws.
        let v = rng_between_inclusive(&mut r, 3, 3);
        assert_eq!(v, 3);
        let _ = WorldgenRandom::next_int_bounded(&mut r2, 1);
        assert_eq!(
            WorldgenRandom::next_int(&mut r),
            WorldgenRandom::next_int(&mut r2)
        );
    }
}
