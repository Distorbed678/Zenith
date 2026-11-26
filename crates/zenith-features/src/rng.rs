//! Java-exact `WorldgenRandom`, `BlockPos` / `Rotation` / `Direction` helpers, and RNG utilities
//! matching `docs/specs/worldgen/features.md` ("Inputs and state").

use zenith_blocks::Direction;
use zenith_math::java::D;
use zenith_math::random::MarsagliaPolarGaussian;
use zenith_math::{
    BitRandomSource, LegacyRandom, PositionalRandomFactory, RandomSource, XoroshiroRandom,
};

/// 3D integer block position matching Java `net.minecraft.core.BlockPos`.
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

    /// Constructs a new `BlockPos`.
    #[inline]
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// Offsets by `(dx, dy, dz)` with wrapping 32-bit addition.
    #[inline]
    pub const fn offset(self, dx: i32, dy: i32, dz: i32) -> Self {
        Self {
            x: self.x.wrapping_add(dx),
            y: self.y.wrapping_add(dy),
            z: self.z.wrapping_add(dz),
        }
    }

    /// Returns a copy with Y replaced by `y`.
    #[inline]
    pub const fn at_y(self, y: i32) -> Self {
        Self {
            x: self.x,
            y,
            z: self.z,
        }
    }

    /// Moves one block in `dir`.
    #[inline]
    pub const fn relative(self, dir: Direction) -> Self {
        self.relative_n(dir, 1)
    }

    /// Moves `n` blocks in `dir`.
    #[inline]
    pub const fn relative_n(self, dir: Direction, n: i32) -> Self {
        let (dx, dy, dz) = direction_step(dir);
        self.offset(dx.wrapping_mul(n), dy.wrapping_mul(n), dz.wrapping_mul(n))
    }

    /// One block above (`y + 1`).
    #[inline]
    pub const fn above(self) -> Self {
        self.offset(0, 1, 0)
    }

    /// `n` blocks above (`y + n`).
    #[inline]
    pub const fn above_n(self, n: i32) -> Self {
        self.offset(0, n, 0)
    }

    /// One block below (`y - 1`).
    #[inline]
    pub const fn below(self) -> Self {
        self.offset(0, -1, 0)
    }

    /// `n` blocks below (`y - n`).
    #[inline]
    pub const fn below_n(self, n: i32) -> Self {
        self.offset(0, -n, 0)
    }

    /// Squared Euclidean distance in `f64` (`distSqr` / `distToLowCornerSqr`).
    #[inline]
    pub fn dist_sqr(self, other: Self) -> f64 {
        let dx = f64::from(self.x) - f64::from(other.x);
        let dy = f64::from(self.y) - f64::from(other.y);
        let dz = f64::from(self.z) - f64::from(other.z);
        dx * dx + dy * dy + dz * dz
    }

    /// Strict `<` Euclidean distance test (`closerThan`).
    #[inline]
    pub fn closer_than(self, other: Self, dist: f64) -> bool {
        self.dist_sqr(other) < dist * dist
    }

    /// Manhattan distance matching `Vec3i.distManhattan`:
    /// `trunc_i32(f32(|b.x - a.x|) + f32(|b.y - a.y|) + f32(|b.z - a.z|))`.
    #[inline]
    pub fn dist_manhattan(self, other: Self) -> i32 {
        let fx = (other.x.wrapping_sub(self.x) as f32).abs();
        let fy = (other.y.wrapping_sub(self.y) as f32).abs();
        let fz = (other.z.wrapping_sub(self.z) as f32).abs();
        (fx + fy + fz) as i32
    }

    /// Chebyshev (chessboard) distance `max(|dx|, |dy|, |dz|)`.
    #[inline]
    pub fn dist_chessboard(self, other: Self) -> i32 {
        let dx = self.x.wrapping_sub(other.x).wrapping_abs();
        let dy = self.y.wrapping_sub(other.y).wrapping_abs();
        let dz = self.z.wrapping_sub(other.z).wrapping_abs();
        dx.max(dy).max(dz)
    }

    /// Java `Vec3i.hashCode()`: `((y + z * 31) * 31) + x`.
    #[inline]
    pub const fn java_hash_code(self) -> i32 {
        self.y
            .wrapping_add(self.z.wrapping_mul(31))
            .wrapping_mul(31)
            .wrapping_add(self.x)
    }

    /// Chunk X coordinate (`x >> 4`).
    #[inline]
    pub const fn chunk_x(self) -> i32 {
        self.x >> 4
    }

    /// Chunk Z coordinate (`z >> 4`).
    #[inline]
    pub const fn chunk_z(self) -> i32 {
        self.z >> 4
    }
}

/// Unit step `(dx, dy, dz)` for a [`Direction`].
#[inline]
pub const fn direction_step(dir: Direction) -> (i32, i32, i32) {
    match dir {
        Direction::Down => (0, -1, 0),
        Direction::Up => (0, 1, 0),
        Direction::North => (0, 0, -1),
        Direction::South => (0, 0, 1),
        Direction::West => (-1, 0, 0),
        Direction::East => (1, 0, 0),
    }
}

/// Axis string (`"x"`, `"y"`, or `"z"`) for a [`Direction`].
#[inline]
pub const fn direction_axis_name(dir: Direction) -> &'static str {
    match dir {
        Direction::West | Direction::East => "x",
        Direction::Down | Direction::Up => "y",
        Direction::North | Direction::South => "z",
    }
}

/// Whether `dir` is horizontal (`NORTH`, `SOUTH`, `WEST`, `EAST`).
#[inline]
pub const fn direction_is_horizontal(dir: Direction) -> bool {
    !matches!(dir, Direction::Down | Direction::Up)
}

/// `Direction.from2DDataValue(i)`: `[SOUTH(0), WEST(1), NORTH(2), EAST(3)]`.
#[inline]
pub const fn direction_from_2d_data_value(i: i32) -> Direction {
    match i.rem_euclid(4) {
        0 => Direction::South,
        1 => Direction::West,
        2 => Direction::North,
        _ => Direction::East,
    }
}

/// Block rotation matching `net.minecraft.world.level.block.Rotation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rotation {
    /// `none` (0 degrees).
    None,
    /// `clockwise_90` (90 degrees clockwise).
    Clockwise90,
    /// `180` (180 degrees).
    Clockwise180,
    /// `counterclockwise_90` (270 degrees clockwise).
    Counterclockwise90,
}

impl Rotation {
    /// All 4 rotations in `Rotation.values()` order.
    pub const ALL: [Self; 4] = [
        Self::None,
        Self::Clockwise90,
        Self::Clockwise180,
        Self::Counterclockwise90,
    ];

    /// Parses a serialized rotation name.
    pub fn from_name(s: &str) -> Option<Self> {
        match s {
            "none" => Some(Self::None),
            "clockwise_90" => Some(Self::Clockwise90),
            "180" => Some(Self::Clockwise180),
            "counterclockwise_90" => Some(Self::Counterclockwise90),
            _ => None,
        }
    }

    /// Picks a uniform random rotation (`Rotation.getRandom`).
    #[inline]
    pub fn get_random(rng: &mut impl RandomSource) -> Self {
        Self::ALL[rng.next_int_bounded(4) as usize]
    }

    /// Rotates a horizontal [`Direction`] (vertical directions are unchanged).
    #[inline]
    pub const fn rotate(self, dir: Direction) -> Direction {
        if !direction_is_horizontal(dir) {
            return dir;
        }
        match self {
            Self::None => dir,
            Self::Clockwise90 => dir.clockwise(),
            Self::Clockwise180 => dir.opposite(),
            Self::Counterclockwise90 => dir.counter_clockwise(),
        }
    }
}

/// Backing source wrapped by [`WorldgenRandom`].
#[derive(Debug, Clone)]
enum WorldgenBacking {
    Xoroshiro(XoroshiroRandom),
    Legacy(LegacyRandom),
}

/// Exact replica of `net.minecraft.world.level.levelgen.WorldgenRandom` (`features.md` Inputs & State).
///
/// Crucially, `WorldgenRandom` subclasses `LegacyRandomSource` (`BitRandomSource`) and overrides
/// ONLY `next(bits)`, `setSeed(seed)`, `fork()`, and `forkPositional()`. When wrapping a
/// `XoroshiroRandomSource`:
/// - `next(bits)` returns the **top** `bits` of one 64-bit Xoroshiro output (`nextLong() >>> (64 - bits)`).
/// - `nextInt(bound)`, `nextLong()` (2 steps), `nextBoolean()` (MSB), `nextFloat()`, `nextDouble()` (2 steps),
///   and `nextGaussian()` ALL run through `BitRandomSource`!
/// - `setSeed(seed)` forwards to the backing source and does **NOT** reset `WorldgenRandom`'s own
///   inherited `MarsagliaPolarGaussian` cache (per spec).
#[derive(Debug, Clone)]
pub struct WorldgenRandom {
    backing: WorldgenBacking,
    gaussian: MarsagliaPolarGaussian,
    count: u64,
}

impl WorldgenRandom {
    /// Creates a `WorldgenRandom` wrapping a `XoroshiroRandom` (used by `applyBiomeDecoration`).
    #[inline]
    pub fn new_xoroshiro(seed: i64) -> Self {
        Self {
            backing: WorldgenBacking::Xoroshiro(XoroshiroRandom::new(seed)),
            gaussian: MarsagliaPolarGaussian::new(),
            count: 0,
        }
    }

    /// Creates a `WorldgenRandom` wrapping a `LegacyRandom` (used by noise-based state providers and geodes).
    #[inline]
    pub fn new_legacy(seed: i64) -> Self {
        Self {
            backing: WorldgenBacking::Legacy(LegacyRandom::new(seed)),
            gaussian: MarsagliaPolarGaussian::new(),
            count: 0,
        }
    }

    /// Number of `next(bits)` calls executed so far.
    #[inline]
    pub const fn count(&self) -> u64 {
        self.count
    }

    /// `WorldgenRandom.setDecorationSeed(long worldSeed, int chunkBlockX, int chunkBlockZ)`.
    #[inline]
    pub fn set_decoration_seed(
        &mut self,
        world_seed: i64,
        chunk_block_x: i32,
        chunk_block_z: i32,
    ) -> i64 {
        self.set_seed(world_seed);
        let x_scale = self.next_long() | 1;
        let z_scale = self.next_long() | 1;
        let result = i64::from(chunk_block_x)
            .wrapping_mul(x_scale)
            .wrapping_add(i64::from(chunk_block_z).wrapping_mul(z_scale))
            ^ world_seed;
        self.set_seed(result);
        result
    }

    /// `WorldgenRandom.setFeatureSeed(long decorationSeed, int index, int step)`.
    #[inline]
    pub fn set_feature_seed(&mut self, decoration_seed: i64, index: i32, step: i32) {
        let step_term = 10_000_i32.wrapping_mul(step);
        let seed = decoration_seed
            .wrapping_add(i64::from(index))
            .wrapping_add(i64::from(step_term));
        self.set_seed(seed);
    }

    /// `WorldgenRandom.setLargeFeatureSeed(long seed, int chunkX, int chunkZ)`.
    #[inline]
    pub fn set_large_feature_seed(&mut self, seed: i64, chunk_x: i32, chunk_z: i32) {
        self.set_seed(seed);
        let a = self.next_long();
        let b = self.next_long();
        let res = i64::from(chunk_x).wrapping_mul(a) ^ i64::from(chunk_z).wrapping_mul(b) ^ seed;
        self.set_seed(res);
    }

    /// `WorldgenRandom.setLargeFeatureWithSalt(long seed, int x, int z, int salt)`.
    #[inline]
    pub fn set_large_feature_with_salt(&mut self, seed: i64, x: i32, z: i32, salt: i32) {
        let s = i64::from(x)
            .wrapping_mul(341_873_128_712)
            .wrapping_add(i64::from(z).wrapping_mul(132_897_987_541))
            .wrapping_add(seed)
            .wrapping_add(i64::from(salt));
        self.set_seed(s);
    }
}

impl BitRandomSource for WorldgenRandom {
    #[inline]
    fn next_bits(&mut self, bits: u32) -> i32 {
        self.count = self.count.wrapping_add(1);
        match &mut self.backing {
            WorldgenBacking::Xoroshiro(x) => ((x.next_long() as u64) >> (64 - bits)) as i32,
            WorldgenBacking::Legacy(l) => l.next_bits(bits),
        }
    }
}

/// Dummy positional factory returned by [`WorldgenRandom::fork_positional`] when used through
/// the [`RandomSource`] trait.
#[derive(Debug, Clone)]
pub enum WorldgenPositionalFactory {
    /// Xoroshiro positional factory.
    Xoroshiro(zenith_math::XoroshiroPositionalRandomFactory),
    /// Legacy positional factory.
    Legacy(zenith_math::LegacyPositionalRandomFactory),
}

impl PositionalRandomFactory for WorldgenPositionalFactory {
    type Random = WorldgenRandom;

    fn at(&self, x: i32, y: i32, z: i32) -> Self::Random {
        match self {
            Self::Xoroshiro(f) => WorldgenRandom {
                backing: WorldgenBacking::Xoroshiro(f.at(x, y, z)),
                gaussian: MarsagliaPolarGaussian::new(),
                count: 0,
            },
            Self::Legacy(f) => WorldgenRandom {
                backing: WorldgenBacking::Legacy(f.at(x, y, z)),
                gaussian: MarsagliaPolarGaussian::new(),
                count: 0,
            },
        }
    }

    fn from_hash_of(&self, name: &str) -> Self::Random {
        match self {
            Self::Xoroshiro(f) => WorldgenRandom {
                backing: WorldgenBacking::Xoroshiro(f.from_hash_of(name)),
                gaussian: MarsagliaPolarGaussian::new(),
                count: 0,
            },
            Self::Legacy(f) => WorldgenRandom {
                backing: WorldgenBacking::Legacy(f.from_hash_of(name)),
                gaussian: MarsagliaPolarGaussian::new(),
                count: 0,
            },
        }
    }

    fn from_seed(&self, seed: i64) -> Self::Random {
        match self {
            Self::Xoroshiro(f) => WorldgenRandom {
                backing: WorldgenBacking::Xoroshiro(f.from_seed(seed)),
                gaussian: MarsagliaPolarGaussian::new(),
                count: 0,
            },
            Self::Legacy(f) => WorldgenRandom {
                backing: WorldgenBacking::Legacy(f.from_seed(seed)),
                gaussian: MarsagliaPolarGaussian::new(),
                count: 0,
            },
        }
    }
}

impl RandomSource for WorldgenRandom {
    type Positional = WorldgenPositionalFactory;

    #[inline]
    fn set_seed(&mut self, seed: i64) {
        // NOTE: Intentionally does NOT reset `self.gaussian`, matching vanilla `WorldgenRandom`!
        match &mut self.backing {
            WorldgenBacking::Xoroshiro(x) => x.set_seed(seed),
            WorldgenBacking::Legacy(l) => l.set_seed(seed),
        }
    }

    #[inline]
    fn next_int(&mut self) -> i32 {
        self.bit_next_int()
    }

    #[inline]
    fn next_int_bounded(&mut self, bound: i32) -> i32 {
        self.bit_next_int_bounded(bound)
    }

    #[inline]
    fn next_long(&mut self) -> i64 {
        self.bit_next_long()
    }

    #[inline]
    fn next_boolean(&mut self) -> bool {
        self.bit_next_boolean()
    }

    #[inline]
    fn next_float(&mut self) -> f32 {
        self.bit_next_float()
    }

    #[inline]
    fn next_double(&mut self) -> f64 {
        self.bit_next_double()
    }

    #[inline]
    fn next_gaussian(&mut self) -> f64 {
        let backing = &mut self.backing;
        let count = &mut self.count;
        self.gaussian.next_gaussian(|| {
            *count = count.wrapping_add(2);
            let (hi, lo) = match backing {
                WorldgenBacking::Xoroshiro(x) => {
                    let h = ((x.next_long() as u64) >> (64 - 26)) as i32;
                    let l = ((x.next_long() as u64) >> (64 - 27)) as i32;
                    (h, l)
                }
                WorldgenBacking::Legacy(l) => (l.next_bits(26), l.next_bits(27)),
            };
            let combined = (i64::from(hi) << 27) + i64::from(lo);
            (combined as f64) * D
        })
    }

    #[inline]
    fn fork(&mut self) -> Self {
        match &mut self.backing {
            WorldgenBacking::Xoroshiro(x) => Self {
                backing: WorldgenBacking::Xoroshiro(x.fork()),
                gaussian: MarsagliaPolarGaussian::new(),
                count: 0,
            },
            WorldgenBacking::Legacy(l) => Self {
                backing: WorldgenBacking::Legacy(l.fork()),
                gaussian: MarsagliaPolarGaussian::new(),
                count: 0,
            },
        }
    }

    #[inline]
    fn fork_positional(&mut self) -> Self::Positional {
        match &mut self.backing {
            WorldgenBacking::Xoroshiro(x) => {
                WorldgenPositionalFactory::Xoroshiro(x.fork_positional())
            }
            WorldgenBacking::Legacy(l) => WorldgenPositionalFactory::Legacy(l.fork_positional()),
        }
    }
}

/// `Mth.nextInt(rng, min, max)`: skips RNG when `min >= max`.
#[inline]
pub fn mth_next_int(rng: &mut impl RandomSource, min: i32, max: i32) -> i32 {
    if min >= max {
        min
    } else {
        rng.next_int_bounded(max.wrapping_sub(min).wrapping_add(1))
            .wrapping_add(min)
    }
}

/// `Mth.randomBetweenInclusive(rng, min, max)`: unconditionally draws `nextInt(max - min + 1) + min`.
#[inline]
pub fn random_between_inclusive(rng: &mut impl RandomSource, min: i32, max: i32) -> i32 {
    rng.next_int_bounded(max.wrapping_sub(min).wrapping_add(1))
        .wrapping_add(min)
}

/// `Mth.randomBetween(rng, min, max)`: `rng.nextFloat() * (max - min) + min`.
#[inline]
pub fn random_between_f32(rng: &mut impl RandomSource, min: f32, max: f32) -> f32 {
    rng.next_float() * (max - min) + min
}

/// `Mth.normal(rng, mean, dev)`: `mean + (rng.nextGaussian() as f32) * dev`.
#[inline]
pub fn mth_normal(rng: &mut impl RandomSource, mean: f32, dev: f32) -> f32 {
    mean + (rng.next_gaussian() as f32) * dev
}

/// `Util.shuffle`: backward Fisher-Yates for `i = n` down to `2` inclusive.
#[inline]
pub fn util_shuffle<T>(slice: &mut [T], rng: &mut impl RandomSource) {
    let mut i = slice.len();
    while i > 1 {
        let swap_to = rng.next_int_bounded(i as i32) as usize;
        slice.swap(i - 1, swap_to);
        i -= 1;
    }
}

/// `Direction.getRandom(rng)`.
#[inline]
pub fn direction_get_random(rng: &mut impl RandomSource) -> Direction {
    Direction::ALL[rng.next_int_bounded(6) as usize]
}

/// `Direction.Plane.HORIZONTAL.getRandomDirection(rng)` (`[NORTH, EAST, SOUTH, WEST]`).
#[inline]
pub fn direction_horizontal_random(rng: &mut impl RandomSource) -> Direction {
    Direction::HORIZONTAL[rng.next_int_bounded(4) as usize]
}

/// `Direction.allShuffled(rng)`.
#[inline]
pub fn direction_all_shuffled(rng: &mut impl RandomSource) -> [Direction; 6] {
    let mut dirs = Direction::ALL;
    util_shuffle(&mut dirs, rng);
    dirs
}

/// `Direction.Plane.HORIZONTAL.shuffledCopy(rng)`.
#[inline]
pub fn direction_horizontal_shuffled(rng: &mut impl RandomSource) -> [Direction; 4] {
    let mut dirs = Direction::HORIZONTAL;
    util_shuffle(&mut dirs, rng);
    dirs
}

/// `BlockPos.betweenClosed(a, b)`: normalized box, **`z` outer, `y` middle, `x` inner** (all ascending).
#[derive(Debug, Clone)]
pub struct BetweenClosed {
    min_x: i32,
    min_y: i32,
    min_z: i32,
    width: i32,
    height: i32,
    end: i32,
    index: i32,
}

impl BetweenClosed {
    /// Creates an iterator over all block positions in the normalized inclusive box `[a, b]`.
    #[inline]
    pub fn new(a: BlockPos, b: BlockPos) -> Self {
        let min_x = a.x.min(b.x);
        let max_x = a.x.max(b.x);
        let min_y = a.y.min(b.y);
        let max_y = a.y.max(b.y);
        let min_z = a.z.min(b.z);
        let max_z = a.z.max(b.z);
        let width = max_x - min_x + 1;
        let height = max_y - min_y + 1;
        let depth = max_z - min_z + 1;
        Self {
            min_x,
            min_y,
            min_z,
            width,
            height,
            end: width * height * depth,
            index: 0,
        }
    }
}

impl Iterator for BetweenClosed {
    type Item = BlockPos;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.end {
            return None;
        }
        let i = self.index;
        self.index += 1;
        let x = i % self.width;
        let j = i / self.width;
        let y = j % self.height;
        let z = j / self.height;
        Some(BlockPos::new(
            self.min_x + x,
            self.min_y + y,
            self.min_z + z,
        ))
    }
}

/// `BlockPos.betweenClosed(a, b)`.
#[inline]
pub fn between_closed(a: BlockPos, b: BlockPos) -> BetweenClosed {
    BetweenClosed::new(a, b)
}

/// `BlockPos.withinBoxByManhattanDistance(origin, reach_x, reach_y, reach_z)`.
pub fn within_box_by_manhattan_distance(
    origin: BlockPos,
    reach_x: i32,
    reach_y: i32,
    reach_z: i32,
    mut f: impl FnMut(BlockPos) -> bool,
) {
    let max_d = reach_x + reach_y + reach_z;
    for d in 0..=max_d {
        let max_x = reach_x.min(d);
        for x in -max_x..=max_x {
            let rem_y = d - x.abs();
            let max_y = reach_y.min(rem_y);
            for y in -max_y..=max_y {
                let z = rem_y - y.abs();
                if z <= reach_z {
                    if !f(BlockPos::new(origin.x + x, origin.y + y, origin.z + z)) {
                        return;
                    }
                    if z != 0 && !f(BlockPos::new(origin.x + x, origin.y + y, origin.z - z)) {
                        return;
                    }
                }
            }
        }
    }
}

/// `BlockPos.randomBetweenClosed` single lazy step: draws `x`, `y`, `z` in exact order.
#[inline]
pub fn next_random_between_closed(
    rng: &mut impl RandomSource,
    min_x: i32,
    min_y: i32,
    min_z: i32,
    max_x: i32,
    max_y: i32,
    max_z: i32,
) -> BlockPos {
    let w = max_x - min_x + 1;
    let h = max_y - min_y + 1;
    let d = max_z - min_z + 1;
    let x = min_x + rng.next_int_bounded(w);
    let y = min_y + rng.next_int_bounded(h);
    let z = min_z + rng.next_int_bounded(d);
    BlockPos::new(x, y, z)
}

/// `BoundingBox.forAllCorners` exact corner order.
#[inline]
pub fn bounding_box_corners(
    min_x: i32,
    min_y: i32,
    min_z: i32,
    max_x: i32,
    max_y: i32,
    max_z: i32,
) -> [BlockPos; 8] {
    [
        BlockPos::new(max_x, max_y, max_z),
        BlockPos::new(min_x, max_y, max_z),
        BlockPos::new(max_x, min_y, max_z),
        BlockPos::new(min_x, min_y, max_z),
        BlockPos::new(max_x, max_y, min_z),
        BlockPos::new(min_x, max_y, min_z),
        BlockPos::new(max_x, min_y, min_z),
        BlockPos::new(min_x, min_y, min_z),
    ]
}
