//! Ocean monument: room graph, fitter assembly and block placement
//! (`docs/specs/worldgen/structures-legacy-pieces.md` §5.3–§5.6).
//!
//! The 11 child pieces are not stored in the start; like vanilla's
//! `regeneratePiecesAfterLoad` they are rebuilt deterministically from the start's chunk, the
//! world seed and the building's orientation the first time the building is placed.

use zenith_math::LegacyRandom;
use zenith_nbt::{Compound, Tag};

use crate::generator::StructurePiece;
use crate::geom::{
    BoundingBox, BoundingBoxExt, Direction, random_horizontal_direction, set_large_feature_seed,
};
use crate::piece::PieceBuilder;
use crate::piece_data::PieceData;
use crate::place::PieceCall;
use crate::world::WorldgenRandom;

const D_DOWN: usize = 0;
const D_UP: usize = 1;
const D_NORTH: usize = 2;
const D_SOUTH: usize = 3;
const D_WEST: usize = 4;
const D_EAST: usize = 5;

fn opp(d: usize) -> usize {
    d ^ 1
}

fn step(d: usize) -> (i32, i32, i32) {
    match d {
        D_DOWN => (0, -1, 0),
        D_UP => (0, 1, 0),
        D_NORTH => (0, 0, -1),
        D_SOUTH => (0, 0, 1),
        D_WEST => (-1, 0, 0),
        _ => (1, 0, 0),
    }
}

/// `RoomDefinition`.
#[derive(Debug, Clone, PartialEq)]
pub struct RoomDef {
    /// Grid index (`>= 75` for the virtual wing / roof rooms).
    pub index: i32,
    /// Neighbour slots per 3D direction.
    pub conn: [Option<usize>; 6],
    /// Open edges per 3D direction.
    pub open: [bool; 6],
    claimed: bool,
    is_source: bool,
    scan_index: i32,
}

impl RoomDef {
    fn new(index: i32) -> Self {
        Self {
            index,
            conn: [None; 6],
            open: [false; 6],
            claimed: false,
            is_source: false,
            scan_index: 0,
        }
    }
    fn count_openings(&self) -> i32 {
        self.open.iter().filter(|o| **o).count() as i32
    }
}

fn find_source(rooms: &mut [RoomDef], slot: usize, search: i32) -> bool {
    if rooms[slot].is_source {
        return true;
    }
    rooms[slot].scan_index = search;
    for i in 0..6 {
        if let Some(n) = rooms[slot].conn[i]
            && rooms[slot].open[i]
            && rooms[n].scan_index != search
            && find_source(rooms, n, search)
        {
            return true;
        }
    }
    false
}

/// Kind of a monument child piece.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RoomKind {
    /// `omentry`.
    Entry,
    /// `omcr`.
    Core,
    /// `omdxr`.
    DoubleX,
    /// `omdyr`.
    DoubleY,
    /// `omdzr`.
    DoubleZ,
    /// `omdxyr`.
    DoubleXY,
    /// `omdyzr`.
    DoubleYZ,
    /// `omsimple`.
    Simple {
        /// `mainDesign` (0..2).
        main_design: i32,
    },
    /// `omsimplet`.
    SimpleTop,
    /// `omwr`.
    Wing {
        /// `mainDesign` (0..1).
        main_design: i32,
    },
    /// `ompenthouse`.
    Penthouse,
}

/// A child piece of the monument building.
#[derive(Debug, Clone, PartialEq)]
pub struct MonumentChild {
    /// Room kind.
    pub kind: RoomKind,
    /// World-space bounding box.
    pub bb: BoundingBox,
    /// Room definition slot (`usize::MAX` for wings / penthouse).
    pub room: usize,
}

/// `MonumentBuilding` data: lazily built room graph and children.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MonumentBuilding {
    built: bool,
    rooms: Vec<RoomDef>,
    children: Vec<MonumentChild>,
}

fn room_index(x: i32, y: i32, z: i32) -> usize {
    (y * 25 + z * 5 + x) as usize
}

fn set_connection(rooms: &mut [RoomDef], a: usize, d: usize, b: usize) {
    rooms[a].conn[d] = Some(b);
    rooms[b].conn[opp(d)] = Some(a);
}

impl MonumentBuilding {
    fn build(&mut self, rng: &mut LegacyRandom, bb: BoundingBox, dir: Direction) {
        let mut rooms: Vec<RoomDef> = Vec::new();
        let mut grid: [Option<usize>; 75] = [None; 75];
        let add = |rooms: &mut Vec<RoomDef>, idx: i32| -> usize {
            rooms.push(RoomDef::new(idx));
            rooms.len() - 1
        };
        for x in 0..5 {
            for z in 0..4 {
                for y in [0, 1] {
                    let i = room_index(x, y, z);
                    grid[i] = Some(add(&mut rooms, i as i32));
                }
            }
        }
        for x in 1..4 {
            for z in 0..2 {
                let i = room_index(x, 2, z);
                grid[i] = Some(add(&mut rooms, i as i32));
            }
        }
        let source = grid[2].expect("source room");
        for x in 0..5 {
            for z in 0..5 {
                for y in 0..3 {
                    let Some(def) = grid[room_index(x, y, z)] else {
                        continue;
                    };
                    for d in 0..6 {
                        let (dx, dy, dz) = step(d);
                        let (nx, ny, nz) = (x + dx, y + dy, z + dz);
                        if (0..5).contains(&nx)
                            && (0..5).contains(&nz)
                            && (0..3).contains(&ny)
                            && let Some(n) = grid[room_index(nx, ny, nz)]
                        {
                            if nz == z {
                                set_connection(&mut rooms, def, d, n);
                            } else {
                                set_connection(&mut rooms, def, opp(d), n);
                            }
                        }
                    }
                }
            }
        }
        let roof = add(&mut rooms, 1003);
        let left_wing = add(&mut rooms, 1001);
        let right_wing = add(&mut rooms, 1002);
        set_connection(&mut rooms, grid[52].expect("52"), D_UP, roof);
        set_connection(&mut rooms, grid[25].expect("25"), D_SOUTH, left_wing);
        set_connection(&mut rooms, grid[29].expect("29"), D_SOUTH, right_wing);
        rooms[roof].claimed = true;
        rooms[left_wing].claimed = true;
        rooms[right_wing].claimed = true;
        rooms[source].is_source = true;
        let core = grid[room_index(rng.next_int_bounded(4), 0, 2)].expect("core");
        let e = rooms[core].conn[D_EAST].expect("e");
        let n = rooms[core].conn[D_NORTH].expect("n");
        let en = rooms[e].conn[D_NORTH].expect("en");
        let u = rooms[core].conn[D_UP].expect("u");
        let eu = rooms[e].conn[D_UP].expect("eu");
        let nu = rooms[n].conn[D_UP].expect("nu");
        let enu = rooms[en].conn[D_UP].expect("enu");
        for s in [core, e, n, en, u, eu, nu, enu] {
            rooms[s].claimed = true;
        }
        let mut defs: Vec<usize> = grid.iter().flatten().copied().collect();
        for &s in &defs {
            for i in 0..6 {
                rooms[s].open[i] = rooms[s].conn[i].is_some();
            }
        }
        for i in 0..6 {
            rooms[roof].open[i] = rooms[roof].conn[i].is_some();
        }
        crate::world::rng_shuffle(&mut defs, rng);
        let mut scan_index = 1;
        for &def in &defs {
            let (mut close_count, mut attempts) = (0, 0);
            while close_count < 2 && attempts < 5 {
                attempts += 1;
                let f = rng.next_int_bounded(6) as usize;
                if rooms[def].open[f] {
                    let o = opp(f);
                    let other = rooms[def].conn[f].expect("conn");
                    rooms[def].open[f] = false;
                    rooms[other].open[o] = false;
                    let a = find_source(&mut rooms, def, scan_index);
                    scan_index += 1;
                    let ok = a && {
                        let r = find_source(&mut rooms, other, scan_index);
                        scan_index += 1;
                        r
                    };
                    if ok {
                        close_count += 1;
                    } else {
                        rooms[def].open[f] = true;
                        rooms[other].open[o] = true;
                    }
                }
            }
        }
        defs.push(roof);
        defs.push(left_wing);
        defs.push(right_wing);

        // Fit rooms.
        let mut children: Vec<MonumentChild> = Vec::new();
        rooms[source].claimed = true;
        let mk_bb = |idx: i32, w: i32, h: i32, dp: i32| -> BoundingBox {
            let rx = idx % 5;
            let rz = (idx / 5) % 5;
            let ry = idx / 25;
            let base = BoundingBox::make_box(0, 0, 0, dir, w * 8, h * 4, dp * 8);
            match dir {
                Direction::North => base.moved(rx * 8, ry * 4, -(rz + dp) * 8 + 1),
                Direction::South => base.moved(rx * 8, ry * 4, rz * 8),
                Direction::West => base.moved(-(rz + dp) * 8 + 1, ry * 4, rx * 8),
                _ => base.moved(rz * 8, ry * 4, rx * 8),
            }
        };
        children.push(MonumentChild {
            kind: RoomKind::Entry,
            bb: mk_bb(rooms[source].index, 1, 1, 1),
            room: source,
        });
        children.push(MonumentChild {
            kind: RoomKind::Core,
            bb: mk_bb(rooms[core].index, 2, 2, 2),
            room: core,
        });
        for &def in &defs {
            if rooms[def].claimed || rooms[def].index >= 75 {
                continue;
            }
            let idx = rooms[def].index;
            let r = &rooms;
            let unclaimed = |s: Option<usize>| s.is_some_and(|s| !r[s].claimed);
            let open_to = |s: usize, d: usize| r[s].open[d] && unclaimed(r[s].conn[d]);
            // 1. DoubleXY
            if open_to(def, D_EAST) && open_to(def, D_UP) {
                let east = r[def].conn[D_EAST].expect("e");
                if open_to(east, D_UP) {
                    let up = r[def].conn[D_UP].expect("u");
                    let eu = r[east].conn[D_UP].expect("eu");
                    for s in [def, east, up, eu] {
                        rooms[s].claimed = true;
                    }
                    children.push(MonumentChild {
                        kind: RoomKind::DoubleXY,
                        bb: mk_bb(idx, 2, 2, 1),
                        room: def,
                    });
                    continue;
                }
            }
            // 2. DoubleYZ
            if open_to(def, D_NORTH) && open_to(def, D_UP) {
                let north = r[def].conn[D_NORTH].expect("n");
                if open_to(north, D_UP) {
                    let up = r[def].conn[D_UP].expect("u");
                    let nu = r[north].conn[D_UP].expect("nu");
                    for s in [def, north, up, nu] {
                        rooms[s].claimed = true;
                    }
                    children.push(MonumentChild {
                        kind: RoomKind::DoubleYZ,
                        bb: mk_bb(idx, 1, 2, 2),
                        room: def,
                    });
                    continue;
                }
            }
            // 3. DoubleZ
            if open_to(def, D_NORTH) {
                let mut other = r[def].conn[D_NORTH].expect("n");
                let mut piece_room = def;
                if !r[def].open[D_NORTH] || r[other].claimed {
                    piece_room = r[def].conn[D_SOUTH].expect("s");
                    other = def;
                }
                let _ = piece_room;
                rooms[def].claimed = true;
                rooms[other].claimed = true;
                children.push(MonumentChild {
                    kind: RoomKind::DoubleZ,
                    bb: mk_bb(idx, 1, 1, 2),
                    room: def,
                });
                continue;
            }
            // 4. DoubleX
            if open_to(def, D_EAST) {
                let east = r[def].conn[D_EAST].expect("e");
                rooms[def].claimed = true;
                rooms[east].claimed = true;
                children.push(MonumentChild {
                    kind: RoomKind::DoubleX,
                    bb: mk_bb(idx, 2, 1, 1),
                    room: def,
                });
                continue;
            }
            // 5. DoubleY
            if open_to(def, D_UP) {
                let up = r[def].conn[D_UP].expect("u");
                rooms[def].claimed = true;
                rooms[up].claimed = true;
                children.push(MonumentChild {
                    kind: RoomKind::DoubleY,
                    bb: mk_bb(idx, 1, 2, 1),
                    room: def,
                });
                continue;
            }
            // 6. SimpleTop
            let d = &r[def];
            if !d.open[D_WEST]
                && !d.open[D_EAST]
                && !d.open[D_NORTH]
                && !d.open[D_SOUTH]
                && !d.open[D_UP]
            {
                rooms[def].claimed = true;
                children.push(MonumentChild {
                    kind: RoomKind::SimpleTop,
                    bb: mk_bb(idx, 1, 1, 1),
                    room: def,
                });
                continue;
            }
            // 7. Simple
            rooms[def].claimed = true;
            let main_design = rng.next_int_bounded(3);
            children.push(MonumentChild {
                kind: RoomKind::Simple { main_design },
                bb: mk_bb(idx, 1, 1, 1),
                room: def,
            });
        }
        // Move every child by getWorldPos(9, 0, 22).
        let off = crate::geom::piece_world_pos(bb, Some(dir), 9, 0, 22);
        for c in &mut children {
            c.bb = c.bb.moved(off.x, off.y, off.z);
        }
        let wp = |x, y, z| crate::geom::piece_world_pos(bb, Some(dir), x, y, z);
        let left_bb = BoundingBox::from_corners(wp(1, 1, 1), wp(23, 8, 21));
        let right_bb = BoundingBox::from_corners(wp(34, 1, 1), wp(56, 8, 21));
        let pent_bb = BoundingBox::from_corners(wp(22, 13, 22), wp(35, 17, 35));
        let wing_random = rng.next_int();
        children.push(MonumentChild {
            kind: RoomKind::Wing {
                main_design: wing_random & 1,
            },
            bb: left_bb,
            room: usize::MAX,
        });
        children.push(MonumentChild {
            kind: RoomKind::Wing {
                main_design: wing_random.wrapping_add(1) & 1,
            },
            bb: right_bb,
            room: usize::MAX,
        });
        children.push(MonumentChild {
            kind: RoomKind::Penthouse,
            bb: pent_bb,
            room: usize::MAX,
        });
        self.rooms = rooms;
        self.children = children;
        self.built = true;
    }

    /// The child pieces (empty until the building is first placed).
    pub fn children(&self) -> &[MonumentChild] {
        &self.children
    }
}

/// Block shorthands and helpers shared by all monument pieces.
struct Mon<'a, 'b> {
    b: &'a mut PieceBuilder<'b>,
    g: u32,
    l: u32,
    d: u32,
    sl: u32,
    w: u32,
}

impl<'a, 'b> Mon<'a, 'b> {
    fn new(b: &'a mut PieceBuilder<'b>) -> Self {
        let g = b.d("prismarine");
        let l = b.d("prismarine_bricks");
        let d = b.d("dark_prismarine");
        let sl = b.d("sea_lantern");
        let w = b.d("water");
        Self { b, g, l, d, sl, w }
    }
    #[allow(clippy::too_many_arguments)]
    fn f(&mut self, blk: u32, x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32) {
        self.b.fill(x0, y0, z0, x1, y1, z1, blk);
    }
    fn p(&mut self, blk: u32, x: i32, y: i32, z: i32) {
        self.b.place_block(blk, x, y, z);
    }
    /// `generateWaterBox`.
    #[allow(clippy::too_many_arguments)]
    fn water(&mut self, x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32) {
        let sea = self.b.world.sea_level();
        let air = self.b.air();
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    let blk = self.b.get_block(x, y, z);
                    let path = self.b.path_of(blk);
                    if matches!(path.as_str(), "ice" | "packed_ice" | "blue_ice" | "water") {
                        continue;
                    }
                    let wy = self.b.bb.min_y + y;
                    if wy >= sea && blk != self.w {
                        self.b.place_block(air, x, y, z);
                    } else {
                        self.b.place_block(self.w, x, y, z);
                    }
                }
            }
        }
    }
    /// `generateBoxOnFillOnly`.
    #[allow(clippy::too_many_arguments)]
    fn on_fill(&mut self, blk: u32, x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if self.b.get_block(x, y, z) == self.w {
                        self.b.place_block(blk, x, y, z);
                    }
                }
            }
        }
    }
    fn default_floor(&mut self, xo: i32, zo: i32, down_opening: bool) {
        let g = self.g;
        let l = self.l;
        if !down_opening {
            self.f(g, xo, 0, zo, xo + 7, 0, zo + 7);
        } else {
            self.f(g, xo, 0, zo, xo + 2, 0, zo + 7);
            self.f(g, xo + 5, 0, zo, xo + 7, 0, zo + 7);
            self.f(g, xo + 3, 0, zo, xo + 4, 0, zo + 2);
            self.f(g, xo + 3, 0, zo + 5, xo + 4, 0, zo + 7);
            self.f(l, xo + 3, 0, zo + 2, xo + 4, 0, zo + 2);
            self.f(l, xo + 3, 0, zo + 5, xo + 4, 0, zo + 5);
            self.f(l, xo + 2, 0, zo + 3, xo + 2, 0, zo + 4);
            self.f(l, xo + 5, 0, zo + 3, xo + 5, 0, zo + 4);
        }
    }
    fn chunk_intersects(&self, x0: i32, z0: i32, x1: i32, z1: i32) -> bool {
        let a = self.b.world_pos(x0, 0, z0);
        let c = self.b.world_pos(x1, 0, z1);
        self.b
            .chunk
            .intersects_xz(a.x.min(c.x), a.z.min(c.z), a.x.max(c.x), a.z.max(c.z))
    }
    fn elder(&mut self, x: i32, y: i32, z: i32) {
        let pos = self.b.world_pos(x, y, z);
        if self.b.chunk.is_inside(pos) {
            let mut nbt = Compound::new();
            nbt.insert("id", Tag::from("minecraft:elder_guardian"));
            let mut p = zenith_nbt::List::new();
            for v in [
                f64::from(pos.x) + 0.5,
                f64::from(pos.y),
                f64::from(pos.z) + 0.5,
            ] {
                p.push(Tag::Double(v));
            }
            nbt.insert("Pos", Tag::List(p));
            nbt.insert("PersistenceRequired", Tag::Byte(1));
            self.b.world.add_entity(nbt);
        }
    }
    /// L/G/L bands (`y = 1 / 2 / 3`) over the wall line `(x0, z0)..(x1, z1)`.
    fn band(&mut self, x0: i32, z0: i32, x1: i32, z1: i32) {
        let (l, g) = (self.l, self.g);
        self.f(l, x0, 1, z0, x1, 1, z1);
        self.f(g, x0, 2, z0, x1, 2, z1);
        self.f(l, x0, 3, z0, x1, 3, z1);
    }
}

fn open(rooms: &[RoomDef], slot: usize, d: usize) -> bool {
    rooms[slot].open[d]
}

fn conn(rooms: &[RoomDef], slot: usize, d: usize) -> Option<usize> {
    rooms[slot].conn[d]
}

/// `postProcess` of the monument building (and its child rooms).
pub fn post_process(piece: &mut StructurePiece, call: &mut PieceCall<'_>) {
    let bb = piece.bounding_box;
    let Some(dir) = piece.orientation else {
        return;
    };
    let chunk = *call.chunk_bb;
    let PieceData::Monument(data) = &mut piece.data else {
        return;
    };
    if !data.built {
        // regeneratePiecesAfterLoad
        let mut rng = LegacyRandom::new(0);
        set_large_feature_seed(
            &mut rng,
            call.world.world_seed(),
            call.start_chunk.0,
            call.start_chunk.1,
        );
        let _ = random_horizontal_direction(&mut rng);
        data.build(&mut rng, bb, dir);
    }
    let rooms = data.rooms.clone();
    let children = data.children.clone();
    let sea = call.world.sea_level();
    let mut b = PieceBuilder::new(&mut *call.world, bb, Some(dir), chunk);
    {
        let mut m = Mon::new(&mut b);
        let (g, l, d, sl) = (m.g, m.l, m.d, m.sl);
        let water_height = sea.max(64) - bb.min_y;
        m.water(0, 0, 0, 58, water_height, 58);
        for (flipped, xoff) in [(false, 0), (true, 33)] {
            if !m.chunk_intersects(xoff, 0, xoff + 23, 20) {
                continue;
            }
            m.f(g, xoff, 0, 0, xoff + 24, 0, 20);
            m.water(xoff, 1, 0, xoff + 24, 10, 20);
            for i in 0..4 {
                m.f(l, xoff + i, i + 1, i, xoff + i, i + 1, 20);
                m.f(l, xoff + i + 7, i + 5, i + 7, xoff + i + 7, i + 5, 20);
                m.f(l, xoff + 17 - i, i + 5, i + 7, xoff + 17 - i, i + 5, 20);
                m.f(l, xoff + 24 - i, i + 1, i, xoff + 24 - i, i + 1, 20);
                m.f(l, xoff + i + 1, i + 1, i, xoff + 23 - i, i + 1, i);
                m.f(l, xoff + i + 8, i + 5, i + 7, xoff + 16 - i, i + 5, i + 7);
            }
            m.f(g, xoff + 4, 4, 4, xoff + 6, 4, 20);
            m.f(g, xoff + 7, 4, 4, xoff + 17, 4, 6);
            m.f(g, xoff + 18, 4, 4, xoff + 20, 4, 20);
            m.f(g, xoff + 11, 8, 11, xoff + 13, 8, 20);
            for z in [12, 15, 18] {
                m.p(l, xoff + 12, 9, z);
            }
            let left_pos = xoff + if flipped { 19 } else { 5 };
            let right_pos = xoff + if flipped { 5 } else { 19 };
            for z in [20, 17, 14, 11, 8, 5] {
                m.p(l, left_pos, 5, z);
            }
            for z in [19, 16, 13, 10, 7] {
                m.p(l, right_pos, 5, z);
            }
            for i in 0..4 {
                let x = if flipped {
                    xoff + 24 - (17 - 3 * i)
                } else {
                    xoff + 17 - 3 * i
                };
                m.p(l, x, 5, 5);
            }
            m.p(l, right_pos, 5, 5);
            m.f(g, xoff + 11, 1, 12, xoff + 13, 7, 12);
            m.f(g, xoff + 12, 1, 11, xoff + 12, 7, 13);
        }
        if m.chunk_intersects(22, 5, 35, 17) {
            m.water(25, 0, 0, 32, 8, 20);
            for i in 0..4 {
                let z = 5 + i * 4;
                m.f(l, 24, 2, z, 24, 4, z);
                m.f(l, 22, 4, z, 23, 4, z);
                m.p(l, 25, 5, z);
                m.p(l, 26, 6, z);
                m.p(sl, 26, 5, z);
                m.f(l, 33, 2, z, 33, 4, z);
                m.f(l, 34, 4, z, 35, 4, z);
                m.p(l, 32, 5, z);
                m.p(l, 31, 6, z);
                m.p(sl, 31, 5, z);
                m.f(g, 27, 6, z, 30, 6, z);
            }
        }
        if m.chunk_intersects(15, 20, 42, 21) {
            for (x0, y0, x1, y1) in [
                (15, 0, 42, 0),
                (21, 12, 36, 12),
                (17, 11, 40, 11),
                (16, 10, 41, 10),
                (15, 7, 42, 9),
                (16, 6, 41, 6),
                (17, 5, 40, 5),
                (21, 4, 36, 4),
                (22, 3, 26, 3),
                (31, 3, 35, 3),
                (23, 2, 25, 2),
                (32, 2, 34, 2),
            ] {
                m.f(g, x0, y0, 21, x1, y1, 21);
            }
            m.water(26, 1, 21, 31, 3, 21);
            m.f(l, 28, 4, 20, 29, 4, 21);
            for (x, y) in [(27, 3), (30, 3), (26, 2), (31, 2), (25, 1), (32, 1)] {
                m.p(l, x, y, 21);
            }
            for i in 0..=6 {
                m.p(d, 28 - i, 6 + i, 21);
                m.p(d, 29 + i, 6 + i, 21);
            }
            for i in 0..=3 {
                m.p(d, 28 - i, 9 + i, 21);
                m.p(d, 29 + i, 9 + i, 21);
            }
            m.p(d, 28, 12, 21);
            m.p(d, 29, 12, 21);
            for i in 0..=2 {
                m.f(d, 22 - i * 2, 8, 21, 22 - i * 2, 9, 21);
                m.f(d, 35 + i * 2, 8, 21, 35 + i * 2, 9, 21);
            }
            for (x0, y0, x1, y1) in [
                (15, 13, 42, 15),
                (15, 1, 15, 6),
                (16, 1, 16, 5),
                (17, 1, 20, 4),
                (21, 1, 21, 3),
                (22, 1, 22, 2),
                (23, 1, 24, 1),
                (42, 1, 42, 6),
                (41, 1, 41, 5),
                (37, 1, 40, 4),
                (36, 1, 36, 3),
                (33, 1, 34, 1),
                (35, 1, 35, 2),
            ] {
                m.water(x0, y0, 21, x1, y1, 21);
            }
        }
        if m.chunk_intersects(21, 21, 36, 36) {
            m.f(g, 21, 0, 22, 36, 0, 36);
            m.water(21, 1, 22, 36, 23, 36);
            for i in 0..4 {
                let y = 13 + i;
                m.f(l, 21 + i, y, 21 + i, 36 - i, y, 21 + i);
                m.f(l, 21 + i, y, 36 - i, 36 - i, y, 36 - i);
                m.f(l, 21 + i, y, 22 + i, 21 + i, y, 35 - i);
                m.f(l, 36 - i, y, 22 + i, 36 - i, y, 35 - i);
            }
            m.f(g, 25, 16, 25, 32, 16, 32);
            for (x, z) in [(25, 25), (25, 32), (32, 25), (32, 32)] {
                m.f(l, x, 17, z, x, 19, z);
            }
            for (ax, az, bx, bz) in [
                (26, 26, 27, 27),
                (26, 31, 27, 30),
                (31, 31, 30, 30),
                (31, 26, 30, 27),
            ] {
                m.p(l, ax, 20, az);
                m.p(l, bx, 21, bz);
                m.p(sl, bx, 20, bz);
            }
            m.f(g, 28, 21, 27, 29, 21, 27);
            m.f(g, 27, 21, 28, 27, 21, 29);
            m.f(g, 28, 21, 30, 29, 21, 30);
            m.f(g, 30, 21, 28, 30, 21, 29);
        }
        // Lower wall.
        if m.chunk_intersects(0, 21, 6, 58) {
            m.f(g, 0, 0, 21, 6, 0, 57);
            m.water(0, 1, 21, 6, 7, 57);
            m.f(g, 4, 4, 21, 6, 4, 53);
            for i in 0..4 {
                m.f(l, i, i + 1, 21, i, i + 1, 57 - i);
            }
            let mut z = 23;
            while z <= 50 {
                m.p(l, 5, 5, z);
                z += 3;
            }
            m.p(l, 5, 5, 52);
            for i in 0..4 {
                m.f(l, i, i + 1, 21, i, i + 1, 57 - i);
            }
            m.f(g, 4, 1, 52, 6, 3, 52);
            m.f(g, 5, 1, 51, 5, 3, 53);
        }
        if m.chunk_intersects(51, 21, 58, 58) {
            m.f(g, 51, 0, 21, 57, 0, 57);
            m.water(51, 1, 21, 57, 7, 57);
            m.f(g, 51, 4, 21, 53, 4, 53);
            for i in 0..4 {
                m.f(l, 57 - i, i + 1, 21, 57 - i, i + 1, 57 - i);
            }
            let mut z = 23;
            while z <= 50 {
                m.p(l, 52, 5, z);
                z += 3;
            }
            m.p(l, 52, 5, 52);
            m.f(g, 51, 1, 52, 53, 3, 52);
            m.f(g, 52, 1, 51, 52, 3, 53);
        }
        if m.chunk_intersects(0, 51, 57, 57) {
            m.f(g, 7, 0, 51, 50, 0, 57);
            m.water(7, 1, 51, 50, 10, 57);
            for i in 0..4 {
                m.f(l, i + 1, i + 1, 57 - i, 56 - i, i + 1, 57 - i);
            }
        }
        // Middle wall.
        if m.chunk_intersects(7, 21, 13, 50) {
            m.f(g, 7, 0, 21, 13, 0, 50);
            m.water(7, 1, 21, 13, 10, 50);
            m.f(g, 11, 8, 21, 13, 8, 53);
            for i in 0..4 {
                m.f(l, i + 7, i + 5, 21, i + 7, i + 5, 54);
            }
            let mut z = 21;
            while z <= 45 {
                m.p(l, 12, 9, z);
                z += 3;
            }
        }
        if m.chunk_intersects(44, 21, 50, 54) {
            m.f(g, 44, 0, 21, 50, 0, 50);
            m.water(44, 1, 21, 50, 10, 50);
            m.f(g, 44, 8, 21, 46, 8, 53);
            for i in 0..4 {
                m.f(l, 50 - i, i + 5, 21, 50 - i, i + 5, 54);
            }
            let mut z = 21;
            while z <= 45 {
                m.p(l, 45, 9, z);
                z += 3;
            }
        }
        if m.chunk_intersects(8, 44, 49, 54) {
            m.f(g, 14, 0, 44, 43, 0, 50);
            m.water(14, 1, 44, 43, 10, 50);
            let mut x = 12;
            while x <= 45 {
                m.p(l, x, 9, 45);
                m.p(l, x, 9, 52);
                if matches!(x, 12 | 18 | 24 | 33 | 39 | 45) {
                    m.p(l, x, 9, 47);
                    m.p(l, x, 9, 50);
                    m.f(l, x, 10, 45, x, 10, 46);
                    m.f(l, x, 10, 51, x, 10, 52);
                    m.p(l, x, 11, 47);
                    m.p(l, x, 11, 50);
                    m.f(l, x, 12, 48, x, 12, 49);
                }
                x += 3;
            }
            for i in 0..3 {
                m.f(g, 8 + i, 5 + i, 54, 49 - i, 5 + i, 54);
            }
            m.f(l, 11, 8, 54, 46, 8, 54);
            m.f(g, 14, 8, 44, 43, 8, 53);
        }
        // Upper wall.
        if m.chunk_intersects(14, 21, 20, 43) {
            m.f(g, 14, 0, 21, 20, 0, 43);
            m.water(14, 1, 22, 20, 14, 43);
            m.f(g, 18, 12, 22, 20, 12, 39);
            m.f(l, 18, 12, 21, 20, 12, 21);
            for i in 0..4 {
                m.f(l, i + 14, i + 9, 21, i + 14, i + 9, 43 - i);
            }
            let mut z = 23;
            while z <= 39 {
                m.p(l, 19, 13, z);
                z += 3;
            }
        }
        if m.chunk_intersects(37, 21, 43, 43) {
            m.f(g, 37, 0, 21, 43, 0, 43);
            m.water(37, 1, 22, 43, 14, 43);
            m.f(g, 37, 12, 22, 39, 12, 39);
            m.f(l, 37, 12, 21, 39, 12, 21);
            for i in 0..4 {
                m.f(l, 43 - i, i + 9, 21, 43 - i, i + 9, 43 - i);
            }
            let mut z = 23;
            while z <= 39 {
                m.p(l, 38, 13, z);
                z += 3;
            }
        }
        if m.chunk_intersects(15, 37, 42, 43) {
            m.f(g, 21, 0, 37, 36, 0, 43);
            m.water(21, 1, 37, 36, 14, 43);
            m.f(g, 21, 12, 37, 36, 12, 39);
            for i in 0..4 {
                m.f(l, 15 + i, i + 9, 43 - i, 42 - i, i + 9, 43 - i);
            }
            let mut x = 21;
            while x <= 36 {
                m.p(l, x, 13, 38);
                x += 3;
            }
        }
        // Foundation pillars.
        for px in 0..7 {
            for pz in 0..7 {
                if !(px == 0 || px == 6 || pz == 0 || pz == 6) || (px == 3 && pz == 0) {
                    continue;
                }
                let (bx, bz) = (px * 9, pz * 9);
                for w in 0..4 {
                    for dd in 0..4 {
                        m.p(l, bx + w, 0, bz + dd);
                        m.b.fill_column_down(l, bx + w, -1, bz + dd);
                    }
                }
            }
        }
        for i in 0..=4 {
            m.water(-1 - i, 2 * i, -1 - i, -1 - i, 23, 58 + i);
            m.water(58 + i, 2 * i, -1 - i, 58 + i, 23, 58 + i);
            m.water(-i, 2 * i, -1 - i, 57 + i, 23, -1 - i);
            m.water(-i, 2 * i, 58 + i, 57 + i, 23, 58 + i);
        }
    }
    // Child pieces.
    for child in &children {
        if !child.bb.intersects(chunk) {
            continue;
        }
        let mut cb = PieceBuilder::new(&mut *b.world, child.bb, Some(dir), chunk);
        child_post_process(&mut cb, child, &rooms, &mut *call.random);
    }
}

fn child_post_process(
    b: &mut PieceBuilder<'_>,
    c: &MonumentChild,
    rooms: &[RoomDef],
    rng: &mut dyn WorldgenRandom,
) {
    let mut m = Mon::new(b);
    let (g, l, d, sl, w) = (m.g, m.l, m.d, m.sl, m.w);
    let s = c.room;
    let down_open = |slot: Option<usize>| slot.is_some_and(|x| open(rooms, x, D_DOWN));
    let idx_above0 = |slot: usize| rooms[slot].index / 25 > 0;
    match c.kind {
        RoomKind::Entry => {
            m.f(l, 0, 3, 0, 2, 3, 7);
            m.f(l, 5, 3, 0, 7, 3, 7);
            m.f(l, 0, 2, 0, 1, 2, 7);
            m.f(l, 6, 2, 0, 7, 2, 7);
            m.f(l, 0, 1, 0, 0, 1, 7);
            m.f(l, 7, 1, 0, 7, 1, 7);
            m.f(l, 0, 1, 7, 7, 3, 7);
            m.f(l, 1, 1, 0, 2, 3, 0);
            m.f(l, 5, 1, 0, 6, 3, 0);
            if open(rooms, s, D_NORTH) {
                m.water(3, 1, 7, 4, 2, 7);
            }
            if open(rooms, s, D_WEST) {
                m.water(0, 1, 3, 1, 2, 4);
            }
            if open(rooms, s, D_EAST) {
                m.water(6, 1, 3, 7, 2, 4);
            }
        }
        RoomKind::Core => {
            m.on_fill(g, 1, 8, 0, 14, 8, 14);
            m.f(l, 0, 7, 0, 0, 7, 15);
            m.f(l, 15, 7, 0, 15, 7, 15);
            m.f(l, 1, 7, 0, 15, 7, 0);
            m.f(l, 1, 7, 15, 14, 7, 15);
            for y in 1..=6 {
                let blk = if y == 2 || y == 6 { g } else { l };
                for x in [0, 15] {
                    m.f(blk, x, y, 0, x, y, 1);
                    m.f(blk, x, y, 6, x, y, 9);
                    m.f(blk, x, y, 14, x, y, 15);
                }
                for x in [1, 6, 7, 8, 9, 14] {
                    m.f(blk, x, y, 0, x, y, 0);
                }
                m.f(blk, 1, y, 15, 14, y, 15);
            }
            m.f(d, 6, 3, 6, 9, 6, 9);
            let gold = m.b.d("gold_block");
            m.f(gold, 7, 4, 7, 8, 5, 8);
            for x in [6, 9] {
                for y in [3, 6] {
                    for z in [6, 9] {
                        m.p(sl, x, y, z);
                    }
                }
            }
            for (x, z) in [(5, 6), (5, 9), (10, 6), (10, 9)] {
                m.f(l, x, 1, z, x, 2, z);
            }
            for (x, z) in [(6, 5), (9, 5), (6, 10), (9, 10)] {
                m.f(l, x, 1, z, x, 2, z);
            }
            for x in [5, 10] {
                for z in [5, 10] {
                    m.f(l, x, 2, z, x, 6, z);
                }
            }
            for x in [5, 10] {
                m.f(l, x, 7, 1, x, 7, 6);
                m.f(l, x, 7, 9, x, 7, 14);
            }
            for z in [5, 10] {
                m.f(l, 1, 7, z, 6, 7, z);
                m.f(l, 9, 7, z, 14, 7, z);
            }
            m.f(l, 2, 1, 2, 2, 1, 3);
            m.p(l, 3, 1, 2);
            m.f(l, 13, 1, 2, 13, 1, 3);
            m.p(l, 12, 1, 2);
            m.f(l, 2, 1, 12, 2, 1, 13);
            m.p(l, 3, 1, 13);
            m.f(l, 13, 1, 12, 13, 1, 13);
            m.p(l, 12, 1, 13);
        }
        RoomKind::DoubleX => {
            let east = conn(rooms, s, D_EAST);
            if idx_above0(s) {
                m.default_floor(8, 0, down_open(east));
                m.default_floor(0, 0, open(rooms, s, D_DOWN));
            }
            if conn(rooms, s, D_UP).is_none() {
                m.on_fill(g, 1, 4, 1, 7, 4, 6);
            }
            if east.is_some_and(|e| conn(rooms, e, D_UP).is_none()) {
                m.on_fill(g, 8, 4, 1, 14, 4, 6);
            }
            m.band(0, 0, 0, 7);
            m.band(15, 0, 15, 7);
            m.band(1, 0, 15, 0);
            m.band(1, 7, 14, 7);
            m.f(l, 5, 1, 0, 10, 1, 4);
            m.f(l, 5, 3, 0, 10, 3, 4);
            m.f(g, 6, 2, 0, 9, 2, 3);
            m.p(sl, 6, 2, 3);
            m.p(sl, 9, 2, 3);
            if open(rooms, s, D_SOUTH) {
                m.water(3, 1, 0, 4, 2, 0);
            }
            if open(rooms, s, D_NORTH) {
                m.water(3, 1, 7, 4, 2, 7);
            }
            if open(rooms, s, D_WEST) {
                m.water(0, 1, 3, 0, 2, 4);
            }
            if let Some(e) = east {
                if open(rooms, e, D_SOUTH) {
                    m.water(11, 1, 0, 12, 2, 0);
                }
                if open(rooms, e, D_NORTH) {
                    m.water(11, 1, 7, 12, 2, 7);
                }
                if open(rooms, e, D_EAST) {
                    m.water(15, 1, 3, 15, 2, 4);
                }
            }
        }
        RoomKind::DoubleY => {
            let above = conn(rooms, s, D_UP);
            if idx_above0(s) {
                m.default_floor(0, 0, open(rooms, s, D_DOWN));
            }
            if above.is_some_and(|a| conn(rooms, a, D_UP).is_none()) {
                m.on_fill(g, 1, 8, 1, 6, 8, 6);
            }
            m.f(l, 0, 4, 0, 0, 4, 7);
            m.f(l, 7, 4, 0, 7, 4, 7);
            m.f(l, 1, 4, 0, 6, 4, 0);
            m.f(l, 1, 4, 7, 6, 4, 7);
            for (x, z) in [(2, 1), (5, 1), (2, 5), (5, 5)] {
                m.f(l, x, 4, z, x, 4, z + 1);
            }
            m.p(l, 1, 4, 2);
            m.p(l, 6, 4, 2);
            m.p(l, 1, 4, 5);
            m.p(l, 6, 4, 5);
            for (y, slot) in [(1, Some(s)), (5, above)] {
                let Some(r) = slot else { continue };
                // South z=0, North z=7, West x=0, East x=7.
                for (dir, fixed_z, fixed_x) in [
                    (D_SOUTH, Some(0), None),
                    (D_NORTH, Some(7), None),
                    (D_WEST, None, Some(0)),
                    (D_EAST, None, Some(7)),
                ] {
                    if let Some(z) = fixed_z {
                        if open(rooms, r, dir) {
                            m.f(l, 2, y, z, 2, y + 2, z);
                            m.f(l, 5, y, z, 5, y + 2, z);
                            m.f(l, 3, y + 2, z, 4, y + 2, z);
                        } else {
                            m.f(l, 0, y, z, 7, y + 2, z);
                            m.f(g, 0, y + 1, z, 7, y + 1, z);
                        }
                    } else if let Some(x) = fixed_x {
                        if open(rooms, r, dir) {
                            m.f(l, x, y, 2, x, y + 2, 2);
                            m.f(l, x, y, 5, x, y + 2, 5);
                            m.f(l, x, y + 2, 3, x, y + 2, 4);
                        } else {
                            m.f(l, x, y, 0, x, y + 2, 7);
                            m.f(g, x, y + 1, 0, x, y + 1, 7);
                        }
                    }
                }
            }
        }
        RoomKind::DoubleZ => {
            let north = conn(rooms, s, D_NORTH);
            if idx_above0(s) {
                m.default_floor(0, 8, down_open(north));
                m.default_floor(0, 0, open(rooms, s, D_DOWN));
            }
            if conn(rooms, s, D_UP).is_none() {
                m.on_fill(g, 1, 4, 1, 6, 4, 7);
            }
            if north.is_some_and(|n| conn(rooms, n, D_UP).is_none()) {
                m.on_fill(g, 1, 4, 8, 6, 4, 14);
            }
            m.band(0, 0, 0, 15);
            m.band(7, 0, 7, 15);
            m.band(1, 0, 7, 0);
            m.band(1, 15, 6, 15);
            for x in [1, 6] {
                for y in [1, 3] {
                    m.f(l, x, y, 1, x, y, 2);
                    m.f(l, x, y, 13, x, y, 14);
                }
            }
            for x in [2, 5] {
                for z in [6, 9] {
                    m.f(l, x, 1, z, x, 3, z);
                    m.f(l, x, 2, 7, x, 2, 8);
                }
                m.p(sl, x, 2, 5);
                m.p(sl, x, 2, 10);
                m.p(l, x, 3, 5);
                m.p(l, x, 3, 10);
            }
            m.f(l, 3, 2, 6, 4, 2, 6);
            m.f(l, 3, 2, 9, 4, 2, 9);
            if open(rooms, s, D_SOUTH) {
                m.water(3, 1, 0, 4, 2, 0);
            }
            if open(rooms, s, D_EAST) {
                m.water(7, 1, 3, 7, 2, 4);
            }
            if open(rooms, s, D_WEST) {
                m.water(0, 1, 3, 0, 2, 4);
            }
            if let Some(n) = north {
                if open(rooms, n, D_NORTH) {
                    m.water(3, 1, 15, 4, 2, 15);
                }
                if open(rooms, n, D_WEST) {
                    m.water(0, 1, 11, 0, 2, 12);
                }
                if open(rooms, n, D_EAST) {
                    m.water(7, 1, 11, 7, 2, 12);
                }
            }
        }
        RoomKind::DoubleXY => {
            let east = conn(rooms, s, D_EAST);
            let up = conn(rooms, s, D_UP);
            let east_up = east.and_then(|e| conn(rooms, e, D_UP));
            if idx_above0(s) {
                m.default_floor(8, 0, down_open(east));
                m.default_floor(0, 0, open(rooms, s, D_DOWN));
            }
            if up.is_some_and(|u| conn(rooms, u, D_UP).is_none()) {
                m.on_fill(g, 1, 8, 1, 7, 8, 6);
            }
            if east_up.is_some_and(|u| conn(rooms, u, D_UP).is_none()) {
                m.on_fill(g, 8, 8, 1, 14, 8, 6);
            }
            for y in 1..=7 {
                let blk = if y == 2 || y == 6 { g } else { l };
                m.f(blk, 0, y, 0, 0, y, 7);
                m.f(blk, 15, y, 0, 15, y, 7);
                m.f(blk, 1, y, 0, 15, y, 0);
                m.f(blk, 1, y, 7, 14, y, 7);
            }
            m.f(l, 2, 1, 3, 2, 7, 4);
            m.f(l, 3, 1, 2, 4, 7, 2);
            m.f(l, 3, 1, 5, 4, 7, 5);
            m.f(l, 13, 1, 3, 13, 7, 4);
            m.f(l, 11, 1, 2, 12, 7, 2);
            m.f(l, 11, 1, 5, 12, 7, 5);
            m.f(l, 5, 1, 3, 5, 3, 4);
            m.f(l, 10, 1, 3, 10, 3, 4);
            m.f(l, 5, 7, 2, 10, 7, 5);
            for x in [5, 10] {
                for z in [2, 5] {
                    m.f(l, x, 5, z, x, 7, z);
                }
            }
            for x in [6, 9] {
                for z in [2, 5] {
                    m.p(l, x, 6, z);
                }
            }
            m.f(l, 5, 4, 3, 6, 4, 4);
            m.f(l, 9, 4, 3, 10, 4, 4);
            for x in [5, 10] {
                for z in [2, 5] {
                    m.p(sl, x, 4, z);
                }
            }
            let doors = |m: &mut Mon<'_, '_>, slot: Option<usize>, y: i32, east_side: bool| {
                let Some(r) = slot else { return };
                if open(rooms, r, D_SOUTH) {
                    let (x0, x1) = if east_side { (11, 12) } else { (3, 4) };
                    m.water(x0, y, 0, x1, y + 1, 0);
                }
                if open(rooms, r, D_NORTH) {
                    let (x0, x1) = if east_side { (11, 12) } else { (3, 4) };
                    m.water(x0, y, 7, x1, y + 1, 7);
                }
                if !east_side && open(rooms, r, D_WEST) {
                    m.water(0, y, 3, 0, y + 1, 4);
                }
                if east_side && open(rooms, r, D_EAST) {
                    m.water(15, y, 3, 15, y + 1, 4);
                }
            };
            doors(&mut m, Some(s), 1, false);
            doors(&mut m, east, 1, true);
            doors(&mut m, up, 5, false);
            doors(&mut m, east_up, 5, true);
        }
        RoomKind::DoubleYZ => {
            let north = conn(rooms, s, D_NORTH);
            let up = conn(rooms, s, D_UP);
            let north_up = north.and_then(|n| conn(rooms, n, D_UP));
            if idx_above0(s) {
                m.default_floor(0, 8, down_open(north));
                m.default_floor(0, 0, open(rooms, s, D_DOWN));
            }
            if up.is_some_and(|u| conn(rooms, u, D_UP).is_none()) {
                m.on_fill(g, 1, 8, 1, 6, 8, 7);
            }
            if north_up.is_some_and(|u| conn(rooms, u, D_UP).is_none()) {
                m.on_fill(g, 1, 8, 8, 6, 8, 14);
            }
            for y in 1..=7 {
                let blk = if y == 2 || y == 6 { g } else { l };
                m.f(blk, 0, y, 0, 0, y, 15);
                m.f(blk, 7, y, 0, 7, y, 15);
                m.f(blk, 1, y, 0, 6, y, 0);
                m.f(blk, 1, y, 15, 6, y, 15);
            }
            for y in 1..=7 {
                let blk = if y == 2 || y == 6 { sl } else { d };
                m.f(blk, 3, y, 7, 4, y, 8);
            }
            if open(rooms, s, D_SOUTH) {
                m.water(3, 1, 0, 4, 2, 0);
            }
            if open(rooms, s, D_EAST) {
                m.water(7, 1, 3, 7, 2, 4);
            }
            if open(rooms, s, D_WEST) {
                m.water(0, 1, 3, 0, 2, 4);
            }
            if let Some(n) = north {
                if open(rooms, n, D_NORTH) {
                    m.water(3, 1, 15, 4, 2, 15);
                }
                if open(rooms, n, D_WEST) {
                    m.water(0, 1, 11, 0, 2, 12);
                }
                if open(rooms, n, D_EAST) {
                    m.water(7, 1, 11, 7, 2, 12);
                }
            }
            if let Some(u) = up {
                if open(rooms, u, D_SOUTH) {
                    m.water(3, 5, 0, 4, 6, 0);
                }
                if open(rooms, u, D_EAST) {
                    m.water(7, 5, 3, 7, 6, 4);
                    m.f(l, 5, 4, 2, 6, 4, 5);
                    m.f(l, 6, 1, 2, 6, 3, 2);
                    m.f(l, 6, 1, 5, 6, 3, 5);
                }
                if open(rooms, u, D_WEST) {
                    m.water(0, 5, 3, 0, 6, 4);
                    m.f(l, 1, 4, 2, 2, 4, 5);
                    m.f(l, 1, 1, 2, 1, 3, 2);
                    m.f(l, 1, 1, 5, 1, 3, 5);
                }
            }
            if let Some(nu) = north_up {
                if open(rooms, nu, D_NORTH) {
                    m.water(3, 5, 15, 4, 6, 15);
                }
                if open(rooms, nu, D_WEST) {
                    m.water(0, 5, 11, 0, 6, 12);
                    m.f(l, 1, 4, 10, 2, 4, 13);
                    m.f(l, 1, 1, 10, 1, 3, 10);
                    m.f(l, 1, 1, 13, 1, 3, 13);
                }
                if open(rooms, nu, D_EAST) {
                    m.water(7, 5, 11, 7, 6, 12);
                    m.f(l, 5, 4, 10, 6, 4, 13);
                    m.f(l, 6, 1, 10, 6, 3, 10);
                    m.f(l, 6, 1, 13, 6, 3, 13);
                }
            }
        }
        RoomKind::Simple { main_design } => {
            if idx_above0(s) {
                m.default_floor(0, 0, open(rooms, s, D_DOWN));
            }
            if conn(rooms, s, D_UP).is_none() {
                m.on_fill(g, 1, 4, 1, 6, 4, 6);
            }
            let center_pillar = main_design != 0
                && rng.next_boolean()
                && !open(rooms, s, D_DOWN)
                && !open(rooms, s, D_UP)
                && rooms[s].count_openings() > 1;
            match main_design {
                0 => {
                    for (x0, z0, x1, z1) in [(0, 0, 2, 2), (5, 0, 7, 2), (0, 5, 2, 7), (5, 5, 7, 7)]
                    {
                        m.f(l, x0, 1, z0, x1, 1, z1);
                        m.f(l, x0, 3, z0, x1, 3, z1);
                    }
                    m.f(g, 0, 2, 0, 0, 2, 2);
                    m.f(g, 1, 2, 0, 2, 2, 0);
                    m.f(g, 7, 2, 0, 7, 2, 2);
                    m.f(g, 5, 2, 0, 6, 2, 0);
                    m.f(g, 0, 2, 5, 0, 2, 7);
                    m.f(g, 1, 2, 7, 2, 2, 7);
                    m.f(g, 7, 2, 5, 7, 2, 7);
                    m.f(g, 5, 2, 7, 6, 2, 7);
                    for (x, z) in [(1, 1), (6, 1), (1, 6), (6, 6)] {
                        m.p(sl, x, 2, z);
                    }
                    for (dir, line) in [
                        (D_SOUTH, (3, 0, 4, 0)),
                        (D_NORTH, (3, 7, 4, 7)),
                        (D_WEST, (0, 3, 0, 4)),
                        (D_EAST, (7, 3, 7, 4)),
                    ] {
                        let (x0, z0, x1, z1) = line;
                        if open(rooms, s, dir) {
                            m.f(l, x0, 3, z0, x1, 3, z1);
                        } else {
                            m.f(l, x0, 1, z0, x1, 1, z1);
                            m.f(l, x0, 3, z0, x1, 3, z1);
                            m.f(g, x0, 2, z0, x1, 2, z1);
                        }
                    }
                }
                1 => {
                    for x in [2, 5] {
                        for z in [2, 5] {
                            m.f(l, x, 1, z, x, 3, z);
                            m.p(sl, x, 2, z);
                        }
                    }
                    for (x0, z0, x1, z1) in [
                        (0, 0, 1, 0),
                        (0, 1, 0, 1),
                        (6, 0, 7, 0),
                        (7, 1, 7, 1),
                        (0, 7, 1, 7),
                        (0, 6, 0, 6),
                        (6, 7, 7, 7),
                        (7, 6, 7, 6),
                    ] {
                        m.f(l, x0, 1, z0, x1, 3, z1);
                    }
                    for (x, z) in [
                        (1, 0),
                        (0, 1),
                        (1, 7),
                        (0, 6),
                        (6, 7),
                        (7, 6),
                        (6, 0),
                        (7, 1),
                    ] {
                        m.p(g, x, 2, z);
                    }
                    for (dir, line) in [
                        (D_SOUTH, (1, 0, 6, 0)),
                        (D_NORTH, (1, 7, 6, 7)),
                        (D_WEST, (0, 1, 0, 6)),
                        (D_EAST, (7, 1, 7, 6)),
                    ] {
                        if !open(rooms, s, dir) {
                            let (x0, z0, x1, z1) = line;
                            m.f(l, x0, 3, z0, x1, 3, z1);
                            m.f(g, x0, 2, z0, x1, 2, z1);
                            m.f(l, x0, 1, z0, x1, 1, z1);
                        }
                    }
                }
                _ => {
                    m.band(0, 0, 7, 0);
                    m.band(0, 7, 7, 7);
                    m.band(0, 0, 0, 7);
                    m.band(7, 0, 7, 7);
                    m.f(d, 3, 1, 0, 4, 2, 0);
                    m.f(d, 3, 1, 7, 4, 2, 7);
                    m.f(d, 0, 1, 3, 0, 2, 4);
                    m.f(d, 7, 1, 3, 7, 2, 4);
                    if open(rooms, s, D_SOUTH) {
                        m.water(3, 1, 0, 4, 2, 0);
                    }
                    if open(rooms, s, D_NORTH) {
                        m.water(3, 1, 7, 4, 2, 7);
                    }
                    if open(rooms, s, D_WEST) {
                        m.water(0, 1, 3, 0, 2, 4);
                    }
                    if open(rooms, s, D_EAST) {
                        m.water(7, 1, 3, 7, 2, 4);
                    }
                }
            }
            if center_pillar {
                m.f(l, 3, 1, 3, 4, 1, 4);
                m.f(g, 3, 2, 3, 4, 2, 4);
                m.f(l, 3, 3, 3, 4, 3, 4);
            }
        }
        RoomKind::SimpleTop => {
            if idx_above0(s) {
                m.default_floor(0, 0, open(rooms, s, D_DOWN));
            }
            if conn(rooms, s, D_UP).is_none() {
                m.on_fill(g, 1, 4, 1, 6, 4, 6);
            }
            let sponge = m.b.d("wet_sponge");
            for x in 1..=6 {
                for z in 1..=6 {
                    if rng.next_int_bounded(3) != 0 {
                        let y0 = 2 + if rng.next_int_bounded(4) == 0 { 0 } else { 1 };
                        m.f(sponge, x, y0, z, x, 3, z);
                    }
                }
            }
            m.band(0, 0, 7, 0);
            m.band(0, 7, 7, 7);
            m.band(0, 0, 0, 7);
            m.band(7, 0, 7, 7);
            m.f(d, 3, 1, 0, 4, 2, 0);
            m.f(d, 3, 1, 7, 4, 2, 7);
            m.f(d, 0, 1, 3, 0, 2, 4);
            m.f(d, 7, 1, 3, 7, 2, 4);
            if open(rooms, s, D_SOUTH) {
                m.water(3, 1, 0, 4, 2, 0);
            }
        }
        RoomKind::Wing { main_design } => {
            if main_design == 0 {
                for i in 0..4 {
                    m.f(l, 10 - i, 3 - i, 20 - i, 12 + i, 3 - i, 20);
                }
                for (x0, y0, z0, x1, y1, z1) in [
                    (7, 0, 6, 15, 0, 16),
                    (6, 0, 6, 6, 3, 20),
                    (16, 0, 6, 16, 3, 20),
                    (7, 1, 7, 7, 1, 20),
                    (15, 1, 7, 15, 1, 20),
                    (7, 1, 6, 9, 3, 6),
                    (13, 1, 6, 15, 3, 6),
                    (8, 1, 7, 9, 1, 7),
                    (13, 1, 7, 14, 1, 7),
                    (9, 0, 5, 13, 0, 5),
                ] {
                    m.f(l, x0, y0, z0, x1, y1, z1);
                }
                m.f(d, 10, 0, 7, 12, 0, 7);
                m.f(d, 8, 0, 10, 8, 0, 12);
                m.f(d, 14, 0, 10, 14, 0, 12);
                for z in [18, 15, 12, 9] {
                    m.p(sl, 6, 3, z);
                    m.p(sl, 16, 3, z);
                }
                for (x, z) in [(10, 10), (10, 12), (12, 10), (12, 12)] {
                    m.p(sl, x, 0, z);
                }
                m.p(sl, 8, 3, 6);
                m.p(sl, 14, 3, 6);
                for x in [4, 18] {
                    for z in [4, 18] {
                        m.p(l, x, 0, z);
                        m.p(sl, x, 1, z);
                        m.p(l, x, 2, z);
                    }
                }
                m.p(l, 9, 7, 20);
                m.p(l, 13, 7, 20);
                m.f(l, 6, 0, 21, 7, 4, 21);
                m.f(l, 15, 0, 21, 16, 4, 21);
                m.elder(11, 2, 16);
            } else {
                m.f(l, 9, 3, 18, 13, 3, 20);
                for x in [9, 13] {
                    m.f(l, x, 0, 18, x, 2, 18);
                }
                for x in [9, 13] {
                    m.p(l, x, 4, 20);
                    m.p(sl, x, 5, 20);
                    m.p(l, x, 6, 20);
                }
                m.f(l, 7, 3, 7, 15, 3, 14);
                for x in [10, 12] {
                    for z in [10, 12] {
                        m.f(l, x, 0, z, x, 6, z);
                        m.p(sl, x, 0, z);
                        m.p(sl, x, 4, z);
                    }
                }
                for x in [8, 14] {
                    for z in [7, 14] {
                        m.f(l, x, 0, z, x, 2, z);
                    }
                    m.f(d, x, 3, 8, x, 3, 13);
                }
                m.elder(11, 5, 13);
            }
        }
        RoomKind::Penthouse => {
            m.f(l, 2, -1, 2, 11, -1, 11);
            m.f(g, 0, -1, 0, 1, -1, 11);
            m.f(g, 12, -1, 0, 13, -1, 11);
            m.f(g, 2, -1, 0, 11, -1, 1);
            m.f(g, 2, -1, 12, 11, -1, 13);
            m.f(l, 0, 0, 0, 0, 0, 13);
            m.f(l, 13, 0, 0, 13, 0, 13);
            m.f(l, 1, 0, 0, 12, 0, 0);
            m.f(l, 1, 0, 13, 12, 0, 13);
            for i in [2, 5, 8, 11] {
                m.p(sl, 0, 0, i);
                m.p(sl, 13, 0, i);
                m.p(sl, i, 0, 0);
            }
            m.f(l, 2, 0, 3, 4, 0, 9);
            m.f(l, 9, 0, 3, 11, 0, 9);
            m.f(l, 4, 0, 9, 9, 0, 11);
            for (x, z) in [(5, 8), (8, 8), (10, 10), (3, 10)] {
                m.p(l, x, 0, z);
            }
            m.f(d, 3, 0, 3, 3, 0, 7);
            m.f(d, 10, 0, 3, 10, 0, 7);
            m.f(d, 6, 0, 10, 7, 0, 10);
            for x in [3, 10] {
                for z in [2, 5, 8] {
                    m.f(l, x, 0, z, x, 2, z);
                }
            }
            m.f(l, 5, 0, 10, 5, 2, 10);
            m.f(l, 8, 0, 10, 8, 2, 10);
            m.f(d, 6, -1, 7, 7, -1, 8);
            m.water(6, -1, 3, 7, -1, 4);
            m.elder(6, 1, 6);
        }
    }
    let _ = w;
}
