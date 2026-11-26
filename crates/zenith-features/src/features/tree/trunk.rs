//! Trunk placers (`features-trees.md` §4).

use zenith_blocks::Direction;
use zenith_math::RandomSource;
use zenith_math::helpers::intrinsics::{java_math_cos, java_math_sin};
use zenith_math::helpers::trig;

use super::config::TrunkKind;
use super::place::{Attachment, TreeSession};
use crate::providers::IntProvider;
use crate::rng::{
    BlockPos, direction_all_shuffled, direction_axis_name, direction_horizontal_random,
    direction_step, random_between_inclusive,
};

#[inline]
fn floor_f32(v: f32) -> i32 {
    let i = v as i32;
    if v < i as f32 { i - 1 } else { i }
}

#[inline]
fn floor_f64(v: f64) -> i32 {
    let i = v as i32;
    if v < f64::from(i) { i - 1 } else { i }
}

impl TreeSession<'_> {
    /// Dispatches `TrunkPlacer.placeTrunk`.
    pub(super) fn place_trunk(&mut self, tree_height: i32, origin: BlockPos) -> Vec<Attachment> {
        let cfg = self.cfg;
        match &cfg.trunk_placer.kind {
            TrunkKind::Straight => self.trunk_straight(tree_height, origin),
            TrunkKind::Forking => self.trunk_forking(tree_height, origin),
            TrunkKind::Giant => self.trunk_giant(tree_height, origin),
            TrunkKind::MegaJungle => self.trunk_mega_jungle(tree_height, origin),
            TrunkKind::DarkOak => self.trunk_dark_oak(tree_height, origin),
            TrunkKind::Fancy => self.trunk_fancy(tree_height, origin),
            TrunkKind::Bending {
                min_height_for_leaves,
                bend_length,
            } => self.trunk_bending(tree_height, origin, *min_height_for_leaves, bend_length),
            TrunkKind::UpwardsBranching {
                extra_branch_steps,
                place_branch_per_log_probability,
                extra_branch_length,
                ..
            } => self.trunk_upwards_branching(
                tree_height,
                origin,
                extra_branch_steps,
                *place_branch_per_log_probability,
                extra_branch_length,
            ),
            TrunkKind::Cherry {
                branch_count,
                branch_horizontal_length,
                branch_start_offset_from_top,
                branch_end_offset_from_top,
            } => self.trunk_cherry(
                tree_height,
                origin,
                branch_count,
                branch_horizontal_length,
                *branch_start_offset_from_top,
                branch_end_offset_from_top,
            ),
            TrunkKind::Poplar {
                trunk_height_above_branches,
                branch_amount,
            } => self.trunk_poplar(
                tree_height,
                origin,
                trunk_height_above_branches,
                branch_amount,
            ),
        }
    }

    fn trunk_straight(&mut self, tree_height: i32, origin: BlockPos) -> Vec<Attachment> {
        self.place_below_trunk_block(origin.below());
        for y in 0..tree_height {
            self.place_log(origin.above_n(y), None);
        }
        vec![Attachment::new(origin.above_n(tree_height), 0, false)]
    }

    fn trunk_forking(&mut self, tree_height: i32, origin: BlockPos) -> Vec<Attachment> {
        self.place_below_trunk_block(origin.below());
        let mut out = Vec::new();
        let lean_dir = direction_horizontal_random(&mut *self.rng);
        let lean_height = tree_height - self.rng.next_int_bounded(4) - 1;
        let mut lean_steps = 3 - self.rng.next_int_bounded(3);
        let mut tx = origin.x;
        let mut tz = origin.z;
        let mut ey: Option<i32> = None;
        let (ldx, _, ldz) = direction_step(lean_dir);
        for yo in 0..tree_height {
            let yy = origin.y + yo;
            if yo >= lean_height && lean_steps > 0 {
                tx += ldx;
                tz += ldz;
                lean_steps -= 1;
            }
            if self.place_log(BlockPos::new(tx, yy, tz), None) {
                ey = Some(yy + 1);
            }
        }
        if let Some(v) = ey {
            out.push(Attachment::new(BlockPos::new(tx, v, tz), 1, false));
        }
        tx = origin.x;
        tz = origin.z;
        let branch_dir = direction_horizontal_random(&mut *self.rng);
        if branch_dir != lean_dir {
            let (bdx, _, bdz) = direction_step(branch_dir);
            let branch_pos = lean_height - self.rng.next_int_bounded(2) - 1;
            let mut branch_steps = 1 + self.rng.next_int_bounded(3);
            ey = None;
            let mut yo = branch_pos;
            while yo < tree_height && branch_steps > 0 {
                if yo >= 1 {
                    let yy = origin.y + yo;
                    tx += bdx;
                    tz += bdz;
                    if self.place_log(BlockPos::new(tx, yy, tz), None) {
                        ey = Some(yy + 1);
                    }
                }
                yo += 1;
                branch_steps -= 1;
            }
            if let Some(v) = ey {
                out.push(Attachment::new(BlockPos::new(tx, v, tz), 0, false));
            }
        }
        out
    }

    fn place_below_2x2(&mut self, origin: BlockPos) {
        let b = origin.below();
        self.place_below_trunk_block(b);
        self.place_below_trunk_block(b.relative(Direction::East));
        self.place_below_trunk_block(b.relative(Direction::South));
        self.place_below_trunk_block(b.relative(Direction::South).relative(Direction::East));
    }

    fn trunk_giant(&mut self, tree_height: i32, origin: BlockPos) -> Vec<Attachment> {
        self.place_below_2x2(origin);
        for hh in 0..tree_height {
            self.place_log_if_free(origin.offset(0, hh, 0));
            if hh < tree_height - 1 {
                self.place_log_if_free(origin.offset(1, hh, 0));
                self.place_log_if_free(origin.offset(1, hh, 1));
                self.place_log_if_free(origin.offset(0, hh, 1));
            }
        }
        vec![Attachment::new(origin.above_n(tree_height), 0, true)]
    }

    fn trunk_mega_jungle(&mut self, tree_height: i32, origin: BlockPos) -> Vec<Attachment> {
        let mut out = self.trunk_giant(tree_height, origin);
        let mut branch_height = tree_height - 2 - self.rng.next_int_bounded(4);
        while branch_height > tree_height / 2 {
            let angle: f32 = self.rng.next_float() * f32::from_bits(0x40C9_0FDB);
            let mut bx = 0;
            let mut bz = 0;
            for b in 0..=4 {
                bx = (1.5_f32 + trig::cos(f64::from(angle)) * b as f32) as i32;
                bz = (1.5_f32 + trig::sin(f64::from(angle)) * b as f32) as i32;
                self.place_log(origin.offset(bx, branch_height - 3 + b / 2, bz), None);
            }
            out.push(Attachment::new(
                origin.offset(bx, branch_height, bz),
                -2,
                false,
            ));
            branch_height -= 2 + self.rng.next_int_bounded(4);
        }
        out
    }

    fn trunk_dark_oak(&mut self, tree_height: i32, origin: BlockPos) -> Vec<Attachment> {
        self.place_below_2x2(origin);
        let mut out = Vec::new();
        let lean_dir = direction_horizontal_random(&mut *self.rng);
        let lean_height = tree_height - self.rng.next_int_bounded(4);
        let mut lean_steps = 2 - self.rng.next_int_bounded(3);
        let (x, y, z) = (origin.x, origin.y, origin.z);
        let mut tx = x;
        let mut tz = z;
        let ey = y + tree_height - 1;
        let (ldx, _, ldz) = direction_step(lean_dir);
        for dy in 0..tree_height {
            if dy >= lean_height && lean_steps > 0 {
                tx += ldx;
                tz += ldz;
                lean_steps -= 1;
            }
            let p = BlockPos::new(tx, y + dy, tz);
            if self.is_air_or_leaves(p) {
                self.place_log(p, None);
                self.place_log(p.relative(Direction::East), None);
                self.place_log(p.relative(Direction::South), None);
                self.place_log(p.relative(Direction::East).relative(Direction::South), None);
            }
        }
        out.push(Attachment::new(BlockPos::new(tx, ey, tz), 0, true));
        for ox in -1..=2 {
            for oz in -1..=2 {
                if !((0..=1).contains(&ox) && (0..=1).contains(&oz))
                    && self.rng.next_int_bounded(3) <= 0
                {
                    let length = self.rng.next_int_bounded(3) + 2;
                    for branch_y in 0..length {
                        self.place_log(BlockPos::new(x + ox, ey - branch_y - 1, z + oz), None);
                    }
                    out.push(Attachment::new(BlockPos::new(x + ox, ey, z + oz), 0, false));
                }
            }
        }
        out
    }

    // ---- fancy ------------------------------------------------------------------------------

    fn fancy_tree_shape(height: i32, y: i32) -> f32 {
        if (y as f32) < (height as f32) * 0.3_f32 {
            return -1.0_f32;
        }
        let radius = (height as f32) / 2.0_f32;
        let adjacent = radius - y as f32;
        let mut distance = f64::from(radius * radius - adjacent * adjacent).sqrt() as f32;
        if adjacent == 0.0_f32 {
            distance = radius;
        } else if adjacent.abs() >= radius {
            return 0.0_f32;
        }
        distance * 0.5_f32
    }

    fn fancy_log_axis(start: BlockPos, pos: BlockPos) -> &'static str {
        let xdiff = (pos.x - start.x).abs();
        let zdiff = (pos.z - start.z).abs();
        let maxdiff = xdiff.max(zdiff);
        if maxdiff > 0 {
            if xdiff == maxdiff { "x" } else { "z" }
        } else {
            "y"
        }
    }

    fn fancy_make_limb(&mut self, start: BlockPos, end: BlockPos, do_place: bool) -> bool {
        if !do_place && start == end {
            return true;
        }
        let dxi = end.x - start.x;
        let dyi = end.y - start.y;
        let dzi = end.z - start.z;
        let steps = dxi.abs().max(dyi.abs().max(dzi.abs()));
        let dx = dxi as f32 / steps as f32;
        let dy = dyi as f32 / steps as f32;
        let dz = dzi as f32 / steps as f32;
        for i in 0..=steps {
            let p = start.offset(
                floor_f32(0.5_f32 + i as f32 * dx),
                floor_f32(0.5_f32 + i as f32 * dy),
                floor_f32(0.5_f32 + i as f32 * dz),
            );
            if do_place {
                self.place_log(p, Some(Self::fancy_log_axis(start, p)));
            } else if !self.is_free(p) {
                return false;
            }
        }
        true
    }

    fn fancy_trim_branches(height: i32, local_y: i32) -> bool {
        f64::from(local_y) >= f64::from(height) * 0.2
    }

    fn trunk_fancy(&mut self, tree_height: i32, origin: BlockPos) -> Vec<Attachment> {
        let height = tree_height + 2;
        let trunk_height = floor_f64(f64::from(height) * 0.618);
        self.place_below_trunk_block(origin.below());
        let q = f64::from(height) / 13.0;
        let clusters_per_y = i32::min(1, floor_f64(1.382 + q * q));
        let trunk_top = origin.y + trunk_height;
        let mut relative_y = height - 5;
        let mut foliage_coords: Vec<(BlockPos, i32)> =
            vec![(origin.above_n(relative_y), trunk_top)];
        while relative_y >= 0 {
            let shape = Self::fancy_tree_shape(height, relative_y);
            if shape >= 0.0_f32 {
                for _ in 0..clusters_per_y {
                    let radius = f64::from(shape) * (f64::from(self.rng.next_float()) + 0.328);
                    let angle = f64::from(self.rng.next_float() * 2.0_f32) * std::f64::consts::PI;
                    let x = radius * java_math_sin(angle) + 0.5;
                    let z = radius * java_math_cos(angle) + 0.5;
                    let check_start = origin.offset(floor_f64(x), relative_y - 1, floor_f64(z));
                    let check_end = check_start.above_n(5);
                    if self.fancy_make_limb(check_start, check_end, false) {
                        let dx = origin.x - check_start.x;
                        let dz = origin.z - check_start.z;
                        let branch_height =
                            f64::from(check_start.y) - f64::from(dx * dx + dz * dz).sqrt() * 0.381;
                        let branch_top = if branch_height > f64::from(trunk_top) {
                            trunk_top
                        } else {
                            branch_height as i32
                        };
                        let base = BlockPos::new(origin.x, branch_top, origin.z);
                        if self.fancy_make_limb(base, check_start, false) {
                            foliage_coords.push((check_start, branch_top));
                        }
                    }
                }
            }
            relative_y -= 1;
        }
        self.fancy_make_limb(origin, origin.above_n(trunk_height), true);
        for &(end_pos, branch_base) in &foliage_coords {
            let base_pos = BlockPos::new(origin.x, branch_base, origin.z);
            if base_pos != end_pos && Self::fancy_trim_branches(height, branch_base - origin.y) {
                self.fancy_make_limb(base_pos, end_pos, true);
            }
        }
        let mut out = Vec::new();
        for &(end_pos, branch_base) in &foliage_coords {
            if Self::fancy_trim_branches(height, branch_base - origin.y) {
                out.push(Attachment::new(end_pos, 0, false));
            }
        }
        out
    }

    // ---- bending ------------------------------------------------------------------------------

    fn trunk_bending(
        &mut self,
        tree_height: i32,
        origin: BlockPos,
        min_height_for_leaves: i32,
        bend_length: &IntProvider,
    ) -> Vec<Attachment> {
        let direction = direction_horizontal_random(&mut *self.rng);
        let log_height = tree_height - 1;
        let mut pos = origin;
        self.place_below_trunk_block(origin.below());
        let mut out = Vec::new();
        for i in 0..=log_height {
            let r = self.rng.next_int_bounded(2);
            if i + 1 >= log_height + r {
                pos = pos.relative(direction);
            }
            if self.valid_tree_pos(pos) {
                self.place_log(pos, None);
            }
            if i >= min_height_for_leaves {
                out.push(Attachment::new(pos, 0, false));
            }
            pos = pos.above();
        }
        let dir_length = bend_length.sample(&mut *self.rng);
        for _ in 0..=dir_length {
            if self.valid_tree_pos(pos) {
                self.place_log(pos, None);
            }
            out.push(Attachment::new(pos, 0, false));
            pos = pos.relative(direction);
        }
        out
    }

    // ---- upwards branching -------------------------------------------------------------------

    fn trunk_upwards_branching(
        &mut self,
        tree_height: i32,
        origin: BlockPos,
        extra_branch_steps: &IntProvider,
        place_branch_per_log_probability: f32,
        extra_branch_length: &IntProvider,
    ) -> Vec<Attachment> {
        let mut out = Vec::new();
        for height_pos in 0..tree_height {
            let current_height = origin.y + height_pos;
            let placed = self.place_log(BlockPos::new(origin.x, current_height, origin.z), None);
            if placed
                && height_pos < tree_height - 1
                && self.rng.next_float() < place_branch_per_log_probability
            {
                let branch_dir = direction_horizontal_random(&mut *self.rng);
                let branch_len = extra_branch_length.sample(&mut *self.rng);
                let second = extra_branch_length.sample(&mut *self.rng);
                let branch_pos = i32::max(0, branch_len - second - 1);
                let mut branch_steps = extra_branch_steps.sample(&mut *self.rng);
                // place_branch
                let mut height_along_branch = current_height + branch_pos;
                let mut log_x = origin.x;
                let mut log_z = origin.z;
                let (bdx, _, bdz) = direction_step(branch_dir);
                let mut idx = branch_pos;
                while idx < tree_height && branch_steps > 0 {
                    if idx >= 1 {
                        let placement_y = current_height + idx;
                        log_x += bdx;
                        log_z += bdz;
                        height_along_branch = placement_y;
                        if self.place_log(BlockPos::new(log_x, placement_y, log_z), None) {
                            height_along_branch += 1;
                        }
                        out.push(Attachment::new(
                            BlockPos::new(log_x, placement_y, log_z),
                            0,
                            false,
                        ));
                    }
                    idx += 1;
                    branch_steps -= 1;
                }
                if height_along_branch - current_height > 1 {
                    let fp = BlockPos::new(log_x, height_along_branch, log_z);
                    out.push(Attachment::new(fp, 0, false));
                    out.push(Attachment::new(fp.below_n(2), 0, false));
                }
            }
            if height_pos == tree_height - 1 {
                out.push(Attachment::new(
                    BlockPos::new(origin.x, current_height + 1, origin.z),
                    0,
                    false,
                ));
            }
        }
        out
    }

    // ---- cherry ------------------------------------------------------------------------------

    fn trunk_cherry(
        &mut self,
        tree_height: i32,
        origin: BlockPos,
        branch_count: &IntProvider,
        branch_horizontal_length: &IntProvider,
        start_offset: (i32, i32),
        branch_end_offset_from_top: &IntProvider,
    ) -> Vec<Attachment> {
        self.place_below_trunk_block(origin.below());
        let first_off = i32::max(
            0,
            tree_height - 1
                + random_between_inclusive(&mut *self.rng, start_offset.0, start_offset.1),
        );
        let mut second_off = i32::max(
            0,
            tree_height - 1
                + random_between_inclusive(&mut *self.rng, start_offset.0, start_offset.1 - 1),
        );
        if second_off >= first_off {
            second_off += 1;
        }
        let count = branch_count.sample(&mut *self.rng);
        let has_middle = count == 3;
        let has_both_sides = count >= 2;
        let trunk_height = if has_middle {
            tree_height
        } else if has_both_sides {
            first_off.max(second_off) + 1
        } else {
            first_off + 1
        };
        for y in 0..trunk_height {
            self.place_log(origin.above_n(y), None);
        }
        let mut out = Vec::new();
        if has_middle {
            out.push(Attachment::new(origin.above_n(trunk_height), 0, false));
        }
        let tree_dir = direction_horizontal_random(&mut *self.rng);
        let a = self.cherry_branch(
            origin,
            tree_height,
            tree_dir,
            first_off,
            first_off < trunk_height - 1,
            branch_horizontal_length,
            branch_end_offset_from_top,
        );
        out.push(a);
        if has_both_sides {
            let b = self.cherry_branch(
                origin,
                tree_height,
                tree_dir.opposite(),
                second_off,
                second_off < trunk_height - 1,
                branch_horizontal_length,
                branch_end_offset_from_top,
            );
            out.push(b);
        }
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn cherry_branch(
        &mut self,
        origin: BlockPos,
        tree_height: i32,
        branch_dir: Direction,
        offset_from_origin: i32,
        middle_continues_upwards: bool,
        branch_horizontal_length: &IntProvider,
        branch_end_offset_from_top: &IntProvider,
    ) -> Attachment {
        let axis = direction_axis_name(branch_dir);
        let mut log_pos = origin.above_n(offset_from_origin);
        let end_off = tree_height - 1 + branch_end_offset_from_top.sample(&mut *self.rng);
        let extend = middle_continues_upwards || end_off < offset_from_origin;
        let dist = branch_horizontal_length.sample(&mut *self.rng) + i32::from(extend);
        let branch_end = origin.relative_n(branch_dir, dist).above_n(end_off);
        let horiz_steps = if extend { 2 } else { 1 };
        for _ in 0..horiz_steps {
            log_pos = log_pos.relative(branch_dir);
            self.place_log(log_pos, Some(axis));
        }
        let vert_dir = if branch_end.y > log_pos.y {
            Direction::Up
        } else {
            Direction::Down
        };
        loop {
            let manhattan = log_pos.dist_manhattan(branch_end);
            if manhattan == 0 {
                return Attachment::new(branch_end.above(), 0, false);
            }
            let vert_chance = ((branch_end.y - log_pos.y).abs() as f32) / manhattan as f32;
            let grow_vert = self.rng.next_float() < vert_chance;
            log_pos = log_pos.relative(if grow_vert { vert_dir } else { branch_dir });
            self.place_log(log_pos, if grow_vert { None } else { Some(axis) });
        }
    }

    // ---- poplar ------------------------------------------------------------------------------

    fn trunk_poplar(
        &mut self,
        tree_height: i32,
        origin: BlockPos,
        trunk_height_above_branches: &IntProvider,
        branch_amount: &IntProvider,
    ) -> Vec<Attachment> {
        self.place_below_trunk_block(origin.below());
        let branch_top_y = tree_height - trunk_height_above_branches.sample(&mut *self.rng);
        for y in 0..tree_height {
            self.place_log(origin.above_n(y), None);
            let all = direction_all_shuffled(&mut *self.rng);
            let dirs: Vec<Direction> = all
                .into_iter()
                .filter(|d| !matches!(d, Direction::Up | Direction::Down))
                .collect();
            if branch_top_y - 1 == y {
                let branches = branch_amount.sample(&mut *self.rng);
                for k in 0..branches {
                    let d = dirs[k as usize];
                    self.place_log(origin.above_n(y).relative(d), Some(direction_axis_name(d)));
                }
            }
        }
        vec![Attachment::new(origin.above_n(branch_top_y), 0, false)]
    }
}
