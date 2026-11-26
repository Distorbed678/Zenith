//! Per-chunk "lowest sky source" table (`sky-light-sources.md`).

use zenith_blocks::Direction;

use crate::chunk::ChunkBlocks;
use crate::facts::LightFacts;

/// One lowest-source Y per column; the sentinel `i32::MIN` means "sources reach below the world".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkyLightSources {
    /// `worldMinY - 1`.
    min_y: i32,
    /// `value - min_y` per column, index `x + 16 * z`.
    raw: Box<[i32; 256]>,
}

impl SkyLightSources {
    /// A fresh all-zero table (every column reads `i32::MIN`) for a chunk of `height` blocks from `world_min_y`.
    pub fn new(world_min_y: i32, _height: i32) -> Self {
        Self {
            min_y: world_min_y - 1,
            raw: Box::new([0; 256]),
        }
    }

    /// `getLowestSourceY(x, z)`.
    #[inline]
    pub fn lowest_source_y(&self, x: usize, z: usize) -> i32 {
        let v = self.raw[x + z * 16] + self.min_y;
        if v == self.min_y { i32::MIN } else { v }
    }

    /// `getHighestLowestSourceY()`.
    pub fn highest_lowest_source_y(&self) -> i32 {
        let v = self.raw.iter().copied().max().unwrap_or(0) + self.min_y;
        if v == self.min_y { i32::MIN } else { v }
    }

    fn current(&self, x: usize, z: usize) -> i32 {
        self.raw[x + z * 16] + self.min_y
    }

    fn store(&mut self, x: usize, z: usize, y: i32) {
        self.raw[x + z * 16] = y - self.min_y;
    }

    /// `fillFrom(chunk)`: recomputes every column from the block states.
    pub fn fill_from<B: ChunkBlocks + ?Sized>(&mut self, blocks: &B, facts: &LightFacts) {
        let sections = (blocks.height() / 16) as usize;
        let top = (0..sections)
            .rev()
            .find(|&i| !blocks.section_only_air(i, facts));
        let Some(top) = top else {
            self.raw.fill(0);
            return;
        };
        for z in 0..16 {
            for x in 0..16 {
                let v = find_lowest_source_y(blocks, facts, top, x, z).max(self.min_y);
                self.store(x, z, v);
            }
        }
    }

    /// `update(chunk, x, y, z)`: incremental update after the block at local `(x, z)`, world `y` changed light
    /// properties; returns whether the table changed.
    pub fn update<B: ChunkBlocks + ?Sized>(
        &mut self,
        blocks: &B,
        facts: &LightFacts,
        x: usize,
        y: i32,
        z: usize,
    ) -> bool {
        let current = self.current(x, z);
        let upper_edge_y = y + 1;
        if upper_edge_y < current {
            return false;
        }
        let top_state = block_or_air(blocks, facts, x, y + 1, z);
        let middle_state = block_or_air(blocks, facts, x, y, z);
        if self.update_edge(blocks, facts, x, z, (y + 1, top_state), (y, middle_state)) {
            return true;
        }
        let bottom_state = block_or_air(blocks, facts, x, y - 1, z);
        self.update_edge(
            blocks,
            facts,
            x,
            z,
            (y, middle_state),
            (y - 1, bottom_state),
        )
    }

    fn update_edge<B: ChunkBlocks + ?Sized>(
        &mut self,
        blocks: &B,
        facts: &LightFacts,
        x: usize,
        z: usize,
        top: (i32, u32),
        bottom: (i32, u32),
    ) -> bool {
        let current = self.current(x, z);
        let checked_edge_y = top.0;
        if is_edge_occluded(facts, top.1, bottom.1) {
            if checked_edge_y > current {
                self.store(x, z, checked_edge_y);
                return true;
            }
            false
        } else if checked_edge_y == current {
            let v = self.find_lowest_source_below(blocks, facts, x, z, bottom);
            self.store(x, z, v);
            true
        } else {
            false
        }
    }

    fn find_lowest_source_below<B: ChunkBlocks + ?Sized>(
        &self,
        blocks: &B,
        facts: &LightFacts,
        x: usize,
        z: usize,
        start: (i32, u32),
    ) -> i32 {
        let (mut top_y, mut top_state) = start;
        let mut bottom_y = start.0 - 1;
        while bottom_y >= self.min_y {
            let bottom_state = block_or_air(blocks, facts, x, bottom_y, z);
            if is_edge_occluded(facts, top_state, bottom_state) {
                return top_y;
            }
            top_state = bottom_state;
            top_y = bottom_y;
            bottom_y -= 1;
        }
        self.min_y
    }
}

fn block_or_air<B: ChunkBlocks + ?Sized>(
    blocks: &B,
    facts: &LightFacts,
    x: usize,
    y: i32,
    z: usize,
) -> u32 {
    if y < blocks.min_y() || y >= blocks.min_y() + blocks.height() {
        facts.air_state()
    } else {
        blocks.state(x, y, z)
    }
}

/// `isEdgeOccluded(top, bottom)`: only the lower block's dampening counts, or the face shapes close the gap.
#[inline]
pub fn is_edge_occluded(facts: &LightFacts, top: u32, bottom: u32) -> bool {
    facts.dampening(bottom) != 0 || facts.shape_occludes(top, bottom, Direction::Down)
}

fn find_lowest_source_y<B: ChunkBlocks + ?Sized>(
    blocks: &B,
    facts: &LightFacts,
    top_index: usize,
    x: usize,
    z: usize,
) -> i32 {
    let min_y = blocks.min_y();
    let mut top_edge_y = min_y + (top_index as i32 + 1) * 16;
    let mut top_state = facts.air_state();
    for index in (0..=top_index).rev() {
        let base = min_y + index as i32 * 16;
        if blocks.section_only_air(index, facts) {
            top_state = facts.air_state();
            top_edge_y = base;
            continue;
        }
        for ly in (0..16).rev() {
            let bottom_state = blocks.state(x, base + ly, z);
            if is_edge_occluded(facts, top_state, bottom_state) {
                return top_edge_y;
            }
            top_state = bottom_state;
            top_edge_y = base + ly;
        }
    }
    min_y - 1
}
