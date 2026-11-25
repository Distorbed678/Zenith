//! Repo automation. Run with `cargo xtask <command>`.

mod guard;
mod oracle;
mod parity;
mod parity_blocks;
mod parity_structures;
mod parity_terrain;
mod parity_text;
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
    /// M7 stage 1: noise-router fields, aquifer functions and chunk volumes vs the game's.
    WorldgenRouter {
        #[arg(long, default_value = "26.3")]
        version: String,
        #[arg(long)]
        write: bool,
    },
    /// M7 stage 2: climate target points, biome sources and biome zoom vs the game's.
    WorldgenBiomes {
        #[arg(long, default_value = "26.3")]
        version: String,
        #[arg(long)]
        write: bool,
    },
    /// M7 stage 3: base column heights (4 heightmaps) and block-state IDs vs the game's.
    WorldgenColumns {
        #[arg(long, default_value = "26.3")]
        version: String,
        #[arg(long)]
        write: bool,
    },
    /// M7 stage 4: full `terrain`-status chunks (blocks, biomes, 6 heightmaps, post-processing, ticks) vs the game's.
    GoldenTerrain {
        #[arg(long, default_value = "26.3")]
        version: String,
        #[arg(long)]
        write: bool,
    },
    /// M8: `structure_starts` & `structure_references` across Overworld, Nether, End vs the game's.
    StructureStarts {
        #[arg(long, default_value = "26.3")]
        version: String,
        #[arg(long)]
        write: bool,
    },
    /// Text components: decode, JSON + NBT re-encode and plain-string flattening vs the game's own.
    TextComponent {
        #[arg(long, default_value = "26.3")]
        version: String,
        #[arg(long)]
        write: bool,
    },
    /// Stage 1: Zenith's JSON layer vs the game's parsers (jar JSON + edge cases) and primitive codecs.
    DatapackLoad {
        #[arg(long, default_value = "26.3")]
        version: String,
        #[arg(long)]
        write: bool,
    },
    /// Compare zenith-math's Java-exact helpers with the game's own on a fixed probe list.
    MathHelpers {
        #[arg(long, default_value = "26.3")]
        version: String,
        #[arg(long)]
        write: bool,
    },
    /// Zenith copies real vanilla region files; vanilla reads both and compares chunks and sector layouts.
    AnvilIo {
        #[arg(long, default_value = "26.3")]
        version: String,
        #[arg(long)]
        worlds: Vec<PathBuf>,
        #[arg(long)]
        write: bool,
    },
    /// Compute every block-state ID from the spec rule and compare with vanilla's report.
    BlockstateIds {
        #[arg(long, default_value = "26.3")]
        version: String,
        #[arg(long)]
        write: bool,
    },
    /// Every static fact and shape of every block state (35,723 in 26.3) vs the game's own (ADR 0009).
    BlockFacts {
        #[arg(long, default_value = "26.3")]
        version: String,
        #[arg(long)]
        write: bool,
        /// How many failing states to print.
        #[arg(long, default_value_t = 30)]
        limit: usize,
        /// Only print failures whose state description contains this text (e.g. `minecraft:oak_stairs`).
        #[arg(long)]
        only: Option<String>,
    },
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
    /// Generate chunks in a fresh vanilla world and print a summary of the ZTC1 dump (oracle self-check).
    Chunks {
        #[arg(long, default_value = "26.3")]
        version: String,
        #[arg(long, default_value_t = 12345)]
        seed: i64,
        #[arg(long, default_value = "minecraft:normal")]
        level_type: String,
        #[arg(long)]
        structures: bool,
        /// `dimension:x:z`, e.g. `minecraft:overworld:200:200` (repeatable).
        #[arg(long = "chunk", required = true)]
        chunks: Vec<String>,
        #[arg(long, default_value = "terrain")]
        status: String,
    },
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
            cmd: ParityCmd::WorldgenRouter { version, write },
        } => parity_terrain::worldgen_router(&version, write),
        Cmd::Parity {
            cmd: ParityCmd::WorldgenBiomes { version, write },
        } => parity_terrain::worldgen_biomes(&version, write),
        Cmd::Parity {
            cmd: ParityCmd::WorldgenColumns { version, write },
        } => parity_terrain::worldgen_columns(&version, write),
        Cmd::Parity {
            cmd: ParityCmd::GoldenTerrain { version, write },
        } => parity_terrain::golden_terrain(&version, write),
        Cmd::Parity {
            cmd: ParityCmd::StructureStarts { version, write },
        } => parity_structures::structure_starts(&version, write),
        Cmd::Parity {
            cmd: ParityCmd::TextComponent { version, write },
        } => parity_text::text_component(&version, write),
        Cmd::Parity {
            cmd: ParityCmd::DatapackLoad { version, write },
        } => parity::datapack_load(&version, write),
        Cmd::Parity {
            cmd: ParityCmd::MathHelpers { version, write },
        } => parity::math_helpers(&version, write),
        Cmd::Parity {
            cmd:
                ParityCmd::AnvilIo {
                    version,
                    worlds,
                    write,
                },
        } => parity::anvil_io(&version, &worlds, write),
        Cmd::Parity {
            cmd: ParityCmd::BlockstateIds { version, write },
        } => parity::blockstate_ids(&version, write),
        Cmd::Parity {
            cmd:
                ParityCmd::BlockFacts {
                    version,
                    write,
                    limit,
                    only,
                },
        } => parity_blocks::block_facts(&version, write, limit, only.as_deref()),
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
                OracleCmd::Chunks {
                    version,
                    seed,
                    level_type,
                    structures,
                    chunks,
                    status,
                },
        } => oracle::chunks_command(&version, seed, level_type, structures, &chunks, &status),
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
