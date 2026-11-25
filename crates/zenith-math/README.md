# `zenith-math`

Java-exact math primitives, deterministic random number sources, and 26.3 noise generators for
Zenith, implemented clean-room from [`docs/specs/math/random.md`](../../docs/specs/math/random.md),
[`docs/specs/math/noise.md`](../../docs/specs/math/noise.md), and public algorithm specifications.

## Modules

- **`java`**: Wrapping 32-bit/64-bit integer operations, Java shift semantics (`<<`, `>>`, `>>>`,
  `rotateLeft`/`rotateRight`), UTF-16 `String.hashCode`, the float multipliers `F` (`5.9604645e-8f`
  = `2^-24`) and `D` (`1.110223e-16_f32 as f64` = `2^-53`), and `java_math_log`.
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
- **`noise`**:
  - `primitives`: `GradientNoiseState` (3 `next_double` draws + 256-step forward Fisher-Yates
    including `next_int_bounded(1)`), 16-entry `GRADIENT` table, asymmetric `wrap` at
    `nextDown(2^24)`, conservative `f32` `Interval` bounds, Z-X-Y `VolumeGrid`, `Noise` trait,
    and `f32` smoothstep / trilinear interpolation helpers.
  - `perlin`: `PerlinNoise` (3D, 2D XZ slice, accumulating analytical derivatives, column-cached
    volume accumulation).
  - `smeared`: `SmearedPerlinNoise` ("y-fuzz" variant with unwrapped `y_in` clamp, widened `f32`
    `1e-7f` epsilon, and split fade vs dot Y coordinate).
  - `simplex`: `SimplexNoise` (unwrapped 2D/3D `f64`-internal simplex noise with 32-bit wrapping
    unskew sum and `% 12` gradient indexing), plus fixed-seed biome (`1234`, `2345`, `3456`) and
    End-island (`consume_count(17292)`) constructors.
  - `octave`: Flattened `OctaveStack` / `OctaveStackBuilder` and `build_legacy_fbm` (unconditional
    octave-0 draw and 262-draw skipping).
  - `normal`: `NormalNoiseParameters`, `NormalizationMode` (`Enabled`, `Disabled`, `Legacy`),
    built-in `parity` constructor, and `NormalNoise` (interleaved standard instantiation and
    non-interleaved legacy Nether instantiation).
  - `blended`: `BlendedNoiseParameters` (with `OVERWORLD`, `NETHER`, `END` presets),
    `build_smeared_fbm`, and `BlendedNoise` (`minecraft:old_blended_noise`) with exact `0.0` /
    `1.0` selector short-circuiting and volume evaluation.
  - `utils`: `java_math_sin`, `bias_towards_extreme`, and Java `HALF_UP` `parity_octave_config` /
    `parity_octave_config_i32`.

## Verification

Regenerate the JDK 21 reference fixture (requires only a standard JDK 21 runtime, no game files):

```bash
java tools/javamap-fixtures/RngFixtures.java > crates/zenith-math/tests/fixtures/rng_jdk.json
```

Run tests and lints:

```bash
cargo test -p zenith-math
RUSTFLAGS="-D warnings" cargo clippy -p zenith-math --all-targets
```
