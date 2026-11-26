//! What the light engines need from a chunk, and a ready-made wrapper that adds the light-owned chunk state.

use crate::facts::LightFacts;
use crate::sky_sources::SkyLightSources;

/// Block access of a column of sections: implemented by generated terrain chunks and by live chunks.
///
/// Local `x` / `z` are `0..16`; `y` is a world coordinate inside `[min_y, min_y + height)` (the caller never asks
/// outside it).
pub trait ChunkBlocks {
    /// Lowest block Y of the chunk.
    fn min_y(&self) -> i32;
    /// Height in blocks (a multiple of 16).
    fn height(&self) -> i32;
    /// Global block-state id at the position.
    fn state(&self, x: usize, y: i32, z: usize) -> u32;
    /// Writes a state, returning the previous one.
    fn set_state(&mut self, x: usize, y: i32, z: usize, state: u32) -> u32;
    /// Whether section `index` (0 = lowest) holds only air blocks (`hasOnlyAir`; all air kinds count).
    fn section_only_air(&self, index: usize, facts: &LightFacts) -> bool {
        let base = self.min_y() + index as i32 * 16;
        for y in 0..16 {
            for z in 0..16 {
                for x in 0..16 {
                    if !facts.is_air(self.state(x, base + y, z)) {
                        return false;
                    }
                }
            }
        }
        true
    }
}

/// The engine's view of a chunk found through the chunk-for-lighting accessor.
pub trait LightChunk {
    /// Lowest block Y of the chunk.
    fn min_y(&self) -> i32;
    /// Height in blocks.
    fn height(&self) -> i32;
    /// Global block-state id at local `(x, z)` and world `y` (inside the chunk's height).
    fn block_state(&self, x: usize, y: i32, z: usize) -> u32;
    /// Whether section `index` (0 = lowest) holds only air.
    fn section_only_air(&self, index: usize, facts: &LightFacts) -> bool;
    /// The per-column lowest sky-source table.
    fn sky_sources(&self) -> &SkyLightSources;
}

/// The chunk-for-lighting accessor: the chunk at `(cx, cz)` once it finished `features`, else `None`.
pub trait LightChunkSource {
    /// The chunk visible to lighting at `(cx, cz)`.
    fn chunk_for_lighting(&self, cx: i32, cz: i32) -> Option<&dyn LightChunk>;
}

/// Chunk statuses relevant to lighting, in pipeline order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LightStatus {
    /// Generated up to and including `features` (the chunk is visible to lighting).
    Features,
    /// `initialize_light` done.
    InitializeLight,
    /// `light` done.
    Light,
    /// `spawn` done.
    Spawn,
    /// `full`.
    Full,
}

/// A chunk plus the state the lighting system keeps on it.
#[derive(Debug, Clone)]
pub struct LitChunk<B> {
    /// Chunk X.
    pub cx: i32,
    /// Chunk Z.
    pub cz: i32,
    /// The block storage.
    pub blocks: B,
    /// Lowest source Y per column (derived data, never saved).
    pub sky_sources: SkyLightSources,
    /// The persisted status.
    pub status: LightStatus,
    /// NBT `isLightOn`.
    pub light_correct: bool,
    /// Set whenever a light change reaches the chunk (the chunk would be saved again).
    pub unsaved: bool,
    /// Non-air block count per section, built on demand (see [`LitChunk::ensure_counts`]).
    counts: Option<Vec<u32>>,
}

impl<B: ChunkBlocks> LitChunk<B> {
    /// Wraps a generated chunk (status `features`, never lit).
    pub fn new(cx: i32, cz: i32, blocks: B) -> Self {
        let sky_sources = SkyLightSources::new(blocks.min_y(), blocks.height());
        Self {
            cx,
            cz,
            blocks,
            sky_sources,
            status: LightStatus::Features,
            light_correct: false,
            unsaved: false,
            counts: None,
        }
    }

    /// Same, with a persisted status and `isLightOn` (for chunks loaded from disk).
    pub fn loaded(cx: i32, cz: i32, blocks: B, status: LightStatus, light_correct: bool) -> Self {
        let mut c = Self::new(cx, cz, blocks);
        c.status = status;
        c.light_correct = light_correct;
        c
    }

    /// `isLighted`: persisted status at least `light` and `isLightOn`.
    pub fn is_lighted(&self) -> bool {
        self.status >= LightStatus::Light && self.light_correct
    }

    /// Number of sections.
    pub fn section_count(&self) -> usize {
        (self.blocks.height() / 16) as usize
    }

    /// Builds the per-section non-air counts that make `hasOnlyAir` O(1). Call
    /// [`LitChunk::invalidate_counts`] after writing to `blocks` directly.
    pub fn ensure_counts(&mut self, facts: &LightFacts) {
        if self.counts.is_some() {
            return;
        }
        let min_y = self.blocks.min_y();
        let counts = (0..self.section_count())
            .map(|i| {
                let base = min_y + i as i32 * 16;
                let mut n = 0u32;
                for y in 0..16 {
                    for z in 0..16 {
                        for x in 0..16 {
                            n += u32::from(!facts.is_air(self.blocks.state(x, base + y, z)));
                        }
                    }
                }
                n
            })
            .collect();
        self.counts = Some(counts);
    }

    /// Drops the cached counts (after direct writes to `blocks`).
    pub fn invalidate_counts(&mut self) {
        self.counts = None;
    }

    pub(crate) fn counts_mut(&mut self) -> &mut [u32] {
        self.counts.as_deref_mut().unwrap_or(&mut [])
    }
}

impl<B: ChunkBlocks> LightChunk for LitChunk<B> {
    fn min_y(&self) -> i32 {
        self.blocks.min_y()
    }
    fn height(&self) -> i32 {
        self.blocks.height()
    }
    #[inline]
    fn block_state(&self, x: usize, y: i32, z: usize) -> u32 {
        self.blocks.state(x, y, z)
    }
    fn section_only_air(&self, index: usize, facts: &LightFacts) -> bool {
        match &self.counts {
            Some(c) => c[index] == 0,
            None => self.blocks.section_only_air(index, facts),
        }
    }
    fn sky_sources(&self) -> &SkyLightSources {
        &self.sky_sources
    }
}

#[cfg(feature = "worldgen")]
impl ChunkBlocks for zenith_worldgen::TerrainChunk {
    fn min_y(&self) -> i32 {
        self.min_y()
    }
    fn height(&self) -> i32 {
        self.height()
    }
    fn state(&self, x: usize, y: i32, z: usize) -> u32 {
        self.get_block(x, y, z)
    }
    fn set_state(&mut self, x: usize, y: i32, z: usize, state: u32) -> u32 {
        let old = self.get_block(x, y, z);
        self.set_block(x, y, z, state);
        old
    }
    fn section_only_air(&self, index: usize, facts: &LightFacts) -> bool {
        self.section(index).iter().all(|&s| facts.is_air(s))
    }
}

/// A plain in-memory chunk of block-state ids, for tests and tools.
#[derive(Debug, Clone)]
pub struct DenseBlocks {
    min_y: i32,
    height: i32,
    air: u32,
    blocks: Vec<u32>,
}

impl DenseBlocks {
    /// An all-`air` chunk of `height` blocks from `min_y`.
    pub fn new(min_y: i32, height: i32, air: u32) -> Self {
        assert!(height > 0 && height % 16 == 0 && min_y % 16 == 0);
        Self {
            min_y,
            height,
            air,
            blocks: vec![air; (height as usize) * 256],
        }
    }

    #[inline]
    fn index(&self, x: usize, y: i32, z: usize) -> usize {
        ((y - self.min_y) as usize) * 256 + z * 16 + x
    }

    /// Fills a box (inclusive local `x`/`z`, world `y`) with `state`.
    pub fn fill(&mut self, x: (usize, usize), y: (i32, i32), z: (usize, usize), state: u32) {
        for yy in y.0..=y.1 {
            for zz in z.0..=z.1 {
                for xx in x.0..=x.1 {
                    self.set_state(xx, yy, zz, state);
                }
            }
        }
    }
}

impl ChunkBlocks for DenseBlocks {
    fn min_y(&self) -> i32 {
        self.min_y
    }
    fn height(&self) -> i32 {
        self.height
    }
    fn state(&self, x: usize, y: i32, z: usize) -> u32 {
        if y < self.min_y || y >= self.min_y + self.height {
            return self.air;
        }
        self.blocks[self.index(x, y, z)]
    }
    fn set_state(&mut self, x: usize, y: i32, z: usize, state: u32) -> u32 {
        let i = self.index(x, y, z);
        std::mem::replace(&mut self.blocks[i], state)
    }
}
