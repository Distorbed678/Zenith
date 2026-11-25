//! `NoiseChunk`, `TerrainChunk`, `NoiseGenerator`, base column/height queries, and terrain fill
//! (`docs/specs/worldgen/density-functions.md` §6–§7,
//! `docs/specs/worldgen/aquifers-and-ore-veins.md` §4).

use std::sync::Arc;

use thiserror::Error;
use zenith_registry::{BlockStateTable, Identifier};

use crate::aquifer::{Aquifer, GlobalFluidPicker};
use crate::blender::Blender;
use crate::compiler::CompileError;
use crate::data::WorldgenData;
use crate::heightmap::{BlockPredicates, BlockTags, Heightmap, HeightmapKind, resolve_block_state};
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

/// Output of `NoiseGenerator::fill_chunk` (post-`doFill`, input to `buildSurface` and carvers).
///
/// Block states are stored in sections bottom-up (`0..section_count`), and within each section at
/// index `(local_y * 16 + local_z) * 16 + local_x` — matching `docs/specs/parity/terrain-chunks.md`.
#[derive(Debug, Clone)]
pub struct TerrainChunk {
    chunk_x: i32,
    chunk_z: i32,
    min_y: i32,
    height: i32,
    section_count: usize,
    air_id: u32,
    blocks: Vec<u32>,
    ocean_floor_wg: Heightmap,
    world_surface_wg: Heightmap,
    final_heightmaps: Option<[Heightmap; 4]>,
    post_processing: Vec<Vec<u16>>,
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
            ocean_floor_wg: Heightmap::new(HeightmapKind::OceanFloorWg, min_y),
            world_surface_wg: Heightmap::new(HeightmapKind::WorldSurfaceWg, min_y),
            final_heightmaps: None,
            post_processing: vec![Vec::new(); section_count],
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
        let min_y = self.settings().min_y();
        let height = self.settings().height();
        let air_id = self.predicates.air_id();
        let min_block_x = chunk_x.wrapping_shl(4);
        let min_block_z = chunk_z.wrapping_shl(4);

        let mut chunk = TerrainChunk::new(chunk_x, chunk_z, min_y, height, air_id);

        if height <= 0 {
            let dummy_vol =
                DensityVolume::unit(1, 1, 1, min_block_x, min_y, min_block_z).expect("valid");
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

        let h = height as usize;
        let volume = DensityVolume::unit(16, h, 16, min_block_x, min_y, min_block_z)
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

        let min_section_y = min_y >> 4;
        let default_block_id = self.default_block_id;

        // Exact doFill walk order (density-functions.md §7, aquifers-and-ore-veins.md §4.2):
        // z = 0..16 (outer), x = 0..16 (middle), y = height-1 down to 0 (inner).
        for z in 0..16usize {
            let block_z = min_block_z.wrapping_add(z as i32);
            for x in 0..16usize {
                let block_x = min_block_x.wrapping_add(x as i32);
                let col_base = (x + z * 16) * h;
                for y in (0..h).rev() {
                    let block_y = min_y + (y as i32);
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
