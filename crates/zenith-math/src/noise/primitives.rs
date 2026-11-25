//! Shared gradient-noise foundation, interval bounds, volume grid, and interpolation primitives.
//!
//! All single-octave gradient generators ([`crate::noise::PerlinNoise`],
//! [`crate::noise::SmearedPerlinNoise`], and [`crate::noise::SimplexNoise`]) build on the
//! shared [`GradientNoiseState`], 16-vector [`GRADIENT`] table, and IEEE-754 primitives defined
//! here.

use std::fmt;
use std::ops::{Add, Mul};

use crate::random::RandomSource;

/// Coordinate wrap period `R = 2^25 = 33,554,432.0` (`0x4180_0000_0000_0000`).
pub const WRAP_PERIOD: f64 = f64::from_bits(0x4180_0000_0000_0000);

/// Coordinate wrap half-bound `H = nextDown(2^24) = 16,777,215.999999998` (`0x416F_FFFF_FFFF_FFFF`).
pub const WRAP_HALF_BOUND: f64 = f64::from_bits(0x416F_FFFF_FFFF_FFFF);

/// Default origin offset scale `S = 256.0` (`0x4070_0000_0000_0000`).
pub const DEFAULT_OFFSET_SCALE: f64 = 256.0;

/// 16-entry gradient table `G[0..15]`.
///
/// Entries `0..12` are the 12 cube-edge vectors; entries `12..16` repeat four of them so
/// Perlin noise can mask hash indices with `& 15`, while simplex noise reduces indices `% 12`.
pub const GRADIENT: [[i8; 3]; 16] = [
    [1, 1, 0],
    [-1, 1, 0],
    [1, -1, 0],
    [-1, -1, 0],
    [1, 0, 1],
    [-1, 0, 1],
    [1, 0, -1],
    [-1, 0, -1],
    [0, 1, 1],
    [0, -1, 1],
    [0, 1, -1],
    [0, -1, -1],
    [1, 1, 0],
    [0, -1, 1],
    [-1, 1, 0],
    [0, -1, -1],
];

/// Errors returned when validating noise parameters, octave stacks, or volume grids.
#[derive(Debug, Clone, PartialEq)]
pub enum NoiseError {
    /// `base_amplitude` is NaN or outside `[f64::from(1.0e-5_f32), 1_000_000.0]`.
    InvalidBaseAmplitude(f64),
    /// `base_octave` is outside `[-32, 32]`.
    InvalidBaseOctave(i32),
    /// `octave_count` is outside `[1, 32]`.
    InvalidOctaveCount(usize),
    /// Non-empty `amplitude_modifiers` length does not match `octave_count`.
    AmplitudeModifierCountMismatch {
        /// Expected modifier count (`octave_count`).
        expected: usize,
        /// Actual modifier slice length.
        actual: usize,
    },
    /// An `amplitude_modifiers` entry is NaN or outside `[0.0, 1_000_000.0]`.
    InvalidAmplitudeModifier {
        /// Zero-based octave index.
        index: usize,
        /// Invalid modifier value.
        value: f64,
    },
    /// Legacy FBM initializer received octaves reaching above exponent `0`.
    PositiveLegacyOctave {
        /// First octave exponent.
        first_octave: i32,
        /// Number of octaves.
        octave_count: usize,
    },
    /// Legacy FBM populated slot count did not match non-zero amplitude count.
    LegacyOctaveCountMismatch {
        /// Number of populated octave slots.
        populated: usize,
        /// Number of non-zero amplitudes.
        expected: usize,
    },
    /// Smeared FBM builder received a positive `first_octave > 0`.
    PositiveSmearedFirstOctave(i32),
    /// A blended-noise parameter is NaN or outside its valid range.
    InvalidBlendedParameter {
        /// Parameter field name.
        name: &'static str,
        /// Invalid parameter value.
        value: f64,
    },
    /// Volume grid dimensions or strides are non-positive, or the total element count overflows.
    InvalidVolumeGrid,
}

impl fmt::Display for NoiseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBaseAmplitude(v) => {
                write!(f, "invalid base_amplitude {v}: out of valid range")
            }
            Self::InvalidBaseOctave(v) => {
                write!(f, "invalid base_octave {v}: expected -32..=32")
            }
            Self::InvalidOctaveCount(v) => {
                write!(f, "invalid octave_count {v}: expected 1..=32")
            }
            Self::AmplitudeModifierCountMismatch { expected, actual } => {
                write!(
                    f,
                    "amplitude_modifiers length {actual} does not match octave_count {expected}"
                )
            }
            Self::InvalidAmplitudeModifier { index, value } => {
                write!(
                    f,
                    "invalid amplitude_modifiers[{index}] = {value}: expected 0.0..=1_000_000.0"
                )
            }
            Self::PositiveLegacyOctave {
                first_octave,
                octave_count,
            } => {
                write!(
                    f,
                    "legacy FBM rejects positive octaves (first_octave={first_octave}, count={octave_count})"
                )
            }
            Self::LegacyOctaveCountMismatch {
                populated,
                expected,
            } => {
                write!(
                    f,
                    "legacy FBM populated {populated} octaves, expected {expected}"
                )
            }
            Self::PositiveSmearedFirstOctave(v) => {
                write!(f, "smeared FBM requires first_octave <= 0, got {v}")
            }
            Self::InvalidBlendedParameter { name, value } => {
                write!(f, "invalid blended noise parameter `{name}` = {value}")
            }
            Self::InvalidVolumeGrid => {
                write!(
                    f,
                    "volume grid dimensions and strides must be positive and non-overflowing"
                )
            }
        }
    }
}

impl std::error::Error for NoiseError {}

/// Conservative `f32` closed output interval `[min, max]`, or not-an-interval ([`Interval::NaI`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Interval {
    /// Closed interval `[min, max]` where neither bound is `NaN`.
    Bounds {
        /// Lower bound (inclusive).
        min: f32,
        /// Upper bound (inclusive).
        max: f32,
    },
    /// Not-an-interval, produced whenever a bound becomes `NaN`.
    NaI,
}

impl Interval {
    /// Creates a closed interval `[min, max]`, or [`Interval::NaI`] if either bound is `NaN`.
    #[inline]
    pub fn new(min: f32, max: f32) -> Self {
        if min.is_nan() || max.is_nan() {
            Self::NaI
        } else {
            Self::Bounds { min, max }
        }
    }

    /// Creates an exact point interval `[value, value]`, or [`Interval::NaI`] if `value` is `NaN`.
    #[inline]
    pub fn exact(value: f32) -> Self {
        Self::new(value, value)
    }

    /// Creates a symmetric interval `[-radius, radius]`, or [`Interval::NaI`] if `radius` is
    /// `NaN` or negative.
    #[inline]
    pub fn symmetric(radius: f32) -> Self {
        if radius.is_nan() || radius < 0.0_f32 {
            Self::NaI
        } else {
            Self::Bounds {
                min: -radius,
                max: radius,
            }
        }
    }

    /// Returns `true` if this interval is [`Interval::NaI`].
    #[inline]
    pub const fn is_nai(self) -> bool {
        matches!(self, Self::NaI)
    }

    /// Returns `Some((min, max))` for valid bounds, or `None` for [`Interval::NaI`].
    #[inline]
    pub const fn bounds(self) -> Option<(f32, f32)> {
        match self {
            Self::Bounds { min, max } => Some((min, max)),
            Self::NaI => None,
        }
    }

    /// Returns the lower bound, or `f32::NAN` for [`Interval::NaI`].
    #[inline]
    pub const fn min(self) -> f32 {
        match self {
            Self::Bounds { min, .. } => min,
            Self::NaI => f32::NAN,
        }
    }

    /// Returns the upper bound, or `f32::NAN` for [`Interval::NaI`].
    #[inline]
    pub const fn max(self) -> f32 {
        match self {
            Self::Bounds { max, .. } => max,
            Self::NaI => f32::NAN,
        }
    }

    /// Interval addition in `f32`: `[a1, b1] + [a2, b2] = [a1 + a2, b1 + b2]`.
    #[inline]
    pub fn add_interval(self, rhs: Self) -> Self {
        match (self, rhs) {
            (Self::Bounds { min: a1, max: b1 }, Self::Bounds { min: a2, max: b2 }) => {
                Self::new(a1 + a2, b1 + b2)
            }
            _ => Self::NaI,
        }
    }

    /// Interval multiplication in `f32` using zero-absorbing corner products (`0 * inf = 0`).
    #[inline]
    pub fn mul_interval(self, rhs: Self) -> Self {
        match (self, rhs) {
            (Self::Bounds { min: a1, max: b1 }, Self::Bounds { min: a2, max: b2 }) => {
                let p0 = zero_absorbing_mul_f32(a1, a2);
                let p1 = zero_absorbing_mul_f32(a1, b2);
                let p2 = zero_absorbing_mul_f32(b1, a2);
                let p3 = zero_absorbing_mul_f32(b1, b2);
                let min = java_min_f32(java_min_f32(p0, p1), java_min_f32(p2, p3));
                let max = java_max_f32(java_max_f32(p0, p1), java_max_f32(p2, p3));
                Self::new(min, max)
            }
            _ => Self::NaI,
        }
    }
}

impl Add for Interval {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        self.add_interval(rhs)
    }
}

impl Mul for Interval {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: Self) -> Self::Output {
        self.mul_interval(rhs)
    }
}

#[inline]
fn zero_absorbing_mul_f32(u: f32, v: f32) -> f32 {
    if u == 0.0_f32 || v == 0.0_f32 {
        0.0_f32
    } else {
        u * v
    }
}

/// Java `Math.min(float, float)`: propagates `NaN` and orders `-0.0 < +0.0`.
#[inline]
pub(crate) fn java_min_f32(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a == 0.0_f32 && b == 0.0_f32 {
        if a.is_sign_negative() || b.is_sign_negative() {
            -0.0_f32
        } else {
            0.0_f32
        }
    } else if a < b {
        a
    } else {
        b
    }
}

/// Java `Math.max(float, float)`: propagates `NaN` and orders `-0.0 < +0.0`.
#[inline]
pub(crate) fn java_max_f32(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        f32::NAN
    } else if a == 0.0_f32 && b == 0.0_f32 {
        if a.is_sign_positive() || b.is_sign_positive() {
            0.0_f32
        } else {
            -0.0_f32
        }
    } else if a > b {
        a
    } else {
        b
    }
}

/// 3D regular block sampling grid for volume accumulation.
///
/// Flat buffer layout is Z-major, then X, then Y contiguous:
/// `index(ix, iy, iz) = iy + (ix + iz * size_x) * size_y`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VolumeGrid {
    size_x: usize,
    size_y: usize,
    size_z: usize,
    origin_x: i32,
    origin_y: i32,
    origin_z: i32,
    stride_x: i32,
    stride_y: i32,
    stride_z: i32,
}

impl VolumeGrid {
    /// Creates a new volume grid with positive dimensions `size`, integer `origin`, and positive
    /// integer `stride`.
    pub fn new(size: [usize; 3], origin: [i32; 3], stride: [i32; 3]) -> Result<Self, NoiseError> {
        let [size_x, size_y, size_z] = size;
        let [origin_x, origin_y, origin_z] = origin;
        let [stride_x, stride_y, stride_z] = stride;

        if size_x == 0
            || size_y == 0
            || size_z == 0
            || stride_x <= 0
            || stride_y <= 0
            || stride_z <= 0
        {
            return Err(NoiseError::InvalidVolumeGrid);
        }

        size_x
            .checked_mul(size_y)
            .and_then(|xy| xy.checked_mul(size_z))
            .ok_or(NoiseError::InvalidVolumeGrid)?;

        Ok(Self {
            size_x,
            size_y,
            size_z,
            origin_x,
            origin_y,
            origin_z,
            stride_x,
            stride_y,
            stride_z,
        })
    }

    /// Number of samples along the X axis.
    #[inline]
    pub const fn size_x(&self) -> usize {
        self.size_x
    }

    /// Number of samples along the Y axis.
    #[inline]
    pub const fn size_y(&self) -> usize {
        self.size_y
    }

    /// Number of samples along the Z axis.
    #[inline]
    pub const fn size_z(&self) -> usize {
        self.size_z
    }

    /// Integer origin coordinate along X.
    #[inline]
    pub const fn origin_x(&self) -> i32 {
        self.origin_x
    }

    /// Integer origin coordinate along Y.
    #[inline]
    pub const fn origin_y(&self) -> i32 {
        self.origin_y
    }

    /// Integer origin coordinate along Z.
    #[inline]
    pub const fn origin_z(&self) -> i32 {
        self.origin_z
    }

    /// Positive integer stride along X.
    #[inline]
    pub const fn stride_x(&self) -> i32 {
        self.stride_x
    }

    /// Positive integer stride along Y.
    #[inline]
    pub const fn stride_y(&self) -> i32 {
        self.stride_y
    }

    /// Positive integer stride along Z.
    #[inline]
    pub const fn stride_z(&self) -> i32 {
        self.stride_z
    }

    /// Total number of voxels `size_x * size_y * size_z`.
    #[inline]
    pub const fn len(&self) -> usize {
        self.size_x * self.size_y * self.size_z
    }

    /// Always returns `false` because valid grids have strictly positive dimensions.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// Computes the flat buffer index `iy + (ix + iz * size_x) * size_y`.
    #[inline]
    pub const fn index(&self, ix: usize, iy: usize, iz: usize) -> usize {
        iy + (ix + iz * self.size_x) * self.size_y
    }

    /// Computes the wrapping 32-bit block X coordinate for index `ix`.
    #[inline]
    pub const fn block_x(&self, ix: usize) -> i32 {
        self.origin_x
            .wrapping_add((ix as i32).wrapping_mul(self.stride_x))
    }

    /// Computes the wrapping 32-bit block Y coordinate for index `iy`.
    #[inline]
    pub const fn block_y(&self, iy: usize) -> i32 {
        self.origin_y
            .wrapping_add((iy as i32).wrapping_mul(self.stride_y))
    }

    /// Computes the wrapping 32-bit block Z coordinate for index `iz`.
    #[inline]
    pub const fn block_z(&self, iz: usize) -> i32 {
        self.origin_z
            .wrapping_add((iz as i32).wrapping_mul(self.stride_z))
    }
}

/// Common interface for 2D/3D noise generators and octave stacks.
pub trait Noise {
    /// Samples the generator at 3D world/lattice coordinates `(x, y, z)`.
    fn sample(&self, x: f64, y: f64, z: f64) -> f32;

    /// Samples the generator at 2D coordinates `(x, y)`.
    fn sample_2d(&self, x: f64, y: f64) -> f32;

    /// Returns the conservative `f32` output interval for this generator.
    fn range(&self) -> Interval;

    /// Accumulates `amplitude * sample(x * scale_xz, y * scale_y, z * scale_xz)` into `buffer`
    /// over `grid` in `(z, x, y)` order.
    ///
    /// # Panics
    ///
    /// Panics if `buffer.len() < grid.len()`.
    fn accumulate_volume(
        &self,
        grid: &VolumeGrid,
        scale_xz: f64,
        scale_y: f64,
        amplitude: f32,
        buffer: &mut [f32],
    ) {
        assert!(
            buffer.len() >= grid.len(),
            "volume buffer length {} is smaller than grid size {}",
            buffer.len(),
            grid.len()
        );
        let mut idx = 0;
        for iz in 0..grid.size_z() {
            let z = f64::from(grid.block_z(iz)) * scale_xz;
            for ix in 0..grid.size_x() {
                let x = f64::from(grid.block_x(ix)) * scale_xz;
                for iy in 0..grid.size_y() {
                    let y = f64::from(grid.block_y(iy)) * scale_y;
                    buffer[idx] += amplitude * self.sample(x, y, z);
                    idx += 1;
                }
            }
        }
    }
}

/// Shared origin offsets and 256-byte permutation table for gradient-based single-octave noises.
#[derive(Debug, Clone, PartialEq)]
pub struct GradientNoiseState {
    xo: f64,
    yo: f64,
    zo: f64,
    p: [i8; 256],
}

impl GradientNoiseState {
    /// Initializes origin offsets scaled by `offset_scale` and shuffles the 256-byte permutation
    /// table using forward Fisher-Yates for `i = 0..=255`.
    ///
    /// Always draws three `next_double()` values (even when `offset_scale == 0.0`) followed by
    /// 256 `next_int_bounded(256 - i)` calls (including the final `next_int_bounded(1)`).
    pub fn new<R: RandomSource>(rng: &mut R, offset_scale: f64) -> Self {
        let xo = rng.next_double() * offset_scale;
        let yo = rng.next_double() * offset_scale;
        let zo = rng.next_double() * offset_scale;

        let mut p = [0_i8; 256];
        for (i, slot) in p.iter_mut().enumerate() {
            *slot = i as i8;
        }
        for i in 0..256 {
            let k = rng.next_int_bounded(256 - i as i32) as usize;
            p.swap(i, i + k);
        }

        Self { xo, yo, zo, p }
    }

    /// Constructs state directly from raw offsets and a permutation table (primarily for tests
    /// and diagnostics).
    pub const fn from_raw(xo: f64, yo: f64, zo: f64, p: [i8; 256]) -> Self {
        Self { xo, yo, zo, p }
    }

    /// X origin offset.
    #[inline]
    pub const fn xo(&self) -> f64 {
        self.xo
    }

    /// Y origin offset.
    #[inline]
    pub const fn yo(&self) -> f64 {
        self.yo
    }

    /// Z origin offset.
    #[inline]
    pub const fn zo(&self) -> f64 {
        self.zo
    }

    /// Reference to the signed 256-byte permutation table.
    #[inline]
    pub const fn permutation(&self) -> &[i8; 256] {
        &self.p
    }

    /// Zero-extended permutation lookup `p(n) = (P[n & 0xFF] & 0xFF) as i32` in `0..=255`.
    #[inline]
    pub const fn hash(&self, n: i32) -> i32 {
        (self.p[(n & 0xFF) as usize] as u8) as i32
    }

    /// Formats this state using [`crate::noise::parity_octave_config`].
    pub fn parity_config_string(&self) -> String {
        crate::noise::utils::parity_octave_config(self.xo, self.yo, self.zo, &self.p)
    }
}

/// Floors an `f64` and narrows to `i32` with Java saturating semantics
/// (`NaN -> 0`, `>= i32::MAX -> i32::MAX`, `<= i32::MIN -> i32::MIN`).
#[inline]
pub fn floor_to_i32(v: f64) -> i32 {
    v.floor() as i32
}

/// Wraps a coordinate into `[-2^24, 2^24)` using the asymmetric pass-through range `[-H, H)`
/// where `H = nextDown(2^24)`.
#[inline]
pub fn wrap(x: f64) -> f64 {
    if (-WRAP_HALF_BOUND..WRAP_HALF_BOUND).contains(&x) {
        x
    } else {
        x - (x / WRAP_PERIOD + 0.5).floor() * WRAP_PERIOD
    }
}

/// Perlin quintic fade curve `s(t) = ((t * t) * t) * (t * (t * 6.0 - 15.0) + 10.0)` in `f32`.
#[inline]
pub fn smoothstep(t: f32) -> f32 {
    ((t * t) * t) * (t * (t * 6.0_f32 - 15.0_f32) + 10.0_f32)
}

/// Derivative of the quintic fade curve `s'(t) = (((30.0 * t) * t) * (t - 1.0)) * (t - 1.0)` in
/// `f32`.
#[inline]
pub fn smoothstep_derivative(t: f32) -> f32 {
    (((30.0_f32 * t) * t) * (t - 1.0_f32)) * (t - 1.0_f32)
}

/// Single-precision linear interpolation `a + alpha * (b - a)` (no FMA).
#[inline]
pub fn lerp(alpha: f32, a: f32, b: f32) -> f32 {
    a + alpha * (b - a)
}

/// Single-precision bilinear interpolation.
#[inline]
pub fn lerp2(alpha1: f32, alpha2: f32, c00: f32, c10: f32, c01: f32, c11: f32) -> f32 {
    lerp(alpha2, lerp(alpha1, c00, c10), lerp(alpha1, c01, c11))
}

/// Single-precision trilinear interpolation.
#[inline]
#[allow(clippy::too_many_arguments)]
pub fn lerp3(
    alpha1: f32,
    alpha2: f32,
    alpha3: f32,
    c000: f32,
    c100: f32,
    c010: f32,
    c110: f32,
    c001: f32,
    c101: f32,
    c011: f32,
    c111: f32,
) -> f32 {
    lerp(
        alpha3,
        lerp2(alpha1, alpha2, c000, c100, c010, c110),
        lerp2(alpha1, alpha2, c001, c101, c011, c111),
    )
}

/// Evaluates `((gx * x) + (gy * y)) + (gz * z)` in `f32` for a gradient vector `g`.
#[inline]
pub fn grad_dot_f32(g: [i8; 3], x: f32, y: f32, z: f32) -> f32 {
    ((f32::from(g[0]) * x) + (f32::from(g[1]) * y)) + (f32::from(g[2]) * z)
}

/// Evaluates `((gx * x) + (gy * y)) + (gz * z)` in `f64` for a gradient vector `g`.
#[inline]
pub fn grad_dot_f64(g: [i8; 3], x: f64, y: f64, z: f64) -> f64 {
    ((f64::from(g[0]) * x) + (f64::from(g[1]) * y)) + (f64::from(g[2]) * z)
}

/// Evaluates the XZ partial dot product `(gx * x) + (gz * z)` in `f32` for column-cached volume
/// sampling.
#[inline]
pub fn grad_dot_xz_f32(g: [i8; 3], x: f32, z: f32) -> f32 {
    (f32::from(g[0]) * x) + (f32::from(g[2]) * z)
}
