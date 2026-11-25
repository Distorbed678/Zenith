//! Biome data required by surface rules and carvers: climate temperature evaluation
//! (`coldEnoughToSnow`, `shouldMeltFrozenOceanIcebergSlightly`), biome carver lists,
//! and dense `BiomeId` / `BiomeBitSet` interning (`docs/specs/worldgen/surface-and-carvers.md` §1.8 & §3.2).

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, LazyLock};

use zenith_codec::codec::{Codec, DataResult, FloatCodec, IdentifierCodec, RecordReader};
use zenith_codec::json::Json;
use zenith_math::noise::{
    OctaveStack, SimplexNoise, biome_frozen_temperature_noise, biome_info_noise,
    biome_temperature_noise,
};
use zenith_registry::Identifier;

use crate::carver::{Carver, decode_carver};

static TEMPERATURE_NOISE: LazyLock<SimplexNoise> = LazyLock::new(biome_temperature_noise);
static BIOME_INFO_NOISE: LazyLock<SimplexNoise> = LazyLock::new(biome_info_noise);
static FROZEN_TEMPERATURE_NOISE: LazyLock<OctaveStack> =
    LazyLock::new(biome_frozen_temperature_noise);

/// Biome temperature modifier (`Biome.TemperatureModifier`, `surface-and-carvers.md` §1.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TemperatureModifier {
    /// No modification to `base_temperature`.
    #[default]
    None,
    /// Frozen ocean warm-pocket modulation using `FROZEN_TEMPERATURE_NOISE` and `BIOME_INFO_NOISE`.
    Frozen,
}

/// Raw unresolved carver entry in a biome's `"carvers"` holder set.
#[derive(Debug, Clone, PartialEq)]
pub enum RawCarverEntry {
    /// Reference to a registered carver ID in `minecraft:worldgen/carver`.
    Reference(Identifier),
    /// Reference to a carver tag `"#<id>"` in `tags/worldgen/carver`.
    Tag(Identifier),
    /// Inline carver object.
    Inline(Carver),
}

/// Raw biome terrain settings decoded from `worldgen/biome/<id>.json` before carver resolution.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawBiomeData {
    pub(crate) temperature: f32,
    pub(crate) temperature_modifier: TemperatureModifier,
    pub(crate) carvers: Vec<RawCarverEntry>,
}

/// Terrain-relevant fields decoded from `minecraft:worldgen/biome` (`surface-and-carvers.md`).
#[derive(Debug, Clone, PartialEq)]
pub struct BiomeData {
    /// Base biome temperature (`temperature` field in `worldgen/biome`).
    pub temperature: f32,
    /// Temperature modifier (`"none"` or `"frozen"`).
    pub temperature_modifier: TemperatureModifier,
    /// Ordered list of configured carvers attached to this biome (`carvers` field).
    pub carvers: Vec<Arc<Carver>>,
}

impl Default for BiomeData {
    fn default() -> Self {
        Self {
            temperature: 0.5_f32,
            temperature_modifier: TemperatureModifier::None,
            carvers: Vec::new(),
        }
    }
}

impl BiomeData {
    /// Evaluates `Biome.getHeightAdjustedTemperature(BlockPos(x, y, z), seaLevel)`
    /// (`surface-and-carvers.md` §1.8).
    #[inline]
    pub fn height_adjusted_temperature(&self, x: i32, y: i32, z: i32, sea_level: i32) -> f32 {
        let adjusted = match self.temperature_modifier {
            TemperatureModifier::None => self.temperature,
            TemperatureModifier::Frozen => {
                let large_var = f64::from(
                    FROZEN_TEMPERATURE_NOISE
                        .sample_2d(f64::from(x) * 0.05_f64, f64::from(z) * 0.05_f64)
                        * 7.0_f32,
                );
                let edge_var = f64::from(
                    BIOME_INFO_NOISE.sample_2d(f64::from(x) * 0.2_f64, f64::from(z) * 0.2_f64),
                );
                if large_var + edge_var < 0.3_f64 {
                    let small_var = f64::from(
                        BIOME_INFO_NOISE
                            .sample_2d(f64::from(x) * 0.09_f64, f64::from(z) * 0.09_f64),
                    );
                    if small_var < 0.8_f64 {
                        0.2_f32
                    } else {
                        self.temperature
                    }
                } else {
                    self.temperature
                }
            }
        };

        let snow_level = sea_level.wrapping_add(17);
        if y > snow_level {
            let v = TEMPERATURE_NOISE.sample_2d(
                f64::from((x as f32) / 8.0_f32),
                f64::from((z as f32) / 8.0_f32),
            ) * 8.0_f32;
            adjusted - (((v + (y as f32)) - (snow_level as f32)) * 0.05_f32) / 40.0_f32
        } else {
            adjusted
        }
    }

    /// Evaluates `Biome.coldEnoughToSnow(BlockPos(x, y, z), seaLevel)`: `!(temp >= 0.15_f32)`.
    #[inline]
    pub fn cold_enough_to_snow(&self, x: i32, y: i32, z: i32, sea_level: i32) -> bool {
        let t = self.height_adjusted_temperature(x, y, z, sea_level);
        t < 0.15_f32 || t.is_nan()
    }

    /// Evaluates `Biome.shouldMeltFrozenOceanIcebergSlightly(BlockPos(x, y, z), seaLevel)`:
    /// `temp > 0.1_f32`.
    #[inline]
    pub fn should_melt_frozen_ocean_iceberg_slightly(
        &self,
        x: i32,
        y: i32,
        z: i32,
        sea_level: i32,
    ) -> bool {
        self.height_adjusted_temperature(x, y, z, sea_level) > 0.1_f32
    }
}

/// Dense zero-based biome index inside a [`BiomeTable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BiomeId(pub u16);

/// Compact bitset of [`BiomeId`]s used for O(1) `BiomeCondition` checks and compile-time
/// `possibleBiomes` pruning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BiomeBitSet {
    words: Vec<u64>,
}

impl BiomeBitSet {
    /// Creates an empty bitset sized for `biome_count` biomes.
    pub fn new(biome_count: usize) -> Self {
        Self {
            words: vec![0u64; biome_count.div_ceil(64).max(1)],
        }
    }

    /// Inserts `id` into this set.
    #[inline]
    pub fn insert(&mut self, id: BiomeId) {
        let idx = id.0 as usize;
        let word = idx >> 6;
        if word >= self.words.len() {
            self.words.resize(word + 1, 0);
        }
        self.words[word] |= 1u64 << (idx & 63);
    }

    /// Returns `true` if `id` is present in this set.
    #[inline]
    pub fn contains(&self, id: BiomeId) -> bool {
        let idx = id.0 as usize;
        let word = idx >> 6;
        self.words
            .get(word)
            .is_some_and(|w| (w & (1u64 << (idx & 63))) != 0)
    }

    /// Returns `true` if `self` and `other` share at least one biome.
    #[inline]
    pub fn intersects(&self, other: &Self) -> bool {
        self.words
            .iter()
            .zip(&other.words)
            .any(|(a, b)| (a & b) != 0)
    }

    /// Returns `true` if every biome in `self` is also in `other` (`self ⊆ other`).
    #[inline]
    pub fn is_subset_of(&self, other: &Self) -> bool {
        for (i, &w) in self.words.iter().enumerate() {
            let ow = other.words.get(i).copied().unwrap_or(0);
            if (w & !ow) != 0 {
                return false;
            }
        }
        true
    }
}

/// Interned biome table for a dimension, mapping `Identifier <-> BiomeId` with O(1) data lookup.
#[derive(Debug, Clone)]
pub struct BiomeTable {
    ids: Vec<Identifier>,
    data: Vec<BiomeData>,
    by_name: HashMap<Identifier, BiomeId>,
    eroded_badlands: Option<BiomeId>,
    frozen_ocean: Option<BiomeId>,
    deep_frozen_ocean: Option<BiomeId>,
}

impl BiomeTable {
    /// Builds a [`BiomeTable`] from loaded `worldgen/biome` entries plus any extra identifiers
    /// (such as biomes mentioned by `BiomeSource` or `MaterialRule` in synthetic tests).
    pub fn new<'a>(
        biomes: &BTreeMap<Identifier, BiomeData>,
        extra_ids: impl IntoIterator<Item = &'a Identifier>,
    ) -> Self {
        let mut ids = Vec::with_capacity(biomes.len() + 8);
        let mut data = Vec::with_capacity(biomes.len() + 8);
        let mut by_name = HashMap::with_capacity(biomes.len() + 8);

        for (id, bd) in biomes {
            let bid = BiomeId(ids.len() as u16);
            by_name.insert(id.clone(), bid);
            ids.push(id.clone());
            data.push(bd.clone());
        }
        for id in extra_ids {
            if !by_name.contains_key(id) {
                let bid = BiomeId(ids.len() as u16);
                by_name.insert(id.clone(), bid);
                ids.push(id.clone());
                data.push(BiomeData::default());
            }
        }

        let eroded_badlands = by_name
            .get(&Identifier::with_default_namespace("eroded_badlands").expect("valid"))
            .copied();
        let frozen_ocean = by_name
            .get(&Identifier::with_default_namespace("frozen_ocean").expect("valid"))
            .copied();
        let deep_frozen_ocean = by_name
            .get(&Identifier::with_default_namespace("deep_frozen_ocean").expect("valid"))
            .copied();

        Self {
            ids,
            data,
            by_name,
            eroded_badlands,
            frozen_ocean,
            deep_frozen_ocean,
        }
    }

    /// Number of interned biomes.
    #[inline]
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// Returns `true` if no biomes are registered.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// Looks up the dense [`BiomeId`] for `id`.
    #[inline]
    pub fn get_id(&self, id: &Identifier) -> Option<BiomeId> {
        self.by_name.get(id).copied()
    }

    /// Returns the canonical identifier for `bid`.
    #[inline]
    pub fn identifier(&self, bid: BiomeId) -> &Identifier {
        &self.ids[bid.0 as usize]
    }

    /// Returns the [`BiomeData`] for `bid`.
    #[inline]
    pub fn data(&self, bid: BiomeId) -> &BiomeData {
        &self.data[bid.0 as usize]
    }

    /// Dense ID of `minecraft:eroded_badlands`, if present.
    #[inline]
    pub const fn eroded_badlands(&self) -> Option<BiomeId> {
        self.eroded_badlands
    }

    /// Dense ID of `minecraft:frozen_ocean`, if present.
    #[inline]
    pub const fn frozen_ocean(&self) -> Option<BiomeId> {
        self.frozen_ocean
    }

    /// Dense ID of `minecraft:deep_frozen_ocean`, if present.
    #[inline]
    pub const fn deep_frozen_ocean(&self) -> Option<BiomeId> {
        self.deep_frozen_ocean
    }
}

// ---------------------------------------------------------------------------
// JSON Codec for `worldgen/biome` terrain fields
// ---------------------------------------------------------------------------

struct TemperatureModifierCodec;
impl Codec<TemperatureModifier> for TemperatureModifierCodec {
    fn decode(&self, input: &Json) -> DataResult<TemperatureModifier> {
        let Some(s) = input.as_str() else {
            return DataResult::error(format!("Not a string: {}", input.type_name()));
        };
        match s {
            "none" => DataResult::Success(TemperatureModifier::None),
            "frozen" => DataResult::Success(TemperatureModifier::Frozen),
            other => DataResult::error(format!("Unknown temperature_modifier '{other}'")),
        }
    }
    fn encode(&self, _value: &TemperatureModifier) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

fn decode_carver_holder_entry(input: &Json) -> DataResult<RawCarverEntry> {
    if let Some(s) = input.as_str() {
        if let Some(tag_str) = s.strip_prefix('#') {
            return Identifier::parse(tag_str)
                .map(RawCarverEntry::Tag)
                .map_err(|e| format!("Invalid carver tag identifier '{s}': {e}"))
                .map_or_else(DataResult::error, DataResult::Success);
        }
        return IdentifierCodec.decode(input).map(RawCarverEntry::Reference);
    }
    match input {
        Json::Object(_) => decode_carver(input).map(RawCarverEntry::Inline),
        other => DataResult::error(format!(
            "Expected carver identifier, #tag, or inline object, got {}",
            other.type_name()
        )),
    }
}

struct CarverHolderSetCodec;
impl Codec<Vec<RawCarverEntry>> for CarverHolderSetCodec {
    fn decode(&self, input: &Json) -> DataResult<Vec<RawCarverEntry>> {
        match input {
            Json::String(_) | Json::Object(_) => decode_carver_holder_entry(input).map(|e| vec![e]),
            Json::Array(arr) => {
                let mut out = Vec::with_capacity(arr.len());
                let mut errs = Vec::new();
                for item in arr {
                    match decode_carver_holder_entry(item) {
                        DataResult::Success(e) => out.push(e),
                        DataResult::Error { message, .. } => errs.push(message),
                        DataResult::Failure(m) => return DataResult::Failure(m),
                    }
                }
                if errs.is_empty() {
                    DataResult::Success(out)
                } else {
                    DataResult::error(errs.join("; "))
                }
            }
            other => DataResult::error(format!(
                "Expected carver holder set (string, object, or array), got {}",
                other.type_name()
            )),
        }
    }
    fn encode(&self, _value: &Vec<RawCarverEntry>) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

pub(crate) fn decode_raw_biome_data(input: &Json) -> DataResult<RawBiomeData> {
    let mut r = RecordReader::new(input);
    let temperature = r.required("temperature", &FloatCodec);
    let temperature_modifier = r.optional(
        "temperature_modifier",
        &TemperatureModifierCodec,
        TemperatureModifier::None,
    );
    let carvers = r.optional("carvers", &CarverHolderSetCodec, Vec::new());
    r.finish(|| {
        Some(RawBiomeData {
            temperature: temperature?,
            temperature_modifier,
            carvers,
        })
    })
}
