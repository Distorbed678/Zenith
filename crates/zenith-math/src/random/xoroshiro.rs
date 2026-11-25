//! 128-bit `xoroshiro128++` random source and positional factory.

use crate::java::{D, F};
use crate::random::gaussian::MarsagliaPolarGaussian;
use crate::random::seed::{
    GOLDEN_RATIO_64, SILVER_RATIO_64, Seed128, position_hash, seed_from_name,
    upgrade_seed_to_128bit,
};
use crate::random::{PositionalRandomFactory, RandomSource};

/// 128-bit `xoroshiro128++` random number source.
///
/// Uses Blackman & Vigna's `xoroshiro128++` state transition, multiply-and-reject bounded ints,
/// top-bits float/double conversion (with [`D`](crate::java::D)), and replaces an all-zero
/// `(0, 0)` initial state with `(GOLDEN_RATIO_64, SILVER_RATIO_64)`.
#[derive(Debug, Clone)]
pub struct XoroshiroRandom {
    lo: u64,
    hi: u64,
    gaussian: MarsagliaPolarGaussian,
}

impl XoroshiroRandom {
    /// Creates a new `XoroshiroRandom` from a 64-bit seed using [`upgrade_seed_to_128bit`].
    #[inline]
    pub fn new(seed: i64) -> Self {
        Self::from_seed_128(upgrade_seed_to_128bit(seed))
    }

    /// Creates a new `XoroshiroRandom` directly from a 128-bit seed `(lo, hi)`.
    ///
    /// If both `lo` and `hi` are zero, the state is replaced with `(GOLDEN_RATIO_64,
    /// SILVER_RATIO_64)`.
    #[inline]
    pub const fn from_seed_128(seed: Seed128) -> Self {
        Self::from_raw_state(seed.lo, seed.hi)
    }

    /// Creates a new `XoroshiroRandom` directly from raw 64-bit halves `(lo, hi)`.
    ///
    /// If both `lo` and `hi` are zero, the state is replaced with `(GOLDEN_RATIO_64,
    /// SILVER_RATIO_64)`.
    #[inline]
    pub const fn from_raw_state(lo: i64, hi: i64) -> Self {
        let (lo, hi) = if (lo | hi) == 0 {
            (GOLDEN_RATIO_64 as u64, SILVER_RATIO_64 as u64)
        } else {
            (lo as u64, hi as u64)
        };
        Self {
            lo,
            hi,
            gaussian: MarsagliaPolarGaussian::new(),
        }
    }

    /// Returns the current 128-bit state `(lo, hi)`.
    #[inline]
    pub const fn state(&self) -> Seed128 {
        Seed128 {
            lo: self.lo as i64,
            hi: self.hi as i64,
        }
    }

    #[inline]
    fn step_parts(lo: &mut u64, hi: &mut u64) -> u64 {
        let s0 = *lo;
        let s1 = *hi;
        let result = s0.wrapping_add(s1).rotate_left(17).wrapping_add(s0);
        let s1 = s1 ^ s0;
        *lo = s0.rotate_left(49) ^ s1 ^ (s1 << 21);
        *hi = s1.rotate_left(28);
        result
    }

    #[inline]
    fn next_u64(&mut self) -> u64 {
        Self::step_parts(&mut self.lo, &mut self.hi)
    }
}

impl RandomSource for XoroshiroRandom {
    type Positional = XoroshiroPositionalRandomFactory;

    #[inline]
    fn set_seed(&mut self, seed: i64) {
        let upgraded = upgrade_seed_to_128bit(seed);
        let (lo, hi) = if (upgraded.lo | upgraded.hi) == 0 {
            (GOLDEN_RATIO_64 as u64, SILVER_RATIO_64 as u64)
        } else {
            (upgraded.lo as u64, upgraded.hi as u64)
        };
        self.lo = lo;
        self.hi = hi;
        self.gaussian.reset();
    }

    #[inline]
    fn next_int(&mut self) -> i32 {
        self.next_u64() as i32
    }

    #[inline]
    fn next_int_bounded(&mut self, bound: i32) -> i32 {
        assert!(bound > 0, "bound must be positive, got {bound}");

        let bound_u64 = bound as u64;
        let mut u = (self.next_int() as u32) as u64;
        let mut p = u * bound_u64;
        let mut frac = p & 0xFFFF_FFFF;
        if frac < bound_u64 {
            let threshold = ((1u64 << 32) - bound_u64) % bound_u64;
            while frac < threshold {
                u = (self.next_int() as u32) as u64;
                p = u * bound_u64;
                frac = p & 0xFFFF_FFFF;
            }
        }
        (p >> 32) as i32
    }

    #[inline]
    fn next_long(&mut self) -> i64 {
        self.next_u64() as i64
    }

    #[inline]
    fn next_boolean(&mut self) -> bool {
        (self.next_u64() & 1) != 0
    }

    #[inline]
    fn next_float(&mut self) -> f32 {
        let top24 = self.next_u64() >> 40;
        (top24 as f32) * F
    }

    #[inline]
    fn next_double(&mut self) -> f64 {
        let top53 = self.next_u64() >> 11;
        (top53 as f64) * D
    }

    #[inline]
    fn next_gaussian(&mut self) -> f64 {
        let lo = &mut self.lo;
        let hi = &mut self.hi;
        self.gaussian.next_gaussian(|| {
            let top53 = Self::step_parts(lo, hi) >> 11;
            (top53 as f64) * D
        })
    }

    #[inline]
    fn fork(&mut self) -> Self {
        let lo = self.next_long();
        let hi = self.next_long();
        Self::from_raw_state(lo, hi)
    }

    #[inline]
    fn fork_positional(&mut self) -> Self::Positional {
        let lo = self.next_long();
        let hi = self.next_long();
        XoroshiroPositionalRandomFactory::new(lo, hi)
    }

    #[inline]
    fn consume_count(&mut self, count: usize) {
        for _ in 0..count {
            let _ = self.next_u64();
        }
    }
}

/// Positional random factory for [`XoroshiroRandom`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct XoroshiroPositionalRandomFactory {
    lo: i64,
    hi: i64,
}

impl XoroshiroPositionalRandomFactory {
    /// Creates a new xoroshiro positional factory with base state `(lo, hi)`.
    #[inline]
    pub const fn new(lo: i64, hi: i64) -> Self {
        Self { lo, hi }
    }

    /// Creates a new xoroshiro positional factory from a [`Seed128`].
    #[inline]
    pub const fn from_seed_128(seed: Seed128) -> Self {
        Self {
            lo: seed.lo,
            hi: seed.hi,
        }
    }

    /// Returns the factory's 128-bit base seed `(lo, hi)`.
    #[inline]
    pub const fn state(&self) -> Seed128 {
        Seed128 {
            lo: self.lo,
            hi: self.hi,
        }
    }
}

impl PositionalRandomFactory for XoroshiroPositionalRandomFactory {
    type Random = XoroshiroRandom;

    #[inline]
    fn at(&self, x: i32, y: i32, z: i32) -> Self::Random {
        XoroshiroRandom::from_raw_state(position_hash(x, y, z) ^ self.lo, self.hi)
    }

    #[inline]
    fn from_hash_of(&self, name: &str) -> Self::Random {
        let name_seed = seed_from_name(name);
        XoroshiroRandom::from_raw_state(name_seed.lo ^ self.lo, name_seed.hi ^ self.hi)
    }

    #[inline]
    fn from_seed(&self, seed: i64) -> Self::Random {
        XoroshiroRandom::from_raw_state(seed ^ self.lo, seed ^ self.hi)
    }
}
