//! 2D and 3D simplex gradient noise plus fixed-seed biome and End-island constructors.

use crate::noise::octave::{OctaveStack, OctaveStackBuilder};
use crate::noise::primitives::{
    DEFAULT_OFFSET_SCALE, GRADIENT, GradientNoiseState, Interval, Noise, VolumeGrid, floor_to_i32,
    grad_dot_f64,
};
use crate::random::{LegacyRandom, RandomSource};

/// Empirical standard deviation constant of simplex noise (`0x3FDB_3A68_B19A_415F`).
pub const SIMPLEX_STD_DEV: f64 = f64::from_bits(0x3FDB_3A68_B19A_415F);

/// `sqrt(3.0)` (`0x3FFB_B67A_E858_4CAA`).
pub const SQRT_3: f64 = f64::from_bits(0x3FFB_B67A_E858_4CAA);

/// 2D simplex skew factor `F2 = 0.5 * (sqrt(3) - 1)` (`0x3FD7_6CF5_D0B0_9954`).
pub const F2: f64 = f64::from_bits(0x3FD7_6CF5_D0B0_9954);

/// 2D simplex unskew factor `G2 = (3 - sqrt(3)) / 6` (`0x3FCB_0CB1_74DF_99C8`).
pub const G2: f64 = f64::from_bits(0x3FCB_0CB1_74DF_99C8);

/// 3D simplex skew factor `F3 = 1 / 3` (`0x3FD5_5555_5555_5555`).
pub const F3: f64 = f64::from_bits(0x3FD5_5555_5555_5555);

/// 3D simplex unskew factor `G3 = 1 / 6` (`0x3FC5_5555_5555_5555`).
pub const G3: f64 = f64::from_bits(0x3FC5_5555_5555_5555);

/// Single-octave 2D and 3D simplex gradient noise.
///
/// Unlike Perlin noise, simplex noise never wraps input coordinates, evaluates all internal
/// arithmetic in `f64`, reduces gradient hashes modulo `12`, and narrows only the final scaled
/// value to `f32`.
#[derive(Debug, Clone, PartialEq)]
pub struct SimplexNoise {
    state: GradientNoiseState,
}

impl SimplexNoise {
    /// Constructs a new simplex noise from `rng` using the default offset scale `256.0`.
    pub fn new<R: RandomSource>(rng: &mut R) -> Self {
        Self::with_offset_scale(rng, DEFAULT_OFFSET_SCALE)
    }

    /// Constructs a simplex noise with zeroed origin offsets (`offset_scale = 0.0`).
    ///
    /// Still consumes three `next_double()` draws (multiplied by `0.0`) before the 256-step
    /// permutation shuffle, matching vanilla RNG advancement.
    pub fn new_without_offset<R: RandomSource>(rng: &mut R) -> Self {
        Self::with_offset_scale(rng, 0.0)
    }

    /// Constructs a simplex noise with an explicit `offset_scale`.
    pub fn with_offset_scale<R: RandomSource>(rng: &mut R, offset_scale: f64) -> Self {
        Self {
            state: GradientNoiseState::new(rng, offset_scale),
        }
    }

    /// Constructs a simplex noise from an existing [`GradientNoiseState`].
    pub const fn from_state(state: GradientNoiseState) -> Self {
        Self { state }
    }

    /// Returns the underlying gradient-noise state.
    #[inline]
    pub const fn state(&self) -> &GradientNoiseState {
        &self.state
    }

    /// Returns the parity configuration summary string for this octave.
    pub fn parity_config_string(&self) -> String {
        self.state.parity_config_string()
    }

    /// Conservative `f32` output interval `[-2.0, 2.0]`.
    #[inline]
    pub const fn range(&self) -> Interval {
        Interval::Bounds {
            min: -2.0_f32,
            max: 2.0_f32,
        }
    }

    #[inline]
    fn corner_noise(grad_index: usize, x: f64, y: f64, z: f64, falloff_base: f64) -> f64 {
        let t = ((falloff_base - x * x) - y * y) - z * z;
        if t < 0.0 {
            0.0
        } else {
            let t2 = t * t;
            (t2 * t2) * grad_dot_f64(GRADIENT[grad_index], x, y, z)
        }
    }

    /// Samples 2D simplex noise at `(x_in, y_in)` without coordinate wrapping.
    pub fn sample_2d(&self, x_in: f64, y_in: f64) -> f32 {
        let x = x_in + self.state.xo();
        let y = y_in + self.state.yo();

        let s = (x + y) * F2;
        let i = floor_to_i32(x + s);
        let j = floor_to_i32(y + s);

        let t = f64::from(i.wrapping_add(j)) * G2;
        let x0_origin = f64::from(i) - t;
        let y0_origin = f64::from(j) - t;
        let x0 = x - x0_origin;
        let y0 = y - y0_origin;

        let (i1, j1): (i32, i32) = if x0 > y0 { (1, 0) } else { (0, 1) };

        let x1 = (x0 - f64::from(i1)) + G2;
        let y1 = (y0 - f64::from(j1)) + G2;
        let x2 = (x0 - 1.0) + (2.0 * G2);
        let y2 = (y0 - 1.0) + (2.0 * G2);

        let im = i & 0xFF;
        let jm = j & 0xFF;

        let g0 = (self.state.hash(im.wrapping_add(self.state.hash(jm))) % 12) as usize;
        let g1 = (self.state.hash(
            im.wrapping_add(i1)
                .wrapping_add(self.state.hash(jm.wrapping_add(j1))),
        ) % 12) as usize;
        let g2 = (self.state.hash(
            im.wrapping_add(1)
                .wrapping_add(self.state.hash(jm.wrapping_add(1))),
        ) % 12) as usize;

        let n0 = Self::corner_noise(g0, x0, y0, 0.0, 0.5);
        let n1 = Self::corner_noise(g1, x1, y1, 0.0, 0.5);
        let n2 = Self::corner_noise(g2, x2, y2, 0.0, 0.5);

        (70.0 * ((n0 + n1) + n2)) as f32
    }

    /// Samples 3D simplex noise at `(x_in, y_in, z_in)` without coordinate wrapping.
    pub fn sample(&self, x_in: f64, y_in: f64, z_in: f64) -> f32 {
        let x = x_in + self.state.xo();
        let y = y_in + self.state.yo();
        let z = z_in + self.state.zo();

        let s = ((x + y) + z) * F3;
        let i = floor_to_i32(x + s);
        let j = floor_to_i32(y + s);
        let k = floor_to_i32(z + s);

        let t = f64::from(i.wrapping_add(j).wrapping_add(k)) * G3;
        let x0 = x - (f64::from(i) - t);
        let y0 = y - (f64::from(j) - t);
        let z0 = z - (f64::from(k) - t);

        let (i1, j1, k1, i2, j2, k2): (i32, i32, i32, i32, i32, i32) = if x0 >= y0 {
            if y0 >= z0 {
                (1, 0, 0, 1, 1, 0)
            } else if x0 >= z0 {
                (1, 0, 0, 1, 0, 1)
            } else {
                (0, 0, 1, 1, 0, 1)
            }
        } else if y0 < z0 {
            (0, 0, 1, 0, 1, 1)
        } else if x0 < z0 {
            (0, 1, 0, 0, 1, 1)
        } else {
            (0, 1, 0, 1, 1, 0)
        };

        let x1 = (x0 - f64::from(i1)) + G3;
        let y1 = (y0 - f64::from(j1)) + G3;
        let z1 = (z0 - f64::from(k1)) + G3;

        let x2 = (x0 - f64::from(i2)) + F3;
        let y2 = (y0 - f64::from(j2)) + F3;
        let z2 = (z0 - f64::from(k2)) + F3;

        let x3 = (x0 - 1.0) + 0.5;
        let y3 = (y0 - 1.0) + 0.5;
        let z3 = (z0 - 1.0) + 0.5;

        let im = i & 0xFF;
        let jm = j & 0xFF;
        let km = k & 0xFF;

        let g0 = (self
            .state
            .hash(im.wrapping_add(self.state.hash(jm.wrapping_add(self.state.hash(km)))))
            % 12) as usize;
        let g1 = (self.state.hash(
            im.wrapping_add(i1).wrapping_add(
                self.state.hash(
                    jm.wrapping_add(j1)
                        .wrapping_add(self.state.hash(km.wrapping_add(k1))),
                ),
            ),
        ) % 12) as usize;
        let g2 = (self.state.hash(
            im.wrapping_add(i2).wrapping_add(
                self.state.hash(
                    jm.wrapping_add(j2)
                        .wrapping_add(self.state.hash(km.wrapping_add(k2))),
                ),
            ),
        ) % 12) as usize;
        let g3 = (self.state.hash(
            im.wrapping_add(1).wrapping_add(
                self.state.hash(
                    jm.wrapping_add(1)
                        .wrapping_add(self.state.hash(km.wrapping_add(1))),
                ),
            ),
        ) % 12) as usize;

        let n0 = Self::corner_noise(g0, x0, y0, z0, 0.6);
        let n1 = Self::corner_noise(g1, x1, y1, z1, 0.6);
        let n2 = Self::corner_noise(g2, x2, y2, z2, 0.6);
        let n3 = Self::corner_noise(g3, x3, y3, z3, 0.6);

        (32.0 * (((n0 + n1) + n2) + n3)) as f32
    }

    /// Accumulates `amplitude * sample(x * scale_xz, y * scale_y, z * scale_xz)` into `buffer`
    /// over `grid` in `(z, x, y)` order.
    ///
    /// # Panics
    ///
    /// Panics if `buffer.len() < grid.len()`.
    pub fn accumulate_volume(
        &self,
        grid: &VolumeGrid,
        scale_xz: f64,
        scale_y: f64,
        amplitude: f32,
        buffer: &mut [f32],
    ) {
        <Self as Noise>::accumulate_volume(self, grid, scale_xz, scale_y, amplitude, buffer);
    }
}

impl Noise for SimplexNoise {
    #[inline]
    fn sample(&self, x: f64, y: f64, z: f64) -> f32 {
        SimplexNoise::sample(self, x, y, z)
    }

    #[inline]
    fn sample_2d(&self, x: f64, y: f64) -> f32 {
        SimplexNoise::sample_2d(self, x, y)
    }

    #[inline]
    fn range(&self) -> Interval {
        SimplexNoise::range(self)
    }
}

/// Fixed-seed biome temperature simplex noise (`LegacyRandom(1234)`, zero offset scale).
pub fn biome_temperature_noise() -> SimplexNoise {
    let mut rng = LegacyRandom::new(1234);
    SimplexNoise::new_without_offset(&mut rng)
}

/// Fixed-seed biome info simplex noise (`LegacyRandom(2345)`, zero offset scale).
pub fn biome_info_noise() -> SimplexNoise {
    let mut rng = LegacyRandom::new(2345);
    SimplexNoise::new_without_offset(&mut rng)
}

/// Fixed-seed frozen-ocean temperature 3-layer simplex octave stack (`LegacyRandom(3456)`, zero
/// offset scale, frequencies `1.0 / 0.5 / 0.25`, amplitudes `0.14285715 / 0.2857143 / 0.5714286`).
pub fn biome_frozen_temperature_noise() -> OctaveStack {
    let mut rng = LegacyRandom::new(3456);
    let mut builder = OctaveStackBuilder::new();
    builder.add_layer(
        SimplexNoise::new_without_offset(&mut rng),
        1.0,
        0.142_857_15_f32,
    );
    builder.add_layer(
        SimplexNoise::new_without_offset(&mut rng),
        0.5,
        0.285_714_3_f32,
    );
    builder.add_layer(
        SimplexNoise::new_without_offset(&mut rng),
        0.25,
        0.571_428_6_f32,
    );
    builder.build()
}

/// End-island simplex noise (`LegacyRandom(world_seed)`, `consume_count(17292)`, zero offset
/// scale).
pub fn end_island_noise(world_seed: i64) -> SimplexNoise {
    let mut rng = LegacyRandom::new(world_seed);
    rng.consume_count(17_292);
    SimplexNoise::new_without_offset(&mut rng)
}
