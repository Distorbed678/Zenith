//! Group E: Plants, corals, Nether formations, underwater magma, and sculk
//! (`features.md` §4.34–§4.41).
// `!(a > b)` on floats is deliberate: it keeps NaN semantics identical to the game.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::sync::LazyLock;

use zenith_blocks::Direction;
use zenith_math::RandomSource;
use zenith_math::helpers::rounding::floor_f32;

use crate::features::{FeatureContext, SteppedColumnClusterConfig};
use crate::placement::PlacedFeature;
use crate::providers::{Column, IntProvider};
use crate::region::GenerationRegion;
use crate::rng::{
    BlockPos, WorldgenRandom, between_closed, direction_all_shuffled, direction_horizontal_random,
    direction_horizontal_shuffled, next_random_between_closed, util_shuffle,
    within_box_by_manhattan_distance,
};

/// §4.34 `minecraft:chorus_plant`.
pub fn place_chorus_plant(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
) -> bool {
    let ops = region.ops_arc().clone();
    if !ops.is_air(region.get_block(origin))
        || !ops
            .tag_supports_chorus_plant
            .contains(ops.block_id(region.get_block(origin.below())))
    {
        return false;
    }
    ops.generate_chorus_plant(region, origin, rng, 8);
    true
}

/// §4.35 `minecraft:vines`.
pub fn place_vines(region: &mut GenerationRegion, origin: BlockPos) -> bool {
    let ops = region.ops_arc().clone();
    if !ops.is_air(region.get_block(origin)) {
        return false;
    }
    for dir in Direction::ALL {
        if dir == Direction::Down {
            continue;
        }
        if ops.is_acceptable_vine_neighbour(region.get_block(origin.relative(dir)), dir) {
            let vine_def = ops.blocks.blocks()[ops.bid_vine as usize].default_state;
            let state = ops.try_set_property(vine_def, dir.name(), "true");
            region.set_block(origin, state, 2);
            return true;
        }
    }
    false
}

/// §4.36 `minecraft:coral_claw`.
pub fn place_coral_claw(
    ctx: &FeatureContext<'_>,
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    feature: &PlacedFeature,
) -> bool {
    if !ctx.place_sub_feature(feature, region, rng, origin) {
        return false;
    }
    let claw_dir = direction_horizontal_random(rng);
    let n_branches = (rng.next_int_bounded(2) + 2) as usize;
    let mut possible_dirs = [claw_dir, claw_dir.clockwise(), claw_dir.counter_clockwise()];
    util_shuffle(&mut possible_dirs, rng);

    for &branch_dir in &possible_dirs[..n_branches] {
        let mut mut_pos = origin;
        let sideway_length = rng.next_int_bounded(2) + 1;
        mut_pos = mut_pos.relative(branch_dir);
        let (segment_dir, inway_length) = if branch_dir == claw_dir {
            (claw_dir, rng.next_int_bounded(3) + 2)
        } else {
            mut_pos = mut_pos.above();
            let sd = if rng.next_int_bounded(2) == 0 {
                branch_dir
            } else {
                Direction::Up
            };
            (sd, rng.next_int_bounded(3) + 3)
        };
        for _ in 0..sideway_length {
            if !ctx.place_sub_feature(feature, region, rng, mut_pos) {
                break;
            }
            mut_pos = mut_pos.relative(segment_dir);
        }
        mut_pos = mut_pos.relative(segment_dir.opposite());
        mut_pos = mut_pos.above();
        for _ in 0..inway_length {
            mut_pos = mut_pos.relative(claw_dir);
            if !ctx.place_sub_feature(feature, region, rng, mut_pos) {
                break;
            }
            if rng.next_float() < 0.25_f32 {
                mut_pos = mut_pos.above();
            }
        }
    }
    true
}

/// §4.37 `minecraft:coral_tree`.
pub fn place_coral_tree(
    ctx: &FeatureContext<'_>,
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    feature: &PlacedFeature,
) -> bool {
    let mut mut_pos = origin;
    let trunk_height = rng.next_int_bounded(3) + 1;
    for _ in 0..trunk_height {
        if !ctx.place_sub_feature(feature, region, rng, mut_pos) {
            return true;
        }
        mut_pos = mut_pos.above();
    }
    let trunk_top = mut_pos;
    let n_branches = (rng.next_int_bounded(3) + 2) as usize;
    let dirs = direction_horizontal_shuffled(rng);
    for &branch_dir in &dirs[..n_branches] {
        let mut p = trunk_top.relative(branch_dir);
        let branch_height = rng.next_int_bounded(5) + 2;
        let mut seg_len = 0;
        for j in 0..branch_height {
            if !ctx.place_sub_feature(feature, region, rng, p) {
                break;
            }
            seg_len += 1;
            p = p.above();
            if j == 0 || (seg_len >= 2 && rng.next_float() < 0.25_f32) {
                p = p.relative(branch_dir);
                seg_len = 0;
            }
        }
    }
    true
}

/// §4.38 `minecraft:stepped_column_cluster`.
pub fn place_stepped_column_cluster(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    cfg: &SteppedColumnClusterConfig,
) -> bool {
    if !stepped_can_place_at(region, origin, cfg) {
        return false;
    }
    let column_height = cfg.height.sample(rng);
    let c_reach = column_height.min(cfg.cluster_reach.sample(rng));
    let count = cfg.column_count.sample(rng);
    let mut placed = false;

    for _ in 0..count {
        let pos = next_random_between_closed(
            rng,
            origin.x - c_reach,
            origin.y,
            origin.z - c_reach,
            origin.x + c_reach,
            origin.y,
            origin.z + c_reach,
        );
        let blocks_to_place_y = column_height - pos.dist_manhattan(origin);
        if blocks_to_place_y >= 0 {
            let reach = cfg.column_reach.sample(rng);
            for col_pos in between_closed(
                BlockPos::new(pos.x - reach, pos.y, pos.z - reach),
                BlockPos::new(pos.x + reach, pos.y, pos.z + reach),
            ) {
                let step_limit = col_pos.dist_manhattan(pos);
                let start = if cfg.can_replace.test(region, col_pos) {
                    stepped_find_surface(region, col_pos, step_limit, cfg)
                } else {
                    stepped_find_air(region, col_pos, step_limit, cfg)
                };
                if let Some(mut cursor) = start {
                    let mut blocks_y = blocks_to_place_y - step_limit / 2;
                    while blocks_y >= 0 {
                        if cfg.can_replace.test(region, cursor) {
                            let st = cfg.block.get_state(region, rng, cursor);
                            region.set_block(cursor, st, 3);
                            cursor = cursor.above();
                            placed = true;
                        } else if cfg.continue_through.test(region, cursor) {
                            cursor = cursor.above();
                        } else {
                            break;
                        }
                        blocks_y -= 1;
                    }
                }
            }
        }
    }
    placed
}

#[inline]
fn stepped_can_place_at(
    region: &GenerationRegion,
    pos: BlockPos,
    cfg: &SteppedColumnClusterConfig,
) -> bool {
    if !cfg.can_replace.test(region, pos) {
        return false;
    }
    let below = region.get_block(pos.below());
    let ops = region.ops();
    !ops.is_air(below) && !cfg.cannot_place_on.contains(ops.block_id(below))
}

fn stepped_find_surface(
    region: &GenerationRegion,
    mut cursor: BlockPos,
    mut limit: i32,
    cfg: &SteppedColumnClusterConfig,
) -> Option<BlockPos> {
    while cursor.y > region.min_y() + 1 && limit > 0 {
        limit -= 1;
        if stepped_can_place_at(region, cursor, cfg) {
            return Some(cursor);
        }
        cursor = cursor.below();
    }
    None
}

fn stepped_find_air(
    region: &GenerationRegion,
    mut cursor: BlockPos,
    mut limit: i32,
    cfg: &SteppedColumnClusterConfig,
) -> Option<BlockPos> {
    let ops = region.ops();
    while cursor.y <= region.max_y() && limit > 0 {
        limit -= 1;
        let s = region.get_block(cursor);
        if cfg.cannot_place_on.contains(ops.block_id(s)) {
            return None;
        }
        if ops.is_air(s) {
            return Some(cursor);
        }
        cursor = cursor.above();
    }
    None
}

/// §4.39 `minecraft:delta_feature`.
pub fn place_delta_feature(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    contents: u32,
    rim: u32,
    size: &IntProvider,
    rim_size: &IntProvider,
) -> bool {
    let spawn_rim = rng.next_double() < 0.9;
    let rim_x = if spawn_rim { rim_size.sample(rng) } else { 0 };
    let rim_z = if spawn_rim { rim_size.sample(rng) } else { 0 };
    let has_rim = spawn_rim && rim_x != 0 && rim_z != 0;
    let radius_x = size.sample(rng);
    let radius_z = size.sample(rng);
    let radius_limit = radius_x.max(radius_z);
    let contents_bid = region.ops().block_id(contents);
    let mut any_placed = false;

    within_box_by_manhattan_distance(origin, radius_x, 0, radius_z, |pos| {
        if pos.dist_manhattan(origin) > radius_limit {
            return false;
        }
        if delta_is_clear(region, pos, contents_bid) {
            if has_rim {
                any_placed = true;
                region.set_block(pos, rim, 3);
            }
            let pos_offset = pos.offset(rim_x, 0, rim_z);
            if delta_is_clear(region, pos_offset, contents_bid) {
                any_placed = true;
                region.set_block(pos_offset, contents, 3);
            }
        }
        true
    });
    any_placed
}

fn delta_is_clear(region: &GenerationRegion, pos: BlockPos, contents_bid: u32) -> bool {
    let ops = region.ops();
    let s = region.get_block(pos);
    let bid = ops.block_id(s);
    if bid == contents_bid || ops.delta_cannot_replace.contains(bid) {
        return false;
    }
    for d in Direction::ALL {
        let air = ops.is_air(region.get_block(pos.relative(d)));
        if (d != Direction::Up && air) || (d == Direction::Up && !air) {
            return false;
        }
    }
    true
}

/// §4.40 `minecraft:underwater_magma`.
pub fn place_underwater_magma(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    floor_search_range: i32,
    placement_radius_around_floor: i32,
    placement_probability_per_valid_position: f32,
) -> bool {
    let ops = region.ops_arc().clone();
    let Some(col) = Column::scan(
        region,
        origin,
        floor_search_range,
        |s| ops.is_water(s),
        |s| !ops.is_water(s),
    ) else {
        return false;
    };
    let Some(floor_y) = col.floor() else {
        return false;
    };
    let floor_pos = origin.at_y(floor_y);
    let r = placement_radius_around_floor;
    let mut placed = 0;

    for pos in between_closed(floor_pos.offset(-r, -r, -r), floor_pos.offset(r, r, r)) {
        // RNG drawn FIRST for every cell before validity check!
        if !(rng.next_float() < placement_probability_per_valid_position) {
            continue;
        }
        let s = region.get_block(pos);
        if ops.is_water(s) || ops.is_air(s) {
            continue;
        }
        let is_visible = |p: BlockPos, face: Direction| -> bool {
            let st = region.get_block(p);
            let face_shape = ops.facts.face_occlusion_shape(st, face);
            face_shape.is_empty() || !face_shape.is_face_full(face)
        };
        if is_visible(pos.below(), Direction::Up) {
            continue;
        }
        let mut valid = true;
        for dir in Direction::HORIZONTAL {
            if is_visible(pos.relative(dir), dir.opposite()) {
                valid = false;
                break;
            }
        }
        if valid {
            region.set_block(pos, ops.sid_magma_block, 2);
            placed += 1;
        }
    }
    placed > 0
}

// ---------------------------------------------------------------------------
// §4.41 `minecraft:sculk_patch`
// ---------------------------------------------------------------------------

static NON_CORNER_NEIGHBOURS: LazyLock<Vec<BlockPos>> = LazyLock::new(|| {
    between_closed(BlockPos::new(-1, -1, -1), BlockPos::new(1, 1, 1))
        .filter(|p| (p.x == 0 || p.y == 0 || p.z == 0) && !(p.x == 0 && p.y == 0 && p.z == 0))
        .collect()
});

#[derive(Clone)]
struct ChargeCursor {
    pos: BlockPos,
    charge: i32,
    decay_delay: i32,
    update_delay: i32,
    facings: Option<u8>,
}

/// §4.41 `minecraft:sculk_patch`.
#[allow(clippy::too_many_arguments)]
pub fn place_sculk_patch(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    charge_count: i32,
    amount_per_charge: i32,
    spread_attempts: i32,
    growth_rounds: i32,
    spread_rounds: i32,
) -> bool {
    if !sculk_can_spread_from(region, origin) {
        return false;
    }
    let total_rounds = spread_rounds + growth_rounds;
    let mut cursors: Vec<ChargeCursor> = Vec::new();
    for round in 0..total_rounds {
        for _ in 0..charge_count {
            let mut rem = amount_per_charge;
            while rem > 0 && cursors.len() < 32 {
                let take = rem.min(1000);
                rem -= take;
                cursors.push(ChargeCursor {
                    pos: origin,
                    charge: take,
                    decay_delay: 1,
                    update_delay: 0,
                    facings: None,
                });
            }
        }
        let spread_veins = round < spread_rounds;
        for _ in 0..spread_attempts {
            cursors = sculk_update_cursors(cursors, region, origin, rng, spread_veins);
        }
        cursors.clear();
    }
    true
}

fn sculk_can_spread_from(region: &GenerationRegion, origin: BlockPos) -> bool {
    let ops = region.ops();
    let s = region.get_block(origin);
    if ops.is_sculk_behaviour(s) {
        return true;
    }
    if !(ops.is_air(s) || (ops.is_water(s) && ops.fluid(s).is_source)) {
        return false;
    }
    for dir in Direction::ALL {
        if ops.is_collision_shape_full_block(region.get_block(origin.relative(dir))) {
            return true;
        }
    }
    false
}

fn sculk_update_cursors(
    cursors: Vec<ChargeCursor>,
    region: &mut GenerationRegion,
    origin: BlockPos,
    rng: &mut WorldgenRandom,
    spread_veins: bool,
) -> Vec<ChargeCursor> {
    let ops = region.ops_arc().clone();
    let mut processed = Vec::with_capacity(cursors.len());

    for mut cursor in cursors {
        if cursor.pos.dist_chessboard(origin) > 1024 || cursor.charge <= 0 {
            continue;
        }
        if cursor.update_delay > 0 {
            cursor.update_delay -= 1;
            processed.push(cursor);
            continue;
        }

        let mut cur_state = region.get_block(cursor.pos);
        let mut cur_bid = ops.block_id(cur_state);

        // 1. Vein spread
        if spread_veins {
            let spread_ok = if cur_bid == ops.bid_sculk || cur_bid == ops.bid_sculk_vein {
                ops.sculk_vein_spread_all(cur_state, region, cursor.pos, false, true) > 0
            } else {
                match cursor.facings {
                    None => {
                        ops.sculk_vein_spread_all(cur_state, region, cursor.pos, true, true) > 0
                    }
                    Some(0) => false,
                    Some(mask) => {
                        if ops.is_air(cur_state)
                            || ops.fluid(cur_state).kind == zenith_blocks::FluidKind::Water
                        {
                            ops.sculk_vein_regrow(region, cursor.pos, cur_state, mask)
                        } else {
                            ops.sculk_vein_spread_all(cur_state, region, cursor.pos, false, true)
                                > 0
                        }
                    }
                }
            };
            if spread_ok && cur_bid != ops.bid_sculk {
                cur_state = region.get_block(cursor.pos);
                cur_bid = ops.block_id(cur_state);
            }
        }

        // 2. Use charge
        cursor.charge = if cur_bid == ops.bid_sculk_vein {
            sculk_vein_use_charge(&ops, &cursor, cur_state, region, rng, spread_veins)
        } else if cur_bid == ops.bid_sculk {
            sculk_block_use_charge(&ops, &cursor, region, origin, rng)
        } else if cursor.decay_delay > 0 {
            cursor.charge
        } else {
            0
        };

        // 3. Discharge / movement
        if cursor.charge <= 0 {
            if cur_bid == ops.bid_sculk_vein {
                ops.sculk_vein_on_discharged(region, cursor.pos, cur_state);
            }
            continue;
        }

        if let Some(transfer_pos) = sculk_valid_movement_pos(&ops, region, cursor.pos, rng, origin)
        {
            if cur_bid == ops.bid_sculk_vein {
                ops.sculk_vein_on_discharged(region, cursor.pos, cur_state);
            }
            cursor.pos = transfer_pos;
            cur_state = region.get_block(cursor.pos);
            cur_bid = ops.block_id(cur_state);
        } else {
            if cur_bid == ops.bid_sculk_vein {
                ops.sculk_vein_on_discharged(region, cursor.pos, cur_state);
            }
            continue;
        }

        if ops.is_sculk_behaviour(cur_state) {
            cursor.facings = Some(ops.multiface_available_faces(cur_state));
        }
        cursor.decay_delay = if cur_bid == ops.bid_sculk || cur_bid == ops.bid_sculk_vein {
            1
        } else {
            (cursor.decay_delay - 1).max(0)
        };
        cursor.update_delay = 1;

        if cursor.charge > 0 {
            processed.push(cursor);
        }
    }

    processed
}

fn sculk_vein_use_charge(
    ops: &crate::block_ops::BlockOps,
    cursor: &ChargeCursor,
    state: u32,
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    spread_veins: bool,
) -> i32 {
    if spread_veins {
        for support in direction_all_shuffled(rng) {
            if ops.multiface_has_face(state, support) {
                let support_pos = cursor.pos.relative(support);
                let support_st = region.get_block(support_pos);
                if ops
                    .tag_sculk_replaceable_world_gen
                    .contains(ops.block_id(support_st))
                {
                    region.set_block(support_pos, ops.sid_sculk, 3);
                    ops.sculk_vein_spread_all(ops.sid_sculk, region, support_pos, false, true);
                    for dir in Direction::ALL {
                        if dir == support.opposite() {
                            continue;
                        }
                        let np = support_pos.relative(dir);
                        let ns = region.get_block(np);
                        if ops.block_id(ns) == ops.bid_sculk_vein {
                            ops.sculk_vein_on_discharged(region, np, ns);
                        }
                    }
                    return cursor.charge - 1;
                }
            }
        }
    }
    if rng.next_int_bounded(5) == 0 {
        floor_f32((cursor.charge as f32) * 0.5_f32)
    } else {
        cursor.charge
    }
}

fn sculk_block_use_charge(
    ops: &crate::block_ops::BlockOps,
    cursor: &ChargeCursor,
    region: &mut GenerationRegion,
    origin: BlockPos,
    rng: &mut WorldgenRandom,
) -> i32 {
    let charge = cursor.charge;
    if charge != 0 && rng.next_int_bounded(5) == 0 {
        let close = cursor.pos.closer_than(origin, 1.0);
        if !close && sculk_can_place_growth(ops, region, cursor.pos) {
            if rng.next_int_bounded(50) < charge {
                let mut growth = if rng.next_int_bounded(11) == 0 {
                    ops.sid_sculk_shrieker_summon
                } else {
                    ops.sid_sculk_sensor
                };
                let above = cursor.pos.above();
                if !ops.fluid(region.get_block(above)).is_empty() {
                    growth = ops.copy_waterlogged(growth, true);
                }
                region.set_block(above, growth, 3);
            }
            (charge - 50).max(0)
        } else if rng.next_int_bounded(10) != 0 {
            charge
        } else if close {
            charge - 1
        } else {
            let outer = (cursor.pos.dist_sqr(origin).sqrt() as f32) - 1.0_f32;
            let outer_sq = outer * outer;
            let factor = (outer_sq / 529.0_f32).min(1.0_f32);
            let decay = (((charge as f32) * factor * 0.5_f32) as i32).max(1);
            charge - decay
        }
    } else {
        charge
    }
}

fn sculk_can_place_growth(
    ops: &crate::block_ops::BlockOps,
    region: &GenerationRegion,
    pos: BlockPos,
) -> bool {
    let above = region.get_block(pos.above());
    if !(ops.is_air(above) || (ops.is_water(above) && ops.fluid(above).is_source)) {
        return false;
    }
    let mut inhibitors = 0;
    for p in between_closed(pos.offset(-4, 0, -4), pos.offset(4, 2, 4)) {
        if ops
            .tag_sculk_growth_inhibitors
            .contains(ops.block_id(region.get_block(p)))
        {
            inhibitors += 1;
            if inhibitors > 2 {
                return false;
            }
        }
    }
    true
}

fn sculk_valid_movement_pos(
    ops: &crate::block_ops::BlockOps,
    region: &GenerationRegion,
    pos: BlockPos,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
) -> Option<BlockPos> {
    let mut offsets = NON_CORNER_NEIGHBOURS.clone();
    util_shuffle(&mut offsets, rng);
    let mut best: Option<BlockPos> = None;
    for off in offsets {
        let neighbour = pos.offset(off.x, off.y, off.z);
        let dx = origin.x - neighbour.x;
        let dz = origin.z - neighbour.z;
        if dx * dx + dz * dz > 144 {
            continue;
        }
        let ns = region.get_block(neighbour);
        if !ops.is_sculk_behaviour(ns) || !sculk_movement_unobstructed(ops, region, pos, neighbour)
        {
            continue;
        }
        best = Some(neighbour);
        if ops.sculk_vein_has_substrate_access(region, ns, neighbour) {
            break;
        }
    }
    best
}

fn sculk_movement_unobstructed(
    ops: &crate::block_ops::BlockOps,
    region: &GenerationRegion,
    from: BlockPos,
    to: BlockPos,
) -> bool {
    if from.dist_manhattan(to) != 2 {
        return true;
    }
    let diff = BlockPos::new(to.x - from.x, to.y - from.y, to.z - from.z);
    let mut dirs = [Direction::Down; 2];
    let mut n = 0;
    if diff.x != 0 {
        dirs[n] = if diff.x > 0 {
            Direction::East
        } else {
            Direction::West
        };
        n += 1;
    }
    if diff.y != 0 {
        dirs[n] = if diff.y > 0 {
            Direction::Up
        } else {
            Direction::Down
        };
        n += 1;
    }
    if diff.z != 0 {
        dirs[n] = if diff.z > 0 {
            Direction::South
        } else {
            Direction::North
        };
        n += 1;
    }
    if n < 2 {
        return true;
    }
    for &d in &dirs[..2] {
        let inter = from.relative(d);
        if !ops.is_face_sturdy(region.get_block(inter), d.opposite()) {
            return true;
        }
    }
    false
}
