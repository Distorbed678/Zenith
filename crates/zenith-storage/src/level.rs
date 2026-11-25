//! `level.dat` / `level.dat_old` persistence, automatic fallback & quarantine, and world summary scanning.

use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use zenith_nbt::{Accounter, Compound, List, Tag};

use crate::compression::STREAM_BUFFER_SIZE;
use crate::error::{Result, StorageError};
use crate::fs_util::{
    create_temp_file_in, file_mtime_millis, format_quarantine_timestamp, is_regular_file,
};
use crate::layout::WorldLayout;
use crate::nbt_io::{
    SkippingCompoundCollector, read_named_compound_level, scan_named_stream,
    write_gzip_wrapped_compound_fallback,
};
use crate::safe_replace::safe_replace_file;
use crate::session_lock::SessionLock;

/// `Data.version` for Anvil worlds (`19133`).
pub const LEVEL_STORAGE_VERSION_ANVIL: i32 = 19_133;

/// `Data.version` for legacy McRegion worlds (`19132`, requires manual conversion).
pub const LEVEL_STORAGE_VERSION_MCREGION: i32 = 19_132;

/// 26.3 `DataVersion` / `Version.Id` (`5023`).
pub const DATA_VERSION_26_3: i32 = 5_023;

/// Uncompressed NBT byte quota enforced on `level.dat`, `level.dat_old`, and world summaries (100 MiB).
pub const LEVEL_NBT_QUOTA: u64 = zenith_nbt::UNCOMPRESSED_QUOTA;

/// Usable disk space warning threshold (64 MiB).
pub const LOW_DISK_SPACE_THRESHOLD: u64 = 67_108_864;

/// Appends `data_version` to `data["version_history"]` (List of Int) if absent, empty, or
/// different from its last element.
pub fn append_version_history(data: &mut Compound, data_version: i32) {
    let mut history = match data.remove("version_history") {
        Some(Tag::List(list)) => list,
        _ => List::new(),
    };
    let should_append = history
        .last()
        .and_then(Tag::as_int)
        .is_none_or(|last| last != data_version);
    if should_append {
        history.push(Tag::Int(data_version));
    }
    data.insert("version_history", Tag::List(history));
}

/// Writes `level.dat` using the spec's exact sequence:
/// 1. Create temp file `level<random>.dat` in `<world>/` opened with `O_SYNC`.
/// 2. Write gzip-compressed `{ "Data": <data> }` with oversized-string-to-empty fallback.
/// 3. Run [`safe_replace_file`] with `(level.dat, temp, level.dat_old, no_rollback = false)`.
pub fn write_level_dat(layout: &WorldLayout, data: &Compound) -> Result<()> {
    let (temp_path, temp_file) = create_temp_file_in(layout.root(), "level", ".dat", true)?;

    if let Err(err) = write_gzip_wrapped_compound_fallback(temp_file, "Data", data) {
        let _ = fs::remove_file(&temp_path);
        return Err(err);
    }

    safe_replace_file(
        &layout.level_dat(),
        &temp_path,
        &layout.level_dat_old(),
        false,
    )
}

/// Same as [`write_level_dat`], catching and logging any error without rethrowing (spec line 339).
pub fn save_level_dat(layout: &WorldLayout, data: &Compound) {
    if let Err(err) = write_level_dat(layout, data) {
        log::error!("Failed to save level.dat in {:?}: {err}", layout.root());
    }
}

/// Reads a single gzip-compressed `level.dat`-format file with the 100 MiB quota and extracts
/// `root["Data"]` (or an empty [`Compound`] if `"Data"` is absent).
pub fn read_level_dat_file(path: &Path) -> Result<Compound> {
    let file = File::open(path)?;
    let mut decoder = BufReader::with_capacity(STREAM_BUFFER_SIZE, GzDecoder::new(file));
    let mut root = read_named_compound_level(&mut decoder)?;
    match root.remove("Data") {
        Some(Tag::Compound(data)) => Ok(data),
        _ => Ok(Compound::new()),
    }
}

/// Direct read of `<world>/level.dat` after validating `session_lock`.
pub fn read_level_dat(layout: &WorldLayout, session_lock: &SessionLock) -> Result<Compound> {
    session_lock.check_valid()?;
    read_level_dat_file(&layout.level_dat())
}

/// Outcome of [`read_level_dat_with_fallback`].
#[derive(Debug, Clone)]
pub struct LevelFallbackLoad {
    /// Loaded `Data` compound.
    pub data: Compound,
    /// `true` if `level.dat` failed and `level.dat_old` was loaded and promoted.
    pub restored_from_old: bool,
    /// Path to `level.dat_corrupted_<ts>` when a broken `level.dat` was quarantined.
    pub quarantined_path: Option<PathBuf>,
}

/// Automatic fallback read of `level.dat` with `level.dat_old` restoration and quarantine naming:
/// 1. Validates `session_lock`.
/// 2. Attempts direct read of `level.dat` (100 MiB quota).
/// 3. On I/O or NBT error:
///    - Logs warning for `level.dat` and info for `level.dat_old`.
///    - Reads `level.dat_old` (100 MiB quota); propagates error if this also fails.
///    - Restores `level.dat` from `level.dat_old` via [`safe_replace_file`] with
///      `(level.dat, level.dat_old, level.dat_corrupted_<YYYY-MM-DD_HH-mm-ss>, no_rollback = true)`.
///      Because `level.dat_old` is moved onto `level.dat`, `level.dat_old` no longer exists until
///      the next save.
pub fn read_level_dat_with_fallback(
    layout: &WorldLayout,
    session_lock: &SessionLock,
) -> Result<LevelFallbackLoad> {
    read_level_dat_with_fallback_at(layout, session_lock, &format_quarantine_timestamp())
}

/// Same as [`read_level_dat_with_fallback`] with an explicit quarantine timestamp string
/// (format `YYYY-MM-DD_HH-mm-ss`, useful for deterministic tests).
pub fn read_level_dat_with_fallback_at(
    layout: &WorldLayout,
    session_lock: &SessionLock,
    quarantine_timestamp: &str,
) -> Result<LevelFallbackLoad> {
    session_lock.check_valid()?;

    let level_path = layout.level_dat();
    match read_level_dat_file(&level_path) {
        Ok(data) => Ok(LevelFallbackLoad {
            data,
            restored_from_old: false,
            quarantined_path: None,
        }),
        Err(primary_err) => {
            log::warn!("Failed to read level.dat ({level_path:?}): {primary_err}");
            let old_path = layout.level_dat_old();
            log::info!("Attempting fallback read from level.dat_old ({old_path:?})");

            let data = read_level_dat_file(&old_path)?;
            let quarantine_path = layout.level_dat_corrupted(quarantine_timestamp);

            safe_replace_file(&level_path, &old_path, &quarantine_path, true)?;

            let quarantined_path = if quarantine_path.exists() {
                Some(quarantine_path)
            } else {
                None
            };

            Ok(LevelFallbackLoad {
                data,
                restored_from_old: true,
                quarantined_path,
            })
        }
    }
}

// --- World discovery & summary scanning ---

/// Allowlist policy for symlinked `level.dat` files (default: disallow all symlinks).
#[derive(Debug, Clone, Default)]
pub struct AllowedSymlinks {
    allowed_prefixes: Vec<PathBuf>,
}

impl AllowedSymlinks {
    /// Disallows all symlinks (vanilla default when `allowed_symlinks.txt` is absent/empty).
    #[must_use]
    pub fn disallow_all() -> Self {
        Self::default()
    }

    /// Creates an allowlist permitting symlinks whose canonical targets live under any listed prefix.
    #[must_use]
    pub fn with_allowed_prefixes(prefixes: Vec<PathBuf>) -> Self {
        Self {
            allowed_prefixes: prefixes,
        }
    }

    /// Returns `true` if the symlink at `link_path` is permitted.
    #[must_use]
    pub fn is_allowed(&self, link_path: &Path) -> bool {
        if self.allowed_prefixes.is_empty() {
            return false;
        }
        let Ok(canon) = fs::canonicalize(link_path) else {
            return false;
        };
        self.allowed_prefixes
            .iter()
            .any(|prefix| fs::canonicalize(prefix).is_ok_and(|p| canon.starts_with(p)))
    }
}

/// Summary produced when scanning a single candidate world directory.
#[derive(Debug, Clone)]
pub enum WorldSummary {
    /// Successfully parsed `level.dat` summary (`Data.Player` and `Data.WorldGenSettings` skipped).
    Valid {
        /// Directory name under the saves folder.
        dir_name: String,
        /// `Data.LastPlayed` (epoch milliseconds; defaults to `0` if absent).
        last_played: i64,
        /// `Data.version` (`19133` Anvil or `19132` McRegion).
        storage_version: i32,
        /// `true` when `storage_version == 19132` (McRegion).
        requires_manual_conversion: bool,
        /// `Data.DataVersion` if present.
        data_version: Option<i32>,
        /// Skipped-field `Data` compound.
        data: Compound,
    },
    /// `level.dat` was missing, unreadable, non-compound root, had an invalid storage version,
    /// or failed NBT parsing (never falls back to `level.dat_old`).
    Corrupted {
        /// Directory name under the saves folder.
        dir_name: String,
        /// Fallback timestamp: `mtime(level.dat)`, else `mtime(level.dat_old)`, else `-1`.
        last_played: i64,
        /// Human-readable reason.
        reason: String,
    },
    /// `level.dat` is a symlink rejected by [`AllowedSymlinks`].
    SymlinkWarning {
        /// Directory name under the saves folder.
        dir_name: String,
        /// Link target path if readable.
        target: Option<PathBuf>,
    },
}

impl WorldSummary {
    /// Directory name of this world summary.
    #[must_use]
    pub fn dir_name(&self) -> &str {
        match self {
            Self::Valid { dir_name, .. }
            | Self::Corrupted { dir_name, .. }
            | Self::SymlinkWarning { dir_name, .. } => dir_name,
        }
    }

    /// Timestamp used for sorting summaries (`LastPlayed`, fallback mtime, or `-1`).
    #[must_use]
    pub const fn last_played(&self) -> i64 {
        match self {
            Self::Valid { last_played, .. } | Self::Corrupted { last_played, .. } => *last_played,
            Self::SymlinkWarning { .. } => -1,
        }
    }
}

/// Returns `true` iff `world_dir` is a world candidate (either `level.dat` or `level.dat_old`
/// exists as a regular file, or `level.dat` is a symlink).
#[must_use]
pub fn is_world_candidate(world_dir: &Path) -> bool {
    let layout = WorldLayout::new(world_dir);
    let level = layout.level_dat();
    let old = layout.level_dat_old();
    let level_is_symlink = fs::symlink_metadata(&level).is_ok_and(|m| m.file_type().is_symlink());
    level_is_symlink || is_regular_file(&level) || is_regular_file(&old)
}

/// Scans a single world directory for its [`WorldSummary`]:
/// - Checks symlink policy on `level.dat`.
/// - Inspects **only `level.dat`** (never falls back to `level.dat_old`).
/// - Streams via a skip visitor that skips `Data.Player` and `Data.WorldGenSettings` while
///   enforcing the 100 MiB uncompressed quota.
/// - Accepts `Data.version` `19133` (Anvil) and `19132` (McRegion); any other value yields `Corrupted`.
#[must_use]
pub fn scan_world_summary(world_dir: &Path, allowed_symlinks: &AllowedSymlinks) -> WorldSummary {
    let dir_name = world_dir
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let layout = WorldLayout::new(world_dir);
    let level_path = layout.level_dat();
    let old_path = layout.level_dat_old();

    if let Ok(meta) = fs::symlink_metadata(&level_path)
        && meta.file_type().is_symlink()
        && !allowed_symlinks.is_allowed(&level_path)
    {
        return WorldSummary::SymlinkWarning {
            dir_name,
            target: fs::read_link(&level_path).ok(),
        };
    }

    let fallback_mtime = file_mtime_millis(&level_path)
        .or_else(|| file_mtime_millis(&old_path))
        .unwrap_or(-1);

    match read_level_summary_data(&level_path) {
        Ok(data) => {
            let storage_version = data.get("version").and_then(Tag::as_int).unwrap_or(0);
            let requires_manual_conversion = match storage_version {
                LEVEL_STORAGE_VERSION_ANVIL => false,
                LEVEL_STORAGE_VERSION_MCREGION => true,
                other => {
                    return WorldSummary::Corrupted {
                        dir_name,
                        last_played: fallback_mtime,
                        reason: StorageError::InvalidStorageVersion(other).to_string(),
                    };
                }
            };
            let last_played = data.get("LastPlayed").and_then(Tag::as_long).unwrap_or(0);
            let data_version = data.get("DataVersion").and_then(Tag::as_int);
            WorldSummary::Valid {
                dir_name,
                last_played,
                storage_version,
                requires_manual_conversion,
                data_version,
                data,
            }
        }
        Err(err) => WorldSummary::Corrupted {
            dir_name,
            last_played: fallback_mtime,
            reason: err.to_string(),
        },
    }
}

fn read_level_summary_data(level_path: &Path) -> Result<Compound> {
    let file = File::open(level_path)?;
    let mut decoder = BufReader::with_capacity(STREAM_BUFFER_SIZE, GzDecoder::new(file));
    let mut acc = Accounter::uncompressed();
    let mut collector =
        SkippingCompoundCollector::new(&[&["Data", "Player"], &["Data", "WorldGenSettings"]]);
    scan_named_stream(&mut decoder, &mut acc, &mut collector)?;
    let root = collector
        .into_root()
        .ok_or(StorageError::Nbt(zenith_nbt::Error::Format(
            zenith_nbt::FormatError::UnexpectedTagType {
                expected: zenith_nbt::TagType::Compound,
                found: zenith_nbt::TagType::End,
            },
        )))?
        .into_compound()?;
    match root.get("Data") {
        Some(Tag::Compound(c)) => Ok(c.clone()),
        _ => Ok(Compound::new()),
    }
}

/// Discovers candidate world directories directly under `saves_dir`, scans their summaries, and
/// sorts them descending by `LastPlayed`, tie-broken ascending lexicographically by directory name.
pub fn list_world_summaries(
    saves_dir: &Path,
    allowed_symlinks: &AllowedSymlinks,
) -> Result<Vec<WorldSummary>> {
    let mut summaries = Vec::new();
    for entry in fs::read_dir(saves_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if is_world_candidate(&path) {
            summaries.push(scan_world_summary(&path, allowed_symlinks));
        }
    }
    summaries.sort_by(|a, b| {
        b.last_played()
            .cmp(&a.last_played())
            .then_with(|| a.dir_name().cmp(b.dir_name()))
    });
    Ok(summaries)
}
