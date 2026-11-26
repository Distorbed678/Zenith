//! Nether fortress piece placement (`docs/specs/worldgen/structures-legacy-pieces.md` §4.4).

use zenith_math::LegacyRandom;

use crate::generator::StructurePiece;
use crate::geom::BoundingBoxExt;
use crate::piece::PieceBuilder;
use crate::piece_data::PieceData;
use crate::place::PieceCall;
use crate::world::WorldgenRandom;

/// Fortress piece data.
#[derive(Debug, Clone, PartialEq)]
pub struct FortressPiece {
    /// Registered piece id (e.g. `minecraft:nebs`).
    pub id: &'static str,
    /// `isNeedingChest` of the small-corridor turns.
    pub needs_chest: bool,
    /// `selfSeed` of the bridge end filler (`needs_chest` is meaningless there).
    pub self_seed: i32,
}

struct Ctx<'a, 'b> {
    b: &'a mut PieceBuilder<'b>,
    nb: u32,
    air: u32,
}

impl Ctx<'_, '_> {
    #[allow(clippy::too_many_arguments)]
    fn nb(&mut self, x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32) {
        self.b.fill(x0, y0, z0, x1, y1, z1, self.nb);
    }
    #[allow(clippy::too_many_arguments)]
    fn air(&mut self, x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32) {
        self.b.fill(x0, y0, z0, x1, y1, z1, self.air);
    }
    fn fence(&self, props: &[&str]) -> u32 {
        let p: Vec<(&str, &str)> = props.iter().map(|k| (*k, "true")).collect();
        self.b.s("nether_brick_fence", &p)
    }
    #[allow(clippy::too_many_arguments)]
    fn f(&mut self, props: &[&str], x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32) {
        let st = self.fence(props);
        self.b.fill(x0, y0, z0, x1, y1, z1, st);
    }
    fn col(&mut self, x: i32, z: i32) {
        self.b.fill_column_down(self.nb, x, -1, z);
    }
}

const NS: [&str; 2] = ["north", "south"];
const WE: [&str; 2] = ["west", "east"];

fn castle_room_common(c: &mut Ctx<'_, '_>, entrance: bool) {
    c.nb(0, 3, 0, 12, 4, 12);
    c.air(0, 5, 0, 12, 13, 12);
    c.nb(0, 5, 0, 1, 12, 12);
    c.nb(11, 5, 0, 12, 12, 12);
    c.nb(2, 5, 11, 4, 12, 12);
    c.nb(8, 5, 11, 10, 12, 12);
    c.nb(5, 9, 11, 7, 12, 12);
    c.nb(2, 5, 0, 4, 12, 1);
    c.nb(8, 5, 0, 10, 12, 1);
    c.nb(5, 9, 0, 7, 12, 1);
    c.nb(2, 11, 2, 10, 12, 10);
    if entrance {
        c.f(&[], 5, 8, 0, 7, 8, 0);
    }
    for i in [1, 3, 5, 7, 9, 11] {
        c.f(&WE, i, 10, 0, i, 11, 0);
        c.f(&WE, i, 10, 12, i, 11, 12);
        c.f(&NS, 0, 10, i, 0, 11, i);
        c.f(&NS, 12, 10, i, 12, 11, i);
        let nb = c.nb;
        c.b.place_block(nb, i, 13, 0);
        c.b.place_block(nb, i, 13, 12);
        c.b.place_block(nb, 0, 13, i);
        c.b.place_block(nb, 12, 13, i);
        if i != 11 {
            c.f(&WE, i + 1, 13, 0, i + 1, 13, 0);
            c.f(&WE, i + 1, 13, 12, i + 1, 13, 12);
            c.f(&NS, 0, 13, i + 1, 0, 13, i + 1);
            c.f(&NS, 12, 13, i + 1, 12, 13, i + 1);
        }
    }
    c.f(&["north", "east"], 0, 13, 0, 0, 13, 0);
    c.f(&["south", "east"], 0, 13, 12, 0, 13, 12);
    c.f(&["south", "west"], 12, 13, 12, 12, 13, 12);
    c.f(&["north", "west"], 12, 13, 0, 12, 13, 0);
    for z in [3, 5, 7, 9] {
        c.f(&["north", "south", "west"], 1, 7, z, 1, 8, z);
        c.f(&["north", "south", "east"], 11, 7, z, 11, 8, z);
    }
    c.nb(4, 2, 0, 8, 2, 12);
    c.nb(0, 2, 4, 12, 2, 8);
    c.nb(4, 0, 0, 8, 1, 3);
    c.nb(4, 0, 9, 8, 1, 12);
    c.nb(0, 0, 4, 3, 1, 8);
    c.nb(9, 0, 4, 12, 1, 8);
    for x in 4..=8 {
        for z in (0..=2).chain(10..=12) {
            c.col(x, z);
        }
    }
    for x in (0..=2).chain(10..=12) {
        for z in 4..=8 {
            c.col(x, z);
        }
    }
}

fn small_corridor_roof_and_columns(c: &mut Ctx<'_, '_>) {
    c.nb(0, 6, 0, 4, 6, 4);
    for x in 0..=4 {
        for z in 0..=4 {
            c.col(x, z);
        }
    }
}

/// `postProcess` of every fortress piece.
pub fn post_process(piece: &mut StructurePiece, call: &mut PieceCall<'_>) {
    let bb = piece.bounding_box;
    let orient = piece.orientation;
    let chunk = *call.chunk_bb;
    let PieceData::Fortress(data) = &mut piece.data else {
        return;
    };
    let mut b = PieceBuilder::new(&mut *call.world, bb, orient, chunk);
    let nb = b.d("nether_bricks");
    let air = b.air();
    let rng = &mut *call.random;
    let mut c = Ctx { b: &mut b, nb, air };
    match data.id {
        "minecraft:nebcr" => {
            c.nb(7, 3, 0, 11, 4, 18);
            c.nb(0, 3, 7, 18, 4, 11);
            c.air(8, 5, 0, 10, 7, 18);
            c.air(0, 5, 8, 18, 7, 10);
            for (x0, z0, x1, z1) in [
                (7, 0, 7, 7),
                (7, 11, 7, 18),
                (11, 0, 11, 7),
                (11, 11, 11, 18),
            ] {
                c.nb(x0, 5, z0, x1, 5, z1);
            }
            for (x0, z0, x1, z1) in [
                (0, 7, 7, 7),
                (11, 7, 18, 7),
                (0, 11, 7, 11),
                (11, 11, 18, 11),
            ] {
                c.nb(x0, 5, z0, x1, 5, z1);
            }
            c.nb(7, 2, 0, 11, 2, 5);
            c.nb(7, 2, 13, 11, 2, 18);
            c.nb(7, 0, 0, 11, 1, 3);
            c.nb(7, 0, 15, 11, 1, 18);
            for x in 7..=11 {
                for z in (0..=2).chain(16..=18) {
                    c.col(x, z);
                }
            }
            c.nb(0, 2, 7, 5, 2, 11);
            c.nb(13, 2, 7, 18, 2, 11);
            c.nb(0, 0, 7, 3, 1, 11);
            c.nb(15, 0, 7, 18, 1, 11);
            for x in (0..=2).chain(16..=18) {
                for z in 7..=11 {
                    c.col(x, z);
                }
            }
        }
        "minecraft:nebef" => {
            let mut r = LegacyRandom::new(i64::from(data.self_seed));
            for x in 0..=4 {
                for y in 3..=4 {
                    let z = r.next_int_bounded(8);
                    c.nb(x, y, 0, x, y, z);
                }
            }
            let z = r.next_int_bounded(8);
            c.nb(0, 5, 0, 0, 5, z);
            let z = r.next_int_bounded(8);
            c.nb(4, 5, 0, 4, 5, z);
            for x in 0..=4 {
                let z = r.next_int_bounded(5);
                c.nb(x, 2, 0, x, 2, z);
            }
            for x in 0..=4 {
                for y in 0..=1 {
                    let z = r.next_int_bounded(3);
                    c.nb(x, y, 0, x, y, z);
                }
            }
        }
        "minecraft:nebs" => {
            c.nb(0, 3, 0, 4, 4, 18);
            c.air(1, 5, 0, 3, 7, 18);
            c.nb(0, 5, 0, 0, 5, 18);
            c.nb(4, 5, 0, 4, 5, 18);
            c.nb(0, 2, 0, 4, 2, 5);
            c.nb(0, 2, 13, 4, 2, 18);
            c.nb(0, 0, 0, 4, 1, 3);
            c.nb(0, 0, 15, 4, 1, 18);
            for x in 0..=4 {
                for z in (0..=2).chain(16..=18) {
                    c.col(x, z);
                }
            }
            let ne = ["north", "south", "east"];
            let nw = ["north", "south", "west"];
            for (z0, y0, z1) in [(1, 1, 1), (4, 3, 4), (14, 3, 14), (17, 1, 17)] {
                c.f(&ne, 0, y0, z0, 0, 4, z1);
                c.f(&nw, 4, y0, z0, 4, 4, z1);
            }
        }
        "minecraft:nerc" => {
            c.nb(0, 0, 0, 6, 1, 6);
            c.air(0, 2, 0, 6, 7, 6);
            for (x0, z0, x1, z1) in [
                (0, 0, 1, 0),
                (0, 6, 1, 6),
                (5, 0, 6, 0),
                (5, 6, 6, 6),
                (0, 0, 0, 1),
                (0, 5, 0, 6),
                (6, 0, 6, 1),
                (6, 5, 6, 6),
            ] {
                c.nb(x0, 2, z0, x1, 6, z1);
            }
            c.nb(2, 6, 0, 4, 6, 0);
            c.f(&WE, 2, 5, 0, 4, 5, 0);
            c.nb(2, 6, 6, 4, 6, 6);
            c.f(&WE, 2, 5, 6, 4, 5, 6);
            c.nb(0, 6, 2, 0, 6, 4);
            c.f(&NS, 0, 5, 2, 0, 5, 4);
            c.nb(6, 6, 2, 6, 6, 4);
            c.f(&NS, 6, 5, 2, 6, 5, 4);
            for x in 0..=6 {
                for z in 0..=6 {
                    c.col(x, z);
                }
            }
        }
        "minecraft:nesr" => {
            c.nb(0, 0, 0, 6, 1, 6);
            c.air(0, 2, 0, 6, 10, 6);
            c.nb(0, 2, 0, 1, 8, 0);
            c.nb(5, 2, 0, 6, 8, 0);
            c.nb(0, 2, 1, 0, 8, 6);
            c.nb(6, 2, 1, 6, 8, 6);
            c.nb(1, 2, 6, 5, 8, 6);
            c.f(&NS, 0, 3, 2, 0, 5, 4);
            c.f(&NS, 6, 3, 2, 6, 5, 2);
            c.f(&NS, 6, 3, 4, 6, 5, 4);
            c.nb(5, 2, 5, 5, 2, 5);
            c.nb(4, 2, 5, 4, 3, 5);
            c.nb(3, 2, 5, 3, 4, 5);
            c.nb(2, 2, 5, 2, 5, 5);
            c.nb(1, 2, 5, 1, 6, 5);
            c.nb(1, 7, 1, 5, 7, 4);
            c.air(6, 8, 2, 6, 8, 4);
            c.nb(2, 6, 0, 4, 8, 0);
            c.f(&WE, 2, 5, 0, 4, 5, 0);
            for x in 0..=6 {
                for z in 0..=6 {
                    c.col(x, z);
                }
            }
        }
        "minecraft:nemt" => {
            c.air(0, 2, 0, 6, 7, 7);
            c.nb(1, 0, 0, 5, 1, 7);
            c.nb(1, 2, 1, 5, 2, 7);
            c.nb(1, 3, 2, 5, 3, 7);
            c.nb(1, 4, 3, 5, 4, 7);
            c.nb(1, 2, 0, 1, 4, 2);
            c.nb(5, 2, 0, 5, 4, 2);
            c.nb(1, 5, 2, 1, 5, 3);
            c.nb(5, 5, 2, 5, 5, 3);
            c.nb(0, 5, 3, 0, 5, 8);
            c.nb(6, 5, 3, 6, 5, 8);
            c.nb(1, 5, 8, 5, 5, 8);
            c.f(&["west"], 1, 6, 3, 1, 6, 3);
            c.f(&["east"], 5, 6, 3, 5, 6, 3);
            c.f(&["east", "north"], 0, 6, 3, 0, 6, 3);
            c.f(&["west", "north"], 6, 6, 3, 6, 6, 3);
            c.f(&NS, 0, 6, 4, 0, 6, 7);
            c.f(&NS, 6, 6, 4, 6, 6, 7);
            c.f(&["east", "south"], 0, 6, 8, 0, 6, 8);
            c.f(&["west", "south"], 6, 6, 8, 6, 6, 8);
            c.f(&WE, 1, 6, 8, 5, 6, 8);
            c.f(&["east"], 1, 7, 8, 1, 7, 8);
            c.f(&WE, 2, 7, 8, 4, 7, 8);
            c.f(&["west"], 5, 7, 8, 5, 7, 8);
            c.f(&["east"], 2, 8, 8, 2, 8, 8);
            c.f(&WE, 3, 8, 8, 3, 8, 8);
            c.f(&["west"], 4, 8, 8, 4, 8, 8);
            // Blaze spawner (the flag lives in the data only through the chest bit; vanilla
            // keeps `hasPlacedSpawner` in the piece, tracked here via `needs_chest`).
            if !data.needs_chest {
                let pos = c.b.world_pos(3, 5, 5);
                if chunk.is_inside(pos) {
                    data.needs_chest = true;
                    let sp = c.b.d("spawner");
                    c.b.world.set_block_state(pos, sp, 2);
                    crate::piece::set_spawner(c.b.world, pos, "minecraft:blaze");
                }
            }
            for x in 0..=6 {
                for z in 0..=6 {
                    c.col(x, z);
                }
            }
        }
        "minecraft:nece" => castle_room_common(&mut c, true),
        "minecraft:necsr" => {
            castle_room_common(&mut c, false);
            let stairs_n = c.b.s("nether_brick_stairs", &[("facing", "north")]);
            let stairs_w = c.b.s("nether_brick_stairs", &[("facing", "west")]);
            let stairs_e = c.b.s("nether_brick_stairs", &[("facing", "east")]);
            for i in 0..=6 {
                let z = i + 4;
                for x in 5..=7 {
                    c.b.place_block(stairs_n, x, 5 + i, z);
                }
                if (5..=8).contains(&z) {
                    c.nb(5, 5, z, 7, i + 4, z);
                } else if (9..=10).contains(&z) {
                    c.nb(5, 8, z, 7, i + 4, z);
                }
                if i >= 1 {
                    c.air(5, 6 + i, z, 7, 9 + i, z);
                }
            }
            for x in 5..=7 {
                c.b.place_block(stairs_n, x, 12, 11);
            }
            c.f(&["north", "south", "east"], 5, 6, 7, 5, 7, 7);
            c.f(&["north", "south", "west"], 7, 6, 7, 7, 7, 7);
            c.air(5, 13, 12, 7, 13, 12);
            c.nb(2, 5, 2, 3, 5, 3);
            c.nb(2, 5, 9, 3, 5, 10);
            c.nb(2, 5, 4, 2, 5, 8);
            c.nb(9, 5, 2, 10, 5, 3);
            c.nb(9, 5, 9, 10, 5, 10);
            c.nb(10, 5, 4, 10, 5, 8);
            for (x, z) in [(4, 2), (4, 3), (4, 9), (4, 10)] {
                c.b.place_block(stairs_w, x, 5, z);
            }
            for (x, z) in [(8, 2), (8, 3), (8, 9), (8, 10)] {
                c.b.place_block(stairs_e, x, 5, z);
            }
            let sand = c.b.d("soul_sand");
            let wart = c.b.s("nether_wart", &[("age", "0")]);
            c.b.fill(3, 4, 4, 4, 4, 8, sand);
            c.b.fill(8, 4, 4, 9, 4, 8, sand);
            c.b.fill(3, 5, 4, 4, 5, 8, wart);
            c.b.fill(8, 5, 4, 9, 5, 8, wart);
        }
        "minecraft:nesc" => {
            c.nb(0, 0, 0, 4, 1, 4);
            c.air(0, 2, 0, 4, 5, 4);
            c.nb(0, 2, 0, 0, 5, 4);
            c.nb(4, 2, 0, 4, 5, 4);
            c.f(&NS, 0, 3, 1, 0, 4, 1);
            c.f(&NS, 0, 3, 3, 0, 4, 3);
            c.f(&NS, 4, 3, 1, 4, 4, 1);
            c.f(&NS, 4, 3, 3, 4, 4, 3);
            small_corridor_roof_and_columns(&mut c);
        }
        "minecraft:nescsc" => {
            c.nb(0, 0, 0, 4, 1, 4);
            c.air(0, 2, 0, 4, 5, 4);
            c.nb(0, 2, 0, 0, 5, 0);
            c.nb(4, 2, 0, 4, 5, 0);
            c.nb(0, 2, 4, 0, 5, 4);
            c.nb(4, 2, 4, 4, 5, 4);
            small_corridor_roof_and_columns(&mut c);
        }
        "minecraft:nesclt" | "minecraft:nescrt" => {
            let left = data.id == "minecraft:nesclt";
            c.nb(0, 0, 0, 4, 1, 4);
            c.air(0, 2, 0, 4, 5, 4);
            if left {
                c.nb(4, 2, 0, 4, 5, 4);
                c.f(&NS, 4, 3, 1, 4, 4, 1);
                c.f(&NS, 4, 3, 3, 4, 4, 3);
                c.nb(0, 2, 0, 0, 5, 0);
                c.nb(0, 2, 4, 3, 5, 4);
                c.f(&WE, 1, 3, 4, 1, 4, 4);
                c.f(&WE, 3, 3, 4, 3, 4, 4);
                let wp = c.b.world_pos(3, 2, 3);
                if data.needs_chest && chunk.is_inside(wp) {
                    data.needs_chest = false;
                    c.b.create_chest(rng, 3, 2, 3, "minecraft:chests/nether_bridge");
                }
            } else {
                c.nb(0, 2, 0, 0, 5, 4);
                c.f(&NS, 0, 3, 1, 0, 4, 1);
                c.f(&NS, 0, 3, 3, 0, 4, 3);
                c.nb(4, 2, 0, 4, 5, 0);
                c.nb(1, 2, 4, 4, 5, 4);
                c.f(&WE, 1, 3, 4, 1, 4, 4);
                c.f(&WE, 3, 3, 4, 3, 4, 4);
                let wp = c.b.world_pos(1, 2, 3);
                if data.needs_chest && chunk.is_inside(wp) {
                    data.needs_chest = false;
                    c.b.create_chest(rng, 1, 2, 3, "minecraft:chests/nether_bridge");
                }
            }
            small_corridor_roof_and_columns(&mut c);
        }
        "minecraft:neccs" => {
            let stairs = c.b.s("nether_brick_stairs", &[("facing", "south")]);
            for step in 0..=9 {
                let z = step;
                let floor = 1.max(7 - step);
                let roof = (floor + 5).max(14 - step).min(13);
                c.nb(0, 0, z, 4, floor, z);
                c.air(1, floor + 1, z, 3, roof - 1, z);
                if step <= 6 {
                    for x in 1..=3 {
                        c.b.place_block(stairs, x, floor + 1, z);
                    }
                }
                c.nb(0, roof, z, 4, roof, z);
                c.nb(0, floor + 1, z, 0, roof - 1, z);
                c.nb(4, floor + 1, z, 4, roof - 1, z);
                if step & 1 == 0 {
                    c.f(&NS, 0, floor + 2, z, 0, floor + 3, z);
                    c.f(&NS, 4, floor + 2, z, 4, floor + 3, z);
                }
                for x in 0..=4 {
                    c.col(x, z);
                }
            }
        }
        "minecraft:nectb" => {
            c.nb(0, 0, 0, 8, 1, 8);
            c.air(0, 2, 0, 8, 5, 8);
            c.nb(0, 6, 0, 8, 6, 5);
            c.nb(0, 2, 0, 2, 5, 0);
            c.nb(6, 2, 0, 8, 5, 0);
            c.f(&WE, 1, 3, 0, 1, 4, 0);
            c.f(&WE, 7, 3, 0, 7, 4, 0);
            c.nb(0, 2, 4, 8, 2, 8);
            c.air(1, 1, 4, 2, 2, 4);
            c.air(6, 1, 4, 7, 2, 4);
            c.f(&WE, 1, 3, 8, 7, 3, 8);
            c.f(&["east", "south"], 0, 3, 8, 0, 3, 8);
            c.f(&["west", "south"], 8, 3, 8, 8, 3, 8);
            c.f(&NS, 0, 3, 6, 0, 3, 7);
            c.f(&NS, 8, 3, 6, 8, 3, 7);
            c.nb(0, 3, 4, 0, 5, 5);
            c.nb(8, 3, 4, 8, 5, 5);
            c.nb(1, 3, 5, 2, 5, 5);
            c.nb(6, 3, 5, 7, 5, 5);
            c.f(&WE, 1, 4, 5, 1, 5, 5);
            c.f(&WE, 7, 4, 5, 7, 5, 5);
            for x in 0..=8 {
                for z in 0..=5 {
                    c.col(x, z);
                }
            }
        }
        _ => {}
    }
    let _ = (rng as &mut dyn WorldgenRandom, c.air);
}
