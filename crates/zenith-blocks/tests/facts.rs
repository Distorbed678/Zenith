//! Facts of a handful of blocks, checked against values stated in `docs/specs/blocks/` (no game files needed), plus
//! a full-registry smoke test that skips when the data-generator tables are not cached.

use zenith_blocks::{
    BlockFactTags, BlockFacts, Direction, FluidKind, OffsetType, ShapeKind, SupportType, flag,
};
use zenith_registry::{BlockDef, BlockStateTable, Identifier, PropertyDef};

fn id(name: &str) -> Identifier {
    Identifier::parse(&format!("minecraft:{name}")).unwrap()
}

fn en(name: &str, values: &[&str]) -> PropertyDef {
    PropertyDef::enumeration(name, values.iter().map(|v| (*v).to_owned()).collect()).unwrap()
}

fn block(name: &str, props: Vec<PropertyDef>) -> BlockDef {
    BlockDef::new(id(name), props).unwrap()
}

fn mini() -> (BlockStateTable, BlockFacts) {
    let wl = || PropertyDef::boolean("waterlogged").unwrap();
    let facing = || en("facing", &["north", "south", "west", "east"]);
    let defs = vec![
        block("stone", vec![]),
        block(
            "oak_slab",
            vec![en("type", &["top", "bottom", "double"]), wl()],
        ),
        block(
            "oak_stairs",
            vec![
                facing(),
                en("half", &["top", "bottom"]),
                en(
                    "shape",
                    &[
                        "straight",
                        "inner_left",
                        "inner_right",
                        "outer_left",
                        "outer_right",
                    ],
                ),
                wl(),
            ],
        ),
        block(
            "oak_fence",
            vec![
                PropertyDef::boolean("east").unwrap(),
                PropertyDef::boolean("north").unwrap(),
                PropertyDef::boolean("south").unwrap(),
                wl(),
                PropertyDef::boolean("west").unwrap(),
            ],
        ),
        block("water", vec![PropertyDef::integer("level", 0, 15).unwrap()]),
        block("dandelion", vec![]),
        block(
            "oak_leaves",
            vec![
                PropertyDef::integer("distance", 1, 7).unwrap(),
                PropertyDef::boolean("persistent").unwrap(),
                wl(),
            ],
        ),
    ];
    let table = BlockStateTable::from_blocks(defs).unwrap();
    let facts =
        BlockFacts::build_partial(&table, &BlockFactTags::empty(table.blocks().len())).unwrap();
    (table, facts)
}

fn state(table: &BlockStateTable, name: &str, props: &[(&str, &str)]) -> u32 {
    table
        .id_of_exact(&id(name), props)
        .unwrap_or_else(|| panic!("no state {name} {props:?}"))
}

fn px(s: &zenith_blocks::VoxelShape) -> Vec<[f64; 6]> {
    s.boxes()
        .iter()
        .map(|b| b.to_array().map(|v| v * 16.0))
        .collect()
}

#[test]
fn stone_is_a_plain_full_cube() {
    let (t, f) = mini();
    let s = f.state(state(&t, "stone", &[]));
    assert!(s.has(flag::SOLID_RENDER | flag::COLLISION_SHAPE_FULL_BLOCK));
    assert!(s.has(flag::IS_DEFAULT_STATE) && s.has(flag::CAN_OCCLUDE) && s.has(flag::LEGACY_SOLID));
    assert!(s.has(flag::REQUIRES_CORRECT_TOOL_FOR_DROPS));
    assert!(!s.has(flag::PROPAGATES_SKYLIGHT_DOWN));
    assert_eq!(
        (s.light_block, s.map_color_id, s.map_color_rgb),
        (15, 11, 0x70_70_70)
    );
    assert_eq!(s.destroy_speed, 1.5);
    for d in Direction::ALL {
        for ty in SupportType::ALL {
            assert!(s.is_face_sturdy(d, ty));
        }
        assert!(
            f.face_occlusion_shape(state(&t, "stone", &[]), d)
                .is_block()
        );
    }
}

#[test]
fn bottom_slab_and_stairs_boxes_follow_the_spec_examples() {
    let (t, f) = mini();
    let slab = state(
        &t,
        "oak_slab",
        &[("type", "bottom"), ("waterlogged", "false")],
    );
    assert_eq!(
        px(f.shape_at_origin(slab, ShapeKind::Collision)),
        vec![[0.0, 0.0, 0.0, 16.0, 8.0, 16.0]]
    );
    let s = f.state(slab);
    assert!(s.has(flag::USE_SHAPE_FOR_LIGHT_OCCLUSION) && !s.has(flag::SOLID_RENDER));
    assert!(s.is_face_sturdy(Direction::Down, SupportType::Full));
    assert!(!s.is_face_sturdy(Direction::Up, SupportType::Full));
    assert!(
        !s.is_face_sturdy(Direction::Up, SupportType::Center),
        "the slab top is below the probe layer"
    );
    assert!(!s.is_face_sturdy(Direction::North, SupportType::Center));
    let double = state(
        &t,
        "oak_slab",
        &[("type", "double"), ("waterlogged", "false")],
    );
    assert!(
        f.state(double).has(flag::SOLID_RENDER)
            && !f.state(double).has(flag::USE_SHAPE_FOR_LIGHT_OCCLUSION)
    );

    // shapes.md §2.2 worked example: north-facing bottom straight stairs.
    let stairs = |half: &str| {
        state(
            &t,
            "oak_stairs",
            &[
                ("facing", "north"),
                ("half", half),
                ("shape", "straight"),
                ("waterlogged", "false"),
            ],
        )
    };
    assert_eq!(
        px(f.shape_at_origin(stairs("bottom"), ShapeKind::Outline)),
        vec![
            [0.0, 0.0, 0.0, 16.0, 8.0, 16.0],
            [0.0, 8.0, 0.0, 16.0, 16.0, 8.0]
        ]
    );
    assert_eq!(
        px(f.shape_at_origin(stairs("top"), ShapeKind::Outline)),
        vec![
            [0.0, 0.0, 0.0, 16.0, 16.0, 8.0],
            [0.0, 8.0, 8.0, 16.0, 16.0, 16.0]
        ]
    );
    // Slice-bounds quirk: the UP face occlusion shape of the bottom stair has one box but the full bounds.
    let up = f.face_occlusion_shape(stairs("bottom"), Direction::Up);
    assert_eq!(px(up), vec![[0.0, 0.0, 0.0, 16.0, 16.0, 8.0]]);
    assert_eq!(up.bounds().unwrap(), zenith_blocks::Aabb::UNIT);
}

#[test]
fn fence_collision_is_tall() {
    let (t, f) = mini();
    let post = state(
        &t,
        "oak_fence",
        &[
            ("east", "false"),
            ("north", "false"),
            ("south", "false"),
            ("waterlogged", "false"),
            ("west", "false"),
        ],
    );
    let s = f.state(post);
    assert!(s.has(flag::HAS_LARGE_COLLISION_SHAPE) && !s.has(flag::COLLISION_SHAPE_FULL_BLOCK));
    assert_eq!(
        px(f.shape_at_origin(post, ShapeKind::Collision)),
        vec![[6.0, 0.0, 6.0, 10.0, 24.0, 10.0]]
    );
    assert_eq!(
        px(f.shape_at_origin(post, ShapeKind::Outline)),
        vec![[6.0, 0.0, 6.0, 10.0, 16.0, 10.0]]
    );
    assert!(s.is_face_sturdy(Direction::Up, SupportType::Center));
    assert!(!s.is_face_sturdy(Direction::Up, SupportType::Full));
}

#[test]
fn liquids_report_their_fluid_states() {
    let (t, f) = mini();
    let level = |l: &str| f.state(state(&t, "water", &[("level", l)])).fluid;
    let source = level("0");
    assert_eq!(
        (source.kind, source.amount, source.is_source, source.falling),
        (FluidKind::Water, 8, true, false)
    );
    assert_eq!(source.own_height, 8.0_f32 / 9.0_f32);
    let flowing = level("3");
    assert_eq!(
        (flowing.kind, flowing.amount, flowing.is_source),
        (FluidKind::FlowingWater, 5, false)
    );
    let falling = level("9");
    assert_eq!((falling.amount, falling.falling), (8, true));
    assert!(
        f.state(state(&t, "water", &[("level", "0")]))
            .has(flag::LIQUID | flag::REPLACEABLE)
    );
}

#[test]
fn flowers_move_with_the_position_offset() {
    let (t, f) = mini();
    let d = state(&t, "dandelion", &[]);
    assert_eq!(f.state(d).offset_type, OffsetType::Xz);
    // shapes.md §5.4 reference: every nibble is 0 at the origin, so the offset is (-0.25, 0, -0.25).
    assert_eq!(f.offset(d, [0, 0, 0]), [-0.25, 0.0, -0.25]);
    let o = f.shape_at(d, ShapeKind::Outline, [0, 0, 0]);
    assert_eq!(o.bounds().unwrap().min[0], 5.0 / 16.0 - 0.25);
    // The shape at another column is the unmoved box plus that column's offset.
    let p1 = f.offset(d, [1, 0, 3]);
    let moved = f.shape_at(d, ShapeKind::Outline, [1, 0, 3]);
    assert_eq!(moved.bounds().unwrap().min[0], 5.0 / 16.0 + p1[0]);
    assert!(!f.state(d).has(flag::IS_AIR));
}

#[test]
fn leaves_do_not_support_but_dampen_light() {
    let (t, f) = mini();
    let l = state(
        &t,
        "oak_leaves",
        &[
            ("distance", "7"),
            ("persistent", "false"),
            ("waterlogged", "false"),
        ],
    );
    let s = f.state(l);
    assert_eq!(s.light_block, 1);
    assert!(
        s.has(flag::IS_RANDOMLY_TICKING)
            && !s.has(flag::SOLID_RENDER)
            && s.has(flag::COLLISION_SHAPE_FULL_BLOCK)
    );
    for d in Direction::ALL {
        assert!(
            !s.is_face_sturdy(d, SupportType::Full),
            "leaves support nothing"
        );
    }
    let near = state(
        &t,
        "oak_leaves",
        &[
            ("distance", "3"),
            ("persistent", "false"),
            ("waterlogged", "false"),
        ],
    );
    assert!(!f.state(near).has(flag::IS_RANDOMLY_TICKING));
}

/// Builds the whole 26.3 registry when the data-generator tables are cached; otherwise does nothing.
#[test]
fn full_registry_when_cached() {
    let path = zenith_data::version_dir("26.3").join("tables.bin");
    let Ok(tables) = zenith_data::Tables::load(&path) else {
        eprintln!("skipping: no cached tables at {}", path.display());
        return;
    };
    let table = BlockStateTable::from_tables(&tables).unwrap();
    let facts = BlockFacts::build(&table, &BlockFactTags::empty(table.blocks().len())).unwrap();
    assert_eq!(facts.len(), 35_723);
    // 1,286 blocks, every state has six shape ids and the pool shares identical shapes.
    assert_eq!(facts.blocks().len(), 1286);
    assert!(facts.shape_count() < 2_000);
}
