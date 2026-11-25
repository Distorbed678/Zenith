//! Runtime loader for per-item default component prototypes from the data-generator report folder
//! `reports/minecraft/components/item/*.json` (spec B.2).

use std::fs;
use std::path::{Path, PathBuf};

use zenith_codec::json::{ParseMode, parse_bytes};
use zenith_registry::Identifier;

use super::patch::ComponentMap;
use super::stack::PrototypeRegistry;
use crate::dynval::Dyn;
use crate::registry_context::RegistryContext;

/// Returns the cached `reports/minecraft/components/item` directory for a game version
/// (`~/.cache/zenith/versions/<version>/reports/minecraft/components/item`).
pub fn cached_item_components_dir(version: &str) -> PathBuf {
    zenith_data::version_dir(version)
        .join("reports")
        .join("minecraft")
        .join("components")
        .join("item")
}

/// Loads all per-item default component prototypes from a data-generator `components/item` directory.
///
/// Each `<name>.json` file holds `{"components": { ... }}` printed by the game's persistent codecs.
/// Returns a [`PrototypeRegistry`] binding `minecraft:<name>` to its decoded [`ComponentMap`].
pub fn load_prototypes_from_dir(
    item_dir: &Path,
    registries: &dyn RegistryContext,
) -> Result<PrototypeRegistry, String> {
    let entries = fs::read_dir(item_dir).map_err(|e| {
        format!(
            "Failed to read item components dir {}: {e}",
            item_dir.display()
        )
    })?;
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read dir entry: {e}"))?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("json") && path.is_file() {
            files.push(path);
        }
    }
    files.sort();

    let mut registry = PrototypeRegistry::new();
    for path in files {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| format!("Invalid item component filename: {}", path.display()))?;
        let item_id = Identifier::with_default_namespace(stem)
            .map_err(|e| format!("Invalid item identifier '{stem}': {e}"))?;
        let bytes =
            fs::read(&path).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
        let json = parse_bytes(&bytes, ParseMode::Data)
            .map_err(|e| format!("{}: invalid JSON: {e}", path.display()))?;
        let dyn_root = Dyn::from_json(&json);
        let Some(comps_dyn) = dyn_root.get("components") else {
            return Err(format!("{}: missing 'components' key", path.display()));
        };
        let map = ComponentMap::decode(comps_dyn, registries)
            .demand()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        registry.bind(item_id, map);
    }
    Ok(registry)
}

/// Loads per-item default prototypes for an ingested game version from Zenith's local cache.
pub fn load_prototypes_for_version(
    version: &str,
    registries: &dyn RegistryContext,
) -> Result<PrototypeRegistry, String> {
    load_prototypes_from_dir(&cached_item_components_dir(version), registries)
}
