//! Clean-room identifiers, registries, block-state IDs, tags, and feature flags for Zenith.
//!
//! Implemented strictly from `docs/specs/registry/identifiers-registries-tags.md`.

pub mod block_state;
pub mod feature;
pub mod holder;
pub mod identifier;
pub mod registry;
pub mod tag;

pub use block_state::{
    BlockDef, BlockEntry, BlockState, BlockStateError, BlockStateMismatch, BlockStateTable,
    PropertyDef, PropertyKind, is_valid_property_name,
};
pub use feature::{
    DataPackFeatureInfo, FeatureElement, FeatureError, FeatureFilteredRegistry, FeatureFlag,
    FeatureFlagSet, FeatureUniverse, FeatureUniverseBuilder, MAX_FLAGS_PER_UNIVERSE,
    PackSelectionMode, PackSelectionResult, PartialFeatureParseError, select_data_packs,
};
pub use holder::{
    Holder, HolderError, HolderSet, NamedHolderSet, ReferenceHolder, decode_holder_registry_only,
    decode_holder_set, decode_holder_with_inline, encode_holder_registry_only, encode_holder_set,
    encode_holder_with_inline,
};
pub use identifier::{
    DEFAULT_NAMESPACE, Identifier, IdentifierError, NETWORK_MAX_UTF8_BYTES,
    NETWORK_MAX_UTF16_UNITS, ResourceKey, SEPARATOR, TagKey, escape_java_string,
    is_valid_command_char, is_valid_namespace, is_valid_namespace_char, is_valid_path,
    is_valid_path_char,
};
pub use registry::{
    ClientAssembleError, ClientRegistryAssembler, DEFAULTED_BUILTIN_REGISTRIES, DataDrivenIdError,
    FEATURE_FILTERED_REGISTRIES, INTRUSIVE_HOLDER_REGISTRIES, KnownPack, Lifecycle,
    NON_EMPTY_SYNCHRONIZED_REGISTRIES, RELOADABLE_REGISTRIES, RegistrationInfo, Registry,
    RegistryError, SYNCHRONIZED_DYNAMIC_REGISTRIES, SyncRegistryEntry, assign_data_driven_ids,
    build_sync_entries, negotiate_known_packs,
};
pub use tag::{
    DecodedTagMap, MemoryTagSource, TagDiagnostic, TagElementLookup, TagEntry, TagFile,
    TagLoadResult, TagLoader, TagParseError, TagSource, TagSourceItem, TagWireError,
    TrackedTagEntry, decode_registry_tags, encode_registry_tags, encode_update_tags_payload,
};
