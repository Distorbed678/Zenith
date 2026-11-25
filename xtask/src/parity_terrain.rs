//! M7 stage suites against `TerrainProbe` (format: `docs/specs/parity/terrain-probe.md`, `ZTP1`).
//!
//! * `worldgen-router`: every noise-router field and aquifer-config function at fixed points (uncached point path),
//!   plus whole-chunk volumes of final density and the six climate channels (caching volume path).
//! * `worldgen-biomes`: quantized climate target points, the chosen biome per quart position for every built-in
//!   biome source plus hand-made fixed/checkerboard sources, the obfuscated zoom seed and zoomed block biomes.
//! * `worldgen-columns`: base heights (4 heightmaps) and full vertical base column block-state IDs across all 7
//!   noise settings (exercising `NoiseChunk`, `Aquifer`, global fluid picker, and block-tag heightmap predicates).
//!
//! The corpus (seeds and positions) is generated here from fixed constants; nothing of the game's is stored.

use std::fs;
use std::io::Read;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::Context;
use zenith_biome::{
    BiomeSource, ClimateSampler, PointClimate, SearchState, obfuscate_seed, zoomed_biome,
};
use zenith_codec::json::{Json, ParseMode, parse_bytes};
use zenith_codec::log::null_log;
use zenith_codec::resources::ZipPackResources;
use zenith_parity::SuiteResult;
use zenith_registry::{BlockStateTable, Identifier};
use zenith_worldgen::{
    BlockTags, DensityVolume, HeightmapKind, NoiseGenerator, RandomState, RouterField,
    TerrainGenerator, WorldgenData,
};

use crate::parity::vanilla_command;
use crate::repo_root;

// ── ZTP1 encoding ────────────────────────────────────────────────────────────

#[derive(Default)]
pub(crate) struct In(Vec<u8>);

impl In {
    pub(crate) fn header(seed: i64, selector: &str) -> Self {
        let mut i = In::default();
        i.i64(seed);
        i.str(selector);
        i
    }
    pub(crate) fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    pub(crate) fn u32(&mut self, v: usize) {
        self.0
            .extend_from_slice(&u32::try_from(v).expect("count fits").to_be_bytes());
    }
    pub(crate) fn i64(&mut self, v: i64) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    pub(crate) fn str(&mut self, s: &str) {
        self.0
            .extend_from_slice(&u16::try_from(s.len()).expect("short string").to_be_bytes());
        self.0.extend_from_slice(s.as_bytes());
    }
    pub(crate) fn bytes(&mut self, b: &[u8]) {
        self.u32(b.len());
        self.0.extend_from_slice(b);
    }
}

pub(crate) struct Out<'a> {
    raw: &'a [u8],
    at: usize,
}

impl<'a> Out<'a> {
    pub(crate) fn new(raw: &'a [u8], mode_id: u8) -> anyhow::Result<Self> {
        anyhow::ensure!(raw.len() >= 5 && &raw[..4] == b"ZTP1", "not a ZTP1 file");
        anyhow::ensure!(raw[4] == mode_id, "mode {} instead of {mode_id}", raw[4]);
        Ok(Out { raw, at: 5 })
    }
    fn take(&mut self, n: usize) -> anyhow::Result<&'a [u8]> {
        let b = self
            .raw
            .get(self.at..self.at + n)
            .context("truncated ZTP1 output")?;
        self.at += n;
        Ok(b)
    }
    pub(crate) fn u8(&mut self) -> anyhow::Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub(crate) fn i32(&mut self) -> anyhow::Result<i32> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into()?))
    }
    pub(crate) fn u32(&mut self) -> anyhow::Result<usize> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into()?) as usize)
    }
    pub(crate) fn i64(&mut self) -> anyhow::Result<i64> {
        Ok(i64::from_be_bytes(self.take(8)?.try_into()?))
    }
    /// Raw f32 bits (compared as bits, so -0.0 and NaN payloads count).
    pub(crate) fn f32bits(&mut self) -> anyhow::Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into()?))
    }
    pub(crate) fn str(&mut self) -> anyhow::Result<String> {
        let n = u16::from_be_bytes(self.take(2)?.try_into()?) as usize;
        Ok(String::from_utf8(self.take(n)?.to_vec())?)
    }
    pub(crate) fn finish(&self) -> anyhow::Result<()> {
        anyhow::ensure!(self.at == self.raw.len(), "trailing bytes in ZTP1 output");
        Ok(())
    }
}

static RUN: AtomicUsize = AtomicUsize::new(0);

/// Runs `TerrainProbe` in `mode` on one input and returns the raw output.
pub(crate) fn run_probe(version: &str, mode: &str, input: &In) -> anyhow::Result<Vec<u8>> {
    // Build the oracle once per process: parallel jobs must not rebuild the same jar concurrently.
    static TOOL: std::sync::OnceLock<Result<(std::path::PathBuf, std::ffi::OsString), String>> =
        std::sync::OnceLock::new();
    let (java, classpath) = TOOL
        .get_or_init(|| {
            (|| -> anyhow::Result<_> {
                let (install, java) = crate::oracle::install_and_java(version)?;
                let agent = crate::oracle::build(version)?;
                let mut cp = zenith_install::libraries::game_classpath(&install)?;
                cp.push(agent);
                Ok((java.java, zenith_install::libraries::join_classpath(&cp)))
            })()
            .map_err(|e| format!("{e:#}"))
        })
        .as_ref()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let dir = zenith_install::cache_dir().join("parity/terrain");
    fs::create_dir_all(&dir)?;
    // Unique names: probes run in parallel.
    let n = RUN.fetch_add(1, Ordering::Relaxed);
    let (corpus, results) = (
        dir.join(format!("{mode}-{}-{n}.in", std::process::id())),
        dir.join(format!("{mode}-{}-{n}.out", std::process::id())),
    );
    fs::write(&corpus, &input.0)?;
    let out = vanilla_command(java)?
        .arg("-Xmx2G")
        .arg("-cp")
        .arg(classpath)
        .arg("dev.zenith.oracle.TerrainProbe")
        .arg(&corpus)
        .arg(&results)
        .arg(mode)
        .output()
        .context("run vanilla terrain probe")?;
    anyhow::ensure!(
        out.status.success(),
        "vanilla terrain probe ({mode}) failed:\n{}{}",
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .rev()
            .take(20)
            .collect::<Vec<_>>()
            .join("\n"),
        String::from_utf8_lossy(&out.stderr)
    );
    let raw = fs::read(&results)?;
    let _ = fs::remove_file(&corpus);
    let _ = fs::remove_file(&results);
    Ok(raw)
}

// ── corpus ───────────────────────────────────────────────────────────────────

/// Seeds: zero, small, negative, large, and the extremes.
pub(crate) const SEEDS: &[i64] = &[
    0,
    1,
    -1,
    12345,
    1_234_567_890_123_456_789,
    i64::MIN,
    i64::MAX,
];

/// Built-in noise settings of 26.3 and their vertical ranges (min_y, height) for choosing points.
pub(crate) const SETTINGS: &[(&str, i32, i32)] = &[
    ("minecraft:overworld", -64, 384),
    ("minecraft:large_biomes", -64, 384),
    ("minecraft:amplified", -64, 384),
    ("minecraft:nether", 0, 128),
    ("minecraft:end", 0, 128),
    ("minecraft:caves", -64, 192),
    ("minecraft:floating_islands", 0, 256),
];

/// A small deterministic generator for corpus positions (our own; not a game algorithm).
pub(crate) struct Lcg(u64);

impl Lcg {
    pub(crate) fn new(seed: u64) -> Self {
        Lcg(seed ^ 0x9E37_79B9_7F4A_7C15)
    }
    pub(crate) fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 11
    }
    /// Uniform in `lo..hi`.
    pub(crate) fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % (hi - lo) as u64) as i64
    }
}

/// Block positions: fixed edge cases (origin, cell and chunk boundaries, negatives, far coordinates) then
/// pseudo-random positions in three distance bands.
pub(crate) fn block_points(
    min_y: i32,
    height: i32,
    n_random: usize,
    salt: u64,
) -> Vec<(i32, i32, i32)> {
    let max_y = min_y + height - 1;
    let mut p = vec![
        (0, 0, 0),
        (0, min_y, 0),
        (0, max_y, 0),
        (0, 63, 0),
        (-1, 64, -1),
        (7, 64, 8),
        (8, 64, 7),
        (15, 64, 16),
        (16, 64, 15),
        (-16, 40, -17),
        (1000, 70, -1000),
        (-29_999_984, 64, 29_999_984),
        (16_777_216, 64, -16_777_216),
        (16_777_215, 64, 16_777_217),
    ];
    let mut r = Lcg::new(salt);
    for i in 0..n_random {
        let span = [2_000, 100_000, 10_000_000][i % 3];
        p.push((
            r.range(-span, span) as i32,
            r.range(i64::from(min_y), i64::from(max_y) + 1) as i32,
            r.range(-span, span) as i32,
        ));
    }
    p
}

// ── tallies and parallel jobs ────────────────────────────────────────────────

#[derive(Default)]
struct Tally {
    passed: u64,
    total: u64,
    failures: Vec<String>,
}

impl Tally {
    fn check(&mut self, ok: bool, what: impl FnOnce() -> String) {
        self.total += 1;
        if ok {
            self.passed += 1;
        } else if self.failures.len() < 8 {
            self.failures.push(what());
        }
    }
    fn merge(&mut self, other: Tally) {
        self.passed += other.passed;
        self.total += other.total;
        self.failures.extend(other.failures);
    }
}

type Job<'a> = Box<dyn FnOnce() -> anyhow::Result<Tally> + Send + 'a>;

/// Runs jobs on a few threads (each job starts one game process).
fn run_jobs(jobs: Vec<Job<'_>>) -> anyhow::Result<Tally> {
    let workers = std::thread::available_parallelism()
        .map_or(2, |n| n.get() / 2)
        .clamp(1, 6);
    let queue = std::sync::Mutex::new(jobs.into_iter().collect::<Vec<_>>());
    let results = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let Some(job) = queue.lock().expect("queue").pop() else {
                        break;
                    };
                    let r = job();
                    results.lock().expect("results").push(r);
                }
            });
        }
    });
    let mut tally = Tally::default();
    for r in results.into_inner().expect("results") {
        tally.merge(r?);
    }
    Ok(tally)
}

struct Vanilla {
    data: WorldgenData,
    jar: std::path::PathBuf,
}

fn load_vanilla(version: &str) -> anyhow::Result<Vanilla> {
    let install = zenith_install::discover_installs(Some(version))
        .into_iter()
        .next()
        .context("no install")?;
    let pack = ZipPackResources::open("vanilla", &install.client_jar, null_log());
    let data = WorldgenData::load(&pack).map_err(|e| anyhow::anyhow!("load worldgen data: {e}"))?;
    Ok(Vanilla {
        data,
        jar: install.client_jar,
    })
}

fn random_state(v: &Vanilla, settings: &str, seed: i64) -> anyhow::Result<RandomState> {
    let id = Identifier::parse(settings).map_err(|e| anyhow::anyhow!("{e}"))?;
    RandomState::new(&v.data, &id, seed)
        .map_err(|e| anyhow::anyhow!("random state {settings}: {e}"))
}

fn report(
    suite: &str,
    version: &str,
    detail: &str,
    t: &Tally,
    write: bool,
) -> anyhow::Result<bool> {
    println!(
        "{suite} {version}: {}/{} identical ({detail})",
        t.passed, t.total
    );
    for f in &t.failures {
        println!("  FAIL {f}");
    }
    if write {
        SuiteResult::new(suite, version, t.passed, t.total)
            .write(&repo_root().join("progress/parity"))?;
    }
    Ok(t.passed == t.total)
}

fn first_diff(a: &[u32], b: &[u32]) -> String {
    match a.iter().zip(b).position(|(x, y)| x != y) {
        Some(i) => format!(
            "first difference at index {i}: vanilla {} | zenith {} (of {} / {})",
            f32::from_bits(a[i]),
            f32::from_bits(b[i]),
            a.len(),
            b.len()
        ),
        None => format!("lengths {} / {}", a.len(), b.len()),
    }
}

// ── worldgen-router ──────────────────────────────────────────────────────────

const ROUTER_SEEDS: usize = 3;

fn router_job(
    v: &Vanilla,
    version: &str,
    seed: i64,
    settings: &str,
    min_y: i32,
    height: i32,
) -> anyhow::Result<Tally> {
    let mut t = Tally::default();
    let salt = (seed as u64) ^ u64::from(settings.len() as u32) << 32 ^ min_y as u64;
    let pts = block_points(min_y, height, 120, salt);
    let mut input = In::header(seed, settings);
    input.u32(pts.len());
    for &(x, y, z) in &pts {
        input.i32(x);
        input.i32(y);
        input.i32(z);
    }
    let raw = run_probe(version, "router", &input)?;
    let mut o = Out::new(&raw, 1)?;
    let has_aquifers = o.u8()? == 1;
    anyhow::ensure!(o.u32()? == pts.len(), "point count");
    let rs = random_state(v, settings, seed)?;
    t.check(has_aquifers == rs.settings().aquifers().is_some(), || {
        format!("{settings} seed {seed}: aquifer config present: vanilla {has_aquifers}")
    });
    for &(x, y, z) in &pts {
        for field in RouterField::ALL {
            let (van, zen) = (o.f32bits()?, rs.sample_router(field, x, y, z).to_bits());
            t.check(van == zen, || {
                format!(
                    "{settings} seed {seed} {field:?} at ({x},{y},{z}): vanilla {} | zenith {}",
                    f32::from_bits(van),
                    f32::from_bits(zen)
                )
            });
        }
        if has_aquifers {
            for k in 0..6 {
                let van = o.f32bits()?;
                let zen = rs.sample_aquifer(k, x, y, z).map(f32::to_bits);
                t.check(zen == Some(van), || {
                    format!(
                        "{settings} seed {seed} aquifer#{k} at ({x},{y},{z}): vanilla {} | zenith {:?}",
                        f32::from_bits(van),
                        zen.map(f32::from_bits)
                    )
                });
            }
        }
    }
    o.finish()?;
    Ok(t)
}

fn volume_job(v: &Vanilla, version: &str, seed: i64, settings: &str) -> anyhow::Result<Tally> {
    let mut t = Tally::default();
    let mut r = Lcg::new(seed as u64 ^ 0x5EED);
    let chunks = [
        (0, 0),
        (-1, -1),
        (
            r.range(-60_000, 60_000) as i32,
            r.range(-60_000, 60_000) as i32,
        ),
    ];
    let mut input = In::header(seed, settings);
    input.u32(chunks.len());
    for &(cx, cz) in &chunks {
        input.i32(cx);
        input.i32(cz);
    }
    let raw = run_probe(version, "router-volume", &input)?;
    let mut o = Out::new(&raw, 2)?;
    let (min_y, height) = (o.i32()?, o.i32()?);
    let fs = (o.u32()?, o.u32()?, o.u32()?);
    let cs = (o.u32()?, o.u32()?, o.u32()?);
    anyhow::ensure!(o.u32()? == chunks.len(), "chunk count");
    let rs = random_state(v, settings, seed)?;
    let read = |o: &mut Out<'_>, n: usize| -> anyhow::Result<Vec<u32>> {
        (0..n).map(|_| o.f32bits()).collect()
    };
    let climate = [
        RouterField::Temperature,
        RouterField::Vegetation,
        RouterField::Continents,
        RouterField::Erosion,
        RouterField::Depth,
        RouterField::Ridges,
    ];
    for &(cx, cz) in &chunks {
        let van = read(&mut o, fs.0 * fs.1 * fs.2)?;
        let zen: Vec<u32> = rs
            .sample_volume(RouterField::FinalDensity, cx, cz)
            .into_iter()
            .map(f32::to_bits)
            .collect();
        t.check(van == zen, || {
            format!(
                "{settings} seed {seed} chunk ({cx},{cz}) final density volume: {}",
                first_diff(&van, &zen)
            )
        });
        let volume = DensityVolume::new([cs.0, cs.1, cs.2], [cx * 16, min_y, cz * 16], [4, 4, 4])
            .context("climate volume")?;
        for field in climate {
            let van = read(&mut o, cs.0 * cs.1 * cs.2)?;
            let zen: Vec<u32> = rs
                .sample_volume_for(field, volume, true)
                .into_iter()
                .map(f32::to_bits)
                .collect();
            t.check(van == zen, || {
                format!(
                    "{settings} seed {seed} chunk ({cx},{cz}) {field:?} climate volume: {}",
                    first_diff(&van, &zen)
                )
            });
        }
    }
    let _ = height;
    o.finish()?;
    Ok(t)
}

/// `worldgen-router`: router fields + aquifer functions at points, and chunk volumes.
pub fn worldgen_router(version: &str, write: bool) -> anyhow::Result<bool> {
    let v = load_vanilla(version)?;
    let mut jobs: Vec<Job<'_>> = Vec::new();
    for &(settings, min_y, height) in SETTINGS {
        for &seed in &SEEDS[..ROUTER_SEEDS] {
            let v = &v;
            jobs.push(Box::new(move || {
                router_job(v, version, seed, settings, min_y, height)
            }));
        }
        for &seed in &SEEDS[3..5] {
            let v = &v;
            jobs.push(Box::new(move || volume_job(v, version, seed, settings)));
        }
    }
    let t = run_jobs(jobs)?;
    report(
        "worldgen-router",
        version,
        "8 router fields + 6 aquifer functions per point, final-density and climate chunk volumes; 7 noise settings",
        &t,
        write,
    )
}

// ── worldgen-biomes ──────────────────────────────────────────────────────────

fn quart_points(min_y: i32, height: i32, salt: u64) -> Vec<(i32, i32, i32)> {
    let mut q: Vec<(i32, i32, i32)> = block_points(min_y, height, 300, salt)
        .into_iter()
        .map(|(x, y, z)| (x >> 2, y >> 2, z >> 2))
        .collect();
    q.extend([(0, 0, 0), (-1, -1, -1), (1, 100, -1)]);
    q
}

fn climate_job(
    v: &Vanilla,
    version: &str,
    seed: i64,
    settings: &str,
    min_y: i32,
    height: i32,
) -> anyhow::Result<Tally> {
    let mut t = Tally::default();
    let quarts = quart_points(min_y, height, seed as u64 ^ 0xC11A);
    let mut input = In::header(seed, settings);
    input.u32(quarts.len());
    for &(x, y, z) in &quarts {
        input.i32(x);
        input.i32(y);
        input.i32(z);
    }
    let raw = run_probe(version, "climate", &input)?;
    let mut o = Out::new(&raw, 3)?;
    anyhow::ensure!(o.u32()? == quarts.len(), "quart count");
    let rs = random_state(v, settings, seed)?;
    let sampler = PointClimate::new(|x, y, z| rs.climate_point(x, y, z));
    for &(qx, qy, qz) in &quarts {
        let van = [o.i64()?, o.i64()?, o.i64()?, o.i64()?, o.i64()?, o.i64()?];
        let p = sampler.sample(qx, qy, qz);
        let zen = [
            p.temperature,
            p.humidity,
            p.continentalness,
            p.erosion,
            p.depth,
            p.weirdness,
        ];
        t.check(van == zen, || format!("{settings} seed {seed} climate at quart ({qx},{qy},{qz}): vanilla {van:?} | zenith {zen:?}"));
    }
    o.finish()?;
    Ok(t)
}

/// Hand-made biome sources (our own JSON), decoded by both sides. The multi-noise one has deliberate ties.
const JSON_SOURCES: &[&str] = &[
    r#"{"type":"minecraft:fixed","biome":"minecraft:cherry_grove"}"#,
    r#"{"type":"minecraft:checkerboard","biomes":["minecraft:plains","minecraft:desert","minecraft:badlands"],"scale":2}"#,
    r#"{"type":"minecraft:checkerboard","biomes":["minecraft:plains","minecraft:snowy_plains"],"scale":0}"#,
    r#"{"type":"minecraft:multi_noise","biomes":[
        {"biome":"minecraft:plains","parameters":{"temperature":0,"humidity":0,"continentalness":0,"erosion":0,"weirdness":0,"depth":0,"offset":0}},
        {"biome":"minecraft:desert","parameters":{"temperature":0,"humidity":0,"continentalness":0,"erosion":0,"weirdness":0,"depth":0,"offset":0}},
        {"biome":"minecraft:forest","parameters":{"temperature":[-1,0],"humidity":[0,1],"continentalness":0,"erosion":0,"weirdness":0,"depth":0,"offset":0}},
        {"biome":"minecraft:taiga","parameters":{"temperature":[0,1],"humidity":[-1,0],"continentalness":0,"erosion":0,"weirdness":0,"depth":0,"offset":0}},
        {"biome":"minecraft:swamp","parameters":{"temperature":[-0.5,0.5],"humidity":[-0.5,0.5],"continentalness":[-1,1],"erosion":0,"weirdness":0,"depth":0,"offset":0.2}}]}"#,
];

/// (selector, world preset file, dimension key) for the built-in sources.
const PRESET_SOURCES: &[(&str, &str, &str)] = &[
    ("overworld", "normal", "minecraft:overworld"),
    ("large_biomes", "large_biomes", "minecraft:overworld"),
    ("amplified", "amplified", "minecraft:overworld"),
    ("nether", "normal", "minecraft:the_nether"),
    ("end", "normal", "minecraft:the_end"),
];

/// The biome source JSON, noise settings id, and dimension type id of a built-in dimension,
/// read from the user's jar.
fn preset_dimension(
    jar: &std::path::Path,
    preset: &str,
    dimension: &str,
) -> anyhow::Result<(Json, String, String)> {
    let mut zip = zip::ZipArchive::new(fs::File::open(jar)?)?;
    let mut b = Vec::new();
    zip.by_name(&format!(
        "data/minecraft/worldgen/world_preset/{preset}.json"
    ))?
    .read_to_end(&mut b)?;
    let j = parse_bytes(&b, ParseMode::Data).context("parse world preset")?;
    let generator = (|| {
        let Json::Object(root) = &j else { return None };
        let Json::Object(dims) = root.get("dimensions")? else {
            return None;
        };
        let Json::Object(dim) = dims.get(dimension)? else {
            return None;
        };
        let dim_type = dim.get("type")?.as_str()?.to_owned();
        let Json::Object(g) = dim.get("generator")? else {
            return None;
        };
        Some((
            g.get("biome_source")?.clone(),
            g.get("settings")?.as_str()?.to_owned(),
            dim_type,
        ))
    })();
    generator.with_context(|| format!("world preset {preset}: no generator for {dimension}"))
}

/// The biome source JSON and noise settings id of a built-in dimension, read from the user's jar.
fn preset_source(
    jar: &std::path::Path,
    preset: &str,
    dimension: &str,
) -> anyhow::Result<(Json, String)> {
    let (bs, settings, _) = preset_dimension(jar, preset, dimension)?;
    Ok((bs, settings))
}

fn biome_job(
    v: &Vanilla,
    version: &str,
    seed: i64,
    selector: &str,
    source_json: Option<&str>,
) -> anyhow::Result<Tally> {
    let mut t = Tally::default();
    let (source, settings) = match source_json {
        Some(text) => (
            parse_bytes(text.as_bytes(), ParseMode::Data).context("parse source json")?,
            "minecraft:overworld".to_owned(),
        ),
        None => {
            let &(_, preset, dim) = PRESET_SOURCES
                .iter()
                .find(|p| p.0 == selector)
                .context("selector")?;
            preset_source(&v.jar, preset, dim)?
        }
    };
    let (min_y, height) = SETTINGS
        .iter()
        .find(|s| s.0 == settings)
        .map_or((-64, 384), |s| (s.1, s.2));
    let quarts = quart_points(min_y, height, seed as u64 ^ 0xB10E);
    let blocks = block_points(min_y, height, 300, seed as u64 ^ 0x200E);
    let mut input = In::header(seed, selector);
    if let Some(text) = source_json {
        input.bytes(text.as_bytes());
    }
    input.u32(quarts.len());
    for &(x, y, z) in &quarts {
        input.i32(x);
        input.i32(y);
        input.i32(z);
    }
    input.u32(blocks.len());
    for &(x, y, z) in &blocks {
        input.i32(x);
        input.i32(y);
        input.i32(z);
    }
    let raw = run_probe(version, "biome", &input)?;
    let mut o = Out::new(&raw, 4)?;
    let label = source_json.map_or(selector.to_owned(), |s| s.chars().take(48).collect());

    let source = match BiomeSource::decode_json(&source) {
        zenith_codec::codec::DataResult::Success(s) => s,
        other => anyhow::bail!("{label}: zenith rejects the biome source: {other:?}"),
    };
    let rs = random_state(v, &settings, seed)?;
    let sampler = PointClimate::new(|x, y, z| rs.climate_point(x, y, z));
    let mut state = SearchState::new();

    let obf = o.i64()?;
    t.check(obf == obfuscate_seed(seed), || {
        format!(
            "seed {seed}: obfuscated seed vanilla {obf} | zenith {}",
            obfuscate_seed(seed)
        )
    });
    anyhow::ensure!(o.u32()? == quarts.len(), "quart count");
    for &(qx, qy, qz) in &quarts {
        let van = o.str()?;
        let zen = source
            .noise_biome(qx, qy, qz, &sampler, &mut state)
            .to_string();
        t.check(van == zen, || {
            format!("{label} seed {seed} quart ({qx},{qy},{qz}): vanilla {van} | zenith {zen}")
        });
    }
    anyhow::ensure!(o.u32()? == blocks.len(), "block count");
    for &(bx, by, bz) in &blocks {
        let van = o.str()?;
        let zen = zoomed_biome(obf, bx, by, bz, &mut |qx, qy, qz| {
            source.noise_biome(qx, qy, qz, &sampler, &mut state)
        })
        .to_string();
        t.check(van == zen, || {
            format!(
                "{label} seed {seed} zoomed block ({bx},{by},{bz}): vanilla {van} | zenith {zen}"
            )
        });
    }
    o.finish()?;
    Ok(t)
}

/// `worldgen-biomes`: climate target points, biomes per quart for every source, seed obfuscation, zoom.
pub fn worldgen_biomes(version: &str, write: bool) -> anyhow::Result<bool> {
    let v = load_vanilla(version)?;
    let mut jobs: Vec<Job<'_>> = Vec::new();
    for &(settings, min_y, height) in SETTINGS {
        for &seed in &SEEDS[..ROUTER_SEEDS] {
            let v = &v;
            jobs.push(Box::new(move || {
                climate_job(v, version, seed, settings, min_y, height)
            }));
        }
    }
    for &seed in &SEEDS[..4] {
        for &(selector, _, _) in PRESET_SOURCES {
            let v = &v;
            jobs.push(Box::new(move || {
                biome_job(v, version, seed, selector, None)
            }));
        }
        for &json in JSON_SOURCES {
            let v = &v;
            jobs.push(Box::new(move || {
                biome_job(v, version, seed, "json", Some(json))
            }));
        }
    }
    let t = run_jobs(jobs)?;
    report(
        "worldgen-biomes",
        version,
        "quantized climate per quart (7 settings), biomes per quart + zoomed per block for 5 built-in and 4 hand-made sources",
        &t,
        write,
    )
}

// ── worldgen-columns ─────────────────────────────────────────────────────────

struct ColumnContext {
    data: WorldgenData,
    blocks: BlockStateTable,
    tags: BlockTags,
    jar: std::path::PathBuf,
}

fn load_column_context(version: &str) -> anyhow::Result<ColumnContext> {
    let install = zenith_install::discover_installs(Some(version))
        .into_iter()
        .next()
        .context("no install")?;
    let tables = zenith_data::Tables::load(&zenith_data::version_dir(version).join("tables.bin"))
        .context("no cached tables; run `cargo run -p zenith-data -- ingest` first")?;
    let blocks = BlockStateTable::from_tables(&tables)
        .map_err(|e| anyhow::anyhow!("build block-state table: {e}"))?;
    let pack = ZipPackResources::open("vanilla", &install.client_jar, null_log());
    let data = WorldgenData::load(&pack).map_err(|e| anyhow::anyhow!("load worldgen data: {e}"))?;
    let tags =
        BlockTags::load(&pack, &blocks).map_err(|e| anyhow::anyhow!("load block tags: {e}"))?;
    Ok(ColumnContext {
        data,
        blocks,
        tags,
        jar: install.client_jar,
    })
}

fn column_points(min_y: i32, height: i32, n_random: usize, salt: u64) -> Vec<(i32, i32)> {
    let mut cols = Vec::with_capacity(n_random + 16);
    for (x, _, z) in block_points(min_y, height, n_random, salt) {
        if !cols.contains(&(x, z)) {
            cols.push((x, z));
        }
    }
    cols
}

fn describe_state(blocks: &BlockStateTable, id: u32) -> String {
    match blocks.state_by_id_exact(id as i32) {
        Some(s) if s.properties.is_empty() => format!("{id} ({})", s.block_id),
        Some(s) => {
            let props: Vec<String> = s
                .properties
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect();
            format!("{id} ({}[{}])", s.block_id, props.join(","))
        }
        None => format!("{id} (<unknown>)"),
    }
}

fn column_job(
    ctx: &ColumnContext,
    version: &str,
    seed: i64,
    settings: &str,
    min_y: i32,
    height: i32,
) -> anyhow::Result<Tally> {
    let mut t = Tally::default();
    let salt = (seed as u64) ^ (settings.len() as u64) << 32 ^ (min_y as u64) ^ 0xC01A_0001;
    let cols = column_points(min_y, height, 50, salt);
    let mut input = In::header(seed, settings);
    input.u32(cols.len());
    for &(x, z) in &cols {
        input.i32(x);
        input.i32(z);
    }
    let raw = run_probe(version, "column", &input)?;
    let mut o = Out::new(&raw, 5)?;
    let probe_min_y = o.i32()?;
    let probe_height = o.i32()?;
    let heightmap_count = o.u8()?;
    anyhow::ensure!(
        heightmap_count == 4,
        "expected 4 heightmaps, got {heightmap_count}"
    );
    anyhow::ensure!(o.u32()? == cols.len(), "column count");

    let settings_id = Identifier::parse(settings).map_err(|e| anyhow::anyhow!("{e}"))?;
    let generator = NoiseGenerator::new(&ctx.data, &settings_id, seed, &ctx.blocks, &ctx.tags)
        .map_err(|e| anyhow::anyhow!("noise generator {settings}: {e}"))?;
    anyhow::ensure!(
        probe_min_y == generator.settings().min_y()
            && probe_height == generator.settings().height(),
        "{settings}: vertical range mismatch: vanilla ({probe_min_y}, {probe_height}) | zenith ({}, {})",
        generator.settings().min_y(),
        generator.settings().height()
    );

    let h = probe_height as usize;
    for &(x, z) in &cols {
        let van_heights = [o.i32()?, o.i32()?, o.i32()?, o.i32()?];
        let mut van_blocks = Vec::with_capacity(h);
        for _ in 0..h {
            van_blocks.push(o.i32()? as u32);
        }

        let zen_heights = [
            generator.base_height(x, z, HeightmapKind::OceanFloorWg),
            generator.base_height(x, z, HeightmapKind::WorldSurfaceWg),
            generator.base_height(x, z, HeightmapKind::MotionBlocking),
            generator.base_height(x, z, HeightmapKind::MotionBlockingNoLeaves),
        ];
        t.check(van_heights == zen_heights, || {
            format!(
                "{settings} seed {seed} col ({x},{z}) heights [OF_WG, WS_WG, MB, MB_NL]: vanilla {van_heights:?} | zenith {zen_heights:?}"
            )
        });

        let zen_blocks = generator.base_column(x, z);
        t.check(van_blocks == zen_blocks, || {
            match van_blocks.iter().zip(&zen_blocks).position(|(a, b)| a != b) {
                Some(idx) => {
                    let y = probe_min_y + (idx as i32);
                    format!(
                        "{settings} seed {seed} col ({x},{z}) first diff at y={y}: vanilla {} | zenith {}",
                        describe_state(&ctx.blocks, van_blocks[idx]),
                        describe_state(&ctx.blocks, zen_blocks[idx]),
                    )
                }
                None => format!(
                    "{settings} seed {seed} col ({x},{z}) length mismatch: vanilla {} | zenith {}",
                    van_blocks.len(),
                    zen_blocks.len()
                ),
            }
        });
    }
    o.finish()?;
    Ok(t)
}

/// `worldgen-columns`: base heights (4 heightmaps) and base column block-state IDs across all 7 settings.
pub fn worldgen_columns(version: &str, write: bool) -> anyhow::Result<bool> {
    let ctx = load_column_context(version)?;
    let mut jobs: Vec<Job<'_>> = Vec::new();
    for &(settings, min_y, height) in SETTINGS {
        for &seed in &SEEDS[..ROUTER_SEEDS] {
            let ctx = &ctx;
            jobs.push(Box::new(move || {
                column_job(ctx, version, seed, settings, min_y, height)
            }));
        }
    }
    let t = run_jobs(jobs)?;
    report(
        "worldgen-columns",
        version,
        "4 base heights + full base column block-state IDs; 7 noise settings × 3 seeds × ~60 columns",
        &t,
        write,
    )
}

// ── golden-terrain ───────────────────────────────────────────────────────────

const GOLDEN_SEEDS: &[i64] = &[12345, -1, 1_234_567_890_123_456_789];

fn golden_chunks_for(preset: &str) -> Vec<(&'static str, Vec<(i32, i32)>)> {
    match preset {
        "normal" => vec![
            (
                "minecraft:overworld",
                vec![
                    (200, 200),
                    (201, 200),
                    (200, 201),
                    (201, 201),
                    (-300, 450),
                    (512, -256),
                    (10_000, -10_000),
                    (-10_003, 9_997),
                ],
            ),
            (
                "minecraft:the_nether",
                vec![
                    (80, 80),
                    (81, 80),
                    (80, 81),
                    (81, 81),
                    (-120, 150),
                    (250, -180),
                    (10_000, 10_000),
                    (-9_995, -10_002),
                ],
            ),
            (
                "minecraft:the_end",
                vec![
                    (100, 100),
                    (101, 100),
                    (100, 101),
                    (101, 101),
                    (30, -30),
                    (-80, 70),
                    (10_000, -9_999),
                    (-10_001, 10_002),
                ],
            ),
        ],
        "large_biomes" => vec![(
            "minecraft:overworld",
            vec![
                (220, -220),
                (221, -220),
                (220, -219),
                (221, -219),
                (-400, -350),
                (600, 300),
                (10_005, 9_995),
                (-10_008, -9_992),
            ],
        )],
        "amplified" => vec![(
            "minecraft:overworld",
            vec![
                (-210, 210),
                (-209, 210),
                (-210, 211),
                (-209, 211),
                (350, -400),
                (-500, -500),
                (9_998, -10_004),
                (-10_002, 10_006),
            ],
        )],
        _ => Vec::new(),
    }
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

/// Finds deterministic chunks (>= 25 chunks from origin) hitting special surface passes
/// (`eroded_badlands` pillars/bands, `frozen_ocean`/`deep_frozen_ocean` icebergs,
/// `sulfur_caves` 3D noise threshold, and `jagged_peaks`/`snowy_slopes`/`frozen_peaks` steep+temp).
fn find_special_overworld_chunks(generator: &TerrainGenerator) -> Vec<(i32, i32)> {
    let rs = generator.noise_generator().random_state();
    let bs = generator.biome_source();
    let sampler = PointClimate::new(|bx, by, bz| rs.climate_point(bx, by, bz));
    let mut state = SearchState::new();

    let mut found: [Option<(i32, i32)>; 4] = [None; 4];
    for step in 0..4000i32 {
        let cx = 25 + (step % 80) * 16;
        let cz = 25 + (step / 80) * 16;
        let qx = (cx << 2) + 2;
        let qz = (cz << 2) + 2;
        if found[0].is_none() || found[1].is_none() || found[3].is_none() {
            let b = bs.noise_biome(qx, 24, qz, &sampler, &mut state);
            match b.path() {
                "eroded_badlands" if found[0].is_none() => found[0] = Some((cx, cz)),
                "frozen_ocean" | "deep_frozen_ocean" if found[1].is_none() => {
                    found[1] = Some((cx, cz));
                }
                "jagged_peaks" | "frozen_peaks" | "snowy_slopes" if found[3].is_none() => {
                    found[3] = Some((cx, cz));
                }
                _ => {}
            }
        }
        if found[2].is_none() {
            let b = bs.noise_biome(qx, 0, qz, &sampler, &mut state);
            if b.path() == "sulfur_caves" {
                found[2] = Some((cx, cz));
            }
        }
        if found.iter().all(Option::is_some) {
            break;
        }
    }
    found.into_iter().flatten().collect()
}

struct GoldenTiming {
    vanilla_ms: u128,
    zenith_ms: u128,
    chunks: usize,
    overworld_zenith_ms: u128,
    overworld_chunks: usize,
}

fn golden_terrain_job(
    ctx: &ColumnContext,
    version: &str,
    seed: i64,
    preset: &str,
    timing: &std::sync::Mutex<GoldenTiming>,
) -> anyhow::Result<Tally> {
    use crate::oracle::{ChunkRequest, ChunkWorld, dump_chunks, parse_chunk_dump};

    let mut t = Tally::default();
    let mut dims = golden_chunks_for(preset);

    // Build one TerrainGenerator per dimension in this world first so we can also pick
    // special-surface-pass Overworld chunks (eroded badlands, frozen ocean, sulfur caves, peaks).
    let mut generators: std::collections::HashMap<&str, TerrainGenerator> =
        std::collections::HashMap::new();
    for &(dim, _) in &dims {
        let (source_json, settings_str, dim_type_str) = preset_dimension(&ctx.jar, preset, dim)?;
        let biome_source = match BiomeSource::decode_json(&source_json) {
            zenith_codec::codec::DataResult::Success(s) => s,
            other => anyhow::bail!("{preset}/{dim}: failed to decode biome_source: {other:?}"),
        };
        let settings_id = Identifier::parse(&settings_str).map_err(|e| anyhow::anyhow!("{e}"))?;
        let dim_type_id = Identifier::parse(&dim_type_str).map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut generator = TerrainGenerator::new(
            &ctx.data,
            biome_source,
            &settings_id,
            seed,
            &ctx.blocks,
            &ctx.tags,
        )
        .map_err(|e| anyhow::anyhow!("{preset}/{dim} seed {seed}: {e}"))?;
        if let Some(dh) = ctx.data.dimension_height(&dim_type_id) {
            generator = generator.with_chunk_height(dh.min_y, dh.height);
        }
        generators.insert(dim, generator);
    }

    if preset == "normal"
        && let Some(ow_gen) = generators.get("minecraft:overworld")
    {
        let extra = find_special_overworld_chunks(ow_gen);
        if let Some((_, coords)) = dims.iter_mut().find(|(d, _)| *d == "minecraft:overworld") {
            for c in extra {
                if !coords.contains(&c) {
                    coords.push(c);
                }
            }
        }
    }

    // Scale knob: ZENITH_GOLDEN_RANDOM extra random chunks per dimension (far from the origin, up to ±20,000).
    let extra: usize = std::env::var("ZENITH_GOLDEN_RANDOM")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    for (dim, coords) in dims.iter_mut() {
        let mut r =
            Lcg::new((seed as u64) ^ (preset.len() as u64) << 40 ^ (dim.len() as u64) << 48);
        let target = coords.len() + extra;
        while coords.len() < target {
            let c = (
                r.range(-20_000, 20_000) as i32,
                r.range(-20_000, 20_000) as i32,
            );
            if (c.0.abs() > 32 || c.1.abs() > 32) && !coords.contains(&c) {
                coords.push(c);
            }
        }
    }

    let mut requests = Vec::new();
    for (dim, coords) in &dims {
        for &(cx, cz) in coords {
            requests.push(ChunkRequest {
                dimension: (*dim).to_owned(),
                x: cx,
                z: cz,
                status: "terrain".to_owned(),
            });
        }
    }

    let world = ChunkWorld {
        seed,
        level_type: format!("minecraft:{preset}"),
        structures: false,
    };
    let van_start = std::time::Instant::now();
    let raw = dump_chunks(version, &world, &requests)?;
    let van_ms = van_start.elapsed().as_millis();
    let dumped = parse_chunk_dump(&raw)?;
    anyhow::ensure!(
        dumped.len() == requests.len(),
        "dumped chunk count {} != requested {}",
        dumped.len(),
        requests.len()
    );

    let zen_start = std::time::Instant::now();
    let mut ow_ms = 0u128;
    let mut ow_count = 0usize;
    let mut zen_chunks = Vec::with_capacity(dumped.len());
    for dc in &dumped {
        let generator = generators
            .get(dc.dimension.as_str())
            .with_context(|| format!("missing generator for {}", dc.dimension))?;
        let t0 = std::time::Instant::now();
        let chunk = generator.generate_chunk(dc.x, dc.z);
        if dc.dimension == "minecraft:overworld" {
            ow_ms += t0.elapsed().as_micros();
            ow_count += 1;
        }
        zen_chunks.push(chunk);
    }
    let zen_ms = zen_start.elapsed().as_millis();
    {
        let mut guard = timing.lock().expect("timing");
        guard.vanilla_ms += van_ms;
        guard.zenith_ms += zen_ms;
        guard.chunks += dumped.len();
        guard.overworld_zenith_ms += ow_ms;
        guard.overworld_chunks += ow_count;
    }

    for (dc, zc) in dumped.iter().zip(&zen_chunks) {
        let tag = format!("{preset}/{} seed {seed} ({},{})", dc.dimension, dc.x, dc.z);

        // 1. Blocks
        let blocks_ok = dc.min_y == zc.min_y()
            && dc.height == zc.height()
            && dc.blocks.as_slice() == zc.blocks();
        t.check(blocks_ok, || {
            if dc.min_y != zc.min_y() || dc.height != zc.height() {
                return format!(
                    "{tag} blocks: vertical range vanilla ({}, {}) | zenith ({}, {})",
                    dc.min_y,
                    dc.height,
                    zc.min_y(),
                    zc.height()
                );
            }
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
                    // Every differing block (first 16 listed) to tell isolated boundary cases from systematic ones.
                    let diffs: Vec<String> = dc
                        .blocks
                        .iter()
                        .zip(zc.blocks())
                        .enumerate()
                        .filter(|(_, (a, b))| a != b)
                        .map(|(i, (a, b))| {
                            let (sec, rem) = (i / 4096, i % 4096);
                            format!(
                                "({},{},{}) {}→{}",
                                (dc.x << 4) + (rem % 16) as i32,
                                dc.min_y + (sec * 16 + rem / 256) as i32,
                                (dc.z << 4) + ((rem % 256) / 16) as i32,
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
                        diffs.iter().take(16).cloned().collect::<Vec<_>>().join(", ")
                    )
                }
                None => format!(
                    "{tag} blocks: length vanilla {} | zenith {}",
                    dc.blocks.len(),
                    zc.blocks().len()
                ),
            }
        });

        // 2. Biomes
        let zen_biomes: Vec<String> = zc.biomes_iter().map(ToString::to_string).collect();
        t.check(dc.biomes == zen_biomes, || {
            match dc
                .biomes
                .iter()
                .zip(&zen_biomes)
                .position(|(a, b)| a != b)
            {
                Some(idx) => {
                    let sec = idx / 64;
                    let rem = idx % 64;
                    let qy = rem / 16;
                    let qz = (rem % 16) / 4;
                    let qx = rem % 4;
                    format!(
                        "{tag} biomes: first diff at sec={sec} q=({qx},{qy},{qz}): vanilla {} | zenith {}",
                        dc.biomes[idx], zen_biomes[idx]
                    )
                }
                None => format!(
                    "{tag} biomes: length vanilla {} | zenith {}",
                    dc.biomes.len(),
                    zen_biomes.len()
                ),
            }
        });

        // 3. Each heightmap (6 total at terrain status)
        for (hm_name, van_heights) in &dc.heightmaps {
            let kind = heightmap_kind_from_name(hm_name);
            let zen_hm = kind.and_then(|k| zc.heightmap(k));
            let ok = zen_hm.is_some_and(|h| h.heights().as_slice() == van_heights.as_slice());
            t.check(ok, || match zen_hm {
                Some(h) => {
                    match van_heights
                        .iter()
                        .zip(h.heights())
                        .position(|(a, b)| a != b)
                    {
                        Some(idx) => format!(
                            "{tag} heightmap {hm_name}: first diff at local ({},{}): vanilla {} | zenith {}",
                            idx % 16,
                            idx / 16,
                            van_heights[idx],
                            h.heights()[idx]
                        ),
                        None => format!("{tag} heightmap {hm_name}: length mismatch"),
                    }
                }
                None => format!("{tag} heightmap {hm_name}: missing on Zenith chunk"),
            });
        }

        // 4. Post-processing marks
        let zen_marks: Vec<(i32, u16)> = zc
            .post_processing_marks()
            .into_iter()
            .map(|m| (m.section, m.packed))
            .collect();
        t.check(dc.post_processing == zen_marks, || {
            match dc
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
            }
        });

        // 5. Scheduled tick counts
        let ticks_ok =
            dc.block_ticks == zc.block_tick_count() && dc.fluid_ticks == zc.fluid_tick_count();
        t.check(ticks_ok, || {
            format!(
                "{tag} ticks: vanilla ({}, {}) | zenith ({}, {})",
                dc.block_ticks,
                dc.fluid_ticks,
                zc.block_tick_count(),
                zc.fluid_tick_count()
            )
        });
    }

    Ok(t)
}

/// `golden-terrain`: full `terrain`-status chunks (blocks, stored biomes, all 6 heightmaps,
/// post-processing marks, tick counts) across 3 seeds × {normal (OW+Nether+End), large_biomes, amplified}.
pub fn golden_terrain(version: &str, write: bool) -> anyhow::Result<bool> {
    let ctx = load_column_context(version)?;
    let timing = std::sync::Mutex::new(GoldenTiming {
        vanilla_ms: 0,
        zenith_ms: 0,
        chunks: 0,
        overworld_zenith_ms: 0,
        overworld_chunks: 0,
    });
    // Scale knob: ZENITH_GOLDEN_SEEDS total seeds (the fixed ones first, then derived seeds).
    let n_seeds: usize = std::env::var("ZENITH_GOLDEN_SEEDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(GOLDEN_SEEDS.len());
    // Batching knob: ZENITH_GOLDEN_SEED_OFFSET skips that many seeds of the same fixed sequence, so a long soak can
    // run as several shorter batches (e.g. offset 0, 10, 20, ... with ZENITH_GOLDEN_SEEDS=10).
    let offset: usize = std::env::var("ZENITH_GOLDEN_SEED_OFFSET")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut all: Vec<i64> = GOLDEN_SEEDS.to_vec();
    let mut r = Lcg::new(0x601D_E7E5);
    while all.len() < offset + n_seeds {
        all.push(((r.next() << 11) ^ r.next()) as i64);
    }
    let seeds: Vec<i64> = all[offset..offset + n_seeds].to_vec();
    // One vanilla server per (seed, world type); `oracle::dump_chunks` is safe to call in parallel.
    let mut jobs: Vec<Job<'_>> = Vec::new();
    // Debug filter: ZENITH_GOLDEN_ONLY="<seed>:<preset>" runs a single world.
    let only = std::env::var("ZENITH_GOLDEN_ONLY").ok();
    for &seed in &seeds {
        for preset in ["normal", "large_biomes", "amplified"] {
            if only
                .as_ref()
                .is_some_and(|o| *o != format!("{seed}:{preset}"))
            {
                continue;
            }
            let (ctx, timing) = (&ctx, &timing);
            jobs.push(Box::new(move || {
                // Report each world as it finishes, so an interrupted soak still shows what it covered.
                let t = golden_terrain_job(ctx, version, seed, preset, timing)?;
                println!("  world seed {seed} {preset}: {}/{}", t.passed, t.total);
                Ok(t)
            }));
        }
    }
    let t = run_jobs(jobs)?;
    let tm = timing.into_inner().expect("timing");
    let zen_per_chunk = if tm.chunks > 0 {
        tm.zenith_ms as f64 / tm.chunks as f64
    } else {
        0.0
    };
    let ow_per_chunk = if tm.overworld_chunks > 0 {
        (tm.overworld_zenith_ms as f64 / 1000.0) / tm.overworld_chunks as f64
    } else {
        0.0
    };
    println!(
        "golden-terrain timing: {} chunks — zenith {} ms ({:.2} ms/chunk overall; {:.2} ms/chunk overworld) | vanilla server runs {} ms total",
        tm.chunks, tm.zenith_ms, zen_per_chunk, ow_per_chunk, tm.vanilla_ms
    );
    report(
        "golden-terrain",
        version,
        &format!(
            "blocks, stored biomes, 6 heightmaps, post-processing marks, ticks; {} seeds × (normal OW+Nether+End, large_biomes, amplified) × {} chunks",
            seeds.len(),
            8 + std::env::var("ZENITH_GOLDEN_RANDOM")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0)
        ),
        &t,
        write,
    )
}
