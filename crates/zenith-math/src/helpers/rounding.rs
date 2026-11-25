//! Rounding, clamping, modular arithmetic, bit-manipulation, binary search, `Divisor`, and
//! outward integer stepping (`Mth`, `Divisor`).
//!
//! Implements Section 3 of `docs/specs/math/java-math-helpers.md`.

/// `Mth.EPSILON`: `1.0E-5F` (`0x3727C5AC`).
pub const EPSILON: f32 = f32::from_bits(0x3727_C5AC);

/// `Mth.EPSILON` widened to `f64`: `9.999999747378752e-6` (`0x3EE4F8B580000000`).
pub const EPSILON_F64: f64 = EPSILON as f64;

/// `com.mojang.math.Constants.EPSILON`: `1.0E-6F` (`0x358637BD`).
pub const CONSTANTS_EPSILON: f32 = f32::from_bits(0x3586_37BD);

/// De Bruijn multiplier `125613361` (`0x077CB531`) used in [`ceillog2`].
pub const DE_BRUIJN_MULTIPLIER: i64 = 0x077C_B531;

/// `Mth.MULTIPLY_DE_BRUIJN_BIT_POSITION[0..32]` lookup table for [`ceillog2`].
pub const MULTIPLY_DE_BRUIJN_BIT_POSITION: [i32; 32] = [
    0, 1, 28, 2, 29, 14, 24, 3, 30, 22, 20, 15, 25, 17, 4, 8, 31, 27, 13, 23, 21, 19, 16, 7, 26,
    12, 18, 6, 11, 5, 10, 9,
];

// ---------------------------------------------------------------------------
// Java SE floating-point and integer primitives
// ---------------------------------------------------------------------------

/// Java `Math.min(float, float)`: propagates `NaN` and orders `-0.0 < +0.0`.
#[inline]
pub fn java_min_f32(a: f32, b: f32) -> f32 {
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
pub fn java_max_f32(a: f32, b: f32) -> f32 {
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

/// Java `Math.min(double, double)`: propagates `NaN` and orders `-0.0 < +0.0`.
#[inline]
pub fn java_min_f64(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == 0.0_f64 && b == 0.0_f64 {
        if a.is_sign_negative() || b.is_sign_negative() {
            -0.0_f64
        } else {
            0.0_f64
        }
    } else if a < b {
        a
    } else {
        b
    }
}

/// Java `Math.max(double, double)`: propagates `NaN` and orders `-0.0 < +0.0`.
#[inline]
pub fn java_max_f64(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == 0.0_f64 && b == 0.0_f64 {
        if a.is_sign_positive() || b.is_sign_positive() {
            0.0_f64
        } else {
            -0.0_f64
        }
    } else if a > b {
        a
    } else {
        b
    }
}

/// Java `Math.round(float) -> int`: exact `floor(x + 0.5)` with saturating conversion to `i32`
/// (`NaN -> 0`, ties toward `+inf`, so `round(-0.5) == 0` and `round(0.49999997f) == 0`).
#[inline]
pub fn java_round_f32(x: f32) -> i32 {
    if x.is_nan() {
        0
    } else if x.abs() >= 8_388_608.0_f32 {
        // Every f32 with |x| >= 2^23 is already an exact integer (or infinity).
        x as i32
    } else {
        (f64::from(x) + 0.5_f64).floor() as i32
    }
}

/// Java `Math.round(double) -> long`: exact `floor(x + 0.5)` with saturating conversion to `i64`
/// (`NaN -> 0`, ties toward `+inf`, so `round(-0.5) == 0` and `round(0.49999999999999994) == 0`).
#[inline]
pub fn java_round_f64(x: f64) -> i64 {
    if x.is_nan() {
        0
    } else if x.abs() >= 4_503_599_627_370_496.0_f64 {
        // Every f64 with |x| >= 2^52 is already an exact integer (or infinity).
        x as i64
    } else {
        let f = x.floor();
        // By Sterbenz's Lemma, `x - f` cannot cross 0.5 due to rounding for |x| < 2^52.
        if (x - f) >= 0.5_f64 {
            (f as i64).wrapping_add(1)
        } else {
            f as i64
        }
    }
}

/// Java `Math.signum(double)`: returns `NaN` for `NaN`, preserves `±0.0`, `+1.0` for `> 0.0`,
/// and `-1.0` for `< 0.0`.
#[inline]
pub fn java_signum_f64(x: f64) -> f64 {
    if x == 0.0_f64 || x.is_nan() {
        x
    } else if x > 0.0_f64 {
        1.0_f64
    } else {
        -1.0_f64
    }
}

/// Java `Math.clamp(int, int, int)`.
///
/// # Panics
///
/// Panics if `lo > hi`.
#[inline]
pub fn java_clamp_i32(v: i32, lo: i32, hi: i32) -> i32 {
    assert!(lo <= hi, "lo ({lo}) must be <= hi ({hi})");
    v.max(lo).min(hi)
}

/// Java `Math.clamp(long, long, long)`.
///
/// # Panics
///
/// Panics if `lo > hi`.
#[inline]
pub fn java_clamp_i64(v: i64, lo: i64, hi: i64) -> i64 {
    assert!(lo <= hi, "lo ({lo}) must be <= hi ({hi})");
    v.max(lo).min(hi)
}

/// Java `Math.clamp(float, float, float)`: propagates `NaN` and orders `-0.0 < +0.0`.
///
/// # Panics
///
/// Panics if `lo` or `hi` is `NaN`, or `lo > hi` (including `lo = +0.0, hi = -0.0`).
#[inline]
pub fn java_clamp_f32(v: f32, lo: f32, hi: f32) -> f32 {
    assert!(
        !lo.is_nan()
            && !hi.is_nan()
            && lo <= hi
            && !(lo == 0.0_f32 && hi == 0.0_f32 && lo.is_sign_positive() && hi.is_sign_negative()),
        "invalid clamp bounds [{lo}, {hi}]"
    );
    java_min_f32(hi, java_max_f32(v, lo))
}

/// Java `Math.clamp(double, double, double)`: propagates `NaN` and orders `-0.0 < +0.0`.
///
/// # Panics
///
/// Panics if `lo` or `hi` is `NaN`, or `lo > hi` (including `lo = +0.0, hi = -0.0`).
#[inline]
pub fn java_clamp_f64(v: f64, lo: f64, hi: f64) -> f64 {
    assert!(
        !lo.is_nan()
            && !hi.is_nan()
            && lo <= hi
            && !(lo == 0.0_f64 && hi == 0.0_f64 && lo.is_sign_positive() && hi.is_sign_negative()),
        "invalid clamp bounds [{lo}, {hi}]"
    );
    java_min_f64(hi, java_max_f64(v, lo))
}

/// Java `Math.floorDiv(int, int)` (wrapping on `(i32::MIN, -1)`).
///
/// # Panics
///
/// Panics if `b == 0`.
#[inline]
pub fn java_floor_div_i32(a: i32, b: i32) -> i32 {
    assert!(b != 0, "division by zero");
    let r = a.wrapping_div(b);
    if (a ^ b) < 0 && r.wrapping_mul(b) != a {
        r.wrapping_sub(1)
    } else {
        r
    }
}

/// Java `Math.floorDiv(long, long)` (wrapping on `(i64::MIN, -1)`).
///
/// # Panics
///
/// Panics if `b == 0`.
#[inline]
pub fn java_floor_div_i64(a: i64, b: i64) -> i64 {
    assert!(b != 0, "division by zero");
    let r = a.wrapping_div(b);
    if (a ^ b) < 0 && r.wrapping_mul(b) != a {
        r.wrapping_sub(1)
    } else {
        r
    }
}

/// Java `Math.floorMod(int, int)`.
///
/// # Panics
///
/// Panics if `b == 0`.
#[inline]
pub fn java_floor_mod_i32(a: i32, b: i32) -> i32 {
    a.wrapping_sub(java_floor_div_i32(a, b).wrapping_mul(b))
}

/// Java `Math.floorMod(long, long)`.
///
/// # Panics
///
/// Panics if `b == 0`.
#[inline]
pub fn java_floor_mod_i64(a: i64, b: i64) -> i64 {
    a.wrapping_sub(java_floor_div_i64(a, b).wrapping_mul(b))
}

// ---------------------------------------------------------------------------
// Floor, ceil, and fractional part
// ---------------------------------------------------------------------------

/// `Mth.floor(float) -> int`: saturating `floor(f64(v))` to `i32`.
#[inline]
pub fn floor_f32(v: f32) -> i32 {
    f64::from(v).floor() as i32
}

/// `Mth.floor(double) -> int`: saturating `floor(v)` to `i32`.
#[inline]
pub fn floor_f64(v: f64) -> i32 {
    v.floor() as i32
}

/// `Mth.lfloor(double) -> long`: saturating `floor(v)` to `i64`.
#[inline]
pub fn lfloor(v: f64) -> i64 {
    v.floor() as i64
}

/// `Mth.ceil(float) -> int`: saturating `ceil(f64(v))` to `i32`.
#[inline]
pub fn ceil_f32(v: f32) -> i32 {
    f64::from(v).ceil() as i32
}

/// `Mth.ceil(double) -> int`: saturating `ceil(v)` to `i32`.
#[inline]
pub fn ceil_f64(v: f64) -> i32 {
    v.ceil() as i32
}

/// `Mth.ceilLong(double) -> long`: saturating `ceil(v)` to `i64`.
#[inline]
pub fn ceil_long(v: f64) -> i64 {
    v.ceil() as i64
}

/// `Mth.frac(float) -> float`: `v - f32(floor_f32(v))`.
#[inline]
pub fn frac_f32(v: f32) -> f32 {
    v - (floor_f32(v) as f32)
}

/// `Mth.frac(double) -> double`: `v - f64(lfloor(v))`.
#[inline]
pub fn frac_f64(v: f64) -> f64 {
    v - (lfloor(v) as f64)
}

// ---------------------------------------------------------------------------
// Clamping (`Mth.clamp`)
// ---------------------------------------------------------------------------

/// `Mth.clamp(int, int, int)`: `min(max(v, lo), hi)` (returns `hi` if `lo > hi`).
#[inline]
pub const fn clamp_i32(v: i32, lo: i32, hi: i32) -> i32 {
    let m = if v > lo { v } else { lo };
    if m < hi { m } else { hi }
}

/// `Mth.clamp(long, long, long)`: `min(max(v, lo), hi)` (returns `hi` if `lo > hi`).
#[inline]
pub const fn clamp_i64(v: i64, lo: i64, hi: i64) -> i64 {
    let m = if v > lo { v } else { lo };
    if m < hi { m } else { hi }
}

/// `Mth.clamp(float, float, float)`: `if v < lo { lo } else { java_min_f32(v, hi) }`.
///
/// Preserves `-0.0` when `lo = +0.0, hi >= 0.0`, propagates `NaN`, and returns `lo` when
/// `lo > hi && v < lo`.
#[inline]
pub fn clamp_f32(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo { lo } else { java_min_f32(v, hi) }
}

/// `Mth.clamp(double, double, double)`: `if v < lo { lo } else { java_min_f64(v, hi) }`.
///
/// Preserves `-0.0` when `lo = +0.0, hi >= 0.0`, propagates `NaN`, and returns `lo` when
/// `lo > hi && v < lo`.
#[inline]
pub fn clamp_f64(v: f64, lo: f64, hi: f64) -> f64 {
    if v < lo { lo } else { java_min_f64(v, hi) }
}

// ---------------------------------------------------------------------------
// Sign, magnitude, tolerance equality, powers, and lengths
// ---------------------------------------------------------------------------

/// `Mth.abs(int)`: wrapping absolute value (`abs(i32::MIN) == i32::MIN`).
#[inline]
pub const fn abs_i32(v: i32) -> i32 {
    v.wrapping_abs()
}

/// `Mth.abs(float)`: clears the sign bit (`abs(-0.0_f32) == +0.0_f32`).
#[inline]
pub fn abs_f32(v: f32) -> f32 {
    v.abs()
}

/// `Mth.absMax(int, int)`: `max(|a|, |b|)` with wrapping `abs`.
#[inline]
pub const fn abs_max_i32(a: i32, b: i32) -> i32 {
    let aa = a.wrapping_abs();
    let ab = b.wrapping_abs();
    if aa > ab { aa } else { ab }
}

/// `Mth.absMax(float, float)`: `java_max_f32(|a|, |b|)`.
#[inline]
pub fn abs_max_f32(a: f32, b: f32) -> f32 {
    java_max_f32(a.abs(), b.abs())
}

/// `Mth.absMax(double, double)`: `java_max_f64(|a|, |b|)`.
#[inline]
pub fn abs_max_f64(a: f64, b: f64) -> f64 {
    java_max_f64(a.abs(), b.abs())
}

/// `Mth.chessboardDistance(x0, z0, x1, z1)`: `abs_max_i32(x1 - x0, z1 - z0)` (wrapping).
#[inline]
pub const fn chessboard_distance(x0: i32, z0: i32, x1: i32, z1: i32) -> i32 {
    abs_max_i32(x1.wrapping_sub(x0), z1.wrapping_sub(z0))
}

/// `Mth.sign(double) -> int`: `0` if `x == 0.0`, `+1` if `x > 0.0`, and `-1` otherwise
/// (including `NaN`).
#[inline]
pub fn sign(x: f64) -> i32 {
    if x == 0.0_f64 {
        0
    } else if x > 0.0_f64 {
        1
    } else {
        -1
    }
}

/// `Mth.equal(float, float)`: `|b - a| < 1.0E-5F`.
#[inline]
pub fn equal_f32(a: f32, b: f32) -> bool {
    (b - a).abs() < EPSILON
}

/// `Mth.equal(double, double)`: `|b - a| < f64(1.0E-5F)` (`0x3EE4F8B580000000`).
#[inline]
pub fn equal_f64(a: f64, b: f64) -> bool {
    (b - a).abs() < EPSILON_F64
}

/// `Mth.square(float)`: `x * x`.
#[inline]
pub fn square_f32(x: f32) -> f32 {
    x * x
}

/// `Mth.square(double)`: `x * x`.
#[inline]
pub fn square_f64(x: f64) -> f64 {
    x * x
}

/// `Mth.square(int)`: wrapping `x * x`.
#[inline]
pub const fn square_i32(x: i32) -> i32 {
    x.wrapping_mul(x)
}

/// `Mth.square(long)`: wrapping `x * x`.
#[inline]
pub const fn square_i64(x: i64) -> i64 {
    x.wrapping_mul(x)
}

/// `Mth.cube(float)`: `(x * x) * x`.
#[inline]
pub fn cube_f32(x: f32) -> f32 {
    (x * x) * x
}

/// `Mth.lengthSquared(float, float)`.
#[inline]
pub fn length_squared_2d_f32(x: f32, y: f32) -> f32 {
    x * x + y * y
}

/// `Mth.lengthSquared(double, double)`.
#[inline]
pub fn length_squared_2d_f64(x: f64, y: f64) -> f64 {
    x * x + y * y
}

/// `Mth.length(float, float)`: `f32(sqrt(f64(x^2 + y^2)))`.
#[inline]
pub fn length_2d_f32(x: f32, y: f32) -> f32 {
    f64::from(length_squared_2d_f32(x, y)).sqrt() as f32
}

/// `Mth.length(double, double)`.
#[inline]
pub fn length_2d_f64(x: f64, y: f64) -> f64 {
    length_squared_2d_f64(x, y).sqrt()
}

/// `Mth.lengthSquared(float, float, float)`: `(x^2 + y^2) + z^2`.
#[inline]
pub fn length_squared_3d_f32(x: f32, y: f32, z: f32) -> f32 {
    (x * x + y * y) + z * z
}

/// `Mth.lengthSquared(double, double, double)`: `(x^2 + y^2) + z^2`.
#[inline]
pub fn length_squared_3d_f64(x: f64, y: f64, z: f64) -> f64 {
    (x * x + y * y) + z * z
}

/// `Mth.length(float, float, float)`: `f32(sqrt(f64((x^2 + y^2) + z^2)))`.
#[inline]
pub fn length_3d_f32(x: f32, y: f32, z: f32) -> f32 {
    f64::from(length_squared_3d_f32(x, y, z)).sqrt() as f32
}

/// `Mth.length(double, double, double)`.
#[inline]
pub fn length_3d_f64(x: f64, y: f64, z: f64) -> f64 {
    length_squared_3d_f64(x, y, z).sqrt()
}

// ---------------------------------------------------------------------------
// Modulo, division, multiple rounding, quantization, and Divisor
// ---------------------------------------------------------------------------

/// `Mth.positiveModulo(int, int)`: `Math.floorMod(a, m)`.
///
/// # Panics
///
/// Panics if `m == 0`.
#[inline]
pub fn positive_modulo_i32(a: i32, m: i32) -> i32 {
    java_floor_mod_i32(a, m)
}

/// `Mth.positiveModulo(float, float)`: `((a % m) + m) % m`.
#[inline]
pub fn positive_modulo_f32(a: f32, m: f32) -> f32 {
    ((a % m) + m) % m
}

/// `Mth.positiveModulo(double, double)`: `((a % m) + m) % m`.
#[inline]
pub fn positive_modulo_f64(a: f64, m: f64) -> f64 {
    ((a % m) + m) % m
}

/// `Mth.isMultipleOf(int, int)`: `(a % b) == 0` (wrapping on `(i32::MIN, -1)`).
///
/// # Panics
///
/// Panics if `b == 0`.
#[inline]
pub fn is_multiple_of(a: i32, b: i32) -> bool {
    assert!(b != 0, "division by zero");
    a.wrapping_rem(b) == 0
}

/// `Mth.floorDiv(int, int)`: `Math.floorDiv(a, b)`.
///
/// # Panics
///
/// Panics if `b == 0`.
#[inline]
pub fn floor_div(a: i32, b: i32) -> i32 {
    java_floor_div_i32(a, b)
}

/// `Mth.positiveCeilDiv(int, int)`: `-Math.floorDiv(-a, b)` (wrapping negation).
///
/// # Panics
///
/// Panics if `b == 0`.
#[inline]
pub fn positive_ceil_div_i32(a: i32, b: i32) -> i32 {
    java_floor_div_i32(a.wrapping_neg(), b).wrapping_neg()
}

/// `Mth.positiveCeilDiv(long, long)`: `-Math.floorDiv(-a, b)` (wrapping negation).
///
/// # Panics
///
/// Panics if `b == 0`.
#[inline]
pub fn positive_ceil_div_i64(a: i64, b: i64) -> i64 {
    java_floor_div_i64(a.wrapping_neg(), b).wrapping_neg()
}

/// `Mth.roundToward(int, int)`: `positive_ceil_div_i32(a, m) * m` (wrapping multiply).
///
/// # Panics
///
/// Panics if `m == 0`.
#[inline]
pub fn round_toward_i32(a: i32, m: i32) -> i32 {
    positive_ceil_div_i32(a, m).wrapping_mul(m)
}

/// `Mth.roundToward(long, long)`: `positive_ceil_div_i64(a, m) * m` (wrapping multiply).
///
/// # Panics
///
/// Panics if `m == 0`.
#[inline]
pub fn round_toward_i64(a: i64, m: i64) -> i64 {
    positive_ceil_div_i64(a, m).wrapping_mul(m)
}

/// `Mth.quantize(double, int) -> int`: `floor_f64(v / f64(res)) * res` (wrapping multiply).
///
/// When `res == 0`, floating-point division yields `±inf` or `NaN`, floor saturates, and
/// multiplying by `0` yields `0` without panicking.
#[inline]
pub fn quantize(v: f64, res: i32) -> i32 {
    floor_f64(v / f64::from(res)).wrapping_mul(res)
}

/// `Mth.mulAndTruncate(fraction, factor) -> int`: `(numerator * factor) / denominator`
/// (32-bit wrapping product before truncating division).
///
/// # Panics
///
/// Panics if `denominator == 0`.
#[inline]
pub fn mul_and_truncate(numerator: i32, denominator: i32, factor: i32) -> i32 {
    assert!(denominator != 0, "division by zero");
    numerator.wrapping_mul(factor).wrapping_div(denominator)
}

/// `com.mojang.math.Divisor`: distributes `numerator` across `denominator` integer steps.
///
/// Yields zero elements when `denominator <= 0`. For negative `numerator`, the truncating
/// remainder `m <= 0` so the increment branch never fires and every step yields `q`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Divisor {
    denominator: i32,
    quotient: i32,
    mod_val: i32,
    returned_parts: i32,
    remainder: i32,
}

impl Divisor {
    /// Constructs a new `Divisor` iterator for `numerator / denominator`.
    #[inline]
    pub const fn new(numerator: i32, denominator: i32) -> Self {
        if denominator <= 0 {
            Self {
                denominator: 0,
                quotient: 0,
                mod_val: 0,
                returned_parts: 0,
                remainder: 0,
            }
        } else {
            Self {
                denominator,
                quotient: numerator / denominator,
                mod_val: numerator % denominator,
                returned_parts: 0,
                remainder: 0,
            }
        }
    }
}

impl Iterator for Divisor {
    type Item = i32;

    #[inline]
    fn next(&mut self) -> Option<i32> {
        if self.returned_parts >= self.denominator {
            return None;
        }
        self.returned_parts += 1;
        self.remainder = self.remainder.wrapping_add(self.mod_val);
        if self.remainder >= self.denominator {
            self.remainder = self.remainder.wrapping_sub(self.denominator);
            Some(self.quotient.wrapping_add(1))
        } else {
            Some(self.quotient)
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let rem = (self.denominator - self.returned_parts).max(0) as usize;
        (rem, Some(rem))
    }
}

impl ExactSizeIterator for Divisor {}

// ---------------------------------------------------------------------------
// Power-of-two, bit-width, logarithms, search, and outward stepping
// ---------------------------------------------------------------------------

/// `Mth.isPowerOfTwo(int)`: `n != 0 && (n & (n - 1)) == 0`.
///
/// Returns `true` for `i32::MIN` (`0x80000000`).
#[inline]
pub const fn is_power_of_two_i32(n: i32) -> bool {
    n != 0 && (n & n.wrapping_sub(1)) == 0
}

/// `Mth.isPowerOfTwo(long)`: `n != 0 && (n & (n - 1)) == 0`.
///
/// Returns `true` for `i64::MIN` (`0x8000000000000000`).
#[inline]
pub const fn is_power_of_two_i64(n: i64) -> bool {
    n != 0 && (n & n.wrapping_sub(1)) == 0
}

/// `Mth.smallestEncompassingPowerOfTwo(int)`: arithmetic-shift bit smearing.
///
/// Returns the smallest power of two `>= n` for `n in 1..=2^30`, `0` for `n in (i32::MIN + 1)..=0`,
/// and wraps to `i32::MIN` for `n > 2^30` or `n == i32::MIN`.
#[inline]
pub const fn smallest_encompassing_power_of_two(n: i32) -> i32 {
    let mut r = n.wrapping_sub(1);
    r |= r >> 1;
    r |= r >> 2;
    r |= r >> 4;
    r |= r >> 8;
    r |= r >> 16;
    r.wrapping_add(1)
}

/// `Mth.ceillog2(int)`: De Bruijn bit-scan ceiling base-2 logarithm.
///
/// Returns `0` for `n <= 0` except `i32::MIN`, which returns `31`.
#[inline]
pub const fn ceillog2(n: i32) -> i32 {
    let p = if is_power_of_two_i32(n) {
        n
    } else {
        smallest_encompassing_power_of_two(n)
    };
    let idx = (((p as i64).wrapping_mul(DE_BRUIJN_MULTIPLIER) >> 27) & 31) as usize;
    MULTIPLY_DE_BRUIJN_BIT_POSITION[idx]
}

/// `Mth.log2(int)`: `ceillog2(n) - (0 if isPowerOfTwo(n) else 1)`.
///
/// Returns `-1` for `n = 0` and `31` for `n = i32::MIN`.
#[inline]
pub const fn log2(n: i32) -> i32 {
    ceillog2(n) - if is_power_of_two_i32(n) { 0 } else { 1 }
}

/// `Mth.smallestSquareSide(int)`: `ceil(sqrt(f64(count)))` as `i32`.
///
/// # Panics
///
/// Panics if `count < 0`.
#[inline]
pub fn smallest_square_side(count: i32) -> i32 {
    assert!(count >= 0, "count must be non-negative, got {count}");
    ceil_f64(f64::from(count).sqrt())
}

/// `Mth.binarySearch(from, to, predicate)`: lower-bound binary search returning the first index
/// in `[from, to)` where `predicate(i)` is `true`, or `to` if none matches.
#[inline]
pub fn binary_search<F>(mut from: i32, to: i32, mut predicate: F) -> i32
where
    F: FnMut(i32) -> bool,
{
    let mut len = to.wrapping_sub(from);
    while len > 0 {
        let half = len / 2;
        let mid = from.wrapping_add(half);
        if predicate(mid) {
            len = half;
        } else {
            from = mid.wrapping_add(1);
            len -= half + 1;
        }
    }
    from
}

/// Iterator returned by [`out_from_origin`] and [`out_from_origin_step`].
///
/// Starts at `clamp_i32(origin, lower, upper)` and expands symmetrically outward by `step_size`,
/// visiting the positive offset `o + d` before the negative offset `o - d` at each distance `d`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutFromOrigin {
    origin: i64,
    lower: i64,
    upper: i64,
    step_size: i64,
    cursor: i64,
}

impl OutFromOrigin {
    /// Creates a new outward iterator.
    ///
    /// # Panics
    ///
    /// Panics if `lower > upper` or `step_size < 1`.
    #[inline]
    pub fn new(origin: i32, lower: i32, upper: i32, step_size: i32) -> Self {
        assert!(lower <= upper, "lower ({lower}) must be <= upper ({upper})");
        assert!(step_size >= 1, "step_size must be >= 1, got {step_size}");
        let clamped = i64::from(clamp_i32(origin, lower, upper));
        Self {
            origin: clamped,
            lower: i64::from(lower),
            upper: i64::from(upper),
            step_size: i64::from(step_size),
            cursor: clamped,
        }
    }
}

impl Iterator for OutFromOrigin {
    type Item = i32;

    #[inline]
    fn next(&mut self) -> Option<i32> {
        let d = (self.origin - self.cursor).abs();
        if self.origin - d < self.lower && self.origin + d > self.upper {
            return None;
        }

        let current = self.cursor as i32;
        let prev_neg = self.cursor <= self.origin;
        let can_pos = self.origin + d + self.step_size <= self.upper;

        if !prev_neg || !can_pos {
            let a = self.origin - d - if prev_neg { self.step_size } else { 0 };
            if a >= self.lower {
                self.cursor = a;
                return Some(current);
            }
        }
        self.cursor = self.origin + d + self.step_size;
        Some(current)
    }
}

/// `Mth.outFromOrigin(origin, lower, upper)` with default step size `1`.
///
/// # Panics
///
/// Panics if `lower > upper`.
#[inline]
pub fn out_from_origin(origin: i32, lower: i32, upper: i32) -> OutFromOrigin {
    OutFromOrigin::new(origin, lower, upper, 1)
}

/// `Mth.outFromOrigin(origin, lower, upper, stepSize)`.
///
/// # Panics
///
/// Panics if `lower > upper` or `step_size < 1`.
#[inline]
pub fn out_from_origin_step(origin: i32, lower: i32, upper: i32, step_size: i32) -> OutFromOrigin {
    OutFromOrigin::new(origin, lower, upper, step_size)
}
