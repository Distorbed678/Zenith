//! Jar-free tests of the structure processor pipeline and template transforms against facts
//! stated in `docs/specs/worldgen/structures-placement-and-jigsaw.md` §4.2–§4.4, using a
//! synthetic block-state table.

use zenith_blocks::BlockFacts;
use zenith_nbt::{Compound, Tag};
use zenith_registry::{BlockDef, BlockStateTable, Identifier, PropertyDef};
use zenith_structures::data::{
    PosRuleTest, ProcessorRule, RawBlockHolderSet, RuleBlockEntityModifier, RuleTest,
};
use zenith_structures::geom::{BlockPos, BoundingBox, BoundingBoxExt, Mirror, Rotation};
use zenith_structures::{
    ProcessorWorldView, StructureBlockInfo, StructurePlaceSettings, StructureProcessor,
    process_block_infos,
};
use zenith_worldgen::{BlockStateSpec, BlockTags, HeightmapKind, IntProvider};

fn id(s: &str) -> Identifier {
    Identifier::with_default_namespace(s).unwrap()
}

fn table() -> BlockStateTable {
    let plain = |s: &str| BlockDef::new(id(s), vec![]).unwrap();
    BlockStateTable::from_blocks(vec![
        plain("air"),
        plain("stone"),
        plain("cobblestone"),
        plain("gold_block"),
        plain("chest"),
        plain("jigsaw"),
        plain("structure_void"),
        BlockDef::new(
            id("oak_log"),
            vec![
                PropertyDef::enumeration("axis", vec!["x".into(), "y".into(), "z".into()]).unwrap(),
            ],
        )
        .unwrap(),
    ])
    .unwrap()
}

struct View {
    blocks: BlockStateTable,
    tags: BlockTags,
    world_block: u32,
    height: i32,
}

impl ProcessorWorldView for View {
    fn world_seed(&self) -> i64 {
        1234
    }
    fn is_live_server_level(&self) -> bool {
        false
    }
    fn get_block_state(&self, _pos: BlockPos) -> u32 {
        self.world_block
    }
    fn get_height(&self, _kind: HeightmapKind, _x: i32, _z: i32) -> i32 {
        self.height
    }
    fn blocks(&self) -> &BlockStateTable {
        &self.blocks
    }
    fn block_tags(&self) -> &BlockTags {
        &self.tags
    }
    fn block_facts(&self) -> Option<&BlockFacts> {
        None
    }
}

fn view() -> View {
    let blocks = table();
    View {
        world_block: blocks.air_default_state().unwrap().global_id,
        blocks,
        tags: BlockTags::empty(),
        height: 10,
    }
}

fn state(v: &View, name: &str) -> u32 {
    v.blocks.block(&id(name)).unwrap().default_state
}

fn infos(v: &View, name: &str, n: i32) -> Vec<StructureBlockInfo> {
    (0..n)
        .map(|i| StructureBlockInfo {
            pos: BlockPos::new(i, 2, 0),
            state_id: state(v, name),
            nbt: None,
        })
        .collect()
}

fn run(
    v: &View,
    procs: Vec<StructureProcessor>,
    list: &[StructureBlockInfo],
) -> Vec<StructureBlockInfo> {
    let mut s = StructurePlaceSettings {
        processors: procs,
        ..StructurePlaceSettings::default()
    };
    process_block_infos(
        v,
        BlockPos::new(100, 50, 100),
        BlockPos::new(100, 50, 100),
        &mut s,
        list,
    )
}

fn spec(name: &str) -> BlockStateSpec {
    BlockStateSpec {
        name: id(name),
        properties: Default::default(),
    }
}

fn stone_to_cobble(modifier: RuleBlockEntityModifier) -> StructureProcessor {
    StructureProcessor::Rule {
        rules: vec![ProcessorRule {
            input_predicate: RuleTest::BlockMatch { block: id("stone") },
            location_predicate: RuleTest::AlwaysTrue,
            position_predicate: PosRuleTest::AlwaysTrue,
            output_state: spec("cobblestone"),
            block_entity_modifier: modifier,
        }],
    }
}

#[test]
fn processors_see_world_positions_and_template_states() {
    let v = view();
    let out = run(&v, vec![StructureProcessor::Nop], &infos(&v, "stone", 3));
    assert_eq!(out.len(), 3);
    // worldPos = transform(pos) + position.
    assert_eq!(out[1].pos, BlockPos::new(101, 52, 100));
    assert_eq!(out[1].state_id, state(&v, "stone"));
}

#[test]
fn block_ignore_drops_listed_blocks_only() {
    let v = view();
    let mut list = infos(&v, "stone", 2);
    list.extend(infos(&v, "gold_block", 2));
    let out = run(
        &v,
        vec![StructureProcessor::BlockIgnore {
            blocks: vec![id("stone")],
        }],
        &list,
    );
    assert_eq!(out.len(), 2);
    assert!(out.iter().all(|i| i.state_id == state(&v, "gold_block")));
}

#[test]
fn block_rot_integrity_extremes() {
    let v = view();
    let list = infos(&v, "stone", 64);
    let keep = run(
        &v,
        vec![StructureProcessor::BlockRot {
            rottable_blocks: None,
            integrity: 1.0,
        }],
        &list,
    );
    assert_eq!(keep.len(), 64);
    let drop = run(
        &v,
        vec![StructureProcessor::BlockRot {
            rottable_blocks: None,
            integrity: 0.0,
        }],
        &list,
    );
    assert!(drop.len() <= 1, "integrity 0 keeps (almost) nothing");
    // Blocks outside `rottable_blocks` are never rolled.
    let kept = run(
        &v,
        vec![StructureProcessor::BlockRot {
            rottable_blocks: Some(RawBlockHolderSet::Ids(vec![id("gold_block")])),
            integrity: 0.0,
        }],
        &list,
    );
    assert_eq!(kept.len(), 64);
}

#[test]
fn rule_processor_replaces_and_appends_loot() {
    let v = view();
    let out = run(
        &v,
        vec![stone_to_cobble(RuleBlockEntityModifier::AppendLoot {
            loot_table: Identifier::parse("minecraft:chests/test").unwrap(),
        })],
        &infos(&v, "stone", 2),
    );
    for o in &out {
        assert_eq!(o.state_id, state(&v, "cobblestone"));
        let nbt = o.nbt.as_ref().expect("append_loot creates nbt");
        assert_eq!(
            nbt.get("LootTable").and_then(Tag::as_str),
            Some("minecraft:chests/test")
        );
        assert!(nbt.get("LootTableSeed").is_some());
    }
    // The seed comes from the per-position legacy random, so it is deterministic and
    // differs between positions.
    let again = run(
        &v,
        vec![stone_to_cobble(RuleBlockEntityModifier::AppendLoot {
            loot_table: Identifier::parse("minecraft:chests/test").unwrap(),
        })],
        &infos(&v, "stone", 2),
    );
    assert_eq!(out, again);
    assert_ne!(
        out[0].nbt.as_ref().unwrap().get("LootTableSeed"),
        out[1].nbt.as_ref().unwrap().get("LootTableSeed")
    );
}

#[test]
fn clear_modifier_yields_empty_non_null_nbt() {
    let v = view();
    let mut list = infos(&v, "stone", 1);
    let mut nbt = Compound::new();
    nbt.insert("x", Tag::Int(1));
    list[0].nbt = Some(nbt);
    let out = run(
        &v,
        vec![stone_to_cobble(RuleBlockEntityModifier::Clear)],
        &list,
    );
    assert_eq!(out[0].nbt, Some(Compound::new()));
}

#[test]
fn gravity_projects_onto_the_heightmap() {
    let v = view();
    let out = run(
        &v,
        vec![StructureProcessor::Gravity {
            heightmap: HeightmapKind::WorldSurfaceWg,
            offset: -1,
        }],
        &infos(&v, "stone", 1),
    );
    // height 10 + offset -1 + template-relative y (2).
    assert_eq!(out[0].pos.y, 10 - 1 + 2);
    assert_eq!((out[0].pos.x, out[0].pos.z), (100, 100));
}

#[test]
fn protected_blocks_drop_over_listed_world_blocks() {
    let mut v = view();
    v.world_block = state(&v, "gold_block");
    let out = run(
        &v,
        vec![StructureProcessor::ProtectedBlocks {
            value: RawBlockHolderSet::Ids(vec![id("gold_block")]),
        }],
        &infos(&v, "stone", 3),
    );
    assert!(out.is_empty());
}

#[test]
fn chunk_bounding_box_skips_blocks_outside() {
    let v = view();
    let mut s = StructurePlaceSettings {
        bounding_box: Some(BoundingBox::from_coords(100, 0, 0, 101, 100, 200)),
        ..StructurePlaceSettings::default()
    };
    let out = process_block_infos(
        &v,
        BlockPos::new(100, 50, 100),
        BlockPos::new(100, 50, 100),
        &mut s,
        &infos(&v, "stone", 4),
    );
    assert_eq!(out.len(), 2);
}

#[test]
fn capped_processor_limits_replacements_across_the_whole_piece() {
    let v = view();
    let capped = StructureProcessor::Capped {
        delegate: Box::new(stone_to_cobble(RuleBlockEntityModifier::Passthrough)),
        limit: IntProvider::Constant(3),
    };
    // A chunk box that only covers half the piece must not change the outcome: `capped`
    // evaluates the entire piece, then the caller clips on placement.
    let mut s = StructurePlaceSettings {
        processors: vec![capped],
        bounding_box: Some(BoundingBox::from_coords(100, 0, 0, 105, 100, 200)),
        ..StructurePlaceSettings::default()
    };
    let list = infos(&v, "stone", 12);
    let out = process_block_infos(
        &v,
        BlockPos::new(100, 50, 100),
        BlockPos::new(100, 50, 100),
        &mut s,
        &list,
    );
    assert_eq!(
        out.len(),
        12,
        "whole piece processed even outside the chunk box"
    );
    let cobble = state(&v, "cobblestone");
    assert_eq!(out.iter().filter(|o| o.state_id == cobble).count(), 3);
}

#[test]
fn jigsaw_replacement_uses_final_state() {
    let v = view();
    let mut nbt = Compound::new();
    nbt.insert("final_state", Tag::from("minecraft:cobblestone"));
    let list = vec![
        StructureBlockInfo {
            pos: BlockPos::ZERO,
            state_id: state(&v, "jigsaw"),
            nbt: Some(nbt.clone()),
        },
        StructureBlockInfo {
            pos: BlockPos::new(1, 0, 0),
            state_id: state(&v, "jigsaw"),
            nbt: {
                let mut n = Compound::new();
                n.insert("final_state", Tag::from("minecraft:structure_void"));
                Some(n)
            },
        },
    ];
    let out = run(&v, vec![StructureProcessor::JigsawReplacement], &list);
    assert_eq!(out.len(), 1, "structure_void final state drops the block");
    assert_eq!(out[0].state_id, state(&v, "cobblestone"));
    assert!(out[0].nbt.is_none());
}

#[test]
fn rotation_and_mirror_change_world_positions_not_template_states() {
    let v = view();
    let list = vec![StructureBlockInfo {
        pos: BlockPos::new(2, 0, 3),
        state_id: state(&v, "oak_log"),
        nbt: None,
    }];
    let mut s = StructurePlaceSettings {
        rotation: Rotation::Clockwise90,
        mirror: Mirror::None,
        ..StructurePlaceSettings::default()
    };
    let out = process_block_infos(&v, BlockPos::ZERO, BlockPos::ZERO, &mut s, &list);
    // CLOCKWISE_90 around (0, 0): (x, z) -> (-z, x).
    assert_eq!(out[0].pos, BlockPos::new(-3, 0, 2));
    // Processors still see the unrotated template state (rotation applies on placement).
    assert_eq!(out[0].state_id, state(&v, "oak_log"));
}

#[test]
fn entity_positions_transform_with_the_documented_offsets() {
    use zenith_structures::geom::transform_vec3;
    let p = [1.5, 2.0, 0.25];
    let pivot = BlockPos::ZERO;
    assert_eq!(transform_vec3(p, Mirror::None, Rotation::None, pivot), p);
    assert_eq!(
        transform_vec3(p, Mirror::None, Rotation::Clockwise90, pivot),
        [1.0 - 0.25, 2.0, 1.5]
    );
    assert_eq!(
        transform_vec3(p, Mirror::None, Rotation::Clockwise180, pivot),
        [1.0 - 1.5, 2.0, 1.0 - 0.25]
    );
    assert_eq!(
        transform_vec3(p, Mirror::None, Rotation::Counterclockwise90, pivot),
        [0.25, 2.0, 1.0 - 1.5]
    );
    assert_eq!(
        transform_vec3(p, Mirror::LeftRight, Rotation::None, pivot),
        [1.5, 2.0, 1.0 - 0.25]
    );
    assert_eq!(
        transform_vec3(p, Mirror::FrontBack, Rotation::None, pivot),
        [1.0 - 1.5, 2.0, 0.25]
    );
}

#[test]
fn template_bounding_box_swaps_axes_on_quarter_turns() {
    let _ = BoundingBox::from_coords(0, 0, 0, 1, 1, 1);
    let pivot = BlockPos::ZERO;
    let c1 = zenith_structures::geom::transform_pos(
        BlockPos::new(4, 2, 6),
        Mirror::None,
        Rotation::Clockwise90,
        pivot,
    );
    // size (5,3,7) -> delta (4,2,6); CLOCKWISE_90 -> (-6, 2, 4): a 7 x 5 footprint.
    assert_eq!(c1, BlockPos::new(-6, 2, 4));
}
