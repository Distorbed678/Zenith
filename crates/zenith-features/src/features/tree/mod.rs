//! `minecraft:tree` (and the tree decorators shared with `fallen_tree`), implemented from
//! `docs/specs/worldgen/features-trees.md`.

mod config;
mod decorator;
mod foliage;
mod leaves;
mod place;
mod root;
mod shape;
mod trunk;

pub use config::{
    AboveRootPlacement, FeatureSize, FeatureSizeKind, FoliageKind, FoliagePlacer,
    MangroveRootPlacement, RootPlacer, TreeConfig, TreeDecorator, TrunkKind, TrunkPlacer,
};
pub use place::{VanillaTreeHook, place_tree};

use zenith_registry::Identifier;

use crate::block_ops::{BlockBitSet, BlockOps};

/// Update flags used by every tree setter (`UPDATE_NEIGHBORS | UPDATE_CLIENTS | UPDATE_KNOWN_SHAPE`).
pub(crate) const TREE_UPDATE_FLAGS: u32 = 19;

/// Block ids, tags and states used by the tree code, resolved once per [`BlockOps`].
#[derive(Debug)]
pub struct TreeCache {
    pub(crate) tag_logs: BlockBitSet,
    pub(crate) tag_prevents_nearby_leaf_decay: BlockBitSet,
    pub(crate) tag_snow: BlockBitSet,
    /// Default vine state (all faces `false`).
    pub(crate) sid_vine: u32,
    pub(crate) bid_shelf_mushroom: u32,
    pub(crate) sid_shelf_mushroom: u32,
    pub(crate) sid_pale_hanging_moss: u32,
    pub(crate) sid_creaking_heart: u32,
    pub(crate) sid_cocoa: u32,
    pub(crate) sid_bee_nest: u32,
}

impl TreeCache {
    pub(crate) fn new(ops: &BlockOps) -> Self {
        let tag = |name: &str| -> BlockBitSet {
            Identifier::with_default_namespace(name)
                .map(|id| ops.bitset_for_tag(&id))
                .unwrap_or_else(|_| BlockBitSet::new(ops.blocks.blocks().len()))
        };
        let block = |name: &str| -> (u32, u32) {
            Identifier::with_default_namespace(name)
                .ok()
                .and_then(|id| ops.blocks.block(&id))
                .map_or((u32::MAX, 0), |e| (e.block_protocol_id, e.default_state))
        };
        let (_, sid_vine) = block("vine");
        let (bid_shelf_mushroom, sid_shelf_mushroom) = block("shelf_mushroom");
        let (_, sid_pale_hanging_moss) = block("pale_hanging_moss");
        let (_, sid_creaking_heart) = block("creaking_heart");
        let (_, sid_cocoa) = block("cocoa");
        let (_, sid_bee_nest) = block("bee_nest");
        Self {
            tag_logs: tag("logs"),
            tag_prevents_nearby_leaf_decay: tag("prevents_nearby_leaf_decay"),
            tag_snow: tag("snow"),
            sid_vine,
            bid_shelf_mushroom,
            sid_shelf_mushroom,
            sid_pale_hanging_moss,
            sid_creaking_heart,
            sid_cocoa,
            sid_bee_nest,
        }
    }
}
