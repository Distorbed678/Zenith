//! Interpolation, remapping, smoothstep, Catmull-Rom, triangle wave, all 32 easing curves,
//! cubic Bézier solver, piecewise cubic Hermite splines, and stateful animators
//! (`Mth`, `Ease`, `EasingType`, `CubicSpline`, `SmoothDouble`, `BinaryAnimator`).
//!
//! Implements Section 4 of `docs/specs/math/java-math-helpers.md`.

use std::fmt;

use crate::helpers::geom::Vec3;
use crate::helpers::intrinsics::{java_math_pow, java_math_sin};
use crate::helpers::rounding::{
    EPSILON, binary_search, clamp_f32, cube_f32, floor_f32, java_max_f32, java_min_f32,
    java_signum_f64,
};
use crate::helpers::trig::{HALF_PI, PI, cos, sin};
use crate::noise::Interval;

// ---------------------------------------------------------------------------
// Easing constants (exact IEEE-754 bit patterns from spec)
// ---------------------------------------------------------------------------

/// `Ease` elastic `C4 = f64(f32(2*pi / 3))` (`0x4000C15240000000`).
pub const ELASTIC_C4: f64 = f64::from_bits(0x4000_C152_4000_0000);

/// `Ease` elastic `C5 = f64((PI_f32 * 4.0f) / 9.0f)` (`0x3FF6571860000000`).
pub const ELASTIC_C5: f64 = f64::from_bits(0x3FF6_5718_6000_0000);

/// `Ease` back `c1 = 1.70158_f32` (`0x3FD9CD60`).
pub const BACK_C1: f32 = f32::from_bits(0x3FD9_CD60);

/// `Ease` back `c3 = 2.70158_f32` (`0x402CE6B0`).
pub const BACK_C3: f32 = f32::from_bits(0x402C_E6B0);

/// `Ease` back `c2 = 2.5949094_f32` (`0x402612FF`).
pub const BACK_C2: f32 = f32::from_bits(0x4026_12FF);

/// `Ease` back `c2 + 1 = 3.5949094_f32` (`0x406612FF`).
pub const BACK_C2_PLUS_1: f32 = f32::from_bits(0x4066_12FF);

/// `Ease` back `2*c2 + 2 = 7.189819_f32` (`0x40E612FF`).
pub const BACK_2C2_PLUS_2: f32 = f32::from_bits(0x40E6_12FF);

/// `Ease` bounce threshold 1 (`f32`): `0.36363637_f32` (`0x3EBA2E8C`).
pub const BOUNCE_THRESHOLD_1: f32 = f32::from_bits(0x3EBA_2E8C);

/// `Ease` bounce threshold 2 (`f32`): `0.72727275_f32` (`0x3F3A2E8C`).
pub const BOUNCE_THRESHOLD_2: f32 = f32::from_bits(0x3F3A_2E8C);

/// `Ease` bounce threshold 3 (`f64`): `0.9090909090909091` (`0x3FED1745D1745D17`).
pub const BOUNCE_THRESHOLD_3: f64 = f64::from_bits(0x3FED_1745_D174_5D17);

/// `Ease` bounce offset 1 (`f32`): `0.54545456_f32` (`0x3F0BA2E9`).
pub const BOUNCE_OFFSET_1: f32 = f32::from_bits(0x3F0B_A2E9);

/// `Ease` bounce offset 2 (`f32`): `0.8181818_f32` (`0x3F51745D`).
pub const BOUNCE_OFFSET_2: f32 = f32::from_bits(0x3F51_745D);

/// `Ease` bounce offset 3 (`f32`): `0.95454544_f32` (`0x3F745D17`).
pub const BOUNCE_OFFSET_3: f32 = f32::from_bits(0x3F74_5D17);

// ---------------------------------------------------------------------------
// Basic interpolation and remapping
// ---------------------------------------------------------------------------

/// `Mth.lerp(float, float, float)`: `p0 + alpha * (p1 - p0)` (no FMA).
#[inline]
pub fn lerp_f32(alpha: f32, p0: f32, p1: f32) -> f32 {
    p0 + alpha * (p1 - p0)
}

/// `Mth.lerp(double, double, double)`: `p0 + alpha * (p1 - p0)` (no FMA).
#[inline]
pub fn lerp_f64(alpha: f64, p0: f64, p1: f64) -> f64 {
    p0 + alpha * (p1 - p0)
}

/// `Mth.lerp2` (`f32`): bilinear interpolation (innermost along `alpha1`, outermost along `alpha2`).
#[inline]
pub fn lerp2_f32(alpha1: f32, alpha2: f32, c00: f32, c10: f32, c01: f32, c11: f32) -> f32 {
    lerp_f32(
        alpha2,
        lerp_f32(alpha1, c00, c10),
        lerp_f32(alpha1, c01, c11),
    )
}

/// `Mth.lerp2` (`f64`): bilinear interpolation (innermost along `alpha1`, outermost along `alpha2`).
#[inline]
pub fn lerp2_f64(alpha1: f64, alpha2: f64, c00: f64, c10: f64, c01: f64, c11: f64) -> f64 {
    lerp_f64(
        alpha2,
        lerp_f64(alpha1, c00, c10),
        lerp_f64(alpha1, c01, c11),
    )
}

/// `Mth.lerp3` (`f32`): trilinear interpolation.
#[inline]
#[allow(clippy::too_many_arguments)]
pub fn lerp3_f32(
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
    lerp_f32(
        alpha3,
        lerp2_f32(alpha1, alpha2, c000, c100, c010, c110),
        lerp2_f32(alpha1, alpha2, c001, c101, c011, c111),
    )
}

/// `Mth.lerp3` (`f64`): trilinear interpolation.
#[inline]
#[allow(clippy::too_many_arguments)]
pub fn lerp3_f64(
    alpha1: f64,
    alpha2: f64,
    alpha3: f64,
    c000: f64,
    c100: f64,
    c010: f64,
    c110: f64,
    c001: f64,
    c101: f64,
    c011: f64,
    c111: f64,
) -> f64 {
    lerp_f64(
        alpha3,
        lerp2_f64(alpha1, alpha2, c000, c100, c010, c110),
        lerp2_f64(alpha1, alpha2, c001, c101, c011, c111),
    )
}

/// `Mth.lerp(double, Vec3, Vec3)`: component-wise `f64` linear interpolation.
#[inline]
pub fn lerp_vec3(alpha: f64, a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(
        lerp_f64(alpha, a.x, b.x),
        lerp_f64(alpha, a.y, b.y),
        lerp_f64(alpha, a.z, b.z),
    )
}

/// `Mth.clampedLerp(float, float, float)`: does **not** short-circuit at `0.0` or `1.0`, and
/// propagates `NaN`.
#[inline]
pub fn clamped_lerp_f32(alpha: f32, lo: f32, hi: f32) -> f32 {
    if alpha < 0.0_f32 {
        lo
    } else if alpha > 1.0_f32 {
        hi
    } else {
        lerp_f32(alpha, lo, hi)
    }
}

/// `Mth.clampedLerp(double, double, double)`: does **not** short-circuit at `0.0` or `1.0`, and
/// propagates `NaN`.
#[inline]
pub fn clamped_lerp_f64(alpha: f64, lo: f64, hi: f64) -> f64 {
    if alpha < 0.0_f64 {
        lo
    } else if alpha > 1.0_f64 {
        hi
    } else {
        lerp_f64(alpha, lo, hi)
    }
}

/// `Mth.lerpInt(float, int, int)`: `p0 + floor(alpha * f32(p1 - p0))`.
#[inline]
pub fn lerp_int(alpha: f32, p0: i32, p1: i32) -> i32 {
    p0.wrapping_add(floor_f32(alpha * (p1.wrapping_sub(p0) as f32)))
}

/// `Mth.lerpDiscrete(float, int, int)`.
#[inline]
pub fn lerp_discrete(alpha: f32, p0: i32, p1: i32) -> i32 {
    let delta = p1.wrapping_sub(p0);
    p0.wrapping_add(floor_f32(alpha * (delta.wrapping_sub(1) as f32)))
        .wrapping_add(if alpha > 0.0_f32 { 1 } else { 0 })
}

/// `Mth.inverseLerp(float, float, float)`: `(v - lo) / (hi - lo)`.
#[inline]
pub fn inverse_lerp_f32(v: f32, lo: f32, hi: f32) -> f32 {
    (v - lo) / (hi - lo)
}

/// `Mth.inverseLerp(double, double, double)`: `(v - lo) / (hi - lo)`.
#[inline]
pub fn inverse_lerp_f64(v: f64, lo: f64, hi: f64) -> f64 {
    (v - lo) / (hi - lo)
}

/// `Mth.map(float, ...)`: `lerp_f32(inverse_lerp_f32(v, f0, f1), t0, t1)`.
#[inline]
pub fn map_f32(v: f32, f0: f32, f1: f32, t0: f32, t1: f32) -> f32 {
    lerp_f32(inverse_lerp_f32(v, f0, f1), t0, t1)
}

/// `Mth.map(double, ...)`: `lerp_f64(inverse_lerp_f64(v, f0, f1), t0, t1)`.
#[inline]
pub fn map_f64(v: f64, f0: f64, f1: f64, t0: f64, t1: f64) -> f64 {
    lerp_f64(inverse_lerp_f64(v, f0, f1), t0, t1)
}

/// `Mth.clampedMap(float, ...)`: `clamped_lerp_f32(inverse_lerp_f32(v, f0, f1), t0, t1)`.
#[inline]
pub fn clamped_map_f32(v: f32, f0: f32, f1: f32, t0: f32, t1: f32) -> f32 {
    clamped_lerp_f32(inverse_lerp_f32(v, f0, f1), t0, t1)
}

/// `Mth.clampedMap(double, ...)`: `clamped_lerp_f64(inverse_lerp_f64(v, f0, f1), t0, t1)`.
#[inline]
pub fn clamped_map_f64(v: f64, f0: f64, f1: f64, t0: f64, t1: f64) -> f64 {
    clamped_lerp_f64(inverse_lerp_f64(v, f0, f1), t0, t1)
}

// ---------------------------------------------------------------------------
// Smoothstep, Catmull-Rom, and triangle wave
// ---------------------------------------------------------------------------

/// `Mth.smoothstep(float)`: unclamped `((x * x) * x) * (x * (x * 6.0 - 15.0) + 10.0)`.
#[inline]
pub fn smoothstep(x: f32) -> f32 {
    crate::noise::smoothstep(x)
}

/// `Mth.smoothstepDerivative(float)`: unclamped `(((30.0 * x) * x) * (x - 1.0)) * (x - 1.0)`.
#[inline]
pub fn smoothstep_derivative(x: f32) -> f32 {
    crate::noise::smoothstep_derivative(x)
}

/// `Mth.catmullrom(alpha, p0, p1, p2, p3)` (`f32`, exact left-to-right parenthesization).
#[inline]
pub fn catmullrom(alpha: f32, p0: f32, p1: f32, p2: f32, p3: f32) -> f32 {
    0.5_f32
        * (((2.0_f32 * p1 + (p2 - p0) * alpha)
            + (((2.0_f32 * p0 - 5.0_f32 * p1) + 4.0_f32 * p2) - p3) * alpha * alpha)
            + ((((3.0_f32 * p1 - p0) - 3.0_f32 * p2) + p3) * alpha * alpha * alpha))
}

/// `Mth.triangleWave(index, period)` (`f32`).
#[inline]
pub fn triangle_wave(index: f32, period: f32) -> f32 {
    (((index % period) - period * 0.5_f32).abs() - period * 0.25_f32) / (period * 0.25_f32)
}

// ---------------------------------------------------------------------------
// Easing curves (`Ease` and `EasingType`)
// ---------------------------------------------------------------------------

/// The 32 named easing curves registered in `EasingType` (`constant`, `linear`, plus the 30
/// `Ease` curves).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ease {
    /// Constant `0.0_f32`.
    Constant,
    /// Identity `x`.
    Linear,
    /// Quadratic ease-in.
    InQuad,
    /// Quadratic ease-out.
    OutQuad,
    /// Quadratic ease-in-out.
    InOutQuad,
    /// Cubic ease-in.
    InCubic,
    /// Cubic ease-out.
    OutCubic,
    /// Cubic ease-in-out.
    InOutCubic,
    /// Quartic ease-in.
    InQuart,
    /// Quartic ease-out.
    OutQuart,
    /// Quartic ease-in-out.
    InOutQuart,
    /// Quintic ease-in.
    InQuint,
    /// Quintic ease-out.
    OutQuint,
    /// Quintic ease-in-out.
    InOutQuint,
    /// Sine ease-in (uses lookup-table `cos`).
    InSine,
    /// Sine ease-out (uses lookup-table `sin`).
    OutSine,
    /// Sine ease-in-out (uses lookup-table `cos`).
    InOutSine,
    /// Exponential ease-in.
    InExpo,
    /// Exponential ease-out.
    OutExpo,
    /// Exponential ease-in-out.
    InOutExpo,
    /// Circular ease-in.
    InCirc,
    /// Circular ease-out.
    OutCirc,
    /// Circular ease-in-out.
    InOutCirc,
    /// Back ease-in.
    InBack,
    /// Back ease-out.
    OutBack,
    /// Back ease-in-out.
    InOutBack,
    /// Elastic ease-in (uses JVM `Math.sin` and `f32`-folded `ELASTIC_C4`).
    InElastic,
    /// Elastic ease-out (uses JVM `Math.sin` and `f32`-folded `ELASTIC_C4`).
    OutElastic,
    /// Elastic ease-in-out (uses JVM `Math.sin` and `f32`-folded `ELASTIC_C5`).
    InOutElastic,
    /// Bounce ease-in.
    InBounce,
    /// Bounce ease-out.
    OutBounce,
    /// Bounce ease-in-out.
    InOutBounce,
}

impl Ease {
    /// All 32 named easing curves in canonical order.
    pub const ALL: [Self; 32] = [
        Self::Constant,
        Self::Linear,
        Self::InQuad,
        Self::OutQuad,
        Self::InOutQuad,
        Self::InCubic,
        Self::OutCubic,
        Self::InOutCubic,
        Self::InQuart,
        Self::OutQuart,
        Self::InOutQuart,
        Self::InQuint,
        Self::OutQuint,
        Self::InOutQuint,
        Self::InSine,
        Self::OutSine,
        Self::InOutSine,
        Self::InExpo,
        Self::OutExpo,
        Self::InOutExpo,
        Self::InCirc,
        Self::OutCirc,
        Self::InOutCirc,
        Self::InBack,
        Self::OutBack,
        Self::InOutBack,
        Self::InElastic,
        Self::OutElastic,
        Self::InOutElastic,
        Self::InBounce,
        Self::OutBounce,
        Self::InOutBounce,
    ];

    /// Returns the `EasingType` snake_case identifier.
    #[inline]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Constant => "constant",
            Self::Linear => "linear",
            Self::InQuad => "in_quad",
            Self::OutQuad => "out_quad",
            Self::InOutQuad => "in_out_quad",
            Self::InCubic => "in_cubic",
            Self::OutCubic => "out_cubic",
            Self::InOutCubic => "in_out_cubic",
            Self::InQuart => "in_quart",
            Self::OutQuart => "out_quart",
            Self::InOutQuart => "in_out_quart",
            Self::InQuint => "in_quint",
            Self::OutQuint => "out_quint",
            Self::InOutQuint => "in_out_quint",
            Self::InSine => "in_sine",
            Self::OutSine => "out_sine",
            Self::InOutSine => "in_out_sine",
            Self::InExpo => "in_expo",
            Self::OutExpo => "out_expo",
            Self::InOutExpo => "in_out_expo",
            Self::InCirc => "in_circ",
            Self::OutCirc => "out_circ",
            Self::InOutCirc => "in_out_circ",
            Self::InBack => "in_back",
            Self::OutBack => "out_back",
            Self::InOutBack => "in_out_back",
            Self::InElastic => "in_elastic",
            Self::OutElastic => "out_elastic",
            Self::InOutElastic => "in_out_elastic",
            Self::InBounce => "in_bounce",
            Self::OutBounce => "out_bounce",
            Self::InOutBounce => "in_out_bounce",
        }
    }

    /// Parses an `EasingType` identifier into an [`Ease`] variant.
    #[inline]
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "constant" => Some(Self::Constant),
            "linear" => Some(Self::Linear),
            "in_quad" => Some(Self::InQuad),
            "out_quad" => Some(Self::OutQuad),
            "in_out_quad" => Some(Self::InOutQuad),
            "in_cubic" => Some(Self::InCubic),
            "out_cubic" => Some(Self::OutCubic),
            "in_out_cubic" => Some(Self::InOutCubic),
            "in_quart" => Some(Self::InQuart),
            "out_quart" => Some(Self::OutQuart),
            "in_out_quart" => Some(Self::InOutQuart),
            "in_quint" => Some(Self::InQuint),
            "out_quint" => Some(Self::OutQuint),
            "in_out_quint" => Some(Self::InOutQuint),
            "in_sine" => Some(Self::InSine),
            "out_sine" => Some(Self::OutSine),
            "in_out_sine" => Some(Self::InOutSine),
            "in_expo" => Some(Self::InExpo),
            "out_expo" => Some(Self::OutExpo),
            "in_out_expo" => Some(Self::InOutExpo),
            "in_circ" => Some(Self::InCirc),
            "out_circ" => Some(Self::OutCirc),
            "in_out_circ" => Some(Self::InOutCirc),
            "in_back" => Some(Self::InBack),
            "out_back" => Some(Self::OutBack),
            "in_out_back" => Some(Self::InOutBack),
            "in_elastic" => Some(Self::InElastic),
            "out_elastic" => Some(Self::OutElastic),
            "in_out_elastic" => Some(Self::InOutElastic),
            "in_bounce" => Some(Self::InBounce),
            "out_bounce" => Some(Self::OutBounce),
            "in_out_bounce" => Some(Self::InOutBounce),
            _ => None,
        }
    }

    /// Evaluates the easing curve at `x` following exact `f32`/`f64` promotion and lookup-table
    /// vs `Math.*` rules.
    #[inline]
    pub fn apply(self, x: f32) -> f32 {
        let xd = f64::from(x);
        match self {
            Self::Constant => 0.0_f32,
            Self::Linear => x,
            Self::InQuad => x * x,
            Self::OutQuad => {
                let d = 1.0_f32 - x;
                1.0_f32 - d * d
            }
            Self::InOutQuad => {
                if x < 0.5_f32 {
                    2.0_f32 * (x * x)
                } else {
                    (1.0_f64 - java_math_pow(-2.0_f64 * xd + 2.0_f64, 2.0_f64) / 2.0_f64) as f32
                }
            }
            Self::InCubic => (x * x) * x,
            Self::OutCubic => 1.0_f32 - cube_f32(1.0_f32 - x),
            Self::InOutCubic => {
                if x < 0.5_f32 {
                    4.0_f32 * cube_f32(x)
                } else {
                    (1.0_f64 - java_math_pow(-2.0_f64 * xd + 2.0_f64, 3.0_f64) / 2.0_f64) as f32
                }
            }
            Self::InQuart => {
                let sq = x * x;
                sq * sq
            }
            Self::OutQuart => {
                let d = 1.0_f32 - x;
                let sq = d * d;
                1.0_f32 - sq * sq
            }
            Self::InOutQuart => {
                if x < 0.5_f32 {
                    let sq = x * x;
                    8.0_f32 * (sq * sq)
                } else {
                    (1.0_f64 - java_math_pow(-2.0_f64 * xd + 2.0_f64, 4.0_f64) / 2.0_f64) as f32
                }
            }
            Self::InQuint => {
                let sq = x * x;
                (sq * sq) * x
            }
            Self::OutQuint => 1.0_f32 - (java_math_pow(1.0_f64 - xd, 5.0_f64) as f32),
            Self::InOutQuint => {
                if xd < 0.5_f64 {
                    ((((16.0_f32 * x) * x) * x) * x) * x
                } else {
                    (1.0_f64 - java_math_pow(-2.0_f64 * xd + 2.0_f64, 5.0_f64) / 2.0_f64) as f32
                }
            }
            Self::InSine => 1.0_f32 - cos(f64::from(x * HALF_PI)),
            Self::OutSine => sin(f64::from(x * HALF_PI)),
            Self::InOutSine => -(cos(f64::from(PI * x)) - 1.0_f32) / 2.0_f32,
            Self::InExpo => {
                if x == 0.0_f32 {
                    0.0_f32
                } else {
                    java_math_pow(2.0_f64, 10.0_f64 * xd - 10.0_f64) as f32
                }
            }
            Self::OutExpo => {
                if x == 1.0_f32 {
                    1.0_f32
                } else {
                    1.0_f32 - (java_math_pow(2.0_f64, -10.0_f64 * xd) as f32)
                }
            }
            Self::InOutExpo => {
                if x < 0.5_f32 {
                    if x == 0.0_f32 {
                        0.0_f32
                    } else {
                        (java_math_pow(2.0_f64, 20.0_f64 * xd - 10.0_f64) / 2.0_f64) as f32
                    }
                } else if x == 1.0_f32 {
                    1.0_f32
                } else {
                    ((2.0_f64 - java_math_pow(2.0_f64, -20.0_f64 * xd + 10.0_f64)) / 2.0_f64) as f32
                }
            }
            Self::InCirc => (-(f64::from(1.0_f32 - x * x).sqrt()) as f32) + 1.0_f32,
            Self::OutCirc => {
                let d = x - 1.0_f32;
                f64::from(1.0_f32 - d * d).sqrt() as f32
            }
            Self::InOutCirc => {
                if x < 0.5_f32 {
                    ((1.0_f64 - (1.0_f64 - java_math_pow(2.0_f64 * xd, 2.0_f64)).sqrt()) / 2.0_f64)
                        as f32
                } else {
                    (((1.0_f64 - java_math_pow(-2.0_f64 * xd + 2.0_f64, 2.0_f64)).sqrt() + 1.0_f64)
                        / 2.0_f64) as f32
                }
            }
            Self::InBack => (x * x) * (BACK_C3 * x - BACK_C1),
            Self::OutBack => {
                let d = x - 1.0_f32;
                (1.0_f32 + BACK_C3 * cube_f32(d)) + BACK_C1 * (d * d)
            }
            Self::InOutBack => {
                if x < 0.5_f32 {
                    (((4.0_f32 * x) * x) * (BACK_2C2_PLUS_2 * x - BACK_C2)) / 2.0_f32
                } else {
                    let d = 2.0_f32 * x - 2.0_f32;
                    ((d * d) * (BACK_C2_PLUS_1 * d + BACK_C2) + 2.0_f32) / 2.0_f32
                }
            }
            Self::InElastic => {
                if x == 0.0_f32 {
                    0.0_f32
                } else if x == 1.0_f32 {
                    1.0_f32
                } else {
                    (-java_math_pow(2.0_f64, 10.0_f64 * xd - 10.0_f64)
                        * java_math_sin((xd * 10.0_f64 - 10.75_f64) * ELASTIC_C4))
                        as f32
                }
            }
            Self::OutElastic => {
                if x == 0.0_f32 {
                    0.0_f32
                } else if x == 1.0_f32 {
                    1.0_f32
                } else {
                    (java_math_pow(2.0_f64, -10.0_f64 * xd)
                        * java_math_sin((xd * 10.0_f64 - 0.75_f64) * ELASTIC_C4)
                        + 1.0_f64) as f32
                }
            }
            Self::InOutElastic => {
                if x == 0.0_f32 {
                    0.0_f32
                } else if x == 1.0_f32 {
                    1.0_f32
                } else {
                    let s = java_math_sin((20.0_f64 * xd - 11.125_f64) * ELASTIC_C5);
                    if x < 0.5_f32 {
                        (-(java_math_pow(2.0_f64, 20.0_f64 * xd - 10.0_f64) * s) / 2.0_f64) as f32
                    } else {
                        ((java_math_pow(2.0_f64, -20.0_f64 * xd + 10.0_f64) * s) / 2.0_f64
                            + 1.0_f64) as f32
                    }
                }
            }
            Self::OutBounce => out_bounce(x),
            Self::InBounce => 1.0_f32 - out_bounce(1.0_f32 - x),
            Self::InOutBounce => {
                if x < 0.5_f32 {
                    (1.0_f32 - out_bounce(1.0_f32 - 2.0_f32 * x)) / 2.0_f32
                } else {
                    (1.0_f32 + out_bounce(2.0_f32 * x - 1.0_f32)) / 2.0_f32
                }
            }
        }
    }
}

#[inline]
fn out_bounce(x: f32) -> f32 {
    if x < BOUNCE_THRESHOLD_1 {
        7.5625_f32 * (x * x)
    } else if x < BOUNCE_THRESHOLD_2 {
        let d = x - BOUNCE_OFFSET_1;
        7.5625_f32 * (d * d) + 0.75_f32
    } else if f64::from(x) < BOUNCE_THRESHOLD_3 {
        let d = x - BOUNCE_OFFSET_2;
        7.5625_f32 * (d * d) + 0.9375_f32
    } else {
        let d = x - BOUNCE_OFFSET_3;
        7.5625_f32 * (d * d) + 0.984375_f32
    }
}

// ---------------------------------------------------------------------------
// CubicBezier easing
// ---------------------------------------------------------------------------

/// `EasingType.CubicBezier`: cubic Bézier easing curve solved in `f32` via 4 Newton-Raphson
/// iterations with bisection fallback.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CubicBezier {
    x_a: f32,
    x_b: f32,
    x_c: f32,
    y_a: f32,
    y_b: f32,
    y_c: f32,
}

impl CubicBezier {
    /// Precomputes the `f32` cubic polynomials for control points `(x1, y1)` and `(x2, y2)`.
    #[inline]
    pub fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        Self {
            x_a: (3.0_f32 * x1 - 3.0_f32 * x2) + 1.0_f32,
            x_b: (-6.0_f32 * x1) + 3.0_f32 * x2,
            x_c: 3.0_f32 * x1,
            y_a: (3.0_f32 * y1 - 3.0_f32 * y2) + 1.0_f32,
            y_b: (-6.0_f32 * y1) + 3.0_f32 * y2,
            y_c: 3.0_f32 * y1,
        }
    }

    /// Symmetric cubic Bézier with `x2 = 1.0 - x1`, `y2 = 1.0 - y1`.
    #[inline]
    pub fn symmetric(x1: f32, y1: f32) -> Self {
        Self::new(x1, y1, 1.0_f32 - x1, 1.0_f32 - y1)
    }

    /// Evaluates `P_x(t) = ((x_a * t + x_b) * t + x_c) * t`.
    #[inline]
    pub fn sample_x(self, t: f32) -> f32 {
        ((self.x_a * t + self.x_b) * t + self.x_c) * t
    }

    /// Evaluates `P'_x(t) = ((3 * x_a) * t + 2 * x_b) * t + x_c`.
    #[inline]
    pub fn sample_x_derivative(self, t: f32) -> f32 {
        ((3.0_f32 * self.x_a) * t + 2.0_f32 * self.x_b) * t + self.x_c
    }

    /// Evaluates `P_y(t) = ((y_a * t + y_b) * t + y_c) * t`.
    #[inline]
    pub fn sample_y(self, t: f32) -> f32 {
        ((self.y_a * t + self.y_b) * t + self.y_c) * t
    }

    /// Solves `P_x(t) = x` in `f32`: up to 4 clamped Newton-Raphson iterations followed by
    /// bisection initialized with `t = t_nr`, `t0 = 0.0`, `t1 = 1.0`.
    ///
    /// Includes a stagnation guard (`next_t == t`) so adjacent `f32` bracket endpoints that never
    /// meet `|e| < 1e-5` terminate without affecting any converging input.
    #[inline]
    pub fn solve_t(self, x: f32) -> f32 {
        let mut t = x;
        for _ in 0..4 {
            let e = self.sample_x(t) - x;
            if e.abs() < EPSILON {
                return t;
            }
            let g = self.sample_x_derivative(t);
            if g < EPSILON {
                break;
            }
            t -= clamp_f32(e / g, -0.25_f32, 0.25_f32);
        }

        let mut t0 = 0.0_f32;
        let mut t1 = 1.0_f32;
        let mut iter = 0_u32;
        while t0 < t1 && iter < 128 {
            iter += 1;
            let e = self.sample_x(t) - x;
            if e.abs() < EPSILON {
                return t;
            }
            if e < 0.0_f32 {
                t0 = t;
            } else {
                t1 = t;
            }
            let next_t = (t1 + t0) / 2.0_f32;
            if next_t == t {
                return next_t;
            }
            t = next_t;
        }
        t
    }

    /// Evaluates the easing curve `P_y(solve_t(x))`.
    #[inline]
    pub fn apply(self, x: f32) -> f32 {
        self.sample_y(self.solve_t(x))
    }
}

/// Unified `EasingType`: either one of the 32 named [`Ease`] curves or a [`CubicBezier`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EasingType {
    /// Named easing curve.
    Named(Ease),
    /// Cubic Bézier curve.
    CubicBezier(CubicBezier),
}

impl EasingType {
    /// Evaluates this easing function at `x`.
    #[inline]
    pub fn apply(self, x: f32) -> f32 {
        match self {
            Self::Named(ease) => ease.apply(x),
            Self::CubicBezier(bezier) => bezier.apply(x),
        }
    }
}

impl From<Ease> for EasingType {
    #[inline]
    fn from(value: Ease) -> Self {
        Self::Named(value)
    }
}

impl From<CubicBezier> for EasingType {
    #[inline]
    fn from(value: CubicBezier) -> Self {
        Self::CubicBezier(value)
    }
}

// ---------------------------------------------------------------------------
// Piecewise cubic Hermite spline (`CubicSpline`)
// ---------------------------------------------------------------------------

/// Coordinate extractor used by [`CubicSpline::Multipoint`].
pub trait SplineCoordinate<C> {
    /// Evaluates the coordinate value for `context`.
    fn apply(&self, context: &C) -> f32;

    /// Returns the conservative `f32` output interval of this coordinate.
    fn range(&self) -> Interval;
}

/// Trivial 1D coordinate passing an `f32` context straight through with a configured [`Interval`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IdentityCoordinate {
    range: Interval,
}

impl IdentityCoordinate {
    /// Creates an identity coordinate with conservative input `range`.
    #[inline]
    pub const fn new(range: Interval) -> Self {
        Self { range }
    }
}

impl SplineCoordinate<f32> for IdentityCoordinate {
    #[inline]
    fn apply(&self, context: &f32) -> f32 {
        *context
    }

    #[inline]
    fn range(&self) -> Interval {
        self.range
    }
}

/// Validation error returned when constructing a [`CubicSpline::Multipoint`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplineError {
    /// A multipoint spline requires at least one control point.
    EmptyPoints,
    /// `locations`, `values`, and `derivatives` did not all have the same length.
    LengthMismatch,
    /// Control-point `locations` were not strictly ascending (or contained `NaN`).
    NonAscendingLocations,
}

impl fmt::Display for SplineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPoints => write!(f, "cubic spline requires at least one control point"),
            Self::LengthMismatch => {
                write!(
                    f,
                    "cubic spline locations, values, and derivatives must have equal length"
                )
            }
            Self::NonAscendingLocations => {
                write!(f, "cubic spline locations must be strictly ascending")
            }
        }
    }
}

impl std::error::Error for SplineError {}

/// Piecewise cubic Hermite spline (`CubicSpline`).
#[derive(Debug, Clone, PartialEq)]
pub enum CubicSpline<C, F> {
    /// Constant value `v` with interval `[v, v]`.
    Constant(f32),
    /// Piecewise cubic Hermite spline with strictly ascending `locations`.
    Multipoint {
        /// Coordinate function producing the spline input parameter `u`.
        coordinate: F,
        /// Strictly ascending knot locations (`len >= 1`).
        locations: Vec<f32>,
        /// Nested spline values at each knot.
        values: Vec<CubicSpline<C, F>>,
        /// Slopes (`du`) at each knot.
        derivatives: Vec<f32>,
        /// Precomputed conservative output interval.
        range: Interval,
        /// Marker binding context type `C`.
        _marker: std::marker::PhantomData<fn(&C)>,
    },
}

impl<C, F: SplineCoordinate<C>> CubicSpline<C, F> {
    /// Creates a constant spline node.
    #[inline]
    pub const fn constant(value: f32) -> Self {
        Self::Constant(value)
    }

    /// Creates a validated multipoint cubic Hermite spline and precomputes its conservative
    /// [`Interval`].
    pub fn multipoint(
        coordinate: F,
        locations: Vec<f32>,
        values: Vec<Self>,
        derivatives: Vec<f32>,
    ) -> Result<Self, SplineError> {
        let n = locations.len();
        if n == 0 {
            return Err(SplineError::EmptyPoints);
        }
        if values.len() != n || derivatives.len() != n {
            return Err(SplineError::LengthMismatch);
        }
        for loc in &locations {
            if loc.is_nan() {
                return Err(SplineError::NonAscendingLocations);
            }
        }
        for i in 0..(n - 1) {
            if locations[i] >= locations[i + 1] {
                return Err(SplineError::NonAscendingLocations);
            }
        }

        let range = compute_multipoint_range(&coordinate, &locations, &values, &derivatives);
        Ok(Self::Multipoint {
            coordinate,
            locations,
            values,
            derivatives,
            range,
            _marker: std::marker::PhantomData,
        })
    }

    /// Evaluates the spline at `context`.
    pub fn sample(&self, context: &C) -> f32 {
        match self {
            Self::Constant(v) => *v,
            Self::Multipoint {
                coordinate,
                locations,
                values,
                derivatives,
                ..
            } => {
                let u = coordinate.apply(context);
                let n = locations.len();
                let k = binary_search(0, n as i32, |i| u < locations[i as usize]) - 1;
                if k < 0 {
                    linear_extend(u, locations[0], values[0].sample(context), derivatives[0])
                } else if (k as usize) == n - 1 {
                    let last = n - 1;
                    linear_extend(
                        u,
                        locations[last],
                        values[last].sample(context),
                        derivatives[last],
                    )
                } else {
                    let idx = k as usize;
                    let x1 = locations[idx];
                    let x2 = locations[idx + 1];
                    let t = (u - x1) / (x2 - x1);
                    let y1 = values[idx].sample(context);
                    let y2 = values[idx + 1].sample(context);
                    let d1 = derivatives[idx];
                    let d2 = derivatives[idx + 1];
                    let a = d1 * (x2 - x1) - (y2 - y1);
                    let b = (-d2) * (x2 - x1) + (y2 - y1);
                    lerp_f32(t, y1, y2) + (t * (1.0_f32 - t)) * lerp_f32(t, a, b)
                }
            }
        }
    }

    /// Returns the conservative `f32` output interval for this spline.
    #[inline]
    pub fn range(&self) -> Interval {
        match self {
            Self::Constant(v) => Interval::exact(*v),
            Self::Multipoint { range, .. } => *range,
        }
    }
}

#[inline]
fn linear_extend(u: f32, loc: f32, y: f32, deriv: f32) -> f32 {
    if deriv == 0.0_f32 {
        y
    } else {
        y + deriv * (u - loc)
    }
}

fn compute_multipoint_range<C, F: SplineCoordinate<C>>(
    coordinate: &F,
    locations: &[f32],
    values: &[CubicSpline<C, F>],
    derivatives: &[f32],
) -> Interval {
    let input_range = coordinate.range();
    let Some((in_min, in_max)) = input_range.bounds() else {
        return Interval::NaI;
    };

    let n = locations.len();
    let mut acc_min = f32::INFINITY;
    let mut acc_max = f32::NEG_INFINITY;

    if in_min < locations[0] {
        let Some((m0, big_m0)) = values[0].range().bounds() else {
            return Interval::NaI;
        };
        let e0 = linear_extend(in_min, locations[0], m0, derivatives[0]);
        let e1 = linear_extend(in_min, locations[0], big_m0, derivatives[0]);
        acc_min = java_min_f32(acc_min, java_min_f32(e0, e1));
        acc_max = java_max_f32(acc_max, java_max_f32(e0, e1));
    }

    if in_max > locations[n - 1] {
        let last = n - 1;
        let Some((ml, big_ml)) = values[last].range().bounds() else {
            return Interval::NaI;
        };
        let e0 = linear_extend(in_max, locations[last], ml, derivatives[last]);
        let e1 = linear_extend(in_max, locations[last], big_ml, derivatives[last]);
        acc_min = java_min_f32(acc_min, java_min_f32(e0, e1));
        acc_max = java_max_f32(acc_max, java_max_f32(e0, e1));
    }

    for v in values {
        let Some((mv, big_mv)) = v.range().bounds() else {
            return Interval::NaI;
        };
        acc_min = java_min_f32(acc_min, mv);
        acc_max = java_max_f32(acc_max, big_mv);
    }

    for i in 0..(n - 1) {
        let d1 = derivatives[i];
        let d2 = derivatives[i + 1];
        if d1 != 0.0_f32 || d2 != 0.0_f32 {
            let dx = locations[i + 1] - locations[i];
            let p1 = d1 * dx;
            let p2 = d2 * dx;
            let (m1, big_m1) = values[i].range().bounds().expect("checked non-NaI above");
            let (m2, big_m2) = values[i + 1]
                .range()
                .bounds()
                .expect("checked non-NaI above");
            let lo =
                java_min_f32(m1, m2) + 0.25_f32 * java_min_f32(p1 - big_m2 + m1, -p2 + m2 - big_m1);
            let hi = java_max_f32(big_m1, big_m2)
                + 0.25_f32 * java_max_f32(p1 - m2 + big_m1, -p2 + big_m2 - m1);
            acc_min = java_min_f32(acc_min, lo);
            acc_max = java_max_f32(acc_max, hi);
        }
    }

    Interval::new(acc_min, acc_max)
}

// ---------------------------------------------------------------------------
// Stateful interpolation helpers (`SmoothDouble`, `BinaryAnimator`)
// ---------------------------------------------------------------------------

/// `SmoothDouble`: damped scalar smoother (e.g., cinematic camera smoothing).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SmoothDouble {
    target: f64,
    remaining: f64,
    last_amount: f64,
}

impl SmoothDouble {
    /// Creates a zero-initialized `SmoothDouble`.
    #[inline]
    pub const fn new() -> Self {
        Self {
            target: 0.0,
            remaining: 0.0,
            last_amount: 0.0,
        }
    }

    /// Resets all internal accumulators to `0.0`.
    #[inline]
    pub fn reset(&mut self) {
        self.target = 0.0;
        self.remaining = 0.0;
        self.last_amount = 0.0;
    }

    /// Advances the smoother by `(target_delta, time)` and returns the smoothed step.
    #[inline]
    pub fn get_new_delta(&mut self, target_delta: f64, time: f64) -> f64 {
        self.target += target_delta;
        let mut delta = self.target - self.remaining;
        let new_last = lerp_f64(0.5_f64, self.last_amount, delta);
        let s = java_signum_f64(delta);
        if s * delta > s * self.last_amount {
            delta = new_last;
        }
        self.last_amount = new_last;
        self.remaining += delta * time;
        delta * time
    }
}

/// `BinaryAnimator`: discrete tick counter in `[0, length]` with eased fractional readout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BinaryAnimator {
    length: i32,
    easing: EasingType,
    ticks: i32,
    ticks_old: i32,
}

impl BinaryAnimator {
    /// Creates a new `BinaryAnimator` starting at `ticks = 0, ticks_old = 0`.
    #[inline]
    pub const fn new(length: i32, easing: EasingType) -> Self {
        Self {
            length,
            easing,
            ticks: 0,
            ticks_old: 0,
        }
    }

    /// Advances one tick: copies `ticks` to `ticks_old`, then increments if `active && ticks < length`
    /// or decrements if `!active && ticks > 0`.
    #[inline]
    pub fn tick(&mut self, active: bool) {
        self.ticks_old = self.ticks;
        if active {
            if self.ticks < self.length {
                self.ticks += 1;
            }
        } else if self.ticks > 0 {
            self.ticks -= 1;
        }
    }

    /// Returns the eased animation factor at `partial_ticks`.
    #[inline]
    pub fn get_factor(&self, partial_ticks: f32) -> f32 {
        let t = lerp_f32(partial_ticks, self.ticks_old as f32, self.ticks as f32)
            / (self.length as f32);
        self.easing.apply(t)
    }

    /// Returns the current integer tick count.
    #[inline]
    pub const fn ticks(&self) -> i32 {
        self.ticks
    }

    /// Returns the previous integer tick count.
    #[inline]
    pub const fn ticks_old(&self) -> i32 {
        self.ticks_old
    }
}
