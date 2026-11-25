//! Mineshaft (`minecraft:mineshaft`) piece-tree generator
//! (`docs/specs/worldgen/structures-legacy-pieces.md` §2).

use zenith_math::{LegacyRandom, RandomSource};
use zenith_worldgen::HeightmapKind;

use crate::data::MineshaftType;
use crate::geom::{BlockPos, BoundingBox, BoundingBoxExt, Direction, random_between_inclusive};
use crate::legacy::{
    LegacyGenContext, LegacyPiece, has_collision, move_below_sea_level, offset_pieces_vertically,
    pieces_bounding_box,
};

#[derive(Debug, Clone)]
enum MinePiece {
    Room {
        bb: BoundingBox,
        gen_depth: i32,
    },
    Corridor {
        bb: BoundingBox,
        orientation: Direction,
        gen_depth: i32,
    },
    Crossing {
        bb: BoundingBox,
        direction: Direction,
        is_two_floored: bool,
        gen_depth: i32,
    },
    Stairs {
        bb: BoundingBox,
        orientation: Direction,
        gen_depth: i32,
    },
}

impl MinePiece {
    fn to_legacy(&self) -> LegacyPiece {
        match *self {
            Self::Room { bb, gen_depth } => {
                LegacyPiece::new("minecraft:msroom", bb, None, gen_depth)
            }
            Self::Corridor {
                bb,
                orientation,
                gen_depth,
            } => LegacyPiece::new("minecraft:mscorridor", bb, Some(orientation), gen_depth),
            Self::Crossing { bb, gen_depth, .. } => {
                LegacyPiece::new("minecraft:mscrossing", bb, None, gen_depth)
            }
            Self::Stairs {
                bb,
                orientation,
                gen_depth,
            } => LegacyPiece::new("minecraft:msstairs", bb, Some(orientation), gen_depth),
        }
    }
}

/// Generates pieces for `minecraft:mineshaft` (§2.2–§2.5).
pub fn generate(
    mineshaft_type: MineshaftType,
    chunk_x: i32,
    chunk_z: i32,
    rng: &mut LegacyRandom,
    ctx: &mut dyn LegacyGenContext,
) -> Option<Vec<LegacyPiece>> {
    // §2.2 step 1: discard one legacy double.
    let _ = rng.next_double();

    // Asymmetric start position: (middleBlockX, 50, minBlockZ).
    let start_pos = BlockPos::new((chunk_x << 4) + 8, 50, chunk_z << 4);

    // Construct MineShaftRoom at (blockX(2), 50, blockZ(2)).
    let west = (chunk_x << 4) + 2;
    let north = (chunk_z << 4) + 2;
    let x_extra = rng.next_int_bounded(6);
    let y_extra = rng.next_int_bounded(6);
    let z_extra = rng.next_int_bounded(6);
    let start_bb = BoundingBox::from_coords(
        west,
        50,
        north,
        west + 7 + x_extra,
        54 + y_extra,
        north + 7 + z_extra,
    );
    let start_piece = MinePiece::Room {
        bb: start_bb,
        gen_depth: 0,
    };

    let mut pieces = vec![start_piece.to_legacy()];
    add_children(&start_piece, start_bb, &mut pieces, rng);

    let dy = match mineshaft_type {
        MineshaftType::Mesa => {
            let bb = pieces_bounding_box(&pieces)?;
            let center = bb.center();
            let surface_y = ctx.base_height(center.x, center.z, HeightmapKind::WorldSurfaceWg);
            let sea_level = ctx.sea_level();
            let target_y = if surface_y <= sea_level {
                sea_level
            } else {
                random_between_inclusive(rng, sea_level, surface_y)
            };
            let dy = target_y - center.y;
            offset_pieces_vertically(&mut pieces, dy);
            dy
        }
        MineshaftType::Normal => {
            move_below_sea_level(&mut pieces, ctx.sea_level(), ctx.noise_min_y(), rng, 10)
        }
    };

    // Biome validation runs on the shifted start_pos (§2.2 step 5).
    if !ctx.is_valid_biome(start_pos.offset(0, dy, 0)) {
        return None;
    }

    Some(pieces)
}

#[allow(clippy::too_many_arguments)]
fn generate_and_add_piece(
    start_bb: BoundingBox,
    pieces: &mut Vec<LegacyPiece>,
    rng: &mut LegacyRandom,
    foot_x: i32,
    foot_y: i32,
    foot_z: i32,
    dir: Direction,
    depth: i32,
) -> Option<BoundingBox> {
    if depth > 8 {
        return None;
    }
    if (foot_x - start_bb.min_x).abs() > 80 || (foot_z - start_bb.min_z).abs() > 80 {
        return None;
    }
    let sel = rng.next_int_bounded(100);
    let child = if sel >= 80 {
        let bb = find_crossing(pieces, rng, foot_x, foot_y, foot_z, dir)?;
        MinePiece::Crossing {
            bb,
            direction: dir,
            is_two_floored: bb.y_span() > 3,
            gen_depth: depth + 1,
        }
    } else if sel >= 70 {
        let bb = find_stairs(pieces, foot_x, foot_y, foot_z, dir)?;
        MinePiece::Stairs {
            bb,
            orientation: dir,
            gen_depth: depth + 1,
        }
    } else {
        let bb = find_corridor_size(pieces, rng, foot_x, foot_y, foot_z, dir)?;
        // MineShaftCorridor constructor RNG draws (§2.5B):
        let has_rails = rng.next_int_bounded(3) == 0;
        let _spider_corridor = !has_rails && rng.next_int_bounded(23) == 0;
        MinePiece::Corridor {
            bb,
            orientation: dir,
            gen_depth: depth + 1,
        }
    };

    let child_bb = match child {
        MinePiece::Room { bb, .. }
        | MinePiece::Corridor { bb, .. }
        | MinePiece::Crossing { bb, .. }
        | MinePiece::Stairs { bb, .. } => bb,
    };
    pieces.push(child.to_legacy());
    // Pure DFS recursion immediately after adding!
    add_children(&child, start_bb, pieces, rng);
    Some(child_bb)
}

fn add_children(
    piece: &MinePiece,
    start_bb: BoundingBox,
    pieces: &mut Vec<LegacyPiece>,
    rng: &mut LegacyRandom,
) {
    match *piece {
        MinePiece::Room { bb, gen_depth } => {
            let height_space = (bb.y_span() - 4).max(1);
            // NORTH
            let mut pos = 0;
            while pos < bb.x_span() {
                pos += rng.next_int_bounded(bb.x_span());
                if pos + 3 > bb.x_span() {
                    break;
                }
                let y = bb.min_y + rng.next_int_bounded(height_space) + 1;
                let _ = generate_and_add_piece(
                    start_bb,
                    pieces,
                    rng,
                    bb.min_x + pos,
                    y,
                    bb.min_z - 1,
                    Direction::North,
                    gen_depth,
                );
                pos += 4;
            }
            // SOUTH
            let mut pos = 0;
            while pos < bb.x_span() {
                pos += rng.next_int_bounded(bb.x_span());
                if pos + 3 > bb.x_span() {
                    break;
                }
                let y = bb.min_y + rng.next_int_bounded(height_space) + 1;
                let _ = generate_and_add_piece(
                    start_bb,
                    pieces,
                    rng,
                    bb.min_x + pos,
                    y,
                    bb.max_z + 1,
                    Direction::South,
                    gen_depth,
                );
                pos += 4;
            }
            // WEST
            let mut pos = 0;
            while pos < bb.z_span() {
                pos += rng.next_int_bounded(bb.z_span());
                if pos + 3 > bb.z_span() {
                    break;
                }
                let y = bb.min_y + rng.next_int_bounded(height_space) + 1;
                let _ = generate_and_add_piece(
                    start_bb,
                    pieces,
                    rng,
                    bb.min_x - 1,
                    y,
                    bb.min_z + pos,
                    Direction::West,
                    gen_depth,
                );
                pos += 4;
            }
            // EAST
            let mut pos = 0;
            while pos < bb.z_span() {
                pos += rng.next_int_bounded(bb.z_span());
                if pos + 3 > bb.z_span() {
                    break;
                }
                let y = bb.min_y + rng.next_int_bounded(height_space) + 1;
                let _ = generate_and_add_piece(
                    start_bb,
                    pieces,
                    rng,
                    bb.max_x + 1,
                    y,
                    bb.min_z + pos,
                    Direction::East,
                    gen_depth,
                );
                pos += 4;
            }
        }
        MinePiece::Corridor {
            bb,
            orientation,
            gen_depth,
        } => {
            let end_sel = rng.next_int_bounded(4);
            let y = bb.min_y - 1 + rng.next_int_bounded(3);
            match orientation {
                Direction::North => {
                    if end_sel <= 1 {
                        generate_and_add_piece(
                            start_bb,
                            pieces,
                            rng,
                            bb.min_x,
                            y,
                            bb.min_z - 1,
                            Direction::North,
                            gen_depth,
                        );
                    } else if end_sel == 2 {
                        generate_and_add_piece(
                            start_bb,
                            pieces,
                            rng,
                            bb.min_x - 1,
                            y,
                            bb.min_z,
                            Direction::West,
                            gen_depth,
                        );
                    } else {
                        generate_and_add_piece(
                            start_bb,
                            pieces,
                            rng,
                            bb.max_x + 1,
                            y,
                            bb.min_z,
                            Direction::East,
                            gen_depth,
                        );
                    }
                }
                Direction::South => {
                    if end_sel <= 1 {
                        generate_and_add_piece(
                            start_bb,
                            pieces,
                            rng,
                            bb.min_x,
                            y,
                            bb.max_z + 1,
                            Direction::South,
                            gen_depth,
                        );
                    } else if end_sel == 2 {
                        generate_and_add_piece(
                            start_bb,
                            pieces,
                            rng,
                            bb.min_x - 1,
                            y,
                            bb.max_z - 3,
                            Direction::West,
                            gen_depth,
                        );
                    } else {
                        generate_and_add_piece(
                            start_bb,
                            pieces,
                            rng,
                            bb.max_x + 1,
                            y,
                            bb.max_z - 3,
                            Direction::East,
                            gen_depth,
                        );
                    }
                }
                Direction::West => {
                    if end_sel <= 1 {
                        generate_and_add_piece(
                            start_bb,
                            pieces,
                            rng,
                            bb.min_x - 1,
                            y,
                            bb.min_z,
                            Direction::West,
                            gen_depth,
                        );
                    } else if end_sel == 2 {
                        generate_and_add_piece(
                            start_bb,
                            pieces,
                            rng,
                            bb.min_x,
                            y,
                            bb.min_z - 1,
                            Direction::North,
                            gen_depth,
                        );
                    } else {
                        generate_and_add_piece(
                            start_bb,
                            pieces,
                            rng,
                            bb.min_x,
                            y,
                            bb.max_z + 1,
                            Direction::South,
                            gen_depth,
                        );
                    }
                }
                Direction::East => {
                    if end_sel <= 1 {
                        generate_and_add_piece(
                            start_bb,
                            pieces,
                            rng,
                            bb.max_x + 1,
                            y,
                            bb.min_z,
                            Direction::East,
                            gen_depth,
                        );
                    } else if end_sel == 2 {
                        generate_and_add_piece(
                            start_bb,
                            pieces,
                            rng,
                            bb.max_x - 3,
                            y,
                            bb.min_z - 1,
                            Direction::North,
                            gen_depth,
                        );
                    } else {
                        generate_and_add_piece(
                            start_bb,
                            pieces,
                            rng,
                            bb.max_x - 3,
                            y,
                            bb.max_z + 1,
                            Direction::South,
                            gen_depth,
                        );
                    }
                }
                _ => {}
            }

            if gen_depth < 8 {
                if matches!(orientation, Direction::North | Direction::South) {
                    let mut z = bb.min_z + 3;
                    while z + 3 <= bb.max_z {
                        let sel = rng.next_int_bounded(5);
                        if sel == 0 {
                            generate_and_add_piece(
                                start_bb,
                                pieces,
                                rng,
                                bb.min_x - 1,
                                bb.min_y,
                                z,
                                Direction::West,
                                gen_depth + 1,
                            );
                        } else if sel == 1 {
                            generate_and_add_piece(
                                start_bb,
                                pieces,
                                rng,
                                bb.max_x + 1,
                                bb.min_y,
                                z,
                                Direction::East,
                                gen_depth + 1,
                            );
                        }
                        z += 5;
                    }
                } else {
                    let mut x = bb.min_x + 3;
                    while x + 3 <= bb.max_x {
                        let sel = rng.next_int_bounded(5);
                        if sel == 0 {
                            generate_and_add_piece(
                                start_bb,
                                pieces,
                                rng,
                                x,
                                bb.min_y,
                                bb.min_z - 1,
                                Direction::North,
                                gen_depth + 1,
                            );
                        } else if sel == 1 {
                            generate_and_add_piece(
                                start_bb,
                                pieces,
                                rng,
                                x,
                                bb.min_y,
                                bb.max_z + 1,
                                Direction::South,
                                gen_depth + 1,
                            );
                        }
                        x += 5;
                    }
                }
            }
        }
        MinePiece::Crossing {
            bb,
            direction,
            is_two_floored,
            gen_depth,
        } => {
            match direction {
                Direction::North => {
                    generate_and_add_piece(
                        start_bb,
                        pieces,
                        rng,
                        bb.min_x + 1,
                        bb.min_y,
                        bb.min_z - 1,
                        Direction::North,
                        gen_depth,
                    );
                    generate_and_add_piece(
                        start_bb,
                        pieces,
                        rng,
                        bb.min_x - 1,
                        bb.min_y,
                        bb.min_z + 1,
                        Direction::West,
                        gen_depth,
                    );
                    generate_and_add_piece(
                        start_bb,
                        pieces,
                        rng,
                        bb.max_x + 1,
                        bb.min_y,
                        bb.min_z + 1,
                        Direction::East,
                        gen_depth,
                    );
                }
                Direction::South => {
                    generate_and_add_piece(
                        start_bb,
                        pieces,
                        rng,
                        bb.min_x + 1,
                        bb.min_y,
                        bb.max_z + 1,
                        Direction::South,
                        gen_depth,
                    );
                    generate_and_add_piece(
                        start_bb,
                        pieces,
                        rng,
                        bb.min_x - 1,
                        bb.min_y,
                        bb.min_z + 1,
                        Direction::West,
                        gen_depth,
                    );
                    generate_and_add_piece(
                        start_bb,
                        pieces,
                        rng,
                        bb.max_x + 1,
                        bb.min_y,
                        bb.min_z + 1,
                        Direction::East,
                        gen_depth,
                    );
                }
                Direction::West => {
                    generate_and_add_piece(
                        start_bb,
                        pieces,
                        rng,
                        bb.min_x + 1,
                        bb.min_y,
                        bb.min_z - 1,
                        Direction::North,
                        gen_depth,
                    );
                    generate_and_add_piece(
                        start_bb,
                        pieces,
                        rng,
                        bb.min_x + 1,
                        bb.min_y,
                        bb.max_z + 1,
                        Direction::South,
                        gen_depth,
                    );
                    generate_and_add_piece(
                        start_bb,
                        pieces,
                        rng,
                        bb.min_x - 1,
                        bb.min_y,
                        bb.min_z + 1,
                        Direction::West,
                        gen_depth,
                    );
                }
                Direction::East => {
                    generate_and_add_piece(
                        start_bb,
                        pieces,
                        rng,
                        bb.min_x + 1,
                        bb.min_y,
                        bb.min_z - 1,
                        Direction::North,
                        gen_depth,
                    );
                    generate_and_add_piece(
                        start_bb,
                        pieces,
                        rng,
                        bb.min_x + 1,
                        bb.min_y,
                        bb.max_z + 1,
                        Direction::South,
                        gen_depth,
                    );
                    generate_and_add_piece(
                        start_bb,
                        pieces,
                        rng,
                        bb.max_x + 1,
                        bb.min_y,
                        bb.min_z + 1,
                        Direction::East,
                        gen_depth,
                    );
                }
                _ => {}
            }
            if is_two_floored {
                if rng.next_boolean() {
                    generate_and_add_piece(
                        start_bb,
                        pieces,
                        rng,
                        bb.min_x + 1,
                        bb.min_y + 4,
                        bb.min_z - 1,
                        Direction::North,
                        gen_depth,
                    );
                }
                if rng.next_boolean() {
                    generate_and_add_piece(
                        start_bb,
                        pieces,
                        rng,
                        bb.min_x - 1,
                        bb.min_y + 4,
                        bb.min_z + 1,
                        Direction::West,
                        gen_depth,
                    );
                }
                if rng.next_boolean() {
                    generate_and_add_piece(
                        start_bb,
                        pieces,
                        rng,
                        bb.max_x + 1,
                        bb.min_y + 4,
                        bb.min_z + 1,
                        Direction::East,
                        gen_depth,
                    );
                }
                if rng.next_boolean() {
                    generate_and_add_piece(
                        start_bb,
                        pieces,
                        rng,
                        bb.min_x + 1,
                        bb.min_y + 4,
                        bb.max_z + 1,
                        Direction::South,
                        gen_depth,
                    );
                }
            }
        }
        MinePiece::Stairs {
            bb,
            orientation,
            gen_depth,
        } => match orientation {
            Direction::North => {
                generate_and_add_piece(
                    start_bb,
                    pieces,
                    rng,
                    bb.min_x,
                    bb.min_y,
                    bb.min_z - 1,
                    Direction::North,
                    gen_depth,
                );
            }
            Direction::South => {
                generate_and_add_piece(
                    start_bb,
                    pieces,
                    rng,
                    bb.min_x,
                    bb.min_y,
                    bb.max_z + 1,
                    Direction::South,
                    gen_depth,
                );
            }
            Direction::West => {
                generate_and_add_piece(
                    start_bb,
                    pieces,
                    rng,
                    bb.min_x - 1,
                    bb.min_y,
                    bb.min_z,
                    Direction::West,
                    gen_depth,
                );
            }
            Direction::East => {
                generate_and_add_piece(
                    start_bb,
                    pieces,
                    rng,
                    bb.max_x + 1,
                    bb.min_y,
                    bb.min_z,
                    Direction::East,
                    gen_depth,
                );
            }
            _ => {}
        },
    }
}

fn find_corridor_size(
    pieces: &[LegacyPiece],
    rng: &mut LegacyRandom,
    foot_x: i32,
    foot_y: i32,
    foot_z: i32,
    dir: Direction,
) -> Option<BoundingBox> {
    let mut corridor_length = rng.next_int_bounded(3) + 2;
    while corridor_length > 0 {
        let l = corridor_length * 5;
        let rel = match dir {
            Direction::North => BoundingBox::from_coords(0, 0, -(l - 1), 2, 2, 0),
            Direction::South => BoundingBox::from_coords(0, 0, 0, 2, 2, l - 1),
            Direction::West => BoundingBox::from_coords(-(l - 1), 0, 0, 0, 2, 2),
            Direction::East => BoundingBox::from_coords(0, 0, 0, l - 1, 2, 2),
            _ => BoundingBox::from_coords(0, 0, 0, 2, 2, l - 1),
        };
        let bb = rel.moved(foot_x, foot_y, foot_z);
        if !has_collision(pieces, bb) {
            return Some(bb);
        }
        corridor_length -= 1;
    }
    None
}

fn find_crossing(
    pieces: &[LegacyPiece],
    rng: &mut LegacyRandom,
    foot_x: i32,
    foot_y: i32,
    foot_z: i32,
    dir: Direction,
) -> Option<BoundingBox> {
    let y1 = if rng.next_int_bounded(4) == 0 { 6 } else { 2 };
    let rel = match dir {
        Direction::North => BoundingBox::from_coords(-1, 0, -4, 3, y1, 0),
        Direction::South => BoundingBox::from_coords(-1, 0, 0, 3, y1, 4),
        Direction::West => BoundingBox::from_coords(-4, 0, -1, 0, y1, 3),
        Direction::East => BoundingBox::from_coords(0, 0, -1, 4, y1, 3),
        _ => BoundingBox::from_coords(-1, 0, 0, 3, y1, 4),
    };
    let bb = rel.moved(foot_x, foot_y, foot_z);
    if has_collision(pieces, bb) {
        None
    } else {
        Some(bb)
    }
}

fn find_stairs(
    pieces: &[LegacyPiece],
    foot_x: i32,
    foot_y: i32,
    foot_z: i32,
    dir: Direction,
) -> Option<BoundingBox> {
    let rel = match dir {
        Direction::North => BoundingBox::from_coords(0, -5, -8, 2, 2, 0),
        Direction::South => BoundingBox::from_coords(0, -5, 0, 2, 2, 8),
        Direction::West => BoundingBox::from_coords(-8, -5, 0, 0, 2, 2),
        Direction::East => BoundingBox::from_coords(0, -5, 0, 8, 2, 2),
        _ => BoundingBox::from_coords(0, -5, 0, 2, 2, 8),
    };
    let bb = rel.moved(foot_x, foot_y, foot_z);
    if has_collision(pieces, bb) {
        None
    } else {
        Some(bb)
    }
}
