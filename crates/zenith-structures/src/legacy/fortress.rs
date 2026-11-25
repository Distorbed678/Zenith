//! Nether Fortress (`minecraft:fortress`) piece-graph generator
//! (`docs/specs/worldgen/structures-legacy-pieces.md` §4).

use zenith_math::{LegacyRandom, RandomSource};

use crate::geom::{BlockPos, BoundingBox, BoundingBoxExt, Direction, random_horizontal_direction};
use crate::legacy::{LegacyGenContext, LegacyPiece, has_collision, move_inside_heights};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FortressKind {
    // Bridge pool
    BridgeStraight,
    BridgeCrossing,
    RoomCrossing,
    StairsRoom,
    MonsterThrone,
    CastleEntrance,
    // Castle pool
    CastleSmallCorridor,
    CastleSmallCorridorCrossing,
    CastleSmallCorridorRightTurn,
    CastleSmallCorridorLeftTurn,
    CastleCorridorStairs,
    CastleCorridorTBalcony,
    CastleStalkRoom,
    // Special
    StartPiece,
    BridgeEndFiller,
}

#[derive(Debug, Clone, Copy)]
struct PieceWeight {
    kind: FortressKind,
    weight: i32,
    max_place_count: i32,
    allow_in_row: bool,
    place_count: i32,
}

impl PieceWeight {
    fn is_valid(&self) -> bool {
        self.max_place_count == 0 || self.place_count < self.max_place_count
    }

    fn do_place(&self, _depth: i32) -> bool {
        self.is_valid()
    }
}

fn bridge_weights() -> Vec<PieceWeight> {
    vec![
        PieceWeight {
            kind: FortressKind::BridgeStraight,
            weight: 30,
            max_place_count: 0,
            allow_in_row: true,
            place_count: 0,
        },
        PieceWeight {
            kind: FortressKind::BridgeCrossing,
            weight: 10,
            max_place_count: 4,
            allow_in_row: false,
            place_count: 0,
        },
        PieceWeight {
            kind: FortressKind::RoomCrossing,
            weight: 10,
            max_place_count: 4,
            allow_in_row: false,
            place_count: 0,
        },
        PieceWeight {
            kind: FortressKind::StairsRoom,
            weight: 10,
            max_place_count: 3,
            allow_in_row: false,
            place_count: 0,
        },
        PieceWeight {
            kind: FortressKind::MonsterThrone,
            weight: 5,
            max_place_count: 2,
            allow_in_row: false,
            place_count: 0,
        },
        PieceWeight {
            kind: FortressKind::CastleEntrance,
            weight: 5,
            max_place_count: 1,
            allow_in_row: false,
            place_count: 0,
        },
    ]
}

fn castle_weights() -> Vec<PieceWeight> {
    vec![
        PieceWeight {
            kind: FortressKind::CastleSmallCorridor,
            weight: 25,
            max_place_count: 0,
            allow_in_row: true,
            place_count: 0,
        },
        PieceWeight {
            kind: FortressKind::CastleSmallCorridorCrossing,
            weight: 15,
            max_place_count: 5,
            allow_in_row: false,
            place_count: 0,
        },
        PieceWeight {
            kind: FortressKind::CastleSmallCorridorRightTurn,
            weight: 5,
            max_place_count: 10,
            allow_in_row: false,
            place_count: 0,
        },
        PieceWeight {
            kind: FortressKind::CastleSmallCorridorLeftTurn,
            weight: 5,
            max_place_count: 10,
            allow_in_row: false,
            place_count: 0,
        },
        PieceWeight {
            kind: FortressKind::CastleCorridorStairs,
            weight: 10,
            max_place_count: 3,
            allow_in_row: true,
            place_count: 0,
        },
        PieceWeight {
            kind: FortressKind::CastleCorridorTBalcony,
            weight: 7,
            max_place_count: 2,
            allow_in_row: false,
            place_count: 0,
        },
        PieceWeight {
            kind: FortressKind::CastleStalkRoom,
            weight: 5,
            max_place_count: 2,
            allow_in_row: false,
            place_count: 0,
        },
    ]
}

#[derive(Debug, Clone, Copy)]
struct FPiece {
    kind: FortressKind,
    bb: BoundingBox,
    orient: Direction,
    gen_depth: i32,
}

impl FPiece {
    fn piece_id(&self) -> &'static str {
        match self.kind {
            FortressKind::StartPiece | FortressKind::BridgeCrossing => "minecraft:nebcr",
            FortressKind::BridgeEndFiller => "minecraft:nebef",
            FortressKind::BridgeStraight => "minecraft:nebs",
            FortressKind::RoomCrossing => "minecraft:nerc",
            FortressKind::StairsRoom => "minecraft:nesr",
            FortressKind::MonsterThrone => "minecraft:nemt",
            FortressKind::CastleEntrance => "minecraft:nece",
            FortressKind::CastleSmallCorridor => "minecraft:nesc",
            FortressKind::CastleSmallCorridorCrossing => "minecraft:nescsc",
            FortressKind::CastleSmallCorridorLeftTurn => "minecraft:nesclt",
            FortressKind::CastleSmallCorridorRightTurn => "minecraft:nescrt",
            FortressKind::CastleCorridorStairs => "minecraft:neccs",
            FortressKind::CastleCorridorTBalcony => "minecraft:nectb",
            FortressKind::CastleStalkRoom => "minecraft:necsr",
        }
    }

    fn to_legacy(self) -> LegacyPiece {
        LegacyPiece::new(self.piece_id(), self.bb, Some(self.orient), self.gen_depth)
    }
}

struct State {
    start_bb: BoundingBox,
    bridge_pool: Vec<PieceWeight>,
    castle_pool: Vec<PieceWeight>,
    previous: Option<FortressKind>,
    pending: Vec<FPiece>,
    pieces: Vec<LegacyPiece>,
}

/// Generates pieces for `minecraft:fortress` (§4.2–§4.4).
pub fn generate(
    chunk_x: i32,
    chunk_z: i32,
    rng: &mut LegacyRandom,
    ctx: &mut dyn LegacyGenContext,
) -> Option<Vec<LegacyPiece>> {
    // §4.2: stub position is (chunkMinX, 64, chunkMinZ).
    let stub_pos = BlockPos::new(chunk_x << 4, 64, chunk_z << 4);
    if !ctx.is_valid_biome(stub_pos) {
        return None;
    }

    let dir = random_horizontal_direction(rng);
    let start_bb =
        BoundingBox::make_box((chunk_x << 4) + 2, 64, (chunk_z << 4) + 2, dir, 19, 10, 19);
    let start = FPiece {
        kind: FortressKind::StartPiece,
        bb: start_bb,
        orient: dir,
        gen_depth: 0,
    };

    let mut st = State {
        start_bb,
        bridge_pool: bridge_weights(),
        castle_pool: castle_weights(),
        previous: None,
        pending: Vec::new(),
        pieces: vec![start.to_legacy()],
    };

    add_children(start, &mut st, rng);
    while !st.pending.is_empty() {
        let idx = rng.next_int_bounded(st.pending.len() as i32) as usize;
        let next = st.pending.remove(idx);
        add_children(next, &mut st, rng);
    }

    move_inside_heights(&mut st.pieces, rng, 48, 70);
    Some(st.pieces)
}

#[inline]
fn is_ok_box(bb: BoundingBox) -> bool {
    bb.min_y > 10
}

#[allow(clippy::too_many_arguments)]
fn create_piece(
    kind: FortressKind,
    pieces: &[LegacyPiece],
    rng: &mut LegacyRandom,
    foot_x: i32,
    foot_y: i32,
    foot_z: i32,
    dir: Direction,
    depth: i32,
) -> Option<FPiece> {
    let bb = match kind {
        FortressKind::BridgeStraight => {
            BoundingBox::orient_box(foot_x, foot_y, foot_z, -1, -3, 0, 5, 10, 19, dir)
        }
        FortressKind::BridgeCrossing | FortressKind::StartPiece => {
            BoundingBox::orient_box(foot_x, foot_y, foot_z, -8, -3, 0, 19, 10, 19, dir)
        }
        FortressKind::RoomCrossing => {
            BoundingBox::orient_box(foot_x, foot_y, foot_z, -2, 0, 0, 7, 9, 7, dir)
        }
        FortressKind::StairsRoom => {
            BoundingBox::orient_box(foot_x, foot_y, foot_z, -2, 0, 0, 7, 11, 7, dir)
        }
        FortressKind::MonsterThrone => {
            BoundingBox::orient_box(foot_x, foot_y, foot_z, -2, 0, 0, 7, 8, 9, dir)
        }
        FortressKind::CastleEntrance | FortressKind::CastleStalkRoom => {
            BoundingBox::orient_box(foot_x, foot_y, foot_z, -5, -3, 0, 13, 14, 13, dir)
        }
        FortressKind::CastleSmallCorridor
        | FortressKind::CastleSmallCorridorCrossing
        | FortressKind::CastleSmallCorridorLeftTurn
        | FortressKind::CastleSmallCorridorRightTurn => {
            BoundingBox::orient_box(foot_x, foot_y, foot_z, -1, 0, 0, 5, 7, 5, dir)
        }
        FortressKind::CastleCorridorStairs => {
            BoundingBox::orient_box(foot_x, foot_y, foot_z, -1, -7, 0, 5, 14, 10, dir)
        }
        FortressKind::CastleCorridorTBalcony => {
            BoundingBox::orient_box(foot_x, foot_y, foot_z, -3, 0, 0, 9, 7, 9, dir)
        }
        FortressKind::BridgeEndFiller => {
            BoundingBox::orient_box(foot_x, foot_y, foot_z, -1, -3, 0, 5, 10, 8, dir)
        }
    };
    if !is_ok_box(bb) || has_collision(pieces, bb) {
        return None;
    }
    match kind {
        FortressKind::CastleSmallCorridorLeftTurn | FortressKind::CastleSmallCorridorRightTurn => {
            let _is_needing_chest = rng.next_int_bounded(3) == 0;
        }
        FortressKind::BridgeEndFiller => {
            let _self_seed = rng.next_int();
        }
        _ => {}
    }
    Some(FPiece {
        kind,
        bb,
        orient: dir,
        gen_depth: depth,
    })
}

#[allow(clippy::too_many_arguments)]
fn generate_piece(
    st: &mut State,
    is_castle: bool,
    rng: &mut LegacyRandom,
    foot_x: i32,
    foot_y: i32,
    foot_z: i32,
    dir: Direction,
    depth: i32,
) -> Option<FPiece> {
    let pool = if is_castle {
        &mut st.castle_pool
    } else {
        &mut st.bridge_pool
    };
    let mut total_weight = 0_i32;
    let mut has_any_limited = false;
    for pw in pool.iter() {
        if pw.max_place_count > 0 && pw.place_count < pw.max_place_count {
            has_any_limited = true;
        }
        total_weight += pw.weight;
    }
    if !has_any_limited {
        total_weight = -1;
    }

    if total_weight > 0 && depth <= 30 {
        for _ in 0..5 {
            let mut w = rng.next_int_bounded(total_weight);
            let mut i = 0;
            while i < pool.len() {
                w -= pool[i].weight;
                if w < 0 {
                    let kind = pool[i].kind;
                    let allow = pool[i].allow_in_row;
                    if !pool[i].do_place(depth) || (st.previous == Some(kind) && !allow) {
                        break;
                    }
                    if let Some(piece) =
                        create_piece(kind, &st.pieces, rng, foot_x, foot_y, foot_z, dir, depth)
                    {
                        pool[i].place_count += 1;
                        st.previous = Some(kind);
                        if !pool[i].is_valid() {
                            pool.remove(i);
                        }
                        return Some(piece);
                    }
                }
                i += 1;
            }
        }
    }

    create_piece(
        FortressKind::BridgeEndFiller,
        &st.pieces,
        rng,
        foot_x,
        foot_y,
        foot_z,
        dir,
        depth,
    )
}

#[allow(clippy::too_many_arguments)]
fn generate_and_add_piece(
    st: &mut State,
    rng: &mut LegacyRandom,
    foot_x: i32,
    foot_y: i32,
    foot_z: i32,
    dir: Direction,
    depth: i32,
    is_castle: bool,
) {
    if (foot_x - st.start_bb.min_x).abs() <= 112 && (foot_z - st.start_bb.min_z).abs() <= 112 {
        if let Some(child) =
            generate_piece(st, is_castle, rng, foot_x, foot_y, foot_z, dir, depth + 1)
        {
            st.pieces.push(child.to_legacy());
            st.pending.push(child);
        }
    } else {
        // Out-of-bounds BridgeEndFiller quirk (§4.3 & edge case #7): consumes RNG if valid, not added!
        let _ = create_piece(
            FortressKind::BridgeEndFiller,
            &st.pieces,
            rng,
            foot_x,
            foot_y,
            foot_z,
            dir,
            depth,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn child_forward(
    bb: BoundingBox,
    orient: Direction,
    depth: i32,
    x_off: i32,
    y_off: i32,
    is_castle: bool,
    st: &mut State,
    rng: &mut LegacyRandom,
) {
    match orient {
        Direction::North => generate_and_add_piece(
            st,
            rng,
            bb.min_x + x_off,
            bb.min_y + y_off,
            bb.min_z - 1,
            orient,
            depth,
            is_castle,
        ),
        Direction::South => generate_and_add_piece(
            st,
            rng,
            bb.min_x + x_off,
            bb.min_y + y_off,
            bb.max_z + 1,
            orient,
            depth,
            is_castle,
        ),
        Direction::West => generate_and_add_piece(
            st,
            rng,
            bb.min_x - 1,
            bb.min_y + y_off,
            bb.min_z + x_off,
            orient,
            depth,
            is_castle,
        ),
        Direction::East => generate_and_add_piece(
            st,
            rng,
            bb.max_x + 1,
            bb.min_y + y_off,
            bb.min_z + x_off,
            orient,
            depth,
            is_castle,
        ),
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn child_left(
    bb: BoundingBox,
    orient: Direction,
    depth: i32,
    y_off: i32,
    z_off: i32,
    is_castle: bool,
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
                is_castle,
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
                is_castle,
            );
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn child_right(
    bb: BoundingBox,
    orient: Direction,
    depth: i32,
    y_off: i32,
    z_off: i32,
    is_castle: bool,
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
                is_castle,
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
                is_castle,
            );
        }
        _ => {}
    }
}

fn add_children(piece: FPiece, st: &mut State, rng: &mut LegacyRandom) {
    let bb = piece.bb;
    let o = piece.orient;
    let d = piece.gen_depth;
    match piece.kind {
        FortressKind::StartPiece | FortressKind::BridgeCrossing => {
            child_forward(bb, o, d, 8, 3, false, st, rng);
            child_left(bb, o, d, 3, 8, false, st, rng);
            child_right(bb, o, d, 3, 8, false, st, rng);
        }
        FortressKind::BridgeStraight => {
            child_forward(bb, o, d, 1, 3, false, st, rng);
        }
        FortressKind::RoomCrossing => {
            child_forward(bb, o, d, 2, 0, false, st, rng);
            child_left(bb, o, d, 0, 2, false, st, rng);
            child_right(bb, o, d, 0, 2, false, st, rng);
        }
        FortressKind::StairsRoom => {
            child_right(bb, o, d, 6, 2, false, st, rng);
        }
        FortressKind::CastleEntrance => {
            child_forward(bb, o, d, 5, 3, true, st, rng);
        }
        FortressKind::CastleSmallCorridor | FortressKind::CastleCorridorStairs => {
            child_forward(bb, o, d, 1, 0, true, st, rng);
        }
        FortressKind::CastleSmallCorridorCrossing => {
            child_forward(bb, o, d, 1, 0, true, st, rng);
            child_left(bb, o, d, 0, 1, true, st, rng);
            child_right(bb, o, d, 0, 1, true, st, rng);
        }
        FortressKind::CastleSmallCorridorLeftTurn => {
            child_left(bb, o, d, 0, 1, true, st, rng);
        }
        FortressKind::CastleSmallCorridorRightTurn => {
            child_right(bb, o, d, 0, 1, true, st, rng);
        }
        FortressKind::CastleCorridorTBalcony => {
            let z_off = if matches!(o, Direction::West | Direction::North) {
                5
            } else {
                1
            };
            let left_castle = rng.next_int_bounded(8) > 0;
            child_left(bb, o, d, 0, z_off, left_castle, st, rng);
            let right_castle = rng.next_int_bounded(8) > 0;
            child_right(bb, o, d, 0, z_off, right_castle, st, rng);
        }
        FortressKind::CastleStalkRoom => {
            child_forward(bb, o, d, 5, 3, true, st, rng);
            child_forward(bb, o, d, 5, 11, true, st, rng);
        }
        FortressKind::MonsterThrone | FortressKind::BridgeEndFiller => {}
    }
}
