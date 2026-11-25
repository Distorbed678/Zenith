//! Geometry, direction, rotation, mirror, bounding-box, and `WorldgenRandom` helpers
//! (`docs/specs/worldgen/structures-placement-and-jigsaw.md` "Inputs and state",
//! `docs/specs/worldgen/structures-legacy-pieces.md` §1.1 & §1.7).

use std::cmp::Ordering;

use zenith_blocks::Aabb;
pub use zenith_blocks::Direction;
use zenith_math::RandomSource;
use zenith_math::helpers::packing::block_as_long;
use zenith_math::helpers::rounding::java_floor_mod_i32;
pub use zenith_worldgen::BoundingBox;
use zenith_worldgen::{HeightProvider, WorldGenerationContext};

/// Integer 3D block position (`BlockPos`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct BlockPos {
    /// Block X coordinate.
    pub x: i32,
    /// Block Y coordinate.
    pub y: i32,
    /// Block Z coordinate.
    pub z: i32,
}

impl BlockPos {
    /// Origin `(0, 0, 0)`.
    pub const ZERO: Self = Self { x: 0, y: 0, z: 0 };

    /// Constructs a `BlockPos(x, y, z)`.
    #[inline]
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// Returns `self + (dx, dy, dz)` (wrapping `i32`).
    #[inline]
    pub const fn offset(self, dx: i32, dy: i32, dz: i32) -> Self {
        Self {
            x: self.x.wrapping_add(dx),
            y: self.y.wrapping_add(dy),
            z: self.z.wrapping_add(dz),
        }
    }

    /// Returns `self + other` (wrapping `i32`).
    #[inline]
    pub const fn add(self, other: Self) -> Self {
        self.offset(other.x, other.y, other.z)
    }

    /// Returns `self - other` (wrapping `i32`).
    #[inline]
    pub const fn sub(self, other: Self) -> Self {
        Self {
            x: self.x.wrapping_sub(other.x),
            y: self.y.wrapping_sub(other.y),
            z: self.z.wrapping_sub(other.z),
        }
    }

    /// Returns `self + (0, dy, 0)`.
    #[inline]
    pub const fn above(self, dy: i32) -> Self {
        self.offset(0, dy, 0)
    }

    /// Returns `self - (0, dy, 0)`.
    #[inline]
    pub const fn below(self, dy: i32) -> Self {
        self.offset(0, dy.wrapping_neg(), 0)
    }

    /// Returns `self` shifted by `dist` blocks in `dir`.
    #[inline]
    pub const fn relative(self, dir: Direction, dist: i32) -> Self {
        let (dx, dy, dz) = dir_step(dir);
        self.offset(
            dx.wrapping_mul(dist),
            dy.wrapping_mul(dist),
            dz.wrapping_mul(dist),
        )
    }

    /// Manhattan distance `|dx| + |dy| + |dz|`.
    #[inline]
    pub const fn dist_manhattan(self, other: Self) -> i32 {
        (self.x.wrapping_sub(other.x))
            .abs()
            .wrapping_add((self.y.wrapping_sub(other.y)).abs())
            .wrapping_add((self.z.wrapping_sub(other.z)).abs())
    }

    /// `BlockPos.asLong(x, y, z)`.
    #[inline]
    pub const fn as_long(self) -> i64 {
        block_as_long(self.x, self.y, self.z)
    }

    /// `Vec3i.compareTo`: Y ascending, then Z ascending, then X ascending.
    #[inline]
    pub fn cmp_vec3i(self, other: Self) -> Ordering {
        self.y
            .cmp(&other.y)
            .then_with(|| self.z.cmp(&other.z))
            .then_with(|| self.x.cmp(&other.x))
    }
}

/// Unit step `(dx, dy, dz)` for a [`Direction`].
#[inline]
pub const fn dir_step(dir: Direction) -> (i32, i32, i32) {
    match dir {
        Direction::Down => (0, -1, 0),
        Direction::Up => (0, 1, 0),
        Direction::North => (0, 0, -1),
        Direction::South => (0, 0, 1),
        Direction::West => (-1, 0, 0),
        Direction::East => (1, 0, 0),
    }
}

/// `Direction.get2DDataValue()`: `SOUTH=0, WEST=1, NORTH=2, EAST=3`, `DOWN/UP=-1`.
#[inline]
pub const fn dir_2d_data_value(dir: Direction) -> i32 {
    match dir {
        Direction::South => 0,
        Direction::West => 1,
        Direction::North => 2,
        Direction::East => 3,
        Direction::Down | Direction::Up => -1,
    }
}

/// `Direction.from2DDataValue(v)`: `0=SOUTH, 1=WEST, 2=NORTH, 3=EAST` (wrapping mod 4).
#[inline]
pub fn dir_from_2d_data_value(v: i32) -> Direction {
    match java_floor_mod_i32(v, 4) {
        0 => Direction::South,
        1 => Direction::West,
        2 => Direction::North,
        _ => Direction::East,
    }
}

/// `Direction.Plane.HORIZONTAL.getRandomDirection(random)` (`[NORTH, EAST, SOUTH, WEST]`).
#[inline]
pub fn random_horizontal_direction<R: RandomSource>(rng: &mut R) -> Direction {
    Direction::HORIZONTAL[rng.next_int_bounded(4) as usize]
}

/// Whether `dir` is one of `NORTH`, `EAST`, `SOUTH`, `WEST`.
#[inline]
pub const fn is_horizontal(dir: Direction) -> bool {
    matches!(
        dir,
        Direction::North | Direction::East | Direction::South | Direction::West
    )
}

/// Horizontal 90-degree rotation (`Rotation`, `structures-legacy-pieces.md` "Inputs and state").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Rotation {
    /// 0 degrees (`"NONE"`).
    #[default]
    None = 0,
    /// 90 degrees clockwise (`"CLOCKWISE_90"`).
    Clockwise90 = 1,
    /// 180 degrees (`"CLOCKWISE_180"`).
    Clockwise180 = 2,
    /// 90 degrees counter-clockwise (`"COUNTERCLOCKWISE_90"`).
    Counterclockwise90 = 3,
}

impl Rotation {
    /// `Rotation.values()` in canonical order.
    pub const ALL: [Self; 4] = [
        Self::None,
        Self::Clockwise90,
        Self::Clockwise180,
        Self::Counterclockwise90,
    ];

    /// `Rotation.getRandom(random)`.
    #[inline]
    pub fn random<R: RandomSource>(rng: &mut R) -> Self {
        Self::ALL[rng.next_int_bounded(4) as usize]
    }

    /// `Rotation.getShuffled(random)` (3 draws: `nextInt(4)`, `nextInt(3)`, `nextInt(2)`).
    #[inline]
    pub fn shuffled<R: RandomSource>(rng: &mut R) -> [Self; 4] {
        let mut arr = Self::ALL;
        shuffle_slice(&mut arr, rng);
        arr
    }

    /// `this.getRotated(other)`.
    #[inline]
    pub const fn get_rotated(self, other: Self) -> Self {
        Self::ALL[((self as usize) + (other as usize)) & 3]
    }

    /// `Rotation.rotate(Direction)`.
    #[inline]
    pub const fn rotate_dir(self, dir: Direction) -> Direction {
        if !is_horizontal(dir) {
            return dir;
        }
        match self {
            Self::None => dir,
            Self::Clockwise90 => dir.clockwise(),
            Self::Clockwise180 => dir.opposite(),
            Self::Counterclockwise90 => dir.counter_clockwise(),
        }
    }

    /// Rotates a `BlockPos` around `(0, 0, 0)` (matches `BlockPos.rotate(Rotation)`).
    #[inline]
    pub const fn rotate_pos(self, pos: BlockPos) -> BlockPos {
        match self {
            Self::None => pos,
            Self::Clockwise90 => BlockPos::new(pos.z.wrapping_neg(), pos.y, pos.x),
            Self::Clockwise180 => BlockPos::new(pos.x.wrapping_neg(), pos.y, pos.z.wrapping_neg()),
            Self::Counterclockwise90 => BlockPos::new(pos.z, pos.y, pos.x.wrapping_neg()),
        }
    }

    /// Uppercase legacy NBT name (`"NONE"`, `"CLOCKWISE_90"`, etc.).
    #[inline]
    pub const fn legacy_name(self) -> &'static str {
        match self {
            Self::None => "NONE",
            Self::Clockwise90 => "CLOCKWISE_90",
            Self::Clockwise180 => "CLOCKWISE_180",
            Self::Counterclockwise90 => "COUNTERCLOCKWISE_90",
        }
    }
}

/// Mirror transform (`Mirror`, `structures-legacy-pieces.md` "Inputs and state").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Mirror {
    /// Identity.
    #[default]
    None,
    /// Inverts Z (`z -> -z`, swaps `NORTH <-> SOUTH`).
    LeftRight,
    /// Inverts X (`x -> -x`, swaps `WEST <-> EAST`).
    FrontBack,
}

impl Mirror {
    /// `Mirror.mirror(Direction)`.
    #[inline]
    pub const fn mirror_dir(self, dir: Direction) -> Direction {
        match (self, dir) {
            (Self::LeftRight, Direction::North) => Direction::South,
            (Self::LeftRight, Direction::South) => Direction::North,
            (Self::FrontBack, Direction::West) => Direction::East,
            (Self::FrontBack, Direction::East) => Direction::West,
            _ => dir,
        }
    }

    /// `Mirror.getRotation(Direction)`: returns `CLOCKWISE_180` if `dir` is on the mirrored axis, else `NONE`.
    #[inline]
    pub const fn get_rotation(self, dir: Direction) -> Rotation {
        match (self, dir) {
            (Self::LeftRight, Direction::North | Direction::South)
            | (Self::FrontBack, Direction::East | Direction::West) => Rotation::Clockwise180,
            _ => Rotation::None,
        }
    }
}

/// 12-way jigsaw block orientation (`FrontAndTop`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrontAndTop {
    /// `down_east`: front=DOWN, top=EAST.
    DownEast,
    /// `down_north`: front=DOWN, top=NORTH.
    DownNorth,
    /// `down_south`: front=DOWN, top=SOUTH.
    DownSouth,
    /// `down_west`: front=DOWN, top=WEST.
    DownWest,
    /// `up_east`: front=UP, top=EAST.
    UpEast,
    /// `up_north`: front=UP, top=NORTH.
    UpNorth,
    /// `up_south`: front=UP, top=SOUTH.
    UpSouth,
    /// `up_west`: front=UP, top=WEST.
    UpWest,
    /// `west_up`: front=WEST, top=UP.
    WestUp,
    /// `east_up`: front=EAST, top=UP.
    EastUp,
    /// `north_up`: front=NORTH, top=UP.
    NorthUp,
    /// `south_up`: front=SOUTH, top=UP.
    SouthUp,
}

impl FrontAndTop {
    /// Parses the serialized block-state property value (e.g. `"north_up"`, `"down_south"`).
    pub fn from_name(s: &str) -> Option<Self> {
        Some(match s {
            "down_east" => Self::DownEast,
            "down_north" => Self::DownNorth,
            "down_south" => Self::DownSouth,
            "down_west" => Self::DownWest,
            "up_east" => Self::UpEast,
            "up_north" => Self::UpNorth,
            "up_south" => Self::UpSouth,
            "up_west" => Self::UpWest,
            "west_up" => Self::WestUp,
            "east_up" => Self::EastUp,
            "north_up" => Self::NorthUp,
            "south_up" => Self::SouthUp,
            _ => return None,
        })
    }

    /// Serialized property value name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::DownEast => "down_east",
            Self::DownNorth => "down_north",
            Self::DownSouth => "down_south",
            Self::DownWest => "down_west",
            Self::UpEast => "up_east",
            Self::UpNorth => "up_north",
            Self::UpSouth => "up_south",
            Self::UpWest => "up_west",
            Self::WestUp => "west_up",
            Self::EastUp => "east_up",
            Self::NorthUp => "north_up",
            Self::SouthUp => "south_up",
        }
    }

    /// Constructs from orthogonal `(front, top)` directions.
    pub const fn from_front_and_top(front: Direction, top: Direction) -> Option<Self> {
        Some(match (front, top) {
            (Direction::Down, Direction::East) => Self::DownEast,
            (Direction::Down, Direction::North) => Self::DownNorth,
            (Direction::Down, Direction::South) => Self::DownSouth,
            (Direction::Down, Direction::West) => Self::DownWest,
            (Direction::Up, Direction::East) => Self::UpEast,
            (Direction::Up, Direction::North) => Self::UpNorth,
            (Direction::Up, Direction::South) => Self::UpSouth,
            (Direction::Up, Direction::West) => Self::UpWest,
            (Direction::West, Direction::Up) => Self::WestUp,
            (Direction::East, Direction::Up) => Self::EastUp,
            (Direction::North, Direction::Up) => Self::NorthUp,
            (Direction::South, Direction::Up) => Self::SouthUp,
            _ => return None,
        })
    }

    /// Front direction (`JigsawBlock.getFrontFacing`).
    #[inline]
    pub const fn front(self) -> Direction {
        match self {
            Self::DownEast | Self::DownNorth | Self::DownSouth | Self::DownWest => Direction::Down,
            Self::UpEast | Self::UpNorth | Self::UpSouth | Self::UpWest => Direction::Up,
            Self::WestUp => Direction::West,
            Self::EastUp => Direction::East,
            Self::NorthUp => Direction::North,
            Self::SouthUp => Direction::South,
        }
    }

    /// Top direction (`JigsawBlock.getTopFacing`).
    #[inline]
    pub const fn top(self) -> Direction {
        match self {
            Self::DownEast | Self::UpEast => Direction::East,
            Self::DownNorth | Self::UpNorth => Direction::North,
            Self::DownSouth | Self::UpSouth => Direction::South,
            Self::DownWest | Self::UpWest => Direction::West,
            Self::WestUp | Self::EastUp | Self::NorthUp | Self::SouthUp => Direction::Up,
        }
    }

    /// Applies horizontal rotation to both `front` and `top`.
    #[inline]
    pub const fn rotate(self, rot: Rotation) -> Self {
        match Self::from_front_and_top(rot.rotate_dir(self.front()), rot.rotate_dir(self.top())) {
            Some(v) => v,
            None => self,
        }
    }

    /// Applies horizontal mirror to both `front` and `top`.
    #[inline]
    pub const fn mirror(self, mirror: Mirror) -> Self {
        match Self::from_front_and_top(
            mirror.mirror_dir(self.front()),
            mirror.mirror_dir(self.top()),
        ) {
            Some(v) => v,
            None => self,
        }
    }
}

/// `StructureTemplate.transform(pos, mirror, rotation, pivot)` (`structures-placement-and-jigsaw.md` §4.2).
#[inline]
pub const fn transform_pos(
    pos: BlockPos,
    mirror: Mirror,
    rotation: Rotation,
    pivot: BlockPos,
) -> BlockPos {
    let (x, y, z) = match mirror {
        Mirror::None => (pos.x, pos.y, pos.z),
        Mirror::LeftRight => (pos.x, pos.y, pos.z.wrapping_neg()),
        Mirror::FrontBack => (pos.x.wrapping_neg(), pos.y, pos.z),
    };
    let px = pivot.x;
    let pz = pivot.z;
    match rotation {
        Rotation::None => BlockPos::new(x, y, z),
        Rotation::Clockwise90 => BlockPos::new(
            px.wrapping_add(pz).wrapping_sub(z),
            y,
            pz.wrapping_sub(px).wrapping_add(x),
        ),
        Rotation::Clockwise180 => BlockPos::new(
            px.wrapping_add(px).wrapping_sub(x),
            y,
            pz.wrapping_add(pz).wrapping_sub(z),
        ),
        Rotation::Counterclockwise90 => BlockPos::new(
            px.wrapping_sub(pz).wrapping_add(z),
            y,
            px.wrapping_add(pz).wrapping_sub(x),
        ),
    }
}

/// `StructureTemplate.transform(Vec3, mirror, rotation, pivot)` (`structures-placement-and-jigsaw.md` §4.2).
#[inline]
pub fn transform_vec3(
    pos: [f64; 3],
    mirror: Mirror,
    rotation: Rotation,
    pivot: BlockPos,
) -> [f64; 3] {
    let (x, y, z) = match mirror {
        Mirror::None => (pos[0], pos[1], pos[2]),
        Mirror::LeftRight => (pos[0], pos[1], 1.0 - pos[2]),
        Mirror::FrontBack => (1.0 - pos[0], pos[1], pos[2]),
    };
    let px = f64::from(pivot.x);
    let pz = f64::from(pivot.z);
    match rotation {
        Rotation::None => [x, y, z],
        Rotation::Clockwise90 => [px + pz + 1.0 - z, y, pz - px + x],
        Rotation::Clockwise180 => [px + px + 1.0 - x, y, pz + pz + 1.0 - z],
        Rotation::Counterclockwise90 => [px - pz + z, y, px + pz + 1.0 - x],
    }
}

/// `StructureTemplate.getZeroPositionWithTransform` (`structures-legacy-pieces.md` §1.7).
#[inline]
pub const fn get_zero_position_with_transform(
    zero_pos: BlockPos,
    mirror: Mirror,
    rotation: Rotation,
    size_x: i32,
    size_z: i32,
) -> BlockPos {
    let sx = size_x - 1;
    let sz = size_z - 1;
    let mx = if matches!(mirror, Mirror::FrontBack) {
        sx
    } else {
        0
    };
    let mz = if matches!(mirror, Mirror::LeftRight) {
        sz
    } else {
        0
    };
    match rotation {
        Rotation::None => zero_pos.offset(mx, 0, mz),
        Rotation::Clockwise90 => zero_pos.offset(sz - mz, 0, mx),
        Rotation::Clockwise180 => zero_pos.offset(sx - mx, 0, sz - mz),
        Rotation::Counterclockwise90 => zero_pos.offset(mz, 0, sx - mx),
    }
}

/// Extension methods on [`BoundingBox`] matching vanilla `BoundingBox`.
pub trait BoundingBoxExt: Sized {
    /// Creates a `BoundingBox`, normalizing inverted min/max per axis.
    fn from_coords(min_x: i32, min_y: i32, min_z: i32, max_x: i32, max_y: i32, max_z: i32) -> Self;
    /// Creates a `BoundingBox` from two inclusive corner positions.
    fn from_corners(a: BlockPos, b: BlockPos) -> Self;
    /// `StructurePiece.makeBoundingBox(x, y, z, dir, width, height, depth)`.
    fn make_box(
        x: i32,
        y: i32,
        z: i32,
        dir: Direction,
        width: i32,
        height: i32,
        depth: i32,
    ) -> Self;
    /// `BoundingBox.orientBox(footX, footY, footZ, offX, offY, offZ, width, height, depth, dir)`.
    #[allow(clippy::too_many_arguments)]
    fn orient_box(
        foot_x: i32,
        foot_y: i32,
        foot_z: i32,
        off_x: i32,
        off_y: i32,
        off_z: i32,
        width: i32,
        height: i32,
        depth: i32,
        dir: Direction,
    ) -> Self;
    /// `maxX - minX + 1`.
    fn x_span(&self) -> i32;
    /// `maxY - minY + 1`.
    fn y_span(&self) -> i32;
    /// `maxZ - minZ + 1`.
    fn z_span(&self) -> i32;
    /// `getCenter()`: `(minX + xSpan / 2, minY + ySpan / 2, minZ + zSpan / 2)` (truncating `/ 2`).
    fn center(&self) -> BlockPos;
    /// Inclusive 3D containment check.
    fn is_inside(&self, pos: BlockPos) -> bool;
    /// Inclusive 3D overlap check.
    fn intersects(&self, other: Self) -> bool;
    /// Inclusive 2D XZ overlap check.
    fn intersects_xz(&self, min_x: i32, min_z: i32, max_x: i32, max_z: i32) -> bool;
    /// Returns a box shifted by `(dx, dy, dz)`.
    fn moved(&self, dx: i32, dy: i32, dz: i32) -> Self;
    /// Expands this box in place to include `pos`.
    fn encapsulate_pos(&mut self, pos: BlockPos);
    /// Half-open `f64` [`Aabb`]: `[minX, maxX + 1] × [minY, maxY + 1] × [minZ, maxZ + 1]`.
    fn to_aabb(&self) -> Aabb;
    /// `[min_x, min_y, min_z, max_x, max_y, max_z]`.
    fn to_array(&self) -> [i32; 6];
}

impl BoundingBoxExt for BoundingBox {
    #[inline]
    fn from_coords(min_x: i32, min_y: i32, min_z: i32, max_x: i32, max_y: i32, max_z: i32) -> Self {
        Self {
            min_x: min_x.min(max_x),
            min_y: min_y.min(max_y),
            min_z: min_z.min(max_z),
            max_x: min_x.max(max_x),
            max_y: min_y.max(max_y),
            max_z: min_z.max(max_z),
        }
    }

    #[inline]
    fn from_corners(a: BlockPos, b: BlockPos) -> Self {
        Self::from_coords(a.x, a.y, a.z, b.x, b.y, b.z)
    }

    #[inline]
    fn make_box(
        x: i32,
        y: i32,
        z: i32,
        dir: Direction,
        width: i32,
        height: i32,
        depth: i32,
    ) -> Self {
        match dir {
            Direction::West | Direction::East => {
                Self::from_coords(x, y, z, x + depth - 1, y + height - 1, z + width - 1)
            }
            _ => Self::from_coords(x, y, z, x + width - 1, y + height - 1, z + depth - 1),
        }
    }

    #[inline]
    fn orient_box(
        foot_x: i32,
        foot_y: i32,
        foot_z: i32,
        off_x: i32,
        off_y: i32,
        off_z: i32,
        width: i32,
        height: i32,
        depth: i32,
        dir: Direction,
    ) -> Self {
        match dir {
            Direction::North => Self::from_coords(
                foot_x + off_x,
                foot_y + off_y,
                foot_z - depth + 1 + off_z,
                foot_x + width - 1 + off_x,
                foot_y + height - 1 + off_y,
                foot_z + off_z,
            ),
            Direction::West => Self::from_coords(
                foot_x - depth + 1 + off_z,
                foot_y + off_y,
                foot_z + off_x,
                foot_x + off_z,
                foot_y + height - 1 + off_y,
                foot_z + width - 1 + off_x,
            ),
            Direction::East => Self::from_coords(
                foot_x + off_z,
                foot_y + off_y,
                foot_z + off_x,
                foot_x + depth - 1 + off_z,
                foot_y + height - 1 + off_y,
                foot_z + width - 1 + off_x,
            ),
            _ => Self::from_coords(
                foot_x + off_x,
                foot_y + off_y,
                foot_z + off_z,
                foot_x + width - 1 + off_x,
                foot_y + height - 1 + off_y,
                foot_z + depth - 1 + off_z,
            ),
        }
    }

    #[inline]
    fn x_span(&self) -> i32 {
        self.max_x - self.min_x + 1
    }

    #[inline]
    fn y_span(&self) -> i32 {
        self.max_y - self.min_y + 1
    }

    #[inline]
    fn z_span(&self) -> i32 {
        self.max_z - self.min_z + 1
    }

    #[inline]
    fn center(&self) -> BlockPos {
        BlockPos::new(
            self.min_x + self.x_span() / 2,
            self.min_y + self.y_span() / 2,
            self.min_z + self.z_span() / 2,
        )
    }

    #[inline]
    fn is_inside(&self, pos: BlockPos) -> bool {
        self.contains(pos.x, pos.y, pos.z)
    }

    #[inline]
    fn intersects(&self, other: Self) -> bool {
        self.max_x >= other.min_x
            && self.min_x <= other.max_x
            && self.max_z >= other.min_z
            && self.min_z <= other.max_z
            && self.max_y >= other.min_y
            && self.min_y <= other.max_y
    }

    #[inline]
    fn intersects_xz(&self, min_x: i32, min_z: i32, max_x: i32, max_z: i32) -> bool {
        self.max_x >= min_x && self.min_x <= max_x && self.max_z >= min_z && self.min_z <= max_z
    }

    #[inline]
    fn moved(&self, dx: i32, dy: i32, dz: i32) -> Self {
        Self {
            min_x: self.min_x.wrapping_add(dx),
            min_y: self.min_y.wrapping_add(dy),
            min_z: self.min_z.wrapping_add(dz),
            max_x: self.max_x.wrapping_add(dx),
            max_y: self.max_y.wrapping_add(dy),
            max_z: self.max_z.wrapping_add(dz),
        }
    }

    #[inline]
    fn encapsulate_pos(&mut self, pos: BlockPos) {
        self.min_x = self.min_x.min(pos.x);
        self.min_y = self.min_y.min(pos.y);
        self.min_z = self.min_z.min(pos.z);
        self.max_x = self.max_x.max(pos.x);
        self.max_y = self.max_y.max(pos.y);
        self.max_z = self.max_z.max(pos.z);
    }

    #[inline]
    fn to_aabb(&self) -> Aabb {
        Aabb::new(
            [
                f64::from(self.min_x),
                f64::from(self.min_y),
                f64::from(self.min_z),
            ],
            [
                f64::from(self.max_x + 1),
                f64::from(self.max_y + 1),
                f64::from(self.max_z + 1),
            ],
        )
    }

    #[inline]
    fn to_array(&self) -> [i32; 6] {
        [
            self.min_x, self.min_y, self.min_z, self.max_x, self.max_y, self.max_z,
        ]
    }
}

/// Computes `StructurePiece.getWorldPos(x, y, z)` for a piece with `bb` and `orientation`.
#[inline]
pub const fn piece_world_pos(
    bb: BoundingBox,
    orientation: Option<Direction>,
    x: i32,
    y: i32,
    z: i32,
) -> BlockPos {
    match orientation {
        None => BlockPos::new(x, y, z),
        Some(Direction::North) => BlockPos::new(bb.min_x + x, bb.min_y + y, bb.max_z - z),
        Some(Direction::South) => BlockPos::new(bb.min_x + x, bb.min_y + y, bb.min_z + z),
        Some(Direction::West) => BlockPos::new(bb.max_x - z, bb.min_y + y, bb.min_z + x),
        Some(Direction::East) => BlockPos::new(bb.min_x + z, bb.min_y + y, bb.min_z + x),
        Some(Direction::Down | Direction::Up) => BlockPos::new(x, bb.min_y + y, z),
    }
}

// ---------------------------------------------------------------------------
// WorldgenRandom seeding and RNG helpers
// ---------------------------------------------------------------------------

/// `WorldgenRandom.setLargeFeatureSeed(seed, chunkX, chunkZ)`.
#[inline]
pub fn set_large_feature_seed<R: RandomSource>(rng: &mut R, seed: i64, chunk_x: i32, chunk_z: i32) {
    rng.set_seed(seed);
    let x_scale = rng.next_long();
    let z_scale = rng.next_long();
    let mixed =
        i64::from(chunk_x).wrapping_mul(x_scale) ^ i64::from(chunk_z).wrapping_mul(z_scale) ^ seed;
    rng.set_seed(mixed);
}

/// `WorldgenRandom.setLargeFeatureWithSalt(seed, x, z, salt)`.
#[inline]
pub fn set_large_feature_with_salt<R: RandomSource>(
    rng: &mut R,
    seed: i64,
    x: i32,
    z: i32,
    salt: i32,
) {
    let s = i64::from(x)
        .wrapping_mul(341_873_128_712_i64)
        .wrapping_add(i64::from(z).wrapping_mul(132_897_987_541_i64))
        .wrapping_add(seed)
        .wrapping_add(i64::from(salt));
    rng.set_seed(s);
}

/// `WorldgenRandom.setDecorationSeed(seed, blockX, blockZ) -> i64`.
#[inline]
pub fn set_decoration_seed<R: RandomSource>(
    rng: &mut R,
    seed: i64,
    block_x: i32,
    block_z: i32,
) -> i64 {
    rng.set_seed(seed);
    let x_scale = rng.next_long() | 1;
    let z_scale = rng.next_long() | 1;
    let result = i64::from(block_x)
        .wrapping_mul(x_scale)
        .wrapping_add(i64::from(block_z).wrapping_mul(z_scale))
        ^ seed;
    rng.set_seed(result);
    result
}

/// `WorldgenRandom.setFeatureSeed(decorationSeed, index, step)`.
#[inline]
pub fn set_feature_seed<R: RandomSource>(rng: &mut R, decoration_seed: i64, index: i32, step: i32) {
    let s = decoration_seed
        .wrapping_add(i64::from(index))
        .wrapping_add(i64::from(10_000_i32.wrapping_mul(step)));
    rng.set_seed(s);
}

/// Backward Fisher-Yates shuffle matching `Util.shuffle` / `Util.shuffledCopy`.
#[inline]
pub fn shuffle_slice<T, R: RandomSource>(slice: &mut [T], rng: &mut R) {
    let n = slice.len();
    for i in (2..=n).rev() {
        let j = rng.next_int_bounded(i as i32) as usize;
        slice.swap(i - 1, j);
    }
}

/// `Mth.randomBetweenInclusive(rng, min, max)`: unconditionally draws `nextInt(max - min + 1) + min`.
#[inline]
pub fn random_between_inclusive<R: RandomSource>(rng: &mut R, min: i32, max: i32) -> i32 {
    rng.next_int_bounded(max.wrapping_sub(min).wrapping_add(1))
        .wrapping_add(min)
}

/// `Mth.nextInt(rng, min, max)`: returns `min` without consuming RNG when `min >= max`.
#[inline]
pub fn mth_next_int<R: RandomSource>(rng: &mut R, min: i32, max: i32) -> i32 {
    if min >= max {
        min
    } else {
        rng.next_int_bounded(max.wrapping_sub(min).wrapping_add(1))
            .wrapping_add(min)
    }
}

/// Exact `HeightProvider.sample` per `structures-placement-and-jigsaw.md` §3.3.
///
/// Handles `VeryBiasedToBottom`'s `Mth.nextInt` zero-draw guard when `lo >= hi`.
pub fn sample_height_provider<R: RandomSource>(
    hp: &HeightProvider,
    rng: &mut R,
    ctx: WorldGenerationContext,
) -> i32 {
    match hp {
        HeightProvider::Constant(a) => a.resolve_y(ctx),
        HeightProvider::Uniform {
            min_inclusive,
            max_inclusive,
        } => {
            let min = min_inclusive.resolve_y(ctx);
            let max = max_inclusive.resolve_y(ctx);
            if min > max {
                return min;
            }
            random_between_inclusive(rng, min, max)
        }
        HeightProvider::BiasedToBottom {
            min_inclusive,
            max_inclusive,
            inner,
        } => {
            let min = min_inclusive.resolve_y(ctx);
            let max = max_inclusive.resolve_y(ctx);
            let span = max.wrapping_sub(min).wrapping_sub(*inner).wrapping_add(1);
            if span <= 0 {
                return min;
            }
            let limit = rng.next_int_bounded(span);
            rng.next_int_bounded(limit.wrapping_add(*inner))
                .wrapping_add(min)
        }
        HeightProvider::VeryBiasedToBottom {
            min_inclusive,
            max_inclusive,
            inner,
        } => {
            let min = min_inclusive.resolve_y(ctx);
            let max = max_inclusive.resolve_y(ctx);
            let span = max.wrapping_sub(min).wrapping_sub(*inner).wrapping_add(1);
            if span <= 0 {
                return min;
            }
            let u = mth_next_int(rng, min.wrapping_add(*inner), max);
            let b = mth_next_int(rng, min, u.wrapping_sub(1));
            mth_next_int(rng, min, b.wrapping_sub(1).wrapping_add(*inner))
        }
        HeightProvider::Trapezoid {
            min_inclusive,
            max_inclusive,
            plateau,
        } => {
            let min = min_inclusive.resolve_y(ctx);
            let max = max_inclusive.resolve_y(ctx);
            if min > max {
                return min;
            }
            let range = max.wrapping_sub(min);
            if *plateau >= range {
                return random_between_inclusive(rng, min, max);
            }
            let p_start = (range - *plateau) / 2;
            let p_end = range - p_start;
            min.wrapping_add(random_between_inclusive(rng, 0, p_end))
                .wrapping_add(random_between_inclusive(rng, 0, p_start))
        }
        HeightProvider::WeightedList(entries) => {
            let total: i32 = entries.iter().map(|(_, w)| *w).sum();
            if total <= 0 || entries.is_empty() {
                return ctx.min_gen_y;
            }
            let mut pick = rng.next_int_bounded(total);
            for (p, w) in entries {
                pick -= *w;
                if pick < 0 {
                    return sample_height_provider(p, rng, ctx);
                }
            }
            sample_height_provider(&entries[0].0, rng, ctx)
        }
    }
}
