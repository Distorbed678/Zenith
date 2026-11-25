//! Blended terrain noise (`minecraft:old_blended_noise`) combining three smeared-Perlin octave
//! stacks via clamped linear interpolation with exact endpoint short-circuiting.

use crate::noise::octave::{OctaveStack, OctaveStackBuilder};
use crate::noise::primitives::{Interval, NoiseError, VolumeGrid, lerp};
use crate::noise::smeared::SmearedPerlinNoise;
use crate::random::{LegacyRandom, PositionalRandomFactory, RandomSource};

/// Density function type ID for blended noise.
pub const BLENDED_NOISE_ID: &str = "minecraft:old_blended_noise";

/// Worldgen positional RNG hash identifier for blended noise.
pub const BLENDED_NOISE_SEED_ID: &str = "minecraft:terrain";

/// Base coordinate scale `B = 684.412` (`0x4085_634B_C6A7_EF9E`).
pub const BLENDED_BASE_SCALE: f64 = f64::from_bits(0x4085_634B_C6A7_EF9E);

/// Limit-stack value factor: the `f32` literal `0.99998474_f32` (`0x3F7F_FF00`) widened to `f64`,
/// equal to exact `65535.0 / 65536.0 = 0.9999847412109375`.
pub const BLENDED_LIMIT_FACTOR: f64 = f32::from_bits(0x3F7F_FF00) as f64;

/// Main selector stack value factor `12.75` (`0x4029_8000_0000_0000`).
pub const BLENDED_MAIN_FACTOR: f64 = 12.75;

/// First octave exponent for the lower-limit and upper-limit stacks (`-15`, 16 octaves).
pub const BLENDED_LIMIT_FIRST_OCTAVE: i32 = -15;

/// First octave exponent for the main selector stack (`-7`, 8 octaves).
pub const BLENDED_MAIN_FIRST_OCTAVE: i32 = -7;

/// Validated 5-parameter configuration for [`BlendedNoise`] (`minecraft:old_blended_noise`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlendedNoiseParameters {
    xz_scale: f64,
    y_scale: f64,
    xz_factor: f64,
    y_factor: f64,
    smear_scale_multiplier: f64,
}

impl BlendedNoiseParameters {
    /// Overworld preset `(0.25, 0.125, 80.0, 160.0, 8.0)`.
    pub const OVERWORLD: Self = Self {
        xz_scale: 0.25,
        y_scale: 0.125,
        xz_factor: 80.0,
        y_factor: 160.0,
        smear_scale_multiplier: 8.0,
    };

    /// Nether preset `(0.25, 0.375, 80.0, 60.0, 8.0)`.
    pub const NETHER: Self = Self {
        xz_scale: 0.25,
        y_scale: 0.375,
        xz_factor: 80.0,
        y_factor: 60.0,
        smear_scale_multiplier: 8.0,
    };

    /// End preset `(0.25, 0.25, 80.0, 160.0, 4.0)`.
    pub const END: Self = Self {
        xz_scale: 0.25,
        y_scale: 0.25,
        xz_factor: 80.0,
        y_factor: 160.0,
        smear_scale_multiplier: 4.0,
    };

    /// Creates and validates blended-noise parameters:
    /// - `xz_scale`, `y_scale`, `xz_factor`, `y_factor` in `[0.001, 1000.0]`
    /// - `smear_scale_multiplier` in `[1.0, 8.0]`
    pub fn new(
        xz_scale: f64,
        y_scale: f64,
        xz_factor: f64,
        y_factor: f64,
        smear_scale_multiplier: f64,
    ) -> Result<Self, NoiseError> {
        let check_scale = |name: &'static str, value: f64| {
            if (0.001..=1000.0).contains(&value) {
                Ok(())
            } else {
                Err(NoiseError::InvalidBlendedParameter { name, value })
            }
        };
        check_scale("xz_scale", xz_scale)?;
        check_scale("y_scale", y_scale)?;
        check_scale("xz_factor", xz_factor)?;
        check_scale("y_factor", y_factor)?;
        if !(1.0..=8.0).contains(&smear_scale_multiplier) {
            return Err(NoiseError::InvalidBlendedParameter {
                name: "smear_scale_multiplier",
                value: smear_scale_multiplier,
            });
        }
        Ok(Self {
            xz_scale,
            y_scale,
            xz_factor,
            y_factor,
            smear_scale_multiplier,
        })
    }

    /// Horizontal scale `s_xz`.
    #[inline]
    pub const fn xz_scale(self) -> f64 {
        self.xz_scale
    }

    /// Vertical scale `s_y`.
    #[inline]
    pub const fn y_scale(self) -> f64 {
        self.y_scale
    }

    /// Horizontal selector divisor `f_xz`.
    #[inline]
    pub const fn xz_factor(self) -> f64 {
        self.xz_factor
    }

    /// Vertical selector divisor `f_y`.
    #[inline]
    pub const fn y_factor(self) -> f64 {
        self.y_factor
    }

    /// Y-smear step multiplier `m_s`.
    #[inline]
    pub const fn smear_scale_multiplier(self) -> f64 {
        self.smear_scale_multiplier
    }

    /// Computes the conservative `f32` output interval for blended noise with these parameters.
    ///
    /// Runs the 16-iteration high-to-low loop matching the lower-limit stack
    /// (`first_octave = -15`, `smear_scale_y = B * y_scale * smear_scale_multiplier`,
    /// `value_factor = BLENDED_LIMIT_FACTOR`).
    pub fn range(self) -> Interval {
        let y_lim = (BLENDED_BASE_SCALE * self.y_scale) * self.smear_scale_multiplier;
        let n = (-BLENDED_LIMIT_FIRST_OCTAVE + 1) as usize;
        let mut freq = 1.0_f64;
        let mut amp = BLENDED_LIMIT_FACTOR / (2.0_f64.powi(n as i32) - 1.0);
        let mut acc = Interval::exact(0.0_f32);
        for _ in 0..n {
            let layer_range = SmearedPerlinNoise::range_for_scale(y_lim * freq);
            acc = acc + (layer_range * Interval::exact(amp as f32));
            freq /= 2.0;
            amp *= 2.0;
        }
        acc
    }
}

/// Builds a high-to-low smeared-Perlin [`OctaveStack`] from `rng`.
///
/// Requires `first_octave <= 0`. Creates `n = -first_octave + 1` smeared-Perlin octaves from
/// frequency `1.0` down to `2^first_octave` with amplitudes doubling from
/// `value_factor / (2^n - 1)`.
pub fn build_smeared_fbm<R: RandomSource>(
    rng: &mut R,
    first_octave: i32,
    smear_scale_y: f64,
    value_factor: f64,
) -> Result<OctaveStack, NoiseError> {
    if first_octave > 0 {
        return Err(NoiseError::PositiveSmearedFirstOctave(first_octave));
    }
    let n = (-i64::from(first_octave) + 1) as usize;
    let mut freq = 1.0_f64;
    let mut amp = value_factor / (2.0_f64.powi(n as i32) - 1.0);

    let mut builder = OctaveStackBuilder::with_capacity(n);
    for _ in 0..n {
        let q_y = smear_scale_y * freq;
        let noise = SmearedPerlinNoise::new(rng, q_y);
        builder.add_layer(noise, freq, amp as f32);
        freq /= 2.0;
        amp *= 2.0;
    }

    Ok(builder.build())
}

/// Blended terrain noise (`minecraft:old_blended_noise`).
///
/// Combines a 16-octave lower-limit stack, a 16-octave upper-limit stack, and an 8-octave main
/// selector stack via clamped linear interpolation with exact `0.0_f32` and `1.0_f32` endpoint
/// short-circuiting.
#[derive(Debug, Clone, PartialEq)]
pub struct BlendedNoise {
    parameters: BlendedNoiseParameters,
    lower_limit: OctaveStack,
    upper_limit: OctaveStack,
    main_selector: OctaveStack,
    xz_multiplier: f64,
    y_multiplier: f64,
    xz_scale_main: f64,
    y_scale_main: f64,
    range: Interval,
}

impl BlendedNoise {
    /// Constructs a blended noise sequentially from `rng` (lower-limit, upper-limit, then main
    /// selector — 40 smeared Perlin octaves in total).
    pub fn new<R: RandomSource>(rng: &mut R, parameters: BlendedNoiseParameters) -> Self {
        let xz_multiplier = BLENDED_BASE_SCALE * parameters.xz_scale;
        let y_multiplier = BLENDED_BASE_SCALE * parameters.y_scale;
        let y_lim = y_multiplier * parameters.smear_scale_multiplier;
        let y_main = y_lim / parameters.y_factor;
        let xz_scale_main = xz_multiplier / parameters.xz_factor;
        let y_scale_main = y_multiplier / parameters.y_factor;

        let lower_limit =
            build_smeared_fbm(rng, BLENDED_LIMIT_FIRST_OCTAVE, y_lim, BLENDED_LIMIT_FACTOR)
                .expect("BLENDED_LIMIT_FIRST_OCTAVE (-15) is <= 0");
        let upper_limit =
            build_smeared_fbm(rng, BLENDED_LIMIT_FIRST_OCTAVE, y_lim, BLENDED_LIMIT_FACTOR)
                .expect("BLENDED_LIMIT_FIRST_OCTAVE (-15) is <= 0");
        let main_selector =
            build_smeared_fbm(rng, BLENDED_MAIN_FIRST_OCTAVE, y_main, BLENDED_MAIN_FACTOR)
                .expect("BLENDED_MAIN_FIRST_OCTAVE (-7) is <= 0");

        let range = parameters.range();

        Self {
            parameters,
            lower_limit,
            upper_limit,
            main_selector,
            xz_multiplier,
            y_multiplier,
            xz_scale_main,
            y_scale_main,
            range,
        }
    }

    /// Constructs a blended noise for a legacy-RNG dimension seeded directly from `world_seed`.
    pub fn from_legacy_world_seed(world_seed: i64, parameters: BlendedNoiseParameters) -> Self {
        let mut rng = LegacyRandom::new(world_seed);
        Self::new(&mut rng, parameters)
    }

    /// Constructs a blended noise from a world positional random factory via
    /// `from_hash_of("minecraft:terrain")`.
    pub fn from_positional_factory<F: PositionalRandomFactory>(
        factory: &F,
        parameters: BlendedNoiseParameters,
    ) -> Self {
        let mut rng = factory.from_hash_of(BLENDED_NOISE_SEED_ID);
        Self::new(&mut rng, parameters)
    }

    /// Returns the parameters used to construct this blended noise.
    #[inline]
    pub const fn parameters(&self) -> BlendedNoiseParameters {
        self.parameters
    }

    /// Returns the 16-octave lower-limit smeared-Perlin stack.
    #[inline]
    pub const fn lower_limit(&self) -> &OctaveStack {
        &self.lower_limit
    }

    /// Returns the 16-octave upper-limit smeared-Perlin stack.
    #[inline]
    pub const fn upper_limit(&self) -> &OctaveStack {
        &self.upper_limit
    }

    /// Returns the 8-octave main selector smeared-Perlin stack.
    #[inline]
    pub const fn main_selector(&self) -> &OctaveStack {
        &self.main_selector
    }

    /// Returns the conservative `f32` output interval.
    #[inline]
    pub const fn range(&self) -> Interval {
        self.range
    }

    /// Samples blended noise at integer block coordinates `(x, y, z)`.
    ///
    /// Short-circuits exact endpoints (`alpha == 0.0_f32` returns the lower limit directly;
    /// `alpha == 1.0_f32` returns the upper limit directly).
    pub fn sample(&self, x: i32, y: i32, z: i32) -> f32 {
        let xf = f64::from(x);
        let yf = f64::from(y);
        let zf = f64::from(z);

        let main_val = self.main_selector.sample(
            xf * self.xz_scale_main,
            yf * self.y_scale_main,
            zf * self.xz_scale_main,
        );
        let alpha = clamp_unit_f32(main_val + 0.5_f32);

        if alpha == 0.0_f32 {
            self.lower_limit.sample(
                xf * self.xz_multiplier,
                yf * self.y_multiplier,
                zf * self.xz_multiplier,
            )
        } else if alpha == 1.0_f32 {
            self.upper_limit.sample(
                xf * self.xz_multiplier,
                yf * self.y_multiplier,
                zf * self.xz_multiplier,
            )
        } else {
            let a = self.lower_limit.sample(
                xf * self.xz_multiplier,
                yf * self.y_multiplier,
                zf * self.xz_multiplier,
            );
            let b = self.upper_limit.sample(
                xf * self.xz_multiplier,
                yf * self.y_multiplier,
                zf * self.xz_multiplier,
            );
            lerp(alpha, a, b)
        }
    }

    /// Evaluates blended noise over `grid`, writing results into `buffer[0..grid.len()]`.
    ///
    /// Uses volume accumulation with outer amplitude `1.0_f32` for the main, lower-limit, and
    /// upper-limit stacks, then applies the exact `alpha == 0.0` / `alpha == 1.0` / `lerp`
    /// selection per voxel.
    ///
    /// # Panics
    ///
    /// Panics if `buffer.len() < grid.len()`.
    pub fn fill_volume(&self, grid: &VolumeGrid, buffer: &mut [f32]) {
        let len = grid.len();
        assert!(
            buffer.len() >= len,
            "volume buffer length {} is smaller than grid size {}",
            buffer.len(),
            len
        );

        let mut main_buf = vec![0.0_f32; len];
        self.main_selector.accumulate_volume(
            grid,
            self.xz_scale_main,
            self.y_scale_main,
            1.0_f32,
            &mut main_buf,
        );

        let mut need_lower = false;
        let mut need_upper = false;
        for &m in &main_buf {
            let alpha = clamp_unit_f32(m + 0.5_f32);
            if alpha != 1.0_f32 {
                need_lower = true;
            }
            if alpha != 0.0_f32 {
                need_upper = true;
            }
            if need_lower && need_upper {
                break;
            }
        }

        let lower_buf = if need_lower {
            let mut b = vec![0.0_f32; len];
            self.lower_limit.accumulate_volume(
                grid,
                self.xz_multiplier,
                self.y_multiplier,
                1.0_f32,
                &mut b,
            );
            b
        } else {
            Vec::new()
        };

        let upper_buf = if need_upper {
            let mut b = vec![0.0_f32; len];
            self.upper_limit.accumulate_volume(
                grid,
                self.xz_multiplier,
                self.y_multiplier,
                1.0_f32,
                &mut b,
            );
            b
        } else {
            Vec::new()
        };

        for i in 0..len {
            let alpha = clamp_unit_f32(main_buf[i] + 0.5_f32);
            buffer[i] = if alpha == 0.0_f32 {
                lower_buf[i]
            } else if alpha == 1.0_f32 {
                upper_buf[i]
            } else {
                lerp(alpha, lower_buf[i], upper_buf[i])
            };
        }
    }
}

/// Java `Mth.clamp(v, 0.0f, 1.0f)`: `0.0f` if `v < 0.0f`, else `Math.min(v, 1.0f)` (preserving
/// `NaN` and `-0.0f`).
#[inline]
pub(crate) fn clamp_unit_f32(v: f32) -> f32 {
    v.clamp(0.0_f32, 1.0_f32)
}
