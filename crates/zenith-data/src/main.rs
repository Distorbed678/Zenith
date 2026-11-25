//! `zenith-data`: ingest the game's data generator output and verify it.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use zenith_data::ingest::{IngestOptions, ingest};
use zenith_data::{Tables, verify, version_dir};

#[derive(Parser)]
#[command(
    name = "zenith-data",
    about = "Build Zenith's game tables from your own install"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Prove ownership, run the data generator, and compile the tables (reuses the cache when the jar is unchanged).
    Ingest {
        #[arg(long, default_value = "26.3")]
        version: String,
        #[arg(long)]
        offline: bool,
        #[arg(long)]
        force: bool,
    },
    /// Run the `datagen-ingest` parity suite against the cached tables.
    Verify {
        #[arg(long, default_value = "26.3")]
        version: String,
        /// Write `datagen-ingest.json` into this directory (e.g. `progress/parity`).
        #[arg(long)]
        results_dir: Option<PathBuf>,
    },
    /// Summarize the cached tables.
    Info {
        #[arg(long, default_value = "26.3")]
        version: String,
    },
}

fn run() -> anyhow::Result<bool> {
    match Cli::parse().cmd {
        Cmd::Ingest {
            version,
            offline,
            force,
        } => {
            let o = ingest(&IngestOptions {
                version,
                offline,
                force,
            })?;
            let t = &o.tables;
            println!(
                "{} tables for {} in {:.1}s: {} blocks, {} states, {} registries, {} packets",
                if o.reused_cache { "Reused" } else { "Built" },
                t.meta.version,
                o.seconds,
                t.blocks.len(),
                t.states.len(),
                t.registries.len(),
                t.packets.len()
            );
            if let Some(j) = o.java {
                println!("  Java {} from {}", j.major, j.source);
            }
            println!("  {}", o.tables_path.display());
            Ok(true)
        }
        Cmd::Verify {
            version,
            results_dir,
        } => {
            let (r, c) = verify::run(&version, results_dir.as_deref())?;
            println!(
                "datagen-ingest {}: {}/{} checks passed",
                r.version, r.passed, r.total
            );
            for f in &c.failures {
                println!("  FAIL {f}");
            }
            Ok(r.passed == r.total)
        }
        Cmd::Info { version } => {
            let t = Tables::load(&version_dir(&version).join("tables.bin"))?;
            println!(
                "{} (world {}, protocol {}), jar {}",
                t.meta.version, t.meta.world_version, t.meta.protocol, t.meta.jar_sha1
            );
            println!("ingested {}", t.meta.ingested_at);
            println!(
                "{} blocks, {} states, {} registries, {} packets",
                t.blocks.len(),
                t.states.len(),
                t.registries.len(),
                t.packets.len()
            );
            Ok(true)
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}
