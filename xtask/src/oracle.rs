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
    let classes = build.join("classes");
    if classes.exists() {
        fs::remove_dir_all(&classes)?;
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
    let manifest = build.join("MANIFEST.MF");
    fs::write(&manifest, "Premain-Class: dev.zenith.oracle.Agent\n")?;
    let jar = build.join("zenith-oracle.jar");
    let args: Vec<std::ffi::OsString> = vec![
        "-m".into(),
        "jdk.jartool/sun.tools.jar.Main".into(),
        "--create".into(),
        "--file".into(),
        jar.clone().into(),
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
