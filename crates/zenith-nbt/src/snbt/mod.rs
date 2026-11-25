//! SNBT (Stringified NBT) single-line printer and 1.21.5+ parser.

pub mod parser;
pub mod printer;
pub mod style;

pub use parser::{parse_snbt, parse_snbt_compound};
pub use printer::{is_bare_key, to_snbt};
