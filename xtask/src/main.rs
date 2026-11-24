//! Repo automation. Run with `cargo xtask <command>`.

mod guard;
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
