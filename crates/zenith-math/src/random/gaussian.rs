//! Marsaglia polar Gaussian sampler with a one-value cache.

use crate::java::java_math_log;

/// One-value cache for Marsaglia polar standard normal generation.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MarsagliaPolarGaussian {
    next: Option<f64>,
}

impl MarsagliaPolarGaussian {
    /// Creates a new Gaussian sampler with an empty cache.
    #[inline]
    pub const fn new() -> Self {
        Self { next: None }
    }

    /// Clears any cached Gaussian sample.
    #[inline]
    pub fn reset(&mut self) {
        self.next = None;
    }

    /// Returns `true` if a second sample from the previous polar draw is currently cached.
    #[inline]
    pub const fn has_cached(&self) -> bool {
        self.next.is_some()
    }

    /// Draws the next standard normal sample using `next_double` for uniform `[0.0, 1.0)` draws.
    ///
    /// If a value is cached, returns and clears it without invoking `next_double`. Otherwise draws
    /// pairs `(x, y)` in `[-1.0, 1.0)` until `0 < x*x + y*y < 1`, caches `y * m`, and returns
    /// `x * m` where `m = sqrt(-2 * ln(r) / r)`.
    pub fn next_gaussian<F>(&mut self, mut next_double: F) -> f64
    where
        F: FnMut() -> f64,
    {
        if let Some(cached) = self.next.take() {
            return cached;
        }

        loop {
            let x = 2.0 * next_double() - 1.0;
            let y = 2.0 * next_double() - 1.0;
            let r = x * x + y * y;
            if r > 0.0 && r < 1.0 {
                let m = (-2.0 * java_math_log(r) / r).sqrt();
                self.next = Some(y * m);
                return x * m;
            }
        }
    }
}
