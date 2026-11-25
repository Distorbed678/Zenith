//! Structure starts & references parity suite (`cargo xtask parity structure-starts [--write]`).
//!
//! Compares `zenith-structures` (`StructureGenerator::structure_starts` and
//! `StructureGenerator::structure_references_packed_with_cache`) against the vanilla dedicated
//! server's `ZTC1` chunk dump (`crate::oracle::dump_chunks`) across 3 seeds × 3 normal-preset
//! dimensions (Overworld, Nether, End), covering every structure set and structure type plus
//! random negative-test chunks and `structure_references` chunks.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Context;
use zenith_biome::BiomeSource;
use zenith_blocks::{BlockFactTags, BlockFacts};
use zenith_codec::json::{Json, ParseMode, parse_bytes};
use zenith_codec::log::null_log;
use zenith_codec::resources::ZipPackResources;
use zenith_parity::SuiteResult;
use zenith_registry::{BlockStateTable, Identifier};
use zenith_structures::{
    BoundingBox, StructureData, StructureGenerator, StructurePlacement, StructureStart,
    TemplateManager, potential_random_spread_chunk,
};
use zenith_worldgen::{BlockTags, NoiseGenerator, WorldgenData};

use crate::oracle::{self, ChunkRequest, ChunkWorld, DumpedStart};
use crate::repo_root;

const SEEDS: &[i64] = &[12345, -1, 1_234_567_890_123_456_789, 0];

/// Dimension descriptor for normal preset: `(dim_id, chunk_min_y, chunk_height)`.
const DIMS: &[(&str, i32, i32)] = &[
    ("minecraft:overworld", -64, 384),
    ("minecraft:the_nether", 0, 256),
    ("minecraft:the_end", 0, 256),
];

struct SharedData {
    worldgen_data: WorldgenData,
    structure_data: Arc<StructureData>,
    template_manager: Arc<TemplateManager>,
    blocks: BlockStateTable,
    tags: BlockTags,
    block_facts: Arc<BlockFacts>,
    jar: std::path::PathBuf,
}

fn load_shared(version: &str) -> anyhow::Result<SharedData> {
    let install = zenith_install::discover_installs(Some(version))
        .into_iter()
        .next()
        .context("no install found")?;
    let tables = zenith_data::Tables::load(&zenith_data::version_dir(version).join("tables.bin"))
        .context("no cached tables; run `cargo run -p zenith-data -- ingest` first")?;
    let blocks = BlockStateTable::from_tables(&tables)
        .map_err(|e| anyhow::anyhow!("build block-state table: {e}"))?;
    let pack = Arc::new(ZipPackResources::open(
        "vanilla",
        &install.client_jar,
        null_log(),
    ));
    let worldgen_data = WorldgenData::load(pack.as_ref())
        .map_err(|e| anyhow::anyhow!("load worldgen data: {e}"))?;
    let tags = BlockTags::load(pack.as_ref(), &blocks)
        .map_err(|e| anyhow::anyhow!("load block tags: {e}"))?;
    let fact_tags = BlockFactTags::from_resolver(&blocks, |name| {
        Identifier::parse(name)
            .ok()
            .and_then(|id| tags.block_ids(&id))
    });
    let block_facts = Arc::new(
        BlockFacts::build(&blocks, &fact_tags)
            .map_err(|e| anyhow::anyhow!("build block facts: {e}"))?,
    );
    let structure_data = Arc::new(
        StructureData::load(pack.as_ref(), worldgen_data.biome_tags())
            .map_err(|e| anyhow::anyhow!("load structure data: {e}"))?,
    );
    let template_manager = Arc::new(TemplateManager::new(
        pack,
        Arc::new(blocks.clone()),
        Some(Arc::clone(&block_facts)),
    ));

    Ok(SharedData {
        worldgen_data,
        structure_data,
        template_manager,
        blocks,
        tags,
        block_facts,
        jar: install.client_jar,
    })
}

fn preset_dimension(
    jar: &std::path::Path,
    preset: &str,
    dimension: &str,
) -> anyhow::Result<(Json, String)> {
    let mut zip = zip::ZipArchive::new(fs::File::open(jar)?)?;
    let mut b = Vec::new();
    zip.by_name(&format!(
        "data/minecraft/worldgen/world_preset/{preset}.json"
    ))?
    .read_to_end(&mut b)?;
    let j = parse_bytes(&b, ParseMode::Data).context("parse world preset")?;
    let pair = (|| {
        let Json::Object(root) = &j else { return None };
        let Json::Object(dims) = root.get("dimensions")? else {
            return None;
        };
        let Json::Object(dim) = dims.get(dimension)? else {
            return None;
        };
        let Json::Object(g) = dim.get("generator")? else {
            return None;
        };
        Some((
            g.get("biome_source")?.clone(),
            g.get("settings")?.as_str()?.to_owned(),
        ))
    })();
    pair.with_context(|| format!("world preset {preset}: no generator for {dimension}"))
}

struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed ^ 0x9E37_79B9_7F4A_7C15)
    }
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 11
    }
    fn range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + (self.next() % (hi - lo) as u64) as i32
    }
}

/// Generates spiral grid coordinates `(gx, gz)` up to Chebyshev radius `max_radius`.
fn spiral_cells(max_radius: i32) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    out.push((0, 0));
    for r in 1..=max_radius {
        for x in -r..=r {
            out.push((x, -r));
            out.push((x, r));
        }
        for z in (-r + 1)..=(r - 1) {
            out.push((-r, z));
            out.push((r, z));
        }
    }
    out
}

#[derive(Default, Clone)]
struct TypeStats {
    starts_passed: usize,
    starts_total: usize,
    pieces_passed: usize,
    pieces_total: usize,
    refs_passed: usize,
    refs_total: usize,
}

#[derive(Default)]
struct SeedReport {
    passed: usize,
    total: usize,
    by_type: BTreeMap<String, TypeStats>,
    failures: Vec<String>,
    chunks_checked: usize,
    vanilla_ms: u128,
    zenith_ms: u128,
}

impl SeedReport {
    fn record_check(&mut self, ok: bool, msg: impl FnOnce() -> String) {
        self.total += 1;
        if ok {
            self.passed += 1;
        } else if self.failures.len() < 40 {
            self.failures.push(msg());
        }
    }

    fn merge(&mut self, other: SeedReport) {
        self.passed += other.passed;
        self.total += other.total;
        self.chunks_checked += other.chunks_checked;
        self.vanilla_ms += other.vanilla_ms;
        self.zenith_ms += other.zenith_ms;
        for (k, v) in other.by_type {
            let e = self.by_type.entry(k).or_default();
            e.starts_passed += v.starts_passed;
            e.starts_total += v.starts_total;
            e.pieces_passed += v.pieces_passed;
            e.pieces_total += v.pieces_total;
            e.refs_passed += v.refs_passed;
            e.refs_total += v.refs_total;
        }
        for f in other.failures {
            if self.failures.len() < 60 {
                self.failures.push(f);
            }
        }
    }
}

fn bb_arr(bb: BoundingBox) -> [i32; 6] {
    [bb.min_x, bb.min_y, bb.min_z, bb.max_x, bb.max_y, bb.max_z]
}

fn dir_name(d: zenith_structures::Direction) -> &'static str {
    match d {
        zenith_structures::Direction::Down => "down",
        zenith_structures::Direction::Up => "up",
        zenith_structures::Direction::North => "north",
        zenith_structures::Direction::South => "south",
        zenith_structures::Direction::West => "west",
        zenith_structures::Direction::East => "east",
    }
}

fn compare_starts(
    report: &mut SeedReport,
    tag: &str,
    vanilla: &[DumpedStart],
    zenith: &[StructureStart],
) {
    // Vanilla chunk.getAllStarts() may in theory contain invalid starts; filter to valid starts
    // while flagging any unexpected invalid starts if present.
    let mut van_map: BTreeMap<String, &DumpedStart> = BTreeMap::new();
    for s in vanilla {
        if s.valid {
            van_map.insert(s.structure.clone(), s);
        }
    }
    let mut zen_map: BTreeMap<String, &StructureStart> = BTreeMap::new();
    for s in zenith {
        if s.is_valid() {
            zen_map.insert(s.structure_id.to_string(), s);
        }
    }

    let mut all_ids: Vec<String> = van_map.keys().chain(zen_map.keys()).cloned().collect();
    all_ids.sort();
    all_ids.dedup();

    if all_ids.is_empty() {
        // Negative-test chunk: both agree there are no starts.
        report.record_check(true, String::new);
        return;
    }

    for sid in all_ids {
        let stats = report.by_type.entry(sid.clone()).or_default();
        stats.starts_total += 1;

        match (van_map.get(&sid), zen_map.get(&sid)) {
            (Some(v), Some(z)) => {
                let header_ok = v.chunk_x == z.chunk_x
                    && v.chunk_z == z.chunk_z
                    && v.valid == z.is_valid()
                    && v.bounds == bb_arr(z.bounding_box)
                    && v.pieces.len() == z.pieces.len();

                let mut pieces_ok = v.pieces.len() == z.pieces.len();
                let mut first_piece_diff: Option<String> = None;

                let n = v.pieces.len().max(z.pieces.len());
                for i in 0..n {
                    stats.pieces_total += 1;
                    match (v.pieces.get(i), z.pieces.get(i)) {
                        (Some(vp), Some(zp)) => {
                            let z_orient = zp.orientation.map_or("", dir_name);
                            let ok = vp.kind == zp.kind
                                && vp.bounds == bb_arr(zp.bounding_box)
                                && vp.orientation == z_orient
                                && vp.depth == zp.gen_depth;
                            if ok {
                                stats.pieces_passed += 1;
                            } else {
                                pieces_ok = false;
                                if first_piece_diff.is_none() {
                                    first_piece_diff = Some(format!(
                                        "piece[{i}]: vanilla=({}, {:?}, '{}', gd={}) | zenith=({}, {:?}, '{}', gd={})",
                                        vp.kind,
                                        vp.bounds,
                                        vp.orientation,
                                        vp.depth,
                                        zp.kind,
                                        bb_arr(zp.bounding_box),
                                        z_orient,
                                        zp.gen_depth
                                    ));
                                }
                            }
                        }
                        (Some(vp), None) => {
                            pieces_ok = false;
                            if first_piece_diff.is_none() {
                                first_piece_diff = Some(format!(
                                    "piece[{i}] missing in zenith: vanilla=({}, {:?}, '{}', gd={})",
                                    vp.kind, vp.bounds, vp.orientation, vp.depth
                                ));
                            }
                        }
                        (None, Some(zp)) => {
                            pieces_ok = false;
                            if first_piece_diff.is_none() {
                                first_piece_diff = Some(format!(
                                    "piece[{i}] extra in zenith: zenith=({}, {:?}, '{}', gd={})",
                                    zp.kind,
                                    bb_arr(zp.bounding_box),
                                    zp.orientation.map_or("", dir_name),
                                    zp.gen_depth
                                ));
                            }
                        }
                        (None, None) => {}
                    }
                }

                let all_ok = header_ok && pieces_ok;
                if all_ok {
                    stats.starts_passed += 1;
                }
                report.record_check(all_ok, || {
                    format!(
                        "{tag} {sid}: bounds van={:?} zen={:?}, pieces van={} zen={}{}",
                        v.bounds,
                        bb_arr(z.bounding_box),
                        v.pieces.len(),
                        z.pieces.len(),
                        first_piece_diff
                            .as_ref()
                            .map_or(String::new(), |d| format!("; {d}"))
                    )
                });
            }
            (Some(v), None) => {
                stats.pieces_total += v.pieces.len();
                report.record_check(false, || {
                    format!(
                        "{tag} {sid}: MISSING in zenith (vanilla has {} pieces, bounds={:?})",
                        v.pieces.len(),
                        v.bounds
                    )
                });
            }
            (None, Some(z)) => {
                stats.pieces_total += z.pieces.len();
                report.record_check(false, || {
                    format!(
                        "{tag} {sid}: EXTRA in zenith ({} pieces, bounds={:?})",
                        z.pieces.len(),
                        bb_arr(z.bounding_box)
                    )
                });
            }
            (None, None) => {}
        }
    }
}

fn compare_references(
    report: &mut SeedReport,
    tag: &str,
    vanilla: &[(String, Vec<i64>)],
    zenith: &[(Identifier, Vec<i64>)],
) {
    let van_map: BTreeMap<String, &[i64]> = vanilla
        .iter()
        .map(|(k, v)| (k.clone(), v.as_slice()))
        .collect();
    let zen_map: BTreeMap<String, &[i64]> = zenith
        .iter()
        .map(|(k, v)| (k.to_string(), v.as_slice()))
        .collect();

    let mut all_ids: Vec<String> = van_map.keys().chain(zen_map.keys()).cloned().collect();
    all_ids.sort();
    all_ids.dedup();

    if all_ids.is_empty() {
        report.record_check(true, String::new);
        return;
    }

    for sid in all_ids {
        let stats = report.by_type.entry(sid.clone()).or_default();
        stats.refs_total += 1;
        let v = van_map.get(&sid).copied().unwrap_or(&[]);
        let z = zen_map.get(&sid).copied().unwrap_or(&[]);
        let ok = v == z;
        if ok {
            stats.refs_passed += 1;
        }
        report.record_check(ok, || {
            let fmt_chunks = |s: &[i64]| -> Vec<(i32, i32)> {
                s.iter().map(|&p| (p as i32, (p >> 32) as i32)).collect()
            };
            format!(
                "{tag} refs {sid}: vanilla={:?} | zenith={:?}",
                fmt_chunks(v),
                fmt_chunks(z)
            )
        });
    }
}

struct CandidateChunks {
    starts: Vec<(i32, i32)>,
    refs: Vec<(i32, i32)>,
}

fn select_candidates(
    generator: &StructureGenerator,
    dim_id: &str,
    seed: i64,
    starts_cache: &mut HashMap<(i32, i32), Vec<StructureStart>>,
) -> CandidateChunks {
    let mut start_chunks: Vec<(i32, i32)> = Vec::new();
    let mut seen_start: HashSet<(i32, i32)> = HashSet::new();
    let mut ref_chunks: Vec<(i32, i32)> = Vec::new();
    let mut seen_ref: HashSet<(i32, i32)> = HashSet::new();

    let mut add_start = |cx: i32, cz: i32| {
        if seen_start.insert((cx, cz)) {
            start_chunks.push((cx, cz));
        }
    };
    let mut add_ref = |cx: i32, cz: i32, max_refs: usize| {
        if ref_chunks.len() < max_refs && seen_ref.insert((cx, cz)) {
            ref_chunks.push((cx, cz));
        }
    };

    let cells_default = spiral_cells(32);
    let cells_dense = spiral_cells(120);

    for set in generator.placement_state().possible_structure_sets() {
        let target_per_struct = if set.structures.len() == 1 { 2 } else { 1 };
        let mut found_counts: HashMap<Identifier, usize> = HashMap::new();
        let target_ids: Vec<Identifier> = set
            .structures
            .iter()
            .map(|e| match &e.structure {
                zenith_structures::StructureRef::Reference(id) => id.clone(),
                zenith_structures::StructureRef::Inline(d) => d.id.clone(),
            })
            .collect();

        let all_satisfied = |counts: &HashMap<Identifier, usize>| -> bool {
            target_ids
                .iter()
                .all(|id| counts.get(id).copied().unwrap_or(0) >= target_per_struct)
        };

        let mut try_chunk = |cx: i32, cz: i32, counts: &mut HashMap<Identifier, usize>| -> bool {
            let starts = starts_cache
                .entry((cx, cz))
                .or_insert_with(|| generator.structure_starts(cx, cz));
            let mut matched_new = false;
            for s in starts.iter() {
                if target_ids.contains(&s.structure_id) {
                    let c = counts.entry(s.structure_id.clone()).or_insert(0);
                    if *c < target_per_struct {
                        *c += 1;
                        matched_new = true;
                    }
                }
            }
            matched_new
        };

        match &set.placement {
            StructurePlacement::DimensionOrigin => {
                let (ox, oz) = generator.placement_state().dimension_origin();
                let _ = try_chunk(ox, oz, &mut found_counts);
                add_start(ox, oz);
                add_ref(ox, oz, usize::MAX);
                add_ref(ox + 1, oz, usize::MAX);
            }
            StructurePlacement::ConcentricRings { .. } => {
                let positions = generator.ring_positions_for(set);
                let mut picked = 0usize;
                for &(cx, cz) in positions.iter().take(16) {
                    if try_chunk(cx, cz, &mut found_counts) {
                        add_start(cx, cz);
                        if picked == 0 {
                            add_ref(cx, cz, usize::MAX);
                            add_ref(cx + 1, cz + 1, usize::MAX);
                        }
                        picked += 1;
                        if all_satisfied(&found_counts) {
                            break;
                        }
                    }
                }
                // Always include the first ring position even if biome/validity differed.
                if let Some(&(cx, cz)) = positions.first() {
                    add_start(cx, cz);
                }
            }
            StructurePlacement::RandomSpread {
                common,
                spacing,
                separation,
                spread_type,
            } => {
                let mut checked = 0usize;
                let mut ref_added_for_set = false;
                let (cells, max_checks) = if *spacing <= 2 {
                    (&cells_dense, 2400usize)
                } else if *spacing >= 50 {
                    (&cells_default, 400usize)
                } else {
                    (&cells_default, 900usize)
                };
                let mut first_candidate: Option<(i32, i32)> = None;
                for &(gx, gz) in cells {
                    if checked >= max_checks || all_satisfied(&found_counts) {
                        break;
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
                    if !generator.is_structure_chunk(set, cx, cz) {
                        continue;
                    }
                    checked += 1;
                    if first_candidate.is_none() {
                        first_candidate = Some((cx, cz));
                    }
                    if try_chunk(cx, cz, &mut found_counts) {
                        add_start(cx, cz);
                        if !ref_added_for_set {
                            ref_added_for_set = true;
                            add_ref(cx, cz, usize::MAX);
                            add_ref(cx + 1, cz, usize::MAX);
                        }
                    }
                }
                if found_counts.is_empty()
                    && let Some((cx, cz)) = first_candidate
                {
                    add_start(cx, cz);
                }
            }
        }
    }

    // Add deterministic pseudo-random chunks (negative tests + edge coords).
    add_start(0, 0);
    let mut rng = Lcg::new((seed as u64) ^ (dim_id.len() as u64).wrapping_mul(0x517C_C1B7));
    for _ in 0..8 {
        let cx = rng.range(-128, 128);
        let cz = rng.range(-128, 128);
        add_start(cx, cz);
    }
    for _ in 0..2 {
        let cx = rng.range(-128, 128);
        let cz = rng.range(-128, 128);
        add_ref(cx, cz, usize::MAX);
    }

    CandidateChunks {
        starts: start_chunks,
        refs: ref_chunks,
    }
}

fn run_seed(shared: &SharedData, version: &str, seed: i64) -> anyhow::Result<SeedReport> {
    let mut gens: HashMap<&'static str, StructureGenerator> = HashMap::new();
    let mut caches: HashMap<&'static str, HashMap<(i32, i32), Vec<StructureStart>>> =
        HashMap::new();
    let mut requests: Vec<ChunkRequest> = Vec::new();
    let mut seen_req: HashSet<(&str, i32, i32, &str)> = HashSet::new();

    let t_zen_start = Instant::now();
    for &(dim_id, min_y, height) in DIMS {
        let (bs_json, settings_str) = preset_dimension(&shared.jar, "normal", dim_id)?;
        let biome_source = match BiomeSource::decode_json(&bs_json) {
            zenith_codec::codec::DataResult::Success(bs) => bs,
            other => anyhow::bail!("decode biome source for {dim_id}: {other:?}"),
        };
        let settings_id = Identifier::parse(&settings_str)
            .map_err(|e| anyhow::anyhow!("settings id {settings_str}: {e}"))?;
        let noise_gen = Arc::new(
            NoiseGenerator::new(
                &shared.worldgen_data,
                &settings_id,
                seed,
                &shared.blocks,
                &shared.tags,
            )
            .map_err(|e| anyhow::anyhow!("noise generator for {dim_id}: {e}"))?,
        );
        let generator = StructureGenerator::new(
            seed,
            &shared.worldgen_data,
            Arc::clone(&shared.structure_data),
            Arc::clone(&shared.template_manager),
            noise_gen,
            biome_source,
            Arc::clone(&shared.block_facts),
            &shared.blocks,
            min_y,
            height,
        );
        let mut cache = HashMap::new();
        let candidates = select_candidates(&generator, dim_id, seed, &mut cache);
        for (cx, cz) in candidates.starts {
            if seen_req.insert((dim_id, cx, cz, "structure_starts")) {
                requests.push(ChunkRequest {
                    dimension: dim_id.to_owned(),
                    x: cx,
                    z: cz,
                    status: "structure_starts".to_owned(),
                });
            }
        }
        for (cx, cz) in candidates.refs {
            if seen_req.insert((dim_id, cx, cz, "structure_references")) {
                requests.push(ChunkRequest {
                    dimension: dim_id.to_owned(),
                    x: cx,
                    z: cz,
                    status: "structure_references".to_owned(),
                });
            }
        }
        gens.insert(dim_id, generator);
        caches.insert(dim_id, cache);
    }
    let candidate_ms = t_zen_start.elapsed().as_millis();

    let t_van = Instant::now();
    let raw = oracle::dump_chunks(
        version,
        &ChunkWorld {
            seed,
            level_type: "minecraft:normal".to_owned(),
            structures: true,
        },
        &requests,
    )?;
    let vanilla_ms = t_van.elapsed().as_millis();

    let dumped = oracle::parse_chunk_dump(&raw)?;
    anyhow::ensure!(
        dumped.len() == requests.len(),
        "seed {seed}: dumped {} chunks for {} requests",
        dumped.len(),
        requests.len()
    );

    let mut report = SeedReport {
        vanilla_ms,
        zenith_ms: candidate_ms,
        ..SeedReport::default()
    };

    let t_cmp = Instant::now();
    for dc in &dumped {
        report.chunks_checked += 1;
        let dim_key = dc.dimension.as_str();
        let generator = gens
            .get(dim_key)
            .with_context(|| format!("unknown dimension {dim_key}"))?;
        let cache = caches.get_mut(dim_key).expect("cache exists");

        let tag = format!(
            "seed={seed} {} ({},{}) [{}]",
            dc.dimension, dc.x, dc.z, dc.requested
        );
        let z_starts = cache
            .entry((dc.x, dc.z))
            .or_insert_with(|| generator.structure_starts(dc.x, dc.z))
            .clone();
        compare_starts(&mut report, &tag, &dc.starts, &z_starts);

        if dc.requested == "structure_references" {
            let z_refs = generator.structure_references_packed_with_cache(dc.x, dc.z, cache);
            compare_references(&mut report, &tag, &dc.references, &z_refs);
        }
    }
    report.zenith_ms += t_cmp.elapsed().as_millis();

    println!(
        "  seed {seed}: {}/{} checks ({} chunks; vanilla {} ms, zenith {} ms)",
        report.passed, report.total, report.chunks_checked, report.vanilla_ms, report.zenith_ms
    );
    Ok(report)
}

/// Runs `cargo xtask parity structure-starts [--write]`.
pub fn structure_starts(version: &str, write: bool) -> anyhow::Result<bool> {
    let shared = load_shared(version)?;

    let only_seed: Option<i64> = std::env::var("ZENITH_STRUCTURES_SEED")
        .ok()
        .and_then(|s| s.parse().ok());
    let seeds: Vec<i64> = match only_seed {
        Some(s) => vec![s],
        None => SEEDS.to_vec(),
    };

    let reports: Vec<anyhow::Result<SeedReport>> = std::thread::scope(|scope| {
        let handles: Vec<_> = seeds
            .iter()
            .map(|&seed| {
                let shared = &shared;
                scope.spawn(move || run_seed(shared, version, seed))
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("seed thread panicked"))
            .collect()
    });

    let mut total = SeedReport::default();
    for r in reports {
        total.merge(r?);
    }

    println!("\nPer-structure breakdown:");
    println!(
        "  {:<42} {:>12} {:>14} {:>12}",
        "structure", "starts", "pieces", "references"
    );
    for (sid, s) in &total.by_type {
        println!(
            "  {:<42} {:>5}/{:<6} {:>6}/{:<7} {:>5}/{:<6}",
            sid,
            s.starts_passed,
            s.starts_total,
            s.pieces_passed,
            s.pieces_total,
            s.refs_passed,
            s.refs_total
        );
    }

    println!(
        "\nstructure-starts ({version}): {}/{} checks passed across {} chunks ({} structure types observed)",
        total.passed,
        total.total,
        total.chunks_checked,
        total.by_type.len()
    );
    for f in &total.failures {
        println!("  FAIL: {f}");
    }

    if write {
        let res = SuiteResult::new(
            "structure-starts",
            version,
            total.passed as u64,
            total.total as u64,
        );
        // `SuiteResult::write` takes the results directory and names the file after the suite.
        let dir = repo_root().join("progress/parity");
        res.write(&dir)?;
        println!("wrote {}", dir.join("structure-starts.json").display());
    }

    Ok(total.passed == total.total && total.total > 0)
}
