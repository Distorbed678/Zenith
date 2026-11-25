use std::fs::{self, File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};

use tempfile::TempDir;
use zenith_nbt::{Compound, Tag};
use zenith_storage::{
    CHUNK_HEADER_SIZE, ChunkPos, CompressionId, HEADER_SIZE, MAX_INTERNAL_RECORD_BYTES,
    MAX_VANILLA_SECTOR_NUMBER, RegionFile, RegionFileOptions, RegionPos, SECTOR_SIZE, StorageError,
    compression::new_gzip_encoder, write_named_compound_fallback,
};

fn sample_chunk(x: i32, z: i32, payload_bytes: usize) -> Compound {
    let mut c = Compound::new();
    c.insert("DataVersion", Tag::Int(5023));
    c.insert("xPos", Tag::Int(x));
    c.insert("zPos", Tag::Int(z));
    if payload_bytes > 0 {
        c.insert("data", Tag::ByteArray(vec![0x33i8; payload_bytes]));
    }
    c
}

// --- Compression IDs 1, 2, 3, 4, 127 ---

#[test]
fn all_five_compression_ids_and_server_property_rules() {
    assert_eq!(
        CompressionId::from_server_property("deflate").unwrap(),
        CompressionId::Zlib
    );
    assert_eq!(
        CompressionId::from_server_property("none").unwrap(),
        CompressionId::None
    );
    assert_eq!(
        CompressionId::from_server_property("lz4").unwrap(),
        CompressionId::Lz4
    );
    assert!(CompressionId::from_server_property("gzip").is_err());
    assert!(CompressionId::from_server_property("custom").is_err());
    assert_eq!(
        CompressionId::apply_server_property(CompressionId::Lz4, "bogus"),
        CompressionId::Lz4
    );

    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();

    // Writable IDs: Zlib (2), None (3), LZ4 (4)
    for (idx, comp) in [CompressionId::Zlib, CompressionId::None, CompressionId::Lz4]
        .into_iter()
        .enumerate()
    {
        let pos = ChunkPos::new(idx as i32, 0);
        let chunk = sample_chunk(pos.x, pos.z, 256);
        let opts = RegionFileOptions {
            sync_writes: false,
            compression: comp,
        };
        let mut rf = RegionFile::open_in_dir(dir, RegionPos::new(0, 0), opts).unwrap();
        rf.write_chunk(pos, &chunk).unwrap();
        assert!(rf.has_chunk(pos));
        let loaded = rf.read_chunk(pos).unwrap().unwrap();
        assert_eq!(loaded, chunk);
        rf.close().unwrap();
    }

    // Verify LZ4 chunk actually begins with "LZ4Block" on disk.
    {
        let rf = RegionFile::open_in_dir(
            dir,
            RegionPos::new(0, 0),
            RegionFileOptions {
                sync_writes: false,
                compression: CompressionId::Lz4,
            },
        )
        .unwrap();
        let (sector, _) = rf.chunk_sectors(ChunkPos::new(2, 0)).unwrap();
        rf.close().unwrap();
        let raw = fs::read(dir.join("r.0.0.mca")).unwrap();
        let off = (sector as usize) * SECTOR_SIZE;
        assert_eq!(raw[off + 4], CompressionId::Lz4.id());
        assert_eq!(&raw[off + 5..off + 13], b"LZ4Block");
    }

    // Read-only IDs (Gzip = 1, Custom = 127) must be rejected for write_chunk.
    for ro in [CompressionId::Gzip, CompressionId::Custom] {
        let mut rf = RegionFile::open_in_dir(
            dir,
            RegionPos::new(1, 0),
            RegionFileOptions {
                sync_writes: false,
                compression: ro,
            },
        )
        .unwrap();
        assert!(matches!(
            rf.write_chunk(ChunkPos::new(32, 0), &sample_chunk(32, 0, 0)),
            Err(StorageError::UnsupportedCompressionForWrite(id)) if id == ro.id()
        ));
        rf.close().unwrap();
    }

    // GZip (ID 1) read round trip (both internal and external .mcc)
    {
        let pos_int = ChunkPos::new(5, 5);
        let chunk_int = sample_chunk(5, 5, 64);
        let mut gz_buf = Vec::new();
        {
            let mut enc = new_gzip_encoder(&mut gz_buf);
            write_named_compound_fallback(&mut enc, &chunk_int).unwrap();
            enc.finish().unwrap();
        }
        // Verify RFC 1952 header mtime == 0 and OS byte == 0xFF
        assert_eq!(&gz_buf[0..2], &[0x1F, 0x8B]);
        assert_eq!(&gz_buf[4..8], &[0, 0, 0, 0]);
        assert_eq!(gz_buf[9], 0xFF);

        let mut rf = RegionFile::open_in_dir(
            dir,
            RegionPos::new(0, 0),
            RegionFileOptions {
                sync_writes: false,
                compression: CompressionId::Zlib,
            },
        )
        .unwrap();
        rf.write_raw_compressed_chunk(pos_int, CompressionId::Gzip.id(), &gz_buf, 1000)
            .unwrap();
        assert!(rf.has_chunk(pos_int));
        assert_eq!(rf.read_chunk(pos_int).unwrap().unwrap(), chunk_int);
        rf.close().unwrap();
    }

    // Custom ID 127 behavior (edge case 10):
    // - has_chunk returns true (internal 127 and external 255 with .mcc)
    // - full read reads modified-UTF-8 string, logs error (valid or invalid id), returns Ok(None)
    // - truncated stream under ID 127 propagates I/O error
    {
        let mut rf = RegionFile::open_in_dir(
            dir,
            RegionPos::new(2, 0),
            RegionFileOptions {
                sync_writes: false,
                compression: CompressionId::Zlib,
            },
        )
        .unwrap();

        // Valid identifier payload
        let valid_id_chunk = ChunkPos::new(64, 0);
        let id_bytes = b"mymod:zstd";
        let mut payload = Vec::new();
        payload.extend_from_slice(&(id_bytes.len() as u16).to_be_bytes());
        payload.extend_from_slice(id_bytes);
        rf.write_raw_compressed_chunk(valid_id_chunk, CompressionId::Custom.id(), &payload, 10)
            .unwrap();
        assert!(rf.has_chunk(valid_id_chunk));
        assert_eq!(rf.read_chunk(valid_id_chunk).unwrap(), None);

        // Invalid identifier payload
        let invalid_id_chunk = ChunkPos::new(65, 0);
        let bad_id = b"NOT A VALID ID";
        let mut payload2 = Vec::new();
        payload2.extend_from_slice(&(bad_id.len() as u16).to_be_bytes());
        payload2.extend_from_slice(bad_id);
        rf.write_raw_compressed_chunk(invalid_id_chunk, CompressionId::Custom.id(), &payload2, 11)
            .unwrap();
        assert!(rf.has_chunk(invalid_id_chunk));
        assert_eq!(rf.read_chunk(invalid_id_chunk).unwrap(), None);

        // Truncated modified-UTF-8 under ID 127 propagates I/O error
        let trunc_chunk = ChunkPos::new(66, 0);
        rf.write_raw_compressed_chunk(
            trunc_chunk,
            CompressionId::Custom.id(),
            &[0x00, 0x10, b'a'],
            12,
        )
        .unwrap();
        assert!(rf.has_chunk(trunc_chunk));
        assert!(rf.read_chunk(trunc_chunk).is_err());

        // External 255 (127 | 0x80) with existing .mcc file
        let ext_custom = ChunkPos::new(67, 0);
        // Write a 1-sector chunk first, then patch it into an external stub and write `.mcc`
        // (writing an internal chunk deletes any pre-existing stale `.mcc` per edge case 6!).
        rf.write_raw_compressed_chunk(ext_custom, CompressionId::None.id(), &[1], 13)
            .unwrap();
        let (sec, _) = rf.chunk_sectors(ext_custom).unwrap();
        rf.close().unwrap();

        fs::write(dir.join(ext_custom.external_filename()), &payload).unwrap();
        let mut f = OpenOptions::new()
            .write(true)
            .open(dir.join("r.2.0.mca"))
            .unwrap();
        f.seek(SeekFrom::Start(u64::from(sec) * SECTOR_SIZE as u64))
            .unwrap();
        f.write_all(&[0, 0, 0, 1, CompressionId::Custom.external_id()])
            .unwrap();
        drop(f);

        let mut rf = RegionFile::open_in_dir(
            dir,
            RegionPos::new(2, 0),
            RegionFileOptions {
                sync_writes: false,
                compression: CompressionId::Zlib,
            },
        )
        .unwrap();
        assert!(rf.has_chunk(ext_custom));
        assert_eq!(rf.read_chunk(ext_custom).unwrap(), None);

        // External GZip (129 = 1 | 0x80) round trip
        let ext_gz = ChunkPos::new(68, 0);
        let gz_chunk = sample_chunk(68, 0, 48);
        let mut gz_ext_buf = Vec::new();
        {
            let mut enc = new_gzip_encoder(&mut gz_ext_buf);
            write_named_compound_fallback(&mut enc, &gz_chunk).unwrap();
            enc.finish().unwrap();
        }
        rf.write_raw_compressed_chunk(ext_gz, CompressionId::None.id(), &[1], 14)
            .unwrap();
        let (gz_sec, _) = rf.chunk_sectors(ext_gz).unwrap();
        rf.close().unwrap();

        fs::write(dir.join(ext_gz.external_filename()), &gz_ext_buf).unwrap();
        let mut f = OpenOptions::new()
            .write(true)
            .open(dir.join("r.2.0.mca"))
            .unwrap();
        f.seek(SeekFrom::Start(u64::from(gz_sec) * SECTOR_SIZE as u64))
            .unwrap();
        f.write_all(&[0, 0, 0, 1, CompressionId::Gzip.external_id()])
            .unwrap();
        drop(f);

        let mut rf = RegionFile::open_in_dir(
            dir,
            RegionPos::new(2, 0),
            RegionFileOptions {
                sync_writes: false,
                compression: CompressionId::Zlib,
            },
        )
        .unwrap();
        assert!(rf.has_chunk(ext_gz));
        assert_eq!(rf.read_chunk(ext_gz).unwrap().unwrap(), gz_chunk);
        rf.close().unwrap();
    }
}

// --- Edge cases 3, 4, 14, 15: never-in-place, first-fit gap, no truncation, timestamps, close-time padding ---

#[test]
fn sector_allocation_never_in_place_first_fit_no_truncation_and_close_padding() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let opts = RegionFileOptions {
        sync_writes: false,
        compression: CompressionId::None,
    };

    let c0 = ChunkPos::new(0, 0);
    let c1 = ChunkPos::new(1, 0);
    let c2 = ChunkPos::new(2, 0);

    let mut rf = RegionFile::open_in_dir(dir, RegionPos::new(0, 0), opts).unwrap();

    // Write c0 (1 sector -> sector 2)
    rf.write_chunk_with_timestamp(c0, &sample_chunk(0, 0, 100), 1000)
        .unwrap();
    assert_eq!(rf.chunk_sectors(c0), Some((2, 1)));
    assert_eq!(rf.timestamp(c0), 1000);

    // Edge case 15: while open, file is NOT yet padded to 4,096-byte boundary!
    let unpadded_len = fs::metadata(dir.join("r.0.0.mca")).unwrap().len();
    assert_ne!(unpadded_len % SECTOR_SIZE as u64, 0);

    // Edge case 3: rewriting c0 with identical or smaller size MUST move to a different sector (sector 3)!
    rf.write_chunk_with_timestamp(c0, &sample_chunk(0, 0, 50), 1001)
        .unwrap();
    assert_eq!(rf.chunk_sectors(c0), Some((3, 1)));
    assert_eq!(rf.timestamp(c0), 1001);
    // Sector 2 was freed AFTER writing sector 3, so next 1-sector write gets sector 2 (first-fit!).
    rf.write_chunk_with_timestamp(c1, &sample_chunk(1, 0, 50), 1002)
        .unwrap();
    assert_eq!(rf.chunk_sectors(c1), Some((2, 1)));

    // Grow c1 to 2 sectors (> 4,096 B): sector 2 is still held during allocation, sector 3 is c0,
    // so c1 moves to sector 4..6 and frees sector 2.
    rf.write_chunk_with_timestamp(c1, &sample_chunk(1, 0, 5000), 1003)
        .unwrap();
    assert_eq!(rf.chunk_sectors(c1), Some((4, 2)));

    // First-fit: c2 (1 sector) takes the lowest free gap at sector 2!
    rf.write_chunk_with_timestamp(c2, &sample_chunk(2, 0, 50), 1004)
        .unwrap();
    assert_eq!(rf.chunk_sectors(c2), Some((2, 1)));

    // Edge case 14: clearing c1 zeroes offset, updates timestamp to deletion time (NOT 0!),
    // and frees sectors 4..6 WITHOUT truncating the file!
    let size_before_clear = fs::metadata(dir.join("r.0.0.mca")).unwrap().len();
    rf.clear_chunk_with_timestamp(c1, 2000).unwrap();
    assert_eq!(rf.location_entry(c1), 0);
    assert_eq!(rf.timestamp(c1), 2000);
    assert_eq!(
        fs::metadata(dir.join("r.0.0.mca")).unwrap().len(),
        size_before_clear
    );

    // Clearing an already-absent chunk does NOT update its timestamp!
    rf.clear_chunk_with_timestamp(c1, 9999).unwrap();
    assert_eq!(rf.timestamp(c1), 2000);

    // Close pads to an exact 4,096-byte boundary.
    rf.close().unwrap();
    let padded_len = fs::metadata(dir.join("r.0.0.mca")).unwrap().len();
    assert_eq!(padded_len % SECTOR_SIZE as u64, 0);
    assert!(padded_len >= size_before_clear);
}

// --- Edge cases 5, 6, 7, 8: exact external .mcc cutoff, stub rules, shrink cleanup ---

#[test]
fn exact_external_cutoff_and_shrink_cleanup() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let opts = RegionFileOptions {
        sync_writes: false,
        compression: CompressionId::None,
    };

    let mut rf = RegionFile::open_in_dir(dir, RegionPos::new(-1, -1), opts).unwrap();
    let pos_fit = ChunkPos::new(-1, -1); // negative coords -> c.-1.-1.mcc
    let pos_spill = ChunkPos::new(-2, -1);

    // Build an uncompressed NBT compound whose exact serialized length is 1,044,475 bytes
    // (dataSize = 5 + 1,044,475 = 1,044,480 = 255 * 4096 => STAYS INTERNAL!).
    let base = sample_chunk(pos_fit.x, pos_fit.z, 1);
    let mut base_bytes = Vec::new();
    write_named_compound_fallback(&mut base_bytes, &base).unwrap();
    let overhead = base_bytes.len() - 1;

    let exact_internal_nbt_len = MAX_INTERNAL_RECORD_BYTES - CHUNK_HEADER_SIZE; // 1,044,475
    let exact_fit_chunk = sample_chunk(pos_fit.x, pos_fit.z, exact_internal_nbt_len - overhead);
    let mut check_fit = Vec::new();
    write_named_compound_fallback(&mut check_fit, &exact_fit_chunk).unwrap();
    assert_eq!(check_fit.len() + CHUNK_HEADER_SIZE, 1_044_480);

    rf.write_chunk(pos_fit, &exact_fit_chunk).unwrap();
    assert_eq!(rf.chunk_sectors(pos_fit), Some((2, 255)));
    assert!(!dir.join(pos_fit.external_filename()).exists());
    assert_eq!(rf.read_chunk(pos_fit).unwrap().unwrap(), exact_fit_chunk);

    // Now 1 byte larger: dataSize = 1,044,481 (payload = 1,044,476) => SPILLS TO c.-2.-1.mcc!
    let exact_spill_chunk = sample_chunk(
        pos_spill.x,
        pos_spill.z,
        exact_internal_nbt_len - overhead + 1,
    );
    let mut check_spill = Vec::new();
    write_named_compound_fallback(&mut check_spill, &exact_spill_chunk).unwrap();
    assert_eq!(check_spill.len() + CHUNK_HEADER_SIZE, 1_044_481);

    rf.write_chunk(pos_spill, &exact_spill_chunk).unwrap();
    let (stub_sector, stub_count) = rf.chunk_sectors(pos_spill).unwrap();
    assert_eq!(stub_count, 1);
    let mcc_path = dir.join("c.-2.-1.mcc");
    assert!(mcc_path.is_file());
    // Edge case 6: .mcc holds raw compressed payload ONLY (no 5-byte header).
    assert_eq!(fs::read(&mcc_path).unwrap(), check_spill);
    assert_eq!(
        rf.read_chunk(pos_spill).unwrap().unwrap(),
        exact_spill_chunk
    );

    // Verify 5-byte stub in .mca: big-endian int32(1) + (versionId | 0x80)
    rf.flush().unwrap();
    let mca_bytes = fs::read(dir.join("r.-1.-1.mca")).unwrap();
    let stub_off = (stub_sector as usize) * SECTOR_SIZE;
    assert_eq!(
        &mca_bytes[stub_off..stub_off + 5],
        &[0, 0, 0, 1, CompressionId::None.external_id()]
    );

    // Shrinking oversized chunk back to <= 255 sectors deletes stale .mcc!
    let small_chunk = sample_chunk(pos_spill.x, pos_spill.z, 16);
    rf.write_chunk(pos_spill, &small_chunk).unwrap();
    assert!(!mcc_path.exists());
    assert_eq!(rf.read_chunk(pos_spill).unwrap().unwrap(), small_chunk);

    rf.close().unwrap();
}

#[test]
fn external_stub_unexpected_length_and_zero_length_quirks() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let opts = RegionFileOptions {
        sync_writes: false,
        compression: CompressionId::None,
    };

    let pos = ChunkPos::new(0, 0);
    let chunk = sample_chunk(0, 0, 32);
    let mut raw_nbt = Vec::new();
    write_named_compound_fallback(&mut raw_nbt, &chunk).unwrap();

    let mut rf = RegionFile::open_in_dir(dir, RegionPos::new(0, 0), opts).unwrap();
    rf.write_chunk(pos, &chunk).unwrap();
    let (sec, _) = rf.chunk_sectors(pos).unwrap();
    rf.close().unwrap();

    let mcc_path = dir.join("c.0.0.mcc");
    fs::write(&mcc_path, &raw_nbt).unwrap();
    let mca_path = dir.join("r.0.0.mca");

    let patch_stub = |length: i32, version_byte: u8| {
        let mut f = OpenOptions::new().write(true).open(&mca_path).unwrap();
        f.seek(SeekFrom::Start(u64::from(sec) * SECTOR_SIZE as u64))
            .unwrap();
        f.write_all(&length.to_be_bytes()).unwrap();
        f.write_all(&[version_byte]).unwrap();
    };

    // Edge case 8a: external bit set + non-zero streamLength (length = 10 -> streamLength = 9) STILL reads .mcc!
    patch_stub(10, CompressionId::None.external_id());
    let mut rf = RegionFile::open_in_dir(dir, RegionPos::new(0, 0), opts).unwrap();
    assert!(rf.has_chunk(pos));
    assert_eq!(rf.read_chunk(pos).unwrap().unwrap(), chunk);
    rf.close().unwrap();

    // Edge case 8b: external bit set + negative length (-5 -> streamLength = -6) STILL reads .mcc!
    patch_stub(-5, CompressionId::None.external_id());
    let mut rf = RegionFile::open_in_dir(dir, RegionPos::new(0, 0), opts).unwrap();
    assert!(rf.has_chunk(pos));
    assert_eq!(rf.read_chunk(pos).unwrap().unwrap(), chunk);
    rf.close().unwrap();

    // Edge case 8c + 16: external bit set + length == 0:
    // - full read rejects length == 0 BEFORE checking bit 7 -> returns Ok(None)
    // - lightweight probe has_chunk does NOT check length == 0 for external -> returns true!
    patch_stub(0, CompressionId::None.external_id());
    let mut rf = RegionFile::open_in_dir(dir, RegionPos::new(0, 0), opts).unwrap();
    assert!(rf.has_chunk(pos));
    assert_eq!(rf.read_chunk(pos).unwrap(), None);
    rf.close().unwrap();

    // Missing .mcc returns Ok(None) and has_chunk == false.
    fs::remove_file(&mcc_path).unwrap();
    patch_stub(1, CompressionId::None.external_id());
    let mut rf = RegionFile::open_in_dir(dir, RegionPos::new(0, 0), opts).unwrap();
    assert!(!rf.has_chunk(pos));
    assert_eq!(rf.read_chunk(pos).unwrap(), None);
    rf.close().unwrap();
}

// --- Edge cases 11, 12, 13, 16: 0-byte file, header recovery, 32-bit offset limit, probe slack ---

#[test]
fn zero_byte_untouched_and_clear_of_missing() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let opts = RegionFileOptions {
        sync_writes: false,
        compression: CompressionId::Zlib,
    };

    // Untouched open + close leaves 0-byte file.
    let rf = RegionFile::open_in_dir(dir, RegionPos::new(0, 0), opts).unwrap();
    rf.close().unwrap();
    assert_eq!(fs::metadata(dir.join("r.0.0.mca")).unwrap().len(), 0);

    // Clear of missing chunk in brand-new region leaves 0-byte file.
    let mut rf = RegionFile::open_in_dir(dir, RegionPos::new(1, 1), opts).unwrap();
    rf.clear_chunk(ChunkPos::new(32, 32)).unwrap();
    rf.close().unwrap();
    assert_eq!(fs::metadata(dir.join("r.1.1.mca")).unwrap().len(), 0);

    // Same via RegionCache::write(pos, None) on a non-existent region (edge case 11).
    let mut cache = zenith_storage::RegionCache::new(dir, opts);
    cache.write(ChunkPos::new(64, 64), None).unwrap();
    cache.close().unwrap();
    assert_eq!(fs::metadata(dir.join("r.2.2.mca")).unwrap().len(), 0);
}

#[test]
fn truncated_and_corrupt_header_recovery_and_probe_slack() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let opts = RegionFileOptions {
        sync_writes: false,
        compression: CompressionId::None,
    };

    // 1. Truncated header (< 8192 bytes)
    let trunc_path = dir.join("r.0.0.mca");
    let mut partial_hdr = vec![0u8; 64];
    // Slot 0 points to sector 2, count 1 (out of bounds since file is 64 bytes -> zeroed in memory)
    partial_hdr[0..4].copy_from_slice(&((2u32 << 8) | 1).to_be_bytes());
    fs::write(&trunc_path, &partial_hdr).unwrap();
    let rf = RegionFile::open(&trunc_path, dir, opts).unwrap();
    assert_eq!(rf.location_entry(ChunkPos::new(0, 0)), 0);
    // In-memory only: file on disk still has the original 64 bytes before close (close pads 64 -> 4096)
    assert_eq!(fs::metadata(&trunc_path).unwrap().len(), 64);
    rf.close().unwrap();

    // 2. Bad slots: sector < 2, numSectors == 0, sector * 4096 > fileSize, overlapping slots,
    //    and start <= fileSize < end.
    let corrupt_path = dir.join("r.1.0.mca");
    let mut data = vec![0u8; HEADER_SIZE + SECTOR_SIZE]; // 3 sectors total (0, 1, 2)
    // Slot 0: sector 1, count 1 (overlaps header -> zeroed in memory)
    data[0..4].copy_from_slice(&((1u32 << 8) | 1).to_be_bytes());
    // Slot 1: sector 2, count 0 (zero sector count -> zeroed in memory)
    data[4..8].copy_from_slice(&(2u32 << 8).to_be_bytes());
    // Slot 2: sector 4, count 1 (4 * 4096 = 16384 > 12288 -> zeroed in memory)
    data[8..12].copy_from_slice(&((4u32 << 8) | 1).to_be_bytes());
    // Slot 3 & Slot 4: overlapping both pointing to sector 2, count 1 (passes open!)
    data[12..16].copy_from_slice(&((2u32 << 8) | 1).to_be_bytes());
    data[16..20].copy_from_slice(&((2u32 << 8) | 1).to_be_bytes());
    // Slot 5: sector 2, count 3 (start 8192 <= 12288, end 20480 > 12288 -> passes open!)
    data[20..24].copy_from_slice(&((2u32 << 8) | 3).to_be_bytes());

    // Put a valid uncompressed chunk in sector 2
    let chunk = sample_chunk(35, 0, 8);
    let mut nbt = Vec::new();
    write_named_compound_fallback(&mut nbt, &chunk).unwrap();
    let len = (1 + nbt.len()) as i32;
    data[HEADER_SIZE..HEADER_SIZE + 4].copy_from_slice(&len.to_be_bytes());
    data[HEADER_SIZE + 4] = CompressionId::None.id();
    data[HEADER_SIZE + 5..HEADER_SIZE + 5 + nbt.len()].copy_from_slice(&nbt);

    fs::write(&corrupt_path, &data).unwrap();
    let mut rf = RegionFile::open(&corrupt_path, dir, opts).unwrap();
    assert_eq!(rf.location_entry(ChunkPos::new(32, 0)), 0); // slot 0 zeroed
    assert_eq!(rf.location_entry(ChunkPos::new(33, 0)), 0); // slot 1 zeroed
    assert_eq!(rf.location_entry(ChunkPos::new(34, 0)), 0); // slot 2 zeroed
    // Overlapping slots 3 & 4 both readable
    assert_eq!(rf.read_chunk(ChunkPos::new(35, 0)).unwrap().unwrap(), chunk);
    assert_eq!(rf.read_chunk(ChunkPos::new(36, 0)).unwrap().unwrap(), chunk);
    // Slot 5 passed open-time check
    assert_ne!(rf.location_entry(ChunkPos::new(37, 0)), 0);
    // Disk header was NOT modified by open-time repairs!
    let disk_after_open = fs::read(&corrupt_path).unwrap();
    assert_eq!(&disk_after_open[0..12], &data[0..12]);
    rf.close().unwrap();

    // 3. Edge case 16: existence-probe upper-bound slack (`length - 1 == 4096 * numSectors`)
    //    and file truncated right after byte 5.
    let slack_path = dir.join("r.2.0.mca");
    let mut slack_data = vec![0u8; HEADER_SIZE + 5]; // truncated right after byte 5!
    slack_data[0..4].copy_from_slice(&((2u32 << 8) | 1).to_be_bytes());
    // length - 1 = 4096 == 4096 * 1 (exceeds available 4091 bytes by 5!)
    slack_data[HEADER_SIZE..HEADER_SIZE + 4].copy_from_slice(&4097i32.to_be_bytes());
    slack_data[HEADER_SIZE + 4] = CompressionId::None.id();
    fs::write(&slack_path, &slack_data).unwrap();

    let mut rf = RegionFile::open(&slack_path, dir, opts).unwrap();
    assert!(rf.has_chunk(ChunkPos::new(64, 0))); // probe says true!
    assert_eq!(rf.read_chunk(ChunkPos::new(64, 0)).unwrap(), None); // full read says truncated!
    rf.close().unwrap();

    // Negative declared stream length (`length = -1` -> `streamLength = -2`) rejected by both
    let mut neg_data = vec![0u8; HEADER_SIZE + SECTOR_SIZE];
    neg_data[0..4].copy_from_slice(&((2u32 << 8) | 1).to_be_bytes());
    neg_data[HEADER_SIZE..HEADER_SIZE + 4].copy_from_slice(&(-1i32).to_be_bytes());
    neg_data[HEADER_SIZE + 4] = CompressionId::None.id();
    fs::write(&slack_path, &neg_data).unwrap();
    let mut rf = RegionFile::open(&slack_path, dir, opts).unwrap();
    assert!(!rf.has_chunk(ChunkPos::new(64, 0)));
    assert_eq!(rf.read_chunk(ChunkPos::new(64, 0)).unwrap(), None);
    rf.close().unwrap();
}

#[test]
fn region_32bit_signed_byte_offset_limit() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let path = dir.join("r.0.0.mca");

    // Create a sparse 2 GiB + 4 KiB file so open-time 64-bit bounds check accepts sector 524,288,
    // while read/probe enforce vanilla's 32-bit signed byte-offset cap (< 524,288 sectors).
    let mut f = File::create(&path).unwrap();
    let mut hdr = vec![0u8; HEADER_SIZE];
    let entry = (MAX_VANILLA_SECTOR_NUMBER << 8) | 1;
    hdr[0..4].copy_from_slice(&entry.to_be_bytes());
    f.write_all(&hdr).unwrap();
    let target_len =
        u64::from(MAX_VANILLA_SECTOR_NUMBER) * (SECTOR_SIZE as u64) + SECTOR_SIZE as u64;
    if f.set_len(target_len).is_ok() {
        drop(f);
        let mut rf = RegionFile::open(
            &path,
            dir,
            RegionFileOptions {
                sync_writes: false,
                compression: CompressionId::Zlib,
            },
        )
        .unwrap();
        // Open-time 64-bit check kept the slot!
        assert_eq!(rf.location_entry(ChunkPos::new(0, 0)), entry);
        // Probe returns false, full read returns RegionOffsetOverflow!
        assert!(!rf.has_chunk(ChunkPos::new(0, 0)));
        assert!(matches!(
            rf.read_chunk(ChunkPos::new(0, 0)),
            Err(StorageError::RegionOffsetOverflow {
                sector: MAX_VANILLA_SECTOR_NUMBER
            })
        ));
        rf.close().unwrap();
    }
}

// --- Edge case 19: oversized NBT strings (> 65,535 modified UTF-8 bytes) replaced with "" ---

#[test]
fn oversized_nbt_strings_replaced_with_empty_on_chunk_write() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let opts = RegionFileOptions {
        sync_writes: false,
        compression: CompressionId::Zlib,
    };

    let mut chunk = Compound::new();
    chunk.insert("ok", Tag::from("short"));
    chunk.insert("too_long_ascii", Tag::from("a".repeat(70_000)));
    // 30,000 U+0000 characters = 60,000 modified UTF-8 bytes (fits!),
    // 35,000 U+0000 characters = 70,000 modified UTF-8 bytes (overflows -> "").
    chunk.insert("nul_fits", Tag::from("\0".repeat(30_000)));
    chunk.insert("nul_overflow", Tag::from("\0".repeat(35_000)));
    // Oversized key as well
    chunk.insert("k".repeat(70_000), Tag::Int(99));

    let pos = ChunkPos::new(0, 0);
    let mut rf = RegionFile::open_in_dir(dir, RegionPos::new(0, 0), opts).unwrap();
    rf.write_chunk(pos, &chunk).unwrap();
    let loaded = rf.read_chunk(pos).unwrap().unwrap();
    rf.close().unwrap();

    assert_eq!(loaded.get("ok").and_then(Tag::as_str), Some("short"));
    assert_eq!(loaded.get("too_long_ascii").and_then(Tag::as_str), Some(""));
    assert_eq!(
        loaded.get("nul_fits").and_then(Tag::as_str).map(str::len),
        Some(30_000)
    );
    assert_eq!(loaded.get("nul_overflow").and_then(Tag::as_str), Some(""));
    assert_eq!(loaded.get("").and_then(Tag::as_int), Some(99));
}

#[test]
fn region_open_requires_existing_external_directory() {
    let tmp = TempDir::new().unwrap();
    let missing_dir = tmp.path().join("does_not_exist");
    let res = RegionFile::open(
        &tmp.path().join("r.0.0.mca"),
        &missing_dir,
        RegionFileOptions::default(),
    );
    assert!(matches!(res, Err(StorageError::DirectoryNotFound(_))));
}
