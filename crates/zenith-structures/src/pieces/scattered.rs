//! Desert pyramid (§7), jungle temple (§8), swamp hut (§9) and buried treasure (§13)
//! placement (`docs/specs/worldgen/structures-legacy-pieces.md`).

use zenith_math::{LegacyRandom, PositionalRandomFactory};
use zenith_nbt::{Compound, Tag};
use zenith_worldgen::HeightmapKind;

use crate::generator::StructurePiece;
use crate::geom::{BlockPos, BoundingBox, BoundingBoxExt, Direction, dir_2d_data_value, dir_step};
use crate::piece::{PieceBuilder, set_loot, spawn_entity};
use crate::piece_data::PieceData;
use crate::place::PieceCall;
use crate::world::{StructureWorld, WorldgenRandom, rng_shuffle};

/// `ScatteredFeaturePiece.updateAverageGroundHeight`.
fn update_average_ground_height(
    world: &dyn StructureWorld,
    bb: &mut BoundingBox,
    chunk: &BoundingBox,
    height_position: &mut i32,
    offset: i32,
) -> bool {
    if *height_position >= 0 {
        return true;
    }
    let (mut total, mut count) = (0i32, 0i32);
    for z in bb.min_z..=bb.max_z {
        for x in bb.min_x..=bb.max_x {
            if chunk.is_inside(BlockPos::new(x, 64, z)) {
                total += world.get_height(HeightmapKind::MotionBlockingNoLeaves, x, z);
                count += 1;
            }
        }
    }
    if count == 0 {
        return false;
    }
    *height_position = total / count;
    *bb = bb.moved(0, *height_position - bb.min_y + offset, 0);
    true
}

/// `ScatteredFeaturePiece.updateHeightPositionToLowestGroundHeight`.
fn update_lowest_ground_height(
    world: &dyn StructureWorld,
    bb: &mut BoundingBox,
    height_position: &mut i32,
    offset: i32,
) -> bool {
    if *height_position >= 0 {
        return true;
    }
    let mut lowest = i32::MAX;
    for z in bb.min_z..=bb.max_z {
        for x in bb.min_x..=bb.max_x {
            lowest = lowest.min(world.get_height(HeightmapKind::MotionBlockingNoLeaves, x, z));
        }
    }
    *height_position = lowest;
    *bb = bb.moved(0, *height_position - bb.min_y + offset, 0);
    true
}

/// `RandomSource.createThreadLocalInstance(seed).forkPositional().at(pos)`.
pub fn thread_local_positional(seed: i64, pos: BlockPos) -> LegacyRandom {
    use zenith_math::RandomSource as _;
    LegacyRandom::new(seed)
        .fork_positional()
        .at(pos.x, pos.y, pos.z)
}

// ---------------------------------------------------------------------------
// Desert pyramid
// ---------------------------------------------------------------------------

/// `DesertPyramidPiece.postProcess`.
pub fn desert_pyramid(piece: &mut StructurePiece, call: &mut PieceCall<'_>) {
    let PieceData::DesertPyramid(st) = &mut piece.data else {
        return;
    };
    // The argument is evaluated before the `heightPosition >= 0` check.
    let offset = -call.random.next_int_bounded(3);
    let mut bb = piece.bounding_box;
    if !update_lowest_ground_height(&*call.world, &mut bb, &mut st.height_position, offset) {
        return;
    }
    piece.bounding_box = bb;
    let chunk = *call.chunk_bb;
    let mut b = PieceBuilder::new(&mut *call.world, bb, piece.orientation, chunk);
    let rng = &mut *call.random;

    let ss = b.d("sandstone");
    let css = b.d("cut_sandstone");
    let hss = b.d("chiseled_sandstone");
    let ot = b.d("orange_terracotta");
    let bt = b.d("blue_terracotta");
    let air = b.air();
    let stairs = |b: &PieceBuilder<'_>, f: &str| b.s("sandstone_stairs", &[("facing", f)]);
    let (sn, ss_s, se, sw) = (
        stairs(&b, "north"),
        stairs(&b, "south"),
        stairs(&b, "east"),
        stairs(&b, "west"),
    );

    b.gen_box(0, -4, 0, 20, 0, 20, ss, ss, false);
    for p in 1..=9 {
        b.gen_box(p, p, p, 20 - p, p, 20 - p, ss, ss, false);
        b.gen_box(p + 1, p, p + 1, 19 - p, p, 19 - p, air, air, false);
    }
    for x in 0..=20 {
        for z in 0..=20 {
            b.fill_column_down(ss, x, -5, z);
        }
    }
    // Twin towers.
    b.gen_box(0, 0, 0, 4, 9, 4, ss, air, false);
    b.gen_box(1, 10, 1, 3, 10, 3, ss, ss, false);
    b.place_block(sn, 2, 10, 0);
    b.place_block(ss_s, 2, 10, 4);
    b.place_block(se, 0, 10, 2);
    b.place_block(sw, 4, 10, 2);
    b.gen_box(16, 0, 0, 20, 9, 4, ss, air, false);
    b.gen_box(17, 10, 1, 19, 10, 3, ss, ss, false);
    b.place_block(sn, 18, 10, 0);
    b.place_block(ss_s, 18, 10, 4);
    b.place_block(se, 16, 10, 2);
    b.place_block(sw, 20, 10, 2);
    // Entrance and halls.
    b.gen_box(8, 0, 0, 12, 4, 4, ss, air, false);
    b.air_box(9, 1, 0, 11, 3, 4);
    for (x, y) in [(9, 1), (9, 2), (9, 3), (10, 3), (11, 1), (11, 2), (11, 3)] {
        b.place_block(css, x, y, 1);
    }
    b.gen_box(4, 1, 1, 8, 3, 3, ss, air, false);
    b.air_box(4, 1, 2, 8, 2, 2);
    b.gen_box(12, 1, 1, 16, 3, 3, ss, air, false);
    b.air_box(12, 1, 2, 16, 2, 2);
    // Interior.
    b.gen_box(5, 4, 5, 15, 4, 15, ss, ss, false);
    b.air_box(9, 4, 9, 11, 4, 11);
    for (x, z) in [(8, 8), (12, 8), (8, 12), (12, 12)] {
        b.fill(x, 1, z, x, 3, z, css);
    }
    b.fill(1, 1, 5, 4, 4, 11, ss);
    b.fill(16, 1, 5, 19, 4, 11, ss);
    b.fill(6, 7, 9, 6, 7, 11, ss);
    b.fill(14, 7, 9, 14, 7, 11, ss);
    b.fill(5, 5, 9, 5, 7, 11, css);
    b.fill(15, 5, 9, 15, 7, 11, css);
    for (x, y, z) in [
        (5, 5, 10),
        (5, 6, 10),
        (6, 6, 10),
        (15, 5, 10),
        (15, 6, 10),
        (14, 6, 10),
    ] {
        b.place_block(air, x, y, z);
    }
    b.air_box(2, 4, 4, 2, 6, 4);
    b.air_box(18, 4, 4, 18, 6, 4);
    b.place_block(sn, 2, 4, 5);
    b.place_block(sn, 2, 3, 4);
    b.place_block(sn, 18, 4, 5);
    b.place_block(sn, 18, 3, 4);
    b.fill(1, 1, 3, 2, 2, 3, ss);
    b.fill(18, 1, 3, 19, 2, 3, ss);
    b.place_block(ss, 1, 1, 2);
    b.place_block(ss, 19, 1, 2);
    let slab = b.d("sandstone_slab");
    b.place_block(slab, 1, 2, 2);
    b.place_block(slab, 19, 2, 2);
    b.place_block(sw, 2, 1, 2);
    b.place_block(se, 18, 1, 2);
    b.fill(4, 3, 5, 4, 3, 17, ss);
    b.fill(16, 3, 5, 16, 3, 17, ss);
    b.air_box(3, 1, 5, 4, 2, 16);
    b.air_box(15, 1, 5, 16, 2, 16);
    let mut z = 5;
    while z <= 17 {
        b.place_block(css, 4, 1, z);
        b.place_block(hss, 4, 2, z);
        b.place_block(css, 16, 1, z);
        b.place_block(hss, 16, 2, z);
        z += 2;
    }
    // Floor mosaic.
    b.place_block(bt, 10, 0, 10);
    for (x, z) in [
        (10, 7),
        (10, 8),
        (9, 9),
        (11, 9),
        (8, 10),
        (12, 10),
        (7, 10),
        (13, 10),
        (9, 11),
        (11, 11),
        (10, 12),
        (10, 13),
    ] {
        b.place_block(ot, x, 0, z);
    }
    // Ankh facades.
    let row = |y: i32| -> [u32; 3] {
        match y {
            2 | 3 | 5 => [css, ot, css],
            4 | 6 => [ot, hss, ot],
            7 => [ot, ot, ot],
            _ => [css, css, css],
        }
    };
    for x in [0, 20] {
        for y in 2..=8 {
            let r = row(y);
            for (i, z) in (1..=3).enumerate() {
                b.place_block(r[i], x, y, z);
            }
        }
    }
    for x in [2, 18] {
        for y in 2..=8 {
            let r = row(y);
            for (i, xx) in (x - 1..=x + 1).enumerate() {
                b.place_block(r[i], xx, y, 0);
            }
        }
    }
    b.fill(8, 4, 0, 12, 6, 0, css);
    b.place_block(air, 8, 6, 0);
    b.place_block(air, 12, 6, 0);
    b.place_block(ot, 9, 5, 0);
    b.place_block(ot, 11, 5, 0);
    b.place_block(hss, 10, 5, 0);
    // Treasure chamber.
    b.fill(8, -14, 8, 12, -11, 12, css);
    b.fill(8, -10, 8, 12, -10, 12, hss);
    b.fill(8, -9, 8, 12, -9, 12, css);
    b.fill(8, -8, 8, 12, -1, 12, ss);
    b.air_box(9, -11, 9, 11, -1, 11);
    let plate = b.d("stone_pressure_plate");
    b.place_block(plate, 10, -11, 10);
    let tnt = b.d("tnt");
    b.gen_box(9, -13, 9, 11, -13, 11, tnt, air, false);
    for ((ix, iz), (ox, oz)) in [
        ((8, 10), (7, 10)),
        ((12, 10), (13, 10)),
        ((10, 8), (10, 7)),
        ((10, 12), (10, 13)),
    ] {
        b.place_block(air, ix, -11, iz);
        b.place_block(air, ix, -10, iz);
        b.place_block(hss, ox, -10, oz);
        b.place_block(css, ox, -11, oz);
    }
    for dir in [
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
    ] {
        let idx = dir_2d_data_value(dir) as usize;
        if !st.has_placed_chest[idx] {
            let (dx, _, dz) = dir_step(dir);
            st.has_placed_chest[idx] = b.create_chest(
                rng,
                10 + 2 * dx,
                -11,
                10 + 2 * dz,
                "minecraft:chests/desert_pyramid",
            );
        }
    }
    // Archaeology cellar.
    let sand = b.d("sand");
    let stairs_w = sw;
    for (x, y, z) in [(13, -1, 17), (14, -2, 17), (15, -3, 17)] {
        b.place_block(stairs_w, x, y, z);
    }
    let variant = b.world.level_random().next_boolean();
    for x in 12..=16 {
        b.place_block(sand, x, 0, 17);
    }
    for (x, y) in [(14, -1), (15, -2), (16, -3)] {
        b.place_block(sand, x, y, 17);
    }
    b.place_block(ss, 16, -2, 17);
    b.place_block(if variant { sand } else { ss }, 15, -1, 17);
    b.place_block(if !variant { sand } else { ss }, 16, -1, 17);
    for (y, blk) in [(-3, css), (-2, hss), (-1, css)] {
        b.gen_box(13, y, 10, 13, y, 15, blk, blk, true);
        b.gen_box(19, y, 10, 19, y, 15, blk, blk, true);
        b.gen_box(13, y, 10, 19, y, 11, blk, blk, true);
        b.gen_box(13, y, 16, 19, y, 16, blk, blk, true);
    }
    // placeSandBox(14, -3, 11, 18, -1, 15)
    for y in -3..=-1 {
        for x in 14..=18 {
            for z in 11..=15 {
                let p = b.world_pos(x, y, z);
                st.potential_sand.push(p);
            }
        }
    }
    // placeCollapsedRoof(14, 0, 11, 18, 15)
    for rx in 14..=18 {
        for rz in 11..=15 {
            let f = b.world.level_random().next_float();
            b.place_block(if f < 0.33 { ss } else { sand }, rx, 0, rz);
        }
    }
    let seed = b.world.world_seed();
    let origin = b.world_pos(14, 0, 11);
    let mut r = thread_local_positional(seed, origin);
    let rpx = r.next_int_bounded(5) + 14;
    let rpz = r.next_int_bounded(5) + 11;
    st.collapsed_roof_pos = b.world_pos(rpx, 0, rpz);
    b.place_block(bt, 16, -4, 13);
    for (x, z) in [
        (17, 12),
        (17, 14),
        (15, 12),
        (15, 14),
        (18, 13),
        (14, 13),
        (16, 15),
        (16, 11),
        (19, 13),
        (13, 13),
        (16, 16),
        (16, 10),
    ] {
        b.place_block(ot, x, -4, z);
    }
    for (x, z, wall) in [
        (19, 13, Some((20, 13))),
        (13, 13, Some((12, 13))),
        (16, 16, None),
        (16, 10, Some((16, 9))),
    ] {
        for y in [-3, -2] {
            let p = b.world_pos(x, y, z);
            st.potential_sand.push(p);
        }
        if let Some((wx, wz)) = wall {
            b.place_block(css, wx, -3, wz);
            b.place_block(hss, wx, -2, wz);
        }
    }
}

/// `DesertPyramidStructure.afterPlace` (suspicious sand).
pub fn desert_after_place(
    pieces: &[StructurePiece],
    world: &mut dyn StructureWorld,
    chunk: &BoundingBox,
) {
    let mut positions: Vec<BlockPos> = Vec::new();
    let mut roofs: Vec<BlockPos> = Vec::new();
    for p in pieces {
        if let PieceData::DesertPyramid(st) = &p.data {
            positions.extend(st.potential_sand.iter().copied());
            roofs.push(st.collapsed_roof_pos);
        }
    }
    if roofs.is_empty() {
        return;
    }
    let susp = crate::block_xform::default_state(world.blocks(), "suspicious_sand").unwrap_or(0);
    let sand = crate::block_xform::default_state(world.blocks(), "sand").unwrap_or(0);
    let place_susp = |world: &mut dyn StructureWorld, pos: BlockPos| {
        world.set_block_state(pos, susp, 2);
        set_loot(
            world,
            pos,
            "minecraft:archaeology/desert_pyramid",
            pos_as_long(pos),
        );
    };
    for r in roofs {
        if chunk.is_inside(r) {
            place_susp(world, r);
        }
    }
    positions.sort_by(|a, b| a.cmp_vec3i(*b));
    positions.dedup();
    let mut bb = pieces[0].bounding_box;
    for p in &pieces[1..] {
        bb.encapsulate(p.bounding_box);
    }
    let c = bb.center();
    let mut pos_rng = thread_local_positional(world.world_seed(), c);
    rng_shuffle(&mut positions, &mut pos_rng);
    let count = (positions.len() as i32).min(5 + pos_rng.next_int_bounded(3)) as usize;
    for (i, pos) in positions.iter().enumerate() {
        if !chunk.is_inside(*pos) {
            continue;
        }
        if i < count {
            place_susp(world, *pos);
        } else {
            world.set_block_state(*pos, sand, 2);
        }
    }
}

fn pos_as_long(p: BlockPos) -> i64 {
    p.as_long()
}

// ---------------------------------------------------------------------------
// Jungle temple
// ---------------------------------------------------------------------------

/// `JungleTemplePiece.postProcess`.
pub fn jungle_temple(piece: &mut StructurePiece, call: &mut PieceCall<'_>) {
    let PieceData::JungleTemple(st) = &mut piece.data else {
        return;
    };
    let mut bb = piece.bounding_box;
    let chunk = *call.chunk_bb;
    if !update_average_ground_height(&*call.world, &mut bb, &chunk, &mut st.height_position, 0) {
        return;
    }
    piece.bounding_box = bb;
    let mut b = PieceBuilder::new(&mut *call.world, bb, piece.orientation, chunk);
    let rng = &mut *call.random;
    let cobble = b.d("cobblestone");
    let mossy = b.d("mossy_cobblestone");
    let air = b.air();
    let mut sel = move |r: &mut dyn WorldgenRandom, _x: i32, _y: i32, _z: i32, _e: bool| {
        if r.next_float() < 0.4 { cobble } else { mossy }
    };
    macro_rules! stone {
        ($x0:expr, $y0:expr, $z0:expr, $x1:expr, $y1:expr, $z1:expr) => {
            b.gen_box_sel($x0, $y0, $z0, $x1, $y1, $z1, false, rng, &mut sel)
        };
    }
    stone!(0, -4, 0, 11, 0, 14);
    stone!(2, 1, 2, 9, 2, 2);
    stone!(2, 1, 12, 9, 2, 12);
    stone!(2, 1, 3, 2, 2, 11);
    stone!(9, 1, 3, 9, 2, 11);
    stone!(1, 3, 1, 10, 6, 1);
    stone!(1, 3, 13, 10, 6, 13);
    stone!(1, 3, 2, 1, 6, 12);
    stone!(10, 3, 2, 10, 6, 12);
    stone!(2, 3, 2, 9, 3, 12);
    stone!(2, 6, 2, 9, 6, 12);
    stone!(3, 7, 3, 8, 7, 11);
    stone!(4, 8, 4, 7, 8, 10);
    b.air_box(3, 1, 3, 8, 2, 11);
    b.air_box(4, 3, 6, 7, 3, 9);
    b.air_box(2, 4, 2, 9, 5, 12);
    b.air_box(4, 6, 5, 7, 6, 9);
    b.air_box(5, 7, 6, 6, 7, 8);
    b.air_box(5, 1, 2, 6, 2, 2);
    b.air_box(5, 2, 12, 6, 2, 12);
    b.air_box(5, 5, 1, 6, 5, 1);
    b.air_box(5, 5, 13, 6, 5, 13);
    for (x, y, z) in [(1, 5, 5), (10, 5, 5), (1, 5, 9), (10, 5, 9)] {
        b.place_block(air, x, y, z);
    }
    for z in [0, 14] {
        stone!(2, 4, z, 2, 5, z);
        stone!(4, 4, z, 4, 5, z);
        stone!(7, 4, z, 7, 5, z);
        stone!(9, 4, z, 9, 5, z);
    }
    stone!(5, 6, 0, 6, 6, 0);
    for x in [0, 11] {
        let mut z = 2;
        while z <= 12 {
            stone!(x, 4, z, x, 5, z);
            z += 2;
        }
        stone!(x, 6, 5, x, 6, 5);
        stone!(x, 6, 9, x, 6, 9);
    }
    stone!(2, 7, 2, 2, 9, 2);
    stone!(9, 7, 2, 9, 9, 2);
    stone!(2, 7, 12, 2, 9, 12);
    stone!(9, 7, 12, 9, 9, 12);
    stone!(4, 9, 4, 4, 9, 4);
    stone!(7, 9, 4, 7, 9, 4);
    stone!(4, 9, 10, 4, 9, 10);
    stone!(7, 9, 10, 7, 9, 10);
    stone!(5, 9, 7, 6, 9, 7);
    let cs = |b: &PieceBuilder<'_>, f: &str| b.s("cobblestone_stairs", &[("facing", f)]);
    let (cn, cso, ce, cw) = (
        cs(&b, "north"),
        cs(&b, "south"),
        cs(&b, "east"),
        cs(&b, "west"),
    );
    b.place_block(cn, 5, 9, 6);
    b.place_block(cn, 6, 9, 6);
    b.place_block(cso, 5, 9, 8);
    b.place_block(cso, 6, 9, 8);
    for x in 4..=7 {
        b.place_block(cn, x, 0, 0);
    }
    for (x, y, z) in [
        (4, 1, 8),
        (4, 2, 9),
        (4, 3, 10),
        (7, 1, 8),
        (7, 2, 9),
        (7, 3, 10),
    ] {
        b.place_block(cn, x, y, z);
    }
    stone!(4, 1, 9, 4, 1, 9);
    stone!(7, 1, 9, 7, 1, 9);
    stone!(4, 1, 10, 7, 2, 10);
    stone!(5, 4, 5, 6, 4, 5);
    b.place_block(ce, 4, 4, 5);
    b.place_block(cw, 7, 4, 5);
    for i in 0..=3 {
        b.place_block(cso, 5, -i, 6 + i);
        b.place_block(cso, 6, -i, 6 + i);
        b.air_box(5, -i, 7 + i, 6, -i, 9 + i);
    }
    b.air_box(1, -3, 12, 10, -1, 13);
    b.air_box(1, -3, 1, 3, -1, 13);
    b.air_box(1, -3, 1, 9, -1, 5);
    let mut z = 1;
    while z <= 13 {
        stone!(1, -3, z, 1, -2, z);
        z += 2;
    }
    let mut z = 2;
    while z <= 12 {
        stone!(1, -1, z, 3, -1, z);
        z += 2;
    }
    stone!(2, -2, 1, 5, -2, 1);
    stone!(7, -2, 1, 9, -2, 1);
    stone!(6, -3, 1, 6, -3, 1);
    stone!(6, -1, 1, 6, -1, 1);
    // Arrow trap 1.
    let hook = |b: &PieceBuilder<'_>, f: &str| {
        b.s("tripwire_hook", &[("attached", "true"), ("facing", f)])
    };
    let (he, hw, hn, hs) = (
        hook(&b, "east"),
        hook(&b, "west"),
        hook(&b, "north"),
        hook(&b, "south"),
    );
    b.place_block(he, 1, -3, 8);
    b.place_block(hw, 4, -3, 8);
    let wire_ew = b.s(
        "tripwire",
        &[("east", "true"), ("west", "true"), ("attached", "true")],
    );
    b.place_block(wire_ew, 2, -3, 8);
    b.place_block(wire_ew, 3, -3, 8);
    let rs = |b: &PieceBuilder<'_>, p: &[(&str, &str)]| b.s("redstone_wire", p);
    let ns = rs(&b, &[("north", "side"), ("south", "side")]);
    let nw = rs(&b, &[("north", "side"), ("west", "side")]);
    let ew = rs(&b, &[("east", "side"), ("west", "side")]);
    for z in 2..=7 {
        b.place_block(ns, 5, -3, z);
    }
    b.place_block(nw, 5, -3, 1);
    b.place_block(ew, 4, -3, 1);
    b.place_block(mossy, 3, -3, 1);
    if !st.placed_trap1 {
        st.placed_trap1 = b.create_dispenser(
            rng,
            3,
            -2,
            1,
            Direction::North,
            "minecraft:chests/jungle_temple_dispenser",
        );
    }
    let vine_s = b.s("vine", &[("south", "true")]);
    b.place_block(vine_s, 3, -2, 2);
    // Arrow trap 2.
    b.place_block(hn, 7, -3, 1);
    b.place_block(hs, 7, -3, 5);
    let wire_ns = b.s(
        "tripwire",
        &[("north", "true"), ("south", "true"), ("attached", "true")],
    );
    for z in 2..=4 {
        b.place_block(wire_ns, 7, -3, z);
    }
    b.place_block(ew, 8, -3, 6);
    let ws = rs(&b, &[("west", "side"), ("south", "side")]);
    b.place_block(ws, 9, -3, 6);
    let ns_up = rs(&b, &[("north", "side"), ("south", "up")]);
    b.place_block(ns_up, 9, -3, 5);
    b.place_block(mossy, 9, -3, 4);
    b.place_block(ns, 9, -2, 4);
    if !st.placed_trap2 {
        st.placed_trap2 = b.create_dispenser(
            rng,
            9,
            -2,
            3,
            Direction::West,
            "minecraft:chests/jungle_temple_dispenser",
        );
    }
    let vine_e = b.s("vine", &[("east", "true")]);
    b.place_block(vine_e, 8, -1, 3);
    b.place_block(vine_e, 8, -2, 3);
    if !st.placed_main_chest {
        st.placed_main_chest = b.create_chest(rng, 8, -3, 3, "minecraft:chests/jungle_temple");
    }
    for (x, y, z) in [
        (9, -3, 2),
        (8, -3, 1),
        (4, -3, 5),
        (5, -2, 5),
        (5, -1, 5),
        (6, -3, 5),
        (7, -2, 5),
        (7, -1, 5),
        (8, -3, 5),
    ] {
        b.place_block(mossy, x, y, z);
    }
    stone!(9, -1, 1, 9, -1, 5);
    // Lever puzzle.
    b.air_box(8, -3, 8, 10, -1, 10);
    let csb = b.d("chiseled_stone_bricks");
    for x in 8..=10 {
        b.place_block(csb, x, -2, 11);
    }
    let lever = b.s("lever", &[("facing", "north"), ("face", "wall")]);
    for x in 8..=10 {
        b.place_block(lever, x, -2, 12);
    }
    stone!(8, -3, 8, 8, -3, 10);
    stone!(10, -3, 8, 10, -3, 10);
    b.place_block(mossy, 10, -2, 9);
    b.place_block(ns, 8, -2, 9);
    b.place_block(ns, 8, -2, 10);
    let all4 = rs(
        &b,
        &[
            ("north", "side"),
            ("south", "side"),
            ("east", "side"),
            ("west", "side"),
        ],
    );
    b.place_block(all4, 10, -1, 9);
    let piston = |b: &PieceBuilder<'_>, f: &str| b.s("sticky_piston", &[("facing", f)]);
    let (pu, pw) = (piston(&b, "up"), piston(&b, "west"));
    b.place_block(pu, 9, -2, 8);
    b.place_block(pw, 10, -2, 8);
    b.place_block(pw, 10, -1, 8);
    let rep = b.s("repeater", &[("facing", "north")]);
    b.place_block(rep, 10, -2, 10);
    if !st.placed_hidden_chest {
        st.placed_hidden_chest = b.create_chest(rng, 9, -3, 10, "minecraft:chests/jungle_temple");
    }
}

// ---------------------------------------------------------------------------
// Swamp hut
// ---------------------------------------------------------------------------

/// `SwampHutPiece.postProcess`.
pub fn swamp_hut(piece: &mut StructurePiece, call: &mut PieceCall<'_>) {
    let PieceData::SwampHut(st) = &mut piece.data else {
        return;
    };
    let mut bb = piece.bounding_box;
    let chunk = *call.chunk_bb;
    if !update_average_ground_height(&*call.world, &mut bb, &chunk, &mut st.height_position, 0) {
        return;
    }
    piece.bounding_box = bb;
    let mut b = PieceBuilder::new(&mut *call.world, bb, piece.orientation, chunk);
    let planks = b.d("spruce_planks");
    let log = b.d("oak_log");
    let fence = b.d("oak_fence");
    let air = b.air();
    b.fill(1, 1, 1, 5, 1, 7, planks);
    b.fill(1, 4, 2, 5, 4, 7, planks);
    b.fill(2, 1, 0, 4, 1, 0, planks);
    b.fill(2, 2, 2, 3, 3, 2, planks);
    b.fill(1, 2, 3, 1, 3, 6, planks);
    b.fill(5, 2, 3, 5, 3, 6, planks);
    b.fill(2, 2, 7, 4, 3, 7, planks);
    b.fill(1, 0, 2, 1, 3, 2, log);
    b.fill(5, 0, 2, 5, 3, 2, log);
    b.fill(1, 0, 7, 1, 3, 7, log);
    b.fill(5, 0, 7, 5, 3, 7, log);
    for (x, y, z) in [(2, 3, 2), (3, 3, 7), (1, 2, 1), (5, 2, 1)] {
        b.place_block(fence, x, y, z);
    }
    for (x, y, z) in [(1, 3, 4), (5, 3, 4), (5, 3, 5)] {
        b.place_block(air, x, y, z);
    }
    let pot = b.d("potted_red_mushroom");
    b.place_block(pot, 1, 3, 5);
    let ct = b.d("crafting_table");
    b.place_block(ct, 3, 2, 6);
    let cauldron = b.d("cauldron");
    b.place_block(cauldron, 4, 2, 6);
    let stairs = |b: &PieceBuilder<'_>, f: &str, shape: &str| {
        b.s("spruce_stairs", &[("facing", f), ("shape", shape)])
    };
    let sn = stairs(&b, "north", "straight");
    let se = stairs(&b, "east", "straight");
    let sw = stairs(&b, "west", "straight");
    let ss = stairs(&b, "south", "straight");
    b.fill(0, 4, 1, 6, 4, 1, sn);
    b.fill(0, 4, 2, 0, 4, 7, se);
    b.fill(6, 4, 2, 6, 4, 7, sw);
    b.fill(0, 4, 8, 6, 4, 8, ss);
    let c1 = stairs(&b, "north", "outer_right");
    let c2 = stairs(&b, "north", "outer_left");
    let c3 = stairs(&b, "south", "outer_left");
    let c4 = stairs(&b, "south", "outer_right");
    b.place_block(c1, 0, 4, 1);
    b.place_block(c2, 6, 4, 1);
    b.place_block(c3, 0, 4, 8);
    b.place_block(c4, 6, 4, 8);
    for z in [2, 7] {
        for x in [1, 5] {
            b.fill_column_down(log, x, -1, z);
        }
    }
    let pos = b.world_pos(2, 2, 5);
    if chunk.is_inside(pos) {
        let at = [
            f64::from(pos.x) + 0.5,
            f64::from(pos.y),
            f64::from(pos.z) + 0.5,
        ];
        if !st.spawned_witch {
            st.spawned_witch = true;
            spawn_entity(b.world, "minecraft:witch", at, true);
        }
        if !st.spawned_cat {
            st.spawned_cat = true;
            let mut nbt = Compound::new();
            nbt.insert("id", Tag::from("minecraft:cat"));
            nbt.insert("variant", Tag::from("minecraft:all_black"));
            nbt.insert("PersistenceRequired", Tag::Byte(1));
            let mut p = zenith_nbt::List::new();
            for v in at {
                p.push(Tag::Double(v));
            }
            nbt.insert("Pos", Tag::List(p));
            b.world.add_entity(nbt);
        }
    }
}

// ---------------------------------------------------------------------------
// Buried treasure
// ---------------------------------------------------------------------------

/// `BuriedTreasurePiece.postProcess`.
pub fn buried_treasure(piece: &mut StructurePiece, call: &mut PieceCall<'_>) {
    let bb = piece.bounding_box;
    let y = call
        .world
        .get_height(HeightmapKind::OceanFloorWg, bb.min_x, bb.min_z);
    let mut pos = BlockPos::new(bb.min_x, y, bb.min_z);
    let chunk = *call.chunk_bb;
    let min_y = call.world.min_y();
    let mut b = PieceBuilder::new(&mut *call.world, bb, None, chunk);
    let soft_targets = ["sandstone", "stone", "andesite", "granite", "diorite"];
    let sand = b.d("sand");
    while pos.y > min_y {
        let cur = b.world.get_block_state(pos);
        let below = b.world.get_block_state(pos.below(1));
        let below_path = b.path_of(below);
        let is_target = soft_targets.contains(&below_path.as_str());
        if is_target {
            let fluid_or_air =
                |b: &PieceBuilder<'_>, s: u32| b.is_air(s) || b.is(s, "water") || b.is(s, "lava");
            let soft = if !fluid_or_air(&b, cur) { cur } else { sand };
            for dir in [
                Direction::Down,
                Direction::Up,
                Direction::North,
                Direction::South,
                Direction::West,
                Direction::East,
            ] {
                let rel = pos.relative(dir, 1);
                let rs = b.world.get_block_state(rel);
                if fluid_or_air(&b, rs) {
                    let brs = b.world.get_block_state(rel.below(1));
                    if fluid_or_air(&b, brs) && dir != Direction::Up {
                        b.world.set_block_state(rel, below, 3);
                    } else {
                        b.world.set_block_state(rel, soft, 3);
                    }
                }
            }
            piece.bounding_box = BoundingBox::from_coords(pos.x, pos.y, pos.z, pos.x, pos.y, pos.z);
            b.create_chest_at(
                &mut *call.random,
                pos,
                "minecraft:chests/buried_treasure",
                None,
            );
            return;
        }
        pos.y -= 1;
    }
}
