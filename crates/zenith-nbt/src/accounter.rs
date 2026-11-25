//! Read-side byte quota and nesting-depth accounting for NBT decoding.

use crate::error::{AccountingError, Error};

/// Default read quota (2 MiB) used for network payloads and most reads.
pub const DEFAULT_QUOTA: u64 = 2_097_152;

/// Quota (100 MiB) used for uncompressed file reads.
pub const UNCOMPRESSED_QUOTA: u64 = 104_857_600;

/// Default maximum nesting depth for compounds and lists.
pub const DEFAULT_MAX_DEPTH: usize = 512;

/// In-memory size charges per NBT element as specified in `docs/specs/nbt/binary-format.md`.
pub(crate) mod charges {
    pub const END: u64 = 8;
    pub const BYTE: u64 = 9;
    pub const SHORT: u64 = 10;
    pub const INT: u64 = 12;
    pub const FLOAT: u64 = 12;
    pub const LONG: u64 = 16;
    pub const DOUBLE: u64 = 16;
    pub const STRING_BASE: u64 = 36;
    pub const BYTE_ARRAY_BASE: u64 = 24;
    pub const INT_ARRAY_BASE: u64 = 24;
    pub const LONG_ARRAY_BASE: u64 = 24;
    pub const LIST_BASE: u64 = 36;
    pub const LIST_PER_ELEM: u64 = 4;
    pub const COMPOUND_BASE: u64 = 48;
    pub const COMPOUND_KEY_BASE: u64 = 28;
    pub const COMPOUND_NEW_KEY_ENTRY: u64 = 36;
    pub const UTF16_UNIT: u64 = 2;
}

/// Tracks estimated in-memory byte usage and compound/list nesting depth while decoding NBT.
#[derive(Debug, Clone)]
pub struct Accounter {
    quota: u64,
    used: u64,
    depth: usize,
    max_depth: usize,
}

impl Accounter {
    /// Creates an accounter with the given byte quota and maximum nesting depth.
    #[must_use]
    pub const fn new(quota: u64, max_depth: usize) -> Self {
        Self {
            quota,
            used: 0,
            depth: 0,
            max_depth,
        }
    }

    /// Creates an accounter with [`DEFAULT_QUOTA`] (2 MiB) and [`DEFAULT_MAX_DEPTH`] (512).
    #[must_use]
    pub const fn default_quota() -> Self {
        Self::new(DEFAULT_QUOTA, DEFAULT_MAX_DEPTH)
    }

    /// Creates an accounter with [`UNCOMPRESSED_QUOTA`] (100 MiB) and [`DEFAULT_MAX_DEPTH`] (512).
    #[must_use]
    pub const fn uncompressed() -> Self {
        Self::new(UNCOMPRESSED_QUOTA, DEFAULT_MAX_DEPTH)
    }

    /// Creates an accounter with an unlimited byte quota (`u64::MAX`) and [`DEFAULT_MAX_DEPTH`] (512).
    #[must_use]
    pub const fn unlimited() -> Self {
        Self::new(u64::MAX, DEFAULT_MAX_DEPTH)
    }

    /// Returns the configured byte quota.
    #[must_use]
    pub const fn quota(&self) -> u64 {
        self.quota
    }

    /// Returns the number of bytes charged so far.
    #[must_use]
    pub const fn used(&self) -> u64 {
        self.used
    }

    /// Returns the current nesting depth.
    #[must_use]
    pub const fn depth(&self) -> usize {
        self.depth
    }

    /// Returns the maximum allowed nesting depth.
    #[must_use]
    pub const fn max_depth(&self) -> usize {
        self.max_depth
    }

    /// Charges `bytes` against the quota.
    ///
    /// Succeeds when `used + bytes <= quota`; fails with [`AccountingError::QuotaExceeded`]
    /// if the addition overflows or exceeds `quota`.
    pub fn charge(&mut self, bytes: u64) -> Result<(), Error> {
        let Some(next) = self.used.checked_add(bytes) else {
            return Err(AccountingError::QuotaExceeded {
                used: self.used,
                requested: bytes,
                quota: self.quota,
            }
            .into());
        };
        if next > self.quota {
            return Err(AccountingError::QuotaExceeded {
                used: self.used,
                requested: bytes,
                quota: self.quota,
            }
            .into());
        }
        self.used = next;
        Ok(())
    }

    /// Charges `count * elem_size` against the quota using overflow-safe arithmetic.
    pub fn charge_many(&mut self, count: u64, elem_size: u64) -> Result<(), Error> {
        let Some(total) = count.checked_mul(elem_size) else {
            return Err(AccountingError::QuotaExceeded {
                used: self.used,
                requested: u64::MAX,
                quota: self.quota,
            }
            .into());
        };
        self.charge(total)
    }

    /// Enters one level of compound or list nesting.
    ///
    /// Fails with [`AccountingError::MaxDepthExceeded`] when entering level `max_depth + 1`.
    pub fn push_depth(&mut self) -> Result<(), Error> {
        let next = self.depth.saturating_add(1);
        if next > self.max_depth {
            return Err(AccountingError::MaxDepthExceeded {
                max_depth: self.max_depth,
            }
            .into());
        }
        self.depth = next;
        Ok(())
    }

    /// Exits one level of compound or list nesting.
    pub fn pop_depth(&mut self) {
        debug_assert!(self.depth > 0, "pop_depth called at depth 0");
        self.depth = self.depth.saturating_sub(1);
    }
}

impl Default for Accounter {
    fn default() -> Self {
        Self::default_quota()
    }
}
