//! Mineshaft piece placement (`docs/specs/worldgen/structures-legacy-pieces.md` §2.4–§2.5).

use zenith_blocks::SupportType;
use zenith_nbt::{Compound, Tag};
use zenith_registry::Identifier;

use crate::data::MineshaftType;
use crate::generator::StructurePiece;
use crate::geom::{BlockPos, BoundingBox, BoundingBoxExt, Direction};
use crate::piece::{PieceBuilder, set_spawner};
use crate::piece_data::PieceData;
use crate::place::PieceCall;
use crate::world::WorldgenRandom;

/// Kind-specific data of a mineshaft piece.
#[derive(Debug, Clone, PartialEq)]
pub enum MineshaftKind {
    /// `msroom`: attached corridor entrance boxes (world space).
    Room {
        /// Entrance boxes (`childEntranceBoxes`).
        entrance_boxes: Vec<BoundingBox>,
    },
    /// `mscorridor`.
    Corridor {
        /// `hasRails`.
        has_rails: bool,
        /// `spiderCorridor`.
        spider_corridor: bool,
        /// `hasPlacedSpider`.
        has_placed_spider: bool,
        /// `numSections`.
        num_sections: i32,
    },
    /// `mscrossing`.
    Crossing {
        /// `isTwoFloored`.
        is_two_floored: bool,
    },
    /// `msstairs`.
    Stairs,
}

/// Mineshaft piece data.
#[derive(Debug, Clone, PartialEq)]
pub struct MineshaftPiece {
    /// Normal or mesa timber.
    pub mtype: MineshaftType,
    /// Piece kind.
    pub kind: MineshaftKind,
}

fn timber(t: MineshaftType) -> (&'static str, &'static str, &'static str) {
    match t {
        MineshaftType::Normal => ("oak_planks", "oak_log", "oak_fence"),
        MineshaftType::Mesa => ("dark_oak_planks", "dark_oak_log", "dark_oak_fence"),
    }
}

fn is_falling_block(path: &str) -> bool {
    matches!(
        path,
        "sand"
            | "red_sand"
            | "gravel"
            | "suspicious_sand"
            | "suspicious_gravel"
            | "anvil"
            | "chipped_anvil"
            | "damaged_anvil"
            | "dragon_egg"
            | "pointed_dripstone"
    ) || path.ends_with("_concrete_powder")
}

/// `MineShaftPiece.isInInvalidLocation`.
fn is_in_invalid_location(call: &PieceCall<'_>, bb: BoundingBox, chunk: BoundingBox) -> bool {
    let x0 = (bb.min_x - 1).max(chunk.min_x);
    let y0 = (bb.min_y - 1).max(chunk.min_y);
    let z0 = (bb.min_z - 1).max(chunk.min_z);
    let x1 = (bb.max_x + 1).min(chunk.max_x);
    let y1 = (bb.max_y + 1).min(chunk.max_y);
    let z1 = (bb.max_z + 1).min(chunk.max_z);
    let mid = BlockPos::new((x0 + x1) / 2, (y0 + y1) / 2, (z0 + z1) / 2);
    if let Some(b) = call.world.biome_at(mid) {
        let tag = Identifier::with_default_namespace("mineshaft_blocking").expect("valid");
        if call.env.data.biome_in_tag(&tag, &b) {
            return true;
        }
    }
    let liquid = |x: i32, y: i32, z: i32| {
        let st = call.world.get_block_state(BlockPos::new(x, y, z));
        call.world
            .facts()
            .state(st)
            .has(zenith_blocks::flag::LIQUID)
    };
    for x in x0..=x1 {
        for z in z0..=z1 {
            if liquid(x, y0, z) || liquid(x, y1, z) {
                return true;
            }
        }
    }
    for x in x0..=x1 {
        for y in y0..=y1 {
            if liquid(x, y, z0) || liquid(x, y, z1) {
                return true;
            }
        }
    }
    for z in z0..=z1 {
        for y in y0..=y1 {
            if liquid(x0, y, z) || liquid(x1, y, z) {
                return true;
            }
        }
    }
    false
}

/// `MineShaftPiece.setPlanksBlock`.
fn set_planks_block(b: &mut PieceBuilder<'_>, planks: u32, x: i32, y: i32, z: i32) {
    if b.is_interior(x, y, z) {
        let pos = b.world_pos(x, y, z);
        let st = b.world.get_block_state(pos);
        if !b
            .world
            .facts()
            .state(st)
            .is_face_sturdy(Direction::Up, SupportType::Full)
        {
            b.world.set_block_state(pos, planks, 2);
        }
    }
}

/// `MineShaftPiece.isSupportingBox(level, chunkBB, x0, x1, y1, z0)`.
fn is_supporting_box(b: &PieceBuilder<'_>, x0: i32, x1: i32, y1: i32, z0: i32) -> bool {
    (x0..=x1).all(|x| !b.is_air(b.get_block(x, y1 + 1, z0)))
}

fn has_sturdy_neighbours(b: &PieceBuilder<'_>, x: i32, y: i32, z: i32, min: i32) -> bool {
    let pos = b.world_pos(x, y, z);
    let mut count = 0;
    for d in [
        Direction::Down,
        Direction::Up,
        Direction::North,
        Direction::South,
        Direction::West,
        Direction::East,
    ] {
        let n = pos.relative(d, 1);
        if b.chunk.is_inside(n) {
            let st = b.world.get_block_state(n);
            if b.world
                .facts()
                .state(st)
                .is_face_sturdy(d.opposite(), SupportType::Full)
            {
                count += 1;
                if count >= min {
                    return true;
                }
            }
        }
    }
    false
}

#[allow(clippy::too_many_arguments)]
fn maybe_cobweb(
    b: &mut PieceBuilder<'_>,
    rng: &mut dyn WorldgenRandom,
    prob: f32,
    web: u32,
    x: i32,
    y: i32,
    z: i32,
) {
    if b.is_interior(x, y, z) && rng.next_float() < prob && has_sturdy_neighbours(b, x, y, z, 2) {
        b.place_block(web, x, y, z);
    }
}

fn fill_pillar_down_or_chain_up(
    b: &mut PieceBuilder<'_>,
    wood: u32,
    fence: u32,
    chain: u32,
    px: i32,
    y: i32,
    z: i32,
) {
    let pos = b.world_pos(px, y, z);
    if !b.chunk.is_inside(pos) {
        return;
    }
    let world_y = pos.y;
    let (mut check_below, mut check_above) = (true, true);
    let mut dist = 1;
    let min_y = b.world.min_y();
    let max_y = b.world.max_y();
    while check_below || check_above {
        if check_below {
            let p = BlockPos::new(pos.x, world_y - dist, pos.z);
            let below = b.world.get_block_state(p);
            let empty_below = b.replaceable_by_structures(below) && !b.is(below, "lava");
            if !empty_below
                && b.world
                    .facts()
                    .state(below)
                    .is_face_sturdy(Direction::Up, SupportType::Full)
            {
                for yy in (world_y - dist + 1)..=(world_y - 1) {
                    b.world
                        .set_block_state(BlockPos::new(pos.x, yy, pos.z), wood, 2);
                }
                return;
            }
            check_below = dist <= 20 && empty_below && world_y - dist > min_y + 1;
        }
        if check_above {
            let p = BlockPos::new(pos.x, world_y + dist, pos.z);
            let above = b.world.get_block_state(p);
            let empty_above = b.replaceable_by_structures(above);
            let path = b.path_of(above);
            if !empty_above
                && b.world
                    .facts()
                    .state(above)
                    .is_face_sturdy(Direction::Down, SupportType::Center)
                && !is_falling_block(&path)
            {
                b.world
                    .set_block_state(BlockPos::new(pos.x, world_y + 1, pos.z), fence, 2);
                for yy in (world_y + 2)..=(world_y + dist - 1) {
                    b.world
                        .set_block_state(BlockPos::new(pos.x, yy, pos.z), chain, 2);
                }
                return;
            }
            check_above = dist <= 50 && empty_above && world_y + dist < max_y;
        }
        dist += 1;
    }
}

#[allow(clippy::too_many_arguments)]
fn place_double_support(
    b: &mut PieceBuilder<'_>,
    planks: u32,
    wood: u32,
    fence: u32,
    chain: u32,
    x: i32,
    y: i32,
    z: i32,
) {
    for px in [x, x + 2] {
        if b.get_block(px, y, z) == planks || b.path_of(b.get_block(px, y, z)) == b.path_of(planks)
        {
            fill_pillar_down_or_chain_up(b, wood, fence, chain, px, y, z);
        }
    }
}

/// `postProcess` of every mineshaft piece.
pub fn post_process(piece: &mut StructurePiece, call: &mut PieceCall<'_>) {
    let bb = piece.bounding_box;
    let chunk = *call.chunk_bb;
    if is_in_invalid_location(call, bb, chunk) {
        return;
    }
    let PieceData::Mineshaft(data) = &mut piece.data else {
        return;
    };
    let (pl, wo, fe) = timber(data.mtype);
    let mut b = PieceBuilder::new(&mut *call.world, bb, piece.orientation, chunk);
    b.no_replace = vec![pl, wo, fe, "iron_chain"];
    let rng = &mut *call.random;
    let planks = b.d(pl);
    let wood = b.d(wo);
    let fence = b.d(fe);
    let chain = b.d("iron_chain");
    let cave_air = b.d("cave_air");
    match &mut data.kind {
        MineshaftKind::Room { entrance_boxes } => {
            b.gen_box(
                bb.min_x,
                bb.min_y + 1,
                bb.min_z,
                bb.max_x,
                (bb.min_y + 3).min(bb.max_y),
                bb.max_z,
                cave_air,
                cave_air,
                false,
            );
            for eb in entrance_boxes.iter() {
                b.gen_box(
                    eb.min_x,
                    eb.max_y - 2,
                    eb.min_z,
                    eb.max_x,
                    eb.max_y,
                    eb.max_z,
                    cave_air,
                    cave_air,
                    false,
                );
            }
            b.upper_half_sphere(
                bb.min_x,
                bb.min_y + 4,
                bb.min_z,
                bb.max_x,
                bb.max_y,
                bb.max_z,
                cave_air,
                false,
            );
        }
        MineshaftKind::Corridor {
            has_rails,
            spider_corridor,
            has_placed_spider,
            num_sections,
        } => {
            let length = *num_sections * 5 - 1;
            let web = b.d("cobweb");
            b.gen_box(0, 0, 0, 2, 1, length, cave_air, cave_air, false);
            b.maybe_box(
                rng, 0.8, 0, 2, 0, 2, 2, length, cave_air, cave_air, false, false,
            );
            if *spider_corridor {
                b.maybe_box(rng, 0.6, 0, 0, 0, 2, 1, length, web, cave_air, false, true);
            }
            let fence_w = b.with(fence, "west", "true");
            let fence_e = b.with(fence, "east", "true");
            let torch_s = b.s("wall_torch", &[("facing", "south")]);
            let torch_n = b.s("wall_torch", &[("facing", "north")]);
            for section in 0..*num_sections {
                let z = 2 + section * 5;
                if is_supporting_box(&b, 0, 2, 2, z) {
                    b.gen_box(0, 0, z, 0, 1, z, fence_w, cave_air, false);
                    b.gen_box(2, 0, z, 2, 1, z, fence_e, cave_air, false);
                    if rng.next_int_bounded(4) == 0 {
                        b.place_block(planks, 0, 2, z);
                        b.place_block(planks, 2, 2, z);
                    } else {
                        b.gen_box(0, 2, z, 2, 2, z, planks, cave_air, false);
                        b.maybe_block(rng, 0.05, 1, 2, z - 1, torch_s);
                        b.maybe_block(rng, 0.05, 1, 2, z + 1, torch_n);
                    }
                }
                maybe_cobweb(&mut b, rng, 0.1, web, 0, 2, z - 1);
                maybe_cobweb(&mut b, rng, 0.1, web, 2, 2, z - 1);
                maybe_cobweb(&mut b, rng, 0.1, web, 0, 2, z + 1);
                maybe_cobweb(&mut b, rng, 0.1, web, 2, 2, z + 1);
                maybe_cobweb(&mut b, rng, 0.05, web, 0, 2, z - 2);
                maybe_cobweb(&mut b, rng, 0.05, web, 2, 2, z - 2);
                maybe_cobweb(&mut b, rng, 0.05, web, 0, 2, z + 2);
                maybe_cobweb(&mut b, rng, 0.05, web, 2, 2, z + 2);
                if rng.next_int_bounded(100) == 0 {
                    create_minecart_chest(&mut b, rng, 2, 0, z - 1);
                }
                if rng.next_int_bounded(100) == 0 {
                    create_minecart_chest(&mut b, rng, 0, 0, z + 1);
                }
                if *spider_corridor && !*has_placed_spider {
                    let new_z = z - 1 + rng.next_int_bounded(3);
                    let pos = b.world_pos(1, 0, new_z);
                    if chunk.is_inside(pos) && b.is_interior(1, 0, new_z) {
                        *has_placed_spider = true;
                        let sp = b.d("spawner");
                        b.world.set_block_state(pos, sp, 2);
                        set_spawner(b.world, pos, "minecraft:cave_spider");
                    }
                }
            }
            for x in 0..=2 {
                for z in 0..=length {
                    set_planks_block(&mut b, planks, x, -1, z);
                }
            }
            place_double_support(&mut b, planks, wood, fence, chain, 0, -1, 2);
            if *num_sections > 1 {
                place_double_support(&mut b, planks, wood, fence, chain, 0, -1, length - 2);
            }
            if *has_rails {
                let rail = b.s("rail", &[("shape", "north_south")]);
                for z in 0..=length {
                    let floor = b.get_block(1, -1, z);
                    if !b.is_air(floor) && b.is_solid_render(floor) {
                        let prob = if b.is_interior(1, 0, z) { 0.7 } else { 0.9 };
                        b.maybe_block(rng, prob, 1, 0, z, rail);
                    }
                }
            }
        }
        MineshaftKind::Crossing { is_two_floored } => {
            let (x0, y0, z0) = (bb.min_x, bb.min_y, bb.min_z);
            let (x1, y1, z1) = (bb.max_x, bb.max_y, bb.max_z);
            if *is_two_floored {
                b.gen_box(
                    x0 + 1,
                    y0,
                    z0,
                    x1 - 1,
                    y0 + 2,
                    z1,
                    cave_air,
                    cave_air,
                    false,
                );
                b.gen_box(
                    x0,
                    y0,
                    z0 + 1,
                    x1,
                    y0 + 2,
                    z1 - 1,
                    cave_air,
                    cave_air,
                    false,
                );
                b.gen_box(
                    x0 + 1,
                    y1 - 2,
                    z0,
                    x1 - 1,
                    y1,
                    z1,
                    cave_air,
                    cave_air,
                    false,
                );
                b.gen_box(
                    x0,
                    y1 - 2,
                    z0 + 1,
                    x1,
                    y1,
                    z1 - 1,
                    cave_air,
                    cave_air,
                    false,
                );
                b.gen_box(
                    x0 + 1,
                    y0 + 3,
                    z0 + 1,
                    x1 - 1,
                    y0 + 3,
                    z1 - 1,
                    cave_air,
                    cave_air,
                    false,
                );
            } else {
                b.gen_box(x0 + 1, y0, z0, x1 - 1, y1, z1, cave_air, cave_air, false);
                b.gen_box(x0, y0, z0 + 1, x1, y1, z1 - 1, cave_air, cave_air, false);
            }
            for (px, pz) in [
                (x0 + 1, z0 + 1),
                (x0 + 1, z1 - 1),
                (x1 - 1, z0 + 1),
                (x1 - 1, z1 - 1),
            ] {
                if !b.is_air(b.get_block(px, y1 + 1, pz)) {
                    b.gen_box(px, y0, pz, px, y1, pz, planks, planks, false);
                }
            }
            for x in x0..=x1 {
                for z in z0..=z1 {
                    set_planks_block(&mut b, planks, x, y0 - 1, z);
                }
            }
        }
        MineshaftKind::Stairs => {
            b.gen_box(0, 5, 0, 2, 7, 1, cave_air, cave_air, false);
            b.gen_box(0, 0, 7, 2, 2, 8, cave_air, cave_air, false);
            for i in 0..5 {
                b.gen_box(
                    0,
                    5 - i - if i < 4 { 1 } else { 0 },
                    2 + i,
                    2,
                    7 - i,
                    2 + i,
                    cave_air,
                    cave_air,
                    false,
                );
            }
        }
    }
}

/// `MineShaftCorridor.createChest`: a chest minecart on a rail.
fn create_minecart_chest(
    b: &mut PieceBuilder<'_>,
    rng: &mut dyn WorldgenRandom,
    x: i32,
    y: i32,
    z: i32,
) -> bool {
    let pos = b.world_pos(x, y, z);
    if b.chunk.is_inside(pos)
        && b.is_air(b.world.get_block_state(pos))
        && !b.is_air(b.world.get_block_state(pos.below(1)))
    {
        let shape = if rng.next_boolean() {
            "north_south"
        } else {
            "east_west"
        };
        let rail = b.s("rail", &[("shape", shape)]);
        b.place_block(rail, x, y, z);
        let mut nbt = Compound::new();
        nbt.insert("id", Tag::from("minecraft:chest_minecart"));
        let mut p = zenith_nbt::List::new();
        for v in [
            f64::from(pos.x) + 0.5,
            f64::from(pos.y) + 0.5,
            f64::from(pos.z) + 0.5,
        ] {
            p.push(Tag::Double(v));
        }
        nbt.insert("Pos", Tag::List(p));
        nbt.insert(
            "LootTable",
            Tag::from("minecraft:chests/abandoned_mineshaft"),
        );
        nbt.insert("LootTableSeed", Tag::Long(rng.next_long()));
        b.world.add_entity(nbt);
        true
    } else {
        false
    }
}
