//! Feature flags, bitset universes, filtered registry views, and data-pack gating.
//!
//! Implements Section 5 of `docs/specs/registry/identifiers-registries-tags.md`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use thiserror::Error;

use crate::holder::{Holder, NamedHolderSet};
use crate::identifier::{Identifier, ResourceKey, TagKey};
use crate::registry::Registry;

/// Hard limit on the number of feature flags per universe (`64`).
pub const MAX_FLAGS_PER_UNIVERSE: usize = 64;

/// Errors raised during feature-universe construction or set operations.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FeatureError {
    /// A universe exceeded the 64-flag limit.
    #[error("feature universe '{universe}' exceeded maximum of {MAX_FLAGS_PER_UNIVERSE} flags")]
    TooManyFlags {
        /// Name of the universe.
        universe: String,
    },

    /// Duplicate flag identifier within the same universe.
    #[error("duplicate feature flag '{id}' in universe '{universe}'")]
    DuplicateFlag {
        /// Name of the universe.
        universe: String,
        /// Duplicate flag identifier.
        id: Identifier,
    },

    /// Attempted to combine feature flag sets belonging to different universes.
    #[error("feature flag universe mismatch: '{left}' vs '{right}'")]
    UniverseMismatch {
        /// First universe name.
        left: String,
        /// Second universe name.
        right: String,
    },
}

/// Partial decode error returned when deserializing a feature flag list containing unknown IDs.
///
/// Carries both the unknown identifiers and the partial [`FeatureFlagSet`] of recognized flags.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unknown feature flags: {unknown:?}")]
pub struct PartialFeatureParseError {
    /// Identifiers that were not registered in the universe.
    pub unknown: Vec<Identifier>,
    /// The set of recognized flags that succeeded.
    pub partial: FeatureFlagSet,
}

/// One feature flag belonging to a named universe, occupying one bit (`0..64`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FeatureFlag {
    universe: Arc<str>,
    id: Identifier,
    bit_index: u8,
    mask: u64,
}

impl FeatureFlag {
    /// Returns the name of the owning universe.
    #[inline]
    pub fn universe(&self) -> &str {
        &self.universe
    }

    /// Returns this flag's identifier.
    #[inline]
    pub fn id(&self) -> &Identifier {
        &self.id
    }

    /// Returns this flag's bit index (`0..64`).
    #[inline]
    pub const fn bit_index(&self) -> u8 {
        self.bit_index
    }

    /// Returns the single-bit `u64` mask (`1u64 << bit_index`).
    #[inline]
    pub const fn mask(&self) -> u64 {
        self.mask
    }
}

/// A `(universe, u64 mask)` feature flag bitset.
///
/// The empty set always has `universe = None` and `mask = 0`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct FeatureFlagSet {
    universe: Option<Arc<str>>,
    mask: u64,
}

impl FeatureFlagSet {
    /// Returns the empty feature flag set (`universe = None`, `mask = 0`).
    #[inline]
    pub const fn empty() -> Self {
        Self {
            universe: None,
            mask: 0,
        }
    }

    /// Creates a singleton set containing `flag`.
    pub fn of(flag: &FeatureFlag) -> Self {
        Self {
            universe: Some(Arc::clone(&flag.universe)),
            mask: flag.mask,
        }
    }

    /// Creates a set from an iterator of flags, verifying they all share one universe.
    pub fn from_flags<'a>(
        flags: impl IntoIterator<Item = &'a FeatureFlag>,
    ) -> Result<Self, FeatureError> {
        let mut acc = Self::empty();
        for flag in flags {
            acc = acc.join(&Self::of(flag))?;
        }
        Ok(acc)
    }

    /// Returns `true` if this set contains no flags.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.mask == 0
    }

    /// Returns the universe name, or `None` for the empty set.
    #[inline]
    pub fn universe(&self) -> Option<&str> {
        self.universe.as_deref()
    }

    /// Returns the raw `u64` bitmask.
    #[inline]
    pub const fn mask(&self) -> u64 {
        self.mask
    }

    /// Membership test (`flag ∈ self`). Returns `false` if universes differ.
    pub fn contains(&self, flag: &FeatureFlag) -> bool {
        self.universe.as_deref() == Some(flag.universe()) && (self.mask & flag.mask) != 0
    }

    /// Subset test (`self ⊆ other`).
    ///
    /// Always `true` when `self` is empty; otherwise requires equal universes and
    /// `(self.mask & !other.mask) == 0`.
    pub fn is_subset_of(&self, other: &Self) -> bool {
        if self.is_empty() {
            return true;
        }
        self.universe == other.universe && (self.mask & !other.mask) == 0
    }

    /// Non-empty intersection test (`self ∩ other ≠ ∅`).
    pub fn intersects(&self, other: &Self) -> bool {
        !self.is_empty()
            && !other.is_empty()
            && self.universe == other.universe
            && (self.mask & other.mask) != 0
    }

    /// Union (`self ∪ other`). Fails if both are non-empty and belong to different universes.
    pub fn join(&self, other: &Self) -> Result<Self, FeatureError> {
        if self.is_empty() {
            return Ok(other.clone());
        }
        if other.is_empty() {
            return Ok(self.clone());
        }
        if self.universe != other.universe {
            return Err(FeatureError::UniverseMismatch {
                left: self.universe.as_deref().unwrap_or("").to_owned(),
                right: other.universe.as_deref().unwrap_or("").to_owned(),
            });
        }
        Ok(Self {
            universe: self.universe.clone(),
            mask: self.mask | other.mask,
        })
    }

    /// Difference (`self \ other`). Fails if both are non-empty and belong to different universes.
    pub fn subtract(&self, other: &Self) -> Result<Self, FeatureError> {
        if self.is_empty() || other.is_empty() {
            return Ok(self.clone());
        }
        if self.universe != other.universe {
            return Err(FeatureError::UniverseMismatch {
                left: self.universe.as_deref().unwrap_or("").to_owned(),
                right: other.universe.as_deref().unwrap_or("").to_owned(),
            });
        }
        let mask = self.mask & !other.mask;
        if mask == 0 {
            Ok(Self::empty())
        } else {
            Ok(Self {
                universe: self.universe.clone(),
                mask,
            })
        }
    }
}

/// Builder for a [`FeatureUniverse`].
#[derive(Debug, Clone)]
pub struct FeatureUniverseBuilder {
    name: Arc<str>,
    flags: Vec<FeatureFlag>,
    by_id: HashMap<Identifier, usize>,
}

impl FeatureUniverseBuilder {
    /// Starts building a feature universe with the given name.
    pub fn new(name: impl Into<Arc<str>>) -> Self {
        Self {
            name: name.into(),
            flags: Vec::new(),
            by_id: HashMap::new(),
        }
    }

    /// Adds a flag to the universe, assigning the next bit index (`0..64`).
    pub fn add_flag(&mut self, id: Identifier) -> Result<FeatureFlag, FeatureError> {
        if self.by_id.contains_key(&id) {
            return Err(FeatureError::DuplicateFlag {
                universe: self.name.to_string(),
                id,
            });
        }
        let bit_index = self.flags.len();
        if bit_index >= MAX_FLAGS_PER_UNIVERSE {
            return Err(FeatureError::TooManyFlags {
                universe: self.name.to_string(),
            });
        }
        let flag = FeatureFlag {
            universe: Arc::clone(&self.name),
            id: id.clone(),
            bit_index: bit_index as u8,
            mask: 1u64 << bit_index,
        };
        self.by_id.insert(id, bit_index);
        self.flags.push(flag.clone());
        Ok(flag)
    }

    /// Finishes building the immutable [`FeatureUniverse`].
    pub fn build(self) -> FeatureUniverse {
        let mut all_mask = 0u64;
        for f in &self.flags {
            all_mask |= f.mask;
        }
        let all_flags = if all_mask == 0 {
            FeatureFlagSet::empty()
        } else {
            FeatureFlagSet {
                universe: Some(Arc::clone(&self.name)),
                mask: all_mask,
            }
        };
        FeatureUniverse {
            name: self.name,
            flags: self.flags,
            by_id: self.by_id,
            all_flags,
        }
    }
}

/// A named feature-flag registry holding up to 64 flags.
#[derive(Debug, Clone)]
pub struct FeatureUniverse {
    name: Arc<str>,
    flags: Vec<FeatureFlag>,
    by_id: HashMap<Identifier, usize>,
    all_flags: FeatureFlagSet,
}

impl FeatureUniverse {
    /// Builds the 26.3 built-in `"main"` feature flag universe:
    /// - bit 0: `minecraft:vanilla`
    /// - bit 1: `minecraft:trade_rebalance`
    /// - bit 2: `minecraft:redstone_experiments`
    /// - bit 3: `minecraft:minecart_improvements`
    pub fn builtin_main() -> Self {
        let mut builder = FeatureUniverseBuilder::new("main");
        for name in [
            "vanilla",
            "trade_rebalance",
            "redstone_experiments",
            "minecart_improvements",
        ] {
            builder
                .add_flag(
                    Identifier::with_default_namespace(name)
                        .expect("built-in feature flag identifier is valid"),
                )
                .expect("built-in main universe has 4 unique flags");
        }
        builder.build()
    }

    /// Returns this universe's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns all flags in this universe in bit-index order.
    pub fn flags(&self) -> &[FeatureFlag] {
        &self.flags
    }

    /// Looks up a single flag by identifier.
    pub fn flag(&self, id: &Identifier) -> Option<&FeatureFlag> {
        self.by_id.get(id).map(|&idx| &self.flags[idx])
    }

    /// Returns a [`FeatureFlagSet`] containing every flag in this universe.
    pub fn all_flags(&self) -> FeatureFlagSet {
        self.all_flags.clone()
    }

    /// Returns the `{minecraft:vanilla}` singleton set if `minecraft:vanilla` exists in this universe.
    pub fn vanilla_set(&self) -> Option<FeatureFlagSet> {
        let id = Identifier::with_default_namespace("vanilla").ok()?;
        self.flag(&id).map(FeatureFlagSet::of)
    }

    /// Returns `true` if `set` is experimental, i.e., **not** a subset of `{minecraft:vanilla}`.
    pub fn is_experimental(&self, set: &FeatureFlagSet) -> bool {
        match self.vanilla_set() {
            Some(vanilla) => !set.is_subset_of(&vanilla),
            None => !set.is_empty(),
        }
    }

    /// Strict decoding from identifier names: if any identifier is unknown, returns a
    /// [`PartialFeatureParseError`] carrying both the unknown IDs and the partial set of
    /// recognized flags.
    pub fn from_names(
        &self,
        ids: &[Identifier],
    ) -> Result<FeatureFlagSet, PartialFeatureParseError> {
        let (set, unknown) = self.from_names_lenient(ids);
        if unknown.is_empty() {
            Ok(set)
        } else {
            Err(PartialFeatureParseError {
                unknown,
                partial: set,
            })
        }
    }

    /// Lenient decoding from identifier names: returns `(recognized_set, unknown_ids)`.
    pub fn from_names_lenient(&self, ids: &[Identifier]) -> (FeatureFlagSet, Vec<Identifier>) {
        let mut mask = 0u64;
        let mut unknown = Vec::new();
        for id in ids {
            if let Some(flag) = self.flag(id) {
                mask |= flag.mask;
            } else {
                unknown.push(id.clone());
            }
        }
        let set = if mask == 0 {
            FeatureFlagSet::empty()
        } else {
            FeatureFlagSet {
                universe: Some(Arc::clone(&self.name)),
                mask,
            }
        };
        (set, unknown)
    }

    /// Encodes a [`FeatureFlagSet`] into its sorted list of flag identifiers (in bit-index order).
    pub fn to_names(&self, set: &FeatureFlagSet) -> Vec<Identifier> {
        if set.is_empty() || set.universe.as_deref() != Some(self.name()) {
            return Vec::new();
        }
        self.flags
            .iter()
            .filter(|f| (set.mask & f.mask) != 0)
            .map(|f| f.id.clone())
            .collect()
    }
}

/// Trait implemented by registry values gated by required feature flags.
pub trait FeatureElement {
    /// Returns the feature flag set required for this element to be enabled.
    fn required_features(&self) -> &FeatureFlagSet;

    /// Returns `true` iff `required_features ⊆ enabled_features`.
    fn is_enabled(&self, enabled: &FeatureFlagSet) -> bool {
        self.required_features().is_subset_of(enabled)
    }
}

/// A read-only feature-filtered view over a [`Registry<T>`].
///
/// Per Section 5.4:
/// - Numeric IDs are **never shifted**.
/// - Single-key lookups and element iteration hide disabled elements.
/// - Tag lookups and tag listings remain **unfiltered**.
#[derive(Debug)]
pub struct FeatureFilteredRegistry<'a, T> {
    registry: &'a Registry<T>,
    enabled_features: FeatureFlagSet,
}

impl<'a, T: FeatureElement> FeatureFilteredRegistry<'a, T> {
    /// Creates a filtered view over `registry` using `enabled_features`.
    pub fn new(registry: &'a Registry<T>, enabled_features: FeatureFlagSet) -> Self {
        Self {
            registry,
            enabled_features,
        }
    }

    /// Returns the underlying unfiltered registry.
    pub fn underlying(&self) -> &'a Registry<T> {
        self.registry
    }

    /// Returns the active enabled feature flag set.
    pub fn enabled_features(&self) -> &FeatureFlagSet {
        &self.enabled_features
    }

    /// Single-key optional value lookup; hides disabled entries.
    pub fn get_optional(&self, id: &Identifier) -> Option<&'a Arc<T>> {
        self.registry
            .get_optional(id)
            .filter(|v| v.is_enabled(&self.enabled_features))
    }

    /// Single-key value lookup by resource key; hides disabled entries.
    pub fn get_value_by_key(&self, key: &ResourceKey) -> Option<&'a Arc<T>> {
        self.registry
            .get_value_by_key(key)
            .filter(|v| v.is_enabled(&self.enabled_features))
    }

    /// Single-key holder lookup by identifier; hides disabled entries.
    pub fn get_holder(&self, id: &Identifier) -> Option<Holder<T>> {
        self.registry.get_holder(id).filter(|h| {
            h.value()
                .map(|v| v.is_enabled(&self.enabled_features))
                .unwrap_or(false)
        })
    }

    /// Single-key holder lookup by resource key; hides disabled entries.
    pub fn get_holder_by_key(&self, key: &ResourceKey) -> Option<Holder<T>> {
        self.registry.get_holder_by_key(key).filter(|h| {
            h.value()
                .map(|v| v.is_enabled(&self.enabled_features))
                .unwrap_or(false)
        })
    }

    /// Single-key holder lookup by numeric ID (preserving original IDs); hides disabled entries.
    pub fn get_holder_by_id(&self, raw_id: i32) -> Option<Holder<T>> {
        self.registry.get_holder_by_id(raw_id).filter(|h| {
            h.value()
                .map(|v| v.is_enabled(&self.enabled_features))
                .unwrap_or(false)
        })
    }

    /// Iterates enabled elements in numeric ID order, preserving their original unmodified IDs.
    pub fn iter(&self) -> impl Iterator<Item = (u32, &'a Identifier, &'a Arc<T>)> + '_ {
        self.registry
            .iter()
            .filter(|(_, _, val)| val.is_enabled(&self.enabled_features))
    }

    /// Looks up a tag by [`TagKey`]. **Unfiltered** per Section 5.4.
    pub fn get_tag(&self, tag: &TagKey) -> Option<NamedHolderSet<T>> {
        self.registry.get_tag(tag)
    }

    /// Lists all tags in the registry. **Unfiltered** per Section 5.4.
    pub fn tags(&self) -> impl Iterator<Item = (&'a Identifier, NamedHolderSet<T>)> + '_ {
        self.registry.tags()
    }
}

/// Metadata describing a data pack's feature-flag requirements (from `pack.mcmeta`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPackFeatureInfo {
    /// Pack identifier (e.g. `"vanilla"`, `"trade_rebalance"`, `"file/my_pack.zip"`).
    pub id: String,
    /// Flags declared in `pack.mcmeta` under `"features": { "enabled": [...] }`.
    pub requested_features: FeatureFlagSet,
    /// Whether this pack is auto-enabled when its requested features are allowed.
    pub auto_enabled: bool,
    /// Whether this pack is a built-in feature-source pack capable of covering forced features.
    pub is_builtin_feature_source: bool,
}

/// Mode used during world data-pack feature configuration (Section 5.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackSelectionMode {
    /// Safe mode: selects only `"vanilla"` plus any built-in feature packs needed for `forced_features`.
    SafeMode {
        /// Forced feature flags that must be covered.
        forced_features: FeatureFlagSet,
    },
    /// New-world initialization: all flags in `all_features` are allowed, none forced.
    NewWorld {
        /// Full universe flag set.
        all_features: FeatureFlagSet,
    },
    /// Existing-world load: forced = allowed = persisted `enabled_features`.
    ExistingWorld {
        /// Persisted `enabled_features` from `level.dat`.
        persisted_enabled: FeatureFlagSet,
    },
}

/// Outcome of world data-pack selection with feature gating.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackSelectionResult {
    /// Final selected pack IDs.
    pub selected_packs: Vec<String>,
    /// Previously selected packs that were automatically disabled because they requested unenabled flags.
    pub auto_disabled_packs: Vec<String>,
    /// Resulting world `enabled_features` (union of selected packs' requested features and forced features).
    pub enabled_features: FeatureFlagSet,
}

/// Evaluates world data-pack selection and feature gating according to Section 5.4.
pub fn select_data_packs(
    available: &[DataPackFeatureInfo],
    previously_selected: &[String],
    mode: &PackSelectionMode,
) -> Result<PackSelectionResult, FeatureError> {
    let by_id: HashMap<&str, &DataPackFeatureInfo> =
        available.iter().map(|p| (p.id.as_str(), p)).collect();

    let (forced, allowed, safe_mode) = match mode {
        PackSelectionMode::SafeMode { forced_features } => {
            (forced_features.clone(), forced_features.clone(), true)
        }
        PackSelectionMode::NewWorld { all_features } => {
            (FeatureFlagSet::empty(), all_features.clone(), false)
        }
        PackSelectionMode::ExistingWorld { persisted_enabled } => {
            (persisted_enabled.clone(), persisted_enabled.clone(), false)
        }
    };

    let mut selected: Vec<String> = Vec::new();
    let mut selected_set: HashSet<String> = HashSet::new();
    let mut auto_disabled: Vec<String> = Vec::new();

    if safe_mode {
        if by_id.contains_key("vanilla") {
            selected.push("vanilla".to_owned());
            selected_set.insert("vanilla".to_owned());
        }
    } else {
        for pack_id in previously_selected {
            let Some(pack) = by_id.get(pack_id.as_str()) else {
                continue;
            };
            if pack.requested_features.is_subset_of(&allowed) {
                if selected_set.insert(pack.id.clone()) {
                    selected.push(pack.id.clone());
                }
            } else {
                auto_disabled.push(pack.id.clone());
            }
        }

        for pack in available {
            if pack.auto_enabled
                && pack.requested_features.is_subset_of(&allowed)
                && selected_set.insert(pack.id.clone())
            {
                selected.push(pack.id.clone());
            }
        }

        if selected.is_empty() && by_id.contains_key("vanilla") {
            selected.push("vanilla".to_owned());
            selected_set.insert("vanilla".to_owned());
        }
    }

    // Compute currently covered features from selected packs.
    let mut covered = FeatureFlagSet::empty();
    for pack_id in &selected {
        if let Some(pack) = by_id.get(pack_id.as_str()) {
            covered = covered.join(&pack.requested_features)?;
        }
    }

    // Force-enable built-in feature-source packs to cover any missing forced flags.
    let missing_forced = forced.subtract(&covered)?;
    if !missing_forced.is_empty() {
        for pack in available {
            if pack.is_builtin_feature_source
                && pack.requested_features.intersects(&missing_forced)
                && pack.requested_features.is_subset_of(&forced)
                && selected_set.insert(pack.id.clone())
            {
                selected.push(pack.id.clone());
                covered = covered.join(&pack.requested_features)?;
            }
        }
    }

    let enabled_features = covered.join(&forced)?;

    Ok(PackSelectionResult {
        selected_packs: selected,
        auto_disabled_packs: auto_disabled,
        enabled_features,
    })
}
