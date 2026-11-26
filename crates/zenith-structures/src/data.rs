//! Data-pack JSON codecs and runtime loader for `minecraft:worldgen/structure_set`,
//! `minecraft:worldgen/structure`, `minecraft:worldgen/template_pool`, and
//! `minecraft:worldgen/processor_list`
//! (`docs/specs/worldgen/structures-placement-and-jigsaw.md` §1, §3, §4;
//! `docs/specs/worldgen/structures-legacy-pieces.md`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use thiserror::Error;
use zenith_codec::codec::{
    BoolCodec, Codec, DataResult, FloatCodec, IdentifierCodec, IntCodec, RecordReader, StringCodec,
    decode_dispatch, list, validated,
};
use zenith_codec::format::PackType;
use zenith_codec::json::{Json, ParseMode, parse_bytes};
use zenith_codec::resources::{PackResources, ResourceError};
use zenith_registry::Identifier;
use zenith_worldgen::providers::{HeightProviderCodec, IntProviderCodec};
use zenith_worldgen::{
    BlockStateSpec, HeightProvider, HeightmapKind, IntProvider, TerrainAdjustment,
};

use crate::geom::BlockPos;

const STRUCTURE_SET_DIR: &str = "worldgen/structure_set";
const STRUCTURE_DIR: &str = "worldgen/structure";
const TEMPLATE_POOL_DIR: &str = "worldgen/template_pool";
const PROCESSOR_LIST_DIR: &str = "worldgen/processor_list";

/// Error returned when loading structure registries from a data pack.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum StructureLoadError {
    /// Pack listing failed.
    #[error("pack listing error: {0}")]
    Resource(#[from] ResourceError),
    /// A listed file could not be read from the pack.
    #[error("missing resource {id} in pack")]
    MissingResource {
        /// File identifier.
        id: Identifier,
    },
    /// Strict JSON parsing failed.
    #[error("JSON parse error in {registry} entry {id}: {message}")]
    JsonParse {
        /// Registry directory.
        registry: &'static str,
        /// Element identifier.
        id: Identifier,
        /// Error detail.
        message: String,
    },
    /// Codec decode or validation failed.
    #[error("codec decode error in {registry} entry {id}: {message}")]
    Decode {
        /// Registry directory.
        registry: &'static str,
        /// Element identifier.
        id: Identifier,
        /// Error detail.
        message: String,
    },
    /// Unknown biome tag referenced by a structure or placement.
    #[error("unknown biome tag '#{tag}' in {registry} entry {owner}")]
    UnknownBiomeTag {
        /// Registry directory.
        registry: &'static str,
        /// Element identifier.
        owner: Identifier,
        /// Missing tag identifier.
        tag: Identifier,
    },
}

// ---------------------------------------------------------------------------
// §1 Structure sets and placements
// ---------------------------------------------------------------------------

/// Frequency reduction method (`StructurePlacement.FrequencyReductionMethod`, §1.2–§1.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FrequencyReductionMethod {
    /// `default` (`setLargeFeatureWithSalt(seed, salt, cx, cz)`).
    #[default]
    Default,
    /// `legacy_type_1` (pillager outpost reducer).
    LegacyType1,
    /// `legacy_type_2` (`setLargeFeatureWithSalt(seed, cx, cz, 10387320)`).
    LegacyType2,
    /// `legacy_type_3` (`setLargeFeatureSeed(seed, cx, cz)`).
    LegacyType3,
}

/// Spread type for `minecraft:random_spread` (§1.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RandomSpreadType {
    /// Uniform `nextInt(limit)`.
    #[default]
    Linear,
    /// `(nextInt(limit) + nextInt(limit)) / 2`.
    Triangular,
}

/// Exclusion zone on a spreading placement (§1.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExclusionZone {
    /// Registry ID of the forbidden `minecraft:worldgen/structure_set`.
    pub other_set: Identifier,
    /// Chunk search radius in `[1, 16]`.
    pub chunk_count: i32,
}

/// Shared fields of `AbstractSpreadingStructurePlacement` (§1.2).
#[derive(Debug, Clone, PartialEq)]
pub struct SpreadingPlacement {
    /// Locate offset in `[-16, 16]^3`.
    pub locate_offset: BlockPos,
    /// Frequency reduction method.
    pub frequency_reduction_method: FrequencyReductionMethod,
    /// Frequency in `[0.0, 1.0]`.
    pub frequency: f32,
    /// Non-negative placement salt.
    pub salt: i32,
    /// Optional exclusion zone.
    pub exclusion_zone: Option<ExclusionZone>,
}

/// All 3 built-in structure placements (`minecraft:worldgen/structure_placement`, §1.4–§1.6).
#[derive(Debug, Clone, PartialEq)]
pub enum StructurePlacement {
    /// `minecraft:random_spread`.
    RandomSpread {
        /// Shared spreading parameters.
        common: SpreadingPlacement,
        /// Grid spacing (`> separation`, in `[0, 4096]`).
        spacing: i32,
        /// Minimum chunk separation (`< spacing`, in `[0, 4096]`).
        separation: i32,
        /// Linear or triangular offset distribution.
        spread_type: RandomSpreadType,
    },
    /// `minecraft:concentric_rings`.
    ConcentricRings {
        /// Shared spreading parameters.
        common: SpreadingPlacement,
        /// Ring distance parameter in `[0, 1023]`.
        distance: i32,
        /// Initial count in ring 0 (`[0, 1023]`).
        spread: i32,
        /// Total count across all rings (`[1, 4095]`).
        count: i32,
        /// Resolved preferred biome IDs for the 112-block horizontal biome search.
        preferred_biomes: HashSet<Identifier>,
    },
    /// `minecraft:dimension_origin` (26.3).
    DimensionOrigin,
}

impl StructurePlacement {
    /// Locate offset for `/locate` and explorer maps (§1.7).
    pub const fn locate_offset(&self) -> BlockPos {
        match self {
            Self::RandomSpread { common, .. } | Self::ConcentricRings { common, .. } => {
                common.locate_offset
            }
            Self::DimensionOrigin => BlockPos::ZERO,
        }
    }

    /// Locate position `(chunkX << 4) + offset.x, offset.y, (chunkZ << 4) + offset.z` (§1.7).
    pub const fn locate_pos(&self, chunk_x: i32, chunk_z: i32) -> BlockPos {
        let off = self.locate_offset();
        BlockPos::new(
            (chunk_x << 4).wrapping_add(off.x),
            off.y,
            (chunk_z << 4).wrapping_add(off.z),
        )
    }
}

/// Reference to a `Structure` (registry ID or inline definition).
#[derive(Debug, Clone)]
pub enum StructureRef {
    /// Registered `minecraft:worldgen/structure` ID.
    Reference(Identifier),
    /// Inline structure definition.
    Inline(Box<StructureDef>),
}

/// One weighted structure entry inside a [`StructureSet`] (§1.1).
#[derive(Debug, Clone)]
pub struct StructureSelectionEntry {
    /// Target structure.
    pub structure: StructureRef,
    /// Selection weight (`>= 1`).
    pub weight: i32,
}

/// Decoded `minecraft:worldgen/structure_set` entry (§1.1).
#[derive(Debug, Clone)]
pub struct StructureSet {
    /// Registry identifier of this set.
    pub id: Identifier,
    /// Weighted candidate structures.
    pub structures: Vec<StructureSelectionEntry>,
    /// Placement rule.
    pub placement: StructurePlacement,
}

// ---------------------------------------------------------------------------
// §3 Common structure schema and all 16 structure types
// ---------------------------------------------------------------------------

/// `GenerationStep.Decoration` ordinals (`0..=10`, §3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum GenerationStep {
    /// 0: `raw_generation`
    RawGeneration = 0,
    /// 1: `lakes`
    Lakes = 1,
    /// 2: `local_modifications`
    LocalModifications = 2,
    /// 3: `underground_structures`
    UndergroundStructures = 3,
    /// 4: `surface_structures`
    SurfaceStructures = 4,
    /// 5: `strongholds`
    Strongholds = 5,
    /// 6: `underground_ores`
    UndergroundOres = 6,
    /// 7: `underground_decoration`
    UndergroundDecoration = 7,
    /// 8: `fluid_springs`
    FluidSprings = 8,
    /// 9: `vegetal_decoration`
    VegetalDecoration = 9,
    /// 10: `top_layer_modification`
    TopLayerModification = 10,
}

impl GenerationStep {
    /// Ordinal `0..=10`.
    #[inline]
    pub const fn ordinal(self) -> usize {
        self as usize
    }

    fn from_name(s: &str) -> Option<Self> {
        Some(match s {
            "raw_generation" => Self::RawGeneration,
            "lakes" => Self::Lakes,
            "local_modifications" => Self::LocalModifications,
            "underground_structures" => Self::UndergroundStructures,
            "surface_structures" => Self::SurfaceStructures,
            "strongholds" => Self::Strongholds,
            "underground_ores" => Self::UndergroundOres,
            "underground_decoration" => Self::UndergroundDecoration,
            "fluid_springs" => Self::FluidSprings,
            "vegetal_decoration" => Self::VegetalDecoration,
            "top_layer_modification" => Self::TopLayerModification,
            _ => return None,
        })
    }
}

/// Spawn-override bounding-box scope (`"piece"` or `"full"`, §3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpawnBoundingBox {
    /// Individual piece box.
    Piece,
    /// Full structure start box.
    Full,
}

/// Mob category keys for `spawn_overrides` (§3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MobCategory {
    /// `monster`
    Monster,
    /// `creature`
    Creature,
    /// `ambient`
    Ambient,
    /// `axolotls`
    Axolotls,
    /// `underground_water_creature`
    UndergroundWaterCreature,
    /// `water_creature`
    WaterCreature,
    /// `water_ambient`
    WaterAmbient,
    /// `misc`
    Misc,
}

impl MobCategory {
    fn from_name(s: &str) -> Option<Self> {
        Some(match s {
            "monster" => Self::Monster,
            "creature" => Self::Creature,
            "ambient" => Self::Ambient,
            "axolotls" => Self::Axolotls,
            "underground_water_creature" => Self::UndergroundWaterCreature,
            "water_creature" => Self::WaterCreature,
            "water_ambient" => Self::WaterAmbient,
            "misc" => Self::Misc,
            _ => return None,
        })
    }
}

/// One weighted mob spawn entry inside `StructureSpawnOverride`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnerEntry {
    /// Entity type ID.
    pub entity_type: Identifier,
    /// Weight (`>= 0`).
    pub weight: i32,
    /// Minimum group count.
    pub min_count: i32,
    /// Maximum group count.
    pub max_count: i32,
}

/// Per-category spawn override on a structure (§3.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureSpawnOverride {
    /// Piece-level or full-structure box.
    pub bounding_box: SpawnBoundingBox,
    /// Weighted spawn entries.
    pub spawns: Vec<SpawnerEntry>,
}

/// Common settings shared by all 16 structure types (`StructureSettings`, §3.1).
#[derive(Debug, Clone)]
pub struct StructureSettings {
    /// Resolved allowed biome IDs at the generation point.
    pub biomes: HashSet<Identifier>,
    /// Per-category spawn overrides.
    pub spawn_overrides: BTreeMap<MobCategory, StructureSpawnOverride>,
    /// Decoration step during `FEATURES`.
    pub step: GenerationStep,
    /// Terrain adaptation mode (`Beardifier` + `±12` box inflation).
    pub terrain_adaptation: TerrainAdjustment,
}

/// `LiquidSettings` for jigsaw structures and pool elements (§3.2, §3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LiquidSettings {
    /// `apply_waterlogging` (default).
    #[default]
    ApplyWaterlogging,
    /// `ignore_waterlogging`.
    IgnoreWaterlogging,
}

/// `max_distance_from_center` for `minecraft:jigsaw` (§3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaxDistance {
    /// Horizontal max distance in `[1, 128]`.
    pub horizontal: i32,
    /// Vertical max distance in `[1, 4064]` (default `4064` in object form).
    pub vertical: i32,
}

/// `dimension_padding` for `minecraft:jigsaw` (§3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DimensionPadding {
    /// Minimum padding above `minY` (`>= 0`).
    pub bottom: i32,
    /// Minimum padding below `maxY` (`>= 0`).
    pub top: i32,
}

impl DimensionPadding {
    /// Zero padding (`{ bottom: 0, top: 0 }`).
    pub const ZERO: Self = Self { bottom: 0, top: 0 };

    /// Whether both `bottom` and `top` are zero.
    #[inline]
    pub const fn is_zero(self) -> bool {
        self.bottom == 0 && self.top == 0
    }
}

/// `PoolAliasBinding` (`minecraft:worldgen/pool_alias_binding`, §3.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PoolAliasBinding {
    /// `minecraft:direct`: `alias -> target`.
    Direct {
        /// Alias pool ID.
        alias: Identifier,
        /// Target pool ID.
        target: Identifier,
    },
    /// `minecraft:random`: `alias -> weighted targets`.
    Random {
        /// Alias pool ID.
        alias: Identifier,
        /// Weighted `(target_id, weight)` choices.
        targets: Vec<(Identifier, i32)>,
    },
    /// `minecraft:random_group`: weighted list of binding groups.
    RandomGroup {
        /// Weighted `(bindings, weight)` choices.
        groups: Vec<(Vec<PoolAliasBinding>, i32)>,
    },
}

/// Reference to a `StructureTemplatePool` (registry ID or inline).
#[derive(Debug, Clone)]
pub enum TemplatePoolRef {
    /// Registered `minecraft:worldgen/template_pool` ID.
    Reference(Identifier),
    /// Inline template pool definition.
    Inline(Box<StructureTemplatePool>),
}

/// Configuration for `minecraft:jigsaw` (§3.2).
#[derive(Debug, Clone)]
pub struct JigsawConfig {
    /// Start template pool.
    pub start_pool: TemplatePoolRef,
    /// Optional start jigsaw block name to anchor on.
    pub start_jigsaw_name: Option<Identifier>,
    /// Maximum depth (`size`, in `[0, 20]`).
    pub size: i32,
    /// Start height provider.
    pub start_height: HeightProvider,
    /// Whether to apply the vertical expansion hack (`ySpan <= 16`).
    pub use_expansion_hack: bool,
    /// Optional heightmap projection for the start piece.
    pub project_start_to_heightmap: Option<HeightmapKind>,
    /// Horizontal and vertical distance bounds from center.
    pub max_distance_from_center: MaxDistance,
    /// Pool alias bindings.
    pub pool_aliases: Vec<PoolAliasBinding>,
    /// Dimension vertical padding.
    pub dimension_padding: DimensionPadding,
    /// Default liquid settings for pieces.
    pub liquid_settings: LiquidSettings,
}

/// Mineshaft variant (`"normal"` | `"mesa"`, `structures-legacy-pieces.md` §2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MineshaftType {
    /// Normal oak mineshaft.
    Normal,
    /// Badlands/mesa dark oak mineshaft.
    Mesa,
}

/// Ocean ruin temperature variant (`"warm"` | `"cold"`, `structures-legacy-pieces.md` §12.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OceanRuinTemp {
    /// Warm single-layer ruins.
    Warm,
    /// Cold 3-layer ruins.
    Cold,
}

/// Ruined portal vertical placement mode (`structures-legacy-pieces.md` §16.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuinedPortalPlacement {
    /// `on_land_surface`
    OnLandSurface,
    /// `partly_buried`
    PartlyBuried,
    /// `on_ocean_floor`
    OnOceanFloor,
    /// `in_mountain`
    InMountain,
    /// `underground`
    Underground,
    /// `in_nether`
    InNether,
}

/// One setup entry for `minecraft:ruined_portal` (`structures-legacy-pieces.md` §16.1).
#[derive(Debug, Clone, PartialEq)]
pub struct RuinedPortalSetup {
    /// Vertical placement mode.
    pub placement: RuinedPortalPlacement,
    /// Probability of carving air pocket (`[0, 1]`).
    pub air_pocket_probability: f32,
    /// Mossiness for `BlockAgeProcessor` (`[0, 1]`).
    pub mossiness: f32,
    /// Whether to scatter jungle leaves.
    pub overgrown: bool,
    /// Whether to add vines.
    pub vines: bool,
    /// Whether cold-biome checks apply.
    pub can_be_cold: bool,
    /// Whether to apply `BlackstoneReplaceProcessor`.
    pub replace_with_blackstone: bool,
    /// Selection weight (`> 0`).
    pub weight: f32,
}

/// All 16 built-in structure types (`minecraft:worldgen/structure_type`).
#[derive(Debug, Clone)]
pub enum StructureKind {
    /// `minecraft:jigsaw`
    Jigsaw(Box<JigsawConfig>),
    /// `minecraft:mineshaft`
    Mineshaft(MineshaftType),
    /// `minecraft:stronghold`
    Stronghold,
    /// `minecraft:fortress`
    Fortress,
    /// `minecraft:ocean_monument`
    OceanMonument,
    /// `minecraft:woodland_mansion`
    WoodlandMansion,
    /// `minecraft:desert_pyramid`
    DesertPyramid,
    /// `minecraft:jungle_temple`
    JungleTemple,
    /// `minecraft:swamp_hut`
    SwampHut,
    /// `minecraft:igloo`
    Igloo,
    /// `minecraft:shipwreck`
    Shipwreck {
        /// Whether this is a beached shipwreck.
        is_beached: bool,
    },
    /// `minecraft:ocean_ruin`
    OceanRuin {
        /// Warm or cold variant.
        biome_temp: OceanRuinTemp,
        /// Probability of large ruin (`[0, 1]`).
        large_probability: f32,
        /// Probability of cluster around large ruin (`[0, 1]`).
        cluster_probability: f32,
    },
    /// `minecraft:buried_treasure`
    BuriedTreasure,
    /// `minecraft:end_city`
    EndCity,
    /// `minecraft:nether_fossil`
    NetherFossil {
        /// Vertical starting height provider.
        height: HeightProvider,
    },
    /// `minecraft:ruined_portal`
    RuinedPortal {
        /// Non-empty weighted setup list.
        setups: Vec<RuinedPortalSetup>,
    },
}

/// Decoded `minecraft:worldgen/structure` entry.
#[derive(Debug, Clone)]
pub struct StructureDef {
    /// Registry identifier (or synthetic ID for inline structures).
    pub id: Identifier,
    /// Common settings.
    pub settings: StructureSettings,
    /// Type-specific configuration.
    pub kind: StructureKind,
}

// ---------------------------------------------------------------------------
// §3.5 Template pools and pool elements
// ---------------------------------------------------------------------------

/// `StructureTemplatePool.Projection` (§3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Projection {
    /// `"rigid"`: fixed Y, contributes `BeardifierRigid`.
    Rigid,
    /// `"terrain_matching"`: applies `GravityProcessor(WORLD_SURFACE_WG, -1)`.
    TerrainMatching,
}

/// Reference to a `StructureProcessorList` (registry ID or inline list).
#[derive(Debug, Clone)]
pub enum ProcessorListRef {
    /// Registered `minecraft:worldgen/processor_list` ID.
    Reference(Identifier),
    /// Inline processor list.
    Inline(Vec<StructureProcessor>),
}

/// All 5 built-in pool element types (`minecraft:worldgen/structure_pool_element`, §3.5).
#[derive(Debug, Clone)]
pub enum StructurePoolElement {
    /// `minecraft:single_pool_element`.
    Single {
        /// Template identifier (`data/<ns>/structure/<path>.nbt`).
        location: Identifier,
        /// Processor list.
        processors: ProcessorListRef,
        /// Projection mode.
        projection: Projection,
        /// Optional liquid-settings override.
        override_liquid_settings: Option<LiquidSettings>,
    },
    /// `minecraft:legacy_single_pool_element`.
    LegacySingle {
        /// Template identifier.
        location: Identifier,
        /// Processor list.
        processors: ProcessorListRef,
        /// Projection mode.
        projection: Projection,
        /// Optional liquid-settings override.
        override_liquid_settings: Option<LiquidSettings>,
    },
    /// `minecraft:feature_pool_element`.
    Feature {
        /// Feature holder (ID string or inline JSON; preserved for placement).
        feature: Json,
        /// Projection mode.
        projection: Projection,
    },
    /// `minecraft:list_pool_element`.
    List {
        /// Child elements (each already coerced to this list's projection).
        elements: Vec<StructurePoolElement>,
        /// Projection mode.
        projection: Projection,
    },
    /// `minecraft:empty_pool_element`.
    Empty,
}

impl StructurePoolElement {
    /// Projection mode of this element (`Empty` is always `TerrainMatching`).
    #[inline]
    pub const fn projection(&self) -> Projection {
        match self {
            Self::Single { projection, .. }
            | Self::LegacySingle { projection, .. }
            | Self::Feature { projection, .. }
            | Self::List { projection, .. } => *projection,
            Self::Empty => Projection::TerrainMatching,
        }
    }

    /// Ground-level delta (`1` for single/legacy single, `0` otherwise).
    #[inline]
    pub const fn ground_level_delta(&self) -> i32 {
        match self {
            Self::Single { .. } | Self::LegacySingle { .. } => 1,
            Self::Feature { .. } | Self::List { .. } | Self::Empty => 0,
        }
    }

    /// Whether this is `EmptyPoolElement.INSTANCE`.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }

    fn set_projection(&mut self, proj: Projection) {
        match self {
            Self::Single { projection, .. }
            | Self::LegacySingle { projection, .. }
            | Self::Feature { projection, .. }
            | Self::List { projection, .. } => *projection = proj,
            Self::Empty => {}
        }
    }
}

/// Decoded `minecraft:worldgen/template_pool` entry (§3.5).
#[derive(Debug, Clone)]
pub struct StructureTemplatePool {
    /// Fallback pool reference.
    pub fallback: TemplatePoolRef,
    /// Raw weighted `(element, weight)` pairs.
    pub raw_elements: Vec<(Arc<StructurePoolElement>, i32)>,
    /// Flat weight-expanded list (`templates` in vanilla), preserving exact order.
    pub templates: Vec<Arc<StructurePoolElement>>,
}

impl StructureTemplatePool {
    /// Constructs a template pool and expands `raw_elements` into `templates`.
    pub fn new(
        fallback: TemplatePoolRef,
        raw_elements: Vec<(Arc<StructurePoolElement>, i32)>,
    ) -> Self {
        let total: usize = raw_elements.iter().map(|(_, w)| (*w).max(0) as usize).sum();
        let mut templates = Vec::with_capacity(total);
        for (elem, weight) in &raw_elements {
            for _ in 0..(*weight).max(0) {
                templates.push(Arc::clone(elem));
            }
        }
        Self {
            fallback,
            raw_elements,
            templates,
        }
    }

    /// Number of expanded entries in `templates`.
    #[inline]
    pub fn size(&self) -> usize {
        self.templates.len()
    }
}

// ---------------------------------------------------------------------------
// §4.4 Structure processors, rule tests, pos tests, and BE modifiers
// ---------------------------------------------------------------------------

/// `HolderSet<Block>` decoded as raw block IDs and `#tag` IDs (resolved at placement time).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawBlockHolderSet {
    /// `#<tag_id>`
    Tag(Identifier),
    /// Explicit list of block IDs.
    Ids(Vec<Identifier>),
}

/// All 10 `RuleTest` types (`minecraft:worldgen/rule_test`, §4.4).
#[derive(Debug, Clone, PartialEq)]
pub enum RuleTest {
    /// `minecraft:always_true`
    AlwaysTrue,
    /// `minecraft:block_match`
    BlockMatch {
        /// Target block ID.
        block: Identifier,
    },
    /// `minecraft:blockstate_match`
    BlockStateMatch {
        /// Target block state.
        block_state: BlockStateSpec,
    },
    /// `minecraft:tag_match` (bare tag ID, no `#`).
    TagMatch {
        /// Block tag ID.
        tag: Identifier,
    },
    /// `minecraft:random_block_match`
    RandomBlockMatch {
        /// Target block ID.
        block: Identifier,
        /// Replacement probability.
        probability: f32,
    },
    /// `minecraft:random_blockstate_match`
    RandomBlockStateMatch {
        /// Target block state.
        block_state: BlockStateSpec,
        /// Replacement probability.
        probability: f32,
    },
    /// `minecraft:height_match` (26.3)
    HeightMatch {
        /// Inclusive min Y.
        min_inclusive: i32,
        /// Inclusive max Y.
        max_inclusive: i32,
    },
    /// `minecraft:all_of` (26.3)
    AllOf {
        /// Child rules (short-circuit AND).
        rules: Vec<RuleTest>,
    },
    /// `minecraft:any_of` (26.3)
    AnyOf {
        /// Child rules (short-circuit OR).
        rules: Vec<RuleTest>,
    },
    /// `minecraft:not` (26.3)
    Not {
        /// Negated rule.
        rule: Box<RuleTest>,
    },
}

/// Axis for `minecraft:axis_aligned_linear_pos`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Axis {
    /// X axis.
    X,
    /// Y axis.
    Y,
    /// Z axis.
    Z,
}

/// All 3 `PosRuleTest` types (`minecraft:worldgen/pos_rule_test`, §4.4).
#[derive(Debug, Clone, PartialEq)]
pub enum PosRuleTest {
    /// `minecraft:always_true`
    AlwaysTrue,
    /// `minecraft:linear_pos`
    LinearPos {
        /// Chance at `min_dist`.
        min_chance: f32,
        /// Chance at `max_dist`.
        max_chance: f32,
        /// Minimum Manhattan distance.
        min_dist: i32,
        /// Maximum Manhattan distance.
        max_dist: i32,
    },
    /// `minecraft:axis_aligned_linear_pos`
    AxisAlignedLinearPos {
        /// Chance at `min_dist`.
        min_chance: f32,
        /// Chance at `max_dist`.
        max_chance: f32,
        /// Minimum axis distance.
        min_dist: i32,
        /// Maximum axis distance.
        max_dist: i32,
        /// Axis to measure along.
        axis: Axis,
    },
}

/// All 4 `RuleBlockEntityModifier` types (`minecraft:worldgen/rule_block_entity_modifier`, §4.4).
#[derive(Debug, Clone, PartialEq)]
pub enum RuleBlockEntityModifier {
    /// `minecraft:passthrough`
    Passthrough,
    /// `minecraft:clear`
    Clear,
    /// `minecraft:append_static` (JSON data merged as NBT compound).
    AppendStatic {
        /// Static compound payload (JSON representation).
        data: Json,
    },
    /// `minecraft:append_loot`
    AppendLoot {
        /// Loot table identifier.
        loot_table: Identifier,
    },
}

/// One rule inside `minecraft:rule` (`ProcessorRule`, §4.4).
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessorRule {
    /// Predicate against template block state.
    pub input_predicate: RuleTest,
    /// Predicate against existing world block state.
    pub location_predicate: RuleTest,
    /// Predicate against position.
    pub position_predicate: PosRuleTest,
    /// Replacement block state.
    pub output_state: BlockStateSpec,
    /// Block entity NBT modifier.
    pub block_entity_modifier: RuleBlockEntityModifier,
}

/// All 11 built-in structure processors (`minecraft:worldgen/structure_processor`, §4.4).
#[derive(Debug, Clone, PartialEq)]
pub enum StructureProcessor {
    /// `minecraft:nop`
    Nop,
    /// `minecraft:block_ignore`
    BlockIgnore {
        /// Ignored block IDs.
        blocks: Vec<Identifier>,
    },
    /// `minecraft:block_rot`
    BlockRot {
        /// Optional filter of rottable blocks.
        rottable_blocks: Option<RawBlockHolderSet>,
        /// Survival integrity in `[0.0, 1.0]`.
        integrity: f32,
    },
    /// `minecraft:gravity`
    Gravity {
        /// Target heightmap.
        heightmap: HeightmapKind,
        /// Vertical offset added to height.
        offset: i32,
    },
    /// `minecraft:rule`
    Rule {
        /// Ordered processor rules.
        rules: Vec<ProcessorRule>,
    },
    /// `minecraft:protected_blocks`
    ProtectedBlocks {
        /// Protected block holder set.
        value: RawBlockHolderSet,
    },
    /// `minecraft:capped`
    Capped {
        /// Delegate processor applied to at most `limit` blocks.
        delegate: Box<StructureProcessor>,
        /// Maximum number of blocks to replace.
        limit: IntProvider,
    },
    /// `minecraft:jigsaw_replacement`
    JigsawReplacement,
    /// `minecraft:lava_submerged_block`
    LavaSubmergedBlock,
    /// `minecraft:block_age`
    BlockAge {
        /// Mossiness probability.
        mossiness: f32,
    },
    /// `minecraft:blackstone_replace`
    BlackstoneReplace,
}

impl StructureProcessor {
    /// Whether this processor requires whole-piece scanning across chunk borders (`Capped` only).
    #[inline]
    pub const fn evaluates_entire_piece_state(&self) -> bool {
        matches!(self, Self::Capped { .. })
    }
}

// ---------------------------------------------------------------------------
// Loaded structure data container
// ---------------------------------------------------------------------------

/// All structure-related dynamic registries loaded at run time from the user's data pack.
///
/// Preserves exact dynamic-registry numeric ID order (ascending by full file identifier) for
/// both `structure_sets` and `structures`.
#[derive(Debug, Clone)]
pub struct StructureData {
    structure_sets: Vec<Arc<StructureSet>>,
    structure_sets_by_id: HashMap<Identifier, usize>,
    structures: Vec<Arc<StructureDef>>,
    structures_by_id: HashMap<Identifier, usize>,
    template_pools: HashMap<Identifier, Arc<StructureTemplatePool>>,
    processor_lists: HashMap<Identifier, Arc<Vec<StructureProcessor>>>,
    biome_tags: BTreeMap<Identifier, Vec<Identifier>>,
}

impl StructureData {
    /// Loads `worldgen/structure_set`, `worldgen/structure`, `worldgen/template_pool`, and
    /// `worldgen/processor_list` from `pack`, resolving biome tags via `biome_tags`
    /// (typically `worldgen_data.biome_tags()`).
    pub fn load(
        pack: &dyn PackResources,
        biome_tags: &BTreeMap<Identifier, Vec<Identifier>>,
    ) -> Result<Self, StructureLoadError> {
        let mut processor_lists = HashMap::new();
        for (elem_id, file_id) in list_json_files(pack, PROCESSOR_LIST_DIR)? {
            let json = read_json_file(pack, PROCESSOR_LIST_DIR, &elem_id, &file_id)?;
            let procs =
                decode_processor_list(&json)
                    .demand()
                    .map_err(|e| StructureLoadError::Decode {
                        registry: PROCESSOR_LIST_DIR,
                        id: elem_id.clone(),
                        message: e.message,
                    })?;
            processor_lists.insert(elem_id, Arc::new(procs));
        }

        let mut template_pools = HashMap::new();
        for (elem_id, file_id) in list_json_files(pack, TEMPLATE_POOL_DIR)? {
            let json = read_json_file(pack, TEMPLATE_POOL_DIR, &elem_id, &file_id)?;
            let pool =
                decode_template_pool(&json)
                    .demand()
                    .map_err(|e| StructureLoadError::Decode {
                        registry: TEMPLATE_POOL_DIR,
                        id: elem_id.clone(),
                        message: e.message,
                    })?;
            template_pools.insert(elem_id, Arc::new(pool));
        }

        let mut structures = Vec::new();
        let mut structures_by_id = HashMap::new();
        for (elem_id, file_id) in list_json_files(pack, STRUCTURE_DIR)? {
            let json = read_json_file(pack, STRUCTURE_DIR, &elem_id, &file_id)?;
            let def = decode_structure(&elem_id, &json, biome_tags)?;
            let idx = structures.len();
            structures.push(Arc::new(def));
            structures_by_id.insert(elem_id, idx);
        }

        let mut structure_sets = Vec::new();
        let mut structure_sets_by_id = HashMap::new();
        for (elem_id, file_id) in list_json_files(pack, STRUCTURE_SET_DIR)? {
            let json = read_json_file(pack, STRUCTURE_SET_DIR, &elem_id, &file_id)?;
            let set = decode_structure_set(&elem_id, &json, biome_tags)?;
            let idx = structure_sets.len();
            structure_sets.push(Arc::new(set));
            structure_sets_by_id.insert(elem_id, idx);
        }

        Ok(Self {
            structure_sets,
            structure_sets_by_id,
            structures,
            structures_by_id,
            template_pools,
            processor_lists,
            biome_tags: biome_tags.clone(),
        })
    }

    /// All structure sets in dynamic-registry numeric ID order.
    #[inline]
    pub fn structure_sets(&self) -> &[Arc<StructureSet>] {
        &self.structure_sets
    }

    /// Looks up a structure set by identifier.
    #[inline]
    pub fn structure_set(&self, id: &Identifier) -> Option<&Arc<StructureSet>> {
        self.structure_sets_by_id
            .get(id)
            .map(|&i| &self.structure_sets[i])
    }

    /// All structures in dynamic-registry numeric ID order.
    #[inline]
    pub fn structures(&self) -> &[Arc<StructureDef>] {
        &self.structures
    }

    /// Looks up a structure by identifier.
    #[inline]
    pub fn structure(&self, id: &Identifier) -> Option<&Arc<StructureDef>> {
        self.structures_by_id.get(id).map(|&i| &self.structures[i])
    }

    /// Resolves a [`StructureRef`] to a [`StructureDef`].
    #[inline]
    pub fn resolve_structure<'a>(&'a self, r: &'a StructureRef) -> Option<&'a StructureDef> {
        match r {
            StructureRef::Reference(id) => self.structure(id).map(Arc::as_ref),
            StructureRef::Inline(s) => Some(s.as_ref()),
        }
    }

    /// Looks up a template pool by identifier.
    #[inline]
    pub fn template_pool(&self, id: &Identifier) -> Option<&Arc<StructureTemplatePool>> {
        self.template_pools.get(id)
    }

    /// Looks up a processor list by identifier.
    #[inline]
    pub fn processor_list(&self, id: &Identifier) -> Option<&Arc<Vec<StructureProcessor>>> {
        self.processor_lists.get(id)
    }

    /// Decodes a `Holder<StructureProcessorList>` JSON value (a registry id string or an inline
    /// list) into its processors, e.g. the `fossil_processors` of a `fossil` feature.
    pub fn decode_processor_list_holder(
        &self,
        json: &Json,
    ) -> Result<Vec<StructureProcessor>, String> {
        match ProcessorListRefCodec.decode(json).demand() {
            Ok(ProcessorListRef::Inline(list)) => Ok(list),
            Ok(ProcessorListRef::Reference(id)) => self
                .processor_list(&id)
                .map(|l| l.as_ref().clone())
                .ok_or_else(|| format!("unknown processor list {id}")),
            Err(e) => Err(e.message),
        }
    }

    /// Resolved biome tags (`tags/worldgen/biome`).
    #[inline]
    pub fn biome_tags(&self) -> &BTreeMap<Identifier, Vec<Identifier>> {
        &self.biome_tags
    }

    /// Checks whether `biome` belongs to biome tag `tag_id` (without `#`).
    #[inline]
    pub fn biome_in_tag(&self, tag_id: &Identifier, biome: &Identifier) -> bool {
        self.biome_tags
            .get(tag_id)
            .is_some_and(|list| list.contains(biome))
    }
}

// ---------------------------------------------------------------------------
// File discovery and JSON helpers
// ---------------------------------------------------------------------------

fn list_json_files(
    pack: &dyn PackResources,
    dir: &str,
) -> Result<Vec<(Identifier, Identifier)>, StructureLoadError> {
    let prefix = format!("{dir}/");
    let mut files: Vec<Identifier> = Vec::new();
    for ns in pack.namespaces(PackType::Data) {
        let listed = pack.list(PackType::Data, &ns, dir)?;
        for file_id in listed {
            if file_id.path().ends_with(".json") && file_id.path().starts_with(&prefix) {
                files.push(file_id);
            }
        }
    }
    // Dynamic-registry numeric ID order: ascending by full file identifier (spec §2.4).
    files.sort();
    let mut out = Vec::with_capacity(files.len());
    for file_id in files {
        let rel = file_id
            .path()
            .strip_prefix(&prefix)
            .and_then(|s| s.strip_suffix(".json"))
            .expect("filtered above");
        let elem_id = Identifier::from_parts(file_id.namespace(), rel).map_err(|e| {
            StructureLoadError::Decode {
                registry: "worldgen",
                id: file_id.clone(),
                message: e.to_string(),
            }
        })?;
        out.push((elem_id, file_id));
    }
    Ok(out)
}

fn read_json_file(
    pack: &dyn PackResources,
    registry: &'static str,
    elem_id: &Identifier,
    file_id: &Identifier,
) -> Result<Json, StructureLoadError> {
    let bytes =
        pack.get(PackType::Data, file_id)
            .ok_or_else(|| StructureLoadError::MissingResource {
                id: file_id.clone(),
            })?;
    parse_bytes(&bytes, ParseMode::Data).map_err(|e| StructureLoadError::JsonParse {
        registry,
        id: elem_id.clone(),
        message: e.to_string(),
    })
}

fn resolve_biome_holder_set(
    input: &Json,
    biome_tags: &BTreeMap<Identifier, Vec<Identifier>>,
    registry: &'static str,
    owner: &Identifier,
) -> Result<HashSet<Identifier>, StructureLoadError> {
    if let Some(s) = input.as_str() {
        if let Some(tag_str) = s.strip_prefix('#') {
            let tag_id = Identifier::parse(tag_str).map_err(|e| StructureLoadError::Decode {
                registry,
                id: owner.clone(),
                message: format!("invalid biome tag '#{tag_str}': {e}"),
            })?;
            let ids =
                biome_tags
                    .get(&tag_id)
                    .ok_or_else(|| StructureLoadError::UnknownBiomeTag {
                        registry,
                        owner: owner.clone(),
                        tag: tag_id,
                    })?;
            return Ok(ids.iter().cloned().collect());
        }
        let id = Identifier::parse(s).map_err(|e| StructureLoadError::Decode {
            registry,
            id: owner.clone(),
            message: format!("invalid biome id '{s}': {e}"),
        })?;
        let mut set = HashSet::with_capacity(1);
        set.insert(id);
        return Ok(set);
    }
    if let Some(arr) = input.as_array() {
        let mut set = HashSet::with_capacity(arr.len());
        for item in arr {
            let s = item.as_str().ok_or_else(|| StructureLoadError::Decode {
                registry,
                id: owner.clone(),
                message: format!("expected biome id string, got {}", item.type_name()),
            })?;
            let id = Identifier::parse(s).map_err(|e| StructureLoadError::Decode {
                registry,
                id: owner.clone(),
                message: format!("invalid biome id '{s}': {e}"),
            })?;
            set.insert(id);
        }
        return Ok(set);
    }
    Err(StructureLoadError::Decode {
        registry,
        id: owner.clone(),
        message: format!("expected biome holder set, got {}", input.type_name()),
    })
}

// ---------------------------------------------------------------------------
// JSON decoders
// ---------------------------------------------------------------------------

struct BlockStateSpecCodec;

impl Codec<BlockStateSpec> for BlockStateSpecCodec {
    fn decode(&self, input: &Json) -> DataResult<BlockStateSpec> {
        // 26.3 `BlockState.CODEC` accepts a bare block ID string (shorthand for default state)
        // in addition to `{ "id"|"Name": ..., "properties"|"Properties": {...} }`.
        if let Some(s) = input.as_str() {
            return match Identifier::parse(s) {
                Ok(name) => DataResult::Success(BlockStateSpec {
                    name,
                    properties: BTreeMap::new(),
                }),
                Err(e) => DataResult::error(format!("Invalid BlockState identifier '{s}': {e}")),
            };
        }
        let Some(obj) = input.as_object() else {
            return DataResult::error(format!(
                "Expected BlockState string or object, got {}",
                input.type_name()
            ));
        };
        // 26.3 (DataVersion 5006+) uses `id`/`properties`; also accept legacy `Name`/`Properties`.
        let name_val = obj.get("id").or_else(|| obj.get("Name"));
        let Some(name_val) = name_val else {
            return DataResult::error("Missing required 'id' (or 'Name') in BlockState");
        };
        let name = match IdentifierCodec.decode(name_val) {
            DataResult::Success(id) => id,
            DataResult::Error { message, .. } => return DataResult::error(message),
            DataResult::Failure(m) => return DataResult::Failure(m),
        };
        let mut properties = BTreeMap::new();
        if let Some(props_val) = obj.get("properties").or_else(|| obj.get("Properties"))
            && !matches!(props_val, Json::Null)
        {
            let Some(props_obj) = props_val.as_object() else {
                return DataResult::error("BlockState properties must be an object");
            };
            for (k, v) in props_obj.iter() {
                let Some(ks) = k.as_str() else {
                    return DataResult::error("BlockState property key must be UTF-8");
                };
                let Some(vs) = v.as_str() else {
                    return DataResult::error(format!(
                        "BlockState property '{ks}' value must be string"
                    ));
                };
                properties.insert(ks.to_owned(), vs.to_owned());
            }
        }
        DataResult::Success(BlockStateSpec { name, properties })
    }

    fn encode(&self, _value: &BlockStateSpec) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct RawBlockHolderSetCodec;

impl Codec<RawBlockHolderSet> for RawBlockHolderSetCodec {
    fn decode(&self, input: &Json) -> DataResult<RawBlockHolderSet> {
        if let Some(s) = input.as_str() {
            if let Some(tag_str) = s.strip_prefix('#') {
                return match Identifier::parse(tag_str) {
                    Ok(id) => DataResult::Success(RawBlockHolderSet::Tag(id)),
                    Err(e) => DataResult::error(format!("Invalid block tag '{s}': {e}")),
                };
            }
            return match Identifier::parse(s) {
                Ok(id) => DataResult::Success(RawBlockHolderSet::Ids(vec![id])),
                Err(e) => DataResult::error(format!("Invalid block id '{s}': {e}")),
            };
        }
        list(IdentifierCodec)
            .decode(input)
            .map(RawBlockHolderSet::Ids)
    }

    fn encode(&self, _value: &RawBlockHolderSet) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct HeightmapKindCodec;

impl Codec<HeightmapKind> for HeightmapKindCodec {
    fn decode(&self, input: &Json) -> DataResult<HeightmapKind> {
        let Some(s) = input.as_str() else {
            return DataResult::error(format!(
                "Expected heightmap string, got {}",
                input.type_name()
            ));
        };
        match s {
            "WORLD_SURFACE_WG" => DataResult::Success(HeightmapKind::WorldSurfaceWg),
            "WORLD_SURFACE" => DataResult::Success(HeightmapKind::WorldSurface),
            "OCEAN_FLOOR_WG" => DataResult::Success(HeightmapKind::OceanFloorWg),
            "OCEAN_FLOOR" => DataResult::Success(HeightmapKind::OceanFloor),
            "MOTION_BLOCKING" => DataResult::Success(HeightmapKind::MotionBlocking),
            "MOTION_BLOCKING_NO_LEAVES" => {
                DataResult::Success(HeightmapKind::MotionBlockingNoLeaves)
            }
            other => DataResult::error(format!("Unknown Heightmap.Types '{other}'")),
        }
    }

    fn encode(&self, _value: &HeightmapKind) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct RuleTestCodec;

impl Codec<RuleTest> for RuleTestCodec {
    fn decode(&self, input: &Json) -> DataResult<RuleTest> {
        decode_dispatch(input, "predicate_type", |ty, obj| {
            let id = Identifier::parse(ty).ok()?;
            if id.namespace() != "minecraft" {
                return None;
            }
            match id.path() {
                "always_true" => Some(DataResult::Success(RuleTest::AlwaysTrue)),
                "block_match" => Some({
                    let mut r = RecordReader::new(obj);
                    let block = r.required("block", &IdentifierCodec);
                    r.finish(|| Some(RuleTest::BlockMatch { block: block? }))
                }),
                "blockstate_match" => Some({
                    let mut r = RecordReader::new(obj);
                    let block_state = r.required("block_state", &BlockStateSpecCodec);
                    r.finish(|| {
                        Some(RuleTest::BlockStateMatch {
                            block_state: block_state?,
                        })
                    })
                }),
                "tag_match" => Some({
                    let mut r = RecordReader::new(obj);
                    let tag = r.required("tag", &IdentifierCodec);
                    r.finish(|| Some(RuleTest::TagMatch { tag: tag? }))
                }),
                "random_block_match" => Some({
                    let mut r = RecordReader::new(obj);
                    let block = r.required("block", &IdentifierCodec);
                    let probability = r.required("probability", &FloatCodec);
                    r.finish(|| {
                        Some(RuleTest::RandomBlockMatch {
                            block: block?,
                            probability: probability?,
                        })
                    })
                }),
                "random_blockstate_match" => Some({
                    let mut r = RecordReader::new(obj);
                    let block_state = r.required("block_state", &BlockStateSpecCodec);
                    let probability = r.required("probability", &FloatCodec);
                    r.finish(|| {
                        Some(RuleTest::RandomBlockStateMatch {
                            block_state: block_state?,
                            probability: probability?,
                        })
                    })
                }),
                "height_match" => Some({
                    let mut r = RecordReader::new(obj);
                    let min_inclusive = r.required("min_inclusive", &IntCodec);
                    let max_inclusive = r.required("max_inclusive", &IntCodec);
                    r.finish(|| {
                        Some(RuleTest::HeightMatch {
                            min_inclusive: min_inclusive?,
                            max_inclusive: max_inclusive?,
                        })
                    })
                }),
                "all_of" => Some({
                    let mut r = RecordReader::new(obj);
                    let rules = r.required("rules", &list(RuleTestCodec));
                    r.finish(|| Some(RuleTest::AllOf { rules: rules? }))
                }),
                "any_of" => Some({
                    let mut r = RecordReader::new(obj);
                    let rules = r.required("rules", &list(RuleTestCodec));
                    r.finish(|| Some(RuleTest::AnyOf { rules: rules? }))
                }),
                "not" => Some({
                    let mut r = RecordReader::new(obj);
                    let rule = r.required("rule", &RuleTestCodec);
                    r.finish(|| {
                        Some(RuleTest::Not {
                            rule: Box::new(rule?),
                        })
                    })
                }),
                _ => None,
            }
        })
    }

    fn encode(&self, _value: &RuleTest) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct AxisCodec;

impl Codec<Axis> for AxisCodec {
    fn decode(&self, input: &Json) -> DataResult<Axis> {
        match input.as_str() {
            Some("x") => DataResult::Success(Axis::X),
            Some("y") => DataResult::Success(Axis::Y),
            Some("z") => DataResult::Success(Axis::Z),
            _ => DataResult::error("Expected axis 'x', 'y', or 'z'"),
        }
    }
    fn encode(&self, _value: &Axis) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct PosRuleTestCodec;

impl Codec<PosRuleTest> for PosRuleTestCodec {
    fn decode(&self, input: &Json) -> DataResult<PosRuleTest> {
        decode_dispatch(input, "predicate_type", |ty, obj| {
            let id = Identifier::parse(ty).ok()?;
            if id.namespace() != "minecraft" {
                return None;
            }
            match id.path() {
                "always_true" => Some(DataResult::Success(PosRuleTest::AlwaysTrue)),
                "linear_pos" => Some({
                    let mut r = RecordReader::new(obj);
                    let min_chance = r.optional("min_chance", &FloatCodec, 0.0);
                    let max_chance = r.optional("max_chance", &FloatCodec, 0.0);
                    let min_dist = r.optional("min_dist", &IntCodec, 0);
                    let max_dist = r.optional("max_dist", &IntCodec, 0);
                    r.finish(|| {
                        Some(PosRuleTest::LinearPos {
                            min_chance,
                            max_chance,
                            min_dist,
                            max_dist,
                        })
                    })
                }),
                "axis_aligned_linear_pos" => Some({
                    let mut r = RecordReader::new(obj);
                    let min_chance = r.optional("min_chance", &FloatCodec, 0.0);
                    let max_chance = r.optional("max_chance", &FloatCodec, 0.0);
                    let min_dist = r.optional("min_dist", &IntCodec, 0);
                    let max_dist = r.optional("max_dist", &IntCodec, 0);
                    let axis = r.optional("axis", &AxisCodec, Axis::Y);
                    r.finish(|| {
                        Some(PosRuleTest::AxisAlignedLinearPos {
                            min_chance,
                            max_chance,
                            min_dist,
                            max_dist,
                            axis,
                        })
                    })
                }),
                _ => None,
            }
        })
    }
    fn encode(&self, _value: &PosRuleTest) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct BeModifierCodec;

impl Codec<RuleBlockEntityModifier> for BeModifierCodec {
    fn decode(&self, input: &Json) -> DataResult<RuleBlockEntityModifier> {
        decode_dispatch(input, "type", |ty, obj| {
            let id = Identifier::parse(ty).ok()?;
            if id.namespace() != "minecraft" {
                return None;
            }
            match id.path() {
                "passthrough" => Some(DataResult::Success(RuleBlockEntityModifier::Passthrough)),
                "clear" => Some(DataResult::Success(RuleBlockEntityModifier::Clear)),
                "append_static" => Some({
                    let Some(map) = obj.as_object() else {
                        return Some(DataResult::error("append_static must be an object"));
                    };
                    match map.get("data") {
                        Some(d) => DataResult::Success(RuleBlockEntityModifier::AppendStatic {
                            data: d.clone(),
                        }),
                        None => DataResult::error("Missing 'data' in append_static"),
                    }
                }),
                "append_loot" => Some({
                    let mut r = RecordReader::new(obj);
                    let loot_table = r.required("loot_table", &IdentifierCodec);
                    r.finish(|| {
                        Some(RuleBlockEntityModifier::AppendLoot {
                            loot_table: loot_table?,
                        })
                    })
                }),
                _ => None,
            }
        })
    }
    fn encode(&self, _value: &RuleBlockEntityModifier) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct ProcessorRuleCodec;

impl Codec<ProcessorRule> for ProcessorRuleCodec {
    fn decode(&self, input: &Json) -> DataResult<ProcessorRule> {
        let mut r = RecordReader::new(input);
        let input_predicate = r.required("input_predicate", &RuleTestCodec);
        let location_predicate = r.required("location_predicate", &RuleTestCodec);
        let position_predicate = r.lenient(
            "position_predicate",
            &PosRuleTestCodec,
            PosRuleTest::AlwaysTrue,
        );
        let output_state = r.required("output_state", &BlockStateSpecCodec);
        let block_entity_modifier = r.lenient(
            "block_entity_modifier",
            &BeModifierCodec,
            RuleBlockEntityModifier::Passthrough,
        );
        r.finish(|| {
            Some(ProcessorRule {
                input_predicate: input_predicate?,
                location_predicate: location_predicate?,
                position_predicate,
                output_state: output_state?,
                block_entity_modifier,
            })
        })
    }
    fn encode(&self, _value: &ProcessorRule) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct StructureProcessorCodec;

impl Codec<StructureProcessor> for StructureProcessorCodec {
    fn decode(&self, input: &Json) -> DataResult<StructureProcessor> {
        decode_dispatch(input, "processor_type", |ty, obj| {
            let id = Identifier::parse(ty).ok()?;
            if id.namespace() != "minecraft" {
                return None;
            }
            match id.path() {
                "nop" => Some(DataResult::Success(StructureProcessor::Nop)),
                "block_ignore" => Some({
                    let mut r = RecordReader::new(obj);
                    let blocks = r.required("blocks", &list(BlockStateSpecCodec));
                    r.finish(|| {
                        Some(StructureProcessor::BlockIgnore {
                            blocks: blocks?.into_iter().map(|b| b.name).collect(),
                        })
                    })
                }),
                "block_rot" => Some({
                    let mut r = RecordReader::new(obj);
                    let rottable_blocks =
                        r.optional_opt("rottable_blocks", &RawBlockHolderSetCodec);
                    let integrity = r.required("integrity", &FloatCodec);
                    r.finish(|| {
                        Some(StructureProcessor::BlockRot {
                            rottable_blocks,
                            integrity: integrity?,
                        })
                    })
                }),
                "gravity" => Some({
                    let mut r = RecordReader::new(obj);
                    let heightmap = r.optional(
                        "heightmap",
                        &HeightmapKindCodec,
                        HeightmapKind::WorldSurfaceWg,
                    );
                    let offset = r.optional("offset", &IntCodec, 0);
                    r.finish(|| Some(StructureProcessor::Gravity { heightmap, offset }))
                }),
                "rule" => Some({
                    let mut r = RecordReader::new(obj);
                    let rules = r.required("rules", &list(ProcessorRuleCodec));
                    r.finish(|| Some(StructureProcessor::Rule { rules: rules? }))
                }),
                "protected_blocks" => Some({
                    let mut r = RecordReader::new(obj);
                    let value = r.required("value", &RawBlockHolderSetCodec);
                    r.finish(|| Some(StructureProcessor::ProtectedBlocks { value: value? }))
                }),
                "capped" => Some({
                    let mut r = RecordReader::new(obj);
                    let delegate = r.required("delegate", &StructureProcessorCodec);
                    let limit = r.required("limit", &IntProviderCodec);
                    r.finish(|| {
                        Some(StructureProcessor::Capped {
                            delegate: Box::new(delegate?),
                            limit: limit?,
                        })
                    })
                }),
                "jigsaw_replacement" => {
                    Some(DataResult::Success(StructureProcessor::JigsawReplacement))
                }
                "lava_submerged_block" => {
                    Some(DataResult::Success(StructureProcessor::LavaSubmergedBlock))
                }
                "block_age" => Some({
                    let mut r = RecordReader::new(obj);
                    let mossiness = r.required("mossiness", &FloatCodec);
                    r.finish(|| {
                        Some(StructureProcessor::BlockAge {
                            mossiness: mossiness?,
                        })
                    })
                }),
                "blackstone_replace" => {
                    Some(DataResult::Success(StructureProcessor::BlackstoneReplace))
                }
                _ => None,
            }
        })
    }

    fn encode(&self, _value: &StructureProcessor) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

fn decode_processor_list(input: &Json) -> DataResult<Vec<StructureProcessor>> {
    if matches!(input, Json::Array(_)) {
        return list(StructureProcessorCodec).decode(input);
    }
    let mut r = RecordReader::new(input);
    let processors = r.required("processors", &list(StructureProcessorCodec));
    r.finish(|| processors)
}

struct ProcessorListRefCodec;

impl Codec<ProcessorListRef> for ProcessorListRefCodec {
    fn decode(&self, input: &Json) -> DataResult<ProcessorListRef> {
        if input.as_str().is_some() {
            return IdentifierCodec
                .decode(input)
                .map(ProcessorListRef::Reference);
        }
        decode_processor_list(input).map(ProcessorListRef::Inline)
    }

    fn encode(&self, _value: &ProcessorListRef) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct ProjectionCodec;

impl Codec<Projection> for ProjectionCodec {
    fn decode(&self, input: &Json) -> DataResult<Projection> {
        match input.as_str() {
            Some("rigid") => DataResult::Success(Projection::Rigid),
            Some("terrain_matching") => DataResult::Success(Projection::TerrainMatching),
            other => DataResult::error(format!("Unknown projection {other:?}")),
        }
    }
    fn encode(&self, _value: &Projection) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct LiquidSettingsCodec;

impl Codec<LiquidSettings> for LiquidSettingsCodec {
    fn decode(&self, input: &Json) -> DataResult<LiquidSettings> {
        match input.as_str() {
            Some("apply_waterlogging") => DataResult::Success(LiquidSettings::ApplyWaterlogging),
            Some("ignore_waterlogging") => DataResult::Success(LiquidSettings::IgnoreWaterlogging),
            other => DataResult::error(format!("Unknown liquid_settings {other:?}")),
        }
    }
    fn encode(&self, _value: &LiquidSettings) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct PoolElementCodec;

impl Codec<StructurePoolElement> for PoolElementCodec {
    fn decode(&self, input: &Json) -> DataResult<StructurePoolElement> {
        decode_dispatch(input, "element_type", |ty, obj| {
            let id = Identifier::parse(ty).ok()?;
            if id.namespace() != "minecraft" {
                return None;
            }
            match id.path() {
                "single_pool_element" => Some({
                    let mut r = RecordReader::new(obj);
                    let location = r.required("location", &IdentifierCodec);
                    let processors = r.required("processors", &ProcessorListRefCodec);
                    let projection = r.required("projection", &ProjectionCodec);
                    let override_liquid_settings =
                        r.optional_opt("override_liquid_settings", &LiquidSettingsCodec);
                    r.finish(|| {
                        Some(StructurePoolElement::Single {
                            location: location?,
                            processors: processors?,
                            projection: projection?,
                            override_liquid_settings,
                        })
                    })
                }),
                "legacy_single_pool_element" => Some({
                    let mut r = RecordReader::new(obj);
                    let location = r.required("location", &IdentifierCodec);
                    let processors = r.required("processors", &ProcessorListRefCodec);
                    let projection = r.required("projection", &ProjectionCodec);
                    let override_liquid_settings =
                        r.optional_opt("override_liquid_settings", &LiquidSettingsCodec);
                    r.finish(|| {
                        Some(StructurePoolElement::LegacySingle {
                            location: location?,
                            processors: processors?,
                            projection: projection?,
                            override_liquid_settings,
                        })
                    })
                }),
                "feature_pool_element" => Some({
                    let Some(map) = obj.as_object() else {
                        return Some(DataResult::error("feature_pool_element must be an object"));
                    };
                    let feature = map.get("feature").cloned();
                    let mut r = RecordReader::new(obj);
                    let projection = r.required("projection", &ProjectionCodec);
                    r.finish(|| {
                        Some(StructurePoolElement::Feature {
                            feature: feature?,
                            projection: projection?,
                        })
                    })
                }),
                "list_pool_element" => Some({
                    let mut r = RecordReader::new(obj);
                    let elements = r.required("elements", &list(PoolElementCodec));
                    let projection = r.required("projection", &ProjectionCodec);
                    r.finish(|| {
                        let proj = projection?;
                        let mut elems = elements?;
                        for e in &mut elems {
                            e.set_projection(proj);
                        }
                        Some(StructurePoolElement::List {
                            elements: elems,
                            projection: proj,
                        })
                    })
                }),
                "empty_pool_element" => Some(DataResult::Success(StructurePoolElement::Empty)),
                _ => None,
            }
        })
    }

    fn encode(&self, _value: &StructurePoolElement) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct WeightedPoolElementCodec;

impl Codec<(Arc<StructurePoolElement>, i32)> for WeightedPoolElementCodec {
    fn decode(&self, input: &Json) -> DataResult<(Arc<StructurePoolElement>, i32)> {
        let mut r = RecordReader::new(input);
        let element = r.required("element", &PoolElementCodec);
        let weight = r.required("weight", &IntCodec);
        r.finish(|| Some((Arc::new(element?), weight?)))
    }
    fn encode(&self, _value: &(Arc<StructurePoolElement>, i32)) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct TemplatePoolRefCodec;

impl Codec<TemplatePoolRef> for TemplatePoolRefCodec {
    fn decode(&self, input: &Json) -> DataResult<TemplatePoolRef> {
        if input.as_str().is_some() {
            return IdentifierCodec
                .decode(input)
                .map(TemplatePoolRef::Reference);
        }
        decode_template_pool(input).map(|p| TemplatePoolRef::Inline(Box::new(p)))
    }
    fn encode(&self, _value: &TemplatePoolRef) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

fn decode_template_pool(input: &Json) -> DataResult<StructureTemplatePool> {
    let mut r = RecordReader::new(input);
    let fallback = r.required("fallback", &TemplatePoolRefCodec);
    let elements = r.required("elements", &list(WeightedPoolElementCodec));
    r.finish(|| Some(StructureTemplatePool::new(fallback?, elements?)))
}

struct WeightedIdCodec;

impl Codec<(Identifier, i32)> for WeightedIdCodec {
    fn decode(&self, input: &Json) -> DataResult<(Identifier, i32)> {
        let mut r = RecordReader::new(input);
        let data = r.required("data", &IdentifierCodec);
        let weight = r.required("weight", &IntCodec);
        r.finish(|| Some((data?, weight?)))
    }
    fn encode(&self, _value: &(Identifier, i32)) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct WeightedAliasGroupCodec;

impl Codec<(Vec<PoolAliasBinding>, i32)> for WeightedAliasGroupCodec {
    fn decode(&self, input: &Json) -> DataResult<(Vec<PoolAliasBinding>, i32)> {
        let mut r = RecordReader::new(input);
        let data = r.required("data", &list(PoolAliasBindingCodec));
        let weight = r.required("weight", &IntCodec);
        r.finish(|| Some((data?, weight?)))
    }
    fn encode(&self, _value: &(Vec<PoolAliasBinding>, i32)) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct PoolAliasBindingCodec;

impl Codec<PoolAliasBinding> for PoolAliasBindingCodec {
    fn decode(&self, input: &Json) -> DataResult<PoolAliasBinding> {
        decode_dispatch(input, "type", |ty, obj| {
            let id = Identifier::parse(ty).ok()?;
            if id.namespace() != "minecraft" {
                return None;
            }
            match id.path() {
                "direct" => Some({
                    let mut r = RecordReader::new(obj);
                    let alias = r.required("alias", &IdentifierCodec);
                    let target = r.required("target", &IdentifierCodec);
                    r.finish(|| {
                        Some(PoolAliasBinding::Direct {
                            alias: alias?,
                            target: target?,
                        })
                    })
                }),
                "random" => Some({
                    let mut r = RecordReader::new(obj);
                    let alias = r.required("alias", &IdentifierCodec);
                    let targets = r.required("targets", &list(WeightedIdCodec));
                    r.finish(|| {
                        Some(PoolAliasBinding::Random {
                            alias: alias?,
                            targets: targets?,
                        })
                    })
                }),
                "random_group" => Some({
                    let mut r = RecordReader::new(obj);
                    let groups = r.required("groups", &list(WeightedAliasGroupCodec));
                    r.finish(|| Some(PoolAliasBinding::RandomGroup { groups: groups? }))
                }),
                _ => None,
            }
        })
    }
    fn encode(&self, _value: &PoolAliasBinding) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct MaxDistanceCodec;

impl Codec<MaxDistance> for MaxDistanceCodec {
    fn decode(&self, input: &Json) -> DataResult<MaxDistance> {
        let h_codec = validated(IntCodec, |v: &i32| {
            if (1..=128).contains(v) {
                Ok(())
            } else {
                Err(format!("horizontal {v} outside [1, 128]"))
            }
        });
        let v_codec = validated(IntCodec, |v: &i32| {
            if (1..=4064).contains(v) {
                Ok(())
            } else {
                Err(format!("vertical {v} outside [1, 4064]"))
            }
        });
        if matches!(input, Json::Number(_)) {
            return h_codec.decode(input).map(|d| MaxDistance {
                horizontal: d,
                vertical: d,
            });
        }
        let mut r = RecordReader::new(input);
        let horizontal = r.required("horizontal", &h_codec);
        let vertical = r.optional("vertical", &v_codec, 4064);
        r.finish(|| {
            Some(MaxDistance {
                horizontal: horizontal?,
                vertical,
            })
        })
    }
    fn encode(&self, _value: &MaxDistance) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct DimensionPaddingCodec;

impl Codec<DimensionPadding> for DimensionPaddingCodec {
    fn decode(&self, input: &Json) -> DataResult<DimensionPadding> {
        let non_neg = validated(IntCodec, |v: &i32| {
            if *v >= 0 {
                Ok(())
            } else {
                Err(format!("padding {v} < 0"))
            }
        });
        if matches!(input, Json::Number(_)) {
            return non_neg
                .decode(input)
                .map(|p| DimensionPadding { bottom: p, top: p });
        }
        let mut r = RecordReader::new(input);
        let bottom = r.lenient("bottom", &non_neg, 0);
        let top = r.lenient("top", &non_neg, 0);
        r.finish(|| Some(DimensionPadding { bottom, top }))
    }
    fn encode(&self, _value: &DimensionPadding) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct RuinedPortalSetupCodec;

impl Codec<RuinedPortalSetup> for RuinedPortalSetupCodec {
    fn decode(&self, input: &Json) -> DataResult<RuinedPortalSetup> {
        let mut r = RecordReader::new(input);
        let placement_s = r.required("placement", &StringCodec);
        let air_pocket_probability = r.required("air_pocket_probability", &FloatCodec);
        let mossiness = r.required("mossiness", &FloatCodec);
        let overgrown = r.required("overgrown", &BoolCodec);
        let vines = r.required("vines", &BoolCodec);
        let can_be_cold = r.required("can_be_cold", &BoolCodec);
        let replace_with_blackstone = r.required("replace_with_blackstone", &BoolCodec);
        let weight = r.required("weight", &FloatCodec);
        r.finish(|| {
            let placement = match placement_s?.as_str() {
                "on_land_surface" => RuinedPortalPlacement::OnLandSurface,
                "partly_buried" => RuinedPortalPlacement::PartlyBuried,
                "on_ocean_floor" => RuinedPortalPlacement::OnOceanFloor,
                "in_mountain" => RuinedPortalPlacement::InMountain,
                "underground" => RuinedPortalPlacement::Underground,
                "in_nether" => RuinedPortalPlacement::InNether,
                _ => return None,
            };
            Some(RuinedPortalSetup {
                placement,
                air_pocket_probability: air_pocket_probability?,
                mossiness: mossiness?,
                overgrown: overgrown?,
                vines: vines?,
                can_be_cold: can_be_cold?,
                replace_with_blackstone: replace_with_blackstone?,
                weight: weight?,
            })
        })
    }
    fn encode(&self, _value: &RuinedPortalSetup) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

fn decode_spawn_overrides(
    input: &Json,
    registry: &'static str,
    owner: &Identifier,
) -> Result<BTreeMap<MobCategory, StructureSpawnOverride>, StructureLoadError> {
    let Some(obj) = input.as_object() else {
        return Err(StructureLoadError::Decode {
            registry,
            id: owner.clone(),
            message: format!(
                "spawn_overrides must be an object, got {}",
                input.type_name()
            ),
        });
    };
    let mut out = BTreeMap::new();
    for (k, v) in obj.iter() {
        let Some(ks) = k.as_str() else {
            return Err(StructureLoadError::Decode {
                registry,
                id: owner.clone(),
                message: "spawn_overrides key must be UTF-8".to_owned(),
            });
        };
        let cat = MobCategory::from_name(ks).ok_or_else(|| StructureLoadError::Decode {
            registry,
            id: owner.clone(),
            message: format!("unknown MobCategory '{ks}'"),
        })?;
        let Some(ov) = v.as_object() else {
            return Err(StructureLoadError::Decode {
                registry,
                id: owner.clone(),
                message: format!("spawn override '{ks}' must be an object"),
            });
        };
        let bb = match ov.get("bounding_box").and_then(Json::as_str) {
            Some("piece") => SpawnBoundingBox::Piece,
            Some("full") => SpawnBoundingBox::Full,
            other => {
                return Err(StructureLoadError::Decode {
                    registry,
                    id: owner.clone(),
                    message: format!("invalid spawn bounding_box {other:?}"),
                });
            }
        };
        let mut spawns = Vec::new();
        if let Some(arr) = ov.get("spawns").and_then(Json::as_array) {
            for item in arr {
                let Some(io) = item.as_object() else {
                    continue;
                };
                // 26.3 WeightedList<SpawnerData>: either flat `{type, weight, minCount, maxCount}`
                // or `{data: {type, minCount, maxCount}, weight}`.
                let data_obj = io.get("data").and_then(Json::as_object).unwrap_or(io);
                let entity_type = data_obj
                    .get("type")
                    .and_then(Json::as_str)
                    .and_then(|s| Identifier::parse(s).ok())
                    .ok_or_else(|| StructureLoadError::Decode {
                        registry,
                        id: owner.clone(),
                        message: "invalid spawner entry entity type".to_owned(),
                    })?;
                let weight = io
                    .get("weight")
                    .and_then(|w| IntCodec.decode(w).success())
                    .unwrap_or(1);
                let min_count = data_obj
                    .get("minCount")
                    .and_then(|w| IntCodec.decode(w).success())
                    .unwrap_or(1);
                let max_count = data_obj
                    .get("maxCount")
                    .and_then(|w| IntCodec.decode(w).success())
                    .unwrap_or(1);
                spawns.push(SpawnerEntry {
                    entity_type,
                    weight,
                    min_count,
                    max_count,
                });
            }
        }
        out.insert(
            cat,
            StructureSpawnOverride {
                bounding_box: bb,
                spawns,
            },
        );
    }
    Ok(out)
}

fn decode_structure(
    elem_id: &Identifier,
    input: &Json,
    biome_tags: &BTreeMap<Identifier, Vec<Identifier>>,
) -> Result<StructureDef, StructureLoadError> {
    let err = |msg: String| StructureLoadError::Decode {
        registry: STRUCTURE_DIR,
        id: elem_id.clone(),
        message: msg,
    };
    let Some(obj) = input.as_object() else {
        return Err(err(format!(
            "expected structure object, got {}",
            input.type_name()
        )));
    };
    let biomes_val = obj
        .get("biomes")
        .ok_or_else(|| err("missing required 'biomes'".to_owned()))?;
    let biomes = resolve_biome_holder_set(biomes_val, biome_tags, STRUCTURE_DIR, elem_id)?;
    let spawn_val = obj
        .get("spawn_overrides")
        .ok_or_else(|| err("missing required 'spawn_overrides'".to_owned()))?;
    let spawn_overrides = decode_spawn_overrides(spawn_val, STRUCTURE_DIR, elem_id)?;
    let step_str = obj
        .get("step")
        .and_then(Json::as_str)
        .ok_or_else(|| err("missing required 'step'".to_owned()))?;
    let step = GenerationStep::from_name(step_str)
        .ok_or_else(|| err(format!("unknown step '{step_str}'")))?;
    let terrain_adaptation = match obj.get("terrain_adaptation") {
        None | Some(Json::Null) => TerrainAdjustment::None,
        Some(v) => match v.as_str() {
            Some("none") => TerrainAdjustment::None,
            Some("bury") => TerrainAdjustment::Bury,
            Some("beard_thin") => TerrainAdjustment::BeardThin,
            Some("beard_box") => TerrainAdjustment::BeardBox,
            Some("encapsulate") => TerrainAdjustment::Encapsulate,
            other => return Err(err(format!("unknown terrain_adaptation {other:?}"))),
        },
    };
    let settings = StructureSettings {
        biomes,
        spawn_overrides,
        step,
        terrain_adaptation,
    };

    let ty_str = obj
        .get("type")
        .and_then(Json::as_str)
        .ok_or_else(|| err("missing required 'type'".to_owned()))?;
    let ty = Identifier::parse(ty_str).map_err(|e| err(format!("invalid type '{ty_str}': {e}")))?;
    if ty.namespace() != "minecraft" {
        return Err(err(format!("unsupported structure type '{ty}'")));
    }

    let kind = match ty.path() {
        "jigsaw" => {
            let mut r = RecordReader::new(input);
            let start_pool = r.required("start_pool", &TemplatePoolRefCodec);
            let start_jigsaw_name = r.optional_opt("start_jigsaw_name", &IdentifierCodec);
            let size = r.required(
                "size",
                &validated(IntCodec, |v: &i32| {
                    if (0..=20).contains(v) {
                        Ok(())
                    } else {
                        Err(format!("size {v} outside [0, 20]"))
                    }
                }),
            );
            let start_height = r.required("start_height", &HeightProviderCodec);
            let use_expansion_hack = r.required("use_expansion_hack", &BoolCodec);
            let project_start_to_heightmap =
                r.optional_opt("project_start_to_heightmap", &HeightmapKindCodec);
            let max_distance_from_center =
                r.required("max_distance_from_center", &MaxDistanceCodec);
            let pool_aliases = r.optional("pool_aliases", &list(PoolAliasBindingCodec), Vec::new());
            let dimension_padding = r.optional(
                "dimension_padding",
                &DimensionPaddingCodec,
                DimensionPadding::ZERO,
            );
            let liquid_settings = r.optional(
                "liquid_settings",
                &LiquidSettingsCodec,
                LiquidSettings::ApplyWaterlogging,
            );
            let cfg = r
                .finish(|| {
                    Some(JigsawConfig {
                        start_pool: start_pool?,
                        start_jigsaw_name,
                        size: size?,
                        start_height: start_height?,
                        use_expansion_hack: use_expansion_hack?,
                        project_start_to_heightmap,
                        max_distance_from_center: max_distance_from_center?,
                        pool_aliases,
                        dimension_padding,
                        liquid_settings,
                    })
                })
                .demand()
                .map_err(|e| err(e.message))?;
            StructureKind::Jigsaw(Box::new(cfg))
        }
        "mineshaft" => {
            let mt = obj
                .get("mineshaft_type")
                .and_then(Json::as_str)
                .ok_or_else(|| err("missing 'mineshaft_type'".to_owned()))?;
            match mt {
                "normal" => StructureKind::Mineshaft(MineshaftType::Normal),
                "mesa" => StructureKind::Mineshaft(MineshaftType::Mesa),
                other => return Err(err(format!("unknown mineshaft_type '{other}'"))),
            }
        }
        "stronghold" => StructureKind::Stronghold,
        "fortress" => StructureKind::Fortress,
        "ocean_monument" => StructureKind::OceanMonument,
        "woodland_mansion" => StructureKind::WoodlandMansion,
        "desert_pyramid" => StructureKind::DesertPyramid,
        "jungle_temple" => StructureKind::JungleTemple,
        "swamp_hut" => StructureKind::SwampHut,
        "igloo" => StructureKind::Igloo,
        "shipwreck" => {
            let mut r = RecordReader::new(input);
            let is_beached = r.required("is_beached", &BoolCodec);
            let is_beached = r
                .finish(|| is_beached)
                .demand()
                .map_err(|e| err(e.message))?;
            StructureKind::Shipwreck { is_beached }
        }
        "ocean_ruin" => {
            let mut r = RecordReader::new(input);
            let temp_s = r.required("biome_temp", &StringCodec);
            let large_probability = r.required("large_probability", &FloatCodec);
            let cluster_probability = r.required("cluster_probability", &FloatCodec);
            r.finish(|| {
                let biome_temp = match temp_s?.as_str() {
                    "warm" => OceanRuinTemp::Warm,
                    "cold" => OceanRuinTemp::Cold,
                    _ => return None,
                };
                Some(StructureKind::OceanRuin {
                    biome_temp,
                    large_probability: large_probability?,
                    cluster_probability: cluster_probability?,
                })
            })
            .demand()
            .map_err(|e| err(e.message))?
        }
        "buried_treasure" => StructureKind::BuriedTreasure,
        "end_city" => StructureKind::EndCity,
        "nether_fossil" => {
            let mut r = RecordReader::new(input);
            let height = r.required("height", &HeightProviderCodec);
            let height = r.finish(|| height).demand().map_err(|e| err(e.message))?;
            StructureKind::NetherFossil { height }
        }
        "ruined_portal" => {
            let mut r = RecordReader::new(input);
            let setups = r.required("setups", &list(RuinedPortalSetupCodec));
            let setups = r.finish(|| setups).demand().map_err(|e| err(e.message))?;
            StructureKind::RuinedPortal { setups }
        }
        other => return Err(err(format!("unknown structure type 'minecraft:{other}'"))),
    };

    Ok(StructureDef {
        id: elem_id.clone(),
        settings,
        kind,
    })
}

struct FreqMethodCodec;
impl Codec<FrequencyReductionMethod> for FreqMethodCodec {
    fn decode(&self, input: &Json) -> DataResult<FrequencyReductionMethod> {
        match input.as_str() {
            Some("default") => DataResult::Success(FrequencyReductionMethod::Default),
            Some("legacy_type_1") => DataResult::Success(FrequencyReductionMethod::LegacyType1),
            Some("legacy_type_2") => DataResult::Success(FrequencyReductionMethod::LegacyType2),
            Some("legacy_type_3") => DataResult::Success(FrequencyReductionMethod::LegacyType3),
            other => DataResult::error(format!("Unknown frequency_reduction_method {other:?}")),
        }
    }
    fn encode(&self, _value: &FrequencyReductionMethod) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct SpreadTypeCodec;
impl Codec<RandomSpreadType> for SpreadTypeCodec {
    fn decode(&self, input: &Json) -> DataResult<RandomSpreadType> {
        match input.as_str() {
            Some("linear") => DataResult::Success(RandomSpreadType::Linear),
            Some("triangular") => DataResult::Success(RandomSpreadType::Triangular),
            other => DataResult::error(format!("Unknown spread_type {other:?}")),
        }
    }
    fn encode(&self, _value: &RandomSpreadType) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct Vec3iOffsetCodec;
impl Codec<BlockPos> for Vec3iOffsetCodec {
    fn decode(&self, input: &Json) -> DataResult<BlockPos> {
        list(IntCodec).decode(input).and_then(|v| {
            if v.len() == 3 && v.iter().all(|&c| (-16..=16).contains(&c)) {
                DataResult::Success(BlockPos::new(v[0], v[1], v[2]))
            } else {
                DataResult::error("locate_offset must be [x, y, z] in [-16, 16]")
            }
        })
    }
    fn encode(&self, _value: &BlockPos) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

struct ExclusionZoneCodec;
impl Codec<ExclusionZone> for ExclusionZoneCodec {
    fn decode(&self, input: &Json) -> DataResult<ExclusionZone> {
        let mut r = RecordReader::new(input);
        let other_set = r.required("other_set", &IdentifierCodec);
        let chunk_count = r.required(
            "chunk_count",
            &validated(IntCodec, |v: &i32| {
                if (1..=16).contains(v) {
                    Ok(())
                } else {
                    Err(format!("chunk_count {v} outside [1, 16]"))
                }
            }),
        );
        r.finish(|| {
            Some(ExclusionZone {
                other_set: other_set?,
                chunk_count: chunk_count?,
            })
        })
    }
    fn encode(&self, _value: &ExclusionZone) -> DataResult<Json> {
        DataResult::error("encoding not implemented")
    }
}

fn decode_spreading_common(input: &Json) -> DataResult<SpreadingPlacement> {
    let mut r = RecordReader::new(input);
    let locate_offset = r.optional("locate_offset", &Vec3iOffsetCodec, BlockPos::ZERO);
    let frequency_reduction_method = r.optional(
        "frequency_reduction_method",
        &FreqMethodCodec,
        FrequencyReductionMethod::Default,
    );
    let frequency = r.optional("frequency", &FloatCodec, 1.0_f32);
    let salt = r.required("salt", &IntCodec);
    let exclusion_zone = r.optional_opt("exclusion_zone", &ExclusionZoneCodec);
    r.finish(|| {
        Some(SpreadingPlacement {
            locate_offset,
            frequency_reduction_method,
            frequency,
            salt: salt?,
            exclusion_zone,
        })
    })
}

fn decode_structure_set(
    elem_id: &Identifier,
    input: &Json,
    biome_tags: &BTreeMap<Identifier, Vec<Identifier>>,
) -> Result<StructureSet, StructureLoadError> {
    let err = |msg: String| StructureLoadError::Decode {
        registry: STRUCTURE_SET_DIR,
        id: elem_id.clone(),
        message: msg,
    };
    let Some(obj) = input.as_object() else {
        return Err(err(format!(
            "expected structure_set object, got {}",
            input.type_name()
        )));
    };
    let structures_arr = obj
        .get("structures")
        .and_then(Json::as_array)
        .ok_or_else(|| err("missing required 'structures' array".to_owned()))?;
    let mut structures = Vec::with_capacity(structures_arr.len());
    for entry in structures_arr {
        let Some(eo) = entry.as_object() else {
            return Err(err("structure entry must be an object".to_owned()));
        };
        let s_val = eo
            .get("structure")
            .ok_or_else(|| err("missing 'structure' in entry".to_owned()))?;
        let structure = if let Some(s) = s_val.as_str() {
            let id = Identifier::parse(s)
                .map_err(|e| err(format!("invalid structure id '{s}': {e}")))?;
            StructureRef::Reference(id)
        } else {
            let inline = decode_structure(elem_id, s_val, biome_tags)?;
            StructureRef::Inline(Box::new(inline))
        };
        let weight = eo
            .get("weight")
            .ok_or_else(|| err("missing 'weight' in entry".to_owned()))
            .and_then(|w| IntCodec.decode(w).demand().map_err(|e| err(e.message)))?;
        structures.push(StructureSelectionEntry { structure, weight });
    }

    let placement_val = obj
        .get("placement")
        .ok_or_else(|| err("missing required 'placement'".to_owned()))?;
    let Some(pobj) = placement_val.as_object() else {
        return Err(err("placement must be an object".to_owned()));
    };
    let ty_str = pobj
        .get("type")
        .and_then(Json::as_str)
        .ok_or_else(|| err("missing 'type' in placement".to_owned()))?;
    let ty = Identifier::parse(ty_str)
        .map_err(|e| err(format!("invalid placement type '{ty_str}': {e}")))?;
    if ty.namespace() != "minecraft" {
        return Err(err(format!("unsupported placement type '{ty}'")));
    }

    let placement = match ty.path() {
        "random_spread" => {
            let common = decode_spreading_common(placement_val)
                .demand()
                .map_err(|e| err(e.message))?;
            let mut r = RecordReader::new(placement_val);
            let spacing = r.required("spacing", &IntCodec);
            let separation = r.required("separation", &IntCodec);
            let spread_type = r.optional("spread_type", &SpreadTypeCodec, RandomSpreadType::Linear);
            let (spacing, separation, spread_type) = r
                .finish(|| Some((spacing?, separation?, spread_type)))
                .demand()
                .map_err(|e| err(e.message))?;
            if spacing <= separation {
                return Err(err(format!(
                    "spacing ({spacing}) must be > separation ({separation})"
                )));
            }
            StructurePlacement::RandomSpread {
                common,
                spacing,
                separation,
                spread_type,
            }
        }
        "concentric_rings" => {
            let common = decode_spreading_common(placement_val)
                .demand()
                .map_err(|e| err(e.message))?;
            let mut r = RecordReader::new(placement_val);
            let distance = r.required("distance", &IntCodec);
            let spread = r.required("spread", &IntCodec);
            let count = r.required("count", &IntCodec);
            let (distance, spread, count) = r
                .finish(|| Some((distance?, spread?, count?)))
                .demand()
                .map_err(|e| err(e.message))?;
            let pref_val = pobj
                .get("preferred_biomes")
                .ok_or_else(|| err("missing 'preferred_biomes'".to_owned()))?;
            let preferred_biomes =
                resolve_biome_holder_set(pref_val, biome_tags, STRUCTURE_SET_DIR, elem_id)?;
            StructurePlacement::ConcentricRings {
                common,
                distance,
                spread,
                count,
                preferred_biomes,
            }
        }
        "dimension_origin" => StructurePlacement::DimensionOrigin,
        other => return Err(err(format!("unknown placement type 'minecraft:{other}'"))),
    };

    Ok(StructureSet {
        id: elem_id.clone(),
        structures,
        placement,
    })
}
