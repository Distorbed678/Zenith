//! Structure terrain adaptation (`Beardifier`, `docs/specs/worldgen/density-functions.md` §8).
//!
//! Provides the complete `Beardifier` interface, `Beardifier::empty()` for chunks with no
//! structures (exact zero contribution), and the 24³ kernel evaluation. The pieces and junctions
//! are collected from a chunk's real structure starts by `zenith-structures`
//! (`beardifier_from_starts`, `docs/specs/worldgen/structures-placement-and-jigsaw.md` §5.2) and
//! passed to the terrain step through `TerrainGenerator::generate_chunk_with_hooks`.

use std::sync::OnceLock;

use zenith_math::helpers::intrinsics::java_math_pow;
use zenith_math::helpers::roots::{fast_inv_sqrt, sqrt};
use zenith_math::helpers::rounding::java_floor_div_i32;

use crate::volume::{ContextSampler, DensityVolume};

const KERNEL_SIZE: usize = 24;
const KERNEL_RADIUS: i32 = 12;
const KERNEL_LEN: usize = KERNEL_SIZE * KERNEL_SIZE * KERNEL_SIZE;

fn beard_kernel() -> &'static [f32; KERNEL_LEN] {
    static KERNEL: OnceLock<[f32; KERNEL_LEN]> = OnceLock::new();
    KERNEL.get_or_init(|| {
        let mut table = [0.0_f32; KERNEL_LEN];
        for zi in 0..KERNEL_SIZE {
            let dz = (zi as i32) - KERNEL_RADIUS;
            for xi in 0..KERNEL_SIZE {
                let dx = (xi as i32) - KERNEL_RADIUS;
                for yi in 0..KERNEL_SIZE {
                    let dy = f64::from((yi as i32) - KERNEL_RADIUS) + 0.5_f64;
                    let dist_sq = f64::from(dx * dx) + dy * dy + f64::from(dz * dz);
                    let idx = zi * 576 + xi * 24 + yi;
                    // Spec §8: Math.pow(Math.E, -distSq / 16.0) narrowed to f32.
                    table[idx] = java_math_pow(std::f64::consts::E, -dist_sq / 16.0_f64) as f32;
                }
            }
        }
        table
    })
}

/// Inclusive integer 3D bounding box matching vanilla `BoundingBox`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BoundingBox {
    /// Minimum X block coordinate (inclusive).
    pub min_x: i32,
    /// Minimum Y block coordinate (inclusive).
    pub min_y: i32,
    /// Minimum Z block coordinate (inclusive).
    pub min_z: i32,
    /// Maximum X block coordinate (inclusive).
    pub max_x: i32,
    /// Maximum Y block coordinate (inclusive).
    pub max_y: i32,
    /// Maximum Z block coordinate (inclusive).
    pub max_z: i32,
}

impl BoundingBox {
    /// Returns a 1×1×1 bounding box at `(x, y, z)`.
    #[inline]
    pub const fn point(x: i32, y: i32, z: i32) -> Self {
        Self {
            min_x: x,
            min_y: y,
            min_z: z,
            max_x: x,
            max_y: y,
            max_z: z,
        }
    }

    /// Returns `true` if `(x, y, z)` lies inside this inclusive box.
    #[inline]
    pub const fn contains(&self, x: i32, y: i32, z: i32) -> bool {
        x >= self.min_x
            && x <= self.max_x
            && y >= self.min_y
            && y <= self.max_y
            && z >= self.min_z
            && z <= self.max_z
    }

    /// Returns `true` if this box intersects `volume`'s inclusive `[min_block, max_block]` extent.
    #[inline]
    pub const fn intersects_volume(&self, volume: &DensityVolume) -> bool {
        self.max_x >= volume.min_block_x()
            && self.min_x <= volume.max_block_x()
            && self.max_y >= volume.min_block_y()
            && self.min_y <= volume.max_block_y()
            && self.max_z >= volume.min_block_z()
            && self.min_z <= volume.max_block_z()
    }

    /// Expands this box in place to enclose `other`.
    #[inline]
    pub fn encapsulate(&mut self, other: Self) {
        self.min_x = self.min_x.min(other.min_x);
        self.min_y = self.min_y.min(other.min_y);
        self.min_z = self.min_z.min(other.min_z);
        self.max_x = self.max_x.max(other.max_x);
        self.max_y = self.max_y.max(other.max_y);
        self.max_z = self.max_z.max(other.max_z);
    }

    /// Returns a new box expanded outward by `amount` on all six faces.
    #[inline]
    pub const fn inflated_by(&self, amount: i32) -> Self {
        Self {
            min_x: self.min_x.saturating_sub(amount),
            min_y: self.min_y.saturating_sub(amount),
            min_z: self.min_z.saturating_sub(amount),
            max_x: self.max_x.saturating_add(amount),
            max_y: self.max_y.saturating_add(amount),
            max_z: self.max_z.saturating_add(amount),
        }
    }
}

/// Structure terrain-adaptation mode (`TerrainAdjustment`, spec §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TerrainAdjustment {
    /// No terrain adaptation.
    None,
    /// Buries the piece box smoothly into terrain.
    Bury,
    /// Thin beard beneath ground level.
    BeardThin,
    /// Box-clamped beard beneath ground level.
    BeardBox,
    /// Encapsulates the full bounding box.
    Encapsulate,
}

/// One rigid structure piece contributing to a [`Beardifier`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BeardifierRigid {
    /// Piece bounding box.
    pub bounding_box: BoundingBox,
    /// Terrain adaptation mode.
    pub adjustment: TerrainAdjustment,
    /// Ground level offset relative to `bounding_box.min_y`.
    pub ground_level_delta: i32,
}

/// One jigsaw junction contributing to a [`Beardifier`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JigsawJunction {
    /// Junction source X.
    pub source_x: i32,
    /// Junction source ground Y.
    pub source_ground_y: i32,
    /// Junction source Z.
    pub source_z: i32,
}

/// Structure terrain-adaptation sampler (`Beardifier`, `density-functions.md` §8).
#[derive(Debug, Clone)]
pub struct Beardifier {
    rigids: Vec<BeardifierRigid>,
    junctions: Vec<JigsawJunction>,
    affected_box: Option<BoundingBox>,
}

impl Beardifier {
    /// Empty beardifier (`Beardifier.EMPTY`): contributes `0.0_f32` everywhere.
    pub const fn empty() -> Self {
        Self {
            rigids: Vec::new(),
            junctions: Vec::new(),
            affected_box: None,
        }
    }

    /// Constructs a `Beardifier` from collected `rigids` and `junctions`.
    pub fn new(rigids: Vec<BeardifierRigid>, junctions: Vec<JigsawJunction>) -> Self {
        let mut any_box: Option<BoundingBox> = None;
        for r in &rigids {
            match &mut any_box {
                Some(b) => b.encapsulate(r.bounding_box),
                None => any_box = Some(r.bounding_box),
            }
        }
        for j in &junctions {
            let p = BoundingBox::point(j.source_x, j.source_ground_y, j.source_z);
            match &mut any_box {
                Some(b) => b.encapsulate(p),
                None => any_box = Some(p),
            }
        }
        let affected_box = any_box.map(|b| b.inflated_by(24));
        Self {
            rigids,
            junctions,
            affected_box,
        }
    }

    /// Returns `true` if this beardifier has no structure pieces or junctions.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.affected_box.is_none()
    }

    /// Returns the inflated bounding box outside which contributions are guaranteed zero.
    #[inline]
    pub const fn affected_box(&self) -> Option<BoundingBox> {
        self.affected_box
    }

    fn sample_value_unchecked(&self, bx: i32, by: i32, bz: i32) -> f32 {
        let mut total = 0.0_f32;
        for r in &self.rigids {
            let b = r.bounding_box;
            let dx = 0.max((b.min_x - bx).max(bx - b.max_x));
            let dz = 0.max((b.min_z - bz).max(bz - b.max_z));
            let ground_y = b.min_y + r.ground_level_delta;
            let dy_to_ground = by - ground_y;
            let dy = match r.adjustment {
                TerrainAdjustment::None => 0,
                TerrainAdjustment::Bury | TerrainAdjustment::BeardThin => dy_to_ground,
                TerrainAdjustment::BeardBox => 0.max((ground_y - by).max(by - b.max_y)),
                TerrainAdjustment::Encapsulate => 0.max((b.min_y - by).max(by - b.max_y)),
            };
            let contrib = match r.adjustment {
                TerrainAdjustment::None => 0.0_f32,
                TerrainAdjustment::Bury => bury(dx as f32, (dy as f32) / 2.0_f32, dz as f32),
                TerrainAdjustment::BeardThin | TerrainAdjustment::BeardBox => {
                    beard(dx, dy, dz, dy_to_ground) * 0.8_f32
                }
                TerrainAdjustment::Encapsulate => {
                    bury(
                        (dx as f32) / 2.0_f32,
                        (dy as f32) / 2.0_f32,
                        (dz as f32) / 2.0_f32,
                    ) * 0.8_f32
                }
            };
            total += contrib;
        }
        for j in &self.junctions {
            let dx = bx - j.source_x;
            let dy = by - j.source_ground_y;
            let dz = bz - j.source_z;
            total += beard(dx, dy, dz, dy) * 0.4_f32;
        }
        total
    }
}

impl Default for Beardifier {
    fn default() -> Self {
        Self::empty()
    }
}

impl ContextSampler for Beardifier {
    fn sample_value(&self, bx: i32, by: i32, bz: i32) -> f32 {
        match self.affected_box {
            Some(box_) if box_.contains(bx, by, bz) => self.sample_value_unchecked(bx, by, bz),
            _ => 0.0_f32,
        }
    }

    fn sample_volume(&self, out: &mut [f32], volume: DensityVolume) {
        out.fill(0.0_f32);
        let Some(box_) = self.affected_box else {
            return;
        };
        if !box_.intersects_volume(&volume) {
            return;
        }
        let min_x =
            java_floor_div_i32(0.max(box_.min_x - volume.min_block_x()), volume.step_x()) as usize;
        let max_x = ((volume.size_x() - 1) as i32).min(java_floor_div_i32(
            box_.max_x - volume.min_block_x(),
            volume.step_x(),
        )) as usize;
        let min_y =
            java_floor_div_i32(0.max(box_.min_y - volume.min_block_y()), volume.step_y()) as usize;
        let max_y = ((volume.size_y() - 1) as i32).min(java_floor_div_i32(
            box_.max_y - volume.min_block_y(),
            volume.step_y(),
        )) as usize;
        let min_z =
            java_floor_div_i32(0.max(box_.min_z - volume.min_block_z()), volume.step_z()) as usize;
        let max_z = ((volume.size_z() - 1) as i32).min(java_floor_div_i32(
            box_.max_z - volume.min_block_z(),
            volume.step_z(),
        )) as usize;

        for iz in min_z..=max_z {
            let bz = volume.block_z(iz);
            for ix in min_x..=max_x {
                let bx = volume.block_x(ix);
                for iy in min_y..=max_y {
                    let by = volume.block_y(iy);
                    out[volume.index(ix, iy, iz)] = self.sample_value_unchecked(bx, by, bz);
                }
            }
        }
    }
}

#[inline]
fn bury(dx: f32, dy: f32, dz: f32) -> f32 {
    let d2 = (dx * dx + dy * dy) + dz * dz;
    if d2 >= 36.0_f32 {
        0.0_f32
    } else {
        1.0_f32 - sqrt(d2) / 6.0_f32
    }
}

#[inline]
fn beard(dx: i32, dy: i32, dz: i32, y_to_ground: i32) -> f32 {
    let xi = dx + KERNEL_RADIUS;
    let yi = dy + KERNEL_RADIUS;
    let zi = dz + KERNEL_RADIUS;
    if !(0..24).contains(&xi) || !(0..24).contains(&yi) || !(0..24).contains(&zi) {
        return 0.0_f32;
    }
    let dy_off = (y_to_ground as f32) + 0.5_f32;
    let d2 = ((dx as f32) * (dx as f32) + dy_off * dy_off) + (dz as f32) * (dz as f32);
    let val = (-dy_off * (fast_inv_sqrt(f64::from(d2 / 2.0_f32)) as f32)) / 2.0_f32;
    let idx = (zi as usize) * 576 + (xi as usize) * 24 + (yi as usize);
    val * beard_kernel()[idx]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rigid(adjustment: TerrainAdjustment) -> BeardifierRigid {
        BeardifierRigid {
            bounding_box: BoundingBox {
                min_x: 0,
                min_y: 60,
                min_z: 0,
                max_x: 15,
                max_y: 70,
                max_z: 15,
            },
            adjustment,
            ground_level_delta: 1,
        }
    }

    #[test]
    fn empty_is_zero_everywhere() {
        let b = Beardifier::empty();
        assert!(b.is_empty());
        assert_eq!(b.sample_value(0, 64, 0), 0.0);
    }

    #[test]
    fn affected_box_is_inflated_by_24() {
        let b = Beardifier::new(vec![rigid(TerrainAdjustment::Bury)], Vec::new());
        let bb = b.affected_box().expect("non-empty");
        assert_eq!((bb.min_x, bb.min_y, bb.min_z), (-24, 36, -24));
        assert_eq!((bb.max_x, bb.max_y, bb.max_z), (39, 94, 39));
        // Outside the inflated box the value is exactly zero.
        assert_eq!(b.sample_value(40, 64, 0), 0.0);
    }

    #[test]
    fn bury_is_one_inside_the_box_and_zero_beyond_six_blocks() {
        let b = Beardifier::new(vec![rigid(TerrainAdjustment::Bury)], Vec::new());
        // Inside the box horizontally and at ground level: dx = dz = 0, dy = 0 -> 1 - 0/6.
        assert_eq!(b.sample_value(5, 61, 5), 1.0);
        // 6 blocks outside horizontally -> d2 = 36 -> 0.
        assert_eq!(b.sample_value(21, 61, 5), 0.0);
        // 3 blocks outside horizontally -> 1 - 3/6.
        assert_eq!(b.sample_value(18, 61, 5), 0.5);
    }

    #[test]
    fn beard_modes_are_non_zero_below_ground_inside_the_box() {
        for mode in [TerrainAdjustment::BeardThin, TerrainAdjustment::BeardBox] {
            let b = Beardifier::new(vec![rigid(mode)], Vec::new());
            assert!(b.sample_value(5, 55, 5) != 0.0, "{mode:?}");
        }
    }

    #[test]
    fn none_adjustment_contributes_nothing() {
        let b = Beardifier::new(vec![rigid(TerrainAdjustment::None)], Vec::new());
        assert_eq!(b.sample_value(5, 61, 5), 0.0);
    }

    #[test]
    fn junction_is_zero_outside_the_kernel_range() {
        let j = JigsawJunction {
            source_x: 0,
            source_ground_y: 64,
            source_z: 0,
        };
        let b = Beardifier::new(Vec::new(), vec![j]);
        assert!(b.sample_value(1, 63, 1) != 0.0);
        // dx = +12 is outside the kernel's [-12, 11] range.
        assert_eq!(b.sample_value(12, 64, 0), 0.0);
    }
}
