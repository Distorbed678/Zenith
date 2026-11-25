//! Material rules, material conditions, ore veins (`minecraft:ore_vein`), badlands clay bands,
//! eroded badlands pillars, frozen ocean icebergs, and `MaterialSystem`
//! (`docs/specs/worldgen/surface-and-carvers.md` §1–§2,
//! `docs/specs/worldgen/aquifers-and-ore-veins.md` §3).

use std::collections::HashMap;
use std::sync::Arc;

use zenith_codec::codec::{
    BoolCodec, Codec, DataResult, DoubleCodec, FloatCodec, IdentifierCodec, IntCodec, RecordReader,
    decode_dispatch, list, validated,
};
use zenith_codec::json::Json;
use zenith_math::helpers::interp::map_f64;
use zenith_math::helpers::rounding::{floor_f32, floor_f64, java_min_f64, java_round_f32};
use zenith_math::{NormalNoise, NormalNoiseParameters};
use zenith_registry::{BlockStateTable, Identifier};

use crate::aquifer::WAY_BELOW_MIN_Y;
use crate::ast::{DensityFunction, RawDensityFunction};
use crate::biome_data::{BiomeBitSet, BiomeId, BiomeTable};
use crate::codec::{BlockStateSpecCodec, RawDensityFunctionCodec};
use crate::compiler::{CompileError, CompiledSampler};
use crate::data::WorldgenData;
use crate::generator::TerrainChunk;
use crate::heightmap::{BlockPredicates, resolve_block_state};
use crate::providers::{VerticalAnchor, VerticalAnchorCodec, WorldGenerationContext};
use crate::random_state::{PositionalFactory, RandomState};
use crate::settings::{BlockStateSpec, RouterField};
use crate::volume::{DensityVolume, SamplerContext};

/// Positional RNG identifier for `minecraft:ore_vein` (`aquifers-and-ore-veins.md` §3.2).
pub const ORE_RANDOM_ID: &str = "minecraft:ore";

/// RNG hash name for generating the 192-entry badlands terracotta `clayBands` table.
pub const CLAY_BANDS_RANDOM_ID: &str = "minecraft:clay_bands";

/// Surface orientation for `minecraft:stone_depth` (`CaveSurface`, `surface-and-carvers.md` §1.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CaveSurface {
    /// Downward / from above (`stoneDepthAbove`).
    Floor,
    /// Upward / from below (`stoneDepthBelow`).
    Ceiling,
}

/// Unresolved `HolderSet<Biome>` inside a `minecraft:biome` condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RawBiomeHolderSet {
    Tag(Identifier),
    Direct(Vec<Identifier>),
}

/// Unresolved `MaterialCondition` decoded from JSON before reference/tag resolution.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RawMaterialCondition {
    Reference(Identifier),
    Biome(RawBiomeHolderSet),
    NoiseThreshold {
        noise: Identifier,
        min_threshold: f64,
        max_threshold: f64,
        is_3d: bool,
    },
    VerticalGradient {
        random_name: Identifier,
        true_at_and_below: VerticalAnchor,
        false_at_and_above: VerticalAnchor,
    },
    YAbove {
        anchor: VerticalAnchor,
        surface_depth_multiplier: i32,
        add_stone_depth: bool,
    },
    Water {
        offset: i32,
        surface_depth_multiplier: i32,
        add_stone_depth: bool,
    },
    Temperature,
    Steep,
    Not(Box<RawMaterialCondition>),
    Hole,
    AbovePreliminarySurface,
    StoneDepth {
        offset: i32,
        add_surface_depth: bool,
        secondary_depth_range: i32,
        surface_type: CaveSurface,
    },
}

/// Unresolved `MaterialRule` decoded from JSON before reference/density resolution.
#[derive(Debug, Clone)]
pub(crate) enum RawMaterialRule {
    Reference(Identifier),
    Block(BlockStateSpec),
    Bandlands,
    Sequence(Vec<RawMaterialRule>),
    Condition {
        if_true: RawMaterialCondition,
        then_run: Box<RawMaterialRule>,
    },
    OreVein {
        ore_block: BlockStateSpec,
        raw_ore_block: BlockStateSpec,
        filler_block: BlockStateSpec,
        raw_ore_chance: f32,
        density: Box<RawDensityFunction>,
        richness: Box<RawDensityFunction>,
        filler_gap: Box<RawDensityFunction>,
    },
}

/// Resolved `minecraft:worldgen/material_condition` AST (all 11 built-in types).
#[derive(Debug, Clone, PartialEq)]
pub enum MaterialCondition {
    /// `minecraft:biome`: tests membership in the resolved biome list.
    Biome(Vec<Identifier>),
    /// `minecraft:noise_threshold`: tests 2D or 3D normal noise against `[min_threshold, max_threshold]`.
    NoiseThreshold {
        /// Noise registry ID.
        noise: Identifier,
        /// Inclusive lower threshold.
        min_threshold: f64,
        /// Inclusive upper threshold.
        max_threshold: f64,
        /// Whether to sample 3D `(x, y, z)` instead of 2D `(x, 0.0, z)` (new in 26.3).
        is_3d: bool,
    },
    /// `minecraft:vertical_gradient`: probabilistic vertical transition using a named positional factory.
    VerticalGradient {
        /// Positional random factory identifier.
        random_name: Identifier,
        /// Anchor at/below which the condition is always `true`.
        true_at_and_below: VerticalAnchor,
        /// Anchor at/above which the condition is always `false`.
        false_at_and_above: VerticalAnchor,
    },
    /// `minecraft:y_above`.
    YAbove {
        /// Base vertical anchor.
        anchor: VerticalAnchor,
        /// Multiplier on `surfaceDepth` in `[-20, 20]`.
        surface_depth_multiplier: i32,
        /// Whether to add `stoneDepthAbove` to `blockY`.
        add_stone_depth: bool,
    },
    /// `minecraft:water`.
    Water {
        /// Offset added to `waterHeight`.
        offset: i32,
        /// Multiplier on `surfaceDepth` in `[-20, 20]`.
        surface_depth_multiplier: i32,
        /// Whether to add `stoneDepthAbove` to `blockY`.
        add_stone_depth: bool,
    },
    /// `minecraft:temperature`: `biome.coldEnoughToSnow(pos, seaLevel)`.
    Temperature,
    /// `minecraft:steep`: `surfaceGradientX <= -4 || surfaceGradientZ >= 4`.
    Steep,
    /// `minecraft:not`.
    Not(Box<MaterialCondition>),
    /// `minecraft:hole`: `surfaceDepth <= 0`.
    Hole,
    /// `minecraft:above_preliminary_surface`: `blockY >= minSurfaceLevel`.
    AbovePreliminarySurface,
    /// `minecraft:stone_depth`.
    StoneDepth {
        /// Depth offset.
        offset: i32,
        /// Whether to add `surfaceDepth`.
        add_surface_depth: bool,
        /// Secondary depth noise mapping range.
        secondary_depth_range: i32,
        /// Floor (`stoneDepthAbove`) vs ceiling (`stoneDepthBelow`).
        surface_type: CaveSurface,
    },
}

/// Resolved `minecraft:worldgen/material_rule` AST (all 5 built-in types).
#[derive(Debug, Clone, PartialEq)]
pub enum MaterialRule {
    /// `minecraft:block`.
    Block(BlockStateSpec),
    /// `minecraft:bandlands`.
    Bandlands,
    /// `minecraft:sequence`.
    Sequence(Vec<MaterialRule>),
    /// `minecraft:condition`.
    Condition {
        /// Guard condition.
        if_true: MaterialCondition,
        /// Rule evaluated when `if_true` passes.
        then_run: Box<MaterialRule>,
    },
    /// `minecraft:ore_vein` (`aquifers-and-ore-veins.md` §3).
    OreVein {
        /// Regular ore block.
        ore_block: BlockStateSpec,
        /// Raw ore block.
        raw_ore_block: BlockStateSpec,
        /// Filler rock block.
        filler_block: BlockStateSpec,
        /// Probability of raw ore block when ore is chosen (`[0.0, 1.0]`).
        raw_ore_chance: f32,
        /// Vein solidness/density function (prefilled).
        density: Box<DensityFunction>,
        /// Ore richness function (prefilled).
        richness: Box<DensityFunction>,
        /// Filler gap suppression function (lazy point-evaluated).
        filler_gap: Box<DensityFunction>,
    },
}

impl MaterialRule {
    /// Collects references to every `DensityFunction` inside `OreVein` rules in deterministic
    /// tree order (`density`, `richness`, `filler_gap`), so they can be compiled alongside
    /// `NoiseRouter` and `AquiferConfig` in [`RandomState`].
    pub(crate) fn collect_ore_vein_functions<'a>(&'a self, out: &mut Vec<&'a DensityFunction>) {
        match self {
            Self::Block(_) | Self::Bandlands => {}
            Self::Sequence(children) => {
                for c in children {
                    c.collect_ore_vein_functions(out);
                }
            }
            Self::Condition { then_run, .. } => {
                then_run.collect_ore_vein_functions(out);
            }
            Self::OreVein {
                density,
                richness,
                filler_gap,
                ..
            } => {
                out.push(density);
                out.push(richness);
                out.push(filler_gap);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// JSON codecs for `MaterialCondition` and `MaterialRule`
// ---------------------------------------------------------------------------

struct CaveSurfaceCodec;
impl Codec<CaveSurface> for CaveSurfaceCodec {
    fn decode(&self, input: &Json) -> DataResult<CaveSurface> {
        let Some(s) = input.as_str() else {
            return DataResult::error(format!("Not a string: {}", input.type_name()));
        };
        match s {
            "floor" => DataResult::Success(CaveSurface::Floor),
            "ceiling" => DataResult::Success(CaveSurface::Ceiling),
            other => DataResult::error(format!("Invalid surface_type '{other}'")),
        }
    }
    fn encode(&self, _value: &CaveSurface) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct RawBiomeHolderSetCodec;
impl Codec<RawBiomeHolderSet> for RawBiomeHolderSetCodec {
    fn decode(&self, input: &Json) -> DataResult<RawBiomeHolderSet> {
        if let Some(s) = input.as_str() {
            if let Some(tag_str) = s.strip_prefix('#') {
                return Identifier::parse(tag_str)
                    .map(RawBiomeHolderSet::Tag)
                    .map_err(|e| format!("Invalid biome tag '{s}': {e}"))
                    .map_or_else(DataResult::error, DataResult::Success);
            }
            return IdentifierCodec
                .decode(input)
                .map(|id| RawBiomeHolderSet::Direct(vec![id]));
        }
        match input {
            Json::Array(_) => list(IdentifierCodec)
                .decode(input)
                .map(RawBiomeHolderSet::Direct),
            other => DataResult::error(format!(
                "Expected biome holder set (string or list), got {}",
                other.type_name()
            )),
        }
    }
    fn encode(&self, _value: &RawBiomeHolderSet) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

fn surface_depth_mult_codec() -> impl Codec<i32> {
    validated(IntCodec, |v: &i32| {
        if (-20..=20).contains(v) {
            Ok(())
        } else {
            Err(format!("surface_depth_multiplier {v} outside [-20, 20]"))
        }
    })
}

fn unit_float_codec() -> impl Codec<f32> {
    validated(FloatCodec, |v: &f32| {
        if (0.0_f32..=1.0_f32).contains(v) {
            Ok(())
        } else {
            Err(format!("Value {v} outside [0.0, 1.0]"))
        }
    })
}

struct RawMaterialConditionCodec;
impl Codec<RawMaterialCondition> for RawMaterialConditionCodec {
    fn decode(&self, input: &Json) -> DataResult<RawMaterialCondition> {
        decode_raw_material_condition(input)
    }
    fn encode(&self, _value: &RawMaterialCondition) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

pub(crate) fn decode_raw_material_condition(input: &Json) -> DataResult<RawMaterialCondition> {
    if matches!(input, Json::String(_)) {
        return IdentifierCodec
            .decode(input)
            .map(RawMaterialCondition::Reference);
    }
    decode_dispatch(input, "type", |ty, obj| {
        let id = Identifier::parse(ty).ok()?;
        if id.namespace() != "minecraft" {
            return None;
        }
        match id.path() {
            "biome" => Some({
                let mut r = RecordReader::new(obj);
                let biome_is = r.required("biome_is", &RawBiomeHolderSetCodec);
                r.finish(|| Some(RawMaterialCondition::Biome(biome_is?)))
            }),
            "noise_threshold" => Some({
                let mut r = RecordReader::new(obj);
                let noise = r.required("noise", &IdentifierCodec);
                let min_threshold = r.required("min_threshold", &DoubleCodec);
                let max_threshold = r.required("max_threshold", &DoubleCodec);
                let is_3d = r.optional("is_3d", &BoolCodec, false);
                r.finish(|| {
                    Some(RawMaterialCondition::NoiseThreshold {
                        noise: noise?,
                        min_threshold: min_threshold?,
                        max_threshold: max_threshold?,
                        is_3d,
                    })
                })
            }),
            "vertical_gradient" => Some({
                let mut r = RecordReader::new(obj);
                let random_name = r.required("random_name", &IdentifierCodec);
                let true_at_and_below = r.required("true_at_and_below", &VerticalAnchorCodec);
                let false_at_and_above = r.required("false_at_and_above", &VerticalAnchorCodec);
                r.finish(|| {
                    Some(RawMaterialCondition::VerticalGradient {
                        random_name: random_name?,
                        true_at_and_below: true_at_and_below?,
                        false_at_and_above: false_at_and_above?,
                    })
                })
            }),
            "y_above" => Some({
                let mut r = RecordReader::new(obj);
                let anchor = r.required("anchor", &VerticalAnchorCodec);
                let surface_depth_multiplier =
                    r.required("surface_depth_multiplier", &surface_depth_mult_codec());
                let add_stone_depth = r.required("add_stone_depth", &BoolCodec);
                r.finish(|| {
                    Some(RawMaterialCondition::YAbove {
                        anchor: anchor?,
                        surface_depth_multiplier: surface_depth_multiplier?,
                        add_stone_depth: add_stone_depth?,
                    })
                })
            }),
            "water" => Some({
                let mut r = RecordReader::new(obj);
                let offset = r.required("offset", &IntCodec);
                let surface_depth_multiplier =
                    r.required("surface_depth_multiplier", &surface_depth_mult_codec());
                let add_stone_depth = r.required("add_stone_depth", &BoolCodec);
                r.finish(|| {
                    Some(RawMaterialCondition::Water {
                        offset: offset?,
                        surface_depth_multiplier: surface_depth_multiplier?,
                        add_stone_depth: add_stone_depth?,
                    })
                })
            }),
            "temperature" => Some(DataResult::Success(RawMaterialCondition::Temperature)),
            "steep" => Some(DataResult::Success(RawMaterialCondition::Steep)),
            "not" => Some({
                let mut r = RecordReader::new(obj);
                let invert = r.required("invert", &RawMaterialConditionCodec);
                r.finish(|| Some(RawMaterialCondition::Not(Box::new(invert?))))
            }),
            "hole" => Some(DataResult::Success(RawMaterialCondition::Hole)),
            "above_preliminary_surface" => Some(DataResult::Success(
                RawMaterialCondition::AbovePreliminarySurface,
            )),
            "stone_depth" => Some({
                let mut r = RecordReader::new(obj);
                let offset = r.required("offset", &IntCodec);
                let add_surface_depth = r.required("add_surface_depth", &BoolCodec);
                let secondary_depth_range = r.required("secondary_depth_range", &IntCodec);
                let surface_type = r.required("surface_type", &CaveSurfaceCodec);
                r.finish(|| {
                    Some(RawMaterialCondition::StoneDepth {
                        offset: offset?,
                        add_surface_depth: add_surface_depth?,
                        secondary_depth_range: secondary_depth_range?,
                        surface_type: surface_type?,
                    })
                })
            }),
            _ => None,
        }
    })
}

struct RawMaterialRuleCodec;
impl Codec<RawMaterialRule> for RawMaterialRuleCodec {
    fn decode(&self, input: &Json) -> DataResult<RawMaterialRule> {
        decode_raw_material_rule(input)
    }
    fn encode(&self, _value: &RawMaterialRule) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

pub(crate) fn decode_raw_material_rule(input: &Json) -> DataResult<RawMaterialRule> {
    if matches!(input, Json::String(_)) {
        return IdentifierCodec
            .decode(input)
            .map(RawMaterialRule::Reference);
    }
    decode_dispatch(input, "type", |ty, obj| {
        let id = Identifier::parse(ty).ok()?;
        if id.namespace() != "minecraft" {
            return None;
        }
        match id.path() {
            "block" => Some({
                let mut r = RecordReader::new(obj);
                let result_state = r.required("result_state", &BlockStateSpecCodec);
                r.finish(|| Some(RawMaterialRule::Block(result_state?)))
            }),
            "bandlands" => Some(DataResult::Success(RawMaterialRule::Bandlands)),
            "sequence" => Some({
                let mut r = RecordReader::new(obj);
                let sequence = r.required("sequence", &list(RawMaterialRuleCodec));
                r.finish(|| Some(RawMaterialRule::Sequence(sequence?)))
            }),
            "condition" => Some({
                let mut r = RecordReader::new(obj);
                let if_true = r.required("if_true", &RawMaterialConditionCodec);
                let then_run = r.required("then_run", &RawMaterialRuleCodec);
                r.finish(|| {
                    Some(RawMaterialRule::Condition {
                        if_true: if_true?,
                        then_run: Box::new(then_run?),
                    })
                })
            }),
            "ore_vein" => Some({
                let mut r = RecordReader::new(obj);
                let ore_block = r.required("ore_block", &BlockStateSpecCodec);
                let raw_ore_block = r.required("raw_ore_block", &BlockStateSpecCodec);
                let filler_block = r.required("filler_block", &BlockStateSpecCodec);
                let raw_ore_chance = r.required("raw_ore_chance", &unit_float_codec());
                let density = r.required("density", &RawDensityFunctionCodec);
                let richness = r.required("richness", &RawDensityFunctionCodec);
                let filler_gap = r.required("filler_gap", &RawDensityFunctionCodec);
                r.finish(|| {
                    Some(RawMaterialRule::OreVein {
                        ore_block: ore_block?,
                        raw_ore_block: raw_ore_block?,
                        filler_block: filler_block?,
                        raw_ore_chance: raw_ore_chance?,
                        density: Box::new(density?),
                        richness: Box::new(richness?),
                        filler_gap: Box::new(filler_gap?),
                    })
                })
            }),
            _ => None,
        }
    })
}

// ---------------------------------------------------------------------------
// Dimension-level prepared MaterialRule & MaterialSystem
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum PreparedCondition {
    Biome(BiomeBitSet),
    NoiseThreshold {
        noise_idx: usize,
        min_threshold: f64,
        max_threshold: f64,
        is_3d: bool,
    },
    VerticalGradient {
        factory: PositionalFactory,
        true_at_and_below: VerticalAnchor,
        false_at_and_above: VerticalAnchor,
    },
    YAbove {
        anchor: VerticalAnchor,
        surface_depth_multiplier: i32,
        add_stone_depth: bool,
    },
    Water {
        offset: i32,
        surface_depth_multiplier: i32,
        add_stone_depth: bool,
    },
    Temperature,
    Steep,
    Not(Box<PreparedCondition>),
    Hole,
    AbovePreliminarySurface,
    StoneDepth {
        offset: i32,
        add_surface_depth: bool,
        secondary_depth_range: i32,
        surface_type: CaveSurface,
    },
}

#[derive(Debug, Clone)]
enum PreparedRule {
    Block(u32),
    Bandlands,
    Sequence(Vec<PreparedRule>),
    Condition {
        if_true: PreparedCondition,
        then_run: Box<PreparedRule>,
    },
    OreVein {
        ore_block: u32,
        raw_ore_block: u32,
        filler_block: u32,
        raw_ore_chance: f32,
        density: CompiledSampler,
        richness: CompiledSampler,
        filler_gap: CompiledSampler,
        ore_random: PositionalFactory,
    },
}

/// Dimension-level persistent surface state (`MaterialSystem`, `surface-and-carvers.md` §Inputs & §2).
#[derive(Debug)]
pub(crate) struct MaterialSystem {
    default_block_id: u32,
    default_block_first_state: u32,
    default_block_state_count: u32,
    sea_level: i32,
    snow_block_id: u32,
    packed_ice_id: u32,
    clay_bands: [u32; 192],
    clay_bands_offset_noise: Arc<NormalNoise>,
    surface_noise: Arc<NormalNoise>,
    surface_secondary_noise: Arc<NormalNoise>,
    badlands_pillar_noise: Arc<NormalNoise>,
    badlands_pillar_roof_noise: Arc<NormalNoise>,
    badlands_surface_noise: Arc<NormalNoise>,
    iceberg_pillar_noise: Arc<NormalNoise>,
    iceberg_pillar_roof_noise: Arc<NormalNoise>,
    iceberg_surface_noise: Arc<NormalNoise>,
    noises_2d: Vec<Arc<NormalNoise>>,
    noises_3d: Vec<Arc<NormalNoise>>,
    prepared_rule: PreparedRule,
}

#[derive(Debug)]
pub(crate) enum MaterialInitError {
    Compile(CompileError),
    UnknownBlock(Identifier),
    UnknownNoise(Identifier),
}

impl From<CompileError> for MaterialInitError {
    fn from(e: CompileError) -> Self {
        Self::Compile(e)
    }
}

fn require_block(blocks: &BlockStateTable, name: &str) -> Result<u32, MaterialInitError> {
    let id = Identifier::with_default_namespace(name).expect("valid static id");
    blocks
        .block(&id)
        .map(|b| b.default_state)
        .ok_or(MaterialInitError::UnknownBlock(id))
}

fn require_surface_noise(
    rs: &RandomState,
    data: &WorldgenData,
    name: &str,
    fallback_first_octave: i32,
    fallback_amplitudes: &[f64],
) -> Result<Arc<NormalNoise>, MaterialInitError> {
    let id = Identifier::with_default_namespace(name).expect("valid static id");
    if let Some(params) = data.noise(&id) {
        return Ok(rs.instantiate_noise(&id, params)?);
    }
    let params = NormalNoiseParameters::parity(fallback_first_octave, fallback_amplitudes.to_vec())
        .map_err(|e| MaterialInitError::Compile(CompileError::Noise(e.to_string())))?;
    Ok(rs.instantiate_noise(&id, &params)?)
}

#[allow(clippy::too_many_arguments)]
fn generate_clay_bands(
    root_random: &PositionalFactory,
    terracotta: u32,
    orange: u32,
    yellow: u32,
    brown: u32,
    red: u32,
    white: u32,
    light_gray: u32,
) -> [u32; 192] {
    let mut rng = root_random.from_hash_of(CLAY_BANDS_RANDOM_ID);
    let mut bands = [terracotta; 192];

    // Orange terracotta bands (spec §2.1 step 2): body adds nextInt(5) + 1 AND loop adds 1.
    let mut i = 0usize;
    while i < 192 {
        i += (rng.next_int_bounded(5) + 1) as usize;
        if i < 192 {
            bands[i] = orange;
        }
        i += 1;
    }

    // Yellow (1), brown (2), red (1) bands (spec §2.1 step 3-4).
    let mut make_bands = |base_width: i32, state: u32| {
        let band_count = rng.next_int_bounded(10) + 6;
        for _ in 0..band_count {
            let width = base_width + rng.next_int_bounded(3);
            let start = rng.next_int_bounded(192);
            for p in 0..width {
                let idx = start + p;
                if idx >= 192 {
                    break;
                }
                bands[idx as usize] = state;
            }
        }
    };
    make_bands(1, yellow);
    make_bands(2, brown);
    make_bands(1, red);

    // White and light-gray bands (spec §2.1 step 5).
    let white_band_count = rng.next_int_bounded(7) + 9;
    let mut start: i32 = 0;
    let mut k = 0;
    while k < white_band_count && start < 192 {
        bands[start as usize] = white;
        if start - 1 > 0 && rng.next_boolean() {
            bands[(start - 1) as usize] = light_gray;
        }
        if start + 1 < 192 && rng.next_boolean() {
            bands[(start + 1) as usize] = light_gray;
        }
        k += 1;
        start += rng.next_int_bounded(16) + 4;
    }

    bands
}

struct RulePreparer<'a> {
    rs: &'a RandomState,
    data: &'a WorldgenData,
    blocks: &'a BlockStateTable,
    biomes: &'a BiomeTable,
    ore_samplers: &'a [CompiledSampler],
    ore_sampler_cursor: usize,
    ore_random: PositionalFactory,
    noises_2d: Vec<Arc<NormalNoise>>,
    noises_2d_map: HashMap<Identifier, usize>,
    noises_3d: Vec<Arc<NormalNoise>>,
    noises_3d_map: HashMap<Identifier, usize>,
    random_factories: HashMap<Identifier, PositionalFactory>,
}

impl<'a> RulePreparer<'a> {
    fn get_noise_idx(&mut self, id: &Identifier, is_3d: bool) -> Result<usize, MaterialInitError> {
        let map = if is_3d {
            &mut self.noises_3d_map
        } else {
            &mut self.noises_2d_map
        };
        if let Some(&idx) = map.get(id) {
            return Ok(idx);
        }
        let params = self
            .data
            .noise(id)
            .ok_or_else(|| MaterialInitError::UnknownNoise(id.clone()))?;
        let noise = self.rs.instantiate_noise(id, params)?;
        if is_3d {
            let idx = self.noises_3d.len();
            self.noises_3d.push(noise);
            self.noises_3d_map.insert(id.clone(), idx);
            Ok(idx)
        } else {
            let idx = self.noises_2d.len();
            self.noises_2d.push(noise);
            self.noises_2d_map.insert(id.clone(), idx);
            Ok(idx)
        }
    }

    fn prepare_condition(
        &mut self,
        cond: &MaterialCondition,
    ) -> Result<PreparedCondition, MaterialInitError> {
        Ok(match cond {
            MaterialCondition::Biome(ids) => {
                let mut set = BiomeBitSet::new(self.biomes.len());
                for id in ids {
                    if let Some(bid) = self.biomes.get_id(id) {
                        set.insert(bid);
                    }
                }
                PreparedCondition::Biome(set)
            }
            MaterialCondition::NoiseThreshold {
                noise,
                min_threshold,
                max_threshold,
                is_3d,
            } => {
                let noise_idx = self.get_noise_idx(noise, *is_3d)?;
                PreparedCondition::NoiseThreshold {
                    noise_idx,
                    min_threshold: *min_threshold,
                    max_threshold: *max_threshold,
                    is_3d: *is_3d,
                }
            }
            MaterialCondition::VerticalGradient {
                random_name,
                true_at_and_below,
                false_at_and_above,
            } => {
                let factory = self
                    .random_factories
                    .entry(random_name.clone())
                    .or_insert_with(|| self.rs.positional_random_factory(random_name))
                    .clone();
                PreparedCondition::VerticalGradient {
                    factory,
                    true_at_and_below: *true_at_and_below,
                    false_at_and_above: *false_at_and_above,
                }
            }
            MaterialCondition::YAbove {
                anchor,
                surface_depth_multiplier,
                add_stone_depth,
            } => PreparedCondition::YAbove {
                anchor: *anchor,
                surface_depth_multiplier: *surface_depth_multiplier,
                add_stone_depth: *add_stone_depth,
            },
            MaterialCondition::Water {
                offset,
                surface_depth_multiplier,
                add_stone_depth,
            } => PreparedCondition::Water {
                offset: *offset,
                surface_depth_multiplier: *surface_depth_multiplier,
                add_stone_depth: *add_stone_depth,
            },
            MaterialCondition::Temperature => PreparedCondition::Temperature,
            MaterialCondition::Steep => PreparedCondition::Steep,
            MaterialCondition::Not(inner) => {
                PreparedCondition::Not(Box::new(self.prepare_condition(inner)?))
            }
            MaterialCondition::Hole => PreparedCondition::Hole,
            MaterialCondition::AbovePreliminarySurface => {
                PreparedCondition::AbovePreliminarySurface
            }
            MaterialCondition::StoneDepth {
                offset,
                add_surface_depth,
                secondary_depth_range,
                surface_type,
            } => PreparedCondition::StoneDepth {
                offset: *offset,
                add_surface_depth: *add_surface_depth,
                secondary_depth_range: *secondary_depth_range,
                surface_type: *surface_type,
            },
        })
    }

    fn prepare_rule(&mut self, rule: &MaterialRule) -> Result<PreparedRule, MaterialInitError> {
        Ok(match rule {
            MaterialRule::Block(spec) => {
                let sid = resolve_block_state(self.blocks, spec)
                    .map_err(MaterialInitError::UnknownBlock)?;
                PreparedRule::Block(sid)
            }
            MaterialRule::Bandlands => PreparedRule::Bandlands,
            MaterialRule::Sequence(children) => {
                let mut out = Vec::with_capacity(children.len());
                for c in children {
                    out.push(self.prepare_rule(c)?);
                }
                if out.len() == 1 {
                    out.pop().expect("single element")
                } else {
                    PreparedRule::Sequence(out)
                }
            }
            MaterialRule::Condition { if_true, then_run } => PreparedRule::Condition {
                if_true: self.prepare_condition(if_true)?,
                then_run: Box::new(self.prepare_rule(then_run)?),
            },
            MaterialRule::OreVein {
                ore_block,
                raw_ore_block,
                filler_block,
                raw_ore_chance,
                ..
            } => {
                let ore_id = resolve_block_state(self.blocks, ore_block)
                    .map_err(MaterialInitError::UnknownBlock)?;
                let raw_ore_id = resolve_block_state(self.blocks, raw_ore_block)
                    .map_err(MaterialInitError::UnknownBlock)?;
                let filler_id = resolve_block_state(self.blocks, filler_block)
                    .map_err(MaterialInitError::UnknownBlock)?;
                let idx = self.ore_sampler_cursor;
                self.ore_sampler_cursor += 3;
                PreparedRule::OreVein {
                    ore_block: ore_id,
                    raw_ore_block: raw_ore_id,
                    filler_block: filler_id,
                    raw_ore_chance: *raw_ore_chance,
                    density: self.ore_samplers[idx].clone(),
                    richness: self.ore_samplers[idx + 1].clone(),
                    filler_gap: self.ore_samplers[idx + 2].clone(),
                    ore_random: self.ore_random.clone(),
                }
            }
        })
    }
}

impl MaterialSystem {
    pub(crate) fn new(
        rs: &RandomState,
        data: &WorldgenData,
        blocks: &BlockStateTable,
        biomes: &BiomeTable,
        rule: &MaterialRule,
        ore_samplers: &[CompiledSampler],
        default_block_id: u32,
    ) -> Result<Self, MaterialInitError> {
        let default_entry = blocks
            .block(rs.settings().default_block_id())
            .ok_or_else(|| {
                MaterialInitError::UnknownBlock(rs.settings().default_block_id().clone())
            })?;
        let terracotta = require_block(blocks, "terracotta")?;
        let orange = require_block(blocks, "orange_terracotta")?;
        let yellow = require_block(blocks, "yellow_terracotta")?;
        let brown = require_block(blocks, "brown_terracotta")?;
        let red = require_block(blocks, "red_terracotta")?;
        let white = require_block(blocks, "white_terracotta")?;
        let light_gray = require_block(blocks, "light_gray_terracotta")?;
        let snow_block_id = require_block(blocks, "snow_block")?;
        let packed_ice_id = require_block(blocks, "packed_ice")?;

        let clay_bands = generate_clay_bands(
            rs.root_random(),
            terracotta,
            orange,
            yellow,
            brown,
            red,
            white,
            light_gray,
        );

        let clay_bands_offset_noise =
            require_surface_noise(rs, data, "clay_bands_offset", -8, &[1.0])?;
        let surface_noise = require_surface_noise(rs, data, "surface", -6, &[1.0, 1.0, 1.0])?;
        let surface_secondary_noise =
            require_surface_noise(rs, data, "surface_secondary", -6, &[1.0, 1.0, 0.0, 1.0])?;
        let badlands_pillar_noise =
            require_surface_noise(rs, data, "badlands_pillar", -2, &[1.0, 1.0, 1.0, 1.0])?;
        let badlands_pillar_roof_noise =
            require_surface_noise(rs, data, "badlands_pillar_roof", -8, &[1.0])?;
        let badlands_surface_noise =
            require_surface_noise(rs, data, "badlands_surface", -6, &[1.0, 1.0, 1.0])?;
        let iceberg_pillar_noise =
            require_surface_noise(rs, data, "iceberg_pillar", -6, &[1.0, 1.0, 1.0, 1.0])?;
        let iceberg_pillar_roof_noise =
            require_surface_noise(rs, data, "iceberg_pillar_roof", -3, &[1.0])?;
        let iceberg_surface_noise =
            require_surface_noise(rs, data, "iceberg_surface", -6, &[1.0, 1.0, 1.0])?;

        let ore_id = Identifier::parse(ORE_RANDOM_ID).expect("valid static id");
        let mut preparer = RulePreparer {
            rs,
            data,
            blocks,
            biomes,
            ore_samplers,
            ore_sampler_cursor: 0,
            ore_random: rs.positional_random_factory(&ore_id),
            noises_2d: Vec::new(),
            noises_2d_map: HashMap::new(),
            noises_3d: Vec::new(),
            noises_3d_map: HashMap::new(),
            random_factories: HashMap::new(),
        };
        let prepared_rule = preparer.prepare_rule(rule)?;

        Ok(Self {
            default_block_id,
            default_block_first_state: default_entry.first_state,
            default_block_state_count: default_entry.state_count,
            sea_level: rs.settings().sea_level(),
            snow_block_id,
            packed_ice_id,
            clay_bands,
            clay_bands_offset_noise,
            surface_noise,
            surface_secondary_noise,
            badlands_pillar_noise,
            badlands_pillar_roof_noise,
            badlands_surface_noise,
            iceberg_pillar_noise,
            iceberg_pillar_roof_noise,
            iceberg_surface_noise,
            noises_2d: preparer.noises_2d,
            noises_3d: preparer.noises_3d,
            prepared_rule,
        })
    }

    #[inline]
    fn is_default_block(&self, state_id: u32) -> bool {
        state_id >= self.default_block_first_state
            && state_id < self.default_block_first_state + self.default_block_state_count
    }

    #[inline]
    fn get_band(&self, world_x: i32, y: i32, world_z: i32) -> u32 {
        let offset = java_round_f32(
            self.clay_bands_offset_noise
                .sample(f64::from(world_x), 0.0_f64, f64::from(world_z))
                * 4.0_f32,
        );
        let idx = y.wrapping_add(offset).wrapping_add(192) % 192;
        self.clay_bands[idx as usize]
    }

    #[inline]
    fn get_surface_depth(&self, root_random: &PositionalFactory, x: i32, z: i32) -> i32 {
        let n = f64::from(
            self.surface_noise
                .sample(f64::from(x), 0.0_f64, f64::from(z)),
        );
        let r = root_random.at(x, 0, z).next_double();
        (n * 2.75_f64 + 3.0_f64 + r * 0.25_f64) as i32
    }

    /// Executes `MaterialSystem.buildSurface` (`surface-and-carvers.md` §1.6 & §2).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build_surface<F>(
        &self,
        rs: &RandomState,
        biomes: &BiomeTable,
        gen_ctx: WorldGenerationContext,
        chunk: &mut TerrainChunk,
        full_volume: DensityVolume,
        sampler_ctx: &mut SamplerContext,
        predicates: &BlockPredicates,
        possible_biomes: Option<&BiomeBitSet>,
        mut get_biome: F,
    ) where
        F: FnMut(i32, i32, i32) -> BiomeId,
    {
        let min_y = chunk.min_y();
        let height = chunk.height();
        if height <= 0 {
            return;
        }
        let max_y = min_y + height - 1;
        let min_section_y = min_y >> 4;

        // Narrowed volume from highest filled section (spec §1.6 step 1).
        let mut highest_filled_section: i32 = -1;
        for sec in (0..chunk.section_count()).rev() {
            if chunk.section(sec).iter().any(|&b| !predicates.is_air(b)) {
                highest_filled_section = sec as i32;
                break;
            }
        }
        let max_filled_y = if highest_filled_section >= 0 {
            (min_section_y + highest_filled_section) * 16 + 15
        } else {
            min_y - 1
        };
        let narrowed_size_y = (max_filled_y - full_volume.min_block_y() + 1).max(1) as usize;
        let narrowed_volume = DensityVolume::unit(
            full_volume.size_x(),
            narrowed_size_y,
            full_volume.size_z(),
            full_volume.min_block_x(),
            full_volume.min_block_y(),
            full_volume.min_block_z(),
        )
        .expect("narrowed surface volume is valid");

        // Compile rule for this chunk (triggers BiomeCondition pruning + OreVein prefill).
        let can_prune_empty_ore_vein = possible_biomes
            .is_some_and(|pb| biomes.eroded_badlands().is_none_or(|eb| !pb.contains(eb)));
        let active_rule = compile_active_rule(
            &self.prepared_rule,
            gen_ctx,
            narrowed_volume,
            sampler_ctx,
            possible_biomes,
            can_prune_empty_ore_vein,
        );

        let mut rule_ctx = MaterialRuleContext::new(
            self,
            rs,
            biomes,
            narrowed_volume,
            sampler_ctx,
            &mut get_biome,
        );

        let min_block_x = chunk.chunk_x().wrapping_shl(4);
        let min_block_z = chunk.chunk_z().wrapping_shl(4);
        let eroded_badlands = biomes.eroded_badlands();
        let frozen_ocean = biomes.frozen_ocean();
        let deep_frozen_ocean = biomes.deep_frozen_ocean();

        // Exact loop order: x = 0..16 (outer), z = 0..16 (inner) (spec §1.6 step 3).
        for x in 0..16usize {
            let block_x = min_block_x.wrapping_add(x as i32);
            for z in 0..16usize {
                let block_z = min_block_z.wrapping_add(z as i32);
                let starting_height = chunk.world_surface_wg().first_available(x, z);
                let surface_biome = (rule_ctx.get_biome)(block_x, starting_height, block_z);

                // Pre-pass: eroded badlands pillars (spec §2.2).
                if Some(surface_biome) == eroded_badlands {
                    self.eroded_badlands_extension(
                        chunk,
                        x,
                        z,
                        block_x,
                        block_z,
                        starting_height,
                        min_y,
                        max_y,
                        predicates,
                    );
                }

                // Re-read height & live gradients (spec §1.6 step 3.5).
                let col_height = chunk.world_surface_wg().first_available(x, z);
                let ws = chunk.world_surface_wg();
                let gradient_x = ws.first_available((x + 1).min(15), z)
                    - ws.first_available(x.saturating_sub(1), z);
                let gradient_z = ws.first_available(x, (z + 1).min(15))
                    - ws.first_available(x, z.saturating_sub(1));
                rule_ctx.update_xz(block_x, block_z, gradient_x, gradient_z);

                // Downward column walk (spec §1.6 step 3.6).
                let mut stone_above_depth = 0i32;
                let mut water_height = i32::MIN;
                let mut next_ceiling_stone_y = i32::MAX;
                let end_y = min_y;

                let mut y = col_height;
                while y >= end_y {
                    let old = chunk.get_block(x, y, z);
                    if predicates.is_air(old) {
                        stone_above_depth = 0;
                        water_height = i32::MIN;
                    } else if predicates.has_fluid(old) {
                        if water_height == i32::MIN {
                            water_height = y + 1;
                        }
                    } else {
                        if next_ceiling_stone_y >= y {
                            next_ceiling_stone_y = WAY_BELOW_MIN_Y;
                            let mut lookahead_y = y - 1;
                            while lookahead_y >= end_y - 1 {
                                let la = chunk.get_block(x, lookahead_y, z);
                                if !predicates.is_stone(la) {
                                    next_ceiling_stone_y = lookahead_y + 1;
                                    break;
                                }
                                lookahead_y -= 1;
                            }
                        }
                        stone_above_depth += 1;
                        let stone_below_depth = y - next_ceiling_stone_y + 1;
                        rule_ctx.update_y(stone_above_depth, stone_below_depth, water_height, y);
                        if y >= min_y
                            && y <= max_y
                            && let Some(state) =
                                active_rule.try_apply(&mut rule_ctx, block_x, y, block_z)
                        {
                            column_set_block(
                                chunk, x, y, z, block_x, block_z, min_y, max_y, state, predicates,
                            );
                        }
                    }
                    y -= 1;
                }

                // Post-pass: frozen ocean icebergs (spec §2.3).
                if Some(surface_biome) == frozen_ocean || Some(surface_biome) == deep_frozen_ocean {
                    let min_surface_level = rule_ctx.get_min_surface_level();
                    self.frozen_ocean_extension(
                        rs.root_random(),
                        biomes,
                        min_surface_level,
                        surface_biome,
                        chunk,
                        x,
                        z,
                        block_x,
                        block_z,
                        starting_height,
                        min_y,
                        max_y,
                        predicates,
                    );
                }
            }
        }
    }

    /// Executes `MaterialSystem.topMaterial` for carver grass/mycelium fix-up (`surface-and-carvers.md` §1.7).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn top_material<F>(
        &self,
        rs: &RandomState,
        biomes: &BiomeTable,
        gen_ctx: WorldGenerationContext,
        chunk: &TerrainChunk,
        sampler_ctx: &mut SamplerContext,
        block_x: i32,
        block_y: i32,
        block_z: i32,
        under_fluid: bool,
        mut get_biome: F,
    ) -> Option<u32>
    where
        F: FnMut(i32, i32, i32) -> BiomeId,
    {
        let vol = DensityVolume::unit(1, 1, 1, block_x, block_y, block_z).expect("1x1x1 valid");
        let active_rule =
            compile_active_rule(&self.prepared_rule, gen_ctx, vol, sampler_ctx, None, true);
        let lx = (block_x & 15) as usize;
        let lz = (block_z & 15) as usize;
        let ws = chunk.world_surface_wg();
        let gradient_x =
            ws.first_available((lx + 1).min(15), lz) - ws.first_available(lx.saturating_sub(1), lz);
        let gradient_z =
            ws.first_available(lx, (lz + 1).min(15)) - ws.first_available(lx, lz.saturating_sub(1));

        let mut rule_ctx =
            MaterialRuleContext::new(self, rs, biomes, vol, sampler_ctx, &mut get_biome);
        rule_ctx.update_xz(block_x, block_z, gradient_x, gradient_z);
        rule_ctx.update_y(
            1,
            1,
            if under_fluid { block_y + 1 } else { i32::MIN },
            block_y,
        );
        active_rule.try_apply(&mut rule_ctx, block_x, block_y, block_z)
    }

    #[allow(clippy::too_many_arguments)]
    fn eroded_badlands_extension(
        &self,
        chunk: &mut TerrainChunk,
        x: usize,
        z: usize,
        block_x: i32,
        block_z: i32,
        starting_height: i32,
        min_y: i32,
        max_y: i32,
        predicates: &BlockPredicates,
    ) {
        let bx = f64::from(block_x);
        let bz = f64::from(block_z);
        let a = (f64::from(self.badlands_surface_noise.sample(bx, 0.0_f64, bz)) * 8.25_f64).abs();
        let b = f64::from(
            self.badlands_pillar_noise
                .sample(bx * 0.2_f64, 0.0_f64, bz * 0.2_f64)
                * 15.0_f32,
        );
        let pillar_buffer = java_min_f64(a, b);
        if pillar_buffer <= 0.0_f64 {
            return;
        }
        let pillar_floor = (f64::from(self.badlands_pillar_roof_noise.sample(
            bx * 0.75_f64,
            0.0_f64,
            bz * 0.75_f64,
        )) * 1.5_f64)
            .abs();
        let extension_top = 64.0_f64
            + java_min_f64(
                pillar_buffer * pillar_buffer * 2.5_f64,
                (pillar_floor * 50.0_f64).ceil() + 24.0_f64,
            );
        let start_y = floor_f64(extension_top);
        if starting_height > start_y {
            return;
        }

        // Water check (spec §2.2 step 7).
        let mut y = start_y;
        while y >= min_y {
            let s = chunk.get_block(x, y, z);
            if self.is_default_block(s) {
                break;
            }
            if predicates.is_water(s) {
                return;
            }
            y -= 1;
        }

        // Pillar fill (spec §2.2 step 8).
        let mut y = start_y;
        while y >= min_y && predicates.is_air(chunk.get_block(x, y, z)) {
            column_set_block(
                chunk,
                x,
                y,
                z,
                block_x,
                block_z,
                min_y,
                max_y,
                self.default_block_id,
                predicates,
            );
            y -= 1;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn frozen_ocean_extension(
        &self,
        root_random: &PositionalFactory,
        biomes: &BiomeTable,
        min_surface_level: i32,
        surface_biome: BiomeId,
        chunk: &mut TerrainChunk,
        x: usize,
        z: usize,
        block_x: i32,
        block_z: i32,
        starting_height: i32,
        min_y: i32,
        max_y: i32,
        predicates: &BlockPredicates,
    ) {
        let bx = f64::from(block_x);
        let bz = f64::from(block_z);
        let a = (f64::from(self.iceberg_surface_noise.sample(bx, 0.0_f64, bz)) * 8.25_f64).abs();
        let b = f64::from(
            self.iceberg_pillar_noise
                .sample(bx * 1.28_f64, 0.0_f64, bz * 1.28_f64)
                * 15.0_f32,
        );
        let iceberg = java_min_f64(a, b);
        if iceberg <= 1.8_f64 {
            return;
        }
        let iceberg_roof = (f64::from(self.iceberg_pillar_roof_noise.sample(
            bx * 1.17_f64,
            0.0_f64,
            bz * 1.17_f64,
        )) * 1.5_f64)
            .abs();
        let mut top = java_min_f64(
            iceberg * iceberg * 1.2_f64,
            (iceberg_roof * 40.0_f64).ceil() + 14.0_f64,
        );
        if biomes
            .data(surface_biome)
            .should_melt_frozen_ocean_iceberg_slightly(
                block_x,
                self.sea_level,
                block_z,
                self.sea_level,
            )
        {
            top -= 2.0_f64;
        }
        if top <= 2.0_f64 {
            return;
        }
        let extension_bottom = f64::from(self.sea_level) - top - 7.0_f64;
        let extension_top = top + f64::from(self.sea_level);
        let mut rng = root_random.at(block_x, 0, block_z);
        let max_snow_depth = 2 + rng.next_int_bounded(4);
        let min_snow_height = self.sea_level + 18 + rng.next_int_bounded(10);
        let mut snow_depth = 0;

        let ext_top_i32 = extension_top as i32;
        let ext_bot_i32 = extension_bottom as i32;
        let mut y = starting_height.max(ext_top_i32 + 1);
        while y >= min_surface_level {
            let cur = chunk.get_block(x, y, z);
            let place = (predicates.is_air(cur) && y < ext_top_i32 && rng.next_double() > 0.01_f64)
                || (predicates.is_water(cur)
                    && y > ext_bot_i32
                    && y < self.sea_level
                    && rng.next_double() > 0.15_f64);
            if place {
                if snow_depth <= max_snow_depth && y > min_snow_height {
                    column_set_block(
                        chunk,
                        x,
                        y,
                        z,
                        block_x,
                        block_z,
                        min_y,
                        max_y,
                        self.snow_block_id,
                        predicates,
                    );
                    snow_depth += 1;
                } else {
                    column_set_block(
                        chunk,
                        x,
                        y,
                        z,
                        block_x,
                        block_z,
                        min_y,
                        max_y,
                        self.packed_ice_id,
                        predicates,
                    );
                }
            }
            y -= 1;
        }
    }
}

#[inline]
#[allow(clippy::too_many_arguments)]
fn column_set_block(
    chunk: &mut TerrainChunk,
    x: usize,
    y: i32,
    z: usize,
    block_x: i32,
    block_z: i32,
    min_y: i32,
    max_y: i32,
    state: u32,
    predicates: &BlockPredicates,
) {
    if y >= min_y && y <= max_y {
        chunk.set_block_and_update_heightmaps(x, y, z, state, predicates);
        if predicates.has_fluid(state) {
            chunk.mark_pos_for_post_processing(block_x, y, block_z);
        }
    }
}

// ---------------------------------------------------------------------------
// Per-chunk compiled ActiveRule & MaterialRuleContext
// ---------------------------------------------------------------------------

enum ActiveCondition<'a> {
    Const(bool),
    Biome(&'a BiomeBitSet),
    NoiseThreshold2D {
        noise_idx: usize,
        min_threshold: f64,
        max_threshold: f64,
    },
    NoiseThreshold3D {
        noise_idx: usize,
        min_threshold: f64,
        max_threshold: f64,
    },
    VerticalGradient {
        factory: &'a PositionalFactory,
        lo: i32,
        hi: i32,
    },
    YAbove {
        anchor_y: i32,
        surface_depth_multiplier: i32,
        add_stone_depth: bool,
    },
    Water {
        offset: i32,
        surface_depth_multiplier: i32,
        add_stone_depth: bool,
    },
    Temperature,
    Steep,
    Not(Box<ActiveCondition<'a>>),
    Hole,
    AbovePreliminarySurface,
    StoneDepth {
        offset: i32,
        add_surface_depth: bool,
        secondary_depth_range: i32,
        surface_type: CaveSurface,
    },
}

enum ActiveRule<'a> {
    Noop,
    Block(u32),
    Bandlands,
    Sequence(Vec<ActiveRule<'a>>),
    Condition {
        if_true: ActiveCondition<'a>,
        then_run: Box<ActiveRule<'a>>,
    },
    OreVein {
        ore_block: u32,
        raw_ore_block: u32,
        filler_block: u32,
        raw_ore_chance: f32,
        expected_volume: DensityVolume,
        density_buf: Vec<f32>,
        richness_buf: Vec<f32>,
        density_sampler: &'a CompiledSampler,
        richness_sampler: &'a CompiledSampler,
        filler_gap_sampler: &'a CompiledSampler,
        ore_random: &'a PositionalFactory,
    },
}

fn compile_active_condition<'a>(
    cond: &'a PreparedCondition,
    gen_ctx: WorldGenerationContext,
    possible_biomes: Option<&BiomeBitSet>,
) -> ActiveCondition<'a> {
    match cond {
        PreparedCondition::Biome(set) => {
            if let Some(pb) = possible_biomes {
                if !pb.intersects(set) {
                    return ActiveCondition::Const(false);
                }
                if pb.is_subset_of(set) {
                    return ActiveCondition::Const(true);
                }
            }
            ActiveCondition::Biome(set)
        }
        PreparedCondition::NoiseThreshold {
            noise_idx,
            min_threshold,
            max_threshold,
            is_3d,
        } => {
            if *is_3d {
                ActiveCondition::NoiseThreshold3D {
                    noise_idx: *noise_idx,
                    min_threshold: *min_threshold,
                    max_threshold: *max_threshold,
                }
            } else {
                ActiveCondition::NoiseThreshold2D {
                    noise_idx: *noise_idx,
                    min_threshold: *min_threshold,
                    max_threshold: *max_threshold,
                }
            }
        }
        PreparedCondition::VerticalGradient {
            factory,
            true_at_and_below,
            false_at_and_above,
        } => ActiveCondition::VerticalGradient {
            factory,
            lo: true_at_and_below.resolve_y(gen_ctx),
            hi: false_at_and_above.resolve_y(gen_ctx),
        },
        PreparedCondition::YAbove {
            anchor,
            surface_depth_multiplier,
            add_stone_depth,
        } => ActiveCondition::YAbove {
            anchor_y: anchor.resolve_y(gen_ctx),
            surface_depth_multiplier: *surface_depth_multiplier,
            add_stone_depth: *add_stone_depth,
        },
        PreparedCondition::Water {
            offset,
            surface_depth_multiplier,
            add_stone_depth,
        } => ActiveCondition::Water {
            offset: *offset,
            surface_depth_multiplier: *surface_depth_multiplier,
            add_stone_depth: *add_stone_depth,
        },
        PreparedCondition::Temperature => ActiveCondition::Temperature,
        PreparedCondition::Steep => ActiveCondition::Steep,
        PreparedCondition::Not(inner) => {
            match compile_active_condition(inner, gen_ctx, possible_biomes) {
                ActiveCondition::Const(b) => ActiveCondition::Const(!b),
                other => ActiveCondition::Not(Box::new(other)),
            }
        }
        PreparedCondition::Hole => ActiveCondition::Hole,
        PreparedCondition::AbovePreliminarySurface => ActiveCondition::AbovePreliminarySurface,
        PreparedCondition::StoneDepth {
            offset,
            add_surface_depth,
            secondary_depth_range,
            surface_type,
        } => ActiveCondition::StoneDepth {
            offset: *offset,
            add_surface_depth: *add_surface_depth,
            secondary_depth_range: *secondary_depth_range,
            surface_type: *surface_type,
        },
    }
}

fn compile_active_rule<'a>(
    rule: &'a PreparedRule,
    gen_ctx: WorldGenerationContext,
    expected_volume: DensityVolume,
    sampler_ctx: &mut SamplerContext,
    possible_biomes: Option<&BiomeBitSet>,
    can_prune_empty_ore_vein: bool,
) -> ActiveRule<'a> {
    match rule {
        PreparedRule::Block(s) => ActiveRule::Block(*s),
        PreparedRule::Bandlands => ActiveRule::Bandlands,
        PreparedRule::Sequence(children) => {
            let mut active = Vec::with_capacity(children.len());
            for c in children {
                let compiled = compile_active_rule(
                    c,
                    gen_ctx,
                    expected_volume,
                    sampler_ctx,
                    possible_biomes,
                    can_prune_empty_ore_vein,
                );
                if !matches!(compiled, ActiveRule::Noop) {
                    let is_unconditional =
                        matches!(compiled, ActiveRule::Block(_) | ActiveRule::Bandlands);
                    active.push(compiled);
                    if is_unconditional {
                        break;
                    }
                }
            }
            match active.len() {
                0 => ActiveRule::Noop,
                1 => active.pop().expect("single element"),
                _ => ActiveRule::Sequence(active),
            }
        }
        PreparedRule::Condition { if_true, then_run } => {
            let cond = compile_active_condition(if_true, gen_ctx, possible_biomes);
            match cond {
                ActiveCondition::Const(false) => ActiveRule::Noop,
                ActiveCondition::Const(true) => compile_active_rule(
                    then_run,
                    gen_ctx,
                    expected_volume,
                    sampler_ctx,
                    possible_biomes,
                    can_prune_empty_ore_vein,
                ),
                other => {
                    let inner = compile_active_rule(
                        then_run,
                        gen_ctx,
                        expected_volume,
                        sampler_ctx,
                        possible_biomes,
                        can_prune_empty_ore_vein,
                    );
                    if matches!(inner, ActiveRule::Noop) {
                        ActiveRule::Noop
                    } else {
                        ActiveRule::Condition {
                            if_true: other,
                            then_run: Box::new(inner),
                        }
                    }
                }
            }
        }
        PreparedRule::OreVein {
            ore_block,
            raw_ore_block,
            filler_block,
            raw_ore_chance,
            density,
            richness,
            filler_gap,
            ore_random,
        } => {
            let len = expected_volume.len();
            let mut density_buf = vec![0.0_f32; len];
            density.sample_volume(sampler_ctx, &mut density_buf, expected_volume);
            if can_prune_empty_ore_vein && density_buf.iter().all(|&d| d <= 0.0_f32) {
                return ActiveRule::Noop;
            }
            let mut richness_buf = vec![0.0_f32; len];
            richness.sample_volume(sampler_ctx, &mut richness_buf, expected_volume);
            ActiveRule::OreVein {
                ore_block: *ore_block,
                raw_ore_block: *raw_ore_block,
                filler_block: *filler_block,
                raw_ore_chance: *raw_ore_chance,
                expected_volume,
                density_buf,
                richness_buf,
                density_sampler: density,
                richness_sampler: richness,
                filler_gap_sampler: filler_gap,
                ore_random,
            }
        }
    }
}

struct MaterialRuleContext<'a, F> {
    system: &'a MaterialSystem,
    rs: &'a RandomState,
    biomes: &'a BiomeTable,
    sampler_ctx: &'a mut SamplerContext,
    get_biome: &'a mut F,
    prelim_vol: DensityVolume,
    prelim_buf: Option<Vec<f32>>,

    last_update_xz: i64,
    last_update_y: i64,

    block_x: i32,
    block_y: i32,
    block_z: i32,
    gradient_x: i32,
    gradient_z: i32,
    surface_depth: i32,
    stone_depth_above: i32,
    stone_depth_below: i32,
    water_height: i32,

    cached_biome: Option<BiomeId>,
    secondary_cache: (i64, f64),
    min_surface_level_cache: (i64, i32),
    noise_2d_cache: Vec<(i64, f64)>,
    noise_3d_cache: Vec<(i64, f64)>,
}

impl<'a, F> MaterialRuleContext<'a, F>
where
    F: FnMut(i32, i32, i32) -> BiomeId,
{
    fn new(
        system: &'a MaterialSystem,
        rs: &'a RandomState,
        biomes: &'a BiomeTable,
        expected_volume: DensityVolume,
        sampler_ctx: &'a mut SamplerContext,
        get_biome: &'a mut F,
    ) -> Self {
        let prelim_vol = DensityVolume::unit(
            expected_volume.size_x(),
            1,
            expected_volume.size_z(),
            expected_volume.min_block_x(),
            0,
            expected_volume.min_block_z(),
        )
        .expect("preliminary surface 2D volume is valid");

        let init_ts = i64::MIN;
        Self {
            system,
            rs,
            biomes,
            sampler_ctx,
            get_biome,
            prelim_vol,
            prelim_buf: None,
            last_update_xz: i64::MIN + 1,
            last_update_y: i64::MIN + 1,
            block_x: 0,
            block_y: 0,
            block_z: 0,
            gradient_x: 0,
            gradient_z: 0,
            surface_depth: 0,
            stone_depth_above: 0,
            stone_depth_below: 0,
            water_height: i32::MIN,
            cached_biome: None,
            secondary_cache: (init_ts, 0.0),
            min_surface_level_cache: (init_ts, 0),
            noise_2d_cache: vec![(init_ts, 0.0); system.noises_2d.len()],
            noise_3d_cache: vec![(init_ts, 0.0); system.noises_3d.len()],
        }
    }

    #[inline]
    fn update_xz(&mut self, block_x: i32, block_z: i32, gradient_x: i32, gradient_z: i32) {
        self.last_update_xz = self.last_update_xz.wrapping_add(1);
        self.last_update_y = self.last_update_y.wrapping_add(1);
        self.block_x = block_x;
        self.block_z = block_z;
        self.gradient_x = gradient_x;
        self.gradient_z = gradient_z;
        self.surface_depth = self
            .system
            .get_surface_depth(self.rs.root_random(), block_x, block_z);
    }

    #[inline]
    fn update_y(
        &mut self,
        stone_depth_above: i32,
        stone_depth_below: i32,
        water_height: i32,
        block_y: i32,
    ) {
        self.last_update_y = self.last_update_y.wrapping_add(1);
        self.cached_biome = None;
        self.block_y = block_y;
        self.water_height = water_height;
        self.stone_depth_below = stone_depth_below;
        self.stone_depth_above = stone_depth_above;
    }

    #[inline]
    fn biome(&mut self) -> BiomeId {
        if let Some(b) = self.cached_biome {
            return b;
        }
        let b = (self.get_biome)(self.block_x, self.block_y, self.block_z);
        self.cached_biome = Some(b);
        b
    }

    #[inline]
    fn surface_secondary(&mut self) -> f64 {
        if self.secondary_cache.0 != self.last_update_xz {
            let v = f64::from(self.system.surface_secondary_noise.sample(
                f64::from(self.block_x),
                0.0_f64,
                f64::from(self.block_z),
            ));
            self.secondary_cache = (self.last_update_xz, v);
        }
        self.secondary_cache.1
    }

    #[inline]
    fn get_min_surface_level(&mut self) -> i32 {
        if self.min_surface_level_cache.0 != self.last_update_xz {
            let prelim_val = if let Some(idx) =
                self.prelim_vol
                    .index_of_block(self.block_x, 0, self.block_z)
            {
                let buf = self.prelim_buf.get_or_insert_with(|| {
                    let mut b = vec![0.0_f32; self.prelim_vol.len()];
                    self.rs.sample_volume_with_context(
                        self.sampler_ctx,
                        RouterField::ChunkSurfaceLevel,
                        self.prelim_vol,
                        &mut b,
                    );
                    b
                });
                buf[idx]
            } else {
                self.rs.sample_router_with_context(
                    self.sampler_ctx,
                    RouterField::ChunkSurfaceLevel,
                    self.block_x,
                    0,
                    self.block_z,
                )
            };
            let min_sl = floor_f32(prelim_val)
                .wrapping_add(self.surface_depth)
                .wrapping_sub(8);
            self.min_surface_level_cache = (self.last_update_xz, min_sl);
        }
        self.min_surface_level_cache.1
    }

    #[inline]
    fn sample_noise_2d(&mut self, noise_idx: usize) -> f64 {
        let entry = &mut self.noise_2d_cache[noise_idx];
        if entry.0 != self.last_update_xz {
            let v = f64::from(self.system.noises_2d[noise_idx].sample(
                f64::from(self.block_x),
                0.0_f64,
                f64::from(self.block_z),
            ));
            *entry = (self.last_update_xz, v);
        }
        entry.1
    }

    #[inline]
    fn sample_noise_3d(&mut self, noise_idx: usize) -> f64 {
        let entry = &mut self.noise_3d_cache[noise_idx];
        if entry.0 != self.last_update_y {
            let v = f64::from(self.system.noises_3d[noise_idx].sample(
                f64::from(self.block_x),
                f64::from(self.block_y),
                f64::from(self.block_z),
            ));
            *entry = (self.last_update_y, v);
        }
        entry.1
    }
}

impl ActiveCondition<'_> {
    #[inline]
    fn test<F: FnMut(i32, i32, i32) -> BiomeId>(
        &self,
        ctx: &mut MaterialRuleContext<'_, F>,
    ) -> bool {
        match self {
            Self::Const(b) => *b,
            Self::Biome(set) => {
                let b = ctx.biome();
                set.contains(b)
            }
            Self::NoiseThreshold2D {
                noise_idx,
                min_threshold,
                max_threshold,
            } => {
                let v = ctx.sample_noise_2d(*noise_idx);
                v >= *min_threshold && v <= *max_threshold
            }
            Self::NoiseThreshold3D {
                noise_idx,
                min_threshold,
                max_threshold,
            } => {
                let v = ctx.sample_noise_3d(*noise_idx);
                v >= *min_threshold && v <= *max_threshold
            }
            Self::VerticalGradient { factory, lo, hi } => {
                let y = ctx.block_y;
                if y <= *lo {
                    true
                } else if y >= *hi {
                    false
                } else {
                    let p = map_f64(
                        f64::from(y),
                        f64::from(*lo),
                        f64::from(*hi),
                        1.0_f64,
                        0.0_f64,
                    );
                    let r = factory.at(ctx.block_x, y, ctx.block_z).next_float();
                    f64::from(r) < p
                }
            }
            Self::YAbove {
                anchor_y,
                surface_depth_multiplier,
                add_stone_depth,
            } => {
                let lhs = ctx.block_y.wrapping_add(if *add_stone_depth {
                    ctx.stone_depth_above
                } else {
                    0
                });
                let rhs = anchor_y
                    .wrapping_add(ctx.surface_depth.wrapping_mul(*surface_depth_multiplier));
                lhs >= rhs
            }
            Self::Water {
                offset,
                surface_depth_multiplier,
                add_stone_depth,
            } => {
                if ctx.water_height == i32::MIN {
                    return true;
                }
                let lhs = ctx.block_y.wrapping_add(if *add_stone_depth {
                    ctx.stone_depth_above
                } else {
                    0
                });
                let rhs = ctx
                    .water_height
                    .wrapping_add(*offset)
                    .wrapping_add(ctx.surface_depth.wrapping_mul(*surface_depth_multiplier));
                lhs >= rhs
            }
            Self::Temperature => {
                let bid = ctx.biome();
                ctx.biomes.data(bid).cold_enough_to_snow(
                    ctx.block_x,
                    ctx.block_y,
                    ctx.block_z,
                    ctx.system.sea_level,
                )
            }
            Self::Steep => ctx.gradient_x <= -4 || ctx.gradient_z >= 4,
            Self::Not(inner) => !inner.test(ctx),
            Self::Hole => ctx.surface_depth <= 0,
            Self::AbovePreliminarySurface => ctx.block_y >= ctx.get_min_surface_level(),
            Self::StoneDepth {
                offset,
                add_surface_depth,
                secondary_depth_range,
                surface_type,
            } => {
                let stone_depth = match surface_type {
                    CaveSurface::Floor => ctx.stone_depth_above,
                    CaveSurface::Ceiling => ctx.stone_depth_below,
                };
                let surf = if *add_surface_depth {
                    ctx.surface_depth
                } else {
                    0
                };
                let sec = if *secondary_depth_range == 0 {
                    0
                } else {
                    map_f64(
                        ctx.surface_secondary(),
                        -1.0_f64,
                        1.0_f64,
                        0.0_f64,
                        f64::from(*secondary_depth_range),
                    ) as i32
                };
                stone_depth
                    <= 1i32
                        .wrapping_add(*offset)
                        .wrapping_add(surf)
                        .wrapping_add(sec)
            }
        }
    }
}

impl ActiveRule<'_> {
    #[inline]
    fn try_apply<F: FnMut(i32, i32, i32) -> BiomeId>(
        &self,
        ctx: &mut MaterialRuleContext<'_, F>,
        block_x: i32,
        block_y: i32,
        block_z: i32,
    ) -> Option<u32> {
        match self {
            Self::Noop => None,
            Self::Block(state) => Some(*state),
            Self::Bandlands => Some(ctx.system.get_band(block_x, block_y, block_z)),
            Self::Sequence(children) => {
                for c in children {
                    if let Some(s) = c.try_apply(ctx, block_x, block_y, block_z) {
                        return Some(s);
                    }
                }
                None
            }
            Self::Condition { if_true, then_run } => {
                if if_true.test(ctx) {
                    then_run.try_apply(ctx, block_x, block_y, block_z)
                } else {
                    None
                }
            }
            Self::OreVein {
                ore_block,
                raw_ore_block,
                filler_block,
                raw_ore_chance,
                expected_volume,
                density_buf,
                richness_buf,
                density_sampler,
                richness_sampler,
                filler_gap_sampler,
                ore_random,
            } => {
                let idx_opt = expected_volume.index_of_block(block_x, block_y, block_z);
                let d = match idx_opt {
                    Some(i) => density_buf[i],
                    None => {
                        density_sampler.sample_value(ctx.sampler_ctx, block_x, block_y, block_z)
                    }
                };
                if d <= 0.0_f32 {
                    return None;
                }
                let mut rng = ore_random.at(block_x, block_y, block_z);
                if rng.next_float() > d {
                    return None;
                }
                let richness = match idx_opt {
                    Some(i) => richness_buf[i],
                    None => {
                        richness_sampler.sample_value(ctx.sampler_ctx, block_x, block_y, block_z)
                    }
                };
                if rng.next_float() < richness
                    && filler_gap_sampler.sample_value(ctx.sampler_ctx, block_x, block_y, block_z)
                        < 0.0_f32
                {
                    if rng.next_float() < *raw_ore_chance {
                        Some(*raw_ore_block)
                    } else {
                        Some(*ore_block)
                    }
                } else {
                    Some(*filler_block)
                }
            }
        }
    }
}
