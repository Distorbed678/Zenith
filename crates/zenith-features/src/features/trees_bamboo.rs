//! Group C: Bamboo, trees (hook), and fallen trees (`features.md` §4.22–§4.24).

use zenith_blocks::Direction;
use zenith_math::RandomSource;
use zenith_worldgen::HeightmapKind;

use crate::features::FeatureContext;
use crate::features::tree::TreeDecorator;
use crate::java_hash::JavaBlockPosSet;
use crate::providers::{BlockStateProvider, IntProvider};
use crate::region::GenerationRegion;
use crate::rng::{BlockPos, WorldgenRandom, direction_axis_name, direction_horizontal_random};

/// §4.22 `minecraft:bamboo`.
pub fn place_bamboo(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    probability: f32,
) -> bool {
    if !region.is_empty_block(origin) {
        return false;
    }
    let ops = region.ops_arc().clone();
    let below_bid = ops.block_id(region.get_block(origin.below()));
    let can_survive = ops.tag_supports_bamboo.contains(below_bid)
        || ops.tag_bamboo_plantable_on.contains(below_bid);
    if can_survive {
        let height = rng.next_int_bounded(12) + 5;
        if rng.next_float() < probability {
            let r = rng.next_int_bounded(4) + 1;
            for xx in (origin.x - r)..=(origin.x + r) {
                for zz in (origin.z - r)..=(origin.z + r) {
                    let dx = xx - origin.x;
                    let dz = zz - origin.z;
                    if dx * dx + dz * dz <= r * r {
                        let y = region.get_height(HeightmapKind::WorldSurface, xx, zz) - 1;
                        let podzol_pos = BlockPos::new(xx, y, zz);
                        if ops
                            .tag_beneath_bamboo_podzol_replaceable
                            .contains(ops.block_id(region.get_block(podzol_pos)))
                        {
                            region.set_block(podzol_pos, ops.sid_podzol, 2);
                        }
                    }
                }
            }
        }
        let mut bamboo_pos = origin;
        for _ in 0..height {
            if !region.is_empty_block(bamboo_pos) {
                break;
            }
            region.set_block(bamboo_pos, ops.sid_bamboo_trunk, 2);
            bamboo_pos = bamboo_pos.above();
        }
        if bamboo_pos.y - origin.y >= 3 {
            region.set_block(bamboo_pos, ops.sid_bamboo_final_large, 2);
            bamboo_pos = bamboo_pos.below();
            region.set_block(bamboo_pos, ops.sid_bamboo_top_large, 2);
            bamboo_pos = bamboo_pos.below();
            region.set_block(bamboo_pos, ops.sid_bamboo_top_small, 2);
        }
    }
    true
}

/// §4.24 `minecraft:fallen_tree`.
#[allow(clippy::too_many_arguments)]
pub fn place_fallen_tree(
    ctx: &FeatureContext<'_>,
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    trunk_provider: &BlockStateProvider,
    log_length: &IntProvider,
    stump_decorators: &[TreeDecorator],
    log_decorators: &[TreeDecorator],
) -> bool {
    // 1. Stump
    let stump_state = trunk_provider.get_state(region, rng, origin);
    region.set_block(origin, stump_state, 3);
    region.mark_above_for_post_processing(origin);
    if !stump_decorators.is_empty() {
        ctx.tree_hook
            .run_tree_decorators(ctx, stump_decorators, region, rng, &[origin]);
    }

    // 2. Fallen log geometry
    let dir = direction_horizontal_random(rng);
    let log_len = log_length.sample(rng) - 2;
    let dist = 2 + rng.next_int_bounded(2);
    let mut log_start = origin.relative_n(dir, dist);

    // 3. Ground snapping (move up 1, then up to 6 checks moving down 1)
    log_start = log_start.above();
    for _ in 0..6 {
        if fallen_may_place_on(region, log_start) {
            break;
        }
        log_start = log_start.below();
    }

    // 4. Validation (`canPlaceEntireFallenLog`)
    let mut gap_in_ground = 0;
    let mut cursor = log_start;
    let mut valid = true;
    for _ in 0..log_len {
        if !region.ops().valid_tree_pos(region.get_block(cursor)) {
            valid = false;
            break;
        }
        if !fallen_is_over_solid_ground(region, cursor) {
            gap_in_ground += 1;
            if gap_in_ground > 2 {
                valid = false;
                break;
            }
        } else {
            gap_in_ground = 0;
        }
        cursor = cursor.relative(dir);
    }

    // 5. Log placement
    if valid {
        let axis = direction_axis_name(dir);
        let mut fallen_log = JavaBlockPosSet::new();
        let mut place_pos = log_start;
        for _ in 0..log_len {
            let raw = trunk_provider.get_state(region, rng, place_pos);
            let rotated = region.ops().try_set_property(raw, "axis", axis);
            region.set_block(place_pos, rotated, 3);
            region.mark_above_for_post_processing(place_pos);
            fallen_log.insert(place_pos);
            place_pos = place_pos.relative(dir);
        }
        if !log_decorators.is_empty() {
            let ordered: Vec<BlockPos> = fallen_log.iter().collect();
            ctx.tree_hook
                .run_tree_decorators(ctx, log_decorators, region, rng, &ordered);
        }
    }

    true
}

#[inline]
fn fallen_is_over_solid_ground(region: &GenerationRegion, pos: BlockPos) -> bool {
    let below = region.get_block(pos.below());
    region.ops().is_face_sturdy(below, Direction::Up)
}

#[inline]
fn fallen_may_place_on(region: &GenerationRegion, pos: BlockPos) -> bool {
    region.ops().valid_tree_pos(region.get_block(pos)) && fallen_is_over_solid_ground(region, pos)
}
