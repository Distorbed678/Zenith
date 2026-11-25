//! Error types for `zenith-storage`.

use std::path::PathBuf;

use thiserror::Error;

/// Convenience result alias for storage operations.
pub type Result<T> = std::result::Result<T, StorageError>;

/// Top-level error type returned by `zenith-storage`.
#[derive(Debug, Error)]
pub enum StorageError {
    /// Underlying filesystem or stream I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// NBT encoding, decoding, quota, or depth error.
    #[error("NBT error: {0}")]
    Nbt(#[from] zenith_nbt::Error),

    /// Invalid `<namespace>:<path>` identifier syntax.
    #[error("invalid identifier: {0:?}")]
    InvalidIdentifier(String),

    /// Resolved identifier or relative path escaped its root directory.
    #[error("path {resolved:?} escapes root directory {root:?}")]
    PathEscape {
        /// Normalized root directory.
        root: PathBuf,
        /// Resolved path that escaped `root`.
        resolved: PathBuf,
    },

    /// Required directory does not exist or is not a directory.
    #[error("required directory does not exist or is not a directory: {0:?}")]
    DirectoryNotFound(PathBuf),

    /// `session.lock` is already held by another session or process.
    #[error("session lock is already held: {0:?}")]
    LockHeld(PathBuf),

    /// Previously acquired `session.lock` is no longer valid.
    #[error("session lock is no longer valid: {0:?}")]
    LockInvalidated(PathBuf),

    /// Compression ID is not supported for writing (e.g., GZip `1` or Custom `127`).
    #[error("compression ID {0} is not supported for writing")]
    UnsupportedCompressionForWrite(u8),

    /// Invalid `region-file-compression` configuration value.
    #[error("invalid region compression setting: {0:?}")]
    InvalidCompressionName(String),

    /// Malformed `lz4-java` `LZ4Block` stream.
    #[error("invalid LZ4Block stream: {0}")]
    Lz4Format(String),

    /// Region sector byte offset exceeded the 32-bit signed byte-offset limit (`>= 2 GiB`).
    #[error("region sector {sector} exceeds the 32-bit signed byte-offset limit (524,288 sectors)")]
    RegionOffsetOverflow {
        /// Starting sector index that overflowed.
        sector: u32,
    },

    /// Unsupported `level.dat` `Data.version` value (expected `19133` or `19132`).
    #[error("unsupported level.dat storage version: {0}")]
    InvalidStorageVersion(i32),

    /// Safe file replacement protocol failed after exhausting retries.
    #[error("safe file replacement failed in phase {phase} for target {target:?}")]
    SafeReplaceFailed {
        /// Target file path being replaced.
        target: PathBuf,
        /// Protocol phase that failed (`backup`, `remove_target`, or `install`).
        phase: &'static str,
        /// Last underlying I/O error observed during the phase, if any.
        #[source]
        source: Option<std::io::Error>,
    },

    /// One or more errors occurred while closing cached region files.
    #[error("{first} (plus {} suppressed close error(s))", suppressed.len())]
    MultiClose {
        /// First error encountered during close.
        first: Box<StorageError>,
        /// Additional errors collected while closing remaining files.
        suppressed: Vec<StorageError>,
    },

    /// Storage worker has shut down and cannot complete the request.
    #[error("storage worker has shut down")]
    WorkerShutdown,
}

impl StorageError {
    /// Returns `true` if this error represents an underlying I/O error (including I/O errors
    /// surfaced while streaming NBT or decompressing a chunk stream).
    #[must_use]
    pub fn is_io(&self) -> bool {
        matches!(self, Self::Io(_) | Self::Nbt(zenith_nbt::Error::Io(_)))
    }
}
