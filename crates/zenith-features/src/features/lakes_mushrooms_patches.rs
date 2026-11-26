//! Group H: Lakes, huge mushrooms/fungi, vegetation patches, void platform, and multiface growth
//! (`features.md` §4.51–§4.58).

use zenith_blocks::Direction;
use zenith_math::RandomSource;
use zenith_worldgen::CaveSurface;

use crate::block_ops::BlockBitSet;
use crate::features::ice_caves::biome_should_freeze;
use crate::features::{FeatureContext, HugeMushroomConfig, VegetationPatchConfig};
use crate::java_hash::JavaBlockPosSet;
use crate::providers::{BlockPredicate, BlockStateProvider};
use crate::region::GenerationRegion;
use crate::rng::{BlockPos, WorldgenRandom, mth_next_int, util_shuffle};

/// §4.51 `minecraft:lake`.
#[allow(clippy::too_many_arguments)]
pub fn place_lake(
    ctx: &FeatureContext<'_>,
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    fluid: &BlockStateProvider,
    barrier: &BlockStateProvider,
    can_place_feature: &BlockPredicate,
    can_replace_with_air_or_fluid: &BlockPredicate,
    can_replace_with_barrier: &BlockPredicate,
) -> bool {
    if origin.y <= region.min_y() + 4 {
        return false;
    }
    let origin = origin.offset(-8, -4, -8);
    let mut grid = [false; 2048];
    let spots = rng.next_int_bounded(4) + 4;
    for _ in 0..spots {
        let xr = rng.next_double() * 6.0 + 3.0;
        let yr = rng.next_double() * 4.0 + 2.0;
        let zr = rng.next_double() * 6.0 + 3.0;
        let xp = rng.next_double() * (16.0 - xr - 2.0) + 1.0 + xr / 2.0;
        let yp = rng.next_double() * (8.0 - yr - 4.0) + 2.0 + yr / 2.0;
        let zp = rng.next_double() * (16.0 - zr - 2.0) + 1.0 + zr / 2.0;
        for xx in 1..15usize {
            for zz in 1..15usize {
                for yy in 1..7usize {
                    let xd = (xx as f64 - xp) / (xr / 2.0);
                    let yd = (yy as f64 - yp) / (yr / 2.0);
                    let zd = (zz as f64 - zp) / (zr / 2.0);
                    if xd * xd + yd * yd + zd * zd < 1.0 {
                        grid[(xx * 16 + zz) * 8 + yy] = true;
                    }
                }
            }
        }
    }

    let fluid_state = fluid.get_state(region, rng, origin);
    let ops = region.ops_arc().clone();

    let is_boundary = |xx: usize, yy: usize, zz: usize| -> bool {
        !grid[(xx * 16 + zz) * 8 + yy]
            && ((xx < 15 && grid[((xx + 1) * 16 + zz) * 8 + yy])
                || (xx > 0 && grid[((xx - 1) * 16 + zz) * 8 + yy])
                || (zz < 15 && grid[(xx * 16 + zz + 1) * 8 + yy])
                || (zz > 0 && grid[(xx * 16 + zz - 1) * 8 + yy])
                || (yy < 7 && grid[(xx * 16 + zz) * 8 + yy + 1])
                || (yy > 0 && grid[(xx * 16 + zz) * 8 + yy - 1]))
    };

    // Boundary check
    for xx in 0..16usize {
        for zz in 0..16usize {
            for yy in 0..8usize {
                if is_boundary(xx, yy, zz) {
                    let offset_pos = origin.offset(xx as i32, yy as i32, zz as i32);
                    let s = region.get_block(offset_pos);
                    if yy >= 4 && ops.is_liquid(s) {
                        return false;
                    }
                    if yy < 4 && !ops.is_solid(s) && s != fluid_state {
                        return false;
                    }
                    if !can_place_feature.test(region, offset_pos) {
                        return false;
                    }
                }
            }
        }
    }

    // Carve & fill
    for xx in 0..16usize {
        for zz in 0..16usize {
            for yy in 0..8usize {
                if grid[(xx * 16 + zz) * 8 + yy] {
                    let p = origin.offset(xx as i32, yy as i32, zz as i32);
                    if can_replace_with_air_or_fluid.test(region, p) {
                        if yy >= 4 {
                            region.set_block(p, ops.sid_cave_air, 2);
                            region.schedule_block_tick(p, ops.bid_cave_air, 0);
                            region.mark_above_for_post_processing(p);
                        } else {
                            region.set_block(p, fluid_state, 2);
                        }
                    }
                }
            }
        }
    }

    // Barrier shell
    let barrier_state = barrier.get_state(region, rng, origin);
    if !ops.is_air(barrier_state) {
        for xx in 0..16usize {
            for zz in 0..16usize {
                for yy in 0..8usize {
                    if is_boundary(xx, yy, zz) && (yy < 4 || rng.next_int_bounded(2) != 0) {
                        let p = origin.offset(xx as i32, yy as i32, zz as i32);
                        if ops.is_solid(region.get_block(p))
                            && can_replace_with_barrier.test(region, p)
                        {
                            region.set_block(p, barrier_state, 2);
                            region.mark_above_for_post_processing(p);
                        }
                    }
                }
            }
        }
    }

    // Surface freeze
    if ops.is_water_fluid(fluid_state) {
        for xx in 0..16usize {
            for zz in 0..16usize {
                let p = origin.offset(xx as i32, 4, zz as i32);
                if biome_should_freeze(ctx, region, p)
                    && can_replace_with_air_or_fluid.test(region, p)
                {
                    region.set_block(p, ops.sid_ice, 2);
                }
            }
        }
    }

    true
}

/// §4.52 `minecraft:huge_brown_mushroom` (`is_red = false`) & §4.53 `minecraft:huge_red_mushroom` (`is_red = true`).
pub fn place_huge_mushroom(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    cfg: &HugeMushroomConfig,
    is_red: bool,
) -> bool {
    let mut tree_height = rng.next_int_bounded(3) + 4;
    if rng.next_int_bounded(12) == 0 {
        tree_height *= 2;
    }

    if origin.y < region.min_y() + 1 || origin.y + tree_height + 1 > region.max_y() {
        return false;
    }
    if !cfg.can_place_on.test(region, origin.below()) {
        return false;
    }

    let ops = region.ops_arc().clone();
    // Validation
    for dy in 0..=tree_height {
        // Quirk: HugeRedMushroom passes treeHeight=-1 so radius is ALWAYS 0 during validation!
        let r = if is_red || dy <= 3 {
            0
        } else {
            cfg.foliage_radius
        };
        for dx in -r..=r {
            for dz in -r..=r {
                let s = region.get_block(origin.offset(dx, dy, dz));
                if !ops.is_air(s) && !ops.tag_leaves.contains(ops.block_id(s)) {
                    return false;
                }
            }
        }
    }

    let place_mushroom_block = |region: &mut GenerationRegion, pos: BlockPos, st: u32| {
        let cur = region.get_block(pos);
        if ops.is_air(cur) || ops.tag_replaceable_by_mushrooms.contains(ops.block_id(cur)) {
            region.set_block(pos, st, 3);
        }
    };

    // Cap BEFORE trunk; providers are queried at `origin`!
    if !is_red {
        let r = cfg.foliage_radius;
        for dx in -r..=r {
            for dz in -r..=r {
                if dx.abs() == r && dz.abs() == r {
                    continue;
                }
                let mut st = cfg.cap_provider.get_state(region, rng, origin);
                if ops.has_property(st, "west")
                    && ops.has_property(st, "east")
                    && ops.has_property(st, "north")
                    && ops.has_property(st, "south")
                {
                    let west = dx == -r || (dz.abs() == r && dx == 1 - r);
                    let east = dx == r || (dz.abs() == r && dx == r - 1);
                    let north = dz == -r || (dx.abs() == r && dz == 1 - r);
                    let south = dz == r || (dx.abs() == r && dz == r - 1);
                    st = ops.try_set_property(st, "west", if west { "true" } else { "false" });
                    st = ops.try_set_property(st, "east", if east { "true" } else { "false" });
                    st = ops.try_set_property(st, "north", if north { "true" } else { "false" });
                    st = ops.try_set_property(st, "south", if south { "true" } else { "false" });
                }
                place_mushroom_block(region, origin.offset(dx, tree_height, dz), st);
            }
        }
    } else {
        let center = cfg.foliage_radius - 2;
        for dy in (tree_height - 3)..=tree_height {
            let r = if dy < tree_height {
                cfg.foliage_radius
            } else {
                cfg.foliage_radius - 1
            };
            for dx in -r..=r {
                for dz in -r..=r {
                    let include = dy >= tree_height || ((dx.abs() == r) != (dz.abs() == r));
                    if !include {
                        continue;
                    }
                    let mut st = cfg.cap_provider.get_state(region, rng, origin);
                    if ops.has_property(st, "west")
                        && ops.has_property(st, "east")
                        && ops.has_property(st, "north")
                        && ops.has_property(st, "south")
                        && ops.has_property(st, "up")
                    {
                        st = ops.try_set_property(
                            st,
                            "up",
                            if dy >= tree_height - 1 {
                                "true"
                            } else {
                                "false"
                            },
                        );
                        st = ops.try_set_property(
                            st,
                            "west",
                            if dx < -center { "true" } else { "false" },
                        );
                        st = ops.try_set_property(
                            st,
                            "east",
                            if dx > center { "true" } else { "false" },
                        );
                        st = ops.try_set_property(
                            st,
                            "north",
                            if dz < -center { "true" } else { "false" },
                        );
                        st = ops.try_set_property(
                            st,
                            "south",
                            if dz > center { "true" } else { "false" },
                        );
                    }
                    place_mushroom_block(region, origin.offset(dx, dy, dz), st);
                }
            }
        }
    }

    // Trunk
    for dy in 0..tree_height {
        let st = cfg.stem_provider.get_state(region, rng, origin);
        place_mushroom_block(region, origin.above_n(dy), st);
    }

    true
}

/// §4.54 `minecraft:huge_fungus`.
#[allow(clippy::too_many_arguments)]
pub fn place_huge_fungus(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    valid_base_block_id: u32,
    stem_state: u32,
    hat_state: u32,
    decor_state: u32,
    replaceable_blocks: &BlockPredicate,
    planted: bool,
) -> bool {
    let ops = region.ops_arc().clone();
    if ops.block_id(region.get_block(origin.below())) != valid_base_block_id {
        return false;
    }
    let mut total_height = mth_next_int(rng, 4, 13);
    if rng.next_int_bounded(12) == 0 {
        total_height *= 2;
    }
    if !planted && origin.y + total_height + 1 >= region.gen_depth() {
        return false;
    }
    let is_huge = !planted && rng.next_float() < 0.06_f32;
    region.set_block(origin, ops.sid_air, 260);

    let is_replaceable = |region: &GenerationRegion, p: BlockPos, check_plants: bool| -> bool {
        ops.is_replaceable(region.get_block(p))
            || (check_plants && replaceable_blocks.test(region, p))
    };

    // placeStem
    let stem_radius: i32 = if is_huge { 1 } else { 0 };
    for dx in -stem_radius..=stem_radius {
        for dz in -stem_radius..=stem_radius {
            let corner = is_huge && dx.abs() == stem_radius && dz.abs() == stem_radius;
            for dy in 0..total_height {
                let pos = origin.offset(dx, dy, dz);
                if is_replaceable(region, pos, true) {
                    if planted {
                        region.set_block(pos, stem_state, 3);
                    } else if corner {
                        if rng.next_float() < 0.1_f32 {
                            region.set_block(pos, stem_state, 3);
                        }
                    } else {
                        region.set_block(pos, stem_state, 3);
                    }
                }
            }
        }
    }

    // placeHat
    let hat_bid = ops.block_id(hat_state);
    let place_vines = hat_bid == ops.bid_nether_wart_block;
    let hat_height = (rng.next_int_bounded(1 + total_height / 3) + 5).min(total_height);
    let hat_start_y = total_height - hat_height;

    for dy in hat_start_y..=total_height {
        let mut radius: i32 = if dy < total_height - rng.next_int_bounded(3) {
            2
        } else {
            1
        };
        if hat_height > 8 && dy < hat_start_y + 4 {
            radius = 3;
        }
        if is_huge {
            radius += 1;
        }
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                let is_edge_x = dx.abs() == radius;
                let is_edge_z = dz.abs() == radius;
                let inside = !is_edge_x && !is_edge_z && dy != total_height;
                let corner = is_edge_x && is_edge_z;
                let is_hat_bottom = dy < hat_start_y + 3;
                let pos = origin.offset(dx, dy, dz);
                if is_replaceable(region, pos, false) {
                    if is_hat_bottom {
                        if !inside {
                            if ops.block_id(region.get_block(pos.below())) == hat_bid {
                                region.set_block(pos, hat_state, 3);
                            } else if f64::from(rng.next_float()) < 0.15 {
                                region.set_block(pos, hat_state, 3);
                                if place_vines && rng.next_int_bounded(11) == 0 {
                                    try_place_weeping_vines(region, rng, pos);
                                }
                            }
                        }
                    } else {
                        let (decor_prob, hat_prob, vines_prob) = if inside {
                            (
                                0.1_f32,
                                0.2_f32,
                                if place_vines { 0.1_f32 } else { 0.0_f32 },
                            )
                        } else if corner {
                            (
                                0.01_f32,
                                0.7_f32,
                                if place_vines { 0.083_f32 } else { 0.0_f32 },
                            )
                        } else {
                            (
                                5.0e-4_f32,
                                0.98_f32,
                                if place_vines { 0.07_f32 } else { 0.0_f32 },
                            )
                        };
                        if rng.next_float() < decor_prob {
                            region.set_block(pos, decor_state, 3);
                        } else if rng.next_float() < hat_prob {
                            region.set_block(pos, hat_state, 3);
                            // Quirk: ALWAYS draws nextFloat() even when vines_prob == 0.0F!
                            if rng.next_float() < vines_prob {
                                try_place_weeping_vines(region, rng, pos);
                            }
                        }
                    }
                }
            }
        }
    }

    true
}

fn try_place_weeping_vines(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    hat_pos: BlockPos,
) {
    let mut p = hat_pos.below();
    if !region.is_empty_block(p) {
        return;
    }
    let mut goal_h = mth_next_int(rng, 1, 5);
    if rng.next_int_bounded(7) == 0 {
        goal_h *= 2;
    }
    let ops = region.ops_arc().clone();
    let vine_def = ops.blocks.blocks()[ops.bid_weeping_vines as usize].default_state;
    for h in 0..=goal_h {
        if !region.is_empty_block(p) {
            break;
        }
        if h == goal_h || !region.is_empty_block(p.below()) {
            let age = mth_next_int(rng, 23, 25).to_string();
            let st = ops.try_set_property(vine_def, "age", &age);
            region.set_block(p, st, 2);
            break;
        } else {
            region.set_block(p, ops.sid_weeping_vines_plant, 2);
        }
        p = p.below();
    }
}

/// §4.55 `minecraft:vegetation_patch` (`waterlogged = false`) and §4.56 `minecraft:waterlogged_vegetation_patch` (`waterlogged = true`).
pub fn place_vegetation_patch(
    ctx: &FeatureContext<'_>,
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    cfg: &VegetationPatchConfig,
    waterlogged: bool,
) -> bool {
    let x_radius = cfg.xz_radius.sample(rng) + 1;
    let z_radius = cfg.xz_radius.sample(rng) + 1;
    let inwards = match cfg.surface {
        CaveSurface::Floor => Direction::Down,
        CaveSurface::Ceiling => Direction::Up,
    };
    let outwards = inwards.opposite();
    let ops = region.ops_arc().clone();

    let mut surface = JavaBlockPosSet::new();
    for dx in -x_radius..=x_radius {
        let on_x_edge = dx.abs() == x_radius;
        for dz in -z_radius..=z_radius {
            let on_z_edge = dz.abs() == z_radius;
            if on_x_edge && on_z_edge {
                continue;
            }
            if (on_x_edge || on_z_edge)
                && (cfg.extra_edge_column_chance == 0.0_f32
                    || rng.next_float() > cfg.extra_edge_column_chance)
            {
                continue;
            }
            let mut pos = origin.offset(dx, 0, dz);
            for _ in 0..cfg.vertical_range {
                if !ops.is_air(region.get_block(pos)) {
                    break;
                }
                pos = pos.relative(inwards);
            }
            for _ in 0..cfg.vertical_range {
                if ops.is_air(region.get_block(pos)) {
                    break;
                }
                pos = pos.relative(outwards);
            }
            let mut below_pos = pos.relative(inwards);
            if region.is_empty_block(pos)
                && ops.is_face_sturdy(region.get_block(below_pos), outwards)
            {
                let d = cfg.depth.sample(rng)
                    + if cfg.extra_bottom_block_chance > 0.0_f32
                        && rng.next_float() < cfg.extra_bottom_block_chance
                    {
                        1
                    } else {
                        0
                    };
                let ground_pos = below_pos;
                let mut ground_placed = true;
                for i in 0..d {
                    let st_to_place = cfg.ground_state.get_state(region, rng, below_pos);
                    let cur = region.get_block(below_pos);
                    if ops.block_id(st_to_place) != ops.block_id(cur) {
                        if !cfg.replaceable.contains(ops.block_id(cur)) {
                            ground_placed = i != 0;
                            break;
                        }
                        region.set_block(below_pos, st_to_place, 2);
                        below_pos = below_pos.relative(inwards);
                    }
                }
                if ground_placed {
                    surface.insert(ground_pos);
                }
            }
        }
    }

    let final_surface = if waterlogged {
        let mut water_surface = JavaBlockPosSet::new();
        for sp in surface.iter() {
            let mut exposed = false;
            for d in [
                Direction::North,
                Direction::East,
                Direction::South,
                Direction::West,
                Direction::Down,
            ] {
                let adj = sp.relative(d);
                if !ops.is_face_sturdy(region.get_block(adj), d.opposite()) {
                    exposed = true;
                    break;
                }
            }
            if !exposed {
                water_surface.insert(sp);
            }
        }
        for sp in water_surface.iter() {
            region.set_block(sp, ops.sid_water, 2);
        }
        water_surface
    } else {
        surface
    };

    for surface_pos in final_surface.iter() {
        if cfg.vegetation_chance > 0.0_f32 && rng.next_float() < cfg.vegetation_chance {
            if waterlogged {
                let target = surface_pos.below().relative(outwards);
                if ctx.place_sub_feature(&cfg.vegetation_feature, region, rng, target) {
                    let placed = region.get_block(surface_pos);
                    if ops.get_property(placed, "waterlogged") == Some("false") {
                        let wl = ops.copy_waterlogged(placed, true);
                        region.set_block(surface_pos, wl, 2);
                    }
                }
            } else {
                ctx.place_sub_feature(
                    &cfg.vegetation_feature,
                    region,
                    rng,
                    surface_pos.relative(outwards),
                );
            }
        }
    }

    !final_surface.is_empty()
}

/// §4.57 `minecraft:void_start_platform`.
pub fn place_void_start_platform(region: &mut GenerationRegion, origin: BlockPos) -> bool {
    let cx = origin.chunk_x();
    let cz = origin.chunk_z();
    if cx.abs().max(cz.abs()) > 1 {
        return true;
    }
    let ops = region.ops_arc().clone();
    let min_x = cx << 4;
    let min_z = cz << 4;
    let y = origin.y + 3;
    for z in min_z..=(min_z + 15) {
        for x in min_x..=(min_x + 15) {
            if (8 - x).abs().max((8 - z).abs()) <= 16 {
                let st = if x == 8 && z == 8 {
                    ops.sid_cobblestone
                } else {
                    ops.sid_stone
                };
                region.set_block(BlockPos::new(x, y, z), st, 2);
            }
        }
    }
    true
}

/// §4.58 `minecraft:multiface_growth`.
#[allow(clippy::too_many_arguments)]
pub fn place_multiface_growth(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    block_id: u32,
    search_range: i32,
    can_place_on_floor: bool,
    can_place_on_ceiling: bool,
    can_place_on_wall: bool,
    chance_of_spreading: f32,
    can_be_placed_on: &BlockBitSet,
) -> bool {
    let ops = region.ops_arc().clone();
    let cur = region.get_block(origin);
    if !ops.is_air(cur) && !ops.is_water(cur) {
        return false;
    }

    let mut valid_dirs = Vec::with_capacity(6);
    if can_place_on_ceiling {
        valid_dirs.push(Direction::Up);
    }
    if can_place_on_floor {
        valid_dirs.push(Direction::Down);
    }
    if can_place_on_wall {
        valid_dirs.extend_from_slice(&Direction::HORIZONTAL);
    }

    let mut dirs = valid_dirs.clone();
    util_shuffle(&mut dirs, rng);
    if place_growth_if_possible(
        region,
        rng,
        origin,
        cur,
        block_id,
        chance_of_spreading,
        can_be_placed_on,
        &dirs,
    ) {
        return true;
    }

    for &search_dir in &dirs {
        let opp = search_dir.opposite();
        let mut placement_dirs: Vec<Direction> =
            valid_dirs.iter().copied().filter(|&d| d != opp).collect();
        util_shuffle(&mut placement_dirs, rng);
        for _ in 0..search_range {
            // VANILLA BUG REPRODUCED FOR PARITY: resets pos to origin.relative(search_dir) every iteration!
            let pos = origin.relative(search_dir);
            let s = region.get_block(pos);
            if !ops.is_air(s) && !ops.is_water(s) && ops.block_id(s) != block_id {
                break;
            }
            if place_growth_if_possible(
                region,
                rng,
                pos,
                s,
                block_id,
                chance_of_spreading,
                can_be_placed_on,
                &placement_dirs,
            ) {
                return true;
            }
        }
    }

    false
}

#[allow(clippy::too_many_arguments)]
fn place_growth_if_possible(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    pos: BlockPos,
    old_state: u32,
    block_id: u32,
    chance_of_spreading: f32,
    can_be_placed_on: &BlockBitSet,
    placement_dirs: &[Direction],
) -> bool {
    let ops = region.ops_arc().clone();
    for &dir in placement_dirs {
        if can_be_placed_on.contains(ops.block_id(region.get_block(pos.relative(dir)))) {
            let Some(new_state) =
                ops.multiface_state_for_placement(block_id, old_state, region, pos, dir)
            else {
                return false;
            };
            region.set_block(pos, new_state, 3);
            region.mark_pos_for_post_processing(pos);
            if rng.next_float() < chance_of_spreading {
                ops.multiface_spread_random_dir(block_id, new_state, region, pos, dir, rng, true);
            }
            return true;
        }
    }
    false
}
