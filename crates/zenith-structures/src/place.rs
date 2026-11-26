//! Per-chunk block placement of structure starts in the `features` step
//! (`docs/specs/worldgen/structures-placement-and-jigsaw.md` §5.1,
//! `docs/specs/worldgen/structures-legacy-pieces.md` §1.8).
//!
//! Drive it once per chunk and per decoration step: for every structure of the step (registry
//! order) seed the decoration random with `set_feature_seed(decoration_seed, index, step)`, then
//! call [`StructureStart::place_in_chunk`] for each start referencing the chunk.

use zenith_registry::Identifier;
use zenith_worldgen::HeightmapKind;

use crate::data::{
    LiquidSettings, ProcessorListRef, Projection, StructureData, StructureKind,
    StructurePoolElement, StructureProcessor,
};
use crate::generator::{StructurePiece, StructureStart};
use crate::geom::{BlockPos, BoundingBox, BoundingBoxExt, Rotation};
use crate::piece_data::PieceData;
use crate::pieces;
use crate::template::{StructurePlaceSettings, TemplateManager};
use crate::template_place::place_template;
use crate::world::{StructureWorld, WorldgenRandom};

/// Registries needed while placing pieces.
#[derive(Debug, Clone, Copy)]
pub struct PlacementEnv<'a> {
    /// Loaded structures, pools and processor lists.
    pub data: &'a StructureData,
    /// `.nbt` template loader.
    pub templates: &'a TemplateManager,
}

/// Everything a piece's `postProcess` receives.
#[allow(missing_debug_implementations)]
pub struct PieceCall<'a> {
    /// Registries.
    pub env: &'a PlacementEnv<'a>,
    /// World.
    pub world: &'a mut dyn StructureWorld,
    /// The step's `WorldgenRandom` (seeded per structure with `setFeatureSeed`).
    pub random: &'a mut dyn WorldgenRandom,
    /// The shared writable area (`chunkBB`); a few pieces encapsulate into it.
    pub chunk_bb: &'a mut BoundingBox,
    /// `referencePos` of the start (`pieces[0]` centre, `minY`).
    pub reference_pos: BlockPos,
    /// Chunk of the start (`structure_starts` chunk); used to regenerate ocean monument rooms.
    pub start_chunk: (i32, i32),
}

impl StructureStart {
    /// `StructureStart.placeInChunk(level, structureManager, generator, random, chunkBB, chunkPos)`
    /// followed by `structure.afterPlace(...)`.
    ///
    /// `&mut self`: pieces keep state between chunk calls (snapped heights, placed-chest flags,
    /// shifted template positions), exactly like vanilla's in-memory pieces. `writable_area`
    /// is the chunk's writable area (`[chunkMinX, minY + 1, chunkMinZ] .. [chunkMinX + 15, maxY,
    /// chunkMinZ + 15]`, see [`crate::world::writable_area`]); ruined portals and nether fossils
    /// enlarge it, and the enlarged box stays in effect for later structures of the same chunk.
    pub fn place_in_chunk(
        &mut self,
        env: &PlacementEnv<'_>,
        world: &mut dyn StructureWorld,
        random: &mut dyn WorldgenRandom,
        writable_area: &mut BoundingBox,
    ) {
        if self.pieces.is_empty() {
            return;
        }
        let first = self.pieces[0].bounding_box;
        let c = first.center();
        let reference_pos = BlockPos::new(c.x, first.min_y, c.z);
        for i in 0..self.pieces.len() {
            if !self.pieces[i].bounding_box.intersects(*writable_area) {
                continue;
            }
            let mut call = PieceCall {
                env,
                world: &mut *world,
                random: &mut *random,
                chunk_bb: &mut *writable_area,
                reference_pos,
                start_chunk: (self.chunk_x, self.chunk_z),
            };
            post_process_piece(&mut self.pieces[i], &mut call);
        }
        let kind = env.data.structure(&self.structure_id).map(|d| &d.kind);
        match kind {
            Some(StructureKind::DesertPyramid) => {
                pieces::scattered::desert_after_place(&self.pieces, world, writable_area);
            }
            Some(StructureKind::WoodlandMansion) => {
                pieces::template_pieces::mansion_after_place(&self.pieces, world, writable_area);
            }
            _ => {}
        }
    }
}

/// `StructurePiece.postProcess` dispatch.
pub fn post_process_piece(piece: &mut StructurePiece, call: &mut PieceCall<'_>) {
    if let Some(jig) = &piece.jigsaw {
        let element = jig.element.clone();
        let (position, rotation, liquid) = (jig.position, jig.rotation, jig.liquid_settings);
        let chunk_bb = *call.chunk_bb;
        place_pool_element(call, &element, position, rotation, chunk_bb, liquid, false);
        return;
    }
    match &piece.data {
        PieceData::None | PieceData::BuriedTreasure => {
            if piece.kind == "minecraft:btp" {
                pieces::scattered::buried_treasure(piece, call);
            }
        }
        PieceData::Template(_) => pieces::template_pieces::post_process(piece, call),
        PieceData::DesertPyramid(_) => pieces::scattered::desert_pyramid(piece, call),
        PieceData::JungleTemple(_) => pieces::scattered::jungle_temple(piece, call),
        PieceData::SwampHut(_) => pieces::scattered::swamp_hut(piece, call),
        PieceData::Mineshaft(_) => pieces::mineshaft::post_process(piece, call),
        PieceData::Stronghold(_) => pieces::stronghold::post_process(piece, call),
        PieceData::Fortress(_) => pieces::fortress::post_process(piece, call),
        PieceData::Monument(_) => pieces::monument::post_process(piece, call),
    }
}

fn structure_block_ignore() -> StructureProcessor {
    StructureProcessor::BlockIgnore {
        blocks: vec![Identifier::with_default_namespace("structure_block").expect("valid")],
    }
}

fn structure_and_air_ignore() -> StructureProcessor {
    StructureProcessor::BlockIgnore {
        blocks: vec![
            Identifier::with_default_namespace("air").expect("valid"),
            Identifier::with_default_namespace("structure_block").expect("valid"),
        ],
    }
}

/// `StructurePoolElement.place(...)` (§3.5 / §4.5), returning whether anything was placed.
pub fn place_pool_element(
    call: &mut PieceCall<'_>,
    element: &StructurePoolElement,
    position: BlockPos,
    rotation: Rotation,
    chunk_bb: BoundingBox,
    liquid: LiquidSettings,
    keep_jigsaws: bool,
) -> bool {
    match element {
        StructurePoolElement::Empty => true,
        StructurePoolElement::Feature { feature, .. } => {
            call.world
                .place_feature(feature, position, &mut *call.random)
        }
        StructurePoolElement::List { elements, .. } => {
            for e in elements {
                if !place_pool_element(call, e, position, rotation, chunk_bb, liquid, keep_jigsaws)
                {
                    return false;
                }
            }
            true
        }
        StructurePoolElement::Single {
            location,
            processors,
            projection,
            override_liquid_settings,
        }
        | StructurePoolElement::LegacySingle {
            location,
            processors,
            projection,
            override_liquid_settings,
        } => {
            let legacy = matches!(element, StructurePoolElement::LegacySingle { .. });
            let Ok(template) = call.env.templates.get(location) else {
                return false;
            };
            let mut procs: Vec<StructureProcessor> = Vec::new();
            if !legacy {
                procs.push(structure_block_ignore());
            }
            if !keep_jigsaws {
                procs.push(StructureProcessor::JigsawReplacement);
            }
            match processors {
                ProcessorListRef::Reference(id) => {
                    if let Some(l) = call.env.data.processor_list(id) {
                        procs.extend(l.iter().cloned());
                    }
                }
                ProcessorListRef::Inline(l) => procs.extend(l.iter().cloned()),
            }
            if *projection == Projection::TerrainMatching {
                procs.push(StructureProcessor::Gravity {
                    heightmap: HeightmapKind::WorldSurfaceWg,
                    offset: -1,
                });
            }
            if legacy {
                procs.push(structure_and_air_ignore());
            }
            let mut settings = StructurePlaceSettings {
                rotation,
                bounding_box: Some(chunk_bb),
                known_shape: true,
                ignore_entities: false,
                finalize_entities: true,
                liquid_settings: override_liquid_settings.unwrap_or(liquid),
                processors: procs,
                ..StructurePlaceSettings::default()
            };
            place_template(
                call.world,
                &template,
                position,
                call.reference_pos,
                &mut settings,
                &mut *call.random,
                18,
            )
        }
    }
}
