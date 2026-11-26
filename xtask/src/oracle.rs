//! `cargo xtask oracle build|run`: build the parity oracle agent and run it inside the user's own vanilla server.

use std::fs;
use std::io::Write as _;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, bail, ensure};
use zenith_install::jar::read_jar_version;
use zenith_install::java::{JavaRuntime, find_java};
use zenith_install::libraries::{game_classpath, join_classpath};
use zenith_install::{GameInstall, ProofOptions, discover_installs, prove_ownership};
use zenith_parity::DumpReader;

use crate::repo_root;

/// The dedicated server's entry point inside the game jar (a fact, from listing the jar's classes).
const SERVER_MAIN: &str = "net.minecraft.server.Main";

fn oracle_dir() -> PathBuf {
    repo_root().join("tools/oracle")
}

fn eula_marker() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(std::env::temp_dir)
        .join("zenith/eula-accepted")
}

pub(crate) fn install_and_java(version: &str) -> anyhow::Result<(GameInstall, JavaRuntime)> {
    let install = discover_installs(Some(version))
        .into_iter()
        .next()
        .with_context(|| format!("no install of {version} found"))?;
    let jar = read_jar_version(&install.client_jar)?;
    let required = jar.java_version.unwrap_or(25);
    let java = find_java(&install, required, jar.java_component.as_deref()).with_context(|| {
        format!("no Java {required}+ runtime; launch {version} once in your launcher")
    })?;
    Ok((install, java))
}

fn java_files(dir: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for e in fs::read_dir(dir)? {
        let p = e?.path();
        if p.is_dir() {
            java_files(&p, out)?;
        } else if p.extension().is_some_and(|x| x == "java") {
            out.push(p);
        }
    }
    Ok(())
}

fn run_tool(java: &JavaRuntime, args: &[&std::ffi::OsStr]) -> anyhow::Result<()> {
    let out = Command::new(&java.java)
        .args(args)
        .output()
        .context("run JDK tool")?;
    ensure!(
        out.status.success(),
        "JDK tool failed:\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(())
}

/// Compile the agent with the launcher-provided JDK modules and package it. Returns the agent jar path.
pub fn build(version: &str) -> anyhow::Result<PathBuf> {
    let (_, java) = install_and_java(version)?;
    let dir = oracle_dir();
    let build = dir.join("build");
    // Build in a private folder and rename the finished jar into place: parallel builds cannot collide, and a vanilla
    // server already running from the old jar keeps reading its (unlinked) file instead of a half-written one.
    static BUILDS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let work = build.join(format!(
        "tmp-{}-{}",
        std::process::id(),
        BUILDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let classes = work.join("classes");
    if work.exists() {
        fs::remove_dir_all(&work)?;
    }
    fs::create_dir_all(classes.join("hooks"))?;

    let mut sources = Vec::new();
    java_files(&dir.join("src"), &mut sources)?;
    let mut args: Vec<std::ffi::OsString> =
        ["-m", "jdk.compiler/com.sun.tools.javac.Main", "--release"]
            .into_iter()
            .map(Into::into)
            .collect();
    args.push(java.major.to_string().into());
    args.push("-d".into());
    args.push(classes.clone().into());
    args.extend(sources.into_iter().map(Into::into));
    run_tool(
        &java,
        &args.iter().map(|a| a.as_os_str()).collect::<Vec<_>>(),
    )?;

    for e in fs::read_dir(dir.join("hooks"))? {
        let p = e?.path();
        fs::copy(
            &p,
            classes
                .join("hooks")
                .join(p.file_name().context("hook file name")?),
        )?;
    }
    let manifest = work.join("MANIFEST.MF");
    fs::write(&manifest, "Premain-Class: dev.zenith.oracle.Agent\n")?;
    let jar = build.join("zenith-oracle.jar");
    let tmp_jar = work.join("zenith-oracle.jar");
    let args: Vec<std::ffi::OsString> = vec![
        "-m".into(),
        "jdk.jartool/sun.tools.jar.Main".into(),
        "--create".into(),
        "--file".into(),
        tmp_jar.clone().into(),
        "--manifest".into(),
        manifest.into(),
        "-C".into(),
        classes.into(),
        ".".into(),
    ];
    run_tool(
        &java,
        &args.iter().map(|a| a.as_os_str()).collect::<Vec<_>>(),
    )?;
    fs::rename(&tmp_jar, &jar).context("install the agent jar")?;
    let _ = fs::remove_dir_all(&work);
    println!(
        "oracle agent: {} (Java {} from {})",
        jar.display(),
        java.major,
        java.source
    );
    Ok(jar)
}

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub version: String,
    pub ticks: u32,
    pub seed: i64,
    pub out: PathBuf,
    pub accept_eula: bool,
    pub timeout_secs: u64,
}

fn eula_accepted(flag: bool) -> anyhow::Result<bool> {
    let marker = eula_marker();
    if flag {
        fs::create_dir_all(marker.parent().context("marker dir")?)?;
        fs::write(
            &marker,
            "Accepted the Minecraft EULA (https://aka.ms/MinecraftEULA) for Zenith oracle runs.\n",
        )?;
        return Ok(true);
    }
    Ok(marker.is_file() || std::env::var("ZENITH_ACCEPT_EULA").is_ok_and(|v| v == "1"))
}

fn free_port() -> anyhow::Result<u16> {
    Ok(TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

/// Start the user's own vanilla dedicated server with the agent attached, for `ticks` ticks.
pub fn run(opts: &RunOptions) -> anyhow::Result<bool> {
    if !eula_accepted(opts.accept_eula)? {
        bail!(
            "the vanilla server only starts after the Minecraft EULA (https://aka.ms/MinecraftEULA) is accepted.\n\
             Re-run with --accept-eula to accept it for Zenith oracle runs (stored in {})",
            eula_marker().display()
        );
    }
    let proof = prove_ownership(&ProofOptions {
        version: opts.version.clone(),
        offline: true,
    });
    ensure!(
        proof.passed,
        "ownership proof failed; run `cargo run -p zenith-install -- check`"
    );
    let (install, java) = install_and_java(&opts.version)?;
    let agent = build(&opts.version)?;
    let cp = game_classpath(&install)?;

    let run_dir = zenith_install::cache_dir().join("oracle").join(format!(
        "run-{}",
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    ));
    fs::create_dir_all(&run_dir)?;
    fs::write(run_dir.join("eula.txt"), "eula=true\n")?;
    let mut props = fs::File::create(run_dir.join("server.properties"))?;
    for (k, v) in [
        ("online-mode", "false".to_owned()),
        ("level-seed", opts.seed.to_string()),
        ("server-port", free_port()?.to_string()),
        ("view-distance", "3".into()),
        ("simulation-distance", "3".into()),
        ("max-tick-time", "-1".into()),
        ("spawn-protection", "0".into()),
        ("sync-chunk-writes", "false".into()),
        ("motd", "Zenith parity oracle".into()),
    ] {
        writeln!(props, "{k}={v}")?;
    }
    drop(props);

    let out = std::path::absolute(&opts.out)?;
    if let Some(d) = out.parent() {
        fs::create_dir_all(d)?;
    }
    let agent_args = format!(
        "out={},ticks={},seed={},version={}",
        out.display(),
        opts.ticks,
        opts.seed,
        opts.version
    );
    let log = fs::File::create(run_dir.join("server.log"))?;
    let started = Instant::now();
    let mut child = Command::new(&java.java)
        .arg("-Xmx2G")
        .arg(format!("-javaagent:{}={agent_args}", agent.display()))
        .arg("-cp")
        .arg(join_classpath(&cp))
        .arg(SERVER_MAIN)
        .args(["--nogui", "--universe", "worlds", "--world", "oracle"])
        .current_dir(&run_dir)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .spawn()
        .context("start vanilla server")?;
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break s;
        }
        if started.elapsed() > Duration::from_secs(opts.timeout_secs) {
            let _ = child.kill();
            bail!(
                "server did not finish within {}s; see {}",
                opts.timeout_secs,
                run_dir.join("server.log").display()
            );
        }
        std::thread::sleep(Duration::from_millis(250));
    };

    let reader = DumpReader::open(&out).with_context(|| {
        format!(
            "no dump written; see {}",
            run_dir.join("server.log").display()
        )
    })?;
    let header = reader.header.clone();
    let mut records = 0u64;
    let mut last_tick = 0u64;
    for r in reader {
        let r = r?;
        records += 1;
        last_tick = r.tick;
    }
    println!(
        "vanilla {} (seed {}): {} records, last tick {} in {:.1}s, server exit {}\n  dump: {}\n  run:  {}",
        header.version,
        header.seed,
        records,
        last_tick,
        started.elapsed().as_secs_f64(),
        status,
        out.display(),
        run_dir.display()
    );
    Ok(records > 0)
}

/// One chunk to dump: dimension id, chunk x, chunk z, and the target status name (e.g. `terrain`).
#[derive(Clone, Debug)]
pub struct ChunkRequest {
    pub dimension: String,
    pub x: i32,
    pub z: i32,
    pub status: String,
}

/// World options for a chunk dump (server.properties).
#[derive(Clone, Debug)]
pub struct ChunkWorld {
    pub seed: i64,
    /// World preset: `minecraft:normal`, `minecraft:large_biomes`, `minecraft:amplified`, ...
    pub level_type: String,
    pub structures: bool,
}

/// Generate the requested chunks in the user's own vanilla server and return the raw `ZTC1` dump
/// (format: docs/specs/parity/terrain-chunks.md). A fresh world is created per call.
pub fn dump_chunks(
    version: &str,
    world: &ChunkWorld,
    requests: &[ChunkRequest],
) -> anyhow::Result<Vec<u8>> {
    dump_chunks_with(version, world, requests, 0)
}

/// [`dump_chunks`] with optional sections (`EXTRA_*` bits); any bit makes the dump a `ZTC2` file.
pub fn dump_chunks_with(
    version: &str,
    world: &ChunkWorld,
    requests: &[ChunkRequest],
    extras: u8,
) -> anyhow::Result<Vec<u8>> {
    if !eula_accepted(false)? {
        bail!(
            "the vanilla server only starts after the Minecraft EULA (https://aka.ms/MinecraftEULA) is accepted; \
             run `cargo xtask oracle run --accept-eula ...` once"
        );
    }
    let (install, java) = install_and_java(version)?;
    // Parallel callers share one agent build and get distinct run folders.
    static AGENT: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);
    static RUNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let agent = {
        let mut a = AGENT.lock().expect("agent lock");
        match a.as_ref() {
            Some(p) => p.clone(),
            None => {
                let p = build(version)?;
                *a = Some(p.clone());
                p
            }
        }
    };
    let cp = game_classpath(&install)?;
    let run_dir = zenith_install::cache_dir().join("oracle").join(format!(
        "chunks-{}-{}-{}",
        chrono::Utc::now().format("%Y%m%d-%H%M%S%3f"),
        std::process::id(),
        RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir_all(&run_dir)?;
    fs::write(run_dir.join("eula.txt"), "eula=true\n")?;
    let mut props = fs::File::create(run_dir.join("server.properties"))?;
    for (k, v) in [
        ("online-mode", "false".to_owned()),
        ("level-seed", world.seed.to_string()),
        ("level-type", world.level_type.clone()),
        ("generate-structures", world.structures.to_string()),
        ("server-port", free_port()?.to_string()),
        ("view-distance", "3".into()),
        ("simulation-distance", "3".into()),
        ("max-tick-time", "-1".into()),
        ("sync-chunk-writes", "false".into()),
        ("motd", "Zenith parity oracle".into()),
    ] {
        writeln!(props, "{k}={v}")?;
    }
    drop(props);

    let mut req = Vec::new();
    req.extend_from_slice(&u32::try_from(requests.len())?.to_be_bytes());
    let put_str = |req: &mut Vec<u8>, s: &str| {
        req.extend_from_slice(&(s.len() as u16).to_be_bytes());
        req.extend_from_slice(s.as_bytes());
    };
    for r in requests {
        put_str(&mut req, &r.dimension);
        req.extend_from_slice(&r.x.to_be_bytes());
        req.extend_from_slice(&r.z.to_be_bytes());
        put_str(&mut req, &r.status);
    }
    let (req_file, out_file, dump) = (
        run_dir.join("chunks.in"),
        run_dir.join("chunks.out"),
        run_dir.join("ticks.zpd"),
    );
    fs::write(&req_file, req)?;
    let agent_args = format!(
        "out={},ticks=1,seed={},version={version},chunks={},chunks_out={},chunks_extra={extras}",
        dump.display(),
        world.seed,
        req_file.display(),
        out_file.display()
    );
    let log = fs::File::create(run_dir.join("server.log"))?;
    let started = Instant::now();
    let mut child = Command::new(&java.java)
        .arg("-Xmx3G")
        .arg(format!("-javaagent:{}={agent_args}", agent.display()))
        .arg("-cp")
        .arg(join_classpath(&cp))
        .arg(SERVER_MAIN)
        .args(["--nogui", "--universe", "worlds", "--world", "oracle"])
        .current_dir(&run_dir)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .spawn()
        .context("start vanilla server")?;
    let timeout = Duration::from_secs(600 + 5 * requests.len() as u64);
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break s;
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            bail!(
                "server did not finish within {}s; see {}",
                timeout.as_secs(),
                run_dir.join("server.log").display()
            );
        }
        std::thread::sleep(Duration::from_millis(250));
    };
    let raw = fs::read(&out_file).with_context(|| {
        format!(
            "no chunk dump written (server exit {status}); see {}",
            run_dir.join("server.log").display()
        )
    })?;
    // The world and dump contain game-generated data: keep nothing in the cache once read.
    let _ = fs::remove_dir_all(&run_dir);
    Ok(raw)
}

/// One chunk of a `ZTC1` dump.
#[derive(Clone, Debug)]
pub struct DumpedChunk {
    pub dimension: String,
    pub x: i32,
    pub z: i32,
    pub requested: String,
    pub reached: String,
    pub min_y: i32,
    pub height: i32,
    /// Global block-state ids; sections bottom-up, inside a section index `(y * 16 + z) * 16 + x`.
    pub blocks: Vec<u32>,
    /// Biome ids per section, 64 each in `(qy, qz, qx)` order.
    pub biomes: Vec<String>,
    /// Heightmap serialization name → 256 first-available heights in `(z, x)` order.
    pub heightmaps: Vec<(String, Vec<i32>)>,
    /// (section index, packed local offset) in list order.
    pub post_processing: Vec<(i32, u16)>,
    pub block_ticks: i32,
    pub fluid_ticks: i32,
    /// Structure starts held by the chunk, sorted by structure id.
    pub starts: Vec<DumpedStart>,
    /// Structure references: structure id → packed start-chunk positions (sorted).
    pub references: Vec<(String, Vec<i64>)>,
    /// `ZTC2` light extra: per light section (bottom-up) its y and the block / sky nibble layers (2048 bytes) if present.
    pub light: Option<Vec<LightSection>>,
    /// `ZTC2` block-entity extra: position (x, y, z) and the saved NBT as network NBT bytes (`None` if none).
    pub block_entities: Option<Vec<DumpedBlockEntity>>,
    /// `ZTC2` entity extra: saved entity NBT in list order (network NBT bytes); `Some(None)` when the chunk keeps none.
    pub entities: Option<Option<Vec<Vec<u8>>>>,
}

/// A block entity of a `ZTC2` dump: position (x, y, z) and its saved NBT (network NBT bytes), if any.
pub type DumpedBlockEntity = ([i32; 3], Option<Vec<u8>>);

/// One light section of a `ZTC2` dump.
#[derive(Clone, Debug)]
pub struct LightSection {
    pub y: i32,
    pub block: Option<Vec<u8>>,
    pub sky: Option<Vec<u8>>,
}

/// Optional dump sections (agent `chunks_extra` bitmask).
pub const EXTRA_LIGHT: u8 = 1;
pub const EXTRA_BLOCK_ENTITIES: u8 = 2;
pub const EXTRA_ENTITIES: u8 = 4;

/// A structure start of a `ZTC1` dump.
#[derive(Clone, Debug)]
pub struct DumpedStart {
    pub structure: String,
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub references: i32,
    pub valid: bool,
    /// min x, y, z, max x, y, z (inclusive).
    pub bounds: [i32; 6],
    pub pieces: Vec<DumpedPiece>,
}

/// One piece of a structure start.
#[derive(Clone, Debug)]
pub struct DumpedPiece {
    pub kind: String,
    pub bounds: [i32; 6],
    /// Orientation name (`north`, ...) or empty.
    pub orientation: String,
    pub depth: i32,
}

/// Parse a `ZTC1` dump (docs/specs/parity/terrain-chunks.md).
pub fn parse_chunk_dump(raw: &[u8]) -> anyhow::Result<Vec<DumpedChunk>> {
    struct R<'a>(&'a [u8], usize);
    impl R<'_> {
        fn take(&mut self, n: usize) -> anyhow::Result<&[u8]> {
            let b = self
                .0
                .get(self.1..self.1 + n)
                .context("truncated ZTC1 dump")?;
            self.1 += n;
            Ok(b)
        }
        fn u16(&mut self) -> anyhow::Result<u16> {
            Ok(u16::from_be_bytes(self.take(2)?.try_into()?))
        }
        fn i32(&mut self) -> anyhow::Result<i32> {
            Ok(i32::from_be_bytes(self.take(4)?.try_into()?))
        }
        fn str(&mut self) -> anyhow::Result<String> {
            let n = self.u16()? as usize;
            Ok(String::from_utf8(self.take(n)?.to_vec())?)
        }
        fn i64(&mut self) -> anyhow::Result<i64> {
            Ok(i64::from_be_bytes(self.take(8)?.try_into()?))
        }
        fn bounds(&mut self) -> anyhow::Result<[i32; 6]> {
            Ok([
                self.i32()?,
                self.i32()?,
                self.i32()?,
                self.i32()?,
                self.i32()?,
                self.i32()?,
            ])
        }
    }
    let mut r = R(raw, 0);
    let magic = r.take(4)?.to_vec();
    ensure!(
        magic == b"ZTC1" || magic == b"ZTC2",
        "not a ZTC1 / ZTC2 dump"
    );
    let n = r.i32()?;
    let extras = if magic == b"ZTC2" { r.take(1)?[0] } else { 0 };
    let mut chunks = Vec::with_capacity(n.max(0) as usize);
    for _ in 0..n {
        let (dimension, x, z) = (r.str()?, r.i32()?, r.i32()?);
        let (requested, reached) = (r.str()?, r.str()?);
        let (min_y, height, sections) = (r.i32()?, r.i32()?, r.i32()? as usize);
        let blocks = (0..sections * 4096)
            .map(|_| r.i32().map(|v| v as u32))
            .collect::<anyhow::Result<Vec<_>>>()?;
        let table = (0..r.u16()?)
            .map(|_| r.str())
            .collect::<anyhow::Result<Vec<_>>>()?;
        let biomes = (0..sections * 64)
            .map(|_| {
                let i = r.u16()? as usize;
                table.get(i).cloned().context("biome index out of range")
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let maps = r.take(1)?[0];
        let mut heightmaps = Vec::new();
        for _ in 0..maps {
            let name = r.str()?;
            let h = (0..256)
                .map(|_| r.i32())
                .collect::<anyhow::Result<Vec<_>>>()?;
            heightmaps.push((name, h));
        }
        let marks = r.i32()?;
        let post_processing = (0..marks)
            .map(|_| Ok((r.i32()?, r.u16()?)))
            .collect::<anyhow::Result<Vec<_>>>()?;
        let (block_ticks, fluid_ticks) = (r.i32()?, r.i32()?);
        let mut starts = Vec::new();
        for _ in 0..r.i32()? {
            let structure = r.str()?;
            let (chunk_x, chunk_z, references) = (r.i32()?, r.i32()?, r.i32()?);
            let valid = r.take(1)?[0] != 0;
            let bounds = r.bounds()?;
            let mut pieces = Vec::new();
            for _ in 0..r.i32()? {
                let kind = r.str()?;
                let bounds = r.bounds()?;
                let orientation = r.str()?;
                pieces.push(DumpedPiece {
                    kind,
                    bounds,
                    orientation,
                    depth: r.i32()?,
                });
            }
            starts.push(DumpedStart {
                structure,
                chunk_x,
                chunk_z,
                references,
                valid,
                bounds,
                pieces,
            });
        }
        let mut references = Vec::new();
        for _ in 0..r.i32()? {
            let id = r.str()?;
            let longs = (0..r.i32()?)
                .map(|_| r.i64())
                .collect::<anyhow::Result<Vec<_>>>()?;
            references.push((id, longs));
        }
        let light = if extras & EXTRA_LIGHT != 0 {
            let mut v = Vec::new();
            for _ in 0..r.i32()? {
                let y = r.i32()?;
                let block = if r.take(1)?[0] != 0 {
                    Some(r.take(2048)?.to_vec())
                } else {
                    None
                };
                let sky = if r.take(1)?[0] != 0 {
                    Some(r.take(2048)?.to_vec())
                } else {
                    None
                };
                v.push(LightSection { y, block, sky });
            }
            Some(v)
        } else {
            None
        };
        let nbt = |r: &mut R<'_>| -> anyhow::Result<Option<Vec<u8>>> {
            let len = r.i32()?;
            Ok(if len < 0 {
                None
            } else {
                Some(r.take(len as usize)?.to_vec())
            })
        };
        let block_entities = if extras & EXTRA_BLOCK_ENTITIES != 0 {
            let mut v = Vec::new();
            for _ in 0..r.i32()? {
                let pos = [r.i32()?, r.i32()?, r.i32()?];
                v.push((pos, nbt(&mut r)?));
            }
            Some(v)
        } else {
            None
        };
        let entities = if extras & EXTRA_ENTITIES != 0 {
            let count = r.i32()?;
            if count < 0 {
                Some(None)
            } else {
                let mut v = Vec::new();
                for _ in 0..count {
                    v.push(nbt(&mut r)?.unwrap_or_default());
                }
                Some(Some(v))
            }
        } else {
            None
        };
        chunks.push(DumpedChunk {
            dimension,
            x,
            z,
            requested,
            reached,
            min_y,
            height,
            blocks,
            biomes,
            heightmaps,
            post_processing,
            block_ticks,
            fluid_ticks,
            starts,
            references,
            light,
            block_entities,
            entities,
        });
    }
    ensure!(r.1 == raw.len(), "trailing bytes in ZTC1 dump");
    Ok(chunks)
}

/// `cargo xtask oracle chunks`: dump chunks and print per-chunk statistics (block histogram, heights).
pub fn chunks_command(
    version: &str,
    seed: i64,
    level_type: String,
    structures: bool,
    specs: &[String],
    status: &str,
    extras: u8,
) -> anyhow::Result<bool> {
    let mut requests = Vec::new();
    for s in specs {
        let (dim, rest) = s
            .rsplit_once(':')
            .and_then(|(a, z)| a.rsplit_once(':').map(|(d, x)| (d, (x, z))))
            .with_context(|| format!("bad --chunk {s}; use dimension:x:z"))?;
        requests.push(ChunkRequest {
            dimension: dim.to_owned(),
            x: rest.0.parse()?,
            z: rest.1.parse()?,
            status: status.to_owned(),
        });
    }
    let world = ChunkWorld {
        seed,
        level_type,
        structures,
    };
    let started = Instant::now();
    let chunks = parse_chunk_dump(&dump_chunks_with(version, &world, &requests, extras)?)?;
    let tables = zenith_data::Tables::load(&zenith_data::version_dir(version).join("tables.bin"))
        .context("no cached tables; run `cargo run -p zenith-data -- ingest` first")?;
    let table = zenith_registry::block_state::BlockStateTable::from_tables(&tables)
        .map_err(|e| anyhow::anyhow!("block-state table: {e}"))?;
    let mut names = vec![String::new(); table.len()];
    for b in table.blocks() {
        for s in b.first_state..b.first_state + b.state_count {
            names[s as usize] = b.id.to_string();
        }
    }
    println!(
        "{} chunks in {:.1}s",
        chunks.len(),
        started.elapsed().as_secs_f64()
    );
    for c in &chunks {
        let mut hist: std::collections::BTreeMap<&str, u32> = std::collections::BTreeMap::new();
        for &b in &c.blocks {
            *hist
                .entry(names.get(b as usize).map_or("?", String::as_str))
                .or_default() += 1;
        }
        let mut top: Vec<_> = hist.into_iter().collect();
        top.sort_by_key(|e| std::cmp::Reverse(e.1));
        let bottom = names.get(c.blocks[0] as usize).map_or("?", String::as_str);
        let mut biomes: Vec<&str> = c.biomes.iter().map(String::as_str).collect();
        biomes.dedup();
        biomes.sort_unstable();
        biomes.dedup();
        println!(
            "{} ({},{}) {} -> {}: y {}..{}, bottom {bottom}, post-processing {}, ticks {}/{}\n  blocks: {}\n  biomes: {}\n  heightmaps: {}",
            c.dimension,
            c.x,
            c.z,
            c.requested,
            c.reached,
            c.min_y,
            c.min_y + c.height,
            c.post_processing.len(),
            c.block_ticks,
            c.fluid_ticks,
            top.iter()
                .take(8)
                .map(|(n, k)| format!("{n} {k}"))
                .collect::<Vec<_>>()
                .join(", "),
            biomes.join(", "),
            c.heightmaps
                .iter()
                .map(|(n, h)| format!("{n}[0]={}", h[0]))
                .collect::<Vec<_>>()
                .join(" ")
        );
        for st in &c.starts {
            println!(
                "  start {} at chunk ({},{}) refs {} valid {} box {:?}: {} pieces, first {}",
                st.structure,
                st.chunk_x,
                st.chunk_z,
                st.references,
                st.valid,
                st.bounds,
                st.pieces.len(),
                st.pieces.first().map_or_else(
                    || "-".to_owned(),
                    |p| format!(
                        "{} {:?} '{}' depth {}",
                        p.kind, p.bounds, p.orientation, p.depth
                    )
                )
            );
        }
        if let Some(light) = &c.light {
            let block = light.iter().filter(|l| l.block.is_some()).count();
            let sky = light.iter().filter(|l| l.sky.is_some()).count();
            let max_block = light
                .iter()
                .filter_map(|l| l.block.as_ref())
                .flat_map(|d| d.iter().flat_map(|x| [x & 15, x >> 4]))
                .max();
            println!(
                "  light: sections y {}..={}, {block} block layers (max {max_block:?}), {sky} sky layers",
                light.first().map_or(0, |l| l.y),
                light.last().map_or(0, |l| l.y)
            );
        }
        if let Some(be) = &c.block_entities {
            println!(
                "  block entities: {}{}",
                be.len(),
                be.first().map_or(String::new(), |(p, n)| format!(
                    ", first at {p:?} ({} NBT bytes)",
                    n.as_ref().map_or(0, Vec::len)
                ))
            );
        }
        if let Some(e) = &c.entities {
            println!(
                "  entities: {}",
                e.as_ref()
                    .map_or_else(|| "kept by the level".to_owned(), |v| v.len().to_string())
            );
        }
        if !c.references.is_empty() {
            println!(
                "  references: {}",
                c.references
                    .iter()
                    .map(|(id, l)| {
                        let starts: Vec<String> = l
                            .iter()
                            .map(|&v| format!("({},{})", v as i32, (v >> 32) as i32))
                            .collect();
                        format!("{id} from {}", starts.join(" "))
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
    Ok(!chunks.is_empty())
}
