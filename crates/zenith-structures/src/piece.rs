//! `StructurePiece` block-placement toolkit used by every legacy piece
//! (`docs/specs/worldgen/structures-legacy-pieces.md` §1.4–§1.6): orientation transform,
//! `placeBlock`, `getBlock`, box/sphere/selector fills with their exact RNG consumption, chests,
//! dispensers and column fills.

use zenith_blocks::flag;
use zenith_nbt::{Compound, Tag};
use zenith_worldgen::HeightmapKind;

use crate::block_xform::{
    StateTransformer, block_path, default_state, get_property, is_block, with_properties,
    with_property,
};
use crate::geom::{
    BlockPos, BoundingBox, BoundingBoxExt, Direction, Mirror, Rotation, dir_step, piece_world_pos,
};
use crate::world::{StructureWorld, WorldgenRandom};

/// `StructurePiece.mirror/rotation` derived from the orientation (§1.4 table).
pub fn orientation_transform(orientation: Option<Direction>) -> (Mirror, Rotation) {
    match orientation {
        None | Some(Direction::North | Direction::Up | Direction::Down) => {
            (Mirror::None, Rotation::None)
        }
        Some(Direction::South) => (Mirror::LeftRight, Rotation::None),
        Some(Direction::West) => (Mirror::LeftRight, Rotation::Clockwise90),
        Some(Direction::East) => (Mirror::None, Rotation::Clockwise90),
    }
}

/// Blocks whose placement marks the position for post-processing (`SHAPE_CHECK_BLOCKS`).
fn is_shape_check_block(path: &str) -> bool {
    matches!(
        path,
        "nether_brick_fence"
            | "torch"
            | "wall_torch"
            | "oak_fence"
            | "spruce_fence"
            | "dark_oak_fence"
            | "pale_oak_fence"
            | "acacia_fence"
            | "birch_fence"
            | "jungle_fence"
            | "ladder"
            | "iron_bars"
    )
}

/// A box fill callback: `(random, x, y, z, is_edge) -> state`.
pub type Selector<'a> = dyn FnMut(&mut dyn WorldgenRandom, i32, i32, i32, bool) -> u32 + 'a;

/// Placement context for one piece in one chunk call.
#[allow(missing_debug_implementations)]
pub struct PieceBuilder<'a> {
    /// World.
    pub world: &'a mut dyn StructureWorld,
    /// The piece's bounding box (world-space).
    pub bb: BoundingBox,
    /// The piece's orientation.
    pub orientation: Option<Direction>,
    /// The writable area / `chunkBB`.
    pub chunk: BoundingBox,
    /// Block paths (no namespace) that `placeBlock` must not overwrite (`canBeReplaced`;
    /// mineshaft pieces protect their own timber and chains).
    pub no_replace: Vec<&'static str>,
    xform: StateTransformer,
}

impl<'a> PieceBuilder<'a> {
    /// Creates a builder for a piece.
    pub fn new(
        world: &'a mut dyn StructureWorld,
        bb: BoundingBox,
        orientation: Option<Direction>,
        chunk: BoundingBox,
    ) -> Self {
        let (mirror, rotation) = orientation_transform(orientation);
        Self {
            world,
            bb,
            orientation,
            chunk,
            no_replace: Vec::new(),
            xform: StateTransformer::new(mirror, rotation),
        }
    }

    // ----- block lookup helpers -----

    /// Default state of `minecraft:<name>` (air if unknown).
    pub fn d(&self, name: &str) -> u32 {
        default_state(self.world.blocks(), name).unwrap_or_else(|| self.air())
    }

    /// State of `minecraft:<name>` with properties applied.
    pub fn s(&self, name: &str, props: &[(&str, &str)]) -> u32 {
        let st = self.d(name);
        with_properties(self.world.blocks(), st, props)
    }

    /// `state.setValue(prop, value)`.
    pub fn with(&self, state: u32, prop: &str, value: &str) -> u32 {
        with_property(self.world.blocks(), state, prop, value)
    }

    /// Property value of a state.
    pub fn prop(&self, state: u32, prop: &str) -> Option<String> {
        get_property(self.world.blocks(), state, prop)
    }

    /// Air state id.
    pub fn air(&self) -> u32 {
        self.world
            .blocks()
            .air_default_state()
            .map_or(0, |s| s.global_id)
    }

    /// Whether `state` is the block `minecraft:<name>`.
    pub fn is(&self, state: u32, name: &str) -> bool {
        is_block(self.world.blocks(), state, name)
    }

    /// Block path (no namespace) of `state`.
    pub fn path_of(&self, state: u32) -> String {
        block_path(self.world.blocks(), state).to_owned()
    }

    /// `state.isAir()`.
    pub fn is_air(&self, state: u32) -> bool {
        self.world.facts().state(state).has(flag::IS_AIR)
    }

    /// `state.liquid()`.
    pub fn is_liquid(&self, state: u32) -> bool {
        self.world.facts().state(state).has(flag::LIQUID)
    }

    /// `state.isSolidRender()`.
    pub fn is_solid_render(&self, state: u32) -> bool {
        self.world.facts().state(state).has(flag::SOLID_RENDER)
    }

    // ----- coordinates -----

    /// `getWorldX/Y/Z`.
    #[inline]
    pub fn world_pos(&self, x: i32, y: i32, z: i32) -> BlockPos {
        piece_world_pos(self.bb, self.orientation, x, y, z)
    }

    /// `getBlock(level, x, y, z, chunkBB)`: air outside `chunkBB`.
    pub fn get_block(&self, x: i32, y: i32, z: i32) -> u32 {
        let pos = self.world_pos(x, y, z);
        if !self.chunk.is_inside(pos) {
            return self.air();
        }
        self.world.get_block_state(pos)
    }

    /// `isInterior(level, x, y, z, chunkBB)`.
    pub fn is_interior(&self, x: i32, y: i32, z: i32) -> bool {
        let pos = self.world_pos(x, y + 1, z);
        if !self.chunk.is_inside(pos) {
            return false;
        }
        pos.y
            < self
                .world
                .get_height(HeightmapKind::OceanFloorWg, pos.x, pos.z)
    }

    /// `canBeReplaced(level, x, y, z, chunkBB)`.
    fn can_be_replaced(&self, x: i32, y: i32, z: i32) -> bool {
        if self.no_replace.is_empty() {
            return true;
        }
        let pos = self.world_pos(x, y, z);
        let st = self.world.get_block_state(pos);
        let p = block_path(self.world.blocks(), st);
        !self.no_replace.contains(&p)
    }

    // ----- placement -----

    /// `placeBlock(level, state, x, y, z, chunkBB)`.
    pub fn place_block(&mut self, state: u32, x: i32, y: i32, z: i32) {
        let pos = self.world_pos(x, y, z);
        if !self.chunk.is_inside(pos) {
            return;
        }
        if !self.can_be_replaced(x, y, z) {
            return;
        }
        let state = self.xform.apply(self.world.blocks(), state);
        self.world.set_block_state(pos, state, 2);
        let placed = self.world.get_block_state(pos);
        if !self.world.facts().state(placed).fluid.is_empty() {
            self.world.schedule_fluid_tick(pos, 0);
        }
        if is_shape_check_block(block_path(self.world.blocks(), state)) {
            self.world.mark_pos_for_post_processing(pos);
        }
    }

    /// `level.setBlock(pos, state, 2)` bypassing the piece transform (raw world position).
    pub fn set_raw(&mut self, pos: BlockPos, state: u32) {
        self.world.set_block_state(pos, state, 2);
    }

    /// `generateAirBox`.
    #[allow(clippy::too_many_arguments)]
    pub fn air_box(&mut self, x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32) {
        let air = self.air();
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    self.place_block(air, x, y, z);
                }
            }
        }
    }

    /// `generateBox(level, chunkBB, x0, y0, z0, x1, y1, z1, edge, fill, skipAir)`.
    #[allow(clippy::too_many_arguments)]
    pub fn gen_box(
        &mut self,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        edge: u32,
        fill: u32,
        skip_air: bool,
    ) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if skip_air && self.is_air(self.get_block(x, y, z)) {
                        continue;
                    }
                    let is_edge = y == y0 || y == y1 || x == x0 || x == x1 || z == z0 || z == z1;
                    self.place_block(if is_edge { edge } else { fill }, x, y, z);
                }
            }
        }
    }

    /// Solid fill: `generateBox(..., block, block, false)`.
    #[allow(clippy::too_many_arguments)]
    pub fn fill(&mut self, x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32, block: u32) {
        self.gen_box(x0, y0, z0, x1, y1, z1, block, block, false);
    }

    /// Box fill with a selector (`generateBox(..., skipAir, random, selector)`).
    #[allow(clippy::too_many_arguments)]
    pub fn gen_box_sel(
        &mut self,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        skip_air: bool,
        random: &mut dyn WorldgenRandom,
        selector: &mut Selector<'_>,
    ) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if skip_air && self.is_air(self.get_block(x, y, z)) {
                        continue;
                    }
                    let is_edge = y == y0 || y == y1 || x == x0 || x == x1 || z == z0 || z == z1;
                    let st = selector(random, x, y, z, is_edge);
                    self.place_block(st, x, y, z);
                }
            }
        }
    }

    /// `generateMaybeBox`.
    #[allow(clippy::too_many_arguments)]
    pub fn maybe_box(
        &mut self,
        random: &mut dyn WorldgenRandom,
        prob: f32,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        edge: u32,
        fill: u32,
        skip_air: bool,
        has_to_be_inside: bool,
    ) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if random.next_float() > prob {
                        continue;
                    }
                    if skip_air && self.is_air(self.get_block(x, y, z)) {
                        continue;
                    }
                    if has_to_be_inside && !self.is_interior(x, y, z) {
                        continue;
                    }
                    let is_edge = y == y0 || y == y1 || x == x0 || x == x1 || z == z0 || z == z1;
                    self.place_block(if is_edge { edge } else { fill }, x, y, z);
                }
            }
        }
    }

    /// `maybeGenerateBlock`: always consumes one `nextFloat()`.
    pub fn maybe_block(
        &mut self,
        random: &mut dyn WorldgenRandom,
        prob: f32,
        x: i32,
        y: i32,
        z: i32,
        state: u32,
    ) {
        if random.next_float() < prob {
            self.place_block(state, x, y, z);
        }
    }

    /// `generateUpperHalfSphere`.
    #[allow(clippy::too_many_arguments)]
    pub fn upper_half_sphere(
        &mut self,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        fill: u32,
        skip_air: bool,
    ) {
        let dx = (x1 - x0 + 1) as f32;
        let dy = (y1 - y0 + 1) as f32;
        let dz = (z1 - z0 + 1) as f32;
        let cx = x0 as f32 + dx / 2.0;
        let cz = z0 as f32 + dz / 2.0;
        for y in y0..=y1 {
            let ny = (y - y0) as f32 / dy;
            for x in x0..=x1 {
                let nx = (x as f32 - cx) / (dx * 0.5);
                for z in z0..=z1 {
                    let nz = (z as f32 - cz) / (dz * 0.5);
                    if !skip_air || !self.is_air(self.get_block(x, y, z)) {
                        let d = (nx * nx + ny * ny) + nz * nz;
                        if d <= 1.05 {
                            self.place_block(fill, x, y, z);
                        }
                    }
                }
            }
        }
    }

    /// `isReplaceableByStructures(state)`.
    pub fn replaceable_by_structures(&self, state: u32) -> bool {
        self.is_air(state)
            || self.is_liquid(state)
            || self.is(state, "glow_lichen")
            || self.is(state, "seagrass")
            || self.is(state, "tall_seagrass")
    }

    /// `fillColumnDown(level, state, x, startY, z, chunkBB)` (bypasses mirror/rotation).
    pub fn fill_column_down(&mut self, state: u32, x: i32, start_y: i32, z: i32) {
        let mut pos = self.world_pos(x, start_y, z);
        if !self.chunk.is_inside(pos) {
            return;
        }
        let floor = self.world.min_y() + 1;
        while self.replaceable_by_structures(self.world.get_block_state(pos)) && pos.y > floor {
            self.world.set_block_state(pos, state, 2);
            pos.y -= 1;
        }
    }

    /// `reorient(level, pos, state)`.
    pub fn reorient(&self, pos: BlockPos, state: u32) -> u32 {
        let mut solid_neighbor: Option<Direction> = None;
        for dir in [
            Direction::North,
            Direction::East,
            Direction::South,
            Direction::West,
        ] {
            let n = self.world.get_block_state(pos.relative(dir, 1));
            if self.is(n, "chest") {
                return state;
            }
            if self.is_solid_render(n) {
                if solid_neighbor.is_some() {
                    solid_neighbor = None;
                    break;
                }
                solid_neighbor = Some(dir);
            }
        }
        if let Some(d) = solid_neighbor {
            return self.with(state, "facing", d.opposite().name());
        }
        let mut lock = self
            .prop(state, "facing")
            .and_then(|f| Direction::from_name(&f))
            .unwrap_or(Direction::North);
        let solid_at = |me: &Self, d: Direction| {
            me.is_solid_render(me.world.get_block_state(pos.relative(d, 1)))
        };
        if solid_at(self, lock) {
            lock = lock.opposite();
            if solid_at(self, lock) {
                lock = lock.clockwise();
                if solid_at(self, lock) {
                    lock = lock.opposite();
                }
            }
        }
        self.with(state, "facing", lock.name())
    }

    /// `createChest(level, chunkBB, random, pos, lootTable, state)`.
    pub fn create_chest_at(
        &mut self,
        random: &mut dyn WorldgenRandom,
        pos: BlockPos,
        loot_table: &str,
        state: Option<u32>,
    ) -> bool {
        if !self.chunk.is_inside(pos) || self.is(self.world.get_block_state(pos), "chest") {
            return false;
        }
        let st = match state {
            Some(s) => s,
            None => {
                let c = self.d("chest");
                self.reorient(pos, c)
            }
        };
        self.world.set_block_state(pos, st, 2);
        set_loot(self.world, pos, loot_table, random.next_long());
        true
    }

    /// Local-coordinate `createChest`.
    pub fn create_chest(
        &mut self,
        random: &mut dyn WorldgenRandom,
        x: i32,
        y: i32,
        z: i32,
        loot_table: &str,
    ) -> bool {
        let pos = self.world_pos(x, y, z);
        self.create_chest_at(random, pos, loot_table, None)
    }

    /// `createDispenser`.
    #[allow(clippy::too_many_arguments)]
    pub fn create_dispenser(
        &mut self,
        random: &mut dyn WorldgenRandom,
        x: i32,
        y: i32,
        z: i32,
        facing: Direction,
        loot_table: &str,
    ) -> bool {
        let pos = self.world_pos(x, y, z);
        if self.chunk.is_inside(pos) && !self.is(self.world.get_block_state(pos), "dispenser") {
            let st = self.s("dispenser", &[("facing", facing.name())]);
            self.place_block(st, x, y, z);
            set_loot(self.world, pos, loot_table, random.next_long());
            true
        } else {
            false
        }
    }

    /// Spawner at local coordinates with `SpawnData.entity.id = entity`.
    pub fn place_spawner(&mut self, x: i32, y: i32, z: i32, entity: &str) {
        let pos = self.world_pos(x, y, z);
        if !self.chunk.is_inside(pos) {
            return;
        }
        let st = self.d("spawner");
        self.place_block(st, x, y, z);
        set_spawner(self.world, pos, entity);
    }
}

/// Writes `LootTable` / `LootTableSeed` block-entity data.
pub fn set_loot(world: &mut dyn StructureWorld, pos: BlockPos, loot_table: &str, seed: i64) {
    let mut nbt = Compound::new();
    nbt.insert("LootTable", Tag::from(loot_table));
    nbt.insert("LootTableSeed", Tag::Long(seed));
    world.set_block_entity(pos, nbt);
}

/// Writes spawner block-entity data (`SpawnData.entity.id`).
pub fn set_spawner(world: &mut dyn StructureWorld, pos: BlockPos, entity: &str) {
    let mut ent = Compound::new();
    ent.insert("id", Tag::from(entity));
    let mut sd = Compound::new();
    sd.insert("entity", Tag::Compound(ent));
    let mut nbt = Compound::new();
    nbt.insert("SpawnData", Tag::Compound(sd));
    world.set_block_entity(pos, nbt);
}

/// Adds a freshly spawned entity (`id`, centred `Pos`, optional persistence flag).
pub fn spawn_entity(world: &mut dyn StructureWorld, id: &str, pos: [f64; 3], persistent: bool) {
    let mut nbt = Compound::new();
    nbt.insert("id", Tag::from(id));
    let mut p = zenith_nbt::List::new();
    for v in pos {
        p.push(Tag::Double(v));
    }
    nbt.insert("Pos", Tag::List(p));
    if persistent {
        nbt.insert("PersistenceRequired", Tag::Byte(1));
    }
    world.add_entity(nbt);
}

/// Unit step of a direction as `(dx, dz)`.
pub fn step_xz(d: Direction) -> (i32, i32) {
    let (dx, _, dz) = dir_step(d);
    (dx, dz)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orientation_table_matches_spec() {
        assert_eq!(orientation_transform(None), (Mirror::None, Rotation::None));
        assert_eq!(
            orientation_transform(Some(Direction::North)),
            (Mirror::None, Rotation::None)
        );
        assert_eq!(
            orientation_transform(Some(Direction::South)),
            (Mirror::LeftRight, Rotation::None)
        );
        assert_eq!(
            orientation_transform(Some(Direction::West)),
            (Mirror::LeftRight, Rotation::Clockwise90)
        );
        assert_eq!(
            orientation_transform(Some(Direction::East)),
            (Mirror::None, Rotation::Clockwise90)
        );
    }

    #[test]
    fn world_pos_follows_orientation() {
        let bb = BoundingBox::from_coords(10, 5, 20, 14, 9, 26);
        assert_eq!(piece_world_pos(bb, None, 1, 2, 3), BlockPos::new(1, 2, 3));
        assert_eq!(
            piece_world_pos(bb, Some(Direction::North), 1, 2, 3),
            BlockPos::new(11, 7, 23)
        );
        assert_eq!(
            piece_world_pos(bb, Some(Direction::South), 1, 2, 3),
            BlockPos::new(11, 7, 23)
        );
        assert_eq!(
            piece_world_pos(bb, Some(Direction::West), 1, 2, 3),
            BlockPos::new(14 - 3, 7, 20 + 1)
        );
        assert_eq!(
            piece_world_pos(bb, Some(Direction::East), 1, 2, 3),
            BlockPos::new(10 + 3, 7, 20 + 1)
        );
    }
}
