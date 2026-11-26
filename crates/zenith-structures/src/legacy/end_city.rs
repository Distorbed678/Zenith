//! End City (`minecraft:end_city`) recursive section-generator tree
//! (`docs/specs/worldgen/structures-legacy-pieces.md` §14).

use zenith_math::{LegacyRandom, RandomSource};
use zenith_registry::Identifier;

use crate::geom::{BlockPos, BoundingBox, BoundingBoxExt, Direction, Mirror, Rotation};
use crate::legacy::{LegacyGenContext, LegacyPiece, get_lowest_y_in_5by5_box};
use crate::piece_data::{PieceData, TemplateKind, TemplatePiece};
use crate::template::TemplateManager;

const MAX_GEN_DEPTH: i32 = 8;

const TOWER_BRIDGES: [(Rotation, BlockPos); 4] = [
    (Rotation::None, BlockPos::new(1, -1, 0)),
    (Rotation::Clockwise90, BlockPos::new(6, -1, 1)),
    (Rotation::Counterclockwise90, BlockPos::new(0, -1, 5)),
    (Rotation::Clockwise180, BlockPos::new(5, -1, 6)),
];

const FAT_TOWER_BRIDGES: [(Rotation, BlockPos); 4] = [
    (Rotation::None, BlockPos::new(4, -1, 0)),
    (Rotation::Clockwise90, BlockPos::new(12, -1, 4)),
    (Rotation::Counterclockwise90, BlockPos::new(0, -1, 8)),
    (Rotation::Clockwise180, BlockPos::new(8, -1, 12)),
];

#[derive(Debug, Clone)]
struct EcPiece {
    name: String,
    template_pos: BlockPos,
    rot: Rotation,
    bb: BoundingBox,
    gen_depth: i32,
}

#[derive(Debug, Clone, Copy)]
enum SectionGen {
    HouseTower,
    Tower,
    TowerBridge,
    FatTower,
}

/// Generates pieces for `minecraft:end_city` (§14.1–§14.2).
pub fn generate(
    chunk_x: i32,
    chunk_z: i32,
    rng: &mut LegacyRandom,
    ctx: &mut dyn LegacyGenContext,
) -> Option<Vec<LegacyPiece>> {
    let block_x = (chunk_x << 4) + 7;
    let block_z = (chunk_z << 4) + 7;
    if !ctx.could_valid_biome_exist_in_terrain_column(block_x, block_z) {
        return None;
    }
    let rotation = Rotation::random(rng);
    let start_pos = get_lowest_y_in_5by5_box(ctx, block_x, block_z, rotation);
    if start_pos.y < 60 {
        return None;
    }
    if !ctx.is_valid_biome(start_pos) {
        return None;
    }

    let tm = ctx.template_manager();
    let mut ship_created = false;
    let mut pieces: Vec<EcPiece> = Vec::new();

    let p0 = make_piece(tm, start_pos, "base_floor", rotation)?;
    pieces.push(p0.clone());
    let p1 = add_piece(
        tm,
        &p0,
        BlockPos::new(-1, 0, -1),
        "second_floor_1",
        rotation,
    )?;
    pieces.push(p1.clone());
    let p2 = add_piece(tm, &p1, BlockPos::new(-1, 4, -1), "third_floor_1", rotation)?;
    pieces.push(p2.clone());
    let p3 = add_piece(tm, &p2, BlockPos::new(-1, 8, -1), "third_roof", rotation)?;
    pieces.push(p3.clone());

    recursive_children(
        tm,
        SectionGen::Tower,
        1,
        &p3,
        None,
        &mut pieces,
        &mut ship_created,
        rng,
    );

    Some(
        pieces
            .into_iter()
            .map(|p| {
                let overwrite = !matches!(
                    p.name.as_str(),
                    "second_floor_1"
                        | "third_floor_1"
                        | "second_floor_2"
                        | "third_floor_2"
                        | "second_roof"
                );
                LegacyPiece::new("minecraft:ecp", p.bb, Some(Direction::North), p.gen_depth)
                    .with_data(PieceData::Template(Box::new(TemplatePiece {
                        kind: TemplateKind::EndCity { overwrite },
                        template: Identifier::with_default_namespace(format!(
                            "end_city/{}",
                            p.name
                        ))
                        .expect("valid"),
                        position: p.template_pos,
                        rotation: p.rot,
                        mirror: Mirror::None,
                        pivot: BlockPos::ZERO,
                        height_adjusted: false,
                    })))
            })
            .collect(),
    )
}

fn make_piece(tm: &TemplateManager, pos: BlockPos, name: &str, rot: Rotation) -> Option<EcPiece> {
    let id = Identifier::with_default_namespace(format!("end_city/{name}")).ok()?;
    let tpl = tm.get(&id).ok()?;
    let bb = tpl.bounding_box(pos, rot, BlockPos::ZERO, Mirror::None);
    Some(EcPiece {
        name: name.to_owned(),
        template_pos: pos,
        rot,
        bb,
        gen_depth: 0,
    })
}

fn add_piece(
    tm: &TemplateManager,
    parent: &EcPiece,
    offset: BlockPos,
    name: &str,
    rot: Rotation,
) -> Option<EcPiece> {
    let delta = parent.rot.rotate_pos(offset);
    let pos = parent.template_pos.add(delta);
    make_piece(tm, pos, name, rot)
}

#[allow(clippy::too_many_arguments)]
fn recursive_children(
    tm: &TemplateManager,
    section_gen: SectionGen,
    gen_depth: i32,
    parent: &EcPiece,
    offset: Option<BlockPos>,
    pieces: &mut Vec<EcPiece>,
    ship_created: &mut bool,
    rng: &mut LegacyRandom,
) -> bool {
    if gen_depth > MAX_GEN_DEPTH {
        return false;
    }
    let mut child_pieces: Vec<EcPiece> = Vec::new();
    let ok = match section_gen {
        SectionGen::HouseTower => gen_house_tower(
            tm,
            gen_depth,
            parent,
            offset.unwrap_or(BlockPos::ZERO),
            &mut child_pieces,
            ship_created,
            rng,
        ),
        SectionGen::Tower => gen_tower(tm, gen_depth, parent, &mut child_pieces, ship_created, rng),
        SectionGen::TowerBridge => {
            gen_tower_bridge(tm, gen_depth, parent, &mut child_pieces, ship_created, rng)
        }
        SectionGen::FatTower => {
            gen_fat_tower(tm, gen_depth, parent, &mut child_pieces, ship_created, rng)
        }
    };
    if ok {
        let mut collision = false;
        let child_tag = rng.next_int();
        for child in &mut child_pieces {
            child.gen_depth = child_tag;
            if let Some(col) = pieces.iter().find(|p| p.bb.intersects(child.bb))
                && col.gen_depth != parent.gen_depth
            {
                collision = true;
                break;
            }
        }
        if !collision {
            pieces.extend(child_pieces);
            return true;
        }
    }
    false
}

fn gen_house_tower(
    tm: &TemplateManager,
    gen_depth: i32,
    parent: &EcPiece,
    offset: BlockPos,
    pieces: &mut Vec<EcPiece>,
    ship_created: &mut bool,
    rng: &mut LegacyRandom,
) -> bool {
    if gen_depth > MAX_GEN_DEPTH {
        return false;
    }
    let rot = parent.rot;
    let Some(mut last) = add_piece(tm, parent, offset, "base_floor", rot) else {
        return false;
    };
    pieces.push(last.clone());
    let num_floors = rng.next_int_bounded(3);
    if num_floors == 0 {
        if let Some(p) = add_piece(tm, &last, BlockPos::new(-1, 4, -1), "base_roof", rot) {
            pieces.push(p);
        }
    } else if num_floors == 1 {
        if let Some(p) = add_piece(tm, &last, BlockPos::new(-1, 0, -1), "second_floor_2", rot) {
            pieces.push(p.clone());
            last = p;
        }
        if let Some(p) = add_piece(tm, &last, BlockPos::new(-1, 8, -1), "second_roof", rot) {
            pieces.push(p.clone());
            last = p;
        }
        recursive_children(
            tm,
            SectionGen::Tower,
            gen_depth + 1,
            &last,
            None,
            pieces,
            ship_created,
            rng,
        );
    } else if num_floors == 2 {
        if let Some(p) = add_piece(tm, &last, BlockPos::new(-1, 0, -1), "second_floor_2", rot) {
            pieces.push(p.clone());
            last = p;
        }
        if let Some(p) = add_piece(tm, &last, BlockPos::new(-1, 4, -1), "third_floor_2", rot) {
            pieces.push(p.clone());
            last = p;
        }
        if let Some(p) = add_piece(tm, &last, BlockPos::new(-1, 8, -1), "third_roof", rot) {
            pieces.push(p.clone());
            last = p;
        }
        recursive_children(
            tm,
            SectionGen::Tower,
            gen_depth + 1,
            &last,
            None,
            pieces,
            ship_created,
            rng,
        );
    }
    true
}

fn gen_tower(
    tm: &TemplateManager,
    gen_depth: i32,
    parent: &EcPiece,
    pieces: &mut Vec<EcPiece>,
    ship_created: &mut bool,
    rng: &mut LegacyRandom,
) -> bool {
    let rot = parent.rot;
    let ox = 3 + rng.next_int_bounded(2);
    let oz = 3 + rng.next_int_bounded(2);
    let Some(mut last) = add_piece(tm, parent, BlockPos::new(ox, -3, oz), "tower_base", rot) else {
        return false;
    };
    pieces.push(last.clone());
    if let Some(p) = add_piece(tm, &last, BlockPos::new(0, 7, 0), "tower_piece", rot) {
        pieces.push(p.clone());
        last = p;
    }
    let mut bridge_piece = if rng.next_int_bounded(3) == 0 {
        Some(last.clone())
    } else {
        None
    };
    let tower_height = 1 + rng.next_int_bounded(3);
    for i in 0..tower_height {
        if let Some(p) = add_piece(tm, &last, BlockPos::new(0, 4, 0), "tower_piece", rot) {
            pieces.push(p.clone());
            last = p;
        }
        if i < tower_height - 1 && rng.next_boolean() {
            bridge_piece = Some(last.clone());
        }
    }
    if let Some(bp) = bridge_piece {
        for (bridge_rot, bridge_off) in TOWER_BRIDGES {
            if rng.next_boolean()
                && let Some(end) = add_piece(
                    tm,
                    &bp,
                    bridge_off,
                    "bridge_end",
                    rot.get_rotated(bridge_rot),
                )
            {
                pieces.push(end.clone());
                recursive_children(
                    tm,
                    SectionGen::TowerBridge,
                    gen_depth + 1,
                    &end,
                    None,
                    pieces,
                    ship_created,
                    rng,
                );
            }
        }
        if let Some(top) = add_piece(tm, &last, BlockPos::new(-1, 4, -1), "tower_top", rot) {
            pieces.push(top);
        }
        true
    } else if gen_depth != 7 {
        recursive_children(
            tm,
            SectionGen::FatTower,
            gen_depth + 1,
            &last,
            None,
            pieces,
            ship_created,
            rng,
        )
    } else {
        if let Some(top) = add_piece(tm, &last, BlockPos::new(-1, 4, -1), "tower_top", rot) {
            pieces.push(top);
        }
        true
    }
}

fn gen_tower_bridge(
    tm: &TemplateManager,
    gen_depth: i32,
    parent: &EcPiece,
    pieces: &mut Vec<EcPiece>,
    ship_created: &mut bool,
    rng: &mut LegacyRandom,
) -> bool {
    let rot = parent.rot;
    let bridge_length = rng.next_int_bounded(4) + 1;
    let Some(mut last) = add_piece(tm, parent, BlockPos::new(0, 0, -4), "bridge_piece", rot) else {
        return false;
    };
    last.gen_depth = -1;
    pieces.push(last.clone());

    let mut next_y = 0_i32;
    for _ in 0..bridge_length {
        if rng.next_boolean() {
            if let Some(p) = add_piece(tm, &last, BlockPos::new(0, next_y, -4), "bridge_piece", rot)
            {
                pieces.push(p.clone());
                last = p;
            }
            next_y = 0;
        } else {
            if rng.next_boolean() {
                if let Some(p) = add_piece(
                    tm,
                    &last,
                    BlockPos::new(0, next_y, -4),
                    "bridge_steep_stairs",
                    rot,
                ) {
                    pieces.push(p.clone());
                    last = p;
                }
            } else if let Some(p) = add_piece(
                tm,
                &last,
                BlockPos::new(0, next_y, -8),
                "bridge_gentle_stairs",
                rot,
            ) {
                pieces.push(p.clone());
                last = p;
            }
            next_y = 4;
        }
    }

    if !*ship_created && rng.next_int_bounded(10 - gen_depth) == 0 {
        let sx = -8 + rng.next_int_bounded(8);
        let sz = -70 + rng.next_int_bounded(10);
        if let Some(ship) = add_piece(tm, &last, BlockPos::new(sx, next_y, sz), "ship", rot) {
            pieces.push(ship);
        }
        *ship_created = true;
    } else if !recursive_children(
        tm,
        SectionGen::HouseTower,
        gen_depth + 1,
        &last,
        Some(BlockPos::new(-3, next_y + 1, -11)),
        pieces,
        ship_created,
        rng,
    ) {
        return false;
    }

    if let Some(mut end) = add_piece(
        tm,
        &last,
        BlockPos::new(4, next_y, 0),
        "bridge_end",
        rot.get_rotated(Rotation::Clockwise180),
    ) {
        end.gen_depth = -1;
        pieces.push(end);
    }
    true
}

fn gen_fat_tower(
    tm: &TemplateManager,
    gen_depth: i32,
    parent: &EcPiece,
    pieces: &mut Vec<EcPiece>,
    ship_created: &mut bool,
    rng: &mut LegacyRandom,
) -> bool {
    let rot = parent.rot;
    let Some(mut last) = add_piece(tm, parent, BlockPos::new(-3, 4, -3), "fat_tower_base", rot)
    else {
        return false;
    };
    pieces.push(last.clone());
    if let Some(p) = add_piece(tm, &last, BlockPos::new(0, 4, 0), "fat_tower_middle", rot) {
        pieces.push(p.clone());
        last = p;
    }

    for _ in 0..2 {
        if rng.next_int_bounded(3) == 0 {
            break;
        }
        if let Some(p) = add_piece(tm, &last, BlockPos::new(0, 8, 0), "fat_tower_middle", rot) {
            pieces.push(p.clone());
            last = p;
        }
        for (bridge_rot, bridge_off) in FAT_TOWER_BRIDGES {
            if rng.next_boolean()
                && let Some(end) = add_piece(
                    tm,
                    &last,
                    bridge_off,
                    "bridge_end",
                    rot.get_rotated(bridge_rot),
                )
            {
                pieces.push(end.clone());
                recursive_children(
                    tm,
                    SectionGen::TowerBridge,
                    gen_depth + 1,
                    &end,
                    None,
                    pieces,
                    ship_created,
                    rng,
                );
            }
        }
    }

    if let Some(top) = add_piece(tm, &last, BlockPos::new(-2, 8, -2), "fat_tower_top", rot) {
        pieces.push(top);
    }
    true
}
