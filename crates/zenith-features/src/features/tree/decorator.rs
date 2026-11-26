//! Tree decorators (`features-trees.md` §7).

use std::collections::HashSet;

use zenith_blocks::Direction;
use zenith_math::RandomSource;
use zenith_worldgen::HeightmapKind;

use super::config::TreeDecorator;
use super::{TREE_UPDATE_FLAGS, TreeCache};
use crate::block_ops::BlockOps;
use crate::features::FeatureContext;
use crate::java_hash::JavaBlockPosSet;
use crate::region::GenerationRegion;
use crate::rng::{
    BlockPos, WorldgenRandom, direction_horizontal_random, random_between_inclusive, util_shuffle,
};

/// Runs `decorators` in order against the `TreeDecorator.Context` built from `logs` / `leaves` /
/// `roots` (already sorted by ascending `y`). Positions written are recorded in `decorations`.
#[allow(clippy::too_many_arguments)]
pub(super) fn run_decorators(
    feat: &FeatureContext<'_>,
    decorators: &[TreeDecorator],
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    logs: &[BlockPos],
    leaves: &[BlockPos],
    roots: &[BlockPos],
    decorations: &mut JavaBlockPosSet,
) {
    let ops_arc = std::sync::Arc::clone(region.ops_arc());
    let ops: &BlockOps = &ops_arc;
    let mut c = DecoratorContext {
        feat,
        region,
        rng,
        ops,
        cache: ops.tree_cache(),
        logs,
        leaves,
        roots,
        decorations,
    };
    for d in decorators {
        c.place(d);
    }
}

struct DecoratorContext<'a, 'f> {
    feat: &'a FeatureContext<'f>,
    region: &'a mut GenerationRegion,
    rng: &'a mut WorldgenRandom,
    ops: &'a BlockOps,
    cache: &'a TreeCache,
    logs: &'a [BlockPos],
    leaves: &'a [BlockPos],
    roots: &'a [BlockPos],
    decorations: &'a mut JavaBlockPosSet,
}

impl DecoratorContext<'_, '_> {
    #[inline]
    fn is_air(&self, pos: BlockPos) -> bool {
        self.ops.is_air(self.region.get_block(pos))
    }

    #[inline]
    fn is_replaceable(&self, pos: BlockPos) -> bool {
        self.ops.is_replaceable(self.region.get_block(pos))
    }

    fn is_water_or_water_nearby(&self, pos: BlockPos) -> bool {
        let is_water = |p: BlockPos| self.ops.is_water(self.region.get_block(p));
        is_water(pos)
            || is_water(pos.relative(Direction::East))
            || is_water(pos.relative(Direction::West))
            || is_water(pos.relative(Direction::North))
            || is_water(pos.relative(Direction::South))
    }

    #[inline]
    fn set_block(&mut self, pos: BlockPos, state: u32) {
        self.decorations.insert(pos);
        self.region.set_block(pos, state, TREE_UPDATE_FLAGS);
    }

    fn place_vine(&mut self, pos: BlockPos, prop: &str) {
        let s = self.ops.try_set_property(self.cache.sid_vine, prop, "true");
        self.set_block(pos, s);
    }

    #[inline]
    fn is_log(&self, pos: BlockPos) -> bool {
        self.cache
            .tag_logs
            .contains(self.ops.block_id(self.region.get_block(pos)))
    }

    /// `TreeFeature.getLowestTrunkOrRootOfTree`.
    fn lowest_trunk_or_root(&self) -> Vec<BlockPos> {
        if self.roots.is_empty() {
            self.logs.to_vec()
        } else if !self.logs.is_empty() && self.roots[0].y == self.logs[0].y {
            let mut v = self.logs.to_vec();
            v.extend_from_slice(self.roots);
            v
        } else {
            self.roots.to_vec()
        }
    }

    fn place(&mut self, d: &TreeDecorator) {
        match d {
            TreeDecorator::TrunkVine => self.trunk_vine(),
            TreeDecorator::LeaveVine { probability } => self.leave_vine(*probability),
            TreeDecorator::PaleMoss {
                leaves_probability,
                trunk_probability,
                ground_probability,
                patch_feature,
            } => self.pale_moss(
                *leaves_probability,
                *trunk_probability,
                *ground_probability,
                patch_feature.as_deref(),
            ),
            TreeDecorator::CreakingHeart { probability } => self.creaking_heart(*probability),
            TreeDecorator::Cocoa { probability } => self.cocoa(*probability),
            TreeDecorator::ShelfMushroom { probability } => self.shelf_mushroom(*probability),
            TreeDecorator::Beehive { probability } => self.beehive(*probability),
            TreeDecorator::AlterGround { provider } => self.alter_ground(provider),
            TreeDecorator::AttachedToLeaves {
                probability,
                exclusion_radius_xz,
                exclusion_radius_y,
                block_provider,
                required_empty_blocks,
                directions,
            } => self.attached_to_leaves(
                *probability,
                *exclusion_radius_xz,
                *exclusion_radius_y,
                block_provider,
                *required_empty_blocks,
                directions,
            ),
            TreeDecorator::PlaceOnGround {
                tries,
                radius,
                height,
                block_state_provider,
            } => self.place_on_ground(*tries, *radius, *height, block_state_provider),
            TreeDecorator::AttachedToLogs {
                probability,
                block_provider,
                directions,
            } => self.attached_to_logs(*probability, block_provider, directions),
        }
    }

    // ---- 7.1 trunk_vine ----------------------------------------------------------------------

    fn trunk_vine(&mut self) {
        let logs = self.logs;
        for &pos in logs {
            if self.rng.next_int_bounded(3) > 0 && self.is_air(pos.relative(Direction::West)) {
                self.place_vine(pos.relative(Direction::West), "east");
            }
            if self.rng.next_int_bounded(3) > 0 && self.is_air(pos.relative(Direction::East)) {
                self.place_vine(pos.relative(Direction::East), "west");
            }
            if self.rng.next_int_bounded(3) > 0 && self.is_air(pos.relative(Direction::North)) {
                self.place_vine(pos.relative(Direction::North), "south");
            }
            if self.rng.next_int_bounded(3) > 0 && self.is_air(pos.relative(Direction::South)) {
                self.place_vine(pos.relative(Direction::South), "north");
            }
        }
    }

    // ---- 7.2 leave_vine ----------------------------------------------------------------------

    fn add_hanging_vine(&mut self, pos: BlockPos, prop: &str) {
        self.place_vine(pos, prop);
        let mut below = pos.below();
        let mut i = 4;
        while self.is_air(below) && i > 0 {
            self.place_vine(below, prop);
            below = below.below();
            i -= 1;
        }
    }

    fn leave_vine(&mut self, probability: f32) {
        let leaves = self.leaves;
        for &pos in leaves {
            for (dir, prop) in [
                (Direction::West, "east"),
                (Direction::East, "west"),
                (Direction::North, "south"),
                (Direction::South, "north"),
            ] {
                let p = pos.relative(dir);
                if self.rng.next_float() < probability && self.is_air(p) {
                    self.add_hanging_vine(p, prop);
                }
            }
        }
    }

    // ---- 7.3 pale_moss -----------------------------------------------------------------------

    fn add_moss_hanger(&mut self, mut pos: BlockPos) {
        let stem = self
            .ops
            .try_set_property(self.cache.sid_pale_hanging_moss, "tip", "false");
        let tip = self
            .ops
            .try_set_property(self.cache.sid_pale_hanging_moss, "tip", "true");
        while self.is_air(pos.below()) && self.rng.next_float() >= 0.5_f32 {
            self.set_block(pos, stem);
            pos = pos.below();
        }
        self.set_block(pos, tip);
    }

    fn pale_moss(
        &mut self,
        leaves_probability: f32,
        trunk_probability: f32,
        ground_probability: f32,
        patch: Option<&crate::features::Feature>,
    ) {
        let mut shuffled: Vec<BlockPos> = self.logs.to_vec();
        util_shuffle(&mut shuffled, &mut *self.rng);
        if shuffled.is_empty() {
            return;
        }
        let mut origin = shuffled[0];
        for p in &shuffled {
            if p.y < origin.y {
                origin = *p;
            }
        }
        if self.rng.next_float() < ground_probability
            && let Some(f) = patch
        {
            f.place(self.feat, self.region, self.rng, origin.above());
        }
        let logs = self.logs;
        for &pos in logs {
            if self.rng.next_float() < trunk_probability && self.is_air(pos.below()) {
                self.add_moss_hanger(pos.below());
            }
        }
        let leaves = self.leaves;
        for &pos in leaves {
            if self.rng.next_float() < leaves_probability && self.is_air(pos.below()) {
                self.add_moss_hanger(pos.below());
            }
        }
    }

    // ---- 7.4 creaking_heart --------------------------------------------------------------------

    fn creaking_heart(&mut self, probability: f32) {
        if self.logs.is_empty() {
            return;
        }
        if self.rng.next_float() >= probability {
            return;
        }
        let mut list: Vec<BlockPos> = self.logs.to_vec();
        util_shuffle(&mut list, &mut *self.rng);
        let found = list
            .iter()
            .copied()
            .find(|&p| Direction::ALL.iter().all(|&d| self.is_log(p.relative(d))));
        if let Some(pos) = found {
            let ops = self.ops;
            let mut s = ops.try_set_property(self.cache.sid_creaking_heart, "axis", "y");
            s = ops.try_set_property(s, "creaking_heart_state", "dormant");
            s = ops.try_set_property(s, "natural", "true");
            self.set_block(pos, s);
        }
    }

    // ---- 7.5 cocoa -----------------------------------------------------------------------------

    fn cocoa(&mut self, probability: f32) {
        if self.rng.next_float() >= probability {
            return;
        }
        if self.logs.is_empty() {
            return;
        }
        let tree_y = self.logs[0].y;
        let logs = self.logs;
        for &pos in logs {
            if pos.y - tree_y > 2 {
                continue;
            }
            for dir in Direction::HORIZONTAL {
                if self.rng.next_float() <= 0.25_f32 {
                    let cocoa_pos = pos.relative(dir.opposite());
                    if self.is_air(cocoa_pos) {
                        let age = self.rng.next_int_bounded(3);
                        let ops = self.ops;
                        let age_s = ["0", "1", "2"][age as usize];
                        let mut s = ops.try_set_property(self.cache.sid_cocoa, "age", age_s);
                        s = ops.try_set_property(s, "facing", dir.name());
                        self.set_block(cocoa_pos, s);
                    }
                }
            }
        }
    }

    // ---- 7.6 shelf_mushroom --------------------------------------------------------------------

    fn is_shelf_mushroom(&self, pos: BlockPos) -> bool {
        self.ops.block_id(self.region.get_block(pos)) == self.cache.bid_shelf_mushroom
    }

    fn shelf_state(&self, age: i32, facing: Direction) -> u32 {
        let age_s = if age == 0 { "0" } else { "1" };
        let s = self
            .ops
            .try_set_property(self.cache.sid_shelf_mushroom, "age", age_s);
        self.ops.try_set_property(s, "facing", facing.name())
    }

    fn shelf_mushroom(&mut self, probability: f32) {
        if self.rng.next_float() >= probability {
            return;
        }
        if self.logs.is_empty() {
            return;
        }
        let logs = self.logs;
        let first_y = logs[0].y;
        let last_y = logs[logs.len() - 1].y;
        let is_fallen = first_y == last_y;
        if !is_fallen {
            let first = direction_horizontal_random(&mut *self.rng);
            let dirs = [first, first.clockwise()];
            for &log_pos in logs {
                let dy = log_pos.y - first_y;
                if (1..=4).contains(&dy) {
                    for facing in dirs {
                        if self.rng.next_float() <= 0.25_f32 {
                            let mpos = log_pos.relative(facing);
                            if self.is_replaceable(mpos)
                                && !self.is_water_or_water_nearby(mpos)
                                && !self.is_shelf_mushroom(mpos.below())
                            {
                                let age = self.rng.next_int_bounded(2);
                                let s = self.shelf_state(age, facing);
                                self.set_block(mpos, s);
                                break;
                            }
                        }
                    }
                }
            }
        } else {
            let first = logs[0];
            let last = logs[logs.len() - 1];
            let dirs = if first.x != last.x {
                [Direction::North, Direction::South]
            } else {
                [Direction::East, Direction::West]
            };
            for &log_pos in logs {
                for facing in dirs {
                    if self.rng.next_float() <= 0.25_f32 {
                        let mpos = log_pos.relative(facing);
                        if self.is_replaceable(mpos)
                            && !self.is_water_or_water_nearby(mpos)
                            && !Direction::HORIZONTAL
                                .iter()
                                .any(|&d| self.is_shelf_mushroom(mpos.relative(d)))
                            && !Direction::HORIZONTAL
                                .iter()
                                .any(|&d| self.is_shelf_mushroom(log_pos.relative(d)))
                        {
                            let age = self.rng.next_int_bounded(2);
                            let s = self.shelf_state(age, facing);
                            self.set_block(mpos, s);
                        }
                    }
                }
            }
        }
    }

    // ---- 7.7 beehive ---------------------------------------------------------------------------

    fn beehive(&mut self, probability: f32) {
        if self.logs.is_empty() {
            return;
        }
        if self.rng.next_float() >= probability {
            return;
        }
        let logs = self.logs;
        let hive_y = if !self.leaves.is_empty() {
            i32::max(self.leaves[0].y - 1, logs[0].y + 1)
        } else {
            i32::min(
                logs[0].y + 1 + self.rng.next_int_bounded(3),
                logs[logs.len() - 1].y,
            )
        };
        let mut candidates: Vec<BlockPos> = Vec::new();
        for &lp in logs {
            if lp.y == hive_y {
                candidates.push(lp.relative(Direction::East));
                candidates.push(lp.relative(Direction::South));
                candidates.push(lp.relative(Direction::West));
            }
        }
        if candidates.is_empty() {
            return;
        }
        util_shuffle(&mut candidates, &mut *self.rng);
        let found = candidates
            .iter()
            .copied()
            .find(|&p| self.is_air(p) && self.is_air(p.relative(Direction::South)));
        if let Some(pos) = found {
            let ops = self.ops;
            let mut s = ops.try_set_property(self.cache.sid_bee_nest, "facing", "south");
            s = ops.try_set_property(s, "honey_level", "0");
            self.set_block(pos, s);
            if self.region.has_block_entity_at(pos) {
                let num_bees = 2 + self.rng.next_int_bounded(2);
                for _ in 0..num_bees {
                    let _ticks_in_hive = self.rng.next_int_bounded(599);
                }
            }
        }
    }

    // ---- 7.8 alter_ground ----------------------------------------------------------------------

    fn alter_ground(&mut self, provider: &crate::providers::BlockStateProvider) {
        let base_list = self.lowest_trunk_or_root();
        if base_list.is_empty() {
            return;
        }
        let min_y = base_list[0].y;
        for &pos in base_list.iter().filter(|p| p.y == min_y) {
            self.alter_circle(
                provider,
                pos.relative(Direction::West).relative(Direction::North),
            );
            self.alter_circle(
                provider,
                pos.relative_n(Direction::East, 2)
                    .relative(Direction::North),
            );
            self.alter_circle(
                provider,
                pos.relative(Direction::West)
                    .relative_n(Direction::South, 2),
            );
            self.alter_circle(
                provider,
                pos.relative_n(Direction::East, 2)
                    .relative_n(Direction::South, 2),
            );
            for _ in 0..5 {
                let v = self.rng.next_int_bounded(64);
                let xx = v % 8;
                let zz = v / 8;
                if xx == 0 || xx == 7 || zz == 0 || zz == 7 {
                    self.alter_circle(provider, pos.offset(-3 + xx, 0, -3 + zz));
                }
            }
        }
    }

    fn alter_circle(&mut self, provider: &crate::providers::BlockStateProvider, center: BlockPos) {
        for xx in -2..=2i32 {
            for zz in -2..=2i32 {
                if xx.abs() != 2 || zz.abs() != 2 {
                    self.alter_block_at(provider, center.offset(xx, 0, zz));
                }
            }
        }
    }

    fn alter_block_at(
        &mut self,
        provider: &crate::providers::BlockStateProvider,
        target: BlockPos,
    ) {
        let mut dy = 2;
        while dy >= -3 {
            let cursor = target.above_n(dy);
            if let Some(state) = provider.get_optional_state(&*self.region, &mut *self.rng, cursor)
            {
                self.set_block(cursor, state);
                break;
            }
            if !self.is_air(cursor) && dy < 0 {
                break;
            }
            dy -= 1;
        }
    }

    // ---- 7.9 attached_to_leaves ----------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn attached_to_leaves(
        &mut self,
        probability: f32,
        exclusion_radius_xz: i32,
        exclusion_radius_y: i32,
        block_provider: &crate::providers::BlockStateProvider,
        required_empty_blocks: i32,
        directions: &[Direction],
    ) {
        let mut blacklist: HashSet<BlockPos> = HashSet::new();
        let mut shuffled: Vec<BlockPos> = self.leaves.to_vec();
        util_shuffle(&mut shuffled, &mut *self.rng);
        for leaf_pos in shuffled {
            let dir = directions[self.rng.next_int_bounded(directions.len() as i32) as usize];
            let place_pos = leaf_pos.relative(dir);
            if !blacklist.contains(&place_pos)
                && self.rng.next_float() < probability
                && (1..=required_empty_blocks).all(|i| self.is_air(leaf_pos.relative_n(dir, i)))
            {
                for x in -exclusion_radius_xz..=exclusion_radius_xz {
                    for y in -exclusion_radius_y..=exclusion_radius_y {
                        for z in -exclusion_radius_xz..=exclusion_radius_xz {
                            blacklist.insert(place_pos.offset(x, y, z));
                        }
                    }
                }
                let s = block_provider.get_state(&*self.region, &mut *self.rng, place_pos);
                self.set_block(place_pos, s);
            }
        }
    }

    // ---- 7.10 place_on_ground ------------------------------------------------------------------

    fn place_on_ground(
        &mut self,
        tries: i32,
        radius: i32,
        height: i32,
        provider: &crate::providers::BlockStateProvider,
    ) {
        let base_list = self.lowest_trunk_or_root();
        if base_list.is_empty() {
            return;
        }
        let min_y = base_list[0].y;
        let mut min_x = i32::MAX;
        let mut max_x = i32::MIN;
        let mut min_z = i32::MAX;
        let mut max_z = i32::MIN;
        for p in base_list.iter().filter(|p| p.y == min_y) {
            min_x = min_x.min(p.x);
            max_x = max_x.max(p.x);
            min_z = min_z.min(p.z);
            max_z = max_z.max(p.z);
        }
        let (bx0, bx1) = (min_x - radius, max_x + radius);
        let (by0, by1) = (min_y - height, min_y + height);
        let (bz0, bz1) = (min_z - radius, max_z + radius);
        for _ in 0..tries {
            let x = random_between_inclusive(&mut *self.rng, bx0, bx1);
            let y = random_between_inclusive(&mut *self.rng, by0, by1);
            let z = random_between_inclusive(&mut *self.rng, bz0, bz1);
            let pos = BlockPos::new(x, y, z);
            let above = pos.above();
            let above_state = self.region.get_block(above);
            if (self.ops.is_air(above_state) || self.ops.block_id(above_state) == self.ops.bid_vine)
                && self.ops.is_solid_render(self.region.get_block(pos))
                && self
                    .region
                    .get_heightmap_pos(HeightmapKind::MotionBlockingNoLeaves, pos)
                    .y
                    <= above.y
            {
                let s = provider.get_state(&*self.region, &mut *self.rng, above);
                self.set_block(above, s);
            }
        }
    }

    // ---- 7.11 attached_to_logs -----------------------------------------------------------------

    fn attached_to_logs(
        &mut self,
        probability: f32,
        block_provider: &crate::providers::BlockStateProvider,
        directions: &[Direction],
    ) {
        let mut shuffled: Vec<BlockPos> = self.logs.to_vec();
        util_shuffle(&mut shuffled, &mut *self.rng);
        for log_pos in shuffled {
            let dir = directions[self.rng.next_int_bounded(directions.len() as i32) as usize];
            let place_pos = log_pos.relative(dir);
            if self.rng.next_float() <= probability && self.is_air(place_pos) {
                let s = block_provider.get_state(&*self.region, &mut *self.rng, place_pos);
                self.set_block(place_pos, s);
            }
        }
    }
}
