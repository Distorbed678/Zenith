//! Seeded 3D Voronoi biome zoom (`BiomeManager`, spec §5).
//!
//! Includes a self-contained FIPS 180-4 SHA-256 implementation for [`obfuscate_seed`] matching
//! Guava's `Hashing.sha256().hashLong(seed).asLong()` (little-endian input and little-endian
//! output).

use zenith_math::helpers::hash::lcg_next;
use zenith_math::helpers::rounding::square_f64;
use zenith_registry::Identifier;

/// Initial SHA-256 hash values `H^(0)` (FIPS 180-4 §5.3.3).
const SHA256_IV: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

/// SHA-256 round constants `K_0..K_63` (FIPS 180-4 §4.2.2).
const SHA256_K: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

/// Compresses one 64-byte SHA-256 block into `state` (FIPS 180-4 §6.2.2).
fn sha256_compress(state: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0u32; 64];
    for (i, &chunk) in block.as_chunks::<4>().0.iter().enumerate() {
        w[i] = u32::from_be_bytes(chunk);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }

    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ ((!e) & g);
        let temp1 = h
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(SHA256_K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let temp2 = s0.wrapping_add(maj);

        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(temp1);
        d = c;
        c = b;
        b = a;
        a = temp1.wrapping_add(temp2);
    }

    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);
}

/// Computes the SHA-256 digest of an 8-byte message (single 64-byte padded block).
fn sha256_8_bytes(msg: [u8; 8]) -> [u8; 32] {
    let mut block = [0u8; 64];
    block[0..8].copy_from_slice(&msg);
    block[8] = 0x80;
    // Message length in bits = 8 * 8 = 64, stored as big-endian u64 in bytes 56..64.
    block[56..64].copy_from_slice(&64u64.to_be_bytes());

    let mut state = SHA256_IV;
    sha256_compress(&mut state, &block);

    let mut digest = [0u8; 32];
    for (chunk, word) in digest.as_chunks_mut::<4>().0.iter_mut().zip(state) {
        *chunk = word.to_be_bytes();
    }
    digest
}

/// Computes `BiomeManager::obfuscateSeed(worldSeed)` (spec §5).
///
/// Serializes `seed` as 8 little-endian bytes, computes SHA-256, and interprets the first
/// 8 digest bytes as a little-endian signed `i64` (matching Guava's
/// `Hashing.sha256().hashLong(seed).asLong()`).
#[inline]
pub fn obfuscate_seed(seed: i64) -> i64 {
    let digest = sha256_8_bytes(seed.to_le_bytes());
    i64::from_le_bytes([
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7],
    ])
}

/// Jitter function `BiomeManager::getFiddle(long)` (spec §5):
/// `((floorMod(r >> 24, 1024) as f64 / 1024.0) - 0.5) * 0.9`.
#[inline]
pub fn fiddle(r: i64) -> f64 {
    // 1024 = 2^10, so floorMod(r >> 24, 1024) == ((r >> 24) & 1023) for all signed i64.
    let m = ((r >> 24) & 1023) as i32;
    ((f64::from(m) / 1024.0_f64) - 0.5_f64) * 0.9_f64
}

/// Computes `BiomeManager::getFiddledDistance(seed, cx, cy, cz, dx, dy, dz)` (spec §5).
///
/// Uses the quadratic 64-bit LCG mixer from `zenith_math::helpers::hash::lcg_next` and sums
/// squared components in exact **Z, then Y, then X** left-to-right `f64` order (no FMA):
/// `((dz + fz)^2 + (dy + fy)^2) + (dx + fx)^2`.
#[inline]
pub fn fiddled_distance(seed: i64, cx: i32, cy: i32, cz: i32, dx: f64, dy: f64, dz: f64) -> f64 {
    let cx = i64::from(cx);
    let cy = i64::from(cy);
    let cz = i64::from(cz);

    let mut r = seed;
    r = lcg_next(r, cx);
    r = lcg_next(r, cy);
    r = lcg_next(r, cz);
    r = lcg_next(r, cx);
    r = lcg_next(r, cy);
    r = lcg_next(r, cz);
    let fx = fiddle(r);

    r = lcg_next(r, seed);
    let fy = fiddle(r);

    r = lcg_next(r, seed);
    let fz = fiddle(r);

    (square_f64(dz + fz) + square_f64(dy + fy)) + square_f64(dx + fx)
}

/// Resolves the winning quart coordinate `(quart_x, quart_y, quart_z)` for block `(x, y, z)`
/// using the exact 8-corner Voronoi biome zoom (spec §5).
///
/// Exact distance ties keep the smaller corner bitmask `i ∈ 0..8` (strict `min_dist > d`).
#[inline]
pub fn zoomed_quart_pos(obfuscated_seed: i64, x: i32, y: i32, z: i32) -> (i32, i32, i32) {
    let abs_x = x.wrapping_sub(2);
    let abs_y = y.wrapping_sub(2);
    let abs_z = z.wrapping_sub(2);

    let parent_x = abs_x >> 2;
    let parent_y = abs_y >> 2;
    let parent_z = abs_z >> 2;

    let fract_x = f64::from(abs_x & 3) / 4.0_f64;
    let fract_y = f64::from(abs_y & 3) / 4.0_f64;
    let fract_z = f64::from(abs_z & 3) / 4.0_f64;

    let mut min_i: i32 = 0;
    let mut min_dist: f64 = f64::INFINITY;

    for i in 0..8_i32 {
        let x_even = (i & 4) == 0;
        let y_even = (i & 2) == 0;
        let z_even = (i & 1) == 0;

        let corner_x = if x_even {
            parent_x
        } else {
            parent_x.wrapping_add(1)
        };
        let corner_y = if y_even {
            parent_y
        } else {
            parent_y.wrapping_add(1)
        };
        let corner_z = if z_even {
            parent_z
        } else {
            parent_z.wrapping_add(1)
        };

        let dx = if x_even { fract_x } else { fract_x - 1.0_f64 };
        let dy = if y_even { fract_y } else { fract_y - 1.0_f64 };
        let dz = if z_even { fract_z } else { fract_z - 1.0_f64 };

        let d = fiddled_distance(obfuscated_seed, corner_x, corner_y, corner_z, dx, dy, dz);
        if min_dist > d {
            min_i = i;
            min_dist = d;
        }
    }

    let biome_x = if (min_i & 4) == 0 {
        parent_x
    } else {
        parent_x.wrapping_add(1)
    };
    let biome_y = if (min_i & 2) == 0 {
        parent_y
    } else {
        parent_y.wrapping_add(1)
    };
    let biome_z = if (min_i & 1) == 0 {
        parent_z
    } else {
        parent_z.wrapping_add(1)
    };
    (biome_x, biome_y, biome_z)
}

/// Resolves the zoomed biome at **block** coordinates `(x, y, z)` using `obfuscated_seed`
/// (from [`obfuscate_seed`]) and a quart-resolution `noise_biome` lookup callback (spec §5).
#[inline]
pub fn zoomed_biome(
    obfuscated_seed: i64,
    x: i32,
    y: i32,
    z: i32,
    noise_biome: &mut dyn FnMut(i32, i32, i32) -> Identifier,
) -> Identifier {
    let (qx, qy, qz) = zoomed_quart_pos(obfuscated_seed, x, y, z);
    noise_biome(qx, qy, qz)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obfuscate_seed_matches_spec_test_vectors() {
        // Exact reference vectors from docs/specs/worldgen/biomes-and-climate.md ("Constants and IDs").
        assert_eq!(obfuscate_seed(0), 8_794_265_229_978_523_055_i64);
        assert_eq!(obfuscate_seed(0) as u64, 0x7A0B_81A1_F570_55AF);

        assert_eq!(obfuscate_seed(1), -6_467_378_160_175_308_932_i64);
        assert_eq!(obfuscate_seed(1) as u64, 0xA63F_41D4_36A1_9F7C);

        assert_eq!(obfuscate_seed(-1), 6_759_447_113_877_070_610_i64);
        assert_eq!(obfuscate_seed(-1) as u64, 0x5DCE_6156_44AE_A312);

        assert_eq!(
            obfuscate_seed(1_234_567_890_123_456_789_i64),
            7_890_956_078_138_091_352_i64
        );
        assert_eq!(
            obfuscate_seed(1_234_567_890_123_456_789_i64) as u64,
            0x6D82_4EBE_B7A1_6758
        );
    }

    #[test]
    fn fiddle_bounds_and_signed_floor_mod() {
        // Minimum: ((r >> 24) & 1023) == 0 => -0.5 * 0.9 = -0.45
        assert_eq!(fiddle(0), -0.45_f64);
        // Maximum: ((r >> 24) & 1023) == 1023 => (1023/1024 - 0.5) * 0.9 = 0.44912109375
        assert_eq!(
            fiddle(-1_i64),
            ((1023.0_f64 / 1024.0_f64) - 0.5_f64) * 0.9_f64
        );
        assert_eq!(fiddle(1023_i64 << 24), fiddle(-1_i64));
        // Midpoint: 512 => 0.0
        assert_eq!(fiddle(512_i64 << 24), 0.0_f64);
    }

    #[test]
    fn zoomed_biome_selects_surrounding_quart_and_calls_callback_once() {
        let obf = obfuscate_seed(42);
        for &(x, y, z) in &[
            (0, 64, 0),
            (-1, 0, -1),
            (15, -64, 15),
            (-100, 319, 250),
            (2, 2, 2),
        ] {
            let mut calls = 0usize;
            let expected = zoomed_quart_pos(obf, x, y, z);
            // Winning quart must be one of the 8 corners around ((coord - 2) >> 2).
            let px = x.wrapping_sub(2) >> 2;
            let py = y.wrapping_sub(2) >> 2;
            let pz = z.wrapping_sub(2) >> 2;
            assert!(expected.0 == px || expected.0 == px + 1);
            assert!(expected.1 == py || expected.1 == py + 1);
            assert!(expected.2 == pz || expected.2 == pz + 1);

            let id = zoomed_biome(obf, x, y, z, &mut |qx, qy, qz| {
                calls += 1;
                assert_eq!((qx, qy, qz), expected);
                Identifier::with_default_namespace("plains").unwrap()
            });
            assert_eq!(calls, 1);
            assert_eq!(id, Identifier::with_default_namespace("plains").unwrap());
        }
    }
}
