//! Top-level structure generator (`StructureGenerator`), `structure_starts`,
//! `structure_references`, and `Beardifier` handoff
//! (`docs/specs/worldgen/structures-placement-and-jigsaw.md` §2 & §5).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use zenith_biome::{BiomeSource, SearchState};
use zenith_blocks::shape::{Direction as BlockDirection, SupportType};
use zenith_blocks::{BlockFacts, flag};
use zenith_math::helpers::packing::chunk_pack;
use zenith_math::{LegacyRandom, RandomSource};
use zenith_registry::{BlockStateTable, Identifier};
use zenith_worldgen::{
    Beardifier, BeardifierRigid, HeightmapKind, JigsawJunction, NoiseGenerator, SamplerContext,
    TerrainAdjustment, WorldGenerationContext, WorldgenData,
};

use crate::data::{
    Projection, StructureData, StructureDef, StructureKind, StructureRef, StructureSet,
};
use crate::geom::{BlockPos, BoundingBox, BoundingBoxExt, Direction, set_large_feature_seed};
use crate::jigsaw::{JigsawGenContext, PoolElementPiece, generate_jigsaw_pieces};
use crate::legacy::{LegacyGenContext, generate_legacy_pieces};
use crate::piece_data::PieceData;
use crate::placement::{
    StructurePlacementState, compute_dimension_origin, could_structure_exist_in_column,
    sample_noise_biome_point,
};
use crate::template::TemplateManager;

/// One generated structure piece in a [`StructureStart`].
#[derive(Debug, Clone)]
pub struct StructurePiece {
    /// Registered `minecraft:worldgen/structure_piece` identifier (e.g. `"minecraft:jigsaw"`,
    /// `"minecraft:msroom"`).
    pub kind: &'static str,
    /// Piece inclusive bounding box.
    pub bounding_box: BoundingBox,
    /// Horizontal orientation (`None` for jigsaw, template, and world-space pieces).
    pub orientation: Option<Direction>,
    /// Generation depth (`"GD"`).
    pub gen_depth: i32,
    /// Full jigsaw piece data when `kind == "minecraft:jigsaw"` (used by [`Beardifier`] and
    /// downstream block placement).
    pub jigsaw: Option<PoolElementPiece>,
    /// Kind-specific placement data of legacy (non-jigsaw) pieces.
    pub data: PieceData,
}

/// One valid generated structure start (`StructureStart`, §2.1 & §2.3).
#[derive(Debug, Clone)]
pub struct StructureStart {
    /// Registered `minecraft:worldgen/structure` identifier.
    pub structure_id: Identifier,
    /// Start chunk X.
    pub chunk_x: i32,
    /// Start chunk Z.
    pub chunk_z: i32,
    /// Reference counter (`references`, 0 at `structure_starts` creation).
    pub references: i32,
    /// Structure's terrain adaptation mode (`terrain_adaptation`).
    pub terrain_adaptation: TerrainAdjustment,
    /// Adjusted bounding box (`StructureStart.getBoundingBox()`): piece union inflated by `±12`
    /// on all six faces when `terrain_adaptation != TerrainAdjustment::None`.
    pub bounding_box: BoundingBox,
    /// Ordered list of pieces belonging to this start.
    pub pieces: Vec<StructurePiece>,
}

impl StructureStart {
    /// Returns `true` iff this start has at least one piece (`StructureStart.isValid()`).
    #[inline]
    pub fn is_valid(&self) -> bool {
        !self.pieces.is_empty()
    }
}

/// Applies `Structure.adjustBoundingBox` (§2.2 & §5.1): inflates by `±12` when
/// `terrain_adaptation != TerrainAdjustment::None`.
#[inline]
pub const fn adjust_bounding_box(
    raw_union: BoundingBox,
    terrain_adaptation: TerrainAdjustment,
) -> BoundingBox {
    match terrain_adaptation {
        TerrainAdjustment::None => raw_union,
        TerrainAdjustment::Bury
        | TerrainAdjustment::BeardThin
        | TerrainAdjustment::BeardBox
        | TerrainAdjustment::Encapsulate => raw_union.inflated_by(12),
    }
}

/// Dimension-bound structure generator implementing `structure_starts`, `structure_references`,
/// and `Beardifier` handoff (§1–§5 + all 15 legacy structure types).
#[derive(Debug)]
pub struct StructureGenerator {
    seed: i64,
    structure_data: Arc<StructureData>,
    template_manager: Arc<TemplateManager>,
    noise_generator: Arc<NoiseGenerator>,
    biome_source: BiomeSource,
    block_facts: Arc<BlockFacts>,
    placement_state: StructurePlacementState,
    chunk_min_y: i32,
    chunk_height: i32,
    soul_sand_block_id: Option<u32>,
    biome_data: HashMap<Identifier, zenith_worldgen::BiomeData>,
}

impl StructureGenerator {
    /// Constructs a [`StructureGenerator`] for one dimension and world seed.
    ///
    /// - `chunk_min_y` / `chunk_height`: dimension height accessor bounds (`DimensionType`, e.g.
    ///   `(-64, 384)` for Overworld, `(0, 256)` for Nether/End).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        seed: i64,
        worldgen_data: &WorldgenData,
        structure_data: Arc<StructureData>,
        template_manager: Arc<TemplateManager>,
        noise_generator: Arc<NoiseGenerator>,
        biome_source: BiomeSource,
        block_facts: Arc<BlockFacts>,
        blocks: &BlockStateTable,
        chunk_min_y: i32,
        chunk_height: i32,
    ) -> Self {
        let settings = noise_generator.settings();
        let origin = compute_dimension_origin(
            settings.spawn_target(),
            worldgen_data,
            seed,
            settings.legacy_random_source(),
        );
        let placement_state = StructurePlacementState::for_normal(
            seed,
            origin,
            &biome_source,
            Arc::clone(&structure_data),
        );
        let soul_sand_id = Identifier::parse("minecraft:soul_sand").expect("valid");
        let soul_sand_block_id = block_facts
            .blocks()
            .iter()
            .enumerate()
            .find(|(_, b)| b.id == soul_sand_id)
            .map(|(i, _)| i as u32);
        let _ = blocks;

        Self {
            seed,
            structure_data,
            template_manager,
            noise_generator,
            biome_source,
            block_facts,
            placement_state,
            chunk_min_y,
            chunk_height,
            soul_sand_block_id,
            biome_data: worldgen_data
                .biomes()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        }
    }

    /// World seed.
    #[inline]
    pub const fn seed(&self) -> i64 {
        self.seed
    }

    /// Loaded structure registries.
    #[inline]
    pub fn structure_data(&self) -> &StructureData {
        &self.structure_data
    }

    /// Structure template manager.
    #[inline]
    pub fn template_manager(&self) -> &TemplateManager {
        &self.template_manager
    }

    /// Dimension noise generator.
    #[inline]
    pub fn noise_generator(&self) -> &NoiseGenerator {
        &self.noise_generator
    }

    /// Dimension biome source.
    #[inline]
    pub fn biome_source(&self) -> &BiomeSource {
        &self.biome_source
    }

    /// Placement state (filtered structure sets, concentric rings, dimension origin).
    #[inline]
    pub fn placement_state(&self) -> &StructurePlacementState {
        &self.placement_state
    }

    /// Dimension chunk height accessor minimum Y.
    #[inline]
    pub const fn chunk_min_y(&self) -> i32 {
        self.chunk_min_y
    }

    /// Dimension chunk height accessor inclusive maximum Y.
    #[inline]
    pub const fn chunk_max_y(&self) -> i32 {
        self.chunk_min_y + self.chunk_height - 1
    }

    /// `WorldGenerationContext` for height providers in this dimension.
    #[inline]
    pub fn world_gen_context(&self) -> WorldGenerationContext {
        let noise = self.noise_generator.settings();
        let min_gen_y = self.chunk_min_y.max(noise.min_y());
        let gen_depth = self.chunk_height.min(noise.height());
        WorldGenerationContext::new(min_gen_y, gen_depth, noise.sea_level())
    }

    /// Returns whether `set` passes `is_structure_chunk` at `(chunk_x, chunk_z)`.
    pub fn is_structure_chunk(&self, set: &StructureSet, chunk_x: i32, chunk_z: i32) -> bool {
        self.placement_state.is_structure_chunk(
            set,
            chunk_x,
            chunk_z,
            &self.biome_source,
            self.noise_generator.random_state(),
        )
    }

    /// Precomputes and returns concentric ring chunk positions for a concentric-ring structure set.
    pub fn ring_positions_for(&self, set: &StructureSet) -> Vec<(i32, i32)> {
        self.placement_state.ring_positions_for(
            set,
            &self.biome_source,
            self.noise_generator.random_state(),
        )
    }

    /// Executes `structure_starts` (`ChunkGenerator.createStructures`, §2.1) for `(chunk_x, chunk_z)`.
    ///
    /// Returns all valid [`StructureStart`]s on this chunk, sorted by canonical structure ID.
    pub fn structure_starts(&self, chunk_x: i32, chunk_z: i32) -> Vec<StructureStart> {
        let rs = self.noise_generator.random_state();
        let mut starts_by_id: BTreeMap<Identifier, StructureStart> = BTreeMap::new();
        let mut sampler_ctx = rs.new_sampler_context(true);
        let mut search_state = SearchState::new();

        for set in self.placement_state.possible_structure_sets() {
            // Skip if any structure in this set already has a valid start on this chunk (§2.1 step 1).
            let already_present = set.structures.iter().any(|entry| {
                let Some(id) = structure_ref_id(&entry.structure) else {
                    return false;
                };
                starts_by_id.get(id).is_some_and(StructureStart::is_valid)
            });
            if already_present {
                continue;
            }

            // Placement test (§2.1 step 2).
            if !self.placement_state.is_structure_chunk(
                set,
                chunk_x,
                chunk_z,
                &self.biome_source,
                rs,
            ) {
                continue;
            }

            // Structure selection within the set (§2.1 step 3).
            if set.structures.len() == 1 {
                let entry = &set.structures[0];
                let _ = self.try_generate_structure(
                    &entry.structure,
                    chunk_x,
                    chunk_z,
                    &mut starts_by_id,
                    &mut sampler_ctx,
                    &mut search_state,
                );
            } else if !set.structures.is_empty() {
                let mut candidates: Vec<_> = set.structures.iter().collect();
                let mut total_weight: i32 = candidates.iter().map(|e| e.weight).sum();
                let mut set_rng = LegacyRandom::new(0);
                set_large_feature_seed(&mut set_rng, self.seed, chunk_x, chunk_z);

                while !candidates.is_empty() && total_weight > 0 {
                    let mut roll = set_rng.next_int_bounded(total_weight);
                    let mut chosen_idx = 0;
                    for (idx, entry) in candidates.iter().enumerate() {
                        roll -= entry.weight;
                        if roll < 0 {
                            chosen_idx = idx;
                            break;
                        }
                    }
                    let chosen = candidates.remove(chosen_idx);
                    total_weight -= chosen.weight;
                    if self.try_generate_structure(
                        &chosen.structure,
                        chunk_x,
                        chunk_z,
                        &mut starts_by_id,
                        &mut sampler_ctx,
                        &mut search_state,
                    ) {
                        break;
                    }
                }
            }
        }

        starts_by_id.into_values().collect()
    }

    fn try_generate_structure(
        &self,
        structure_ref: &StructureRef,
        chunk_x: i32,
        chunk_z: i32,
        starts_by_id: &mut BTreeMap<Identifier, StructureStart>,
        sampler_ctx: &mut SamplerContext,
        search_state: &mut SearchState,
    ) -> bool {
        let Some(def) = self.structure_data.resolve_structure(structure_ref) else {
            return false;
        };
        let structure_id = match structure_ref {
            StructureRef::Reference(id) => id.clone(),
            StructureRef::Inline(_) => def.id.clone(),
        };

        let existing_refs = if let Some(existing) = starts_by_id.get(&structure_id) {
            if existing.is_valid() {
                return false;
            }
            existing.references
        } else {
            0
        };

        let mut rng = LegacyRandom::new(0);
        set_large_feature_seed(&mut rng, self.seed, chunk_x, chunk_z);
        let pieces = self.generate_pieces_for_structure(
            def,
            chunk_x,
            chunk_z,
            &mut rng,
            sampler_ctx,
            search_state,
        );
        let Some(pieces) = pieces else {
            return false;
        };
        if pieces.is_empty() {
            return false;
        }

        let mut raw_bb = pieces[0].bounding_box;
        for p in &pieces[1..] {
            raw_bb.encapsulate(p.bounding_box);
        }
        let bounding_box = adjust_bounding_box(raw_bb, def.settings.terrain_adaptation);

        starts_by_id.insert(
            structure_id.clone(),
            StructureStart {
                structure_id,
                chunk_x,
                chunk_z,
                references: existing_refs,
                terrain_adaptation: def.settings.terrain_adaptation,
                bounding_box,
                pieces,
            },
        );
        true
    }

    fn generate_pieces_for_structure(
        &self,
        def: &StructureDef,
        chunk_x: i32,
        chunk_z: i32,
        rng: &mut zenith_math::LegacyRandom,
        sampler_ctx: &mut SamplerContext,
        search_state: &mut SearchState,
    ) -> Option<Vec<StructurePiece>> {
        let mut ctx = GenContextImpl {
            generator: self,
            allowed_biomes: &def.settings.biomes,
            sampler_ctx,
            search_state,
        };
        match &def.kind {
            StructureKind::Jigsaw(cfg) => {
                let pool_pieces = generate_jigsaw_pieces(cfg, chunk_x, chunk_z, rng, &mut ctx)?;
                if pool_pieces.is_empty() {
                    return None;
                }
                Some(
                    pool_pieces
                        .into_iter()
                        .map(|p| StructurePiece {
                            kind: "minecraft:jigsaw",
                            bounding_box: p.bounding_box,
                            orientation: None,
                            gen_depth: 0,
                            jigsaw: Some(p),
                            data: PieceData::None,
                        })
                        .collect(),
                )
            }
            other => {
                let legacy = generate_legacy_pieces(other, chunk_x, chunk_z, rng, &mut ctx)?;
                if legacy.is_empty() {
                    return None;
                }
                Some(
                    legacy
                        .into_iter()
                        .map(|p| StructurePiece {
                            kind: p.kind,
                            bounding_box: p.bounding_box,
                            orientation: p.orientation,
                            gen_depth: p.gen_depth,
                            jigsaw: None,
                            data: p.data,
                        })
                        .collect(),
                )
            }
        }
    }

    /// Executes `structure_references` (`ChunkGenerator.createReferences`, §2.2) for
    /// `(target_x, target_z)`, returning `(structure_id, sorted_start_chunks)`.
    pub fn structure_references(
        &self,
        target_x: i32,
        target_z: i32,
    ) -> Vec<(Identifier, Vec<(i32, i32)>)> {
        let mut cache = HashMap::new();
        self.structure_references_packed_with_cache(target_x, target_z, &mut cache)
            .into_iter()
            .map(|(id, packed)| {
                let coords = packed
                    .into_iter()
                    .map(|p| (p as i32, (p >> 32) as i32))
                    .collect();
                (id, coords)
            })
            .collect()
    }

    /// Executes `structure_references` (§2.2), returning packed `ChunkPos` longs (`x` in low 32
    /// bits, `z` in high 32 bits), sorted ascending per structure ID (matching ZTC1).
    pub fn structure_references_packed(
        &self,
        target_x: i32,
        target_z: i32,
    ) -> Vec<(Identifier, Vec<i64>)> {
        let mut cache = HashMap::new();
        self.structure_references_packed_with_cache(target_x, target_z, &mut cache)
    }

    /// Cached version of [`Self::structure_references_packed`] sharing `structure_starts` results
    /// across adjacent chunk queries.
    pub fn structure_references_packed_with_cache(
        &self,
        target_x: i32,
        target_z: i32,
        starts_cache: &mut HashMap<(i32, i32), Vec<StructureStart>>,
    ) -> Vec<(Identifier, Vec<i64>)> {
        let min_bx = target_x << 4;
        let min_bz = target_z << 4;
        let max_bx = min_bx + 15;
        let max_bz = min_bz + 15;

        let mut refs: BTreeMap<Identifier, Vec<i64>> = BTreeMap::new();

        for source_x in (target_x - 8)..=(target_x + 8) {
            for source_z in (target_z - 8)..=(target_z + 8) {
                let starts = starts_cache
                    .entry((source_x, source_z))
                    .or_insert_with(|| self.structure_starts(source_x, source_z));
                let packed = chunk_pack(source_x, source_z);
                for start in starts.iter() {
                    if start.is_valid()
                        && start
                            .bounding_box
                            .intersects_xz(min_bx, min_bz, max_bx, max_bz)
                    {
                        refs.entry(start.structure_id.clone())
                            .or_default()
                            .push(packed);
                    }
                }
            }
        }

        for list in refs.values_mut() {
            list.sort_unstable();
            list.dedup();
        }

        refs.into_iter().collect()
    }

    /// Builds the [`Beardifier`] for chunk `(chunk_x, chunk_z)` per §5.2.
    pub fn beardifier_for_chunk(&self, chunk_x: i32, chunk_z: i32) -> Beardifier {
        let mut cache = HashMap::new();
        self.beardifier_for_chunk_with_cache(chunk_x, chunk_z, &mut cache)
    }

    /// Cached version of [`Self::beardifier_for_chunk`].
    pub fn beardifier_for_chunk_with_cache(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        starts_cache: &mut HashMap<(i32, i32), Vec<StructureStart>>,
    ) -> Beardifier {
        let min_bx = chunk_x << 4;
        let min_bz = chunk_z << 4;
        let max_bx = min_bx + 15;
        let max_bz = min_bz + 15;

        let mut relevant: Vec<StructureStart> = Vec::new();
        for source_x in (chunk_x - 8)..=(chunk_x + 8) {
            for source_z in (chunk_z - 8)..=(chunk_z + 8) {
                let starts = starts_cache
                    .entry((source_x, source_z))
                    .or_insert_with(|| self.structure_starts(source_x, source_z));
                for start in starts.iter() {
                    if start.is_valid()
                        && start.terrain_adaptation != TerrainAdjustment::None
                        && start
                            .bounding_box
                            .intersects_xz(min_bx, min_bz, max_bx, max_bz)
                    {
                        relevant.push(start.clone());
                    }
                }
            }
        }

        let refs: Vec<&StructureStart> = relevant.iter().collect();
        beardifier_from_starts(chunk_x, chunk_z, &refs)
    }
}

/// Builds a [`Beardifier`] from a set of referenced [`StructureStart`]s for chunk
/// `(chunk_x, chunk_z)` using the exact §5.2 contract:
/// - Skip starts with `terrain_adaptation == TerrainAdjustment::None`.
/// - Filter pieces by inclusive `piece.bounding_box.intersects_xz(cx - 12, cz - 12, cx + 27, cz + 27)`.
/// - For jigsaw pieces:
///   - If `element.projection() == Projection::Rigid`: push `BeardifierRigid` with `piece.ground_level_delta`.
///   - For ALL passing jigsaw pieces (both `Rigid` and `TerrainMatching`): push every junction
///     whose `(source_x, source_z)` satisfies the **strict** inequality
///     `source_x > cx - 12 && source_z > cz - 12 && source_x < cx + 27 && source_z < cz + 27`.
/// - For non-jigsaw pieces: push `BeardifierRigid` with `ground_level_delta = 0`.
pub fn beardifier_from_starts(
    chunk_x: i32,
    chunk_z: i32,
    starts: &[&StructureStart],
) -> Beardifier {
    let cx = chunk_x << 4;
    let cz = chunk_z << 4;
    let pad_min_x = cx - 12;
    let pad_min_z = cz - 12;
    let pad_max_x = cx + 27;
    let pad_max_z = cz + 27;

    let mut rigids: Vec<BeardifierRigid> = Vec::new();
    let mut junctions: Vec<JigsawJunction> = Vec::new();

    for &start in starts {
        if start.terrain_adaptation == TerrainAdjustment::None || !start.is_valid() {
            continue;
        }
        let adjustment = start.terrain_adaptation;
        for piece in &start.pieces {
            if !piece
                .bounding_box
                .intersects_xz(pad_min_x, pad_min_z, pad_max_x, pad_max_z)
            {
                continue;
            }
            if let Some(jigsaw) = &piece.jigsaw {
                if jigsaw.element.projection() == Projection::Rigid {
                    rigids.push(BeardifierRigid {
                        bounding_box: piece.bounding_box,
                        adjustment,
                        ground_level_delta: jigsaw.ground_level_delta,
                    });
                }
                for j in &jigsaw.junctions {
                    if j.source_x > pad_min_x
                        && j.source_z > pad_min_z
                        && j.source_x < pad_max_x
                        && j.source_z < pad_max_z
                    {
                        junctions.push(JigsawJunction {
                            source_x: j.source_x,
                            source_ground_y: j.source_ground_y,
                            source_z: j.source_z,
                        });
                    }
                }
            } else {
                rigids.push(BeardifierRigid {
                    bounding_box: piece.bounding_box,
                    adjustment,
                    ground_level_delta: 0,
                });
            }
        }
    }

    if rigids.is_empty() && junctions.is_empty() {
        Beardifier::empty()
    } else {
        Beardifier::new(rigids, junctions)
    }
}

fn structure_ref_id(r: &StructureRef) -> Option<&Identifier> {
    match r {
        StructureRef::Reference(id) => Some(id),
        StructureRef::Inline(def) => Some(&def.id),
    }
}

struct GenContextImpl<'a> {
    generator: &'a StructureGenerator,
    allowed_biomes: &'a HashSet<Identifier>,
    sampler_ctx: &'a mut SamplerContext,
    search_state: &'a mut SearchState,
}

impl JigsawGenContext for GenContextImpl<'_> {
    fn world_seed(&self) -> i64 {
        self.generator.seed
    }

    fn min_y(&self) -> i32 {
        self.generator.chunk_min_y
    }

    fn max_y(&self) -> i32 {
        self.generator.chunk_max_y()
    }

    fn world_gen_context(&self) -> WorldGenerationContext {
        self.generator.world_gen_context()
    }

    fn structure_data(&self) -> &StructureData {
        &self.generator.structure_data
    }

    fn template_manager(&self) -> &TemplateManager {
        &self.generator.template_manager
    }

    fn could_structure_exist_in_column(
        &self,
        block_x: i32,
        block_z: i32,
        min_block_y: i32,
        max_block_y: i32,
    ) -> bool {
        could_structure_exist_in_column(
            &self.generator.biome_source,
            self.generator.noise_generator.random_state(),
            block_x,
            block_z,
            min_block_y,
            max_block_y,
            self.allowed_biomes,
        )
    }

    fn is_valid_biome(&mut self, pos: BlockPos) -> bool {
        let biome = sample_noise_biome_point(
            &self.generator.biome_source,
            self.generator.noise_generator.random_state(),
            self.sampler_ctx,
            self.search_state,
            pos.x >> 2,
            pos.y >> 2,
            pos.z >> 2,
        );
        self.allowed_biomes.contains(&biome)
    }

    fn first_free_height(&self, x: i32, z: i32, kind: HeightmapKind) -> i32 {
        self.generator.noise_generator.base_height(x, z, kind)
    }
}

impl LegacyGenContext for GenContextImpl<'_> {
    fn world_seed(&self) -> i64 {
        self.generator.seed
    }

    fn min_y(&self) -> i32 {
        self.generator.chunk_min_y
    }

    fn max_y(&self) -> i32 {
        self.generator.chunk_max_y()
    }

    fn noise_min_y(&self) -> i32 {
        self.generator.noise_generator.settings().min_y()
    }

    fn sea_level(&self) -> i32 {
        self.generator.noise_generator.settings().sea_level()
    }

    fn world_gen_context(&self) -> WorldGenerationContext {
        self.generator.world_gen_context()
    }

    fn structure_data(&self) -> &StructureData {
        &self.generator.structure_data
    }

    fn template_manager(&self) -> &TemplateManager {
        &self.generator.template_manager
    }

    fn could_valid_biome_exist_in_terrain_column(&self, bx: i32, bz: i32) -> bool {
        could_structure_exist_in_column(
            &self.generator.biome_source,
            self.generator.noise_generator.random_state(),
            bx,
            bz,
            self.generator.chunk_min_y,
            self.generator.chunk_max_y(),
            self.allowed_biomes,
        )
    }

    fn cold_enough_to_snow(&mut self, pos: BlockPos) -> bool {
        let biome = sample_noise_biome_point(
            &self.generator.biome_source,
            self.generator.noise_generator.random_state(),
            self.sampler_ctx,
            self.search_state,
            pos.x >> 2,
            pos.y >> 2,
            pos.z >> 2,
        );
        let sea = self.generator.noise_generator.settings().sea_level();
        self.generator
            .biome_data
            .get(&biome)
            .is_some_and(|d| d.cold_enough_to_snow(pos.x, pos.y, pos.z, sea))
    }

    fn is_valid_biome(&mut self, pos: BlockPos) -> bool {
        let biome = sample_noise_biome_point(
            &self.generator.biome_source,
            self.generator.noise_generator.random_state(),
            self.sampler_ctx,
            self.search_state,
            pos.x >> 2,
            pos.y >> 2,
            pos.z >> 2,
        );
        self.allowed_biomes.contains(&biome)
    }

    fn biomes_within(&mut self, bx: i32, by: i32, bz: i32, radius: i32) -> HashSet<Identifier> {
        let min_qx = (bx - radius) >> 2;
        let min_qy = (by - radius) >> 2;
        let min_qz = (bz - radius) >> 2;
        let max_qx = (bx + radius) >> 2;
        let max_qy = (by + radius) >> 2;
        let max_qz = (bz + radius) >> 2;
        let rs = self.generator.noise_generator.random_state();
        let mut out = HashSet::new();
        for qy in min_qy..=max_qy {
            for qz in min_qz..=max_qz {
                for qx in min_qx..=max_qx {
                    let b = sample_noise_biome_point(
                        &self.generator.biome_source,
                        rs,
                        self.sampler_ctx,
                        self.search_state,
                        qx,
                        qy,
                        qz,
                    );
                    out.insert(b);
                }
            }
        }
        out
    }

    fn base_height(&self, x: i32, z: i32, kind: HeightmapKind) -> i32 {
        self.generator.noise_generator.base_height(x, z, kind)
    }

    fn base_column(&self, x: i32, z: i32) -> Vec<u32> {
        self.generator.noise_generator.base_column(x, z)
    }

    fn is_air(&self, state_id: u32) -> bool {
        self.generator
            .block_facts
            .states()
            .get(state_id as usize)
            .is_some_and(|s| s.has(flag::IS_AIR))
    }

    fn is_soul_sand(&self, state_id: u32) -> bool {
        let Some(soul_sand) = self.generator.soul_sand_block_id else {
            return false;
        };
        self.generator
            .block_facts
            .states()
            .get(state_id as usize)
            .is_some_and(|s| s.block == soul_sand)
    }

    fn is_face_sturdy_up(&self, state_id: u32) -> bool {
        self.generator
            .block_facts
            .states()
            .get(state_id as usize)
            .is_some_and(|s| s.is_face_sturdy(BlockDirection::Up, SupportType::Full))
    }

    fn is_heightmap_opaque(&self, kind: HeightmapKind, state_id: u32) -> bool {
        self.generator
            .noise_generator
            .predicates()
            .is_opaque(kind, state_id)
    }
}
