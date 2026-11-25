//! Octave stack composition, point/volume sampling, and the legacy FBM initializer.

use crate::noise::perlin::PerlinNoise;
use crate::noise::primitives::{GradientNoiseState, Interval, Noise, NoiseError, VolumeGrid};
use crate::noise::simplex::SimplexNoise;
use crate::noise::smeared::SmearedPerlinNoise;
use crate::random::RandomSource;

/// Number of RNG draws consumed when the legacy FBM initializer skips an octave.
pub const LEGACY_FBM_SKIP_COUNT: usize = 262;

/// Single-octave noise generator stored inside an [`OctaveLayer`].
#[derive(Debug, Clone, PartialEq)]
pub enum OctaveLayerNoise {
    /// Improved Perlin noise octave.
    Perlin(PerlinNoise),
    /// Smeared Perlin ("y-fuzz") noise octave.
    SmearedPerlin(SmearedPerlinNoise),
    /// Simplex noise octave.
    Simplex(SimplexNoise),
}

impl OctaveLayerNoise {
    /// Returns the underlying gradient-noise state of this layer.
    #[inline]
    pub const fn state(&self) -> &GradientNoiseState {
        match self {
            Self::Perlin(n) => n.state(),
            Self::SmearedPerlin(n) => n.state(),
            Self::Simplex(n) => n.state(),
        }
    }

    /// Samples this layer at 3D coordinates `(x, y, z)`.
    #[inline]
    pub fn sample(&self, x: f64, y: f64, z: f64) -> f32 {
        match self {
            Self::Perlin(n) => n.sample(x, y, z),
            Self::SmearedPerlin(n) => n.sample(x, y, z),
            Self::Simplex(n) => n.sample(x, y, z),
        }
    }

    /// Samples this layer at 2D coordinates `(x, y)`.
    #[inline]
    pub fn sample_2d(&self, x: f64, y: f64) -> f32 {
        match self {
            Self::Perlin(n) => n.sample_2d(x, y),
            Self::SmearedPerlin(n) => n.sample_2d(x, y),
            Self::Simplex(n) => n.sample_2d(x, y),
        }
    }

    /// Returns the conservative `f32` output interval for this layer.
    #[inline]
    pub fn range(&self) -> Interval {
        match self {
            Self::Perlin(n) => n.range(),
            Self::SmearedPerlin(n) => n.range(),
            Self::Simplex(n) => n.range(),
        }
    }

    /// Accumulates this layer into `buffer` over `grid`.
    #[inline]
    pub fn accumulate_volume(
        &self,
        grid: &VolumeGrid,
        scale_xz: f64,
        scale_y: f64,
        amplitude: f32,
        buffer: &mut [f32],
    ) {
        match self {
            Self::Perlin(n) => n.accumulate_volume(grid, scale_xz, scale_y, amplitude, buffer),
            Self::SmearedPerlin(n) => {
                n.accumulate_volume(grid, scale_xz, scale_y, amplitude, buffer);
            }
            Self::Simplex(n) => n.accumulate_volume(grid, scale_xz, scale_y, amplitude, buffer),
        }
    }
}

impl From<PerlinNoise> for OctaveLayerNoise {
    #[inline]
    fn from(noise: PerlinNoise) -> Self {
        Self::Perlin(noise)
    }
}

impl From<SmearedPerlinNoise> for OctaveLayerNoise {
    #[inline]
    fn from(noise: SmearedPerlinNoise) -> Self {
        Self::SmearedPerlin(noise)
    }
}

impl From<SimplexNoise> for OctaveLayerNoise {
    #[inline]
    fn from(noise: SimplexNoise) -> Self {
        Self::Simplex(noise)
    }
}

impl Noise for OctaveLayerNoise {
    #[inline]
    fn sample(&self, x: f64, y: f64, z: f64) -> f32 {
        OctaveLayerNoise::sample(self, x, y, z)
    }

    #[inline]
    fn sample_2d(&self, x: f64, y: f64) -> f32 {
        OctaveLayerNoise::sample_2d(self, x, y)
    }

    #[inline]
    fn range(&self) -> Interval {
        OctaveLayerNoise::range(self)
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
        OctaveLayerNoise::accumulate_volume(self, grid, scale_xz, scale_y, amplitude, buffer);
    }
}

/// One layer in an [`OctaveStack`]: a child generator, `f64` frequency, and `f32` amplitude.
#[derive(Debug, Clone, PartialEq)]
pub struct OctaveLayer {
    noise: OctaveLayerNoise,
    frequency: f64,
    amplitude: f32,
}

impl OctaveLayer {
    /// Creates a new layer.
    #[inline]
    pub fn new(noise: impl Into<OctaveLayerNoise>, frequency: f64, amplitude: f32) -> Self {
        Self {
            noise: noise.into(),
            frequency,
            amplitude,
        }
    }

    /// Reference to the layer's noise generator.
    #[inline]
    pub const fn noise(&self) -> &OctaveLayerNoise {
        &self.noise
    }

    /// Coordinate frequency multiplier (`f64`).
    #[inline]
    pub const fn frequency(&self) -> f64 {
        self.frequency
    }

    /// Output amplitude weight (`f32`).
    #[inline]
    pub const fn amplitude(&self) -> f32 {
        self.amplitude
    }
}

/// Builder for constructing a flattened [`OctaveStack`].
#[derive(Debug, Clone, Default)]
pub struct OctaveStackBuilder {
    layers: Vec<OctaveLayer>,
}

impl OctaveStackBuilder {
    /// Creates an empty builder.
    #[inline]
    pub fn new() -> Self {
        Self { layers: Vec::new() }
    }

    /// Creates an empty builder with preallocated capacity.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            layers: Vec::with_capacity(capacity),
        }
    }

    /// Appends a single layer `(noise, frequency, amplitude)`.
    #[inline]
    pub fn add_layer(
        &mut self,
        noise: impl Into<OctaveLayerNoise>,
        frequency: f64,
        amplitude: f32,
    ) -> &mut Self {
        self.layers
            .push(OctaveLayer::new(noise, frequency, amplitude));
        self
    }

    /// Flattens every layer of `stack` into this builder, scaling each layer's frequency by
    /// `outer_frequency` in `f64` and amplitude by `outer_amplitude` in `f32`.
    pub fn add_stack(
        &mut self,
        stack: &OctaveStack,
        outer_frequency: f64,
        outer_amplitude: f32,
    ) -> &mut Self {
        self.layers.reserve(stack.layers.len());
        for layer in &stack.layers {
            self.layers.push(OctaveLayer {
                noise: layer.noise.clone(),
                frequency: layer.frequency * outer_frequency,
                amplitude: layer.amplitude * outer_amplitude,
            });
        }
        self
    }

    /// Finalizes the stack and precomputes its conservative `f32` interval range.
    pub fn build(self) -> OctaveStack {
        let mut range = Interval::exact(0.0_f32);
        for layer in &self.layers {
            range = range + (layer.noise.range() * Interval::exact(layer.amplitude));
        }
        OctaveStack {
            layers: self.layers,
            range,
        }
    }
}

/// Flat ordered sequence of `(noise, frequency: f64, amplitude: f32)` layers accumulated in `f32`.
#[derive(Debug, Clone, PartialEq)]
pub struct OctaveStack {
    layers: Vec<OctaveLayer>,
    range: Interval,
}

impl OctaveStack {
    /// Creates an empty octave stack with range `[0.0, 0.0]`.
    #[inline]
    pub fn empty() -> Self {
        Self {
            layers: Vec::new(),
            range: Interval::exact(0.0_f32),
        }
    }

    /// Returns a slice of all layers in evaluation order.
    #[inline]
    pub fn layers(&self) -> &[OctaveLayer] {
        &self.layers
    }

    /// Conservative `f32` output interval computed by interval-summing all layers in order.
    #[inline]
    pub const fn range(&self) -> Interval {
        self.range
    }

    /// Samples all layers in order at `(x * phi_k, y * phi_k, z * phi_k)` and accumulates
    /// `a_k * sample` in `f32`.
    pub fn sample(&self, x: f64, y: f64, z: f64) -> f32 {
        let mut sum = 0.0_f32;
        for layer in &self.layers {
            let sample = layer.noise.sample(
                x * layer.frequency,
                y * layer.frequency,
                z * layer.frequency,
            );
            sum += layer.amplitude * sample;
        }
        sum
    }

    /// Samples all layers in order in 2D at `(x * phi_k, y * phi_k)` and accumulates
    /// `a_k * sample` in `f32`.
    pub fn sample_2d(&self, x: f64, y: f64) -> f32 {
        let mut sum = 0.0_f32;
        for layer in &self.layers {
            let sample = layer
                .noise
                .sample_2d(x * layer.frequency, y * layer.frequency);
            sum += layer.amplitude * sample;
        }
        sum
    }

    /// Accumulates every layer over `grid` in order with scales `(scale_xz * phi_k, scale_y * phi_k)`
    /// in `f64` and amplitude `amplitude * a_k` in `f32`.
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
        assert!(
            buffer.len() >= grid.len(),
            "volume buffer length {} is smaller than grid size {}",
            buffer.len(),
            grid.len()
        );
        for layer in &self.layers {
            layer.noise.accumulate_volume(
                grid,
                scale_xz * layer.frequency,
                scale_y * layer.frequency,
                amplitude * layer.amplitude,
                buffer,
            );
        }
    }
}

impl Noise for OctaveStack {
    #[inline]
    fn sample(&self, x: f64, y: f64, z: f64) -> f32 {
        OctaveStack::sample(self, x, y, z)
    }

    #[inline]
    fn sample_2d(&self, x: f64, y: f64) -> f32 {
        OctaveStack::sample_2d(self, x, y)
    }

    #[inline]
    fn range(&self) -> Interval {
        OctaveStack::range(self)
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
        OctaveStack::accumulate_volume(self, grid, scale_xz, scale_y, amplitude, buffer);
    }
}

/// Legacy FBM initializer used by legacy Nether biome noises.
///
/// Always constructs octave `0` unconditionally (discarding it if out of bounds or zero
/// amplitude), walks downward from `z - 1` to `0` calling `rng.consume_count(262)` for every
/// skipped or zero-amplitude slot, rejects positive octaves (`first_octave + n - 1 > 0`), and
/// assembles the resulting [`OctaveStack`] in ascending frequency order.
pub fn build_legacy_fbm<R: RandomSource>(
    rng: &mut R,
    first_octave: i32,
    amplitudes: &[f64],
) -> Result<OctaveStack, NoiseError> {
    let n = amplitudes.len();
    let z: i64 = -i64::from(first_octave);
    let mut octaves: Vec<Option<PerlinNoise>> = vec![None; n];

    // Step 1-2: unconditionally construct octave 0; keep only if in bounds and non-zero.
    let p0 = PerlinNoise::new(rng);
    if z >= 0 && (z as usize) < n && amplitudes[z as usize] != 0.0 {
        octaves[z as usize] = Some(p0);
    }

    // Step 3: walk downward from z - 1 down to 0 inclusive.
    if z > 0 {
        for i in (0..z).rev() {
            let idx = i as usize;
            if idx < n && amplitudes[idx] != 0.0 {
                octaves[idx] = Some(PerlinNoise::new(rng));
            } else {
                rng.consume_count(LEGACY_FBM_SKIP_COUNT);
            }
        }
    }

    // Step 4: reject positive octaves first (the configuration error), then validate the populated count.
    if z < (n as i64) - 1 {
        return Err(NoiseError::PositiveLegacyOctave {
            first_octave,
            octave_count: n,
        });
    }
    let populated = octaves.iter().filter(|slot| slot.is_some()).count();
    let expected = amplitudes.iter().filter(|&&w| w != 0.0).count();
    if populated != expected {
        return Err(NoiseError::LegacyOctaveCountMismatch {
            populated,
            expected,
        });
    }

    // Assemble octave stack from lowest exponent (first_octave) upward.
    let mut freq = 2.0_f64.powi(first_octave);
    let mut amp_scale = if n == 0 {
        0.0
    } else {
        2.0_f64.powi((n - 1) as i32) / (2.0_f64.powi(n as i32) - 1.0)
    };

    let mut builder = OctaveStackBuilder::with_capacity(populated);
    for (slot, &w) in octaves.into_iter().zip(amplitudes.iter()) {
        if let Some(perlin) = slot {
            builder.add_layer(perlin, freq, (amp_scale * w) as f32);
        }
        freq *= 2.0;
        amp_scale /= 2.0;
    }

    Ok(builder.build())
}
