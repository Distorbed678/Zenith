//! The per-column lowest-source table (`sky-light-sources.md`).

mod common;

use common::{HEIGHT, MIN_Y, scene};
use zenith_light::{ChunkBlocks, DenseBlocks, SkyLightSources};

fn table(blocks: &DenseBlocks, s: &common::Scene) -> SkyLightSources {
    let mut t = SkyLightSources::new(MIN_Y, HEIGHT);
    t.fill_from(blocks, &s.facts);
    t
}

#[test]
fn fresh_and_empty_tables_read_below_the_world() {
    let s = scene();
    let t = SkyLightSources::new(MIN_Y, HEIGHT);
    assert_eq!(t.lowest_source_y(3, 4), i32::MIN);
    assert_eq!(t.highest_lowest_source_y(), i32::MIN);
    let blocks = DenseBlocks::new(MIN_Y, HEIGHT, s.air);
    let t = table(&blocks, &s);
    assert_eq!(t.lowest_source_y(0, 0), i32::MIN);
}

#[test]
fn occluders_set_the_source_line_one_above_themselves_or_at_themselves() {
    let s = scene();
    let mut b = DenseBlocks::new(MIN_Y, HEIGHT, s.air);
    b.fill((0, 15), (MIN_Y, 63), (0, 15), s.stone);
    b.set_state(1, 70, 1, s.slab_bottom); // face shapes: source line at the slab itself
    b.set_state(2, 70, 2, s.slab_top); // top slab closes the top face: line above it
    b.set_state(3, 70, 3, s.slab_double);
    b.set_state(4, 80, 4, s.leaves); // dampening 1 ends the column too
    b.set_state(5, 80, 5, s.water);
    b.set_state(6, 80, 6, s.glass); // transparent: the floor stays the line
    b.set_state(7, 80, 7, s.torch);
    let t = table(&b, &s);
    assert_eq!(t.lowest_source_y(0, 0), 64);
    assert_eq!(t.lowest_source_y(1, 1), 70);
    assert_eq!(t.lowest_source_y(2, 2), 71);
    assert_eq!(t.lowest_source_y(3, 3), 71);
    assert_eq!(t.lowest_source_y(4, 4), 81);
    assert_eq!(t.lowest_source_y(5, 5), 81);
    assert_eq!(t.lowest_source_y(6, 6), 64);
    assert_eq!(t.lowest_source_y(7, 7), 64);
    assert_eq!(t.highest_lowest_source_y(), 81);
}

#[test]
fn sections_of_air_between_blocks_contribute_no_edge() {
    let s = scene();
    let mut b = DenseBlocks::new(MIN_Y, HEIGHT, s.air);
    b.set_state(0, 0, 0, s.stone);
    b.set_state(0, 200, 0, s.stone);
    let t = table(&b, &s);
    assert_eq!(t.lowest_source_y(0, 0), 201);
    assert_eq!(t.lowest_source_y(1, 0), i32::MIN);
    // the top block of the world
    b.set_state(1, 319, 1, s.stone);
    let t = table(&b, &s);
    assert_eq!(t.lowest_source_y(1, 1), 320);
}

#[test]
fn incremental_updates_agree_with_a_full_recompute_for_notified_changes() {
    let s = scene();
    let mut b = DenseBlocks::new(MIN_Y, HEIGHT, s.air);
    b.fill((0, 15), (MIN_Y, 63), (0, 15), s.stone);
    let mut t = table(&b, &s);
    let pool = [
        s.air,
        s.stone,
        s.glass,
        s.slab_bottom,
        s.slab_top,
        s.leaves,
        s.water,
        s.torch,
        s.slab_double,
    ];
    let mut r: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = || {
        r ^= r << 13;
        r ^= r >> 7;
        r ^= r << 17;
        r
    };
    for step in 0..4000 {
        let (x, z) = ((next() % 4) as usize, (next() % 4) as usize);
        let y = 40 + (next() % 60) as i32;
        let new = pool[(next() % pool.len() as u64) as usize];
        let old = b.set_state(x, y, z, new);
        if s.facts.has_different_light_properties(old, new) {
            t.update(&b, &s.facts, x, y, z);
        }
        if step % 25 == 0 {
            let fresh = table(&b, &s);
            for zz in 0..4 {
                for xx in 0..4 {
                    assert_eq!(
                        t.lowest_source_y(xx, zz),
                        fresh.lowest_source_y(xx, zz),
                        "step {step}: column ({xx},{zz})"
                    );
                }
            }
        }
    }
}

#[test]
fn update_reports_whether_the_table_changed() {
    let s = scene();
    let mut b = DenseBlocks::new(MIN_Y, HEIGHT, s.air);
    b.fill((0, 15), (MIN_Y, 63), (0, 15), s.stone);
    let mut t = table(&b, &s);
    // far below the line: nothing to do
    b.set_state(2, 10, 2, s.air);
    assert!(!t.update(&b, &s.facts, 2, 10, 2));
    // a block above the line raises it
    b.set_state(2, 90, 2, s.stone);
    assert!(t.update(&b, &s.facts, 2, 90, 2));
    assert_eq!(t.lowest_source_y(2, 2), 91);
    // removing it digs the line back down to the floor
    b.set_state(2, 90, 2, s.air);
    assert!(t.update(&b, &s.facts, 2, 90, 2));
    assert_eq!(t.lowest_source_y(2, 2), 64);
}
