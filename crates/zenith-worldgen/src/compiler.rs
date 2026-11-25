//! Two-pass optimizer (`DensityFunctionCompiler`: reference inlining + cache deduplication,
//! `SliceUniformAxes`) and compiled `f32` point/volume evaluator (`CompiledSampler`).
//!
//! Implements `docs/specs/worldgen/density-functions.md` §1, §2, §3, and §6.2.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use thiserror::Error;
use zenith_math::helpers::interp::{lerp_f32, lerp3_f32};
use zenith_math::helpers::packing::block_as_long;
use zenith_math::helpers::roots::sqrt;
use zenith_math::helpers::rounding::{
    clamp_f32, clamp_i32, floor_f32, java_floor_div_i32, java_floor_mod_i32, java_max_f32,
    java_min_f32,
};
use zenith_math::{
    BlendedNoise, BlendedNoiseParameters, Interval, NoiseError, NormalNoise, SimplexNoise,
};
use zenith_registry::Identifier;

use crate::ast::{
    Axis3D, BinaryOp, DensityFunction, DistanceMetric, GradientTiling, NoiseHolder, RoundOp,
    ShiftKind, UnaryOp,
};
use crate::interval::{
    self, INFINITE, canonical_f32_bits, canonical_f64_bits, java_pow_f64, of, of_exact,
};
use crate::spline::CubicSpline;
use crate::volume::{ALL_AXES, AXIS_X, AXIS_Y, AXIS_Z, DensityVolume, NO_AXES, SamplerContext};

/// Error returned while compiling density functions or initializing [`crate::RandomState`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CompileError {
    /// The requested `worldgen/noise_settings` identifier was not present in `WorldgenData`.
    #[error("unknown noise_settings '{id}'")]
    UnknownNoiseSettings {
        /// Missing settings identifier.
        id: Identifier,
    },
    /// `RandomState` encountered an unregistered inline `NormalNoise` definition
    /// (`density-functions.md` §4 step 2 / Edge Case #11).
    #[error("inline (unregistered) NormalNoise is not supported by RandomState")]
    UnregisteredInlineNoise,
    /// Underlying noise instantiation failed (e.g. invalid legacy Nether octave range).
    #[error("noise instantiation error: {0}")]
    Noise(String),
}

impl From<NoiseError> for CompileError {
    fn from(e: NoiseError) -> Self {
        Self::Noise(e.to_string())
    }
}

/// Seeding hooks provided by [`crate::RandomState`] to [`DensityFunctionCompiler`].
pub(crate) trait CompileBackend {
    fn create_noise_sampler(
        &mut self,
        holder: &NoiseHolder,
    ) -> Result<Arc<NormalNoise>, CompileError>;
    fn create_blended_noise(&mut self, params: BlendedNoiseParameters) -> BlendedNoise;
    fn create_end_island_noise(&mut self) -> SimplexNoise;
}

/// A compiled, immutable density-function sampler supporting both point and volume evaluation.
#[derive(Debug, Clone)]
pub struct CompiledSampler {
    root: Arc<CompiledNode>,
    range: Interval,
    domain_axes: u8,
}

impl CompiledSampler {
    /// Evaluates a single block point `(bx, by, bz)`.
    #[inline]
    pub fn sample_value(&self, ctx: &mut SamplerContext, bx: i32, by: i32, bz: i32) -> f32 {
        self.root.sample_value(ctx, bx, by, bz)
    }

    /// Evaluates over `volume`, writing `volume.len()` floats into `out[..volume.len()]` in
    /// Z-outer, X-middle, Y-inner layout (`index(ix, iy, iz) = iy + (ix + iz * size_x) * size_y`).
    ///
    /// # Panics
    ///
    /// Panics if `out.len() < volume.len()`.
    #[inline]
    pub fn sample_volume(&self, ctx: &mut SamplerContext, out: &mut [f32], volume: DensityVolume) {
        assert!(
            out.len() >= volume.len(),
            "output buffer length {} is smaller than volume size {}",
            out.len(),
            volume.len()
        );
        self.root
            .sample_volume(ctx, &mut out[..volume.len()], volume);
    }

    /// Conservative `f32` output interval reported by the compiled function.
    #[inline]
    pub const fn range(&self) -> Interval {
        self.range
    }

    /// 3-bit `@Axes` bitmask of coordinates this function depends on.
    #[inline]
    pub const fn domain_axes(&self) -> u8 {
        self.domain_axes
    }
}

// ---------------------------------------------------------------------------
// Internal post-pass-1 / post-pass-2 AST (`OptNode`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum OptNode {
    PreparedCache {
        id: usize,
        range: Interval,
        domain_axes: u8,
        sampler: Arc<CompiledNode>,
    },
    Constant(f32),
    BlendAlpha,
    BlendOffset,
    Beardifier,
    Noise {
        noise: NoiseHolder,
        xz_scale: f64,
        y_scale: f64,
        shift_x: Box<OptNode>,
        shift_y: Box<OptNode>,
        shift_z: Box<OptNode>,
    },
    Shift {
        kind: ShiftKind,
        noise: NoiseHolder,
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
        input: Box<OptNode>,
    },
    Round {
        op: RoundOp,
        input: Box<OptNode>,
        multiple: Box<OptNode>,
    },
    Binary {
        op: BinaryOp,
        left: Box<OptNode>,
        right: Box<OptNode>,
    },
    Pow {
        base: Box<OptNode>,
        exponent: Box<OptNode>,
    },
    Lerp {
        alpha: Box<OptNode>,
        first: Box<OptNode>,
        second: Box<OptNode>,
    },
    Clamp {
        input: Box<OptNode>,
        min: f32,
        max: f32,
    },
    RangeChoice {
        input: Box<OptNode>,
        min_inclusive: f32,
        max_exclusive: f32,
        when_in_range: Box<OptNode>,
        when_out_of_range: Box<OptNode>,
    },
    IntervalSelect {
        input: Box<OptNode>,
        thresholds: Vec<f32>,
        functions: Vec<OptNode>,
    },
    Spline(CubicSpline<Box<OptNode>>),
    BlendDensity(Box<OptNode>),
    Interpolated {
        input: Box<OptNode>,
        cell_size_xz: i32,
        cell_size_y: i32,
    },
    Slice {
        axis: Axis3D,
        coordinate: i32,
        input: Box<OptNode>,
    },
    FindTopSurface {
        density: Box<OptNode>,
        upper_bound: Box<OptNode>,
        lower_bound: i32,
        cell_height: i32,
    },
}

impl OptNode {
    fn domain_axes(&self) -> u8 {
        match self {
            Self::PreparedCache { domain_axes, .. } => *domain_axes,
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
            Self::Spline(s) => opt_spline_domain_axes(s),
            Self::Slice { axis, input, .. } => input.domain_axes() & !axis.mask(),
            Self::FindTopSurface {
                density,
                upper_bound,
                ..
            } => (density.domain_axes() | upper_bound.domain_axes()) & !AXIS_Y,
        }
    }

    fn range(&self) -> Interval {
        match self {
            Self::PreparedCache { range, .. } => *range,
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
            Self::OldBlendedNoise(p) => p.range(),
            Self::Unary { op, input } => op.range(input.range()),
            Self::Round {
                op,
                input,
                multiple,
            } => {
                let m = multiple.range();
                let d = interval::div(input.range(), m);
                let r = interval::map_monotonic(d, |v| op.round_to_int(v));
                interval::mul(r, m)
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
                let ranges: Vec<Interval> = functions.iter().map(OptNode::range).collect();
                interval::encapsulating(&ranges)
            }
            Self::Spline(s) => s.range(),
            Self::BlendDensity(input)
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

    #[inline]
    fn is_positive_zero_const(&self) -> bool {
        matches!(self, Self::Constant(v) if v.to_bits() == 0.0_f32.to_bits())
    }

    #[inline]
    fn is_one_const(&self) -> bool {
        matches!(self, Self::Constant(v) if v.to_bits() == 1.0_f32.to_bits())
    }
}

fn opt_spline_domain_axes(spline: &CubicSpline<Box<OptNode>>) -> u8 {
    match spline {
        CubicSpline::Constant(_) => NO_AXES,
        CubicSpline::Multipoint {
            coordinate, values, ..
        } => {
            let mut axes = coordinate.domain_axes();
            for v in values {
                axes |= opt_spline_domain_axes(v);
            }
            axes
        }
    }
}

impl PartialEq for OptNode {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::PreparedCache { id: a, .. }, Self::PreparedCache { id: b, .. }) => a == b,
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

impl Eq for OptNode {}

impl Hash for OptNode {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Self::PreparedCache { id, .. } => id.hash(state),
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
            Self::BlendDensity(i) => i.hash(state),
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

// ---------------------------------------------------------------------------
// Two-pass compiler (`DensityFunctionCompiler`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct PreparedCacheEntry {
    id: usize,
    range: Interval,
    domain_axes: u8,
    sampler: Arc<CompiledNode>,
}

/// Compiler implementing Step 1A (reference inlining & `cache` deduplication),
/// Step 1B (`SliceUniformAxes`), and Step 1C (specialization to `CompiledNode`).
#[derive(Debug, Default)]
pub(crate) struct DensityFunctionCompiler {
    next_cache_id: usize,
    prepared_caches: HashMap<DensityFunction, PreparedCacheEntry>,
    memoized: HashMap<DensityFunction, CompiledSampler>,
}

impl DensityFunctionCompiler {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn cache_count(&self) -> usize {
        self.next_cache_id
    }

    /// Compiles (or retrieves memoized) sampler for `func`.
    pub(crate) fn get_sampler(
        &mut self,
        func: &DensityFunction,
        backend: &mut dyn CompileBackend,
    ) -> Result<CompiledSampler, CompileError> {
        if let Some(existing) = self.memoized.get(func) {
            return Ok(existing.clone());
        }
        let opt = self.optimize(func, backend)?;
        let range = opt.range();
        let domain_axes = opt.domain_axes();
        let root = Arc::new(self.compile_node(&opt, backend)?);
        let sampler = CompiledSampler {
            root,
            range,
            domain_axes,
        };
        self.memoized.insert(func.clone(), sampler.clone());
        Ok(sampler)
    }

    /// Runs Pass 1 (inlining & cache deduplication) followed by Pass 2 (`SliceUniformAxes`).
    fn optimize(
        &mut self,
        func: &DensityFunction,
        backend: &mut dyn CompileBackend,
    ) -> Result<OptNode, CompileError> {
        let pass1 = self.pass1_inline_and_dedup(func, backend)?;
        Ok(slice_uniform_axes(pass1, ALL_AXES))
    }

    /// Step 1A: unwrap `Reference` (`HolderHolder`) and deduplicate `Cache` subtrees.
    fn pass1_inline_and_dedup(
        &mut self,
        mut func: &DensityFunction,
        backend: &mut dyn CompileBackend,
    ) -> Result<OptNode, CompileError> {
        // 1. Unwrap any chained HolderHolder references.
        while let DensityFunction::Reference { target, .. } = func {
            func = target;
        }

        // 2. CacheFunction deduplication keyed on the UN-REWRITTEN child.
        if let DensityFunction::Cache(input) = func {
            if let Some(entry) = self.prepared_caches.get(input.as_ref()) {
                return Ok(OptNode::PreparedCache {
                    id: entry.id,
                    range: entry.range,
                    domain_axes: entry.domain_axes,
                    sampler: Arc::clone(&entry.sampler),
                });
            }
            let id = self.next_cache_id;
            self.next_cache_id += 1;
            let opt_input = self.optimize(input, backend)?;
            let range = opt_input.range();
            let domain_axes = opt_input.domain_axes();
            let compiled_input = Arc::new(self.compile_node(&opt_input, backend)?);
            let caching_sampler = Arc::new(CompiledNode::Cache {
                id,
                input: compiled_input,
            });
            let entry = PreparedCacheEntry {
                id,
                range,
                domain_axes,
                sampler: Arc::clone(&caching_sampler),
            };
            self.prepared_caches.insert(input.as_ref().clone(), entry);
            return Ok(OptNode::PreparedCache {
                id,
                range,
                domain_axes,
                sampler: caching_sampler,
            });
        }

        // 3. Recursively rewrite children.
        Ok(match func {
            DensityFunction::Reference { .. } | DensityFunction::Cache(_) => {
                unreachable!("handled above")
            }
            DensityFunction::Constant(v) => OptNode::Constant(*v),
            DensityFunction::BlendAlpha => OptNode::BlendAlpha,
            DensityFunction::BlendOffset => OptNode::BlendOffset,
            DensityFunction::Beardifier => OptNode::Beardifier,
            DensityFunction::Noise {
                noise,
                xz_scale,
                y_scale,
                shift_x,
                shift_y,
                shift_z,
            } => OptNode::Noise {
                noise: noise.clone(),
                xz_scale: *xz_scale,
                y_scale: *y_scale,
                shift_x: Box::new(self.pass1_inline_and_dedup(shift_x, backend)?),
                shift_y: Box::new(self.pass1_inline_and_dedup(shift_y, backend)?),
                shift_z: Box::new(self.pass1_inline_and_dedup(shift_z, backend)?),
            },
            DensityFunction::Shift { kind, noise } => OptNode::Shift {
                kind: *kind,
                noise: noise.clone(),
            },
            DensityFunction::EndOuterIslands => OptNode::EndOuterIslands,
            DensityFunction::DistanceToPoint { point, metric } => OptNode::DistanceToPoint {
                point: *point,
                metric: *metric,
            },
            DensityFunction::Gradient {
                axis,
                tiling,
                from_coordinate,
                to_coordinate,
                from_value,
                to_value,
            } => OptNode::Gradient {
                axis: *axis,
                tiling: *tiling,
                from_coordinate: *from_coordinate,
                to_coordinate: *to_coordinate,
                from_value: *from_value,
                to_value: *to_value,
            },
            DensityFunction::OldBlendedNoise(p) => OptNode::OldBlendedNoise(*p),
            DensityFunction::Unary { op, input } => OptNode::Unary {
                op: *op,
                input: Box::new(self.pass1_inline_and_dedup(input, backend)?),
            },
            DensityFunction::Round {
                op,
                input,
                multiple,
            } => OptNode::Round {
                op: *op,
                input: Box::new(self.pass1_inline_and_dedup(input, backend)?),
                multiple: Box::new(self.pass1_inline_and_dedup(multiple, backend)?),
            },
            DensityFunction::Binary { op, left, right } => OptNode::Binary {
                op: *op,
                left: Box::new(self.pass1_inline_and_dedup(left, backend)?),
                right: Box::new(self.pass1_inline_and_dedup(right, backend)?),
            },
            DensityFunction::Pow { base, exponent } => OptNode::Pow {
                base: Box::new(self.pass1_inline_and_dedup(base, backend)?),
                exponent: Box::new(self.pass1_inline_and_dedup(exponent, backend)?),
            },
            DensityFunction::Lerp {
                alpha,
                first,
                second,
            } => OptNode::Lerp {
                alpha: Box::new(self.pass1_inline_and_dedup(alpha, backend)?),
                first: Box::new(self.pass1_inline_and_dedup(first, backend)?),
                second: Box::new(self.pass1_inline_and_dedup(second, backend)?),
            },
            DensityFunction::Clamp { input, min, max } => OptNode::Clamp {
                input: Box::new(self.pass1_inline_and_dedup(input, backend)?),
                min: *min,
                max: *max,
            },
            DensityFunction::RangeChoice {
                input,
                min_inclusive,
                max_exclusive,
                when_in_range,
                when_out_of_range,
            } => OptNode::RangeChoice {
                input: Box::new(self.pass1_inline_and_dedup(input, backend)?),
                min_inclusive: *min_inclusive,
                max_exclusive: *max_exclusive,
                when_in_range: Box::new(self.pass1_inline_and_dedup(when_in_range, backend)?),
                when_out_of_range: Box::new(
                    self.pass1_inline_and_dedup(when_out_of_range, backend)?,
                ),
            },
            DensityFunction::IntervalSelect {
                input,
                thresholds,
                functions,
            } => {
                let opt_in = Box::new(self.pass1_inline_and_dedup(input, backend)?);
                let mut opt_fns = Vec::with_capacity(functions.len());
                for f in functions {
                    opt_fns.push(self.pass1_inline_and_dedup(f, backend)?);
                }
                OptNode::IntervalSelect {
                    input: opt_in,
                    thresholds: thresholds.clone(),
                    functions: opt_fns,
                }
            }
            DensityFunction::Spline(spline) => {
                let opt_spline = spline.try_map_coordinates(&mut |coord| {
                    let opt = self.pass1_inline_and_dedup(coord, backend)?;
                    let r = opt.range();
                    Ok::<_, CompileError>((Box::new(opt), r))
                })?;
                OptNode::Spline(opt_spline)
            }
            DensityFunction::BlendDensity(input) => {
                OptNode::BlendDensity(Box::new(self.pass1_inline_and_dedup(input, backend)?))
            }
            DensityFunction::Interpolated {
                input,
                cell_size_xz,
                cell_size_y,
            } => OptNode::Interpolated {
                input: Box::new(self.pass1_inline_and_dedup(input, backend)?),
                cell_size_xz: *cell_size_xz,
                cell_size_y: *cell_size_y,
            },
            DensityFunction::Slice {
                axis,
                coordinate,
                input,
            } => OptNode::Slice {
                axis: *axis,
                coordinate: *coordinate,
                input: Box::new(self.pass1_inline_and_dedup(input, backend)?),
            },
            DensityFunction::FindTopSurface {
                density,
                upper_bound,
                lower_bound,
                cell_height,
            } => OptNode::FindTopSurface {
                density: Box::new(self.pass1_inline_and_dedup(density, backend)?),
                upper_bound: Box::new(self.pass1_inline_and_dedup(upper_bound, backend)?),
                lower_bound: *lower_bound,
                cell_height: *cell_height,
            },
        })
    }

    /// Step 1C: compile optimized AST to `CompiledNode`.
    fn compile_node(
        &self,
        node: &OptNode,
        backend: &mut dyn CompileBackend,
    ) -> Result<CompiledNode, CompileError> {
        Ok(match node {
            OptNode::PreparedCache { sampler, .. } => CompiledNode::Shared(Arc::clone(sampler)),
            OptNode::Constant(v) => CompiledNode::Constant(*v),
            OptNode::BlendAlpha => CompiledNode::BlendAlpha,
            OptNode::BlendOffset => CompiledNode::BlendOffset,
            OptNode::Beardifier => CompiledNode::Beardifier,
            OptNode::Noise {
                noise,
                xz_scale,
                y_scale,
                shift_x,
                shift_y,
                shift_z,
            } => {
                let sampler = backend.create_noise_sampler(noise)?;
                if shift_x.is_positive_zero_const()
                    && shift_y.is_positive_zero_const()
                    && shift_z.is_positive_zero_const()
                {
                    CompiledNode::NoiseUnshifted {
                        noise: sampler,
                        xz_scale: *xz_scale,
                        y_scale: *y_scale,
                    }
                } else if shift_y.is_positive_zero_const() {
                    CompiledNode::NoiseShiftedXz {
                        noise: sampler,
                        xz_scale: *xz_scale,
                        y_scale: *y_scale,
                        shift_x: Box::new(self.compile_node(shift_x, backend)?),
                        shift_z: Box::new(self.compile_node(shift_z, backend)?),
                    }
                } else {
                    CompiledNode::NoiseShiftedXyz {
                        noise: sampler,
                        xz_scale: *xz_scale,
                        y_scale: *y_scale,
                        shift_x: Box::new(self.compile_node(shift_x, backend)?),
                        shift_y: Box::new(self.compile_node(shift_y, backend)?),
                        shift_z: Box::new(self.compile_node(shift_z, backend)?),
                    }
                }
            }
            OptNode::Shift { kind, noise } => {
                let sampler = backend.create_noise_sampler(noise)?;
                match kind {
                    ShiftKind::Shift => CompiledNode::ConstMul {
                        input: Box::new(CompiledNode::NoiseUnshifted {
                            noise: sampler,
                            xz_scale: 0.25_f64,
                            y_scale: 0.25_f64,
                        }),
                        constant: 4.0_f32,
                    },
                    ShiftKind::ShiftA => CompiledNode::ConstMul {
                        input: Box::new(CompiledNode::NoiseUnshifted {
                            noise: sampler,
                            xz_scale: 0.25_f64,
                            y_scale: 0.0_f64,
                        }),
                        constant: 4.0_f32,
                    },
                    ShiftKind::ShiftB => CompiledNode::ShiftB { noise: sampler },
                }
            }
            OptNode::EndOuterIslands => CompiledNode::EndOuterIslands {
                noise: backend.create_end_island_noise(),
            },
            OptNode::DistanceToPoint { point, metric } => CompiledNode::DistanceToPoint {
                point: *point,
                metric: *metric,
            },
            OptNode::Gradient {
                axis,
                tiling,
                from_coordinate,
                to_coordinate,
                from_value,
                to_value,
            } => {
                let coord_range = to_coordinate.wrapping_sub(*from_coordinate);
                let factor = (*to_value - *from_value) / (coord_range as f32);
                CompiledNode::Gradient {
                    axis: *axis,
                    tiling: *tiling,
                    from_coordinate: *from_coordinate,
                    to_coordinate: *to_coordinate,
                    from_value: *from_value,
                    coord_range,
                    factor,
                }
            }
            OptNode::OldBlendedNoise(params) => {
                CompiledNode::OldBlendedNoise(Box::new(backend.create_blended_noise(*params)))
            }
            OptNode::Unary { op, input } => CompiledNode::Unary {
                op: *op,
                input: Box::new(self.compile_node(input, backend)?),
            },
            OptNode::Round {
                op,
                input,
                multiple,
            } => {
                let compiled_in = Box::new(self.compile_node(input, backend)?);
                if multiple.is_one_const() {
                    CompiledNode::RoundUnit {
                        op: *op,
                        input: compiled_in,
                    }
                } else {
                    CompiledNode::RoundMultiple {
                        op: *op,
                        input: compiled_in,
                        multiple: Box::new(self.compile_node(multiple, backend)?),
                    }
                }
            }
            OptNode::Binary { op, left, right } => match op {
                BinaryOp::Add => {
                    if let OptNode::Constant(c) = **left {
                        CompiledNode::ConstAdd {
                            input: Box::new(self.compile_node(right, backend)?),
                            constant: c,
                        }
                    } else if let OptNode::Constant(c) = **right {
                        CompiledNode::ConstAdd {
                            input: Box::new(self.compile_node(left, backend)?),
                            constant: c,
                        }
                    } else {
                        CompiledNode::Add {
                            left: Box::new(self.compile_node(left, backend)?),
                            right: Box::new(self.compile_node(right, backend)?),
                        }
                    }
                }
                BinaryOp::Sub => {
                    if let OptNode::Constant(c) = **left {
                        CompiledNode::ConstSub {
                            constant: c,
                            input: Box::new(self.compile_node(right, backend)?),
                        }
                    } else if let OptNode::Constant(c) = **right {
                        CompiledNode::ConstAdd {
                            input: Box::new(self.compile_node(left, backend)?),
                            constant: -c,
                        }
                    } else {
                        CompiledNode::Sub {
                            left: Box::new(self.compile_node(left, backend)?),
                            right: Box::new(self.compile_node(right, backend)?),
                        }
                    }
                }
                BinaryOp::Mul => {
                    if let OptNode::Constant(c) = **left {
                        CompiledNode::ConstMul {
                            input: Box::new(self.compile_node(right, backend)?),
                            constant: c,
                        }
                    } else if let OptNode::Constant(c) = **right {
                        CompiledNode::ConstMul {
                            input: Box::new(self.compile_node(left, backend)?),
                            constant: c,
                        }
                    } else {
                        CompiledNode::Mul {
                            left: Box::new(self.compile_node(left, backend)?),
                            right: Box::new(self.compile_node(right, backend)?),
                        }
                    }
                }
                BinaryOp::Div => {
                    if let OptNode::Constant(c) = **left {
                        CompiledNode::ConstDiv {
                            constant: c,
                            input: Box::new(self.compile_node(right, backend)?),
                        }
                    } else if let OptNode::Constant(c) = **right {
                        // Spec §2.4 & Edge Case #5: right constant compiles to multiply by reciprocal.
                        CompiledNode::ConstMul {
                            input: Box::new(self.compile_node(left, backend)?),
                            constant: 1.0_f32 / c,
                        }
                    } else {
                        CompiledNode::Div {
                            left: Box::new(self.compile_node(left, backend)?),
                            right: Box::new(self.compile_node(right, backend)?),
                        }
                    }
                }
                BinaryOp::Min => {
                    let lr = left.range();
                    let rr = right.range();
                    if let (Some((l_min, l_max)), Some((r_min, r_max))) = (lr.bounds(), rr.bounds())
                    {
                        if l_max < r_min {
                            return self.compile_node(left, backend);
                        }
                        if r_max < l_min {
                            return self.compile_node(right, backend);
                        }
                    }
                    if let OptNode::Constant(c) = **left {
                        CompiledNode::ConstMin {
                            input: Box::new(self.compile_node(right, backend)?),
                            constant: c,
                        }
                    } else if let OptNode::Constant(c) = **right {
                        CompiledNode::ConstMin {
                            input: Box::new(self.compile_node(left, backend)?),
                            constant: c,
                        }
                    } else {
                        CompiledNode::Min {
                            left: Box::new(self.compile_node(left, backend)?),
                            right: Box::new(self.compile_node(right, backend)?),
                            right_min: rr.min(),
                        }
                    }
                }
                BinaryOp::Max => {
                    let lr = left.range();
                    let rr = right.range();
                    if let (Some((l_min, l_max)), Some((r_min, r_max))) = (lr.bounds(), rr.bounds())
                    {
                        if l_min > r_max {
                            return self.compile_node(left, backend);
                        }
                        if r_min > l_max {
                            return self.compile_node(right, backend);
                        }
                    }
                    if let OptNode::Constant(c) = **left {
                        CompiledNode::ConstMax {
                            input: Box::new(self.compile_node(right, backend)?),
                            constant: c,
                        }
                    } else if let OptNode::Constant(c) = **right {
                        CompiledNode::ConstMax {
                            input: Box::new(self.compile_node(left, backend)?),
                            constant: c,
                        }
                    } else {
                        CompiledNode::Max {
                            left: Box::new(self.compile_node(left, backend)?),
                            right: Box::new(self.compile_node(right, backend)?),
                            right_max: rr.max(),
                        }
                    }
                }
            },
            OptNode::Pow { base, exponent } => {
                // Spec §2.5 & Edge Case #9: base constant check runs FIRST.
                if let OptNode::Constant(b) = **base {
                    CompiledNode::PowConstBase {
                        base: b,
                        exponent: Box::new(self.compile_node(exponent, backend)?),
                    }
                } else if let OptNode::Constant(e) = **exponent {
                    let a = e.abs();
                    let compiled_base = self.compile_node(base, backend)?;
                    let special = if a == 0.5_f32 {
                        Some(CompiledNode::Unary {
                            op: UnaryOp::Sqrt,
                            input: Box::new(compiled_base),
                        })
                    } else if a == 1.0_f32 {
                        Some(compiled_base)
                    } else if a == 2.0_f32 {
                        Some(CompiledNode::Unary {
                            op: UnaryOp::Square,
                            input: Box::new(compiled_base),
                        })
                    } else if a == 3.0_f32 {
                        Some(CompiledNode::Unary {
                            op: UnaryOp::Cube,
                            input: Box::new(compiled_base),
                        })
                    } else {
                        return Ok(CompiledNode::PowConstExp {
                            base: Box::new(compiled_base),
                            exponent: e,
                        });
                    };
                    let inner = special.expect("handled above");
                    if e < 0.0_f32 {
                        CompiledNode::Unary {
                            op: UnaryOp::Reciprocal,
                            input: Box::new(inner),
                        }
                    } else {
                        inner
                    }
                } else {
                    CompiledNode::Pow {
                        base: Box::new(self.compile_node(base, backend)?),
                        exponent: Box::new(self.compile_node(exponent, backend)?),
                    }
                }
            }
            OptNode::Lerp {
                alpha,
                first,
                second,
            } => {
                let alpha = Box::new(self.compile_node(alpha, backend)?);
                if let OptNode::Constant(f) = **first {
                    CompiledNode::LerpConstFirst {
                        alpha,
                        first: f,
                        second: Box::new(self.compile_node(second, backend)?),
                    }
                } else if let OptNode::Constant(s) = **second {
                    CompiledNode::LerpConstSecond {
                        alpha,
                        first: Box::new(self.compile_node(first, backend)?),
                        second: s,
                    }
                } else {
                    CompiledNode::Lerp {
                        alpha,
                        first: Box::new(self.compile_node(first, backend)?),
                        second: Box::new(self.compile_node(second, backend)?),
                    }
                }
            }
            OptNode::Clamp { input, min, max } => CompiledNode::Clamp {
                input: Box::new(self.compile_node(input, backend)?),
                min: *min,
                max: *max,
            },
            OptNode::RangeChoice {
                input,
                min_inclusive,
                max_exclusive,
                when_in_range,
                when_out_of_range,
            } => {
                let input = Box::new(self.compile_node(input, backend)?);
                if let (OptNode::Constant(in_v), OptNode::Constant(out_v)) =
                    (when_in_range.as_ref(), when_out_of_range.as_ref())
                {
                    CompiledNode::RangeChoiceConst {
                        input,
                        min_inclusive: *min_inclusive,
                        max_exclusive: *max_exclusive,
                        when_in_range: *in_v,
                        when_out_of_range: *out_v,
                    }
                } else {
                    CompiledNode::RangeChoice {
                        input,
                        min_inclusive: *min_inclusive,
                        max_exclusive: *max_exclusive,
                        when_in_range: Box::new(self.compile_node(when_in_range, backend)?),
                        when_out_of_range: Box::new(self.compile_node(when_out_of_range, backend)?),
                    }
                }
            }
            OptNode::IntervalSelect {
                input,
                thresholds,
                functions,
            } => {
                let input = Box::new(self.compile_node(input, backend)?);
                if thresholds.len() == 1 {
                    CompiledNode::IntervalSelectSingle {
                        input,
                        threshold: thresholds[0],
                        below: Box::new(self.compile_node(&functions[0], backend)?),
                        at_or_above: Box::new(self.compile_node(&functions[1], backend)?),
                    }
                } else {
                    let mut compiled_fns = Vec::with_capacity(functions.len());
                    for f in functions {
                        compiled_fns.push(self.compile_node(f, backend)?);
                    }
                    CompiledNode::IntervalSelect {
                        input,
                        thresholds: thresholds.clone(),
                        functions: compiled_fns,
                    }
                }
            }
            OptNode::Spline(spline) => {
                let mut coord_map: HashMap<OptNode, (usize, Interval)> = HashMap::new();
                let mut coords: Vec<CompiledNode> = Vec::new();
                let indexed = spline.try_map_coordinates(&mut |coord| {
                    if let Some(&entry) = coord_map.get(coord.as_ref()) {
                        return Ok(entry);
                    }
                    let idx = coords.len();
                    let range = coord.range();
                    let compiled = self.compile_node(coord, backend)?;
                    coord_map.insert(coord.as_ref().clone(), (idx, range));
                    coords.push(compiled);
                    Ok::<_, CompileError>((idx, range))
                })?;
                CompiledNode::Spline {
                    spline: indexed,
                    coords,
                }
            }
            OptNode::BlendDensity(input) => CompiledNode::BlendDensity {
                input: Box::new(self.compile_node(input, backend)?),
            },
            OptNode::Interpolated {
                input,
                cell_size_xz,
                cell_size_y,
            } => CompiledNode::Interpolated {
                input: Box::new(self.compile_node(input, backend)?),
                cell_size_xz: *cell_size_xz,
                cell_size_y: *cell_size_y,
                inv_xz: 1.0_f32 / (*cell_size_xz as f32),
                inv_y: 1.0_f32 / (*cell_size_y as f32),
            },
            OptNode::Slice {
                axis,
                coordinate,
                input,
            } => match (axis, input.as_ref()) {
                (
                    Axis3D::X,
                    OptNode::Slice {
                        axis: Axis3D::Z,
                        coordinate: cz,
                        input: inner,
                    },
                ) => CompiledNode::SliceXz {
                    x: *coordinate,
                    z: *cz,
                    input: Box::new(self.compile_node(inner, backend)?),
                },
                (
                    Axis3D::Z,
                    OptNode::Slice {
                        axis: Axis3D::X,
                        coordinate: cx,
                        input: inner,
                    },
                ) => CompiledNode::SliceXz {
                    x: *cx,
                    z: *coordinate,
                    input: Box::new(self.compile_node(inner, backend)?),
                },
                (Axis3D::X, _) => CompiledNode::SliceX {
                    coordinate: *coordinate,
                    input: Box::new(self.compile_node(input, backend)?),
                },
                (Axis3D::Y, _) => CompiledNode::SliceY {
                    coordinate: *coordinate,
                    input: Box::new(self.compile_node(input, backend)?),
                },
                (Axis3D::Z, _) => CompiledNode::SliceZ {
                    coordinate: *coordinate,
                    input: Box::new(self.compile_node(input, backend)?),
                },
            },
            OptNode::FindTopSurface {
                density,
                upper_bound,
                lower_bound,
                cell_height,
            } => {
                // Spec §2.5: FindTopSurface wraps its sampler in SliceFunction.YSampler(..., 0).
                CompiledNode::SliceY {
                    coordinate: 0,
                    input: Box::new(CompiledNode::FindTopSurface {
                        density: Box::new(self.compile_node(density, backend)?),
                        upper_bound: Box::new(self.compile_node(upper_bound, backend)?),
                        lower_bound: *lower_bound,
                        cell_height: *cell_height,
                    }),
                }
            }
        })
    }
}

/// Step 1B: `SliceUniformAxes` rewrite pass (`density-functions.md` §1B).
fn slice_uniform_axes(node: OptNode, parent_domain_axes: u8) -> OptNode {
    if matches!(node, OptNode::Constant(_) | OptNode::Gradient { .. }) {
        return node;
    }
    let axes = node.domain_axes();
    if axes == parent_domain_axes {
        return rewrite_children_slice(node, parent_domain_axes);
    }

    let mut rewritten = rewrite_children_slice(node, axes);
    let removed_axes = parent_domain_axes & !axes;

    // Collect existingRemovedAxes by inspecting consecutive outermost Slice nodes.
    let mut existing_removed_axes = NO_AXES;
    let mut cursor = &rewritten;
    while let OptNode::Slice { axis, input, .. } = cursor {
        existing_removed_axes |= axis.mask();
        cursor = input;
    }

    let filtered_axes = removed_axes & !existing_removed_axes;
    // Exact order from innermost to outermost: X, Z, Y.
    if filtered_axes & AXIS_X != 0 {
        rewritten = OptNode::Slice {
            axis: Axis3D::X,
            coordinate: 0,
            input: Box::new(rewritten),
        };
    }
    if filtered_axes & AXIS_Z != 0 {
        rewritten = OptNode::Slice {
            axis: Axis3D::Z,
            coordinate: 0,
            input: Box::new(rewritten),
        };
    }
    if filtered_axes & AXIS_Y != 0 {
        rewritten = OptNode::Slice {
            axis: Axis3D::Y,
            coordinate: 0,
            input: Box::new(rewritten),
        };
    }
    rewritten
}

fn rewrite_children_slice(node: OptNode, child_parent_axes: u8) -> OptNode {
    match node {
        // PreparedCache.rewriteChildren returns `this` (do not descend inside).
        node @ (OptNode::PreparedCache { .. }
        | OptNode::Constant(_)
        | OptNode::BlendAlpha
        | OptNode::BlendOffset
        | OptNode::Beardifier
        | OptNode::Shift { .. }
        | OptNode::EndOuterIslands
        | OptNode::DistanceToPoint { .. }
        | OptNode::Gradient { .. }
        | OptNode::OldBlendedNoise(_)) => node,
        OptNode::Noise {
            noise,
            xz_scale,
            y_scale,
            shift_x,
            shift_y,
            shift_z,
        } => OptNode::Noise {
            noise,
            xz_scale,
            y_scale,
            shift_x: Box::new(slice_uniform_axes(*shift_x, child_parent_axes)),
            shift_y: Box::new(slice_uniform_axes(*shift_y, child_parent_axes)),
            shift_z: Box::new(slice_uniform_axes(*shift_z, child_parent_axes)),
        },
        OptNode::Unary { op, input } => OptNode::Unary {
            op,
            input: Box::new(slice_uniform_axes(*input, child_parent_axes)),
        },
        OptNode::Round {
            op,
            input,
            multiple,
        } => OptNode::Round {
            op,
            input: Box::new(slice_uniform_axes(*input, child_parent_axes)),
            multiple: Box::new(slice_uniform_axes(*multiple, child_parent_axes)),
        },
        OptNode::Binary { op, left, right } => OptNode::Binary {
            op,
            left: Box::new(slice_uniform_axes(*left, child_parent_axes)),
            right: Box::new(slice_uniform_axes(*right, child_parent_axes)),
        },
        OptNode::Pow { base, exponent } => OptNode::Pow {
            base: Box::new(slice_uniform_axes(*base, child_parent_axes)),
            exponent: Box::new(slice_uniform_axes(*exponent, child_parent_axes)),
        },
        OptNode::Lerp {
            alpha,
            first,
            second,
        } => OptNode::Lerp {
            alpha: Box::new(slice_uniform_axes(*alpha, child_parent_axes)),
            first: Box::new(slice_uniform_axes(*first, child_parent_axes)),
            second: Box::new(slice_uniform_axes(*second, child_parent_axes)),
        },
        OptNode::Clamp { input, min, max } => OptNode::Clamp {
            input: Box::new(slice_uniform_axes(*input, child_parent_axes)),
            min,
            max,
        },
        OptNode::RangeChoice {
            input,
            min_inclusive,
            max_exclusive,
            when_in_range,
            when_out_of_range,
        } => OptNode::RangeChoice {
            input: Box::new(slice_uniform_axes(*input, child_parent_axes)),
            min_inclusive,
            max_exclusive,
            when_in_range: Box::new(slice_uniform_axes(*when_in_range, child_parent_axes)),
            when_out_of_range: Box::new(slice_uniform_axes(*when_out_of_range, child_parent_axes)),
        },
        OptNode::IntervalSelect {
            input,
            thresholds,
            functions,
        } => OptNode::IntervalSelect {
            input: Box::new(slice_uniform_axes(*input, child_parent_axes)),
            thresholds,
            functions: functions
                .into_iter()
                .map(|f| slice_uniform_axes(f, child_parent_axes))
                .collect(),
        },
        OptNode::Spline(spline) => OptNode::Spline(spline.map_coordinates(
            &mut |c| Box::new(slice_uniform_axes((**c).clone(), child_parent_axes)),
            &|c| c.range(),
        )),
        OptNode::BlendDensity(input) => {
            OptNode::BlendDensity(Box::new(slice_uniform_axes(*input, child_parent_axes)))
        }
        OptNode::Interpolated {
            input,
            cell_size_xz,
            cell_size_y,
        } => OptNode::Interpolated {
            input: Box::new(slice_uniform_axes(*input, child_parent_axes)),
            cell_size_xz,
            cell_size_y,
        },
        OptNode::Slice {
            axis,
            coordinate,
            input,
        } => OptNode::Slice {
            axis,
            coordinate,
            input: Box::new(slice_uniform_axes(*input, child_parent_axes)),
        },
        OptNode::FindTopSurface {
            density,
            upper_bound,
            lower_bound,
            cell_height,
        } => OptNode::FindTopSurface {
            density: Box::new(slice_uniform_axes(*density, child_parent_axes)),
            upper_bound: Box::new(slice_uniform_axes(*upper_bound, child_parent_axes)),
            lower_bound,
            cell_height,
        },
    }
}

// ---------------------------------------------------------------------------
// Compiled sampler tree (`CompiledNode`) & runtime evaluation
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum CompiledNode {
    Shared(Arc<CompiledNode>),
    Cache {
        id: usize,
        input: Arc<CompiledNode>,
    },
    Constant(f32),
    BlendAlpha,
    BlendOffset,
    Beardifier,
    NoiseUnshifted {
        noise: Arc<NormalNoise>,
        xz_scale: f64,
        y_scale: f64,
    },
    NoiseShiftedXz {
        noise: Arc<NormalNoise>,
        xz_scale: f64,
        y_scale: f64,
        shift_x: Box<CompiledNode>,
        shift_z: Box<CompiledNode>,
    },
    NoiseShiftedXyz {
        noise: Arc<NormalNoise>,
        xz_scale: f64,
        y_scale: f64,
        shift_x: Box<CompiledNode>,
        shift_y: Box<CompiledNode>,
        shift_z: Box<CompiledNode>,
    },
    ShiftB {
        noise: Arc<NormalNoise>,
    },
    EndOuterIslands {
        noise: SimplexNoise,
    },
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
        coord_range: i32,
        factor: f32,
    },
    OldBlendedNoise(Box<BlendedNoise>),
    Unary {
        op: UnaryOp,
        input: Box<CompiledNode>,
    },
    RoundUnit {
        op: RoundOp,
        input: Box<CompiledNode>,
    },
    RoundMultiple {
        op: RoundOp,
        input: Box<CompiledNode>,
        multiple: Box<CompiledNode>,
    },
    ConstAdd {
        input: Box<CompiledNode>,
        constant: f32,
    },
    Add {
        left: Box<CompiledNode>,
        right: Box<CompiledNode>,
    },
    ConstSub {
        constant: f32,
        input: Box<CompiledNode>,
    },
    Sub {
        left: Box<CompiledNode>,
        right: Box<CompiledNode>,
    },
    ConstMul {
        input: Box<CompiledNode>,
        constant: f32,
    },
    Mul {
        left: Box<CompiledNode>,
        right: Box<CompiledNode>,
    },
    ConstDiv {
        constant: f32,
        input: Box<CompiledNode>,
    },
    Div {
        left: Box<CompiledNode>,
        right: Box<CompiledNode>,
    },
    ConstMin {
        input: Box<CompiledNode>,
        constant: f32,
    },
    Min {
        left: Box<CompiledNode>,
        right: Box<CompiledNode>,
        right_min: f32,
    },
    ConstMax {
        input: Box<CompiledNode>,
        constant: f32,
    },
    Max {
        left: Box<CompiledNode>,
        right: Box<CompiledNode>,
        right_max: f32,
    },
    PowConstBase {
        base: f32,
        exponent: Box<CompiledNode>,
    },
    PowConstExp {
        base: Box<CompiledNode>,
        exponent: f32,
    },
    Pow {
        base: Box<CompiledNode>,
        exponent: Box<CompiledNode>,
    },
    LerpConstFirst {
        alpha: Box<CompiledNode>,
        first: f32,
        second: Box<CompiledNode>,
    },
    LerpConstSecond {
        alpha: Box<CompiledNode>,
        first: Box<CompiledNode>,
        second: f32,
    },
    Lerp {
        alpha: Box<CompiledNode>,
        first: Box<CompiledNode>,
        second: Box<CompiledNode>,
    },
    Clamp {
        input: Box<CompiledNode>,
        min: f32,
        max: f32,
    },
    RangeChoiceConst {
        input: Box<CompiledNode>,
        min_inclusive: f32,
        max_exclusive: f32,
        when_in_range: f32,
        when_out_of_range: f32,
    },
    RangeChoice {
        input: Box<CompiledNode>,
        min_inclusive: f32,
        max_exclusive: f32,
        when_in_range: Box<CompiledNode>,
        when_out_of_range: Box<CompiledNode>,
    },
    IntervalSelectSingle {
        input: Box<CompiledNode>,
        threshold: f32,
        below: Box<CompiledNode>,
        at_or_above: Box<CompiledNode>,
    },
    IntervalSelect {
        input: Box<CompiledNode>,
        thresholds: Vec<f32>,
        functions: Vec<CompiledNode>,
    },
    Spline {
        spline: CubicSpline<usize>,
        coords: Vec<CompiledNode>,
    },
    BlendDensity {
        input: Box<CompiledNode>,
    },
    Interpolated {
        input: Box<CompiledNode>,
        cell_size_xz: i32,
        cell_size_y: i32,
        inv_xz: f32,
        inv_y: f32,
    },
    SliceX {
        coordinate: i32,
        input: Box<CompiledNode>,
    },
    SliceY {
        coordinate: i32,
        input: Box<CompiledNode>,
    },
    SliceZ {
        coordinate: i32,
        input: Box<CompiledNode>,
    },
    SliceXz {
        x: i32,
        z: i32,
        input: Box<CompiledNode>,
    },
    FindTopSurface {
        density: Box<CompiledNode>,
        upper_bound: Box<CompiledNode>,
        lower_bound: i32,
        cell_height: i32,
    },
}

impl CompiledNode {
    fn sample_value(&self, ctx: &mut SamplerContext, bx: i32, by: i32, bz: i32) -> f32 {
        match self {
            Self::Shared(inner) => inner.sample_value(ctx, bx, by, bz),
            Self::Cache { id, input } => {
                if !ctx.enable_caches {
                    return input.sample_value(ctx, bx, by, bz);
                }
                ctx.ensure_cache_cell(*id);
                let key = block_as_long(bx, by, bz);
                {
                    let cell = &ctx.caches[*id];
                    if cell.value_key == key && !cell.value.is_nan() {
                        return cell.value;
                    }
                    if let (Some(vol), Some(buf)) = (&cell.volume, &cell.buffer)
                        && let Some(idx) = vol.index_of_block(bx, by, bz)
                    {
                        return buf[idx];
                    }
                }
                let v = input.sample_value(ctx, bx, by, bz);
                let cell = &mut ctx.caches[*id];
                cell.value_key = key;
                cell.value = v;
                v
            }
            Self::Constant(v) => *v,
            Self::BlendAlpha => ctx
                .blend_alpha
                .as_ref()
                .map_or(1.0_f32, |s| s.sample_value(bx, by, bz)),
            Self::BlendOffset => ctx
                .blend_offset
                .as_ref()
                .map_or(0.0_f32, |s| s.sample_value(bx, by, bz)),
            Self::Beardifier => ctx
                .beardifier
                .as_ref()
                .map_or(0.0_f32, |s| s.sample_value(bx, by, bz)),
            Self::NoiseUnshifted {
                noise,
                xz_scale,
                y_scale,
            } => noise.sample(
                f64::from(bx) * *xz_scale,
                f64::from(by) * *y_scale,
                f64::from(bz) * *xz_scale,
            ),
            Self::NoiseShiftedXz {
                noise,
                xz_scale,
                y_scale,
                shift_x,
                shift_z,
            } => {
                let sx = shift_x.sample_value(ctx, bx, by, bz);
                let sz = shift_z.sample_value(ctx, bx, by, bz);
                noise.sample(
                    f64::from(bx) * *xz_scale + f64::from(sx),
                    f64::from(by) * *y_scale,
                    f64::from(bz) * *xz_scale + f64::from(sz),
                )
            }
            Self::NoiseShiftedXyz {
                noise,
                xz_scale,
                y_scale,
                shift_x,
                shift_y,
                shift_z,
            } => {
                let sx = shift_x.sample_value(ctx, bx, by, bz);
                let sy = shift_y.sample_value(ctx, bx, by, bz);
                let sz = shift_z.sample_value(ctx, bx, by, bz);
                noise.sample(
                    f64::from(bx) * *xz_scale + f64::from(sx),
                    f64::from(by) * *y_scale + f64::from(sy),
                    f64::from(bz) * *xz_scale + f64::from(sz),
                )
            }
            Self::ShiftB { noise } => {
                noise.sample(f64::from(bz) * 0.25_f64, f64::from(bx) * 0.25_f64, 0.0_f64) * 4.0_f32
            }
            Self::EndOuterIslands { noise } => sample_end_outer_islands(noise, bx, bz),
            Self::DistanceToPoint { point, metric } => {
                sample_distance_to_point(*point, *metric, bx, by, bz)
            }
            Self::Gradient {
                axis,
                tiling,
                from_coordinate,
                to_coordinate,
                from_value,
                coord_range,
                factor,
            } => sample_gradient(
                axis.select(bx, by, bz),
                *tiling,
                *from_coordinate,
                *to_coordinate,
                *from_value,
                *coord_range,
                *factor,
            ),
            Self::OldBlendedNoise(bn) => bn.sample(bx, by, bz),
            Self::Unary { op, input } => op.apply(input.sample_value(ctx, bx, by, bz)),
            Self::RoundUnit { op, input } => op.round_to_int(input.sample_value(ctx, bx, by, bz)),
            Self::RoundMultiple {
                op,
                input,
                multiple,
            } => {
                let v = input.sample_value(ctx, bx, by, bz);
                let m = multiple.sample_value(ctx, bx, by, bz);
                if m == 0.0_f32 {
                    v
                } else {
                    op.round_to_int(v / m) * m
                }
            }
            Self::ConstAdd { input, constant } => input.sample_value(ctx, bx, by, bz) + *constant,
            Self::Add { left, right } => {
                left.sample_value(ctx, bx, by, bz) + right.sample_value(ctx, bx, by, bz)
            }
            Self::ConstSub { constant, input } => *constant - input.sample_value(ctx, bx, by, bz),
            Self::Sub { left, right } => {
                left.sample_value(ctx, bx, by, bz) - right.sample_value(ctx, bx, by, bz)
            }
            Self::ConstMul { input, constant } => input.sample_value(ctx, bx, by, bz) * *constant,
            Self::Mul { left, right } => {
                // Spec §2.4 & Edge Case #6: point path short-circuits on left == 0.0f -> +0.0f.
                let l = left.sample_value(ctx, bx, by, bz);
                if l == 0.0_f32 {
                    0.0_f32
                } else {
                    l * right.sample_value(ctx, bx, by, bz)
                }
            }
            Self::ConstDiv { constant, input } => *constant / input.sample_value(ctx, bx, by, bz),
            Self::Div { left, right } => {
                let l = left.sample_value(ctx, bx, by, bz);
                if l == 0.0_f32 {
                    0.0_f32
                } else {
                    l / right.sample_value(ctx, bx, by, bz)
                }
            }
            Self::ConstMin { input, constant } => {
                java_min_f32(input.sample_value(ctx, bx, by, bz), *constant)
            }
            Self::Min {
                left,
                right,
                right_min,
            } => {
                let l = left.sample_value(ctx, bx, by, bz);
                if l <= *right_min {
                    l
                } else {
                    java_min_f32(l, right.sample_value(ctx, bx, by, bz))
                }
            }
            Self::ConstMax { input, constant } => {
                java_max_f32(input.sample_value(ctx, bx, by, bz), *constant)
            }
            Self::Max {
                left,
                right,
                right_max,
            } => {
                let l = left.sample_value(ctx, bx, by, bz);
                if l >= *right_max {
                    l
                } else {
                    java_max_f32(l, right.sample_value(ctx, bx, by, bz))
                }
            }
            Self::PowConstBase { base, exponent } => {
                let e = exponent.sample_value(ctx, bx, by, bz);
                java_pow_f64(f64::from(*base), f64::from(e)) as f32
            }
            Self::PowConstExp { base, exponent } => {
                let b = base.sample_value(ctx, bx, by, bz);
                java_pow_f64(f64::from(b), f64::from(*exponent)) as f32
            }
            Self::Pow { base, exponent } => {
                let b = base.sample_value(ctx, bx, by, bz);
                let e = exponent.sample_value(ctx, bx, by, bz);
                java_pow_f64(f64::from(b), f64::from(e)) as f32
            }
            Self::LerpConstFirst {
                alpha,
                first,
                second,
            } => {
                let a = alpha.sample_value(ctx, bx, by, bz);
                if a == 0.0_f32 {
                    *first
                } else if a == 1.0_f32 {
                    second.sample_value(ctx, bx, by, bz)
                } else {
                    lerp_f32(a, *first, second.sample_value(ctx, bx, by, bz))
                }
            }
            Self::LerpConstSecond {
                alpha,
                first,
                second,
            } => {
                let a = alpha.sample_value(ctx, bx, by, bz);
                if a == 0.0_f32 {
                    first.sample_value(ctx, bx, by, bz)
                } else if a == 1.0_f32 {
                    *second
                } else {
                    lerp_f32(a, first.sample_value(ctx, bx, by, bz), *second)
                }
            }
            Self::Lerp {
                alpha,
                first,
                second,
            } => {
                let a = alpha.sample_value(ctx, bx, by, bz);
                if a == 0.0_f32 {
                    first.sample_value(ctx, bx, by, bz)
                } else if a == 1.0_f32 {
                    second.sample_value(ctx, bx, by, bz)
                } else {
                    let f = first.sample_value(ctx, bx, by, bz);
                    let s = second.sample_value(ctx, bx, by, bz);
                    lerp_f32(a, f, s)
                }
            }
            Self::Clamp { input, min, max } => {
                clamp_f32(input.sample_value(ctx, bx, by, bz), *min, *max)
            }
            Self::RangeChoiceConst {
                input,
                min_inclusive,
                max_exclusive,
                when_in_range,
                when_out_of_range,
            } => {
                let v = input.sample_value(ctx, bx, by, bz);
                if v >= *min_inclusive && v < *max_exclusive {
                    *when_in_range
                } else {
                    *when_out_of_range
                }
            }
            Self::RangeChoice {
                input,
                min_inclusive,
                max_exclusive,
                when_in_range,
                when_out_of_range,
            } => {
                let v = input.sample_value(ctx, bx, by, bz);
                if v >= *min_inclusive && v < *max_exclusive {
                    when_in_range.sample_value(ctx, bx, by, bz)
                } else {
                    when_out_of_range.sample_value(ctx, bx, by, bz)
                }
            }
            Self::IntervalSelectSingle {
                input,
                threshold,
                below,
                at_or_above,
            } => {
                let v = input.sample_value(ctx, bx, by, bz);
                if v < *threshold {
                    below.sample_value(ctx, bx, by, bz)
                } else {
                    at_or_above.sample_value(ctx, bx, by, bz)
                }
            }
            Self::IntervalSelect {
                input,
                thresholds,
                functions,
            } => {
                let v = input.sample_value(ctx, bx, by, bz);
                let mut idx = functions.len() - 1;
                for (i, &t) in thresholds.iter().enumerate() {
                    if v < t {
                        idx = i;
                        break;
                    }
                }
                functions[idx].sample_value(ctx, bx, by, bz)
            }
            Self::Spline { spline, coords } => {
                if coords.len() <= 16 {
                    let mut cached = [f32::NAN; 16];
                    spline.sample_with(&mut |&k| {
                        let v = cached[k];
                        if !v.is_nan() {
                            v
                        } else {
                            let val = coords[k].sample_value(ctx, bx, by, bz);
                            cached[k] = val;
                            val
                        }
                    })
                } else {
                    let mut cached = vec![f32::NAN; coords.len()];
                    spline.sample_with(&mut |&k| {
                        let v = cached[k];
                        if !v.is_nan() {
                            v
                        } else {
                            let val = coords[k].sample_value(ctx, bx, by, bz);
                            cached[k] = val;
                            val
                        }
                    })
                }
            }
            Self::BlendDensity { input } => {
                let v = input.sample_value(ctx, bx, by, bz);
                match &ctx.blender {
                    Some(blender) => blender.blend_density(bx, by, bz, v),
                    None => v,
                }
            }
            Self::Interpolated {
                input,
                cell_size_xz,
                cell_size_y,
                ..
            } => {
                let x_in = java_floor_mod_i32(bx, *cell_size_xz);
                let y_in = java_floor_mod_i32(by, *cell_size_y);
                let z_in = java_floor_mod_i32(bz, *cell_size_xz);
                if x_in == 0 && y_in == 0 && z_in == 0 {
                    return input.sample_value(ctx, bx, by, bz);
                }
                let corner_vol = DensityVolume::new(
                    [2, 2, 2],
                    [
                        bx.wrapping_sub(x_in),
                        by.wrapping_sub(y_in),
                        bz.wrapping_sub(z_in),
                    ],
                    [*cell_size_xz, *cell_size_y, *cell_size_xz],
                )
                .expect("2x2x2 corner volume is always valid");
                let mut corners = [0.0_f32; 8];
                input.sample_volume(ctx, &mut corners, corner_vol);
                // Z-outer, X-middle, Y-inner: index(ix, iy, iz) = iy + (ix + iz*2)*2
                let c000 = corners[0];
                let c010 = corners[1];
                let c100 = corners[2];
                let c110 = corners[3];
                let c001 = corners[4];
                let c011 = corners[5];
                let c101 = corners[6];
                let c111 = corners[7];
                lerp3_f32(
                    (x_in as f32) / (*cell_size_xz as f32),
                    (y_in as f32) / (*cell_size_y as f32),
                    (z_in as f32) / (*cell_size_xz as f32),
                    c000,
                    c100,
                    c010,
                    c110,
                    c001,
                    c101,
                    c011,
                    c111,
                )
            }
            Self::SliceX { coordinate, input } => input.sample_value(ctx, *coordinate, by, bz),
            Self::SliceY { coordinate, input } => input.sample_value(ctx, bx, *coordinate, bz),
            Self::SliceZ { coordinate, input } => input.sample_value(ctx, bx, by, *coordinate),
            Self::SliceXz { x, z, input } => input.sample_value(ctx, *x, by, *z),
            Self::FindTopSurface {
                density,
                upper_bound,
                lower_bound,
                cell_height,
            } => {
                let ub = upper_bound.sample_value(ctx, bx, 0, bz);
                find_top_surface_at(ctx, density, bx, bz, ub, *lower_bound, *cell_height)
            }
        }
    }

    fn sample_volume(&self, ctx: &mut SamplerContext, out: &mut [f32], vol: DensityVolume) {
        let len = vol.len();
        debug_assert_eq!(out.len(), len);
        match self {
            Self::Shared(inner) => inner.sample_volume(ctx, out, vol),
            Self::Cache { id, input } => {
                if !ctx.enable_caches {
                    input.sample_volume(ctx, out, vol);
                    return;
                }
                ctx.ensure_cache_cell(*id);
                let hit = ctx.caches[*id].volume == Some(vol) && ctx.caches[*id].buffer.is_some();
                if !hit {
                    if let Some(old) = ctx.caches[*id].buffer.take() {
                        ctx.release_buffer(old);
                    }
                    let mut buf = ctx.acquire_buffer(len);
                    input.sample_volume(ctx, &mut buf[..len], vol);
                    let cell = &mut ctx.caches[*id];
                    cell.volume = Some(vol);
                    cell.buffer = Some(buf);
                }
                let cached = ctx.caches[*id].buffer.as_ref().expect("populated above");
                out.copy_from_slice(&cached[..len]);
            }
            Self::Constant(v) => out.fill(*v),
            Self::BlendAlpha => match &ctx.blend_alpha {
                Some(s) => s.sample_volume(out, vol),
                None => out.fill(1.0_f32),
            },
            Self::BlendOffset => match &ctx.blend_offset {
                Some(s) => s.sample_volume(out, vol),
                None => out.fill(0.0_f32),
            },
            Self::Beardifier => match &ctx.beardifier {
                Some(s) => s.sample_volume(out, vol),
                None => out.fill(0.0_f32),
            },
            Self::NoiseUnshifted {
                noise,
                xz_scale,
                y_scale,
            } => {
                out.fill(0.0_f32);
                noise.accumulate_volume(&vol.to_volume_grid(), *xz_scale, *y_scale, 1.0_f32, out);
            }
            Self::NoiseShiftedXz {
                noise,
                xz_scale,
                y_scale,
                shift_x,
                shift_z,
            } => {
                shift_x.sample_volume(ctx, out, vol);
                let mut buf_z = ctx.acquire_buffer(len);
                shift_z.sample_volume(ctx, &mut buf_z[..len], vol);
                let mut i = 0;
                for iz in 0..vol.size_z() {
                    let base_z = f64::from(vol.block_z(iz)) * *xz_scale;
                    for ix in 0..vol.size_x() {
                        let base_x = f64::from(vol.block_x(ix)) * *xz_scale;
                        for iy in 0..vol.size_y() {
                            let y = f64::from(vol.block_y(iy)) * *y_scale;
                            out[i] = noise.sample(
                                base_x + f64::from(out[i]),
                                y,
                                base_z + f64::from(buf_z[i]),
                            );
                            i += 1;
                        }
                    }
                }
                ctx.release_buffer(buf_z);
            }
            Self::NoiseShiftedXyz {
                noise,
                xz_scale,
                y_scale,
                shift_x,
                shift_y,
                shift_z,
            } => {
                shift_x.sample_volume(ctx, out, vol);
                let mut buf_y = ctx.acquire_buffer(len);
                let mut buf_z = ctx.acquire_buffer(len);
                shift_y.sample_volume(ctx, &mut buf_y[..len], vol);
                shift_z.sample_volume(ctx, &mut buf_z[..len], vol);
                let mut i = 0;
                for iz in 0..vol.size_z() {
                    let base_z = f64::from(vol.block_z(iz)) * *xz_scale;
                    for ix in 0..vol.size_x() {
                        let base_x = f64::from(vol.block_x(ix)) * *xz_scale;
                        for iy in 0..vol.size_y() {
                            let base_y = f64::from(vol.block_y(iy)) * *y_scale;
                            out[i] = noise.sample(
                                base_x + f64::from(out[i]),
                                base_y + f64::from(buf_y[i]),
                                base_z + f64::from(buf_z[i]),
                            );
                            i += 1;
                        }
                    }
                }
                ctx.release_buffer(buf_y);
                ctx.release_buffer(buf_z);
            }
            Self::ShiftB { noise } => {
                // Transposed volume: (sizeZ, sizeX, 1, minZ, minX, 0, stepZ, stepX, 1)
                let transposed = DensityVolume::new(
                    [vol.size_z(), vol.size_x(), 1],
                    [vol.min_block_z(), vol.min_block_x(), 0],
                    [vol.step_z(), vol.step_x(), 1],
                )
                .expect("transposed shift_b volume is valid");
                let t_len = transposed.len();
                let mut temp = ctx.acquire_buffer(t_len);
                temp[..t_len].fill(0.0_f32);
                noise.accumulate_volume(
                    &transposed.to_volume_grid(),
                    0.25_f64,
                    0.25_f64,
                    4.0_f32,
                    &mut temp[..t_len],
                );
                let sy = vol.size_y();
                let mut dst = 0;
                for iz in 0..vol.size_z() {
                    for ix in 0..vol.size_x() {
                        let v = temp[transposed.index(iz, ix, 0)];
                        out[dst..dst + sy].fill(v);
                        dst += sy;
                    }
                }
                ctx.release_buffer(temp);
            }
            Self::EndOuterIslands { noise } => {
                let sy = vol.size_y();
                let mut dst = 0;
                for iz in 0..vol.size_z() {
                    let bz = vol.block_z(iz);
                    for ix in 0..vol.size_x() {
                        let bx = vol.block_x(ix);
                        let v = sample_end_outer_islands(noise, bx, bz);
                        out[dst..dst + sy].fill(v);
                        dst += sy;
                    }
                }
            }
            Self::DistanceToPoint { point, metric } => {
                let mut i = 0;
                for iz in 0..vol.size_z() {
                    let bz = vol.block_z(iz);
                    for ix in 0..vol.size_x() {
                        let bx = vol.block_x(ix);
                        for iy in 0..vol.size_y() {
                            let by = vol.block_y(iy);
                            out[i] = sample_distance_to_point(*point, *metric, bx, by, bz);
                            i += 1;
                        }
                    }
                }
            }
            Self::Gradient {
                axis,
                tiling,
                from_coordinate,
                to_coordinate,
                from_value,
                coord_range,
                factor,
            } => {
                let sx = vol.size_x();
                let sy = vol.size_y();
                let sz = vol.size_z();
                match axis {
                    Axis3D::X => {
                        for ix in 0..sx {
                            let v = sample_gradient(
                                vol.block_x(ix),
                                *tiling,
                                *from_coordinate,
                                *to_coordinate,
                                *from_value,
                                *coord_range,
                                *factor,
                            );
                            for iz in 0..sz {
                                let base = vol.index(ix, 0, iz);
                                out[base..base + sy].fill(v);
                            }
                        }
                    }
                    Axis3D::Y => {
                        let mut col = ctx.acquire_buffer(sy);
                        for (iy, slot) in col.iter_mut().enumerate().take(sy) {
                            *slot = sample_gradient(
                                vol.block_y(iy),
                                *tiling,
                                *from_coordinate,
                                *to_coordinate,
                                *from_value,
                                *coord_range,
                                *factor,
                            );
                        }
                        let col_slice = &col[..sy];
                        let mut dst = 0;
                        for _ in 0..(sz * sx) {
                            out[dst..dst + sy].copy_from_slice(col_slice);
                            dst += sy;
                        }
                        ctx.release_buffer(col);
                    }
                    Axis3D::Z => {
                        let plane = sx * sy;
                        for iz in 0..sz {
                            let v = sample_gradient(
                                vol.block_z(iz),
                                *tiling,
                                *from_coordinate,
                                *to_coordinate,
                                *from_value,
                                *coord_range,
                                *factor,
                            );
                            let base = iz * plane;
                            out[base..base + plane].fill(v);
                        }
                    }
                }
            }
            Self::OldBlendedNoise(bn) => {
                bn.fill_volume(&vol.to_volume_grid(), out);
            }
            Self::Unary { op, input } => {
                input.sample_volume(ctx, out, vol);
                for x in out.iter_mut() {
                    *x = op.apply(*x);
                }
            }
            Self::RoundUnit { op, input } => {
                input.sample_volume(ctx, out, vol);
                for x in out.iter_mut() {
                    *x = op.round_to_int(*x);
                }
            }
            Self::RoundMultiple {
                op,
                input,
                multiple,
            } => {
                input.sample_volume(ctx, out, vol);
                let mut m_buf = ctx.acquire_buffer(len);
                multiple.sample_volume(ctx, &mut m_buf[..len], vol);
                for i in 0..len {
                    let m = m_buf[i];
                    if m != 0.0_f32 {
                        out[i] = op.round_to_int(out[i] / m) * m;
                    }
                }
                ctx.release_buffer(m_buf);
            }
            Self::ConstAdd { input, constant } => {
                input.sample_volume(ctx, out, vol);
                let c = *constant;
                for x in out.iter_mut() {
                    *x += c;
                }
            }
            Self::Add { left, right } => {
                left.sample_volume(ctx, out, vol);
                let mut rhs = ctx.acquire_buffer(len);
                right.sample_volume(ctx, &mut rhs[..len], vol);
                for i in 0..len {
                    out[i] += rhs[i];
                }
                ctx.release_buffer(rhs);
            }
            Self::ConstSub { constant, input } => {
                input.sample_volume(ctx, out, vol);
                let c = *constant;
                for x in out.iter_mut() {
                    *x = c - *x;
                }
            }
            Self::Sub { left, right } => {
                // Spec §2.4: volume computes left + (-right).
                left.sample_volume(ctx, out, vol);
                let mut rhs = ctx.acquire_buffer(len);
                right.sample_volume(ctx, &mut rhs[..len], vol);
                for i in 0..len {
                    out[i] += -rhs[i];
                }
                ctx.release_buffer(rhs);
            }
            Self::ConstMul { input, constant } => {
                input.sample_volume(ctx, out, vol);
                let c = *constant;
                for x in out.iter_mut() {
                    *x *= c;
                }
            }
            Self::Mul { left, right } => {
                // Spec §2.4 & Edge Case #6: volume has NO zero short-circuit.
                left.sample_volume(ctx, out, vol);
                let mut rhs = ctx.acquire_buffer(len);
                right.sample_volume(ctx, &mut rhs[..len], vol);
                for i in 0..len {
                    out[i] *= rhs[i];
                }
                ctx.release_buffer(rhs);
            }
            Self::ConstDiv { constant, input } => {
                input.sample_volume(ctx, out, vol);
                let c = *constant;
                for x in out.iter_mut() {
                    *x = c / *x;
                }
            }
            Self::Div { left, right } => {
                // Spec §2.4 & Edge Case #6: volume has NO zero short-circuit.
                left.sample_volume(ctx, out, vol);
                let mut rhs = ctx.acquire_buffer(len);
                right.sample_volume(ctx, &mut rhs[..len], vol);
                for i in 0..len {
                    out[i] /= rhs[i];
                }
                ctx.release_buffer(rhs);
            }
            Self::ConstMin { input, constant } => {
                // Spec §2.4 & Edge Case #7: strict `<` comparison.
                input.sample_volume(ctx, out, vol);
                let c = *constant;
                for x in out.iter_mut() {
                    if c < *x {
                        *x = c;
                    }
                }
            }
            Self::Min { left, right, .. } => {
                left.sample_volume(ctx, out, vol);
                let mut rhs = ctx.acquire_buffer(len);
                right.sample_volume(ctx, &mut rhs[..len], vol);
                for i in 0..len {
                    if rhs[i] < out[i] {
                        out[i] = rhs[i];
                    }
                }
                ctx.release_buffer(rhs);
            }
            Self::ConstMax { input, constant } => {
                // Spec §2.4 & Edge Case #7: strict `>` comparison.
                input.sample_volume(ctx, out, vol);
                let c = *constant;
                for x in out.iter_mut() {
                    if c > *x {
                        *x = c;
                    }
                }
            }
            Self::Max { left, right, .. } => {
                left.sample_volume(ctx, out, vol);
                let mut rhs = ctx.acquire_buffer(len);
                right.sample_volume(ctx, &mut rhs[..len], vol);
                for i in 0..len {
                    if rhs[i] > out[i] {
                        out[i] = rhs[i];
                    }
                }
                ctx.release_buffer(rhs);
            }
            Self::PowConstBase { base, exponent } => {
                exponent.sample_volume(ctx, out, vol);
                let b = f64::from(*base);
                for x in out.iter_mut() {
                    *x = java_pow_f64(b, f64::from(*x)) as f32;
                }
            }
            Self::PowConstExp { base, exponent } => {
                base.sample_volume(ctx, out, vol);
                let e = f64::from(*exponent);
                for x in out.iter_mut() {
                    *x = java_pow_f64(f64::from(*x), e) as f32;
                }
            }
            Self::Pow { base, exponent } => {
                base.sample_volume(ctx, out, vol);
                let mut exp = ctx.acquire_buffer(len);
                exponent.sample_volume(ctx, &mut exp[..len], vol);
                for i in 0..len {
                    out[i] = java_pow_f64(f64::from(out[i]), f64::from(exp[i])) as f32;
                }
                ctx.release_buffer(exp);
            }
            Self::LerpConstFirst {
                alpha,
                first,
                second,
            } => {
                let f = *first;
                let mut a_buf = ctx.acquire_buffer(len);
                alpha.sample_volume(ctx, &mut a_buf[..len], vol);
                second.sample_volume(ctx, out, vol);
                for i in 0..len {
                    let a = a_buf[i];
                    if a == 0.0_f32 {
                        out[i] = f;
                    } else if a != 1.0_f32 {
                        out[i] = lerp_f32(a, f, out[i]);
                    }
                }
                ctx.release_buffer(a_buf);
            }
            Self::LerpConstSecond {
                alpha,
                first,
                second,
            } => {
                let s = *second;
                let mut a_buf = ctx.acquire_buffer(len);
                alpha.sample_volume(ctx, &mut a_buf[..len], vol);
                first.sample_volume(ctx, out, vol);
                for i in 0..len {
                    let a = a_buf[i];
                    if a == 1.0_f32 {
                        out[i] = s;
                    } else if a != 0.0_f32 {
                        out[i] = lerp_f32(a, out[i], s);
                    }
                }
                ctx.release_buffer(a_buf);
            }
            Self::Lerp {
                alpha,
                first,
                second,
            } => {
                let mut a_buf = ctx.acquire_buffer(len);
                let mut s_buf = ctx.acquire_buffer(len);
                alpha.sample_volume(ctx, &mut a_buf[..len], vol);
                first.sample_volume(ctx, out, vol);
                second.sample_volume(ctx, &mut s_buf[..len], vol);
                for i in 0..len {
                    let a = a_buf[i];
                    if a == 1.0_f32 {
                        out[i] = s_buf[i];
                    } else if a != 0.0_f32 {
                        out[i] = lerp_f32(a, out[i], s_buf[i]);
                    }
                }
                ctx.release_buffer(a_buf);
                ctx.release_buffer(s_buf);
            }
            Self::Clamp { input, min, max } => {
                input.sample_volume(ctx, out, vol);
                let (lo, hi) = (*min, *max);
                for x in out.iter_mut() {
                    *x = clamp_f32(*x, lo, hi);
                }
            }
            Self::RangeChoiceConst {
                input,
                min_inclusive,
                max_exclusive,
                when_in_range,
                when_out_of_range,
            } => {
                input.sample_volume(ctx, out, vol);
                let (lo, hi, v_in, v_out) = (
                    *min_inclusive,
                    *max_exclusive,
                    *when_in_range,
                    *when_out_of_range,
                );
                for x in out.iter_mut() {
                    *x = if *x >= lo && *x < hi { v_in } else { v_out };
                }
            }
            Self::RangeChoice {
                input,
                min_inclusive,
                max_exclusive,
                when_in_range,
                when_out_of_range,
            } => {
                // Spec §2.5: fill out with when_in_range, sample input & when_out_of_range into temps.
                when_in_range.sample_volume(ctx, out, vol);
                let mut in_buf = ctx.acquire_buffer(len);
                let mut out_buf = ctx.acquire_buffer(len);
                input.sample_volume(ctx, &mut in_buf[..len], vol);
                when_out_of_range.sample_volume(ctx, &mut out_buf[..len], vol);
                let (lo, hi) = (*min_inclusive, *max_exclusive);
                for i in 0..len {
                    let v = in_buf[i];
                    if !(v >= lo && v < hi) {
                        out[i] = out_buf[i];
                    }
                }
                ctx.release_buffer(in_buf);
                ctx.release_buffer(out_buf);
            }
            Self::IntervalSelectSingle {
                input,
                threshold,
                below,
                at_or_above,
            } => {
                let mut in_buf = ctx.acquire_buffer(len);
                let mut hi_buf = ctx.acquire_buffer(len);
                input.sample_volume(ctx, &mut in_buf[..len], vol);
                below.sample_volume(ctx, out, vol);
                at_or_above.sample_volume(ctx, &mut hi_buf[..len], vol);
                let t = *threshold;
                for i in 0..len {
                    if in_buf[i] >= t || in_buf[i].is_nan() {
                        out[i] = hi_buf[i];
                    }
                }
                ctx.release_buffer(in_buf);
                ctx.release_buffer(hi_buf);
            }
            Self::IntervalSelect {
                input,
                thresholds,
                functions,
            } => {
                let mut in_buf = ctx.acquire_buffer(len);
                input.sample_volume(ctx, &mut in_buf[..len], vol);
                let mut fn_bufs: Vec<Vec<f32>> = Vec::with_capacity(functions.len());
                for f in functions {
                    let mut b = ctx.acquire_buffer(len);
                    f.sample_volume(ctx, &mut b[..len], vol);
                    fn_bufs.push(b);
                }
                let last = functions.len() - 1;
                for i in 0..len {
                    let v = in_buf[i];
                    let mut pick = last;
                    for (j, &t) in thresholds.iter().enumerate() {
                        if v < t {
                            pick = j;
                            break;
                        }
                    }
                    out[i] = fn_bufs[pick][i];
                }
                for b in fn_bufs {
                    ctx.release_buffer(b);
                }
                ctx.release_buffer(in_buf);
            }
            Self::Spline { spline, coords } => {
                let mut bufs: Vec<Option<Vec<f32>>> = vec![None; coords.len()];
                for (i, dst) in out.iter_mut().enumerate() {
                    *dst = spline.sample_with(&mut |&k| {
                        let buf = bufs[k].get_or_insert_with(|| {
                            let mut b = ctx.acquire_buffer(len);
                            coords[k].sample_volume(ctx, &mut b[..len], vol);
                            b
                        });
                        buf[i]
                    });
                }
                for b in bufs.into_iter().flatten() {
                    ctx.release_buffer(b);
                }
            }
            Self::BlendDensity { input } => {
                input.sample_volume(ctx, out, vol);
                if let Some(blender) = &ctx.blender {
                    let mut i = 0;
                    for iz in 0..vol.size_z() {
                        let bz = vol.block_z(iz);
                        for ix in 0..vol.size_x() {
                            let bx = vol.block_x(ix);
                            for iy in 0..vol.size_y() {
                                let by = vol.block_y(iy);
                                out[i] = blender.blend_density(bx, by, bz, out[i]);
                                i += 1;
                            }
                        }
                    }
                }
            }
            Self::Interpolated {
                input,
                cell_size_xz,
                cell_size_y,
                inv_xz,
                inv_y,
            } => {
                sample_interpolated_volume(
                    ctx,
                    input,
                    *cell_size_xz,
                    *cell_size_y,
                    *inv_xz,
                    *inv_y,
                    out,
                    vol,
                );
            }
            Self::SliceX { coordinate, input } => {
                if vol.size_x() == 1 && vol.min_block_x() == *coordinate {
                    input.sample_volume(ctx, out, vol);
                    return;
                }
                let reduced = DensityVolume::new(
                    [1, vol.size_y(), vol.size_z()],
                    [*coordinate, vol.min_block_y(), vol.min_block_z()],
                    [vol.step_x(), vol.step_y(), vol.step_z()],
                )
                .expect("reduced X slice volume is valid");
                let r_len = reduced.len();
                let mut temp = ctx.acquire_buffer(r_len);
                input.sample_volume(ctx, &mut temp[..r_len], reduced);
                let sx = vol.size_x();
                let sy = vol.size_y();
                for iz in 0..vol.size_z() {
                    let src_start = iz * sy;
                    let src = &temp[src_start..src_start + sy];
                    for ix in 0..sx {
                        let dst = vol.index(ix, 0, iz);
                        out[dst..dst + sy].copy_from_slice(src);
                    }
                }
                ctx.release_buffer(temp);
            }
            Self::SliceY { coordinate, input } => {
                if vol.size_y() == 1 && vol.min_block_y() == *coordinate {
                    input.sample_volume(ctx, out, vol);
                    return;
                }
                let reduced = DensityVolume::new(
                    [vol.size_x(), 1, vol.size_z()],
                    [vol.min_block_x(), *coordinate, vol.min_block_z()],
                    [vol.step_x(), vol.step_y(), vol.step_z()],
                )
                .expect("reduced Y slice volume is valid");
                let r_len = reduced.len();
                let mut temp = ctx.acquire_buffer(r_len);
                input.sample_volume(ctx, &mut temp[..r_len], reduced);
                let sy = vol.size_y();
                let mut dst = 0;
                for &v in &temp[..r_len] {
                    out[dst..dst + sy].fill(v);
                    dst += sy;
                }
                ctx.release_buffer(temp);
            }
            Self::SliceZ { coordinate, input } => {
                if vol.size_z() == 1 && vol.min_block_z() == *coordinate {
                    input.sample_volume(ctx, out, vol);
                    return;
                }
                let reduced = DensityVolume::new(
                    [vol.size_x(), vol.size_y(), 1],
                    [vol.min_block_x(), vol.min_block_y(), *coordinate],
                    [vol.step_x(), vol.step_y(), vol.step_z()],
                )
                .expect("reduced Z slice volume is valid");
                let r_len = reduced.len();
                let mut temp = ctx.acquire_buffer(r_len);
                input.sample_volume(ctx, &mut temp[..r_len], reduced);
                let slice = &temp[..r_len];
                for iz in 0..vol.size_z() {
                    let dst = iz * r_len;
                    out[dst..dst + r_len].copy_from_slice(slice);
                }
                ctx.release_buffer(temp);
            }
            Self::SliceXz { x, z, input } => {
                if vol.size_x() == 1
                    && vol.min_block_x() == *x
                    && vol.size_z() == 1
                    && vol.min_block_z() == *z
                {
                    input.sample_volume(ctx, out, vol);
                    return;
                }
                let reduced = DensityVolume::new(
                    [1, vol.size_y(), 1],
                    [*x, vol.min_block_y(), *z],
                    [vol.step_x(), vol.step_y(), vol.step_z()],
                )
                .expect("reduced XZ slice volume is valid");
                let sy = vol.size_y();
                let mut temp = ctx.acquire_buffer(sy);
                input.sample_volume(ctx, &mut temp[..sy], reduced);
                let col = &temp[..sy];
                let mut dst = 0;
                for _ in 0..(vol.size_z() * vol.size_x()) {
                    out[dst..dst + sy].copy_from_slice(col);
                    dst += sy;
                }
                ctx.release_buffer(temp);
            }
            Self::FindTopSurface {
                density,
                upper_bound,
                lower_bound,
                cell_height,
            } => {
                let ub_vol = DensityVolume::new(
                    [vol.size_x(), 1, vol.size_z()],
                    [vol.min_block_x(), 0, vol.min_block_z()],
                    [vol.step_x(), vol.step_y(), vol.step_z()],
                )
                .expect("upper_bound 2D volume is valid");
                let ub_len = ub_vol.len();
                let mut ub_buf = ctx.acquire_buffer(ub_len);
                upper_bound.sample_volume(ctx, &mut ub_buf[..ub_len], ub_vol);
                let sy = vol.size_y();
                let mut dst = 0;
                let mut u_idx = 0;
                for iz in 0..vol.size_z() {
                    let bz = vol.block_z(iz);
                    for ix in 0..vol.size_x() {
                        let bx = vol.block_x(ix);
                        let v = find_top_surface_at(
                            ctx,
                            density,
                            bx,
                            bz,
                            ub_buf[u_idx],
                            *lower_bound,
                            *cell_height,
                        );
                        out[dst..dst + sy].fill(v);
                        dst += sy;
                        u_idx += 1;
                    }
                }
                ctx.release_buffer(ub_buf);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Specialized evaluator helpers
// ---------------------------------------------------------------------------

fn sample_end_outer_islands(noise: &SimplexNoise, bx: i32, bz: i32) -> f32 {
    // Spec §2.1 & Edge Case #14: truncating `/` and `%` toward zero.
    let section_x = bx / 8;
    let section_z = bz / 8;
    let chunk_x = section_x / 2;
    let chunk_z = section_z / 2;
    let sub_x = section_x % 2;
    let sub_z = section_z % 2;

    let mut doffs = -100.0_f32;
    for xo in -12..=12_i32 {
        for zo in -12..=12_i32 {
            let tx = i64::from(chunk_x.wrapping_add(xo));
            let tz = i64::from(chunk_z.wrapping_add(zo));
            if tx * tx + tz * tz > 4096 && noise.sample_2d(tx as f64, tz as f64) < -0.9_f32 {
                let island_size = ((tx as f32).abs() * 3439.0_f32 + (tz as f32).abs() * 147.0_f32)
                    % 13.0_f32
                    + 9.0_f32;
                let xd = sub_x.wrapping_sub(xo.wrapping_mul(2)) as f32;
                let zd = sub_z.wrapping_sub(zo.wrapping_mul(2)) as f32;
                let new_doffs = clamp_f32(
                    100.0_f32 - sqrt(xd * xd + zd * zd) * island_size,
                    -100.0_f32,
                    80.0_f32,
                );
                doffs = java_max_f32(doffs, new_doffs);
            }
        }
    }
    (doffs - 8.0_f32) / 128.0_f32
}

#[inline]
fn sample_distance_to_point(
    point: [i32; 3],
    metric: DistanceMetric,
    bx: i32,
    by: i32,
    bz: i32,
) -> f32 {
    let dx = point[0].wrapping_sub(bx) as f32;
    let dy = point[1].wrapping_sub(by) as f32;
    let dz = point[2].wrapping_sub(bz) as f32;
    match metric {
        DistanceMetric::Euclidean => sqrt((dx * dx + dy * dy) + dz * dz),
        DistanceMetric::EuclideanSquared => (dx * dx + dy * dy) + dz * dz,
        DistanceMetric::Manhattan => (dx.abs() + dy.abs()) + dz.abs(),
        DistanceMetric::Chebyshev => java_max_f32(java_max_f32(dx.abs(), dy.abs()), dz.abs()),
    }
}

#[inline]
fn sample_gradient(
    c: i32,
    tiling: GradientTiling,
    from_coordinate: i32,
    to_coordinate: i32,
    from_value: f32,
    coord_range: i32,
    factor: f32,
) -> f32 {
    match tiling {
        GradientTiling::ClampToEdge => {
            let lo = from_coordinate.min(to_coordinate);
            let hi = from_coordinate.max(to_coordinate);
            from_value + (clamp_i32(c, lo, hi).wrapping_sub(from_coordinate) as f32) * factor
        }
        GradientTiling::Repeat => {
            let rel = c.wrapping_sub(from_coordinate);
            from_value + (java_floor_mod_i32(rel, coord_range) as f32) * factor
        }
        GradientTiling::MirroredRepeat => {
            let rel = c.wrapping_sub(from_coordinate);
            let tile = java_floor_div_i32(rel, coord_range);
            let local = rel.wrapping_sub(tile.wrapping_mul(coord_range));
            if (tile & 1) == 0 {
                from_value + (local as f32) * factor
            } else {
                from_value + (coord_range.wrapping_sub(local) as f32) * factor
            }
        }
    }
}

fn find_top_surface_at(
    ctx: &mut SamplerContext,
    density: &CompiledNode,
    bx: i32,
    bz: i32,
    ub: f32,
    lower_bound: i32,
    cell_height: i32,
) -> f32 {
    let top_y = floor_f32(ub / (cell_height as f32)).wrapping_mul(cell_height);
    if top_y <= lower_bound {
        return lower_bound as f32;
    }
    let mut probe_y = top_y;
    while probe_y >= lower_bound {
        if density.sample_value(ctx, bx, probe_y, bz) > 0.0_f32 {
            return probe_y as f32;
        }
        match probe_y.checked_sub(cell_height) {
            Some(next) => probe_y = next,
            None => break,
        }
    }
    lower_bound as f32
}

/// Exact `InterpolatedFunction.sampleVolume` (`density-functions.md` §6.2).
#[allow(clippy::too_many_arguments)]
fn sample_interpolated_volume(
    ctx: &mut SamplerContext,
    input: &CompiledNode,
    cell_xz: i32,
    cell_y: i32,
    inv_xz: f32,
    inv_y: f32,
    out: &mut [f32],
    vol: DensityVolume,
) {
    // Branch 1: aligned cell-grid passthrough.
    if (vol.step_x() == cell_xz || vol.size_x() == 1)
        && (vol.step_y() == cell_y || vol.size_y() == 1)
        && (vol.step_z() == cell_xz || vol.size_z() == 1)
        && java_floor_mod_i32(vol.min_block_x(), cell_xz) == 0
        && java_floor_mod_i32(vol.min_block_y(), cell_y) == 0
        && java_floor_mod_i32(vol.min_block_z(), cell_xz) == 0
    {
        input.sample_volume(ctx, out, vol);
        return;
    }

    // Branch 2: unit block step.
    if vol.step_x() == 1 && vol.step_y() == 1 && vol.step_z() == 1 {
        fill_interpolated_unit_volume(ctx, input, cell_xz, cell_y, inv_xz, inv_y, out, vol);
        return;
    }

    // Branch 3: non-unit step -> evaluate full unit-step volume and subsample.
    let full_sx = vol.size_x() * (vol.step_x() as usize);
    let full_sy = vol.size_y() * (vol.step_y() as usize);
    let full_sz = vol.size_z() * (vol.step_z() as usize);
    let full_vol = DensityVolume::unit(
        full_sx,
        full_sy,
        full_sz,
        vol.min_block_x(),
        vol.min_block_y(),
        vol.min_block_z(),
    )
    .expect("interpolated unit-step volume fits in memory");
    let full_len = full_vol.len();
    let mut full_buf = ctx.acquire_buffer(full_len);
    fill_interpolated_unit_volume(
        ctx,
        input,
        cell_xz,
        cell_y,
        inv_xz,
        inv_y,
        &mut full_buf[..full_len],
        full_vol,
    );
    let stx = vol.step_x() as usize;
    let sty = vol.step_y() as usize;
    let stz = vol.step_z() as usize;
    let mut dst = 0;
    for iz in 0..vol.size_z() {
        let fz = iz * stz;
        for ix in 0..vol.size_x() {
            let fx = ix * stx;
            for iy in 0..vol.size_y() {
                let fy = iy * sty;
                out[dst] = full_buf[full_vol.index(fx, fy, fz)];
                dst += 1;
            }
        }
    }
    ctx.release_buffer(full_buf);
}

#[allow(clippy::too_many_arguments)]
fn fill_interpolated_unit_volume(
    ctx: &mut SamplerContext,
    input: &CompiledNode,
    cell_xz: i32,
    cell_y: i32,
    inv_xz: f32,
    inv_y: f32,
    out: &mut [f32],
    vol: DensityVolume,
) {
    let min_cx = java_floor_div_i32(vol.min_block_x(), cell_xz);
    let max_cx = java_floor_div_i32(vol.max_block_x(), cell_xz);
    let count_x = (max_cx - min_cx + 1) as usize;
    let cv_sx = if java_floor_mod_i32(vol.max_block_x(), cell_xz) == 0 {
        count_x
    } else {
        count_x + 1
    };

    let min_cy = java_floor_div_i32(vol.min_block_y(), cell_y);
    let max_cy = java_floor_div_i32(vol.max_block_y(), cell_y);
    let count_y = (max_cy - min_cy + 1) as usize;
    let cv_sy = if java_floor_mod_i32(vol.max_block_y(), cell_y) == 0 {
        count_y
    } else {
        count_y + 1
    };

    let min_cz = java_floor_div_i32(vol.min_block_z(), cell_xz);
    let max_cz = java_floor_div_i32(vol.max_block_z(), cell_xz);
    let count_z = (max_cz - min_cz + 1) as usize;
    let cv_sz = if java_floor_mod_i32(vol.max_block_z(), cell_xz) == 0 {
        count_z
    } else {
        count_z + 1
    };

    let cell_vol = DensityVolume::new(
        [cv_sx, cv_sy, cv_sz],
        [
            min_cx.wrapping_mul(cell_xz),
            min_cy.wrapping_mul(cell_y),
            min_cz.wrapping_mul(cell_xz),
        ],
        [cell_xz, cell_y, cell_xz],
    )
    .expect("covering cell volume is valid");

    let c_len = cell_vol.len();
    let mut cell_buf = ctx.acquire_buffer(c_len);
    input.sample_volume(ctx, &mut cell_buf[..c_len], cell_vol);

    let vol_sx = vol.size_x() as i32;
    let vol_sy = vol.size_y() as i32;
    let vol_sz = vol.size_z() as i32;

    for cz in 0..count_z {
        let nz = (cz + 1).min(cv_sz - 1);
        let off_z = cell_vol.block_z(cz).wrapping_sub(vol.min_block_z());
        let z0 = 0_i32.max(-off_z);
        let z1 = cell_xz.min(vol_sz - off_z) - 1;

        for cx in 0..count_x {
            let nx = (cx + 1).min(cv_sx - 1);
            let off_x = cell_vol.block_x(cx).wrapping_sub(vol.min_block_x());
            let x0 = 0_i32.max(-off_x);
            let x1 = cell_xz.min(vol_sx - off_x) - 1;

            for cy in 0..count_y {
                let ny = (cy + 1).min(cv_sy - 1);
                let off_y = cell_vol.block_y(cy).wrapping_sub(vol.min_block_y());
                let y0 = 0_i32.max(-off_y);
                let y1 = cell_y.min(vol_sy - off_y) - 1;

                let v000 = cell_buf[cell_vol.index(cx, cy, cz)];
                let v100 = cell_buf[cell_vol.index(nx, cy, cz)];
                let v010 = cell_buf[cell_vol.index(cx, ny, cz)];
                let v110 = cell_buf[cell_vol.index(nx, ny, cz)];
                let v001 = cell_buf[cell_vol.index(cx, cy, nz)];
                let v101 = cell_buf[cell_vol.index(nx, cy, nz)];
                let v011 = cell_buf[cell_vol.index(cx, ny, nz)];
                let v111 = cell_buf[cell_vol.index(nx, ny, nz)];

                // Exact fillCell interpolation order: Z -> X -> incremental Y (all f32).
                for z in z0..=z1 {
                    let alpha_z = (z as f32) * inv_xz;
                    let v00_ = lerp_f32(alpha_z, v000, v001);
                    let v01_ = lerp_f32(alpha_z, v010, v011);
                    let v10_ = lerp_f32(alpha_z, v100, v101);
                    let v11_ = lerp_f32(alpha_z, v110, v111);
                    let out_z = (off_z + z) as usize;

                    for x in x0..=x1 {
                        let alpha_x = (x as f32) * inv_xz;
                        let v_0_ = lerp_f32(alpha_x, v00_, v10_);
                        let v_1_ = lerp_f32(alpha_x, v01_, v11_);
                        let value_step = (v_1_ - v_0_) * inv_y;
                        let mut value = v_0_ + value_step * (y0 as f32);
                        let out_x = (off_x + x) as usize;
                        let base_idx = vol.index(out_x, (off_y + y0) as usize, out_z);
                        let y_count = (y1 - y0 + 1) as usize;
                        for dst in &mut out[base_idx..base_idx + y_count] {
                            *dst = value;
                            value += value_step;
                        }
                    }
                }
            }
        }
    }

    ctx.release_buffer(cell_buf);
}
