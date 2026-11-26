//! Stronghold piece placement (`docs/specs/worldgen/structures-legacy-pieces.md` §3.4–§3.5).

use crate::generator::StructurePiece;
use crate::geom::{BoundingBoxExt, Direction};
use crate::piece::PieceBuilder;
use crate::piece_data::PieceData;
use crate::place::PieceCall;
use crate::world::WorldgenRandom;

/// Small door type (`StrongholdPiece.SmallDoorType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SmallDoor {
    /// Open doorway.
    #[default]
    Opening,
    /// Oak door.
    WoodDoor,
    /// Iron bars.
    Grates,
    /// Iron door with buttons.
    IronDoor,
}

/// Stronghold piece kinds and their per-piece flags.
#[derive(Debug, Clone, PartialEq)]
pub enum ShKind {
    /// `shstart`.
    Start,
    /// `shsd`.
    StairsDown,
    /// `shs`.
    Straight { left_child: bool, right_child: bool },
    /// `shcc`.
    ChestCorridor { has_placed_chest: bool },
    /// `shfc`.
    FillerCorridor,
    /// `shlt`.
    LeftTurn,
    /// `shrt`.
    RightTurn,
    /// `shph`.
    PrisonHall,
    /// `shrc`.
    RoomCrossing { room_type: i32 },
    /// `shssd`.
    StraightStairsDown,
    /// `sh5c`.
    FiveCrossing {
        left_low: bool,
        left_high: bool,
        right_low: bool,
        right_high: bool,
    },
    /// `shli`.
    Library,
    /// `shpr`.
    PortalRoom { has_placed_spawner: bool },
}

/// Stronghold piece data.
#[derive(Debug, Clone, PartialEq)]
pub struct StrongholdPiece {
    /// Kind and flags.
    pub kind: ShKind,
    /// Entry door type.
    pub door: SmallDoor,
}

/// `SmoothStoneSelector`.
fn smooth_stone_selector<'a>(
    cracked: u32,
    mossy: u32,
    infested: u32,
    bricks: u32,
    air: u32,
) -> impl FnMut(&mut dyn WorldgenRandom, i32, i32, i32, bool) -> u32 + 'a {
    move |r: &mut dyn WorldgenRandom, _x, _y, _z, edge| {
        if !edge {
            return air;
        }
        let f = r.next_float();
        if f < 0.2 {
            cracked
        } else if f < 0.5 {
            mossy
        } else if f < 0.55 {
            infested
        } else {
            bricks
        }
    }
}

/// `generateSmallDoor`.
fn small_door(b: &mut PieceBuilder<'_>, door: SmallDoor, fx: i32, fy: i32, fz: i32) {
    let air = b.d("cave_air");
    let sb = b.d("stone_bricks");
    match door {
        SmallDoor::Opening => b.gen_box(fx, fy, fz, fx + 2, fy + 2, fz, air, air, false),
        SmallDoor::WoodDoor | SmallDoor::IronDoor => {
            b.gen_box(fx, fy, fz, fx, fy + 2, fz, sb, air, false);
            b.place_block(sb, fx + 1, fy + 2, fz);
            b.gen_box(fx + 2, fy, fz, fx + 2, fy + 2, fz, sb, air, false);
            let name = if door == SmallDoor::WoodDoor {
                "oak_door"
            } else {
                "iron_door"
            };
            let lo = b.s(name, &[("facing", "north"), ("half", "lower")]);
            let up = b.s(name, &[("facing", "north"), ("half", "upper")]);
            b.place_block(lo, fx + 1, fy, fz);
            b.place_block(up, fx + 1, fy + 1, fz);
            if door == SmallDoor::IronDoor {
                let bn = b.s("stone_button", &[("facing", "north")]);
                let bs = b.s("stone_button", &[("facing", "south")]);
                b.place_block(bn, fx + 2, fy + 1, fz + 1);
                b.place_block(bs, fx + 2, fy + 1, fz - 1);
            }
        }
        SmallDoor::Grates => {
            b.place_block(air, fx + 1, fy, fz);
            b.place_block(air, fx + 1, fy + 1, fz);
            let w = b.s("iron_bars", &[("west", "true")]);
            let ew = b.s("iron_bars", &[("east", "true"), ("west", "true")]);
            let e = b.s("iron_bars", &[("east", "true")]);
            b.place_block(w, fx, fy, fz);
            b.place_block(w, fx, fy + 1, fz);
            b.place_block(ew, fx, fy + 2, fz);
            b.place_block(ew, fx + 1, fy + 2, fz);
            b.place_block(ew, fx + 2, fy + 2, fz);
            // The east column is placed top-down (the order shows in the post-processing marks).
            b.place_block(e, fx + 2, fy + 1, fz);
            b.place_block(e, fx + 2, fy, fz);
        }
    }
}

/// `postProcess` of every stronghold piece.
pub fn post_process(piece: &mut StructurePiece, call: &mut PieceCall<'_>) {
    let bb = piece.bounding_box;
    let orient = piece.orientation;
    let chunk = *call.chunk_bb;
    let PieceData::Stronghold(data) = &mut piece.data else {
        return;
    };
    let mut b = PieceBuilder::new(&mut *call.world, bb, orient, chunk);
    let rng = &mut *call.random;
    let air = b.d("cave_air");
    let cracked = b.d("cracked_stone_bricks");
    let mossy = b.d("mossy_stone_bricks");
    let infested = b.d("infested_stone_bricks");
    let sb = b.d("stone_bricks");
    let mut sel = smooth_stone_selector(cracked, mossy, infested, sb, air);
    let door = data.door;
    let torch = |b: &PieceBuilder<'_>, f: &str| b.s("wall_torch", &[("facing", f)]);
    let slab = b.d("smooth_stone_slab");
    let sbslab = b.d("stone_brick_slab");
    let cobble = b.d("cobblestone");
    let planks = b.d("oak_planks");
    let north_horizontal = matches!(orient, Some(Direction::North | Direction::East));
    match &mut data.kind {
        ShKind::Start | ShKind::StairsDown => {
            let is_start = matches!(data.kind, ShKind::Start);
            let _ = is_start;
            b.gen_box_sel(0, 0, 0, 4, 10, 4, true, rng, &mut sel);
            small_door(&mut b, door, 1, 7, 0);
            small_door(&mut b, SmallDoor::Opening, 1, 1, 4);
            for (x, y, z) in [
                (2, 6, 1),
                (1, 5, 1),
                (1, 5, 2),
                (1, 4, 3),
                (2, 4, 3),
                (3, 3, 3),
                (3, 3, 2),
                (3, 2, 1),
                (2, 2, 1),
                (1, 1, 1),
                (1, 1, 2),
            ] {
                b.place_block(sb, x, y, z);
            }
            for (x, y, z) in [
                (1, 6, 1),
                (1, 5, 3),
                (3, 4, 3),
                (3, 3, 1),
                (1, 2, 1),
                (1, 1, 3),
            ] {
                b.place_block(slab, x, y, z);
            }
        }
        ShKind::Straight {
            left_child,
            right_child,
        } => {
            b.gen_box_sel(0, 0, 0, 4, 4, 6, true, rng, &mut sel);
            small_door(&mut b, door, 1, 1, 0);
            small_door(&mut b, SmallDoor::Opening, 1, 1, 6);
            let te = torch(&b, "east");
            let tw = torch(&b, "west");
            b.maybe_block(rng, 0.1, 1, 2, 1, te);
            b.maybe_block(rng, 0.1, 3, 2, 1, tw);
            b.maybe_block(rng, 0.1, 1, 2, 5, te);
            b.maybe_block(rng, 0.1, 3, 2, 5, tw);
            if *left_child {
                b.gen_box(0, 1, 2, 0, 3, 4, air, air, false);
            }
            if *right_child {
                b.gen_box(4, 1, 2, 4, 3, 4, air, air, false);
            }
        }
        ShKind::ChestCorridor { has_placed_chest } => {
            b.gen_box_sel(0, 0, 0, 4, 4, 6, true, rng, &mut sel);
            small_door(&mut b, door, 1, 1, 0);
            small_door(&mut b, SmallDoor::Opening, 1, 1, 6);
            b.gen_box(3, 1, 2, 3, 1, 4, sb, sb, false);
            for (x, y, z) in [(3, 1, 1), (3, 1, 5), (3, 2, 2), (3, 2, 4)] {
                b.place_block(sbslab, x, y, z);
            }
            for z in 2..=4 {
                b.place_block(sbslab, 2, 1, z);
            }
            let wp = b.world_pos(3, 2, 3);
            if !*has_placed_chest && chunk.is_inside(wp) {
                *has_placed_chest =
                    b.create_chest(rng, 3, 2, 3, "minecraft:chests/stronghold_corridor");
            }
        }
        ShKind::FillerCorridor => {
            let steps = if matches!(orient, Some(Direction::North | Direction::South)) {
                bb.z_span()
            } else {
                bb.x_span()
            };
            for i in 0..steps {
                for x in 0..=4 {
                    b.place_block(sb, x, 0, i);
                    b.place_block(sb, x, 4, i);
                }
                for y in 1..=3 {
                    b.place_block(sb, 0, y, i);
                    b.place_block(sb, 4, y, i);
                    for x in 1..=3 {
                        b.place_block(air, x, y, i);
                    }
                }
            }
        }
        ShKind::LeftTurn | ShKind::RightTurn => {
            let left = matches!(data.kind, ShKind::LeftTurn);
            b.gen_box_sel(0, 0, 0, 4, 4, 4, true, rng, &mut sel);
            small_door(&mut b, door, 1, 1, 0);
            let west_side = if left {
                north_horizontal
            } else {
                !north_horizontal
            };
            if west_side {
                b.gen_box(0, 1, 1, 0, 3, 3, air, air, false);
            } else {
                b.gen_box(4, 1, 1, 4, 3, 3, air, air, false);
            }
        }
        ShKind::PrisonHall => {
            b.gen_box_sel(0, 0, 0, 8, 4, 10, true, rng, &mut sel);
            small_door(&mut b, door, 1, 1, 0);
            b.gen_box(1, 1, 10, 3, 3, 10, air, air, false);
            for z in [1, 3, 7, 9] {
                b.gen_box_sel(4, 1, z, 4, 3, z, false, rng, &mut sel);
            }
            let ns = b.s("iron_bars", &[("north", "true"), ("south", "true")]);
            let nse = b.s(
                "iron_bars",
                &[("north", "true"), ("south", "true"), ("east", "true")],
            );
            let we = b.s("iron_bars", &[("west", "true"), ("east", "true")]);
            for y in 1..=3 {
                b.place_block(ns, 4, y, 4);
                b.place_block(nse, 4, y, 5);
                b.place_block(ns, 4, y, 6);
                b.place_block(we, 5, y, 5);
                b.place_block(we, 6, y, 5);
                b.place_block(we, 7, y, 5);
            }
            b.place_block(ns, 4, 3, 2);
            b.place_block(ns, 4, 3, 8);
            let lo = b.s("iron_door", &[("facing", "west"), ("half", "lower")]);
            let up = b.s("iron_door", &[("facing", "west"), ("half", "upper")]);
            b.place_block(lo, 4, 1, 2);
            b.place_block(up, 4, 2, 2);
            b.place_block(lo, 4, 1, 8);
            b.place_block(up, 4, 2, 8);
        }
        ShKind::RoomCrossing { room_type } => {
            b.gen_box_sel(0, 0, 0, 10, 6, 10, true, rng, &mut sel);
            small_door(&mut b, door, 4, 1, 0);
            b.gen_box(4, 1, 10, 6, 3, 10, air, air, false);
            b.gen_box(0, 1, 4, 0, 3, 6, air, air, false);
            b.gen_box(10, 1, 4, 10, 3, 6, air, air, false);
            match *room_type {
                0 => {
                    b.place_block(sb, 5, 1, 5);
                    b.place_block(sb, 5, 2, 5);
                    b.place_block(sb, 5, 3, 5);
                    let tw = torch(&b, "west");
                    let te = torch(&b, "east");
                    let ts = torch(&b, "south");
                    let tn = torch(&b, "north");
                    b.place_block(tw, 4, 3, 5);
                    b.place_block(te, 6, 3, 5);
                    b.place_block(ts, 5, 3, 4);
                    b.place_block(tn, 5, 3, 6);
                    for x in 4..=6 {
                        for z in 4..=6 {
                            if !(x == 5 && z == 5) {
                                b.place_block(slab, x, 1, z);
                            }
                        }
                    }
                }
                1 => {
                    for i in 0..=4 {
                        b.place_block(sb, 3, 1, 3 + i);
                        b.place_block(sb, 7, 1, 3 + i);
                        b.place_block(sb, 3 + i, 1, 3);
                        b.place_block(sb, 3 + i, 1, 7);
                    }
                    b.place_block(sb, 5, 1, 5);
                    b.place_block(sb, 5, 2, 5);
                    b.place_block(sb, 5, 3, 5);
                    let w = b.d("water");
                    b.place_block(w, 5, 4, 5);
                }
                2 => {
                    for i in 1..=9 {
                        b.place_block(cobble, 1, 3, i);
                        b.place_block(cobble, 9, 3, i);
                    }
                    for i in 1..=9 {
                        b.place_block(cobble, i, 3, 1);
                        b.place_block(cobble, i, 3, 9);
                    }
                    for (x, y, z) in [
                        (5, 1, 4),
                        (5, 1, 6),
                        (5, 3, 4),
                        (5, 3, 6),
                        (4, 1, 5),
                        (6, 1, 5),
                        (4, 3, 5),
                        (6, 3, 5),
                    ] {
                        b.place_block(cobble, x, y, z);
                    }
                    for i in 1..=3 {
                        b.place_block(cobble, 4, i, 4);
                        b.place_block(cobble, 6, i, 4);
                        b.place_block(cobble, 4, i, 6);
                        b.place_block(cobble, 6, i, 6);
                    }
                    let t = b.d("wall_torch");
                    b.place_block(t, 5, 3, 5);
                    for z in 2..=8 {
                        b.place_block(planks, 2, 3, z);
                        b.place_block(planks, 3, 3, z);
                        if z <= 3 || z >= 7 {
                            b.place_block(planks, 4, 3, z);
                            b.place_block(planks, 5, 3, z);
                            b.place_block(planks, 6, 3, z);
                        }
                        b.place_block(planks, 7, 3, z);
                        b.place_block(planks, 8, 3, z);
                    }
                    let ladder = b.s("ladder", &[("facing", "west")]);
                    b.place_block(ladder, 9, 1, 3);
                    b.place_block(ladder, 9, 2, 3);
                    b.place_block(ladder, 9, 3, 3);
                    b.create_chest(rng, 3, 4, 8, "minecraft:chests/stronghold_crossing");
                }
                _ => {}
            }
        }
        ShKind::StraightStairsDown => {
            b.gen_box_sel(0, 0, 0, 4, 10, 7, true, rng, &mut sel);
            small_door(&mut b, door, 1, 7, 0);
            small_door(&mut b, SmallDoor::Opening, 1, 1, 7);
            let stairs = b.s("cobblestone_stairs", &[("facing", "south")]);
            for i in 0..6 {
                for x in 1..=3 {
                    b.place_block(stairs, x, 6 - i, 1 + i);
                    if i < 5 {
                        b.place_block(sb, x, 5 - i, 1 + i);
                    }
                }
            }
        }
        ShKind::FiveCrossing {
            left_low,
            left_high,
            right_low,
            right_high,
        } => {
            b.gen_box_sel(0, 0, 0, 9, 8, 10, true, rng, &mut sel);
            small_door(&mut b, door, 4, 3, 0);
            if *left_low {
                b.gen_box(0, 3, 1, 0, 5, 3, air, air, false);
            }
            if *right_low {
                b.gen_box(9, 3, 1, 9, 5, 3, air, air, false);
            }
            if *left_high {
                b.gen_box(0, 5, 7, 0, 7, 9, air, air, false);
            }
            if *right_high {
                b.gen_box(9, 5, 7, 9, 7, 9, air, air, false);
            }
            b.gen_box(5, 1, 10, 7, 3, 10, air, air, false);
            b.gen_box_sel(1, 2, 1, 8, 2, 6, false, rng, &mut sel);
            b.gen_box_sel(4, 1, 5, 4, 4, 9, false, rng, &mut sel);
            b.gen_box_sel(8, 1, 5, 8, 4, 9, false, rng, &mut sel);
            b.gen_box_sel(1, 4, 7, 3, 4, 9, false, rng, &mut sel);
            b.gen_box_sel(1, 3, 5, 3, 3, 6, false, rng, &mut sel);
            b.gen_box(1, 3, 4, 3, 3, 4, slab, slab, false);
            b.gen_box(1, 4, 6, 3, 4, 6, slab, slab, false);
            b.gen_box_sel(5, 1, 7, 7, 1, 8, false, rng, &mut sel);
            b.gen_box(5, 1, 9, 7, 1, 9, slab, slab, false);
            b.gen_box(5, 2, 7, 7, 2, 7, slab, slab, false);
            b.gen_box(4, 5, 7, 4, 5, 9, slab, slab, false);
            b.gen_box(8, 5, 7, 8, 5, 9, slab, slab, false);
            let dslab = b.s("smooth_stone_slab", &[("type", "double")]);
            b.gen_box(5, 5, 7, 7, 5, 9, dslab, dslab, false);
            let ts = torch(&b, "south");
            b.place_block(ts, 6, 5, 6);
        }
        ShKind::Library => {
            let tall = bb.y_span() > 6;
            let h = if tall { 11 } else { 6 };
            let web = b.d("cobweb");
            let bookshelf = b.d("bookshelf");
            b.gen_box_sel(0, 0, 0, 13, h - 1, 14, true, rng, &mut sel);
            small_door(&mut b, door, 4, 1, 0);
            b.maybe_box(rng, 0.07, 2, 1, 1, 11, 4, 13, web, web, false, false);
            let te = torch(&b, "east");
            let tw = torch(&b, "west");
            for d in 1..=13 {
                if (d - 1) % 4 == 0 {
                    b.gen_box(1, 1, d, 1, 4, d, planks, planks, false);
                    b.gen_box(12, 1, d, 12, 4, d, planks, planks, false);
                    b.place_block(te, 2, 3, d);
                    b.place_block(tw, 11, 3, d);
                    if tall {
                        b.gen_box(1, 6, d, 1, 9, d, planks, planks, false);
                        b.gen_box(12, 6, d, 12, 9, d, planks, planks, false);
                    }
                } else {
                    b.gen_box(1, 1, d, 1, 4, d, bookshelf, bookshelf, false);
                    b.gen_box(12, 1, d, 12, 4, d, bookshelf, bookshelf, false);
                    if tall {
                        b.gen_box(1, 6, d, 1, 9, d, bookshelf, bookshelf, false);
                        b.gen_box(12, 6, d, 12, 9, d, bookshelf, bookshelf, false);
                    }
                }
            }
            for d in [3, 5, 7, 9, 11] {
                b.gen_box(3, 1, d, 4, 3, d, bookshelf, bookshelf, false);
                b.gen_box(6, 1, d, 7, 3, d, bookshelf, bookshelf, false);
                b.gen_box(9, 1, d, 10, 3, d, bookshelf, bookshelf, false);
            }
            if tall {
                b.gen_box(1, 5, 1, 3, 5, 13, planks, planks, false);
                b.gen_box(10, 5, 1, 12, 5, 13, planks, planks, false);
                b.gen_box(4, 5, 1, 9, 5, 2, planks, planks, false);
                b.gen_box(4, 5, 12, 9, 5, 13, planks, planks, false);
                b.place_block(planks, 9, 5, 11);
                b.place_block(planks, 8, 5, 11);
                b.place_block(planks, 9, 5, 10);
                let f = |b: &PieceBuilder<'_>, p: &[&str]| {
                    let props: Vec<(&str, &str)> = p.iter().map(|k| (*k, "true")).collect();
                    b.s("oak_fence", &props)
                };
                let f_ns = f(&b, &["north", "south"]);
                let f_we = f(&b, &["west", "east"]);
                let f_ne = f(&b, &["north", "east"]);
                let f_se = f(&b, &["south", "east"]);
                let f_nw = f(&b, &["north", "west"]);
                let f_sw = f(&b, &["south", "west"]);
                b.gen_box(3, 6, 3, 3, 6, 11, f_ns, f_ns, false);
                b.gen_box(10, 6, 3, 10, 6, 9, f_ns, f_ns, false);
                b.gen_box(4, 6, 2, 9, 6, 2, f_we, f_we, false);
                b.gen_box(4, 6, 12, 7, 6, 12, f_we, f_we, false);
                b.place_block(f_ne, 3, 6, 2);
                b.place_block(f_se, 3, 6, 12);
                b.place_block(f_nw, 10, 6, 2);
                for i in 0..=2 {
                    b.place_block(f_sw, 8 + i, 6, 12 - i);
                    if i != 2 {
                        b.place_block(f_ne, 8 + i, 6, 11 - i);
                    }
                }
                let ladder = b.s("ladder", &[("facing", "south")]);
                for y in 1..=7 {
                    b.place_block(ladder, 10, y, 13);
                }
                let f_e = f(&b, &["east"]);
                let f_w = f(&b, &["west"]);
                let f_all = f(&b, &["north", "south", "west", "east"]);
                b.place_block(f_e, 6, 9, 7);
                b.place_block(f_w, 7, 9, 7);
                b.place_block(f_e, 6, 8, 7);
                b.place_block(f_w, 7, 8, 7);
                b.place_block(f_all, 6, 7, 7);
                b.place_block(f_all, 7, 7, 7);
                b.place_block(f_e, 5, 7, 7);
                b.place_block(f_w, 8, 7, 7);
                b.place_block(f_ne, 6, 7, 6);
                b.place_block(f_se, 6, 7, 8);
                b.place_block(f_nw, 7, 7, 6);
                b.place_block(f_sw, 7, 7, 8);
                let t = b.d("torch");
                for (x, y, z) in [
                    (5, 8, 7),
                    (8, 8, 7),
                    (6, 8, 6),
                    (6, 8, 8),
                    (7, 8, 6),
                    (7, 8, 8),
                ] {
                    b.place_block(t, x, y, z);
                }
            }
            b.create_chest(rng, 3, 3, 5, "minecraft:chests/stronghold_library");
            if tall {
                b.place_block(air, 12, 9, 1);
                b.create_chest(rng, 12, 8, 1, "minecraft:chests/stronghold_library");
            }
        }
        ShKind::PortalRoom { has_placed_spawner } => {
            b.gen_box_sel(0, 0, 0, 10, 7, 15, false, rng, &mut sel);
            small_door(&mut b, SmallDoor::Grates, 4, 1, 0);
            b.gen_box_sel(1, 6, 1, 1, 6, 14, false, rng, &mut sel);
            b.gen_box_sel(9, 6, 1, 9, 6, 14, false, rng, &mut sel);
            b.gen_box_sel(2, 6, 1, 8, 6, 2, false, rng, &mut sel);
            b.gen_box_sel(2, 6, 14, 8, 6, 14, false, rng, &mut sel);
            let lava = b.d("lava");
            b.gen_box_sel(1, 1, 1, 2, 1, 4, false, rng, &mut sel);
            b.gen_box_sel(8, 1, 1, 9, 1, 4, false, rng, &mut sel);
            b.gen_box(1, 1, 1, 1, 1, 3, lava, lava, false);
            b.gen_box(9, 1, 1, 9, 1, 3, lava, lava, false);
            b.gen_box_sel(3, 1, 8, 7, 1, 12, false, rng, &mut sel);
            b.gen_box(4, 1, 9, 6, 1, 11, lava, lava, false);
            let ns = b.s("iron_bars", &[("north", "true"), ("south", "true")]);
            let we = b.s("iron_bars", &[("west", "true"), ("east", "true")]);
            for z in [3, 5, 7, 9, 11, 13] {
                b.gen_box(0, 3, z, 0, 4, z, ns, ns, false);
                b.gen_box(10, 3, z, 10, 4, z, ns, ns, false);
            }
            for x in [2, 4, 6, 8] {
                b.gen_box(x, 3, 15, x, 4, 15, we, we, false);
            }
            b.gen_box_sel(4, 1, 5, 6, 1, 7, false, rng, &mut sel);
            b.gen_box_sel(4, 2, 6, 6, 2, 7, false, rng, &mut sel);
            b.gen_box_sel(4, 3, 7, 6, 3, 7, false, rng, &mut sel);
            let st = b.s("stone_brick_stairs", &[("facing", "north")]);
            for x in 4..=6 {
                b.place_block(st, x, 1, 4);
                b.place_block(st, x, 2, 5);
                b.place_block(st, x, 3, 6);
            }
            let mut eyes = [false; 12];
            let mut all_eyes = true;
            for e in &mut eyes {
                *e = rng.next_float() > 0.9;
                all_eyes &= *e;
            }
            let frame = |b: &PieceBuilder<'_>, facing: &str, eye: bool| {
                b.s(
                    "end_portal_frame",
                    &[
                        ("facing", facing),
                        ("eye", if eye { "true" } else { "false" }),
                    ],
                )
            };
            type FrameRow = (&'static str, [(i32, i32, i32); 3]);
            let spots: [FrameRow; 4] = [
                ("north", [(4, 3, 8), (5, 3, 8), (6, 3, 8)]),
                ("south", [(4, 3, 12), (5, 3, 12), (6, 3, 12)]),
                ("east", [(3, 3, 9), (3, 3, 10), (3, 3, 11)]),
                ("west", [(7, 3, 9), (7, 3, 10), (7, 3, 11)]),
            ];
            for (g, (facing, cells)) in spots.iter().enumerate() {
                for (i, (x, y, z)) in cells.iter().enumerate() {
                    let st = frame(&b, facing, eyes[g * 3 + i]);
                    b.place_block(st, *x, *y, *z);
                }
            }
            if all_eyes {
                let portal = b.d("end_portal");
                for z in 9..=11 {
                    for x in 4..=6 {
                        b.place_block(portal, x, 3, z);
                    }
                }
            }
            if !*has_placed_spawner {
                let pos = b.world_pos(5, 3, 6);
                if chunk.is_inside(pos) {
                    *has_placed_spawner = true;
                    let sp = b.d("spawner");
                    b.world.set_block_state(pos, sp, 2);
                    crate::piece::set_spawner(b.world, pos, "minecraft:silverfish");
                }
            }
        }
    }
}
