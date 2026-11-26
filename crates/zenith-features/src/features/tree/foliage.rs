//! Foliage placers (`features-trees.md` §5).

use zenith_blocks::Direction;
use zenith_math::RandomSource;

use super::config::FoliageKind;
use super::place::{Attachment, TreeSession};
use crate::rng::BlockPos;

#[inline]
fn floor_f32(v: f32) -> i32 {
    let i = v as i32;
    if v < i as f32 { i - 1 } else { i }
}

impl TreeSession<'_> {
    /// `FoliagePlacer.foliageHeight(random, treeHeight, config)`.
    pub(super) fn foliage_height(&mut self, tree_height: i32) -> i32 {
        let cfg = self.cfg;
        match &cfg.foliage_placer.kind {
            FoliageKind::Blob { height }
            | FoliageKind::Bush { height }
            | FoliageKind::Fancy { height }
            | FoliageKind::MegaJungle { height } => *height,
            FoliageKind::Spruce { trunk_height } => {
                i32::max(4, tree_height - trunk_height.sample(&mut *self.rng))
            }
            FoliageKind::Pine { height } => height.sample(&mut *self.rng),
            FoliageKind::Acacia => 0,
            FoliageKind::MegaPine { crown_height } => crown_height.sample(&mut *self.rng),
            FoliageKind::DarkOak => 4,
            FoliageKind::RandomSpread { foliage_height, .. } => {
                foliage_height.sample(&mut *self.rng)
            }
            FoliageKind::Cherry { height, .. } | FoliageKind::Poplar { height, .. } => {
                height.sample(&mut *self.rng)
            }
        }
    }

    /// `FoliagePlacer.foliageRadius(random, trunkHeight)`.
    pub(super) fn foliage_radius(&mut self, trunk_height: i32) -> i32 {
        let cfg = self.cfg;
        let base = cfg.foliage_placer.radius.sample(&mut *self.rng);
        if matches!(cfg.foliage_placer.kind, FoliageKind::Pine { .. }) {
            base + self.rng.next_int_bounded(i32::max(trunk_height + 1, 1))
        } else {
            base
        }
    }

    /// `FoliagePlacer.createFoliage` (outer wrapper + per-type body).
    pub(super) fn create_foliage(
        &mut self,
        _tree_height: i32,
        att: Attachment,
        foliage_height: i32,
        leaf_radius: i32,
    ) {
        let cfg = self.cfg;
        let offset = cfg.foliage_placer.offset.sample(&mut *self.rng);
        let h = foliage_height + att.foliage_height_offset;
        let r_off = att.radius_offset;
        let dbl = att.double_trunk;
        match &cfg.foliage_placer.kind {
            FoliageKind::Blob { .. } => {
                let mut yo = offset;
                while yo >= offset - h {
                    let r = i32::max(leaf_radius + r_off - 1 - yo / 2, 0);
                    self.place_leaves_row(att.pos, r, yo, dbl);
                    yo -= 1;
                }
            }
            FoliageKind::Spruce { .. } => {
                let mut cur_r = self.rng.next_int_bounded(2);
                let mut max_r = 1;
                let mut min_r = 0;
                let mut yo = offset;
                while yo >= -h {
                    self.place_leaves_row(att.pos, cur_r, yo, dbl);
                    if cur_r >= max_r {
                        cur_r = min_r;
                        min_r = 1;
                        max_r = i32::min(max_r + 1, leaf_radius + r_off);
                    } else {
                        cur_r += 1;
                    }
                    yo -= 1;
                }
            }
            FoliageKind::Pine { .. } => {
                let mut cur_r = 0;
                let mut yo = offset;
                while yo >= offset - h {
                    self.place_leaves_row(att.pos, cur_r, yo, dbl);
                    if cur_r >= 1 && yo == offset - h + 1 {
                        cur_r -= 1;
                    } else if cur_r < leaf_radius + r_off {
                        cur_r += 1;
                    }
                    yo -= 1;
                }
            }
            FoliageKind::Acacia => {
                let p = att.pos.above_n(offset);
                self.place_leaves_row(p, leaf_radius + r_off, -1 - h, dbl);
                self.place_leaves_row(p, leaf_radius - 1, -h, dbl);
                self.place_leaves_row(p, leaf_radius + r_off - 1, 0, dbl);
            }
            FoliageKind::Bush { .. } => {
                let mut yo = offset;
                while yo >= offset - h {
                    let r = leaf_radius + r_off - 1 - yo;
                    self.place_leaves_row(att.pos, r, yo, dbl);
                    yo -= 1;
                }
            }
            FoliageKind::Fancy { .. } => {
                let mut yo = offset;
                while yo >= offset - foliage_height {
                    let r = leaf_radius + i32::from(yo != offset && yo != offset - foliage_height);
                    self.place_leaves_row(att.pos, r, yo, dbl);
                    yo -= 1;
                }
            }
            FoliageKind::MegaJungle { .. } => {
                let leaf_h = (if dbl {
                    foliage_height
                } else {
                    1 + self.rng.next_int_bounded(2)
                }) + att.foliage_height_offset;
                let mut yo = offset;
                while yo >= offset - leaf_h {
                    let r = leaf_radius + r_off + 1 - yo;
                    self.place_leaves_row(att.pos, r, yo, dbl);
                    yo -= 1;
                }
            }
            FoliageKind::MegaPine { .. } => {
                let mut prev_r = 0;
                let fp = att.pos;
                let mut yy = fp.y - h + offset;
                while yy <= fp.y + offset {
                    let yo = fp.y - yy;
                    let smooth_r =
                        leaf_radius + r_off + floor_f32((yo as f32 / h as f32) * 3.5_f32);
                    let jagged_r = if yo > 0 && smooth_r == prev_r && (yy & 1) == 0 {
                        smooth_r + 1
                    } else {
                        smooth_r
                    };
                    self.place_leaves_row(BlockPos::new(fp.x, yy, fp.z), jagged_r, 0, dbl);
                    prev_r = smooth_r;
                    yy += 1;
                }
            }
            FoliageKind::DarkOak => {
                let p = att.pos.above_n(offset);
                if dbl {
                    self.place_leaves_row(p, leaf_radius + 2, -1, true);
                    self.place_leaves_row(p, leaf_radius + 3, 0, true);
                    self.place_leaves_row(p, leaf_radius + 2, 1, true);
                    if self.rng.next_boolean() {
                        self.place_leaves_row(p, leaf_radius, 2, true);
                    }
                } else {
                    self.place_leaves_row(p, leaf_radius + 2, -1, false);
                    self.place_leaves_row(p, leaf_radius + 1, 0, false);
                }
            }
            FoliageKind::RandomSpread {
                leaf_placement_attempts,
                ..
            } => {
                let p = att.pos;
                for _ in 0..*leaf_placement_attempts {
                    let a = self.rng.next_int_bounded(leaf_radius);
                    let b = self.rng.next_int_bounded(leaf_radius);
                    let dx = a - b;
                    let c = self.rng.next_int_bounded(foliage_height);
                    let d = self.rng.next_int_bounded(foliage_height);
                    let dy = c - d;
                    let e = self.rng.next_int_bounded(leaf_radius);
                    let f = self.rng.next_int_bounded(leaf_radius);
                    let dz = e - f;
                    self.try_place_leaf(p.offset(dx, dy, dz));
                }
            }
            FoliageKind::Cherry {
                hanging_leaves_chance,
                hanging_leaves_extension_chance,
                ..
            } => {
                let p = att.pos.above_n(offset);
                let r = leaf_radius + r_off - 1;
                self.place_leaves_row(p, r - 2, h - 3, dbl);
                self.place_leaves_row(p, r - 1, h - 4, dbl);
                let mut y = h - 5;
                while y >= 0 {
                    self.place_leaves_row(p, r, y, dbl);
                    y -= 1;
                }
                self.place_leaves_row_hanging(
                    p,
                    r,
                    -1,
                    dbl,
                    *hanging_leaves_chance,
                    *hanging_leaves_extension_chance,
                );
                self.place_leaves_row_hanging(
                    p,
                    r - 1,
                    -2,
                    dbl,
                    *hanging_leaves_chance,
                    *hanging_leaves_extension_chance,
                );
            }
            FoliageKind::Poplar {
                side_hole_chance, ..
            } => {
                let shc = *side_hole_chance;
                let p = att.pos.above_n(offset);
                let r = leaf_radius + r_off - 1;
                let flip = self.rng.next_boolean();
                self.poplar_row(p, r - 2, h - 1, dbl, h, flip, shc);
                self.poplar_row(p, r - 1, h - 2, dbl, h, flip, shc);
                self.poplar_row(p, r - 1, h - 3, dbl, h, flip, shc);
                let mut y = h - 4;
                while y >= 1 {
                    self.poplar_row(p, r, y, dbl, h, flip, shc);
                    y -= 1;
                }
                self.poplar_replace_leaves_with_log(p, r, h - 4, dbl, h, flip);
                self.poplar_row(p, r - 1, 0, dbl, h, flip, shc);
                self.poplar_row(p, (r - 2).clamp(1, 2), -1, dbl, h, flip, shc);
            }
        }
    }

    // ---- shared row helpers ---------------------------------------------------------------

    /// `placeLeavesRow(origin, currentRadius, y, doubleTrunk)`.
    fn place_leaves_row(&mut self, origin: BlockPos, r: i32, y: i32, dbl: bool) {
        let ext = i32::from(dbl);
        for dx in -r..=(r + ext) {
            for dz in -r..=(r + ext) {
                if !self.should_skip_signed(dx, y, dz, r, dbl) {
                    self.try_place_leaf(origin.offset(dx, y, dz));
                }
            }
        }
    }

    fn should_skip_signed(&mut self, dx: i32, y: i32, dz: i32, r: i32, dbl: bool) -> bool {
        let cfg = self.cfg;
        if matches!(cfg.foliage_placer.kind, FoliageKind::DarkOak)
            && y == 0
            && dbl
            && (dx == -r || dx >= r)
            && (dz == -r || dz >= r)
        {
            return true;
        }
        let (ax, az) = if dbl {
            (dx.abs().min((dx - 1).abs()), dz.abs().min((dz - 1).abs()))
        } else {
            (dx.abs(), dz.abs())
        };
        self.should_skip_location(ax, y, az, r, dbl)
    }

    fn should_skip_location(&mut self, dx: i32, y: i32, dz: i32, r: i32, dbl: bool) -> bool {
        let cfg = self.cfg;
        match &cfg.foliage_placer.kind {
            FoliageKind::Blob { .. } => {
                dx == r && dz == r && (self.rng.next_int_bounded(2) == 0 || y == 0)
            }
            FoliageKind::Spruce { .. } | FoliageKind::Pine { .. } => dx == r && dz == r && r > 0,
            FoliageKind::Acacia => {
                if y == 0 {
                    (dx > 1 || dz > 1) && dx != 0 && dz != 0
                } else {
                    dx == r && dz == r && r > 0
                }
            }
            FoliageKind::Bush { .. } => dx == r && dz == r && self.rng.next_int_bounded(2) == 0,
            FoliageKind::Fancy { .. } => {
                let a = dx as f32 + 0.5_f32;
                let b = dz as f32 + 0.5_f32;
                a * a + b * b > (r * r) as f32
            }
            FoliageKind::MegaJungle { .. } | FoliageKind::MegaPine { .. } => {
                dx + dz >= 7 || dx * dx + dz * dz > r * r
            }
            FoliageKind::DarkOak => {
                if y == -1 && !dbl {
                    dx == r && dz == r
                } else if y == 1 {
                    dx + dz > r * 2 - 2
                } else {
                    false
                }
            }
            FoliageKind::RandomSpread { .. } | FoliageKind::Poplar { .. } => false,
            FoliageKind::Cherry {
                wide_bottom_layer_hole_chance,
                corner_hole_chance,
                ..
            } => {
                if y == -1
                    && (dx == r || dz == r)
                    && self.rng.next_float() < *wide_bottom_layer_hole_chance
                {
                    return true;
                }
                let corner = dx == r && dz == r;
                if r > 2 {
                    corner || (dx + dz > r * 2 - 2 && self.rng.next_float() < *corner_hole_chance)
                } else {
                    corner && self.rng.next_float() < *corner_hole_chance
                }
            }
        }
    }

    fn try_place_extension(&mut self, chance: f32, log_pos: BlockPos, pos: BlockPos) -> bool {
        if pos.dist_manhattan(log_pos) >= 7 {
            return false;
        }
        if self.rng.next_float() > chance {
            return false;
        }
        self.try_place_leaf(pos)
    }

    fn place_leaves_row_hanging(
        &mut self,
        origin: BlockPos,
        r: i32,
        y: i32,
        dbl: bool,
        hang_chance: f32,
        ext_chance: f32,
    ) {
        self.place_leaves_row(origin, r, y, dbl);
        let ext = i32::from(dbl);
        let log_pos = origin.below();
        for along_edge in Direction::HORIZONTAL {
            let to_edge = along_edge.clockwise();
            let off_to_edge = if to_edge.is_positive() { r + ext } else { r };
            let mut pos = origin
                .offset(0, y - 1, 0)
                .relative_n(to_edge, off_to_edge)
                .relative_n(along_edge, -r);
            let mut i = -r;
            while i < r + ext {
                let leaves_above = self.foliage.contains(pos.above());
                if leaves_above && self.try_place_extension(hang_chance, log_pos, pos) {
                    self.try_place_extension(ext_chance, log_pos, pos.below());
                }
                pos = pos.relative(along_edge);
                i += 1;
            }
        }
    }

    // ---- poplar -----------------------------------------------------------------------------

    fn poplar_corner_cut(dx: i32, dz: i32, r: i32, partial: bool, flip: bool) -> i32 {
        let small = if flip {
            (dx > 0 && dz > 0) || (dz < 0 && dx < 0)
        } else {
            (dx > 0 && dz < 0) || (dz > 0 && dx < 0)
        };
        if small {
            r - 1
        } else if partial {
            r + 1
        } else {
            r
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn poplar_row(
        &mut self,
        origin: BlockPos,
        r: i32,
        y: i32,
        dbl: bool,
        h: i32,
        flip: bool,
        side_hole_chance: f32,
    ) {
        let ext = i32::from(dbl);
        let partial = y == h - 1 || y == h - 2;
        for dx in -r..=(r + ext) {
            for dz in -r..=(r + ext) {
                let cut = Self::poplar_corner_cut(dx, dz, r, partial, flip);
                let abs_dx = dx.abs();
                let abs_dz = dz.abs();
                if partial && (abs_dx == r || abs_dz == r) {
                    continue;
                }
                let extra = i32::from(self.rng.next_float() <= side_hole_chance);
                if abs_dx + abs_dz <= r * 2 - (cut + extra) {
                    self.try_place_leaf(origin.offset(dx, y, dz));
                }
            }
        }
    }

    fn poplar_replace_leaves_with_log(
        &mut self,
        origin: BlockPos,
        r: i32,
        y: i32,
        dbl: bool,
        h: i32,
        flip: bool,
    ) {
        let cfg = self.cfg;
        let ext = i32::from(dbl);
        let partial = y == h - 1 || y == h - 2;
        for dx in -r..=(r + ext) {
            for dz in -r..=(r + ext) {
                let abs_dz = dz.abs();
                let abs_dx = dx.abs();
                let cut = Self::poplar_corner_cut(dx, dz, r, partial, flip);
                let within = abs_dx + abs_dz <= r * 2 - (cut + 2);
                if within && ((abs_dz == 0 && r - abs_dx >= 4) || (abs_dx == 0 && r - abs_dz >= 4))
                {
                    let pos = origin.offset(dx, y, dz);
                    let axis = if abs_dz == 0 { "x" } else { "z" };
                    let expected =
                        cfg.foliage_provider
                            .get_state(&*self.region, &mut *self.rng, pos);
                    if expected == self.region.get_block(pos) {
                        let raw = cfg
                            .trunk_provider
                            .get_state(&*self.region, &mut *self.rng, pos);
                        let state = self.ops.try_set_property(raw, "axis", axis);
                        self.set_foliage(pos, state);
                    }
                }
            }
        }
    }
}
