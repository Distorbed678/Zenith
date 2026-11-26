//! The client-side light engines (`chunk-steps.md` §9): synchronous, driven by queued packet closures and one
//! engine run per frame.

use std::collections::VecDeque;
use std::sync::Arc;

use crate::change::{ChangeMode, apply_block_change};
use crate::chunk::{ChunkBlocks, LightChunk, LitChunk};
use crate::engine::LevelHeight;
use crate::facts::LightFacts;
use crate::level::LevelLightEngine;
use crate::packet::LightUpdatePacketData;
use crate::pos::section_node;
use crate::storage::{LayerRef, LightLayer};
use crate::world::ChunkMap;

#[derive(Debug)]
enum ClientTask {
    /// `handleLevelChunkWithLight`: `full` also registers the sections.
    ChunkLight {
        cx: i32,
        cz: i32,
        data: LightUpdatePacketData,
        full: bool,
    },
    Forget {
        cx: i32,
        cz: i32,
    },
}

/// A client level's chunks and light engines.
#[derive(Debug)]
pub struct ClientLightWorld<B: ChunkBlocks> {
    facts: Arc<LightFacts>,
    engine: LevelLightEngine,
    chunks: ChunkMap<B>,
    queue: VecDeque<ClientTask>,
}

impl<B: ChunkBlocks> ClientLightWorld<B> {
    /// A client level with block light and (optionally) sky light.
    pub fn new(facts: Arc<LightFacts>, level: LevelHeight, has_skylight: bool) -> Self {
        Self {
            engine: LevelLightEngine::new(facts.clone(), level, has_skylight),
            facts,
            chunks: ChunkMap::default(),
            queue: VecDeque::new(),
        }
    }

    /// The level engine.
    pub fn engine(&self) -> &LevelLightEngine {
        &self.engine
    }

    /// The chunks.
    pub fn chunks(&self) -> &ChunkMap<B> {
        &self.chunks
    }

    /// Number of queued packet closures.
    pub fn queued_tasks(&self) -> usize {
        self.queue.len()
    }

    /// A chunk packet with light: the chunk data is applied now (sources rebuilt), the light payload is queued.
    /// `full` is `true` for a full chunk packet, `false` for a light-update packet (the chunk then must exist).
    pub fn handle_chunk_with_light(
        &mut self,
        chunk: Option<LitChunk<B>>,
        cx: i32,
        cz: i32,
        data: LightUpdatePacketData,
        full: bool,
    ) {
        if let Some(mut c) = chunk {
            c.ensure_counts(&self.facts);
            c.sky_sources.fill_from(&c.blocks, &self.facts);
            self.chunks.insert(c);
        }
        self.queue
            .push_back(ClientTask::ChunkLight { cx, cz, data, full });
    }

    /// `ClientboundForgetLevelChunkPacket`: queued.
    pub fn handle_forget_chunk(&mut self, cx: i32, cz: i32) {
        self.queue.push_back(ClientTask::Forget { cx, cz });
    }

    /// `pollLightUpdates`: runs `size < 1000 ? max(10, size / 10) : size` queued closures.
    pub fn poll_light_updates(&mut self) {
        let size = self.queue.len();
        let n = if size < 1000 {
            (size / 10).max(10)
        } else {
            size
        };
        for _ in 0..n {
            let Some(t) = self.queue.pop_front() else {
                break;
            };
            self.run_task(t);
        }
    }

    /// One frame: poll, then one engine run (block, then sky). Returns the processed-entry count.
    pub fn run_frame(&mut self) -> usize {
        self.poll_light_updates();
        self.engine.run_light_updates(&self.chunks)
    }

    /// Runs frames until the queue is empty and the engines are idle.
    pub fn settle(&mut self) {
        while !self.queue.is_empty() || self.engine.has_light_work() {
            self.run_frame();
        }
    }

    fn run_task(&mut self, task: ClientTask) {
        match task {
            ClientTask::ChunkLight { cx, cz, data, full } => {
                let min = self.engine.min_light_section();
                for (layer, index, l) in data.layers() {
                    let node = section_node(cx, min + index as i32, cz);
                    self.engine.queue_section_data(layer, node, Some(l));
                }
                self.engine.set_light_enabled(&self.chunks, cx, cz, true);
                if full && let Some(chunk) = self.chunks.get(cx, cz) {
                    let min_section = self.engine.level().min_section();
                    let nodes: Vec<(i64, bool)> = (0..chunk.section_count())
                        .map(|i| {
                            (
                                section_node(cx, min_section + i as i32, cz),
                                chunk.section_only_air(i, &self.facts),
                            )
                        })
                        .collect();
                    for (node, empty) in nodes {
                        self.engine.update_section_status(node, empty);
                    }
                }
            }
            ClientTask::Forget { cx, cz } => {
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
                self.chunks.remove(cx, cz);
            }
        }
    }

    /// A locally predicted block change: the same code as the server's level chunk, acting directly on the
    /// engines. The next frame's run recomputes the light.
    pub fn set_block(&mut self, x: i32, y: i32, z: i32, state: u32) -> Option<u32> {
        let facts = self.facts.clone();
        let chunk = self.chunks.get_mut(x >> 4, z >> 4)?;
        let change = apply_block_change(chunk, &facts, ChangeMode::Level, x, y, z, state);
        if let Some((node, empty)) = change.calls.section_status {
            self.engine.update_section_status(node, empty);
        }
        if let Some(node) = change.calls.check_block {
            self.engine.check_block(node);
        }
        change.old
    }

    /// The visible light value of a layer.
    pub fn light_value(&self, layer: LightLayer, x: i32, y: i32, z: i32) -> u8 {
        self.engine.layer_value(layer, x, y, z)
    }

    /// The queued-or-visible layer of a section.
    pub fn layer(&self, layer: LightLayer, sx: i32, sy: i32, sz: i32) -> Option<LayerRef> {
        self.engine.data_layer_data(layer, section_node(sx, sy, sz))
    }
}
