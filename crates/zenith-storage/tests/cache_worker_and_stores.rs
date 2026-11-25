use std::fs;
use std::time::Duration;

use tempfile::TempDir;
use zenith_nbt::{Compound, List, Tag};
use zenith_storage::{
    ChunkPos, CompressionId, DimensionLayout, DimensionStorage, ENTITY_DEFAULT_DATA_VERSION,
    EntityChunkStore, OldChunkScanner, POI_DEFAULT_DATA_VERSION, PoiChunkStore, RegionCache,
    RegionFileOptions, RegionPos, SECTOR_SIZE, StorageWorker, TERRAIN_DEFAULT_DATA_VERSION,
    TerrainChunkStore, TerrainLoadResult, with_terrain_upgrade_context,
};

fn fast_opts() -> RegionFileOptions {
    RegionFileOptions {
        sync_writes: false,
        compression: CompressionId::Zlib,
    }
}

fn tag_with_version(ver: i32, marker: i32) -> Compound {
    let mut c = Compound::new();
    c.insert("DataVersion", Tag::Int(ver));
    c.insert("marker", Tag::Int(marker));
    c
}

// --- Edge case 17: RegionCache LRU eviction & negative caching ---

#[test]
fn region_cache_negative_caching_and_lru_eviction() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("region");

    let mut cache = RegionCache::with_capacity(&dir, fast_opts(), 2);
    let p0 = ChunkPos::new(0, 0); // region (0, 0)
    let p1 = ChunkPos::new(32, 0); // region (1, 0)
    let p2 = ChunkPos::new(64, 0); // region (2, 0)

    // Reading an absent region negatively caches it without creating any file.
    assert_eq!(cache.read(p0).unwrap(), None);
    assert!(cache.is_negatively_cached(p0));
    assert_eq!(cache.len(), 1);
    assert_eq!(cache.open_count(), 0);
    assert!(!dir.join("r.0.0.mca").exists());

    // Writing to a negatively cached region bypasses the sentinel, creates dir & .mca!
    cache.write(p0, Some(&tag_with_version(5023, 1))).unwrap();
    assert!(!cache.is_negatively_cached(p0));
    assert_eq!(cache.open_count(), 1);
    assert!(dir.join("r.0.0.mca").exists());

    // Fill capacity (2) with region (1, 0), then open region (2, 0) -> evicts LRU region (0, 0)!
    cache.write(p1, Some(&tag_with_version(5023, 2))).unwrap();
    assert_eq!(cache.open_count(), 2);
    // Before eviction, r.0.0.mca is not yet padded to 4,096 bytes.
    assert_ne!(
        fs::metadata(dir.join("r.0.0.mca")).unwrap().len() % SECTOR_SIZE as u64,
        0
    );

    cache.write(p2, Some(&tag_with_version(5023, 3))).unwrap();
    assert_eq!(cache.open_count(), 2);
    // Eviction closed r.0.0.mca immediately, padding it to an exact 4,096-byte boundary!
    assert_eq!(
        fs::metadata(dir.join("r.0.0.mca")).unwrap().len() % SECTOR_SIZE as u64,
        0
    );

    cache.flush().unwrap();
    cache.close().unwrap();

    // Verify all 3 chunks persisted cleanly.
    let mut reopen = RegionCache::new(&dir, fast_opts());
    assert_eq!(
        reopen
            .read(p0)
            .unwrap()
            .unwrap()
            .get("marker")
            .and_then(Tag::as_int),
        Some(1)
    );
    assert_eq!(
        reopen
            .read(p1)
            .unwrap()
            .unwrap()
            .get("marker")
            .and_then(Tag::as_int),
        Some(2)
    );
    assert_eq!(
        reopen
            .read(p2)
            .unwrap()
            .unwrap()
            .get("marker")
            .and_then(Tag::as_int),
        Some(3)
    );
    reopen.close().unwrap();
}

// --- Edge case 18: IO worker priority queue, coalescing, deep copy, sync, shutdown drain ---

#[test]
fn worker_coalescing_read_after_write_deep_copy_and_shutdown_drain() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("region");

    let worker = StorageWorker::new_sync(&dir, fast_opts());
    let p0 = ChunkPos::new(0, 0);
    let p1 = ChunkPos::new(1, 0);

    // Enqueue 3 priority-0 store tasks: p0 (v1), p1 (v1), p0 (v2 coalesced!).
    let h_p0_first = worker.submit_store_returning_handle(p0, Some(tag_with_version(5023, 1)));
    let h_p1 = worker.submit_store_returning_handle(p1, Some(tag_with_version(5023, 10)));
    let h_p0_second = worker.submit_store_returning_handle(p0, Some(tag_with_version(5023, 2)));

    // Step ONLY the 3 priority-0 tasks (priority-1 background drains have not run yet!).
    assert!(worker.step_one());
    assert!(worker.step_one());
    assert!(worker.step_one());
    assert_eq!(worker.pending_write_count_sync(), Some(2));
    assert!(!dir.join("r.0.0.mca").exists()); // Nothing written to disk yet!

    // Coalescing returned the EXACT same underlying completion handle!
    let inner_first = h_p0_first.try_get().unwrap().unwrap();
    let inner_second = h_p0_second.try_get().unwrap().unwrap();
    assert!(inner_first.same_handle(&inner_second));
    let inner_p1 = h_p1.try_get().unwrap().unwrap();
    assert!(!inner_first.same_handle(&inner_p1));

    // Foreground read (priority 0) overtakes pending background writes (priority 1),
    // reads coalesced v2 from memory without touching disk, and returns a deep copy!
    let read_h = worker.load(p0);
    assert!(worker.step_one()); // Runs foreground read BEFORE priority-1 write!
    assert!(!dir.join("r.0.0.mca").exists());
    let mut loaded_copy = read_h.try_get().unwrap().unwrap().unwrap();
    assert_eq!(loaded_copy.get("marker").and_then(Tag::as_int), Some(2));
    // Mutating the returned copy must NOT mutate the buffered pending write!
    loaded_copy.insert("marker", Tag::Int(999));

    // Next step runs the FIRST priority-1 drain -> pops OLDEST entry (p0, preserving insertion order!).
    assert!(worker.step_one());
    assert!(inner_first.is_complete());
    assert!(!inner_p1.is_complete());
    assert_eq!(worker.pending_write_count_sync(), Some(1));

    // Now queue a priority-0 read AND initiate shutdown BEFORE stepping it:
    // - queued priority-0 task must become a NO-OP (leaving its handle incomplete!)
    // - remaining priority-1 write (p1) MUST still drain to disk before priority-2 closes!
    let dropped_read = worker.load(p1);
    let _ = worker.begin_shutdown();
    worker.drain_sync();

    assert!(!dropped_read.is_complete());
    assert!(inner_p1.is_complete());
    assert_eq!(worker.pending_write_count_sync(), Some(0));

    // Reopen fresh cache and verify BOTH p0 (marker=2, unmutated!) and p1 (marker=10) are on disk!
    let mut verify = RegionCache::new(&dir, fast_opts());
    assert_eq!(
        verify
            .read(p0)
            .unwrap()
            .unwrap()
            .get("marker")
            .and_then(Tag::as_int),
        Some(2)
    );
    assert_eq!(
        verify
            .read(p1)
            .unwrap()
            .unwrap()
            .get("marker")
            .and_then(Tag::as_int),
        Some(10)
    );
    verify.close().unwrap();
}

#[test]
fn worker_background_thread_mode_and_synchronize_flush() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("region");

    let worker = StorageWorker::spawn(&dir, fast_opts()).unwrap();
    let handles: Vec<_> = (0..16)
        .map(|i| worker.store(ChunkPos::new(i, 0), Some(tag_with_version(5023, i))))
        .collect();
    // Pending read-after-write works immediately on background thread
    let read_val = worker.load_sync(ChunkPos::new(7, 0)).unwrap().unwrap();
    assert_eq!(read_val.get("marker").and_then(Tag::as_int), Some(7));

    worker.synchronize_sync(true).unwrap();
    for h in &handles {
        assert!(h.is_complete());
    }
    worker.close();
    // Idempotent second close is a no-op
    worker.close();

    let mut verify = RegionCache::new(&dir, fast_opts());
    for i in 0..16 {
        assert_eq!(
            verify
                .read(ChunkPos::new(i, 0))
                .unwrap()
                .unwrap()
                .get("marker")
                .and_then(Tag::as_int),
            Some(i)
        );
    }
    verify.close().unwrap();
}

// --- World-blending OldChunkScanner ---

#[test]
fn old_chunk_scanner_detects_version_cutoff_and_blending_data() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("region");
    let worker = StorageWorker::new_sync(&dir, fast_opts());

    // Modern chunk (DataVersion = 4997, no blending_data) -> NOT old
    worker
        .store_sync(ChunkPos::new(0, 0), Some(tag_with_version(4997, 1)))
        .unwrap();
    // Old chunk by DataVersion (4996 < 4997) -> OLD
    worker
        .store_sync(ChunkPos::new(5, 0), Some(tag_with_version(4996, 2)))
        .unwrap();
    // Modern DataVersion BUT has "blending_data" compound -> OLD
    let mut with_blend = tag_with_version(5023, 3);
    with_blend.insert("blending_data", Tag::Compound(Compound::new()));
    worker
        .store_sync(ChunkPos::new(-1, -1), Some(with_blend))
        .unwrap();
    // Missing DataVersion (defaults to 0 < 4997) -> OLD
    let mut no_ver = Compound::new();
    no_ver.insert("foo", Tag::Int(1));
    worker
        .store_sync(ChunkPos::new(10, 10), Some(no_ver))
        .unwrap();

    let mut scanner = OldChunkScanner::new();
    assert!(!scanner.is_old_chunk_around(&worker, ChunkPos::new(0, 0), 0));
    assert!(!scanner.is_old_chunk_around(&worker, ChunkPos::new(2, 0), 2));
    // Range 1 from (-1, -1) reaches (-1, -1) across region boundary from (0, 0)
    assert!(scanner.is_old_chunk_around(&worker, ChunkPos::new(0, 0), 1));
    // Range 3 from (2, 0) reaches (5, 0)
    assert!(scanner.is_old_chunk_around(&worker, ChunkPos::new(2, 0), 3));
    assert!(scanner.is_old_chunk_around(&worker, ChunkPos::new(10, 10), 0));
    assert!(scanner.cache_len() >= 2);

    worker.close();
}

// --- Terrain / Entity / POI store semantics and DimensionStorage ---

#[test]
fn terrain_entity_and_poi_storage_differences() {
    let tmp = TempDir::new().unwrap();
    let dim_layout = DimensionLayout::new(tmp.path().join("dimensions/minecraft/overworld"));

    // 1. Terrain store
    {
        let terrain = TerrainChunkStore::new(StorageWorker::new_sync(
            dim_layout.region_dir(),
            fast_opts(),
        ));
        let pos = ChunkPos::new(2, 3);

        // Empty proto-chunk with no valid structure starts is NOT written.
        let mut empty_proto = Compound::new();
        empty_proto.insert("Status", Tag::from("minecraft:empty"));
        assert!(!terrain.save_chunk(pos, &empty_proto, false).unwrap());

        // Empty proto-chunk WITH valid structure start IS written (and keeps inline entities).
        let mut starts = Compound::new();
        let mut village = Compound::new();
        village.insert("id", Tag::from("minecraft:village"));
        starts.insert("minecraft:village", Tag::Compound(village));
        let mut structures = Compound::new();
        structures.insert("starts", Tag::Compound(starts));
        empty_proto.insert("structures", Tag::Compound(structures));
        empty_proto.insert("entities", Tag::List(List::from(vec![Tag::Int(1)])));
        assert!(terrain.save_chunk(pos, &empty_proto, false).unwrap());
        if let TerrainLoadResult::Loaded {
            chunk,
            data_version,
        } = terrain.load_chunk(pos)
        {
            assert_eq!(data_version, TERRAIN_DEFAULT_DATA_VERSION);
            assert!(chunk.contains_key("entities"));
        } else {
            panic!("expected Loaded");
        }

        // Upgrade context helper inserts and cleans up "__context".
        let mut full = Compound::new();
        full.insert("DataVersion", Tag::Int(5023));
        full.insert("xPos", Tag::Int(pos.x));
        full.insert("zPos", Tag::Int(pos.z));
        full.insert("Status", Tag::from("minecraft:full"));
        full.insert("entities", Tag::List(List::new())); // must be omitted for full chunk
        with_terrain_upgrade_context(
            &mut full,
            "minecraft:overworld",
            Some("minecraft:noise"),
            |c| {
                let ctx = c.get("__context").unwrap().as_compound().unwrap();
                assert_eq!(
                    ctx.get("dimension").and_then(Tag::as_str),
                    Some("minecraft:overworld")
                );
            },
        );
        assert!(!full.contains_key("__context"));

        assert!(terrain.save_chunk(pos, &full, true).unwrap());
        if let TerrainLoadResult::Loaded { chunk, .. } = terrain.load_chunk(pos) {
            assert!(!chunk.contains_key("entities"));
        } else {
            panic!("expected Loaded");
        }

        // Proto-chunk NEVER overwrites existing full chunk!
        let mut proto2 = Compound::new();
        proto2.insert("Status", Tag::from("minecraft:carvers"));
        assert!(!terrain.save_chunk(pos, &proto2, false).unwrap());

        // Misplaced chunk (xPos/zPos mismatch) is relocated on load.
        let mut misplaced = Compound::new();
        misplaced.insert("xPos", Tag::Int(99));
        misplaced.insert("zPos", Tag::Int(-88));
        misplaced.insert("Status", Tag::from("minecraft:full"));
        let p_mis = ChunkPos::new(4, 5);
        terrain.save_chunk(p_mis, &misplaced, true).unwrap();
        match terrain.load_chunk(p_mis) {
            TerrainLoadResult::Relocated {
                chunk,
                original_pos,
                ..
            } => {
                assert_eq!(original_pos, ChunkPos::new(99, -88));
                assert_eq!(chunk.get("xPos").and_then(Tag::as_int), Some(4));
                assert_eq!(chunk.get("zPos").and_then(Tag::as_int), Some(5));
            }
            other => panic!("expected Relocated, got {other:?}"),
        }

        terrain.close();
    }

    // 2. Entity store
    {
        let mut entities = EntityChunkStore::new(StorageWorker::new_sync(
            dim_layout.entities_dir(),
            fast_opts(),
        ));
        let pos = ChunkPos::new(0, 0);

        // Loading an absent chunk adds it to known-empty set.
        let absent = entities.load_chunk(pos).unwrap();
        assert!(absent.was_absent);
        assert_eq!(absent.data_version, ENTITY_DEFAULT_DATA_VERSION);
        assert!(entities.is_known_empty(pos));

        // Repeat empty store is skipped!
        assert!(!entities.save_chunk(pos, &List::new(), 5023).unwrap());

        // Non-empty store writes and removes from known-empty set.
        let mut sheep = Compound::new();
        sheep.insert("id", Tag::from("minecraft:sheep"));
        let list = List::from(vec![Tag::Compound(sheep)]);
        assert!(entities.save_chunk(pos, &list, 5023).unwrap());
        assert!(!entities.is_known_empty(pos));

        let loaded = entities.load_chunk(pos).unwrap();
        assert_eq!(loaded.entities.len(), 1);
        assert_eq!(loaded.data_version, 5023);

        // Storing empty list now writes a deletion (None) and adds back to known-empty.
        assert!(entities.save_chunk(pos, &List::new(), 5023).unwrap());
        assert!(entities.is_known_empty(pos));
        assert!(!entities.save_chunk(pos, &List::new(), 5023).unwrap());
        assert!(entities.load_chunk(pos).unwrap().was_absent);

        // Misplaced "Position" still loads entities and reports misplaced_from.
        let p_mis = ChunkPos::new(1, 1);
        let mut raw_ent = Compound::new();
        raw_ent.insert("Position", Tag::IntArray(vec![77, 88]));
        raw_ent.insert("Entities", Tag::List(list.clone()));
        entities.worker().store_sync(p_mis, Some(raw_ent)).unwrap();
        let mis_loaded = entities.load_chunk(p_mis).unwrap();
        assert_eq!(mis_loaded.misplaced_from, Some(ChunkPos::new(77, 88)));
        assert_eq!(mis_loaded.entities.len(), 1);

        entities.close();
    }

    // 3. POI store
    {
        let mut poi =
            PoiChunkStore::new(StorageWorker::new_sync(dim_layout.poi_dir(), fast_opts()));
        let p0 = ChunkPos::new(0, 0);
        let p1 = ChunkPos::new(1, 0);

        // Absent chunk defaults DataVersion to 1945.
        let empty_load = poi.load_chunk(p0).unwrap();
        assert_eq!(empty_load.data_version, POI_DEFAULT_DATA_VERSION);
        assert!(empty_load.sections.is_empty());

        // Dirtied empty chunk writes empty "Sections" compound (NEVER issues None/clear).
        poi.mark_dirty(p0, Compound::new(), 5023);
        poi.mark_dirty(p1, Compound::new(), 5023);
        assert_eq!(poi.dirty_count(), 2);
        assert!(poi.flush_chunk(p0).unwrap());
        assert_eq!(poi.dirty_count(), 1);
        assert_eq!(poi.flush_dirty_within(Duration::from_secs(1)).unwrap(), 1);
        assert_eq!(poi.dirty_count(), 0);

        // Verify p0 actually exists on disk with empty Sections compound (not deleted!).
        let on_disk = poi.worker().load_sync(p0).unwrap().unwrap();
        assert_eq!(on_disk.get("DataVersion").and_then(Tag::as_int), Some(5023));
        assert!(
            on_disk
                .get("Sections")
                .and_then(Tag::as_compound)
                .unwrap()
                .is_empty()
        );

        poi.close();

        // Corrupt POI chunk: non-I/O (NBT format) error propagates, while I/O error (corrupt zlib)
        // is treated as empty chunk!
        let mca_path = dim_layout.poi_dir().join("r.0.0.mca");
        let mut rf = zenith_storage::RegionFile::open(
            &mca_path,
            &dim_layout.poi_dir(),
            RegionFileOptions {
                sync_writes: false,
                compression: CompressionId::None,
            },
        )
        .unwrap();
        // Invalid NBT tag type ID 99 under CompressionId::None -> StorageError::Nbt (must propagate!)
        rf.write_raw_compressed_chunk(
            ChunkPos::new(2, 0),
            CompressionId::None.id(),
            &[99, 0, 0],
            1,
        )
        .unwrap();
        // Corrupt zlib stream -> flate2 returns std::io::Error -> StorageError::Io (treated as empty!)
        rf.write_raw_compressed_chunk(
            ChunkPos::new(3, 0),
            CompressionId::Zlib.id(),
            &[0xFF, 0xFF, 0xFF, 0xFF],
            1,
        )
        .unwrap();
        rf.close().unwrap();

        let poi2 = PoiChunkStore::new(StorageWorker::new_sync(dim_layout.poi_dir(), fast_opts()));
        assert!(poi2.load_chunk(ChunkPos::new(2, 0)).is_err());
        let io_recovered = poi2.load_chunk(ChunkPos::new(3, 0)).unwrap();
        assert!(io_recovered.recovered_from_io_error);
        assert_eq!(io_recovered.data_version, POI_DEFAULT_DATA_VERSION);
        poi2.close();
    }

    // 4. DimensionStorage full_save_with_flush & shutdown ordering
    {
        let mut dim = DimensionStorage::new_sync(&dim_layout, fast_opts());
        let pos = ChunkPos::new(5, 5);
        dim.poi.mark_dirty(pos, Compound::new(), 5023);
        let mut chunk = Compound::new();
        chunk.insert("Status", Tag::from("minecraft:full"));
        chunk.insert("xPos", Tag::Int(5));
        chunk.insert("zPos", Tag::Int(5));
        dim.full_save_with_flush(&[(pos, chunk, true)]).unwrap();
        assert_eq!(dim.poi.dirty_count(), 0);
        dim.close();
    }
}

#[test]
fn terrain_corrupt_chunk_falls_back_to_empty_proto_chunk() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("region");
    fs::create_dir_all(&dir).unwrap();

    let mut rf = zenith_storage::RegionFile::open_in_dir(
        &dir,
        RegionPos::new(0, 0),
        RegionFileOptions {
            sync_writes: false,
            compression: CompressionId::None,
        },
    )
    .unwrap();
    // Corrupt NBT tag ID 99
    rf.write_raw_compressed_chunk(
        ChunkPos::new(0, 0),
        CompressionId::None.id(),
        &[99, 0, 0],
        1,
    )
    .unwrap();
    rf.close().unwrap();

    let terrain = TerrainChunkStore::new(StorageWorker::new_sync(&dir, fast_opts()));
    match terrain.load_chunk(ChunkPos::new(0, 0)) {
        TerrainLoadResult::CorruptedFallback { chunk, .. } => {
            assert_eq!(
                chunk.get("Status").and_then(Tag::as_str),
                Some("minecraft:empty")
            );
            assert_eq!(chunk.get("xPos").and_then(Tag::as_int), Some(0));
            assert_eq!(chunk.get("zPos").and_then(Tag::as_int), Some(0));
        }
        other => panic!("expected CorruptedFallback, got {other:?}"),
    }
    terrain.close();
}
