# `zenith-math`

Java-exact math primitives and deterministic random number sources for Zenith, implemented
clean-room from [`docs/specs/math/random.md`](../../docs/specs/math/random.md) and public
algorithm specifications.

## Modules

- **`java`**: Wrapping 32-bit/64-bit integer operations, Java shift semantics (`<<`, `>>`, `>>>`,
  `rotateLeft`/`rotateRight`), UTF-16 `String.hashCode`, the float multipliers `F` (`5.9604645e-8f`
  = `2^-24`) and `D` (`1.110223e-16_f64`, intentionally not `2^-53`), and `java_math_log`.
- **`random`**:
  - `RandomSource` and `PositionalRandomFactory` traits.
  - `BitRandomSource` shared derived operations (`next_int`, `next_int_bounded`, `next_long`,
    `next_boolean`, `next_float`, `next_double`).
  - `LegacyRandom` (48-bit LCG) and `LegacyPositionalRandomFactory`.
  - `XoroshiroRandom` (`xoroshiro128++` with multiply-and-reject bounded ints, top-bits float and
    double conversion, and `(0, 0) -> (G, S)` zero-seed replacement) and
    `XoroshiroPositionalRandomFactory`.
  - Seed helpers: `mix13`, `upgrade_seed_to_128bit`, `upgrade_seed_to_128bit_unmixed`,
    `seed_from_name` (MD5 over UTF-8), and `position_hash` (preserving the 32-bit `x * 3129871`
    wrap).
  - `MarsagliaPolarGaussian` polar sampler with one-value cache cleared on `set_seed`.

## Verification

Regenerate the JDK 21 reference fixture (requires only a standard JDK 21 runtime, no game files):

```bash
java tools/javamap-fixtures/RngFixtures.java > crates/zenith-math/tests/fixtures/rng_jdk.json
```

Run tests and lints:

```bash
cargo test -p zenith-math
cargo clippy -p zenith-math --all-targets -- -D warnings
```
