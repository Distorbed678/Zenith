//! Clean-room NBT (Named Binary Tag) and SNBT (Stringified NBT) library for Zenith.
//!
//! `zenith-nbt` provides:
//! - **Owned data model:** [`Tag`] covering all 13 NBT tag types, [`NbtString`] for lossless
//!   Java modified UTF-8 strings (including `U+0000`, astral characters, and lone surrogates),
//!   heterogeneous [`List`] with binary wrapper-compound encoding/decoding, and [`Compound`]
//!   reproducing Java 21 `HashMap<String, Tag>` iteration order (including resize split order
//!   and copy-constructor sizing).
//! - **Binary I/O ([`io`]):** named-root (file/region), unnamed-root (network), gzip, and zlib
//!   readers and writers preserving exact IEEE-754 float/double bit patterns.
//! - **Read limits ([`Accounter`]):** byte-quota accounting (`DEFAULT_QUOTA = 2_097_152`,
//!   `UNCOMPRESSED_QUOTA = 104_857_600`, or unlimited) and nesting-depth enforcement (`512`).
//! - **SNBT ([`snbt`]):** plain single-line printer (with formatting constants isolated in
//!   [`snbt::style`]) and a recursive-descent parser for the 1.21.5+ SNBT grammar.
//! - **Optional `serde` integration:** `to_tag` / `from_tag` behind the `serde` feature flag.

pub mod accounter;
pub mod compound;
pub mod error;
pub mod io;
pub mod java_number;
pub mod list;
pub mod mutf8;
#[cfg(feature = "serde")]
pub mod serde;
pub mod snbt;
pub mod tag;

#[cfg(feature = "serde")]
pub use crate::serde::{from_tag, to_tag};
pub use accounter::{Accounter, DEFAULT_MAX_DEPTH, DEFAULT_QUOTA, UNCOMPRESSED_QUOTA};
pub use compound::Compound;
pub use error::{AccountingError, Error, FormatError, SnbtError};
pub use io::{
    from_bytes_named, from_bytes_network, read_gzip_named, read_named, read_named_compound,
    read_network, read_zlib_named, to_bytes_named, to_bytes_network, write_gzip_named, write_named,
    write_network, write_zlib_named,
};
pub use list::List;
pub use mutf8::{AsNbtString, MAX_ENCODED_LEN, NbtString};
pub use snbt::{parse_snbt, parse_snbt_compound, to_snbt, to_snbt_utf16};
pub use tag::{Tag, TagType};
