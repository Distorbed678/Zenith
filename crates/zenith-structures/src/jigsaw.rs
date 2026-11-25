//! Jigsaw structure assembly (`JigsawStructure`, `JigsawPlacement`, `PoolAliasLookup`,
//! `SequencedPriorityIterator`, `Placer`)
//! (`docs/specs/worldgen/structures-placement-and-jigsaw.md` §3).

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::rc::Rc;
use std::sync::Arc;

use zenith_blocks::Aabb;
use zenith_math::{LegacyRandom, PositionalRandomFactory, RandomSource};
use zenith_registry::Identifier;
use zenith_worldgen::{HeightmapKind, WorldGenerationContext};

use crate::data::{
    JigsawConfig, LiquidSettings, PoolAliasBinding, Projection, StructureData,
    StructurePoolElement, StructureTemplatePool, TemplatePoolRef,
};
use crate::geom::{
    BlockPos, BoundingBox, BoundingBoxExt, FrontAndTop, Mirror, Rotation, sample_height_provider,
    shuffle_slice,
};
use crate::template::{JigsawBlockInfo, JointType, TemplateManager};

const EPSILON: f64 = 1.0e-7;

/// Recorded jigsaw junction (`JigsawJunction`, §3.6 & §5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StructureJigsawJunction {
    /// Target/source block X.
    pub source_x: i32,
    /// Computed ground Y.
    pub source_ground_y: i32,
    /// Target/source block Z.
    pub source_z: i32,
    /// Vertical delta between connected jigsaws.
    pub delta_y: i32,
    /// Destination piece projection.
    pub dest_projection: Projection,
}

/// One assembled `PoolElementStructurePiece` (`minecraft:jigsaw`).
#[derive(Debug, Clone)]
pub struct PoolElementPiece {
    /// Pool element that created this piece.
    pub element: Arc<StructurePoolElement>,
    /// Piece world origin (`PosX, PosY, PosZ`).
    pub position: BlockPos,
    /// Ground level delta (`ground_level_delta`).
    pub ground_level_delta: i32,
    /// Piece rotation.
    pub rotation: Rotation,
    /// Piece bounding box (may include `use_expansion_hack` vertical expansion).
    pub bounding_box: BoundingBox,
    /// Liquid settings.
    pub liquid_settings: LiquidSettings,
    /// Recorded jigsaw junctions in insertion order.
    pub junctions: Vec<StructureJigsawJunction>,
}

impl PoolElementPiece {
    /// Shifts `position` and `bounding_box` by `(dx, dy, dz)`.
    pub fn move_by(&mut self, dx: i32, dy: i32, dz: i32) {
        self.position = self.position.offset(dx, dy, dz);
        self.bounding_box = self.bounding_box.moved(dx, dy, dz);
    }
}

/// Resolved `PoolAliasLookup` (§3.4).
#[derive(Debug, Clone, Default)]
pub struct PoolAliasLookup {
    map: HashMap<Identifier, Identifier>,
}

impl PoolAliasLookup {
    /// `PoolAliasLookup.create(poolAliases, startPos, worldSeed)` (§3.4).
    pub fn create(aliases: &[PoolAliasBinding], start_pos: BlockPos, world_seed: i64) -> Self {
        if aliases.is_empty() {
            return Self::default();
        }
        let mut rng = LegacyRandom::new(world_seed).fork_positional().at(
            start_pos.x,
            start_pos.y,
            start_pos.z,
        );
        let mut map = HashMap::new();
        for binding in aliases {
            resolve_binding(binding, &mut rng, &mut map);
        }
        Self { map }
    }

    /// Returns the mapped target pool ID if present, else `key` unchanged.
    #[inline]
    pub fn lookup<'a>(&'a self, key: &'a Identifier) -> &'a Identifier {
        self.map.get(key).unwrap_or(key)
    }
}

fn resolve_binding(
    binding: &PoolAliasBinding,
    rng: &mut LegacyRandom,
    map: &mut HashMap<Identifier, Identifier>,
) {
    match binding {
        PoolAliasBinding::Direct { alias, target } => {
            map.insert(alias.clone(), target.clone());
        }
        PoolAliasBinding::Random { alias, targets } => {
            if let Some(target) = pick_weighted(targets, rng) {
                map.insert(alias.clone(), target.clone());
            }
        }
        PoolAliasBinding::RandomGroup { groups } => {
            if let Some(group) = pick_weighted(groups, rng) {
                for sub in group {
                    resolve_binding(sub, rng, map);
                }
            }
        }
    }
}

fn pick_weighted<'a, T>(entries: &'a [(T, i32)], rng: &mut LegacyRandom) -> Option<&'a T> {
    let total: i32 = entries.iter().map(|(_, w)| *w).sum();
    if total <= 0 || entries.is_empty() {
        return None;
    }
    let mut s = rng.next_int_bounded(total);
    for (item, weight) in entries {
        s -= *weight;
        if s < 0 {
            return Some(item);
        }
    }
    None
}

/// Exact set-algebra representation of `outer \ (∪ occupied_i)`, mathematically equivalent to
/// `VoxelShape` `Shapes.joinUnoptimized(..., ONLY_FIRST)` and `Shapes.joinIsNotEmpty(..., ONLY_SECOND)`.
#[derive(Debug, Clone)]
pub(crate) struct FreeSpace {
    outer: Aabb,
    occupied: Vec<Aabb>,
}

impl FreeSpace {
    pub(crate) fn new(outer: Aabb) -> Self {
        Self {
            outer,
            occupied: Vec::new(),
        }
    }

    pub(crate) fn subtract(&mut self, box_aabb: Aabb) {
        self.occupied.push(box_aabb);
    }

    /// Returns `true` iff `cand_deflated` lies completely inside `outer \ (∪ occupied)`
    /// (i.e. `!Shapes.joinIsNotEmpty(free, cand_deflated, ONLY_SECOND)`).
    pub(crate) fn fits_deflated(&self, cand_deflated: &Aabb) -> bool {
        // Must be contained inside `outer` (within EPSILON tolerance).
        for a in 0..3 {
            if self.outer.max[a] - self.outer.min[a] < EPSILON {
                return false;
            }
            if cand_deflated.min[a] < self.outer.min[a] - EPSILON
                || cand_deflated.max[a] > self.outer.max[a] + EPSILON
            {
                return false;
            }
        }
        // Must not overlap any already-occupied box (within EPSILON tolerance).
        for occ in &self.occupied {
            let overlaps = (0..3).all(|a| {
                cand_deflated.min[a] < occ.max[a] - EPSILON
                    && occ.min[a] < cand_deflated.max[a] - EPSILON
            });
            if overlaps {
                return false;
            }
        }
        true
    }
}

/// Priority-ordered BFS queue (`SequencedPriorityIterator`, §3.6):
/// highest integer priority pops first; equal priorities pop in FIFO order.
pub(crate) struct PriorityQueue<T> {
    queues: BTreeMap<i32, VecDeque<T>>,
}

impl<T> PriorityQueue<T> {
    pub(crate) fn new() -> Self {
        Self {
            queues: BTreeMap::new(),
        }
    }

    pub(crate) fn add(&mut self, item: T, priority: i32) {
        self.queues.entry(priority).or_default().push_back(item);
    }

    pub(crate) fn pop_highest(&mut self) -> Option<T> {
        let (&highest_key, _) = self.queues.iter().next_back()?;
        let item = self.queues.get_mut(&highest_key)?.pop_front();
        if self
            .queues
            .get(&highest_key)
            .is_some_and(VecDeque::is_empty)
        {
            self.queues.remove(&highest_key);
        }
        item
    }
}

struct PieceState {
    piece_idx: usize,
    free: Rc<RefCell<FreeSpace>>,
    depth: i32,
}

/// Context callbacks required by [`generate_jigsaw_pieces`] (keeps jigsaw assembly decoupled from
/// the top-level generator).
pub trait JigsawGenContext {
    /// World seed.
    fn world_seed(&self) -> i64;
    /// Dimension vertical bounds `(min_y, height)` for `heightAccessor`.
    fn min_y(&self) -> i32;
    /// Inclusive max Y (`min_y + height - 1`).
    fn max_y(&self) -> i32;
    /// `WorldGenerationContext` for `start_height.sample`.
    fn world_gen_context(&self) -> WorldGenerationContext;
    /// Loaded structure/template-pool registries.
    fn structure_data(&self) -> &StructureData;
    /// `.nbt` template manager.
    fn template_manager(&self) -> &TemplateManager;
    /// 26.3 column biome pre-check (`context.couldStructureExistInColumn`).
    fn could_structure_exist_in_column(
        &self,
        block_x: i32,
        block_z: i32,
        min_block_y: i32,
        max_block_y: i32,
    ) -> bool;
    /// `context.isValidBiome(stubPos)`.
    fn is_valid_biome(&mut self, pos: BlockPos) -> bool;
    /// `chunkGenerator.getFirstFreeHeight(x, z, hmType, ...)`.
    fn first_free_height(&self, x: i32, z: i32, kind: HeightmapKind) -> i32;
}

/// Runs both Phase A and Phase B of `JigsawPlacement.addPieces` (§3.6), returning the assembled
/// `PoolElementPiece` list if valid (or `None` if aborted / biome rejected / `size == 0`).
pub fn generate_jigsaw_pieces(
    cfg: &JigsawConfig,
    chunk_x: i32,
    chunk_z: i32,
    rng: &mut LegacyRandom,
    ctx: &mut dyn JigsawGenContext,
) -> Option<Vec<PoolElementPiece>> {
    let data = ctx.structure_data();
    let tm = ctx.template_manager();

    // Phase A — Start piece selection, anchoring, and vertical placement.
    let height = sample_height_provider(&cfg.start_height, rng, ctx.world_gen_context());
    let position = BlockPos::new(chunk_x << 4, height, chunk_z << 4);
    let pool_alias_lookup = PoolAliasLookup::create(&cfg.pool_aliases, position, ctx.world_seed());
    let center_rotation = Rotation::random(rng);

    let center_pool: &StructureTemplatePool = match &cfg.start_pool {
        TemplatePoolRef::Reference(k) => {
            let aliased = pool_alias_lookup.lookup(k);
            data.template_pool(aliased)
                .or_else(|| data.template_pool(k))
                .map(Arc::as_ref)?
        }
        TemplatePoolRef::Inline(p) => p.as_ref(),
    };

    let center_element = get_random_template(center_pool, rng);
    if center_element.is_empty() {
        return None;
    }

    let anchored_position = if let Some(ref start_name) = cfg.start_jigsaw_name {
        let jigsaws =
            get_shuffled_jigsaw_blocks(&center_element, tm, position, center_rotation, rng)?;
        let found = jigsaws
            .into_iter()
            .find(|j| j.name.as_ref() == Some(start_name))?;
        found.pos
    } else {
        position
    };

    let local_anchor = anchored_position.sub(position);
    let adjusted_position = position.sub(local_anchor);

    let box0 = element_bounding_box(&center_element, tm, adjusted_position, center_rotation)?;
    let mut center_piece = PoolElementPiece {
        element: center_element,
        position: adjusted_position,
        ground_level_delta: 0, // set immediately below
        rotation: center_rotation,
        bounding_box: box0,
        liquid_settings: cfg.liquid_settings,
        junctions: Vec::new(),
    };
    center_piece.ground_level_delta = center_piece.element.ground_level_delta();

    // Horizontal center with truncating division toward zero (§3.6 step 8 & edge case #5).
    let center_x = (center_piece.bounding_box.max_x + center_piece.bounding_box.min_x) / 2;
    let center_z = (center_piece.bounding_box.max_z + center_piece.bounding_box.min_z) / 2;

    let min_y = ctx.min_y();
    let max_y = ctx.max_y();

    let bottom_y = if let Some(hm) = cfg.project_start_to_heightmap {
        if !ctx.could_structure_exist_in_column(center_x, center_z, min_y, max_y) {
            return None;
        }
        position.y + ctx.first_free_height(center_x, center_z, hm)
    } else {
        adjusted_position.y
    };

    let old_abs_ground_y = center_piece.bounding_box.min_y + center_piece.ground_level_delta;
    center_piece.move_by(0, bottom_y - old_abs_ground_y, 0);

    // Dimension padding check (§3.6 step 11).
    if !cfg.dimension_padding.is_zero()
        && (center_piece.bounding_box.min_y < min_y + cfg.dimension_padding.bottom
            || center_piece.bounding_box.max_y > max_y - cfg.dimension_padding.top)
    {
        return None;
    }

    let center_y = bottom_y + local_anchor.y;

    // Biome validation at `(center_x, center_y, center_z)` BEFORE Phase B!
    if !ctx.is_valid_biome(BlockPos::new(center_x, center_y, center_z)) {
        return None;
    }

    // Phase B — Priority-BFS child expansion.
    if cfg.size <= 0 {
        return None;
    }

    let h_dist = cfg.max_distance_from_center.horizontal;
    let v_dist = cfg.max_distance_from_center.vertical;
    let outer_aabb = Aabb::new(
        [
            f64::from(center_x - h_dist),
            f64::from((center_y - v_dist).max(min_y + cfg.dimension_padding.bottom)),
            f64::from(center_z - h_dist),
        ],
        [
            f64::from(center_x + h_dist + 1),
            f64::from((center_y + v_dist + 1).min(max_y + 1 - cfg.dimension_padding.top)),
            f64::from(center_z + h_dist + 1),
        ],
    );

    let mut initial_free = FreeSpace::new(outer_aabb);
    initial_free.subtract(center_piece.bounding_box.to_aabb());
    let root_free = Rc::new(RefCell::new(initial_free));

    let mut pieces = vec![center_piece];
    let mut placing: PriorityQueue<PieceState> = PriorityQueue::new();
    let mut max_size_cache: HashMap<Identifier, i32> = HashMap::new();

    try_placing_children(
        0,
        Rc::clone(&root_free),
        0,
        cfg,
        &pool_alias_lookup,
        &mut pieces,
        &mut placing,
        &mut max_size_cache,
        rng,
        ctx,
    );

    while let Some(state) = placing.pop_highest() {
        try_placing_children(
            state.piece_idx,
            state.free,
            state.depth,
            cfg,
            &pool_alias_lookup,
            &mut pieces,
            &mut placing,
            &mut max_size_cache,
            rng,
            ctx,
        );
    }

    Some(pieces)
}

#[allow(clippy::too_many_arguments)]
fn try_placing_children(
    source_idx: usize,
    context_free: Rc<RefCell<FreeSpace>>,
    depth: i32,
    cfg: &JigsawConfig,
    pool_alias_lookup: &PoolAliasLookup,
    pieces: &mut Vec<PoolElementPiece>,
    placing: &mut PriorityQueue<PieceState>,
    max_size_cache: &mut HashMap<Identifier, i32>,
    rng: &mut LegacyRandom,
    ctx: &dyn JigsawGenContext,
) {
    let data = ctx.structure_data();
    let tm = ctx.template_manager();

    let source_element = Arc::clone(&pieces[source_idx].element);
    let source_box_pos = pieces[source_idx].position;
    let source_rotation = pieces[source_idx].rotation;
    let source_projection = source_element.projection();
    let source_rigid = source_projection == Projection::Rigid;
    let source_bb = pieces[source_idx].bounding_box;
    let source_box_y = source_bb.min_y;
    let source_ground_level_delta = pieces[source_idx].ground_level_delta;

    let mut source_free: Option<Rc<RefCell<FreeSpace>>> = None;

    let Some(source_jigsaws) =
        get_shuffled_jigsaw_blocks(&source_element, tm, source_box_pos, source_rotation, rng)
    else {
        return;
    };

    for source_jigsaw in source_jigsaws {
        let source_dir = source_jigsaw.orientation.front();
        let source_jigsaw_pos = source_jigsaw.pos;
        let target_jigsaw_pos = source_jigsaw_pos.relative(source_dir, 1);
        let source_jigsaw_local_y = source_jigsaw_pos.y - source_box_y;
        let mut source_jigsaw_base_height = i32::MIN;

        let pool_key = pool_alias_lookup.lookup(&source_jigsaw.pool);
        let Some(target_pool) = data.template_pool(pool_key) else {
            continue;
        };
        if target_pool.size() == 0 && !is_minecraft_empty(pool_key) {
            continue;
        }
        let Some((fallback_id, fallback_pool)) = resolve_pool_ref(&target_pool.fallback, data)
        else {
            continue;
        };
        if fallback_pool.size() == 0 && !fallback_id.is_some_and(is_minecraft_empty) {
            continue;
        }

        // Free-space scope: inside `source_bb` uses `source_free`, else `context_free`.
        let children_free =
            if source_bb.is_inside(target_jigsaw_pos) {
                Rc::clone(source_free.get_or_insert_with(|| {
                    Rc::new(RefCell::new(FreeSpace::new(source_bb.to_aabb())))
                }))
            } else {
                Rc::clone(&context_free)
            };

        // Candidate template list (`targetPieces`) — exact RNG consumption.
        let mut target_pieces: Vec<Arc<StructurePoolElement>> = Vec::new();
        if depth != cfg.size {
            target_pieces.extend(get_shuffled_templates(target_pool, rng));
        }
        target_pieces.extend(get_shuffled_templates(fallback_pool, rng));

        let placement_priority = source_jigsaw.placement_priority;

        'target_elem: for target_element in target_pieces {
            if target_element.is_empty() {
                break;
            }
            let rotations = Rotation::shuffled(rng);
            for target_rotation in rotations {
                let Some(target_jigsaws) = get_shuffled_jigsaw_blocks(
                    &target_element,
                    tm,
                    BlockPos::ZERO,
                    target_rotation,
                    rng,
                ) else {
                    continue;
                };

                // Expansion hack (§3.6 step 8.2).
                let Some(hack_box) =
                    element_bounding_box(&target_element, tm, BlockPos::ZERO, target_rotation)
                else {
                    continue;
                };
                let expand_to = if cfg.use_expansion_hack && hack_box.y_span() <= 16 {
                    let mut max_exp = 0_i32;
                    for tj in &target_jigsaws {
                        let next_pos = tj.pos.relative(tj.orientation.front(), 1);
                        if hack_box.is_inside(next_pos) {
                            let child_pool_key = pool_alias_lookup.lookup(&tj.pool);
                            let h = data.template_pool(child_pool_key).map_or(0, |cp| {
                                let h1 =
                                    pool_max_size(Some(child_pool_key), cp, tm, max_size_cache);
                                let h2 = resolve_pool_ref(&cp.fallback, data)
                                    .map_or(0, |(fid, fp)| {
                                        pool_max_size(fid, fp, tm, max_size_cache)
                                    });
                                h1.max(h2)
                            });
                            if h > max_exp {
                                max_exp = h;
                            }
                        }
                    }
                    max_exp
                } else {
                    0
                };

                for target_jigsaw in &target_jigsaws {
                    if !source_jigsaw.can_attach(target_jigsaw) {
                        continue;
                    }
                    let raw_target_box_pos = target_jigsaw_pos.sub(target_jigsaw.pos);
                    let Some(raw_target_bb) = element_bounding_box(
                        &target_element,
                        tm,
                        raw_target_box_pos,
                        target_rotation,
                    ) else {
                        continue;
                    };
                    let raw_target_y = raw_target_bb.min_y;
                    let target_projection = target_element.projection();
                    let target_rigid = target_projection == Projection::Rigid;
                    let target_jigsaw_local_y = target_jigsaw.pos.y;
                    let (_, step_y, _) = crate::geom::dir_step(source_dir);
                    let delta_y = source_jigsaw_local_y - target_jigsaw_local_y + step_y;

                    let target_box_y = if source_rigid && target_rigid {
                        source_box_y + delta_y
                    } else {
                        if source_jigsaw_base_height == i32::MIN {
                            source_jigsaw_base_height = ctx.first_free_height(
                                source_jigsaw_pos.x,
                                source_jigsaw_pos.z,
                                HeightmapKind::WorldSurfaceWg,
                            );
                        }
                        source_jigsaw_base_height - target_jigsaw_local_y
                    };

                    let y_offset = target_box_y - raw_target_y;
                    let mut target_bb = raw_target_bb.moved(0, y_offset, 0);
                    let target_box_pos = raw_target_box_pos.offset(0, y_offset, 0);

                    if expand_to > 0 {
                        let new_size = (expand_to + 1).max(target_bb.max_y - target_bb.min_y);
                        target_bb.encapsulate_pos(BlockPos::new(
                            target_bb.min_x,
                            target_bb.min_y + new_size,
                            target_bb.min_z,
                        ));
                    }

                    // Collision test with 0.25 deflation margin.
                    let cand_aabb = target_bb.to_aabb();
                    let cand_deflated = Aabb::new(
                        [
                            cand_aabb.min[0] + 0.25,
                            cand_aabb.min[1] + 0.25,
                            cand_aabb.min[2] + 0.25,
                        ],
                        [
                            cand_aabb.max[0] - 0.25,
                            cand_aabb.max[1] - 0.25,
                            cand_aabb.max[2] - 0.25,
                        ],
                    );
                    if !children_free.borrow().fits_deflated(&cand_deflated) {
                        continue;
                    }

                    // Accept piece!
                    children_free.borrow_mut().subtract(cand_aabb);

                    let target_ground_level_delta = if target_rigid {
                        source_ground_level_delta - delta_y
                    } else {
                        target_element.ground_level_delta()
                    };

                    let junction_y = if source_rigid {
                        source_box_y + source_jigsaw_local_y
                    } else if target_rigid {
                        target_box_y + target_jigsaw_local_y
                    } else {
                        if source_jigsaw_base_height == i32::MIN {
                            source_jigsaw_base_height = ctx.first_free_height(
                                source_jigsaw_pos.x,
                                source_jigsaw_pos.z,
                                HeightmapKind::WorldSurfaceWg,
                            );
                        }
                        source_jigsaw_base_height + delta_y / 2
                    };

                    pieces[source_idx].junctions.push(StructureJigsawJunction {
                        source_x: target_jigsaw_pos.x,
                        source_ground_y: junction_y - source_jigsaw_local_y
                            + source_ground_level_delta,
                        source_z: target_jigsaw_pos.z,
                        delta_y,
                        dest_projection: target_projection,
                    });

                    let mut target_piece = PoolElementPiece {
                        element: Arc::clone(&target_element),
                        position: target_box_pos,
                        ground_level_delta: target_ground_level_delta,
                        rotation: target_rotation,
                        bounding_box: target_bb,
                        liquid_settings: cfg.liquid_settings,
                        junctions: Vec::new(),
                    };
                    target_piece.junctions.push(StructureJigsawJunction {
                        source_x: source_jigsaw_pos.x,
                        source_ground_y: junction_y - target_jigsaw_local_y
                            + target_ground_level_delta,
                        source_z: source_jigsaw_pos.z,
                        delta_y: -delta_y,
                        dest_projection: source_projection,
                    });

                    let target_idx = pieces.len();
                    pieces.push(target_piece);

                    if depth < cfg.size {
                        placing.add(
                            PieceState {
                                piece_idx: target_idx,
                                free: Rc::clone(&children_free),
                                depth: depth + 1,
                            },
                            placement_priority,
                        );
                    }
                    break 'target_elem;
                }
            }
        }
    }
}

fn is_minecraft_empty(id: &Identifier) -> bool {
    id.namespace() == "minecraft" && id.path() == "empty"
}

fn resolve_pool_ref<'a>(
    r: &'a TemplatePoolRef,
    data: &'a StructureData,
) -> Option<(Option<&'a Identifier>, &'a StructureTemplatePool)> {
    match r {
        TemplatePoolRef::Reference(id) => data.template_pool(id).map(|p| (Some(id), p.as_ref())),
        TemplatePoolRef::Inline(p) => Some((None, p.as_ref())),
    }
}

fn get_random_template(
    pool: &StructureTemplatePool,
    rng: &mut LegacyRandom,
) -> Arc<StructurePoolElement> {
    if pool.templates.is_empty() {
        Arc::new(StructurePoolElement::Empty)
    } else {
        let idx = rng.next_int_bounded(pool.templates.len() as i32) as usize;
        Arc::clone(&pool.templates[idx])
    }
}

fn get_shuffled_templates(
    pool: &StructureTemplatePool,
    rng: &mut LegacyRandom,
) -> Vec<Arc<StructurePoolElement>> {
    let mut copy = pool.templates.clone();
    shuffle_slice(&mut copy, rng);
    copy
}

fn pool_max_size(
    id: Option<&Identifier>,
    pool: &StructureTemplatePool,
    tm: &TemplateManager,
    cache: &mut HashMap<Identifier, i32>,
) -> i32 {
    if let Some(id) = id
        && let Some(&cached) = cache.get(id)
    {
        return cached;
    }
    let mut max_h = 0_i32;
    for elem in &pool.templates {
        if elem.is_empty() {
            continue;
        }
        if let Some(bb) = element_bounding_box(elem, tm, BlockPos::ZERO, Rotation::None) {
            max_h = max_h.max(bb.y_span());
        }
    }
    if let Some(id) = id {
        cache.insert(id.clone(), max_h);
    }
    max_h
}

/// Computes `element.getBoundingBox(templateManager, position, rotation)` (§3.5).
pub fn element_bounding_box(
    element: &StructurePoolElement,
    tm: &TemplateManager,
    position: BlockPos,
    rotation: Rotation,
) -> Option<BoundingBox> {
    match element {
        StructurePoolElement::Single { location, .. }
        | StructurePoolElement::LegacySingle { location, .. } => {
            let tpl = tm.get(location).ok()?;
            Some(tpl.bounding_box(position, rotation, BlockPos::ZERO, Mirror::None))
        }
        StructurePoolElement::Feature { .. } => {
            Some(BoundingBox::point(position.x, position.y, position.z))
        }
        StructurePoolElement::List { elements, .. } => {
            let mut acc: Option<BoundingBox> = None;
            for child in elements {
                if child.is_empty() {
                    continue;
                }
                let bb = element_bounding_box(child, tm, position, rotation)?;
                match &mut acc {
                    Some(a) => a.encapsulate(bb),
                    None => acc = Some(bb),
                }
            }
            acc
        }
        StructurePoolElement::Empty => None,
    }
}

/// Computes `element.getShuffledJigsawBlocks(templateManager, position, rotation, random)` (§3.5).
pub fn get_shuffled_jigsaw_blocks(
    element: &StructurePoolElement,
    tm: &TemplateManager,
    position: BlockPos,
    rotation: Rotation,
    rng: &mut LegacyRandom,
) -> Option<Vec<JigsawBlockInfo>> {
    match element {
        StructurePoolElement::Single { location, .. }
        | StructurePoolElement::LegacySingle { location, .. } => {
            let tpl = tm.get(location).ok()?;
            let mut jigsaws = tpl.get_jigsaws(position, rotation);
            shuffle_slice(&mut jigsaws, rng);
            // Stable sort by `selection_priority` descending (§3.5).
            jigsaws.sort_by_key(|a| std::cmp::Reverse(a.selection_priority));
            Some(jigsaws)
        }
        StructurePoolElement::Feature { .. } => {
            let empty_id = Identifier::with_default_namespace("empty").expect("valid");
            Some(vec![JigsawBlockInfo {
                pos: position,
                orientation: FrontAndTop::DownSouth,
                name: None,
                target: empty_id.clone(),
                pool: empty_id,
                joint: JointType::Rollable,
                placement_priority: 0,
                selection_priority: 0,
                final_state: "minecraft:air".to_owned(),
            }])
        }
        StructurePoolElement::List { elements, .. } => {
            // Delegates ONLY to `elements[0]` (§3.5 & edge case #9).
            let first = elements.first()?;
            get_shuffled_jigsaw_blocks(first, tm, position, rotation, rng)
        }
        StructurePoolElement::Empty => Some(Vec::new()),
    }
}
