//! Java's `Double.toString` and `Float.toString` (JDK 19+ behavior), implemented from the public JDK specification.
//!
//! The specification, in short: among all decimals that round to the value, take those with the fewest significant
//! digits `n`. If `n` is 1, use two-digit decimals instead. Pick the one closest to the value, breaking an exact tie
//! toward an even last digit. Values with `10⁻³ ≤ |v| < 10⁷` print in plain notation (always with a fractional
//! part); everything else prints as `d.dddE±x`.
//!
//! Two parity edge cases this gets right and Rust's shortest `{}` formatting does not:
//! `f64::from_bits(1)` prints `4.9E-324` (not `5.0E-324`), and exact ties pick the even digit
//! (`-36.8203125f32` prints `-36.820312`).

use std::cmp::Ordering;

/// Rust's fixed-precision scientific formatting is exact and breaks ties toward even, which is what Java's
/// "closest decimal, even digit on ties" rule needs for each candidate length.
fn sci(precision: usize, formatted: &dyn Fn(usize) -> String) -> (String, i32) {
    let s = formatted(precision);
    let (mantissa, exp) = s.split_once('e').expect("LowerExp output has an exponent");
    (
        mantissa.replace('.', ""),
        exp.parse().expect("integer exponent"),
    )
}

/// Step an n-digit decimal (`digits` × 10^(exp − n + 1)) one unit in its last place, staying at n digits.
fn step(digits: &str, exp: i32, up: bool) -> (String, i32) {
    let n = digits.len();
    let m: u64 = digits.parse().expect("at most 17 digits");
    let lowest = 10u64.pow(n as u32 - 1);
    if up {
        let next = m + 1;
        if next == lowest * 10 {
            (format!("{lowest}"), exp + 1)
        } else {
            (format!("{next:0n$}"), exp)
        }
    } else if m == lowest {
        ("9".repeat(n), exp - 1)
    } else {
        (format!("{:0n$}", m - 1), exp)
    }
}

/// The closest n-digit decimal that still reads back as the value, if one exists.
///
/// The correctly rounded candidate is tried first. If it falls outside the rounding interval (which happens just
/// below a power of two, where the interval is lopsided), only its neighbor on the *other* side of the value can
/// be inside, because anything further out on the failing side is further outside.
fn best_of_len(
    n: usize,
    formatted: &dyn Fn(usize) -> String,
    cmp: &dyn Fn(&str, i32) -> Ordering,
) -> Option<(String, i32)> {
    let (digits, exp) = sci(n - 1, formatted);
    match cmp(&digits, exp) {
        Ordering::Equal => Some((digits, exp)),
        side => {
            let (other, oexp) = step(&digits, exp, side == Ordering::Less);
            (cmp(&other, oexp) == Ordering::Equal).then_some((other, oexp))
        }
    }
}

/// Digits (no dot) and decimal exponent of the first digit, for a finite, non-zero, non-negative value.
/// `cmp` says how a candidate decimal *reads back* compared with the value (`Equal` = it round-trips).
fn java_digits(
    max_digits: usize,
    formatted: &dyn Fn(usize) -> String,
    cmp: &dyn Fn(&str, i32) -> Ordering,
) -> (String, i32) {
    for n in 1..=max_digits {
        if let Some((digits, exp)) = best_of_len(n, formatted, cmp) {
            if n > 1 {
                return (digits, exp);
            }
            // Java considers two-digit decimals when one digit would do.
            return best_of_len(2, formatted, cmp).unwrap_or((format!("{digits}0"), exp));
        }
    }
    sci(max_digits - 1, formatted)
}

fn as_decimal(digits: &str, exp: i32) -> String {
    format!(
        "{}.{}e{exp}",
        &digits[..1],
        if digits.len() > 1 { &digits[1..] } else { "0" }
    )
}

/// Lay out digits/exponent the way Java does.
fn layout(out: &mut String, negative: bool, digits: &str, exp: i32, plain: bool) {
    if negative {
        out.push('-');
    }
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    if plain {
        if exp >= 0 {
            let int_len = exp as usize + 1;
            if digits.len() <= int_len {
                out.push_str(digits);
                out.extend(std::iter::repeat_n('0', int_len - digits.len()));
                out.push_str(".0");
            } else {
                out.push_str(&digits[..int_len]);
                out.push('.');
                out.push_str(&digits[int_len..]);
            }
        } else {
            out.push_str("0.");
            out.extend(std::iter::repeat_n('0', (-exp - 1) as usize));
            out.push_str(digits);
        }
    } else {
        out.push_str(&digits[..1]);
        out.push('.');
        out.push_str(if digits.len() > 1 { &digits[1..] } else { "0" });
        out.push('E');
        out.push_str(&exp.to_string());
    }
}

/// Append Java's `Double.toString(v)`.
pub fn write_java_double(out: &mut String, v: f64) {
    if v.is_nan() {
        out.push_str("NaN");
    } else if v.is_infinite() {
        out.push_str(if v < 0.0 { "-Infinity" } else { "Infinity" });
    } else if v == 0.0 {
        out.push_str(if v.is_sign_negative() { "-0.0" } else { "0.0" });
    } else {
        let a = v.abs();
        let (digits, exp) = java_digits(17, &|p| format!("{a:.p$e}"), &|d, e| {
            as_decimal(d, e)
                .parse::<f64>()
                .map_or(Ordering::Less, |x| x.total_cmp(&a))
        });
        layout(out, v < 0.0, &digits, exp, (1e-3..1e7).contains(&a));
    }
}

/// Append Java's `Float.toString(v)`.
pub fn write_java_float(out: &mut String, v: f32) {
    if v.is_nan() {
        out.push_str("NaN");
    } else if v.is_infinite() {
        out.push_str(if v < 0.0 { "-Infinity" } else { "Infinity" });
    } else if v == 0.0 {
        out.push_str(if v.is_sign_negative() { "-0.0" } else { "0.0" });
    } else {
        let a = v.abs();
        let (digits, exp) = java_digits(9, &|p| format!("{a:.p$e}"), &|d, e| {
            as_decimal(d, e)
                .parse::<f32>()
                .map_or(Ordering::Less, |x| x.total_cmp(&a))
        });
        layout(out, v < 0.0, &digits, exp, (1e-3..1e7).contains(&a));
    }
}

/// Java's `Double.toString` as a `String`.
pub fn java_double_to_string(v: f64) -> String {
    let mut s = String::new();
    write_java_double(&mut s, v);
    s
}

/// Java's `Float.toString` as a `String`.
pub fn java_float_to_string(v: f32) -> String {
    let mut s = String::new();
    write_java_float(&mut s, v);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    // -36.8203125 is exactly representable in f32; the tie-to-even case needs that exact value.
    #[allow(clippy::excessive_precision)]
    fn spec_edge_cases() {
        assert_eq!(java_double_to_string(f64::from_bits(1)), "4.9E-324");
        assert_eq!(java_float_to_string(f32::from_bits(1)), "1.4E-45");
        assert_eq!(java_float_to_string(-36.820_312_5), "-36.820312");
        assert_eq!(java_double_to_string(100.0), "100.0");
        assert_eq!(java_double_to_string(1e7), "1.0E7");
        assert_eq!(java_double_to_string(0.001), "0.001");
        assert_eq!(
            java_double_to_string(0.000_999_999_999_999_999_8),
            "9.999999999999998E-4"
        );
        assert_eq!(java_double_to_string(-0.0), "-0.0");
        assert_eq!(java_double_to_string(0.1), "0.1");
        assert_eq!(java_double_to_string(1.0 / 3.0), "0.3333333333333333");
        assert_eq!(java_float_to_string(1.0e10), "1.0E10");
        // Just below the interval's narrow side: a power of two (lopsided rounding interval).
        assert_eq!(
            java_double_to_string(f64::from_bits(0x3e70_0000_0000_0000)),
            "5.960464477539063E-8"
        );
    }
}
