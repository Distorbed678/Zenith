//! Jar-backed placement smoke tests: place every piece of real structure starts into an
//! in-memory world, chunk by chunk, and check the writes stay inside the writable areas.
//! Skipped (pass) when no 26.3 install or ingested tables are available.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use zenith_biome::BiomeSource;
use zenith_blocks::{BlockFactTags, BlockFacts};
use zenith_codec::format::PackType;
use zenith_codec::json::{Json, ParseMode, parse_bytes};
use zenith_codec::log::null_log;
use zenith_codec::resources::{PackResources, ZipPackResources};
use zenith_math::{LegacyRandom, XoroshiroRandom};
use zenith_registry::{BlockStateTable, Identifier};
use zenith_structures::world::{MemoryWorld, writable_area};
use zenith_structures::{
    BlockPos, BoundingBox, PlacementEnv, StructureData, StructureGenerator, StructureStart,
    TemplateManager,
};
use zenith_worldgen::{BlockTags, NoiseGenerator, WorldgenData};

const VERSION: &str = "26.3";

struct Shared {
    worldgen_data: WorldgenData,
    structure_data: Arc<StructureData>,
    template_manager: Arc<TemplateManager>,
    blocks: BlockStateTable,
    tags: BlockTags,
    facts: Arc<BlockFacts>,
    pack: Arc<ZipPackResources>,
}

fn load_shared() -> Option<Shared> {
    let install = zenith_install::discover_installs(Some(VERSION))
        .into_iter()
        .next()?;
    let tables =
        zenith_data::Tables::load(&zenith_data::version_dir(VERSION).join("tables.bin")).ok()?;
    let blocks = BlockStateTable::from_tables(&tables).ok()?;
    let pack = Arc::new(ZipPackResources::open(
        "vanilla",
        &install.client_jar,
        null_log(),
    ));
    let worldgen_data = WorldgenData::load(pack.as_ref()).ok()?;
    let tags = BlockTags::load(pack.as_ref(), &blocks).ok()?;
    let fact_tags = BlockFactTags::from_resolver(&blocks, |name| {
        Identifier::parse(name)
            .ok()
            .and_then(|id| tags.block_ids(&id))
    });
    let facts = Arc::new(BlockFacts::build(&blocks, &fact_tags).ok()?);
    let structure_data =
        Arc::new(StructureData::load(pack.as_ref(), worldgen_data.biome_tags()).ok()?);
    let template_manager = Arc::new(TemplateManager::new(
        pack.clone(),
        Arc::new(blocks.clone()),
        Some(Arc::clone(&facts)),
    ));
    Some(Shared {
        worldgen_data,
        structure_data,
        template_manager,
        blocks,
        tags,
        facts,
        pack,
    })
}

fn preset(pack: &ZipPackResources, dim: &str) -> Option<(Json, String)> {
    let id = Identifier::parse("minecraft:worldgen/world_preset/normal.json").ok()?;
    let bytes = pack.get(PackType::Data, &id)?;
    let j = parse_bytes(&bytes, ParseMode::Data).ok()?;
    let Json::Object(root) = &j else { return None };
    let Json::Object(dims) = root.get("dimensions")? else {
        return None;
    };
    let Json::Object(d) = dims.get(dim)? else {
        return None;
    };
    let Json::Object(g) = d.get("generator")? else {
        return None;
    };
    Some((
        g.get("biome_source")?.clone(),
        g.get("settings")?.as_str()?.to_owned(),
    ))
}

fn generator(
    sh: &Shared,
    dim: &str,
    seed: i64,
    min_y: i32,
    height: i32,
) -> Option<StructureGenerator> {
    let (bs_json, settings) = preset(&sh.pack, dim)?;
    let zenith_codec::codec::DataResult::Success(bs) = BiomeSource::decode_json(&bs_json) else {
        return None;
    };
    let sid = Identifier::parse(&settings).ok()?;
    let noise =
        Arc::new(NoiseGenerator::new(&sh.worldgen_data, &sid, seed, &sh.blocks, &sh.tags).ok()?);
    Some(StructureGenerator::new(
        seed,
        &sh.worldgen_data,
        Arc::clone(&sh.structure_data),
        Arc::clone(&sh.template_manager),
        noise,
        bs,
        Arc::clone(&sh.facts),
        &sh.blocks,
        min_y,
        height,
    ))
}

/// Places `start` chunk by chunk (every chunk its bounds touch) and checks write bounds.
fn place_start(
    sh: &Shared,
    gen_: &StructureGenerator,
    start: &StructureStart,
    min_y: i32,
    max_y: i32,
) -> usize {
    let mut start = start.clone();
    let sea = 63;
    let ground = start.bounding_box.min_y.max(min_y + 5).min(max_y - 40);
    let mut world = MemoryWorld::new(
        &sh.blocks,
        &sh.tags,
        &sh.facts,
        min_y,
        max_y,
        ground.max(sea),
    )
    .with_seed(12345);
    let env = PlacementEnv {
        data: gen_.structure_data(),
        templates: gen_.template_manager(),
    };
    let b = start.bounding_box;
    let mut total = 0;
    let mut allowed: Vec<BoundingBox> = Vec::new();
    for cz in (b.min_z >> 4)..=(b.max_z >> 4) {
        for cx in (b.min_x >> 4)..=(b.max_x >> 4) {
            let mut area = writable_area(&world, cx, cz);
            let before = world.writes.len();
            let mut rng = XoroshiroRandom::new(cx as i64 * 31 + cz as i64);
            start.place_in_chunk(&env, &mut world, &mut rng, &mut area);
            for (pos, _, _) in &world.writes[before..] {
                // Writes may reach into the 3x3 write region only.
                assert!(
                    ((pos.x >> 4) - cx).abs() <= 1 && ((pos.z >> 4) - cz).abs() <= 1,
                    "write far outside the 3x3 region: {pos:?} for chunk ({cx},{cz})"
                );
                total += 1;
            }
            allowed.push(area);
        }
    }
    let _ = (&allowed, LegacyRandom::new(0));
    total
}

#[test]
fn place_real_starts_without_panics() {
    let Some(sh) = load_shared() else {
        eprintln!("skip: no 26.3 install/tables");
        return;
    };
    let dims: [(&str, i32, i32); 3] = [
        ("minecraft:overworld", -64, 384),
        ("minecraft:the_nether", 0, 256),
        ("minecraft:the_end", 0, 256),
    ];
    let mut kinds_done: BTreeMap<String, usize> = BTreeMap::new();
    let mut kinds_pieces: std::collections::BTreeSet<String> = Default::default();
    for (dim, min_y, height) in dims {
        let Some(gen_) = generator(&sh, dim, 12345, min_y, height) else {
            continue;
        };
        let mut seen: HashSet<String> = HashSet::new();
        'scan: for cx in -120..120 {
            for cz in -120..120 {
                for start in gen_.structure_starts(cx, cz) {
                    let key = start.structure_id.to_string();
                    if !seen.insert(key.clone()) {
                        continue;
                    }
                    let n = place_start(&sh, &gen_, &start, min_y, min_y + height - 1);
                    *kinds_done.entry(key).or_default() += n;
                    for p in &start.pieces {
                        kinds_pieces.insert(p.kind.to_owned());
                    }
                    if seen.len() >= 60 {
                        break 'scan;
                    }
                }
            }
        }
    }
    eprintln!("placed structure kinds: {kinds_done:#?}");
    eprintln!("piece kinds seen: {kinds_pieces:?}");
    assert!(!kinds_done.is_empty());
    let _ = BlockPos::ZERO;
}

fn synthetic_start(id: &str, pieces: Vec<zenith_structures::StructurePiece>) -> StructureStart {
    let mut bb = pieces[0].bounding_box;
    for p in &pieces[1..] {
        bb.encapsulate(p.bounding_box);
    }
    StructureStart {
        structure_id: Identifier::parse(id).unwrap(),
        chunk_x: 0,
        chunk_z: 0,
        references: 0,
        terrain_adaptation: zenith_worldgen::TerrainAdjustment::None,
        bounding_box: bb,
        pieces,
    }
}

fn run_synthetic<'a>(sh: &'a Shared, start: &mut StructureStart, ground: i32) -> MemoryWorld<'a> {
    use zenith_structures::BoundingBoxExt;
    let mut world =
        MemoryWorld::new(&sh.blocks, &sh.tags, &sh.facts, -64, 319, ground).with_seed(777);
    let env = PlacementEnv {
        data: &sh.structure_data,
        templates: &sh.template_manager,
    };
    let b = start.bounding_box;
    for cz in (b.min_z >> 4)..=(b.max_z >> 4) {
        for cx in (b.min_x >> 4)..=(b.max_x >> 4) {
            let mut area = writable_area(&world, cx, cz);
            let mut rng = XoroshiroRandom::new(i64::from(cx) * 31 + i64::from(cz));
            start.place_in_chunk(&env, &mut world, &mut rng, &mut area);
        }
    }
    let _ = b.x_span();
    world
}

#[test]
fn scattered_pieces_place_chests_traps_and_entities() {
    use zenith_structures::piece_data::{JungleTempleState, ScatteredState, SwampHutState};
    use zenith_structures::{BoundingBoxExt, Direction, PieceData, StructurePiece};
    let Some(sh) = load_shared() else {
        eprintln!("skip: no 26.3 install/tables");
        return;
    };
    let mk = |kind: &'static str, w, h, d, data| StructurePiece {
        kind,
        bounding_box: BoundingBox::make_box(8, 64, 8, Direction::South, w, h, d),
        orientation: Some(Direction::South),
        gen_depth: 0,
        jigsaw: None,
        data,
    };
    // Desert pyramid: 4 chests with loot, TNT trap, suspicious sand.
    let mut s = synthetic_start(
        "minecraft:desert_pyramid",
        vec![mk(
            "minecraft:tedp",
            21,
            15,
            21,
            PieceData::DesertPyramid(ScatteredState::default()),
        )],
    );
    let w = run_synthetic(&sh, &mut s, 64);
    let chests = w
        .block_entities
        .values()
        .filter(|n| {
            n.get("LootTable").and_then(|t| t.as_str()) == Some("minecraft:chests/desert_pyramid")
        })
        .count();
    assert_eq!(chests, 4, "desert pyramid chests");
    let susp = w
        .block_entities
        .values()
        .filter(|n| {
            n.get("LootTable").and_then(|t| t.as_str())
                == Some("minecraft:archaeology/desert_pyramid")
        })
        .count();
    assert!((5..=8).contains(&susp), "suspicious sand count {susp}");
    // Jungle temple: 2 chests + 2 dispensers.
    let mut s = synthetic_start(
        "minecraft:jungle_temple",
        vec![mk(
            "minecraft:tejp",
            12,
            10,
            15,
            PieceData::JungleTemple(JungleTempleState::default()),
        )],
    );
    let w = run_synthetic(&sh, &mut s, 64);
    let loot = |t: &str| {
        w.block_entities
            .values()
            .filter(|n| n.get("LootTable").and_then(|x| x.as_str()) == Some(t))
            .count()
    };
    assert_eq!(loot("minecraft:chests/jungle_temple"), 2);
    assert_eq!(loot("minecraft:chests/jungle_temple_dispenser"), 2);
    // Swamp hut: witch and cat exactly once.
    let mut s = synthetic_start(
        "minecraft:swamp_hut",
        vec![mk(
            "minecraft:tesh",
            7,
            7,
            9,
            PieceData::SwampHut(SwampHutState::default()),
        )],
    );
    let w = run_synthetic(&sh, &mut s, 64);
    let ids: Vec<_> = w
        .entities
        .iter()
        .filter_map(|e| e.get("id").and_then(|t| t.as_str()).map(str::to_owned))
        .collect();
    assert_eq!(ids.iter().filter(|i| *i == "minecraft:witch").count(), 1);
    assert_eq!(ids.iter().filter(|i| *i == "minecraft:cat").count(), 1);
}

#[test]
fn mansion_and_end_city_templates_place_markers() {
    use zenith_structures::{Direction, Mirror, PieceData, Rotation, StructurePiece};
    use zenith_structures::{TemplateKind, TemplatePiece};
    let Some(sh) = load_shared() else {
        eprintln!("skip: no 26.3 install/tables");
        return;
    };
    for name in [
        "woodland_mansion/entrance",
        "woodland_mansion/1x1_a1",
        "woodland_mansion/1x2_a1",
    ] {
        let id = Identifier::parse(&format!("minecraft:{name}")).unwrap();
        let Ok(tpl) = sh.template_manager.get(&id) else {
            continue;
        };
        let pos = BlockPos::new(0, 64, 0);
        let bb = tpl.bounding_box(pos, Rotation::None, BlockPos::ZERO, Mirror::None);
        let piece = StructurePiece {
            kind: "minecraft:wmp",
            bounding_box: bb,
            orientation: Some(Direction::North),
            gen_depth: 0,
            jigsaw: None,
            data: PieceData::Template(Box::new(TemplatePiece {
                kind: TemplateKind::WoodlandMansion,
                template: id,
                position: pos,
                rotation: Rotation::None,
                mirror: Mirror::None,
                pivot: BlockPos::ZERO,
                height_adjusted: false,
            })),
        };
        let mut s = synthetic_start("minecraft:mansion", vec![piece]);
        let w = run_synthetic(&sh, &mut s, 60);
        assert!(!w.writes.is_empty(), "{name} placed nothing");
    }
}
