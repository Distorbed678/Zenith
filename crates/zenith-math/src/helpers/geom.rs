//! 3D/2D vectors, `LpVec3` network quantization, `AABB`, slab segment clipping, and SAT
//! ray-AABB intersection (`Vec3i`, `BlockPos`, `Vec3`, `Vec2`, `LpVec3`, `AABB`,
//! `Mth.rayIntersectsAABB`).
//!
//! Implements Section 9 of `docs/specs/math/java-math-helpers.md`.

use std::fmt;

use crate::helpers::hash::vec3i_hash_code;
use crate::helpers::interp::lerp_f64;
use crate::helpers::intrinsics::{java_math_asin, java_math_atan2};
use crate::helpers::packing::{block_as_long, block_get_x, block_get_y, block_get_z};
use crate::helpers::rounding::{
    EPSILON, EPSILON_F64, abs_max_f64, ceil_long, clamp_f64, floor_f64, java_clamp_f64,
    java_max_f32, java_max_f64, java_min_f32, java_min_f64, java_round_f64,
};
use crate::helpers::trig::{DEG_TO_RAD, PI, RAD_TO_DEG, cos, sin};
use crate::random::RandomSource;

/// `Vec2` normalization threshold `1.0E-4F` (`0x38D1B717`).
pub const VEC2_NORMALIZE_EPSILON: f32 = f32::from_bits(0x38D1_B717);

/// `AABB.EPSILON`: `1.0e-7` (`0x3E7AD7F29ABCAF48`).
pub const AABB_EPSILON: f64 = f64::from_bits(0x3E7A_D7F2_9ABC_AF48);

/// `LpVec3.ABS_MAX_VALUE`: `2^34 - 1 = 17179869183.0` (`0x420FFFFFFFF80000`).
pub const LP_VEC3_ABS_MAX_VALUE: f64 = f64::from_bits(0x420F_FFFF_FFF8_0000);

/// `LpVec3.ABS_MIN_VALUE`: `1.0 / 32766.0 = 3.051944088384301e-5` (`0x3F00004001000400`).
pub const LP_VEC3_ABS_MIN_VALUE: f64 = 1.0_f64 / 32_766.0_f64;

/// Cardinal coordinate axis (`X`, `Y`, `Z`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Axis {
    /// X axis.
    X,
    /// Y axis.
    Y,
    /// Z axis.
    Z,
}

/// Six axis-aligned 3D face directions matching vanilla's `Direction` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Negative Y (`-Y`).
    Down,
    /// Positive Y (`+Y`).
    Up,
    /// Negative Z (`-Z`).
    North,
    /// Positive Z (`+Z`).
    South,
    /// Negative X (`-X`).
    West,
    /// Positive X (`+X`).
    East,
}

impl Direction {
    /// Returns the horizontal 2D data index (`South = 0, West = 1, North = 2, East = 3`), or
    /// `None` for vertical directions (`Down`, `Up`).
    #[inline]
    pub const fn data_2d(self) -> Option<i32> {
        match self {
            Self::South => Some(0),
            Self::West => Some(1),
            Self::North => Some(2),
            Self::East => Some(3),
            Self::Down | Self::Up => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Vec3i / BlockPos
// ---------------------------------------------------------------------------

/// Signed 32-bit integer 3D vector (`Vec3i` / `BlockPos`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Vec3i {
    /// X component.
    pub x: i32,
    /// Y component.
    pub y: i32,
    /// Z component.
    pub z: i32,
}

impl Vec3i {
    /// Origin `(0, 0, 0)`.
    pub const ZERO: Self = Self { x: 0, y: 0, z: 0 };

    /// Constructs a `Vec3i`.
    #[inline]
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// `Vec3i.hashCode()`: `((y + z * 31) * 31) + x` (wrapping 32-bit).
    #[inline]
    pub const fn hash_code(self) -> i32 {
        vec3i_hash_code(self.x, self.y, self.z)
    }

    /// `Vec3i.compareTo(other)`: compares Y, then Z, then X via wrapping 32-bit subtraction.
    #[inline]
    pub const fn compare_to(self, other: Self) -> i32 {
        if self.y == other.y {
            if self.z == other.z {
                self.x.wrapping_sub(other.x)
            } else {
                self.z.wrapping_sub(other.z)
            }
        } else {
            self.y.wrapping_sub(other.y)
        }
    }

    /// `Vec3i.distManhattan(other)`: converts each `wrapping_abs(other.c - self.c)` to `f32`,
    /// sums left-to-right in `f32`, and truncates back to `i32`.
    #[inline]
    pub fn dist_manhattan(self, other: Self) -> i32 {
        let dx = other.x.wrapping_sub(self.x).wrapping_abs() as f32;
        let dy = other.y.wrapping_sub(self.y).wrapping_abs() as f32;
        let dz = other.z.wrapping_sub(self.z).wrapping_abs() as f32;
        ((dx + dy) + dz) as i32
    }

    /// `Vec3i.distChessboard(other)`: `max(max(|x1 - x2|, |y1 - y2|), |z1 - z2|)` in wrapping
    /// `i32`.
    #[inline]
    pub const fn dist_chessboard(self, other: Self) -> i32 {
        let dx = self.x.wrapping_sub(other.x).wrapping_abs();
        let dy = self.y.wrapping_sub(other.y).wrapping_abs();
        let dz = self.z.wrapping_sub(other.z).wrapping_abs();
        let m = if dx > dy { dx } else { dy };
        if m > dz { m } else { dz }
    }

    /// `Vec3i.distToCenterSqr(x, y, z)`: squared distance from `(this.x + 0.5, this.y + 0.5, this.z + 0.5)`.
    #[inline]
    pub fn dist_to_center_sqr(self, x: f64, y: f64, z: f64) -> f64 {
        let dx = (f64::from(self.x) + 0.5_f64) - x;
        let dy = (f64::from(self.y) + 0.5_f64) - y;
        let dz = (f64::from(self.z) + 0.5_f64) - z;
        (dx * dx + dy * dy) + dz * dz
    }

    /// `BlockPos.clampLocationWithin(Vec3)`: clamps each axis using the asymmetric `f32`/`f64`
    /// bounds `[f64(f32(c) + 1e-5f), (f64(c) + 1.0) - f64(1e-5f)]`.
    #[inline]
    pub fn clamp_location_within(self, loc: Vec3) -> Vec3 {
        Vec3::new(
            clamp_f64(
                loc.x,
                f64::from((self.x as f32) + EPSILON),
                (f64::from(self.x) + 1.0_f64) - EPSILON_F64,
            ),
            clamp_f64(
                loc.y,
                f64::from((self.y as f32) + EPSILON),
                (f64::from(self.y) + 1.0_f64) - EPSILON_F64,
            ),
            clamp_f64(
                loc.z,
                f64::from((self.z as f32) + EPSILON),
                (f64::from(self.z) + 1.0_f64) - EPSILON_F64,
            ),
        )
    }

    /// Packs this position into a 64-bit `BlockPos` long.
    #[inline]
    pub const fn as_long(self) -> i64 {
        block_as_long(self.x, self.y, self.z)
    }

    /// Unpacks a 64-bit `BlockPos` long into a `Vec3i`.
    #[inline]
    pub const fn from_long(packed: i64) -> Self {
        Self {
            x: block_get_x(packed),
            y: block_get_y(packed),
            z: block_get_z(packed),
        }
    }

    /// Wrapping component-wise offset.
    #[inline]
    pub const fn offset(self, dx: i32, dy: i32, dz: i32) -> Self {
        Self {
            x: self.x.wrapping_add(dx),
            y: self.y.wrapping_add(dy),
            z: self.z.wrapping_add(dz),
        }
    }
}

// ---------------------------------------------------------------------------
// Vec2 (f32, f32)
// ---------------------------------------------------------------------------

/// Single-precision 2D vector (`Vec2`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec2 {
    /// X component.
    pub x: f32,
    /// Y component.
    pub y: f32,
}

impl Vec2 {
    /// Zero vector `(0.0, 0.0)`.
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    /// Constructs a `Vec2`.
    #[inline]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// `Vec2.scale(float)`.
    #[inline]
    pub fn scale(self, factor: f32) -> Self {
        Self::new(self.x * factor, self.y * factor)
    }

    /// `Vec2.dot(Vec2)`.
    #[inline]
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y
    }

    /// `Vec2.add(Vec2)`.
    #[inline]
    #[allow(clippy::should_implement_trait)]
    pub fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y)
    }

    /// `Vec2.lengthSquared()`.
    #[inline]
    pub fn length_squared(self) -> f32 {
        self.x * self.x + self.y * self.y
    }

    /// `Vec2.length()`: `f32(sqrt(f64(x^2 + y^2)))`.
    #[inline]
    pub fn length(self) -> f32 {
        f64::from(self.length_squared()).sqrt() as f32
    }

    /// `Vec2.distanceToSqr(Vec2)`.
    #[inline]
    pub fn distance_to_sqr(self, other: Self) -> f32 {
        let dx = other.x - self.x;
        let dy = other.y - self.y;
        dx * dx + dy * dy
    }

    /// `Vec2.negated()`.
    #[inline]
    pub fn negated(self) -> Self {
        Self::new(-self.x, -self.y)
    }

    /// `Vec2.normalized()`: returns `ZERO` if `length < 1.0E-4F`, else `(x / len, y / len)`.
    #[inline]
    pub fn normalized(self) -> Self {
        let len = self.length();
        if len < VEC2_NORMALIZE_EPSILON {
            Self::ZERO
        } else {
            Self::new(self.x / len, self.y / len)
        }
    }

    /// `Vec2.rotate(double)`: rotates using lookup-table `cos`/`sin`, returning
    /// `(x * c - y * s, y * c + x * s)` in `f32`.
    #[inline]
    pub fn rotate(self, radians: f64) -> Self {
        let c = cos(radians);
        let s = sin(radians);
        Self::new(self.x * c - self.y * s, self.y * c + self.x * s)
    }

    /// `Vec2.equals(Vec2)`: uses IEEE-754 `==` (`-0.0 == +0.0`, `NaN != NaN`).
    #[inline]
    pub fn java_equals(self, other: Self) -> bool {
        self.x == other.x && self.y == other.y
    }
}

// ---------------------------------------------------------------------------
// Vec3 (f64, f64, f64)
// ---------------------------------------------------------------------------

/// Double-precision 3D vector (`Vec3`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    /// X component.
    pub x: f64,
    /// Y component.
    pub y: f64,
    /// Z component.
    pub z: f64,
}

impl Vec3 {
    /// Zero vector `(0.0, 0.0, 0.0)`.
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    /// Constructs a `Vec3`.
    #[inline]
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// `Vec3.add(x, y, z)`.
    #[inline]
    pub fn add_coords(self, x: f64, y: f64, z: f64) -> Self {
        Self::new(self.x + x, self.y + y, self.z + z)
    }

    /// `Vec3.add(Vec3)`.
    #[inline]
    #[allow(clippy::should_implement_trait)]
    pub fn add(self, other: Self) -> Self {
        self.add_coords(other.x, other.y, other.z)
    }

    /// `Vec3.subtract(x, y, z)`: delegates to `add_coords(-x, -y, -z)`.
    #[inline]
    pub fn subtract_coords(self, x: f64, y: f64, z: f64) -> Self {
        self.add_coords(-x, -y, -z)
    }

    /// `Vec3.subtract(Vec3)`: delegates to `subtract_coords(other.x, other.y, other.z)`.
    #[inline]
    pub fn subtract(self, other: Self) -> Self {
        self.subtract_coords(other.x, other.y, other.z)
    }

    /// `Vec3.scale(double)`.
    #[inline]
    pub fn scale(self, factor: f64) -> Self {
        Self::new(self.x * factor, self.y * factor, self.z * factor)
    }

    /// `Vec3.reverse()`: delegates to `scale(-1.0)` (turning `+0.0` into `-0.0`).
    #[inline]
    pub fn reverse(self) -> Self {
        self.scale(-1.0_f64)
    }

    /// `Vec3.dot(Vec3)`: `(x1 * x2 + y1 * y2) + z1 * z2`.
    #[inline]
    pub fn dot(self, other: Self) -> f64 {
        (self.x * other.x + self.y * other.y) + self.z * other.z
    }

    /// `Vec3.cross(Vec3)`.
    #[inline]
    pub fn cross(self, other: Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    /// `Vec3.lengthSqr()`: `(x^2 + y^2) + z^2`.
    #[inline]
    pub fn length_sqr(self) -> f64 {
        (self.x * self.x + self.y * self.y) + self.z * self.z
    }

    /// `Vec3.length()`: `sqrt((x^2 + y^2) + z^2)`.
    #[inline]
    pub fn length(self) -> f64 {
        self.length_sqr().sqrt()
    }

    /// `Vec3.distanceToSqr(Vec3)`.
    #[inline]
    pub fn distance_to_sqr(self, other: Self) -> f64 {
        let dx = other.x - self.x;
        let dy = other.y - self.y;
        let dz = other.z - self.z;
        (dx * dx + dy * dy) + dz * dz
    }

    /// `Vec3.distanceTo(Vec3)`.
    #[inline]
    pub fn distance_to(self, other: Self) -> f64 {
        self.distance_to_sqr(other).sqrt()
    }

    /// `Vec3.normalize()`: returns `ZERO` if `length < f64(1.0E-5F)`, else `(x/d, y/d, z/d)`.
    ///
    /// On `NaN`, the threshold test is false and `(NaN, NaN, NaN)` is returned.
    #[inline]
    pub fn normalize(self) -> Self {
        let d = self.length();
        if d < EPSILON_F64 {
            Self::ZERO
        } else {
            Self::new(self.x / d, self.y / d, self.z / d)
        }
    }

    /// `Vec3.xRot(float)`: rotates around X using lookup-table `cos`/`sin` widened to `f64`.
    #[inline]
    pub fn x_rot(self, radians: f32) -> Self {
        let c = f64::from(cos(f64::from(radians)));
        let s = f64::from(sin(f64::from(radians)));
        Self::new(self.x, self.y * c + self.z * s, self.z * c - self.y * s)
    }

    /// `Vec3.yRot(float)`: rotates around Y using lookup-table `cos`/`sin` widened to `f64`.
    #[inline]
    pub fn y_rot(self, radians: f32) -> Self {
        let c = f64::from(cos(f64::from(radians)));
        let s = f64::from(sin(f64::from(radians)));
        Self::new(self.x * c + self.z * s, self.y, self.z * c - self.x * s)
    }

    /// `Vec3.zRot(float)`: rotates around Z using lookup-table `cos`/`sin` widened to `f64`.
    #[inline]
    pub fn z_rot(self, radians: f32) -> Self {
        let c = f64::from(cos(f64::from(radians)));
        let s = f64::from(sin(f64::from(radians)));
        Self::new(self.x * c + self.y * s, self.y * c - self.x * s, self.z)
    }

    /// `Vec3.directionFromRotation(rotX, rotY)` (pitch, yaw in degrees).
    #[inline]
    pub fn direction_from_rotation(rot_x: f32, rot_y: f32) -> Self {
        let theta_y = (-rot_y) * DEG_TO_RAD - PI;
        let theta_x = (-rot_x) * DEG_TO_RAD;
        let cy = cos(f64::from(theta_y));
        let sy = sin(f64::from(theta_y));
        let cx = -cos(f64::from(theta_x));
        let sx = sin(f64::from(theta_x));
        Self::new(f64::from(sy * cx), f64::from(sx), f64::from(cy * cx))
    }

    /// `Vec3.rotation() -> Vec2(pitch, yaw)`: uses JVM `Math.atan2` and `Math.asin` narrowed to
    /// `f32` before multiplying by `Mth.RAD_TO_DEG`.
    #[inline]
    pub fn rotation(self) -> Vec2 {
        let yaw = (java_math_atan2(-self.x, self.z) as f32) * RAD_TO_DEG;
        let pitch = (java_math_asin(-self.y / self.length()) as f32) * RAD_TO_DEG;
        Vec2::new(pitch, yaw)
    }

    /// `Vec3.projectedOn(onto)`: returns `onto` if `onto.length_sqr() == 0.0`, otherwise performs
    /// two separate scalings `onto.scale(self.dot(onto)).scale(1.0 / len_sqr)`.
    #[inline]
    pub fn projected_on(self, onto: Self) -> Self {
        let len_sqr = onto.length_sqr();
        if len_sqr == 0.0_f64 {
            onto
        } else {
            onto.scale(self.dot(onto)).scale(1.0_f64 / len_sqr)
        }
    }

    /// `Vec3.offsetRandom(rng, offset)`: draws `(rng.next_float() - 0.5_f32) * offset` in X, Y, Z
    /// order, widens to `f64`, and adds.
    #[inline]
    pub fn offset_random<R: RandomSource>(self, rng: &mut R, offset: f32) -> Self {
        let dx = f64::from((rng.next_float() - 0.5_f32) * offset);
        let dy = f64::from((rng.next_float() - 0.5_f32) * offset);
        let dz = f64::from((rng.next_float() - 0.5_f32) * offset);
        self.add_coords(dx, dy, dz)
    }

    /// `Vec3.offsetRandomXZ(rng, offset)`: draws `(rng.next_float() - 0.5_f32) * offset` in X, Z
    /// order, widens to `f64`, and adds.
    #[inline]
    pub fn offset_random_xz<R: RandomSource>(self, rng: &mut R, offset: f32) -> Self {
        let dx = f64::from((rng.next_float() - 0.5_f32) * offset);
        let dz = f64::from((rng.next_float() - 0.5_f32) * offset);
        self.add_coords(dx, 0.0_f64, dz)
    }

    /// `Vec3.align(axes)`: replaces each selected axis with `f64(floor_f64(c))`.
    #[inline]
    pub fn align(self, axes: &[Axis]) -> Self {
        let mut x = self.x;
        let mut y = self.y;
        let mut z = self.z;
        for &axis in axes {
            match axis {
                Axis::X => x = f64::from(floor_f64(x)),
                Axis::Y => y = f64::from(floor_f64(y)),
                Axis::Z => z = f64::from(floor_f64(z)),
            }
        }
        Self::new(x, y, z)
    }

    /// Component-wise `f64` linear interpolation (`Vec3.lerp` / `Mth.lerp`).
    #[inline]
    pub fn lerp(self, to: Self, alpha: f64) -> Self {
        Self::new(
            lerp_f64(alpha, self.x, to.x),
            lerp_f64(alpha, self.y, to.y),
            lerp_f64(alpha, self.z, to.z),
        )
    }

    /// `Vec3.equals`: uses `Double.compare` (`-0.0 != +0.0`, `NaN == NaN`).
    #[inline]
    pub fn java_equals(self, other: Self) -> bool {
        double_compare_eq(self.x, other.x)
            && double_compare_eq(self.y, other.y)
            && double_compare_eq(self.z, other.z)
    }

    /// `Vec3.hashCode`: uses `Double.doubleToLongBits` (canonicalizing `NaN`) with multiplier 31.
    #[inline]
    pub fn java_hash_code(self) -> i32 {
        let lx = double_to_long_bits(self.x);
        let mut h = (lx ^ (lx >> 32)) as i32;
        let ly = double_to_long_bits(self.y);
        h = h.wrapping_mul(31).wrapping_add((ly ^ (ly >> 32)) as i32);
        let lz = double_to_long_bits(self.z);
        h.wrapping_mul(31).wrapping_add((lz ^ (lz >> 32)) as i32)
    }
}

#[inline]
fn double_to_long_bits(v: f64) -> u64 {
    if v.is_nan() {
        0x7FF8_0000_0000_0000
    } else {
        v.to_bits()
    }
}

#[inline]
fn double_compare_eq(a: f64, b: f64) -> bool {
    if a.is_nan() && b.is_nan() {
        true
    } else {
        a.to_bits() == b.to_bits()
    }
}

// ---------------------------------------------------------------------------
// LpVec3 network quantization
// ---------------------------------------------------------------------------

/// Error returned when decoding a malformed [`lp_vec3_decode`] buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LpVec3DecodeError {
    /// Input buffer ended before all required bytes were read.
    UnexpectedEof,
    /// Trailing scale VarInt exceeded 5 bytes.
    VarIntTooLong,
}

impl fmt::Display for LpVec3DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEof => write!(f, "unexpected end of LpVec3 buffer"),
            Self::VarIntTooLong => write!(f, "LpVec3 scale VarInt exceeded 5 bytes"),
        }
    }
}

impl std::error::Error for LpVec3DecodeError {}

#[inline]
fn lp_sanitize(v: f64) -> f64 {
    if v.is_nan() {
        0.0_f64
    } else {
        java_clamp_f64(v, -LP_VEC3_ABS_MAX_VALUE, LP_VEC3_ABS_MAX_VALUE)
    }
}

/// Encodes a [`Vec3`] into the compact `LpVec3` network format (1 byte for near-zero, 6 bytes
/// for scale `<= 3`, or 6 + VarInt bytes for larger scales).
pub fn lp_vec3_encode(v: Vec3) -> Vec<u8> {
    let x = lp_sanitize(v.x);
    let y = lp_sanitize(v.y);
    let z = lp_sanitize(v.z);
    let max_coord = abs_max_f64(x, abs_max_f64(y, z));
    if max_coord < LP_VEC3_ABS_MIN_VALUE {
        return vec![0_u8];
    }

    let s = ceil_long(max_coord);
    let partial = (s & 3) != s;
    let markers = (s & 3) | if partial { 4 } else { 0 };
    let sf = s as f64;
    let qx = java_round_f64(((x / sf) * 0.5_f64 + 0.5_f64) * 32_766.0_f64) as u64;
    let qy = java_round_f64(((y / sf) * 0.5_f64 + 0.5_f64) * 32_766.0_f64) as u64;
    let qz = java_round_f64(((z / sf) * 0.5_f64 + 0.5_f64) * 32_766.0_f64) as u64;

    let buf: u64 = (markers as u64) | (qx << 3) | (qy << 18) | (qz << 33);

    let mut out = Vec::with_capacity(if partial { 11 } else { 6 });
    out.push(buf as u8);
    out.push((buf >> 8) as u8);
    out.extend_from_slice(&((buf >> 16) as u32).to_be_bytes());

    if partial {
        let mut var = (s >> 2) as u32;
        loop {
            if (var & !0x7F) == 0 {
                out.push(var as u8);
                break;
            }
            out.push(((var & 0x7F) | 0x80) as u8);
            var >>= 7;
        }
    }

    out
}

/// Decodes a [`Vec3`] from an `LpVec3` byte slice, returning `(vec, bytes_consumed)`.
pub fn lp_vec3_decode(bytes: &[u8]) -> Result<(Vec3, usize), LpVec3DecodeError> {
    let &b0 = bytes.first().ok_or(LpVec3DecodeError::UnexpectedEof)?;
    if b0 == 0 {
        return Ok((Vec3::ZERO, 1));
    }
    if bytes.len() < 6 {
        return Err(LpVec3DecodeError::UnexpectedEof);
    }

    let b1 = bytes[1];
    let w = u32::from_be_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]);
    let buf: u64 = (u64::from(w) << 16) | (u64::from(b1) << 8) | u64::from(b0);

    let mut offset = 6usize;
    let mut s = u64::from(b0 & 3);
    if (b0 & 4) != 0 {
        let mut var: u32 = 0;
        let mut shift = 0u32;
        loop {
            if shift >= 35 {
                return Err(LpVec3DecodeError::VarIntTooLong);
            }
            let &byte = bytes.get(offset).ok_or(LpVec3DecodeError::UnexpectedEof)?;
            offset += 1;
            var |= u32::from(byte & 0x7F) << shift;
            if (byte & 0x80) == 0 {
                break;
            }
            shift += 7;
        }
        s |= u64::from(var) << 2;
    }

    let sf = s as f64;
    let unpack = |shift: u32| -> f64 {
        let q = ((buf >> shift) & 0x7FFF) as f64;
        (java_min_f64(q, 32_766.0_f64) * 2.0_f64 / 32_766.0_f64 - 1.0_f64) * sf
    };

    Ok((Vec3::new(unpack(3), unpack(18), unpack(33)), offset))
}

// ---------------------------------------------------------------------------
// AABB and ray tests
// ---------------------------------------------------------------------------

/// Axis-aligned bounding box (`AABB`) in `f64` with canonicalizing constructors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    /// Minimum X coordinate.
    pub min_x: f64,
    /// Minimum Y coordinate.
    pub min_y: f64,
    /// Minimum Z coordinate.
    pub min_z: f64,
    /// Maximum X coordinate.
    pub max_x: f64,
    /// Maximum Y coordinate.
    pub max_y: f64,
    /// Maximum Z coordinate.
    pub max_z: f64,
}

impl Aabb {
    /// Canonicalizing constructor: sets `min_c = java_min_f64(a_c, b_c)` and
    /// `max_c = java_max_f64(a_c, b_c)` per axis.
    #[inline]
    pub fn new(x1: f64, y1: f64, z1: f64, x2: f64, y2: f64, z2: f64) -> Self {
        Self {
            min_x: java_min_f64(x1, x2),
            min_y: java_min_f64(y1, y2),
            min_z: java_min_f64(z1, z2),
            max_x: java_max_f64(x1, x2),
            max_y: java_max_f64(y1, y2),
            max_z: java_max_f64(z1, z2),
        }
    }

    /// `new AABB(Vec3, Vec3)`.
    #[inline]
    pub fn from_vecs(a: Vec3, b: Vec3) -> Self {
        Self::new(a.x, a.y, a.z, b.x, b.y, b.z)
    }

    /// `new AABB(BlockPos)`: adds `+ 1` in wrapping 32-bit `i32` before widening to `f64`.
    #[inline]
    pub fn from_block_pos(pos: Vec3i) -> Self {
        Self::new(
            f64::from(pos.x),
            f64::from(pos.y),
            f64::from(pos.z),
            f64::from(pos.x.wrapping_add(1)),
            f64::from(pos.y.wrapping_add(1)),
            f64::from(pos.z.wrapping_add(1)),
        )
    }

    /// `AABB.of(BoundingBox)`: adds `+ 1` to the max coordinates in wrapping 32-bit `i32` before
    /// widening to `f64`.
    #[inline]
    pub fn of_bounding_box(
        min_x: i32,
        min_y: i32,
        min_z: i32,
        max_x: i32,
        max_y: i32,
        max_z: i32,
    ) -> Self {
        Self::new(
            f64::from(min_x),
            f64::from(min_y),
            f64::from(min_z),
            f64::from(max_x.wrapping_add(1)),
            f64::from(max_y.wrapping_add(1)),
            f64::from(max_z.wrapping_add(1)),
        )
    }

    /// `AABB.encapsulatingFullBlocks(a, b)`: adds `+ 1` to the per-axis max integer coordinate
    /// in wrapping 32-bit `i32` before widening to `f64`.
    #[inline]
    pub fn encapsulating_full_blocks(a: Vec3i, b: Vec3i) -> Self {
        Self::new(
            f64::from(a.x.min(b.x)),
            f64::from(a.y.min(b.y)),
            f64::from(a.z.min(b.z)),
            f64::from(a.x.max(b.x).wrapping_add(1)),
            f64::from(a.y.max(b.y).wrapping_add(1)),
            f64::from(a.z.max(b.z).wrapping_add(1)),
        )
    }

    /// `AABB.ofSize(center, dx, dy, dz)`.
    #[inline]
    pub fn of_size(center: Vec3, dx: f64, dy: f64, dz: f64) -> Self {
        Self::new(
            center.x - dx / 2.0_f64,
            center.y - dy / 2.0_f64,
            center.z - dz / 2.0_f64,
            center.x + dx / 2.0_f64,
            center.y + dy / 2.0_f64,
            center.z + dz / 2.0_f64,
        )
    }

    /// `AABB.setMinX`.
    #[inline]
    pub fn set_min_x(self, min_x: f64) -> Self {
        Self::new(
            min_x, self.min_y, self.min_z, self.max_x, self.max_y, self.max_z,
        )
    }

    /// `AABB.setMinY`.
    #[inline]
    pub fn set_min_y(self, min_y: f64) -> Self {
        Self::new(
            self.min_x, min_y, self.min_z, self.max_x, self.max_y, self.max_z,
        )
    }

    /// `AABB.setMinZ`.
    #[inline]
    pub fn set_min_z(self, min_z: f64) -> Self {
        Self::new(
            self.min_x, self.min_y, min_z, self.max_x, self.max_y, self.max_z,
        )
    }

    /// `AABB.setMaxX`.
    #[inline]
    pub fn set_max_x(self, max_x: f64) -> Self {
        Self::new(
            self.min_x, self.min_y, self.min_z, max_x, self.max_y, self.max_z,
        )
    }

    /// `AABB.setMaxY`.
    #[inline]
    pub fn set_max_y(self, max_y: f64) -> Self {
        Self::new(
            self.min_x, self.min_y, self.min_z, self.max_x, max_y, self.max_z,
        )
    }

    /// `AABB.setMaxZ`.
    #[inline]
    pub fn set_max_z(self, max_z: f64) -> Self {
        Self::new(
            self.min_x, self.min_y, self.min_z, self.max_x, self.max_y, max_z,
        )
    }

    /// `AABB.contract(dx, dy, dz)`: shrinks along each axis and re-canonicalizes via [`Self::new`].
    #[inline]
    pub fn contract(self, dx: f64, dy: f64, dz: f64) -> Self {
        let mut min_x = self.min_x;
        let mut min_y = self.min_y;
        let mut min_z = self.min_z;
        let mut max_x = self.max_x;
        let mut max_y = self.max_y;
        let mut max_z = self.max_z;
        if dx < 0.0 {
            min_x -= dx;
        } else if dx > 0.0 {
            max_x -= dx;
        }
        if dy < 0.0 {
            min_y -= dy;
        } else if dy > 0.0 {
            max_y -= dy;
        }
        if dz < 0.0 {
            min_z -= dz;
        } else if dz > 0.0 {
            max_z -= dz;
        }
        Self::new(min_x, min_y, min_z, max_x, max_y, max_z)
    }

    /// `AABB.expandTowards(dx, dy, dz)`: expands along each signed direction and re-canonicalizes.
    #[inline]
    pub fn expand_towards(self, dx: f64, dy: f64, dz: f64) -> Self {
        let mut min_x = self.min_x;
        let mut min_y = self.min_y;
        let mut min_z = self.min_z;
        let mut max_x = self.max_x;
        let mut max_y = self.max_y;
        let mut max_z = self.max_z;
        if dx < 0.0 {
            min_x += dx;
        } else if dx > 0.0 {
            max_x += dx;
        }
        if dy < 0.0 {
            min_y += dy;
        } else if dy > 0.0 {
            max_y += dy;
        }
        if dz < 0.0 {
            min_z += dz;
        } else if dz > 0.0 {
            max_z += dz;
        }
        Self::new(min_x, min_y, min_z, max_x, max_y, max_z)
    }

    /// `AABB.inflate(dx, dy, dz)`.
    #[inline]
    pub fn inflate(self, dx: f64, dy: f64, dz: f64) -> Self {
        Self::new(
            self.min_x - dx,
            self.min_y - dy,
            self.min_z - dz,
            self.max_x + dx,
            self.max_y + dy,
            self.max_z + dz,
        )
    }

    /// `AABB.deflate(dx, dy, dz)`: delegates to `inflate(-dx, -dy, -dz)`.
    #[inline]
    pub fn deflate(self, dx: f64, dy: f64, dz: f64) -> Self {
        self.inflate(-dx, -dy, -dz)
    }

    /// `AABB.intersect(other)`: passes `(max(min1, min2), min(max1, max2))` to the canonicalizing
    /// constructor (for disjoint boxes, this swaps min and max and returns the gap box).
    #[inline]
    pub fn intersect(self, other: Self) -> Self {
        Self::new(
            java_max_f64(self.min_x, other.min_x),
            java_max_f64(self.min_y, other.min_y),
            java_max_f64(self.min_z, other.min_z),
            java_min_f64(self.max_x, other.max_x),
            java_min_f64(self.max_y, other.max_y),
            java_min_f64(self.max_z, other.max_z),
        )
    }

    /// `AABB.minmax(other)`: union bounding box.
    #[inline]
    pub fn minmax(self, other: Self) -> Self {
        Self::new(
            java_min_f64(self.min_x, other.min_x),
            java_min_f64(self.min_y, other.min_y),
            java_min_f64(self.min_z, other.min_z),
            java_max_f64(self.max_x, other.max_x),
            java_max_f64(self.max_y, other.max_y),
            java_max_f64(self.max_z, other.max_z),
        )
    }

    /// `AABB.move(dx, dy, dz)`.
    #[inline]
    pub fn move_by(self, dx: f64, dy: f64, dz: f64) -> Self {
        Self::new(
            self.min_x + dx,
            self.min_y + dy,
            self.min_z + dz,
            self.max_x + dx,
            self.max_y + dy,
            self.max_z + dz,
        )
    }

    /// `AABB.move(BlockPos)`.
    #[inline]
    pub fn move_by_vec3i(self, pos: Vec3i) -> Self {
        self.move_by(f64::from(pos.x), f64::from(pos.y), f64::from(pos.z))
    }

    /// `AABB.nextDeflated()`: passes `(nextUp(min), nextDown(max))` to the canonicalizing
    /// constructor (for a zero-thickness axis `min == max`, swaps to `[nextDown, nextUp]`).
    #[inline]
    pub fn next_deflated(self) -> Self {
        Self::new(
            self.min_x.next_up(),
            self.min_y.next_up(),
            self.min_z.next_up(),
            self.max_x.next_down(),
            self.max_y.next_down(),
            self.max_z.next_down(),
        )
    }

    /// `AABB.intersects(other)`: strict inequalities on all 3 axes.
    #[inline]
    pub fn intersects(self, other: Self) -> bool {
        self.intersects_coords(
            other.min_x,
            other.min_y,
            other.min_z,
            other.max_x,
            other.max_y,
            other.max_z,
        )
    }

    /// `AABB.intersects(minX, minY, minZ, maxX, maxY, maxZ)`: strict inequalities on all 3 axes.
    #[inline]
    pub fn intersects_coords(
        self,
        min_x: f64,
        min_y: f64,
        min_z: f64,
        max_x: f64,
        max_y: f64,
        max_z: f64,
    ) -> bool {
        self.min_x < max_x
            && self.max_x > min_x
            && self.min_y < max_y
            && self.max_y > min_y
            && self.min_z < max_z
            && self.max_z > min_z
    }

    /// `AABB.contains(x, y, z)`: half-open `[min_c, max_c)` on all 3 axes.
    #[inline]
    pub fn contains(self, x: f64, y: f64, z: f64) -> bool {
        x >= self.min_x
            && x < self.max_x
            && y >= self.min_y
            && y < self.max_y
            && z >= self.min_z
            && z < self.max_z
    }

    /// `AABB.contains(Vec3)`: half-open `[min_c, max_c)` on all 3 axes.
    #[inline]
    pub fn contains_vec(self, v: Vec3) -> bool {
        self.contains(v.x, v.y, v.z)
    }

    /// `AABB.getXsize()`.
    #[inline]
    pub fn get_xsize(self) -> f64 {
        self.max_x - self.min_x
    }

    /// `AABB.getYsize()`.
    #[inline]
    pub fn get_ysize(self) -> f64 {
        self.max_y - self.min_y
    }

    /// `AABB.getZsize()`.
    #[inline]
    pub fn get_zsize(self) -> f64 {
        self.max_z - self.min_z
    }

    /// `AABB.getSize()`: `((maxX - minX) + (maxY - minY) + (maxZ - minZ)) / 3.0`.
    #[inline]
    pub fn get_size(self) -> f64 {
        ((self.max_x - self.min_x) + (self.max_y - self.min_y) + (self.max_z - self.min_z))
            / 3.0_f64
    }

    /// `AABB.getCenter()`: uses `Mth.lerp(0.5, min, max)` per axis.
    #[inline]
    pub fn get_center(self) -> Vec3 {
        Vec3::new(
            lerp_f64(0.5_f64, self.min_x, self.max_x),
            lerp_f64(0.5_f64, self.min_y, self.max_y),
            lerp_f64(0.5_f64, self.min_z, self.max_z),
        )
    }

    /// `AABB.getBottomCenter()`: `(lerp(0.5, minX, maxX), minY, lerp(0.5, minZ, maxZ))`.
    #[inline]
    pub fn get_bottom_center(self) -> Vec3 {
        Vec3::new(
            lerp_f64(0.5_f64, self.min_x, self.max_x),
            self.min_y,
            lerp_f64(0.5_f64, self.min_z, self.max_z),
        )
    }

    /// `AABB.distanceToSqr(Vec3)`.
    #[inline]
    pub fn distance_to_sqr_point(self, p: Vec3) -> f64 {
        let dx = java_max_f64(java_max_f64(self.min_x - p.x, p.x - self.max_x), 0.0_f64);
        let dy = java_max_f64(java_max_f64(self.min_y - p.y, p.y - self.max_y), 0.0_f64);
        let dz = java_max_f64(java_max_f64(self.min_z - p.z, p.z - self.max_z), 0.0_f64);
        (dx * dx + dy * dy) + dz * dz
    }

    /// `AABB.distanceToSqr(AABB)`.
    #[inline]
    pub fn distance_to_sqr_aabb(self, other: Self) -> f64 {
        let dx = java_max_f64(
            java_max_f64(self.min_x - other.max_x, other.min_x - self.max_x),
            0.0_f64,
        );
        let dy = java_max_f64(
            java_max_f64(self.min_y - other.max_y, other.min_y - self.max_y),
            0.0_f64,
        );
        let dz = java_max_f64(
            java_max_f64(self.min_z - other.max_z, other.min_z - self.max_z),
            0.0_f64,
        );
        (dx * dx + dy * dy) + dz * dz
    }

    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn clip_face(
        plane: f64,
        d_a: f64,
        from_a: f64,
        d_b: f64,
        from_b: f64,
        d_c: f64,
        from_c: f64,
        min_b: f64,
        max_b: f64,
        min_c: f64,
        max_c: f64,
        dir: Direction,
        s_best: &mut f64,
        hit_dir: &mut Option<Direction>,
    ) {
        let s = (plane - from_a) / d_a;
        let pb = from_b + s * d_b;
        let pc = from_c + s * d_c;
        if 0.0_f64 < s
            && s < *s_best
            && min_b - AABB_EPSILON < pb
            && pb < max_b + AABB_EPSILON
            && min_c - AABB_EPSILON < pc
            && pc < max_c + AABB_EPSILON
        {
            *s_best = s;
            *hit_dir = Some(dir);
        }
    }

    fn clip_into(
        self,
        from: Vec3,
        dx: f64,
        dy: f64,
        dz: f64,
        s_best: &mut f64,
    ) -> Option<Direction> {
        let mut hit_dir = None;
        if dx > AABB_EPSILON {
            Self::clip_face(
                self.min_x,
                dx,
                from.x,
                dy,
                from.y,
                dz,
                from.z,
                self.min_y,
                self.max_y,
                self.min_z,
                self.max_z,
                Direction::West,
                s_best,
                &mut hit_dir,
            );
        } else if dx < -AABB_EPSILON {
            Self::clip_face(
                self.max_x,
                dx,
                from.x,
                dy,
                from.y,
                dz,
                from.z,
                self.min_y,
                self.max_y,
                self.min_z,
                self.max_z,
                Direction::East,
                s_best,
                &mut hit_dir,
            );
        }

        if dy > AABB_EPSILON {
            Self::clip_face(
                self.min_y,
                dy,
                from.y,
                dz,
                from.z,
                dx,
                from.x,
                self.min_z,
                self.max_z,
                self.min_x,
                self.max_x,
                Direction::Down,
                s_best,
                &mut hit_dir,
            );
        } else if dy < -AABB_EPSILON {
            Self::clip_face(
                self.max_y,
                dy,
                from.y,
                dz,
                from.z,
                dx,
                from.x,
                self.min_z,
                self.max_z,
                self.min_x,
                self.max_x,
                Direction::Up,
                s_best,
                &mut hit_dir,
            );
        }

        if dz > AABB_EPSILON {
            Self::clip_face(
                self.min_z,
                dz,
                from.z,
                dx,
                from.x,
                dy,
                from.y,
                self.min_x,
                self.max_x,
                self.min_y,
                self.max_y,
                Direction::North,
                s_best,
                &mut hit_dir,
            );
        } else if dz < -AABB_EPSILON {
            Self::clip_face(
                self.max_z,
                dz,
                from.z,
                dx,
                from.x,
                dy,
                from.y,
                self.min_x,
                self.max_x,
                self.min_y,
                self.max_y,
                Direction::South,
                s_best,
                &mut hit_dir,
            );
        }

        hit_dir
    }

    /// `AABB.clip(from, to)` returning both the hit point and struck face [`Direction`].
    #[inline]
    pub fn clip_with_face(self, from: Vec3, to: Vec3) -> Option<(Vec3, Direction)> {
        let mut s_best = 1.0_f64;
        let dx = to.x - from.x;
        let dy = to.y - from.y;
        let dz = to.z - from.z;
        let dir = self.clip_into(from, dx, dy, dz, &mut s_best)?;
        Some((from.add_coords(s_best * dx, s_best * dy, s_best * dz), dir))
    }

    /// `AABB.clip(from, to)`: segment clipping against X, Y, Z entry faces with strict
    /// `0.0 < s < s_best` and `±1e-7` transverse face padding.
    #[inline]
    pub fn clip(self, from: Vec3, to: Vec3) -> Option<Vec3> {
        self.clip_with_face(from, to).map(|(hit, _)| hit)
    }

    /// `AABB.clip(aabbs, from, to, pos)`: clips a segment against multiple boxes offset by `pos`,
    /// sharing a single `s_best` across boxes in iteration order.
    pub fn clip_many<I>(aabbs: I, from: Vec3, to: Vec3, pos: Vec3i) -> Option<(Vec3, Direction)>
    where
        I: IntoIterator<Item = Self>,
    {
        let mut s_best = 1.0_f64;
        let mut best_dir = None;
        let dx = to.x - from.x;
        let dy = to.y - from.y;
        let dz = to.z - from.z;

        for aabb in aabbs {
            if let Some(dir) = aabb
                .move_by_vec3i(pos)
                .clip_into(from, dx, dy, dz, &mut s_best)
            {
                best_dir = Some(dir);
            }
        }

        let dir = best_dir?;
        Some((from.add_coords(s_best * dx, s_best * dy, s_best * dz), dir))
    }

    /// `AABB.collidedAlongVector(vec, aabbs)`.
    pub fn collided_along_vector<I>(self, vec: Vec3, aabbs: I) -> bool
    where
        I: IntoIterator<Item = Self>,
    {
        let from = self.get_center();
        let to = from.add(vec);
        let hx = self.get_xsize() * 0.5_f64 - AABB_EPSILON;
        let hy = self.get_ysize() * 0.5_f64 - AABB_EPSILON;
        let hz = self.get_zsize() * 0.5_f64 - AABB_EPSILON;
        for shape in aabbs {
            let inflated = shape.inflate(hx, hy, hz);
            if inflated.contains_vec(to)
                || inflated.contains_vec(from)
                || inflated.clip(from, to).is_some()
            {
                return true;
            }
        }
        false
    }
}

/// `AABB.Builder`: accumulates bounds in `f32` starting from `±inf`, widening all 6 bounds to
/// `f64` on [`AabbBuilder::build`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AabbBuilder {
    min_x: f32,
    min_y: f32,
    min_z: f32,
    max_x: f32,
    max_y: f32,
    max_z: f32,
}

impl Default for AabbBuilder {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl AabbBuilder {
    /// Creates an empty builder with `min = +inf`, `max = -inf`.
    #[inline]
    pub const fn new() -> Self {
        Self {
            min_x: f32::INFINITY,
            min_y: f32::INFINITY,
            min_z: f32::INFINITY,
            max_x: f32::NEG_INFINITY,
            max_y: f32::NEG_INFINITY,
            max_z: f32::NEG_INFINITY,
        }
    }

    /// Includes an `f32` 3D point `[x, y, z]` in the accumulated bounds.
    #[inline]
    pub fn include(&mut self, point: [f32; 3]) -> &mut Self {
        self.min_x = java_min_f32(self.min_x, point[0]);
        self.min_y = java_min_f32(self.min_y, point[1]);
        self.min_z = java_min_f32(self.min_z, point[2]);
        self.max_x = java_max_f32(self.max_x, point[0]);
        self.max_y = java_max_f32(self.max_y, point[1]);
        self.max_z = java_max_f32(self.max_z, point[2]);
        self
    }

    /// Widens the accumulated `f32` bounds to `f64` and constructs an [`Aabb`].
    #[inline]
    pub fn build(self) -> Aabb {
        Aabb::new(
            f64::from(self.min_x),
            f64::from(self.min_y),
            f64::from(self.min_z),
            f64::from(self.max_x),
            f64::from(self.max_y),
            f64::from(self.max_z),
        )
    }
}

/// `Mth.rayIntersectsAABB(start, dir, aabb)`: SAT / Woo ray-box intersection test.
///
/// Preserves the strict `<` on the Z-cross test, so any pure Z-axis ray (`dir.x == 0 && dir.y == 0`)
/// evaluates `0.0 < 0.0` and returns `false`.
#[inline]
pub fn ray_intersects_aabb(start: Vec3, dir: Vec3, aabb: Aabb) -> bool {
    let mx = (aabb.min_x + aabb.max_x) * 0.5_f64;
    let ex = (aabb.max_x - aabb.min_x) * 0.5_f64;
    let dx = start.x - mx;
    if dx.abs() > ex && dx * dir.x >= 0.0_f64 {
        return false;
    }

    let my = (aabb.min_y + aabb.max_y) * 0.5_f64;
    let ey = (aabb.max_y - aabb.min_y) * 0.5_f64;
    let dy = start.y - my;
    if dy.abs() > ey && dy * dir.y >= 0.0_f64 {
        return false;
    }

    let mz = (aabb.min_z + aabb.max_z) * 0.5_f64;
    let ez = (aabb.max_z - aabb.min_z) * 0.5_f64;
    let dz = start.z - mz;
    if dz.abs() > ez && dz * dir.z >= 0.0_f64 {
        return false;
    }

    let ax = dir.x.abs();
    let ay = dir.y.abs();
    let az = dir.z.abs();

    if (dir.y * dz - dir.z * dy).abs() > ey * az + ez * ay {
        return false;
    }
    if (dir.z * dx - dir.x * dz).abs() > ex * az + ez * ax {
        return false;
    }
    (dir.x * dy - dir.y * dx).abs() < ex * ay + ey * ax
}
