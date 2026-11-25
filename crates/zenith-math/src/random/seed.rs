//! Seed derivation helpers: Mix13, 128-bit upgrade, MD5 name seeding, and position hashing.

use md5::{Digest, Md5};

/// Golden ratio 64-bit constant `G = 0x9E3779B97F4A7C15`.
pub const GOLDEN_RATIO_64: i64 = 0x9E37_79B9_7F4A_7C15_u64 as i64;

/// Silver ratio 64-bit constant `S = 0x6A09E667F3BCC909`.
pub const SILVER_RATIO_64: i64 = 0x6A09_E667_F3BC_C909_u64 as i64;

/// A 128-bit seed represented as two signed 64-bit halves `(lo, hi)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Seed128 {
    /// Low 64-bit half (`s[0]` in xoroshiro128++).
    pub lo: i64,
    /// High 64-bit half (`s[1]` in xoroshiro128++).
    pub hi: i64,
}

impl Seed128 {
    /// Constructs a `Seed128` from `(lo, hi)`.
    #[inline]
    pub const fn new(lo: i64, hi: i64) -> Self {
        Self { lo, hi }
    }

    /// XORs both halves with the corresponding halves of `other`.
    #[inline]
    pub const fn xor(self, other: Self) -> Self {
        Self {
            lo: self.lo ^ other.lo,
            hi: self.hi ^ other.hi,
        }
    }
}

/// Stafford's SplitMix64 "Mix13" 64-bit finalizer.
///
/// Computes:
/// - `z = (z ^ (z >>> 30)) * 0xBF58476D1CE4E5B9`
/// - `z = (z ^ (z >>> 27)) * 0x94D049BB133111EB`
/// - `z ^ (z >>> 31)`
#[inline]
pub const fn mix13(seed: i64) -> i64 {
    let mut z = seed as u64;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (z ^ (z >> 31)) as i64
}

/// Upgrades a 64-bit seed to 128 bits without applying [`mix13`].
///
/// Computes `lo = seed ^ S` and `hi = lo.wrapping_add(G)`.
#[inline]
pub const fn upgrade_seed_to_128bit_unmixed(seed: i64) -> Seed128 {
    let lo = seed ^ SILVER_RATIO_64;
    let hi = lo.wrapping_add(GOLDEN_RATIO_64);
    Seed128 { lo, hi }
}

/// Upgrades a 64-bit seed to 128 bits and applies [`mix13`] to each half.
#[inline]
pub const fn upgrade_seed_to_128bit(seed: i64) -> Seed128 {
    let unmixed = upgrade_seed_to_128bit_unmixed(seed);
    Seed128 {
        lo: mix13(unmixed.lo),
        hi: mix13(unmixed.hi),
    }
}

/// Derives a 128-bit seed from the MD5 digest of `name`'s UTF-8 bytes.
///
/// The first 8 digest bytes (big-endian) become `lo`; the last 8 bytes (big-endian) become `hi`.
pub fn seed_from_name(name: &str) -> Seed128 {
    let digest = Md5::digest(name.as_bytes());
    let lo = i64::from_be_bytes([
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7],
    ]);
    let hi = i64::from_be_bytes([
        digest[8], digest[9], digest[10], digest[11], digest[12], digest[13], digest[14],
        digest[15],
    ]);
    Seed128 { lo, hi }
}

/// Computes the 3D integer position hash `h(x, y, z)`.
///
/// Preserves the deliberate 32-bit wrapping multiplication on `x * 3_129_871` before
/// sign-extending to 64 bits:
/// 1. `a = i64::from(x.wrapping_mul(3_129_871))`
/// 2. `b = i64::from(z).wrapping_mul(116_129_781)`
/// 3. `s = a ^ b ^ i64::from(y)`
/// 4. `s = s * s * 42_317_861 + s * 11` (64-bit wrapping)
/// 5. `s >> 16` (arithmetic shift)
#[inline]
pub const fn position_hash(x: i32, y: i32, z: i32) -> i64 {
    let a = x.wrapping_mul(3_129_871) as i64;
    let b = (z as i64).wrapping_mul(116_129_781);
    let s = a ^ b ^ (y as i64);
    let s = s
        .wrapping_mul(s)
        .wrapping_mul(42_317_861)
        .wrapping_add(s.wrapping_mul(11));
    s >> 16
}
