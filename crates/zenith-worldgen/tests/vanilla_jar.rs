//! Integration test loading all vanilla 26.3 worldgen registries (`worldgen/noise`,
//! `worldgen/density_function`, `worldgen/noise_settings`) from the user's own client jar.
//!
//! Skips cleanly (prints a note and passes) when no 26.3 install is present on the host.

use zenith_biome::{BiomeSource, MultiNoiseBiomeSource, MultiNoisePreset, TheEndBiomeSource};
use zenith_codec::log::null_log;
use zenith_codec::resources::ZipPackResources;
use zenith_registry::{BlockStateTable, Identifier};
use zenith_worldgen::interval::contains;
use zenith_worldgen::{
    AquiferConfig, BlockTags, HeightmapKind, NoiseGenerator, RandomState, RouterField,
    TerrainGenerator, WorldgenData,
};

#[test]
fn load_vanilla_jar_and_sample_all_noise_settings() {
    let installs = zenith_install::discover_installs(Some("26.3"));
    let Some(install) = installs.first() else {
        eprintln!("NOTE: no local 26.3 install found; skipping vanilla jar worldgen test");
        return;
    };

    let pack = ZipPackResources::open("vanilla", &install.client_jar, null_log());
    let data = WorldgenData::load(&pack).expect("vanilla 26.3 worldgen registries must load");

    assert!(
        !data.noises().is_empty(),
        "expected vanilla worldgen/noise entries"
    );
    assert!(
        !data.density_functions().is_empty(),
        "expected vanilla worldgen/density_function entries"
    );
    assert!(
        !data.all_noise_settings().is_empty(),
        "expected vanilla worldgen/noise_settings entries"
    );

    // Verify vanilla preset dimensions/flags match docs/specs/worldgen/density-functions.md
    let expected_presets = [
        ("minecraft:overworld", -64, 384, 63, false, true),
        ("minecraft:large_biomes", -64, 384, 63, false, true),
        ("minecraft:amplified", -64, 384, 63, false, true),
        ("minecraft:nether", 0, 128, 32, true, false),
        ("minecraft:end", 0, 128, 0, true, false),
        ("minecraft:caves", -64, 192, 32, true, false),
        ("minecraft:floating_islands", 0, 256, -64, true, false),
    ];
    for (id_str, min_y, height, sea_level, legacy, has_aquifers) in expected_presets {
        let id = zenith_registry::Identifier::parse(id_str).unwrap();
        let s = data
            .noise_settings(&id)
            .unwrap_or_else(|| panic!("missing vanilla preset {id_str}"));
        assert_eq!(s.min_y(), min_y, "{id_str} min_y");
        assert_eq!(s.height(), height, "{id_str} height");
        assert_eq!(s.sea_level(), sea_level, "{id_str} sea_level");
        assert_eq!(s.legacy_random_source(), legacy, "{id_str} legacy");
        assert_eq!(s.aquifers().is_some(), has_aquifers, "{id_str} aquifers");
    }

    let test_points = [(0, 64, 0), (-17, 0, -29), (123, 80, -456), (2048, 32, 2048)];

    for (settings_id, settings) in data.all_noise_settings() {
        let rs = RandomState::new(&data, settings_id, 123456789)
            .unwrap_or_else(|e| panic!("RandomState::new failed for {settings_id}: {e}"));

        // Check settings accessors
        assert!(settings.height() >= 0);
        assert_eq!(settings.min_y() % 16, 0);
        assert_eq!(settings.height() % 16, 0);

        // Sample all 8 router fields at test points and verify bounds
        for &(x, y, z) in &test_points {
            let climate = rs.climate_point(x, y, z);
            for field in RouterField::ALL {
                let v = rs.sample_router(field, x, y, z);
                assert!(
                    !v.is_nan(),
                    "{settings_id} {field:?} returned NaN at ({x},{y},{z})"
                );
                let range = rs.router_range(field);
                assert!(
                    contains(range, v),
                    "{settings_id} {field:?} value {v} outside declared range {range:?} at ({x},{y},{z})"
                );
                if field.index() < 6 {
                    assert_eq!(
                        climate[field.index()].to_bits(),
                        v.to_bits(),
                        "{settings_id} climate_point mismatch for {field:?}"
                    );
                }
            }

            // Sample aquifer config functions if present
            if settings.aquifers().is_some() {
                for idx in 0..AquiferConfig::COUNT {
                    let v = rs
                        .sample_aquifer(idx, x, y, z)
                        .expect("aquifer function present");
                    assert!(
                        !v.is_nan(),
                        "{settings_id} aquifer[{idx}] returned NaN at ({x},{y},{z})"
                    );
                    let range = rs.aquifer_range(idx).unwrap();
                    assert!(
                        contains(range, v),
                        "{settings_id} aquifer[{idx}] value {v} outside declared range {range:?}"
                    );
                }
                assert_eq!(rs.sample_aquifer(AquiferConfig::COUNT, x, y, z), None);
            } else {
                assert_eq!(rs.sample_aquifer(0, x, y, z), None);
            }
        }

        // Sample chunk volume for FinalDensity and Temperature at chunk (0, 0)
        let expected_len = 16 * (settings.height() as usize) * 16;
        let final_vol = rs.sample_volume(RouterField::FinalDensity, 0, 0);
        assert_eq!(final_vol.len(), expected_len);
        let final_range = rs.router_range(RouterField::FinalDensity);
        for &v in &final_vol {
            assert!(!v.is_nan(), "{settings_id} final_density volume had NaN");
            assert!(
                contains(final_range, v),
                "{settings_id} final_density volume value {v} outside {final_range:?}"
            );
        }

        let temp_vol = rs.sample_volume(RouterField::Temperature, 0, 0);
        assert_eq!(temp_vol.len(), expected_len);
        let temp_range = rs.router_range(RouterField::Temperature);
        for &v in &temp_vol {
            assert!(!v.is_nan(), "{settings_id} temperature volume had NaN");
            assert!(
                contains(temp_range, v),
                "{settings_id} temperature volume value {v} outside {temp_range:?}"
            );
        }
    }
}

#[test]
fn fill_vanilla_chunks_and_verify_heightmaps() {
    let installs = zenith_install::discover_installs(Some("26.3"));
    let Some(install) = installs.first() else {
        eprintln!("NOTE: no local 26.3 install found; skipping vanilla chunk fill test");
        return;
    };
    let tables_path = zenith_data::version_dir("26.3").join("tables.bin");
    let Ok(tables) = zenith_data::Tables::load(&tables_path) else {
        eprintln!("NOTE: no cached tables.bin found; skipping vanilla chunk fill test");
        return;
    };

    let blocks = BlockStateTable::from_tables(&tables).expect("block state table");
    let pack = ZipPackResources::open("vanilla", &install.client_jar, null_log());
    let data = WorldgenData::load(&pack).expect("worldgen data");
    let tags = BlockTags::load(&pack, &blocks).expect("block tags");
    assert!(!tags.is_empty(), "expected vanilla block tags");

    for settings_name in ["minecraft:overworld", "minecraft:nether", "minecraft:end"] {
        let sid = Identifier::parse(settings_name).unwrap();
        let generator = NoiseGenerator::new(&data, &sid, 12345, &blocks, &tags)
            .unwrap_or_else(|e| panic!("NoiseGenerator::new failed for {settings_name}: {e}"));

        let mut chunk = generator.fill_chunk(200, 200);
        assert_eq!(
            chunk.section_count(),
            (generator.settings().height() / 16) as usize
        );
        assert_eq!(chunk.blocks().len(), chunk.section_count() * 4096);

        // Primed final heightmaps must agree with the incrementally maintained WG heightmaps.
        chunk.prime_final_heightmaps(generator.predicates());
        assert_eq!(
            chunk.ocean_floor_wg().heights(),
            chunk
                .heightmap(HeightmapKind::OceanFloor)
                .unwrap()
                .heights(),
            "{settings_name}: OCEAN_FLOOR_WG != primed OCEAN_FLOOR"
        );
        assert_eq!(
            chunk.world_surface_wg().heights(),
            chunk
                .heightmap(HeightmapKind::WorldSurface)
                .unwrap()
                .heights(),
            "{settings_name}: WORLD_SURFACE_WG != primed WORLD_SURFACE"
        );

        // Every post-processing mark must point to a fluid block inside the chunk.
        for mark in chunk.post_processing_marks() {
            assert!(mark.section >= 0 && (mark.section as usize) < chunk.section_count());
            let (lx, ly, lz) = zenith_worldgen::unpack_offset_coordinates(mark.packed);
            let block_y = chunk.min_y() + mark.section * 16 + ly;
            let state = chunk.get_block(lx as usize, block_y, lz as usize);
            assert!(
                generator.predicates().has_fluid(state),
                "{settings_name}: post-processing mark at ({lx},{block_y},{lz}) is not a fluid"
            );
        }
    }
}

#[test]
fn generate_vanilla_terrain_chunks_all_dimensions() {
    let installs = zenith_install::discover_installs(Some("26.3"));
    let Some(install) = installs.first() else {
        eprintln!("NOTE: no local 26.3 install found; skipping vanilla terrain step test");
        return;
    };
    let tables_path = zenith_data::version_dir("26.3").join("tables.bin");
    let Ok(tables) = zenith_data::Tables::load(&tables_path) else {
        eprintln!("NOTE: no cached tables.bin found; skipping vanilla terrain step test");
        return;
    };

    let blocks = BlockStateTable::from_tables(&tables).expect("block state table");
    let pack = ZipPackResources::open("vanilla", &install.client_jar, null_log());
    let data = WorldgenData::load(&pack).expect("worldgen data");
    let tags = BlockTags::load(&pack, &blocks).expect("block tags");

    let cases = [
        (
            "minecraft:overworld",
            "minecraft:overworld",
            BiomeSource::MultiNoise(MultiNoiseBiomeSource::from_preset(
                MultiNoisePreset::Overworld,
            )),
            -64,
            384,
        ),
        (
            "minecraft:nether",
            "minecraft:the_nether",
            BiomeSource::MultiNoise(MultiNoiseBiomeSource::from_preset(MultiNoisePreset::Nether)),
            0,
            256,
        ),
        (
            "minecraft:end",
            "minecraft:the_end",
            BiomeSource::TheEnd(TheEndBiomeSource::new()),
            0,
            256,
        ),
    ];

    for (settings_str, dim_type_str, bs, exp_min_y, exp_height) in cases {
        let sid = Identifier::parse(settings_str).unwrap();
        let did = Identifier::parse(dim_type_str).unwrap();
        let dh = data.dimension_height(&did).expect("dimension height");
        assert_eq!((dh.min_y, dh.height), (exp_min_y, exp_height));

        let generator = TerrainGenerator::new(&data, bs, &sid, 12345, &blocks, &tags)
            .expect("TerrainGenerator::new")
            .with_chunk_height(dh.min_y, dh.height);
        let chunk = generator.generate_chunk(200, 200);
        assert_eq!(chunk.min_y(), exp_min_y);
        assert_eq!(chunk.height(), exp_height);
        assert_eq!(chunk.biome_indices().len(), chunk.section_count() * 64);
        assert!(!chunk.biome_palette().is_empty());
        for kind in [
            HeightmapKind::OceanFloorWg,
            HeightmapKind::WorldSurfaceWg,
            HeightmapKind::OceanFloor,
            HeightmapKind::WorldSurface,
            HeightmapKind::MotionBlocking,
            HeightmapKind::MotionBlockingNoLeaves,
        ] {
            assert!(chunk.heightmap(kind).is_some(), "missing {kind:?}");
        }
    }
}
