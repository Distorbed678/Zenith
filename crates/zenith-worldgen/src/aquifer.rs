//! Global fluid picker, disabled aquifer, and Voronoi noise-based aquifer
//! (`docs/specs/worldgen/aquifers-and-ore-veins.md` §1–§2).

use std::collections::HashMap;

use zenith_math::helpers::interp::{clamped_map_f64, map_f64};
use zenith_math::helpers::packing::{
    block_as_long, block_get_x, block_get_y, block_get_z, chunk_pack,
};
use zenith_math::helpers::rounding::{clamp_f64, floor_f32, java_floor_div_i32, quantize};

use crate::compiler::CompiledSampler;
use crate::heightmap::BlockPredicates;
use crate::random_state::PositionalFactory;
use crate::settings::{AquiferConfig, DIMENSION_MAX_Y, DIMENSION_MIN_Y};
use crate::volume::{DensityVolume, SamplerContext};

/// Dry aquifer sentinel (`DimensionType.WAY_BELOW_MIN_Y = MIN_Y << 4 = -32512`).
pub const WAY_BELOW_MIN_Y: i32 = DIMENSION_MIN_Y << 4;

/// Upper vertical sentinel (`DimensionType.WAY_ABOVE_MAX_Y = MAX_Y << 4 = 32496`).
pub const WAY_ABOVE_MAX_Y: i32 = DIMENSION_MAX_Y << 4;

/// Global deep lava Y threshold (`-54`).
pub const GLOBAL_LAVA_LEVEL: i32 = -54;

/// Flowing-update similarity threshold: `similarity(100, 144) = 1.0 - 44.0 / 25.0 = -0.76`.
pub const FLOWING_UPDATE_SIMILARITY: f64 = 1.0_f64 - 44.0_f64 / 25.0_f64;

/// Exact 13 chunk-offset pairs sampled around an aquifer cell center (`aquifers-and-ore-veins.md` §2.4).
pub const SURFACE_SAMPLING_OFFSETS_IN_CHUNKS: [(i32, i32); 13] = [
    (0, 0),
    (-2, -1),
    (-1, -1),
    (0, -1),
    (1, -1),
    (-3, 0),
    (-2, 0),
    (-1, 0),
    (1, 0),
    (-2, 1),
    (-1, 1),
    (0, 1),
    (1, 1),
];

/// Fluid level and block-state ID for a region or aquifer cell (`Aquifer.FluidStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FluidStatus {
    /// Fluid surface height; blocks strictly below `fluid_level` receive `fluid_type`.
    pub fluid_level: i32,
    /// Global block-state ID of the fluid block.
    pub fluid_type: u32,
}

impl FluidStatus {
    /// Returns `self.fluid_type` if `block_y < self.fluid_level`, else `air_id`.
    #[inline]
    pub const fn at(self, block_y: i32, air_id: u32) -> u32 {
        if block_y < self.fluid_level {
            self.fluid_type
        } else {
            air_id
        }
    }
}

/// Global fluid picker (`NoiseBasedChunkGenerator.createFluidPicker`, spec §1.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobalFluidPicker {
    lava_status: FluidStatus,
    sea_status: FluidStatus,
    lava_cutoff: i32,
}

impl GlobalFluidPicker {
    /// Constructs the global fluid picker for `sea_level`, `default_fluid_id`, and `lava_id`.
    #[inline]
    pub const fn new(sea_level: i32, default_fluid_id: u32, lava_id: u32) -> Self {
        let lava_cutoff = if GLOBAL_LAVA_LEVEL < sea_level {
            GLOBAL_LAVA_LEVEL
        } else {
            sea_level
        };
        Self {
            lava_status: FluidStatus {
                fluid_level: GLOBAL_LAVA_LEVEL,
                fluid_type: lava_id,
            },
            sea_status: FluidStatus {
                fluid_level: sea_level,
                fluid_type: default_fluid_id,
            },
            lava_cutoff,
        }
    }

    /// Evaluates `globalFluidPicker.computeFluid(block_x, block_y, block_z)` (`x`/`z` unused).
    #[inline]
    pub const fn compute_fluid(&self, _block_x: i32, block_y: i32, _block_z: i32) -> FluidStatus {
        if block_y < self.lava_cutoff {
            self.lava_status
        } else {
            self.sea_status
        }
    }
}

/// Aquifer implementation bound to one chunk or column `NoiseChunk`.
#[derive(Debug)]
pub enum Aquifer<'a> {
    /// Disabled aquifer (`NoiseGeneratorSettings.aquifers` absent, spec §1.2).
    Disabled(DisabledAquifer),
    /// Voronoi noise-based aquifer (`NoiseGeneratorSettings.aquifers` present, spec §2).
    NoiseBased(Box<NoiseBasedAquifer<'a>>),
}

impl<'a> Aquifer<'a> {
    /// Creates a disabled aquifer falling back directly to `global_fluid_picker`.
    #[inline]
    pub const fn disabled(global_fluid_picker: GlobalFluidPicker, air_id: u32) -> Self {
        Self::Disabled(DisabledAquifer {
            global_fluid_picker,
            air_id,
        })
    }

    /// Creates a noise-based aquifer, pre-populating its `surface_level` cache over `volume`.
    #[allow(clippy::too_many_arguments)]
    pub fn noise_based(
        ctx: &mut SamplerContext,
        samplers: &'a [CompiledSampler; AquiferConfig::COUNT],
        positional_random: &'a PositionalFactory,
        volume: DensityVolume,
        global_fluid_picker: GlobalFluidPicker,
        predicates: &'a BlockPredicates,
    ) -> Self {
        Self::NoiseBased(Box::new(NoiseBasedAquifer::new(
            ctx,
            samplers,
            positional_random,
            volume,
            global_fluid_picker,
            predicates,
        )))
    }

    /// Evaluates the block substance at `(block_x, block_y, block_z)` for `density` (`f64`).
    ///
    /// Returns `None` for solid terrain (caller substitutes `default_block`), or `Some(state_id)`
    /// for air or fluid.
    #[inline]
    pub fn compute_substance(
        &mut self,
        ctx: &mut SamplerContext,
        block_x: i32,
        block_y: i32,
        block_z: i32,
        density: f64,
    ) -> Option<u32> {
        match self {
            Self::Disabled(a) => a.compute_substance(block_x, block_y, block_z, density),
            Self::NoiseBased(a) => a.compute_substance(ctx, block_x, block_y, block_z, density),
        }
    }

    /// Whether the most recent `compute_substance` call requests a fluid post-processing mark.
    #[inline]
    pub const fn should_schedule_fluid_update(&self) -> bool {
        match self {
            Self::Disabled(_) => false,
            Self::NoiseBased(a) => a.should_schedule_fluid_update,
        }
    }
}

/// Disabled aquifer (`Aquifer.createDisabled`, spec §1.2).
#[derive(Debug, Clone, Copy)]
pub struct DisabledAquifer {
    global_fluid_picker: GlobalFluidPicker,
    air_id: u32,
}

impl DisabledAquifer {
    /// Returns `None` if `density > 0.0`, else `Some(globalFluidPicker.computeFluid(...).at(y))`.
    #[inline]
    pub const fn compute_substance(
        &self,
        block_x: i32,
        block_y: i32,
        block_z: i32,
        density: f64,
    ) -> Option<u32> {
        if density > 0.0_f64 {
            None
        } else {
            Some(
                self.global_fluid_picker
                    .compute_fluid(block_x, block_y, block_z)
                    .at(block_y, self.air_id),
            )
        }
    }
}

#[inline]
const fn grid_x(bx: i32) -> i32 {
    bx >> 4
}

#[inline]
const fn from_grid_x(gx: i32, ox: i32) -> i32 {
    gx.wrapping_shl(4).wrapping_add(ox)
}

#[inline]
fn grid_y(by: i32) -> i32 {
    java_floor_div_i32(by, 12)
}

#[inline]
const fn from_grid_y(gy: i32, oy: i32) -> i32 {
    gy.wrapping_mul(12).wrapping_add(oy)
}

#[inline]
const fn grid_z(bz: i32) -> i32 {
    bz >> 4
}

#[inline]
const fn from_grid_z(gz: i32, oz: i32) -> i32 {
    gz.wrapping_shl(4).wrapping_add(oz)
}

#[inline]
fn similarity(d1: i32, d2: i32) -> f64 {
    1.0_f64 - f64::from(d2.wrapping_sub(d1)) / 25.0_f64
}

/// Voronoi-cell noise-based aquifer (`Aquifer.NoiseBasedAquifer`, spec §2).
#[derive(Debug)]
pub struct NoiseBasedAquifer<'a> {
    // 0: barrier, 1: floodedness, 2: spread, 3: lava, 4: exclusion, 5: surface_level
    samplers: &'a [CompiledSampler; AquiferConfig::COUNT],
    positional_random: &'a PositionalFactory,
    global_fluid_picker: GlobalFluidPicker,
    predicates: &'a BlockPredicates,
    air_id: u32,
    lava_id: u32,

    min_grid_x: i32,
    min_grid_y: i32,
    min_grid_z: i32,
    grid_size_x: usize,
    grid_size_z: usize,

    aquifer_cache: Vec<Option<FluidStatus>>,
    aquifer_location_cache: Vec<i64>,

    // Dense 2D table for the pre-populated surface_level bounding box + overflow map for out-of-box misses.
    surf_min_qx: i32,
    surf_max_qx: i32,
    surf_min_qz: i32,
    surf_max_qz: i32,
    surf_size_x: usize,
    surf_dense: Vec<i32>,
    surf_overflow: HashMap<i64, i32>,

    skip_sampling_above_y: i32,
    should_schedule_fluid_update: bool,
}

impl<'a> NoiseBasedAquifer<'a> {
    /// Constructs a `NoiseBasedAquifer` for `volume`, pre-populating `surface_level` over the
    /// quart-strided bounding box of the aquifer grid (`aquifers-and-ore-veins.md` §2.1).
    pub fn new(
        ctx: &mut SamplerContext,
        samplers: &'a [CompiledSampler; AquiferConfig::COUNT],
        positional_random: &'a PositionalFactory,
        volume: DensityVolume,
        global_fluid_picker: GlobalFluidPicker,
        predicates: &'a BlockPredicates,
    ) -> Self {
        let min_grid_x = grid_x(volume.min_block_x().wrapping_sub(5));
        let max_grid_x = grid_x(volume.max_block_x().wrapping_sub(5)).wrapping_add(1);
        let grid_size_x = (max_grid_x - min_grid_x + 1) as usize;

        let min_grid_y = grid_y(volume.min_block_y().wrapping_add(1)).wrapping_sub(1);
        let max_grid_y = grid_y(volume.max_block_y().wrapping_add(1)).wrapping_add(1);
        let grid_size_y = (max_grid_y - min_grid_y + 1) as usize;

        let min_grid_z = grid_z(volume.min_block_z().wrapping_sub(5));
        let max_grid_z = grid_z(volume.max_block_z().wrapping_sub(5)).wrapping_add(1);
        let grid_size_z = (max_grid_z - min_grid_z + 1) as usize;

        let total_grid_size = grid_size_x * grid_size_y * grid_size_z;
        let aquifer_cache = vec![None; total_grid_size];
        let aquifer_location_cache = vec![i64::MAX; total_grid_size];

        // Step 4: Pre-populate surfaceLevelCache and compute skipSamplingAboveY.
        let min_bx = from_grid_x(min_grid_x, 0);
        let min_bz = from_grid_z(min_grid_z, 0);
        let max_bx = from_grid_x(max_grid_x, 9);
        let max_bz = from_grid_z(max_grid_z, 9);

        let surf_min_qx = min_bx >> 2;
        let surf_max_qx = max_bx >> 2;
        let surf_min_qz = min_bz >> 2;
        let surf_max_qz = max_bz >> 2;
        let surf_size_x = (surf_max_qx - surf_min_qx + 1) as usize;
        let surf_size_z = (surf_max_qz - surf_min_qz + 1) as usize;

        let surf_vol = DensityVolume::new(
            [surf_size_x, 1, surf_size_z],
            [surf_min_qx << 2, 0, surf_min_qz << 2],
            [4, 1, 4],
        )
        .expect("aquifer surface_level volume is valid");

        let surf_len = surf_vol.len();
        let mut buf = ctx.acquire_buffer(surf_len);
        samplers[5].sample_volume(ctx, &mut buf[..surf_len], surf_vol);

        let mut surf_dense = vec![0_i32; surf_size_x * surf_size_z];
        let mut max_y = i32::MIN;
        for iz in 0..surf_size_z {
            let row = iz * surf_size_x;
            for ix in 0..surf_size_x {
                let sl = floor_f32(buf[surf_vol.index(ix, 0, iz)]);
                surf_dense[row + ix] = sl;
                if sl > max_y {
                    max_y = sl;
                }
            }
        }
        ctx.release_buffer(buf);

        let max_adjusted_surface_level = max_y.wrapping_add(8);
        let skip_sampling_above_grid_y =
            grid_y(max_adjusted_surface_level.wrapping_add(12)).wrapping_add(1);
        let skip_sampling_above_y = from_grid_y(skip_sampling_above_grid_y, 11).wrapping_sub(1);

        Self {
            samplers,
            positional_random,
            global_fluid_picker,
            predicates,
            air_id: predicates.air_id(),
            lava_id: predicates.lava_id(),
            min_grid_x,
            min_grid_y,
            min_grid_z,
            grid_size_x,
            grid_size_z,
            aquifer_cache,
            aquifer_location_cache,
            surf_min_qx,
            surf_max_qx,
            surf_min_qz,
            surf_max_qz,
            surf_size_x,
            surf_dense,
            surf_overflow: HashMap::new(),
            skip_sampling_above_y,
            should_schedule_fluid_update: false,
        }
    }

    #[inline]
    fn get_index(&self, gx: i32, gy: i32, gz: i32) -> usize {
        let rx = (gx - self.min_grid_x) as usize;
        let ry = (gy - self.min_grid_y) as usize;
        let rz = (gz - self.min_grid_z) as usize;
        (ry * self.grid_size_z + rz) * self.grid_size_x + rx
    }

    /// Evaluates per-block aquifer substance (`aquifers-and-ore-veins.md` §2.2).
    pub fn compute_substance(
        &mut self,
        ctx: &mut SamplerContext,
        block_x: i32,
        block_y: i32,
        block_z: i32,
        density: f64,
    ) -> Option<u32> {
        // 1. Solid terrain fast path.
        if density > 0.0_f64 {
            self.should_schedule_fluid_update = false;
            return None;
        }

        // 2. Global fluid lookup and upper sky fast path.
        let global_fluid = self
            .global_fluid_picker
            .compute_fluid(block_x, block_y, block_z);
        if block_y > self.skip_sampling_above_y {
            self.should_schedule_fluid_update = false;
            return Some(global_fluid.at(block_y, self.air_id));
        }

        // 3. Deep global lava fast path.
        if self
            .predicates
            .is_lava(global_fluid.at(block_y, self.air_id))
        {
            self.should_schedule_fluid_update = false;
            return Some(self.lava_id);
        }

        // 4. Nearest-4 Voronoi cell search.
        let x_anchor = grid_x(block_x.wrapping_sub(5));
        let y_anchor = grid_y(block_y.wrapping_add(1));
        let z_anchor = grid_z(block_z.wrapping_sub(5));

        let mut distance_sqr1 = i32::MAX;
        let mut distance_sqr2 = i32::MAX;
        let mut distance_sqr3 = i32::MAX;
        let mut distance_sqr4 = i32::MAX;
        let mut closest_index1 = 0usize;
        let mut closest_index2 = 0usize;
        let mut closest_index3 = 0usize;
        let mut closest_index4 = 0usize;

        for x1 in 0..=1 {
            let gx = x_anchor.wrapping_add(x1);
            for y1 in -1..=1 {
                let gy = y_anchor.wrapping_add(y1);
                for z1 in 0..=1 {
                    let gz = z_anchor.wrapping_add(z1);
                    let index = self.get_index(gx, gy, gz);
                    let mut location = self.aquifer_location_cache[index];
                    if location == i64::MAX {
                        let mut rng = self.positional_random.at(gx, gy, gz);
                        let ox = rng.next_int_bounded(10);
                        let oy = rng.next_int_bounded(9);
                        let oz = rng.next_int_bounded(10);
                        location = block_as_long(
                            from_grid_x(gx, ox),
                            from_grid_y(gy, oy),
                            from_grid_z(gz, oz),
                        );
                        self.aquifer_location_cache[index] = location;
                    }
                    let dx = block_get_x(location).wrapping_sub(block_x);
                    let dy = block_get_y(location).wrapping_sub(block_y);
                    let dz = block_get_z(location).wrapping_sub(block_z);
                    let new_distance = dx
                        .wrapping_mul(dx)
                        .wrapping_add(dy.wrapping_mul(dy))
                        .wrapping_add(dz.wrapping_mul(dz));

                    if distance_sqr1 >= new_distance {
                        distance_sqr4 = distance_sqr3;
                        closest_index4 = closest_index3;
                        distance_sqr3 = distance_sqr2;
                        closest_index3 = closest_index2;
                        distance_sqr2 = distance_sqr1;
                        closest_index2 = closest_index1;
                        distance_sqr1 = new_distance;
                        closest_index1 = index;
                    } else if distance_sqr2 >= new_distance {
                        distance_sqr4 = distance_sqr3;
                        closest_index4 = closest_index3;
                        distance_sqr3 = distance_sqr2;
                        closest_index3 = closest_index2;
                        distance_sqr2 = new_distance;
                        closest_index2 = index;
                    } else if distance_sqr3 >= new_distance {
                        distance_sqr4 = distance_sqr3;
                        closest_index4 = closest_index3;
                        distance_sqr3 = new_distance;
                        closest_index3 = index;
                    } else if distance_sqr4 >= new_distance {
                        distance_sqr4 = new_distance;
                        closest_index4 = index;
                    }
                }
            }
        }

        // 5. Closest cell status and similarity.
        let closest_status1 = self.get_aquifer_status(ctx, closest_index1);
        let similarity12 = similarity(distance_sqr1, distance_sqr2);
        let fluid_state = closest_status1.at(block_y, self.air_id);

        // 6. Non-overlapping cell branch.
        if similarity12 <= 0.0_f64 {
            if similarity12 >= FLOWING_UPDATE_SIMILARITY {
                let closest_status2 = self.get_aquifer_status(ctx, closest_index2);
                self.should_schedule_fluid_update = closest_status1 != closest_status2;
            } else {
                self.should_schedule_fluid_update = false;
            }
            return Some(fluid_state);
        }

        // 7. Border / barrier branch.
        // 7.1 Water-above-global-lava override.
        if self.predicates.is_water(fluid_state)
            && self.predicates.is_lava(
                self.global_fluid_picker
                    .compute_fluid(block_x, block_y.wrapping_sub(1), block_z)
                    .at(block_y.wrapping_sub(1), self.air_id),
            )
        {
            self.should_schedule_fluid_update = true;
            return Some(fluid_state);
        }

        // 7.2 Shared lazy barrier noise cache.
        let mut barrier_noise_cache = f64::NAN;

        // 7.3 Pair (1, 2) barrier.
        let closest_status2 = self.get_aquifer_status(ctx, closest_index2);
        let barrier12 = similarity12
            * self.calculate_pressure(
                ctx,
                block_x,
                block_y,
                block_z,
                &mut barrier_noise_cache,
                closest_status1,
                closest_status2,
            );
        if density + barrier12 > 0.0_f64 {
            self.should_schedule_fluid_update = false;
            return None;
        }

        // 7.4 Pair (1, 3) barrier (closest_status3 fetched unconditionally per spec).
        let closest_status3 = self.get_aquifer_status(ctx, closest_index3);
        let similarity13 = similarity(distance_sqr1, distance_sqr3);
        if similarity13 > 0.0_f64 {
            let barrier13 = (similarity12 * similarity13)
                * self.calculate_pressure(
                    ctx,
                    block_x,
                    block_y,
                    block_z,
                    &mut barrier_noise_cache,
                    closest_status1,
                    closest_status3,
                );
            if density + barrier13 > 0.0_f64 {
                self.should_schedule_fluid_update = false;
                return None;
            }
        }

        // 7.5 Pair (2, 3) barrier.
        let similarity23 = similarity(distance_sqr2, distance_sqr3);
        if similarity23 > 0.0_f64 {
            let barrier23 = (similarity12 * similarity23)
                * self.calculate_pressure(
                    ctx,
                    block_x,
                    block_y,
                    block_z,
                    &mut barrier_noise_cache,
                    closest_status2,
                    closest_status3,
                );
            if density + barrier23 > 0.0_f64 {
                self.should_schedule_fluid_update = false;
                return None;
            }
        }

        // 7.6 Fluid-update flag decision.
        let may_flow12 = closest_status1 != closest_status2;
        let may_flow23 =
            similarity23 >= FLOWING_UPDATE_SIMILARITY && closest_status2 != closest_status3;
        let may_flow13 =
            similarity13 >= FLOWING_UPDATE_SIMILARITY && closest_status1 != closest_status3;
        if may_flow12 || may_flow23 || may_flow13 {
            self.should_schedule_fluid_update = true;
        } else {
            self.should_schedule_fluid_update = similarity13 >= FLOWING_UPDATE_SIMILARITY
                && similarity(distance_sqr1, distance_sqr4) >= FLOWING_UPDATE_SIMILARITY
                && closest_status1 != self.get_aquifer_status(ctx, closest_index4);
        }

        Some(fluid_state)
    }

    /// Barrier pressure between two neighboring aquifer cells (`aquifers-and-ore-veins.md` §2.3).
    #[allow(clippy::too_many_arguments)]
    fn calculate_pressure(
        &self,
        ctx: &mut SamplerContext,
        block_x: i32,
        block_y: i32,
        block_z: i32,
        barrier_noise_cache: &mut f64,
        status1: FluidStatus,
        status2: FluidStatus,
    ) -> f64 {
        let type1 = status1.at(block_y, self.air_id);
        let type2 = status2.at(block_y, self.air_id);
        if (self.predicates.is_lava(type1) && self.predicates.is_water(type2))
            || (self.predicates.is_water(type1) && self.predicates.is_lava(type2))
        {
            return 2.0_f64;
        }

        let fluid_y_diff = status1
            .fluid_level
            .wrapping_sub(status2.fluid_level)
            .wrapping_abs();
        if fluid_y_diff == 0 {
            return 0.0_f64;
        }

        let average_fluid_y =
            0.5_f64 * f64::from(status1.fluid_level.wrapping_add(status2.fluid_level));
        let how_far_above_average = (f64::from(block_y) + 0.5_f64) - average_fluid_y;
        let base_value = f64::from(fluid_y_diff) / 2.0_f64;
        let distance_from_edge = base_value - how_far_above_average.abs();

        let gradient = if how_far_above_average > 0.0_f64 {
            let center_point = 0.0_f64 + distance_from_edge;
            if center_point > 0.0_f64 {
                center_point / 1.5_f64
            } else {
                center_point / 2.5_f64
            }
        } else {
            let center_point = 3.0_f64 + distance_from_edge;
            if center_point > 0.0_f64 {
                center_point / 3.0_f64
            } else {
                center_point / 10.0_f64
            }
        };

        let noise_value = if (-2.0_f64..=2.0_f64).contains(&gradient) || gradient.is_nan() {
            if barrier_noise_cache.is_nan() {
                *barrier_noise_cache =
                    f64::from(self.samplers[0].sample_value(ctx, block_x, block_y, block_z));
            }
            *barrier_noise_cache
        } else {
            0.0_f64
        };

        2.0_f64 * (noise_value + gradient)
    }

    #[inline]
    fn get_aquifer_status(&mut self, ctx: &mut SamplerContext, index: usize) -> FluidStatus {
        if let Some(status) = self.aquifer_cache[index] {
            return status;
        }
        let loc = self.aquifer_location_cache[index];
        let status = self.compute_fluid(ctx, block_get_x(loc), block_get_y(loc), block_get_z(loc));
        self.aquifer_cache[index] = Some(status);
        status
    }

    #[inline]
    fn surface_level(&mut self, ctx: &mut SamplerContext, bx: i32, bz: i32) -> i32 {
        let qx = bx >> 2;
        let qz = bz >> 2;
        if qx >= self.surf_min_qx
            && qx <= self.surf_max_qx
            && qz >= self.surf_min_qz
            && qz <= self.surf_max_qz
        {
            let ix = (qx - self.surf_min_qx) as usize;
            let iz = (qz - self.surf_min_qz) as usize;
            return self.surf_dense[iz * self.surf_size_x + ix];
        }
        let qbx = qx << 2;
        let qbz = qz << 2;
        let key = chunk_pack(qbx, qbz);
        if let Some(&cached) = self.surf_overflow.get(&key) {
            return cached;
        }
        let sl = floor_f32(self.samplers[5].sample_value(ctx, qbx, 0, qbz));
        self.surf_overflow.insert(key, sl);
        sl
    }

    /// Computes fluid level and type for one aquifer Voronoi cell (`aquifers-and-ore-veins.md` §2.4).
    fn compute_fluid(&mut self, ctx: &mut SamplerContext, x: i32, y: i32, z: i32) -> FluidStatus {
        let global_fluid = self.global_fluid_picker.compute_fluid(x, y, z);
        let mut lowest_preliminary_surface = i32::MAX;
        let top_of_cell = y.wrapping_add(12);
        let bottom_of_cell = y.wrapping_sub(12);
        let mut surface_at_center_under_global_fluid = false;

        for (ox, oz) in SURFACE_SAMPLING_OFFSETS_IN_CHUNKS {
            let sample_x = x.wrapping_add(ox << 4);
            let sample_z = z.wrapping_add(oz << 4);
            let sl = self.surface_level(ctx, sample_x, sample_z);
            let adjusted_sl = sl.wrapping_add(8);
            let is_start = ox == 0 && oz == 0;
            if is_start && bottom_of_cell > adjusted_sl {
                return global_fluid;
            }
            let top_pokes_above = top_of_cell > adjusted_sl;
            if top_pokes_above || is_start {
                let global_fluid_at_surface =
                    self.global_fluid_picker
                        .compute_fluid(sample_x, adjusted_sl, sample_z);
                if !self
                    .predicates
                    .is_air(global_fluid_at_surface.at(adjusted_sl, self.air_id))
                {
                    if is_start {
                        surface_at_center_under_global_fluid = true;
                    }
                    if top_pokes_above {
                        return global_fluid_at_surface;
                    }
                }
            }
            lowest_preliminary_surface = lowest_preliminary_surface.min(sl);
        }

        // Step 4: computeSurfaceLevel.
        let (partially_floodedness, fully_floodedness) =
            if f64::from(self.samplers[4].sample_value(ctx, x, y, z)) > 0.0_f64 {
                (-1.0_f64, -1.0_f64)
            } else {
                let distance_below_surface =
                    lowest_preliminary_surface.wrapping_add(8).wrapping_sub(y);
                let floodedness_factor = if surface_at_center_under_global_fluid {
                    clamped_map_f64(
                        f64::from(distance_below_surface),
                        0.0_f64,
                        64.0_f64,
                        1.0_f64,
                        0.0_f64,
                    )
                } else {
                    0.0_f64
                };
                let floodedness_noise_val = clamp_f64(
                    f64::from(self.samplers[1].sample_value(ctx, x, y, z)),
                    -1.0_f64,
                    1.0_f64,
                );
                let fully_flooded_threshold =
                    map_f64(floodedness_factor, 1.0_f64, 0.0_f64, -0.3_f64, 0.8_f64);
                let partially_flooded_threshold =
                    map_f64(floodedness_factor, 1.0_f64, 0.0_f64, -0.8_f64, 0.4_f64);
                (
                    floodedness_noise_val - partially_flooded_threshold,
                    floodedness_noise_val - fully_flooded_threshold,
                )
            };

        let fluid_surface_level = if fully_floodedness > 0.0_f64 {
            global_fluid.fluid_level
        } else if partially_floodedness > 0.0_f64 {
            let cell_x = java_floor_div_i32(x, 16);
            let cell_y = java_floor_div_i32(y, 40);
            let cell_z = java_floor_div_i32(z, 16);
            let cell_middle_y = cell_y.wrapping_mul(40).wrapping_add(20);
            // Multiplied by 10.0f in f32 before widening to f64 (spec Edge Case #2).
            let spread =
                f64::from(self.samplers[2].sample_value(ctx, cell_x, cell_y, cell_z) * 10.0_f32);
            let spread_quantized = quantize(spread, 3);
            lowest_preliminary_surface.min(cell_middle_y.wrapping_add(spread_quantized))
        } else {
            WAY_BELOW_MIN_Y
        };

        // Step 5: computeFluidType.
        let mut fluid_type = global_fluid.fluid_type;
        if fluid_surface_level <= -10
            && fluid_surface_level != WAY_BELOW_MIN_Y
            && fluid_type != self.lava_id
        {
            let type_cell_x = java_floor_div_i32(x, 64);
            let type_cell_y = java_floor_div_i32(y, 40);
            let type_cell_z = java_floor_div_i32(z, 64);
            let lava_val = f64::from(self.samplers[3].sample_value(
                ctx,
                type_cell_x,
                type_cell_y,
                type_cell_z,
            ));
            if lava_val.abs() > 0.3_f64 {
                fluid_type = self.lava_id;
            }
        }

        FluidStatus {
            fluid_level: fluid_surface_level,
            fluid_type,
        }
    }
}
