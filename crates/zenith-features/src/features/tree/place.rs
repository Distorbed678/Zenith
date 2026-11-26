//! Core `TreeFeature::place` algorithm (`features-trees.md` §2).

use zenith_blocks::FluidKind;
use zenith_math::RandomSource;

use super::config::{TreeConfig, TrunkKind};
use super::{TREE_UPDATE_FLAGS, TreeCache, decorator, leaves};
use crate::block_ops::BlockOps;
use crate::features::{FeatureContext, TreeFeatureHook};
use crate::java_hash::JavaBlockPosSet;
use crate::region::GenerationRegion;
use crate::rng::{BlockPos, WorldgenRandom};

/// One trunk-emitted foliage attachment point.
#[derive(Debug, Clone, Copy)]
pub(super) struct Attachment {
    pub pos: BlockPos,
    pub radius_offset: i32,
    pub foliage_height_offset: i32,
    pub double_trunk: bool,
}

impl Attachment {
    #[inline]
    pub(super) const fn new(pos: BlockPos, radius_offset: i32, double_trunk: bool) -> Self {
        Self {
            pos,
            radius_offset,
            foliage_height_offset: 0,
            double_trunk,
        }
    }
}

/// Inclusive axis-aligned bounding box.
#[derive(Debug, Clone, Copy)]
pub(super) struct Bounds {
    pub min: [i32; 3],
    pub max: [i32; 3],
}

impl Bounds {
    #[inline]
    pub(super) const fn contains(&self, p: BlockPos) -> bool {
        p.x >= self.min[0]
            && p.x <= self.max[0]
            && p.y >= self.min[1]
            && p.y <= self.max[1]
            && p.z >= self.min[2]
            && p.z <= self.max[2]
    }

    #[inline]
    pub(super) const fn span(&self) -> [i32; 3] {
        [
            self.max[0] - self.min[0] + 1,
            self.max[1] - self.min[1] + 1,
            self.max[2] - self.min[2] + 1,
        ]
    }
}

/// Default tree implementation of [`TreeFeatureHook`].
#[derive(Debug, Default)]
pub struct VanillaTreeHook;

impl TreeFeatureHook for VanillaTreeHook {
    fn place_tree(
        &self,
        ctx: &FeatureContext<'_>,
        config: &TreeConfig,
        region: &mut GenerationRegion,
        rng: &mut WorldgenRandom,
        origin: BlockPos,
    ) -> bool {
        place_tree(ctx, config, region, rng, origin)
    }

    fn run_tree_decorators(
        &self,
        ctx: &FeatureContext<'_>,
        decorators: &[super::TreeDecorator],
        region: &mut GenerationRegion,
        rng: &mut WorldgenRandom,
        log_positions: &[BlockPos],
    ) {
        let mut logs: Vec<BlockPos> = log_positions.to_vec();
        logs.sort_by_key(|p| p.y);
        let mut decorations = JavaBlockPosSet::new();
        decorator::run_decorators(
            ctx,
            decorators,
            region,
            rng,
            &logs,
            &[],
            &[],
            &mut decorations,
        );
    }
}

/// `TreeFeature::place`: places `config` at `origin`. This is the shared entry point used by the
/// `minecraft:tree` feature and by sapling growth (`TreeGrower`).
pub fn place_tree(
    ctx: &FeatureContext<'_>,
    config: &TreeConfig,
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
) -> bool {
    let ops = std::sync::Arc::clone(region.ops_arc());
    let mut s = TreeSession {
        region,
        rng,
        cfg: config,
        ops: &ops,
        cache: ops.tree_cache(),
        roots: JavaBlockPosSet::new(),
        trunks: JavaBlockPosSet::new(),
        foliage: JavaBlockPosSet::new(),
        decorations: JavaBlockPosSet::new(),
    };
    s.place(ctx, origin)
}

/// Mutable state of one `TreeFeature::place` invocation.
pub(super) struct TreeSession<'a> {
    pub region: &'a mut GenerationRegion,
    pub rng: &'a mut WorldgenRandom,
    pub cfg: &'a TreeConfig,
    pub ops: &'a BlockOps,
    pub cache: &'a TreeCache,
    pub roots: JavaBlockPosSet,
    pub trunks: JavaBlockPosSet,
    pub foliage: JavaBlockPosSet,
    pub decorations: JavaBlockPosSet,
}

impl TreeSession<'_> {
    // ---- setters -------------------------------------------------------------------------

    #[inline]
    pub(super) fn set_root(&mut self, pos: BlockPos, state: u32) {
        self.roots.insert(pos);
        self.region.set_block(pos, state, TREE_UPDATE_FLAGS);
    }

    #[inline]
    pub(super) fn set_trunk(&mut self, pos: BlockPos, state: u32) {
        self.trunks.insert(pos);
        self.region.set_block(pos, state, TREE_UPDATE_FLAGS);
    }

    #[inline]
    pub(super) fn set_foliage(&mut self, pos: BlockPos, state: u32) {
        self.foliage.insert(pos);
        self.region.set_block(pos, state, TREE_UPDATE_FLAGS);
    }

    // ---- predicates ----------------------------------------------------------------------

    /// `TreeFeature.validTreePos`.
    #[inline]
    pub(super) fn valid_tree_pos(&self, pos: BlockPos) -> bool {
        self.ops.valid_tree_pos(self.region.get_block(pos))
    }

    /// `TreeFeature.isAirOrLeaves`.
    #[inline]
    pub(super) fn is_air_or_leaves(&self, pos: BlockPos) -> bool {
        let s = self.region.get_block(pos);
        self.ops.is_air(s) || self.ops.tag_leaves.contains(self.ops.block_id(s))
    }

    /// `TrunkPlacer.validTreePos`.
    #[inline]
    pub(super) fn trunk_valid_tree_pos(&self, pos: BlockPos) -> bool {
        let s = self.region.get_block(pos);
        if self.ops.valid_tree_pos(s) {
            return true;
        }
        if let TrunkKind::UpwardsBranching {
            can_grow_through, ..
        } = &self.cfg.trunk_placer.kind
        {
            return can_grow_through.contains(self.ops.block_id(s));
        }
        false
    }

    /// `TrunkPlacer.isFree`.
    #[inline]
    pub(super) fn is_free(&self, pos: BlockPos) -> bool {
        if self.trunk_valid_tree_pos(pos) {
            return true;
        }
        let s = self.region.get_block(pos);
        self.cache.tag_logs.contains(self.ops.block_id(s))
    }

    /// `TreeFeature.isVine`.
    #[inline]
    fn is_vine(&self, pos: BlockPos) -> bool {
        self.ops.block_id(self.region.get_block(pos)) == self.ops.bid_vine
    }

    // ---- main sequence ---------------------------------------------------------------------

    fn place(&mut self, ctx: &FeatureContext<'_>, origin: BlockPos) -> bool {
        let cfg = self.cfg;

        // 2. dimensions
        let tp = &cfg.trunk_placer;
        let tree_height = tp.base_height
            + self.rng.next_int_bounded(tp.height_rand_a + 1)
            + self.rng.next_int_bounded(tp.height_rand_b + 1);
        let foliage_height = self.foliage_height(tree_height);
        let trunk_height = tree_height - foliage_height;
        let leaf_radius = self.foliage_radius(trunk_height);
        let trunk_origin = match &cfg.root_placer {
            Some(rp) => origin.above_n(rp.trunk_offset_y.sample(&mut *self.rng)),
            None => origin,
        };

        // 3. build height
        let min_y = origin.y.min(trunk_origin.y);
        let max_y = origin.y.max(trunk_origin.y) + tree_height + 1;
        if min_y < self.region.min_y() + 1 || max_y > self.region.max_y() + 1 {
            return false;
        }

        // 4. free-space scan
        let clipped_tree_height = self.max_free_tree_height(tree_height, trunk_origin);

        // 5. acceptance
        let ok = clipped_tree_height >= tree_height
            || cfg
                .minimum_size
                .min_clipped_height
                .is_some_and(|m| clipped_tree_height >= m);
        if !ok {
            return false;
        }

        // 6. roots
        if cfg.root_placer.is_some() && !self.place_roots(origin, trunk_origin) {
            return false;
        }

        // 7. trunk
        let attachments = self.place_trunk(clipped_tree_height, trunk_origin);

        // 8. foliage
        for att in &attachments {
            self.create_foliage(clipped_tree_height, *att, foliage_height, leaf_radius);
        }

        // 9. guard
        if self.trunks.is_empty() && self.foliage.is_empty() {
            return false;
        }

        // 10. decorators
        if !cfg.decorators.is_empty() {
            let mut roots_l: Vec<BlockPos> = self.roots.iter().collect();
            let mut logs_l: Vec<BlockPos> = self.trunks.iter().collect();
            let mut leaves_l: Vec<BlockPos> = self.foliage.iter().collect();
            roots_l.sort_by_key(|p| p.y);
            logs_l.sort_by_key(|p| p.y);
            leaves_l.sort_by_key(|p| p.y);
            decorator::run_decorators(
                ctx,
                &cfg.decorators,
                &mut *self.region,
                &mut *self.rng,
                &logs_l,
                &leaves_l,
                &roots_l,
                &mut self.decorations,
            );
        }

        // 11. bounds + leaf distances + edge updates
        let mut min = [i32::MAX; 3];
        let mut max = [i32::MIN; 3];
        let mut any = false;
        for set in [&self.roots, &self.trunks, &self.foliage, &self.decorations] {
            for p in set.iter() {
                any = true;
                min[0] = min[0].min(p.x);
                min[1] = min[1].min(p.y);
                min[2] = min[2].min(p.z);
                max[0] = max[0].max(p.x);
                max[1] = max[1].max(p.y);
                max[2] = max[2].max(p.z);
            }
        }
        if !any {
            return false;
        }
        let bounds = Bounds { min, max };
        let shape = leaves::update_leaves(
            self.region,
            self.ops,
            self.cache,
            &bounds,
            &self.trunks,
            &self.decorations,
            &self.roots,
        );
        leaves::update_shape_at_edge(self.region, self.ops, self.cache, &shape, &bounds);
        true
    }

    /// `getMaxFreeTreeHeight`.
    fn max_free_tree_height(&self, tree_height: i32, trunk_origin: BlockPos) -> i32 {
        let ignore_vines = self.cfg.ignore_vines;
        for y in 0..=(tree_height + 1) {
            let r = self.cfg.minimum_size.size_at_height(tree_height, y);
            for x in -r..=r {
                for z in -r..=r {
                    let p = trunk_origin.offset(x, y, z);
                    if !self.is_free(p) || (!ignore_vines && self.is_vine(p)) {
                        return y - 2;
                    }
                }
            }
        }
        tree_height
    }

    // ---- shared placement helpers --------------------------------------------------------

    /// `place_below_trunk_block`.
    pub(super) fn place_below_trunk_block(&mut self, pos: BlockPos) {
        let cfg = self.cfg;
        if let Some(s) =
            cfg.below_trunk_provider
                .get_optional_state(&*self.region, &mut *self.rng, pos)
        {
            self.set_trunk(pos, s);
        }
    }

    /// `TrunkPlacer.placeLog(.., modifier)`; `axis` applies `trySetValue(axis, ..)`.
    pub(super) fn place_log(&mut self, pos: BlockPos, axis: Option<&str>) -> bool {
        if !self.trunk_valid_tree_pos(pos) {
            return false;
        }
        let cfg = self.cfg;
        let raw = cfg
            .trunk_provider
            .get_state(&*self.region, &mut *self.rng, pos);
        let state = match axis {
            Some(a) => self.ops.try_set_property(raw, "axis", a),
            None => raw,
        };
        self.set_trunk(pos, state);
        true
    }

    /// `TrunkPlacer.placeLogIfFree`.
    pub(super) fn place_log_if_free(&mut self, pos: BlockPos) {
        if self.is_free(pos) {
            self.place_log(pos, None);
        }
    }

    /// `FoliagePlacer.tryPlaceLeaf`.
    pub(super) fn try_place_leaf(&mut self, pos: BlockPos) -> bool {
        let cur = self.region.get_block(pos);
        if self.ops.get_property(cur, "persistent") == Some("true") {
            return false;
        }
        if !self.ops.valid_tree_pos(cur) {
            return false;
        }
        let cfg = self.cfg;
        let mut leaf = cfg
            .foliage_provider
            .get_state(&*self.region, &mut *self.rng, pos);
        if self.ops.has_property(leaf, "waterlogged") {
            let f = self.ops.fluid(cur);
            let source_water = f.kind == FluidKind::Water && f.is_source;
            leaf = self.ops.copy_waterlogged(leaf, source_water);
        }
        self.set_foliage(pos, leaf);
        true
    }
}
