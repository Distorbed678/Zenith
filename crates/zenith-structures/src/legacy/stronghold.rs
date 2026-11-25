//! Stronghold (`minecraft:stronghold`) piece-graph generator
//! (`docs/specs/worldgen/structures-legacy-pieces.md` §3).

use zenith_math::{LegacyRandom, RandomSource};

use crate::geom::{
    BlockPos, BoundingBox, BoundingBoxExt, Direction, random_horizontal_direction,
    set_large_feature_seed,
};
use crate::legacy::{LegacyGenContext, LegacyPiece, has_collision, move_below_sea_level};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WeightKind {
    Straight,
    PrisonHall,
    LeftTurn,
    RightTurn,
    RoomCrossing,
    StraightStairsDown,
    StairsDown,
    FiveCrossing,
    ChestCorridor,
    Library,
    PortalRoom,
}

#[derive(Debug, Clone, Copy)]
struct PieceWeight {
    kind: WeightKind,
    weight: i32,
    max_place_count: i32,
    place_count: i32,
}

impl PieceWeight {
    fn do_place(&self, depth: i32) -> bool {
        let extra = match self.kind {
            WeightKind::Library => depth > 4,
            WeightKind::PortalRoom => depth > 5,
            _ => true,
        };
        extra && (self.max_place_count == 0 || self.place_count < self.max_place_count)
    }

    fn is_valid(&self) -> bool {
        self.max_place_count == 0 || self.place_count < self.max_place_count
    }
}

fn initial_weights() -> Vec<PieceWeight> {
    vec![
        PieceWeight {
            kind: WeightKind::Straight,
            weight: 40,
            max_place_count: 0,
            place_count: 0,
        },
        PieceWeight {
            kind: WeightKind::PrisonHall,
            weight: 5,
            max_place_count: 5,
            place_count: 0,
        },
        PieceWeight {
            kind: WeightKind::LeftTurn,
            weight: 20,
            max_place_count: 0,
            place_count: 0,
        },
        PieceWeight {
            kind: WeightKind::RightTurn,
            weight: 20,
            max_place_count: 0,
            place_count: 0,
        },
        PieceWeight {
            kind: WeightKind::RoomCrossing,
            weight: 10,
            max_place_count: 6,
            place_count: 0,
        },
        PieceWeight {
            kind: WeightKind::StraightStairsDown,
            weight: 5,
            max_place_count: 5,
            place_count: 0,
        },
        PieceWeight {
            kind: WeightKind::StairsDown,
            weight: 5,
            max_place_count: 5,
            place_count: 0,
        },
        PieceWeight {
            kind: WeightKind::FiveCrossing,
            weight: 5,
            max_place_count: 4,
            place_count: 0,
        },
        PieceWeight {
            kind: WeightKind::ChestCorridor,
            weight: 5,
            max_place_count: 4,
            place_count: 0,
        },
        PieceWeight {
            kind: WeightKind::Library,
            weight: 10,
            max_place_count: 2,
            place_count: 0,
        },
        PieceWeight {
            kind: WeightKind::PortalRoom,
            weight: 20,
            max_place_count: 1,
            place_count: 0,
        },
    ]
}

#[derive(Debug, Clone)]
enum ShPiece {
    StartPiece {
        bb: BoundingBox,
        orient: Direction,
        gen_depth: i32,
    },
    StairsDown {
        bb: BoundingBox,
        orient: Direction,
        gen_depth: i32,
    },
    Straight {
        bb: BoundingBox,
        orient: Direction,
        gen_depth: i32,
        left_child: bool,
        right_child: bool,
    },
    ChestCorridor {
        bb: BoundingBox,
        orient: Direction,
        gen_depth: i32,
    },
    FillerCorridor {
        bb: BoundingBox,
        orient: Direction,
        gen_depth: i32,
    },
    LeftTurn {
        bb: BoundingBox,
        orient: Direction,
        gen_depth: i32,
    },
    RightTurn {
        bb: BoundingBox,
        orient: Direction,
        gen_depth: i32,
    },
    PrisonHall {
        bb: BoundingBox,
        orient: Direction,
        gen_depth: i32,
    },
    RoomCrossing {
        bb: BoundingBox,
        orient: Direction,
        gen_depth: i32,
    },
    StraightStairsDown {
        bb: BoundingBox,
        orient: Direction,
        gen_depth: i32,
    },
    FiveCrossing {
        bb: BoundingBox,
        orient: Direction,
        gen_depth: i32,
        left_low: bool,
        left_high: bool,
        right_low: bool,
        right_high: bool,
    },
    Library {
        bb: BoundingBox,
        orient: Direction,
        gen_depth: i32,
    },
    PortalRoom {
        bb: BoundingBox,
        orient: Direction,
        gen_depth: i32,
    },
}

impl ShPiece {
    fn to_legacy(&self) -> LegacyPiece {
        match *self {
            Self::StartPiece {
                bb,
                orient,
                gen_depth,
            } => LegacyPiece::new("minecraft:shstart", bb, Some(orient), gen_depth),
            Self::StairsDown {
                bb,
                orient,
                gen_depth,
            } => LegacyPiece::new("minecraft:shsd", bb, Some(orient), gen_depth),
            Self::Straight {
                bb,
                orient,
                gen_depth,
                ..
            } => LegacyPiece::new("minecraft:shs", bb, Some(orient), gen_depth),
            Self::ChestCorridor {
                bb,
                orient,
                gen_depth,
            } => LegacyPiece::new("minecraft:shcc", bb, Some(orient), gen_depth),
            Self::FillerCorridor {
                bb,
                orient,
                gen_depth,
            } => LegacyPiece::new("minecraft:shfc", bb, Some(orient), gen_depth),
            Self::LeftTurn {
                bb,
                orient,
                gen_depth,
            } => LegacyPiece::new("minecraft:shlt", bb, Some(orient), gen_depth),
            Self::RightTurn {
                bb,
                orient,
                gen_depth,
            } => LegacyPiece::new("minecraft:shrt", bb, Some(orient), gen_depth),
            Self::PrisonHall {
                bb,
                orient,
                gen_depth,
            } => LegacyPiece::new("minecraft:shph", bb, Some(orient), gen_depth),
            Self::RoomCrossing {
                bb,
                orient,
                gen_depth,
            } => LegacyPiece::new("minecraft:shrc", bb, Some(orient), gen_depth),
            Self::StraightStairsDown {
                bb,
                orient,
                gen_depth,
            } => LegacyPiece::new("minecraft:shssd", bb, Some(orient), gen_depth),
            Self::FiveCrossing {
                bb,
                orient,
                gen_depth,
                ..
            } => LegacyPiece::new("minecraft:sh5c", bb, Some(orient), gen_depth),
            Self::Library {
                bb,
                orient,
                gen_depth,
            } => LegacyPiece::new("minecraft:shli", bb, Some(orient), gen_depth),
            Self::PortalRoom {
                bb,
                orient,
                gen_depth,
            } => LegacyPiece::new("minecraft:shpr", bb, Some(orient), gen_depth),
        }
    }
}

struct State {
    start_bb: BoundingBox,
    weights: Vec<PieceWeight>,
    imposed: Option<WeightKind>,
    previous: Option<WeightKind>,
    has_portal_room: bool,
    pending: Vec<ShPiece>,
    pieces: Vec<LegacyPiece>,
}

/// Generates pieces for `minecraft:stronghold` (§3.2–§3.5).
pub fn generate(
    chunk_x: i32,
    chunk_z: i32,
    rng: &mut LegacyRandom,
    ctx: &mut dyn LegacyGenContext,
) -> Option<Vec<LegacyPiece>> {
    // §3.2: stub position is (chunkMinX, 0, chunkMinZ).
    let stub_pos = BlockPos::new(chunk_x << 4, 0, chunk_z << 4);
    if !ctx.is_valid_biome(stub_pos) {
        return None;
    }

    let seed = ctx.world_seed();
    let mut tries = 0_i64;
    loop {
        set_large_feature_seed(rng, seed.wrapping_add(tries), chunk_x, chunk_z);
        tries += 1;

        let dir = random_horizontal_direction(rng);
        let start_bb =
            BoundingBox::make_box((chunk_x << 4) + 2, 64, (chunk_z << 4) + 2, dir, 5, 11, 5);
        let start_piece = ShPiece::StartPiece {
            bb: start_bb,
            orient: dir,
            gen_depth: 0,
        };

        let mut st = State {
            start_bb,
            weights: initial_weights(),
            imposed: None,
            previous: None,
            has_portal_room: false,
            pending: Vec::new(),
            pieces: vec![start_piece.to_legacy()],
        };

        add_children(&start_piece, &mut st, rng);
        while !st.pending.is_empty() {
            let idx = rng.next_int_bounded(st.pending.len() as i32) as usize;
            let next = st.pending.remove(idx);
            add_children(&next, &mut st, rng);
        }

        move_below_sea_level(&mut st.pieces, ctx.sea_level(), ctx.noise_min_y(), rng, 10);

        if !st.pieces.is_empty() && st.has_portal_room {
            return Some(st.pieces);
        }
    }
}

#[inline]
fn is_ok_box(bb: BoundingBox) -> bool {
    bb.min_y > 10
}

#[inline]
fn random_small_door(rng: &mut LegacyRandom) {
    let _ = rng.next_int_bounded(5);
}

#[allow(clippy::too_many_arguments)]
fn create_piece(
    kind: WeightKind,
    pieces: &[LegacyPiece],
    rng: &mut LegacyRandom,
    foot_x: i32,
    foot_y: i32,
    foot_z: i32,
    dir: Direction,
    depth: i32,
) -> Option<ShPiece> {
    match kind {
        WeightKind::Straight => {
            let bb = BoundingBox::orient_box(foot_x, foot_y, foot_z, -1, -1, 0, 5, 5, 7, dir);
            if !is_ok_box(bb) || has_collision(pieces, bb) {
                return None;
            }
            random_small_door(rng);
            let left_child = rng.next_int_bounded(2) == 0;
            let right_child = rng.next_int_bounded(2) == 0;
            Some(ShPiece::Straight {
                bb,
                orient: dir,
                gen_depth: depth,
                left_child,
                right_child,
            })
        }
        WeightKind::PrisonHall => {
            let bb = BoundingBox::orient_box(foot_x, foot_y, foot_z, -1, -1, 0, 9, 5, 11, dir);
            if !is_ok_box(bb) || has_collision(pieces, bb) {
                return None;
            }
            random_small_door(rng);
            Some(ShPiece::PrisonHall {
                bb,
                orient: dir,
                gen_depth: depth,
            })
        }
        WeightKind::LeftTurn => {
            let bb = BoundingBox::orient_box(foot_x, foot_y, foot_z, -1, -1, 0, 5, 5, 5, dir);
            if !is_ok_box(bb) || has_collision(pieces, bb) {
                return None;
            }
            random_small_door(rng);
            Some(ShPiece::LeftTurn {
                bb,
                orient: dir,
                gen_depth: depth,
            })
        }
        WeightKind::RightTurn => {
            let bb = BoundingBox::orient_box(foot_x, foot_y, foot_z, -1, -1, 0, 5, 5, 5, dir);
            if !is_ok_box(bb) || has_collision(pieces, bb) {
                return None;
            }
            random_small_door(rng);
            Some(ShPiece::RightTurn {
                bb,
                orient: dir,
                gen_depth: depth,
            })
        }
        WeightKind::RoomCrossing => {
            let bb = BoundingBox::orient_box(foot_x, foot_y, foot_z, -4, -1, 0, 11, 7, 11, dir);
            if !is_ok_box(bb) || has_collision(pieces, bb) {
                return None;
            }
            random_small_door(rng);
            let _room_type = rng.next_int_bounded(5);
            Some(ShPiece::RoomCrossing {
                bb,
                orient: dir,
                gen_depth: depth,
            })
        }
        WeightKind::StraightStairsDown => {
            let bb = BoundingBox::orient_box(foot_x, foot_y, foot_z, -1, -7, 0, 5, 11, 8, dir);
            if !is_ok_box(bb) || has_collision(pieces, bb) {
                return None;
            }
            random_small_door(rng);
            Some(ShPiece::StraightStairsDown {
                bb,
                orient: dir,
                gen_depth: depth,
            })
        }
        WeightKind::StairsDown => {
            let bb = BoundingBox::orient_box(foot_x, foot_y, foot_z, -1, -7, 0, 5, 11, 5, dir);
            if !is_ok_box(bb) || has_collision(pieces, bb) {
                return None;
            }
            random_small_door(rng);
            Some(ShPiece::StairsDown {
                bb,
                orient: dir,
                gen_depth: depth,
            })
        }
        WeightKind::FiveCrossing => {
            let bb = BoundingBox::orient_box(foot_x, foot_y, foot_z, -4, -3, 0, 10, 9, 11, dir);
            if !is_ok_box(bb) || has_collision(pieces, bb) {
                return None;
            }
            random_small_door(rng);
            let left_low = rng.next_boolean();
            let left_high = rng.next_boolean();
            let right_low = rng.next_boolean();
            let right_high = rng.next_int_bounded(3) > 0;
            Some(ShPiece::FiveCrossing {
                bb,
                orient: dir,
                gen_depth: depth,
                left_low,
                left_high,
                right_low,
                right_high,
            })
        }
        WeightKind::ChestCorridor => {
            let bb = BoundingBox::orient_box(foot_x, foot_y, foot_z, -1, -1, 0, 5, 5, 7, dir);
            if !is_ok_box(bb) || has_collision(pieces, bb) {
                return None;
            }
            random_small_door(rng);
            Some(ShPiece::ChestCorridor {
                bb,
                orient: dir,
                gen_depth: depth,
            })
        }
        WeightKind::Library => {
            let mut bb =
                BoundingBox::orient_box(foot_x, foot_y, foot_z, -4, -1, 0, 14, 11, 15, dir);
            if !is_ok_box(bb) || has_collision(pieces, bb) {
                bb = BoundingBox::orient_box(foot_x, foot_y, foot_z, -4, -1, 0, 14, 6, 15, dir);
                if !is_ok_box(bb) || has_collision(pieces, bb) {
                    return None;
                }
            }
            random_small_door(rng);
            Some(ShPiece::Library {
                bb,
                orient: dir,
                gen_depth: depth,
            })
        }
        WeightKind::PortalRoom => {
            let bb = BoundingBox::orient_box(foot_x, foot_y, foot_z, -4, -1, 0, 11, 8, 16, dir);
            if !is_ok_box(bb) || has_collision(pieces, bb) {
                return None;
            }
            Some(ShPiece::PortalRoom {
                bb,
                orient: dir,
                gen_depth: depth,
            })
        }
    }
}

fn find_filler_corridor(
    pieces: &[LegacyPiece],
    foot_x: i32,
    foot_y: i32,
    foot_z: i32,
    dir: Direction,
) -> Option<BoundingBox> {
    let bb = BoundingBox::orient_box(foot_x, foot_y, foot_z, -1, -1, 0, 5, 5, 4, dir);
    let col = pieces.iter().find(|p| p.bounding_box.intersects(bb))?;
    if col.bounding_box.min_y == bb.min_y {
        for d in (1..=2).rev() {
            let test = BoundingBox::orient_box(foot_x, foot_y, foot_z, -1, -1, 0, 5, 5, d, dir);
            if !col.bounding_box.intersects(test) {
                return Some(BoundingBox::orient_box(
                    foot_x,
                    foot_y,
                    foot_z,
                    -1,
                    -1,
                    0,
                    5,
                    5,
                    d + 1,
                    dir,
                ));
            }
        }
    }
    None
}

fn generate_piece_from_small_door(
    st: &mut State,
    rng: &mut LegacyRandom,
    foot_x: i32,
    foot_y: i32,
    foot_z: i32,
    dir: Direction,
    depth: i32,
) -> Option<ShPiece> {
    let mut total_weight = 0_i32;
    let mut has_any_limited = false;
    for pw in &st.weights {
        if pw.max_place_count > 0 && pw.place_count < pw.max_place_count {
            has_any_limited = true;
        }
        total_weight += pw.weight;
    }
    if !has_any_limited {
        return None;
    }

    if let Some(imp) = st.imposed.take()
        && let Some(p) = create_piece(imp, &st.pieces, rng, foot_x, foot_y, foot_z, dir, depth)
    {
        return Some(p);
    }

    for _ in 0..5 {
        let mut w = rng.next_int_bounded(total_weight);
        let mut i = 0;
        while i < st.weights.len() {
            w -= st.weights[i].weight;
            if w < 0 {
                let kind = st.weights[i].kind;
                if !st.weights[i].do_place(depth) || st.previous == Some(kind) {
                    break;
                }
                if let Some(piece) =
                    create_piece(kind, &st.pieces, rng, foot_x, foot_y, foot_z, dir, depth)
                {
                    st.weights[i].place_count += 1;
                    st.previous = Some(kind);
                    if !st.weights[i].is_valid() {
                        st.weights.remove(i);
                    }
                    return Some(piece);
                }
            }
            i += 1;
        }
    }

    if let Some(bb) = find_filler_corridor(&st.pieces, foot_x, foot_y, foot_z, dir)
        && bb.min_y > 1
    {
        return Some(ShPiece::FillerCorridor {
            bb,
            orient: dir,
            gen_depth: depth,
        });
    }
    None
}

fn generate_and_add_piece(
    st: &mut State,
    rng: &mut LegacyRandom,
    foot_x: i32,
    foot_y: i32,
    foot_z: i32,
    dir: Direction,
    depth: i32,
) {
    if depth > 50 {
        return;
    }
    if (foot_x - st.start_bb.min_x).abs() > 112 || (foot_z - st.start_bb.min_z).abs() > 112 {
        return;
    }
    if let Some(child) =
        generate_piece_from_small_door(st, rng, foot_x, foot_y, foot_z, dir, depth + 1)
    {
        st.pieces.push(child.to_legacy());
        st.pending.push(child);
    }
}

fn child_forward(
    bb: BoundingBox,
    orient: Direction,
    depth: i32,
    x_off: i32,
    y_off: i32,
    st: &mut State,
    rng: &mut LegacyRandom,
) {
    match orient {
        Direction::North => {
            generate_and_add_piece(
                st,
                rng,
                bb.min_x + x_off,
                bb.min_y + y_off,
                bb.min_z - 1,
                orient,
                depth,
            );
        }
        Direction::South => {
            generate_and_add_piece(
                st,
                rng,
                bb.min_x + x_off,
                bb.min_y + y_off,
                bb.max_z + 1,
                orient,
                depth,
            );
        }
        Direction::West => {
            generate_and_add_piece(
                st,
                rng,
                bb.min_x - 1,
                bb.min_y + y_off,
                bb.min_z + x_off,
                orient,
                depth,
            );
        }
        Direction::East => {
            generate_and_add_piece(
                st,
                rng,
                bb.max_x + 1,
                bb.min_y + y_off,
                bb.min_z + x_off,
                orient,
                depth,
            );
        }
        _ => {}
    }
}

fn child_left(
    bb: BoundingBox,
    orient: Direction,
    depth: i32,
    y_off: i32,
    z_off: i32,
    st: &mut State,
    rng: &mut LegacyRandom,
) {
    match orient {
        Direction::North | Direction::South => {
            generate_and_add_piece(
                st,
                rng,
                bb.min_x - 1,
                bb.min_y + y_off,
                bb.min_z + z_off,
                Direction::West,
                depth,
            );
        }
        Direction::West | Direction::East => {
            generate_and_add_piece(
                st,
                rng,
                bb.min_x + z_off,
                bb.min_y + y_off,
                bb.min_z - 1,
                Direction::North,
                depth,
            );
        }
        _ => {}
    }
}

fn child_right(
    bb: BoundingBox,
    orient: Direction,
    depth: i32,
    y_off: i32,
    z_off: i32,
    st: &mut State,
    rng: &mut LegacyRandom,
) {
    match orient {
        Direction::North | Direction::South => {
            generate_and_add_piece(
                st,
                rng,
                bb.max_x + 1,
                bb.min_y + y_off,
                bb.min_z + z_off,
                Direction::East,
                depth,
            );
        }
        Direction::West | Direction::East => {
            generate_and_add_piece(
                st,
                rng,
                bb.min_x + z_off,
                bb.min_y + y_off,
                bb.max_z + 1,
                Direction::South,
                depth,
            );
        }
        _ => {}
    }
}

fn add_children(piece: &ShPiece, st: &mut State, rng: &mut LegacyRandom) {
    match *piece {
        ShPiece::StartPiece {
            bb,
            orient,
            gen_depth,
        } => {
            st.imposed = Some(WeightKind::FiveCrossing);
            child_forward(bb, orient, gen_depth, 1, 1, st, rng);
        }
        ShPiece::StairsDown {
            bb,
            orient,
            gen_depth,
        }
        | ShPiece::ChestCorridor {
            bb,
            orient,
            gen_depth,
        }
        | ShPiece::PrisonHall {
            bb,
            orient,
            gen_depth,
        }
        | ShPiece::StraightStairsDown {
            bb,
            orient,
            gen_depth,
        } => {
            child_forward(bb, orient, gen_depth, 1, 1, st, rng);
        }
        ShPiece::Straight {
            bb,
            orient,
            gen_depth,
            left_child,
            right_child,
        } => {
            child_forward(bb, orient, gen_depth, 1, 1, st, rng);
            if left_child {
                child_left(bb, orient, gen_depth, 1, 2, st, rng);
            }
            if right_child {
                child_right(bb, orient, gen_depth, 1, 2, st, rng);
            }
        }
        ShPiece::LeftTurn {
            bb,
            orient,
            gen_depth,
        } => {
            if matches!(orient, Direction::North | Direction::East) {
                child_left(bb, orient, gen_depth, 1, 1, st, rng);
            } else {
                child_right(bb, orient, gen_depth, 1, 1, st, rng);
            }
        }
        ShPiece::RightTurn {
            bb,
            orient,
            gen_depth,
        } => {
            if matches!(orient, Direction::North | Direction::East) {
                child_right(bb, orient, gen_depth, 1, 1, st, rng);
            } else {
                child_left(bb, orient, gen_depth, 1, 1, st, rng);
            }
        }
        ShPiece::RoomCrossing {
            bb,
            orient,
            gen_depth,
        } => {
            child_forward(bb, orient, gen_depth, 4, 1, st, rng);
            child_left(bb, orient, gen_depth, 1, 4, st, rng);
            child_right(bb, orient, gen_depth, 1, 4, st, rng);
        }
        ShPiece::FiveCrossing {
            bb,
            orient,
            gen_depth,
            left_low,
            left_high,
            right_low,
            right_high,
        } => {
            let (y_off_a, y_off_b) = if matches!(orient, Direction::West | Direction::North) {
                (5, 3)
            } else {
                (3, 5)
            };
            child_forward(bb, orient, gen_depth, 5, 1, st, rng);
            if left_low {
                child_left(bb, orient, gen_depth, y_off_a, 1, st, rng);
            }
            if left_high {
                child_left(bb, orient, gen_depth, y_off_b, 7, st, rng);
            }
            if right_low {
                child_right(bb, orient, gen_depth, y_off_a, 1, st, rng);
            }
            if right_high {
                child_right(bb, orient, gen_depth, y_off_b, 7, st, rng);
            }
        }
        ShPiece::PortalRoom { .. } => {
            st.has_portal_room = true;
        }
        ShPiece::FillerCorridor { .. } | ShPiece::Library { .. } => {}
    }
}
