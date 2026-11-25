//! Pack loading and resolution of `worldgen/noise`, `worldgen/density_function`, and
//! `worldgen/noise_settings` registries (`WorldgenData::load`).

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use thiserror::Error;
use zenith_codec::format::PackType;
use zenith_codec::json::{Json, ParseMode, parse_bytes};
use zenith_codec::resources::{PackResources, ResourceError};
use zenith_math::NormalNoiseParameters;
use zenith_registry::Identifier;

use crate::ast::{
    DensityFunction, NoiseHolder, RawCubicSpline, RawDensityFunction, RawNoiseHolder,
};
use crate::codec::{
    RawAquiferConfig, RawDebugFunctionEntry, RawNoiseGeneratorSettings, RawNoiseRouter,
    decode_normal_noise_params, decode_raw_density_function, decode_raw_noise_generator_settings,
};
use crate::settings::{AquiferConfig, DebugFunctionEntry, NoiseGeneratorSettings, NoiseRouter};
use crate::spline::CubicSpline;

const NOISE_DIR: &str = "worldgen/noise";
const DENSITY_FUNCTION_DIR: &str = "worldgen/density_function";
const NOISE_SETTINGS_DIR: &str = "worldgen/noise_settings";

/// Error returned when loading worldgen registries from a data pack or decoding JSON.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LoadError {
    /// Listing files in the pack failed.
    #[error("pack listing error: {0}")]
    Resource(#[from] ResourceError),
    /// A listed resource file could not be read from the pack.
    #[error("missing resource {id} in pack")]
    MissingResource {
        /// File identifier that failed to read.
        id: Identifier,
    },
    /// Strict JSON parsing failed.
    #[error("JSON parse error in {registry} entry {id}: {message}")]
    JsonParse {
        /// Registry directory (e.g. `worldgen/density_function`).
        registry: &'static str,
        /// Element identifier.
        id: Identifier,
        /// Error detail.
        message: String,
    },
    /// Codec validation or schema decoding failed.
    #[error("codec decode error in {registry} entry {id}: {message}")]
    Decode {
        /// Registry directory.
        registry: &'static str,
        /// Element identifier.
        id: Identifier,
        /// Error detail.
        message: String,
    },
    /// A density function referenced an unknown `worldgen/noise` identifier.
    #[error("unknown noise reference '{target}' in {registry} entry {owner}")]
    UnknownNoise {
        /// Registry containing the broken reference.
        registry: &'static str,
        /// Element identifier containing the broken reference.
        owner: Identifier,
        /// Unresolved noise identifier.
        target: Identifier,
    },
    /// A density function or spawn target referenced an unknown `worldgen/density_function` identifier.
    #[error("unknown density_function reference '{target}' in {registry} entry {owner}")]
    UnknownDensityFunction {
        /// Registry containing the broken reference.
        registry: &'static str,
        /// Element identifier containing the broken reference.
        owner: Identifier,
        /// Unresolved density function identifier.
        target: Identifier,
    },
    /// A cycle was detected in `worldgen/density_function` references.
    #[error("cyclic density_function reference involving '{id}'")]
    DensityFunctionCycle {
        /// Identifier participating in the cycle.
        id: Identifier,
    },
}

/// Decoded worldgen registries (`worldgen/noise`, `worldgen/density_function`,
/// `worldgen/noise_settings`) loaded from a data pack.
#[derive(Debug, Clone)]
pub struct WorldgenData {
    noises: BTreeMap<Identifier, Arc<NormalNoiseParameters>>,
    density_functions: BTreeMap<Identifier, Arc<DensityFunction>>,
    noise_settings: BTreeMap<Identifier, NoiseGeneratorSettings>,
}

impl WorldgenData {
    /// Loads and validates `worldgen/noise`, `worldgen/density_function`, and
    /// `worldgen/noise_settings` from `pack`.
    ///
    /// Follows `docs/specs/registry/identifiers-registries-tags.md` §2.4 ordering (sorting
    /// discovered `.json` files by full file identifier) and fails the load if any element
    /// fails JSON parsing, codec validation, or reference resolution.
    pub fn load(pack: &dyn PackResources) -> Result<Self, LoadError> {
        // 1. Load worldgen/noise
        let noise_files = list_json_files(pack, NOISE_DIR)?;
        let mut noises = BTreeMap::new();
        for (elem_id, file_id) in noise_files {
            let json = read_json_file(pack, NOISE_DIR, &elem_id, &file_id)?;
            let params =
                decode_normal_noise_params(&json)
                    .demand()
                    .map_err(|e| LoadError::Decode {
                        registry: NOISE_DIR,
                        id: elem_id.clone(),
                        message: e.message,
                    })?;
            noises.insert(elem_id, Arc::new(params));
        }

        // 2. Parse worldgen/density_function into raw ASTs
        let df_files = list_json_files(pack, DENSITY_FUNCTION_DIR)?;
        let mut raw_dfs: BTreeMap<Identifier, RawDensityFunction> = BTreeMap::new();
        for (elem_id, file_id) in df_files {
            let json = read_json_file(pack, DENSITY_FUNCTION_DIR, &elem_id, &file_id)?;
            let raw =
                decode_raw_density_function(&json)
                    .demand()
                    .map_err(|e| LoadError::Decode {
                        registry: DENSITY_FUNCTION_DIR,
                        id: elem_id.clone(),
                        message: e.message,
                    })?;
            raw_dfs.insert(elem_id, raw);
        }

        // Resolve all density functions (with cycle detection)
        let mut resolved_dfs: BTreeMap<Identifier, Arc<DensityFunction>> = BTreeMap::new();
        let mut state: HashMap<Identifier, VisitState> = HashMap::with_capacity(raw_dfs.len());
        let df_keys: Vec<Identifier> = raw_dfs.keys().cloned().collect();
        for id in df_keys {
            resolve_registry_df(&id, &noises, &raw_dfs, &mut state, &mut resolved_dfs)?;
        }

        // 3. Load and resolve worldgen/noise_settings
        let ns_files = list_json_files(pack, NOISE_SETTINGS_DIR)?;
        let mut noise_settings = BTreeMap::new();
        for (elem_id, file_id) in ns_files {
            let json = read_json_file(pack, NOISE_SETTINGS_DIR, &elem_id, &file_id)?;
            let raw = decode_raw_noise_generator_settings(&json)
                .demand()
                .map_err(|e| LoadError::Decode {
                    registry: NOISE_SETTINGS_DIR,
                    id: elem_id.clone(),
                    message: e.message,
                })?;
            let resolved = resolve_noise_settings(raw, &elem_id, &noises, &resolved_dfs)?;
            noise_settings.insert(elem_id, resolved);
        }

        Ok(Self {
            noises,
            density_functions: resolved_dfs,
            noise_settings,
        })
    }

    /// Creates a `WorldgenData` directly from pre-built maps (primarily for unit tests).
    pub fn from_maps(
        noises: BTreeMap<Identifier, Arc<NormalNoiseParameters>>,
        density_functions: BTreeMap<Identifier, Arc<DensityFunction>>,
        noise_settings: BTreeMap<Identifier, NoiseGeneratorSettings>,
    ) -> Self {
        Self {
            noises,
            density_functions,
            noise_settings,
        }
    }

    /// Looks up a registered `worldgen/noise` entry.
    #[inline]
    pub fn noise(&self, id: &Identifier) -> Option<&Arc<NormalNoiseParameters>> {
        self.noises.get(id)
    }

    /// All loaded `worldgen/noise` entries.
    #[inline]
    pub fn noises(&self) -> &BTreeMap<Identifier, Arc<NormalNoiseParameters>> {
        &self.noises
    }

    /// Looks up a registered `worldgen/density_function` entry.
    #[inline]
    pub fn density_function(&self, id: &Identifier) -> Option<&Arc<DensityFunction>> {
        self.density_functions.get(id)
    }

    /// All loaded `worldgen/density_function` entries.
    #[inline]
    pub fn density_functions(&self) -> &BTreeMap<Identifier, Arc<DensityFunction>> {
        &self.density_functions
    }

    /// Looks up a registered `worldgen/noise_settings` entry.
    #[inline]
    pub fn noise_settings(&self, id: &Identifier) -> Option<&NoiseGeneratorSettings> {
        self.noise_settings.get(id)
    }

    /// All loaded `worldgen/noise_settings` entries.
    #[inline]
    pub fn all_noise_settings(&self) -> &BTreeMap<Identifier, NoiseGeneratorSettings> {
        &self.noise_settings
    }

    /// Decodes a standalone `DensityFunction` from JSON, resolving any references against this
    /// `WorldgenData`'s loaded noise and density-function registries.
    pub fn decode_density_function(&self, json: &Json) -> Result<DensityFunction, LoadError> {
        let owner = Identifier::with_default_namespace("inline").expect("valid static identifier");
        let raw = decode_raw_density_function(json)
            .demand()
            .map_err(|e| LoadError::Decode {
                registry: DENSITY_FUNCTION_DIR,
                id: owner.clone(),
                message: e.message,
            })?;
        resolve_df_against_resolved(
            &raw,
            DENSITY_FUNCTION_DIR,
            &owner,
            &self.noises,
            &self.density_functions,
        )
    }
}

/// Decodes a standalone `DensityFunction` from JSON text with no external registries.
pub fn decode_density_function_str(json_text: &str) -> Result<DensityFunction, LoadError> {
    let owner = Identifier::with_default_namespace("inline").expect("valid static identifier");
    let json = zenith_codec::json::parse(json_text, ParseMode::Data).map_err(|e| {
        LoadError::JsonParse {
            registry: DENSITY_FUNCTION_DIR,
            id: owner.clone(),
            message: e.to_string(),
        }
    })?;
    let empty = WorldgenData::from_maps(BTreeMap::new(), BTreeMap::new(), BTreeMap::new());
    empty.decode_density_function(&json)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VisitState {
    Visiting,
    Done,
}

fn list_json_files(
    pack: &dyn PackResources,
    dir: &str,
) -> Result<Vec<(Identifier, Identifier)>, LoadError> {
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
    // Sort by full file identifier (path first, then namespace) per spec §2.4.
    files.sort();
    let mut out = Vec::with_capacity(files.len());
    for file_id in files {
        let rel = file_id
            .path()
            .strip_prefix(&prefix)
            .and_then(|s| s.strip_suffix(".json"))
            .expect("filtered above");
        let elem_id =
            Identifier::from_parts(file_id.namespace(), rel).map_err(|e| LoadError::Decode {
                registry: "worldgen",
                id: file_id.clone(),
                message: e.to_string(),
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
) -> Result<Json, LoadError> {
    let bytes = pack
        .get(PackType::Data, file_id)
        .ok_or_else(|| LoadError::MissingResource {
            id: file_id.clone(),
        })?;
    parse_bytes(&bytes, ParseMode::Data).map_err(|e| LoadError::JsonParse {
        registry,
        id: elem_id.clone(),
        message: e.to_string(),
    })
}

fn resolve_noise_holder(
    raw: &RawNoiseHolder,
    registry: &'static str,
    owner: &Identifier,
    noises: &BTreeMap<Identifier, Arc<NormalNoiseParameters>>,
) -> Result<NoiseHolder, LoadError> {
    match raw {
        RawNoiseHolder::Reference(id) => {
            let params = noises.get(id).ok_or_else(|| LoadError::UnknownNoise {
                registry,
                owner: owner.clone(),
                target: id.clone(),
            })?;
            Ok(NoiseHolder::Reference {
                id: id.clone(),
                params: Arc::clone(params),
            })
        }
        RawNoiseHolder::Inline(params) => Ok(NoiseHolder::Inline(Arc::new(params.clone()))),
    }
}

fn resolve_registry_df(
    id: &Identifier,
    noises: &BTreeMap<Identifier, Arc<NormalNoiseParameters>>,
    raw_dfs: &BTreeMap<Identifier, RawDensityFunction>,
    state: &mut HashMap<Identifier, VisitState>,
    resolved: &mut BTreeMap<Identifier, Arc<DensityFunction>>,
) -> Result<Arc<DensityFunction>, LoadError> {
    if let Some(existing) = resolved.get(id) {
        return Ok(Arc::clone(existing));
    }
    if state.get(id) == Some(&VisitState::Visiting) {
        return Err(LoadError::DensityFunctionCycle { id: id.clone() });
    }
    let raw = raw_dfs
        .get(id)
        .ok_or_else(|| LoadError::UnknownDensityFunction {
            registry: DENSITY_FUNCTION_DIR,
            owner: id.clone(),
            target: id.clone(),
        })?;

    state.insert(id.clone(), VisitState::Visiting);
    let df = resolve_raw_df_lazy(raw, id, noises, raw_dfs, state, resolved)?;
    state.insert(id.clone(), VisitState::Done);
    let arc = Arc::new(df);
    resolved.insert(id.clone(), Arc::clone(&arc));
    Ok(arc)
}

fn resolve_raw_df_lazy(
    raw: &RawDensityFunction,
    owner: &Identifier,
    noises: &BTreeMap<Identifier, Arc<NormalNoiseParameters>>,
    raw_dfs: &BTreeMap<Identifier, RawDensityFunction>,
    state: &mut HashMap<Identifier, VisitState>,
    resolved: &mut BTreeMap<Identifier, Arc<DensityFunction>>,
) -> Result<DensityFunction, LoadError> {
    let mut resolve_ref = |target_id: &Identifier| -> Result<Arc<DensityFunction>, LoadError> {
        if !raw_dfs.contains_key(target_id) {
            return Err(LoadError::UnknownDensityFunction {
                registry: DENSITY_FUNCTION_DIR,
                owner: owner.clone(),
                target: target_id.clone(),
            });
        }
        resolve_registry_df(target_id, noises, raw_dfs, state, resolved)
    };
    resolve_raw_df_with(raw, DENSITY_FUNCTION_DIR, owner, noises, &mut resolve_ref)
}

fn resolve_df_against_resolved(
    raw: &RawDensityFunction,
    registry: &'static str,
    owner: &Identifier,
    noises: &BTreeMap<Identifier, Arc<NormalNoiseParameters>>,
    resolved: &BTreeMap<Identifier, Arc<DensityFunction>>,
) -> Result<DensityFunction, LoadError> {
    let mut resolve_ref = |target_id: &Identifier| -> Result<Arc<DensityFunction>, LoadError> {
        resolved
            .get(target_id)
            .cloned()
            .ok_or_else(|| LoadError::UnknownDensityFunction {
                registry,
                owner: owner.clone(),
                target: target_id.clone(),
            })
    };
    resolve_raw_df_with(raw, registry, owner, noises, &mut resolve_ref)
}

fn resolve_raw_df_with(
    raw: &RawDensityFunction,
    registry: &'static str,
    owner: &Identifier,
    noises: &BTreeMap<Identifier, Arc<NormalNoiseParameters>>,
    resolve_ref: &mut impl FnMut(&Identifier) -> Result<Arc<DensityFunction>, LoadError>,
) -> Result<DensityFunction, LoadError> {
    Ok(match raw {
        RawDensityFunction::Reference(target_id) => {
            let target = resolve_ref(target_id)?;
            DensityFunction::Reference {
                id: target_id.clone(),
                target,
            }
        }
        RawDensityFunction::Constant(v) => DensityFunction::Constant(*v),
        RawDensityFunction::BlendAlpha => DensityFunction::BlendAlpha,
        RawDensityFunction::BlendOffset => DensityFunction::BlendOffset,
        RawDensityFunction::Beardifier => DensityFunction::Beardifier,
        RawDensityFunction::Noise {
            noise,
            xz_scale,
            y_scale,
            shift_x,
            shift_y,
            shift_z,
        } => DensityFunction::Noise {
            noise: resolve_noise_holder(noise, registry, owner, noises)?,
            xz_scale: *xz_scale,
            y_scale: *y_scale,
            shift_x: Box::new(resolve_raw_df_with(
                shift_x,
                registry,
                owner,
                noises,
                resolve_ref,
            )?),
            shift_y: Box::new(resolve_raw_df_with(
                shift_y,
                registry,
                owner,
                noises,
                resolve_ref,
            )?),
            shift_z: Box::new(resolve_raw_df_with(
                shift_z,
                registry,
                owner,
                noises,
                resolve_ref,
            )?),
        },
        RawDensityFunction::Shift { kind, noise } => DensityFunction::Shift {
            kind: *kind,
            noise: resolve_noise_holder(noise, registry, owner, noises)?,
        },
        RawDensityFunction::EndOuterIslands => DensityFunction::EndOuterIslands,
        RawDensityFunction::DistanceToPoint { point, metric } => DensityFunction::DistanceToPoint {
            point: *point,
            metric: *metric,
        },
        RawDensityFunction::Gradient {
            axis,
            tiling,
            from_coordinate,
            to_coordinate,
            from_value,
            to_value,
        } => DensityFunction::Gradient {
            axis: *axis,
            tiling: *tiling,
            from_coordinate: *from_coordinate,
            to_coordinate: *to_coordinate,
            from_value: *from_value,
            to_value: *to_value,
        },
        RawDensityFunction::OldBlendedNoise(p) => DensityFunction::OldBlendedNoise(*p),
        RawDensityFunction::Unary { op, input } => DensityFunction::Unary {
            op: *op,
            input: Box::new(resolve_raw_df_with(
                input,
                registry,
                owner,
                noises,
                resolve_ref,
            )?),
        },
        RawDensityFunction::Round {
            op,
            input,
            multiple,
        } => DensityFunction::Round {
            op: *op,
            input: Box::new(resolve_raw_df_with(
                input,
                registry,
                owner,
                noises,
                resolve_ref,
            )?),
            multiple: Box::new(resolve_raw_df_with(
                multiple,
                registry,
                owner,
                noises,
                resolve_ref,
            )?),
        },
        RawDensityFunction::Binary { op, left, right } => DensityFunction::Binary {
            op: *op,
            left: Box::new(resolve_raw_df_with(
                left,
                registry,
                owner,
                noises,
                resolve_ref,
            )?),
            right: Box::new(resolve_raw_df_with(
                right,
                registry,
                owner,
                noises,
                resolve_ref,
            )?),
        },
        RawDensityFunction::Pow { base, exponent } => DensityFunction::Pow {
            base: Box::new(resolve_raw_df_with(
                base,
                registry,
                owner,
                noises,
                resolve_ref,
            )?),
            exponent: Box::new(resolve_raw_df_with(
                exponent,
                registry,
                owner,
                noises,
                resolve_ref,
            )?),
        },
        RawDensityFunction::Lerp {
            alpha,
            first,
            second,
        } => DensityFunction::Lerp {
            alpha: Box::new(resolve_raw_df_with(
                alpha,
                registry,
                owner,
                noises,
                resolve_ref,
            )?),
            first: Box::new(resolve_raw_df_with(
                first,
                registry,
                owner,
                noises,
                resolve_ref,
            )?),
            second: Box::new(resolve_raw_df_with(
                second,
                registry,
                owner,
                noises,
                resolve_ref,
            )?),
        },
        RawDensityFunction::Clamp { input, min, max } => DensityFunction::Clamp {
            input: Box::new(resolve_raw_df_with(
                input,
                registry,
                owner,
                noises,
                resolve_ref,
            )?),
            min: *min,
            max: *max,
        },
        RawDensityFunction::RangeChoice {
            input,
            min_inclusive,
            max_exclusive,
            when_in_range,
            when_out_of_range,
        } => DensityFunction::RangeChoice {
            input: Box::new(resolve_raw_df_with(
                input,
                registry,
                owner,
                noises,
                resolve_ref,
            )?),
            min_inclusive: *min_inclusive,
            max_exclusive: *max_exclusive,
            when_in_range: Box::new(resolve_raw_df_with(
                when_in_range,
                registry,
                owner,
                noises,
                resolve_ref,
            )?),
            when_out_of_range: Box::new(resolve_raw_df_with(
                when_out_of_range,
                registry,
                owner,
                noises,
                resolve_ref,
            )?),
        },
        RawDensityFunction::IntervalSelect {
            input,
            thresholds,
            functions,
        } => {
            let resolved_input = Box::new(resolve_raw_df_with(
                input,
                registry,
                owner,
                noises,
                resolve_ref,
            )?);
            let mut resolved_fns = Vec::with_capacity(functions.len());
            for f in functions {
                resolved_fns.push(resolve_raw_df_with(
                    f,
                    registry,
                    owner,
                    noises,
                    resolve_ref,
                )?);
            }
            DensityFunction::IntervalSelect {
                input: resolved_input,
                thresholds: thresholds.clone(),
                functions: resolved_fns,
            }
        }
        RawDensityFunction::Spline(raw_spline) => DensityFunction::Spline(resolve_raw_spline(
            raw_spline,
            registry,
            owner,
            noises,
            resolve_ref,
        )?),
        RawDensityFunction::Cache(input) => DensityFunction::Cache(Box::new(resolve_raw_df_with(
            input,
            registry,
            owner,
            noises,
            resolve_ref,
        )?)),
        RawDensityFunction::BlendDensity(input) => DensityFunction::BlendDensity(Box::new(
            resolve_raw_df_with(input, registry, owner, noises, resolve_ref)?,
        )),
        RawDensityFunction::Interpolated {
            input,
            cell_size_xz,
            cell_size_y,
        } => DensityFunction::Interpolated {
            input: Box::new(resolve_raw_df_with(
                input,
                registry,
                owner,
                noises,
                resolve_ref,
            )?),
            cell_size_xz: *cell_size_xz,
            cell_size_y: *cell_size_y,
        },
        RawDensityFunction::Slice {
            axis,
            coordinate,
            input,
        } => DensityFunction::Slice {
            axis: *axis,
            coordinate: *coordinate,
            input: Box::new(resolve_raw_df_with(
                input,
                registry,
                owner,
                noises,
                resolve_ref,
            )?),
        },
        RawDensityFunction::FindTopSurface {
            density,
            upper_bound,
            lower_bound,
            cell_height,
        } => DensityFunction::FindTopSurface {
            density: Box::new(resolve_raw_df_with(
                density,
                registry,
                owner,
                noises,
                resolve_ref,
            )?),
            upper_bound: Box::new(resolve_raw_df_with(
                upper_bound,
                registry,
                owner,
                noises,
                resolve_ref,
            )?),
            lower_bound: *lower_bound,
            cell_height: *cell_height,
        },
    })
}

fn resolve_raw_spline(
    raw: &RawCubicSpline,
    registry: &'static str,
    owner: &Identifier,
    noises: &BTreeMap<Identifier, Arc<NormalNoiseParameters>>,
    resolve_ref: &mut impl FnMut(&Identifier) -> Result<Arc<DensityFunction>, LoadError>,
) -> Result<CubicSpline<Box<DensityFunction>>, LoadError> {
    match raw {
        RawCubicSpline::Constant(v) => Ok(CubicSpline::constant(*v)),
        RawCubicSpline::Multipoint {
            coordinate,
            locations,
            values,
            derivatives,
        } => {
            let coord = Box::new(resolve_raw_df_with(
                coordinate,
                registry,
                owner,
                noises,
                resolve_ref,
            )?);
            let coord_range = coord.range();
            let mut resolved_values = Vec::with_capacity(values.len());
            for v in values {
                resolved_values.push(resolve_raw_spline(v, registry, owner, noises, resolve_ref)?);
            }
            Ok(CubicSpline::multipoint(
                coord,
                coord_range,
                locations.clone(),
                resolved_values,
                derivatives.clone(),
            ))
        }
    }
}

fn resolve_noise_settings(
    raw: RawNoiseGeneratorSettings,
    owner: &Identifier,
    noises: &BTreeMap<Identifier, Arc<NormalNoiseParameters>>,
    resolved_dfs: &BTreeMap<Identifier, Arc<DensityFunction>>,
) -> Result<NoiseGeneratorSettings, LoadError> {
    let res = |f: &RawDensityFunction| {
        resolve_df_against_resolved(f, NOISE_SETTINGS_DIR, owner, noises, resolved_dfs)
    };

    let RawNoiseRouter {
        temperature,
        vegetation,
        continents,
        erosion,
        depth,
        ridges,
        chunk_surface_level,
        final_density,
    } = raw.noise_router;
    let noise_router = NoiseRouter {
        temperature: res(&temperature)?,
        vegetation: res(&vegetation)?,
        continents: res(&continents)?,
        erosion: res(&erosion)?,
        depth: res(&depth)?,
        ridges: res(&ridges)?,
        chunk_surface_level: res(&chunk_surface_level)?,
        final_density: res(&final_density)?,
    };

    let aquifers = match raw.aquifers {
        Some(RawAquiferConfig {
            barrier,
            fluid_level_floodedness,
            fluid_level_spread,
            lava,
            exclusion,
            surface_level,
        }) => Some(AquiferConfig {
            barrier: res(&barrier)?,
            fluid_level_floodedness: res(&fluid_level_floodedness)?,
            fluid_level_spread: res(&fluid_level_spread)?,
            lava: res(&lava)?,
            exclusion: res(&exclusion)?,
            surface_level: res(&surface_level)?,
        }),
        None => None,
    };

    // Validate spawn_target density_function references exist.
    for pt in &raw.spawn_target {
        for (df_id, _) in &pt.entries {
            if !resolved_dfs.contains_key(df_id) {
                return Err(LoadError::UnknownDensityFunction {
                    registry: NOISE_SETTINGS_DIR,
                    owner: owner.clone(),
                    target: df_id.clone(),
                });
            }
        }
    }

    let mut debug_functions = Vec::with_capacity(raw.debug_functions.len());
    for RawDebugFunctionEntry { label, function } in raw.debug_functions {
        debug_functions.push(DebugFunctionEntry {
            label,
            function: res(&function)?,
        });
    }

    Ok(NoiseGeneratorSettings::new(
        raw.noise,
        raw.default_block,
        raw.default_fluid,
        noise_router,
        raw.material_rule,
        raw.spawn_target,
        raw.sea_level,
        raw.disable_mob_generation,
        aquifers,
        raw.legacy_random_source,
        debug_functions,
    ))
}
