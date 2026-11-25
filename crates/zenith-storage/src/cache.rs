//! 256-entry LRU cache of open [`RegionFile`] handles with negative caching.

use std::path::{Path, PathBuf};

use zenith_nbt::Compound;

use crate::error::{Result, StorageError};
use crate::fs_util::create_dir_all_resolving_symlinks;
use crate::layout::ChunkPos;
use crate::nbt_io::StreamTagVisitor;
use crate::region::{RegionFile, RegionFileOptions};

/// Default LRU cache capacity per region directory (`256`).
pub const REGION_CACHE_CAPACITY: usize = 256;

/// LRU cache of open `.mca` [`RegionFile`] handles (and negative-cache sentinels for absent
/// `.mca` files) for a single region directory (`region/`, `entities/`, or `poi/`).
#[derive(Debug)]
pub struct RegionCache {
    dir: PathBuf,
    options: RegionFileOptions,
    capacity: usize,
    /// Ordered from most-recently-used (index 0) to least-recently-used (last element).
    /// `None` is a negative-cache sentinel indicating the `.mca` file did not exist on a read/scan.
    entries: Vec<(u64, Option<RegionFile>)>,
}

impl RegionCache {
    /// Creates a new cache for `dir` with the standard 256-entry capacity.
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>, options: RegionFileOptions) -> Self {
        Self::with_capacity(dir, options, REGION_CACHE_CAPACITY)
    }

    /// Creates a new cache for `dir` with a custom LRU capacity (minimum `1`).
    #[must_use]
    pub fn with_capacity(
        dir: impl Into<PathBuf>,
        options: RegionFileOptions,
        capacity: usize,
    ) -> Self {
        let capacity = capacity.max(1);
        Self {
            dir: dir.into(),
            options,
            capacity,
            entries: Vec::with_capacity(capacity),
        }
    }

    /// Directory managed by this cache.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Configured LRU capacity.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Current number of cached entries (including negative-cache sentinels).
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if no entries are currently cached.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Number of currently open region files (excluding negative-cache sentinels).
    #[must_use]
    pub fn open_count(&self) -> usize {
        self.entries.iter().filter(|(_, f)| f.is_some()).count()
    }

    /// Returns `true` if `pos`'s region is currently cached as an absent (negative) sentinel.
    #[must_use]
    pub fn is_negatively_cached(&self, pos: ChunkPos) -> bool {
        let key = pos.region_pos().pack_key();
        self.entries
            .iter()
            .find(|(k, _)| *k == key)
            .is_some_and(|(_, f)| f.is_none())
    }

    /// Looks up or opens the region file containing `pos`.
    ///
    /// - When `existing_only == true` (reads / scans / probes): if the `.mca` file does not exist,
    ///   inserts a negative-cache sentinel (`None`) and returns `Ok(None)` without touching disk
    ///   on subsequent lookups.
    /// - When `existing_only == false` (writes / clears): bypasses any cached negative sentinel,
    ///   creates `dir` safely, opens or creates the `.mca` file, and places it at the MRU slot.
    pub fn get_region_file(
        &mut self,
        pos: ChunkPos,
        existing_only: bool,
    ) -> Result<Option<&mut RegionFile>> {
        let region_pos = pos.region_pos();
        let key = region_pos.pack_key();

        if let Some(idx) = self.entries.iter().position(|(k, _)| *k == key) {
            // Move hit to MRU (front).
            let entry = self.entries.remove(idx);
            self.entries.insert(0, entry);

            if self.entries[0].1.is_some() {
                return Ok(self.entries[0].1.as_mut());
            }

            // Cached negative sentinel.
            if existing_only {
                return Ok(None);
            }

            // Write path: bypass negative sentinel, create dir safely, open/create `.mca`.
            create_dir_all_resolving_symlinks(&self.dir)?;
            let mca_path = self.dir.join(region_pos.filename());
            let region = RegionFile::open(&mca_path, &self.dir, self.options)?;
            self.entries[0].1 = Some(region);
            return Ok(self.entries[0].1.as_mut());
        }

        // Cache miss: evict LRU if at capacity.
        if self.entries.len() >= self.capacity
            && let Some((_, Some(evicted))) = self.entries.pop()
        {
            evicted.close()?;
        }

        let mca_path = self.dir.join(region_pos.filename());
        if existing_only && !mca_path.exists() {
            self.entries.insert(0, (key, None));
            return Ok(None);
        }

        create_dir_all_resolving_symlinks(&self.dir)?;
        let region = RegionFile::open(&mca_path, &self.dir, self.options)?;
        self.entries.insert(0, (key, Some(region)));
        Ok(self.entries[0].1.as_mut())
    }

    /// Reads a chunk from its region file (using negative caching for absent regions).
    pub fn read(&mut self, pos: ChunkPos) -> Result<Option<Compound>> {
        let Some(region) = self.get_region_file(pos, true)? else {
            return Ok(None);
        };
        region.read_chunk(pos)
    }

    /// Writes (`Some(tag)`) or clears (`None`) a chunk in its region file.
    ///
    /// Note: clearing a chunk (`None`) opens/creates the `.mca` file (`existing_only = false`),
    /// leaving a brand-new untouched `.mca` at 0 bytes (spec edge case 11).
    pub fn write(&mut self, pos: ChunkPos, tag: Option<&Compound>) -> Result<()> {
        let region = self
            .get_region_file(pos, false)?
            .expect("existing_only=false always returns Some(RegionFile)");
        match tag {
            Some(compound) => region.write_chunk(pos, compound),
            None => region.clear_chunk(pos),
        }
    }

    /// Streaming-scans a chunk from its region file (using negative caching).
    pub fn scan<V: StreamTagVisitor>(&mut self, pos: ChunkPos, visitor: &mut V) -> Result<bool> {
        let Some(region) = self.get_region_file(pos, true)? else {
            return Ok(false);
        };
        region.scan_chunk(pos, visitor)
    }

    /// Lightweight existence probe for `pos` (using negative caching).
    pub fn has_chunk(&mut self, pos: ChunkPos) -> Result<bool> {
        let Some(region) = self.get_region_file(pos, true)? else {
            return Ok(false);
        };
        Ok(region.has_chunk(pos))
    }

    /// `fsync`s every open region file currently in the cache.
    pub fn flush(&mut self) -> Result<()> {
        for (_, maybe_region) in &mut self.entries {
            if let Some(region) = maybe_region.as_mut() {
                region.flush()?;
            }
        }
        Ok(())
    }

    /// Closes every open region file in the cache, attempting every close even if an earlier one
    /// fails. Collects all errors and returns the first error with any later errors attached.
    pub fn close(mut self) -> Result<()> {
        self.close_inner()
    }

    fn close_inner(&mut self) -> Result<()> {
        let mut errors: Vec<StorageError> = Vec::new();
        for (_, maybe_region) in self.entries.drain(..) {
            if let Some(region) = maybe_region
                && let Err(err) = region.close()
            {
                errors.push(err);
            }
        }
        match errors.len() {
            0 => Ok(()),
            1 => Err(errors.remove(0)),
            _ => {
                let first = Box::new(errors.remove(0));
                Err(StorageError::MultiClose {
                    first,
                    suppressed: errors,
                })
            }
        }
    }
}

impl Drop for RegionCache {
    fn drop(&mut self) {
        let _ = self.close_inner();
    }
}
