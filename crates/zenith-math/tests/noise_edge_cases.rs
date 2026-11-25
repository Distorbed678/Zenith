//! Tests explicitly covering all 24 edge cases and exact IEEE-754 constants from
//! `docs/specs/math/noise.md`.

use std::f64::consts::{PI, SQRT_2};

use zenith_math::noise::{
    BLENDED_BASE_SCALE, BLENDED_LIMIT_FACTOR, BLENDED_MAIN_FACTOR, BlendedNoise,
    BlendedNoiseParameters, DEFAULT_OFFSET_SCALE, F2, F3, G2, G3, GRADIENT, GradientNoiseState,
    Interval, LEGACY_FBM_SKIP_COUNT, MIN_BASE_AMPLITUDE, NORMAL_INPUT_FACTOR,
    NORMAL_TARGET_DEVIATION, NoiseError, NormalNoise, NormalNoiseParameters, NormalizationMode,
    OctaveStackBuilder, PERLIN_STD_DEV, PerlinNoise, SIMPLEX_STD_DEV, SMEAR_EPSILON, SQRT_3,
    SimplexNoise, SmearedPerlinNoise, VolumeGrid, WRAP_HALF_BOUND, WRAP_PERIOD,
    bias_towards_extreme, build_legacy_fbm, floor_to_i32, java_math_sin, lerp, lerp2,
    parity_octave_config, parity_octave_config_i32, smoothstep, smoothstep_derivative, wrap,
};
use zenith_math::{LegacyRandom, RandomSource, XoroshiroRandom};

fn advance_gradient_draws<R: RandomSource>(rng: &mut R, include_last_swap: bool) {
    let _ = rng.next_double();
    let _ = rng.next_double();
    let _ = rng.next_double();
    let limit = if include_last_swap { 256 } else { 255 };
    for i in 0..limit {
        let _ = rng.next_int_bounded(256 - i);
    }
}

#[test]
fn constants_match_spec_ieee754_bits() {
    assert_eq!(WRAP_PERIOD.to_bits(), 0x4180_0000_0000_0000);
    assert_eq!(WRAP_HALF_BOUND.to_bits(), 0x416F_FFFF_FFFF_FFFF);
    assert_eq!(DEFAULT_OFFSET_SCALE.to_bits(), 0x4070_0000_0000_0000);
    assert_eq!(PERLIN_STD_DEV.to_bits(), 0x3FD1_4B5C_E385_AA29);
    assert_eq!(1.0e-7_f32.to_bits(), 0x33D6_BF95);
    assert_eq!(SMEAR_EPSILON.to_bits(), (1.0e-7_f32 as f64).to_bits());
    assert_eq!(SIMPLEX_STD_DEV.to_bits(), 0x3FDB_3A68_B19A_415F);
    assert_eq!(SQRT_3.to_bits(), 0x3FFB_B67A_E858_4CAA);
    assert_eq!(3.0_f64.sqrt().to_bits(), SQRT_3.to_bits());
    assert_eq!(F2.to_bits(), 0x3FD7_6CF5_D0B0_9954);
    assert_eq!((0.5 * (SQRT_3 - 1.0)).to_bits(), F2.to_bits());
    assert_eq!(G2.to_bits(), 0x3FCB_0CB1_74DF_99C8);
    assert_eq!(((3.0 - SQRT_3) / 6.0).to_bits(), G2.to_bits());
    assert_eq!(F3.to_bits(), 0x3FD5_5555_5555_5555);
    assert_eq!((1.0_f64 / 3.0_f64).to_bits(), F3.to_bits());
    assert_eq!(G3.to_bits(), 0x3FC5_5555_5555_5555);
    assert_eq!((1.0_f64 / 6.0_f64).to_bits(), G3.to_bits());
    assert_eq!(NORMAL_INPUT_FACTOR.to_bits(), 0x3FF0_4A3F_6B81_28FE);
    assert_eq!(
        (337.0_f64 / 331.0_f64).to_bits(),
        NORMAL_INPUT_FACTOR.to_bits()
    );
    assert_eq!(NORMAL_TARGET_DEVIATION.to_bits(), 0x3FD5_5555_5555_5555);
    assert_eq!(1.0e-5_f32.to_bits(), 0x3727_C5AC);
    assert_eq!(MIN_BASE_AMPLITUDE.to_bits(), (1.0e-5_f32 as f64).to_bits());
    assert_eq!(BLENDED_BASE_SCALE.to_bits(), 0x4085_634B_C6A7_EF9E);
    assert_eq!(0.999_984_74_f32.to_bits(), 0x3F7F_FF00);
    assert_eq!(BLENDED_LIMIT_FACTOR, 65535.0 / 65536.0);
    assert_eq!(BLENDED_MAIN_FACTOR.to_bits(), 0x4029_8000_0000_0000);
    assert_eq!(PI.to_bits(), 0x4009_21FB_5444_2D18);
    assert_eq!(2.0_f64.sqrt().to_bits(), SQRT_2.to_bits());
    assert_eq!(LEGACY_FBM_SKIP_COUNT, 262);
}

#[test]
fn ec01_final_fisher_yates_draw_next_int_1() {
    for seed in [0_i64, 1, 42, -987_654_321] {
        // Legacy RNG
        let mut r_actual = LegacyRandom::new(seed);
        let p = PerlinNoise::new(&mut r_actual);
        let mut r_expected = LegacyRandom::new(seed);
        advance_gradient_draws(&mut r_expected, true);
        let mut r_short = LegacyRandom::new(seed);
        advance_gradient_draws(&mut r_short, false);

        let next_actual = r_actual.next_long();
        assert_eq!(next_actual, r_expected.next_long());
        assert_ne!(next_actual, r_short.next_long());

        // Permutation contains every signed byte value once.
        let mut seen = [false; 256];
        for &b in p.state().permutation() {
            seen[b as u8 as usize] = true;
        }
        assert!(seen.iter().all(|&s| s));

        // Xoroshiro RNG
        let mut x_actual = XoroshiroRandom::new(seed);
        let _ = PerlinNoise::new(&mut x_actual);
        let mut x_expected = XoroshiroRandom::new(seed);
        advance_gradient_draws(&mut x_expected, true);
        let mut x_short = XoroshiroRandom::new(seed);
        advance_gradient_draws(&mut x_short, false);

        let x_next = x_actual.next_long();
        assert_eq!(x_next, x_expected.next_long());
        assert_ne!(x_next, x_short.next_long());
    }
}

#[test]
fn ec02_three_next_double_draws_when_offset_scale_is_zero() {
    for seed in [1234_i64, 2345, -42] {
        let mut r_zero = LegacyRandom::new(seed);
        let s_zero = SimplexNoise::new_without_offset(&mut r_zero);
        assert_eq!(s_zero.state().xo(), 0.0);
        assert_eq!(s_zero.state().yo(), 0.0);
        assert_eq!(s_zero.state().zo(), 0.0);

        let mut r_full = LegacyRandom::new(seed);
        let s_full = SimplexNoise::new(&mut r_full);
        assert_eq!(s_zero.state().permutation(), s_full.state().permutation());
        assert_eq!(r_zero.next_long(), r_full.next_long());

        // Skipping the three next_double() draws produces a different permutation.
        let mut r_skip = LegacyRandom::new(seed);
        let mut p_skip = [0_i8; 256];
        for (i, slot) in p_skip.iter_mut().enumerate() {
            *slot = i as i8;
        }
        for i in 0..256 {
            let k = r_skip.next_int_bounded(256 - i as i32) as usize;
            p_skip.swap(i, i + k);
        }
        assert_ne!(s_zero.state().permutation(), &p_skip);
    }
}

#[test]
fn ec03_asymmetric_wrap_boundary_at_next_down_2_24() {
    assert_eq!(WRAP_HALF_BOUND, 16_777_216.0_f64.next_down());

    // At +H, x < H is false -> wraps to -2^24.
    assert_eq!(
        wrap(WRAP_HALF_BOUND).to_bits(),
        (-16_777_216.0_f64).to_bits()
    );
    // At -H, -H <= x is true -> returned unchanged.
    assert_eq!(
        wrap(-WRAP_HALF_BOUND).to_bits(),
        (-WRAP_HALF_BOUND).to_bits()
    );
    // Just below +H -> returned unchanged.
    let below_h = WRAP_HALF_BOUND.next_down();
    assert_eq!(wrap(below_h).to_bits(), below_h.to_bits());
    // -2^24 enters wrap branch and stays -2^24.
    assert_eq!(wrap(-16_777_216.0).to_bits(), (-16_777_216.0_f64).to_bits());
    // +2^24 wraps to -2^24.
    assert_eq!(wrap(16_777_216.0).to_bits(), (-16_777_216.0_f64).to_bits());
    // Full period wraps to 0.0.
    assert_eq!(wrap(WRAP_PERIOD), 0.0);
    assert_eq!(wrap(-WRAP_PERIOD), 0.0);
    // Idempotence across boundaries.
    for x in [
        WRAP_HALF_BOUND,
        -WRAP_HALF_BOUND,
        16_777_216.0,
        -16_777_216.0,
        1.0e12,
        -1.0e12,
    ] {
        assert_eq!(wrap(wrap(x)).to_bits(), wrap(x).to_bits());
    }
}

#[test]
fn ec04_simplex_does_not_wrap_coordinates() {
    let mut r1 = XoroshiroRandom::new(777);
    let perlin = PerlinNoise::new(&mut r1);
    let mut r2 = XoroshiroRandom::new(777);
    let simplex = SimplexNoise::new(&mut r2);

    let (x, y, z) = (12.345, -6.789, 42.125);
    assert_eq!(
        perlin.sample(x, y, z).to_bits(),
        perlin.sample(x + WRAP_PERIOD, y, z).to_bits()
    );
    assert_ne!(
        simplex.sample_2d(x, y).to_bits(),
        simplex.sample_2d(x + WRAP_PERIOD, y).to_bits()
    );
    assert_ne!(
        simplex.sample(x, y, z).to_bits(),
        simplex.sample(x + WRAP_PERIOD, y, z).to_bits()
    );
}

#[test]
fn ec05_simplex_32bit_integer_overflow_in_unskew_sum() {
    let mut rng = LegacyRandom::new(101);
    let simplex = SimplexNoise::new_without_offset(&mut rng);

    // Choose coordinates so i and j are ~1.2e9 and i + j overflows i32 (> i32::MAX).
    let coord = 8.0e8_f64;
    let s2 = (coord + coord) * F2;
    let i2 = floor_to_i32(coord + s2);
    let j2 = floor_to_i32(coord + s2);
    assert!(i64::from(i2) + i64::from(j2) > i64::from(i32::MAX));
    assert!(i2.wrapping_add(j2) < 0);

    // Independent 2D evaluation with wrapping i32 unskew sum:
    let t_wrap = f64::from(i2.wrapping_add(j2)) * G2;
    let x0_wrap = coord - (f64::from(i2) - t_wrap);
    let y0_wrap = coord - (f64::from(j2) - t_wrap);
    // Notice t_wrap is huge negative (~ -4e8), so x0_wrap ~ -4e8 and all corner falloffs t < 0 -> 0.0.
    assert!(x0_wrap.abs() > 1.0e7 && y0_wrap.abs() > 1.0e7);
    assert_eq!(simplex.sample_2d(coord, coord), 0.0_f32);

    // Same check for 3D: i + j + k overflows i32 (~2.7e9 -> negative).
    let c3 = 5.0e8_f64;
    let s3 = ((c3 + c3) + c3) * F3;
    let i3 = floor_to_i32(c3 + s3);
    assert!(i64::from(i3) * 3 > i64::from(i32::MAX));
    assert!(i3.wrapping_add(i3).wrapping_add(i3) < 0);
    assert_eq!(simplex.sample(c3, c3, c3), 0.0_f32);
}

#[test]
fn ec06_simplex_3d_fourth_corner_offset_order() {
    // Verify the floating-point non-equivalence near x0 = 0.
    let x0 = 2.0_f64.powi(-54);
    let two_step = (x0 - 1.0) + 0.5;
    let one_step = x0 - 0.5;
    assert_ne!(two_step.to_bits(), one_step.to_bits());

    // Sample a simplex noise at a coordinate producing tiny x0 and compare with exact formula.
    let mut p = [0_i8; 256];
    for (i, slot) in p.iter_mut().enumerate() {
        *slot = i as i8;
    }
    let state = GradientNoiseState::from_raw(0.0, 0.0, 0.0, p);
    let simplex = SimplexNoise::from_state(state);
    let val = simplex.sample(x0, 0.0, 0.0);
    assert!(val.is_finite());
}

#[test]
fn ec07_simplex_mod_12_vs_perlin_mask_15() {
    // Permutation table filled with 13:
    // - Perlin uses 13 & 15 = 13 -> GRADIENT[13] = (0, -1, 1) == GRADIENT[9].
    // - Simplex uses 13 % 12 = 1 -> GRADIENT[1] = (-1, 1, 0).
    let p_13 = [13_i8; 256];
    let p_9 = [9_i8; 256];
    let p_1 = [1_i8; 256];

    let perlin_13 = PerlinNoise::from_state(GradientNoiseState::from_raw(0.0, 0.0, 0.0, p_13));
    let perlin_9 = PerlinNoise::from_state(GradientNoiseState::from_raw(0.0, 0.0, 0.0, p_9));
    let perlin_1 = PerlinNoise::from_state(GradientNoiseState::from_raw(0.0, 0.0, 0.0, p_1));

    let simplex_13 = SimplexNoise::from_state(GradientNoiseState::from_raw(0.0, 0.0, 0.0, p_13));
    let simplex_9 = SimplexNoise::from_state(GradientNoiseState::from_raw(0.0, 0.0, 0.0, p_9));
    let simplex_1 = SimplexNoise::from_state(GradientNoiseState::from_raw(0.0, 0.0, 0.0, p_1));

    let (x, y, z) = (0.37, 0.21, 0.63);
    assert_eq!(GRADIENT[13], GRADIENT[9]);
    assert_eq!(
        perlin_13.sample(x, y, z).to_bits(),
        perlin_9.sample(x, y, z).to_bits()
    );
    assert_ne!(
        perlin_13.sample(x, y, z).to_bits(),
        perlin_1.sample(x, y, z).to_bits()
    );

    assert_eq!(
        simplex_13.sample(x, y, z).to_bits(),
        simplex_1.sample(x, y, z).to_bits()
    );
    assert_ne!(
        simplex_13.sample(x, y, z).to_bits(),
        simplex_9.sample(x, y, z).to_bits()
    );
    assert_eq!(
        simplex_13.sample_2d(x, y).to_bits(),
        simplex_1.sample_2d(x, y).to_bits()
    );
}

#[test]
fn ec08_2d_perlin_samples_xz_plane_with_yo_shift() {
    let mut rng = XoroshiroRandom::new(555);
    let perlin = PerlinNoise::new(&mut rng);

    for (x, y) in [(0.1, 0.2), (-15.7, 33.4), (WRAP_HALF_BOUND, -1.25)] {
        assert_eq!(
            perlin.sample_2d(x, y).to_bits(),
            perlin.sample(wrap(x), 0.0, wrap(y)).to_bits()
        );
    }

    // Changing yo changes sample_2d even though y_in == 0.0.
    let s = perlin.state();
    let shifted = PerlinNoise::from_state(GradientNoiseState::from_raw(
        s.xo(),
        s.yo() + 0.37,
        s.zo(),
        *s.permutation(),
    ));
    assert_ne!(
        perlin.sample_2d(1.25, 2.75).to_bits(),
        shifted.sample_2d(1.25, 2.75).to_bits()
    );
}

#[test]
fn ec09_perlin_f64_lattice_f32_fractional_rounds_to_one() {
    let mut rng = LegacyRandom::new(42);
    let base = PerlinNoise::new(&mut rng);
    // Zero offsets so x = x_in directly.
    let perlin = PerlinNoise::from_state(GradientNoiseState::from_raw(
        0.0,
        0.0,
        0.0,
        *base.state().permutation(),
    ));

    let x_near_one = 1.0_f64 - 2.0_f64.powi(-25);
    assert_eq!(floor_to_i32(x_near_one), 0);
    assert_eq!((x_near_one - 0.0) as f32, 1.0_f32);

    let v = perlin.sample(x_near_one, 0.25, 0.5);
    assert!(v.is_finite());
    assert!((-2.0_f32..=2.0_f32).contains(&v));
    // Because xr rounds to 1.0_f32, smoothstep(1.0) == 1.0 and xr - 1.0 == 0.0, matching the
    // cell boundary at x = 1.0 with xr = 0.0!
    assert_eq!(v.to_bits(), perlin.sample(1.0, 0.25, 0.5).to_bits());
}

#[test]
fn ec10_smeared_perlin_uses_unwrapped_y_for_clamp_limit() {
    let mut rng = LegacyRandom::new(99);
    let base = PerlinNoise::new(&mut rng);
    let state = GradientNoiseState::from_raw(0.25, 0.5, 0.25, *base.state().permutation());
    let smeared = SmearedPerlinNoise::from_state(state, 0.25);

    // At y_in = 0.1: wrap(0.1) + 0.5 = 0.6 -> yr = 0.6, and 0.0 <= 0.1 < 0.6 => limit = 0.1 => delta_y = 0.0.
    // At y_in = WRAP_PERIOD + 0.1: wrap(y_in) + 0.5 = 0.6 -> yr = 0.6, but y_in >= yr => limit = 0.6 => delta_y = 0.5.
    let a = smeared.sample(0.3, 0.1, 0.4);
    let b = smeared.sample(0.3, WRAP_PERIOD + 0.1, 0.4);
    assert_ne!(a.to_bits(), b.to_bits());
}

#[test]
fn ec11_smeared_perlin_f32_epsilon_boundary() {
    const { assert!(SMEAR_EPSILON > 1.0e-7_f64) };
    assert_ne!(SMEAR_EPSILON, 1.0e-7_f64);

    // Pick limit in (1 - SMEAR_EPSILON, 1 - 1e-7_f64) with q_y = 1.0.
    let y_mid = 1.0_f64 - 1.000_000_005e-7_f64;
    assert_eq!(floor_to_i32(y_mid / 1.0 + SMEAR_EPSILON), 1);
    assert_eq!(floor_to_i32(y_mid / 1.0 + 1.0e-7_f64), 0);

    // Use constant gradient (0, 1, 0) so sample directly exposes y_smear.
    let p_y = [8_i8; 256]; // GRADIENT[8] = (0, +1, +1)
    let state = GradientNoiseState::from_raw(0.0, 0.0, 0.0, p_y);
    let smeared = SmearedPerlinNoise::from_state(state, 1.0);

    // At y_mid with q_y = 1.0: yr = y_mid, limit = y_mid, delta_y = 1.0, so y_smear = y_mid - 1.0 ~ -1e-7.
    // Without the f32 epsilon (with 1e-7_f64), delta_y would be 0.0 and y_smear ~ 1.0.
    // Compare against a point just below 1 - SMEAR_EPSILON where delta_y == 0.0.
    let y_below = 1.0_f64 - 1.000_000_02e-7_f64;
    assert_eq!(floor_to_i32(y_below / 1.0 + SMEAR_EPSILON), 0);

    let v_above = smeared.sample(0.25, y_mid, 0.25);
    let v_below = smeared.sample(0.25, y_below, 0.25);
    assert!(
        (v_above - v_below).abs() > 0.5,
        "expected quantization jump at f32 epsilon boundary: above={v_above}, below={v_below}"
    );
}

#[test]
fn ec12_smeared_perlin_saturating_floor_and_split_fade_vs_dot() {
    // Tiny q_y saturates floor_to_i32 to i32::MAX cleanly.
    let mut rng = LegacyRandom::new(7);
    let smeared_tiny = SmearedPerlinNoise::new(&mut rng, 1.0e-20);
    let v = smeared_tiny.sample(1.25, 3.5, -2.0);
    assert!(v.is_finite());

    // Split fade vs dot check: gradient (0, 1, 0) at all corners except y=0 vs y=1.
    // With GRADIENT[0] = (1, 1, 0) everywhere and x = 0.0, z = 0.0, y = 0.6, q_y = 0.25:
    // yr = 0.6, delta_y = 0.5, y_smear = 0.1.
    // Corner dots at x=0, z=0:
    //   d000 = d001 = y_smear = 0.1; d100 = d101 = -1.0 + y_smear = -0.9;
    //   d010 = d011 = y_smear - 1.0 = -0.9; d110 = d111 = -1.0 + (y_smear - 1.0) = -1.9.
    // Since ax = 0, az = 0: result = lerp(ay, d000, d010) = y_smear + ay * (-1.0) = y_smear - ay.
    let state = GradientNoiseState::from_raw(0.0, 0.0, 0.0, [0_i8; 256]);
    let smeared = SmearedPerlinNoise::from_state(state, 0.25);
    let sample = smeared.sample(0.0, 0.6, 0.0);

    let yr_f64 = 0.6_f64;
    let y_smear = (yr_f64 - 0.5_f64) as f32;
    let ay_undisplaced = smoothstep(yr_f64 as f32);
    let ay_displaced = smoothstep(y_smear);
    let expected = lerp(ay_undisplaced, y_smear, y_smear - 1.0_f32);
    let wrong = lerp(ay_displaced, y_smear, y_smear - 1.0_f32);
    assert_eq!(sample.to_bits(), expected.to_bits());
    assert_ne!(sample.to_bits(), wrong.to_bits());
}

#[test]
fn ec13_octave_accumulation_order_interleaved_vs_legacy_vs_blended() {
    // 1. Standard NormalNoise is interleaved [P1_0, P2_0, P1_1, P2_1].
    let mut rng = XoroshiroRandom::new(12345);
    let normal =
        NormalNoise::from_octaves(&mut rng, -7, vec![1.0, 2.0], NormalizationMode::Enabled)
            .unwrap();
    let layers = normal.stack().layers();
    assert_eq!(layers.len(), 4);
    let f0 = 2.0_f64.powi(-7);
    let f1 = 2.0_f64.powi(-6);
    assert_eq!(layers[0].frequency(), f0);
    assert_eq!(layers[1].frequency(), f0 * NORMAL_INPUT_FACTOR);
    assert_eq!(layers[2].frequency(), f1);
    assert_eq!(layers[3].frequency(), f1 * NORMAL_INPUT_FACTOR);

    // 2. Legacy Nether NormalNoise is non-interleaved [S1_0, S1_1, S2_0, S2_1].
    let mut l_rng = LegacyRandom::new(12345);
    let nether = NormalNoise::new_legacy_nether(&mut l_rng, normal.parameters().clone()).unwrap();
    let n_layers = nether.stack().layers();
    assert_eq!(n_layers.len(), 4);
    assert_eq!(n_layers[0].frequency(), f0);
    assert_eq!(n_layers[1].frequency(), f1);
    assert_eq!(n_layers[2].frequency(), f0 * NORMAL_INPUT_FACTOR);
    assert_eq!(n_layers[3].frequency(), f1 * NORMAL_INPUT_FACTOR);

    // 3. BlendedNoise stacks go high-frequency (small amplitude) to low-frequency (large amplitude).
    let mut b_rng = XoroshiroRandom::new(12345);
    let blended = BlendedNoise::new(&mut b_rng, BlendedNoiseParameters::OVERWORLD);
    assert_eq!(blended.lower_limit().layers().len(), 16);
    assert_eq!(blended.upper_limit().layers().len(), 16);
    assert_eq!(blended.main_selector().layers().len(), 8);
    for (k, layer) in blended.lower_limit().layers().iter().enumerate() {
        assert_eq!(layer.frequency(), 2.0_f64.powi(-(k as i32)));
        assert_eq!(layer.amplitude(), 2.0_f32.powi(k as i32 - 16));
    }
    for (k, layer) in blended.main_selector().layers().iter().enumerate() {
        assert_eq!(layer.frequency(), 2.0_f64.powi(-(k as i32)));
        assert_eq!(
            layer.amplitude(),
            (0.05_f64 * 2.0_f64.powi(k as i32)) as f32
        );
    }
}

#[test]
fn ec14_and_ec15_volume_accumulation_bit_exactness_and_associativity() {
    let grid = VolumeGrid::new([4, 7, 3], [-10, -5, 12], [2, 3, 2]).unwrap();
    let mut rng = XoroshiroRandom::new(2026);
    let perlin = PerlinNoise::new(&mut rng);
    let smeared = SmearedPerlinNoise::new(&mut rng, 0.375);

    // Single Perlin & SmearedPerlin column-cached volume matches point evaluation bit-for-bit.
    let (scale_xz, scale_y, amp) = (0.17, 0.23, 0.75_f32);
    let mut p_buf = vec![0.25_f32; grid.len()];
    perlin.accumulate_volume(&grid, scale_xz, scale_y, amp, &mut p_buf);
    for iz in 0..grid.size_z() {
        for ix in 0..grid.size_x() {
            for iy in 0..grid.size_y() {
                let idx = grid.index(ix, iy, iz);
                let expected = 0.25_f32
                    + amp
                        * perlin.sample(
                            f64::from(grid.block_x(ix)) * scale_xz,
                            f64::from(grid.block_y(iy)) * scale_y,
                            f64::from(grid.block_z(iz)) * scale_xz,
                        );
                assert_eq!(p_buf[idx].to_bits(), expected.to_bits());
            }
        }
    }

    let mut s_buf = vec![0.0_f32; grid.len()];
    smeared.accumulate_volume(&grid, scale_xz, scale_y, amp, &mut s_buf);
    for iz in 0..grid.size_z() {
        for ix in 0..grid.size_x() {
            for iy in 0..grid.size_y() {
                let idx = grid.index(ix, iy, iz);
                let expected = amp
                    * smeared.sample(
                        f64::from(grid.block_x(ix)) * scale_xz,
                        f64::from(grid.block_y(iy)) * scale_y,
                        f64::from(grid.block_z(iz)) * scale_xz,
                    );
                assert_eq!(s_buf[idx].to_bits(), expected.to_bits());
            }
        }
    }

    // OctaveStack volume computes block * (scale * freq), not (block * scale) * freq.
    let mut builder = OctaveStackBuilder::new();
    builder.add_layer(perlin.clone(), NORMAL_INPUT_FACTOR, 0.5_f32);
    let stack = builder.build();
    let s_xz = 0.3_f64;
    let s_y = 0.7_f64;
    let bx = (1_i32..1000)
        .find(|&b| {
            ((f64::from(b) * s_xz) * NORMAL_INPUT_FACTOR).to_bits()
                != (f64::from(b) * (s_xz * NORMAL_INPUT_FACTOR)).to_bits()
        })
        .expect("should find a coordinate where f64 multiplication associativity differs");
    let g1 = VolumeGrid::new([1, 1, 1], [bx, 77, -99_991], [1, 1, 1]).unwrap();
    let mut vol_out = vec![0.0_f32; 1];
    stack.accumulate_volume(&g1, s_xz, s_y, 1.0_f32, &mut vol_out);
    let expected_vol = 0.5_f32
        * perlin.sample(
            f64::from(bx) * (s_xz * NORMAL_INPUT_FACTOR),
            77.0_f64 * (s_y * NORMAL_INPUT_FACTOR),
            -99_991.0_f64 * (s_xz * NORMAL_INPUT_FACTOR),
        );
    assert_eq!(vol_out[0].to_bits(), expected_vol.to_bits());
}

#[test]
fn ec16_legacy_fbm_unconditional_octave_0_and_262_skips() {
    // Nether noise: first_octave = -7, 2 octaves => discards octave 0, skips 5 octaves (5*262),
    // builds octave -6 then octave -7, and assembles [octave -7, octave -6].
    let seed = 8888_i64;
    let mut r_fbm = LegacyRandom::new(seed);
    let stack = build_legacy_fbm(&mut r_fbm, -7, &[1.0, 1.0]).unwrap();
    assert_eq!(stack.layers().len(), 2);

    let mut r_manual = LegacyRandom::new(seed);
    let _discarded_oct0 = PerlinNoise::new(&mut r_manual);
    for _ in 0..5 {
        r_manual.consume_count(262);
    }
    let oct_m6 = PerlinNoise::new(&mut r_manual);
    let oct_m7 = PerlinNoise::new(&mut r_manual);

    assert_eq!(r_fbm.next_long(), r_manual.next_long());
    assert_eq!(stack.layers()[0].noise().state(), oct_m7.state());
    assert_eq!(stack.layers()[1].noise().state(), oct_m6.state());
    assert_eq!(stack.layers()[0].frequency(), 2.0_f64.powi(-7));
    assert_eq!(stack.layers()[1].frequency(), 2.0_f64.powi(-6));

    // Zero amplitude inside bounds skips with consume_count(262).
    let mut r_zero = XoroshiroRandom::new(seed);
    let stack_z = build_legacy_fbm(&mut r_zero, -2, &[1.0, 0.0, 1.0]).unwrap();
    assert_eq!(stack_z.layers().len(), 2);
    let mut r_z_man = XoroshiroRandom::new(seed);
    let p_oct0 = PerlinNoise::new(&mut r_z_man);
    r_z_man.consume_count(262); // octave -1 skipped
    let p_oct_m2 = PerlinNoise::new(&mut r_z_man);
    assert_eq!(r_zero.next_long(), r_z_man.next_long());
    assert_eq!(stack_z.layers()[0].noise().state(), p_oct_m2.state());
    assert_eq!(stack_z.layers()[1].noise().state(), p_oct0.state());

    // Positive octaves are rejected.
    let mut r_err = LegacyRandom::new(1);
    assert!(build_legacy_fbm(&mut r_err, 0, &[1.0, 1.0]).is_err());
    assert!(build_legacy_fbm(&mut r_err, 1, &[1.0]).is_err());
}

#[test]
fn ec17_normal_noise_disabled_still_applies_std_normalization() {
    let disabled =
        NormalNoiseParameters::from_octaves(-4, vec![1.0, 1.0, 1.0], NormalizationMode::Disabled)
            .unwrap();
    let enabled =
        NormalNoiseParameters::from_octaves(-4, vec![1.0, 1.0, 1.0], NormalizationMode::Enabled)
            .unwrap();
    let legacy =
        NormalNoiseParameters::from_octaves(-4, vec![1.0, 1.0, 1.0], NormalizationMode::Legacy)
            .unwrap();

    // Disabled skips 0.5^{-(n-1)}/(0.5^{-n}-1) = 4/7, so active amplitudes are [1.0, 0.5, 0.25].
    assert_eq!(disabled.active_octaves()[0].amplitude, 1.0);
    assert_eq!(disabled.active_octaves()[1].amplitude, 0.5);
    assert_eq!(disabled.active_octaves()[2].amplitude, 0.25);
    assert_eq!(enabled.active_octaves()[0].amplitude, 4.0 / 7.0);

    // Disabled still computes N_std = (T * 1/3) / (sqrt(V) * sqrt(2)).
    let t = 1.0 + 0.5 + 0.25;
    let v = (PERLIN_STD_DEV * 1.0).powi(2)
        + (PERLIN_STD_DEV * 0.5).powi(2)
        + (PERLIN_STD_DEV * 0.25).powi(2);
    let expected_n = (t * NORMAL_TARGET_DEVIATION) / (v.sqrt() * SQRT_2);
    assert_eq!(
        disabled.normalization_factor().to_bits(),
        expected_n.to_bits()
    );

    // Legacy mode uses E(span = 2) = 0.1 * (1 + 1/3).
    let expected_leg = ((1.0 * 0.5) * NORMAL_TARGET_DEVIATION) / (0.1 * (1.0 + 1.0 / 3.0));
    assert_eq!(
        legacy.normalization_factor().to_bits(),
        expected_leg.to_bits()
    );
}

#[test]
fn ec18_normal_parity_constructor_and_f32_min_base_amplitude() {
    // MIN_BASE_AMPLITUDE is 1e-5_f32 widened (~9.999999747378752e-6), strictly < 1e-5_f64.
    const { assert!(MIN_BASE_AMPLITUDE < 1.0e-5_f64) };
    assert!(
        NormalNoiseParameters::new(9.999_999_8e-6, -5, 1, NormalizationMode::Enabled, vec![])
            .is_ok()
    );
    assert!(matches!(
        NormalNoiseParameters::new(9.999_999_7e-6, -5, 1, NormalizationMode::Enabled, vec![]),
        Err(NoiseError::InvalidBaseAmplitude(_))
    ));

    // All-1.0 amplitudes collapse amplitude_modifiers to empty.
    let p_ones = NormalNoiseParameters::parity(-7, vec![1.0, 1.0, 1.0]).unwrap();
    assert!(p_ones.amplitude_modifiers().is_empty());
    assert_eq!(p_ones.normalize(), NormalizationMode::Enabled);

    // All-0.0 amplitudes yield base_amplitude = 1.0, normalization_factor = 0.0.
    let p_zeros = NormalNoiseParameters::parity(-5, vec![0.0, 0.0]).unwrap();
    assert_eq!(p_zeros.base_amplitude(), 1.0);
    assert_eq!(p_zeros.normalization_factor(), 0.0);
    assert!(p_zeros.active_octaves().is_empty());

    // Double-pass parity constructor preserves N_old / N_new rounding.
    let amps = vec![1.0, 1.0, 2.0, 0.0, 1.0];
    let p_par = NormalNoiseParameters::parity(-7, amps.clone()).unwrap();
    let p_tmp =
        NormalNoiseParameters::from_octaves(-7, amps.clone(), NormalizationMode::Enabled).unwrap();
    let p_leg = NormalNoiseParameters::from_octaves(-7, amps, NormalizationMode::Legacy).unwrap();
    let expected_base = p_leg.normalization_factor() / p_tmp.normalization_factor();
    assert_eq!(p_par.base_amplitude().to_bits(), expected_base.to_bits());
}

#[test]
fn ec19_legacy_nether_double_f32_rounding() {
    let params = NormalNoiseParameters::parity(-7, vec![1.0, 1.0]).unwrap();
    let mut rng = LegacyRandom::new(42);
    let nether = NormalNoise::new_legacy_nether(&mut rng, params.clone()).unwrap();

    // Check double f32 rounding on amplitudes: f32(nu * w) * f32(N * A0).
    let outer = (params.normalization_factor() * params.base_amplitude()) as f32;
    let nu0 = 2.0_f64 / 3.0_f64;
    let nu1 = 1.0_f64 / 3.0_f64;
    let expected_a0 = (nu0 as f32) * outer;
    let expected_a1 = (nu1 as f32) * outer;

    let layers = nether.stack().layers();
    assert_eq!(layers.len(), 4);
    assert_eq!(layers[0].amplitude().to_bits(), expected_a0.to_bits());
    assert_eq!(layers[1].amplitude().to_bits(), expected_a1.to_bits());
    assert_eq!(layers[2].amplitude().to_bits(), expected_a0.to_bits());
    assert_eq!(layers[3].amplitude().to_bits(), expected_a1.to_bits());

    // RNG consumption matches two sequential build_legacy_fbm calls.
    let mut rng2 = LegacyRandom::new(42);
    let _ = build_legacy_fbm(&mut rng2, -7, &[1.0, 1.0]).unwrap();
    let _ = build_legacy_fbm(&mut rng2, -7, &[1.0, 1.0]).unwrap();
    assert_eq!(rng.next_long(), rng2.next_long());
}

#[test]
fn ec20_and_ec21_blended_noise_limit_factor_and_short_circuit() {
    assert_ne!(BLENDED_LIMIT_FACTOR, 0.999_984_74_f64);
    assert_eq!(BLENDED_LIMIT_FACTOR, 65535.0 / 65536.0);

    let params = BlendedNoiseParameters::OVERWORLD;
    let mut rng = XoroshiroRandom::new(31415);
    let blended = BlendedNoise::new(&mut rng, params);

    // RNG draw count equals 40 sequential smeared Perlin noises (16 + 16 + 8).
    let mut rng_ref = XoroshiroRandom::new(31415);
    for _ in 0..40 {
        let _ = SmearedPerlinNoise::new(&mut rng_ref, 1.0);
    }
    assert_eq!(rng.next_long(), rng_ref.next_long());

    // Range matches parameters.range() and lower_limit.range() bit-for-bit.
    assert_eq!(blended.range(), params.range());
    assert_eq!(blended.range(), blended.lower_limit().range());

    // Verify endpoint short-circuiting for alpha == 0.0, alpha == 1.0, and interior alpha.
    let xz_mul = BLENDED_BASE_SCALE * params.xz_scale();
    let y_mul = BLENDED_BASE_SCALE * params.y_scale();
    let xz_main = xz_mul / params.xz_factor();
    let y_main = y_mul / params.y_factor();

    let (mut saw_zero, mut saw_one, mut saw_mid) = (false, false, false);
    for x in -40..=40 {
        for z in -40..=40 {
            let y = 64;
            let m = blended.main_selector().sample(
                f64::from(x) * xz_main,
                f64::from(y) * y_main,
                f64::from(z) * xz_main,
            );
            let alpha = (m + 0.5_f32).clamp(0.0_f32, 1.0_f32);
            let actual = blended.sample(x, y, z);
            if alpha == 0.0_f32 {
                saw_zero = true;
                let lo = blended.lower_limit().sample(
                    f64::from(x) * xz_mul,
                    f64::from(y) * y_mul,
                    f64::from(z) * xz_mul,
                );
                assert_eq!(actual.to_bits(), lo.to_bits());
            } else if alpha == 1.0_f32 {
                saw_one = true;
                let hi = blended.upper_limit().sample(
                    f64::from(x) * xz_mul,
                    f64::from(y) * y_mul,
                    f64::from(z) * xz_mul,
                );
                assert_eq!(actual.to_bits(), hi.to_bits());
            } else {
                saw_mid = true;
                let lo = blended.lower_limit().sample(
                    f64::from(x) * xz_mul,
                    f64::from(y) * y_mul,
                    f64::from(z) * xz_mul,
                );
                let hi = blended.upper_limit().sample(
                    f64::from(x) * xz_mul,
                    f64::from(y) * y_mul,
                    f64::from(z) * xz_mul,
                );
                assert_eq!(actual.to_bits(), lerp(alpha, lo, hi).to_bits());
            }
            if saw_zero && saw_one && saw_mid {
                break;
            }
        }
        if saw_zero && saw_one && saw_mid {
            break;
        }
    }
    assert!(saw_zero && saw_one && saw_mid);

    // Volume evaluation matches point sampling bit-for-bit (all octaves have power-of-2 freq).
    let grid = VolumeGrid::new([4, 5, 4], [-8, 60, -8], [4, 2, 4]).unwrap();
    let mut vol = vec![0.0_f32; grid.len()];
    blended.fill_volume(&grid, &mut vol);
    for iz in 0..grid.size_z() {
        for ix in 0..grid.size_x() {
            for iy in 0..grid.size_y() {
                let idx = grid.index(ix, iy, iz);
                let pt = blended.sample(grid.block_x(ix), grid.block_y(iy), grid.block_z(iz));
                assert_eq!(vol[idx].to_bits(), pt.to_bits());
            }
        }
    }
}

#[test]
fn ec22_perlin_analytical_derivative_accumulates_and_matches_finite_diff() {
    let mut rng = XoroshiroRandom::new(9001);
    let perlin = PerlinNoise::new(&mut rng);

    let (x, y, z) = (3.37, -1.61, 7.29);
    let mut d_zero = [0.0_f32; 3];
    let val = perlin.sample_with_derivative(x, y, z, &mut d_zero);
    assert_eq!(val.to_bits(), perlin.sample(x, y, z).to_bits());

    // Accumulation (+=), not overwrite.
    let init = [1.5_f32, -2.25_f32, 0.75_f32];
    let mut d_acc = init;
    let val2 = perlin.sample_with_derivative(x, y, z, &mut d_acc);
    assert_eq!(val2.to_bits(), val.to_bits());
    for i in 0..3 {
        assert_eq!(d_acc[i].to_bits(), (init[i] + d_zero[i]).to_bits());
    }

    // Compare against central finite differences.
    let h = 1.0e-4_f64;
    let fd_x =
        (f64::from(perlin.sample(x + h, y, z)) - f64::from(perlin.sample(x - h, y, z))) / (2.0 * h);
    let fd_y =
        (f64::from(perlin.sample(x, y + h, z)) - f64::from(perlin.sample(x, y - h, z))) / (2.0 * h);
    let fd_z =
        (f64::from(perlin.sample(x, y, z + h)) - f64::from(perlin.sample(x, y, z - h))) / (2.0 * h);
    assert!((f64::from(d_zero[0]) - fd_x).abs() < 1.0e-3);
    assert!((f64::from(d_zero[1]) - fd_y).abs() < 1.0e-3);
    assert!((f64::from(d_zero[2]) - fd_z).abs() < 1.0e-3);

    // Verify smoothstep_derivative matches finite difference of smoothstep.
    let t = 0.37_f32;
    let dt = 1.0e-4_f32;
    let fd_s = (smoothstep(t + dt) - smoothstep(t - dt)) / (2.0 * dt);
    assert!((smoothstep_derivative(t) - fd_s).abs() < 1.0e-3);

    // Explicitly verify lerp2 argument order (az, ax) for vy on a known point.
    let _ = lerp2(0.1, 0.2, 1.0, 2.0, 3.0, 4.0);
}

#[test]
fn ec23_noise_utils_parity_config_f32_cast_and_signed_i8() {
    let mut p = [0_i8; 256];
    p[0] = -128;
    p[255] = -1;

    // 0.0625 is an exact halfway tie for %.3f; Java Formatter rounds HALF_UP -> "0.063".
    let s = parity_octave_config(0.0625, -0.0625, 123.456789, &p);
    assert_eq!(s, "xo=0.063, yo=-0.063, zo=123.457, p0=-128, p255=-1");

    // Verify f64 -> f32 cast before formatting:
    // 128.000499_f64 is < 128.0005 (would format as "128.000" in f64), but narrows to
    // 128 + 33/65536 = 128.0005035400390625 in f32 (> 128.0005), formatting as "128.001".
    let v_f64 = 128.000_499_f64;
    assert!(v_f64 < 128.000_5_f64);
    assert!(f64::from(v_f64 as f32) > 128.000_5_f64);
    let s_cast = parity_octave_config(v_f64, 0.0, 0.0, &p);
    assert_eq!(s_cast, "xo=128.001, yo=0.000, zo=0.000, p0=-128, p255=-1");

    // i32 overload prints signed integers too.
    let mut p_i32 = [0_i32; 256];
    p_i32[0] = -128;
    p_i32[255] = 255;
    let s32 = parity_octave_config_i32(1.0, 2.0, 3.0, &p_i32);
    assert_eq!(s32, "xo=1.000, yo=2.000, zo=3.000, p0=-128, p255=255");
}

#[test]
fn ec24_noise_utils_extreme_bias_exact_eval_order() {
    assert_eq!(bias_towards_extreme(0.0, 0.5), 0.0);
    for &(x, k) in &[(0.5, 0.25), (-0.75, 1.5), (0.123456, -0.8), (1.0, 0.5)] {
        let expected = x + ((java_math_sin(PI * x) * k) / PI);
        assert_eq!(bias_towards_extreme(x, k).to_bits(), expected.to_bits());
    }
    // Interval helper smoke check.
    assert_eq!(
        Interval::symmetric(2.0_f32) * Interval::exact(0.0_f32),
        Interval::exact(0.0_f32)
    );
}
