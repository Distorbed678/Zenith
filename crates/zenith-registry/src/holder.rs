//! Holders, holder sets, and their network wire encoding.
//!
//! Implements Section 2.6 of `docs/specs/registry/identifiers-registries-tags.md`.

use std::collections::HashSet;
use std::fmt;
use std::sync::{Arc, RwLock};

use thiserror::Error;

use crate::identifier::{
    Identifier, IdentifierError, ResourceKey, TagKey, read_varint, write_varint,
};
use crate::registry::Registry;

/// Errors raised by holder and holder-set operations or wire codecs.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum HolderError {
    /// Attempted to access the resource key of a reference holder before its key was bound.
    #[error("reference holder key is not yet bound")]
    UnboundKey,

    /// Attempted to access the value of a reference holder before its value was bound.
    #[error("reference holder value is not yet bound (key: {key})")]
    UnboundValue {
        /// Key associated with the unbound holder, or `"<unknown>"`.
        key: String,
    },

    /// Attempted to query tag membership on a reference holder before tags were bound.
    #[error("reference holder tags are not yet bound")]
    TagsNotBound,

    /// Attempted to access a named holder set before its contents were bound.
    #[error("named holder set {tag} is not yet bound")]
    HolderSetNotBound {
        /// The tag key of the unbound holder set.
        tag: Box<TagKey>,
    },

    /// Attempted to re-bind a holder to a different key or value.
    #[error("holder is already bound to a different {kind}")]
    AlreadyBound {
        /// `"key"` or `"value"`.
        kind: &'static str,
    },

    /// A direct (unregistered) holder was used where only registered reference holders are allowed.
    #[error("direct (unregistered) holder is not permitted here")]
    DirectHolderNotAllowed,

    /// A reference holder belonged to a different registry than expected.
    #[error("holder belongs to registry {actual}, expected {expected}")]
    OwnerMismatch {
        /// Expected registry key.
        expected: Box<ResourceKey>,
        /// Actual registry key.
        actual: Box<ResourceKey>,
    },

    /// Wire encoding/decoding failed.
    #[error("holder wire error: {0}")]
    Wire(String),

    /// Identifier decoding failed while reading a named holder set.
    #[error(transparent)]
    Identifier(#[from] IdentifierError),

    /// Referenced registry ID or tag was absent during wire decoding.
    #[error("missing registry target during wire decode: {0}")]
    MissingTarget(String),
}

struct ReferenceState<T> {
    key: Option<ResourceKey>,
    value: Option<Arc<T>>,
    tags: Option<HashSet<TagKey>>,
}

struct ReferenceInner<T> {
    owner: ResourceKey,
    state: RwLock<ReferenceState<T>>,
}

/// A reference holder belonging to a specific registry owner.
pub struct ReferenceHolder<T> {
    inner: Arc<ReferenceInner<T>>,
}

impl<T> Clone for ReferenceHolder<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> fmt::Debug for ReferenceHolder<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.inner.state.read().expect("holder lock poisoned");
        f.debug_struct("ReferenceHolder")
            .field("owner", &self.inner.owner)
            .field("key", &state.key)
            .field("value_bound", &state.value.is_some())
            .field("tags_bound", &state.tags.is_some())
            .finish()
    }
}

impl<T> ReferenceHolder<T> {
    /// Creates a stand-alone placeholder holder bound to a key, with value unbound.
    pub(crate) fn new_placeholder(owner: ResourceKey, key: ResourceKey) -> Self {
        Self {
            inner: Arc::new(ReferenceInner {
                owner,
                state: RwLock::new(ReferenceState {
                    key: Some(key),
                    value: None,
                    tags: None,
                }),
            }),
        }
    }

    /// Creates an intrusive holder bound to a value, with key unbound.
    pub(crate) fn new_intrusive(owner: ResourceKey, value: Arc<T>) -> Self {
        Self {
            inner: Arc::new(ReferenceInner {
                owner,
                state: RwLock::new(ReferenceState {
                    key: None,
                    value: Some(value),
                    tags: None,
                }),
            }),
        }
    }

    /// Creates a fully bound reference holder with both key and value set.
    pub(crate) fn new_bound(owner: ResourceKey, key: ResourceKey, value: Arc<T>) -> Self {
        Self {
            inner: Arc::new(ReferenceInner {
                owner,
                state: RwLock::new(ReferenceState {
                    key: Some(key),
                    value: Some(value),
                    tags: None,
                }),
            }),
        }
    }

    /// Returns the registry owner key.
    pub fn owner(&self) -> &ResourceKey {
        &self.inner.owner
    }

    /// Returns `true` if both key and value are bound.
    pub fn is_bound(&self) -> bool {
        let state = self.inner.state.read().expect("holder lock poisoned");
        state.key.is_some() && state.value.is_some()
    }

    /// Returns `true` if the value is bound.
    pub fn has_value(&self) -> bool {
        self.inner
            .state
            .read()
            .expect("holder lock poisoned")
            .value
            .is_some()
    }

    /// Returns the bound [`ResourceKey`], or [`HolderError::UnboundKey`] if not yet bound.
    pub fn key(&self) -> Result<ResourceKey, HolderError> {
        self.inner
            .state
            .read()
            .expect("holder lock poisoned")
            .key
            .clone()
            .ok_or(HolderError::UnboundKey)
    }

    /// Returns the bound value, or [`HolderError::UnboundValue`] if not yet bound.
    pub fn value(&self) -> Result<Arc<T>, HolderError> {
        let state = self.inner.state.read().expect("holder lock poisoned");
        state
            .value
            .clone()
            .ok_or_else(|| HolderError::UnboundValue {
                key: state
                    .key
                    .as_ref()
                    .map(|k| k.location().to_string())
                    .unwrap_or_else(|| "<unknown>".to_owned()),
            })
    }

    /// Checks whether this holder belongs to `tag`. Fails if tags have not been bound yet.
    pub fn matches_tag(&self, tag: &TagKey) -> Result<bool, HolderError> {
        let state = self.inner.state.read().expect("holder lock poisoned");
        let Some(tags) = &state.tags else {
            return Err(HolderError::TagsNotBound);
        };
        Ok(tags.contains(tag))
    }

    /// Returns the set of bound tags on this holder, or [`HolderError::TagsNotBound`].
    pub fn bound_tags(&self) -> Result<Vec<TagKey>, HolderError> {
        let state = self.inner.state.read().expect("holder lock poisoned");
        let Some(tags) = &state.tags else {
            return Err(HolderError::TagsNotBound);
        };
        let mut out: Vec<TagKey> = tags.iter().cloned().collect();
        out.sort();
        Ok(out)
    }

    pub(crate) fn bind_key(&self, key: ResourceKey) -> Result<(), HolderError> {
        let mut state = self.inner.state.write().expect("holder lock poisoned");
        if let Some(existing) = &state.key {
            if existing != &key {
                return Err(HolderError::AlreadyBound { kind: "key" });
            }
            return Ok(());
        }
        state.key = Some(key);
        Ok(())
    }

    pub(crate) fn bind_value(&self, value: Arc<T>) -> Result<(), HolderError> {
        let mut state = self.inner.state.write().expect("holder lock poisoned");
        if let Some(existing) = &state.value {
            if !Arc::ptr_eq(existing, &value) {
                return Err(HolderError::AlreadyBound { kind: "value" });
            }
            return Ok(());
        }
        state.value = Some(value);
        Ok(())
    }

    pub(crate) fn bind_tags(&self, tags: HashSet<TagKey>) {
        let mut state = self.inner.state.write().expect("holder lock poisoned");
        state.tags = Some(tags);
    }
}

/// Either a registered [`ReferenceHolder`] or an inline unregistered [`Direct`](Holder::Direct) value.
pub enum Holder<T> {
    /// A reference holder belonging to a specific registry.
    Reference(ReferenceHolder<T>),
    /// An inline, unregistered value.
    Direct(Arc<T>),
}

impl<T> Clone for Holder<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Reference(r) => Self::Reference(r.clone()),
            Self::Direct(v) => Self::Direct(Arc::clone(v)),
        }
    }
}

impl<T> fmt::Debug for Holder<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reference(r) => f.debug_tuple("Holder::Reference").field(r).finish(),
            Self::Direct(_) => f.write_str("Holder::Direct([unregistered])"),
        }
    }
}

impl<T> Holder<T> {
    /// Wraps an unregistered inline value in a direct holder.
    pub fn direct(value: T) -> Self {
        Self::Direct(Arc::new(value))
    }

    /// Wraps an existing `Arc<T>` in a direct holder.
    pub fn direct_arc(value: Arc<T>) -> Self {
        Self::Direct(value)
    }

    /// Returns `true` if this is a direct (unregistered) holder.
    pub fn is_direct(&self) -> bool {
        matches!(self, Self::Direct(_))
    }

    /// Returns `true` if this is a reference holder.
    pub fn is_reference(&self) -> bool {
        matches!(self, Self::Reference(_))
    }

    /// Returns `true` if the holder is bound (always `true` for direct holders).
    pub fn is_bound(&self) -> bool {
        match self {
            Self::Reference(r) => r.is_bound(),
            Self::Direct(_) => true,
        }
    }

    /// Returns the registry owner key for a reference holder, or `None` for a direct holder.
    pub fn owner(&self) -> Option<&ResourceKey> {
        match self {
            Self::Reference(r) => Some(r.owner()),
            Self::Direct(_) => None,
        }
    }

    /// Returns `Ok(None)` for direct holders, `Ok(Some(key))` for bound reference holders,
    /// or `Err(HolderError::UnboundKey)` for unbound reference holders.
    pub fn key(&self) -> Result<Option<ResourceKey>, HolderError> {
        match self {
            Self::Reference(r) => r.key().map(Some),
            Self::Direct(_) => Ok(None),
        }
    }

    /// Returns the wrapped value, failing if this is an unbound reference holder.
    pub fn value(&self) -> Result<Arc<T>, HolderError> {
        match self {
            Self::Reference(r) => r.value(),
            Self::Direct(v) => Ok(Arc::clone(v)),
        }
    }

    /// Display name: `"[unregistered]"` for direct holders, or the key's canonical identifier.
    pub fn display_name(&self) -> String {
        match self {
            Self::Direct(_) => "[unregistered]".to_owned(),
            Self::Reference(r) => r
                .key()
                .map(|k| k.location().to_string())
                .unwrap_or_else(|_| "[unbound]".to_owned()),
        }
    }

    /// Returns `true` if this holder is a reference holder whose key matches `id`.
    /// Direct holders always return `false`.
    pub fn matches_id(&self, id: &Identifier) -> bool {
        match self {
            Self::Direct(_) => false,
            Self::Reference(r) => r.key().map(|k| k.location() == id).unwrap_or(false),
        }
    }

    /// Returns `true` if this holder is a reference holder whose key equals `key`.
    /// Direct holders always return `false`.
    pub fn matches_key(&self, key: &ResourceKey) -> bool {
        match self {
            Self::Direct(_) => false,
            Self::Reference(r) => r.key().map(|k| &k == key).unwrap_or(false),
        }
    }

    /// Returns `true` if this holder is a reference holder matching the predicate on its key.
    /// Direct holders always return `false`.
    pub fn matches_predicate(&self, predicate: impl FnOnce(&ResourceKey) -> bool) -> bool {
        match self {
            Self::Direct(_) => false,
            Self::Reference(r) => r.key().map(|k| predicate(&k)).unwrap_or(false),
        }
    }

    /// Checks whether this holder belongs to `tag`.
    /// Direct holders always return `Ok(false)`. Reference holders fail before tags are bound.
    pub fn matches_tag(&self, tag: &TagKey) -> Result<bool, HolderError> {
        match self {
            Self::Direct(_) => Ok(false),
            Self::Reference(r) => r.matches_tag(tag),
        }
    }

    /// Matches against another holder per Section 2.6:
    /// - Reference holder: delegates to `other.matches_key(&self_key)`.
    /// - Direct holder: compares wrapped values by value equality.
    pub fn matches_holder(&self, other: &Holder<T>) -> bool
    where
        T: PartialEq,
    {
        match self {
            Self::Reference(r) => r.key().map(|k| other.matches_key(&k)).unwrap_or(false),
            Self::Direct(a) => match other {
                Self::Direct(b) => a == b,
                Self::Reference(_) => false,
            },
        }
    }

    /// Verifies that this holder can be serialized in a registry context with `expected_owner`.
    pub fn check_owner(&self, expected_owner: &ResourceKey) -> Result<(), HolderError> {
        if let Self::Reference(r) = self
            && r.owner() != expected_owner
        {
            return Err(HolderError::OwnerMismatch {
                expected: Box::new(expected_owner.clone()),
                actual: Box::new(r.owner().clone()),
            });
        }
        Ok(())
    }
}

struct NamedHolderSetInner<T> {
    tag_key: TagKey,
    contents: RwLock<Option<Vec<Holder<T>>>>,
}

/// A named holder set backed by a [`TagKey`] and bound to a list of reference holders.
pub struct NamedHolderSet<T> {
    inner: Arc<NamedHolderSetInner<T>>,
}

impl<T> Clone for NamedHolderSet<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> fmt::Debug for NamedHolderSet<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let guard = self
            .inner
            .contents
            .read()
            .expect("holder set lock poisoned");
        f.debug_struct("NamedHolderSet")
            .field("tag_key", &self.inner.tag_key)
            .field("bound", &guard.is_some())
            .field("len", &guard.as_ref().map(Vec::len))
            .finish()
    }
}

impl<T> NamedHolderSet<T> {
    /// Creates an initially unbound named holder set for `tag_key`.
    pub(crate) fn new_unbound(tag_key: TagKey) -> Self {
        Self {
            inner: Arc::new(NamedHolderSetInner {
                tag_key,
                contents: RwLock::new(None),
            }),
        }
    }

    /// Creates a named holder set immediately bound to `contents`.
    pub(crate) fn new_bound(tag_key: TagKey, contents: Vec<Holder<T>>) -> Self {
        Self {
            inner: Arc::new(NamedHolderSetInner {
                tag_key,
                contents: RwLock::new(Some(contents)),
            }),
        }
    }

    /// Returns this named set's [`TagKey`].
    pub fn tag_key(&self) -> &TagKey {
        &self.inner.tag_key
    }

    /// Returns `true` if contents have been bound.
    pub fn is_bound(&self) -> bool {
        self.inner
            .contents
            .read()
            .expect("holder set lock poisoned")
            .is_some()
    }

    /// Returns a cloned list of the bound holders, or [`HolderError::HolderSetNotBound`].
    pub fn contents(&self) -> Result<Vec<Holder<T>>, HolderError> {
        self.inner
            .contents
            .read()
            .expect("holder set lock poisoned")
            .clone()
            .ok_or_else(|| HolderError::HolderSetNotBound {
                tag: Box::new(self.inner.tag_key.clone()),
            })
    }

    /// Checks membership via the holder's reverse tag-membership set (per Section 2.6),
    /// rather than scanning the element list.
    pub fn contains(&self, holder: &Holder<T>) -> Result<bool, HolderError> {
        holder.matches_tag(&self.inner.tag_key)
    }

    pub(crate) fn bind(&self, contents: Vec<Holder<T>>) {
        let mut guard = self
            .inner
            .contents
            .write()
            .expect("holder set lock poisoned");
        *guard = Some(contents);
    }
}

/// Either a named (tag-backed) holder set or a direct list of holders.
pub enum HolderSet<T> {
    /// Tag-backed named holder set.
    Named(NamedHolderSet<T>),
    /// Explicit immutable list of holders.
    Direct(Vec<Holder<T>>),
}

impl<T> Clone for HolderSet<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Named(n) => Self::Named(n.clone()),
            Self::Direct(v) => Self::Direct(v.clone()),
        }
    }
}

impl<T> fmt::Debug for HolderSet<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Named(n) => f.debug_tuple("HolderSet::Named").field(n).finish(),
            Self::Direct(v) => f.debug_tuple("HolderSet::Direct").field(v).finish(),
        }
    }
}

impl<T> HolderSet<T> {
    /// Creates a direct holder set from an explicit list of holders.
    pub fn direct(holders: Vec<Holder<T>>) -> Self {
        Self::Direct(holders)
    }

    /// Returns the tag key if this is a named set.
    pub fn tag_key(&self) -> Option<&TagKey> {
        match self {
            Self::Named(n) => Some(n.tag_key()),
            Self::Direct(_) => None,
        }
    }

    /// Returns the holders in this set, failing if this is an unbound named set.
    pub fn contents(&self) -> Result<Vec<Holder<T>>, HolderError> {
        match self {
            Self::Named(n) => n.contents(),
            Self::Direct(v) => Ok(v.clone()),
        }
    }

    /// Checks whether `holder` is a member of this set.
    pub fn contains(&self, holder: &Holder<T>) -> Result<bool, HolderError>
    where
        T: PartialEq,
    {
        match self {
            Self::Named(n) => n.contains(holder),
            Self::Direct(list) => Ok(list.iter().any(|h| h.matches_holder(holder))),
        }
    }
}

/// Encodes a registry-only network holder as `VarInt(registry_id)`.
pub fn encode_holder_registry_only<T>(
    registry: &Registry<T>,
    holder: &Holder<T>,
    buf: &mut Vec<u8>,
) -> Result<(), HolderError> {
    let Holder::Reference(r) = holder else {
        return Err(HolderError::DirectHolderNotAllowed);
    };
    holder.check_owner(registry.key())?;
    let val = r.value()?;
    let id = registry.id_of_exact(&val).ok_or_else(|| {
        HolderError::Wire(format!(
            "holder {} is not registered",
            holder.display_name()
        ))
    })?;
    write_varint(buf, id as i32);
    Ok(())
}

/// Decodes a registry-only network holder from `VarInt(registry_id)`.
/// Out-of-range IDs fail.
pub fn decode_holder_registry_only<T>(
    registry: &Registry<T>,
    bytes: &[u8],
) -> Result<(Holder<T>, usize), HolderError> {
    let (raw_id, used) = read_varint(bytes).map_err(HolderError::Wire)?;
    let holder = registry.get_holder_by_id(raw_id).ok_or_else(|| {
        HolderError::MissingTarget(format!(
            "registry {} has no entry at numeric ID {raw_id}",
            registry.key().location()
        ))
    })?;
    Ok((holder, used))
}

/// Encodes an inline-capable network holder using the spec's off-by-one framing:
/// - Direct holder: `VarInt(0)` followed by inline element payload.
/// - Reference holder: `VarInt(registry_id + 1)`.
pub fn encode_holder_with_inline<T, E>(
    registry: &Registry<T>,
    holder: &Holder<T>,
    buf: &mut Vec<u8>,
    encode_inline: impl FnOnce(&T, &mut Vec<u8>) -> Result<(), E>,
) -> Result<(), HolderError>
where
    E: fmt::Display,
{
    match holder {
        Holder::Direct(val) => {
            write_varint(buf, 0);
            encode_inline(val, buf).map_err(|e| HolderError::Wire(e.to_string()))?;
            Ok(())
        }
        Holder::Reference(r) => {
            holder.check_owner(registry.key())?;
            let val = r.value()?;
            let id = registry.id_of_exact(&val).ok_or_else(|| {
                HolderError::Wire(format!(
                    "holder {} is not registered",
                    holder.display_name()
                ))
            })?;
            write_varint(buf, (id as i32) + 1);
            Ok(())
        }
    }
}

/// Decodes an inline-capable network holder using the spec's off-by-one framing:
/// - `0` -> decodes inline payload into [`Holder::Direct`].
/// - `id + 1` (`> 0`) -> looks up reference holder at `id` (out-of-range fails).
pub fn decode_holder_with_inline<T, E>(
    registry: &Registry<T>,
    bytes: &[u8],
    decode_inline: impl FnOnce(&[u8]) -> Result<(T, usize), E>,
) -> Result<(Holder<T>, usize), HolderError>
where
    E: fmt::Display,
{
    let (raw, header) = read_varint(bytes).map_err(HolderError::Wire)?;
    if raw < 0 {
        return Err(HolderError::Wire(format!(
            "negative holder wire discriminator {raw}"
        )));
    }
    if raw == 0 {
        let (val, body_len) =
            decode_inline(&bytes[header..]).map_err(|e| HolderError::Wire(e.to_string()))?;
        return Ok((Holder::direct(val), header + body_len));
    }
    let reg_id = raw - 1;
    let holder = registry.get_holder_by_id(reg_id).ok_or_else(|| {
        HolderError::MissingTarget(format!(
            "registry {} has no entry at numeric ID {reg_id}",
            registry.key().location()
        ))
    })?;
    Ok((holder, header))
}

/// Encodes a network [`HolderSet`] using the spec's off-by-one framing:
/// - Named set: `VarInt(0)` followed by the tag's network identifier.
/// - Direct set: `VarInt(count + 1)` followed by `count` registry-only holder IDs.
pub fn encode_holder_set<T>(
    registry: &Registry<T>,
    set: &HolderSet<T>,
    buf: &mut Vec<u8>,
) -> Result<(), HolderError> {
    match set {
        HolderSet::Named(named) => {
            if !named.tag_key().is_for(registry.key()) {
                return Err(HolderError::OwnerMismatch {
                    expected: Box::new(registry.key().clone()),
                    actual: Box::new(named.tag_key().registry().clone()),
                });
            }
            write_varint(buf, 0);
            named.tag_key().location().encode_network(buf)?;
            Ok(())
        }
        HolderSet::Direct(holders) => {
            write_varint(buf, (holders.len() as i32) + 1);
            for holder in holders {
                encode_holder_registry_only(registry, holder, buf)?;
            }
            Ok(())
        }
    }
}

/// Decodes a network [`HolderSet`] using the spec's off-by-one framing:
/// - `0` followed by tag identifier -> resolves the named tag in `registry` (missing tag fails).
/// - `count + 1` -> reads `count` registry-only holder IDs (any out-of-range ID fails).
pub fn decode_holder_set<T>(
    registry: &Registry<T>,
    bytes: &[u8],
) -> Result<(HolderSet<T>, usize), HolderError> {
    let (raw, mut offset) = read_varint(bytes).map_err(HolderError::Wire)?;
    if raw < 0 {
        return Err(HolderError::Wire(format!(
            "negative holder-set wire discriminator {raw}"
        )));
    }
    if raw == 0 {
        let (tag_id, consumed) = Identifier::decode_network(&bytes[offset..])?;
        offset += consumed;
        let tag_key = TagKey::new(registry.key().clone(), tag_id);
        let named = registry.get_tag(&tag_key).ok_or_else(|| {
            HolderError::MissingTarget(format!(
                "missing tag {} in registry {}",
                tag_key.location(),
                registry.key().location()
            ))
        })?;
        return Ok((HolderSet::Named(named), offset));
    }
    let count = (raw - 1) as usize;
    let mut holders = Vec::with_capacity(count);
    for _ in 0..count {
        let (holder, consumed) = decode_holder_registry_only(registry, &bytes[offset..])?;
        offset += consumed;
        holders.push(holder);
    }
    Ok((HolderSet::Direct(holders), offset))
}
