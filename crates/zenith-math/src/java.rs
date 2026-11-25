//! Helpers implementing explicit Java primitive semantics.
//!
//! Java `int` (`i32`) and `long` (`i64`) arithmetic is two's complement with wrapping overflow,
//! and shift distances are masked to the low 5 bits (`& 31`) for `int` or low 6 bits (`& 63`)
//! for `long`. `String.hashCode` operates over UTF-16 code units (`u16`).

/// Single-precision constant written `5.9604645e-8f` in Java, equal to `2^-24`.
pub const F: f32 = 5.960_464_5e-8_f32;

/// The single-precision constant written `1.110223e-16`, widened to `f64`.
///
/// The `f32` literal rounds to exactly `2^-53`, so `D == 2^-53` (`0x3CA0_0000_0000_0000`) and
/// `next_double` matches `java.util.Random`'s multiplier. (An earlier spec revision claimed the
/// opposite; it was corrected after this implementation's open question, see `random.md`.)
pub const D: f64 = 1.110_223e-16_f32 as f64;

/// Wrapping 32-bit addition (Java `int + int`).
#[inline]
pub const fn add_i32(a: i32, b: i32) -> i32 {
    a.wrapping_add(b)
}

/// Wrapping 32-bit subtraction (Java `int - int`).
#[inline]
pub const fn sub_i32(a: i32, b: i32) -> i32 {
    a.wrapping_sub(b)
}

/// Wrapping 32-bit multiplication (Java `int * int`).
#[inline]
pub const fn mul_i32(a: i32, b: i32) -> i32 {
    a.wrapping_mul(b)
}

/// Wrapping 64-bit addition (Java `long + long`).
#[inline]
pub const fn add_i64(a: i64, b: i64) -> i64 {
    a.wrapping_add(b)
}

/// Wrapping 64-bit subtraction (Java `long - long`).
#[inline]
pub const fn sub_i64(a: i64, b: i64) -> i64 {
    a.wrapping_sub(b)
}

/// Wrapping 64-bit multiplication (Java `long * long`).
#[inline]
pub const fn mul_i64(a: i64, b: i64) -> i64 {
    a.wrapping_mul(b)
}

/// Java `int << shift` (shift distance masked by `& 31`).
#[inline]
pub const fn shl_i32(value: i32, shift: u32) -> i32 {
    value.wrapping_shl(shift)
}

/// Java `int >> shift` arithmetic right shift (shift distance masked by `& 31`).
#[inline]
pub const fn shr_i32(value: i32, shift: u32) -> i32 {
    value.wrapping_shr(shift)
}

/// Java `int >>> shift` logical right shift (shift distance masked by `& 31`).
#[inline]
pub const fn ushr_i32(value: i32, shift: u32) -> i32 {
    (value as u32).wrapping_shr(shift) as i32
}

/// Java `long << shift` (shift distance masked by `& 63`).
#[inline]
pub const fn shl_i64(value: i64, shift: u32) -> i64 {
    value.wrapping_shl(shift)
}

/// Java `long >> shift` arithmetic right shift (shift distance masked by `& 63`).
#[inline]
pub const fn shr_i64(value: i64, shift: u32) -> i64 {
    value.wrapping_shr(shift)
}

/// Java `long >>> shift` logical right shift (shift distance masked by `& 63`).
#[inline]
pub const fn ushr_i64(value: i64, shift: u32) -> i64 {
    (value as u64).wrapping_shr(shift) as i64
}

/// Java `Long.rotateLeft(value, distance)` (distance taken modulo 64).
#[inline]
pub const fn rotl_i64(value: i64, distance: u32) -> i64 {
    (value as u64).rotate_left(distance) as i64
}

/// Java `Long.rotateRight(value, distance)` (distance taken modulo 64).
#[inline]
pub const fn rotr_i64(value: i64, distance: u32) -> i64 {
    (value as u64).rotate_right(distance) as i64
}

/// Computes Java's `String.hashCode()` over a slice of UTF-16 code units.
///
/// Defined as `h = 31 * h + unit` in wrapping 32-bit signed arithmetic starting from `0`.
#[inline]
pub fn string_hash_code_utf16(units: &[u16]) -> i32 {
    let mut h: i32 = 0;
    for &unit in units {
        h = h.wrapping_mul(31).wrapping_add(i32::from(unit));
    }
    h
}

/// Computes Java's `String.hashCode()` for a UTF-8 Rust string by iterating its UTF-16 code units.
///
/// Non-BMP Unicode scalar values are encoded as surrogate pairs, matching Java `String` layout.
#[inline]
pub fn string_hash_code(s: &str) -> i32 {
    let mut h: i32 = 0;
    for unit in s.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(i32::from(unit));
    }
    h
}

/// Natural logarithm matching Java's `Math.log(double)`.
///
/// All Gaussian logarithm calls route through this single entry point so platform-specific
/// HotSpot intrinsic differences can be verified and handled in one place.
// PARITY: verify against oracle samples per platform (spec: Gaussian edge case)
#[inline]
pub fn java_math_log(x: f64) -> f64 {
    x.ln()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_constants_match_spec() {
        assert_eq!(F, 2f32.powi(-24));
        assert_eq!(D, 2f64.powi(-53));
        assert_eq!(F.to_bits(), 0x3380_0000);
        assert_eq!(D.to_bits(), 0x3CA0_0000_0000_0000);
        assert_eq!(
            (1.110_223e-16_f32 as f64).to_bits(),
            2f64.powi(-53).to_bits()
        );
    }

    #[test]
    fn wrapping_int_and_long_ops() {
        assert_eq!(add_i32(i32::MAX, 1), i32::MIN);
        assert_eq!(sub_i32(i32::MIN, 1), i32::MAX);
        assert_eq!(mul_i32(687, 3_129_871), -2_144_745_919);

        assert_eq!(add_i64(i64::MAX, 1), i64::MIN);
        assert_eq!(sub_i64(i64::MIN, 1), i64::MAX);
        assert_eq!(mul_i64(i64::MAX, 2), -2);
    }

    #[test]
    fn shifts_follow_java_masking_and_sign_rules() {
        assert_eq!(ushr_i32(-1, 1), i32::MAX);
        assert_eq!(shr_i32(-1, 1), -1);
        assert_eq!(ushr_i32(-1, 32), -1);
        assert_eq!(shl_i32(1, 31), i32::MIN);
        assert_eq!(shl_i32(1, 32), 1);

        assert_eq!(ushr_i64(-1, 1), i64::MAX);
        assert_eq!(shr_i64(-1, 1), -1);
        assert_eq!(ushr_i64(-1, 64), -1);
        assert_eq!(shl_i64(1, 63), i64::MIN);
        assert_eq!(shl_i64(1, 64), 1);

        assert_eq!(rotl_i64(1, 17), 1 << 17);
        assert_eq!(rotl_i64(i64::MIN, 1), 1);
        assert_eq!(rotr_i64(1, 1), i64::MIN);
    }

    #[test]
    fn string_hash_code_matches_java_semantics() {
        assert_eq!(string_hash_code(""), 0);
        assert_eq!(string_hash_code("a"), 97);
        assert_eq!(string_hash_code("hello"), 99_162_322);
        // Classic Java String.hashCode() == Integer.MIN_VALUE example.
        assert_eq!(string_hash_code("polygenelubricants"), i32::MIN);
        // Non-BMP character uses UTF-16 surrogate pair (0xD83D, 0xDE00 for U+1F600).
        let emoji_units: Vec<u16> = "😀".encode_utf16().collect();
        assert_eq!(emoji_units, vec![0xD83D, 0xDE00]);
        let expected = (0xD83D_i32).wrapping_mul(31).wrapping_add(0xDE00);
        assert_eq!(string_hash_code("😀"), expected);
        assert_eq!(string_hash_code_utf16(&emoji_units), expected);
    }
}
