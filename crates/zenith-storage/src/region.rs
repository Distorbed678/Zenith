//! Anvil region file (`.mca`) and external oversized chunk (`.mcc`) container.

use std::fs::{self, File};
use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use zenith_nbt::Compound;

use crate::compression::{
    CompressionId, EXTERNAL_STREAM_FLAG, compress_chunk_payload, open_chunk_decompressor,
};
use crate::error::{Result, StorageError};
use crate::fs_util::{create_temp_file_in, current_epoch_seconds, is_regular_file, open_region_rw};
use crate::layout::{ChunkPos, RegionPos};
use crate::nbt_io::{
    StreamTagVisitor, read_named_compound_unlimited, scan_named_stream,
    write_named_compound_fallback,
};

/// Size of one Anvil region sector in bytes (`4,096`).
pub const SECTOR_SIZE: usize = 4_096;

/// Size of the Anvil region file header in bytes (`8,192` = 2 sectors).
pub const HEADER_SIZE: usize = 8_192;

/// Number of chunk slots per region (`32 × 32 = 1,024`).
pub const CHUNKS_PER_REGION: usize = 1_024;

/// Chunk record header size in bytes (4-byte BE length + 1-byte version).
pub const CHUNK_HEADER_SIZE: usize = 5;

/// Sector count threshold at or above which a chunk spills into an external `c.<x>.<z>.mcc` file.
///
/// Triggered when `sectors_needed >= 256`, i.e., `data_size >= 1,044,481` bytes
/// (`compressed_payload >= 1,044,476` bytes).
pub const EXTERNAL_SECTOR_THRESHOLD: usize = 256;

/// Maximum internal record size in bytes (`255 * 4096 = 1,044,480`).
pub const MAX_INTERNAL_RECORD_BYTES: usize = (EXTERNAL_SECTOR_THRESHOLD - 1) * SECTOR_SIZE;

/// Exclusive upper bound on sector indices under vanilla's 32-bit signed byte-offset limit
/// (`524,288 * 4,096 = 2 GiB`).
pub const MAX_VANILLA_SECTOR_NUMBER: u32 = 524_288;

/// Options controlling [`RegionFile`] open and write behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionFileOptions {
    /// Open `.mca` files with `O_DSYNC` (`sync-chunk-writes=true`, dedicated-server default).
    pub sync_writes: bool,
    /// Compression scheme used when writing new chunk records (default: [`CompressionId::Zlib`]).
    pub compression: CompressionId,
}

impl Default for RegionFileOptions {
    fn default() -> Self {
        Self {
            sync_writes: true,
            compression: CompressionId::Zlib,
        }
    }
}

/// First-fit sector allocation bitset tracking occupied 4,096-byte sectors.
#[derive(Debug, Clone)]
pub struct SectorBitset {
    words: Vec<u64>,
}

impl SectorBitset {
    /// Creates a new sector bitset with sectors `0` and `1` (the header) permanently marked occupied.
    #[must_use]
    pub fn new_with_header() -> Self {
        let mut bs = Self { words: vec![0] };
        bs.force(0, 2);
        bs
    }

    /// Marks `[start, start + count)` as occupied.
    pub fn force(&mut self, start: usize, count: usize) {
        if count == 0 {
            return;
        }
        let end = start + count;
        let needed_words = end.div_ceil(64);
        if self.words.len() < needed_words {
            self.words.resize(needed_words, 0);
        }
        for bit in start..end {
            self.words[bit / 64] |= 1u64 << (bit % 64);
        }
    }

    /// Marks `[start, start + count)` as free.
    pub fn free(&mut self, start: usize, count: usize) {
        let end = start + count;
        for bit in start..end {
            let word_idx = bit / 64;
            if word_idx < self.words.len() {
                self.words[word_idx] &= !(1u64 << (bit % 64));
            }
        }
    }

    /// Returns `true` if `sector` is currently marked occupied.
    #[must_use]
    pub fn is_used(&self, sector: usize) -> bool {
        let word_idx = sector / 64;
        if word_idx >= self.words.len() {
            return false;
        }
        (self.words[word_idx] & (1u64 << (sector % 64))) != 0
    }

    fn next_clear_bit(&self, from: usize) -> usize {
        let mut word_idx = from / 64;
        let mut bit_offset = from % 64;
        while word_idx < self.words.len() {
            // Invert so clear bits become 1s, and mask off bits below `bit_offset`.
            let inverted = (!self.words[word_idx]) & (!0u64 << bit_offset);
            if inverted != 0 {
                return word_idx * 64 + inverted.trailing_zeros() as usize;
            }
            word_idx += 1;
            bit_offset = 0;
        }
        from.max(self.words.len() * 64)
    }

    fn next_set_bit(&self, from: usize) -> Option<usize> {
        let mut word_idx = from / 64;
        let mut bit_offset = from % 64;
        while word_idx < self.words.len() {
            let masked = self.words[word_idx] & (!0u64 << bit_offset);
            if masked != 0 {
                return Some(word_idx * 64 + masked.trailing_zeros() as usize);
            }
            word_idx += 1;
            bit_offset = 0;
        }
        None
    }

    /// First-fit allocation: finds the lowest-index contiguous run of at least `count` clear
    /// sectors starting from sector `0` (effectively `>= 2`), marks them occupied, and returns
    /// the starting sector index.
    pub fn reserve(&mut self, count: usize) -> usize {
        debug_assert!(count > 0);
        let mut idx = 0usize;
        loop {
            let free_start = self.next_clear_bit(idx);
            match self.next_set_bit(free_start) {
                None => {
                    self.force(free_start, count);
                    return free_start;
                }
                Some(next_set) => {
                    if next_set - free_start >= count {
                        self.force(free_start, count);
                        return free_start;
                    }
                    idx = next_set + 1;
                }
            }
        }
    }
}

/// An open Anvil region file (`r.<rx>.<rz>.mca`) paired with its external directory for
/// `c.<x>.<z>.mcc` oversized chunks.
#[derive(Debug)]
pub struct RegionFile {
    path: PathBuf,
    external_dir: PathBuf,
    options: RegionFileOptions,
    file: Option<File>,
    offsets: [u32; CHUNKS_PER_REGION],
    timestamps: [i32; CHUNKS_PER_REGION],
    used_sectors: SectorBitset,
}

impl RegionFile {
    /// Opens or creates a region file at `path`, storing external `.mcc` chunks in `external_dir`.
    ///
    /// - Verifies `external_dir` exists and is a directory.
    /// - Does **not** write an 8,192-byte header when opening a brand-new 0-byte file.
    /// - Recovers truncated headers and invalid slots in memory only (persisted on next write/clear).
    pub fn open(path: &Path, external_dir: &Path, options: RegionFileOptions) -> Result<Self> {
        if !external_dir.is_dir() {
            return Err(StorageError::DirectoryNotFound(external_dir.to_path_buf()));
        }

        let mut file = open_region_rw(path, options.sync_writes)?;
        let mut offsets = [0u32; CHUNKS_PER_REGION];
        let mut timestamps = [0i32; CHUNKS_PER_REGION];
        let mut used_sectors = SectorBitset::new_with_header();

        let mut header = [0u8; HEADER_SIZE];
        let bytes_read = read_at_or_eof(&mut file, 0, &mut header)?;

        if bytes_read > 0 {
            if bytes_read < HEADER_SIZE {
                log::warn!(
                    "Region file {:?} has truncated header: read {bytes_read} of {HEADER_SIZE} bytes",
                    path
                );
            }

            let file_size = file.metadata()?.len();

            for i in 0..CHUNKS_PER_REGION {
                let off_idx = i * 4;
                let entry = u32::from_be_bytes([
                    header[off_idx],
                    header[off_idx + 1],
                    header[off_idx + 2],
                    header[off_idx + 3],
                ]);
                let ts_idx = SECTOR_SIZE + i * 4;
                timestamps[i] = i32::from_be_bytes([
                    header[ts_idx],
                    header[ts_idx + 1],
                    header[ts_idx + 2],
                    header[ts_idx + 3],
                ]);

                if entry == 0 {
                    continue;
                }

                let sector_number = (entry >> 8) & 0x00FF_FFFF;
                let num_sectors = entry & 0xFF;

                if sector_number < 2 {
                    log::warn!(
                        "Region file {:?} slot {i} overlaps header (sector {sector_number}); zeroing in memory",
                        path
                    );
                    offsets[i] = 0;
                } else if num_sectors == 0 {
                    log::warn!(
                        "Region file {:?} slot {i} has zero sector count; zeroing in memory",
                        path
                    );
                    offsets[i] = 0;
                } else if u64::from(sector_number) * (SECTOR_SIZE as u64) > file_size {
                    log::warn!(
                        "Region file {:?} slot {i} starts past EOF (sector {sector_number}, file_size {file_size}); zeroing in memory",
                        path
                    );
                    offsets[i] = 0;
                } else {
                    offsets[i] = entry;
                    used_sectors.force(sector_number as usize, num_sectors as usize);
                }
            }
        }

        Ok(Self {
            path: path.to_path_buf(),
            external_dir: external_dir.to_path_buf(),
            options,
            file: Some(file),
            offsets,
            timestamps,
            used_sectors,
        })
    }

    /// Convenience helper: opens `dir/r.<rx>.<rz>.mca` with `external_dir = dir`.
    pub fn open_in_dir(dir: &Path, pos: RegionPos, options: RegionFileOptions) -> Result<Self> {
        let path = dir.join(pos.filename());
        Self::open(&path, dir, options)
    }

    /// Path to the `.mca` file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Directory where external `c.<x>.<z>.mcc` files are stored.
    #[must_use]
    pub fn external_dir(&self) -> &Path {
        &self.external_dir
    }

    /// Configured write compression scheme.
    #[must_use]
    pub const fn compression(&self) -> CompressionId {
        self.options.compression
    }

    /// Returns the raw 32-bit location entry for `pos` (`0` if absent).
    #[must_use]
    pub fn location_entry(&self, pos: ChunkPos) -> u32 {
        self.offsets[pos.header_index()]
    }

    /// Returns the stored `(sector_number, sector_count)` for `pos`, or `None` if absent.
    #[must_use]
    pub fn chunk_sectors(&self, pos: ChunkPos) -> Option<(u32, u8)> {
        let entry = self.location_entry(pos);
        if entry == 0 {
            None
        } else {
            Some(((entry >> 8) & 0x00FF_FFFF, (entry & 0xFF) as u8))
        }
    }

    /// Returns the stored 32-bit epoch-second timestamp for `pos`.
    #[must_use]
    pub fn timestamp(&self, pos: ChunkPos) -> i32 {
        self.timestamps[pos.header_index()]
    }

    /// Returns the path of the external `.mcc` file for `pos`.
    #[must_use]
    pub fn external_chunk_path(&self, pos: ChunkPos) -> PathBuf {
        self.external_dir.join(pos.external_filename())
    }

    /// Returns a reference to the in-memory sector allocation bitset.
    #[must_use]
    pub fn used_sectors(&self) -> &SectorBitset {
        &self.used_sectors
    }

    /// Lightweight existence probe (used by region upgraders).
    ///
    /// Reads only 5 bytes at `sectorNumber * 4096` and mirrors vanilla's exact rules:
    /// - External (`(versionByte & 0x80) != 0`): `true` iff `(versionByte & 0x7F)` is in
    ///   `{1, 2, 3, 4, 127}` and `c.<x>.<z>.mcc` is a regular file (does NOT check `length`).
    /// - Internal: `true` iff `versionByte` is in `{1, 2, 3, 4, 127}`, `length != 0`, and
    ///   `0 <= (length - 1) <= 4096 * numSectors` (5-byte upper-bound slack; file length past
    ///   byte 5 is not checked).
    pub fn has_chunk(&mut self, pos: ChunkPos) -> bool {
        let entry = self.offsets[pos.header_index()];
        if entry == 0 {
            return false;
        }
        let sector_number = (entry >> 8) & 0x00FF_FFFF;
        let num_sectors = entry & 0xFF;
        if sector_number >= MAX_VANILLA_SECTOR_NUMBER {
            return false;
        }
        let Some(file) = self.file.as_mut() else {
            return false;
        };

        let mut hdr = [0u8; CHUNK_HEADER_SIZE];
        let offset = u64::from(sector_number) * (SECTOR_SIZE as u64);
        let Ok(read_n) = read_at_or_eof(file, offset, &mut hdr) else {
            return false;
        };
        if read_n < CHUNK_HEADER_SIZE {
            return false;
        }

        let length = i32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]);
        let version_byte = hdr[4];

        if (version_byte & EXTERNAL_STREAM_FLAG) != 0 {
            let version_id = version_byte & 0x7F;
            CompressionId::from_id(version_id).is_some()
                && is_regular_file(&self.external_chunk_path(pos))
        } else {
            if CompressionId::from_id(version_byte).is_none() || length == 0 {
                return false;
            }
            let stream_len = i64::from(length) - 1;
            let max_len = i64::from(num_sectors) * (SECTOR_SIZE as i64);
            stream_len >= 0 && stream_len <= max_len
        }
    }

    /// Opens the decompressed NBT byte stream for `pos`, returning `Ok(None)` when the chunk
    /// is missing or recoverable-as-missing per the spec.
    pub fn open_chunk_read_stream(&mut self, pos: ChunkPos) -> Result<Option<Box<dyn Read>>> {
        let entry = self.offsets[pos.header_index()];
        if entry == 0 {
            return Ok(None);
        }
        let sector_number = (entry >> 8) & 0x00FF_FFFF;
        let num_sectors = entry & 0xFF;
        if sector_number >= MAX_VANILLA_SECTOR_NUMBER {
            return Err(StorageError::RegionOffsetOverflow {
                sector: sector_number,
            });
        }

        let file = self
            .file
            .as_mut()
            .ok_or_else(|| io::Error::other("region file is closed"))?;

        let mut buf = vec![0u8; (num_sectors as usize) * SECTOR_SIZE];
        let offset = u64::from(sector_number) * (SECTOR_SIZE as u64);
        let actual_bytes_read = read_at_or_eof(file, offset, &mut buf)?;

        if actual_bytes_read < CHUNK_HEADER_SIZE {
            log::error!(
                "Region file {:?} chunk ({}, {}): truncated chunk header ({actual_bytes_read} bytes)",
                self.path,
                pos.x,
                pos.z
            );
            return Ok(None);
        }

        let length = i32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let version_byte = buf[4];

        // Checked BEFORE bit 7 (edge case 8).
        if length == 0 {
            log::warn!(
                "Region file {:?} chunk ({}, {}): allocated sector with length 0 (stream missing)",
                self.path,
                pos.x,
                pos.z
            );
            return Ok(None);
        }

        let stream_length = i64::from(length) - 1;

        let (version_id, raw_stream): (u8, Box<dyn Read>) = if (version_byte & EXTERNAL_STREAM_FLAG)
            != 0
        {
            if stream_length != 0 {
                log::warn!(
                    "Region file {:?} chunk ({}, {}): has both internal (stream_length={stream_length}) and external streams; continuing with external file",
                    self.path,
                    pos.x,
                    pos.z
                );
            }
            let version_id = version_byte & 0x7F;
            let mcc_path = self.external_chunk_path(pos);
            if !is_regular_file(&mcc_path) {
                log::error!(
                    "Region file {:?} chunk ({}, {}): missing external file {:?}",
                    self.path,
                    pos.x,
                    pos.z,
                    mcc_path
                );
                return Ok(None);
            }
            let mcc_file = File::open(&mcc_path)?;
            (version_id, Box::new(mcc_file))
        } else {
            let avail = (actual_bytes_read - CHUNK_HEADER_SIZE) as i64;
            if stream_length > avail {
                log::error!(
                    "Region file {:?} chunk ({}, {}): truncated internal stream (declared {stream_length}, available {avail})",
                    self.path,
                    pos.x,
                    pos.z
                );
                return Ok(None);
            }
            if stream_length < 0 {
                log::error!(
                    "Region file {:?} chunk ({}, {}): negative declared stream length ({stream_length})",
                    self.path,
                    pos.x,
                    pos.z
                );
                return Ok(None);
            }
            let end = CHUNK_HEADER_SIZE + (stream_length as usize);
            let payload = buf[CHUNK_HEADER_SIZE..end].to_vec();
            (version_byte, Box::new(Cursor::new(payload)))
        };

        let Some(compression) = CompressionId::from_id(version_id) else {
            log::error!(
                "Region file {:?} chunk ({}, {}): unknown compression version ID {version_id}",
                self.path,
                pos.x,
                pos.z
            );
            return Ok(None);
        };

        open_chunk_decompressor(compression, raw_stream)
    }

    /// Reads and decodes a chunk's named-root NBT compound (unlimited heap quota, 512 depth).
    pub fn read_chunk(&mut self, pos: ChunkPos) -> Result<Option<Compound>> {
        let Some(mut stream) = self.open_chunk_read_stream(pos)? else {
            return Ok(None);
        };
        let compound = read_named_compound_unlimited(&mut stream)?;
        Ok(Some(compound))
    }

    /// Streams a chunk's NBT directly into `visitor` with early-termination support.
    ///
    /// Returns `Ok(true)` if the chunk existed and was visited, or `Ok(false)` if missing.
    pub fn scan_chunk<V: StreamTagVisitor>(
        &mut self,
        pos: ChunkPos,
        visitor: &mut V,
    ) -> Result<bool> {
        let Some(mut stream) = self.open_chunk_read_stream(pos)? else {
            return Ok(false);
        };
        let mut acc = zenith_nbt::Accounter::unlimited();
        scan_named_stream(&mut stream, &mut acc, visitor)?;
        Ok(true)
    }

    /// Writes a chunk's NBT compound using the configured compression scheme and current epoch time.
    pub fn write_chunk(&mut self, pos: ChunkPos, compound: &Compound) -> Result<()> {
        self.write_chunk_with_timestamp(pos, compound, current_epoch_seconds())
    }

    /// Writes a chunk's NBT compound using an explicit epoch-second `timestamp`.
    pub fn write_chunk_with_timestamp(
        &mut self,
        pos: ChunkPos,
        compound: &Compound,
        timestamp: i32,
    ) -> Result<()> {
        let compression = self.options.compression;
        if !compression.is_writable() {
            return Err(StorageError::UnsupportedCompressionForWrite(
                compression.id(),
            ));
        }

        // Prefix with 5-byte chunk header placeholder: [len_be: 4][version_id: 1].
        let mut record = Vec::with_capacity(CHUNK_HEADER_SIZE + 512);
        record.extend_from_slice(&[0, 0, 0, 0, compression.id()]);

        compress_chunk_payload(compression, &mut record, |w| {
            write_named_compound_fallback(w, compound)
        })?;

        self.write_prepared_record(pos, compression.id(), &mut record, timestamp)
    }

    /// Writes a pre-compressed chunk payload (for low-level / exact-cutoff / custom tests).
    ///
    /// `version_id` must be writable unless called for testing; `payload` is the raw compressed
    /// stream (without the 5-byte header).
    pub fn write_raw_compressed_chunk(
        &mut self,
        pos: ChunkPos,
        version_id: u8,
        payload: &[u8],
        timestamp: i32,
    ) -> Result<()> {
        let mut record = Vec::with_capacity(CHUNK_HEADER_SIZE + payload.len());
        record.extend_from_slice(&[0, 0, 0, 0, version_id]);
        record.extend_from_slice(payload);
        self.write_prepared_record(pos, version_id, &mut record, timestamp)
    }

    fn write_prepared_record(
        &mut self,
        pos: ChunkPos,
        version_id: u8,
        record: &mut [u8],
        timestamp: i32,
    ) -> Result<()> {
        let data_size = record.len();
        debug_assert!(data_size >= CHUNK_HEADER_SIZE);
        let length = i32::try_from(data_size - 4)
            .map_err(|_| io::Error::other("chunk record exceeds i32::MAX bytes"))?;
        record[0..4].copy_from_slice(&length.to_be_bytes());
        record[4] = version_id;

        let index = pos.header_index();
        let old_entry = self.offsets[index];
        let old_sector = (old_entry >> 8) & 0x00FF_FFFF;
        let old_count = old_entry & 0xFF;

        let mut sectors_needed = data_size.div_ceil(SECTOR_SIZE);
        let mcc_path = self.external_chunk_path(pos);

        enum ExternalCommit {
            MoveTemp {
                temp_path: PathBuf,
                mcc_path: PathBuf,
            },
            DeleteStale {
                mcc_path: PathBuf,
            },
        }

        let (new_sector, commit) = if sectors_needed >= EXTERNAL_SECTOR_THRESHOLD {
            log::warn!(
                "Chunk ({}, {}) compressed record size {data_size} bytes ({sectors_needed} sectors) exceeds 255 sectors; saving externally to {:?}",
                pos.x,
                pos.z,
                mcc_path
            );
            sectors_needed = 1;
            let new_sector_usize = self.used_sectors.reserve(1);
            let new_sector = u32::try_from(new_sector_usize).unwrap_or(u32::MAX);
            if new_sector >= MAX_VANILLA_SECTOR_NUMBER {
                self.used_sectors.free(new_sector_usize, 1);
                return Err(StorageError::RegionOffsetOverflow { sector: new_sector });
            }

            // Create temp file "tmp*.tmp" WITHOUT O_DSYNC or fsync, write raw compressed payload
            // (bytes 5..data_size) only, and close it.
            let (temp_path, mut temp_file) =
                match create_temp_file_in(&self.external_dir, "tmp", ".tmp", false) {
                    Ok(pair) => pair,
                    Err(err) => {
                        self.used_sectors.free(new_sector_usize, 1);
                        return Err(StorageError::Io(err));
                    }
                };
            if let Err(err) = temp_file.write_all(&record[CHUNK_HEADER_SIZE..]) {
                drop(temp_file);
                let _ = fs::remove_file(&temp_path);
                self.used_sectors.free(new_sector_usize, 1);
                return Err(StorageError::Io(err));
            }
            drop(temp_file);

            // Build 5-byte external stub: big-endian int32(1) + (version_id | 0x80).
            let mut stub = [0u8; CHUNK_HEADER_SIZE];
            stub[0..4].copy_from_slice(&1i32.to_be_bytes());
            stub[4] = version_id | EXTERNAL_STREAM_FLAG;

            let offset = u64::from(new_sector) * (SECTOR_SIZE as u64);
            let file = self
                .file
                .as_mut()
                .ok_or_else(|| io::Error::other("region file is closed"))?;
            if let Err(err) = write_all_at(file, offset, &stub) {
                let _ = fs::remove_file(&temp_path);
                self.used_sectors.free(new_sector_usize, 1);
                return Err(StorageError::Io(err));
            }

            (
                new_sector,
                ExternalCommit::MoveTemp {
                    temp_path,
                    mcc_path,
                },
            )
        } else {
            let new_sector_usize = self.used_sectors.reserve(sectors_needed);
            let new_sector = u32::try_from(new_sector_usize).unwrap_or(u32::MAX);
            if new_sector >= MAX_VANILLA_SECTOR_NUMBER {
                self.used_sectors.free(new_sector_usize, sectors_needed);
                return Err(StorageError::RegionOffsetOverflow { sector: new_sector });
            }

            let offset = u64::from(new_sector) * (SECTOR_SIZE as u64);
            let file = self
                .file
                .as_mut()
                .ok_or_else(|| io::Error::other("region file is closed"))?;
            if let Err(err) = write_all_at(file, offset, record) {
                self.used_sectors.free(new_sector_usize, sectors_needed);
                return Err(StorageError::Io(err));
            }

            (new_sector, ExternalCommit::DeleteStale { mcc_path })
        };

        // Step 4 & 5: Update header in memory and write entire 8,192-byte header to offset 0
        // BEFORE executing the deferred external commit step (edge case 7).
        self.offsets[index] = (new_sector << 8) | (sectors_needed as u32);
        self.timestamps[index] = timestamp;
        self.write_header()?;

        // Step 6: Execute deferred commit step.
        match commit {
            ExternalCommit::MoveTemp {
                temp_path,
                mcc_path,
            } => {
                replace_external_file(&temp_path, &mcc_path)?;
            }
            ExternalCommit::DeleteStale { mcc_path } => match fs::remove_file(&mcc_path) {
                Ok(()) => {}
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(StorageError::Io(err)),
            },
        }

        // Step 7: Free previous sectors only AFTER header write and external commit.
        if old_sector != 0 {
            self.used_sectors
                .free(old_sector as usize, old_count as usize);
        }

        Ok(())
    }

    /// Clears (deletes) a chunk using the current epoch time.
    ///
    /// If the chunk is already absent (`offsets[index] == 0`), does nothing (no header write,
    /// no timestamp update).
    pub fn clear_chunk(&mut self, pos: ChunkPos) -> Result<()> {
        self.clear_chunk_with_timestamp(pos, current_epoch_seconds())
    }

    /// Clears (deletes) a chunk using an explicit epoch-second `timestamp`.
    pub fn clear_chunk_with_timestamp(&mut self, pos: ChunkPos, timestamp: i32) -> Result<()> {
        let index = pos.header_index();
        let old_entry = self.offsets[index];
        if old_entry == 0 {
            return Ok(());
        }
        let old_sector = (old_entry >> 8) & 0x00FF_FFFF;
        let old_count = old_entry & 0xFF;

        self.offsets[index] = 0;
        self.timestamps[index] = timestamp;
        self.write_header()?;

        let mcc_path = self.external_chunk_path(pos);
        match fs::remove_file(&mcc_path) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(StorageError::Io(err)),
        }

        if old_sector != 0 {
            self.used_sectors
                .free(old_sector as usize, old_count as usize);
        }

        Ok(())
    }

    /// `fsync`s file data and metadata.
    pub fn flush(&mut self) -> Result<()> {
        if let Some(file) = self.file.as_ref() {
            file.sync_all()?;
        }
        Ok(())
    }

    /// Pads the `.mca` file to an exact 4,096-byte sector boundary (leaving 0-byte untouched
    /// files at 0 bytes), always `fsync`s data and metadata, and always closes the descriptor.
    pub fn close(mut self) -> Result<()> {
        self.close_inner()
    }

    fn close_inner(&mut self) -> Result<()> {
        let Some(mut file) = self.file.take() else {
            return Ok(());
        };

        let mut first_err: Option<io::Error> = None;

        // 1. Sector-boundary padding.
        match pad_to_sector_boundary(&mut file) {
            Ok(()) => {}
            Err(err) => first_err = Some(err),
        }

        // 2. Always fsync data and metadata, even if padding failed.
        if let Err(err) = file.sync_all()
            && first_err.is_none()
        {
            first_err = Some(err);
        }

        // 3. Always close the file descriptor.
        drop(file);

        match first_err {
            Some(err) => Err(StorageError::Io(err)),
            None => Ok(()),
        }
    }

    fn write_header(&mut self) -> Result<()> {
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| io::Error::other("region file is closed"))?;
        let mut header = [0u8; HEADER_SIZE];
        for i in 0..CHUNKS_PER_REGION {
            let off_idx = i * 4;
            header[off_idx..off_idx + 4].copy_from_slice(&self.offsets[i].to_be_bytes());
            let ts_idx = SECTOR_SIZE + i * 4;
            header[ts_idx..ts_idx + 4].copy_from_slice(&self.timestamps[i].to_be_bytes());
        }
        write_all_at(file, 0, &header)?;
        Ok(())
    }
}

impl Drop for RegionFile {
    fn drop(&mut self) {
        let _ = self.close_inner();
    }
}

fn pad_to_sector_boundary(file: &mut File) -> io::Result<()> {
    let file_size = file.metadata()?.len();
    let padded_size = file_size.div_ceil(SECTOR_SIZE as u64) * (SECTOR_SIZE as u64);
    if file_size != padded_size {
        write_all_at(file, padded_size - 1, &[0u8])?;
    }
    Ok(())
}

fn replace_external_file(temp_path: &Path, mcc_path: &Path) -> io::Result<()> {
    match fs::rename(temp_path, mcc_path) {
        Ok(()) => Ok(()),
        Err(err) => {
            // Fallback for platforms where rename does not overwrite an existing target.
            if mcc_path.exists() {
                let _ = fs::remove_file(mcc_path);
                fs::rename(temp_path, mcc_path)
            } else {
                Err(err)
            }
        }
    }
}

fn read_at_or_eof(file: &mut File, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
    file.seek(SeekFrom::Start(offset))?;
    let mut total = 0;
    while total < buf.len() {
        match file.read(&mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        }
    }
    Ok(total)
}

fn write_all_at(file: &mut File, offset: u64, buf: &[u8]) -> io::Result<()> {
    file.seek(SeekFrom::Start(offset))?;
    file.write_all(buf)
}
