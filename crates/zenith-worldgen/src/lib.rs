//! Terrain generation for Zenith (26.3): density functions, cubic splines, noise settings and router,
//! seeded random state, aquifers, heightmaps, `NoiseChunk`, and terrain fill.
//!
//! Implemented strictly from `docs/specs/worldgen/density-functions.md` and
//! `docs/specs/worldgen/aquifers-and-ore-veins.md` (clean room). Biome placement lives in
//! `zenith-biome`; material rules (including ore veins) and carvers follow in the next milestone.

pub mod aquifer;
pub mod ast;
pub mod beardifier;
pub mod blender;
pub mod codec;
pub mod compiler;
pub mod data;
pub mod generator;
pub mod heightmap;
pub mod interval;
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
pub use blender::Blender;
pub use compiler::{CompileError, CompiledSampler};
pub use data::{LoadError, WorldgenData, decode_density_function_str};
pub use generator::{
    AQUIFER_RANDOM_ID, NoiseChunk, NoiseGenerator, NoiseGeneratorError, PostProcessingMark,
    TerrainChunk, pack_offset_coordinates, unpack_offset_coordinates,
};
pub use heightmap::{
    BlockPredicates, BlockTags, Heightmap, HeightmapKind, TAG_BLOCKS_MOTION_IN_HEIGHTMAP,
    TAG_BLOCKS_MOTION_IN_HEIGHTMAP_NO_LEAVES, resolve_block_state,
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
