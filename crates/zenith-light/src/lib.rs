//! Block-light and sky-light engines, light data layers and the chunk light steps for Zenith
//! (clean-room implementation of `docs/specs/lighting/`).
//!
//! # Layers of the crate
//!
//! - [`pos`]: packed block / section nodes. [`fastutil`]: the exact iteration order of fastutil's
//!   `LongOpenHashSet` (the engine's pending-check set). [`layer`]: [`DataLayer`] nibble arrays.
//! - [`facts`]: per-state light inputs from `zenith-blocks`. [`sky_sources`]: the per-column lowest-source table.
//! - [`storage`]: section states, updating / visible layer maps, queued data. [`engine`]: the block and sky
//!   propagation engines. [`level`]: both engines, brightness helpers.
//! - [`world`]: [`LightWorld`], the deterministic server model (chunk steps, 1000-task batches, block changes,
//!   save / load / unload, change broadcast). [`client`]: [`ClientLightWorld`]. [`packet`]: light payloads.
//!
//! # Driving it
//!
//! ```ignore
//! let mut world = LightWorld::new(facts, LevelHeight { min_y: -64, height: 384 }, true);
//! world.light_chunks(vec![LitChunk::new(0, 0, terrain_chunk) /* … the neighbourhood … */]);
//! world.set_block(3, 70, 4, torch_state);
//! world.flush();
//! let (block, sky) = world.section_layers(0, 4, 0);
//! ```

pub mod change;
pub mod chunk;
pub mod client;
pub mod engine;
pub mod facts;
pub mod fastutil;
pub mod layer;
pub mod level;
pub mod packet;
pub mod pos;
pub mod queue;
pub mod sky_sources;
pub mod storage;
pub mod world;

pub use change::{BlockChange, ChangeMode, LightCalls, apply_block_change};
pub use chunk::{ChunkBlocks, DenseBlocks, LightChunk, LightChunkSource, LightStatus, LitChunk};
pub use client::ClientLightWorld;
pub use engine::{LevelHeight, LightEngine};
pub use facts::{LightFacts, merged_face_occludes};
pub use fastutil::LongOpenHashSet;
pub use layer::{BadLayerSize, DataLayer, LAYER_BYTES};
pub use level::{
    LevelLightEngine, can_see_sky, effective_sky_brightness, light_level_dependent_magic_value,
    max_local_raw_brightness, sky_darken_from_level,
};
pub use packet::{BitSet, LightUpdatePacketData, PacketError};
pub use queue::{QueueKind, TraceEntry};
pub use sky_sources::SkyLightSources;
pub use storage::{LayerRef, LayerStorage, LightLayer};
pub use world::{BATCH_SIZE, ChunkLightSave, ChunkMap, LightWorld, SectionLight};
