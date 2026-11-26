//! Group B: Ores, replacements, blobs, disks, columns, simple blocks, and springs
//! (`features.md` §4.8–§4.21).

use zenith_blocks::{Direction, FluidState};
use zenith_math::RandomSource;
use zenith_math::helpers::interp::lerp_f64;
use zenith_math::helpers::intrinsics::{java_math_cos, java_math_sin};
use zenith_math::helpers::rounding::{ceil_f32, floor_f64, java_round_f32};
use zenith_math::helpers::trig::{PI as MTH_PI, sin as mth_sin};
use zenith_worldgen::HeightmapKind;

use crate::block_ops::BlockBitSet;
use crate::features::{BlockColumnLayer, FeatureContext, OreConfig};
use crate::placement::PlacedFeature;
use crate::providers::{BlockPredicate, BlockReplacement, BlockStateProvider, IntProvider};
use crate::region::GenerationRegion;
use crate::rng::{BlockPos, WorldgenRandom, between_closed, within_box_by_manhattan_distance};

fn can_place_ore(
    state_id: u32,
    region: &GenerationRegion,
    rng: &mut WorldgenRandom,
    target: &BlockReplacement,
    discard_chance: f32,
    pos: BlockPos,
) -> bool {
    if !target.target.test(state_id, pos, rng, region) {
        return false;
    }
    let skip_air_check = if discard_chance <= 0.0_f32 {
        true
    } else if discard_chance >= 1.0_f32 {
        false
    } else {
        rng.next_float() >= discard_chance
    };
    if skip_air_check {
        return true;
    }
    let ops = region.ops();
    for dir in Direction::ALL {
        if ops.is_air(region.get_block(pos.relative(dir))) {
            return false;
        }
    }
    true
}

/// §4.8 `minecraft:ore`.
pub fn place_ore(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    cfg: &OreConfig,
) -> bool {
    let dir = rng.next_float() * MTH_PI;
    let spread_xy = (cfg.size as f32) / 8.0_f32;
    let max_radius = ceil_f32(((cfg.size as f32) / 16.0_f32 * 2.0_f32 + 1.0_f32) / 2.0_f32);
    let sin_dir = java_math_sin(f64::from(dir));
    let cos_dir = java_math_cos(f64::from(dir));
    let x0 = f64::from(origin.x) + sin_dir * f64::from(spread_xy);
    let x1 = f64::from(origin.x) - sin_dir * f64::from(spread_xy);
    let z0 = f64::from(origin.z) + cos_dir * f64::from(spread_xy);
    let z1 = f64::from(origin.z) - cos_dir * f64::from(spread_xy);
    let y0 = f64::from(origin.y + rng.next_int_bounded(3) - 2);
    let y1 = f64::from(origin.y + rng.next_int_bounded(3) - 2);

    let x_start = origin.x - ceil_f32(spread_xy) - max_radius;
    let y_start = origin.y - 2 - max_radius;
    let z_start = origin.z - ceil_f32(spread_xy) - max_radius;
    let size_xz = 2 * (ceil_f32(spread_xy) + max_radius);
    let size_y = 2 * (2 + max_radius);

    for xp in x_start..=x_start + size_xz {
        for zp in z_start..=z_start + size_xz {
            if y_start <= region.get_height(HeightmapKind::OceanFloorWg, xp, zp) {
                return do_place_ore(
                    region, rng, cfg, x0, x1, z0, z1, y0, y1, x_start, y_start, z_start, size_xz,
                    size_y,
                );
            }
        }
    }
    false
}

#[allow(clippy::too_many_arguments)]
fn do_place_ore(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    cfg: &OreConfig,
    x0: f64,
    x1: f64,
    z0: f64,
    z1: f64,
    y0: f64,
    y1: f64,
    x_start: i32,
    y_start: i32,
    z_start: i32,
    size_xz: i32,
    size_y: i32,
) -> bool {
    let size = cfg.size as usize;
    let bit_count = (size_xz as usize) * (size_y as usize) * (size_xz as usize);
    let mut tested = vec![0u64; bit_count.div_ceil(64)];
    let mut data = vec![0.0_f64; size * 4];

    for i in 0..size {
        let step = (i as f32) / (cfg.size as f32);
        let xx = lerp_f64(f64::from(step), x0, x1);
        let yy = lerp_f64(f64::from(step), y0, y1);
        let zz = lerp_f64(f64::from(step), z0, z1);
        let ss = rng.next_double() * f64::from(cfg.size) / 16.0;
        let r = (f64::from(mth_sin(f64::from(MTH_PI * step)) + 1.0_f32) * ss + 1.0) / 2.0;
        data[i * 4] = xx;
        data[i * 4 + 1] = yy;
        data[i * 4 + 2] = zz;
        data[i * 4 + 3] = r;
    }

    // Contained-sphere culling
    for i1 in 0..size.saturating_sub(1) {
        if data[i1 * 4 + 3] <= 0.0 {
            continue;
        }
        for i2 in (i1 + 1)..size {
            if data[i2 * 4 + 3] <= 0.0 {
                continue;
            }
            let dx = data[i1 * 4] - data[i2 * 4];
            let dy = data[i1 * 4 + 1] - data[i2 * 4 + 1];
            let dz = data[i1 * 4 + 2] - data[i2 * 4 + 2];
            let dr = data[i1 * 4 + 3] - data[i2 * 4 + 3];
            if dr * dr > dx * dx + dy * dy + dz * dz {
                if dr > 0.0 {
                    data[i2 * 4 + 3] = -1.0;
                } else {
                    data[i1 * 4 + 3] = -1.0;
                }
            }
        }
    }

    let mut placed = 0i32;
    for i in 0..size {
        let r = data[i * 4 + 3];
        if r < 0.0 {
            continue;
        }
        let xx = data[i * 4];
        let yy = data[i * 4 + 1];
        let zz = data[i * 4 + 2];

        let x_min = floor_f64(xx - r).max(x_start);
        let y_min = floor_f64(yy - r).max(y_start);
        let z_min = floor_f64(zz - r).max(z_start);
        let x_max = floor_f64(xx + r).max(x_min);
        let y_max = floor_f64(yy + r).max(y_min);
        let z_max = floor_f64(zz + r).max(z_min);

        for x in x_min..=x_max {
            let xd = (f64::from(x) + 0.5 - xx) / r;
            if xd * xd >= 1.0 {
                continue;
            }
            for y in y_min..=y_max {
                let yd = (f64::from(y) + 0.5 - yy) / r;
                if xd * xd + yd * yd >= 1.0 {
                    continue;
                }
                if region.is_outside_build_height(y) {
                    continue;
                }
                for z in z_min..=z_max {
                    let zd = (f64::from(z) + 0.5 - zz) / r;
                    if xd * xd + yd * yd + zd * zd >= 1.0 {
                        continue;
                    }
                    let bit_idx = ((x - x_start)
                        + (y - y_start) * size_xz
                        + (z - z_start) * size_xz * size_y)
                        as usize;
                    let word = bit_idx >> 6;
                    let mask = 1u64 << (bit_idx & 63);
                    if (tested[word] & mask) != 0 {
                        continue;
                    }
                    tested[word] |= mask;

                    let ore_pos = BlockPos::new(x, y, z);
                    if !region.ensure_can_write(ore_pos) {
                        continue;
                    }
                    let block_state = region.get_block(ore_pos);
                    for target in &cfg.targets {
                        if can_place_ore(
                            block_state,
                            region,
                            rng,
                            target,
                            cfg.discard_chance_on_air_exposure,
                            ore_pos,
                        ) {
                            // BulkSectionAccess writes directly into chunk section (no heightmap update / post-process).
                            region.set_block_bulk(ore_pos, target.state);
                            placed += 1;
                            break;
                        }
                    }
                }
            }
        }
    }

    placed > 0
}

/// §4.9 `minecraft:scattered_ore`.
pub fn place_scattered_ore(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    cfg: &OreConfig,
) -> bool {
    let tries = rng.next_int_bounded(cfg.size + 1);
    for i in 0..tries {
        let max_dist = i.min(7) as f32;
        let xd = java_round_f32((rng.next_float() - rng.next_float()) * max_dist);
        let yd = java_round_f32((rng.next_float() - rng.next_float()) * max_dist);
        let zd = java_round_f32((rng.next_float() - rng.next_float()) * max_dist);
        let target_pos = origin.offset(xd, yd, zd);
        let block_state = region.get_block(target_pos);
        for target in &cfg.targets {
            if can_place_ore(
                block_state,
                region,
                rng,
                target,
                cfg.discard_chance_on_air_exposure,
                target_pos,
            ) {
                region.set_block(target_pos, target.state, 2);
                break;
            }
        }
    }
    true
}

/// §4.10 `minecraft:replace_single_block`.
pub fn place_replace_single_block(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    targets: &[BlockReplacement],
) -> bool {
    let cur = region.get_block(origin);
    for target in targets {
        if target.target.test(cur, origin, rng, region) {
            region.set_block(origin, target.state, 2);
            break;
        }
    }
    true
}

/// §4.11 `minecraft:netherrack_replace_blobs`.
pub fn place_replace_blobs(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    target_block_id: u32,
    state: u32,
    radius: &IntProvider,
) -> bool {
    let mut cursor = origin.at_y(origin.y.clamp(region.min_y() + 1, region.max_y()));
    let mut found = false;
    while cursor.y > region.min_y() + 1 {
        if region.ops().block_id(region.get_block(cursor)) == target_block_id {
            found = true;
            break;
        }
        cursor = cursor.below();
    }
    if !found {
        return false;
    }
    let rx = radius.sample(rng);
    let ry = radius.sample(rng);
    let rz = radius.sample(rng);
    let max_r = rx.max(ry.max(rz));
    let mut replaced_any = false;
    within_box_by_manhattan_distance(cursor, rx, ry, rz, |pos| {
        if pos.dist_manhattan(cursor) > max_r {
            return false;
        }
        if region.ops().block_id(region.get_block(pos)) == target_block_id {
            region.set_block(pos, state, 3);
            replaced_any = true;
        }
        true
    });
    replaced_any
}

/// §4.12 `minecraft:disk`.
pub fn place_disk(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    state_provider: &BlockStateProvider,
    target: &BlockPredicate,
    radius: &IntProvider,
    half_height: i32,
) -> bool {
    let top = origin.y + half_height;
    let bottom = origin.y - half_height - 1;
    let r = radius.sample(rng);
    let mut placed_any = false;
    for col in between_closed(origin.offset(-r, 0, -r), origin.offset(r, 0, r)) {
        let dx = col.x - origin.x;
        let dz = col.z - origin.z;
        if dx * dx + dz * dz <= r * r {
            let mut placed_above = false;
            let mut y = top;
            while y > bottom {
                let pos = BlockPos::new(col.x, y, col.z);
                if target.test(region, pos) {
                    if let Some(st) = state_provider.get_optional_state(region, rng, pos) {
                        region.set_block(pos, st, 2);
                        if !placed_above {
                            region.mark_above_for_post_processing(pos);
                        }
                        placed_any = true;
                        placed_above = true;
                    }
                } else {
                    placed_above = false;
                }
                y -= 1;
            }
        }
    }
    placed_any
}

/// §4.13 `minecraft:simple_block`.
pub fn place_simple_block(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    to_place: &BlockStateProvider,
    schedule_tick: bool,
) -> bool {
    let Some(state) = to_place.get_optional_state(region, rng, origin) else {
        return false;
    };
    let ops = region.ops_arc().clone();
    if !ops.can_survive(region, origin, state) {
        return false;
    }
    if ops.is_double_plant(state) {
        let above = region.get_block(origin.above());
        if !ops.is_air(above)
            && (ops.fluid(state) != ops.fluid(above) || !ops.is_replaceable(above))
        {
            return false;
        }
        ops.place_double_plant(region, state, origin, 2);
    } else if ops.block_class(state) == "MossyCarpetBlock" {
        ops.place_mossy_carpet(region, origin, 2);
    } else {
        region.set_block(origin, state, 2);
    }
    if schedule_tick {
        let placed_block = ops.block_id(region.get_block(origin));
        region.schedule_block_tick(origin, placed_block, 1);
    }
    true
}

/// §4.14 `minecraft:block_blob`.
pub fn place_block_blob(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    mut origin: BlockPos,
    state: u32,
    can_place_on: &BlockPredicate,
) -> bool {
    while origin.y > region.min_y() + 3 && !can_place_on.test(region, origin.below()) {
        origin = origin.below();
    }
    if origin.y <= region.min_y() + 3 {
        return false;
    }
    for _ in 0..3 {
        let xr = rng.next_int_bounded(2);
        let yr = rng.next_int_bounded(2);
        let zr = rng.next_int_bounded(2);
        let tr = ((xr + yr + zr) as f32) * 0.333_f32 + 0.5_f32;
        let max_d2 = f64::from(tr * tr);
        for pos in between_closed(origin.offset(-xr, -yr, -zr), origin.offset(xr, yr, zr)) {
            if pos.dist_sqr(origin) <= max_d2 {
                region.set_block(pos, state, 3);
            }
        }
        let ox = -1 + rng.next_int_bounded(2);
        let oy = -rng.next_int_bounded(2);
        let oz = -1 + rng.next_int_bounded(2);
        origin = origin.offset(ox, oy, oz);
    }
    true
}

/// §4.15 `minecraft:block_column`.
pub fn place_block_column(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    layers: &[BlockColumnLayer],
    direction: Direction,
    allowed_placement: &BlockPredicate,
    prioritize_tip: bool,
) -> bool {
    let mut layer_heights = Vec::with_capacity(layers.len());
    let mut total_height = 0i32;
    for layer in layers {
        let h = layer.height.sample(rng);
        layer_heights.push(h);
        total_height += h;
    }
    if total_height == 0 {
        return false;
    }

    // Truncation scan starts at origin.relative(direction), NOT origin!
    let mut next_pos = origin.relative(direction);
    for y in 0..total_height {
        if !allowed_placement.test(region, next_pos) {
            let mut to_remove = total_height - y;
            if prioritize_tip {
                for lh in &mut layer_heights {
                    let sub = (*lh).min(to_remove);
                    *lh -= sub;
                    to_remove -= sub;
                    if to_remove == 0 {
                        break;
                    }
                }
            } else {
                for lh in layer_heights.iter_mut().rev() {
                    let sub = (*lh).min(to_remove);
                    *lh -= sub;
                    to_remove -= sub;
                    if to_remove == 0 {
                        break;
                    }
                }
            }
            break;
        }
        next_pos = next_pos.relative(direction);
    }

    let mut place_pos = origin;
    for (layer, &h) in layers.iter().zip(&layer_heights) {
        for _ in 0..h {
            let st = layer.provider.get_state(region, rng, place_pos);
            region.set_block(place_pos, st, 2);
            place_pos = place_pos.relative(direction);
        }
    }
    true
}

/// §4.16 `minecraft:block_pile`.
pub fn place_block_pile(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    state_provider: &BlockStateProvider,
) -> bool {
    if origin.y < region.min_y() + 5 {
        return false;
    }
    let xr = 2 + rng.next_int_bounded(2);
    let zr = 2 + rng.next_int_bounded(2);
    for pos in between_closed(origin.offset(-xr, 0, -zr), origin.offset(xr, 1, zr)) {
        let xd = origin.x - pos.x;
        let zd = origin.z - pos.z;
        let cond1 = ((xd * xd + zd * zd) as f32)
            <= rng.next_float() * 10.0_f32 - rng.next_float() * 6.0_f32;
        if cond1 || f64::from(rng.next_float()) < 0.031 {
            try_place_pile_block(region, rng, pos, state_provider);
        }
    }
    true
}

fn try_place_pile_block(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    pos: BlockPos,
    state_provider: &BlockStateProvider,
) {
    if !region.is_empty_block(pos) {
        return;
    }
    let below = region.get_block(pos.below());
    let may_place = if region.ops().block_id(below) == region.ops().bid_dirt_path {
        rng.next_boolean()
    } else {
        region.ops().is_face_sturdy(below, Direction::Up)
    };
    if may_place {
        let st = state_provider.get_state(region, rng, pos);
        region.set_block(pos, st, 260);
    }
}

/// §4.17 `minecraft:fill_layer`.
pub fn place_fill_layer(
    region: &mut GenerationRegion,
    origin: BlockPos,
    height: i32,
    state: u32,
) -> bool {
    let y = region.min_y() + height;
    for dx in 0..16 {
        for dz in 0..16 {
            let pos = BlockPos::new(origin.x + dx, y, origin.z + dz);
            if region.ops().is_air(region.get_block(pos)) {
                region.set_block(pos, state, 2);
            }
        }
    }
    true
}

/// §4.18 `minecraft:projected_random_patchy_square`.
pub fn place_projected_random_patchy_square(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    block: &BlockStateProvider,
    project_through: &BlockPredicate,
    size: &IntProvider,
    max_projection_height: i32,
) -> bool {
    let sz = size.sample(rng);
    let bound = sz * sz + 1;
    for dx in -sz..=sz {
        for dz in -sz..=sz {
            let prob = dx.abs() * dz.abs();
            if rng.next_int_bounded(bound) < bound - prob {
                let mut base_pos = origin.offset(dx, 0, dz);
                let mut drop = max_projection_height;
                while project_through.test(region, base_pos.below()) {
                    base_pos = base_pos.below();
                    drop -= 1;
                    if drop <= 0 {
                        break;
                    }
                }
                if let Some(st) = block.get_optional_state(region, rng, base_pos) {
                    region.set_block(base_pos, st, 2);
                }
            }
        }
    }
    true
}

/// §4.19 `minecraft:random_neighbor_spread`.
#[allow(clippy::too_many_arguments)]
pub fn place_random_neighbor_spread(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    block: &BlockStateProvider,
    accepted_neighbors: &BlockBitSet,
    can_replace: &BlockPredicate,
    attempts: &IntProvider,
    xz_offset: &IntProvider,
    y_offset: &IntProvider,
) -> bool {
    let s0 = block.get_state(region, rng, origin);
    region.set_block(origin, s0, 2);
    let n = attempts.sample(rng);
    for _ in 0..n {
        let dx = xz_offset.sample(rng);
        let dy = y_offset.sample(rng);
        let dz = xz_offset.sample(rng);
        let place_pos = origin.offset(dx, dy, dz);
        if can_replace.test(region, place_pos) {
            let mut neighbours = 0;
            let ops = region.ops();
            for dir in Direction::ALL {
                if accepted_neighbors
                    .contains(ops.block_id(region.get_block(place_pos.relative(dir))))
                {
                    neighbours += 1;
                    if neighbours > 1 {
                        break;
                    }
                }
            }
            if neighbours == 1 {
                let st = block.get_state(region, rng, place_pos);
                region.set_block(place_pos, st, 2);
            }
        }
    }
    true
}

/// §4.20 `minecraft:single_block_pillar`.
#[allow(clippy::too_many_arguments)]
pub fn place_single_block_pillar(
    ctx: &FeatureContext<'_>,
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    block: &BlockStateProvider,
    can_replace: &BlockPredicate,
    direction: Direction,
    chance_to_continue: f32,
    cap_feature: Option<&PlacedFeature>,
) -> bool {
    let mut pos = origin;
    while can_replace.test(region, pos)
        && rng.next_float() < chance_to_continue
        && !region.is_outside_build_height(pos.y)
    {
        let st = block.get_state(region, rng, pos);
        region.set_block(pos, st, 2);
        pos = pos.relative(direction);
    }
    pos = pos.relative(direction.opposite());
    if let Some(cap) = cap_feature {
        ctx.place_sub_feature(cap, region, rng, pos);
    }
    true
}

/// §4.21 `minecraft:spring_feature`.
#[allow(clippy::too_many_arguments)]
pub fn place_spring(
    region: &mut GenerationRegion,
    origin: BlockPos,
    fluid_state: FluidState,
    legacy_block_state: u32,
    requires_block_below: bool,
    rock_count: i32,
    hole_count: i32,
    valid_blocks: &BlockBitSet,
) -> bool {
    let ops = region.ops();
    if !valid_blocks.contains(ops.block_id(region.get_block(origin.above()))) {
        return false;
    }
    if requires_block_below
        && !valid_blocks.contains(ops.block_id(region.get_block(origin.below())))
    {
        return false;
    }
    let cur = region.get_block(origin);
    if !ops.is_air(cur) && !valid_blocks.contains(ops.block_id(cur)) {
        return false;
    }
    let mut rocks = 0;
    let mut holes = 0;
    for dir in [
        Direction::West,
        Direction::East,
        Direction::North,
        Direction::South,
        Direction::Down,
    ] {
        let p = origin.relative(dir);
        let s = region.get_block(p);
        if valid_blocks.contains(ops.block_id(s)) {
            rocks += 1;
        }
        if ops.is_air(s) {
            holes += 1;
        }
    }
    if rocks == rock_count && holes == hole_count {
        region.set_block(origin, legacy_block_state, 2);
        region.schedule_fluid_tick(origin, fluid_state.kind, 0);
        true
    } else {
        false
    }
}
