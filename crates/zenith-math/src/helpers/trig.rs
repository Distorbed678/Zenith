//! Lookup-table trigonometry (`Mth.sin`, `Mth.cos`, `Mth.atan2`) and scalar angle constants.
//!
//! Implements Section 1 of `docs/specs/math/java-math-helpers.md`.

use std::sync::LazyLock;

use crate::helpers::intrinsics::{java_math_asin, java_math_cos, java_math_sin};
use crate::helpers::roots::fast_inv_sqrt;

/// `Mth.PI`: `f32(pi) = 3.1415927_f32` (`0x40490FDB`).
pub const PI: f32 = f32::from_bits(0x4049_0FDB);

/// `Mth.HALF_PI`: `f32(pi / 2) = 1.5707964_f32` (`0x3FC90FDB`).
pub const HALF_PI: f32 = f32::from_bits(0x3FC9_0FDB);

/// `Mth.TWO_PI`: `f32(2 * pi) = 6.2831855_f32` (`0x40C90FDB`).
pub const TWO_PI: f32 = f32::from_bits(0x40C9_0FDB);

/// `Mth.DEG_TO_RAD`: `f32(pi / 180) = 0.017453292_f32` (`0x3C8EFA35`).
pub const DEG_TO_RAD: f32 = f32::from_bits(0x3C8E_FA35);

/// `Mth.RAD_TO_DEG`: `180.0_f32 / PI = 57.295776_f32` (`0x42652EE0`).
///
/// Note that this is 1 ULP below `(180.0 / PI_f64) as f32` (`0x42652EE1`).
pub const RAD_TO_DEG: f32 = 180.0_f32 / PI;

/// Number of entries in the sine lookup table (`65,536 = 2^16`).
pub const SIN_TABLE_LEN: usize = 65_536;

/// Index mask for the sine lookup table (`0xFFFF`).
pub const SIN_TABLE_MASK: i64 = 0xFFFF;

/// Quarter-turn offset in sine-table units (`16,384.0`).
pub const COS_OFFSET: f64 = 16_384.0;

/// Scale factor `K = 65536.0 / (2 * pi) = 10430.378350470453` (`0x40C45F306DC9C883`).
pub const SIN_TABLE_SCALE: f64 = 65_536.0_f64 / (2.0_f64 * std::f64::consts::PI);

/// IEEE-754 quantization bias `2^44` (`0x42B0000000000000`) used in [`atan2`].
pub const ATAN2_FRAC_BIAS: f64 = f64::from_bits(0x42B0_0000_0000_0000);

/// `1.0 / 6.0` (`0x3FC5555555555555`) used in the 3rd-order arcsine residual in [`atan2`].
pub const ATAN2_ONE_SIXTH: f64 = 1.0_f64 / 6.0_f64;

/// Number of entries in `ASIN_TAB` and `COS_TAB` (`257`).
pub const ATAN2_TABLE_LEN: usize = 257;

static SIN_TABLE: LazyLock<Box<[f32; SIN_TABLE_LEN]>> = LazyLock::new(|| {
    let mut table = Box::new([0.0_f32; SIN_TABLE_LEN]);
    for (i, slot) in table.iter_mut().enumerate() {
        *slot = java_math_sin((i as f64) / SIN_TABLE_SCALE) as f32;
    }
    table
});

struct Atan2Tables {
    asin: [f64; ATAN2_TABLE_LEN],
    cos: [f64; ATAN2_TABLE_LEN],
}

static ATAN2_TABLES: LazyLock<Atan2Tables> = LazyLock::new(|| {
    let mut asin = [0.0_f64; ATAN2_TABLE_LEN];
    let mut cos = [0.0_f64; ATAN2_TABLE_LEN];
    for k in 0..ATAN2_TABLE_LEN {
        let v = (k as f64) / 256.0_f64;
        let a = java_math_asin(v);
        asin[k] = a;
        cos[k] = java_math_cos(a);
    }
    Atan2Tables { asin, cos }
});

/// Returns a reference to the lazily initialized 65,536-entry `f32` sine lookup table.
#[inline]
pub fn sin_table() -> &'static [f32; SIN_TABLE_LEN] {
    &SIN_TABLE
}

/// Returns a reference to the lazily initialized 257-entry `ASIN_TAB` used by [`atan2`].
#[inline]
pub fn asin_table() -> &'static [f64; ATAN2_TABLE_LEN] {
    &ATAN2_TABLES.asin
}

/// Returns a reference to the lazily initialized 257-entry `COS_TAB` used by [`atan2`].
#[inline]
pub fn cos_table() -> &'static [f64; ATAN2_TABLE_LEN] {
    &ATAN2_TABLES.cos
}

/// `Mth.sin(double)`: lookup-table sine indexed by `trunc_i64(x * K) & 0xFFFF`.
#[inline]
pub fn sin(x: f64) -> f32 {
    let idx = ((x * SIN_TABLE_SCALE) as i64 & SIN_TABLE_MASK) as usize;
    SIN_TABLE[idx]
}

/// `Mth.cos(double)`: lookup-table cosine indexed by `trunc_i64(x * K + 16384.0) & 0xFFFF`.
#[inline]
pub fn cos(x: f64) -> f32 {
    let idx = ((x * SIN_TABLE_SCALE + COS_OFFSET) as i64 & SIN_TABLE_MASK) as usize;
    SIN_TABLE[idx]
}

/// Fallible version of [`atan2`] returning `None` if the quantized table index falls outside
/// `0..=256` (where Java throws `ArrayIndexOutOfBoundsException`, e.g., on `x^2 + y^2 = +inf`
/// or extreme subnormals).
#[inline]
pub fn try_atan2(mut y: f64, mut x: f64) -> Option<f64> {
    let d2 = x * x + y * y;
    if d2.is_nan() {
        return Some(f64::NAN);
    }

    let neg_y = y < 0.0_f64;
    if neg_y {
        y = -y;
    }
    let neg_x = x < 0.0_f64;
    if neg_x {
        x = -x;
    }

    let steep = y > x;
    if steep {
        std::mem::swap(&mut x, &mut y);
    }

    let r = fast_inv_sqrt(d2);
    x *= r;
    y *= r;

    let y_p = ATAN2_FRAC_BIAS + y;
    let idx = y_p.to_bits() as i32;
    if !(0..ATAN2_TABLE_LEN as i32).contains(&idx) {
        return None;
    }

    let tables = &*ATAN2_TABLES;
    let phi = tables.asin[idx as usize];
    let c_phi = tables.cos[idx as usize];
    let s_phi = y_p - ATAN2_FRAC_BIAS;
    let s_d = y * c_phi - x * s_phi;
    let d = ((6.0_f64 + s_d * s_d) * s_d) * ATAN2_ONE_SIXTH;
    let mut theta = phi + d;

    if steep {
        theta = (std::f64::consts::PI / 2.0_f64) - theta;
    }
    if neg_x {
        theta = std::f64::consts::PI - theta;
    }
    if neg_y {
        theta = -theta;
    }

    Some(theta)
}

/// `Mth.atan2(double, double)`: fast 257-entry table + 3rd-order Taylor arctangent built on
/// [`fast_inv_sqrt`].
///
/// # Panics
///
/// Panics if the quantized table index falls outside `0..=256` (mirroring Java's
/// `ArrayIndexOutOfBoundsException` on overflow/subnormal inputs). Use [`try_atan2`] for a
/// non-panicking check.
#[inline]
pub fn atan2(y: f64, x: f64) -> f64 {
    try_atan2(y, x).expect("atan2 lookup table index out of bounds (0..=256)")
}
