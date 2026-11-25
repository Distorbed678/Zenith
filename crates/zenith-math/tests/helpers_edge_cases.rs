//! Tests covering all 28 edge cases, exact IEEE-754 / integer constant bit patterns, and
//! lookup-table spot values from `docs/specs/math/java-math-helpers.md`.

use zenith_math::helpers::angles::{
    rot_lerp_rad, wrap_degrees_f32, wrap_degrees_f64, wrap_degrees_i32,
};
use zenith_math::helpers::color::{
    FULL_BRIGHT, FULL_SKY, GREYSCALE_WEIGHT_B, GREYSCALE_WEIGHT_G, GREYSCALE_WEIGHT_R,
    LINEAR_TO_SRGB_EXPONENT, LINEAR_TO_SRGB_THRESHOLD, SRGB_TO_LINEAR_THRESHOLD, alpha,
    as_8bit_channel, color_argb, greyscale, hsv_to_argb, hsv_to_rgb, linear_to_srgb_channel,
    linear_to_srgb_table, multiply_alpha, scale_rgb_f32, set_brightness, srgb_to_linear_table,
    try_hsv_to_argb,
};
use zenith_math::helpers::geom::{
    AABB_EPSILON, Aabb, Direction, LP_VEC3_ABS_MAX_VALUE, LP_VEC3_ABS_MIN_VALUE,
    VEC2_NORMALIZE_EPSILON, Vec2, Vec3, Vec3i, ray_intersects_aabb,
};
use zenith_math::helpers::hash::{
    CHUNK_HASH_A, CHUNK_HASH_C, CHUNK_HASH_Z_XOR, LCG_INCREMENT, LCG_MULTIPLIER, MURMUR3_M1,
    MURMUR3_M2, lcg_next, next_int, random_between_inclusive,
};
use zenith_math::helpers::interp::{
    BACK_2C2_PLUS_2, BACK_C1, BACK_C2, BACK_C2_PLUS_1, BACK_C3, BOUNCE_OFFSET_1, BOUNCE_OFFSET_2,
    BOUNCE_OFFSET_3, BOUNCE_THRESHOLD_1, BOUNCE_THRESHOLD_2, BOUNCE_THRESHOLD_3, CubicBezier,
    ELASTIC_C4, ELASTIC_C5, Ease, clamped_lerp_f32, clamped_lerp_f64, lerp_f32, lerp_f64,
};
use zenith_math::helpers::packing::{
    INVALID_CHUNK_POS, block_as_long, block_get_x, block_get_y, block_get_z, block_offset,
    chunk_pack, section_as_long, section_get_x, section_get_y, section_get_z,
};
use zenith_math::helpers::roots::{
    FAST_INV_CUBE_ROOT_MAGIC, FAST_INV_CUBE_ROOT_TWO_THIRDS, FAST_INV_SQRT_MAGIC, SQRT_OF_TWO,
    fast_inv_cube_root, fast_inv_sqrt, inv_sqrt_f32,
};
use zenith_math::helpers::rounding::{
    CONSTANTS_EPSILON, DE_BRUIJN_MULTIPLIER, EPSILON, EPSILON_F64, MULTIPLY_DE_BRUIJN_BIT_POSITION,
    ceillog2, clamp_f32, clamp_f64, clamp_i32, equal_f64, frac_f32, frac_f64, is_power_of_two_i32,
    is_power_of_two_i64, log2, positive_ceil_div_i32, quantize, sign,
    smallest_encompassing_power_of_two,
};
use zenith_math::helpers::trig::{
    ATAN2_FRAC_BIAS, ATAN2_ONE_SIXTH, DEG_TO_RAD, HALF_PI, PI, RAD_TO_DEG, SIN_TABLE_SCALE, TWO_PI,
    atan2, cos, sin, sin_table, try_atan2,
};
use zenith_math::{LegacyRandom, RandomSource, XoroshiroRandom};

#[test]
fn constants_match_spec_bit_patterns() {
    // Scalar floating-point constants
    assert_eq!(PI.to_bits(), 0x4049_0FDB);
    assert_eq!(HALF_PI.to_bits(), 0x3FC9_0FDB);
    assert_eq!(TWO_PI.to_bits(), 0x40C9_0FDB);
    assert_eq!(DEG_TO_RAD.to_bits(), 0x3C8E_FA35);
    assert_eq!(RAD_TO_DEG.to_bits(), 0x4265_2EE0);
    assert_eq!(EPSILON.to_bits(), 0x3727_C5AC);
    assert_eq!(EPSILON_F64.to_bits(), 0x3EE4_F8B5_8000_0000);
    assert_eq!(SQRT_OF_TWO.to_bits(), 0x3FB5_04F3);
    assert_eq!(CONSTANTS_EPSILON.to_bits(), 0x3586_37BD);
    assert_eq!(VEC2_NORMALIZE_EPSILON.to_bits(), 0x38D1_B717);
    assert_eq!(SIN_TABLE_SCALE.to_bits(), 0x40C4_5F30_6DC9_C883);
    assert_eq!(ATAN2_FRAC_BIAS.to_bits(), 0x42B0_0000_0000_0000);
    assert_eq!(ATAN2_ONE_SIXTH.to_bits(), 0x3FC5_5555_5555_5555);
    assert_eq!(FAST_INV_CUBE_ROOT_TWO_THIRDS.to_bits(), 0x3F2A_AAAB);
    assert_eq!(AABB_EPSILON.to_bits(), 0x3E7A_D7F2_9ABC_AF48);
    assert_eq!(LP_VEC3_ABS_MAX_VALUE.to_bits(), 0x420F_FFFF_FFF8_0000);
    assert_eq!(LP_VEC3_ABS_MIN_VALUE.to_bits(), 0x3F00_0040_0100_0400);
    assert_eq!(SRGB_TO_LINEAR_THRESHOLD.to_bits(), 0x3D25_AEE6);
    assert_eq!(LINEAR_TO_SRGB_THRESHOLD.to_bits(), 0x3B4D_2E1C);
    assert_eq!(LINEAR_TO_SRGB_EXPONENT.to_bits(), 0x3FDA_AAAA_AAAA_AAAB);
    assert_eq!(GREYSCALE_WEIGHT_R.to_bits(), 0x3E99_999A);
    assert_eq!(GREYSCALE_WEIGHT_G.to_bits(), 0x3F17_0A3D);
    assert_eq!(GREYSCALE_WEIGHT_B.to_bits(), 0x3DE1_47AE);
    assert_eq!(ELASTIC_C4.to_bits(), 0x4000_C152_4000_0000);
    assert_eq!(
        f64::from((2.0_f64 * std::f64::consts::PI / 3.0_f64) as f32).to_bits(),
        ELASTIC_C4.to_bits()
    );
    assert_eq!(ELASTIC_C5.to_bits(), 0x3FF6_5718_6000_0000);
    assert_eq!(
        f64::from((PI * 4.0_f32) / 9.0_f32).to_bits(),
        ELASTIC_C5.to_bits()
    );
    assert_eq!(BACK_C1.to_bits(), 0x3FD9_CD60);
    assert_eq!(BACK_C3.to_bits(), 0x402C_E6B0);
    assert_eq!(BACK_C2.to_bits(), 0x4026_12FF);
    assert_eq!(BACK_C2_PLUS_1.to_bits(), 0x4066_12FF);
    assert_eq!(BACK_2C2_PLUS_2.to_bits(), 0x40E6_12FF);
    assert_eq!(BOUNCE_THRESHOLD_1.to_bits(), 0x3EBA_2E8C);
    assert_eq!(BOUNCE_THRESHOLD_2.to_bits(), 0x3F3A_2E8C);
    assert_eq!(BOUNCE_THRESHOLD_3.to_bits(), 0x3FED_1745_D174_5D17);
    assert_eq!(BOUNCE_OFFSET_1.to_bits(), 0x3F0B_A2E9);
    assert_eq!(BOUNCE_OFFSET_2.to_bits(), 0x3F51_745D);
    assert_eq!(BOUNCE_OFFSET_3.to_bits(), 0x3F74_5D17);

    // Integer constants
    assert_eq!(FAST_INV_SQRT_MAGIC, 6_910_469_410_427_058_090);
    assert_eq!(FAST_INV_CUBE_ROOT_MAGIC, 1_419_967_116);
    assert_eq!(DE_BRUIJN_MULTIPLIER, 125_613_361);
    assert_eq!(MURMUR3_M1, -2_048_144_789);
    assert_eq!(MURMUR3_M2, -1_028_477_387);
    assert_eq!(LCG_MULTIPLIER, 6_364_136_223_846_793_005);
    assert_eq!(LCG_INCREMENT, 1_442_695_040_888_963_407);
    assert_eq!(CHUNK_HASH_A, 1_664_525);
    assert_eq!(CHUNK_HASH_C, 1_013_904_223);
    assert_eq!(CHUNK_HASH_Z_XOR, -559_038_737);
    assert_eq!(INVALID_CHUNK_POS, 0x001C_9C7A_001C_9C7A);
    assert_eq!(INVALID_CHUNK_POS, chunk_pack(1_875_066, 1_875_066));
    assert_eq!(FULL_BRIGHT, 15_728_880);
    assert_eq!(FULL_SKY, 15_728_640);

    // De Bruijn bit-position table
    assert_eq!(
        MULTIPLY_DE_BRUIJN_BIT_POSITION,
        [
            0, 1, 28, 2, 29, 14, 24, 3, 30, 22, 20, 15, 25, 17, 4, 8, 31, 27, 13, 23, 21, 19, 16,
            7, 26, 12, 18, 6, 11, 5, 10, 9,
        ]
    );
}

#[test]
fn ec01_sin_cos_truncation_toward_zero_and_saturation() {
    let tbl = sin_table();
    // Small negative in (-1/K, 0) truncates to 0, NOT 65535.
    let tiny_neg = -0.5 / SIN_TABLE_SCALE;
    assert_eq!(sin(tiny_neg).to_bits(), tbl[0].to_bits());
    assert_ne!(sin(tiny_neg).to_bits(), tbl[65_535].to_bits());

    // Overflowing i64 saturates to i64::MAX (& 0xFFFF = 65535) and i64::MIN (& 0xFFFF = 0).
    assert_eq!(sin(1.0e30).to_bits(), tbl[65_535].to_bits());
    assert_eq!(sin(-1.0e30).to_bits(), tbl[0].to_bits());
    assert_eq!(sin(f64::INFINITY).to_bits(), tbl[65_535].to_bits());
    assert_eq!(sin(f64::NEG_INFINITY).to_bits(), tbl[0].to_bits());
    assert_eq!(sin(f64::NAN).to_bits(), tbl[0].to_bits());
    assert_eq!(cos(f64::NAN).to_bits(), tbl[0].to_bits());
    assert_eq!(cos(0.0).to_bits(), tbl[16_384].to_bits());
}

#[test]
fn ec02_sin_table_spot_values() {
    let tbl = sin_table();
    assert_eq!(tbl[0].to_bits(), 0.0_f32.to_bits());
    assert_eq!(tbl[32_768].to_bits(), 0x250D_3132);
    assert_eq!(tbl[65_535].to_bits(), 0xB8C9_0FDB);
}

#[test]
fn ec03_rad_to_deg_one_ulp_below_f64_cast() {
    assert_eq!(RAD_TO_DEG.to_bits(), 0x4265_2EE0);
    let from_f64 = (180.0_f64 / std::f64::consts::PI) as f32;
    assert_eq!(from_f64.to_bits(), 0x4265_2EE1);
    assert_eq!(RAD_TO_DEG.to_bits() + 1, from_f64.to_bits());
}

#[test]
fn ec04_inv_sqrt_f32_narrows_before_dividing() {
    // Find an input where 1.0f / f32(sqrt(f64(x))) differs by 1 ULP from f32(1.0 / sqrt(f64(x))).
    let mut found_diff = false;
    for k in 1..1000 {
        let x = (k as f32) * 0.37_f32;
        let actual = inv_sqrt_f32(x);
        let expected = 1.0_f32 / (f64::from(x).sqrt() as f32);
        let alt = (1.0_f64 / f64::from(x).sqrt()) as f32;
        assert_eq!(actual.to_bits(), expected.to_bits());
        if actual.to_bits() != alt.to_bits() {
            found_diff = true;
        }
    }
    assert!(found_diff);
}

#[test]
fn ec05_fast_inv_sqrt_zero_is_finite_and_preserves_raw_bits() {
    assert_eq!(fast_inv_sqrt(0.0).to_bits(), 0x5FF1_307C_95C7_E9C0);
    assert!(fast_inv_sqrt(0.0).is_finite());
    assert_eq!(atan2(0.0, 0.0).to_bits(), 0.0_f64.to_bits());

    // Raw NaN payload handling: does not canonicalize before bit-hack.
    let quiet_nan_1 = f64::from_bits(0x7FF8_0000_0000_0001);
    let quiet_nan_2 = f64::from_bits(0x7FF8_0000_0000_0002);
    assert!(fast_inv_sqrt(quiet_nan_1).is_nan());
    assert!(fast_inv_sqrt(quiet_nan_2).is_nan());
}

#[test]
fn ec06_atan2_ignores_neg_zero_and_reports_oob_on_overflow() {
    // Signed zero fails `< 0.0`, so atan2(-0.0, -0.0) == +0.0 (not -PI).
    assert_eq!(atan2(-0.0, -0.0).to_bits(), 0.0_f64.to_bits());
    assert_eq!(atan2(-0.0, 1.0).to_bits(), 0.0_f64.to_bits());
    assert_eq!(atan2(1.0, -0.0).to_bits(), atan2(1.0, 0.0).to_bits());

    // NaN propagates.
    assert!(atan2(f64::NAN, 1.0).is_nan());
    assert!(atan2(1.0, f64::NAN).is_nan());

    // Overflowing x^2 + y^2 = +inf makes fast_inv_sqrt return -inf and y_p = -inf (low 32 bits 0),
    // yielding -inf when y > 0 and NaN when y == 0; extreme subnormals stay finite.
    assert_eq!(try_atan2(1.0e200, 1.0e200), Some(f64::NEG_INFINITY));
    assert!(try_atan2(0.0, 1.0e200).is_some_and(f64::is_nan));
    assert!(try_atan2(1.0e-154, 1.0e-154).is_some_and(f64::is_finite));
}

#[test]
fn ec07_fast_inv_cube_root_canonicalizes_nan_and_matches_two_newton_steps() {
    let nan_a = f32::from_bits(0x7FC0_0001);
    assert!(fast_inv_cube_root(nan_a).is_nan());

    // Verify exact bit-hack (signed truncating division by 3) + two Newton steps on + and - inputs.
    for x in [27.0_f32, 8.0_f32, 0.125_f32, -8.0_f32, -27.0_f32] {
        let i = FAST_INV_CUBE_ROOT_MAGIC.wrapping_sub((x.to_bits() as i32) / 3);
        let mut y = f32::from_bits(i as u32);
        y = (FAST_INV_CUBE_ROOT_TWO_THIRDS * y) + (1.0_f32 / (((3.0_f32 * y) * y) * x));
        y = (FAST_INV_CUBE_ROOT_TWO_THIRDS * y) + (1.0_f32 / (((3.0_f32 * y) * y) * x));
        assert_eq!(fast_inv_cube_root(x).to_bits(), y.to_bits());
    }
    assert!((fast_inv_cube_root(27.0_f32) - (1.0_f32 / 3.0_f32)).abs() < 1.0e-5_f32);
    assert!((fast_inv_cube_root(8.0_f32) - 0.5_f32).abs() < 1.0e-5_f32);
}

#[test]
fn ec08_sign_returns_minus_one_for_nan() {
    assert_eq!(sign(0.0), 0);
    assert_eq!(sign(-0.0), 0);
    assert_eq!(sign(1.0e-300), 1);
    assert_eq!(sign(-1.0e-300), -1);
    assert_eq!(sign(f64::NAN), -1);
}

#[test]
fn ec09_equal_f64_uses_widened_f32_epsilon() {
    // 9.9999998e-6 is < 1e-5_f64, but > f64(1e-5_f32) == 9.999999747378752e-6.
    assert!(!equal_f64(0.0, 9.999_999_8e-6));
    assert!(equal_f64(0.0, 9.999_999_7e-6));
    assert!(!equal_f64(0.0, EPSILON_F64));
    assert!(equal_f64(0.0, EPSILON_F64.next_down()));
}

#[test]
fn ec10_clamp_float_quirks_vs_int_clamp() {
    // Float clamp preserves -0.0 when lo = +0.0, hi >= 0.0.
    assert_eq!(clamp_f32(-0.0, 0.0, 1.0).to_bits(), (-0.0_f32).to_bits());
    assert_eq!(clamp_f64(-0.0, 0.0, 1.0).to_bits(), (-0.0_f64).to_bits());

    // Inverted bounds lo > hi: float returns lo when v < lo, integer always returns hi.
    assert_eq!(clamp_f32(0.0, 10.0, 5.0), 10.0);
    assert_eq!(clamp_f64(0.0, 10.0, 5.0), 10.0);
    assert_eq!(clamp_i32(0, 10, 5), 5);
    assert_eq!(clamp_i32(20, 10, 5), 5);

    // NaN propagates.
    assert!(clamp_f32(f32::NAN, 0.0, 1.0).is_nan());
    assert!(clamp_f64(f64::NAN, 0.0, 1.0).is_nan());
}

#[test]
fn ec11_frac_uses_integer_floor_and_saturates_beyond_int_range() {
    assert_eq!(frac_f32(1.25), 0.25);
    assert_eq!(frac_f32(-1.25), 0.75);
    assert_eq!(frac_f64(-1.25), 0.75);

    // Beyond i32 range for f32: floor saturates to i32::MAX (2^31 - 1, casts to 2^31 in f32).
    let huge_f32 = 1.0e12_f32;
    assert_eq!(frac_f32(huge_f32), huge_f32 - (i32::MAX as f32));
    assert_ne!(frac_f32(huge_f32), 0.0_f32);

    // Beyond i64 range for f64: floor saturates to i64::MAX.
    let huge_f64 = 1.0e20_f64;
    assert_eq!(frac_f64(huge_f64), huge_f64 - (i64::MAX as f64));
    assert_ne!(frac_f64(huge_f64), 0.0_f64);
}

#[test]
fn ec12_power_of_two_and_log2_quirks() {
    assert!(is_power_of_two_i32(i32::MIN));
    assert!(is_power_of_two_i64(i64::MIN));
    assert!(!is_power_of_two_i32(0));
    assert!(!is_power_of_two_i64(0));

    assert_eq!(smallest_encompassing_power_of_two(0), 0);
    assert_eq!(smallest_encompassing_power_of_two(-1), 0);
    assert_eq!(smallest_encompassing_power_of_two(i32::MIN + 1), 0);
    assert_eq!(smallest_encompassing_power_of_two(i32::MIN), i32::MIN);
    assert_eq!(smallest_encompassing_power_of_two((1 << 30) + 1), i32::MIN);
    assert_eq!(smallest_encompassing_power_of_two(i32::MAX), i32::MIN);
    assert_eq!(smallest_encompassing_power_of_two(30_000_000), 1 << 25);

    assert_eq!(ceillog2(i32::MIN), 31);
    assert_eq!(log2(i32::MIN), 31);
    assert_eq!(ceillog2(0), 0);
    assert_eq!(log2(0), -1);
    assert_eq!(ceillog2(-5), 0);
    assert_eq!(log2(-5), -1);
    assert_eq!(ceillog2(1), 0);
    assert_eq!(log2(1), 0);
    assert_eq!(ceillog2(3), 2);
    assert_eq!(log2(3), 1);
    assert_eq!(1 + log2(smallest_encompassing_power_of_two(30_000_000)), 26);
}

#[test]
fn ec13_positive_ceil_div_wrap_and_quantize_zero() {
    // -i32::MIN wraps to i32::MIN -> floorDiv(i32::MIN, 2) = -2^30 -> negated = +2^30!
    assert_eq!(positive_ceil_div_i32(i32::MIN, 2), 1 << 30);
    assert_eq!(positive_ceil_div_i32(7, 3), 3);
    assert_eq!(positive_ceil_div_i32(-7, 3), -2);

    // quantize(v, 0) returns 0 without panicking.
    assert_eq!(quantize(42.5, 0), 0);
    assert_eq!(quantize(-42.5, 0), 0);
    assert_eq!(quantize(0.0, 0), 0);
    assert_eq!(quantize(f64::NAN, 0), 0);
}

#[test]
fn ec14_lerp_and_clamped_lerp_endpoints_and_nan() {
    // Find (a, b) where a + 1.0 * (b - a) != b by 1 ULP.
    let a = 1.0_f32;
    let b = 1.0e-7_f32;
    let at_one = lerp_f32(1.0, a, b);
    assert_ne!(at_one.to_bits(), b.to_bits());
    // clamped_lerp does NOT short-circuit at alpha == 1.0.
    assert_eq!(clamped_lerp_f32(1.0, a, b).to_bits(), at_one.to_bits());
    assert_eq!(clamped_lerp_f32(1.1, a, b).to_bits(), b.to_bits());
    assert!(clamped_lerp_f32(f32::NAN, 0.0, 1.0).is_nan());

    let ad = 1.0_f64;
    let bd = 1.0e-16_f64;
    assert_ne!(lerp_f64(1.0, ad, bd).to_bits(), bd.to_bits());
    assert_eq!(
        clamped_lerp_f64(1.0, ad, bd).to_bits(),
        lerp_f64(1.0, ad, bd).to_bits()
    );
    assert!(clamped_lerp_f64(f64::NAN, 0.0, 1.0).is_nan());
}

#[test]
fn ec15_wrap_degrees_half_open_and_rot_lerp_rad_loop() {
    assert_eq!(wrap_degrees_i32(180), -180);
    assert_eq!(wrap_degrees_i32(-180), -180);
    assert_eq!(wrap_degrees_f32(180.0), -180.0);
    assert_eq!(wrap_degrees_f32(-180.0), -180.0);
    assert_eq!(wrap_degrees_f32(-0.0).to_bits(), (-0.0_f32).to_bits());
    assert_eq!(wrap_degrees_f64(180.0), -180.0);
    assert_eq!(wrap_degrees_f64(-180.0), -180.0);
    assert_eq!(wrap_degrees_f64(-0.0).to_bits(), (-0.0_f64).to_bits());
    assert!(wrap_degrees_f64(f64::INFINITY).is_nan());

    // rot_lerp_rad loops in f32: verify multi-turn difference matches exact loop.
    let from = 0.25_f32;
    let to = 0.25_f32 + 100.0_f32 * TWO_PI + 0.5_f32;
    let mut d = to - from;
    while d < -PI {
        d += TWO_PI;
    }
    while d >= PI {
        d -= TWO_PI;
    }
    assert_eq!(
        rot_lerp_rad(0.5, from, to).to_bits(),
        (from + 0.5_f32 * d).to_bits()
    );
}

#[test]
fn ec16_ease_mixed_precision_and_lookup_table_sine() {
    // Sine easings use lookup-table Mth.sin/Mth.cos, not exact Math.sin/cos.
    let x = 0.37_f32;
    assert_eq!(
        Ease::InSine.apply(x).to_bits(),
        (1.0_f32 - cos(f64::from(x * HALF_PI))).to_bits()
    );
    assert_eq!(
        Ease::OutSine.apply(x).to_bits(),
        sin(f64::from(x * HALF_PI)).to_bits()
    );

    // outBounce 3rd threshold is compared in f64 (0.9090909090909091):
    // 0.90909094_f32 > 0.9090909090909091, while 0.9090909_f32 == 0.9090908765792847 < threshold.
    let below = 0.909_090_9_f32;
    let above = 0.909_090_94_f32;
    assert!(f64::from(below) < BOUNCE_THRESHOLD_3);
    assert!(f64::from(above) >= BOUNCE_THRESHOLD_3);
    let expected_below = 7.5625_f32 * (below - BOUNCE_OFFSET_2).powi(2) + 0.9375_f32;
    let expected_above = 7.5625_f32 * (above - BOUNCE_OFFSET_3).powi(2) + 0.984375_f32;
    assert_eq!(
        Ease::OutBounce.apply(below).to_bits(),
        expected_below.to_bits()
    );
    assert_eq!(
        Ease::OutBounce.apply(above).to_bits(),
        expected_above.to_bits()
    );

    // inCirc narrows -sqrt(...) to f32 before adding 1.0f.
    let xc = 0.001_f32;
    let expected_in_circ = (-(f64::from(1.0_f32 - xc * xc).sqrt()) as f32) + 1.0_f32;
    assert_eq!(Ease::InCirc.apply(xc).to_bits(), expected_in_circ.to_bits());
}

#[test]
fn ec17_cubic_bezier_bisection_from_newton_exit_and_termination() {
    // Steep curve where Newton-Raphson hits low derivative and falls back to bisection.
    let steep = CubicBezier::new(0.0, 0.0, 0.0, 1.0);
    let t = steep.solve_t(0.5);
    assert!((steep.sample_x(t) - 0.5).abs() < 1.0e-4);

    // Degenerate / out-of-range inputs must terminate cleanly (no infinite loop).
    let flat = CubicBezier::new(0.0, 0.0, 1.0, 1.0);
    assert!(flat.apply(-0.5).is_finite());
    assert!(flat.apply(1.5).is_finite());
    let zero_slope = CubicBezier::new(0.0, 0.5, 0.0, 0.5);
    assert!(zero_slope.apply(0.999999).is_finite());
}

#[test]
fn ec18_next_int_vs_random_between_inclusive_consumption() {
    let mut r1 = LegacyRandom::new(42);
    let mut r2 = LegacyRandom::new(42);
    let mut r3 = LegacyRandom::new(42);

    // next_int(rng, k, k) does NOT advance rng.
    assert_eq!(next_int(&mut r1, 7, 7), 7);
    assert_eq!(r1.state(), r2.state());

    // random_between_inclusive(rng, k, k) DOES consume next_int_bounded(1).
    assert_eq!(random_between_inclusive(&mut r3, 7, 7), 7);
    assert_ne!(r3.state(), r2.state());
    let _ = r2.next_int_bounded(1);
    assert_eq!(r3.state(), r2.state());

    // Same for XoroshiroRandom.
    let mut x1 = XoroshiroRandom::new(42);
    let mut x2 = XoroshiroRandom::new(42);
    let mut x3 = XoroshiroRandom::new(42);
    assert_eq!(next_int(&mut x1, 7, 7), 7);
    assert_eq!(x1.state(), x2.state());
    assert_eq!(random_between_inclusive(&mut x3, 7, 7), 7);
    let _ = x2.next_int_bounded(1);
    assert_eq!(x3.state(), x2.state());
}

#[test]
fn ec19_lcg_next_is_quadratic() {
    let r: i64 = 123_456_789;
    let c: i64 = -987_654_321;
    let quadratic = r
        .wrapping_mul(r.wrapping_mul(LCG_MULTIPLIER).wrapping_add(LCG_INCREMENT))
        .wrapping_add(c);
    let linear = r.wrapping_mul(LCG_MULTIPLIER).wrapping_add(c);
    assert_eq!(lcg_next(r, c), quadratic);
    assert_ne!(lcg_next(r, c), linear);
}

#[test]
fn ec20_hsv_to_rgb_alpha_zero_negative_throw_and_hue_ge_one() {
    // hsv_to_rgb sets alpha = 0 (not 255).
    let red = hsv_to_rgb(0.0, 1.0, 1.0);
    assert_eq!(alpha(red), 0);
    assert_eq!(red, 0x00FF_0000);

    // Negative hue <= -1/6 fails sector check.
    assert!(try_hsv_to_argb(-0.2, 1.0, 1.0, 255).is_none());
    // Tiny negative hue in (-1/6, 0) truncates to sector 0.
    assert!(try_hsv_to_argb(-0.05, 1.0, 1.0, 255).is_some());

    // hue >= 1.0 wraps sector hi % 6 while leaving f = hue * 6 - hi >= 6.0!
    // At hue = 1.0, s = 1.0, v = 1.0: hi = 6 % 6 = 0, f = 6.0 -> t = 1 - (1 - 6)*1 = 6.0 -> clamped to 255!
    // So (v, t, p) = (1.0, 6.0, 0.0) -> (255, 255, 0) yellow instead of red!
    let wrapped_yellow = hsv_to_argb(1.0, 1.0, 1.0, 255);
    assert_eq!(wrapped_yellow, color_argb(255, 255, 255, 0));
}

#[test]
fn ec21_argb_truncation_floor_round_and_multiply_alpha_zero() {
    // as_8bit_channel uses unclamped floor.
    assert_eq!(as_8bit_channel(-0.01), -3);
    assert_eq!(as_8bit_channel(2.0), 510);

    // linear_to_srgb_channel uses floor, not round.
    let l2s = linear_to_srgb_table();
    assert_eq!(linear_to_srgb_channel(0.9999 / 1023.0), i32::from(l2s[0]));

    // scale_rgb_f32 and greyscale truncate toward zero.
    let c = color_argb(200, 10, 10, 10);
    assert_eq!(scale_rgb_f32(c, 0.99, 0.99, 0.99), color_argb(200, 9, 9, 9));
    // 10*0.3 + 10*0.59 + 10*0.11 = 10.0, try (1, 1, 1): 0.3 + 0.59 + 0.11 = 1.0; try (2, 1, 1): 0.6 + 0.59 + 0.11 = 1.3 -> 1
    assert_eq!(
        greyscale(color_argb(128, 2, 1, 1)),
        color_argb(128, 1, 1, 1)
    );

    // multiply_alpha(c, 0.0) zeroes RGB as well!
    assert_eq!(multiply_alpha(color_argb(255, 123, 45, 67), 0.0), 0);
    assert_eq!(multiply_alpha(color_argb(255, 123, 45, 67), -0.5), 0);

    // SRGB_TO_LINEAR and LINEAR_TO_SRGB endpoints.
    let s2l = srgb_to_linear_table();
    assert_eq!(s2l[0], 0);
    assert_eq!(s2l[255], 1023);
    assert_eq!(l2s[0], 0);
    assert_eq!(l2s[1023], 255);

    // set_brightness uses Math.round.
    let sb = set_brightness(color_argb(200, 100, 100, 100), 0.5);
    assert_eq!(sb, color_argb(200, 128, 128, 128));
}

#[test]
fn ec22_block_and_section_pos_signed_wrapping_and_offset_isolation() {
    // BlockPos Y wraps modulo 4096 into [-2048, 2047] without bleeding into Z.
    let p = block_as_long(10, 2047, -5);
    let p_plus = block_offset(p, 0, 1, 0);
    assert_eq!(block_get_x(p_plus), 10);
    assert_eq!(block_get_y(p_plus), -2048);
    assert_eq!(block_get_z(p_plus), -5);

    // X/Z wrap modulo 2^26.
    let px = block_as_long(33_554_432, 0, -33_554_433);
    assert_eq!(block_get_x(px), -33_554_432);
    assert_eq!(block_get_z(px), 33_554_431);

    // SectionPos X/Z wrap modulo 2^22, Y modulo 2^20.
    let sp = section_as_long(2_097_152, 524_288, -2_097_153);
    assert_eq!(section_get_x(sp), -2_097_152);
    assert_eq!(section_get_y(sp), -524_288);
    assert_eq!(section_get_z(sp), 2_097_151);
}

#[test]
fn ec23_block_pos_clamp_location_within_asymmetric_precision() {
    // For |c| >= 2^20, f32(c) + 1e-5f == f32(c) (zero lower margin).
    let pos_20 = Vec3i::new(1 << 20, 0, 0);
    let clamped_lo = pos_20.clamp_location_within(Vec3::new(0.0, 0.5, 0.5));
    assert_eq!(clamped_lo.x, f64::from(1 << 20));
    let clamped_hi = pos_20.clamp_location_within(Vec3::new(1.0e9, 0.5, 0.5));
    assert_eq!(clamped_hi.x, (f64::from(1 << 20) + 1.0) - EPSILON_F64);

    // For |c| > 2^24, f32(c) rounds odd integers away from c (2^24 + 1 rounds to 2^24).
    let odd = (1 << 24) + 1;
    let pos_24 = Vec3i::new(odd, 0, 0);
    let clamped = pos_24.clamp_location_within(Vec3::new(0.0, 0.5, 0.5));
    assert_eq!(clamped.x, f64::from(1 << 24));
}

#[test]
fn ec24_vec3i_dist_manhattan_f32_loss_and_compare_to_wrapping() {
    // Difference > 2^24 loses low bits in f32.
    let a = Vec3i::new(0, 0, 0);
    let b = Vec3i::new((1 << 24) + 1, 0, 0);
    assert_eq!(a.dist_manhattan(b), 1 << 24);

    // Difference == i32::MIN stays negative under wrapping_abs.
    let c = Vec3i::new(i32::MIN, 0, 0);
    assert_eq!(a.dist_manhattan(c), i32::MIN);

    // compare_to orders Y, then Z, then X using wrapping subtraction.
    assert!(Vec3i::new(100, 1, 100).compare_to(Vec3i::new(0, 2, 0)) < 0);
    assert!(Vec3i::new(100, 1, 1).compare_to(Vec3i::new(0, 1, 2)) < 0);
    // Wrapping overflow flips sign of compare_to: i32::MAX - i32::MIN wraps to -1.
    assert_eq!(
        Vec3i::new(0, i32::MAX, 0).compare_to(Vec3i::new(0, i32::MIN, 0)),
        -1
    );
}

#[test]
fn ec25_vec3_and_vec2_quirks() {
    // Vec3.normalize compares against f64(1e-5f) and preserves NaN.
    let v_below = Vec3::new(9.999_999_7e-6, 0.0, 0.0);
    let v_above = Vec3::new(9.999_999_8e-6, 0.0, 0.0);
    assert_eq!(v_below.normalize(), Vec3::ZERO);
    assert_ne!(v_above.normalize(), Vec3::ZERO);
    assert!(Vec3::new(f64::NAN, 1.0, 0.0).normalize().x.is_nan());

    // Vec3.reverse turns +0.0 into -0.0.
    assert_eq!(Vec3::ZERO.reverse().x.to_bits(), (-0.0_f64).to_bits());

    // Vec3.equals uses Double.compare (-0.0 != +0.0, NaN == NaN).
    assert!(!Vec3::new(-0.0, 0.0, 0.0).java_equals(Vec3::ZERO));
    assert!(Vec3::new(f64::NAN, 1.0, 2.0).java_equals(Vec3::new(f64::NAN, 1.0, 2.0)));

    // Vec2.equals uses == (-0.0 == +0.0, NaN != NaN).
    assert!(Vec2::new(-0.0, 0.0).java_equals(Vec2::ZERO));
    assert!(!Vec2::new(f32::NAN, 0.0).java_equals(Vec2::new(f32::NAN, 0.0)));

    // Vec2.normalized uses 1e-4f threshold (ten times Vec3's).
    assert_eq!(Vec2::new(5.0e-5, 0.0).normalized(), Vec2::ZERO);
    assert_ne!(Vec2::new(2.0e-4, 0.0).normalized(), Vec2::ZERO);

    // Vec3.projected_on does two separate scalings.
    let a = Vec3::new(1.3, 2.7, -3.1);
    let onto = Vec3::new(0.7, -1.1, 2.3);
    let expected = onto.scale(a.dot(onto)).scale(1.0 / onto.length_sqr());
    assert_eq!(a.projected_on(onto).x.to_bits(), expected.x.to_bits());
    assert_eq!(a.projected_on(onto).y.to_bits(), expected.y.to_bits());
    assert_eq!(a.projected_on(onto).z.to_bits(), expected.z.to_bits());

    // Vec3.rotation uses RAD_TO_DEG (0x42652EE0) and Vec3.direction_from_rotation uses lookup trig.
    let dir = Vec3::direction_from_rotation(0.0, 0.0);
    assert!(dir.z > 0.99);
    let rot = Vec3::new(0.0, 0.0, 1.0).rotation();
    assert_eq!(rot.x, 0.0);
    assert_eq!(rot.y, 0.0);
}

#[test]
fn ec26_aabb_canonicalizing_constructor_flips_disjoint_and_zero_thickness() {
    // Disjoint intersect returns the gap box!
    let b1 = Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
    let b2 = Aabb::new(3.0, 0.0, 0.0, 4.0, 1.0, 1.0);
    let gap = b1.intersect(b2);
    assert_eq!(gap.min_x, 1.0);
    assert_eq!(gap.max_x, 3.0);

    // Over-deflate swaps min and max.
    let over = b1.deflate(2.0, 0.0, 0.0);
    assert_eq!(over.min_x, -1.0);
    assert_eq!(over.max_x, 2.0);

    // Zero-thickness next_deflated expands by 2 ULPs!
    let flat = Aabb::new(1.0, 2.0, 3.0, 1.0, 2.0, 3.0);
    let nd = flat.next_deflated();
    assert_eq!(nd.min_x, 1.0_f64.next_down());
    assert_eq!(nd.max_x, 1.0_f64.next_up());
}

#[test]
fn ec27_aabb_clip_strict_bounds_and_epsilon_pad() {
    let box1 = Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);

    // s == 0.0 (starts on face) and s == 1.0 (ends on face) are strictly excluded.
    assert!(
        box1.clip(Vec3::new(0.0, 0.5, 0.5), Vec3::new(2.0, 0.5, 0.5))
            .is_none()
    );
    assert!(
        box1.clip(Vec3::new(-1.0, 0.5, 0.5), Vec3::new(0.0, 0.5, 0.5))
            .is_none()
    );

    // |d_c| <= 1e-7 ignores that axis.
    assert!(
        box1.clip(Vec3::new(-0.5e-7, 0.5, 0.5), Vec3::new(0.5e-7, 0.5, 0.5))
            .is_none()
    );

    // Transverse bounds are padded outward by ±1e-7.
    let hit = box1
        .clip_with_face(Vec3::new(-1.0, -0.5e-7, 0.5), Vec3::new(1.0, -0.5e-7, 0.5))
        .expect("padded hit should succeed");
    assert_eq!(hit.1, Direction::West);
    assert_eq!(hit.0.x, 0.0);
}

#[test]
fn ec28_ray_intersects_aabb_strict_z_cross_fails_pure_z_ray() {
    let unit = Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);

    // Pure Z-axis ray aimed right through center evaluates 0.0 < 0.0 -> false!
    assert!(!ray_intersects_aabb(
        Vec3::new(0.5, 0.5, -5.0),
        Vec3::new(0.0, 0.0, 1.0),
        unit
    ));

    // Pure X-axis and Y-axis rays succeed because their cross tests use `>` (not `<`) and
    // the Z-cross test has ex * ay + ey * ax > 0!
    assert!(ray_intersects_aabb(
        Vec3::new(-5.0, 0.5, 0.5),
        Vec3::new(1.0, 0.0, 0.0),
        unit
    ));
    assert!(ray_intersects_aabb(
        Vec3::new(0.5, -5.0, 0.5),
        Vec3::new(0.0, 1.0, 0.0),
        unit
    ));
}
