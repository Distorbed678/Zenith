//! Exclusive world `session.lock` handling and safe world directory deletion.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use crate::error::{Result, StorageError};
use crate::fs_util::create_dir_all_resolving_symlinks;

/// Exact 3-byte contents of `session.lock`: UTF-8 U+2603 SNOWMAN (`☃`).
pub const SESSION_LOCK_BYTES: [u8; 3] = [0xE2, 0x98, 0x83];

/// Number of world deletion retry attempts.
pub const DELETE_WORLD_RETRIES: usize = 5;

/// Delay between world deletion retry attempts (500 ms).
pub const DELETE_WORLD_RETRY_DELAY: Duration = Duration::from_millis(500);

/// An acquired exclusive lock on `<world>/session.lock`.
///
/// Releasing or dropping the lock unlocks and closes the OS file handle while leaving
/// `session.lock` on disk.
#[derive(Debug)]
pub struct SessionLock {
    path: PathBuf,
    file: Option<File>,
}

impl SessionLock {
    /// Acquires `<world_dir>/session.lock`:
    /// 1. Safely creates `<world_dir>` if missing (resolving symlinks first).
    /// 2. Opens `<world_dir>/session.lock` for create + write (without truncating).
    /// 3. Writes `E2 98 83` (`☃`) at offset 0.
    /// 4. Calls `fsync` (data + metadata) **before** acquiring the OS lock.
    /// 5. Acquires a non-blocking exclusive OS file lock on the whole file.
    pub fn acquire(world_dir: &Path) -> Result<Self> {
        create_dir_all_resolving_symlinks(world_dir)?;
        let lock_path = world_dir.join("session.lock");

        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)?;

        file.seek(SeekFrom::Start(0))?;
        file.write_all(&SESSION_LOCK_BYTES)?;
        // fsync data + metadata BEFORE acquiring the OS lock (per spec).
        file.sync_all()?;

        match file.try_lock() {
            Ok(()) => Ok(Self {
                path: lock_path,
                file: Some(file),
            }),
            Err(TryLockError::WouldBlock) => {
                drop(file);
                Err(StorageError::LockHeld(lock_path))
            }
            Err(TryLockError::Error(err)) => {
                drop(file);
                Err(StorageError::Io(err))
            }
        }
    }

    /// Non-destructively probes whether `<world_dir>/session.lock` is currently locked:
    /// - Opens write-only (**does not create**).
    /// - `NotFound` → `Ok(false)` (unlocked).
    /// - `PermissionDenied` → `Ok(true)` (locked).
    /// - Otherwise tries a non-blocking exclusive lock; if acquired, immediately unlocks and
    ///   closes, returning `Ok(false)`.
    pub fn is_locked(world_dir: &Path) -> Result<bool> {
        let lock_path = world_dir.join("session.lock");
        let file = match OpenOptions::new().write(true).open(&lock_path) {
            Ok(f) => f,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(err) if err.kind() == io::ErrorKind::PermissionDenied => return Ok(true),
            Err(err) => return Err(StorageError::Io(err)),
        };

        match file.try_lock() {
            Ok(()) => {
                let _ = file.unlock();
                drop(file);
                Ok(false)
            }
            Err(TryLockError::WouldBlock) => {
                drop(file);
                Ok(true)
            }
            Err(TryLockError::Error(err)) => {
                drop(file);
                Err(StorageError::Io(err))
            }
        }
    }

    /// Path to the locked `session.lock` file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns `true` if this lock handle is still active and valid.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.file.is_some()
    }

    /// Fails with [`StorageError::LockInvalidated`] if this lock has been released/invalidated.
    pub fn check_valid(&self) -> Result<()> {
        if self.is_valid() {
            Ok(())
        } else {
            Err(StorageError::LockInvalidated(self.path.clone()))
        }
    }

    /// Releases the lock and closes the file handle, leaving `session.lock` on disk.
    pub fn release(&mut self) -> Result<()> {
        if let Some(file) = self.file.take() {
            let unlock_res = file.unlock();
            drop(file);
            if let Err(err) = unlock_res {
                return Err(StorageError::Io(err));
            }
        }
        Ok(())
    }
}

impl Drop for SessionLock {
    fn drop(&mut self) {
        if let Some(file) = self.file.take() {
            let _ = file.unlock();
            drop(file);
        }
    }
}

/// Deletes a world directory following the spec's retry and lock-ordering rules:
/// - Up to 5 attempts with 500 ms sleep between failures.
/// - Skips `session.lock` while deleting all other entries in `<world>/`.
/// - Releases/closes `lock` (if held) and deletes `session.lock` only during post-visit of
///   `<world>/` itself before removing `<world>/`.
pub fn delete_world_dir(world_dir: &Path, lock: Option<SessionLock>) -> Result<()> {
    delete_world_dir_with_retry_delay(world_dir, lock, DELETE_WORLD_RETRY_DELAY)
}

/// Same as [`delete_world_dir`] with a configurable retry delay (useful for tests).
pub fn delete_world_dir_with_retry_delay(
    world_dir: &Path,
    mut lock: Option<SessionLock>,
    retry_delay: Duration,
) -> Result<()> {
    let lock_path = world_dir.join("session.lock");
    let mut last_err = None;

    for attempt in 1..=DELETE_WORLD_RETRIES {
        match delete_world_once(world_dir, &lock_path, &mut lock) {
            Ok(()) => return Ok(()),
            Err(err) => {
                log::warn!("World delete attempt {attempt}/{DELETE_WORLD_RETRIES} failed: {err}");
                last_err = Some(err);
                if attempt < DELETE_WORLD_RETRIES && !retry_delay.is_zero() {
                    thread::sleep(retry_delay);
                }
            }
        }
    }

    Err(StorageError::Io(last_err.unwrap_or_else(|| {
        io::Error::other("world deletion failed")
    })))
}

fn delete_world_once(
    world_dir: &Path,
    lock_path: &Path,
    lock: &mut Option<SessionLock>,
) -> io::Result<()> {
    if !world_dir.exists() {
        return Ok(());
    }
    delete_children_skipping_lock(world_dir, lock_path)?;

    // Post-visit of `<world>/`: release lock, delete `session.lock`, then remove `<world>/`.
    if let Some(mut held) = lock.take() {
        let _ = held.release();
    }
    match fs::remove_file(lock_path) {
        Ok(()) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(err),
    }
    fs::remove_dir(world_dir)
}

fn delete_children_skipping_lock(dir: &Path, lock_path: &Path) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path == lock_path {
            continue;
        }
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            delete_children_skipping_lock(&path, lock_path)?;
            fs::remove_dir(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}
