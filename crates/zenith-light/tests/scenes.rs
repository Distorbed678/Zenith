//! Hand-built scenes with values derived from `docs/specs/lighting/` (no game files needed).

mod common;

use common::{LEVEL, lit, scene, terrain};
use zenith_light::{DenseBlocks, LightLayer, LightWorld, QueueKind, TraceEntry};

fn block(w: &LightWorld<DenseBlocks>, x: i32, y: i32, z: i32) -> u8 {
    w.light_value(LightLayer::Block, x, y, z)
}

fn sky(w: &LightWorld<DenseBlocks>, x: i32, y: i32, z: i32) -> u8 {
    w.light_value(LightLayer::Sky, x, y, z)
}

#[test]
fn facts_match_the_spec_assumptions() {
    let s = scene();
    let f = &s.facts;
    assert_eq!(f.emission(s.torch), 14);
    assert_eq!(f.emission(s.glowstone), 15);
    assert!(f.empty_shape(s.torch), "a torch cannot occlude");
    assert!(
        f.empty_shape(s.glowstone),
        "glowstone is a full cube without light shape"
    );
    assert_eq!(f.dampening(s.stone), 15);
    assert_eq!(f.dampening(s.air), 0);
    assert_eq!(f.dampening(s.glass), 0);
    assert!(
        !f.empty_shape(s.slab_bottom),
        "slabs use their shape for light"
    );
    assert_eq!(f.bedrock_state(), s.bedrock);
    let _ = &s.table;
}

#[test]
fn torch_in_a_cave_lights_the_cave_by_manhattan_distance() {
    let s = scene();
    // stone up to y=63, cave 5..=11 x 7..=13 x 5..=11 hollowed out, torch in the middle
    let w = lit(&s, Some(63), |b, s| {
        b.fill((5, 11), (7, 13), (5, 11), s.air);
        b.fill((8, 8), (10, 10), (8, 8), s.torch);
    });
    let (tx, ty, tz): (i32, i32, i32) = (8, 10, 8);
    for x in 5..=11 {
        for y in 7..=13 {
            for z in 5..=11 {
                let d = (x - tx).abs() + (y - ty).abs() + (z - tz).abs();
                let want = (14 - d).max(0) as u8;
                assert_eq!(block(&w, x, y, z), want, "block light at ({x},{y},{z})");
                // the cave is sealed from the sky
                assert_eq!(sky(&w, x, y, z), 0, "sky light at ({x},{y},{z})");
            }
        }
    }
    // stone around the cave stays dark
    assert_eq!(block(&w, 4, 10, 8), 0);
    assert_eq!(block(&w, 12, 10, 8), 0);
    // open sky above the floor is full bright, the floor itself dark
    assert_eq!(sky(&w, 8, 64, 8), 15);
    assert_eq!(sky(&w, 8, 200, 8), 15);
    assert_eq!(sky(&w, 8, 63, 8), 0);
    assert_eq!(w.raw_brightness(8, 10, 8, 0), 14);
    assert_eq!(w.raw_brightness(8, 100, 8, 5), 10);
}

#[test]
fn sky_column_under_a_bottom_slab_is_fed_from_the_sides() {
    let s = scene();
    let w = lit(&s, Some(63), |b, s| {
        b.fill((8, 8), (70, 70), (8, 8), s.slab_bottom);
    });
    // the slab cell is itself a source (the column's lowest source is the slab's Y)
    let chunk = w.chunks().get(0, 0).unwrap();
    assert_eq!(chunk.sky_sources.lowest_source_y(8, 8), 70);
    assert_eq!(chunk.sky_sources.lowest_source_y(7, 8), 64);
    assert_eq!(sky(&w, 8, 71, 8), 15);
    assert_eq!(sky(&w, 8, 70, 8), 15);
    // below it: one step from the open columns next to it
    for y in 64..=69 {
        assert_eq!(sky(&w, 8, y, 8), 14, "under the slab at y={y}");
        assert_eq!(sky(&w, 9, y, 8), 15);
    }
    // two cells in diagonally: 13 only where another side feeds it; a diagonal neighbour is one step further
    assert_eq!(sky(&w, 7, 66, 7), 15);
}

#[test]
fn placing_and_removing_a_torch_in_the_open_matches_the_flood_distance() {
    let s = scene();
    let mut w = lit(&s, Some(63), |_, _| {});
    assert_eq!(block(&w, 8, 70, 8), 0);
    assert_eq!(w.set_block(8, 70, 8, s.torch), Some(s.air));
    w.flush();
    assert_eq!(block(&w, 8, 70, 8), 14);
    assert_eq!(block(&w, 8, 71, 8), 13);
    assert_eq!(block(&w, 11, 70, 8), 11);
    // light passes chunk borders: x = 8 -> -1 is 9 steps
    assert_eq!(block(&w, -1, 70, 8), 5);
    // solid floor stops it
    assert_eq!(block(&w, 8, 63, 8), 0);
    assert_eq!(block(&w, 8, 64, 8), 8);
    // removal
    assert_eq!(w.set_block(8, 70, 8, s.air), Some(s.torch));
    w.flush();
    for (x, y, z) in [(8, 70, 8), (8, 71, 8), (11, 70, 8), (-1, 70, 8), (8, 64, 8)] {
        assert_eq!(block(&w, x, y, z), 0, "after removal ({x},{y},{z})");
    }
}

#[test]
fn a_sealed_torch_costs_one_decrease_and_one_increase_and_the_sky_engine_a_pull_in() {
    let s = scene();
    let mut w = lit(&s, Some(63), |_, _| {});
    w.engine_mut()
        .engine_mut(LightLayer::Block)
        .unwrap()
        .start_trace();
    assert_eq!(w.set_block(8, 10, 8, s.torch), Some(s.stone));
    let n = w.run_update();
    let node = zenith_light::pos::block_node(8, 10, 8);
    let trace = w
        .engine_mut()
        .engine_mut(LightLayer::Block)
        .unwrap()
        .take_trace();
    assert_eq!(
        trace,
        vec![
            TraceEntry {
                kind: QueueKind::Decrease,
                node,
                data: 1009
            },
            TraceEntry {
                kind: QueueKind::Increase,
                node,
                data: 4094
            },
        ]
    );
    // block engine: 1 decrease + 1 increase; sky engine: 1 pull-in decrease
    assert_eq!(n, 3);
    w.flush();
    assert_eq!(block(&w, 8, 10, 8), 14);
    assert_eq!(block(&w, 8, 11, 8), 0);
}

#[test]
fn light_crosses_section_boundaries() {
    let s = scene();
    let mut w = lit(&s, Some(63), |_, _| {});
    // y=79 is the top cell of section 4 (64..=79)
    w.set_block(8, 79, 8, s.glowstone);
    w.flush();
    assert_eq!(block(&w, 8, 79, 8), 15);
    assert_eq!(block(&w, 8, 80, 8), 14);
    assert_eq!(block(&w, 8, 81, 8), 13);
    assert_eq!(block(&w, 8, 78, 8), 14);
    // both sections hold light layers whose nibbles match
    let (b4, _) = w.section_layers(0, 4, 0);
    let (b5, _) = w.section_layers(0, 5, 0);
    let (b4, b5) = (b4.expect("section 4 layer"), b5.expect("section 5 layer"));
    assert_eq!(b4.get(8, 15, 8), 15);
    assert_eq!(b5.get(8, 0, 8), 14);
}

#[test]
fn removing_a_light_dims_only_what_it_fed() {
    let s = scene();
    let mut w = lit(&s, Some(63), |_, _| {});
    w.set_block(4, 70, 8, s.glowstone);
    w.set_block(12, 70, 8, s.glowstone);
    w.flush();
    assert_eq!(block(&w, 8, 70, 8), 11);
    w.set_block(4, 70, 8, s.air);
    w.flush();
    // now only the right glowstone is left: distance 4 from it
    assert_eq!(block(&w, 8, 70, 8), 11);
    assert_eq!(block(&w, 6, 70, 8), 9);
    assert_eq!(block(&w, 4, 70, 8), 7);
    assert_eq!(block(&w, 1, 70, 8), 4);
    assert_eq!(block(&w, -3, 70, 8), 0);
}

#[test]
fn section_states_and_layers_follow_the_data_and_neighbour_rule() {
    let s = scene();
    let w = lit(&s, Some(63), |_, _| {});
    let e = w.engine();
    // sections with stone: -4..=3 have data (state 0); 4 is air but touches data (state 1)
    let node = |sy| zenith_light::pos::section_node(0, sy, 0);
    for sy in -4..=3 {
        assert_eq!(
            e.debug_section_type(LightLayer::Block, node(sy)),
            0,
            "section {sy}"
        );
    }
    assert_eq!(e.debug_section_type(LightLayer::Block, node(4)), 1);
    assert_eq!(
        e.debug_section_type(LightLayer::Block, node(-5)),
        1,
        "padding below the world"
    );
    assert_eq!(e.debug_section_type(LightLayer::Block, node(5)), 2);
    assert!(w.layer(LightLayer::Sky, 0, 5, 0).is_none());
    assert!(w.layer(LightLayer::Sky, 0, 4, 0).is_some());
    // a section without a layer reads 15 for sky (open air above the top layer) and 0 for block
    assert_eq!(sky(&w, 3, 150, 3), 15);
    assert_eq!(block(&w, 3, 150, 3), 0);
}

#[test]
fn an_empty_section_above_the_top_is_all_sky() {
    let s = scene();
    // a single stone block in the top world section (y=319): sections 18..=20 get layers
    let w = lit(&s, None, |b, s| {
        b.fill((3, 3), (319, 319), (3, 3), s.stone);
    });
    let top_pad = w
        .layer(LightLayer::Sky, 0, 20, 0)
        .expect("padding section above the world");
    for (x, y, z) in [(0, 0, 0), (3, 7, 3), (15, 15, 15)] {
        assert_eq!(top_pad.get(x, y, z), 15);
    }
    assert!(!top_pad.is_empty());
    // the stone block shades the cell right below it only through its neighbours
    assert_eq!(sky(&w, 3, 320, 3), 15);
    assert_eq!(sky(&w, 3, 318, 3), 14);
    // columns without the block are sources all the way down; the shaded column is fed from its neighbours
    assert_eq!(sky(&w, 5, 100, 5), 15);
    assert_eq!(sky(&w, 5, -64, 5), 15);
    assert_eq!(sky(&w, 3, 100, 3), 14);
    let chunk = w.chunks().get(0, 0).unwrap();
    assert_eq!(chunk.sky_sources.lowest_source_y(3, 3), 320);
    assert_eq!(chunk.sky_sources.lowest_source_y(4, 3), i32::MIN);
    assert_eq!(chunk.sky_sources.highest_lowest_source_y(), 320);
}

#[test]
fn glass_does_not_stop_sky_light() {
    let s = scene();
    let w = lit(&s, Some(63), |b, s| {
        b.fill((0, 15), (90, 90), (0, 15), s.glass);
    });
    let chunk = w.chunks().get(0, 0).unwrap();
    assert_eq!(chunk.sky_sources.lowest_source_y(4, 4), 64);
    assert_eq!(sky(&w, 4, 80, 4), 15);
    assert_eq!(sky(&w, 4, 200, 4), 15);
}

#[test]
fn save_keeps_non_empty_layers_and_load_adopts_them_without_reseeding() {
    let s = scene();
    let mut w = lit(&s, Some(63), |b, s| {
        b.fill((5, 11), (7, 13), (5, 11), s.air);
        b.fill((8, 8), (10, 10), (8, 8), s.torch);
    });
    let save = w.save_chunk_light(0, 0).unwrap();
    assert!(save.light_correct);
    let block_secs: Vec<i32> = save
        .sections
        .iter()
        .filter(|s| s.block_light.is_some())
        .map(|s| s.y)
        .collect();
    assert!(
        block_secs.contains(&0),
        "the cave section carries block light: {block_secs:?}"
    );
    let sec0 = save.sections.iter().find(|s| s.y == 0).unwrap();
    let bytes = sec0.block_light.as_ref().unwrap();
    assert_eq!(bytes.len(), 2048);
    // (8,10,8): local y = 10 (section 0 covers 0..16), index (y<<8)|(z<<4)|x = 2688+128+8
    let idx = (10 << 8) | (8 << 4) | 8;
    assert_eq!(bytes[idx >> 1] >> if idx & 1 == 1 { 4 } else { 0 } & 15, 14);

    // rebuild an identical world from the saved light: lighted chunks keep the stored arrays
    let mut w2 = LightWorld::new(s.facts.clone(), LEVEL, true);
    let mut chunks = terrain(&s, Some(63), |b, s| {
        b.fill((5, 11), (7, 13), (5, 11), s.air);
        b.fill((8, 8), (10, 10), (8, 8), s.torch);
    });
    for c in &mut chunks {
        if (c.cx, c.cz) == (0, 0) {
            c.status = zenith_light::LightStatus::Full;
            c.light_correct = true;
        }
    }
    for c in chunks {
        w2.add_chunk(c);
    }
    w2.load_chunk_light(0, 0, &save.sections).unwrap();
    w2.flush();
    // queued data is what a save or a packet would see right after the load
    assert!(w2.layer(LightLayer::Block, 0, 0, 0).is_some());
    for (cx, cz) in [
        (0, 0),
        (-1, -1),
        (0, -1),
        (1, -1),
        (-1, 0),
        (1, 0),
        (-1, 1),
        (0, 1),
        (1, 1),
    ] {
        w2.initialize_light(cx, cz);
    }
    w2.light(0, 0);
    assert_eq!(block(&w2, 8, 10, 8), 14);
    assert_eq!(block(&w2, 8, 10, 11), 11);
    assert!(w2.chunks().get(0, 0).unwrap().light_correct);
}

#[test]
fn light_packet_data_lists_non_empty_and_empty_layers() {
    let s = scene();
    let mut w = lit(&s, Some(63), |_, _| {});
    w.set_block(8, 70, 8, s.torch);
    let p = w.chunk_light_packet(0, 0);
    // light section index = section - (-5); section 4 (index 9) has block data from the torch
    assert!(p.block_y_mask.get(9));
    assert!(!p.block_y_mask.get(0));
    let round = zenith_light::LightUpdatePacketData::decode(&p.encode()).unwrap();
    assert_eq!(round, p);
    // sky: the sections with a layer that is not empty carry data, every array is 2048 bytes
    assert!(p.sky_updates.iter().all(|a| a.len() == 2048));
    assert_eq!(p.sky_updates.len(), p.sky_y_mask.iter().count());
}

#[test]
fn block_changes_report_light_updates_for_broadcast() {
    let s = scene();
    let mut w = lit(&s, Some(63), |_, _| {});
    w.promote_to_full(0, 0);
    w.set_block(8, 70, 8, s.torch);
    let packets = w.take_light_update_packets();
    assert!(
        packets
            .iter()
            .any(|(pos, d)| *pos == (0, 0) && !d.block_y_mask.is_empty())
    );
    assert!(
        w.take_light_update_packets().is_empty(),
        "filters are cleared by a broadcast"
    );
}

#[test]
fn unloading_a_chunk_removes_layers_nobody_else_needs() {
    let s = scene();
    let mut w = lit(&s, Some(63), |_, _| {});
    let c = w.unload_chunk(0, 0).unwrap();
    assert_eq!((c.cx, c.cz), (0, 0));
    // the neighbours still have data, so the border sections of the unloaded chunk keep their layers
    assert!(w.layer(LightLayer::Block, 0, 0, 0).is_some());
    assert!(w.chunks().get(0, 0).is_none());
    // light of unregistered chunk reads as unlit block light
    for (cx, cz) in [
        (-1, -1),
        (0, -1),
        (1, -1),
        (-1, 0),
        (1, 0),
        (-1, 1),
        (0, 1),
        (1, 1),
    ] {
        w.unload_chunk(cx, cz).unwrap();
    }
    assert!(w.layer(LightLayer::Block, 0, 0, 0).is_none());
}
