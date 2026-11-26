//! Loads all 26.3 `worldgen/block_state_provider`, `worldgen/feature`, `worldgen/placed_feature`,
//! and `worldgen/biome` entries from the user's installed jar (skips cleanly if no jar is present).

use std::sync::Arc;

use zenith_biome::{BiomeSource, MultiNoiseBiomeSource, MultiNoisePreset, TheEndBiomeSource};
use zenith_blocks::{BlockFactTags, BlockFacts};
use zenith_codec::log::null_log;
use zenith_codec::resources::ZipPackResources;
use zenith_data::Tables;
use zenith_features::{BlockOps, FeatureData, FeatureDecorator};
use zenith_registry::{BlockStateTable, Identifier};
use zenith_worldgen::{BlockPredicates, BlockTags};

#[test]
fn load_all_26_3_feature_registries_and_sort() {
    let Some(install) = zenith_install::discover_installs(Some("26.3"))
        .into_iter()
        .next()
    else {
        return;
    };
    let tables_path = zenith_install::cache_dir().join("versions/26.3/tables.bin");
    if !tables_path.is_file() {
        return;
    }
    let tables = Tables::load(&tables_path).expect("read tables.bin");
    let blocks = Arc::new(BlockStateTable::from_tables(&tables).expect("block state table"));
    let pack = ZipPackResources::open("vanilla", &install.client_jar, null_log());
    let tags = Arc::new(BlockTags::load(&pack, &blocks).expect("load block tags"));
    let fact_tags = BlockFactTags::from_resolver(&blocks, |id| {
        let ident = Identifier::parse(id).ok()?;
        tags.block_ids(&ident)
    });
    let facts = Arc::new(BlockFacts::build(&blocks, &fact_tags).expect("build block facts"));
    let predicates = Arc::new(BlockPredicates::new(&blocks, &tags).expect("predicates"));
    let ops = Arc::new(BlockOps::new(blocks, facts, tags, predicates));

    let data = Arc::new(FeatureData::load(&pack, &ops).expect("load all 26.3 feature registries"));
    assert!(!data.features.is_empty());
    assert!(!data.placed_features.is_empty());
    assert!(!data.biomes.is_empty());
    // Every `minecraft:tree` / `fallen_tree` configuration (all trunk, foliage, root and decorator
    // types) must decode.
    let trees = data
        .features
        .values()
        .filter(|f| matches!(f.as_ref(), zenith_features::Feature::Tree { .. }))
        .count();
    assert!(
        trees >= 40,
        "expected the vanilla tree configurations, got {trees}"
    );
    assert!(
        data.features
            .values()
            .any(|f| matches!(f.as_ref(), zenith_features::Feature::FallenTree { .. }))
    );

    // FeatureSorter must succeed with zero cycles across Overworld, Nether, and End!
    let ow = BiomeSource::MultiNoise(MultiNoiseBiomeSource::from_preset(
        MultiNoisePreset::Overworld,
    ));
    let nether =
        BiomeSource::MultiNoise(MultiNoiseBiomeSource::from_preset(MultiNoisePreset::Nether));
    let end = BiomeSource::TheEnd(TheEndBiomeSource::new());

    let ow_dec = FeatureDecorator::new(
        Arc::clone(&data),
        Arc::clone(&ops),
        &ow,
        12345,
        -64,
        384,
        63,
        false,
    )
    .expect("Overworld FeatureSorter must succeed");
    let nether_dec = FeatureDecorator::new(
        Arc::clone(&data),
        Arc::clone(&ops),
        &nether,
        12345,
        0,
        128,
        32,
        true,
    )
    .expect("Nether FeatureSorter must succeed");
    let end_dec = FeatureDecorator::new(data, ops, &end, 12345, 0, 128, 0, false)
        .expect("End FeatureSorter must succeed");

    eprintln!(
        "Loaded: {} BSPs, {} features, {} placed_features, {} biomes | Sorted steps: OW={}, Nether={}, End={}",
        ow_dec
            .features_per_step()
            .iter()
            .map(|s| s.features.len())
            .sum::<usize>(),
        nether_dec
            .features_per_step()
            .iter()
            .map(|s| s.features.len())
            .sum::<usize>(),
        end_dec
            .features_per_step()
            .iter()
            .map(|s| s.features.len())
            .sum::<usize>(),
        ow_dec.features_per_step().len(),
        ow_dec.features_per_step().len(),
        nether_dec.features_per_step().len(),
        end_dec.features_per_step().len(),
    );
}
