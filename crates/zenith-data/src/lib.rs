//! Versioned game tables, produced from the user's own install.
//!
//! Pipeline: prove ownership → run the game's built-in data generator from the user's jar with the user's
//! launcher-provided Java → compile its reports into compact [`Tables`] → cache them per version and jar hash in
//! `<cache>/versions/<version>/`. Nothing produced here is ever committed or uploaded.

pub mod ingest;
pub mod reports;
pub mod tables;
pub mod verify;

pub use tables::{Block, Packet, Property, Registry, RegistryEntry, StateRow, Tables, TablesMeta};

use std::path::PathBuf;

/// The cache directory for one game version.
pub fn version_dir(version: &str) -> PathBuf {
    zenith_install::cache_dir().join("versions").join(version)
}
