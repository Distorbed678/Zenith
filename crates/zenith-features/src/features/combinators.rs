//! Group A: Meta, selector, and combinator features (`features.md` §4.1–§4.7).

use std::sync::Arc;

use zenith_math::RandomSource;

use crate::features::FeatureContext;
use crate::placement::PlacedFeature;
use crate::providers::WeightedList;
use crate::region::GenerationRegion;
use crate::rng::{BlockPos, WorldgenRandom};

/// §4.2 `minecraft:random_boolean_selector`.
pub fn place_random_boolean_selector(
    ctx: &FeatureContext<'_>,
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    feature_true: &PlacedFeature,
    feature_false: &PlacedFeature,
) -> bool {
    let chosen = if rng.next_boolean() {
        feature_true
    } else {
        feature_false
    };
    ctx.place_sub_feature(chosen, region, rng, origin)
}

/// §4.3 `minecraft:random_selector`.
pub fn place_random_selector(
    ctx: &FeatureContext<'_>,
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    features: &[(Arc<PlacedFeature>, f32)],
    default: &PlacedFeature,
) -> bool {
    for (feat, chance) in features {
        if rng.next_float() < *chance {
            return ctx.place_sub_feature(feat, region, rng, origin);
        }
    }
    ctx.place_sub_feature(default, region, rng, origin)
}

/// §4.4 `minecraft:simple_random_selector`.
pub fn place_simple_random_selector(
    ctx: &FeatureContext<'_>,
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    features: &[Arc<PlacedFeature>],
) -> bool {
    if features.is_empty() {
        return false;
    }
    let idx = rng.next_int_bounded(features.len() as i32) as usize;
    ctx.place_sub_feature(&features[idx], region, rng, origin)
}

/// §4.5 `minecraft:weighted_random_selector`.
pub fn place_weighted_random_selector(
    ctx: &FeatureContext<'_>,
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    features: &WeightedList<Arc<PlacedFeature>>,
) -> bool {
    if let Some(feat) = features.get_random(rng) {
        ctx.place_sub_feature(feat, region, rng, origin)
    } else {
        false
    }
}

/// §4.6 `minecraft:overlay`.
pub fn place_overlay(
    ctx: &FeatureContext<'_>,
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    features: &[Arc<PlacedFeature>],
) -> bool {
    let mut placed_any = false;
    for feat in features {
        placed_any |= ctx.place_sub_feature(feat, region, rng, origin);
    }
    placed_any
}

/// §4.7 `minecraft:sequence`.
pub fn place_sequence(
    ctx: &FeatureContext<'_>,
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    features: &[Arc<PlacedFeature>],
) -> bool {
    for feat in features {
        if !ctx.place_sub_feature(feat, region, rng, origin) {
            return false;
        }
    }
    true
}
