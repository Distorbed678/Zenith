//! Double-Perlin ("normal") noise parameters, normalization modes, parity constructor, and
//! standard / legacy-Nether instantiation.

use std::f64::consts::SQRT_2;

use crate::noise::octave::{OctaveStack, OctaveStackBuilder, build_legacy_fbm};
use crate::noise::perlin::{PERLIN_STD_DEV, PerlinNoise};
use crate::noise::primitives::{Interval, Noise, NoiseError, VolumeGrid};
use crate::random::{LegacyRandom, PositionalRandomFactory, RandomSource};

/// Frequency multiplier applied to the second Perlin octave in each pair:
/// `337.0 / 331.0 = 1.0181268882175227` (`0x3FF0_4A3F_6B81_28FE`).
pub const NORMAL_INPUT_FACTOR: f64 = f64::from_bits(0x3FF0_4A3F_6B81_28FE);

/// Target deviation ratio `1.0 / 3.0 = 0.3333333333333333` (`0x3FD5_5555_5555_5555`).
pub const NORMAL_TARGET_DEVIATION: f64 = f64::from_bits(0x3FD5_5555_5555_5555);

/// Minimum valid `base_amplitude`: the `f32` literal `1.0e-5_f32` (`0x3727_C5AC`) widened to
/// `f64` (`9.999999747378752e-6`).
pub const MIN_BASE_AMPLITUDE: f64 = f32::from_bits(0x3727_C5AC) as f64;

/// Maximum valid `base_amplitude` and `amplitude_modifiers` entry (`1,000,000.0`).
pub const MAX_AMPLITUDE: f64 = 1_000_000.0;

/// Minimum valid `base_octave` (`-32`).
pub const MIN_BASE_OCTAVE: i32 = -32;

/// Maximum valid `base_octave` (`32`).
pub const MAX_BASE_OCTAVE: i32 = 32;

/// Maximum valid `octave_count` (`32`).
pub const MAX_OCTAVES: usize = 32;

/// Registry key for the legacy Nether temperature noise (seeded with `world_seed + 0`).
pub const NETHER_TEMPERATURE_ID: &str = "minecraft:nether/temperature";

/// Registry key for the legacy Nether vegetation noise (seeded with `world_seed + 1`).
pub const NETHER_VEGETATION_ID: &str = "minecraft:nether/vegetation";

/// Three-mode normalization setting for `minecraft:worldgen/noise`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum NormalizationMode {
    /// `normalize: true` — applies geometric octave scaling and standard-deviation normalization.
    #[default]
    Enabled,
    /// `normalize: false` — skips geometric octave scaling (`0.5^{-(n-1)} / (0.5^{-n} - 1)`),
    /// while still computing and applying standard-deviation normalization.
    Disabled,
    /// `normalize: "legacy"` — applies geometric octave scaling and overrides the normalization
    /// factor with the legacy span-based formula when `N_std != 0.0`.
    Legacy,
}

/// Precomputed metadata for one non-zero octave in [`NormalNoiseParameters`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActiveOctave {
    /// Signed octave exponent `k = base_octave + i`.
    pub octave: i32,
    /// Layer frequency `2.0^k`.
    pub frequency: f64,
    /// Pre-normalization octave amplitude `a * modifier`.
    pub amplitude: f64,
}

/// Immutable double-Perlin noise specification (`minecraft:worldgen/noise`).
///
/// Precomputes active octaves, target amplitude, normalization factor, and conservative range
/// once at construction so worldgen can cache and instantiate stacks cheaply.
#[derive(Debug, Clone, PartialEq)]
pub struct NormalNoiseParameters {
    base_amplitude: f64,
    base_octave: i32,
    octave_count: usize,
    normalize: NormalizationMode,
    amplitude_modifiers: Vec<f64>,
    active_octaves: Vec<ActiveOctave>,
    target_amplitude: f64,
    normalization_factor: f64,
    range: Interval,
}

impl NormalNoiseParameters {
    /// Constructs and validates a `NormalNoiseParameters` specification from its full JSON fields.
    ///
    /// Validation matches `minecraft:worldgen/noise`:
    /// - `base_amplitude` in `[MIN_BASE_AMPLITUDE, 1_000_000.0]`
    /// - `base_octave` in `[-32, 32]`
    /// - `octave_count` in `[1, 32]`
    /// - `amplitude_modifiers` either empty or of length `octave_count`, each entry in
    ///   `[0.0, 1_000_000.0]`
    pub fn new(
        base_amplitude: f64,
        base_octave: i32,
        octave_count: usize,
        normalize: NormalizationMode,
        amplitude_modifiers: Vec<f64>,
    ) -> Result<Self, NoiseError> {
        if !(MIN_BASE_AMPLITUDE..=MAX_AMPLITUDE).contains(&base_amplitude) {
            return Err(NoiseError::InvalidBaseAmplitude(base_amplitude));
        }
        if !(MIN_BASE_OCTAVE..=MAX_BASE_OCTAVE).contains(&base_octave) {
            return Err(NoiseError::InvalidBaseOctave(base_octave));
        }
        if !(1..=MAX_OCTAVES).contains(&octave_count) {
            return Err(NoiseError::InvalidOctaveCount(octave_count));
        }
        if !amplitude_modifiers.is_empty() && amplitude_modifiers.len() != octave_count {
            return Err(NoiseError::AmplitudeModifierCountMismatch {
                expected: octave_count,
                actual: amplitude_modifiers.len(),
            });
        }
        for (index, &value) in amplitude_modifiers.iter().enumerate() {
            if !(0.0..=MAX_AMPLITUDE).contains(&value) {
                return Err(NoiseError::InvalidAmplitudeModifier { index, value });
            }
        }

        let (active_octaves, mut target_amplitude, n_std) = compute_standard_normalization(
            base_amplitude,
            base_octave,
            octave_count,
            normalize,
            &amplitude_modifiers,
        );

        let normalization_factor = if normalize == NormalizationMode::Legacy && n_std != 0.0 {
            let span = active_span(octave_count, &amplitude_modifiers)
                .expect("non-zero n_std guarantees at least one active octave");
            let n_leg = legacy_normalization_factor(base_amplitude, span);
            target_amplitude *= n_leg / n_std;
            n_leg
        } else {
            n_std
        };

        let radius = ((target_amplitude * NORMAL_TARGET_DEVIATION) * 6.0) as f32;
        let range = Interval::symmetric(radius);

        Ok(Self {
            base_amplitude,
            base_octave,
            octave_count,
            normalize,
            amplitude_modifiers,
            active_octaves,
            target_amplitude,
            normalization_factor,
            range,
        })
    }

    /// Convenience constructor from `(first_octave, amplitudes, normalize)` with
    /// `base_amplitude = 1.0`.
    pub fn from_octaves(
        first_octave: i32,
        amplitudes: Vec<f64>,
        normalize: NormalizationMode,
    ) -> Result<Self, NoiseError> {
        let octave_count = amplitudes.len();
        Self::new(1.0, first_octave, octave_count, normalize, amplitudes)
    }

    /// Parity constructor used for built-in vanilla worldgen noise definitions.
    ///
    /// Computes `N_new` (with `A_0 = 1.0, normalize = Enabled`) and `N_old` (legacy formula with
    /// `A_0 = 1.0`), sets `base_amplitude = N_old / N_new` (or `1.0` if `N_new == 0.0`), and
    /// re-runs normalization with that `base_amplitude` so intermediate rounding is preserved.
    /// Collapses `amplitude_modifiers` to empty when every entry equals `1.0`.
    pub fn parity(first_octave: i32, amplitudes: Vec<f64>) -> Result<Self, NoiseError> {
        let octave_count = amplitudes.len();
        if !(MIN_BASE_OCTAVE..=MAX_BASE_OCTAVE).contains(&first_octave) {
            return Err(NoiseError::InvalidBaseOctave(first_octave));
        }
        if !(1..=MAX_OCTAVES).contains(&octave_count) {
            return Err(NoiseError::InvalidOctaveCount(octave_count));
        }
        for (index, &value) in amplitudes.iter().enumerate() {
            if !(0.0..=MAX_AMPLITUDE).contains(&value) {
                return Err(NoiseError::InvalidAmplitudeModifier { index, value });
            }
        }

        let modifiers = if amplitudes.iter().all(|&a| a == 1.0) {
            Vec::new()
        } else {
            amplitudes
        };

        let (_, _, n_new) = compute_standard_normalization(
            1.0,
            first_octave,
            octave_count,
            NormalizationMode::Enabled,
            &modifiers,
        );

        let base_amplitude = if n_new == 0.0 {
            1.0
        } else {
            let span = active_span(octave_count, &modifiers)
                .expect("non-zero n_new guarantees at least one active octave");
            let n_old = legacy_normalization_factor(1.0, span);
            n_old / n_new
        };

        Self::new(
            base_amplitude,
            first_octave,
            octave_count,
            NormalizationMode::Enabled,
            modifiers,
        )
    }

    /// Base amplitude `A_0`.
    #[inline]
    pub const fn base_amplitude(&self) -> f64 {
        self.base_amplitude
    }

    /// Lowest octave exponent `o_0`.
    #[inline]
    pub const fn base_octave(&self) -> i32 {
        self.base_octave
    }

    /// Total number of octaves `n`.
    #[inline]
    pub const fn octave_count(&self) -> usize {
        self.octave_count
    }

    /// Normalization mode (`Enabled`, `Disabled`, or `Legacy`).
    #[inline]
    pub const fn normalize(&self) -> NormalizationMode {
        self.normalize
    }

    /// Stored amplitude modifiers (empty means all `1.0`).
    #[inline]
    pub fn amplitude_modifiers(&self) -> &[f64] {
        &self.amplitude_modifiers
    }

    /// Active (non-zero) octave metadata in ascending octave order.
    #[inline]
    pub fn active_octaves(&self) -> &[ActiveOctave] {
        &self.active_octaves
    }

    /// Precomputed target amplitude `T` (adjusted by `N_leg / N_std` in legacy mode).
    #[inline]
    pub const fn target_amplitude(&self) -> f64 {
        self.target_amplitude
    }

    /// Final normalization factor `N`.
    #[inline]
    pub const fn normalization_factor(&self) -> f64 {
        self.normalization_factor
    }

    /// Conservative parameter range `[-r, r]` where `r = f32((T * (1/3)) * 6.0)`.
    #[inline]
    pub const fn range(&self) -> Interval {
        self.range
    }

    /// Standard instantiation from `rng`: forks two positional random factories and builds an
    /// interleaved [`OctaveStack`] (`[P_{1,0}, P_{2,0}, P_{1,1}, P_{2,1}, ...]`) using
    /// `from_hash_of("octave_<k>")` for each active octave.
    pub fn instantiate<R: RandomSource>(&self, rng: &mut R) -> OctaveStack {
        let f1 = rng.fork_positional();
        let f2 = rng.fork_positional();

        let mut builder = OctaveStackBuilder::with_capacity(self.active_octaves.len() * 2);
        for oct in &self.active_octaves {
            let seed_name = format!("octave_{}", oct.octave);
            let mut r1 = f1.from_hash_of(&seed_name);
            let p1 = PerlinNoise::new(&mut r1);
            let mut r2 = f2.from_hash_of(&seed_name);
            let p2 = PerlinNoise::new(&mut r2);

            let weight = (self.normalization_factor * oct.amplitude) as f32;
            builder.add_layer(p1, oct.frequency, weight);
            builder.add_layer(p2, oct.frequency * NORMAL_INPUT_FACTOR, weight);
        }

        builder.build()
    }

    /// Legacy Nether biome instantiation from `rng` (sequentially builds two legacy FBM stacks
    /// `S_1` and `S_2`, then flattens `S_1` at `freq = 1.0` followed by `S_2` at
    /// `freq = 337/331`, both scaled by `v = f32(N * A_0)`).
    pub fn instantiate_legacy_nether<R: RandomSource>(
        &self,
        rng: &mut R,
    ) -> Result<OctaveStack, NoiseError> {
        let owned_ones;
        let amplitudes: &[f64] = if self.amplitude_modifiers.is_empty() {
            owned_ones = vec![1.0; self.octave_count];
            &owned_ones
        } else {
            &self.amplitude_modifiers
        };

        let s1 = build_legacy_fbm(rng, self.base_octave, amplitudes)?;
        let s2 = build_legacy_fbm(rng, self.base_octave, amplitudes)?;

        let outer_amp = (self.normalization_factor * self.base_amplitude) as f32;
        let mut builder = OctaveStackBuilder::with_capacity(s1.layers().len() + s2.layers().len());
        builder.add_stack(&s1, 1.0, outer_amp);
        builder.add_stack(&s2, NORMAL_INPUT_FACTOR, outer_amp);
        Ok(builder.build())
    }
}

fn compute_standard_normalization(
    base_amplitude: f64,
    base_octave: i32,
    octave_count: usize,
    normalize: NormalizationMode,
    amplitude_modifiers: &[f64],
) -> (Vec<ActiveOctave>, f64, f64) {
    let mut freq = 2.0_f64.powi(base_octave);
    let mut amp = base_amplitude;
    if normalize != NormalizationMode::Disabled {
        let n_i32 = octave_count as i32;
        amp *= 0.5_f64.powi(-(n_i32 - 1)) / (0.5_f64.powi(-n_i32) - 1.0);
    }

    let mut active_octaves = Vec::with_capacity(octave_count);
    for i in 0..octave_count {
        let modifier = if amplitude_modifiers.is_empty() {
            1.0
        } else {
            amplitude_modifiers[i]
        };
        if modifier != 0.0 {
            active_octaves.push(ActiveOctave {
                octave: base_octave + i as i32,
                frequency: freq,
                amplitude: amp * modifier,
            });
        }
        freq *= 2.0;
        amp *= 0.5;
    }

    let mut target_amplitude = 0.0_f64;
    for o in &active_octaves {
        target_amplitude += o.amplitude.abs();
    }

    let mut variance = 0.0_f64;
    for o in &active_octaves {
        let term = PERLIN_STD_DEV * o.amplitude.abs();
        variance += term * term;
    }

    let d_in = variance.sqrt();
    let n_std = if d_in == 0.0 {
        0.0
    } else {
        (target_amplitude * NORMAL_TARGET_DEVIATION) / (d_in * SQRT_2)
    };

    (active_octaves, target_amplitude, n_std)
}

fn active_span(octave_count: usize, amplitude_modifiers: &[f64]) -> Option<usize> {
    if amplitude_modifiers.is_empty() {
        return (octave_count > 0).then_some(octave_count - 1);
    }
    let mut min_idx: Option<usize> = None;
    let mut max_idx = 0;
    for (i, &m) in amplitude_modifiers.iter().enumerate() {
        if m != 0.0 {
            if min_idx.is_none() {
                min_idx = Some(i);
            }
            max_idx = i;
        }
    }
    min_idx.map(|min_i| max_idx - min_i)
}

#[inline]
fn legacy_expected_deviation(span: usize) -> f64 {
    0.1 * (1.0 + 1.0 / ((span + 1) as f64))
}

#[inline]
fn legacy_normalization_factor(base_amplitude: f64, span: usize) -> f64 {
    ((base_amplitude * 0.5) * NORMAL_TARGET_DEVIATION) / legacy_expected_deviation(span)
}

/// Instantiated double-Perlin ("normal") noise backed by an [`OctaveStack`].
///
/// Keeps its [`NormalNoiseParameters`] alongside the compiled [`OctaveStack`] so callers can
/// inspect both the precomputed parameter range and the stack's layer-summed range.
#[derive(Debug, Clone, PartialEq)]
pub struct NormalNoise {
    parameters: NormalNoiseParameters,
    stack: OctaveStack,
}

impl NormalNoise {
    /// Instantiates a normal noise from `rng` and `parameters` using the standard interleaved path.
    pub fn new<R: RandomSource>(rng: &mut R, parameters: NormalNoiseParameters) -> Self {
        let stack = parameters.instantiate(rng);
        Self { parameters, stack }
    }

    /// Instantiates a normal noise directly from `(rng, first_octave, amplitudes, normalize)`
    /// with `base_amplitude = 1.0`.
    pub fn from_octaves<R: RandomSource>(
        rng: &mut R,
        first_octave: i32,
        amplitudes: Vec<f64>,
        normalize: NormalizationMode,
    ) -> Result<Self, NoiseError> {
        let parameters = NormalNoiseParameters::from_octaves(first_octave, amplitudes, normalize)?;
        Ok(Self::new(rng, parameters))
    }

    /// Instantiates a built-in vanilla normal noise using [`NormalNoiseParameters::parity`].
    pub fn from_parity<R: RandomSource>(
        rng: &mut R,
        first_octave: i32,
        amplitudes: Vec<f64>,
    ) -> Result<Self, NoiseError> {
        let parameters = NormalNoiseParameters::parity(first_octave, amplitudes)?;
        Ok(Self::new(rng, parameters))
    }

    /// Instantiates a legacy Nether biome normal noise from `rng` and `parameters` using the
    /// non-interleaved legacy FBM path.
    pub fn new_legacy_nether<R: RandomSource>(
        rng: &mut R,
        parameters: NormalNoiseParameters,
    ) -> Result<Self, NoiseError> {
        let stack = parameters.instantiate_legacy_nether(rng)?;
        Ok(Self { parameters, stack })
    }

    /// Instantiates a legacy Nether biome normal noise seeded with a `LegacyRandom` at
    /// `world_seed.wrapping_add(seed_offset)` (`0` for `minecraft:nether/temperature`, `1` for
    /// `minecraft:nether/vegetation`).
    pub fn from_nether_world_seed(
        world_seed: i64,
        seed_offset: i64,
        parameters: NormalNoiseParameters,
    ) -> Result<Self, NoiseError> {
        let mut rng = LegacyRandom::new(world_seed.wrapping_add(seed_offset));
        Self::new_legacy_nether(&mut rng, parameters)
    }

    /// Returns the specification parameters.
    #[inline]
    pub const fn parameters(&self) -> &NormalNoiseParameters {
        &self.parameters
    }

    /// Returns the compiled octave stack.
    #[inline]
    pub const fn stack(&self) -> &OctaveStack {
        &self.stack
    }

    /// Returns the precomputed parameter range `[-f32((T * 1/3) * 6.0), +f32((T * 1/3) * 6.0)]`.
    #[inline]
    pub const fn parameter_range(&self) -> Interval {
        self.parameters.range()
    }

    /// Returns the compiled octave stack's layer-summed conservative range.
    #[inline]
    pub const fn range(&self) -> Interval {
        self.stack.range()
    }

    /// Samples the normal noise in 3D at `(x, y, z)`.
    #[inline]
    pub fn sample(&self, x: f64, y: f64, z: f64) -> f32 {
        self.stack.sample(x, y, z)
    }

    /// Samples the normal noise in 2D on the XZ plane at `(x, y)`.
    #[inline]
    pub fn sample_2d(&self, x: f64, y: f64) -> f32 {
        self.stack.sample_2d(x, y)
    }

    /// Accumulates this normal noise over `grid` in `(z, x, y)` order.
    ///
    /// # Panics
    ///
    /// Panics if `buffer.len() < grid.len()`.
    #[inline]
    pub fn accumulate_volume(
        &self,
        grid: &VolumeGrid,
        scale_xz: f64,
        scale_y: f64,
        amplitude: f32,
        buffer: &mut [f32],
    ) {
        self.stack
            .accumulate_volume(grid, scale_xz, scale_y, amplitude, buffer);
    }
}

impl Noise for NormalNoise {
    #[inline]
    fn sample(&self, x: f64, y: f64, z: f64) -> f32 {
        NormalNoise::sample(self, x, y, z)
    }

    #[inline]
    fn sample_2d(&self, x: f64, y: f64) -> f32 {
        NormalNoise::sample_2d(self, x, y)
    }

    #[inline]
    fn range(&self) -> Interval {
        NormalNoise::range(self)
    }

    #[inline]
    fn accumulate_volume(
        &self,
        grid: &VolumeGrid,
        scale_xz: f64,
        scale_y: f64,
        amplitude: f32,
        buffer: &mut [f32],
    ) {
        NormalNoise::accumulate_volume(self, grid, scale_xz, scale_y, amplitude, buffer);
    }
}
