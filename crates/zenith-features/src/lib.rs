//! Worldgen features, placement modifiers, height/block-state providers, and the `features` chunk
//! step for Zenith (26.3).
//!
//! Implemented strictly from `docs/specs/worldgen/features.md` (clean room).

pub mod block_ops;
pub mod data;
pub mod decorator;
pub mod fastutil;
pub mod features;
pub mod java_hash;
pub mod placement;
pub mod providers;
pub mod region;
pub mod rng;
pub mod sorter;
pub mod structure_place;

pub use block_ops::{BlockBitSet, BlockOps};
pub use data::{BiomeFeatureSettings, FeatureData, FeatureLoadError};
pub use decorator::{DECORATION_STEP_COUNT, FeatureDecorator, WORLDGEN_REGION_RANDOM_ID};
pub use features::{
    Feature, FeatureContext, FossilConfig, NoopStructureHook, NoopTemplateHook, NoopTreeHook,
    StructureInterleaveHook, TemplateConfig, TemplateFeatureHook, TreeFeatureHook,
    tree::{TreeConfig, TreeDecorator, VanillaTreeHook, place_tree},
};
pub use java_hash::JavaBlockPosSet;
pub use placement::{
    PlacedFeature, PlacedFeatureId, PlacementModifier, place_feature_with_modifiers,
};
pub use providers::{
    BlockPredicate, BlockReplacement, BlockStateProvider, Column, FloatProvider, FluidSet,
    HeightProvider, IntProvider, RuleTest, WeightedList,
};
pub use region::{FeatureBlockEntity, FeatureEntity, GenerationRegion, RegionChunk};
pub use rng::{BlockPos, Rotation, WorldgenRandom};
pub use sorter::{FeatureCycleError, StepFeatureData, build_features_per_step};
pub use structure_place::{
    PlaceFeatureEnv, RegionRandom, RegionStructureWorld, StartStore, StructurePlacementHook,
    StructureTemplateHook,
};
