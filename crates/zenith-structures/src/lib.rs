//! Structure placement, jigsaw assembly, legacy structure starts, `.nbt` templates,
//! processors, and [`zenith_worldgen::Beardifier`] handoff (`docs/specs/worldgen/structures-placement-and-jigsaw.md`,
//! `docs/specs/worldgen/structures-legacy-pieces.md`).

pub mod block_xform;
pub mod data;
pub mod generator;
pub mod geom;
pub mod jigsaw;
pub mod legacy;
pub mod piece;
pub mod piece_data;
pub mod pieces;
pub mod place;
pub mod placement;
pub mod template;
pub mod template_place;
pub mod world;

pub use data::{
    DimensionPadding, ExclusionZone, FrequencyReductionMethod, JigsawConfig, LiquidSettings,
    MaxDistance, MineshaftType, OceanRuinTemp, PoolAliasBinding, ProcessorListRef, Projection,
    RandomSpreadType, RuinedPortalSetup, RuleBlockEntityModifier, StructureData, StructureDef,
    StructureKind, StructureLoadError, StructurePlacement, StructurePoolElement,
    StructureProcessor, StructureRef, StructureSelectionEntry, StructureSet, StructureSettings,
    StructureTemplatePool, TemplatePoolRef,
};
pub use generator::{
    StructureGenerator, StructurePiece, StructureStart, adjust_bounding_box, beardifier_from_starts,
};
pub use geom::{
    BlockPos, BoundingBox, BoundingBoxExt, Direction, FrontAndTop, Mirror, Rotation,
    sample_height_provider, set_large_feature_seed, set_large_feature_with_salt,
};
pub use jigsaw::{PoolAliasLookup, PoolElementPiece, StructureJigsawJunction};
pub use legacy::LegacyPiece;
pub use piece_data::{PieceData, TemplateKind, TemplatePiece};
pub use place::{PieceCall, PlacementEnv, place_pool_element, post_process_piece};
pub use placement::{
    StructurePlacementState, compute_dimension_origin, potential_random_spread_chunk,
};
pub use template::{
    JigsawBlockInfo, JointType, ProcessorWorldView, StructureBlockInfo, StructureEntityInfo,
    StructurePlaceSettings, StructureTemplate, TemplateError, TemplateManager, process_block_infos,
    transform_block_state,
};
pub use world::{MemoryWorld, StructureWorld, WorldgenRandom, writable_area};

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use zenith_math::{LegacyRandom, RandomSource};
    use zenith_registry::Identifier;
    use zenith_worldgen::{
        ContextSampler, HeightProvider, TerrainAdjustment, VerticalAnchor, WorldGenerationContext,
    };

    use crate::data::{LiquidSettings, Projection, RandomSpreadType, StructurePoolElement};
    use crate::generator::{
        StructurePiece, StructureStart, adjust_bounding_box, beardifier_from_starts,
    };
    use crate::geom::{
        BlockPos, BoundingBox, BoundingBoxExt, Direction, Mirror, Rotation, sample_height_provider,
        transform_pos,
    };
    use crate::jigsaw::{FreeSpace, PoolElementPiece, PriorityQueue, StructureJigsawJunction};
    use crate::placement::potential_random_spread_chunk;

    #[test]
    fn transform_pos_matches_spec() {
        let p = BlockPos::new(2, 5, 3);
        let pivot = BlockPos::new(1, 0, 2);
        assert_eq!(transform_pos(p, Mirror::None, Rotation::None, pivot), p);
        assert_eq!(
            transform_pos(p, Mirror::None, Rotation::Clockwise90, pivot),
            BlockPos::new(1 + 2 - 3, 5, 2 - 1 + 2)
        );
        assert_eq!(
            transform_pos(p, Mirror::None, Rotation::Clockwise180, pivot),
            BlockPos::new(2 - 2, 5, 4 - 3)
        );
        assert_eq!(
            transform_pos(p, Mirror::None, Rotation::Counterclockwise90, pivot),
            BlockPos::new(1 - 2 + 3, 5, 1 + 2 - 2)
        );
        // Mirror applied first, then rotation.
        assert_eq!(
            transform_pos(p, Mirror::LeftRight, Rotation::None, pivot),
            BlockPos::new(2, 5, -3)
        );
        assert_eq!(
            transform_pos(p, Mirror::FrontBack, Rotation::None, pivot),
            BlockPos::new(-2, 5, 3)
        );
    }

    #[test]
    fn orient_box_covers_all_four_directions() {
        let n = BoundingBox::orient_box(10, 20, 30, -1, 0, 0, 5, 4, 6, Direction::North);
        assert_eq!(
            n,
            BoundingBox {
                min_x: 9,
                min_y: 20,
                max_x: 13,
                max_y: 23,
                min_z: 25,
                max_z: 30,
            }
        );
        let s = BoundingBox::orient_box(10, 20, 30, -1, 0, 0, 5, 4, 6, Direction::South);
        assert_eq!(
            s,
            BoundingBox {
                min_x: 9,
                min_y: 20,
                max_x: 13,
                max_y: 23,
                min_z: 30,
                max_z: 35,
            }
        );
        let w = BoundingBox::orient_box(10, 20, 30, -1, 0, 0, 5, 4, 6, Direction::West);
        assert_eq!(
            w,
            BoundingBox {
                min_x: 5,
                min_y: 20,
                max_x: 10,
                max_y: 23,
                min_z: 29,
                max_z: 33,
            }
        );
        let e = BoundingBox::orient_box(10, 20, 30, -1, 0, 0, 5, 4, 6, Direction::East);
        assert_eq!(
            e,
            BoundingBox {
                min_x: 10,
                min_y: 20,
                max_x: 15,
                max_y: 23,
                min_z: 29,
                max_z: 33,
            }
        );
    }

    #[test]
    fn free_space_containment_and_subtraction() {
        let deflate = |a: zenith_blocks::Aabb, d: f64| {
            zenith_blocks::Aabb::new(
                [a.min[0] + d, a.min[1] + d, a.min[2] + d],
                [a.max[0] - d, a.max[1] - d, a.max[2] - d],
            )
        };
        let outer = zenith_blocks::Aabb::new([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let mut fs = FreeSpace::new(outer);
        let piece_a = zenith_blocks::Aabb::new([2.0, 0.0, 2.0], [6.0, 5.0, 6.0]);
        assert!(fs.fits_deflated(&deflate(piece_a, 0.25)));
        fs.subtract(piece_a);

        // Inside subtracted region -> rejected.
        assert!(!fs.fits_deflated(&deflate(piece_a, 0.25)));
        // Adjacent touching region -> accepted (strict interior doesn't overlap).
        let piece_b = zenith_blocks::Aabb::new([6.0, 0.0, 2.0], [9.0, 5.0, 6.0]);
        assert!(fs.fits_deflated(&deflate(piece_b, 0.25)));
        // Partially outside outer -> rejected.
        let outside = zenith_blocks::Aabb::new([9.0, 0.0, 2.0], [12.0, 5.0, 6.0]);
        assert!(!fs.fits_deflated(&deflate(outside, 0.25)));
    }

    #[test]
    fn priority_queue_pops_highest_then_fifo() {
        let mut pq = PriorityQueue::new();
        pq.add("low1", 0);
        pq.add("high1", 5);
        pq.add("low2", 0);
        pq.add("high2", 5);
        pq.add("neg", -2);
        assert_eq!(pq.pop_highest(), Some("high1"));
        assert_eq!(pq.pop_highest(), Some("high2"));
        assert_eq!(pq.pop_highest(), Some("low1"));
        assert_eq!(pq.pop_highest(), Some("low2"));
        assert_eq!(pq.pop_highest(), Some("neg"));
        assert_eq!(pq.pop_highest(), None);
    }

    #[test]
    fn random_spread_stays_within_cell_and_respects_separation() {
        for gx in -3..=3 {
            for gz in -3..=3 {
                let (cx, cz) = potential_random_spread_chunk(
                    12345,
                    gx * 34,
                    gz * 34,
                    34,
                    8,
                    RandomSpreadType::Linear,
                    10387312,
                );
                assert!(cx >= gx * 34 && cx < gx * 34 + (34 - 8));
                assert!(cz >= gz * 34 && cz < gz * 34 + (34 - 8));
                // Any other chunk in the same cell maps to the same candidate.
                let again = potential_random_spread_chunk(
                    12345,
                    gx * 34 + 33,
                    gz * 34 + 33,
                    34,
                    8,
                    RandomSpreadType::Linear,
                    10387312,
                );
                assert_eq!((cx, cz), again);
            }
        }
    }

    #[test]
    fn height_provider_very_biased_to_bottom_degenerate_no_draw() {
        let hp = HeightProvider::VeryBiasedToBottom {
            min_inclusive: VerticalAnchor::Absolute(10),
            max_inclusive: VerticalAnchor::Absolute(5),
            inner: 1,
        };
        let mut rng_a = LegacyRandom::new(42);
        let mut rng_b = LegacyRandom::new(42);
        let ctx = WorldGenerationContext::new(-64, 384, 63);
        let y = sample_height_provider(&hp, &mut rng_a, ctx);
        assert_eq!(y, 10);
        // Must consume zero draws on degenerate min >= max.
        assert_eq!(rng_a.next_int(), rng_b.next_int());
    }

    #[test]
    fn beardifier_handoff_filters_pieces_and_junctions() {
        let piece_bb = BoundingBox {
            min_x: 0,
            min_y: 64,
            min_z: 0,
            max_x: 10,
            max_y: 72,
            max_z: 10,
        };
        let pool_piece = PoolElementPiece {
            element: Arc::new(StructurePoolElement::Empty),
            position: BlockPos::new(0, 64, 0),
            ground_level_delta: 2,
            rotation: Rotation::None,
            bounding_box: piece_bb,
            liquid_settings: LiquidSettings::ApplyWaterlogging,
            junctions: vec![
                // Strictly inside (-12, 27): included.
                StructureJigsawJunction {
                    source_x: 5,
                    source_ground_y: 66,
                    source_z: 5,
                    delta_y: 0,
                    dest_projection: Projection::TerrainMatching,
                },
                // On boundary x = -12: excluded by strict inequality!
                StructureJigsawJunction {
                    source_x: -12,
                    source_ground_y: 66,
                    source_z: 5,
                    delta_y: 0,
                    dest_projection: Projection::TerrainMatching,
                },
            ],
        };
        let start = StructureStart {
            structure_id: Identifier::parse("minecraft:village_plains").unwrap(),
            chunk_x: 0,
            chunk_z: 0,
            references: 0,
            terrain_adaptation: TerrainAdjustment::BeardThin,
            bounding_box: adjust_bounding_box(piece_bb, TerrainAdjustment::BeardThin),
            pieces: vec![StructurePiece {
                data: crate::piece_data::PieceData::None,
                kind: "minecraft:jigsaw",
                bounding_box: piece_bb,
                orientation: None,
                gen_depth: 0,
                jigsaw: Some(pool_piece),
            }],
        };

        assert_eq!(start.bounding_box, piece_bb.inflated_by(12));
        let b = beardifier_from_starts(0, 0, &[&start]);
        assert!(!b.is_empty());
        // Junction at (5, 66, 5) produces non-zero contribution near it.
        assert!(b.sample_value(5, 65, 5) != 0.0);
    }
}
