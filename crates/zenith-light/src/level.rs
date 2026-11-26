//! `LevelLightEngine`: the block engine plus the optional sky engine, and the brightness helpers
//! (`light-engine.md` rule 22, `chunk-steps.md` §10).

use std::sync::Arc;

use crate::chunk::LightChunkSource;
use crate::engine::{LevelHeight, LightEngine};
use crate::facts::LightFacts;
use crate::layer::DataLayer;
use crate::pos::{block_node, chunk_zero, section_node};
use crate::storage::{LayerRef, LightLayer};

/// Both engines of a level (the sky engine exists only in dimensions with sky light).
#[derive(Debug, Clone)]
pub struct LevelLightEngine {
    facts: Arc<LightFacts>,
    level: LevelHeight,
    block: LightEngine,
    sky: Option<LightEngine>,
}

impl LevelLightEngine {
    /// Builds the engines for a level of the given extent.
    pub fn new(facts: Arc<LightFacts>, level: LevelHeight, has_skylight: bool) -> Self {
        Self {
            block: LightEngine::new(LightLayer::Block, facts.clone(), level),
            sky: has_skylight.then(|| LightEngine::new(LightLayer::Sky, facts.clone(), level)),
            facts,
            level,
        }
    }

    /// The per-state light tables.
    pub fn facts(&self) -> &Arc<LightFacts> {
        &self.facts
    }

    /// The level extent.
    pub const fn level(&self) -> LevelHeight {
        self.level
    }

    /// `getMinLightSection`.
    pub const fn min_light_section(&self) -> i32 {
        self.level.min_light_section()
    }

    /// `getMaxLightSection` (exclusive).
    pub const fn max_light_section(&self) -> i32 {
        self.level.max_light_section()
    }

    /// `getLightSectionCount`.
    pub const fn light_section_count(&self) -> i32 {
        self.level.light_section_count()
    }

    /// The engine of a layer (`None` for sky in a dimension without sky light).
    pub fn engine(&self, layer: LightLayer) -> Option<&LightEngine> {
        match layer {
            LightLayer::Block => Some(&self.block),
            LightLayer::Sky => self.sky.as_ref(),
        }
    }

    /// The engine of a layer, mutably.
    pub fn engine_mut(&mut self, layer: LightLayer) -> Option<&mut LightEngine> {
        match layer {
            LightLayer::Block => Some(&mut self.block),
            LightLayer::Sky => self.sky.as_mut(),
        }
    }

    /// Whether the level has a sky engine.
    pub const fn has_sky(&self) -> bool {
        self.sky.is_some()
    }

    fn each(&mut self, mut f: impl FnMut(&mut LightEngine)) {
        f(&mut self.block);
        if let Some(s) = &mut self.sky {
            f(s);
        }
    }

    /// `checkBlock(pos)`: block engine, then sky engine.
    pub fn check_block(&mut self, block_node: i64) {
        self.each(|e| e.check_block(block_node));
    }

    /// `updateSectionStatus(sectionPos, sectionEmpty)` on both engines.
    pub fn update_section_status(&mut self, section_node: i64, section_empty: bool) {
        self.each(|e| e.update_section_status(section_node, section_empty));
    }

    /// `setLightEnabled(chunkPos, enable)` on both engines (the sky engine also fills layers).
    pub fn set_light_enabled(
        &mut self,
        chunks: &dyn LightChunkSource,
        cx: i32,
        cz: i32,
        enable: bool,
    ) {
        self.each(|e| e.set_light_enabled(chunks, cx, cz, enable));
    }

    /// `retainData(chunkPos, retain)` on both engines.
    pub fn retain_data(&mut self, cx: i32, cz: i32, retain: bool) {
        self.each(|e| e.retain_data(chunk_zero(cx, cz), retain));
    }

    /// `queueSectionData(layer, sectionPos, data)`; ignored when the layer has no engine.
    pub fn queue_section_data(
        &mut self,
        layer: LightLayer,
        section_node: i64,
        data: Option<DataLayer>,
    ) {
        if let Some(e) = self.engine_mut(layer) {
            e.queue_section_data(section_node, data);
        }
    }

    /// `propagateLightSources(chunkPos)`: block engine first, then sky.
    pub fn propagate_light_sources(&mut self, chunks: &dyn LightChunkSource, cx: i32, cz: i32) {
        self.each(|e| e.propagate_light_sources(chunks, cx, cz));
    }

    /// `runLightUpdates`: the block engine, then the sky engine; returns the sum of processed entries.
    pub fn run_light_updates(&mut self, chunks: &dyn LightChunkSource) -> usize {
        let mut n = self.block.run_light_updates(chunks);
        if let Some(s) = &mut self.sky {
            n += s.run_light_updates(chunks);
        }
        n
    }

    /// `hasLightWork()`.
    pub fn has_light_work(&self) -> bool {
        self.block.has_light_work() || self.sky.as_ref().is_some_and(LightEngine::has_light_work)
    }

    /// Sections reported by completed runs since the last call, block engine's first.
    pub fn drain_light_updates(&mut self) -> Vec<(LightLayer, i64)> {
        let mut out: Vec<(LightLayer, i64)> = self
            .block
            .drain_light_updates()
            .into_iter()
            .map(|n| (LightLayer::Block, n))
            .collect();
        if let Some(s) = &mut self.sky {
            out.extend(
                s.drain_light_updates()
                    .into_iter()
                    .map(|n| (LightLayer::Sky, n)),
            );
        }
        out
    }

    /// `lightOnInColumn(zeroNode)`.
    pub fn light_on_in_column(&self, zero: i64) -> bool {
        self.block.storage().light_on_in_column(zero)
            && self
                .sky
                .as_ref()
                .is_none_or(|s| s.storage().light_on_in_column(zero))
    }

    /// `getDataLayerData` of a layer (queued, else visible); `None` without an engine.
    pub fn data_layer_data(&self, layer: LightLayer, section_node: i64) -> Option<LayerRef> {
        self.engine(layer)?.data_layer_data(section_node)
    }

    /// The visible light value of a layer (0 without an engine).
    pub fn layer_value(&self, layer: LightLayer, x: i32, y: i32, z: i32) -> u8 {
        self.engine(layer)
            .map_or(0, |e| e.light_value(block_node(x, y, z)))
    }

    /// `getRawBrightness(pos, skyDampen)`: `max(block, sky - skyDampen)`; the dampen applies only with a sky engine.
    pub fn raw_brightness(&self, x: i32, y: i32, z: i32, sky_dampen: i32) -> i32 {
        let node = block_node(x, y, z);
        let sky = self
            .sky
            .as_ref()
            .map_or(0, |s| i32::from(s.light_value(node)) - sky_dampen);
        let block = i32::from(self.block.light_value(node));
        block.max(sky)
    }

    /// `getDebugSectionType`.
    pub fn debug_section_type(&self, layer: LightLayer, section_node: i64) -> u8 {
        self.engine(layer)
            .map_or(2, |e| e.storage().debug_section_type(section_node))
    }

    /// Section node helper for callers working in chunk coordinates.
    pub const fn section_node(cx: i32, sy: i32, cz: i32) -> i64 {
        section_node(cx, sy, cz)
    }
}

/// `getMaxLocalRawBrightness(pos, skyDarken)`: 15 far outside the world border range, else the raw brightness.
pub fn max_local_raw_brightness(
    light: &LevelLightEngine,
    x: i32,
    y: i32,
    z: i32,
    sky_darken: i32,
) -> i32 {
    if !(-30_000_000..30_000_000).contains(&x) || !(-30_000_000..30_000_000).contains(&z) {
        return 15;
    }
    light.raw_brightness(x, y, z, sky_darken)
}

/// `getEffectiveSkyBrightness(pos)`: `max(sky - skyDarken, 0)`.
pub fn effective_sky_brightness(
    light: &LevelLightEngine,
    x: i32,
    y: i32,
    z: i32,
    sky_darken: i32,
) -> i32 {
    (i32::from(light.layer_value(LightLayer::Sky, x, y, z)) - sky_darken).max(0)
}

/// `canSeeSky(pos)`: sky light is 15.
pub fn can_see_sky(light: &LevelLightEngine, x: i32, y: i32, z: i32) -> bool {
    light.layer_value(LightLayer::Sky, x, y, z) >= 15
}

/// `skyDarken = trunc(15.0f - SKY_LIGHT_LEVEL)`.
pub fn sky_darken_from_level(sky_light_level: f32) -> i32 {
    (15.0f32 - sky_light_level) as i32
}

/// `getLightLevelDependentMagicValue(pos)`: `lerp(delta = ambient, start = v / (4 - 3v), end = 1)` with
/// `v = raw / 15`, i.e. `curved + ambient * (1 - curved)`.
pub fn light_level_dependent_magic_value(raw_brightness: i32, ambient_light: f32) -> f32 {
    let v = raw_brightness as f32 / 15.0f32;
    let curved = v / (4.0f32 - 3.0f32 * v);
    curved + ambient_light * (1.0f32 - curved)
}
