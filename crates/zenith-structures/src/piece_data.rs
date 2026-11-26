//! Per-piece data carried from start generation into block placement
//! (`docs/specs/worldgen/structures-legacy-pieces.md`): template references, scattered-feature
//! state, and the room/section parameters of the large legacy structures.

use zenith_registry::Identifier;

use crate::data::{OceanRuinTemp, RuinedPortalPlacement};
use crate::geom::{BlockPos, Mirror, Rotation};

/// Kind-specific data of a [`crate::StructurePiece`] (everything except the jigsaw payload).
#[derive(Debug, Clone, PartialEq, Default)]
pub enum PieceData {
    /// No extra data (e.g. pieces whose placement needs only the bounding box).
    #[default]
    None,
    /// Any `TemplateStructurePiece` subclass.
    Template(Box<TemplatePiece>),
    /// `minecraft:tedp`.
    DesertPyramid(ScatteredState),
    /// `minecraft:tejp`.
    JungleTemple(JungleTempleState),
    /// `minecraft:tesh`.
    SwampHut(SwampHutState),
    /// `minecraft:btp` (no state; bounds only).
    BuriedTreasure,
    /// Mineshaft pieces.
    Mineshaft(crate::pieces::mineshaft::MineshaftPiece),
    /// Stronghold pieces.
    Stronghold(crate::pieces::stronghold::StrongholdPiece),
    /// Nether fortress pieces.
    Fortress(crate::pieces::fortress::FortressPiece),
    /// Ocean monument building (rooms are regenerated from this).
    Monument(crate::pieces::monument::MonumentBuilding),
}

/// Shared `ScatteredFeaturePiece` state.
#[derive(Debug, Clone, PartialEq)]
pub struct ScatteredState {
    /// `heightPosition` (`-1` until the first `postProcess` snaps it).
    pub height_position: i32,
    /// `hasPlacedChest[4]` of the desert pyramid (indexed by 2D data value).
    pub has_placed_chest: [bool; 4],
    /// Ephemeral `potentialSuspiciousSandWorldPositions`.
    pub potential_sand: Vec<BlockPos>,
    /// Ephemeral `randomCollapsedRoofPos`.
    pub collapsed_roof_pos: BlockPos,
}

impl Default for ScatteredState {
    fn default() -> Self {
        Self {
            height_position: -1,
            has_placed_chest: [false; 4],
            potential_sand: Vec::new(),
            collapsed_roof_pos: BlockPos::ZERO,
        }
    }
}

/// Jungle temple state.
#[derive(Debug, Clone, PartialEq)]
pub struct JungleTempleState {
    /// `heightPosition`.
    pub height_position: i32,
    /// `placedMainChest`.
    pub placed_main_chest: bool,
    /// `placedHiddenChest`.
    pub placed_hidden_chest: bool,
    /// `placedTrap1`.
    pub placed_trap1: bool,
    /// `placedTrap2`.
    pub placed_trap2: bool,
}

impl Default for JungleTempleState {
    fn default() -> Self {
        Self {
            height_position: -1,
            placed_main_chest: false,
            placed_hidden_chest: false,
            placed_trap1: false,
            placed_trap2: false,
        }
    }
}

/// Swamp hut state.
#[derive(Debug, Clone, PartialEq)]
pub struct SwampHutState {
    /// `heightPosition`.
    pub height_position: i32,
    /// `spawnedWitch`.
    pub spawned_witch: bool,
    /// `spawnedCat`.
    pub spawned_cat: bool,
}

impl Default for SwampHutState {
    fn default() -> Self {
        Self {
            height_position: -1,
            spawned_witch: false,
            spawned_cat: false,
        }
    }
}

/// Ruined-portal parameters captured at start generation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RuinedPortalPiece {
    /// Vertical placement mode.
    pub placement: RuinedPortalPlacement,
    /// Whether the portal was generated cold (no magma spread, no moss fire...).
    pub cold: bool,
    /// Mossiness for `BlockAgeProcessor`.
    pub mossiness: f32,
    /// Whether the portal sits in an air pocket (keeps template air).
    pub air_pocket: bool,
    /// Overgrown (jungle leaves).
    pub overgrown: bool,
    /// Vines.
    pub vines: bool,
    /// Blackstone replacement.
    pub replace_with_blackstone: bool,
}

/// Which template-piece subclass this is, with its extra parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TemplateKind {
    /// `minecraft:iglu`.
    Igloo,
    /// `minecraft:shipwreck`.
    Shipwreck {
        /// Beached variant.
        is_beached: bool,
    },
    /// `minecraft:orp`.
    OceanRuin {
        /// Block-rot integrity.
        integrity: f32,
        /// Warm / cold archaeology blocks.
        temp: OceanRuinTemp,
        /// Large variant (chest loot).
        is_large: bool,
    },
    /// `minecraft:rupo`.
    RuinedPortal(RuinedPortalPiece),
    /// `minecraft:nefos`.
    NetherFossil,
    /// `minecraft:ecp`.
    EndCity {
        /// `overwrite` flag (`"OW"`).
        overwrite: bool,
    },
    /// `minecraft:wmp`.
    WoodlandMansion,
}

/// A `TemplateStructurePiece`: template id, placement transform and kind.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplatePiece {
    /// Subclass and parameters.
    pub kind: TemplateKind,
    /// Template id (e.g. `minecraft:igloo/top`).
    pub template: Identifier,
    /// `templatePosition`.
    pub position: BlockPos,
    /// Placement rotation.
    pub rotation: Rotation,
    /// Placement mirror.
    pub mirror: Mirror,
    /// Rotation pivot.
    pub pivot: BlockPos,
    /// Shipwreck `heightAdjusted`.
    pub height_adjusted: bool,
}

impl PieceData {
    /// Applies `StructurePiece.move(dx, dy, dz)` to the stored world-space data.
    pub fn shift(&mut self, dx: i32, dy: i32, dz: i32) {
        use crate::geom::BoundingBoxExt;
        match self {
            Self::Template(t) => t.position = t.position.offset(dx, dy, dz),
            Self::Mineshaft(m) => {
                if let crate::pieces::mineshaft::MineshaftKind::Room { entrance_boxes } =
                    &mut m.kind
                {
                    for b in entrance_boxes {
                        *b = b.moved(dx, dy, dz);
                    }
                }
            }
            _ => {}
        }
    }
}
