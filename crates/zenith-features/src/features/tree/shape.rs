//! `BlockState.updateShape` for the blocks that can border a freshly placed tree
//! (used by `StructureTemplate.updateShapeAtEdge`, `features-trees.md` §2.5).
//!
//! Only the observable effects during the `features` step are modelled: state changes (snowy
//! grass, vines, unsupported plants), and scheduled block / fluid ticks.

use zenith_blocks::{Direction, FluidKind};

use super::TreeCache;
use crate::block_ops::BlockOps;
use crate::region::GenerationRegion;
use crate::rng::BlockPos;

/// `LeavesBlock.getOptionalDistanceAt`.
#[inline]
pub(super) fn optional_distance(ops: &BlockOps, cache: &TreeCache, state: u32) -> Option<i32> {
    if cache
        .tag_prevents_nearby_leaf_decay
        .contains(ops.block_id(state))
    {
        return Some(0);
    }
    if !ops.has_property(state, "persistent") {
        return None;
    }
    ops.get_property(state, "distance")?.parse().ok()
}

#[inline]
fn is_leaves(ops: &BlockOps, state: u32) -> bool {
    ops.has_property(state, "persistent") && ops.has_property(state, "distance")
}

/// Classes whose `updateShape` turns the block into air when `canSurvive` fails
/// (`BushBlock` / `VegetationBlock` family, carpets, snow layers).
fn removes_when_unsupported(class: &str) -> bool {
    matches!(
        class,
        "TallGrassBlock"
            | "FlowerBlock"
            | "EyeblossomBlock"
            | "SaplingBlock"
            | "BushBlock"
            | "VegetationBlock"
            | "FireflyBushBlock"
            | "SweetBerryBushBlock"
            | "WitherRoseBlock"
            | "AzaleaBlock"
            | "ShortDryGrassBlock"
            | "TallDryGrassBlock"
            | "DryVegetationBlock"
            | "DeadBushBlock"
            | "MushroomBlock"
            | "FlowerBedBlock"
            | "LeafLitterBlock"
            | "CarpetBlock"
            | "SnowLayerBlock"
            | "SeagrassBlock"
            | "WaterlilyBlock"
            | "LilyPadBlock"
            | "CactusFlowerBlock"
            | "NetherSproutsBlock"
            | "NetherRootsBlock"
    )
}

/// `state.updateShape(level, level, pos, dir, npos, nstate, random)` for the tree boundary.
#[allow(clippy::too_many_arguments)]
pub(super) fn update_shape(
    region: &mut GenerationRegion,
    ops: &BlockOps,
    cache: &TreeCache,
    pos: BlockPos,
    state: u32,
    dir: Direction,
    _npos: BlockPos,
    nstate: u32,
) -> u32 {
    let bid = ops.block_id(state);
    // LeavesBlock
    if is_leaves(ops, state) {
        if ops.get_property(state, "waterlogged") == Some("true") {
            region.schedule_fluid_tick(pos, FluidKind::Water, 5);
        }
        let d = optional_distance(ops, cache, nstate).unwrap_or(7) + 1;
        let cur: i32 = ops
            .get_property(state, "distance")
            .and_then(|s| s.parse().ok())
            .unwrap_or(7);
        if d != 1 || cur != d {
            region.schedule_block_tick(pos, bid, 1);
        }
        return state;
    }

    // LiquidBlock
    if bid == ops.bid_water || bid == ops.bid_lava {
        let f = ops.fluid(state);
        if f.is_source || ops.fluid(nstate).is_source {
            region.schedule_fluid_tick(pos, f.kind, 5);
        }
        return state;
    }

    let class = ops.block_class(state);
    let mut out = state;

    // SnowyDirtBlock (grass block, podzol, mycelium)
    if ops.has_property(state, "snowy") {
        if dir == Direction::Up {
            let snowy = cache.tag_snow.contains(ops.block_id(nstate));
            out = ops.try_set_property(state, "snowy", if snowy { "true" } else { "false" });
        }
        return out;
    }

    if bid == ops.bid_vine {
        if dir == Direction::Down {
            return state;
        }
        let new_state = vine_updated_state(region, ops, pos, state);
        let any = ["up", "north", "east", "south", "west"]
            .iter()
            .any(|p| ops.get_property(new_state, p) == Some("true"));
        return if any { new_state } else { ops.sid_air };
    }

    match class {
        // FenceBlock: horizontal neighbours reconnect (a trunk placed next to a structure's fence).
        "FenceBlock" => {
            if ops.get_property(state, "waterlogged") == Some("true") {
                region.schedule_fluid_tick(pos, FluidKind::Water, 5);
            }
            if matches!(
                dir,
                Direction::North | Direction::East | Direction::South | Direction::West
            ) {
                let connects = fence_connects(ops, state, nstate, dir.opposite());
                return ops.try_set_property(
                    state,
                    dir.name(),
                    if connects { "true" } else { "false" },
                );
            }
            return state;
        }
        // FallingBlock / BrushableBlock: every shape update schedules a tick.
        c if c.contains("FallingBlock")
            || c == "SandBlock"
            || c == "ConcretePowderBlock"
            || c == "BrushableBlock"
            || c == "AnvilBlock" =>
        {
            region.schedule_block_tick(pos, bid, 2);
        }
        "DoublePlantBlock" | "TallFlowerBlock" => {
            let half_lower = ops.get_property(state, "half") == Some("lower");
            let vertical = matches!(dir, Direction::Up | Direction::Down);
            if vertical
                && half_lower == (dir == Direction::Up)
                && (ops.block_id(nstate) != bid
                    || ops.get_property(nstate, "half") == ops.get_property(state, "half"))
            {
                return ops.sid_air;
            }
            if half_lower && dir == Direction::Down && !ops.can_survive(region, pos, state) {
                return ops.sid_air;
            }
            if !ops.can_survive(region, pos, state) {
                return ops.sid_air;
            }
        }
        "MossyCarpetBlock" => {
            if !ops.can_survive(region, pos, state) {
                return ops.sid_air;
            }
            let ns = mossy_carpet_updated_state(region, ops, pos, state);
            let bottom = ops.get_property(ns, "bottom") == Some("true");
            let any_side = Direction::HORIZONTAL
                .iter()
                .any(|d| ops.get_property(ns, d.name()) != Some("none"));
            return if bottom || any_side { ns } else { ops.sid_air };
        }
        "HangingMossBlock" => {
            // Hanging moss stays while its upper neighbour can hold it (collision face counts, so
            // leaves work); it schedules a tick instead of dropping immediately.
            let above = region.get_block(pos.above());
            let stays = ops.block_id(above) == bid
                || ops.can_attach_multiface_to(above, Direction::Up)
                || ops.is_face_sturdy(above, Direction::Down);
            if !stays {
                region.schedule_block_tick(pos, bid, 1);
            }
            if dir == Direction::Down {
                let tip = ops.block_id(nstate) != bid;
                out = ops.try_set_property(state, "tip", if tip { "true" } else { "false" });
            }
            return out;
        }
        _ => {}
    }

    let removed = match class {
        "HangingRootsBlock" | "SporeBlossomBlock" | "MangrovePropaguleBlock" => {
            dir == Direction::Up && !ops.can_survive(region, pos, state)
        }
        "TorchBlock" => dir == Direction::Down && !ops.can_survive(region, pos, state),
        "WallTorchBlock" => {
            let facing = ops
                .get_property(state, "facing")
                .and_then(Direction::from_name);
            facing == Some(dir.opposite()) && !ops.can_survive(region, pos, state)
        }
        c if removes_when_unsupported(c) => !ops.can_survive(region, pos, state),
        _ => false,
    };
    if removed {
        return ops.sid_air;
    }

    if ops.get_property(out, "waterlogged") == Some("true") {
        region.schedule_fluid_tick(pos, FluidKind::Water, 5);
    }
    out
}

/// `FenceBlock.connectsTo(neighbour, neighbour.isFaceSturdy(..., towards), towards)`.
fn fence_connects(ops: &BlockOps, fence: u32, neighbour: u32, towards: Direction) -> bool {
    let sturdy = ops.is_face_sturdy(neighbour, towards);
    let exception = is_leaves(ops, neighbour)
        || matches!(
            ops.block_path(neighbour),
            "barrier" | "carved_pumpkin" | "jack_o_lantern" | "melon" | "pumpkin"
        )
        || ops.block_class(neighbour) == "ShulkerBoxBlock";
    if !exception && sturdy {
        return true;
    }
    let tag = |t: &str, s: u32| {
        zenith_registry::Identifier::parse(t).is_ok_and(|id| ops.is_in_tag(s, &id))
    };
    if tag("minecraft:fences", neighbour)
        && tag("minecraft:wooden_fences", neighbour) == tag("minecraft:wooden_fences", fence)
    {
        return true;
    }
    ops.block_class(neighbour) == "FenceGateBlock"
        && ops
            .get_property(neighbour, "facing")
            .and_then(Direction::from_name)
            .is_some_and(|f| f.axis() == towards.clockwise().axis())
}

/// `VineBlock.getUpdatedState`.
fn vine_updated_state(region: &GenerationRegion, ops: &BlockOps, pos: BlockPos, state: u32) -> u32 {
    let mut s = state;
    let above = pos.above();
    if ops.get_property(s, "up") == Some("true") {
        let above_state = region.get_block(above);
        let ok = ops.is_acceptable_vine_neighbour(above_state, Direction::Up);
        s = ops.try_set_property(s, "up", if ok { "true" } else { "false" });
    }
    let mut above_state: Option<u32> = None;
    for dir in Direction::HORIZONTAL {
        if ops.get_property(s, dir.name()) == Some("true") {
            let neighbor = region.get_block(pos.relative(dir));
            let mut supported = ops.is_acceptable_vine_neighbour(neighbor, dir);
            if !supported {
                let a = *above_state.get_or_insert_with(|| region.get_block(above));
                supported = ops.block_id(a) == ops.bid_vine
                    && ops.get_property(a, dir.name()) == Some("true");
            }
            s = ops.try_set_property(s, dir.name(), if supported { "true" } else { "false" });
        }
    }
    s
}

/// `MossyCarpetBlock.getUpdatedState(state, level, pos, createSides = false)`.
fn mossy_carpet_updated_state(
    region: &GenerationRegion,
    ops: &BlockOps,
    pos: BlockPos,
    mut state: u32,
) -> u32 {
    let bottom = ops.get_property(state, "bottom") == Some("true");
    let create_sides = bottom;
    let bid = ops.block_id(state);
    let mut above: Option<u32> = None;
    let mut below: Option<u32> = None;
    for dir in Direction::HORIZONTAL {
        let prop = dir.name();
        let against = region.get_block(pos.relative(dir));
        let supported = ops.can_attach_multiface_to(against, dir);
        let mut side: &str = if supported {
            if create_sides {
                "low"
            } else {
                ops.get_property(state, prop).unwrap_or("none")
            }
        } else {
            "none"
        };
        if side == "low" {
            let a = *above.get_or_insert_with(|| region.get_block(pos.above()));
            if ops.block_id(a) == bid
                && ops.get_property(a, prop) != Some("none")
                && ops.get_property(a, "bottom") != Some("true")
            {
                side = "tall";
            }
            if !bottom {
                let b = *below.get_or_insert_with(|| region.get_block(pos.below()));
                if ops.block_id(b) == bid && ops.get_property(b, prop) == Some("none") {
                    side = "none";
                }
            }
        }
        state = ops.try_set_property(state, prop, side);
    }
    state
}
