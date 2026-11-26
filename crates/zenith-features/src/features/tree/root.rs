//! Root placers (`features-trees.md` §6).

use zenith_blocks::Direction;
use zenith_math::RandomSource;

use super::config::RootPlacer;
use super::place::TreeSession;
use crate::rng::BlockPos;

impl TreeSession<'_> {
    /// `RootPlacer.getPotentiallyWaterloggedState`: `waterlogged` follows the `#minecraft:water` fluid tag.
    fn potentially_waterlogged(&self, pos: BlockPos, state: u32) -> u32 {
        if self.ops.has_property(state, "waterlogged") {
            let cur = self.region.get_block(pos);
            self.ops
                .copy_waterlogged(state, self.ops.is_water_fluid(cur))
        } else {
            state
        }
    }

    fn can_place_root(&self, rp: &RootPlacer, pos: BlockPos) -> bool {
        let s = self.region.get_block(pos);
        self.ops.valid_tree_pos(s) || rp.mangrove.can_grow_through.contains(self.ops.block_id(s))
    }

    fn place_root(&mut self, rp: &RootPlacer, pos: BlockPos) {
        let cur = self.region.get_block(pos);
        if rp.mangrove.muddy_roots_in.contains(self.ops.block_id(cur)) {
            let s = rp
                .mangrove
                .muddy_roots_provider
                .get_state(&*self.region, &mut *self.rng, pos);
            let s = self.potentially_waterlogged(pos, s);
            self.set_root(pos, s);
            return;
        }
        if !self.can_place_root(rp, pos) {
            return;
        }
        let s = rp
            .root_provider
            .get_state(&*self.region, &mut *self.rng, pos);
        let s = self.potentially_waterlogged(pos, s);
        self.set_root(pos, s);
        if let Some(above) = &rp.above_root {
            let ap = pos.above();
            if self.rng.next_float() < above.chance && self.region.is_empty_block(ap) {
                let a = above.provider.get_state(&*self.region, &mut *self.rng, ap);
                let a = self.potentially_waterlogged(ap, a);
                self.set_root(ap, a);
            }
        }
    }

    /// `MangroveRootPlacer.placeRoots`.
    pub(super) fn place_roots(&mut self, origin: BlockPos, trunk_origin: BlockPos) -> bool {
        let cfg = self.cfg;
        let Some(rp) = &cfg.root_placer else {
            return true;
        };
        let mut p = origin;
        while p.y < trunk_origin.y {
            if !self.can_place_root(rp, p) {
                return false;
            }
            p = p.above();
        }
        let mut root_positions: Vec<BlockPos> = vec![trunk_origin.below()];
        for dir in Direction::HORIZONTAL {
            let start = trunk_origin.relative(dir);
            let mut arm = Vec::new();
            if !self.simulate_roots(rp, start, dir, trunk_origin, &mut arm, 0) {
                return false;
            }
            root_positions.extend(arm);
            root_positions.push(start);
        }
        for rpos in root_positions {
            self.place_root(rp, rpos);
        }
        true
    }

    // The `width > max_root_width` branch must not draw RNG while the next one does, so the two
    // identical-looking bodies cannot be merged.
    #[allow(clippy::if_same_then_else)]
    fn simulate_roots(
        &mut self,
        rp: &RootPlacer,
        root_pos: BlockPos,
        dir: Direction,
        root_origin: BlockPos,
        arm: &mut Vec<BlockPos>,
        layer: i32,
    ) -> bool {
        let m = &rp.mangrove;
        if layer == m.max_root_length || arm.len() as i32 > m.max_root_length {
            return false;
        }
        let below = root_pos.below();
        let next_to = root_pos.relative(dir);
        let width = root_pos.dist_manhattan(root_origin);
        let candidates: Vec<BlockPos> = if width > m.max_root_width - 3 && width <= m.max_root_width
        {
            if self.rng.next_float() < m.random_skew_chance {
                vec![below, next_to.below()]
            } else {
                vec![below]
            }
        } else if width > m.max_root_width {
            vec![below]
        } else if self.rng.next_float() < m.random_skew_chance {
            vec![below]
        } else if self.rng.next_boolean() {
            vec![next_to]
        } else {
            vec![below]
        };
        for cand in candidates {
            if self.can_place_root(rp, cand) {
                arm.push(cand);
                if !self.simulate_roots(rp, cand, dir, root_origin, arm, layer + 1) {
                    return false;
                }
            }
        }
        true
    }
}
