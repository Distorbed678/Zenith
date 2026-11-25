//! Clean-room unit tests for the 26.3 `terrain` chunk step (no game jar required):
//! - Vertical anchors & value/height providers (`providers.rs`)
//! - Biome temperature evaluation (`coldEnoughToSnow`, `shouldMeltFrozenOceanIcebergSlightly`)
//! - `CarvingMask` bit layout and vertical run splitting, `setLargeFeatureSeed`, cave/canyon carvers
//! - Material rules & conditions decoding, tag/reference resolution, cycle detection, `OreVeinRule`
//! - Synthetic end-to-end `TerrainGenerator::generate_chunk` (biomes, fill, surface, carvers, heightmaps)

use std::collections::HashMap;

use zenith_biome::{BiomeSource, FixedBiomeSource};
use zenith_codec::json::{ParseMode, parse};
use zenith_codec::log::null_log;
use zenith_codec::resources::{MemoryPack, PathRules};
use zenith_math::{LegacyRandom, RandomSource};
use zenith_registry::{BlockDef, BlockStateTable, Identifier};
use zenith_worldgen::carver::{CarvingMask, decode_carver, set_large_feature_seed};
use zenith_worldgen::providers::{
    FloatProvider, FloatProviderCodec, HeightProvider, HeightProviderCodec, IntProvider,
    IntProviderCodec, VerticalAnchor, VerticalAnchorCodec, WorldGenerationContext,
};
use zenith_worldgen::{
    BiomeBitSet, BiomeData, BiomeId, BlockTags, HeightmapKind, LoadError, TAG_UNCARVABLE,
    TemperatureModifier, TerrainGenerator, WorldgenData,
};

fn id(s: &str) -> Identifier {
    Identifier::parse(s).unwrap()
}

fn minimal_block_table() -> BlockStateTable {
    let names = [
        "air",
        "cave_air",
        "void_air",
        "stone",
        "water",
        "lava",
        "bedrock",
        "dirt",
        "grass_block",
        "mycelium",
        "sand",
        "deepslate",
        "terracotta",
        "orange_terracotta",
        "yellow_terracotta",
        "brown_terracotta",
        "red_terracotta",
        "white_terracotta",
        "light_gray_terracotta",
        "snow_block",
        "packed_ice",
        "copper_ore",
        "raw_copper_block",
        "granite",
    ];
    let defs: Vec<BlockDef> = names
        .iter()
        .map(|name| {
            BlockDef::new(Identifier::with_default_namespace(*name).unwrap(), vec![]).unwrap()
        })
        .collect();
    BlockStateTable::from_blocks(defs).expect("valid minimal block table")
}

fn minimal_block_tags(blocks: &BlockStateTable) -> BlockTags {
    let mut motion = Vec::new();
    for b in blocks.blocks() {
        let p = b.id.path();
        if !matches!(p, "air" | "cave_air" | "void_air" | "water" | "lava") {
            motion.push(b.block_protocol_id);
        }
    }
    let bedrock = blocks
        .block(&id("minecraft:bedrock"))
        .unwrap()
        .block_protocol_id;
    let mut map = HashMap::new();
    map.insert(id("minecraft:blocks_motion_in_heightmap"), motion.clone());
    map.insert(id("minecraft:blocks_motion_in_heightmap_no_leaves"), motion);
    map.insert(id(TAG_UNCARVABLE), vec![bedrock]);
    BlockTags::from_resolved(map)
}

#[test]
fn vertical_anchors_resolve_all_four_26_3_forms() {
    use zenith_codec::codec::Codec;

    let ctx = WorldGenerationContext::new(-64, 384, 63);
    assert_eq!(VerticalAnchor::Absolute(12).resolve_y(ctx), 12);
    assert_eq!(VerticalAnchor::AboveBottom(5).resolve_y(ctx), -59);
    assert_eq!(VerticalAnchor::BelowTop(5).resolve_y(ctx), 314);
    assert_eq!(VerticalAnchor::RelativeToSeaLevel(-10).resolve_y(ctx), 53);

    let j = parse(r#"{"relative_to_sea_level": 4}"#, ParseMode::Data).unwrap();
    let decoded = VerticalAnchorCodec.decode(&j).demand().unwrap();
    assert_eq!(decoded, VerticalAnchor::RelativeToSeaLevel(4));
    assert_eq!(decoded.resolve_y(ctx), 67);

    // Out-of-range offset rejected.
    let bad = parse(r#"{"absolute": 3000}"#, ParseMode::Data).unwrap();
    assert!(VerticalAnchorCodec.decode(&bad).demand().is_err());
}

#[test]
fn providers_decode_and_match_exact_rng_draws() {
    use zenith_codec::codec::Codec;

    // IntProvider::VeryBiasedToBottom draws 3 nested nextInt calls.
    let j_int = parse(
        r#"{"type":"minecraft:very_biased_to_bottom","min_inclusive":0,"max_inclusive":14}"#,
        ParseMode::Data,
    )
    .unwrap();
    let ip = IntProviderCodec.decode(&j_int).demand().unwrap();
    let mut r1 = LegacyRandom::new(777);
    let mut r2 = LegacyRandom::new(777);
    let expected = {
        let a = r2.next_int_bounded(15);
        let b = r2.next_int_bounded(a + 1);
        r2.next_int_bounded(b + 1)
    };
    assert_eq!(ip.sample(&mut r1), expected);
    assert_eq!(r1.next_long(), r2.next_long());

    // FloatProvider::Trapezoid draws 2 nextFloat calls.
    let j_flt = parse(
        r#"{"type":"minecraft:trapezoid","min":0.0,"max":3.0,"plateau":1.0}"#,
        ParseMode::Data,
    )
    .unwrap();
    let fp = FloatProviderCodec.decode(&j_flt).demand().unwrap();
    let mut r1 = LegacyRandom::new(888);
    let mut r2 = LegacyRandom::new(888);
    let f1 = r2.next_float();
    let f2 = r2.next_float();
    assert_eq!(
        fp.sample(&mut r1).to_bits(),
        (f1 * 2.0_f32 + f2 * 1.0_f32).to_bits()
    );
    assert_eq!(r1.next_long(), r2.next_long());

    // HeightProvider::Uniform with min > max does NOT consume RNG.
    let hp = HeightProvider::Uniform {
        min_inclusive: VerticalAnchor::Absolute(50),
        max_inclusive: VerticalAnchor::Absolute(40),
    };
    let ctx = WorldGenerationContext::new(-64, 384, 63);
    let mut r1 = LegacyRandom::new(999);
    let mut r2 = LegacyRandom::new(999);
    assert_eq!(hp.sample(&mut r1, ctx), 50);
    assert_eq!(r1.next_long(), r2.next_long());

    // Inline shorthand for HeightProvider / FloatProvider / IntProvider.
    let j_const_h = parse(r#"{"above_bottom": 8}"#, ParseMode::Data).unwrap();
    assert_eq!(
        HeightProviderCodec.decode(&j_const_h).demand().unwrap(),
        HeightProvider::Constant(VerticalAnchor::AboveBottom(8))
    );
    let j_const_f = parse(r#"1.5"#, ParseMode::Data).unwrap();
    assert_eq!(
        FloatProviderCodec.decode(&j_const_f).demand().unwrap(),
        FloatProvider::Constant(1.5_f32)
    );
    let j_const_i = parse(r#"7"#, ParseMode::Data).unwrap();
    assert_eq!(
        IntProviderCodec.decode(&j_const_i).demand().unwrap(),
        IntProvider::Constant(7)
    );
}

#[test]
fn carving_mask_visits_contiguous_vertical_runs_and_splits_columns() {
    let mut mask = CarvingMask::new(-63, 312);
    assert!(mask.is_empty());

    // Two disconnected vertical runs in column (0, 0) plus top boundary touching column (0, 1).
    mask.carve(0, -60, 0);
    mask.carve(0, -59, 0);
    mask.carve(0, 10, 0);
    mask.carve(0, 11, 0);
    mask.carve(0, 12, 0);
    mask.carve(0, 312, 0);
    mask.carve(0, -63, 1);
    mask.carve(15, 0, 15);
    assert!(!mask.is_empty());

    let mut runs = Vec::new();
    mask.visit_runs(|lx, lz, bot, top| runs.push((lx, lz, bot, top)));
    assert_eq!(
        runs,
        vec![
            (0, 0, -60, -59),
            (0, 0, 10, 12),
            (0, 0, 312, 312),
            (0, 1, -63, -63),
            (15, 15, 0, 0),
        ]
    );
}

#[test]
fn carver_seeding_and_cave_canyon_mask_bounds() {
    let cave_json = parse(
        r#"{
            "type": "minecraft:cave",
            "probability": 1.0,
            "y": {"type": "minecraft:uniform", "min_inclusive": {"above_bottom": 8}, "max_inclusive": {"absolute": 80}},
            "count": 4,
            "thickness": 2.0,
            "weird_thickness_bias": true,
            "room_vertical_radius_multiplier": 0.5,
            "horizontal_radius_multiplier": 1.0,
            "vertical_radius_multiplier": 1.0,
            "floor_level": -0.7
        }"#,
        ParseMode::Data,
    )
    .unwrap();
    let cave = decode_carver(&cave_json).demand().unwrap();

    let canyon_json = parse(
        r#"{
            "type": "minecraft:canyon",
            "probability": 1.0,
            "y": {"absolute": 40},
            "vertical_rotation": 0.0,
            "shape": {
                "distance_factor": 1.0,
                "thickness": 4.0,
                "width_smoothness": 3,
                "horizontal_radius_factor": 1.0,
                "vertical_radius_default_factor": 1.0,
                "vertical_radius_center_factor": 0.0,
                "y_scale": 3.0
            }
        }"#,
        ParseMode::Data,
    )
    .unwrap();
    let canyon = decode_carver(&canyon_json).demand().unwrap();

    let ctx = WorldGenerationContext::new(-64, 192, 63);
    let mask_min_y = ctx.min_gen_y + 1;
    let mask_max_y = ctx.min_gen_y + ctx.gen_depth - 1 - 7;
    let mut mask = CarvingMask::new(mask_min_y, mask_max_y);
    let mut rng = LegacyRandom::new(0);

    set_large_feature_seed(&mut rng, 12345, 5, 5);
    assert!(cave.is_start_chunk(&mut rng));
    cave.carve(ctx, &mut rng, 5, 5, 5, 5, &mut mask);

    set_large_feature_seed(&mut rng, 12346, 5, 5);
    assert!(canyon.is_start_chunk(&mut rng));
    canyon.carve(ctx, &mut rng, 5, 5, 5, 5, &mut mask);

    assert!(!mask.is_empty());
    // Spec Edge Case #10: world_y > mask_min_y strictly, so mask_min_y is NEVER carved.
    for lx in 0..16 {
        for lz in 0..16 {
            assert!(!mask.is_carved(lx, mask_min_y, lz));
        }
    }
}

#[test]
fn biome_temperature_and_bitset_operations() {
    let warm = BiomeData {
        temperature: 0.8,
        temperature_modifier: TemperatureModifier::None,
        carvers: Vec::new(),
    };
    assert!(!warm.cold_enough_to_snow(0, 64, 0, 63));
    assert!(!warm.cold_enough_to_snow(0, 80, 0, 63));
    // Lapse rate is 0.05/40 = 0.00125 per block above sea_level + 17 (80):
    // at y = 700, 0.8 - ~620 * 0.00125 ≈ 0.025 < 0.15.
    assert!(warm.cold_enough_to_snow(0, 700, 0, 63));

    let cold = BiomeData {
        temperature: 0.0,
        temperature_modifier: TemperatureModifier::Frozen,
        carvers: Vec::new(),
    };
    // Frozen modifier can warm specific columns to 0.2 (> 0.1) while keeping others at 0.0.
    let mut saw_melt = false;
    let mut saw_frozen = false;
    for x in 0..64 {
        if cold.should_melt_frozen_ocean_iceberg_slightly(x * 4, 63, 0, 63) {
            saw_melt = true;
        } else {
            saw_frozen = true;
        }
    }
    assert!(saw_melt && saw_frozen);

    let mut a = BiomeBitSet::new(130);
    let mut b = BiomeBitSet::new(130);
    a.insert(BiomeId(2));
    a.insert(BiomeId(70));
    b.insert(BiomeId(2));
    b.insert(BiomeId(70));
    b.insert(BiomeId(129));
    assert!(a.is_subset_of(&b));
    assert!(!b.is_subset_of(&a));
    assert!(a.intersects(&b));
}

#[test]
fn synthetic_terrain_generator_runs_full_pipeline_and_detects_rule_cycles() {
    let pack = MemoryPack::new("synth", PathRules::Zip, null_log())
        .with_file(
            "data/minecraft/worldgen/noise/surface.json",
            r#"{"base_octave": -6, "octave_count": 3}"#,
        )
        .with_file(
            "data/minecraft/worldgen/biome/plains.json",
            r#"{"temperature": 0.8, "carvers": []}"#,
        )
        .with_file(
            "data/minecraft/tags/worldgen/biome/is_test.json",
            r#"{"values": ["minecraft:plains"]}"#,
        )
        .with_file(
            "data/minecraft/worldgen/material_condition/on_floor.json",
            r#"{"type": "minecraft:stone_depth", "offset": 0, "add_surface_depth": false, "secondary_depth_range": 0, "surface_type": "floor"}"#,
        )
        .with_file(
            "data/minecraft/worldgen/material_rule/test_rule.json",
            r##"{
                "type": "minecraft:sequence",
                "sequence": [
                    {
                        "type": "minecraft:condition",
                        "if_true": {
                            "type": "minecraft:vertical_gradient",
                            "random_name": "minecraft:bedrock_floor",
                            "true_at_and_below": {"above_bottom": 0},
                            "false_at_and_above": {"above_bottom": 1}
                        },
                        "then_run": {"type": "minecraft:block", "result_state": "minecraft:bedrock"}
                    },
                    {
                        "type": "minecraft:condition",
                        "if_true": {
                            "type": "minecraft:biome",
                            "biome_is": "#minecraft:is_test"
                        },
                        "then_run": {
                            "type": "minecraft:condition",
                            "if_true": "minecraft:on_floor",
                            "then_run": {"type": "minecraft:block", "result_state": "minecraft:grass_block"}
                        }
                    },
                    {
                        "type": "minecraft:ore_vein",
                        "ore_block": "minecraft:copper_ore",
                        "raw_ore_block": "minecraft:raw_copper_block",
                        "filler_block": "minecraft:granite",
                        "raw_ore_chance": 0.02,
                        "density": {
                            "type": "minecraft:range_choice",
                            "input": {"type": "minecraft:gradient", "axis": "y", "from_coordinate": 0, "to_coordinate": 64, "from_value": 0.0, "to_value": 64.0},
                            "min_inclusive": 10.0,
                            "max_exclusive": 15.0,
                            "when_in_range": 1.0,
                            "when_out_of_range": -1.0
                        },
                        "richness": 0.5,
                        "filler_gap": -1.0
                    }
                ]
            }"##,
        )
        .with_file(
            "data/minecraft/worldgen/noise_settings/flat_test.json",
            r#"{
                "noise": {"min_y": 0, "height": 64},
                "default_block": "minecraft:stone",
                "default_fluid": "minecraft:water",
                "noise_router": {
                    "temperature": 0.0,
                    "vegetation": 0.0,
                    "continents": 0.0,
                    "erosion": 0.0,
                    "depth": 0.0,
                    "ridges": 0.0,
                    "chunk_surface_level": 32.0,
                    "final_density": {
                        "type": "minecraft:gradient",
                        "axis": "y",
                        "from_coordinate": 0,
                        "to_coordinate": 64,
                        "from_value": 1.0,
                        "to_value": -1.0
                    }
                },
                "material_rule": "minecraft:test_rule",
                "spawn_target": [],
                "sea_level": 20,
                "disable_mob_generation": false,
                "legacy_random_source": false
            }"#,
        );

    let data = WorldgenData::load(&pack).expect("synthetic pack loads");
    let blocks = minimal_block_table();
    let tags = minimal_block_tags(&blocks);
    let bs = BiomeSource::Fixed(FixedBiomeSource::new(id("minecraft:plains")));
    let generator =
        TerrainGenerator::new(&data, bs, &id("minecraft:flat_test"), 42, &blocks, &tags)
            .expect("terrain generator constructs");

    let chunk = generator.generate_chunk(2, -3);
    assert_eq!(chunk.min_y(), 0);
    assert_eq!(chunk.height(), 64);
    assert_eq!(chunk.biome_indices().len(), 4 * 64);
    assert_eq!(chunk.block_tick_count(), 0);
    assert_eq!(chunk.fluid_tick_count(), 0);

    let bedrock_id = blocks
        .block(&id("minecraft:bedrock"))
        .unwrap()
        .default_state;
    let grass_id = blocks
        .block(&id("minecraft:grass_block"))
        .unwrap()
        .default_state;
    let copper_id = blocks
        .block(&id("minecraft:copper_ore"))
        .unwrap()
        .default_state;
    let raw_copper_id = blocks
        .block(&id("minecraft:raw_copper_block"))
        .unwrap()
        .default_state;
    let granite_id = blocks
        .block(&id("minecraft:granite"))
        .unwrap()
        .default_state;

    // y = 0 is bedrock, top solid block (y = 31) is grass_block, y = 10..15 is ore vein blocks.
    assert_eq!(chunk.get_block(0, 0, 0), bedrock_id);
    let top_y = chunk.world_surface_wg().first_available(0, 0) - 1;
    assert_eq!(top_y, 31);
    assert_eq!(chunk.get_block(0, top_y, 0), grass_id);
    for y in 10..15 {
        let s = chunk.get_block(0, y, 0);
        assert!(s == copper_id || s == raw_copper_id || s == granite_id);
    }

    // All 6 heightmaps are present and agree for this solid terrain.
    for kind in [
        HeightmapKind::OceanFloorWg,
        HeightmapKind::WorldSurfaceWg,
        HeightmapKind::OceanFloor,
        HeightmapKind::WorldSurface,
        HeightmapKind::MotionBlocking,
        HeightmapKind::MotionBlockingNoLeaves,
    ] {
        assert_eq!(chunk.heightmap(kind).unwrap().first_available(0, 0), 32);
    }

    // Cycle detection on material_rule.
    let cyclic = MemoryPack::new("cyc", PathRules::Zip, null_log())
        .with_file(
            "data/minecraft/worldgen/material_rule/a.json",
            r#""minecraft:b""#,
        )
        .with_file(
            "data/minecraft/worldgen/material_rule/b.json",
            r#""minecraft:a""#,
        );
    assert!(matches!(
        WorldgenData::load(&cyclic),
        Err(LoadError::MaterialRuleCycle { .. })
    ));

    // Cycle detection on material_condition.
    let cyclic_c = MemoryPack::new("cyc_c", PathRules::Zip, null_log())
        .with_file(
            "data/minecraft/worldgen/material_condition/x.json",
            r#""minecraft:y""#,
        )
        .with_file(
            "data/minecraft/worldgen/material_condition/y.json",
            r#""minecraft:x""#,
        );
    assert!(matches!(
        WorldgenData::load(&cyclic_c),
        Err(LoadError::MaterialConditionCycle { .. })
    ));
}
