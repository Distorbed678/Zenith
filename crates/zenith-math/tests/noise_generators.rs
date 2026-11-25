//! Functional, invariant, RNG-draw-count, interval-arithmetic, and validation tests for
//! `zenith_math::noise`.

use zenith_math::noise::{
    BLENDED_LIMIT_FIRST_OCTAVE, BLENDED_MAIN_FIRST_OCTAVE, BLENDED_NOISE_ID, BLENDED_NOISE_SEED_ID,
    BlendedNoise, BlendedNoiseParameters, Interval, MAX_AMPLITUDE, MAX_BASE_OCTAVE, MAX_OCTAVES,
    MIN_BASE_AMPLITUDE, MIN_BASE_OCTAVE, NETHER_TEMPERATURE_ID, NETHER_VEGETATION_ID, Noise,
    NoiseError, NormalNoise, NormalNoiseParameters, NormalizationMode, OctaveStack,
    OctaveStackBuilder, PerlinNoise, SimplexNoise, SmearedPerlinNoise, VolumeGrid,
    biome_frozen_temperature_noise, biome_info_noise, biome_temperature_noise, build_smeared_fbm,
    end_island_noise,
};
use zenith_math::{LegacyRandom, PositionalRandomFactory, RandomSource, XoroshiroRandom};

#[test]
fn interval_arithmetic_and_nai_propagation() {
    let a = Interval::new(-2.0_f32, 3.0_f32);
    let b = Interval::new(-4.0_f32, 1.0_f32);
    assert_eq!(a + b, Interval::new(-6.0_f32, 4.0_f32));
    assert_eq!(a * b, Interval::new(-12.0_f32, 8.0_f32));

    // Zero-absorbing multiplication: 0.0 * inf == 0.0 (not NaN).
    let inf_iv = Interval::new(f32::NEG_INFINITY, f32::INFINITY);
    let zero_iv = Interval::exact(0.0_f32);
    assert_eq!(inf_iv * zero_iv, Interval::exact(0.0_f32));

    // NaN / NaI propagation.
    assert!(Interval::new(f32::NAN, 1.0).is_nai());
    assert!(Interval::symmetric(f32::NAN).is_nai());
    assert!(Interval::symmetric(-1.0).is_nai());
    assert!((Interval::exact(f32::INFINITY) + Interval::exact(f32::NEG_INFINITY)).is_nai());
    assert!((a + Interval::NaI).is_nai());
    assert!((a * Interval::NaI).is_nai());
    assert!(Interval::NaI.min().is_nan());
    assert!(Interval::NaI.max().is_nan());
    assert_eq!(Interval::NaI.bounds(), None);
}

#[test]
fn volume_grid_layout_and_validation() {
    assert!(VolumeGrid::new([0, 1, 1], [0, 0, 0], [1, 1, 1]).is_err());
    assert!(VolumeGrid::new([1, 0, 1], [0, 0, 0], [1, 1, 1]).is_err());
    assert!(VolumeGrid::new([1, 1, 0], [0, 0, 0], [1, 1, 1]).is_err());
    assert!(VolumeGrid::new([1, 1, 1], [0, 0, 0], [0, 1, 1]).is_err());
    assert!(VolumeGrid::new([1, 1, 1], [0, 0, 0], [1, -1, 1]).is_err());

    let grid = VolumeGrid::new([3, 4, 2], [-10, 5, 20], [2, 3, 4]).unwrap();
    assert_eq!(grid.len(), 24);
    assert!(!grid.is_empty());
    assert_eq!(grid.block_x(2), -6);
    assert_eq!(grid.block_y(3), 14);
    assert_eq!(grid.block_z(1), 24);

    // Z-major, then X, then Y contiguous.
    let mut seq = 0;
    for iz in 0..grid.size_z() {
        for ix in 0..grid.size_x() {
            for iy in 0..grid.size_y() {
                assert_eq!(grid.index(ix, iy, iz), seq);
                seq += 1;
            }
        }
    }
}

#[test]
fn normal_noise_standard_rng_draw_counts() {
    // Standard instantiation forks exactly two positional factories from rng, regardless of how
    // many octaves are active.
    let params = NormalNoiseParameters::from_octaves(
        -10,
        vec![1.0, 0.0, 2.0, 1.0, 0.5],
        NormalizationMode::Enabled,
    )
    .unwrap();
    assert_eq!(params.active_octaves().len(), 4);

    // Legacy: each fork_positional consumes 1 next_long() -> 2 total.
    let mut l_actual = LegacyRandom::new(98765);
    let n_legacy = NormalNoise::new(&mut l_actual, params.clone());
    let mut l_expected = LegacyRandom::new(98765);
    let _f1 = l_expected.fork_positional();
    let _f2 = l_expected.fork_positional();
    assert_eq!(l_actual.next_long(), l_expected.next_long());
    assert_eq!(n_legacy.stack().layers().len(), 8);

    // Xoroshiro: each fork_positional consumes 2 next_long() -> 4 total.
    let mut x_actual = XoroshiroRandom::new(98765);
    let n_xoro = NormalNoise::new(&mut x_actual, params);
    let mut x_expected = XoroshiroRandom::new(98765);
    let _f1 = x_expected.fork_positional();
    let _f2 = x_expected.fork_positional();
    assert_eq!(x_actual.next_long(), x_expected.next_long());
    assert_eq!(n_xoro.stack().layers().len(), 8);
}

#[test]
fn normal_noise_parameter_validation() {
    assert!(matches!(
        NormalNoiseParameters::new(f64::NAN, 0, 1, NormalizationMode::Enabled, vec![]),
        Err(NoiseError::InvalidBaseAmplitude(_))
    ));
    assert!(matches!(
        NormalNoiseParameters::new(
            MAX_AMPLITUDE + 1.0,
            0,
            1,
            NormalizationMode::Enabled,
            vec![]
        ),
        Err(NoiseError::InvalidBaseAmplitude(_))
    ));
    assert!(matches!(
        NormalNoiseParameters::new(
            MIN_BASE_AMPLITUDE,
            MIN_BASE_OCTAVE - 1,
            1,
            NormalizationMode::Enabled,
            vec![]
        ),
        Err(NoiseError::InvalidBaseOctave(_))
    ));
    assert!(matches!(
        NormalNoiseParameters::new(
            1.0,
            MAX_BASE_OCTAVE + 1,
            1,
            NormalizationMode::Enabled,
            vec![]
        ),
        Err(NoiseError::InvalidBaseOctave(_))
    ));
    assert!(matches!(
        NormalNoiseParameters::new(1.0, 0, 0, NormalizationMode::Enabled, vec![]),
        Err(NoiseError::InvalidOctaveCount(0))
    ));
    assert!(matches!(
        NormalNoiseParameters::new(1.0, 0, MAX_OCTAVES + 1, NormalizationMode::Enabled, vec![]),
        Err(NoiseError::InvalidOctaveCount(_))
    ));
    assert!(matches!(
        NormalNoiseParameters::new(1.0, 0, 2, NormalizationMode::Enabled, vec![1.0]),
        Err(NoiseError::AmplitudeModifierCountMismatch {
            expected: 2,
            actual: 1
        })
    ));
    assert!(matches!(
        NormalNoiseParameters::new(1.0, 0, 2, NormalizationMode::Enabled, vec![1.0, -0.1]),
        Err(NoiseError::InvalidAmplitudeModifier { index: 1, .. })
    ));
}

#[test]
fn blended_noise_validation_presets_and_seeding() {
    assert_eq!(BLENDED_NOISE_ID, "minecraft:old_blended_noise");
    assert_eq!(BLENDED_NOISE_SEED_ID, "minecraft:terrain");
    assert_eq!(BLENDED_LIMIT_FIRST_OCTAVE, -15);
    assert_eq!(BLENDED_MAIN_FIRST_OCTAVE, -7);
    assert_eq!(NETHER_TEMPERATURE_ID, "minecraft:nether/temperature");
    assert_eq!(NETHER_VEGETATION_ID, "minecraft:nether/vegetation");

    for preset in [
        BlendedNoiseParameters::OVERWORLD,
        BlendedNoiseParameters::NETHER,
        BlendedNoiseParameters::END,
    ] {
        let rebuilt = BlendedNoiseParameters::new(
            preset.xz_scale(),
            preset.y_scale(),
            preset.xz_factor(),
            preset.y_factor(),
            preset.smear_scale_multiplier(),
        )
        .unwrap();
        assert_eq!(rebuilt, preset);
    }

    assert!(BlendedNoiseParameters::new(0.0009, 1.0, 1.0, 1.0, 1.0).is_err());
    assert!(BlendedNoiseParameters::new(1.0, 1000.1, 1.0, 1.0, 1.0).is_err());
    assert!(BlendedNoiseParameters::new(1.0, 1.0, f64::NAN, 1.0, 1.0).is_err());
    assert!(BlendedNoiseParameters::new(1.0, 1.0, 1.0, 1.0, 0.99).is_err());
    assert!(BlendedNoiseParameters::new(1.0, 1.0, 1.0, 1.0, 8.01).is_err());

    let mut rng = LegacyRandom::new(54321);
    assert!(build_smeared_fbm(&mut rng, 1, 1.0, 1.0).is_err());

    // Legacy world seed vs direct LegacyRandom::new(seed).
    let b1 = BlendedNoise::from_legacy_world_seed(42, BlendedNoiseParameters::NETHER);
    let mut l_rng = LegacyRandom::new(42);
    let b2 = BlendedNoise::new(&mut l_rng, BlendedNoiseParameters::NETHER);
    assert_eq!(b1, b2);

    // Positional factory seeding via "minecraft:terrain".
    let mut root = XoroshiroRandom::new(42);
    let factory = root.fork_positional();
    let b_pos = BlendedNoise::from_positional_factory(&factory, BlendedNoiseParameters::END);
    let mut direct_rng = factory.from_hash_of("minecraft:terrain");
    let b_dir = BlendedNoise::new(&mut direct_rng, BlendedNoiseParameters::END);
    assert_eq!(b_pos, b_dir);
}

#[test]
fn fixed_seed_biome_and_end_island_simplex_noises() {
    let temp = biome_temperature_noise();
    let info = biome_info_noise();
    let frozen = biome_frozen_temperature_noise();
    let end = end_island_noise(123_456);

    assert_eq!(temp.state().xo(), 0.0);
    assert_eq!(info.state().xo(), 0.0);
    assert_eq!(end.state().xo(), 0.0);
    assert_ne!(temp.state().permutation(), info.state().permutation());

    // Frozen temperature is a 3-layer simplex stack sequentially drawn from LegacyRandom(3456).
    assert_eq!(frozen.layers().len(), 3);
    assert_eq!(frozen.layers()[0].frequency(), 1.0);
    assert_eq!(frozen.layers()[0].amplitude(), 0.142_857_15_f32);
    assert_eq!(frozen.layers()[1].frequency(), 0.5);
    assert_eq!(frozen.layers()[1].amplitude(), 0.285_714_3_f32);
    assert_eq!(frozen.layers()[2].frequency(), 0.25);
    assert_eq!(frozen.layers()[2].amplitude(), 0.571_428_6_f32);

    let mut r_frozen = LegacyRandom::new(3456);
    for layer in frozen.layers() {
        let expected = SimplexNoise::new_without_offset(&mut r_frozen);
        assert_eq!(layer.noise().state(), expected.state());
    }

    // End island noise matches LegacyRandom(seed) + consume_count(17292) + zero-offset simplex.
    let mut r_end = LegacyRandom::new(123_456);
    r_end.consume_count(17_292);
    let expected_end = SimplexNoise::new_without_offset(&mut r_end);
    assert_eq!(end, expected_end);
}

#[test]
fn octave_stack_flattening_and_empty_stack() {
    let empty = OctaveStack::empty();
    assert_eq!(empty.sample(1.0, 2.0, 3.0), 0.0_f32);
    assert_eq!(empty.sample_2d(1.0, 2.0), 0.0_f32);
    assert_eq!(empty.range(), Interval::exact(0.0_f32));

    let mut rng = XoroshiroRandom::new(77);
    let p1 = PerlinNoise::new(&mut rng);
    let p2 = PerlinNoise::new(&mut rng);

    let mut sub = OctaveStackBuilder::new();
    sub.add_layer(p1.clone(), 0.5, 0.25_f32);
    sub.add_layer(p2.clone(), 1.0, 0.75_f32);
    let sub_stack = sub.build();

    let mut outer = OctaveStackBuilder::new();
    outer.add_stack(&sub_stack, 2.0, 0.5_f32);
    let flat = outer.build();
    assert_eq!(flat.layers().len(), 2);
    assert_eq!(flat.layers()[0].frequency(), 1.0);
    assert_eq!(flat.layers()[0].amplitude(), 0.125_f32);
    assert_eq!(flat.layers()[1].frequency(), 2.0);
    assert_eq!(flat.layers()[1].amplitude(), 0.375_f32);
}

#[test]
fn all_generators_stay_within_reported_conservative_range() {
    let mut rng = XoroshiroRandom::new(2026_1124);
    let perlin = PerlinNoise::new(&mut rng);
    let smeared = SmearedPerlinNoise::new(&mut rng, 1.5);
    let simplex = SimplexNoise::new(&mut rng);
    let normal = NormalNoise::from_parity(&mut rng, -6, vec![1.0, 1.0, 2.0, 0.5]).unwrap();
    let blended = BlendedNoise::new(&mut rng, BlendedNoiseParameters::OVERWORLD);

    let check_noise = |name: &str, noise: &dyn Noise, x: f64, y: f64, z: f64| {
        let (lo, hi) = noise.range().bounds().expect("expected finite bounds");
        let v3 = noise.sample(x, y, z);
        let v2 = noise.sample_2d(x, z);
        assert!(
            (lo..=hi).contains(&v3),
            "{name} 3d sample {v3} outside conservative range [{lo}, {hi}]"
        );
        assert!(
            (lo..=hi).contains(&v2),
            "{name} 2d sample {v2} outside conservative range [{lo}, {hi}]"
        );
    };

    for i in -20..=20 {
        let x = f64::from(i) * 1.37;
        let y = f64::from(i) * -0.83;
        let z = f64::from(i) * 2.11;
        check_noise("perlin", &perlin, x, y, z);
        check_noise("smeared", &smeared, x, y, z);
        check_noise("simplex", &simplex, x, y, z);
        check_noise("normal", &normal, x, y, z);
        let (blo, bhi) = blended.range().bounds().expect("expected finite bounds");
        let bv = blended.sample(i * 7, i * 3 + 64, -i * 5);
        assert!(
            (blo..=bhi).contains(&bv),
            "blended sample {bv} outside conservative range [{blo}, {bhi}]"
        );
    }
}
