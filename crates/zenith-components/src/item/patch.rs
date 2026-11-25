//! Component maps (prototypes), component patches, patched-map semantics, and JSON/NBT patch encoding
//! (spec B.1, B.3, B.4).

use std::collections::BTreeMap;

use zenith_codec::codec::DataResult;
use zenith_codec::json::Json;
use zenith_nbt::Tag;

use super::codec::{decode_component_value, encode_component_value};
use super::registry::ComponentTypeId;
use super::types::ComponentValue;
use crate::dynval::{Dyn, MapBuilder, read_map};
use crate::registry_context::RegistryContext;

/// An immutable component map (used as an item type's prototype and for full component maps).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ComponentMap {
    entries: BTreeMap<ComponentTypeId, ComponentValue>,
}

impl ComponentMap {
    /// An empty component map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts a component value.
    pub fn insert(&mut self, ty: ComponentTypeId, value: ComponentValue) {
        self.entries.insert(ty, value);
    }

    /// Looks up a component value.
    pub fn get(&self, ty: ComponentTypeId) -> Option<&ComponentValue> {
        self.entries.get(&ty)
    }

    /// Whether `ty` is present.
    pub fn contains(&self, ty: ComponentTypeId) -> bool {
        self.entries.contains_key(&ty)
    }

    /// Number of components.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the map is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterates `(type, value)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (ComponentTypeId, &ComponentValue)> {
        self.entries.iter().map(|(&k, v)| (k, v))
    }

    /// Decodes a full component map (no `!` removal keys allowed; spec B.4).
    pub fn decode(d: &Dyn, registries: &dyn RegistryContext) -> DataResult<Self> {
        let entries = match read_map(d) {
            DataResult::Success(m) => m,
            other => return other.map(|_| Self::new()),
        };
        let mut map = Self::new();
        let mut errors = Vec::new();
        for (raw_key, val) in entries {
            let key_str = raw_key.to_string_lossy();
            let ty = match ComponentTypeId::parse_id(&key_str) {
                Ok(t) => t,
                Err(m) => {
                    errors.push(m);
                    continue;
                }
            };
            if ty.is_transient() {
                errors.push(format!("{ty} is not a persistent component"));
                continue;
            }
            match decode_component_value(ty, val, registries) {
                DataResult::Success(v) => map.insert(ty, v),
                DataResult::Error { message, partial } => {
                    errors.push(format!("{ty}: {message}"));
                    if let Some(p) = partial {
                        map.insert(ty, p);
                    }
                }
                DataResult::Failure(m) => return DataResult::Failure(m),
            }
        }
        if errors.is_empty() {
            DataResult::Success(map)
        } else {
            DataResult::error_with_partial(errors.join("; "), map)
        }
    }

    /// Encodes a full component map (omitting any transient types).
    pub fn encode(&self) -> DataResult<Dyn> {
        let mut b = MapBuilder::new();
        for (&ty, val) in &self.entries {
            if ty.is_transient() {
                continue;
            }
            match encode_component_value(ty, val) {
                DataResult::Success(v) => {
                    b.put(ty.id_str(), v);
                }
                other => return other,
            }
        }
        DataResult::Success(b.build())
    }
}

/// One entry in a [`ComponentPatch`]: either an added/overridden value or an explicit removal.
#[derive(Debug, Clone, PartialEq)]
pub enum PatchEntry {
    /// Set/override the component to this value.
    Set(Box<ComponentValue>),
    /// Explicitly remove the component from the prototype.
    Removed,
}

/// A patch against a prototype map: maps component types to either a value or a removal marker.
///
/// Preserves entry insertion order for iteration (spec B.3 rule 5) while comparing equal regardless of
/// entry order (two patches with identical key→entry bindings are equal).
#[derive(Debug, Clone, Default)]
pub struct ComponentPatch {
    entries: Vec<(ComponentTypeId, PatchEntry)>,
}

impl PartialEq for ComponentPatch {
    fn eq(&self, other: &Self) -> bool {
        if self.entries.len() != other.entries.len() {
            return false;
        }
        self.entries.iter().all(|(k, v)| other.get(*k) == Some(v))
    }
}

impl ComponentPatch {
    /// An empty patch.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets `ty` to `value` in the patch (replacing any previous entry for `ty`).
    pub fn set(&mut self, ty: ComponentTypeId, value: ComponentValue) {
        self.put_entry(ty, PatchEntry::Set(Box::new(value)));
    }

    /// Marks `ty` as explicitly removed in the patch (replacing any previous entry for `ty`).
    pub fn remove(&mut self, ty: ComponentTypeId) {
        self.put_entry(ty, PatchEntry::Removed);
    }

    /// Removes `ty` from the patch completely (reverts to prototype).
    pub fn clear(&mut self, ty: ComponentTypeId) {
        self.entries.retain(|(k, _)| *k != ty);
    }

    fn put_entry(&mut self, ty: ComponentTypeId, entry: PatchEntry) {
        if let Some((_, existing)) = self.entries.iter_mut().find(|(k, _)| *k == ty) {
            *existing = entry;
        } else {
            self.entries.push((ty, entry));
        }
    }

    /// Looks up `ty` in the patch.
    pub fn get(&self, ty: ComponentTypeId) -> Option<&PatchEntry> {
        self.entries.iter().find(|(k, _)| *k == ty).map(|(_, v)| v)
    }

    /// Whether the patch has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Number of patch entries (sets + removals).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Iterates all patch entries in patch order.
    pub fn iter(&self) -> impl Iterator<Item = (ComponentTypeId, &PatchEntry)> {
        self.entries.iter().map(|(k, v)| (*k, v))
    }

    /// Iterates set entries in patch order.
    pub fn sets(&self) -> impl Iterator<Item = (ComponentTypeId, &ComponentValue)> {
        self.entries.iter().filter_map(|(k, v)| match v {
            PatchEntry::Set(val) => Some((*k, val.as_ref())),
            PatchEntry::Removed => None,
        })
    }

    /// Iterates removed component types in patch order.
    pub fn removals(&self) -> impl Iterator<Item = ComponentTypeId> + '_ {
        self.entries.iter().filter_map(|(k, v)| match v {
            PatchEntry::Removed => Some(*k),
            PatchEntry::Set(_) => None,
        })
    }

    /// Decodes a patch from [`Dyn`] (spec B.4, edge cases 38, 39, 40).
    ///
    /// Both `"x"` and `"!x"` in one input map are distinct keys; entries are processed in input map
    /// iteration order and the last processed entry wins. Partial results keep good entries.
    // PARITY [oracle]: settled by the text-component suite (spec open questions 1 & 5: partial results and duplicate set/remove key order).
    pub fn decode(d: &Dyn, registries: &dyn RegistryContext) -> DataResult<Self> {
        let entries = match read_map(d) {
            DataResult::Success(m) => m,
            other => return other.map(|_| Self::new()),
        };
        let mut patch = Self::new();
        let mut errors = Vec::new();
        for (raw_key, val) in entries {
            let key_str = raw_key.to_string_lossy();
            let (is_remove, id_part) = match key_str.strip_prefix('!') {
                Some(rest) => (true, rest),
                None => (false, key_str.as_ref()),
            };
            let ty = match ComponentTypeId::parse_id(id_part) {
                Ok(t) => t,
                Err(m) => {
                    errors.push(m);
                    continue;
                }
            };
            if ty.is_transient() {
                errors.push(format!("{ty} is not a persistent component"));
                continue;
            }
            if is_remove {
                if !val.is_map() {
                    errors.push(format!(
                        "!{ty}: removal value must be a map, got {}",
                        val.kind()
                    ));
                    continue;
                }
                patch.remove(ty);
            } else {
                match decode_component_value(ty, val, registries) {
                    DataResult::Success(v) => patch.set(ty, v),
                    DataResult::Error { message, partial } => {
                        errors.push(format!("{ty}: {message}"));
                        if let Some(p) = partial {
                            patch.set(ty, p);
                        }
                    }
                    DataResult::Failure(m) => return DataResult::Failure(m),
                }
            }
        }
        if errors.is_empty() {
            DataResult::Success(patch)
        } else {
            DataResult::error_with_partial(errors.join("; "), patch)
        }
    }

    /// Decodes a patch from [`Json`].
    pub fn decode_json(json: &Json, registries: &dyn RegistryContext) -> DataResult<Self> {
        Self::decode(&Dyn::from_json(json), registries)
    }

    /// Decodes a patch from an NBT [`Tag`].
    pub fn decode_nbt(tag: &Tag, registries: &dyn RegistryContext) -> DataResult<Self> {
        Self::decode(&Dyn::from_tag(tag), registries)
    }

    /// Encodes a patch to [`Dyn`] (spec B.4 rule 3): writes every non-transient entry (`id` for sets,
    /// `!id` with `{}` for removals); an empty patch is `{}`.
    // PARITY [oracle]: settled by the text-component suite (spec open question 3: JSON key order).
    pub fn encode(&self) -> DataResult<Dyn> {
        let mut b = MapBuilder::new();
        for (ty, entry) in &self.entries {
            if ty.is_transient() {
                continue;
            }
            match entry {
                PatchEntry::Set(val) => match encode_component_value(*ty, val) {
                    DataResult::Success(v) => {
                        b.put(ty.id_str(), v);
                    }
                    other => return other,
                },
                PatchEntry::Removed => {
                    b.put(format!("!{}", ty.id_str()), Dyn::empty_map());
                }
            }
        }
        DataResult::Success(b.build())
    }

    /// Encodes a patch to [`Json`].
    pub fn encode_json(&self) -> DataResult<Json> {
        self.encode().and_then(|d| match d.to_json() {
            Ok(j) => DataResult::Success(j),
            Err(m) => DataResult::error(m),
        })
    }

    /// Encodes a patch to an NBT [`Tag`].
    pub fn encode_nbt(&self) -> DataResult<Tag> {
        self.encode().and_then(|d| match d.to_tag() {
            Ok(t) => DataResult::Success(t),
            Err(m) => DataResult::error(m),
        })
    }
}

/// A prototype [`ComponentMap`] plus a sanitized [`ComponentPatch`] (spec B.3).
///
/// Invariant: the patch never stores a value equal to the prototype's value or a removal of a type
/// the prototype lacks.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PatchedComponentMap {
    prototype: ComponentMap,
    patch: ComponentPatch,
}

impl PatchedComponentMap {
    /// Creates a patched map with an empty patch over `prototype`.
    pub fn from_prototype(prototype: ComponentMap) -> Self {
        Self {
            prototype,
            patch: ComponentPatch::new(),
        }
    }

    /// Creates a patched map by applying and sanitizing `patch` over `prototype` (spec B.3 rule 4).
    pub fn from_prototype_and_patch(prototype: ComponentMap, patch: &ComponentPatch) -> Self {
        let mut m = Self::from_prototype(prototype);
        m.apply_patch(patch);
        m
    }

    /// The underlying prototype map.
    pub fn prototype(&self) -> &ComponentMap {
        &self.prototype
    }

    /// The sanitized patch.
    pub fn patch(&self) -> &ComponentPatch {
        &self.patch
    }

    /// **Get(type)** (spec B.3 rule 1).
    pub fn get(&self, ty: ComponentTypeId) -> Option<&ComponentValue> {
        match self.patch.get(ty) {
            Some(PatchEntry::Set(v)) => Some(v.as_ref()),
            Some(PatchEntry::Removed) => None,
            None => self.prototype.get(ty),
        }
    }

    /// Whether `ty` is currently present.
    pub fn contains(&self, ty: ComponentTypeId) -> bool {
        self.get(ty).is_some()
    }

    /// **Set(type, value)** (spec B.3 rule 2): drops the patch entry if `value` equals the prototype's
    /// value, otherwise stores it in the patch.
    pub fn set(&mut self, ty: ComponentTypeId, value: ComponentValue) {
        if self.prototype.get(ty) == Some(&value) {
            self.patch.clear(ty);
        } else {
            self.patch.set(ty, value);
        }
    }

    /// **Remove(type)** (spec B.3 rule 3): stores a removal marker if the prototype has `ty`,
    /// otherwise drops any patch entry.
    pub fn remove(&mut self, ty: ComponentTypeId) {
        if self.prototype.contains(ty) {
            self.patch.remove(ty);
        } else {
            self.patch.clear(ty);
        }
    }

    /// Sets `ty` to `Some(v)` or removes it on `None` (spec B.3 rule 2).
    pub fn set_opt(&mut self, ty: ComponentTypeId, value: Option<ComponentValue>) {
        match value {
            Some(v) => self.set(ty, v),
            None => self.remove(ty),
        }
    }

    /// **Applying a patch** (spec B.3 rule 4): applies each entry through `set` / `remove`.
    pub fn apply_patch(&mut self, patch: &ComponentPatch) {
        for (ty, entry) in patch.iter() {
            match entry {
                PatchEntry::Set(v) => self.set(ty, (**v).clone()),
                PatchEntry::Removed => self.remove(ty),
            }
        }
    }

    /// Replaces the patch directly (used to roll back failed validated patch applications).
    pub(crate) fn restore_patch(&mut self, patch: ComponentPatch) {
        self.patch = patch;
    }

    /// Number of active components (spec B.3 rule 5).
    pub fn len(&self) -> usize {
        self.iter().len()
    }

    /// Whether no components are active.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Active components: patched set values first (in patch order), then untouched prototype entries
    /// (spec B.3 rule 5).
    pub fn iter(&self) -> Vec<(ComponentTypeId, &ComponentValue)> {
        let mut out = Vec::new();
        for (ty, val) in self.patch.sets() {
            out.push((ty, val));
        }
        for (ty, val) in self.prototype.iter() {
            if self.patch.get(ty).is_none() {
                out.push((ty, val));
            }
        }
        out
    }

    /// Active component type keys (in iteration order).
    pub fn keys(&self) -> Vec<ComponentTypeId> {
        self.iter().into_iter().map(|(k, _)| k).collect()
    }
}
