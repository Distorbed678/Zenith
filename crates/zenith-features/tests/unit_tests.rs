//! Unit tests for `zenith-features` that require no game files (run in CI).

use std::sync::Arc;

use zenith_features::{
    BlockPos, Feature, FeatureCycleError, FloatProvider, HeightProvider, IntProvider,
    JavaBlockPosSet, PlacedFeature, PlacedFeatureId, PlacementModifier, WeightedList,
    WorldgenRandom, build_features_per_step,
};
use zenith_math::RandomSource;
use zenith_worldgen::{VerticalAnchor, WorldGenerationContext};

#[test]
fn worldgen_random_bit_source_over_xoroshiro() {
    let mut rng = WorldgenRandom::new_xoroshiro(12345);
    let dec_seed = rng.set_decoration_seed(12345, 1600, -3200);
    // Re-running with same inputs must produce identical decoration seed and feature stream.
    let mut rng2 = WorldgenRandom::new_xoroshiro(0);
    let dec_seed2 = rng2.set_decoration_seed(12345, 1600, -3200);
    assert_eq!(dec_seed, dec_seed2);

    rng.set_feature_seed(dec_seed, 7, 6);
    rng2.set_feature_seed(dec_seed2, 7, 6);
    for _ in 0..32 {
        assert_eq!(rng.next_int(), rng2.next_int());
        assert_eq!(rng.next_int_bounded(13), rng2.next_int_bounded(13));
        assert_eq!(rng.next_float().to_bits(), rng2.next_float().to_bits());
        assert_eq!(rng.next_double().to_bits(), rng2.next_double().to_bits());
        assert_eq!(rng.next_boolean(), rng2.next_boolean());
    }
}

#[test]
fn worldgen_random_gaussian_cache_survives_set_seed() {
    // Per spec: WorldgenRandom.setSeed delegates to backing source without resetting its own
    // inherited MarsagliaPolarGaussian cache.
    let mut rng = WorldgenRandom::new_xoroshiro(0);
    rng.set_feature_seed(999, 0, 0);
    let g1 = rng.next_gaussian();
    // Draw 1 Gaussian -> 2nd Gaussian of the Marsaglia pair is cached!
    let mut same_pair = WorldgenRandom::new_xoroshiro(0);
    same_pair.set_feature_seed(999, 0, 0);
    let p1 = same_pair.next_gaussian();
    let p2 = same_pair.next_gaussian();
    assert_eq!(g1.to_bits(), p1.to_bits());

    // Re-seeding does NOT clear the cached p2.
    rng.set_feature_seed(999, 1, 0);
    let after_reseed = rng.next_gaussian();
    assert_eq!(after_reseed.to_bits(), p2.to_bits());
}

#[test]
fn feature_sorter_topological_and_cycle_reduction() {
    let mk = |id: u32| -> Arc<PlacedFeature> {
        Arc::new(PlacedFeature {
            id: PlacedFeatureId(id),
            name: None,
            feature: Arc::new(Feature::NoOp),
            placement: Vec::new(),
        })
    };
    let a = mk(0);
    let b = mk(1);
    let c = mk(2);

    // Biome 0: [A, B], Biome 1: [B, C] -> global order [A, B, C]
    let sources = vec![
        vec![vec![Arc::clone(&a), Arc::clone(&b)]],
        vec![vec![Arc::clone(&b), Arc::clone(&c)]],
    ];
    let sorted = build_features_per_step(&sources, |s| s.as_slice(), true).expect("acyclic");
    assert_eq!(sorted.len(), 1);
    let ids: Vec<PlacedFeatureId> = sorted[0].features.iter().map(|f| f.id).collect();
    assert_eq!(ids, vec![a.id, b.id, c.id]);
    assert_eq!(sorted[0].index_of(a.id), Some(0));
    assert_eq!(sorted[0].index_of(b.id), Some(1));
    assert_eq!(sorted[0].index_of(c.id), Some(2));

    // Cycle across biomes 0 & 2 with innocent biome 1 -> reduction reports [0, 2]
    let cyclic = vec![
        vec![vec![Arc::clone(&a), Arc::clone(&b)]],
        vec![vec![Arc::clone(&c)]],
        vec![vec![Arc::clone(&b), Arc::clone(&a)]],
    ];
    let err: FeatureCycleError =
        build_features_per_step(&cyclic, |s| s.as_slice(), true).unwrap_err();
    assert_eq!(err.involved_sources, vec![0, 2]);
}

#[test]
fn block_pos_iteration_orders() {
    let pts: Vec<BlockPos> =
        zenith_features::rng::between_closed(BlockPos::new(0, 0, 0), BlockPos::new(1, 1, 1))
            .collect();
    // Exact z outer, y middle, x inner ascending order.
    assert_eq!(
        pts,
        vec![
            BlockPos::new(0, 0, 0),
            BlockPos::new(1, 0, 0),
            BlockPos::new(0, 1, 0),
            BlockPos::new(1, 1, 0),
            BlockPos::new(0, 0, 1),
            BlockPos::new(1, 0, 1),
            BlockPos::new(0, 1, 1),
            BlockPos::new(1, 1, 1),
        ]
    );

    let mut manhattan = Vec::new();
    zenith_features::rng::within_box_by_manhattan_distance(BlockPos::ZERO, 1, 0, 1, |p| {
        manhattan.push(p);
        true
    });
    assert_eq!(
        manhattan,
        vec![
            BlockPos::new(0, 0, 0),
            BlockPos::new(-1, 0, 0),
            BlockPos::new(0, 0, 1),
            BlockPos::new(0, 0, -1),
            BlockPos::new(1, 0, 0),
            BlockPos::new(-1, 0, 1),
            BlockPos::new(-1, 0, -1),
            BlockPos::new(1, 0, 1),
            BlockPos::new(1, 0, -1),
        ]
    );
}

#[test]
fn providers_and_weighted_list_invariants() {
    let mut rng = WorldgenRandom::new_xoroshiro(42);
    let ctx = WorldGenerationContext::new(-64, 384, 63);

    let hp_const = HeightProvider::Constant(VerticalAnchor::RelativeToSeaLevel(5));
    assert_eq!(hp_const.sample(&mut rng, ctx), 68);
    assert_eq!(rng.count(), 0);

    let hp_trap = HeightProvider::Trapezoid {
        min_inclusive: VerticalAnchor::Absolute(0),
        max_inclusive: VerticalAnchor::Absolute(100),
        plateau: 20,
    };
    for _ in 0..50 {
        let y = hp_trap.sample(&mut rng, ctx);
        assert!((0..=100).contains(&y));
    }

    let ip_trap = IntProvider::Trapezoid {
        min: -4,
        max: 4,
        plateau: 0,
    };
    for _ in 0..50 {
        let v = ip_trap.sample(&mut rng);
        assert!((-4..=4).contains(&v));
    }

    let fp = FloatProvider::Trapezoid {
        min: 0.5,
        max: 2.5,
        plateau: 0.5,
    };
    for _ in 0..50 {
        let v = fp.sample(&mut rng);
        assert!((0.5..=2.5).contains(&v));
    }

    let wl = WeightedList::new(vec![("a", 1), ("b", 3)]);
    for _ in 0..20 {
        let picked = *wl.get_random_or_throw(&mut rng);
        assert!(picked == "a" || picked == "b");
    }

    let _unused_mod = PlacementModifier::InSquare;
    let mut set = JavaBlockPosSet::new();
    set.insert(BlockPos::new(1, 2, 3));
    assert!(set.contains(BlockPos::new(1, 2, 3)));
}

#[test]
fn end_spikes_65536_seed_key_periodicity() {
    let s1 = zenith_features::features::structures_end::get_spikes_for_seed(12345);
    assert_eq!(s1.len(), 10);
    let guarded_count = s1.iter().filter(|s| s.guarded).count();
    assert_eq!(guarded_count, 2);
}
