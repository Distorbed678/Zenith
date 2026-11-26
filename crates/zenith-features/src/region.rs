//! `GenerationRegion` (`WorldGenRegion`) and `RegionChunk` holding a `3 × 3` window of terrain
//! chunks during the `minecraft:features` chunk step (`docs/specs/worldgen/features.md` §1.1).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use zenith_biome::{section_biome_index, zoomed_quart_pos};
use zenith_blocks::{FluidKind, ShapeKind, flag};
use zenith_registry::Identifier;
use zenith_worldgen::{
    BiomeId, Heightmap, HeightmapKind, PostProcessingMark, SeededRandom, TerrainChunk,
    pack_offset_coordinates,
};

use crate::block_ops::BlockOps;
use crate::rng::BlockPos;

/// Block-entity metadata placed during the `features` step (e.g. dungeon spawner, loot chest, end gateway).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeatureBlockEntity {
    /// Placeholder created by `ProtoChunk.setBlockState` for any block with `hasBlockEntity`.
    Dummy,
    /// Loot container (`BonusChestFeature`, `MonsterRoomFeature`).
    LootContainer {
        /// Loot table identifier (e.g. `minecraft:chests/simple_dungeon`).
        loot_table: Identifier,
        /// Loot table RNG seed drawn via `random.nextLong()`.
        loot_table_seed: i64,
    },
    /// Mob spawner (`MonsterRoomFeature`).
    Spawner {
        /// Spawned entity type ID (e.g. `minecraft:zombie`).
        entity_id: Identifier,
    },
    /// End gateway (`EndGatewayFeature`).
    EndGateway {
        /// Exit teleport position, if configured.
        exit: Option<BlockPos>,
        /// Whether teleportation is exact.
        exact: bool,
    },
}

/// Entity spawned during the `features` step (e.g. `minecraft:end_crystal` on `EndSpikeFeature`).
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureEntity {
    /// Entity type ID.
    pub id: Identifier,
    /// Exact world position `(x, y, z)`.
    pub pos: [f64; 3],
    /// Entity yaw in degrees.
    pub yaw: f32,
    /// Entity pitch in degrees.
    pub pitch: f32,
    /// Optional beam target (for End crystals).
    pub beam_target: Option<BlockPos>,
    /// Whether the entity is invulnerable.
    pub invulnerable: bool,
}

/// Mutable chunk state inside a [`GenerationRegion`], initialized from a [`TerrainChunk`].
#[derive(Debug, Clone)]
pub struct RegionChunk {
    chunk_x: i32,
    chunk_z: i32,
    min_y: i32,
    height: i32,
    section_count: usize,
    air_id: u32,
    blocks: Vec<u32>,
    biome_palette: Vec<Identifier>,
    biome_indices: Vec<u16>,
    /// All 6 heightmaps:
    /// `[OceanFloorWg, WorldSurfaceWg, OceanFloor, WorldSurface, MotionBlocking, MotionBlockingNoLeaves]`.
    heightmaps: [Heightmap; 6],
    post_processing: Vec<Vec<u16>>,
    block_ticks: HashSet<(i32, i32, i32, u32)>,
    fluid_ticks: HashSet<(i32, i32, i32, u8)>,
    initial_block_ticks: i32,
    initial_fluid_ticks: i32,
    block_entities: HashMap<BlockPos, FeatureBlockEntity>,
    entities: Vec<FeatureEntity>,
}

impl RegionChunk {
    /// Builds a [`RegionChunk`] from a completed `terrain`-status [`TerrainChunk`].
    pub fn from_terrain_chunk(chunk: &TerrainChunk, ops: &BlockOps) -> Self {
        let min_y = chunk.min_y();
        let height = chunk.height();
        let section_count = chunk.section_count();
        let air_id = ops.sid_air;
        let blocks = chunk.blocks().to_vec();

        let make_hm = |kind: HeightmapKind| -> Heightmap {
            if let Some(hm) = chunk.heightmap(kind) {
                hm.clone()
            } else {
                let b = &blocks;
                Heightmap::prime(kind, min_y, height, &ops.predicates, |lx, y, lz| {
                    if y < min_y || y >= min_y + height {
                        return air_id;
                    }
                    let sec = ((y >> 4) - (min_y >> 4)) as usize;
                    let ly = (y & 15) as usize;
                    b[sec * 4096 + (ly * 16 + lz) * 16 + lx]
                })
            }
        };

        let heightmaps = [
            make_hm(HeightmapKind::OceanFloorWg),
            make_hm(HeightmapKind::WorldSurfaceWg),
            make_hm(HeightmapKind::OceanFloor),
            make_hm(HeightmapKind::WorldSurface),
            make_hm(HeightmapKind::MotionBlocking),
            make_hm(HeightmapKind::MotionBlockingNoLeaves),
        ];

        Self {
            chunk_x: chunk.chunk_x(),
            chunk_z: chunk.chunk_z(),
            min_y,
            height,
            section_count,
            air_id,
            blocks,
            biome_palette: chunk.biome_palette().to_vec(),
            biome_indices: chunk.biome_indices().to_vec(),
            heightmaps,
            post_processing: chunk.post_processing().to_vec(),
            block_ticks: HashSet::new(),
            fluid_ticks: HashSet::new(),
            initial_block_ticks: chunk.block_tick_count(),
            initial_fluid_ticks: chunk.fluid_tick_count(),
            block_entities: HashMap::new(),
            entities: Vec::new(),
        }
    }

    /// Chunk X coordinate.
    #[inline]
    pub const fn chunk_x(&self) -> i32 {
        self.chunk_x
    }

    /// Chunk Z coordinate.
    #[inline]
    pub const fn chunk_z(&self) -> i32 {
        self.chunk_z
    }

    /// Minimum block Y.
    #[inline]
    pub const fn min_y(&self) -> i32 {
        self.min_y
    }

    /// Chunk vertical height in blocks.
    #[inline]
    pub const fn height(&self) -> i32 {
        self.height
    }

    /// Number of 16×16×16 vertical sections.
    #[inline]
    pub const fn section_count(&self) -> usize {
        self.section_count
    }

    /// Flat global block-state IDs (`section_count * 4096`).
    #[inline]
    pub fn blocks(&self) -> &[u32] {
        &self.blocks
    }

    /// Biome palette identifiers.
    #[inline]
    pub fn biome_palette(&self) -> &[Identifier] {
        &self.biome_palette
    }

    /// Stored quart biome palette indices (`section_count * 64`).
    #[inline]
    pub fn biome_indices(&self) -> &[u16] {
        &self.biome_indices
    }

    /// Iterates every stored biome identifier in section bottom-up, `(qy, qz, qx)` order.
    pub fn biomes_iter(&self) -> impl Iterator<Item = &Identifier> {
        self.biome_indices
            .iter()
            .map(|&idx| &self.biome_palette[idx as usize])
    }

    /// Looks up the stored biome at chunk-local quart `(local_qx, quart_y_from_bottom, local_qz)`.
    #[inline]
    pub fn biome_at_quart(
        &self,
        local_qx: usize,
        quart_y_from_bottom: usize,
        local_qz: usize,
    ) -> Option<&Identifier> {
        let sec = quart_y_from_bottom >> 2;
        let ly = quart_y_from_bottom & 3;
        let idx = sec * 64 + section_biome_index(local_qx, ly, local_qz);
        let pal = *self.biome_indices.get(idx)? as usize;
        self.biome_palette.get(pal)
    }

    /// Returns one of the 6 maintained heightmaps.
    #[inline]
    pub fn heightmap(&self, kind: HeightmapKind) -> &Heightmap {
        let idx = heightmap_index(kind);
        &self.heightmaps[idx]
    }

    /// Flattened post-processing marks in section-index order and insertion order.
    pub fn post_processing_marks(&self) -> Vec<PostProcessingMark> {
        let total: usize = self.post_processing.iter().map(Vec::len).sum();
        let mut out = Vec::with_capacity(total);
        for (sec, list) in self.post_processing.iter().enumerate() {
            for &packed in list {
                out.push(PostProcessingMark {
                    section: sec as i32,
                    packed,
                });
            }
        }
        out
    }

    /// Scheduled block tick count held by this chunk.
    #[inline]
    pub fn block_tick_count(&self) -> i32 {
        self.initial_block_ticks + self.block_ticks.len() as i32
    }

    /// Block ticks scheduled by the `features` step as `(x, y, z, block_protocol_id)`.
    pub fn scheduled_block_ticks(&self) -> impl Iterator<Item = (i32, i32, i32, u32)> + '_ {
        self.block_ticks.iter().copied()
    }

    /// Scheduled fluid tick count held by this chunk.
    #[inline]
    pub fn fluid_tick_count(&self) -> i32 {
        self.initial_fluid_ticks + self.fluid_ticks.len() as i32
    }

    /// Block entities recorded on this chunk.
    #[inline]
    pub fn block_entities(&self) -> &HashMap<BlockPos, FeatureBlockEntity> {
        &self.block_entities
    }

    /// Entities added to this chunk during feature placement.
    #[inline]
    pub fn entities(&self) -> &[FeatureEntity] {
        &self.entities
    }

    #[inline]
    const fn section_index_of(&self, block_y: i32) -> Option<usize> {
        if block_y < self.min_y || block_y >= self.min_y + self.height {
            return None;
        }
        Some(((block_y >> 4) - (self.min_y >> 4)) as usize)
    }

    #[inline]
    const fn flat_index(&self, lx: usize, block_y: i32, lz: usize) -> Option<usize> {
        let Some(sec) = self.section_index_of(block_y) else {
            return None;
        };
        let ly = (block_y & 15) as usize;
        Some(sec * 4096 + (ly * 16 + lz) * 16 + lx)
    }

    /// Reads the block state at chunk-local `(lx, y, lz)`, returning `air_id` if out of height bounds.
    #[inline]
    pub fn get_block(&self, lx: usize, block_y: i32, lz: usize) -> u32 {
        match self.flat_index(lx, block_y, lz) {
            Some(idx) => self.blocks[idx],
            None => self.air_id,
        }
    }

    /// Raw section block write (bypasses heightmap updates and post-processing; used by `OreFeature`).
    #[inline]
    pub fn set_block_bulk(&mut self, lx: usize, block_y: i32, lz: usize, state_id: u32) -> bool {
        let Some(idx) = self.flat_index(lx, block_y, lz) else {
            return false;
        };
        self.blocks[idx] = state_id;
        true
    }

    /// `ProtoChunk.setBlockState`: writes block, updates all 6 heightmaps, and returns previous state.
    pub fn set_block_state(
        &mut self,
        lx: usize,
        block_y: i32,
        lz: usize,
        state_id: u32,
        ops: &BlockOps,
    ) -> Option<u32> {
        let idx = self.flat_index(lx, block_y, lz)?;
        let old = self.blocks[idx];
        if old == state_id {
            return Some(old);
        }
        self.blocks[idx] = state_id;

        let min_y = self.min_y;
        let height = self.height;
        let air_id = self.air_id;
        let blocks = &self.blocks;
        let lookup = |x: usize, y: i32, z: usize| -> u32 {
            if y < min_y || y >= min_y + height {
                return air_id;
            }
            let sec = ((y >> 4) - (min_y >> 4)) as usize;
            let ly = (y & 15) as usize;
            blocks[sec * 4096 + (ly * 16 + z) * 16 + x]
        };

        // During the `features` step, `ProtoChunk.setBlockState` updates only the 4 `FINAL`
        // heightmaps (`OCEAN_FLOOR`, `WORLD_SURFACE`, `MOTION_BLOCKING`, `MOTION_BLOCKING_NO_LEAVES`);
        // `OCEAN_FLOOR_WG` and `WORLD_SURFACE_WG` remain frozen at their `terrain`-step values.
        for hm in &mut self.heightmaps[2..6] {
            hm.update(lx, block_y, lz, state_id, &ops.predicates, lookup);
        }
        Some(old)
    }

    /// Marks `(bx, by, bz)` for post-processing (not deduplicated, matching `ProtoChunk.markPosForPostProcessing`).
    #[inline]
    pub fn mark_pos_for_post_processing(&mut self, bx: i32, by: i32, bz: i32) {
        if let Some(sec) = self.section_index_of(by) {
            let packed = pack_offset_coordinates(bx, by, bz);
            self.post_processing[sec].push(packed);
        }
    }
}

#[inline]
const fn heightmap_index(kind: HeightmapKind) -> usize {
    match kind {
        HeightmapKind::OceanFloorWg => 0,
        HeightmapKind::WorldSurfaceWg => 1,
        HeightmapKind::OceanFloor => 2,
        HeightmapKind::WorldSurface => 3,
        HeightmapKind::MotionBlocking => 4,
        HeightmapKind::MotionBlockingNoLeaves => 5,
    }
}

/// `3 × 3` writable chunk window (`WorldGenRegion`, `features.md` §1.1) centered on `(center_x, center_z)`.
#[derive(Debug)]
pub struct GenerationRegion {
    center_x: i32,
    center_z: i32,
    /// 9 chunks in row-major `(dz + 1) * 3 + (dx + 1)` order.
    chunks: Box<[RegionChunk; 9]>,
    world_seed: i64,
    obfuscated_seed: i64,
    min_y: i32,
    height: i32,
    gen_min_y: i32,
    gen_depth: i32,
    sea_level: i32,
    /// `WorldGenRegion.random` (initialized from `minecraft:worldgen_region_random` at `(centerX*16, 0, centerZ*16)`).
    region_random: SeededRandom,
    ops: Arc<BlockOps>,
    /// Interned biome IDs per chunk palette for fast biome zoom lookups.
    biome_id_lookup: HashMap<Identifier, BiomeId>,
    /// Pre-mapped palette -> BiomeId per chunk.
    chunk_biome_palettes: [Vec<BiomeId>; 9],
    /// Whether the dimension has a sky-light engine (Overworld only). During `features` no light has
    /// been computed, and the vanilla sky-light lookup reports `15` for every unlit position.
    has_skylight: bool,
}

impl GenerationRegion {
    /// Constructs a [`GenerationRegion`] from 9 `terrain`-status chunks in `(dz = -1..=1, dx = -1..=1)` order,
    /// i.e. index `(dz + 1) * 3 + (dx + 1)` for `dx, dz in -1..=1`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        center_x: i32,
        center_z: i32,
        chunks: [RegionChunk; 9],
        world_seed: i64,
        obfuscated_seed: i64,
        gen_min_y: i32,
        gen_depth: i32,
        sea_level: i32,
        region_random: SeededRandom,
        ops: Arc<BlockOps>,
        biome_id_lookup: HashMap<Identifier, BiomeId>,
    ) -> Self {
        let min_y = chunks[4].min_y();
        let height = chunks[4].height();
        let chunk_biome_palettes = std::array::from_fn(|i| {
            chunks[i]
                .biome_palette()
                .iter()
                .map(|id| biome_id_lookup.get(id).copied().unwrap_or(BiomeId(0)))
                .collect()
        });
        Self {
            center_x,
            center_z,
            chunks: Box::new(chunks),
            world_seed,
            obfuscated_seed,
            min_y,
            height,
            gen_min_y,
            gen_depth,
            sea_level,
            region_random,
            ops,
            biome_id_lookup,
            chunk_biome_palettes,
            has_skylight: gen_min_y < 0,
        }
    }

    /// Overrides whether the dimension has sky light (default: `gen_min_y < 0`, i.e. the Overworld).
    #[must_use]
    pub fn with_skylight(mut self, has_skylight: bool) -> Self {
        self.has_skylight = has_skylight;
        self
    }

    /// Whether sky light exists in this dimension (see the field docs).
    #[inline]
    pub const fn has_skylight(&self) -> bool {
        self.has_skylight
    }

    /// `level.getRawBrightness(pos, 0)` during the `features` step: block light is `0` and sky light
    /// is `15` in dimensions with sky light (no light has been computed yet), else `0`.
    #[inline]
    pub const fn raw_brightness(&self) -> u8 {
        if self.has_skylight { 15 } else { 0 }
    }

    /// Center chunk X coordinate.
    #[inline]
    pub const fn center_x(&self) -> i32 {
        self.center_x
    }

    /// Center chunk Z coordinate.
    #[inline]
    pub const fn center_z(&self) -> i32 {
        self.center_z
    }

    /// World seed (`level.getSeed()`).
    #[inline]
    pub const fn world_seed(&self) -> i64 {
        self.world_seed
    }

    /// Minimum build height Y (`level.getMinY()`).
    #[inline]
    pub const fn min_y(&self) -> i32 {
        self.min_y
    }

    /// Total chunk height (`level.getHeight()`).
    #[inline]
    pub const fn height(&self) -> i32 {
        self.height
    }

    /// Maximum inclusive build height Y (`min_y + height - 1`).
    #[inline]
    pub const fn max_y(&self) -> i32 {
        self.min_y + self.height - 1
    }

    /// Minimum section Y (`min_y >> 4`).
    #[inline]
    pub const fn min_section_y(&self) -> i32 {
        self.min_y >> 4
    }

    /// Generator minimum Y (`chunkGenerator.getMinY()`).
    #[inline]
    pub const fn gen_min_y(&self) -> i32 {
        self.gen_min_y
    }

    /// Generator vertical depth (`chunkGenerator.getGenDepth()`).
    #[inline]
    pub const fn gen_depth(&self) -> i32 {
        self.gen_depth
    }

    /// Dimension sea level (`chunkGenerator.getSeaLevel()`).
    #[inline]
    pub const fn sea_level(&self) -> i32 {
        self.sea_level
    }

    /// Shared [`BlockOps`].
    #[inline]
    pub fn ops(&self) -> &BlockOps {
        &self.ops
    }

    /// Shared [`BlockOps`] `Arc`.
    #[inline]
    pub fn ops_arc(&self) -> &Arc<BlockOps> {
        &self.ops
    }

    /// Mutable access to `WorldGenRegion.getRandom()`.
    #[inline]
    pub fn region_random_mut(&mut self) -> &mut SeededRandom {
        &mut self.region_random
    }

    /// Returns the center [`RegionChunk`].
    #[inline]
    pub fn center_chunk(&self) -> &RegionChunk {
        &self.chunks[4]
    }

    /// Returns all 9 chunks in `(dz + 1) * 3 + (dx + 1)` order.
    #[inline]
    pub fn chunks(&self) -> &[RegionChunk; 9] {
        &self.chunks
    }

    /// Consumes the region and returns the decorated center [`RegionChunk`].
    pub fn into_center_chunk(self) -> RegionChunk {
        let [_, _, _, _, center, _, _, _, _] = *self.chunks;
        center
    }

    /// Returns `true` if `y` is outside `[min_y, min_y + height)`.
    #[inline]
    pub const fn is_outside_build_height(&self, y: i32) -> bool {
        y < self.min_y || y >= self.min_y + self.height
    }

    /// Returns `true` if `y` is inside `[min_y, min_y + height)`.
    #[inline]
    pub const fn is_inside_build_height(&self, y: i32) -> bool {
        !self.is_outside_build_height(y)
    }

    #[inline]
    fn chunk_slot(&self, cx: i32, cz: i32) -> Option<usize> {
        let dx = cx - self.center_x;
        let dz = cz - self.center_z;
        if (-1..=1).contains(&dx) && (-1..=1).contains(&dz) {
            Some(((dz + 1) * 3 + (dx + 1)) as usize)
        } else {
            None
        }
    }

    /// `WorldGenRegion.ensureCanWrite(pos)`: `true` iff `(pos.x >> 4, pos.z >> 4)` lies in the `3 × 3` window.
    #[inline]
    pub fn ensure_can_write(&self, pos: BlockPos) -> bool {
        self.chunk_slot(pos.chunk_x(), pos.chunk_z()).is_some()
    }

    /// Reads the block state at world `pos`, returning `minecraft:air` if outside build height or region.
    #[inline]
    pub fn get_block(&self, pos: BlockPos) -> u32 {
        let Some(slot) = self.chunk_slot(pos.chunk_x(), pos.chunk_z()) else {
            return self.ops.sid_air;
        };
        self.chunks[slot].get_block((pos.x & 15) as usize, pos.y, (pos.z & 15) as usize)
    }

    /// `level.isEmptyBlock(pos)` (`getBlockState(pos).isAir()`).
    #[inline]
    pub fn is_empty_block(&self, pos: BlockPos) -> bool {
        self.ops.is_air(self.get_block(pos))
    }

    /// `WorldGenRegion.setBlock(pos, state, flags)` (`features.md` §1.1 step 4).
    pub fn set_block(&mut self, pos: BlockPos, state_id: u32, flags: u32) -> bool {
        let Some(slot) = self.chunk_slot(pos.chunk_x(), pos.chunk_z()) else {
            return false;
        };
        let lx = (pos.x & 15) as usize;
        let lz = (pos.z & 15) as usize;
        let ops = Arc::clone(&self.ops);
        let old = self.chunks[slot].set_block_state(lx, pos.y, lz, state_id, &ops);

        // Block-entity lifecycle (spec §1.1 step 4.4).
        let new_has_be = ops.facts.state(state_id).has(flag::HAS_BLOCK_ENTITY);
        if new_has_be {
            self.chunks[slot]
                .block_entities
                .insert(pos, FeatureBlockEntity::Dummy);
        } else if let Some(old_id) = old
            && ops.facts.state(old_id).has(flag::HAS_BLOCK_ENTITY)
        {
            self.chunks[slot].block_entities.remove(&pos);
        }

        // Automatic post-processing mark when UPDATE_KNOWN_SHAPE (bit 16) is clear.
        if (flags & 16) == 0
            && let Some([dx, dy, dz]) = ops.facts.state(state_id).post_process
        {
            let pp = pos.offset(dx, dy, dz);
            self.mark_pos_for_post_processing(pp);
        }

        true
    }

    /// `Feature.safeSetBlock(level, pos, state, canReplace)` (`flags = 2`).
    #[inline]
    pub fn safe_set_block(
        &mut self,
        pos: BlockPos,
        state_id: u32,
        mut can_replace: impl FnMut(u32) -> bool,
    ) {
        let cur = self.get_block(pos);
        if can_replace(cur) {
            self.set_block(pos, state_id, 2);
        }
    }

    /// Raw section block write for `OreFeature` (`BulkSectionAccess` bypasses heightmaps & post-processing).
    #[inline]
    pub fn set_block_bulk(&mut self, pos: BlockPos, state_id: u32) -> bool {
        let Some(slot) = self.chunk_slot(pos.chunk_x(), pos.chunk_z()) else {
            return false;
        };
        self.chunks[slot].set_block_bulk(
            (pos.x & 15) as usize,
            pos.y,
            (pos.z & 15) as usize,
            state_id,
        )
    }

    /// `level.getChunk(pos).markPosForPostProcessing(pos)`.
    #[inline]
    pub fn mark_pos_for_post_processing(&mut self, pos: BlockPos) {
        if let Some(slot) = self.chunk_slot(pos.chunk_x(), pos.chunk_z()) {
            self.chunks[slot].mark_pos_for_post_processing(pos.x, pos.y, pos.z);
        }
    }

    /// `Feature.markAboveForPostProcessing(level, placePos)` (`features.md` Inputs & State).
    pub fn mark_above_for_post_processing(&mut self, place_pos: BlockPos) {
        let mut p = place_pos;
        for _ in 0..2 {
            p = p.above();
            if self.ops.is_air(self.get_block(p)) {
                return;
            }
            self.mark_pos_for_post_processing(p);
        }
    }

    /// `WorldGenRegion.getHeight(type, x, z)`: returns Y above highest matching block, or `min_y`.
    #[inline]
    pub fn get_height(&self, kind: HeightmapKind, x: i32, z: i32) -> i32 {
        let Some(slot) = self.chunk_slot(x >> 4, z >> 4) else {
            return self.min_y;
        };
        self.chunks[slot]
            .heightmap(kind)
            .first_available((x & 15) as usize, (z & 15) as usize)
    }

    /// `level.getHeightmapPos(type, pos)`.
    #[inline]
    pub fn get_heightmap_pos(&self, kind: HeightmapKind, pos: BlockPos) -> BlockPos {
        BlockPos::new(pos.x, self.get_height(kind, pos.x, pos.z), pos.z)
    }

    /// `level.getBiome(pos)` using 3D Voronoi zoom (`BiomeManager.getBiome`) over stored 3×3 chunk biomes.
    pub fn get_biome(&self, pos: BlockPos) -> BiomeId {
        let (qx, qy, qz) = zoomed_quart_pos(self.obfuscated_seed, pos.x, pos.y, pos.z);
        let min_qy = self.min_y >> 2;
        let max_qy = ((self.min_y + self.height) >> 2) - 1;
        let cqy = qy.clamp(min_qy, max_qy);
        let cx = qx >> 2;
        let cz = qz >> 2;
        // Clamp to the 3x3 region if zoomed quart slightly crosses edge (possible at region perimeter).
        let clamped_cx = cx.clamp(self.center_x - 1, self.center_x + 1);
        let clamped_cz = cz.clamp(self.center_z - 1, self.center_z + 1);
        let slot =
            ((clamped_cz - self.center_z + 1) * 3 + (clamped_cx - self.center_x + 1)) as usize;
        let local_qx = if clamped_cx == cx {
            (qx & 3) as usize
        } else if cx < clamped_cx {
            0
        } else {
            3
        };
        let local_qz = if clamped_cz == cz {
            (qz & 3) as usize
        } else if cz < clamped_cz {
            0
        } else {
            3
        };
        let qy_from_bot = (cqy - min_qy) as usize;
        let sec = qy_from_bot >> 2;
        let ly = qy_from_bot & 3;
        let idx = sec * 64 + section_biome_index(local_qx, ly, local_qz);
        let pal_idx = self.chunks[slot]
            .biome_indices
            .get(idx)
            .copied()
            .unwrap_or(0) as usize;
        self.chunk_biome_palettes[slot]
            .get(pal_idx)
            .copied()
            .unwrap_or(BiomeId(0))
    }

    /// `level.scheduleTick(pos, block, delay)`.
    #[inline]
    pub fn schedule_block_tick(&mut self, pos: BlockPos, block_id: u32, _delay: i32) {
        if let Some(slot) = self.chunk_slot(pos.chunk_x(), pos.chunk_z()) {
            self.chunks[slot]
                .block_ticks
                .insert((pos.x, pos.y, pos.z, block_id));
        }
    }

    /// `level.scheduleTick(pos, fluid, delay)`.
    #[inline]
    pub fn schedule_fluid_tick(&mut self, pos: BlockPos, fluid: FluidKind, _delay: i32) {
        if fluid == FluidKind::Empty {
            return;
        }
        if let Some(slot) = self.chunk_slot(pos.chunk_x(), pos.chunk_z()) {
            let fid = match fluid {
                FluidKind::Empty => 0,
                FluidKind::Water => 1,
                FluidKind::FlowingWater => 2,
                FluidKind::Lava => 3,
                FluidKind::FlowingLava => 4,
            };
            self.chunks[slot]
                .fluid_ticks
                .insert((pos.x, pos.y, pos.z, fid));
        }
    }

    /// Sets or updates a block entity at `pos` if `pos` is inside the write zone and already has a BE.
    pub fn set_block_entity(&mut self, pos: BlockPos, be: FeatureBlockEntity) -> bool {
        let Some(slot) = self.chunk_slot(pos.chunk_x(), pos.chunk_z()) else {
            return false;
        };
        if let Some(existing) = self.chunks[slot].block_entities.get_mut(&pos) {
            *existing = be;
            true
        } else {
            false
        }
    }

    /// Returns whether a block entity exists at `pos`.
    #[inline]
    pub fn has_block_entity_at(&self, pos: BlockPos) -> bool {
        self.chunk_slot(pos.chunk_x(), pos.chunk_z())
            .is_some_and(|slot| self.chunks[slot].block_entities.contains_key(&pos))
    }

    /// `level.addFreshEntity(entity)`.
    pub fn add_fresh_entity(&mut self, entity: FeatureEntity) {
        let cx = (entity.pos[0].floor() as i32) >> 4;
        let cz = (entity.pos[2].floor() as i32) >> 4;
        if let Some(slot) = self.chunk_slot(cx, cz) {
            self.chunks[slot].entities.push(entity);
        }
    }

    /// `level.isUnobstructed(null, Shapes.block().move(origin))`: during worldgen `WorldGenRegion`
    /// has no colliding entities (Wait: `isUnobstructed` checks entities, which `WorldGenRegion`
    /// always returns empty for!).
    #[inline]
    pub fn is_unobstructed(&self, _pos: BlockPos) -> bool {
        true
    }

    /// `level.getBrightness(LightLayer.BLOCK, pos)`: always 0 during `FEATURES` before `LIGHT`.
    #[inline]
    pub fn get_block_brightness(&self, _pos: BlockPos) -> u8 {
        0
    }

    /// Checks whether a block's collision shape at `pos` is empty (`getCollisionShape(level, pos).isEmpty()`).
    #[inline]
    pub fn is_collision_empty(&self, pos: BlockPos) -> bool {
        let s = self.get_block(pos);
        self.ops
            .facts
            .shape_at_origin(s, ShapeKind::Collision)
            .is_empty()
    }

    /// Returns all unique `BiomeId`s present in the stored palettes of all 9 chunks in the `3 × 3` region.
    pub fn collect_region_biomes(&self) -> Vec<BiomeId> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for chunk_pal in &self.chunk_biome_palettes {
            for &bid in chunk_pal {
                if seen.insert(bid) {
                    out.push(bid);
                }
            }
        }
        out
    }

    /// Looks up a `BiomeId` by identifier.
    #[inline]
    pub fn biome_id(&self, id: &Identifier) -> Option<BiomeId> {
        self.biome_id_lookup.get(id).copied()
    }
}
