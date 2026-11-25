//! Java-exact random number sources and seed helpers.
//!
//! Implements the random number algorithms specified in `docs/specs/math/random.md`:
//!
//! - [`LegacyRandom`]: 48-bit LCG (`java.util.Random` transition) with Zenith's shared
//!   [`BitRandomSource`] derived operations and [`java::D`](crate::java::D) double scaling.
//! - [`XoroshiroRandom`]: 128-bit `xoroshiro128++` with multiply-and-reject bounded ints,
//!   top-bits float/double conversion, and `(0, 0) -> (G, S)` zero-seed replacement.
//! - [`LegacyPositionalRandomFactory`] and [`XoroshiroPositionalRandomFactory`]: deterministic
//!   positional, named, and seeded child-source factories.
//! - Seed helpers: [`mix13`], [`upgrade_seed_to_128bit`], [`upgrade_seed_to_128bit_unmixed`],
//!   [`seed_from_name`], and [`position_hash`].

mod gaussian;
mod legacy;
mod seed;
mod xoroshiro;

pub use gaussian::MarsagliaPolarGaussian;
pub use legacy::{BitRandomSource, LegacyPositionalRandomFactory, LegacyRandom};
pub use seed::{
    GOLDEN_RATIO_64, SILVER_RATIO_64, Seed128, mix13, position_hash, seed_from_name,
    upgrade_seed_to_128bit, upgrade_seed_to_128bit_unmixed,
};
pub use xoroshiro::{XoroshiroPositionalRandomFactory, XoroshiroRandom};

/// Common interface for deterministic random number sources.
pub trait RandomSource: Sized {
    /// Positional factory produced by [`RandomSource::fork_positional`].
    type Positional: PositionalRandomFactory<Random = Self>;

    /// Re-seeds this random source and clears its cached Gaussian sample.
    fn set_seed(&mut self, seed: i64);

    /// Returns the next signed 32-bit integer.
    fn next_int(&mut self) -> i32;

    /// Returns a uniformly distributed integer in `0..bound`.
    ///
    /// # Panics
    ///
    /// Panics if `bound <= 0`.
    fn next_int_bounded(&mut self, bound: i32) -> i32;

    /// Returns the next signed 64-bit integer.
    fn next_long(&mut self) -> i64;

    /// Returns the next boolean.
    fn next_boolean(&mut self) -> bool;

    /// Returns a single-precision float in `[0.0, 1.0)`.
    fn next_float(&mut self) -> f32;

    /// Returns a double-precision float in `[0.0, 1.0)` scaled by [`crate::java::D`].
    fn next_double(&mut self) -> f64;

    /// Returns a standard normal (`mean = 0.0, stddev = 1.0`) sample using the Marsaglia polar
    /// method with a one-value cache.
    fn next_gaussian(&mut self) -> f64;

    /// Creates an independent child random source seeded from this source's current stream.
    fn fork(&mut self) -> Self;

    /// Creates a [`PositionalRandomFactory`] seeded from this source's current stream.
    fn fork_positional(&mut self) -> Self::Positional;

    /// Advances the source by `count` steps, discarding the results.
    fn consume_count(&mut self, count: usize) {
        for _ in 0..count {
            let _ = self.next_int();
        }
    }
}

/// Deterministic factory for deriving random sources from block coordinates, names, or seeds.
pub trait PositionalRandomFactory {
    /// Random source type created by this factory.
    type Random: RandomSource;

    /// Creates a random source for 3D integer coordinates `(x, y, z)`.
    fn at(&self, x: i32, y: i32, z: i32) -> Self::Random;

    /// Creates a random source from the hash of `name`.
    #[allow(clippy::wrong_self_convention)]
    fn from_hash_of(&self, name: &str) -> Self::Random;

    /// Creates a random source from a 64-bit `seed`.
    #[allow(clippy::wrong_self_convention)]
    fn from_seed(&self, seed: i64) -> Self::Random;
}
