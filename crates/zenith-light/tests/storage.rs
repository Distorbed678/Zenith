//! Layer storage rules of `data-layers.md` exercised through the public `LayerStorage` API.

use zenith_light::pos::{block_node, section_node, section_zero};
use zenith_light::{DataLayer, LayerStorage, LightLayer};

fn sec(y: i32) -> i64 {
    section_node(0, y, 0)
}

fn run_end(s: &mut LayerStorage) -> Vec<i64> {
    s.mark_new_inconsistencies();
    s.swap_section_map()
}

#[test]
fn section_status_creates_a_layer_for_the_section_and_its_26_neighbours() {
    let mut s = LayerStorage::new(LightLayer::Block);
    s.update_section_status(sec(3), false);
    assert!(s.has_inconsistencies());
    for dx in -1..=1 {
        for dy in -1..=1 {
            for dz in -1..=1 {
                let n = section_node(dx, 3 + dy, dz);
                assert!(s.storing_light_for_section(n), "({dx},{dy},{dz})");
                let want = if (dx, dy, dz) == (0, 0, 0) { 32 } else { 1 };
                assert_eq!(s.section_state(n), want);
            }
        }
    }
    assert!(!s.storing_light_for_section(sec(5)));
    assert_eq!(s.debug_section_type(sec(3)), 0);
    assert_eq!(s.debug_section_type(sec(4)), 1);
    assert_eq!(s.debug_section_type(sec(5)), 2);
    // each of the 27 new layers marks itself and its 26 neighbours: the union is the 5x5x5 box
    let reported = run_end(&mut s);
    assert_eq!(reported.len(), 125);
    assert!(reported.contains(&section_node(2, 5, -2)));
}

#[test]
fn removal_is_deferred_to_the_end_of_the_run_and_the_swap_hides_it_later() {
    let mut s = LayerStorage::new(LightLayer::Block);
    s.update_section_status(sec(3), false);
    run_end(&mut s);
    assert!(s.visible_layer(sec(3)).is_some());
    s.update_section_status(sec(3), true);
    // still readable in the run that follows the flip
    assert!(s.storing_light_for_section(sec(3)));
    assert!(s.has_inconsistencies());
    run_end(&mut s);
    assert!(!s.storing_light_for_section(sec(3)));
    assert!(s.visible_layer(sec(3)).is_none());
    assert_eq!(s.section_state(sec(3)), 0);
}

#[test]
fn a_removal_cancelled_by_a_re_add_keeps_the_old_layer_untouched() {
    let mut s = LayerStorage::new(LightLayer::Block);
    s.update_section_status(sec(3), false);
    run_end(&mut s);
    s.set_stored_level(block_node(5, 50, 5), 9);
    run_end(&mut s);
    s.update_section_status(sec(3), true);
    s.update_section_status(sec(3), false);
    run_end(&mut s);
    assert_eq!(s.stored_level(block_node(5, 50, 5)), 9);
    assert_eq!(s.visible_light(block_node(5, 50, 5)), 9);
}

#[test]
fn queued_data_is_adopted_by_the_new_section_and_mutated_in_place() {
    let mut s = LayerStorage::new(LightLayer::Block);
    let mut l = DataLayer::new();
    l.set(1, 2, 3, 12);
    s.queue_section_data(sec(3), Some(l));
    // right after the load the queued bytes are what a save would see
    assert_eq!(s.data_layer_data(sec(3)).unwrap().get(1, 2, 3), 12);
    s.update_section_status(sec(3), false);
    assert_eq!(s.stored_level(block_node(1, 50, 3)), 12);
    // same-run write goes in place; the visible snapshot is made after the run
    s.set_stored_level(block_node(2, 50, 3), 7);
    run_end(&mut s);
    assert_eq!(s.visible_light(block_node(2, 50, 3)), 7);
    assert_eq!(s.visible_light(block_node(1, 50, 3)), 12);
    assert!(!s.has_queued_data(sec(3)));
}

#[test]
fn queued_data_for_a_section_without_a_layer_stays_queued_until_dropped() {
    let mut s = LayerStorage::new(LightLayer::Block);
    s.queue_section_data(sec(9), Some(DataLayer::filled(4)));
    run_end(&mut s);
    assert!(s.has_queued_data(sec(9)));
    assert!(s.data_layer_data(sec(9)).is_some());
    s.queue_section_data(sec(9), None);
    assert!(s.data_layer_data(sec(9)).is_none());
}

#[test]
fn retain_flag_puts_removed_data_back_into_the_queue() {
    let mut s = LayerStorage::new(LightLayer::Block);
    s.update_section_status(sec(3), false);
    run_end(&mut s);
    s.set_stored_level(block_node(4, 50, 4), 6);
    run_end(&mut s);
    s.retain_data(section_zero(sec(3)), true);
    s.update_section_status(sec(3), true);
    run_end(&mut s);
    assert!(!s.storing_light_for_section(sec(3)));
    assert!(s.has_queued_data(sec(3)));
    assert_eq!(s.data_layer_data(sec(3)).unwrap().get(4, 2, 4), 6);
    // without retain the data is gone
    let mut s = LayerStorage::new(LightLayer::Block);
    s.update_section_status(sec(3), false);
    run_end(&mut s);
    s.update_section_status(sec(3), true);
    run_end(&mut s);
    assert!(!s.has_queued_data(sec(3)));
}

#[test]
fn a_neighbour_keeps_a_border_layer_alive_with_its_data() {
    let mut s = LayerStorage::new(LightLayer::Block);
    s.update_section_status(section_node(0, 3, 0), false);
    s.update_section_status(section_node(1, 3, 0), false);
    run_end(&mut s);
    s.set_stored_level(block_node(15, 50, 4), 5); // in chunk 0's section 3, next to chunk 1
    run_end(&mut s);
    s.update_section_status(section_node(0, 3, 0), true);
    run_end(&mut s);
    // section (0,3,0) is now only a neighbour of (1,3,0): it keeps layer and data
    assert!(s.storing_light_for_section(section_node(0, 3, 0)));
    assert_eq!(s.debug_section_type(section_node(0, 3, 0)), 1);
    assert_eq!(s.stored_level(block_node(15, 50, 4)), 5);
}

#[test]
fn copy_on_write_keeps_the_visible_snapshot_stable_during_a_run() {
    let mut s = LayerStorage::new(LightLayer::Block);
    s.update_section_status(sec(3), false);
    run_end(&mut s);
    let p = block_node(0, 50, 0);
    s.set_stored_level(p, 8);
    assert_eq!(s.stored_level(p), 8);
    assert_eq!(s.visible_light(p), 0, "the visible map lags until the swap");
    run_end(&mut s);
    assert_eq!(s.visible_light(p), 8);
}

#[test]
fn sky_top_sections_and_lowest_section_bookkeeping() {
    let mut s = LayerStorage::new(LightLayer::Sky);
    let z0 = section_zero(sec(0));
    assert_eq!(s.top_section_y(z0), i32::MAX);
    s.update_section_status(sec(5), false);
    run_end(&mut s);
    assert_eq!(s.bottom_section_y(), 4);
    assert_eq!(s.top_section_y(z0), 7);
    // a neighbouring column's top is its own
    assert_eq!(s.top_section_y(section_zero(section_node(1, 0, 0))), 7);
    s.update_section_status(sec(5), true);
    run_end(&mut s);
    // all layers gone: entries removed, the lowest section never rises
    assert_eq!(s.top_section_y(z0), 4);
    assert_eq!(s.bottom_section_y(), 4);
}

#[test]
fn sky_reads_without_layers_are_15_and_walk_up_across_gaps() {
    let mut s = LayerStorage::new(LightLayer::Sky);
    assert_eq!(s.visible_light(block_node(1, 70, 1)), 15);
    // layers at sections 2..=4 (data in 3); then take out section 3's neighbours' influence by looking above
    s.update_section_status(sec(3), false);
    run_end(&mut s);
    // above the top (section 5 and up) is 15
    assert_eq!(s.visible_light(block_node(1, 5 * 16 + 2, 1)), 15);
    // inside a layer: reads the nibble (empty layer, light not enabled -> 0)
    assert_eq!(s.visible_light(block_node(1, 3 * 16 + 2, 1)), 0);
    // far below the lowest layer: no layer at section -2, the walk up reaches section 2 (layer) and reads its y=0
    let mut w = DataLayer::new();
    w.set(1, 0, 1, 9);
    s.queue_section_data(sec(2), Some(w));
    run_end(&mut s);
    // the queued layer replaced section 2's layer in the end-of-run pass
    assert_eq!(s.visible_light(block_node(1, 2 * 16 + 7, 1)), 0);
    assert_eq!(s.visible_light(block_node(1, 2 * 16, 1)), 9);
}

#[test]
fn sky_layer_creation_copies_the_bottom_slice_of_the_section_above() {
    let mut s = LayerStorage::new(LightLayer::Sky);
    s.set_light_enabled(section_zero(sec(0)), true);
    // section 6 first: layers 5, 6, 7, all lazily filled with 15 because the column is light-enabled
    s.update_section_status(sec(6), false);
    run_end(&mut s);
    assert!(
        s.updating_layer(sec(5))
            .unwrap()
            .is_definitely_filled_with(15)
    );
    // put distinct data into the bottom slice of section 5, then register section 2: its layers are made from
    // the first layer found above (section 5), bottom slice replicated downward
    {
        let l = s.layer_for_write(sec(5)).unwrap();
        l.set(3, 0, 4, 2);
    }
    run_end(&mut s);
    s.update_section_status(sec(2), false);
    // sections 1, 2, 3 are created; the very first creation (section 2) copies from the first layer above it
    // (section 5, whose bottom slice holds the 2)
    let l2 = s.updating_layer(sec(2)).unwrap();
    for y in 0..16 {
        assert_eq!(l2.get(3, y, 4), 2, "replicated slice at y={y}");
    }
    assert_eq!(l2.get(0, 9, 0), 15);
}
