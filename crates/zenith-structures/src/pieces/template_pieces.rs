//! `TemplateStructurePiece` subclasses (§1.7, §10–§12, §14–§16 and §6.4 of
//! `docs/specs/worldgen/structures-legacy-pieces.md`): igloo, shipwreck, ocean ruin, ruined
//! portal, nether fossil, end city and woodland mansion pieces.

use zenith_nbt::{Compound, List, Tag};
use zenith_registry::Identifier;
use zenith_worldgen::{HeightmapKind, resolve_block_state};

use crate::block_xform::{block_path, default_state, with_property};
use crate::data::{
    LiquidSettings, OceanRuinTemp, ProcessorRule, RawBlockHolderSet, RuinedPortalPlacement,
    RuleBlockEntityModifier, RuleTest, StructureProcessor,
};
use crate::generator::StructurePiece;
use crate::geom::{
    BlockPos, BoundingBox, BoundingBoxExt, Direction, Mirror, Rotation, transform_pos,
};
use crate::piece_data::{PieceData, RuinedPortalPiece, TemplateKind, TemplatePiece};
use crate::pieces::scattered::thread_local_positional;
use crate::place::PieceCall;
use crate::template::StructurePlaceSettings;
use crate::template_place::{filter_blocks, is_randomizable_container, place_template};
use crate::world::{StructureWorld, WorldgenRandom};
use zenith_worldgen::BlockStateSpec;

const HORIZONTALS: [Direction; 4] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];

fn id(path: &str) -> Identifier {
    Identifier::with_default_namespace(path).expect("valid identifier")
}

fn block_ignore(names: &[&str]) -> StructureProcessor {
    StructureProcessor::BlockIgnore {
        blocks: names.iter().map(|n| id(n)).collect(),
    }
}

fn spec(name: &str) -> BlockStateSpec {
    BlockStateSpec {
        name: id(name),
        properties: Default::default(),
    }
}

fn rule(input: RuleTest, output: &str, modifier: RuleBlockEntityModifier) -> ProcessorRule {
    ProcessorRule {
        input_predicate: input,
        location_predicate: RuleTest::AlwaysTrue,
        position_predicate: crate::data::PosRuleTest::AlwaysTrue,
        output_state: spec(output),
        block_entity_modifier: modifier,
    }
}

fn settings_for(tp: &TemplatePiece, chunk: BoundingBox) -> StructurePlaceSettings {
    let mut s = StructurePlaceSettings {
        rotation: tp.rotation,
        mirror: tp.mirror,
        rotation_pivot: tp.pivot,
        bounding_box: Some(chunk),
        ..StructurePlaceSettings::default()
    };
    match tp.kind {
        TemplateKind::Igloo => {
            s.processors = vec![block_ignore(&["structure_block"])];
            s.liquid_settings = LiquidSettings::IgnoreWaterlogging;
        }
        TemplateKind::Shipwreck { .. } | TemplateKind::NetherFossil => {
            s.processors = vec![block_ignore(&["air", "structure_block"])];
        }
        TemplateKind::OceanRuin {
            integrity, temp, ..
        } => {
            let (cand, repl, loot) = match temp {
                OceanRuinTemp::Warm => (
                    "sand",
                    "suspicious_sand",
                    "minecraft:archaeology/ocean_ruin_warm",
                ),
                OceanRuinTemp::Cold => (
                    "gravel",
                    "suspicious_gravel",
                    "minecraft:archaeology/ocean_ruin_cold",
                ),
            };
            s.processors = vec![
                StructureProcessor::BlockRot {
                    rottable_blocks: None,
                    integrity,
                },
                block_ignore(&["air", "structure_block"]),
                StructureProcessor::Capped {
                    delegate: Box::new(StructureProcessor::Rule {
                        rules: vec![rule(
                            RuleTest::BlockMatch { block: id(cand) },
                            repl,
                            RuleBlockEntityModifier::AppendLoot {
                                loot_table: Identifier::parse(loot).expect("valid"),
                            },
                        )],
                    }),
                    limit: zenith_worldgen::IntProvider::Constant(5),
                },
            ];
        }
        TemplateKind::EndCity { overwrite } => {
            s.ignore_entities = true;
            s.processors = vec![if overwrite {
                block_ignore(&["structure_block"])
            } else {
                block_ignore(&["air", "structure_block"])
            }];
        }
        TemplateKind::WoodlandMansion => {
            s.ignore_entities = true;
            s.processors = vec![block_ignore(&["structure_block"])];
        }
        TemplateKind::RuinedPortal(rp) => {
            let mut rules = vec![rule(
                RuleTest::RandomBlockMatch {
                    block: id("gold_block"),
                    probability: 0.3,
                },
                "air",
                RuleBlockEntityModifier::Passthrough,
            )];
            let lava = if rp.placement == RuinedPortalPlacement::OnOceanFloor {
                rule(
                    RuleTest::RandomBlockMatch {
                        block: id("lava"),
                        probability: 1.0,
                    },
                    "magma_block",
                    RuleBlockEntityModifier::Passthrough,
                )
            } else if rp.cold {
                rule(
                    RuleTest::RandomBlockMatch {
                        block: id("lava"),
                        probability: 1.0,
                    },
                    "netherrack",
                    RuleBlockEntityModifier::Passthrough,
                )
            } else {
                rule(
                    RuleTest::RandomBlockMatch {
                        block: id("lava"),
                        probability: 0.2,
                    },
                    "magma_block",
                    RuleBlockEntityModifier::Passthrough,
                )
            };
            rules.push(lava);
            if !rp.cold {
                rules.push(rule(
                    RuleTest::RandomBlockMatch {
                        block: id("netherrack"),
                        probability: 0.07,
                    },
                    "magma_block",
                    RuleBlockEntityModifier::Passthrough,
                ));
            }
            s.processors = vec![
                if rp.air_pocket {
                    block_ignore(&["structure_block"])
                } else {
                    block_ignore(&["air", "structure_block"])
                },
                StructureProcessor::Rule { rules },
                StructureProcessor::BlockAge {
                    mossiness: rp.mossiness,
                },
                StructureProcessor::ProtectedBlocks {
                    value: RawBlockHolderSet::Tag(id("features_cannot_replace")),
                },
                StructureProcessor::LavaSubmergedBlock,
            ];
            if rp.replace_with_blackstone {
                s.processors.push(StructureProcessor::BlackstoneReplace);
            }
        }
    }
    s
}

fn set_container_loot(
    world: &mut dyn StructureWorld,
    random: &mut dyn WorldgenRandom,
    pos: BlockPos,
    table: &str,
) {
    let st = world.get_block_state(pos);
    let path = block_path(world.blocks(), st).to_owned();
    if is_randomizable_container(&path) {
        crate::piece::set_loot(world, pos, table, random.next_long());
    }
}

fn entity_nbt(id: &str, pos: [f64; 3], persistent: bool) -> Compound {
    let mut nbt = Compound::new();
    nbt.insert("id", Tag::from(id));
    let mut p = List::new();
    for v in pos {
        p.push(Tag::Double(v));
    }
    nbt.insert("Pos", Tag::List(p));
    if persistent {
        nbt.insert("PersistenceRequired", Tag::Byte(1));
    }
    nbt
}

/// `TemplateStructurePiece.postProcess` plus the subclass overrides.
pub fn post_process(piece: &mut StructurePiece, call: &mut PieceCall<'_>) {
    let PieceData::Template(tp) = &mut piece.data else {
        return;
    };
    match tp.kind {
        TemplateKind::Igloo => igloo(piece, call),
        TemplateKind::Shipwreck { is_beached } => shipwreck(piece, call, is_beached),
        TemplateKind::OceanRuin { .. } => ocean_ruin(piece, call),
        TemplateKind::RuinedPortal(rp) => ruined_portal(piece, call, rp),
        TemplateKind::NetherFossil => nether_fossil(piece, call),
        TemplateKind::EndCity { .. } | TemplateKind::WoodlandMansion => {
            base_post_process(piece, call);
        }
    }
}

/// The shared `TemplateStructurePiece.postProcess` body. Returns whether the template placed.
fn base_post_process(piece: &mut StructurePiece, call: &mut PieceCall<'_>) -> bool {
    let PieceData::Template(tp) = &piece.data else {
        return false;
    };
    let tp = (**tp).clone();
    let Ok(template) = call.env.templates.get(&tp.template) else {
        return false;
    };
    let mut settings = settings_for(&tp, *call.chunk_bb);
    piece.bounding_box = template.bounding_box_with_settings(&settings, tp.position);
    let placed = place_template(
        call.world,
        &template,
        tp.position,
        call.reference_pos,
        &mut settings,
        &mut *call.random,
        2,
    );
    if !placed {
        return false;
    }
    // Data markers.
    let markers = filter_blocks(
        &*call.world,
        &template,
        tp.position,
        &mut settings,
        "structure_block",
    );
    for m in markers {
        let Some(nbt) = &m.nbt else { continue };
        if nbt.get("mode").and_then(Tag::as_str) != Some("DATA") {
            continue;
        }
        let meta = nbt
            .get("metadata")
            .and_then(Tag::as_str)
            .unwrap_or("")
            .to_owned();
        handle_data_marker(&tp, &meta, m.pos, call);
    }
    // Jigsaw final states.
    let jigsaws = filter_blocks(
        &*call.world,
        &template,
        tp.position,
        &mut settings,
        "jigsaw",
    );
    for j in jigsaws {
        let state_str = j
            .nbt
            .as_ref()
            .and_then(|n| n.get("final_state"))
            .and_then(Tag::as_str)
            .unwrap_or("minecraft:air")
            .to_owned();
        if let Some(sp) = parse_state_string(&state_str)
            && let Ok(st) = resolve_block_state(call.world.blocks(), &sp)
        {
            call.world.set_block_state(j.pos, st, 3);
        }
    }
    true
}

fn parse_state_string(s: &str) -> Option<BlockStateSpec> {
    let (name, props) = match s.find('[') {
        Some(i) => (&s[..i], s[i + 1..].trim_end_matches(']')),
        None => (s, ""),
    };
    let name = Identifier::parse(name).ok()?;
    let mut properties = std::collections::BTreeMap::new();
    for kv in props.split(',').filter(|p| !p.is_empty()) {
        let (k, v) = kv.split_once('=')?;
        properties.insert(k.trim().to_owned(), v.trim().to_owned());
    }
    Some(BlockStateSpec { name, properties })
}

fn handle_data_marker(tp: &TemplatePiece, meta: &str, pos: BlockPos, call: &mut PieceCall<'_>) {
    let chunk = *call.chunk_bb;
    match tp.kind {
        TemplateKind::Igloo => {
            if meta == "chest" {
                let air = call
                    .world
                    .blocks()
                    .air_default_state()
                    .map_or(0, |s| s.global_id);
                call.world.set_block_state(pos, air, 3);
                let below = pos.below(1);
                let st = call.world.get_block_state(below);
                let p = block_path(call.world.blocks(), st);
                if p == "chest" || p == "trapped_chest" {
                    crate::piece::set_loot(
                        call.world,
                        below,
                        "minecraft:chests/igloo_chest",
                        call.random.next_long(),
                    );
                }
            }
        }
        TemplateKind::Shipwreck { .. } => {
            let table = match meta {
                "map_chest" => "minecraft:chests/shipwreck_map",
                "treasure_chest" => "minecraft:chests/shipwreck_treasure",
                "supply_chest" => "minecraft:chests/shipwreck_supply",
                _ => return,
            };
            set_container_loot(call.world, &mut *call.random, pos.below(1), table);
        }
        TemplateKind::OceanRuin { is_large, .. } => match meta {
            "chest" => {
                let fluid = call
                    .world
                    .facts()
                    .state(call.world.get_block_state(pos))
                    .fluid;
                let chest = default_state(call.world.blocks(), "chest").unwrap_or(0);
                let wl = if fluid.kind.is_water() {
                    "true"
                } else {
                    "false"
                };
                let st = with_property(call.world.blocks(), chest, "waterlogged", wl);
                call.world.set_block_state(pos, st, 2);
                let table = if is_large {
                    "minecraft:chests/underwater_ruin_big"
                } else {
                    "minecraft:chests/underwater_ruin_small"
                };
                crate::piece::set_loot(call.world, pos, table, call.random.next_long());
            }
            "drowned" => {
                call.world.add_entity(entity_nbt(
                    "minecraft:drowned",
                    [f64::from(pos.x), f64::from(pos.y), f64::from(pos.z)],
                    true,
                ));
                let sea = call.world.sea_level();
                let replacement = if pos.y > sea {
                    call.world
                        .blocks()
                        .air_default_state()
                        .map_or(0, |s| s.global_id)
                } else {
                    default_state(call.world.blocks(), "water").unwrap_or(0)
                };
                call.world.set_block_state(pos, replacement, 2);
            }
            _ => {}
        },
        TemplateKind::EndCity { .. } => {
            if meta.starts_with("Chest") {
                if chunk.is_inside(pos.below(1)) {
                    set_container_loot(
                        call.world,
                        &mut *call.random,
                        pos.below(1),
                        "minecraft:chests/end_city_treasure",
                    );
                }
            } else if chunk.is_inside(pos) {
                if meta.starts_with("Sentry") {
                    call.world.add_entity(entity_nbt(
                        "minecraft:shulker",
                        [
                            f64::from(pos.x) + 0.5,
                            f64::from(pos.y),
                            f64::from(pos.z) + 0.5,
                        ],
                        false,
                    ));
                } else if meta.starts_with("Elytra") {
                    let facing = tp.rotation.rotate_dir(Direction::South);
                    let mut nbt = entity_nbt(
                        "minecraft:item_frame",
                        [
                            f64::from(pos.x) + 0.5,
                            f64::from(pos.y) + 0.5,
                            f64::from(pos.z) + 0.5,
                        ],
                        false,
                    );
                    let f = match facing {
                        Direction::Down => 0,
                        Direction::Up => 1,
                        Direction::North => 2,
                        Direction::South => 3,
                        Direction::West => 4,
                        Direction::East => 5,
                    };
                    nbt.insert("Facing", Tag::Byte(f));
                    let mut item = Compound::new();
                    item.insert("id", Tag::from("minecraft:elytra"));
                    item.insert("count", Tag::Int(1));
                    nbt.insert("Item", Tag::Compound(item));
                    nbt.insert("block_pos", Tag::IntArray(vec![pos.x, pos.y, pos.z]));
                    call.world.add_entity(nbt);
                }
            }
        }
        TemplateKind::WoodlandMansion => {
            if meta.starts_with("Chest") {
                let dir = match meta {
                    "ChestWest" => Direction::West,
                    "ChestEast" => Direction::East,
                    "ChestSouth" => Direction::South,
                    _ => Direction::North,
                };
                let dir = if matches!(
                    meta,
                    "ChestWest" | "ChestEast" | "ChestSouth" | "ChestNorth"
                ) {
                    tp.rotation.rotate_dir(dir)
                } else {
                    Direction::North
                };
                let chest = default_state(call.world.blocks(), "chest").unwrap_or(0);
                let st = with_property(call.world.blocks(), chest, "facing", dir.name());
                if chunk.is_inside(pos)
                    && block_path(call.world.blocks(), call.world.get_block_state(pos)) != "chest"
                {
                    call.world.set_block_state(pos, st, 2);
                    crate::piece::set_loot(
                        call.world,
                        pos,
                        "minecraft:chests/woodland_mansion",
                        call.random.next_long(),
                    );
                }
            } else {
                let mob = match meta {
                    "Mage" => Some(("minecraft:evoker", 1)),
                    "Warrior" => Some(("minecraft:vindicator", 1)),
                    "Group of Allays" => {
                        let n = call.world.level_random().next_int_bounded(3) + 1;
                        Some(("minecraft:allay", n))
                    }
                    _ => None,
                };
                if let Some((e, n)) = mob {
                    for _ in 0..n {
                        call.world.add_entity(entity_nbt(
                            e,
                            [f64::from(pos.x), f64::from(pos.y), f64::from(pos.z)],
                            true,
                        ));
                    }
                    let air = call
                        .world
                        .blocks()
                        .air_default_state()
                        .map_or(0, |s| s.global_id);
                    call.world.set_block_state(pos, air, 2);
                }
            }
        }
        TemplateKind::RuinedPortal(_) | TemplateKind::NetherFossil => {}
    }
}

// ---------------------------------------------------------------------------
// Igloo
// ---------------------------------------------------------------------------

fn igloo(piece: &mut StructurePiece, call: &mut PieceCall<'_>) {
    let PieceData::Template(tp) = &mut piece.data else {
        return;
    };
    let name = tp.template.path().to_owned();
    let offset = match name.as_str() {
        "igloo/middle" => BlockPos::new(2, -3, 4),
        "igloo/bottom" => BlockPos::new(0, -3, -2),
        _ => BlockPos::ZERO,
    };
    let settings = settings_for(tp, *call.chunk_bb);
    let rel = transform_pos(
        BlockPos::new(3 - offset.x, 0, -offset.z),
        settings.mirror,
        settings.rotation,
        settings.rotation_pivot,
    );
    let entrance = tp.position.add(rel);
    let height = call
        .world
        .get_height(HeightmapKind::WorldSurfaceWg, entrance.x, entrance.z);
    let original = tp.position;
    tp.position = original.offset(0, height - 90 - 1, 0);
    let shifted = tp.position;
    let is_top = name == "igloo/top";
    let settings2 = settings_for(tp, *call.chunk_bb);
    base_post_process(piece, call);
    if is_top {
        let rel = transform_pos(
            BlockPos::new(3, 0, 5),
            settings2.mirror,
            settings2.rotation,
            settings2.rotation_pivot,
        );
        let pos = shifted.add(rel);
        let below = call.world.get_block_state(pos.below(1));
        let facts_air = call
            .world
            .facts()
            .state(below)
            .has(zenith_blocks::flag::IS_AIR);
        if !facts_air && block_path(call.world.blocks(), below) != "ladder" {
            let snow = default_state(call.world.blocks(), "snow_block").unwrap_or(0);
            call.world.set_block_state(pos, snow, 3);
        }
    }
    if let PieceData::Template(tp) = &mut piece.data {
        tp.position = original;
    }
}

// ---------------------------------------------------------------------------
// Shipwreck
// ---------------------------------------------------------------------------

fn shipwreck(piece: &mut StructurePiece, call: &mut PieceCall<'_>, is_beached: bool) {
    let PieceData::Template(tp) = &mut piece.data else {
        return;
    };
    if let Ok(template) = call.env.templates.get(&tp.template) {
        let too_big = template.size.x > 32 || template.size.y > 32;
        if !tp.height_adjusted && !too_big {
            let hm = if is_beached {
                HeightmapKind::WorldSurfaceWg
            } else {
                HeightmapKind::OceanFloorWg
            };
            let sz = template.size;
            let base_size = sz.x * sz.z;
            let (mut mean, mut min_y) = (0i32, i32::MAX);
            if base_size == 0 {
                let h = call.world.get_height(hm, tp.position.x, tp.position.z);
                mean = h;
                min_y = h;
            } else {
                for z in tp.position.z..=tp.position.z + sz.z - 1 {
                    for x in tp.position.x..=tp.position.x + sz.x - 1 {
                        let h = call.world.get_height(hm, x, z);
                        mean += h;
                        min_y = min_y.min(h);
                    }
                }
                mean /= base_size;
            }
            let new_y = if is_beached {
                min_y - sz.y / 2 - call.random.next_int_bounded(3)
            } else {
                mean
            };
            tp.position.y = new_y;
            tp.height_adjusted = true;
        }
    }
    base_post_process(piece, call);
}

// ---------------------------------------------------------------------------
// Ocean ruin
// ---------------------------------------------------------------------------

fn ocean_ruin(piece: &mut StructurePiece, call: &mut PieceCall<'_>) {
    let PieceData::Template(tp) = &mut piece.data else {
        return;
    };
    let Ok(template) = call.env.templates.get(&tp.template) else {
        return;
    };
    let h = call
        .world
        .get_height(HeightmapKind::OceanFloorWg, tp.position.x, tp.position.z);
    tp.position.y = h;
    let corner = transform_pos(
        BlockPos::new(template.size.x - 1, 0, template.size.z - 1),
        Mirror::None,
        tp.rotation,
        BlockPos::ZERO,
    )
    .add(tp.position);
    // getHeight(tp, level, corner)
    let new_y = tp.position.y;
    let mut min_y = 512;
    let top_y = new_y - 1;
    let mut area = 0;
    let (x0, x1) = (tp.position.x.min(corner.x), tp.position.x.max(corner.x));
    let (z0, z1) = (tp.position.z.min(corner.z), tp.position.z.max(corner.z));
    let (y0, y1) = (tp.position.y.min(corner.y), tp.position.y.max(corner.y));
    let world_min = call.world.min_y();
    for z in z0..=z1 {
        for y in y0..=y1 {
            for x in x0..=x1 {
                let _ = y;
                let mut floor_y = tp.position.y - 1;
                loop {
                    let st = call.world.get_block_state(BlockPos::new(x, floor_y, z));
                    let facts = call.world.facts().state(st);
                    let air_or_water_or_ice = facts.has(zenith_blocks::flag::IS_AIR)
                        || facts.fluid.kind.is_water()
                        || is_ice(call, st);
                    if air_or_water_or_ice && floor_y > world_min + 1 {
                        floor_y -= 1;
                    } else {
                        break;
                    }
                }
                min_y = min_y.min(floor_y);
                if floor_y < top_y - 2 {
                    area += 1;
                }
            }
        }
    }
    let width = (tp.position.x - corner.x).abs();
    tp.position.y = if top_y - min_y > 2 && area > width - 2 {
        min_y + 1
    } else {
        new_y
    };
    base_post_process(piece, call);
}

fn is_ice(call: &PieceCall<'_>, st: u32) -> bool {
    let Some(s) = call.world.blocks().state_by_id_exact(st as i32) else {
        return false;
    };
    call.world
        .block_tags()
        .contains_block(&id("ice"), s.block_index)
}

// ---------------------------------------------------------------------------
// Nether fossil
// ---------------------------------------------------------------------------

fn nether_fossil(piece: &mut StructurePiece, call: &mut PieceCall<'_>) {
    let PieceData::Template(tp) = &piece.data else {
        return;
    };
    let tp = (**tp).clone();
    let Ok(template) = call.env.templates.get(&tp.template) else {
        return;
    };
    let settings = settings_for(&tp, *call.chunk_bb);
    let fossil_bb = template.bounding_box_with_settings(&settings, tp.position);
    call.chunk_bb.encapsulate(fossil_bb);
    base_post_process(piece, call);
    // placeDriedGhast
    let center = fossil_bb.center();
    let mut pr = thread_local_positional(call.world.world_seed(), center);
    if pr.next_float() < 0.5 {
        let x = fossil_bb.min_x + pr.next_int_bounded(fossil_bb.x_span());
        let y = fossil_bb.min_y;
        let z = fossil_bb.min_z + pr.next_int_bounded(fossil_bb.z_span());
        let pos = BlockPos::new(x, y, z);
        let st = call.world.get_block_state(pos);
        let is_air = call
            .world
            .facts()
            .state(st)
            .has(zenith_blocks::flag::IS_AIR);
        if is_air && call.chunk_bb.is_inside(pos) {
            let rot = Rotation::random(&mut pr);
            let ghast = default_state(call.world.blocks(), "dried_ghast").unwrap_or(0);
            let facing = rot.rotate_dir(Direction::North);
            let st = with_property(call.world.blocks(), ghast, "facing", facing.name());
            call.world.set_block_state(pos, st, 2);
        }
    }
}

// ---------------------------------------------------------------------------
// Ruined portal
// ---------------------------------------------------------------------------

fn can_replace_netherrack(
    world: &dyn StructureWorld,
    st: u32,
    placement: RuinedPortalPlacement,
) -> bool {
    let facts = world.facts().state(st);
    if facts.has(zenith_blocks::flag::IS_AIR) {
        return false;
    }
    let blocks = world.blocks();
    let Some(s) = blocks.state_by_id_exact(st as i32) else {
        return false;
    };
    if s.block_id.path() == "obsidian" {
        return false;
    }
    if world
        .block_tags()
        .contains_block(&id("features_cannot_replace"), s.block_index)
    {
        return false;
    }
    placement == RuinedPortalPlacement::InNether || s.block_id.path() != "lava"
}

fn place_netherrack_or_magma(
    world: &mut dyn StructureWorld,
    random: &mut dyn WorldgenRandom,
    pos: BlockPos,
    cold: bool,
) {
    let name = if !cold && random.next_float() < 0.07 {
        "magma_block"
    } else {
        "netherrack"
    };
    let st = default_state(world.blocks(), name).unwrap_or(0);
    world.set_block_state(pos, st, 3);
}

fn maybe_add_leaves_above(
    world: &mut dyn StructureWorld,
    random: &mut dyn WorldgenRandom,
    pos: BlockPos,
) {
    if random.next_float() < 0.5 {
        let here = world.get_block_state(pos);
        let above = world.get_block_state(pos.above(1));
        let air = world.facts().state(above).has(zenith_blocks::flag::IS_AIR);
        if block_path(world.blocks(), here) == "netherrack" && air {
            let leaves = default_state(world.blocks(), "jungle_leaves").unwrap_or(0);
            let st = with_property(world.blocks(), leaves, "persistent", "true");
            world.set_block_state(pos.above(1), st, 3);
        }
    }
}

fn drip_column(
    world: &mut dyn StructureWorld,
    random: &mut dyn WorldgenRandom,
    mut pos: BlockPos,
    cold: bool,
) {
    place_netherrack_or_magma(world, random, pos, cold);
    let mut rem = 8;
    while rem > 0 && random.next_float() < 0.5 {
        pos = pos.below(1);
        rem -= 1;
        place_netherrack_or_magma(world, random, pos, cold);
    }
}

fn ruined_portal(piece: &mut StructurePiece, call: &mut PieceCall<'_>, rp: RuinedPortalPiece) {
    let PieceData::Template(tp) = &piece.data else {
        return;
    };
    let tp = (**tp).clone();
    let Ok(template) = call.env.templates.get(&tp.template) else {
        return;
    };
    let settings = settings_for(&tp, *call.chunk_bb);
    let bb = template.bounding_box_with_settings(&settings, tp.position);
    if !call.chunk_bb.is_inside(bb.center()) {
        return;
    }
    call.chunk_bb.encapsulate(bb);
    base_post_process(piece, call);
    let hm = if rp.placement == RuinedPortalPlacement::OnOceanFloor {
        HeightmapKind::OceanFloorWg
    } else {
        HeightmapKind::WorldSurfaceWg
    };
    // spreadNetherrack
    let follow_ground = matches!(
        rp.placement,
        RuinedPortalPlacement::OnLandSurface | RuinedPortalPlacement::OnOceanFloor
    );
    let center = bb.center();
    const PROB: [f32; 14] = [
        1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.9, 0.9, 0.8, 0.7, 0.6, 0.4, 0.2,
    ];
    let avg_width = (bb.x_span() + bb.z_span()) / 2;
    let dist_adj = call.random.next_int_bounded(1.max(8 - avg_width / 2));
    for x in center.x - 14..=center.x + 14 {
        for z in center.z - 14..=center.z + 14 {
            let adj = 0.max((x - center.x).abs() + (z - center.z).abs() + dist_adj);
            if adj < 14 && call.random.next_double() < f64::from(PROB[adj as usize]) {
                let surf_y = call.world.get_height(hm, x, z) - 1;
                let y = if follow_ground {
                    surf_y
                } else {
                    bb.min_y.min(surf_y)
                };
                let pos = BlockPos::new(x, y, z);
                if (y - bb.min_y).abs() <= 3
                    && can_replace_netherrack(
                        &*call.world,
                        call.world.get_block_state(pos),
                        rp.placement,
                    )
                {
                    place_netherrack_or_magma(call.world, &mut *call.random, pos, rp.cold);
                    if rp.overgrown {
                        maybe_add_leaves_above(call.world, &mut *call.random, pos);
                    }
                    drip_column(call.world, &mut *call.random, pos.below(1), rp.cold);
                }
            }
        }
    }
    // addNetherrackDripColumnsBelowPortal
    for x in bb.min_x + 1..=bb.max_x - 1 {
        for z in bb.min_z + 1..=bb.max_z - 1 {
            let p = BlockPos::new(x, bb.min_y, z);
            let st = call.world.get_block_state(p);
            if block_path(call.world.blocks(), st) == "netherrack" {
                drip_column(call.world, &mut *call.random, p.below(1), rp.cold);
            }
        }
    }
    if rp.vines || rp.overgrown {
        for z in bb.min_z..=bb.max_z {
            for y in bb.min_y..=bb.max_y {
                for x in bb.min_x..=bb.max_x {
                    let pos = BlockPos::new(x, y, z);
                    if rp.vines {
                        let st = call.world.get_block_state(pos);
                        let is_air = call
                            .world
                            .facts()
                            .state(st)
                            .has(zenith_blocks::flag::IS_AIR);
                        if !is_air && block_path(call.world.blocks(), st) != "vine" {
                            let dir = HORIZONTALS[call.random.next_int_bounded(4) as usize];
                            let n = pos.relative(dir, 1);
                            let nst = call.world.get_block_state(n);
                            let n_air = call
                                .world
                                .facts()
                                .state(nst)
                                .has(zenith_blocks::flag::IS_AIR);
                            if n_air && face_full(&*call.world, st, dir) {
                                let vine = default_state(call.world.blocks(), "vine").unwrap_or(0);
                                let v = with_property(
                                    call.world.blocks(),
                                    vine,
                                    dir.opposite().name(),
                                    "true",
                                );
                                call.world.set_block_state(n, v, 3);
                            }
                        }
                    }
                    if rp.overgrown {
                        maybe_add_leaves_above(call.world, &mut *call.random, pos);
                    }
                }
            }
        }
    }
}

fn face_full(world: &dyn StructureWorld, st: u32, dir: Direction) -> bool {
    world.facts().state(st).collision_face_full & (1 << dir.ordinal()) != 0
}

// ---------------------------------------------------------------------------
// Woodland mansion afterPlace
// ---------------------------------------------------------------------------

/// `WoodlandMansionStructure.afterPlace`: cobblestone foundation columns.
pub fn mansion_after_place(
    pieces: &[StructurePiece],
    world: &mut dyn StructureWorld,
    chunk: &BoundingBox,
) {
    if pieces.is_empty() {
        return;
    }
    let mut bb = pieces[0].bounding_box;
    for p in &pieces[1..] {
        bb.encapsulate(p.bounding_box);
    }
    let y_start = bb.min_y;
    let min_y = world.min_y();
    let cobble = default_state(world.blocks(), "cobblestone").unwrap_or(0);
    for x in chunk.min_x..=chunk.max_x {
        for z in chunk.min_z..=chunk.max_z {
            let pos = BlockPos::new(x, y_start, z);
            let st = world.get_block_state(pos);
            let empty = world.facts().state(st).has(zenith_blocks::flag::IS_AIR);
            if !empty && bb.is_inside(pos) && pieces.iter().any(|p| p.bounding_box.is_inside(pos)) {
                let mut y = y_start - 1;
                while y > min_y {
                    let p2 = BlockPos::new(x, y, z);
                    let s2 = world.get_block_state(p2);
                    let f = world.facts().state(s2);
                    if !f.has(zenith_blocks::flag::IS_AIR) && !f.has(zenith_blocks::flag::LIQUID) {
                        break;
                    }
                    world.set_block_state(p2, cobble, 2);
                    y -= 1;
                }
            }
        }
    }
}
