//! `BlockState.mirror(Mirror)` / `BlockState.rotate(Rotation)` for structure placement, and
//! small state-editing helpers (`withProperty`, default-state lookup) used by piece placement.
//!
//! The rules follow the behaviour of the block classes used by structure templates and legacy
//! pieces: every block that has a horizontal `facing` rotates/mirrors it; `axis` swaps `x`/`z` on
//! 90° rotations; the 16-step `rotation` property of standing signs, banners and skulls is
//! stepped; the four-way connection properties of fences, panes, bars, walls, redstone wire,
//! tripwire, vines, multiface blocks and huge mushrooms are permuted; stairs, doors and rails
//! have their own shape tables; the jigsaw/crafter `orientation` uses [`FrontAndTop`].

use std::collections::HashMap;

use zenith_registry::{BlockStateTable, Identifier};

use crate::geom::{Direction, FrontAndTop, Mirror, Rotation};

/// Looks up the default state of `minecraft:<name>` (or a full `ns:name`); `None` for unknown blocks.
pub fn default_state(blocks: &BlockStateTable, name: &str) -> Option<u32> {
    let id = if name.contains(':') {
        Identifier::parse(name).ok()?
    } else {
        Identifier::from_parts("minecraft", name).ok()?
    };
    blocks.block(&id).map(|e| e.default_state)
}

/// Value of property `prop` on `state_id`.
pub fn get_property(blocks: &BlockStateTable, state_id: u32, prop: &str) -> Option<String> {
    let st = blocks.state_by_id_exact(state_id as i32)?;
    st.property_value(prop).map(str::to_owned)
}

/// `state.setValue(prop, value)`; returns `state_id` unchanged if the block has no such
/// property or the value is not allowed.
pub fn with_property(blocks: &BlockStateTable, state_id: u32, prop: &str, value: &str) -> u32 {
    with_properties(blocks, state_id, &[(prop, value)])
}

/// Sets several properties at once (unknown properties/values are ignored).
pub fn with_properties(blocks: &BlockStateTable, state_id: u32, props: &[(&str, &str)]) -> u32 {
    let Some(st) = blocks.state_by_id_exact(state_id as i32) else {
        return state_id;
    };
    let Some(entry) = blocks.block(&st.block_id) else {
        return state_id;
    };
    let mut offset: u32 = 0;
    for p in &entry.properties {
        let cur = st.property_value(p.name()).unwrap_or("");
        let want = props
            .iter()
            .rev()
            .find(|(k, v)| *k == p.name() && p.values().iter().any(|x| x == v))
            .map_or(cur, |(_, v)| *v);
        let idx = p.values().iter().position(|x| x == want).unwrap_or(0) as u32;
        offset = offset * p.values().len() as u32 + idx;
    }
    entry.first_state + offset
}

/// Name (without namespace) of the block of `state_id`.
pub fn block_path(blocks: &BlockStateTable, state_id: u32) -> &str {
    blocks
        .state_by_id_exact(state_id as i32)
        .map_or("air", |s| s.block_id.path())
}

/// Whether `state_id` belongs to block `minecraft:<name>`.
pub fn is_block(blocks: &BlockStateTable, state_id: u32, name: &str) -> bool {
    blocks
        .state_by_id_exact(state_id as i32)
        .is_some_and(|s| s.block_id.namespace() == "minecraft" && s.block_id.path() == name)
}

fn dir_from(s: &str) -> Option<Direction> {
    Direction::from_name(s)
}

fn mirror_rotation_int(mirror: Mirror, rot: i32, steps: i32) -> i32 {
    let half = steps / 2;
    let full = if rot > half { rot - steps } else { rot };
    match mirror {
        Mirror::FrontBack => (steps - full).rem_euclid(steps),
        Mirror::LeftRight => (half - full + steps).rem_euclid(steps),
        Mirror::None => rot,
    }
}

fn rotate_rotation_int(rotation: Rotation, rot: i32, steps: i32) -> i32 {
    let quarter = steps / 4;
    match rotation {
        Rotation::None => rot,
        Rotation::Clockwise90 => (rot + quarter).rem_euclid(steps),
        Rotation::Clockwise180 => (rot + 2 * quarter).rem_euclid(steps),
        Rotation::Counterclockwise90 => (rot + 3 * quarter).rem_euclid(steps),
    }
}

fn swap_values(props: &mut [(String, String)], a: &str, b: &str) {
    let ia = props.iter().position(|(k, _)| k == a);
    let ib = props.iter().position(|(k, _)| k == b);
    if let (Some(ia), Some(ib)) = (ia, ib) {
        let va = props[ia].1.clone();
        props[ia].1 = props[ib].1.clone();
        props[ib].1 = va;
    }
}

fn is_four_way_block(path: &str) -> bool {
    path.ends_with("_fence")
        || path.ends_with("_pane")
        || path.ends_with("_bars")
        || (path.ends_with("_wall") && !path.contains("sign") && !path.contains("banner"))
        || matches!(
            path,
            "redstone_wire"
                | "tripwire"
                | "vine"
                | "glow_lichen"
                | "sculk_vein"
                | "resin_clump"
                | "pale_moss_carpet"
                | "brown_mushroom_block"
                | "red_mushroom_block"
                | "mushroom_stem"
        )
}

fn rail_mirror(shape: &str, mirror: Mirror) -> &str {
    match mirror {
        Mirror::None => shape,
        Mirror::LeftRight => match shape {
            "ascending_north" => "ascending_south",
            "ascending_south" => "ascending_north",
            "south_east" => "north_east",
            "south_west" => "north_west",
            "north_west" => "south_west",
            "north_east" => "south_east",
            other => other,
        },
        Mirror::FrontBack => match shape {
            "ascending_east" => "ascending_west",
            "ascending_west" => "ascending_east",
            "south_east" => "south_west",
            "south_west" => "south_east",
            "north_west" => "north_east",
            "north_east" => "north_west",
            other => other,
        },
    }
}

fn rail_rotate(shape: &str, rotation: Rotation) -> &str {
    match rotation {
        Rotation::None => shape,
        Rotation::Clockwise180 => match shape {
            "ascending_east" => "ascending_west",
            "ascending_west" => "ascending_east",
            "ascending_north" => "ascending_south",
            "ascending_south" => "ascending_north",
            "south_east" => "north_west",
            "south_west" => "north_east",
            "north_west" => "south_east",
            "north_east" => "south_west",
            other => other,
        },
        Rotation::Counterclockwise90 => match shape {
            "north_south" => "east_west",
            "east_west" => "north_south",
            "ascending_east" => "ascending_north",
            "ascending_west" => "ascending_south",
            "ascending_north" => "ascending_west",
            "ascending_south" => "ascending_east",
            "south_east" => "north_east",
            "south_west" => "south_east",
            "north_west" => "south_west",
            "north_east" => "north_west",
            other => other,
        },
        Rotation::Clockwise90 => match shape {
            "north_south" => "east_west",
            "east_west" => "north_south",
            "ascending_east" => "ascending_south",
            "ascending_west" => "ascending_north",
            "ascending_north" => "ascending_east",
            "ascending_south" => "ascending_west",
            "south_east" => "south_west",
            "south_west" => "north_west",
            "north_west" => "north_east",
            "north_east" => "south_east",
            other => other,
        },
    }
}

fn set(props: &mut [(String, String)], key: &str, value: &str) {
    if let Some(p) = props.iter_mut().find(|(k, _)| k == key) {
        p.1 = value.to_owned();
    }
}

fn find<'a>(props: &'a [(String, String)], key: &str) -> Option<&'a str> {
    props
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

/// Rotates the four horizontal connection properties by `rotation` (values travel with the
/// property: the value of `north` moves to `rotation.rotate(north)`).
fn rotate_four_way(props: &mut [(String, String)], rotation: Rotation) {
    if rotation == Rotation::None {
        return;
    }
    let old: Vec<(Direction, Option<String>)> = [
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
    ]
    .iter()
    .map(|d| (*d, find(props, d.name()).map(str::to_owned)))
    .collect();
    for (d, v) in old {
        if let Some(v) = v {
            let nd = rotation.rotate_dir(d);
            set(props, nd.name(), &v);
        }
    }
}

/// `state.mirror(mirror).rotate(rotation)` on the state `state_id`.
pub fn mirror_rotate_state(
    blocks: &BlockStateTable,
    state_id: u32,
    mirror: Mirror,
    rotation: Rotation,
) -> u32 {
    if mirror == Mirror::None && rotation == Rotation::None {
        return state_id;
    }
    let Some(st) = blocks.state_by_id_exact(state_id as i32) else {
        return state_id;
    };
    if st.properties.is_empty() {
        return state_id;
    }
    let path = st.block_id.path().to_owned();
    let mut props: Vec<(String, String)> = st.properties.clone();
    let mut changed = false;

    // ----- mirror -----
    if mirror != Mirror::None {
        if path.ends_with("_stairs") && find(&props, "shape").is_some() {
            if let Some(facing) = find(&props, "facing").and_then(dir_from) {
                let axis_hit = matches!(
                    (mirror, facing),
                    (Mirror::LeftRight, Direction::North | Direction::South)
                        | (Mirror::FrontBack, Direction::East | Direction::West)
                );
                if axis_hit {
                    let shape = find(&props, "shape").unwrap_or("straight").to_owned();
                    let new_shape = match shape.as_str() {
                        "inner_left" => "inner_right",
                        "inner_right" => "inner_left",
                        "outer_left" => "outer_right",
                        "outer_right" => "outer_left",
                        other => other,
                    };
                    set(&mut props, "shape", new_shape);
                    set(&mut props, "facing", facing.opposite().name());
                    changed = true;
                }
            }
        } else if path.ends_with("_door") && find(&props, "hinge").is_some() {
            if let Some(facing) = find(&props, "facing").and_then(dir_from) {
                set(&mut props, "facing", mirror.mirror_dir(facing).name());
            }
            let hinge = if find(&props, "hinge") == Some("left") {
                "right"
            } else {
                "left"
            };
            set(&mut props, "hinge", hinge);
            changed = true;
        } else {
            if let Some(facing) = find(&props, "facing").and_then(dir_from) {
                set(&mut props, "facing", mirror.mirror_dir(facing).name());
                changed = true;
            }
            if let Some(ori) = find(&props, "orientation").and_then(FrontAndTop::from_name) {
                set(&mut props, "orientation", ori.mirror(mirror).name());
                changed = true;
            }
            if let Some(rot) = find(&props, "rotation").and_then(|v| v.parse::<i32>().ok()) {
                let steps = rotation_steps(blocks, state_id);
                set(
                    &mut props,
                    "rotation",
                    &mirror_rotation_int(mirror, rot, steps).to_string(),
                );
                changed = true;
            }
            if is_four_way_block(&path) {
                match mirror {
                    Mirror::LeftRight => swap_values(&mut props, "north", "south"),
                    Mirror::FrontBack => swap_values(&mut props, "east", "west"),
                    Mirror::None => {}
                }
                changed = true;
            }
            if matches!(
                path.as_str(),
                "rail" | "powered_rail" | "detector_rail" | "activator_rail"
            ) && let Some(shape) = find(&props, "shape").map(str::to_owned)
            {
                set(&mut props, "shape", rail_mirror(&shape, mirror));
                changed = true;
            }
        }
    }

    // ----- rotation -----
    if rotation != Rotation::None {
        if let Some(facing) = find(&props, "facing").and_then(dir_from) {
            set(&mut props, "facing", rotation.rotate_dir(facing).name());
            changed = true;
        }
        if let Some(ori) = find(&props, "orientation").and_then(FrontAndTop::from_name) {
            set(&mut props, "orientation", ori.rotate(rotation).name());
            changed = true;
        }
        if let Some(rot) = find(&props, "rotation").and_then(|v| v.parse::<i32>().ok()) {
            let steps = rotation_steps(blocks, state_id);
            set(
                &mut props,
                "rotation",
                &rotate_rotation_int(rotation, rot, steps).to_string(),
            );
            changed = true;
        }
        if matches!(
            rotation,
            Rotation::Clockwise90 | Rotation::Counterclockwise90
        ) && let Some(axis) = find(&props, "axis").map(str::to_owned)
        {
            let na = match axis.as_str() {
                "x" => "z",
                "z" => "x",
                other => other,
            };
            set(&mut props, "axis", na);
            changed = true;
        }
        if is_four_way_block(&path) {
            rotate_four_way(&mut props, rotation);
            changed = true;
        }
        if matches!(
            path.as_str(),
            "rail" | "powered_rail" | "detector_rail" | "activator_rail"
        ) && let Some(shape) = find(&props, "shape").map(str::to_owned)
        {
            set(&mut props, "shape", rail_rotate(&shape, rotation));
            changed = true;
        }
    }

    if !changed {
        return state_id;
    }
    let pairs: Vec<(&str, &str)> = props
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    with_properties(blocks, state_id, &pairs)
}

/// Number of `rotation` steps of the property of `state_id` (16 for signs/banners/skulls).
fn rotation_steps(blocks: &BlockStateTable, state_id: u32) -> i32 {
    blocks
        .state_by_id_exact(state_id as i32)
        .and_then(|s| blocks.block(&s.block_id))
        .and_then(|e| e.properties.iter().find(|p| p.name() == "rotation"))
        .map_or(16, |p| p.values().len() as i32)
}

/// Memoising wrapper for [`mirror_rotate_state`] with a fixed `(mirror, rotation)`.
#[derive(Debug, Clone)]
pub struct StateTransformer {
    mirror: Mirror,
    rotation: Rotation,
    cache: HashMap<u32, u32>,
}

impl StateTransformer {
    /// Creates a transformer applying `mirror` then `rotation`.
    pub fn new(mirror: Mirror, rotation: Rotation) -> Self {
        Self {
            mirror,
            rotation,
            cache: HashMap::new(),
        }
    }

    /// Whether the transform is the identity.
    pub fn is_identity(&self) -> bool {
        self.mirror == Mirror::None && self.rotation == Rotation::None
    }

    /// Transforms `state_id`.
    pub fn apply(&mut self, blocks: &BlockStateTable, state_id: u32) -> u32 {
        if self.is_identity() {
            return state_id;
        }
        if let Some(v) = self.cache.get(&state_id) {
            return *v;
        }
        let v = mirror_rotate_state(blocks, state_id, self.mirror, self.rotation);
        self.cache.insert(state_id, v);
        v
    }
}

#[cfg(test)]
mod tests {
    use zenith_registry::{BlockDef, PropertyDef};

    use super::*;

    fn en(name: &str, values: &[&str]) -> PropertyDef {
        PropertyDef::enumeration(name, values.iter().map(|s| (*s).to_owned()).collect()).unwrap()
    }

    fn table() -> BlockStateTable {
        let id = |s: &str| Identifier::parse(&format!("minecraft:{s}")).unwrap();
        let facing = || en("facing", &["north", "south", "west", "east"]);
        let b = |p: &str| PropertyDef::boolean(p).unwrap();
        BlockStateTable::from_blocks(vec![
            BlockDef::new(id("air"), vec![]).unwrap(),
            BlockDef::new(
                id("oak_stairs"),
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
                ],
            )
            .unwrap(),
            BlockDef::new(
                id("oak_door"),
                vec![
                    facing(),
                    en("hinge", &["left", "right"]),
                    en("half", &["upper", "lower"]),
                ],
            )
            .unwrap(),
            BlockDef::new(
                id("rail"),
                vec![en(
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
                )],
            )
            .unwrap(),
            BlockDef::new(
                id("oak_fence"),
                vec![b("north"), b("east"), b("south"), b("west")],
            )
            .unwrap(),
            BlockDef::new(
                id("oak_sign"),
                vec![PropertyDef::integer("rotation", 0, 15).unwrap()],
            )
            .unwrap(),
            BlockDef::new(id("oak_log"), vec![en("axis", &["x", "y", "z"])]).unwrap(),
            BlockDef::new(id("stone_bricks"), vec![]).unwrap(),
        ])
        .unwrap()
    }

    fn st(t: &BlockStateTable, name: &str, props: &[(&str, &str)]) -> u32 {
        with_properties(t, default_state(t, name).unwrap(), props)
    }

    #[test]
    fn facing_rotates_clockwise_and_mirrors_on_its_axis() {
        let t = table();
        let s = st(&t, "oak_stairs", &[("facing", "north")]);
        let e = mirror_rotate_state(&t, s, Mirror::None, Rotation::Clockwise90);
        assert_eq!(get_property(&t, e, "facing").as_deref(), Some("east"));
        let w = mirror_rotate_state(&t, s, Mirror::None, Rotation::Counterclockwise90);
        assert_eq!(get_property(&t, w, "facing").as_deref(), Some("west"));
        let d = mirror_rotate_state(&t, s, Mirror::None, Rotation::Clockwise180);
        assert_eq!(get_property(&t, d, "facing").as_deref(), Some("south"));
    }

    #[test]
    fn stairs_mirror_swaps_inner_and_outer_sides() {
        let t = table();
        let s = st(
            &t,
            "oak_stairs",
            &[("facing", "north"), ("shape", "inner_left")],
        );
        let m = mirror_rotate_state(&t, s, Mirror::LeftRight, Rotation::None);
        assert_eq!(get_property(&t, m, "facing").as_deref(), Some("south"));
        assert_eq!(get_property(&t, m, "shape").as_deref(), Some("inner_right"));
        // Mirror across the other axis leaves a north-facing stair alone.
        let m = mirror_rotate_state(&t, s, Mirror::FrontBack, Rotation::None);
        assert_eq!(m, s);
    }

    #[test]
    fn door_mirror_flips_the_hinge() {
        let t = table();
        let s = st(&t, "oak_door", &[("facing", "east"), ("hinge", "left")]);
        let m = mirror_rotate_state(&t, s, Mirror::FrontBack, Rotation::None);
        assert_eq!(get_property(&t, m, "hinge").as_deref(), Some("right"));
        assert_eq!(get_property(&t, m, "facing").as_deref(), Some("west"));
    }

    #[test]
    fn fence_connections_rotate_with_the_block() {
        let t = table();
        let s = st(
            &t,
            "oak_fence",
            &[
                ("north", "true"),
                ("east", "false"),
                ("south", "false"),
                ("west", "false"),
            ],
        );
        let r = mirror_rotate_state(&t, s, Mirror::None, Rotation::Clockwise90);
        assert_eq!(get_property(&t, r, "east").as_deref(), Some("true"));
        assert_eq!(get_property(&t, r, "north").as_deref(), Some("false"));
        let m = mirror_rotate_state(&t, s, Mirror::LeftRight, Rotation::None);
        assert_eq!(get_property(&t, m, "south").as_deref(), Some("true"));
    }

    #[test]
    fn sign_rotation_steps_and_mirror() {
        let t = table();
        let s0 = st(&t, "oak_sign", &[("rotation", "0")]);
        let r = mirror_rotate_state(&t, s0, Mirror::None, Rotation::Clockwise90);
        assert_eq!(get_property(&t, r, "rotation").as_deref(), Some("4"));
        let r = mirror_rotate_state(&t, s0, Mirror::LeftRight, Rotation::None);
        assert_eq!(get_property(&t, r, "rotation").as_deref(), Some("8"));
        let s4 = st(&t, "oak_sign", &[("rotation", "4")]);
        let r = mirror_rotate_state(&t, s4, Mirror::FrontBack, Rotation::None);
        assert_eq!(get_property(&t, r, "rotation").as_deref(), Some("12"));
    }

    #[test]
    fn axis_swaps_only_on_quarter_turns() {
        let t = table();
        let s = st(&t, "oak_log", &[("axis", "x")]);
        let r = mirror_rotate_state(&t, s, Mirror::None, Rotation::Clockwise90);
        assert_eq!(get_property(&t, r, "axis").as_deref(), Some("z"));
        assert_eq!(
            mirror_rotate_state(&t, s, Mirror::None, Rotation::Clockwise180),
            s
        );
        assert_eq!(
            mirror_rotate_state(&t, s, Mirror::LeftRight, Rotation::None),
            s
        );
    }

    #[test]
    fn rail_shapes_follow_the_tables() {
        let t = table();
        let s = st(&t, "rail", &[("shape", "ascending_east")]);
        let r = mirror_rotate_state(&t, s, Mirror::None, Rotation::Clockwise90);
        assert_eq!(
            get_property(&t, r, "shape").as_deref(),
            Some("ascending_south")
        );
        let m = mirror_rotate_state(&t, s, Mirror::FrontBack, Rotation::None);
        assert_eq!(
            get_property(&t, m, "shape").as_deref(),
            Some("ascending_west")
        );
        let c = st(&t, "rail", &[("shape", "south_east")]);
        let r = mirror_rotate_state(&t, c, Mirror::None, Rotation::Clockwise180);
        assert_eq!(get_property(&t, r, "shape").as_deref(), Some("north_west"));
    }

    #[test]
    fn state_transformer_caches_and_identity_is_free() {
        let t = table();
        let s = st(&t, "oak_stairs", &[("facing", "north")]);
        let mut id = StateTransformer::new(Mirror::None, Rotation::None);
        assert!(id.is_identity());
        assert_eq!(id.apply(&t, s), s);
        let mut x = StateTransformer::new(Mirror::None, Rotation::Clockwise90);
        let a = x.apply(&t, s);
        assert_eq!(x.apply(&t, s), a);
        assert_eq!(get_property(&t, a, "facing").as_deref(), Some("east"));
    }

    #[test]
    fn with_property_ignores_unknown_values() {
        let t = table();
        let s = default_state(&t, "oak_stairs").unwrap();
        assert_eq!(with_property(&t, s, "facing", "up"), s);
        assert_eq!(with_property(&t, s, "nope", "x"), s);
        assert!(is_block(&t, s, "oak_stairs"));
        assert_eq!(block_path(&t, s), "oak_stairs");
    }
}
