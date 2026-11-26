//! `golden-features` parity suite: full `features`-status chunks (blocks, biomes, 6 heightmaps,
//! post-processing marks, tick counts) with `generate-structures=false` across Nether, End,
//! tree-free Overworld biomes and forest-type Overworld biomes (trees, fallen trees, decorators)
//! vs the user's own vanilla 26.3 server. Fossil / template features run through the structure
//! template placement of `zenith-structures`.
//!
//! `golden-structures` runs the same comparison with `generate-structures=true`: chunks around
//! every structure type (pieces placed per decoration step), fossils and template features.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::Context;
use zenith_biome::{
    BiomeSource, ClimateSampler, ClimateVolume, MultiNoiseBiomeSource, MultiNoisePreset,
    SearchState, TheEndBiomeSource,
};
use zenith_blocks::{BlockFactTags, BlockFacts};
use zenith_codec::log::null_log;
use zenith_codec::resources::ZipPackResources;
use zenith_data::Tables;
use zenith_features::{
    BlockOps, BlockPos, FeatureData, FeatureDecorator, FossilConfig, GenerationRegion, RegionChunk,
    StructureInterleaveHook, StructurePlacementHook, StructureTemplateHook, TemplateConfig,
    TemplateFeatureHook, WorldgenRandom,
};
use zenith_parity::SuiteResult;
use zenith_registry::{BlockStateTable, Identifier};
use zenith_structures::{BoundingBoxExt, StructureGenerator};
use zenith_worldgen::{
    BlockPredicates, BlockTags, DensityVolume, HeightmapKind, NoiseGenerator, RandomState,
    RouterField, TerrainChunk, TerrainGenerator, WorldgenData,
};

use crate::oracle::{ChunkRequest, ChunkWorld, DumpedChunk, dump_chunks, parse_chunk_dump};
use crate::repo_root;

const GOLDEN_SEEDS: &[i64] = &[12345, 0, -8_472_910_384_721];

/// Forest-type Overworld chunks per seed (tree / fallen-tree / tree-decorator coverage).
const FOREST_CHUNKS_PER_SEED: usize = 32;

/// Overworld land biomes picked first for the forest-type corpus (rarest tree configurations first).
const FOREST_PRIORITY_BIOMES: &[&str] = &[
    "pale_garden",
    "dark_forest",
    "mangrove_swamp",
    "swamp",
    "cherry_grove",
    "bamboo_jungle",
    "jungle",
    "sparse_jungle",
    "old_growth_pine_taiga",
    "old_growth_spruce_taiga",
    "taiga",
    "snowy_taiga",
    "birch_forest",
    "old_growth_birch_forest",
    "flower_forest",
    "forest",
    "dappled_forest",
    "savanna",
    "savanna_plateau",
    "windswept_savanna",
    "windswept_forest",
    "windswept_hills",
    "windswept_gravelly_hills",
    "plains",
    "sunflower_plains",
    "meadow",
    "grove",
];

/// Surface biomes in Overworld that are tree-free (or only have rare water trees / fossils /
/// desert wells that almost never trigger in a given chunk, verified by [`StubCallCounter`]).
const OVERWORLD_TREE_FREE_SURFACE_BIOMES: &[&str] = &[
    "minecraft:desert",
    "minecraft:badlands",
    "minecraft:eroded_badlands",
    "minecraft:ice_spikes",
    "minecraft:mushroom_fields",
    "minecraft:frozen_ocean",
    "minecraft:deep_frozen_ocean",
    "minecraft:stony_peaks",
    "minecraft:jagged_peaks",
    "minecraft:frozen_peaks",
    "minecraft:snowy_slopes",
    "minecraft:stony_shore",
    "minecraft:beach",
    "minecraft:snowy_beach",
    "minecraft:ocean",
    "minecraft:deep_ocean",
    "minecraft:cold_ocean",
    "minecraft:deep_cold_ocean",
    "minecraft:lukewarm_ocean",
    "minecraft:deep_lukewarm_ocean",
    "minecraft:river",
    "minecraft:frozen_river",
];

/// High-value Overworld biomes (rare or feature-rich: ice spikes, icebergs, huge mushrooms,
/// deserts, badlands, mountain peaks, dripstone caves, deep dark) prioritized during chunk picking.
const OVERWORLD_PRIORITY_BIOMES: &[&str] = &[
    "minecraft:ice_spikes",
    "minecraft:desert",
    "minecraft:badlands",
    "minecraft:eroded_badlands",
    "minecraft:mushroom_fields",
    "minecraft:frozen_ocean",
    "minecraft:deep_frozen_ocean",
    "minecraft:frozen_peaks",
    "minecraft:jagged_peaks",
    "minecraft:stony_peaks",
    "minecraft:snowy_slopes",
    "minecraft:dripstone_caves",
    "minecraft:deep_dark",
];

/// Counts calls into (and placements by) the fossil / template feature hooks.
#[derive(Debug)]
struct TemplateCounter {
    inner: StructureTemplateHook,
    calls: AtomicUsize,
    placed: AtomicUsize,
    fossils: AtomicUsize,
    templates: AtomicUsize,
}

impl TemplateCounter {
    fn new(inner: StructureTemplateHook) -> Self {
        Self {
            inner,
            calls: AtomicUsize::new(0),
            placed: AtomicUsize::new(0),
            fossils: AtomicUsize::new(0),
            templates: AtomicUsize::new(0),
        }
    }

    fn reset(&self) {
        self.calls.store(0, Ordering::Relaxed);
        self.placed.store(0, Ordering::Relaxed);
        self.fossils.store(0, Ordering::Relaxed);
        self.templates.store(0, Ordering::Relaxed);
    }

    fn fossil_placed(&self) -> usize {
        self.fossils.load(Ordering::Relaxed)
    }

    fn template_placed(&self) -> usize {
        self.templates.load(Ordering::Relaxed)
    }

    fn placed(&self) -> usize {
        self.placed.load(Ordering::Relaxed)
    }
}

impl TemplateFeatureHook for TemplateCounter {
    fn place_fossil(
        &self,
        config: &FossilConfig,
        region: &mut GenerationRegion,
        random: &mut WorldgenRandom,
        origin: BlockPos,
    ) -> bool {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let ok = self.inner.place_fossil(config, region, random, origin);
        self.placed.fetch_add(usize::from(ok), Ordering::Relaxed);
        self.fossils.fetch_add(usize::from(ok), Ordering::Relaxed);
        ok
    }

    fn place_template(
        &self,
        config: &TemplateConfig,
        region: &mut GenerationRegion,
        random: &mut WorldgenRandom,
        origin: BlockPos,
    ) -> bool {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let ok = self.inner.place_template(config, region, random, origin);
        self.placed.fetch_add(usize::from(ok), Ordering::Relaxed);
        self.templates.fetch_add(usize::from(ok), Ordering::Relaxed);
        ok
    }
}

/// Biomes that have neither `minecraft:tree` nor `minecraft:fallen_tree` features in their biome
/// feature lists (checked dynamically against the loaded 26.3 datapack, plus surface-biome
/// filtering for 3×3 neighborhoods).
fn biome_is_tree_and_template_free(data: &FeatureData, biome_id: &Identifier) -> bool {
    let Some(settings) = data.biome(biome_id) else {
        return false;
    };
    for step in &settings.features {
        for pf in step {
            if feature_uses_tree(&pf.feature) {
                return false;
            }
        }
    }
    true
}

/// Whether `feat` (or any sub-feature it can place) is a `tree` / `fallen_tree`.
fn feature_uses_tree(feat: &zenith_features::Feature) -> bool {
    use zenith_features::Feature;
    feature_any(feat, &|f| {
        matches!(f, Feature::Tree { .. } | Feature::FallenTree { .. })
    })
}

/// Recursive "any (sub-)feature matches `pred`" over the combinator features.
fn feature_any(
    feat: &zenith_features::Feature,
    pred: &dyn Fn(&zenith_features::Feature) -> bool,
) -> bool {
    use zenith_features::Feature;
    if pred(feat) {
        return true;
    }
    match feat {
        Feature::RandomBooleanSelector {
            feature_true,
            feature_false,
        } => feature_any(&feature_true.feature, pred) || feature_any(&feature_false.feature, pred),
        Feature::RandomSelector { features, default } => {
            features.iter().any(|(f, _)| feature_any(&f.feature, pred))
                || feature_any(&default.feature, pred)
        }
        Feature::SimpleRandomSelector { features }
        | Feature::Overlay { features }
        | Feature::Sequence { features } => features.iter().any(|f| feature_any(&f.feature, pred)),
        Feature::WeightedRandomSelector { features } => features
            .entries()
            .iter()
            .any(|(f, _)| feature_any(&f.feature, pred)),
        Feature::SingleBlockPillar { cap_feature, .. } => cap_feature
            .as_ref()
            .is_some_and(|f| feature_any(&f.feature, pred)),
        Feature::RootSystem(cfg) => feature_any(&cfg.feature.feature, pred),
        Feature::CoralClaw { feature } | Feature::CoralTree { feature } => {
            feature_any(&feature.feature, pred)
        }
        Feature::VegetationPatch(cfg) | Feature::WaterloggedVegetationPatch(cfg) => {
            feature_any(&cfg.vegetation_feature.feature, pred)
        }
        _ => false,
    }
}

/// Whether `biome`'s feature lists can place trees or fallen trees.
fn biome_has_trees(data: &FeatureData, biome_id: &Identifier) -> bool {
    data.biome(biome_id).is_some_and(|settings| {
        settings
            .features
            .iter()
            .flatten()
            .any(|pf| feature_uses_tree(&pf.feature))
    })
}

/// Reporting group of an Overworld surface biome (name based, so new biomes still land somewhere).
fn biome_group(biome: &str) -> &'static str {
    let b = biome.strip_prefix("minecraft:").unwrap_or(biome);
    if b.contains("cherry") || b == "meadow" || b == "grove" {
        "cherry/meadow/grove"
    } else if b.contains("pale") || b == "dark_forest" {
        "dark/pale forest"
    } else if b.contains("birch") {
        "birch"
    } else if b.contains("jungle") {
        "jungle"
    } else if b.contains("savanna") {
        "savanna"
    } else if b.contains("swamp") || b.contains("mangrove") {
        "swamp/mangrove"
    } else if b.contains("taiga") {
        "taiga"
    } else if b.contains("windswept") {
        "windswept"
    } else if b.contains("forest") {
        "forest"
    } else if b.contains("plains") {
        "plains"
    } else if OVERWORLD_TREE_FREE_SURFACE_BIOMES.contains(&biome) {
        "tree-free overworld"
    } else {
        "other overworld"
    }
}

#[derive(Default)]
struct Tally {
    passed: u64,
    total: u64,
    /// Blocks the features step changed vs. the terrain-only chunk (proves the suite exercises it).
    changed_blocks: u64,
    failures: Vec<String>,
    /// Maximum number of failure messages kept (`0` = the default of 24).
    failure_cap: usize,
    /// Differing blocks listed per chunk in a blocks failure (`0` = the default of 12).
    diff_limit: usize,
    /// Per reporting group: `(chunks fully identical, chunks compared)`.
    groups: BTreeMap<String, (u64, u64)>,
}

impl Tally {
    fn check(&mut self, ok: bool, what: impl FnOnce() -> String) {
        self.total += 1;
        if ok {
            self.passed += 1;
        } else if self.failures.len()
            < if self.failure_cap == 0 {
                24
            } else {
                self.failure_cap
            }
        {
            self.failures.push(what());
        }
    }

    fn merge(&mut self, other: Tally) {
        self.passed += other.passed;
        self.total += other.total;
        self.changed_blocks += other.changed_blocks;
        self.failures.extend(other.failures);
        self.failure_cap = self.failure_cap.max(other.failure_cap);
        self.diff_limit = self.diff_limit.max(other.diff_limit);
        for (g, (p, t)) in other.groups {
            let e = self.groups.entry(g).or_default();
            e.0 += p;
            e.1 += t;
        }
    }
}

#[derive(Default)]
struct Timing {
    vanilla_ms: u128,
    terrain_us: u128,
    features_us: u128,
    chunks: usize,
}

struct FeatureContextData {
    worldgen: WorldgenData,
    features: Arc<FeatureData>,
    blocks: Arc<BlockStateTable>,
    tags: Arc<BlockTags>,
    ops: Arc<BlockOps>,
    structure_data: Arc<zenith_structures::StructureData>,
    templates: Arc<zenith_structures::TemplateManager>,
    facts: Arc<BlockFacts>,
}

fn load_context(version: &str) -> anyhow::Result<FeatureContextData> {
    let install = zenith_install::discover_installs(Some(version))
        .into_iter()
        .next()
        .context("no install found")?;
    let tables_path = zenith_install::cache_dir()
        .join("versions")
        .join(version)
        .join("tables.bin");
    let tables =
        Tables::load(&tables_path).with_context(|| format!("load {}", tables_path.display()))?;
    let blocks = Arc::new(
        BlockStateTable::from_tables(&tables).map_err(|e| anyhow::anyhow!("block table: {e}"))?,
    );
    let pack = ZipPackResources::open("vanilla", &install.client_jar, null_log());
    let worldgen =
        WorldgenData::load(&pack).map_err(|e| anyhow::anyhow!("load worldgen data: {e}"))?;
    let tags = Arc::new(
        BlockTags::load(&pack, &blocks).map_err(|e| anyhow::anyhow!("load block tags: {e}"))?,
    );
    let fact_tags = BlockFactTags::from_resolver(&blocks, |id| {
        let ident = Identifier::parse(id).ok()?;
        tags.block_ids(&ident)
    });
    let facts = Arc::new(
        BlockFacts::build(&blocks, &fact_tags)
            .map_err(|e| anyhow::anyhow!("build block facts: {e}"))?,
    );
    let predicates = Arc::new(
        BlockPredicates::new(&blocks, &tags)
            .map_err(|id| anyhow::anyhow!("missing predicate block {id}"))?,
    );
    let structure_data = Arc::new(
        zenith_structures::StructureData::load(&pack, worldgen.biome_tags())
            .map_err(|e| anyhow::anyhow!("load structure data: {e}"))?,
    );
    let templates = Arc::new(zenith_structures::TemplateManager::new(
        Arc::new(ZipPackResources::open(
            "vanilla",
            &install.client_jar,
            null_log(),
        )),
        Arc::clone(&blocks),
        Some(Arc::clone(&facts)),
    ));
    let ops = Arc::new(BlockOps::new(
        Arc::clone(&blocks),
        Arc::clone(&facts),
        Arc::clone(&tags),
        predicates,
    ));
    let features = Arc::new(
        FeatureData::load(&pack, &ops).map_err(|e| anyhow::anyhow!("load feature data: {e}"))?,
    );

    Ok(FeatureContextData {
        worldgen,
        features,
        blocks,
        tags,
        ops,
        structure_data,
        templates,
        facts,
    })
}

fn describe_state(blocks: &BlockStateTable, state_id: u32) -> String {
    let Some(st) = blocks.state_by_id_exact(state_id as i32) else {
        return format!("?{state_id}");
    };
    if st.properties.is_empty() {
        return st.block_id.to_string();
    }
    let props: Vec<String> = st
        .properties
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    format!("{}[{}]", st.block_id, props.join(","))
}

fn heightmap_kind_from_name(name: &str) -> Option<HeightmapKind> {
    match name {
        "OCEAN_FLOOR_WG" => Some(HeightmapKind::OceanFloorWg),
        "WORLD_SURFACE_WG" => Some(HeightmapKind::WorldSurfaceWg),
        "OCEAN_FLOOR" => Some(HeightmapKind::OceanFloor),
        "WORLD_SURFACE" => Some(HeightmapKind::WorldSurface),
        "MOTION_BLOCKING" => Some(HeightmapKind::MotionBlocking),
        "MOTION_BLOCKING_NO_LEAVES" => Some(HeightmapKind::MotionBlockingNoLeaves),
        _ => None,
    }
}

/// Checks that `(cx, cz)` is at least Chebyshev distance 3 from every already chosen chunk so
/// vanilla's 3×3 feature write zone never causes one requested chunk to write into another.
fn well_spaced(chosen: &[(i32, i32)], cx: i32, cz: i32) -> bool {
    chosen
        .iter()
        .all(|&(ox, oz)| (cx - ox).abs().max((cz - oz).abs()) >= 3)
}

/// Fast 3D biome palette collection for a single chunk without generating terrain blocks.
/// Also returns the chunk's surface biome (at `y = 64` or top quart).
fn sample_chunk_biome_palette(
    biome_source: &BiomeSource,
    rs: &RandomState,
    min_y: i32,
    height: i32,
    cx: i32,
    cz: i32,
) -> (Vec<Identifier>, Identifier) {
    let qh = (height >> 2) as usize;
    let min_qy = min_y >> 2;
    let mut seen: HashSet<Identifier> = HashSet::new();
    let mut palette: Vec<Identifier> = Vec::new();
    let mut surface_biome = Identifier::with_default_namespace("plains").expect("valid");
    let surface_gy = ((64 - min_y).clamp(0, height - 1) >> 2) as usize;
    match biome_source {
        BiomeSource::Fixed(src) => {
            palette.push(src.biome.clone());
            surface_biome = src.biome.clone();
        }
        BiomeSource::Checkerboard(src) => {
            for lx in 0..4 {
                for lz in 0..4 {
                    let id = src.noise_biome((cx << 2) + lx, (cz << 2) + lz).clone();
                    if lx == 2 && lz == 2 {
                        surface_biome = id.clone();
                    }
                    if seen.insert(id.clone()) {
                        palette.push(id);
                    }
                }
            }
        }
        BiomeSource::TheEnd(src) => {
            let rep_qx = cx << 2;
            let rep_qz = cz << 2;
            for gy in 0..qh {
                let qy = min_qy + gy as i32;
                let id = src.noise_biome_with_erosion(rep_qx, qy, rep_qz, |bx, by, bz| {
                    rs.sample_router(RouterField::Erosion, bx, by, bz)
                });
                if gy == surface_gy {
                    surface_biome = id.clone();
                }
                if seen.insert(id.clone()) {
                    palette.push(id);
                }
            }
        }
        BiomeSource::MultiNoise(src) => {
            let vol = DensityVolume::new([4, qh, 4], [cx << 4, min_y, cz << 4], [4, 4, 4])
                .expect("valid climate volume");
            let channels = rs.sample_climate_volume(vol);
            let climate_vol = ClimateVolume::new(cx << 2, min_qy, cz << 2, 4, qh, 4, channels)
                .expect("valid climate volume");
            let mut state = SearchState::new();
            let param_list = src.parameter_list();
            for lx in 0..4usize {
                for gy in 0..qh {
                    for lz in 0..4usize {
                        let target = climate_vol.sample(
                            (cx << 2) + lx as i32,
                            min_qy + gy as i32,
                            (cz << 2) + lz as i32,
                        );
                        let id = param_list.find_value(target, &mut state);
                        if lx == 2 && lz == 2 && gy == surface_gy {
                            surface_biome = id.clone();
                        }
                        if seen.insert(id.clone()) {
                            palette.push(id.clone());
                        }
                    }
                }
            }
        }
    }
    (palette, surface_biome)
}

fn is_overworld_tree_free_surface(biome: &Identifier) -> bool {
    let s = biome.to_string();
    OVERWORLD_TREE_FREE_SURFACE_BIOMES.contains(&s.as_str())
}

fn is_overworld_priority_biome(biome: &Identifier) -> bool {
    let s = biome.to_string();
    OVERWORLD_PRIORITY_BIOMES.contains(&s.as_str())
}

/// Fast single-point biome probe for Overworld scanning (surface `qy = 20` / underground `qy = 0, -8`).
fn quick_overworld_probe(
    biome_source: &BiomeSource,
    rs: &RandomState,
    cx: i32,
    cz: i32,
) -> Option<(Identifier, Option<Identifier>)> {
    let BiomeSource::MultiNoise(src) = biome_source else {
        return None;
    };
    let bx = (cx << 4) + 8;
    let bz = (cz << 4) + 8;
    let qx = bx >> 2;
    let qz = bz >> 2;
    let param_list = src.parameter_list();
    let mut state = SearchState::new();
    let sample_at = |qy: i32, st: &mut SearchState| -> Identifier {
        let vol = DensityVolume::new([1, 1, 1], [bx, qy << 2, bz], [4, 4, 4]).expect("valid");
        let channels = rs.sample_climate_volume(vol);
        let cv = ClimateVolume::new(qx, qy, qz, 1, 1, 1, channels).expect("valid");
        param_list.find_value(cv.sample(qx, qy, qz), st).clone()
    };
    let surface = sample_at(20, &mut state); // y = 80
    if !is_overworld_tree_free_surface(&surface) {
        return None;
    }
    let cave_mid = sample_at(0, &mut state); // y = 0
    let cave_deep = sample_at(-8, &mut state); // y = -32
    let underground = if cave_deep.to_string() == "minecraft:deep_dark" {
        Some(cave_deep)
    } else if cave_mid.to_string() == "minecraft:dripstone_caves" {
        Some(cave_mid)
    } else {
        None
    };
    Some((surface, underground))
}

/// Chosen chunks, their terrain-chunk neighbourhoods, and log lines.
type Selection = (
    Vec<(i32, i32)>,
    HashMap<(i32, i32), Vec<TerrainChunk>>,
    Vec<String>,
);

/// Picks `target` well-spaced chunks in a dimension that do not invoke any unimplemented tree,
/// fallen-tree, fossil, or template feature hooks, prioritizing distinct surface biomes.
fn select_chunks_for_dim(
    pipe: &DimPipeline,
    data: &FeatureData,
    seed: i64,
    dim: &str,
    target: usize,
) -> Selection {
    let mut chosen: Vec<(i32, i32)> = Vec::new();
    let mut chosen_biomes: Vec<String> = Vec::new();
    let mut seen_biomes: HashSet<Identifier> = HashSet::new();
    let mut cached_3x3: HashMap<(i32, i32), Vec<TerrainChunk>> = HashMap::new();
    let mut rng =
        crate::parity_terrain::Lcg::new((seed as u64) ^ (dim.len() as u64) << 40 ^ 0xF3A7_CAFE);

    let mut candidates: Vec<(i32, i32)> = Vec::new();
    if dim == "minecraft:the_end" {
        for &(cx, cz) in &[
            (2, 0),
            (-2, 2),
            (25, 25),
            (-30, 28),
            (66, 0),
            (72, 12),
            (80, -24),
            (-75, 40),
            (95, 95),
            (-110, -85),
            (130, 50),
            (-68, -68),
            (150, -120),
            (-140, 110),
        ] {
            candidates.push((cx, cz));
        }
    }
    if dim == "minecraft:overworld" {
        // Fast single-point probe across 4000 coords to find rare tree-free & underground biomes
        // (ice_spikes, desert, badlands, mushroom_fields, frozen_ocean, peaks, dripstone_caves, deep_dark).
        let mut prio_seen: HashSet<Identifier> = HashSet::new();
        let mut prio_cands: Vec<(i32, i32)> = Vec::new();
        let mut regular_cands: Vec<(i32, i32)> = Vec::new();
        for _ in 0..4000 {
            let cx = rng.range(-600, 600) as i32;
            let cz = rng.range(-600, 600) as i32;
            if cx.abs() < 24 && cz.abs() < 24 {
                continue;
            }
            if let Some((surf, under)) = quick_overworld_probe(&pipe.biome_source, &pipe.rs, cx, cz)
            {
                let mut is_prio = false;
                if let Some(u) = under
                    && prio_seen.insert(u)
                {
                    is_prio = true;
                }
                if is_overworld_priority_biome(&surf) && prio_seen.insert(surf) {
                    is_prio = true;
                }
                if is_prio {
                    prio_cands.push((cx, cz));
                } else if regular_cands.len() < 120 {
                    regular_cands.push((cx, cz));
                }
            }
        }
        candidates.extend(prio_cands);
        candidates.extend(regular_cands);
    } else {
        for _ in 0..200 {
            let cx = rng.range(-400, 400) as i32;
            let cz = rng.range(-400, 400) as i32;
            if cx.abs() >= 24 || cz.abs() >= 24 {
                candidates.push((cx, cz));
            }
        }
    }

    let mut palette_cache: HashMap<(i32, i32), (Vec<Identifier>, Identifier)> = HashMap::new();

    // Classify candidates as:
    //   strictly_clean: every biome in the 3×3 neighborhood has zero Tree/FallenTree/Fossil/Template features
    //   surface_clean:  Overworld surface biome is tree-free (e.g. desert, ocean, badlands, beach, peaks)
    //                   and may only have rare fossils/desert_wells/water_trees -> verified with StubCallCounter
    let mut strict_new: Vec<(i32, i32, Identifier)> = Vec::new();
    let mut strict_extra: Vec<(i32, i32, Identifier)> = Vec::new();
    let mut verify_new: Vec<(i32, i32, Identifier)> = Vec::new();
    let mut verify_extra: Vec<(i32, i32, Identifier)> = Vec::new();
    let mut seen_verify_biomes: HashSet<Identifier> = HashSet::new();

    for &(cx, cz) in &candidates {
        let (_, center_biome) = palette_cache
            .entry((cx, cz))
            .or_insert_with(|| {
                sample_chunk_biome_palette(
                    &pipe.biome_source,
                    &pipe.rs,
                    pipe.min_y,
                    pipe.height,
                    cx,
                    cz,
                )
            })
            .clone();

        let mut all_strict = true;
        for dz in -1..=1 {
            for dx in -1..=1 {
                let (pal, _) = palette_cache.entry((cx + dx, cz + dz)).or_insert_with(|| {
                    sample_chunk_biome_palette(
                        &pipe.biome_source,
                        &pipe.rs,
                        pipe.min_y,
                        pipe.height,
                        cx + dx,
                        cz + dz,
                    )
                });
                if !pal.iter().all(|b| biome_is_tree_and_template_free(data, b)) {
                    all_strict = false;
                    break;
                }
            }
            if !all_strict {
                break;
            }
        }

        if all_strict {
            if seen_biomes.insert(center_biome.clone()) {
                strict_new.push((cx, cz, center_biome));
            } else if strict_extra.len() < target * 2 {
                strict_extra.push((cx, cz, center_biome));
            }
        } else if dim == "minecraft:overworld" && is_overworld_tree_free_surface(&center_biome) {
            if seen_verify_biomes.insert(center_biome.clone()) {
                verify_new.push((cx, cz, center_biome));
            } else if verify_extra.len() < target * 3 {
                verify_extra.push((cx, cz, center_biome));
            }
        }
    }

    // 1. Add strictly-clean new-biome chunks first.
    for (cx, cz, b) in strict_new {
        if chosen.len() >= target {
            break;
        }
        if well_spaced(&chosen, cx, cz) {
            chosen.push((cx, cz));
            chosen_biomes.push(b.to_string());
        }
    }

    // 2. For Overworld, add diverse surface-tree-free biomes (desert, oceans, badlands, etc.)
    //    verified via StubCallCounter == 0 so we get broad biome coverage without trees/templates.
    if dim == "minecraft:overworld" && chosen.len() < target {
        for (cx, cz, b) in verify_new.into_iter().chain(verify_extra) {
            if chosen.len() >= target {
                break;
            }
            if !well_spaced(&chosen, cx, cz) {
                continue;
            }
            let mut tchunks = Vec::with_capacity(9);
            for dz in -1..=1 {
                for dx in -1..=1 {
                    tchunks.push(pipe.terrain.generate_chunk(cx + dx, cz + dz));
                }
            }
            chosen.push((cx, cz));
            chosen_biomes.push(b.to_string());
            cached_3x3.insert((cx, cz), tchunks);
        }
    }

    // 3. Top up from strictly-clean fallback chunks if needed.
    for (cx, cz, b) in strict_extra {
        if chosen.len() >= target {
            break;
        }
        if well_spaced(&chosen, cx, cz) {
            chosen.push((cx, cz));
            chosen_biomes.push(b.to_string());
        }
    }

    (chosen, cached_3x3, chosen_biomes)
}

/// Surface biome of a chunk centre (`y = 80`), with no tree-free filter (cf. [`quick_overworld_probe`]).
fn quick_surface_biome(
    biome_source: &BiomeSource,
    rs: &RandomState,
    cx: i32,
    cz: i32,
) -> Option<Identifier> {
    let BiomeSource::MultiNoise(src) = biome_source else {
        return None;
    };
    let bx = (cx << 4) + 8;
    let bz = (cz << 4) + 8;
    let (qx, qz) = (bx >> 2, bz >> 2);
    let qy = 20;
    let vol = DensityVolume::new([1, 1, 1], [bx, qy << 2, bz], [4, 4, 4]).expect("valid");
    let channels = rs.sample_climate_volume(vol);
    let cv = ClimateVolume::new(qx, qy, qz, 1, 1, 1, channels).expect("valid");
    let mut state = SearchState::new();
    Some(
        src.parameter_list()
            .find_value(cv.sample(qx, qy, qz), &mut state)
            .clone(),
    )
}

/// Forest-type chunks (`((cx, cz), biome)`) and their cached 3x3 terrain neighbourhoods.
type ForestSelection = (
    Vec<((i32, i32), String)>,
    HashMap<(i32, i32), Vec<TerrainChunk>>,
);

/// Forest-type Overworld chunks: surface biomes whose feature lists place trees / fallen trees,
/// picked round-robin over distinct biomes (so rare ones are covered), well spaced from `taken`,
/// and verified free of fossil / template feature calls.
fn select_forest_chunks(
    pipe: &DimPipeline,
    data: &FeatureData,
    seed: i64,
    target: usize,
    taken: &[(i32, i32)],
) -> ForestSelection {
    const PER_BIOME_CANDIDATES: usize = 24;
    let mut rng = crate::parity_terrain::Lcg::new((seed as u64) ^ 0xF0E5_7777_1234_9ABC);
    let mut by_biome: BTreeMap<String, Vec<(i32, i32)>> = BTreeMap::new();
    for _ in 0..9000 {
        let cx = rng.range(-1500, 1500) as i32;
        let cz = rng.range(-1500, 1500) as i32;
        if cx.abs() < 24 && cz.abs() < 24 {
            continue;
        }
        let Some(b) = quick_surface_biome(&pipe.biome_source, &pipe.rs, cx, cz) else {
            continue;
        };
        let name = b.to_string();
        if !biome_has_trees(data, &b)
            || OVERWORLD_TREE_FREE_SURFACE_BIOMES.contains(&name.as_str())
            || ["ocean", "cave", "deep_dark", "river", "beach"]
                .iter()
                .any(|w| name.contains(w))
        {
            continue;
        }
        let list = by_biome.entry(b.to_string()).or_default();
        if list.len() < PER_BIOME_CANDIDATES {
            list.push((cx, cz));
        }
    }

    // Land-forest biomes first (each round takes one chunk per biome until `target` is reached).
    let rank = |name: &str| -> usize {
        FOREST_PRIORITY_BIOMES
            .iter()
            .position(|p| *p == name.strip_prefix("minecraft:").unwrap_or(name))
            .unwrap_or(FOREST_PRIORITY_BIOMES.len())
    };
    let mut by_biome: Vec<(String, Vec<(i32, i32)>)> = by_biome.into_iter().collect();
    by_biome.sort_by_key(|(name, _)| (rank(name), name.clone()));

    let mut chosen: Vec<(i32, i32)> = taken.to_vec();
    let mut out: Vec<((i32, i32), String)> = Vec::new();
    let mut cached: HashMap<(i32, i32), Vec<TerrainChunk>> = HashMap::new();
    let mut cursor: BTreeMap<String, usize> = BTreeMap::new();
    let mut progressed = true;
    while out.len() < target && progressed {
        progressed = false;
        for (biome, cands) in &by_biome {
            if out.len() >= target {
                break;
            }
            let idx = cursor.entry(biome.clone()).or_insert(0);
            while *idx < cands.len() {
                let (cx, cz) = cands[*idx];
                *idx += 1;
                progressed = true;
                if !well_spaced(&chosen, cx, cz) {
                    continue;
                }
                let mut tchunks = Vec::with_capacity(9);
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        tchunks.push(pipe.terrain.generate_chunk(cx + dx, cz + dz));
                    }
                }
                chosen.push((cx, cz));
                out.push(((cx, cz), biome.clone()));
                cached.insert((cx, cz), tchunks);
                break;
            }
        }
    }
    (out, cached)
}

struct DimPipeline {
    terrain: TerrainGenerator,
    decorator: FeatureDecorator,
    template_counter: Arc<TemplateCounter>,
    /// Structure placement hook (`generate-structures = true` pipelines only).
    structures: Option<Arc<StructurePlacementHook>>,
    biome_source: BiomeSource,
    rs: RandomState,
    min_y: i32,
    height: i32,
}

fn build_dim_pipeline(
    ctx: &FeatureContextData,
    dim: &str,
    seed: i64,
    structures_on: bool,
) -> anyhow::Result<DimPipeline> {
    let (settings_id, dim_type_id, biome_source) = match dim {
        "minecraft:overworld" => (
            Identifier::parse("minecraft:overworld")?,
            Identifier::parse("minecraft:overworld")?,
            BiomeSource::MultiNoise(MultiNoiseBiomeSource::from_preset(
                MultiNoisePreset::Overworld,
            )),
        ),
        "minecraft:the_nether" => (
            Identifier::parse("minecraft:nether")?,
            Identifier::parse("minecraft:the_nether")?,
            BiomeSource::MultiNoise(MultiNoiseBiomeSource::from_preset(MultiNoisePreset::Nether)),
        ),
        "minecraft:the_end" => (
            Identifier::parse("minecraft:end")?,
            Identifier::parse("minecraft:the_end")?,
            BiomeSource::TheEnd(TheEndBiomeSource::new()),
        ),
        other => anyhow::bail!("unsupported dimension {other}"),
    };

    let mut terrain = TerrainGenerator::new(
        &ctx.worldgen,
        biome_source.clone(),
        &settings_id,
        seed,
        &ctx.blocks,
        &ctx.tags,
    )
    .map_err(|e| anyhow::anyhow!("terrain gen {dim}: {e}"))?;
    let (min_y, height) = if let Some(dh) = ctx.worldgen.dimension_height(&dim_type_id) {
        terrain = terrain.with_chunk_height(dh.min_y, dh.height);
        (dh.min_y, dh.height)
    } else {
        let ns = ctx
            .worldgen
            .noise_settings(&settings_id)
            .with_context(|| format!("missing noise settings {settings_id}"))?;
        (ns.min_y(), ns.height())
    };

    let ns = ctx
        .worldgen
        .noise_settings(&settings_id)
        .with_context(|| format!("missing noise settings {settings_id}"))?;
    let rs = RandomState::new(&ctx.worldgen, &settings_id, seed)
        .map_err(|e| anyhow::anyhow!("random state {dim}: {e}"))?;
    let template_counter = Arc::new(TemplateCounter::new(StructureTemplateHook::new(
        Arc::clone(&ctx.structure_data),
        Arc::clone(&ctx.templates),
    )));
    let template_hook: Arc<dyn TemplateFeatureHook> =
        Arc::clone(&template_counter) as Arc<dyn TemplateFeatureHook>;
    let decorator = FeatureDecorator::new(
        Arc::clone(&ctx.features),
        Arc::clone(&ctx.ops),
        &biome_source,
        seed,
        ns.min_y(),
        ns.height(),
        ns.sea_level(),
        ns.legacy_random_source(),
    )
    .map_err(|e| anyhow::anyhow!("feature decorator {dim}: {e}"))?
    .with_template_hook(template_hook);
    let (decorator, structures) = if structures_on {
        let noise_gen = Arc::new(
            NoiseGenerator::new(&ctx.worldgen, &settings_id, seed, &ctx.blocks, &ctx.tags)
                .map_err(|e| anyhow::anyhow!("noise generator {dim}: {e}"))?,
        );
        let generator = Arc::new(StructureGenerator::new(
            seed,
            &ctx.worldgen,
            Arc::clone(&ctx.structure_data),
            Arc::clone(&ctx.templates),
            noise_gen,
            biome_source.clone(),
            Arc::clone(&ctx.facts),
            &ctx.blocks,
            min_y,
            height,
        ));
        let hook = Arc::new(StructurePlacementHook::new(
            generator,
            Arc::clone(&ctx.features),
        ));
        (
            decorator.with_structure_hook(Arc::clone(&hook) as Arc<dyn StructureInterleaveHook>),
            Some(hook),
        )
    } else {
        (decorator, None)
    };
    // Debug aid: `ZENITH_FEATURES_NOTREES=1` disables tree placement to isolate non-tree diffs.
    let decorator = if std::env::var_os("ZENITH_FEATURES_NOTREES").is_some() {
        decorator.with_tree_hook(Arc::new(zenith_features::NoopTreeHook))
    } else {
        decorator
    };

    Ok(DimPipeline {
        terrain,
        decorator,
        template_counter,
        structures,
        biome_source,
        rs,
        min_y,
        height,
    })
}

/// ` ; only vanilla: ... ; only zenith: ...` — the marks present on one side only, as world
/// positions with the block (on the Zenith side) at the position.
fn mark_difference(
    ctx: &FeatureContextData,
    dc: &DumpedChunk,
    zc: &RegionChunk,
    zen: &[(i32, u16)],
) -> String {
    let mut counts: HashMap<(i32, u16), i32> = HashMap::new();
    for m in &dc.post_processing {
        *counts.entry(*m).or_default() += 1;
    }
    for m in zen {
        *counts.entry(*m).or_default() -= 1;
    }
    let desc = |(sec, packed): (i32, u16)| {
        let (lx, ly, lz) = (
            i32::from(packed & 15),
            i32::from((packed >> 4) & 15),
            i32::from((packed >> 8) & 15),
        );
        let y = dc.min_y + sec * 16 + ly;
        let block = describe_state(&ctx.blocks, zc.get_block(lx as usize, y, lz as usize));
        let block = block.split('[').next().unwrap_or("").to_owned();
        format!(
            "({},{},{}) {}",
            (dc.x << 4) + lx,
            y,
            (dc.z << 4) + lz,
            block.replace("minecraft:", "")
        )
    };
    let mut only_van: Vec<String> = Vec::new();
    let mut only_zen: Vec<String> = Vec::new();
    let mut keys: Vec<_> = counts.iter().collect();
    keys.sort();
    for (k, &c) in keys {
        if c > 0 {
            only_van.push(format!("{}x{c}", desc(*k)));
        } else if c < 0 {
            only_zen.push(format!("{}x{}", desc(*k), -c));
        }
    }
    // Order-only differences: list the sequences from the first differing entry.
    let first = dc.post_processing.iter().zip(zen).position(|(a, b)| a != b);
    let seq_len = if std::env::var_os("ZENITH_FEATURES_FULLSEQ").is_some() {
        usize::MAX
    } else {
        10
    };
    let seq = if let Some(i) = first {
        let st = if seq_len == usize::MAX { 0 } else { i };
        let v: Vec<String> = dc.post_processing[st..]
            .iter()
            .take(seq_len)
            .map(|m| desc(*m))
            .collect();
        let z: Vec<String> = zen[st..].iter().take(seq_len).map(|m| desc(*m)).collect();
        format!(
            " ; van seq: [{}] ; zen seq: [{}]",
            v.join(" | "),
            z.join(" | ")
        )
    } else {
        String::new()
    };
    seq + &format!(
        " ; only vanilla: [{}] ; only zenith: [{}]",
        only_van
            .iter()
            .take(16)
            .cloned()
            .collect::<Vec<_>>()
            .join(", "),
        only_zen
            .iter()
            .take(16)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// Compares one dumped vanilla chunk with Zenith's decorated chunk (blocks, biomes, 6 heightmaps,
/// post-processing marks, tick counts); returns whether every part is identical.
/// `base_blocks` is the terrain-only block array (debug output of what the step changed), may be empty.
fn compare_chunk(
    ctx: &FeatureContextData,
    t: &mut Tally,
    dc: &DumpedChunk,
    zc: &RegionChunk,
    base_blocks: &[u32],
    tag: &str,
) -> bool {
    let (passed_before, total_before) = (t.passed, t.total);
    t.changed_blocks += zc
        .blocks()
        .iter()
        .zip(base_blocks)
        .filter(|(a, b)| a != b)
        .count() as u64;

    // 1. Blocks
    let blocks_ok =
        dc.min_y == zc.min_y() && dc.height == zc.height() && dc.blocks.as_slice() == zc.blocks();
    if let Ok(v) = std::env::var("ZENITH_FEATURES_NEIGH") {
        let p: Vec<i32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        if p.len() == 3 && (p[0] >> 4) == dc.x && (p[2] >> 4) == dc.z {
            for (dx, dy, dz) in [
                (0, 0, 0),
                (1, 0, 0),
                (-1, 0, 0),
                (0, 0, 1),
                (0, 0, -1),
                (0, 1, 0),
                (0, -1, 0),
            ] {
                let (x, y, z) = (p[0] + dx, p[1] + dy, p[2] + dz);
                if (x >> 4) != dc.x || (z >> 4) != dc.z {
                    continue;
                }
                let (lx, lz) = ((x & 15) as usize, (z & 15) as usize);
                let ly = (y - dc.min_y) as usize;
                let idx = (ly / 16) * 4096 + ((ly % 16) * 16 + lz) * 16 + lx;
                println!(
                    "NEIGH ({dx},{dy},{dz}) vanilla {} | zenith {}",
                    describe_state(&ctx.blocks, dc.blocks[idx]),
                    describe_state(&ctx.blocks, zc.blocks()[idx])
                );
            }
        }
    }
    if let Ok(name) = std::env::var("ZENITH_FEATURES_COLMAP")
        && !dc.blocks.as_slice().eq(zc.blocks())
    {
        // Top-down map of the highest block called `name` per column: vanilla | zenith.
        let height = dc.blocks.len() / 256;
        let top = |blocks: &[u32], lx: usize, lz: usize| -> char {
            for ly in (0..height).rev() {
                let idx = (ly / 16) * 4096 + ((ly % 16) * 16 + lz) * 16 + lx;
                if describe_state(&ctx.blocks, blocks[idx])
                    .starts_with(&format!("minecraft:{name}"))
                {
                    return char::from_digit(((dc.min_y + ly as i32).rem_euclid(10)) as u32, 10)
                        .unwrap_or('?');
                }
            }
            '.'
        };
        println!("COLMAP {tag} (last digit of highest {name}; vanilla | zenith)");
        for lz in 0..16 {
            let v: String = (0..16).map(|lx| top(&dc.blocks, lx, lz)).collect();
            let z: String = (0..16).map(|lx| top(zc.blocks(), lx, lz)).collect();
            println!("  z={lz:2} {v} | {z}");
        }
    }
    let diff_limit = if t.diff_limit == 0 { 12 } else { t.diff_limit };
    t.check(blocks_ok, || {
        match dc
            .blocks
            .iter()
            .zip(zc.blocks())
            .position(|(a, b)| a != b)
        {
            Some(idx) => {
                let sec = idx / 4096;
                let rem = idx % 4096;
                let ly = rem / 256;
                let lz = (rem % 256) / 16;
                let lx = rem % 16;
                let wx = (dc.x << 4) + lx as i32;
                let wy = dc.min_y + (sec * 16 + ly) as i32;
                let wz = (dc.z << 4) + lz as i32;
                let biome = zc
                    .biome_at_quart(lx >> 2, (sec * 16 + ly) >> 2, lz >> 2)
                    .map_or_else(|| "?".to_owned(), ToString::to_string);
                let diffs: Vec<String> = dc
                    .blocks
                    .iter()
                    .zip(zc.blocks())
                    .enumerate()
                    .filter(|(_, (a, b))| a != b)
                    .map(|(i, (a, b))| {
                        let (s, r) = (i / 4096, i % 4096);
                        format!(
                            "({},{},{}) {}→{}",
                            (dc.x << 4) + (r % 16) as i32,
                            dc.min_y + (s * 16 + r / 256) as i32,
                            (dc.z << 4) + ((r % 256) / 16) as i32,
                            describe_state(&ctx.blocks, *a),
                            describe_state(&ctx.blocks, *b)
                        )
                    })
                    .collect();
                format!(
                    "{tag} blocks: first diff at ({wx},{wy},{wz}) [biome {biome}]: vanilla {} | zenith {}; {} blocks differ: {}",
                    describe_state(&ctx.blocks, dc.blocks[idx]),
                    describe_state(&ctx.blocks, zc.blocks()[idx]),
                    diffs.len(),
                    diffs.iter().take(diff_limit).cloned().collect::<Vec<_>>().join(", ")
                )
            }
            None => format!(
                "{tag} blocks: length vanilla {} | zenith {}",
                dc.blocks.len(),
                zc.blocks().len()
            ),
        }
    });

    if !blocks_ok && std::env::var_os("ZENITH_FEATURES_DUMP").is_some() {
        dump_diff_region(
            &ctx.blocks,
            dc.min_y,
            dc.x,
            dc.z,
            &dc.blocks,
            zc.blocks(),
            tag,
        );
    }

    // 2. Biomes
    let zen_biomes: Vec<String> = zc.biomes_iter().map(ToString::to_string).collect();
    t.check(dc.biomes == zen_biomes, || format!("{tag} biomes differ"));

    // 3. Heightmaps (6 total)
    for (hm_name, van_heights) in &dc.heightmaps {
        let kind = heightmap_kind_from_name(hm_name);
        let zen_hm = kind.map(|k| zc.heightmap(k));
        let ok = zen_hm.is_some_and(|h| h.heights().as_slice() == van_heights.as_slice());
        t.check(ok, || match zen_hm {
            Some(h) => match van_heights.iter().zip(h.heights()).position(|(a, b)| a != b) {
                Some(idx) => format!(
                    "{tag} heightmap {hm_name}: first diff at local ({},{}): vanilla {} | zenith {}",
                    idx % 16,
                    idx / 16,
                    van_heights[idx],
                    h.heights()[idx]
                ),
                None => format!("{tag} heightmap {hm_name}: length mismatch"),
            },
            None => format!("{tag} heightmap {hm_name}: missing on Zenith"),
        });
    }

    // 4. Post-processing marks
    let zen_marks: Vec<(i32, u16)> = zc
        .post_processing_marks()
        .into_iter()
        .map(|m| (m.section, m.packed))
        .collect();
    t.check(dc.post_processing == zen_marks, || {
        let head = match dc
            .post_processing
            .iter()
            .zip(&zen_marks)
            .position(|(a, b)| a != b)
        {
            Some(idx) => format!(
                "{tag} post_processing: first diff at #{idx} (len van {} / zen {}): vanilla {:?} | zenith {:?}",
                dc.post_processing.len(),
                zen_marks.len(),
                dc.post_processing[idx],
                zen_marks[idx]
            ),
            None => format!(
                "{tag} post_processing: len vanilla {} | zenith {}",
                dc.post_processing.len(),
                zen_marks.len()
            ),
        };
        head + &mark_difference(ctx, dc, zc, &zen_marks)
    });

    // 5. Scheduled tick counts
    let ticks_ok =
        dc.block_ticks == zc.block_tick_count() && dc.fluid_ticks == zc.fluid_tick_count();
    if !ticks_ok && std::env::var_os("ZENITH_FEATURES_DUMP").is_some() {
        let mut by_block: BTreeMap<u32, usize> = BTreeMap::new();
        for (_, _, _, b) in zc.scheduled_block_ticks() {
            *by_block.entry(b).or_default() += 1;
        }
        let named: Vec<String> = by_block
            .iter()
            .map(|(b, n)| {
                let name = ctx
                    .blocks
                    .blocks()
                    .get(*b as usize)
                    .map_or_else(|| format!("#{b}"), |e| e.id.to_string());
                format!("{name}={n}")
            })
            .collect();
        println!(
            "--- {tag}: zenith feature-step block ticks: {}",
            named.join(", ")
        );
        let mut changed: BTreeMap<String, usize> = BTreeMap::new();
        for (a, b) in zc.blocks().iter().zip(base_blocks) {
            if a != b {
                let n = describe_state(&ctx.blocks, *a);
                let n = n.split('[').next().unwrap_or("").to_owned();
                *changed.entry(n).or_default() += 1;
            }
        }
        println!("--- {tag}: blocks written by features: {changed:?}");
    }
    t.check(ticks_ok, || {
        format!(
            "{tag} ticks: vanilla ({}, {}) | zenith ({}, {})",
            dc.block_ticks,
            dc.fluid_ticks,
            zc.block_tick_count(),
            zc.fluid_tick_count()
        )
    });

    t.passed - passed_before == t.total - total_before
}

fn run_world_job(
    ctx: &FeatureContextData,
    version: &str,
    seed: i64,
    timing: &Mutex<Timing>,
    only_dim: Option<&str>,
) -> anyhow::Result<Tally> {
    let mut t = Tally::default();
    let dims = [
        "minecraft:the_nether",
        "minecraft:the_end",
        "minecraft:overworld",
    ];
    let mut pipelines: HashMap<&str, DimPipeline> = HashMap::new();
    let mut precomputed_terrain: HashMap<(String, i32, i32), Vec<TerrainChunk>> = HashMap::new();
    let mut requests: Vec<ChunkRequest> = Vec::new();
    let mut labels: HashMap<(String, i32, i32), String> = HashMap::new();

    for &dim in &dims {
        if let Some(filter) = only_dim
            && filter != dim
        {
            continue;
        }
        let pipe = build_dim_pipeline(ctx, dim, seed, false)?;
        let target_chunks = if dim == "minecraft:overworld" { 12 } else { 8 };
        let (coords, cached, biomes) =
            select_chunks_for_dim(&pipe, &ctx.features, seed, dim, target_chunks);
        println!(
            "  select {dim} seed {seed}: {} chunks [{}]",
            coords.len(),
            biomes.join(", ")
        );
        for (coord, chunks) in cached {
            precomputed_terrain.insert((dim.to_owned(), coord.0, coord.1), chunks);
        }
        for (&coord, biome) in coords.iter().zip(&biomes) {
            let label = match dim {
                "minecraft:the_nether" => "nether",
                "minecraft:the_end" => "end",
                _ => biome_group(biome),
            };
            labels.insert((dim.to_owned(), coord.0, coord.1), label.to_owned());
        }
        for (cx, cz) in coords.iter().copied() {
            requests.push(ChunkRequest {
                dimension: dim.to_owned(),
                x: cx,
                z: cz,
                status: "features".to_owned(),
            });
        }
        if dim == "minecraft:overworld" {
            let (forest, cached_forest) =
                select_forest_chunks(&pipe, &ctx.features, seed, FOREST_CHUNKS_PER_SEED, &coords);
            println!(
                "  select {dim} seed {seed}: {} forest-type chunks [{}]",
                forest.len(),
                forest
                    .iter()
                    .map(|(_, b)| b.strip_prefix("minecraft:").unwrap_or(b))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            for (coord, chunks) in cached_forest {
                precomputed_terrain.insert((dim.to_owned(), coord.0, coord.1), chunks);
            }
            for ((cx, cz), biome) in forest {
                labels.insert((dim.to_owned(), cx, cz), biome_group(&biome).to_owned());
                requests.push(ChunkRequest {
                    dimension: dim.to_owned(),
                    x: cx,
                    z: cz,
                    status: "features".to_owned(),
                });
            }
        }
        pipelines.insert(dim, pipe);
    }

    if requests.is_empty() {
        return Ok(t);
    }

    // Debug aid: `ZENITH_FEATURES_NEIGHBORS=cx,cz` also dumps the 8 neighbours at `terrain` status
    // and compares them with Zenith's terrain (neighbour terrain feeds the 3x3 feature region).
    let mut neighbor_checks: std::collections::HashSet<(i32, i32)> =
        std::collections::HashSet::new();
    let mut neighbor_blocks: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
    if let Ok(v) = std::env::var("ZENITH_FEATURES_NEIGHBORS") {
        let p: Vec<i32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        if p.len() == 2 {
            for dz in -1..=1 {
                for dx in -1..=1 {
                    if dx != 0 || dz != 0 {
                        neighbor_checks.insert((p[0] + dx, p[1] + dz));
                        requests.push(ChunkRequest {
                            dimension: "minecraft:overworld".to_owned(),
                            x: p[0] + dx,
                            z: p[1] + dz,
                            status: "terrain".to_owned(),
                        });
                    }
                }
            }
        }
    }

    let world = ChunkWorld {
        seed,
        level_type: "minecraft:normal".to_owned(),
        structures: false,
    };
    let van_start = Instant::now();
    let raw = dump_chunks(version, &world, &requests)?;
    let van_ms = van_start.elapsed().as_millis();
    let dumped = parse_chunk_dump(&raw)?;
    anyhow::ensure!(
        dumped.len() == requests.len(),
        "dumped chunk count {} != requested {}",
        dumped.len(),
        requests.len()
    );

    let mut terrain_us = 0u128;
    let mut features_us = 0u128;

    for dc in &dumped {
        if dc.dimension == "minecraft:overworld"
            && neighbor_checks.contains(&(dc.x, dc.z))
            && let Some(pipe) = pipelines.get("minecraft:overworld")
        {
            let _ = pipe;
            let Some(ours) = neighbor_blocks.get(&(dc.x, dc.z)) else {
                continue;
            };
            let ndiff = dc.blocks.iter().zip(ours).filter(|(a, b)| a != b).count();
            println!("  NEIGHBOR ({},{}) differing blocks: {ndiff}", dc.x, dc.z);
            if std::env::var_os("ZENITH_FEATURES_NEIGHBOR_VEG").is_some() {
                for (i, (a, b)) in dc.blocks.iter().zip(ours).enumerate() {
                    let (va, vb) = (
                        describe_state(&ctx.blocks, *a),
                        describe_state(&ctx.blocks, *b),
                    );
                    let veg = |n: &str| {
                        n.contains("moss_carpet")
                            || n.contains("short_grass")
                            || n.contains("tall_grass")
                            || n.contains("azalea")
                    };
                    if veg(&va) || veg(&vb) {
                        let (sec, r) = (i / 4096, i % 4096);
                        if (r % 16) < 8 {
                            println!(
                                "    veg {} {} {}: {} | {}",
                                (dc.x << 4) + (r % 16) as i32,
                                dc.min_y + (sec * 16 + r / 256) as i32,
                                (dc.z << 4) + ((r % 256) / 16) as i32,
                                va.replace("minecraft:", ""),
                                vb.replace("minecraft:", "")
                            );
                        }
                    }
                }
            }
            for (i, (a, b)) in dc
                .blocks
                .iter()
                .zip(ours)
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .take(12)
            {
                let (sec, r) = (i / 4096, i % 4096);
                println!(
                    "    at {} {} {}: vanilla {} | zenith {}",
                    (dc.x << 4) + (r % 16) as i32,
                    dc.min_y + (sec * 16 + r / 256) as i32,
                    (dc.z << 4) + ((r % 256) / 16) as i32,
                    describe_state(&ctx.blocks, *a),
                    describe_state(&ctx.blocks, *b)
                );
            }
            continue;
        }
        let pipe = pipelines
            .get(dc.dimension.as_str())
            .with_context(|| format!("missing pipeline for {}", dc.dimension))?;
        let terrain = &pipe.terrain;
        let decorator = &pipe.decorator;

        let t0 = Instant::now();
        let tchunks =
            if let Some(cached) = precomputed_terrain.remove(&(dc.dimension.clone(), dc.x, dc.z)) {
                cached
            } else {
                let mut v = Vec::with_capacity(9);
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        v.push(terrain.generate_chunk(dc.x + dx, dc.z + dz));
                    }
                }
                v
            };
        terrain_us += t0.elapsed().as_micros();

        let t1 = Instant::now();
        let refs: [&TerrainChunk; 9] = std::array::from_fn(|i| &tchunks[i]);
        let mut region = decorator.make_region(dc.x, dc.z, refs);
        pipe.template_counter.reset();
        decorator.decorate(&mut region, dc.x, dc.z);
        if !neighbor_checks.is_empty() && !neighbor_checks.contains(&(dc.x, dc.z)) {
            for (i, rc) in region.chunks().iter().enumerate() {
                let (nx, nz) = (dc.x + (i % 3) as i32 - 1, dc.z + (i / 3) as i32 - 1);
                if neighbor_checks.contains(&(nx, nz)) {
                    neighbor_blocks.insert((nx, nz), rc.blocks().to_vec());
                }
            }
        }
        let zc = region.into_center_chunk();
        features_us += t1.elapsed().as_micros();

        let tag = format!("{} seed {seed} ({},{})", dc.dimension, dc.x, dc.z);
        let chunk_ok = compare_chunk(ctx, &mut t, dc, &zc, tchunks[4].blocks(), &tag);
        if pipe.template_counter.placed() > 0 {
            let e = t
                .groups
                .entry("fossil/template placed".to_owned())
                .or_default();
            e.1 += 1;
            e.0 += u64::from(chunk_ok);
        }
        let label = labels
            .get(&(dc.dimension.clone(), dc.x, dc.z))
            .cloned()
            .unwrap_or_else(|| "unlabelled".to_owned());
        let e = t.groups.entry(label).or_default();
        e.1 += 1;
        e.0 += u64::from(chunk_ok);
    }

    {
        let mut g = timing.lock().expect("timing");
        g.vanilla_ms += van_ms;
        g.terrain_us += terrain_us;
        g.features_us += features_us;
        g.chunks += dumped.len();
    }

    Ok(t)
}

/// Debug aid (`ZENITH_FEATURES_DUMP=1`): ASCII layers around the first differing block
/// (leaves print their `distance`, logs `L`, air `.`, anything else `#`; vanilla | zenith).
fn dump_diff_region(
    blocks: &BlockStateTable,
    min_y: i32,
    cx: i32,
    cz: i32,
    vanilla: &[u32],
    zenith: &[u32],
    tag: &str,
) {
    let Some(first) = vanilla.iter().zip(zenith).position(|(a, b)| a != b) else {
        return;
    };
    let (sec, rem) = (first / 4096, first % 4096);
    let (mut fy, mut fz, mut fx) = (sec * 16 + rem / 256, (rem % 256) / 16, rem % 16);
    if let Some(at) = std::env::var("ZENITH_FEATURES_DUMP_AT").ok().and_then(|v| {
        let p: Vec<i32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        (p.len() == 3).then_some(p)
    }) {
        (fx, fz) = (
            (at[0] - (cx << 4)).clamp(0, 15) as usize,
            (at[2] - (cz << 4)).clamp(0, 15) as usize,
        );
        fy = (at[1] - min_y).max(0) as usize;
    }
    let glyph = |id: u32| -> char {
        let d = describe_state(blocks, id);
        if d.ends_with(":air") || d.contains("cave_air") {
            '.'
        } else if let Some(i) = d.find("distance=") {
            d.as_bytes()[i + 9] as char
        } else if d.contains("_log") || d.contains("_wood") || d.contains("_stem") {
            'L'
        } else if d.contains("leaves") {
            'l'
        } else {
            '#'
        }
    };
    println!(
        "--- {tag}: first diff local ({fx},{},{fz}); vanilla | zenith",
        min_y + fy as i32
    );
    let mut hist: BTreeMap<(String, String), usize> = BTreeMap::new();
    for (a, b) in vanilla.iter().zip(zenith) {
        if a != b {
            let n = |id: u32| {
                let d = describe_state(blocks, id);
                d.split('[').next().unwrap_or("").to_owned()
            };
            *hist.entry((n(*a), n(*b))).or_default() += 1;
        }
    }
    for ((a, b), n) in &hist {
        println!("    diff {a} -> {b}: {n}");
    }
    if std::env::var_os("ZENITH_FEATURES_DUMP_ALL").is_some() {
        for (i, (a, b)) in vanilla.iter().zip(zenith).enumerate() {
            if a != b {
                let (s, r) = (i / 4096, i % 4096);
                let below = |arr: &[u32]| -> String {
                    if i >= 256 {
                        describe_state(blocks, arr[i - 256])
                    } else {
                        "?".to_owned()
                    }
                };
                println!(
                    "    at {} {} {} : {} -> {}   [below: {} | {}]",
                    (cx << 4) + (r % 16) as i32,
                    min_y + (s * 16 + r / 256) as i32,
                    (cz << 4) + ((r % 256) / 16) as i32,
                    describe_state(blocks, *a),
                    describe_state(blocks, *b),
                    below(vanilla),
                    below(zenith)
                );
            }
        }
    }
    let at = |arr: &[u32], x: i32, y: i32, z: i32| -> char {
        if !(0..16).contains(&x) || !(0..16).contains(&z) || y < 0 {
            return ' ';
        }
        let (s, ly) = (y as usize / 16, y as usize % 16);
        arr.get(s * 4096 + ly * 256 + z as usize * 16 + x as usize)
            .map_or(' ', |&id| glyph(id))
    };
    if std::env::var_os("ZENITH_FEATURES_DUMP_NAMES").is_some() {
        for y in (fy as i32 - 1)..=(fy as i32 + 2) {
            for z in (fz as i32 - 3)..=(fz as i32 + 3) {
                for x in (fx as i32 - 3)..=(fx as i32 + 3) {
                    if !(0..16).contains(&x) || !(0..16).contains(&z) || y < 0 {
                        continue;
                    }
                    let idx = (y as usize / 16) * 4096
                        + (y as usize % 16) * 256
                        + z as usize * 16
                        + x as usize;
                    let (a, b) = (vanilla[idx], zenith[idx]);
                    let n = |id: u32| describe_state(blocks, id).replace("minecraft:", "");
                    let mark = if a == b { "  " } else { "!!" };
                    println!("   {mark} ({x},{y},{z}) {} | {}", n(a), n(b));
                }
            }
        }
    }
    let y0 = (fy as i32 - 3).max(0);
    for y in y0..(fy as i32 + 5) {
        println!("y={}", min_y + y);
        for z in 0..16 {
            let a: String = (0..16).map(|x| at(vanilla, x, y, z)).collect();
            let b: String = (0..16).map(|x| at(zenith, x, y, z)).collect();
            if a.trim().is_empty() && b.trim().is_empty() {
                continue;
            }
            println!("  z={z:>2} {a} | {b}");
        }
    }
}

/// `cargo xtask parity golden-features [--write]`.
pub fn golden_features(version: &str, write: bool) -> anyhow::Result<bool> {
    let ctx = load_context(version)?;
    let timing = Mutex::new(Timing::default());

    let only_env = std::env::var("ZENITH_FEATURES_ONLY").ok();
    let (only_seed, only_dim): (Option<i64>, Option<String>) = match only_env.as_deref() {
        Some(s) => {
            if let Some((seed_s, dim_s)) = s.split_once(':') {
                (seed_s.parse().ok(), Some(dim_s.to_owned()))
            } else {
                (None, Some(s.to_owned()))
            }
        }
        None => (None, None),
    };

    // `ZENITH_FEATURES_SEEDS=1,2,3` replaces the golden seeds (extended local runs).
    let golden: Vec<i64> = std::env::var("ZENITH_FEATURES_SEEDS")
        .ok()
        .map(|v| v.split(',').filter_map(|x| x.trim().parse().ok()).collect())
        .unwrap_or_else(|| GOLDEN_SEEDS.to_vec());
    let seeds: Vec<i64> = golden
        .iter()
        .copied()
        .filter(|&s| only_seed.is_none_or(|os| os == s))
        .collect();

    let results = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for &seed in &seeds {
            let ctx = &ctx;
            let timing = &timing;
            let results = &results;
            let only_dim = only_dim.as_deref();
            scope.spawn(move || {
                let r = run_world_job(ctx, version, seed, timing, only_dim).inspect(|t| {
                    println!(
                        "  world seed {seed}: {}/{} ({} blocks written by features)",
                        t.passed, t.total, t.changed_blocks
                    );
                });
                results.lock().expect("results").push(r);
            });
        }
    });

    let mut tally = Tally::default();
    for r in results.into_inner().expect("results") {
        tally.merge(r?);
    }

    let tm = timing.into_inner().expect("timing");
    let feat_ms_per_chunk = if tm.chunks > 0 {
        (tm.features_us as f64 / 1000.0) / tm.chunks as f64
    } else {
        0.0
    };
    let terrain_ms_per_chunk = if tm.chunks > 0 {
        (tm.terrain_us as f64 / 1000.0) / tm.chunks as f64
    } else {
        0.0
    };
    println!(
        "golden-features timing: {} chunks — features {:.2} ms/chunk (3x3 terrain {:.2} ms/chunk) | vanilla server runs {} ms total",
        tm.chunks, feat_ms_per_chunk, terrain_ms_per_chunk, tm.vanilla_ms
    );

    println!(
        "golden-features {version}: {}/{} identical (blocks, biomes, 6 heightmaps, post-processing marks, ticks; {} seeds, structures off)",
        tally.passed,
        tally.total,
        seeds.len()
    );
    for (group, (p, n)) in &tally.groups {
        println!("  group {group:<22} {p}/{n} chunks identical");
    }
    for f in &tally.failures {
        println!("  FAIL {f}");
    }
    if write {
        SuiteResult::new("golden-features", version, tally.passed, tally.total)
            .write(&repo_root().join("progress/parity"))?;
    }
    Ok(tally.passed == tally.total)
}

// ===========================================================================
// golden-structures: features chunks with `generate-structures=true`
// ===========================================================================

/// Structure types whose pieces span many chunks: several chunks per start are compared.
const BIG_STRUCTURES: &[&str] = &[
    "village_plains",
    "village_desert",
    "village_savanna",
    "village_snowy",
    "village_taiga",
    "stronghold",
    "mansion",
    "monument",
    "fortress",
    "bastion_remnant",
    "ancient_city",
    "trial_chambers",
    "end_city",
    "mineshaft",
    "mineshaft_mesa",
    "pillager_outpost",
    "shipwreck",
    "ocean_ruin_cold",
    "ocean_ruin_warm",
];

/// Starts picked per structure id, and chunks compared per start (big structures / others).
const STRUCT_PER_ID: usize = 3;
const CHUNKS_PER_BIG_START: usize = 8;
const CHUNKS_PER_START: usize = 3;
/// Random-spread candidate chunks examined per structure set while searching for starts.
const STRUCT_MAX_CHECKS: usize = 500;
const STRUCT_MAX_RADIUS: i32 = 60;
/// Template / fossil chunks wanted per seed (overworld) and how many candidates are scanned.
const TEMPLATE_CHUNKS_PER_SEED: usize = 8;
const TEMPLATE_SCAN: usize = 600;

/// Chunks (raster order) that a piece of `start` intersects (the pieces, not the inflated box).
fn start_piece_chunks(start: &zenith_structures::StructureStart) -> Vec<(i32, i32)> {
    let mut set: std::collections::BTreeSet<(i32, i32)> = std::collections::BTreeSet::new();
    for p in &start.pieces {
        let bb = p.bounding_box;
        for cz in (bb.min_z >> 4)..=(bb.max_z >> 4) {
            for cx in (bb.min_x >> 4)..=(bb.max_x >> 4) {
                set.insert((cz, cx));
            }
        }
    }
    set.into_iter().map(|(z, x)| (x, z)).collect()
}

/// Structure types (path names) whose pieces intersect chunk `(cx, cz)`, from the shared starts.
fn types_at_chunk(
    store: &zenith_features::StartStore,
    cx: i32,
    cz: i32,
) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    for sx in (cx - 8)..=(cx + 8) {
        for sz in (cz - 8)..=(cz + 8) {
            if let Some(starts) = store.get(&(sx, sz)) {
                for s in starts {
                    if s.is_valid()
                        && s.pieces.iter().any(|p| {
                            p.bounding_box.intersects_xz(
                                cx << 4,
                                cz << 4,
                                (cx << 4) + 15,
                                (cz << 4) + 15,
                            )
                        })
                    {
                        out.insert(s.structure_id.path().to_owned());
                    }
                }
            }
        }
    }
    out
}

/// Starts of every structure type of the dimension (`STRUCT_PER_ID` each where the world has
/// them), found with `zenith-structures`; vanilla verifies the placed pieces.
fn pick_structure_starts(
    gen_: &zenith_structures::StructureGenerator,
    store: &mut zenith_features::StartStore,
    seed: i64,
) -> Vec<zenith_structures::StructureStart> {
    use zenith_structures::{StructurePlacement, StructureRef, potential_random_spread_chunk};
    let mut picked: Vec<zenith_structures::StructureStart> = Vec::new();
    let sets: Vec<_> = gen_.placement_state().possible_structure_sets().to_vec();
    for set in &sets {
        let ids: Vec<Identifier> = set
            .structures
            .iter()
            .map(|e| match &e.structure {
                StructureRef::Reference(id) => id.clone(),
                StructureRef::Inline(d) => d.id.clone(),
            })
            .collect();
        let mut counts: BTreeMap<Identifier, usize> = BTreeMap::new();
        let satisfied = |c: &BTreeMap<Identifier, usize>| {
            ids.iter()
                .all(|id| c.get(id).copied().unwrap_or(0) >= STRUCT_PER_ID)
        };
        let mut take = |cx: i32,
                        cz: i32,
                        counts: &mut BTreeMap<Identifier, usize>,
                        store: &mut zenith_features::StartStore| {
            let starts = store
                .entry((cx, cz))
                .or_insert_with(|| gen_.structure_starts(cx, cz));
            for s in starts.iter() {
                if s.is_valid() && ids.contains(&s.structure_id) {
                    let c = counts.entry(s.structure_id.clone()).or_insert(0);
                    if *c < STRUCT_PER_ID {
                        *c += 1;
                        picked.push(s.clone());
                    }
                }
            }
        };
        match &set.placement {
            StructurePlacement::DimensionOrigin => {}
            StructurePlacement::ConcentricRings { .. } => {
                for (cx, cz) in gen_.ring_positions_for(set).into_iter().take(24) {
                    take(cx, cz, &mut counts, store);
                    if satisfied(&counts) {
                        break;
                    }
                }
            }
            StructurePlacement::RandomSpread {
                common,
                spacing,
                separation,
                spread_type,
            } => {
                let mut checked = 0usize;
                'rings: for r in 0..=STRUCT_MAX_RADIUS {
                    for gx in -r..=r {
                        for gz in -r..=r {
                            if gx.abs().max(gz.abs()) != r {
                                continue;
                            }
                            let (cx, cz) = potential_random_spread_chunk(
                                seed,
                                gx * spacing,
                                gz * spacing,
                                *spacing,
                                *separation,
                                *spread_type,
                                common.salt,
                            );
                            // Stay away from the world spawn (the server generates it fully).
                            if cx.abs() <= 40 && cz.abs() <= 40 {
                                continue;
                            }
                            if !gen_.is_structure_chunk(set, cx, cz) {
                                continue;
                            }
                            checked += 1;
                            take(cx, cz, &mut counts, store);
                            if checked >= STRUCT_MAX_CHECKS || satisfied(&counts) {
                                break 'rings;
                            }
                        }
                    }
                }
            }
        }
    }
    picked
}

/// Chunks to compare for the picked starts: for each start its own chunk and chunks spread over
/// its pieces (more of them for the big structure types).
fn structure_chunks(
    picked: &[zenith_structures::StructureStart],
    taken: &mut HashSet<(i32, i32)>,
) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    for s in picked {
        let all = start_piece_chunks(s);
        let budget = if BIG_STRUCTURES.contains(&s.structure_id.path()) {
            CHUNKS_PER_BIG_START
        } else {
            CHUNKS_PER_START
        };
        let mut chosen: Vec<(i32, i32)> = Vec::new();
        if all.contains(&(s.chunk_x, s.chunk_z)) {
            chosen.push((s.chunk_x, s.chunk_z));
        }
        let n = all.len();
        let take = budget.min(n);
        for i in 0..take {
            if chosen.len() >= budget {
                break;
            }
            let c = all[(i * n + n / 2) / take.max(1) % n.max(1)];
            if !chosen.contains(&c) {
                chosen.push(c);
            }
        }
        for c in chosen {
            // The vanilla server generates the area around the spawn completely.
            if (c.0.abs() > 32 || c.1.abs() > 32) && taken.insert(c) {
                out.push(c);
            }
        }
    }
    out
}

/// Overworld chunks whose features step calls a fossil / template feature (found with a
/// structure-free decoration pass), preferring biomes that carry those features.
fn pick_template_chunks(
    pipe: &DimPipeline,
    data: &FeatureData,
    seed: i64,
    taken: &HashSet<(i32, i32)>,
) -> Vec<(i32, i32)> {
    let mut rng = crate::parity_terrain::Lcg::new((seed as u64) ^ 0x7E3F_1A55_C0DE_0001);
    let has_template = |id: &Identifier| {
        data.biome(id).is_some_and(|b| {
            b.features.iter().flatten().any(|pf| {
                feature_any(&pf.feature, &|f| {
                    matches!(
                        f,
                        zenith_features::Feature::Fossil(_) | zenith_features::Feature::Template(_)
                    )
                })
            })
        })
    };
    let mut out: Vec<(i32, i32)> = Vec::new();
    let (mut fossils, mut templates) = (0usize, 0usize);
    for _ in 0..TEMPLATE_SCAN {
        if out.len() >= TEMPLATE_CHUNKS_PER_SEED {
            break;
        }
        let cx = rng.range(-900, 900) as i32;
        let cz = rng.range(-900, 900) as i32;
        if cx.abs() <= 32 && cz.abs() <= 32 {
            continue;
        }
        if taken.contains(&(cx, cz)) || !well_spaced(&out, cx, cz) {
            continue;
        }
        let Some(b) = quick_surface_biome(&pipe.biome_source, &pipe.rs, cx, cz) else {
            continue;
        };
        if !has_template(&b) {
            continue;
        }
        let mut tchunks = Vec::with_capacity(9);
        for dz in -1..=1 {
            for dx in -1..=1 {
                tchunks.push(pipe.terrain.generate_chunk(cx + dx, cz + dz));
            }
        }
        let refs: [&TerrainChunk; 9] = std::array::from_fn(|i| &tchunks[i]);
        let mut region = pipe.decorator.make_region(cx, cz, refs);
        pipe.template_counter.reset();
        pipe.decorator.decorate(&mut region, cx, cz);
        let (f, t) = (
            pipe.template_counter.fossil_placed(),
            pipe.template_counter.template_placed(),
        );
        // Wanted: placements; keep the mix balanced between fossils and templates.
        if f + t > 0 && (f == 0 || fossils <= templates || t > 0) {
            fossils += usize::from(f > 0);
            templates += usize::from(t > 0);
            out.push((cx, cz));
        }
    }
    out
}

fn run_structures_world_job(
    ctx: &FeatureContextData,
    version: &str,
    seed: i64,
    timing: &Mutex<Timing>,
) -> anyhow::Result<Tally> {
    let mut t = Tally {
        failure_cap: 400,
        diff_limit: 120,
        ..Tally::default()
    };
    let dims = [
        "minecraft:overworld",
        "minecraft:the_nether",
        "minecraft:the_end",
    ];
    let mut pipes: HashMap<&str, DimPipeline> = HashMap::new();
    let mut requests: Vec<ChunkRequest> = Vec::new();
    let mut labels: HashMap<(String, i32, i32), &'static str> = HashMap::new();

    for &dim in &dims {
        let pipe = build_dim_pipeline(ctx, dim, seed, true)?;
        let hook = pipe.structures.as_ref().expect("structures on");
        let mut picked = hook.with_starts(|g, store| pick_structure_starts(g, store, seed));
        // Debug aid: `ZENITH_STRUCT_ONLY=a,b` keeps only starts whose structure id contains a
        // listed word (and the fossil/template chunks when `fossil` is listed).
        let only: Option<Vec<String>> = std::env::var("ZENITH_STRUCT_ONLY")
            .ok()
            .map(|v| v.split(',').map(|x| x.trim().to_owned()).collect());
        if let Some(words) = &only {
            picked.retain(|s| {
                words
                    .iter()
                    .any(|w| s.structure_id.path().contains(w.as_str()))
            });
        }
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for s in &picked {
            *counts.entry(s.structure_id.path().to_owned()).or_default() += 1;
        }
        let mut taken: HashSet<(i32, i32)> = HashSet::new();
        let mut chunks = structure_chunks(&picked, &mut taken);
        // Debug aid: `ZENITH_STRUCT_NEIGHBORS=1` also compares the 8 neighbours of every chunk
        // (requested after all the primary chunks).
        if std::env::var_os("ZENITH_STRUCT_NEIGHBORS").is_some() {
            let primary = chunks.clone();
            for &(cx, cz) in &primary {
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        let c = (cx + dx, cz + dz);
                        if taken.insert(c) && (c.0.abs() > 32 || c.1.abs() > 32) {
                            chunks.push(c);
                        }
                    }
                }
            }
        }
        println!(
            "  select {dim} seed {seed}: {} starts of {} structure types, {} chunks",
            picked.len(),
            counts.len(),
            chunks.len()
        );
        for &(cx, cz) in &chunks {
            labels.insert((dim.to_owned(), cx, cz), "structure");
            requests.push(ChunkRequest {
                dimension: dim.to_owned(),
                x: cx,
                z: cz,
                status: "features".to_owned(),
            });
        }
        if dim == "minecraft:overworld"
            && only
                .as_ref()
                .is_none_or(|w| w.iter().any(|x| x == "fossil"))
        {
            // Fossil / template chunks come last in the request order, away from every structure.
            let plain = build_dim_pipeline(ctx, dim, seed, false)?;
            let mut near: HashSet<(i32, i32)> = HashSet::new();
            for &(cx, cz) in &taken {
                for dz in -3..=3 {
                    for dx in -3..=3 {
                        near.insert((cx + dx, cz + dz));
                    }
                }
            }
            let tpl = pick_template_chunks(&plain, &ctx.features, seed, &near);
            println!(
                "  select {dim} seed {seed}: {} fossil/template chunks",
                tpl.len()
            );
            for (cx, cz) in tpl {
                labels.insert((dim.to_owned(), cx, cz), "fossil/template");
                requests.push(ChunkRequest {
                    dimension: dim.to_owned(),
                    x: cx,
                    z: cz,
                    status: "features".to_owned(),
                });
            }
        }
        pipes.insert(dim, pipe);
    }

    let world = ChunkWorld {
        seed,
        level_type: "minecraft:normal".to_owned(),
        structures: true,
    };
    let van_start = Instant::now();
    let raw = dump_chunks(version, &world, &requests)?;
    let van_ms = van_start.elapsed().as_millis();
    let dumped = parse_chunk_dump(&raw)?;
    anyhow::ensure!(
        dumped.len() == requests.len(),
        "dumped chunk count {} != requested {}",
        dumped.len(),
        requests.len()
    );

    // Chunk state carried over between requests, in request order (like the vanilla server's
    // chunks): a later chunk's decoration sees what earlier decorations wrote into it.
    let mut world_chunks: HashMap<(&str, i32, i32), RegionChunk> = HashMap::new();
    let (mut terrain_us, mut features_us) = (0u128, 0u128);
    for dc in &dumped {
        let dim: &str = dims
            .iter()
            .copied()
            .find(|d| *d == dc.dimension)
            .with_context(|| format!("unexpected dimension {}", dc.dimension))?;
        let pipe = &pipes[dim];
        let hook = pipe.structures.as_ref().expect("structures on");
        let t0 = Instant::now();
        let mut chunks: Vec<RegionChunk> = Vec::with_capacity(9);
        for dz in -1..=1 {
            for dx in -1..=1 {
                let key = (dim, dc.x + dx, dc.z + dz);
                let rc = match world_chunks.remove(&key) {
                    Some(rc) => rc,
                    None => {
                        let beard = hook.beardifier_for_chunk(key.1, key.2);
                        let sampler: Option<Arc<dyn zenith_worldgen::ContextSampler>> =
                            if beard.is_empty() {
                                None
                            } else {
                                Some(Arc::new(beard))
                            };
                        let tc = pipe.terrain.generate_chunk_with_hooks(
                            key.1,
                            key.2,
                            sampler,
                            &zenith_worldgen::Blender::empty(),
                        );
                        RegionChunk::from_terrain_chunk(&tc, &ctx.ops)
                    }
                };
                chunks.push(rc);
            }
        }
        terrain_us += t0.elapsed().as_micros();

        let t1 = Instant::now();
        let arr: [RegionChunk; 9] = chunks
            .try_into()
            .map_err(|_| anyhow::anyhow!("region chunk count"))?;
        let mut region = pipe.decorator.make_region_from_chunks(dc.x, dc.z, arr);
        pipe.template_counter.reset();
        pipe.decorator.decorate(&mut region, dc.x, dc.z);
        features_us += t1.elapsed().as_micros();

        let types = hook.with_starts(|_, store| types_at_chunk(store, dc.x, dc.z));
        let tag = format!(
            "{} seed {seed} ({},{}) [{}]",
            dc.dimension,
            dc.x,
            dc.z,
            types.iter().cloned().collect::<Vec<_>>().join(",")
        );
        let ok = compare_chunk(ctx, &mut t, dc, region.center_chunk(), &[], &tag);

        let kind = labels
            .get(&(dc.dimension.clone(), dc.x, dc.z))
            .copied()
            .unwrap_or("structure");
        let mut groups: Vec<String> = types
            .iter()
            .map(|s| format!("{dim_short}:{s}", dim_short = short_dim(dim)))
            .collect();
        if kind == "fossil/template" || pipe.template_counter.placed() > 0 {
            groups.push("fossil/template".to_owned());
        }
        if groups.is_empty() {
            groups.push("no structure".to_owned());
        }
        groups.push("ALL chunks".to_owned());
        for g in groups {
            let e = t.groups.entry(g).or_default();
            e.1 += 1;
            e.0 += u64::from(ok);
        }

        let back = region.into_chunks();
        for (i, rc) in back.into_iter().enumerate() {
            let (dx, dz) = ((i % 3) as i32 - 1, (i / 3) as i32 - 1);
            world_chunks.insert((dim, dc.x + dx, dc.z + dz), rc);
        }
    }

    {
        let mut g = timing.lock().expect("timing");
        g.vanilla_ms += van_ms;
        g.terrain_us += terrain_us;
        g.features_us += features_us;
        g.chunks += dumped.len();
    }
    Ok(t)
}

fn short_dim(dim: &str) -> &'static str {
    match dim {
        "minecraft:overworld" => "ow",
        "minecraft:the_nether" => "nether",
        _ => "end",
    }
}

/// `cargo xtask parity golden-structures`: features-status chunks with structures on.
pub fn golden_structures(version: &str, write: bool) -> anyhow::Result<bool> {
    let ctx = load_context(version)?;
    let timing = Mutex::new(Timing::default());
    let golden: Vec<i64> = std::env::var("ZENITH_FEATURES_SEEDS")
        .ok()
        .map(|v| v.split(',').filter_map(|x| x.trim().parse().ok()).collect())
        .unwrap_or_else(|| GOLDEN_SEEDS.to_vec());
    let results = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for &seed in &golden {
            let (ctx, timing, results) = (&ctx, &timing, &results);
            // Jigsaw assembly and template placement are deeply recursive in debug builds.
            std::thread::Builder::new()
                .stack_size(256 << 20)
                .spawn_scoped(scope, move || {
                    let r = run_structures_world_job(ctx, version, seed, timing).inspect(|t| {
                        println!("  world seed {seed}: {}/{}", t.passed, t.total);
                    });
                    results.lock().expect("results").push(r);
                })
                .expect("spawn world job");
        }
    });
    let mut tally = Tally {
        failure_cap: 400,
        ..Tally::default()
    };
    for r in results.into_inner().expect("results") {
        tally.merge(r?);
    }
    let tm = timing.into_inner().expect("timing");
    println!(
        "golden-structures timing: {} chunks — features {:.2} ms/chunk (3x3 terrain+beardifier {:.2} ms/chunk) | vanilla server runs {} ms total",
        tm.chunks,
        tm.features_us as f64 / 1000.0 / tm.chunks.max(1) as f64,
        tm.terrain_us as f64 / 1000.0 / tm.chunks.max(1) as f64,
        tm.vanilla_ms
    );
    println!(
        "golden-structures {version}: {}/{} identical (blocks, biomes, 6 heightmaps, post-processing marks, tick counts; {} seeds, structures on; block entities and entities are not in the dump yet)",
        tally.passed,
        tally.total,
        golden.len()
    );
    for (group, (p, n)) in &tally.groups {
        println!("  group {group:<34} {p}/{n} chunks identical");
    }
    for f in &tally.failures {
        println!("  FAIL {f}");
    }
    if write {
        SuiteResult::new("golden-structures", version, tally.passed, tally.total)
            .write(&repo_root().join("progress/parity"))?;
    }
    Ok(tally.passed == tally.total)
}
