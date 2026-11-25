//! Climate sampling traits, single-point adapters, and chunk volume buffers.
//!
//! Implements the point resolver vs chunk volume resolver distinction from Sections 2 and 4 of
//! `docs/specs/worldgen/biomes-and-climate.md` without depending on `zenith-worldgen`.

use std::fmt;

use zenith_math::helpers::packing::{quart_from_block, quart_to_block};

use crate::climate::{ClimateError, TargetPoint, unquantize_coord};

/// The six router climate channels in exact spec / `sampleVolume` evaluation order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum ClimateChannel {
    /// Router `temperature` channel (index 0).
    Temperature = 0,
    /// Router `vegetation` / `humidity` channel (index 1).
    Humidity = 1,
    /// Router `continents` / `continentalness` channel (index 2).
    Continentalness = 2,
    /// Router `erosion` channel (index 3).
    Erosion = 3,
    /// Router `depth` channel (index 4).
    Depth = 4,
    /// Router `ridges` / `weirdness` channel (index 5).
    Weirdness = 5,
}

impl ClimateChannel {
    /// All six channels in the exact order filled by `MultiNoiseBiomeSource::createResolverForChunk`.
    pub const ALL: [ClimateChannel; 6] = [
        Self::Temperature,
        Self::Humidity,
        Self::Continentalness,
        Self::Erosion,
        Self::Depth,
        Self::Weirdness,
    ];
}

/// Source of climate samples at quart coordinates (and raw erosion at block coordinates for The End).
pub trait ClimateSampler {
    /// Samples the 6 quantized climate values at `(quart_x, quart_y, quart_z)`.
    fn sample(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> TargetPoint;

    /// Evaluates the raw unquantized `f32` `erosion` channel at **block** coordinates
    /// `(block_x, block_y, block_z)`.
    ///
    /// Used by [`crate::BiomeSource::TheEnd`], which evaluates `router.erosion` at
    /// `((chunk_x * 2 + 1) * 8, quart_y << 2, (chunk_z * 2 + 1) * 8)` and compares the
    /// unquantized `f32` (widened to `f64`) against exact thresholds (`0.25`, `-0.0625`,
    /// `-0.21875`).
    ///
    /// The default implementation falls back to unquantizing
    /// `self.sample(block_x >> 2, block_y >> 2, block_z >> 2).erosion`, which loses sub-`1e-4`
    /// precision near thresholds (in particular `-0.21875`). Implementations used with The End
    /// (such as [`PointClimate`], [`EndErosionClimate`], or a worldgen router sampler) should
    /// override this to return the exact raw `f32`.
    fn sample_erosion_block(&self, block_x: i32, block_y: i32, block_z: i32) -> f32 {
        unquantize_coord(
            self.sample(
                quart_from_block(block_x),
                quart_from_block(block_y),
                quart_from_block(block_z),
            )
            .erosion,
        )
    }
}

/// Single-point [`ClimateSampler`] adapter backed by a closure returning raw router values at
/// **block** coordinates `(block_x, block_y, block_z)`.
///
/// Channel order in the returned array is:
/// `[temperature, humidity/vegetation, continentalness, erosion, depth, weirdness/ridges]`.
///
/// Matches the spec's single-point resolver (`createResolver`): maps `(quart_x, quart_y, quart_z)`
/// to `(quart_x << 2, quart_y << 2, quart_z << 2)` and quantizes each channel via
/// [`crate::quantize_coord`]. Also exposes the exact unquantized `f32` erosion channel via
/// [`ClimateSampler::sample_erosion_block`].
#[derive(Clone, Copy)]
pub struct PointClimate<F> {
    sampler: F,
}

impl<F> fmt::Debug for PointClimate<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PointClimate").finish_non_exhaustive()
    }
}

impl<F> PointClimate<F>
where
    F: Fn(i32, i32, i32) -> [f32; 6],
{
    /// Wraps a block-coordinate closure `Fn(block_x, block_y, block_z) -> [f32; 6]`.
    #[inline]
    pub const fn new(sampler: F) -> Self {
        Self { sampler }
    }

    /// Evaluates the six raw `f32` channels at block coordinates `(block_x, block_y, block_z)`.
    #[inline]
    pub fn sample_raw_block(&self, block_x: i32, block_y: i32, block_z: i32) -> [f32; 6] {
        (self.sampler)(block_x, block_y, block_z)
    }
}

impl<F> ClimateSampler for PointClimate<F>
where
    F: Fn(i32, i32, i32) -> [f32; 6],
{
    #[inline]
    fn sample(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> TargetPoint {
        let block_x = quart_to_block(quart_x);
        let block_y = quart_to_block(quart_y);
        let block_z = quart_to_block(quart_z);
        TargetPoint::from_array((self.sampler)(block_x, block_y, block_z))
    }

    #[inline]
    fn sample_erosion_block(&self, block_x: i32, block_y: i32, block_z: i32) -> f32 {
        (self.sampler)(block_x, block_y, block_z)[ClimateChannel::Erosion as usize]
    }
}

/// Lightweight [`ClimateSampler`] adapter for [`crate::BiomeSource::TheEnd`] that only evaluates
/// the `erosion` density function at block coordinates `(block_x, block_y, block_z)`.
#[derive(Clone, Copy)]
pub struct EndErosionClimate<F> {
    erosion: F,
}

impl<F> fmt::Debug for EndErosionClimate<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EndErosionClimate").finish_non_exhaustive()
    }
}

impl<F> EndErosionClimate<F>
where
    F: Fn(i32, i32, i32) -> f32,
{
    /// Wraps a block-coordinate erosion evaluator `Fn(block_x, block_y, block_z) -> f32`.
    #[inline]
    pub const fn new(erosion: F) -> Self {
        Self { erosion }
    }
}

impl<F> ClimateSampler for EndErosionClimate<F>
where
    F: Fn(i32, i32, i32) -> f32,
{
    #[inline]
    fn sample(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> TargetPoint {
        let e = (self.erosion)(
            quart_to_block(quart_x),
            quart_to_block(quart_y),
            quart_to_block(quart_z),
        );
        TargetPoint::from_floats(0.0, 0.0, 0.0, e, 0.0, 0.0)
    }

    #[inline]
    fn sample_erosion_block(&self, block_x: i32, block_y: i32, block_z: i32) -> f32 {
        (self.erosion)(block_x, block_y, block_z)
    }
}

/// Precomputed 6-channel volume of raw `f32` climate values for chunk-level `BIOMES` sampling
/// (`MultiNoiseBiomeSource::createResolverForChunk`, spec §2 & §4).
///
/// Each channel buffer stores `size_x * size_y * size_z` samples in **Z-major, X-middle,
/// Y-contiguous** order matching `DensityBuffer`:
/// `index(ix, iy, iz) = iy + (ix + iz * size_x) * size_y`.
///
/// The corresponding block-space `DensityVolume` has origin
/// `(min_quart_x << 2, min_quart_y << 2, min_quart_z << 2)` and stride `(4, 4, 4)`.
#[derive(Debug, Clone, PartialEq)]
pub struct ClimateVolume {
    min_quart_x: i32,
    min_quart_y: i32,
    min_quart_z: i32,
    size_x: usize,
    size_y: usize,
    size_z: usize,
    channels: [Vec<f32>; 6],
}

impl ClimateVolume {
    /// Block stride along each axis for quart-resolution volume sampling (`(4, 4, 4)`).
    pub const BLOCK_STRIDE: (i32, i32, i32) = (4, 4, 4);

    /// Wraps six precomputed raw channel buffers `[T, H, C, E, D, W]`.
    ///
    /// Returns [`ClimateError::InvalidVolumeLength`] if any buffer does not have exact length
    /// `size_x * size_y * size_z`.
    pub fn new(
        min_quart_x: i32,
        min_quart_y: i32,
        min_quart_z: i32,
        size_x: usize,
        size_y: usize,
        size_z: usize,
        channels: [Vec<f32>; 6],
    ) -> Result<Self, ClimateError> {
        let expected = size_x
            .checked_mul(size_y)
            .and_then(|xy| xy.checked_mul(size_z))
            .ok_or(ClimateError::VolumeSizeOverflow {
                size_x,
                size_y,
                size_z,
            })?;
        for (channel, buf) in channels.iter().enumerate() {
            if buf.len() != expected {
                return Err(ClimateError::InvalidVolumeLength {
                    size_x,
                    size_y,
                    size_z,
                    expected,
                    channel,
                    actual: buf.len(),
                });
            }
        }
        Ok(Self {
            min_quart_x,
            min_quart_y,
            min_quart_z,
            size_x,
            size_y,
            size_z,
            channels,
        })
    }

    /// Allocates 6 buffers of length `size_x * size_y * size_z` and invokes `fill_channel` in
    /// exact spec order (`Temperature`, `Humidity`, `Continentalness`, `Erosion`, `Depth`,
    /// `Weirdness`).
    pub fn from_channel_filler(
        min_quart_x: i32,
        min_quart_y: i32,
        min_quart_z: i32,
        size_x: usize,
        size_y: usize,
        size_z: usize,
        mut fill_channel: impl FnMut(ClimateChannel, &mut [f32]),
    ) -> Result<Self, ClimateError> {
        let expected = size_x
            .checked_mul(size_y)
            .and_then(|xy| xy.checked_mul(size_z))
            .ok_or(ClimateError::VolumeSizeOverflow {
                size_x,
                size_y,
                size_z,
            })?;
        let mut channels = std::array::from_fn(|_| vec![0.0_f32; expected]);
        for channel in ClimateChannel::ALL {
            fill_channel(channel, &mut channels[channel as usize]);
        }
        Ok(Self {
            min_quart_x,
            min_quart_y,
            min_quart_z,
            size_x,
            size_y,
            size_z,
            channels,
        })
    }

    /// Origin of this volume in quart coordinates `(min_quart_x, min_quart_y, min_quart_z)`.
    #[inline]
    pub const fn min_quart(&self) -> (i32, i32, i32) {
        (self.min_quart_x, self.min_quart_y, self.min_quart_z)
    }

    /// Origin of this volume in block coordinates `(min_quart_x << 2, min_quart_y << 2, min_quart_z << 2)`.
    #[inline]
    pub const fn origin_block(&self) -> (i32, i32, i32) {
        (
            quart_to_block(self.min_quart_x),
            quart_to_block(self.min_quart_y),
            quart_to_block(self.min_quart_z),
        )
    }

    /// Extents of this volume in quarts `(size_x, size_y, size_z)`.
    #[inline]
    pub const fn size(&self) -> (usize, usize, usize) {
        (self.size_x, self.size_y, self.size_z)
    }

    /// Flat buffer index for local quart offsets `(ix, iy, iz)`:
    /// `iy + (ix + iz * size_x) * size_y` (Z-major, X-middle, Y-contiguous).
    #[inline]
    pub const fn index(&self, ix: usize, iy: usize, iz: usize) -> usize {
        iy + (ix + iz * self.size_x) * self.size_y
    }

    /// Reads the 6 raw `f32` channel values at `(quart_x, quart_y, quart_z)`.
    ///
    /// # Panics
    ///
    /// Panics if `(quart_x, quart_y, quart_z)` lies outside the volume bounds.
    #[inline]
    pub fn sample_raw(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> [f32; 6] {
        let idx = self.checked_index(quart_x, quart_y, quart_z);
        [
            self.channels[0][idx],
            self.channels[1][idx],
            self.channels[2][idx],
            self.channels[3][idx],
            self.channels[4][idx],
            self.channels[5][idx],
        ]
    }

    /// Direct access to one channel's raw slice.
    #[inline]
    pub fn channel(&self, channel: ClimateChannel) -> &[f32] {
        &self.channels[channel as usize]
    }

    #[inline]
    fn checked_index(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> usize {
        let dx = quart_x.wrapping_sub(self.min_quart_x);
        let dy = quart_y.wrapping_sub(self.min_quart_y);
        let dz = quart_z.wrapping_sub(self.min_quart_z);
        assert!(
            dx >= 0
                && dy >= 0
                && dz >= 0
                && (dx as usize) < self.size_x
                && (dy as usize) < self.size_y
                && (dz as usize) < self.size_z,
            "quart ({quart_x}, {quart_y}, {quart_z}) out of ClimateVolume bounds origin=({}, {}, {}) size=({}, {}, {})",
            self.min_quart_x,
            self.min_quart_y,
            self.min_quart_z,
            self.size_x,
            self.size_y,
            self.size_z
        );
        self.index(dx as usize, dy as usize, dz as usize)
    }
}

impl ClimateSampler for ClimateVolume {
    #[inline]
    fn sample(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> TargetPoint {
        TargetPoint::from_array(self.sample_raw(quart_x, quart_y, quart_z))
    }

    #[inline]
    fn sample_erosion_block(&self, block_x: i32, block_y: i32, block_z: i32) -> f32 {
        let idx = self.checked_index(
            quart_from_block(block_x),
            quart_from_block(block_y),
            quart_from_block(block_z),
        );
        self.channels[ClimateChannel::Erosion as usize][idx]
    }
}

/// Flat index into a chunk section's 64-entry biome `PalettedContainer` for local quart
/// coordinates `(local_x, local_y, local_z)` in `0..4`:
/// `((local_y << 2) | local_z) << 2 | local_x = 16 * local_y + 4 * local_z + local_x`.
///
/// Note that section *storage* is Y-major, Z-middle, X-contiguous, whereas chunk *filling*
/// iterates X outer, Y middle, Z inner (spec §4).
#[inline]
pub const fn section_biome_index(local_x: usize, local_y: usize, local_z: usize) -> usize {
    ((local_y << 2) | local_z) << 2 | local_x
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::climate::quantize_coord;

    #[test]
    fn point_climate_maps_quart_to_block_and_quantizes() {
        let sampler = PointClimate::new(|bx, by, bz| {
            [
                bx as f32 * 0.01,
                by as f32 * 0.01,
                bz as f32 * 0.01,
                -0.21875_f32,
                0.5_f32,
                -0.78_f32,
            ]
        });
        let tp = sampler.sample(-3, 5, 2);
        // block coords: (-12, 20, 8)
        assert_eq!(tp.temperature, quantize_coord(-12.0_f32 * 0.01_f32));
        assert_eq!(tp.humidity, quantize_coord(20.0_f32 * 0.01_f32));
        assert_eq!(tp.continentalness, quantize_coord(8.0_f32 * 0.01_f32));
        assert_eq!(tp.erosion, quantize_coord(-0.21875_f32));
        assert_eq!(tp.depth, 5000);
        assert_eq!(tp.weirdness, -7799);
        assert_eq!(sampler.sample_erosion_block(8, 0, 8), -0.21875_f32);
    }

    #[test]
    fn climate_volume_indexing_and_channel_order() {
        let mut order = Vec::new();
        let vol = ClimateVolume::from_channel_filler(-4, -16, 8, 4, 8, 4, |ch, buf| {
            order.push(ch);
            for (idx, slot) in buf.iter_mut().enumerate() {
                *slot = (ch as usize as f32) * 0.1 + (idx as f32) * 0.0001;
            }
        })
        .unwrap();

        assert_eq!(order, ClimateChannel::ALL);
        assert_eq!(vol.origin_block(), (-16, -64, 32));
        // Local (ix=2, iy=3, iz=1) -> iy + (ix + iz * 4) * 8 = 3 + (2 + 4) * 8 = 51
        assert_eq!(vol.index(2, 3, 1), 51);
        let raw = vol.sample_raw(-2, -13, 9);
        assert_eq!(raw[0], 51.0 * 0.0001);
        assert_eq!(raw[3], 0.3_f32 + 51.0 * 0.0001);
        let tp = vol.sample(-2, -13, 9);
        assert_eq!(tp, TargetPoint::from_array(raw));
    }
}
