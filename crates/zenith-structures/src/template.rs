//! `.nbt` structure template loader, caching [`TemplateManager`], transforms, jigsaw extraction,
//! and the structure processor pipeline (`docs/specs/worldgen/structures-placement-and-jigsaw.md` §4).

use std::collections::{BTreeMap, HashMap};
use std::io::Cursor;
use std::sync::{Arc, RwLock};

use thiserror::Error;
use zenith_blocks::{BlockFacts, flag};
use zenith_codec::format::PackType;
use zenith_codec::json::Json;
use zenith_codec::resources::PackResources;
use zenith_math::helpers::interp::{clamped_lerp_f32, inverse_lerp_f32};
use zenith_math::random::position_hash;
use zenith_math::{LegacyRandom, PositionalRandomFactory, RandomSource};
use zenith_nbt::{Accounter, Compound, Tag, read_gzip_named};
use zenith_registry::{BlockStateTable, Identifier};
use zenith_worldgen::{BlockStateSpec, BlockTags, HeightmapKind, resolve_block_state};

use crate::data::{
    Axis, LiquidSettings, PosRuleTest, RawBlockHolderSet, RuleBlockEntityModifier, RuleTest,
    StructureProcessor,
};
use crate::geom::{
    BlockPos, BoundingBox, BoundingBoxExt, FrontAndTop, Mirror, Rotation, is_horizontal,
    random_horizontal_direction, shuffle_slice, transform_pos,
};

/// Error loading a `.nbt` structure template.
#[derive(Debug, Error)]
pub enum TemplateError {
    /// Template file was not found in the pack.
    #[error("missing structure template '{id}'")]
    Missing {
        /// Template identifier.
        id: Identifier,
    },
    /// Gzip or NBT decoding failed.
    #[error("failed to decode structure template '{id}': {source}")]
    Nbt {
        /// Template identifier.
        id: Identifier,
        /// Underlying NBT error.
        #[source]
        source: zenith_nbt::Error,
    },
}

/// Jigsaw joint type (`JigsawBlockEntity.JointType`, §3.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JointType {
    /// `rollable`: top orientation ignored during attachment.
    Rollable,
    /// `aligned`: top orientations must match during attachment.
    Aligned,
}

/// Extracted jigsaw block metadata from a template (already transformed when returned by `get_jigsaws`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JigsawBlockInfo {
    /// Block position (relative or world-space).
    pub pos: BlockPos,
    /// Transformed orientation (`FrontAndTop`).
    pub orientation: FrontAndTop,
    /// Jigsaw `name` (`None` only for synthetic `FeaturePoolElement` jigsaw, which matches any target).
    pub name: Option<Identifier>,
    /// Target jigsaw name to connect to.
    pub target: Identifier,
    /// Target template pool ID.
    pub pool: Identifier,
    /// Joint type (`Rollable` or `Aligned`).
    pub joint: JointType,
    /// Priority for BFS expansion ordering (higher expands first).
    pub placement_priority: i32,
    /// Priority for jigsaw selection order within a piece (higher tried first).
    pub selection_priority: i32,
    /// Final block state string after replacement.
    pub final_state: String,
}

impl JigsawBlockInfo {
    /// `JigsawBlock.canAttach(source, target)` (§3.6).
    #[inline]
    pub fn can_attach(&self, target: &Self) -> bool {
        let src_front = self.orientation.front();
        let dst_front = target.orientation.front();
        if src_front != dst_front.opposite() {
            return false;
        }
        if self.joint != JointType::Rollable && self.orientation.top() != target.orientation.top() {
            return false;
        }
        match &target.name {
            None => true,
            Some(name) => self.target == *name,
        }
    }
}

/// One block entry inside a template palette (`StructureTemplate.StructureBlockInfo`).
#[derive(Debug, Clone, PartialEq)]
pub struct StructureBlockInfo {
    /// Block position (template-local in template storage, world-space during processing).
    pub pos: BlockPos,
    /// Global block-state ID.
    pub state_id: u32,
    /// Optional block-entity NBT.
    pub nbt: Option<Compound>,
}

/// One entity entry inside a template (`StructureTemplate.StructureEntityInfo`).
#[derive(Debug, Clone, PartialEq)]
pub struct StructureEntityInfo {
    /// Continuous entity position relative to template origin.
    pub pos: [f64; 3],
    /// Block position relative to template origin.
    pub block_pos: BlockPos,
    /// Entity NBT compound.
    pub nbt: Compound,
}

/// One block palette inside a [`StructureTemplate`].
#[derive(Debug, Clone)]
pub struct TemplatePalette {
    /// Sorted blocks in exact vanilla order: `fullBlockList + otherBlocksList + blockEntitiesList`,
    /// each stably sorted by `(y, x, z)` ascending (§4.1).
    pub blocks: Vec<StructureBlockInfo>,
    /// Extracted jigsaw blocks in `blockEntitiesList` (Y, X, Z) order.
    pub jigsaws: Vec<JigsawBlockInfo>,
}

/// Placement settings (`StructurePlaceSettings`, §4.2–§4.5).
#[derive(Debug, Clone)]
pub struct StructurePlaceSettings {
    /// Mirror applied before rotation.
    pub mirror: Mirror,
    /// Horizontal rotation around `rotation_pivot`.
    pub rotation: Rotation,
    /// Rotation pivot in template coordinates.
    pub rotation_pivot: BlockPos,
    /// Optional chunk clipping bounding box.
    pub bounding_box: Option<BoundingBox>,
    /// Whether to skip shape updates (`knownShape`).
    pub known_shape: bool,
    /// Whether to skip placing entities.
    pub ignore_entities: bool,
    /// Whether to call `finalizeSpawn` on spawned mobs.
    pub finalize_entities: bool,
    /// Waterlogging policy.
    pub liquid_settings: LiquidSettings,
    /// Ordered processor list.
    pub processors: Vec<StructureProcessor>,
    /// Optional explicit RNG seed state (when `None`, derived per position via `position_hash`).
    pub random: Option<LegacyRandom>,
}

impl Default for StructurePlaceSettings {
    fn default() -> Self {
        Self {
            mirror: Mirror::None,
            rotation: Rotation::None,
            rotation_pivot: BlockPos::ZERO,
            bounding_box: None,
            known_shape: false,
            ignore_entities: false,
            finalize_entities: false,
            liquid_settings: LiquidSettings::ApplyWaterlogging,
            processors: Vec::new(),
            random: None,
        }
    }
}

impl StructurePlaceSettings {
    /// `settings.getRandom(pos)` (§4.2).
    pub fn get_random(&mut self, pos: Option<BlockPos>) -> LegacyRandom {
        if let Some(ref rng) = self.random {
            return rng.clone();
        }
        match pos {
            Some(p) => LegacyRandom::new(position_hash(p.x, p.y, p.z)),
            None => LegacyRandom::new(0),
        }
    }

    /// Selects a palette from `palettes` (`settings.getRandomPalette`, §4.2).
    pub fn select_palette_index(&mut self, palette_count: usize, pos: Option<BlockPos>) -> usize {
        if palette_count == 0 {
            return 0;
        }
        if let Some(ref mut rng) = self.random {
            return rng.next_int_bounded(palette_count as i32) as usize;
        }
        let mut rng = match pos {
            Some(p) => LegacyRandom::new(position_hash(p.x, p.y, p.z)),
            None => LegacyRandom::new(0),
        };
        rng.next_int_bounded(palette_count as i32) as usize
    }
}

/// Decoded `.nbt` structure template (`StructureTemplate`, §4.1).
#[derive(Debug, Clone)]
pub struct StructureTemplate {
    /// `[sizeX, sizeY, sizeZ]`.
    pub size: BlockPos,
    /// One or more block palettes.
    pub palettes: Vec<TemplatePalette>,
    /// Template entities.
    pub entities: Vec<StructureEntityInfo>,
}

impl StructureTemplate {
    /// Decodes a gzipped named-root `.nbt` template (§4.1).
    pub fn from_gzip_nbt(
        id: &Identifier,
        bytes: &[u8],
        blocks: &BlockStateTable,
        facts: Option<&BlockFacts>,
    ) -> Result<Self, TemplateError> {
        let (_name, root_tag) =
            read_gzip_named(&mut Cursor::new(bytes), &mut Accounter::unlimited()).map_err(|e| {
                TemplateError::Nbt {
                    id: id.clone(),
                    source: e,
                }
            })?;
        let Some(root) = root_tag.as_compound() else {
            return Ok(Self {
                size: BlockPos::ZERO,
                palettes: Vec::new(),
                entities: Vec::new(),
            });
        };
        Ok(Self::from_compound(root, blocks, facts))
    }

    /// Decodes a template from its parsed root NBT [`Compound`] (§4.1).
    pub fn from_compound(
        root: &Compound,
        blocks: &BlockStateTable,
        facts: Option<&BlockFacts>,
    ) -> Self {
        let air_id = blocks.air_default_state().map_or(0, |s| s.global_id);
        let jigsaw_block_id = Identifier::with_default_namespace("jigsaw").expect("valid");
        let empty_id = Identifier::with_default_namespace("empty").expect("valid");

        // Size: List<Int>[3]
        let size = match root.get("size").and_then(Tag::as_list) {
            Some(list) => {
                let get_i =
                    |idx: usize| -> i32 { list.get(idx).and_then(Tag::as_int).unwrap_or(0) };
                BlockPos::new(get_i(0), get_i(1), get_i(2))
            }
            None => BlockPos::ZERO,
        };

        // Palettes: `palettes` overrides `palette` if present.
        let raw_palettes: Vec<Vec<u32>> =
            if let Some(palettes_list) = root.get("palettes").and_then(Tag::as_list) {
                palettes_list
                    .iter()
                    .filter_map(Tag::as_list)
                    .map(|p| decode_nbt_palette(p, blocks, air_id))
                    .collect()
            } else if let Some(palette_list) = root.get("palette").and_then(Tag::as_list) {
                vec![decode_nbt_palette(palette_list, blocks, air_id)]
            } else {
                Vec::new()
            };

        let raw_blocks = root.get("blocks").and_then(Tag::as_list);
        let mut palettes = Vec::with_capacity(raw_palettes.len());

        for pal in &raw_palettes {
            let mut full_blocks: Vec<StructureBlockInfo> = Vec::new();
            let mut other_blocks: Vec<StructureBlockInfo> = Vec::new();
            let mut be_blocks: Vec<StructureBlockInfo> = Vec::new();

            if let Some(blist) = raw_blocks {
                for entry in blist.iter() {
                    let Some(c) = entry.as_compound() else {
                        continue;
                    };
                    let pos = match c.get("pos").and_then(Tag::as_list) {
                        Some(p) => BlockPos::new(
                            p.first().and_then(Tag::as_int).unwrap_or(0),
                            p.get(1).and_then(Tag::as_int).unwrap_or(0),
                            p.get(2).and_then(Tag::as_int).unwrap_or(0),
                        ),
                        None => BlockPos::ZERO,
                    };
                    let state_idx = c.get("state").and_then(Tag::as_int).unwrap_or(0);
                    let state_id = if state_idx >= 0 {
                        pal.get(state_idx as usize).copied().unwrap_or(air_id)
                    } else {
                        air_id
                    };
                    let nbt = c.get("nbt").and_then(Tag::as_compound).cloned();
                    let info = StructureBlockInfo { pos, state_id, nbt };
                    if info.nbt.is_some() {
                        be_blocks.push(info);
                    } else if is_full_static_block(state_id, facts) {
                        full_blocks.push(info);
                    } else {
                        other_blocks.push(info);
                    }
                }
            }

            let sort_yxz = |a: &StructureBlockInfo, b: &StructureBlockInfo| {
                a.pos
                    .y
                    .cmp(&b.pos.y)
                    .then_with(|| a.pos.x.cmp(&b.pos.x))
                    .then_with(|| a.pos.z.cmp(&b.pos.z))
            };
            full_blocks.sort_by(sort_yxz);
            other_blocks.sort_by(sort_yxz);
            be_blocks.sort_by(sort_yxz);

            let mut jigsaws = Vec::new();
            for info in &be_blocks {
                if let Some(st) = blocks.state_by_id_exact(info.state_id as i32)
                    && st.block_id == jigsaw_block_id
                {
                    let orient_str = st.property_value("orientation").unwrap_or("north_up");
                    let orientation =
                        FrontAndTop::from_name(orient_str).unwrap_or(FrontAndTop::NorthUp);
                    let nbt = info.nbt.as_ref();
                    let name = nbt
                        .and_then(|n| n.get("name"))
                        .and_then(Tag::as_str)
                        .and_then(|s| Identifier::parse(s).ok())
                        .unwrap_or_else(|| empty_id.clone());
                    let target = nbt
                        .and_then(|n| n.get("target"))
                        .and_then(Tag::as_str)
                        .and_then(|s| Identifier::parse(s).ok())
                        .unwrap_or_else(|| empty_id.clone());
                    let pool = nbt
                        .and_then(|n| n.get("pool"))
                        .and_then(Tag::as_str)
                        .and_then(|s| Identifier::parse(s).ok())
                        .unwrap_or_else(|| empty_id.clone());
                    let joint = match nbt.and_then(|n| n.get("joint")).and_then(Tag::as_str) {
                        Some("rollable") => JointType::Rollable,
                        Some("aligned") => JointType::Aligned,
                        _ => {
                            if is_horizontal(orientation.front()) {
                                JointType::Aligned
                            } else {
                                JointType::Rollable
                            }
                        }
                    };
                    let placement_priority = nbt
                        .and_then(|n| n.get("placement_priority"))
                        .and_then(Tag::as_int)
                        .unwrap_or(0);
                    let selection_priority = nbt
                        .and_then(|n| n.get("selection_priority"))
                        .and_then(Tag::as_int)
                        .unwrap_or(0);
                    let final_state = nbt
                        .and_then(|n| n.get("final_state"))
                        .and_then(Tag::as_str)
                        .unwrap_or("minecraft:air")
                        .to_owned();
                    jigsaws.push(JigsawBlockInfo {
                        pos: info.pos,
                        orientation,
                        name: Some(name),
                        target,
                        pool,
                        joint,
                        placement_priority,
                        selection_priority,
                        final_state,
                    });
                }
            }

            let mut all =
                Vec::with_capacity(full_blocks.len() + other_blocks.len() + be_blocks.len());
            all.extend(full_blocks);
            all.extend(other_blocks);
            all.extend(be_blocks);
            palettes.push(TemplatePalette {
                blocks: all,
                jigsaws,
            });
        }

        // Entities
        let mut entities = Vec::new();
        if let Some(elist) = root.get("entities").and_then(Tag::as_list) {
            for entry in elist.iter() {
                let Some(c) = entry.as_compound() else {
                    continue;
                };
                let Some(nbt) = c.get("nbt").and_then(Tag::as_compound) else {
                    continue;
                };
                let pos = match c.get("pos").and_then(Tag::as_list) {
                    Some(p) => [
                        p.first().and_then(Tag::as_double).unwrap_or(0.0),
                        p.get(1).and_then(Tag::as_double).unwrap_or(0.0),
                        p.get(2).and_then(Tag::as_double).unwrap_or(0.0),
                    ],
                    None => [0.0; 3],
                };
                let block_pos = match c.get("blockPos").and_then(Tag::as_list) {
                    Some(p) => BlockPos::new(
                        p.first().and_then(Tag::as_int).unwrap_or(0),
                        p.get(1).and_then(Tag::as_int).unwrap_or(0),
                        p.get(2).and_then(Tag::as_int).unwrap_or(0),
                    ),
                    None => BlockPos::ZERO,
                };
                entities.push(StructureEntityInfo {
                    pos,
                    block_pos,
                    nbt: nbt.clone(),
                });
            }
        }

        Self {
            size,
            palettes,
            entities,
        }
    }

    /// `template.getSize(rotation)` (§4.2).
    #[inline]
    pub const fn rotated_size(&self, rotation: Rotation) -> BlockPos {
        match rotation {
            Rotation::Clockwise90 | Rotation::Counterclockwise90 => {
                BlockPos::new(self.size.z, self.size.y, self.size.x)
            }
            Rotation::None | Rotation::Clockwise180 => self.size,
        }
    }

    /// `template.getBoundingBox(position, rotation, pivot, mirror)` (§4.2).
    #[inline]
    pub fn bounding_box(
        &self,
        position: BlockPos,
        rotation: Rotation,
        pivot: BlockPos,
        mirror: Mirror,
    ) -> BoundingBox {
        let delta = BlockPos::new(self.size.x - 1, self.size.y - 1, self.size.z - 1);
        let c1 = transform_pos(BlockPos::ZERO, mirror, rotation, pivot);
        let c2 = transform_pos(delta, mirror, rotation, pivot);
        BoundingBox::from_corners(c1, c2).moved(position.x, position.y, position.z)
    }

    /// `template.getBoundingBox(settings, position)`.
    #[inline]
    pub fn bounding_box_with_settings(
        &self,
        settings: &StructurePlaceSettings,
        position: BlockPos,
    ) -> BoundingBox {
        self.bounding_box(
            position,
            settings.rotation,
            settings.rotation_pivot,
            settings.mirror,
        )
    }

    /// `StructureTemplate.calculateRelativePosition(settings, pos)` (`structures-legacy-pieces.md` §1.7).
    #[inline]
    pub const fn calculate_relative_position(
        settings: &StructurePlaceSettings,
        pos: BlockPos,
    ) -> BlockPos {
        transform_pos(
            pos,
            settings.mirror,
            settings.rotation,
            settings.rotation_pivot,
        )
    }

    /// `StructureTemplate.calculateConnectedPosition(settings1, conn1, settings2, conn2)`.
    #[inline]
    pub const fn calculate_connected_position(
        settings1: &StructurePlaceSettings,
        conn1: BlockPos,
        settings2: &StructurePlaceSettings,
        conn2: BlockPos,
    ) -> BlockPos {
        Self::calculate_relative_position(settings1, conn1)
            .sub(Self::calculate_relative_position(settings2, conn2))
    }

    /// `template.getJigsaws(pos, rotation)`: returns all jigsaw blocks transformed by `rotation`
    /// and translated by `pos` (in exact palette Y-X-Z order).
    pub fn get_jigsaws(&self, pos: BlockPos, rotation: Rotation) -> Vec<JigsawBlockInfo> {
        if self.palettes.is_empty() {
            return Vec::new();
        }
        let pal_idx = if self.palettes.len() == 1 {
            0
        } else {
            let mut rng = LegacyRandom::new(position_hash(pos.x, pos.y, pos.z));
            rng.next_int_bounded(self.palettes.len() as i32) as usize
        };
        self.palettes[pal_idx]
            .jigsaws
            .iter()
            .map(|j| {
                let mut out = j.clone();
                out.pos = transform_pos(j.pos, Mirror::None, rotation, BlockPos::ZERO).add(pos);
                out.orientation = j.orientation.rotate(rotation);
                out
            })
            .collect()
    }
}

fn is_full_static_block(state_id: u32, facts: Option<&BlockFacts>) -> bool {
    let Some(facts) = facts else {
        return false;
    };
    let Some(sf) = facts.states().get(state_id as usize) else {
        return false;
    };
    !sf.has(flag::HAS_DYNAMIC_SHAPE) && sf.has(flag::COLLISION_SHAPE_FULL_BLOCK)
}

fn decode_nbt_palette(
    palette_list: &zenith_nbt::List,
    blocks: &BlockStateTable,
    air_id: u32,
) -> Vec<u32> {
    let mut out = Vec::with_capacity(palette_list.len());
    for entry in palette_list.iter() {
        let Some(c) = entry.as_compound() else {
            out.push(air_id);
            continue;
        };
        let id_str = c
            .get("id")
            .and_then(Tag::as_str)
            .or_else(|| c.get("Name").and_then(Tag::as_str));
        let Some(id_str) = id_str else {
            out.push(air_id);
            continue;
        };
        let Ok(name) = Identifier::parse(id_str) else {
            out.push(air_id);
            continue;
        };
        let mut properties = BTreeMap::new();
        let props_tag = c
            .get("properties")
            .and_then(Tag::as_compound)
            .or_else(|| c.get("Properties").and_then(Tag::as_compound));
        if let Some(pc) = props_tag {
            for (k, v) in pc.iter() {
                if let (Some(ks), Some(vs)) = (k.as_str(), v.as_str()) {
                    properties.insert(ks.to_owned(), vs.to_owned());
                }
            }
        }
        let spec = BlockStateSpec { name, properties };
        let state_id = resolve_block_state(blocks, &spec).unwrap_or(air_id);
        out.push(state_id);
    }
    out
}

/// Caching loader for `.nbt` structure templates backed by a [`PackResources`] (`StructureTemplateManager`).
pub struct TemplateManager {
    pack: Arc<dyn PackResources + Send + Sync>,
    blocks: Arc<BlockStateTable>,
    facts: Option<Arc<BlockFacts>>,
    cache: RwLock<HashMap<Identifier, Arc<StructureTemplate>>>,
}

impl std::fmt::Debug for TemplateManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TemplateManager")
            .field("cached", &self.cache.read().map(|c| c.len()).unwrap_or(0))
            .finish()
    }
}

impl TemplateManager {
    /// Creates a new `TemplateManager` reading `data/<ns>/structure/<path>.nbt` from `pack`.
    pub fn new(
        pack: Arc<dyn PackResources + Send + Sync>,
        blocks: Arc<BlockStateTable>,
        facts: Option<Arc<BlockFacts>>,
    ) -> Self {
        Self {
            pack,
            blocks,
            facts,
            cache: RwLock::new(HashMap::new()),
        }
    }

    /// Loads (or returns cached) template `id`.
    pub fn get(&self, id: &Identifier) -> Result<Arc<StructureTemplate>, TemplateError> {
        if let Ok(guard) = self.cache.read()
            && let Some(t) = guard.get(id)
        {
            return Ok(Arc::clone(t));
        }
        let file_path = format!("structure/{}.nbt", id.path());
        let file_id = Identifier::from_parts(id.namespace(), &file_path)
            .map_err(|_| TemplateError::Missing { id: id.clone() })?;
        let bytes = self
            .pack
            .get(PackType::Data, &file_id)
            .ok_or_else(|| TemplateError::Missing { id: id.clone() })?;
        let template = Arc::new(StructureTemplate::from_gzip_nbt(
            id,
            &bytes,
            &self.blocks,
            self.facts.as_deref(),
        )?);
        if let Ok(mut guard) = self.cache.write() {
            guard.insert(id.clone(), Arc::clone(&template));
        }
        Ok(template)
    }

    /// Inserts a pre-built template into the cache (useful for unit tests without a jar).
    pub fn insert(&self, id: Identifier, template: StructureTemplate) {
        if let Ok(mut guard) = self.cache.write() {
            guard.insert(id, Arc::new(template));
        }
    }

    /// Underlying [`BlockStateTable`].
    #[inline]
    pub fn blocks(&self) -> &BlockStateTable {
        &self.blocks
    }

    /// Optional [`BlockFacts`].
    #[inline]
    pub fn facts(&self) -> Option<&BlockFacts> {
        self.facts.as_deref()
    }
}

// ---------------------------------------------------------------------------
// §4.3–§4.4 Processor pipeline (for template placement)
// ---------------------------------------------------------------------------

/// World view required by [`process_block_infos`] when running structure processors.
pub trait ProcessorWorldView {
    /// World seed (for `CappedProcessor`).
    fn world_seed(&self) -> i64;
    /// Whether this is a live `ServerLevel` (remaps `*_WG` heightmaps in `GravityProcessor`).
    fn is_live_server_level(&self) -> bool;
    /// Reads the existing world block state ID at `pos`.
    fn get_block_state(&self, pos: BlockPos) -> u32;
    /// Reads `level.getHeight(kind, x, z)` (first available Y).
    fn get_height(&self, kind: HeightmapKind, x: i32, z: i32) -> i32;
    /// Block state table for resolving IDs and properties.
    fn blocks(&self) -> &BlockStateTable;
    /// Resolved block tags.
    fn block_tags(&self) -> &BlockTags;
    /// Optional block facts for shape checks (`LavaSubmergedBlockProcessor`).
    fn block_facts(&self) -> Option<&BlockFacts>;
}

/// Runs `StructureTemplate.processBlockInfos` (§4.3–§4.4).
pub fn process_block_infos(
    world: &dyn ProcessorWorldView,
    position: BlockPos,
    reference_pos: BlockPos,
    settings: &mut StructurePlaceSettings,
    block_info_list: &[StructureBlockInfo],
) -> Vec<StructureBlockInfo> {
    let any_whole_piece = settings
        .processors
        .iter()
        .any(StructureProcessor::evaluates_entire_piece_state);
    let process_only_in_chunk = !any_whole_piece;

    let mut original_list = Vec::with_capacity(block_info_list.len());
    let mut processed_list = Vec::with_capacity(block_info_list.len());

    let processors = settings.processors.clone();
    for info in block_info_list {
        let world_pos = transform_pos(
            info.pos,
            settings.mirror,
            settings.rotation,
            settings.rotation_pivot,
        )
        .add(position);
        if process_only_in_chunk
            && let Some(bb) = settings.bounding_box
            && !bb.is_inside(world_pos)
        {
            continue;
        }
        let mut cur = Some(StructureBlockInfo {
            pos: world_pos,
            state_id: info.state_id,
            nbt: info.nbt.clone(),
        });
        for proc in &processors {
            let Some(b) = cur else { break };
            cur = apply_processor(world, position, reference_pos, info.pos, b, proc, settings);
        }
        if let Some(b) = cur {
            processed_list.push(b);
            original_list.push(info.clone());
        }
    }

    for proc in &processors {
        if let StructureProcessor::Capped { delegate, limit } = proc {
            processed_list = finalize_capped(
                world,
                position,
                reference_pos,
                &original_list,
                processed_list,
                delegate,
                limit,
                settings,
            );
        }
    }

    processed_list
}

fn block_matches_holder_set(
    state_id: u32,
    set: &RawBlockHolderSet,
    blocks: &BlockStateTable,
    tags: &BlockTags,
) -> bool {
    let Some(st) = blocks.state_by_id_exact(state_id as i32) else {
        return false;
    };
    match set {
        RawBlockHolderSet::Tag(tag_id) => tags.contains_block(tag_id, st.block_index),
        RawBlockHolderSet::Ids(ids) => ids.contains(&st.block_id),
    }
}

fn eval_rule_test(
    test: &RuleTest,
    state_id: u32,
    world_pos: BlockPos,
    rng: &mut LegacyRandom,
    blocks: &BlockStateTable,
    tags: &BlockTags,
) -> bool {
    match test {
        RuleTest::AlwaysTrue => true,
        RuleTest::BlockMatch { block } => blocks
            .state_by_id_exact(state_id as i32)
            .is_some_and(|st| st.block_id == *block),
        RuleTest::BlockStateMatch { block_state } => {
            resolve_block_state(blocks, block_state).ok() == Some(state_id)
        }
        RuleTest::TagMatch { tag } => blocks
            .state_by_id_exact(state_id as i32)
            .is_some_and(|st| tags.contains_block(tag, st.block_index)),
        RuleTest::RandomBlockMatch { block, probability } => {
            blocks
                .state_by_id_exact(state_id as i32)
                .is_some_and(|st| st.block_id == *block)
                && rng.next_float() < *probability
        }
        RuleTest::RandomBlockStateMatch {
            block_state,
            probability,
        } => {
            resolve_block_state(blocks, block_state).ok() == Some(state_id)
                && rng.next_float() < *probability
        }
        RuleTest::HeightMatch {
            min_inclusive,
            max_inclusive,
        } => world_pos.y >= *min_inclusive && world_pos.y <= *max_inclusive,
        RuleTest::AllOf { rules } => rules
            .iter()
            .all(|r| eval_rule_test(r, state_id, world_pos, rng, blocks, tags)),
        RuleTest::AnyOf { rules } => rules
            .iter()
            .any(|r| eval_rule_test(r, state_id, world_pos, rng, blocks, tags)),
        RuleTest::Not { rule } => !eval_rule_test(rule, state_id, world_pos, rng, blocks, tags),
    }
}

fn eval_pos_rule_test(
    test: &PosRuleTest,
    world_pos: BlockPos,
    world_ref: BlockPos,
    rng: &mut LegacyRandom,
) -> bool {
    match *test {
        PosRuleTest::AlwaysTrue => true,
        PosRuleTest::LinearPos {
            min_chance,
            max_chance,
            min_dist,
            max_dist,
        } => {
            let dist = world_pos.dist_manhattan(world_ref);
            let t = inverse_lerp_f32(dist as f32, min_dist as f32, max_dist as f32);
            rng.next_float() <= clamped_lerp_f32(min_chance, max_chance, t)
        }
        PosRuleTest::AxisAlignedLinearPos {
            min_chance,
            max_chance,
            min_dist,
            max_dist,
            axis,
        } => {
            let diff = match axis {
                Axis::X => world_pos.x - world_ref.x,
                Axis::Y => world_pos.y - world_ref.y,
                Axis::Z => world_pos.z - world_ref.z,
            };
            let dist = (diff as f32).abs() as i32;
            let t = inverse_lerp_f32(dist as f32, min_dist as f32, max_dist as f32);
            rng.next_float() <= clamped_lerp_f32(min_chance, max_chance, t)
        }
    }
}

fn apply_be_modifier(
    modifier: &RuleBlockEntityModifier,
    rng: &mut LegacyRandom,
    existing: Option<Compound>,
) -> Option<Compound> {
    match modifier {
        RuleBlockEntityModifier::Passthrough => existing,
        RuleBlockEntityModifier::Clear => Some(Compound::new()),
        RuleBlockEntityModifier::AppendStatic { data } => {
            let mut base = existing.unwrap_or_default();
            if let Some(obj) = data.as_object() {
                for (k, v) in obj.iter() {
                    if let (Some(ks), Some(tag)) = (k.as_str(), json_to_nbt(v)) {
                        base.insert(ks, tag);
                    }
                }
            }
            Some(base)
        }
        RuleBlockEntityModifier::AppendLoot { loot_table } => {
            let mut base = existing.unwrap_or_default();
            base.insert("LootTable", Tag::from(loot_table.to_string()));
            base.insert("LootTableSeed", Tag::Long(rng.next_long()));
            Some(base)
        }
    }
}

fn json_to_nbt(j: &Json) -> Option<Tag> {
    match j {
        Json::Null => None,
        Json::Bool(b) => Some(Tag::Byte(i8::from(*b))),
        Json::Number(n) => {
            let text = n.text();
            if let Ok(i) = text.parse::<i32>() {
                Some(Tag::Int(i))
            } else if let Ok(l) = text.parse::<i64>() {
                Some(Tag::Long(l))
            } else if let Ok(f) = text.parse::<f64>() {
                Some(Tag::Double(f))
            } else {
                None
            }
        }
        Json::String(s) => s.as_str().map(Tag::from),
        Json::Array(arr) => {
            let mut list = zenith_nbt::List::new();
            for item in arr {
                if let Some(t) = json_to_nbt(item) {
                    list.push(t);
                }
            }
            Some(Tag::List(list))
        }
        Json::Object(obj) => {
            let mut c = Compound::new();
            for (k, v) in obj.iter() {
                if let (Some(ks), Some(t)) = (k.as_str(), json_to_nbt(v)) {
                    c.insert(ks, t);
                }
            }
            Some(Tag::Compound(c))
        }
    }
}

fn apply_processor(
    world: &dyn ProcessorWorldView,
    _position: BlockPos,
    reference_pos: BlockPos,
    template_rel_pos: BlockPos,
    mut info: StructureBlockInfo,
    proc: &StructureProcessor,
    settings: &mut StructurePlaceSettings,
) -> Option<StructureBlockInfo> {
    let blocks = world.blocks();
    let tags = world.block_tags();
    match proc {
        StructureProcessor::Nop | StructureProcessor::Capped { .. } => Some(info),
        StructureProcessor::BlockIgnore { blocks: ignored } => {
            if let Some(st) = blocks.state_by_id_exact(info.state_id as i32)
                && ignored.contains(&st.block_id)
            {
                return None;
            }
            Some(info)
        }
        StructureProcessor::BlockRot {
            rottable_blocks,
            integrity,
        } => {
            let mut rng = settings.get_random(Some(info.pos));
            let matches = match rottable_blocks {
                None => true,
                Some(set) => block_matches_holder_set(info.state_id, set, blocks, tags),
            };
            if matches && (rng.next_float() > *integrity || integrity.is_nan()) {
                return None;
            }
            Some(info)
        }
        StructureProcessor::Gravity { heightmap, offset } => {
            let hm = if world.is_live_server_level() {
                match heightmap {
                    HeightmapKind::WorldSurfaceWg => HeightmapKind::WorldSurface,
                    HeightmapKind::OceanFloorWg => HeightmapKind::OceanFloor,
                    other => *other,
                }
            } else {
                *heightmap
            };
            let h = world.get_height(hm, info.pos.x, info.pos.z) + offset;
            info.pos.y = h + template_rel_pos.y;
            Some(info)
        }
        StructureProcessor::Rule { rules } => {
            let mut rng = LegacyRandom::new(position_hash(info.pos.x, info.pos.y, info.pos.z));
            let mut world_state: Option<u32> = None;
            for rule in rules {
                if !eval_rule_test(
                    &rule.input_predicate,
                    info.state_id,
                    info.pos,
                    &mut rng,
                    blocks,
                    tags,
                ) {
                    continue;
                }
                let loc_ok = match &rule.location_predicate {
                    RuleTest::AlwaysTrue => true,
                    other => {
                        let ws =
                            *world_state.get_or_insert_with(|| world.get_block_state(info.pos));
                        eval_rule_test(other, ws, info.pos, &mut rng, blocks, tags)
                    }
                };
                if !loc_ok {
                    continue;
                }
                if !eval_pos_rule_test(&rule.position_predicate, info.pos, reference_pos, &mut rng)
                {
                    continue;
                }
                let out_id =
                    resolve_block_state(blocks, &rule.output_state).unwrap_or(info.state_id);
                let out_nbt = apply_be_modifier(&rule.block_entity_modifier, &mut rng, info.nbt);
                return Some(StructureBlockInfo {
                    pos: info.pos,
                    state_id: out_id,
                    nbt: out_nbt,
                });
            }
            Some(info)
        }
        StructureProcessor::ProtectedBlocks { value } => {
            let ws = world.get_block_state(info.pos);
            if block_matches_holder_set(ws, value, blocks, tags) {
                return None;
            }
            Some(info)
        }
        StructureProcessor::JigsawReplacement => {
            let is_jigsaw = blocks
                .state_by_id_exact(info.state_id as i32)
                .is_some_and(|st| {
                    st.block_id.namespace() == "minecraft" && st.block_id.path() == "jigsaw"
                });
            if !is_jigsaw {
                return Some(info);
            }
            let Some(nbt) = info.nbt.as_ref() else {
                return Some(info);
            };
            let state_str = nbt
                .get("final_state")
                .and_then(Tag::as_str)
                .unwrap_or("minecraft:air");
            let spec = parse_command_block_state(state_str)?;
            if spec.name.namespace() == "minecraft" && spec.name.path() == "structure_void" {
                return None;
            }
            let state_id = resolve_block_state(blocks, &spec).ok()?;
            Some(StructureBlockInfo {
                pos: info.pos,
                state_id,
                nbt: None,
            })
        }
        StructureProcessor::LavaSubmergedBlock => {
            let ws = world.get_block_state(info.pos);
            let is_lava = blocks.state_by_id_exact(ws as i32).is_some_and(|st| {
                st.block_id.namespace() == "minecraft" && st.block_id.path() == "lava"
            });
            if is_lava {
                let is_full = world
                    .block_facts()
                    .and_then(|f| f.states().get(info.state_id as usize))
                    .is_some_and(|s| s.has(flag::COLLISION_SHAPE_FULL_BLOCK));
                if !is_full
                    && let Some(lava) =
                        blocks.block(&Identifier::with_default_namespace("lava").expect("valid"))
                {
                    info.state_id = lava.default_state;
                }
            }
            Some(info)
        }
        StructureProcessor::BlockAge { mossiness } => {
            let mut rng = settings.get_random(Some(info.pos));
            if let Some(new_id) = apply_block_age(info.state_id, *mossiness, &mut rng, blocks, tags)
            {
                info.state_id = new_id;
            }
            Some(info)
        }
        StructureProcessor::BlackstoneReplace => {
            if let Some(new_id) = apply_blackstone_replace(info.state_id, blocks) {
                info.state_id = new_id;
            }
            Some(info)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn finalize_capped(
    world: &dyn ProcessorWorldView,
    position: BlockPos,
    reference_pos: BlockPos,
    original_list: &[StructureBlockInfo],
    mut processed_list: Vec<StructureBlockInfo>,
    delegate: &StructureProcessor,
    limit: &zenith_worldgen::IntProvider,
    settings: &mut StructurePlaceSettings,
) -> Vec<StructureBlockInfo> {
    if processed_list.is_empty() || original_list.len() != processed_list.len() {
        return processed_list;
    }
    let mut rng = LegacyRandom::new(world.world_seed())
        .fork_positional()
        .at(position.x, position.y, position.z);
    let max_to_replace = (limit.sample(&mut rng).max(0) as usize).min(processed_list.len());
    if max_to_replace < 1 {
        return processed_list;
    }
    let mut indices: Vec<usize> = (0..processed_list.len()).collect();
    shuffle_slice(&mut indices, &mut rng);
    let mut replaced = 0usize;
    for idx in indices {
        if replaced >= max_to_replace {
            break;
        }
        let orig = &original_list[idx];
        let cur = processed_list[idx].clone();
        if let Some(altered) = apply_processor(
            world,
            position,
            reference_pos,
            orig.pos,
            cur.clone(),
            delegate,
            settings,
        ) && altered != cur
        {
            processed_list[idx] = altered;
            replaced += 1;
        }
    }
    processed_list
}

fn parse_command_block_state(s: &str) -> Option<BlockStateSpec> {
    let s = s.trim();
    if let Some(bracket) = s.find('[') {
        let name = Identifier::parse(&s[..bracket]).ok()?;
        let rest = s[bracket + 1..].strip_suffix(']')?;
        let mut properties = BTreeMap::new();
        if !rest.is_empty() {
            for pair in rest.split(',') {
                let (k, v) = pair.split_once('=')?;
                properties.insert(k.trim().to_owned(), v.trim().to_owned());
            }
        }
        Some(BlockStateSpec { name, properties })
    } else {
        Some(BlockStateSpec {
            name: Identifier::parse(s).ok()?,
            properties: BTreeMap::new(),
        })
    }
}

fn with_properties_of(
    target_block: &str,
    src_state_id: u32,
    blocks: &BlockStateTable,
) -> Option<u32> {
    let src = blocks.state_by_id_exact(src_state_id as i32)?;
    let id = Identifier::with_default_namespace(target_block).ok()?;
    let mut props = BTreeMap::new();
    for (k, v) in &src.properties {
        props.insert(k.clone(), v.clone());
    }
    resolve_block_state(
        blocks,
        &BlockStateSpec {
            name: id,
            properties: props,
        },
    )
    .ok()
}

fn random_facing_stairs(
    block: &str,
    rng: &mut LegacyRandom,
    blocks: &BlockStateTable,
) -> Option<u32> {
    let facing = random_horizontal_direction(rng).name();
    let half = if rng.next_int_bounded(2) == 0 {
        "top"
    } else {
        "bottom"
    };
    let mut props = BTreeMap::new();
    props.insert("facing".to_owned(), facing.to_owned());
    props.insert("half".to_owned(), half.to_owned());
    resolve_block_state(
        blocks,
        &BlockStateSpec {
            name: Identifier::with_default_namespace(block).ok()?,
            properties: props,
        },
    )
    .ok()
}

fn default_state_of(block: &str, blocks: &BlockStateTable) -> Option<u32> {
    let id = Identifier::with_default_namespace(block).ok()?;
    blocks.block(&id).map(|b| b.default_state)
}

fn apply_block_age(
    state_id: u32,
    mossiness: f32,
    rng: &mut LegacyRandom,
    blocks: &BlockStateTable,
    tags: &BlockTags,
) -> Option<u32> {
    let st = blocks.state_by_id_exact(state_id as i32)?;
    let path = if st.block_id.namespace() == "minecraft" {
        st.block_id.path()
    } else {
        ""
    };
    if matches!(path, "stone_bricks" | "stone" | "chiseled_stone_bricks") {
        if rng.next_float() < 0.5_f32 {
            let non_mossy_stairs = random_facing_stairs("stone_brick_stairs", rng, blocks)?;
            let mossy_stairs = random_facing_stairs("mossy_stone_brick_stairs", rng, blocks)?;
            let is_mossy = rng.next_float() < mossiness;
            let pick = rng.next_int_bounded(2);
            return if is_mossy {
                if pick == 0 {
                    default_state_of("mossy_stone_bricks", blocks)
                } else {
                    Some(mossy_stairs)
                }
            } else if pick == 0 {
                default_state_of("cracked_stone_bricks", blocks)
            } else {
                Some(non_mossy_stairs)
            };
        }
        return None;
    }
    let stairs_tag = Identifier::with_default_namespace("stairs").expect("valid");
    let slabs_tag = Identifier::with_default_namespace("slabs").expect("valid");
    let walls_tag = Identifier::with_default_namespace("walls").expect("valid");
    if tags.contains_block(&stairs_tag, st.block_index) {
        if rng.next_float() < 0.5_f32 {
            let is_mossy = rng.next_float() < mossiness;
            let pick = rng.next_int_bounded(2);
            return if is_mossy {
                if pick == 0 {
                    with_properties_of("mossy_stone_brick_stairs", state_id, blocks)
                } else {
                    default_state_of("mossy_stone_brick_slab", blocks)
                }
            } else if pick == 0 {
                default_state_of("stone_slab", blocks)
            } else {
                default_state_of("stone_brick_slab", blocks)
            };
        }
        return None;
    }
    if tags.contains_block(&slabs_tag, st.block_index) {
        if rng.next_float() < mossiness {
            return with_properties_of("mossy_stone_brick_slab", state_id, blocks);
        }
        return None;
    }
    if tags.contains_block(&walls_tag, st.block_index) {
        if rng.next_float() < mossiness {
            return with_properties_of("mossy_stone_brick_wall", state_id, blocks);
        }
        return None;
    }
    if path == "obsidian" && rng.next_float() < 0.15_f32 {
        return default_state_of("crying_obsidian", blocks);
    }
    None
}

fn apply_blackstone_replace(state_id: u32, blocks: &BlockStateTable) -> Option<u32> {
    let st = blocks.state_by_id_exact(state_id as i32)?;
    if st.block_id.namespace() != "minecraft" {
        return None;
    }
    let replacement = match st.block_id.path() {
        "cobblestone" | "mossy_cobblestone" => "blackstone",
        "stone" => "polished_blackstone",
        "stone_bricks" | "mossy_stone_bricks" => "polished_blackstone_bricks",
        "chiseled_stone_bricks" => "chiseled_polished_blackstone",
        "cracked_stone_bricks" => "cracked_polished_blackstone_bricks",
        "iron_bars" => "iron_chain",
        "cobblestone_stairs" | "mossy_cobblestone_stairs" => "blackstone_stairs",
        "stone_stairs" => "polished_blackstone_stairs",
        "stone_brick_stairs" | "mossy_stone_brick_stairs" => "polished_blackstone_brick_stairs",
        "cobblestone_slab" | "mossy_cobblestone_slab" => "blackstone_slab",
        "smooth_stone_slab" | "stone_slab" => "polished_blackstone_slab",
        "stone_brick_slab" | "mossy_stone_brick_slab" => "polished_blackstone_brick_slab",
        "cobblestone_wall" | "mossy_cobblestone_wall" => "blackstone_wall",
        "stone_brick_wall" | "mossy_stone_brick_wall" => "polished_blackstone_brick_wall",
        _ => return None,
    };
    let mut props = BTreeMap::new();
    for key in ["facing", "half", "type"] {
        if let Some(v) = st.property_value(key) {
            props.insert(key.to_owned(), v.to_owned());
        }
    }
    resolve_block_state(
        blocks,
        &BlockStateSpec {
            name: Identifier::with_default_namespace(replacement).ok()?,
            properties: props,
        },
    )
    .ok()
}

/// `state.mirror(mirror).rotate(rotation)` for a global block-state id (see [`crate::block_xform`]).
pub fn transform_block_state(
    state_id: u32,
    mirror: Mirror,
    rotation: Rotation,
    blocks: &BlockStateTable,
) -> u32 {
    crate::block_xform::mirror_rotate_state(blocks, state_id, mirror, rotation)
}
