//! Conservative `f32` interval arithmetic and Java-exact float helpers for density functions.
//!
//! Implements the `Interval` rules in `docs/specs/worldgen/density-functions.md` ("Inputs and state").

use zenith_math::Interval;
use zenith_math::helpers::intrinsics::{java_math_log, java_math_pow};
use zenith_math::helpers::rounding::{java_max_f32, java_min_f32};

/// Unbounded interval `[-inf, +inf]`.
pub const INFINITE: Interval = Interval::Bounds {
    min: f32::NEG_INFINITY,
    max: f32::INFINITY,
};

/// Canonical IEEE-754 `f32` bit representation matching Java `Float.compare` / `Float.hashCode`:
/// distinguishes `-0.0` from `+0.0` and canonicalizes all `NaN` payloads to `0x7FC0_0000`.
#[inline]
pub(crate) fn canonical_f32_bits(v: f32) -> u32 {
    if v.is_nan() { 0x7FC0_0000 } else { v.to_bits() }
}

/// Canonical IEEE-754 `f64` bit representation matching Java `Double.compare` / `Double.hashCode`:
/// distinguishes `-0.0` from `+0.0` and canonicalizes all `NaN` payloads to `0x7FF8_0000_0000_0000`.
#[inline]
pub(crate) fn canonical_f64_bits(v: f64) -> u64 {
    if v.is_nan() {
        0x7FF8_0000_0000_0000
    } else {
        v.to_bits()
    }
}

/// Java `Float.compare(a, b)`: orders `-0.0 < +0.0` and `NaN` greater than all other values
/// (equal to `NaN`).
#[inline]
pub fn java_compare_f32(a: f32, b: f32) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    if a < b {
        Ordering::Less
    } else if a > b {
        Ordering::Greater
    } else {
        let ab = canonical_f32_bits(a) as i32;
        let bb = canonical_f32_bits(b) as i32;
        // For NaN: 0x7FC0_0000 > +Inf (0x7F80_0000).
        // For -0.0 (0x8000_0000 = i32::MIN) vs +0.0 (0): ab < bb.
        ab.cmp(&bb)
    }
}

/// Java `Math.signum(float)`: returns `NaN` for `NaN`, preserves `±0.0`, `+1.0` for `> 0.0`,
/// and `-1.0` for `< 0.0`.
#[inline]
pub fn java_signum_f32(x: f32) -> f32 {
    if x == 0.0_f32 || x.is_nan() {
        x
    } else if x > 0.0_f32 {
        1.0_f32
    } else {
        -1.0_f32
    }
}

/// Java `Math.pow(double, double)` with Java's `|base| == 1.0 && exp.is_infinite() -> NaN` rule.
#[inline]
pub fn java_pow_f64(base: f64, exp: f64) -> f64 {
    if exp.is_infinite() && base.abs() == 1.0_f64 {
        f64::NAN
    } else {
        java_math_pow(base, exp)
    }
}

#[inline]
fn pow_f32(base: f32, exp: f32) -> f32 {
    java_pow_f64(f64::from(base), f64::from(exp)) as f32
}

#[inline]
fn zero_absorbing_mul_f32(u: f32, v: f32) -> f32 {
    if u == 0.0_f32 || v == 0.0_f32 {
        0.0_f32
    } else {
        u * v
    }
}

/// Constructs `[min, max]`, returning [`Interval::NaI`] if either bound is `NaN` or `max < min`.
#[inline]
pub fn of(min: f32, max: f32) -> Interval {
    if min.is_nan() || max.is_nan() || max < min {
        Interval::NaI
    } else {
        Interval::Bounds { min, max }
    }
}

/// Exact singleton interval `[v, v]` (or `NaI` if `v` is `NaN`).
#[inline]
pub fn of_exact(v: f32) -> Interval {
    Interval::exact(v)
}

/// Symmetric interval `[-r, r]` (or `NaI` if `r` is `NaN` or negative).
#[inline]
pub fn of_symmetric(r: f32) -> Interval {
    Interval::symmetric(r)
}

/// Smallest interval encapsulating all non-`NaI` elements of `list`, or `NaI` if none.
pub fn encapsulating(list: &[Interval]) -> Interval {
    let mut lo = f32::INFINITY;
    let mut hi = f32::NEG_INFINITY;
    for &item in list {
        if let Interval::Bounds { min, max } = item {
            lo = java_min_f32(lo, min);
            hi = java_max_f32(hi, max);
        }
    }
    if hi < lo {
        Interval::NaI
    } else {
        Interval::Bounds { min: lo, max: hi }
    }
}

/// Smallest interval encapsulating two `f32` scalars (ignoring `NaN`; `NaI` if both are `NaN`).
#[inline]
pub fn encapsulating_pair(a: f32, b: f32) -> Interval {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => Interval::NaI,
        (true, false) => of_exact(b),
        (false, true) => of_exact(a),
        (false, false) => Interval::Bounds {
            min: java_min_f32(a, b),
            max: java_max_f32(a, b),
        },
    }
}

/// Interval addition `A + B`.
#[inline]
pub fn add(a: Interval, b: Interval) -> Interval {
    match (a, b) {
        (Interval::Bounds { min: a0, max: a1 }, Interval::Bounds { min: b0, max: b1 }) => {
            of(a0 + b0, a1 + b1)
        }
        _ => Interval::NaI,
    }
}

/// Interval subtraction `A - B = [A.min - B.max, A.max - B.min]`.
#[inline]
pub fn sub(a: Interval, b: Interval) -> Interval {
    match (a, b) {
        (Interval::Bounds { min: a0, max: a1 }, Interval::Bounds { min: b0, max: b1 }) => {
            of(a0 - b1, a1 - b0)
        }
        _ => Interval::NaI,
    }
}

/// Interval multiplication using zero-absorbing corner products.
#[inline]
pub fn mul(a: Interval, b: Interval) -> Interval {
    a.mul_interval(b)
}

/// Interval reciprocal `1 / A`.
#[inline]
pub fn reciprocal(a: Interval) -> Interval {
    let Interval::Bounds { min, max } = a else {
        return Interval::NaI;
    };
    if min == 0.0_f32 && max == 0.0_f32 {
        return Interval::NaI;
    }
    if !(min <= 0.0_f32 && 0.0_f32 <= max) {
        of(1.0_f32 / max, 1.0_f32 / min)
    } else if max == 0.0_f32 {
        of(f32::NEG_INFINITY, 1.0_f32 / min)
    } else if min == 0.0_f32 {
        of(1.0_f32 / max, f32::INFINITY)
    } else {
        INFINITE
    }
}

/// Interval division `mul(A, reciprocal(B))`.
#[inline]
pub fn div(a: Interval, b: Interval) -> Interval {
    mul(a, reciprocal(b))
}

/// Interval element-wise minimum.
#[inline]
pub fn min(a: Interval, b: Interval) -> Interval {
    match (a, b) {
        (Interval::Bounds { min: a0, max: a1 }, Interval::Bounds { min: b0, max: b1 }) => {
            Interval::Bounds {
                min: java_min_f32(a0, b0),
                max: java_min_f32(a1, b1),
            }
        }
        _ => Interval::NaI,
    }
}

/// Interval element-wise maximum.
#[inline]
pub fn max(a: Interval, b: Interval) -> Interval {
    match (a, b) {
        (Interval::Bounds { min: a0, max: a1 }, Interval::Bounds { min: b0, max: b1 }) => {
            Interval::Bounds {
                min: java_max_f32(a0, b0),
                max: java_max_f32(a1, b1),
            }
        }
        _ => Interval::NaI,
    }
}

/// Clamps `a` into `[lo, hi]`.
#[inline]
pub fn clamp(a: Interval, lo: f32, hi: f32) -> Interval {
    debug_assert!(lo <= hi, "clamp bounds must satisfy lo <= hi");
    let Interval::Bounds { min, max } = a else {
        return Interval::NaI;
    };
    if min >= hi {
        of_exact(hi)
    } else if max <= lo {
        of_exact(lo)
    } else {
        Interval::Bounds {
            min: java_max_f32(min, lo),
            max: java_min_f32(max, hi),
        }
    }
}

/// Interval absolute value.
#[inline]
pub fn abs(a: Interval) -> Interval {
    let Interval::Bounds { min, max } = a else {
        return Interval::NaI;
    };
    let hi = java_max_f32(min.abs(), max.abs());
    if min <= 0.0_f32 && 0.0_f32 <= max {
        Interval::Bounds {
            min: 0.0_f32,
            max: hi,
        }
    } else {
        Interval::Bounds {
            min: java_min_f32(min.abs(), max.abs()),
            max: hi,
        }
    }
}

/// Interval square.
#[inline]
pub fn square(a: Interval) -> Interval {
    let Interval::Bounds { min, max } = a else {
        return Interval::NaI;
    };
    let s0 = min * min;
    let s1 = max * max;
    let hi = java_max_f32(s0, s1);
    if min <= 0.0_f32 && 0.0_f32 <= max {
        Interval::Bounds {
            min: 0.0_f32,
            max: hi,
        }
    } else {
        Interval::Bounds {
            min: java_min_f32(s0, s1),
            max: hi,
        }
    }
}

/// Applies a monotonic `f32 -> f32` function across an interval.
#[inline]
pub fn map_monotonic(a: Interval, f: impl Fn(f32) -> f32) -> Interval {
    let Interval::Bounds { min, max } = a else {
        return Interval::NaI;
    };
    let u = f(min);
    let v = f(max);
    if u.is_nan() || v.is_nan() {
        Interval::NaI
    } else {
        Interval::Bounds {
            min: java_min_f32(u, v),
            max: java_max_f32(u, v),
        }
    }
}

/// Interval natural logarithm.
#[inline]
pub fn log(a: Interval) -> Interval {
    let Interval::Bounds { max: a_max, .. } = a else {
        return Interval::NaI;
    };
    if a_max < 0.0_f32 {
        return Interval::NaI;
    }
    let clamped = max(
        a,
        Interval::Bounds {
            min: 0.0_f32,
            max: 0.0_f32,
        },
    );
    map_monotonic(clamped, |x| java_math_log(f64::from(x)) as f32)
}

/// Interval signum.
#[inline]
pub fn sign(a: Interval) -> Interval {
    let Interval::Bounds { min, max } = a else {
        return Interval::NaI;
    };
    if min == max {
        return of_exact(java_signum_f32(min));
    }
    if min <= 0.0_f32 && 0.0_f32 <= max {
        if min == 0.0_f32 {
            Interval::Bounds {
                min: 0.0_f32,
                max: 1.0_f32,
            }
        } else if max == 0.0_f32 {
            Interval::Bounds {
                min: -1.0_f32,
                max: 0.0_f32,
            }
        } else {
            Interval::Bounds {
                min: -1.0_f32,
                max: 1.0_f32,
            }
        }
    } else {
        of_exact(if min > 0.0_f32 { 1.0_f32 } else { -1.0_f32 })
    }
}

/// Interval power `B ^ E`.
pub fn pow(base: Interval, exp: Interval) -> Interval {
    let (Interval::Bounds { min: b0, max: b1 }, Interval::Bounds { .. }) = (base, exp) else {
        return Interval::NaI;
    };
    if b0 == b1 {
        return pow_scalar(b0, exp);
    }
    let mut parts = [Interval::NaI; 4];
    let mut count = 2;
    parts[0] = pow_scalar(b0, exp);
    parts[1] = pow_scalar(b1, exp);
    if b0 <= 0.0_f32 && 0.0_f32 <= b1 {
        if b1 > 0.0_f32 {
            parts[count] = pow_scalar(0.0_f32, exp);
            count += 1;
        }
        if b0 < 0.0_f32 {
            parts[count] = pow_scalar(-0.0_f32, exp);
            count += 1;
        }
    }
    encapsulating(&parts[..count])
}

/// Scalar-base interval power `b ^ E` (spec lines 74–80).
pub fn pow_scalar(b: f32, exp: Interval) -> Interval {
    let Interval::Bounds { min: e0, max: e1 } = exp else {
        return Interval::NaI;
    };
    if b.is_nan() {
        return Interval::NaI;
    }
    if e0 == e1 {
        let v = pow_f32(b, e0);
        return if v.is_nan() {
            Interval::NaI
        } else {
            of_exact(v)
        };
    }
    if b == 0.0_f32 {
        let z = if e0 <= 0.0_f32 && 0.0_f32 <= e1 {
            if e1 == 0.0_f32 {
                Interval::Bounds {
                    min: 1.0_f32,
                    max: f32::INFINITY,
                }
            } else if e0 == 0.0_f32 {
                Interval::Bounds {
                    min: 0.0_f32,
                    max: 1.0_f32,
                }
            } else {
                Interval::Bounds {
                    min: 0.0_f32,
                    max: f32::INFINITY,
                }
            }
        } else if e1 < 0.0_f32 {
            of_exact(f32::INFINITY)
        } else {
            of_exact(0.0_f32)
        };
        return mul(z, of_exact(1.0_f32.copysign(b)));
    }
    if b == 1.0_f32 {
        return of_exact(1.0_f32);
    }
    if b > 0.0_f32 {
        if e0.is_finite() && e1.is_finite() {
            encapsulating_pair(pow_f32(b, e0), pow_f32(b, e1))
        } else if e0.is_infinite() && e1.is_infinite() {
            Interval::Bounds {
                min: 0.0_f32,
                max: f32::INFINITY,
            }
        } else if e0.is_infinite() {
            let v = pow_f32(b, e1);
            if b < 1.0_f32 {
                of(v, f32::INFINITY)
            } else {
                of(0.0_f32, v)
            }
        } else {
            let v = pow_f32(b, e0);
            if b < 1.0_f32 {
                of(0.0_f32, v)
            } else {
                of(v, f32::INFINITY)
            }
        }
    } else {
        // b < 0.0
        let lo = f64::from(e0).ceil() as f32;
        let hi = f64::from(e1).floor() as f32;
        if hi < lo {
            return Interval::NaI;
        }
        let mut parts = [Interval::NaI; 3];
        let mut count = 1;
        parts[0] = encapsulating_pair(pow_f32(b, lo), pow_f32(b, hi));
        if lo.is_infinite() {
            parts[count] = of_exact(-pow_f32(b, lo));
            count += 1;
        } else if lo + 1.0_f32 < hi {
            parts[count] = of_exact(pow_f32(b, lo + 1.0_f32));
            count += 1;
        }
        if hi.is_infinite() {
            parts[count] = of_exact(-pow_f32(b, hi));
            count += 1;
        } else if hi - 1.0_f32 > lo {
            parts[count] = of_exact(pow_f32(b, hi - 1.0_f32));
            count += 1;
        }
        encapsulating(&parts[..count])
    }
}

/// Interval linear interpolation `lerp(T, A, B)`.
pub fn lerp(t: Interval, a: Interval, b: Interval) -> Interval {
    let (
        Interval::Bounds { .. },
        Interval::Bounds { min: a0, max: a1 },
        Interval::Bounds { min: b0, max: b1 },
    ) = (t, a, b)
    else {
        return Interval::NaI;
    };
    encapsulating(&[
        lerp_scalar(t, a0, b0),
        lerp_scalar(t, a1, b0),
        lerp_scalar(t, a0, b1),
        lerp_scalar(t, a1, b1),
    ])
}

fn lerp_scalar(t: Interval, a: f32, b: f32) -> Interval {
    let Interval::Bounds { min: t0, max: t1 } = t else {
        return Interval::NaI;
    };
    if a.is_finite() && b.is_finite() {
        let d = b - a;
        encapsulating_pair(
            a + zero_absorbing_mul_f32(t0, d),
            a + zero_absorbing_mul_f32(t1, d),
        )
    } else if a == b {
        of_exact(a)
    } else {
        encapsulating_pair(lerp_inf_point(t0, a, b), lerp_inf_point(t1, a, b))
    }
}

fn lerp_inf_point(tv: f32, a: f32, b: f32) -> f32 {
    let lhs = zero_absorbing_mul_f32(1.0_f32 - tv, a);
    let rhs = zero_absorbing_mul_f32(tv, b);
    if lhs.is_infinite() && rhs.is_infinite() {
        if tv <= 0.0_f32 {
            if b > a {
                f32::NEG_INFINITY
            } else {
                f32::INFINITY
            }
        } else if tv >= 1.0_f32 {
            if b > a {
                f32::INFINITY
            } else {
                f32::NEG_INFINITY
            }
        } else {
            f32::NAN
        }
    } else {
        lhs + rhs
    }
}

/// Returns `true` if `value` is contained in `interval` (`min <= value <= max`).
#[inline]
pub fn contains(interval: Interval, value: f32) -> bool {
    match interval {
        Interval::Bounds { min, max } => !value.is_nan() && value >= min && value <= max,
        Interval::NaI => false,
    }
}
