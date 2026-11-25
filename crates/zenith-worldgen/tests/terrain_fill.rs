//! Unit tests for `NoiseChunk`, `Aquifer`, `GlobalFluidPicker`, `Heightmap`, `BlockTags`,
//! `BlockPredicates`, `Beardifier`, `Blender`, `TerrainChunk`, and `NoiseGenerator`.
//!
//! Runs with zero external game files (purely synthetic registries and density functions).

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use zenith_codec::json::Json;
use zenith_registry::{BlockDef, BlockStateTable, Identifier, PropertyDef};
use zenith_worldgen::{
    Aquifer, AquiferConfig, Beardifier, BeardifierRigid, Blender, BlockPredicates, BlockStateSpec,
    BlockTags, BoundingBox, ContextSampler, DensityFunction, DensityVolume,
    FLOWING_UPDATE_SIMILARITY, FluidStatus, GLOBAL_LAVA_LEVEL, GlobalFluidPicker, Heightmap,
    HeightmapKind, JigsawJunction, MaterialRuleRef, NoiseGenerator, NoiseGeneratorSettings,
    NoiseRouter, NoiseSettings, RandomState, SamplerContext, TAG_BLOCKS_MOTION_IN_HEIGHTMAP,
    TAG_BLOCKS_MOTION_IN_HEIGHTMAP_NO_LEAVES, TerrainAdjustment, WAY_BELOW_MIN_Y, WorldgenData,
    pack_offset_coordinates, resolve_block_state, unpack_offset_coordinates,
};

fn id(s: &str) -> Identifier {
    Identifier::parse(s).expect("valid identifier")
}

fn synthetic_blocks_and_tags() -> (BlockStateTable, BlockTags) {
    let air = BlockDef::new(id("minecraft:air"), vec![]).unwrap();
    let stone = BlockDef::new(id("minecraft:stone"), vec![]).unwrap();
    let water = BlockDef::new(
        id("minecraft:water"),
        vec![PropertyDef::integer("level", 0, 15).unwrap()],
    )
    .unwrap();
    let lava = BlockDef::new(
        id("minecraft:lava"),
        vec![PropertyDef::integer("level", 0, 15).unwrap()],
    )
    .unwrap();
    let oak_leaves = BlockDef::new(
        id("minecraft:oak_leaves"),
        vec![
            PropertyDef::integer("distance", 1, 7).unwrap(),
            PropertyDef::boolean("persistent").unwrap(),
            PropertyDef::boolean("waterlogged").unwrap(),
        ],
    )
    .unwrap()
    .with_default_values(BTreeMap::from([
        ("distance".to_owned(), "7".to_owned()),
        ("persistent".to_owned(), "false".to_owned()),
        ("waterlogged".to_owned(), "false".to_owned()),
    ]));
    let cave_air = BlockDef::new(id("minecraft:cave_air"), vec![]).unwrap();
    let short_grass = BlockDef::new(id("minecraft:short_grass"), vec![]).unwrap();
    let stairs = BlockDef::new(
        id("minecraft:stone_stairs"),
        vec![PropertyDef::boolean("waterlogged").unwrap()],
    )
    .unwrap()
    .with_default_values(BTreeMap::from([(
        "waterlogged".to_owned(),
        "false".to_owned(),
    )]));

    let table = BlockStateTable::from_blocks(vec![
        air,
        stone,
        water,
        lava,
        oak_leaves,
        cave_air,
        short_grass,
        stairs,
    ])
    .unwrap();

    let stone_pid = table
        .block(&id("minecraft:stone"))
        .unwrap()
        .block_protocol_id;
    let leaves_pid = table
        .block(&id("minecraft:oak_leaves"))
        .unwrap()
        .block_protocol_id;
    let stairs_pid = table
        .block(&id("minecraft:stone_stairs"))
        .unwrap()
        .block_protocol_id;

    let mut map = HashMap::new();
    map.insert(
        id(TAG_BLOCKS_MOTION_IN_HEIGHTMAP),
        vec![stone_pid, leaves_pid, stairs_pid],
    );
    map.insert(
        id(TAG_BLOCKS_MOTION_IN_HEIGHTMAP_NO_LEAVES),
        vec![stone_pid, stairs_pid],
    );

    (table, BlockTags::from_resolved(map))
}

fn flat_router(final_density: DensityFunction) -> NoiseRouter {
    NoiseRouter {
        temperature: DensityFunction::Constant(0.0),
        vegetation: DensityFunction::Constant(0.0),
        continents: DensityFunction::Constant(0.0),
        erosion: DensityFunction::Constant(0.0),
        depth: DensityFunction::Constant(0.0),
        ridges: DensityFunction::Constant(0.0),
        chunk_surface_level: DensityFunction::Constant(0.0),
        final_density,
    }
}

fn make_settings(
    min_y: i32,
    height: i32,
    sea_level: i32,
    default_block: &str,
    default_fluid: &str,
    final_density: DensityFunction,
    aquifers: Option<AquiferConfig>,
) -> NoiseGeneratorSettings {
    NoiseGeneratorSettings::new(
        NoiseSettings::new(min_y, height).unwrap(),
        BlockStateSpec {
            name: id(default_block),
            properties: BTreeMap::new(),
        },
        BlockStateSpec {
            name: id(default_fluid),
            properties: BTreeMap::new(),
        },
        flat_router(final_density),
        MaterialRuleRef::Inline(Json::Object(Default::default())),
        Vec::new(),
        sea_level,
        false,
        aquifers,
        false,
        Vec::new(),
    )
}

#[test]
fn resolve_block_state_lenient_5006_properties() {
    let (blocks, _) = synthetic_blocks_and_tags();

    // Bare default state.
    let leaves_default = blocks
        .block(&id("minecraft:oak_leaves"))
        .unwrap()
        .default_state;
    assert_eq!(
        resolve_block_state(
            &blocks,
            &BlockStateSpec {
                name: id("minecraft:oak_leaves"),
                properties: BTreeMap::new(),
            },
        ),
        Ok(leaves_default)
    );

    // Partial valid + unknown + invalid property silently falls back to defaults.
    let expected = blocks
        .id_of_exact(
            &id("minecraft:oak_leaves"),
            &[
                ("distance", "3"),
                ("persistent", "false"),
                ("waterlogged", "true"),
            ],
        )
        .unwrap();
    let spec = BlockStateSpec {
        name: id("minecraft:oak_leaves"),
        properties: BTreeMap::from([
            ("distance".to_owned(), "3".to_owned()),
            ("persistent".to_owned(), "not_a_bool".to_owned()),
            ("waterlogged".to_owned(), "true".to_owned()),
            ("unknown_prop".to_owned(), "42".to_owned()),
        ]),
    };
    assert_eq!(resolve_block_state(&blocks, &spec), Ok(expected));

    // Unknown block fails.
    assert_eq!(
        resolve_block_state(
            &blocks,
            &BlockStateSpec {
                name: id("minecraft:does_not_exist"),
                properties: BTreeMap::new(),
            },
        ),
        Err(id("minecraft:does_not_exist"))
    );
}

#[test]
fn global_fluid_picker_matches_all_vanilla_preset_regimes() {
    let (blocks, _) = synthetic_blocks_and_tags();
    let air = blocks.block(&id("minecraft:air")).unwrap().default_state;
    let water = blocks.block(&id("minecraft:water")).unwrap().default_state;
    let lava = blocks.block(&id("minecraft:lava")).unwrap().default_state;

    // Overworld: sea_level = 63, default_fluid = water
    let ow = GlobalFluidPicker::new(63, water, lava);
    assert_eq!(ow.compute_fluid(0, -55, 0).at(-55, air), lava);
    assert_eq!(ow.compute_fluid(0, -54, 0).at(-54, air), water);
    assert_eq!(ow.compute_fluid(0, 62, 0).at(62, air), water);
    assert_eq!(ow.compute_fluid(0, 63, 0).at(63, air), air);

    // Nether: sea_level = 32, default_fluid = lava, y >= 0
    let nether = GlobalFluidPicker::new(32, lava, lava);
    assert_eq!(nether.compute_fluid(0, 0, 0).at(0, air), lava);
    assert_eq!(nether.compute_fluid(0, 31, 0).at(31, air), lava);
    assert_eq!(nether.compute_fluid(0, 32, 0).at(32, air), air);

    // End: sea_level = 0, default_fluid = air
    let end = GlobalFluidPicker::new(0, air, lava);
    assert_eq!(end.compute_fluid(0, 0, 0).at(0, air), air);

    // Floating islands: sea_level = -64, default_fluid = water -> lavaCutoff = -64
    let fi = GlobalFluidPicker::new(-64, water, lava);
    assert_eq!(fi.compute_fluid(0, -65, 0).at(-65, air), lava);
    assert_eq!(fi.compute_fluid(0, -64, 0).at(-64, air), air);
    assert_eq!(fi.compute_fluid(0, 0, 0).at(0, air), air);
}

#[test]
fn heightmap_predicates_and_updates() {
    let (blocks, tags) = synthetic_blocks_and_tags();
    let preds = BlockPredicates::new(&blocks, &tags).unwrap();

    let air = preds.air_id();
    let cave_air = blocks
        .block(&id("minecraft:cave_air"))
        .unwrap()
        .default_state;
    let stone = blocks.block(&id("minecraft:stone")).unwrap().default_state;
    let water = preds.water_id();
    let lava = preds.lava_id();
    let grass = blocks
        .block(&id("minecraft:short_grass"))
        .unwrap()
        .default_state;
    let leaves_dry = blocks
        .block(&id("minecraft:oak_leaves"))
        .unwrap()
        .default_state;
    let leaves_wet = blocks
        .id_of_exact(
            &id("minecraft:oak_leaves"),
            &[
                ("distance", "7"),
                ("persistent", "false"),
                ("waterlogged", "true"),
            ],
        )
        .unwrap();

    // Air & cave_air are non-opaque for all heightmaps.
    for kind in [
        HeightmapKind::WorldSurfaceWg,
        HeightmapKind::WorldSurface,
        HeightmapKind::OceanFloorWg,
        HeightmapKind::OceanFloor,
        HeightmapKind::MotionBlocking,
        HeightmapKind::MotionBlockingNoLeaves,
    ] {
        assert!(!preds.is_opaque(kind, air), "{kind:?} air");
        assert!(!preds.is_opaque(kind, cave_air), "{kind:?} cave_air");
        assert!(preds.is_opaque(kind, stone), "{kind:?} stone");
    }

    // Water & lava: WORLD_SURFACE + MOTION_BLOCKING*, NOT OCEAN_FLOOR*.
    for fluid in [water, lava] {
        assert!(preds.is_opaque(HeightmapKind::WorldSurfaceWg, fluid));
        assert!(!preds.is_opaque(HeightmapKind::OceanFloorWg, fluid));
        assert!(preds.is_opaque(HeightmapKind::MotionBlocking, fluid));
        assert!(preds.is_opaque(HeightmapKind::MotionBlockingNoLeaves, fluid));
    }

    // Short grass: only WORLD_SURFACE*.
    assert!(preds.is_opaque(HeightmapKind::WorldSurfaceWg, grass));
    assert!(!preds.is_opaque(HeightmapKind::OceanFloorWg, grass));
    assert!(!preds.is_opaque(HeightmapKind::MotionBlocking, grass));
    assert!(!preds.is_opaque(HeightmapKind::MotionBlockingNoLeaves, grass));

    // Dry oak leaves: MOTION_BLOCKING yes, MOTION_BLOCKING_NO_LEAVES no.
    assert!(preds.is_opaque(HeightmapKind::OceanFloorWg, leaves_dry));
    assert!(preds.is_opaque(HeightmapKind::MotionBlocking, leaves_dry));
    assert!(!preds.is_opaque(HeightmapKind::MotionBlockingNoLeaves, leaves_dry));

    // Waterlogged oak leaves: MOTION_BLOCKING_NO_LEAVES yes because of fluid state!
    assert!(preds.has_fluid(leaves_wet));
    assert!(preds.is_opaque(HeightmapKind::MotionBlockingNoLeaves, leaves_wet));

    // Test Heightmap::update raising and lowering.
    let min_y = -64;
    let mut col = [air; 128];
    col[10] = stone; // y = -54
    col[20] = stone; // y = -44
    let mut hm = Heightmap::new(HeightmapKind::OceanFloorWg, min_y);
    assert_eq!(hm.first_available(0, 0), min_y);
    assert!(hm.update(0, -44, 0, stone, &preds, |_, y, _| col
        [(y - min_y) as usize]));
    assert_eq!(hm.first_available(0, 0), -43);
    // Below top-1 does nothing.
    assert!(!hm.update(0, -54, 0, stone, &preds, |_, y, _| col
        [(y - min_y) as usize]));
    // Removing top opaque block rescans downward to y = -54 -> first_available = -53.
    col[20] = air;
    assert!(hm.update(0, -44, 0, air, &preds, |_, y, _| col[(y - min_y) as usize]));
    assert_eq!(hm.first_available(0, 0), -53);
}

#[test]
fn pack_and_unpack_post_processing_offset() {
    for x in [-16, -1, 0, 7, 15, 31] {
        for y in [-64, -1, 0, 12, 15, 319] {
            for z in [-17, -1, 0, 9, 15, 100] {
                let packed = pack_offset_coordinates(x, y, z);
                assert_eq!(
                    packed,
                    ((x & 15) | ((y & 15) << 4) | ((z & 15) << 8)) as u16
                );
                assert_eq!(unpack_offset_coordinates(packed), (x & 15, y & 15, z & 15));
            }
        }
    }
}

#[test]
fn disabled_aquifer_base_column_height_and_fill_chunk() {
    let (blocks, tags) = synthetic_blocks_and_tags();
    let stone = blocks.block(&id("minecraft:stone")).unwrap().default_state;
    let water = blocks.block(&id("minecraft:water")).unwrap().default_state;
    let lava = blocks.block(&id("minecraft:lava")).unwrap().default_state;
    let air = blocks.block(&id("minecraft:air")).unwrap().default_state;

    // Density > 0 for y <= 10, <= 0 for y > 10 (wait: gradient from y=0 -> +10 to y=20 -> -10 crosses 0 at y=10).
    // Specifically: at y=10 -> 0.0 (not > 0, so fluid!), at y=9 -> +1.0 (solid stone).
    let density = DensityFunction::Gradient {
        axis: zenith_worldgen::Axis3D::Y,
        tiling: zenith_worldgen::GradientTiling::ClampToEdge,
        from_coordinate: 0,
        to_coordinate: 20,
        from_value: 10.0,
        to_value: -10.0,
    };
    // Make a pocket below -54 by overriding y < -54 with negative density using RangeChoice:
    // y in [-64, -54) -> -1.0 (deep lava), y in [-54, 10) -> +1.0 (stone), y >= 10 -> -1.0 (water up to 32, air >= 32)
    let _ = density;
    let pocket_density = DensityFunction::RangeChoice {
        input: Box::new(DensityFunction::Gradient {
            axis: zenith_worldgen::Axis3D::Y,
            tiling: zenith_worldgen::GradientTiling::ClampToEdge,
            from_coordinate: -64,
            to_coordinate: 64,
            from_value: -64.0,
            to_value: 64.0,
        }),
        min_inclusive: -54.0,
        max_exclusive: 10.0,
        when_in_range: Box::new(DensityFunction::Constant(1.0)),
        when_out_of_range: Box::new(DensityFunction::Constant(-1.0)),
    };

    let settings = make_settings(
        -64,
        128,
        32,
        "minecraft:stone",
        "minecraft:water",
        pocket_density,
        None,
    );
    let mut ns_map = BTreeMap::new();
    ns_map.insert(id("minecraft:test"), settings);
    let data = WorldgenData::from_maps(BTreeMap::new(), BTreeMap::new(), ns_map);
    let generator = NoiseGenerator::new(&data, &id("minecraft:test"), 42, &blocks, &tags).unwrap();

    let col = generator.base_column(5, -3);
    assert_eq!(col.len(), 128);
    for (idx, &state) in col.iter().enumerate() {
        let y = -64 + idx as i32;
        let expected = if y < GLOBAL_LAVA_LEVEL {
            lava
        } else if y < 10 {
            stone
        } else if y < 32 {
            water
        } else {
            air
        };
        assert_eq!(state, expected, "mismatch at y={y}");
    }

    assert_eq!(
        generator.base_height(5, -3, HeightmapKind::OceanFloorWg),
        10
    );
    assert_eq!(
        generator.base_height(5, -3, HeightmapKind::WorldSurfaceWg),
        32
    );
    assert_eq!(
        generator.base_height(5, -3, HeightmapKind::MotionBlocking),
        32
    );
    assert_eq!(
        generator.base_height(5, -3, HeightmapKind::MotionBlockingNoLeaves),
        32
    );

    let mut chunk = generator.fill_chunk(0, -1);
    assert_eq!(chunk.section_count(), 8);
    assert_eq!(chunk.blocks().len(), 8 * 4096);
    assert!(chunk.post_processing_marks().is_empty());

    // Check section bottom-up + (ly*16 + lz)*16 + lx indexing matches base_column.
    for z in 0..16 {
        for x in 0..16 {
            assert_eq!(chunk.ocean_floor_wg().first_available(x, z), 10);
            assert_eq!(chunk.world_surface_wg().first_available(x, z), 32);
            for y in -64..64 {
                let sec = ((y >> 4) - (-64 >> 4)) as usize;
                let ly = (y & 15) as usize;
                let via_raw = chunk.section(sec)[(ly * 16 + z) * 16 + x];
                assert_eq!(via_raw, col[(y + 64) as usize]);
                assert_eq!(chunk.get_block(x, y, z), via_raw);
            }
        }
    }

    chunk.prime_final_heightmaps(generator.predicates());
    assert_eq!(
        chunk
            .heightmap(HeightmapKind::MotionBlocking)
            .unwrap()
            .first_available(0, 0),
        32
    );
    assert_eq!(
        chunk
            .heightmap(HeightmapKind::OceanFloor)
            .unwrap()
            .first_available(0, 0),
        10
    );
}

#[test]
fn disabled_aquifer_direct_behavior() {
    let (blocks, tags) = synthetic_blocks_and_tags();
    let preds = BlockPredicates::new(&blocks, &tags).unwrap();
    let picker = GlobalFluidPicker::new(63, preds.water_id(), preds.lava_id());
    let mut aq = Aquifer::disabled(picker, preds.air_id());
    let mut ctx = SamplerContext::uncached();

    assert_eq!(aq.compute_substance(&mut ctx, 0, 10, 0, 0.0001), None);
    assert!(!aq.should_schedule_fluid_update());
    assert_eq!(
        aq.compute_substance(&mut ctx, 0, 10, 0, 0.0),
        Some(preds.water_id())
    );
    assert!(!aq.should_schedule_fluid_update());
    assert_eq!(
        aq.compute_substance(&mut ctx, 0, 63, 0, -1.0),
        Some(preds.air_id())
    );
    assert_eq!(
        aq.compute_substance(&mut ctx, 0, -55, 0, -1.0),
        Some(preds.lava_id())
    );
}

#[test]
fn noise_based_aquifer_via_generator_dry_and_lava() {
    let (blocks, tags) = synthetic_blocks_and_tags();
    let air = blocks.block(&id("minecraft:air")).unwrap().default_state;
    let lava = blocks.block(&id("minecraft:lava")).unwrap().default_state;
    let water = blocks.block(&id("minecraft:water")).unwrap().default_state;

    // 1) Exclusion > 0 makes all underground aquifer cells dry (WAY_BELOW_MIN_Y), while y < -54
    //    still hits the deep global lava fast path.
    let excluded_cfg = AquiferConfig {
        barrier: DensityFunction::Constant(-1.0),
        fluid_level_floodedness: DensityFunction::Constant(1.0),
        fluid_level_spread: DensityFunction::Constant(0.0),
        lava: DensityFunction::Constant(0.0),
        exclusion: DensityFunction::Constant(1.0),
        surface_level: DensityFunction::Constant(80.0),
    };
    let settings = make_settings(
        -64,
        192,
        63,
        "minecraft:stone",
        "minecraft:water",
        DensityFunction::Constant(-1.0),
        Some(excluded_cfg),
    );
    let rs = RandomState::from_settings(settings, 12345).unwrap();
    let generator = NoiseGenerator::from_random_state(rs, &blocks, &tags).unwrap();
    let col = generator.base_column(0, 0);
    assert_eq!(col[0], lava); // y = -64 < -54
    assert_eq!(col[9], lava); // y = -55 < -54
    assert_eq!(col[10], air); // y = -54 excluded -> dry -> air
    assert_eq!(col[64], air); // y = 0 excluded -> dry -> air

    // 2) Deep partially-flooded aquifer with lava noise > 0.3 produces lava below cell level.
    //    Use density = -10.0 so water/lava pressure barriers (+2.0) don't turn border voxels to stone.
    let lava_cfg = AquiferConfig {
        barrier: DensityFunction::Constant(-1.0),
        fluid_level_floodedness: DensityFunction::Constant(0.6), // > 0.4 partial, < 0.8 full
        fluid_level_spread: DensityFunction::Constant(0.0),
        lava: DensityFunction::Constant(0.5), // |0.5| > 0.3 -> lava when level <= -10
        exclusion: DensityFunction::Constant(-1.0),
        surface_level: DensityFunction::Constant(80.0),
    };
    let settings2 = make_settings(
        -64,
        192,
        63,
        "minecraft:stone",
        "minecraft:water",
        DensityFunction::Constant(-10.0),
        Some(lava_cfg),
    );
    let rs2 = RandomState::from_settings(settings2, 99).unwrap();
    let gen2 = NoiseGenerator::from_random_state(rs2, &blocks, &tags).unwrap();
    let col2 = gen2.base_column(0, 0);
    // At y = -25, closest Voronoi cell center lies in [-36, -16] -> cell_y = -1 -> cell_middle_y = -20 <= -10,
    // and -25 < -20 -> lava!
    assert_eq!(col2[(-25 + 64) as usize], lava);
    // At y = 10, closest Voronoi cell center lies in [0, 20] -> cell_y = 0 -> cell_middle_y = 20 > -10,
    // and 10 < 20 -> water!
    assert_eq!(col2[(10 + 64) as usize], water);

    // FluidStatus helper check
    let st = FluidStatus {
        fluid_level: WAY_BELOW_MIN_Y,
        fluid_type: water,
    };
    assert_eq!(st.at(-64, air), air);
    assert!((FLOWING_UPDATE_SIMILARITY - (-0.76_f64)).abs() < 1e-15);
}

#[test]
fn beardifier_empty_and_nonempty_modes() {
    let empty = Beardifier::empty();
    assert!(empty.is_empty());
    assert_eq!(empty.sample_value(0, 0, 0), 0.0);
    let vol = DensityVolume::unit(4, 4, 4, -2, -2, -2).unwrap();
    let mut out = vec![1.0_f32; vol.len()];
    empty.sample_volume(&mut out, vol);
    assert!(out.iter().all(|&v| v == 0.0));

    // Non-empty beardifier with BeardThin + Bury + JigsawJunction.
    let b = Beardifier::new(
        vec![
            BeardifierRigid {
                bounding_box: BoundingBox {
                    min_x: 0,
                    min_y: 10,
                    min_z: 0,
                    max_x: 4,
                    max_y: 16,
                    max_z: 4,
                },
                adjustment: TerrainAdjustment::BeardThin,
                ground_level_delta: 0,
            },
            BeardifierRigid {
                bounding_box: BoundingBox::point(20, 20, 20),
                adjustment: TerrainAdjustment::Bury,
                ground_level_delta: 0,
            },
            BeardifierRigid {
                bounding_box: BoundingBox::point(40, 40, 40),
                adjustment: TerrainAdjustment::None,
                ground_level_delta: 0,
            },
        ],
        vec![JigsawJunction {
            source_x: 2,
            source_ground_y: 10,
            source_z: 2,
        }],
    );
    assert!(!b.is_empty());

    // Below ground (y = 8 < 10) beard is positive (builds terrain up); above ground (y = 12) negative (carves).
    assert!(b.sample_value(2, 8, 2) > 0.0);
    assert!(b.sample_value(2, 12, 2) < 0.0);
    // At bury center (20, 20, 20), dx=dy=dz=0 -> bury = 1.0.
    assert_eq!(b.sample_value(20, 20, 20), 1.0);
    // Outside affected_box -> exact 0.0.
    assert_eq!(b.sample_value(1000, 1000, 1000), 0.0);

    // sample_volume matches sample_value bit-for-bit.
    let v2 = DensityVolume::unit(8, 8, 8, -2, 6, -2).unwrap();
    let mut buf = vec![0.0_f32; v2.len()];
    b.sample_volume(&mut buf, v2);
    for iz in 0..8 {
        for ix in 0..8 {
            for iy in 0..8 {
                let got = buf[v2.index(ix, iy, iz)];
                let exp = b.sample_value(v2.block_x(ix), v2.block_y(iy), v2.block_z(iz));
                assert_eq!(got.to_bits(), exp.to_bits());
            }
        }
    }
}

#[test]
fn blender_empty_is_exact_identity() {
    let blender = Blender::empty();
    assert!(blender.is_empty());

    let expr = DensityFunction::BlendDensity(Box::new(DensityFunction::Binary {
        op: zenith_worldgen::BinaryOp::Add,
        left: Box::new(DensityFunction::BlendAlpha),
        right: Box::new(DensityFunction::BlendOffset),
    }));
    let (sampler, caches) = RandomState::compile_function(&expr, 7, false).unwrap();
    let vol = DensityVolume::unit(4, 4, 4, 0, 0, 0).unwrap();
    let mut ctx = blender.attach_to_context(SamplerContext::caching(caches), vol);
    assert_eq!(sampler.sample_value(&mut ctx, 1, 2, 3), 1.0_f32);
    let mut out = vec![0.0_f32; vol.len()];
    sampler.sample_volume(&mut ctx, &mut out, vol);
    assert!(out.iter().all(|&v| v == 1.0_f32));

    // Attach Beardifier::empty() too and confirm 0.0 contribution.
    let beard_fn = DensityFunction::Beardifier;
    let (b_sampler, _) = RandomState::compile_function(&beard_fn, 7, false).unwrap();
    let mut ctx2 = SamplerContext::uncached().with_beardifier(Arc::new(Beardifier::empty()));
    assert_eq!(b_sampler.sample_value(&mut ctx2, 5, 10, -5), 0.0_f32);
}
