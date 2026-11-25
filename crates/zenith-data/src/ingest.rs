//! Run the game's own data generator on the user's jar and compile the result.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use anyhow::{Context, bail, ensure};

use zenith_install::jar::{read_jar_version, sha1_file};
use zenith_install::java::{JavaRuntime, find_java};
use zenith_install::libraries::{game_classpath, join_classpath};
use zenith_install::{ProofOptions, prove_ownership};

use crate::reports::Reports;
use crate::tables::{Tables, TablesMeta};
use crate::version_dir;

/// The data generator's entry point inside the game jar (a fact about the jar, found by listing its classes).
const DATAGEN_MAIN: &str = "net.minecraft.data.Main";

#[derive(Debug, Clone)]
pub struct IngestOptions {
    pub version: String,
    pub offline: bool,
    /// Re-run even if a cache for this exact jar exists.
    pub force: bool,
}

#[derive(Debug)]
pub struct IngestOutcome {
    pub tables_path: PathBuf,
    pub reports_dir: PathBuf,
    pub reused_cache: bool,
    pub java: Option<JavaRuntime>,
    pub seconds: f64,
    pub tables: Tables,
}

/// Prove ownership, then build (or reuse) the tables for `opts.version`.
pub fn ingest(opts: &IngestOptions) -> anyhow::Result<IngestOutcome> {
    let started = Instant::now();
    let proof = prove_ownership(&ProofOptions {
        version: opts.version.clone(),
        offline: opts.offline,
    });
    if !proof.passed {
        let failed: Vec<String> = proof
            .checks
            .iter()
            .filter(|c| c.status == zenith_install::CheckStatus::Fail)
            .map(|c| format!("{}: {}", c.title, c.detail))
            .collect();
        bail!(
            "ownership proof failed for {}:\n  {}",
            opts.version,
            failed.join("\n  ")
        );
    }
    let install = proof.install.context("proof passed without an install")?;
    let jar = read_jar_version(&install.client_jar)?;
    let jar_sha1 = sha1_file(&install.client_jar)?;

    let dir = version_dir(&opts.version);
    fs::create_dir_all(&dir)?;
    let tables_path = dir.join("tables.bin");
    let reports_dir = dir.join("reports");
    let stamp = dir.join("jar.sha1");

    if !opts.force
        && fs::read_to_string(&stamp).is_ok_and(|s| s.trim() == jar_sha1)
        && let Ok(tables) = Tables::load(&tables_path)
        && tables.meta.jar_sha1 == jar_sha1
    {
        return Ok(IngestOutcome {
            tables_path,
            reports_dir,
            reused_cache: true,
            java: None,
            seconds: started.elapsed().as_secs_f64(),
            tables,
        });
    }

    let required = jar.java_version.unwrap_or(21);
    let java = find_java(&install, required, jar.java_component.as_deref()).with_context(|| {
        format!(
            "no Java {required}+ runtime found; launch {} once in your launcher so it installs one",
            opts.version
        )
    })?;
    let classpath = game_classpath(&install)?;

    let work = dir.join("datagen-run");
    if work.exists() {
        fs::remove_dir_all(&work)?;
    }
    fs::create_dir_all(&work)?;
    let output = Command::new(&java.java)
        .arg("-cp")
        .arg(join_classpath(&classpath))
        .arg(DATAGEN_MAIN)
        .args(["--reports", "--output", "out"])
        .current_dir(&work)
        .output()
        .with_context(|| format!("start {}", java.java.display()))?;
    let mut log = output.stdout.clone();
    log.extend_from_slice(&output.stderr);
    fs::write(dir.join("ingest.log"), &log)?;
    ensure!(
        output.status.success(),
        "data generator failed ({}); see {}",
        output.status,
        dir.join("ingest.log").display()
    );

    if reports_dir.exists() {
        fs::remove_dir_all(&reports_dir)?;
    }
    fs::rename(work.join("out/reports"), &reports_dir)
        .context("data generator produced no reports")?;
    fs::remove_dir_all(&work)?;

    let reports = Reports::load(&reports_dir)?;
    let meta = TablesMeta {
        version: opts.version.clone(),
        jar_sha1: jar_sha1.clone(),
        world_version: jar.world_version,
        protocol: jar.protocol_version,
        ingested_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
    };
    let tables = Tables::compile(&reports, meta)?;
    tables.save(&tables_path)?;
    fs::write(&stamp, &jar_sha1)?;

    Ok(IngestOutcome {
        tables_path,
        reports_dir,
        reused_cache: false,
        java: Some(java),
        seconds: started.elapsed().as_secs_f64(),
        tables,
    })
}
