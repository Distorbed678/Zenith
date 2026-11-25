//! Auxiliary saved data (`<world>/data/<ns>/<path>.dat` and `<dim>/data/<ns>/<path>.dat`).
//!
//! - **Read:** peeks first 2 bytes via a 2-byte pushback buffer; if `(b[1] << 8) | b[0] == 0x8B1F`
//!   (gzip magic `1F 8B`), reads gzip-compressed NBT; otherwise reads uncompressed named-root NBT.
//!   Uses unlimited heap quota. Missing `DataVersion` defaults to `1343`.
//! - **Write:** wraps payload in `{ "data": <payload>, "DataVersion": <version> }`, creates parent
//!   directories, and overwrites the target path directly with `O_SYNC | WRITE | CREATE | TRUNCATE_EXISTING`
//!   (no temp file, no `.dat_old` backup).

use std::fs::File;
use std::io::{BufReader, BufWriter, Cursor, Read, Write};
use std::path::Path;

use flate2::read::GzDecoder;
use zenith_nbt::{Compound, Tag};

use crate::compression::{STREAM_BUFFER_SIZE, new_gzip_encoder};
use crate::error::Result;
use crate::fs_util::{create_dir_all_resolving_symlinks, open_sync_write};
use crate::level::DATA_VERSION_26_3;
use crate::nbt_io::{read_named_compound_unlimited, write_named_compound_fallback};

/// Default `DataVersion` for auxiliary `data/*.dat` files when `"DataVersion"` is absent (`1343`).
pub const SAVED_DATA_DEFAULT_VERSION: i32 = 1_343;

/// Little-endian 16-bit gzip magic (`(b[1] << 8) | b[0] == 0x8B1F`, bytes `1F 8B`).
pub const GZIP_MAGIC_LE: u16 = 0x8B1F;

/// Decoded auxiliary saved-data file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedDataFile {
    /// Full root NBT compound (typically `{ "data": ..., "DataVersion": ... }`).
    pub root: Compound,
    /// Resolved `DataVersion` (defaults to `1343` if absent).
    pub data_version: i32,
    /// `true` if the file on disk was gzip-compressed (`1F 8B` magic), `false` if uncompressed.
    pub was_gzip: bool,
}

impl SavedDataFile {
    /// Returns the inner `"data"` compound if present.
    #[must_use]
    pub fn data(&self) -> Option<&Compound> {
        self.root.get("data").and_then(Tag::as_compound)
    }
}

/// Reads an auxiliary saved-data file, auto-detecting gzip (`1F 8B`) vs. uncompressed named-root
/// NBT via a 2-byte pushback buffer and using unlimited heap quota.
pub fn read_saved_data(path: &Path) -> Result<SavedDataFile> {
    let mut file = File::open(path)?;
    let mut header = [0u8; 2];
    let mut read_len = 0;
    while read_len < 2 {
        match file.read(&mut header[read_len..]) {
            Ok(0) => break,
            Ok(n) => read_len += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
    }

    let is_gzip =
        read_len == 2 && (((u16::from(header[1]) << 8) | u16::from(header[0])) == GZIP_MAGIC_LE);

    let pushed = Cursor::new(header[..read_len].to_vec()).chain(file);
    let root = if is_gzip {
        let mut decoder = BufReader::with_capacity(STREAM_BUFFER_SIZE, GzDecoder::new(pushed));
        read_named_compound_unlimited(&mut decoder)?
    } else {
        let mut reader = BufReader::with_capacity(STREAM_BUFFER_SIZE, pushed);
        read_named_compound_unlimited(&mut reader)?
    };

    let data_version = root
        .get("DataVersion")
        .and_then(Tag::as_int)
        .unwrap_or(SAVED_DATA_DEFAULT_VERSION);

    Ok(SavedDataFile {
        root,
        data_version,
        was_gzip: is_gzip,
    })
}

/// Writes an auxiliary saved-data file directly to `path` with `O_SYNC | WRITE | CREATE | TRUNCATE_EXISTING`
/// (no temp file and no `.dat_old` backup), wrapping `payload` in
/// `{ "data": <payload>, "DataVersion": data_version }` in gzip format with the
/// oversized-string-to-empty fallback.
pub fn write_saved_data(path: &Path, payload: &Compound, data_version: i32) -> Result<()> {
    if let Some(parent) = path.parent() {
        create_dir_all_resolving_symlinks(parent)?;
    }

    let mut root = Compound::new();
    root.insert("data", Tag::Compound(payload.clone()));
    root.insert("DataVersion", Tag::Int(data_version));

    let file = open_sync_write(path)?;
    let outer = BufWriter::with_capacity(STREAM_BUFFER_SIZE, file);
    let gz = new_gzip_encoder(outer);
    let mut inner = BufWriter::with_capacity(STREAM_BUFFER_SIZE, gz);
    write_named_compound_fallback(&mut inner, &root)?;
    let gz = inner.into_inner().map_err(|e| e.into_error())?;
    let mut outer = gz.finish()?;
    outer.flush()?;
    Ok(())
}

/// Convenience helper calling [`write_saved_data`] with [`DATA_VERSION_26_3`] (`5023`).
pub fn write_saved_data_26_3(path: &Path, payload: &Compound) -> Result<()> {
    write_saved_data(path, payload, DATA_VERSION_26_3)
}
