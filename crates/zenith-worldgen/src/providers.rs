//! Vertical anchors and worldgen value/height providers (`VerticalAnchor`, `WorldGenerationContext`,
//! `IntProvider`, `FloatProvider`, `HeightProvider`) used by material rules and carvers
//! (`docs/specs/worldgen/surface-and-carvers.md` §1.2 & §3.1).

use zenith_codec::codec::{
    Codec, DataResult, FloatCodec, IntCodec, RecordReader, decode_dispatch, list, validated,
};
use zenith_codec::json::Json;
use zenith_math::RandomSource;
use zenith_math::helpers::rounding::{clamp_f32, clamp_i32, java_round_f32};
use zenith_registry::Identifier;

use crate::settings::{DIMENSION_MAX_Y, DIMENSION_MIN_Y};

/// Vertical generation bounds passed to `VerticalAnchor::resolve_y` and carvers
/// (`WorldGenerationContext`, `surface-and-carvers.md` "Inputs and state").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorldGenerationContext {
    /// Minimum generated block Y (`max(height_accessor.min_y, generator.min_y)`).
    pub min_gen_y: i32,
    /// Vertical generation depth in blocks (`min(height_accessor.height, generator.gen_depth)`).
    pub gen_depth: i32,
    /// Dimension sea level (`generator.sea_level`).
    pub sea_level: i32,
}

impl WorldGenerationContext {
    /// Constructs a `WorldGenerationContext`.
    #[inline]
    pub const fn new(min_gen_y: i32, gen_depth: i32, sea_level: i32) -> Self {
        Self {
            min_gen_y,
            gen_depth,
            sea_level,
        }
    }
}

/// Vertical anchor (`VerticalAnchor`, `surface-and-carvers.md` §1.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VerticalAnchor {
    /// `{ "absolute": y }` -> `y`.
    Absolute(i32),
    /// `{ "above_bottom": offset }` -> `ctx.min_gen_y + offset`.
    AboveBottom(i32),
    /// `{ "below_top": offset }` -> `ctx.gen_depth - 1 + ctx.min_gen_y - offset`.
    BelowTop(i32),
    /// `{ "relative_to_sea_level": offset }` -> `ctx.sea_level + offset` (new in 26.3).
    RelativeToSeaLevel(i32),
}

impl VerticalAnchor {
    /// Resolves this vertical anchor against `ctx`.
    #[inline]
    pub const fn resolve_y(self, ctx: WorldGenerationContext) -> i32 {
        match self {
            Self::Absolute(y) => y,
            Self::AboveBottom(offset) => ctx.min_gen_y.wrapping_add(offset),
            Self::BelowTop(offset) => ctx
                .gen_depth
                .wrapping_sub(1)
                .wrapping_add(ctx.min_gen_y)
                .wrapping_sub(offset),
            Self::RelativeToSeaLevel(offset) => ctx.sea_level.wrapping_add(offset),
        }
    }
}

fn anchor_offset_codec() -> impl Codec<i32> {
    validated(IntCodec, |v: &i32| {
        if (DIMENSION_MIN_Y..=DIMENSION_MAX_Y).contains(v) {
            Ok(())
        } else {
            Err(format!(
                "VerticalAnchor offset {v} outside of range [{DIMENSION_MIN_Y}, {DIMENSION_MAX_Y}]"
            ))
        }
    })
}

/// JSON codec for [`VerticalAnchor`].
#[derive(Debug, Clone, Copy, Default)]
pub struct VerticalAnchorCodec;

impl Codec<VerticalAnchor> for VerticalAnchorCodec {
    fn decode(&self, input: &Json) -> DataResult<VerticalAnchor> {
        let Some(obj) = input.as_object() else {
            return DataResult::error(format!(
                "Expected VerticalAnchor object, got {}",
                input.type_name()
            ));
        };
        let off = anchor_offset_codec();
        if let Some(v) = obj.get("absolute") {
            return off.decode(v).map(VerticalAnchor::Absolute);
        }
        if let Some(v) = obj.get("above_bottom") {
            return off.decode(v).map(VerticalAnchor::AboveBottom);
        }
        if let Some(v) = obj.get("below_top") {
            return off.decode(v).map(VerticalAnchor::BelowTop);
        }
        if let Some(v) = obj.get("relative_to_sea_level") {
            return off.decode(v).map(VerticalAnchor::RelativeToSeaLevel);
        }
        DataResult::error(
            "VerticalAnchor must have one of: absolute, above_bottom, below_top, relative_to_sea_level",
        )
    }

    fn encode(&self, _value: &VerticalAnchor) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

/// Integer provider (`IntProvider`, `surface-and-carvers.md` §3.1).
#[derive(Debug, Clone, PartialEq)]
pub enum IntProvider {
    /// Constant integer value.
    Constant(i32),
    /// Uniform inclusive `[min_inclusive, max_inclusive]`.
    Uniform {
        /// Inclusive minimum.
        min_inclusive: i32,
        /// Inclusive maximum.
        max_inclusive: i32,
    },
    /// `min + nextInt(nextInt(max - min + 1) + 1)`.
    BiasedToBottom {
        /// Inclusive minimum.
        min_inclusive: i32,
        /// Inclusive maximum.
        max_inclusive: i32,
    },
    /// `min + nextInt(nextInt(nextInt(max - min + 1) + 1) + 1)`.
    VeryBiasedToBottom {
        /// Inclusive minimum.
        min_inclusive: i32,
        /// Inclusive maximum.
        max_inclusive: i32,
    },
    /// Clamps another `IntProvider` into `[min_inclusive, max_inclusive]`.
    Clamped {
        /// Inner provider.
        source: Box<IntProvider>,
        /// Inclusive minimum.
        min_inclusive: i32,
        /// Inclusive maximum.
        max_inclusive: i32,
    },
    /// Clamped normal distribution rounded to integer.
    ClampedNormal {
        /// Mean.
        mean: f32,
        /// Standard deviation.
        deviation: f32,
        /// Inclusive minimum.
        min_inclusive: i32,
        /// Inclusive maximum.
        max_inclusive: i32,
    },
    /// Weighted list of child `IntProvider`s.
    WeightedList(Vec<(IntProvider, i32)>),
}

impl IntProvider {
    /// Samples one integer using `rng`, preserving exact vanilla RNG draw order.
    pub fn sample<R: RandomSource>(&self, rng: &mut R) -> i32 {
        match self {
            Self::Constant(v) => *v,
            Self::Uniform {
                min_inclusive,
                max_inclusive,
            } => {
                let bound = max_inclusive.wrapping_sub(*min_inclusive).wrapping_add(1);
                rng.next_int_bounded(bound).wrapping_add(*min_inclusive)
            }
            Self::BiasedToBottom {
                min_inclusive,
                max_inclusive,
            } => {
                let range = max_inclusive.wrapping_sub(*min_inclusive).wrapping_add(1);
                let a = rng.next_int_bounded(range);
                let b = rng.next_int_bounded(a.wrapping_add(1));
                min_inclusive.wrapping_add(b)
            }
            Self::VeryBiasedToBottom {
                min_inclusive,
                max_inclusive,
            } => {
                let range = max_inclusive.wrapping_sub(*min_inclusive).wrapping_add(1);
                let a = rng.next_int_bounded(range);
                let b = rng.next_int_bounded(a.wrapping_add(1));
                let c = rng.next_int_bounded(b.wrapping_add(1));
                min_inclusive.wrapping_add(c)
            }
            Self::Clamped {
                source,
                min_inclusive,
                max_inclusive,
            } => clamp_i32(source.sample(rng), *min_inclusive, *max_inclusive),
            Self::ClampedNormal {
                mean,
                deviation,
                min_inclusive,
                max_inclusive,
            } => {
                let g = (rng.next_gaussian() as f32) * *deviation + *mean;
                let clamped = clamp_f32(g, *min_inclusive as f32, *max_inclusive as f32);
                java_round_f32(clamped)
            }
            Self::WeightedList(entries) => {
                let total: i32 = entries.iter().map(|(_, w)| *w).sum();
                if total <= 0 || entries.is_empty() {
                    return 0;
                }
                let mut pick = rng.next_int_bounded(total);
                for (p, w) in entries {
                    pick -= *w;
                    if pick < 0 {
                        return p.sample(rng);
                    }
                }
                entries[0].0.sample(rng)
            }
        }
    }
}

struct WeightedIntEntryCodec;
impl Codec<(IntProvider, i32)> for WeightedIntEntryCodec {
    fn decode(&self, input: &Json) -> DataResult<(IntProvider, i32)> {
        let mut r = RecordReader::new(input);
        let data = r.required("data", &IntProviderCodec);
        let weight = r.required("weight", &IntCodec);
        r.finish(|| Some((data?, weight?)))
    }
    fn encode(&self, _value: &(IntProvider, i32)) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

/// JSON codec for [`IntProvider`].
#[derive(Debug, Clone, Copy, Default)]
pub struct IntProviderCodec;

impl Codec<IntProvider> for IntProviderCodec {
    fn decode(&self, input: &Json) -> DataResult<IntProvider> {
        if matches!(input, Json::Number(_)) {
            return IntCodec.decode(input).map(IntProvider::Constant);
        }
        decode_dispatch(input, "type", |ty, obj| {
            let id = Identifier::parse(ty).ok()?;
            if id.namespace() != "minecraft" {
                return None;
            }
            // Pre-1.20.5 sometimes nested inside "value"; 26.3 places fields directly on obj.
            let target = obj.as_object().and_then(|m| m.get("value")).unwrap_or(obj);
            match id.path() {
                "constant" => Some({
                    let mut r = RecordReader::new(obj);
                    let value = r.required("value", &IntCodec);
                    r.finish(|| Some(IntProvider::Constant(value?)))
                }),
                "uniform" => Some({
                    let mut r = RecordReader::new(target);
                    let min_inclusive = r.required("min_inclusive", &IntCodec);
                    let max_inclusive = r.required("max_inclusive", &IntCodec);
                    r.finish(|| {
                        Some(IntProvider::Uniform {
                            min_inclusive: min_inclusive?,
                            max_inclusive: max_inclusive?,
                        })
                    })
                }),
                "biased_to_bottom" => Some({
                    let mut r = RecordReader::new(target);
                    let min_inclusive = r.required("min_inclusive", &IntCodec);
                    let max_inclusive = r.required("max_inclusive", &IntCodec);
                    r.finish(|| {
                        Some(IntProvider::BiasedToBottom {
                            min_inclusive: min_inclusive?,
                            max_inclusive: max_inclusive?,
                        })
                    })
                }),
                "very_biased_to_bottom" => Some({
                    let mut r = RecordReader::new(target);
                    let min_inclusive = r.required("min_inclusive", &IntCodec);
                    let max_inclusive = r.required("max_inclusive", &IntCodec);
                    r.finish(|| {
                        Some(IntProvider::VeryBiasedToBottom {
                            min_inclusive: min_inclusive?,
                            max_inclusive: max_inclusive?,
                        })
                    })
                }),
                "clamped" => Some({
                    let mut r = RecordReader::new(target);
                    let source = r.required("source", &IntProviderCodec);
                    let min_inclusive = r.required("min_inclusive", &IntCodec);
                    let max_inclusive = r.required("max_inclusive", &IntCodec);
                    r.finish(|| {
                        Some(IntProvider::Clamped {
                            source: Box::new(source?),
                            min_inclusive: min_inclusive?,
                            max_inclusive: max_inclusive?,
                        })
                    })
                }),
                "clamped_normal" => Some({
                    let mut r = RecordReader::new(target);
                    let mean = r.required("mean", &FloatCodec);
                    let deviation = r.required("deviation", &FloatCodec);
                    let min_inclusive = r.required("min_inclusive", &IntCodec);
                    let max_inclusive = r.required("max_inclusive", &IntCodec);
                    r.finish(|| {
                        Some(IntProvider::ClampedNormal {
                            mean: mean?,
                            deviation: deviation?,
                            min_inclusive: min_inclusive?,
                            max_inclusive: max_inclusive?,
                        })
                    })
                }),
                "weighted_list" => Some({
                    let mut r = RecordReader::new(obj);
                    let dist = r.required("distribution", &list(WeightedIntEntryCodec));
                    r.finish(|| Some(IntProvider::WeightedList(dist?)))
                }),
                _ => None,
            }
        })
    }

    fn encode(&self, _value: &IntProvider) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

/// Float provider (`FloatProvider`, `surface-and-carvers.md` §3.1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FloatProvider {
    /// Constant `f32`.
    Constant(f32),
    /// Uniform `[min_inclusive, max_exclusive)`: `rng.next_float() * (max - min) + min`.
    Uniform {
        /// Inclusive minimum.
        min_inclusive: f32,
        /// Exclusive maximum.
        max_exclusive: f32,
    },
    /// Trapezoid distribution (`min + nextFloat * pEnd + nextFloat * pStart`).
    Trapezoid {
        /// Minimum value.
        min: f32,
        /// Maximum value.
        max: f32,
        /// Flat plateau width (`<= max - min`).
        plateau: f32,
    },
    /// Clamped normal distribution.
    ClampedNormal {
        /// Mean.
        mean: f32,
        /// Standard deviation.
        deviation: f32,
        /// Inclusive minimum.
        min: f32,
        /// Inclusive maximum.
        max: f32,
    },
}

impl FloatProvider {
    /// Samples one `f32` using `rng`, preserving exact vanilla RNG draw order and `f32` ops.
    #[inline]
    pub fn sample<R: RandomSource>(&self, rng: &mut R) -> f32 {
        match *self {
            Self::Constant(v) => v,
            Self::Uniform {
                min_inclusive,
                max_exclusive,
            } => rng.next_float() * (max_exclusive - min_inclusive) + min_inclusive,
            Self::Trapezoid { min, max, plateau } => {
                let range = max - min;
                let p_start = (range - plateau) / 2.0_f32;
                let p_end = range - p_start;
                let f1 = rng.next_float();
                let f2 = rng.next_float();
                (min + f1 * p_end) + f2 * p_start
            }
            Self::ClampedNormal {
                mean,
                deviation,
                min,
                max,
            } => {
                let g = (rng.next_gaussian() as f32) * deviation + mean;
                clamp_f32(g, min, max)
            }
        }
    }
}

/// JSON codec for [`FloatProvider`].
#[derive(Debug, Clone, Copy, Default)]
pub struct FloatProviderCodec;

impl Codec<FloatProvider> for FloatProviderCodec {
    fn decode(&self, input: &Json) -> DataResult<FloatProvider> {
        if matches!(input, Json::Number(_)) {
            return FloatCodec.decode(input).map(FloatProvider::Constant);
        }
        decode_dispatch(input, "type", |ty, obj| {
            let id = Identifier::parse(ty).ok()?;
            if id.namespace() != "minecraft" {
                return None;
            }
            let target = obj.as_object().and_then(|m| m.get("value")).unwrap_or(obj);
            match id.path() {
                "constant" => Some({
                    let mut r = RecordReader::new(obj);
                    let value = r.required("value", &FloatCodec);
                    r.finish(|| Some(FloatProvider::Constant(value?)))
                }),
                "uniform" => Some({
                    let mut r = RecordReader::new(target);
                    let min_inclusive = r.required("min_inclusive", &FloatCodec);
                    let max_exclusive = r.required("max_exclusive", &FloatCodec);
                    r.finish(|| {
                        Some(FloatProvider::Uniform {
                            min_inclusive: min_inclusive?,
                            max_exclusive: max_exclusive?,
                        })
                    })
                }),
                "trapezoid" => Some({
                    let mut r = RecordReader::new(target);
                    let min = r.required("min", &FloatCodec);
                    let max = r.required("max", &FloatCodec);
                    let plateau = r.required("plateau", &FloatCodec);
                    r.finish(|| {
                        Some(FloatProvider::Trapezoid {
                            min: min?,
                            max: max?,
                            plateau: plateau?,
                        })
                    })
                }),
                "clamped_normal" => Some({
                    let mut r = RecordReader::new(target);
                    let mean = r.required("mean", &FloatCodec);
                    let deviation = r.required("deviation", &FloatCodec);
                    let min = r.required("min", &FloatCodec);
                    let max = r.required("max", &FloatCodec);
                    r.finish(|| {
                        Some(FloatProvider::ClampedNormal {
                            mean: mean?,
                            deviation: deviation?,
                            min: min?,
                            max: max?,
                        })
                    })
                }),
                _ => None,
            }
        })
    }

    fn encode(&self, _value: &FloatProvider) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

/// Height provider (`HeightProvider`, `surface-and-carvers.md` §3.1).
#[derive(Debug, Clone, PartialEq)]
pub enum HeightProvider {
    /// Constant `VerticalAnchor`.
    Constant(VerticalAnchor),
    /// Uniform inclusive `[min_inclusive, max_inclusive]`.
    Uniform {
        /// Inclusive lower anchor.
        min_inclusive: VerticalAnchor,
        /// Inclusive upper anchor.
        max_inclusive: VerticalAnchor,
    },
    /// Biased toward bottom.
    BiasedToBottom {
        /// Inclusive lower anchor.
        min_inclusive: VerticalAnchor,
        /// Inclusive upper anchor.
        max_inclusive: VerticalAnchor,
        /// Inner divisor (`>= 1`, default `1`).
        inner: i32,
    },
    /// Very biased toward bottom.
    VeryBiasedToBottom {
        /// Inclusive lower anchor.
        min_inclusive: VerticalAnchor,
        /// Inclusive upper anchor.
        max_inclusive: VerticalAnchor,
        /// Inner divisor (`>= 1`, default `1`).
        inner: i32,
    },
    /// Trapezoid height distribution.
    Trapezoid {
        /// Inclusive lower anchor.
        min_inclusive: VerticalAnchor,
        /// Inclusive upper anchor.
        max_inclusive: VerticalAnchor,
        /// Plateau width (default `0`).
        plateau: i32,
    },
    /// Weighted list of child `HeightProvider`s.
    WeightedList(Vec<(HeightProvider, i32)>),
}

impl HeightProvider {
    /// Samples a block Y coordinate using `rng` and `ctx`.
    pub fn sample<R: RandomSource>(&self, rng: &mut R, ctx: WorldGenerationContext) -> i32 {
        match self {
            Self::Constant(anchor) => anchor.resolve_y(ctx),
            Self::Uniform {
                min_inclusive,
                max_inclusive,
            } => {
                let min = min_inclusive.resolve_y(ctx);
                let max = max_inclusive.resolve_y(ctx);
                if min > max {
                    return min;
                }
                rng.next_int_bounded(max.wrapping_sub(min).wrapping_add(1))
                    .wrapping_add(min)
            }
            Self::BiasedToBottom {
                min_inclusive,
                max_inclusive,
                inner,
            } => {
                let min = min_inclusive.resolve_y(ctx);
                let max = max_inclusive.resolve_y(ctx);
                let span = max.wrapping_sub(min).wrapping_sub(*inner).wrapping_add(1);
                if span <= 0 {
                    return min;
                }
                let upper = rng.next_int_bounded(span).wrapping_add(*inner);
                rng.next_int_bounded(upper).wrapping_add(min)
            }
            Self::VeryBiasedToBottom {
                min_inclusive,
                max_inclusive,
                inner,
            } => {
                let min = min_inclusive.resolve_y(ctx);
                let max = max_inclusive.resolve_y(ctx);
                let span = max.wrapping_sub(min).wrapping_sub(*inner).wrapping_add(1);
                if span <= 0 {
                    return min;
                }
                let a = rng.next_int_bounded(span).wrapping_add(*inner);
                let b = rng
                    .next_int_bounded(a.wrapping_sub(*inner).wrapping_add(1))
                    .wrapping_add(*inner);
                rng.next_int_bounded(b).wrapping_add(min)
            }
            Self::Trapezoid {
                min_inclusive,
                max_inclusive,
                plateau,
            } => {
                let min = min_inclusive.resolve_y(ctx);
                let max = max_inclusive.resolve_y(ctx);
                if min > max {
                    return min;
                }
                let range = max.wrapping_sub(min);
                if range <= *plateau {
                    return rng
                        .next_int_bounded(range.wrapping_add(1))
                        .wrapping_add(min);
                }
                let p_start = (range - *plateau) / 2;
                let p_end = range - p_start;
                let a = rng.next_int_bounded(p_end.wrapping_add(1));
                let b = rng.next_int_bounded(p_start.wrapping_add(1));
                min.wrapping_add(a).wrapping_add(b)
            }
            Self::WeightedList(entries) => {
                let total: i32 = entries.iter().map(|(_, w)| *w).sum();
                if total <= 0 || entries.is_empty() {
                    return ctx.min_gen_y;
                }
                let mut pick = rng.next_int_bounded(total);
                for (p, w) in entries {
                    pick -= *w;
                    if pick < 0 {
                        return p.sample(rng, ctx);
                    }
                }
                entries[0].0.sample(rng, ctx)
            }
        }
    }
}

struct WeightedHeightEntryCodec;
impl Codec<(HeightProvider, i32)> for WeightedHeightEntryCodec {
    fn decode(&self, input: &Json) -> DataResult<(HeightProvider, i32)> {
        let mut r = RecordReader::new(input);
        let data = r.required("data", &HeightProviderCodec);
        let weight = r.required("weight", &IntCodec);
        r.finish(|| Some((data?, weight?)))
    }
    fn encode(&self, _value: &(HeightProvider, i32)) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

/// JSON codec for [`HeightProvider`].
#[derive(Debug, Clone, Copy, Default)]
pub struct HeightProviderCodec;

impl Codec<HeightProvider> for HeightProviderCodec {
    fn decode(&self, input: &Json) -> DataResult<HeightProvider> {
        // Inline VerticalAnchor shorthand (no "type" field).
        if let Some(obj) = input.as_object()
            && !obj.contains_key("type")
        {
            return VerticalAnchorCodec
                .decode(input)
                .map(HeightProvider::Constant);
        }
        decode_dispatch(input, "type", |ty, obj| {
            let id = Identifier::parse(ty).ok()?;
            if id.namespace() != "minecraft" {
                return None;
            }
            match id.path() {
                "constant" => Some({
                    let mut r = RecordReader::new(obj);
                    let value = r.required("value", &VerticalAnchorCodec);
                    r.finish(|| Some(HeightProvider::Constant(value?)))
                }),
                "uniform" => Some({
                    let mut r = RecordReader::new(obj);
                    let min_inclusive = r.required("min_inclusive", &VerticalAnchorCodec);
                    let max_inclusive = r.required("max_inclusive", &VerticalAnchorCodec);
                    r.finish(|| {
                        Some(HeightProvider::Uniform {
                            min_inclusive: min_inclusive?,
                            max_inclusive: max_inclusive?,
                        })
                    })
                }),
                "biased_to_bottom" => Some({
                    let mut r = RecordReader::new(obj);
                    let min_inclusive = r.required("min_inclusive", &VerticalAnchorCodec);
                    let max_inclusive = r.required("max_inclusive", &VerticalAnchorCodec);
                    let inner = r.optional("inner", &IntCodec, 1);
                    r.finish(|| {
                        Some(HeightProvider::BiasedToBottom {
                            min_inclusive: min_inclusive?,
                            max_inclusive: max_inclusive?,
                            inner,
                        })
                    })
                }),
                "very_biased_to_bottom" => Some({
                    let mut r = RecordReader::new(obj);
                    let min_inclusive = r.required("min_inclusive", &VerticalAnchorCodec);
                    let max_inclusive = r.required("max_inclusive", &VerticalAnchorCodec);
                    let inner = r.optional("inner", &IntCodec, 1);
                    r.finish(|| {
                        Some(HeightProvider::VeryBiasedToBottom {
                            min_inclusive: min_inclusive?,
                            max_inclusive: max_inclusive?,
                            inner,
                        })
                    })
                }),
                "trapezoid" => Some({
                    let mut r = RecordReader::new(obj);
                    let min_inclusive = r.required("min_inclusive", &VerticalAnchorCodec);
                    let max_inclusive = r.required("max_inclusive", &VerticalAnchorCodec);
                    let plateau = r.optional("plateau", &IntCodec, 0);
                    r.finish(|| {
                        Some(HeightProvider::Trapezoid {
                            min_inclusive: min_inclusive?,
                            max_inclusive: max_inclusive?,
                            plateau,
                        })
                    })
                }),
                "weighted_list" => Some({
                    let mut r = RecordReader::new(obj);
                    let dist = r.required("distribution", &list(WeightedHeightEntryCodec));
                    r.finish(|| Some(HeightProvider::WeightedList(dist?)))
                }),
                _ => None,
            }
        })
    }

    fn encode(&self, _value: &HeightProvider) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}
