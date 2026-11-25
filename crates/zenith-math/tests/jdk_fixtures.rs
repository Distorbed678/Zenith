//! Tests verifying bit-exact parity against fixtures generated on the real JDK 21 runtime
//! (`tools/javamap-fixtures/RngFixtures.java` -> `tests/fixtures/rng_jdk.json`).

use serde::Deserialize;
use zenith_math::java::{self, D, F};
use zenith_math::random::{LegacyRandom, RandomSource, Seed128, XoroshiroRandom, seed_from_name};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureRoot {
    constants: ConstantsFixture,
    strings: Vec<StringFixture>,
    legacy: Vec<LegacyFixture>,
    xoroshiro128_plus_plus: Vec<XoroshiroFixture>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConstantsFixture {
    f_bits: u32,
    d_float_widened_bits: u64,
    pow_neg53_bits: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StringFixture {
    text: String,
    hash_code: i32,
    md5_lo: i64,
    md5_hi: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyFixture {
    seed: i64,
    next_int: Vec<i32>,
    next_long: Vec<i64>,
    next_boolean: Vec<bool>,
    next_float_bits: Vec<u32>,
    spec_next_double_bits: Vec<i64>,
    jdk_next_double_bits: Vec<i64>,
    bounded: Vec<BoundedFixture>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BoundedFixture {
    bound: i32,
    values: Vec<i32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct XoroshiroFixture {
    lo: i64,
    hi: i64,
    next_long: Vec<i64>,
}

fn load_fixture() -> FixtureRoot {
    serde_json::from_str(include_str!("fixtures/rng_jdk.json"))
        .expect("rng_jdk.json should deserialize cleanly")
}

#[test]
fn jdk_float_constants_match_bit_for_bit() {
    let fixture = load_fixture();
    assert_eq!(F.to_bits(), fixture.constants.f_bits);
    assert_eq!(D.to_bits(), fixture.constants.d_float_widened_bits);
    assert_eq!(2f64.powi(-53).to_bits(), fixture.constants.pow_neg53_bits);
    assert_eq!(
        (1.110_223e-16_f32 as f64).to_bits(),
        fixture.constants.d_float_widened_bits
    );
    assert_eq!(
        fixture.constants.d_float_widened_bits,
        fixture.constants.pow_neg53_bits
    );
    assert_eq!(D.to_bits(), fixture.constants.pow_neg53_bits);
}

#[test]
fn jdk_string_hash_code_and_md5_seed_match() {
    let fixture = load_fixture();
    assert!(!fixture.strings.is_empty());

    for case in &fixture.strings {
        assert_eq!(
            java::string_hash_code(&case.text),
            case.hash_code,
            "String.hashCode mismatch for {:?}",
            case.text
        );
        assert_eq!(
            seed_from_name(&case.text),
            Seed128::new(case.md5_lo, case.md5_hi),
            "MD5 name seed mismatch for {:?}",
            case.text
        );
    }
}

#[test]
fn jdk_legacy_random_streams_match_bit_for_bit() {
    let fixture = load_fixture();
    assert!(!fixture.legacy.is_empty());

    for case in &fixture.legacy {
        // nextInt
        let mut rng = LegacyRandom::new(case.seed);
        let got_ints: Vec<i32> = (0..case.next_int.len()).map(|_| rng.next_int()).collect();
        assert_eq!(
            got_ints, case.next_int,
            "nextInt mismatch for seed {}",
            case.seed
        );

        // nextLong
        rng.set_seed(case.seed);
        let got_longs: Vec<i64> = (0..case.next_long.len()).map(|_| rng.next_long()).collect();
        assert_eq!(
            got_longs, case.next_long,
            "nextLong mismatch for seed {}",
            case.seed
        );

        // nextBoolean
        rng.set_seed(case.seed);
        let got_bools: Vec<bool> = (0..case.next_boolean.len())
            .map(|_| rng.next_boolean())
            .collect();
        assert_eq!(
            got_bools, case.next_boolean,
            "nextBoolean mismatch for seed {}",
            case.seed
        );

        // nextFloat
        rng.set_seed(case.seed);
        let got_float_bits: Vec<u32> = (0..case.next_float_bits.len())
            .map(|_| rng.next_float().to_bits())
            .collect();
        assert_eq!(
            got_float_bits, case.next_float_bits,
            "nextFloat mismatch for seed {}",
            case.seed
        );

        // nextDouble: must match the spec formula, which equals java.util.Random exactly because D == 2^-53
        rng.set_seed(case.seed);
        let got_double_bits: Vec<i64> = (0..case.spec_next_double_bits.len())
            .map(|_| rng.next_double().to_bits() as i64)
            .collect();
        assert_eq!(
            got_double_bits, case.spec_next_double_bits,
            "spec nextDouble mismatch for seed {}",
            case.seed
        );
        assert_eq!(
            got_double_bits, case.jdk_next_double_bits,
            "nextDouble must equal java.util.Random for seed {}",
            case.seed
        );

        // nextInt(bound) for all tested bounds
        for bounded in &case.bounded {
            rng.set_seed(case.seed);
            let got_vals: Vec<i32> = (0..bounded.values.len())
                .map(|_| rng.next_int_bounded(bounded.bound))
                .collect();
            assert_eq!(
                got_vals, bounded.values,
                "nextInt({}) mismatch for seed {}",
                bounded.bound, case.seed
            );
        }
    }

    // Spec edge case 1: D must produce different low bits from 2^-53 on real streams.
}

#[test]
fn jdk_xoroshiro128_plus_plus_next_long_matches_bit_for_bit() {
    let fixture = load_fixture();
    assert!(!fixture.xoroshiro128_plus_plus.is_empty());

    for case in &fixture.xoroshiro128_plus_plus {
        let mut rng = XoroshiroRandom::from_raw_state(case.lo, case.hi);
        let got: Vec<i64> = (0..case.next_long.len()).map(|_| rng.next_long()).collect();
        assert_eq!(
            got, case.next_long,
            "xoroshiro128++ nextLong mismatch for state ({:#x}, {:#x})",
            case.lo, case.hi
        );
    }
}
