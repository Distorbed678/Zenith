//! Per-dimension terrain (`region/`), entity (`entities/`), and POI (`poi/`) storage differences
//! and dimension save/shutdown coordination.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use zenith_nbt::{Compound, List, NbtString, Tag, TagType};

use crate::error::Result;
use crate::layout::{ChunkPos, DimensionLayout};
use crate::nbt_io::{EntryResult, StreamTagVisitor, ValueResult};
use crate::region::RegionFileOptions;
use crate::worker::{SharedStorageError, StorageWorker};

/// Default `DataVersion` for terrain chunks when `"DataVersion"` is absent (`-1`).
pub const TERRAIN_DEFAULT_DATA_VERSION: i32 = -1;

/// Default `DataVersion` for entity chunks when `"DataVersion"` is absent (`-1`).
pub const ENTITY_DEFAULT_DATA_VERSION: i32 = -1;

/// Default `DataVersion` for POI chunks when `"DataVersion"` is absent (`1945`).
pub const POI_DEFAULT_DATA_VERSION: i32 = 1_945;

// --- Terrain chunk store (`region/`) ---

/// Outcome of loading a terrain chunk from `region/`.
#[derive(Debug)]
pub enum TerrainLoadResult {
    /// Chunk did not exist on disk; caller receives a fresh empty proto-chunk.
    Missing(Compound),
    /// Chunk loaded normally.
    Loaded {
        /// Loaded chunk NBT.
        chunk: Compound,
        /// Resolved `DataVersion` (defaults to `-1` if absent).
        data_version: i32,
    },
    /// Chunk's `(xPos, zPos)` did not match the requested [`ChunkPos`]; relocated in-memory.
    Relocated {
        /// Relocated chunk NBT (with `xPos` and `zPos` updated to requested coordinates).
        chunk: Compound,
        /// Resolved `DataVersion` (defaults to `-1` if absent).
        data_version: i32,
        /// Original mismatched coordinates stored in the chunk.
        original_pos: ChunkPos,
    },
    /// Non-fatal load error (I/O, NBT, or parse); fell back to a fresh replaceable empty proto-chunk.
    CorruptedFallback {
        /// Fresh replaceable empty proto-chunk.
        chunk: Compound,
        /// Error that triggered fallback.
        error: SharedStorageError,
    },
}

/// Creates a fresh replaceable empty proto-chunk NBT compound at `pos`.
#[must_use]
pub fn empty_proto_chunk(pos: ChunkPos) -> Compound {
    let mut c = Compound::new();
    c.insert("xPos", Tag::Int(pos.x));
    c.insert("zPos", Tag::Int(pos.z));
    c.insert("Status", Tag::from("minecraft:empty"));
    c
}

/// Temporarily injects `"__context": { "dimension": ..., "generator": ... }` into `chunk` while
/// running `upgrade_fn`, removing `"__context"` unconditionally afterward.
pub fn with_terrain_upgrade_context<R>(
    chunk: &mut Compound,
    dimension: &str,
    generator: Option<&str>,
    upgrade_fn: impl FnOnce(&mut Compound) -> R,
) -> R {
    let mut ctx = Compound::new();
    ctx.insert("dimension", Tag::from(dimension));
    if let Some(gen_id) = generator {
        ctx.insert("generator", Tag::from(gen_id));
    }
    chunk.insert("__context", Tag::Compound(ctx));
    let result = upgrade_fn(chunk);
    chunk.remove("__context");
    result
}

#[derive(Debug, Default)]
struct StatusScanVisitor {
    depth: usize,
    reading_status: bool,
    status: Option<String>,
}

impl StreamTagVisitor for StatusScanVisitor {
    fn visit_root_entry(&mut self, tag_type: TagType) -> ValueResult {
        if tag_type == TagType::Compound {
            self.depth = 0;
            ValueResult::Continue
        } else {
            ValueResult::Halt
        }
    }

    fn visit_entry(&mut self, tag_type: TagType, name: &NbtString) -> EntryResult {
        if self.depth == 0 && tag_type == TagType::String && name == "Status" {
            self.reading_status = true;
            return EntryResult::Enter;
        }
        EntryResult::Skip
    }

    fn visit_string(&mut self, value: &NbtString) -> ValueResult {
        if self.reading_status {
            self.reading_status = false;
            self.status = Some(value.to_string_lossy().into_owned());
            return ValueResult::Halt;
        }
        ValueResult::Continue
    }
}

fn is_full_status(status: &str) -> bool {
    status == "minecraft:full" || status == "full"
}

fn is_empty_status(status: Option<&str>) -> bool {
    matches!(status, None | Some("minecraft:empty") | Some("empty"))
}

fn has_valid_structure_starts(chunk: &Compound) -> bool {
    let Some(structures) = chunk.get("structures").and_then(Tag::as_compound) else {
        return false;
    };
    let Some(starts) = structures.get("starts").and_then(Tag::as_compound) else {
        return false;
    };
    starts.iter().any(|(_, v)| {
        v.as_compound()
            .and_then(|c| c.get("id"))
            .and_then(Tag::as_str)
            .is_some_and(|id| !id.is_empty() && id != "INVALID")
    })
}

/// Per-dimension terrain chunk store (`<dimension>/region/`).
#[derive(Debug)]
pub struct TerrainChunkStore {
    worker: StorageWorker,
}

impl TerrainChunkStore {
    /// Creates a terrain store backed by `worker`.
    #[must_use]
    pub fn new(worker: StorageWorker) -> Self {
        Self { worker }
    }

    /// Accesses the underlying [`StorageWorker`].
    #[must_use]
    pub fn worker(&self) -> &StorageWorker {
        &self.worker
    }

    /// Loads a terrain chunk, applying the spec's default `DataVersion` (`-1`), misplaced-chunk
    /// relocation, and non-fatal error fallback to a fresh replaceable empty proto-chunk.
    pub fn load_chunk(&self, pos: ChunkPos) -> TerrainLoadResult {
        match self.worker.load_sync(pos) {
            Ok(None) => TerrainLoadResult::Missing(empty_proto_chunk(pos)),
            Ok(Some(mut chunk)) => {
                let data_version = chunk
                    .get("DataVersion")
                    .and_then(Tag::as_int)
                    .unwrap_or(TERRAIN_DEFAULT_DATA_VERSION);

                let stored_x = chunk.get("xPos").and_then(Tag::as_int);
                let stored_z = chunk.get("zPos").and_then(Tag::as_int);
                if let (Some(sx), Some(sz)) = (stored_x, stored_z)
                    && (sx != pos.x || sz != pos.z)
                {
                    log::error!(
                        "Misplaced terrain chunk at ({}, {}): stored coordinates were ({sx}, {sz}); relocating",
                        pos.x,
                        pos.z
                    );
                    chunk.insert("xPos", Tag::Int(pos.x));
                    chunk.insert("zPos", Tag::Int(pos.z));
                    return TerrainLoadResult::Relocated {
                        chunk,
                        data_version,
                        original_pos: ChunkPos::new(sx, sz),
                    };
                }

                TerrainLoadResult::Loaded {
                    chunk,
                    data_version,
                }
            }
            Err(error) => {
                log::error!(
                    "Failed to load terrain chunk ({}, {}): {error}; falling back to empty proto-chunk",
                    pos.x,
                    pos.z
                );
                TerrainLoadResult::CorruptedFallback {
                    chunk: empty_proto_chunk(pos),
                    error,
                }
            }
        }
    }

    /// Saves a terrain chunk, enforcing:
    /// - Full chunks omit `"entities"` (proto-chunks preserve inline `"entities"`).
    /// - Empty-status proto-chunks with no valid structure starts are **not** written.
    /// - Proto-chunks **never** overwrite an existing full chunk on disk.
    ///
    /// Returns `Ok(true)` if the chunk was queued/written, or `Ok(false)` if skipped.
    pub fn save_chunk(
        &self,
        pos: ChunkPos,
        chunk: &Compound,
        is_full_chunk: bool,
    ) -> std::result::Result<bool, SharedStorageError> {
        if !is_full_chunk {
            let status = chunk.get("Status").and_then(Tag::as_str);
            if is_empty_status(status) && !has_valid_structure_starts(chunk) {
                return Ok(false);
            }
            // Check whether an existing full chunk is already present on disk / pending.
            if let Ok((true, visitor)) = self.worker.scan_sync(pos, StatusScanVisitor::default())
                && visitor.status.as_deref().is_some_and(is_full_status)
            {
                return Ok(false);
            }
            self.worker.store_sync(pos, Some(chunk.clone()))?;
            return Ok(true);
        }

        // Full chunks omit `"entities"`.
        let to_store = if chunk.contains_key("entities") {
            let mut stripped = chunk.clone();
            stripped.remove("entities");
            stripped
        } else {
            chunk.clone()
        };
        self.worker.store_sync(pos, Some(to_store))?;
        Ok(true)
    }

    /// Synchronizes and optionally flushes the terrain worker.
    pub fn synchronize(&self, flush: bool) -> std::result::Result<(), SharedStorageError> {
        self.worker.synchronize_sync(flush)
    }

    /// Shuts down the terrain store.
    pub fn close(self) {
        self.worker.close();
    }
}

// --- Entity chunk store (`entities/`) ---

/// Result of loading an entity chunk from `entities/`.
#[derive(Debug, Clone)]
pub struct EntityLoadResult {
    /// Resolved `DataVersion` (defaults to `-1` if absent).
    pub data_version: i32,
    /// Loaded entity list (empty if absent).
    pub entities: List,
    /// Set if the stored `"Position"` did not match the requested [`ChunkPos`] (entities are still loaded).
    pub misplaced_from: Option<ChunkPos>,
    /// `true` if no entity chunk existed on disk.
    pub was_absent: bool,
}

/// Per-dimension full-chunk entity store (`<dimension>/entities/`).
#[derive(Debug)]
pub struct EntityChunkStore {
    worker: StorageWorker,
    known_empty: HashSet<ChunkPos>,
}

impl EntityChunkStore {
    /// Creates an entity store backed by `worker`.
    #[must_use]
    pub fn new(worker: StorageWorker) -> Self {
        Self {
            worker,
            known_empty: HashSet::new(),
        }
    }

    /// Accesses the underlying [`StorageWorker`].
    #[must_use]
    pub fn worker(&self) -> &StorageWorker {
        &self.worker
    }

    /// Returns `true` if `pos` is currently in the in-memory known-empty set.
    #[must_use]
    pub fn is_known_empty(&self, pos: ChunkPos) -> bool {
        self.known_empty.contains(&pos)
    }

    /// Loads entities for `pos`.
    ///
    /// - Absent chunks are added to the known-empty set.
    /// - Mismatched `"Position"` logs an error and records `misplaced_from`, while still loading entities.
    pub fn load_chunk(
        &mut self,
        pos: ChunkPos,
    ) -> std::result::Result<EntityLoadResult, SharedStorageError> {
        let Some(tag) = self.worker.load_sync(pos)? else {
            self.known_empty.insert(pos);
            return Ok(EntityLoadResult {
                data_version: ENTITY_DEFAULT_DATA_VERSION,
                entities: List::new(),
                misplaced_from: None,
                was_absent: true,
            });
        };

        let data_version = tag
            .get("DataVersion")
            .and_then(Tag::as_int)
            .unwrap_or(ENTITY_DEFAULT_DATA_VERSION);

        let misplaced_from = tag
            .get("Position")
            .and_then(Tag::as_int_array)
            .and_then(|arr| {
                if arr.len() == 2 && (arr[0] != pos.x || arr[1] != pos.z) {
                    let stored = ChunkPos::new(arr[0], arr[1]);
                    log::error!(
                        "Misplaced entity chunk at ({}, {}): stored Position was ({}, {}); still loading entities",
                        pos.x,
                        pos.z,
                        stored.x,
                        stored.z
                    );
                    Some(stored)
                } else {
                    None
                }
            });

        let entities = tag
            .get("Entities")
            .and_then(Tag::as_list)
            .cloned()
            .unwrap_or_default();

        Ok(EntityLoadResult {
            data_version,
            entities,
            misplaced_from,
            was_absent: false,
        })
    }

    /// Stores entities for `pos`:
    /// - Empty list for a chunk **not** in `known_empty` issues a deletion (`None`, clearing its
    ///   `.mca` slot and deleting any `.mcc`) and adds `pos` to `known_empty`.
    /// - Repeat empty stores for a chunk **already** in `known_empty` are skipped (`Ok(false)`).
    /// - Non-empty list removes `pos` from `known_empty` and writes
    ///   `{ "DataVersion", "Position": [x, z], "Entities": [...] }`.
    pub fn save_chunk(
        &mut self,
        pos: ChunkPos,
        entities: &List,
        data_version: i32,
    ) -> std::result::Result<bool, SharedStorageError> {
        if entities.is_empty() {
            if !self.known_empty.insert(pos) {
                return Ok(false);
            }
            self.worker.store_sync(pos, None)?;
            return Ok(true);
        }

        self.known_empty.remove(&pos);
        let mut root = Compound::new();
        root.insert("DataVersion", Tag::Int(data_version));
        root.insert("Position", Tag::IntArray(vec![pos.x, pos.z]));
        root.insert("Entities", Tag::List(entities.clone()));
        self.worker.store_sync(pos, Some(root))?;
        Ok(true)
    }

    /// Synchronizes and optionally flushes the entity worker.
    pub fn synchronize(&self, flush: bool) -> std::result::Result<(), SharedStorageError> {
        self.worker.synchronize_sync(flush)
    }

    /// Shuts down the entity store.
    pub fn close(self) {
        self.worker.close();
    }
}

// --- POI chunk store (`poi/`) ---

/// Result of loading a POI chunk from `poi/`.
#[derive(Debug, Clone)]
pub struct PoiLoadResult {
    /// Resolved `DataVersion` (defaults to `1945` if absent).
    pub data_version: i32,
    /// `"Sections"` compound (empty if absent or recovered from an I/O read error).
    pub sections: Compound,
    /// `true` if an I/O read error was caught and treated as an empty chunk.
    pub recovered_from_io_error: bool,
}

#[derive(Debug, Clone)]
struct DirtyPoiEntry {
    data_version: i32,
    sections: Compound,
}

/// Per-dimension point-of-interest section store (`<dimension>/poi/`).
#[derive(Debug)]
pub struct PoiChunkStore {
    worker: StorageWorker,
    dirty_map: HashMap<ChunkPos, DirtyPoiEntry>,
    dirty_order: VecDeque<ChunkPos>,
}

impl PoiChunkStore {
    /// Creates a POI store backed by `worker`.
    #[must_use]
    pub fn new(worker: StorageWorker) -> Self {
        Self {
            worker,
            dirty_map: HashMap::new(),
            dirty_order: VecDeque::new(),
        }
    }

    /// Accesses the underlying [`StorageWorker`].
    #[must_use]
    pub fn worker(&self) -> &StorageWorker {
        &self.worker
    }

    /// Number of currently dirty POI chunks awaiting flush.
    #[must_use]
    pub fn dirty_count(&self) -> usize {
        self.dirty_map.len()
    }

    /// Marks `pos` dirty with `sections` and `data_version`.
    ///
    /// Note: POI storage **never** issues `None`/clear writes; a dirtied empty chunk writes an
    /// empty `"Sections"` compound.
    pub fn mark_dirty(&mut self, pos: ChunkPos, sections: Compound, data_version: i32) {
        let is_new = !self.dirty_map.contains_key(&pos);
        self.dirty_map.insert(
            pos,
            DirtyPoiEntry {
                data_version,
                sections,
            },
        );
        if is_new {
            self.dirty_order.push_back(pos);
        }
    }

    /// Loads POI sections for `pos`:
    /// - Missing `"DataVersion"` defaults to `1945`.
    /// - **I/O read errors** are logged and treated as an empty chunk.
    /// - **All other errors** (e.g., NBT format/depth errors) propagate!
    pub fn load_chunk(
        &self,
        pos: ChunkPos,
    ) -> std::result::Result<PoiLoadResult, SharedStorageError> {
        if let Some(dirty) = self.dirty_map.get(&pos) {
            return Ok(PoiLoadResult {
                data_version: dirty.data_version,
                sections: dirty.sections.clone(),
                recovered_from_io_error: false,
            });
        }

        match self.worker.load_sync(pos) {
            Ok(None) => Ok(PoiLoadResult {
                data_version: POI_DEFAULT_DATA_VERSION,
                sections: Compound::new(),
                recovered_from_io_error: false,
            }),
            Ok(Some(tag)) => {
                let data_version = tag
                    .get("DataVersion")
                    .and_then(Tag::as_int)
                    .unwrap_or(POI_DEFAULT_DATA_VERSION);
                let sections = tag
                    .get("Sections")
                    .and_then(Tag::as_compound)
                    .cloned()
                    .unwrap_or_default();
                Ok(PoiLoadResult {
                    data_version,
                    sections,
                    recovered_from_io_error: false,
                })
            }
            Err(err) => {
                if err.is_io() {
                    log::error!(
                        "I/O error reading POI chunk ({}, {}): {err}; treating as empty chunk",
                        pos.x,
                        pos.z
                    );
                    Ok(PoiLoadResult {
                        data_version: POI_DEFAULT_DATA_VERSION,
                        sections: Compound::new(),
                        recovered_from_io_error: true,
                    })
                } else {
                    Err(err)
                }
            }
        }
    }

    /// Flushes a single dirty POI chunk (called explicitly right before its terrain chunk saves).
    ///
    /// Returns `Ok(true)` if `pos` was dirty and flushed, or `Ok(false)` if clean.
    pub fn flush_chunk(&mut self, pos: ChunkPos) -> std::result::Result<bool, SharedStorageError> {
        let Some(entry) = self.dirty_map.remove(&pos) else {
            return Ok(false);
        };
        self.write_poi_entry(pos, entry)?;
        Ok(true)
    }

    /// Incrementally flushes dirty POI chunks until `budget` elapses or no dirty chunks remain.
    pub fn flush_dirty_within(
        &mut self,
        budget: Duration,
    ) -> std::result::Result<usize, SharedStorageError> {
        let start = Instant::now();
        let mut flushed = 0;
        while let Some(pos) = self.dirty_order.pop_front() {
            if let Some(entry) = self.dirty_map.remove(&pos) {
                self.write_poi_entry(pos, entry)?;
                flushed += 1;
                if start.elapsed() >= budget {
                    break;
                }
            }
        }
        Ok(flushed)
    }

    /// Flushes all remaining dirty POI chunks at once (used on full save).
    pub fn flush_all_dirty(&mut self) -> std::result::Result<usize, SharedStorageError> {
        let mut flushed = 0;
        while let Some(pos) = self.dirty_order.pop_front() {
            if let Some(entry) = self.dirty_map.remove(&pos) {
                self.write_poi_entry(pos, entry)?;
                flushed += 1;
            }
        }
        Ok(flushed)
    }

    fn write_poi_entry(
        &self,
        pos: ChunkPos,
        entry: DirtyPoiEntry,
    ) -> std::result::Result<(), SharedStorageError> {
        let mut root = Compound::new();
        root.insert("DataVersion", Tag::Int(entry.data_version));
        root.insert("Sections", Tag::Compound(entry.sections));
        self.worker.store_sync(pos, Some(root))
    }

    /// Synchronizes and optionally flushes the POI worker.
    pub fn synchronize(&self, flush: bool) -> std::result::Result<(), SharedStorageError> {
        self.worker.synchronize_sync(flush)
    }

    /// Shuts down the POI store.
    pub fn close(self) {
        self.worker.close();
    }
}

// --- Per-dimension coordinator ---

/// Coordinates a dimension's three independent region stores (`region/`, `entities/`, `poi/`)
/// sharing [`RegionFileOptions`], save ordering, and shutdown ordering.
#[derive(Debug)]
pub struct DimensionStorage {
    /// Terrain chunk store (`region/`).
    pub terrain: TerrainChunkStore,
    /// Entity chunk store (`entities/`).
    pub entities: EntityChunkStore,
    /// POI section store (`poi/`).
    pub poi: PoiChunkStore,
}

impl DimensionStorage {
    /// Opens a dimension's three region stores in synchronous mode.
    #[must_use]
    pub fn new_sync(layout: &DimensionLayout, options: RegionFileOptions) -> Self {
        Self {
            terrain: TerrainChunkStore::new(StorageWorker::new_sync(layout.region_dir(), options)),
            entities: EntityChunkStore::new(StorageWorker::new_sync(
                layout.entities_dir(),
                options,
            )),
            poi: PoiChunkStore::new(StorageWorker::new_sync(layout.poi_dir(), options)),
        }
    }

    /// Opens a dimension's three region stores with dedicated background IO threads.
    pub fn spawn(layout: &DimensionLayout, options: RegionFileOptions) -> Result<Self> {
        let terrain = TerrainChunkStore::new(StorageWorker::spawn(layout.region_dir(), options)?);
        let entities = EntityChunkStore::new(StorageWorker::spawn(layout.entities_dir(), options)?);
        let poi = PoiChunkStore::new(StorageWorker::spawn(layout.poi_dir(), options)?);
        Ok(Self {
            terrain,
            entities,
            poi,
        })
    }

    /// Saves a single terrain chunk, flushing its POI chunk first (per spec).
    pub fn save_terrain_chunk(
        &mut self,
        pos: ChunkPos,
        chunk: &Compound,
        is_full_chunk: bool,
    ) -> std::result::Result<bool, SharedStorageError> {
        self.poi.flush_chunk(pos)?;
        self.terrain.save_chunk(pos, chunk, is_full_chunk)
    }

    /// Executes the spec's full save ordering:
    /// 1. Save each terrain chunk (flushing its POI chunk immediately beforehand).
    /// 2. Flush all remaining dirty POI chunks.
    /// 3. Synchronize the terrain worker with `flush = true`.
    pub fn full_save_with_flush(
        &mut self,
        terrain_chunks: &[(ChunkPos, Compound, bool)],
    ) -> std::result::Result<(), SharedStorageError> {
        for (pos, chunk, is_full) in terrain_chunks {
            self.poi.flush_chunk(*pos)?;
            self.terrain.save_chunk(*pos, chunk, *is_full)?;
        }
        self.poi.flush_all_dirty()?;
        self.terrain.synchronize(true)?;
        Ok(())
    }

    /// Executes the spec's shutdown ordering:
    /// close POI store → close terrain store → close entity store.
    pub fn close(self) {
        self.poi.close();
        self.terrain.close();
        self.entities.close();
    }
}
