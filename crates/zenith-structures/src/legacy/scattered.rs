//! Single-piece and template-backed legacy structure start generators:
//! Desert Pyramid (§7), Jungle Temple (§8), Swamp Hut (§9), Igloo (§10), Shipwreck (§11),
//! Ocean Ruins (§12), Buried Treasure (§13), Nether Fossil (§15), and Ruined Portal (§16)
//! (`docs/specs/worldgen/structures-legacy-pieces.md`).

use zenith_math::{LegacyRandom, RandomSource};
use zenith_registry::Identifier;
use zenith_worldgen::{HeightProvider, HeightmapKind};

use crate::data::{OceanRuinTemp, RuinedPortalPlacement, RuinedPortalSetup};
use crate::geom::{
    BlockPos, BoundingBox, BoundingBoxExt, Direction, Mirror, Rotation, mth_next_int,
    random_between_inclusive, random_horizontal_direction, sample_height_provider, transform_pos,
};
use crate::legacy::{
    LegacyGenContext, LegacyPiece, get_lowest_y, get_mean_first_occupied_height,
    on_top_of_chunk_center,
};
use crate::piece_data::{
    JungleTempleState, PieceData, RuinedPortalPiece, ScatteredState, SwampHutState, TemplateKind,
    TemplatePiece,
};
use crate::template::TemplateManager;

// ---------------------------------------------------------------------------
// §7 Desert Pyramid, §8 Jungle Temple, §9 Swamp Hut, §13 Buried Treasure
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn generate_single_piece(
    chunk_x: i32,
    chunk_z: i32,
    width: i32,
    height: i32,
    depth: i32,
    piece_kind: &'static str,
    data: PieceData,
    rng: &mut LegacyRandom,
    ctx: &mut dyn LegacyGenContext,
) -> Option<Vec<LegacyPiece>> {
    let mid_x = (chunk_x << 4) + 8;
    let mid_z = (chunk_z << 4) + 8;
    if !ctx.could_valid_biome_exist_in_terrain_column(mid_x, mid_z) {
        return None;
    }
    if get_lowest_y(ctx, chunk_x << 4, chunk_z << 4, width, depth) < ctx.sea_level() {
        return None;
    }
    let y = ctx.first_occupied_height(mid_x, mid_z, HeightmapKind::WorldSurfaceWg);
    if !ctx.is_valid_biome(BlockPos::new(mid_x, y, mid_z)) {
        return None;
    }
    let dir = random_horizontal_direction(rng);
    let bb = BoundingBox::make_box(chunk_x << 4, 64, chunk_z << 4, dir, width, height, depth);
    Some(vec![
        LegacyPiece::new(piece_kind, bb, Some(dir), 0).with_data(data),
    ])
}

/// §7 Desert Pyramid (`minecraft:desert_pyramid`, piece `minecraft:tedp`).
pub fn generate_desert_pyramid(
    chunk_x: i32,
    chunk_z: i32,
    rng: &mut LegacyRandom,
    ctx: &mut dyn LegacyGenContext,
) -> Option<Vec<LegacyPiece>> {
    generate_single_piece(
        chunk_x,
        chunk_z,
        21,
        15,
        21,
        "minecraft:tedp",
        PieceData::DesertPyramid(ScatteredState::default()),
        rng,
        ctx,
    )
}

/// §8 Jungle Temple (`minecraft:jungle_temple`, piece `minecraft:tejp`).
pub fn generate_jungle_temple(
    chunk_x: i32,
    chunk_z: i32,
    rng: &mut LegacyRandom,
    ctx: &mut dyn LegacyGenContext,
) -> Option<Vec<LegacyPiece>> {
    generate_single_piece(
        chunk_x,
        chunk_z,
        12,
        10,
        15,
        "minecraft:tejp",
        PieceData::JungleTemple(JungleTempleState::default()),
        rng,
        ctx,
    )
}

/// §9 Swamp Hut (`minecraft:swamp_hut`, piece `minecraft:tesh`).
pub fn generate_swamp_hut(
    chunk_x: i32,
    chunk_z: i32,
    rng: &mut LegacyRandom,
    ctx: &mut dyn LegacyGenContext,
) -> Option<Vec<LegacyPiece>> {
    on_top_of_chunk_center(
        chunk_x,
        chunk_z,
        HeightmapKind::WorldSurfaceWg,
        ctx,
        |_ctx| {
            let dir = random_horizontal_direction(rng);
            let bb = BoundingBox::make_box(chunk_x << 4, 64, chunk_z << 4, dir, 7, 7, 9);
            vec![
                LegacyPiece::new("minecraft:tesh", bb, Some(dir), 0)
                    .with_data(PieceData::SwampHut(SwampHutState::default())),
            ]
        },
    )
}

/// §13 Buried Treasure (`minecraft:buried_treasure`, piece `minecraft:btp`).
pub fn generate_buried_treasure(
    chunk_x: i32,
    chunk_z: i32,
    _rng: &mut LegacyRandom,
    ctx: &mut dyn LegacyGenContext,
) -> Option<Vec<LegacyPiece>> {
    on_top_of_chunk_center(chunk_x, chunk_z, HeightmapKind::OceanFloorWg, ctx, |_ctx| {
        let bb = BoundingBox::point((chunk_x << 4) + 9, 90, (chunk_z << 4) + 9);
        vec![LegacyPiece::new("minecraft:btp", bb, None, 0).with_data(PieceData::BuriedTreasure)]
    })
}

// ---------------------------------------------------------------------------
// §10 Igloo
// ---------------------------------------------------------------------------

fn igloo_piece(
    tm: &TemplateManager,
    name: &str,
    pivot: BlockPos,
    offset: BlockPos,
    start_pos: BlockPos,
    rot: Rotation,
    depth_offset: i32,
) -> Option<LegacyPiece> {
    let id = Identifier::with_default_namespace(name).ok()?;
    let tpl = tm.get(&id).ok()?;
    let template_pos = start_pos.add(offset).offset(0, -depth_offset, 0);
    let bb = tpl.bounding_box(template_pos, rot, pivot, Mirror::None);
    Some(
        LegacyPiece::new("minecraft:iglu", bb, Some(Direction::North), 0).with_data(
            PieceData::Template(Box::new(TemplatePiece {
                kind: TemplateKind::Igloo,
                template: id,
                position: template_pos,
                rotation: rot,
                mirror: Mirror::None,
                pivot,
                height_adjusted: false,
            })),
        ),
    )
}

/// §10 Igloo (`minecraft:igloo`, piece `minecraft:iglu`).
pub fn generate_igloo(
    chunk_x: i32,
    chunk_z: i32,
    rng: &mut LegacyRandom,
    ctx: &mut dyn LegacyGenContext,
) -> Option<Vec<LegacyPiece>> {
    on_top_of_chunk_center(
        chunk_x,
        chunk_z,
        HeightmapKind::WorldSurfaceWg,
        ctx,
        |ctx| {
            let tm = ctx.template_manager();
            let start_pos = BlockPos::new(chunk_x << 4, 90, chunk_z << 4);
            let rot = Rotation::random(rng);
            let mut pieces = Vec::new();
            if rng.next_double() < 0.5 {
                let depth = rng.next_int_bounded(8) + 4;
                if let Some(p) = igloo_piece(
                    tm,
                    "igloo/bottom",
                    BlockPos::new(3, 6, 7),
                    BlockPos::new(0, -3, -2),
                    start_pos,
                    rot,
                    depth * 3,
                ) {
                    pieces.push(p);
                }
                for i in 0..(depth - 1) {
                    if let Some(p) = igloo_piece(
                        tm,
                        "igloo/middle",
                        BlockPos::new(1, 3, 1),
                        BlockPos::new(2, -3, 4),
                        start_pos,
                        rot,
                        i * 3,
                    ) {
                        pieces.push(p);
                    }
                }
            }
            if let Some(p) = igloo_piece(
                tm,
                "igloo/top",
                BlockPos::new(3, 5, 5),
                BlockPos::ZERO,
                start_pos,
                rot,
                0,
            ) {
                pieces.push(p);
            }
            pieces
        },
    )
}

// ---------------------------------------------------------------------------
// §11 Shipwreck
// ---------------------------------------------------------------------------

const SHIPWRECK_BEACHED: [&str; 11] = [
    "shipwreck/with_mast",
    "shipwreck/sideways_full",
    "shipwreck/sideways_fronthalf",
    "shipwreck/sideways_backhalf",
    "shipwreck/rightsideup_full",
    "shipwreck/rightsideup_fronthalf",
    "shipwreck/rightsideup_backhalf",
    "shipwreck/with_mast_degraded",
    "shipwreck/rightsideup_full_degraded",
    "shipwreck/rightsideup_fronthalf_degraded",
    "shipwreck/rightsideup_backhalf_degraded",
];

const SHIPWRECK_OCEAN: [&str; 20] = [
    "shipwreck/with_mast",
    "shipwreck/upsidedown_full",
    "shipwreck/upsidedown_fronthalf",
    "shipwreck/upsidedown_backhalf",
    "shipwreck/sideways_full",
    "shipwreck/sideways_fronthalf",
    "shipwreck/sideways_backhalf",
    "shipwreck/rightsideup_full",
    "shipwreck/rightsideup_fronthalf",
    "shipwreck/rightsideup_backhalf",
    "shipwreck/with_mast_degraded",
    "shipwreck/upsidedown_full_degraded",
    "shipwreck/upsidedown_fronthalf_degraded",
    "shipwreck/upsidedown_backhalf_degraded",
    "shipwreck/sideways_full_degraded",
    "shipwreck/sideways_fronthalf_degraded",
    "shipwreck/sideways_backhalf_degraded",
    "shipwreck/rightsideup_full_degraded",
    "shipwreck/rightsideup_fronthalf_degraded",
    "shipwreck/rightsideup_backhalf_degraded",
];

/// §11 Shipwreck (`minecraft:shipwreck`, piece `minecraft:shipwreck`).
pub fn generate_shipwreck(
    is_beached: bool,
    chunk_x: i32,
    chunk_z: i32,
    rng: &mut LegacyRandom,
    ctx: &mut dyn LegacyGenContext,
) -> Option<Vec<LegacyPiece>> {
    let hm = if is_beached {
        HeightmapKind::WorldSurfaceWg
    } else {
        HeightmapKind::OceanFloorWg
    };
    on_top_of_chunk_center(chunk_x, chunk_z, hm, ctx, |ctx| {
        let rot = Rotation::random(rng);
        let mut offset = BlockPos::new(chunk_x << 4, 90, chunk_z << 4);
        let name = if is_beached {
            SHIPWRECK_BEACHED[rng.next_int_bounded(SHIPWRECK_BEACHED.len() as i32) as usize]
        } else {
            SHIPWRECK_OCEAN[rng.next_int_bounded(SHIPWRECK_OCEAN.len() as i32) as usize]
        };
        let Ok(id) = Identifier::with_default_namespace(name) else {
            return Vec::new();
        };
        let Ok(tpl) = ctx.template_manager().get(&id) else {
            return Vec::new();
        };
        let pivot = BlockPos::new(4, 0, 15);
        let mut bb = tpl.bounding_box(offset, rot, pivot, Mirror::None);
        // Oversized check quirk (§11.2): checks X and Y (not Z) against 32.
        if tpl.size.x > 32 || tpl.size.y > 32 {
            let h = if is_beached {
                let min_y = get_lowest_y(ctx, bb.min_x, bb.min_z, bb.x_span(), bb.z_span());
                min_y - tpl.size.y / 2 - rng.next_int_bounded(3)
            } else {
                get_mean_first_occupied_height(ctx, bb.min_x, bb.x_span(), bb.min_z, bb.z_span())
            };
            offset.y = h;
            bb = tpl.bounding_box(offset, rot, pivot, Mirror::None);
        }
        vec![
            LegacyPiece::new("minecraft:shipwreck", bb, Some(Direction::North), 0).with_data(
                PieceData::Template(Box::new(TemplatePiece {
                    kind: TemplateKind::Shipwreck { is_beached },
                    template: id.clone(),
                    position: offset,
                    rotation: rot,
                    mirror: Mirror::None,
                    pivot,
                    height_adjusted: tpl.size.x > 32 || tpl.size.y > 32,
                })),
            ),
        ]
    })
}

// ---------------------------------------------------------------------------
// §12 Ocean Ruins
// ---------------------------------------------------------------------------

const WARM_RUINS: [&str; 8] = [
    "underwater_ruin/warm_1",
    "underwater_ruin/warm_2",
    "underwater_ruin/warm_3",
    "underwater_ruin/warm_4",
    "underwater_ruin/warm_5",
    "underwater_ruin/warm_6",
    "underwater_ruin/warm_7",
    "underwater_ruin/warm_8",
];
const BIG_WARM_RUINS: [&str; 4] = [
    "underwater_ruin/big_warm_4",
    "underwater_ruin/big_warm_5",
    "underwater_ruin/big_warm_6",
    "underwater_ruin/big_warm_7",
];
const RUINS_BRICK: [&str; 8] = [
    "underwater_ruin/brick_1",
    "underwater_ruin/brick_2",
    "underwater_ruin/brick_3",
    "underwater_ruin/brick_4",
    "underwater_ruin/brick_5",
    "underwater_ruin/brick_6",
    "underwater_ruin/brick_7",
    "underwater_ruin/brick_8",
];
const RUINS_CRACKED: [&str; 8] = [
    "underwater_ruin/cracked_1",
    "underwater_ruin/cracked_2",
    "underwater_ruin/cracked_3",
    "underwater_ruin/cracked_4",
    "underwater_ruin/cracked_5",
    "underwater_ruin/cracked_6",
    "underwater_ruin/cracked_7",
    "underwater_ruin/cracked_8",
];
const RUINS_MOSSY: [&str; 8] = [
    "underwater_ruin/mossy_1",
    "underwater_ruin/mossy_2",
    "underwater_ruin/mossy_3",
    "underwater_ruin/mossy_4",
    "underwater_ruin/mossy_5",
    "underwater_ruin/mossy_6",
    "underwater_ruin/mossy_7",
    "underwater_ruin/mossy_8",
];
const BIG_RUINS_BRICK: [&str; 4] = [
    "underwater_ruin/big_brick_1",
    "underwater_ruin/big_brick_2",
    "underwater_ruin/big_brick_3",
    "underwater_ruin/big_brick_8",
];
const BIG_RUINS_CRACKED: [&str; 4] = [
    "underwater_ruin/big_cracked_1",
    "underwater_ruin/big_cracked_2",
    "underwater_ruin/big_cracked_3",
    "underwater_ruin/big_cracked_8",
];
const BIG_RUINS_MOSSY: [&str; 4] = [
    "underwater_ruin/big_mossy_1",
    "underwater_ruin/big_mossy_2",
    "underwater_ruin/big_mossy_3",
    "underwater_ruin/big_mossy_8",
];

#[allow(clippy::too_many_arguments)]
fn add_ocean_ruin_piece(
    tm: &TemplateManager,
    pieces: &mut Vec<LegacyPiece>,
    pos: BlockPos,
    rot: Rotation,
    temp: OceanRuinTemp,
    is_large: bool,
    base_integrity: f32,
    rng: &mut LegacyRandom,
) {
    let push_tpl = |pieces: &mut Vec<LegacyPiece>, name: &str, integrity: f32| {
        if let Ok(id) = Identifier::with_default_namespace(name)
            && let Ok(tpl) = tm.get(&id)
        {
            let bb = tpl.bounding_box(pos, rot, BlockPos::ZERO, Mirror::None);
            pieces.push(
                LegacyPiece::new("minecraft:orp", bb, Some(Direction::North), 0).with_data(
                    PieceData::Template(Box::new(TemplatePiece {
                        kind: TemplateKind::OceanRuin {
                            integrity,
                            temp,
                            is_large,
                        },
                        template: id,
                        position: pos,
                        rotation: rot,
                        mirror: Mirror::None,
                        pivot: BlockPos::ZERO,
                        height_adjusted: false,
                    })),
                ),
            );
        }
    };
    match temp {
        OceanRuinTemp::Warm => {
            let name = if is_large {
                BIG_WARM_RUINS[rng.next_int_bounded(BIG_WARM_RUINS.len() as i32) as usize]
            } else {
                WARM_RUINS[rng.next_int_bounded(WARM_RUINS.len() as i32) as usize]
            };
            push_tpl(pieces, name, base_integrity);
        }
        OceanRuinTemp::Cold => {
            if is_large {
                let idx = rng.next_int_bounded(4) as usize;
                push_tpl(pieces, BIG_RUINS_BRICK[idx], base_integrity);
                push_tpl(pieces, BIG_RUINS_CRACKED[idx], 0.7);
                push_tpl(pieces, BIG_RUINS_MOSSY[idx], 0.5);
            } else {
                let idx = rng.next_int_bounded(8) as usize;
                push_tpl(pieces, RUINS_BRICK[idx], base_integrity);
                push_tpl(pieces, RUINS_CRACKED[idx], 0.7);
                push_tpl(pieces, RUINS_MOSSY[idx], 0.5);
            }
        }
    }
}

/// §12 Ocean Ruins (`minecraft:ocean_ruin`, piece `minecraft:orp`).
pub fn generate_ocean_ruin(
    biome_temp: OceanRuinTemp,
    large_probability: f32,
    cluster_probability: f32,
    chunk_x: i32,
    chunk_z: i32,
    rng: &mut LegacyRandom,
    ctx: &mut dyn LegacyGenContext,
) -> Option<Vec<LegacyPiece>> {
    on_top_of_chunk_center(chunk_x, chunk_z, HeightmapKind::OceanFloorWg, ctx, |ctx| {
        let tm = ctx.template_manager();
        let offset = BlockPos::new(chunk_x << 4, 90, chunk_z << 4);
        let rot = Rotation::random(rng);
        let is_large = rng.next_float() <= large_probability;
        let mut pieces = Vec::new();
        let base_integrity = if is_large { 0.9_f32 } else { 0.8_f32 };
        add_ocean_ruin_piece(
            tm,
            &mut pieces,
            offset,
            rot,
            biome_temp,
            is_large,
            base_integrity,
            rng,
        );

        if is_large && rng.next_float() <= cluster_probability {
            let parent_pos = BlockPos::new(offset.x, 90, offset.z);
            let parent_corner =
                transform_pos(BlockPos::new(15, 0, 15), Mirror::None, rot, BlockPos::ZERO)
                    .add(parent_pos);
            let parent_bb = BoundingBox::from_corners(parent_pos, parent_corner);
            let parent_bl = BlockPos::new(
                parent_pos.x.min(parent_corner.x),
                90,
                parent_pos.z.min(parent_corner.z),
            );

            let mut all_positions = vec![
                parent_bl.offset(
                    -16 + mth_next_int(rng, 1, 8),
                    0,
                    16 + mth_next_int(rng, 1, 7),
                ),
                parent_bl.offset(-16 + mth_next_int(rng, 1, 8), 0, mth_next_int(rng, 1, 7)),
                parent_bl.offset(
                    -16 + mth_next_int(rng, 1, 8),
                    0,
                    -16 + mth_next_int(rng, 4, 8),
                ),
                parent_bl.offset(mth_next_int(rng, 1, 7), 0, 16 + mth_next_int(rng, 1, 7)),
                parent_bl.offset(mth_next_int(rng, 1, 7), 0, -16 + mth_next_int(rng, 4, 6)),
                parent_bl.offset(
                    16 + mth_next_int(rng, 1, 7),
                    0,
                    16 + mth_next_int(rng, 3, 8),
                ),
                parent_bl.offset(16 + mth_next_int(rng, 1, 7), 0, mth_next_int(rng, 1, 7)),
                parent_bl.offset(
                    16 + mth_next_int(rng, 1, 7),
                    0,
                    -16 + mth_next_int(rng, 4, 8),
                ),
            ];

            let ruins = mth_next_int(rng, 4, 8);
            for _ in 0..ruins {
                if all_positions.is_empty() {
                    break;
                }
                let idx = rng.next_int_bounded(all_positions.len() as i32) as usize;
                let pos = all_positions.remove(idx);
                let next_rot = Rotation::random(rng);
                let next_corner = transform_pos(
                    BlockPos::new(5, 0, 6),
                    Mirror::None,
                    next_rot,
                    BlockPos::ZERO,
                )
                .add(pos);
                let next_bb = BoundingBox::from_corners(pos, next_corner);
                if !next_bb.intersects(parent_bb) {
                    add_ocean_ruin_piece(
                        tm,
                        &mut pieces,
                        pos,
                        next_rot,
                        biome_temp,
                        false,
                        0.8,
                        rng,
                    );
                }
            }
        }
        pieces
    })
}

// ---------------------------------------------------------------------------
// §15 Nether Fossil
// ---------------------------------------------------------------------------

const FOSSILS: [&str; 14] = [
    "nether_fossils/fossil_1",
    "nether_fossils/fossil_2",
    "nether_fossils/fossil_3",
    "nether_fossils/fossil_4",
    "nether_fossils/fossil_5",
    "nether_fossils/fossil_6",
    "nether_fossils/fossil_7",
    "nether_fossils/fossil_8",
    "nether_fossils/fossil_9",
    "nether_fossils/fossil_10",
    "nether_fossils/fossil_11",
    "nether_fossils/fossil_12",
    "nether_fossils/fossil_13",
    "nether_fossils/fossil_14",
];

/// §15 Nether Fossil (`minecraft:nether_fossil`, piece `minecraft:nefos`).
pub fn generate_nether_fossil(
    height_provider: &HeightProvider,
    chunk_x: i32,
    chunk_z: i32,
    rng: &mut LegacyRandom,
    ctx: &mut dyn LegacyGenContext,
) -> Option<Vec<LegacyPiece>> {
    let block_x = (chunk_x << 4) + rng.next_int_bounded(16);
    let block_z = (chunk_z << 4) + rng.next_int_bounded(16);
    let sea_level = ctx.sea_level();
    let mut y = sample_height_provider(height_provider, rng, ctx.world_gen_context());
    let col = ctx.base_column(block_x, block_z);
    let noise_min_y = ctx.noise_min_y();
    let get_col = |by: i32| -> u32 {
        let idx = by - noise_min_y;
        if idx >= 0 && (idx as usize) < col.len() {
            col[idx as usize]
        } else {
            0
        }
    };

    while y > sea_level {
        let cur = get_col(y);
        y -= 1;
        let below = get_col(y);
        if ctx.is_air(cur) && (ctx.is_soul_sand(below) || ctx.is_face_sturdy_up(below)) {
            break;
        }
    }
    if y <= sea_level {
        return None;
    }

    let pos = BlockPos::new(block_x, y, block_z);
    if !ctx.is_valid_biome(pos) {
        return None;
    }

    let rot = Rotation::random(rng);
    let name = FOSSILS[rng.next_int_bounded(FOSSILS.len() as i32) as usize];
    let id = Identifier::with_default_namespace(name).ok()?;
    let tpl = ctx.template_manager().get(&id).ok()?;
    let bb = tpl.bounding_box(pos, rot, BlockPos::ZERO, Mirror::None);
    Some(vec![
        LegacyPiece::new("minecraft:nefos", bb, Some(Direction::North), 0).with_data(
            PieceData::Template(Box::new(TemplatePiece {
                kind: TemplateKind::NetherFossil,
                template: id,
                position: pos,
                rotation: rot,
                mirror: Mirror::None,
                pivot: BlockPos::ZERO,
                height_adjusted: false,
            })),
        ),
    ])
}

// ---------------------------------------------------------------------------
// §16 Ruined Portal
// ---------------------------------------------------------------------------

const PORTALS: [&str; 10] = [
    "ruined_portal/portal_1",
    "ruined_portal/portal_2",
    "ruined_portal/portal_3",
    "ruined_portal/portal_4",
    "ruined_portal/portal_5",
    "ruined_portal/portal_6",
    "ruined_portal/portal_7",
    "ruined_portal/portal_8",
    "ruined_portal/portal_9",
    "ruined_portal/portal_10",
];
const GIANT_PORTALS: [&str; 3] = [
    "ruined_portal/giant_portal_1",
    "ruined_portal/giant_portal_2",
    "ruined_portal/giant_portal_3",
];

/// §16 Ruined Portal (`minecraft:ruined_portal`, piece `minecraft:rupo`).
pub fn generate_ruined_portal(
    setups: &[RuinedPortalSetup],
    chunk_x: i32,
    chunk_z: i32,
    rng: &mut LegacyRandom,
    ctx: &mut dyn LegacyGenContext,
) -> Option<Vec<LegacyPiece>> {
    if setups.is_empty() {
        return None;
    }
    // 1. Choose Setup
    let setup = if setups.len() == 1 {
        &setups[0]
    } else {
        let total: f32 = setups.iter().map(|s| s.weight).sum();
        let mut pick = rng.next_float();
        let mut chosen = &setups[0];
        for s in setups {
            pick -= s.weight / total;
            if pick < 0.0_f32 {
                chosen = s;
                break;
            }
        }
        chosen
    };

    // 2. airPocket
    let air_pocket = if setup.air_pocket_probability == 0.0_f32 {
        false
    } else if setup.air_pocket_probability == 1.0_f32 {
        true
    } else {
        rng.next_float() < setup.air_pocket_probability
    };

    // 3. Template
    let name = if rng.next_float() < 0.05_f32 {
        GIANT_PORTALS[rng.next_int_bounded(GIANT_PORTALS.len() as i32) as usize]
    } else {
        PORTALS[rng.next_int_bounded(PORTALS.len() as i32) as usize]
    };
    let id = Identifier::with_default_namespace(name).ok()?;
    let tpl = ctx.template_manager().get(&id).ok()?;

    // 4. Transform
    let rot = Rotation::random(rng);
    let mirror = if rng.next_float() < 0.5_f32 {
        Mirror::None
    } else {
        Mirror::FrontBack
    };
    let pivot = BlockPos::new(tpl.size.x / 2, 0, tpl.size.z / 2);

    // 5. Height selection
    let base_pos = BlockPos::new(chunk_x << 4, 0, chunk_z << 4);
    let bb = tpl.bounding_box(base_pos, rot, pivot, mirror);
    let center = bb.center();
    let hm_type = if setup.placement == RuinedPortalPlacement::OnOceanFloor {
        HeightmapKind::OceanFloorWg
    } else {
        HeightmapKind::WorldSurfaceWg
    };
    let surface_y = ctx.first_occupied_height(center.x, center.z, hm_type);

    let min_y = ctx.min_y() + 15;
    let new_y = match setup.placement {
        RuinedPortalPlacement::InNether => {
            if air_pocket {
                random_between_inclusive(rng, 32, 100)
            } else if rng.next_float() < 0.5_f32 {
                random_between_inclusive(rng, 27, 29)
            } else {
                random_between_inclusive(rng, 29, 100)
            }
        }
        RuinedPortalPlacement::InMountain => {
            let max_y = surface_y - bb.y_span();
            if 70 < max_y {
                random_between_inclusive(rng, 70, max_y)
            } else {
                max_y
            }
        }
        RuinedPortalPlacement::Underground => {
            let max_y = surface_y - bb.y_span();
            if min_y < max_y {
                random_between_inclusive(rng, min_y, max_y)
            } else {
                max_y
            }
        }
        RuinedPortalPlacement::PartlyBuried => {
            surface_y - bb.y_span() + random_between_inclusive(rng, 2, 8)
        }
        RuinedPortalPlacement::OnLandSurface | RuinedPortalPlacement::OnOceanFloor => surface_y,
    };

    let cols = [
        ctx.base_column(bb.min_x, bb.min_z),
        ctx.base_column(bb.max_x, bb.min_z),
        ctx.base_column(bb.min_x, bb.max_z),
        ctx.base_column(bb.max_x, bb.max_z),
    ];
    let noise_min_y = ctx.noise_min_y();
    let mut projected_y = new_y;
    while projected_y > min_y {
        let mut solid_corners = 0;
        for col in &cols {
            let idx = projected_y - noise_min_y;
            let state = if idx >= 0 && (idx as usize) < col.len() {
                col[idx as usize]
            } else {
                0
            };
            if ctx.is_heightmap_opaque(hm_type, state) {
                solid_corners += 1;
            }
        }
        if solid_corners >= 3 {
            break;
        }
        projected_y -= 1;
    }

    let origin = BlockPos::new(base_pos.x, projected_y, base_pos.z);
    if !ctx.is_valid_biome(origin) {
        return None;
    }
    let final_bb = tpl.bounding_box(origin, rot, pivot, mirror);
    let cold =
        setup.can_be_cold && ctx.cold_enough_to_snow(BlockPos::new(origin.x, origin.y, origin.z));
    Some(vec![
        LegacyPiece::new("minecraft:rupo", final_bb, Some(Direction::North), 0).with_data(
            PieceData::Template(Box::new(TemplatePiece {
                kind: TemplateKind::RuinedPortal(RuinedPortalPiece {
                    placement: setup.placement,
                    cold,
                    mossiness: setup.mossiness,
                    air_pocket,
                    overgrown: setup.overgrown,
                    vines: setup.vines,
                    replace_with_blackstone: setup.replace_with_blackstone,
                }),
                template: id,
                position: origin,
                rotation: rot,
                mirror,
                pivot,
                height_adjusted: false,
            })),
        ),
    ])
}
