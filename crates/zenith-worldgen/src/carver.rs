//! World carvers (`minecraft:cave`, `minecraft:canyon`), `CarvingMask`, and carving-step
//! execution (`docs/specs/worldgen/surface-and-carvers.md` §3).

use zenith_codec::codec::{
    BoolCodec, Codec, DataResult, FloatCodec, IntCodec, RecordReader, decode_dispatch, validated,
};
use zenith_codec::json::Json;
use zenith_math::helpers::rounding::floor_f64;
use zenith_math::helpers::trig::{HALF_PI, PI, TWO_PI, cos, sin};
use zenith_math::{LegacyRandom, RandomSource};
use zenith_registry::Identifier;

use crate::providers::{
    FloatProvider, FloatProviderCodec, HeightProvider, HeightProviderCodec, IntProvider,
    IntProviderCodec, WorldGenerationContext,
};

/// Default chunk reach range for all built-in carvers (`WorldCarver.getRange() = 4`).
pub const CARVER_RANGE: i32 = 4;

/// Maximum tunnel distance in blocks (`(CARVER_RANGE * 2 - 1) << 4 = 112`).
pub const CARVER_MAX_DISTANCE: i32 = (CARVER_RANGE * 2 - 1) << 4;

/// Neighborhood radius (in chunks) scanned by `generateCarvers` (`17 × 17` chunks).
pub const CARVER_CHUNK_RADIUS: i32 = 8;

/// Number of top blocks excluded from `CarvingMask` on non-upgrading chunks.
pub const CARVER_PROTECTED_BLOCKS_ON_TOP: i32 = 7;

/// Configured `minecraft:cave` carver (`CaveWorldCarver`, `surface-and-carvers.md` §3.1 & §3.4).
#[derive(Debug, Clone, PartialEq)]
pub struct CaveCarver {
    /// Start probability in `[0.0, 1.0]`.
    pub probability: f32,
    /// Starting Y height provider.
    pub y: HeightProvider,
    /// Number of caves per starting chunk.
    pub count: IntProvider,
    /// Base tunnel thickness provider.
    pub thickness: FloatProvider,
    /// Whether to apply the 1-in-10 thickness boost (`weird_thickness_bias`, default `false`).
    pub weird_thickness_bias: bool,
    /// Vertical radius multiplier for circular cave rooms.
    pub room_vertical_radius_multiplier: FloatProvider,
    /// Horizontal radius multiplier for tunnels and rooms.
    pub horizontal_radius_multiplier: FloatProvider,
    /// Vertical radius multiplier for tunnels and rooms.
    pub vertical_radius_multiplier: FloatProvider,
    /// Initial tunnel vertical radius scale (`start_vertical_radius_multiplier`, default `1.0`).
    pub start_vertical_radius_multiplier: FloatProvider,
    /// Relative floor cutoff in `[-1.0, 1.0]`.
    pub floor_level: FloatProvider,
}

/// Shape parameters for `minecraft:canyon` (`CanyonWorldCarver.Shape`, `surface-and-carvers.md` §3.1).
#[derive(Debug, Clone, PartialEq)]
pub struct CanyonShape {
    /// Distance factor multiplying `CARVER_MAX_DISTANCE`.
    pub distance_factor: FloatProvider,
    /// Canyon thickness provider.
    pub thickness: FloatProvider,
    /// Controls frequency of vertical ledge width changes (`>= 1`).
    pub width_smoothness: i32,
    /// Per-step horizontal radius multiplier.
    pub horizontal_radius_factor: FloatProvider,
    /// Base vertical radius factor.
    pub vertical_radius_default_factor: f32,
    /// Center-boost vertical radius factor.
    pub vertical_radius_center_factor: f32,
    /// Vertical radius scale (`y_scale`).
    pub y_scale: FloatProvider,
}

/// Configured `minecraft:canyon` carver (`CanyonWorldCarver`, `surface-and-carvers.md` §3.1 & §3.5).
#[derive(Debug, Clone, PartialEq)]
pub struct CanyonCarver {
    /// Start probability in `[0.0, 1.0]`.
    pub probability: f32,
    /// Starting Y height provider.
    pub y: HeightProvider,
    /// Initial vertical pitch provider.
    pub vertical_rotation: FloatProvider,
    /// Canyon shape parameters.
    pub shape: CanyonShape,
}

/// A 26.3 configured carver (`minecraft:worldgen/carver`).
#[derive(Debug, Clone, PartialEq)]
pub enum Carver {
    /// `minecraft:cave` (also used for `minecraft:nether_cave`).
    Cave(CaveCarver),
    /// `minecraft:canyon`.
    Canyon(CanyonCarver),
}

impl Carver {
    /// Start probability of this carver.
    #[inline]
    pub const fn probability(&self) -> f32 {
        match self {
            Self::Cave(c) => c.probability,
            Self::Canyon(c) => c.probability,
        }
    }

    /// Evaluates `WorldCarver.isStartChunk(random)`: `random.nextFloat() <= probability`.
    #[inline]
    pub fn is_start_chunk(&self, rng: &mut LegacyRandom) -> bool {
        rng.next_float() <= self.probability()
    }

    /// Runs this carver from `(source_chunk_x, source_chunk_z)` into target chunk
    /// `(chunk_x, chunk_z)`'s [`CarvingMask`].
    #[allow(clippy::too_many_arguments)]
    pub fn carve(
        &self,
        ctx: WorldGenerationContext,
        rng: &mut LegacyRandom,
        chunk_x: i32,
        chunk_z: i32,
        source_chunk_x: i32,
        source_chunk_z: i32,
        mask: &mut CarvingMask,
    ) {
        match self {
            Self::Cave(c) => c.carve(
                ctx,
                rng,
                chunk_x,
                chunk_z,
                source_chunk_x,
                source_chunk_z,
                mask,
            ),
            Self::Canyon(c) => c.carve(
                ctx,
                rng,
                chunk_x,
                chunk_z,
                source_chunk_x,
                source_chunk_z,
                mask,
            ),
        }
    }
}

/// Ephemeral per-chunk carving bitset (`CarvingMask`, `surface-and-carvers.md` §3.6).
///
/// Bit index for chunk-local `(x, z) in 0..16` and world `y in [min_y, max_y]`:
/// `(y - min_y) + (z + (x << 4)) * height`.
#[derive(Debug, Clone)]
pub struct CarvingMask {
    min_y: i32,
    max_y: i32,
    height: usize,
    words: Vec<u64>,
    any_set: bool,
}

impl CarvingMask {
    /// Allocates an empty `CarvingMask` covering inclusive world Y range `[min_y, max_y]`.
    pub fn new(min_y: i32, max_y: i32) -> Self {
        if min_y > max_y {
            return Self {
                min_y,
                max_y,
                height: 0,
                words: Vec::new(),
                any_set: false,
            };
        }
        let height = (max_y - min_y + 1) as usize;
        let total_bits = 256 * height;
        let word_count = total_bits.div_ceil(64);
        Self {
            min_y,
            max_y,
            height,
            words: vec![0u64; word_count],
            any_set: false,
        }
    }

    /// Inclusive minimum Y covered by this mask.
    #[inline]
    pub const fn min_y(&self) -> i32 {
        self.min_y
    }

    /// Inclusive maximum Y covered by this mask.
    #[inline]
    pub const fn max_y(&self) -> i32 {
        self.max_y
    }

    /// Returns `true` if no voxel has been marked carved.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        !self.any_set
    }

    /// Marks chunk-local `(local_x, world_y, local_z)` as carved if within bounds.
    #[inline]
    pub fn carve(&mut self, local_x: usize, world_y: i32, local_z: usize) {
        if world_y < self.min_y || world_y > self.max_y || local_x >= 16 || local_z >= 16 {
            return;
        }
        let bit = (world_y - self.min_y) as usize + (local_z + (local_x << 4)) * self.height;
        self.words[bit >> 6] |= 1u64 << (bit & 63);
        self.any_set = true;
    }

    /// Returns whether chunk-local `(local_x, world_y, local_z)` is marked carved.
    #[inline]
    pub fn is_carved(&self, local_x: usize, world_y: i32, local_z: usize) -> bool {
        if world_y < self.min_y || world_y > self.max_y || local_x >= 16 || local_z >= 16 {
            return false;
        }
        let bit = (world_y - self.min_y) as usize + (local_z + (local_x << 4)) * self.height;
        (self.words[bit >> 6] & (1u64 << (bit & 63))) != 0
    }

    /// Visits every contiguous vertical carved interval `(local_x, local_z, bottom_y, top_y)`
    /// in ascending bit-index order (`local_x` outer, `local_z` middle, `y` ascending inner),
    /// matching `CarvingMask.visit` (`surface-and-carvers.md` §3.6).
    pub fn visit_runs(&self, mut visitor: impl FnMut(usize, usize, i32, i32)) {
        if !self.any_set || self.height == 0 {
            return;
        }
        let h = self.height;
        for lx in 0..16usize {
            for lz in 0..16usize {
                let col_start = (lz + (lx << 4)) * h;
                let col_end = col_start + h;
                let mut bit = col_start;
                while bit < col_end {
                    // Find next set bit in [bit, col_end).
                    let Some(run_start) = self.next_set_bit(bit, col_end) else {
                        break;
                    };
                    // Find next clear bit in [run_start, col_end).
                    let run_end_excl = self.next_clear_bit(run_start + 1, col_end);
                    let bottom_y = self.min_y + (run_start - col_start) as i32;
                    let top_y = self.min_y + (run_end_excl - 1 - col_start) as i32;
                    visitor(lx, lz, bottom_y, top_y);
                    bit = run_end_excl;
                }
            }
        }
    }

    #[inline]
    fn next_set_bit(&self, start: usize, end: usize) -> Option<usize> {
        let mut word_idx = start >> 6;
        let bit_offset = start & 63;
        let end_word = (end - 1) >> 6;
        let mut w = self.words[word_idx] & (!0u64 << bit_offset);
        loop {
            if w != 0 {
                let pos = (word_idx << 6) + (w.trailing_zeros() as usize);
                return if pos < end { Some(pos) } else { None };
            }
            if word_idx >= end_word {
                return None;
            }
            word_idx += 1;
            w = self.words[word_idx];
        }
    }

    #[inline]
    fn next_clear_bit(&self, start: usize, end: usize) -> usize {
        if start >= end {
            return end;
        }
        let mut word_idx = start >> 6;
        let bit_offset = start & 63;
        let end_word = (end - 1) >> 6;
        let mut w = (!self.words[word_idx]) & (!0u64 << bit_offset);
        loop {
            if w != 0 {
                let pos = (word_idx << 6) + (w.trailing_zeros() as usize);
                return pos.min(end);
            }
            if word_idx >= end_word {
                return end;
            }
            word_idx += 1;
            w = !self.words[word_idx];
        }
    }
}

/// Seeds `rng` using `WorldgenRandom.setLargeFeatureSeed(seed, chunk_x, chunk_z)`
/// (`surface-and-carvers.md` §3.2).
#[inline]
pub fn set_large_feature_seed(rng: &mut LegacyRandom, seed: i64, chunk_x: i32, chunk_z: i32) {
    rng.set_seed(seed);
    let x_scale = rng.next_long();
    let z_scale = rng.next_long();
    let combined =
        i64::from(chunk_x).wrapping_mul(x_scale) ^ i64::from(chunk_z).wrapping_mul(z_scale) ^ seed;
    rng.set_seed(combined);
}

#[inline]
fn can_reach(
    chunk_x: i32,
    chunk_z: i32,
    x: f64,
    z: f64,
    current_step: i32,
    total_steps: i32,
    thickness: f32,
) -> bool {
    let middle_x = f64::from(chunk_x.wrapping_shl(4).wrapping_add(8));
    let middle_z = f64::from(chunk_z.wrapping_shl(4).wrapping_add(8));
    let xd = x - middle_x;
    let zd = z - middle_z;
    let remaining = f64::from(total_steps - current_step);
    let rr = f64::from((thickness + 2.0_f32) + 16.0_f32);
    (xd * xd + zd * zd) - remaining * remaining <= rr * rr
}

#[allow(clippy::too_many_arguments)]
fn carve_ellipsoid(
    chunk_x: i32,
    chunk_z: i32,
    x: f64,
    y: f64,
    z: f64,
    horizontal_radius: f64,
    vertical_radius: f64,
    mask: &mut CarvingMask,
    should_skip: impl Fn(f64, f64, f64, i32) -> bool,
) {
    let min_block_x = chunk_x.wrapping_shl(4);
    let min_block_z = chunk_z.wrapping_shl(4);
    let middle_x = f64::from(min_block_x.wrapping_add(8));
    let middle_z = f64::from(min_block_z.wrapping_add(8));
    let max_delta = 16.0_f64 + horizontal_radius * 2.0_f64;
    if (x - middle_x).abs() > max_delta || (z - middle_z).abs() > max_delta {
        return;
    }

    let min_x = (floor_f64(x - horizontal_radius) - min_block_x - 1).max(0);
    let max_x = (floor_f64(x + horizontal_radius) - min_block_x).min(15);
    let min_y = (floor_f64(y - vertical_radius) - 1).max(mask.min_y());
    let max_y = (floor_f64(y + vertical_radius) + 1).min(mask.max_y());
    let min_z = (floor_f64(z - horizontal_radius) - min_block_z - 1).max(0);
    let max_z = (floor_f64(z + horizontal_radius) - min_block_z).min(15);

    if min_x > max_x || min_z > max_z || max_y <= min_y {
        return;
    }

    for lx in min_x..=max_x {
        let xd = (f64::from(min_block_x + lx) + 0.5_f64 - x) / horizontal_radius;
        let xd2 = xd * xd;
        for lz in min_z..=max_z {
            let zd = (f64::from(min_block_z + lz) + 0.5_f64 - z) / horizontal_radius;
            if xd2 + zd * zd < 1.0_f64 {
                // Strictly world_y > min_y (spec §3.3 step 3 & Edge Case #10).
                let mut world_y = max_y;
                while world_y > min_y {
                    let yd = (f64::from(world_y) - 0.5_f64 - y) / vertical_radius;
                    if !should_skip(xd, yd, zd, world_y) {
                        mask.carve(lx as usize, world_y, lz as usize);
                    }
                    world_y -= 1;
                }
            }
        }
    }
}

impl CaveCarver {
    #[allow(clippy::too_many_arguments)]
    fn carve(
        &self,
        ctx: WorldGenerationContext,
        rng: &mut LegacyRandom,
        chunk_x: i32,
        chunk_z: i32,
        source_chunk_x: i32,
        source_chunk_z: i32,
        mask: &mut CarvingMask,
    ) {
        let max_distance = CARVER_MAX_DISTANCE;
        let cave_count = self.count.sample(rng);
        let src_min_x = source_chunk_x.wrapping_shl(4);
        let src_min_z = source_chunk_z.wrapping_shl(4);

        for _ in 0..cave_count {
            let x = f64::from(src_min_x.wrapping_add(rng.next_int_bounded(16)));
            let y = f64::from(self.y.sample(rng, ctx));
            let z = f64::from(src_min_z.wrapping_add(rng.next_int_bounded(16)));
            let h_mult = f64::from(self.horizontal_radius_multiplier.sample(rng));
            let v_mult = f64::from(self.vertical_radius_multiplier.sample(rng));
            let start_v_mult = f64::from(self.start_vertical_radius_multiplier.sample(rng));
            let floor_level = f64::from(self.floor_level.sample(rng));

            let skip = |xd: f64, yd: f64, zd: f64, _world_y: i32| -> bool {
                yd <= floor_level || (xd * xd + yd * yd + zd * zd) >= 1.0_f64
            };

            let mut tunnels = 1;
            if rng.next_int_bounded(4) == 0 {
                let room_y_scale = f64::from(self.room_vertical_radius_multiplier.sample(rng));
                let room_thickness = 1.0_f32 + rng.next_float() * 6.0_f32;
                let h_rad = 1.5_f64 + f64::from(sin(f64::from(HALF_PI)) * room_thickness);
                let v_rad = h_rad * room_y_scale;
                carve_ellipsoid(
                    chunk_x,
                    chunk_z,
                    x + 1.0_f64,
                    y,
                    z,
                    h_rad,
                    v_rad,
                    mask,
                    skip,
                );
                tunnels += rng.next_int_bounded(4);
            }

            for _ in 0..tunnels {
                let h_rot = rng.next_float() * TWO_PI;
                let v_rot = (rng.next_float() - 0.5_f32) / 4.0_f32;
                let mut thickness = self.thickness.sample(rng);
                if self.weird_thickness_bias && rng.next_int_bounded(10) == 0 {
                    let f1 = rng.next_float();
                    let f2 = rng.next_float();
                    thickness *= f1 * f2 * 3.0_f32 + 1.0_f32;
                }
                let distance = max_distance - rng.next_int_bounded(max_distance / 4);
                let tunnel_seed = rng.next_long();
                Self::create_tunnel(
                    chunk_x,
                    chunk_z,
                    tunnel_seed,
                    x,
                    y,
                    z,
                    h_mult,
                    v_mult,
                    thickness,
                    h_rot,
                    v_rot,
                    0,
                    distance,
                    start_v_mult,
                    mask,
                    floor_level,
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn create_tunnel(
        chunk_x: i32,
        chunk_z: i32,
        tunnel_seed: i64,
        mut x: f64,
        mut y: f64,
        mut z: f64,
        h_mult: f64,
        v_mult: f64,
        thickness: f32,
        mut h_rot: f32,
        mut v_rot: f32,
        step: i32,
        dist: i32,
        y_scale: f64,
        mask: &mut CarvingMask,
        floor_level: f64,
    ) {
        let mut rng = LegacyRandom::new(tunnel_seed);
        let split_point = rng.next_int_bounded(dist / 2) + dist / 4;
        let steep = rng.next_int_bounded(6) == 0;
        let mut y_rota = 0.0_f32;
        let mut x_rota = 0.0_f32;

        let skip = |xd: f64, yd: f64, zd: f64, _world_y: i32| -> bool {
            yd <= floor_level || (xd * xd + yd * yd + zd * zd) >= 1.0_f64
        };

        for current_step in step..dist {
            let h_rad = 1.5_f64
                + f64::from(
                    sin(f64::from((PI * (current_step as f32)) / (dist as f32))) * thickness,
                );
            let v_rad = h_rad * y_scale;
            let cos_x = cos(f64::from(v_rot));
            x += f64::from(cos(f64::from(h_rot)) * cos_x);
            y += f64::from(sin(f64::from(v_rot)));
            z += f64::from(sin(f64::from(h_rot)) * cos_x);

            v_rot *= if steep { 0.92_f32 } else { 0.7_f32 };
            v_rot += x_rota * 0.1_f32;
            h_rot += y_rota * 0.1_f32;
            x_rota *= 0.9_f32;
            y_rota *= 0.75_f32;

            let f1 = rng.next_float();
            let f2 = rng.next_float();
            let f3 = rng.next_float();
            x_rota += ((f1 - f2) * f3) * 2.0_f32;

            let g1 = rng.next_float();
            let g2 = rng.next_float();
            let g3 = rng.next_float();
            y_rota += ((g1 - g2) * g3) * 4.0_f32;

            if current_step == split_point && thickness > 1.0_f32 {
                let seed1 = rng.next_long();
                let thick1 = rng.next_float() * 0.5_f32 + 0.5_f32;
                Self::create_tunnel(
                    chunk_x,
                    chunk_z,
                    seed1,
                    x,
                    y,
                    z,
                    h_mult,
                    v_mult,
                    thick1,
                    h_rot - HALF_PI,
                    v_rot / 3.0_f32,
                    current_step,
                    dist,
                    1.0_f64,
                    mask,
                    floor_level,
                );
                let seed2 = rng.next_long();
                let thick2 = rng.next_float() * 0.5_f32 + 0.5_f32;
                Self::create_tunnel(
                    chunk_x,
                    chunk_z,
                    seed2,
                    x,
                    y,
                    z,
                    h_mult,
                    v_mult,
                    thick2,
                    h_rot + HALF_PI,
                    v_rot / 3.0_f32,
                    current_step,
                    dist,
                    1.0_f64,
                    mask,
                    floor_level,
                );
                return;
            }

            if rng.next_int_bounded(4) != 0 {
                if !can_reach(chunk_x, chunk_z, x, z, current_step, dist, thickness) {
                    return;
                }
                carve_ellipsoid(
                    chunk_x,
                    chunk_z,
                    x,
                    y,
                    z,
                    h_rad * h_mult,
                    v_rad * v_mult,
                    mask,
                    skip,
                );
            }
        }
    }
}

impl CanyonCarver {
    #[allow(clippy::too_many_arguments)]
    fn carve(
        &self,
        ctx: WorldGenerationContext,
        rng: &mut LegacyRandom,
        chunk_x: i32,
        chunk_z: i32,
        source_chunk_x: i32,
        source_chunk_z: i32,
        mask: &mut CarvingMask,
    ) {
        let max_distance = CARVER_MAX_DISTANCE;
        let src_min_x = source_chunk_x.wrapping_shl(4);
        let src_min_z = source_chunk_z.wrapping_shl(4);

        let x = f64::from(src_min_x.wrapping_add(rng.next_int_bounded(16)));
        let y = f64::from(self.y.sample(rng, ctx));
        let z = f64::from(src_min_z.wrapping_add(rng.next_int_bounded(16)));
        let h_rot = rng.next_float() * TWO_PI;
        let v_rot = self.vertical_rotation.sample(rng);
        let y_scale = f64::from(self.shape.y_scale.sample(rng));
        let thickness = self.shape.thickness.sample(rng);
        let distance = ((max_distance as f32) * self.shape.distance_factor.sample(rng)) as i32;
        let tunnel_seed = rng.next_long();

        self.do_carve(
            ctx,
            chunk_x,
            chunk_z,
            tunnel_seed,
            x,
            y,
            z,
            thickness,
            h_rot,
            v_rot,
            0,
            distance,
            y_scale,
            mask,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn do_carve(
        &self,
        ctx: WorldGenerationContext,
        chunk_x: i32,
        chunk_z: i32,
        tunnel_seed: i64,
        mut x: f64,
        mut y: f64,
        mut z: f64,
        thickness: f32,
        mut h_rot: f32,
        mut v_rot: f32,
        step: i32,
        distance: i32,
        y_scale: f64,
        mask: &mut CarvingMask,
    ) {
        let mut rng = LegacyRandom::new(tunnel_seed);
        let gen_depth = ctx.gen_depth.max(0) as usize;
        let mut width_factors = vec![0.0_f32; gen_depth];
        let mut wf = 1.0_f32;
        for (y_index, slot) in width_factors.iter_mut().enumerate() {
            if y_index == 0 || rng.next_int_bounded(self.shape.width_smoothness) == 0 {
                let f1 = rng.next_float();
                let f2 = rng.next_float();
                wf = 1.0_f32 + f1 * f2;
            }
            *slot = wf * wf;
        }

        let mut y_rota = 0.0_f32;
        let mut x_rota = 0.0_f32;
        let min_gen_y = ctx.min_gen_y;

        for current_step in step..distance {
            let mut h_rad = 1.5_f64
                + f64::from(
                    sin(f64::from(((current_step as f32) * PI) / (distance as f32))) * thickness,
                );
            let mut v_rad = h_rad * y_scale;
            h_rad *= f64::from(self.shape.horizontal_radius_factor.sample(&mut rng));

            // updateVerticalRadius
            let v_mult =
                1.0_f32 - (0.5_f32 - (current_step as f32) / (distance as f32)).abs() * 2.0_f32;
            let factor = self.shape.vertical_radius_default_factor
                + self.shape.vertical_radius_center_factor * v_mult;
            let rand_v = rng.next_float() * (1.0_f32 - 0.75_f32) + 0.75_f32;
            v_rad = (f64::from(factor) * v_rad) * f64::from(rand_v);

            let xc = cos(f64::from(v_rot));
            let xs = sin(f64::from(v_rot));
            x += f64::from(cos(f64::from(h_rot)) * xc);
            y += f64::from(xs);
            z += f64::from(sin(f64::from(h_rot)) * xc);

            v_rot *= 0.7_f32;
            v_rot += x_rota * 0.05_f32;
            h_rot += y_rota * 0.05_f32;
            x_rota *= 0.8_f32;
            y_rota *= 0.5_f32;

            let f1 = rng.next_float();
            let f2 = rng.next_float();
            let f3 = rng.next_float();
            x_rota += ((f1 - f2) * f3) * 2.0_f32;

            let g1 = rng.next_float();
            let g2 = rng.next_float();
            let g3 = rng.next_float();
            y_rota += ((g1 - g2) * g3) * 4.0_f32;

            if rng.next_int_bounded(4) != 0 {
                if !can_reach(chunk_x, chunk_z, x, z, current_step, distance, thickness) {
                    return;
                }
                let skip = |xd: f64, yd: f64, zd: f64, world_y: i32| -> bool {
                    let y_index = (world_y - min_gen_y) as usize;
                    let wf_sq = f64::from(width_factors[y_index - 1]);
                    ((xd * xd + zd * zd) * wf_sq + (yd * yd) / 6.0_f64) >= 1.0_f64
                };
                carve_ellipsoid(chunk_x, chunk_z, x, y, z, h_rad, v_rad, mask, skip);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// JSON Codec for `minecraft:worldgen/carver`
// ---------------------------------------------------------------------------

fn probability_codec() -> impl Codec<f32> {
    validated(FloatCodec, |v: &f32| {
        if (0.0_f32..=1.0_f32).contains(v) {
            Ok(())
        } else {
            Err(format!("probability {v} outside of range [0.0, 1.0]"))
        }
    })
}

fn positive_int_codec() -> impl Codec<i32> {
    validated(IntCodec, |v: &i32| {
        if *v >= 1 {
            Ok(())
        } else {
            Err(format!("Value must be positive: {v}"))
        }
    })
}

struct CanyonShapeCodec;
impl Codec<CanyonShape> for CanyonShapeCodec {
    fn decode(&self, input: &Json) -> DataResult<CanyonShape> {
        let mut r = RecordReader::new(input);
        let distance_factor = r.required("distance_factor", &FloatProviderCodec);
        let thickness = r.required("thickness", &FloatProviderCodec);
        let width_smoothness = r.required("width_smoothness", &positive_int_codec());
        let horizontal_radius_factor = r.required("horizontal_radius_factor", &FloatProviderCodec);
        let vertical_radius_default_factor =
            r.required("vertical_radius_default_factor", &FloatCodec);
        let vertical_radius_center_factor =
            r.required("vertical_radius_center_factor", &FloatCodec);
        let y_scale = r.required("y_scale", &FloatProviderCodec);
        r.finish(|| {
            Some(CanyonShape {
                distance_factor: distance_factor?,
                thickness: thickness?,
                width_smoothness: width_smoothness?,
                horizontal_radius_factor: horizontal_radius_factor?,
                vertical_radius_default_factor: vertical_radius_default_factor?,
                vertical_radius_center_factor: vertical_radius_center_factor?,
                y_scale: y_scale?,
            })
        })
    }
    fn encode(&self, _value: &CanyonShape) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

/// Decodes a `minecraft:worldgen/carver` JSON object into a [`Carver`].
pub fn decode_carver(input: &Json) -> DataResult<Carver> {
    decode_dispatch(input, "type", |ty, obj| {
        let id = Identifier::parse(ty).ok()?;
        if id.namespace() != "minecraft" {
            return None;
        }
        match id.path() {
            "cave" => Some({
                let mut r = RecordReader::new(obj);
                let probability = r.required("probability", &probability_codec());
                let y = r.required("y", &HeightProviderCodec);
                let count = r.required("count", &IntProviderCodec);
                let thickness = r.required("thickness", &FloatProviderCodec);
                let weird_thickness_bias = r.optional("weird_thickness_bias", &BoolCodec, false);
                let room_vertical_radius_multiplier =
                    r.required("room_vertical_radius_multiplier", &FloatProviderCodec);
                let horizontal_radius_multiplier =
                    r.required("horizontal_radius_multiplier", &FloatProviderCodec);
                let vertical_radius_multiplier =
                    r.required("vertical_radius_multiplier", &FloatProviderCodec);
                let start_vertical_radius_multiplier = r.optional(
                    "start_vertical_radius_multiplier",
                    &FloatProviderCodec,
                    FloatProvider::Constant(1.0_f32),
                );
                let floor_level = r.required("floor_level", &FloatProviderCodec);
                r.finish(|| {
                    Some(Carver::Cave(CaveCarver {
                        probability: probability?,
                        y: y?,
                        count: count?,
                        thickness: thickness?,
                        weird_thickness_bias,
                        room_vertical_radius_multiplier: room_vertical_radius_multiplier?,
                        horizontal_radius_multiplier: horizontal_radius_multiplier?,
                        vertical_radius_multiplier: vertical_radius_multiplier?,
                        start_vertical_radius_multiplier,
                        floor_level: floor_level?,
                    }))
                })
            }),
            "canyon" => Some({
                let mut r = RecordReader::new(obj);
                let probability = r.required("probability", &probability_codec());
                let y = r.required("y", &HeightProviderCodec);
                let vertical_rotation = r.required("vertical_rotation", &FloatProviderCodec);
                let shape = r.required("shape", &CanyonShapeCodec);
                r.finish(|| {
                    Some(Carver::Canyon(CanyonCarver {
                        probability: probability?,
                        y: y?,
                        vertical_rotation: vertical_rotation?,
                        shape: shape?,
                    }))
                })
            }),
            _ => None,
        }
    })
}
