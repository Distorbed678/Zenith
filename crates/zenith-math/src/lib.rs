//! Java-exact math primitives and random number sources for Zenith.
//!
//! This crate is an independent clean-room implementation built from the behavior specifications
//! in `docs/specs/math/` and public algorithm definitions (JDK `java.util.Random`, Blackman &
//! Vigna's `xoroshiro128++`, and Stafford's Mix13 finalizer).
//!
//! # Modules
//!
//! - [`java`]: Explicit Java integer/shift semantics (`i32`/`i64` wrapping ops, `>>>`, UTF-16
//!   `String.hashCode`), the float multipliers [`java::F`] and [`java::D`], and the centralized
//!   logarithm entry point [`java::java_math_log`].
//! - [`random`]: The [`random::RandomSource`] and [`random::PositionalRandomFactory`] traits,
//!   seed-derivation helpers (Mix13, 128-bit upgrade, UTF-8 MD5 name seeding, 32-bit-wrapping
//!   position hash), [`random::LegacyRandom`] (48-bit LCG), and [`random::XoroshiroRandom`]
//!   (`xoroshiro128++`).
//!
//! # Parity invariants
//!
//! - Floating-point `next_double` uses [`java::D`]: the `f32` constant `1.110223e-16` widened to
//!   `f64`, which is exactly `2^-53`.
//! - Position hashing performs the `x * 3_129_871` product in wrapping 32-bit arithmetic before
//!   sign-extending to 64 bits.
//! - Gaussian sampling uses the Marsaglia polar method with a one-value cache cleared on
//!   [`random::RandomSource::set_seed`].
//!
//! # Example
//!
//! ```
//! use zenith_math::{LegacyRandom, PositionalRandomFactory, RandomSource, XoroshiroRandom};
//!
//! let mut legacy = LegacyRandom::new(42);
//! let _ = legacy.next_int_bounded(16);
//!
//! let mut xoro = XoroshiroRandom::new(42);
//! let factory = xoro.fork_positional();
//! let mut chunk_rng = factory.at(12, 64, -8);
//! let _ = chunk_rng.next_gaussian();
//! ```

pub mod java;
pub mod noise;
pub mod random;

pub use noise::{
    BlendedNoise, BlendedNoiseParameters, GradientNoiseState, Interval, Noise, NoiseError,
    NormalNoise, NormalNoiseParameters, NormalizationMode, OctaveLayer, OctaveLayerNoise,
    OctaveStack, OctaveStackBuilder, PerlinNoise, SimplexNoise, SmearedPerlinNoise, VolumeGrid,
};
pub use random::{
    BitRandomSource, LegacyPositionalRandomFactory, LegacyRandom, PositionalRandomFactory,
    RandomSource, Seed128, XoroshiroPositionalRandomFactory, XoroshiroRandom,
};
