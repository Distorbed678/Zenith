//! Terrain generation for Zenith (26.3): density functions, cubic splines, noise settings and router,
//! seeded random state, aquifers, heightmaps, `NoiseChunk`, material rules (including ore veins),
//! special surface passes, cave/canyon carvers, and the unified `terrain` chunk step.
//!
//! Implemented strictly from `docs/specs/worldgen/density-functions.md`,
//! `docs/specs/worldgen/aquifers-and-ore-veins.md`, `docs/specs/worldgen/surface-and-carvers.md`,
//! and `docs/specs/worldgen/biomes-and-climate.md` (clean room).

pub mod aquifer;
pub mod ast;
pub mod beardifier;
pub mod biome_data;
pub mod blender;
pub mod carver;
pub mod codec;
pub mod compiler;
pub mod data;
pub mod generator;
pub mod heightmap;
pub mod interval;
pub mod material;
pub mod providers;
pub mod random_state;
pub mod settings;
pub mod spline;
pub mod volume;

pub use aquifer::{
    Aquifer, DisabledAquifer, FLOWING_UPDATE_SIMILARITY, FluidStatus, GLOBAL_LAVA_LEVEL,
    GlobalFluidPicker, NoiseBasedAquifer, SURFACE_SAMPLING_OFFSETS_IN_CHUNKS, WAY_ABOVE_MAX_Y,
    WAY_BELOW_MIN_Y,
};
pub use ast::{
    Axis3D, BinaryOp, DensityFunction, DistanceMetric, GradientTiling, NoiseHolder, RoundOp,
    ShiftKind, UnaryOp,
};
pub use beardifier::{Beardifier, BeardifierRigid, BoundingBox, JigsawJunction, TerrainAdjustment};
pub use biome_data::{BiomeBitSet, BiomeData, BiomeId, BiomeTable, TemperatureModifier};
pub use blender::Blender;
pub use carver::{
    CARVER_CHUNK_RADIUS, CARVER_MAX_DISTANCE, CARVER_PROTECTED_BLOCKS_ON_TOP, CARVER_RANGE,
    CanyonCarver, CanyonShape, Carver, CarvingMask, CaveCarver, decode_carver,
    set_large_feature_seed,
};
pub use compiler::{CompileError, CompiledSampler};
pub use data::{DimensionHeight, LoadError, WorldgenData, decode_density_function_str};
pub use generator::{
    AQUIFER_RANDOM_ID, NoiseChunk, NoiseGenerator, NoiseGeneratorError, PostProcessingMark,
    TerrainChunk, TerrainGenerator, TerrainGeneratorError, pack_offset_coordinates,
    unpack_offset_coordinates,
};
pub use heightmap::{
    BlockPredicates, BlockTags, Heightmap, HeightmapKind, TAG_BLOCKS_MOTION_IN_HEIGHTMAP,
    TAG_BLOCKS_MOTION_IN_HEIGHTMAP_NO_LEAVES, TAG_UNCARVABLE, resolve_block_state,
};
pub use material::{
    CLAY_BANDS_RANDOM_ID, CaveSurface, MaterialCondition, MaterialRule, ORE_RANDOM_ID,
};
pub use providers::{
    FloatProvider, HeightProvider, IntProvider, VerticalAnchor, WorldGenerationContext,
};
pub use random_state::{PositionalFactory, RandomState, SeededRandom};
pub use settings::{
    AquiferConfig, BlockStateSpec, CLIMATE_QUANTIZATION_FACTOR, ClimateParameter, DIMENSION_MAX_Y,
    DIMENSION_MIN_Y, DIMENSION_Y_SIZE, DebugFunctionEntry, MaterialRuleRef, NoiseGeneratorSettings,
    NoiseRouter, NoiseSettings, RouterField, SpawnTargetPoint,
};
pub use spline::CubicSpline;
pub use volume::{
    ALL_AXES, AXIS_X, AXIS_Y, AXIS_Z, BlenderHook, ContextSampler, DensityVolume, NO_AXES,
    SamplerContext,
};
