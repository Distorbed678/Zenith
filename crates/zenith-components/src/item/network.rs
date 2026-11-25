//! Network encodings for component patches, item stacks, creative slots, and hashed stacks (spec B.5).

use std::collections::{BTreeMap, BTreeSet};

use zenith_nbt::{Accounter, Tag};
use zenith_registry::Identifier;

use super::codec::{decode_component_value, encode_component_value};
use super::patch::{ComponentPatch, PatchEntry};
use super::registry::ComponentTypeId;
use super::stack::{ItemStack, PrototypeRegistry};
use super::types::{ComponentValue, MapPostProcessing, UnitValue};
use crate::dynval::Dyn;
use crate::registry_context::RegistryContext;

/// Maximum number of added or removed component entries in a [`HashedStack`] (spec B.5).
pub const MAX_HASHED_PATCH_ENTRIES: usize = 256;

/// Writes a protocol VarInt (up to 5 bytes).
pub fn write_varint(buf: &mut Vec<u8>, value: i32) {
    let mut v = value as u32;
    loop {
        if (v & !0x7F) == 0 {
            buf.push(v as u8);
            return;
        }
        buf.push(((v & 0x7F) | 0x80) as u8);
        v >>= 7;
    }
}

/// Reads a protocol VarInt (up to 5 bytes), returning `(value, bytes_consumed)`.
pub fn read_varint(bytes: &[u8]) -> Result<(i32, usize), String> {
    let mut value: u32 = 0;
    let mut shift: u32 = 0;
    for (idx, &byte) in bytes.iter().enumerate() {
        if idx >= 5 {
            return Err("VarInt exceeds 5 bytes".to_owned());
        }
        value |= u32::from(byte & 0x7F) << shift;
        if (byte & 0x80) == 0 {
            return Ok((value as i32, idx + 1));
        }
        shift += 7;
    }
    Err("Unexpected EOF while reading VarInt".to_owned())
}

/// Resolves item numeric protocol IDs ↔ [`Identifier`] for network stack encoding/decoding.
pub trait ItemIdRegistry {
    /// Returns the numeric protocol ID for `id`.
    fn item_protocol_id(&self, id: &Identifier) -> Option<i32>;
    /// Returns the [`Identifier`] for `protocol_id`.
    fn item_by_protocol_id(&self, protocol_id: i32) -> Option<&Identifier>;
}

/// Simple bidirectional table implementing [`ItemIdRegistry`].
#[derive(Debug, Clone, Default)]
pub struct SimpleItemRegistry {
    by_id: BTreeMap<Identifier, i32>,
    by_num: BTreeMap<i32, Identifier>,
}

impl SimpleItemRegistry {
    /// Creates an empty item registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `(protocol_id, id)`.
    pub fn insert(&mut self, protocol_id: i32, id: Identifier) {
        self.by_id.insert(id.clone(), protocol_id);
        self.by_num.insert(protocol_id, id);
    }
}

impl ItemIdRegistry for SimpleItemRegistry {
    fn item_protocol_id(&self, id: &Identifier) -> Option<i32> {
        self.by_id.get(id).copied()
    }

    fn item_by_protocol_id(&self, protocol_id: i32) -> Option<&Identifier> {
        self.by_num.get(&protocol_id)
    }
}

// ----------------------------------------------------------------------------------------------------------
// Component value network encoding
// ----------------------------------------------------------------------------------------------------------

/// Encodes one component value on the network.
///
/// - Transient components use their dedicated wire encodings (`creative_slot_lock` = empty,
///   `additional_trade_cost` = VarInt, `map_post_processing` = VarInt).
/// - Components without a dedicated network codec (`custom_data`, `intangible_projectile`,
///   `map_decorations`, `recipes`, `debug_stick_state`, `lock`, `container_loot`) are sent as
///   nameless-root network NBT produced by their persistent codec (spec B.5).
/// - Other persistent types use compact network encodings where self-contained, or network NBT
///   produced by their persistent codec.
// PARITY [oracle]: settled by the protocol suite (per-component dedicated wire layouts beyond NBT-backed and transient types).
pub fn encode_component_value_network(
    ty: ComponentTypeId,
    value: &ComponentValue,
    buf: &mut Vec<u8>,
) -> Result<(), String> {
    match (ty, value) {
        (ComponentTypeId::CreativeSlotLock, ComponentValue::Unit(_)) => Ok(()),
        (ComponentTypeId::AdditionalTradeCost, ComponentValue::Int(n)) => {
            write_varint(buf, *n);
            Ok(())
        }
        (ComponentTypeId::MapPostProcessing, ComponentValue::MapPostProcessing(p)) => {
            write_varint(buf, *p as i32);
            Ok(())
        }
        (
            ComponentTypeId::MaxStackSize
            | ComponentTypeId::MaxDamage
            | ComponentTypeId::Damage
            | ComponentTypeId::RepairCost
            | ComponentTypeId::MapId
            | ComponentTypeId::OminousBottleAmplifier,
            ComponentValue::Int(n),
        ) => {
            write_varint(buf, *n);
            Ok(())
        }
        (
            ComponentTypeId::Unbreakable | ComponentTypeId::Glider | ComponentTypeId::Waxed,
            ComponentValue::Unit(_),
        ) => Ok(()),
        (ComponentTypeId::EnchantmentGlintOverride, ComponentValue::Bool(b)) => {
            buf.push(u8::from(*b));
            Ok(())
        }
        (
            ComponentTypeId::MinimumAttackCharge | ComponentTypeId::PotionDurationScale,
            ComponentValue::Float(f),
        ) => {
            buf.extend_from_slice(&f.to_be_bytes());
            Ok(())
        }
        (ComponentTypeId::Rarity, ComponentValue::Rarity(r)) => {
            write_varint(buf, *r as i32);
            Ok(())
        }
        (
            ComponentTypeId::Dye
            | ComponentTypeId::BaseColor
            | ComponentTypeId::WolfCollar
            | ComponentTypeId::CatCollar
            | ComponentTypeId::SheepColor
            | ComponentTypeId::ShulkerColor
            | ComponentTypeId::TropicalFishBaseColor
            | ComponentTypeId::TropicalFishPatternColor
            | ComponentTypeId::CushionColor,
            ComponentValue::Dye(d),
        ) => {
            write_varint(buf, d.id() as i32);
            Ok(())
        }
        (ComponentTypeId::DyedColor, ComponentValue::Int(rgb)) => {
            buf.extend_from_slice(&rgb.to_be_bytes());
            Ok(())
        }
        _ => {
            // Persistent codec -> nameless-root network NBT tag (mandatory for the 7 NBT-backed
            // components in B.5, and used as lossless wire encoding for remaining complex types).
            let dyn_val = encode_component_value(ty, value)
                .demand()
                .map_err(|e| e.message)?;
            let tag = dyn_val.to_tag()?;
            let nbt_bytes = zenith_nbt::to_bytes_network(Some(&tag)).map_err(|e| e.to_string())?;
            buf.extend_from_slice(&nbt_bytes);
            Ok(())
        }
    }
}

/// Decodes one component value from the network, returning `(value, bytes_consumed)`.
// PARITY [oracle]: settled by the protocol suite (per-component dedicated wire layouts beyond NBT-backed and transient types).
pub fn decode_component_value_network(
    ty: ComponentTypeId,
    bytes: &[u8],
    registries: &dyn RegistryContext,
) -> Result<(ComponentValue, usize), String> {
    match ty {
        ComponentTypeId::CreativeSlotLock => Ok((ComponentValue::Unit(UnitValue), 0)),
        ComponentTypeId::AdditionalTradeCost => {
            let (n, used) = read_varint(bytes)?;
            Ok((ComponentValue::Int(n), used))
        }
        ComponentTypeId::MapPostProcessing => {
            let (n, used) = read_varint(bytes)?;
            let p = u32::try_from(n)
                .ok()
                .and_then(MapPostProcessing::from_id)
                .ok_or_else(|| format!("Invalid map_post_processing id: {n}"))?;
            Ok((ComponentValue::MapPostProcessing(p), used))
        }
        ComponentTypeId::MaxStackSize => {
            let (n, used) = read_varint(bytes)?;
            if !(1..=99).contains(&n) {
                return Err(format!("max_stack_size {n} outside 1..=99"));
            }
            Ok((ComponentValue::Int(n), used))
        }
        ComponentTypeId::MaxDamage => {
            let (n, used) = read_varint(bytes)?;
            if n < 1 {
                return Err(format!("max_damage {n} must be >= 1"));
            }
            Ok((ComponentValue::Int(n), used))
        }
        ComponentTypeId::Damage | ComponentTypeId::RepairCost => {
            let (n, used) = read_varint(bytes)?;
            if n < 0 {
                return Err(format!("{ty} {n} must be >= 0"));
            }
            Ok((ComponentValue::Int(n), used))
        }
        ComponentTypeId::MapId => {
            let (n, used) = read_varint(bytes)?;
            Ok((ComponentValue::Int(n), used))
        }
        ComponentTypeId::OminousBottleAmplifier => {
            let (n, used) = read_varint(bytes)?;
            if !(0..=4).contains(&n) {
                return Err(format!("ominous_bottle_amplifier {n} outside 0..=4"));
            }
            Ok((ComponentValue::Int(n), used))
        }
        ComponentTypeId::Unbreakable | ComponentTypeId::Glider | ComponentTypeId::Waxed => {
            Ok((ComponentValue::Unit(UnitValue), 0))
        }
        ComponentTypeId::EnchantmentGlintOverride => {
            let Some(&b) = bytes.first() else {
                return Err("Unexpected EOF reading bool".to_owned());
            };
            Ok((ComponentValue::Bool(b != 0), 1))
        }
        ComponentTypeId::MinimumAttackCharge | ComponentTypeId::PotionDurationScale => {
            if bytes.len() < 4 {
                return Err("Unexpected EOF reading f32".to_owned());
            }
            let f = f32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            let val =
                decode_component_value(ty, &Dyn::Num(crate::dynval::Num::Float(f)), registries)
                    .demand()
                    .map_err(|e| e.message)?;
            Ok((val, 4))
        }
        ComponentTypeId::Rarity => {
            let (n, used) = read_varint(bytes)?;
            let r = match n {
                0 => super::types::Rarity::Common,
                1 => super::types::Rarity::Uncommon,
                2 => super::types::Rarity::Rare,
                3 => super::types::Rarity::Epic,
                _ => return Err(format!("Invalid rarity ordinal: {n}")),
            };
            Ok((ComponentValue::Rarity(r), used))
        }
        ComponentTypeId::Dye
        | ComponentTypeId::BaseColor
        | ComponentTypeId::WolfCollar
        | ComponentTypeId::CatCollar
        | ComponentTypeId::SheepColor
        | ComponentTypeId::ShulkerColor
        | ComponentTypeId::TropicalFishBaseColor
        | ComponentTypeId::TropicalFishPatternColor
        | ComponentTypeId::CushionColor => {
            let (n, used) = read_varint(bytes)?;
            let d = u32::try_from(n)
                .ok()
                .and_then(super::types::DyeColor::from_id)
                .ok_or_else(|| format!("Invalid dye ordinal: {n}"))?;
            Ok((ComponentValue::Dye(d), used))
        }
        ComponentTypeId::DyedColor => {
            if bytes.len() < 4 {
                return Err("Unexpected EOF reading dyed_color i32".to_owned());
            }
            let rgb = i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            Ok((ComponentValue::Int(rgb), 4))
        }
        _ => {
            let mut cursor = std::io::Cursor::new(bytes);
            let mut acc = Accounter::default_quota();
            let tag: Tag = zenith_nbt::read_network(&mut cursor, &mut acc)
                .map_err(|e| format!("Network NBT decode failed for {ty}: {e}"))?
                .ok_or_else(|| format!("Empty network NBT (TAG_End) for {ty}"))?;
            let consumed = cursor.position() as usize;
            let val = decode_component_value(ty, &Dyn::from_tag(&tag), registries)
                .demand()
                .map_err(|e| e.message)?;
            Ok((val, consumed))
        }
    }
}

// ----------------------------------------------------------------------------------------------------------
// Patch network encodings (spec B.5, edge case 47)
// ----------------------------------------------------------------------------------------------------------

/// Encodes a [`ComponentPatch`] on the network (spec B.5, edge case 47):
/// `VarInt(set_count)`, `VarInt(removed_count)`, then all set entries `(VarInt id, value)`,
/// then all removed entries `(VarInt id)`. Set entries always precede removals regardless of in-memory order.
pub fn encode_patch_network(patch: &ComponentPatch, buf: &mut Vec<u8>) -> Result<(), String> {
    encode_patch_network_inner(patch, buf, false)
}

/// Encodes a length-delimited [`ComponentPatch`] for untrusted client→server creative-slot packets:
/// identical to [`encode_patch_network`], except each set value is prefixed with its `VarInt` byte length.
pub fn encode_patch_delimited(patch: &ComponentPatch, buf: &mut Vec<u8>) -> Result<(), String> {
    encode_patch_network_inner(patch, buf, true)
}

fn encode_patch_network_inner(
    patch: &ComponentPatch,
    buf: &mut Vec<u8>,
    delimited: bool,
) -> Result<(), String> {
    let sets: Vec<(ComponentTypeId, &ComponentValue)> = patch.sets().collect();
    let removals: Vec<ComponentTypeId> = patch.removals().collect();

    write_varint(buf, sets.len() as i32);
    write_varint(buf, removals.len() as i32);

    for (ty, val) in sets {
        write_varint(buf, ty.protocol_id() as i32);
        if delimited {
            let mut tmp = Vec::new();
            encode_component_value_network(ty, val, &mut tmp)?;
            write_varint(buf, tmp.len() as i32);
            buf.extend_from_slice(&tmp);
        } else {
            encode_component_value_network(ty, val, buf)?;
        }
    }
    for ty in removals {
        write_varint(buf, ty.protocol_id() as i32);
    }
    Ok(())
}

/// Decodes a [`ComponentPatch`] from the network. Zero/zero decodes to the empty patch.
pub fn decode_patch_network(
    bytes: &[u8],
    registries: &dyn RegistryContext,
) -> Result<(ComponentPatch, usize), String> {
    decode_patch_network_inner(bytes, false, registries)
}

/// Decodes a length-delimited [`ComponentPatch`] (creative-slot variant): every set value is prefixed
/// with a `VarInt` byte length and must consume **exactly** that many bytes.
pub fn decode_patch_delimited(
    bytes: &[u8],
    registries: &dyn RegistryContext,
) -> Result<(ComponentPatch, usize), String> {
    decode_patch_network_inner(bytes, true, registries)
}

fn decode_patch_network_inner(
    bytes: &[u8],
    delimited: bool,
    registries: &dyn RegistryContext,
) -> Result<(ComponentPatch, usize), String> {
    let mut off = 0;
    let (set_count, u) = read_varint(&bytes[off..])?;
    off += u;
    let (rem_count, u) = read_varint(&bytes[off..])?;
    off += u;
    if set_count < 0 || rem_count < 0 {
        return Err(format!(
            "Negative patch entry count: sets={set_count}, removals={rem_count}"
        ));
    }
    if set_count == 0 && rem_count == 0 {
        return Ok((ComponentPatch::new(), off));
    }

    let mut patch = ComponentPatch::new();
    for _ in 0..set_count {
        let (raw_id, u) = read_varint(&bytes[off..])?;
        off += u;
        let ty = u32::try_from(raw_id)
            .ok()
            .and_then(ComponentTypeId::from_protocol_id)
            .ok_or_else(|| format!("Unknown component type protocol id: {raw_id}"))?;
        if delimited {
            let (byte_len, u) = read_varint(&bytes[off..])?;
            off += u;
            if byte_len < 0 {
                return Err(format!("Negative delimited value length: {byte_len}"));
            }
            let len = byte_len as usize;
            if off + len > bytes.len() {
                return Err(format!(
                    "Truncated delimited component {ty}: expected {len} bytes"
                ));
            }
            let slice = &bytes[off..off + len];
            let (val, consumed) = decode_component_value_network(ty, slice, registries)?;
            if consumed != len {
                return Err(format!(
                    "Delimited component {ty} consumed {consumed} bytes, expected exactly {len}"
                ));
            }
            off += len;
            patch.set(ty, val);
        } else {
            let (val, consumed) = decode_component_value_network(ty, &bytes[off..], registries)?;
            off += consumed;
            patch.set(ty, val);
        }
    }

    for _ in 0..rem_count {
        let (raw_id, u) = read_varint(&bytes[off..])?;
        off += u;
        let ty = u32::try_from(raw_id)
            .ok()
            .and_then(ComponentTypeId::from_protocol_id)
            .ok_or_else(|| format!("Unknown component type protocol id: {raw_id}"))?;
        patch.remove(ty);
    }

    Ok((patch, off))
}

// ----------------------------------------------------------------------------------------------------------
// Item stack network encodings (spec B.5, edge case 47)
// ----------------------------------------------------------------------------------------------------------

/// Encodes an optional [`ItemStack`] on the wire: `count <= 0` writes `0` and nothing else;
/// otherwise `VarInt(count)`, `VarInt(item_protocol_id)`, then the patch.
pub fn encode_optional_stack_network(
    stack: &ItemStack,
    items: &dyn ItemIdRegistry,
    buf: &mut Vec<u8>,
) -> Result<(), String> {
    if stack.is_empty() {
        write_varint(buf, 0);
        return Ok(());
    }
    encode_stack_network_inner(stack, items, buf, false)
}

/// Encodes a non-optional [`ItemStack`] on the wire (rejects empty stacks).
pub fn encode_stack_network(
    stack: &ItemStack,
    items: &dyn ItemIdRegistry,
    buf: &mut Vec<u8>,
) -> Result<(), String> {
    if stack.is_empty() {
        return Err("Cannot encode empty ItemStack with non-optional network codec".to_owned());
    }
    encode_stack_network_inner(stack, items, buf, false)
}

/// Encodes an optional [`ItemStack`] using the delimited creative-slot patch format.
pub fn encode_creative_slot_stack(
    stack: &ItemStack,
    items: &dyn ItemIdRegistry,
    buf: &mut Vec<u8>,
) -> Result<(), String> {
    if stack.is_empty() {
        write_varint(buf, 0);
        return Ok(());
    }
    encode_stack_network_inner(stack, items, buf, true)
}

fn encode_stack_network_inner(
    stack: &ItemStack,
    items: &dyn ItemIdRegistry,
    buf: &mut Vec<u8>,
    delimited: bool,
) -> Result<(), String> {
    let id = stack.id().ok_or("Empty stack")?;
    let item_pid = items
        .item_protocol_id(id)
        .ok_or_else(|| format!("Unknown item id for network encoding: {id}"))?;
    write_varint(buf, stack.count());
    write_varint(buf, item_pid);
    let patch = stack.components().expect("non-empty").patch();
    if delimited {
        encode_patch_delimited(patch, buf)
    } else {
        encode_patch_network(patch, buf)
    }
}

/// Decodes an optional [`ItemStack`] from the wire (`count <= 0` yields the empty stack; spec B.5, edge case 47).
pub fn decode_optional_stack_network(
    bytes: &[u8],
    items: &dyn ItemIdRegistry,
    prototypes: &PrototypeRegistry,
    registries: &dyn RegistryContext,
) -> Result<(ItemStack, usize), String> {
    let (count, mut off) = read_varint(bytes)?;
    if count <= 0 {
        return Ok((ItemStack::empty(), off));
    }
    let (stack, u) =
        decode_non_empty_body(&bytes[off..], count, items, prototypes, false, registries)?;
    off += u;
    Ok((stack, off))
}

/// Decodes a non-optional [`ItemStack`] from the wire (rejects `count <= 0`).
pub fn decode_stack_network(
    bytes: &[u8],
    items: &dyn ItemIdRegistry,
    prototypes: &PrototypeRegistry,
    registries: &dyn RegistryContext,
) -> Result<(ItemStack, usize), String> {
    let (count, mut off) = read_varint(bytes)?;
    if count <= 0 {
        return Err(format!(
            "Non-optional item stack cannot be empty (count={count})"
        ));
    }
    let (stack, u) =
        decode_non_empty_body(&bytes[off..], count, items, prototypes, false, registries)?;
    off += u;
    Ok((stack, off))
}

/// Decodes a creative-slot packet stack (spec B.5, edge case 42):
/// uses length-delimited patches and additionally verifies that the decoded stack encodes with the
/// persistent codec (does **not** run stack validation).
pub fn decode_creative_slot_stack(
    bytes: &[u8],
    items: &dyn ItemIdRegistry,
    prototypes: &PrototypeRegistry,
    registries: &dyn RegistryContext,
) -> Result<(ItemStack, usize), String> {
    let (count, mut off) = read_varint(bytes)?;
    if count <= 0 {
        return Ok((ItemStack::empty(), off));
    }
    let (stack, u) =
        decode_non_empty_body(&bytes[off..], count, items, prototypes, true, registries)?;
    off += u;
    // Spec B.5: must encode with the persistent item-stack codec, or the packet is rejected.
    stack
        .encode()
        .demand()
        .map_err(|e| format!("Creative-slot stack failed persistent encoding check: {e}"))?;
    Ok((stack, off))
}

fn decode_non_empty_body(
    bytes: &[u8],
    count: i32,
    items: &dyn ItemIdRegistry,
    prototypes: &PrototypeRegistry,
    delimited: bool,
    registries: &dyn RegistryContext,
) -> Result<(ItemStack, usize), String> {
    let mut off = 0;
    let (item_pid, u) = read_varint(&bytes[off..])?;
    off += u;
    let id = items
        .item_by_protocol_id(item_pid)
        .ok_or_else(|| format!("Unknown item protocol id: {item_pid}"))?
        .clone();
    let (patch, u) = if delimited {
        decode_patch_delimited(&bytes[off..], registries)?
    } else {
        decode_patch_network(&bytes[off..], registries)?
    };
    off += u;
    let stack = ItemStack::from_parts(id, count, &patch, prototypes)?;
    Ok((stack, off))
}

// ----------------------------------------------------------------------------------------------------------
// Hashed stacks (spec B.5, open question 10)
// ----------------------------------------------------------------------------------------------------------

/// Computes the 32-bit hash of a component's persistent encoding (spec B.5, open question 10).
// PARITY [oracle]: settled by the protocol suite (hashed-stack hash function belongs to the protocol spec).
pub trait ComponentHasher {
    /// Returns the 32-bit hash for `(ty, value)`.
    fn hash_component(&self, ty: ComponentTypeId, value: &ComponentValue) -> Result<i32, String>;
}

/// Client→server container-click hashed patch (`added` map + `removed` set, each at most 256 entries).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HashedPatchMap {
    /// Set component types → 32-bit hash of their persistent encoding.
    pub added: BTreeMap<ComponentTypeId, i32>,
    /// Explicitly removed component types.
    pub removed: BTreeSet<ComponentTypeId>,
}

/// Client→server container-click hashed item stack (spec B.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashedStack {
    /// `None` for empty stack, `Some` for non-empty.
    pub item: Option<ActualHashedStack>,
}

/// Non-empty payload of a [`HashedStack`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActualHashedStack {
    /// Item identifier.
    pub id: Identifier,
    /// Item count.
    pub count: i32,
    /// Hashed component patch.
    pub components: HashedPatchMap,
}

impl HashedStack {
    /// Creates an empty hashed stack.
    pub const fn empty() -> Self {
        Self { item: None }
    }

    /// Computes a [`HashedStack`] from an [`ItemStack`] using `hasher`.
    pub fn from_stack(stack: &ItemStack, hasher: &dyn ComponentHasher) -> Result<Self, String> {
        let Some(id) = stack.id() else {
            return Ok(Self::empty());
        };
        let patch = stack.components().expect("non-empty").patch();
        let mut added = BTreeMap::new();
        let mut removed = BTreeSet::new();
        for (ty, entry) in patch.iter() {
            match entry {
                PatchEntry::Set(val) => {
                    added.insert(ty, hasher.hash_component(ty, val)?);
                }
                PatchEntry::Removed => {
                    removed.insert(ty);
                }
            }
        }
        if added.len() > MAX_HASHED_PATCH_ENTRIES || removed.len() > MAX_HASHED_PATCH_ENTRIES {
            return Err("Hashed patch exceeds 256 entries".to_owned());
        }
        Ok(Self {
            item: Some(ActualHashedStack {
                id: id.clone(),
                count: stack.count(),
                components: HashedPatchMap { added, removed },
            }),
        })
    }

    /// Checks whether `stack` matches this hashed stack (spec B.5):
    /// same item & count, equal removed sets, and every set component's hash matches.
    pub fn matches(&self, stack: &ItemStack, hasher: &dyn ComponentHasher) -> Result<bool, String> {
        match (&self.item, stack.id()) {
            (None, None) => Ok(true),
            (Some(actual), Some(id)) => {
                if actual.id != *id || actual.count != stack.count() {
                    return Ok(false);
                }
                let patch = stack.components().expect("non-empty").patch();
                let actual_removed: BTreeSet<ComponentTypeId> = patch.removals().collect();
                if actual_removed != actual.components.removed {
                    return Ok(false);
                }
                let sets: Vec<(ComponentTypeId, &ComponentValue)> = patch.sets().collect();
                if sets.len() != actual.components.added.len() {
                    return Ok(false);
                }
                for (ty, val) in sets {
                    let Some(&expected_hash) = actual.components.added.get(&ty) else {
                        return Ok(false);
                    };
                    if hasher.hash_component(ty, val)? != expected_hash {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Encodes a [`HashedStack`] on the wire (spec B.5).
    pub fn encode_network(
        &self,
        items: &dyn ItemIdRegistry,
        buf: &mut Vec<u8>,
    ) -> Result<(), String> {
        let Some(actual) = &self.item else {
            buf.push(0);
            return Ok(());
        };
        if actual.components.added.len() > MAX_HASHED_PATCH_ENTRIES
            || actual.components.removed.len() > MAX_HASHED_PATCH_ENTRIES
        {
            return Err("Hashed patch exceeds 256 entries".to_owned());
        }
        let pid = items
            .item_protocol_id(&actual.id)
            .ok_or_else(|| format!("Unknown item id: {}", actual.id))?;
        buf.push(1);
        write_varint(buf, pid);
        write_varint(buf, actual.count);
        write_varint(buf, actual.components.added.len() as i32);
        for (&ty, &hash) in &actual.components.added {
            write_varint(buf, ty.protocol_id() as i32);
            buf.extend_from_slice(&hash.to_be_bytes());
        }
        write_varint(buf, actual.components.removed.len() as i32);
        for &ty in &actual.components.removed {
            write_varint(buf, ty.protocol_id() as i32);
        }
        Ok(())
    }

    /// Decodes a [`HashedStack`] from the wire, enforcing the 256-entry cap on both maps.
    pub fn decode_network(
        bytes: &[u8],
        items: &dyn ItemIdRegistry,
    ) -> Result<(Self, usize), String> {
        let Some(&present) = bytes.first() else {
            return Err("Unexpected EOF reading HashedStack presence byte".to_owned());
        };
        let mut off = 1;
        if present == 0 {
            return Ok((Self::empty(), off));
        }
        let (pid, u) = read_varint(&bytes[off..])?;
        off += u;
        let id = items
            .item_by_protocol_id(pid)
            .ok_or_else(|| format!("Unknown item protocol id: {pid}"))?
            .clone();
        let (count, u) = read_varint(&bytes[off..])?;
        off += u;
        let (added_len, u) = read_varint(&bytes[off..])?;
        off += u;
        if added_len < 0 || added_len as usize > MAX_HASHED_PATCH_ENTRIES {
            return Err(format!(
                "Hashed added count {added_len} out of range 0..=256"
            ));
        }
        let mut added = BTreeMap::new();
        for _ in 0..added_len {
            let (raw_ty, u) = read_varint(&bytes[off..])?;
            off += u;
            let ty = u32::try_from(raw_ty)
                .ok()
                .and_then(ComponentTypeId::from_protocol_id)
                .ok_or_else(|| format!("Unknown component type protocol id: {raw_ty}"))?;
            if off + 4 > bytes.len() {
                return Err("Unexpected EOF reading component hash".to_owned());
            }
            let hash =
                i32::from_be_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]]);
            off += 4;
            added.insert(ty, hash);
        }
        let (rem_len, u) = read_varint(&bytes[off..])?;
        off += u;
        if rem_len < 0 || rem_len as usize > MAX_HASHED_PATCH_ENTRIES {
            return Err(format!(
                "Hashed removed count {rem_len} out of range 0..=256"
            ));
        }
        let mut removed = BTreeSet::new();
        for _ in 0..rem_len {
            let (raw_ty, u) = read_varint(&bytes[off..])?;
            off += u;
            let ty = u32::try_from(raw_ty)
                .ok()
                .and_then(ComponentTypeId::from_protocol_id)
                .ok_or_else(|| format!("Unknown component type protocol id: {raw_ty}"))?;
            removed.insert(ty);
        }
        Ok((
            Self {
                item: Some(ActualHashedStack {
                    id,
                    count,
                    components: HashedPatchMap { added, removed },
                }),
            },
            off,
        ))
    }
}
