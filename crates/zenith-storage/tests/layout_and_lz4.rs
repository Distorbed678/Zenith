use std::io::{Cursor, Read, Write};
use std::path::Path;

use zenith_storage::{
    ChunkPos, Identifier, LZ4_BLOCK_MAGIC, LZ4_CHECKSUM_MASK, LZ4_DEFAULT_BLOCK_SIZE,
    LZ4_METHOD_LZ4, LZ4_METHOD_RAW, LZ4_XXHASH32_SEED, Lz4BlockReader, Lz4BlockWriter, RegionPos,
    RegionStoreKind, StorageError, WorldLayout, compression_level_for_block_size,
    lz4_block_checksum, resolve_identifier_path,
};

// --- Edge case 1: 26.3 unified dimension & player directories + path-escape validation ---

#[test]
fn layout_26_3_paths_and_builtins() {
    let world = WorldLayout::new("/saves/test_world");

    assert_eq!(
        world.session_lock(),
        Path::new("/saves/test_world/session.lock")
    );
    assert_eq!(world.level_dat(), Path::new("/saves/test_world/level.dat"));
    assert_eq!(
        world.level_dat_old(),
        Path::new("/saves/test_world/level.dat_old")
    );
    assert_eq!(
        world.level_dat_corrupted("2026-11-24_20-35-00"),
        Path::new("/saves/test_world/level.dat_corrupted_2026-11-24_20-35-00")
    );
    assert_eq!(world.icon_png(), Path::new("/saves/test_world/icon.png"));
    assert_eq!(
        world.players_data_dir(),
        Path::new("/saves/test_world/players/data")
    );
    assert_eq!(
        world.players_advancements_dir(),
        Path::new("/saves/test_world/players/advancements")
    );
    assert_eq!(
        world.players_stats_dir(),
        Path::new("/saves/test_world/players/stats")
    );
    assert_eq!(
        world.bundled_resourcepack(),
        Path::new("/saves/test_world/resourcepacks/resources.zip")
    );

    let uuid = "069a79f4-44e9-4726-a5be-fca90e38aaf5";
    assert_eq!(
        world.player_dat(uuid).unwrap(),
        Path::new("/saves/test_world/players/data/069a79f4-44e9-4726-a5be-fca90e38aaf5.dat")
    );
    assert_eq!(
        world.player_dat_old(uuid).unwrap(),
        Path::new("/saves/test_world/players/data/069a79f4-44e9-4726-a5be-fca90e38aaf5.dat_old")
    );
    assert_eq!(
        world
            .player_dat_corrupted(uuid, "2026-11-24_20-35-00")
            .unwrap(),
        Path::new(
            "/saves/test_world/players/data/069a79f4-44e9-4726-a5be-fca90e38aaf5_corrupted_2026-11-24_20-35-00.dat"
        )
    );

    // Built-in 26.3 dimensions live under dimensions/minecraft/{overworld,the_nether,the_end}/
    let ow = world.dimension(&Identifier::overworld()).unwrap();
    assert_eq!(
        ow.root(),
        Path::new("/saves/test_world/dimensions/minecraft/overworld")
    );
    assert_eq!(
        ow.region_dir(),
        Path::new("/saves/test_world/dimensions/minecraft/overworld/region")
    );
    assert_eq!(
        ow.entities_dir(),
        Path::new("/saves/test_world/dimensions/minecraft/overworld/entities")
    );
    assert_eq!(
        ow.poi_dir(),
        Path::new("/saves/test_world/dimensions/minecraft/overworld/poi")
    );
    assert_eq!(ow.store_dir(RegionStoreKind::Terrain), ow.region_dir());
    assert_eq!(ow.store_dir(RegionStoreKind::Entities), ow.entities_dir());
    assert_eq!(ow.store_dir(RegionStoreKind::Poi), ow.poi_dir());

    let nether = world.dimension(&Identifier::the_nether()).unwrap();
    assert_eq!(
        nether.root(),
        Path::new("/saves/test_world/dimensions/minecraft/the_nether")
    );
    let end = world.dimension(&Identifier::the_end()).unwrap();
    assert_eq!(
        end.root(),
        Path::new("/saves/test_world/dimensions/minecraft/the_end")
    );

    // World-scoped and dimension-scoped data/<ns>/<path>.dat
    let scoreboard = Identifier::parse("minecraft:scoreboard").unwrap();
    assert_eq!(
        world.data_file(&scoreboard).unwrap(),
        Path::new("/saves/test_world/data/minecraft/scoreboard.dat")
    );
    let raids = Identifier::parse("minecraft:raids/custom").unwrap();
    assert_eq!(
        ow.data_file(&raids).unwrap(),
        Path::new(
            "/saves/test_world/dimensions/minecraft/overworld/data/minecraft/raids/custom.dat"
        )
    );
}

#[test]
fn identifier_validation_and_path_escape_rejection() {
    assert!(Identifier::is_valid("minecraft:overworld"));
    assert!(Identifier::is_valid("my_mod-1.0:sub/path_2.0"));
    assert!(!Identifier::is_valid("no_colon"));
    assert!(!Identifier::is_valid(":empty_ns"));
    assert!(!Identifier::is_valid("empty_path:"));
    assert!(!Identifier::is_valid("UpperCase:foo"));
    assert!(!Identifier::is_valid("ns:a:b"));
    assert!(!Identifier::is_valid("ns:space path"));

    let world = WorldLayout::new("/saves/world");

    // Path traversal escaping root via `..` in path or namespace must be rejected.
    let escape_path = Identifier::parse("minecraft:../../etc/passwd").unwrap();
    assert!(matches!(
        world.dimension_dir(&escape_path),
        Err(StorageError::PathEscape { .. })
    ));
    assert!(matches!(
        world.data_file(&escape_path),
        Err(StorageError::PathEscape { .. })
    ));

    let escape_ns = Identifier::parse("..:outside").unwrap();
    assert!(matches!(
        world.dimension_dir(&escape_ns),
        Err(StorageError::PathEscape { .. })
    ));

    let leading_slash = Identifier::parse("minecraft:/abs/path").unwrap();
    assert!(matches!(
        resolve_identifier_path(Path::new("/saves/world/data"), &leading_slash, ".dat"),
        Err(StorageError::PathEscape { .. })
    ));

    let equals_root = Identifier::parse("minecraft:..").unwrap();
    assert!(matches!(
        world.dimension_dir(&equals_root),
        Err(StorageError::PathEscape { .. })
    ));

    // Player UUID traversal rejected.
    assert!(matches!(
        world.player_dat("../evil"),
        Err(StorageError::PathEscape { .. })
    ));
    assert!(matches!(
        world.player_dat("sub/uuid"),
        Err(StorageError::PathEscape { .. })
    ));
}

// --- Edge case 2: negative chunk coordinates and filename parsing ---

#[test]
fn negative_chunk_coordinates_and_region_filename_parsing() {
    let neg = ChunkPos::new(-1, -1);
    assert_eq!(neg.region_pos(), RegionPos::new(-1, -1));
    assert_eq!(neg.local_x(), 31);
    assert_eq!(neg.local_z(), 31);
    assert_eq!(neg.header_index(), 1023);
    assert_eq!(neg.region_pos().filename(), "r.-1.-1.mca");
    assert_eq!(neg.external_filename(), "c.-1.-1.mcc");

    let edge = ChunkPos::new(-32, -32);
    assert_eq!(edge.region_pos(), RegionPos::new(-1, -1));
    assert_eq!(edge.local_x(), 0);
    assert_eq!(edge.local_z(), 0);
    assert_eq!(edge.header_index(), 0);

    let edge2 = ChunkPos::new(-33, 32);
    assert_eq!(edge2.region_pos(), RegionPos::new(-2, 1));
    assert_eq!(edge2.local_x(), 31);
    assert_eq!(edge2.local_z(), 0);
    assert_eq!(edge2.header_index(), 31);

    // Packed LRU key round trip with negative coords
    let rp = RegionPos::new(-1, -42);
    assert_eq!(RegionPos::unpack_key(rp.pack_key()), rp);
    assert_eq!(
        rp.pack_key(),
        (u32::MAX as u64) | (((-42i32) as u32 as u64) << 32)
    );

    // Filename regex `^r\.(-?[0-9]+)\.(-?[0-9]+)\.mca$`
    assert_eq!(
        RegionPos::parse_filename("r.0.0.mca"),
        Some(RegionPos::new(0, 0))
    );
    assert_eq!(
        RegionPos::parse_filename("r.-1.-12.mca"),
        Some(RegionPos::new(-1, -12))
    );
    assert_eq!(RegionPos::parse_filename("r.+1.0.mca"), None);
    assert_eq!(RegionPos::parse_filename("r.1.0.mcc"), None);
    assert_eq!(RegionPos::parse_filename("r.1.0.mca.tmp"), None);
    assert_eq!(RegionPos::parse_filename("r.1.2.3.mca"), None);
    assert_eq!(RegionPos::parse_filename("r..0.mca"), None);
}

// --- Edge case 9: hand-made lz4-java LZ4Block framing test vectors ---

#[test]
fn lz4_block_hand_made_raw_and_compressed_vectors() {
    assert_eq!(compression_level_for_block_size(LZ4_DEFAULT_BLOCK_SIZE), 6);
    assert_eq!(compression_level_for_block_size(1024), 0);
    assert_eq!(compression_level_for_block_size(64), 0);
    assert_eq!(compression_level_for_block_size(2048), 1);

    // Verify 28-bit masking clears the top 4 bits of xxHash32(seed = 0x9747B28C).
    // Find a small payload whose unmasked xxHash32 has non-zero top 4 bits.
    let mut masked_sample = Vec::new();
    for b in 0u8..=255 {
        let candidate = [b, b ^ 0x5A, b.wrapping_mul(13), 0x42];
        let full = xxhash_rust::xxh32::xxh32(&candidate, LZ4_XXHASH32_SEED);
        if (full & 0xF000_0000) != 0 {
            masked_sample = candidate.to_vec();
            assert_eq!(lz4_block_checksum(&masked_sample), full & LZ4_CHECKSUM_MASK);
            assert_eq!(lz4_block_checksum(&masked_sample) & 0xF000_0000, 0);
            break;
        }
    }
    assert!(!masked_sample.is_empty());

    // 1. Hand-made RAW block vector (`token = 0x16`) + 21-byte zero-length terminator block.
    let mut raw_stream = Vec::new();
    raw_stream.extend_from_slice(&LZ4_BLOCK_MAGIC);
    raw_stream.push(LZ4_METHOD_RAW | 6); // 0x16
    raw_stream.extend_from_slice(&(masked_sample.len() as u32).to_le_bytes());
    raw_stream.extend_from_slice(&(masked_sample.len() as u32).to_le_bytes());
    raw_stream.extend_from_slice(&lz4_block_checksum(&masked_sample).to_le_bytes());
    raw_stream.extend_from_slice(&masked_sample);
    // Terminator block: 21 bytes
    raw_stream.extend_from_slice(&LZ4_BLOCK_MAGIC);
    raw_stream.push(LZ4_METHOD_RAW | 6); // 0x16
    raw_stream.extend_from_slice(&0u32.to_le_bytes());
    raw_stream.extend_from_slice(&0u32.to_le_bytes());
    raw_stream.extend_from_slice(&0u32.to_le_bytes());

    let mut decoded = Vec::new();
    Lz4BlockReader::new(Cursor::new(&raw_stream))
        .read_to_end(&mut decoded)
        .unwrap();
    assert_eq!(decoded, masked_sample);

    // 2. Hand-made LZ4-compressed block vector (`token = 0x26`):
    //    Payload: 20 bytes of 'A' (0x41).
    //    Raw LZ4 block:
    //      token = 0x1F (1 literal, match length = 15 + 4 = 19, extra byte 0 -> total 1 + 19 = 20?
    //      Actually: 1 literal 'A', offset = 1 (0x01 0x00), match length = 15 + 4 + 0? Wait:
    //      In LZ4, last 5 bytes MUST be literals! Let's encode 25 'A's:
    //      - token = 0x1B (1 literal 'A', match len = 11 + 4 = 15), offset = 1 -> 16 'A's
    //      - token = 0x90 (9 trailing literals 'A' * 9, no match) -> 25 'A's total!
    //      Compressed length = 1 + 1 + 2 + 1 + 9 = 14 bytes (< 25 bytes!).
    let expected_aaa = vec![b'A'; 25];
    let hand_lz4_payload: Vec<u8> = vec![
        0x1B, b'A', 0x01, 0x00, // 1 literal + match len 15 at offset 1 => 16 'A's
        0x90, b'A', b'A', b'A', b'A', b'A', b'A', b'A', b'A', b'A', // 9 trailing literals
    ];
    let mut lz4_stream = Vec::new();
    lz4_stream.extend_from_slice(&LZ4_BLOCK_MAGIC);
    lz4_stream.push(LZ4_METHOD_LZ4 | 6); // 0x26
    lz4_stream.extend_from_slice(&(hand_lz4_payload.len() as u32).to_le_bytes());
    lz4_stream.extend_from_slice(&(expected_aaa.len() as u32).to_le_bytes());
    lz4_stream.extend_from_slice(&lz4_block_checksum(&expected_aaa).to_le_bytes());
    lz4_stream.extend_from_slice(&hand_lz4_payload);
    // Terminator
    lz4_stream.extend_from_slice(&LZ4_BLOCK_MAGIC);
    lz4_stream.push(0x16);
    lz4_stream.extend_from_slice(&[0u8; 12]);

    let mut decoded_aaa = Vec::new();
    Lz4BlockReader::new(Cursor::new(&lz4_stream))
        .read_to_end(&mut decoded_aaa)
        .unwrap();
    assert_eq!(decoded_aaa, expected_aaa);

    // 3. Writer verifies incompressible fallback (0x16), compressible block (0x26),
    //    multi-block streaming (> 65,536 bytes), syncFlush=false, and exact terminator.
    let mut incompressible = Vec::with_capacity(128);
    let mut x = 0x1234_5678u32;
    for _ in 0..128 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        incompressible.push((x & 0xFF) as u8);
    }
    let mut writer = Lz4BlockWriter::new(Vec::new());
    writer.write_all(&incompressible).unwrap();
    // syncFlush = false: flush must NOT emit a partial block before finish().
    writer.flush().unwrap();
    let encoded_incomp = writer.finish().unwrap();
    assert_eq!(&encoded_incomp[0..8], &LZ4_BLOCK_MAGIC);
    assert_eq!(encoded_incomp[8], 0x16); // raw fallback!
    // Last 21 bytes are the zero-length terminator block.
    let term = &encoded_incomp[encoded_incomp.len() - 21..];
    assert_eq!(&term[0..8], &LZ4_BLOCK_MAGIC);
    assert_eq!(term[8], 0x16);
    assert_eq!(&term[9..21], &[0u8; 12]);

    // Multi-block (> 65,536 bytes) compressible round trip
    let mut big = Vec::with_capacity(150_000);
    for i in 0..150_000usize {
        big.push(((i / 17) & 0xFF) as u8);
    }
    let mut w = Lz4BlockWriter::new(Vec::new());
    w.write_all(&big).unwrap();
    let encoded_big = w.finish().unwrap();
    assert_eq!(&encoded_big[0..8], &LZ4_BLOCK_MAGIC);
    assert_eq!(encoded_big[8], 0x26); // LZ4 compressed!
    let mut roundtrip = Vec::new();
    Lz4BlockReader::new(Cursor::new(&encoded_big))
        .read_to_end(&mut roundtrip)
        .unwrap();
    assert_eq!(roundtrip, big);
}

#[test]
fn lz4_block_rejects_corrupt_headers_and_unmasked_checksums() {
    let payload = b"hello world hello world hello world";
    let mut w = Lz4BlockWriter::new(Vec::new());
    w.write_all(payload).unwrap();
    let valid = w.finish().unwrap();

    // Bad magic
    let mut bad_magic = valid.clone();
    bad_magic[0] = b'X';
    assert!(
        Lz4BlockReader::new(Cursor::new(&bad_magic))
            .read_to_end(&mut Vec::new())
            .is_err()
    );

    // Bad method nibble (0x30)
    let mut bad_method = valid.clone();
    bad_method[8] = 0x36;
    assert!(
        Lz4BlockReader::new(Cursor::new(&bad_method))
            .read_to_end(&mut Vec::new())
            .is_err()
    );

    // Non-zero top 4 bits in checksum (violates 28-bit mask)
    let mut bad_cksum = valid.clone();
    bad_cksum[20] |= 0xF0;
    assert!(
        Lz4BlockReader::new(Cursor::new(&bad_cksum))
            .read_to_end(&mut Vec::new())
            .is_err()
    );

    // Non-zero checksum on terminator
    let mut bad_term = valid.clone();
    let len = bad_term.len();
    bad_term[len - 1] = 1;
    assert!(
        Lz4BlockReader::new(Cursor::new(&bad_term))
            .read_to_end(&mut Vec::new())
            .is_err()
    );

    // Decompressed length exceeding max block size for level 0 (1024 B)
    let mut over_max = Vec::new();
    over_max.extend_from_slice(&LZ4_BLOCK_MAGIC);
    over_max.push(LZ4_METHOD_RAW); // level 0 => max 1024
    over_max.extend_from_slice(&1025u32.to_le_bytes());
    over_max.extend_from_slice(&1025u32.to_le_bytes());
    over_max.extend_from_slice(&0u32.to_le_bytes());
    assert!(
        Lz4BlockReader::new(Cursor::new(&over_max))
            .read_to_end(&mut Vec::new())
            .is_err()
    );

    // Raw block with compressed_len != decompressed_len
    let mut raw_mismatch = Vec::new();
    raw_mismatch.extend_from_slice(&LZ4_BLOCK_MAGIC);
    raw_mismatch.push(0x16);
    raw_mismatch.extend_from_slice(&4u32.to_le_bytes());
    raw_mismatch.extend_from_slice(&5u32.to_le_bytes());
    raw_mismatch.extend_from_slice(&0u32.to_le_bytes());
    assert!(
        Lz4BlockReader::new(Cursor::new(&raw_mismatch))
            .read_to_end(&mut Vec::new())
            .is_err()
    );

    // Negative length (high bit set in LE i32)
    let mut neg_len = Vec::new();
    neg_len.extend_from_slice(&LZ4_BLOCK_MAGIC);
    neg_len.push(0x16);
    neg_len.extend_from_slice(&(-1i32).to_le_bytes());
    neg_len.extend_from_slice(&(-1i32).to_le_bytes());
    neg_len.extend_from_slice(&0u32.to_le_bytes());
    assert!(
        Lz4BlockReader::new(Cursor::new(&neg_len))
            .read_to_end(&mut Vec::new())
            .is_err()
    );
}
