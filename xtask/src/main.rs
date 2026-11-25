//! Repo automation. Run with `cargo xtask <command>`.

mod guard;
mod oracle;
mod parity;
mod progress;
mod svg;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "xtask")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Regenerate progress/progress.json, progress/history.json and the README SVG tracker.
    Progress {
        /// Don't write anything; fail if the committed outputs are out of date.
        #[arg(long)]
        check: bool,
    },
    /// Build or run the parity oracle agent inside the user's own vanilla server.
    Oracle {
        #[command(subcommand)]
        cmd: OracleCmd,
    },
    /// Run a file-based parity suite against data vanilla wrote on this machine.
    Parity {
        #[command(subcommand)]
        cmd: ParityCmd,
    },
    /// Scan the repo for Mojang files, decompiled code and trademark misuse.
    Guard {
        /// Scan only files staged for commit (used by the pre-commit hook).
        #[arg(long)]
        staged: bool,
        /// Game version whose files make up the hash blocklist.
        #[arg(long, default_value = "26.3")]
        version: String,
        /// Download the official jar + asset index from Mojang (for CI, where no install exists).
        #[arg(long)]
        fetch: bool,
        /// Skip the hash blocklist and run only the text/name rules.
        #[arg(long)]
        no_hashes: bool,
    },
}

#[derive(Subcommand)]
enum ParityCmd {
    /// Compare zenith-math's noise generators with vanilla's at fixed sample points.
    NoiseSamples {
        #[arg(long, default_value = "26.3")]
        version: String,
        #[arg(long)]
        write: bool,
    },
    /// Compare zenith-math's random sources with vanilla's over a fixed script of operations.
    RngStreams {
        #[arg(long, default_value = "26.3")]
        version: String,
        #[arg(long)]
        write: bool,
    },
    /// Compare Zenith's SNBT text with vanilla's for every corpus item.
    SnbtFormat {
        #[arg(long, default_value = "26.3")]
        version: String,
        #[arg(long)]
        worlds: Vec<PathBuf>,
        #[arg(long)]
        write: bool,
    },
    /// Read + re-write every NBT blob vanilla wrote (jar structures, oracle worlds, your saves) and compare bytes.
    NbtRoundtrip {
        #[arg(long, default_value = "26.3")]
        version: String,
        /// Extra world folders to include.
        #[arg(long)]
        worlds: Vec<PathBuf>,
        /// Write progress/parity/nbt-roundtrip.json.
        #[arg(long)]
        write: bool,
    },
}

#[derive(Subcommand)]
enum OracleCmd {
    /// Compile the agent jar into tools/oracle/build/.
    Build {
        #[arg(long, default_value = "26.3")]
        version: String,
    },
    /// Run the vanilla dedicated server with the agent for N ticks and write a ZPD1 dump.
    Run {
        #[arg(long, default_value = "26.3")]
        version: String,
        #[arg(long, default_value_t = 100)]
        ticks: u32,
        #[arg(long, default_value_t = 12345)]
        seed: i64,
        #[arg(long)]
        out: PathBuf,
        /// Accept the Minecraft EULA for oracle runs (remembered in ~/.config/zenith/eula-accepted).
        #[arg(long)]
        accept_eula: bool,
        #[arg(long, default_value_t = 600)]
        timeout_secs: u64,
    },
}

/// The repository root (the parent of this crate's directory).
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives in the repo")
        .to_path_buf()
}

fn main() -> ExitCode {
    let result = match Cli::parse().cmd {
        Cmd::Progress { check } => progress::run(check),
        Cmd::Parity {
            cmd: ParityCmd::RngStreams { version, write },
        } => parity::rng_streams(&version, write),
        Cmd::Parity {
            cmd: ParityCmd::NoiseSamples { version, write },
        } => parity::noise_samples(&version, write),
        Cmd::Parity {
            cmd:
                ParityCmd::SnbtFormat {
                    version,
                    worlds,
                    write,
                },
        } => parity::snbt_format(&version, &worlds, write),
        Cmd::Parity {
            cmd:
                ParityCmd::NbtRoundtrip {
                    version,
                    worlds,
                    write,
                },
        } => parity::nbt_roundtrip(&version, &worlds, write),
        Cmd::Oracle {
            cmd: OracleCmd::Build { version },
        } => oracle::build(&version).map(|_| true),
        Cmd::Oracle {
            cmd:
                OracleCmd::Run {
                    version,
                    ticks,
                    seed,
                    out,
                    accept_eula,
                    timeout_secs,
                },
        } => oracle::run(&oracle::RunOptions {
            version,
            ticks,
            seed,
            out,
            accept_eula,
            timeout_secs,
        }),
        Cmd::Guard {
            staged,
            version,
            fetch,
            no_hashes,
        } => guard::run(staged, &version, fetch, no_hashes),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}
