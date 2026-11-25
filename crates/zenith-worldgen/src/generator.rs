//! `NoiseChunk`, `TerrainChunk`, `NoiseGenerator`, base column/height queries, and terrain fill
//! (`docs/specs/worldgen/density-functions.md` §6–§7,
//! `docs/specs/worldgen/aquifers-and-ore-veins.md` §4).

use std::sync::Arc;

use thiserror::Error;
use zenith_biome::{
    BiomeSource, ClimateSampler, ClimateVolume, EndErosionClimate, PointClimate, SearchState,
    obfuscate_seed, section_biome_index, zoomed_quart_pos,
};
use zenith_math::LegacyRandom;
use zenith_registry::{BlockStateTable, Identifier};

use crate::aquifer::{Aquifer, GlobalFluidPicker};
use crate::biome_data::{BiomeBitSet, BiomeId, BiomeTable};
use crate::blender::Blender;
use crate::carver::{
    CARVER_CHUNK_RADIUS, CARVER_PROTECTED_BLOCKS_ON_TOP, CarvingMask, set_large_feature_seed,
};
use crate::compiler::CompileError;
use crate::data::{LoadError, WorldgenData};
use crate::heightmap::{BlockPredicates, BlockTags, Heightmap, HeightmapKind, resolve_block_state};
use crate::material::{MaterialInitError, MaterialSystem};
use crate::providers::WorldGenerationContext;
use crate::random_state::{PositionalFactory, RandomState};
use crate::settings::{NoiseGeneratorSettings, RouterField};
use crate::volume::{ContextSampler, DensityVolume, SamplerContext};

/// Identifier hashed on `RandomState.root_random` to derive the aquifer positional factory.
pub const AQUIFER_RANDOM_ID: &str = "minecraft:aquifer";

/// Packs section-relative block coordinates into a 16-bit post-processing offset
/// (`ProtoChunk.packOffsetCoordinates`, `aquifers-and-ore-veins.md` §2.5):
/// `(x & 15) | ((y & 15) << 4) | ((z & 15) << 8)`.
#[inline]
pub const fn pack_offset_coordinates(x: i32, y: i32, z: i32) -> u16 {
    ((x & 15) | ((y & 15) << 4) | ((z & 15) << 8)) as u16
}

/// Unpacks `(local_x, local_y, local_z)` from a [`pack_offset_coordinates`] value.
#[inline]
pub const fn unpack_offset_coordinates(packed: u16) -> (i32, i32, i32) {
    let p = packed as i32;
    (p & 15, (p >> 4) & 15, (p >> 8) & 15)
}

/// One queued chunk post-processing mark (section index + packed section-relative offset).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PostProcessingMark {
    /// Zero-based section index from the bottom of the chunk (`0 = lowest section`).
    pub section: i32,
    /// Packed section-relative offset from [`pack_offset_coordinates`].
    pub packed: u16,
}

/// Error returned when constructing a [`NoiseGenerator`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NoiseGeneratorError {
    /// Compiling the dimension's density functions or looking up noise settings failed.
    #[error(transparent)]
    Compile(#[from] CompileError),
    /// A required block identifier (`default_block`, `default_fluid`, `air`, `water`, or `lava`)
    /// was absent from the supplied [`BlockStateTable`].
    #[error("unknown block identifier '{id}' in BlockStateTable")]
    UnknownBlock {
        /// Missing block identifier.
        id: Identifier,
    },
}

/// Per-chunk (or per-column) noise sampling session (`NoiseChunk`, `density-functions.md` §6.1).
///
/// Owns the shared caching [`SamplerContext`] and [`Aquifer`] reused across `doFill`,
/// `buildSurface` (material rules / ore veins), and `generateCarvers`.
#[derive(Debug)]
pub struct NoiseChunk<'a> {
    random_state: &'a RandomState,
    volume: DensityVolume,
    ctx: SamplerContext,
    aquifer: Aquifer<'a>,
}

impl<'a> NoiseChunk<'a> {
    /// Creates a new `NoiseChunk` for `volume`, binding `beardifier`, `blender`, caches, and
    /// constructing the chunk's [`Aquifer`] (which pre-samples `surface_level` when enabled).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        random_state: &'a RandomState,
        global_fluid_picker: GlobalFluidPicker,
        predicates: &'a BlockPredicates,
        aquifer_random: Option<&'a PositionalFactory>,
        volume: DensityVolume,
        beardifier: Option<Arc<dyn ContextSampler>>,
        blender: &Blender,
    ) -> Self {
        let mut ctx = random_state.new_sampler_context(true);
        if let Some(b) = beardifier {
            ctx = ctx.with_beardifier(b);
        }
        if !blender.is_empty() {
            ctx = blender.attach_to_context(ctx, volume);
        }

        let aquifer = match (random_state.aquifer_samplers(), aquifer_random) {
            (Some(samplers), Some(rng_factory)) => Aquifer::noise_based(
                &mut ctx,
                samplers,
                rng_factory,
                volume,
                global_fluid_picker,
                predicates,
            ),
            _ => Aquifer::disabled(global_fluid_picker, predicates.air_id()),
        };

        Self {
            random_state,
            volume,
            ctx,
            aquifer,
        }
    }

    /// Returns this `NoiseChunk`'s [`DensityVolume`].
    #[inline]
    pub const fn volume(&self) -> DensityVolume {
        self.volume
    }

    /// Returns mutable access to the shared caching [`SamplerContext`] (for downstream material
    /// rules / ore veins and carvers).
    #[inline]
    pub fn context_mut(&mut self) -> &mut SamplerContext {
        &mut self.ctx
    }

    /// Evaluates `final_density` across this `NoiseChunk`'s `volume` into `out[..volume.len()]`.
    #[inline]
    pub fn sample_final_density_into(&mut self, out: &mut [f32]) {
        self.random_state
            .router_sampler(RouterField::FinalDensity)
            .sample_volume(&mut self.ctx, out, self.volume);
    }

    /// Evaluates `aquifer.computeSubstance(bx, by, bz, density)` using the shared `SamplerContext`.
    #[inline]
    pub fn compute_substance(&mut self, bx: i32, by: i32, bz: i32, density: f64) -> Option<u32> {
        self.aquifer
            .compute_substance(&mut self.ctx, bx, by, bz, density)
    }

    /// Returns whether the last [`Self::compute_substance`] call requested a fluid update mark.
    #[inline]
    pub const fn should_schedule_fluid_update(&self) -> bool {
        self.aquifer.should_schedule_fluid_update()
    }
}

/// Output of terrain generation (`NoiseGenerator::fill_chunk` or `TerrainGenerator::generate_chunk`).
///
/// Block states are stored in sections bottom-up (`0..section_count`), and within each section at
/// index `(local_y * 16 + local_z) * 16 + local_x` — matching `docs/specs/parity/terrain-chunks.md`.
/// Biomes (populated by [`TerrainGenerator`]) are stored in sections bottom-up (`section_count * 64`
/// quart entries), indexed within each section as `(qy * 4 + qz) * 4 + qx`.
#[derive(Debug, Clone)]
pub struct TerrainChunk {
    chunk_x: i32,
    chunk_z: i32,
    min_y: i32,
    height: i32,
    section_count: usize,
    air_id: u32,
    blocks: Vec<u32>,
    biome_palette: Vec<Identifier>,
    biome_indices: Vec<u16>,
    ocean_floor_wg: Heightmap,
    world_surface_wg: Heightmap,
    final_heightmaps: Option<[Heightmap; 4]>,
    post_processing: Vec<Vec<u16>>,
    block_tick_count: i32,
    fluid_tick_count: i32,
}

impl TerrainChunk {
    /// Creates an empty (all-air) `TerrainChunk` covering `[min_y, min_y + height)`.
    pub fn new(chunk_x: i32, chunk_z: i32, min_y: i32, height: i32, air_id: u32) -> Self {
        let section_count = if height > 0 {
            (height / 16) as usize
        } else {
            0
        };
        Self {
            chunk_x,
            chunk_z,
            min_y,
            height: height.max(0),
            section_count,
            air_id,
            blocks: vec![air_id; section_count * 4096],
            biome_palette: Vec::new(),
            biome_indices: Vec::new(),
            ocean_floor_wg: Heightmap::new(HeightmapKind::OceanFloorWg, min_y),
            world_surface_wg: Heightmap::new(HeightmapKind::WorldSurfaceWg, min_y),
            final_heightmaps: None,
            post_processing: vec![Vec::new(); section_count],
            block_tick_count: 0,
            fluid_tick_count: 0,
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

    /// Minimum block Y coordinate of the chunk.
    #[inline]
    pub const fn min_y(&self) -> i32 {
        self.min_y
    }

    /// Total vertical height of the chunk in blocks.
    #[inline]
    pub const fn height(&self) -> i32 {
        self.height
    }

    /// Number of 16×16×16 vertical sections (`height / 16`).
    #[inline]
    pub const fn section_count(&self) -> usize {
        self.section_count
    }

    /// Flat global block-state IDs for all sections bottom-up; within section `s`, index is
    /// `s * 4096 + (local_y * 16 + local_z) * 16 + local_x`.
    #[inline]
    pub fn blocks(&self) -> &[u32] {
        &self.blocks
    }

    /// Mutable access to the flat global block-state ID buffer.
    #[inline]
    pub fn blocks_mut(&mut self) -> &mut [u32] {
        &mut self.blocks
    }

    /// Global block-state IDs for section `section_index` (`0` = lowest section), indexed by
    /// `(local_y * 16 + local_z) * 16 + local_x`.
    #[inline]
    pub fn section(&self, section_index: usize) -> &[u32] {
        let start = section_index * 4096;
        &self.blocks[start..start + 4096]
    }

    /// Maps a world `block_y` to its section index (`0..section_count`), or `None` if out of bounds.
    #[inline]
    pub const fn section_index_of(&self, block_y: i32) -> Option<usize> {
        if block_y < self.min_y || block_y >= self.min_y + self.height {
            return None;
        }
        Some(((block_y >> 4) - (self.min_y >> 4)) as usize)
    }

    /// Flat index into [`Self::blocks`] for chunk-local `(local_x, block_y, local_z)`.
    #[inline]
    const fn block_flat_index(
        &self,
        local_x: usize,
        block_y: i32,
        local_z: usize,
    ) -> Option<usize> {
        let Some(sec) = self.section_index_of(block_y) else {
            return None;
        };
        let ly = (block_y & 15) as usize;
        Some(sec * 4096 + (ly * 16 + local_z) * 16 + local_x)
    }

    /// Reads the global block-state ID at chunk-local `(local_x, block_y, local_z)`, returning
    /// `air_id` if `block_y` is out of range.
    #[inline]
    pub fn get_block(&self, local_x: usize, block_y: i32, local_z: usize) -> u32 {
        match self.block_flat_index(local_x, block_y, local_z) {
            Some(idx) => self.blocks[idx],
            None => self.air_id,
        }
    }

    /// Writes `state_id` at chunk-local `(local_x, block_y, local_z)` without updating heightmaps.
    #[inline]
    pub fn set_block(&mut self, local_x: usize, block_y: i32, local_z: usize, state_id: u32) {
        if let Some(idx) = self.block_flat_index(local_x, block_y, local_z) {
            self.blocks[idx] = state_id;
        }
    }

    /// Writes `state_id` and live-updates `OCEAN_FLOOR_WG` and `WORLD_SURFACE_WG` (used by
    /// `buildSurface` and `applyCarvingMask` in `surface-and-carvers.md`).
    pub fn set_block_and_update_heightmaps(
        &mut self,
        local_x: usize,
        block_y: i32,
        local_z: usize,
        state_id: u32,
        predicates: &BlockPredicates,
    ) {
        let Some(idx) = self.block_flat_index(local_x, block_y, local_z) else {
            return;
        };
        self.blocks[idx] = state_id;
        let min_y = self.min_y;
        let height = self.height;
        let air_id = self.air_id;
        let blocks = &self.blocks;
        let lookup = |lx: usize, y: i32, lz: usize| -> u32 {
            if y < min_y || y >= min_y + height {
                return air_id;
            }
            let sec = ((y >> 4) - (min_y >> 4)) as usize;
            let ly = (y & 15) as usize;
            blocks[sec * 4096 + (ly * 16 + lz) * 16 + lx]
        };
        self.ocean_floor_wg
            .update(local_x, block_y, local_z, state_id, predicates, lookup);
        self.world_surface_wg
            .update(local_x, block_y, local_z, state_id, predicates, lookup);
    }

    /// Appends a post-processing mark for `(block_x, block_y, block_z)` if `block_y` is inside
    /// build height (not deduplicated, matching `ProtoChunk.markPosForPostProcessing`).
    #[inline]
    pub fn mark_pos_for_post_processing(&mut self, block_x: i32, block_y: i32, block_z: i32) {
        if let Some(sec) = self.section_index_of(block_y) {
            self.post_processing[sec].push(pack_offset_coordinates(block_x, block_y, block_z));
        }
    }

    /// Returns the `OCEAN_FLOOR_WG` heightmap maintained during `doFill`.
    #[inline]
    pub const fn ocean_floor_wg(&self) -> &Heightmap {
        &self.ocean_floor_wg
    }

    /// Returns the `WORLD_SURFACE_WG` heightmap maintained during `doFill`.
    #[inline]
    pub const fn world_surface_wg(&self) -> &Heightmap {
        &self.world_surface_wg
    }

    /// Looks up a heightmap by [`HeightmapKind`]. `OceanFloorWg` and `WorldSurfaceWg` are always
    /// present; the four `FINAL` heightmaps are present after calling [`Self::prime_final_heightmaps`].
    pub fn heightmap(&self, kind: HeightmapKind) -> Option<&Heightmap> {
        match kind {
            HeightmapKind::OceanFloorWg => Some(&self.ocean_floor_wg),
            HeightmapKind::WorldSurfaceWg => Some(&self.world_surface_wg),
            HeightmapKind::OceanFloor => self.final_heightmaps.as_ref().map(|h| &h[0]),
            HeightmapKind::WorldSurface => self.final_heightmaps.as_ref().map(|h| &h[1]),
            HeightmapKind::MotionBlocking => self.final_heightmaps.as_ref().map(|h| &h[2]),
            HeightmapKind::MotionBlockingNoLeaves => self.final_heightmaps.as_ref().map(|h| &h[3]),
        }
    }

    /// Primes the four `FINAL` heightmaps (`OCEAN_FLOOR`, `WORLD_SURFACE`, `MOTION_BLOCKING`,
    /// `MOTION_BLOCKING_NO_LEAVES`), matching `ChunkStatusTasks.buildTerrain` step 3.
    pub fn prime_final_heightmaps(&mut self, predicates: &BlockPredicates) {
        let min_y = self.min_y;
        let height = self.height;
        let air_id = self.air_id;
        let blocks = &self.blocks;
        let lookup = |lx: usize, y: i32, lz: usize| -> u32 {
            if y < min_y || y >= min_y + height {
                return air_id;
            }
            let sec = ((y >> 4) - (min_y >> 4)) as usize;
            let ly = (y & 15) as usize;
            blocks[sec * 4096 + (ly * 16 + lz) * 16 + lx]
        };
        self.final_heightmaps = Some([
            Heightmap::prime(HeightmapKind::OceanFloor, min_y, height, predicates, lookup),
            Heightmap::prime(
                HeightmapKind::WorldSurface,
                min_y,
                height,
                predicates,
                lookup,
            ),
            Heightmap::prime(
                HeightmapKind::MotionBlocking,
                min_y,
                height,
                predicates,
                lookup,
            ),
            Heightmap::prime(
                HeightmapKind::MotionBlockingNoLeaves,
                min_y,
                height,
                predicates,
                lookup,
            ),
        ]);
    }

    /// Per-section post-processing mark lists in insertion order (section `0` = lowest).
    #[inline]
    pub fn post_processing(&self) -> &[Vec<u16>] {
        &self.post_processing
    }

    /// Flattened post-processing marks in section-index order and list insertion order,
    /// matching `docs/specs/parity/terrain-chunks.md`.
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

    /// Biome identifier palette referenced by [`Self::biome_indices`].
    #[inline]
    pub fn biome_palette(&self) -> &[Identifier] {
        &self.biome_palette
    }

    /// Stored quart-resolution biome palette indices (`section_count * 64` entries when populated
    /// by [`TerrainGenerator`]; sections bottom-up, within each section `(qy * 4 + qz) * 4 + qx`).
    #[inline]
    pub fn biome_indices(&self) -> &[u16] {
        &self.biome_indices
    }

    /// Looks up the stored biome identifier at chunk-local quart `(local_qx, quart_y_from_bottom, local_qz)`.
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
        let pal_idx = *self.biome_indices.get(idx)? as usize;
        self.biome_palette.get(pal_idx)
    }

    /// Iterates every stored biome identifier in section bottom-up, `(qy, qz, qx)` order
    /// (matching `DumpedChunk::biomes` in `docs/specs/parity/terrain-chunks.md`).
    pub fn biomes_iter(&self) -> impl Iterator<Item = &Identifier> {
        self.biome_indices
            .iter()
            .map(|&idx| &self.biome_palette[idx as usize])
    }

    /// Scheduled block tick count held by this chunk (`0` after the `terrain` step).
    #[inline]
    pub const fn block_tick_count(&self) -> i32 {
        self.block_tick_count
    }

    /// Scheduled fluid tick count held by this chunk (`0` after the `terrain` step).
    #[inline]
    pub const fn fluid_tick_count(&self) -> i32 {
        self.fluid_tick_count
    }

    pub(crate) fn set_stored_biomes(&mut self, palette: Vec<Identifier>, indices: Vec<u16>) {
        self.biome_palette = palette;
        self.biome_indices = indices;
    }
}

/// Noise-based terrain generator (`NoiseBasedChunkGenerator` density fill + base column/height).
#[derive(Debug)]
pub struct NoiseGenerator {
    random_state: RandomState,
    default_block_id: u32,
    default_fluid_id: u32,
    global_fluid_picker: GlobalFluidPicker,
    aquifer_random: Option<PositionalFactory>,
    predicates: BlockPredicates,
}

impl NoiseGenerator {
    /// Constructs a [`NoiseGenerator`] for `noise_settings` and `seed`, resolving block states
    /// against `blocks` and heightmap predicates against `tags`.
    pub fn new(
        data: &WorldgenData,
        noise_settings: &Identifier,
        seed: i64,
        blocks: &BlockStateTable,
        tags: &BlockTags,
    ) -> Result<Self, NoiseGeneratorError> {
        let random_state = RandomState::new(data, noise_settings, seed)?;
        Self::from_random_state(random_state, blocks, tags)
    }

    /// Constructs a [`NoiseGenerator`] from an already-seeded [`RandomState`].
    pub fn from_random_state(
        random_state: RandomState,
        blocks: &BlockStateTable,
        tags: &BlockTags,
    ) -> Result<Self, NoiseGeneratorError> {
        let predicates = BlockPredicates::new(blocks, tags)
            .map_err(|id| NoiseGeneratorError::UnknownBlock { id })?;
        let settings = random_state.settings();
        let default_block_id = resolve_block_state(blocks, settings.default_block())
            .map_err(|id| NoiseGeneratorError::UnknownBlock { id })?;
        let default_fluid_id = resolve_block_state(blocks, settings.default_fluid())
            .map_err(|id| NoiseGeneratorError::UnknownBlock { id })?;

        let global_fluid_picker =
            GlobalFluidPicker::new(settings.sea_level(), default_fluid_id, predicates.lava_id());

        let aquifer_random = if settings.aquifers().is_some() {
            let aq_id = Identifier::parse(AQUIFER_RANDOM_ID).expect("valid static id");
            Some(random_state.positional_random_factory(&aq_id))
        } else {
            None
        };

        Ok(Self {
            random_state,
            default_block_id,
            default_fluid_id,
            global_fluid_picker,
            aquifer_random,
            predicates,
        })
    }

    /// Underlying seeded [`RandomState`].
    #[inline]
    pub const fn random_state(&self) -> &RandomState {
        &self.random_state
    }

    /// Active [`NoiseGeneratorSettings`].
    #[inline]
    pub const fn settings(&self) -> &NoiseGeneratorSettings {
        self.random_state.settings()
    }

    /// Global block-state ID of `default_block`.
    #[inline]
    pub const fn default_block_id(&self) -> u32 {
        self.default_block_id
    }

    /// Global block-state ID of `default_fluid`.
    #[inline]
    pub const fn default_fluid_id(&self) -> u32 {
        self.default_fluid_id
    }

    /// Global block-state ID of `minecraft:air` default state.
    #[inline]
    pub const fn air_id(&self) -> u32 {
        self.predicates.air_id()
    }

    /// Global fluid picker for this dimension.
    #[inline]
    pub const fn global_fluid_picker(&self) -> GlobalFluidPicker {
        self.global_fluid_picker
    }

    /// Precomputed block predicates (air, fluid, heightmap tags).
    #[inline]
    pub const fn predicates(&self) -> &BlockPredicates {
        &self.predicates
    }

    /// Evaluates `NoiseBasedChunkGenerator.getBaseColumn(x, z, ...)` (`density-functions.md` §7,
    /// `aquifers-and-ore-veins.md` §4.3).
    ///
    /// Returns global block-state IDs from `min_y` upward (`len == height`), reflecting pre-surface
    /// terrain fill (density + aquifer + default block/fluid; no structures, blending, surface
    /// rules, or carvers).
    pub fn base_column(&self, x: i32, z: i32) -> Vec<u32> {
        let min_y = self.settings().min_y();
        let height = self.settings().height();
        if height <= 0 {
            return Vec::new();
        }
        let h = height as usize;
        let volume = DensityVolume::unit(1, h, 1, x, min_y, z).expect("column volume is valid");
        let empty_blender = Blender::empty();
        let mut noise_chunk = NoiseChunk::new(
            &self.random_state,
            self.global_fluid_picker,
            &self.predicates,
            self.aquifer_random.as_ref(),
            volume,
            None,
            &empty_blender,
        );

        let mut density_buf = vec![0.0_f32; h];
        noise_chunk.sample_final_density_into(&mut density_buf);

        let mut column = vec![self.predicates.air_id(); h];
        for iy in (0..h).rev() {
            let block_y = min_y + (iy as i32);
            let density = f64::from(density_buf[iy]);
            let state = noise_chunk
                .compute_substance(x, block_y, z, density)
                .unwrap_or(self.default_block_id);
            column[iy] = state;
        }
        column
    }

    /// Evaluates `NoiseBasedChunkGenerator.getBaseHeight(x, z, kind, ...)` (`density-functions.md`
    /// §7, `aquifers-and-ore-veins.md` §4.3).
    ///
    /// Returns `block_y + 1` for the highest block matching `kind`, or `min_y` if none matches.
    pub fn base_height(&self, x: i32, z: i32, kind: HeightmapKind) -> i32 {
        let min_y = self.settings().min_y();
        let height = self.settings().height();
        if height <= 0 {
            return min_y;
        }
        let h = height as usize;
        let volume = DensityVolume::unit(1, h, 1, x, min_y, z).expect("column volume is valid");
        let empty_blender = Blender::empty();
        let mut noise_chunk = NoiseChunk::new(
            &self.random_state,
            self.global_fluid_picker,
            &self.predicates,
            self.aquifer_random.as_ref(),
            volume,
            None,
            &empty_blender,
        );

        let mut density_buf = vec![0.0_f32; h];
        noise_chunk.sample_final_density_into(&mut density_buf);

        for iy in (0..h).rev() {
            let block_y = min_y + (iy as i32);
            let density = f64::from(density_buf[iy]);
            let state = noise_chunk
                .compute_substance(x, block_y, z, density)
                .unwrap_or(self.default_block_id);
            if self.predicates.is_opaque(kind, state) {
                return block_y + 1;
            }
        }
        min_y
    }

    /// Executes `NoiseBasedChunkGenerator.doFill` for a fresh chunk `(chunk_x, chunk_z)` with no
    /// structures (`Beardifier::empty()`) and no old-chunk blending (`Blender::empty()`).
    ///
    /// Returns the filled [`TerrainChunk`] with global block-state IDs, maintained `OCEAN_FLOOR_WG`
    /// and `WORLD_SURFACE_WG` heightmaps, and fluid post-processing marks.
    ///
    /// Note: `BelowZeroRetrogen` applies only when upgrading pre-1.18 chunks and is a no-op for
    /// fresh 26.3 chunks (`density-functions.md` §7).
    pub fn fill_chunk(&self, chunk_x: i32, chunk_z: i32) -> TerrainChunk {
        let empty_blender = Blender::empty();
        let (chunk, _noise_chunk) =
            self.fill_chunk_with_hooks(chunk_x, chunk_z, None, &empty_blender);
        chunk
    }

    /// Executes `doFill` with explicit `beardifier` and `blender` hooks, returning both the filled
    /// [`TerrainChunk`] and the live [`NoiseChunk`] so downstream `buildSurface` (material rules /
    /// ore veins) and `generateCarvers` can reuse its shared caches and aquifer.
    pub fn fill_chunk_with_hooks(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        beardifier: Option<Arc<dyn ContextSampler>>,
        blender: &Blender,
    ) -> (TerrainChunk, NoiseChunk<'_>) {
        self.fill_chunk_with_bounds(
            chunk_x,
            chunk_z,
            self.settings().min_y(),
            self.settings().height(),
            beardifier,
            blender,
        )
    }

    /// Executes `doFill` into a chunk with explicit dimension vertical bounds `(chunk_min_y, chunk_height)`
    /// (which may exceed `settings.noise`, e.g., Nether and End chunks have height 256 while their
    /// noise settings have height 128).
    pub fn fill_chunk_with_bounds(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        chunk_min_y: i32,
        chunk_height: i32,
        beardifier: Option<Arc<dyn ContextSampler>>,
        blender: &Blender,
    ) -> (TerrainChunk, NoiseChunk<'_>) {
        let air_id = self.predicates.air_id();
        let min_block_x = chunk_x.wrapping_shl(4);
        let min_block_z = chunk_z.wrapping_shl(4);

        let mut chunk = TerrainChunk::new(chunk_x, chunk_z, chunk_min_y, chunk_height, air_id);

        let (gen_min_y, gen_height) = if chunk_height > 0 {
            self.settings()
                .noise_settings()
                .clamp_to_height_accessor(chunk_min_y, chunk_min_y + chunk_height - 1)
        } else {
            (chunk_min_y, 0)
        };

        if gen_height <= 0 {
            let dummy_vol =
                DensityVolume::unit(1, 1, 1, min_block_x, gen_min_y, min_block_z).expect("valid");
            let nc = NoiseChunk::new(
                &self.random_state,
                self.global_fluid_picker,
                &self.predicates,
                self.aquifer_random.as_ref(),
                dummy_vol,
                beardifier,
                blender,
            );
            return (chunk, nc);
        }

        let h = gen_height as usize;
        let volume = DensityVolume::unit(16, h, 16, min_block_x, gen_min_y, min_block_z)
            .expect("chunk volume is valid");
        let mut noise_chunk = NoiseChunk::new(
            &self.random_state,
            self.global_fluid_picker,
            &self.predicates,
            self.aquifer_random.as_ref(),
            volume,
            beardifier,
            blender,
        );

        let vol_len = volume.len();
        let mut density_buf = noise_chunk.ctx.acquire_buffer(vol_len);
        noise_chunk.sample_final_density_into(&mut density_buf[..vol_len]);

        let min_section_y = chunk_min_y >> 4;
        let default_block_id = self.default_block_id;

        // Exact doFill walk order (density-functions.md §7, aquifers-and-ore-veins.md §4.2):
        // z = 0..16 (outer), x = 0..16 (middle), y = gen_height-1 down to 0 (inner).
        for z in 0..16usize {
            let block_z = min_block_z.wrapping_add(z as i32);
            for x in 0..16usize {
                let block_x = min_block_x.wrapping_add(x as i32);
                let col_base = (x + z * 16) * h;
                for y in (0..h).rev() {
                    let block_y = gen_min_y + (y as i32);
                    let density = f64::from(density_buf[col_base + y]);
                    let state = noise_chunk
                        .compute_substance(block_x, block_y, block_z, density)
                        .unwrap_or(default_block_id);
                    if state != air_id {
                        let sec = ((block_y >> 4) - min_section_y) as usize;
                        let ly = (block_y & 15) as usize;
                        chunk.blocks[sec * 4096 + (ly * 16 + z) * 16 + x] = state;
                        chunk.ocean_floor_wg.update_top_down(
                            x,
                            block_y,
                            z,
                            state,
                            &self.predicates,
                        );
                        chunk.world_surface_wg.update_top_down(
                            x,
                            block_y,
                            z,
                            state,
                            &self.predicates,
                        );
                        if noise_chunk.should_schedule_fluid_update()
                            && self.predicates.has_fluid(state)
                        {
                            chunk.post_processing[sec]
                                .push(pack_offset_coordinates(block_x, block_y, block_z));
                        }
                    }
                }
            }
        }

        noise_chunk.ctx.release_buffer(density_buf);
        (chunk, noise_chunk)
    }
}

/// Error returned when constructing a [`TerrainGenerator`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TerrainGeneratorError {
    /// Compiling the dimension's density functions or noise settings failed.
    #[error(transparent)]
    Compile(#[from] CompileError),
    /// Resolving the dimension's material rule from `WorldgenData` failed.
    #[error(transparent)]
    Load(#[from] LoadError),
    /// Constructing the underlying [`NoiseGenerator`] failed.
    #[error(transparent)]
    NoiseGenerator(#[from] NoiseGeneratorError),
    /// A required block identifier was absent from the supplied [`BlockStateTable`].
    #[error("unknown block identifier '{id}' in BlockStateTable")]
    UnknownBlock {
        /// Missing block identifier.
        id: Identifier,
    },
    /// A `noise_threshold` condition referenced an unknown `worldgen/noise` identifier.
    #[error("unknown noise identifier '{id}' in WorldgenData")]
    UnknownNoise {
        /// Missing noise identifier.
        id: Identifier,
    },
}

impl From<MaterialInitError> for TerrainGeneratorError {
    fn from(e: MaterialInitError) -> Self {
        match e {
            MaterialInitError::Compile(c) => Self::Compile(c),
            MaterialInitError::UnknownBlock(id) => Self::UnknownBlock { id },
            MaterialInitError::UnknownNoise(id) => Self::UnknownNoise { id },
        }
    }
}

/// Complete 26.3 `ChunkStatus.TERRAIN` generator: biomes (`BIOMES` step), noise fill (`doFill`),
/// material rules & ore veins & special surface passes (`buildSurface`), cave/canyon carvers
/// (`generateCarvers` → `applyCarvingMask`), and final heightmap priming
/// (`docs/specs/worldgen/surface-and-carvers.md` §4).
#[derive(Debug)]
pub struct TerrainGenerator {
    noise_generator: NoiseGenerator,
    biome_source: BiomeSource,
    obfuscated_seed: i64,
    biome_table: BiomeTable,
    material_system: MaterialSystem,
    has_any_carvers: bool,
    /// When every biome producible by `biome_source` shares the exact same carver list (true for
    /// vanilla Overworld, Nether, and End), `run_carvers` can skip the 289 uncached `quartY = 0`
    /// climate evaluations with mathematically identical results.
    uniform_carvers: Option<Vec<Arc<crate::carver::Carver>>>,
    chunk_min_y: i32,
    chunk_height: i32,
}

impl TerrainGenerator {
    /// Constructs a [`TerrainGenerator`] for a dimension from its loaded [`WorldgenData`],
    /// [`BiomeSource`], `noise_settings` ID, `seed`, [`BlockStateTable`], and [`BlockTags`].
    ///
    /// Uses `noise_settings.noise`'s `(min_y, height)` as the default chunk vertical bounds;
    /// call [`Self::with_chunk_height`] when the dimension's `dimension_type` has taller chunks
    /// than its noise settings (e.g. Nether and End have `dimension_type.height = 256` with
    /// `noise.height = 128`).
    pub fn new(
        data: &WorldgenData,
        biome_source: BiomeSource,
        noise_settings: &Identifier,
        seed: i64,
        blocks: &BlockStateTable,
        tags: &BlockTags,
    ) -> Result<Self, TerrainGeneratorError> {
        let settings = data
            .noise_settings(noise_settings)
            .ok_or_else(|| CompileError::UnknownNoiseSettings {
                id: noise_settings.clone(),
            })?
            .clone();

        let material_rule =
            data.resolve_material_rule_ref(noise_settings, settings.material_rule())?;
        let mut ore_dfs = Vec::new();
        material_rule.collect_ore_vein_functions(&mut ore_dfs);

        let chunk_min_y = settings.min_y();
        let chunk_height = settings.height();

        let (random_state, ore_samplers) =
            RandomState::from_settings_with_extra(settings, seed, &ore_dfs)?;
        let noise_generator = NoiseGenerator::from_random_state(random_state, blocks, tags)?;

        let source_possible = biome_source.possible_biomes();
        let biome_table = BiomeTable::new(data.biomes(), source_possible.iter());

        let has_any_carvers = source_possible.iter().any(|id| {
            biome_table
                .get_id(id)
                .is_some_and(|bid| !biome_table.data(bid).carvers.is_empty())
        });

        let uniform_carvers = if let Some(first_id) = source_possible.first() {
            let first = biome_table
                .get_id(first_id)
                .map(|bid| &biome_table.data(bid).carvers);
            match first {
                Some(first_list)
                    if source_possible.iter().all(|id| {
                        biome_table
                            .get_id(id)
                            .is_some_and(|bid| &biome_table.data(bid).carvers == first_list)
                    }) =>
                {
                    Some(first_list.clone())
                }
                _ => None,
            }
        } else {
            None
        };

        let material_system = MaterialSystem::new(
            noise_generator.random_state(),
            data,
            blocks,
            &biome_table,
            &material_rule,
            &ore_samplers,
            noise_generator.default_block_id(),
        )?;

        Ok(Self {
            noise_generator,
            biome_source,
            obfuscated_seed: obfuscate_seed(seed),
            biome_table,
            material_system,
            has_any_carvers,
            uniform_carvers,
            chunk_min_y,
            chunk_height,
        })
    }

    /// Overrides the chunk vertical bounds `(min_y, height)` to match the dimension's
    /// `dimension_type` (`DimensionHeight`).
    #[inline]
    pub const fn with_chunk_height(mut self, min_y: i32, height: i32) -> Self {
        self.chunk_min_y = min_y;
        self.chunk_height = height;
        self
    }

    /// Underlying [`NoiseGenerator`] (also exposes `RandomState`, `NoiseGeneratorSettings`,
    /// `BlockPredicates`, `base_column`, and `base_height`).
    #[inline]
    pub const fn noise_generator(&self) -> &NoiseGenerator {
        &self.noise_generator
    }

    /// Active [`BiomeSource`].
    #[inline]
    pub const fn biome_source(&self) -> &BiomeSource {
        &self.biome_source
    }

    /// Interned [`BiomeTable`] for this dimension.
    #[inline]
    pub const fn biome_table(&self) -> &BiomeTable {
        &self.biome_table
    }

    /// Generates chunk `(chunk_x, chunk_z)` through the full 26.3 `terrain` chunk step with
    /// structures off (`Beardifier::empty()`) and no old-chunk blending (`Blender::empty()`).
    pub fn generate_chunk(&self, chunk_x: i32, chunk_z: i32) -> TerrainChunk {
        let empty_blender = Blender::empty();
        self.generate_chunk_with_hooks(chunk_x, chunk_z, None, &empty_blender)
    }

    /// Generates chunk `(chunk_x, chunk_z)` with explicit `beardifier` and `blender` hooks.
    pub fn generate_chunk_with_hooks(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        beardifier: Option<Arc<dyn ContextSampler>>,
        blender: &Blender,
    ) -> TerrainChunk {
        let rs = self.noise_generator.random_state();
        let settings = rs.settings();
        let chunk_min_y = self.chunk_min_y;
        let chunk_height = self.chunk_height;
        let predicates = self.noise_generator.predicates();

        if chunk_height <= 0 {
            let mut chunk = TerrainChunk::new(
                chunk_x,
                chunk_z,
                chunk_min_y,
                chunk_height,
                predicates.air_id(),
            );
            chunk.prime_final_heightmaps(predicates);
            return chunk;
        }

        // Step 0 (BIOMES): fill stored biomes for this chunk across all dimension sections
        // plus the 1-quart border needed by BiomeManager's 3D Voronoi zoom (6 × qh × 6 grid).
        let (palette, stored_indices, neighborhood_biomes, possible_biomes) =
            self.fill_biomes_with_border(chunk_x, chunk_z, chunk_min_y, chunk_height);

        // Step 1 (doFill): density fill + aquifers -> TerrainChunk + live NoiseChunk.
        let (mut chunk, mut noise_chunk) = self.noise_generator.fill_chunk_with_bounds(
            chunk_x,
            chunk_z,
            chunk_min_y,
            chunk_height,
            beardifier,
            blender,
        );
        chunk.set_stored_biomes(palette, stored_indices);

        // WorldGenerationContext (surface-and-carvers.md "Inputs and state"):
        // minGenY = max(chunk.minY, generator.minY), genDepth = min(chunk.height, generator.genDepth).
        let gen_ctx = WorldGenerationContext::new(
            chunk_min_y.max(settings.min_y()),
            chunk_height.min(settings.height()),
            settings.sea_level(),
        );
        let full_volume = noise_chunk.volume();

        // Step 2 (buildSurface): material rules, ore veins, badlands pillars, frozen ocean icebergs.
        let min_quart_x = (chunk_x << 2) - 1;
        let min_quart_z = (chunk_z << 2) - 1;
        let min_quart_y = chunk_min_y >> 2;
        let max_quart_y = ((chunk_min_y + chunk_height) >> 2) - 1;
        let obf = self.obfuscated_seed;

        let zoomed_from_neighborhood = |bx: i32, by: i32, bz: i32| -> BiomeId {
            let (qx, qy, qz) = zoomed_quart_pos(obf, bx, by, bz);
            let cqy = qy.clamp(min_quart_y, max_quart_y);
            let gx = (qx - min_quart_x) as usize;
            let gz = (qz - min_quart_z) as usize;
            let gy = (cqy - min_quart_y) as usize;
            neighborhood_biomes[(gy * 6 + gz) * 6 + gx]
        };

        self.material_system.build_surface(
            rs,
            &self.biome_table,
            gen_ctx,
            &mut chunk,
            full_volume,
            noise_chunk.context_mut(),
            predicates,
            Some(&possible_biomes),
            zoomed_from_neighborhood,
        );

        // Step 3 (generateCarvers -> applyCarvingMask).
        if self.has_any_carvers {
            self.run_carvers(gen_ctx, &mut chunk, &mut noise_chunk, chunk_x, chunk_z);
        }

        // Step 4: prime the four FINAL heightmaps (OCEAN_FLOOR, WORLD_SURFACE, MOTION_BLOCKING,
        // MOTION_BLOCKING_NO_LEAVES).
        chunk.prime_final_heightmaps(predicates);

        chunk
    }

    /// Computes the center chunk's stored biomes (`section_count * 64`) and the `6 × qh × 6`
    /// quart biome grid covering `[chunk_x*4 - 1 ..= chunk_x*4 + 4]` × `[min_quart_y .. min_quart_y + qh)`
    /// × `[chunk_z*4 - 1 ..= chunk_z*4 + 4]`, plus the `possibleBiomes` bitset.
    fn fill_biomes_with_border(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        min_y: i32,
        height: i32,
    ) -> (Vec<Identifier>, Vec<u16>, Vec<BiomeId>, BiomeBitSet) {
        let rs = self.noise_generator.random_state();
        let section_count = (height >> 4) as usize;
        let qh = (height >> 2) as usize;
        let min_quart_y = min_y >> 2;
        let min_quart_x = (chunk_x << 2) - 1;
        let min_quart_z = (chunk_z << 2) - 1;

        let mut grid = vec![BiomeId(0); 6 * qh * 6];
        let mut stored_bids = vec![BiomeId(0); section_count * 64];
        let mut possible = BiomeBitSet::new(self.biome_table.len());

        let lookup_id = |id: &Identifier| -> BiomeId {
            self.biome_table
                .get_id(id)
                .expect("every BiomeSource biome is interned in BiomeTable")
        };

        match &self.biome_source {
            BiomeSource::Fixed(src) => {
                let bid = lookup_id(&src.biome);
                grid.fill(bid);
                stored_bids.fill(bid);
                possible.insert(bid);
            }
            BiomeSource::Checkerboard(src) => {
                for gx in 0..6usize {
                    let qx = min_quart_x + gx as i32;
                    for gz in 0..6usize {
                        let qz = min_quart_z + gz as i32;
                        let bid = lookup_id(&src.noise_biome(qx, qz));
                        possible.insert(bid);
                        for gy in 0..qh {
                            grid[(gy * 6 + gz) * 6 + gx] = bid;
                        }
                        if (1..=4).contains(&gx) && (1..=4).contains(&gz) {
                            let lx = gx - 1;
                            let lz = gz - 1;
                            for sec in 0..section_count {
                                for ly in 0..4usize {
                                    stored_bids[sec * 64 + section_biome_index(lx, ly, lz)] = bid;
                                }
                            }
                        }
                    }
                }
            }
            BiomeSource::TheEnd(src) => {
                // In TheEndBiomeSource, all 16 horizontal quarts in a chunk share the exact same
                // (sample_x, block_y, sample_z) lookup, so we only query the 9 neighboring chunks.
                let mut chunk_cols = vec![BiomeId(0); 9 * qh];
                for dx in -1..=1_i32 {
                    for dz in -1..=1_i32 {
                        let cidx = ((dx + 1) * 3 + (dz + 1)) as usize;
                        let rep_qx = (chunk_x + dx) << 2;
                        let rep_qz = (chunk_z + dz) << 2;
                        for gy in 0..qh {
                            let qy = min_quart_y + gy as i32;
                            let id =
                                src.noise_biome_with_erosion(rep_qx, qy, rep_qz, |bx, by, bz| {
                                    rs.sample_router(RouterField::Erosion, bx, by, bz)
                                });
                            let bid = lookup_id(&id);
                            possible.insert(bid);
                            chunk_cols[cidx * qh + gy] = bid;
                        }
                    }
                }
                // Fill center chunk stored biomes from (dx=0, dz=0) -> cidx = 4.
                for sec in 0..section_count {
                    for ly in 0..4usize {
                        let bid = chunk_cols[4 * qh + sec * 4 + ly];
                        for lz in 0..4usize {
                            for lx in 0..4usize {
                                stored_bids[sec * 64 + section_biome_index(lx, ly, lz)] = bid;
                            }
                        }
                    }
                }
                // Fill 6 × qh × 6 grid.
                for gx in 0..6usize {
                    let dx: i32 = if gx == 0 {
                        -1
                    } else if gx == 5 {
                        1
                    } else {
                        0
                    };
                    for gz in 0..6usize {
                        let dz: i32 = if gz == 0 {
                            -1
                        } else if gz == 5 {
                            1
                        } else {
                            0
                        };
                        let cidx = ((dx + 1) * 3 + (dz + 1)) as usize;
                        for gy in 0..qh {
                            grid[(gy * 6 + gz) * 6 + gx] = chunk_cols[cidx * qh + gy];
                        }
                    }
                }
            }
            BiomeSource::MultiNoise(src) => {
                // Sample all 6 climate channels over the stride-4 6 × qh × 6 volume in one batch.
                let vol = DensityVolume::new(
                    [6, qh, 6],
                    [min_quart_x << 2, min_y, min_quart_z << 2],
                    [4, 4, 4],
                )
                .expect("6xqh x6 climate volume is valid");
                let channels = rs.sample_climate_volume(vol);
                let climate_vol =
                    ClimateVolume::new(min_quart_x, min_quart_y, min_quart_z, 6, qh, 6, channels)
                        .expect("climate volume dimensions match");

                let param_list = src.parameter_list();
                // 1. Center chunk stored biomes in exact vanilla fill_chunk_biomes order:
                // section ascending, lx outer, ly middle, lz inner, fresh SearchState.
                let mut state = SearchState::new();
                let center_qx0 = chunk_x << 2;
                let center_qz0 = chunk_z << 2;
                for sec in 0..section_count {
                    let sec_qy0 = min_quart_y + (sec as i32) * 4;
                    for lx in 0..4usize {
                        let qx = center_qx0 + lx as i32;
                        for ly in 0..4usize {
                            let qy = sec_qy0 + ly as i32;
                            let gy = sec * 4 + ly;
                            for lz in 0..4usize {
                                let qz = center_qz0 + lz as i32;
                                let target = climate_vol.sample(qx, qy, qz);
                                let id = param_list.find_value(target, &mut state);
                                let bid = lookup_id(id);
                                possible.insert(bid);
                                stored_bids[sec * 64 + section_biome_index(lx, ly, lz)] = bid;
                                grid[(gy * 6 + (lz + 1)) * 6 + (lx + 1)] = bid;
                            }
                        }
                    }
                }

                // 2. Fill the 20 border (gx, gz) quart columns.
                for gx in 0..6usize {
                    let qx = min_quart_x + gx as i32;
                    for gz in 0..6usize {
                        if (1..=4).contains(&gx) && (1..=4).contains(&gz) {
                            continue;
                        }
                        let qz = min_quart_z + gz as i32;
                        for gy in 0..qh {
                            let qy = min_quart_y + gy as i32;
                            let target = climate_vol.sample(qx, qy, qz);
                            let id = param_list.find_value(target, &mut state);
                            let bid = lookup_id(id);
                            possible.insert(bid);
                            grid[(gy * 6 + gz) * 6 + gx] = bid;
                        }
                    }
                }
            }
        }

        // Compress stored_bids into a local palette (first-seen order, matching ZTC1).
        let mut palette: Vec<Identifier> = Vec::new();
        let mut bid_to_pal: Vec<u16> = vec![u16::MAX; self.biome_table.len()];
        let mut stored_indices = Vec::with_capacity(stored_bids.len());
        for bid in stored_bids {
            let slot = &mut bid_to_pal[bid.0 as usize];
            if *slot == u16::MAX {
                let p = palette.len() as u16;
                palette.push(self.biome_table.identifier(bid).clone());
                *slot = p;
            }
            stored_indices.push(*slot);
        }

        (palette, stored_indices, grid, possible)
    }

    /// Uncached quart-level biome lookup (`biomeSource.createUncachedResolver(randomState)`).
    #[inline]
    fn uncached_biome(&self, qx: i32, qy: i32, qz: i32, state: &mut SearchState) -> BiomeId {
        let rs = self.noise_generator.random_state();
        let id = match &self.biome_source {
            BiomeSource::Fixed(src) => {
                return self.biome_table.get_id(&src.biome).expect("interned");
            }
            BiomeSource::Checkerboard(src) => src.noise_biome(qx, qz),
            BiomeSource::TheEnd(src) => {
                let sampler = EndErosionClimate::new(|bx, by, bz| {
                    rs.sample_router(RouterField::Erosion, bx, by, bz)
                });
                src.noise_biome(qx, qy, qz, &sampler)
            }
            BiomeSource::MultiNoise(src) => {
                let sampler = PointClimate::new(|bx, by, bz| rs.climate_point(bx, by, bz));
                src.noise_biome(qx, qy, qz, &sampler, state)
            }
        };
        self.biome_table.get_id(&id).expect("interned")
    }

    fn run_carvers(
        &self,
        gen_ctx: WorldGenerationContext,
        chunk: &mut TerrainChunk,
        noise_chunk: &mut NoiseChunk<'_>,
        chunk_x: i32,
        chunk_z: i32,
    ) {
        let rs = self.noise_generator.random_state();
        let seed = rs.seed();
        let predicates = self.noise_generator.predicates();

        let mask_min_y = gen_ctx.min_gen_y + 1;
        let mask_max_y = gen_ctx.min_gen_y + gen_ctx.gen_depth - 1 - CARVER_PROTECTED_BLOCKS_ON_TOP;
        if mask_min_y > mask_max_y {
            return;
        }

        let mut mask = CarvingMask::new(mask_min_y, mask_max_y);
        let mut rng = LegacyRandom::new(0);
        let mut search_state = SearchState::new();

        // Exact 17×17 neighborhood loop: dx = -8..=8 (outer), dz = -8..=8 (inner) (spec §3.2).
        for dx in -CARVER_CHUNK_RADIUS..=CARVER_CHUNK_RADIUS {
            let src_x = chunk_x.wrapping_add(dx);
            let src_qx = src_x.wrapping_shl(2);
            for dz in -CARVER_CHUNK_RADIUS..=CARVER_CHUNK_RADIUS {
                let src_z = chunk_z.wrapping_add(dz);
                let carvers: &[Arc<crate::carver::Carver>] = match &self.uniform_carvers {
                    Some(uniform) => uniform.as_slice(),
                    None => {
                        let src_qz = src_z.wrapping_shl(2);
                        let bid = self.uncached_biome(src_qx, 0, src_qz, &mut search_state);
                        self.biome_table.data(bid).carvers.as_slice()
                    }
                };
                for (index, carver) in carvers.iter().enumerate() {
                    set_large_feature_seed(&mut rng, seed.wrapping_add(index as i64), src_x, src_z);
                    if carver.is_start_chunk(&mut rng) {
                        carver.carve(gen_ctx, &mut rng, chunk_x, chunk_z, src_x, src_z, &mut mask);
                    }
                }
            }
        }

        if mask.is_empty() {
            return;
        }

        // Apply carving mask top-down per contiguous vertical run (spec §3.6).
        let min_block_x = chunk_x.wrapping_shl(4);
        let min_block_z = chunk_z.wrapping_shl(4);
        let dirt_id = predicates.dirt_id();
        let obf = self.obfuscated_seed;

        mask.visit_runs(|lx, lz, bottom_y, top_y| {
            let mut has_grass = false;
            let world_x = min_block_x.wrapping_add(lx as i32);
            let world_z = min_block_z.wrapping_add(lz as i32);
            let mut world_y = top_y;
            while world_y >= bottom_y {
                let block_state = chunk.get_block(lx, world_y, lz);
                if !predicates.is_uncarvable(block_state) {
                    if predicates.is_grass_or_mycelium(block_state) {
                        has_grass = true;
                    }
                    if let Some(state) =
                        noise_chunk.compute_substance(world_x, world_y, world_z, 0.0_f64)
                    {
                        chunk.set_block_and_update_heightmaps(lx, world_y, lz, state, predicates);
                        if noise_chunk.should_schedule_fluid_update() && predicates.has_fluid(state)
                        {
                            chunk.mark_pos_for_post_processing(world_x, world_y, world_z);
                        }
                        if has_grass && dirt_id == Some(chunk.get_block(lx, world_y - 1, lz)) {
                            let down_y = world_y - 1;
                            let under_fluid = predicates.has_fluid(state);
                            let top_mat = self.material_system.top_material(
                                rs,
                                &self.biome_table,
                                gen_ctx,
                                chunk,
                                noise_chunk.context_mut(),
                                world_x,
                                down_y,
                                world_z,
                                under_fluid,
                                |bx, by, bz| {
                                    let (qx, qy, qz) = zoomed_quart_pos(obf, bx, by, bz);
                                    self.uncached_biome(qx, qy, qz, &mut search_state)
                                },
                            );
                            if let Some(mat) = top_mat {
                                chunk.set_block_and_update_heightmaps(
                                    lx, down_y, lz, mat, predicates,
                                );
                                if predicates.has_fluid(mat) {
                                    chunk.mark_pos_for_post_processing(world_x, down_y, world_z);
                                }
                            }
                        }
                    }
                }
                world_y -= 1;
            }
        });
    }
}
