//! Java-exact 26.3 noise generators for Zenith.
//!
//! Independent clean-room implementation built from `docs/specs/math/noise.md`,
//! `docs/specs/math/random.md`, Ken Perlin's improved gradient noise (2002), and Stefan
//! Gustavson's simplex noise formulation (2005).
//!
//! All generators follow the unified 26.3 pipeline: world coordinates and lattice setup in
//! `f64`, sampling outputs and conservative interval bounds in `f32`, and Z-X-Y column-major
//! volume accumulation.
//!
//! # Components
//!
//! - [`primitives`]: [`GradientNoiseState`], 16-vector [`GRADIENT`] table, [`wrap`],
//!   [`Interval`], [`VolumeGrid`], [`Noise`] trait, and `f32` smoothstep/interpolation helpers.
//! - [`perlin`]: [`PerlinNoise`] (3D, 2D XZ, analytical derivatives, column-cached volume path).
//! - [`smeared`]: [`SmearedPerlinNoise`] ("y-fuzz" variant with quantized Y displacement).
//! - [`simplex`]: [`SimplexNoise`] (2D/3D without wrapping) plus fixed-seed biome and End-island
//!   constructors.
//! - [`octave`]: [`OctaveStack`], [`OctaveStackBuilder`], and [`build_legacy_fbm`].
//! - [`normal`]: [`NormalNoiseParameters`], [`NormalizationMode`], and [`NormalNoise`]
//!   (standard interleaved instantiation, legacy Nether instantiation, and parity constructor).
//! - [`blended`]: [`BlendedNoiseParameters`], [`BlendedNoise`] (`minecraft:old_blended_noise`),
//!   and [`build_smeared_fbm`].
//! - [`utils`]: [`bias_towards_extreme`], [`java_math_sin`], and parity config formatting.
//!
//! # Example
//!
//! ```
//! use zenith_math::noise::{NormalNoise, NormalizationMode, PerlinNoise};
//! use zenith_math::XoroshiroRandom;
//!
//! let mut rng = XoroshiroRandom::new(12345);
//! let perlin = PerlinNoise::new(&mut rng);
//! let _single = perlin.sample(1.25, 64.0, -3.5);
//!
//! let normal = NormalNoise::from_octaves(
//!     &mut rng,
//!     -7,
//!     vec![1.0, 1.0, 2.0],
//!     NormalizationMode::Enabled,
//! )
//! .expect("valid parameters");
//! let _combined = normal.sample(10.0, 64.0, 20.0);
//! ```

pub mod blended;
pub mod normal;
pub mod octave;
pub mod perlin;
pub mod primitives;
pub mod simplex;
pub mod smeared;
pub mod utils;

pub use blended::{
    BLENDED_BASE_SCALE, BLENDED_LIMIT_FACTOR, BLENDED_LIMIT_FIRST_OCTAVE, BLENDED_MAIN_FACTOR,
    BLENDED_MAIN_FIRST_OCTAVE, BLENDED_NOISE_ID, BLENDED_NOISE_SEED_ID, BlendedNoise,
    BlendedNoiseParameters, build_smeared_fbm,
};
pub use normal::{
    ActiveOctave, MAX_AMPLITUDE, MAX_BASE_OCTAVE, MAX_OCTAVES, MIN_BASE_AMPLITUDE, MIN_BASE_OCTAVE,
    NETHER_TEMPERATURE_ID, NETHER_VEGETATION_ID, NORMAL_INPUT_FACTOR, NORMAL_TARGET_DEVIATION,
    NormalNoise, NormalNoiseParameters, NormalizationMode,
};
pub use octave::{
    LEGACY_FBM_SKIP_COUNT, OctaveLayer, OctaveLayerNoise, OctaveStack, OctaveStackBuilder,
    build_legacy_fbm,
};
pub use perlin::{PERLIN_STD_DEV, PerlinNoise};
pub use primitives::{
    DEFAULT_OFFSET_SCALE, GRADIENT, GradientNoiseState, Interval, Noise, NoiseError, VolumeGrid,
    WRAP_HALF_BOUND, WRAP_PERIOD, floor_to_i32, grad_dot_f32, grad_dot_f64, grad_dot_xz_f32, lerp,
    lerp2, lerp3, smoothstep, smoothstep_derivative, wrap,
};
pub use simplex::{
    F2, F3, G2, G3, SIMPLEX_STD_DEV, SQRT_3, SimplexNoise, biome_frozen_temperature_noise,
    biome_info_noise, biome_temperature_noise, end_island_noise,
};
pub use smeared::{SMEAR_EPSILON, SmearedPerlinNoise};
pub use utils::{
    bias_towards_extreme, java_math_sin, parity_octave_config, parity_octave_config_i32,
};
