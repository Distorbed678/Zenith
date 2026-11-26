//! Shared worldgen providers, predicates, rule tests, weighted lists, and vertical column scanner
//! (`docs/specs/worldgen/features.md` §2.3 & §3).

use std::sync::Arc;

use zenith_blocks::{Direction, FluidKind};
use zenith_math::RandomSource;
use zenith_math::helpers::interp::clamped_map_f64;
use zenith_math::helpers::rounding::clamp_f32;
use zenith_math::noise::{NormalNoise, NormalNoiseParameters};
use zenith_registry::Identifier;
use zenith_worldgen::{BiomeBitSet, VerticalAnchor, WorldGenerationContext};

use crate::block_ops::BlockBitSet;
use crate::region::GenerationRegion;
use crate::rng::{
    BlockPos, WorldgenRandom, direction_axis_name, direction_get_random, direction_is_horizontal,
    mth_next_int, mth_normal, random_between_f32, random_between_inclusive,
};

// ---------------------------------------------------------------------------
// WeightedList<E> (§3.3)
// ---------------------------------------------------------------------------

/// Weighted random selection list (`net.minecraft.util.random.WeightedList`).
#[derive(Debug, Clone, PartialEq)]
pub struct WeightedList<T> {
    entries: Vec<(T, i32)>,
    total_weight: i32,
}

impl<T> WeightedList<T> {
    /// Creates a new `WeightedList` from `(data, weight)` pairs.
    pub fn new(entries: Vec<(T, i32)>) -> Self {
        let total_weight = entries.iter().map(|(_, w)| *w).sum();
        Self {
            entries,
            total_weight,
        }
    }

    /// Empty weighted list.
    pub const fn empty() -> Self {
        Self {
            entries: Vec::new(),
            total_weight: 0,
        }
    }

    /// Returns `true` if `total_weight == 0`.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.total_weight <= 0
    }

    /// Underlying `(data, weight)` entries.
    #[inline]
    pub fn entries(&self) -> &[(T, i32)] {
        &self.entries
    }

    /// Samples one entry using `rng` (prefix-sum interval matching both `Flat` and `Compact`).
    #[inline]
    pub fn get_random(&self, rng: &mut impl RandomSource) -> Option<&T> {
        if self.total_weight <= 0 {
            return None;
        }
        let mut selection = rng.next_int_bounded(self.total_weight);
        for (data, weight) in &self.entries {
            selection -= *weight;
            if selection < 0 {
                return Some(data);
            }
        }
        None
    }

    /// Samples one entry, panicking if empty.
    #[inline]
    pub fn get_random_or_throw(&self, rng: &mut impl RandomSource) -> &T {
        self.get_random(rng)
            .expect("WeightedList::get_random_or_throw called on empty list")
    }
}

// ---------------------------------------------------------------------------
// IntProvider (all 8 types, §3.3)
// ---------------------------------------------------------------------------

/// All 8 `minecraft:int_provider_type` implementations (`features.md` §3.3).
#[derive(Debug, Clone, PartialEq)]
pub enum IntProvider {
    /// `minecraft:constant` (also bare JSON integer).
    Constant(i32),
    /// `minecraft:uniform`.
    Uniform {
        /// Inclusive minimum.
        min_inclusive: i32,
        /// Inclusive maximum.
        max_inclusive: i32,
    },
    /// `minecraft:biased_to_bottom`.
    BiasedToBottom {
        /// Inclusive minimum.
        min_inclusive: i32,
        /// Inclusive maximum.
        max_inclusive: i32,
    },
    /// `minecraft:very_biased_to_bottom`.
    VeryBiasedToBottom {
        /// Inclusive minimum.
        min_inclusive: i32,
        /// Inclusive maximum.
        max_inclusive: i32,
    },
    /// `minecraft:clamped`.
    Clamped {
        /// Wrapped provider.
        source: Box<IntProvider>,
        /// Inclusive minimum.
        min_inclusive: i32,
        /// Inclusive maximum.
        max_inclusive: i32,
    },
    /// `minecraft:clamped_normal`.
    ClampedNormal {
        /// Mean.
        mean: f32,
        /// Deviation.
        deviation: f32,
        /// Inclusive minimum.
        min_inclusive: i32,
        /// Inclusive maximum.
        max_inclusive: i32,
    },
    /// `minecraft:weighted_list`.
    WeightedList(WeightedList<IntProvider>),
    /// `minecraft:trapezoid`.
    Trapezoid {
        /// Inclusive minimum.
        min: i32,
        /// Inclusive maximum.
        max: i32,
        /// Flat plateau width (`<= max - min`).
        plateau: i32,
    },
}

impl IntProvider {
    /// Minimum inclusive value of this provider.
    pub fn min_inclusive(&self) -> i32 {
        match self {
            Self::Constant(v) => *v,
            Self::Uniform { min_inclusive, .. }
            | Self::BiasedToBottom { min_inclusive, .. }
            | Self::VeryBiasedToBottom { min_inclusive, .. }
            | Self::ClampedNormal { min_inclusive, .. } => *min_inclusive,
            Self::Clamped {
                source,
                min_inclusive,
                ..
            } => (*min_inclusive).max(source.min_inclusive()),
            Self::WeightedList(w) => w
                .entries()
                .iter()
                .map(|(p, _)| p.min_inclusive())
                .min()
                .unwrap_or(0),
            Self::Trapezoid { min, .. } => *min,
        }
    }

    /// Maximum inclusive value of this provider.
    pub fn max_inclusive(&self) -> i32 {
        match self {
            Self::Constant(v) => *v,
            Self::Uniform { max_inclusive, .. }
            | Self::BiasedToBottom { max_inclusive, .. }
            | Self::VeryBiasedToBottom { max_inclusive, .. }
            | Self::ClampedNormal { max_inclusive, .. } => *max_inclusive,
            Self::Clamped {
                source,
                max_inclusive,
                ..
            } => (*max_inclusive).min(source.max_inclusive()),
            Self::WeightedList(w) => w
                .entries()
                .iter()
                .map(|(p, _)| p.max_inclusive())
                .max()
                .unwrap_or(0),
            Self::Trapezoid { max, .. } => *max,
        }
    }

    /// Samples one integer in exact vanilla RNG draw order (`features.md` §3.3).
    pub fn sample(&self, rng: &mut impl RandomSource) -> i32 {
        match self {
            Self::Constant(v) => *v,
            Self::Uniform {
                min_inclusive,
                max_inclusive,
            } => random_between_inclusive(rng, *min_inclusive, *max_inclusive),
            Self::BiasedToBottom {
                min_inclusive,
                max_inclusive,
            } => {
                let inner = rng.next_int_bounded(max_inclusive - min_inclusive + 1);
                min_inclusive + rng.next_int_bounded(inner + 1)
            }
            Self::VeryBiasedToBottom {
                min_inclusive,
                max_inclusive,
            } => {
                let a = rng.next_int_bounded(max_inclusive - min_inclusive + 1);
                let b = rng.next_int_bounded(a + 1);
                min_inclusive + rng.next_int_bounded(b + 1)
            }
            Self::Clamped {
                source,
                min_inclusive,
                max_inclusive,
            } => source.sample(rng).clamp(*min_inclusive, *max_inclusive),
            Self::ClampedNormal {
                mean,
                deviation,
                min_inclusive,
                max_inclusive,
            } => {
                let v = clamp_f32(
                    mth_normal(rng, *mean, *deviation),
                    *min_inclusive as f32,
                    *max_inclusive as f32,
                );
                v as i32
            }
            Self::WeightedList(w) => w.get_random_or_throw(rng).sample(rng),
            Self::Trapezoid { min, max, plateau } => {
                if *plateau == 0 && *max == -*min {
                    rng.next_int_bounded(max + 1) - rng.next_int_bounded(max + 1)
                } else if *plateau == max - min {
                    random_between_inclusive(rng, *min, *max)
                } else {
                    let r = max - min;
                    let p_start = (r - plateau) / 2;
                    let p_end = r - p_start;
                    min + random_between_inclusive(rng, 0, p_end)
                        + random_between_inclusive(rng, 0, p_start)
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// FloatProvider (all 4 types, §3.3)
// ---------------------------------------------------------------------------

/// All 4 `minecraft:float_provider_type` implementations (`features.md` §3.3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FloatProvider {
    /// `minecraft:constant` (also bare JSON float).
    Constant(f32),
    /// `minecraft:uniform`.
    Uniform {
        /// Inclusive minimum.
        min_inclusive: f32,
        /// Exclusive maximum.
        max_exclusive: f32,
    },
    /// `minecraft:clamped_normal`.
    ClampedNormal {
        /// Mean.
        mean: f32,
        /// Standard deviation.
        deviation: f32,
        /// Minimum clamp.
        min: f32,
        /// Maximum clamp.
        max: f32,
    },
    /// `minecraft:trapezoid`.
    Trapezoid {
        /// Minimum value.
        min: f32,
        /// Maximum value.
        max: f32,
        /// Flat plateau width.
        plateau: f32,
    },
}

impl FloatProvider {
    /// Samples one `f32` in exact vanilla RNG draw order.
    #[inline]
    pub fn sample(&self, rng: &mut impl RandomSource) -> f32 {
        match *self {
            Self::Constant(v) => v,
            Self::Uniform {
                min_inclusive,
                max_exclusive,
            } => random_between_f32(rng, min_inclusive, max_exclusive),
            Self::ClampedNormal {
                mean,
                deviation,
                min,
                max,
            } => clamp_f32(mth_normal(rng, mean, deviation), min, max),
            Self::Trapezoid { min, max, plateau } => {
                let r = max - min;
                let p_start = (r - plateau) / 2.0_f32;
                let p_end = r - p_start;
                (min + rng.next_float() * p_end) + rng.next_float() * p_start
            }
        }
    }
}

// ---------------------------------------------------------------------------
// HeightProvider (all 6 types, §2.3)
// ---------------------------------------------------------------------------

/// All 6 `minecraft:height_provider_type` implementations (`features.md` §2.3).
#[derive(Debug, Clone, PartialEq)]
pub enum HeightProvider {
    /// `minecraft:constant` (or bare `VerticalAnchor`).
    Constant(VerticalAnchor),
    /// `minecraft:uniform`.
    Uniform {
        /// Lower vertical anchor.
        min_inclusive: VerticalAnchor,
        /// Upper vertical anchor.
        max_inclusive: VerticalAnchor,
    },
    /// `minecraft:biased_to_bottom`.
    BiasedToBottom {
        /// Lower vertical anchor.
        min_inclusive: VerticalAnchor,
        /// Upper vertical anchor.
        max_inclusive: VerticalAnchor,
        /// Inner bound (`>= 1`, default `1`).
        inner: i32,
    },
    /// `minecraft:very_biased_to_bottom`.
    VeryBiasedToBottom {
        /// Lower vertical anchor.
        min_inclusive: VerticalAnchor,
        /// Upper vertical anchor.
        max_inclusive: VerticalAnchor,
        /// Inner bound (`>= 1`, default `1`).
        inner: i32,
    },
    /// `minecraft:trapezoid`.
    Trapezoid {
        /// Lower vertical anchor.
        min_inclusive: VerticalAnchor,
        /// Upper vertical anchor.
        max_inclusive: VerticalAnchor,
        /// Plateau width (default `0`).
        plateau: i32,
    },
    /// `minecraft:weighted_list`.
    WeightedList(WeightedList<HeightProvider>),
}

impl HeightProvider {
    /// Samples one Y coordinate in exact vanilla RNG draw order (`features.md` §2.3).
    pub fn sample(&self, rng: &mut impl RandomSource, ctx: WorldGenerationContext) -> i32 {
        match self {
            Self::Constant(v) => v.resolve_y(ctx),
            Self::Uniform {
                min_inclusive,
                max_inclusive,
            } => {
                let min = min_inclusive.resolve_y(ctx);
                let max = max_inclusive.resolve_y(ctx);
                if min > max {
                    min
                } else {
                    random_between_inclusive(rng, min, max)
                }
            }
            Self::BiasedToBottom {
                min_inclusive,
                max_inclusive,
                inner,
            } => {
                let min = min_inclusive.resolve_y(ctx);
                let max = max_inclusive.resolve_y(ctx);
                let span = max - min - inner + 1;
                if span <= 0 {
                    min
                } else {
                    let limit = rng.next_int_bounded(span);
                    rng.next_int_bounded(limit + inner) + min
                }
            }
            Self::VeryBiasedToBottom {
                min_inclusive,
                max_inclusive,
                inner,
            } => {
                let min = min_inclusive.resolve_y(ctx);
                let max = max_inclusive.resolve_y(ctx);
                if max - min - inner < 0 {
                    min
                } else {
                    let u = mth_next_int(rng, min + inner, max);
                    let b = mth_next_int(rng, min, u - 1);
                    mth_next_int(rng, min, b - 1 + inner)
                }
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
                let range = max - min;
                if *plateau >= range {
                    random_between_inclusive(rng, min, max)
                } else {
                    let p_start = (range - plateau) / 2;
                    let p_end = range - p_start;
                    min + random_between_inclusive(rng, 0, p_end)
                        + random_between_inclusive(rng, 0, p_start)
                }
            }
            Self::WeightedList(w) => w.get_random_or_throw(rng).sample(rng, ctx),
        }
    }
}

// ---------------------------------------------------------------------------
// RuleTest (all 10 types) & BlockReplacement (§3.4)
// ---------------------------------------------------------------------------

/// All 10 `minecraft:rule_test` implementations (`features.md` §3.4).
#[derive(Debug, Clone)]
pub enum RuleTest {
    /// `minecraft:always_true`.
    AlwaysTrue,
    /// `minecraft:all_of`.
    AllOf(Vec<RuleTest>),
    /// `minecraft:any_of`.
    AnyOf(Vec<RuleTest>),
    /// `minecraft:not`.
    Not(Box<RuleTest>),
    /// `minecraft:block_match`.
    BlockMatch(u32),
    /// `minecraft:blockstate_match`.
    BlockStateMatch(u32),
    /// `minecraft:tag_match`.
    TagMatch(BlockBitSet),
    /// `minecraft:height_match`.
    HeightMatch {
        /// Inclusive minimum Y.
        min_inclusive: i32,
        /// Inclusive maximum Y.
        max_inclusive: i32,
    },
    /// `minecraft:random_block_match`.
    RandomBlockMatch {
        /// Target block protocol ID.
        block: u32,
        /// Replacement probability.
        probability: f32,
    },
    /// `minecraft:random_blockstate_match`.
    RandomBlockStateMatch {
        /// Target global block-state ID.
        block_state: u32,
        /// Replacement probability.
        probability: f32,
    },
}

impl RuleTest {
    /// Evaluates `RuleTest.test(state, pos, random)`.
    pub fn test(
        &self,
        state_id: u32,
        pos: BlockPos,
        rng: &mut impl RandomSource,
        region: &GenerationRegion,
    ) -> bool {
        match self {
            Self::AlwaysTrue => true,
            Self::AllOf(rules) => rules.iter().all(|r| r.test(state_id, pos, rng, region)),
            Self::AnyOf(rules) => rules.iter().any(|r| r.test(state_id, pos, rng, region)),
            Self::Not(rule) => !rule.test(state_id, pos, rng, region),
            Self::BlockMatch(bid) => region.ops().block_id(state_id) == *bid,
            Self::BlockStateMatch(sid) => state_id == *sid,
            Self::TagMatch(bs) => bs.contains(region.ops().block_id(state_id)),
            Self::HeightMatch {
                min_inclusive,
                max_inclusive,
            } => *min_inclusive <= pos.y && pos.y <= *max_inclusive,
            Self::RandomBlockMatch { block, probability } => {
                region.ops().block_id(state_id) == *block && rng.next_float() < *probability
            }
            Self::RandomBlockStateMatch {
                block_state,
                probability,
            } => state_id == *block_state && rng.next_float() < *probability,
        }
    }
}

/// `BlockReplacement` (`{ "target": RuleTest, "state": BlockState }`, §3.4).
#[derive(Debug, Clone)]
pub struct BlockReplacement {
    /// Rule matching existing block state.
    pub target: RuleTest,
    /// Replacement global block-state ID.
    pub state: u32,
}

// ---------------------------------------------------------------------------
// FluidHolderSet (for `matching_fluids`)
// ---------------------------------------------------------------------------

/// Set of fluid kinds (bitmask over `FluidKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FluidSet(u8);

impl FluidSet {
    /// Inserts a fluid kind.
    #[inline]
    pub fn insert(&mut self, kind: FluidKind) {
        self.0 |= 1 << Self::bit(kind);
    }

    /// Tests whether `kind` is in this set.
    #[inline]
    pub const fn contains(self, kind: FluidKind) -> bool {
        (self.0 & (1 << Self::bit(kind))) != 0
    }

    #[inline]
    const fn bit(kind: FluidKind) -> u8 {
        match kind {
            FluidKind::Empty => 0,
            FluidKind::Water => 1,
            FluidKind::FlowingWater => 2,
            FluidKind::Lava => 3,
            FluidKind::FlowingLava => 4,
        }
    }
}

// ---------------------------------------------------------------------------
// BlockPredicate (all 16 types, §3.1)
// ---------------------------------------------------------------------------

/// All 16 `minecraft:block_predicate_type` implementations (`features.md` §3.1).
#[derive(Debug, Clone)]
pub enum BlockPredicate {
    /// `minecraft:all_of`.
    AllOf(Vec<BlockPredicate>),
    /// `minecraft:any_of`.
    AnyOf(Vec<BlockPredicate>),
    /// `minecraft:not`.
    Not(Box<BlockPredicate>),
    /// `minecraft:true`.
    True,
    /// `minecraft:matching_blocks`.
    MatchingBlocks {
        /// Block offset from origin.
        offset: [i32; 3],
        /// Allowed blocks bitset.
        blocks: BlockBitSet,
    },
    /// `minecraft:matching_block_tag`.
    MatchingBlockTag {
        /// Block offset from origin.
        offset: [i32; 3],
        /// Tag block bitset.
        tag: BlockBitSet,
    },
    /// `minecraft:matching_fluids`.
    MatchingFluids {
        /// Block offset from origin.
        offset: [i32; 3],
        /// Allowed fluids.
        fluids: FluidSet,
    },
    /// `minecraft:matching_biomes` (new in 26.3, no offset).
    MatchingBiomes(BiomeBitSet),
    /// `minecraft:has_sturdy_face`.
    HasSturdyFace {
        /// Block offset from origin.
        offset: [i32; 3],
        /// Face direction.
        direction: Direction,
    },
    /// `minecraft:solid` (legacy solid).
    Solid {
        /// Block offset from origin.
        offset: [i32; 3],
    },
    /// `minecraft:replaceable`.
    Replaceable {
        /// Block offset from origin.
        offset: [i32; 3],
    },
    /// `minecraft:would_survive`.
    WouldSurvive {
        /// Block offset from origin.
        offset: [i32; 3],
        /// State whose `canSurvive` is tested.
        state: u32,
    },
    /// `minecraft:inside_world_bounds`.
    InsideWorldBounds {
        /// Block offset from origin.
        offset: [i32; 3],
    },
    /// `minecraft:unobstructed` (quirk: `offset` is decoded and ignored!).
    Unobstructed {
        /// Decoded offset (ignored at evaluation time per vanilla quirk).
        offset: [i32; 3],
    },
    /// `minecraft:height_range` (new in 26.3).
    HeightRange {
        /// Lower anchor.
        min_inclusive: VerticalAnchor,
        /// Upper anchor.
        max_inclusive: VerticalAnchor,
    },
    /// `minecraft:volume_match` (new in 26.3).
    VolumeMatch {
        /// Minimum inclusive offset.
        min: [i32; 3],
        /// Maximum inclusive offset.
        max: [i32; 3],
        /// Inner predicate tested at every offset in the box.
        match_predicate: Box<BlockPredicate>,
    },
}

impl BlockPredicate {
    /// Evaluates `BlockPredicate.test(level, origin)`.
    pub fn test(&self, region: &GenerationRegion, origin: BlockPos) -> bool {
        let ops = region.ops();
        match self {
            Self::AllOf(preds) => preds.iter().all(|p| p.test(region, origin)),
            Self::AnyOf(preds) => preds.iter().any(|p| p.test(region, origin)),
            Self::Not(p) => !p.test(region, origin),
            Self::True => true,
            Self::MatchingBlocks { offset, blocks } => {
                let p = origin.offset(offset[0], offset[1], offset[2]);
                blocks.contains(ops.block_id(region.get_block(p)))
            }
            Self::MatchingBlockTag { offset, tag } => {
                let p = origin.offset(offset[0], offset[1], offset[2]);
                tag.contains(ops.block_id(region.get_block(p)))
            }
            Self::MatchingFluids { offset, fluids } => {
                let p = origin.offset(offset[0], offset[1], offset[2]);
                fluids.contains(ops.fluid(region.get_block(p)).kind)
            }
            Self::MatchingBiomes(biomes) => biomes.contains(region.get_biome(origin)),
            Self::HasSturdyFace { offset, direction } => {
                let p = origin.offset(offset[0], offset[1], offset[2]);
                ops.is_face_sturdy(region.get_block(p), *direction)
            }
            Self::Solid { offset } => {
                let p = origin.offset(offset[0], offset[1], offset[2]);
                ops.is_solid(region.get_block(p))
            }
            Self::Replaceable { offset } => {
                let p = origin.offset(offset[0], offset[1], offset[2]);
                ops.is_replaceable(region.get_block(p))
            }
            Self::WouldSurvive { offset, state } => {
                let p = origin.offset(offset[0], offset[1], offset[2]);
                ops.can_survive(region, p, *state)
            }
            Self::InsideWorldBounds { offset } => {
                let p = origin.offset(offset[0], offset[1], offset[2]);
                region.is_inside_build_height(p.y)
            }
            Self::Unobstructed { .. } => {
                // Quirk: `offset` is ignored; checks `level.isUnobstructed(null, Shapes.block().move(origin))`.
                region.is_unobstructed(origin)
            }
            Self::HeightRange {
                min_inclusive,
                max_inclusive,
            } => {
                let ctx = WorldGenerationContext::new(
                    region.min_y().max(region.gen_min_y()),
                    region.height().min(region.gen_depth()),
                    region.sea_level(),
                );
                let min = min_inclusive.resolve_y(ctx);
                let max = max_inclusive.resolve_y(ctx);
                origin.y >= min && origin.y <= max
            }
            Self::VolumeMatch {
                min,
                max,
                match_predicate,
            } => {
                for ox in min[0]..=max[0] {
                    for oz in min[2]..=max[2] {
                        for oy in min[1]..=max[1] {
                            if !match_predicate.test(region, origin.offset(ox, oy, oz)) {
                                return false;
                            }
                        }
                    }
                }
                true
            }
        }
    }
}

// ---------------------------------------------------------------------------
// BlockStateProvider (all 10 types, §3.2)
// ---------------------------------------------------------------------------

/// One rule inside `RuleBasedStateProvider`.
#[derive(Debug, Clone)]
pub struct StateProviderRule {
    /// Condition predicate.
    pub if_true: BlockPredicate,
    /// Provider evaluated when `if_true` passes.
    pub then: Arc<BlockStateProvider>,
}

/// All 10 `minecraft:worldgen/block_state_provider_type` implementations (`features.md` §3.2).
#[derive(Debug, Clone)]
pub enum BlockStateProvider {
    /// `minecraft:simple`.
    Simple(u32),
    /// `minecraft:weighted`.
    Weighted(WeightedList<u32>),
    /// `minecraft:noise`.
    Noise {
        /// Instantiated `NormalNoise` seeded with `WorldgenRandom(LegacyRandom(seed))`.
        noise: NormalNoise,
        /// Coordinate scale (`f32`, widened to `f64` at sample time).
        scale: f32,
        /// Ordered non-empty state list.
        states: Vec<u32>,
    },
    /// `minecraft:dual_noise`.
    DualNoise {
        /// Variety range `[min, max]`.
        variety_min: i32,
        /// Variety range max inclusive.
        variety_max: i32,
        /// Slow noise instance (seeded with the same `seed`).
        slow_noise: NormalNoise,
        /// Slow noise scale.
        slow_scale: f32,
        /// Fast noise instance.
        noise: NormalNoise,
        /// Fast noise scale.
        scale: f32,
        /// Ordered non-empty state list.
        states: Vec<u32>,
    },
    /// `minecraft:noise_threshold`.
    NoiseThreshold {
        /// Instantiated noise.
        noise: NormalNoise,
        /// Coordinate scale.
        scale: f32,
        /// Noise threshold.
        threshold: f32,
        /// Chance to select from `high_states` when `v >= threshold`.
        high_chance: f32,
        /// Default state when `v >= threshold` and `nextFloat >= high_chance`.
        default_state: u32,
        /// States when `v < threshold`.
        low_states: Vec<u32>,
        /// States when `v >= threshold && nextFloat < high_chance`.
        high_states: Vec<u32>,
    },
    /// `minecraft:rotated`.
    Rotated {
        /// Underlying provider.
        state: Arc<BlockStateProvider>,
        /// Fixed direction, or `None` to draw `Direction.getRandom(rng)` BEFORE querying `state`.
        direction: Option<Direction>,
    },
    /// `minecraft:randomized_int`.
    RandomizedInt {
        /// Underlying provider.
        source: Arc<BlockStateProvider>,
        /// Integer property name.
        property: String,
        /// Integer values provider.
        values: IntProvider,
    },
    /// `minecraft:rule_based`.
    RuleBased {
        /// Optional fallback provider.
        fallback: Option<Arc<BlockStateProvider>>,
        /// Ordered rules.
        rules: Vec<StateProviderRule>,
    },
    /// `minecraft:copy_properties` (new in 26.3).
    CopyProperties {
        /// Underlying provider.
        source: Arc<BlockStateProvider>,
    },
    /// `minecraft:random_block` (new in 26.3).
    RandomBlock {
        /// Default states of every block in the `HolderSet<Block>`.
        default_states: Vec<u32>,
    },
}

impl BlockStateProvider {
    /// Constructs a `NormalNoise` using `WorldgenRandom(LegacyRandom(seed))` as required by §3.2.
    pub fn build_noise(seed: i64, params: NormalNoiseParameters) -> NormalNoise {
        let mut rng = WorldgenRandom::new_legacy(seed);
        NormalNoise::new(&mut rng, params)
    }

    #[inline]
    fn sample_noise(noise: &NormalNoise, pos: BlockPos, scale: f64) -> f32 {
        noise.sample(
            f64::from(pos.x) * scale,
            f64::from(pos.y) * scale,
            f64::from(pos.z) * scale,
        )
    }

    #[inline]
    fn state_from_noise_value(states: &[u32], val: f32) -> u32 {
        let p = clamp_f32((1.0_f32 + val) / 2.0_f32, 0.0_f32, 0.9999_f32);
        let idx = (p * (states.len() as f32)) as usize;
        states[idx]
    }

    /// `BlockStateProvider.getOptionalState(level, random, pos)`.
    pub fn get_optional_state(
        &self,
        region: &GenerationRegion,
        rng: &mut impl RandomSource,
        pos: BlockPos,
    ) -> Option<u32> {
        match self {
            Self::RuleBased { fallback, rules } => {
                for rule in rules {
                    if rule.if_true.test(region, pos)
                        && let Some(s) = rule.then.get_optional_state(region, rng, pos)
                    {
                        return Some(s);
                    }
                }
                fallback
                    .as_ref()
                    .and_then(|fb| fb.get_optional_state(region, rng, pos))
            }
            Self::RandomBlock { default_states } => {
                if default_states.is_empty() {
                    None
                } else {
                    Some(default_states[rng.next_int_bounded(default_states.len() as i32) as usize])
                }
            }
            _ => Some(self.get_state(region, rng, pos)),
        }
    }

    /// `BlockStateProvider.getState(level, random, pos)`.
    pub fn get_state(
        &self,
        region: &GenerationRegion,
        rng: &mut impl RandomSource,
        pos: BlockPos,
    ) -> u32 {
        match self {
            Self::Simple(s) => *s,
            Self::Weighted(w) => *w.get_random_or_throw(rng),
            Self::Noise {
                noise,
                scale,
                states,
            } => {
                let v = Self::sample_noise(noise, pos, f64::from(*scale));
                Self::state_from_noise_value(states, v)
            }
            Self::DualNoise {
                variety_min,
                variety_max,
                slow_noise,
                slow_scale,
                noise,
                scale,
                states,
            } => {
                let slow_s = f64::from(*slow_scale);
                let slow_val = f64::from(Self::sample_noise(slow_noise, pos, slow_s));
                let local_variety = clamped_map_f64(
                    slow_val,
                    -1.0,
                    1.0,
                    f64::from(*variety_min),
                    f64::from(*variety_max + 1),
                ) as i32;
                let mut possible = Vec::with_capacity(local_variety.max(0) as usize);
                for i in 0..local_variety {
                    let p = pos.offset(i * 54545, 0, i * 34234);
                    let sv = Self::sample_noise(slow_noise, p, slow_s);
                    possible.push(Self::state_from_noise_value(states, sv));
                }
                let fv = Self::sample_noise(noise, pos, f64::from(*scale));
                Self::state_from_noise_value(&possible, fv)
            }
            Self::NoiseThreshold {
                noise,
                scale,
                threshold,
                high_chance,
                default_state,
                low_states,
                high_states,
            } => {
                let v = f64::from(Self::sample_noise(noise, pos, f64::from(*scale)));
                if v < f64::from(*threshold) {
                    low_states[rng.next_int_bounded(low_states.len() as i32) as usize]
                } else if rng.next_float() < *high_chance {
                    high_states[rng.next_int_bounded(high_states.len() as i32) as usize]
                } else {
                    *default_state
                }
            }
            Self::Rotated { state, direction } => {
                // Draw direction BEFORE querying `self.state`!
                let dir = direction.unwrap_or_else(|| direction_get_random(rng));
                let mut s = state.get_state(region, rng, pos);
                let ops = region.ops();
                s = ops.try_set_property(s, "axis", direction_axis_name(dir));
                s = ops.try_set_property(s, "facing", dir.name());
                if direction_is_horizontal(dir) {
                    s = ops.try_set_property(s, "facing", dir.name());
                }
                s
            }
            Self::RandomizedInt {
                source,
                property,
                values,
            } => {
                let s = source.get_state(region, rng, pos);
                let ops = region.ops();
                if !ops.has_property(s, property) {
                    return s;
                }
                let val = values.sample(rng);
                let val_str = val.to_string();
                ops.try_set_property(s, property, &val_str)
            }
            Self::RuleBased { .. } | Self::RandomBlock { .. } => self
                .get_optional_state(region, rng, pos)
                .unwrap_or_else(|| region.get_block(pos)),
            Self::CopyProperties { source } => {
                let generated = source.get_state(region, rng, pos);
                let existing = region.get_block(pos);
                region.ops().with_properties_of(generated, existing)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Column.scan (§3.5)
// ---------------------------------------------------------------------------

/// Vertical open-space column returned by [`Column::scan`] (`features.md` §3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    /// Bounded on both floor and ceiling.
    Range {
        /// Floor block Y.
        floor: i32,
        /// Ceiling block Y.
        ceiling: i32,
    },
    /// Bounded on only one side.
    Ray {
        /// Solid boundary block Y.
        edge: i32,
        /// `true` if `edge` is a floor (open upward), `false` if `edge` is a ceiling (open downward).
        pointing_up: bool,
    },
    /// Unbounded on both sides within `search_range`.
    Line,
}

impl Column {
    /// Floor Y if present.
    #[inline]
    pub const fn floor(self) -> Option<i32> {
        match self {
            Self::Range { floor, .. } => Some(floor),
            Self::Ray {
                edge,
                pointing_up: true,
            } => Some(edge),
            _ => None,
        }
    }

    /// Ceiling Y if present.
    #[inline]
    pub const fn ceiling(self) -> Option<i32> {
        match self {
            Self::Range { ceiling, .. } => Some(ceiling),
            Self::Ray {
                edge,
                pointing_up: false,
            } => Some(edge),
            _ => None,
        }
    }

    /// Open interior height (`ceiling - floor - 1`) for `Range`, else `None`.
    #[inline]
    pub const fn height(self) -> Option<i32> {
        match self {
            Self::Range { floor, ceiling } => Some(ceiling - floor - 1),
            _ => None,
        }
    }

    /// Returns a copy with floor replaced by `new_floor`.
    #[inline]
    pub const fn with_floor(self, new_floor: i32) -> Self {
        match self {
            Self::Range { ceiling, .. } => Self::Range {
                floor: new_floor,
                ceiling,
            },
            Self::Ray {
                edge,
                pointing_up: false,
            } => Self::Range {
                floor: new_floor,
                ceiling: edge,
            },
            Self::Ray {
                pointing_up: true, ..
            }
            | Self::Line => Self::Ray {
                edge: new_floor,
                pointing_up: true,
            },
        }
    }

    /// `Column.scan(level, pos, searchRange, insideColumn, validEdge)` (`features.md` §3.5).
    pub fn scan(
        region: &GenerationRegion,
        pos: BlockPos,
        search_range: i32,
        mut inside_column: impl FnMut(u32) -> bool,
        mut valid_edge: impl FnMut(u32) -> bool,
    ) -> Option<Self> {
        if !inside_column(region.get_block(pos)) {
            return None;
        }
        let ceiling = Self::scan_direction(
            region,
            pos,
            1,
            search_range,
            &mut inside_column,
            &mut valid_edge,
        );
        let floor = Self::scan_direction(
            region,
            pos,
            -1,
            search_range,
            &mut inside_column,
            &mut valid_edge,
        );
        Some(match (floor, ceiling) {
            (Some(f), Some(c)) => Self::Range {
                floor: f,
                ceiling: c,
            },
            (Some(f), None) => Self::Ray {
                edge: f,
                pointing_up: true,
            },
            (None, Some(c)) => Self::Ray {
                edge: c,
                pointing_up: false,
            },
            (None, None) => Self::Line,
        })
    }

    fn scan_direction(
        region: &GenerationRegion,
        pos: BlockPos,
        dy: i32,
        search_range: i32,
        inside_column: &mut impl FnMut(u32) -> bool,
        valid_edge: &mut impl FnMut(u32) -> bool,
    ) -> Option<i32> {
        let mut m = pos;
        let mut i = 1;
        while i < search_range && inside_column(region.get_block(m)) {
            m.y += dy;
            i += 1;
        }
        if valid_edge(region.get_block(m)) {
            Some(m.y)
        } else {
            None
        }
    }
}

/// Helper for `minecraft:fluid` tag/id matching when decoding `FluidSet`.
pub fn parse_fluid_kind(id: &Identifier) -> Option<FluidKind> {
    if id.namespace() != "minecraft" {
        return None;
    }
    match id.path() {
        "empty" => Some(FluidKind::Empty),
        "water" => Some(FluidKind::Water),
        "flowing_water" => Some(FluidKind::FlowingWater),
        "lava" => Some(FluidKind::Lava),
        "flowing_lava" => Some(FluidKind::FlowingLava),
        _ => None,
    }
}
