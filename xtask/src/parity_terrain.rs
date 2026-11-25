//! M7 stage suites against `TerrainProbe` (format: `docs/specs/parity/terrain-probe.md`, `ZTP1`).
//!
//! * `worldgen-router`: every noise-router field and aquifer-config function at fixed points (uncached point path),
//!   plus whole-chunk volumes of final density and the six climate channels (caching volume path).
//! * `worldgen-biomes`: quantized climate target points, the chosen biome per quart position for every built-in
//!   biome source plus hand-made fixed/checkerboard sources, the obfuscated zoom seed and zoomed block biomes.
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
use zenith_registry::Identifier;
use zenith_worldgen::{DensityVolume, RandomState, RouterField, WorldgenData};

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

/// The biome source JSON and noise settings id of a built-in dimension, read from the user's jar.
fn preset_source(
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
    let generator = (|| {
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
    generator.with_context(|| format!("world preset {preset}: no generator for {dimension}"))
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
