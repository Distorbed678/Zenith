//! Data-pack loader for 26.3 feature registries: `worldgen/block_state_provider`,
//! `worldgen/feature`, `worldgen/placed_feature`, `worldgen/biome`, and supporting tags
//! (`docs/specs/worldgen/features.md`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use thiserror::Error;
use zenith_blocks::{Direction, FluidKind, FluidState};
use zenith_codec::format::PackType;
use zenith_codec::json::{Json, JsonObject, ParseMode, parse_bytes};
use zenith_codec::resources::{PackResources, ResourceError};
use zenith_math::{NormalNoiseParameters, NormalizationMode};
use zenith_registry::{Identifier, MemoryTagSource, TagElementLookup, TagLoader};
use zenith_worldgen::{
    BiomeBitSet, BiomeData, BiomeId, CaveSurface, HeightmapKind, TemperatureModifier,
    VerticalAnchor,
};

mod tree_decode;

use crate::block_ops::{BlockBitSet, BlockOps};
use crate::features::{
    BlockColumnLayer, EndSpike, Feature, FossilConfig, GeodeConfig, HugeMushroomConfig,
    LargeDripstoneConfig, OreConfig, RootSystemConfig, SpeleothemClusterConfig, SpeleothemConfig,
    SteppedColumnClusterConfig, TemplateConfig, TemplateEntry, VegetationPatchConfig,
};
use crate::placement::{PlacedFeature, PlacedFeatureId, PlacementModifier};
use crate::providers::{
    BlockPredicate, BlockReplacement, BlockStateProvider, FloatProvider, FluidSet, HeightProvider,
    IntProvider, RuleTest, StateProviderRule, WeightedList, parse_fluid_kind,
};
use crate::rng::{BlockPos, Rotation};

const BSP_DIR: &str = "worldgen/block_state_provider";
const FEATURE_DIR: &str = "worldgen/feature";
const PLACED_FEATURE_DIR: &str = "worldgen/placed_feature";
const BIOME_DIR: &str = "worldgen/biome";
const BIOME_TAG_DIR: &str = "tags/worldgen/biome";
const PLACED_FEATURE_TAG_DIR: &str = "tags/worldgen/placed_feature";
const FLUID_TAG_DIR: &str = "tags/fluid";

/// Error returned when loading feature registries from a data pack.
#[derive(Debug, Clone, Error)]
pub enum FeatureLoadError {
    /// Pack listing or I/O error.
    #[error("pack resource error: {0}")]
    Resource(#[from] ResourceError),
    /// Missing file in pack.
    #[error("missing resource {id} in pack")]
    MissingResource {
        /// Missing file identifier.
        id: Identifier,
    },
    /// JSON syntax error.
    #[error("JSON parse error in {registry} entry {id}: {message}")]
    JsonParse {
        /// Registry directory.
        registry: &'static str,
        /// Element identifier.
        id: Identifier,
        /// Error detail.
        message: String,
    },
    /// Schema decode or reference resolution error.
    #[error("decode error in {registry} entry {id}: {message}")]
    Decode {
        /// Registry directory.
        registry: &'static str,
        /// Element identifier.
        id: Identifier,
        /// Error detail.
        message: String,
    },
}

/// Per-biome climate and feature lists decoded from `worldgen/biome/*.json`.
#[derive(Debug, Clone)]
pub struct BiomeFeatureSettings {
    /// Biome identifier.
    pub id: Identifier,
    /// `has_precipitation` flag (used by `freeze_top_layer`).
    pub has_precipitation: bool,
    /// Base biome climate data (`temperature`, `temperature_modifier`).
    pub climate: BiomeData,
    /// Ordered lists of placed features per decoration step (`0..k`).
    pub features: Vec<Vec<Arc<PlacedFeature>>>,
    /// Fast membership set of all placed feature IDs present in this biome (for `BiomeFilter`).
    feature_set: HashSet<PlacedFeatureId>,
}

impl BiomeFeatureSettings {
    /// Returns `true` if `feature_id` belongs to any decoration step of this biome.
    #[inline]
    pub fn has_feature(&self, feature_id: PlacedFeatureId) -> bool {
        self.feature_set.contains(&feature_id)
    }

    /// Evaluates `Biome.coldEnoughToSnow(BlockPos(x, y, z), seaLevel)`.
    #[inline]
    pub fn cold_enough_to_snow(&self, x: i32, y: i32, z: i32, sea_level: i32) -> bool {
        self.climate.cold_enough_to_snow(x, y, z, sea_level)
    }
}

/// All loaded 26.3 feature registries ready to construct [`crate::FeatureDecorator`].
#[derive(Debug, Clone)]
pub struct FeatureData {
    /// Loaded `minecraft:worldgen/block_state_provider` entries.
    pub block_state_providers: BTreeMap<Identifier, Arc<BlockStateProvider>>,
    /// Loaded `minecraft:worldgen/feature` entries.
    pub features: BTreeMap<Identifier, Arc<Feature>>,
    /// Loaded `minecraft:worldgen/placed_feature` entries.
    pub placed_features: BTreeMap<Identifier, Arc<PlacedFeature>>,
    /// Loaded biome settings in deterministic `BiomeId` order.
    pub biomes: Vec<BiomeFeatureSettings>,
    /// Lookup from biome identifier to dense `BiomeId`.
    pub biome_ids: HashMap<Identifier, BiomeId>,
    /// Resolved `tags/worldgen/biome` tags.
    pub biome_tags: BTreeMap<Identifier, Vec<Identifier>>,
}

impl FeatureData {
    /// Loads all 26.3 feature registries (`block_state_provider`, `feature`, `placed_feature`,
    /// `biome`, and tags) from `pack`.
    pub fn load(pack: &dyn PackResources, ops: &BlockOps) -> Result<Self, FeatureLoadError> {
        // 1. Discover raw JSON for all dynamic registries first.
        let bsp_raw = load_raw_json_dir(pack, BSP_DIR)?;
        let feat_raw = load_raw_json_dir(pack, FEATURE_DIR)?;
        let pf_raw = load_raw_json_dir(pack, PLACED_FEATURE_DIR)?;
        let biome_raw = load_raw_json_dir(pack, BIOME_DIR)?;

        // 2. Assign dense BiomeIds (BTreeMap order over all biomes in pack).
        let mut biome_ids = HashMap::with_capacity(biome_raw.len());
        for (idx, id) in biome_raw.keys().enumerate() {
            biome_ids.insert(id.clone(), BiomeId(idx as u16));
        }

        // 3. Load tags (`tags/worldgen/biome`, `tags/worldgen/placed_feature`, `tags/fluid`).
        let biome_tags = load_id_tags(pack, BIOME_TAG_DIR, |id| biome_raw.contains_key(id))?;
        let pf_tags = load_id_tags(pack, PLACED_FEATURE_TAG_DIR, |id| pf_raw.contains_key(id))?;
        let fluid_tags = load_id_tags(pack, FLUID_TAG_DIR, |id| parse_fluid_kind(id).is_some())?;

        // 4. Resolve registries lazily (supports forward references between BSPs, features, and placed features!).
        let mut resolver = Resolver {
            ops,
            biome_ids: &biome_ids,
            biome_tags: &biome_tags,
            pf_tags: &pf_tags,
            fluid_tags: &fluid_tags,
            bsp_raw: &bsp_raw,
            feat_raw: &feat_raw,
            pf_raw: &pf_raw,
            bsp_resolved: BTreeMap::new(),
            feat_resolved: BTreeMap::new(),
            pf_resolved: BTreeMap::new(),
            bsp_visiting: HashSet::new(),
            feat_visiting: HashSet::new(),
            pf_visiting: HashSet::new(),
            next_pf_id: 0,
        };

        for id in bsp_raw.keys() {
            resolver.resolve_bsp(id)?;
        }
        for id in feat_raw.keys() {
            resolver.resolve_feature(id)?;
        }
        for id in pf_raw.keys() {
            resolver.resolve_placed_feature(id)?;
        }

        // 5. Decode biomes in BiomeId order.
        let mut biomes = Vec::with_capacity(biome_raw.len());
        for (id, json) in &biome_raw {
            let settings =
                resolver
                    .decode_biome(id, json)
                    .map_err(|msg| FeatureLoadError::Decode {
                        registry: BIOME_DIR,
                        id: id.clone(),
                        message: msg,
                    })?;
            biomes.push(settings);
        }

        Ok(Self {
            block_state_providers: resolver.bsp_resolved,
            features: resolver.feat_resolved,
            placed_features: resolver.pf_resolved,
            biomes,
            biome_ids,
            biome_tags,
        })
    }

    /// Returns biome settings for `id`.
    pub fn biome(&self, id: &Identifier) -> Option<&BiomeFeatureSettings> {
        self.biome_ids
            .get(id)
            .map(|bid| &self.biomes[bid.0 as usize])
    }
}

// ---------------------------------------------------------------------------
// Pack file listing & tag helpers
// ---------------------------------------------------------------------------

fn load_raw_json_dir(
    pack: &dyn PackResources,
    dir: &'static str,
) -> Result<BTreeMap<Identifier, Json>, FeatureLoadError> {
    let prefix = format!("{dir}/");
    let mut files: Vec<(Identifier, Identifier)> = Vec::new();
    for ns in pack.namespaces(PackType::Data) {
        for file_id in pack.list(PackType::Data, &ns, dir)? {
            if !file_id.path().ends_with(".json") || !file_id.path().starts_with(&prefix) {
                continue;
            }
            let Some(rel) = file_id
                .path()
                .strip_prefix(&prefix)
                .and_then(|s| s.strip_suffix(".json"))
            else {
                continue;
            };
            if let Ok(elem_id) = Identifier::from_parts(file_id.namespace(), rel) {
                files.push((elem_id, file_id));
            }
        }
    }
    files.sort_by(|a, b| a.1.cmp(&b.1));

    let mut out = BTreeMap::new();
    for (elem_id, file_id) in files {
        let bytes = pack
            .get(PackType::Data, &file_id)
            .ok_or(FeatureLoadError::MissingResource { id: file_id })?;
        let json =
            parse_bytes(&bytes, ParseMode::Data).map_err(|e| FeatureLoadError::JsonParse {
                registry: dir,
                id: elem_id.clone(),
                message: e.to_string(),
            })?;
        out.insert(elem_id, json);
    }
    Ok(out)
}

struct FnIdLookup<F>(F);

impl<F: FnMut(&Identifier) -> bool> TagElementLookup<Identifier> for FnIdLookup<F> {
    type DedupKey = Identifier;

    fn lookup(&mut self, id: &Identifier, _required: bool) -> Option<Identifier> {
        (self.0)(id).then(|| id.clone())
    }

    fn dedup_key(element: &Identifier) -> Self::DedupKey {
        element.clone()
    }
}

fn load_id_tags(
    pack: &dyn PackResources,
    dir: &str,
    exists: impl FnMut(&Identifier) -> bool,
) -> Result<BTreeMap<Identifier, Vec<Identifier>>, FeatureLoadError> {
    let prefix = format!("{dir}/");
    let mut source = MemoryTagSource::new().with_pack_name(0, pack.pack_id());
    for ns in pack.namespaces(PackType::Data) {
        for file_id in pack.list(PackType::Data, &ns, dir)? {
            if !file_id.path().ends_with(".json") || !file_id.path().starts_with(&prefix) {
                continue;
            }
            let Some(rel) = file_id
                .path()
                .strip_prefix(&prefix)
                .and_then(|s| s.strip_suffix(".json"))
            else {
                continue;
            };
            if let Ok(tag_id) = Identifier::from_parts(file_id.namespace(), rel) {
                let content = pack
                    .get(PackType::Data, &file_id)
                    .ok_or_else(|| format!("missing tag file {file_id}"));
                source.add_file(0, tag_id, content);
            }
        }
    }
    let mut lookup = FnIdLookup(exists);
    let res = TagLoader::load_and_resolve(&source, &mut lookup);
    Ok(res.resolved.into_iter().collect())
}

// ---------------------------------------------------------------------------
// Recursive JSON resolver
// ---------------------------------------------------------------------------

struct Resolver<'a> {
    ops: &'a BlockOps,
    biome_ids: &'a HashMap<Identifier, BiomeId>,
    biome_tags: &'a BTreeMap<Identifier, Vec<Identifier>>,
    pf_tags: &'a BTreeMap<Identifier, Vec<Identifier>>,
    fluid_tags: &'a BTreeMap<Identifier, Vec<Identifier>>,
    bsp_raw: &'a BTreeMap<Identifier, Json>,
    feat_raw: &'a BTreeMap<Identifier, Json>,
    pf_raw: &'a BTreeMap<Identifier, Json>,
    bsp_resolved: BTreeMap<Identifier, Arc<BlockStateProvider>>,
    feat_resolved: BTreeMap<Identifier, Arc<Feature>>,
    pf_resolved: BTreeMap<Identifier, Arc<PlacedFeature>>,
    bsp_visiting: HashSet<Identifier>,
    feat_visiting: HashSet<Identifier>,
    pf_visiting: HashSet<Identifier>,
    next_pf_id: u32,
}

impl<'a> Resolver<'a> {
    fn alloc_pf_id(&mut self) -> PlacedFeatureId {
        let id = PlacedFeatureId(self.next_pf_id);
        self.next_pf_id += 1;
        id
    }

    fn resolve_bsp(
        &mut self,
        id: &Identifier,
    ) -> Result<Arc<BlockStateProvider>, FeatureLoadError> {
        if let Some(existing) = self.bsp_resolved.get(id) {
            return Ok(Arc::clone(existing));
        }
        if !self.bsp_visiting.insert(id.clone()) {
            return Err(FeatureLoadError::Decode {
                registry: BSP_DIR,
                id: id.clone(),
                message: "cyclic block_state_provider reference".to_owned(),
            });
        }
        let json = self
            .bsp_raw
            .get(id)
            .ok_or_else(|| FeatureLoadError::Decode {
                registry: BSP_DIR,
                id: id.clone(),
                message: format!("unknown block_state_provider '{id}'"),
            })?;
        let decoded = self
            .decode_bsp_direct(json)
            .map_err(|msg| FeatureLoadError::Decode {
                registry: BSP_DIR,
                id: id.clone(),
                message: msg,
            })?;
        self.bsp_visiting.remove(id);
        let arc = Arc::new(decoded);
        self.bsp_resolved.insert(id.clone(), Arc::clone(&arc));
        Ok(arc)
    }

    fn resolve_feature(&mut self, id: &Identifier) -> Result<Arc<Feature>, FeatureLoadError> {
        if let Some(existing) = self.feat_resolved.get(id) {
            return Ok(Arc::clone(existing));
        }
        if !self.feat_visiting.insert(id.clone()) {
            return Err(FeatureLoadError::Decode {
                registry: FEATURE_DIR,
                id: id.clone(),
                message: "cyclic feature reference".to_owned(),
            });
        }
        let json = self
            .feat_raw
            .get(id)
            .ok_or_else(|| FeatureLoadError::Decode {
                registry: FEATURE_DIR,
                id: id.clone(),
                message: format!("unknown feature '{id}'"),
            })?;
        let decoded = self
            .decode_feature(json)
            .map_err(|msg| FeatureLoadError::Decode {
                registry: FEATURE_DIR,
                id: id.clone(),
                message: msg,
            })?;
        self.feat_visiting.remove(id);
        let arc = Arc::new(decoded);
        self.feat_resolved.insert(id.clone(), Arc::clone(&arc));
        Ok(arc)
    }

    fn resolve_placed_feature(
        &mut self,
        id: &Identifier,
    ) -> Result<Arc<PlacedFeature>, FeatureLoadError> {
        if let Some(existing) = self.pf_resolved.get(id) {
            return Ok(Arc::clone(existing));
        }
        if !self.pf_visiting.insert(id.clone()) {
            return Err(FeatureLoadError::Decode {
                registry: PLACED_FEATURE_DIR,
                id: id.clone(),
                message: "cyclic placed_feature reference".to_owned(),
            });
        }
        let json = self
            .pf_raw
            .get(id)
            .ok_or_else(|| FeatureLoadError::Decode {
                registry: PLACED_FEATURE_DIR,
                id: id.clone(),
                message: format!("unknown placed_feature '{id}'"),
            })?;
        let decoded = self
            .decode_placed_feature(json, Some(id.clone()))
            .map_err(|msg| FeatureLoadError::Decode {
                registry: PLACED_FEATURE_DIR,
                id: id.clone(),
                message: msg,
            })?;
        self.pf_visiting.remove(id);
        let arc = Arc::new(decoded);
        self.pf_resolved.insert(id.clone(), Arc::clone(&arc));
        Ok(arc)
    }

    fn decode_biome(
        &mut self,
        id: &Identifier,
        json: &Json,
    ) -> Result<BiomeFeatureSettings, String> {
        let obj = as_obj(json)?;
        let has_precipitation = req_bool(obj, "has_precipitation")?;
        let temperature = req_f32(obj, "temperature")?;
        let temperature_modifier = match opt_str(obj, "temperature_modifier")? {
            None | Some("none") => TemperatureModifier::None,
            Some("frozen") => TemperatureModifier::Frozen,
            Some(other) => return Err(format!("unknown temperature_modifier '{other}'")),
        };
        let mut features = Vec::new();
        let mut feature_set = HashSet::new();
        if let Some(feat_arr) = obj.get("features") {
            let steps = as_arr(feat_arr)?;
            for step_json in steps {
                let list = self.decode_placed_feature_holder_set(step_json)?;
                for pf in &list {
                    feature_set.insert(pf.id);
                }
                features.push(list);
            }
        }
        Ok(BiomeFeatureSettings {
            id: id.clone(),
            has_precipitation,
            climate: BiomeData {
                temperature,
                temperature_modifier,
                carvers: Vec::new(),
            },
            features,
            feature_set,
        })
    }

    // -----------------------------------------------------------------------
    // Primitives & shared helpers
    // -----------------------------------------------------------------------

    fn decode_block_state(&self, json: &Json) -> Result<u32, String> {
        if let Some(s) = json.as_str() {
            let id = parse_id(s)?;
            let entry = self
                .ops
                .blocks
                .block(&id)
                .ok_or_else(|| format!("unknown block '{id}'"))?;
            return Ok(entry.default_state);
        }
        if let Some(obj) = json.as_object() {
            let id_str = obj
                .get("id")
                .or_else(|| obj.get("Name"))
                .and_then(Json::as_str)
                .ok_or_else(|| "BlockState object missing 'id'".to_owned())?;
            let id = parse_id(id_str)?;
            let mut props = BTreeMap::new();
            if let Some(pmap) = obj
                .get("properties")
                .or_else(|| obj.get("Properties"))
                .and_then(Json::as_object)
            {
                for (k, v) in pmap.iter() {
                    if let (Some(ks), Some(vs)) = (k.as_str(), v.as_str()) {
                        props.insert(ks.to_owned(), vs.to_owned());
                    }
                }
            }
            return self
                .ops
                .resolve_state(&id, &props)
                .ok_or_else(|| format!("unknown block '{id}'"));
        }
        Err(format!("expected BlockState, got {}", json.type_name()))
    }

    fn decode_fluid_state(&self, json: &Json) -> Result<(FluidState, u32), String> {
        let (id_str, props) = if let Some(s) = json.as_str() {
            (s, None)
        } else if let Some(obj) = json.as_object() {
            let s = obj
                .get("id")
                .or_else(|| obj.get("Name"))
                .and_then(Json::as_str)
                .ok_or_else(|| "FluidState object missing 'id'".to_owned())?;
            let p = obj
                .get("properties")
                .or_else(|| obj.get("Properties"))
                .and_then(Json::as_object);
            (s, p)
        } else {
            return Err(format!("expected FluidState, got {}", json.type_name()));
        };
        let id = parse_id(id_str)?;
        let kind = parse_fluid_kind(&id).ok_or_else(|| format!("unknown fluid '{id}'"))?;
        let falling = props
            .and_then(|p| p.get("falling"))
            .and_then(Json::as_str)
            .is_some_and(|v| v == "true");
        let level_prop = props
            .and_then(|p| p.get("level"))
            .and_then(Json::as_str)
            .and_then(|v| v.parse::<u8>().ok())
            .unwrap_or(8);

        let (fl, legacy_block) = match kind {
            FluidKind::Empty => (FluidState::EMPTY, self.ops.sid_air),
            FluidKind::Water => (
                FluidState {
                    kind,
                    amount: 8,
                    falling,
                    is_source: true,
                    own_height: 8.0_f32 / 9.0_f32,
                },
                self.ops.sid_water,
            ),
            FluidKind::FlowingWater => {
                let legacy_level = if falling {
                    8
                } else {
                    8u8.saturating_sub(level_prop.min(8))
                };
                let sid = self.ops.try_set_property(
                    self.ops.sid_water,
                    "level",
                    &legacy_level.to_string(),
                );
                (
                    FluidState {
                        kind,
                        amount: level_prop,
                        falling,
                        is_source: false,
                        own_height: f32::from(level_prop) / 9.0_f32,
                    },
                    sid,
                )
            }
            FluidKind::Lava => (
                FluidState {
                    kind,
                    amount: 8,
                    falling,
                    is_source: true,
                    own_height: 8.0_f32 / 9.0_f32,
                },
                self.ops.sid_lava,
            ),
            FluidKind::FlowingLava => {
                let legacy_level = if falling {
                    8
                } else {
                    8u8.saturating_sub(level_prop.min(8))
                };
                let sid = self.ops.try_set_property(
                    self.ops.sid_lava,
                    "level",
                    &legacy_level.to_string(),
                );
                (
                    FluidState {
                        kind,
                        amount: level_prop,
                        falling,
                        is_source: false,
                        own_height: f32::from(level_prop) / 9.0_f32,
                    },
                    sid,
                )
            }
        };
        Ok((fl, legacy_block))
    }

    fn decode_block_id(&self, json: &Json) -> Result<u32, String> {
        let s = json
            .as_str()
            .ok_or_else(|| format!("expected block id string, got {}", json.type_name()))?;
        let id = parse_id(s)?;
        self.ops
            .blocks
            .block(&id)
            .map(|e| e.block_protocol_id)
            .ok_or_else(|| format!("unknown block '{id}'"))
    }

    fn decode_block_holder_set(&self, json: &Json) -> Result<BlockBitSet, String> {
        let mut bs = BlockBitSet::new(self.ops.blocks.blocks().len());
        if let Some(s) = json.as_str() {
            if let Some(tag_s) = s.strip_prefix('#') {
                let tag_id = parse_id(tag_s)?;
                if let Some(ids) = self.ops.tags.block_ids(&tag_id) {
                    for &bid in ids {
                        bs.insert(bid);
                    }
                }
            } else {
                let id = parse_id(s)?;
                let entry = self
                    .ops
                    .blocks
                    .block(&id)
                    .ok_or_else(|| format!("unknown block '{id}'"))?;
                bs.insert(entry.block_protocol_id);
            }
        } else if let Some(arr) = json.as_array() {
            for item in arr {
                bs.insert(self.decode_block_id(item)?);
            }
        } else {
            return Err(format!(
                "expected block holder set, got {}",
                json.type_name()
            ));
        }
        Ok(bs)
    }

    fn decode_block_holder_set_default_states(&self, json: &Json) -> Result<Vec<u32>, String> {
        let mut out = Vec::new();
        if let Some(s) = json.as_str() {
            if let Some(tag_s) = s.strip_prefix('#') {
                let tag_id = parse_id(tag_s)?;
                if let Some(ids) = self.ops.tags.block_ids(&tag_id) {
                    for &bid in ids {
                        out.push(self.ops.blocks.blocks()[bid as usize].default_state);
                    }
                }
            } else {
                let id = parse_id(s)?;
                let entry = self
                    .ops
                    .blocks
                    .block(&id)
                    .ok_or_else(|| format!("unknown block '{id}'"))?;
                out.push(entry.default_state);
            }
        } else if let Some(arr) = json.as_array() {
            for item in arr {
                let bid = self.decode_block_id(item)?;
                out.push(self.ops.blocks.blocks()[bid as usize].default_state);
            }
        } else {
            return Err(format!(
                "expected block holder set, got {}",
                json.type_name()
            ));
        }
        Ok(out)
    }

    fn decode_fluid_holder_set(&self, json: &Json) -> Result<FluidSet, String> {
        let mut set = FluidSet::default();
        if let Some(s) = json.as_str() {
            if let Some(tag_s) = s.strip_prefix('#') {
                let tag_id = parse_id(tag_s)?;
                if let Some(ids) = self.fluid_tags.get(&tag_id) {
                    for fid in ids {
                        if let Some(k) = parse_fluid_kind(fid) {
                            set.insert(k);
                        }
                    }
                }
            } else {
                let id = parse_id(s)?;
                let k = parse_fluid_kind(&id).ok_or_else(|| format!("unknown fluid '{id}'"))?;
                set.insert(k);
            }
        } else if let Some(arr) = json.as_array() {
            for item in arr {
                let s = item.as_str().ok_or("expected fluid id string")?;
                let id = parse_id(s)?;
                let k = parse_fluid_kind(&id).ok_or_else(|| format!("unknown fluid '{id}'"))?;
                set.insert(k);
            }
        } else {
            return Err(format!(
                "expected fluid holder set, got {}",
                json.type_name()
            ));
        }
        Ok(set)
    }

    fn decode_biome_holder_set(&self, json: &Json) -> Result<BiomeBitSet, String> {
        let mut bs = BiomeBitSet::new(self.biome_ids.len());
        if let Some(s) = json.as_str() {
            if let Some(tag_s) = s.strip_prefix('#') {
                let tag_id = parse_id(tag_s)?;
                if let Some(ids) = self.biome_tags.get(&tag_id) {
                    for bid in ids {
                        if let Some(&dense) = self.biome_ids.get(bid) {
                            bs.insert(dense);
                        }
                    }
                }
            } else {
                let id = parse_id(s)?;
                if let Some(&dense) = self.biome_ids.get(&id) {
                    bs.insert(dense);
                }
            }
        } else if let Some(arr) = json.as_array() {
            for item in arr {
                let s = item.as_str().ok_or("expected biome id string")?;
                let id = parse_id(s)?;
                if let Some(&dense) = self.biome_ids.get(&id) {
                    bs.insert(dense);
                }
            }
        } else {
            return Err(format!(
                "expected biome holder set, got {}",
                json.type_name()
            ));
        }
        Ok(bs)
    }

    // -----------------------------------------------------------------------
    // Providers (Int, Float, VerticalAnchor, Height, RuleTest, BlockPredicate, BSP)
    // -----------------------------------------------------------------------

    fn decode_int_provider(&self, json: &Json) -> Result<IntProvider, String> {
        if let Json::Number(n) = json {
            return Ok(IntProvider::Constant(
                n.to_i32().map_err(|e| e.to_string())?,
            ));
        }
        let obj = as_obj(json)?;
        let ty = req_type_path(obj)?;
        let target = obj.get("value").and_then(Json::as_object).unwrap_or(obj);
        match ty.as_str() {
            "constant" => Ok(IntProvider::Constant(req_i32(obj, "value")?)),
            "uniform" => Ok(IntProvider::Uniform {
                min_inclusive: req_i32(target, "min_inclusive")?,
                max_inclusive: req_i32(target, "max_inclusive")?,
            }),
            "biased_to_bottom" => Ok(IntProvider::BiasedToBottom {
                min_inclusive: req_i32(target, "min_inclusive")?,
                max_inclusive: req_i32(target, "max_inclusive")?,
            }),
            "very_biased_to_bottom" => Ok(IntProvider::VeryBiasedToBottom {
                min_inclusive: req_i32(target, "min_inclusive")?,
                max_inclusive: req_i32(target, "max_inclusive")?,
            }),
            "clamped" => Ok(IntProvider::Clamped {
                source: Box::new(self.decode_int_provider(req_field(target, "source")?)?),
                min_inclusive: req_i32(target, "min_inclusive")?,
                max_inclusive: req_i32(target, "max_inclusive")?,
            }),
            "clamped_normal" => Ok(IntProvider::ClampedNormal {
                mean: req_f32(target, "mean")?,
                deviation: req_f32(target, "deviation")?,
                min_inclusive: req_i32(target, "min_inclusive")?,
                max_inclusive: req_i32(target, "max_inclusive")?,
            }),
            "weighted_list" => {
                let wl = decode_weighted_list(req_field(obj, "distribution")?, |j| {
                    self.decode_int_provider(j)
                })?;
                Ok(IntProvider::WeightedList(wl))
            }
            "trapezoid" => Ok(IntProvider::Trapezoid {
                min: req_i32(target, "min")?,
                max: req_i32(target, "max")?,
                plateau: opt_i32(target, "plateau")?.unwrap_or(0),
            }),
            other => Err(format!("unknown int_provider type '{other}'")),
        }
    }

    fn decode_float_provider(&self, json: &Json) -> Result<FloatProvider, String> {
        if let Json::Number(n) = json {
            return Ok(FloatProvider::Constant(n.to_f32()));
        }
        let obj = as_obj(json)?;
        let ty = req_type_path(obj)?;
        let target = obj.get("value").and_then(Json::as_object).unwrap_or(obj);
        match ty.as_str() {
            "constant" => Ok(FloatProvider::Constant(req_f32(obj, "value")?)),
            "uniform" => Ok(FloatProvider::Uniform {
                min_inclusive: req_f32(target, "min_inclusive")?,
                max_exclusive: req_f32(target, "max_exclusive")?,
            }),
            "clamped_normal" => Ok(FloatProvider::ClampedNormal {
                mean: req_f32(target, "mean")?,
                deviation: req_f32(target, "deviation")?,
                min: req_f32(target, "min")?,
                max: req_f32(target, "max")?,
            }),
            "trapezoid" => Ok(FloatProvider::Trapezoid {
                min: req_f32(target, "min")?,
                max: req_f32(target, "max")?,
                plateau: opt_f32(target, "plateau")?.unwrap_or(0.0),
            }),
            other => Err(format!("unknown float_provider type '{other}'")),
        }
    }

    fn decode_vertical_anchor(&self, json: &Json) -> Result<VerticalAnchor, String> {
        let obj = as_obj(json)?;
        if let Some(v) = opt_i32(obj, "absolute")? {
            return Ok(VerticalAnchor::Absolute(v));
        }
        if let Some(v) = opt_i32(obj, "above_bottom")? {
            return Ok(VerticalAnchor::AboveBottom(v));
        }
        if let Some(v) = opt_i32(obj, "below_top")? {
            return Ok(VerticalAnchor::BelowTop(v));
        }
        if let Some(v) = opt_i32(obj, "relative_to_sea_level")? {
            return Ok(VerticalAnchor::RelativeToSeaLevel(v));
        }
        Err(
            "VerticalAnchor missing absolute/above_bottom/below_top/relative_to_sea_level"
                .to_owned(),
        )
    }

    fn decode_height_provider(&self, json: &Json) -> Result<HeightProvider, String> {
        let obj = as_obj(json)?;
        if !obj.contains_key("type") {
            return Ok(HeightProvider::Constant(self.decode_vertical_anchor(json)?));
        }
        let ty = req_type_path(obj)?;
        match ty.as_str() {
            "constant" => Ok(HeightProvider::Constant(
                self.decode_vertical_anchor(req_field(obj, "value")?)?,
            )),
            "uniform" => Ok(HeightProvider::Uniform {
                min_inclusive: self.decode_vertical_anchor(req_field(obj, "min_inclusive")?)?,
                max_inclusive: self.decode_vertical_anchor(req_field(obj, "max_inclusive")?)?,
            }),
            "biased_to_bottom" => Ok(HeightProvider::BiasedToBottom {
                min_inclusive: self.decode_vertical_anchor(req_field(obj, "min_inclusive")?)?,
                max_inclusive: self.decode_vertical_anchor(req_field(obj, "max_inclusive")?)?,
                inner: opt_i32(obj, "inner")?.unwrap_or(1),
            }),
            "very_biased_to_bottom" => Ok(HeightProvider::VeryBiasedToBottom {
                min_inclusive: self.decode_vertical_anchor(req_field(obj, "min_inclusive")?)?,
                max_inclusive: self.decode_vertical_anchor(req_field(obj, "max_inclusive")?)?,
                inner: opt_i32(obj, "inner")?.unwrap_or(1),
            }),
            "trapezoid" => Ok(HeightProvider::Trapezoid {
                min_inclusive: self.decode_vertical_anchor(req_field(obj, "min_inclusive")?)?,
                max_inclusive: self.decode_vertical_anchor(req_field(obj, "max_inclusive")?)?,
                plateau: opt_i32(obj, "plateau")?.unwrap_or(0),
            }),
            "weighted_list" => {
                let wl = decode_weighted_list(req_field(obj, "distribution")?, |j| {
                    self.decode_height_provider(j)
                })?;
                Ok(HeightProvider::WeightedList(wl))
            }
            other => Err(format!("unknown height_provider type '{other}'")),
        }
    }

    fn decode_rule_test(&self, json: &Json) -> Result<RuleTest, String> {
        let obj = as_obj(json)?;
        let ty = parse_id(req_str(obj, "predicate_type")?)?.path().to_owned();
        match ty.as_str() {
            "always_true" => Ok(RuleTest::AlwaysTrue),
            "all_of" => {
                let mut rules = Vec::new();
                for item in as_arr(req_field(obj, "rules")?)? {
                    rules.push(self.decode_rule_test(item)?);
                }
                Ok(RuleTest::AllOf(rules))
            }
            "any_of" => {
                let mut rules = Vec::new();
                for item in as_arr(req_field(obj, "rules")?)? {
                    rules.push(self.decode_rule_test(item)?);
                }
                Ok(RuleTest::AnyOf(rules))
            }
            "not" => Ok(RuleTest::Not(Box::new(
                self.decode_rule_test(req_field(obj, "rule")?)?,
            ))),
            "block_match" => Ok(RuleTest::BlockMatch(
                self.decode_block_id(req_field(obj, "block")?)?,
            )),
            "blockstate_match" => Ok(RuleTest::BlockStateMatch(
                self.decode_block_state(req_field(obj, "block_state")?)?,
            )),
            "tag_match" => {
                let tag_id = parse_id(req_str(obj, "tag")?)?;
                Ok(RuleTest::TagMatch(self.ops.bitset_for_tag(&tag_id)))
            }
            "height_match" => Ok(RuleTest::HeightMatch {
                min_inclusive: req_i32(obj, "min_inclusive")?,
                max_inclusive: req_i32(obj, "max_inclusive")?,
            }),
            "random_block_match" => Ok(RuleTest::RandomBlockMatch {
                block: self.decode_block_id(req_field(obj, "block")?)?,
                probability: req_f32(obj, "probability")?,
            }),
            "random_blockstate_match" => Ok(RuleTest::RandomBlockStateMatch {
                block_state: self.decode_block_state(req_field(obj, "block_state")?)?,
                probability: req_f32(obj, "probability")?,
            }),
            other => Err(format!("unknown rule_test predicate_type '{other}'")),
        }
    }

    fn decode_block_replacements(&self, json: &Json) -> Result<Vec<BlockReplacement>, String> {
        let mut out = Vec::new();
        for item in as_arr(json)? {
            let obj = as_obj(item)?;
            out.push(BlockReplacement {
                target: self.decode_rule_test(req_field(obj, "target")?)?,
                state: self.decode_block_state(req_field(obj, "state")?)?,
            });
        }
        Ok(out)
    }

    fn decode_block_predicate(&self, json: &Json) -> Result<BlockPredicate, String> {
        let obj = as_obj(json)?;
        let ty = req_type_path(obj)?;
        let offset = opt_vec3i(obj, "offset")?.unwrap_or([0, 0, 0]);
        match ty.as_str() {
            "all_of" => {
                let mut preds = Vec::new();
                for item in as_arr(req_field(obj, "predicates")?)? {
                    preds.push(self.decode_block_predicate(item)?);
                }
                Ok(BlockPredicate::AllOf(preds))
            }
            "any_of" => {
                let mut preds = Vec::new();
                for item in as_arr(req_field(obj, "predicates")?)? {
                    preds.push(self.decode_block_predicate(item)?);
                }
                Ok(BlockPredicate::AnyOf(preds))
            }
            "not" => Ok(BlockPredicate::Not(Box::new(
                self.decode_block_predicate(req_field(obj, "predicate")?)?,
            ))),
            "true" => Ok(BlockPredicate::True),
            "matching_blocks" => Ok(BlockPredicate::MatchingBlocks {
                offset,
                blocks: self.decode_block_holder_set(req_field(obj, "blocks")?)?,
            }),
            "matching_block_tag" => {
                let tag_id = parse_id(req_str(obj, "tag")?)?;
                Ok(BlockPredicate::MatchingBlockTag {
                    offset,
                    tag: self.ops.bitset_for_tag(&tag_id),
                })
            }
            "matching_fluids" => Ok(BlockPredicate::MatchingFluids {
                offset,
                fluids: self.decode_fluid_holder_set(req_field(obj, "fluids")?)?,
            }),
            "matching_biomes" => Ok(BlockPredicate::MatchingBiomes(
                self.decode_biome_holder_set(req_field(obj, "biomes")?)?,
            )),
            "has_sturdy_face" => Ok(BlockPredicate::HasSturdyFace {
                offset,
                direction: parse_direction(req_str(obj, "direction")?)?,
            }),
            "solid" => Ok(BlockPredicate::Solid { offset }),
            "replaceable" => Ok(BlockPredicate::Replaceable { offset }),
            "would_survive" => Ok(BlockPredicate::WouldSurvive {
                offset,
                state: self.decode_block_state(req_field(obj, "state")?)?,
            }),
            "inside_world_bounds" => Ok(BlockPredicate::InsideWorldBounds { offset }),
            "unobstructed" => Ok(BlockPredicate::Unobstructed { offset }),
            "height_range" => Ok(BlockPredicate::HeightRange {
                min_inclusive: self.decode_vertical_anchor(req_field(obj, "min_inclusive")?)?,
                max_inclusive: self.decode_vertical_anchor(req_field(obj, "max_inclusive")?)?,
            }),
            "volume_match" => Ok(BlockPredicate::VolumeMatch {
                min: decode_vec3i(req_field(obj, "min")?)?,
                max: decode_vec3i(req_field(obj, "max")?)?,
                match_predicate: Box::new(self.decode_block_predicate(req_field(obj, "match")?)?),
            }),
            other => Err(format!("unknown block_predicate type '{other}'")),
        }
    }

    fn decode_bsp_holder(&mut self, json: &Json) -> Result<Arc<BlockStateProvider>, String> {
        if let Some(s) = json.as_str() {
            let id = parse_id(s)?;
            if self.bsp_raw.contains_key(&id) {
                return self.resolve_bsp(&id).map_err(|e| e.to_string());
            }
            // Fallback: bare block ID string (if ever used as inline BlockState shorthand)
            if let Some(entry) = self.ops.blocks.block(&id) {
                return Ok(Arc::new(BlockStateProvider::Simple(entry.default_state)));
            }
            return Err(format!("unknown block_state_provider '{id}'"));
        }
        Ok(Arc::new(self.decode_bsp_direct(json)?))
    }

    fn decode_bsp_direct(&mut self, json: &Json) -> Result<BlockStateProvider, String> {
        let obj = as_obj(json)?;
        // BlockState.FULL_CODEC shorthand when there is no "type" field.
        if !obj.contains_key("type") {
            let st = self.decode_block_state(json)?;
            return Ok(BlockStateProvider::Simple(st));
        }
        let ty = req_type_path(obj)?;
        match ty.as_str() {
            "simple" | "simple_state_provider" => Ok(BlockStateProvider::Simple(
                self.decode_block_state(req_field(obj, "state")?)?,
            )),
            "weighted" | "weighted_state_provider" => {
                let wl = decode_weighted_list(req_field(obj, "entries")?, |j| {
                    self.decode_block_state(j)
                })?;
                Ok(BlockStateProvider::Weighted(wl))
            }
            "noise" | "noise_provider" => {
                let seed = req_i64(obj, "seed")?;
                let params = decode_normal_noise_params(req_field(obj, "noise")?)?;
                let scale = req_f32(obj, "scale")?;
                let states = self.decode_block_state_list(req_field(obj, "states")?)?;
                Ok(BlockStateProvider::Noise {
                    noise: BlockStateProvider::build_noise(seed, params),
                    scale,
                    states,
                })
            }
            "dual_noise" | "dual_noise_provider" => {
                let seed = req_i64(obj, "seed")?;
                let (variety_min, variety_max) =
                    decode_inclusive_range(req_field(obj, "variety")?)?;
                let slow_params = decode_normal_noise_params(req_field(obj, "slow_noise")?)?;
                let slow_scale = req_f32(obj, "slow_scale")?;
                let params = decode_normal_noise_params(req_field(obj, "noise")?)?;
                let scale = req_f32(obj, "scale")?;
                let states = self.decode_block_state_list(req_field(obj, "states")?)?;
                Ok(BlockStateProvider::DualNoise {
                    variety_min,
                    variety_max,
                    slow_noise: BlockStateProvider::build_noise(seed, slow_params),
                    slow_scale,
                    noise: BlockStateProvider::build_noise(seed, params),
                    scale,
                    states,
                })
            }
            "noise_threshold" | "noise_threshold_provider" => {
                let seed = req_i64(obj, "seed")?;
                let params = decode_normal_noise_params(req_field(obj, "noise")?)?;
                let scale = req_f32(obj, "scale")?;
                let threshold = req_f32(obj, "threshold")?;
                let high_chance = req_f32(obj, "high_chance")?;
                let default_state = self.decode_block_state(req_field(obj, "default_state")?)?;
                let low_states = self.decode_block_state_list(req_field(obj, "low_states")?)?;
                let high_states = self.decode_block_state_list(req_field(obj, "high_states")?)?;
                Ok(BlockStateProvider::NoiseThreshold {
                    noise: BlockStateProvider::build_noise(seed, params),
                    scale,
                    threshold,
                    high_chance,
                    default_state,
                    low_states,
                    high_states,
                })
            }
            "rotated" | "rotated_block_provider" => {
                let state = self.decode_bsp_holder(req_field(obj, "state")?)?;
                let direction = match opt_str(obj, "direction")? {
                    Some(s) => Some(parse_direction(s)?),
                    None => None,
                };
                Ok(BlockStateProvider::Rotated { state, direction })
            }
            "randomized_int" | "randomized_int_state_provider" => {
                let source = self.decode_bsp_holder(req_field(obj, "source")?)?;
                let property = req_str(obj, "property")?.to_owned();
                let values = self.decode_int_provider(req_field(obj, "values")?)?;
                Ok(BlockStateProvider::RandomizedInt {
                    source,
                    property,
                    values,
                })
            }
            "rule_based" | "rule_based_state_provider" => {
                let fallback = match obj.get("fallback") {
                    Some(j) if !matches!(j, Json::Null) => Some(self.decode_bsp_holder(j)?),
                    _ => None,
                };
                let mut rules = Vec::new();
                for item in as_arr(req_field(obj, "rules")?)? {
                    let robj = as_obj(item)?;
                    rules.push(StateProviderRule {
                        if_true: self.decode_block_predicate(req_field(robj, "if_true")?)?,
                        then: self.decode_bsp_holder(req_field(robj, "then")?)?,
                    });
                }
                Ok(BlockStateProvider::RuleBased { fallback, rules })
            }
            "copy_properties" => {
                let source = self.decode_bsp_holder(req_field(obj, "source")?)?;
                Ok(BlockStateProvider::CopyProperties { source })
            }
            "random_block" => {
                let default_states =
                    self.decode_block_holder_set_default_states(req_field(obj, "blocks")?)?;
                Ok(BlockStateProvider::RandomBlock { default_states })
            }
            other => Err(format!("unknown block_state_provider type '{other}'")),
        }
    }

    fn decode_block_state_list(&self, json: &Json) -> Result<Vec<u32>, String> {
        let mut out = Vec::new();
        for item in as_arr(json)? {
            out.push(self.decode_block_state(item)?);
        }
        Ok(out)
    }

    // -----------------------------------------------------------------------
    // PlacedFeature & 18 PlacementModifiers
    // -----------------------------------------------------------------------

    fn decode_placed_feature_holder(&mut self, json: &Json) -> Result<Arc<PlacedFeature>, String> {
        if let Some(s) = json.as_str() {
            let id = parse_id(s)?;
            return self.resolve_placed_feature(&id).map_err(|e| e.to_string());
        }
        let pf = self.decode_placed_feature(json, None)?;
        Ok(Arc::new(pf))
    }

    fn decode_placed_feature_holder_set(
        &mut self,
        json: &Json,
    ) -> Result<Vec<Arc<PlacedFeature>>, String> {
        let mut out = Vec::new();
        if let Some(s) = json.as_str() {
            if let Some(tag_s) = s.strip_prefix('#') {
                let tag_id = parse_id(tag_s)?;
                let ids = self.pf_tags.get(&tag_id).cloned().unwrap_or_default();
                for pid in ids {
                    out.push(
                        self.resolve_placed_feature(&pid)
                            .map_err(|e| e.to_string())?,
                    );
                }
            } else {
                let id = parse_id(s)?;
                out.push(
                    self.resolve_placed_feature(&id)
                        .map_err(|e| e.to_string())?,
                );
            }
        } else if json.as_object().is_some() {
            out.push(self.decode_placed_feature_holder(json)?);
        } else if let Some(arr) = json.as_array() {
            for item in arr {
                out.push(self.decode_placed_feature_holder(item)?);
            }
        } else {
            return Err(format!(
                "expected placed_feature holder set, got {}",
                json.type_name()
            ));
        }
        Ok(out)
    }

    fn decode_placed_feature(
        &mut self,
        json: &Json,
        name: Option<Identifier>,
    ) -> Result<PlacedFeature, String> {
        let obj = as_obj(json)?;
        let feature = self.decode_feature_holder(req_field(obj, "feature")?)?;
        let mut placement = Vec::new();
        for item in as_arr(req_field(obj, "placement")?)? {
            placement.push(self.decode_placement_modifier(item)?);
        }
        let id = self.alloc_pf_id();
        Ok(PlacedFeature {
            id,
            name,
            feature,
            placement,
        })
    }

    fn decode_placement_modifier(&self, json: &Json) -> Result<PlacementModifier, String> {
        let obj = as_obj(json)?;
        let ty = req_type_path(obj)?;
        match ty.as_str() {
            "biome" => Ok(PlacementModifier::Biome),
            "block_predicate_filter" => Ok(PlacementModifier::BlockPredicateFilter(
                self.decode_block_predicate(req_field(obj, "predicate")?)?,
            )),
            "count" => Ok(PlacementModifier::Count(
                self.decode_int_provider(req_field(obj, "count")?)?,
            )),
            "count_on_every_layer" => Ok(PlacementModifier::CountOnEveryLayer(
                self.decode_int_provider(req_field(obj, "count")?)?,
            )),
            "cuboid" => Ok(PlacementModifier::Cuboid {
                xz_size: self.decode_int_provider(req_field(obj, "xz_size")?)?,
                y_size: self.decode_int_provider(req_field(obj, "y_size")?)?,
                include_edges: opt_bool(obj, "include_edges")?.unwrap_or(true),
                include_interior: opt_bool(obj, "include_interior")?.unwrap_or(true),
            }),
            "environment_scan" => Ok(PlacementModifier::EnvironmentScan {
                direction_of_search: parse_direction(req_str(obj, "direction_of_search")?)?,
                target_condition: self
                    .decode_block_predicate(req_field(obj, "target_condition")?)?,
                allowed_search_condition: match obj.get("allowed_search_condition") {
                    Some(j) if !matches!(j, Json::Null) => self.decode_block_predicate(j)?,
                    _ => BlockPredicate::True,
                },
                max_steps: req_i32(obj, "max_steps")?,
            }),
            "fixed_placement" => {
                let mut positions = Vec::new();
                for item in as_arr(req_field(obj, "positions")?)? {
                    let v = decode_vec3i(item)?;
                    positions.push(BlockPos::new(v[0], v[1], v[2]));
                }
                Ok(PlacementModifier::FixedPlacement(positions))
            }
            "height_range" => Ok(PlacementModifier::HeightRange(
                self.decode_height_provider(req_field(obj, "height")?)?,
            )),
            "heightmap" => Ok(PlacementModifier::Heightmap(parse_heightmap(req_str(
                obj,
                "heightmap",
            )?)?)),
            "in_square" => Ok(PlacementModifier::InSquare),
            "noise_based_count" => Ok(PlacementModifier::NoiseBasedCount {
                noise_to_count_ratio: req_i32(obj, "noise_to_count_ratio")?,
                noise_factor: req_f64(obj, "noise_factor")?,
                noise_offset: opt_f64(obj, "noise_offset")?.unwrap_or(0.0),
            }),
            "noise_threshold_count" => Ok(PlacementModifier::NoiseThresholdCount {
                noise_level: req_f64(obj, "noise_level")?,
                below_noise: req_i32(obj, "below_noise")?,
                above_noise: req_i32(obj, "above_noise")?,
            }),
            "offset" => Ok(PlacementModifier::Offset {
                x: self.decode_int_provider(req_field(obj, "x")?)?,
                y: self.decode_int_provider(req_field(obj, "y")?)?,
                z: self.decode_int_provider(req_field(obj, "z")?)?,
            }),
            "random_chance" => Ok(PlacementModifier::RandomChance(req_f32(obj, "chance")?)),
            "randomly_selected" => {
                let mut placements = Vec::new();
                for item in as_arr(req_field(obj, "placements")?)? {
                    placements.push(self.decode_placement_modifier(item)?);
                }
                Ok(PlacementModifier::RandomlySelected(placements))
            }
            "rarity_filter" => Ok(PlacementModifier::RarityFilter(req_i32(obj, "chance")?)),
            "surface_relative_threshold_filter" => {
                Ok(PlacementModifier::SurfaceRelativeThresholdFilter {
                    heightmap: parse_heightmap(req_str(obj, "heightmap")?)?,
                    min_inclusive: opt_i32(obj, "min_inclusive")?.unwrap_or(i32::MIN),
                    max_inclusive: opt_i32(obj, "max_inclusive")?.unwrap_or(i32::MAX),
                })
            }
            "surface_water_depth_filter" => Ok(PlacementModifier::SurfaceWaterDepthFilter(
                req_i32(obj, "max_water_depth")?,
            )),
            other => Err(format!("unknown placement_modifier type '{other}'")),
        }
    }

    // -----------------------------------------------------------------------
    // All 58 Features
    // -----------------------------------------------------------------------

    fn decode_feature_holder(&mut self, json: &Json) -> Result<Arc<Feature>, String> {
        if let Some(s) = json.as_str() {
            let id = parse_id(s)?;
            return self.resolve_feature(&id).map_err(|e| e.to_string());
        }
        Ok(Arc::new(self.decode_feature(json)?))
    }

    fn decode_feature(&mut self, json: &Json) -> Result<Feature, String> {
        let obj = as_obj(json)?;
        let ty = req_type_path(obj)?;
        // In 26.3 features are flat; if a legacy {"type", "config": {...}} wrapper ever appears in tests, unwrap it.
        let cfg = obj.get("config").and_then(Json::as_object).unwrap_or(obj);
        match ty.as_str() {
            // Group A
            "no_op" => Ok(Feature::NoOp),
            "random_boolean_selector" => Ok(Feature::RandomBooleanSelector {
                feature_true: self.decode_placed_feature_holder(req_field(cfg, "feature_true")?)?,
                feature_false: self
                    .decode_placed_feature_holder(req_field(cfg, "feature_false")?)?,
            }),
            "random_selector" => {
                let mut features = Vec::new();
                for item in as_arr(req_field(cfg, "features")?)? {
                    let eobj = as_obj(item)?;
                    features.push((
                        self.decode_placed_feature_holder(req_field(eobj, "feature")?)?,
                        req_f32(eobj, "chance")?,
                    ));
                }
                let default = self.decode_placed_feature_holder(req_field(cfg, "default")?)?;
                Ok(Feature::RandomSelector { features, default })
            }
            "simple_random_selector" => Ok(Feature::SimpleRandomSelector {
                features: self.decode_placed_feature_holder_set(req_field(cfg, "features")?)?,
            }),
            "weighted_random_selector" => {
                let wl = decode_weighted_list(req_field(cfg, "features")?, |j| {
                    self.decode_placed_feature_holder(j)
                })?;
                Ok(Feature::WeightedRandomSelector { features: wl })
            }
            "overlay" => Ok(Feature::Overlay {
                features: self.decode_placed_feature_holder_set(req_field(cfg, "features")?)?,
            }),
            "sequence" => Ok(Feature::Sequence {
                features: self.decode_placed_feature_holder_set(req_field(cfg, "features")?)?,
            }),

            // Group B
            "ore" => Ok(Feature::Ore(self.decode_ore_config(cfg)?)),
            "scattered_ore" => Ok(Feature::ScatteredOre(self.decode_ore_config(cfg)?)),
            "replace_single_block" => Ok(Feature::ReplaceSingleBlock {
                targets: self.decode_block_replacements(req_field(cfg, "targets")?)?,
            }),
            "netherrack_replace_blobs" => {
                let target_state = self.decode_block_state(req_field(cfg, "target")?)?;
                Ok(Feature::NetherrackReplaceBlobs {
                    target_block_id: self.ops.block_id(target_state),
                    state: self.decode_block_state(req_field(cfg, "state")?)?,
                    radius: self.decode_int_provider(req_field(cfg, "radius")?)?,
                })
            }
            "disk" => Ok(Feature::Disk {
                state_provider: self.decode_bsp_holder(req_field(cfg, "state_provider")?)?,
                target: self.decode_block_predicate(req_field(cfg, "target")?)?,
                radius: self.decode_int_provider(req_field(cfg, "radius")?)?,
                half_height: req_i32(cfg, "half_height")?,
            }),
            "simple_block" => Ok(Feature::SimpleBlock {
                to_place: self.decode_bsp_holder(req_field(cfg, "to_place")?)?,
                schedule_tick: opt_bool(cfg, "schedule_tick")?.unwrap_or(false),
            }),
            "block_blob" => Ok(Feature::BlockBlob {
                state: self.decode_block_state(req_field(cfg, "state")?)?,
                can_place_on: self.decode_block_predicate(req_field(cfg, "can_place_on")?)?,
            }),
            "block_column" => {
                let mut layers = Vec::new();
                for item in as_arr(req_field(cfg, "layers")?)? {
                    let lobj = as_obj(item)?;
                    layers.push(BlockColumnLayer {
                        height: self.decode_int_provider(req_field(lobj, "height")?)?,
                        provider: self.decode_bsp_holder(req_field(lobj, "provider")?)?,
                    });
                }
                Ok(Feature::BlockColumn {
                    layers,
                    direction: parse_direction(req_str(cfg, "direction")?)?,
                    allowed_placement: self
                        .decode_block_predicate(req_field(cfg, "allowed_placement")?)?,
                    prioritize_tip: req_bool(cfg, "prioritize_tip")?,
                })
            }
            "block_pile" => Ok(Feature::BlockPile {
                state_provider: self.decode_bsp_holder(req_field(cfg, "state_provider")?)?,
            }),
            "fill_layer" => Ok(Feature::FillLayer {
                height: req_i32(cfg, "height")?,
                state: self.decode_block_state(req_field(cfg, "state")?)?,
            }),
            "projected_random_patchy_square" => Ok(Feature::ProjectedRandomPatchySquare {
                block: self.decode_bsp_holder(req_field(cfg, "block")?)?,
                project_through: self.decode_block_predicate(req_field(cfg, "project_through")?)?,
                size: self.decode_int_provider(req_field(cfg, "size")?)?,
                max_projection_height: req_i32(cfg, "max_projection_height")?,
            }),
            "random_neighbor_spread" => Ok(Feature::RandomNeighborSpread {
                block: self.decode_bsp_holder(req_field(cfg, "block")?)?,
                accepted_neighbors: self
                    .decode_block_holder_set(req_field(cfg, "accepted_neighbors")?)?,
                can_replace: self.decode_block_predicate(req_field(cfg, "can_replace")?)?,
                attempts: self.decode_int_provider(req_field(cfg, "attempts")?)?,
                xz_offset: self.decode_int_provider(req_field(cfg, "xz_offset")?)?,
                y_offset: self.decode_int_provider(req_field(cfg, "y_offset")?)?,
            }),
            "single_block_pillar" => Ok(Feature::SingleBlockPillar {
                block: self.decode_bsp_holder(req_field(cfg, "block")?)?,
                can_replace: match cfg.get("can_replace") {
                    Some(j) if !matches!(j, Json::Null) => self.decode_block_predicate(j)?,
                    _ => BlockPredicate::True,
                },
                direction: parse_direction(req_str(cfg, "direction")?)?,
                chance_to_continue: opt_f32(cfg, "chance_to_continue")?.unwrap_or(1.0),
                cap_feature: match cfg.get("cap_feature") {
                    Some(j) if !matches!(j, Json::Null) => {
                        Some(self.decode_placed_feature_holder(j)?)
                    }
                    _ => None,
                },
            }),
            "spring_feature" => {
                let (fluid_state, legacy_block_state) =
                    self.decode_fluid_state(req_field(cfg, "state")?)?;
                Ok(Feature::SpringFeature {
                    fluid_state,
                    legacy_block_state,
                    requires_block_below: opt_bool(cfg, "requires_block_below")?.unwrap_or(true),
                    rock_count: opt_i32(cfg, "rock_count")?.unwrap_or(4),
                    hole_count: opt_i32(cfg, "hole_count")?.unwrap_or(1),
                    valid_blocks: self.decode_block_holder_set(req_field(cfg, "valid_blocks")?)?,
                })
            }

            // Group C
            "bamboo" => Ok(Feature::Bamboo {
                probability: req_f32(cfg, "probability")?,
            }),
            "tree" => Ok(Feature::Tree {
                config: Box::new(self.decode_tree_config(cfg)?),
            }),
            "fallen_tree" => {
                let stump_decorators = match cfg.get("stump_decorators") {
                    Some(j) => self.decode_tree_decorators(j)?,
                    None => Vec::new(),
                };
                let log_decorators = match cfg.get("log_decorators") {
                    Some(j) => self.decode_tree_decorators(j)?,
                    None => Vec::new(),
                };
                Ok(Feature::FallenTree {
                    trunk_provider: self.decode_bsp_holder(req_field(cfg, "trunk_provider")?)?,
                    log_length: self.decode_int_provider(req_field(cfg, "log_length")?)?,
                    stump_decorators,
                    log_decorators,
                })
            }

            // Group D
            "iceberg" => Ok(Feature::Iceberg {
                state: self.decode_block_state(req_field(cfg, "state")?)?,
            }),
            "freeze_top_layer" => Ok(Feature::FreezeTopLayer),
            "blue_ice" => Ok(Feature::BlueIce),
            "spike" => Ok(Feature::Spike {
                state: self.decode_block_state(req_field(cfg, "state")?)?,
                can_place_on: self.decode_block_predicate(req_field(cfg, "can_place_on")?)?,
                can_replace: self.decode_block_predicate(req_field(cfg, "can_replace")?)?,
            }),
            "geode" => Ok(Feature::Geode(Box::new(self.decode_geode_config(cfg)?))),
            "large_dripstone" => Ok(Feature::LargeDripstone(Box::new(LargeDripstoneConfig {
                replaceable_blocks: self
                    .decode_block_holder_set(req_field(cfg, "replaceable_blocks")?)?,
                floor_to_ceiling_search_range: opt_i32(cfg, "floor_to_ceiling_search_range")?
                    .unwrap_or(30),
                column_radius: self.decode_int_provider(req_field(cfg, "column_radius")?)?,
                height_scale: self.decode_float_provider(req_field(cfg, "height_scale")?)?,
                max_column_radius_to_cave_height_ratio: req_f32(
                    cfg,
                    "max_column_radius_to_cave_height_ratio",
                )?,
                stalactite_bluntness: self
                    .decode_float_provider(req_field(cfg, "stalactite_bluntness")?)?,
                stalagmite_bluntness: self
                    .decode_float_provider(req_field(cfg, "stalagmite_bluntness")?)?,
                wind_speed: self.decode_float_provider(req_field(cfg, "wind_speed")?)?,
                min_radius_for_wind: req_i32(cfg, "min_radius_for_wind")?,
                min_bluntness_for_wind: req_f32(cfg, "min_bluntness_for_wind")?,
            }))),
            "root_system" => Ok(Feature::RootSystem(Box::new(RootSystemConfig {
                feature: self.decode_placed_feature_holder(req_field(cfg, "feature")?)?,
                required_vertical_space_for_tree: req_i32(cfg, "required_vertical_space_for_tree")?,
                level_test_distance: req_i32(cfg, "level_test_distance")?,
                max_level_deviation: req_i32(cfg, "max_level_deviation")?,
                root_radius: req_i32(cfg, "root_radius")?,
                root_replaceable: self
                    .decode_block_holder_set(req_field(cfg, "root_replaceable")?)?,
                root_state_provider: self
                    .decode_bsp_holder(req_field(cfg, "root_state_provider")?)?,
                root_placement_attempts: req_i32(cfg, "root_placement_attempts")?,
                root_column_max_height: req_i32(cfg, "root_column_max_height")?,
                hanging_root_radius: req_i32(cfg, "hanging_root_radius")?,
                hanging_roots_vertical_span: req_i32(cfg, "hanging_roots_vertical_span")?,
                hanging_root_state_provider: self
                    .decode_bsp_holder(req_field(cfg, "hanging_root_state_provider")?)?,
                hanging_root_placement_attempts: req_i32(cfg, "hanging_root_placement_attempts")?,
                allowed_vertical_water_for_tree: req_i32(cfg, "allowed_vertical_water_for_tree")?,
                allowed_tree_position: self
                    .decode_block_predicate(req_field(cfg, "allowed_tree_position")?)?,
            }))),
            "speleothem" => {
                let base_st = self.decode_block_state(req_field(cfg, "base_block")?)?;
                let pointed_st = self.decode_block_state(req_field(cfg, "pointed_block")?)?;
                let base_bid = self.ops.block_id(base_st);
                let pointed_bid = self.ops.block_id(pointed_st);
                Ok(Feature::Speleothem(Box::new(SpeleothemConfig {
                    base_block_id: base_bid,
                    base_default_state: self.ops.blocks.blocks()[base_bid as usize].default_state,
                    pointed_block_id: pointed_bid,
                    pointed_default_state: self.ops.blocks.blocks()[pointed_bid as usize]
                        .default_state,
                    replaceable_blocks: self
                        .decode_block_holder_set(req_field(cfg, "replaceable_blocks")?)?,
                    chance_of_taller_generation: opt_f32(cfg, "chance_of_taller_generation")?
                        .unwrap_or(0.2),
                    chance_of_directional_spread: opt_f32(cfg, "chance_of_directional_spread")?
                        .unwrap_or(0.7),
                    chance_of_spread_radius2: opt_f32(cfg, "chance_of_spread_radius2")?
                        .unwrap_or(0.5),
                    chance_of_spread_radius3: opt_f32(cfg, "chance_of_spread_radius3")?
                        .unwrap_or(0.5),
                })))
            }
            "speleothem_cluster" => {
                let base_st = self.decode_block_state(req_field(cfg, "base_block")?)?;
                let pointed_st = self.decode_block_state(req_field(cfg, "pointed_block")?)?;
                let base_bid = self.ops.block_id(base_st);
                let pointed_bid = self.ops.block_id(pointed_st);
                Ok(Feature::SpeleothemCluster(Box::new(
                    SpeleothemClusterConfig {
                        base_block_id: base_bid,
                        base_default_state: self.ops.blocks.blocks()[base_bid as usize]
                            .default_state,
                        pointed_block_id: pointed_bid,
                        pointed_default_state: self.ops.blocks.blocks()[pointed_bid as usize]
                            .default_state,
                        replaceable_blocks: self
                            .decode_block_holder_set(req_field(cfg, "replaceable_blocks")?)?,
                        floor_to_ceiling_search_range: req_i32(
                            cfg,
                            "floor_to_ceiling_search_range",
                        )?,
                        height: self.decode_int_provider(req_field(cfg, "height")?)?,
                        radius: self.decode_int_provider(req_field(cfg, "radius")?)?,
                        max_stalagmite_stalactite_height_diff: req_i32(
                            cfg,
                            "max_stalagmite_stalactite_height_diff",
                        )?,
                        height_deviation: req_i32(cfg, "height_deviation")?,
                        speleothem_block_layer_thickness: self.decode_int_provider(req_field(
                            cfg,
                            "speleothem_block_layer_thickness",
                        )?)?,
                        density: self.decode_float_provider(req_field(cfg, "density")?)?,
                        wetness: self.decode_float_provider(req_field(cfg, "wetness")?)?,
                        chance_of_speleothem_at_max_distance_from_center: req_f32(
                            cfg,
                            "chance_of_speleothem_at_max_distance_from_center",
                        )?,
                        max_distance_from_edge_affecting_chance_of_speleothem: req_i32(
                            cfg,
                            "max_distance_from_edge_affecting_chance_of_speleothem",
                        )?,
                        max_distance_from_center_affecting_height_bias: req_i32(
                            cfg,
                            "max_distance_from_center_affecting_height_bias",
                        )?,
                    },
                )))
            }

            // Group E
            "chorus_plant" => Ok(Feature::ChorusPlant),
            "vines" => Ok(Feature::Vines),
            "coral_claw" => Ok(Feature::CoralClaw {
                feature: self.decode_placed_feature_holder(req_field(cfg, "feature")?)?,
            }),
            "coral_tree" => Ok(Feature::CoralTree {
                feature: self.decode_placed_feature_holder(req_field(cfg, "feature")?)?,
            }),
            "stepped_column_cluster" => Ok(Feature::SteppedColumnCluster(Box::new(
                SteppedColumnClusterConfig {
                    block: self.decode_bsp_holder(req_field(cfg, "block")?)?,
                    continue_through: self
                        .decode_block_predicate(req_field(cfg, "continue_through")?)?,
                    can_replace: self.decode_block_predicate(req_field(cfg, "can_replace")?)?,
                    cannot_place_on: self
                        .decode_block_holder_set(req_field(cfg, "cannot_place_on")?)?,
                    cluster_reach: self.decode_int_provider(req_field(cfg, "cluster_reach")?)?,
                    column_count: self.decode_int_provider(req_field(cfg, "column_count")?)?,
                    column_reach: self.decode_int_provider(req_field(cfg, "column_reach")?)?,
                    height: self.decode_int_provider(req_field(cfg, "height")?)?,
                },
            ))),
            "delta_feature" => Ok(Feature::DeltaFeature {
                contents: self.decode_block_state(req_field(cfg, "contents")?)?,
                rim: self.decode_block_state(req_field(cfg, "rim")?)?,
                size: self.decode_int_provider(req_field(cfg, "size")?)?,
                rim_size: self.decode_int_provider(req_field(cfg, "rim_size")?)?,
            }),
            "underwater_magma" => Ok(Feature::UnderwaterMagma {
                floor_search_range: req_i32(cfg, "floor_search_range")?,
                placement_radius_around_floor: req_i32(cfg, "placement_radius_around_floor")?,
                placement_probability_per_valid_position: req_f32(
                    cfg,
                    "placement_probability_per_valid_position",
                )?,
            }),
            "sculk_patch" => Ok(Feature::SculkPatch {
                charge_count: req_i32(cfg, "charge_count")?,
                amount_per_charge: req_i32(cfg, "amount_per_charge")?,
                spread_attempts: req_i32(cfg, "spread_attempts")?,
                growth_rounds: req_i32(cfg, "growth_rounds")?,
                spread_rounds: req_i32(cfg, "spread_rounds")?,
            }),

            // Group F & G
            "fossil" => {
                let mut fossil_structures = Vec::new();
                for item in as_arr(req_field(cfg, "fossil_structures")?)? {
                    fossil_structures.push(parse_id(item.as_str().ok_or("expected id string")?)?);
                }
                let mut overlay_structures = Vec::new();
                for item in as_arr(req_field(cfg, "overlay_structures")?)? {
                    overlay_structures.push(parse_id(item.as_str().ok_or("expected id string")?)?);
                }
                Ok(Feature::Fossil(Box::new(FossilConfig {
                    fossil_structures,
                    overlay_structures,
                    fossil_processors: req_field(cfg, "fossil_processors")?.clone(),
                    overlay_processors: req_field(cfg, "overlay_processors")?.clone(),
                    max_empty_corners_allowed: req_i32(cfg, "max_empty_corners_allowed")?,
                })))
            }
            "template" => {
                let templates = decode_weighted_list(req_field(cfg, "templates")?, |j| {
                    let eobj = as_obj(j)?;
                    let id = parse_id(req_str(eobj, "id")?)?;
                    let rotations = match eobj.get("rotations") {
                        Some(rj) if !matches!(rj, Json::Null) => {
                            let mut rots = Vec::new();
                            for item in as_arr(rj)? {
                                let s = item.as_str().ok_or("expected rotation string")?;
                                rots.push(
                                    Rotation::from_name(s)
                                        .ok_or_else(|| format!("unknown rotation '{s}'"))?,
                                );
                            }
                            rots
                        }
                        _ => Rotation::ALL.to_vec(),
                    };
                    Ok(TemplateEntry { id, rotations })
                })?;
                let processors = cfg
                    .get("processors")
                    .filter(|j| !matches!(j, Json::Null))
                    .cloned();
                Ok(Feature::Template(Box::new(TemplateConfig {
                    templates,
                    processors,
                })))
            }
            "bonus_chest" => Ok(Feature::BonusChest),
            "monster_room" => Ok(Feature::MonsterRoom),
            "end_gateway" => {
                let exit = opt_vec3i(cfg, "exit")?.map(|v| BlockPos::new(v[0], v[1], v[2]));
                let exact = req_bool(cfg, "exact")?;
                Ok(Feature::EndGateway { exit, exact })
            }
            "end_island" => Ok(Feature::EndIsland),
            "end_platform" => Ok(Feature::EndPlatform),
            "end_podium" => Ok(Feature::EndPodium {
                active: opt_bool(cfg, "active")?.unwrap_or(false),
            }),
            "end_spike" => {
                let mut spikes = Vec::new();
                for item in as_arr(req_field(cfg, "spikes")?)? {
                    let sobj = as_obj(item)?;
                    spikes.push(EndSpike {
                        center_x: opt_i32(sobj, "centerX")?.unwrap_or(0),
                        center_z: opt_i32(sobj, "centerZ")?.unwrap_or(0),
                        radius: opt_i32(sobj, "radius")?.unwrap_or(0),
                        height: opt_i32(sobj, "height")?.unwrap_or(0),
                        guarded: opt_bool(sobj, "guarded")?.unwrap_or(false),
                    });
                }
                let crystal_invulnerable = opt_bool(cfg, "crystal_invulnerable")?.unwrap_or(false);
                let crystal_beam_target =
                    opt_vec3i(cfg, "crystal_beam_target")?.map(|v| BlockPos::new(v[0], v[1], v[2]));
                Ok(Feature::EndSpike {
                    spikes,
                    crystal_invulnerable,
                    crystal_beam_target,
                })
            }

            // Group H
            "lake" => Ok(Feature::Lake {
                fluid: self.decode_bsp_holder(req_field(cfg, "fluid")?)?,
                barrier: self.decode_bsp_holder(req_field(cfg, "barrier")?)?,
                can_place_feature: self
                    .decode_block_predicate(req_field(cfg, "can_place_feature")?)?,
                can_replace_with_air_or_fluid: self
                    .decode_block_predicate(req_field(cfg, "can_replace_with_air_or_fluid")?)?,
                can_replace_with_barrier: self
                    .decode_block_predicate(req_field(cfg, "can_replace_with_barrier")?)?,
            }),
            "huge_brown_mushroom" => Ok(Feature::HugeBrownMushroom(Box::new(
                self.decode_huge_mushroom_config(cfg)?,
            ))),
            "huge_red_mushroom" => Ok(Feature::HugeRedMushroom(Box::new(
                self.decode_huge_mushroom_config(cfg)?,
            ))),
            "huge_fungus" => {
                let base_st = self.decode_block_state(req_field(cfg, "valid_base_block")?)?;
                Ok(Feature::HugeFungus {
                    valid_base_block_id: self.ops.block_id(base_st),
                    stem_state: self.decode_block_state(req_field(cfg, "stem_state")?)?,
                    hat_state: self.decode_block_state(req_field(cfg, "hat_state")?)?,
                    decor_state: self.decode_block_state(req_field(cfg, "decor_state")?)?,
                    replaceable_blocks: self
                        .decode_block_predicate(req_field(cfg, "replaceable_blocks")?)?,
                    planted: opt_bool(cfg, "planted")?.unwrap_or(false),
                })
            }
            "vegetation_patch" => Ok(Feature::VegetationPatch(Box::new(
                self.decode_vegetation_patch_config(cfg)?,
            ))),
            "waterlogged_vegetation_patch" => Ok(Feature::WaterloggedVegetationPatch(Box::new(
                self.decode_vegetation_patch_config(cfg)?,
            ))),
            "void_start_platform" => Ok(Feature::VoidStartPlatform),
            "multiface_growth" => Ok(Feature::MultifaceGrowth {
                block_id: self.decode_block_id(req_field(cfg, "block")?)?,
                search_range: opt_i32(cfg, "search_range")?.unwrap_or(10),
                can_place_on_floor: opt_bool(cfg, "can_place_on_floor")?.unwrap_or(false),
                can_place_on_ceiling: opt_bool(cfg, "can_place_on_ceiling")?.unwrap_or(false),
                can_place_on_wall: opt_bool(cfg, "can_place_on_wall")?.unwrap_or(false),
                chance_of_spreading: opt_f32(cfg, "chance_of_spreading")?.unwrap_or(0.5),
                can_be_placed_on: self
                    .decode_block_holder_set(req_field(cfg, "can_be_placed_on")?)?,
            }),

            other => Err(format!("unknown feature type '{other}'")),
        }
    }

    fn decode_ore_config(&self, cfg: &JsonObject) -> Result<OreConfig, String> {
        Ok(OreConfig {
            targets: self.decode_block_replacements(req_field(cfg, "targets")?)?,
            size: req_i32(cfg, "size")?,
            discard_chance_on_air_exposure: opt_f32(cfg, "discard_chance_on_air_exposure")?
                .unwrap_or(0.0),
        })
    }

    fn decode_geode_config(&mut self, cfg: &JsonObject) -> Result<GeodeConfig, String> {
        let blocks = as_obj(req_field(cfg, "blocks")?)?;
        let layers = as_obj(req_field(cfg, "layers")?)?;
        let crack = as_obj(req_field(cfg, "crack")?)?;

        let outer_wall_distance = match cfg.get("outer_wall_distance") {
            Some(j) => self.decode_int_provider(j)?,
            None => IntProvider::Uniform {
                min_inclusive: 4,
                max_inclusive: 5,
            },
        };
        let distribution_points = match cfg.get("distribution_points") {
            Some(j) => self.decode_int_provider(j)?,
            None => IntProvider::Uniform {
                min_inclusive: 3,
                max_inclusive: 4,
            },
        };
        let point_offset = match cfg.get("point_offset") {
            Some(j) => self.decode_int_provider(j)?,
            None => IntProvider::Uniform {
                min_inclusive: 1,
                max_inclusive: 2,
            },
        };

        Ok(GeodeConfig {
            filling_provider: self.decode_bsp_holder(req_field(blocks, "filling_provider")?)?,
            inner_layer_provider: self
                .decode_bsp_holder(req_field(blocks, "inner_layer_provider")?)?,
            alternate_inner_layer_provider: self
                .decode_bsp_holder(req_field(blocks, "alternate_inner_layer_provider")?)?,
            middle_layer_provider: self
                .decode_bsp_holder(req_field(blocks, "middle_layer_provider")?)?,
            outer_layer_provider: self
                .decode_bsp_holder(req_field(blocks, "outer_layer_provider")?)?,
            inner_placements: self
                .decode_block_state_list(req_field(blocks, "inner_placements")?)?,
            cannot_replace: self.decode_block_holder_set(req_field(blocks, "cannot_replace")?)?,
            invalid_blocks: self.decode_block_holder_set(req_field(blocks, "invalid_blocks")?)?,
            filling: opt_f64(layers, "filling")?.unwrap_or(1.7),
            inner_layer: opt_f64(layers, "inner_layer")?.unwrap_or(2.2),
            middle_layer: opt_f64(layers, "middle_layer")?.unwrap_or(3.2),
            outer_layer: opt_f64(layers, "outer_layer")?.unwrap_or(4.2),
            generate_crack_chance: opt_f64(crack, "generate_crack_chance")?.unwrap_or(1.0),
            base_crack_size: opt_f64(crack, "base_crack_size")?.unwrap_or(2.0),
            crack_point_offset: opt_i32(crack, "crack_point_offset")?.unwrap_or(2),
            use_potential_placements_chance: opt_f64(cfg, "use_potential_placements_chance")?
                .unwrap_or(0.35),
            use_alternate_layer0_chance: opt_f64(cfg, "use_alternate_layer0_chance")?
                .unwrap_or(0.0),
            placements_require_layer0_alternate: opt_bool(
                cfg,
                "placements_require_layer0_alternate",
            )?
            .unwrap_or(true),
            outer_wall_distance,
            distribution_points,
            point_offset,
            min_gen_offset: opt_i32(cfg, "min_gen_offset")?.unwrap_or(-16),
            max_gen_offset: opt_i32(cfg, "max_gen_offset")?.unwrap_or(16),
            noise_multiplier: opt_f64(cfg, "noise_multiplier")?.unwrap_or(0.05),
            invalid_blocks_threshold: req_i32(cfg, "invalid_blocks_threshold")?,
        })
    }

    fn decode_huge_mushroom_config(
        &mut self,
        cfg: &JsonObject,
    ) -> Result<HugeMushroomConfig, String> {
        Ok(HugeMushroomConfig {
            cap_provider: self.decode_bsp_holder(req_field(cfg, "cap_provider")?)?,
            stem_provider: self.decode_bsp_holder(req_field(cfg, "stem_provider")?)?,
            foliage_radius: opt_i32(cfg, "foliage_radius")?.unwrap_or(2),
            can_place_on: self.decode_block_predicate(req_field(cfg, "can_place_on")?)?,
        })
    }

    fn decode_vegetation_patch_config(
        &mut self,
        cfg: &JsonObject,
    ) -> Result<VegetationPatchConfig, String> {
        let surface = match req_str(cfg, "surface")? {
            "floor" => CaveSurface::Floor,
            "ceiling" => CaveSurface::Ceiling,
            other => return Err(format!("unknown CaveSurface '{other}'")),
        };
        Ok(VegetationPatchConfig {
            replaceable: self.decode_block_holder_set(req_field(cfg, "replaceable")?)?,
            ground_state: self.decode_bsp_holder(req_field(cfg, "ground_state")?)?,
            vegetation_feature: self
                .decode_placed_feature_holder(req_field(cfg, "vegetation_feature")?)?,
            surface,
            depth: self.decode_int_provider(req_field(cfg, "depth")?)?,
            extra_bottom_block_chance: req_f32(cfg, "extra_bottom_block_chance")?,
            vertical_range: req_i32(cfg, "vertical_range")?,
            vegetation_chance: req_f32(cfg, "vegetation_chance")?,
            xz_radius: self.decode_int_provider(req_field(cfg, "xz_radius")?)?,
            extra_edge_column_chance: req_f32(cfg, "extra_edge_column_chance")?,
        })
    }
}

// ---------------------------------------------------------------------------
// JSON helper utilities
// ---------------------------------------------------------------------------

fn parse_id(s: &str) -> Result<Identifier, String> {
    Identifier::parse(s).map_err(|e| format!("invalid identifier '{s}': {e}"))
}

fn as_obj(json: &Json) -> Result<&JsonObject, String> {
    json.as_object()
        .ok_or_else(|| format!("expected JSON object, got {}", json.type_name()))
}

fn as_arr(json: &Json) -> Result<&[Json], String> {
    json.as_array()
        .ok_or_else(|| format!("expected JSON array, got {}", json.type_name()))
}

fn req_field<'a>(obj: &'a JsonObject, key: &str) -> Result<&'a Json, String> {
    obj.get(key)
        .ok_or_else(|| format!("missing required field '{key}'"))
}

fn req_str<'a>(obj: &'a JsonObject, key: &str) -> Result<&'a str, String> {
    req_field(obj, key)?
        .as_str()
        .ok_or_else(|| format!("field '{key}' must be a string"))
}

fn opt_str<'a>(obj: &'a JsonObject, key: &str) -> Result<Option<&'a str>, String> {
    match obj.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(v) => v
            .as_str()
            .map(Some)
            .ok_or_else(|| format!("field '{key}' must be a string")),
    }
}

fn req_type_path(obj: &JsonObject) -> Result<String, String> {
    let s = req_str(obj, "type")?;
    let id = parse_id(s)?;
    Ok(id.path().to_owned())
}

fn req_bool(obj: &JsonObject, key: &str) -> Result<bool, String> {
    match req_field(obj, key)? {
        Json::Bool(b) => Ok(*b),
        _ => Err(format!("field '{key}' must be a bool")),
    }
}

fn opt_bool(obj: &JsonObject, key: &str) -> Result<Option<bool>, String> {
    match obj.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::Bool(b)) => Ok(Some(*b)),
        _ => Err(format!("field '{key}' must be a bool")),
    }
}

fn json_to_i32(json: &Json, ctx: &str) -> Result<i32, String> {
    match json {
        Json::Number(n) => n.to_i32().map_err(|e| format!("{ctx}: {e}")),
        _ => Err(format!("{ctx} must be an integer")),
    }
}

fn json_to_i64(json: &Json, ctx: &str) -> Result<i64, String> {
    match json {
        Json::Number(n) => n.to_i64().map_err(|e| format!("{ctx}: {e}")),
        _ => Err(format!("{ctx} must be an integer")),
    }
}

fn json_to_f32(json: &Json, ctx: &str) -> Result<f32, String> {
    match json {
        Json::Number(n) => Ok(n.to_f32()),
        _ => Err(format!("{ctx} must be a number")),
    }
}

fn json_to_f64(json: &Json, ctx: &str) -> Result<f64, String> {
    match json {
        Json::Number(n) => Ok(n.to_f64()),
        _ => Err(format!("{ctx} must be a number")),
    }
}

fn req_i32(obj: &JsonObject, key: &str) -> Result<i32, String> {
    json_to_i32(req_field(obj, key)?, key)
}

fn opt_i32(obj: &JsonObject, key: &str) -> Result<Option<i32>, String> {
    match obj.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(v) => Ok(Some(json_to_i32(v, key)?)),
    }
}

fn req_i64(obj: &JsonObject, key: &str) -> Result<i64, String> {
    json_to_i64(req_field(obj, key)?, key)
}

fn req_f32(obj: &JsonObject, key: &str) -> Result<f32, String> {
    json_to_f32(req_field(obj, key)?, key)
}

fn opt_f32(obj: &JsonObject, key: &str) -> Result<Option<f32>, String> {
    match obj.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(v) => Ok(Some(json_to_f32(v, key)?)),
    }
}

fn req_f64(obj: &JsonObject, key: &str) -> Result<f64, String> {
    json_to_f64(req_field(obj, key)?, key)
}

fn opt_f64(obj: &JsonObject, key: &str) -> Result<Option<f64>, String> {
    match obj.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(v) => Ok(Some(json_to_f64(v, key)?)),
    }
}

fn decode_vec3i(json: &Json) -> Result<[i32; 3], String> {
    let arr = as_arr(json)?;
    if arr.len() != 3 {
        return Err(format!("expected 3-element vec3i, got {}", arr.len()));
    }
    Ok([
        json_to_i32(&arr[0], "vec3i[0]")?,
        json_to_i32(&arr[1], "vec3i[1]")?,
        json_to_i32(&arr[2], "vec3i[2]")?,
    ])
}

fn opt_vec3i(obj: &JsonObject, key: &str) -> Result<Option<[i32; 3]>, String> {
    match obj.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(j) => Ok(Some(decode_vec3i(j)?)),
    }
}

fn decode_inclusive_range(json: &Json) -> Result<(i32, i32), String> {
    if let Some(arr) = json.as_array()
        && arr.len() == 2
    {
        return Ok((
            json_to_i32(&arr[0], "range[0]")?,
            json_to_i32(&arr[1], "range[1]")?,
        ));
    }
    if let Some(obj) = json.as_object() {
        return Ok((
            req_i32(obj, "min_inclusive")?,
            req_i32(obj, "max_inclusive")?,
        ));
    }
    Err(format!("expected InclusiveRange, got {}", json.type_name()))
}

fn decode_weighted_list<T>(
    json: &Json,
    mut decode_elem: impl FnMut(&Json) -> Result<T, String>,
) -> Result<WeightedList<T>, String> {
    let arr = as_arr(json)?;
    let mut entries = Vec::with_capacity(arr.len());
    for item in arr {
        let obj = as_obj(item)?;
        let data = decode_elem(req_field(obj, "data")?)?;
        let weight = req_i32(obj, "weight")?;
        entries.push((data, weight));
    }
    Ok(WeightedList::new(entries))
}

fn parse_direction(s: &str) -> Result<Direction, String> {
    Direction::from_name(s).ok_or_else(|| format!("unknown direction '{s}'"))
}

fn parse_heightmap(s: &str) -> Result<HeightmapKind, String> {
    match s {
        "WORLD_SURFACE_WG" => Ok(HeightmapKind::WorldSurfaceWg),
        "WORLD_SURFACE" => Ok(HeightmapKind::WorldSurface),
        "OCEAN_FLOOR_WG" => Ok(HeightmapKind::OceanFloorWg),
        "OCEAN_FLOOR" => Ok(HeightmapKind::OceanFloor),
        "MOTION_BLOCKING" => Ok(HeightmapKind::MotionBlocking),
        "MOTION_BLOCKING_NO_LEAVES" => Ok(HeightmapKind::MotionBlockingNoLeaves),
        other => Err(format!("unknown heightmap '{other}'")),
    }
}

fn decode_normal_noise_params(json: &Json) -> Result<NormalNoiseParameters, String> {
    let obj = as_obj(json)?;
    if obj.contains_key("firstOctave") {
        // Pre-26.3 fallback format
        let first_octave = req_i32(obj, "firstOctave")?;
        let mut amps = Vec::new();
        for item in as_arr(req_field(obj, "amplitudes")?)? {
            amps.push(json_to_f64(item, "amplitude")?);
        }
        return NormalNoiseParameters::from_octaves(first_octave, amps, NormalizationMode::Enabled)
            .map_err(|e| e.to_string());
    }
    let base_amplitude = opt_f64(obj, "base_amplitude")?.unwrap_or(1.0);
    let base_octave = req_i32(obj, "base_octave")?;
    let octave_count = opt_i32(obj, "octave_count")?.unwrap_or(1) as usize;
    let normalize = match obj.get("normalize") {
        None | Some(Json::Null) | Some(Json::Bool(true)) => NormalizationMode::Enabled,
        Some(Json::Bool(false)) => NormalizationMode::Disabled,
        Some(j) if j.as_str() == Some("legacy") => NormalizationMode::Legacy,
        Some(other) => return Err(format!("invalid normalize mode: {other:?}")),
    };
    let mut amplitude_modifiers = Vec::new();
    if let Some(arr) = obj.get("amplitude_modifiers") {
        for item in as_arr(arr)? {
            amplitude_modifiers.push(json_to_f64(item, "amplitude_modifier")?);
        }
    }
    NormalNoiseParameters::new(
        base_amplitude,
        base_octave,
        octave_count,
        normalize,
        amplitude_modifiers,
    )
    .map_err(|e| e.to_string())
}
