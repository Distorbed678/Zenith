//! Legacy (non-jigsaw) structure start & piece-graph generators (all 15 legacy types in 26.3)
//! (`docs/specs/worldgen/structures-legacy-pieces.md`).

pub mod end_city;
pub mod fortress;
pub mod mineshaft;
pub mod ocean_monument;
pub mod scattered;
pub mod stronghold;
pub mod woodland_mansion;

use std::collections::HashSet;

use zenith_math::{LegacyRandom, RandomSource};
use zenith_registry::Identifier;
use zenith_worldgen::{HeightmapKind, WorldGenerationContext};

use crate::data::{StructureData, StructureKind};
use crate::geom::{BlockPos, BoundingBox, BoundingBoxExt, Direction, Rotation};
use crate::template::TemplateManager;

/// One generated piece of a legacy structure (`StructurePiece`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyPiece {
    /// Registered `minecraft:worldgen/structure_piece` identifier (e.g. `"minecraft:msroom"`).
    pub kind: &'static str,
    /// Piece inclusive bounding box.
    pub bounding_box: BoundingBox,
    /// Horizontal orientation (`None` for world-space and template pieces).
    pub orientation: Option<Direction>,
    /// Generation depth (`"GD"`).
    pub gen_depth: i32,
}

impl LegacyPiece {
    /// Creates a new legacy piece.
    #[inline]
    pub const fn new(
        kind: &'static str,
        bounding_box: BoundingBox,
        orientation: Option<Direction>,
        gen_depth: i32,
    ) -> Self {
        Self {
            kind,
            bounding_box,
            orientation,
            gen_depth,
        }
    }

    /// Shifts this piece's bounding box by `(dx, dy, dz)`.
    #[inline]
    pub fn move_by(&mut self, dx: i32, dy: i32, dz: i32) {
        self.bounding_box = self.bounding_box.moved(dx, dy, dz);
    }
}

/// Callbacks required by legacy structure start generators.
pub trait LegacyGenContext {
    /// World seed.
    fn world_seed(&self) -> i64;
    /// Dimension chunk height accessor `minY`.
    fn min_y(&self) -> i32;
    /// Dimension chunk height accessor inclusive `maxY`.
    fn max_y(&self) -> i32;
    /// `chunkGenerator.getMinY()` (`noise_settings.min_y`).
    fn noise_min_y(&self) -> i32;
    /// `chunkGenerator.getSeaLevel()`.
    fn sea_level(&self) -> i32;
    /// `WorldGenerationContext` for `HeightProvider`.
    fn world_gen_context(&self) -> WorldGenerationContext;
    /// Loaded structure registries and biome tags.
    fn structure_data(&self) -> &StructureData;
    /// `.nbt` template manager.
    fn template_manager(&self) -> &TemplateManager;
    /// `Structure.GenerationContext.couldValidBiomeExistInTerrainColumn(bx, bz)`.
    fn could_valid_biome_exist_in_terrain_column(&self, bx: i32, bz: i32) -> bool;
    /// `Structure.GenerationContext.isValidBiome(stubPos)`.
    fn is_valid_biome(&mut self, pos: BlockPos) -> bool;
    /// `biomeResolver.getBiomesWithin(bx, by, bz, radius)` (for Ocean Monument).
    fn biomes_within(&mut self, bx: i32, by: i32, bz: i32, radius: i32) -> HashSet<Identifier>;
    /// `chunkGenerator.getBaseHeight(x, z, kind, ...)` (first free Y above highest opaque block).
    fn base_height(&self, x: i32, z: i32, kind: HeightmapKind) -> i32;
    /// `chunkGenerator.getFirstOccupiedHeight(x, z, kind, ...)` (`base_height - 1`).
    #[inline]
    fn first_occupied_height(&self, x: i32, z: i32, kind: HeightmapKind) -> i32 {
        self.base_height(x, z, kind) - 1
    }
    /// `chunkGenerator.getBaseColumn(x, z, ...)` (block state IDs from `noise_min_y` upward).
    fn base_column(&self, x: i32, z: i32) -> Vec<u32>;
    /// Whether `state_id` is air (`state.isAir()`).
    fn is_air(&self, state_id: u32) -> bool;
    /// Whether `state_id` is `minecraft:soul_sand`.
    fn is_soul_sand(&self, state_id: u32) -> bool;
    /// Whether `state_id.isFaceSturdy(UP, FULL)`.
    fn is_face_sturdy_up(&self, state_id: u32) -> bool;
    /// Whether `state_id` matches `kind`'s `isOpaque` predicate.
    fn is_heightmap_opaque(&self, kind: HeightmapKind, state_id: u32) -> bool;
}

/// Dispatches start/piece generation for any of the 15 legacy structure types.
pub fn generate_legacy_pieces(
    kind: &StructureKind,
    chunk_x: i32,
    chunk_z: i32,
    rng: &mut LegacyRandom,
    ctx: &mut dyn LegacyGenContext,
) -> Option<Vec<LegacyPiece>> {
    match kind {
        StructureKind::Jigsaw(_) => None,
        StructureKind::Mineshaft(mt) => mineshaft::generate(*mt, chunk_x, chunk_z, rng, ctx),
        StructureKind::Stronghold => stronghold::generate(chunk_x, chunk_z, rng, ctx),
        StructureKind::Fortress => fortress::generate(chunk_x, chunk_z, rng, ctx),
        StructureKind::OceanMonument => ocean_monument::generate(chunk_x, chunk_z, rng, ctx),
        StructureKind::WoodlandMansion => woodland_mansion::generate(chunk_x, chunk_z, rng, ctx),
        StructureKind::DesertPyramid => {
            scattered::generate_desert_pyramid(chunk_x, chunk_z, rng, ctx)
        }
        StructureKind::JungleTemple => {
            scattered::generate_jungle_temple(chunk_x, chunk_z, rng, ctx)
        }
        StructureKind::SwampHut => scattered::generate_swamp_hut(chunk_x, chunk_z, rng, ctx),
        StructureKind::Igloo => scattered::generate_igloo(chunk_x, chunk_z, rng, ctx),
        StructureKind::Shipwreck { is_beached } => {
            scattered::generate_shipwreck(*is_beached, chunk_x, chunk_z, rng, ctx)
        }
        StructureKind::OceanRuin {
            biome_temp,
            large_probability,
            cluster_probability,
        } => scattered::generate_ocean_ruin(
            *biome_temp,
            *large_probability,
            *cluster_probability,
            chunk_x,
            chunk_z,
            rng,
            ctx,
        ),
        StructureKind::BuriedTreasure => {
            scattered::generate_buried_treasure(chunk_x, chunk_z, rng, ctx)
        }
        StructureKind::EndCity => end_city::generate(chunk_x, chunk_z, rng, ctx),
        StructureKind::NetherFossil { height } => {
            scattered::generate_nether_fossil(height, chunk_x, chunk_z, rng, ctx)
        }
        StructureKind::RuinedPortal { setups } => {
            scattered::generate_ruined_portal(setups, chunk_x, chunk_z, rng, ctx)
        }
    }
}

// ---------------------------------------------------------------------------
// §1.2 & §1.3 Shared structure and pieces-builder helpers
// ---------------------------------------------------------------------------

/// `Structure.getCornerHeights(context, minX, sizeX, minZ, sizeZ)`.
pub fn get_corner_heights(
    ctx: &dyn LegacyGenContext,
    min_x: i32,
    size_x: i32,
    min_z: i32,
    size_z: i32,
) -> [i32; 4] {
    let hm = HeightmapKind::WorldSurfaceWg;
    [
        ctx.first_occupied_height(min_x, min_z, hm),
        ctx.first_occupied_height(min_x, min_z + size_z, hm),
        ctx.first_occupied_height(min_x + size_x, min_z, hm),
        ctx.first_occupied_height(min_x + size_x, min_z + size_z, hm),
    ]
}

/// `Structure.getLowestY(context, minX, minZ, sizeX, sizeZ)`.
pub fn get_lowest_y(
    ctx: &dyn LegacyGenContext,
    min_x: i32,
    min_z: i32,
    size_x: i32,
    size_z: i32,
) -> i32 {
    let h = get_corner_heights(ctx, min_x, size_x, min_z, size_z);
    h[0].min(h[1]).min(h[2]).min(h[3])
}

/// `Structure.getMeanFirstOccupiedHeight(context, minX, sizeX, minZ, sizeZ)`.
pub fn get_mean_first_occupied_height(
    ctx: &dyn LegacyGenContext,
    min_x: i32,
    size_x: i32,
    min_z: i32,
    size_z: i32,
) -> i32 {
    let h = get_corner_heights(ctx, min_x, size_x, min_z, size_z);
    (h[0] + h[1] + h[2] + h[3]) / 4
}

/// `Structure.getLowestYIn5by5Box(context, blockX, blockZ, rotation)`.
pub fn get_lowest_y_in_5by5_box(
    ctx: &dyn LegacyGenContext,
    block_x: i32,
    block_z: i32,
    rotation: Rotation,
) -> BlockPos {
    let (ox, oz) = match rotation {
        Rotation::None => (5, 5),
        Rotation::Clockwise90 => (-5, 5),
        Rotation::Clockwise180 => (-5, -5),
        Rotation::Counterclockwise90 => (5, -5),
    };
    let y = get_lowest_y(ctx, block_x, block_z, ox, oz);
    BlockPos::new(block_x, y, block_z)
}

/// `Structure.onTopOfChunkCenter` (§1.2): column pre-check + heightmap + biome check.
pub fn on_top_of_chunk_center<F>(
    chunk_x: i32,
    chunk_z: i32,
    heightmap: HeightmapKind,
    ctx: &mut dyn LegacyGenContext,
    build: F,
) -> Option<Vec<LegacyPiece>>
where
    F: FnOnce(&mut dyn LegacyGenContext) -> Vec<LegacyPiece>,
{
    let mid_x = (chunk_x << 4) + 8;
    let mid_z = (chunk_z << 4) + 8;
    if !ctx.could_valid_biome_exist_in_terrain_column(mid_x, mid_z) {
        return None;
    }
    let y = ctx.first_occupied_height(mid_x, mid_z, heightmap);
    if !ctx.is_valid_biome(BlockPos::new(mid_x, y, mid_z)) {
        return None;
    }
    Some(build(ctx))
}

/// Encapsulating bounding box of a non-empty piece slice.
pub fn pieces_bounding_box(pieces: &[LegacyPiece]) -> Option<BoundingBox> {
    let mut iter = pieces.iter();
    let mut bb = iter.next()?.bounding_box;
    for p in iter {
        bb.encapsulate(p.bounding_box);
    }
    Some(bb)
}

/// `StructurePiecesBuilder.offsetPiecesVertically(dy)`.
pub fn offset_pieces_vertically(pieces: &mut [LegacyPiece], dy: i32) {
    for p in pieces {
        p.move_by(0, dy, 0);
    }
}

/// `StructurePiecesBuilder.moveBelowSeaLevel(seaLevel, minY, random, offset)` (§1.3).
pub fn move_below_sea_level(
    pieces: &mut [LegacyPiece],
    sea_level: i32,
    min_y: i32,
    rng: &mut LegacyRandom,
    offset: i32,
) -> i32 {
    let Some(bb) = pieces_bounding_box(pieces) else {
        return 0;
    };
    let max_y = sea_level - offset;
    let mut y1_pos = bb.y_span() + min_y + 1;
    if y1_pos < max_y {
        y1_pos += rng.next_int_bounded(max_y - y1_pos);
    }
    let dy = y1_pos - bb.max_y;
    offset_pieces_vertically(pieces, dy);
    dy
}

/// `StructurePiecesBuilder.moveInsideHeights(random, lowestAllowed, highestAllowed)` (§1.3).
pub fn move_inside_heights(
    pieces: &mut [LegacyPiece],
    rng: &mut LegacyRandom,
    lowest_allowed: i32,
    highest_allowed: i32,
) {
    let Some(bb) = pieces_bounding_box(pieces) else {
        return;
    };
    let span = highest_allowed - lowest_allowed + 1 - bb.y_span();
    let y0_pos = if span > 1 {
        lowest_allowed + rng.next_int_bounded(span)
    } else {
        lowest_allowed
    };
    let dy = y0_pos - bb.min_y;
    offset_pieces_vertically(pieces, dy);
}

/// Returns `true` if any piece in `pieces` intersects `box_`.
pub fn has_collision(pieces: &[LegacyPiece], box_: BoundingBox) -> bool {
    pieces.iter().any(|p| p.bounding_box.intersects(box_))
}
