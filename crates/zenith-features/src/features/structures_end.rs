//! Group F & G: Bonus chest, monster room, and End features (`features.md` §4.44–§4.50).
//! (`fossil` §4.42 and `template` §4.43 delegate to [`TemplateFeatureHook`].)

use zenith_blocks::Direction;
use zenith_math::helpers::intrinsics::{java_math_cos, java_math_sin};
use zenith_math::helpers::rounding::{ceil_f32, floor_f32, floor_f64};
use zenith_math::{LegacyRandom, RandomSource};
use zenith_registry::Identifier;
use zenith_worldgen::HeightmapKind;

use crate::features::EndSpike;
use crate::region::{FeatureBlockEntity, FeatureEntity, GenerationRegion};
use crate::rng::{BlockPos, WorldgenRandom, between_closed, util_shuffle};

/// §4.44 `minecraft:bonus_chest`.
pub fn place_bonus_chest(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
) -> bool {
    let cx = origin.chunk_x();
    let cz = origin.chunk_z();
    let min_x = cx << 4;
    let min_z = cz << 4;
    let mut x_poses: Vec<i32> = (min_x..=(min_x + 15)).collect();
    let mut z_poses: Vec<i32> = (min_z..=(min_z + 15)).collect();
    util_shuffle(&mut x_poses, rng);
    util_shuffle(&mut z_poses, rng);

    let ops = region.ops_arc().clone();
    for x in x_poses {
        for &z in &z_poses {
            let chest_pos = region.get_heightmap_pos(
                HeightmapKind::MotionBlockingNoLeaves,
                BlockPos::new(x, 0, z),
            );
            if region.is_empty_block(chest_pos) || region.is_collision_empty(chest_pos) {
                region.set_block(chest_pos, ops.sid_chest, 2);
                if region.has_block_entity_at(chest_pos) {
                    let seed = rng.next_long();
                    region.set_block_entity(
                        chest_pos,
                        FeatureBlockEntity::LootContainer {
                            loot_table: Identifier::with_default_namespace(
                                "chests/spawn_bonus_chest",
                            )
                            .expect("valid id"),
                            loot_table_seed: seed,
                        },
                    );
                }
                for dir in Direction::HORIZONTAL {
                    let tp = chest_pos.relative(dir);
                    if ops.can_survive(region, tp, ops.sid_torch) {
                        region.set_block(tp, ops.sid_torch, 2);
                    }
                }
                return true;
            }
        }
    }
    false
}

/// §4.45 `minecraft:monster_room`.
pub fn place_monster_room(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
) -> bool {
    let xr = rng.next_int_bounded(2) + 2;
    let min_x = -xr - 1;
    let max_x = xr + 1;
    let zr = rng.next_int_bounded(2) + 2;
    let min_z = -zr - 1;
    let max_z = zr + 1;

    let ops = region.ops_arc().clone();
    let mut hole_count = 0;
    for dx in min_x..=max_x {
        for dy in -1..=4 {
            for dz in min_z..=max_z {
                let p = origin.offset(dx, dy, dz);
                let solid = ops.is_solid(region.get_block(p));
                if dy == -1 && !solid {
                    return false;
                }
                if dy == 4 && !solid {
                    return false;
                }
                if (dx == min_x || dx == max_x || dz == min_z || dz == max_z)
                    && dy == 0
                    && region.is_empty_block(p)
                    && region.is_empty_block(p.above())
                {
                    hole_count += 1;
                }
            }
        }
    }
    if !(1..=5).contains(&hole_count) {
        return false;
    }

    let can_replace = |s: u32| !ops.tag_features_cannot_replace.contains(ops.block_id(s));

    // Carve room & place cobble
    for dx in min_x..=max_x {
        for dy in (-1..=3).rev() {
            for dz in min_z..=max_z {
                let p = origin.offset(dx, dy, dz);
                let s = region.get_block(p);
                let on_boundary =
                    dx == min_x || dx == max_x || dz == min_z || dz == max_z || dy == -1 || dy == 4;
                if on_boundary {
                    if p.y >= region.min_y() && !ops.is_solid(region.get_block(p.below())) {
                        region.set_block(p, ops.sid_cave_air, 2);
                    } else if ops.is_solid(s) && ops.block_id(s) != ops.bid_chest {
                        if dy == -1 && rng.next_int_bounded(4) != 0 {
                            region.safe_set_block(p, ops.sid_mossy_cobblestone, can_replace);
                        } else {
                            region.safe_set_block(p, ops.sid_cobblestone, can_replace);
                        }
                    }
                } else if ops.block_id(s) != ops.bid_chest && ops.block_id(s) != ops.bid_spawner {
                    region.safe_set_block(p, ops.sid_cave_air, can_replace);
                }
            }
        }
    }

    // Chests (2 chests, up to 3 attempts each)
    for _ in 0..2 {
        for _ in 0..3 {
            let xc = origin.x + rng.next_int_bounded(xr * 2 + 1) - xr;
            let yc = origin.y;
            let zc = origin.z + rng.next_int_bounded(zr * 2 + 1) - zr;
            let chest_pos = BlockPos::new(xc, yc, zc);
            if region.is_empty_block(chest_pos) {
                let mut wall_count = 0;
                for dir in Direction::HORIZONTAL {
                    if ops.is_solid(region.get_block(chest_pos.relative(dir))) {
                        wall_count += 1;
                    }
                }
                if wall_count == 1 {
                    let oriented = reorient_chest(region, chest_pos, ops.sid_chest);
                    region.safe_set_block(chest_pos, oriented, can_replace);
                    if region.has_block_entity_at(chest_pos) {
                        let seed = rng.next_long();
                        region.set_block_entity(
                            chest_pos,
                            FeatureBlockEntity::LootContainer {
                                loot_table: Identifier::with_default_namespace(
                                    "chests/simple_dungeon",
                                )
                                .expect("valid id"),
                                loot_table_seed: seed,
                            },
                        );
                    }
                    break;
                }
            }
        }
    }

    // Spawner
    region.safe_set_block(origin, ops.sid_spawner, can_replace);
    if region.has_block_entity_at(origin) {
        const MOBS: [&str; 4] = ["skeleton", "zombie", "zombie", "spider"];
        let mob = MOBS[rng.next_int_bounded(4) as usize];
        region.set_block_entity(
            origin,
            FeatureBlockEntity::Spawner {
                entity_id: Identifier::with_default_namespace(mob).expect("valid id"),
            },
        );
    }

    true
}

fn reorient_chest(region: &GenerationRegion, pos: BlockPos, chest_north: u32) -> u32 {
    let ops = region.ops();
    let mut solid_neighbor: Option<Direction> = None;
    for dir in Direction::HORIZONTAL {
        let s = region.get_block(pos.relative(dir));
        if ops.block_id(s) == ops.bid_chest {
            return chest_north;
        }
        if ops.is_solid_render(s) {
            if solid_neighbor.is_none() {
                solid_neighbor = Some(dir);
            } else {
                solid_neighbor = None;
                break;
            }
        }
    }
    let facing = if let Some(sn) = solid_neighbor {
        sn.opposite()
    } else {
        let mut lock_dir = Direction::North;
        if ops.is_solid_render(region.get_block(pos.relative(lock_dir))) {
            lock_dir = lock_dir.opposite();
        }
        if ops.is_solid_render(region.get_block(pos.relative(lock_dir))) {
            lock_dir = lock_dir.clockwise();
        }
        if ops.is_solid_render(region.get_block(pos.relative(lock_dir))) {
            lock_dir = lock_dir.opposite();
        }
        lock_dir
    };
    ops.try_set_property(chest_north, "facing", facing.name())
}

/// §4.46 `minecraft:end_gateway`.
pub fn place_end_gateway(
    region: &mut GenerationRegion,
    origin: BlockPos,
    exit: Option<BlockPos>,
    exact: bool,
) -> bool {
    let ops = region.ops_arc().clone();
    for pos in between_closed(origin.offset(-1, -2, -1), origin.offset(1, 2, 1)) {
        let same_x = pos.x == origin.x;
        let same_y = pos.y == origin.y;
        let same_z = pos.z == origin.z;
        let end = (pos.y - origin.y).abs() == 2;
        if same_x && same_y && same_z {
            region.set_block(pos, ops.sid_end_gateway, 3);
            if exit.is_some() && region.has_block_entity_at(pos) {
                region.set_block_entity(pos, FeatureBlockEntity::EndGateway { exit, exact });
            }
        } else if same_y {
            region.set_block(pos, ops.sid_air, 3);
        } else if (end && same_x && same_z) || ((same_x || same_z) && !end) {
            region.set_block(pos, ops.sid_bedrock, 3);
        } else {
            region.set_block(pos, ops.sid_air, 3);
        }
    }
    true
}

/// §4.47 `minecraft:end_island`.
pub fn place_end_island(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
) -> bool {
    let end_stone = region.ops().sid_end_stone;
    let mut size = (rng.next_int_bounded(3) as f32) + 4.0_f32;
    let mut y = 0;
    while size > 0.5_f32 {
        let r2 = (size + 1.0_f32) * (size + 1.0_f32);
        for x in floor_f32(-size)..=ceil_f32(size) {
            for z in floor_f32(-size)..=ceil_f32(size) {
                if ((x * x + z * z) as f32) <= r2 {
                    region.set_block(origin.offset(x, y, z), end_stone, 3);
                }
            }
        }
        size -= (rng.next_int_bounded(2) as f32) + 0.5_f32;
        y -= 1;
    }
    true
}

/// §4.48 `minecraft:end_platform`.
pub fn place_end_platform(region: &mut GenerationRegion, origin: BlockPos) -> bool {
    let ops = region.ops_arc().clone();
    for dz in -2..=2 {
        for dx in -2..=2 {
            for dy in -1..=2 {
                let pos = origin.offset(dx, dy, dz);
                let (target_bid, target_sid) = if dy == -1 {
                    (ops.bid_obsidian, ops.sid_obsidian)
                } else {
                    (ops.bid_air, ops.sid_air)
                };
                if ops.block_id(region.get_block(pos)) != target_bid {
                    region.set_block(pos, target_sid, 3);
                }
            }
        }
    }
    true
}

/// §4.49 `minecraft:end_podium`.
pub fn place_end_podium(region: &mut GenerationRegion, origin: BlockPos, active: bool) -> bool {
    let ops = region.ops_arc().clone();
    for pos in between_closed(origin.offset(-4, -1, -4), origin.offset(4, 32, 4)) {
        let inside_rim = pos.closer_than(origin, 2.5);
        if inside_rim || pos.closer_than(origin, 3.5) {
            if pos.y < origin.y {
                if inside_rim {
                    region.set_block(pos, ops.sid_bedrock, 3);
                } else if !active || ops.block_id(region.get_block(pos)) != ops.bid_end_stone {
                    region.set_block(pos, ops.sid_end_stone, 3);
                }
            } else if pos.y > origin.y {
                if !active || !ops.is_air(region.get_block(pos)) {
                    region.set_block(pos, ops.sid_air, 3);
                }
            } else if !inside_rim {
                region.set_block(pos, ops.sid_bedrock, 3);
            } else if active {
                region.set_block(pos, ops.sid_end_portal, 3);
            } else {
                region.set_block(pos, ops.sid_air, 3);
            }
        }
    }
    for y in 0..4 {
        region.set_block(origin.above_n(y), ops.sid_bedrock, 3);
    }
    let wall_torch_def = ops.blocks.blocks()[ops.bid_wall_torch as usize].default_state;
    for face in Direction::HORIZONTAL {
        let st = ops.try_set_property(wall_torch_def, "facing", face.name());
        region.set_block(origin.above_n(2).relative(face), st, 3);
    }
    true
}

/// World-seeded 10-pillar End layout (`EndSpikeFeature.getSpikesForLevel`, §4.50).
pub fn get_spikes_for_seed(world_seed: i64) -> Vec<EndSpike> {
    let mut r1 = LegacyRandom::new(world_seed);
    let key = r1.next_long() & 0xFFFF;
    let mut r2 = LegacyRandom::new(key);
    let mut sizes = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
    util_shuffle(&mut sizes, &mut r2);
    let mut out = Vec::with_capacity(10);
    for (i, &size) in sizes.iter().enumerate() {
        let angle = 2.0 * (-std::f64::consts::PI + (std::f64::consts::PI / 10.0) * (i as f64));
        let center_x = floor_f64(42.0 * java_math_cos(angle));
        let center_z = floor_f64(42.0 * java_math_sin(angle));
        let radius = 2 + size / 3;
        let height = 76 + size * 3;
        let guarded = size == 1 || size == 2;
        out.push(EndSpike {
            center_x,
            center_z,
            radius,
            height,
            guarded,
        });
    }
    out
}

/// §4.50 `minecraft:end_spike`.
pub fn place_end_spike(
    region: &mut GenerationRegion,
    rng: &mut WorldgenRandom,
    origin: BlockPos,
    configured_spikes: &[EndSpike],
    crystal_invulnerable: bool,
    crystal_beam_target: Option<BlockPos>,
) -> bool {
    let derived;
    let spikes = if configured_spikes.is_empty() {
        derived = get_spikes_for_seed(region.world_seed());
        &derived
    } else {
        configured_spikes
    };

    let ops = region.ops_arc().clone();
    let origin_cx = origin.chunk_x();
    let origin_cz = origin.chunk_z();
    let iron_bars_def = ops.blocks.blocks()[ops.bid_iron_bars as usize].default_state;

    for spike in spikes {
        if (spike.center_x >> 4) != origin_cx || (spike.center_z >> 4) != origin_cz {
            continue;
        }
        let r = spike.radius;
        let max_d2 = f64::from(r * r + 1);
        for pos in between_closed(
            BlockPos::new(spike.center_x - r, region.min_y(), spike.center_z - r),
            BlockPos::new(spike.center_x + r, spike.height + 10, spike.center_z + r),
        ) {
            let dx = f64::from(pos.x - spike.center_x);
            let dz = f64::from(pos.z - spike.center_z);
            if dx * dx + dz * dz <= max_d2 && pos.y < spike.height {
                region.set_block(pos, ops.sid_obsidian, 3);
            } else if pos.y > 65 {
                region.set_block(pos, ops.sid_air, 3);
            }
        }

        if spike.guarded {
            for dx in -2_i32..=2 {
                for dz in -2_i32..=2 {
                    for dy in 0_i32..=3 {
                        if dx.abs() == 2 || dz.abs() == 2 || dy == 3 {
                            let x_edge = dx == -2 || dx == 2 || dy == 3;
                            let z_edge = dz == -2 || dz == 2 || dy == 3;
                            let mut st = iron_bars_def;
                            st = ops.try_set_property(
                                st,
                                "north",
                                if x_edge && dz != -2 { "true" } else { "false" },
                            );
                            st = ops.try_set_property(
                                st,
                                "south",
                                if x_edge && dz != 2 { "true" } else { "false" },
                            );
                            st = ops.try_set_property(
                                st,
                                "west",
                                if z_edge && dx != -2 { "true" } else { "false" },
                            );
                            st = ops.try_set_property(
                                st,
                                "east",
                                if z_edge && dx != 2 { "true" } else { "false" },
                            );
                            region.set_block(
                                BlockPos::new(
                                    spike.center_x + dx,
                                    spike.height + dy,
                                    spike.center_z + dz,
                                ),
                                st,
                                3,
                            );
                        }
                    }
                }
            }
        }

        let yaw = rng.next_float() * 360.0_f32;
        region.add_fresh_entity(FeatureEntity {
            id: Identifier::with_default_namespace("end_crystal").expect("valid id"),
            pos: [
                f64::from(spike.center_x) + 0.5,
                f64::from(spike.height + 1),
                f64::from(spike.center_z) + 0.5,
            ],
            yaw,
            pitch: 0.0,
            beam_target: crystal_beam_target,
            invulnerable: crystal_invulnerable,
        });

        let crystal_pos = BlockPos::new(spike.center_x, spike.height + 1, spike.center_z);
        region.set_block(crystal_pos.below(), ops.sid_bedrock, 3);
        region.set_block(crystal_pos, ops.sid_fire, 3);
    }

    true
}
