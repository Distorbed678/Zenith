//! The block-light and sky-light propagation engines (`light-engine.md`).
//!
//! One struct serves both engines (`LightLayer::Block` / `LightLayer::Sky`): the queues, the check set and the
//! run loop are shared; the per-node checks, the increase / decrease steps and the seeding differ.

use std::sync::Arc;

use zenith_blocks::Direction;

use crate::chunk::LightChunkSource;
use crate::facts::LightFacts;
use crate::fastutil::LongOpenHashSet;
use crate::layer::DataLayer;
use crate::pos::{
    block_node, block_offset, block_to_section, block_x, block_y, block_z, chunk_zero,
    section_node, section_offset, section_y,
};
use crate::queue::{
    self, ADD_SKY_SOURCE_ENTRY, INCREASE_FROM_EMISSION, PULL_LIGHT_IN_ENTRY, Queues,
    REMOVE_SKY_SOURCE_ENTRY, REMOVE_TOP_SKY_SOURCE_ENTRY, TraceEntry, has_direction,
};
use crate::sky_sources::SkyLightSources;
use crate::storage::{LayerRef, LayerStorage, LightLayer};

/// The vertical extent of the level the engine serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelHeight {
    /// Lowest block Y (`-64` in the Overworld).
    pub min_y: i32,
    /// Height in blocks (`384`).
    pub height: i32,
}

impl LevelHeight {
    /// `minSectionY - 1`: the lowest light section.
    pub const fn min_light_section(&self) -> i32 {
        (self.min_y >> 4) - 1
    }
    /// `sectionsCount + 2`.
    pub const fn light_section_count(&self) -> i32 {
        (self.height >> 4) + 2
    }
    /// Exclusive upper light-section bound.
    pub const fn max_light_section(&self) -> i32 {
        self.min_light_section() + self.light_section_count()
    }
    /// Lowest world section Y.
    pub const fn min_section(&self) -> i32 {
        self.min_y >> 4
    }
    /// Number of world sections.
    pub const fn section_count(&self) -> i32 {
        self.height >> 4
    }
}

/// One propagation engine with its storage.
#[derive(Debug, Clone)]
pub struct LightEngine {
    layer: LightLayer,
    storage: LayerStorage,
    facts: Arc<LightFacts>,
    level: LevelHeight,
    check_set: LongOpenHashSet,
    queues: Queues,
    notifications: Vec<i64>,
}

impl LightEngine {
    /// A fresh engine of kind `layer`.
    pub fn new(layer: LightLayer, facts: Arc<LightFacts>, level: LevelHeight) -> Self {
        Self {
            layer,
            storage: LayerStorage::new(layer),
            facts,
            level,
            check_set: LongOpenHashSet::for_checks(),
            queues: Queues::default(),
            notifications: Vec::new(),
        }
    }

    /// Which engine this is.
    pub const fn layer(&self) -> LightLayer {
        self.layer
    }

    /// The layer storage.
    pub const fn storage(&self) -> &LayerStorage {
        &self.storage
    }

    /// The layer storage, mutably (section status, queued data, retain flags).
    pub fn storage_mut(&mut self) -> &mut LayerStorage {
        &mut self.storage
    }

    /// The level extent.
    pub const fn level(&self) -> LevelHeight {
        self.level
    }

    /// Starts recording every enqueued queue entry (for queue-order parity traces).
    pub fn start_trace(&mut self) {
        self.queues.start_trace();
    }

    /// Returns and clears the recorded enqueues.
    pub fn take_trace(&mut self) -> Vec<TraceEntry> {
        self.queues.take_trace()
    }

    const fn is_sky(&self) -> bool {
        matches!(self.layer, LightLayer::Sky)
    }

    /// `checkBlock(pos)`: queue the block for re-examination.
    pub fn check_block(&mut self, block_node: i64) {
        self.check_set.insert(block_node);
    }

    /// `hasLightWork()`.
    pub fn has_light_work(&self) -> bool {
        self.storage.has_inconsistencies() || !self.check_set.is_empty() || self.queues.is_pending()
    }

    /// Takes the sections reported at the ends of the runs since the last call (`onLightUpdate` order).
    pub fn drain_light_updates(&mut self) -> Vec<i64> {
        std::mem::take(&mut self.notifications)
    }

    // ----- chunk / state access --------------------------------------------------------------------------------

    fn get_state(&self, chunks: &dyn LightChunkSource, node: i64) -> u32 {
        let (x, y, z) = (block_x(node), block_y(node), block_z(node));
        match chunks.chunk_for_lighting(x >> 4, z >> 4) {
            None => self.facts.bedrock_state(),
            Some(c) => {
                if y < c.min_y() || y >= c.min_y() + c.height() {
                    self.facts.air_state()
                } else {
                    c.block_state((x & 15) as usize, y, (z & 15) as usize)
                }
            }
        }
    }

    fn get_emission(&self, node: i64, state: u32) -> u8 {
        let e = self.facts.emission(state);
        if e > 0 && self.storage.light_on_in_section(block_to_section(node)) {
            e
        } else {
            0
        }
    }

    // ----- the run ---------------------------------------------------------------------------------------------

    /// `runLightUpdates` (rule 2): returns the number of dequeued entries.
    pub fn run_light_updates(&mut self, chunks: &dyn LightChunkSource) -> usize {
        let nodes = self.check_set.to_vec();
        for node in nodes {
            if self.is_sky() {
                self.sky_check_node(chunks, node);
            } else {
                self.block_check_node(chunks, node);
            }
        }
        self.check_set.clear();
        self.check_set.trim(512);

        let mut count = 0usize;
        while let Some((node, data)) = self.queues.pop_decrease() {
            if self.is_sky() {
                self.sky_propagate_decrease(chunks, node, data);
            } else {
                self.block_propagate_decrease(chunks, node, data);
            }
            count += 1;
        }
        while let Some((node, data)) = self.queues.pop_increase() {
            if !self
                .storage
                .storing_light_for_section(block_to_section(node))
            {
                // cannot happen with consistent calls (the game would fail here); never write outside a layer
                count += 1;
                continue;
            }
            let mut from_level = self.storage.stored_level(node);
            let target = queue::level(data);
            if data & INCREASE_FROM_EMISSION != 0 && from_level < target {
                self.storage.set_stored_level(node, target);
                from_level = target;
            }
            if from_level == target {
                self.propagate_increase(chunks, node, data, from_level);
            }
            count += 1;
        }
        self.storage.mark_new_inconsistencies();
        let reported = self.storage.swap_section_map();
        self.notifications.extend(reported);
        count
    }

    // ----- shared increase step (rule 3 + sky hook) -----------------------------------------------------------

    fn propagate_increase(
        &mut self,
        chunks: &dyn LightChunkSource,
        from: i64,
        data: u16,
        from_level: u8,
    ) {
        let sky = self.is_sky();
        let empty_below = if sky {
            self.count_empty_sections_below_if_at_border(from)
        } else {
            0
        };
        let from_empty = data & queue::FROM_EMPTY_SHAPE != 0;
        let mut a_state: Option<u32> = None;
        for d in Direction::ALL {
            if !has_direction(data, d) {
                continue;
            }
            let to = block_offset(from, d);
            if !self.storage.storing_light_for_section(block_to_section(to)) {
                continue;
            }
            let to_level = i32::from(self.storage.stored_level(to));
            let a = i32::from(from_level);
            if a - 1 <= to_level {
                continue;
            }
            let b_state = self.get_state(chunks, to);
            let new_to = a - i32::from(self.facts.opacity(b_state));
            if new_to <= to_level {
                continue;
            }
            let a_state_v = match a_state {
                Some(s) => s,
                None => {
                    let s = if from_empty {
                        self.facts.air_state()
                    } else {
                        self.get_state(chunks, from)
                    };
                    a_state = Some(s);
                    s
                }
            };
            if self.facts.shape_occludes(a_state_v, b_state, d) {
                continue;
            }
            let new_to = new_to as u8;
            self.storage.set_stored_level(to, new_to);
            if new_to > 1 {
                self.queues.enqueue_increase(
                    to,
                    queue::increase_skip_one_direction(
                        new_to,
                        self.facts.empty_shape(b_state),
                        d.opposite(),
                    ),
                );
            }
            if sky {
                self.propagate_from_empty_sections(to, d, new_to, true, empty_below);
            }
        }
    }

    // ----- block engine ----------------------------------------------------------------------------------------

    fn block_check_node(&mut self, chunks: &dyn LightChunkSource, node: i64) {
        if !self
            .storage
            .storing_light_for_section(block_to_section(node))
        {
            return;
        }
        let state = self.get_state(chunks, node);
        let emission = self.get_emission(node, state);
        let old = self.storage.stored_level(node);
        if emission < old {
            self.storage.set_stored_level(node, 0);
            self.queues
                .enqueue_decrease(node, queue::decrease_all_directions(old));
        } else {
            self.queues.enqueue_decrease(node, PULL_LIGHT_IN_ENTRY);
        }
        if emission > 0 {
            self.queues.enqueue_increase(
                node,
                queue::increase_light_from_emission(emission, self.facts.empty_shape(state)),
            );
        }
    }

    fn block_propagate_decrease(&mut self, chunks: &dyn LightChunkSource, from: i64, data: u16) {
        let old = i32::from(queue::level(data));
        for d in Direction::ALL {
            if !has_direction(data, d) {
                continue;
            }
            let to = block_offset(from, d);
            if !self.storage.storing_light_for_section(block_to_section(to)) {
                continue;
            }
            let to_level = self.storage.stored_level(to);
            if to_level == 0 {
                continue;
            }
            if i32::from(to_level) < old {
                let to_state = self.get_state(chunks, to);
                let to_emission = self.get_emission(to, to_state);
                self.storage.set_stored_level(to, 0);
                if to_emission < to_level {
                    self.queues.enqueue_decrease(
                        to,
                        queue::decrease_skip_one_direction(to_level, d.opposite()),
                    );
                }
                if to_emission > 0 {
                    self.queues.enqueue_increase(
                        to,
                        queue::increase_light_from_emission(
                            to_emission,
                            self.facts.empty_shape(to_state),
                        ),
                    );
                }
            } else {
                self.queues.enqueue_increase(
                    to,
                    queue::increase_only_one_direction(to_level, false, d.opposite()),
                );
            }
        }
    }

    /// Block `propagateLightSources(chunkPos)` (rule 9).
    fn block_propagate_light_sources(&mut self, chunks: &dyn LightChunkSource, cx: i32, cz: i32) {
        self.storage.set_light_enabled(chunk_zero(cx, cz), true);
        let Some(chunk) = chunks.chunk_for_lighting(cx, cz) else {
            return;
        };
        let sections = (chunk.height() / 16) as usize;
        for index in 0..sections {
            if chunk.section_only_air(index, &self.facts) {
                continue;
            }
            let base = chunk.min_y() + index as i32 * 16;
            for y in 0..16 {
                for z in 0..16 {
                    for x in 0..16 {
                        let state = chunk.block_state(x, base + y, z);
                        let emission = self.facts.emission(state);
                        if emission != 0 {
                            let node = block_node(cx * 16 + x as i32, base + y, cz * 16 + z as i32);
                            self.queues.enqueue_increase(
                                node,
                                queue::increase_light_from_emission(
                                    emission,
                                    self.facts.empty_shape(state),
                                ),
                            );
                        }
                    }
                }
            }
        }
    }

    // ----- sky engine ------------------------------------------------------------------------------------------

    /// `lowestSourceY(x, z, default)`.
    fn lowest_source_y(chunks: &dyn LightChunkSource, x: i32, z: i32, default: i32) -> i32 {
        match chunks.chunk_for_lighting(x >> 4, z >> 4) {
            Some(c) => c
                .sky_sources()
                .lowest_source_y((x & 15) as usize, (z & 15) as usize),
            None => default,
        }
    }

    fn sky_check_node(&mut self, chunks: &dyn LightChunkSource, node: i64) {
        let (x, y, z) = (block_x(node), block_y(node), block_z(node));
        let sec = block_to_section(node);
        let lowest = if self.storage.light_on_in_section(sec) {
            Self::lowest_source_y(chunks, x, z, i32::MAX)
        } else {
            i32::MAX
        };
        if lowest != i32::MAX {
            self.update_sources_in_column(chunks, x, z, lowest);
        }
        if self.storage.storing_light_for_section(sec) {
            if y >= lowest {
                self.queues.enqueue_decrease(node, REMOVE_SKY_SOURCE_ENTRY);
                self.queues.enqueue_increase(node, ADD_SKY_SOURCE_ENTRY);
            } else {
                let old = self.storage.stored_level(node);
                if old > 0 {
                    self.storage.set_stored_level(node, 0);
                    self.queues
                        .enqueue_decrease(node, queue::decrease_all_directions(old));
                } else {
                    self.queues.enqueue_decrease(node, PULL_LIGHT_IN_ENTRY);
                }
            }
        }
    }

    fn update_sources_in_column(
        &mut self,
        chunks: &dyn LightChunkSource,
        x: i32,
        z: i32,
        lowest: i32,
    ) {
        let world_bottom_y = self.storage.bottom_section_y() << 4;
        self.remove_sources_below(x, z, lowest, world_bottom_y);
        self.add_sources_above(chunks, x, z, lowest, world_bottom_y);
    }

    fn remove_sources_below(&mut self, x: i32, z: i32, lowest: i32, world_bottom_y: i32) {
        if lowest <= world_bottom_y {
            return;
        }
        let start_y = lowest - 1;
        let current_lowest = self.storage.bottom_section_y();
        let mut sy = start_y >> 4;
        while sy >= current_lowest {
            let sec = section_node(x >> 4, sy, z >> 4);
            if self.storage.storing_light_for_section(sec) {
                let bottom = sy << 4;
                let top = bottom + 15;
                let mut y = top.min(start_y);
                while y >= bottom {
                    let node = block_node(x, y, z);
                    if self.storage.stored_level(node) != 15 {
                        return;
                    }
                    self.storage.set_stored_level(node, 0);
                    self.queues.enqueue_decrease(
                        node,
                        if y == lowest - 1 {
                            REMOVE_TOP_SKY_SOURCE_ENTRY
                        } else {
                            REMOVE_SKY_SOURCE_ENTRY
                        },
                    );
                    y -= 1;
                }
            }
            sy -= 1;
        }
    }

    fn add_sources_above(
        &mut self,
        chunks: &dyn LightChunkSource,
        x: i32,
        z: i32,
        lowest: i32,
        world_bottom_y: i32,
    ) {
        let neighbor_lowest = Self::lowest_source_y(chunks, x - 1, z, i32::MIN)
            .max(Self::lowest_source_y(chunks, x + 1, z, i32::MIN))
            .max(Self::lowest_source_y(chunks, x, z - 1, i32::MIN))
            .max(Self::lowest_source_y(chunks, x, z + 1, i32::MIN));
        let start_y = lowest.max(world_bottom_y);
        let mut sec = block_to_section(block_node(x, start_y, z));
        while !self.storage.is_above_data(sec) {
            if self.storage.storing_light_for_section(sec) {
                let bottom = section_y(sec) << 4;
                let top = bottom + 15;
                for y in bottom.max(start_y)..=top {
                    let node = block_node(x, y, z);
                    if self.storage.stored_level(node) == 15 {
                        return;
                    }
                    self.storage.set_stored_level(node, 15);
                    if y < neighbor_lowest || y == lowest {
                        self.queues.enqueue_increase(node, ADD_SKY_SOURCE_ENTRY);
                    }
                }
            }
            sec = section_offset(sec, 0, 1, 0);
        }
    }

    fn count_empty_sections_below_if_at_border(&self, node: i64) -> i32 {
        let (lx, ly, lz) = (block_x(node) & 15, block_y(node) & 15, block_z(node) & 15);
        if ly != 0 || !(lx == 0 || lx == 15 || lz == 0 || lz == 15) {
            return 0;
        }
        let sec = block_to_section(node);
        let sy = section_y(sec);
        let (sx, sz) = (crate::pos::section_x(sec), crate::pos::section_z(sec));
        let mut k = 0;
        while !self
            .storage
            .storing_light_for_section(section_node(sx, sy - k - 1, sz))
            && self.storage.has_light_data_at_or_below(sy - k - 1)
        {
            k += 1;
        }
        k
    }

    fn propagate_from_empty_sections(
        &mut self,
        to: i64,
        d: Direction,
        level: u8,
        increase: bool,
        empty_below: i32,
    ) {
        if empty_below == 0 {
            return;
        }
        let (lx, lz) = (block_x(to) & 15, block_z(to) & 15);
        let crossed = match d {
            Direction::North => lz == 15,
            Direction::South => lz == 0,
            Direction::West => lx == 15,
            Direction::East => lx == 0,
            Direction::Down | Direction::Up => false,
        };
        if !crossed {
            return;
        }
        let to_sec_y = block_y(to) >> 4;
        let (sx, sz) = (block_x(to) >> 4, block_z(to) >> 4);
        let mut sy = to_sec_y - 1;
        while sy >= to_sec_y - empty_below {
            if self
                .storage
                .storing_light_for_section(section_node(sx, sy, sz))
            {
                for ly in (0..16).rev() {
                    let node = block_node(block_x(to), (sy << 4) + ly, block_z(to));
                    if increase {
                        self.storage.set_stored_level(node, level);
                        if level > 1 {
                            self.queues.enqueue_increase(
                                node,
                                queue::increase_skip_one_direction(level, true, d.opposite()),
                            );
                        }
                    } else {
                        self.storage.set_stored_level(node, 0);
                        self.queues.enqueue_decrease(
                            node,
                            queue::decrease_skip_one_direction(level, d.opposite()),
                        );
                    }
                }
            }
            sy -= 1;
        }
    }

    fn sky_propagate_decrease(&mut self, _chunks: &dyn LightChunkSource, from: i64, data: u16) {
        let empty_below = self.count_empty_sections_below_if_at_border(from);
        let old = i32::from(queue::level(data));
        for d in Direction::ALL {
            if !has_direction(data, d) {
                continue;
            }
            let to = block_offset(from, d);
            if !self.storage.storing_light_for_section(block_to_section(to)) {
                continue;
            }
            let to_level = self.storage.stored_level(to);
            if to_level == 0 {
                continue;
            }
            if i32::from(to_level) < old {
                self.storage.set_stored_level(to, 0);
                self.queues.enqueue_decrease(
                    to,
                    queue::decrease_skip_one_direction(to_level, d.opposite()),
                );
                self.propagate_from_empty_sections(to, d, to_level, false, empty_below);
            } else {
                self.queues.enqueue_increase(
                    to,
                    queue::increase_only_one_direction(to_level, false, d.opposite()),
                );
            }
        }
    }

    /// Sky `setLightEnabled(chunkPos, enable)` (rule 20); the block engine only toggles the storage flag.
    pub fn set_light_enabled(
        &mut self,
        chunks: &dyn LightChunkSource,
        cx: i32,
        cz: i32,
        enable: bool,
    ) {
        let zero = chunk_zero(cx, cz);
        self.storage.set_light_enabled(zero, enable);
        if !(enable && self.is_sky()) {
            return;
        }
        let highest = chunks
            .chunk_for_lighting(cx, cz)
            .map_or(i32::MIN, |c| c.sky_sources().highest_lowest_source_y());
        let highest_non_source = highest.wrapping_sub(1);
        let lowest_fully_source_section = (highest_non_source >> 4).wrapping_add(1);
        let top = self.storage.top_section_y(zero);
        let bottom = self
            .storage
            .bottom_section_y()
            .max(lowest_fully_source_section);
        for sy in (bottom..top).rev() {
            if let Some(layer) = self.storage.layer_for_write(section_node(cx, sy, cz))
                && layer.is_empty()
            {
                layer.fill(15);
            }
        }
    }

    /// `propagateLightSources(chunkPos)`: seeds a freshly generated chunk (rules 9 and 21).
    pub fn propagate_light_sources(&mut self, chunks: &dyn LightChunkSource, cx: i32, cz: i32) {
        if self.is_sky() {
            self.sky_propagate_light_sources(chunks, cx, cz);
        } else {
            self.block_propagate_light_sources(chunks, cx, cz);
        }
    }

    fn sky_propagate_light_sources(&mut self, chunks: &dyn LightChunkSource, cx: i32, cz: i32) {
        let zero = chunk_zero(cx, cz);
        self.storage.set_light_enabled(zero, true);
        let src = |dx: i32, dz: i32| -> Option<&SkyLightSources> {
            chunks
                .chunk_for_lighting(cx + dx, cz + dz)
                .map(|c| c.sky_sources())
        };
        let lowest = |s: Option<&SkyLightSources>, x: usize, z: usize| -> i32 {
            s.map_or(i32::MIN, |s| s.lowest_source_y(x, z))
        };
        let (s, n, so, w, e) = (src(0, 0), src(0, -1), src(0, 1), src(-1, 0), src(1, 0));
        let top = self.storage.top_section_y(zero);
        let bottom = self.storage.bottom_section_y();
        for sy in (bottom..top).rev() {
            let node = section_node(cx, sy, cz);
            let Some(layer) = self.storage.layer_for_write(node) else {
                continue;
            };
            let min_y = sy * 16;
            let max_y = min_y + 15;
            let mut sources_below = false;
            for z in 0..16usize {
                for x in 0..16usize {
                    let low = lowest(s, x, z);
                    if low > max_y {
                        continue;
                    }
                    let north = if z == 0 {
                        lowest(n, x, 15)
                    } else {
                        lowest(s, x, z - 1)
                    };
                    let south = if z == 15 {
                        lowest(so, x, 0)
                    } else {
                        lowest(s, x, z + 1)
                    };
                    let west = if x == 0 {
                        lowest(w, 15, z)
                    } else {
                        lowest(s, x - 1, z)
                    };
                    let east = if x == 15 {
                        lowest(e, 0, z)
                    } else {
                        lowest(s, x + 1, z)
                    };
                    let nb = north.max(south).max(west).max(east);
                    let mut y = max_y;
                    let floor = min_y.max(low);
                    while y >= floor {
                        layer.set(x, (y & 15) as usize, z, 15);
                        if y == low || y < nb {
                            self.queues.enqueue_increase(
                                block_node(cx * 16 + x as i32, y, cz * 16 + z as i32),
                                queue::increase_sky_source_in_directions(
                                    y == low,
                                    y < north,
                                    y < south,
                                    y < west,
                                    y < east,
                                ),
                            );
                        }
                        y -= 1;
                    }
                    if low < min_y {
                        sources_below = true;
                    }
                }
            }
            if !sources_below {
                break;
            }
        }
    }

    // ----- visible reads ---------------------------------------------------------------------------------------

    /// The visible light value at a block node (what game logic sees).
    pub fn light_value(&self, block_node: i64) -> u8 {
        self.storage.visible_light(block_node)
    }

    /// `getDataLayerData`: the queued layer, else the visible one.
    pub fn data_layer_data(&self, section_node: i64) -> Option<LayerRef> {
        self.storage.data_layer_data(section_node)
    }

    /// Queues external layer data for a section (chunk load, client packet); `None` drops queued data.
    pub fn queue_section_data(&mut self, section_node: i64, layer: Option<DataLayer>) {
        self.storage.queue_section_data(section_node, layer);
    }

    /// `updateSectionStatus`.
    pub fn update_section_status(&mut self, section_node: i64, section_empty: bool) {
        self.storage
            .update_section_status(section_node, section_empty);
    }

    /// `retainData`.
    pub fn retain_data(&mut self, zero_node: i64, retain: bool) {
        self.storage.retain_data(zero_node, retain);
    }
}
