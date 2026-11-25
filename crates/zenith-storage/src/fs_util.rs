//! Internal filesystem, sync-open, timestamp, and temporary-file helpers.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Local;

/// Format string for quarantine timestamps (`uuuu-MM-dd_HH-mm-ss`, local wall-clock time).
pub const QUARANTINE_TIME_FORMAT: &str = "%Y-%m-%d_%H-%M-%S";

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Returns the current Unix epoch time in whole seconds, truncated to signed 32-bit int
/// (`floor(epochMillis / 1000)`).
#[must_use]
pub fn current_epoch_seconds() -> i32 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    (millis / 1000) as i32
}

/// Formats the current local wall-clock time for quarantine filenames (`YYYY-MM-DD_HH-mm-ss`).
#[must_use]
pub fn format_quarantine_timestamp() -> String {
    Local::now().format(QUARANTINE_TIME_FORMAT).to_string()
}

/// Safely creates `dir` (and parents), resolving an existing path through symlinks first
/// so symlinked world or dimension directories succeed cleanly.
pub fn create_dir_all_resolving_symlinks(dir: &Path) -> io::Result<()> {
    match fs::symlink_metadata(dir) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                let target = fs::canonicalize(dir)?;
                if target.is_dir() {
                    return Ok(());
                }
                return fs::create_dir_all(target);
            }
            if meta.is_dir() {
                return Ok(());
            }
            fs::create_dir_all(dir)
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => fs::create_dir_all(dir),
        Err(err) => Err(err),
    }
}

/// Opens a file for read/write, creating it if missing (never truncating).
/// When `sync_writes` is `true`, opens with `O_DSYNC` on Unix.
pub fn open_region_rw(path: &Path, sync_writes: bool) -> io::Result<File> {
    let mut opts = OpenOptions::new();
    opts.read(true).write(true).create(true);
    #[cfg(unix)]
    if sync_writes {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(libc::O_DSYNC);
    }
    #[cfg(not(unix))]
    let _ = sync_writes;
    opts.open(path)
}

/// Opens a file with `O_SYNC | WRITE | CREATE | TRUNCATE_EXISTING` on Unix
/// (plain write/create/truncate on non-Unix).
pub fn open_sync_write(path: &Path) -> io::Result<File> {
    let mut opts = OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(libc::O_SYNC);
    }
    opts.open(path)
}

/// Creates a uniquely named temporary file in `dir` with the given `prefix` and `suffix`.
///
/// When `sync` is `true`, opens with `O_SYNC` (used for `level.dat` and player `.dat` temp files).
/// When `sync` is `false`, opens without `O_DSYNC`/`O_SYNC` (used for external `.mcc` temp files).
pub fn create_temp_file_in(
    dir: &Path,
    prefix: &str,
    suffix: &str,
    sync: bool,
) -> io::Result<(PathBuf, File)> {
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);

    for _ in 0..10_000 {
        let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let stem = nanos
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(seq ^ (u64::from(pid) << 32));
        let name = format!("{prefix}{stem:016x}{suffix}");
        let path = dir.join(name);

        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        if sync {
            use std::os::unix::fs::OpenOptionsExt;
            opts.custom_flags(libc::O_SYNC);
        }
        #[cfg(not(unix))]
        let _ = sync;

        match opts.open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "failed to create unique temporary file after 10,000 attempts",
    ))
}

/// Copies `src` to `dst` (overwriting `dst` if it exists) and copies file permissions
/// and modification time best-effort.
pub fn copy_file_with_attributes(src: &Path, dst: &Path) -> io::Result<()> {
    let meta = fs::metadata(src)?;
    fs::copy(src, dst)?;
    let _ = fs::set_permissions(dst, meta.permissions());
    if let Ok(mtime) = meta.modified()
        && let Ok(file) = OpenOptions::new().write(true).open(dst)
    {
        let _ = file.set_modified(mtime);
    }
    Ok(())
}

/// Returns `true` if `path` exists and is a regular file (follows symlinks).
#[must_use]
pub fn is_regular_file(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file())
}

/// Returns `true` if any directory entry exists at `path` (does not follow symlinks).
#[must_use]
pub fn path_exists_no_follow(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

/// Returns a file's modification time in whole epoch milliseconds, if readable.
#[must_use]
pub fn file_mtime_millis(path: &Path) -> Option<i64> {
    let mtime = fs::metadata(path).ok()?.modified().ok()?;
    match mtime.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_millis()).ok(),
        Err(e) => i64::try_from(e.duration().as_millis()).ok().map(|v| -v),
    }
}
