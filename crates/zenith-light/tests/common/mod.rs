//! Shared hand-built scene for the integration tests.
#![allow(dead_code)]

use std::sync::Arc;

use zenith_blocks::{BlockFactTags, BlockFacts};
use zenith_light::{DenseBlocks, LevelHeight, LightFacts, LightWorld, LitChunk};
use zenith_registry::{BlockDef, BlockStateTable, Identifier, PropertyDef};

pub const MIN_Y: i32 = -64;
pub const HEIGHT: i32 = 384;
pub const LEVEL: LevelHeight = LevelHeight {
    min_y: MIN_Y,
    height: HEIGHT,
};

pub struct Scene {
    pub table: BlockStateTable,
    pub facts: Arc<LightFacts>,
    pub air: u32,
    pub stone: u32,
    pub bedrock: u32,
    pub torch: u32,
    pub glowstone: u32,
    pub glass: u32,
    pub slab_bottom: u32,
    pub slab_top: u32,
    pub slab_double: u32,
    pub leaves: u32,
    pub water: u32,
}

fn id(name: &str) -> Identifier {
    Identifier::parse(&format!("minecraft:{name}")).unwrap()
}

fn en(name: &str, values: &[&str]) -> PropertyDef {
    PropertyDef::enumeration(name, values.iter().map(|v| (*v).to_owned()).collect()).unwrap()
}

pub fn scene() -> Scene {
    let wl = || PropertyDef::boolean("waterlogged").unwrap();
    let defs = vec![
        BlockDef::new(id("air"), vec![]).unwrap(),
        BlockDef::new(id("stone"), vec![]).unwrap(),
        BlockDef::new(id("bedrock"), vec![]).unwrap(),
        BlockDef::new(id("glass"), vec![]).unwrap(),
        BlockDef::new(id("torch"), vec![]).unwrap(),
        BlockDef::new(id("glowstone"), vec![]).unwrap(),
        BlockDef::new(
            id("oak_slab"),
            vec![en("type", &["top", "bottom", "double"]), wl()],
        )
        .unwrap(),
        BlockDef::new(
            id("oak_leaves"),
            vec![
                PropertyDef::integer("distance", 1, 7).unwrap(),
                PropertyDef::boolean("persistent").unwrap(),
                wl(),
            ],
        )
        .unwrap(),
        BlockDef::new(
            id("water"),
            vec![PropertyDef::integer("level", 0, 15).unwrap()],
        )
        .unwrap(),
    ];
    let table = BlockStateTable::from_blocks(defs).unwrap();
    let facts =
        BlockFacts::build_partial(&table, &BlockFactTags::empty(table.blocks().len())).unwrap();
    let st = |n: &str, p: &[(&str, &str)]| table.id_of_exact(&id(n), p).unwrap();
    let lf = LightFacts::from_table(Arc::new(facts), &table).unwrap();
    Scene {
        air: st("air", &[]),
        stone: st("stone", &[]),
        bedrock: st("bedrock", &[]),
        torch: st("torch", &[]),
        glowstone: st("glowstone", &[]),
        glass: st("glass", &[]),
        slab_bottom: st("oak_slab", &[("type", "bottom"), ("waterlogged", "false")]),
        slab_top: st("oak_slab", &[("type", "top"), ("waterlogged", "false")]),
        slab_double: st("oak_slab", &[("type", "double"), ("waterlogged", "false")]),
        leaves: st(
            "oak_leaves",
            &[
                ("distance", "1"),
                ("persistent", "true"),
                ("waterlogged", "false"),
            ],
        ),
        water: st("water", &[("level", "0")]),
        facts: Arc::new(lf),
        table,
    }
}

/// A 3x3 chunk neighbourhood around the origin (centre first); `floor` = highest solid Y (stone); `edit` edits
/// chunk (0,0).
pub fn terrain(
    s: &Scene,
    floor: Option<i32>,
    edit: impl Fn(&mut DenseBlocks, &Scene),
) -> Vec<LitChunk<DenseBlocks>> {
    let mut out = Vec::new();
    for cz in -1..=1 {
        for cx in -1..=1 {
            let mut b = DenseBlocks::new(MIN_Y, HEIGHT, s.air);
            if let Some(f) = floor {
                b.fill((0, 15), (MIN_Y, f), (0, 15), s.stone);
            }
            if (cx, cz) == (0, 0) {
                edit(&mut b, s);
            }
            out.push(LitChunk::new(cx, cz, b));
        }
    }
    out.sort_by_key(|c| (u8::from(c.cx != 0 || c.cz != 0), c.cz, c.cx));
    out
}

pub fn lit(
    s: &Scene,
    floor: Option<i32>,
    edit: impl Fn(&mut DenseBlocks, &Scene),
) -> LightWorld<DenseBlocks> {
    let mut w = LightWorld::new(s.facts.clone(), LEVEL, true);
    w.light_chunks(terrain(s, floor, edit));
    w
}
