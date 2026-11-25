//! Text components and item data components for Zenith (26.3).
//!
//! Implemented strictly from `docs/specs/codec/text-and-data-components.md` (clean room).
//!
//! * [`text`]: text component tree, styles, click/hover events, JSON/NBT codecs, translation,
//!   flattening/rendering, and trait-driven server-side / MOTD resolution.
//! * [`item`]: the 122 built-in item data component types, typed values, persistent JSON/NBT
//!   codecs, prototype maps and sanitized patches, item stacks/templates and validation, network
//!   and hashed stack encodings, and runtime per-item default loading.
//! * [`profile`]: player profile value shared by `profile` components and `object` player contents.
//! * [`nbt_path`]: command NBT-path parser and query engine.
//! * [`dynval`]: format-neutral value tree bridging `zenith-codec`'s JSON layer and `zenith-nbt`.
//! * [`registry_context`]: registry-membership lookup trait used during component decoding.

pub mod dynval;
pub mod grammar;
pub mod item;
pub mod nbt_path;
pub mod profile;
pub mod registry_context;
pub mod text;
