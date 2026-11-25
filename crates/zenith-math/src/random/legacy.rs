//! Legacy 48-bit LCG random source (`java.util.Random` transition) and shared bit-source ops.

use crate::java::{self, D, F};
use crate::random::gaussian::MarsagliaPolarGaussian;
use crate::random::seed::position_hash;
use crate::random::{PositionalRandomFactory, RandomSource};

const LCG_MULTIPLIER: u64 = 0x0005_DEEC_E66D;
const LCG_ADDEND: u64 = 0xB;
const LCG_MASK: u64 = (1u64 << 48) - 1;

/// Trait for random sources backed by a `next(bits)` primitive (up to 32 bits per step).
///
/// Provides the shared derived operations used by [`LegacyRandom`] and any other bit-source RNG.
pub trait BitRandomSource {
    /// Advances the source and returns the top `bits` (in `1..=32`) as a signed 32-bit value.
    fn next_bits(&mut self, bits: u32) -> i32;

    /// Shared `nextInt()` derived from `next_bits(32)`.
    #[inline]
    fn bit_next_int(&mut self) -> i32 {
        self.next_bits(32)
    }

    /// Shared `nextInt(bound)` derived from `next_bits(31)`.
    ///
    /// # Panics
    ///
    /// Panics if `bound <= 0`.
    #[inline]
    fn bit_next_int_bounded(&mut self, bound: i32) -> i32 {
        assert!(bound > 0, "bound must be positive, got {bound}");

        if (bound & (bound - 1)) == 0 {
            return ((i64::from(bound) * i64::from(self.next_bits(31))) >> 31) as i32;
        }

        loop {
            let r = self.next_bits(31);
            let m = r % bound;
            if r.wrapping_sub(m).wrapping_add(bound - 1) >= 0 {
                return m;
            }
        }
    }

    /// Shared `nextLong()` derived as `(next_bits(32) << 32) + next_bits(32)` with the second
    /// value sign-extended before wrapping addition.
    #[inline]
    fn bit_next_long(&mut self) -> i64 {
        let hi = self.next_bits(32);
        let lo = self.next_bits(32);
        (i64::from(hi) << 32).wrapping_add(i64::from(lo))
    }

    /// Shared `nextBoolean()` derived as `next_bits(1) != 0`.
    #[inline]
    fn bit_next_boolean(&mut self) -> bool {
        self.next_bits(1) != 0
    }

    /// Shared `nextFloat()` derived as `next_bits(24) * F`.
    #[inline]
    fn bit_next_float(&mut self) -> f32 {
        (self.next_bits(24) as f32) * F
    }

    /// Shared `nextDouble()` derived as `((next_bits(26) << 27) + next_bits(27)) * D`.
    #[inline]
    fn bit_next_double(&mut self) -> f64 {
        let hi = self.next_bits(26);
        let lo = self.next_bits(27);
        let combined = (i64::from(hi) << 27) + i64::from(lo);
        (combined as f64) * D
    }
}

/// Legacy 48-bit LCG random number source.
///
/// Uses the same 48-bit state transition as `java.util.Random` (`state = (state * 0x5DEECE66D + 0xB)
/// & (2^48 - 1)`), with Zenith's [`D`](crate::java::D) constant in [`RandomSource::next_double`].
#[derive(Debug, Clone)]
pub struct LegacyRandom {
    state: u64,
    gaussian: MarsagliaPolarGaussian,
}

impl LegacyRandom {
    /// Creates a new `LegacyRandom` seeded with `seed`.
    #[inline]
    pub fn new(seed: i64) -> Self {
        let mut rng = Self {
            state: 0,
            gaussian: MarsagliaPolarGaussian::new(),
        };
        rng.set_seed(seed);
        rng
    }

    /// Returns the internal 48-bit LCG state.
    #[inline]
    pub const fn state(&self) -> u64 {
        self.state
    }

    #[inline]
    fn step_state(state: &mut u64, bits: u32) -> i32 {
        debug_assert!(bits <= 32, "bits must be in 0..=32, got {bits}");
        *state = state.wrapping_mul(LCG_MULTIPLIER).wrapping_add(LCG_ADDEND) & LCG_MASK;
        (*state >> (48 - bits)) as i32
    }
}

impl BitRandomSource for LegacyRandom {
    #[inline]
    fn next_bits(&mut self, bits: u32) -> i32 {
        assert!(bits <= 32, "bits must be in 0..=32, got {bits}");
        Self::step_state(&mut self.state, bits)
    }
}

impl RandomSource for LegacyRandom {
    type Positional = LegacyPositionalRandomFactory;

    #[inline]
    fn set_seed(&mut self, seed: i64) {
        self.state = ((seed as u64) ^ LCG_MULTIPLIER) & LCG_MASK;
        self.gaussian.reset();
    }

    #[inline]
    fn next_int(&mut self) -> i32 {
        self.bit_next_int()
    }

    #[inline]
    fn next_int_bounded(&mut self, bound: i32) -> i32 {
        self.bit_next_int_bounded(bound)
    }

    #[inline]
    fn next_long(&mut self) -> i64 {
        self.bit_next_long()
    }

    #[inline]
    fn next_boolean(&mut self) -> bool {
        self.bit_next_boolean()
    }

    #[inline]
    fn next_float(&mut self) -> f32 {
        self.bit_next_float()
    }

    #[inline]
    fn next_double(&mut self) -> f64 {
        self.bit_next_double()
    }

    #[inline]
    fn next_gaussian(&mut self) -> f64 {
        let state = &mut self.state;
        self.gaussian.next_gaussian(|| {
            let hi = Self::step_state(state, 26);
            let lo = Self::step_state(state, 27);
            let combined = (i64::from(hi) << 27) + i64::from(lo);
            (combined as f64) * D
        })
    }

    #[inline]
    fn fork(&mut self) -> Self {
        Self::new(self.next_long())
    }

    #[inline]
    fn fork_positional(&mut self) -> Self::Positional {
        LegacyPositionalRandomFactory::new(self.next_long())
    }
}

/// Positional random factory for [`LegacyRandom`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LegacyPositionalRandomFactory {
    seed: i64,
}

impl LegacyPositionalRandomFactory {
    /// Creates a new legacy positional factory with base seed `seed`.
    #[inline]
    pub const fn new(seed: i64) -> Self {
        Self { seed }
    }

    /// Returns the factory's 64-bit base seed.
    #[inline]
    pub const fn seed(&self) -> i64 {
        self.seed
    }
}

impl PositionalRandomFactory for LegacyPositionalRandomFactory {
    type Random = LegacyRandom;

    #[inline]
    fn at(&self, x: i32, y: i32, z: i32) -> Self::Random {
        LegacyRandom::new(position_hash(x, y, z) ^ self.seed)
    }

    #[inline]
    fn from_hash_of(&self, name: &str) -> Self::Random {
        let hash = i64::from(java::string_hash_code(name));
        LegacyRandom::new(hash ^ self.seed)
    }

    #[inline]
    fn from_seed(&self, seed: i64) -> Self::Random {
        // Per spec, the legacy factory does NOT mix its own seed into `from_seed`.
        LegacyRandom::new(seed)
    }
}
