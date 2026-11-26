//! Per-engine layer storage: section states, the updating / visible layer maps, queued data and the sky-only
//! top-section bookkeeping (`data-layers.md` §3–§9).

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::Arc;

use crate::layer::DataLayer;
use crate::pos::{
    block_to_section, block_x, block_y, block_z, section_node, section_offset, section_x,
    section_y, section_z, section_zero,
};

/// Fast multiplicative hasher for packed node keys.
#[derive(Debug, Default, Clone, Copy)]
pub struct NodeHasher(u64);

impl Hasher for NodeHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0.rotate_left(5) ^ u64::from(b)).wrapping_mul(0x517C_C1B7_2722_0A95);
        }
    }
    fn write_i64(&mut self, i: i64) {
        self.0 = ((i as u64) ^ (i as u64 >> 29)).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        self.0 ^= self.0 >> 32;
    }
    fn write_u64(&mut self, i: u64) {
        self.write_i64(i as i64);
    }
}

/// `HashMap` keyed by packed nodes.
pub type NodeMap<V> = HashMap<i64, V, BuildHasherDefault<NodeHasher>>;
/// `HashSet` of packed nodes.
pub type NodeSet = HashSet<i64, BuildHasherDefault<NodeHasher>>;

/// A shared, copy-on-write layer handle.
pub type LayerRef = Arc<DataLayer>;

/// An insertion-ordered set of section nodes (the order of change notifications is made deterministic).
#[derive(Debug, Default, Clone)]
struct OrderedSet {
    order: Vec<i64>,
    seen: NodeSet,
    last: Option<i64>,
}

impl OrderedSet {
    fn insert(&mut self, node: i64) {
        if self.last == Some(node) {
            return;
        }
        self.last = Some(node);
        if self.seen.insert(node) {
            self.order.push(node);
        }
    }
    fn take(&mut self) -> Vec<i64> {
        self.seen.clear();
        self.last = None;
        std::mem::take(&mut self.order)
    }
    fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

/// Section state bit 5: the section has data.
const HAS_DATA: u8 = 32;
const COUNT_MASK: u8 = 31;

/// The layer map plus the sky-only top-section data (`SkyDataLayerStorageMap`).
#[derive(Debug, Clone)]
struct SectionMap {
    layers: NodeMap<LayerRef>,
    /// zero node → one past the highest section Y with a layer.
    top: NodeMap<i32>,
    /// Lowest section Y for which any layer was ever added (also the default of `top`).
    lowest: i32,
}

impl SectionMap {
    fn new() -> Self {
        Self {
            layers: NodeMap::default(),
            top: NodeMap::default(),
            lowest: i32::MAX,
        }
    }

    #[inline]
    fn top_of(&self, zero: i64) -> i32 {
        self.top.get(&zero).copied().unwrap_or(self.lowest)
    }
}

/// A queued layer (from NBT or a packet) waiting for its section, or a marker that the section adopted it.
#[derive(Debug, Clone)]
enum Queued {
    Data(LayerRef),
    /// `createDataLayer` handed the queued object to the updating map; both names refer to one layer.
    Adopted,
}

/// Which data layer a storage serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LightLayer {
    /// Block light.
    Block,
    /// Sky light.
    Sky,
}

/// One engine's layer storage (`LayerLightSectionStorage` and its sky subclass).
#[derive(Debug, Clone)]
pub struct LayerStorage {
    layer: LightLayer,
    section_states: NodeMap<u8>,
    columns_with_sources: NodeSet,
    updating: SectionMap,
    visible: SectionMap,
    changed_sections: NodeSet,
    affected: OrderedSet,
    queued: NodeMap<Queued>,
    retain: NodeSet,
    to_remove: NodeSet,
    has_inconsistencies: bool,
}

impl LayerStorage {
    /// An empty storage for `layer`.
    pub fn new(layer: LightLayer) -> Self {
        Self {
            layer,
            section_states: NodeMap::default(),
            columns_with_sources: NodeSet::default(),
            updating: SectionMap::new(),
            visible: SectionMap::new(),
            changed_sections: NodeSet::default(),
            affected: OrderedSet::default(),
            queued: NodeMap::default(),
            retain: NodeSet::default(),
            to_remove: NodeSet::default(),
            has_inconsistencies: false,
        }
    }

    /// Which layer this storage serves.
    pub const fn layer(&self) -> LightLayer {
        self.layer
    }

    const fn is_sky(&self) -> bool {
        matches!(self.layer, LightLayer::Sky)
    }

    // ----- section status -------------------------------------------------------------------------------------

    /// `updateSectionStatus(sectionNode, sectionEmpty)` (§4).
    pub fn update_section_status(&mut self, node: i64, section_empty: bool) {
        let state = self.section_states.get(&node).copied().unwrap_or(0);
        let new_state = if section_empty {
            state & !HAS_DATA
        } else {
            state | HAS_DATA
        };
        if new_state == state {
            return;
        }
        self.put_section_state(node, new_state);
        let inc: i8 = if section_empty { -1 } else { 1 };
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if dx == 0 && dy == 0 && dz == 0 {
                        continue;
                    }
                    let n = section_offset(node, dx, dy, dz);
                    let ns = self.section_states.get(&n).copied().unwrap_or(0);
                    let count = (ns & COUNT_MASK) as i8 + inc;
                    assert!(
                        (0..=26).contains(&count),
                        "inconsistent section status calls: neighbour count {count}"
                    );
                    self.put_section_state(n, (ns & !COUNT_MASK) | count as u8);
                }
            }
        }
    }

    fn put_section_state(&mut self, node: i64, state: u8) {
        if state != 0 {
            let old = self.section_states.insert(node, state);
            if old.is_none() {
                self.initialize_section(node);
            }
        } else if self.section_states.remove(&node).is_some() {
            self.remove_section(node);
        }
    }

    fn initialize_section(&mut self, node: i64) {
        if self.to_remove.remove(&node) {
            return;
        }
        let layer = self.create_data_layer(node);
        self.updating.layers.insert(node, layer);
        self.changed_sections.insert(node);
        if self.is_sky() {
            self.on_node_added(node);
        }
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    self.affected.insert(section_offset(node, dx, dy, dz));
                }
            }
        }
        self.has_inconsistencies = true;
    }

    fn remove_section(&mut self, node: i64) {
        self.to_remove.insert(node);
        self.has_inconsistencies = true;
    }

    /// `createDataLayer(node)`: the queued layer if present (adopted), else a block / sky default (§5, §9).
    fn create_data_layer(&mut self, node: i64) -> LayerRef {
        if let Some(q) = self.queued.get_mut(&node)
            && let Queued::Data(layer) = q
        {
            let layer = layer.clone();
            *q = Queued::Adopted;
            return layer;
        }
        if self.is_sky() {
            let zero = section_zero(node);
            let top = self.updating.top_of(zero);
            if top != self.updating.lowest && section_y(node) < top {
                let mut probe = section_offset(node, 0, 1, 0);
                while section_y(probe) < top {
                    if let Some(l) = self.updating.layers.get(&probe) {
                        return Arc::new(l.repeat_first_layer());
                    }
                    probe = section_offset(probe, 0, 1, 0);
                }
            }
            if self.light_on_in_section(node) {
                return Arc::new(DataLayer::filled(15));
            }
        }
        Arc::new(DataLayer::new())
    }

    fn on_node_added(&mut self, node: i64) {
        let y = section_y(node);
        if y < self.updating.lowest {
            self.updating.lowest = y;
        }
        let zero = section_zero(node);
        if self.updating.top_of(zero) < y + 1 {
            self.updating.top.insert(zero, y + 1);
        }
    }

    fn on_node_removed(&mut self, node: i64) {
        let zero = section_zero(node);
        let y = section_y(node);
        if self.updating.top_of(zero) != y + 1 {
            return;
        }
        let mut n = node;
        let mut yy = y;
        while !self.updating.layers.contains_key(&n) && yy >= self.updating.lowest {
            n = section_offset(n, 0, -1, 0);
            yy -= 1;
        }
        if self.updating.layers.contains_key(&n) {
            self.updating.top.insert(zero, yy + 1);
        } else {
            self.updating.top.remove(&zero);
        }
    }

    // ----- queued data, retain flags, light-enabled columns ------------------------------------------------------

    /// `queueSectionData(node, layer)`; `None` drops a still-queued layer.
    pub fn queue_section_data(&mut self, node: i64, layer: Option<DataLayer>) {
        match layer {
            Some(l) => {
                self.queued.insert(node, Queued::Data(Arc::new(l)));
                self.has_inconsistencies = true;
            }
            None => {
                self.queued.remove(&node);
            }
        }
    }

    /// `retainData(zeroNode, retain)`.
    pub fn retain_data(&mut self, zero: i64, retain: bool) {
        if retain {
            self.retain.insert(zero);
        } else {
            self.retain.remove(&zero);
        }
    }

    /// Storage-level `setLightEnabled(zeroNode, enable)` (no layer fill).
    pub fn set_light_enabled(&mut self, zero: i64, enable: bool) {
        if enable {
            self.columns_with_sources.insert(zero);
        } else {
            self.columns_with_sources.remove(&zero);
        }
    }

    /// Whether the column of `section_node` is light-enabled.
    #[inline]
    pub fn light_on_in_section(&self, section_node: i64) -> bool {
        self.columns_with_sources
            .contains(&section_zero(section_node))
    }

    /// Whether the column with this zero node is light-enabled.
    pub fn light_on_in_column(&self, zero: i64) -> bool {
        self.columns_with_sources.contains(&zero)
    }

    // ----- stored levels (engine side: updating map) -----------------------------------------------------------

    /// `storingLightForSection`: the updating map has a layer for the section.
    #[inline]
    pub fn storing_light_for_section(&self, section_node: i64) -> bool {
        self.updating.layers.contains_key(&section_node)
    }

    /// `getStoredLevel(blockNode)`; the section must store light (a missing layer reads 0).
    #[inline]
    pub fn stored_level(&self, block_node: i64) -> u8 {
        match self.updating.layers.get(&block_to_section(block_node)) {
            Some(l) => l.get(
                (block_x(block_node) & 15) as usize,
                (block_y(block_node) & 15) as usize,
                (block_z(block_node) & 15) as usize,
            ),
            None => 0,
        }
    }

    /// `getDataLayerToWrite(sectionNode)`: the writable layer (copy-on-write), `None` without a layer.
    pub fn layer_for_write(&mut self, section_node: i64) -> Option<&mut DataLayer> {
        let layers = &mut self.updating.layers;
        let entry = layers.get_mut(&section_node)?;
        if self.changed_sections.insert(section_node) {
            let copy = entry.copy();
            *entry = Arc::new(copy);
        }
        Some(Arc::make_mut(entry))
    }

    /// `setStoredLevel(blockNode, level)`.
    pub fn set_stored_level(&mut self, block_node: i64, level: u8) {
        let sec = block_to_section(block_node);
        let layer = self
            .layer_for_write(sec)
            .expect("set_stored_level on a section without a layer");
        layer.set(
            (block_x(block_node) & 15) as usize,
            (block_y(block_node) & 15) as usize,
            (block_z(block_node) & 15) as usize,
            level,
        );
        self.mark_affected_around(block_node);
    }

    fn mark_affected_around(&mut self, block_node: i64) {
        let (x, y, z) = (
            block_x(block_node),
            block_y(block_node),
            block_z(block_node),
        );
        let (x0, x1) = ((x - 1) >> 4, (x + 1) >> 4);
        let (y0, y1) = ((y - 1) >> 4, (y + 1) >> 4);
        let (z0, z1) = ((z - 1) >> 4, (z + 1) >> 4);
        if x0 == x1 && y0 == y1 && z0 == z1 {
            self.affected.insert(section_node(x0, y0, z0));
            return;
        }
        for sx in x0..=x1 {
            for sy in y0..=y1 {
                for sz in z0..=z1 {
                    self.affected.insert(section_node(sx, sy, sz));
                }
            }
        }
    }

    // ----- sky bookkeeping reads -----------------------------------------------------------------------------

    /// `getTopSectionY(zero)` (updating map).
    pub fn top_section_y(&self, zero: i64) -> i32 {
        self.updating.top_of(zero)
    }

    /// `getBottomSectionY()`: the lowest section ever stored.
    pub fn bottom_section_y(&self) -> i32 {
        self.updating.lowest
    }

    /// `hasLightDataAtOrBelow(sectionY)`.
    pub fn has_light_data_at_or_below(&self, section_y: i32) -> bool {
        section_y >= self.updating.lowest
    }

    /// `isAboveData(sectionNode)`.
    pub fn is_above_data(&self, section_node: i64) -> bool {
        let top = self.updating.top_of(section_zero(section_node));
        top == self.updating.lowest || section_y(section_node) >= top
    }

    // ----- end of run ------------------------------------------------------------------------------------------

    /// Whether the end-of-run pass has work (`hasInconsistencies`).
    pub const fn has_inconsistencies(&self) -> bool {
        self.has_inconsistencies
    }

    /// `markNewInconsistencies` (§7).
    pub fn mark_new_inconsistencies(&mut self) {
        if !self.has_inconsistencies {
            return;
        }
        self.has_inconsistencies = false;
        let mut removing: Vec<i64> = self.to_remove.iter().copied().collect();
        removing.sort_unstable();
        for &node in &removing {
            let queued = self.queued.remove(&node);
            let stored = self.updating.layers.remove(&node);
            if self.retain.contains(&section_zero(node)) {
                match queued {
                    Some(Queued::Data(l)) => {
                        self.queued.insert(node, Queued::Data(l));
                    }
                    Some(Queued::Adopted) | None => {
                        if let Some(s) = stored {
                            self.queued.insert(node, Queued::Data(s));
                        }
                    }
                }
            }
        }
        for &node in &removing {
            if self.is_sky() {
                self.on_node_removed(node);
            }
            self.changed_sections.insert(node);
        }
        self.to_remove.clear();
        let mut pending: Vec<i64> = self.queued.keys().copied().collect();
        pending.sort_unstable();
        for node in pending {
            if !self.updating.layers.contains_key(&node) {
                continue;
            }
            if let Some(Queued::Data(layer)) = self.queued.get(&node) {
                let same = self
                    .updating
                    .layers
                    .get(&node)
                    .is_some_and(|u| Arc::ptr_eq(u, layer));
                if !same {
                    let layer = layer.clone();
                    self.updating.layers.insert(node, layer);
                    self.changed_sections.insert(node);
                }
            }
            self.queued.remove(&node);
        }
    }

    /// `swapSectionMap` (§7): refreshes the visible snapshot and returns the sections to report, in order.
    pub fn swap_section_map(&mut self) -> Vec<i64> {
        if !self.changed_sections.is_empty() {
            self.visible = self.updating.clone();
            self.changed_sections.clear();
        }
        if self.affected.is_empty() {
            return Vec::new();
        }
        self.affected.take()
    }

    // ----- reads for game logic (visible map) ------------------------------------------------------------------

    /// The visible light value at a block node (§8).
    pub fn visible_light(&self, block_node: i64) -> u8 {
        let sec = block_to_section(block_node);
        let (lx, ly, lz) = (
            (block_x(block_node) & 15) as usize,
            (block_y(block_node) & 15) as usize,
            (block_z(block_node) & 15) as usize,
        );
        if !self.is_sky() {
            return self
                .visible
                .layers
                .get(&sec)
                .map_or(0, |l| l.get(lx, ly, lz));
        }
        let top = self.visible.top_of(section_zero(sec));
        let sy = section_y(sec);
        if top != self.visible.lowest && sy < top {
            if let Some(l) = self.visible.layers.get(&sec) {
                return l.get(lx, ly, lz);
            }
            let mut cur = sy;
            loop {
                cur += 1;
                if cur >= top {
                    return 15;
                }
                let n = section_node(section_x(sec), cur, section_z(sec));
                if let Some(l) = self.visible.layers.get(&n) {
                    return l.get(lx, 0, lz);
                }
            }
        }
        15
    }

    /// `getDataLayerData(sectionNode)`: the queued layer if any, else the visible layer.
    pub fn data_layer_data(&self, section_node: i64) -> Option<LayerRef> {
        match self.queued.get(&section_node) {
            Some(Queued::Data(l)) => Some(l.clone()),
            Some(Queued::Adopted) => self.updating.layers.get(&section_node).cloned(),
            None => self.visible.layers.get(&section_node).cloned(),
        }
    }

    /// `getDebugSectionType`: 2 = empty, 1 = light only, 0 = light and data.
    pub fn debug_section_type(&self, node: i64) -> u8 {
        match self.section_states.get(&node).copied().unwrap_or(0) {
            0 => 2,
            s if s & HAS_DATA == 0 => 1,
            _ => 0,
        }
    }

    /// The raw section state (neighbour count in bits 0..5, "has data" in bit 5).
    pub fn section_state(&self, node: i64) -> u8 {
        self.section_states.get(&node).copied().unwrap_or(0)
    }

    /// The layer in the updating map (engine view).
    pub fn updating_layer(&self, node: i64) -> Option<&LayerRef> {
        self.updating.layers.get(&node)
    }

    /// The layer in the visible map.
    pub fn visible_layer(&self, node: i64) -> Option<&LayerRef> {
        self.visible.layers.get(&node)
    }

    /// Whether data is queued (not yet adopted) for the section.
    pub fn has_queued_data(&self, node: i64) -> bool {
        matches!(self.queued.get(&node), Some(Queued::Data(_)))
    }

    /// The visible sky top-section value of a column (`topSections`), and the visible lowest section.
    pub fn visible_top_section(&self, zero: i64) -> (i32, i32) {
        (self.visible.top_of(zero), self.visible.lowest)
    }
}
