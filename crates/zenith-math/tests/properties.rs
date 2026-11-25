//! Property and edge-case tests for `zenith-math` random number sources and seed helpers.

use zenith_math::java::{self, D, F, java_math_log};
use zenith_math::random::{
    BitRandomSource, GOLDEN_RATIO_64, LegacyPositionalRandomFactory, LegacyRandom,
    PositionalRandomFactory, RandomSource, SILVER_RATIO_64, Seed128,
    XoroshiroPositionalRandomFactory, XoroshiroRandom, mix13, position_hash, seed_from_name,
    upgrade_seed_to_128bit, upgrade_seed_to_128bit_unmixed,
};

#[test]
fn position_hash_preserves_32bit_x_wrap_and_sign_extension() {
    // 686 * 3_129_871 = 2_147_091_506 <= i32::MAX (no wrap)
    // 687 * 3_129_871 = 2_150_221_377 > i32::MAX (wraps to negative i32!)
    let naive_64 = |x: i32, y: i32, z: i32| -> i64 {
        let a = i64::from(x).wrapping_mul(3_129_871);
        let b = i64::from(z).wrapping_mul(116_129_781);
        let s = a ^ b ^ i64::from(y);
        let s = s
            .wrapping_mul(s)
            .wrapping_mul(42_317_861)
            .wrapping_add(s.wrapping_mul(11));
        s >> 16
    };

    assert_eq!(position_hash(686, 0, 0), naive_64(686, 0, 0));
    assert_ne!(
        position_hash(687, 0, 0),
        naive_64(687, 0, 0),
        "x = 687 must overflow 32-bit multiplication and diverge from naive 64-bit multiplication"
    );
    assert_ne!(position_hash(-687, 0, 0), naive_64(-687, 0, 0));
    assert_ne!(
        position_hash(100_000, 64, -50_000),
        naive_64(100_000, 64, -50_000)
    );

    // Exact step-by-step verification for large |x|, negative coords, and y extremes.
    let coords = [
        (0, 0, 0),
        (687, 0, 0),
        (-687, -64, -1_000_000),
        (i32::MAX, i32::MIN, i32::MAX),
        (i32::MIN, i32::MAX, i32::MIN),
        (30_000_000, -2048, -30_000_000),
    ];
    for (x, y, z) in coords {
        let a = i64::from(x.wrapping_mul(3_129_871));
        let b = i64::from(z).wrapping_mul(116_129_781);
        let s0 = a ^ b ^ i64::from(y);
        let s1 = s0
            .wrapping_mul(s0)
            .wrapping_mul(42_317_861)
            .wrapping_add(s0.wrapping_mul(11));
        assert_eq!(position_hash(x, y, z), s1 >> 16);
    }
}

#[test]
fn mix13_and_128bit_seed_upgrade_follow_spec() {
    assert_eq!(mix13(0), 0);
    // Known SplitMix64 / Mix13 value for 1:
    // z0 = (1 ^ 0) * 0xBF58476D1CE4E5B9 = 0xBF58476D1CE4E5B9
    // z1 = (z0 ^ (z0 >> 27)) * 0x94D049BB133111EB
    // out = z1 ^ (z1 >> 31)
    let z0 = 0xBF58_476D_1CE4_E5B9_u64;
    let z1 = (z0 ^ (z0 >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    let expected_1 = (z1 ^ (z1 >> 31)) as i64;
    assert_eq!(mix13(1), expected_1);

    for seed in [
        0i64,
        1,
        -1,
        42,
        SILVER_RATIO_64,
        GOLDEN_RATIO_64,
        i64::MIN,
        i64::MAX,
    ] {
        let unmixed = upgrade_seed_to_128bit_unmixed(seed);
        assert_eq!(unmixed.lo, seed ^ SILVER_RATIO_64);
        assert_eq!(unmixed.hi, unmixed.lo.wrapping_add(GOLDEN_RATIO_64));

        let mixed = upgrade_seed_to_128bit(seed);
        assert_eq!(mixed.lo, mix13(unmixed.lo));
        assert_eq!(mixed.hi, mix13(unmixed.hi));

        // XoroshiroRandom::new must match upgraded seed directly.
        let from_64 = XoroshiroRandom::new(seed);
        let from_128 = XoroshiroRandom::from_seed_128(mixed);
        assert_eq!(from_64.state(), from_128.state());
    }
}

#[test]
fn xoroshiro_zero_seed_replacement_matches_golden_silver() {
    let mut from_zero = XoroshiroRandom::from_raw_state(0, 0);
    let mut from_gs = XoroshiroRandom::from_raw_state(GOLDEN_RATIO_64, SILVER_RATIO_64);
    assert_eq!(
        from_zero.state(),
        Seed128::new(GOLDEN_RATIO_64, SILVER_RATIO_64)
    );

    for _ in 0..64 {
        assert_eq!(from_zero.next_long(), from_gs.next_long());
    }

    // Half-zero states must NOT be replaced.
    assert_eq!(
        XoroshiroRandom::from_raw_state(0, 1).state(),
        Seed128::new(0, 1)
    );
    assert_eq!(
        XoroshiroRandom::from_raw_state(1, 0).state(),
        Seed128::new(1, 0)
    );
}

#[test]
fn xoroshiro_derived_ops_follow_spec_formulas() {
    let mut raw = XoroshiroRandom::new(987_654_321);
    let mut tested = XoroshiroRandom::new(987_654_321);

    // next_int = low 32 bits of step
    for _ in 0..32 {
        assert_eq!(tested.next_int(), raw.next_long() as i32);
    }

    // next_boolean = lowest bit of step
    for _ in 0..32 {
        assert_eq!(tested.next_boolean(), (raw.next_long() & 1) != 0);
    }

    // next_float = (step >>> 40) * F
    for _ in 0..32 {
        let step = raw.next_long() as u64;
        let expected = ((step >> 40) as f32) * F;
        assert_eq!(tested.next_float().to_bits(), expected.to_bits());
    }

    // next_double = (step >>> 11) * D, and D is exactly 2^-53 (the widened f32 constant).
    for _ in 0..64 {
        let step = raw.next_long() as u64;
        let top53 = step >> 11;
        let got = tested.next_double();
        assert_eq!(got.to_bits(), ((top53 as f64) * D).to_bits());
        assert_eq!(got.to_bits(), ((top53 as f64) * 2f64.powi(-53)).to_bits());
    }
}

#[test]
fn bounded_ints_cover_powers_of_two_edges_and_rejection_loops() {
    // 1. bound = 1 always returns 0 and consumes exactly 1 step on both sources.
    let mut leg_a = LegacyRandom::new(1234);
    let mut leg_b = LegacyRandom::new(1234);
    assert_eq!(leg_a.next_int_bounded(1), 0);
    let _ = leg_b.next_bits(31);
    assert_eq!(leg_a.state(), leg_b.state());

    let mut xoro_a = XoroshiroRandom::new(1234);
    let mut xoro_b = XoroshiroRandom::new(1234);
    assert_eq!(xoro_a.next_int_bounded(1), 0);
    let _ = xoro_b.next_long();
    assert_eq!(xoro_a.state(), xoro_b.state());

    // 2. Powers of two (1, 2, 4, ..., 2^30) never reject and stay strictly in range.
    for shift in 0..=30u32 {
        let bound = 1i32 << shift;
        let mut leg = LegacyRandom::new(i64::from(bound));
        let mut xoro = XoroshiroRandom::new(i64::from(bound));
        let mut xoro_raw = XoroshiroRandom::new(i64::from(bound));
        for _ in 0..32 {
            let lv = leg.next_int_bounded(bound);
            assert!((0..bound).contains(&lv));

            let xv = xoro.next_int_bounded(bound);
            assert!((0..bound).contains(&xv));
            // For power-of-two bound = 2^k, multiply-and-reject never redraws and yields
            // (u * 2^k) >> 32 == u >> (32 - k), where u is low 32 bits of next_long().
            let u = (xoro_raw.next_long() as u32) as u64;
            assert_eq!(xv, ((u * (bound as u64)) >> 32) as i32);
        }
    }

    // 3. bound = i32::MAX (2^31 - 1) and high-rejection bounds (2^30 + 1, 0x6000_0000).
    let bounds = [3, 7, 13, 100, 0x4000_0001, 0x6000_0000, i32::MAX];
    for &bound in &bounds {
        let mut leg = LegacyRandom::new(0xCAFE_BABE);
        let mut xoro = XoroshiroRandom::new(0xCAFE_BABE);
        for _ in 0..256 {
            let lv = leg.next_int_bounded(bound);
            let xv = xoro.next_int_bounded(bound);
            assert!(
                (0..bound).contains(&lv),
                "Legacy out of range for bound {bound}"
            );
            assert!(
                (0..bound).contains(&xv),
                "Xoroshiro out of range for bound {bound}"
            );
        }
    }

    // 4. Verify rejection loop actually triggers on LegacyRandom for bound = 0x4000_0001.
    // With bound = 2^30 + 1, any r >= 2^30 + 1 fails (r - m + bound - 1) >= 0 (~50% rejection).
    let mut leg_bound = LegacyRandom::new(42);
    let mut leg_step = LegacyRandom::new(42);
    let mut leg_rejections = 0usize;
    for _ in 0..100 {
        let _ = leg_bound.next_int_bounded(0x4000_0001);
        let mut steps = 0usize;
        while leg_step.state() != leg_bound.state() {
            let _ = leg_step.next_bits(31);
            steps += 1;
        }
        if steps > 1 {
            leg_rejections += 1;
        }
    }
    assert!(
        leg_rejections > 10,
        "expected Legacy rejection loop to trigger"
    );

    // 5. Verify multiply-and-reject actually triggers on XoroshiroRandom for bound = 0x6000_0000.
    // t = (2^32 - 0x6000_0000) % 0x6000_0000 = 0xA000_0000 % 0x6000_0000 = 0x4000_0000 (25% reject).
    let mut xoro_bound = XoroshiroRandom::new(42);
    let mut xoro_step = XoroshiroRandom::new(42);
    let mut xoro_rejections = 0usize;
    for _ in 0..100 {
        let val = xoro_bound.next_int_bounded(0x6000_0000);
        let mut steps = 0usize;
        let mut last_u = 0u64;
        while xoro_step.state() != xoro_bound.state() {
            last_u = (xoro_step.next_int() as u32) as u64;
            steps += 1;
        }
        assert_eq!(val, ((last_u * 0x6000_0000_u64) >> 32) as i32);
        if steps > 1 {
            xoro_rejections += 1;
        }
    }
    assert!(
        xoro_rejections > 5,
        "expected Xoroshiro rejection loop to trigger"
    );
}

#[test]
#[should_panic(expected = "bound must be positive")]
fn legacy_next_int_bounded_rejects_zero() {
    let mut rng = LegacyRandom::new(0);
    let _ = rng.next_int_bounded(0);
}

#[test]
#[should_panic(expected = "bound must be positive")]
fn legacy_next_int_bounded_rejects_negative() {
    let mut rng = LegacyRandom::new(0);
    let _ = rng.next_int_bounded(-5);
}

#[test]
#[should_panic(expected = "bound must be positive")]
fn xoroshiro_next_int_bounded_rejects_zero() {
    let mut rng = XoroshiroRandom::new(0);
    let _ = rng.next_int_bounded(0);
}

#[test]
#[should_panic(expected = "bound must be positive")]
fn xoroshiro_next_int_bounded_rejects_min() {
    let mut rng = XoroshiroRandom::new(0);
    let _ = rng.next_int_bounded(i32::MIN);
}

#[test]
fn fork_and_fork_positional_draw_order() {
    // Legacy fork: seeds child with this.next_long()
    let mut leg = LegacyRandom::new(777);
    let mut leg_ref = LegacyRandom::new(777);
    let expected_seed = leg_ref.next_long();
    let mut child = leg.fork();
    let mut expected_child = LegacyRandom::new(expected_seed);
    assert_eq!(leg.state(), leg_ref.state());
    for _ in 0..16 {
        assert_eq!(child.next_long(), expected_child.next_long());
    }

    // Legacy fork_positional: base seed P = this.next_long()
    let expected_p = leg_ref.next_long();
    let factory = leg.fork_positional();
    assert_eq!(factory.seed(), expected_p);
    assert_eq!(leg.state(), leg_ref.state());

    // Xoroshiro fork: (lo, hi) = (next_long(), next_long()) in that order
    let mut xoro = XoroshiroRandom::new(777);
    let mut xoro_ref = XoroshiroRandom::new(777);
    let expected_lo = xoro_ref.next_long();
    let expected_hi = xoro_ref.next_long();
    let mut xchild = xoro.fork();
    let mut expected_xchild = XoroshiroRandom::from_raw_state(expected_lo, expected_hi);
    assert_eq!(xoro.state(), xoro_ref.state());
    assert_eq!(xchild.state(), Seed128::new(expected_lo, expected_hi));
    // Verify order is NOT swapped
    assert_ne!(expected_lo, expected_hi);
    for _ in 0..16 {
        assert_eq!(xchild.next_long(), expected_xchild.next_long());
    }

    // Xoroshiro fork_positional: (L, H) = (next_long(), next_long()) in that order
    let exp_l = xoro_ref.next_long();
    let exp_h = xoro_ref.next_long();
    let xfactory = xoro.fork_positional();
    assert_eq!(xfactory.state(), Seed128::new(exp_l, exp_h));
    assert_eq!(xoro.state(), xoro_ref.state());
}

#[test]
fn positional_factories_follow_exact_seeding_rules() {
    // Legacy positional factory
    let p = 0x1357_9BDF_2468_ACE0_u64 as i64;
    let lf = LegacyPositionalRandomFactory::new(p);

    // at(x, y, z) -> LegacyRandom::new(h(x, y, z) ^ P)
    let at_rng = lf.at(687, -32, 9999);
    assert_eq!(
        at_rng.state(),
        LegacyRandom::new(position_hash(687, -32, 9999) ^ p).state()
    );

    // from_hash_of(name) -> LegacyRandom::new(sign_ext(String.hashCode(name)) ^ P)
    // Use a string whose Java hashCode is negative to verify sign-extension!
    let neg_name = "polygenelubricants";
    assert!(java::string_hash_code(neg_name) < 0);
    let hash_rng = lf.from_hash_of(neg_name);
    let expected_hash_seed = i64::from(java::string_hash_code(neg_name)) ^ p;
    assert_eq!(
        hash_rng.state(),
        LegacyRandom::new(expected_hash_seed).state()
    );

    // Non-ASCII name
    let utf_name = "café_🎉";
    assert_eq!(
        lf.from_hash_of(utf_name).state(),
        LegacyRandom::new(i64::from(java::string_hash_code(utf_name)) ^ p).state()
    );

    // from_seed(s) -> LegacyRandom::new(s), P is NOT mixed in!
    for s in [0i64, 42, -1, p] {
        assert_eq!(lf.from_seed(s).state(), LegacyRandom::new(s).state());
    }

    // Xoroshiro positional factory
    let l = 0x1111_2222_3333_4444_i64;
    let h = 0x5555_6666_7777_8888_i64;
    let xf = XoroshiroPositionalRandomFactory::new(l, h);

    // at(x, y, z) -> raw (h(x, y, z) ^ L, H) — H untouched, no upgrade!
    let xat = xf.at(687, -32, 9999);
    assert_eq!(
        xat.state(),
        Seed128::new(position_hash(687, -32, 9999) ^ l, h)
    );

    // from_hash_of(name) -> (md5.lo ^ L, md5.hi ^ H)
    for name in ["", "example:overworld", "café", "日本語", "a🎉b🚀c"] {
        let ns = seed_from_name(name);
        assert_eq!(
            xf.from_hash_of(name).state(),
            Seed128::new(ns.lo ^ l, ns.hi ^ h)
        );
    }

    // from_seed(s) -> (s ^ L, s ^ H), including s = 0 -> (L, H)
    assert_eq!(xf.from_seed(0).state(), Seed128::new(l, h));
    assert_eq!(xf.from_seed(42).state(), Seed128::new(42 ^ l, 42 ^ h));

    // If (s ^ L, s ^ H) == (0, 0), XoroshiroRandom replaces (0, 0) with (G, S)!
    let same = 0xDEAD_BEEF_CAFE_BABE_u64 as i64;
    let xf_same = XoroshiroPositionalRandomFactory::new(same, same);
    assert_eq!(
        xf_same.from_seed(same).state(),
        Seed128::new(GOLDEN_RATIO_64, SILVER_RATIO_64)
    );
    let xf_zero = XoroshiroPositionalRandomFactory::new(0, 0);
    assert_eq!(
        xf_zero.from_seed(0).state(),
        Seed128::new(GOLDEN_RATIO_64, SILVER_RATIO_64)
    );
}

#[test]
fn gaussian_matches_spec_formula_caches_pair_and_resets_on_set_seed() {
    fn verify_gaussian_source<R: RandomSource + Clone>(mut rng: R, seed: i64) {
        let mut ref_rng = rng.clone();

        // Draw 10 Gaussian pairs and check every single value bit-for-bit against the spec formula.
        for _ in 0..10 {
            let (expected_first, expected_second) = loop {
                let x = 2.0 * ref_rng.next_double() - 1.0;
                let y = 2.0 * ref_rng.next_double() - 1.0;
                let r = x * x + y * y;
                if r > 0.0 && r < 1.0 {
                    let m = (-2.0 * java_math_log(r) / r).sqrt();
                    break (x * m, y * m);
                }
            };

            let first = rng.next_gaussian();
            assert_eq!(first.to_bits(), expected_first.to_bits());

            // Second draw must come from cache without advancing underlying stream.
            let before_second = rng.clone();
            let second = rng.next_gaussian();
            assert_eq!(second.to_bits(), expected_second.to_bits());

            // Verify underlying stream did NOT advance on the cached draw:
            let mut a = before_second;
            let mut b = rng.clone();
            assert_eq!(a.next_long(), b.next_long());
        }

        // Cache-clear check across set_seed:
        // Draw 1 Gaussian (populates cache), then set_seed(seed), then draw again.
        // The post-set_seed draw MUST equal a fresh source's first draw, NOT the stale cached value.
        rng.set_seed(seed);
        let first_fresh = rng.next_gaussian();
        let second_fresh = rng.next_gaussian();
        assert_ne!(first_fresh.to_bits(), second_fresh.to_bits());

        rng.set_seed(seed);
        let draw_1 = rng.next_gaussian();
        assert_eq!(draw_1.to_bits(), first_fresh.to_bits());
        // Cache now holds `second_fresh`. Re-seeding must discard it!
        rng.set_seed(seed);
        let after_reset = rng.next_gaussian();
        assert_eq!(after_reset.to_bits(), first_fresh.to_bits());
    }

    verify_gaussian_source(LegacyRandom::new(1337), 1337);
    verify_gaussian_source(XoroshiroRandom::new(1337), 1337);
}

#[test]
fn consume_count_advances_exact_number_of_steps_without_clearing_gaussian_cache() {
    // Legacy: n steps == n next_int() calls
    let mut leg1 = LegacyRandom::new(555);
    let mut leg2 = LegacyRandom::new(555);
    leg1.consume_count(0);
    assert_eq!(leg1.state(), leg2.state());
    leg1.consume_count(17);
    for _ in 0..17 {
        let _ = leg2.next_int();
    }
    assert_eq!(leg1.state(), leg2.state());

    // Xoroshiro: n steps == n next_long() calls
    let mut xoro1 = XoroshiroRandom::new(555);
    let mut xoro2 = XoroshiroRandom::new(555);
    xoro1.consume_count(0);
    assert_eq!(xoro1.state(), xoro2.state());
    xoro1.consume_count(17);
    for _ in 0..17 {
        let _ = xoro2.next_long();
    }
    assert_eq!(xoro1.state(), xoro2.state());

    // consume_count must NOT clear the Gaussian cache
    let mut g1 = XoroshiroRandom::new(42);
    let mut g2 = XoroshiroRandom::new(42);
    let _ = g1.next_gaussian();
    let _ = g2.next_gaussian();
    g1.consume_count(5);
    assert_eq!(g1.next_gaussian().to_bits(), g2.next_gaussian().to_bits());
}
