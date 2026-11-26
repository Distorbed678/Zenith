//! Incremental updates must converge to the light of a fresh relight of the final blocks.

mod common;

use common::{lit, scene};
use zenith_light::{DenseBlocks, LightLayer, LightWorld};

fn rng(seed: u64) -> impl FnMut() -> u64 {
    let mut r = seed;
    move || {
        r ^= r << 13;
        r ^= r >> 7;
        r ^= r << 17;
        r
    }
}

fn compare(a: &LightWorld<DenseBlocks>, b: &LightWorld<DenseBlocks>, what: &str) {
    for x in -16..32 {
        for z in -16..32 {
            for y in 44..110 {
                for layer in [LightLayer::Block, LightLayer::Sky] {
                    assert_eq!(
                        a.light_value(layer, x, y, z),
                        b.light_value(layer, x, y, z),
                        "{what}: {layer:?} at ({x},{y},{z})"
                    );
                }
            }
        }
    }
}

fn run(seed: u64, with_floor_holes: bool) {
    let s = scene();
    let mut w = lit(&s, Some(63), |_, _| {});
    let pool = [
        s.air,
        s.air,
        s.stone,
        s.glass,
        s.torch,
        s.glowstone,
        s.slab_bottom,
        s.slab_top,
        s.leaves,
        s.water,
    ];
    let mut next = rng(seed);
    let mut final_edits: Vec<(i32, i32, i32, u32)> = Vec::new();
    for round in 0..14 {
        let batch = 1 + (next() % 5) as usize;
        for _ in 0..batch {
            let x = (next() % 16) as i32;
            let z = (next() % 16) as i32;
            let mut y = 60 + (next() % 20) as i32;
            if with_floor_holes && next().is_multiple_of(4) {
                y = 40 + (next() % 24) as i32;
            }
            let st = pool[(next() % pool.len() as u64) as usize];
            w.set_block(x, y, z, st);
            final_edits.push((x, y, z, st));
        }
        if round % 3 != 2 {
            w.flush();
        }
    }
    w.flush();
    // fresh world with the same final blocks
    let fresh = lit(&s, Some(63), |b, _| {
        use zenith_light::ChunkBlocks;
        for &(x, y, z, st) in &final_edits {
            b.set_state(x as usize, y, z as usize, st);
        }
    });
    compare(&w, &fresh, &format!("seed {seed}"));
}

#[test]
fn random_edits_converge_to_a_fresh_relight() {
    for seed in [1u64, 2, 3, 0xDEAD_BEEF, 77] {
        run(seed, false);
    }
}

#[test]
fn random_edits_with_holes_in_the_floor_converge() {
    for seed in [11u64, 12, 13] {
        run(seed, true);
    }
}

/// Longer soak (run with `--ignored`).
#[test]
#[ignore = "slow soak"]
fn soak_random_edits() {
    for seed in 100..160u64 {
        run(seed, seed % 2 == 0);
    }
}
