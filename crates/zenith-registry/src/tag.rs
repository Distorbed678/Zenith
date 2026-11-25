//! Tag JSON parsing, multi-pack merging, two-pass dependency resolution, and wire codecs.
//!
//! Implements Section 4 of `docs/specs/registry/identifiers-registries-tags.md`.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::hash::Hash;

use serde::Deserialize;
use thiserror::Error;

use crate::holder::{Holder, HolderError};
use crate::identifier::{Identifier, IdentifierError, read_varint, write_varint};
use crate::registry::Registry;

/// One entry inside a tag JSON file's `values` array (Section 4.1).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TagEntry {
    /// Target identifier (element ID or nested tag ID, without any leading `#`).
    pub id: Identifier,
    /// `true` if this entry references a nested tag (`#<id>`), `false` for a direct element.
    pub is_tag: bool,
    /// Whether a missing target fails tag resolution (`true` by default).
    pub required: bool,
}

impl TagEntry {
    /// Creates a required direct element reference.
    pub fn element(id: Identifier) -> Self {
        Self {
            id,
            is_tag: false,
            required: true,
        }
    }

    /// Creates an optional direct element reference (`required: false`).
    pub fn optional_element(id: Identifier) -> Self {
        Self {
            id,
            is_tag: false,
            required: false,
        }
    }

    /// Creates a required nested tag reference (`#<id>`).
    pub fn tag(id: Identifier) -> Self {
        Self {
            id,
            is_tag: true,
            required: true,
        }
    }

    /// Creates an optional nested tag reference (`#<id>`, `required: false`).
    pub fn optional_tag(id: Identifier) -> Self {
        Self {
            id,
            is_tag: true,
            required: false,
        }
    }

    /// Parses a tag entry identifier string (`"<id>"` or `"#<id>"`) with explicit `required`.
    pub fn parse_id(raw: &str, required: bool) -> Result<Self, IdentifierError> {
        if let Some(rest) = raw.strip_prefix('#') {
            Ok(Self {
                id: Identifier::parse(rest)?,
                is_tag: true,
                required,
            })
        } else {
            Ok(Self {
                id: Identifier::parse(raw)?,
                is_tag: false,
                required,
            })
        }
    }

    /// Formats the entry's identifier (`#<id>` for nested tags, `<id>` for elements).
    pub fn display_ref(&self) -> String {
        if self.is_tag {
            format!("#{}", self.id)
        } else {
            self.id.to_string()
        }
    }
}

/// Decoded contents of a single tag JSON file (`values` + optional `replace`, Section 4.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagFile {
    /// Ordered entries declared in `values`.
    pub values: Vec<TagEntry>,
    /// Whether this file clears entries accumulated from lower-priority packs.
    pub replace: bool,
}

#[derive(Deserialize)]
struct RawTagFile {
    values: Vec<RawTagEntry>,
    #[serde(default)]
    replace: bool,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawTagEntry {
    Shorthand(String),
    Object {
        id: String,
        #[serde(default = "default_required")]
        required: bool,
    },
}

const fn default_required() -> bool {
    true
}

impl TagFile {
    /// Parses a strict RFC 8259 JSON byte slice into a [`TagFile`].
    pub fn parse_json(bytes: &[u8]) -> Result<Self, TagParseError> {
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|e| TagParseError::JsonSyntax(e.to_string()))?;
        Self::from_json_value(value)
    }

    /// Decodes an already syntax-parsed [`serde_json::Value`] against the tag-file schema.
    pub fn from_json_value(value: serde_json::Value) -> Result<Self, TagParseError> {
        let raw: RawTagFile =
            serde_json::from_value(value).map_err(|e| TagParseError::Schema(e.to_string()))?;
        let mut entries = Vec::with_capacity(raw.values.len());
        for item in raw.values {
            let entry = match item {
                RawTagEntry::Shorthand(s) => TagEntry::parse_id(&s, true)?,
                RawTagEntry::Object { id, required } => TagEntry::parse_id(&id, required)?,
            };
            entries.push(entry);
        }
        Ok(Self {
            values: entries,
            replace: raw.replace,
        })
    }
}

/// Errors raised while parsing a single tag JSON file.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TagParseError {
    /// Strict JSON syntax error (RFC 8259).
    #[error("invalid JSON syntax: {0}")]
    JsonSyntax(String),

    /// JSON was syntactically valid, but did not conform to the tag file schema.
    #[error("invalid tag file schema: {0}")]
    Schema(String),

    /// An entry's identifier failed validation.
    #[error(transparent)]
    Identifier(#[from] IdentifierError),
}

/// One raw tag file yielded by a [`TagSource`]: `(pack_index, tag_id, Result<json_bytes, err>)`.
///
/// `pack_index` orders packs from lowest priority (`0`, e.g. `vanilla`) to highest priority.
pub type TagSourceItem = (usize, Identifier, Result<Vec<u8>, String>);

/// Abstraction over tag file providers (in-memory tests, zip/jar data packs, filesystem packs).
pub trait TagSource {
    /// Yields `(pack_index, tag_id, Result<json_bytes, err>)` items across all packs.
    fn iter_tag_files(&self) -> Box<dyn Iterator<Item = TagSourceItem> + '_>;

    /// Optional human-readable pack name for diagnostics (defaults to `pack#<index>`).
    fn pack_name(&self, pack_index: usize) -> Option<&str> {
        let _ = pack_index;
        None
    }

    /// Optional pack indices that define a `filter` section capable of clearing lower-priority
    /// tag files (Section 4.2.1). Defaults to empty.
    fn filter_pack_indices(&self) -> Vec<usize> {
        Vec::new()
    }

    /// Returns `true` if `pack_index`'s `filter` section matches `tag_id` and therefore clears
    /// all tag files from lower-priority packs (`< pack_index`). Defaults to `false`.
    fn pack_filters_tag(&self, _pack_index: usize, _tag_id: &Identifier) -> bool {
        false
    }
}

/// Convenience in-memory [`TagSource`] implementation for tests and callers.
#[derive(Debug, Clone, Default)]
pub struct MemoryTagSource {
    items: Vec<TagSourceItem>,
    pack_names: BTreeMap<usize, String>,
    filters: BTreeMap<usize, HashSet<Identifier>>,
}

impl MemoryTagSource {
    /// Creates an empty in-memory tag source.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets a human-readable pack name for `pack_index`.
    pub fn with_pack_name(mut self, pack_index: usize, name: impl Into<String>) -> Self {
        self.pack_names.insert(pack_index, name.into());
        self
    }

    /// Adds a raw tag file entry.
    pub fn add_file(
        &mut self,
        pack_index: usize,
        tag_id: Identifier,
        content: Result<Vec<u8>, String>,
    ) {
        self.items.push((pack_index, tag_id, content));
    }

    /// Adds a UTF-8 JSON string as a tag file entry.
    pub fn add_json(&mut self, pack_index: usize, tag_id: Identifier, json: impl Into<String>) {
        self.items
            .push((pack_index, tag_id, Ok(json.into().into_bytes())));
    }

    /// Marks `tag_id` as filtered by `pack_index` (clearing lower-priority packs' files).
    pub fn add_filter(&mut self, pack_index: usize, tag_id: Identifier) {
        self.filters.entry(pack_index).or_default().insert(tag_id);
    }
}

impl TagSource for MemoryTagSource {
    fn iter_tag_files(&self) -> Box<dyn Iterator<Item = TagSourceItem> + '_> {
        Box::new(self.items.iter().cloned())
    }

    fn pack_name(&self, pack_index: usize) -> Option<&str> {
        self.pack_names.get(&pack_index).map(String::as_str)
    }

    fn filter_pack_indices(&self) -> Vec<usize> {
        self.filters.keys().copied().collect()
    }

    fn pack_filters_tag(&self, pack_index: usize, tag_id: &Identifier) -> bool {
        self.filters
            .get(&pack_index)
            .is_some_and(|set| set.contains(tag_id))
    }
}

/// A tag entry annotated with its originating pack metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackedTagEntry {
    /// The parsed tag entry.
    pub entry: TagEntry,
    /// Pack priority index (`0` = lowest).
    pub pack_index: usize,
    /// Pack display name.
    pub pack_name: String,
}

/// Non-fatal error recorded while reading, parsing, or resolving tags.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TagDiagnostic {
    /// Failed to read raw bytes for a tag file from a pack.
    #[error("failed to read tag {tag_id} from pack '{pack_name}': {reason}")]
    ReadError {
        /// Tag identifier.
        tag_id: Identifier,
        /// Pack index.
        pack_index: usize,
        /// Pack display name.
        pack_name: String,
        /// Underlying I/O or source error message.
        reason: String,
    },

    /// Failed strict JSON syntax parsing; no tag entry was inserted.
    #[error("invalid JSON syntax in tag {tag_id} from pack '{pack_name}': {reason}")]
    JsonSyntaxError {
        /// Tag identifier.
        tag_id: Identifier,
        /// Pack index.
        pack_index: usize,
        /// Pack display name.
        pack_name: String,
        /// Parser error message.
        reason: String,
    },

    /// JSON syntax succeeded (inserting an empty builder slot if new) but tag schema decoding failed.
    #[error("invalid tag schema in tag {tag_id} from pack '{pack_name}': {reason}")]
    SchemaError {
        /// Tag identifier.
        tag_id: Identifier,
        /// Pack index.
        pack_index: usize,
        /// Pack display name.
        pack_name: String,
        /// Schema error message.
        reason: String,
    },

    /// One or more required references were missing when resolving `tag_id`; `tag_id` was omitted.
    #[error("tag {tag_id} failed to load due to missing references: {missing:?}")]
    MissingReferences {
        /// Tag identifier that failed to resolve.
        tag_id: Identifier,
        /// All missing required references (`(<#id or id>, pack_name)`) collected together.
        missing: Vec<(String, String)>,
    },
}

/// Lookup abstraction used during tag resolution (Section 4.3 & 4.4).
pub trait TagElementLookup<E> {
    /// Key type used for first-seen deduplication of resolved elements.
    type DedupKey: Eq + Hash;

    /// Looks up element `id`.
    ///
    /// - Frozen static registries check only registered entries regardless of `required`.
    /// - Writable dynamic registries create an unbound placeholder when `required == true`
    ///   and check only already-registered entries when `required == false`.
    fn lookup(&mut self, id: &Identifier, required: bool) -> Option<E>;

    /// Extracts the deduplication key for a resolved element.
    fn dedup_key(element: &E) -> Self::DedupKey;
}

/// Static/frozen read-only lookup over `&Registry<T>`.
impl<T> TagElementLookup<Holder<T>> for &Registry<T> {
    type DedupKey = Identifier;

    fn lookup(&mut self, id: &Identifier, _required: bool) -> Option<Holder<T>> {
        self.get_holder(id)
    }

    fn dedup_key(element: &Holder<T>) -> Self::DedupKey {
        element
            .key()
            .ok()
            .flatten()
            .expect("registry lookup always returns a reference holder with a key")
            .location()
            .clone()
    }
}

/// Static or dynamic lookup over `&mut Registry<T>` implementing the Section 4.4 divergence
/// (Edge Case 7):
/// - If frozen: read-only lookup; missing required elements return `None` and fail only the tag.
/// - If writable (loading dynamic registry): required lookups create an unbound placeholder
///   holder (so the tag succeeds and registry freezing later fails with unbound keys), while
///   optional lookups check only already-registered entries and silently skip missing IDs.
impl<T> TagElementLookup<Holder<T>> for &mut Registry<T> {
    type DedupKey = Identifier;

    fn lookup(&mut self, id: &Identifier, required: bool) -> Option<Holder<T>> {
        if self.is_frozen() || !required {
            self.get_holder(id)
        } else {
            self.get_or_create_placeholder(id).ok()
        }
    }

    fn dedup_key(element: &Holder<T>) -> Self::DedupKey {
        element
            .key()
            .ok()
            .flatten()
            .expect("registry lookup always returns a reference holder with a key")
            .location()
            .clone()
    }
}

/// Result of loading and resolving tags from a [`TagSource`].
#[derive(Debug)]
pub struct TagLoadResult<E> {
    /// Successfully resolved tags, each containing deduplicated elements in first-seen order.
    pub resolved: BTreeMap<Identifier, Vec<E>>,
    /// All diagnostics logged during reading, parsing, merging, and resolution.
    pub diagnostics: Vec<TagDiagnostic>,
}

/// Multi-pack tag merger and two-pass dependency resolver (Section 4.2–4.4).
#[derive(Debug, Default)]
pub struct TagLoader;

impl TagLoader {
    /// Phase 1: reads, stacks, filters, parses, and merges tag files across packs from lowest
    /// to highest priority, reproducing Section 4.2's exact parse-failure quirks (Edge Case 8).
    pub fn load_builders(
        source: &impl TagSource,
    ) -> (
        BTreeMap<Identifier, Vec<TrackedTagEntry>>,
        Vec<TagDiagnostic>,
    ) {
        let mut diagnostics = Vec::new();
        type TagFileStack = Vec<(usize, Result<Vec<u8>, String>)>;
        let mut stacks: BTreeMap<Identifier, TagFileStack> = BTreeMap::new();

        for (pack_index, tag_id, content) in source.iter_tag_files() {
            stacks
                .entry(tag_id)
                .or_default()
                .push((pack_index, content));
        }

        let mut filter_packs = source.filter_pack_indices();
        filter_packs.sort_unstable();
        filter_packs.dedup();

        let mut builders: BTreeMap<Identifier, Vec<TrackedTagEntry>> = BTreeMap::new();

        for (tag_id, mut stack) in stacks {
            // Stable sort from lowest priority pack_index to highest priority pack_index.
            stack.sort_by_key(|(pack_index, _)| *pack_index);

            // Apply higher-priority pack filters (Section 4.2.1): if pack F filters tag_id,
            // discard all files from packs strictly below F.
            if let Some(&highest_filter_pack) = filter_packs
                .iter()
                .rev()
                .find(|&&p| source.pack_filters_tag(p, &tag_id))
            {
                stack.retain(|(pack_index, _)| *pack_index >= highest_filter_pack);
            }

            for (pack_index, content) in stack {
                let pack_name = source
                    .pack_name(pack_index)
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("pack#{pack_index}"));

                let bytes = match content {
                    Ok(b) => b,
                    Err(reason) => {
                        diagnostics.push(TagDiagnostic::ReadError {
                            tag_id: tag_id.clone(),
                            pack_index,
                            pack_name,
                            reason,
                        });
                        continue;
                    }
                };

                // Step 2.1: strict JSON syntax check. Malformed JSON does NOT insert a builder entry.
                let json_value: serde_json::Value = match serde_json::from_slice(&bytes) {
                    Ok(v) => v,
                    Err(err) => {
                        diagnostics.push(TagDiagnostic::JsonSyntaxError {
                            tag_id: tag_id.clone(),
                            pack_index,
                            pack_name,
                            reason: err.to_string(),
                        });
                        continue;
                    }
                };

                // Step 2.2 (Quirk): insert mutable entry list AFTER JSON syntax succeeds and
                // BEFORE tag-file schema decoding runs.
                let entry_list = builders.entry(tag_id.clone()).or_default();

                // Step 2.3: decode schema; on failure, skip file and preserve earlier entries.
                let tag_file = match TagFile::from_json_value(json_value) {
                    Ok(f) => f,
                    Err(err) => {
                        diagnostics.push(TagDiagnostic::SchemaError {
                            tag_id: tag_id.clone(),
                            pack_index,
                            pack_name,
                            reason: err.to_string(),
                        });
                        continue;
                    }
                };

                // Step 2.4: replace clears lower-priority accumulated entries.
                if tag_file.replace {
                    entry_list.clear();
                }

                // Step 2.5: append values in order with pack provenance.
                for entry in tag_file.values {
                    entry_list.push(TrackedTagEntry {
                        entry,
                        pack_index,
                        pack_name: pack_name.clone(),
                    });
                }
            }
        }

        (builders, diagnostics)
    }

    /// Phase 2: builds the two-pass cycle-free dependency graph (Pass 1 required, Pass 2
    /// optional), runs depth-first post-order traversal, and resolves each tag with
    /// first-seen deduplication (Section 4.3 & 4.4).
    pub fn resolve<E, L>(
        builders: &BTreeMap<Identifier, Vec<TrackedTagEntry>>,
        lookup: &mut L,
        mut diagnostics: Vec<TagDiagnostic>,
    ) -> TagLoadResult<E>
    where
        E: Clone,
        L: TagElementLookup<E>,
    {
        // Build cycle-free dependency graph: tag_id -> ordered set of dependency tag_ids.
        let mut graph: BTreeMap<Identifier, Vec<Identifier>> = BTreeMap::new();

        // Pass 1: required nested tags.
        for (tag_id, entries) in builders {
            for tracked in entries {
                if tracked.entry.is_tag && tracked.entry.required {
                    let dep = &tracked.entry.id;
                    if !is_reachable(&graph, dep, tag_id) {
                        add_edge(&mut graph, tag_id, dep);
                    }
                }
            }
        }

        // Pass 2: optional nested tags.
        for (tag_id, entries) in builders {
            for tracked in entries {
                if tracked.entry.is_tag && !tracked.entry.required {
                    let dep = &tracked.entry.id;
                    if !is_reachable(&graph, dep, tag_id) {
                        add_edge(&mut graph, tag_id, dep);
                    }
                }
            }
        }

        let mut visited: BTreeSet<Identifier> = BTreeSet::new();
        let mut resolved: BTreeMap<Identifier, Vec<E>> = BTreeMap::new();

        let all_tags: Vec<Identifier> = builders.keys().cloned().collect();
        for tag_id in all_tags {
            if !visited.contains(&tag_id) {
                dfs_visit(
                    &tag_id,
                    builders,
                    &graph,
                    &mut visited,
                    &mut resolved,
                    lookup,
                    &mut diagnostics,
                );
            }
        }

        TagLoadResult {
            resolved,
            diagnostics,
        }
    }

    /// Convenience method running both [`load_builders`](Self::load_builders) and
    /// [`resolve`](Self::resolve).
    pub fn load_and_resolve<E, L>(source: &impl TagSource, lookup: &mut L) -> TagLoadResult<E>
    where
        E: Clone,
        L: TagElementLookup<E>,
    {
        let (builders, diagnostics) = Self::load_builders(source);
        Self::resolve(&builders, lookup, diagnostics)
    }
}

/// Returns `true` if `target` is reachable from `start` via a **non-empty** directed path
/// in `graph`.
///
/// Note that for a self-edge `A -> A` when `A` has no outgoing edges yet, this returns
/// `false` so `A -> A` is added and handled consistently during DFS (Section 4.4).
fn is_reachable(
    graph: &BTreeMap<Identifier, Vec<Identifier>>,
    start: &Identifier,
    target: &Identifier,
) -> bool {
    let mut stack: Vec<&Identifier> = match graph.get(start) {
        Some(neighbors) => neighbors.iter().collect(),
        None => return false,
    };
    let mut seen: HashSet<&Identifier> = HashSet::new();

    while let Some(node) = stack.pop() {
        if node == target {
            return true;
        }
        if seen.insert(node)
            && let Some(neighbors) = graph.get(node)
        {
            for next in neighbors {
                stack.push(next);
            }
        }
    }
    false
}

fn add_edge(graph: &mut BTreeMap<Identifier, Vec<Identifier>>, from: &Identifier, to: &Identifier) {
    let list = graph.entry(from.clone()).or_default();
    if !list.contains(to) {
        list.push(to.clone());
    }
}

fn dfs_visit<E, L>(
    tag_id: &Identifier,
    builders: &BTreeMap<Identifier, Vec<TrackedTagEntry>>,
    graph: &BTreeMap<Identifier, Vec<Identifier>>,
    visited: &mut BTreeSet<Identifier>,
    resolved: &mut BTreeMap<Identifier, Vec<E>>,
    lookup: &mut L,
    diagnostics: &mut Vec<TagDiagnostic>,
) where
    E: Clone,
    L: TagElementLookup<E>,
{
    if !visited.insert(tag_id.clone()) {
        return;
    }

    if let Some(deps) = graph.get(tag_id) {
        for dep in deps {
            if !visited.contains(dep) {
                dfs_visit(dep, builders, graph, visited, resolved, lookup, diagnostics);
            }
        }
    }

    let Some(entries) = builders.get(tag_id) else {
        return;
    };

    let mut seen_keys: HashSet<L::DedupKey> = HashSet::new();
    let mut values: Vec<E> = Vec::new();
    let mut missing: Vec<(String, String)> = Vec::new();

    for tracked in entries {
        if tracked.entry.is_tag {
            if let Some(dep_elements) = resolved.get(&tracked.entry.id) {
                for elem in dep_elements {
                    if seen_keys.insert(L::dedup_key(elem)) {
                        values.push(elem.clone());
                    }
                }
            } else if tracked.entry.required {
                missing.push((tracked.entry.display_ref(), tracked.pack_name.clone()));
            }
        } else if let Some(elem) = lookup.lookup(&tracked.entry.id, tracked.entry.required) {
            if seen_keys.insert(L::dedup_key(&elem)) {
                values.push(elem);
            }
        } else if tracked.entry.required {
            missing.push((tracked.entry.display_ref(), tracked.pack_name.clone()));
        }
    }

    if missing.is_empty() {
        resolved.insert(tag_id.clone(), values);
    } else {
        diagnostics.push(TagDiagnostic::MissingReferences {
            tag_id: tag_id.clone(),
            missing,
        });
    }
}

/// Errors raised during `update_tags` wire serialization/deserialization (Section 4.5).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TagWireError {
    /// Attempted to serialize a direct (unregistered) holder inside a tag.
    #[error("cannot serialize direct (unregistered) holder in tag {tag_id}")]
    DirectHolderNotAllowed {
        /// Tag identifier.
        tag_id: Identifier,
    },

    /// Holder error while reading tag contents or numeric IDs.
    #[error(transparent)]
    Holder(#[from] HolderError),

    /// Identifier wire error.
    #[error(transparent)]
    Identifier(#[from] IdentifierError),

    /// Malformed wire framing (negative counts, truncated VarInt, etc.).
    #[error("tag wire framing error: {0}")]
    Wire(String),
}

/// Encodes one registry's tag map in `update_tags` format:
/// `VarInt(tag_count)` followed by `(tag_id, VarInt(len), [VarInt(numeric_id); len])`.
///
/// Rejects any direct (unregistered) holder inside a tag (Section 4.5).
pub fn encode_registry_tags<T>(
    registry: &Registry<T>,
    buf: &mut Vec<u8>,
) -> Result<(), TagWireError> {
    write_varint(buf, registry.tag_count() as i32);
    for (tag_id, set) in registry.tags() {
        tag_id.encode_network(buf)?;
        let contents = set.contents()?;
        write_varint(buf, contents.len() as i32);
        for holder in contents {
            let Holder::Reference(r) = holder else {
                return Err(TagWireError::DirectHolderNotAllowed {
                    tag_id: tag_id.clone(),
                });
            };
            let val = r.value()?;
            let id = registry.id_of_exact(&val).ok_or_else(|| {
                TagWireError::Wire(format!(
                    "holder {} in tag {tag_id} is not registered",
                    r.key()
                        .map(|k| k.location().to_string())
                        .unwrap_or_else(|_| "<unbound>".to_owned())
                ))
            })?;
            write_varint(buf, id as i32);
        }
    }
    Ok(())
}

/// Encodes a multi-registry `update_tags` payload from a list of `(registry_id, encoded_tags_bytes)`,
/// omitting registries with zero tags (Section 4.5).
pub fn encode_update_tags_payload<T>(
    registries: &[&Registry<T>],
    buf: &mut Vec<u8>,
) -> Result<(), TagWireError> {
    let non_empty: Vec<&Registry<T>> = registries
        .iter()
        .copied()
        .filter(|r| r.tag_count() > 0)
        .collect();
    write_varint(buf, non_empty.len() as i32);
    for reg in non_empty {
        reg.key().location().encode_network(buf)?;
        encode_registry_tags(reg, buf)?;
    }
    Ok(())
}

/// Decoded tag map produced by [`decode_registry_tags`].
pub type DecodedTagMap<T> = BTreeMap<Identifier, Vec<Holder<T>>>;

/// Decodes one registry's tag map from `update_tags` wire bytes (Section 4.5, Edge Case 11).
///
/// Any out-of-range numeric ID is **silently dropped** rather than failing decoding.
pub fn decode_registry_tags<T>(
    registry: &Registry<T>,
    bytes: &[u8],
) -> Result<(DecodedTagMap<T>, usize), TagWireError> {
    let (tag_count, mut offset) = read_varint(bytes).map_err(TagWireError::Wire)?;
    if tag_count < 0 {
        return Err(TagWireError::Wire(format!(
            "negative tag count {tag_count}"
        )));
    }
    let mut out = BTreeMap::new();
    for _ in 0..tag_count {
        let (tag_id, used) = Identifier::decode_network(&bytes[offset..])?;
        offset += used;
        let (elem_count, used) = read_varint(&bytes[offset..]).map_err(TagWireError::Wire)?;
        offset += used;
        if elem_count < 0 {
            return Err(TagWireError::Wire(format!(
                "negative element count {elem_count} in tag {tag_id}"
            )));
        }
        let mut holders = Vec::with_capacity(elem_count as usize);
        for _ in 0..elem_count {
            let (raw_id, used) = read_varint(&bytes[offset..]).map_err(TagWireError::Wire)?;
            offset += used;
            // Out-of-range numeric IDs are silently dropped per Section 4.5 / Edge Case 11.
            if let Some(holder) = registry.get_holder_by_id(raw_id) {
                holders.push(holder);
            }
        }
        out.insert(tag_id, holders);
    }
    Ok((out, offset))
}
