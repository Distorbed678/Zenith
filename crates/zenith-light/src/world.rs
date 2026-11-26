//! `LightWorld`: a server-side level's lighting driven the deterministic way (`chunk-steps.md` §2–§8, §11).
//!
//! Owns the chunks visible to lighting, the level light engine and the task list of the threaded wrapper. Tasks
//! are served in global submission order and batched exactly like the vanilla light thread: a batch runs the PRE
//! tasks among the first `min(size, 1000)`, then one engine run (block, then sky), then the POST tasks. `flush`
//! repeats batches until no task and no engine work remain.

use std::collections::HashMap;
use std::sync::Arc;

use crate::change::{ChangeMode, apply_block_change};
use crate::chunk::{ChunkBlocks, LightChunk, LightChunkSource, LightStatus, LitChunk};
use crate::engine::LevelHeight;
use crate::facts::LightFacts;
use crate::layer::{BadLayerSize, DataLayer};
use crate::level::LevelLightEngine;
use crate::packet::{BitSet, LightUpdatePacketData};
use crate::pos::{block_node, chunk_zero, section_node};
use crate::storage::{LayerRef, LightLayer, NodeMap};

/// Tasks per batch of the light thread.
pub const BATCH_SIZE: usize = 1000;

/// The chunks visible to lighting (the chunk-for-lighting accessor).
#[derive(Debug, Clone)]
pub struct ChunkMap<B>(NodeMap<LitChunk<B>>);

impl<B> Default for ChunkMap<B> {
    fn default() -> Self {
        Self(NodeMap::default())
    }
}

impl<B: ChunkBlocks> LightChunkSource for ChunkMap<B> {
    fn chunk_for_lighting(&self, cx: i32, cz: i32) -> Option<&dyn LightChunk> {
        self.0
            .get(&chunk_zero(cx, cz))
            .map(|c| c as &dyn LightChunk)
    }
}

impl<B> ChunkMap<B> {
    /// The chunk at `(cx, cz)`.
    pub fn get(&self, cx: i32, cz: i32) -> Option<&LitChunk<B>> {
        self.0.get(&chunk_zero(cx, cz))
    }

    /// The chunk at `(cx, cz)`, mutably.
    pub fn get_mut(&mut self, cx: i32, cz: i32) -> Option<&mut LitChunk<B>> {
        self.0.get_mut(&chunk_zero(cx, cz))
    }

    /// Inserts a chunk.
    pub fn insert(&mut self, chunk: LitChunk<B>) {
        self.0.insert(chunk_zero(chunk.cx, chunk.cz), chunk);
    }

    /// Removes a chunk.
    pub fn remove(&mut self, cx: i32, cz: i32) -> Option<LitChunk<B>> {
        self.0.remove(&chunk_zero(cx, cz))
    }

    /// Chunk positions, unordered.
    pub fn positions(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        self.0.values().map(|c| (c.cx, c.cz))
    }
}

#[derive(Debug)]
enum Action {
    CheckBlock(i64),
    UpdateSectionStatus(i64, bool),
    QueueSectionData(LightLayer, i64, Option<DataLayer>),
    RetainData(i32, i32, bool),
    SetLightEnabled(i32, i32, bool),
    PropagateLightSources(i32, i32),
    InitializeLightPre(i32, i32),
    InitializeLightPost(i32, i32, bool),
    LightChunkPre(i32, i32, bool),
    LightChunkPost(i32, i32),
    Unload(i32, i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskKind {
    Pre,
    Post,
}

#[derive(Debug)]
struct Task {
    kind: TaskKind,
    action: Option<Action>,
}

/// Per-holder change filters (which light sections changed since the last broadcast).
#[derive(Debug, Default)]
struct ChangeTracker {
    order: Vec<(i32, i32)>,
    filters: HashMap<(i32, i32), (BitSet, BitSet)>,
}

/// Saved light of one section of a chunk (`BlockLight` / `SkyLight`, each exactly 2048 bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionLight {
    /// Section Y (the NBT `Y` byte).
    pub y: i32,
    /// `BlockLight`.
    pub block_light: Option<Vec<u8>>,
    /// `SkyLight`.
    pub sky_light: Option<Vec<u8>>,
}

/// The light part of a saved chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkLightSave {
    /// NBT `isLightOn` (written only when true).
    pub light_correct: bool,
    /// Sections with at least one kept layer, ascending `Y`.
    pub sections: Vec<SectionLight>,
}

/// A level's lighting: chunks, engines and the batched task list.
#[derive(Debug)]
pub struct LightWorld<B: ChunkBlocks> {
    facts: Arc<LightFacts>,
    engine: LevelLightEngine,
    chunks: ChunkMap<B>,
    tasks: Vec<Task>,
    tracker: ChangeTracker,
    runs: u64,
}

impl<B: ChunkBlocks> LightWorld<B> {
    /// A world without chunks.
    pub fn new(facts: Arc<LightFacts>, level: LevelHeight, has_skylight: bool) -> Self {
        Self {
            engine: LevelLightEngine::new(facts.clone(), level, has_skylight),
            facts,
            chunks: ChunkMap::default(),
            tasks: Vec::new(),
            tracker: ChangeTracker::default(),
            runs: 0,
        }
    }

    /// The per-state tables.
    pub fn facts(&self) -> &Arc<LightFacts> {
        &self.facts
    }

    /// The level engine (read access: values, layers, section states).
    pub fn engine(&self) -> &LevelLightEngine {
        &self.engine
    }

    /// The level engine, mutably (tracing and direct drives; prefer the world methods).
    pub fn engine_mut(&mut self) -> &mut LevelLightEngine {
        &mut self.engine
    }

    /// The chunks.
    pub fn chunks(&self) -> &ChunkMap<B> {
        &self.chunks
    }

    /// The chunk at `(cx, cz)`, mutably. Edit blocks through [`LightWorld::set_block`] instead of writing to
    /// `blocks` directly (or call [`LitChunk::invalidate_counts`]).
    pub fn chunk_mut(&mut self, cx: i32, cz: i32) -> Option<&mut LitChunk<B>> {
        self.chunks.get_mut(cx, cz)
    }

    /// Number of engine batches run so far.
    pub const fn batches_run(&self) -> u64 {
        self.runs
    }

    /// Makes a chunk visible to lighting (its `features` step is complete).
    pub fn add_chunk(&mut self, chunk: LitChunk<B>) {
        self.chunks.insert(chunk);
    }

    // ----- tasks ---------------------------------------------------------------------------------------------

    fn submit(&mut self, kind: TaskKind, action: Action) {
        self.tasks.push(Task {
            kind,
            action: Some(action),
        });
        if self.tasks.len() >= BATCH_SIZE {
            self.run_update();
        }
    }

    /// `runUpdate`: one batch. Returns the engine's processed-entry count.
    pub fn run_update(&mut self) -> usize {
        let n = self.tasks.len().min(BATCH_SIZE);
        for i in 0..n {
            if self.tasks[i].kind == TaskKind::Pre
                && let Some(a) = self.tasks[i].action.take()
            {
                self.run_action(a);
            }
        }
        let processed = self.engine.run_light_updates(&self.chunks);
        self.runs += 1;
        for i in 0..n {
            if self.tasks[i].kind == TaskKind::Post
                && let Some(a) = self.tasks[i].action.take()
            {
                self.run_action(a);
            }
        }
        self.tasks.drain(..n);
        for (layer, node) in self.engine.drain_light_updates() {
            self.section_light_changed(layer, node);
        }
        processed
    }

    /// Runs batches until no task and no engine work remain (the deterministic quiescence point).
    pub fn flush(&mut self) {
        while !self.tasks.is_empty() || self.engine.has_light_work() {
            self.run_update();
        }
    }

    /// Number of tasks waiting for the next batch.
    pub fn pending_tasks(&self) -> usize {
        self.tasks.len()
    }

    fn run_action(&mut self, action: Action) {
        match action {
            Action::CheckBlock(node) => self.engine.check_block(node),
            Action::UpdateSectionStatus(node, empty) => {
                self.engine.update_section_status(node, empty)
            }
            Action::QueueSectionData(layer, node, data) => {
                self.engine.queue_section_data(layer, node, data);
            }
            Action::RetainData(cx, cz, retain) => self.engine.retain_data(cx, cz, retain),
            Action::SetLightEnabled(cx, cz, enable) => {
                self.engine.set_light_enabled(&self.chunks, cx, cz, enable);
            }
            Action::PropagateLightSources(cx, cz) => {
                self.engine.propagate_light_sources(&self.chunks, cx, cz);
            }
            Action::InitializeLightPre(cx, cz) => {
                let min_section = self.engine.level().min_section();
                let Some(chunk) = self.chunks.get(cx, cz) else {
                    return;
                };
                let nodes: Vec<i64> = (0..chunk.section_count())
                    .filter(|&i| !chunk.section_only_air(i, &self.facts))
                    .map(|i| section_node(cx, min_section + i as i32, cz))
                    .collect();
                for node in nodes {
                    self.engine.update_section_status(node, false);
                }
            }
            Action::InitializeLightPost(cx, cz, lighted) => {
                self.engine.set_light_enabled(&self.chunks, cx, cz, lighted);
                self.engine.retain_data(cx, cz, false);
            }
            Action::LightChunkPre(cx, cz, lighted) => {
                if !lighted {
                    self.engine.propagate_light_sources(&self.chunks, cx, cz);
                }
            }
            Action::LightChunkPost(cx, cz) => {
                if let Some(c) = self.chunks.get_mut(cx, cz) {
                    c.light_correct = true;
                }
            }
            Action::Unload(cx, cz) => {
                self.engine.retain_data(cx, cz, false);
                self.engine.set_light_enabled(&self.chunks, cx, cz, false);
                let level = self.engine.level();
                for sy in level.min_light_section()..level.max_light_section() {
                    let node = section_node(cx, sy, cz);
                    self.engine
                        .queue_section_data(LightLayer::Block, node, None);
                    self.engine.queue_section_data(LightLayer::Sky, node, None);
                }
                for sy in level.min_section()..level.min_section() + level.section_count() {
                    self.engine
                        .update_section_status(section_node(cx, sy, cz), true);
                }
            }
        }
    }

    fn section_light_changed(&mut self, layer: LightLayer, node: i64) {
        let (cx, cz, sy) = (
            crate::pos::section_x(node),
            crate::pos::section_z(node),
            crate::pos::section_y(node),
        );
        let level = self.engine.level();
        let Some(chunk) = self.chunks.get_mut(cx, cz) else {
            return;
        };
        if chunk.status < LightStatus::InitializeLight {
            return;
        }
        chunk.unsaved = true;
        if chunk.status < LightStatus::Full {
            return;
        }
        if sy < level.min_light_section() || sy > level.max_light_section() {
            return;
        }
        let bit = (sy - level.min_light_section()) as usize;
        let entry = self.tracker.filters.entry((cx, cz)).or_default();
        let newly = match layer {
            LightLayer::Sky => entry.0.set(bit),
            LightLayer::Block => entry.1.set(bit),
        };
        if newly && !self.tracker.order.contains(&(cx, cz)) {
            self.tracker.order.push((cx, cz));
        }
    }

    // ----- chunk steps -----------------------------------------------------------------------------------------

    /// The `initialize_light` step of chunk `(cx, cz)` (§2): rebuilds the sky-source table, registers the
    /// non-empty sections, enables light for lighted chunks. Completes after a flush.
    pub fn initialize_light(&mut self, cx: i32, cz: i32) {
        let facts = self.facts.clone();
        let Some(chunk) = self.chunks.get_mut(cx, cz) else {
            return;
        };
        chunk.ensure_counts(&facts);
        chunk.sky_sources.fill_from(&chunk.blocks, &facts);
        let lighted = chunk.is_lighted();
        self.submit(TaskKind::Pre, Action::InitializeLightPre(cx, cz));
        self.submit(TaskKind::Post, Action::InitializeLightPost(cx, cz, lighted));
        self.flush();
        if let Some(c) = self.chunks.get_mut(cx, cz)
            && c.status < LightStatus::InitializeLight
        {
            c.status = LightStatus::InitializeLight;
        }
    }

    /// The `light` step of chunk `(cx, cz)` (§4): seeds the chunk's light sources unless it was loaded lighted.
    pub fn light(&mut self, cx: i32, cz: i32) {
        let Some(chunk) = self.chunks.get_mut(cx, cz) else {
            return;
        };
        let lighted = chunk.is_lighted();
        chunk.light_correct = false;
        self.submit(TaskKind::Pre, Action::LightChunkPre(cx, cz, lighted));
        self.submit(TaskKind::Post, Action::LightChunkPost(cx, cz));
        self.flush();
        if let Some(c) = self.chunks.get_mut(cx, cz)
            && c.status < LightStatus::Light
        {
            c.status = LightStatus::Light;
        }
    }

    /// Lights a generated neighbourhood: every chunk becomes visible to lighting, then `initialize_light` runs on
    /// each in the given order, then `light` on each in the same order.
    pub fn light_chunks(&mut self, chunks: Vec<LitChunk<B>>) {
        let order: Vec<(i32, i32)> = chunks.iter().map(|c| (c.cx, c.cz)).collect();
        for c in chunks {
            self.add_chunk(c);
        }
        for &(cx, cz) in &order {
            self.initialize_light(cx, cz);
        }
        for &(cx, cz) in &order {
            self.light(cx, cz);
        }
    }

    /// Marks chunk `(cx, cz)` as having reached `spawn` then `full` (the level chunk takes over block changes).
    pub fn promote_to_full(&mut self, cx: i32, cz: i32) {
        if let Some(c) = self.chunks.get_mut(cx, cz) {
            c.status = LightStatus::Full;
        }
    }

    // ----- block changes ---------------------------------------------------------------------------------------

    /// Sets a block (world coordinates) and queues the light work it implies (§7). Returns the previous state, or
    /// `None` when nothing was written. Call [`LightWorld::flush`] to settle.
    pub fn set_block(&mut self, x: i32, y: i32, z: i32, state: u32) -> Option<u32> {
        let facts = self.facts.clone();
        let chunk = self.chunks.get_mut(x >> 4, z >> 4)?;
        let mode = if chunk.status >= LightStatus::Full {
            ChangeMode::Level
        } else {
            ChangeMode::Proto
        };
        let change = apply_block_change(chunk, &facts, mode, x, y, z, state);
        if let Some((node, empty)) = change.calls.section_status {
            self.submit(TaskKind::Pre, Action::UpdateSectionStatus(node, empty));
        }
        if let Some(node) = change.calls.check_block {
            self.submit(TaskKind::Pre, Action::CheckBlock(node));
        }
        change.old
    }

    // ----- save / load / unload --------------------------------------------------------------------------------

    /// The light layers of a chunk as the saver writes them (§6): every light section, kept only when the layer
    /// exists and is not empty. Flushes first.
    pub fn save_chunk_light(&mut self, cx: i32, cz: i32) -> Option<ChunkLightSave> {
        self.flush();
        let chunk = self.chunks.get(cx, cz)?;
        let level = self.engine.level();
        let mut sections = Vec::new();
        for sy in level.min_light_section()..level.max_light_section() {
            let node = section_node(cx, sy, cz);
            let keep = |l: LightLayer| -> Option<Vec<u8>> {
                self.engine
                    .data_layer_data(l, node)
                    .filter(|d| !d.is_empty())
                    .map(|d| d.to_bytes().to_vec())
            };
            let (block_light, sky_light) = (keep(LightLayer::Block), keep(LightLayer::Sky));
            if block_light.is_some() || sky_light.is_some() {
                sections.push(SectionLight {
                    y: sy,
                    block_light,
                    sky_light,
                });
            }
        }
        Some(ChunkLightSave {
            light_correct: chunk.light_correct,
            sections,
        })
    }

    /// Queues the saved layers of a freshly loaded chunk (§5). Arrays must be exactly 2048 bytes.
    pub fn load_chunk_light(
        &mut self,
        cx: i32,
        cz: i32,
        sections: &[SectionLight],
    ) -> Result<(), BadLayerSize> {
        let has_sky = self.engine.has_sky();
        let mut parsed = Vec::new();
        for s in sections {
            let block = s
                .block_light
                .as_deref()
                .map(DataLayer::from_bytes)
                .transpose()?;
            let sky = s
                .sky_light
                .as_deref()
                .map(DataLayer::from_bytes)
                .transpose()?;
            parsed.push((s.y, block, sky));
        }
        let mut retained = false;
        for (y, block, sky) in parsed {
            let sky = sky.filter(|_| has_sky);
            if block.is_none() && sky.is_none() {
                continue;
            }
            if !retained {
                retained = true;
                self.submit(TaskKind::Pre, Action::RetainData(cx, cz, true));
            }
            let node = section_node(cx, y, cz);
            if let Some(b) = block {
                self.submit(
                    TaskKind::Pre,
                    Action::QueueSectionData(LightLayer::Block, node, Some(b)),
                );
            }
            if let Some(s) = sky {
                self.submit(
                    TaskKind::Pre,
                    Action::QueueSectionData(LightLayer::Sky, node, Some(s)),
                );
            }
        }
        Ok(())
    }

    /// Unloads a chunk (§6): runs the unload task, settles, and returns the chunk.
    pub fn unload_chunk(&mut self, cx: i32, cz: i32) -> Option<LitChunk<B>> {
        self.chunks.get(cx, cz)?;
        self.submit(TaskKind::Pre, Action::Unload(cx, cz));
        self.flush();
        self.tracker.filters.remove(&(cx, cz));
        self.tracker.order.retain(|&p| p != (cx, cz));
        self.chunks.remove(cx, cz)
    }

    /// Submits a `setLightEnabled` task (the threaded wrapper's entry).
    pub fn set_light_enabled(&mut self, cx: i32, cz: i32, enable: bool) {
        self.submit(TaskKind::Pre, Action::SetLightEnabled(cx, cz, enable));
    }

    /// Submits a `propagateLightSources` task.
    pub fn propagate_light_sources(&mut self, cx: i32, cz: i32) {
        self.submit(TaskKind::Pre, Action::PropagateLightSources(cx, cz));
    }

    // ----- reads -----------------------------------------------------------------------------------------------

    /// The queued-or-visible layer of a section (`getDataLayerData`).
    pub fn layer(
        &self,
        layer: LightLayer,
        section_x: i32,
        section_y: i32,
        section_z: i32,
    ) -> Option<LayerRef> {
        self.engine
            .data_layer_data(layer, section_node(section_x, section_y, section_z))
    }

    /// Both layers of a chunk's section: `(block, sky)`.
    pub fn section_layers(
        &self,
        cx: i32,
        section_y: i32,
        cz: i32,
    ) -> (Option<LayerRef>, Option<LayerRef>) {
        (
            self.layer(LightLayer::Block, cx, section_y, cz),
            self.layer(LightLayer::Sky, cx, section_y, cz),
        )
    }

    /// The visible light value of a layer at a block position.
    pub fn light_value(&self, layer: LightLayer, x: i32, y: i32, z: i32) -> u8 {
        self.engine.layer_value(layer, x, y, z)
    }

    /// `getRawBrightness(pos, skyDampen)`.
    pub fn raw_brightness(&self, x: i32, y: i32, z: i32, sky_dampen: i32) -> i32 {
        self.engine.raw_brightness(x, y, z, sky_dampen)
    }

    /// The full-chunk light payload (both filters `None`), after a flush.
    pub fn chunk_light_packet(&mut self, cx: i32, cz: i32) -> LightUpdatePacketData {
        self.flush();
        LightUpdatePacketData::from_engine(&self.engine, cx, cz, None, None)
    }

    /// The per-tick change broadcast: one payload per holder whose filters are non-empty, in the order they
    /// became dirty; clears every filter. Flushes first.
    pub fn take_light_update_packets(&mut self) -> Vec<((i32, i32), LightUpdatePacketData)> {
        self.flush();
        let order = std::mem::take(&mut self.tracker.order);
        let mut out = Vec::new();
        for pos in order {
            if let Some((sky, block)) = self.tracker.filters.get_mut(&pos) {
                if !sky.is_empty() || !block.is_empty() {
                    out.push((
                        pos,
                        LightUpdatePacketData::from_engine(
                            &self.engine,
                            pos.0,
                            pos.1,
                            Some(sky),
                            Some(block),
                        ),
                    ));
                }
                sky.clear();
                block.clear();
            }
        }
        out
    }

    /// The block position helper: the world block node.
    pub const fn node(x: i32, y: i32, z: i32) -> i64 {
        block_node(x, y, z)
    }
}
