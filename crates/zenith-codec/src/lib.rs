//! Data packs, pack metadata, resource lookup across the pack stack, the codec framework, and the reload pipeline.
//!
//! Implemented strictly from `docs/specs/codec/datapacks-and-codecs.md` (clean room).
//!
//! * [`json`]: the strict JSON layer (BOM, duplicate keys, number text, `pack.mcmeta` mode).
//! * [`codec`]: decode/encode framework (no coercion, `null` = absent, partial results, error accumulation).
//! * [`format`], [`meta`], [`filter`], [`text`]: pack formats, `pack.mcmeta` and its sections.
//! * [`resources`], [`pack`]: pack contents (folder, zip, memory), discovery and the repository.
//! * [`selection`]: world, server, `/reload` and client pack selection.
//! * [`lookup`]: the pack stack and its lookups.
//! * [`reload`]: the ordered reload pipeline as data and hooks.

pub mod codec;
pub mod filter;
pub mod format;
pub mod json;
pub mod log;
pub mod lookup;
pub mod meta;
pub mod pack;
pub mod reload;
pub mod resources;
pub mod selection;
pub mod text;
