//! Terrain generation for Zenith (26.3): density functions, cubic splines, noise settings and router,
//! and the seeded random state.
//!
//! Implemented strictly from `docs/specs/worldgen/density-functions.md` (clean room). Biome placement
//! lives in `zenith-biome`; aquifers, material rules and carvers follow in later milestones.

pub mod ast;
pub mod codec;
pub mod compiler;
pub mod data;
pub mod interval;
pub mod random_state;
pub mod settings;
pub mod spline;
pub mod volume;

pub use ast::{
    Axis3D, BinaryOp, DensityFunction, DistanceMetric, GradientTiling, NoiseHolder, RoundOp,
    ShiftKind, UnaryOp,
};
pub use compiler::{CompileError, CompiledSampler};
pub use data::{LoadError, WorldgenData, decode_density_function_str};
pub use random_state::{PositionalFactory, RandomState};
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
