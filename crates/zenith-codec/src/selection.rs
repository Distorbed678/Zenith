//! Pack selection and ordering (spec §4): `level.dat` pack lists, the world configuration algorithms, forced feature
//! packs, dedicated-server initial packs, `/reload`, and the client `options.txt` resource-pack lists.

use std::collections::HashSet;

use zenith_nbt::{Compound, Tag};
use zenith_registry::{FeatureFlagSet, FeatureUniverse, Identifier};

use crate::json::{Json, ParseMode};
use crate::log::{LogExt, LogSink};
use crate::pack::{Pack, PackRepository, Position};

// ----------------------------------------------------------------------------------------------------------
// level.dat (spec §4.1)
// ----------------------------------------------------------------------------------------------------------

/// The stored `Data.DataPacks` lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldPackConfig {
    /// Enabled pack ids, lowest priority first.
    pub enabled: Vec<String>,
    /// Disabled pack ids.
    pub disabled: Vec<String>,
}

impl Default for WorldPackConfig {
    fn default() -> Self {
        Self {
            enabled: vec!["vanilla".to_owned()],
            disabled: Vec::new(),
        }
    }
}

fn string_list(tag: Option<&Tag>) -> Option<Vec<String>> {
    let Tag::List(list) = tag? else { return None };
    list.iter()
        .map(|t| match t {
            Tag::String(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

/// Decodes `Data.DataPacks` **leniently**: a missing key, or any failure to decode (a missing `Enabled`/`Disabled`
/// list, a non-string element…), silently yields the default `{Enabled: ["vanilla"], Disabled: []}`.
pub fn decode_world_pack_config(data: &Compound) -> WorldPackConfig {
    let Some(Tag::Compound(packs)) = data.get("DataPacks") else {
        return WorldPackConfig::default();
    };
    match (
        string_list(packs.get("Enabled")),
        string_list(packs.get("Disabled")),
    ) {
        (Some(enabled), Some(disabled)) => WorldPackConfig { enabled, disabled },
        _ => WorldPackConfig::default(),
    }
}

/// Decodes `Data.enabled_features` **leniently** with default `[minecraft:vanilla]`: an unknown or invalid id, or
/// any other failure, makes the whole list fall back to the default (so other valid flags are lost); `[]` is valid
/// and stays empty.
pub fn decode_enabled_features(data: &Compound, universe: &FeatureUniverse) -> FeatureFlagSet {
    let default = universe.vanilla_set().unwrap_or_else(FeatureFlagSet::empty);
    let Some(ids) = string_list(data.get("enabled_features")) else {
        return default;
    };
    let mut parsed = Vec::with_capacity(ids.len());
    for s in &ids {
        match Identifier::parse(s) {
            Ok(id) => parsed.push(id),
            Err(_) => return default,
        }
    }
    universe.from_names(&parsed).unwrap_or(default)
}

/// Writes the pack lists as the `DataPacks` compound.
pub fn encode_world_pack_config(config: &WorldPackConfig) -> Compound {
    fn list(items: &[String]) -> Tag {
        let mut l = zenith_nbt::List::new();
        for i in items {
            l.push(Tag::String(i.as_str().into()));
        }
        Tag::List(l)
    }
    let mut c = Compound::new();
    c.insert("Enabled", list(&config.enabled));
    c.insert("Disabled", list(&config.disabled));
    c
}

// ----------------------------------------------------------------------------------------------------------
// Required packs (spec §4.5)
// ----------------------------------------------------------------------------------------------------------

/// Inserts every available pack marked **required** that is missing from `selection`, by default position, in
/// available order.
pub fn insert_required(repo: &PackRepository, selection: &mut Vec<String>) {
    for pack in repo.available() {
        if !pack.selection.required || selection.iter().any(|s| s == &pack.id) {
            continue;
        }
        let info = |id: &String| repo.get(id).map(|p| p.selection);
        match pack.selection.default_position {
            Position::Bottom => {
                // Before the first pack that is not (fixed with default BOTTOM).
                let at = selection
                    .iter()
                    .position(|id| {
                        !info(id).is_some_and(|c| c.fixed && c.default_position == Position::Bottom)
                    })
                    .unwrap_or(selection.len());
                selection.insert(at, pack.id.clone());
            }
            Position::Top => {
                // After the last pack that is not (fixed with default TOP).
                let at = selection
                    .iter()
                    .rposition(|id| {
                        !info(id).is_some_and(|c| c.fixed && c.default_position == Position::Top)
                    })
                    .map_or(0, |i| i + 1);
                selection.insert(at, pack.id.clone());
            }
        }
    }
}

// ----------------------------------------------------------------------------------------------------------
// World configuration (spec §4.2)
// ----------------------------------------------------------------------------------------------------------

/// Whether the world is being created or loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldMode {
    /// A new world: nothing is forced and every known flag is allowed.
    New,
    /// An existing world: stored `enabled_features` are both forced and allowed.
    Existing,
}

/// The outcome of configuring the pack repository for a world.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldPackSelection {
    /// The final selection (**Enabled**), lowest priority first.
    pub enabled: Vec<String>,
    /// Every available id not selected, in available order; **empty in safe mode**.
    pub disabled: Vec<String>,
    /// Union of every selected pack's requested features, plus the forced flags.
    pub enabled_features: FeatureFlagSet,
}

impl WorldPackSelection {
    /// The pack lists to store back into `level.dat`.
    pub fn stored_config(&self) -> WorldPackConfig {
        WorldPackConfig {
            enabled: self.enabled.clone(),
            disabled: self.disabled.clone(),
        }
    }
}

fn union_features(repo: &PackRepository, ids: &[String]) -> FeatureFlagSet {
    let mut acc = FeatureFlagSet::empty();
    for id in ids {
        if let Some(p) = repo.get(id)
            && let Ok(j) = acc.join(p.requested_features())
        {
            acc = j;
        }
    }
    acc
}

/// Configures the repository for a world: the new-world, existing-world and safe-mode algorithms of spec §4.2.
///
/// `stored` is the world's `DataPacks`; `stored_features` its `enabled_features` (used by existing worlds, and by
/// safe mode for an existing world).
pub fn configure_world_packs(
    repo: &PackRepository,
    stored: &WorldPackConfig,
    stored_features: &FeatureFlagSet,
    mode: WorldMode,
    safe_mode: bool,
    universe: &FeatureUniverse,
    log: &dyn LogSink,
) -> WorldPackSelection {
    let (forced, allowed) = match mode {
        WorldMode::New => (FeatureFlagSet::empty(), universe.all_flags()),
        WorldMode::Existing => (stored_features.clone(), stored_features.clone()),
    };

    let mut selection: Vec<String>;
    if safe_mode {
        selection = vec!["vanilla".to_owned()];
    } else {
        // Step 3: stored Enabled, in order, duplicates collapsed, unavailable ids skipped.
        let mut set: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for id in &stored.enabled {
            if repo.contains(id) {
                if seen.insert(id.clone()) {
                    set.push(id.clone());
                }
            } else {
                log.warn(format!("Missing data pack {id}"));
            }
        }
        // Step 4: walk the available packs that are not disabled.
        let disabled: HashSet<&str> = stored.disabled.iter().map(String::as_str).collect();
        for pack in repo.available() {
            if disabled.contains(pack.id.as_str()) {
                continue; // skipped entirely, including the feature check
            }
            let in_set = seen.contains(&pack.id);
            let fits = pack.requested_features().is_subset_of(&allowed);
            if !in_set {
                if pack.source.auto_add_when_new() {
                    if fits {
                        log.info(format!(
                            "Found new data pack {}, loading it automatically",
                            pack.id
                        ));
                        seen.insert(pack.id.clone());
                        set.push(pack.id.clone());
                    } else {
                        log.info(format!(
                            "Found new data pack {} but its required features are not enabled, not loading it",
                            pack.id
                        ));
                    }
                }
            } else if !fits {
                log.warn(format!(
                    "Pack {} requires features that are not enabled for this world, disabling it",
                    pack.id
                ));
                seen.remove(&pack.id);
                set.retain(|s| s != &pack.id);
            }
        }
        if set.is_empty() {
            set.push("vanilla".to_owned());
        }
        selection = set;
    }

    // Step 6: apply (inserting required packs), then force feature packs.
    selection.retain(|id| repo.contains(id));
    insert_required(repo, &mut selection);
    let covered = union_features(repo, &selection);
    let mut missing = forced
        .subtract(&covered)
        .unwrap_or_else(|_| FeatureFlagSet::empty());
    if !missing.is_empty() {
        for pack in repo.available() {
            if missing.is_empty() {
                break;
            }
            let req = pack.requested_features();
            if pack.source == crate::pack::PackSource::Feature
                && !req.is_empty()
                && req.intersects(&missing)
                && req.is_subset_of(&forced)
                && !selection.contains(&pack.id)
            {
                log.info(format!("Force-enabling feature pack {}", pack.id));
                selection.push(pack.id.clone());
                missing = missing
                    .subtract(req)
                    .unwrap_or_else(|_| FeatureFlagSet::empty());
            }
        }
    }

    let disabled = if safe_mode {
        Vec::new()
    } else {
        repo.ids()
            .into_iter()
            .filter(|id| !selection.contains(id))
            .collect()
    };
    let enabled_features = union_features(repo, &selection)
        .join(&forced)
        .unwrap_or_else(|_| FeatureFlagSet::empty());
    WorldPackSelection {
        enabled: selection,
        disabled,
        enabled_features,
    }
}

// ----------------------------------------------------------------------------------------------------------
// Dedicated servers (spec §4.3)
// ----------------------------------------------------------------------------------------------------------

/// Splits a comma-separated property: surrounding whitespace is trimmed and **empty items are kept**, so an empty
/// value gives a single `""` id.
pub fn split_pack_property(value: &str) -> Vec<String> {
    value.split(',').map(|s| s.trim().to_owned()).collect()
}

/// The initial pack lists of a new dedicated-server world from `server.properties`
/// (`initial-enabled-packs`, default `vanilla`; `initial-disabled-packs`, default empty).
pub fn server_initial_packs(
    initial_enabled: Option<&str>,
    initial_disabled: Option<&str>,
) -> WorldPackConfig {
    WorldPackConfig {
        enabled: split_pack_property(initial_enabled.unwrap_or("vanilla")),
        disabled: split_pack_property(initial_disabled.unwrap_or("")),
    }
}

// ----------------------------------------------------------------------------------------------------------
// /reload (spec §4.4)
// ----------------------------------------------------------------------------------------------------------

/// The selection `/reload` opens: the current selection in order, then every available id that is in neither the
/// current selection nor the world's stored Disabled list, in available order. There is **no source check and no
/// feature check**.
pub fn reload_selection(
    repo: &PackRepository,
    current: &[String],
    stored_disabled: &[String],
) -> Vec<String> {
    let mut out: Vec<String> = current.to_vec();
    for pack in repo.available() {
        if !out.contains(&pack.id) && !stored_disabled.contains(&pack.id) {
            out.push(pack.id.clone());
        }
    }
    insert_required(repo, &mut out);
    out
}

/// The pack lists stored after a successful `/reload`: Enabled = the selection, Disabled = every other available id.
pub fn config_after_reload(repo: &PackRepository, selection: &[String]) -> WorldPackConfig {
    WorldPackConfig {
        enabled: selection.to_vec(),
        disabled: repo
            .ids()
            .into_iter()
            .filter(|id| !selection.contains(id))
            .collect(),
    }
}

// ----------------------------------------------------------------------------------------------------------
// Client options.txt (spec §4.7)
// ----------------------------------------------------------------------------------------------------------

/// The two `options.txt` resource-pack lists.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClientPackOptions {
    /// `resourcePacks`.
    pub resource_packs: Vec<String>,
    /// `incompatibleResourcePacks`.
    pub incompatible_resource_packs: Vec<String>,
}

/// Parses an `options.txt` value holding a JSON array of ids; any problem gives an empty list.
pub fn parse_option_id_list(value: &str) -> Vec<String> {
    match crate::json::parse(value, ParseMode::Data) {
        Ok(Json::Array(items)) => items
            .iter()
            .map(|i| i.as_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Formats ids as a JSON array for `options.txt`.
pub fn format_option_id_list(ids: &[String]) -> String {
    Json::Array(ids.iter().map(|s| Json::string(s.as_str())).collect()).to_json_string()
}

/// Selects the client's resource packs at startup (spec §4.7), updating `options` as the game would.
///
/// Returns the selection, lowest priority first, with required packs inserted.
pub fn select_client_resource_packs(
    repo: &PackRepository,
    options: &mut ClientPackOptions,
    log: &dyn LogSink,
) -> Vec<String> {
    let mut selected: Vec<String> = Vec::new();
    let original = options.resource_packs.clone();
    for id in &original {
        // 1. Look the id up; legacy names get the `file/` prefix.
        let real: Option<&Pack> = repo.get(id).or_else(|| {
            if id.starts_with("file/") {
                None
            } else {
                repo.get(&format!("file/{id}"))
            }
        });
        let was_incompatible = options.incompatible_resource_packs.contains(id);
        match real {
            // 2. Unknown: drop it from the options.
            None => {
                log.warn(format!(
                    "Removed resource pack {id} from options because it doesn't seem to exist"
                ));
                options.resource_packs.retain(|p| p != id);
            }
            // 3. Not compatible and not remembered as incompatible: drop it.
            Some(p) if !p.is_compatible() && !was_incompatible => {
                log.warn(format!(
                    "Removed resource pack {id} from options because it is no longer compatible"
                ));
                options.resource_packs.retain(|x| x != id);
            }
            // 4. Compatible but remembered as incompatible: forget that, but do NOT select it this time.
            Some(p) if p.is_compatible() && was_incompatible => {
                log.info(format!("Removed resource pack {id} from incompatibility list because it's now compatible"));
                options.incompatible_resource_packs.retain(|x| x != id);
            }
            // 5. Otherwise select it by its real id.
            Some(p) => {
                if !selected.contains(&p.id) {
                    selected.push(p.id.clone());
                }
            }
        }
    }
    insert_required(repo, &mut selected);
    selected
}

/// The lists written to `options.txt` for a selection: fixed-position packs are left out of `resourcePacks`, and
/// every saved pack that is incompatible is also written to `incompatibleResourcePacks`.
pub fn client_options_for_selection(
    repo: &PackRepository,
    selection: &[String],
) -> ClientPackOptions {
    let mut out = ClientPackOptions::default();
    for id in selection {
        let Some(p) = repo.get(id) else { continue };
        if p.selection.fixed {
            continue;
        }
        out.resource_packs.push(id.clone());
        if !p.is_compatible() {
            out.incompatible_resource_packs.push(id.clone());
        }
    }
    out
}
