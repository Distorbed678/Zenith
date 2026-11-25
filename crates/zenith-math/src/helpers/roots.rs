//! Roots and bit-level Newton-Raphson fast inverse roots (`Mth`).
//!
//! Implements Section 2 of `docs/specs/math/java-math-helpers.md`.

/// `f32(sqrt(2.0)) = 1.4142135_f32` (`0x3FB504F3`).
pub const SQRT_OF_TWO: f32 = f32::from_bits(0x3FB5_04F3);

/// 64-bit magic constant for [`fast_inv_sqrt`] (`6910469410427058090`, `0x5FE6EB50C7B537AA`).
pub const FAST_INV_SQRT_MAGIC: i64 = 0x5FE6_EB50_C7B5_37AA;

/// 32-bit magic constant for [`fast_inv_cube_root`] (`1419967116`, `0x54A2FA8C`).
pub const FAST_INV_CUBE_ROOT_MAGIC: i32 = 0x54A2_FA8C;

/// `f32` `2/3` constant `0.6666667_f32` (`0x3F2AAAAB`) used in [`fast_inv_cube_root`].
pub const FAST_INV_CUBE_ROOT_TWO_THIRDS: f32 = f32::from_bits(0x3F2A_AAAB);

/// `Mth.sqrt(float)`: widens `x` to `f64`, takes IEEE-754 square root, and narrows to `f32`.
#[inline]
pub fn sqrt(x: f32) -> f32 {
    f64::from(x).sqrt() as f32
}

/// `Mth.invSqrt(float)`: `1.0_f32 / f32(sqrt(f64(x)))`.
///
/// Narrows the square root to `f32` **before** dividing `1.0_f32` in `f32`, matching JOML's
/// `invsqrt(float)`.
#[inline]
pub fn inv_sqrt_f32(x: f32) -> f32 {
    1.0_f32 / (f64::from(x).sqrt() as f32)
}

/// `Mth.invSqrt(double)`: `1.0 / sqrt(x)` in `f64`.
#[inline]
pub fn inv_sqrt_f64(x: f64) -> f64 {
    1.0_f64 / x.sqrt()
}

/// `Mth.fastInvSqrt(double)`: 64-bit IEEE-754 bit-hack with one Newton-Raphson refinement step.
///
/// Uses raw bit reinterpretation (no `NaN` canonicalization) and signed arithmetic right shift
/// `>> 1`. At `x = +0.0`, returns the finite value `0x5FF1307C95C7E9C0` (`1.4404511704572167e+154`).
#[inline]
pub fn fast_inv_sqrt(x: f64) -> f64 {
    let h = 0.5_f64 * x;
    let mut i = x.to_bits() as i64;
    i = FAST_INV_SQRT_MAGIC.wrapping_sub(i >> 1);
    let y = f64::from_bits(i as u64);
    y * (1.5_f64 - ((h * y) * y))
}

/// `Mth.fastInvCubeRoot(float)`: 32-bit IEEE-754 bit-hack with two Newton-Raphson steps.
///
/// Matches `Float.floatToIntBits` (canonicalizing `NaN` to `0x7FC00000`), signed truncating
/// division by 3, and two `f32` iterations using [`FAST_INV_CUBE_ROOT_TWO_THIRDS`].
#[inline]
pub fn fast_inv_cube_root(x: f32) -> f32 {
    let raw = if x.is_nan() {
        0x7FC0_0000_i32
    } else {
        x.to_bits() as i32
    };
    let i = FAST_INV_CUBE_ROOT_MAGIC.wrapping_sub(raw / 3);
    let mut y = f32::from_bits(i as u32);
    y = (FAST_INV_CUBE_ROOT_TWO_THIRDS * y) + (1.0_f32 / (((3.0_f32 * y) * y) * x));
    y = (FAST_INV_CUBE_ROOT_TWO_THIRDS * y) + (1.0_f32 / (((3.0_f32 * y) * y) * x));
    y
}
