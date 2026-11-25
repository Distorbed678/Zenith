//! Error types for `zenith-nbt`.

use thiserror::Error;

use crate::TagType;

/// Top-level error type for NBT binary I/O, limits, SNBT, and serialization.
#[derive(Debug, Error)]
pub enum Error {
    /// Structural or encoding violation in NBT binary data.
    #[error("NBT format error: {0}")]
    Format(#[from] FormatError),

    /// Byte-quota or nesting-depth limit exceeded while reading NBT data.
    #[error("NBT accounting error: {0}")]
    Accounting(#[from] AccountingError),

    /// Underlying I/O error from a reader or writer.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A string's Java modified UTF-8 encoding exceeds the 65,535-byte wire limit.
    #[error(
        "string modified UTF-8 encoding is {encoded_len} bytes, exceeding the 65535-byte limit"
    )]
    StringTooLong {
        /// Encoded length in bytes.
        encoded_len: usize,
    },

    /// A [`Compound`](crate::Compound) reached Java's treeified-bin threshold
    /// (capacity >= 64 and > 8 colliding entries in a single bucket), whose
    /// red-black tree iteration order is not yet reproduced.
    #[error(
        "compound bin reached {bucket_len} colliding entries at capacity {capacity}; \
         treeified-bin iteration order is not yet supported"
    )]
    UnsupportedTreeifiedBin {
        /// Number of entries in the colliding bucket.
        bucket_len: usize,
        /// Current bucket table capacity.
        capacity: usize,
    },

    /// SNBT syntax or evaluation error.
    #[error("SNBT error: {0}")]
    Snbt(#[from] SnbtError),

    /// Serde serialization or deserialization error.
    #[cfg(feature = "serde")]
    #[error("serde error: {0}")]
    Serde(String),
}

/// Binary NBT format violations.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FormatError {
    /// Encountered a tag type ID outside `0..=12`.
    #[error("unknown NBT tag type ID: {0}")]
    UnknownTagType(u8),

    /// Encountered a negative length prefix for a list or typed array.
    #[error("negative NBT collection length: {0}")]
    NegativeLength(i32),

    /// Encountered a list with element type `End` (0) and positive length.
    #[error("non-empty list cannot have element type End (length {0})")]
    NonEmptyEndList(i32),

    /// Invalid Java modified UTF-8 byte sequence.
    #[error("invalid modified UTF-8: {0}")]
    InvalidModifiedUtf8(&'static str),

    /// Attempted to encode `Tag::End` in a position where it is illegal
    /// (e.g. as a compound entry value or list element).
    #[error("tag type {0:?} is not valid in this position")]
    InvalidTagPlacement(TagType),

    /// Root or expected tag had a different type than requested.
    #[error("expected tag type {expected:?}, found {found:?}")]
    UnexpectedTagType {
        /// Expected tag type.
        expected: TagType,
        /// Actual tag type found.
        found: TagType,
    },
}

/// Read-side quota and nesting-depth violations.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AccountingError {
    /// Reading the next item would exceed the configured byte quota.
    #[error("NBT byte quota exceeded: used {used}, requested {requested}, quota {quota}")]
    QuotaExceeded {
        /// Bytes already charged.
        used: u64,
        /// Additional bytes requested by the current item.
        requested: u64,
        /// Maximum allowed byte quota.
        quota: u64,
    },

    /// Nesting of compounds and lists exceeded the maximum allowed depth.
    #[error("NBT nesting depth exceeded maximum of {max_depth}")]
    MaxDepthExceeded {
        /// Maximum allowed nesting depth.
        max_depth: usize,
    },
}

/// SNBT parse or formatting error.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message} at byte {offset}")]
pub struct SnbtError {
    /// Byte offset into the input string where the error occurred.
    pub offset: usize,
    /// Human-readable description of the error.
    pub message: String,
}

impl SnbtError {
    /// Creates a new SNBT error at the given byte offset.
    pub fn new(offset: usize, message: impl Into<String>) -> Self {
        Self {
            offset,
            message: message.into(),
        }
    }
}
