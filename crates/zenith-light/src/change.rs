//! The block-change enqueue rules shared by the server world and the client (`chunk-steps.md` §7).

use crate::chunk::{ChunkBlocks, LightStatus, LitChunk};
use crate::facts::LightFacts;
use crate::pos::{block_node, section_node};

/// Which `setBlockState` flavour applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeMode {
    /// A level chunk (status `full`, or any client chunk).
    Level,
    /// A proto chunk (light notifications only from `initialize_light` on).
    Proto,
}

/// The light-engine calls a block change asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LightCalls {
    /// `updateSectionStatus(section node, emptiness)`.
    pub section_status: Option<(i64, bool)>,
    /// `checkBlock(block node)`.
    pub check_block: Option<i64>,
}

/// Result of [`apply_block_change`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockChange {
    /// The previous state, or `None` when nothing was written (early return or out of range).
    pub old: Option<u32>,
    /// What to tell the engines.
    pub calls: LightCalls,
}

/// `LevelChunk.setBlockState` / `ProtoChunk.setBlockState`, light part only: writes the state, keeps the section
/// emptiness counts, updates the sky-source table immediately and reports the engine calls to make.
#[allow(clippy::too_many_arguments)]
pub fn apply_block_change<B: ChunkBlocks>(
    chunk: &mut LitChunk<B>,
    facts: &LightFacts,
    mode: ChangeMode,
    x: i32,
    y: i32,
    z: i32,
    state: u32,
) -> BlockChange {
    let none = BlockChange {
        old: None,
        calls: LightCalls::default(),
    };
    let min_y = chunk.blocks.min_y();
    if y < min_y || y >= min_y + chunk.blocks.height() {
        return none;
    }
    chunk.ensure_counts(facts);
    let index = ((y >> 4) - (min_y >> 4)) as usize;
    let was_empty = chunk.counts_mut()[index] == 0;
    match mode {
        ChangeMode::Level if was_empty && facts.is_air(state) => return none,
        ChangeMode::Proto if was_empty && state == facts.air_state() => return none,
        _ => {}
    }
    let (lx, lz) = ((x & 15) as usize, (z & 15) as usize);
    let old = chunk.blocks.set_state(lx, y, lz, state);
    {
        let counts = chunk.counts_mut();
        if !facts.is_air(old) {
            counts[index] -= 1;
        }
        if !facts.is_air(state) {
            counts[index] += 1;
        }
    }
    let mut calls = LightCalls::default();
    if mode == ChangeMode::Level && old == state {
        return BlockChange {
            old: Some(old),
            calls,
        };
    }
    if mode == ChangeMode::Proto && chunk.status < LightStatus::InitializeLight {
        return BlockChange {
            old: Some(old),
            calls,
        };
    }
    let is_empty = chunk.counts_mut()[index] == 0;
    if is_empty != was_empty {
        calls.section_status = Some((section_node(x >> 4, y >> 4, z >> 4), is_empty));
    }
    if facts.has_different_light_properties(old, state) {
        chunk.sky_sources.update(&chunk.blocks, facts, lx, y, lz);
        calls.check_block = Some(block_node(x, y, z));
    }
    BlockChange {
        old: Some(old),
        calls,
    }
}
