//! Registries, defaulted fallbacks, freezing, data-driven ID assignment, and sync assembly.
//!
//! Implements Section 2 of `docs/specs/registry/identifiers-registries-tags.md`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use thiserror::Error;

use crate::holder::{Holder, HolderError, NamedHolderSet, ReferenceHolder};
use crate::identifier::{Identifier, IdentifierError, ResourceKey, TagKey};

/// The 10 built-in defaulted registries in 26.3 and their default identifiers (Section 2.2).
pub const DEFAULTED_BUILTIN_REGISTRIES: &[(&str, &str)] = &[
    ("minecraft:game_event", "minecraft:step"),
    ("minecraft:fluid", "minecraft:empty"),
    ("minecraft:block", "minecraft:air"),
    ("minecraft:entity_type", "minecraft:pig"),
    ("minecraft:item", "minecraft:air"),
    ("minecraft:chunk_status", "minecraft:empty"),
    ("minecraft:villager_type", "minecraft:plains"),
    ("minecraft:villager_profession", "minecraft:none"),
    ("minecraft:memory_module_type", "minecraft:dummy"),
    ("minecraft:sensor_type", "minecraft:dummy"),
];

/// The 5 built-in registries that use intrusive holders (Section 2.3).
pub const INTRUSIVE_HOLDER_REGISTRIES: &[&str] = &[
    "minecraft:fluid",
    "minecraft:block",
    "minecraft:entity_type",
    "minecraft:item",
    "minecraft:block_entity_type",
];

/// The 7 built-in registries that support feature-flag filtering (Section 5.4).
pub const FEATURE_FILTERED_REGISTRIES: &[&str] = &[
    "minecraft:item",
    "minecraft:block",
    "minecraft:entity_type",
    "minecraft:game_rule",
    "minecraft:menu",
    "minecraft:potion",
    "minecraft:mob_effect",
];

/// The 32 synchronized dynamic registries in exact configuration-phase order (Section 2.5).
pub const SYNCHRONIZED_DYNAMIC_REGISTRIES: &[&str] = &[
    "minecraft:worldgen/biome",
    "minecraft:chat_type",
    "minecraft:trim_pattern",
    "minecraft:trim_material",
    "minecraft:wolf_variant",
    "minecraft:wolf_sound_variant",
    "minecraft:pig_variant",
    "minecraft:pig_sound_variant",
    "minecraft:frog_variant",
    "minecraft:cat_variant",
    "minecraft:cat_sound_variant",
    "minecraft:cow_sound_variant",
    "minecraft:cow_variant",
    "minecraft:chicken_sound_variant",
    "minecraft:chicken_variant",
    "minecraft:zombie_nautilus_variant",
    "minecraft:painting_variant",
    "minecraft:sulfur_cube_archetype",
    "minecraft:dimension_type",
    "minecraft:damage_type",
    "minecraft:banner_pattern",
    "minecraft:enchantment",
    "minecraft:jukebox_song",
    "minecraft:instrument",
    "minecraft:test_environment",
    "minecraft:test_instance",
    "minecraft:dialog",
    "minecraft:world_clock",
    "minecraft:timeline",
    "minecraft:decorated_pot_pattern",
    "minecraft:block_transformer",
    "minecraft:worldgen/block_state_provider",
];

/// The 13 synchronized dynamic registries validated to be non-empty (Section 2.4 & 2.5).
pub const NON_EMPTY_SYNCHRONIZED_REGISTRIES: &[&str] = &[
    "minecraft:wolf_variant",
    "minecraft:wolf_sound_variant",
    "minecraft:pig_variant",
    "minecraft:pig_sound_variant",
    "minecraft:frog_variant",
    "minecraft:cat_variant",
    "minecraft:cat_sound_variant",
    "minecraft:cow_variant",
    "minecraft:cow_sound_variant",
    "minecraft:chicken_variant",
    "minecraft:chicken_sound_variant",
    "minecraft:zombie_nautilus_variant",
    "minecraft:painting_variant",
];

/// The 8 reloadable dynamic registries (Section 2.4).
pub const RELOADABLE_REGISTRIES: &[&str] = &[
    "minecraft:loot_table",
    "minecraft:predicate",
    "minecraft:context_float_provider",
    "minecraft:context_int_provider",
    "minecraft:item_modifier",
    "minecraft:slot_source",
    "minecraft:advancement",
    "minecraft:recipe",
];

/// Known-pack provenance metadata `(namespace, id, version)` (Section 2.1 & 2.5).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KnownPack {
    /// Pack namespace (e.g. `"minecraft"`).
    pub namespace: String,
    /// Pack identifier (e.g. `"core"`).
    pub id: String,
    /// Pack version string (e.g. `"26.3"`).
    pub version: String,
}

impl KnownPack {
    /// Creates a new [`KnownPack`] provenance record.
    pub fn new(
        namespace: impl Into<String>,
        id: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        Self {
            namespace: namespace.into(),
            id: id.into(),
            version: version.into(),
        }
    }
}

/// Lifecycle stability marker for a registry or individual entry (Section 2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Lifecycle {
    /// Stable vanilla content.
    #[default]
    Stable,
    /// Experimental or non-vanilla content.
    Experimental,
}

impl Lifecycle {
    /// Combines two lifecycles: any [`Experimental`](Self::Experimental) makes the aggregate non-stable.
    #[inline]
    pub const fn combine(self, other: Self) -> Self {
        match (self, other) {
            (Self::Stable, Self::Stable) => Self::Stable,
            _ => Self::Experimental,
        }
    }
}

/// Per-entry registration metadata (Section 2.1 & 2.4).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RegistrationInfo {
    /// Optional known-pack provenance.
    pub known_pack: Option<KnownPack>,
    /// Entry lifecycle (`Stable` or `Experimental`).
    pub lifecycle: Lifecycle,
}

impl RegistrationInfo {
    /// Metadata for built-in static registrations (no known pack, `Stable`).
    pub const fn built_in() -> Self {
        Self {
            known_pack: None,
            lifecycle: Lifecycle::Stable,
        }
    }

    /// Metadata for data-pack registrations (Section 2.4.2.6):
    /// `Stable` iff the winning resource came from a known pack whose namespace is `"minecraft"`.
    pub fn from_pack(known_pack: Option<KnownPack>) -> Self {
        let lifecycle = match &known_pack {
            Some(pack) if pack.namespace == crate::identifier::DEFAULT_NAMESPACE => {
                Lifecycle::Stable
            }
            _ => Lifecycle::Experimental,
        };
        Self {
            known_pack,
            lifecycle,
        }
    }

    /// Metadata for network-synchronized client entries (Section 2.5.5):
    /// empty known-pack provenance and `Experimental` lifecycle.
    pub const fn network() -> Self {
        Self {
            known_pack: None,
            lifecycle: Lifecycle::Experimental,
        }
    }
}

impl Default for RegistrationInfo {
    fn default() -> Self {
        Self::built_in()
    }
}

/// Errors raised by registry registration, freezing, validation, and tag binding.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RegistryError {
    /// Registry is already frozen and rejects mutations.
    #[error("registry {registry} is frozen")]
    Frozen {
        /// Registry identifier.
        registry: Identifier,
    },

    /// Attempted to register an identifier that already exists in the registry.
    #[error("duplicate identifier {id} in registry {registry}")]
    DuplicateIdentifier {
        /// Registry identifier.
        registry: Identifier,
        /// Duplicate entry identifier.
        id: Identifier,
    },

    /// Attempted to register a value (by `Arc` object identity) that is already registered.
    #[error("duplicate value registered in registry {registry} under {id}")]
    DuplicateValue {
        /// Registry identifier.
        registry: Identifier,
        /// Identifier attempted for the duplicate value.
        id: Identifier,
    },

    /// Value was registered into an intrusive-holder registry without first pre-allocating a holder.
    #[error(
        "value registered under {id} in intrusive registry {registry} lacks an intrusive holder"
    )]
    MissingIntrusiveHolder {
        /// Registry identifier.
        registry: Identifier,
        /// Entry identifier.
        id: Identifier,
    },

    /// Intrusive-holder registries reject stand-alone placeholder holder requests.
    #[error("intrusive registry {registry} does not allow stand-alone placeholder holders")]
    IntrusiveRegistryRejectsPlaceholder {
        /// Registry identifier.
        registry: Identifier,
    },

    /// Intrusive-holder API called on a non-intrusive registry.
    #[error("registry {registry} does not use intrusive holders")]
    NotIntrusive {
        /// Registry identifier.
        registry: Identifier,
    },

    /// Freezing failed because one or more placeholder keys were never registered.
    #[error("registry {registry} has unbound keys at freeze time: {keys:?}")]
    UnboundKeys {
        /// Registry identifier.
        registry: Identifier,
        /// Unbound identifiers sorted in natural identifier order.
        keys: Vec<Identifier>,
    },

    /// Freezing failed because pre-allocated intrusive holders were never registered.
    #[error("registry {registry} has {count} unregistered intrusive holder(s) at freeze time")]
    UnregisteredIntrusiveHolders {
        /// Registry identifier.
        registry: Identifier,
        /// Count of unregistered intrusive holders.
        count: usize,
    },

    /// Freezing failed because the active tag table was already bound before freezing.
    #[error("registry {registry} had its active tag table bound before freezing")]
    TagsAlreadyBound {
        /// Registry identifier.
        registry: Identifier,
    },

    /// Freezing failed because one or more pre-created named tags were never bound.
    #[error("registry {registry} has unbound tags at freeze time: {tags:?}")]
    UnboundTags {
        /// Registry identifier.
        registry: Identifier,
        /// Unbound tag identifiers sorted in natural identifier order.
        tags: Vec<Identifier>,
    },

    /// A tag referenced a direct holder or a holder belonging to another registry.
    #[error("invalid holder in tag {tag} of registry {registry}: {source}")]
    InvalidTagHolder {
        /// Registry identifier.
        registry: Identifier,
        /// Tag identifier.
        tag: Identifier,
        /// Underlying holder error.
        #[source]
        source: Box<HolderError>,
    },

    /// A holder operation failed during registration.
    #[error(transparent)]
    Holder(#[from] HolderError),

    /// Post-freeze validation failed because a defaulted registry never registered its default key.
    #[error("defaulted registry {registry} is missing its default entry {default_id}")]
    MissingDefaultEntry {
        /// Registry identifier.
        registry: Identifier,
        /// Expected default identifier.
        default_id: Identifier,
    },

    /// Non-empty validator failed on an empty registry.
    #[error("registry {registry} must contain at least one entry")]
    EmptyRegistry {
        /// Registry identifier.
        registry: Identifier,
    },
}

/// Errors raised during data-driven file identifier ID assignment (Section 2.4).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DataDrivenIdError {
    /// A file identifier did not start with `<registry_path>/`.
    #[error("file identifier {file_id} does not start with expected prefix '{expected_prefix}'")]
    MissingRegistryPrefix {
        /// Offending file identifier.
        file_id: Identifier,
        /// Expected prefix (`<registry_path>/`).
        expected_prefix: String,
    },

    /// A file identifier did not end with `.json`.
    #[error("file identifier {file_id} does not end with '.json'")]
    MissingJsonSuffix {
        /// Offending file identifier.
        file_id: Identifier,
    },

    /// Duplicate file identifier in the input list.
    #[error("duplicate data-driven file identifier: {file_id}")]
    DuplicateFile {
        /// Duplicate file identifier.
        file_id: Identifier,
    },

    /// Stripped element identifier was invalid.
    #[error(transparent)]
    Identifier(#[from] IdentifierError),
}

struct RegisteredEntry<T> {
    id: u32,
    identifier: Identifier,
    resource_key: ResourceKey,
    value: Arc<T>,
    holder: ReferenceHolder<T>,
    info: RegistrationInfo,
}

/// A dense zero-based registry mapping between numeric IDs, identifiers, resource keys,
/// and runtime values (`Arc<T>` matched by pointer identity).
///
/// Supports optional default-entry fallback semantics (Section 2.2), intrusive holders
/// (Section 2.3), placeholder forward references, freezing validation, and tag binding.
pub struct Registry<T> {
    key: ResourceKey,
    lifecycle: Lifecycle,
    default_id: Option<Identifier>,
    default_entry_index: Option<usize>,
    intrusive: bool,
    frozen: bool,

    entries: Vec<RegisteredEntry<T>>,
    by_id: HashMap<Identifier, usize>,
    by_ptr: HashMap<usize, usize>,

    /// Placeholder reference holders created before registration (forward refs / dynamic tag refs).
    placeholders: HashMap<Identifier, ReferenceHolder<T>>,
    /// Pre-allocated intrusive holders awaiting registration, keyed by `Arc::as_ptr` as `usize`.
    unregistered_intrusive: HashMap<usize, ReferenceHolder<T>>,

    /// Named tags accumulated before freeze (or pre-created before tag binding).
    pending_tags: BTreeMap<Identifier, NamedHolderSet<T>>,
    /// Active published tags (populated on freeze or tag reload).
    active_tags: BTreeMap<Identifier, NamedHolderSet<T>>,
    /// Tracks whether `active_tags` was illegally bound prior to freezing.
    active_tags_bound: bool,
}

impl<T> fmt::Debug for Registry<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Registry")
            .field("key", &self.key)
            .field("lifecycle", &self.lifecycle)
            .field("default_id", &self.default_id)
            .field("intrusive", &self.intrusive)
            .field("frozen", &self.frozen)
            .field("len", &self.entries.len())
            .field("active_tag_count", &self.active_tags.len())
            .finish()
    }
}

impl<T> Registry<T> {
    /// Creates a new non-defaulted registry with `Lifecycle::Stable`.
    pub fn new(registry_id: Identifier) -> Self {
        Self::with_options(registry_id, Lifecycle::Stable, None, false)
    }

    /// Creates a new defaulted registry with the given `default_id`.
    pub fn new_defaulted(registry_id: Identifier, default_id: Identifier) -> Self {
        Self::with_options(registry_id, Lifecycle::Stable, Some(default_id), false)
    }

    /// Creates a new registry with explicit initial lifecycle, optional default identifier,
    /// and intrusive-holder mode.
    pub fn with_options(
        registry_id: Identifier,
        initial_lifecycle: Lifecycle,
        default_id: Option<Identifier>,
        intrusive: bool,
    ) -> Self {
        Self {
            key: ResourceKey::registry_key(registry_id),
            lifecycle: initial_lifecycle,
            default_id,
            default_entry_index: None,
            intrusive,
            frozen: false,
            entries: Vec::new(),
            by_id: HashMap::new(),
            by_ptr: HashMap::new(),
            placeholders: HashMap::new(),
            unregistered_intrusive: HashMap::new(),
            pending_tags: BTreeMap::new(),
            active_tags: BTreeMap::new(),
            active_tags_bound: false,
        }
    }

    /// Returns this registry's [`ResourceKey`] (`(minecraft:root, <registry_id>)`).
    #[inline]
    pub fn key(&self) -> &ResourceKey {
        &self.key
    }

    /// Returns this registry's aggregate [`Lifecycle`].
    #[inline]
    pub const fn lifecycle(&self) -> Lifecycle {
        self.lifecycle
    }

    /// Returns the default identifier if this is a defaulted registry.
    #[inline]
    pub fn default_id(&self) -> Option<&Identifier> {
        self.default_id.as_ref()
    }

    /// Returns `true` if this is a defaulted registry.
    #[inline]
    pub const fn is_defaulted(&self) -> bool {
        self.default_id.is_some()
    }

    /// Returns `true` if this registry uses intrusive holders.
    #[inline]
    pub const fn is_intrusive(&self) -> bool {
        self.intrusive
    }

    /// Returns `true` if this registry has been frozen.
    #[inline]
    pub const fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// Returns the number of registered entries.
    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if zero entries are registered.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Pre-allocates an intrusive reference holder for `value` before registration (Section 2.3).
    pub fn create_intrusive_holder(&mut self, value: Arc<T>) -> Result<Holder<T>, RegistryError> {
        if !self.intrusive {
            return Err(RegistryError::NotIntrusive {
                registry: self.key.location().clone(),
            });
        }
        if self.frozen {
            return Err(RegistryError::Frozen {
                registry: self.key.location().clone(),
            });
        }
        let ptr = Arc::as_ptr(&value) as usize;
        if let Some(existing) = self.unregistered_intrusive.get(&ptr) {
            return Ok(Holder::Reference(existing.clone()));
        }
        if let Some(&idx) = self.by_ptr.get(&ptr) {
            return Ok(Holder::Reference(self.entries[idx].holder.clone()));
        }
        let holder = ReferenceHolder::new_intrusive(self.key.clone(), value);
        self.unregistered_intrusive.insert(ptr, holder.clone());
        Ok(Holder::Reference(holder))
    }

    /// Returns an existing reference holder for `id`, or creates a stand-alone unbound placeholder
    /// holder if this registry is writable and non-intrusive (Section 2.3 & 4.4).
    pub fn get_or_create_placeholder(
        &mut self,
        id: &Identifier,
    ) -> Result<Holder<T>, RegistryError> {
        if let Some(&idx) = self.by_id.get(id) {
            return Ok(Holder::Reference(self.entries[idx].holder.clone()));
        }
        if self.intrusive {
            return Err(RegistryError::IntrusiveRegistryRejectsPlaceholder {
                registry: self.key.location().clone(),
            });
        }
        if self.frozen {
            return Err(RegistryError::Frozen {
                registry: self.key.location().clone(),
            });
        }
        let holder = self
            .placeholders
            .entry(id.clone())
            .or_insert_with(|| {
                ReferenceHolder::new_placeholder(
                    self.key.clone(),
                    ResourceKey::for_registry(&self.key, id.clone()),
                )
            })
            .clone();
        Ok(Holder::Reference(holder))
    }

    /// Registers a new value under `id` with `info`, wrapping `value` in an `Arc`.
    pub fn register(
        &mut self,
        id: Identifier,
        value: T,
        info: RegistrationInfo,
    ) -> Result<Holder<T>, RegistryError> {
        self.register_arc(id, Arc::new(value), info)
    }

    /// Registers a new `Arc<T>` under `id` with `info`.
    ///
    /// Assigns dense sequential numeric IDs starting from `0` and rejects duplicate
    /// identifiers, duplicate `Arc` object identities, and registrations after freezing.
    pub fn register_arc(
        &mut self,
        id: Identifier,
        value: Arc<T>,
        info: RegistrationInfo,
    ) -> Result<Holder<T>, RegistryError> {
        if self.frozen {
            return Err(RegistryError::Frozen {
                registry: self.key.location().clone(),
            });
        }
        if self.by_id.contains_key(&id) {
            return Err(RegistryError::DuplicateIdentifier {
                registry: self.key.location().clone(),
                id,
            });
        }
        let ptr = Arc::as_ptr(&value) as usize;
        if self.by_ptr.contains_key(&ptr) {
            return Err(RegistryError::DuplicateValue {
                registry: self.key.location().clone(),
                id,
            });
        }

        let resource_key = ResourceKey::for_registry(&self.key, id.clone());

        let holder = if self.intrusive {
            let Some(intrusive_holder) = self.unregistered_intrusive.remove(&ptr) else {
                return Err(RegistryError::MissingIntrusiveHolder {
                    registry: self.key.location().clone(),
                    id,
                });
            };
            intrusive_holder.bind_key(resource_key.clone())?;
            intrusive_holder
        } else if let Some(placeholder) = self.placeholders.remove(&id) {
            placeholder.bind_value(Arc::clone(&value))?;
            placeholder
        } else {
            ReferenceHolder::new_bound(self.key.clone(), resource_key.clone(), Arc::clone(&value))
        };

        let index = self.entries.len();
        let numeric_id = index as u32;
        self.lifecycle = self.lifecycle.combine(info.lifecycle);

        if self.default_id.as_ref() == Some(&id) {
            self.default_entry_index = Some(index);
        }

        self.by_id.insert(id.clone(), index);
        self.by_ptr.insert(ptr, index);
        self.entries.push(RegisteredEntry {
            id: numeric_id,
            identifier: id,
            resource_key,
            value,
            holder: holder.clone(),
            info,
        });

        Ok(Holder::Reference(holder))
    }

    // -------------------------------------------------------------------------
    // Lookups with Section 2.2 selective default-registry fallback semantics
    // -------------------------------------------------------------------------

    fn default_entry(&self) -> Option<&RegisteredEntry<T>> {
        self.default_entry_index.and_then(|i| self.entries.get(i))
    }

    /// Value lookup by identifier.
    ///
    /// - Non-defaulted: returns `None` if missing.
    /// - Defaulted: falls back to the default entry's value when missing.
    pub fn get_value(&self, id: &Identifier) -> Option<&Arc<T>> {
        if let Some(&idx) = self.by_id.get(id) {
            return Some(&self.entries[idx].value);
        }
        if self.is_defaulted() {
            return self.default_entry().map(|e| &e.value);
        }
        None
    }

    /// Optional value lookup by identifier.
    ///
    /// **No fallback** even on defaulted registries (Section 2.2).
    pub fn get_optional(&self, id: &Identifier) -> Option<&Arc<T>> {
        self.by_id.get(id).map(|&idx| &self.entries[idx].value)
    }

    /// Value lookup by [`ResourceKey`].
    ///
    /// **No fallback** even on defaulted registries (Section 2.2).
    pub fn get_value_by_key(&self, key: &ResourceKey) -> Option<&Arc<T>> {
        if !key.is_for(&self.key) {
            return None;
        }
        self.get_optional(key.location())
    }

    /// Value lookup by numeric ID.
    ///
    /// - Non-defaulted: returns `None` when `raw_id < 0` or `raw_id >= len`.
    /// - Defaulted: falls back to the default entry's value when out of range.
    pub fn by_id(&self, raw_id: i32) -> Option<&Arc<T>> {
        if raw_id >= 0
            && let Some(entry) = self.entries.get(raw_id as usize)
        {
            return Some(&entry.value);
        }
        if self.is_defaulted() {
            return self.default_entry().map(|e| &e.value);
        }
        None
    }

    /// Holder lookup by identifier.
    ///
    /// **No fallback** even on defaulted registries (Section 2.2).
    pub fn get_holder(&self, id: &Identifier) -> Option<Holder<T>> {
        self.by_id
            .get(id)
            .map(|&idx| Holder::Reference(self.entries[idx].holder.clone()))
    }

    /// Holder lookup by [`ResourceKey`].
    ///
    /// **No fallback** even on defaulted registries (Section 2.2).
    pub fn get_holder_by_key(&self, key: &ResourceKey) -> Option<Holder<T>> {
        if !key.is_for(&self.key) {
            return None;
        }
        self.get_holder(key.location())
    }

    /// Holder lookup by numeric ID.
    ///
    /// **No fallback** even on defaulted registries (Section 2.2).
    pub fn get_holder_by_id(&self, raw_id: i32) -> Option<Holder<T>> {
        if raw_id < 0 {
            return None;
        }
        self.entries
            .get(raw_id as usize)
            .map(|e| Holder::Reference(e.holder.clone()))
    }

    /// Numeric ID lookup by value (`Arc` object identity).
    ///
    /// - Non-defaulted: returns `-1` for unregistered values.
    /// - Defaulted: returns the numeric ID of the default value (or `-1` if default is unregistered).
    pub fn id_of(&self, value: &Arc<T>) -> i32 {
        let ptr = Arc::as_ptr(value) as usize;
        if let Some(&idx) = self.by_ptr.get(&ptr) {
            return self.entries[idx].id as i32;
        }
        if self.is_defaulted()
            && let Some(def) = self.default_entry()
        {
            return def.id as i32;
        }
        -1
    }

    /// Numeric ID lookup for an optional value (`None` counts as absent).
    ///
    /// - Non-defaulted: returns `-1` for `None` or unregistered values.
    /// - Defaulted: returns the numeric ID of the default value.
    pub fn id_of_opt(&self, value: Option<&Arc<T>>) -> i32 {
        match value {
            Some(v) => self.id_of(v),
            None => {
                if self.is_defaulted()
                    && let Some(def) = self.default_entry()
                {
                    return def.id as i32;
                }
                -1
            }
        }
    }

    /// Exact numeric ID lookup by value (`Arc` object identity) with no default fallback.
    pub fn id_of_exact(&self, value: &Arc<T>) -> Option<u32> {
        let ptr = Arc::as_ptr(value) as usize;
        self.by_ptr.get(&ptr).map(|&idx| self.entries[idx].id)
    }

    /// Identifier lookup by value (`Arc` object identity).
    ///
    /// - Non-defaulted: returns `None` if unregistered.
    /// - Defaulted: falls back to the default identifier.
    pub fn identifier_of(&self, value: &Arc<T>) -> Option<&Identifier> {
        let ptr = Arc::as_ptr(value) as usize;
        if let Some(&idx) = self.by_ptr.get(&ptr) {
            return Some(&self.entries[idx].identifier);
        }
        self.default_id.as_ref()
    }

    /// Exact resource key lookup by value (`Arc` object identity), with no fallback.
    pub fn resource_key_of(&self, value: &Arc<T>) -> Option<&ResourceKey> {
        let ptr = Arc::as_ptr(value) as usize;
        self.by_ptr
            .get(&ptr)
            .map(|&idx| &self.entries[idx].resource_key)
    }

    /// Any-entry lookup (Section 2.2):
    /// - Non-defaulted: returns the first registered entry (`id 0`), or `None` if empty.
    /// - Defaulted: returns the default entry.
    pub fn get_any(&self) -> Option<Holder<T>> {
        if self.is_defaulted() {
            return self
                .default_entry()
                .map(|e| Holder::Reference(e.holder.clone()));
        }
        self.entries
            .first()
            .map(|e| Holder::Reference(e.holder.clone()))
    }

    /// Random-entry lookup (Section 2.2):
    /// - When empty: returns `None` on non-defaulted, or the default entry on defaulted.
    /// - Otherwise invokes `pick(len)` to choose an index in `0..len`.
    pub fn get_random(&self, pick: impl FnOnce(usize) -> usize) -> Option<Holder<T>> {
        if self.entries.is_empty() {
            return if self.is_defaulted() {
                self.default_entry()
                    .map(|e| Holder::Reference(e.holder.clone()))
            } else {
                None
            };
        }
        let idx = pick(self.entries.len()) % self.entries.len();
        Some(Holder::Reference(self.entries[idx].holder.clone()))
    }

    /// Returns whether `id` is registered in this registry (exact, no default fallback).
    pub fn contains_key(&self, id: &Identifier) -> bool {
        self.by_id.contains_key(id)
    }

    /// Returns registration metadata for `id` if registered.
    pub fn registration_info(&self, id: &Identifier) -> Option<&RegistrationInfo> {
        self.by_id.get(id).map(|&idx| &self.entries[idx].info)
    }

    /// Iterates all registered entries in numeric ID order (`0 .. N - 1`).
    pub fn iter(&self) -> impl Iterator<Item = (u32, &Identifier, &Arc<T>)> + '_ {
        self.entries.iter().map(|e| (e.id, &e.identifier, &e.value))
    }

    /// Iterates all registered holders in numeric ID order (`0 .. N - 1`).
    pub fn holders(&self) -> impl Iterator<Item = Holder<T>> + '_ {
        self.entries
            .iter()
            .map(|e| Holder::Reference(e.holder.clone()))
    }

    // -------------------------------------------------------------------------
    // Tag pre-creation, binding, freezing, validation, and reload
    // -------------------------------------------------------------------------

    /// Gets an existing named tag or pre-creates an unbound named tag placeholder before freezing.
    pub fn get_or_create_tag(&mut self, tag: &TagKey) -> Result<NamedHolderSet<T>, RegistryError> {
        if !tag.is_for(&self.key) {
            return Err(RegistryError::InvalidTagHolder {
                registry: self.key.location().clone(),
                tag: tag.location().clone(),
                source: Box::new(HolderError::OwnerMismatch {
                    expected: Box::new(self.key.clone()),
                    actual: Box::new(tag.registry().clone()),
                }),
            });
        }
        if self.frozen {
            if let Some(existing) = self.active_tags.get(tag.location()) {
                return Ok(existing.clone());
            }
            return Err(RegistryError::Frozen {
                registry: self.key.location().clone(),
            });
        }
        let set = self
            .pending_tags
            .entry(tag.location().clone())
            .or_insert_with(|| NamedHolderSet::new_unbound(tag.clone()))
            .clone();
        Ok(set)
    }

    /// Binds all pre-created tags that are still unbound to empty element lists
    /// (bootstrap step 4.2, Section 2.3).
    pub fn bind_empty_pre_referenced_tags(&mut self) -> Result<(), RegistryError> {
        if self.frozen {
            return Err(RegistryError::Frozen {
                registry: self.key.location().clone(),
            });
        }
        for set in self.pending_tags.values() {
            if !set.is_bound() {
                set.bind(Vec::new());
            }
        }
        Ok(())
    }

    /// Binds tag contents on an unfrozen registry prior to [`freeze`](Self::freeze).
    ///
    /// Reuses existing pre-created [`NamedHolderSet`] instances and creates new ones for
    /// newly introduced tags.
    pub fn bind_tags(
        &mut self,
        tags: BTreeMap<Identifier, Vec<Holder<T>>>,
    ) -> Result<(), RegistryError> {
        if self.frozen {
            return Err(RegistryError::Frozen {
                registry: self.key.location().clone(),
            });
        }
        for (tag_id, holders) in tags {
            self.validate_tag_holders(&tag_id, &holders)?;
            let tag_key = TagKey::new(self.key.clone(), tag_id.clone());
            let set = self
                .pending_tags
                .entry(tag_id)
                .or_insert_with(|| NamedHolderSet::new_unbound(tag_key));
            set.bind(holders);
        }
        Ok(())
    }

    /// Freezes the registry according to Section 2.3's exact checks.
    ///
    /// Freezing an already-frozen registry is an idempotent no-op.
    pub fn freeze(&mut self) -> Result<(), RegistryError> {
        if self.frozen {
            return Ok(());
        }

        // Check for unbound placeholder holders (sorted by identifier).
        if !self.placeholders.is_empty() {
            let mut unbound: Vec<Identifier> = self.placeholders.keys().cloned().collect();
            unbound.sort();
            return Err(RegistryError::UnboundKeys {
                registry: self.key.location().clone(),
                keys: unbound,
            });
        }

        // Check for unregistered intrusive holders.
        if self.intrusive && !self.unregistered_intrusive.is_empty() {
            return Err(RegistryError::UnregisteredIntrusiveHolders {
                registry: self.key.location().clone(),
                count: self.unregistered_intrusive.len(),
            });
        }

        // Check that active tag table was not already bound before freezing.
        if self.active_tags_bound {
            return Err(RegistryError::TagsAlreadyBound {
                registry: self.key.location().clone(),
            });
        }

        // Check that all pre-created named tags are bound (sorted by tag identifier).
        let mut unbound_tags: Vec<Identifier> = self
            .pending_tags
            .iter()
            .filter(|(_, set)| !set.is_bound())
            .map(|(id, _)| id.clone())
            .collect();
        if !unbound_tags.is_empty() {
            unbound_tags.sort();
            return Err(RegistryError::UnboundTags {
                registry: self.key.location().clone(),
                tags: unbound_tags,
            });
        }

        // Verify all tag members belong to this registry and are reference holders.
        for (tag_id, set) in &self.pending_tags {
            let contents = set.contents()?;
            self.validate_tag_holders(tag_id, &contents)?;
        }

        // Publish tags and populate reverse tag-membership sets on every holder.
        self.unregistered_intrusive.clear();
        self.active_tags = std::mem::take(&mut self.pending_tags);
        self.active_tags_bound = true;
        self.recompute_reverse_tag_sets()?;
        self.frozen = true;
        Ok(())
    }

    /// Validates post-freeze invariants:
    /// - Defaulted registries must have registered their default identifier.
    /// - If `require_non_empty` is `true`, the registry must contain at least 1 entry.
    pub fn validate(&self, require_non_empty: bool) -> Result<(), RegistryError> {
        if let Some(default_id) = &self.default_id
            && self.default_entry_index.is_none()
        {
            return Err(RegistryError::MissingDefaultEntry {
                registry: self.key.location().clone(),
                default_id: default_id.clone(),
            });
        }
        if require_non_empty && self.entries.is_empty() {
            return Err(RegistryError::EmptyRegistry {
                registry: self.key.location().clone(),
            });
        }
        Ok(())
    }

    /// Applies a pending tag reload to a frozen static registry (Section 4.5).
    ///
    /// Implements the **empty-reload quirk**: if `loaded_tags` is empty, no reload is
    /// applied and previously bound tags remain untouched (returning `Ok(false)`).
    /// Otherwise reuses existing [`NamedHolderSet`] instances where tag keys match,
    /// replaces the active tag map, recomputes reverse tag-membership sets, and returns `Ok(true)`.
    pub fn apply_tag_reload(
        &mut self,
        loaded_tags: BTreeMap<Identifier, Vec<Holder<T>>>,
    ) -> Result<bool, RegistryError> {
        if loaded_tags.is_empty() {
            return Ok(false);
        }

        for (tag_id, holders) in &loaded_tags {
            self.validate_tag_holders(tag_id, holders)?;
        }

        let mut next_active = BTreeMap::new();
        for (tag_id, holders) in loaded_tags {
            let set = if let Some(existing) = self.active_tags.remove(&tag_id) {
                existing.bind(holders);
                existing
            } else {
                NamedHolderSet::new_bound(TagKey::new(self.key.clone(), tag_id.clone()), holders)
            };
            next_active.insert(tag_id, set);
        }

        // Any previously active tags not in the new reload are rebound to empty in case
        // callers still hold cloned NamedHolderSet handles.
        for (_, stale) in std::mem::take(&mut self.active_tags) {
            stale.bind(Vec::new());
        }

        self.active_tags = next_active;
        self.recompute_reverse_tag_sets()?;
        Ok(true)
    }

    /// Looks up an active bound tag by [`TagKey`].
    pub fn get_tag(&self, tag: &TagKey) -> Option<NamedHolderSet<T>> {
        if !tag.is_for(&self.key) {
            return None;
        }
        self.active_tags.get(tag.location()).cloned()
    }

    /// Looks up an active bound tag by tag [`Identifier`].
    pub fn get_tag_by_id(&self, tag_id: &Identifier) -> Option<NamedHolderSet<T>> {
        self.active_tags.get(tag_id).cloned()
    }

    /// Iterates all active bound tags in identifier order.
    pub fn tags(&self) -> impl Iterator<Item = (&Identifier, NamedHolderSet<T>)> + '_ {
        self.active_tags.iter().map(|(id, set)| (id, set.clone()))
    }

    /// Returns the number of active bound tags.
    pub fn tag_count(&self) -> usize {
        self.active_tags.len()
    }

    fn validate_tag_holders(
        &self,
        tag_id: &Identifier,
        holders: &[Holder<T>],
    ) -> Result<(), RegistryError> {
        for holder in holders {
            match holder {
                Holder::Direct(_) => {
                    return Err(RegistryError::InvalidTagHolder {
                        registry: self.key.location().clone(),
                        tag: tag_id.clone(),
                        source: Box::new(HolderError::DirectHolderNotAllowed),
                    });
                }
                Holder::Reference(r) => {
                    if r.owner() != &self.key {
                        return Err(RegistryError::InvalidTagHolder {
                            registry: self.key.location().clone(),
                            tag: tag_id.clone(),
                            source: Box::new(HolderError::OwnerMismatch {
                                expected: Box::new(self.key.clone()),
                                actual: Box::new(r.owner().clone()),
                            }),
                        });
                    }
                }
            }
        }
        Ok(())
    }

    fn recompute_reverse_tag_sets(&self) -> Result<(), RegistryError> {
        let mut reverse: HashMap<Identifier, HashSet<TagKey>> =
            HashMap::with_capacity(self.entries.len());
        for entry in &self.entries {
            reverse.insert(entry.identifier.clone(), HashSet::new());
        }
        for (tag_id, set) in &self.active_tags {
            let tag_key = TagKey::new(self.key.clone(), tag_id.clone());
            for holder in set.contents()? {
                if let Holder::Reference(r) = holder
                    && let Ok(key) = r.key()
                    && let Some(member_tags) = reverse.get_mut(key.location())
                {
                    member_tags.insert(tag_key.clone());
                }
            }
        }
        for entry in &self.entries {
            let tags = reverse.remove(&entry.identifier).unwrap_or_default();
            entry.holder.bind_tags(tags);
        }
        Ok(())
    }
}

/// Assigns deterministic numeric IDs (`0, 1, ...`) to data-driven registry elements from
/// their full file identifiers (`<namespace>:<registry_path>/<element_path>.json`).
///
/// Per Section 2.4.2.5 (Edge Case 5), sorting occurs on the **full file identifier with
/// `.json` still attached** using [`Identifier`]'s natural ordering (path first, namespace
/// second), *before* stripping `<registry_path>/` and `.json`.
pub fn assign_data_driven_ids(
    registry_path: &str,
    file_ids: &[Identifier],
) -> Result<Vec<(u32, Identifier)>, DataDrivenIdError> {
    let prefix = if registry_path.ends_with('/') {
        registry_path.to_owned()
    } else {
        format!("{registry_path}/")
    };

    let mut sorted: Vec<Identifier> = file_ids.to_vec();
    sorted.sort();

    let mut seen: HashSet<&Identifier> = HashSet::with_capacity(sorted.len());
    let mut out = Vec::with_capacity(sorted.len());

    for (idx, file_id) in sorted.iter().enumerate() {
        if !seen.insert(file_id) {
            return Err(DataDrivenIdError::DuplicateFile {
                file_id: file_id.clone(),
            });
        }
        let path = file_id.path();
        let Some(after_prefix) = path.strip_prefix(&prefix) else {
            return Err(DataDrivenIdError::MissingRegistryPrefix {
                file_id: file_id.clone(),
                expected_prefix: prefix,
            });
        };
        let Some(element_path) = after_prefix.strip_suffix(".json") else {
            return Err(DataDrivenIdError::MissingJsonSuffix {
                file_id: file_id.clone(),
            });
        };
        let element_id = Identifier::from_parts(file_id.namespace(), element_path)?;
        out.push((idx as u32, element_id));
    }

    Ok(out)
}

/// Evaluates server-side known-pack negotiation using the exact-match rule (Section 2.5.2, Edge Case 12).
///
/// If `client_accepted` is equal (same packs in the same order) to `server_requested`, all
/// requested packs are negotiated. Otherwise—even if the client returned a strict subset—the
/// negotiated set is **empty** and the server transmits full NBT for every entry.
pub fn negotiate_known_packs(
    server_requested: &[KnownPack],
    client_accepted: &[KnownPack],
) -> Vec<KnownPack> {
    if server_requested == client_accepted {
        server_requested.to_vec()
    } else {
        Vec::new()
    }
}

/// One entry inside a `registry_data` synchronization packet (Section 2.5.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRegistryEntry<P> {
    /// Element identifier.
    pub id: Identifier,
    /// Optional inline payload (omitted when entry's known pack was negotiated).
    pub payload: Option<P>,
}

/// Builds the server's `registry_data` entries for `registry` in numeric ID order (`0 .. N - 1`),
/// omitting payloads whose known-pack provenance is in `negotiated_packs`.
pub fn build_sync_entries<T, P>(
    registry: &Registry<T>,
    negotiated_packs: &[KnownPack],
    encode_payload: impl Fn(&T) -> P,
) -> Vec<SyncRegistryEntry<P>> {
    registry
        .iter()
        .map(|(_, id, val)| {
            let omitted = registry
                .registration_info(id)
                .and_then(|info| info.known_pack.as_ref())
                .is_some_and(|pack| negotiated_packs.contains(pack));
            SyncRegistryEntry {
                id: id.clone(),
                payload: if omitted {
                    None
                } else {
                    Some(encode_payload(val))
                },
            }
        })
        .collect()
}

/// Client-side accumulator for `registry_data` and `update_tags` configuration packets (Section 2.5.5).
///
/// - Multiple `registry_data` packets for the same registry append entries in arrival order.
/// - Later `update_tags` payloads for the same registry replace earlier ones.
/// - Assembled client registries preserve the server's exact arrival order without re-sorting,
///   assigning [`RegistrationInfo::network()`] to every entry.
#[derive(Debug, Default)]
pub struct ClientRegistryAssembler<P> {
    packets: BTreeMap<Identifier, Vec<SyncRegistryEntry<P>>>,
    tag_payloads: BTreeMap<Identifier, BTreeMap<Identifier, Vec<i32>>>,
}

impl<P> ClientRegistryAssembler<P> {
    /// Creates a new empty client registry assembler.
    pub fn new() -> Self {
        Self {
            packets: BTreeMap::new(),
            tag_payloads: BTreeMap::new(),
        }
    }

    /// Appends one `registry_data` packet's entries in arrival order.
    pub fn push_registry_data(
        &mut self,
        registry_id: Identifier,
        entries: Vec<SyncRegistryEntry<P>>,
    ) {
        self.packets.entry(registry_id).or_default().extend(entries);
    }

    /// Records an `update_tags` payload for `registry_id`, replacing any earlier payload for
    /// that registry.
    pub fn push_tag_payload(
        &mut self,
        registry_id: Identifier,
        tags: BTreeMap<Identifier, Vec<i32>>,
    ) {
        self.tag_payloads.insert(registry_id, tags);
    }

    /// Returns the accumulated tag payload for `registry_id`, if any.
    pub fn tag_payload(&self, registry_id: &Identifier) -> Option<&BTreeMap<Identifier, Vec<i32>>> {
        self.tag_payloads.get(registry_id)
    }

    /// Assembles and freezes the client [`Registry<T>`] for `registry_id`, preserving exact
    /// packet arrival order, resolving omitted known-pack entries via `resolve_known_pack`,
    /// binding any received tags (dropping out-of-range numeric IDs per Section 4.5), and
    /// freezing the registry.
    pub fn assemble<T, E>(
        &self,
        registry_id: &Identifier,
        decode_payload: impl Fn(&Identifier, &P) -> Result<T, E>,
        resolve_known_pack: impl Fn(&Identifier) -> Result<T, E>,
    ) -> Result<Registry<T>, ClientAssembleError<E>> {
        let mut registry =
            Registry::with_options(registry_id.clone(), Lifecycle::Experimental, None, false);

        if let Some(entries) = self.packets.get(registry_id) {
            for entry in entries {
                let value = match &entry.payload {
                    Some(payload) => {
                        decode_payload(&entry.id, payload).map_err(ClientAssembleError::Decode)?
                    }
                    None => resolve_known_pack(&entry.id).map_err(ClientAssembleError::Decode)?,
                };
                registry.register(entry.id.clone(), value, RegistrationInfo::network())?;
            }
        }

        if let Some(raw_tags) = self.tag_payloads.get(registry_id) {
            let mut bound = BTreeMap::new();
            for (tag_id, raw_ids) in raw_tags {
                let holders: Vec<Holder<T>> = raw_ids
                    .iter()
                    .filter_map(|&raw_id| registry.get_holder_by_id(raw_id))
                    .collect();
                bound.insert(tag_id.clone(), holders);
            }
            registry.bind_tags(bound)?;
        }

        registry.freeze()?;
        Ok(registry)
    }
}

/// Errors raised during client-side synchronized registry assembly.
#[derive(Debug, Error)]
pub enum ClientAssembleError<E> {
    /// Entry decoding or known-pack resolution failed.
    #[error("failed to decode synchronized registry entry: {0}")]
    Decode(E),

    /// Registry registration, tag binding, or freezing failed.
    #[error(transparent)]
    Registry(#[from] RegistryError),
}
