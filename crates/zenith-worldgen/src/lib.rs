//! Terrain generation for Zenith (26.3): density functions, cubic splines, noise settings and router, the
//! seeded random state, and (later) the noise chunk and terrain fill.
//!
//! Implemented strictly from `docs/specs/worldgen/density-functions.md` (clean room). Biome placement lives in
//! `zenith-biome`; aquifers, material rules and carvers follow in later milestones.
