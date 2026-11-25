//! `NoiseRouter`, `NoiseSettings`, `AquiferConfig`, `SpawnTargetPoint`, and `NoiseGeneratorSettings`
//! data definitions (`docs/specs/worldgen/density-functions.md` §5).

use std::collections::BTreeMap;

use zenith_codec::json::Json;
use zenith_registry::Identifier;

use crate::ast::DensityFunction;

/// Minimum allowed Y coordinate (`DimensionType.MIN_Y = -2032`).
pub const DIMENSION_MIN_Y: i32 = -2032;
/// Maximum allowed Y coordinate (`DimensionType.MAX_Y = 2031`).
pub const DIMENSION_MAX_Y: i32 = 2031;
/// Maximum allowed vertical height (`DimensionType.Y_SIZE = 4064`).
pub const DIMENSION_Y_SIZE: i32 = 4064;
/// Quantization multiplier for climate parameters (`10_000.0_f32`).
pub const CLIMATE_QUANTIZATION_FACTOR: f32 = 10_000.0_f32;

/// The 8 routed density-function channels of a 26.3 `NoiseRouter`, in spec order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RouterField {
    /// Climate temperature axis.
    Temperature,
    /// Climate vegetation (humidity) axis.
    Vegetation,
    /// Climate continents (continentalness) axis.
    Continents,
    /// Climate erosion axis (also End outer islands in `minecraft:end`).
    Erosion,
    /// Climate depth axis.
    Depth,
    /// Climate ridges (weirdness) axis.
    Ridges,
    /// Preliminary chunk surface level for `MaterialSystem`.
    ChunkSurfaceLevel,
    /// Final terrain density evaluated over the chunk volume in `doFill`.
    FinalDensity,
}

impl RouterField {
    /// All 8 router fields in exact spec order.
    pub const ALL: [Self; 8] = [
        Self::Temperature,
        Self::Vegetation,
        Self::Continents,
        Self::Erosion,
        Self::Depth,
        Self::Ridges,
        Self::ChunkSurfaceLevel,
        Self::FinalDensity,
    ];

    /// Zero-based index into [`Self::ALL`].
    #[inline]
    pub const fn index(self) -> usize {
        match self {
            Self::Temperature => 0,
            Self::Vegetation => 1,
            Self::Continents => 2,
            Self::Erosion => 3,
            Self::Depth => 4,
            Self::Ridges => 5,
            Self::ChunkSurfaceLevel => 6,
            Self::FinalDensity => 7,
        }
    }

    /// Canonical JSON field name inside `noise_router`.
    #[inline]
    pub const fn field_name(self) -> &'static str {
        match self {
            Self::Temperature => "temperature",
            Self::Vegetation => "vegetation",
            Self::Continents => "continents",
            Self::Erosion => "erosion",
            Self::Depth => "depth",
            Self::Ridges => "ridges",
            Self::ChunkSurfaceLevel => "chunk_surface_level",
            Self::FinalDensity => "final_density",
        }
    }
}

/// Vertical generation bounds (`noise` inside `NoiseGeneratorSettings`, spec §5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NoiseSettings {
    min_y: i32,
    height: i32,
}

impl NoiseSettings {
    /// Creates validated `NoiseSettings`.
    ///
    /// Rules:
    /// - `min_y` in `[-2032, 2031]` and `min_y % 16 == 0`
    /// - `height` in `[0, 4064]` and `height % 16 == 0`
    /// - `min_y + height <= 2032`
    pub fn new(min_y: i32, height: i32) -> Result<Self, String> {
        if !(DIMENSION_MIN_Y..=DIMENSION_MAX_Y).contains(&min_y) {
            return Err(format!(
                "min_y {min_y} out of range [{DIMENSION_MIN_Y}, {DIMENSION_MAX_Y}]"
            ));
        }
        if !(0..=DIMENSION_Y_SIZE).contains(&height) {
            return Err(format!(
                "height {height} out of range [0, {DIMENSION_Y_SIZE}]"
            ));
        }
        if min_y % 16 != 0 {
            return Err(format!("min_y {min_y} must be a multiple of 16"));
        }
        if height % 16 != 0 {
            return Err(format!("height {height} must be a multiple of 16"));
        }
        if i64::from(min_y) + i64::from(height) > i64::from(DIMENSION_MAX_Y) + 1 {
            return Err(format!(
                "min_y + height ({}) exceeds maximum {}",
                i64::from(min_y) + i64::from(height),
                DIMENSION_MAX_Y + 1
            ));
        }
        Ok(Self { min_y, height })
    }

    /// Lowest generated block Y coordinate.
    #[inline]
    pub const fn min_y(self) -> i32 {
        self.min_y
    }

    /// Total vertical generation height in blocks.
    #[inline]
    pub const fn height(self) -> i32 {
        self.height
    }

    /// Clamps vertical bounds against a level height accessor (`min_y`, inclusive `max_y`).
    #[inline]
    pub fn clamp_to_height_accessor(self, accessor_min_y: i32, accessor_max_y: i32) -> (i32, i32) {
        let new_min_y = self.min_y.max(accessor_min_y);
        let new_height = (self.min_y + self.height).min(accessor_max_y + 1) - new_min_y;
        (new_min_y, new_height)
    }
}

/// The 8 routed density functions (`noise_router` in 26.3, spec §5.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoiseRouter {
    /// Climate temperature axis.
    pub temperature: DensityFunction,
    /// Climate vegetation (humidity) axis.
    pub vegetation: DensityFunction,
    /// Climate continents axis.
    pub continents: DensityFunction,
    /// Climate erosion axis.
    pub erosion: DensityFunction,
    /// Climate depth axis.
    pub depth: DensityFunction,
    /// Climate ridges (weirdness) axis.
    pub ridges: DensityFunction,
    /// Preliminary chunk surface level.
    pub chunk_surface_level: DensityFunction,
    /// Final terrain density.
    pub final_density: DensityFunction,
}

impl NoiseRouter {
    /// Returns the density function for `field`.
    #[inline]
    pub fn field(&self, field: RouterField) -> &DensityFunction {
        match field {
            RouterField::Temperature => &self.temperature,
            RouterField::Vegetation => &self.vegetation,
            RouterField::Continents => &self.continents,
            RouterField::Erosion => &self.erosion,
            RouterField::Depth => &self.depth,
            RouterField::Ridges => &self.ridges,
            RouterField::ChunkSurfaceLevel => &self.chunk_surface_level,
            RouterField::FinalDensity => &self.final_density,
        }
    }
}

/// Optional `aquifers` configuration on `NoiseGeneratorSettings` (6 fields in spec order, §5.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AquiferConfig {
    /// Barrier density between neighboring aquifers.
    pub barrier: DensityFunction,
    /// Aquifer cell floodedness density.
    pub fluid_level_floodedness: DensityFunction,
    /// Fluid surface height spread.
    pub fluid_level_spread: DensityFunction,
    /// Deep aquifer lava selector.
    pub lava: DensityFunction,
    /// Aquifer flooding exclusion density (e.g. Deep Dark).
    pub exclusion: DensityFunction,
    /// Preliminary 2D surface level density.
    pub surface_level: DensityFunction,
}

impl AquiferConfig {
    /// Number of density functions in `AquiferConfig`.
    pub const COUNT: usize = 6;

    /// Returns the density function at spec field index `0..6`:
    /// `0 = barrier`, `1 = fluid_level_floodedness`, `2 = fluid_level_spread`,
    /// `3 = lava`, `4 = exclusion`, `5 = surface_level`.
    #[inline]
    pub fn by_index(&self, index: usize) -> Option<&DensityFunction> {
        match index {
            0 => Some(&self.barrier),
            1 => Some(&self.fluid_level_floodedness),
            2 => Some(&self.fluid_level_spread),
            3 => Some(&self.lava),
            4 => Some(&self.exclusion),
            5 => Some(&self.surface_level),
            _ => None,
        }
    }
}

/// Decoded JSON `BlockState` (`{"Name": "<block_id>", "Properties"?: { ... }}`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockStateSpec {
    /// Block identifier (e.g., `minecraft:stone`, `minecraft:water`).
    pub name: Identifier,
    /// Optional block-state property map (sorted by property name).
    pub properties: BTreeMap<String, String>,
}

/// Raw `Holder<MaterialRule>` stored on `NoiseGeneratorSettings` (internals decoded in a later milestone).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaterialRuleRef {
    /// Named reference into `minecraft:worldgen/material_rule`.
    Reference(Identifier),
    /// Inline `MaterialRule` JSON object preserved verbatim.
    Inline(Json),
}

/// Closed quantized climate parameter interval `[min, max]` (`i64`, quantized by `(i64)(v * 10000.0f)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClimateParameter {
    /// Inclusive lower bound (quantized).
    pub min: i64,
    /// Inclusive upper bound (quantized).
    pub max: i64,
}

impl ClimateParameter {
    /// Quantizes `v: f32` via `(v * 10000.0_f32) as i64` (truncation toward zero).
    #[inline]
    pub fn quantize(v: f32) -> i64 {
        (v * CLIMATE_QUANTIZATION_FACTOR) as i64
    }

    /// Distance from `[self.min, self.max]` to quantized `target` (`0` if inside).
    #[inline]
    pub const fn distance(self, target: i64) -> i64 {
        let above = target.wrapping_sub(self.max);
        let below = self.min.wrapping_sub(target);
        if above > 0 {
            above
        } else if below > 0 {
            below
        } else {
            0
        }
    }
}

/// One entry in `NoiseGeneratorSettings.spawn_target` (spec §5.3).
///
/// Maps registered `minecraft:worldgen/density_function` identifiers to target `ClimateParameter`
/// intervals (preserving JSON insertion order).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnTargetPoint {
    /// Ordered `(density_function_id, parameter)` entries.
    pub entries: Vec<(Identifier, ClimateParameter)>,
}

/// One entry in `NoiseGeneratorSettings.debug_functions` (F3 debug readout).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugFunctionEntry {
    /// Display label.
    pub label: String,
    /// Density function.
    pub function: DensityFunction,
}

/// Decoded `minecraft:worldgen/noise_settings` registry entry (`density-functions.md` §5.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoiseGeneratorSettings {
    noise: NoiseSettings,
    default_block: BlockStateSpec,
    default_fluid: BlockStateSpec,
    noise_router: NoiseRouter,
    material_rule: MaterialRuleRef,
    spawn_target: Vec<SpawnTargetPoint>,
    sea_level: i32,
    disable_mob_generation: bool,
    aquifers: Option<AquiferConfig>,
    legacy_random_source: bool,
    debug_functions: Vec<DebugFunctionEntry>,
}

impl NoiseGeneratorSettings {
    /// Constructs a `NoiseGeneratorSettings` from its decoded fields.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        noise: NoiseSettings,
        default_block: BlockStateSpec,
        default_fluid: BlockStateSpec,
        noise_router: NoiseRouter,
        material_rule: MaterialRuleRef,
        spawn_target: Vec<SpawnTargetPoint>,
        sea_level: i32,
        disable_mob_generation: bool,
        aquifers: Option<AquiferConfig>,
        legacy_random_source: bool,
        debug_functions: Vec<DebugFunctionEntry>,
    ) -> Self {
        Self {
            noise,
            default_block,
            default_fluid,
            noise_router,
            material_rule,
            spawn_target,
            sea_level,
            disable_mob_generation,
            aquifers,
            legacy_random_source,
            debug_functions,
        }
    }

    /// Vertical generation settings (`noise`).
    #[inline]
    pub const fn noise_settings(&self) -> NoiseSettings {
        self.noise
    }

    /// Minimum generated block Y (`noise.min_y`).
    #[inline]
    pub const fn min_y(&self) -> i32 {
        self.noise.min_y()
    }

    /// Vertical generation height in blocks (`noise.height`).
    #[inline]
    pub const fn height(&self) -> i32 {
        self.noise.height()
    }

    /// Sea level block Y (`sea_level`).
    #[inline]
    pub const fn sea_level(&self) -> i32 {
        self.sea_level
    }

    /// Default solid block state (`default_block`).
    #[inline]
    pub const fn default_block(&self) -> &BlockStateSpec {
        &self.default_block
    }

    /// Default solid block identifier (`default_block.Name`).
    #[inline]
    pub const fn default_block_id(&self) -> &Identifier {
        &self.default_block.name
    }

    /// Default sea-level fluid block state (`default_fluid`).
    #[inline]
    pub const fn default_fluid(&self) -> &BlockStateSpec {
        &self.default_fluid
    }

    /// Default sea-level fluid block identifier (`default_fluid.Name`).
    #[inline]
    pub const fn default_fluid_id(&self) -> &Identifier {
        &self.default_fluid.name
    }

    /// The 8 routed density functions (`noise_router`).
    #[inline]
    pub const fn noise_router(&self) -> &NoiseRouter {
        &self.noise_router
    }

    /// Optional aquifer configuration (`aquifers`; `None` means disabled aquifer).
    #[inline]
    pub const fn aquifers(&self) -> Option<&AquiferConfig> {
        self.aquifers.as_ref()
    }

    /// Raw material rule reference or inline JSON (`material_rule`).
    #[inline]
    pub const fn material_rule(&self) -> &MaterialRuleRef {
        &self.material_rule
    }

    /// Spawn target parameter boxes (`spawn_target`).
    #[inline]
    pub fn spawn_target(&self) -> &[SpawnTargetPoint] {
        &self.spawn_target
    }

    /// Whether initial chunk mob generation is disabled (`disable_mob_generation`).
    #[inline]
    pub const fn disable_mob_generation(&self) -> bool {
        self.disable_mob_generation
    }

    /// Whether to use the legacy 48-bit LCG random source (`legacy_random_source`).
    #[inline]
    pub const fn legacy_random_source(&self) -> bool {
        self.legacy_random_source
    }

    /// F3 debug screen density function entries (`debug_functions`).
    #[inline]
    pub fn debug_functions(&self) -> &[DebugFunctionEntry] {
        &self.debug_functions
    }
}
