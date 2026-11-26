//! `StructureTemplate.placeInWorld` (`docs/specs/worldgen/structures-placement-and-jigsaw.md`
//! §4.5): processor pipeline, transformed block placement, block entities, waterlogging with the
//! fluid flood fill, entities, and data-marker lookup.

use std::collections::HashSet;

use zenith_blocks::flag;
use zenith_nbt::{Compound, List, Tag};

use crate::block_xform::{StateTransformer, block_path, default_state, with_property};
use crate::data::LiquidSettings;
use crate::geom::{
    BlockPos, BoundingBox, BoundingBoxExt, Direction, Mirror, Rotation, transform_pos,
    transform_vec3,
};
use crate::template::{
    StructureBlockInfo, StructurePlaceSettings, StructureTemplate, process_block_infos_shared,
};
use crate::world::{ProcessorView, StructureWorld, UPDATE_BARRIER_FLAGS, WorldgenRandom};

/// Whether the block of `state` is a vanilla `RandomizableContainer` block entity host
/// (`Barrel`, `Chest`/`TrappedChest`, `Crafter`, `DecoratedPot`, `Dispenser`/`Dropper`, `Hopper`,
/// `ShulkerBox`, and the copper chests; **not** brushable blocks).
pub fn is_randomizable_container(path: &str) -> bool {
    matches!(
        path,
        "barrel"
            | "chest"
            | "trapped_chest"
            | "crafter"
            | "decorated_pot"
            | "dispenser"
            | "dropper"
            | "hopper"
    ) || path == "shulker_box"
        || path.ends_with("_shulker_box")
        || path.ends_with("copper_chest")
}

/// `LiquidBlockContainer` implementors: every block with a `waterlogged` property plus kelp and
/// seagrass (which accept no liquid).
fn is_liquid_container(world: &dyn StructureWorld, state: u32) -> bool {
    let blocks = world.blocks();
    let Some(st) = blocks.state_by_id_exact(state as i32) else {
        return false;
    };
    st.property_value("waterlogged").is_some()
        || matches!(
            st.block_id.path(),
            "kelp" | "kelp_plant" | "seagrass" | "tall_seagrass"
        )
}

/// `LiquidBlockContainer.placeLiquid(level, pos, state, fluid)` for water-logging.
/// Only `SimpleWaterloggedBlock`s (blocks with `waterlogged`) accept water.
fn place_liquid(
    world: &mut dyn StructureWorld,
    pos: BlockPos,
    state: u32,
    fluid_is_water: bool,
) -> bool {
    let blocks = world.blocks();
    let Some(st) = blocks.state_by_id_exact(state as i32) else {
        return false;
    };
    let Some(wl) = st.property_value("waterlogged") else {
        return false;
    };
    if wl == "true" || !fluid_is_water {
        return false;
    }
    // SlabBlock refuses double slabs.
    if st.property_value("type") == Some("double") && st.block_id.path().ends_with("_slab") {
        return false;
    }
    let new_state = with_property(blocks, state, "waterlogged", "true");
    world.set_block_state(pos, new_state, 3);
    world.schedule_fluid_tick(pos, 5);
    true
}

fn fluid_of(world: &dyn StructureWorld, pos: BlockPos) -> zenith_blocks::FluidState {
    world.facts().state(world.get_block_state(pos)).fluid
}

/// `StructureTemplate.filterBlocks(position, settings, block)`: the template blocks of block
/// `name` at their transformed world positions (no processors run), restricted to the settings'
/// bounding box. Block states are rotated, `nbt` is copied.
pub fn filter_blocks(
    world: &dyn StructureWorld,
    template: &StructureTemplate,
    position: BlockPos,
    settings: &mut StructurePlaceSettings,
    name: &str,
) -> Vec<StructureBlockInfo> {
    if template.palettes.is_empty() {
        return Vec::new();
    }
    let idx = settings.select_palette_index(template.palettes.len(), Some(position), None);
    let blocks = world.blocks();
    let mut out = Vec::new();
    for b in &template.palettes[idx].blocks {
        if block_path(blocks, b.state_id) != name {
            continue;
        }
        let pos = transform_pos(
            b.pos,
            settings.mirror,
            settings.rotation,
            settings.rotation_pivot,
        )
        .add(position);
        if let Some(bb) = settings.bounding_box
            && !bb.is_inside(pos)
        {
            continue;
        }
        out.push(StructureBlockInfo {
            pos,
            state_id: crate::block_xform::mirror_rotate_state(
                blocks,
                b.state_id,
                Mirror::None,
                settings.rotation,
            ),
            nbt: b.nbt.clone(),
        });
    }
    out
}

/// `StructureTemplate.placeInWorld(level, position, referencePos, settings, random, updateMode)`.
/// Returns `false` when nothing is placed (no palettes / empty template).
pub fn place_template(
    world: &mut dyn StructureWorld,
    template: &StructureTemplate,
    position: BlockPos,
    reference_pos: BlockPos,
    settings: &mut StructurePlaceSettings,
    random: &mut dyn WorldgenRandom,
    update_mode: u32,
) -> bool {
    if template.palettes.is_empty() {
        return false;
    }
    let pal =
        settings.select_palette_index(template.palettes.len(), Some(position), Some(&mut *random));
    let block_list = &template.palettes[pal].blocks;
    if (block_list.is_empty() && (settings.ignore_entities || template.entities.is_empty()))
        || template.size.x < 1
        || template.size.y < 1
        || template.size.z < 1
    {
        return false;
    }

    let processed = process_block_infos_shared(
        &ProcessorView(&*world),
        position,
        reference_pos,
        settings,
        block_list,
        Some(&mut *random),
    );

    let apply_waterlogging = settings.liquid_settings == LiquidSettings::ApplyWaterlogging;
    let barrier = default_state(world.blocks(), "barrier").unwrap_or(0);
    let mut xform = StateTransformer::new(settings.mirror, settings.rotation);
    let mut to_fill: Vec<BlockPos> = Vec::new();
    let mut locked: HashSet<BlockPos> = HashSet::new();
    let mut any_placed = false;

    for info in &processed {
        let pos = info.pos;
        if let Some(bb) = settings.bounding_box
            && !bb.is_inside(pos)
        {
            continue;
        }
        let previous_fluid = apply_waterlogging.then(|| fluid_of(&*world, pos));
        let state = xform.apply(world.blocks(), info.state_id);
        if info.nbt.is_some() {
            world.set_block_state(pos, barrier, UPDATE_BARRIER_FLAGS);
        }
        if !world.set_block_state(pos, state, update_mode) {
            continue;
        }
        any_placed = true;
        let new_facts = world.facts().state(state);
        let has_be = new_facts.has(flag::HAS_BLOCK_ENTITY);
        let new_fluid_is_source = new_facts.fluid.is_source;
        if let Some(nbt) = &info.nbt
            && has_be
        {
            let mut nbt = nbt.clone();
            if is_randomizable_container(block_path(world.blocks(), state)) {
                nbt.insert("LootTableSeed", Tag::Long(random.next_long()));
            }
            world.set_block_entity(pos, nbt);
        }
        if let Some(prev) = previous_fluid {
            if new_fluid_is_source {
                locked.insert(pos);
            } else if is_liquid_container(&*world, state) {
                place_liquid(world, pos, state, prev.kind.is_water() && !prev.is_empty());
                if !prev.is_source {
                    to_fill.push(pos);
                }
            }
        }
    }

    // Fluid flood fill: neighbours tested in the exact order UP, NORTH, EAST, SOUTH, WEST.
    const FILL_DIRS: [Direction; 5] = [
        Direction::Up,
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
    ];
    let mut filled = true;
    while filled && !to_fill.is_empty() {
        filled = false;
        let mut i = 0;
        while i < to_fill.len() {
            let pos = to_fill[i];
            let mut to_place = fluid_of(&*world, pos);
            for d in FILL_DIRS {
                if to_place.is_source {
                    break;
                }
                let n = pos.relative(d, 1);
                let nf = fluid_of(&*world, n);
                if nf.is_source && !locked.contains(&n) {
                    to_place = nf;
                }
            }
            if to_place.is_source {
                let state = world.get_block_state(pos);
                if is_liquid_container(&*world, state) {
                    place_liquid(world, pos, state, to_place.kind.is_water());
                    filled = true;
                    to_fill.remove(i);
                    continue;
                }
            }
            i += 1;
        }
    }

    let _ = any_placed;

    // Entities.
    if !settings.ignore_entities {
        place_entities(world, template, position, settings);
    }
    true
}

fn wrap_degrees(a: f32) -> f32 {
    let mut r = a % 360.0;
    if r >= 180.0 {
        r -= 360.0;
    }
    if r < -180.0 {
        r += 360.0;
    }
    r
}

fn nbt_f32_list(c: &Compound, key: &str) -> Vec<f32> {
    match c.get(key) {
        Some(Tag::List(l)) => l
            .as_slice()
            .iter()
            .map(|t| match t {
                Tag::Float(f) => *f,
                Tag::Double(d) => *d as f32,
                _ => 0.0,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn is_hanging(id: &str) -> bool {
    matches!(
        id,
        "minecraft:item_frame"
            | "minecraft:glow_item_frame"
            | "minecraft:painting"
            | "minecraft:leash_knot"
    )
}

fn place_entities(
    world: &mut dyn StructureWorld,
    template: &StructureTemplate,
    position: BlockPos,
    settings: &StructurePlaceSettings,
) {
    for e in &template.entities {
        let bp = transform_pos(
            e.block_pos,
            settings.mirror,
            settings.rotation,
            settings.rotation_pivot,
        )
        .add(position);
        if let Some(bb) = settings.bounding_box
            && !bb.is_inside(bp)
        {
            continue;
        }
        let mut nbt = e.nbt.clone();
        let p = transform_vec3(
            e.pos,
            settings.mirror,
            settings.rotation,
            settings.rotation_pivot,
        );
        let pos = [
            p[0] + f64::from(position.x),
            p[1] + f64::from(position.y),
            p[2] + f64::from(position.z),
        ];
        let mut list = List::new();
        for v in pos {
            list.push(Tag::Double(v));
        }
        nbt.insert("Pos", Tag::List(list));
        nbt.remove("UUID");
        if nbt.contains_key("block_pos") {
            nbt.insert("block_pos", Tag::IntArray(vec![bp.x, bp.y, bp.z]));
        }
        let id = nbt.get("id").and_then(Tag::as_str).unwrap_or("").to_owned();
        let rot = nbt_f32_list(&nbt, "Rotation");
        let y_rot_old = rot.first().copied().unwrap_or(0.0);
        let x_rot = rot.get(1).copied().unwrap_or(0.0);
        let angle = wrap_degrees(y_rot_old);
        let rotated = match settings.rotation {
            Rotation::Clockwise180 => angle + 180.0,
            Rotation::Counterclockwise90 => angle + 270.0,
            Rotation::Clockwise90 => angle + 90.0,
            Rotation::None => angle,
        };
        let mirrored = match settings.mirror {
            Mirror::LeftRight => -angle,
            Mirror::FrontBack => 180.0 - angle,
            Mirror::None => angle,
        };
        let y_rot = rotated + mirrored - y_rot_old;
        let mut rl = List::new();
        rl.push(Tag::Float(y_rot));
        rl.push(Tag::Float(x_rot));
        nbt.insert("Rotation", Tag::List(rl));
        if is_hanging(&id)
            && let Some(f) = nbt.get("Facing").and_then(Tag::as_byte)
        {
            // Item frames / paintings store the 3D data value of their facing.
            let dir = match f {
                0 => Direction::Down,
                1 => Direction::Up,
                2 => Direction::North,
                3 => Direction::South,
                4 => Direction::West,
                _ => Direction::East,
            };
            let dir = settings
                .rotation
                .rotate_dir(settings.mirror.mirror_dir(dir));
            let v = match dir {
                Direction::Down => 0,
                Direction::Up => 1,
                Direction::North => 2,
                Direction::South => 3,
                Direction::West => 4,
                Direction::East => 5,
            };
            nbt.insert("Facing", Tag::Byte(v));
        }
        world.add_entity(nbt);
    }
}

/// Convenience: whether `bb` clips `pos` (settings bounding box optional).
#[inline]
pub fn clipped(bb: Option<BoundingBox>, pos: BlockPos) -> bool {
    bb.is_some_and(|b| !b.is_inside(pos))
}
