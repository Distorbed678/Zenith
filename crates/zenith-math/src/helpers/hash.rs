//! Hashing, mixing, and bounded RNG sampling helpers (`Mth`, `LinearCongruentialGenerator`,
//! `ChunkPos`, `Vec3i`).
//!
//! Implements Section 6 of `docs/specs/math/java-math-helpers.md`. Core RNG sources and
//! `Mth.getSeed(x, y, z)` (`position_hash`) live in [`crate::random`].

use crate::helpers::rounding::floor_f64;
use crate::java::{mul_i32, ushr_i32};
use crate::random::{LegacyRandom, RandomSource};

pub use crate::random::position_hash as get_seed;

/// MurmurHash3 `fmix32` first multiplier (`-2048144789`, `0x85EBCA6B`).
pub const MURMUR3_M1: i32 = 0x85EB_CA6B_u32 as i32;

/// MurmurHash3 `fmix32` second multiplier (`-1028477387`, `0xC2B2AE35`).
pub const MURMUR3_M2: i32 = 0xC2B2_AE35_u32 as i32;

/// `LinearCongruentialGenerator` multiplier `A = 6364136223846793005` (`0x5851F42D4C957F2D`).
pub const LCG_MULTIPLIER: i64 = 0x5851_F42D_4C95_7F2D;

/// `LinearCongruentialGenerator` addend `B = 1442695040888963407` (`0x14057B7EF767814F`).
pub const LCG_INCREMENT: i64 = 0x1405_7B7E_F767_814F;

/// `ChunkPos.hash` multiplier `1664525` (`0x0019660D`).
pub const CHUNK_HASH_A: i32 = 0x0019_660D;

/// `ChunkPos.hash` addend `1013904223` (`0x3C6EF35F`).
pub const CHUNK_HASH_C: i32 = 0x3C6E_F35F;

/// `ChunkPos.hash` Z XOR mask `-559038737` (`0xDEADBEEF`).
pub const CHUNK_HASH_Z_XOR: i32 = 0xDEAD_BEEF_u32 as i32;

/// `Mth.murmurHash3Mixer(int)`: MurmurHash3 32-bit finalizer (`fmix32`).
#[inline]
pub const fn murmur_hash3_mixer(mut h: i32) -> i32 {
    h ^= ushr_i32(h, 16);
    h = mul_i32(h, MURMUR3_M1);
    h ^= ushr_i32(h, 13);
    h = mul_i32(h, MURMUR3_M2);
    h ^ ushr_i32(h, 16)
}

/// `LinearCongruentialGenerator.next(long, long)`: quadratic 64-bit wrapping mixer
/// `r * (r * A + B) + c`.
#[inline]
pub const fn lcg_next(r: i64, c: i64) -> i64 {
    r.wrapping_mul(r.wrapping_mul(LCG_MULTIPLIER).wrapping_add(LCG_INCREMENT))
        .wrapping_add(c)
}

/// `ChunkPos.hash(int, int)`: wrapping 32-bit chunk coordinate hash.
#[inline]
pub const fn chunk_pos_hash(x: i32, z: i32) -> i32 {
    let a = x.wrapping_mul(CHUNK_HASH_A).wrapping_add(CHUNK_HASH_C);
    let b = (z ^ CHUNK_HASH_Z_XOR)
        .wrapping_mul(CHUNK_HASH_A)
        .wrapping_add(CHUNK_HASH_C);
    a ^ b
}

/// `Vec3i.hashCode()`: wrapping 32-bit `((y + z * 31) * 31) + x`.
#[inline]
pub const fn vec3i_hash_code(x: i32, y: i32, z: i32) -> i32 {
    y.wrapping_add(z.wrapping_mul(31))
        .wrapping_mul(31)
        .wrapping_add(x)
}

/// `Mth.nextInt(rng, min, max)`: returns `min` without advancing `rng` when `min >= max`,
/// otherwise returns `rng.next_int_bounded(max - min + 1) + min`.
#[inline]
pub fn next_int<R: RandomSource>(rng: &mut R, min: i32, max: i32) -> i32 {
    if min >= max {
        min
    } else {
        rng.next_int_bounded(max.wrapping_sub(min).wrapping_add(1))
            .wrapping_add(min)
    }
}

/// `Mth.nextFloat(rng, min, max)`: returns `min` without advancing `rng` when `min >= max`.
#[inline]
pub fn next_float<R: RandomSource>(rng: &mut R, min: f32, max: f32) -> f32 {
    if min >= max {
        min
    } else {
        rng.next_float() * (max - min) + min
    }
}

/// `Mth.nextDouble(rng, min, max)`: returns `min` without advancing `rng` when `min >= max`.
#[inline]
pub fn next_double<R: RandomSource>(rng: &mut R, min: f64, max: f64) -> f64 {
    if min >= max {
        min
    } else {
        rng.next_double() * (max - min) + min
    }
}

/// `Mth.randomBetweenInclusive(rng, min, max)`: unconditionally calls
/// `rng.next_int_bounded(max - min + 1) + min` (consumes one `next_int_bounded(1)` draw when
/// `min == max`).
#[inline]
pub fn random_between_inclusive<R: RandomSource>(rng: &mut R, min: i32, max: i32) -> i32 {
    rng.next_int_bounded(max.wrapping_sub(min).wrapping_add(1))
        .wrapping_add(min)
}

/// `Mth.randomBetween(rng, min, max)`: unconditionally draws `rng.next_float() * (max - min) + min`.
#[inline]
pub fn random_between<R: RandomSource>(rng: &mut R, min: f32, max: f32) -> f32 {
    rng.next_float() * (max - min) + min
}

/// `Mth.normal(rng, mean, dev)`: `mean + f32(rng.next_gaussian()) * dev`.
#[inline]
pub fn normal<R: RandomSource>(rng: &mut R, mean: f32, dev: f32) -> f32 {
    mean + (rng.next_gaussian() as f32) * dev
}

/// `Mth.wobble(double)`: deterministic coordinate wobble using a fresh [`LegacyRandom`] seeded
/// with `sign_extend_i64(floor_f64(coord * 3000.0))`.
#[inline]
pub fn wobble(coord: f64) -> f64 {
    let seed = i64::from(floor_f64(coord * 3000.0_f64));
    let mut rng = LegacyRandom::new(seed);
    let u = rng.next_double();
    coord + (((2.0_f64 * u - 1.0_f64) * 1.0e-7_f64) / 2.0_f64)
}

/// 128-bit UUID pair `(most_significant_bits, least_significant_bits)` matching `java.util.UUID`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Uuid128 {
    /// Most significant 64 bits (version nibble in bits `[15:12]`).
    pub most: i64,
    /// Least significant 64 bits (RFC 4122 variant in bits `[63:62]`).
    pub least: i64,
}

impl Uuid128 {
    /// Encodes the UUID in big-endian RFC 4122 byte order.
    #[inline]
    pub const fn to_be_bytes(self) -> [u8; 16] {
        let m = self.most.to_be_bytes();
        let l = self.least.to_be_bytes();
        [
            m[0], m[1], m[2], m[3], m[4], m[5], m[6], m[7], l[0], l[1], l[2], l[3], l[4], l[5],
            l[6], l[7],
        ]
    }
}

/// `Mth.createInsecureUUID(rng)`: draws two `next_long()` values in `(most, least)` order and
/// stamps version 4 + RFC 4122 variant bits `10`.
#[inline]
pub fn create_insecure_uuid<R: RandomSource>(rng: &mut R) -> Uuid128 {
    let m = rng.next_long();
    let l = rng.next_long();
    let most = (m & !0xF000_i64) | 0x4000_i64;
    let least = (l & 0x3FFF_FFFF_FFFF_FFFF_i64) | (0x8000_0000_0000_0000_u64 as i64);
    Uuid128 { most, least }
}
