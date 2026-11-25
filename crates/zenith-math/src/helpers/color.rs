//! 32-bit ARGB color operations, 10-bit sRGB/linear lookup tables, HSV conversion, and
//! packed light coordinates (`ARGB`, `Mth.hsvToRgb`/`hsvToArgb`, `LightCoordsUtil`).
//!
//! Implements Section 7 of `docs/specs/math/java-math-helpers.md`.

use std::sync::LazyLock;

use crate::helpers::interp::lerp_int;
use crate::helpers::intrinsics::java_math_pow;
use crate::helpers::rounding::{
    clamp_i32, floor_f32, java_clamp_i32, java_clamp_i64, java_round_f32,
};
use crate::java::ushr_i32;

/// sRGB-to-linear knee threshold `0.04045_f32` (`0x3D25AEE6`).
pub const SRGB_TO_LINEAR_THRESHOLD: f32 = f32::from_bits(0x3D25_AEE6);

/// Linear-to-sRGB knee threshold `0.0031308_f32` (`0x3B4D2E1C`).
pub const LINEAR_TO_SRGB_THRESHOLD: f32 = f32::from_bits(0x3B4D_2E1C);

/// Linear-to-sRGB exponent `1.0 / 2.4 = 0.4166666666666667` (`0x3FDAAAAAAAAAAAAB`).
pub const LINEAR_TO_SRGB_EXPONENT: f64 = 1.0_f64 / 2.4_f64;

/// Greyscale red weight `0.3_f32` (`0x3E99999A`).
pub const GREYSCALE_WEIGHT_R: f32 = f32::from_bits(0x3E99_999A);

/// Greyscale green weight `0.59_f32` (`0x3F170A3D`).
pub const GREYSCALE_WEIGHT_G: f32 = f32::from_bits(0x3F17_0A3D);

/// Greyscale blue weight `0.11_f32` (`0x3DE147AE`).
pub const GREYSCALE_WEIGHT_B: f32 = f32::from_bits(0x3DE1_47AE);

/// `LightCoordsUtil.FULL_BRIGHT = pack(15, 15) = 0x00F000F0` (`15728880`).
pub const FULL_BRIGHT: i32 = 0x00F0_00F0;

/// `LightCoordsUtil.FULL_SKY = pack(0, 15) = 0x00F00000` (`15728640`).
pub const FULL_SKY: i32 = 0x00F0_0000;

static SRGB_TO_LINEAR: LazyLock<[i16; 256]> = LazyLock::new(|| {
    let mut table = [0_i16; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let c = (i as f32) / 255.0_f32;
        let t = if c >= SRGB_TO_LINEAR_THRESHOLD {
            java_math_pow((f64::from(c) + 0.055_f64) / 1.055_f64, 2.4_f64) as f32
        } else {
            c / 12.92_f32
        };
        *slot = java_round_f32(t * 1023.0_f32) as i16;
    }
    table
});

static LINEAR_TO_SRGB: LazyLock<[u8; 1024]> = LazyLock::new(|| {
    let mut table = [0_u8; 1024];
    for (i, slot) in table.iter_mut().enumerate() {
        let c = (i as f32) / 1023.0_f32;
        let t = if c >= LINEAR_TO_SRGB_THRESHOLD {
            (1.055_f64 * java_math_pow(f64::from(c), LINEAR_TO_SRGB_EXPONENT) - 0.055_f64) as f32
        } else {
            12.92_f32 * c
        };
        *slot = java_round_f32(t * 255.0_f32) as u8;
    }
    table
});

/// Returns a reference to the lazily initialized 256-entry 10-bit `SRGB_TO_LINEAR` table.
#[inline]
pub fn srgb_to_linear_table() -> &'static [i16; 256] {
    &SRGB_TO_LINEAR
}

/// Returns a reference to the lazily initialized 1024-entry `LINEAR_TO_SRGB` table (unsigned
/// byte values `0..=255`).
#[inline]
pub fn linear_to_srgb_table() -> &'static [u8; 1024] {
    &LINEAR_TO_SRGB
}

// ---------------------------------------------------------------------------
// Basic ARGB channel extraction and packing
// ---------------------------------------------------------------------------

/// `ARGB.alpha(int)`: `c >>> 24`.
#[inline]
pub const fn alpha(c: i32) -> i32 {
    ushr_i32(c, 24)
}

/// `ARGB.red(int)`: `(c >> 16) & 0xFF`.
#[inline]
pub const fn red(c: i32) -> i32 {
    (c >> 16) & 0xFF
}

/// `ARGB.green(int)`: `(c >> 8) & 0xFF`.
#[inline]
pub const fn green(c: i32) -> i32 {
    (c >> 8) & 0xFF
}

/// `ARGB.blue(int)`: `c & 0xFF`.
#[inline]
pub const fn blue(c: i32) -> i32 {
    c & 0xFF
}

/// `ARGB.alphaFloat(int)`: `f32(alpha(c)) / 255.0_f32`.
#[inline]
pub fn alpha_float(c: i32) -> f32 {
    (alpha(c) as f32) / 255.0_f32
}

/// `ARGB.redFloat(int)`: `f32(red(c)) / 255.0_f32`.
#[inline]
pub fn red_float(c: i32) -> f32 {
    (red(c) as f32) / 255.0_f32
}

/// `ARGB.greenFloat(int)`: `f32(green(c)) / 255.0_f32`.
#[inline]
pub fn green_float(c: i32) -> f32 {
    (green(c) as f32) / 255.0_f32
}

/// `ARGB.blueFloat(int)`: `f32(blue(c)) / 255.0_f32`.
#[inline]
pub fn blue_float(c: i32) -> f32 {
    (blue(c) as f32) / 255.0_f32
}

/// `ARGB.as8BitChannel(float)`: `floor_f32(v * 255.0_f32)` (unclamped).
#[inline]
pub fn as_8bit_channel(v: f32) -> i32 {
    floor_f32(v * 255.0_f32)
}

/// `ARGB.color(a, r, g, b)`: masks all four channels with `& 0xFF` before shifting.
#[inline]
pub const fn color_argb(a: i32, r: i32, g: i32, b: i32) -> i32 {
    ((a & 0xFF) << 24) | ((r & 0xFF) << 16) | ((g & 0xFF) << 8) | (b & 0xFF)
}

/// `ARGB.color(r, g, b)`: opaque ARGB (`a = 255`).
#[inline]
pub const fn color_rgb(r: i32, g: i32, b: i32) -> i32 {
    color_argb(255, r, g, b)
}

/// `ARGB.color(int a, int rgb)`: `(a << 24) | (rgb & 0xFFFFFF)`.
#[inline]
pub const fn color_with_alpha(a: i32, rgb: i32) -> i32 {
    (a << 24) | (rgb & 0x00FF_FFFF)
}

/// `ARGB.color(float a, int rgb)`: `color_with_alpha(as_8bit_channel(a), rgb)`.
#[inline]
pub fn color_with_alpha_float(a: f32, rgb: i32) -> i32 {
    color_with_alpha(as_8bit_channel(a), rgb)
}

/// `ARGB.white(int a)`: `(a << 24) | 0x00FFFFFF`.
#[inline]
pub const fn white(a: i32) -> i32 {
    (a << 24) | 0x00FF_FFFF
}

/// `ARGB.white(float a)`: `white(as_8bit_channel(a))`.
#[inline]
pub fn white_float(a: f32) -> i32 {
    white(as_8bit_channel(a))
}

/// `ARGB.black(int a)`: `a << 24`.
#[inline]
pub const fn black(a: i32) -> i32 {
    a << 24
}

/// `ARGB.black(float a)`: `black(as_8bit_channel(a))`.
#[inline]
pub fn black_float(a: f32) -> i32 {
    black(as_8bit_channel(a))
}

/// `ARGB.opaque(int)`: `c | 0xFF000000`.
#[inline]
pub const fn opaque(c: i32) -> i32 {
    c | (0xFF00_0000_u32 as i32)
}

/// `ARGB.transparent(int)`: `c & 0x00FFFFFF`.
#[inline]
pub const fn transparent(c: i32) -> i32 {
    c & 0x00FF_FFFF
}

/// `ARGB.toABGR(int)`: swaps the red and blue channels.
#[inline]
pub const fn to_abgr(c: i32) -> i32 {
    (c & (0xFF00_FF00_u32 as i32)) | ((c & 0x00FF_0000) >> 16) | ((c & 0x0000_00FF) << 16)
}

/// `ARGB.fromABGR(int)`: swaps the red and blue channels (involution of [`to_abgr`]).
#[inline]
pub const fn from_abgr(c: i32) -> i32 {
    to_abgr(c)
}

// ---------------------------------------------------------------------------
// 10-bit sRGB / linear helpers
// ---------------------------------------------------------------------------

/// `ARGB.srgbToLinearChannel(int)`: `f32(SRGB_TO_LINEAR[srgb]) / 1023.0_f32`.
///
/// # Panics
///
/// Panics if `srgb` is outside `0..=255`.
#[inline]
pub fn srgb_to_linear_channel(srgb: i32) -> f32 {
    f32::from(SRGB_TO_LINEAR[srgb as usize]) / 1023.0_f32
}

/// `ARGB.linearToSrgbChannel(float)`: `LINEAR_TO_SRGB[floor_f32(linear * 1023.0_f32)] & 0xFF`.
///
/// Uses `floor` (not `round`).
///
/// # Panics
///
/// Panics if the floored index falls outside `0..=1023`.
#[inline]
pub fn linear_to_srgb_channel(linear: f32) -> i32 {
    i32::from(LINEAR_TO_SRGB[floor_f32(linear * 1023.0_f32) as usize])
}

/// `ARGB.meanLinear(c1, c2, c3, c4)`: averages alpha linearly and RGB channels in 10-bit linear
/// space via integer `/ 4`.
#[inline]
pub fn mean_linear(c1: i32, c2: i32, c3: i32, c4: i32) -> i32 {
    let s2l = &*SRGB_TO_LINEAR;
    let l2s = &*LINEAR_TO_SRGB;
    let a = (alpha(c1) + alpha(c2) + alpha(c3) + alpha(c4)) / 4;
    let r = i32::from(
        l2s[((i32::from(s2l[red(c1) as usize])
            + i32::from(s2l[red(c2) as usize])
            + i32::from(s2l[red(c3) as usize])
            + i32::from(s2l[red(c4) as usize]))
            / 4) as usize],
    );
    let g = i32::from(
        l2s[((i32::from(s2l[green(c1) as usize])
            + i32::from(s2l[green(c2) as usize])
            + i32::from(s2l[green(c3) as usize])
            + i32::from(s2l[green(c4) as usize]))
            / 4) as usize],
    );
    let b = i32::from(
        l2s[((i32::from(s2l[blue(c1) as usize])
            + i32::from(s2l[blue(c2) as usize])
            + i32::from(s2l[blue(c3) as usize])
            + i32::from(s2l[blue(c4) as usize]))
            / 4) as usize],
    );
    color_argb(a, r, g, b)
}

/// `ARGB.linearLerp(alpha, p0, p1)`: interpolates alpha directly and RGB in 10-bit linear space
/// using [`lerp_int`].
#[inline]
pub fn linear_lerp(t: f32, p0: i32, p1: i32) -> i32 {
    let s2l = &*SRGB_TO_LINEAR;
    let l2s = &*LINEAR_TO_SRGB;
    let a = lerp_int(t, alpha(p0), alpha(p1));
    let r = i32::from(
        l2s[lerp_int(
            t,
            i32::from(s2l[red(p0) as usize]),
            i32::from(s2l[red(p1) as usize]),
        ) as usize],
    );
    let g = i32::from(
        l2s[lerp_int(
            t,
            i32::from(s2l[green(p0) as usize]),
            i32::from(s2l[green(p1) as usize]),
        ) as usize],
    );
    let b = i32::from(
        l2s[lerp_int(
            t,
            i32::from(s2l[blue(p0) as usize]),
            i32::from(s2l[blue(p1) as usize]),
        ) as usize],
    );
    color_argb(a, r, g, b)
}

/// `ARGB.srgbLerp(alpha, p0, p1)`: interpolates A, R, G, B directly using [`lerp_int`].
#[inline]
pub fn srgb_lerp(t: f32, p0: i32, p1: i32) -> i32 {
    color_argb(
        lerp_int(t, alpha(p0), alpha(p1)),
        lerp_int(t, red(p0), red(p1)),
        lerp_int(t, green(p0), green(p1)),
        lerp_int(t, blue(p0), blue(p1)),
    )
}

// ---------------------------------------------------------------------------
// ARGB arithmetic, blending, and HSV conversion
// ---------------------------------------------------------------------------

/// `ARGB.multiply(lhs, rhs)`: short-circuits on `-1` (`0xFFFFFFFF`), otherwise `(c1 * c2) / 255`
/// per channel.
#[inline]
pub const fn multiply(lhs: i32, rhs: i32) -> i32 {
    if lhs == -1 {
        return rhs;
    }
    if rhs == -1 {
        return lhs;
    }
    color_argb(
        (alpha(lhs) * alpha(rhs)) / 255,
        (red(lhs) * red(rhs)) / 255,
        (green(lhs) * green(rhs)) / 255,
        (blue(lhs) * blue(rhs)) / 255,
    )
}

/// `ARGB.addRgb(lhs, rhs)`: preserves `alpha(lhs)` and clamps each RGB channel sum to `<= 255`.
#[inline]
pub const fn add_rgb(lhs: i32, rhs: i32) -> i32 {
    let r = red(lhs) + red(rhs);
    let g = green(lhs) + green(rhs);
    let b = blue(lhs) + blue(rhs);
    color_argb(
        alpha(lhs),
        if r > 255 { 255 } else { r },
        if g > 255 { 255 } else { g },
        if b > 255 { 255 } else { b },
    )
}

/// `ARGB.subtractRgb(lhs, rhs)`: preserves `alpha(lhs)` and clamps each RGB difference to `>= 0`.
#[inline]
pub const fn subtract_rgb(lhs: i32, rhs: i32) -> i32 {
    let r = red(lhs) - red(rhs);
    let g = green(lhs) - green(rhs);
    let b = blue(lhs) - blue(rhs);
    color_argb(
        alpha(lhs),
        if r < 0 { 0 } else { r },
        if g < 0 { 0 } else { g },
        if b < 0 { 0 } else { b },
    )
}

/// `ARGB.multiplyAlpha(color, mult)`: returns `0` (clearing RGB too!) if `c == 0 || m <= 0.0`,
/// returns `c` if `m >= 1.0`, else `color_with_alpha_float(alpha_float(c) * m, c)`.
#[inline]
pub fn multiply_alpha(c: i32, m: f32) -> i32 {
    if c == 0 || m <= 0.0_f32 {
        0
    } else if m >= 1.0_f32 {
        c
    } else {
        color_with_alpha_float(alpha_float(c) * m, c)
    }
}

/// `ARGB.scaleRGB(color, sr, sg, sb)`: keeps alpha; each RGB channel is
/// `Math.clamp(trunc_i32(f32(ch) * scale), 0, 255)`.
#[inline]
pub fn scale_rgb_f32(c: i32, sr: f32, sg: f32, sb: f32) -> i32 {
    color_argb(
        alpha(c),
        java_clamp_i32(((red(c) as f32) * sr) as i32, 0, 255),
        java_clamp_i32(((green(c) as f32) * sg) as i32, 0, 255),
        java_clamp_i32(((blue(c) as f32) * sb) as i32, 0, 255),
    )
}

/// `ARGB.scaleRGB(color, scale: i32)`: keeps alpha; each RGB channel is
/// `Math.clamp((i64(ch) * i64(scale)) / 255L, 0, 255)`.
#[inline]
pub fn scale_rgb_i32(c: i32, scale: i32) -> i32 {
    let s = i64::from(scale);
    color_argb(
        alpha(c),
        java_clamp_i64((i64::from(red(c)).wrapping_mul(s)) / 255_i64, 0, 255) as i32,
        java_clamp_i64((i64::from(green(c)).wrapping_mul(s)) / 255_i64, 0, 255) as i32,
        java_clamp_i64((i64::from(blue(c)).wrapping_mul(s)) / 255_i64, 0, 255) as i32,
    )
}

/// `ARGB.greyscale(int)`: `trunc_i32((f32(r) * 0.3f + f32(g) * 0.59f) + f32(b) * 0.11f)`,
/// preserving alpha.
#[inline]
pub fn greyscale(c: i32) -> i32 {
    let g = (((red(c) as f32) * GREYSCALE_WEIGHT_R + (green(c) as f32) * GREYSCALE_WEIGHT_G)
        + (blue(c) as f32) * GREYSCALE_WEIGHT_B) as i32;
    color_argb(alpha(c), g, g, g)
}

/// `ARGB.alphaBlend(dst, src)`: Porter-Duff source-over integer blend.
#[inline]
pub const fn alpha_blend(dst: i32, src: i32) -> i32 {
    let a_s = alpha(src);
    if a_s == 255 {
        return src;
    }
    if a_s == 0 {
        return dst;
    }
    let a_d = alpha(dst);
    let a = a_s + (a_d * (255 - a_s)) / 255;
    let rem = a - a_s;
    color_argb(
        a,
        (red(src) * a_s + red(dst) * rem) / a,
        (green(src) * a_s + green(dst) * rem) / a,
        (blue(src) * a_s + blue(dst) * rem) / a,
    )
}

/// `ARGB.average(lhs, rhs)`: per-channel `(c1 + c2) / 2`.
#[inline]
pub const fn average(lhs: i32, rhs: i32) -> i32 {
    color_argb(
        (alpha(lhs) + alpha(rhs)) / 2,
        (red(lhs) + red(rhs)) / 2,
        (green(lhs) + green(rhs)) / 2,
        (blue(lhs) + blue(rhs)) / 2,
    )
}

/// `ARGB.setBrightness(color, brightness)`: converts RGB to HSV in `f32`, replaces `V` with
/// `brightness`, converts back to RGB with `Math.round(ch * 255.0_f32)`, preserving alpha.
#[inline]
pub fn set_brightness(c: i32, brightness: f32) -> i32 {
    let a = alpha(c);
    let r = red(c);
    let g = green(c);
    let b = blue(c);
    let max_c = r.max(g).max(b);
    let min_c = r.min(g).min(b);
    let delta = (max_c - min_c) as f32;
    let s = if max_c != 0 {
        delta / (max_c as f32)
    } else {
        0.0_f32
    };

    if s == 0.0_f32 {
        let ch = java_round_f32(brightness * 255.0_f32);
        return color_argb(a, ch, ch, ch);
    }

    let mut h = if r == max_c {
        ((g - b) as f32) / delta
    } else if g == max_c {
        2.0_f32 + ((b - r) as f32) / delta
    } else {
        4.0_f32 + ((r - g) as f32) / delta
    };
    h /= 6.0_f32;
    if h < 0.0_f32 {
        h += 1.0_f32;
    }

    let k = (h - (f64::from(h).floor() as f32)) * 6.0_f32;
    let f = k - (f64::from(k).floor() as f32);
    let p = brightness * (1.0_f32 - s);
    let q = brightness * (1.0_f32 - s * f);
    let t = brightness * (1.0_f32 - s * (1.0_f32 - f));

    let (rf, gf, bf) = match k as i32 {
        0 => (brightness, t, p),
        1 => (q, brightness, p),
        2 => (p, brightness, t),
        3 => (p, q, brightness),
        4 => (t, p, brightness),
        _ => (brightness, p, q),
    };

    color_argb(
        a,
        java_round_f32(rf * 255.0_f32),
        java_round_f32(gf * 255.0_f32),
        java_round_f32(bf * 255.0_f32),
    )
}

/// Fallible version of [`hsv_to_argb`] returning `None` when the sector index
/// `trunc_i32(h * 6.0_f32) % 6` is outside `0..=5` (e.g., `h <= -1/6`, where Java throws
/// `RuntimeException`).
#[inline]
pub fn try_hsv_to_argb(h: f32, s: f32, v: f32, alpha_byte: i32) -> Option<i32> {
    let hi = ((h * 6.0_f32) as i32) % 6;
    let f = h * 6.0_f32 - (hi as f32);
    let p = v * (1.0_f32 - s);
    let q = v * (1.0_f32 - f * s);
    let t = v * (1.0_f32 - (1.0_f32 - f) * s);

    let (r, g, b) = match hi {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        5 => (v, p, q),
        _ => return None,
    };

    Some(color_argb(
        alpha_byte,
        clamp_i32((r * 255.0_f32) as i32, 0, 255),
        clamp_i32((g * 255.0_f32) as i32, 0, 255),
        clamp_i32((b * 255.0_f32) as i32, 0, 255),
    ))
}

/// `Mth.hsvToArgb(h, s, v, alpha)`: converts HSV to packed ARGB with `trunc_i32` channel
/// quantization.
///
/// # Panics
///
/// Panics if `trunc_i32(h * 6.0_f32) % 6` is outside `0..=5` (matching Java's `RuntimeException`
/// for `h <= -1/6`). Use [`try_hsv_to_argb`] for a non-panicking check.
#[inline]
pub fn hsv_to_argb(h: f32, s: f32, v: f32, alpha_byte: i32) -> i32 {
    try_hsv_to_argb(h, s, v, alpha_byte)
        .unwrap_or_else(|| panic!("invalid HSV hue sector for h={h}"))
}

/// `Mth.hsvToRgb(h, s, v)`: calls [`hsv_to_argb`] with **`alpha = 0`** (not `255`).
///
/// # Panics
///
/// Panics under the same conditions as [`hsv_to_argb`].
#[inline]
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> i32 {
    hsv_to_argb(h, s, v, 0)
}

// ---------------------------------------------------------------------------
// LightCoordsUtil
// ---------------------------------------------------------------------------

/// `LightCoordsUtil.pack(block, sky)`: `(block << 4) | (sky << 20)`.
#[inline]
pub const fn pack_light(block: i32, sky: i32) -> i32 {
    (block << 4) | (sky << 20)
}

/// `LightCoordsUtil.block(packed)`: `(p >> 4) & 15`.
#[inline]
pub const fn unpack_block_light(packed: i32) -> i32 {
    (packed >> 4) & 15
}

/// `LightCoordsUtil.sky(packed)`: `(p >> 20) & 15`.
#[inline]
pub const fn unpack_sky_light(packed: i32) -> i32 {
    (packed >> 20) & 15
}

/// `LightCoordsUtil.withBlock(packed, block)`: `(p & 0xFF0000) | (b << 4)`.
#[inline]
pub const fn with_block_light(packed: i32, block: i32) -> i32 {
    (packed & 0x00FF_0000) | (block << 4)
}

/// `LightCoordsUtil.smoothPack(block, sky)`: `(block & 0xFF) | ((sky & 0xFF) << 16)`.
#[inline]
pub const fn smooth_pack_light(block: i32, sky: i32) -> i32 {
    (block & 0xFF) | ((sky & 0xFF) << 16)
}

/// `LightCoordsUtil.smoothBlock(packed)`: `p & 0xFF` (range `0..=240`).
#[inline]
pub const fn unpack_smooth_block_light(packed: i32) -> i32 {
    packed & 0xFF
}

/// `LightCoordsUtil.smoothSky(packed)`: `(p >> 16) & 0xFF` (range `0..=240`).
#[inline]
pub const fn unpack_smooth_sky_light(packed: i32) -> i32 {
    (packed >> 16) & 0xFF
}

/// `LightCoordsUtil.smoothBlend(n1, n2, n3, center)`.
#[inline]
pub const fn smooth_blend_light(mut n1: i32, mut n2: i32, mut n3: i32, center: i32) -> i32 {
    if unpack_sky_light(center) > 2 || unpack_block_light(center) > 2 {
        let sky_bits = center & 0x00FF_0000;
        if n1 == 0 {
            n1 = center;
        } else if unpack_sky_light(n1) == 0 {
            n1 |= sky_bits;
        }
        if n2 == 0 {
            n2 = center;
        } else if unpack_sky_light(n2) == 0 {
            n2 |= sky_bits;
        }
        if n3 == 0 {
            n3 = center;
        } else if unpack_sky_light(n3) == 0 {
            n3 |= sky_bits;
        }
    }
    (n1.wrapping_add(n2).wrapping_add(n3).wrapping_add(center) >> 2) & 0x00FF_00FF
}

/// `LightCoordsUtil.smoothWeightedBlend(c0, c1, c2, c3, w0, w1, w2, w3)`: weights `smoothSky`
/// and `smoothBlock` in `f32` left-to-right, truncates each sum to `i32`, and packs via
/// [`smooth_pack_light`].
#[inline]
#[allow(clippy::too_many_arguments)]
pub fn smooth_weighted_blend_light(
    c0: i32,
    c1: i32,
    c2: i32,
    c3: i32,
    w0: f32,
    w1: f32,
    w2: f32,
    w3: f32,
) -> i32 {
    let sky = ((((unpack_smooth_sky_light(c0) as f32) * w0
        + (unpack_smooth_sky_light(c1) as f32) * w1)
        + (unpack_smooth_sky_light(c2) as f32) * w2)
        + (unpack_smooth_sky_light(c3) as f32) * w3) as i32;
    let block = ((((unpack_smooth_block_light(c0) as f32) * w0
        + (unpack_smooth_block_light(c1) as f32) * w1)
        + (unpack_smooth_block_light(c2) as f32) * w2)
        + (unpack_smooth_block_light(c3) as f32) * w3) as i32;
    smooth_pack_light(block, sky)
}
