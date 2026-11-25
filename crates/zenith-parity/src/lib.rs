//! The parity toolchain shared by every suite.
//!
//! - [`dump`]: the `ZPD1` state-dump format, written by both the vanilla oracle agent and Zenith.
//! - [`diff`]: find the *first* divergence between two dumps, with context.
//! - [`results`]: suite result files consumed by the progress tracker.

pub mod diff;
pub mod dump;
pub mod results;

pub use diff::{Divergence, diff_files, diff_records};
pub use dump::{DumpReader, DumpWriter, Header, Phase, Record, Value};
pub use results::SuiteResult;
