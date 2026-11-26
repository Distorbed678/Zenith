//! Client engines, batching, brightness helpers, terrain-chunk plug-in and shape helpers.

mod common;

use common::{HEIGHT, LEVEL, MIN_Y, lit, scene};
use zenith_blocks::{Direction, VoxelShape};
use zenith_light::{
    BATCH_SIZE, ClientLightWorld, DenseBlocks, LightLayer, LightStatus, LightWorld, LitChunk,
    can_see_sky, effective_sky_brightness, light_level_dependent_magic_value,
    max_local_raw_brightness, merged_face_occludes, sky_darken_from_level,
};

#[test]
fn a_thousand_pending_tasks_force_a_batch() {
    let s = scene();
    let mut w = lit(&s, Some(63), |_, _| {});
    // make section 4 (y 64..=79) non-empty first, so later placements submit exactly one check task each
    w.set_block(0, 79, 0, s.stone);
    w.flush();
    let before = w.batches_run();
    for i in 0..(BATCH_SIZE - 1) {
        let (x, y, z) = (
            (i % 16) as i32,
            64 + (i / 16 % 14) as i32,
            (i / 224) as i32 + 1,
        );
        w.set_block(x, y, z, s.torch);
    }
    assert_eq!(w.pending_tasks(), BATCH_SIZE - 1);
    assert_eq!(w.batches_run(), before);
    w.set_block(15, 78, 15, s.torch);
    assert_eq!(
        w.batches_run(),
        before + 1,
        "the 1000th task runs a batch at once"
    );
    assert_eq!(w.pending_tasks(), 0);
    w.flush();
    assert_eq!(w.light_value(LightLayer::Block, 15, 78, 15), 14);
}

#[test]
fn the_client_reproduces_the_server_light_from_a_packet() {
    let s = scene();
    let mut server = lit(&s, Some(63), |b, s| {
        use zenith_light::ChunkBlocks;
        b.set_state(8, 70, 8, s.torch);
        b.set_state(2, 66, 3, s.glowstone);
    });
    let packet = server.chunk_light_packet(0, 0);
    let mut blocks = DenseBlocks::new(MIN_Y, HEIGHT, s.air);
    {
        use zenith_light::ChunkBlocks;
        blocks.fill((0, 15), (MIN_Y, 63), (0, 15), s.stone);
        blocks.set_state(8, 70, 8, s.torch);
        blocks.set_state(2, 66, 3, s.glowstone);
    }
    let mut client = ClientLightWorld::new(s.facts.clone(), LEVEL, true);
    let chunk = LitChunk::loaded(0, 0, blocks, LightStatus::Full, true);
    client.handle_chunk_with_light(Some(chunk), 0, 0, packet, true);
    assert_eq!(client.queued_tasks(), 1);
    client.settle();
    for x in 0..16 {
        for z in 0..16 {
            for y in 40..100 {
                for l in [LightLayer::Block, LightLayer::Sky] {
                    assert_eq!(
                        client.light_value(l, x, y, z),
                        server.light_value(l, x, y, z),
                        "{l:?} ({x},{y},{z})"
                    );
                }
            }
        }
    }
    // a predicted change: placing a torch lights the client on the next frame
    client.set_block(13, 100, 13, s.torch);
    assert_eq!(client.light_value(LightLayer::Block, 13, 100, 13), 0);
    client.run_frame();
    assert_eq!(client.light_value(LightLayer::Block, 13, 100, 13), 14);
    // forgetting the chunk drops the engine state through the queue
    client.handle_forget_chunk(0, 0);
    client.settle();
    assert!(client.chunks().get(0, 0).is_none());
}

#[test]
fn client_polling_runs_at_least_ten_closures() {
    let s = scene();
    let mut client: ClientLightWorld<DenseBlocks> =
        ClientLightWorld::new(s.facts.clone(), LEVEL, true);
    for i in 0..25 {
        client.handle_forget_chunk(i, 0);
    }
    client.poll_light_updates();
    assert_eq!(client.queued_tasks(), 15);
    client.poll_light_updates();
    assert_eq!(client.queued_tasks(), 5);
}

#[cfg(feature = "worldgen")]
#[test]
fn terrain_chunks_plug_in() {
    use zenith_worldgen::TerrainChunk;
    let s = scene();
    let mut chunks = Vec::new();
    for cz in -1..=1 {
        for cx in -1..=1 {
            let mut t = TerrainChunk::new(cx, cz, MIN_Y, HEIGHT, s.air);
            for y in MIN_Y..64 {
                for z in 0..16 {
                    for x in 0..16 {
                        t.set_block(x, y, z, s.stone);
                    }
                }
            }
            if (cx, cz) == (0, 0) {
                t.set_block(4, 70, 4, s.torch);
            }
            chunks.push(LitChunk::new(cx, cz, t));
        }
    }
    let mut w = LightWorld::new(s.facts.clone(), LEVEL, true);
    w.light_chunks(chunks);
    assert_eq!(w.light_value(LightLayer::Block, 4, 70, 4), 14);
    assert_eq!(w.light_value(LightLayer::Block, 4, 72, 4), 12);
    assert_eq!(w.light_value(LightLayer::Sky, 4, 64, 4), 15);
    assert!(w.chunks().get(0, 0).unwrap().is_lighted());
}

#[test]
fn a_world_without_sky_has_no_sky_engine_and_reads_zero() {
    let s = scene();
    let mut w = LightWorld::new(s.facts.clone(), LEVEL, false);
    w.light_chunks(common::terrain(&s, Some(63), |_, _| {}));
    assert!(w.engine().engine(LightLayer::Sky).is_none());
    assert_eq!(w.light_value(LightLayer::Sky, 4, 100, 4), 0);
    assert!(w.layer(LightLayer::Sky, 0, 0, 0).is_none());
    // raw brightness takes no sky dampen without the engine: max(block, 0)
    assert_eq!(w.raw_brightness(4, 100, 4, 3), 0);
}

#[test]
fn brightness_helpers_follow_the_spec_formulas() {
    let s = scene();
    let w = lit(&s, Some(63), |_, _| {});
    let e = w.engine();
    assert_eq!(e.raw_brightness(4, 100, 4, 4), 11);
    assert!(can_see_sky(e, 4, 100, 4));
    assert!(!can_see_sky(e, 4, 20, 4));
    assert_eq!(effective_sky_brightness(e, 4, 100, 4, 4), 11);
    assert_eq!(effective_sky_brightness(e, 4, 100, 4, 20), 0);
    assert_eq!(max_local_raw_brightness(e, 4, 100, 4, 0), 15);
    assert_eq!(max_local_raw_brightness(e, 30_000_000, 100, 4, 0), 15);
    assert_eq!(max_local_raw_brightness(e, -30_000_001, 20, 4, 0), 15);
    assert_eq!(sky_darken_from_level(15.0), 0);
    assert_eq!(sky_darken_from_level(4.5), 10);
    let v = light_level_dependent_magic_value(15, 0.0);
    assert!((v - 1.0).abs() < 1e-6);
    assert_eq!(light_level_dependent_magic_value(0, 0.1), 0.1);
}

#[test]
fn light_property_gate_and_dampening_helpers() {
    let s = scene();
    let f = &s.facts;
    assert!(!f.has_different_light_properties(s.stone, s.stone));
    assert!(f.has_different_light_properties(s.air, s.stone));
    assert!(f.has_different_light_properties(s.air, s.torch));
    assert!(
        f.has_different_light_properties(s.slab_bottom, s.slab_top),
        "light-shape blocks always count"
    );
    assert!(
        !f.has_different_light_properties(s.air, s.glass),
        "same dampening, emission, no shape"
    );
    assert_eq!(
        f.light_dampening_into(s.air, s.stone, Direction::Down, 0),
        0
    );
    // two bottom slabs facing each other along Y close the gap: 16
    // a bottom slab and a top slab above it leave the gap between their halves open
    assert_eq!(
        f.light_dampening_into(s.slab_bottom, s.slab_top, Direction::Up, 0),
        0
    );
    // a top slab under a bottom slab: the full top face meets the full bottom face
    assert_eq!(
        f.light_dampening_into(s.slab_top, s.slab_bottom, Direction::Up, 0),
        16
    );
    // a top slab above a bottom slab (going down): both discarded, nothing merges
    assert_eq!(
        f.light_dampening_into(s.slab_top, s.slab_bottom, Direction::Down, 3),
        3
    );
    // a bottom slab stepping up into another bottom slab: the upper one's full bottom face closes it
    assert_eq!(
        f.light_dampening_into(s.slab_bottom, s.slab_bottom, Direction::Up, 2),
        16
    );
}

#[test]
fn merged_face_occlusion_helper_matches_the_spec_wording() {
    let full = VoxelShape::block();
    let half_low = VoxelShape::px(0.0, 0.0, 0.0, 16.0, 8.0, 16.0);
    let half_high = VoxelShape::px(0.0, 8.0, 0.0, 16.0, 16.0, 16.0);
    let empty = VoxelShape::empty();
    assert!(merged_face_occludes(&full, &empty, Direction::Up));
    assert!(!merged_face_occludes(&empty, &empty, Direction::Up));
    // from = low slab (top face open), to = high slab above (bottom face open): the boundary layers are empty
    assert!(!merged_face_occludes(&half_low, &half_high, Direction::Up));
    // from = high slab, to above: from reaches 1 and its layer is the whole top face
    assert!(merged_face_occludes(&half_high, &empty, Direction::Up));
    // a post that does not reach the face is discarded
    let post = VoxelShape::px(6.0, 0.0, 6.0, 10.0, 8.0, 10.0);
    assert!(!merged_face_occludes(&post, &empty, Direction::Up));
}

#[test]
fn light_changes_mark_chunks_unsaved_and_unloaded_neighbours_are_bedrock() {
    let s = scene();
    let mut w = LightWorld::new(s.facts.clone(), LEVEL, true);
    // a lone chunk: the neighbours are missing and read as bedrock, so light cannot leave the chunk
    let mut chunks = common::terrain(&s, Some(63), |_, _| {});
    chunks.retain(|c| (c.cx, c.cz) == (0, 0));
    w.light_chunks(chunks);
    w.set_block(0, 70, 0, s.torch);
    w.flush();
    assert_eq!(w.light_value(LightLayer::Block, 0, 70, 0), 14);
    assert_eq!(w.light_value(LightLayer::Block, -1, 70, 0), 0);
    assert!(w.chunks().get(0, 0).unwrap().unsaved);
}
