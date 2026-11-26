//! Rotating and mirroring block states (clean-room implementation of `docs/specs/blocks/state-transforms.md`).
//!
//! [`BlockTransforms`] is a precomputed table: for every global block-state id the id of the state after each
//! [`Rotation`] and after each [`Mirror`], in flat arrays. The table is derived from the block-state registry and
//! the rules T0–T9 of the spec; which rule a block follows comes from the spec's appendix, which is embedded at
//! compile time and parsed once (the same way the block table is).
//!
//! Template placement applies the mirror first and the rotation second; [`BlockTransforms::transform`] does that.

use std::collections::HashMap;
use std::sync::OnceLock;

use zenith_registry::{BlockEntry, BlockStateTable};

use crate::BuildError;
use crate::shape::Direction;

const SPEC: &str = include_str!("../../../docs/specs/blocks/state-transforms.md");

/// A rotation about the vertical axis, clockwise as seen from above.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Rotation {
    /// No rotation.
    None,
    /// A quarter turn clockwise (north → east).
    Clockwise90,
    /// A half turn.
    Clockwise180,
    /// A quarter turn counter-clockwise (north → west).
    CounterClockwise90,
}

impl Rotation {
    /// All rotations in index order.
    pub const ALL: [Rotation; 4] = [
        Rotation::None,
        Rotation::Clockwise90,
        Rotation::Clockwise180,
        Rotation::CounterClockwise90,
    ];

    /// The index 0–3 (`none`, `clockwise_90`, `clockwise_180`, `counterclockwise_90`).
    #[inline]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The rotation with index `i mod 4`.
    pub const fn from_index(i: usize) -> Rotation {
        Rotation::ALL[i % 4]
    }

    /// The snake-case name used by data files (`clockwise_90`, …).
    pub const fn name(self) -> &'static str {
        match self {
            Rotation::None => "none",
            Rotation::Clockwise90 => "clockwise_90",
            Rotation::Clockwise180 => "clockwise_180",
            Rotation::CounterClockwise90 => "counterclockwise_90",
        }
    }

    /// Parses a snake-case rotation name.
    pub fn from_name(s: &str) -> Option<Rotation> {
        Rotation::ALL.into_iter().find(|r| r.name() == s)
    }

    /// Performing `self` and then `other` (the cyclic group of order 4).
    pub const fn then(self, other: Rotation) -> Rotation {
        Rotation::from_index(self.index() + other.index())
    }

    /// The rotation that undoes `self`.
    pub const fn inverse(self) -> Rotation {
        Rotation::from_index(4 - self.index())
    }

    /// Rotates a direction; up and down are unchanged (spec §2 `rotateDir`).
    pub const fn rotate_direction(self, d: Direction) -> Direction {
        match self {
            Rotation::None => d,
            Rotation::Clockwise90 => d.clockwise(),
            Rotation::Clockwise180 => match d {
                Direction::Up | Direction::Down => d,
                _ => d.opposite(),
            },
            Rotation::CounterClockwise90 => d.counter_clockwise(),
        }
    }

    /// Rotates an index `v` in `[0, n)` (spec §2 rotate-an-index).
    pub const fn rotate_index(self, v: u32, n: u32) -> u32 {
        match self {
            Rotation::None => v,
            Rotation::Clockwise90 => (v + n / 4) % n,
            Rotation::Clockwise180 => (v + n / 2) % n,
            Rotation::CounterClockwise90 => (v + 3 * n / 4) % n,
        }
    }
}

/// A mirror across a vertical plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Mirror {
    /// No mirror.
    None,
    /// Flips the Z axis (north ↔ south).
    LeftRight,
    /// Flips the X axis (east ↔ west).
    FrontBack,
}

impl Mirror {
    /// All mirrors in index order.
    pub const ALL: [Mirror; 3] = [Mirror::None, Mirror::LeftRight, Mirror::FrontBack];

    /// The index 0–2 (`none`, `left_right`, `front_back`).
    #[inline]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The snake-case name used by data files.
    pub const fn name(self) -> &'static str {
        match self {
            Mirror::None => "none",
            Mirror::LeftRight => "left_right",
            Mirror::FrontBack => "front_back",
        }
    }

    /// Parses a snake-case mirror name.
    pub fn from_name(s: &str) -> Option<Mirror> {
        Mirror::ALL.into_iter().find(|m| m.name() == s)
    }

    /// Mirrors a direction (spec §2 `mirrorDir`): `front_back` swaps east and west, `left_right` swaps north and
    /// south, everything else is unchanged.
    pub const fn mirror_direction(self, d: Direction) -> Direction {
        match (self, d) {
            (Mirror::FrontBack, Direction::East | Direction::West)
            | (Mirror::LeftRight, Direction::North | Direction::South) => d.opposite(),
            _ => d,
        }
    }

    /// The rotation that has the same effect as the mirror on direction `d` (spec §2 `mirrorAsRotation`).
    pub const fn as_rotation(self, d: Direction) -> Rotation {
        match (self, d) {
            (Mirror::FrontBack, Direction::East | Direction::West)
            | (Mirror::LeftRight, Direction::North | Direction::South) => Rotation::Clockwise180,
            _ => Rotation::None,
        }
    }

    /// Mirrors an index `v` in `[0, n)` (spec §2 mirror-an-index): `left_right` gives `n/2 − v`, `front_back`
    /// gives `−v`, both modulo `n`.
    pub const fn mirror_index(self, v: u32, n: u32) -> u32 {
        match self {
            Mirror::None => v,
            Mirror::LeftRight => (n / 2 + n - v % n) % n,
            Mirror::FrontBack => (n - v % n) % n,
        }
    }
}

/// The ten transform rules of spec §6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransformRule {
    /// T0: identity.
    Identity,
    /// T1: `axis` swaps x ↔ z under a quarter turn.
    Axis,
    /// T2: `facing`.
    Facing,
    /// T3: anvil (`facing` rotates, mirrors do nothing).
    Anvil,
    /// T4: door (`facing` and `hinge`).
    Door,
    /// T5: stairs (`facing` and `shape`).
    Stairs,
    /// T6: rails (`shape`).
    Rail,
    /// T7: 16-step `rotation` index.
    RotationIndex,
    /// T8: the four horizontal side properties.
    Sides,
    /// T9: front-and-top `orientation`.
    Orientation,
}

impl TransformRule {
    fn from_digit(d: u8) -> Option<TransformRule> {
        Some(match d {
            0 => TransformRule::Identity,
            1 => TransformRule::Axis,
            2 => TransformRule::Facing,
            3 => TransformRule::Anvil,
            4 => TransformRule::Door,
            5 => TransformRule::Stairs,
            6 => TransformRule::Rail,
            7 => TransformRule::RotationIndex,
            8 => TransformRule::Sides,
            9 => TransformRule::Orientation,
            _ => return None,
        })
    }

    /// The rule number 0–9 (`T0`–`T9`).
    pub const fn number(self) -> usize {
        self as usize
    }

    /// The rule a block (path without namespace, e.g. `oak_stairs`) follows according to the spec appendix.
    pub fn of_block(path: &str) -> Option<TransformRule> {
        rule_map().get(path).copied()
    }
}

/// `block path → rule`, parsed from the appendix of the spec: each group is a bold heading line starting with
/// `**T<n>` followed by a paragraph holding the comma-separated block ids.
fn rule_map() -> &'static HashMap<&'static str, TransformRule> {
    static MAP: OnceLock<HashMap<&'static str, TransformRule>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut map = HashMap::new();
        let appendix = SPEC
            .split_once("### Appendix")
            .map_or("", |(_, rest)| rest)
            .split("## Oracle probe points")
            .next()
            .unwrap_or("");
        let mut current: Option<TransformRule> = None;
        for line in appendix.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(rest) = line.strip_prefix("**T") {
                current = rest
                    .bytes()
                    .next()
                    .and_then(|b| b.is_ascii_digit().then_some(b - b'0'))
                    .and_then(TransformRule::from_digit);
            } else if let Some(rule) = current.take() {
                for name in line.split(',') {
                    map.insert(name.trim(), rule);
                }
            }
        }
        map
    })
}

/// A property of one block: where it sits in the state-id radix and its values.
struct Prop<'a> {
    stride: u32,
    len: u32,
    values: &'a [String],
}

impl Prop<'_> {
    #[inline]
    fn digit(&self, offset: u32) -> u32 {
        offset / self.stride % self.len
    }

    /// The digit map `old digit → new digit` of a function on value names.
    fn map(
        &self,
        block: &str,
        name: &str,
        f: impl Fn(&str) -> Option<String>,
    ) -> Result<Vec<u8>, BuildError> {
        self.values
            .iter()
            .map(|v| {
                let bad = || BuildError::BadValue {
                    block: block.to_owned(),
                    property: name.to_owned(),
                    value: v.clone(),
                };
                let new = f(v).ok_or_else(bad)?;
                let idx = self.values.iter().position(|x| *x == new).ok_or_else(bad)?;
                Ok(idx as u8)
            })
            .collect()
    }
}

/// One change of a block's state offset.
enum Edit<'a> {
    /// A single property whose digit is replaced through a map.
    Map(Prop<'a>, Vec<u8>),
    /// Two properties changed together; the map is indexed by `a * len_b + b` and holds the new `(a, b)`.
    Joint(Prop<'a>, Prop<'a>, Vec<(u8, u8)>),
    /// Four properties permuted: the new value of property `k` is the old value of property `src[k]`.
    Sides([Prop<'a>; 4], [usize; 4]),
}

impl Edit<'_> {
    /// The state offset after the edit; every digit is read from the original `offset`.
    fn apply(&self, offset: u32, new: &mut u32) {
        match self {
            Edit::Map(p, map) => {
                let old = p.digit(offset);
                let nd = u32::from(map[old as usize]);
                *new = *new - old * p.stride + nd * p.stride;
            }
            Edit::Joint(a, b, map) => {
                let (da, db) = (a.digit(offset), b.digit(offset));
                let (na, nb) = map[(da * b.len + db) as usize];
                *new = *new - da * a.stride - db * b.stride
                    + u32::from(na) * a.stride
                    + u32::from(nb) * b.stride;
            }
            Edit::Sides(props, src) => {
                for (k, p) in props.iter().enumerate() {
                    let old = p.digit(offset);
                    let nd = props[src[k]].digit(offset);
                    *new = *new - old * p.stride + nd * p.stride;
                }
            }
        }
    }
}

/// The five primitive operations a table is built from.
#[derive(Clone, Copy)]
enum Op {
    Rot(Rotation),
    Mir(Mirror),
}

/// Rail shapes under the five operations (spec §3): `[old, cw90, cw180, ccw90, left_right, front_back]`.
const RAIL_SHAPES: [[&str; 6]; 10] = [
    [
        "north_south",
        "east_west",
        "north_south",
        "east_west",
        "north_south",
        "north_south",
    ],
    [
        "east_west",
        "north_south",
        "east_west",
        "north_south",
        "east_west",
        "east_west",
    ],
    [
        "ascending_east",
        "ascending_south",
        "ascending_west",
        "ascending_north",
        "ascending_east",
        "ascending_west",
    ],
    [
        "ascending_west",
        "ascending_north",
        "ascending_east",
        "ascending_south",
        "ascending_west",
        "ascending_east",
    ],
    [
        "ascending_north",
        "ascending_east",
        "ascending_south",
        "ascending_west",
        "ascending_south",
        "ascending_north",
    ],
    [
        "ascending_south",
        "ascending_west",
        "ascending_north",
        "ascending_east",
        "ascending_north",
        "ascending_south",
    ],
    [
        "south_east",
        "south_west",
        "north_west",
        "north_east",
        "north_east",
        "south_west",
    ],
    [
        "south_west",
        "north_west",
        "north_east",
        "south_east",
        "north_west",
        "south_east",
    ],
    [
        "north_west",
        "north_east",
        "south_east",
        "south_west",
        "south_west",
        "north_east",
    ],
    [
        "north_east",
        "south_east",
        "south_west",
        "north_west",
        "south_east",
        "north_west",
    ],
];

fn rail_shape(old: &str, op: Op) -> Option<String> {
    let col = match op {
        Op::Rot(Rotation::None) | Op::Mir(Mirror::None) => return Some(old.to_owned()),
        Op::Rot(Rotation::Clockwise90) => 1,
        Op::Rot(Rotation::Clockwise180) => 2,
        Op::Rot(Rotation::CounterClockwise90) => 3,
        Op::Mir(Mirror::LeftRight) => 4,
        Op::Mir(Mirror::FrontBack) => 5,
    };
    RAIL_SHAPES
        .iter()
        .find(|row| row[0] == old)
        .map(|row| row[col].to_owned())
}

/// A direction value name moved by an operation (spec §2).
fn dir_name(old: &str, op: Op) -> Option<String> {
    let d = Direction::from_name(old)?;
    let new = match op {
        Op::Rot(r) => r.rotate_direction(d),
        Op::Mir(m) => m.mirror_direction(d),
    };
    Some(new.name().to_owned())
}

/// A front-and-top orientation value (`front_top`) moved by an operation: both halves move independently.
fn orientation_name(old: &str, op: Op) -> Option<String> {
    let (front, top) = old.split_once('_')?;
    Some(format!("{}_{}", dir_name(front, op)?, dir_name(top, op)?))
}

/// Stair `(facing, shape)` after a mirror (spec §4).
fn stair_mirror(facing: &str, shape: &str, m: Mirror) -> Option<(String, String)> {
    let d = Direction::from_name(facing)?;
    let flips = matches!(
        (m, d.axis()),
        (Mirror::LeftRight, 2) | (Mirror::FrontBack, 0)
    );
    if !flips {
        return Some((facing.to_owned(), shape.to_owned()));
    }
    let new_shape = match (m, shape) {
        (_, "straight") => "straight",
        (_, "outer_left") => "outer_right",
        (_, "outer_right") => "outer_left",
        (Mirror::LeftRight, "inner_left") => "inner_right",
        (Mirror::LeftRight, "inner_right") => "inner_left",
        (_, "inner_left") => "inner_left",
        (_, "inner_right") => "inner_right",
        _ => return None,
    };
    Some((d.opposite().name().to_owned(), new_shape.to_owned()))
}

/// The side permutation of spec §2: `src[k]` is the side (N, E, S, W) whose old value the side `k` receives.
const fn side_sources(op: Op) -> [usize; 4] {
    match op {
        Op::Rot(Rotation::None) | Op::Mir(Mirror::None) => [0, 1, 2, 3],
        Op::Rot(Rotation::Clockwise90) => [3, 0, 1, 2],
        Op::Rot(Rotation::Clockwise180) => [2, 3, 0, 1],
        Op::Rot(Rotation::CounterClockwise90) => [1, 2, 3, 0],
        Op::Mir(Mirror::LeftRight) => [2, 1, 0, 3],
        Op::Mir(Mirror::FrontBack) => [0, 3, 2, 1],
    }
}

struct BlockCtx<'a> {
    entry: &'a BlockEntry,
    name: String,
}

impl<'a> BlockCtx<'a> {
    fn prop(&self, name: &str) -> Result<Prop<'a>, BuildError> {
        let props = &self.entry.properties;
        let pos = props.iter().position(|p| p.name() == name).ok_or_else(|| {
            BuildError::MissingProperty {
                block: self.name.clone(),
                property: name.to_owned(),
            }
        })?;
        let stride = props[pos + 1..]
            .iter()
            .map(|p| p.values().len() as u32)
            .product();
        Ok(Prop {
            stride,
            len: props[pos].values().len() as u32,
            values: props[pos].values(),
        })
    }

    fn map_edit(
        &self,
        name: &str,
        f: impl Fn(&str) -> Option<String>,
    ) -> Result<Edit<'a>, BuildError> {
        let p = self.prop(name)?;
        let map = p.map(&self.name, name, f)?;
        Ok(Edit::Map(p, map))
    }

    /// The edits `rule` makes for `op` (empty when the operation does nothing to this block).
    fn edits(&self, rule: TransformRule, op: Op) -> Result<Vec<Edit<'a>>, BuildError> {
        let mut out = Vec::new();
        match rule {
            TransformRule::Identity => {}
            TransformRule::Axis => {
                if matches!(
                    op,
                    Op::Rot(Rotation::Clockwise90 | Rotation::CounterClockwise90)
                ) {
                    out.push(self.map_edit("axis", |v| {
                        Some(
                            match v {
                                "x" => "z",
                                "z" => "x",
                                other => other,
                            }
                            .to_owned(),
                        )
                    })?);
                }
            }
            TransformRule::Facing => out.push(self.map_edit("facing", |v| dir_name(v, op))?),
            TransformRule::Anvil => {
                if let Op::Rot(_) = op {
                    out.push(self.map_edit("facing", |v| dir_name(v, op))?);
                }
            }
            TransformRule::Door => {
                out.push(self.map_edit("facing", |v| dir_name(v, op))?);
                if let Op::Mir(_) = op {
                    out.push(self.map_edit("hinge", |v| {
                        Some(
                            match v {
                                "left" => "right",
                                "right" => "left",
                                other => other,
                            }
                            .to_owned(),
                        )
                    })?);
                }
            }
            TransformRule::Stairs => match op {
                Op::Rot(_) => out.push(self.map_edit("facing", |v| dir_name(v, op))?),
                Op::Mir(m) => {
                    let (facing, shape) = (self.prop("facing")?, self.prop("shape")?);
                    let mut map = Vec::new();
                    for fv in facing.values {
                        for sv in shape.values {
                            let bad = |value: &str, property: &str| BuildError::BadValue {
                                block: self.name.clone(),
                                property: property.to_owned(),
                                value: value.to_owned(),
                            };
                            let (nf, ns) =
                                stair_mirror(fv, sv, m).ok_or_else(|| bad(sv, "shape"))?;
                            let fi = facing
                                .values
                                .iter()
                                .position(|x| *x == nf)
                                .ok_or_else(|| bad(&nf, "facing"))?;
                            let si = shape
                                .values
                                .iter()
                                .position(|x| *x == ns)
                                .ok_or_else(|| bad(&ns, "shape"))?;
                            map.push((fi as u8, si as u8));
                        }
                    }
                    out.push(Edit::Joint(facing, shape, map));
                }
            },
            TransformRule::Rail => out.push(self.map_edit("shape", |v| rail_shape(v, op))?),
            TransformRule::RotationIndex => out.push(self.map_edit("rotation", |v| {
                let n: u32 = v.parse().ok()?;
                let new = match op {
                    Op::Rot(r) => r.rotate_index(n, 16),
                    Op::Mir(m) => m.mirror_index(n, 16),
                };
                Some(new.to_string())
            })?),
            TransformRule::Sides => {
                let src = side_sources(op);
                if src != [0, 1, 2, 3] {
                    let sides = [
                        self.prop("north")?,
                        self.prop("east")?,
                        self.prop("south")?,
                        self.prop("west")?,
                    ];
                    for p in &sides[1..] {
                        if p.values != sides[0].values {
                            return Err(BuildError::BadValue {
                                block: self.name.clone(),
                                property: "north/east/south/west".to_owned(),
                                value: "differing value sets".to_owned(),
                            });
                        }
                    }
                    out.push(Edit::Sides(sides, src));
                }
            }
            TransformRule::Orientation => {
                out.push(self.map_edit("orientation", |v| orientation_name(v, op))?);
            }
        }
        Ok(out)
    }
}

/// The precomputed rotation and mirror tables of every block state (`docs/specs/blocks/state-transforms.md`).
///
/// Lookups are two flat-array reads. Every result is a state of the same block as the input. A state id outside
/// the table is returned unchanged.
#[derive(Debug, Clone)]
pub struct BlockTransforms {
    len: usize,
    /// `rotations[id * 4 + rotation index]`.
    rotations: Vec<u32>,
    /// `mirrors[id * 3 + mirror index]`.
    mirrors: Vec<u32>,
}

impl BlockTransforms {
    /// Builds the tables for every state of `blocks`.
    ///
    /// # Errors
    /// [`BuildError::UnknownBlock`] when a block has no rule in the spec appendix, and
    /// [`BuildError::MissingProperty`] / [`BuildError::BadValue`] when a block lacks a property its rule rewrites
    /// or holds an unexpected value.
    pub fn build(blocks: &BlockStateTable) -> Result<BlockTransforms, BuildError> {
        let len = blocks.len();
        let mut rotations = vec![0u32; len * 4];
        let mut mirrors = vec![0u32; len * 3];
        for entry in blocks.blocks() {
            let name = entry.id.to_string();
            let path = entry.id.path();
            let rule = (entry.id.namespace() == "minecraft")
                .then(|| TransformRule::of_block(path))
                .flatten()
                .ok_or_else(|| BuildError::UnknownBlock(name.clone()))?;
            let ctx = BlockCtx {
                entry,
                name: name.clone(),
            };
            let ops: Vec<(Op, Vec<Edit<'_>>)> = Rotation::ALL[1..]
                .iter()
                .map(|r| Op::Rot(*r))
                .chain(Mirror::ALL[1..].iter().map(|m| Op::Mir(*m)))
                .map(|op| ctx.edits(rule, op).map(|e| (op, e)))
                .collect::<Result<_, _>>()?;
            for offset in 0..entry.state_count {
                let id = (entry.first_state + offset) as usize;
                rotations[id * 4] = id as u32;
                mirrors[id * 3] = id as u32;
                for (op, edits) in &ops {
                    let mut new = offset;
                    for e in edits {
                        e.apply(offset, &mut new);
                    }
                    let result = entry.first_state + new;
                    match op {
                        Op::Rot(r) => rotations[id * 4 + r.index()] = result,
                        Op::Mir(m) => mirrors[id * 3 + m.index()] = result,
                    }
                }
            }
        }
        Ok(BlockTransforms {
            len,
            rotations,
            mirrors,
        })
    }

    /// Number of states in the table.
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the table has no states.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The state `state_id` rotated by `rotation`.
    #[inline]
    pub fn rotate(&self, state_id: u32, rotation: Rotation) -> u32 {
        self.rotations
            .get(state_id as usize * 4 + rotation.index())
            .copied()
            .unwrap_or(state_id)
    }

    /// The state `state_id` mirrored by `mirror`.
    #[inline]
    pub fn mirror(&self, state_id: u32, mirror: Mirror) -> u32 {
        self.mirrors
            .get(state_id as usize * 3 + mirror.index())
            .copied()
            .unwrap_or(state_id)
    }

    /// The state after template placement settings: mirrored first, then rotated (spec §5).
    #[inline]
    pub fn transform(&self, state_id: u32, mirror: Mirror, rotation: Rotation) -> u32 {
        self.rotate(self.mirror(state_id, mirror), rotation)
    }
}
