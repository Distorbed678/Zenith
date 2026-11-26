//! Decoded `minecraft:tree` configuration (`docs/specs/worldgen/features-trees.md` §1, §3–§7).

use std::sync::Arc;

use zenith_blocks::Direction;

use crate::block_ops::BlockBitSet;
use crate::features::Feature;
use crate::providers::{BlockStateProvider, IntProvider};

/// Full `minecraft:tree` feature configuration.
#[derive(Debug, Clone)]
pub struct TreeConfig {
    /// Log provider for trunks and branches.
    pub trunk_provider: Arc<BlockStateProvider>,
    /// Trunk placer.
    pub trunk_placer: TrunkPlacer,
    /// Leaf provider.
    pub foliage_provider: Arc<BlockStateProvider>,
    /// Foliage placer.
    pub foliage_placer: FoliagePlacer,
    /// Optional root placer.
    pub root_placer: Option<RootPlacer>,
    /// Clearance profile.
    pub minimum_size: FeatureSize,
    /// Ordered tree decorators.
    pub decorators: Vec<TreeDecorator>,
    /// Whether vines are ignored by the clearance scan.
    pub ignore_vines: bool,
    /// Provider sampled (optionally) below each trunk base.
    pub below_trunk_provider: Arc<BlockStateProvider>,
}

/// `minimum_size` (`feature_size_type`).
#[derive(Debug, Clone)]
pub struct FeatureSize {
    /// Optional minimum clipped height.
    pub min_clipped_height: Option<i32>,
    /// Size layout.
    pub kind: FeatureSizeKind,
}

/// The two registered feature size layouts.
#[derive(Debug, Clone, Copy)]
pub enum FeatureSizeKind {
    /// `minecraft:two_layers_feature_size`.
    TwoLayers {
        limit: i32,
        lower_size: i32,
        upper_size: i32,
    },
    /// `minecraft:three_layers_feature_size`.
    ThreeLayers {
        limit: i32,
        upper_limit: i32,
        lower_size: i32,
        middle_size: i32,
        upper_size: i32,
    },
}

impl FeatureSize {
    /// `getSizeAtHeight(treeHeight, yo)`.
    #[inline]
    pub const fn size_at_height(&self, tree_height: i32, yo: i32) -> i32 {
        match self.kind {
            FeatureSizeKind::TwoLayers {
                limit,
                lower_size,
                upper_size,
            } => {
                if yo < limit {
                    lower_size
                } else {
                    upper_size
                }
            }
            FeatureSizeKind::ThreeLayers {
                limit,
                upper_limit,
                lower_size,
                middle_size,
                upper_size,
            } => {
                if yo < limit {
                    lower_size
                } else if yo >= tree_height - upper_limit {
                    upper_size
                } else {
                    middle_size
                }
            }
        }
    }
}

/// A trunk placer with its shared base-height fields.
#[derive(Debug, Clone)]
pub struct TrunkPlacer {
    /// `base_height`.
    pub base_height: i32,
    /// `height_rand_a`.
    pub height_rand_a: i32,
    /// `height_rand_b`.
    pub height_rand_b: i32,
    /// Placer-specific data.
    pub kind: TrunkKind,
}

/// The ten trunk placer types.
#[derive(Debug, Clone)]
pub enum TrunkKind {
    Straight,
    Forking,
    Giant,
    MegaJungle,
    DarkOak,
    Fancy,
    Bending {
        min_height_for_leaves: i32,
        bend_length: IntProvider,
    },
    UpwardsBranching {
        extra_branch_steps: IntProvider,
        place_branch_per_log_probability: f32,
        extra_branch_length: IntProvider,
        can_grow_through: BlockBitSet,
    },
    Cherry {
        branch_count: IntProvider,
        branch_horizontal_length: IntProvider,
        /// `(min_inclusive, max_inclusive)` of the first-branch offset.
        branch_start_offset_from_top: (i32, i32),
        branch_end_offset_from_top: IntProvider,
    },
    Poplar {
        trunk_height_above_branches: IntProvider,
        branch_amount: IntProvider,
    },
}

/// A foliage placer with its shared fields.
#[derive(Debug, Clone)]
pub struct FoliagePlacer {
    /// `radius`.
    pub radius: IntProvider,
    /// `offset`.
    pub offset: IntProvider,
    /// Placer-specific data.
    pub kind: FoliageKind,
}

/// The twelve foliage placer types.
#[derive(Debug, Clone)]
pub enum FoliageKind {
    Blob {
        height: i32,
    },
    Spruce {
        trunk_height: IntProvider,
    },
    Pine {
        height: IntProvider,
    },
    Acacia,
    Bush {
        height: i32,
    },
    Fancy {
        height: i32,
    },
    MegaJungle {
        height: i32,
    },
    MegaPine {
        crown_height: IntProvider,
    },
    DarkOak,
    RandomSpread {
        foliage_height: IntProvider,
        leaf_placement_attempts: i32,
    },
    Cherry {
        height: IntProvider,
        wide_bottom_layer_hole_chance: f32,
        corner_hole_chance: f32,
        hanging_leaves_chance: f32,
        hanging_leaves_extension_chance: f32,
    },
    Poplar {
        height: IntProvider,
        side_hole_chance: f32,
    },
}

/// `root_placer` (`mangrove_root_placer` only).
#[derive(Debug, Clone)]
pub struct RootPlacer {
    /// `trunk_offset_y`.
    pub trunk_offset_y: IntProvider,
    /// `root_provider`.
    pub root_provider: Arc<BlockStateProvider>,
    /// Optional block above each normal root.
    pub above_root: Option<AboveRootPlacement>,
    /// `mangrove_root_placement`.
    pub mangrove: MangroveRootPlacement,
}

/// `above_root_placement`.
#[derive(Debug, Clone)]
pub struct AboveRootPlacement {
    /// `above_root_provider`.
    pub provider: Arc<BlockStateProvider>,
    /// `above_root_placement_chance`.
    pub chance: f32,
}

/// `mangrove_root_placement`.
#[derive(Debug, Clone)]
pub struct MangroveRootPlacement {
    pub can_grow_through: BlockBitSet,
    pub muddy_roots_in: BlockBitSet,
    pub muddy_roots_provider: Arc<BlockStateProvider>,
    pub max_root_width: i32,
    pub max_root_length: i32,
    pub random_skew_chance: f32,
}

/// The eleven tree decorator types.
#[derive(Debug, Clone)]
pub enum TreeDecorator {
    TrunkVine,
    LeaveVine {
        probability: f32,
    },
    PaleMoss {
        leaves_probability: f32,
        trunk_probability: f32,
        ground_probability: f32,
        /// `minecraft:pale_moss_patch`, resolved at load time.
        patch_feature: Option<Arc<Feature>>,
    },
    CreakingHeart {
        probability: f32,
    },
    Cocoa {
        probability: f32,
    },
    ShelfMushroom {
        probability: f32,
    },
    Beehive {
        probability: f32,
    },
    AlterGround {
        provider: Arc<BlockStateProvider>,
    },
    AttachedToLeaves {
        probability: f32,
        exclusion_radius_xz: i32,
        exclusion_radius_y: i32,
        block_provider: Arc<BlockStateProvider>,
        required_empty_blocks: i32,
        directions: Vec<Direction>,
    },
    PlaceOnGround {
        tries: i32,
        radius: i32,
        height: i32,
        block_state_provider: Arc<BlockStateProvider>,
    },
    AttachedToLogs {
        probability: f32,
        block_provider: Arc<BlockStateProvider>,
        directions: Vec<Direction>,
    },
}
