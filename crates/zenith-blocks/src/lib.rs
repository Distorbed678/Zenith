//! Per-block-state static facts and voxel shapes (clean-room implementation of `docs/specs/blocks/`).
//!
//! - [`shape`]: the voxel-shape model ([`VoxelShape`]): boxes in the spec's canonical order, joins, rotations, face
//!   shapes, sturdiness. It is independent of block data and reusable by collision / lighting code.
//! - [`table`]: the per-block table (class, shape family, colours, sounds, flags) parsed from the spec documents.
//! - [`BlockTransforms`]: the rotation / mirror table of every block state.
//! - [`BlockFacts`]: built from a `BlockStateTable` and the block tags, it holds every fact and shape per state.
//!
//! # Example
//! ```
//! use zenith_blocks::{Direction, SupportType, VoxelShape};
//! let slab = VoxelShape::px(0.0, 0.0, 0.0, 16.0, 8.0, 16.0);
//! assert!(slab.is_face_sturdy(Direction::Down, SupportType::Full));
//! assert!(!slab.is_face_sturdy(Direction::Up, SupportType::Full));
//! ```

mod facts;
mod families;
pub mod shape;
pub mod table;
mod transform;

pub use facts::{
    BlockFactTags, BlockFacts, BlockMeta, FluidKind, FluidState, ShapeId, ShapeKind, StateFacts,
    flag,
};
pub use shape::{Aabb, BoolOp, Direction, SupportType, Transform, VoxelShape};
pub use table::OffsetType;
pub use transform::{BlockTransforms, Mirror, Rotation, TransformRule};

use thiserror::Error;

/// Errors raised while building [`BlockFacts`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BuildError {
    /// A registry block has no row in the block table.
    #[error("block {0} has no row in docs/specs/blocks/block-table.md")]
    UnknownBlock(String),
    /// The table and the registry disagree on a block's state count or index.
    #[error("block {block}: table lists {table_states} states, the registry has {registry_states}")]
    StateCountMismatch {
        /// Block id.
        block: String,
        /// States according to the table.
        table_states: u32,
        /// States according to the registry.
        registry_states: u32,
    },
    /// A state lacks a property its shape family needs.
    #[error("block {block} has no property {property}")]
    MissingProperty {
        /// Block id.
        block: String,
        /// Property name.
        property: String,
    },
    /// A property has an unexpected value.
    #[error("block {block}: property {property} has unexpected value {value:?}")]
    BadValue {
        /// Block id.
        block: String,
        /// Property name.
        property: String,
        /// The value.
        value: String,
    },
    /// The table names a shape family this crate does not implement.
    #[error("block {block}: unknown shape family {family:?}")]
    UnknownFamily {
        /// Block id.
        block: String,
        /// Family key.
        family: String,
    },
    /// A table row names an unknown sound type or colour.
    #[error("unknown {what} {name:?}")]
    UnknownName {
        /// What kind of name.
        what: &'static str,
        /// The name.
        name: String,
    },
}
