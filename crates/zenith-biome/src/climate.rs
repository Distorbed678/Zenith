//! Climate parameter space, quantization, intervals, distance metric, and JSON codecs.
//!
//! Implements Section 1 of `docs/specs/worldgen/biomes-and-climate.md`.

use std::cmp::Ordering;

use thiserror::Error;
use zenith_codec::codec::{Codec, DataResult, FloatCodec, RecordReader, RecordWriter};
use zenith_codec::json::Json;
use zenith_math::helpers::rounding::square_i64;

/// Climate coordinate quantization scale (`10000.0f`, IEEE-754 `0x461C4000`).
pub const QUANTIZATION_FACTOR: f32 = 10_000.0_f32;

/// Number of dimensions in the climate R-tree (`T, H, C, E, D, W, offset`).
pub const PARAMETER_COUNT: usize = 7;

/// Minimum allowed JSON endpoint for a climate [`Parameter`].
pub const PARAMETER_JSON_MIN: f32 = -2.0_f32;

/// Maximum allowed JSON endpoint for a climate [`Parameter`].
pub const PARAMETER_JSON_MAX: f32 = 2.0_f32;

/// Minimum allowed JSON value for [`ParameterPoint::offset`].
pub const OFFSET_JSON_MIN: f32 = 0.0_f32;

/// Maximum allowed JSON value for [`ParameterPoint::offset`].
pub const OFFSET_JSON_MAX: f32 = 1.0_f32;

/// Errors raised when constructing climate parameters or volumes.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ClimateError {
    /// Interval lower bound exceeded upper bound (`min > max`).
    #[error("invalid climate parameter interval: min ({min}) > max ({max})")]
    InvalidSpan {
        /// Lower bound description.
        min: String,
        /// Upper bound description.
        max: String,
    },

    /// Precomputed climate volume dimensions or channel buffer lengths did not match.
    #[error(
        "invalid climate volume: expected {expected} samples for ({size_x}x{size_y}x{size_z}), channel {channel} has {actual}"
    )]
    InvalidVolumeLength {
        /// Volume X extent in quarts.
        size_x: usize,
        /// Volume Y extent in quarts.
        size_y: usize,
        /// Volume Z extent in quarts.
        size_z: usize,
        /// Expected sample count (`size_x * size_y * size_z`).
        expected: usize,
        /// Offending channel index (`0..6`).
        channel: usize,
        /// Actual buffer length.
        actual: usize,
    },

    /// Volume dimensions overflowed `usize`.
    #[error("climate volume dimensions ({size_x}x{size_y}x{size_z}) overflow usize")]
    VolumeSizeOverflow {
        /// Volume X extent in quarts.
        size_x: usize,
        /// Volume Y extent in quarts.
        size_y: usize,
        /// Volume Z extent in quarts.
        size_z: usize,
    },
}

/// Java `Float.compare(float, float)` ordering:
/// `-0.0 < +0.0`, all `NaN` values are equal and greater than `+Infinity`.
#[inline]
pub fn java_float_compare(a: f32, b: f32) -> Ordering {
    if a.is_nan() {
        return if b.is_nan() {
            Ordering::Equal
        } else {
            Ordering::Greater
        };
    }
    if b.is_nan() {
        return Ordering::Less;
    }
    if a < b {
        Ordering::Less
    } else if a > b {
        Ordering::Greater
    } else {
        // Numerically equal: distinguish -0.0 (0x8000_0000 -> i32::MIN) < +0.0 (0).
        (a.to_bits() as i32).cmp(&(b.to_bits() as i32))
    }
}

/// Quantizes a router `f32` value into the 64-bit integer climate space:
/// `trunc_i64(x *_{f32} 10000.0f)`.
///
/// Multiplication is performed strictly in `f32` before saturating truncation toward zero
/// (so `-0.78f` quantizes to `-7799`, not `-7800`).
#[inline]
pub fn quantize_coord(v: f32) -> i64 {
    (v * QUANTIZATION_FACTOR) as i64
}

/// Converts a quantized climate coordinate back to `f32`: `(q as f32) /_{f32} 10000.0f`.
#[inline]
pub fn unquantize_coord(q: i64) -> f32 {
    (q as f32) / QUANTIZATION_FACTOR
}

/// A 6-channel quantized climate sample at one position.
///
/// When embedded into the 7D R-tree search vector via [`TargetPoint::to_parameter_array`],
/// the 7th slot (`offset`, index `6`) is always `0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TargetPoint {
    /// Quantized `temperature` channel (index 0).
    pub temperature: i64,
    /// Quantized `humidity` (`vegetation`) channel (index 1).
    pub humidity: i64,
    /// Quantized `continentalness` (`continents`) channel (index 2).
    pub continentalness: i64,
    /// Quantized `erosion` channel (index 3).
    pub erosion: i64,
    /// Quantized `depth` channel (index 4).
    pub depth: i64,
    /// Quantized `weirdness` (`ridges`) channel (index 5).
    pub weirdness: i64,
}

impl TargetPoint {
    /// Constructs a [`TargetPoint`] from already-quantized coordinates.
    #[inline]
    pub const fn new(
        temperature: i64,
        humidity: i64,
        continentalness: i64,
        erosion: i64,
        depth: i64,
        weirdness: i64,
    ) -> Self {
        Self {
            temperature,
            humidity,
            continentalness,
            erosion,
            depth,
            weirdness,
        }
    }

    /// Constructs a [`TargetPoint`] by quantizing six raw `f32` router values.
    #[inline]
    pub fn from_floats(
        temperature: f32,
        humidity: f32,
        continentalness: f32,
        erosion: f32,
        depth: f32,
        weirdness: f32,
    ) -> Self {
        Self {
            temperature: quantize_coord(temperature),
            humidity: quantize_coord(humidity),
            continentalness: quantize_coord(continentalness),
            erosion: quantize_coord(erosion),
            depth: quantize_coord(depth),
            weirdness: quantize_coord(weirdness),
        }
    }

    /// Constructs a [`TargetPoint`] from an array `[T, H, C, E, D, W]` of raw `f32` router values.
    #[inline]
    pub fn from_array(values: [f32; 6]) -> Self {
        Self::from_floats(
            values[0], values[1], values[2], values[3], values[4], values[5],
        )
    }

    /// Expands this target point into the 7D R-tree target vector `[T, H, C, E, D, W, 0]`.
    #[inline]
    pub const fn to_parameter_array(self) -> [i64; PARAMETER_COUNT] {
        [
            self.temperature,
            self.humidity,
            self.continentalness,
            self.erosion,
            self.depth,
            self.weirdness,
            0,
        ]
    }
}

/// A closed quantized climate interval `[min, max]` with `min <= max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Parameter {
    /// Inclusive lower bound in quantized units.
    pub min: i64,
    /// Inclusive upper bound in quantized units.
    pub max: i64,
}

impl Parameter {
    /// Creates a degenerate point interval `[quantize(v), quantize(v)]`.
    #[inline]
    pub fn point(v: f32) -> Self {
        let q = quantize_coord(v);
        Self { min: q, max: q }
    }

    /// Creates a closed interval from raw `f32` bounds, failing if `lo > hi`.
    #[inline]
    pub fn span(lo: f32, hi: f32) -> Result<Self, ClimateError> {
        if lo > hi {
            return Err(ClimateError::InvalidSpan {
                min: lo.to_string(),
                max: hi.to_string(),
            });
        }
        Ok(Self {
            min: quantize_coord(lo),
            max: quantize_coord(hi),
        })
    }

    /// Creates a closed interval `[a.min, b.max]`, failing if `a.min > b.max`.
    #[inline]
    pub fn span_params(a: Parameter, b: Parameter) -> Result<Self, ClimateError> {
        if a.min > b.max {
            return Err(ClimateError::InvalidSpan {
                min: a.min.to_string(),
                max: b.max.to_string(),
            });
        }
        Ok(Self {
            min: a.min,
            max: b.max,
        })
    }

    /// Returns the union span covering `a` and optional `b`:
    /// `a` if `b` is `None`, else `[min(a.min, b.min), max(a.max, b.max)]`.
    #[inline]
    pub const fn union(a: Parameter, b: Option<Parameter>) -> Self {
        match b {
            None => a,
            Some(other) => Self {
                min: if a.min < other.min { a.min } else { other.min },
                max: if a.max > other.max { a.max } else { other.max },
            },
        }
    }

    /// Constructs a [`Parameter`] directly from quantized `i64` bounds, failing if `min > max`.
    #[inline]
    pub fn from_quantized(min: i64, max: i64) -> Result<Self, ClimateError> {
        if min > max {
            return Err(ClimateError::InvalidSpan {
                min: min.to_string(),
                max: max.to_string(),
            });
        }
        Ok(Self { min, max })
    }

    /// Computes the 1D distance from this interval to a quantized coordinate `target`.
    ///
    /// Returns `0` when `min <= target <= max`, and the positive gap otherwise, using wrapping
    /// 64-bit subtraction matching Java `Climate.Parameter#distance(long)`.
    #[inline]
    pub const fn distance(self, target: i64) -> i64 {
        let above = target.wrapping_sub(self.max);
        let below = self.min.wrapping_sub(target);
        if above > 0 {
            above
        } else if below > 0 {
            below
        } else {
            0
        }
    }

    /// Computes the 1D distance between two parameter intervals.
    #[inline]
    pub const fn distance_param(self, other: Parameter) -> i64 {
        let above = other.min.wrapping_sub(self.max);
        let below = self.min.wrapping_sub(other.max);
        if above > 0 {
            above
        } else if below > 0 {
            below
        } else {
            0
        }
    }

    /// Exact midpoint `(min +_64 max) /_trunc 2` used by R-tree split comparators.
    #[inline]
    pub(crate) const fn midpoint(self) -> i64 {
        self.min.wrapping_add(self.max).wrapping_div(2)
    }
}

/// A 7-dimensional climate hyperrectangle: 6 [`Parameter`] intervals plus a quantized `offset`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParameterPoint {
    /// Temperature interval (index 0).
    pub temperature: Parameter,
    /// Humidity interval (index 1).
    pub humidity: Parameter,
    /// Continentalness interval (index 2).
    pub continentalness: Parameter,
    /// Erosion interval (index 3).
    pub erosion: Parameter,
    /// Depth interval (index 4).
    pub depth: Parameter,
    /// Weirdness interval (index 5).
    pub weirdness: Parameter,
    /// Quantized offset scalar (index 6 as `[offset, offset]`).
    pub offset: i64,
}

impl ParameterPoint {
    /// Constructs a [`ParameterPoint`], quantizing `offset` from `f32`.
    #[inline]
    pub fn new(
        temperature: Parameter,
        humidity: Parameter,
        continentalness: Parameter,
        erosion: Parameter,
        depth: Parameter,
        weirdness: Parameter,
        offset: f32,
    ) -> Self {
        Self {
            temperature,
            humidity,
            continentalness,
            erosion,
            depth,
            weirdness,
            offset: quantize_coord(offset),
        }
    }

    /// Constructs a [`ParameterPoint`] from an already-quantized `offset`.
    #[inline]
    pub const fn from_quantized(
        temperature: Parameter,
        humidity: Parameter,
        continentalness: Parameter,
        erosion: Parameter,
        depth: Parameter,
        weirdness: Parameter,
        offset: i64,
    ) -> Self {
        Self {
            temperature,
            humidity,
            continentalness,
            erosion,
            depth,
            weirdness,
            offset,
        }
    }

    /// Returns the 7-element bounding box `[T, H, C, E, D, W, [offset, offset]]`.
    #[inline]
    pub const fn parameter_space(&self) -> [Parameter; PARAMETER_COUNT] {
        [
            self.temperature,
            self.humidity,
            self.continentalness,
            self.erosion,
            self.depth,
            self.weirdness,
            Parameter {
                min: self.offset,
                max: self.offset,
            },
        ]
    }

    /// Computes the 7D squared distance (fitness) between this parameter point and `target`.
    #[inline]
    pub const fn fitness(&self, target: &TargetPoint) -> i64 {
        squared_distance(&self.parameter_space(), &target.to_parameter_array())
    }
}

/// Computes the exact 7D squared L2 distance between a 7-element bounding box and target vector
/// in left-to-right wrapping 64-bit integer arithmetic:
/// `sum_{i=0..=6} square_i64(space[i].distance(target[i]))`.
#[inline]
pub const fn squared_distance(
    space: &[Parameter; PARAMETER_COUNT],
    target: &[i64; PARAMETER_COUNT],
) -> i64 {
    let mut sum: i64 = 0;
    let mut i = 0;
    while i < PARAMETER_COUNT {
        let d = space[i].distance(target[i]);
        sum = sum.wrapping_add(square_i64(d));
        i += 1;
    }
    sum
}

/// Checks whether `v` lies in `[lo, hi]` using Java `Float.compare` semantics.
#[inline]
fn check_float_range(v: f32, lo: f32, hi: f32) -> Result<(), String> {
    if java_float_compare(v, lo) == Ordering::Less || java_float_compare(v, hi) == Ordering::Greater
    {
        Err(format!("Value {v} outside of range [{lo}:{hi}]"))
    } else {
        Ok(())
    }
}

/// Bounded `f32` codec matching Java `Codec.floatRange(min, max)`.
#[derive(Debug, Clone, Copy)]
struct BoundedFloatCodec {
    min: f32,
    max: f32,
}

impl Codec<f32> for BoundedFloatCodec {
    fn decode(&self, input: &Json) -> DataResult<f32> {
        match FloatCodec.decode(input) {
            DataResult::Success(v) => match check_float_range(v, self.min, self.max) {
                Ok(()) => DataResult::Success(v),
                Err(m) => DataResult::error_with_partial(m, v),
            },
            other => other,
        }
    }

    fn encode(&self, value: &f32) -> DataResult<Json> {
        match check_float_range(*value, self.min, self.max) {
            Ok(()) => FloatCodec.encode(value),
            Err(m) => DataResult::error(m),
        }
    }
}

/// JSON codec for [`Parameter`] (`ExtraCodecs.intervalCodec` over `f32` in `[-2.0, 2.0]`).
///
/// Decodes in exact order:
/// 1. Single number `v` in `[-2.0, 2.0]` -> `[quantize(v), quantize(v)]`.
/// 2. 2-element array `[min, max]`, each in `[-2.0, 2.0]`.
/// 3. Object `{"min": <f32>, "max": <f32>}`, each in `[-2.0, 2.0]`.
///
/// Rejects any interval where Java `Float.compare(min, max) > 0` (including `[+0.0, -0.0]`).
/// Encodes point intervals (`min == max`) as a single float and spans as `[min, max]`.
#[derive(Debug, Clone, Copy, Default)]
pub struct ParameterCodec;

impl ParameterCodec {
    const FLOAT_CODEC: BoundedFloatCodec = BoundedFloatCodec {
        min: PARAMETER_JSON_MIN,
        max: PARAMETER_JSON_MAX,
    };

    fn build_checked_span(min: f32, max: f32) -> DataResult<Parameter> {
        if java_float_compare(min, max) == Ordering::Greater {
            return DataResult::error(format!("Min must not be greater than max: [{min}, {max}]"));
        }
        DataResult::Success(Parameter {
            min: quantize_coord(min),
            max: quantize_coord(max),
        })
    }

    fn decode_single(input: &Json) -> DataResult<Parameter> {
        Self::FLOAT_CODEC.decode(input).and_then(|v| {
            let q = quantize_coord(v);
            DataResult::Success(Parameter { min: q, max: q })
        })
    }

    fn decode_array(input: &Json) -> DataResult<Parameter> {
        let Some(items) = input.as_array() else {
            return DataResult::error(format!("Not a json array: {}", input.type_name()));
        };
        if items.len() != 2 {
            return DataResult::error(format!(
                "Expected a 2-element array, got length {}",
                items.len()
            ));
        }
        let min = match Self::FLOAT_CODEC.decode(&items[0]) {
            DataResult::Success(v) => v,
            DataResult::Error { message, .. } => return DataResult::error(message),
            DataResult::Failure(m) => return DataResult::Failure(m),
        };
        let max = match Self::FLOAT_CODEC.decode(&items[1]) {
            DataResult::Success(v) => v,
            DataResult::Error { message, .. } => return DataResult::error(message),
            DataResult::Failure(m) => return DataResult::Failure(m),
        };
        Self::build_checked_span(min, max)
    }

    fn decode_object(input: &Json) -> DataResult<Parameter> {
        let mut r = RecordReader::new(input);
        let min = r.required("min", &Self::FLOAT_CODEC);
        let max = r.required("max", &Self::FLOAT_CODEC);
        r.finish(|| Some((min?, max?)))
            .and_then(|(lo, hi)| Self::build_checked_span(lo, hi))
    }
}

impl Codec<Parameter> for ParameterCodec {
    fn decode(&self, input: &Json) -> DataResult<Parameter> {
        let first_err = match Self::decode_single(input) {
            DataResult::Success(p) => return DataResult::Success(p),
            DataResult::Failure(m) => return DataResult::Failure(m),
            DataResult::Error { message, .. } => message,
        };
        let second_err = match Self::decode_array(input) {
            DataResult::Success(p) => return DataResult::Success(p),
            DataResult::Failure(m) => return DataResult::Failure(m),
            DataResult::Error { message, .. } => message,
        };
        match Self::decode_object(input) {
            DataResult::Success(p) => DataResult::Success(p),
            DataResult::Failure(m) => DataResult::Failure(m),
            DataResult::Error { message, .. } => DataResult::error(format!(
                "Failed to parse either. First: {first_err}; Second: Failed to parse either. First: {second_err}; Second: {message}"
            )),
        }
    }

    fn encode(&self, value: &Parameter) -> DataResult<Json> {
        if value.min > value.max {
            return DataResult::error(format!(
                "Min must not be greater than max: [{}, {}]",
                value.min, value.max
            ));
        }
        let lo = unquantize_coord(value.min);
        let hi = unquantize_coord(value.max);
        if value.min == value.max {
            Self::FLOAT_CODEC.encode(&lo)
        } else {
            let lo_json = match Self::FLOAT_CODEC.encode(&lo) {
                DataResult::Success(j) => j,
                other => return other,
            };
            let hi_json = match Self::FLOAT_CODEC.encode(&hi) {
                DataResult::Success(j) => j,
                other => return other,
            };
            DataResult::Success(Json::Array(vec![lo_json, hi_json]))
        }
    }
}

/// JSON codec for [`ParameterPoint`].
///
/// Decodes an object with 7 required fields: `temperature`, `humidity`, `continentalness`,
/// `erosion`, `depth`, `weirdness` (each a [`Parameter`] in `[-2.0, 2.0]`), and `offset`
/// (`f32` in `[0.0, 1.0]`, quantized via [`quantize_coord`]).
#[derive(Debug, Clone, Copy, Default)]
pub struct ParameterPointCodec;

impl ParameterPointCodec {
    const OFFSET_CODEC: BoundedFloatCodec = BoundedFloatCodec {
        min: OFFSET_JSON_MIN,
        max: OFFSET_JSON_MAX,
    };
}

impl Codec<ParameterPoint> for ParameterPointCodec {
    fn decode(&self, input: &Json) -> DataResult<ParameterPoint> {
        let mut r = RecordReader::new(input);
        let temperature = r.required("temperature", &ParameterCodec);
        let humidity = r.required("humidity", &ParameterCodec);
        let continentalness = r.required("continentalness", &ParameterCodec);
        let erosion = r.required("erosion", &ParameterCodec);
        let depth = r.required("depth", &ParameterCodec);
        let weirdness = r.required("weirdness", &ParameterCodec);
        let offset = r.required("offset", &Self::OFFSET_CODEC);
        r.finish(|| {
            Some(ParameterPoint::new(
                temperature?,
                humidity?,
                continentalness?,
                erosion?,
                depth?,
                weirdness?,
                offset?,
            ))
        })
    }

    fn encode(&self, value: &ParameterPoint) -> DataResult<Json> {
        let mut w = RecordWriter::new();
        w.field("temperature", &ParameterCodec, &value.temperature)
            .field("humidity", &ParameterCodec, &value.humidity)
            .field("continentalness", &ParameterCodec, &value.continentalness)
            .field("erosion", &ParameterCodec, &value.erosion)
            .field("depth", &ParameterCodec, &value.depth)
            .field("weirdness", &ParameterCodec, &value.weirdness)
            .field(
                "offset",
                &Self::OFFSET_CODEC,
                &unquantize_coord(value.offset),
            );
        w.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zenith_codec::json::{ParseMode, parse};

    #[test]
    fn quantization_matches_f32_truncation_quirks() {
        // Spec edge case #1: f32 multiplication truncates toward zero.
        assert_eq!(quantize_coord(-0.78_f32), -7799);
        assert_eq!(quantize_coord(0.26666668_f32), 2666);
        assert_eq!(quantize_coord(-0.26666668_f32), -2666);
        assert_eq!(quantize_coord(0.56666666_f32), 5666);
        assert_eq!(quantize_coord(-0.56666666_f32), -5666);
        assert_eq!(quantize_coord(0.7666667_f32), 7666);
        assert_eq!(quantize_coord(-0.7666667_f32), -7666);
        assert_eq!(quantize_coord(0.93333334_f32), 9333);
        assert_eq!(quantize_coord(-0.93333334_f32), -9333);
        assert_eq!(quantize_coord(f32::NAN), 0);
        assert_eq!(quantize_coord(f32::INFINITY), i64::MAX);
        assert_eq!(quantize_coord(f32::NEG_INFINITY), i64::MIN);
        assert_eq!(unquantize_coord(-7799), -0.7799_f32);
        assert_eq!(unquantize_coord(0), 0.0_f32);
        assert_eq!(unquantize_coord(10_000), 1.0_f32);
    }

    #[test]
    fn parameter_distance_and_fitness() {
        let p = Parameter::from_quantized(-100, 200).unwrap();
        assert_eq!(p.distance(-100), 0);
        assert_eq!(p.distance(0), 0);
        assert_eq!(p.distance(200), 0);
        assert_eq!(p.distance(250), 50);
        assert_eq!(p.distance(-140), 40);

        let pt = ParameterPoint::new(
            Parameter::point(0.0),
            Parameter::point(0.5),
            Parameter::point(0.0),
            Parameter::point(0.0),
            Parameter::point(0.0),
            Parameter::point(0.0),
            0.375,
        );
        let target = TargetPoint::from_floats(0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        // humidity distance = 5000, offset distance = 3750
        assert_eq!(pt.fitness(&target), 5000 * 5000 + 3750 * 3750);
    }

    #[test]
    fn parameter_codec_accepts_three_forms_and_rejects_inverted_zeros() {
        let j1 = parse("0.5", ParseMode::Data).unwrap();
        assert_eq!(
            ParameterCodec.decode(&j1).demand().unwrap(),
            Parameter {
                min: 5000,
                max: 5000
            }
        );

        let j2 = parse("[-0.78, 0.2]", ParseMode::Data).unwrap();
        assert_eq!(
            ParameterCodec.decode(&j2).demand().unwrap(),
            Parameter {
                min: -7799,
                max: 2000
            }
        );

        let j3 = parse(r#"{"min": -1.0, "max": 1.0}"#, ParseMode::Data).unwrap();
        assert_eq!(
            ParameterCodec.decode(&j3).demand().unwrap(),
            Parameter {
                min: -10000,
                max: 10000
            }
        );

        // Spec §1: [+0.0, -0.0] is rejected by Java Float.compare(min, max) > 0.
        let inv_zero = parse("[0.0, -0.0]", ParseMode::Data).unwrap();
        assert!(ParameterCodec.decode(&inv_zero).demand().is_err());

        // [-0.0, 0.0] is valid.
        let neg_pos_zero = parse("[-0.0, 0.0]", ParseMode::Data).unwrap();
        assert_eq!(
            ParameterCodec.decode(&neg_pos_zero).demand().unwrap(),
            Parameter { min: 0, max: 0 }
        );

        // Out-of-range endpoints rejected.
        let oor = parse("2.1", ParseMode::Data).unwrap();
        assert!(ParameterCodec.decode(&oor).demand().is_err());

        // Wrong array length rejected.
        let bad_len = parse("[0.0, 0.5, 1.0]", ParseMode::Data).unwrap();
        assert!(ParameterCodec.decode(&bad_len).demand().is_err());

        // Encode point as single float, span as 2-element array.
        let enc_pt = ParameterCodec
            .encode(&Parameter {
                min: 5000,
                max: 5000,
            })
            .demand()
            .unwrap();
        assert!(enc_pt.as_number().is_some());
        let enc_span = ParameterCodec
            .encode(&Parameter {
                min: -5000,
                max: 5000,
            })
            .demand()
            .unwrap();
        assert_eq!(enc_span.as_array().map(<[_]>::len), Some(2));
    }

    #[test]
    fn parameter_point_codec_round_trip_and_offset_bounds() {
        let json = parse(
            r#"{
                "temperature": [-0.45, -0.15],
                "humidity": 0.1,
                "continentalness": {"min": -0.11, "max": 0.03},
                "erosion": [-0.78, -0.375],
                "depth": 0.0,
                "weirdness": [-0.26666668, -0.05],
                "offset": 0.375
            }"#,
            ParseMode::Data,
        )
        .unwrap();
        let pt = ParameterPointCodec.decode(&json).demand().unwrap();
        assert_eq!(pt.temperature, Parameter::span(-0.45, -0.15).unwrap());
        assert_eq!(pt.humidity, Parameter::point(0.1));
        assert_eq!(pt.continentalness, Parameter::span(-0.11, 0.03).unwrap());
        assert_eq!(
            pt.erosion,
            Parameter {
                min: -7799,
                max: -3750
            }
        );
        assert_eq!(pt.depth, Parameter::point(0.0));
        assert_eq!(
            pt.weirdness,
            Parameter {
                min: -2666,
                max: -500
            }
        );
        assert_eq!(pt.offset, 3750);

        let encoded = ParameterPointCodec.encode(&pt).demand().unwrap();
        let round_trip = ParameterPointCodec.decode(&encoded).demand().unwrap();
        assert_eq!(round_trip, pt);

        // Negative offset rejected.
        let bad_offset = parse(
            r#"{
                "temperature": 0.0, "humidity": 0.0, "continentalness": 0.0,
                "erosion": 0.0, "depth": 0.0, "weirdness": 0.0, "offset": -0.1
            }"#,
            ParseMode::Data,
        )
        .unwrap();
        assert!(ParameterPointCodec.decode(&bad_offset).demand().is_err());
    }
}
