//! Immutable AST for all 44 `minecraft:worldgen/density_function_type` variants plus
//! registry references (`HolderHolder`).
//!
//! Structural equality (`PartialEq`, `Eq`, `Hash`) matches Java record equality
//! (`Float.compare` / `Double.compare`), distinguishing `-0.0` from `+0.0` and canonicalizing
//! `NaN` payloads.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use zenith_math::helpers::intrinsics::java_math_log;
use zenith_math::helpers::roots::sqrt;
use zenith_math::helpers::rounding::{clamp_f32, java_max_f32, java_round_f32};
use zenith_math::{BlendedNoiseParameters, Interval, NormalNoiseParameters};
use zenith_registry::Identifier;

use crate::interval::{
    self, INFINITE, canonical_f32_bits, canonical_f64_bits, java_signum_f32, of, of_exact,
};
use crate::spline::CubicSpline;
use crate::volume::{ALL_AXES, AXIS_X, AXIS_Y, AXIS_Z, NO_AXES};

/// Coordinate axis used by `minecraft:gradient` and `minecraft:slice`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Axis3D {
    /// Block X axis (`"x"`).
    X,
    /// Block Y axis (`"y"`).
    Y,
    /// Block Z axis (`"z"`).
    Z,
}

impl Axis3D {
    /// Single-bit `@Axes` mask for this axis.
    #[inline]
    pub const fn mask(self) -> u8 {
        match self {
            Self::X => AXIS_X,
            Self::Y => AXIS_Y,
            Self::Z => AXIS_Z,
        }
    }

    /// Selects this axis's coordinate from `(bx, by, bz)`.
    #[inline]
    pub const fn select(self, bx: i32, by: i32, bz: i32) -> i32 {
        match self {
            Self::X => bx,
            Self::Y => by,
            Self::Z => bz,
        }
    }
}

/// Tiling mode for `minecraft:gradient`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GradientTiling {
    /// Clamp coordinate to `[min(from, to), max(from, to)]` (default).
    #[default]
    ClampToEdge,
    /// Periodic wrap via `Math.floorMod(c - from, range)`.
    Repeat,
    /// Ping-pong mirrored repeat.
    MirroredRepeat,
}

/// Distance metric for `minecraft:distance_to_point`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DistanceMetric {
    /// `Mth.sqrt((dx*dx + dy*dy) + dz*dz)`.
    Euclidean,
    /// `(dx*dx + dy*dy) + dz*dz`.
    EuclideanSquared,
    /// `(|dx| + |dy|) + |dz|`.
    Manhattan,
    /// `max(max(|dx|, |dy|), |dz|)`.
    Chebyshev,
}

/// Variant of `ShiftNoiseFunction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShiftKind {
    /// `minecraft:shift` (3D: `(bx*0.25, by*0.25, bz*0.25) * 4`).
    Shift,
    /// `minecraft:shift_a` (2D XZ: `(bx*0.25, 0, bz*0.25) * 4`).
    ShiftA,
    /// `minecraft:shift_b` (2D ZX: `(bz*0.25, bx*0.25, 0) * 4`).
    ShiftB,
}

/// All 11 unary density function operations (`density-functions.md` §2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    /// `minecraft:abs`
    Abs,
    /// `minecraft:square`
    Square,
    /// `minecraft:cube`
    Cube,
    /// `minecraft:sqrt`
    Sqrt,
    /// `minecraft:half_negative`
    HalfNegative,
    /// `minecraft:quarter_negative`
    QuarterNegative,
    /// `minecraft:reciprocal`
    Reciprocal,
    /// `minecraft:negate`
    Negate,
    /// `minecraft:squeeze`
    Squeeze,
    /// `minecraft:log`
    Log,
    /// `minecraft:sign`
    Sign,
}

impl UnaryOp {
    /// Evaluates this unary operation on `x: f32`.
    #[inline]
    pub fn apply(self, x: f32) -> f32 {
        match self {
            Self::Abs => x.abs(),
            Self::Square => x * x,
            Self::Cube => (x * x) * x,
            Self::Sqrt => sqrt(x),
            Self::HalfNegative => {
                if x > 0.0_f32 {
                    x
                } else {
                    x * 0.5_f32
                }
            }
            Self::QuarterNegative => {
                if x > 0.0_f32 {
                    x
                } else {
                    x * 0.25_f32
                }
            }
            Self::Reciprocal => 1.0_f32 / x,
            Self::Negate => -x,
            Self::Squeeze => {
                let c = clamp_f32(x, -1.0_f32, 1.0_f32);
                (c / 2.0_f32) - (((c * c) * c) / 24.0_f32)
            }
            Self::Log => java_math_log(f64::from(x)) as f32,
            Self::Sign => java_signum_f32(x),
        }
    }

    /// Conservative output interval given input interval `r`.
    #[inline]
    pub fn range(self, r: Interval) -> Interval {
        match self {
            Self::Abs => interval::abs(r),
            Self::Square => interval::square(r),
            Self::Cube => interval::map_monotonic(r, |x| (x * x) * x),
            Self::Sqrt => interval::pow(r, of_exact(0.5_f32)),
            Self::HalfNegative => interval::map_monotonic(r, |x| Self::HalfNegative.apply(x)),
            Self::QuarterNegative => interval::map_monotonic(r, |x| Self::QuarterNegative.apply(x)),
            Self::Reciprocal => interval::reciprocal(r),
            Self::Negate => interval::sub(of_exact(0.0_f32), r),
            Self::Squeeze => interval::map_monotonic(r, |x| Self::Squeeze.apply(x)),
            Self::Log => interval::log(r),
            Self::Sign => interval::sign(r),
        }
    }
}

/// All 4 rounding density function operations (`density-functions.md` §2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RoundOp {
    /// `minecraft:floor`
    Floor,
    /// `minecraft:round` (Java saturating `i32`, ties toward `+inf`, `NaN -> 0`)
    Round,
    /// `minecraft:ceil`
    Ceil,
    /// `minecraft:truncate`
    Truncate,
}

impl RoundOp {
    /// Primitive `roundToInt(v: f32) -> f32`.
    #[inline]
    pub fn round_to_int(self, v: f32) -> f32 {
        match self {
            Self::Floor => f64::from(v).floor() as f32,
            Self::Round => java_round_f32(v) as f32,
            Self::Ceil => f64::from(v).ceil() as f32,
            Self::Truncate => {
                if v > 0.0_f32 {
                    f64::from(v).floor() as f32
                } else {
                    f64::from(v).ceil() as f32
                }
            }
        }
    }
}

/// All 6 binary arithmetic / min-max density function operations (`density-functions.md` §2.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinaryOp {
    /// `minecraft:add`
    Add,
    /// `minecraft:sub`
    Sub,
    /// `minecraft:mul`
    Mul,
    /// `minecraft:div`
    Div,
    /// `minecraft:min`
    Min,
    /// `minecraft:max`
    Max,
}

impl BinaryOp {
    /// Conservative output interval for `left <op> right`.
    #[inline]
    pub fn range(self, left: Interval, right: Interval) -> Interval {
        match self {
            Self::Add => interval::add(left, right),
            Self::Sub => interval::sub(left, right),
            Self::Mul => interval::mul(left, right),
            Self::Div => interval::div(left, right),
            Self::Min => interval::min(left, right),
            Self::Max => interval::max(left, right),
        }
    }
}

/// Resolved `Holder<NormalNoise>`: either a registered noise entry or an inline parameter object.
///
/// Inline noises decode cleanly per `NormalNoise.CODEC`, and are rejected at `RandomState`
/// compile time (`density-functions.md` §4 step 2 / Edge Case #11).
#[derive(Debug, Clone)]
pub enum NoiseHolder {
    /// Registered entry in `minecraft:worldgen/noise`.
    Reference {
        /// Registry key.
        id: Identifier,
        /// Resolved parameters.
        params: Arc<NormalNoiseParameters>,
    },
    /// Unregistered inline parameter object.
    Inline(Arc<NormalNoiseParameters>),
}

impl NoiseHolder {
    /// Returns the underlying `NormalNoiseParameters`.
    #[inline]
    pub fn params(&self) -> &NormalNoiseParameters {
        match self {
            Self::Reference { params, .. } | Self::Inline(params) => params,
        }
    }

    /// Returns the registry key if this is a registered reference, or `None` if inline.
    #[inline]
    pub fn id(&self) -> Option<&Identifier> {
        match self {
            Self::Reference { id, .. } => Some(id),
            Self::Inline(_) => None,
        }
    }
}

impl PartialEq for NoiseHolder {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Reference { id: a, .. }, Self::Reference { id: b, .. }) => a == b,
            (Self::Inline(a), Self::Inline(b)) => params_eq(a, b),
            _ => false,
        }
    }
}

impl Eq for NoiseHolder {}

impl Hash for NoiseHolder {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::Reference { id, .. } => {
                0u8.hash(state);
                id.hash(state);
            }
            Self::Inline(p) => {
                1u8.hash(state);
                params_hash(p, state);
            }
        }
    }
}

fn params_eq(a: &NormalNoiseParameters, b: &NormalNoiseParameters) -> bool {
    canonical_f64_bits(a.base_amplitude()) == canonical_f64_bits(b.base_amplitude())
        && a.base_octave() == b.base_octave()
        && a.octave_count() == b.octave_count()
        && a.normalize() == b.normalize()
        && a.amplitude_modifiers().len() == b.amplitude_modifiers().len()
        && a.amplitude_modifiers()
            .iter()
            .zip(b.amplitude_modifiers().iter())
            .all(|(&x, &y)| canonical_f64_bits(x) == canonical_f64_bits(y))
}

fn params_hash<H: Hasher>(p: &NormalNoiseParameters, state: &mut H) {
    canonical_f64_bits(p.base_amplitude()).hash(state);
    p.base_octave().hash(state);
    p.octave_count().hash(state);
    p.normalize().hash(state);
    p.amplitude_modifiers().len().hash(state);
    for &m in p.amplitude_modifiers() {
        canonical_f64_bits(m).hash(state);
    }
}

/// Unresolved `Holder<NormalNoise>` decoded from JSON before registry resolution.
#[derive(Debug, Clone)]
pub(crate) enum RawNoiseHolder {
    Reference(Identifier),
    Inline(NormalNoiseParameters),
}

/// Unresolved `DensityFunction` AST decoded from JSON before resolving references into
/// `minecraft:worldgen/density_function` and `minecraft:worldgen/noise`.
#[derive(Debug, Clone)]
pub(crate) enum RawDensityFunction {
    Reference(Identifier),
    Constant(f32),
    BlendAlpha,
    BlendOffset,
    Beardifier,
    Noise {
        noise: RawNoiseHolder,
        xz_scale: f64,
        y_scale: f64,
        shift_x: Box<RawDensityFunction>,
        shift_y: Box<RawDensityFunction>,
        shift_z: Box<RawDensityFunction>,
    },
    Shift {
        kind: ShiftKind,
        noise: RawNoiseHolder,
    },
    EndOuterIslands,
    DistanceToPoint {
        point: [i32; 3],
        metric: DistanceMetric,
    },
    Gradient {
        axis: Axis3D,
        tiling: GradientTiling,
        from_coordinate: i32,
        to_coordinate: i32,
        from_value: f32,
        to_value: f32,
    },
    OldBlendedNoise(BlendedNoiseParameters),
    Unary {
        op: UnaryOp,
        input: Box<RawDensityFunction>,
    },
    Round {
        op: RoundOp,
        input: Box<RawDensityFunction>,
        multiple: Box<RawDensityFunction>,
    },
    Binary {
        op: BinaryOp,
        left: Box<RawDensityFunction>,
        right: Box<RawDensityFunction>,
    },
    Pow {
        base: Box<RawDensityFunction>,
        exponent: Box<RawDensityFunction>,
    },
    Lerp {
        alpha: Box<RawDensityFunction>,
        first: Box<RawDensityFunction>,
        second: Box<RawDensityFunction>,
    },
    Clamp {
        input: Box<RawDensityFunction>,
        min: f32,
        max: f32,
    },
    RangeChoice {
        input: Box<RawDensityFunction>,
        min_inclusive: f32,
        max_exclusive: f32,
        when_in_range: Box<RawDensityFunction>,
        when_out_of_range: Box<RawDensityFunction>,
    },
    IntervalSelect {
        input: Box<RawDensityFunction>,
        thresholds: Vec<f32>,
        functions: Vec<RawDensityFunction>,
    },
    Spline(RawCubicSpline),
    Cache(Box<RawDensityFunction>),
    BlendDensity(Box<RawDensityFunction>),
    Interpolated {
        input: Box<RawDensityFunction>,
        cell_size_xz: i32,
        cell_size_y: i32,
    },
    Slice {
        axis: Axis3D,
        coordinate: i32,
        input: Box<RawDensityFunction>,
    },
    FindTopSurface {
        density: Box<RawDensityFunction>,
        upper_bound: Box<RawDensityFunction>,
        lower_bound: i32,
        cell_height: i32,
    },
}

/// Unresolved cubic spline decoded from JSON before resolving coordinate `DensityFunction`s.
#[derive(Debug, Clone)]
pub(crate) enum RawCubicSpline {
    Constant(f32),
    Multipoint {
        coordinate: Box<RawDensityFunction>,
        locations: Vec<f32>,
        values: Vec<RawCubicSpline>,
        derivatives: Vec<f32>,
    },
}

/// Resolved immutable density function AST covering all 44 `minecraft:worldgen/density_function_type`
/// variants plus named references into `minecraft:worldgen/density_function`.
#[derive(Debug, Clone)]
pub enum DensityFunction {
    /// Named reference into `minecraft:worldgen/density_function` (`HolderHolder`).
    Reference {
        /// Registry key of the referenced density function.
        id: Identifier,
        /// Resolved target density function.
        target: Arc<DensityFunction>,
    },
    /// `minecraft:constant` (or bare JSON number).
    Constant(f32),
    /// `minecraft:blend_alpha`
    BlendAlpha,
    /// `minecraft:blend_offset`
    BlendOffset,
    /// `minecraft:beardifier`
    Beardifier,
    /// `minecraft:noise`
    Noise {
        /// Normal noise holder.
        noise: NoiseHolder,
        /// Horizontal coordinate scale.
        xz_scale: f64,
        /// Vertical coordinate scale.
        y_scale: f64,
        /// X shift function (default `Constant(0.0)`).
        shift_x: Box<DensityFunction>,
        /// Y shift function (default `Constant(0.0)`).
        shift_y: Box<DensityFunction>,
        /// Z shift function (default `Constant(0.0)`).
        shift_z: Box<DensityFunction>,
    },
    /// `minecraft:shift`, `minecraft:shift_a`, `minecraft:shift_b`.
    Shift {
        /// Shift variant.
        kind: ShiftKind,
        /// Normal noise holder.
        noise: NoiseHolder,
    },
    /// `minecraft:end_outer_islands`
    EndOuterIslands,
    /// `minecraft:distance_to_point`
    DistanceToPoint {
        /// Target `[x, y, z]` block point.
        point: [i32; 3],
        /// Distance metric.
        metric: DistanceMetric,
    },
    /// `minecraft:gradient`
    Gradient {
        /// Coordinate axis.
        axis: Axis3D,
        /// Tiling mode.
        tiling: GradientTiling,
        /// Start coordinate.
        from_coordinate: i32,
        /// End coordinate (distinct from `from_coordinate`).
        to_coordinate: i32,
        /// Value at `from_coordinate`.
        from_value: f32,
        /// Value at `to_coordinate`.
        to_value: f32,
    },
    /// `minecraft:old_blended_noise`
    OldBlendedNoise(BlendedNoiseParameters),
    /// One of the 11 unary operations (`density-functions.md` §2.2).
    Unary {
        /// Operation kind.
        op: UnaryOp,
        /// Input density function.
        input: Box<DensityFunction>,
    },
    /// One of the 4 rounding operations (`density-functions.md` §2.3).
    Round {
        /// Rounding kind.
        op: RoundOp,
        /// Input density function.
        input: Box<DensityFunction>,
        /// Quantization multiple (default `Constant(1.0)`).
        multiple: Box<DensityFunction>,
    },
    /// One of the 6 binary arithmetic / min-max operations (`density-functions.md` §2.4).
    Binary {
        /// Binary operator.
        op: BinaryOp,
        /// Left operand.
        left: Box<DensityFunction>,
        /// Right operand.
        right: Box<DensityFunction>,
    },
    /// `minecraft:pow`
    Pow {
        /// Base function.
        base: Box<DensityFunction>,
        /// Exponent function.
        exponent: Box<DensityFunction>,
    },
    /// `minecraft:lerp`
    Lerp {
        /// Interpolation factor.
        alpha: Box<DensityFunction>,
        /// Value at `alpha == 0`.
        first: Box<DensityFunction>,
        /// Value at `alpha == 1`.
        second: Box<DensityFunction>,
    },
    /// `minecraft:clamp`
    Clamp {
        /// Input function.
        input: Box<DensityFunction>,
        /// Lower clamp bound.
        min: f32,
        /// Upper clamp bound.
        max: f32,
    },
    /// `minecraft:range_choice`
    RangeChoice {
        /// Selector input.
        input: Box<DensityFunction>,
        /// Inclusive lower threshold.
        min_inclusive: f32,
        /// Exclusive upper threshold.
        max_exclusive: f32,
        /// Branch when `min_inclusive <= input < max_exclusive`.
        when_in_range: Box<DensityFunction>,
        /// Branch otherwise (including `NaN`).
        when_out_of_range: Box<DensityFunction>,
    },
    /// `minecraft:interval_select`
    IntervalSelect {
        /// Selector input.
        input: Box<DensityFunction>,
        /// Non-decreasing thresholds (`len == functions.len() - 1`).
        thresholds: Vec<f32>,
        /// Candidate functions (`len >= 2`).
        functions: Vec<DensityFunction>,
    },
    /// `minecraft:spline`
    Spline(CubicSpline<Box<DensityFunction>>),
    /// `minecraft:cache`
    Cache(Box<DensityFunction>),
    /// `minecraft:blend_density`
    BlendDensity(Box<DensityFunction>),
    /// `minecraft:interpolated`
    Interpolated {
        /// Input function sampled at cell corners.
        input: Box<DensityFunction>,
        /// Horizontal interpolation cell size (`>= 1`).
        cell_size_xz: i32,
        /// Vertical interpolation cell size (`>= 1`).
        cell_size_y: i32,
    },
    /// `minecraft:slice`
    Slice {
        /// Axis pinned to `coordinate`.
        axis: Axis3D,
        /// Fixed block coordinate along `axis`.
        coordinate: i32,
        /// Wrapped function.
        input: Box<DensityFunction>,
    },
    /// `minecraft:find_top_surface`
    FindTopSurface {
        /// Density function probed downward until `> 0.0f`.
        density: Box<DensityFunction>,
        /// Upper bound function sampled at `y = 0`.
        upper_bound: Box<DensityFunction>,
        /// Minimum Y fallback (`[-4064, 4062]`).
        lower_bound: i32,
        /// Vertical probe step (`>= 1`).
        cell_height: i32,
    },
}

impl DensityFunction {
    /// Returns the 3-bit `@Axes` bitmask of coordinates this function may depend on.
    pub fn domain_axes(&self) -> u8 {
        match self {
            Self::Reference { target, .. } => target.domain_axes(),
            Self::Constant(_) => NO_AXES,
            Self::BlendAlpha | Self::BlendOffset | Self::EndOuterIslands => AXIS_X | AXIS_Z,
            Self::Beardifier | Self::DistanceToPoint { .. } | Self::OldBlendedNoise(_) => ALL_AXES,
            Self::Noise {
                xz_scale,
                y_scale,
                shift_x,
                shift_y,
                shift_z,
                ..
            } => {
                let mut axes = ALL_AXES;
                if *y_scale == 0.0_f64 {
                    axes &= !AXIS_Y;
                }
                if *xz_scale == 0.0_f64 {
                    axes &= !(AXIS_X | AXIS_Z);
                }
                axes | shift_x.domain_axes() | shift_y.domain_axes() | shift_z.domain_axes()
            }
            Self::Shift { kind, .. } => match kind {
                ShiftKind::Shift => ALL_AXES,
                ShiftKind::ShiftA | ShiftKind::ShiftB => AXIS_X | AXIS_Z,
            },
            Self::Gradient { axis, .. } => axis.mask(),
            Self::Unary { input, .. }
            | Self::Clamp { input, .. }
            | Self::Cache(input)
            | Self::BlendDensity(input)
            | Self::Interpolated { input, .. } => input.domain_axes(),
            Self::Round {
                input, multiple, ..
            } => input.domain_axes() | multiple.domain_axes(),
            Self::Binary { left, right, .. } => left.domain_axes() | right.domain_axes(),
            Self::Pow { base, exponent } => base.domain_axes() | exponent.domain_axes(),
            Self::Lerp {
                alpha,
                first,
                second,
            } => alpha.domain_axes() | first.domain_axes() | second.domain_axes(),
            Self::RangeChoice {
                input,
                when_in_range,
                when_out_of_range,
                ..
            } => {
                input.domain_axes() | when_in_range.domain_axes() | when_out_of_range.domain_axes()
            }
            Self::IntervalSelect {
                input, functions, ..
            } => {
                let mut axes = input.domain_axes();
                for f in functions {
                    axes |= f.domain_axes();
                }
                axes
            }
            Self::Spline(spline) => spline_domain_axes(spline),
            Self::Slice { axis, input, .. } => input.domain_axes() & !axis.mask(),
            Self::FindTopSurface {
                density,
                upper_bound,
                ..
            } => (density.domain_axes() | upper_bound.domain_axes()) & !AXIS_Y,
        }
    }

    /// Returns the conservative `f32` output [`Interval`] of this function.
    pub fn range(&self) -> Interval {
        match self {
            Self::Reference { target, .. } => target.range(),
            Self::Constant(v) => of_exact(*v),
            Self::BlendAlpha => of(0.0_f32, 1.0_f32),
            Self::BlendOffset | Self::Beardifier => INFINITE,
            Self::Noise { noise, .. } => noise.params().range(),
            Self::Shift { noise, .. } => interval::mul(noise.params().range(), of_exact(4.0_f32)),
            Self::EndOuterIslands => of(-0.84375_f32, 0.5625_f32),
            Self::DistanceToPoint { .. } => of(0.0_f32, f32::INFINITY),
            Self::Gradient {
                from_value,
                to_value,
                ..
            } => interval::encapsulating_pair(*from_value, *to_value),
            Self::OldBlendedNoise(params) => params.range(),
            Self::Unary { op, input } => op.range(input.range()),
            Self::Round {
                op,
                input,
                multiple,
            } => {
                let m_range = multiple.range();
                let divided = interval::div(input.range(), m_range);
                let rounded = interval::map_monotonic(divided, |v| op.round_to_int(v));
                interval::mul(rounded, m_range)
            }
            Self::Binary { op, left, right } => op.range(left.range(), right.range()),
            Self::Pow { base, exponent } => interval::pow(base.range(), exponent.range()),
            Self::Lerp {
                alpha,
                first,
                second,
            } => interval::lerp(alpha.range(), first.range(), second.range()),
            Self::Clamp { input, min, max } => interval::clamp(input.range(), *min, *max),
            Self::RangeChoice {
                when_in_range,
                when_out_of_range,
                ..
            } => interval::encapsulating(&[when_in_range.range(), when_out_of_range.range()]),
            Self::IntervalSelect { functions, .. } => {
                let ranges: Vec<Interval> = functions.iter().map(DensityFunction::range).collect();
                interval::encapsulating(&ranges)
            }
            Self::Spline(spline) => spline.range(),
            Self::Cache(input)
            | Self::BlendDensity(input)
            | Self::Interpolated { input, .. }
            | Self::Slice { input, .. } => input.range(),
            Self::FindTopSurface {
                upper_bound,
                lower_bound,
                ..
            } => {
                let lb = *lower_bound as f32;
                of(lb, java_max_f32(lb, upper_bound.range().max()))
            }
        }
    }
}

fn spline_domain_axes(spline: &CubicSpline<Box<DensityFunction>>) -> u8 {
    match spline {
        CubicSpline::Constant(_) => NO_AXES,
        CubicSpline::Multipoint {
            coordinate, values, ..
        } => {
            let mut axes = coordinate.domain_axes();
            for v in values {
                axes |= spline_domain_axes(v);
            }
            axes
        }
    }
}

impl PartialEq for DensityFunction {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Reference { id: a, .. }, Self::Reference { id: b, .. }) => a == b,
            (Self::Constant(a), Self::Constant(b)) => {
                canonical_f32_bits(*a) == canonical_f32_bits(*b)
            }
            (Self::BlendAlpha, Self::BlendAlpha)
            | (Self::BlendOffset, Self::BlendOffset)
            | (Self::Beardifier, Self::Beardifier)
            | (Self::EndOuterIslands, Self::EndOuterIslands) => true,
            (
                Self::Noise {
                    noise: n1,
                    xz_scale: x1,
                    y_scale: y1,
                    shift_x: sx1,
                    shift_y: sy1,
                    shift_z: sz1,
                },
                Self::Noise {
                    noise: n2,
                    xz_scale: x2,
                    y_scale: y2,
                    shift_x: sx2,
                    shift_y: sy2,
                    shift_z: sz2,
                },
            ) => {
                n1 == n2
                    && canonical_f64_bits(*x1) == canonical_f64_bits(*x2)
                    && canonical_f64_bits(*y1) == canonical_f64_bits(*y2)
                    && sx1 == sx2
                    && sy1 == sy2
                    && sz1 == sz2
            }
            (
                Self::Shift {
                    kind: k1,
                    noise: n1,
                },
                Self::Shift {
                    kind: k2,
                    noise: n2,
                },
            ) => k1 == k2 && n1 == n2,
            (
                Self::DistanceToPoint {
                    point: p1,
                    metric: m1,
                },
                Self::DistanceToPoint {
                    point: p2,
                    metric: m2,
                },
            ) => p1 == p2 && m1 == m2,
            (
                Self::Gradient {
                    axis: a1,
                    tiling: t1,
                    from_coordinate: fc1,
                    to_coordinate: tc1,
                    from_value: fv1,
                    to_value: tv1,
                },
                Self::Gradient {
                    axis: a2,
                    tiling: t2,
                    from_coordinate: fc2,
                    to_coordinate: tc2,
                    from_value: fv2,
                    to_value: tv2,
                },
            ) => {
                a1 == a2
                    && t1 == t2
                    && fc1 == fc2
                    && tc1 == tc2
                    && canonical_f32_bits(*fv1) == canonical_f32_bits(*fv2)
                    && canonical_f32_bits(*tv1) == canonical_f32_bits(*tv2)
            }
            (Self::OldBlendedNoise(a), Self::OldBlendedNoise(b)) => {
                canonical_f64_bits(a.xz_scale()) == canonical_f64_bits(b.xz_scale())
                    && canonical_f64_bits(a.y_scale()) == canonical_f64_bits(b.y_scale())
                    && canonical_f64_bits(a.xz_factor()) == canonical_f64_bits(b.xz_factor())
                    && canonical_f64_bits(a.y_factor()) == canonical_f64_bits(b.y_factor())
                    && canonical_f64_bits(a.smear_scale_multiplier())
                        == canonical_f64_bits(b.smear_scale_multiplier())
            }
            (Self::Unary { op: o1, input: i1 }, Self::Unary { op: o2, input: i2 }) => {
                o1 == o2 && i1 == i2
            }
            (
                Self::Round {
                    op: o1,
                    input: i1,
                    multiple: m1,
                },
                Self::Round {
                    op: o2,
                    input: i2,
                    multiple: m2,
                },
            ) => o1 == o2 && i1 == i2 && m1 == m2,
            (
                Self::Binary {
                    op: o1,
                    left: l1,
                    right: r1,
                },
                Self::Binary {
                    op: o2,
                    left: l2,
                    right: r2,
                },
            ) => o1 == o2 && l1 == l2 && r1 == r2,
            (
                Self::Pow {
                    base: b1,
                    exponent: e1,
                },
                Self::Pow {
                    base: b2,
                    exponent: e2,
                },
            ) => b1 == b2 && e1 == e2,
            (
                Self::Lerp {
                    alpha: a1,
                    first: f1,
                    second: s1,
                },
                Self::Lerp {
                    alpha: a2,
                    first: f2,
                    second: s2,
                },
            ) => a1 == a2 && f1 == f2 && s1 == s2,
            (
                Self::Clamp {
                    input: i1,
                    min: lo1,
                    max: hi1,
                },
                Self::Clamp {
                    input: i2,
                    min: lo2,
                    max: hi2,
                },
            ) => {
                i1 == i2
                    && canonical_f32_bits(*lo1) == canonical_f32_bits(*lo2)
                    && canonical_f32_bits(*hi1) == canonical_f32_bits(*hi2)
            }
            (
                Self::RangeChoice {
                    input: i1,
                    min_inclusive: lo1,
                    max_exclusive: hi1,
                    when_in_range: in1,
                    when_out_of_range: out1,
                },
                Self::RangeChoice {
                    input: i2,
                    min_inclusive: lo2,
                    max_exclusive: hi2,
                    when_in_range: in2,
                    when_out_of_range: out2,
                },
            ) => {
                i1 == i2
                    && canonical_f32_bits(*lo1) == canonical_f32_bits(*lo2)
                    && canonical_f32_bits(*hi1) == canonical_f32_bits(*hi2)
                    && in1 == in2
                    && out1 == out2
            }
            (
                Self::IntervalSelect {
                    input: i1,
                    thresholds: t1,
                    functions: f1,
                },
                Self::IntervalSelect {
                    input: i2,
                    thresholds: t2,
                    functions: f2,
                },
            ) => {
                i1 == i2
                    && t1.len() == t2.len()
                    && t1
                        .iter()
                        .zip(t2.iter())
                        .all(|(&a, &b)| canonical_f32_bits(a) == canonical_f32_bits(b))
                    && f1 == f2
            }
            (Self::Spline(a), Self::Spline(b)) => a == b,
            (Self::Cache(a), Self::Cache(b)) => a == b,
            (Self::BlendDensity(a), Self::BlendDensity(b)) => a == b,
            (
                Self::Interpolated {
                    input: i1,
                    cell_size_xz: xz1,
                    cell_size_y: y1,
                },
                Self::Interpolated {
                    input: i2,
                    cell_size_xz: xz2,
                    cell_size_y: y2,
                },
            ) => i1 == i2 && xz1 == xz2 && y1 == y2,
            (
                Self::Slice {
                    axis: a1,
                    coordinate: c1,
                    input: i1,
                },
                Self::Slice {
                    axis: a2,
                    coordinate: c2,
                    input: i2,
                },
            ) => a1 == a2 && c1 == c2 && i1 == i2,
            (
                Self::FindTopSurface {
                    density: d1,
                    upper_bound: u1,
                    lower_bound: l1,
                    cell_height: ch1,
                },
                Self::FindTopSurface {
                    density: d2,
                    upper_bound: u2,
                    lower_bound: l2,
                    cell_height: ch2,
                },
            ) => d1 == d2 && u1 == u2 && l1 == l2 && ch1 == ch2,
            _ => false,
        }
    }
}

impl Eq for DensityFunction {}

impl Hash for DensityFunction {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Self::Reference { id, .. } => id.hash(state),
            Self::Constant(v) => canonical_f32_bits(*v).hash(state),
            Self::BlendAlpha | Self::BlendOffset | Self::Beardifier | Self::EndOuterIslands => {}
            Self::Noise {
                noise,
                xz_scale,
                y_scale,
                shift_x,
                shift_y,
                shift_z,
            } => {
                noise.hash(state);
                canonical_f64_bits(*xz_scale).hash(state);
                canonical_f64_bits(*y_scale).hash(state);
                shift_x.hash(state);
                shift_y.hash(state);
                shift_z.hash(state);
            }
            Self::Shift { kind, noise } => {
                kind.hash(state);
                noise.hash(state);
            }
            Self::DistanceToPoint { point, metric } => {
                point.hash(state);
                metric.hash(state);
            }
            Self::Gradient {
                axis,
                tiling,
                from_coordinate,
                to_coordinate,
                from_value,
                to_value,
            } => {
                axis.hash(state);
                tiling.hash(state);
                from_coordinate.hash(state);
                to_coordinate.hash(state);
                canonical_f32_bits(*from_value).hash(state);
                canonical_f32_bits(*to_value).hash(state);
            }
            Self::OldBlendedNoise(p) => {
                canonical_f64_bits(p.xz_scale()).hash(state);
                canonical_f64_bits(p.y_scale()).hash(state);
                canonical_f64_bits(p.xz_factor()).hash(state);
                canonical_f64_bits(p.y_factor()).hash(state);
                canonical_f64_bits(p.smear_scale_multiplier()).hash(state);
            }
            Self::Unary { op, input } => {
                op.hash(state);
                input.hash(state);
            }
            Self::Round {
                op,
                input,
                multiple,
            } => {
                op.hash(state);
                input.hash(state);
                multiple.hash(state);
            }
            Self::Binary { op, left, right } => {
                op.hash(state);
                left.hash(state);
                right.hash(state);
            }
            Self::Pow { base, exponent } => {
                base.hash(state);
                exponent.hash(state);
            }
            Self::Lerp {
                alpha,
                first,
                second,
            } => {
                alpha.hash(state);
                first.hash(state);
                second.hash(state);
            }
            Self::Clamp { input, min, max } => {
                input.hash(state);
                canonical_f32_bits(*min).hash(state);
                canonical_f32_bits(*max).hash(state);
            }
            Self::RangeChoice {
                input,
                min_inclusive,
                max_exclusive,
                when_in_range,
                when_out_of_range,
            } => {
                input.hash(state);
                canonical_f32_bits(*min_inclusive).hash(state);
                canonical_f32_bits(*max_exclusive).hash(state);
                when_in_range.hash(state);
                when_out_of_range.hash(state);
            }
            Self::IntervalSelect {
                input,
                thresholds,
                functions,
            } => {
                input.hash(state);
                thresholds.len().hash(state);
                for &t in thresholds {
                    canonical_f32_bits(t).hash(state);
                }
                functions.hash(state);
            }
            Self::Spline(s) => s.hash(state),
            Self::Cache(i) | Self::BlendDensity(i) => i.hash(state),
            Self::Interpolated {
                input,
                cell_size_xz,
                cell_size_y,
            } => {
                input.hash(state);
                cell_size_xz.hash(state);
                cell_size_y.hash(state);
            }
            Self::Slice {
                axis,
                coordinate,
                input,
            } => {
                axis.hash(state);
                coordinate.hash(state);
                input.hash(state);
            }
            Self::FindTopSurface {
                density,
                upper_bound,
                lower_bound,
                cell_height,
            } => {
                density.hash(state);
                upper_bound.hash(state);
                lower_bound.hash(state);
                cell_height.hash(state);
            }
        }
    }
}
