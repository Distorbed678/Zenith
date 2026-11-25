//! The per-state fact table (`docs/specs/blocks/state-facts.md`, `shapes.md` §5).

use std::collections::HashMap;

use zenith_math::random::position_hash;
use zenith_registry::{BlockStateTable, Identifier};

use crate::BuildError;
use crate::families::{self, Ctx};
use crate::shape::{Direction, SupportType, VoxelShape};
use crate::table::{
    BlockInfo, ConductorRule, EmissiveRule, LightRule, MapColorCond, OffsetType, PostProcess,
    SoundRule, SuffocationRule, Tables, ViewRule,
};

/// Bit positions of [`StateFacts::flags`] (the ZBF1 flag order).
pub mod flag {
    /// The block's default state.
    pub const IS_DEFAULT_STATE: u32 = 1 << 0;
    /// `isAir`.
    pub const IS_AIR: u32 = 1 << 1;
    /// `canOcclude`.
    pub const CAN_OCCLUDE: u32 = 1 << 2;
    /// `isSolidRender`.
    pub const SOLID_RENDER: u32 = 1 << 3;
    /// `propagatesSkylightDown`.
    pub const PROPAGATES_SKYLIGHT_DOWN: u32 = 1 << 4;
    /// `useShapeForLightOcclusion`.
    pub const USE_SHAPE_FOR_LIGHT_OCCLUSION: u32 = 1 << 5;
    /// `isCollisionShapeFullBlock`.
    pub const COLLISION_SHAPE_FULL_BLOCK: u32 = 1 << 6;
    /// `hasLargeCollisionShape`.
    pub const HAS_LARGE_COLLISION_SHAPE: u32 = 1 << 7;
    /// The block has a dynamic shape.
    pub const HAS_DYNAMIC_SHAPE: u32 = 1 << 8;
    /// `isRandomlyTicking`.
    pub const IS_RANDOMLY_TICKING: u32 = 1 << 9;
    /// `canBeReplaced`.
    pub const REPLACEABLE: u32 = 1 << 10;
    /// Legacy `liquid`.
    pub const LIQUID: u32 = 1 << 11;
    /// Legacy `isSolid`.
    pub const LEGACY_SOLID: u32 = 1 << 12;
    /// `#blocks_motion_in_heightmap`.
    pub const BLOCKS_MOTION_IN_HEIGHTMAP: u32 = 1 << 13;
    /// `#blocks_motion_in_heightmap_no_leaves`.
    pub const BLOCKS_MOTION_IN_HEIGHTMAP_NO_LEAVES: u32 = 1 << 14;
    /// `ignitedByLava`.
    pub const IGNITED_BY_LAVA: u32 = 1 << 15;
    /// `requiresCorrectToolForDrops`.
    pub const REQUIRES_CORRECT_TOOL_FOR_DROPS: u32 = 1 << 16;
    /// `isRedstoneConductor`.
    pub const IS_REDSTONE_CONDUCTOR: u32 = 1 << 17;
    /// `isSuffocating`.
    pub const IS_SUFFOCATING: u32 = 1 << 18;
    /// `isViewBlocking` (unit probe box at the origin).
    pub const IS_VIEW_BLOCKING: u32 = 1 << 19;
    /// `emissiveRendering`.
    pub const EMISSIVE_RENDERING: u32 = 1 << 20;
    /// `isSignalSource`.
    pub const IS_SIGNAL_SOURCE: u32 = 1 << 21;
    /// `hasAnalogOutputSignal`.
    pub const HAS_ANALOG_OUTPUT_SIGNAL: u32 = 1 << 22;
    /// `hasBlockEntity`.
    pub const HAS_BLOCK_ENTITY: u32 = 1 << 23;
    /// `shouldSpawnTerrainParticles`.
    pub const SPAWN_TERRAIN_PARTICLES: u32 = 1 << 24;
    /// `isLightPermeable`.
    pub const IS_LIGHT_PERMEABLE: u32 = 1 << 25;
    /// Pathfindable on land.
    pub const PATHFINDABLE_LAND: u32 = 1 << 26;
    /// Pathfindable in water.
    pub const PATHFINDABLE_WATER: u32 = 1 << 27;
    /// Pathfindable in air.
    pub const PATHFINDABLE_AIR: u32 = 1 << 28;
    /// `isPossibleToRespawnInThis`.
    pub const POSSIBLE_TO_RESPAWN_IN: u32 = 1 << 29;
    /// `isExceptionForConnection`.
    pub const EXCEPTION_FOR_CONNECTION: u32 = 1 << 30;
    /// `canBeReplaced(WATER)`.
    pub const CAN_BE_REPLACED_BY_FLUID: u32 = 1 << 31;

    /// Names in bit order (for diagnostics).
    pub const NAMES: [&str; 32] = [
        "is_default_state",
        "is_air",
        "can_occlude",
        "solid_render",
        "propagates_skylight_down",
        "use_shape_for_light_occlusion",
        "collision_shape_full_block",
        "has_large_collision_shape",
        "has_dynamic_shape",
        "is_randomly_ticking",
        "replaceable",
        "liquid",
        "legacy_solid",
        "blocks_motion_in_heightmap",
        "blocks_motion_in_heightmap_no_leaves",
        "ignited_by_lava",
        "requires_correct_tool_for_drops",
        "is_redstone_conductor",
        "is_suffocating",
        "is_view_blocking",
        "emissive_rendering",
        "is_signal_source",
        "has_analog_output_signal",
        "has_block_entity",
        "spawn_terrain_particles",
        "is_light_permeable",
        "pathfindable_land",
        "pathfindable_water",
        "pathfindable_air",
        "possible_to_respawn_in",
        "exception_for_connection",
        "can_be_replaced_by_fluid",
    ];
}

/// Block tags the facts depend on (`state-facts.md` §2.14, 18, 28; `shapes.md` §4.3).
#[derive(Debug, Clone)]
pub struct BlockFactTags {
    sets: [Vec<bool>; 5],
}

impl BlockFactTags {
    /// `#minecraft:causes_suffocation`.
    pub const CAUSES_SUFFOCATION: &'static str = "minecraft:causes_suffocation";
    /// `#minecraft:blocks_motion_in_heightmap`.
    pub const BLOCKS_MOTION_IN_HEIGHTMAP: &'static str = "minecraft:blocks_motion_in_heightmap";
    /// `#minecraft:blocks_motion_in_heightmap_no_leaves`.
    pub const BLOCKS_MOTION_IN_HEIGHTMAP_NO_LEAVES: &'static str =
        "minecraft:blocks_motion_in_heightmap_no_leaves";
    /// `#minecraft:shulker_boxes`.
    pub const SHULKER_BOXES: &'static str = "minecraft:shulker_boxes";
    /// `#minecraft:unstable_bottom_center`.
    pub const UNSTABLE_BOTTOM_CENTER: &'static str = "minecraft:unstable_bottom_center";
    /// The five tag ids in storage order.
    pub const ALL: [&'static str; 5] = [
        Self::CAUSES_SUFFOCATION,
        Self::BLOCKS_MOTION_IN_HEIGHTMAP,
        Self::BLOCKS_MOTION_IN_HEIGHTMAP_NO_LEAVES,
        Self::SHULKER_BOXES,
        Self::UNSTABLE_BOTTOM_CENTER,
    ];

    /// No block belongs to any tag.
    pub fn empty(block_count: usize) -> Self {
        Self {
            sets: std::array::from_fn(|_| vec![false; block_count]),
        }
    }

    /// Builds the tag sets by asking `resolve` for the block protocol ids of each tag in [`BlockFactTags::ALL`]
    /// (e.g. from `zenith_worldgen::BlockTags::block_ids`).
    pub fn from_resolver<'a>(
        blocks: &BlockStateTable,
        mut resolve: impl FnMut(&str) -> Option<&'a [u32]>,
    ) -> Self {
        let mut tags = Self::empty(blocks.blocks().len());
        for (set, name) in tags.sets.iter_mut().zip(Self::ALL) {
            for &id in resolve(name).unwrap_or(&[]) {
                if let Some(slot) = set.get_mut(id as usize) {
                    *slot = true;
                }
            }
        }
        tags
    }

    /// Marks `block_protocol_id` as a member of tag number `tag` (index into [`BlockFactTags::ALL`]).
    pub fn insert(&mut self, tag: usize, block_protocol_id: u32) {
        if let Some(slot) = self.sets[tag].get_mut(block_protocol_id as usize) {
            *slot = true;
        }
    }

    fn contains(&self, tag: usize, block: u32) -> bool {
        self.sets[tag].get(block as usize).copied().unwrap_or(false)
    }
}

/// A fluid kind (`BuiltInRegistries.FLUID` entries that blocks can hold).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FluidKind {
    /// `minecraft:empty`.
    Empty,
    /// `minecraft:water`.
    Water,
    /// `minecraft:flowing_water`.
    FlowingWater,
    /// `minecraft:lava`.
    Lava,
    /// `minecraft:flowing_lava`.
    FlowingLava,
}

impl FluidKind {
    /// The registry id.
    pub const fn id(self) -> &'static str {
        match self {
            FluidKind::Empty => "minecraft:empty",
            FluidKind::Water => "minecraft:water",
            FluidKind::FlowingWater => "minecraft:flowing_water",
            FluidKind::Lava => "minecraft:lava",
            FluidKind::FlowingLava => "minecraft:flowing_lava",
        }
    }

    /// Whether this is (a flowing or source form of) water.
    pub const fn is_water(self) -> bool {
        matches!(self, FluidKind::Water | FluidKind::FlowingWater)
    }

    /// Whether this is (a flowing or source form of) lava.
    pub const fn is_lava(self) -> bool {
        matches!(self, FluidKind::Lava | FluidKind::FlowingLava)
    }
}

/// The fluid state of a block state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FluidState {
    /// Fluid kind.
    pub kind: FluidKind,
    /// Amount 0..=8.
    pub amount: u8,
    /// `FlowingFluid.FALLING`.
    pub falling: bool,
    /// Whether this is a source.
    pub is_source: bool,
    /// Own height (`amount / 9` in single precision).
    pub own_height: f32,
}

impl FluidState {
    /// The empty fluid.
    pub const EMPTY: FluidState = FluidState {
        kind: FluidKind::Empty,
        amount: 0,
        falling: false,
        is_source: false,
        own_height: 0.0,
    };

    /// Whether the fluid is empty.
    pub const fn is_empty(&self) -> bool {
        matches!(self.kind, FluidKind::Empty)
    }

    fn source(kind: FluidKind, falling: bool) -> FluidState {
        FluidState {
            kind,
            amount: 8,
            falling,
            is_source: true,
            own_height: 8.0_f32 / 9.0_f32,
        }
    }

    fn flowing(kind: FluidKind, amount: u8, falling: bool) -> FluidState {
        FluidState {
            kind,
            amount,
            falling,
            is_source: false,
            own_height: f32::from(amount) / 9.0_f32,
        }
    }
}

/// Which of the six per-state shapes to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeKind {
    /// Selection outline (`getShape`).
    Outline,
    /// Collision (`getCollisionShape`, empty context).
    Collision,
    /// Visual (`getVisualShape`).
    Visual,
    /// Interaction (`getInteractionShape`).
    Interaction,
    /// Block support (`getBlockSupportShape`).
    Support,
    /// Stored occlusion (`getOcclusionShape`).
    Occlusion,
}

/// Index into the shared shape pool of a [`BlockFacts`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShapeId(pub u32);

/// The static facts of one block state.
#[derive(Debug, Clone)]
pub struct StateFacts {
    /// Block registration (protocol) id.
    pub block: u32,
    /// Bits of [`flag`].
    pub flags: u32,
    /// Light dampening (`getLightDampening`).
    pub light_block: u8,
    /// Light emission.
    pub light_emission: u8,
    /// Map colour id.
    pub map_color_id: u8,
    /// Map colour `0xRRGGBB`.
    pub map_color_rgb: u32,
    /// Push reaction name (`PUSH_PULL`, …).
    pub push_reaction: &'static str,
    /// Note-block instrument serialized name.
    pub instrument: &'static str,
    /// `RenderShape` ordinal: 0 invisible, 1 model.
    pub render_shape: u8,
    /// Offset type.
    pub offset_type: OffsetType,
    /// Post-process position delta, if any.
    pub post_process: Option<[i32; 3]>,
    /// Hardness (−1 = indestructible).
    pub destroy_speed: f32,
    /// Explosion resistance.
    pub explosion_resistance: f32,
    /// Friction.
    pub friction: f32,
    /// Speed factor.
    pub speed_factor: f32,
    /// Jump factor.
    pub jump_factor: f32,
    /// Bounce restitution.
    pub bounce_restitution: f32,
    /// Fall-distance reduction.
    pub fall_distance_reduction: f32,
    /// Shade brightness.
    pub shade_brightness: f32,
    /// Index into [`Tables::sounds`].
    pub sound: u16,
    /// Fluid state.
    pub fluid: FluidState,
    /// The shapes are moved by the position offset ([`BlockFacts::offset`]); stored shapes are those at P0.
    pub shapes_offset: bool,
    /// Shape ids (unmoved when `shapes_offset` is false, otherwise evaluated at the origin).
    pub shape_ids: [ShapeId; 6],
    /// Face occlusion shapes by direction ordinal.
    pub face_occlusion: [ShapeId; 6],
    /// `isFaceSturdy` answers: bit `d` of `[FULL, CENTER, RIGID]`.
    pub face_sturdy: [u8; 3],
    /// Bit `d`: `isFaceFull(collision, d)`.
    pub collision_face_full: u8,
}

impl StateFacts {
    /// Tests a [`flag`] bit.
    #[inline]
    pub const fn has(&self, bit: u32) -> bool {
        self.flags & bit != 0
    }

    /// Whether `isFaceSturdy(d, ty)` holds (evaluated at the origin).
    pub fn is_face_sturdy(&self, d: Direction, ty: SupportType) -> bool {
        let i = match ty {
            SupportType::Full => 0,
            SupportType::Center => 1,
            SupportType::Rigid => 2,
        };
        self.face_sturdy[i] & (1 << d.ordinal()) != 0
    }
}

/// Per-block data kept next to the states.
#[derive(Debug, Clone)]
pub struct BlockMeta {
    /// The table row.
    pub info: &'static BlockInfo,
    /// Block identifier (with namespace).
    pub id: Identifier,
    /// First global state id.
    pub first_state: u32,
    /// Number of states.
    pub state_count: u32,
    /// Default state's global id.
    pub default_state: u32,
}

/// All per-state facts of a game version.
#[derive(Debug)]
pub struct BlockFacts {
    states: Vec<StateFacts>,
    blocks: Vec<BlockMeta>,
    shapes: Vec<VoxelShape>,
}

#[derive(Default)]
struct Pool {
    shapes: Vec<VoxelShape>,
    index: HashMap<Vec<u64>, u32>,
}

impl Pool {
    fn intern(&mut self, s: &VoxelShape) -> ShapeId {
        let mut key = Vec::with_capacity(6 + s.boxes().len() * 6);
        if let Some(b) = s.bounds() {
            key.extend(b.to_array().map(f64::to_bits));
        }
        for b in s.boxes() {
            key.extend(b.to_array().map(f64::to_bits));
        }
        if let Some(&i) = self.index.get(&key) {
            return ShapeId(i);
        }
        let i = self.shapes.len() as u32;
        self.shapes.push(s.clone());
        self.index.insert(key, i);
        ShapeId(i)
    }
}

/// Maximum horizontal / vertical random offset of a block (`shapes.md` §5.4).
fn offset_limits(info: &BlockInfo) -> (f32, f32) {
    let h = match info.id.as_str() {
        "pointed_dripstone" | "sulfur_spike" => 0.125_f32,
        _ => 0.25_f32,
    };
    let v = if info.id == "small_dripleaf" {
        0.1_f32
    } else {
        0.2_f32
    };
    (h, v)
}

/// The random offset of a block at column `(x, z)`. The seed always uses y = 0, so it is constant over the column.
fn offset_of(info: &BlockInfo, x: i32, z: i32) -> [f64; 3] {
    if info.offset == OffsetType::None {
        return [0.0; 3];
    }
    let (h, v) = offset_limits(info);
    let seed = position_hash(x, 0, z);
    let nib = |s: i64| f32::from((s & 15) as u8) / 15.0_f32;
    let hd = f64::from(h);
    let xz = |q: f32| ((f64::from(q) - 0.5) * 0.5).clamp(-hd, hd);
    let ox = xz(nib(seed));
    let oz = xz(nib(seed >> 8));
    let oy = if info.offset == OffsetType::Xyz {
        (f64::from(nib(seed >> 4)) - 1.0) * f64::from(v)
    } else {
        0.0
    };
    [ox, oy, oz]
}

// ── class lists ─────────────────────────────────────────────────────────────────────────────────────────────────

fn is_vegetation(class: &str) -> bool {
    matches!(
        class,
        "SaplingBlock"
            | "MangrovePropaguleBlock"
            | "TallGrassBlock"
            | "ShortDryGrassBlock"
            | "TallDryGrassBlock"
            | "DryVegetationBlock"
            | "BushBlock"
            | "SeagrassBlock"
            | "TallSeagrassBlock"
            | "DoublePlantBlock"
            | "TallFlowerBlock"
            | "SmallDripleafBlock"
            | "PitcherCropBlock"
            | "FlowerBlock"
            | "WitherRoseBlock"
            | "EyeblossomBlock"
            | "MushroomBlock"
            | "CactusFlowerBlock"
            | "CropBlock"
            | "CarrotBlock"
            | "PotatoBlock"
            | "BeetrootBlock"
            | "TorchflowerCropBlock"
            | "StemBlock"
            | "AttachedStemBlock"
            | "LilyPadBlock"
            | "NetherWartBlock"
            | "SweetBerryBushBlock"
            | "NetherFungusBlock"
            | "NetherRootsBlock"
            | "NetherSproutsBlock"
            | "SeaPickleBlock"
            | "AzaleaBlock"
            | "FlowerBedBlock"
            | "LeafLitterBlock"
            | "FireflyBushBlock"
    )
}

fn is_leaves(class: &str) -> bool {
    matches!(
        class,
        "LeavesBlock"
            | "TintedParticleLeavesBlock"
            | "UntintedParticleLeavesBlock"
            | "MangroveLeavesBlock"
    )
}

fn is_skull(class: &str) -> bool {
    matches!(
        class,
        "SkullBlock"
            | "WallSkullBlock"
            | "PlayerHeadBlock"
            | "PlayerWallHeadBlock"
            | "PiglinWallSkullBlock"
            | "WitherSkullBlock"
            | "WitherWallSkullBlock"
    )
}

fn is_cauldron(class: &str) -> bool {
    matches!(
        class,
        "CauldronBlock" | "LavaCauldronBlock" | "LayeredCauldronBlock"
    )
}

fn is_chest(class: &str) -> bool {
    matches!(
        class,
        "ChestBlock" | "TrappedChestBlock" | "CopperChestBlock"
    )
}

fn is_signal_source(class: &str) -> bool {
    matches!(
        class,
        "ButtonBlock"
            | "LeverBlock"
            | "PressurePlateBlock"
            | "WeightedPressurePlateBlock"
            | "RedstoneTorchBlock"
            | "RedstoneWallTorchBlock"
            | "RepeaterBlock"
            | "ComparatorBlock"
            | "DetectorRailBlock"
            | "JukeboxBlock"
            | "DaylightDetectorBlock"
            | "PoweredBlock"
            | "TripWireHookBlock"
            | "LecternBlock"
            | "TargetBlock"
            | "ObserverBlock"
            | "SculkSensorBlock"
            | "CalibratedSculkSensorBlock"
            | "LightningRodBlock"
            | "TrappedChestBlock"
            | "RedstoneWireBlock"
    )
}

fn has_analog_output(class: &str) -> bool {
    is_cauldron(class)
        || is_chest(class)
        || matches!(
            class,
            "BarrelBlock"
                | "FurnaceBlock"
                | "SmokerBlock"
                | "BlastFurnaceBlock"
                | "BeehiveBlock"
                | "CandleCakeBlock"
                | "BrewingStandBlock"
                | "ChiseledBookShelfBlock"
                | "CakeBlock"
                | "CopperBulbBlock"
                | "CopperGolemStatueBlock"
                | "CommandBlock"
                | "CreakingHeartBlock"
                | "ComposterBlock"
                | "DecoratedPotBlock"
                | "EndPortalFrameBlock"
                | "CrafterBlock"
                | "DispenserBlock"
                | "DropperBlock"
                | "HopperBlock"
                | "JukeboxBlock"
                | "LecternBlock"
                | "DetectorRailBlock"
                | "RespawnAnchorBlock"
                | "SculkSensorBlock"
                | "CalibratedSculkSensorBlock"
                | "ShelfBlock"
                | "ShulkerBoxBlock"
        )
}

fn has_block_entity(class: &str) -> bool {
    is_skull(class)
        || is_chest(class)
        || matches!(
            class,
            "BannerBlock"
                | "WallBannerBlock"
                | "BarrelBlock"
                | "BeaconBlock"
                | "BeehiveBlock"
                | "BellBlock"
                | "FurnaceBlock"
                | "SmokerBlock"
                | "BlastFurnaceBlock"
                | "BrewingStandBlock"
                | "BrushableBlock"
                | "SculkSensorBlock"
                | "CalibratedSculkSensorBlock"
                | "CampfireBlock"
                | "CeilingHangingSignBlock"
                | "WallHangingSignBlock"
                | "StandingSignBlock"
                | "WallSignBlock"
                | "ChiseledBookShelfBlock"
                | "CommandBlock"
                | "ComparatorBlock"
                | "ConduitBlock"
                | "CopperGolemStatueBlock"
                | "CrafterBlock"
                | "CreakingHeartBlock"
                | "DaylightDetectorBlock"
                | "DecoratedPotBlock"
                | "DispenserBlock"
                | "DropperBlock"
                | "EnchantingTableBlock"
                | "EndGatewayBlock"
                | "EndPortalBlock"
                | "HopperBlock"
                | "JigsawBlock"
                | "JukeboxBlock"
                | "LecternBlock"
                | "MovingPistonBlock"
                | "PotentSulfurBlock"
                | "SculkCatalystBlock"
                | "SculkShriekerBlock"
                | "ShelfBlock"
                | "ShulkerBoxBlock"
                | "SpawnerBlock"
                | "StructureBlock"
                | "TestBlock"
                | "TestInstanceBlock"
                | "TrialSpawnerBlock"
                | "VaultBlock"
                | "EnderChestBlock"
        )
}

/// Classes whose pathfinding answers are all `false` (`state-facts.md` §2.26).
fn pathfinding_all_false(class: &str) -> bool {
    is_skull(class)
        || is_chest(class)
        || is_cauldron(class)
        || matches!(
            class,
            "StairBlock"
                | "BedBlock"
                | "StrawBedBlock"
                | "FenceBlock"
                | "WallBlock"
                | "IronBarsBlock"
                | "StainedGlassPaneBlock"
                | "ChainBlock"
                | "LanternBlock"
                | "EndRodBlock"
                | "LightningRodBlock"
                | "CactusBlock"
                | "CakeBlock"
                | "CandleCakeBlock"
                | "EnderChestBlock"
                | "CocoaBlock"
                | "ShelfMushroomBlock"
                | "ConduitBlock"
                | "DecoratedPotBlock"
                | "DragonEggBlock"
                | "DriedGhastBlock"
                | "EnchantingTableBlock"
                | "EndPortalFrameBlock"
                | "FarmlandBlock"
                | "PathBlock"
                | "FlowerPotBlock"
                | "GrindstoneBlock"
                | "HeavyCoreBlock"
                | "HopperBlock"
                | "LecternBlock"
                | "MovingPistonBlock"
                | "PistonBaseBlock"
                | "PistonHeadBlock"
                | "RespawnAnchorBlock"
                | "SculkSensorBlock"
                | "CalibratedSculkSensorBlock"
                | "SnifferEggBlock"
                | "SoulSandBlock"
                | "MudBlock"
                | "PointedDripstoneBlock"
                | "SulfurSpikeBlock"
                | "StonecutterBlock"
                | "BellBlock"
                | "CampfireBlock"
                | "AnvilBlock"
                | "BrewingStandBlock"
                | "WallHangingSignBlock"
                | "ComposterBlock"
                | "ChorusPlantBlock"
                | "BambooStalkBlock"
        )
}

/// Stage of a weathering (unwaxed) copper block: `Some(0..=3)`, `None` when the block is not one.
fn weathering_stage(id: &str) -> Option<u8> {
    if id.starts_with("waxed_") {
        return None;
    }
    let (stage, base) = if let Some(b) = id.strip_prefix("exposed_") {
        (1, b)
    } else if let Some(b) = id.strip_prefix("weathered_") {
        (2, b)
    } else if let Some(b) = id.strip_prefix("oxidized_") {
        (3, b)
    } else {
        (0, id)
    };
    let known = match base {
        "copper_block" => stage == 0,
        "copper" => stage != 0,
        "cut_copper"
        | "cut_copper_stairs"
        | "cut_copper_slab"
        | "chiseled_copper"
        | "copper_door"
        | "copper_trapdoor"
        | "copper_bars"
        | "copper_grate"
        | "copper_bulb"
        | "copper_lantern"
        | "copper_chest"
        | "copper_golem_statue"
        | "lightning_rod"
        | "copper_chain" => true,
        _ => false,
    };
    known.then_some(stage)
}

fn dir_prop_any(ctx: &Ctx, names: &[&str]) -> Result<bool, BuildError> {
    for n in names {
        if ctx.bool(n)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn light_emission(ctx: &Ctx) -> Result<u8, BuildError> {
    let clamp = |v: i32| v.clamp(0, 15) as u8;
    Ok(match ctx.info.light {
        LightRule::Const(n) => n,
        LightRule::Lit(n) => {
            if ctx.bool("lit")? {
                n
            } else {
                0
            }
        }
        LightRule::LitCandles => {
            if ctx.bool("lit")? {
                clamp(3 * ctx.int("candles")?)
            } else {
                0
            }
        }
        LightRule::Berries => {
            if ctx.bool("berries")? {
                14
            } else {
                0
            }
        }
        LightRule::AnyFace => {
            if dir_prop_any(ctx, &["up", "down", "north", "east", "south", "west"])? {
                7
            } else {
                0
            }
        }
        LightRule::Level => clamp(ctx.int("level")?),
        LightRule::SeaPickle => {
            if ctx.bool("waterlogged")? {
                clamp(3 + 3 * ctx.int("pickles")?)
            } else {
                0
            }
        }
        LightRule::RespawnAnchor => {
            let charges = ctx.int("charges")? as f32;
            (charges / 4.0_f32 * 15.0_f32).floor() as u8
        }
        LightRule::TrialSpawner => match ctx.str("trial_spawner_state")? {
            "waiting_for_players" => 4,
            "active" | "waiting_for_reward_ejection" | "ejecting_reward" => 8,
            _ => 0,
        },
        LightRule::Vault => match ctx.str("vault_state")? {
            "inactive" => 6,
            _ => 12,
        },
    })
}

fn fluid_state(ctx: &Ctx) -> Result<FluidState, BuildError> {
    let info = ctx.info;
    if info.class == "LiquidBlock" {
        let (still, flow) = if info.id == "lava" {
            (FluidKind::Lava, FluidKind::FlowingLava)
        } else {
            (FluidKind::Water, FluidKind::FlowingWater)
        };
        let level = ctx.int("level")?;
        return Ok(match level {
            0 => FluidState::source(still, false),
            1..=7 => FluidState::flowing(flow, (8 - level) as u8, false),
            _ => FluidState::flowing(flow, 8, true),
        });
    }
    if matches!(
        info.id.as_str(),
        "seagrass" | "tall_seagrass" | "kelp" | "kelp_plant" | "bubble_column"
    ) {
        return Ok(FluidState::source(FluidKind::Water, false));
    }
    if ctx.has("waterlogged") && ctx.bool("waterlogged")? {
        // Copper grates (`WaterloggedTransparentBlock`) report a falling water source.
        return Ok(FluidState::source(
            FluidKind::Water,
            info.class == "WaterloggedTransparentBlock",
        ));
    }
    Ok(FluidState::EMPTY)
}

fn is_randomly_ticking(ctx: &Ctx) -> Result<bool, BuildError> {
    let info = ctx.info;
    let class = info.class.as_str();
    Ok(match class {
        "CropBlock" | "CarrotBlock" | "PotatoBlock" => ctx.int("age")? < 7,
        "BeetrootBlock" => ctx.int("age")? < 3,
        "TorchflowerCropBlock" => true,
        "PitcherCropBlock" => ctx.str("half")? == "lower" && ctx.int("age")? < 4,
        "CocoaBlock" => ctx.int("age")? < 2,
        "NetherWartBlock" | "SweetBerryBushBlock" => ctx.int("age")? < 3,
        "ChorusFlowerBlock" => ctx.int("age")? < 5,
        "KelpBlock" | "WeepingVinesBlock" | "TwistingVinesBlock" | "CaveVinesBlock" => {
            ctx.int("age")? < 25
        }
        c if is_leaves(c) => ctx.int("distance")? == 7 && !ctx.bool("persistent")?,
        "LiquidBlock" => info.id == "lava",
        "RedStoneOreBlock" => ctx.bool("lit")?,
        "BambooStalkBlock" => ctx.int("stage")? == 0,
        "FlowerPotBlock" => matches!(
            info.id.as_str(),
            "potted_open_eyeblossom" | "potted_closed_eyeblossom"
        ),
        _ => match weathering_stage(&info.id) {
            Some(stage) => stage < 3,
            None => info.random_ticks,
        },
    })
}

fn map_color(ctx: &Ctx, tables: &Tables) -> Result<(u8, u32), BuildError> {
    let rule = &ctx.info.map_color;
    let mut name = rule.base.as_str();
    if let Some((cond, alt)) = &rule.alt {
        let hit = match cond {
            MapColorCond::AxisHorizontal => ctx.str("axis")? != "y",
            MapColorCond::Head => ctx.str("part")? == "head",
            MapColorCond::AgeAtLeast6 => ctx.int("age")? >= 6,
            MapColorCond::Waterlogged => ctx.bool("waterlogged")?,
        };
        if hit {
            name = alt;
        }
    }
    let i = tables
        .color_index(name)
        .ok_or_else(|| BuildError::UnknownName {
            what: "map colour",
            name: name.to_owned(),
        })?;
    Ok((tables.colors[i].id, tables.colors[i].rgb))
}

fn sound_index(ctx: &Ctx, tables: &Tables) -> Result<u16, BuildError> {
    let name = match &ctx.info.sound {
        SoundRule::Fixed(n) => n.as_str(),
        SoundRule::Cracked(a, b) => {
            if ctx.bool("cracked")? {
                a
            } else {
                b
            }
        }
    };
    tables
        .sound_index(name)
        .map(|i| i as u16)
        .ok_or_else(|| BuildError::UnknownName {
            what: "sound type",
            name: name.to_owned(),
        })
}

fn use_shape_for_light_occlusion(ctx: &Ctx) -> Result<bool, BuildError> {
    let info = ctx.info;
    Ok(match info.class.as_str() {
        "SlabBlock" => ctx.str("type")? != "double",
        "StairBlock"
        | "PistonHeadBlock"
        | "FarmlandBlock"
        | "PathBlock"
        | "SnowLayerBlock"
        | "EnchantingTableBlock"
        | "EndPortalFrameBlock"
        | "DaylightDetectorBlock"
        | "LecternBlock"
        | "StonecutterBlock"
        | "SculkSensorBlock"
        | "CalibratedSculkSensorBlock"
        | "SculkShriekerBlock"
        | "ShelfBlock" => true,
        "PistonBaseBlock" => ctx.bool("extended")?,
        _ => false,
    })
}

/// Everything the builder needs to evaluate one state.
struct Eval<'a> {
    info: &'static BlockInfo,
    ctx: Ctx<'a>,
    tags: &'a BlockFactTags,
    block: u32,
}

impl BlockFacts {
    /// Builds the facts of every state of `blocks`, using the block tags in `tags`.
    ///
    /// The block list and each block's state count must agree with `docs/specs/blocks/block-table.md`.
    pub fn build(blocks: &BlockStateTable, tags: &BlockFactTags) -> Result<BlockFacts, BuildError> {
        Self::build_impl(blocks, tags, true)
    }

    /// Like [`BlockFacts::build`] for a registry holding only some of the blocks (tests, tools): rows are matched by
    /// id and state count, the protocol index is not required to equal the table's.
    pub fn build_partial(
        blocks: &BlockStateTable,
        tags: &BlockFactTags,
    ) -> Result<BlockFacts, BuildError> {
        Self::build_impl(blocks, tags, false)
    }

    fn build_impl(
        blocks: &BlockStateTable,
        tags: &BlockFactTags,
        strict: bool,
    ) -> Result<BlockFacts, BuildError> {
        let tables = Tables::get();
        let mut pool = Pool::default();
        let mut states: Vec<StateFacts> = Vec::with_capacity(blocks.len());
        let mut metas: Vec<BlockMeta> = Vec::with_capacity(blocks.blocks().len());
        // Per-shape caches; many states share shapes.
        let mut sturdy_cache: HashMap<ShapeId, [u8; 3]> = HashMap::new();
        let mut face_full_cache: HashMap<ShapeId, u8> = HashMap::new();
        let mut occlusion_cache: HashMap<ShapeId, ([ShapeId; 6], bool)> = HashMap::new();
        let empty_id = pool.intern(&VoxelShape::empty());
        let block_id = pool.intern(&VoxelShape::block());

        for entry in blocks.blocks() {
            let info = tables
                .block(entry.id.path())
                .filter(|_| entry.id.namespace() == "minecraft")
                .ok_or_else(|| BuildError::UnknownBlock(entry.id.to_string()))?;
            if info.states != entry.state_count || (strict && info.index != entry.block_protocol_id)
            {
                return Err(BuildError::StateCountMismatch {
                    block: entry.id.to_string(),
                    table_states: info.states,
                    registry_states: entry.state_count,
                });
            }
            metas.push(BlockMeta {
                info,
                id: entry.id.clone(),
                first_state: entry.first_state,
                state_count: entry.state_count,
                default_state: entry.default_state,
            });
            for g in entry.first_state..entry.first_state + entry.state_count {
                let st = blocks
                    .state_by_id_exact(g as i32)
                    .ok_or_else(|| BuildError::UnknownBlock(format!("state {g}")))?;
                let ev = Eval {
                    info,
                    ctx: Ctx {
                        info,
                        props: &st.properties,
                    },
                    tags,
                    block: entry.block_protocol_id,
                };
                let facts = eval_state(
                    &ev,
                    tables,
                    g == entry.default_state,
                    &mut pool,
                    &mut sturdy_cache,
                    &mut face_full_cache,
                    &mut occlusion_cache,
                    (empty_id, block_id),
                )?;
                states.push(facts);
            }
        }
        Ok(BlockFacts {
            states,
            blocks: metas,
            shapes: pool.shapes,
        })
    }

    /// Number of states.
    pub fn len(&self) -> usize {
        self.states.len()
    }

    /// `true` when there are no states.
    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }

    /// Facts of the state with global id `id`.
    pub fn state(&self, id: u32) -> &StateFacts {
        &self.states[id as usize]
    }

    /// All state facts in global-id order.
    pub fn states(&self) -> &[StateFacts] {
        &self.states
    }

    /// Per-block metadata in registry order.
    pub fn blocks(&self) -> &[BlockMeta] {
        &self.blocks
    }

    /// The block a state belongs to.
    pub fn block_of(&self, id: u32) -> &BlockMeta {
        &self.blocks[self.states[id as usize].block as usize]
    }

    /// A pooled shape.
    pub fn shape(&self, id: ShapeId) -> &VoxelShape {
        &self.shapes[id.0 as usize]
    }

    /// Number of distinct shapes in the pool.
    pub fn shape_count(&self) -> usize {
        self.shapes.len()
    }

    /// The state's shape of the given kind as evaluated at the origin (what the parity suite dumps at P0).
    pub fn shape_at_origin(&self, id: u32, kind: ShapeKind) -> &VoxelShape {
        let s = &self.states[id as usize];
        let i = match kind {
            ShapeKind::Occlusion => 0,
            ShapeKind::Collision => 1,
            ShapeKind::Outline => 2,
            ShapeKind::Visual => 3,
            ShapeKind::Interaction => 4,
            ShapeKind::Support => 5,
        };
        self.shape(s.shape_ids[i])
    }

    /// The state's shape of the given kind at block position `(x, y, z)`: the origin shape moved by the difference
    /// of the random offsets when the block has an offset shape.
    pub fn shape_at(&self, id: u32, kind: ShapeKind, pos: [i32; 3]) -> VoxelShape {
        let base = self.shape_at_origin(id, kind);
        let s = &self.states[id as usize];
        if !s.shapes_offset || kind == ShapeKind::Occlusion {
            return base.clone();
        }
        let info = self.block_of(id).info;
        let o0 = offset_of(info, 0, 0);
        let o = offset_of(info, pos[0], pos[2]);
        // Stored shapes already contain the origin offset; recompute from the unmoved shape by moving back.
        base.translate(o[0] - o0[0], o[1] - o0[1], o[2] - o0[2])
    }

    /// `BlockStateBase.getOffset` for the state at `pos`.
    pub fn offset(&self, id: u32, pos: [i32; 3]) -> [f64; 3] {
        offset_of(self.block_of(id).info, pos[0], pos[2])
    }

    /// The face occlusion shape of the state for direction `d`.
    pub fn face_occlusion_shape(&self, id: u32, d: Direction) -> &VoxelShape {
        self.shape(self.states[id as usize].face_occlusion[d.ordinal()])
    }

    /// The sound-type row of the state.
    pub fn sound_type(&self, id: u32) -> &'static crate::table::SoundTypeInfo {
        &Tables::get().sounds[self.states[id as usize].sound as usize]
    }
}

#[allow(clippy::too_many_arguments)]
fn eval_state(
    ev: &Eval,
    tables: &'static Tables,
    is_default: bool,
    pool: &mut Pool,
    sturdy_cache: &mut HashMap<ShapeId, [u8; 3]>,
    face_full_cache: &mut HashMap<ShapeId, u8>,
    occlusion_cache: &mut HashMap<ShapeId, ([ShapeId; 6], bool)>,
    (empty_id, block_id): (ShapeId, ShapeId),
) -> Result<StateFacts, BuildError> {
    let ctx = &ev.ctx;
    let info = ev.info;
    let class = info.class.as_str();
    let fs = families::shapes(ctx)?;

    // ── shapes (shapes.md §5.1) ──
    let has_collision = !info.no_collision;
    let outline = fs.outline;
    let collision = fs.collision.unwrap_or_else(|| {
        if has_collision {
            outline.clone()
        } else {
            VoxelShape::empty()
        }
    });
    let visual = fs.visual.unwrap_or_else(|| collision.clone());
    let support = fs.support.unwrap_or_else(|| collision.clone());
    let interaction = fs.interaction.unwrap_or_else(VoxelShape::empty);
    let occlusion_raw = fs.occlusion.unwrap_or_else(|| outline.clone());
    let can_occlude = !(info.no_collision || info.no_occlusion);

    let off0 = offset_of(info, 0, 0);
    let mv = |s: VoxelShape| {
        if fs.offset {
            s.translate(off0[0], off0[1], off0[2])
        } else {
            s
        }
    };
    let (outline, collision, visual, support, interaction, occlusion_raw) = (
        mv(outline),
        mv(collision),
        mv(visual),
        mv(support),
        mv(interaction),
        mv(occlusion_raw),
    );
    let occlusion = if can_occlude {
        occlusion_raw
    } else {
        VoxelShape::empty()
    };

    let occ_id = pool.intern(&occlusion);
    let col_id = pool.intern(&collision);
    let out_id = pool.intern(&outline);
    let vis_id = pool.intern(&visual);
    let int_id = pool.intern(&interaction);
    let sup_id = pool.intern(&support);

    // Face occlusion shapes (§5.1 rule 7).
    let (face_occlusion, solid_render) = match occlusion_cache.get(&occ_id) {
        Some(v) => *v,
        None => {
            let solid = occlusion.is_full_block();
            let faces: [ShapeId; 6] = if occlusion.is_empty() {
                [empty_id; 6]
            } else if solid {
                [block_id; 6]
            } else {
                let mut f = [empty_id; 6];
                for d in Direction::ALL {
                    f[d.ordinal()] = pool.intern(&occlusion.face_shape(d));
                }
                f
            };
            occlusion_cache.insert(occ_id, (faces, solid));
            (faces, solid)
        }
    };

    // Sturdiness, collision face-full (cached per shape).
    let face_sturdy = *sturdy_cache.entry(sup_id).or_insert_with(|| {
        let mut m = [0u8; 3];
        for (i, ty) in SupportType::ALL.into_iter().enumerate() {
            for d in Direction::ALL {
                if support.is_face_sturdy(d, ty) {
                    m[i] |= 1 << d.ordinal();
                }
            }
        }
        m
    });
    let collision_face_full = *face_full_cache.entry(col_id).or_insert_with(|| {
        let mut m = 0u8;
        for d in Direction::ALL {
            if collision.is_face_full(d) {
                m |= 1 << d.ordinal();
            }
        }
        m
    });

    let fluid = fluid_state(ctx)?;
    let dynamic = info.dynamic_shape;
    let collision_full = if matches!(info.family.as_str(), "bamboo" | "speleothem") {
        false
    } else {
        collision.is_full_block()
    };
    let large = dynamic
        || collision
            .bounds()
            .is_some_and(|b| (0..3).any(|a| b.min[a] < 0.0 || b.max[a] > 1.0));

    // Legacy solid (§2.13).
    let legacy_solid = if info.force_solid_on {
        true
    } else if info.force_solid_off || dynamic {
        false
    } else {
        match collision.bounds() {
            None => false,
            Some(b) => {
                let (xs, ys, zs) = (
                    b.max[0] - b.min[0],
                    b.max[1] - b.min[1],
                    b.max[2] - b.min[2],
                );
                (xs + ys + zs) / 3.0 >= 0.729_166_666_666_666_6 || ys >= 1.0
            }
        }
    };

    // Outline "is a full block" for the skylight default is evaluated on the moved outline.
    let outline_full = outline.is_full_block();
    let propagates_skylight = match class {
        c if is_vegetation(c) => fluid.is_empty(),
        "TransparentBlock" | "StainedGlassBlock" | "WaterloggedTransparentBlock" => true,
        "TintedGlassBlock" => false,
        "LiquidBlock" | "ChorusPlantBlock" | "ShulkerBoxBlock" => false,
        "BarrierBlock" | "LightBlock" | "GlowLichenBlock" => fluid.is_empty(),
        "VineBlock" | "BambooStalkBlock" | "MossyCarpetBlock" | "HangingMossBlock" => true,
        _ => !outline_full && fluid.is_empty(),
    };

    let light_block: u8 = match class {
        c if is_leaves(c) => 1,
        "TintedGlassBlock" => 15,
        _ => {
            if solid_render {
                15
            } else if propagates_skylight {
                0
            } else {
                1
            }
        }
    };

    let in_tag = |t: usize| ev.tags.contains(t, ev.block);
    let conductor = match info.conductor {
        ConductorRule::Default => collision_full,
        ConductorRule::Never => false,
        ConductorRule::Always => true,
    };
    let suffocating = match info.suffocation {
        SuffocationRule::Default => in_tag(0) && collision_full,
        SuffocationRule::Never => false,
        SuffocationRule::Always | SuffocationRule::Shulker => true,
        SuffocationRule::Piston => !ctx.bool("extended")?,
    };
    let view_blocking = match info.view {
        ViewRule::Default => suffocating,
        ViewRule::Plane => !occlusion.is_empty(),
        ViewRule::Snow8 => ctx.int("layers")? >= 8,
    };
    let emissive = match info.emissive {
        EmissiveRule::Never => false,
        EmissiveRule::Always => true,
        EmissiveRule::SensorActive => ctx.str("sculk_sensor_phase")? == "active",
    };

    // Pathfinding (§2.26).
    let default_land_air = !collision_full;
    let default_water = fluid.kind.is_water();
    let (p_land, p_water, p_air) = match class {
        "DoorBlock" | "FenceGateBlock" => {
            let open = ctx.bool("open")?;
            (open, false, open)
        }
        "TrapDoorBlock" => {
            let open = ctx.bool("open")?;
            (open, ctx.bool("waterlogged")?, open)
        }
        "SlabBlock" => (false, default_water, false),
        "SnowLayerBlock" => (ctx.int("layers")? < 5, false, false),
        "SeaPickleBlock" | "AzaleaBlock" => (false, false, false),
        c if is_vegetation(c) => (
            default_land_air,
            default_water,
            if has_collision {
                default_land_air
            } else {
                true
            },
        ),
        "LiquidBlock" => {
            let ok = !fluid.kind.is_lava();
            (ok, ok, ok)
        }
        "PowderSnowBlock" => (true, true, true),
        "ShelfBlock" | "CopperGolemStatueBlock" => (false, default_water, false),
        c if pathfinding_all_false(c) => (false, false, false),
        _ => (default_land_air, default_water, default_land_air),
    };

    // Shade brightness (§3.12).
    let shade: f32 = match class {
        "SnowLayerBlock" => {
            if ctx.int("layers")? == 8 {
                0.2
            } else {
                1.0
            }
        }
        "SoulSandBlock" | "MudBlock" => 0.2,
        "TransparentBlock"
        | "StainedGlassBlock"
        | "TintedGlassBlock"
        | "WaterloggedTransparentBlock"
        | "BarrierBlock"
        | "LightBlock"
        | "StructureVoidBlock" => 1.0,
        _ => {
            if collision_full {
                0.2
            } else {
                1.0
            }
        }
    };

    let (map_color_id, map_color_rgb) = map_color(ctx, tables)?;
    let render_shape = u8::from(
        !(info.air
            || matches!(
                info.id.as_str(),
                "water"
                    | "lava"
                    | "bubble_column"
                    | "moving_piston"
                    | "barrier"
                    | "light"
                    | "structure_void"
                    | "end_portal"
                    | "end_gateway"
            )),
    );

    let mut flags = 0u32;
    let mut set = |bit: u32, v: bool| {
        if v {
            flags |= bit;
        }
    };
    set(flag::IS_DEFAULT_STATE, is_default);
    set(flag::IS_AIR, info.air);
    set(flag::CAN_OCCLUDE, can_occlude);
    set(flag::SOLID_RENDER, solid_render);
    set(flag::PROPAGATES_SKYLIGHT_DOWN, propagates_skylight);
    set(
        flag::USE_SHAPE_FOR_LIGHT_OCCLUSION,
        use_shape_for_light_occlusion(ctx)?,
    );
    set(flag::COLLISION_SHAPE_FULL_BLOCK, collision_full);
    set(flag::HAS_LARGE_COLLISION_SHAPE, large);
    set(flag::HAS_DYNAMIC_SHAPE, dynamic);
    set(flag::IS_RANDOMLY_TICKING, is_randomly_ticking(ctx)?);
    set(flag::REPLACEABLE, info.replaceable);
    set(flag::LIQUID, info.liquid);
    set(flag::LEGACY_SOLID, legacy_solid);
    set(flag::BLOCKS_MOTION_IN_HEIGHTMAP, in_tag(1));
    set(flag::BLOCKS_MOTION_IN_HEIGHTMAP_NO_LEAVES, in_tag(2));
    set(flag::IGNITED_BY_LAVA, info.ignited_by_lava);
    set(flag::REQUIRES_CORRECT_TOOL_FOR_DROPS, info.requires_tool);
    set(flag::IS_REDSTONE_CONDUCTOR, conductor);
    set(flag::IS_SUFFOCATING, suffocating);
    set(flag::IS_VIEW_BLOCKING, view_blocking);
    set(flag::EMISSIVE_RENDERING, emissive);
    set(flag::IS_SIGNAL_SOURCE, is_signal_source(class));
    set(flag::HAS_ANALOG_OUTPUT_SIGNAL, has_analog_output(class));
    set(flag::HAS_BLOCK_ENTITY, has_block_entity(class));
    set(flag::SPAWN_TERRAIN_PARTICLES, !info.no_terrain_particles);
    set(flag::IS_LIGHT_PERMEABLE, !solid_render || light_block == 0);
    set(flag::PATHFINDABLE_LAND, p_land);
    set(flag::PATHFINDABLE_WATER, p_water);
    set(flag::PATHFINDABLE_AIR, p_air);
    set(
        flag::POSSIBLE_TO_RESPAWN_IN,
        matches!(
            class,
            "StandingSignBlock"
                | "WallSignBlock"
                | "CeilingHangingSignBlock"
                | "WallHangingSignBlock"
                | "BannerBlock"
                | "WallBannerBlock"
                | "PressurePlateBlock"
                | "WeightedPressurePlateBlock"
        ) || (!legacy_solid && !info.liquid),
    );
    set(
        flag::EXCEPTION_FOR_CONNECTION,
        is_leaves(class)
            || matches!(
                info.id.as_str(),
                "barrier" | "carved_pumpkin" | "jack_o_lantern" | "melon" | "pumpkin"
            )
            || in_tag(3),
    );
    set(
        flag::CAN_BE_REPLACED_BY_FLUID,
        !matches!(info.id.as_str(), "end_portal" | "end_gateway")
            && (info.replaceable || !legacy_solid),
    );

    Ok(StateFacts {
        block: ev.block,
        flags,
        light_block,
        light_emission: light_emission(ctx)?,
        map_color_id,
        map_color_rgb,
        push_reaction: info.push_reaction.as_str(),
        instrument: info.instrument.as_str(),
        render_shape,
        offset_type: info.offset,
        post_process: match info.post_process {
            PostProcess::None => None,
            PostProcess::Own => Some([0, 0, 0]),
            PostProcess::Above => Some([0, 1, 0]),
        },
        destroy_speed: info.hardness,
        explosion_resistance: info.resistance.max(0.0),
        friction: info.friction,
        speed_factor: info.speed_factor,
        jump_factor: info.jump_factor,
        bounce_restitution: info.bounce_restitution,
        fall_distance_reduction: info.fall_distance_reduction,
        shade_brightness: shade,
        sound: sound_index(ctx, tables)?,
        fluid,
        shapes_offset: fs.offset,
        shape_ids: [occ_id, col_id, out_id, vis_id, int_id, sup_id],
        face_occlusion,
        face_sturdy,
        collision_face_full,
    })
}
