//! Biome placement for Zenith (26.3): climate parameters and quantization, every biome source type,
//! the 19-way multi-noise R-tree, parameter-list presets, spawn finding, and Voronoi biome zoom.
//!
//! Implemented strictly from `docs/specs/worldgen/biomes-and-climate.md` (clean room). Climate
//! values come from [`ClimateSampler`] so this crate does not depend on `zenith-worldgen`.

pub mod climate;
pub mod presets;
pub mod rtree;
pub mod sampler;
pub mod source;
pub mod spawn;
pub mod zoom;

pub use climate::{
    ClimateError, OFFSET_JSON_MAX, OFFSET_JSON_MIN, PARAMETER_COUNT, PARAMETER_JSON_MAX,
    PARAMETER_JSON_MIN, Parameter, ParameterCodec, ParameterPoint, ParameterPointCodec,
    QUANTIZATION_FACTOR, TargetPoint, java_float_compare, quantize_coord, squared_distance,
    unquantize_coord,
};
pub use presets::{
    MultiNoisePreset, MultiNoisePresetCodec, NETHER_PRESET_ENTRY_COUNT,
    OVERWORLD_PRESET_ENTRY_COUNT, nether_parameter_list, nether_preset_entries,
    overworld_parameter_list, overworld_preset_entries,
};
pub use rtree::{CHILDREN_PER_NODE, ParameterList, ParameterListError, SearchState};
pub use sampler::{
    ClimateChannel, ClimateSampler, ClimateVolume, EndErosionClimate, PointClimate,
    section_biome_index,
};
pub use source::{
    BiomeSource, BiomeSourceCodec, CHECKERBOARD_DEFAULT_SCALE, CHECKERBOARD_MAX_SCALE,
    CheckerboardBiomeSource, END_CENTRAL_ISLAND_RADIUS_SQ, END_HIGHLANDS_THRESHOLD,
    END_MIDLANDS_THRESHOLD, END_SMALL_ISLANDS_THRESHOLD, FixedBiomeSource, MultiNoiseBiomeSource,
    MultiNoiseEntries, TheEndBiomeSource,
};
pub use spawn::{
    SPAWN_FITNESS_WEIGHT, SpawnTargetPoint, SpawnTargetPointCodec, find_spawn_chunk,
    overworld_spawn_targets,
};
pub use zoom::{fiddle, fiddled_distance, obfuscate_seed, zoomed_biome, zoomed_quart_pos};
