//! All 26.3 biome source types (`minecraft:fixed`, `minecraft:checkerboard`,
//! `minecraft:the_end`, `minecraft:multi_noise`), JSON codecs, and chunk section fill helpers.
//!
//! Implements Sections 2, 3, and 4 of `docs/specs/worldgen/biomes-and-climate.md`.

use std::collections::HashMap;

use zenith_codec::codec::{
    Codec, DataResult, IdentifierCodec, IntCodec, RecordReader, RecordWriter, decode_dispatch,
    sized_list, validated,
};
use zenith_codec::json::Json;
use zenith_math::helpers::packing::{block_to_section_coord, quart_to_block};
use zenith_math::helpers::rounding::java_floor_mod_i32;
use zenith_math::java::shr_i32;
use zenith_registry::Identifier;

use crate::climate::{ParameterPoint, ParameterPointCodec};
use crate::presets::{MultiNoisePreset, MultiNoisePresetCodec};
use crate::rtree::{ParameterList, SearchState};
use crate::sampler::{ClimateSampler, section_biome_index};

/// Maximum squared chunk distance from `(0, 0)` belonging to the central End island (`64^2 = 4096`).
pub const END_CENTRAL_ISLAND_RADIUS_SQ: i64 = 4_096;

/// Erosion cutoff (`> 0.25`) for `minecraft:end_highlands`.
pub const END_HIGHLANDS_THRESHOLD: f64 = 0.25_f64;

/// Erosion cutoff (`>= -0.0625`) for `minecraft:end_midlands`.
pub const END_MIDLANDS_THRESHOLD: f64 = -0.0625_f64;

/// Erosion cutoff (`< -0.21875`) for `minecraft:small_end_islands`.
pub const END_SMALL_ISLANDS_THRESHOLD: f64 = -0.21875_f64;

/// Default `scale` for `minecraft:checkerboard` (`2` -> `bitShift = 4`, 4-chunk tiles).
pub const CHECKERBOARD_DEFAULT_SCALE: i32 = 2;

/// Maximum valid `scale` for `minecraft:checkerboard` (`62`).
pub const CHECKERBOARD_MAX_SCALE: i32 = 62;

/// `minecraft:fixed` (`FixedBiomeSource`, spec §3.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedBiomeSource {
    /// The constant biome returned at every quart coordinate.
    pub biome: Identifier,
}

impl FixedBiomeSource {
    /// Creates a fixed biome source returning `biome`.
    pub fn new(biome: Identifier) -> Self {
        Self { biome }
    }

    /// Returns the constant biome without evaluating the climate sampler.
    #[inline]
    pub fn noise_biome(&self) -> Identifier {
        self.biome.clone()
    }
}

/// `minecraft:checkerboard` (`CheckerboardColumnBiomeSource`, spec §3.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckerboardBiomeSource {
    /// Ordered list of biomes in the holder set.
    pub biomes: Vec<Identifier>,
    /// Tile scale in `[0, 62]` (default `2`, giving `bitShift = scale + 2`).
    pub scale: i32,
}

impl CheckerboardBiomeSource {
    /// Creates a checkerboard biome source with the given `biomes` and `scale`.
    pub fn new(biomes: Vec<Identifier>, scale: i32) -> Self {
        Self { biomes, scale }
    }

    /// Samples the checkerboard biome at `(quart_x, _quart_y, quart_z)`.
    ///
    /// Matches Java's 32-bit shift masking (`(scale + 2) & 31`), wrapping 32-bit sum, and
    /// `Math.floorMod(..., biomes.len())`.
    ///
    /// # Panics
    ///
    /// Panics with division by zero if `self.biomes` is empty (matching Java).
    #[inline]
    pub fn noise_biome(&self, quart_x: i32, quart_z: i32) -> Identifier {
        let shift = self.scale.wrapping_add(2) as u32;
        let sum = shr_i32(quart_x, shift).wrapping_add(shr_i32(quart_z, shift));
        let idx = java_floor_mod_i32(sum, self.biomes.len() as i32) as usize;
        self.biomes[idx].clone()
    }
}

/// `minecraft:the_end` (`TheEndBiomeSource`, spec §3.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TheEndBiomeSource {
    /// `minecraft:the_end`
    pub end: Identifier,
    /// `minecraft:end_highlands`
    pub highlands: Identifier,
    /// `minecraft:end_midlands`
    pub midlands: Identifier,
    /// `minecraft:small_end_islands`
    pub small_islands: Identifier,
    /// `minecraft:end_barrens`
    pub barrens: Identifier,
}

impl Default for TheEndBiomeSource {
    fn default() -> Self {
        Self::new()
    }
}

impl TheEndBiomeSource {
    /// Constructs the vanilla End biome source with its five fixed biome identifiers.
    pub fn new() -> Self {
        Self {
            end: Identifier::with_default_namespace("the_end").expect("valid identifier"),
            highlands: Identifier::with_default_namespace("end_highlands")
                .expect("valid identifier"),
            midlands: Identifier::with_default_namespace("end_midlands").expect("valid identifier"),
            small_islands: Identifier::with_default_namespace("small_end_islands")
                .expect("valid identifier"),
            barrens: Identifier::with_default_namespace("end_barrens").expect("valid identifier"),
        }
    }

    /// Samples the End biome at `(quart_x, quart_y, quart_z)`, invoking `erosion(block_x, block_y, block_z)`
    /// ONLY when outside the central island (`chunkX^2 + chunkZ^2 > 4096`).
    #[inline]
    pub fn noise_biome_with_erosion(
        &self,
        quart_x: i32,
        quart_y: i32,
        quart_z: i32,
        erosion: impl FnOnce(i32, i32, i32) -> f32,
    ) -> Identifier {
        let block_x = quart_to_block(quart_x);
        let block_y = quart_to_block(quart_y);
        let block_z = quart_to_block(quart_z);
        let chunk_x = block_to_section_coord(block_x);
        let chunk_z = block_to_section_coord(block_z);
        let dist_sq = i64::from(chunk_x)
            .wrapping_mul(i64::from(chunk_x))
            .wrapping_add(i64::from(chunk_z).wrapping_mul(i64::from(chunk_z)));
        if dist_sq <= END_CENTRAL_ISLAND_RADIUS_SQ {
            return self.end.clone();
        }

        let sample_x = chunk_x.wrapping_mul(2).wrapping_add(1).wrapping_mul(8);
        let sample_z = chunk_z.wrapping_mul(2).wrapping_add(1).wrapping_mul(8);
        let h = f64::from(erosion(sample_x, block_y, sample_z));
        if h > END_HIGHLANDS_THRESHOLD {
            self.highlands.clone()
        } else if h >= END_MIDLANDS_THRESHOLD {
            self.midlands.clone()
        } else if h < END_SMALL_ISLANDS_THRESHOLD {
            self.small_islands.clone()
        } else {
            self.barrens.clone()
        }
    }

    /// Samples the End biome using [`ClimateSampler::sample_erosion_block`].
    #[inline]
    pub fn noise_biome(
        &self,
        quart_x: i32,
        quart_y: i32,
        quart_z: i32,
        sampler: &dyn ClimateSampler,
    ) -> Identifier {
        self.noise_biome_with_erosion(quart_x, quart_y, quart_z, |bx, by, bz| {
            sampler.sample_erosion_block(bx, by, bz)
        })
    }
}

/// Backing form of a `minecraft:multi_noise` biome source (inline list or preset).
#[derive(Debug, Clone)]
pub enum MultiNoiseEntries {
    /// Inline `"biomes"` list decoded directly from JSON.
    Inline(ParameterList<Identifier>),
    /// Built-in preset (referenced by registry ID or inline `{"preset": "..."}`).
    Preset {
        /// Registry ID if referenced by name, or `None` if decoded from an inline `{"preset": ...}` object.
        reference_id: Option<Identifier>,
        /// Resolved built-in preset.
        preset: MultiNoisePreset,
        /// Compiled 19-way R-tree for this preset.
        list: ParameterList<Identifier>,
    },
}

/// `minecraft:multi_noise` (`MultiNoiseBiomeSource`, spec §2).
#[derive(Debug, Clone)]
pub struct MultiNoiseBiomeSource {
    /// Either inline entries or a built-in preset.
    pub entries: MultiNoiseEntries,
}

impl MultiNoiseBiomeSource {
    /// Constructs a multi-noise source from an inline [`ParameterList`].
    pub fn from_list(list: ParameterList<Identifier>) -> Self {
        Self {
            entries: MultiNoiseEntries::Inline(list),
        }
    }

    /// Constructs a multi-noise source from a built-in [`MultiNoisePreset`].
    pub fn from_preset(preset: MultiNoisePreset) -> Self {
        Self {
            entries: MultiNoiseEntries::Preset {
                reference_id: Some(preset.id()),
                preset,
                list: preset.build_parameter_list(),
            },
        }
    }

    /// Returns the compiled [`ParameterList`] used for R-tree lookups.
    #[inline]
    pub fn parameter_list(&self) -> &ParameterList<Identifier> {
        match &self.entries {
            MultiNoiseEntries::Inline(list) => list,
            MultiNoiseEntries::Preset { list, .. } => list,
        }
    }

    /// Evaluates the climate sampler at `(quart_x, quart_y, quart_z)` and searches the R-tree
    /// with explicit warm-start `state`.
    #[inline]
    pub fn noise_biome(
        &self,
        quart_x: i32,
        quart_y: i32,
        quart_z: i32,
        sampler: &dyn ClimateSampler,
        state: &mut SearchState,
    ) -> Identifier {
        let target = sampler.sample(quart_x, quart_y, quart_z);
        self.parameter_list().find_value(target, state).clone()
    }
}

/// All four 26.3 biome sources registered in `minecraft:worldgen/biome_source`.
#[derive(Debug, Clone)]
pub enum BiomeSource {
    /// `minecraft:fixed`
    Fixed(FixedBiomeSource),
    /// `minecraft:checkerboard`
    Checkerboard(CheckerboardBiomeSource),
    /// `minecraft:the_end`
    TheEnd(TheEndBiomeSource),
    /// `minecraft:multi_noise`
    MultiNoise(MultiNoiseBiomeSource),
}

impl BiomeSource {
    /// Decodes a [`BiomeSource`] from a strict JSON value using default resolution
    /// (built-in presets `minecraft:overworld` and `minecraft:nether`, no tag bindings).
    pub fn decode_json(input: &Json) -> DataResult<Self> {
        BiomeSourceCodec::default().decode(input)
    }

    /// Encodes this [`BiomeSource`] to JSON (type-specific fields first, `"type"` last per
    /// `docs/specs/codec/datapacks-and-codecs.md` §7.4).
    pub fn encode_json(&self) -> DataResult<Json> {
        BiomeSourceCodec::default().encode(self)
    }

    /// Resolves the noise biome at quart coordinates `(quart_x, quart_y, quart_z)`.
    ///
    /// - For [`BiomeSource::MultiNoise`], calls `sampler.sample(quart_x, quart_y, quart_z)` and
    ///   searches the 19-way R-tree with warm-start `state`.
    /// - For [`BiomeSource::TheEnd`], tests `chunkX^2 + chunkZ^2 <= 4096` and, when outside the
    ///   central island, evaluates `sampler.sample_erosion_block((chunkX * 2 + 1) * 8, quart_y << 2, (chunkZ * 2 + 1) * 8)`.
    ///   Callers sampling The End should provide a [`ClimateSampler`] whose `sample_erosion_block`
    ///   returns the exact unquantized `f32` erosion channel (such as [`crate::PointClimate`] or
    ///   [`crate::EndErosionClimate`]).
    /// - For [`BiomeSource::Fixed`] and [`BiomeSource::Checkerboard`], `sampler` and `state` are
    ///   not touched.
    #[inline]
    pub fn noise_biome(
        &self,
        quart_x: i32,
        quart_y: i32,
        quart_z: i32,
        sampler: &dyn ClimateSampler,
        state: &mut SearchState,
    ) -> Identifier {
        match self {
            Self::Fixed(src) => src.noise_biome(),
            Self::Checkerboard(src) => src.noise_biome(quart_x, quart_z),
            Self::TheEnd(src) => src.noise_biome(quart_x, quart_y, quart_z, sampler),
            Self::MultiNoise(src) => src.noise_biome(quart_x, quart_y, quart_z, sampler, state),
        }
    }

    /// Fills one 4x4x4 chunk section's biome array in exact vanilla query order
    /// (**X outer, Y middle, Z inner**) and writes into section storage order
    /// (**Y major, Z middle, X contiguous**: `((y << 2) | z) << 2 | x`).
    ///
    /// Preserves exact `SearchState` warm-start progression across all 64 quarts (spec §4).
    pub fn fill_section_biomes(
        &self,
        min_quart_x: i32,
        quart_min_y: i32,
        min_quart_z: i32,
        sampler: &dyn ClimateSampler,
        state: &mut SearchState,
    ) -> [Identifier; 64] {
        let mut slots: [Option<Identifier>; 64] = std::array::from_fn(|_| None);
        for x in 0..4usize {
            let qx = min_quart_x.wrapping_add(x as i32);
            for y in 0..4usize {
                let qy = quart_min_y.wrapping_add(y as i32);
                for z in 0..4usize {
                    let qz = min_quart_z.wrapping_add(z as i32);
                    let biome = self.noise_biome(qx, qy, qz, sampler, state);
                    slots[section_biome_index(x, y, z)] = Some(biome);
                }
            }
        }
        slots.map(|b| b.expect("every slot in 4x4x4 section is filled"))
    }

    /// Fills all sections of a chunk at `(chunk_x, chunk_z)` with vertical bounds `(min_y, height)`
    /// in ascending `section_y` order (`min_section_y..=max_section_y`), matching the `BIOMES`
    /// chunk generation step (spec §4).
    ///
    /// For [`BiomeSource::MultiNoise`], pass a [`crate::ClimateVolume`] covering
    /// `(chunk_x << 2, min_y >> 2, chunk_z << 2)` with size `(4, height >> 2, 4)` to match
    /// `createResolverForChunk` (`sampleVolume`); for the other three biome sources, pass a point
    /// sampler.
    ///
    /// # Panics
    ///
    /// Panics if `min_y` or `height` is not a multiple of 16, or if `height <= 0`.
    pub fn fill_chunk_biomes(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        min_y: i32,
        height: i32,
        sampler: &dyn ClimateSampler,
        state: &mut SearchState,
    ) -> Vec<[Identifier; 64]> {
        assert!(
            min_y & 15 == 0 && height > 0 && height & 15 == 0,
            "chunk vertical bounds min_y={min_y}, height={height} must be multiples of 16 with height > 0"
        );
        let min_section_y = min_y >> 4;
        let section_count = (height >> 4) as usize;
        let min_quart_x = chunk_x << 2;
        let min_quart_z = chunk_z << 2;
        let mut sections = Vec::with_capacity(section_count);
        for i in 0..section_count {
            let section_y = min_section_y.wrapping_add(i as i32);
            let quart_min_y = section_y << 2;
            sections.push(self.fill_section_biomes(
                min_quart_x,
                quart_min_y,
                min_quart_z,
                sampler,
                state,
            ));
        }
        sections
    }

    /// Returns the distinct biome identifiers that this source can produce (in source order,
    /// preserving duplicates only if present in the underlying list).
    pub fn possible_biomes(&self) -> Vec<Identifier> {
        match self {
            Self::Fixed(src) => vec![src.biome.clone()],
            Self::Checkerboard(src) => src.biomes.clone(),
            Self::TheEnd(src) => vec![
                src.end.clone(),
                src.highlands.clone(),
                src.midlands.clone(),
                src.small_islands.clone(),
                src.barrens.clone(),
            ],
            Self::MultiNoise(src) => {
                let mut seen = Vec::new();
                for (_, id) in src.parameter_list().entries() {
                    if !seen.contains(id) {
                        seen.push(id.clone());
                    }
                }
                seen
            }
        }
    }
}

/// Configurable JSON codec for [`BiomeSource`].
///
/// Supports resolving:
/// - Biome tags (`"#ns:id"`) for `minecraft:checkerboard`'s `HolderSet<Biome>` `"biomes"` field.
/// - Custom registry entries in `minecraft:worldgen/multi_noise_biome_source_parameter_list`
///   mapping an arbitrary ID to a built-in [`MultiNoisePreset`].
#[derive(Debug, Clone, Default)]
pub struct BiomeSourceCodec {
    /// Bound biome tags (`tag_id -> ordered biome IDs`) for `checkerboard` `"#..."` holder sets.
    pub biome_tags: HashMap<Identifier, Vec<Identifier>>,
    /// Custom entries in `minecraft:worldgen/multi_noise_biome_source_parameter_list`.
    pub parameter_list_presets: HashMap<Identifier, MultiNoisePreset>,
}

impl BiomeSourceCodec {
    /// Creates a codec with no custom tags or custom preset registrations (only built-in
    /// `minecraft:overworld` and `minecraft:nether` presets).
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a biome tag binding for decoding `checkerboard` `"#ns:id"` holder sets.
    pub fn with_biome_tag(mut self, tag_id: Identifier, biomes: Vec<Identifier>) -> Self {
        self.biome_tags.insert(tag_id, biomes);
        self
    }

    /// Registers a custom `minecraft:worldgen/multi_noise_biome_source_parameter_list` entry.
    pub fn with_parameter_list_preset(mut self, id: Identifier, preset: MultiNoisePreset) -> Self {
        self.parameter_list_presets.insert(id, preset);
        self
    }

    fn resolve_preset_id(&self, id: &Identifier) -> Option<MultiNoisePreset> {
        self.parameter_list_presets
            .get(id)
            .copied()
            .or_else(|| MultiNoisePreset::from_id(id))
    }

    fn decode_fixed(&self, input: &Json) -> DataResult<BiomeSource> {
        let mut r = RecordReader::new(input);
        let biome = r.required("biome", &IdentifierCodec);
        r.finish(|| Some(BiomeSource::Fixed(FixedBiomeSource::new(biome?))))
    }

    fn decode_checkerboard(&self, input: &Json) -> DataResult<BiomeSource> {
        let scale_codec = validated(IntCodec, |v: &i32| {
            if (0..=CHECKERBOARD_MAX_SCALE).contains(v) {
                Ok(())
            } else {
                Err(format!(
                    "Value {v} outside of range [0:{CHECKERBOARD_MAX_SCALE}]"
                ))
            }
        });
        let holder_set_codec = BiomeHolderSetCodec {
            tags: &self.biome_tags,
        };
        let mut r = RecordReader::new(input);
        let biomes = r.required("biomes", &holder_set_codec);
        let scale = r.optional("scale", &scale_codec, CHECKERBOARD_DEFAULT_SCALE);
        r.finish(|| {
            Some(BiomeSource::Checkerboard(CheckerboardBiomeSource::new(
                biomes?, scale,
            )))
        })
    }

    fn decode_the_end(&self, input: &Json) -> DataResult<BiomeSource> {
        let r = RecordReader::new(input);
        r.finish(|| Some(BiomeSource::TheEnd(TheEndBiomeSource::new())))
    }

    fn decode_multi_noise(&self, input: &Json) -> DataResult<BiomeSource> {
        // Spec §2: mapEither(DIRECT, PRESET) — inline `"biomes"` is tried FIRST; if valid,
        // `"preset"` is ignored!
        let direct_err = match decode_multi_noise_direct(input) {
            DataResult::Success(list) => {
                return DataResult::Success(BiomeSource::MultiNoise(
                    MultiNoiseBiomeSource::from_list(list),
                ));
            }
            DataResult::Failure(m) => return DataResult::Failure(m),
            DataResult::Error { message, .. } => message,
        };

        match self.decode_multi_noise_preset(input) {
            DataResult::Success(src) => DataResult::Success(BiomeSource::MultiNoise(src)),
            DataResult::Failure(m) => DataResult::Failure(m),
            DataResult::Error { message, .. } => DataResult::error(format!(
                "Failed to parse either. First: {direct_err}; Second: {message}"
            )),
        }
    }

    fn decode_multi_noise_preset(&self, input: &Json) -> DataResult<MultiNoiseBiomeSource> {
        let preset_holder_codec = PresetHolderCodec { codec: self };
        let mut r = RecordReader::new(input);
        let entry = r.required("preset", &preset_holder_codec);
        r.finish(|| {
            let (reference_id, preset) = entry?;
            Some(MultiNoiseBiomeSource {
                entries: MultiNoiseEntries::Preset {
                    reference_id,
                    preset,
                    list: preset.build_parameter_list(),
                },
            })
        })
    }
}

impl Codec<BiomeSource> for BiomeSourceCodec {
    fn decode(&self, input: &Json) -> DataResult<BiomeSource> {
        decode_dispatch(input, "type", |ty, obj| {
            let id = Identifier::parse(ty).ok()?;
            if id.namespace() != "minecraft" {
                return None;
            }
            match id.path() {
                "fixed" => Some(self.decode_fixed(obj)),
                "checkerboard" => Some(self.decode_checkerboard(obj)),
                "the_end" => Some(self.decode_the_end(obj)),
                "multi_noise" => Some(self.decode_multi_noise(obj)),
                _ => None,
            }
        })
    }

    fn encode(&self, value: &BiomeSource) -> DataResult<Json> {
        // Per `docs/specs/codec/datapacks-and-codecs.md` §7.4: write type-specific fields first
        // and the `"type"` discriminator last.
        let mut w = RecordWriter::new();
        match value {
            BiomeSource::Fixed(src) => {
                w.field("biome", &IdentifierCodec, &src.biome);
                w.field(
                    "type",
                    &IdentifierCodec,
                    &Identifier::with_default_namespace("fixed").expect("valid"),
                );
            }
            BiomeSource::Checkerboard(src) => {
                let holder_set_codec = BiomeHolderSetCodec {
                    tags: &self.biome_tags,
                };
                w.field("biomes", &holder_set_codec, &src.biomes);
                w.field_default("scale", &IntCodec, &src.scale, &CHECKERBOARD_DEFAULT_SCALE);
                w.field(
                    "type",
                    &IdentifierCodec,
                    &Identifier::with_default_namespace("checkerboard").expect("valid"),
                );
            }
            BiomeSource::TheEnd(_) => {
                w.field(
                    "type",
                    &IdentifierCodec,
                    &Identifier::with_default_namespace("the_end").expect("valid"),
                );
            }
            BiomeSource::MultiNoise(src) => {
                match &src.entries {
                    MultiNoiseEntries::Inline(list) => {
                        let entries: Vec<(ParameterPoint, Identifier)> = list.entries().to_vec();
                        w.field(
                            "biomes",
                            &sized_list(BiomeEntryCodec, 1, usize::MAX),
                            &entries,
                        );
                    }
                    MultiNoiseEntries::Preset {
                        reference_id,
                        preset,
                        ..
                    } => {
                        if let Some(id) = reference_id {
                            w.field("preset", &IdentifierCodec, id);
                        } else {
                            w.field("preset", &MultiNoisePresetCodec, preset);
                        }
                    }
                }
                w.field(
                    "type",
                    &IdentifierCodec,
                    &Identifier::with_default_namespace("multi_noise").expect("valid"),
                );
            }
        }
        w.finish()
    }
}

struct BiomeEntryCodec;

impl Codec<(ParameterPoint, Identifier)> for BiomeEntryCodec {
    fn decode(&self, input: &Json) -> DataResult<(ParameterPoint, Identifier)> {
        let mut r = RecordReader::new(input);
        let parameters = r.required("parameters", &ParameterPointCodec);
        let biome = r.required("biome", &IdentifierCodec);
        r.finish(|| Some((parameters?, biome?)))
    }

    fn encode(&self, value: &(ParameterPoint, Identifier)) -> DataResult<Json> {
        let mut w = RecordWriter::new();
        w.field("parameters", &ParameterPointCodec, &value.0).field(
            "biome",
            &IdentifierCodec,
            &value.1,
        );
        w.finish()
    }
}

fn decode_multi_noise_direct(input: &Json) -> DataResult<ParameterList<Identifier>> {
    let mut r = RecordReader::new(input);
    let biomes = r.required("biomes", &sized_list(BiomeEntryCodec, 1, usize::MAX));
    r.finish(|| biomes).and_then(|entries| {
        ParameterList::new(entries)
            .map(DataResult::Success)
            .unwrap_or_else(|e| DataResult::error(e.to_string()))
    })
}

struct PresetHolderCodec<'a> {
    codec: &'a BiomeSourceCodec,
}

impl Codec<(Option<Identifier>, MultiNoisePreset)> for PresetHolderCodec<'_> {
    fn decode(&self, input: &Json) -> DataResult<(Option<Identifier>, MultiNoisePreset)> {
        // Form 1: registry reference ID string.
        if let Some(s) = input.as_str() {
            return match Identifier::parse(s) {
                Ok(id) => match self.codec.resolve_preset_id(&id) {
                    Some(preset) => DataResult::Success((Some(id), preset)),
                    None => DataResult::error(format!(
                        "Unknown multi-noise biome source parameter list: {id}"
                    )),
                },
                Err(e) => DataResult::error(format!("Not a valid resource location: {e}")),
            };
        }
        // Form 2: inline object `{"preset": "<builtin_preset_id>"}`.
        MultiNoisePresetCodec
            .decode(input)
            .map(|preset| (None, preset))
    }

    fn encode(&self, value: &(Option<Identifier>, MultiNoisePreset)) -> DataResult<Json> {
        match &value.0 {
            Some(id) => IdentifierCodec.encode(id),
            None => MultiNoisePresetCodec.encode(&value.1),
        }
    }
}

struct BiomeHolderSetCodec<'a> {
    tags: &'a HashMap<Identifier, Vec<Identifier>>,
}

impl Codec<Vec<Identifier>> for BiomeHolderSetCodec<'_> {
    fn decode(&self, input: &Json) -> DataResult<Vec<Identifier>> {
        // Form 1: string — either a tag `"#ns:id"` or a single biome ID `"ns:id"`.
        if let Some(s) = input.as_str() {
            if let Some(tag_str) = s.strip_prefix('#') {
                return match Identifier::parse(tag_str) {
                    Ok(tag_id) => match self.tags.get(&tag_id) {
                        Some(biomes) => DataResult::Success(biomes.clone()),
                        None => DataResult::error(format!("Unknown biome tag: #{tag_id}")),
                    },
                    Err(e) => DataResult::error(format!("Not a valid tag identifier: {e}")),
                };
            }
            return match Identifier::parse(s) {
                Ok(id) => DataResult::Success(vec![id]),
                Err(e) => DataResult::error(format!("Not a valid resource location: {e}")),
            };
        }
        // Form 2: JSON array of biome IDs.
        sized_list(IdentifierCodec, 0, usize::MAX).decode(input)
    }

    fn encode(&self, value: &Vec<Identifier>) -> DataResult<Json> {
        if value.len() == 1 {
            IdentifierCodec.encode(&value[0])
        } else {
            sized_list(IdentifierCodec, 0, usize::MAX).encode(value)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use zenith_codec::json::{ParseMode, parse};

    use super::*;
    use crate::climate::Parameter;
    use crate::sampler::{EndErosionClimate, PointClimate};

    #[test]
    fn fixed_biome_source_decode_sample_and_encode() {
        let j = parse(
            r#"{"type": "minecraft:fixed", "biome": "minecraft:plains"}"#,
            ParseMode::Data,
        )
        .unwrap();
        let src = BiomeSource::decode_json(&j).demand().unwrap();
        let sampler = PointClimate::new(|_, _, _| panic!("fixed source must not sample climate"));
        let mut state = SearchState::new();
        assert_eq!(
            src.noise_biome(10, -4, 99, &sampler, &mut state),
            Identifier::with_default_namespace("plains").unwrap()
        );
        let encoded = src.encode_json().demand().unwrap();
        let keys: Vec<&str> = encoded
            .as_object()
            .unwrap()
            .keys()
            .map(|k| k.as_str().unwrap())
            .collect();
        // Type-specific fields first, `"type"` last.
        assert_eq!(keys, vec!["biome", "type"]);
    }

    #[test]
    fn checkerboard_scale_default_negative_coords_and_32bit_shift_wrap() {
        let j = parse(
            r#"{"type": "minecraft:checkerboard", "biomes": ["minecraft:plains", "minecraft:desert", "minecraft:forest"]}"#,
            ParseMode::Data,
        )
        .unwrap();
        let src = BiomeSource::decode_json(&j).demand().unwrap();
        let sampler = PointClimate::new(|_, _, _| panic!("checkerboard must not sample climate"));
        let mut state = SearchState::new();
        // Default scale = 2 => bitShift = 4 (16-quart tiles).
        // quartX = -1 => -1 >> 4 = -1; quartZ = 0 => 0; floorMod(-1, 3) = 2 => forest.
        assert_eq!(
            src.noise_biome(-1, 50, 0, &sampler, &mut state),
            Identifier::with_default_namespace("forest").unwrap()
        );
        // Y is ignored.
        assert_eq!(
            src.noise_biome(16, -100, 0, &sampler, &mut state),
            Identifier::with_default_namespace("desert").unwrap()
        );

        // Spec edge case #11: scale = 30 => bitShift = 32 => masked to 0 in 32-bit Java shift!
        let j_wrap = parse(
            r#"{"type": "minecraft:checkerboard", "biomes": ["minecraft:plains", "minecraft:desert"], "scale": 30}"#,
            ParseMode::Data,
        )
        .unwrap();
        let src_wrap = BiomeSource::decode_json(&j_wrap).demand().unwrap();
        assert_eq!(
            src_wrap.noise_biome(0, 0, 0, &sampler, &mut state),
            Identifier::with_default_namespace("plains").unwrap()
        );
        assert_eq!(
            src_wrap.noise_biome(1, 0, 0, &sampler, &mut state),
            Identifier::with_default_namespace("desert").unwrap()
        );

        // scale = 63 rejected; tag resolution supported via BiomeSourceCodec.
        let bad_scale = parse(
            r#"{"type": "minecraft:checkerboard", "biomes": "minecraft:plains", "scale": 63}"#,
            ParseMode::Data,
        )
        .unwrap();
        assert!(BiomeSource::decode_json(&bad_scale).demand().is_err());

        let tag_json = parse(
            r##"{"type": "minecraft:checkerboard", "biomes": "#minecraft:test_tag", "scale": 0}"##,
            ParseMode::Data,
        )
        .unwrap();
        assert!(BiomeSource::decode_json(&tag_json).demand().is_err());
        let codec = BiomeSourceCodec::new().with_biome_tag(
            Identifier::with_default_namespace("test_tag").unwrap(),
            vec![
                Identifier::with_default_namespace("plains").unwrap(),
                Identifier::with_default_namespace("taiga").unwrap(),
            ],
        );
        let tagged = codec.decode(&tag_json).demand().unwrap();
        assert_eq!(
            tagged.noise_biome(4, 0, 0, &sampler, &mut state),
            Identifier::with_default_namespace("taiga").unwrap()
        );
    }

    #[test]
    fn the_end_central_island_and_exact_thresholds() {
        let j = parse(r#"{"type": "minecraft:the_end"}"#, ParseMode::Data).unwrap();
        let src = BiomeSource::decode_json(&j).demand().unwrap();
        let mut state = SearchState::new();

        // Central island: chunk (64, 0) => d^2 = 4096 <= 4096 => minecraft:the_end WITHOUT calling sampler!
        let panic_sampler =
            EndErosionClimate::new(|_, _, _| panic!("must not sample inside R^2<=4096"));
        assert_eq!(
            src.noise_biome(64 * 4, 10, 0, &panic_sampler, &mut state),
            Identifier::with_default_namespace("the_end").unwrap()
        );
        // Chunk (65, 0) => d^2 = 4225 > 4096 => samples at ((65*2 + 1)*8, quart_y*4, (0*2 + 1)*8) = (1048, 40, 8)
        let seen_pos = Cell::new((0, 0, 0));
        let mut check_threshold = |val: f32| {
            let s = EndErosionClimate::new(|bx, by, bz| {
                seen_pos.set((bx, by, bz));
                val
            });
            src.noise_biome(65 * 4 + 3, 10, 2, &s, &mut state)
        };

        assert_eq!(
            check_threshold(0.25001_f32),
            Identifier::with_default_namespace("end_highlands").unwrap()
        );
        assert_eq!(seen_pos.get(), (1048, 40, 8));
        assert_eq!(
            check_threshold(0.25_f32),
            Identifier::with_default_namespace("end_midlands").unwrap()
        );
        assert_eq!(
            check_threshold(-0.0625_f32),
            Identifier::with_default_namespace("end_midlands").unwrap()
        );
        assert_eq!(
            check_threshold(-0.0625001_f32),
            Identifier::with_default_namespace("end_barrens").unwrap()
        );
        assert_eq!(
            check_threshold(-0.21875_f32),
            Identifier::with_default_namespace("end_barrens").unwrap()
        );
        assert_eq!(
            check_threshold(-0.2187501_f32),
            Identifier::with_default_namespace("small_end_islands").unwrap()
        );
        assert_eq!(
            check_threshold(f32::NAN),
            Identifier::with_default_namespace("end_barrens").unwrap()
        );
    }

    #[test]
    fn multi_noise_prefers_inline_biomes_over_preset_and_decodes_presets() {
        // Spec edge case #7: both `biomes` and `preset` present => `biomes` wins!
        let both = parse(
            r#"{
                "type": "minecraft:multi_noise",
                "preset": "minecraft:nether",
                "biomes": [
                    {
                        "biome": "minecraft:plains",
                        "parameters": {
                            "temperature": 0.0, "humidity": 0.0, "continentalness": 0.0,
                            "erosion": 0.0, "depth": 0.0, "weirdness": 0.0, "offset": 0.0
                        }
                    }
                ]
            }"#,
            ParseMode::Data,
        )
        .unwrap();
        let src = BiomeSource::decode_json(&both).demand().unwrap();
        let sampler = PointClimate::new(|_, _, _| [0.0; 6]);
        let mut state = SearchState::new();
        assert_eq!(
            src.noise_biome(0, 0, 0, &sampler, &mut state),
            Identifier::with_default_namespace("plains").unwrap()
        );

        // Preset string form.
        let preset_str = parse(
            r#"{"type": "minecraft:multi_noise", "preset": "minecraft:nether"}"#,
            ParseMode::Data,
        )
        .unwrap();
        let nether = BiomeSource::decode_json(&preset_str).demand().unwrap();
        let crimson_sampler = PointClimate::new(|_, _, _| [0.4, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(
            nether.noise_biome(0, 0, 0, &crimson_sampler, &mut state),
            Identifier::with_default_namespace("crimson_forest").unwrap()
        );

        // Inline preset object form `{"preset": {"preset": "minecraft:nether"}}`.
        let preset_inline = parse(
            r#"{"type": "minecraft:multi_noise", "preset": {"preset": "minecraft:nether"}}"#,
            ParseMode::Data,
        )
        .unwrap();
        let nether2 = BiomeSource::decode_json(&preset_inline).demand().unwrap();
        assert_eq!(
            nether2.noise_biome(0, 0, 0, &crimson_sampler, &mut state),
            Identifier::with_default_namespace("crimson_forest").unwrap()
        );

        // Empty `biomes` array is rejected when no valid `preset` is provided.
        let empty = parse(
            r#"{"type": "minecraft:multi_noise", "biomes": []}"#,
            ParseMode::Data,
        )
        .unwrap();
        assert!(BiomeSource::decode_json(&empty).demand().is_err());
    }

    #[test]
    fn section_fill_uses_xyz_query_order_and_yzx_storage_order() {
        // Verify exact X-outer, Y-middle, Z-inner query order and Y-major, Z-middle, X-contiguous storage.
        let entries = vec![
            (
                ParameterPoint::new(
                    Parameter::point(-0.5),
                    Parameter::point(0.0),
                    Parameter::point(0.0),
                    Parameter::point(0.0),
                    Parameter::point(0.0),
                    Parameter::point(0.0),
                    0.0,
                ),
                Identifier::with_default_namespace("plains").unwrap(),
            ),
            (
                ParameterPoint::new(
                    Parameter::point(0.5),
                    Parameter::point(0.0),
                    Parameter::point(0.0),
                    Parameter::point(0.0),
                    Parameter::point(0.0),
                    Parameter::point(0.0),
                    0.0,
                ),
                Identifier::with_default_namespace("desert").unwrap(),
            ),
        ];
        let src = BiomeSource::MultiNoise(MultiNoiseBiomeSource::from_list(
            ParameterList::new(entries).unwrap(),
        ));
        let queried = std::cell::RefCell::new(Vec::new());
        let sampler = PointClimate::new(|bx, by, bz| {
            let (qx, qy, qz) = (bx >> 2, by >> 2, bz >> 2);
            queried.borrow_mut().push((qx, qy, qz));
            // Pick desert when (qx, qy, qz) == (1, 2, 3), plains otherwise.
            let t = if (qx, qy, qz) == (1, 2, 3) { 0.5 } else { -0.5 };
            [t, 0.0, 0.0, 0.0, 0.0, 0.0]
        });
        let mut state = SearchState::new();
        let section = src.fill_section_biomes(0, 0, 0, &sampler, &mut state);

        // First 5 queried quarts must be (0,0,0), (0,0,1), (0,0,2), (0,0,3), (0,1,0).
        let q = queried.borrow();
        assert_eq!(q.len(), 64);
        assert_eq!(
            &q[0..5],
            &[(0, 0, 0), (0, 0, 1), (0, 0, 2), (0, 0, 3), (0, 1, 0)]
        );
        // Stored at ((2 << 2) | 3) << 2 | 1 = (8 | 3) * 4 + 1 = 45.
        assert_eq!(
            section[45],
            Identifier::with_default_namespace("desert").unwrap()
        );
        assert_eq!(
            section[0],
            Identifier::with_default_namespace("plains").unwrap()
        );
    }

    #[test]
    fn chunk_volume_fill_and_codec_round_trips() {
        let src =
            BiomeSource::MultiNoise(MultiNoiseBiomeSource::from_preset(MultiNoisePreset::Nether));
        let vol =
            crate::sampler::ClimateVolume::from_channel_filler(0, 0, 0, 4, 8, 4, |ch, buf| {
                if ch == crate::sampler::ClimateChannel::Temperature {
                    buf.fill(0.4);
                }
            })
            .unwrap();
        let mut state = SearchState::new();
        let sections = src.fill_chunk_biomes(0, 0, 0, 32, &vol, &mut state);
        assert_eq!(sections.len(), 2);
        for sec in &sections {
            for b in sec {
                assert_eq!(
                    *b,
                    Identifier::with_default_namespace("crimson_forest").unwrap()
                );
            }
        }

        // Round-trip encode/decode all 4 biome source types, checking `"type"` is written last.
        for raw in [
            r#"{"type":"fixed","biome":"plains"}"#,
            r#"{"type":"minecraft:checkerboard","biomes":["minecraft:plains","minecraft:desert"],"scale":null}"#,
            r#"{"type":"minecraft:checkerboard","biomes":["minecraft:plains","minecraft:desert"],"scale":5}"#,
            r#"{"type":"minecraft:the_end"}"#,
            r#"{"type":"minecraft:multi_noise","preset":"minecraft:nether"}"#,
            r#"{"type":"minecraft:multi_noise","preset":{"preset":"minecraft:nether"}}"#,
        ] {
            let j = parse(raw, ParseMode::Data).unwrap();
            let decoded = BiomeSource::decode_json(&j).demand().unwrap();
            let encoded = decoded.encode_json().demand().unwrap();
            let obj = encoded.as_object().unwrap();
            let last_key = obj.keys().last().unwrap().as_str().unwrap();
            assert_eq!(last_key, "type", "type key must be last for {raw}");
            let _ = BiomeSource::decode_json(&encoded).demand().unwrap();
        }
    }
}
