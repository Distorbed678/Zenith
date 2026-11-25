//! Property, round-trip, and functional tests for `zenith_math::helpers`.

use zenith_math::helpers::angles::{
    Quaternionf, SegmentedAnglePrecision, approach, approach_degrees, degrees_difference,
    degrees_difference_abs, pack_degrees, rot_lerp_f32, rot_lerp_f64, rotate_if_necessary,
    rotation_around_axis, unpack_degrees, wrap_degrees_90, wrap_degrees_f32, wrap_degrees_f64,
    wrap_degrees_i32, wrap_degrees_i64,
};
use zenith_math::helpers::color::{
    FULL_BRIGHT, FULL_SKY, add_rgb, alpha, alpha_blend, average, blue, color_argb, color_rgb,
    from_abgr, green, linear_lerp, mean_linear, multiply, opaque, pack_light, red, scale_rgb_i32,
    smooth_blend_light, smooth_pack_light, smooth_weighted_blend_light, srgb_lerp,
    srgb_to_linear_channel, subtract_rgb, to_abgr, transparent, unpack_block_light,
    unpack_sky_light, unpack_smooth_block_light, unpack_smooth_sky_light, with_block_light,
};
use zenith_math::helpers::geom::{
    Aabb, AabbBuilder, Axis, Direction, LP_VEC3_ABS_MAX_VALUE, Vec2, Vec3, Vec3i, lp_vec3_decode,
    lp_vec3_encode,
};
use zenith_math::helpers::hash::{
    chunk_pos_hash, create_insecure_uuid, get_seed, murmur_hash3_mixer, next_double, next_float,
    normal, random_between, vec3i_hash_code, wobble,
};
use zenith_math::helpers::interp::{
    BinaryAnimator, CubicBezier, CubicSpline, Ease, EasingType, IdentityCoordinate, SmoothDouble,
    SplineError, catmullrom, clamped_map_f32, clamped_map_f64, inverse_lerp_f32, lerp_discrete,
    lerp_int, lerp_vec3, lerp2_f32, lerp3_f64, map_f32, map_f64, smoothstep, smoothstep_derivative,
    triangle_wave,
};
use zenith_math::helpers::packing::{
    MAX_HORIZONTAL_COORDINATE, MAX_PACKED_BLOCK_Y, MAX_PACKED_SECTION_XZ, MAX_PACKED_SECTION_Y,
    MIN_HORIZONTAL_COORDINATE, MIN_PACKED_BLOCK_Y, MIN_PACKED_SECTION_XZ, MIN_PACKED_SECTION_Y,
    block_as_long, block_get_flat_index, block_get_x, block_get_y, block_get_z,
    block_to_section_coord, chunk_get_x, chunk_get_z, chunk_pack, chunk_region_local,
    chunk_to_region, quart_from_block, quart_from_section, quart_local, quart_to_block,
    quart_to_section, region_to_min_chunk, section_as_long, section_get_x, section_get_y,
    section_get_z, section_get_zero_node, section_relative, section_relative_pos,
    section_relative_x, section_relative_y, section_relative_z, section_to_block_coord,
};
use zenith_math::helpers::roots::{inv_sqrt_f64, sqrt};
use zenith_math::helpers::rounding::{
    Divisor, abs_i32, binary_search, ceil_f32, ceil_f64, ceil_long, chessboard_distance, equal_f32,
    floor_div, floor_f32, floor_f64, is_multiple_of, java_clamp_f64, java_floor_div_i32,
    java_floor_mod_i32, java_round_f32, java_round_f64, lfloor, mul_and_truncate, out_from_origin,
    out_from_origin_step, positive_modulo_f32, positive_modulo_f64, positive_modulo_i32,
    round_toward_i32, round_toward_i64, smallest_square_side,
};
use zenith_math::helpers::trig::{atan2, cos, sin};
use zenith_math::{Interval, LegacyRandom, XoroshiroRandom};

#[test]
fn packing_round_trips_at_signed_range_limits() {
    // BlockPos extremes and bijection within valid signed range.
    let xs = [
        MIN_HORIZONTAL_COORDINATE,
        MIN_HORIZONTAL_COORDINATE + 1,
        -1,
        0,
        1,
        12_345_678,
        MAX_HORIZONTAL_COORDINATE - 1,
        MAX_HORIZONTAL_COORDINATE,
    ];
    let ys = [
        MIN_PACKED_BLOCK_Y,
        MIN_PACKED_BLOCK_Y + 1,
        -64,
        -1,
        0,
        64,
        319,
        MAX_PACKED_BLOCK_Y - 1,
        MAX_PACKED_BLOCK_Y,
    ];
    for &x in &xs {
        for &y in &ys {
            for &z in &xs {
                let p = block_as_long(x, y, z);
                assert_eq!((block_get_x(p), block_get_y(p), block_get_z(p)), (x, y, z));
                // get_flat_index clears only the low 4 bits of Y.
                let flat = block_get_flat_index(p);
                assert_eq!(block_get_x(flat), x);
                assert_eq!(block_get_z(flat), z);
                assert_eq!(block_get_y(flat), y & !15);
            }
        }
    }

    // SectionPos extremes and zero-node check.
    let sxs = [
        MIN_PACKED_SECTION_XZ,
        -1,
        0,
        1,
        123_456,
        MAX_PACKED_SECTION_XZ,
    ];
    let sys = [MIN_PACKED_SECTION_Y, -4, -1, 0, 1, 20, MAX_PACKED_SECTION_Y];
    for &x in &sxs {
        for &y in &sys {
            for &z in &sxs {
                let p = section_as_long(x, y, z);
                assert_eq!(
                    (section_get_x(p), section_get_y(p), section_get_z(p)),
                    (x, y, z)
                );
                let znode = section_get_zero_node(p);
                assert_eq!(section_get_x(znode), x);
                assert_eq!(section_get_y(znode), 0);
                assert_eq!(section_get_z(znode), z);
            }
        }
    }

    // 12-bit section_relative_pos bijection on [0, 15]^3.
    let mut seen = [false; 4096];
    for x in 0..16 {
        for y in 0..16 {
            for z in 0..16 {
                let r = section_relative_pos(x, y, z);
                assert!(!seen[r as usize]);
                seen[r as usize] = true;
                assert_eq!(section_relative_x(r), x);
                assert_eq!(section_relative_y(r), y);
                assert_eq!(section_relative_z(r), z);
            }
        }
    }
    assert!(seen.iter().all(|&s| s));

    // ChunkPos extremes (full 32-bit signed range).
    for x in [i32::MIN, -1_875_066, -1, 0, 1, 1_875_066, i32::MAX] {
        for z in [i32::MIN, -1_875_066, -1, 0, 1, 1_875_066, i32::MAX] {
            let p = chunk_pack(x, z);
            assert_eq!((chunk_get_x(p), chunk_get_z(p)), (x, z));
        }
    }

    // Block <-> section, chunk <-> region, and QuartPos identities across negatives.
    for b in -100..=100 {
        let s = block_to_section_coord(b);
        let rel = section_relative(b);
        assert!((0..16).contains(&rel));
        assert_eq!(section_to_block_coord(s) + rel, b);

        let q = quart_from_block(b);
        let ql = quart_local(b);
        assert!((0..4).contains(&ql));
        assert_eq!(quart_to_block(q) + ql, b);
        assert_eq!(quart_to_section(q), s);
    }
    for s in -20..=20 {
        assert_eq!(quart_to_section(quart_from_section(s)), s);
    }
    for c in -100..=100 {
        let r = chunk_to_region(c);
        let l = chunk_region_local(c);
        assert!((0..32).contains(&l));
        assert_eq!(region_to_min_chunk(r) + l, c);
    }
}

#[test]
fn wrap_degrees_range_and_idempotence() {
    for a in -2000..=2000 {
        let wi = wrap_degrees_i32(a);
        assert!((-180..180).contains(&wi));
        assert_eq!(wrap_degrees_i32(wi), wi);

        let wl = wrap_degrees_i64(i64::from(a) * 1000 + 180);
        assert!((-180.0_f32..180.0_f32).contains(&wl));

        let wf = wrap_degrees_f32(a as f32 * 0.37_f32);
        assert!((-180.0_f32..180.0_f32).contains(&wf));
        assert_eq!(wrap_degrees_f32(wf), wf);

        let wd = wrap_degrees_f64(f64::from(a) * 0.37_f64);
        assert!((-180.0_f64..180.0_f64).contains(&wd));
        assert_eq!(wrap_degrees_f64(wd), wd);

        let w90 = wrap_degrees_90(a as f32 * 0.37_f32);
        assert!((-45.0_f32..45.0_f32).contains(&w90));
        assert_eq!(wrap_degrees_90(w90), w90);
    }
}

#[test]
fn angles_helpers_pack_unpack_and_segmented_precision() {
    assert_eq!(degrees_difference(170.0, -170.0), 20.0);
    assert_eq!(degrees_difference_abs(170.0, -170.0), 20.0);
    assert_eq!(rotate_if_necessary(0.0, 50.0, 10.0), 40.0);
    assert_eq!(approach(10.0, 20.0, 3.0), 13.0);
    assert_eq!(approach(10.0, 20.0, 50.0), 20.0);
    assert_eq!(approach_degrees(175.0, -175.0, 4.0), 179.0);
    assert_eq!(rot_lerp_f32(0.5, 170.0, -170.0), 180.0);
    assert_eq!(rot_lerp_f64(0.5, 170.0, -170.0), 180.0);

    // Byte-packed degrees exact round-trip for all 256 signed bytes.
    assert_eq!(unpack_degrees(-128), -180.0_f32);
    assert_eq!(unpack_degrees(127), 178.59375_f32);
    for b in i8::MIN..=i8::MAX {
        let deg = unpack_degrees(b);
        assert_eq!(pack_degrees(deg), b);
    }

    // SegmentedAnglePrecision bounds and operations.
    assert!(SegmentedAnglePrecision::try_new(1).is_none());
    assert!(SegmentedAnglePrecision::try_new(31).is_none());
    let prec = SegmentedAnglePrecision::new(4);
    assert_eq!(prec.mask(), 15);
    assert_eq!(prec.from_degrees(0.0), 0);
    assert_eq!(prec.from_degrees(90.0), 4);
    assert_eq!(prec.from_degrees(180.0), 8);
    assert_eq!(prec.to_degrees(8), -180.0);
    assert_eq!(prec.to_degrees(4), 90.0);
    assert!(prec.is_same_axis(2, 10));
    assert!(!prec.is_same_axis(2, 3));
    assert_eq!(prec.from_direction(Direction::Down), 0);
    assert_eq!(prec.from_direction(Direction::Up), 0);
    assert_eq!(prec.from_direction(Direction::South), 0);
    assert_eq!(prec.from_direction(Direction::West), 4);
    assert_eq!(prec.from_direction(Direction::North), 8);
    assert_eq!(prec.from_direction(Direction::East), 12);

    // Quaternion twist projection preserves unit norm.
    let q = Quaternionf::new(0.2, 0.5, -0.3, 0.78);
    let twist = rotation_around_axis([0.0, 1.0, 0.0], q);
    let norm =
        (twist.x * twist.x + twist.y * twist.y + twist.z * twist.z + twist.w * twist.w).sqrt();
    assert!((norm - 1.0).abs() < 1.0e-5);
    assert_eq!(twist.x, 0.0);
    assert_eq!(twist.z, 0.0);
}

#[test]
fn trig_and_roots_consistency() {
    for k in -16..=16 {
        let angle = (k as f64) * 0.4;
        let s = f64::from(sin(angle));
        let c = f64::from(cos(angle));
        assert!((s - angle.sin()).abs() < 2.0e-4);
        assert!((c - angle.cos()).abs() < 2.0e-4);
    }
    for (y, x) in [
        (1.0, 1.0),
        (1.0, -1.0),
        (-1.0, 1.0),
        (-1.0, -1.0),
        (3.0, 4.0),
    ] {
        let approx = atan2(y, x);
        let exact = y.atan2(x);
        assert!((approx - exact).abs() < 1.0e-5);
    }
    assert_eq!(sqrt(9.0), 3.0);
    assert_eq!(inv_sqrt_f64(4.0), 0.5);
}

#[test]
fn rounding_java_round_divisor_out_from_origin_and_binary_search() {
    // Java Math.round tricky edge cases (JDK-6430675 and ties toward +inf).
    assert_eq!(java_round_f32(f32::from_bits(0x3EFF_FFFF)), 0);
    assert_eq!(java_round_f32(0.5), 1);
    assert_eq!(java_round_f32(-0.5), 0);
    assert_eq!(java_round_f32(-0.500_000_06), -1);
    assert_eq!(java_round_f32(8_388_609.0), 8_388_609);
    assert_eq!(java_round_f32(f32::NAN), 0);
    assert_eq!(java_round_f32(f32::INFINITY), i32::MAX);
    assert_eq!(java_round_f32(f32::NEG_INFINITY), i32::MIN);

    assert_eq!(java_round_f64(f64::from_bits(0x3FDF_FFFF_FFFF_FFFF)), 0);
    assert_eq!(java_round_f64(0.5), 1);
    assert_eq!(java_round_f64(-0.5), 0);
    assert_eq!(java_round_f64((-0.5_f64).next_down()), -1);
    assert_eq!(
        java_round_f64(4_503_599_627_370_497.0),
        4_503_599_627_370_497
    );
    assert_eq!(java_round_f64(f64::NAN), 0);
    assert_eq!(java_round_f64(f64::INFINITY), i64::MAX);
    assert_eq!(java_round_f64(f64::NEG_INFINITY), i64::MIN);

    // Java floorDiv / floorMod / clamp / positiveModulo.
    assert_eq!(java_floor_div_i32(i32::MIN, -1), i32::MIN);
    assert_eq!(java_floor_mod_i32(i32::MIN, -1), 0);
    assert_eq!(floor_div(-1, 4), -1);
    assert_eq!(positive_modulo_i32(-1, 4), 3);
    assert_eq!(positive_modulo_f32(-0.25, 1.0), 0.75);
    assert_eq!(positive_modulo_f64(-0.25, 1.0), 0.75);
    assert!(is_multiple_of(i32::MIN, -1));
    assert_eq!(round_toward_i32(10, 4), 12);
    assert_eq!(round_toward_i64(10, 4), 12);
    assert_eq!(mul_and_truncate(3, 4, 10), 7);
    assert_eq!(abs_i32(i32::MIN), i32::MIN);
    assert_eq!(chessboard_distance(1, 2, -3, 10), 8);
    assert!(equal_f32(1.0, 1.0 + 1.0e-6));
    assert_eq!(floor_f32(-1.1), -2);
    assert_eq!(floor_f64(-1.1), -2);
    assert_eq!(lfloor(-1.1), -2);
    assert_eq!(ceil_f32(1.1), 2);
    assert_eq!(ceil_f64(1.1), 2);
    assert_eq!(ceil_long(1.1), 2);
    assert_eq!(smallest_square_side(0), 0);
    assert_eq!(smallest_square_side(10), 4);
    assert_eq!(
        java_clamp_f64(-0.0, -0.0, 0.0).to_bits(),
        (-0.0_f64).to_bits()
    );

    // Divisor
    assert_eq!(Divisor::new(10, 0).collect::<Vec<_>>(), Vec::<i32>::new());
    assert_eq!(Divisor::new(10, -2).collect::<Vec<_>>(), Vec::<i32>::new());
    let parts: Vec<i32> = Divisor::new(10, 3).collect();
    assert_eq!(parts, vec![3, 3, 4]);
    assert_eq!(parts.iter().sum::<i32>(), 10);
    // Negative numerator never increments because remainder m <= 0.
    assert_eq!(Divisor::new(-10, 3).collect::<Vec<_>>(), vec![-3, -3, -3]);

    // binary_search
    let data = [10, 20, 30, 40, 50];
    assert_eq!(binary_search(0, 5, |i| data[i as usize] >= 30), 2);
    assert_eq!(binary_search(0, 5, |i| data[i as usize] >= 99), 5);
    assert_eq!(binary_search(0, 5, |i| data[i as usize] >= 0), 0);

    // OutFromOrigin
    assert_eq!(
        out_from_origin(5, 3, 7).collect::<Vec<_>>(),
        vec![5, 6, 4, 7, 3]
    );
    assert_eq!(out_from_origin(5, 5, 5).collect::<Vec<_>>(), vec![5]);
    assert_eq!(out_from_origin(0, 3, 5).collect::<Vec<_>>(), vec![3, 4, 5]);
    assert_eq!(out_from_origin(9, 3, 5).collect::<Vec<_>>(), vec![5, 4, 3]);
    assert_eq!(
        out_from_origin_step(5, 2, 7, 2).collect::<Vec<_>>(),
        vec![5, 7, 3]
    );
    assert_eq!(
        out_from_origin_step(5, 0, 6, 2).collect::<Vec<_>>(),
        vec![5, 3, 1]
    );
}

#[test]
fn interp_easing_spline_and_animators() {
    // All 32 named easing curves parse by ID and produce finite values on [0, 1].
    assert_eq!(Ease::ALL.len(), 32);
    for ease in Ease::ALL {
        assert_eq!(Ease::from_id(ease.id()), Some(ease));
        let y0 = ease.apply(0.0);
        let y_mid = ease.apply(0.5);
        let y1 = ease.apply(1.0);
        assert!(y0.is_finite() && y_mid.is_finite() && y1.is_finite());
        if ease == Ease::Constant {
            assert_eq!(y0, 0.0);
            assert_eq!(y1, 0.0);
        } else {
            assert!(y0.abs() < 1.0e-4, "{} at 0 = {y0}", ease.id());
            assert!((y1 - 1.0).abs() < 1.0e-4, "{} at 1 = {y1}", ease.id());
        }
    }

    // Basic interpolation helpers
    assert_eq!(lerp2_f32(0.5, 0.5, 0.0, 2.0, 4.0, 6.0), 3.0);
    assert_eq!(
        lerp3_f64(0.5, 0.5, 0.5, 0.0, 1.0, 1.0, 2.0, 1.0, 2.0, 2.0, 3.0),
        1.5
    );
    assert_eq!(
        lerp_vec3(0.5, Vec3::ZERO, Vec3::new(2.0, 4.0, 6.0)),
        Vec3::new(1.0, 2.0, 3.0)
    );
    assert_eq!(lerp_int(0.5, 0, 10), 5);
    assert_eq!(lerp_discrete(0.0, 0, 10), 0);
    assert_eq!(lerp_discrete(0.01, 0, 10), 1);
    assert_eq!(inverse_lerp_f32(5.0, 0.0, 10.0), 0.5);
    assert_eq!(map_f32(5.0, 0.0, 10.0, 100.0, 200.0), 150.0);
    assert_eq!(map_f64(5.0, 0.0, 10.0, 100.0, 200.0), 150.0);
    assert_eq!(clamped_map_f32(15.0, 0.0, 10.0, 100.0, 200.0), 200.0);
    assert_eq!(clamped_map_f64(-5.0, 0.0, 10.0, 100.0, 200.0), 100.0);
    assert_eq!(smoothstep(0.0), 0.0);
    assert_eq!(smoothstep(1.0), 1.0);
    assert_eq!(smoothstep_derivative(0.0), 0.0);
    assert_eq!(smoothstep_derivative(1.0), 0.0);
    assert_eq!(catmullrom(0.0, 0.0, 1.0, 2.0, 3.0), 1.0);
    assert_eq!(catmullrom(1.0, 0.0, 1.0, 2.0, 3.0), 2.0);
    assert_eq!(triangle_wave(0.0, 4.0), 1.0);
    assert_eq!(triangle_wave(2.0, 4.0), -1.0);

    // CubicBezier symmetric
    let bez = CubicBezier::symmetric(0.25, 0.1);
    assert!((bez.apply(0.0) - 0.0).abs() < 1.0e-5);
    assert!((bez.apply(0.5) - 0.5).abs() < 1.0e-4);
    assert!((bez.apply(1.0) - 1.0).abs() < 1.0e-5);

    // CubicSpline validation, sampling, linear extension, and conservative interval range.
    let coord = IdentityCoordinate::new(Interval::new(-1.0, 2.0));
    assert_eq!(
        CubicSpline::multipoint(coord, vec![], vec![], vec![]).unwrap_err(),
        SplineError::EmptyPoints
    );
    assert_eq!(
        CubicSpline::multipoint(
            coord,
            vec![1.0, 1.0],
            vec![CubicSpline::constant(0.0), CubicSpline::constant(1.0)],
            vec![0.0, 0.0],
        )
        .unwrap_err(),
        SplineError::NonAscendingLocations
    );
    let spline = CubicSpline::multipoint(
        coord,
        vec![0.0, 1.0],
        vec![CubicSpline::constant(0.0), CubicSpline::constant(1.0)],
        vec![2.0, -2.0],
    )
    .unwrap();
    let range = spline.range();
    assert!(!range.is_nai());
    for i in -100..=200 {
        let u = (i as f32) * 0.01;
        let v = spline.sample(&u);
        assert!(
            v >= range.min() - 1.0e-5 && v <= range.max() + 1.0e-5,
            "sample({u}) = {v} outside range [{}, {}]",
            range.min(),
            range.max()
        );
    }

    // NaI coordinate propagates to NaI spline range.
    let nai_spline = CubicSpline::multipoint(
        IdentityCoordinate::new(Interval::NaI),
        vec![0.0],
        vec![CubicSpline::constant(1.0)],
        vec![0.0],
    )
    .unwrap();
    assert!(nai_spline.range().is_nai());

    // SmoothDouble
    let mut sd = SmoothDouble::new();
    let step1 = sd.get_new_delta(10.0, 0.5);
    let step2 = sd.get_new_delta(0.0, 0.5);
    assert!(step1 > 0.0 && step2 > 0.0);
    sd.reset();
    assert_eq!(sd.get_new_delta(0.0, 0.5), 0.0);

    // BinaryAnimator
    let mut anim = BinaryAnimator::new(4, EasingType::from(Ease::Linear));
    assert_eq!(anim.get_factor(0.0), 0.0);
    anim.tick(true);
    assert_eq!(anim.ticks_old(), 0);
    assert_eq!(anim.ticks(), 1);
    assert_eq!(anim.get_factor(0.5), 0.125);
    anim.tick(false);
    assert_eq!(anim.ticks(), 0);
}

#[test]
fn hash_and_rng_helpers() {
    assert_eq!(murmur_hash3_mixer(0), 0);
    assert_ne!(murmur_hash3_mixer(1), 0);
    assert_ne!(chunk_pos_hash(0, 0), chunk_pos_hash(0, 1));
    assert_eq!(vec3i_hash_code(1, 2, 3), ((2 + 3 * 31) * 31) + 1);
    assert_eq!(
        get_seed(1, 2, 3),
        zenith_math::random::position_hash(1, 2, 3)
    );

    let mut rng = XoroshiroRandom::new(12345);
    assert_eq!(next_float(&mut rng, 5.0, 2.0), 5.0);
    assert_eq!(next_double(&mut rng, 5.0, 2.0), 5.0);
    let rf = random_between(&mut rng, 1.0, 2.0);
    assert!((1.0..2.0).contains(&rf));
    let n = normal(&mut rng, 10.0, 2.0);
    assert!(n.is_finite());

    let w = wobble(12.5);
    assert!((w - 12.5).abs() <= 1.0e-7);

    // UUID version 4 and RFC 4122 variant bits.
    let uuid = create_insecure_uuid(&mut rng);
    assert_eq!((uuid.most >> 12) & 0xF, 4);
    assert_eq!(((uuid.least as u64) >> 62) & 0x3, 2);
    assert_eq!(uuid.to_be_bytes().len(), 16);
}

#[test]
fn color_and_light_helpers() {
    let c = color_argb(128, 10, 20, 30);
    assert_eq!((alpha(c), red(c), green(c), blue(c)), (128, 10, 20, 30));
    assert_eq!(from_abgr(to_abgr(c)), c);
    assert_eq!(alpha(opaque(c)), 255);
    assert_eq!(alpha(transparent(c)), 0);
    assert_eq!(multiply(-1, c), c);
    assert_eq!(multiply(c, -1), c);
    assert_eq!(
        add_rgb(color_argb(100, 200, 10, 5), color_rgb(100, 20, 30)),
        color_argb(100, 255, 30, 35)
    );
    assert_eq!(
        subtract_rgb(color_argb(100, 50, 10, 5), color_rgb(100, 5, 30)),
        color_argb(100, 0, 5, 0)
    );
    assert_eq!(
        scale_rgb_i32(color_argb(77, 100, 200, 50), 255),
        color_argb(77, 100, 200, 50)
    );
    assert_eq!(
        average(color_argb(100, 20, 40, 60), color_argb(200, 40, 60, 80)),
        color_argb(150, 30, 50, 70)
    );
    assert_eq!(
        alpha_blend(c, opaque(color_rgb(1, 2, 3))),
        opaque(color_rgb(1, 2, 3))
    );
    assert_eq!(alpha_blend(c, transparent(color_rgb(1, 2, 3))), c);

    // sRGB <-> linear helpers
    assert_eq!(srgb_to_linear_channel(0), 0.0);
    assert_eq!(srgb_to_linear_channel(255), 1.0);
    assert_eq!(mean_linear(c, c, c, c), c);
    assert_eq!(linear_lerp(0.0, c, color_argb(255, 255, 255, 255)), c);
    assert_eq!(srgb_lerp(0.0, c, color_argb(255, 255, 255, 255)), c);

    // Light packing
    assert_eq!(pack_light(15, 15), FULL_BRIGHT);
    assert_eq!(pack_light(0, 15), FULL_SKY);
    let lp = pack_light(7, 13);
    assert_eq!(unpack_block_light(lp), 7);
    assert_eq!(unpack_sky_light(lp), 13);
    assert_eq!(unpack_block_light(with_block_light(lp, 11)), 11);
    assert_eq!(unpack_sky_light(with_block_light(lp, 11)), 13);

    let sp = smooth_pack_light(120, 240);
    assert_eq!(unpack_smooth_block_light(sp), 120);
    assert_eq!(unpack_smooth_sky_light(sp), 240);
    assert_eq!(smooth_blend_light(0, 0, 0, FULL_BRIGHT), FULL_BRIGHT);
    assert_eq!(
        smooth_weighted_blend_light(sp, sp, sp, sp, 0.25, 0.25, 0.25, 0.25),
        sp
    );
}

#[test]
fn lp_vec3_and_geom_operations() {
    // Near-zero encodes to 1 byte [0] and decodes to ZERO.
    let zero_buf = lp_vec3_encode(Vec3::new(1.0e-8, -1.0e-8, 0.0));
    assert_eq!(zero_buf, vec![0]);
    assert_eq!(lp_vec3_decode(&zero_buf).unwrap(), (Vec3::ZERO, 1));

    // Scale <= 3 encodes to 6 bytes (no VarInt).
    let small = Vec3::new(1.5, -2.25, 0.75);
    let small_buf = lp_vec3_encode(small);
    assert_eq!(small_buf.len(), 6);
    let (decoded_small, len_small) = lp_vec3_decode(&small_buf).unwrap();
    assert_eq!(len_small, 6);
    assert!((decoded_small.x - small.x).abs() < 5.0e-4);
    assert!((decoded_small.y - small.y).abs() < 5.0e-4);
    assert!((decoded_small.z - small.z).abs() < 5.0e-4);

    // Large scale uses VarInt and round-trips near max range.
    let large = Vec3::new(1000.0, -500.0, 250.0);
    let large_buf = lp_vec3_encode(large);
    assert!(large_buf.len() > 6);
    let (decoded_large, len_large) = lp_vec3_decode(&large_buf).unwrap();
    assert_eq!(len_large, large_buf.len());
    assert!((decoded_large.x - large.x).abs() < 0.2);
    assert!((decoded_large.y - large.y).abs() < 0.2);
    assert!((decoded_large.z - large.z).abs() < 0.2);

    // NaN sanitizes to 0 and huge values clamp to ABS_MAX_VALUE.
    let clamped_buf = lp_vec3_encode(Vec3::new(f64::NAN, 1.0e20, -1.0e20));
    let (decoded_clamped, _) = lp_vec3_decode(&clamped_buf).unwrap();
    assert_eq!(decoded_clamped.x, 0.0);
    assert_eq!(decoded_clamped.y, LP_VEC3_ABS_MAX_VALUE);
    assert_eq!(decoded_clamped.z, -LP_VEC3_ABS_MAX_VALUE);

    // Vec3 align, offset_random, and AABB helpers.
    let aligned = Vec3::new(1.7, -0.2, 3.9).align(&[Axis::X, Axis::Z]);
    assert_eq!(aligned, Vec3::new(1.0, -0.2, 3.0));

    let mut rng = LegacyRandom::new(7);
    let p = Vec3::ZERO.offset_random(&mut rng, 1.0);
    assert!(p.x.abs() <= 0.5 && p.y.abs() <= 0.5 && p.z.abs() <= 0.5);

    let mut builder = AabbBuilder::new();
    builder.include([1.0, 2.0, 3.0]).include([-1.0, 5.0, 0.0]);
    let built = builder.build();
    assert_eq!(built, Aabb::new(-1.0, 2.0, 0.0, 1.0, 5.0, 3.0));
    assert_eq!(built.get_center(), Vec3::new(0.0, 3.5, 1.5));
    assert_eq!(built.get_bottom_center(), Vec3::new(0.0, 2.0, 1.5));
    assert_eq!(built.get_size(), (2.0 + 3.0 + 3.0) / 3.0);
    assert_eq!(built.distance_to_sqr_point(Vec3::new(0.0, 3.0, 1.0)), 0.0);
    assert_eq!(built.distance_to_sqr_point(Vec3::new(4.0, 3.0, 1.0)), 9.0);
    assert!(built.contains(0.0, 2.0, 0.0));
    assert!(!built.contains(1.0, 2.0, 0.0));

    // Aabb::clip_many and collided_along_vector
    let b1 = Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
    let b2 = Aabb::new(2.0, 0.0, 0.0, 3.0, 1.0, 1.0);
    let hit = Aabb::clip_many(
        [b2, b1],
        Vec3::new(-1.0, 0.5, 0.5),
        Vec3::new(5.0, 0.5, 0.5),
        Vec3i::ZERO,
    )
    .expect("should hit closer box b1");
    assert_eq!(hit.0, Vec3::new(0.0, 0.5, 0.5));
    assert_eq!(hit.1, Direction::West);

    let mover = Aabb::new(-2.0, 0.0, 0.0, -1.0, 1.0, 1.0);
    assert!(mover.collided_along_vector(Vec3::new(3.0, 0.0, 0.0), [b1]));
    assert!(!mover.collided_along_vector(Vec3::new(0.0, 3.0, 0.0), [b1]));

    // Vec2 rotate
    let r = Vec2::new(1.0, 0.0).rotate(std::f64::consts::FRAC_PI_2);
    assert!(r.x.abs() < 1.0e-3 && (r.y - 1.0).abs() < 1.0e-3);
}
