//! Centralized entry points for JVM-intrinsic-sensitive floating-point functions.
//!
//! On HotSpot JVMs, transcendental functions (`Math.sin`, `Math.cos`, `Math.asin`, `Math.atan2`,
//! `Math.log`, `Math.exp`, `Math.pow`) may use CPU-specific intrinsics that differ from
//! `StrictMath`/fdlibm by up to 1 ULP on some inputs. Routing all calls in `zenith_math::helpers`
//! through this module (and delegating to [`crate::java`] / [`crate::noise`] where already
//! defined) keeps platform-variance auditing in a single place.

/// Natural logarithm matching Java's `Math.log(double)`.
// PARITY: verify against oracle samples per platform
#[inline]
pub fn java_math_log(x: f64) -> f64 {
    crate::java::java_math_log(x)
}

/// Sine matching Java's `Math.sin(double)`.
// PARITY: verify against oracle samples per platform
#[inline]
pub fn java_math_sin(x: f64) -> f64 {
    crate::noise::java_math_sin(x)
}

/// Cosine matching Java's `Math.cos(double)`.
// PARITY: verify against oracle samples per platform
#[inline]
pub fn java_math_cos(x: f64) -> f64 {
    x.cos()
}

/// Arcsine matching Java's `Math.asin(double)`.
// PARITY: verify against oracle samples per platform
#[inline]
pub fn java_math_asin(x: f64) -> f64 {
    x.asin()
}

/// Two-argument arctangent matching Java's `Math.atan2(double, double)`.
// PARITY: verify against oracle samples per platform
#[inline]
pub fn java_math_atan2(y: f64, x: f64) -> f64 {
    y.atan2(x)
}

/// Exponential matching Java's `Math.exp(double)`.
// PARITY: verify against oracle samples per platform
#[inline]
pub fn java_math_exp(x: f64) -> f64 {
    x.exp()
}

/// Power function matching Java's `Math.pow(double, double)`.
// PARITY: verify against oracle samples per platform
#[inline]
pub fn java_math_pow(base: f64, exp: f64) -> f64 {
    base.powf(exp)
}
