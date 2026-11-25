//! Global block-state (and fluid-state) ID computation and verification.
//!
//! Implements Section 3 of `docs/specs/registry/identifiers-registries-tags.md`.

use std::collections::{BTreeMap, HashMap, HashSet};

use thiserror::Error;
use zenith_data::Tables;

use crate::identifier::{Identifier, IdentifierError};

/// Returns `true` if `s` matches `^[a-z0-9_]+$` (Section 3.1.1).
pub fn is_valid_property_name(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_'))
}

/// Errors raised when validating properties or building a [`BlockStateTable`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BlockStateError {
    /// Property name did not match `^[a-z0-9_]+$`.
    #[error("invalid property name '{name}' (must match ^[a-z0-9_]+$)")]
    InvalidPropertyName {
        /// Offending property name.
        name: String,
    },

    /// Property value did not match `^[a-z0-9_]+$`.
    #[error("invalid value '{value}' for property '{property}' (must match ^[a-z0-9_]+$)")]
    InvalidPropertyValue {
        /// Owning property name.
        property: String,
        /// Offending value string.
        value: String,
    },

    /// Property had fewer than 2 values (`size <= 1`).
    #[error("property '{name}' must have at least 2 values, got {count}")]
    InsufficientCardinality {
        /// Property name.
        name: String,
        /// Number of values supplied.
        count: usize,
    },

    /// Integer property bounds were invalid (`max <= min`).
    #[error("integer property '{name}' has invalid range [{min}, {max}]")]
    InvalidIntegerRange {
        /// Property name.
        name: String,
        /// Minimum bound.
        min: u32,
        /// Maximum bound.
        max: u32,
    },

    /// Duplicate value within a single property.
    #[error("duplicate value '{value}' in property '{property}'")]
    DuplicatePropertyValue {
        /// Property name.
        property: String,
        /// Duplicate value.
        value: String,
    },

    /// Duplicate property name within a single block.
    #[error("duplicate property '{property}' on block {block}")]
    DuplicatePropertyName {
        /// Block identifier.
        block: Identifier,
        /// Duplicate property name.
        property: String,
    },

    /// Duplicate block identifier when building a table.
    #[error("duplicate block identifier in state table: {block}")]
    DuplicateBlock {
        /// Duplicate block identifier.
        block: Identifier,
    },

    /// A block in `minecraft:block` was missing from `Tables::blocks`.
    #[error("block {block} from minecraft:block registry not found in Tables::blocks")]
    MissingBlockDefinition {
        /// Missing block name.
        block: String,
    },

    /// Invalid block identifier while ingesting from `Tables`.
    #[error(transparent)]
    Identifier(#[from] IdentifierError),
}

/// Kind of a block/fluid state property (Section 3.1.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropertyKind {
    /// Boolean property with fixed value order `["true", "false"]`.
    Boolean,
    /// Integer property `[min, max]` (`max > min`) in ascending numeric order.
    Integer {
        /// Inclusive minimum (`>= 0`).
        min: u32,
        /// Inclusive maximum (`> min`).
        max: u32,
    },
    /// Enum property preserving the supplied declaration order.
    Enum,
}

/// A validated block/fluid state property definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyDef {
    name: String,
    kind: PropertyKind,
    values: Vec<String>,
}

impl PropertyDef {
    /// Creates a boolean property with canonical value order `["true", "false"]`.
    pub fn boolean(name: impl Into<String>) -> Result<Self, BlockStateError> {
        let name = name.into();
        if !is_valid_property_name(&name) {
            return Err(BlockStateError::InvalidPropertyName { name });
        }
        Ok(Self {
            name,
            kind: PropertyKind::Boolean,
            values: vec!["true".to_owned(), "false".to_owned()],
        })
    }

    /// Creates an integer property for `[min, max]` (`min >= 0`, `max > min`) in ascending order.
    pub fn integer(name: impl Into<String>, min: u32, max: u32) -> Result<Self, BlockStateError> {
        let name = name.into();
        if !is_valid_property_name(&name) {
            return Err(BlockStateError::InvalidPropertyName { name });
        }
        if max <= min {
            return Err(BlockStateError::InvalidIntegerRange { name, min, max });
        }
        let values = (min..=max).map(|v| v.to_string()).collect();
        Ok(Self {
            name,
            kind: PropertyKind::Integer { min, max },
            values,
        })
    }

    /// Creates an enum property preserving the exact order of `values`.
    pub fn enumeration(
        name: impl Into<String>,
        values: Vec<String>,
    ) -> Result<Self, BlockStateError> {
        let name = name.into();
        if !is_valid_property_name(&name) {
            return Err(BlockStateError::InvalidPropertyName { name });
        }
        if values.len() < 2 {
            return Err(BlockStateError::InsufficientCardinality {
                name,
                count: values.len(),
            });
        }
        let mut seen = HashSet::with_capacity(values.len());
        for value in &values {
            if !is_valid_property_name(value) {
                return Err(BlockStateError::InvalidPropertyValue {
                    property: name,
                    value: value.clone(),
                });
            }
            if !seen.insert(value.as_str()) {
                return Err(BlockStateError::DuplicatePropertyValue {
                    property: name,
                    value: value.clone(),
                });
            }
        }
        Ok(Self {
            name,
            kind: PropertyKind::Enum,
            values,
        })
    }

    /// Constructs a [`PropertyDef`] from raw ingested `(name, values)`, applying Section 3.1.4's
    /// canonical ordering for booleans (`true` before `false`) and contiguous integer ranges
    /// (ascending numeric order), while preserving enum declaration order.
    pub fn from_raw(name: impl Into<String>, values: Vec<String>) -> Result<Self, BlockStateError> {
        let name = name.into();
        if values.len() == 2
            && values.iter().any(|v| v == "true")
            && values.iter().any(|v| v == "false")
        {
            return Self::boolean(name);
        }

        if values.len() >= 2
            && let Some((min, max)) = detect_contiguous_int_range(&values)
        {
            return Self::integer(name, min, max);
        }

        Self::enumeration(name, values)
    }

    /// Returns the property name.
    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the property kind.
    #[inline]
    pub fn kind(&self) -> &PropertyKind {
        &self.kind
    }

    /// Returns the ordered possible values.
    #[inline]
    pub fn values(&self) -> &[String] {
        &self.values
    }
}

fn detect_contiguous_int_range(values: &[String]) -> Option<(u32, u32)> {
    let mut parsed = Vec::with_capacity(values.len());
    for raw in values {
        let n: u32 = raw.parse().ok()?;
        if n.to_string() != *raw {
            return None;
        }
        parsed.push(n);
    }
    parsed.sort_unstable();
    parsed.dedup();
    if parsed.len() != values.len() || parsed.len() < 2 {
        return None;
    }
    let min = parsed[0];
    let max = *parsed.last()?;
    if (max - min) as usize + 1 == parsed.len() {
        Some((min, max))
    } else {
        None
    }
}

/// Definition of a block (or fluid) with its lexicographically sorted properties.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockDef {
    id: Identifier,
    properties: Vec<PropertyDef>,
    default_values: Option<BTreeMap<String, String>>,
}

impl BlockDef {
    /// Creates a [`BlockDef`], validating unique property names and sorting properties
    /// lexicographically by property name (ASCII order, Section 3.1.3).
    pub fn new(id: Identifier, mut properties: Vec<PropertyDef>) -> Result<Self, BlockStateError> {
        properties.sort_by(|a, b| a.name.cmp(&b.name));
        for pair in properties.windows(2) {
            if pair[0].name == pair[1].name {
                return Err(BlockStateError::DuplicatePropertyName {
                    block: id,
                    property: pair[0].name.clone(),
                });
            }
        }
        Ok(Self {
            id,
            properties,
            default_values: None,
        })
    }

    /// Sets the explicit default-state property values for this block.
    pub fn with_default_values(mut self, values: BTreeMap<String, String>) -> Self {
        self.default_values = Some(values);
        self
    }

    /// Returns this block's identifier.
    #[inline]
    pub fn id(&self) -> &Identifier {
        &self.id
    }

    /// Returns this block's properties in sorted lexicographical order.
    #[inline]
    pub fn properties(&self) -> &[PropertyDef] {
        &self.properties
    }
}

/// Summary of one block inside a [`BlockStateTable`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockEntry {
    /// Block registration protocol ID (`0, 1, ...`).
    pub block_protocol_id: u32,
    /// Block identifier.
    pub id: Identifier,
    /// Properties in sorted lexicographical order.
    pub properties: Vec<PropertyDef>,
    /// First global state ID belonging to this block.
    pub first_state: u32,
    /// Number of states belonging to this block (`>= 1`).
    pub state_count: u32,
    /// Default global state ID for this block.
    pub default_state: u32,
}

/// One computed global block state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockState {
    /// Dense global state ID (`0, 1, ...`).
    pub global_id: u32,
    /// Index into [`BlockStateTable::blocks`].
    pub block_index: u32,
    /// Owning block's identifier.
    pub block_id: Identifier,
    /// Sorted `(property_name, property_value)` pairs.
    pub properties: Vec<(String, String)>,
}

impl BlockState {
    /// Looks up the value of property `name` on this state.
    pub fn property_value(&self, name: &str) -> Option<&str> {
        self.properties
            .binary_search_by(|(k, _)| k.as_str().cmp(name))
            .ok()
            .map(|idx| self.properties[idx].1.as_str())
    }
}

/// A single mismatch reported by [`BlockStateTable::diff_against`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockStateMismatch {
    /// Global state ID where the mismatch occurred.
    pub global_id: usize,
    /// Computed state summary, if present.
    pub computed: Option<String>,
    /// Ingested `Tables` state summary, if present.
    pub ingested: Option<String>,
}

/// Dense global block-state (or fluid-state) ID table computed strictly from block
/// registration order and sorted property Cartesian products (Section 3).
#[derive(Debug, Clone)]
pub struct BlockStateTable {
    blocks: Vec<BlockEntry>,
    blocks_by_id: HashMap<Identifier, usize>,
    states: Vec<BlockState>,
    air_default_state: Option<u32>,
}

impl BlockStateTable {
    /// Computes global block-state IDs from an ordered list of [`BlockDef`]s in block
    /// registration order (`0, 1, ...`).
    ///
    /// For each block, enumerates the Cartesian product of its sorted properties in
    /// row-major (odometer) order where the **last** property cycles fastest.
    pub fn from_blocks(block_defs: Vec<BlockDef>) -> Result<Self, BlockStateError> {
        let mut blocks = Vec::with_capacity(block_defs.len());
        let mut blocks_by_id = HashMap::with_capacity(block_defs.len());
        let mut states = Vec::new();
        let air_id =
            Identifier::with_default_namespace("air").expect("minecraft:air is a valid identifier");
        let mut air_default_state = None;

        for (block_idx, def) in block_defs.into_iter().enumerate() {
            if blocks_by_id.contains_key(&def.id) {
                return Err(BlockStateError::DuplicateBlock { block: def.id });
            }

            let first_state = states.len() as u32;
            let prop_count = def.properties.len();
            let mut default_state = first_state;

            if prop_count == 0 {
                states.push(BlockState {
                    global_id: first_state,
                    block_index: block_idx as u32,
                    block_id: def.id.clone(),
                    properties: Vec::new(),
                });
            } else {
                // Row-major (odometer) enumeration: last property varies fastest.
                let mut indices = vec![0usize; prop_count];
                loop {
                    let global_id = states.len() as u32;
                    let props: Vec<(String, String)> = def
                        .properties
                        .iter()
                        .enumerate()
                        .map(|(i, p)| (p.name.clone(), p.values[indices[i]].clone()))
                        .collect();

                    if let Some(expected_defaults) = &def.default_values
                        && props
                            .iter()
                            .all(|(k, v)| expected_defaults.get(k).is_some_and(|dv| dv == v))
                    {
                        default_state = global_id;
                    }

                    states.push(BlockState {
                        global_id,
                        block_index: block_idx as u32,
                        block_id: def.id.clone(),
                        properties: props,
                    });

                    // Advance odometer from the last property backwards.
                    let mut pos = prop_count;
                    let mut carry = true;
                    while pos > 0 && carry {
                        pos -= 1;
                        indices[pos] += 1;
                        if indices[pos] < def.properties[pos].values.len() {
                            carry = false;
                        } else {
                            indices[pos] = 0;
                        }
                    }
                    if carry {
                        break;
                    }
                }
            }

            let state_count = states.len() as u32 - first_state;
            if def.id == air_id {
                air_default_state = Some(default_state);
            }

            blocks_by_id.insert(def.id.clone(), block_idx);
            blocks.push(BlockEntry {
                block_protocol_id: block_idx as u32,
                id: def.id,
                properties: def.properties,
                first_state,
                state_count,
                default_state,
            });
        }

        // If `minecraft:air` wasn't present (e.g. a small synthetic or fluid table),
        // default fallback points to state 0 when non-empty.
        if air_default_state.is_none() && !states.is_empty() {
            air_default_state = Some(0);
        }

        Ok(Self {
            blocks,
            blocks_by_id,
            states,
            air_default_state,
        })
    }

    /// Builds a [`BlockStateTable`] from `zenith_data::Tables` by independently computing
    /// global state IDs from the `minecraft:block` registry's protocol ID order and each
    /// block's property definitions (ignoring ingested state IDs and input property order).
    pub fn from_tables(tables: &Tables) -> Result<Self, BlockStateError> {
        let blocks_by_name: HashMap<&str, &zenith_data::Block> =
            tables.blocks.iter().map(|b| (b.name.as_str(), b)).collect();

        // Determine block registration order from `minecraft:block` registry protocol IDs.
        let ordered_block_names: Vec<&str> = if let Some(block_reg) =
            tables.registry("minecraft:block")
        {
            let mut entries: Vec<&zenith_data::RegistryEntry> = block_reg.entries.iter().collect();
            entries.sort_by_key(|e| e.protocol_id);
            entries.into_iter().map(|e| e.name.as_str()).collect()
        } else {
            tables.blocks.iter().map(|b| b.name.as_str()).collect()
        };

        let mut defs = Vec::with_capacity(ordered_block_names.len());
        for name in ordered_block_names {
            let block = blocks_by_name.get(name).copied().ok_or_else(|| {
                BlockStateError::MissingBlockDefinition {
                    block: name.to_owned(),
                }
            })?;
            let id = Identifier::parse(&block.name)?;
            let mut prop_defs = Vec::with_capacity(block.properties.len());
            for prop in &block.properties {
                prop_defs.push(PropertyDef::from_raw(
                    prop.name.clone(),
                    prop.values.clone(),
                )?);
            }

            let mut def = BlockDef::new(id, prop_defs)?;

            // Capture default-state property values if available in `tables.states`.
            if let Some(default_row) = tables.states.get(block.default_state as usize) {
                let mut defaults = BTreeMap::new();
                for (prop, &val_idx) in block.properties.iter().zip(&default_row.values) {
                    if let Some(val) = prop.values.get(val_idx as usize) {
                        defaults.insert(prop.name.clone(), val.clone());
                    }
                }
                def = def.with_default_values(defaults);
            }

            defs.push(def);
        }

        Self::from_blocks(defs)
    }

    /// Total number of global states in this table.
    #[inline]
    pub fn len(&self) -> usize {
        self.states.len()
    }

    /// Returns `true` if the table has zero states.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }

    /// Returns all block entries in registration order.
    #[inline]
    pub fn blocks(&self) -> &[BlockEntry] {
        &self.blocks
    }

    /// Looks up a block entry by identifier.
    pub fn block(&self, id: &Identifier) -> Option<&BlockEntry> {
        self.blocks_by_id.get(id).map(|&idx| &self.blocks[idx])
    }

    /// Returns the default state for `minecraft:air` (or state 0 if `minecraft:air` is absent).
    pub fn air_default_state(&self) -> Option<&BlockState> {
        self.air_default_state
            .and_then(|id| self.states.get(id as usize))
    }

    /// Looks up a state by global ID with Section 3.2.3 fallback behavior:
    /// out-of-range IDs (`< 0` or `>= len`) fall back to `minecraft:air`'s default state.
    pub fn state_by_id(&self, global_id: i32) -> Option<&BlockState> {
        if global_id >= 0
            && let Some(state) = self.states.get(global_id as usize)
        {
            return Some(state);
        }
        self.air_default_state()
    }

    /// Exact lookup by global ID with no fallback.
    pub fn state_by_id_exact(&self, global_id: i32) -> Option<&BlockState> {
        if global_id < 0 {
            return None;
        }
        self.states.get(global_id as usize)
    }

    /// Computes the exact global state ID for `(block_id, properties)` with no fallback.
    ///
    /// `properties` may be provided in any order, but must specify every property of `block_id`
    /// with a valid value (and no unknown properties).
    pub fn id_of_exact(&self, block_id: &Identifier, properties: &[(&str, &str)]) -> Option<u32> {
        let entry = self.block(block_id)?;
        if properties.len() != entry.properties.len() {
            return None;
        }
        if entry.properties.is_empty() {
            return Some(entry.first_state);
        }

        let mut offset: u32 = 0;
        for prop in &entry.properties {
            let (_, val) = properties.iter().find(|(k, _)| *k == prop.name())?;
            let val_idx = prop.values().iter().position(|v| v == val)? as u32;
            offset = offset * (prop.values().len() as u32) + val_idx;
        }
        Some(entry.first_state + offset)
    }

    /// Looks up the global state ID for `(block_id, properties)` with Section 3.2.3 fallback:
    /// returns `0` if the block or property combination is absent/unregistered.
    pub fn id_of(&self, block_id: &Identifier, properties: &[(&str, &str)]) -> u32 {
        self.id_of_exact(block_id, properties).unwrap_or(0)
    }

    /// Looks up the global state ID of a [`BlockState`] with Section 3.2.3 fallback (`0` if absent).
    pub fn id_of_state(&self, state: &BlockState) -> u32 {
        let pairs: Vec<(&str, &str)> = state
            .properties
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        self.id_of(&state.block_id, &pairs)
    }

    /// Verifies every computed global state ID against the ingested [`Tables`].
    ///
    /// Returns `(matched, total)` where `total = max(self.len(), tables.states.len())`.
    /// A state matches iff both tables assign the exact same block identifier and identical
    /// `(property_name -> property_value)` pairs to that global state ID.
    pub fn verify_against(&self, tables: &Tables) -> (usize, usize) {
        let total = self.states.len().max(tables.states.len());
        let common = self.states.len().min(tables.states.len());
        let mut matched = 0usize;

        for id in 0..common {
            if state_matches_ingested(&self.states[id], tables, id) {
                matched += 1;
            }
        }

        (matched, total)
    }

    /// Returns up to `limit` detailed mismatches against `tables` for diagnostics.
    pub fn diff_against(&self, tables: &Tables, limit: usize) -> Vec<BlockStateMismatch> {
        let total = self.states.len().max(tables.states.len());
        let mut diffs = Vec::new();
        for id in 0..total {
            if diffs.len() >= limit {
                break;
            }
            let matches = id < self.states.len()
                && id < tables.states.len()
                && state_matches_ingested(&self.states[id], tables, id);
            if !matches {
                let computed = self.states.get(id).map(format_computed_state);
                let ingested = tables
                    .states
                    .get(id)
                    .map(|row| format_ingested_state(tables, row));
                diffs.push(BlockStateMismatch {
                    global_id: id,
                    computed,
                    ingested,
                });
            }
        }
        diffs
    }
}

fn state_matches_ingested(computed: &BlockState, tables: &Tables, id: usize) -> bool {
    let row = &tables.states[id];
    let Some(block) = tables.blocks.get(row.block as usize) else {
        return false;
    };
    if computed.block_id.to_string() != block.name {
        return false;
    }
    if computed.properties.len() != block.properties.len()
        || row.values.len() != block.properties.len()
    {
        return false;
    }
    for (prop, &val_idx) in block.properties.iter().zip(&row.values) {
        let Some(expected_val) = prop.values.get(val_idx as usize) else {
            return false;
        };
        if computed.property_value(&prop.name) != Some(expected_val.as_str()) {
            return false;
        }
    }
    true
}

fn format_computed_state(state: &BlockState) -> String {
    if state.properties.is_empty() {
        return state.block_id.to_string();
    }
    let props: Vec<String> = state
        .properties
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    format!("{}[{}]", state.block_id, props.join(","))
}

fn format_ingested_state(tables: &Tables, row: &zenith_data::StateRow) -> String {
    let Some(block) = tables.blocks.get(row.block as usize) else {
        return format!("<invalid block index {}>", row.block);
    };
    if block.properties.is_empty() {
        return block.name.clone();
    }
    let mut pairs: Vec<(&str, &str)> = block
        .properties
        .iter()
        .zip(&row.values)
        .map(|(p, &vi)| {
            let v = p.values.get(vi as usize).map(String::as_str).unwrap_or("?");
            (p.name.as_str(), v)
        })
        .collect();
    pairs.sort_by_key(|(k, _)| *k);
    let props: Vec<String> = pairs.into_iter().map(|(k, v)| format!("{k}={v}")).collect();
    format!("{}[{}]", block.name, props.join(","))
}
