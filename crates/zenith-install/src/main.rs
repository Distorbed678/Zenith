//! `zenith-install`: list game installs and run the local ownership proof.

use std::process::ExitCode;

use clap::{Parser, Subcommand};
use zenith_install::{CheckStatus, ProofOptions, discover_installs, prove_ownership};

#[derive(Parser)]
#[command(
    name = "zenith-install",
    about = "Find your game install and prove you own it"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List every install Zenith can see.
    List,
    /// Run the ownership proof for one version.
    Check {
        #[arg(long, default_value = "26.3")]
        version: String,
        /// Don't contact Mojang; use cached or built-in hashes.
        #[arg(long)]
        offline: bool,
        /// Print the report as JSON.
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    match Cli::parse().cmd {
        Cmd::List => {
            let installs = discover_installs(None);
            if installs.is_empty() {
                println!("No installs found.");
            }
            for i in installs {
                println!(
                    "{:<8} {:<18} {} instance(s)  {}",
                    i.version,
                    i.launcher.label(),
                    i.game_dirs.len(),
                    i.client_jar.display()
                );
            }
            ExitCode::SUCCESS
        }
        Cmd::Check {
            version,
            offline,
            json,
        } => {
            let report = prove_ownership(&ProofOptions { version, offline });
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).expect("serializable")
                );
            } else {
                println!("Ownership proof for {}", report.version);
                for c in &report.checks {
                    let mark = match c.status {
                        CheckStatus::Pass => "pass",
                        CheckStatus::Fail => "FAIL",
                        CheckStatus::Skipped => "skip",
                    };
                    println!("  [{mark}] {:<32} {}", c.title, c.detail);
                }
                println!(
                    "{}",
                    if report.passed {
                        "Result: verified"
                    } else {
                        "Result: NOT verified"
                    }
                );
            }
            if report.passed {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
    }
}
