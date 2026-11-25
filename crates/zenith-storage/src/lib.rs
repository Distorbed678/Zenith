//! Clean-room world storage library for Zenith (26.3), implemented from
//! `docs/specs/storage/region-and-level.md`.
//!
//! Provides:
//! - **World layout (26.3):** [`WorldLayout`], [`DimensionLayout`], [`Identifier`], [`ChunkPos`],
//!   [`RegionPos`], and path-escape validation.
//! - **Anvil region container:** [`RegionFile`] (`.mca` and external `.mcc` files), all 5
//!   compression IDs (`Gzip` read-only, `Zlib`, `None`, `lz4-java` `LZ4Block`, and `Custom` 127),
//!   lazy 0-byte open, truncated/corrupt header recovery, first-fit allocate-before-free
//!   copy-on-write, exact external `.mcc` cutoff (`1,044,480` vs `1,044,481` bytes), close-time
//!   sector padding, and lightweight existence probing.
//! - **Region cache & IO worker:** 256-entry [`RegionCache`] with negative caching,
//!   [`StorageWorker`] (3-level priority queue, write coalescing, foreground read-after-write deep
//!   copies, synchronize/flush, idempotent shutdown drain, synchronous + optional background thread),
//!   and [`OldChunkScanner`] (1,024-region blending cache).
//! - **Per-dimension stores:** [`TerrainChunkStore`], [`EntityChunkStore`], [`PoiChunkStore`],
//!   and [`DimensionStorage`] save/shutdown coordination.
//! - **Session lock & file persistence:** [`SessionLock`] (`E2 98 83`, `fsync`-before-lock,
//!   non-destructive probe), [`safe_replace_file`], `level.dat` / `level.dat_old` fallback &
//!   world summaries, [`PlayerDataStorage`], and dual-format `data/*.dat` saved data.

pub mod cache;
pub mod compression;
pub mod error;
pub mod fs_util;
pub mod layout;
pub mod level;
pub mod lz4_block;
pub mod nbt_io;
pub mod player;
pub mod region;
pub mod safe_replace;
pub mod saved_data;
pub mod session_lock;
pub mod stores;
pub mod worker;

pub use cache::{REGION_CACHE_CAPACITY, RegionCache};
pub use compression::{CompressionId, EXTERNAL_STREAM_FLAG, STREAM_BUFFER_SIZE};
pub use error::{Result, StorageError};
pub use fs_util::QUARANTINE_TIME_FORMAT;
pub use layout::{
    ChunkPos, DimensionLayout, Identifier, RegionPos, RegionStoreKind, WorldLayout,
    resolve_identifier_path,
};
pub use level::{
    AllowedSymlinks, DATA_VERSION_26_3, LEVEL_NBT_QUOTA, LEVEL_STORAGE_VERSION_ANVIL,
    LEVEL_STORAGE_VERSION_MCREGION, LOW_DISK_SPACE_THRESHOLD, LevelFallbackLoad, WorldSummary,
    append_version_history, is_world_candidate, list_world_summaries, read_level_dat,
    read_level_dat_file, read_level_dat_with_fallback, read_level_dat_with_fallback_at,
    save_level_dat, scan_world_summary, write_level_dat,
};
pub use lz4_block::{
    LZ4_BLOCK_HEADER_LEN, LZ4_BLOCK_MAGIC, LZ4_CHECKSUM_MASK, LZ4_DEFAULT_BLOCK_SIZE,
    LZ4_MAX_BLOCK_SIZE, LZ4_METHOD_LZ4, LZ4_METHOD_RAW, LZ4_MIN_BLOCK_SIZE, LZ4_XXHASH32_SEED,
    Lz4BlockReader, Lz4BlockWriter, compression_level_for_block_size, lz4_block_checksum,
};
pub use nbt_io::{
    EntryResult, SkippingCompoundCollector, StreamTagVisitor, ValueResult,
    read_named_compound_level, read_named_compound_unlimited, scan_compound_in_memory,
    scan_named_stream, write_gzip_named_compound_fallback, write_gzip_wrapped_compound_fallback,
    write_named_compound_fallback,
};
pub use player::PlayerDataStorage;
pub use region::{
    CHUNK_HEADER_SIZE, CHUNKS_PER_REGION, EXTERNAL_SECTOR_THRESHOLD, HEADER_SIZE,
    MAX_INTERNAL_RECORD_BYTES, MAX_VANILLA_SECTOR_NUMBER, RegionFile, RegionFileOptions,
    SECTOR_SIZE, SectorBitset,
};
pub use safe_replace::{SAFE_REPLACE_MAX_RETRIES, safe_replace_file};
pub use saved_data::{
    GZIP_MAGIC_LE, SAVED_DATA_DEFAULT_VERSION, SavedDataFile, read_saved_data, write_saved_data,
    write_saved_data_26_3,
};
pub use session_lock::{
    DELETE_WORLD_RETRIES, DELETE_WORLD_RETRY_DELAY, SESSION_LOCK_BYTES, SessionLock,
    delete_world_dir, delete_world_dir_with_retry_delay,
};
pub use stores::{
    DimensionStorage, ENTITY_DEFAULT_DATA_VERSION, EntityChunkStore, EntityLoadResult,
    POI_DEFAULT_DATA_VERSION, PoiChunkStore, PoiLoadResult, TERRAIN_DEFAULT_DATA_VERSION,
    TerrainChunkStore, TerrainLoadResult, empty_proto_chunk, with_terrain_upgrade_context,
};
pub use worker::{
    BLENDER_OLD_CHUNK_VERSION_CUTOFF, BLENDER_SCAN_CACHE_CAPACITY, IoHandle, OldChunkBitset,
    OldChunkScanVisitor, OldChunkScanner, SharedStorageError, StorageWorker, WorkerPriority,
};
