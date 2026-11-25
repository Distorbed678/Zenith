//! Item data components (spec Part B): 122-type registry, typed values, persistent codecs,
//! prototypes & patches, item stacks & templates, network & hashed encodings, and runtime defaults loading.

pub mod codec;
pub mod defaults;
pub mod network;
pub mod patch;
pub mod registry;
pub mod stack;
pub mod types;

pub use codec::{decode_component_value, encode_component_value};
pub use defaults::{
    cached_item_components_dir, load_prototypes_for_version, load_prototypes_from_dir,
};
pub use network::{
    ActualHashedStack, ComponentHasher, HashedPatchMap, HashedStack, ItemIdRegistry,
    MAX_HASHED_PATCH_ENTRIES, SimpleItemRegistry, decode_component_value_network,
    decode_creative_slot_stack, decode_optional_stack_network, decode_patch_delimited,
    decode_patch_network, decode_stack_network, encode_component_value_network,
    encode_creative_slot_stack, encode_optional_stack_network, encode_patch_delimited,
    encode_patch_network, encode_stack_network, read_varint, write_varint,
};
pub use patch::{ComponentMap, ComponentPatch, PatchEntry, PatchedComponentMap};
pub use registry::{COMPONENT_TYPE_COUNT, ComponentTypeId};
pub use stack::{
    DEFAULT_MAX_STACK_SIZE, ItemStack, ItemTemplate, MAX_ITEM_COUNT, PrototypeRegistry,
    WeightFraction, compute_bundle_weight, lore_base_style,
};
pub use types::*;
