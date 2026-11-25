//! Noise utility helpers (`NoiseUtils`): extreme-bias remap and parity configuration formatting.

use std::f64::consts::PI;

/// Sine matching Java's `Math.sin(double)`.
///
/// All noise trigonometric calls route through this single entry point so platform-specific
/// HotSpot intrinsic differences can be verified and handled in one place.
// PARITY: verify against oracle samples per platform
#[inline]
pub fn java_math_sin(x: f64) -> f64 {
    x.sin()
}

/// Extreme-bias remap: `x + ((sin(PI * x) * k) / PI)` in `f64`.
///
/// Exact evaluation order matches Java's `x + Math.sin(Math.PI * x) * k / Math.PI`, routing
/// `sin` through [`java_math_sin`].
#[inline]
pub fn bias_towards_extreme(x: f64, k: f64) -> f64 {
    x + ((java_math_sin(PI * x) * k) / PI)
}

/// Formats an `f32` to 3 fractional digits using Java `Formatter` (`Locale.ROOT`, `%.3f`)
/// round-half-up semantics.
fn format_java_f32_3(value: f32) -> String {
    if value.is_nan() {
        return String::from("NaN");
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            String::from("-Infinity")
        } else {
            String::from("Infinity")
        };
    }

    // For `%.3f`, an IEEE-754 binary32 number is an exact halfway tie `(2n + 1) / 2000`
    // if and only if its reduced denominator is 16 (i.e. `value * 16` is an odd integer,
    // which implies `|value| < 2^24` since all binary32 values >= 2^24 are integers).
    if value.abs() < 16_777_216.0_f32 {
        let d = f64::from(value);
        if (d * 16.0).fract() == 0.0 && (d * 8.0).fract().abs() == 0.5 {
            let millis = (d.abs() * 1000.0 + 0.5) as u64;
            let whole = millis / 1000;
            let frac = millis % 1000;
            return if value.is_sign_negative() {
                format!("-{whole}.{frac:03}")
            } else {
                format!("{whole}.{frac:03}")
            };
        }
    }

    format!("{value:.3}")
}

/// Formats parity octave configuration for a signed `i8[256]` permutation table:
/// `xo=%.3f, yo=%.3f, zo=%.3f, p0=%d, p255=%d`.
///
/// Offsets are narrowed to `f32` before `%.3f` formatting, and `p[0]` / `p[255]` are formatted
/// as signed integers (`-128..=127`).
pub fn parity_octave_config(xo: f64, yo: f64, zo: f64, p: &[i8; 256]) -> String {
    format!(
        "xo={}, yo={}, zo={}, p0={}, p255={}",
        format_java_f32_3(xo as f32),
        format_java_f32_3(yo as f32),
        format_java_f32_3(zo as f32),
        p[0],
        p[255],
    )
}

/// Formats parity octave configuration for a signed `i32[256]` permutation table:
/// `xo=%.3f, yo=%.3f, zo=%.3f, p0=%d, p255=%d`.
///
/// Offsets are narrowed to `f32` before `%.3f` formatting, and `p[0]` / `p[255]` are formatted
/// as signed 32-bit integers.
pub fn parity_octave_config_i32(xo: f64, yo: f64, zo: f64, p: &[i32; 256]) -> String {
    format!(
        "xo={}, yo={}, zo={}, p0={}, p255={}",
        format_java_f32_3(xo as f32),
        format_java_f32_3(yo as f32),
        format_java_f32_3(zo as f32),
        p[0],
        p[255],
    )
}
