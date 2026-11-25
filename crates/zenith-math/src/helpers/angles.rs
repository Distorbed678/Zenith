//! Angle normalization, shortest-arc stepping, rotation interpolation, byte angle packing,
//! `SegmentedAnglePrecision`, and quaternion twist projection (`Mth`, `SegmentedAnglePrecision`).
//!
//! Implements Section 5 of `docs/specs/math/java-math-helpers.md`.

use crate::helpers::geom::Direction;
use crate::helpers::roots::inv_sqrt_f32;
use crate::helpers::rounding::{clamp_f32, floor_f32, java_round_f32};
use crate::helpers::trig::{PI, TWO_PI};

// ---------------------------------------------------------------------------
// Degree wrapping
// ---------------------------------------------------------------------------

/// `Mth.wrapDegrees(int) -> int`: wraps into half-open `[-180, 180)`.
#[inline]
pub const fn wrap_degrees_i32(angle: i32) -> i32 {
    let mut r = angle % 360;
    if r >= 180 {
        r -= 360;
    }
    if r < -180 {
        r += 360;
    }
    r
}

/// `Mth.wrapDegrees(long) -> float`: computes `angle % 360L` in `i64`, casts to `f32`, then
/// wraps into half-open `[-180.0, 180.0)`.
#[inline]
pub fn wrap_degrees_i64(angle: i64) -> f32 {
    let mut r = (angle % 360_i64) as f32;
    if r >= 180.0_f32 {
        r -= 360.0_f32;
    }
    if r < -180.0_f32 {
        r += 360.0_f32;
    }
    r
}

/// `Mth.wrapDegrees(float) -> float`: wraps into half-open `[-180.0, 180.0)`.
///
/// Preserves `-0.0`, maps `±180.0 -> -180.0`, and returns `NaN` on `±inf` or `NaN`.
#[inline]
pub fn wrap_degrees_f32(angle: f32) -> f32 {
    let mut r = angle % 360.0_f32;
    if r >= 180.0_f32 {
        r -= 360.0_f32;
    }
    if r < -180.0_f32 {
        r += 360.0_f32;
    }
    r
}

/// `Mth.wrapDegrees(double) -> double`: wraps into half-open `[-180.0, 180.0)`.
///
/// Preserves `-0.0`, maps `±180.0 -> -180.0`, and returns `NaN` on `±inf` or `NaN`.
#[inline]
pub fn wrap_degrees_f64(angle: f64) -> f64 {
    let mut r = angle % 360.0_f64;
    if r >= 180.0_f64 {
        r -= 360.0_f64;
    }
    if r < -180.0_f64 {
        r += 360.0_f64;
    }
    r
}

/// `Mth.wrapDegrees90(float) -> float`: wraps into half-open `[-45.0, 45.0)`.
#[inline]
pub fn wrap_degrees_90(angle: f32) -> f32 {
    let mut r = angle % 90.0_f32;
    if r >= 45.0_f32 {
        r -= 90.0_f32;
    }
    if r < -45.0_f32 {
        r += 90.0_f32;
    }
    r
}

// ---------------------------------------------------------------------------
// Angle differences, stepping, and rotational interpolation
// ---------------------------------------------------------------------------

/// `Mth.degreesDifference(from, to)`: `wrap_degrees_f32(to - from)`.
#[inline]
pub fn degrees_difference(from: f32, to: f32) -> f32 {
    wrap_degrees_f32(to - from)
}

/// `Mth.degreesDifferenceAbs(a, b)`: `|degrees_difference(a, b)|`.
#[inline]
pub fn degrees_difference_abs(a: f32, b: f32) -> f32 {
    degrees_difference(a, b).abs()
}

/// `Mth.rotateIfNecessary(base, target, maxDiff)`.
#[inline]
pub fn rotate_if_necessary(base: f32, target: f32, max_diff: f32) -> f32 {
    let diff = degrees_difference(base, target);
    target - clamp_f32(diff, -max_diff, max_diff)
}

/// `Mth.approach(current, target, increment)`.
#[inline]
pub fn approach(current: f32, target: f32, increment: f32) -> f32 {
    let s = increment.abs();
    if current < target {
        clamp_f32(current + s, current, target)
    } else {
        clamp_f32(current - s, target, current)
    }
}

/// `Mth.approachDegrees(current, target, increment)`.
#[inline]
pub fn approach_degrees(current: f32, target: f32, increment: f32) -> f32 {
    let diff = degrees_difference(current, target);
    approach(current, current + diff, increment)
}

/// `Mth.rotLerp(alpha, from, to)` (`f32`): `from + alpha * wrap_degrees_f32(to - from)`.
#[inline]
pub fn rot_lerp_f32(alpha: f32, from: f32, to: f32) -> f32 {
    from + alpha * wrap_degrees_f32(to - from)
}

/// `Mth.rotLerp(alpha, from, to)` (`f64`): `from + alpha * wrap_degrees_f64(to - from)`.
#[inline]
pub fn rot_lerp_f64(alpha: f64, from: f64, to: f64) -> f64 {
    from + alpha * wrap_degrees_f64(to - from)
}

/// `Mth.rotLerpRad(alpha, from, to)` (`f32`): wraps `d = to - from` into `[-PI, PI)` by repeated
/// `+= TWO_PI` / `-= TWO_PI` loops, then returns `from + alpha * d`.
///
/// Mirrors vanilla's additive loop (including `f32` drift accumulation on multi-turn differences).
/// Note: passing `±inf` would loop forever in vanilla; callers should only pass finite angles.
#[inline]
pub fn rot_lerp_rad(alpha: f32, from: f32, to: f32) -> f32 {
    let mut d = to - from;
    while d < -PI {
        d += TWO_PI;
    }
    while d >= PI {
        d -= TWO_PI;
    }
    from + alpha * d
}

// ---------------------------------------------------------------------------
// Byte-packed degrees
// ---------------------------------------------------------------------------

/// `Mth.packDegrees(float) -> byte`: `trunc_i8(floor_f32((angle * 256.0_f32) / 360.0_f32))`.
#[inline]
pub fn pack_degrees(angle: f32) -> i8 {
    floor_f32((angle * 256.0_f32) / 360.0_f32) as i8
}

/// `Mth.unpackDegrees(byte) -> float`: `f32(sign_extend_i32(rot) * 360) / 256.0_f32`.
///
/// Exact in `f32`, spanning `[-180.0_f32, 178.59375_f32]`.
#[inline]
pub fn unpack_degrees(rot: i8) -> f32 {
    (i32::from(rot).wrapping_mul(360) as f32) / 256.0_f32
}

// ---------------------------------------------------------------------------
// SegmentedAnglePrecision
// ---------------------------------------------------------------------------

/// `SegmentedAnglePrecision`: configurable `N`-bit binary angle representation for `N in 2..=30`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SegmentedAnglePrecision {
    mask: i32,
    precision: i32,
    degree_to_angle: f32,
    angle_to_degree: f32,
}

impl SegmentedAnglePrecision {
    /// Creates a new `SegmentedAnglePrecision` for `bit_precision` in `2..=30`, returning `None`
    /// outside that range.
    #[inline]
    pub fn try_new(bit_precision: i32) -> Option<Self> {
        if !(2..=30).contains(&bit_precision) {
            return None;
        }
        let turns = 1_i32 << bit_precision;
        let turns_f = turns as f32;
        Some(Self {
            mask: turns - 1,
            precision: bit_precision,
            degree_to_angle: turns_f / 360.0_f32,
            angle_to_degree: 360.0_f32 / turns_f,
        })
    }

    /// Creates a new `SegmentedAnglePrecision` for `bit_precision` in `2..=30`.
    ///
    /// # Panics
    ///
    /// Panics if `bit_precision` is outside `2..=30`.
    #[inline]
    pub fn new(bit_precision: i32) -> Self {
        Self::try_new(bit_precision)
            .unwrap_or_else(|| panic!("bit_precision must be in 2..=30, got {bit_precision}"))
    }

    /// Returns the bit precision `b` in `2..=30`.
    #[inline]
    pub const fn bit_precision(self) -> i32 {
        self.precision
    }

    /// Returns the bit mask `(1 << b) - 1`.
    #[inline]
    pub const fn mask(self) -> i32 {
        self.mask
    }

    /// `normalize(angle)`: `angle & mask`.
    #[inline]
    pub const fn normalize(self, angle: i32) -> i32 {
        angle & self.mask
    }

    /// `fromDegreesWithTurns(deg)`: `Math.round(deg * degree_to_angle)`.
    #[inline]
    pub fn from_degrees_with_turns(self, deg: f32) -> i32 {
        java_round_f32(deg * self.degree_to_angle)
    }

    /// `fromDegrees(deg)`: `normalize(from_degrees_with_turns(deg))`.
    #[inline]
    pub fn from_degrees(self, deg: f32) -> i32 {
        self.normalize(self.from_degrees_with_turns(deg))
    }

    /// `toDegreesWithTurns(angle)`: `f32(angle) * angle_to_degree`.
    #[inline]
    pub fn to_degrees_with_turns(self, angle: i32) -> f32 {
        (angle as f32) * self.angle_to_degree
    }

    /// `toDegrees(angle)`: normalized angle mapped into `[-180.0, 180.0)`.
    #[inline]
    pub fn to_degrees(self, angle: i32) -> f32 {
        let d = self.to_degrees_with_turns(self.normalize(angle));
        if d >= 180.0_f32 { d - 360.0_f32 } else { d }
    }

    /// `isSameAxis(a, b)`: `(a & (mask >> 1)) == (b & (mask >> 1))`.
    #[inline]
    pub const fn is_same_axis(self, a: i32, b: i32) -> bool {
        let half_mask = self.mask >> 1;
        (a & half_mask) == (b & half_mask)
    }

    /// `fromDirection(dir)`: `0` for vertical (`Up`/`Down`), else `dir.data_2d() << (b - 2)`
    /// (South = 0, West = 1, North = 2, East = 3).
    #[inline]
    pub const fn from_direction(self, dir: Direction) -> i32 {
        match dir.data_2d() {
            Some(v) => v << (self.precision - 2),
            None => 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Quaternion twist projection
// ---------------------------------------------------------------------------

/// Single-precision quaternion `(x, y, z, w)` matching JOML `Quaternionf` field order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quaternionf {
    /// Imaginary `i` component.
    pub x: f32,
    /// Imaginary `j` component.
    pub y: f32,
    /// Imaginary `k` component.
    pub z: f32,
    /// Real (scalar) component.
    pub w: f32,
}

impl Quaternionf {
    /// Identity quaternion `(0, 0, 0, 1)`.
    pub const IDENTITY: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    };

    /// Constructs a quaternion `(x, y, z, w)`.
    #[inline]
    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self { x, y, z, w }
    }
}

/// `Mth.rotationAroundAxis(axis, q)`: projects `q` onto `axis` and normalizes using
/// [`inv_sqrt_f32`] (matching JOML `Quaternionf.normalize`).
#[inline]
pub fn rotation_around_axis(axis: [f32; 3], q: Quaternionf) -> Quaternionf {
    let p = (axis[0] * q.x + axis[1] * q.y) + axis[2] * q.z;
    let rx = axis[0] * p;
    let ry = axis[1] * p;
    let rz = axis[2] * p;
    let rw = q.w;
    let inv_len = inv_sqrt_f32(((rx * rx + ry * ry) + rz * rz) + rw * rw);
    Quaternionf {
        x: rx * inv_len,
        y: ry * inv_len,
        z: rz * inv_len,
        w: rw * inv_len,
    }
}
