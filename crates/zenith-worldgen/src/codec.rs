//! Strict JSON codecs for `minecraft:worldgen/noise`, `minecraft:worldgen/density_function`,
//! `CubicSpline`, and `minecraft:worldgen/noise_settings`.
//!
//! Uses `zenith_codec` so error accumulation, `null`-as-absent, integer truncation/wrapping,
//! and strict JSON parsing match `docs/specs/codec/datapacks-and-codecs.md`.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use zenith_codec::codec::{
    BoolCodec, Codec, DataResult, DoubleCodec, FloatCodec, IdentifierCodec, IntCodec, RecordReader,
    StringCodec, decode_dispatch, list, sized_list, validated,
};
use zenith_codec::json::Json;
use zenith_math::noise::{
    MAX_AMPLITUDE, MAX_BASE_OCTAVE, MAX_OCTAVES, MIN_BASE_AMPLITUDE, MIN_BASE_OCTAVE,
};
use zenith_math::{BlendedNoiseParameters, NormalNoiseParameters, NormalizationMode};
use zenith_registry::Identifier;

use crate::ast::{
    Axis3D, BinaryOp, DistanceMetric, GradientTiling, RawCubicSpline, RawDensityFunction,
    RawNoiseHolder, RoundOp, ShiftKind, UnaryOp,
};
use crate::interval::java_compare_f32;
use crate::settings::{
    BlockStateSpec, ClimateParameter, DIMENSION_MAX_Y, DIMENSION_MIN_Y, MaterialRuleRef,
    NoiseSettings, SpawnTargetPoint,
};

/// Maximum magnitude for `NOISE_VALUE_CODEC` (`1_000_000.0_f32`, `0x49742400`).
pub const MAX_REASONABLE_NOISE_VALUE: f32 = 1_000_000.0_f32;

/// Minimum allowed `find_top_surface.lower_bound` (`DimensionType.MIN_Y * 2 = -4064`).
pub const FIND_TOP_SURFACE_MIN_LOWER_BOUND: i32 = DIMENSION_MIN_Y * 2;
/// Maximum allowed `find_top_surface.lower_bound` (`DimensionType.MAX_Y * 2 = 4062`).
pub const FIND_TOP_SURFACE_MAX_LOWER_BOUND: i32 = DIMENSION_MAX_Y * 2;

/// Unresolved `AquiferConfig` decoded from JSON before density-function resolution.
#[derive(Debug, Clone)]
pub(crate) struct RawAquiferConfig {
    pub(crate) barrier: RawDensityFunction,
    pub(crate) fluid_level_floodedness: RawDensityFunction,
    pub(crate) fluid_level_spread: RawDensityFunction,
    pub(crate) lava: RawDensityFunction,
    pub(crate) exclusion: RawDensityFunction,
    pub(crate) surface_level: RawDensityFunction,
}

/// Unresolved `NoiseRouter` decoded from JSON.
#[derive(Debug, Clone)]
pub(crate) struct RawNoiseRouter {
    pub(crate) temperature: RawDensityFunction,
    pub(crate) vegetation: RawDensityFunction,
    pub(crate) continents: RawDensityFunction,
    pub(crate) erosion: RawDensityFunction,
    pub(crate) depth: RawDensityFunction,
    pub(crate) ridges: RawDensityFunction,
    pub(crate) chunk_surface_level: RawDensityFunction,
    pub(crate) final_density: RawDensityFunction,
}

/// Unresolved `DebugFunctionEntry` decoded from JSON.
#[derive(Debug, Clone)]
pub(crate) struct RawDebugFunctionEntry {
    pub(crate) label: String,
    pub(crate) function: RawDensityFunction,
}

/// Unresolved `NoiseGeneratorSettings` decoded from JSON.
#[derive(Debug, Clone)]
pub(crate) struct RawNoiseGeneratorSettings {
    pub(crate) noise: NoiseSettings,
    pub(crate) default_block: BlockStateSpec,
    pub(crate) default_fluid: BlockStateSpec,
    pub(crate) noise_router: RawNoiseRouter,
    pub(crate) material_rule: MaterialRuleRef,
    pub(crate) spawn_target: Vec<SpawnTargetPoint>,
    pub(crate) sea_level: i32,
    pub(crate) disable_mob_generation: bool,
    pub(crate) aquifers: Option<RawAquiferConfig>,
    pub(crate) legacy_random_source: bool,
    pub(crate) debug_functions: Vec<RawDebugFunctionEntry>,
}

// ---------------------------------------------------------------------------
// Primitive helper codecs
// ---------------------------------------------------------------------------

fn noise_value_codec() -> impl Codec<f32> {
    validated(FloatCodec, |v: &f32| {
        if (-MAX_REASONABLE_NOISE_VALUE..=MAX_REASONABLE_NOISE_VALUE).contains(v) {
            Ok(())
        } else {
            Err(format!(
                "Value {v} outside of range [{}, {}]",
                -MAX_REASONABLE_NOISE_VALUE, MAX_REASONABLE_NOISE_VALUE
            ))
        }
    })
}

fn int_range_codec(min: i32, max: i32) -> impl Codec<i32> {
    validated(IntCodec, move |v: &i32| {
        if (min..=max).contains(v) {
            Ok(())
        } else {
            Err(format!("Value {v} outside of range [{min}, {max}]"))
        }
    })
}

fn positive_int_codec() -> impl Codec<i32> {
    validated(IntCodec, |v: &i32| {
        if *v >= 1 {
            Ok(())
        } else {
            Err(format!("Value must be positive: {v}"))
        }
    })
}

fn double_range_codec(min: f64, max: f64) -> impl Codec<f64> {
    validated(DoubleCodec, move |v: &f64| {
        if (min..=max).contains(v) {
            Ok(())
        } else {
            Err(format!("Value {v} outside of range [{min}, {max}]"))
        }
    })
}

fn float_range_codec(min: f32, max: f32) -> impl Codec<f32> {
    validated(FloatCodec, move |v: &f32| {
        if (min..=max).contains(v) {
            Ok(())
        } else {
            Err(format!("Value {v} outside of range [{min}, {max}]"))
        }
    })
}

struct Axis3DCodec;
impl Codec<Axis3D> for Axis3DCodec {
    fn decode(&self, input: &Json) -> DataResult<Axis3D> {
        let Some(s) = input.as_str() else {
            return DataResult::error(format!("Not a string: {}", input.type_name()));
        };
        match s {
            "x" => DataResult::Success(Axis3D::X),
            "y" => DataResult::Success(Axis3D::Y),
            "z" => DataResult::Success(Axis3D::Z),
            other => DataResult::error(format!("Invalid axis '{other}': expected x, y, or z")),
        }
    }
    fn encode(&self, _value: &Axis3D) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct GradientTilingCodec;
impl Codec<GradientTiling> for GradientTilingCodec {
    fn decode(&self, input: &Json) -> DataResult<GradientTiling> {
        let Some(s) = input.as_str() else {
            return DataResult::error(format!("Not a string: {}", input.type_name()));
        };
        match s {
            "clamp_to_edge" => DataResult::Success(GradientTiling::ClampToEdge),
            "repeat" => DataResult::Success(GradientTiling::Repeat),
            "mirrored_repeat" => DataResult::Success(GradientTiling::MirroredRepeat),
            other => DataResult::error(format!(
                "Invalid tiling '{other}': expected clamp_to_edge, repeat, or mirrored_repeat"
            )),
        }
    }
    fn encode(&self, _value: &GradientTiling) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct DistanceMetricCodec;
impl Codec<DistanceMetric> for DistanceMetricCodec {
    fn decode(&self, input: &Json) -> DataResult<DistanceMetric> {
        let Some(s) = input.as_str() else {
            return DataResult::error(format!("Not a string: {}", input.type_name()));
        };
        match s {
            "euclidean" => DataResult::Success(DistanceMetric::Euclidean),
            "euclidean_squared" => DataResult::Success(DistanceMetric::EuclideanSquared),
            "manhattan" => DataResult::Success(DistanceMetric::Manhattan),
            "chebyshev" => DataResult::Success(DistanceMetric::Chebyshev),
            other => DataResult::error(format!("Unknown distance metric: {other}")),
        }
    }
    fn encode(&self, _value: &DistanceMetric) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct Vec3iCodec;
impl Codec<[i32; 3]> for Vec3iCodec {
    fn decode(&self, input: &Json) -> DataResult<[i32; 3]> {
        let c = sized_list(IntCodec, 3, 3);
        c.decode(input).and_then(|v| {
            if v.len() == 3 {
                DataResult::Success([v[0], v[1], v[2]])
            } else {
                DataResult::error(format!("Expected 3 integers, got {}", v.len()))
            }
        })
    }
    fn encode(&self, _value: &[i32; 3]) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

// ---------------------------------------------------------------------------
// NormalNoiseParameters codec (`minecraft:worldgen/noise`)
// ---------------------------------------------------------------------------

struct NormalizationModeCodec;
impl Codec<NormalizationMode> for NormalizationModeCodec {
    fn decode(&self, input: &Json) -> DataResult<NormalizationMode> {
        match input {
            Json::Bool(true) => DataResult::Success(NormalizationMode::Enabled),
            Json::Bool(false) => DataResult::Success(NormalizationMode::Disabled),
            Json::String(s) if s == "legacy" => DataResult::Success(NormalizationMode::Legacy),
            _ => DataResult::error(format!(
                "Invalid normalize mode: expected true, false, or \"legacy\", got {input}"
            )),
        }
    }
    fn encode(&self, _value: &NormalizationMode) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

/// Decodes a `minecraft:worldgen/noise` JSON object into [`NormalNoiseParameters`].
pub fn decode_normal_noise_params(input: &Json) -> DataResult<NormalNoiseParameters> {
    let mut r = RecordReader::new(input);
    let base_amplitude = r.optional(
        "base_amplitude",
        &double_range_codec(MIN_BASE_AMPLITUDE, MAX_AMPLITUDE),
        1.0_f64,
    );
    let base_octave = r.required(
        "base_octave",
        &int_range_codec(MIN_BASE_OCTAVE, MAX_BASE_OCTAVE),
    );
    let octave_count = r.optional("octave_count", &int_range_codec(1, MAX_OCTAVES as i32), 1);
    let normalize = r.optional(
        "normalize",
        &NormalizationModeCodec,
        NormalizationMode::Enabled,
    );
    let amplitude_modifiers = r.optional(
        "amplitude_modifiers",
        &sized_list(double_range_codec(0.0, MAX_AMPLITUDE), 0, MAX_OCTAVES),
        Vec::new(),
    );

    if !r.has_errors()
        && !amplitude_modifiers.is_empty()
        && amplitude_modifiers.len() != octave_count as usize
    {
        r.push_error(format!(
            "amplitude_modifiers length {} does not match octave_count {octave_count}",
            amplitude_modifiers.len()
        ));
    }

    r.finish(|| {
        let base_octave = base_octave?;
        NormalNoiseParameters::new(
            base_amplitude,
            base_octave,
            octave_count as usize,
            normalize,
            amplitude_modifiers,
        )
        .ok()
    })
}

struct RawNoiseHolderCodec;
impl Codec<RawNoiseHolder> for RawNoiseHolderCodec {
    fn decode(&self, input: &Json) -> DataResult<RawNoiseHolder> {
        match input {
            Json::String(_) => IdentifierCodec.decode(input).map(RawNoiseHolder::Reference),
            Json::Object(_) => decode_normal_noise_params(input).map(RawNoiseHolder::Inline),
            other => DataResult::error(format!(
                "Expected noise identifier string or inline noise object, got {}",
                other.type_name()
            )),
        }
    }
    fn encode(&self, _value: &RawNoiseHolder) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

// ---------------------------------------------------------------------------
// CubicSpline codec (`minecraft:spline`)
// ---------------------------------------------------------------------------

struct RawSplinePoint {
    location: f32,
    value: RawCubicSpline,
    derivative: f32,
}

struct RawSplinePointCodec;
impl Codec<RawSplinePoint> for RawSplinePointCodec {
    fn decode(&self, input: &Json) -> DataResult<RawSplinePoint> {
        let mut r = RecordReader::new(input);
        let location = r.required("location", &FloatCodec);
        let value = r.required("value", &RawCubicSplineCodec);
        let derivative = r.required("derivative", &FloatCodec);
        r.finish(|| {
            Some(RawSplinePoint {
                location: location?,
                value: value?,
                derivative: derivative?,
            })
        })
    }
    fn encode(&self, _value: &RawSplinePoint) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct RawCubicSplineCodec;
impl Codec<RawCubicSpline> for RawCubicSplineCodec {
    fn decode(&self, input: &Json) -> DataResult<RawCubicSpline> {
        decode_raw_cubic_spline(input)
    }
    fn encode(&self, _value: &RawCubicSpline) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

pub(crate) fn decode_raw_cubic_spline(input: &Json) -> DataResult<RawCubicSpline> {
    match input {
        Json::Number(_) => FloatCodec.decode(input).map(RawCubicSpline::Constant),
        Json::Object(_) => {
            let mut r = RecordReader::new(input);
            let coordinate = r.required("coordinate", &RawDensityFunctionCodec);
            let points = r.required("points", &sized_list(RawSplinePointCodec, 1, usize::MAX));
            r.finish(|| {
                let coordinate = coordinate?;
                let points = points?;
                let mut locations = Vec::with_capacity(points.len());
                let mut values = Vec::with_capacity(points.len());
                let mut derivatives = Vec::with_capacity(points.len());
                for pt in points {
                    locations.push(pt.location);
                    values.push(pt.value);
                    derivatives.push(pt.derivative);
                }
                Some(RawCubicSpline::Multipoint {
                    coordinate: Box::new(coordinate),
                    locations,
                    values,
                    derivatives,
                })
            })
        }
        other => DataResult::error(format!(
            "Expected float constant or multipoint spline object, got {}",
            other.type_name()
        )),
    }
}

// ---------------------------------------------------------------------------
// DensityFunction codec (all 44 types + literal + reference)
// ---------------------------------------------------------------------------

struct RawDensityFunctionCodec;
impl Codec<RawDensityFunction> for RawDensityFunctionCodec {
    fn decode(&self, input: &Json) -> DataResult<RawDensityFunction> {
        decode_raw_density_function(input)
    }
    fn encode(&self, _value: &RawDensityFunction) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

pub(crate) fn decode_raw_density_function(input: &Json) -> DataResult<RawDensityFunction> {
    match input {
        Json::String(_) => IdentifierCodec
            .decode(input)
            .map(RawDensityFunction::Reference),
        Json::Number(_) => noise_value_codec()
            .decode(input)
            .map(RawDensityFunction::Constant),
        Json::Object(_) => decode_dispatch(input, "type", |ty_raw, obj| {
            let Ok(id) = Identifier::parse(ty_raw) else {
                return Some(DataResult::error(format!(
                    "Not a valid resource location: {ty_raw}"
                )));
            };
            if id.namespace() != "minecraft" {
                return None;
            }
            decode_density_function_variant(id.path(), obj)
        }),
        other => DataResult::error(format!(
            "Expected density function identifier, number, or object, got {}",
            other.type_name()
        )),
    }
}

fn decode_unary(op: UnaryOp, input: &Json) -> DataResult<RawDensityFunction> {
    let mut r = RecordReader::new(input);
    let inner = r.required("input", &RawDensityFunctionCodec);
    r.finish(|| {
        Some(RawDensityFunction::Unary {
            op,
            input: Box::new(inner?),
        })
    })
}

fn decode_round(op: RoundOp, input: &Json) -> DataResult<RawDensityFunction> {
    let mut r = RecordReader::new(input);
    let inner = r.required("input", &RawDensityFunctionCodec);
    let multiple = r.optional(
        "multiple",
        &RawDensityFunctionCodec,
        RawDensityFunction::Constant(1.0_f32),
    );
    r.finish(|| {
        Some(RawDensityFunction::Round {
            op,
            input: Box::new(inner?),
            multiple: Box::new(multiple),
        })
    })
}

fn decode_binary(op: BinaryOp, input: &Json) -> DataResult<RawDensityFunction> {
    let mut r = RecordReader::new(input);
    let left = r.required("left", &RawDensityFunctionCodec);
    let right = r.required("right", &RawDensityFunctionCodec);
    r.finish(|| {
        Some(RawDensityFunction::Binary {
            op,
            left: Box::new(left?),
            right: Box::new(right?),
        })
    })
}

fn decode_shift(kind: ShiftKind, input: &Json) -> DataResult<RawDensityFunction> {
    let mut r = RecordReader::new(input);
    let noise = r.required("noise", &RawNoiseHolderCodec);
    r.finish(|| {
        Some(RawDensityFunction::Shift {
            kind,
            noise: noise?,
        })
    })
}

fn decode_density_function_variant(
    name: &str,
    input: &Json,
) -> Option<DataResult<RawDensityFunction>> {
    match name {
        "constant" => Some({
            let mut r = RecordReader::new(input);
            let value = r.required("value", &noise_value_codec());
            r.finish(|| Some(RawDensityFunction::Constant(value?)))
        }),
        "blend_alpha" => Some(DataResult::Success(RawDensityFunction::BlendAlpha)),
        "blend_offset" => Some(DataResult::Success(RawDensityFunction::BlendOffset)),
        "beardifier" => Some(DataResult::Success(RawDensityFunction::Beardifier)),
        "end_outer_islands" => Some(DataResult::Success(RawDensityFunction::EndOuterIslands)),
        "noise" => Some({
            let mut r = RecordReader::new(input);
            let noise = r.required("noise", &RawNoiseHolderCodec);
            let xz_scale = r.required("xz_scale", &DoubleCodec);
            let y_scale = r.required("y_scale", &DoubleCodec);
            let shift_x = r.optional(
                "shift_x",
                &RawDensityFunctionCodec,
                RawDensityFunction::Constant(0.0_f32),
            );
            let shift_y = r.optional(
                "shift_y",
                &RawDensityFunctionCodec,
                RawDensityFunction::Constant(0.0_f32),
            );
            let shift_z = r.optional(
                "shift_z",
                &RawDensityFunctionCodec,
                RawDensityFunction::Constant(0.0_f32),
            );
            r.finish(|| {
                Some(RawDensityFunction::Noise {
                    noise: noise?,
                    xz_scale: xz_scale?,
                    y_scale: y_scale?,
                    shift_x: Box::new(shift_x),
                    shift_y: Box::new(shift_y),
                    shift_z: Box::new(shift_z),
                })
            })
        }),
        "shift" => Some(decode_shift(ShiftKind::Shift, input)),
        "shift_a" => Some(decode_shift(ShiftKind::ShiftA, input)),
        "shift_b" => Some(decode_shift(ShiftKind::ShiftB, input)),
        "distance_to_point" => Some({
            let mut r = RecordReader::new(input);
            let point = r.required("point", &Vec3iCodec);
            let metric = r.required("metric", &DistanceMetricCodec);
            r.finish(|| {
                Some(RawDensityFunction::DistanceToPoint {
                    point: point?,
                    metric: metric?,
                })
            })
        }),
        "gradient" => Some({
            let mut r = RecordReader::new(input);
            let axis = r.required("axis", &Axis3DCodec);
            let tiling = r.optional("tiling", &GradientTilingCodec, GradientTiling::ClampToEdge);
            let from_coordinate = r.required("from_coordinate", &IntCodec);
            let to_coordinate = r.required("to_coordinate", &IntCodec);
            let from_value = r.required("from_value", &noise_value_codec());
            let to_value = r.required("to_value", &noise_value_codec());
            if let (Some(fc), Some(tc)) = (from_coordinate, to_coordinate)
                && fc == tc
            {
                r.push_error(format!(
                    "gradient from_coordinate and to_coordinate must differ (both {fc})"
                ));
            }
            r.finish(|| {
                Some(RawDensityFunction::Gradient {
                    axis: axis?,
                    tiling,
                    from_coordinate: from_coordinate?,
                    to_coordinate: to_coordinate?,
                    from_value: from_value?,
                    to_value: to_value?,
                })
            })
        }),
        "old_blended_noise" => Some({
            let mut r = RecordReader::new(input);
            let scale_codec = double_range_codec(0.001, 1000.0);
            let xz_scale = r.required("xz_scale", &scale_codec);
            let y_scale = r.required("y_scale", &scale_codec);
            let xz_factor = r.required("xz_factor", &scale_codec);
            let y_factor = r.required("y_factor", &scale_codec);
            let smear_scale_multiplier =
                r.required("smear_scale_multiplier", &double_range_codec(1.0, 8.0));
            r.finish(|| {
                let params = BlendedNoiseParameters::new(
                    xz_scale?,
                    y_scale?,
                    xz_factor?,
                    y_factor?,
                    smear_scale_multiplier?,
                )
                .ok()?;
                Some(RawDensityFunction::OldBlendedNoise(params))
            })
        }),

        // 11 unary functions
        "abs" => Some(decode_unary(UnaryOp::Abs, input)),
        "square" => Some(decode_unary(UnaryOp::Square, input)),
        "cube" => Some(decode_unary(UnaryOp::Cube, input)),
        "sqrt" => Some(decode_unary(UnaryOp::Sqrt, input)),
        "half_negative" => Some(decode_unary(UnaryOp::HalfNegative, input)),
        "quarter_negative" => Some(decode_unary(UnaryOp::QuarterNegative, input)),
        "reciprocal" => Some(decode_unary(UnaryOp::Reciprocal, input)),
        "negate" => Some(decode_unary(UnaryOp::Negate, input)),
        "squeeze" => Some(decode_unary(UnaryOp::Squeeze, input)),
        "log" => Some(decode_unary(UnaryOp::Log, input)),
        "sign" => Some(decode_unary(UnaryOp::Sign, input)),

        // 4 rounding functions
        "floor" => Some(decode_round(RoundOp::Floor, input)),
        "round" => Some(decode_round(RoundOp::Round, input)),
        "ceil" => Some(decode_round(RoundOp::Ceil, input)),
        "truncate" => Some(decode_round(RoundOp::Truncate, input)),

        // 6 binary functions
        "add" => Some(decode_binary(BinaryOp::Add, input)),
        "sub" => Some(decode_binary(BinaryOp::Sub, input)),
        "mul" => Some(decode_binary(BinaryOp::Mul, input)),
        "div" => Some(decode_binary(BinaryOp::Div, input)),
        "min" => Some(decode_binary(BinaryOp::Min, input)),
        "max" => Some(decode_binary(BinaryOp::Max, input)),

        // 11 power / interpolation / clamping / selection / structural functions
        "pow" => Some({
            let mut r = RecordReader::new(input);
            let base = r.required("base", &RawDensityFunctionCodec);
            let exponent = r.required("exponent", &RawDensityFunctionCodec);
            r.finish(|| {
                Some(RawDensityFunction::Pow {
                    base: Box::new(base?),
                    exponent: Box::new(exponent?),
                })
            })
        }),
        "lerp" => Some({
            let mut r = RecordReader::new(input);
            let alpha = r.required("alpha", &RawDensityFunctionCodec);
            let first = r.required("first", &RawDensityFunctionCodec);
            let second = r.required("second", &RawDensityFunctionCodec);
            r.finish(|| {
                Some(RawDensityFunction::Lerp {
                    alpha: Box::new(alpha?),
                    first: Box::new(first?),
                    second: Box::new(second?),
                })
            })
        }),
        "clamp" => Some({
            let mut r = RecordReader::new(input);
            let inner = r.required("input", &RawDensityFunctionCodec);
            let min = r.required("min", &noise_value_codec());
            let max = r.required("max", &noise_value_codec());
            if let (Some(lo), Some(hi)) = (min, max)
                && hi < lo
            {
                r.push_error(format!("clamp max ({hi}) must be >= min ({lo})"));
            }
            r.finish(|| {
                Some(RawDensityFunction::Clamp {
                    input: Box::new(inner?),
                    min: min?,
                    max: max?,
                })
            })
        }),
        "range_choice" => Some({
            let mut r = RecordReader::new(input);
            let inner = r.required("input", &RawDensityFunctionCodec);
            let min_inclusive = r.required("min_inclusive", &noise_value_codec());
            let max_exclusive = r.required("max_exclusive", &noise_value_codec());
            let when_in_range = r.required("when_in_range", &RawDensityFunctionCodec);
            let when_out_of_range = r.required("when_out_of_range", &RawDensityFunctionCodec);
            r.finish(|| {
                Some(RawDensityFunction::RangeChoice {
                    input: Box::new(inner?),
                    min_inclusive: min_inclusive?,
                    max_exclusive: max_exclusive?,
                    when_in_range: Box::new(when_in_range?),
                    when_out_of_range: Box::new(when_out_of_range?),
                })
            })
        }),
        "interval_select" => Some({
            let mut r = RecordReader::new(input);
            let inner = r.required("input", &RawDensityFunctionCodec);
            let thresholds = r.required("thresholds", &list(noise_value_codec()));
            let functions = r.required(
                "functions",
                &sized_list(RawDensityFunctionCodec, 2, usize::MAX),
            );
            if let (Some(ts), Some(fs)) = (&thresholds, &functions) {
                if ts.len() + 1 != fs.len() {
                    r.push_error(format!(
                        "interval_select thresholds length ({}) must equal functions length - 1 ({})",
                        ts.len(),
                        fs.len().saturating_sub(1)
                    ));
                }
                for w in ts.windows(2) {
                    if java_compare_f32(w[0], w[1]) == Ordering::Greater {
                        r.push_error(format!(
                            "interval_select thresholds must be non-decreasing: {} > {}",
                            w[0], w[1]
                        ));
                        break;
                    }
                }
            }
            r.finish(|| {
                Some(RawDensityFunction::IntervalSelect {
                    input: Box::new(inner?),
                    thresholds: thresholds?,
                    functions: functions?,
                })
            })
        }),
        "spline" => Some({
            let mut r = RecordReader::new(input);
            let spline = r.required("spline", &RawCubicSplineCodec);
            r.finish(|| Some(RawDensityFunction::Spline(spline?)))
        }),
        "cache" => Some({
            let mut r = RecordReader::new(input);
            let inner = r.required("input", &RawDensityFunctionCodec);
            r.finish(|| Some(RawDensityFunction::Cache(Box::new(inner?))))
        }),
        "blend_density" => Some({
            let mut r = RecordReader::new(input);
            let inner = r.required("input", &RawDensityFunctionCodec);
            r.finish(|| Some(RawDensityFunction::BlendDensity(Box::new(inner?))))
        }),
        "interpolated" => Some({
            let mut r = RecordReader::new(input);
            let inner = r.required("input", &RawDensityFunctionCodec);
            let cell_size_xz = r.required("cell_size_xz", &positive_int_codec());
            let cell_size_y = r.required("cell_size_y", &positive_int_codec());
            r.finish(|| {
                Some(RawDensityFunction::Interpolated {
                    input: Box::new(inner?),
                    cell_size_xz: cell_size_xz?,
                    cell_size_y: cell_size_y?,
                })
            })
        }),
        "slice" => Some({
            let mut r = RecordReader::new(input);
            let axis = r.required("axis", &Axis3DCodec);
            let coordinate = r.required("coordinate", &IntCodec);
            let inner = r.required("input", &RawDensityFunctionCodec);
            r.finish(|| {
                Some(RawDensityFunction::Slice {
                    axis: axis?,
                    coordinate: coordinate?,
                    input: Box::new(inner?),
                })
            })
        }),
        "find_top_surface" => Some({
            let mut r = RecordReader::new(input);
            let density = r.required("density", &RawDensityFunctionCodec);
            let upper_bound = r.required("upper_bound", &RawDensityFunctionCodec);
            let lower_bound = r.required(
                "lower_bound",
                &int_range_codec(
                    FIND_TOP_SURFACE_MIN_LOWER_BOUND,
                    FIND_TOP_SURFACE_MAX_LOWER_BOUND,
                ),
            );
            let cell_height = r.required("cell_height", &positive_int_codec());
            r.finish(|| {
                Some(RawDensityFunction::FindTopSurface {
                    density: Box::new(density?),
                    upper_bound: Box::new(upper_bound?),
                    lower_bound: lower_bound?,
                    cell_height: cell_height?,
                })
            })
        }),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// NoiseSettings, NoiseRouter, AquiferConfig, BlockStateSpec, SpawnTargetPoint,
// NoiseGeneratorSettings codecs
// ---------------------------------------------------------------------------

struct NoiseSettingsCodec;
impl Codec<NoiseSettings> for NoiseSettingsCodec {
    fn decode(&self, input: &Json) -> DataResult<NoiseSettings> {
        let mut r = RecordReader::new(input);
        let min_y = r.required("min_y", &int_range_codec(DIMENSION_MIN_Y, DIMENSION_MAX_Y));
        let height = r.required(
            "height",
            &int_range_codec(0, crate::settings::DIMENSION_Y_SIZE),
        );
        if let (Some(my), Some(h)) = (min_y, height)
            && let Err(msg) = NoiseSettings::new(my, h)
        {
            r.push_error(msg);
        }
        r.finish(|| NoiseSettings::new(min_y?, height?).ok())
    }
    fn encode(&self, _value: &NoiseSettings) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct BlockStateSpecCodec;
impl Codec<BlockStateSpec> for BlockStateSpecCodec {
    fn decode(&self, input: &Json) -> DataResult<BlockStateSpec> {
        match input {
            Json::String(_) => IdentifierCodec.decode(input).map(|name| BlockStateSpec {
                name,
                properties: BTreeMap::new(),
            }),
            // Data version ≥ 5006 form (registry spec §3.3): `{"id", "properties"}`. `properties` is lenient: a
            // missing or non-map value means the default state, and a non-string value leaves that property at its
            // default. Property names are checked against the block when the state is resolved.
            Json::Object(obj) => {
                let mut r = RecordReader::new(input);
                let name = r.required("id", &IdentifierCodec);
                let properties: BTreeMap<String, String> = match obj.get("properties") {
                    Some(Json::Object(props)) => props
                        .iter()
                        .filter_map(|(k, v)| Some((k.as_str()?.to_owned(), v.as_str()?.to_owned())))
                        .collect(),
                    _ => BTreeMap::new(),
                };
                r.finish(|| {
                    Some(BlockStateSpec {
                        name: name?,
                        properties,
                    })
                })
            }
            other => DataResult::error(format!(
                "Expected block state identifier string or object, got {}",
                other.type_name()
            )),
        }
    }
    fn encode(&self, _value: &BlockStateSpec) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct RawNoiseRouterCodec;
impl Codec<RawNoiseRouter> for RawNoiseRouterCodec {
    fn decode(&self, input: &Json) -> DataResult<RawNoiseRouter> {
        let mut r = RecordReader::new(input);
        let temperature = r.required("temperature", &RawDensityFunctionCodec);
        let vegetation = r.required("vegetation", &RawDensityFunctionCodec);
        let continents = r.required("continents", &RawDensityFunctionCodec);
        let erosion = r.required("erosion", &RawDensityFunctionCodec);
        let depth = r.required("depth", &RawDensityFunctionCodec);
        let ridges = r.required("ridges", &RawDensityFunctionCodec);
        let chunk_surface_level = r.required("chunk_surface_level", &RawDensityFunctionCodec);
        let final_density = r.required("final_density", &RawDensityFunctionCodec);
        r.finish(|| {
            Some(RawNoiseRouter {
                temperature: temperature?,
                vegetation: vegetation?,
                continents: continents?,
                erosion: erosion?,
                depth: depth?,
                ridges: ridges?,
                chunk_surface_level: chunk_surface_level?,
                final_density: final_density?,
            })
        })
    }
    fn encode(&self, _value: &RawNoiseRouter) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct RawAquiferConfigCodec;
impl Codec<RawAquiferConfig> for RawAquiferConfigCodec {
    fn decode(&self, input: &Json) -> DataResult<RawAquiferConfig> {
        let mut r = RecordReader::new(input);
        let barrier = r.required("barrier", &RawDensityFunctionCodec);
        let fluid_level_floodedness =
            r.required("fluid_level_floodedness", &RawDensityFunctionCodec);
        let fluid_level_spread = r.required("fluid_level_spread", &RawDensityFunctionCodec);
        let lava = r.required("lava", &RawDensityFunctionCodec);
        let exclusion = r.required("exclusion", &RawDensityFunctionCodec);
        let surface_level = r.required("surface_level", &RawDensityFunctionCodec);
        r.finish(|| {
            Some(RawAquiferConfig {
                barrier: barrier?,
                fluid_level_floodedness: fluid_level_floodedness?,
                fluid_level_spread: fluid_level_spread?,
                lava: lava?,
                exclusion: exclusion?,
                surface_level: surface_level?,
            })
        })
    }
    fn encode(&self, _value: &RawAquiferConfig) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct MaterialRuleRefCodec;
impl Codec<MaterialRuleRef> for MaterialRuleRefCodec {
    fn decode(&self, input: &Json) -> DataResult<MaterialRuleRef> {
        match input {
            Json::String(_) => IdentifierCodec
                .decode(input)
                .map(MaterialRuleRef::Reference),
            Json::Object(_) => DataResult::Success(MaterialRuleRef::Inline(input.clone())),
            other => DataResult::error(format!(
                "Expected material_rule identifier string or object, got {}",
                other.type_name()
            )),
        }
    }
    fn encode(&self, _value: &MaterialRuleRef) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct ClimateParameterCodec;
impl Codec<ClimateParameter> for ClimateParameterCodec {
    fn decode(&self, input: &Json) -> DataResult<ClimateParameter> {
        let f_codec = float_range_codec(-2.0_f32, 2.0_f32);
        match input {
            Json::Number(_) => f_codec.decode(input).map(|v| {
                let q = ClimateParameter::quantize(v);
                ClimateParameter { min: q, max: q }
            }),
            Json::Array(_) => sized_list(f_codec, 2, 2).decode(input).and_then(|vals| {
                let (lo, hi) = (vals[0], vals[1]);
                if java_compare_f32(lo, hi) == Ordering::Greater {
                    DataResult::error(format!(
                        "Climate parameter min ({lo}) must be <= max ({hi})"
                    ))
                } else {
                    DataResult::Success(ClimateParameter {
                        min: ClimateParameter::quantize(lo),
                        max: ClimateParameter::quantize(hi),
                    })
                }
            }),
            Json::Object(_) => {
                let mut r = RecordReader::new(input);
                let lo = r.required("min", &f_codec);
                let hi = r.required("max", &f_codec);
                if let (Some(a), Some(b)) = (lo, hi)
                    && java_compare_f32(a, b) == Ordering::Greater
                {
                    r.push_error(format!("Climate parameter min ({a}) must be <= max ({b})"));
                }
                r.finish(|| {
                    Some(ClimateParameter {
                        min: ClimateParameter::quantize(lo?),
                        max: ClimateParameter::quantize(hi?),
                    })
                })
            }
            other => DataResult::error(format!(
                "Expected climate parameter float, [min, max], or {{min, max}}, got {}",
                other.type_name()
            )),
        }
    }
    fn encode(&self, _value: &ClimateParameter) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct SpawnTargetPointCodec;
impl Codec<SpawnTargetPoint> for SpawnTargetPointCodec {
    fn decode(&self, input: &Json) -> DataResult<SpawnTargetPoint> {
        let Some(obj) = input.as_object() else {
            return DataResult::error(format!("Not a JSON object: {}", input.type_name()));
        };
        let mut entries = Vec::with_capacity(obj.len());
        let mut errors = Vec::new();
        for (k, v) in obj.iter() {
            let id = match k.as_str().map(Identifier::parse) {
                Some(Ok(id)) => id,
                _ => {
                    errors.push(format!("Entry {k}: Not a valid resource location"));
                    continue;
                }
            };
            match ClimateParameterCodec.decode(v) {
                DataResult::Success(param) => entries.push((id, param)),
                DataResult::Error { message, .. } => {
                    errors.push(format!("Entry {k}: {message}"));
                }
                DataResult::Failure(m) => return DataResult::Failure(m),
            }
        }
        if errors.is_empty() {
            DataResult::Success(SpawnTargetPoint { entries })
        } else {
            DataResult::error_with_partial(errors.join("; "), SpawnTargetPoint { entries })
        }
    }
    fn encode(&self, _value: &SpawnTargetPoint) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct RawDebugFunctionEntryCodec;
impl Codec<RawDebugFunctionEntry> for RawDebugFunctionEntryCodec {
    fn decode(&self, input: &Json) -> DataResult<RawDebugFunctionEntry> {
        let mut r = RecordReader::new(input);
        let label = r.required("label", &StringCodec);
        let function = r.required("function", &RawDensityFunctionCodec);
        r.finish(|| {
            Some(RawDebugFunctionEntry {
                label: label?,
                function: function?,
            })
        })
    }
    fn encode(&self, _value: &RawDebugFunctionEntry) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

pub(crate) fn decode_raw_noise_generator_settings(
    input: &Json,
) -> DataResult<RawNoiseGeneratorSettings> {
    let mut r = RecordReader::new(input);
    let noise = r.required("noise", &NoiseSettingsCodec);
    let default_block = r.required("default_block", &BlockStateSpecCodec);
    let default_fluid = r.required("default_fluid", &BlockStateSpecCodec);
    let noise_router = r.required("noise_router", &RawNoiseRouterCodec);
    let material_rule = r.required("material_rule", &MaterialRuleRefCodec);
    let spawn_target = r.required("spawn_target", &list(SpawnTargetPointCodec));
    let sea_level = r.required("sea_level", &IntCodec);
    let disable_mob_generation = r.required("disable_mob_generation", &BoolCodec);
    let aquifers = r.optional_opt("aquifers", &RawAquiferConfigCodec);
    let legacy_random_source = r.required("legacy_random_source", &BoolCodec);
    let debug_functions = r.optional(
        "debug_functions",
        &list(RawDebugFunctionEntryCodec),
        Vec::new(),
    );
    r.finish(|| {
        Some(RawNoiseGeneratorSettings {
            noise: noise?,
            default_block: default_block?,
            default_fluid: default_fluid?,
            noise_router: noise_router?,
            material_rule: material_rule?,
            spawn_target: spawn_target?,
            sea_level: sea_level?,
            disable_mob_generation: disable_mob_generation?,
            aquifers,
            legacy_random_source: legacy_random_source?,
            debug_functions,
        })
    })
}
