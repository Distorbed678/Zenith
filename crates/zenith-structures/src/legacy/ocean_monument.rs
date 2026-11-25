//! Ocean Monument (`minecraft:ocean_monument`) start generator and room-graph planner
//! (`docs/specs/worldgen/structures-legacy-pieces.md` §5).

use zenith_math::LegacyRandom;
use zenith_registry::Identifier;
use zenith_worldgen::HeightmapKind;

use crate::geom::{BoundingBox, BoundingBoxExt, random_horizontal_direction};
use crate::legacy::{LegacyGenContext, LegacyPiece, on_top_of_chunk_center};

/// Generates the top-level `MonumentBuilding` (`minecraft:omb`) piece for `minecraft:ocean_monument` (§5.2).
///
/// All 11 interior room pieces live inside `MonumentBuilding.childPieces` and are not exposed as
/// top-level entries in `StructureStart.pieces` (spec §5.2).
pub fn generate(
    chunk_x: i32,
    chunk_z: i32,
    rng: &mut LegacyRandom,
    ctx: &mut dyn LegacyGenContext,
) -> Option<Vec<LegacyPiece>> {
    let offset_x = (chunk_x << 4) + 9;
    let offset_z = (chunk_z << 4) + 9;
    let surrounding_tag =
        Identifier::with_default_namespace("required_ocean_monument_surrounding").expect("valid");
    let biomes = ctx.biomes_within(offset_x, ctx.sea_level(), offset_z, 29);
    for biome in &biomes {
        if !ctx.structure_data().biome_in_tag(&surrounding_tag, biome) {
            return None;
        }
    }

    on_top_of_chunk_center(chunk_x, chunk_z, HeightmapKind::OceanFloorWg, ctx, |_ctx| {
        let west = (chunk_x << 4) - 29;
        let north = (chunk_z << 4) - 29;
        let dir = random_horizontal_direction(rng);
        let bb = BoundingBox::make_box(west, 39, north, dir, 58, 23, 58);
        vec![LegacyPiece::new("minecraft:omb", bb, Some(dir), 0)]
    })
}
