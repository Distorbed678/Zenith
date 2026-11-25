//! Integration test loading all vanilla 26.3 worldgen registries (`worldgen/noise`,
//! `worldgen/density_function`, `worldgen/noise_settings`) from the user's own client jar.
//!
//! Skips cleanly (prints a note and passes) when no 26.3 install is present on the host.

use zenith_codec::log::null_log;
use zenith_codec::resources::ZipPackResources;
use zenith_worldgen::interval::contains;
use zenith_worldgen::{AquiferConfig, RandomState, RouterField, WorldgenData};

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
