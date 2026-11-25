//! Piecewise cubic Hermite splines (`minecraft:spline`) with exact `f32` evaluation and
//! conservative interval bounds (`docs/specs/worldgen/density-functions.md` §3).
//!
//! Unlike `zenith_math::CubicSpline::multipoint` (which matches the programmatic builder and
//! rejects unsorted locations), data-pack JSON decoding accepts any non-empty `points` list
//! without enforcing strictly ascending `locations`.

use std::hash::{Hash, Hasher};

use zenith_math::Interval;
use zenith_math::helpers::interp::lerp_f32;
use zenith_math::helpers::rounding::{binary_search, java_max_f32, java_min_f32};

use crate::interval::{canonical_f32_bits, of_exact};

/// Piecewise cubic Hermite spline generic over its coordinate type `F`.
#[derive(Debug, Clone)]
pub enum CubicSpline<F> {
    /// Constant `f32` leaf.
    Constant(f32),
    /// Piecewise cubic Hermite multipoint node.
    Multipoint {
        /// Coordinate function producing `u`.
        coordinate: F,
        /// Control-point locations (`len >= 1`).
        locations: Vec<f32>,
        /// Nested spline values at each control point (`len == locations.len()`).
        values: Vec<CubicSpline<F>>,
        /// Slopes (`du`) at each control point (`len == locations.len()`).
        derivatives: Vec<f32>,
        /// Precomputed conservative output interval.
        range: Interval,
    },
}

impl<F: PartialEq> PartialEq for CubicSpline<F> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Constant(a), Self::Constant(b)) => {
                canonical_f32_bits(*a) == canonical_f32_bits(*b)
            }
            (
                Self::Multipoint {
                    coordinate: c1,
                    locations: l1,
                    values: v1,
                    derivatives: d1,
                    ..
                },
                Self::Multipoint {
                    coordinate: c2,
                    locations: l2,
                    values: v2,
                    derivatives: d2,
                    ..
                },
            ) => {
                c1 == c2
                    && l1.len() == l2.len()
                    && d1.len() == d2.len()
                    && l1
                        .iter()
                        .zip(l2.iter())
                        .all(|(&a, &b)| canonical_f32_bits(a) == canonical_f32_bits(b))
                    && d1
                        .iter()
                        .zip(d2.iter())
                        .all(|(&a, &b)| canonical_f32_bits(a) == canonical_f32_bits(b))
                    && v1 == v2
            }
            _ => false,
        }
    }
}

impl<F: Eq> Eq for CubicSpline<F> {}

impl<F: Hash> Hash for CubicSpline<F> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::Constant(v) => {
                0u8.hash(state);
                canonical_f32_bits(*v).hash(state);
            }
            Self::Multipoint {
                coordinate,
                locations,
                values,
                derivatives,
                ..
            } => {
                1u8.hash(state);
                coordinate.hash(state);
                locations.len().hash(state);
                for &loc in locations {
                    canonical_f32_bits(loc).hash(state);
                }
                values.hash(state);
                derivatives.len().hash(state);
                for &d in derivatives {
                    canonical_f32_bits(d).hash(state);
                }
            }
        }
    }
}

impl<F> CubicSpline<F> {
    /// Creates a constant spline leaf.
    #[inline]
    pub const fn constant(value: f32) -> Self {
        Self::Constant(value)
    }

    /// Creates a multipoint spline node and computes its conservative interval bound using
    /// `coord_range` for the coordinate's range.
    ///
    /// # Panics
    ///
    /// Panics if `locations` is empty or `locations.len() != values.len()` or
    /// `locations.len() != derivatives.len()`.
    pub fn multipoint(
        coordinate: F,
        coord_range: Interval,
        locations: Vec<f32>,
        values: Vec<Self>,
        derivatives: Vec<f32>,
    ) -> Self {
        assert!(!locations.is_empty(), "spline must have at least one point");
        assert_eq!(locations.len(), values.len());
        assert_eq!(locations.len(), derivatives.len());
        let range = compute_multipoint_range(coord_range, &locations, &values, &derivatives);
        Self::Multipoint {
            coordinate,
            locations,
            values,
            derivatives,
            range,
        }
    }

    /// Returns the conservative `f32` output interval for this spline.
    #[inline]
    pub fn range(&self) -> Interval {
        match self {
            Self::Constant(v) => of_exact(*v),
            Self::Multipoint { range, .. } => *range,
        }
    }

    /// Maps every coordinate in the tree using `f`, preserving structure and recomputing ranges
    /// via `range_of`.
    pub fn map_coordinates<G>(
        &self,
        f: &mut impl FnMut(&F) -> G,
        range_of: &impl Fn(&G) -> Interval,
    ) -> CubicSpline<G> {
        match self {
            Self::Constant(v) => CubicSpline::Constant(*v),
            Self::Multipoint {
                coordinate,
                locations,
                values,
                derivatives,
                ..
            } => {
                let new_coord = f(coordinate);
                let coord_range = range_of(&new_coord);
                let new_values = values
                    .iter()
                    .map(|v| v.map_coordinates(f, range_of))
                    .collect();
                CubicSpline::multipoint(
                    new_coord,
                    coord_range,
                    locations.clone(),
                    new_values,
                    derivatives.clone(),
                )
            }
        }
    }

    /// Fallible version of [`Self::map_coordinates`], where `f` returns `(new_coord, new_range)`.
    pub fn try_map_coordinates<G, E>(
        &self,
        f: &mut impl FnMut(&F) -> Result<(G, Interval), E>,
    ) -> Result<CubicSpline<G>, E> {
        match self {
            Self::Constant(v) => Ok(CubicSpline::Constant(*v)),
            Self::Multipoint {
                coordinate,
                locations,
                values,
                derivatives,
                ..
            } => {
                let (new_coord, coord_range) = f(coordinate)?;
                let mut new_values = Vec::with_capacity(values.len());
                for v in values {
                    new_values.push(v.try_map_coordinates(f)?);
                }
                Ok(CubicSpline::multipoint(
                    new_coord,
                    coord_range,
                    locations.clone(),
                    new_values,
                    derivatives.clone(),
                ))
            }
        }
    }

    /// Evaluates this spline in exact `f32` (`density-functions.md` §3).
    pub fn sample_with(&self, eval_coord: &mut impl FnMut(&F) -> f32) -> f32 {
        match self {
            Self::Constant(v) => *v,
            Self::Multipoint {
                coordinate,
                locations,
                values,
                derivatives,
                ..
            } => {
                let u = eval_coord(coordinate);
                let n = locations.len();
                let last = n - 1;
                let start = binary_search(0, n as i32, |i| u < locations[i as usize]) - 1;
                if start < 0 {
                    let y0 = values[0].sample_with(eval_coord);
                    linear_extend(u, locations[0], y0, derivatives[0])
                } else if (start as usize) == last {
                    let y_last = values[last].sample_with(eval_coord);
                    linear_extend(u, locations[last], y_last, derivatives[last])
                } else {
                    let idx = start as usize;
                    let x1 = locations[idx];
                    let x2 = locations[idx + 1];
                    let t = (u - x1) / (x2 - x1);
                    let y1 = values[idx].sample_with(eval_coord);
                    let y2 = values[idx + 1].sample_with(eval_coord);
                    let d1 = derivatives[idx];
                    let d2 = derivatives[idx + 1];
                    let a = d1 * (x2 - x1) - (y2 - y1);
                    let b = (-d2) * (x2 - x1) + (y2 - y1);
                    lerp_f32(t, y1, y2) + (t * (1.0_f32 - t)) * lerp_f32(t, a, b)
                }
            }
        }
    }
}

#[inline]
fn linear_extend(u: f32, loc: f32, y: f32, deriv: f32) -> f32 {
    if deriv == 0.0_f32 {
        y
    } else {
        y + deriv * (u - loc)
    }
}

fn compute_multipoint_range<F>(
    input_range: Interval,
    locations: &[f32],
    values: &[CubicSpline<F>],
    derivatives: &[f32],
) -> Interval {
    let Some((in_min, in_max)) = input_range.bounds() else {
        return Interval::NaI;
    };

    let n = locations.len();
    let mut acc_min = f32::INFINITY;
    let mut acc_max = f32::NEG_INFINITY;

    if in_min < locations[0] {
        let Some((m0, big_m0)) = values[0].range().bounds() else {
            return Interval::NaI;
        };
        let e0 = linear_extend(in_min, locations[0], m0, derivatives[0]);
        let e1 = linear_extend(in_min, locations[0], big_m0, derivatives[0]);
        acc_min = java_min_f32(acc_min, java_min_f32(e0, e1));
        acc_max = java_max_f32(acc_max, java_max_f32(e0, e1));
    }

    if in_max > locations[n - 1] {
        let last = n - 1;
        let Some((ml, big_ml)) = values[last].range().bounds() else {
            return Interval::NaI;
        };
        let e0 = linear_extend(in_max, locations[last], ml, derivatives[last]);
        let e1 = linear_extend(in_max, locations[last], big_ml, derivatives[last]);
        acc_min = java_min_f32(acc_min, java_min_f32(e0, e1));
        acc_max = java_max_f32(acc_max, java_max_f32(e0, e1));
    }

    for v in values {
        let Some((mv, big_mv)) = v.range().bounds() else {
            return Interval::NaI;
        };
        acc_min = java_min_f32(acc_min, mv);
        acc_max = java_max_f32(acc_max, big_mv);
    }

    for i in 0..(n - 1) {
        let d1 = derivatives[i];
        let d2 = derivatives[i + 1];
        if d1 != 0.0_f32 || d2 != 0.0_f32 {
            let dx = locations[i + 1] - locations[i];
            let p1 = d1 * dx;
            let p2 = d2 * dx;
            let (m1, big_m1) = values[i].range().bounds().expect("checked non-NaI above");
            let (m2, big_m2) = values[i + 1]
                .range()
                .bounds()
                .expect("checked non-NaI above");
            let lo =
                java_min_f32(m1, m2) + 0.25_f32 * java_min_f32(p1 - big_m2 + m1, -p2 + m2 - big_m1);
            let hi = java_max_f32(big_m1, big_m2)
                + 0.25_f32 * java_max_f32(p1 - m2 + big_m1, -p2 + big_m2 - m1);
            acc_min = java_min_f32(acc_min, lo);
            acc_max = java_max_f32(acc_max, hi);
        }
    }

    crate::interval::of(acc_min, acc_max)
}
