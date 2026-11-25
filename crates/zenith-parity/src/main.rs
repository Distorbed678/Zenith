//! `zenith-parity`: inspect and diff `ZPD1` dumps.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use zenith_parity::diff::render;
use zenith_parity::{DumpReader, diff_files};

#[derive(Parser)]
#[command(name = "zenith-parity", about = "Inspect and compare parity dumps")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print a dump's header and records.
    Show {
        file: PathBuf,
        /// Stop after this many records.
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Find the first divergence between two dumps.
    Diff { left: PathBuf, right: PathBuf },
}

fn run() -> anyhow::Result<bool> {
    match Cli::parse().cmd {
        Cmd::Show { file, limit } => {
            let r = DumpReader::open(&file)?;
            let h = r.header.clone();
            println!(
                "{} dump of {} (scenario {:?}, seed {})",
                h.source, h.version, h.scenario, h.seed
            );
            for rec in r.take(limit) {
                println!("  {}", render(&rec?));
            }
            Ok(true)
        }
        Cmd::Diff { left, right } => {
            let report = diff_files(&left, &right)?;
            match report.divergence {
                None => {
                    println!("identical: {} records", report.compared);
                    Ok(true)
                }
                Some(d) => {
                    println!("DIVERGENCE at record {}: {}", d.index, d.what);
                    for c in &d.context {
                        println!("    {}", render(c));
                    }
                    println!(
                        "  left:  {}",
                        d.left
                            .as_ref()
                            .map(render)
                            .unwrap_or_else(|| "<end>".into())
                    );
                    println!(
                        "  right: {}",
                        d.right
                            .as_ref()
                            .map(render)
                            .unwrap_or_else(|| "<end>".into())
                    );
                    Ok(false)
                }
            }
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
