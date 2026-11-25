//! The voxel-shape model (`docs/specs/blocks/shapes.md`).
//!
//! A [`VoxelShape`] is a finite union of axis-aligned boxes. The spec describes it as a 3-D grid of on/off cells over
//! three sorted coordinate lists; every output the game exposes (emptiness, bounds, the box list in order, face
//! shapes, all predicates) depends only on the *point set* and the exact boundary coordinates, and the box list is the
//! greedy enumeration of §2.2, which does not depend on how the grid is subdivided. This implementation therefore
//! stores the canonical box list plus the bounds and rebuilds a small grid on demand for boolean operations,
//! rotations and face cuts. Shapes are cheap to clone (`Arc`) and immutable, so identical shapes can be shared.

use std::sync::{Arc, OnceLock};

/// Merge / cube-like / bit-detection tolerance (ε) of the spec.
pub const EPSILON: f64 = 1.0e-7;

/// Probe coordinate used when cutting a face from the positive side of an axis.
pub const FACE_PROBE_POSITIVE: f64 = 0.999_999_9;
/// Probe coordinate used when cutting a face from the negative side of an axis.
pub const FACE_PROBE_NEGATIVE: f64 = 1.0e-7;

/// An axis-aligned box in block-local (or world) coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    /// Minimum corner (x, y, z).
    pub min: [f64; 3],
    /// Maximum corner (x, y, z).
    pub max: [f64; 3],
}

impl Aabb {
    /// The unit cube.
    pub const UNIT: Aabb = Aabb {
        min: [0.0; 3],
        max: [1.0; 3],
    };

    /// Creates a box from its two corners.
    #[inline]
    pub const fn new(min: [f64; 3], max: [f64; 3]) -> Self {
        Self { min, max }
    }

    /// The six values in dump order: `min_x, min_y, min_z, max_x, max_y, max_z`.
    #[inline]
    pub const fn to_array(self) -> [f64; 6] {
        [
            self.min[0],
            self.min[1],
            self.min[2],
            self.max[0],
            self.max[1],
            self.max[2],
        ]
    }

    /// Bit-exact equality (distinguishes `0.0` from `-0.0`).
    pub fn bits_eq(&self, other: &Aabb) -> bool {
        self.to_array()
            .iter()
            .zip(other.to_array())
            .all(|(a, b)| a.to_bits() == b.to_bits())
    }

    /// Whether the open interiors of the two boxes overlap on all three axes.
    pub fn intersects(&self, other: &Aabb) -> bool {
        (0..3).all(|a| self.min[a] < other.max[a] && other.min[a] < self.max[a])
    }
}

/// The six block directions in ordinal order (`DOWN, UP, NORTH, SOUTH, WEST, EAST`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Direction {
    /// −Y.
    Down,
    /// +Y.
    Up,
    /// −Z.
    North,
    /// +Z.
    South,
    /// −X.
    West,
    /// +X.
    East,
}

impl Direction {
    /// All six directions in ordinal order.
    pub const ALL: [Direction; 6] = [
        Direction::Down,
        Direction::Up,
        Direction::North,
        Direction::South,
        Direction::West,
        Direction::East,
    ];
    /// The four horizontal directions in clockwise order seen from above.
    pub const HORIZONTAL: [Direction; 4] = [
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
    ];

    /// Ordinal 0..6.
    #[inline]
    pub const fn ordinal(self) -> usize {
        self as usize
    }

    /// The axis index (X = 0, Y = 1, Z = 2).
    #[inline]
    pub const fn axis(self) -> usize {
        match self {
            Direction::West | Direction::East => 0,
            Direction::Down | Direction::Up => 1,
            Direction::North | Direction::South => 2,
        }
    }

    /// Whether the direction points to increasing coordinates.
    #[inline]
    pub const fn is_positive(self) -> bool {
        matches!(self, Direction::Up | Direction::South | Direction::East)
    }

    /// The opposite direction.
    pub const fn opposite(self) -> Direction {
        match self {
            Direction::Down => Direction::Up,
            Direction::Up => Direction::Down,
            Direction::North => Direction::South,
            Direction::South => Direction::North,
            Direction::West => Direction::East,
            Direction::East => Direction::West,
        }
    }

    /// Next horizontal direction clockwise seen from above (N → E → S → W).
    pub const fn clockwise(self) -> Direction {
        match self {
            Direction::North => Direction::East,
            Direction::East => Direction::South,
            Direction::South => Direction::West,
            Direction::West => Direction::North,
            other => other,
        }
    }

    /// Next horizontal direction counter-clockwise seen from above (N → W → S → E).
    pub const fn counter_clockwise(self) -> Direction {
        match self {
            Direction::North => Direction::West,
            Direction::West => Direction::South,
            Direction::South => Direction::East,
            Direction::East => Direction::North,
            other => other,
        }
    }

    /// Lower-case name as used by block-state property values.
    pub const fn name(self) -> &'static str {
        match self {
            Direction::Down => "down",
            Direction::Up => "up",
            Direction::North => "north",
            Direction::South => "south",
            Direction::West => "west",
            Direction::East => "east",
        }
    }

    /// Parses a lower-case direction name.
    pub fn from_name(s: &str) -> Option<Direction> {
        Direction::ALL.into_iter().find(|d| d.name() == s)
    }
}

/// Two-input boolean operation of the spec §2.3 (those used by block shapes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoolOp {
    /// `a ∨ b`.
    Or,
    /// `a ∧ b`.
    And,
    /// `a ∧ ¬b`.
    OnlyFirst,
    /// `¬a ∧ b`.
    OnlySecond,
    /// `a ≠ b`.
    NotSame,
}

impl BoolOp {
    /// Evaluates the operation.
    #[inline]
    pub const fn apply(self, a: bool, b: bool) -> bool {
        match self {
            BoolOp::Or => a || b,
            BoolOp::And => a && b,
            BoolOp::OnlyFirst => a && !b,
            BoolOp::OnlySecond => !a && b,
            BoolOp::NotSame => a != b,
        }
    }
}

/// How a face is tested for sturdiness (`SupportType` of the game).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SupportType {
    /// The face must be completely covered.
    Full,
    /// The 2×2-pixel centre (see shapes.md §4.3) must be covered.
    Center,
    /// The 2-pixel border ring must be covered.
    Rigid,
}

impl SupportType {
    /// All three support types in dump order.
    pub const ALL: [SupportType; 3] = [SupportType::Full, SupportType::Center, SupportType::Rigid];
}

/// A signed axis permutation: output axis `i` is input axis `perm[i]`, mirrored (`1 − v`) when `flip[i]`.
///
/// These are the cube symmetries used by block shapes (shapes.md §3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transform {
    perm: [usize; 3],
    flip: [bool; 3],
}

impl Transform {
    /// The identity.
    pub const IDENTITY: Transform = Transform {
        perm: [0, 1, 2],
        flip: [false; 3],
    };
    /// North → east about the vertical axis: `(x, y, z) ↦ (1 − z, y, x)`.
    pub const Y90: Transform = Transform {
        perm: [2, 1, 0],
        flip: [true, false, false],
    };
    /// North → south: `(1 − x, y, 1 − z)`.
    pub const Y180: Transform = Transform {
        perm: [0, 1, 2],
        flip: [true, false, true],
    };
    /// North → west: `(z, y, 1 − x)`.
    pub const Y270: Transform = Transform {
        perm: [2, 1, 0],
        flip: [false, false, true],
    };
    /// North → down: `(x, z, 1 − y)`.
    pub const X90: Transform = Transform {
        perm: [0, 2, 1],
        flip: [false, false, true],
    };
    /// North → up: `(x, 1 − z, y)`.
    pub const X270: Transform = Transform {
        perm: [0, 2, 1],
        flip: [false, true, false],
    };
    /// Mirror top and bottom: `(x, 1 − y, z)`.
    pub const INVERT_Y: Transform = Transform {
        perm: [0, 1, 2],
        flip: [false, true, false],
    };
    /// Mirror left and right: `(1 − x, y, z)`.
    pub const INVERT_X: Transform = Transform {
        perm: [0, 1, 2],
        flip: [true, false, false],
    };

    /// The horizontal rotation taking a NORTH-modelled shape to `facing` (identity for NORTH and non-horizontal).
    pub const fn horizontal(facing: Direction) -> Transform {
        match facing {
            Direction::East => Transform::Y90,
            Direction::South => Transform::Y180,
            Direction::West => Transform::Y270,
            _ => Transform::IDENTITY,
        }
    }

    /// `self` applied after `first` ("Q after P" of the spec).
    pub fn after(self, first: Transform) -> Transform {
        let mut perm = [0; 3];
        let mut flip = [false; 3];
        for i in 0..3 {
            perm[i] = first.perm[self.perm[i]];
            flip[i] = self.flip[i] ^ first.flip[self.perm[i]];
        }
        Transform { perm, flip }
    }

    /// Whether this is the identity.
    pub fn is_identity(self) -> bool {
        self == Transform::IDENTITY
    }

    /// Rotation taking a shape modelled for NORTH to `d` the way the `all` rotation family does (UP = X270, DOWN =
    /// X90, the four horizontals as [`Transform::horizontal`]).
    pub const fn all(d: Direction) -> Transform {
        match d {
            Direction::Up => Transform::X270,
            Direction::Down => Transform::X90,
            other => Transform::horizontal(other),
        }
    }
}

#[derive(Debug)]
struct Data {
    boxes: Box<[Aabb]>,
    bounds: Aabb,
}

/// An immutable, cheaply clonable voxel shape.
#[derive(Debug, Clone)]
pub struct VoxelShape(Arc<Data>);

fn empty_arc() -> &'static VoxelShape {
    static EMPTY: OnceLock<VoxelShape> = OnceLock::new();
    EMPTY.get_or_init(|| {
        VoxelShape(Arc::new(Data {
            boxes: Box::new([]),
            bounds: Aabb::new([0.0; 3], [0.0; 3]),
        }))
    })
}

fn block_arc() -> &'static VoxelShape {
    static BLOCK: OnceLock<VoxelShape> = OnceLock::new();
    BLOCK.get_or_init(|| {
        VoxelShape(Arc::new(Data {
            boxes: Box::new([Aabb::UNIT]),
            bounds: Aabb::UNIT,
        }))
    })
}

impl PartialEq for VoxelShape {
    /// Bit-exact equality of the box list and the bounds.
    fn eq(&self, other: &Self) -> bool {
        self.same_as(other)
    }
}

impl VoxelShape {
    /// The empty shape.
    pub fn empty() -> VoxelShape {
        empty_arc().clone()
    }

    /// The unit cube.
    pub fn block() -> VoxelShape {
        block_arc().clone()
    }

    fn from_enumerated(boxes: Vec<Aabb>) -> VoxelShape {
        if boxes.is_empty() {
            return VoxelShape::empty();
        }
        let bounds = hull(&boxes);
        if boxes.len() == 1 && boxes[0].bits_eq(&Aabb::UNIT) {
            return VoxelShape::block();
        }
        VoxelShape(Arc::new(Data {
            boxes: boxes.into_boxed_slice(),
            bounds,
        }))
    }

    /// `Shapes.box` of the spec (§1.1): EMPTY when thinner than ε on some axis, BLOCK for the unit cube.
    pub fn from_box(min: [f64; 3], max: [f64; 3]) -> VoxelShape {
        if (0..3).any(|a| max[a] - min[a] < EPSILON) {
            return VoxelShape::empty();
        }
        // A dyadic axis (grid exponent ≤ 3) snaps its coordinates onto the regular list; other values are kept.
        let snapped = |a: usize| -> Option<(f64, f64)> {
            if min[a] < -EPSILON || max[a] > 1.0 + EPSILON {
                return None;
            }
            for b in 0..=3u32 {
                let n = f64::from(1u32 << b);
                let near = |v: f64| {
                    let r = (v * n + 0.5).floor();
                    (v * n - r).abs() <= EPSILON * n
                };
                if near(min[a]) && near(max[a]) {
                    let r = |v: f64| (v * n + 0.5).floor() / n;
                    return Some((r(min[a]), r(max[a])));
                }
            }
            None
        };
        let (mut lo, mut hi) = (min, max);
        let mut all = true;
        let mut snap = [(0.0, 0.0); 3];
        for (a, s) in snap.iter_mut().enumerate() {
            match snapped(a) {
                Some(v) => *s = v,
                None => all = false,
            }
        }
        // Only when every axis snaps does the box become a regular grid (and take the snapped values).
        if all {
            for (a, s) in snap.iter().enumerate() {
                lo[a] = s.0;
                hi[a] = s.1;
            }
            if lo == [0.0; 3] && hi == [1.0; 3] {
                return VoxelShape::block();
            }
        }
        let aabb = Aabb::new(lo, hi);
        VoxelShape(Arc::new(Data {
            boxes: Box::new([aabb]),
            bounds: aabb,
        }))
    }

    /// `Block.box` of the spec: coordinates in pixels (1/16 block).
    pub fn px(x1: f64, y1: f64, z1: f64, x2: f64, y2: f64, z2: f64) -> VoxelShape {
        VoxelShape::from_box(
            [x1 / 16.0, y1 / 16.0, z1 / 16.0],
            [x2 / 16.0, y2 / 16.0, z2 / 16.0],
        )
    }

    /// `Block.column(w, y1, y2)` (centred square column).
    pub fn column(w: f64, y1: f64, y2: f64) -> VoxelShape {
        VoxelShape::column2(w, w, y1, y2)
    }

    /// `Block.column(wx, wz, y1, y2)`.
    pub fn column2(wx: f64, wz: f64, y1: f64, y2: f64) -> VoxelShape {
        VoxelShape::px(
            8.0 - wx / 2.0,
            y1,
            8.0 - wz / 2.0,
            8.0 + wx / 2.0,
            y2,
            8.0 + wz / 2.0,
        )
    }

    /// `Block.cube(s)`.
    pub fn cube(s: f64) -> VoxelShape {
        VoxelShape::cube3(s, s, s)
    }

    /// `Block.cube(sx, sy, sz)`.
    pub fn cube3(sx: f64, sy: f64, sz: f64) -> VoxelShape {
        VoxelShape::column2(sx, sz, 8.0 - sy / 2.0, 8.0 + sy / 2.0)
    }

    /// Union of several shapes (`Shapes.or`): a left fold of [`VoxelShape::or`].
    pub fn union_all<I: IntoIterator<Item = VoxelShape>>(shapes: I) -> VoxelShape {
        let mut it = shapes.into_iter();
        let Some(mut acc) = it.next() else {
            return VoxelShape::empty();
        };
        for s in it {
            acc = acc.or(&s);
        }
        acc
    }

    /// `true` when the shape contains no point.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.boxes.is_empty()
    }

    /// The boxes in the canonical (greedy) enumeration order of the spec §2.2.
    #[inline]
    pub fn boxes(&self) -> &[Aabb] {
        &self.0.boxes
    }

    /// The bounds (`None` for an empty shape). For face slices these are the source shape's in-plane bounds
    /// (the "slice-bounds quirk", shapes.md §4.2) and may be larger than the hull of [`VoxelShape::boxes`].
    #[inline]
    pub fn bounds(&self) -> Option<Aabb> {
        (!self.is_empty()).then_some(self.0.bounds)
    }

    /// Smallest coordinate on `axis` (`+∞` when empty).
    pub fn min(&self, axis: usize) -> f64 {
        self.bounds().map_or(f64::INFINITY, |b| b.min[axis])
    }

    /// Largest coordinate on `axis` (`−∞` when empty).
    pub fn max(&self, axis: usize) -> f64 {
        self.bounds().map_or(f64::NEG_INFINITY, |b| b.max[axis])
    }

    /// Whether this is exactly the unit cube.
    pub fn is_block(&self) -> bool {
        self.0.boxes.len() == 1 && self.0.boxes[0].bits_eq(&Aabb::UNIT)
    }

    /// Whether the two shapes have identical box lists and bounds, bit for bit.
    pub fn same_as(&self, other: &VoxelShape) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
            || (self.0.boxes.len() == other.0.boxes.len()
                && self
                    .0
                    .boxes
                    .iter()
                    .zip(other.0.boxes.iter())
                    .all(|(a, b)| a.bits_eq(b))
                && (self.is_empty() || self.0.bounds.bits_eq(&other.0.bounds)))
    }

    /// `Shapes.join(a, b, op)`: the combined shape, simplified to the canonical box list.
    pub fn join(&self, other: &VoxelShape, op: BoolOp) -> VoxelShape {
        if Arc::ptr_eq(&self.0, &other.0) {
            return if op.apply(true, true) {
                self.clone()
            } else {
                VoxelShape::empty()
            };
        }
        let keep_a = op.apply(true, false);
        let keep_b = op.apply(false, true);
        if self.is_empty() {
            return if keep_b {
                other.clone()
            } else {
                VoxelShape::empty()
            };
        }
        if other.is_empty() {
            return if keep_a {
                self.clone()
            } else {
                VoxelShape::empty()
            };
        }
        let boxes = combine(&self.0.boxes, &other.0.boxes, |a, b| op.apply(a, b));
        VoxelShape::from_enumerated(boxes)
    }

    /// Union (`Shapes.or(a, b)`).
    pub fn or(&self, other: &VoxelShape) -> VoxelShape {
        self.join(other, BoolOp::Or)
    }

    /// `self` minus `other` (`ONLY_FIRST`).
    pub fn subtract(&self, other: &VoxelShape) -> VoxelShape {
        self.join(other, BoolOp::OnlyFirst)
    }

    /// Intersection.
    pub fn and(&self, other: &VoxelShape) -> VoxelShape {
        self.join(other, BoolOp::And)
    }

    /// `Shapes.joinIsNotEmpty`: does `op(self, other)` contain any point?
    pub fn join_is_not_empty(&self, other: &VoxelShape, op: BoolOp) -> bool {
        if self.is_empty() || other.is_empty() {
            return op.apply(!self.is_empty(), !other.is_empty());
        }
        if Arc::ptr_eq(&self.0, &other.0) {
            return op.apply(true, true);
        }
        let (a, b) = (&self.0.bounds, &other.0.bounds);
        for axis in 0..3 {
            if a.max[axis] < b.min[axis] - EPSILON || b.max[axis] < a.min[axis] - EPSILON {
                return op.apply(true, false) || op.apply(false, true);
            }
        }
        any_cell(&self.0.boxes, &other.0.boxes, |x, y| op.apply(x, y))
    }

    /// Whether both shapes describe the same point set (`Shapes.equal`).
    pub fn equals_shape(&self, other: &VoxelShape) -> bool {
        !self.join_is_not_empty(other, BoolOp::NotSame)
    }

    /// `Block.isShapeFullBlock`: the point set equals the unit cube.
    pub fn is_full_block(&self) -> bool {
        self.is_block() || !VoxelShape::block().join_is_not_empty(self, BoolOp::NotSame)
    }

    /// `VoxelShape.move`: every coordinate gets one double addition.
    pub fn translate(&self, dx: f64, dy: f64, dz: f64) -> VoxelShape {
        if self.is_empty() {
            return self.clone();
        }
        let d = [dx, dy, dz];
        let mv = |b: &Aabb| {
            Aabb::new(
                [b.min[0] + d[0], b.min[1] + d[1], b.min[2] + d[2]],
                [b.max[0] + d[0], b.max[1] + d[1], b.max[2] + d[2]],
            )
        };
        VoxelShape(Arc::new(Data {
            boxes: self.0.boxes.iter().map(mv).collect(),
            bounds: mv(&self.0.bounds),
        }))
    }

    /// Rotates / mirrors the shape about the block centre (shapes.md §3.2). The boxes are re-enumerated on the
    /// rotated grid, which is why a rotated shape's box order can differ from the rotated box list.
    pub fn rotate(&self, t: Transform) -> VoxelShape {
        if t.is_identity() || self.is_empty() {
            return self.clone();
        }
        // Each flipped value is computed as -(v - 0.5) + 0.5 (subtract, negate, add), exactly as the spec says.
        let flip = |v: f64| -(v - 0.5) + 0.5;
        let apply = |b: &Aabb| {
            let mut out = Aabb::new([0.0; 3], [0.0; 3]);
            for i in 0..3 {
                let src = t.perm[i];
                if t.flip[i] {
                    out.min[i] = flip(b.max[src]);
                    out.max[i] = flip(b.min[src]);
                } else {
                    out.min[i] = b.min[src];
                    out.max[i] = b.max[src];
                }
            }
            out
        };
        let rotated: Vec<Aabb> = self.0.boxes.iter().map(apply).collect();
        let boxes = normalize(&rotated);
        let bounds = apply(&self.0.bounds);
        VoxelShape(Arc::new(Data {
            boxes: boxes.into_boxed_slice(),
            bounds,
        }))
    }

    /// `VoxelShape.getFaceShape(d)` (shapes.md §4): the cross-section at the face, extruded over the full depth.
    pub fn face_shape(&self, d: Direction) -> VoxelShape {
        if self.is_empty() || self.is_block() {
            return self.clone();
        }
        let a = d.axis();
        let boxes = &self.0.boxes;
        // "Cube-like along the axis": every cross-section is the same and spans [0, 1].
        let spans_unit = (self.0.bounds.min[a] - 0.0).abs() < EPSILON
            && (self.0.bounds.max[a] - 1.0).abs() < EPSILON;
        if spans_unit && invariant_along(boxes, a) {
            return self.clone();
        }
        let probe = if d.is_positive() {
            FACE_PROBE_POSITIVE
        } else {
            FACE_PROBE_NEGATIVE
        };
        let layer: Vec<Aabb> = boxes
            .iter()
            .filter(|b| b.min[a] <= probe && probe < b.max[a])
            .map(|b| {
                let mut s = *b;
                s.min[a] = 0.0;
                s.max[a] = 1.0;
                s
            })
            .collect();
        if layer.is_empty() {
            return VoxelShape::empty();
        }
        let slice = normalize(&layer);
        let mut bounds = self.0.bounds;
        bounds.min[a] = 0.0;
        bounds.max[a] = 1.0;
        if slice.len() == 1 && slice[0].bits_eq(&Aabb::UNIT) && bounds.bits_eq(&Aabb::UNIT) {
            return VoxelShape::block();
        }
        VoxelShape(Arc::new(Data {
            boxes: slice.into_boxed_slice(),
            bounds,
        }))
    }

    /// `Block.isFaceFull(shape, d)`.
    pub fn is_face_full(&self, d: Direction) -> bool {
        self.face_shape(d).is_full_block()
    }

    /// `isFaceSturdy` on this shape (the state's block-support shape), shapes.md §4.3.
    pub fn is_face_sturdy(&self, d: Direction, ty: SupportType) -> bool {
        if self.is_empty() {
            return false;
        }
        let face = self.face_shape(d);
        match ty {
            SupportType::Full => face.is_full_block(),
            SupportType::Center => !face.join_is_not_empty(center_template(), BoolOp::OnlySecond),
            SupportType::Rigid => !face.join_is_not_empty(rigid_template(), BoolOp::OnlySecond),
        }
    }

    /// Whether the given world-local box strictly overlaps any box of this shape.
    pub fn intersects(&self, query: &Aabb) -> bool {
        self.0.boxes.iter().any(|b| b.intersects(query))
    }

    /// Whether the point lies inside any box (`min <= p < max` on every axis).
    pub fn contains_point(&self, p: [f64; 3]) -> bool {
        self.0
            .boxes
            .iter()
            .any(|b| (0..3).all(|a| b.min[a] <= p[a] && p[a] < b.max[a]))
    }
}

fn center_template() -> &'static VoxelShape {
    static T: OnceLock<VoxelShape> = OnceLock::new();
    T.get_or_init(|| VoxelShape::column(2.0, 0.0, 10.0))
}

fn rigid_template() -> &'static VoxelShape {
    static T: OnceLock<VoxelShape> = OnceLock::new();
    T.get_or_init(|| VoxelShape::block().subtract(&VoxelShape::column(12.0, 0.0, 16.0)))
}

fn hull(boxes: &[Aabb]) -> Aabb {
    let mut h = boxes[0];
    for b in &boxes[1..] {
        for a in 0..3 {
            h.min[a] = h.min[a].min(b.min[a]);
            h.max[a] = h.max[a].max(b.max[a]);
        }
    }
    h
}

/// Every cross-section along `axis` is identical (all boxes span the same `axis` interval).
fn invariant_along(boxes: &[Aabb], axis: usize) -> bool {
    let first = boxes[0];
    boxes
        .iter()
        .all(|b| b.min[axis] == first.min[axis] && b.max[axis] == first.max[axis])
}

// ── Grid rasterisation ───────────────────────────────────────────────────────────────────────────────────────────

struct Grid {
    coords: [Vec<f64>; 3],
}

impl Grid {
    fn new(sets: &[&[Aabb]]) -> Grid {
        let mut coords: [Vec<f64>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        for set in sets {
            for b in *set {
                for (axis, list) in coords.iter_mut().enumerate() {
                    list.push(b.min[axis]);
                    list.push(b.max[axis]);
                }
            }
        }
        for list in &mut coords {
            list.sort_by(f64::total_cmp);
            // Keep the first of values that lie within ε of each other (the "indirect merger" rule).
            let mut out: Vec<f64> = Vec::with_capacity(list.len());
            for &v in list.iter() {
                if out.last().is_none_or(|&last| last < v - EPSILON) {
                    out.push(v);
                }
            }
            *list = out;
        }
        Grid { coords }
    }

    #[inline]
    fn dims(&self) -> [usize; 3] {
        [
            self.coords[0].len().saturating_sub(1),
            self.coords[1].len().saturating_sub(1),
            self.coords[2].len().saturating_sub(1),
        ]
    }

    /// Cell index of the first coordinate ≥ `v − ε`.
    fn index_of(&self, axis: usize, v: f64) -> usize {
        self.coords[axis].partition_point(|&c| c < v - EPSILON)
    }

    /// Fills a flat array (index `(j * nx + i) * nz + k`) with the cells covered by `boxes`.
    fn fill(&self, boxes: &[Aabb]) -> Vec<bool> {
        let [nx, ny, nz] = self.dims();
        let mut cells = vec![false; nx * ny * nz];
        for b in boxes {
            let lo = [
                self.index_of(0, b.min[0]),
                self.index_of(1, b.min[1]),
                self.index_of(2, b.min[2]),
            ];
            let hi = [
                self.index_of(0, b.max[0]),
                self.index_of(1, b.max[1]),
                self.index_of(2, b.max[2]),
            ];
            for j in lo[1]..hi[1].min(ny) {
                for i in lo[0]..hi[0].min(nx) {
                    let base = (j * nx + i) * nz;
                    for cell in &mut cells[base + lo[2]..base + hi[2].min(nz)] {
                        *cell = true;
                    }
                }
            }
        }
        cells
    }

    /// The greedy box enumeration of shapes.md §2.2.
    fn enumerate(&self, mut cells: Vec<bool>) -> Vec<Aabb> {
        let [nx, ny, nz] = self.dims();
        let idx = |i: usize, j: usize, k: usize| (j * nx + i) * nz + k;
        let (xs, ys, zs) = (&self.coords[0], &self.coords[1], &self.coords[2]);
        let mut out = Vec::new();
        for j in 0..ny {
            for i in 0..nx {
                let mut run: Option<usize> = None;
                for k in 0..=nz {
                    let full = k < nz && cells[idx(i, j, k)];
                    if full {
                        if run.is_none() {
                            run = Some(k);
                        }
                        continue;
                    }
                    let Some(k0) = run.take() else { continue };
                    for kk in k0..k {
                        cells[idx(i, j, kk)] = false;
                    }
                    let mut i1 = i;
                    while i1 + 1 < nx && (k0..k).all(|kk| cells[idx(i1 + 1, j, kk)]) {
                        i1 += 1;
                        for kk in k0..k {
                            cells[idx(i1, j, kk)] = false;
                        }
                    }
                    let mut j1 = j;
                    while j1 + 1 < ny
                        && (i..=i1).all(|ii| (k0..k).all(|kk| cells[idx(ii, j1 + 1, kk)]))
                    {
                        j1 += 1;
                        for ii in i..=i1 {
                            for kk in k0..k {
                                cells[idx(ii, j1, kk)] = false;
                            }
                        }
                    }
                    out.push(Aabb::new(
                        [xs[i], ys[j], zs[k0]],
                        [xs[i1 + 1], ys[j1 + 1], zs[k]],
                    ));
                }
            }
        }
        out
    }
}

/// Canonical box list of the union of `boxes`.
fn normalize(boxes: &[Aabb]) -> Vec<Aabb> {
    let grid = Grid::new(&[boxes]);
    let cells = grid.fill(boxes);
    grid.enumerate(cells)
}

/// Canonical box list of `op(a, b)`.
fn combine(a: &[Aabb], b: &[Aabb], op: impl Fn(bool, bool) -> bool) -> Vec<Aabb> {
    let grid = Grid::new(&[a, b]);
    let (ca, cb) = (grid.fill(a), grid.fill(b));
    let cells: Vec<bool> = ca.iter().zip(&cb).map(|(&x, &y)| op(x, y)).collect();
    grid.enumerate(cells)
}

fn any_cell(a: &[Aabb], b: &[Aabb], op: impl Fn(bool, bool) -> bool) -> bool {
    let grid = Grid::new(&[a, b]);
    let (ca, cb) = (grid.fill(a), grid.fill(b));
    ca.iter().zip(&cb).any(|(&x, &y)| op(x, y))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxes_px(s: &VoxelShape) -> Vec<[f64; 6]> {
        s.boxes()
            .iter()
            .map(|b| b.to_array().map(|v| v * 16.0))
            .collect()
    }

    #[test]
    fn box_snaps_and_singletons() {
        assert!(VoxelShape::from_box([0.0; 3], [1.0; 3]).is_block());
        assert!(VoxelShape::px(0.0, 0.0, 0.0, 16.0, 0.0, 16.0).is_empty());
        assert!(
            VoxelShape::px(0.0, 0.0, 0.0, 16.0, 8.0, 16.0)
                .bounds()
                .is_some()
        );
        assert!(VoxelShape::empty().bounds().is_none());
    }

    #[test]
    fn stairs_enumeration_order() {
        // Worked example of shapes.md §2.2: bottom stair facing north.
        let slab = VoxelShape::px(0.0, 0.0, 0.0, 16.0, 8.0, 16.0);
        let stair = slab.or(&VoxelShape::px(0.0, 8.0, 8.0, 16.0, 16.0, 16.0));
        // The north-facing stair has its high half at the back (z 8..16).
        let n = stair.rotate(Transform::IDENTITY);
        assert_eq!(
            boxes_px(&n),
            vec![
                [0.0, 0.0, 0.0, 16.0, 8.0, 16.0],
                [0.0, 8.0, 8.0, 16.0, 16.0, 16.0]
            ]
        );
        // Upside-down: first box merges the lower half-row with the slab vertically.
        let top = VoxelShape::px(0.0, 8.0, 0.0, 16.0, 16.0, 16.0)
            .or(&VoxelShape::px(0.0, 0.0, 8.0, 16.0, 8.0, 16.0));
        assert_eq!(
            boxes_px(&top),
            vec![
                [0.0, 0.0, 8.0, 16.0, 16.0, 16.0],
                [0.0, 8.0, 0.0, 16.0, 16.0, 8.0]
            ]
        );
        let north_top = VoxelShape::px(0.0, 8.0, 0.0, 16.0, 16.0, 16.0)
            .or(&VoxelShape::px(0.0, 0.0, 0.0, 16.0, 8.0, 8.0));
        assert_eq!(
            boxes_px(&north_top),
            vec![
                [0.0, 0.0, 0.0, 16.0, 16.0, 8.0],
                [0.0, 8.0, 8.0, 16.0, 16.0, 16.0]
            ]
        );
    }

    #[test]
    fn rotation_roundtrip() {
        let s = VoxelShape::px(0.0, 0.0, 0.0, 4.0, 8.0, 16.0);
        let r = s.rotate(Transform::Y90).rotate(Transform::Y270);
        assert!(r.same_as(&s));
        let e = s.rotate(Transform::Y90);
        assert_eq!(boxes_px(&e), vec![[0.0, 0.0, 0.0, 16.0, 8.0, 4.0]]);
        // North → up and down.
        let n = VoxelShape::px(6.0, 6.0, 0.0, 10.0, 10.0, 4.0);
        assert_eq!(
            boxes_px(&n.rotate(Transform::X270)),
            vec![[6.0, 12.0, 6.0, 10.0, 16.0, 10.0]]
        );
        assert_eq!(
            boxes_px(&n.rotate(Transform::X90)),
            vec![[6.0, 0.0, 6.0, 10.0, 4.0, 10.0]]
        );
    }

    #[test]
    fn face_shapes_and_sturdiness() {
        let slab = VoxelShape::px(0.0, 0.0, 0.0, 16.0, 8.0, 16.0);
        assert!(slab.is_face_full(Direction::Down));
        assert!(!slab.is_face_full(Direction::Up));
        assert!(
            slab.is_face_sturdy(Direction::Up, SupportType::Rigid)
                || !slab.is_face_full(Direction::Up)
        );
        // Horizontal faces of a bottom slab are the slab itself.
        let north = slab.face_shape(Direction::North);
        assert!(north.same_as(&slab));
        // Slice-bounds quirk: the UP face of a north-facing bottom stair.
        let stair = VoxelShape::px(0.0, 0.0, 0.0, 16.0, 8.0, 16.0)
            .or(&VoxelShape::px(0.0, 8.0, 8.0, 16.0, 16.0, 16.0));
        let up = stair.face_shape(Direction::Up);
        assert_eq!(boxes_px(&up), vec![[0.0, 0.0, 8.0, 16.0, 16.0, 16.0]]);
        assert_eq!(up.bounds().unwrap(), Aabb::UNIT);
        // A centre column supports CENTER but not FULL or RIGID.
        let post = VoxelShape::px(6.0, 0.0, 6.0, 10.0, 16.0, 10.0);
        assert!(post.is_face_sturdy(Direction::Up, SupportType::Center));
        assert!(!post.is_face_sturdy(Direction::Up, SupportType::Full));
        assert!(!post.is_face_sturdy(Direction::Up, SupportType::Rigid));
        assert!(!VoxelShape::empty().is_face_sturdy(Direction::Up, SupportType::Center));
        assert!(VoxelShape::block().is_face_sturdy(Direction::Down, SupportType::Rigid));
    }

    #[test]
    fn join_ops_and_translate() {
        let a = VoxelShape::px(0.0, 0.0, 0.0, 16.0, 16.0, 16.0);
        let hole = VoxelShape::px(4.0, 0.0, 4.0, 12.0, 16.0, 12.0);
        let ring = a.subtract(&hole);
        assert!(!ring.is_empty());
        assert!(ring.or(&hole).is_full_block());
        let moved = hole.translate(0.25, 0.0, -0.25);
        assert_eq!(moved.bounds().unwrap().min, [0.25 + 0.25, 0.0, 0.25 - 0.25]);
        assert!(
            hole.and(&VoxelShape::px(0.0, 0.0, 0.0, 4.0, 4.0, 4.0))
                .is_empty()
        );
    }
}
