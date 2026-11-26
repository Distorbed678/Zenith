//! Group D: Ice, snow, spikes, geodes, root systems, and cave speleothems
//! (`features.md` §4.25–§4.33).
// `!(a > b)` on floats is deliberate: it keeps NaN semantics identical to the game.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use zenith_blocks::Direction;
use zenith_math::RandomSource;
use zenith_math::helpers::interp::{clamped_map_f32, clamped_map_f64};
use zenith_math::helpers::intrinsics::{
    java_math_cos, java_math_log, java_math_pow, java_math_sin,
};
use zenith_math::helpers::roots::{inv_sqrt_f64, sqrt as sqrt_f32};
use zenith_math::helpers::rounding::{ceil_f32, clamp_f32, floor_f64};
use zenith_math::helpers::trig::{
    PI as MTH_PI, TWO_PI as MTH_TWO_PI, cos as mth_cos, sin as mth_sin,
};
use zenith_math::noise::NormalNoise;
use zenith_worldgen::HeightmapKind;

use crate::block_ops::{BlockBitSet, BlockOps};
use crate::features::{
    FeatureContext, GeodeConfig, LargeDripstoneConfig, RootSystemConfig, SpeleothemClusterConfig,
    SpeleothemConfig,
};
use crate::providers::{BlockPredicate, Column};
use crate::region::GenerationRegion;
use crate::rng::{
    BlockPos, WorldgenRandom, between_closed, direction_from_2d_data_value, direction_get_random,
    mth_normal, random_between_f32, random_between_inclusive,
};

// ---------------------------------------------------------------------------
// §4.25 `minecraft:iceberg`
// ---------------------------------------------------------------------------

#[inline]
fn is_iceberg_state(ops: &BlockOps, s: u32) -> bool {
    let bid = ops.block_id(s);
    bid == ops.bid_packed_ice || bid == ops.bid_snow_block || bid == ops.bid_blue_ice
}

fn signed_distance_circle(xo: i32, zo: i32, radius: i32, rng: &mut WorldgenRandom) -> f64 {
    let off = 10.0_f32 * clamp_f32(rng.next_float(), 0.2_f32, 0.8_f32) / (radius as f32);
    f64::from(off) + f64::from(xo) * f64::from(xo) + f64::from(zo) * f64::from(zo)
        - f64::from(radius) * f64::from(radius)
}

fn signed_distance_ellipse(xo: i32, zo: i32, center: BlockPos, a: i32, c: i32, angle: f64) -> f64 {
    let dx = f64::from(xo - center.x);
    let dz = f64::from(zo - center.z);
    let cos_a = java_math_cos(angle);
    let sin_a = java_math_sin(angle);
    let rx = (dx * cos_a - dz * sin_a) / f64::from(a);
    let rz = (dx * sin_a + dz * cos_a) / f64::from(c);
    rx * rx + rz * rz - 1.0
}

#[inline]
fn get_ellipse_c(y_off: i32, height: i32, shape_ellipse_c: i32) -> i32 {
    if y_off > 0 && height - y_off <= 3 {
        shape_ellipse_c - (4 - (height - y_off))
    } else {
        shape_ellipse_c
    }
}

fn height_dependent_radius_round(
    rng: &mut WorldgenRandom,
    y_off: i32,
    height: i32,
    width: i32,
) -> i32 {
    let k = 3.5_f32 - rng.next_float();
    let mut scale = (1.0_f32
        - ((f64::from(y_off) * f64::from(y_off)) as f32) / ((height as f32) * k))
        * (width as f32);
    if height > 15 + rng.next_int_bounded(5) {
        let temp_y_off = if y_off < 3 + rng.next_int_bounded(6) {
            y_off / 2
        } else {
            y_off
        };
        scale = (1.0_f32 - (temp_y_off as f32) / ((height as f32) * k * 0.4_f32)) * (width as f32);
    }
    ceil_f32(scale / 2.0_f32)
}

fn height_dependent_radius_ellipse(y_off: i32, height: i32, width: i32) -> i32 {
    ceil_f32(
        ((1.0_f32 - ((f64::from(y_off) * f64::from(y_off)) as f32) / (height as f32))
            * (width as f32))
            / 2.0_f32,
    )
}

fn height_dependent_radius_steep(
    rng: &mut WorldgenRandom,
    y_off: i32,
    height: i32,
    width: i32,
) -> i32 {
    let k = 1.0_f32 + rng.next_float() / 2.0_f32;
    ceil_f32(((1.0_f32 - (y_off as f32) / ((height as f32) * k)) * (width as f32)) / 2.0_f32)
}

#[allow(clippy::too_many_arguments)]
fn generate_iceberg_block(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    height: i32,
    xo: i32,
    y_off: i32,
    zo: i32,
    radius: i32,
    a: i32,
    is_ellipse: bool,
    shape_ellipse_c: i32,
    shape_angle: f64,
    snow_on_top: bool,
    main_state: u32,
) {
    let signed_dist = if is_ellipse {
        signed_distance_ellipse(
            xo,
            zo,
            BlockPos::ZERO,
            a,
            get_ellipse_c(y_off, height, shape_ellipse_c),
            shape_angle,
        )
    } else {
        signed_distance_circle(xo, zo, radius, rng)
    };
    if signed_dist < 0.0 {
        let pos = origin.offset(xo, y_off, zo);
        let compare_val = if is_ellipse {
            -0.5
        } else {
            f64::from(-6 - rng.next_int_bounded(3))
        };
        if signed_dist > compare_val && rng.next_double() > 0.9 {
            return;
        }
        let h_diff = height - y_off;
        let cur = region.get_block(pos);
        let ops = region.ops();
        let cur_bid = ops.block_id(cur);
        if ops.is_air(cur)
            || cur_bid == ops.bid_snow_block
            || cur_bid == ops.bid_ice
            || cur_bid == ops.bid_water
        {
            let randomness = !is_ellipse || rng.next_double() > 0.05;
            let divisor = if is_ellipse { 3 } else { 2 };
            let snow_block = ops.sid_snow_block;
            let is_water = cur_bid == ops.bid_water;
            if snow_on_top
                && !is_water
                && f64::from(h_diff)
                    <= f64::from(rng.next_int_bounded((height / divisor).max(1)))
                        + f64::from(height) * 0.6
                && randomness
            {
                region.set_block(pos, snow_block, 3);
            } else {
                region.set_block(pos, main_state, 3);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn iceberg_carve(
    r: i32,
    y_off: i32,
    origin: BlockPos,
    region: &mut GenerationRegion,
    under_water: bool,
    angle: f64,
    local_origin: BlockPos,
    shape_ellipse_a: i32,
    shape_ellipse_c: i32,
) {
    let ca = r + 1 + shape_ellipse_a / 3;
    let cc = (r - 3).min(3) + shape_ellipse_c / 2 - 1;
    for xo in -ca..ca {
        for zo in -ca..ca {
            if signed_distance_ellipse(xo, zo, local_origin, ca, cc, angle) < 0.0 {
                let pos = origin.offset(xo, y_off, zo);
                let s = region.get_block(pos);
                let ops = region.ops();
                if is_iceberg_state(ops, s) || ops.block_id(s) == ops.bid_snow_block {
                    if under_water {
                        let water = ops.sid_water;
                        region.set_block(pos, water, 3);
                    } else {
                        let air = ops.sid_air;
                        let bid_snow = ops.bid_snow;
                        region.set_block(pos, air, 3);
                        if region.ops().block_id(region.get_block(pos.above())) == bid_snow {
                            region.set_block(pos.above(), air, 3);
                        }
                    }
                }
            }
        }
    }
}

/// §4.25 `minecraft:iceberg`.
pub fn place_iceberg(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    main_state: u32,
) -> bool {
    let origin = origin.at_y(region.sea_level());
    let snow_on_top = rng.next_double() > 0.7;
    let shape_angle = rng.next_double() * 2.0 * std::f64::consts::PI;
    let shape_ellipse_a = 11 - rng.next_int_bounded(5);
    let shape_ellipse_c = 3 + rng.next_int_bounded(3);
    let is_ellipse = rng.next_double() > 0.7;
    let mut over_water_height = if is_ellipse {
        rng.next_int_bounded(6) + 6
    } else {
        rng.next_int_bounded(15) + 3
    };
    if !is_ellipse && rng.next_double() > 0.9 {
        over_water_height += rng.next_int_bounded(19) + 7;
    }
    let under_water_height = (over_water_height + rng.next_int_bounded(11)).min(18);
    let width = (over_water_height + rng.next_int_bounded(7) - rng.next_int_bounded(5)).min(11);
    let a = if is_ellipse { shape_ellipse_a } else { 11 };

    // Above-water body
    for xo in -a..a {
        for zo in -a..a {
            for y_off in 0..over_water_height {
                let radius = if is_ellipse {
                    height_dependent_radius_ellipse(y_off, over_water_height, width)
                } else {
                    height_dependent_radius_round(rng, y_off, over_water_height, width)
                };
                if is_ellipse || xo < radius {
                    generate_iceberg_block(
                        region,
                        rng,
                        origin,
                        over_water_height,
                        xo,
                        y_off,
                        zo,
                        radius,
                        a,
                        is_ellipse,
                        shape_ellipse_c,
                        shape_angle,
                        snow_on_top,
                        main_state,
                    );
                }
            }
        }
    }

    // Smooth pass
    let sa = if is_ellipse {
        shape_ellipse_a
    } else {
        width / 2
    };
    for x in -sa..=sa {
        for z in -sa..=sa {
            for y_off in 0..=over_water_height {
                let pos = origin.offset(x, y_off, z);
                let s = region.get_block(pos);
                let ops = region.ops();
                let is_ice = is_iceberg_state(ops, s);
                if is_ice || ops.block_id(s) == ops.bid_snow {
                    let air = ops.sid_air;
                    if ops.is_air(region.get_block(pos.below())) {
                        region.set_block(pos, air, 3);
                        region.set_block(pos.above(), air, 3);
                    } else if is_ice {
                        let mut non_ice = 0;
                        for d in [
                            Direction::West,
                            Direction::East,
                            Direction::North,
                            Direction::South,
                        ] {
                            if !is_iceberg_state(region.ops(), region.get_block(pos.relative(d))) {
                                non_ice += 1;
                            }
                        }
                        if non_ice >= 3 {
                            region.set_block(pos, air, 3);
                        }
                    }
                }
            }
        }
    }

    // Under-water body
    for xo in -a..a {
        for zo in -a..a {
            let mut y_off = -1;
            while y_off > -under_water_height {
                let new_a = if is_ellipse {
                    ceil_f32(
                        (a as f32)
                            * (1.0_f32
                                - ((f64::from(y_off) * f64::from(y_off)) as f32)
                                    / ((under_water_height as f32) * 8.0_f32)),
                    )
                } else {
                    a
                };
                let radius = height_dependent_radius_steep(rng, -y_off, under_water_height, width);
                if xo < radius {
                    generate_iceberg_block(
                        region,
                        rng,
                        origin,
                        under_water_height,
                        xo,
                        y_off,
                        zo,
                        radius,
                        new_a,
                        is_ellipse,
                        shape_ellipse_c,
                        shape_angle,
                        snow_on_top,
                        main_state,
                    );
                }
                y_off -= 1;
            }
        }
    }

    // Cut-out pass
    let do_cut_out = if is_ellipse {
        rng.next_double() > 0.1
    } else {
        rng.next_double() > 0.7
    };
    if do_cut_out {
        let sign_x = if rng.next_boolean() { -1 } else { 1 };
        let sign_z = if rng.next_boolean() { -1 } else { 1 };
        let mut x_off = rng.next_int_bounded((width / 2 - 2).max(1));
        if rng.next_boolean() {
            x_off = width / 2 + 1 - rng.next_int_bounded((width - width / 2 - 1).max(1));
        }
        let mut z_off = rng.next_int_bounded((width / 2 - 2).max(1));
        if rng.next_boolean() {
            z_off = width / 2 + 1 - rng.next_int_bounded((width - width / 2 - 1).max(1));
        }
        if is_ellipse {
            let v = rng.next_int_bounded((shape_ellipse_a - 5).max(1));
            x_off = v;
            z_off = v;
        }
        let local_origin = BlockPos::new(sign_x * x_off, 0, sign_z * z_off);
        let angle = if is_ellipse {
            shape_angle + std::f64::consts::PI / 2.0
        } else {
            rng.next_double() * 2.0 * std::f64::consts::PI
        };
        for y_off in 0..(over_water_height - 3) {
            let r = height_dependent_radius_round(rng, y_off, over_water_height, width);
            iceberg_carve(
                r,
                y_off,
                origin,
                region,
                false,
                angle,
                local_origin,
                shape_ellipse_a,
                shape_ellipse_c,
            );
        }
        let mut y_off = -1;
        while y_off > -over_water_height + rng.next_int_bounded(5) {
            let r = height_dependent_radius_steep(rng, -y_off, over_water_height, width);
            iceberg_carve(
                r,
                y_off,
                origin,
                region,
                true,
                angle,
                local_origin,
                shape_ellipse_a,
                shape_ellipse_c,
            );
            y_off -= 1;
        }
    }

    true
}

// ---------------------------------------------------------------------------
// §4.26 `minecraft:freeze_top_layer`
// ---------------------------------------------------------------------------

/// Shared `biome.shouldFreeze(level, pos, false)` helper (used by `freeze_top_layer` and `lake`).
pub fn biome_should_freeze(
    ctx: &FeatureContext<'_>,
    region: &GenerationRegion,
    pos: BlockPos,
) -> bool {
    let biome_id = region.get_biome(pos);
    let Some(biome) = ctx.biome(biome_id) else {
        return false;
    };
    if !biome.cold_enough_to_snow(pos.x, pos.y, pos.z, region.sea_level()) {
        return false;
    }
    if !region.is_inside_build_height(pos.y) || region.get_block_brightness(pos) >= 10 {
        return false;
    }
    let st = region.get_block(pos);
    let ops = region.ops();
    ops.fluid(st).kind == zenith_blocks::FluidKind::Water && ops.block_class(st) == "LiquidBlock"
}

/// §4.26 `minecraft:freeze_top_layer`.
pub fn place_freeze_top_layer(
    ctx: &FeatureContext<'_>,
    region: &mut GenerationRegion,
    origin: BlockPos,
) -> bool {
    let ops = region.ops_arc().clone();
    let sea_level = region.sea_level();
    for dx in 0..16 {
        for dz in 0..16 {
            let x = origin.x + dx;
            let z = origin.z + dz;
            let y = region.get_height(HeightmapKind::MotionBlocking, x, z);
            let top_pos = BlockPos::new(x, y, z);
            let below_pos = BlockPos::new(x, y - 1, z);
            let biome_id = region.get_biome(top_pos);
            let Some(biome) = ctx.biome(biome_id) else {
                continue;
            };

            // Freeze check at below_pos (uses biome sampled at top_pos, matching vanilla!)
            if biome.cold_enough_to_snow(below_pos.x, below_pos.y, below_pos.z, sea_level)
                && region.is_inside_build_height(below_pos.y)
                && region.get_block_brightness(below_pos) < 10
            {
                let below_st = region.get_block(below_pos);
                if ops.fluid(below_st).kind == zenith_blocks::FluidKind::Water
                    && ops.block_class(below_st) == "LiquidBlock"
                {
                    region.set_block(below_pos, ops.sid_ice, 2);
                }
            }

            // Snow check at top_pos
            if biome.has_precipitation
                && biome.cold_enough_to_snow(top_pos.x, top_pos.y, top_pos.z, sea_level)
                && region.is_inside_build_height(top_pos.y)
                && region.get_block_brightness(top_pos) < 10
            {
                let top_st = region.get_block(top_pos);
                if (ops.is_air(top_st) || ops.block_id(top_st) == ops.bid_snow)
                    && ops.can_survive(region, top_pos, ops.sid_snow_1)
                {
                    region.set_block(top_pos, ops.sid_snow_1, 2);
                    let below_st = region.get_block(below_pos);
                    if ops.has_property(below_st, "snowy") {
                        let snowy = ops.try_set_property(below_st, "snowy", "true");
                        region.set_block(below_pos, snowy, 2);
                    }
                }
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// §4.27 `minecraft:blue_ice`
// ---------------------------------------------------------------------------

/// §4.27 `minecraft:blue_ice`.
pub fn place_blue_ice(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
) -> bool {
    if origin.y > region.sea_level() - 1 {
        return false;
    }
    let ops = region.ops_arc().clone();
    if !ops.is_water(region.get_block(origin)) && !ops.is_water(region.get_block(origin.below())) {
        return false;
    }
    let mut found_packed = false;
    for dir in Direction::ALL {
        if dir == Direction::Down {
            continue;
        }
        if ops.block_id(region.get_block(origin.relative(dir))) == ops.bid_packed_ice {
            found_packed = true;
            break;
        }
    }
    if !found_packed {
        return false;
    }
    region.set_block(origin, ops.sid_blue_ice, 2);
    for _ in 0..200 {
        let y_off = rng.next_int_bounded(5) - rng.next_int_bounded(6);
        let xz_diff = 3 + if y_off < 2 { y_off / 2 } else { 0 };
        if xz_diff >= 1 {
            let dx = rng.next_int_bounded(xz_diff) - rng.next_int_bounded(xz_diff);
            let dz = rng.next_int_bounded(xz_diff) - rng.next_int_bounded(xz_diff);
            let place_pos = origin.offset(dx, y_off, dz);
            let s = region.get_block(place_pos);
            let bid = ops.block_id(s);
            if ops.is_air(s)
                || bid == ops.bid_water
                || bid == ops.bid_packed_ice
                || bid == ops.bid_ice
            {
                for dir in Direction::ALL {
                    if ops.block_id(region.get_block(place_pos.relative(dir))) == ops.bid_blue_ice {
                        region.set_block(place_pos, ops.sid_blue_ice, 2);
                        break;
                    }
                }
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// §4.28 `minecraft:spike`
// ---------------------------------------------------------------------------

/// §4.28 `minecraft:spike`.
pub fn place_spike(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    mut origin: BlockPos,
    state: u32,
    can_place_on: &BlockPredicate,
    can_replace: &BlockPredicate,
) -> bool {
    while region.is_empty_block(origin) && origin.y > region.min_y() + 2 {
        origin = origin.below();
    }
    if !can_place_on.test(region, origin) {
        return false;
    }
    origin = origin.above_n(rng.next_int_bounded(4));
    let height = rng.next_int_bounded(4) + 7;
    let width = height / 4 + rng.next_int_bounded(2);
    if width > 1 && rng.next_int_bounded(60) == 0 {
        origin = origin.above_n(10 + rng.next_int_bounded(30));
    }

    for y_off in 0..height {
        let scale = (1.0_f32 - (y_off as f32) / (height as f32)) * (width as f32);
        let new_width = ceil_f32(scale);
        for xo in -new_width..=new_width {
            let dx = (xo.abs() as f32) - 0.25_f32;
            for zo in -new_width..=new_width {
                let dz = (zo.abs() as f32) - 0.25_f32;
                if (xo == 0 && zo == 0) || !(dx * dx + dz * dz > scale * scale) {
                    let on_edge = xo.abs() == new_width || zo.abs() == new_width;
                    if !on_edge || !(rng.next_float() > 0.75_f32) {
                        let pos_up = origin.offset(xo, y_off, zo);
                        if region.ops().is_air(region.get_block(pos_up))
                            || can_replace.test(region, pos_up)
                        {
                            region.set_block(pos_up, state, 3);
                        }
                        if y_off != 0 && new_width > 1 {
                            let pos_down = origin.offset(xo, -y_off, zo);
                            if region.ops().is_air(region.get_block(pos_down))
                                || can_replace.test(region, pos_down)
                            {
                                region.set_block(pos_down, state, 3);
                            }
                        }
                    }
                }
            }
        }
    }

    let pillar_width = (width - 1).clamp(0, 1);
    for xo in -pillar_width..=pillar_width {
        for zo in -pillar_width..=pillar_width {
            let mut cursor = origin.offset(xo, -1, zo);
            let mut run_length = if xo.abs() == 1 && zo.abs() == 1 {
                rng.next_int_bounded(5)
            } else {
                50
            };
            while cursor.y > 50 {
                let cur = region.get_block(cursor);
                if !region.ops().is_air(cur) && !can_replace.test(region, cursor) && cur != state {
                    break;
                }
                region.set_block(cursor, state, 3);
                cursor = cursor.below();
                run_length -= 1;
                if run_length <= 0 {
                    cursor = cursor.below_n(rng.next_int_bounded(5) + 1);
                    run_length = rng.next_int_bounded(5);
                }
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// §4.29 `minecraft:geode`
// ---------------------------------------------------------------------------

/// §4.29 `minecraft:geode`.
pub fn place_geode(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    cfg: &GeodeConfig,
) -> bool {
    let num_points = cfg.distribution_points.sample(rng);
    let mut noise_rng = WorldgenRandom::new_legacy(region.world_seed());
    let noise = NormalNoise::from_parity(&mut noise_rng, -4, vec![1.0])
        .expect("geode parity normal noise parameters are valid");

    let adj = f64::from(num_points) / f64::from(cfg.outer_wall_distance.max_inclusive());
    let inner_air = 1.0 / cfg.filling.sqrt();
    let innermost_block_layer = 1.0 / (cfg.inner_layer + adj).sqrt();
    let inner_crust = 1.0 / (cfg.middle_layer + adj).sqrt();
    let outer_crust = 1.0 / (cfg.outer_layer + adj).sqrt();
    let crack_size = 1.0
        / (cfg.base_crack_size + rng.next_double() / 2.0 + if num_points > 3 { adj } else { 0.0 })
            .sqrt();
    let should_generate_crack = f64::from(rng.next_float()) < cfg.generate_crack_chance;

    let mut num_invalid = 0;
    let mut points = Vec::with_capacity(num_points.max(0) as usize);
    let ops = region.ops_arc().clone();
    for _ in 0..num_points {
        let x = cfg.outer_wall_distance.sample(rng);
        let y = cfg.outer_wall_distance.sample(rng);
        let z = cfg.outer_wall_distance.sample(rng);
        let pos = origin.offset(x, y, z);
        let s = region.get_block(pos);
        if ops.is_air(s) || cfg.invalid_blocks.contains(ops.block_id(s)) {
            num_invalid += 1;
            if num_invalid > cfg.invalid_blocks_threshold {
                return false;
            }
        }
        points.push((pos, cfg.point_offset.sample(rng)));
    }

    let mut crack_points = Vec::new();
    if should_generate_crack {
        let offset_index = rng.next_int_bounded(4);
        let co = num_points * 2 + 1;
        let (cx, cz) = match offset_index {
            0 => (co, 0),
            1 => (0, co),
            2 => (co, co),
            _ => (0, 0),
        };
        for cy in [7, 5, 1] {
            crack_points.push(origin.offset(cx, cy, cz));
        }
    }

    let can_replace = |s: u32| !cfg.cannot_replace.contains(ops.block_id(s));
    let mut potential_crystal_placements = Vec::new();

    for p in between_closed(
        origin.offset(cfg.min_gen_offset, cfg.min_gen_offset, cfg.min_gen_offset),
        origin.offset(cfg.max_gen_offset, cfg.max_gen_offset, cfg.max_gen_offset),
    ) {
        let noise_offset = f64::from(noise.sample(f64::from(p.x), f64::from(p.y), f64::from(p.z)))
            * cfg.noise_multiplier;
        let mut dist_sum_shell = 0.0_f64;
        for &(pt, off) in &points {
            dist_sum_shell += inv_sqrt_f64(p.dist_sqr(pt) + f64::from(off)) + noise_offset;
        }
        if dist_sum_shell < outer_crust {
            continue;
        }
        if dist_sum_shell >= inner_air {
            let st = cfg.filling_provider.get_state(region, rng, p);
            region.safe_set_block(p, st, can_replace);
        } else {
            let mut dist_sum_crack = 0.0_f64;
            for &cp in &crack_points {
                dist_sum_crack +=
                    inv_sqrt_f64(p.dist_sqr(cp) + f64::from(cfg.crack_point_offset)) + noise_offset;
            }
            if should_generate_crack && dist_sum_crack >= crack_size {
                region.safe_set_block(p, ops.sid_air, can_replace);
                for dir in Direction::ALL {
                    let adj_pos = p.relative(dir);
                    let adj_fl = ops.fluid(region.get_block(adj_pos));
                    if !adj_fl.is_empty() {
                        region.schedule_fluid_tick(adj_pos, adj_fl.kind, 0);
                    }
                }
            } else if dist_sum_shell >= innermost_block_layer {
                let use_alt = f64::from(rng.next_float()) < cfg.use_alternate_layer0_chance;
                let st = if use_alt {
                    cfg.alternate_inner_layer_provider.get_state(region, rng, p)
                } else {
                    cfg.inner_layer_provider.get_state(region, rng, p)
                };
                region.safe_set_block(p, st, can_replace);
                if (!cfg.placements_require_layer0_alternate || use_alt)
                    && f64::from(rng.next_float()) < cfg.use_potential_placements_chance
                {
                    potential_crystal_placements.push(p);
                }
            } else if dist_sum_shell >= inner_crust {
                let st = cfg.middle_layer_provider.get_state(region, rng, p);
                region.safe_set_block(p, st, can_replace);
            } else if dist_sum_shell >= outer_crust {
                let st = cfg.outer_layer_provider.get_state(region, rng, p);
                region.safe_set_block(p, st, can_replace);
            }
        }
    }

    for crystal_pos in potential_crystal_placements {
        let base_state =
            cfg.inner_placements[rng.next_int_bounded(cfg.inner_placements.len() as i32) as usize];
        for dir in Direction::ALL {
            let mut block_state = ops.try_set_property(base_state, "facing", dir.name());
            let place_pos = crystal_pos.relative(dir);
            let place_state = region.get_block(place_pos);
            let fl = ops.fluid(place_state);
            block_state = ops.copy_waterlogged(block_state, fl.is_source);
            let can_grow = ops.is_air(place_state)
                || (ops.is_water(place_state) && fl.amount == 8 && fl.is_source);
            if can_grow {
                region.safe_set_block(place_pos, block_state, can_replace);
                break;
            }
        }
    }

    true
}

// ---------------------------------------------------------------------------
// Shared SpeleothemUtils (§4.30, §4.32, §4.33)
// ---------------------------------------------------------------------------

#[inline]
fn is_empty_or_water(ops: &BlockOps, s: u32) -> bool {
    ops.is_air(s) || ops.is_water(s)
}

#[inline]
fn is_empty_or_water_or_lava(ops: &BlockOps, s: u32) -> bool {
    ops.is_air(s) || ops.is_water(s) || ops.is_lava(s)
}

#[inline]
fn is_speleothem_base(
    ops: &BlockOps,
    s: u32,
    base_block_id: u32,
    replaceable: &BlockBitSet,
) -> bool {
    let bid = ops.block_id(s);
    bid == base_block_id || replaceable.contains(bid)
}

fn get_speleothem_height(xz_dist: f64, radius: f64, scale: f64, bluntness: f64) -> f64 {
    let d = xz_dist.max(bluntness);
    let r = d / radius * 0.384;
    let part1 = 0.75 * java_math_pow(r, 1.333_333_333_333_333_3);
    let part2 = java_math_pow(r, 0.666_666_666_666_666_6);
    let part3 = 0.333_333_333_333_333_3 * java_math_log(r);
    let h_rel = (scale * (part1 - part2 - part3)).max(0.0);
    h_rel / 0.384 * radius
}

fn is_circle_mostly_embedded_in_stone(
    region: &GenerationRegion,
    center: BlockPos,
    xz_radius: i32,
) -> bool {
    let ops = region.ops();
    if is_empty_or_water_or_lava(ops, region.get_block(center)) {
        return false;
    }
    let angle_inc = 6.0_f32 / (xz_radius as f32);
    let mut angle = 0.0_f32;
    while angle < MTH_TWO_PI {
        let dx = (mth_cos(f64::from(angle)) * (xz_radius as f32)) as i32;
        let dz = (mth_sin(f64::from(angle)) * (xz_radius as f32)) as i32;
        if is_empty_or_water_or_lava(ops, region.get_block(center.offset(dx, 0, dz))) {
            return false;
        }
        angle += angle_inc;
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn grow_speleothem(
    region: &mut GenerationRegion,
    start_pos: BlockPos,
    tip_dir: Direction,
    height: i32,
    merged_tip: bool,
    base_block_id: u32,
    pointed_default_state: u32,
    replaceable: &BlockBitSet,
) {
    if height <= 0 {
        return;
    }
    let ops = region.ops_arc().clone();
    let behind = region.get_block(start_pos.relative(tip_dir.opposite()));
    if !is_speleothem_base(&ops, behind, base_block_id, replaceable) {
        return;
    }
    let mut thicknesses: Vec<&'static str> = Vec::with_capacity(height as usize);
    if height >= 3 {
        thicknesses.push("base");
        thicknesses.extend(std::iter::repeat_n("middle", (height - 3) as usize));
    }
    if height >= 2 {
        thicknesses.push("frustum");
    }
    if height >= 1 {
        thicknesses.push(if merged_tip { "tip_merge" } else { "tip" });
    }
    let mut pos = start_pos;
    for thickness in thicknesses {
        let mut st =
            ops.try_set_property(pointed_default_state, "vertical_direction", tip_dir.name());
        st = ops.try_set_property(st, "thickness", thickness);
        st = ops.copy_waterlogged(st, ops.is_water_fluid(region.get_block(pos)));
        region.set_block(pos, st, 2);
        pos = pos.relative(tip_dir);
    }
}

fn place_base_block_if_possible(
    region: &mut GenerationRegion,
    pos: BlockPos,
    base_default_state: u32,
    replaceable: &BlockBitSet,
) -> bool {
    let cur = region.get_block(pos);
    if replaceable.contains(region.ops().block_id(cur)) {
        region.set_block(pos, base_default_state, 2);
        true
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// §4.30 `minecraft:large_dripstone`
// ---------------------------------------------------------------------------

struct LargeDripstoneHalf {
    root: BlockPos,
    pointing_up: bool,
    radius: i32,
    bluntness: f64,
    scale: f64,
}

#[derive(Clone, Copy)]
struct WindOffsetter {
    origin_y: i32,
    wind: Option<(f64, f64, i32)>,
}

impl WindOffsetter {
    fn offset(self, pos: BlockPos) -> BlockPos {
        let Some((wx, wz, max_off)) = self.wind else {
            return pos;
        };
        let dy = f64::from(self.origin_y - pos.y);
        let dx = floor_f64(wx * dy).clamp(-max_off, max_off);
        let dz = floor_f64(wz * dy).clamp(-max_off, max_off);
        pos.offset(dx, 0, dz)
    }
}

impl LargeDripstoneHalf {
    fn embed_base(&mut self, region: &GenerationRegion, wind: WindOffsetter) -> bool {
        while self.radius > 1 {
            let mut new_root = self.root;
            let max_tries = 10.min(get_speleothem_height(
                0.0,
                f64::from(self.radius),
                self.scale,
                self.bluntness,
            ) as i32);
            for _ in 0..max_tries {
                if region.ops().is_lava(region.get_block(new_root)) {
                    return false;
                }
                if is_circle_mostly_embedded_in_stone(region, wind.offset(new_root), self.radius) {
                    self.root = new_root;
                    return true;
                }
                new_root = if self.pointing_up {
                    new_root.below()
                } else {
                    new_root.above()
                };
            }
            self.radius /= 2;
        }
        false
    }

    fn place_blocks(
        &self,
        region: &mut GenerationRegion,
        rng: &mut WorldgenRandom,
        wind: WindOffsetter,
    ) {
        let ops = region.ops_arc().clone();
        for dx in -self.radius..=self.radius {
            for dz in -self.radius..=self.radius {
                let cur_r = sqrt_f32((dx * dx + dz * dz) as f32);
                if cur_r > (self.radius as f32) {
                    continue;
                }
                let mut h = get_speleothem_height(
                    f64::from(cur_r),
                    f64::from(self.radius),
                    self.scale,
                    self.bluntness,
                ) as i32;
                if h > 0 {
                    if f64::from(rng.next_float()) < 0.2 {
                        h = ((h as f32) * (rng.next_float() * 0.2_f32 + 0.8_f32)) as i32;
                    }
                    let mut pos = self.root.offset(dx, 0, dz);
                    let mut has_been_out_of_stone = false;
                    let max_y = if self.pointing_up {
                        region.get_height(HeightmapKind::WorldSurfaceWg, pos.x, pos.z)
                    } else {
                        i32::MAX
                    };
                    for _ in 0..h {
                        if pos.y >= max_y {
                            break;
                        }
                        let w_pos = wind.offset(pos);
                        let cur = region.get_block(w_pos);
                        if is_empty_or_water_or_lava(&ops, cur) {
                            has_been_out_of_stone = true;
                            region.set_block(w_pos, ops.sid_dripstone_block, 2);
                        } else if has_been_out_of_stone
                            && ops.tag_base_stone_overworld.contains(ops.block_id(cur))
                        {
                            break;
                        }
                        pos = if self.pointing_up {
                            pos.above()
                        } else {
                            pos.below()
                        };
                    }
                }
            }
        }
    }
}

/// §4.30 `minecraft:large_dripstone`.
pub fn place_large_dripstone(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    cfg: &LargeDripstoneConfig,
) -> bool {
    let ops = region.ops_arc().clone();
    if !is_empty_or_water(&ops, region.get_block(origin)) {
        return false;
    }
    let Some(Column::Range { floor, ceiling }) = Column::scan(
        region,
        origin,
        cfg.floor_to_ceiling_search_range,
        |s| is_empty_or_water(&ops, s),
        |s| {
            is_speleothem_base(&ops, s, ops.bid_dripstone_block, &cfg.replaceable_blocks)
                || ops.is_lava(s)
        },
    ) else {
        return false;
    };
    let cave_height = ceiling - floor - 1;
    if cave_height < 4 {
        return false;
    }
    let max_by_height = ((cave_height as f32) * cfg.max_column_radius_to_cave_height_ratio) as i32;
    let min_r = cfg.column_radius.min_inclusive();
    let max_col_r = max_by_height.clamp(min_r, cfg.column_radius.max_inclusive());
    let radius = random_between_inclusive(rng, min_r, max_col_r);

    let mut stalactite = LargeDripstoneHalf {
        root: origin.at_y(ceiling - 1),
        pointing_up: false,
        radius,
        bluntness: f64::from(cfg.stalactite_bluntness.sample(rng)),
        scale: f64::from(cfg.height_scale.sample(rng)),
    };
    let mut stalagmite = LargeDripstoneHalf {
        root: origin.at_y(floor + 1),
        pointing_up: true,
        radius,
        bluntness: f64::from(cfg.stalagmite_bluntness.sample(rng)),
        scale: f64::from(cfg.height_scale.sample(rng)),
    };

    let wind = if stalactite.radius >= cfg.min_radius_for_wind
        && stalactite.bluntness >= f64::from(cfg.min_bluntness_for_wind)
        && stalagmite.radius >= cfg.min_radius_for_wind
        && stalagmite.bluntness >= f64::from(cfg.min_bluntness_for_wind)
    {
        let speed = cfg.wind_speed.sample(rng);
        let dir = random_between_f32(rng, 0.0_f32, MTH_PI);
        WindOffsetter {
            origin_y: origin.y,
            wind: Some((
                f64::from(mth_cos(f64::from(dir)) * speed),
                f64::from(mth_sin(f64::from(dir)) * speed),
                16 - radius,
            )),
        }
    } else {
        WindOffsetter {
            origin_y: origin.y,
            wind: None,
        }
    };

    let s1 = stalactite.embed_base(region, wind);
    let s2 = stalagmite.embed_base(region, wind);
    if s1 {
        stalactite.place_blocks(region, rng, wind);
    }
    if s2 {
        stalagmite.place_blocks(region, rng, wind);
    }
    true
}

// ---------------------------------------------------------------------------
// §4.31 `minecraft:root_system`
// ---------------------------------------------------------------------------

fn space_for_tree(region: &GenerationRegion, pos: BlockPos, cfg: &RootSystemConfig) -> bool {
    let ops = region.ops();
    for i in 1..=cfg.required_vertical_space_for_tree {
        let s = region.get_block(pos.above_n(i));
        let ok =
            ops.is_air(s) || (i < cfg.allowed_vertical_water_for_tree && ops.is_water_fluid(s));
        if !ok {
            return false;
        }
    }
    if cfg.level_test_distance > 0 {
        for i in 0..4 {
            let dir = direction_from_2d_data_value(i);
            let corner = pos.relative_n(dir, cfg.level_test_distance);
            if ops.is_air(region.get_block(corner.below_n(cfg.max_level_deviation)))
                || !ops.is_air(region.get_block(corner.above_n(cfg.max_level_deviation)))
            {
                return false;
            }
        }
    }
    true
}

/// §4.31 `minecraft:root_system`.
pub fn place_root_system(
    ctx: &FeatureContext<'_>,
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    cfg: &RootSystemConfig,
) -> bool {
    if !region.ops().is_air(region.get_block(origin)) {
        return false;
    }

    // placeDirtAndTree
    let mut working_pos = origin;
    let mut placed_tree = false;
    for y in 0..cfg.root_column_max_height {
        working_pos = working_pos.above();
        if region.get_height(HeightmapKind::WorldSurface, working_pos.x, working_pos.z)
            < working_pos.y
        {
            break;
        }
        if cfg.allowed_tree_position.test(region, working_pos)
            && space_for_tree(region, working_pos, cfg)
        {
            let below_pos = working_pos.below();
            let below_st = region.get_block(below_pos);
            if region.ops().is_lava_fluid(below_st) || !region.ops().is_solid(below_st) {
                break;
            }
            if ctx.place_sub_feature(&cfg.feature, region, rng, working_pos) {
                // placeDirt: yy in origin.y .. origin.y + y (exclusive)
                for yy in origin.y..(origin.y + y) {
                    for _ in 0..cfg.root_placement_attempts {
                        let dx = rng.next_int_bounded(cfg.root_radius)
                            - rng.next_int_bounded(cfg.root_radius);
                        let dz = rng.next_int_bounded(cfg.root_radius)
                            - rng.next_int_bounded(cfg.root_radius);
                        let p = BlockPos::new(origin.x + dx, yy, origin.z + dz);
                        if cfg
                            .root_replaceable
                            .contains(region.ops().block_id(region.get_block(p)))
                        {
                            let st = cfg.root_state_provider.get_state(region, rng, p);
                            region.set_block(p, st, 2);
                        }
                    }
                }
                placed_tree = true;
                break;
            }
        }
    }

    if placed_tree {
        for _ in 0..cfg.hanging_root_placement_attempts {
            let dx = rng.next_int_bounded(cfg.hanging_root_radius)
                - rng.next_int_bounded(cfg.hanging_root_radius);
            let dy = rng.next_int_bounded(cfg.hanging_roots_vertical_span)
                - rng.next_int_bounded(cfg.hanging_roots_vertical_span);
            let dz = rng.next_int_bounded(cfg.hanging_root_radius)
                - rng.next_int_bounded(cfg.hanging_root_radius);
            let p = origin.offset(dx, dy, dz);
            if region.is_empty_block(p) {
                let target_state = cfg.hanging_root_state_provider.get_state(region, rng, p);
                let ops = region.ops();
                if ops.can_survive(region, p, target_state)
                    && ops.is_face_sturdy(region.get_block(p.above()), Direction::Down)
                {
                    region.set_block(p, target_state, 2);
                }
            }
        }
    }

    true
}

// ---------------------------------------------------------------------------
// §4.32 `minecraft:speleothem`
// ---------------------------------------------------------------------------

/// §4.32 `minecraft:speleothem`.
pub fn place_speleothem(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    cfg: &SpeleothemConfig,
) -> bool {
    let ops = region.ops_arc().clone();
    let can_above = is_speleothem_base(
        &ops,
        region.get_block(origin.above()),
        cfg.base_block_id,
        &cfg.replaceable_blocks,
    );
    let can_below = is_speleothem_base(
        &ops,
        region.get_block(origin.below()),
        cfg.base_block_id,
        &cfg.replaceable_blocks,
    );
    let tip_dir = match (can_above, can_below) {
        (true, true) => {
            if rng.next_boolean() {
                Direction::Down
            } else {
                Direction::Up
            }
        }
        (true, false) => Direction::Down,
        (false, true) => Direction::Up,
        (false, false) => return false,
    };

    let root_pos = origin.relative(tip_dir.opposite());
    place_base_block_if_possible(
        region,
        root_pos,
        cfg.base_default_state,
        &cfg.replaceable_blocks,
    );
    for dir in Direction::HORIZONTAL {
        if !(rng.next_float() > cfg.chance_of_directional_spread) {
            let pos1 = root_pos.relative(dir);
            place_base_block_if_possible(
                region,
                pos1,
                cfg.base_default_state,
                &cfg.replaceable_blocks,
            );
            if !(rng.next_float() > cfg.chance_of_spread_radius2) {
                let pos2 = pos1.relative(direction_get_random(rng));
                place_base_block_if_possible(
                    region,
                    pos2,
                    cfg.base_default_state,
                    &cfg.replaceable_blocks,
                );
                if !(rng.next_float() > cfg.chance_of_spread_radius3) {
                    let pos3 = pos2.relative(direction_get_random(rng));
                    place_base_block_if_possible(
                        region,
                        pos3,
                        cfg.base_default_state,
                        &cfg.replaceable_blocks,
                    );
                }
            }
        }
    }

    let height = if rng.next_float() < cfg.chance_of_taller_generation
        && is_empty_or_water(&ops, region.get_block(origin.relative(tip_dir)))
    {
        2
    } else {
        1
    };
    grow_speleothem(
        region,
        origin,
        tip_dir,
        height,
        false,
        cfg.base_block_id,
        cfg.pointed_default_state,
        &cfg.replaceable_blocks,
    );
    true
}

// ---------------------------------------------------------------------------
// §4.33 `minecraft:speleothem_cluster`
// ---------------------------------------------------------------------------

fn cluster_can_place_pool(
    region: &GenerationRegion,
    p: BlockPos,
    base_block_id: u32,
    pointed_block_id: u32,
) -> bool {
    let ops = region.ops();
    let s = region.get_block(p);
    let bid = ops.block_id(s);
    if bid == ops.bid_water || bid == base_block_id || bid == pointed_block_id {
        return false;
    }
    if ops.is_water_fluid(region.get_block(p.above())) {
        return false;
    }
    for dir in Direction::HORIZONTAL {
        let adj = region.get_block(p.relative(dir));
        if !ops.tag_base_stone_overworld.contains(ops.block_id(adj)) && !ops.is_water_fluid(adj) {
            return false;
        }
    }
    let below = region.get_block(p.below());
    ops.tag_base_stone_overworld.contains(ops.block_id(below)) || ops.is_water_fluid(below)
}

fn cluster_sample_height(
    rng: &mut WorldgenRandom,
    dx: i32,
    dz: i32,
    density: f32,
    max_h: i32,
    cfg: &SpeleothemClusterConfig,
) -> i32 {
    if rng.next_float() > density {
        return 0;
    }
    let dist = dx.abs() + dz.abs();
    let mean = clamped_map_f64(
        f64::from(dist),
        0.0,
        f64::from(cfg.max_distance_from_center_affecting_height_bias),
        f64::from(max_h) / 2.0,
        0.0,
    ) as f32;
    let sampled = clamp_f32(
        mth_normal(rng, mean, cfg.height_deviation as f32),
        0.0_f32,
        max_h as f32,
    );
    sampled as i32
}

/// §4.33 `minecraft:speleothem_cluster`.
pub fn place_speleothem_cluster(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    cfg: &SpeleothemClusterConfig,
) -> bool {
    let ops = region.ops_arc().clone();
    if !is_empty_or_water(&ops, region.get_block(origin)) {
        return false;
    }
    let cluster_height = cfg.height.sample(rng);
    let wet = cfg.wetness.sample(rng);
    let dens = cfg.density.sample(rng);
    let x_radius = cfg.radius.sample(rng);
    let z_radius = cfg.radius.sample(rng);

    for dx in -x_radius..=x_radius {
        for dz in -z_radius..=z_radius {
            let dist_from_edge = (x_radius - dx.abs()).min(z_radius - dz.abs());
            let chance = f64::from(clamped_map_f32(
                dist_from_edge as f32,
                0.0_f32,
                cfg.max_distance_from_edge_affecting_chance_of_speleothem as f32,
                cfg.chance_of_speleothem_at_max_distance_from_center,
                1.0_f32,
            ));
            let col_pos = origin.offset(dx, 0, dz);
            let Some(base_col) = Column::scan(
                region,
                col_pos,
                cfg.floor_to_ceiling_search_range,
                |s| is_empty_or_water(&ops, s),
                |s| !is_empty_or_water(&ops, s),
            ) else {
                continue;
            };
            let ceiling = base_col.ceiling();
            let base_floor = base_col.floor();
            if ceiling.is_none() && base_floor.is_none() {
                continue;
            }

            let want_pool = rng.next_float() < wet;
            let column = if want_pool
                && let Some(bf) = base_floor
                && cluster_can_place_pool(
                    region,
                    col_pos.at_y(bf),
                    cfg.base_block_id,
                    cfg.pointed_block_id,
                ) {
                region.set_block(col_pos.at_y(bf), ops.sid_water, 2);
                base_col.with_floor(bf - 1)
            } else {
                base_col
            };
            let floor = column.floor();

            // Stalactite (RNG drawn unconditionally!)
            let want_stalactite = rng.next_double() < chance;
            let stalactite_height = if let Some(ceil_y) = ceiling
                && want_stalactite
                && !ops.is_lava(region.get_block(col_pos.at_y(ceil_y)))
            {
                let thickness = cfg.speleothem_block_layer_thickness.sample(rng);
                for t in 0..thickness {
                    if !place_base_block_if_possible(
                        region,
                        col_pos.at_y(ceil_y + t),
                        cfg.base_default_state,
                        &cfg.replaceable_blocks,
                    ) {
                        break;
                    }
                }
                let max_h = match floor {
                    Some(fl_y) => cluster_height.min(ceil_y - fl_y),
                    None => cluster_height,
                };
                cluster_sample_height(rng, dx, dz, dens, max_h, cfg)
            } else {
                0
            };

            // Stalagmite (RNG drawn unconditionally!)
            let want_stalagmite = rng.next_double() < chance;
            let stalagmite_height = if let Some(fl_y) = floor
                && want_stalagmite
                && !ops.is_lava(region.get_block(col_pos.at_y(fl_y)))
            {
                let thickness = cfg.speleothem_block_layer_thickness.sample(rng);
                for t in 0..thickness {
                    if !place_base_block_if_possible(
                        region,
                        col_pos.at_y(fl_y - t),
                        cfg.base_default_state,
                        &cfg.replaceable_blocks,
                    ) {
                        break;
                    }
                }
                if ceiling.is_some() {
                    let diff = random_between_inclusive(
                        rng,
                        -cfg.max_stalagmite_stalactite_height_diff,
                        cfg.max_stalagmite_stalactite_height_diff,
                    );
                    (stalactite_height + diff).max(0)
                } else {
                    cluster_sample_height(rng, dx, dz, dens, cluster_height, cfg)
                }
            } else {
                0
            };

            // Collision resolution
            let (actual_stalactite, actual_stalagmite) = if let (Some(ceil_y), Some(fl_y)) =
                (ceiling, floor)
                && ceil_y - stalactite_height <= fl_y + stalagmite_height
            {
                let low_bot = (ceil_y - stalactite_height).max(fl_y + 1);
                let high_top = (fl_y + stalagmite_height).min(ceil_y - 1);
                let actual_bot = random_between_inclusive(rng, low_bot, high_top + 1);
                (ceil_y - actual_bot, (actual_bot - 1) - fl_y)
            } else {
                (stalactite_height, stalagmite_height)
            };

            // Quirk: nextBoolean is ALWAYS drawn first!
            let merge_tips = rng.next_boolean()
                && actual_stalactite > 0
                && actual_stalagmite > 0
                && column
                    .height()
                    .is_some_and(|h| actual_stalactite + actual_stalagmite == h);

            if let Some(ceil_y) = ceiling {
                grow_speleothem(
                    region,
                    col_pos.at_y(ceil_y - 1),
                    Direction::Down,
                    actual_stalactite,
                    merge_tips,
                    cfg.base_block_id,
                    cfg.pointed_default_state,
                    &cfg.replaceable_blocks,
                );
            }
            if let Some(fl_y) = floor {
                grow_speleothem(
                    region,
                    col_pos.at_y(fl_y + 1),
                    Direction::Up,
                    actual_stalagmite,
                    merge_tips,
                    cfg.base_block_id,
                    cfg.pointed_default_state,
                    &cfg.replaceable_blocks,
                );
            }
        }
    }

    true
}
