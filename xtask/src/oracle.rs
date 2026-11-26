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

// ---------------------------------------------------------------------------------------------------------------
// Tick-by-tick scenarios (format: docs/specs/parity/scenarios.md): `ZSC1` in, `ZSO1` out.
// ---------------------------------------------------------------------------------------------------------------

/// World options for [`run_scenario`] (server.properties). The default is a superflat overworld holding a single
/// bedrock layer at the bottom of the world, biome plains, no structures, no features: nothing but air to build in.
#[derive(Clone, Debug)]
pub struct ScenarioWorld {
    pub seed: i64,
    pub level_type: String,
    /// The `generator-settings` JSON (only read for `minecraft:flat`).
    pub generator_settings: String,
    pub difficulty: String,
}

impl Default for ScenarioWorld {
    fn default() -> Self {
        Self {
            seed: 0,
            level_type: "minecraft:flat".into(),
            generator_settings: r#"{"layers":[{"block":"minecraft:bedrock","height":1}],"biome":"minecraft:plains","features":false,"lakes":false,"structure_overrides":[]}"#.into(),
            difficulty: "normal".into(),
        }
    }
}

/// Run a `ZSC1` scenario (see [`ScenarioSpec::to_bytes`]) in a fresh vanilla world and return the raw `ZSO1` output.
/// Parse it with [`parse_scenario_output`].
pub fn run_scenario(
    version: &str,
    world: &ScenarioWorld,
    scenario: &[u8],
) -> anyhow::Result<Vec<u8>> {
    if !eula_accepted(false)? {
        bail!(
            "the vanilla server only starts after the Minecraft EULA (https://aka.ms/MinecraftEULA) is accepted; \
             run `cargo xtask oracle run --accept-eula ...` once"
        );
    }
    let (install, java) = install_and_java(version)?;
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
        "scenario-{}-{}-{}",
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
        ("generator-settings", world.generator_settings.clone()),
        ("generate-structures", "false".into()),
        ("difficulty", world.difficulty.clone()),
        ("spawn-monsters", "false".into()),
        ("spawn-animals", "false".into()),
        ("spawn-npcs", "false".into()),
        ("allow-nether", "false".into()),
        ("pause-when-empty-seconds", "0".into()),
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

    let (scn_file, out_file) = (run_dir.join("scenario.in"), run_dir.join("scenario.out"));
    fs::write(&scn_file, scenario)?;
    let agent_args = format!(
        "scenario={},scenario_out={},version={version}",
        scn_file.display(),
        out_file.display()
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
    let timeout = Duration::from_secs(1800);
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
        std::thread::sleep(Duration::from_millis(100));
    };
    let raw = fs::read(&out_file).with_context(|| {
        format!(
            "no scenario output written (server exit {status}); see {}",
            run_dir.join("server.log").display()
        )
    })?;
    // The world and the output hold game-generated data: keep nothing in the cache once read (unless the caller
    // opts in to debugging the server log with ZENITH_KEEP_ORACLE_RUN=1; the folder is in the user's own cache).
    if std::env::var_os("ZENITH_KEEP_ORACLE_RUN").is_none() {
        let _ = fs::remove_dir_all(&run_dir);
    } else {
        eprintln!("kept {}", run_dir.display());
    }
    Ok(raw)
}

/// How a scenario pins the level random (`Level.random`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(dead_code)] // public scenario API, fields/variants are for the parity suites
pub enum RngPin {
    /// Leave it alone (it is seeded from the clock and not reproducible).
    #[default]
    None,
    /// `setSeed(value)`: the 48-bit state becomes `(value ^ 0x5DEECE66D) & (2^48 - 1)`.
    Seed(i64),
    /// The raw 48-bit internal state is `value & (2^48 - 1)`.
    Raw(i64),
}

/// Which part of a timeline tick an operation runs in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Phase {
    /// Before the body of the server tick (before game time advances).
    Start = 0,
    /// After the body of the server tick.
    End = 1,
}

/// `ZSC1` operation codes' payloads.
#[derive(Clone, Debug)]
#[allow(dead_code)] // public scenario API, fields/variants are for the parity suites
pub enum ScenarioOp {
    SetBlock {
        pos: [i32; 3],
        state: u32,
        flags: i32,
    },
    /// Inclusive box; cells are written y ascending, then z, then x.
    Fill {
        min: [i32; 3],
        max: [i32; 3],
        state: u32,
        flags: i32,
    },
    ScheduleBlock {
        pos: [i32; 3],
        block: i32,
        delay: i32,
        priority: i32,
    },
    ScheduleFluid {
        pos: [i32; 3],
        fluid: i32,
        delay: i32,
        priority: i32,
    },
    BlockEvent {
        pos: [i32; 3],
        block: i32,
        a: i32,
        b: i32,
    },
    PinRng {
        rng: RngPin,
        rand_value: Option<i32>,
    },
    Dump {
        min: [i32; 3],
        max: [i32; 3],
        sections: u8,
    },
    GameRule {
        name: String,
        value: String,
    },
}

pub const SECTION_BLOCKS: u8 = 1;
pub const SECTION_TICKS: u8 = 2;
pub const SECTION_EVENTS: u8 = 4;
pub const SECTION_BLOCK_ENTITIES: u8 = 8;
pub const SECTION_ALL: u8 = 15;

/// A scenario in memory; [`ScenarioSpec::to_bytes`] writes the `ZSC1` file.
#[derive(Clone, Debug)]
pub struct ScenarioSpec {
    pub dimension: String,
    /// Inclusive chunk box `[min_x, min_z, max_x, max_z]` held loaded and ticking by forced-chunk tickets.
    pub chunk_box: [i32; 4],
    /// Wait for the light engine at every tick boundary (strongly recommended).
    pub light_sync: bool,
    /// Run the server far faster than 20 ticks per second (does not change what a tick does).
    pub fast_ticks: bool,
    pub gamerules: Vec<(String, String)>,
    /// Level-random pin applied when the timeline starts.
    pub rng: RngPin,
    /// Random-tick LCG value (`Level.randValue`) applied when the timeline starts.
    pub rand_value: Option<i32>,
    /// Number of timeline ticks to run.
    pub total_ticks: i32,
    pub ops: Vec<(i32, Phase, ScenarioOp)>,
}

impl ScenarioSpec {
    /// The overworld, a chunk box, the recommended game rules and no operations yet.
    pub fn new(chunk_box: [i32; 4], total_ticks: i32) -> Self {
        Self {
            dimension: "minecraft:overworld".into(),
            chunk_box,
            light_sync: true,
            fast_ticks: true,
            gamerules: [
                ("advance_time", "false"),
                ("advance_weather", "false"),
                ("spawn_mobs", "false"),
                ("spawn_monsters", "false"),
                ("raids", "false"),
                ("random_tick_speed", "0"),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect(),
            rng: RngPin::Seed(0),
            rand_value: Some(0),
            total_ticks,
            ops: Vec::new(),
        }
    }

    pub fn rule(&mut self, name: &str, value: &str) {
        self.gamerules.retain(|(k, _)| k != name);
        self.gamerules.push((name.to_owned(), value.to_owned()));
    }

    pub fn op(&mut self, tick: i32, phase: Phase, op: ScenarioOp) {
        self.ops.push((tick, phase, op));
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        fn s(out: &mut Vec<u8>, v: &str) {
            out.extend_from_slice(&(v.len() as u16).to_be_bytes());
            out.extend_from_slice(v.as_bytes());
        }
        fn i(out: &mut Vec<u8>, v: i32) {
            out.extend_from_slice(&v.to_be_bytes());
        }
        fn l(out: &mut Vec<u8>, v: i64) {
            out.extend_from_slice(&v.to_be_bytes());
        }
        fn p(out: &mut Vec<u8>, v: [i32; 3]) {
            for c in v {
                i(out, c);
            }
        }
        fn pin(rng: RngPin) -> (u8, i64) {
            match rng {
                RngPin::None => (0, 0),
                RngPin::Seed(v) => (1, v),
                RngPin::Raw(v) => (2, v),
            }
        }
        let mut o = b"ZSC1".to_vec();
        s(&mut o, &self.dimension);
        for c in self.chunk_box {
            i(&mut o, c);
        }
        o.push(u8::from(self.light_sync) | (u8::from(self.fast_ticks) << 1));
        o.extend_from_slice(&(self.gamerules.len() as u16).to_be_bytes());
        for (k, v) in &self.gamerules {
            s(&mut o, k);
            s(&mut o, v);
        }
        let (mode, value) = pin(self.rng);
        o.push(mode);
        l(&mut o, value);
        o.push(u8::from(self.rand_value.is_some()));
        i(&mut o, self.rand_value.unwrap_or(0));
        i(&mut o, self.total_ticks);
        i(&mut o, self.ops.len() as i32);
        for (tick, phase, op) in &self.ops {
            i(&mut o, *tick);
            o.push(*phase as u8);
            match op {
                ScenarioOp::SetBlock { pos, state, flags } => {
                    o.push(1);
                    p(&mut o, *pos);
                    i(&mut o, *state as i32);
                    i(&mut o, *flags);
                }
                ScenarioOp::Fill {
                    min,
                    max,
                    state,
                    flags,
                } => {
                    o.push(2);
                    p(&mut o, *min);
                    p(&mut o, *max);
                    i(&mut o, *state as i32);
                    i(&mut o, *flags);
                }
                ScenarioOp::ScheduleBlock {
                    pos,
                    block,
                    delay,
                    priority,
                } => {
                    o.push(3);
                    p(&mut o, *pos);
                    for v in [*block, *delay, *priority] {
                        i(&mut o, v);
                    }
                }
                ScenarioOp::ScheduleFluid {
                    pos,
                    fluid,
                    delay,
                    priority,
                } => {
                    o.push(4);
                    p(&mut o, *pos);
                    for v in [*fluid, *delay, *priority] {
                        i(&mut o, v);
                    }
                }
                ScenarioOp::BlockEvent { pos, block, a, b } => {
                    o.push(5);
                    p(&mut o, *pos);
                    for v in [*block, *a, *b] {
                        i(&mut o, v);
                    }
                }
                ScenarioOp::PinRng { rng, rand_value } => {
                    o.push(6);
                    let (mode, value) = pin(*rng);
                    o.push(mode);
                    l(&mut o, value);
                    o.push(u8::from(rand_value.is_some()));
                    i(&mut o, rand_value.unwrap_or(0));
                }
                ScenarioOp::Dump { min, max, sections } => {
                    o.push(7);
                    p(&mut o, *min);
                    p(&mut o, *max);
                    o.push(*sections);
                }
                ScenarioOp::GameRule { name, value } => {
                    o.push(8);
                    s(&mut o, name);
                    s(&mut o, value);
                }
            }
        }
        o
    }
}

/// One scheduled block / fluid tick in a dump.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DumpedScheduledTick {
    pub pos: [i32; 3],
    pub type_id: i32,
    pub type_name: String,
    pub trigger_tick: i64,
    pub priority: i32,
    pub sub_tick_order: i64,
}

/// One pending block event in a dump.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DumpedBlockEvent {
    pub pos: [i32; 3],
    pub block_id: i32,
    pub block_name: String,
    pub a: i32,
    pub b: i32,
}

/// One dump point of a `ZSO1` output.
#[derive(Clone, Debug)]
#[allow(dead_code)] // public scenario API, fields/variants are for the parity suites
pub struct ScenarioDumpRecord {
    pub timeline_tick: i32,
    pub phase: u8,
    pub game_time: i64,
    /// Raw 48-bit internal state of `Level.random`.
    pub random_state: i64,
    /// Cached second Gaussian of the level random, if any.
    pub gaussian: Option<f64>,
    /// The random-tick LCG value (`Level.randValue`).
    pub rand_value: i32,
    pub sub_tick_count: i64,
    pub level_block_ticks: i32,
    pub level_fluid_ticks: i32,
    /// Chunks in the order the level visits them for random ticks.
    pub ticking_chunks: Vec<[i32; 2]>,
    pub min: [i32; 3],
    pub max: [i32; 3],
    pub sections: u8,
    pub blocks: Option<Vec<u32>>,
    pub block_ticks: Option<Vec<DumpedScheduledTick>>,
    pub fluid_ticks: Option<Vec<DumpedScheduledTick>>,
    pub events: Option<Vec<DumpedBlockEvent>>,
    pub block_entities: Option<Vec<DumpedBlockEntity>>,
}

/// A parsed `ZSO1` output.
#[derive(Clone, Debug)]
pub struct ScenarioOutput {
    pub dimension: String,
    pub planned_ticks: i32,
    pub dumps: Vec<ScenarioDumpRecord>,
    /// 0 = the timeline finished; anything else = the runner stopped with `message`.
    pub status: u8,
    pub message: String,
    pub ticks_run: i32,
}

/// Parse a `ZSO1` output (format: docs/specs/parity/scenarios.md).
pub fn parse_scenario_output(raw: &[u8]) -> anyhow::Result<ScenarioOutput> {
    struct R<'a>(&'a [u8]);
    impl R<'_> {
        fn take(&mut self, n: usize) -> anyhow::Result<&[u8]> {
            ensure!(self.0.len() >= n, "truncated scenario output");
            let (a, b) = self.0.split_at(n);
            self.0 = b;
            Ok(a)
        }
        fn u8(&mut self) -> anyhow::Result<u8> {
            Ok(self.take(1)?[0])
        }
        fn i32(&mut self) -> anyhow::Result<i32> {
            Ok(i32::from_be_bytes(self.take(4)?.try_into()?))
        }
        fn i64(&mut self) -> anyhow::Result<i64> {
            Ok(i64::from_be_bytes(self.take(8)?.try_into()?))
        }
        fn f64(&mut self) -> anyhow::Result<f64> {
            Ok(f64::from_be_bytes(self.take(8)?.try_into()?))
        }
        fn str(&mut self) -> anyhow::Result<String> {
            let n = u16::from_be_bytes(self.take(2)?.try_into()?) as usize;
            Ok(String::from_utf8(self.take(n)?.to_vec())?)
        }
        fn pos(&mut self) -> anyhow::Result<[i32; 3]> {
            Ok([self.i32()?, self.i32()?, self.i32()?])
        }
        fn ticks(&mut self) -> anyhow::Result<Vec<DumpedScheduledTick>> {
            let n = self.i32()?;
            (0..n)
                .map(|_| {
                    Ok(DumpedScheduledTick {
                        pos: self.pos()?,
                        type_id: self.i32()?,
                        type_name: self.str()?,
                        trigger_tick: self.i64()?,
                        priority: self.i32()?,
                        sub_tick_order: self.i64()?,
                    })
                })
                .collect()
        }
    }
    let mut r = R(raw);
    ensure!(r.take(4)? == b"ZSO1", "not a ZSO1 scenario output");
    let dimension = r.str()?;
    let planned_ticks = r.i32()?;
    let mut dumps = Vec::new();
    loop {
        match r.u8()? {
            1 => {
                let timeline_tick = r.i32()?;
                let phase = r.u8()?;
                let game_time = r.i64()?;
                let random_state = r.i64()?;
                let has_gauss = r.u8()? != 0;
                let g = r.f64()?;
                let rand_value = r.i32()?;
                let sub_tick_count = r.i64()?;
                let level_block_ticks = r.i32()?;
                let level_fluid_ticks = r.i32()?;
                let n = r.i32()?;
                let mut ticking_chunks = Vec::new();
                for _ in 0..n {
                    ticking_chunks.push([r.i32()?, r.i32()?]);
                }
                let min = r.pos()?;
                let max = r.pos()?;
                let sections = r.u8()?;
                let mut d = ScenarioDumpRecord {
                    timeline_tick,
                    phase,
                    game_time,
                    random_state,
                    gaussian: has_gauss.then_some(g),
                    rand_value,
                    sub_tick_count,
                    level_block_ticks,
                    level_fluid_ticks,
                    ticking_chunks,
                    min,
                    max,
                    sections,
                    blocks: None,
                    block_ticks: None,
                    fluid_ticks: None,
                    events: None,
                    block_entities: None,
                };
                if sections & SECTION_BLOCKS != 0 {
                    let n = r.i32()?;
                    let mut b = Vec::with_capacity(n as usize);
                    for _ in 0..n {
                        b.push(r.i32()? as u32);
                    }
                    d.blocks = Some(b);
                }
                if sections & SECTION_TICKS != 0 {
                    d.block_ticks = Some(r.ticks()?);
                    d.fluid_ticks = Some(r.ticks()?);
                }
                if sections & SECTION_EVENTS != 0 {
                    let n = r.i32()?;
                    let mut ev = Vec::new();
                    for _ in 0..n {
                        ev.push(DumpedBlockEvent {
                            pos: r.pos()?,
                            block_id: r.i32()?,
                            block_name: r.str()?,
                            a: r.i32()?,
                            b: r.i32()?,
                        });
                    }
                    d.events = Some(ev);
                }
                if sections & SECTION_BLOCK_ENTITIES != 0 {
                    let n = r.i32()?;
                    let mut be = Vec::new();
                    for _ in 0..n {
                        let pos = r.pos()?;
                        let len = r.i32()?;
                        let nbt = if len < 0 {
                            None
                        } else {
                            Some(r.take(len as usize)?.to_vec())
                        };
                        be.push((pos, nbt));
                    }
                    d.block_entities = Some(be);
                }
                dumps.push(d);
            }
            0xFF => {
                let status = r.u8()?;
                let message = r.str()?;
                let ticks_run = r.i32()?;
                ensure!(r.0.is_empty(), "trailing bytes in scenario output");
                return Ok(ScenarioOutput {
                    dimension,
                    planned_ticks,
                    dumps,
                    status,
                    message,
                    ticks_run,
                });
            }
            t => bail!("unknown scenario output record tag {t}"),
        }
    }
}

// ---- built-in example scenarios --------------------------------------------------------------------------------

/// Block-state and registry ids for building scenarios, read from the ingested tables (no game data in the repo).
#[allow(dead_code)] // public scenario API, fields/variants are for the parity suites
pub struct ScenarioIds {
    states: zenith_registry::block_state::BlockStateTable,
    fluids: Vec<(String, u32)>,
}

#[allow(dead_code)]
impl ScenarioIds {
    pub fn load(version: &str) -> anyhow::Result<Self> {
        let tables =
            zenith_data::Tables::load(&zenith_data::version_dir(version).join("tables.bin"))
                .context("no cached tables; run `cargo run -p zenith-data -- ingest` first")?;
        let states = zenith_registry::block_state::BlockStateTable::from_tables(&tables)
            .map_err(|e| anyhow::anyhow!("block-state table: {e}"))?;
        let fluids = tables
            .registry("minecraft:fluid")
            .context("no fluid registry in the tables")?
            .entries
            .iter()
            .map(|e| (e.name.clone(), e.protocol_id))
            .collect();
        Ok(Self { states, fluids })
    }

    fn ident(name: &str) -> anyhow::Result<zenith_registry::identifier::Identifier> {
        format!("minecraft:{name}")
            .parse()
            .map_err(|e| anyhow::anyhow!("bad identifier {name}: {e:?}"))
    }

    /// Global state id of `minecraft:<name>`; unlisted properties take the block's default value.
    pub fn state(&self, name: &str, props: &[(&str, &str)]) -> anyhow::Result<u32> {
        let id = Self::ident(name)?;
        let block = self
            .states
            .block(&id)
            .with_context(|| format!("no block {name}"))?;
        let default = self
            .states
            .state_by_id_exact(block.default_state as i32)
            .context("default state")?;
        let mut all: Vec<(String, String)> = default.properties.clone();
        for (k, v) in props {
            let slot = all
                .iter_mut()
                .find(|(pk, _)| pk == k)
                .with_context(|| format!("{name} has no property {k}"))?;
            slot.1 = (*v).to_owned();
        }
        let pairs: Vec<(&str, &str)> = all.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        self.states
            .id_of_exact(&id, &pairs)
            .with_context(|| format!("no state {name}{props:?}"))
    }

    /// Block registry raw id of `minecraft:<name>`.
    pub fn block(&self, name: &str) -> anyhow::Result<i32> {
        let id = Self::ident(name)?;
        Ok(self
            .states
            .block(&id)
            .with_context(|| format!("no block {name}"))?
            .block_protocol_id as i32)
    }

    /// Fluid registry raw id of `minecraft:<name>`.
    pub fn fluid(&self, name: &str) -> anyhow::Result<i32> {
        let full = format!("minecraft:{name}");
        self.fluids
            .iter()
            .find(|(n, _)| *n == full)
            .map(|(_, id)| *id as i32)
            .with_context(|| format!("no fluid {name}"))
    }

    /// `block[prop=value,...]` for a global state id (for reports).
    pub fn describe(&self, state: u32) -> String {
        match self.states.state_by_id_exact(state as i32) {
            Some(s) => {
                let name = s.block_id.to_string();
                let name = name.strip_prefix("minecraft:").unwrap_or(&name).to_owned();
                if s.properties.is_empty() {
                    name
                } else {
                    let p: Vec<String> = s
                        .properties
                        .iter()
                        .map(|(k, v)| format!("{k}={v}"))
                        .collect();
                    format!("{name}[{}]", p.join(","))
                }
            }
            None => format!("?{state}"),
        }
    }
}

/// Names of the built-in example scenarios.
pub const SCENARIO_EXAMPLES: &[&str] = &[
    "torch-clock",
    "water-spread",
    "piston-12",
    "sand-fall",
    "crop-random-tick",
    "ops-smoke",
];

/// Common prefix of the examples: chunk box (4,4)..(5,5) = blocks x/z 64..95; a stone floor at y = 0.
const EX_CHUNKS: [i32; 4] = [4, 4, 5, 5];

fn ex_floor(spec: &mut ScenarioSpec, ids: &ScenarioIds, half: i32) -> anyhow::Result<()> {
    let stone = ids.state("stone", &[])?;
    spec.op(
        0,
        Phase::Start,
        ScenarioOp::Fill {
            min: [80 - half, 0, 80 - half],
            max: [80 + half, 0, 80 + half],
            state: stone,
            // 2 | 16: client update only, no neighbour reactions, no shape updates: a silent build step
            flags: 18,
        },
    );
    Ok(())
}

/// Build one of [`SCENARIO_EXAMPLES`].
pub fn scenario_example(name: &str, ids: &ScenarioIds) -> anyhow::Result<ScenarioSpec> {
    let (cx, cz) = (80, 80);
    let end = Phase::End;
    match name {
        "torch-clock" => {
            // A wall torch on block B; C above the torch is strongly powered while it is lit and feeds dust lying on
            // B, which powers B and turns the torch off: a two-delay (4-tick) loop.
            let mut s = ScenarioSpec::new(EX_CHUNKS, 24);
            ex_floor(&mut s, ids, 6)?;
            let stone = ids.state("stone", &[])?;
            let torch = ids.state(
                "redstone_wall_torch",
                &[("facing", "east"), ("lit", "true")],
            )?;
            let dust = ids.state("redstone_wire", &[])?;
            s.op(
                0,
                Phase::Start,
                ScenarioOp::SetBlock {
                    pos: [cx, 1, cz],
                    state: stone,
                    flags: 18,
                },
            );
            s.op(
                0,
                Phase::Start,
                ScenarioOp::SetBlock {
                    pos: [cx + 1, 2, cz],
                    state: stone,
                    flags: 18,
                },
            );
            s.op(
                0,
                Phase::Start,
                ScenarioOp::SetBlock {
                    pos: [cx + 1, 1, cz],
                    state: torch,
                    flags: 3,
                },
            );
            s.op(
                0,
                Phase::Start,
                ScenarioOp::SetBlock {
                    pos: [cx, 2, cz],
                    state: dust,
                    flags: 3,
                },
            );
            for t in 0..24 {
                s.op(
                    t,
                    end,
                    ScenarioOp::Dump {
                        min: [cx, 1, cz],
                        max: [cx + 1, 2, cz],
                        sections: SECTION_BLOCKS | SECTION_TICKS,
                    },
                );
            }
            Ok(s)
        }
        "water-spread" => {
            let mut s = ScenarioSpec::new(EX_CHUNKS, 41);
            ex_floor(&mut s, ids, 10)?;
            let water = ids.state("water", &[("level", "0")])?;
            s.op(
                0,
                Phase::Start,
                ScenarioOp::SetBlock {
                    pos: [cx, 1, cz],
                    state: water,
                    flags: 3,
                },
            );
            for t in 0..41 {
                if t % 5 == 4 || t < 2 {
                    s.op(
                        t,
                        end,
                        ScenarioOp::Dump {
                            min: [cx - 9, 1, cz - 9],
                            max: [cx + 9, 1, cz + 9],
                            sections: SECTION_BLOCKS | SECTION_TICKS,
                        },
                    );
                }
            }
            Ok(s)
        }
        "piston-12" => {
            // A piston facing east pushes a line of 12 stone blocks (the most it can move).
            let mut s = ScenarioSpec::new(EX_CHUNKS, 12);
            ex_floor(&mut s, ids, 12)?;
            let stone = ids.state("stone", &[])?;
            let piston = ids.state("piston", &[("facing", "east"), ("extended", "false")])?;
            let rs = ids.state("redstone_block", &[])?;
            s.op(
                0,
                Phase::Start,
                ScenarioOp::SetBlock {
                    pos: [cx, 1, cz],
                    state: piston,
                    flags: 18,
                },
            );
            s.op(
                0,
                Phase::Start,
                ScenarioOp::Fill {
                    min: [cx + 1, 1, cz],
                    max: [cx + 12, 1, cz],
                    state: stone,
                    flags: 18,
                },
            );
            s.op(
                2,
                Phase::Start,
                ScenarioOp::SetBlock {
                    pos: [cx, 2, cz],
                    state: rs,
                    flags: 3,
                },
            );
            for t in 0..12 {
                s.op(
                    t,
                    end,
                    ScenarioOp::Dump {
                        min: [cx, 1, cz],
                        max: [cx + 14, 2, cz],
                        sections: SECTION_ALL,
                    },
                );
            }
            Ok(s)
        }
        "sand-fall" => {
            let mut s = ScenarioSpec::new(EX_CHUNKS, 30);
            ex_floor(&mut s, ids, 4)?;
            let stone = ids.state("stone", &[])?;
            let sand = ids.state("sand", &[])?;
            let air = ids.state("air", &[])?;
            s.op(
                0,
                Phase::Start,
                ScenarioOp::SetBlock {
                    pos: [cx, 6, cz],
                    state: stone,
                    flags: 18,
                },
            );
            s.op(
                0,
                Phase::Start,
                ScenarioOp::SetBlock {
                    pos: [cx, 7, cz],
                    state: sand,
                    flags: 18,
                },
            );
            s.op(
                2,
                Phase::Start,
                ScenarioOp::SetBlock {
                    pos: [cx, 6, cz],
                    state: air,
                    flags: 3,
                },
            );
            for t in 0..30 {
                s.op(
                    t,
                    end,
                    ScenarioOp::Dump {
                        min: [cx, 0, cz],
                        max: [cx, 8, cz],
                        sections: SECTION_BLOCKS | SECTION_TICKS,
                    },
                );
            }
            Ok(s)
        }
        "crop-random-tick" => {
            // Wheat on farmland next to a water trough; a huge random-tick speed makes growth show up quickly.
            let mut s = ScenarioSpec::new(EX_CHUNKS, 60);
            s.rule("random_tick_speed", "3000");
            s.rng = RngPin::Seed(12345);
            s.rand_value = Some(0x1234_5678);
            ex_floor(&mut s, ids, 6)?;
            let farmland = ids.state("farmland", &[("moisture", "7")])?;
            let wheat = ids.state("wheat", &[("age", "0")])?;
            let stone = ids.state("stone", &[])?;
            let water = ids.state("water", &[("level", "0")])?;
            s.op(
                0,
                Phase::Start,
                ScenarioOp::Fill {
                    min: [cx, 1, cz],
                    max: [cx + 1, 1, cz],
                    state: farmland,
                    flags: 18,
                },
            );
            s.op(
                0,
                Phase::Start,
                ScenarioOp::Fill {
                    min: [cx, 2, cz],
                    max: [cx + 1, 2, cz],
                    state: wheat,
                    flags: 18,
                },
            );
            s.op(
                0,
                Phase::Start,
                ScenarioOp::Fill {
                    min: [cx + 2, 1, cz],
                    max: [cx + 2, 1, cz],
                    state: stone,
                    flags: 18,
                },
            );
            s.op(
                0,
                Phase::Start,
                ScenarioOp::SetBlock {
                    pos: [cx - 1, 1, cz],
                    state: stone,
                    flags: 18,
                },
            );
            s.op(
                0,
                Phase::Start,
                ScenarioOp::SetBlock {
                    pos: [cx, 1, cz + 1],
                    state: water,
                    flags: 18,
                },
            );
            s.op(
                0,
                Phase::Start,
                ScenarioOp::SetBlock {
                    pos: [cx, 1, cz - 1],
                    state: stone,
                    flags: 18,
                },
            );
            s.op(
                0,
                Phase::Start,
                ScenarioOp::SetBlock {
                    pos: [cx + 1, 1, cz - 1],
                    state: stone,
                    flags: 18,
                },
            );
            s.op(
                0,
                Phase::Start,
                ScenarioOp::SetBlock {
                    pos: [cx + 1, 1, cz + 1],
                    state: stone,
                    flags: 18,
                },
            );
            s.op(
                0,
                Phase::Start,
                ScenarioOp::SetBlock {
                    pos: [cx, 1, cz + 2],
                    state: stone,
                    flags: 18,
                },
            );
            s.op(
                0,
                Phase::Start,
                ScenarioOp::SetBlock {
                    pos: [cx - 1, 1, cz + 1],
                    state: stone,
                    flags: 18,
                },
            );
            for t in (0..60).step_by(5) {
                s.op(
                    t,
                    end,
                    ScenarioOp::Dump {
                        min: [cx - 1, 1, cz - 1],
                        max: [cx + 2, 2, cz + 2],
                        sections: SECTION_BLOCKS | SECTION_TICKS,
                    },
                );
            }
            Ok(s)
        }
        "ops-smoke" => {
            // Exercises every operation: scheduled block / fluid ticks, a block event, an RNG re-pin, a game rule.
            let mut s = ScenarioSpec::new(EX_CHUNKS, 14);
            ex_floor(&mut s, ids, 6)?;
            let piston = ids.state("piston", &[("facing", "east"), ("extended", "false")])?;
            let stone = ids.state("stone", &[])?;
            let sand = ids.state("sand", &[])?;
            let water = ids.state("water", &[("level", "0")])?;
            let all = SECTION_ALL;
            s.op(
                0,
                Phase::Start,
                ScenarioOp::SetBlock {
                    pos: [cx, 1, cz],
                    state: piston,
                    flags: 18,
                },
            );
            s.op(
                0,
                Phase::Start,
                ScenarioOp::SetBlock {
                    pos: [cx + 1, 1, cz],
                    state: stone,
                    flags: 18,
                },
            );
            s.op(
                0,
                Phase::Start,
                ScenarioOp::SetBlock {
                    pos: [cx + 3, 5, cz + 3],
                    state: sand,
                    flags: 18,
                },
            );
            s.op(
                0,
                Phase::Start,
                ScenarioOp::SetBlock {
                    pos: [cx, 1, cz + 5],
                    state: water,
                    flags: 18,
                },
            );
            let area = (cx - 1, cz - 1, cx + 5, cz + 6);
            let dump = |s: &mut ScenarioSpec, t: i32, ph: Phase| {
                s.op(
                    t,
                    ph,
                    ScenarioOp::Dump {
                        min: [area.0, 1, area.1],
                        max: [area.2, 6, area.3],
                        sections: all,
                    },
                );
            };
            s.op(
                1,
                Phase::Start,
                ScenarioOp::BlockEvent {
                    pos: [cx, 1, cz],
                    block: ids.block("piston")?,
                    a: 0,
                    b: 5,
                },
            );
            dump(&mut s, 1, Phase::Start);
            dump(&mut s, 1, end);
            s.op(
                3,
                Phase::Start,
                ScenarioOp::ScheduleFluid {
                    pos: [cx, 1, cz + 5],
                    fluid: ids.fluid("water")?,
                    delay: 3,
                    priority: -1,
                },
            );
            s.op(
                3,
                Phase::Start,
                ScenarioOp::ScheduleBlock {
                    pos: [cx + 3, 5, cz + 3],
                    block: ids.block("sand")?,
                    delay: 2,
                    priority: 1,
                },
            );
            dump(&mut s, 3, Phase::Start);
            dump(&mut s, 6, end);
            s.op(
                7,
                Phase::Start,
                ScenarioOp::PinRng {
                    rng: RngPin::Seed(777),
                    rand_value: Some(99),
                },
            );
            s.op(
                7,
                Phase::Start,
                ScenarioOp::GameRule {
                    name: "random_tick_speed".into(),
                    value: "5".into(),
                },
            );
            dump(&mut s, 7, Phase::Start);
            dump(&mut s, 7, end);
            Ok(s)
        }
        other => bail!(
            "unknown example '{other}'; have {}",
            SCENARIO_EXAMPLES.join(", ")
        ),
    }
}

/// Print a readable summary of a parsed output (block layers as characters, tick lists, RNG values).
fn print_scenario_summary(ids: &ScenarioIds, out: &ScenarioOutput) {
    println!(
        "{}: {} dumps, status {}{}, {} of {} ticks run",
        out.dimension,
        out.dumps.len(),
        out.status,
        if out.message.is_empty() {
            String::new()
        } else {
            format!(" ({})", out.message)
        },
        out.ticks_run,
        out.planned_ticks
    );
    let mut legend: Vec<(String, char)> = Vec::new();
    for d in &out.dumps {
        println!(
            "-- tick {} {} | gameTime {} | rng {:012x} randValue {:08x} sub {} | level ticks {}/{}",
            d.timeline_tick,
            if d.phase == 0 { "start" } else { "end" },
            d.game_time,
            d.random_state,
            d.rand_value as u32,
            d.sub_tick_count,
            d.level_block_ticks,
            d.level_fluid_ticks
        );
        if let Some(blocks) = &d.blocks {
            let (w, h, dz) = (
                (d.max[0] - d.min[0] + 1) as usize,
                (d.max[1] - d.min[1] + 1) as usize,
                (d.max[2] - d.min[2] + 1) as usize,
            );
            for y in (0..h).rev() {
                for z in 0..dz {
                    let mut line = String::new();
                    for x in 0..w {
                        let s = blocks[(y * dz + z) * w + x];
                        let label = ids.describe(s);
                        let c = if label == "air" {
                            '.'
                        } else if let Some(l) = label
                            .strip_prefix("water[level=")
                            .and_then(|r| r.chars().next())
                        {
                            if l == '0' { 'S' } else { l }
                        } else {
                            match legend.iter().find(|(n, _)| *n == label) {
                                Some((_, c)) => *c,
                                None => {
                                    let c = (b'a' + (legend.len() as u8 % 26)) as char;
                                    legend.push((label.clone(), c));
                                    c
                                }
                            }
                        };
                        line.push(c);
                    }
                    println!(
                        "  y={} z={}: {line}",
                        d.min[1] + y as i32,
                        d.min[2] + z as i32
                    );
                }
            }
        }
        for (kind, ticks) in [("block", &d.block_ticks), ("fluid", &d.fluid_ticks)] {
            if let Some(t) = ticks.as_ref().filter(|t| !t.is_empty()) {
                let shown: Vec<String> = t
                    .iter()
                    .take(6)
                    .map(|k| {
                        format!(
                            "{}@{:?}t{}p{}#{}",
                            k.type_name
                                .strip_prefix("minecraft:")
                                .unwrap_or(&k.type_name),
                            k.pos,
                            k.trigger_tick,
                            k.priority,
                            k.sub_tick_order
                        )
                    })
                    .collect();
                println!(
                    "  {kind} ticks ({}): {}{}",
                    t.len(),
                    shown.join(" "),
                    if t.len() > 6 { " ..." } else { "" }
                );
            }
        }
        if let Some(e) = d.events.as_ref().filter(|e| !e.is_empty()) {
            println!("  events: {e:?}");
        }
        if let Some(be) = d.block_entities.as_ref().filter(|b| !b.is_empty()) {
            println!(
                "  block entities: {}",
                be.iter()
                    .map(|(p, n)| format!("{p:?}:{}B", n.as_ref().map_or(0, Vec::len)))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
    }
    if !legend.is_empty() {
        println!(
            "legend: {}",
            legend
                .iter()
                .map(|(n, c)| format!("{c}={n}"))
                .collect::<Vec<_>>()
                .join("  ")
        );
    }
}

/// `cargo xtask oracle scenario`: run a built-in example (or a scenario file) `runs` times and compare the outputs
/// byte for byte (determinism check with the pinned random sources), then print a summary of the first run.
pub fn scenario_command(
    version: &str,
    example: Option<&str>,
    file: Option<&Path>,
    runs: u32,
    out: Option<&Path>,
    quiet: bool,
) -> anyhow::Result<bool> {
    let ids = ScenarioIds::load(version)?;
    let bytes = match (example, file) {
        (_, Some(f)) => fs::read(f).with_context(|| format!("read {}", f.display()))?,
        (Some(e), None) => scenario_example(e, &ids)?.to_bytes(),
        (None, None) => bail!(
            "give --example <name> ({}) or --file <ZSC1>",
            SCENARIO_EXAMPLES.join(", ")
        ),
    };
    let world = ScenarioWorld::default();
    let mut outputs: Vec<Vec<u8>> = Vec::new();
    for n in 0..runs.max(1) {
        let started = Instant::now();
        let raw = run_scenario(version, &world, &bytes)?;
        println!(
            "run {} of {}: {} bytes in {:.1}s",
            n + 1,
            runs.max(1),
            raw.len(),
            started.elapsed().as_secs_f64()
        );
        outputs.push(raw);
    }
    let parsed = parse_scenario_output(&outputs[0])?;
    if !quiet {
        print_scenario_summary(&ids, &parsed);
    }
    if let Some(p) = out {
        fs::write(p, &outputs[0])?;
    }
    let identical = outputs.iter().all(|o| *o == outputs[0]);
    if !identical {
        for (n, o) in outputs.iter().enumerate().skip(1) {
            if *o != outputs[0] {
                match (parse_scenario_output(&outputs[0]), parse_scenario_output(o)) {
                    (Ok(a), Ok(b)) => println!(
                        "run 1 vs run {}: {}",
                        n + 1,
                        first_scenario_difference(&a, &b)
                    ),
                    _ => println!("run {} could not be parsed", n + 1),
                }
            }
        }
    }
    println!(
        "determinism: {} run(s), outputs {}",
        outputs.len(),
        if identical {
            "byte-identical"
        } else {
            "DIFFER"
        }
    );
    Ok(identical && parsed.status == 0)
}

/// Describe the first difference between two parsed outputs (which dump, which field).
fn first_scenario_difference(a: &ScenarioOutput, b: &ScenarioOutput) -> String {
    if a.dumps.len() != b.dumps.len() {
        return format!("dump counts differ: {} vs {}", a.dumps.len(), b.dumps.len());
    }
    for (x, y) in a.dumps.iter().zip(&b.dumps) {
        let at = format!("dump at tick {} phase {}", x.timeline_tick, x.phase);
        macro_rules! field {
            ($f:ident) => {
                if x.$f != y.$f {
                    return format!(
                        "{at}: {} differs ({:?} vs {:?})",
                        stringify!($f),
                        x.$f,
                        y.$f
                    );
                }
            };
        }
        field!(game_time);
        field!(random_state);
        field!(rand_value);
        field!(sub_tick_count);
        field!(level_block_ticks);
        field!(level_fluid_ticks);
        field!(ticking_chunks);
        if x.blocks != y.blocks {
            let (p, q) = (
                x.blocks.as_deref().unwrap_or(&[]),
                y.blocks.as_deref().unwrap_or(&[]),
            );
            let i = p.iter().zip(q).position(|(m, n)| m != n);
            return format!("{at}: blocks differ (first at index {i:?})");
        }
        field!(block_ticks);
        field!(fluid_ticks);
        field!(events);
        field!(block_entities);
    }
    "no field differs (status/message/ticks_run?)".to_owned()
}
