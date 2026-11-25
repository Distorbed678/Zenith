//! Comprehensive unit tests for all 44 density function types, interval arithmetic,
//! cubic splines, compiler passes (caching, slicing), and JSON decode errors.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use zenith_codec::json::{ParseMode, parse};
use zenith_codec::log::null_log;
use zenith_codec::resources::{MemoryPack, PathRules};
use zenith_math::{Interval, NormalNoiseParameters, NormalizationMode};
use zenith_registry::Identifier;
use zenith_worldgen::interval::{self, INFINITE, contains, of, of_exact};
use zenith_worldgen::{
    ALL_AXES, AXIS_X, AXIS_Y, AXIS_Z, BlenderHook, CompileError, ContextSampler, DensityFunction,
    DensityVolume, LoadError, NO_AXES, RandomState, RouterField, SamplerContext, WorldgenData,
    decode_density_function_str,
};

fn compile_json(
    json: &str,
    seed: i64,
) -> (DensityFunction, zenith_worldgen::CompiledSampler, usize) {
    let df = decode_density_function_str(json).expect("valid density function JSON");
    let (sampler, caches) = RandomState::compile_function(&df, seed, false).expect("compile");
    (df, sampler, caches)
}

fn eval_point(sampler: &zenith_worldgen::CompiledSampler, x: i32, y: i32, z: i32) -> f32 {
    let mut ctx = SamplerContext::uncached();
    sampler.sample_value(&mut ctx, x, y, z)
}

fn eval_vol(
    sampler: &zenith_worldgen::CompiledSampler,
    vol: DensityVolume,
    cache_count: usize,
) -> Vec<f32> {
    let mut ctx = if cache_count > 0 {
        SamplerContext::caching(cache_count)
    } else {
        SamplerContext::uncached()
    };
    let mut out = vec![0.0_f32; vol.len()];
    sampler.sample_volume(&mut ctx, &mut out, vol);
    out
}

fn worldgen_with_test_noise() -> WorldgenData {
    let mut noises = BTreeMap::new();
    let test_id = Identifier::parse("minecraft:test_noise").unwrap();
    let params =
        NormalNoiseParameters::new(1.0, -4, 3, NormalizationMode::Enabled, vec![]).unwrap();
    noises.insert(test_id, Arc::new(params));

    let nether_t = Identifier::parse("minecraft:nether/temperature").unwrap();
    let nt_params =
        NormalNoiseParameters::new(1.0, -7, 2, NormalizationMode::Legacy, vec![1.0, 1.0]).unwrap();
    noises.insert(nether_t, Arc::new(nt_params));

    let nether_v = Identifier::parse("minecraft:nether/vegetation").unwrap();
    let nv_params =
        NormalNoiseParameters::new(1.0, -7, 2, NormalizationMode::Legacy, vec![1.0, 1.0]).unwrap();
    noises.insert(nether_v, Arc::new(nv_params));

    WorldgenData::from_maps(noises, BTreeMap::new(), BTreeMap::new())
}

// ===========================================================================
// 1. Interval arithmetic tests
// ===========================================================================

#[test]
fn interval_basic_ops_and_edge_cases() {
    assert_eq!(interval::add(of(-2.0, 3.0), of(1.0, 4.0)), of(-1.0, 7.0));
    assert_eq!(interval::sub(of(-2.0, 3.0), of(1.0, 4.0)), of(-6.0, 2.0));
    // Zero-absorbing multiplication: 0 * Inf = 0
    assert_eq!(interval::mul(of_exact(0.0), INFINITE), of_exact(0.0));
    assert_eq!(interval::mul(of(-2.0, 3.0), of(-4.0, 5.0)), of(-12.0, 15.0));

    // Reciprocal
    assert_eq!(interval::reciprocal(of_exact(0.0)), Interval::NaI);
    assert_eq!(interval::reciprocal(of(2.0, 4.0)), of(0.25, 0.5));
    assert_eq!(
        interval::reciprocal(of(-2.0, 0.0)),
        of(f32::NEG_INFINITY, -0.5)
    );
    assert_eq!(interval::reciprocal(of(0.0, 2.0)), of(0.5, f32::INFINITY));
    assert_eq!(interval::reciprocal(of(-1.0, 1.0)), INFINITE);

    // Abs, square, clamp, sign, log
    assert_eq!(interval::abs(of(-3.0, 2.0)), of(0.0, 3.0));
    assert_eq!(interval::abs(of(-5.0, -2.0)), of(2.0, 5.0));
    assert_eq!(interval::square(of(-3.0, 2.0)), of(0.0, 9.0));
    assert_eq!(interval::square(of(2.0, 4.0)), of(4.0, 16.0));
    assert_eq!(interval::clamp(of(-10.0, 10.0), -1.0, 2.0), of(-1.0, 2.0));
    assert_eq!(interval::clamp(of(5.0, 10.0), -1.0, 2.0), of_exact(2.0));
    assert_eq!(interval::clamp(of(-10.0, -5.0), -1.0, 2.0), of_exact(-1.0));
    assert_eq!(interval::sign(of(-2.0, 2.0)), of(-1.0, 1.0));
    assert_eq!(interval::sign(of(0.0, 2.0)), of(0.0, 1.0));
    assert_eq!(interval::sign(of(-2.0, 0.0)), of(-1.0, 0.0));
    assert_eq!(interval::sign(of(1.0, 5.0)), of_exact(1.0));
    assert_eq!(interval::log(of(-5.0, -1.0)), Interval::NaI);
    assert_eq!(interval::log(of(-1.0, 1.0)), of(f32::NEG_INFINITY, 0.0));

    // Pow
    assert_eq!(interval::pow(of(2.0, 3.0), of_exact(2.0)), of(4.0, 9.0));
    assert_eq!(interval::pow(of(-2.0, 3.0), of_exact(2.0)), of(0.0, 9.0));
    assert_eq!(interval::pow(of_exact(1.0), of(-10.0, 10.0)), of_exact(1.0));
    assert_eq!(
        interval::pow(of_exact(2.0), of(0.0, f32::INFINITY)),
        of(1.0, f32::INFINITY)
    );
    assert_eq!(
        interval::pow(of_exact(0.5), of(0.0, f32::INFINITY)),
        of(0.0, 1.0)
    );

    // Lerp
    assert_eq!(
        interval::lerp(of(0.0, 1.0), of(10.0, 20.0), of(30.0, 40.0)),
        of(10.0, 40.0)
    );
    assert_eq!(
        interval::lerp(
            of(0.0, 1.0),
            of_exact(f32::NEG_INFINITY),
            of_exact(f32::INFINITY)
        ),
        INFINITE
    );
}

// ===========================================================================
// 2. Generator functions (12 types)
// ===========================================================================

#[test]
fn constant_object_and_literal_shorthand() {
    let (df1, s1, _) = compile_json("42.5", 0);
    let (df2, s2, _) = compile_json(r#"{"type": "minecraft:constant", "value": 42.5}"#, 0);
    assert_eq!(df1, df2);
    assert_eq!(df1.domain_axes(), NO_AXES);
    assert_eq!(df1.range(), of_exact(42.5));
    assert_eq!(eval_point(&s1, 10, -5, 3), 42.5);
    let vol = DensityVolume::unit(2, 3, 2, 0, 0, 0).unwrap();
    assert_eq!(eval_vol(&s2, vol, 0), vec![42.5; 12]);
}

#[derive(Debug)]
struct MockContextSampler(f32);
impl ContextSampler for MockContextSampler {
    fn sample_value(&self, bx: i32, by: i32, bz: i32) -> f32 {
        self.0 + (bx + by + bz) as f32
    }
}

#[derive(Debug)]
struct MockBlender;
impl BlenderHook for MockBlender {
    fn blend_density(&self, _bx: i32, _by: i32, _bz: i32, density: f32) -> f32 {
        density * 2.0 + 1.0
    }
}

#[test]
fn context_bound_generators_and_blend_density() {
    let (df_a, sa, _) = compile_json(r#"{"type": "minecraft:blend_alpha"}"#, 0);
    let (df_o, so, _) = compile_json(r#"{"type": "minecraft:blend_offset"}"#, 0);
    let (df_b, sb, _) = compile_json(r#"{"type": "minecraft:beardifier"}"#, 0);
    let (df_bd, sbd, _) = compile_json(r#"{"type": "minecraft:blend_density", "input": 3.0}"#, 0);

    assert_eq!(df_a.domain_axes(), AXIS_X | AXIS_Z);
    assert_eq!(df_a.range(), of(0.0, 1.0));
    assert_eq!(df_o.domain_axes(), AXIS_X | AXIS_Z);
    assert_eq!(df_o.range(), INFINITE);
    assert_eq!(df_b.domain_axes(), ALL_AXES);
    assert_eq!(df_b.range(), INFINITE);
    assert_eq!(df_bd.range(), of_exact(3.0));

    // Default fallbacks without context hooks
    assert_eq!(eval_point(&sa, 1, 2, 3), 1.0);
    assert_eq!(eval_point(&so, 1, 2, 3), 0.0);
    assert_eq!(eval_point(&sb, 1, 2, 3), 0.0);
    assert_eq!(eval_point(&sbd, 1, 2, 3), 3.0);

    // With context hooks attached
    let mut ctx = SamplerContext::uncached()
        .with_beardifier(Arc::new(MockContextSampler(10.0)))
        .with_blender(
            Arc::new(MockContextSampler(0.5)),
            Arc::new(MockContextSampler(2.0)),
            Arc::new(MockBlender),
        );
    // blend_alpha/blend_offset are wrapped in Slice(Y, 0, ...) by SliceUniformAxes, so y=0!
    assert_eq!(sa.sample_value(&mut ctx, 1, 99, 3), 0.5 + (1 + 3) as f32);
    assert_eq!(so.sample_value(&mut ctx, 1, 99, 3), 2.0 + (1 + 3) as f32);
    assert_eq!(
        sb.sample_value(&mut ctx, 1, 2, 3),
        10.0 + (1 + 2 + 3) as f32
    );
    assert_eq!(sbd.sample_value(&mut ctx, 1, 2, 3), 7.0);
}

#[test]
fn noise_unshifted_shifted_and_nether_special_cases() {
    let data = worldgen_with_test_noise();
    let unshifted_j = parse(
        r#"{"type": "minecraft:noise", "noise": "minecraft:test_noise", "xz_scale": 0.25, "y_scale": 0.0}"#,
        ParseMode::Data,
    )
    .unwrap();
    let df_unshifted = data.decode_density_function(&unshifted_j).unwrap();
    assert_eq!(df_unshifted.domain_axes(), AXIS_X | AXIS_Z);
    let (s_unshifted, _) = RandomState::compile_function(&df_unshifted, 12345, false).unwrap();

    let vol = DensityVolume::unit(4, 3, 4, -2, 10, -2).unwrap();
    let buf = eval_vol(&s_unshifted, vol, 0);
    for iz in 0..4 {
        for ix in 0..4 {
            let p = eval_point(&s_unshifted, vol.block_x(ix), 10, vol.block_z(iz));
            assert!(contains(df_unshifted.range(), p));
            for iy in 0..3 {
                // y_scale == 0.0 => constant along Y
                assert_eq!(buf[vol.index(ix, iy, iz)], buf[vol.index(ix, 0, iz)]);
            }
        }
    }

    // XZ-shifted and XYZ-shifted
    let shifted_xz_j = parse(
        r#"{
            "type": "minecraft:noise",
            "noise": "minecraft:test_noise",
            "xz_scale": 0.25,
            "y_scale": 0.5,
            "shift_x": {"type": "minecraft:constant", "value": 1.5}
        }"#,
        ParseMode::Data,
    )
    .unwrap();
    let df_xz = data.decode_density_function(&shifted_xz_j).unwrap();
    let (s_xz, _) = RandomState::compile_function(&df_xz, 12345, false).unwrap();
    let buf_xz = eval_vol(&s_xz, vol, 0);
    for iz in 0..4 {
        for ix in 0..4 {
            for iy in 0..3 {
                let p = eval_point(&s_xz, vol.block_x(ix), vol.block_y(iy), vol.block_z(iz));
                assert_eq!(buf_xz[vol.index(ix, iy, iz)].to_bits(), p.to_bits());
            }
        }
    }

    let shifted_xyz_j = parse(
        r#"{
            "type": "minecraft:noise",
            "noise": "minecraft:test_noise",
            "xz_scale": 0.25,
            "y_scale": 0.5,
            "shift_y": 2.0
        }"#,
        ParseMode::Data,
    )
    .unwrap();
    let df_xyz = data.decode_density_function(&shifted_xyz_j).unwrap();
    let (s_xyz, _) = RandomState::compile_function(&df_xyz, 12345, false).unwrap();
    let buf_xyz = eval_vol(&s_xyz, vol, 0);
    for iz in 0..4 {
        for ix in 0..4 {
            for iy in 0..3 {
                let p = eval_point(&s_xyz, vol.block_x(ix), vol.block_y(iy), vol.block_z(iz));
                assert_eq!(buf_xyz[vol.index(ix, iy, iz)].to_bits(), p.to_bits());
            }
        }
    }

    // Legacy Nether temperature/vegetation ignore legacy_random_source flag
    let nt_j = parse(
        r#"{"type": "minecraft:noise", "noise": "minecraft:nether/temperature", "xz_scale": 0.25, "y_scale": 0.0}"#,
        ParseMode::Data,
    )
    .unwrap();
    let df_nt = data.decode_density_function(&nt_j).unwrap();
    let (s_nt_xoro, _) = RandomState::compile_function(&df_nt, 999, false).unwrap();
    let (s_nt_leg, _) = RandomState::compile_function(&df_nt, 999, true).unwrap();
    assert_eq!(
        eval_point(&s_nt_xoro, 12, 0, -34).to_bits(),
        eval_point(&s_nt_leg, 12, 0, -34).to_bits()
    );
}

#[test]
fn shift_shift_a_and_shift_b() {
    let data = worldgen_with_test_noise();
    for (ty, expected_axes) in [
        ("minecraft:shift", ALL_AXES),
        ("minecraft:shift_a", AXIS_X | AXIS_Z),
        ("minecraft:shift_b", AXIS_X | AXIS_Z),
    ] {
        let j = parse(
            &format!(r#"{{"type": "{ty}", "noise": "minecraft:test_noise"}}"#),
            ParseMode::Data,
        )
        .unwrap();
        let df = data.decode_density_function(&j).unwrap();
        assert_eq!(df.domain_axes(), expected_axes);
        let (s, _) = RandomState::compile_function(&df, 77, false).unwrap();
        let vol = DensityVolume::unit(4, 3, 4, -4, 0, 8).unwrap();
        let buf = eval_vol(&s, vol, 0);
        for iz in 0..4 {
            for ix in 0..4 {
                for iy in 0..3 {
                    let p = eval_point(&s, vol.block_x(ix), vol.block_y(iy), vol.block_z(iz));
                    let v = buf[vol.index(ix, iy, iz)];
                    assert!(contains(df.range(), p));
                    assert!(contains(df.range(), v));
                    // 0.25 and 4.0 are exact powers of 2, so shift_b point and volume should agree closely
                    assert!((p - v).abs() < 1e-5, "{ty}: point={p} vol={v}");
                }
            }
        }
    }
}

#[test]
fn end_outer_islands_bounds_symmetry_and_legacy_rng() {
    let (df, s_xoro, _) = compile_json(r#"{"type": "minecraft:end_outer_islands"}"#, 42);
    let (s_leg, _) = RandomState::compile_function(&df, 42, true).unwrap();
    assert_eq!(df.domain_axes(), AXIS_X | AXIS_Z);
    assert_eq!(df.range(), of(-0.84375, 0.5625));

    // Always uses LegacyRandom(worldSeed) regardless of legacy_random_source
    assert_eq!(
        eval_point(&s_xoro, 1200, 50, -1500).to_bits(),
        eval_point(&s_leg, 1200, 50, -1500).to_bits()
    );
    // Inside central 64-chunk radius => no outer islands => (-100 - 8) / 128 = -0.84375
    assert_eq!(eval_point(&s_xoro, 0, 0, 0), -0.84375_f32);
    // Negative coord truncation quirk: bx = -7 => -7 / 8 == 0 == 7 / 8
    assert_eq!(
        eval_point(&s_xoro, -7, 0, 1200).to_bits(),
        eval_point(&s_xoro, 7, 0, 1200).to_bits()
    );
    // Volume broadcast matches point
    let vol = DensityVolume::unit(3, 4, 3, 1100, -10, 1100).unwrap();
    let buf = eval_vol(&s_xoro, vol, 0);
    for iz in 0..3 {
        for ix in 0..3 {
            let p = eval_point(&s_xoro, vol.block_x(ix), 0, vol.block_z(iz));
            assert!(contains(df.range(), p));
            for iy in 0..4 {
                assert_eq!(buf[vol.index(ix, iy, iz)].to_bits(), p.to_bits());
            }
        }
    }
}

#[test]
fn distance_to_point_all_four_metrics() {
    let cases = [
        ("euclidean", 5.0_f32),
        ("euclidean_squared", 25.0_f32),
        ("manhattan", 7.0_f32),
        ("chebyshev", 4.0_f32),
    ];
    for (metric, expected) in cases {
        let json = format!(
            r#"{{"type": "minecraft:distance_to_point", "point": [10, 20, 30], "metric": "{metric}"}}"#
        );
        let (df, s, _) = compile_json(&json, 0);
        assert_eq!(df.domain_axes(), ALL_AXES);
        assert_eq!(df.range(), of(0.0, f32::INFINITY));
        // dx = 3, dy = 4, dz = 0
        assert_eq!(eval_point(&s, 7, 16, 30), expected);
        let vol = DensityVolume::unit(1, 1, 1, 7, 16, 30).unwrap();
        assert_eq!(eval_vol(&s, vol, 0), vec![expected]);
    }
}

#[test]
fn gradient_all_axes_tilings_and_negative_range() {
    // clamp_to_edge (default) on Y
    let (df_y, sy, _) = compile_json(
        r#"{
            "type": "minecraft:gradient",
            "axis": "y",
            "from_coordinate": 0,
            "to_coordinate": 10,
            "from_value": 1.0,
            "to_value": -1.0
        }"#,
        0,
    );
    assert_eq!(df_y.domain_axes(), AXIS_Y);
    assert_eq!(df_y.range(), of(-1.0, 1.0));
    assert_eq!(eval_point(&sy, 0, -5, 0), 1.0);
    assert_eq!(eval_point(&sy, 0, 5, 0), 0.0);
    assert_eq!(eval_point(&sy, 0, 20, 0), -1.0);

    // repeat with negative coordinateRange (from > to) on X
    let (df_rep, s_rep, _) = compile_json(
        r#"{
            "type": "minecraft:gradient",
            "axis": "x",
            "tiling": "repeat",
            "from_coordinate": 10,
            "to_coordinate": 6,
            "from_value": 0.0,
            "to_value": 4.0
        }"#,
        0,
    );
    assert_eq!(df_rep.domain_axes(), AXIS_X);
    let vol = DensityVolume::unit(8, 2, 2, 3, 0, 0).unwrap();
    let buf = eval_vol(&s_rep, vol, 0);
    for ix in 0..8 {
        let bx = vol.block_x(ix);
        let p = eval_point(&s_rep, bx, 0, 0);
        assert!(contains(df_rep.range(), p));
        assert_eq!(buf[vol.index(ix, 0, 0)].to_bits(), p.to_bits());
        assert_eq!(buf[vol.index(ix, 1, 1)].to_bits(), p.to_bits());
    }

    // mirrored_repeat on Z
    let (df_mir, s_mir, _) = compile_json(
        r#"{
            "type": "minecraft:gradient",
            "axis": "z",
            "tiling": "mirrored_repeat",
            "from_coordinate": 0,
            "to_coordinate": 4,
            "from_value": 0.0,
            "to_value": 4.0
        }"#,
        0,
    );
    assert_eq!(df_mir.domain_axes(), AXIS_Z);
    // z = 0,1,2,3,4,5,6,7,8 -> 0,1,2,3,4,3,2,1,0
    let expected = [0.0, 1.0, 2.0, 3.0, 4.0, 3.0, 2.0, 1.0, 0.0];
    for (z, &exp) in expected.iter().enumerate() {
        assert_eq!(eval_point(&s_mir, 0, 0, z as i32), exp);
    }
    let vol_z = DensityVolume::unit(2, 2, 9, 0, 0, 0).unwrap();
    let buf_z = eval_vol(&s_mir, vol_z, 0);
    for (iz, &exp) in expected.iter().enumerate() {
        assert_eq!(buf_z[vol_z.index(1, 1, iz)], exp);
    }
}

#[test]
fn old_blended_noise_seeding_and_volume() {
    let json = r#"{
        "type": "minecraft:old_blended_noise",
        "xz_scale": 0.25,
        "y_scale": 0.125,
        "xz_factor": 80.0,
        "y_factor": 160.0,
        "smear_scale_multiplier": 8.0
    }"#;
    let (df, s_xoro, _) = compile_json(json, 12345);
    let (s_leg, _) = RandomState::compile_function(&df, 12345, true).unwrap();
    assert_eq!(df.domain_axes(), ALL_AXES);
    let p_xoro = eval_point(&s_xoro, 12, 64, -7);
    let p_leg = eval_point(&s_leg, 12, 64, -7);
    assert!(contains(df.range(), p_xoro));
    assert!(contains(df.range(), p_leg));
    assert_ne!(p_xoro.to_bits(), p_leg.to_bits());

    let vol = DensityVolume::unit(3, 3, 3, 10, 62, -8).unwrap();
    let buf = eval_vol(&s_xoro, vol, 0);
    for &v in &buf {
        assert!(contains(df.range(), v));
    }
}

// ===========================================================================
// 3. All 11 unary functions
// ===========================================================================

#[test]
fn all_eleven_unary_functions_eval_and_bounds() {
    let cases: &[(&str, f32, f32)] = &[
        ("abs", -3.5, 3.5),
        ("square", -3.0, 9.0),
        ("cube", -2.0, -8.0),
        ("sqrt", 9.0, 3.0),
        ("half_negative", -4.0, -2.0),
        ("half_negative", 4.0, 4.0),
        ("quarter_negative", -8.0, -2.0),
        ("quarter_negative", 8.0, 8.0),
        ("reciprocal", 4.0, 0.25),
        ("negate", 2.5, -2.5),
        (
            "squeeze",
            0.5,
            (0.5_f32 / 2.0) - ((0.5_f32 * 0.5 * 0.5) / 24.0),
        ),
        ("log", 1.0, 0.0),
        ("sign", -7.0, -1.0),
        ("sign", 0.0, 0.0),
        ("sign", 7.0, 1.0),
    ];
    for &(op, input, expected) in cases {
        let json = format!(r#"{{"type": "minecraft:{op}", "input": {input}}}"#);
        let (df, s, _) = compile_json(&json, 0);
        let p = eval_point(&s, 0, 0, 0);
        assert_eq!(p.to_bits(), expected.to_bits(), "op={op} input={input}");
        assert!(
            contains(df.range(), p),
            "op={op} range={:?} p={p}",
            df.range()
        );
        let vol = DensityVolume::unit(2, 2, 2, 0, 0, 0).unwrap();
        let buf = eval_vol(&s, vol, 0);
        assert_eq!(buf, vec![expected; 8]);
    }
}

// ===========================================================================
// 4. All 4 rounding functions
// ===========================================================================

#[test]
fn all_four_rounding_functions_unit_and_multiple() {
    let unit_cases: &[(&str, f32, f32)] = &[
        ("floor", -1.2, -2.0),
        ("floor", 1.9, 1.0),
        ("round", -0.5, 0.0), // Java ties toward +Inf!
        ("round", 0.5, 1.0),
        ("round", -1.5, -1.0),
        ("ceil", -1.8, -1.0),
        ("ceil", 1.1, 2.0),
        ("truncate", -1.8, -1.0),
        ("truncate", 1.8, 1.0),
    ];
    for &(op, input, expected) in unit_cases {
        let json = format!(r#"{{"type": "minecraft:{op}", "input": {input}}}"#);
        let (df, s, _) = compile_json(&json, 0);
        let p = eval_point(&s, 0, 0, 0);
        assert_eq!(p, expected, "op={op} input={input}");
        assert!(contains(df.range(), p));
    }

    // Custom multiple (including multiple = 0.0 returning input unchanged)
    let (df_m, s_m, _) = compile_json(
        r#"{"type": "minecraft:floor", "input": 7.5, "multiple": 2.0}"#,
        0,
    );
    assert_eq!(eval_point(&s_m, 0, 0, 0), 6.0);
    assert!(contains(df_m.range(), 6.0));

    let (_, s_zero_m, _) = compile_json(
        r#"{"type": "minecraft:round", "input": 3.75, "multiple": 0.0}"#,
        0,
    );
    assert_eq!(eval_point(&s_zero_m, 0, 0, 0), 3.75);
    let vol = DensityVolume::unit(2, 2, 1, 0, 0, 0).unwrap();
    assert_eq!(eval_vol(&s_zero_m, vol, 0), vec![3.75; 4]);

    // Edge Case #10: RoundFunction(ROUND) saturates to i32 and maps NaN -> 0.0f
    let (_, s_round_nan, _) = compile_json(
        r#"{"type": "minecraft:round", "input": {"type": "minecraft:sqrt", "input": -1.0}}"#,
        0,
    );
    assert_eq!(eval_point(&s_round_nan, 0, 0, 0), 0.0);

    let (_, s_round_huge, _) = compile_json(
        r#"{"type": "minecraft:round", "input": {"type": "minecraft:mul", "left": 1000000.0, "right": 1000000.0}}"#,
        0,
    );
    assert_eq!(eval_point(&s_round_huge, 0, 0, 0), i32::MAX as f32);
}

// ===========================================================================
// 5. All 6 binary functions + quirks (Edge Cases #5, #6, #7, #8)
// ===========================================================================

#[test]
fn all_six_binary_functions_and_quirks() {
    let basic: &[(&str, f32, f32, f32)] = &[
        ("add", 3.0, 4.0, 7.0),
        ("sub", 3.0, 4.0, -1.0),
        ("mul", 3.0, 4.0, 12.0),
        ("div", 12.0, 4.0, 3.0),
        ("min", 3.0, 4.0, 3.0),
        ("max", 3.0, 4.0, 4.0),
    ];
    for &(op, l, r, exp) in basic {
        let json = format!(r#"{{"type": "minecraft:{op}", "left": {l}, "right": {r}}}"#);
        let (df, s, _) = compile_json(&json, 0);
        assert_eq!(eval_point(&s, 0, 0, 0), exp);
        assert!(contains(df.range(), exp));
    }

    // Edge Case #5: div by constant compiles to ConstMul(left, 1.0f / c)
    let (_, s_div_const, _) = compile_json(
        r#"{
            "type": "minecraft:div",
            "left": {"type": "minecraft:gradient", "axis": "x", "from_coordinate": 0, "to_coordinate": 100, "from_value": 0.0, "to_value": 100.0},
            "right": 7.0
        }"#,
        0,
    );
    for x in 0..100 {
        let got = eval_point(&s_div_const, x, 0, 0);
        let expected_mul_recip = (x as f32) * (1.0_f32 / 7.0_f32);
        assert_eq!(got.to_bits(), expected_mul_recip.to_bits());
    }

    // Edge Case #6: mul and div zero short-circuit ONLY in point mode, NOT volume mode
    // left = 0.0 (non-const gradient at x=0), right = reciprocal(gradient at x=0) = +Inf
    let mul_zero_inf = r#"{
        "type": "minecraft:mul",
        "left": {"type": "minecraft:gradient", "axis": "x", "from_coordinate": 0, "to_coordinate": 10, "from_value": 0.0, "to_value": 10.0},
        "right": {"type": "minecraft:reciprocal", "input": {"type": "minecraft:gradient", "axis": "x", "from_coordinate": 0, "to_coordinate": 10, "from_value": 0.0, "to_value": 10.0}}
    }"#;
    let (_, s_mzi, _) = compile_json(mul_zero_inf, 0);
    assert_eq!(eval_point(&s_mzi, 0, 0, 0).to_bits(), 0.0_f32.to_bits());
    let vol = DensityVolume::unit(1, 1, 1, 0, 0, 0).unwrap();
    assert!(eval_vol(&s_mzi, vol, 0)[0].is_nan());

    // Edge Case #7: min/max volume ignores NaN on right while point propagates NaN
    // left = gradient(x=0 -> 1.0), right = sqrt(gradient(x=0 -> -1.0)) = NaN
    let min_nan = r#"{
        "type": "minecraft:min",
        "left": {"type": "minecraft:gradient", "axis": "x", "from_coordinate": 0, "to_coordinate": 10, "from_value": 1.0, "to_value": 2.0},
        "right": {"type": "minecraft:sqrt", "input": {"type": "minecraft:gradient", "axis": "x", "from_coordinate": 0, "to_coordinate": 10, "from_value": -1.0, "to_value": 1.0}}
    }"#;
    let (_, s_mn, _) = compile_json(min_nan, 0);
    assert!(eval_point(&s_mn, 0, 0, 0).is_nan());
    assert_eq!(eval_vol(&s_mn, vol, 0)[0], 1.0);

    // Edge Case #8: non-overlapping interval pruning
    let min_disjoint = r#"{
        "type": "minecraft:min",
        "left": {"type": "minecraft:gradient", "axis": "x", "from_coordinate": 0, "to_coordinate": 10, "from_value": 0.0, "to_value": 1.0},
        "right": {"type": "minecraft:gradient", "axis": "x", "from_coordinate": 0, "to_coordinate": 10, "from_value": 5.0, "to_value": 10.0}
    }"#;
    let (df_md, s_md, _) = compile_json(min_disjoint, 0);
    assert_eq!(df_md.range(), of(0.0, 1.0));
    assert_eq!(eval_point(&s_md, 5, 0, 0), 0.5);
}

// ===========================================================================
// 6. Pow, Lerp, Clamp, RangeChoice, IntervalSelect, Spline, Slice,
//    Interpolated, Cache, FindTopSurface
// ===========================================================================

#[test]
fn pow_special_and_general_exponents() {
    for (exp, expected) in [
        (0.5_f32, 2.0_f32),
        (-0.5, 0.5),
        (1.0, 4.0),
        (-1.0, 0.25),
        (2.0, 16.0),
        (-2.0, 1.0 / 16.0),
        (3.0, 64.0),
        (-3.0, 1.0 / 64.0),
        (2.5, 32.0),
    ] {
        let json = format!(
            r#"{{
                "type": "minecraft:pow",
                "base": {{"type": "minecraft:gradient", "axis": "x", "from_coordinate": 0, "to_coordinate": 10, "from_value": 4.0, "to_value": 4.0}},
                "exponent": {exp}
            }}"#
        );
        let (df, s, _) = compile_json(&json, 0);
        let p = eval_point(&s, 0, 0, 0);
        assert_eq!(p, expected, "exp={exp}");
        assert!(contains(df.range(), p));
        let vol = DensityVolume::unit(1, 1, 1, 0, 0, 0).unwrap();
        assert_eq!(eval_vol(&s, vol, 0)[0], expected);
    }

    // Constant base
    let (df_cb, s_cb, _) = compile_json(
        r#"{
            "type": "minecraft:pow",
            "base": 2.0,
            "exponent": {"type": "minecraft:gradient", "axis": "x", "from_coordinate": 0, "to_coordinate": 4, "from_value": 0.0, "to_value": 4.0}
        }"#,
        0,
    );
    assert_eq!(eval_point(&s_cb, 3, 0, 0), 8.0);
    assert!(contains(df_cb.range(), 8.0));
}

#[test]
fn lerp_clamp_range_choice_and_interval_select() {
    // Lerp with exact endpoint short-circuit
    let (df_l, s_l, _) = compile_json(
        r#"{
            "type": "minecraft:lerp",
            "alpha": {"type": "minecraft:gradient", "axis": "x", "from_coordinate": 0, "to_coordinate": 4, "from_value": 0.0, "to_value": 1.0},
            "first": 10.0,
            "second": 20.0
        }"#,
        0,
    );
    assert_eq!(df_l.range(), of(10.0, 20.0));
    assert_eq!(eval_point(&s_l, 0, 0, 0), 10.0);
    assert_eq!(eval_point(&s_l, 2, 0, 0), 15.0);
    assert_eq!(eval_point(&s_l, 4, 0, 0), 20.0);

    // Clamp
    let (df_c, s_c, _) = compile_json(
        r#"{
            "type": "minecraft:clamp",
            "input": {"type": "minecraft:gradient", "axis": "x", "from_coordinate": 0, "to_coordinate": 10, "from_value": -5.0, "to_value": 5.0},
            "min": -1.0,
            "max": 2.0
        }"#,
        0,
    );
    assert_eq!(df_c.range(), of(-1.0, 2.0));
    assert_eq!(eval_point(&s_c, 0, 0, 0), -1.0);
    assert_eq!(eval_point(&s_c, 5, 0, 0), 0.0);
    assert_eq!(eval_point(&s_c, 10, 0, 0), 2.0);

    // RangeChoice (inclusive lower, exclusive upper, plus inverted range allowed)
    let (df_rc, s_rc, _) = compile_json(
        r#"{
            "type": "minecraft:range_choice",
            "input": {"type": "minecraft:gradient", "axis": "x", "from_coordinate": 0, "to_coordinate": 10, "from_value": 0.0, "to_value": 10.0},
            "min_inclusive": 2.0,
            "max_exclusive": 5.0,
            "when_in_range": 100.0,
            "when_out_of_range": -100.0
        }"#,
        0,
    );
    assert_eq!(df_rc.range(), of(-100.0, 100.0));
    assert_eq!(eval_point(&s_rc, 1, 0, 0), -100.0);
    assert_eq!(eval_point(&s_rc, 2, 0, 0), 100.0);
    assert_eq!(eval_point(&s_rc, 4, 0, 0), 100.0);
    assert_eq!(eval_point(&s_rc, 5, 0, 0), -100.0);

    // Inverted range_choice decodes cleanly and always picks when_out_of_range
    let (_, s_rc_inv, _) = compile_json(
        r#"{
            "type": "minecraft:range_choice",
            "input": 3.0,
            "min_inclusive": 5.0,
            "max_exclusive": 2.0,
            "when_in_range": 1.0,
            "when_out_of_range": 2.0
        }"#,
        0,
    );
    assert_eq!(eval_point(&s_rc_inv, 0, 0, 0), 2.0);

    // IntervalSelect (multi-threshold and single-threshold)
    let (df_is, s_is, _) = compile_json(
        r#"{
            "type": "minecraft:interval_select",
            "input": {"type": "minecraft:gradient", "axis": "x", "from_coordinate": 0, "to_coordinate": 10, "from_value": 0.0, "to_value": 10.0},
            "thresholds": [3.0, 7.0],
            "functions": [-1.0, 0.0, 1.0]
        }"#,
        0,
    );
    assert_eq!(df_is.range(), of(-1.0, 1.0));
    assert_eq!(eval_point(&s_is, 2, 0, 0), -1.0);
    assert_eq!(eval_point(&s_is, 3, 0, 0), 0.0);
    assert_eq!(eval_point(&s_is, 6, 0, 0), 0.0);
    assert_eq!(eval_point(&s_is, 7, 0, 0), 1.0);
    let vol = DensityVolume::unit(10, 1, 1, 0, 0, 0).unwrap();
    let buf = eval_vol(&s_is, vol, 0);
    assert_eq!(
        buf,
        vec![-1.0, -1.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0]
    );
}

#[test]
fn cubic_spline_hermite_derivatives_extrapolation_and_nesting() {
    // 1D cubic Hermite spline with non-zero derivatives at endpoints for extrapolation
    let json = r#"{
        "type": "minecraft:spline",
        "spline": {
            "coordinate": {
                "type": "minecraft:gradient",
                "axis": "x",
                "from_coordinate": -10,
                "to_coordinate": 10,
                "from_value": -10.0,
                "to_value": 10.0
            },
            "points": [
                {"location": 0.0, "value": 1.0, "derivative": 2.0},
                {"location": 4.0, "value": 5.0, "derivative": -1.0}
            ]
        }
    }"#;
    let (df, s, _) = compile_json(json, 0);
    assert_eq!(df.domain_axes(), AXIS_X);

    // Left extrapolation: u = -2.0 -> 1.0 + 2.0 * (-2.0 - 0.0) = -3.0
    assert_eq!(eval_point(&s, -2, 0, 0), -3.0);
    // Exact knots
    assert_eq!(eval_point(&s, 0, 0, 0), 1.0);
    assert_eq!(eval_point(&s, 4, 0, 0), 5.0);
    // Right extrapolation: u = 6.0 -> 5.0 + (-1.0) * (6.0 - 4.0) = 3.0
    assert_eq!(eval_point(&s, 6, 0, 0), 3.0);
    // Interior midpoint u = 2.0, t = 0.5:
    // y1=1, y2=5, d1=2, d2=-1, dx=4
    // a = 2*4 - (5-1) = 4; b = -(-1)*4 + (5-1) = 8
    // lerp(0.5, 1, 5) + 0.25 * lerp(0.5, 4, 8) = 3.0 + 0.25 * 6.0 = 4.5
    assert_eq!(eval_point(&s, 2, 0, 0), 4.5);

    // Every value across [-10..=10] lies inside conservative spline range
    let vol = DensityVolume::unit(21, 1, 1, -10, 0, 0).unwrap();
    let buf = eval_vol(&s, vol, 0);
    for (ix, &v) in buf.iter().enumerate() {
        let bx = vol.block_x(ix);
        assert_eq!(v.to_bits(), eval_point(&s, bx, 0, 0).to_bits());
        assert!(
            contains(df.range(), v),
            "x={bx} v={v} range={:?}",
            df.range()
        );
    }

    // Nested 2D spline with shared coordinate deduplication + constant spline leaf
    let nested_json = r#"{
        "type": "minecraft:spline",
        "spline": {
            "coordinate": {"type": "minecraft:gradient", "axis": "x", "from_coordinate": 0, "to_coordinate": 10, "from_value": 0.0, "to_value": 10.0},
            "points": [
                {
                    "location": 0.0,
                    "derivative": 0.0,
                    "value": {
                        "coordinate": {"type": "minecraft:gradient", "axis": "z", "from_coordinate": 0, "to_coordinate": 10, "from_value": 0.0, "to_value": 10.0},
                        "points": [
                            {"location": 0.0, "value": 1.0, "derivative": 0.0},
                            {"location": 10.0, "value": 3.0, "derivative": 0.0}
                        ]
                    }
                },
                {
                    "location": 10.0,
                    "derivative": 0.0,
                    "value": {
                        "coordinate": {"type": "minecraft:gradient", "axis": "z", "from_coordinate": 0, "to_coordinate": 10, "from_value": 0.0, "to_value": 10.0},
                        "points": [
                            {"location": 0.0, "value": 5.0, "derivative": 0.0},
                            {"location": 10.0, "value": 7.0, "derivative": 0.0}
                        ]
                    }
                }
            ]
        }
    }"#;
    let (df_n, s_n, _) = compile_json(nested_json, 0);
    assert_eq!(df_n.domain_axes(), AXIS_X | AXIS_Z);
    assert_eq!(eval_point(&s_n, 0, 0, 0), 1.0);
    assert_eq!(eval_point(&s_n, 10, 0, 10), 7.0);
    let vol_n = DensityVolume::unit(3, 2, 3, 0, 0, 0).unwrap();
    let buf_n = eval_vol(&s_n, vol_n, 0);
    assert_eq!(buf_n[vol_n.index(0, 0, 0)], 1.0);

    // Unsorted locations decode cleanly per spec §3 (JSON codec does not enforce ascending order)
    let unsorted = decode_density_function_str(
        r#"{
            "type": "minecraft:spline",
            "spline": {
                "coordinate": 0.0,
                "points": [
                    {"location": 2.0, "value": 10.0, "derivative": 0.0},
                    {"location": 1.0, "value": 20.0, "derivative": 0.0}
                ]
            }
        }"#,
    );
    assert!(unsorted.is_ok());

    // NaN coordinate selects last knot (right extrapolation: u < loc is always false)
    let (_, s_nan_u, _) = compile_json(
        r#"{
            "type": "minecraft:spline",
            "spline": {
                "coordinate": {"type": "minecraft:sqrt", "input": -1.0},
                "points": [
                    {"location": 0.0, "value": 1.0, "derivative": 0.0},
                    {"location": 1.0, "value": 9.0, "derivative": 0.0}
                ]
            }
        }"#,
        0,
    );
    assert_eq!(eval_point(&s_nan_u, 0, 0, 0), 9.0);
}

#[test]
fn interpolated_volume_vs_point_and_aligned_passthrough() {
    // Non-constant input along Y so we can inspect exact fillCell vs lerp3 behavior
    let json = r#"{
        "type": "minecraft:interpolated",
        "cell_size_xz": 4,
        "cell_size_y": 8,
        "input": {
            "type": "minecraft:add",
            "left": {"type": "minecraft:gradient", "axis": "x", "from_coordinate": -16, "to_coordinate": 16, "from_value": -1.0, "to_value": 1.0},
            "right": {"type": "minecraft:gradient", "axis": "y", "from_coordinate": 0, "to_coordinate": 16, "from_value": 0.0, "to_value": 1.0}
        }
    }"#;
    let (df, s, _) = compile_json(json, 0);
    assert_eq!(df.domain_axes(), AXIS_X | AXIS_Y);

    // At cell corners, point and volume match exact input
    assert_eq!(eval_point(&s, 0, 0, 0), 0.0);
    assert_eq!(eval_point(&s, 4, 8, 0), 0.25 + 0.5);

    // Unit-step volume across negative and positive coords
    let vol = DensityVolume::unit(6, 10, 5, -2, 0, -1).unwrap();
    let buf = eval_vol(&s, vol, 0);
    for iz in 0..5 {
        for ix in 0..6 {
            for iy in 0..10 {
                let p = eval_point(&s, vol.block_x(ix), vol.block_y(iy), vol.block_z(iz));
                let v = buf[vol.index(ix, iy, iz)];
                assert!(contains(df.range(), v));
                assert!((p - v).abs() < 1e-6, "p={p} v={v}");
            }
        }
    }

    // Aligned cell-grid passthrough (step = (4, 8, 4), origin aligned to cell)
    let aligned_vol = DensityVolume::new([3, 2, 2], [0, 0, 0], [4, 8, 4]).unwrap();
    let aligned_buf = eval_vol(&s, aligned_vol, 0);
    for iz in 0..2 {
        for ix in 0..3 {
            for iy in 0..2 {
                let p = eval_point(
                    &s,
                    aligned_vol.block_x(ix),
                    aligned_vol.block_y(iy),
                    aligned_vol.block_z(iz),
                );
                assert_eq!(
                    aligned_buf[aligned_vol.index(ix, iy, iz)].to_bits(),
                    p.to_bits()
                );
            }
        }
    }

    // Non-unit, non-aligned step (branch 3)
    let sub_vol = DensityVolume::new([2, 2, 2], [1, 1, 1], [2, 2, 2]).unwrap();
    let sub_buf = eval_vol(&s, sub_vol, 0);
    assert_eq!(sub_buf.len(), 8);
}

#[test]
fn slice_fusion_and_find_top_surface() {
    // Fused XZ slice
    let (df_xz, s_xz, _) = compile_json(
        r#"{
            "type": "minecraft:slice",
            "axis": "x",
            "coordinate": 5,
            "input": {
                "type": "minecraft:slice",
                "axis": "z",
                "coordinate": 7,
                "input": {
                    "type": "minecraft:distance_to_point",
                    "point": [5, 0, 7],
                    "metric": "manhattan"
                }
            }
        }"#,
        0,
    );
    assert_eq!(df_xz.domain_axes(), AXIS_Y);
    assert_eq!(eval_point(&s_xz, 999, 12, -999), 12.0);
    let vol = DensityVolume::unit(3, 4, 3, 100, 10, -100).unwrap();
    let buf = eval_vol(&s_xz, vol, 0);
    for iy in 0..4 {
        assert_eq!(buf[vol.index(2, iy, 2)], (10 + iy) as f32);
    }

    // FindTopSurface: density > 0 for y < 60, upper_bound = 80, lower_bound = 0, cell_height = 8
    // Probes: 80, 72, 64, 56 -> first > 0 is 56!
    let (df_fts, s_fts, _) = compile_json(
        r#"{
            "type": "minecraft:find_top_surface",
            "density": {
                "type": "minecraft:gradient",
                "axis": "y",
                "from_coordinate": 0,
                "to_coordinate": 60,
                "from_value": 1.0,
                "to_value": 0.0
            },
            "upper_bound": 80.0,
            "lower_bound": 0,
            "cell_height": 8
        }"#,
        0,
    );
    assert_eq!(df_fts.domain_axes(), NO_AXES);
    assert_eq!(df_fts.range(), of(0.0, 80.0));
    assert_eq!(eval_point(&s_fts, 0, 123, 0), 56.0);
    let vol_fts = DensityVolume::unit(2, 3, 2, 0, 10, 0).unwrap();
    assert_eq!(eval_vol(&s_fts, vol_fts, 0), vec![56.0; 12]);
}

#[derive(Debug)]
struct CountingSampler(Arc<AtomicUsize>);
impl ContextSampler for CountingSampler {
    fn sample_value(&self, bx: i32, by: i32, bz: i32) -> f32 {
        self.0.fetch_add(1, Ordering::Relaxed);
        (bx + by + bz) as f32
    }
}

#[test]
fn cache_deduplication_and_volume_to_point_cache_hit() {
    // Two identical `cache(beardifier)` subtrees in an `add` share ONE cache cell.
    let json = r#"{
        "type": "minecraft:add",
        "left": {"type": "minecraft:cache", "input": {"type": "minecraft:beardifier"}},
        "right": {"type": "minecraft:cache", "input": {"type": "minecraft:beardifier"}}
    }"#;
    let (_, s, cache_count) = compile_json(json, 0);
    assert_eq!(cache_count, 1);

    let counter = Arc::new(AtomicUsize::new(0));
    let mut ctx = SamplerContext::caching(cache_count)
        .with_beardifier(Arc::new(CountingSampler(Arc::clone(&counter))));
    let vol = DensityVolume::unit(2, 2, 2, 0, 0, 0).unwrap();
    let mut out = vec![0.0_f32; vol.len()];
    s.sample_volume(&mut ctx, &mut out, vol);
    // Underlying beardifier evaluated only ONCE for the 8 voxels (not twice)!
    assert_eq!(counter.load(Ordering::Relaxed), 8);
    assert_eq!(out[vol.index(1, 1, 1)], 6.0); // (1+1+1) + (1+1+1)

    // Subsequent point query inside cached volume hits cached buffer with 0 new calls!
    let p = s.sample_value(&mut ctx, 1, 1, 1);
    assert_eq!(p, 6.0);
    assert_eq!(counter.load(Ordering::Relaxed), 8);

    // Point query outside cached volume evaluates once (shared across left & right via valueKey cache)
    let p_out = s.sample_value(&mut ctx, 5, 5, 5);
    assert_eq!(p_out, 30.0);
    assert_eq!(counter.load(Ordering::Relaxed), 9);
}

// ===========================================================================
// 7. JSON decode & resolution error tests
// ===========================================================================

#[test]
fn decode_and_validation_errors() {
    let bad_inputs = [
        // Out of NOISE_VALUE_CODEC range
        "1000001.0",
        r#"{"type": "minecraft:constant", "value": -1000001.0}"#,
        // No string->number coercion
        r#"{"type": "minecraft:constant", "value": "5"}"#,
        // Missing required key
        r#"{"type": "minecraft:add", "left": 1.0}"#,
        // Unknown type
        r#"{"type": "minecraft:nonexistent_type"}"#,
        // Gradient equal coordinates
        r#"{"type": "minecraft:gradient", "axis": "y", "from_coordinate": 5, "to_coordinate": 5, "from_value": 0.0, "to_value": 1.0}"#,
        // Clamp max < min
        r#"{"type": "minecraft:clamp", "input": 0.0, "min": 2.0, "max": 1.0}"#,
        // IntervalSelect wrong threshold length
        r#"{"type": "minecraft:interval_select", "input": 0.0, "thresholds": [0.0, 1.0], "functions": [1.0, 2.0]}"#,
        // IntervalSelect decreasing thresholds
        r#"{"type": "minecraft:interval_select", "input": 0.0, "thresholds": [2.0, 1.0], "functions": [1.0, 2.0, 3.0]}"#,
        // IntervalSelect fewer than 2 functions
        r#"{"type": "minecraft:interval_select", "input": 0.0, "thresholds": [], "functions": [1.0]}"#,
        // Spline empty points
        r#"{"type": "minecraft:spline", "spline": {"coordinate": 0.0, "points": []}}"#,
        // Interpolated non-positive cell size
        r#"{"type": "minecraft:interpolated", "input": 0.0, "cell_size_xz": 0, "cell_size_y": 4}"#,
        // FindTopSurface lower_bound out of range
        r#"{"type": "minecraft:find_top_surface", "density": 0.0, "upper_bound": 0.0, "lower_bound": -5000, "cell_height": 4}"#,
        // FindTopSurface non-positive cell_height
        r#"{"type": "minecraft:find_top_surface", "density": 0.0, "upper_bound": 0.0, "lower_bound": 0, "cell_height": 0}"#,
        // OldBlendedNoise out of range
        r#"{"type": "minecraft:old_blended_noise", "xz_scale": 0.0, "y_scale": 1.0, "xz_factor": 1.0, "y_factor": 1.0, "smear_scale_multiplier": 1.0}"#,
        // DistanceToPoint wrong point length
        r#"{"type": "minecraft:distance_to_point", "point": [1, 2], "metric": "euclidean"}"#,
    ];

    for bad in bad_inputs {
        assert!(
            decode_density_function_str(bad).is_err(),
            "expected decode error for: {bad}"
        );
    }

    // Inline unregistered NormalNoise decodes cleanly but fails in RandomState::compile_function
    let inline_noise_df = decode_density_function_str(
        r#"{
            "type": "minecraft:noise",
            "noise": {"base_octave": -4, "octave_count": 2},
            "xz_scale": 1.0,
            "y_scale": 1.0
        }"#,
    )
    .expect("inline NormalNoise decodes");
    assert_eq!(
        RandomState::compile_function(&inline_noise_df, 0, false).unwrap_err(),
        CompileError::UnregisteredInlineNoise
    );
}

#[test]
fn pack_loading_references_cycles_and_noise_settings_validation() {
    // Valid pack with cross-referencing density functions and noise settings
    let pack = MemoryPack::new("test", PathRules::Zip, null_log())
        .with_file(
            "data/minecraft/worldgen/noise/base.json",
            r#"{"base_octave": -4, "octave_count": 2, "normalize": true}"#,
        )
        .with_file(
            "data/minecraft/worldgen/density_function/a.json",
            r#"{"type": "minecraft:add", "left": "minecraft:b", "right": 1.0}"#,
        )
        .with_file(
            "data/minecraft/worldgen/density_function/b.json",
            r#"{"type": "minecraft:noise", "noise": "minecraft:base", "xz_scale": 0.25, "y_scale": 0.25}"#,
        )
        .with_file(
            "data/minecraft/worldgen/noise_settings/custom.json",
            r#"{
                "noise": {"min_y": -64, "height": 128},
                "default_block": {"id": "minecraft:stone"},
                "default_fluid": {"properties": {"level": "0"}, "id": "minecraft:water"},
                "noise_router": {
                    "temperature": "minecraft:a",
                    "vegetation": 0.1,
                    "continents": 0.2,
                    "erosion": 0.3,
                    "depth": 0.4,
                    "ridges": 0.5,
                    "chunk_surface_level": 64.0,
                    "final_density": "minecraft:a"
                },
                "material_rule": "minecraft:overworld",
                "spawn_target": [{"minecraft:a": [-1.0, 1.0]}],
                "sea_level": 63,
                "disable_mob_generation": false,
                "legacy_random_source": false
            }"#,
        );

    let data = WorldgenData::load(&pack).expect("pack loads cleanly");
    let custom_id = Identifier::parse("minecraft:custom").unwrap();
    let rs = RandomState::new(&data, &custom_id, 98765).expect("RandomState initializes");
    assert_eq!(rs.settings().min_y(), -64);
    assert_eq!(rs.settings().height(), 128);
    assert_eq!(rs.settings().sea_level(), 63);
    assert_eq!(
        rs.settings().default_block_id().to_string(),
        "minecraft:stone"
    );
    assert_eq!(
        rs.settings().default_fluid_id().to_string(),
        "minecraft:water"
    );
    assert!(rs.settings().aquifers().is_none());
    assert_eq!(rs.sample_aquifer(0, 0, 0, 0), None);

    let cp = rs.climate_point(10, 20, 30);
    assert_eq!(
        cp[0],
        rs.sample_router(RouterField::Temperature, 10, 20, 30)
    );
    assert_eq!(cp[1..6], [0.1, 0.2, 0.3, 0.4, 0.5]);

    // Cycle detection
    let cyclic_pack = MemoryPack::new("cyclic", PathRules::Zip, null_log())
        .with_file(
            "data/minecraft/worldgen/density_function/x.json",
            r#""minecraft:y""#,
        )
        .with_file(
            "data/minecraft/worldgen/density_function/y.json",
            r#""minecraft:x""#,
        );
    assert!(matches!(
        WorldgenData::load(&cyclic_pack),
        Err(LoadError::DensityFunctionCycle { .. })
    ));

    // Unknown reference
    let missing_ref_pack = MemoryPack::new("missing", PathRules::Zip, null_log()).with_file(
        "data/minecraft/worldgen/density_function/x.json",
        r#""minecraft:does_not_exist""#,
    );
    assert!(matches!(
        WorldgenData::load(&missing_ref_pack),
        Err(LoadError::UnknownDensityFunction { .. })
    ));

    // Invalid NoiseSettings (height not multiple of 16)
    let bad_ns_pack = MemoryPack::new("bad_ns", PathRules::Zip, null_log()).with_file(
        "data/minecraft/worldgen/noise_settings/bad.json",
        r#"{
            "noise": {"min_y": 0, "height": 100},
            "default_block": {"id": "minecraft:stone"},
            "default_fluid": "minecraft:water",
            "noise_router": {
                "temperature": 0, "vegetation": 0, "continents": 0, "erosion": 0,
                "depth": 0, "ridges": 0, "chunk_surface_level": 0, "final_density": 0
            },
            "material_rule": "minecraft:overworld",
            "spawn_target": [],
            "sea_level": 63,
            "disable_mob_generation": false,
            "legacy_random_source": false
        }"#,
    );
    assert!(matches!(
        WorldgenData::load(&bad_ns_pack),
        Err(LoadError::Decode { .. })
    ));

    // Invalid NormalNoiseParameters (amplitude_modifiers length mismatch & invalid normalize)
    let bad_noise_len = MemoryPack::new("bad_n1", PathRules::Zip, null_log()).with_file(
        "data/minecraft/worldgen/noise/bad.json",
        r#"{"base_octave": -4, "octave_count": 3, "amplitude_modifiers": [1.0, 2.0]}"#,
    );
    assert!(matches!(
        WorldgenData::load(&bad_noise_len),
        Err(LoadError::Decode { .. })
    ));

    let bad_noise_norm = MemoryPack::new("bad_n2", PathRules::Zip, null_log()).with_file(
        "data/minecraft/worldgen/noise/bad.json",
        r#"{"base_octave": -4, "octave_count": 1, "normalize": "unknown_mode"}"#,
    );
    assert!(matches!(
        WorldgenData::load(&bad_noise_norm),
        Err(LoadError::Decode { .. })
    ));
}

/// Registry spec §3.3: bare id or `{"id", "properties"}`; the pre-5006 `{"Name", "Properties"}` form is rejected.
#[test]
fn block_state_forms_follow_data_version_5006() {
    let settings = |block: &str| {
        format!(
            r#"{{"noise": {{"min_y": 0, "height": 64}}, "default_block": {block}, "default_fluid": "minecraft:water",
                "noise_router": {{"temperature": 0, "vegetation": 0, "continents": 0, "erosion": 0, "depth": 0,
                "ridges": 0, "chunk_surface_level": 0, "final_density": 0}}, "material_rule": "minecraft:overworld",
                "spawn_target": [], "sea_level": 0, "disable_mob_generation": false, "legacy_random_source": false}}"#
        )
    };
    let load = |block: &str| {
        let pack = zenith_codec::resources::MemoryPack::new(
            "test",
            zenith_codec::resources::PathRules::Zip,
            zenith_codec::log::null_log(),
        )
        .with_file(
            "data/minecraft/worldgen/noise_settings/s.json",
            settings(block),
        );
        WorldgenData::load(&pack)
    };
    assert!(load(r#""minecraft:stone""#).is_ok());
    assert!(load(r#"{"id": "minecraft:stone"}"#).is_ok());
    assert!(
        load(r#"{"id": "minecraft:furnace", "properties": {"lit": "true", "facing": 3}}"#).is_ok()
    );
    assert!(
        load(r#"{"id": "minecraft:furnace", "properties": 5}"#).is_ok(),
        "properties is lenient"
    );
    assert!(
        load(r#"{"Name": "minecraft:stone"}"#).is_err(),
        "pre-5006 form is rejected"
    );
}
