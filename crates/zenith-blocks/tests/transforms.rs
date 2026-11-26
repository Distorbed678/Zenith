//! Rotation and mirror of block states, checked against the spot values of `docs/specs/blocks/state-transforms.md`
//! (no game files needed), plus the spec's counts and laws on the full registry when the tables are cached.

use zenith_blocks::{BlockTransforms, Mirror, Rotation, TransformRule};
use zenith_registry::{BlockDef, BlockStateTable, Identifier, PropertyDef};

fn id(name: &str) -> Identifier {
    Identifier::parse(&format!("minecraft:{name}")).unwrap()
}

fn en(name: &str, values: &[&str]) -> PropertyDef {
    PropertyDef::enumeration(name, values.iter().map(|v| (*v).to_owned()).collect()).unwrap()
}

fn boolean(name: &str) -> PropertyDef {
    PropertyDef::boolean(name).unwrap()
}

fn block(name: &str, props: Vec<PropertyDef>) -> BlockDef {
    BlockDef::new(id(name), props).unwrap()
}

const H4: [&str; 4] = ["north", "south", "west", "east"];

fn mini() -> BlockStateTable {
    let defs = vec![
        block("stone", vec![]),
        block(
            "oak_stairs",
            vec![
                en("facing", &H4),
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
                boolean("waterlogged"),
            ],
        ),
        block(
            "rail",
            vec![
                en(
                    "shape",
                    &[
                        "north_south",
                        "east_west",
                        "ascending_east",
                        "ascending_west",
                        "ascending_north",
                        "ascending_south",
                        "south_east",
                        "south_west",
                        "north_west",
                        "north_east",
                    ],
                ),
                boolean("waterlogged"),
            ],
        ),
        block(
            "chest",
            vec![
                en("facing", &H4),
                en("type", &["single", "left", "right"]),
                boolean("waterlogged"),
            ],
        ),
        block(
            "oak_sign",
            vec![
                PropertyDef::integer("rotation", 0, 15).unwrap(),
                boolean("waterlogged"),
            ],
        ),
        block(
            "red_bed",
            vec![
                en("facing", &H4),
                boolean("occupied"),
                en("part", &["head", "foot"]),
            ],
        ),
        block(
            "oak_door",
            vec![
                en("facing", &H4),
                en("half", &["upper", "lower"]),
                en("hinge", &["left", "right"]),
                boolean("open"),
                boolean("powered"),
            ],
        ),
        block(
            "jigsaw",
            vec![en(
                "orientation",
                &[
                    "down_east",
                    "down_north",
                    "down_south",
                    "down_west",
                    "up_east",
                    "up_north",
                    "up_south",
                    "up_west",
                    "west_up",
                    "east_up",
                    "north_up",
                    "south_up",
                ],
            )],
        ),
        block("oak_log", vec![en("axis", &["x", "y", "z"])]),
        block("anvil", vec![en("facing", &H4)]),
        block(
            "oak_fence",
            vec![
                boolean("east"),
                boolean("north"),
                boolean("south"),
                boolean("waterlogged"),
                boolean("west"),
            ],
        ),
    ];
    BlockStateTable::from_blocks(defs).unwrap()
}

fn state(t: &BlockStateTable, block: &str, props: &[(&str, &str)]) -> u32 {
    t.id_of_exact(&id(block), props)
        .unwrap_or_else(|| panic!("no state {block} {props:?}"))
}

fn prop<'a>(t: &'a BlockStateTable, state: u32, name: &str) -> &'a str {
    t.state_by_id_exact(state as i32)
        .unwrap()
        .property_value(name)
        .unwrap()
}

#[test]
fn enum_primitives() {
    use zenith_blocks::Direction as D;
    assert_eq!(Rotation::Clockwise90.rotate_direction(D::North), D::East);
    assert_eq!(Rotation::Clockwise180.rotate_direction(D::East), D::West);
    assert_eq!(
        Rotation::CounterClockwise90.rotate_direction(D::North),
        D::West
    );
    assert_eq!(Rotation::Clockwise90.rotate_direction(D::Up), D::Up);
    assert_eq!(Rotation::Clockwise180.rotate_direction(D::Down), D::Down);
    assert_eq!(Mirror::LeftRight.mirror_direction(D::North), D::South);
    assert_eq!(Mirror::LeftRight.mirror_direction(D::East), D::East);
    assert_eq!(Mirror::FrontBack.mirror_direction(D::East), D::West);
    assert_eq!(Mirror::FrontBack.mirror_direction(D::Up), D::Up);
    for m in Mirror::ALL {
        for d in D::ALL {
            assert_eq!(
                m.as_rotation(d).rotate_direction(d),
                m.mirror_direction(d),
                "{m:?} {d:?}"
            );
        }
    }
    // The index table of §6 T7.
    assert_eq!(Rotation::Clockwise90.rotate_index(15, 16), 3);
    assert_eq!(Mirror::LeftRight.mirror_index(1, 16), 7);
    assert_eq!(Mirror::LeftRight.mirror_index(0, 16), 8);
    assert_eq!(Mirror::FrontBack.mirror_index(1, 16), 15);
    assert_eq!(Mirror::FrontBack.mirror_index(0, 16), 0);
    assert_eq!(
        Rotation::Clockwise90.then(Rotation::CounterClockwise90),
        Rotation::None
    );
    assert_eq!(
        Rotation::Clockwise90.inverse(),
        Rotation::CounterClockwise90
    );
}

#[test]
fn appendix_assigns_the_documented_rules() {
    assert_eq!(
        TransformRule::of_block("stone"),
        Some(TransformRule::Identity)
    );
    assert_eq!(
        TransformRule::of_block("oak_log"),
        Some(TransformRule::Axis)
    );
    assert_eq!(
        TransformRule::of_block("chest"),
        Some(TransformRule::Facing)
    );
    assert_eq!(TransformRule::of_block("anvil"), Some(TransformRule::Anvil));
    assert_eq!(
        TransformRule::of_block("oak_door"),
        Some(TransformRule::Door)
    );
    assert_eq!(
        TransformRule::of_block("oak_stairs"),
        Some(TransformRule::Stairs)
    );
    assert_eq!(TransformRule::of_block("rail"), Some(TransformRule::Rail));
    assert_eq!(
        TransformRule::of_block("oak_sign"),
        Some(TransformRule::RotationIndex)
    );
    assert_eq!(
        TransformRule::of_block("oak_fence"),
        Some(TransformRule::Sides)
    );
    assert_eq!(
        TransformRule::of_block("jigsaw"),
        Some(TransformRule::Orientation)
    );
    assert_eq!(
        TransformRule::of_block("fire"),
        Some(TransformRule::Identity)
    );
    assert_eq!(TransformRule::of_block("not_a_block"), None);
}

#[test]
fn unknown_block_is_an_error() {
    let t = BlockStateTable::from_blocks(vec![block("not_a_block", vec![])]).unwrap();
    assert!(BlockTransforms::build(&t).is_err());
}

#[test]
fn stairs() {
    let t = mini();
    let x = BlockTransforms::build(&t).unwrap();
    let s = state(
        &t,
        "oak_stairs",
        &[
            ("facing", "north"),
            ("half", "bottom"),
            ("shape", "straight"),
            ("waterlogged", "false"),
        ],
    );
    let facing = |id| prop(&t, id, "facing").to_owned();
    assert_eq!(facing(x.rotate(s, Rotation::Clockwise90)), "east");
    assert_eq!(facing(x.rotate(s, Rotation::Clockwise180)), "south");
    assert_eq!(facing(x.rotate(s, Rotation::CounterClockwise90)), "west");
    assert_eq!(facing(x.mirror(s, Mirror::LeftRight)), "south");
    assert_eq!(x.mirror(s, Mirror::FrontBack), s);
    // §4: left_right swaps both inner and outer, front_back only the outer shapes.
    let with = |facing: &str, shape: &str| {
        state(
            &t,
            "oak_stairs",
            &[
                ("facing", facing),
                ("half", "top"),
                ("shape", shape),
                ("waterlogged", "true"),
            ],
        )
    };
    assert_eq!(
        x.mirror(with("north", "inner_left"), Mirror::LeftRight),
        with("south", "inner_right")
    );
    assert_eq!(
        x.mirror(with("east", "inner_left"), Mirror::FrontBack),
        with("west", "inner_left")
    );
    assert_eq!(
        x.mirror(with("east", "outer_left"), Mirror::FrontBack),
        with("west", "outer_right")
    );
    assert_eq!(
        x.mirror(with("east", "outer_left"), Mirror::LeftRight),
        with("east", "outer_left")
    );
}

#[test]
fn rails() {
    let t = mini();
    let x = BlockTransforms::build(&t).unwrap();
    let rail = |shape: &str| state(&t, "rail", &[("shape", shape), ("waterlogged", "false")]);
    let ne = rail("north_east");
    assert_eq!(x.mirror(ne, Mirror::LeftRight), rail("south_east"));
    assert_eq!(x.mirror(ne, Mirror::FrontBack), rail("north_west"));
    assert_eq!(x.rotate(ne, Rotation::Clockwise90), rail("south_east"));
    assert_eq!(x.rotate(ne, Rotation::Clockwise180), rail("south_west"));
    assert_eq!(
        x.rotate(ne, Rotation::CounterClockwise90),
        rail("north_west")
    );
    assert_eq!(
        x.transform(ne, Mirror::LeftRight, Rotation::Clockwise90),
        rail("south_west")
    );
}

#[test]
fn chests_keep_their_type() {
    let t = mini();
    let x = BlockTransforms::build(&t).unwrap();
    let chest = |facing: &str, ty: &str| {
        state(
            &t,
            "chest",
            &[("facing", facing), ("type", ty), ("waterlogged", "false")],
        )
    };
    assert_eq!(
        x.mirror(chest("north", "left"), Mirror::LeftRight),
        chest("south", "left")
    );
    assert_eq!(
        x.mirror(chest("east", "right"), Mirror::FrontBack),
        chest("west", "right")
    );
    assert_eq!(
        x.mirror(chest("east", "right"), Mirror::LeftRight),
        chest("east", "right")
    );
}

#[test]
fn signs_beds_doors_jigsaw() {
    let t = mini();
    let x = BlockTransforms::build(&t).unwrap();
    let sign = |r: u32| {
        state(
            &t,
            "oak_sign",
            &[("rotation", &r.to_string()), ("waterlogged", "false")],
        )
    };
    let rot_of = |id| prop(&t, id, "rotation").parse::<u32>().unwrap();
    for (r, want) in [
        (1, [5, 9, 13, 7, 15]),
        (3, [7, 11, 15, 5, 13]),
        (15, [3, 7, 11, 9, 1]),
    ] {
        let s = sign(r);
        let got = [
            rot_of(x.rotate(s, Rotation::Clockwise90)),
            rot_of(x.rotate(s, Rotation::Clockwise180)),
            rot_of(x.rotate(s, Rotation::CounterClockwise90)),
            rot_of(x.mirror(s, Mirror::LeftRight)),
            rot_of(x.mirror(s, Mirror::FrontBack)),
        ];
        assert_eq!(got, want, "sign rotation {r}");
    }

    let bed = |facing: &str, part: &str| {
        state(
            &t,
            "red_bed",
            &[("facing", facing), ("occupied", "false"), ("part", part)],
        )
    };
    assert_eq!(
        x.rotate(bed("north", "head"), Rotation::Clockwise90),
        bed("east", "head")
    );
    assert_eq!(
        x.mirror(bed("north", "head"), Mirror::LeftRight),
        bed("south", "head")
    );
    assert_eq!(
        x.mirror(bed("east", "foot"), Mirror::LeftRight),
        bed("east", "foot")
    );

    let door = |facing: &str, hinge: &str| {
        state(
            &t,
            "oak_door",
            &[
                ("facing", facing),
                ("half", "lower"),
                ("hinge", hinge),
                ("open", "false"),
                ("powered", "false"),
            ],
        )
    };
    let d = door("north", "left");
    assert_eq!(x.mirror(d, Mirror::LeftRight), door("south", "right"));
    assert_eq!(x.mirror(d, Mirror::FrontBack), door("north", "right"));
    assert_eq!(
        x.transform(d, Mirror::LeftRight, Rotation::Clockwise90),
        door("west", "right")
    );
    // Rotating first and mirroring second gives another result (§5 item 5).
    assert_eq!(
        x.mirror(x.rotate(d, Rotation::Clockwise90), Mirror::LeftRight),
        door("east", "right")
    );

    let jig = |o: &str| state(&t, "jigsaw", &[("orientation", o)]);
    assert_eq!(
        x.rotate(jig("down_east"), Rotation::Clockwise90),
        jig("down_south")
    );
    assert_eq!(
        x.mirror(jig("down_east"), Mirror::FrontBack),
        jig("down_west")
    );
    assert_eq!(
        x.rotate(jig("north_up"), Rotation::Clockwise90),
        jig("east_up")
    );
    assert_eq!(x.mirror(jig("up_west"), Mirror::FrontBack), jig("up_east"));
}

#[test]
fn axis_anvil_fence_identity() {
    let t = mini();
    let x = BlockTransforms::build(&t).unwrap();
    let log = |a: &str| state(&t, "oak_log", &[("axis", a)]);
    assert_eq!(x.rotate(log("x"), Rotation::Clockwise90), log("z"));
    assert_eq!(x.rotate(log("x"), Rotation::Clockwise180), log("x"));
    assert_eq!(x.rotate(log("y"), Rotation::CounterClockwise90), log("y"));
    assert_eq!(x.mirror(log("x"), Mirror::FrontBack), log("x"));

    let anvil = |f: &str| state(&t, "anvil", &[("facing", f)]);
    assert_eq!(
        x.rotate(anvil("north"), Rotation::Clockwise90),
        anvil("east")
    );
    assert_eq!(x.mirror(anvil("north"), Mirror::LeftRight), anvil("north"));

    let fence = |n: &str, e: &str, s: &str, w: &str| {
        state(
            &t,
            "oak_fence",
            &[
                ("east", e),
                ("north", n),
                ("south", s),
                ("waterlogged", "true"),
                ("west", w),
            ],
        )
    };
    let f = fence("true", "false", "false", "false");
    assert_eq!(
        x.rotate(f, Rotation::Clockwise90),
        fence("false", "true", "false", "false")
    );
    assert_eq!(
        x.rotate(f, Rotation::CounterClockwise90),
        fence("false", "false", "false", "true")
    );
    assert_eq!(
        x.mirror(f, Mirror::LeftRight),
        fence("false", "false", "true", "false")
    );
    assert_eq!(x.mirror(f, Mirror::FrontBack), f);

    let stone = state(&t, "stone", &[]);
    for m in Mirror::ALL {
        for r in Rotation::ALL {
            assert_eq!(x.transform(stone, m, r), stone);
        }
    }
    // Out-of-range ids come back unchanged.
    assert_eq!(x.rotate(99_999, Rotation::Clockwise90), 99_999);
}

/// The spec's counts and laws on the whole 26.3 registry, when the data-generator tables are cached.
#[test]
fn full_registry_when_cached() {
    let path = zenith_data::version_dir("26.3").join("tables.bin");
    let Ok(tables) = zenith_data::Tables::load(&path) else {
        eprintln!("skipping: no cached tables at {}", path.display());
        return;
    };
    let table = BlockStateTable::from_tables(&tables).unwrap();
    let x = BlockTransforms::build(&table).unwrap();
    assert_eq!(x.len(), 35_723);
    let changed = |m: Mirror, r: Rotation| {
        (0..35_723u32)
            .filter(|&s| x.transform(s, m, r) != s)
            .count()
    };
    assert_eq!(changed(Mirror::None, Rotation::Clockwise90), 30_206);
    assert_eq!(changed(Mirror::None, Rotation::Clockwise180), 28_876);
    assert_eq!(changed(Mirror::None, Rotation::CounterClockwise90), 30_206);
    assert_eq!(changed(Mirror::LeftRight, Rotation::None), 18_668);
    assert_eq!(changed(Mirror::FrontBack, Rotation::None), 18_668);
    assert_eq!(changed(Mirror::LeftRight, Rotation::Clockwise90), 28_858);
    assert_eq!(changed(Mirror::LeftRight, Rotation::Clockwise180), 21_784);
    assert_eq!(changed(Mirror::FrontBack, Rotation::Clockwise180), 20_232);
    for s in 0..35_723u32 {
        let block = table.state_by_id_exact(s as i32).unwrap().block_index;
        for m in Mirror::ALL {
            // Each mirror is an involution.
            assert_eq!(x.mirror(x.mirror(s, m), m), s);
            for r in Rotation::ALL {
                let t = x.transform(s, m, r);
                assert_eq!(
                    table.state_by_id_exact(t as i32).unwrap().block_index,
                    block
                );
                // Rotations compose.
                for r2 in Rotation::ALL {
                    assert_eq!(x.rotate(x.rotate(s, r), r2), x.rotate(s, r.then(r2)));
                }
            }
        }
    }
}
