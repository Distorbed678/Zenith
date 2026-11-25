//! Safe 3-phase delete-then-move file replacement protocol.
//!
//! Used for `level.dat` saves, `level.dat_old` restoration, and player `.dat` saves.
//! Mirrors the exact 3-phase protocol (10 immediate retries per phase, plain move with
//! explicit prior deletion, optional rollback on install failure).

use std::fs;
use std::io;
use std::path::Path;

use crate::error::{Result, StorageError};
use crate::fs_util::{is_regular_file, path_exists_no_follow};

/// Maximum number of immediate retry attempts per phase.
pub const SAFE_REPLACE_MAX_RETRIES: usize = 10;

/// Deletes `path` if any directory entry exists at `path`.
fn delete_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

/// Plain move with no overwrite flag: fails with `AlreadyExists` if `dst` still exists.
fn move_no_overwrite(src: &Path, dst: &Path) -> io::Result<()> {
    if path_exists_no_follow(dst) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("destination already exists: {}", dst.display()),
        ));
    }
    fs::rename(src, dst)
}

fn run_phase<F>(
    target: &Path,
    phase: &'static str,
    mut attempt: F,
) -> std::result::Result<(), StorageError>
where
    F: FnMut() -> io::Result<()>,
{
    let mut last_err = None;
    for _ in 0..SAFE_REPLACE_MAX_RETRIES {
        match attempt() {
            Ok(()) => return Ok(()),
            Err(err) => last_err = Some(err),
        }
    }
    Err(StorageError::SafeReplaceFailed {
        target: target.to_path_buf(),
        phase,
        source: last_err,
    })
}

/// Executes the 3-phase safe file replacement protocol:
///
/// 1. **Create backup** *(only if `target` currently exists)* — up to 10 immediate retries:
///    delete `backup` if present, move `target -> backup`, verify `backup` is a regular file.
/// 2. **Ensure target is removed** — up to 10 immediate retries:
///    delete `target` if present, verify `target` does not exist.
/// 3. **Install new file** — up to 10 immediate retries:
///    move `new_file -> target`, verify `target` is a regular file.
///    If all 10 attempts fail and `no_rollback == false`, attempts up to 10 retries to move
///    `backup -> target` and verify `target` is a regular file before returning failure.
pub fn safe_replace_file(
    target: &Path,
    new_file: &Path,
    backup: &Path,
    no_rollback: bool,
) -> Result<()> {
    // Phase 1: Create backup (only if target currently exists).
    if path_exists_no_follow(target) {
        run_phase(target, "backup", || {
            delete_if_exists(backup)?;
            move_no_overwrite(target, backup)?;
            if !is_regular_file(backup) {
                return Err(io::Error::other("backup is not a regular file after move"));
            }
            Ok(())
        })?;
    }

    // Phase 2: Ensure target is removed.
    run_phase(target, "remove_target", || {
        delete_if_exists(target)?;
        if path_exists_no_follow(target) {
            return Err(io::Error::other("target still exists after delete"));
        }
        Ok(())
    })?;

    // Phase 3: Install new file.
    if let Err(err) = run_phase(target, "install", || {
        move_no_overwrite(new_file, target)?;
        if !is_regular_file(target) {
            return Err(io::Error::other("target is not a regular file after move"));
        }
        Ok(())
    }) {
        if !no_rollback {
            let rollback_res = run_phase(target, "rollback", || {
                move_no_overwrite(backup, target)?;
                if !is_regular_file(target) {
                    return Err(io::Error::other(
                        "target is not a regular file after rollback",
                    ));
                }
                Ok(())
            });
            if let Err(rb_err) = rollback_res {
                log::warn!("Rollback failed during safe file replacement: {rb_err}");
            }
        }
        return Err(err);
    }

    Ok(())
}
