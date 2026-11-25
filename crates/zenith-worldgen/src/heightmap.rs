//! Block tags, fast block-state heightmap/fluid predicates, data-version-5006 state resolution,
//! and chunk heightmaps (`docs/specs/worldgen/density-functions.md` §7,
//! `docs/specs/worldgen/surface-and-carvers.md` §4,
//! `docs/specs/registry/identifiers-registries-tags.md` §3.3 & §4).

use std::collections::HashMap;

use zenith_codec::format::PackType;
use zenith_codec::resources::PackResources;
use zenith_registry::{BlockStateTable, Identifier, MemoryTagSource, TagElementLookup, TagLoader};

use crate::data::LoadError;
use crate::settings::BlockStateSpec;

const BLOCK_TAG_DIR: &str = "tags/block";

/// Tag ID controlling `OCEAN_FLOOR`, `OCEAN_FLOOR_WG`, and `MOTION_BLOCKING` solid checks.
pub const TAG_BLOCKS_MOTION_IN_HEIGHTMAP: &str = "minecraft:blocks_motion_in_heightmap";

/// Tag ID controlling `MOTION_BLOCKING_NO_LEAVES` solid checks.
pub const TAG_BLOCKS_MOTION_IN_HEIGHTMAP_NO_LEAVES: &str =
    "minecraft:blocks_motion_in_heightmap_no_leaves";

/// Resolved block tags (`data/<ns>/tags/block/*.json`) keyed by tag identifier.
///
/// Each tag stores deduplicated block protocol IDs from [`BlockStateTable`].
#[derive(Debug, Clone, Default)]
pub struct BlockTags {
    tags: HashMap<Identifier, Vec<u32>>,
}

struct BlockTableLookup<'a>(&'a BlockStateTable);

impl TagElementLookup<u32> for BlockTableLookup<'_> {
    type DedupKey = u32;

    fn lookup(&mut self, id: &Identifier, _required: bool) -> Option<u32> {
        self.0.block(id).map(|b| b.block_protocol_id)
    }

    fn dedup_key(element: &u32) -> Self::DedupKey {
        *element
    }
}

impl BlockTags {
    /// Creates an empty block-tag map (no blocks belong to any tag).
    pub fn empty() -> Self {
        Self::default()
    }

    /// Creates a `BlockTags` directly from pre-resolved `(tag_id -> block_protocol_ids)` entries.
    pub fn from_resolved(tags: HashMap<Identifier, Vec<u32>>) -> Self {
        Self { tags }
    }

    /// Loads and resolves all `tags/block/*.json` files from `pack` against `blocks`, following
    /// `docs/specs/registry/identifiers-registries-tags.md` §4.
    pub fn load(pack: &dyn PackResources, blocks: &BlockStateTable) -> Result<Self, LoadError> {
        let prefix = format!("{BLOCK_TAG_DIR}/");
        let mut source = MemoryTagSource::new().with_pack_name(0, pack.pack_id());

        for ns in pack.namespaces(PackType::Data) {
            let listed = pack.list(PackType::Data, &ns, BLOCK_TAG_DIR)?;
            for file_id in listed {
                if !file_id.path().ends_with(".json") || !file_id.path().starts_with(&prefix) {
                    continue;
                }
                let Some(rel) = file_id
                    .path()
                    .strip_prefix(&prefix)
                    .and_then(|s| s.strip_suffix(".json"))
                else {
                    continue;
                };
                let Ok(tag_id) = Identifier::from_parts(file_id.namespace(), rel) else {
                    continue;
                };
                let content = pack
                    .get(PackType::Data, &file_id)
                    .ok_or_else(|| format!("missing tag file {file_id}"));
                source.add_file(0, tag_id, content);
            }
        }

        let mut lookup = BlockTableLookup(blocks);
        let result = TagLoader::load_and_resolve(&source, &mut lookup);
        let tags: HashMap<Identifier, Vec<u32>> = result.resolved.into_iter().collect();
        Ok(Self { tags })
    }

    /// Returns the resolved block protocol IDs belonging to `tag_id`, if present.
    #[inline]
    pub fn block_ids(&self, tag_id: &Identifier) -> Option<&[u32]> {
        self.tags.get(tag_id).map(Vec::as_slice)
    }

    /// Returns `true` if `block_protocol_id` belongs to `tag_id`.
    #[inline]
    pub fn contains_block(&self, tag_id: &Identifier, block_protocol_id: u32) -> bool {
        self.tags
            .get(tag_id)
            .is_some_and(|ids| ids.contains(&block_protocol_id))
    }

    /// Number of resolved block tags.
    #[inline]
    pub fn len(&self) -> usize {
        self.tags.len()
    }

    /// Returns `true` if no block tags are loaded.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.tags.is_empty()
    }
}

/// Heightmap variants defined by Minecraft: Java Edition 26.3 (`Heightmap.Types`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HeightmapKind {
    /// Worldgen ocean floor: `state.is(#minecraft:blocks_motion_in_heightmap)`.
    OceanFloorWg,
    /// Worldgen surface: `!state.isAir()`.
    WorldSurfaceWg,
    /// Final ocean floor: `state.is(#minecraft:blocks_motion_in_heightmap)`.
    OceanFloor,
    /// Final world surface: `!state.isAir()`.
    WorldSurface,
    /// Motion-blocking blocks or fluids:
    /// `state.is(#minecraft:blocks_motion_in_heightmap) || !state.getFluidState().isEmpty()`.
    MotionBlocking,
    /// Motion-blocking (excluding leaves) or fluids:
    /// `state.is(#minecraft:blocks_motion_in_heightmap_no_leaves) || !state.getFluidState().isEmpty()`.
    MotionBlockingNoLeaves,
}

impl HeightmapKind {
    /// The two worldgen heightmaps maintained during `doFill`, `buildSurface`, and carvers.
    pub const WORLDGEN: [Self; 2] = [Self::OceanFloorWg, Self::WorldSurfaceWg];

    /// The four final heightmaps primed at the end of `ChunkStatus.TERRAIN`.
    pub const FINAL: [Self; 4] = [
        Self::OceanFloor,
        Self::WorldSurface,
        Self::MotionBlocking,
        Self::MotionBlockingNoLeaves,
    ];

    /// The four heightmaps probed by `TerrainProbe` mode `column` (exact order).
    pub const BASE_COLUMN_PROBE: [Self; 4] = [
        Self::OceanFloorWg,
        Self::WorldSurfaceWg,
        Self::MotionBlocking,
        Self::MotionBlockingNoLeaves,
    ];

    /// Vanilla serialization name (e.g. `"OCEAN_FLOOR_WG"`).
    #[inline]
    pub const fn serialization_name(self) -> &'static str {
        match self {
            Self::OceanFloorWg => "OCEAN_FLOOR_WG",
            Self::WorldSurfaceWg => "WORLD_SURFACE_WG",
            Self::OceanFloor => "OCEAN_FLOOR",
            Self::WorldSurface => "WORLD_SURFACE",
            Self::MotionBlocking => "MOTION_BLOCKING",
            Self::MotionBlockingNoLeaves => "MOTION_BLOCKING_NO_LEAVES",
        }
    }
}

const FLAG_IS_AIR: u8 = 1 << 0;
const FLAG_HAS_FLUID: u8 = 1 << 1;
const FLAG_IS_WATER: u8 = 1 << 2;
const FLAG_IS_LAVA: u8 = 1 << 3;
const FLAG_BLOCKS_MOTION: u8 = 1 << 4;
const FLAG_BLOCKS_MOTION_NO_LEAVES: u8 = 1 << 5;

/// Precomputed O(1) block-state predicates for terrain fill, aquifers, and heightmaps.
#[derive(Debug, Clone)]
pub struct BlockPredicates {
    air_id: u32,
    water_id: u32,
    lava_id: u32,
    state_flags: Vec<u8>,
}

impl BlockPredicates {
    /// Builds per-state lookup flags from `blocks` and resolved `tags`.
    ///
    /// Returns `Err(missing_block_id)` if `minecraft:air`, `minecraft:water`, or `minecraft:lava`
    /// is missing from `blocks`.
    pub fn new(blocks: &BlockStateTable, tags: &BlockTags) -> Result<Self, Identifier> {
        let air_block = Identifier::with_default_namespace("air").expect("valid id");
        let cave_air_block = Identifier::with_default_namespace("cave_air").expect("valid id");
        let void_air_block = Identifier::with_default_namespace("void_air").expect("valid id");
        let water_block = Identifier::with_default_namespace("water").expect("valid id");
        let lava_block = Identifier::with_default_namespace("lava").expect("valid id");
        let bubble_column_block =
            Identifier::with_default_namespace("bubble_column").expect("valid id");
        let kelp_block = Identifier::with_default_namespace("kelp").expect("valid id");
        let kelp_plant_block = Identifier::with_default_namespace("kelp_plant").expect("valid id");
        let seagrass_block = Identifier::with_default_namespace("seagrass").expect("valid id");
        let tall_seagrass_block =
            Identifier::with_default_namespace("tall_seagrass").expect("valid id");

        let air_id = blocks
            .block(&air_block)
            .ok_or_else(|| air_block.clone())?
            .default_state;
        let water_id = blocks
            .block(&water_block)
            .ok_or_else(|| water_block.clone())?
            .default_state;
        let lava_id = blocks
            .block(&lava_block)
            .ok_or_else(|| lava_block.clone())?
            .default_state;

        let motion_tag = Identifier::parse(TAG_BLOCKS_MOTION_IN_HEIGHTMAP).expect("valid tag id");
        let motion_no_leaves_tag =
            Identifier::parse(TAG_BLOCKS_MOTION_IN_HEIGHTMAP_NO_LEAVES).expect("valid tag id");

        let block_count = blocks.blocks().len();
        let mut block_flags = vec![0u8; block_count];

        if let Some(ids) = tags.block_ids(&motion_tag) {
            for &bid in ids {
                if let Some(slot) = block_flags.get_mut(bid as usize) {
                    *slot |= FLAG_BLOCKS_MOTION;
                }
            }
        }
        if let Some(ids) = tags.block_ids(&motion_no_leaves_tag) {
            for &bid in ids {
                if let Some(slot) = block_flags.get_mut(bid as usize) {
                    *slot |= FLAG_BLOCKS_MOTION_NO_LEAVES;
                }
            }
        }

        let mut state_flags = vec![0u8; blocks.len()];
        for entry in blocks.blocks() {
            let mut base = block_flags[entry.block_protocol_id as usize];
            if entry.id == air_block || entry.id == cave_air_block || entry.id == void_air_block {
                base |= FLAG_IS_AIR;
            }
            if entry.id == water_block {
                base |= FLAG_IS_WATER | FLAG_HAS_FLUID;
            } else if entry.id == lava_block {
                base |= FLAG_IS_LAVA | FLAG_HAS_FLUID;
            } else if entry.id == bubble_column_block
                || entry.id == kelp_block
                || entry.id == kelp_plant_block
                || entry.id == seagrass_block
                || entry.id == tall_seagrass_block
            {
                base |= FLAG_HAS_FLUID;
            }

            let has_waterlogged_prop = entry.properties.iter().any(|p| p.name() == "waterlogged");
            let first = entry.first_state as usize;
            let count = entry.state_count as usize;
            if !has_waterlogged_prop {
                state_flags[first..first + count].fill(base);
            } else {
                for offset in 0..count {
                    let sid = first + offset;
                    let mut flags = base;
                    if let Some(state) = blocks.state_by_id_exact(sid as i32)
                        && state.property_value("waterlogged") == Some("true")
                    {
                        flags |= FLAG_HAS_FLUID;
                    }
                    state_flags[sid] = flags;
                }
            }
        }

        Ok(Self {
            air_id,
            water_id,
            lava_id,
            state_flags,
        })
    }

    /// Global block-state ID of `minecraft:air`'s default state.
    #[inline]
    pub const fn air_id(&self) -> u32 {
        self.air_id
    }

    /// Global block-state ID of `minecraft:water`'s default state (`level=0`).
    #[inline]
    pub const fn water_id(&self) -> u32 {
        self.water_id
    }

    /// Global block-state ID of `minecraft:lava`'s default state (`level=0`).
    #[inline]
    pub const fn lava_id(&self) -> u32 {
        self.lava_id
    }

    #[inline]
    fn flags(&self, state_id: u32) -> u8 {
        self.state_flags
            .get(state_id as usize)
            .copied()
            .unwrap_or(FLAG_IS_AIR)
    }

    /// `state.isAir()` (`minecraft:air`, `minecraft:cave_air`, `minecraft:void_air`).
    #[inline]
    pub fn is_air(&self, state_id: u32) -> bool {
        (self.flags(state_id) & FLAG_IS_AIR) != 0
    }

    /// `!state.getFluidState().isEmpty()`.
    #[inline]
    pub fn has_fluid(&self, state_id: u32) -> bool {
        (self.flags(state_id) & FLAG_HAS_FLUID) != 0
    }

    /// `state.is(Blocks.WATER)`.
    #[inline]
    pub fn is_water(&self, state_id: u32) -> bool {
        (self.flags(state_id) & FLAG_IS_WATER) != 0
    }

    /// `state.is(Blocks.LAVA)`.
    #[inline]
    pub fn is_lava(&self, state_id: u32) -> bool {
        (self.flags(state_id) & FLAG_IS_LAVA) != 0
    }

    /// Evaluates `Heightmap.Types::isOpaque` for `kind` and `state_id` (26.3 predicates).
    #[inline]
    pub fn is_opaque(&self, kind: HeightmapKind, state_id: u32) -> bool {
        let f = self.flags(state_id);
        match kind {
            HeightmapKind::WorldSurface | HeightmapKind::WorldSurfaceWg => (f & FLAG_IS_AIR) == 0,
            HeightmapKind::OceanFloor | HeightmapKind::OceanFloorWg => {
                (f & FLAG_BLOCKS_MOTION) != 0
            }
            HeightmapKind::MotionBlocking => (f & (FLAG_BLOCKS_MOTION | FLAG_HAS_FLUID)) != 0,
            HeightmapKind::MotionBlockingNoLeaves => {
                (f & (FLAG_BLOCKS_MOTION_NO_LEAVES | FLAG_HAS_FLUID)) != 0
            }
        }
    }
}

/// Resolves a data-version-5006 [`BlockStateSpec`] into its global state ID in `blocks`,
/// following `docs/specs/registry/identifiers-registries-tags.md` §3.3.
///
/// - Unknown block identifier fails with `Err(spec.name)`.
/// - Missing, unknown, or invalid property entries silently fall back to the block's default state.
pub fn resolve_block_state(
    blocks: &BlockStateTable,
    spec: &BlockStateSpec,
) -> Result<u32, Identifier> {
    let entry = blocks.block(&spec.name).ok_or_else(|| spec.name.clone())?;
    if entry.properties.is_empty() || spec.properties.is_empty() {
        return Ok(entry.default_state);
    }

    let default_state = blocks
        .state_by_id_exact(entry.default_state as i32)
        .expect("default_state is always a valid state in BlockStateTable");

    let mut offset: u32 = 0;
    for prop in &entry.properties {
        let val_idx = spec
            .properties
            .get(prop.name())
            .and_then(|v| prop.values().iter().position(|pv| pv == v))
            .unwrap_or_else(|| {
                let default_val = default_state
                    .property_value(prop.name())
                    .expect("default state carries every block property");
                prop.values()
                    .iter()
                    .position(|pv| pv == default_val)
                    .expect("default value exists in property values")
            }) as u32;
        offset = offset * (prop.values().len() as u32) + val_idx;
    }

    Ok(entry.first_state + offset)
}

/// Per-chunk 16×16 heightmap storing the first available Y (`highest_opaque_y + 1`, or `min_y`).
///
/// Indexed as `z * 16 + x` for local coordinates `x, z in 0..16` (matching `terrain-chunks.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heightmap {
    kind: HeightmapKind,
    min_y: i32,
    heights: [i32; 256],
}

impl Heightmap {
    /// Creates an empty (all-`min_y`) heightmap of `kind`.
    pub fn new(kind: HeightmapKind, min_y: i32) -> Self {
        Self {
            kind,
            min_y,
            heights: [min_y; 256],
        }
    }

    /// Returns this heightmap's kind.
    #[inline]
    pub const fn kind(&self) -> HeightmapKind {
        self.kind
    }

    /// Returns the minimum chunk Y coordinate.
    #[inline]
    pub const fn min_y(&self) -> i32 {
        self.min_y
    }

    /// Returns the first available Y at chunk-local `(x, z)` (`x, z in 0..16`).
    #[inline]
    pub fn first_available(&self, local_x: usize, local_z: usize) -> i32 {
        self.heights[local_z * 16 + local_x]
    }

    /// Returns all 256 heights in `z * 16 + x` order.
    #[inline]
    pub const fn heights(&self) -> &[i32; 256] {
        &self.heights
    }

    /// Fast top-down heightmap update for `doFill` on an initially empty chunk (strictly descending Y).
    #[inline]
    pub fn update_top_down(
        &mut self,
        local_x: usize,
        block_y: i32,
        local_z: usize,
        state_id: u32,
        predicates: &BlockPredicates,
    ) -> bool {
        let idx = local_z * 16 + local_x;
        let first = self.heights[idx];
        if block_y >= first && predicates.is_opaque(self.kind, state_id) {
            self.heights[idx] = block_y + 1;
            true
        } else {
            false
        }
    }

    /// Full vanilla `Heightmap.update(x, y, z, state)` semantics (handles both raising and lowering).
    pub fn update(
        &mut self,
        local_x: usize,
        block_y: i32,
        local_z: usize,
        state_id: u32,
        predicates: &BlockPredicates,
        mut get_block: impl FnMut(usize, i32, usize) -> u32,
    ) -> bool {
        let idx = local_z * 16 + local_x;
        let first = self.heights[idx];
        if block_y <= first - 2 {
            return false;
        }
        if predicates.is_opaque(self.kind, state_id) {
            if block_y >= first {
                self.heights[idx] = block_y + 1;
                return true;
            }
        } else if first - 1 == block_y {
            let mut y = block_y - 1;
            while y >= self.min_y {
                if predicates.is_opaque(self.kind, get_block(local_x, y, local_z)) {
                    self.heights[idx] = y + 1;
                    return true;
                }
                y -= 1;
            }
            self.heights[idx] = self.min_y;
            return true;
        }
        false
    }

    /// Primes a heightmap by scanning each `(x, z)` column from `min_y + height - 1` down to `min_y`.
    pub fn prime(
        kind: HeightmapKind,
        min_y: i32,
        height: i32,
        predicates: &BlockPredicates,
        mut get_block: impl FnMut(usize, i32, usize) -> u32,
    ) -> Self {
        let mut hm = Self::new(kind, min_y);
        if height <= 0 {
            return hm;
        }
        let max_y = min_y + height - 1;
        for z in 0..16 {
            for x in 0..16 {
                let mut y = max_y;
                while y >= min_y {
                    if predicates.is_opaque(kind, get_block(x, y, z)) {
                        hm.heights[z * 16 + x] = y + 1;
                        break;
                    }
                    y -= 1;
                }
            }
        }
        hm
    }
}
