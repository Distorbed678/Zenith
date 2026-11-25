//! Sampling grids (`DensityVolume`), axis bitmasks, buffer pools, and runtime `SamplerContext`.
//!
//! Buffer layout is Z-outer, X-middle, Y-inner (contiguous):
//! `index(ix, iy, iz) = iy + (ix + iz * size_x) * size_y`.

use std::fmt;
use std::sync::Arc;

use zenith_math::VolumeGrid;
use zenith_math::helpers::rounding::{java_floor_div_i32, java_floor_mod_i32};

/// Axis bitmask: depends on no coordinates.
pub const NO_AXES: u8 = 0b000;
/// Axis bitmask: depends on block X.
pub const AXIS_X: u8 = 0b001;
/// Axis bitmask: depends on block Y.
pub const AXIS_Y: u8 = 0b010;
/// Axis bitmask: depends on block Z.
pub const AXIS_Z: u8 = 0b100;
/// Axis bitmask: depends on all three axes (X | Y | Z).
pub const ALL_AXES: u8 = 0b111;

/// Regular 3D strided sampling grid for density-function volume evaluation.
///
/// Flat buffer index layout is **Z outer, X middle, Y contiguous inner**:
/// `index(ix, iy, iz) = iy + (ix + iz * size_x) * size_y`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DensityVolume {
    size_x: usize,
    size_y: usize,
    size_z: usize,
    min_x: i32,
    min_y: i32,
    min_z: i32,
    step_x: i32,
    step_y: i32,
    step_z: i32,
}

impl DensityVolume {
    /// Creates a new strided sampling grid.
    ///
    /// Returns `None` if any size is `0`, any step is `<= 0`, or `size_x * size_y * size_z` overflows.
    pub fn new(size: [usize; 3], min_block: [i32; 3], step: [i32; 3]) -> Option<Self> {
        let [size_x, size_y, size_z] = size;
        let [min_x, min_y, min_z] = min_block;
        let [step_x, step_y, step_z] = step;
        if size_x == 0 || size_y == 0 || size_z == 0 || step_x <= 0 || step_y <= 0 || step_z <= 0 {
            return None;
        }
        size_x.checked_mul(size_y)?.checked_mul(size_z)?;
        Some(Self {
            size_x,
            size_y,
            size_z,
            min_x,
            min_y,
            min_z,
            step_x,
            step_y,
            step_z,
        })
    }

    /// Creates a unit-step (`step = [1, 1, 1]`) sampling grid.
    pub fn unit(
        size_x: usize,
        size_y: usize,
        size_z: usize,
        min_x: i32,
        min_y: i32,
        min_z: i32,
    ) -> Option<Self> {
        Self::new([size_x, size_y, size_z], [min_x, min_y, min_z], [1, 1, 1])
    }

    /// Number of samples along X.
    #[inline]
    pub const fn size_x(&self) -> usize {
        self.size_x
    }

    /// Number of samples along Y.
    #[inline]
    pub const fn size_y(&self) -> usize {
        self.size_y
    }

    /// Number of samples along Z.
    #[inline]
    pub const fn size_z(&self) -> usize {
        self.size_z
    }

    /// Minimum block X coordinate (`block_x(0)`).
    #[inline]
    pub const fn min_block_x(&self) -> i32 {
        self.min_x
    }

    /// Minimum block Y coordinate (`block_y(0)`).
    #[inline]
    pub const fn min_block_y(&self) -> i32 {
        self.min_y
    }

    /// Minimum block Z coordinate (`block_z(0)`).
    #[inline]
    pub const fn min_block_z(&self) -> i32 {
        self.min_z
    }

    /// Inclusive max block X (`min_x + size_x * step_x - 1`).
    #[inline]
    pub const fn max_block_x(&self) -> i32 {
        self.min_x
            .wrapping_add((self.size_x as i32).wrapping_mul(self.step_x))
            .wrapping_sub(1)
    }

    /// Inclusive max block Y (`min_y + size_y * step_y - 1`).
    #[inline]
    pub const fn max_block_y(&self) -> i32 {
        self.min_y
            .wrapping_add((self.size_y as i32).wrapping_mul(self.step_y))
            .wrapping_sub(1)
    }

    /// Inclusive max block Z (`min_z + size_z * step_z - 1`).
    #[inline]
    pub const fn max_block_z(&self) -> i32 {
        self.min_z
            .wrapping_add((self.size_z as i32).wrapping_mul(self.step_z))
            .wrapping_sub(1)
    }

    /// Block stride along X.
    #[inline]
    pub const fn step_x(&self) -> i32 {
        self.step_x
    }

    /// Block stride along Y.
    #[inline]
    pub const fn step_y(&self) -> i32 {
        self.step_y
    }

    /// Block stride along Z.
    #[inline]
    pub const fn step_z(&self) -> i32 {
        self.step_z
    }

    /// Total number of voxels `size_x * size_y * size_z`.
    #[inline]
    pub const fn len(&self) -> usize {
        self.size_x * self.size_y * self.size_z
    }

    /// Always `false` for a valid volume.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// Wrapping block X coordinate at grid index `ix`.
    #[inline]
    pub const fn block_x(&self, ix: usize) -> i32 {
        self.min_x
            .wrapping_add((ix as i32).wrapping_mul(self.step_x))
    }

    /// Wrapping block Y coordinate at grid index `iy`.
    #[inline]
    pub const fn block_y(&self, iy: usize) -> i32 {
        self.min_y
            .wrapping_add((iy as i32).wrapping_mul(self.step_y))
    }

    /// Wrapping block Z coordinate at grid index `iz`.
    #[inline]
    pub const fn block_z(&self, iz: usize) -> i32 {
        self.min_z
            .wrapping_add((iz as i32).wrapping_mul(self.step_z))
    }

    /// Flat Z-X-Y buffer index `iy + (ix + iz * size_x) * size_y`.
    #[inline]
    pub const fn index(&self, ix: usize, iy: usize, iz: usize) -> usize {
        iy + (ix + iz * self.size_x) * self.size_y
    }

    /// Reverse lookup from world block coordinates `(bx, by, bz)` to flat buffer index,
    /// returning `None` if out of bounds or unaligned with the grid steps.
    #[inline]
    pub fn index_of_block(&self, bx: i32, by: i32, bz: i32) -> Option<usize> {
        let rx = bx.wrapping_sub(self.min_x);
        let ry = by.wrapping_sub(self.min_y);
        let rz = bz.wrapping_sub(self.min_z);
        if self.step_x == 1 && self.step_y == 1 && self.step_z == 1 {
            if rx >= 0
                && (rx as usize) < self.size_x
                && ry >= 0
                && (ry as usize) < self.size_y
                && rz >= 0
                && (rz as usize) < self.size_z
            {
                Some(self.index(rx as usize, ry as usize, rz as usize))
            } else {
                None
            }
        } else {
            let span_x = (self.size_x as i64) * i64::from(self.step_x);
            let span_y = (self.size_y as i64) * i64::from(self.step_y);
            let span_z = (self.size_z as i64) * i64::from(self.step_z);
            if rx >= 0
                && i64::from(rx) < span_x
                && ry >= 0
                && i64::from(ry) < span_y
                && rz >= 0
                && i64::from(rz) < span_z
                && java_floor_mod_i32(rx, self.step_x) == 0
                && java_floor_mod_i32(ry, self.step_y) == 0
                && java_floor_mod_i32(rz, self.step_z) == 0
            {
                let ix = java_floor_div_i32(rx, self.step_x) as usize;
                let iy = java_floor_div_i32(ry, self.step_y) as usize;
                let iz = java_floor_div_i32(rz, self.step_z) as usize;
                Some(self.index(ix, iy, iz))
            } else {
                None
            }
        }
    }

    /// Converts to [`zenith_math::VolumeGrid`] for noise accumulation.
    #[inline]
    pub fn to_volume_grid(&self) -> VolumeGrid {
        VolumeGrid::new(
            [self.size_x, self.size_y, self.size_z],
            [self.min_x, self.min_y, self.min_z],
            [self.step_x, self.step_y, self.step_z],
        )
        .expect("DensityVolume invariants guarantee a valid VolumeGrid")
    }
}

/// Reusable `f32` scratch buffer pool for volume evaluation.
#[derive(Debug, Default)]
pub(crate) struct BufferPool {
    free: Vec<Vec<f32>>,
}

impl BufferPool {
    #[inline]
    pub(crate) fn acquire(&mut self, len: usize) -> Vec<f32> {
        // Reuse the smallest pooled buffer that already has sufficient capacity, if any.
        if let Some((best_idx, _)) = self
            .free
            .iter()
            .enumerate()
            .filter(|(_, b)| b.capacity() >= len)
            .min_by_key(|(_, b)| b.capacity())
        {
            let mut buf = self.free.swap_remove(best_idx);
            buf.resize(len, 0.0_f32);
            return buf;
        }
        // Round allocation size up to a multiple of 16 (matching spec §Inputs and state).
        let cap = (len + 15) & !15;
        let mut buf = Vec::with_capacity(cap);
        buf.resize(len, 0.0_f32);
        buf
    }

    #[inline]
    pub(crate) fn release(&mut self, buf: Vec<f32>) {
        if self.free.len() < 32 {
            self.free.push(buf);
        }
    }
}

/// Per-cache-id runtime cache cell (`density-functions.md` §1D).
#[derive(Debug)]
pub(crate) struct CacheCell {
    pub(crate) volume: Option<DensityVolume>,
    pub(crate) buffer: Option<Vec<f32>>,
    pub(crate) value_key: i64,
    pub(crate) value: f32,
}

impl Default for CacheCell {
    fn default() -> Self {
        Self {
            volume: None,
            buffer: None,
            value_key: 0,
            value: f32::NAN,
        }
    }
}

/// Context-bound density sampler hook (used by `beardifier`, `blend_alpha`, `blend_offset`).
pub trait ContextSampler: Send + Sync + fmt::Debug {
    /// Point evaluation at `(bx, by, bz)`.
    fn sample_value(&self, bx: i32, by: i32, bz: i32) -> f32;

    /// Volume evaluation over `volume` into `out[..volume.len()]`.
    fn sample_volume(&self, out: &mut [f32], volume: DensityVolume) {
        let mut idx = 0;
        for iz in 0..volume.size_z() {
            let bz = volume.block_z(iz);
            for ix in 0..volume.size_x() {
                let bx = volume.block_x(ix);
                for iy in 0..volume.size_y() {
                    let by = volume.block_y(iy);
                    out[idx] = self.sample_value(bx, by, bz);
                    idx += 1;
                }
            }
        }
    }
}

/// Hook for `minecraft:blend_density` when an active non-empty `Blender` is attached.
pub trait BlenderHook: Send + Sync + fmt::Debug {
    /// Blends `density` at `(bx, by, bz)`.
    fn blend_density(&self, bx: i32, by: i32, bz: i32, density: f32) -> f32;
}

/// Per-sampling-session runtime state: cache table, temporary buffer arena, and context hooks.
#[derive(Debug)]
pub struct SamplerContext {
    pub(crate) enable_caches: bool,
    pub(crate) caches: Vec<CacheCell>,
    pub(crate) pool: BufferPool,
    pub(crate) blend_alpha: Option<Arc<dyn ContextSampler>>,
    pub(crate) blend_offset: Option<Arc<dyn ContextSampler>>,
    pub(crate) beardifier: Option<Arc<dyn ContextSampler>>,
    pub(crate) blender: Option<Arc<dyn BlenderHook>>,
}

impl SamplerContext {
    /// Creates an uncached context with no context overrides (equivalent to `EMPTY_UNCACHED`).
    pub fn uncached() -> Self {
        Self {
            enable_caches: false,
            caches: Vec::new(),
            pool: BufferPool::default(),
            blend_alpha: None,
            blend_offset: None,
            beardifier: None,
            blender: None,
        }
    }

    /// Creates a caching context sized for `cache_count` compiler cache cells.
    pub fn caching(cache_count: usize) -> Self {
        let mut caches = Vec::with_capacity(cache_count);
        caches.resize_with(cache_count, CacheCell::default);
        Self {
            enable_caches: true,
            caches,
            pool: BufferPool::default(),
            blend_alpha: None,
            blend_offset: None,
            beardifier: None,
            blender: None,
        }
    }

    /// Whether per-context caching is enabled.
    #[inline]
    pub const fn caches_enabled(&self) -> bool {
        self.enable_caches
    }

    /// Attaches a `beardifier` context sampler.
    pub fn with_beardifier(mut self, sampler: Arc<dyn ContextSampler>) -> Self {
        self.beardifier = Some(sampler);
        self
    }

    /// Attaches `blend_alpha`, `blend_offset`, and `blend_density` hooks.
    pub fn with_blender(
        mut self,
        alpha: Arc<dyn ContextSampler>,
        offset: Arc<dyn ContextSampler>,
        blender: Arc<dyn BlenderHook>,
    ) -> Self {
        self.blend_alpha = Some(alpha);
        self.blend_offset = Some(offset);
        self.blender = Some(blender);
        self
    }

    #[inline]
    pub(crate) fn acquire_buffer(&mut self, len: usize) -> Vec<f32> {
        self.pool.acquire(len)
    }

    #[inline]
    pub(crate) fn release_buffer(&mut self, buf: Vec<f32>) {
        self.pool.release(buf);
    }

    #[inline]
    pub(crate) fn ensure_cache_cell(&mut self, cache_id: usize) {
        if cache_id >= self.caches.len() {
            self.caches.resize_with(cache_id + 1, CacheCell::default);
        }
    }
}
