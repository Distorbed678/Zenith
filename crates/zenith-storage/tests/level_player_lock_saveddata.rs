use std::fs;
use std::io::Write;
use std::time::Duration;

use tempfile::TempDir;
use zenith_nbt::{Compound, List, Tag};
use zenith_storage::{
    AllowedSymlinks, DATA_VERSION_26_3, LEVEL_STORAGE_VERSION_ANVIL,
    LEVEL_STORAGE_VERSION_MCREGION, PlayerDataStorage, SAVED_DATA_DEFAULT_VERSION,
    SESSION_LOCK_BYTES, SessionLock, StorageError, WorldLayout, WorldSummary,
    append_version_history, delete_world_dir_with_retry_delay, is_world_candidate,
    list_world_summaries, read_level_dat, read_level_dat_with_fallback_at, read_saved_data,
    safe_replace_file, scan_world_summary, write_level_dat, write_saved_data_26_3,
};

fn level_data(version: i32, last_played: i64, name: &str) -> Compound {
    let mut data = Compound::new();
    data.insert("version", Tag::Int(version));
    data.insert("DataVersion", Tag::Int(DATA_VERSION_26_3));
    data.insert("LastPlayed", Tag::Long(last_played));
    data.insert("LevelName", Tag::from(name));
    data
}

// --- Edge case 23: safe file replacement protocol ---

#[test]
fn safe_replace_rotates_backup_and_respects_rollback_flag() {
    let tmp = TempDir::new().unwrap();
    let target = tmp.path().join("level.dat");
    let backup = tmp.path().join("level.dat_old");
    let new1 = tmp.path().join("tmp1.dat");
    let new2 = tmp.path().join("tmp2.dat");

    // First install (no pre-existing target)
    fs::write(&new1, b"v1").unwrap();
    safe_replace_file(&target, &new1, &backup, false).unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"v1");
    assert!(!backup.exists());

    // Second install rotates v1 -> backup and installs v2 -> target
    fs::write(&new2, b"v2").unwrap();
    safe_replace_file(&target, &new2, &backup, false).unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"v2");
    assert_eq!(fs::read(&backup).unwrap(), b"v1");

    // Failing install (missing new_file) with no_rollback = false rolls back target from backup!
    let missing_new = tmp.path().join("missing.dat");
    let err = safe_replace_file(&target, &missing_new, &backup, false).unwrap_err();
    assert!(matches!(
        err,
        StorageError::SafeReplaceFailed {
            phase: "install",
            ..
        }
    ));
    // Phase 1 moved v2 -> backup; phase 3 failed; rollback moved backup (v2) -> target!
    assert_eq!(fs::read(&target).unwrap(), b"v2");
    assert!(!backup.exists());

    // Failing install with no_rollback = true leaves backup in place and target absent!
    let err2 = safe_replace_file(&target, &missing_new, &backup, true).unwrap_err();
    assert!(matches!(
        err2,
        StorageError::SafeReplaceFailed {
            phase: "install",
            ..
        }
    ));
    assert!(!target.exists());
    assert_eq!(fs::read(&backup).unwrap(), b"v2");
}

// --- Edge case 24: session.lock exact bytes, fsync-before-lock, probe, persistence, delete ---

#[test]
fn session_lock_bytes_probe_persistence_and_world_deletion() {
    let tmp = TempDir::new().unwrap();
    let world_dir = tmp.path().join("world");

    // Non-destructive probe on absent world returns false and does NOT create session.lock!
    assert!(!SessionLock::is_locked(&world_dir).unwrap());
    assert!(!world_dir.exists());

    let mut lock = SessionLock::acquire(&world_dir).unwrap();
    assert!(lock.is_valid());
    let lock_path = world_dir.join("session.lock");
    assert_eq!(fs::read(&lock_path).unwrap(), SESSION_LOCK_BYTES);
    assert_eq!(SESSION_LOCK_BYTES, [0xE2, 0x98, 0x83]);

    // While held: probe returns true, second acquire fails with LockHeld.
    assert!(SessionLock::is_locked(&world_dir).unwrap());
    assert!(matches!(
        SessionLock::acquire(&world_dir),
        Err(StorageError::LockHeld(_))
    ));

    // Releasing unlocks while leaving session.lock on disk!
    lock.release().unwrap();
    assert!(!lock.is_valid());
    assert!(matches!(
        lock.check_valid(),
        Err(StorageError::LockInvalidated(_))
    ));
    assert!(lock_path.is_file());
    assert!(!SessionLock::is_locked(&world_dir).unwrap());

    // Acquire does NOT truncate existing trailing bytes in session.lock.
    fs::write(&lock_path, b"abcdef").unwrap();
    let lock2 = SessionLock::acquire(&world_dir).unwrap();
    let bytes = fs::read(&lock_path).unwrap();
    assert_eq!(&bytes[0..3], &SESSION_LOCK_BYTES);
    assert_eq!(&bytes[3..6], b"def");

    // World deletion deletes nested files first, releases lock, deletes session.lock and world dir.
    fs::create_dir_all(world_dir.join("dimensions/minecraft/overworld/region")).unwrap();
    fs::write(world_dir.join("level.dat"), b"dummy").unwrap();
    delete_world_dir_with_retry_delay(&world_dir, Some(lock2), Duration::from_millis(1)).unwrap();
    assert!(!world_dir.exists());
}

// --- Edge cases 19, 20, 21: level.dat, level.dat_old fallback, quarantine, summaries, quota ---

#[test]
fn level_dat_write_read_fallback_quarantine_and_version_history() {
    let tmp = TempDir::new().unwrap();
    let layout = WorldLayout::new(tmp.path().join("world"));
    let mut lock = SessionLock::acquire(layout.root()).unwrap();

    let mut d1 = level_data(LEVEL_STORAGE_VERSION_ANVIL, 1000, "First");
    append_version_history(&mut d1, 5000);
    append_version_history(&mut d1, 5000); // duplicate last element is NOT appended
    append_version_history(&mut d1, DATA_VERSION_26_3);
    assert_eq!(
        d1.get("version_history")
            .unwrap()
            .as_list()
            .unwrap()
            .as_slice(),
        &[Tag::Int(5000), Tag::Int(DATA_VERSION_26_3)]
    );
    // Include an oversized string to verify edge case 19 applies to level.dat too!
    d1.insert("huge", Tag::from("x".repeat(70_000)));

    write_level_dat(&layout, &d1).unwrap();
    assert!(layout.level_dat().is_file());
    assert!(!layout.level_dat_old().exists());

    let loaded1 = read_level_dat(&layout, &lock).unwrap();
    assert_eq!(
        loaded1.get("LevelName").and_then(Tag::as_str),
        Some("First")
    );
    assert_eq!(loaded1.get("huge").and_then(Tag::as_str), Some(""));

    // Second save rotates First -> level.dat_old and installs Second -> level.dat.
    let d2 = level_data(LEVEL_STORAGE_VERSION_ANVIL, 2000, "Second");
    write_level_dat(&layout, &d2).unwrap();
    assert!(layout.level_dat_old().is_file());

    // Corrupt level.dat and test automatic fallback read:
    // - moves broken level.dat -> level.dat_corrupted_<ts>
    // - moves level.dat_old -> level.dat (leaving NO level.dat_old!)
    fs::write(layout.level_dat(), b"not valid gzip").unwrap();
    let fallback = read_level_dat_with_fallback_at(&layout, &lock, "2026-11-24_21-00-00").unwrap();
    assert!(fallback.restored_from_old);
    assert_eq!(
        fallback.data.get("LevelName").and_then(Tag::as_str),
        Some("First")
    );
    let expected_quarantine = layout.level_dat_corrupted("2026-11-24_21-00-00");
    assert_eq!(
        fallback.quarantined_path.as_deref(),
        Some(expected_quarantine.as_path())
    );
    assert_eq!(fs::read(&expected_quarantine).unwrap(), b"not valid gzip");
    assert!(!layout.level_dat_old().exists()); // moved, not copied!
    assert_eq!(
        read_level_dat(&layout, &lock)
            .unwrap()
            .get("LevelName")
            .and_then(Tag::as_str),
        Some("First")
    );

    // Invalidated session lock rejects read_level_dat.
    lock.release().unwrap();
    assert!(matches!(
        read_level_dat(&layout, &lock),
        Err(StorageError::LockInvalidated(_))
    ));
}

#[test]
fn world_summaries_never_fall_back_skip_fields_validate_version_and_sort() {
    let tmp = TempDir::new().unwrap();
    let saves = tmp.path();

    // World A: valid Anvil, LastPlayed = 5000, contains large Data.Player and Data.WorldGenSettings
    let w_a = WorldLayout::new(saves.join("world_a"));
    fs::create_dir_all(w_a.root()).unwrap();
    let mut da = level_data(LEVEL_STORAGE_VERSION_ANVIL, 5000, "A");
    let mut player = Compound::new();
    player.insert("Secret", Tag::from("should_be_skipped"));
    da.insert("Player", Tag::Compound(player));
    let mut wg = Compound::new();
    wg.insert("seed", Tag::Long(12345));
    da.insert("WorldGenSettings", Tag::Compound(wg));
    write_level_dat(&w_a, &da).unwrap();

    // World B: valid McRegion (19132 -> requires_manual_conversion = true), tie LastPlayed = 5000
    let w_b = WorldLayout::new(saves.join("world_b"));
    fs::create_dir_all(w_b.root()).unwrap();
    write_level_dat(&w_b, &level_data(LEVEL_STORAGE_VERSION_MCREGION, 5000, "B")).unwrap();

    // World C: ONLY level.dat_old exists (level.dat missing) -> candidate, yields Corrupted (NEVER falls back!)
    let w_c = WorldLayout::new(saves.join("world_c"));
    fs::create_dir_all(w_c.root()).unwrap();
    write_level_dat(&w_c, &level_data(LEVEL_STORAGE_VERSION_ANVIL, 9999, "C")).unwrap();
    fs::rename(w_c.level_dat(), w_c.level_dat_old()).unwrap();
    assert!(is_world_candidate(w_c.root()));

    // World D: invalid storage version (99999) -> yields Corrupted
    let w_d = WorldLayout::new(saves.join("world_d"));
    fs::create_dir_all(w_d.root()).unwrap();
    write_level_dat(&w_d, &level_data(99999, 100, "D")).unwrap();

    // World E: newer LastPlayed = 9000 -> must sort FIRST!
    let w_e = WorldLayout::new(saves.join("world_e"));
    fs::create_dir_all(w_e.root()).unwrap();
    write_level_dat(&w_e, &level_data(LEVEL_STORAGE_VERSION_ANVIL, 9000, "E")).unwrap();

    // Non-candidate directory (no level.dat or level.dat_old) is ignored
    fs::create_dir_all(saves.join("not_a_world")).unwrap();

    let summary_a = scan_world_summary(w_a.root(), &AllowedSymlinks::disallow_all());
    match summary_a {
        WorldSummary::Valid {
            requires_manual_conversion,
            data,
            ..
        } => {
            assert!(!requires_manual_conversion);
            assert!(!data.contains_key("Player"));
            assert!(!data.contains_key("WorldGenSettings"));
            assert_eq!(data.get("LevelName").and_then(Tag::as_str), Some("A"));
        }
        other => panic!("expected Valid for world_a, got {other:?}"),
    }

    let summary_b = scan_world_summary(w_b.root(), &AllowedSymlinks::disallow_all());
    assert!(matches!(
        summary_b,
        WorldSummary::Valid {
            storage_version: LEVEL_STORAGE_VERSION_MCREGION,
            requires_manual_conversion: true,
            ..
        }
    ));

    let summary_c = scan_world_summary(w_c.root(), &AllowedSymlinks::disallow_all());
    assert!(matches!(summary_c, WorldSummary::Corrupted { last_played, .. } if last_played > 0));

    let summary_d = scan_world_summary(w_d.root(), &AllowedSymlinks::disallow_all());
    assert!(matches!(summary_d, WorldSummary::Corrupted { .. }));

    // Sorting: descending by LastPlayed, tie-broken ascending lexicographically by dir_name.
    // Note: world_c's corrupted mtime is current epoch millis (>> 9000), so inspect valid trio order:
    let all = list_world_summaries(saves, &AllowedSymlinks::disallow_all()).unwrap();
    assert_eq!(all.len(), 5);
    let valid_names: Vec<&str> = all
        .iter()
        .filter(|s| matches!(s, WorldSummary::Valid { .. }))
        .map(WorldSummary::dir_name)
        .collect();
    assert_eq!(valid_names, vec!["world_e", "world_a", "world_b"]);

    // Symlink validation on Unix
    #[cfg(unix)]
    {
        let w_sym = WorldLayout::new(saves.join("world_sym"));
        fs::create_dir_all(w_sym.root()).unwrap();
        std::os::unix::fs::symlink(w_a.level_dat(), w_sym.level_dat()).unwrap();
        let blocked = scan_world_summary(w_sym.root(), &AllowedSymlinks::disallow_all());
        assert!(matches!(blocked, WorldSummary::SymlinkWarning { .. }));

        let allowed = scan_world_summary(
            w_sym.root(),
            &AllowedSymlinks::with_allowed_prefixes(vec![saves.to_path_buf()]),
        );
        assert!(matches!(allowed, WorldSummary::Valid { .. }));
    }
}

#[test]
fn nbt_depth_and_level_quota_enforced() {
    let tmp = TempDir::new().unwrap();
    let layout = WorldLayout::new(tmp.path().join("world"));
    let lock = SessionLock::acquire(layout.root()).unwrap();

    // Build a 513-deep compound payload (exceeds 512 max depth) directly in gzip.
    let mut raw = Vec::new();
    // Root named compound ""
    raw.extend_from_slice(&[10, 0, 0]);
    for _ in 0..513 {
        raw.extend_from_slice(&[10, 0, 1, b'a']);
    }
    raw.extend(std::iter::repeat_n(0u8, 514));
    let mut gz = zenith_storage::compression::new_gzip_encoder(Vec::new());
    gz.write_all(&raw).unwrap();
    let compressed = gz.finish().unwrap();
    fs::write(layout.level_dat(), &compressed).unwrap();

    let err = read_level_dat(&layout, &lock).unwrap_err();
    assert!(matches!(
        err,
        StorageError::Nbt(zenith_nbt::Error::Accounting(
            zenith_nbt::AccountingError::MaxDepthExceeded { max_depth: 512 }
        ))
    ));

    // Also verify 100 MiB uncompressed quota on level.dat using a highly compressible ByteArray of 105 MiB!
    let mut huge_raw = Vec::new();
    huge_raw.extend_from_slice(&[10, 0, 0]); // root compound ""
    huge_raw.extend_from_slice(&[7, 0, 1, b'b']); // ByteArray "b"
    let len_bytes: i32 = 105 * 1024 * 1024;
    huge_raw.extend_from_slice(&len_bytes.to_be_bytes());
    let mut gz2 = zenith_storage::compression::new_gzip_encoder(Vec::new());
    gz2.write_all(&huge_raw).unwrap();
    let zeros = vec![0u8; 64 * 1024];
    let mut rem = len_bytes as usize;
    while rem > 0 {
        let n = rem.min(zeros.len());
        gz2.write_all(&zeros[..n]).unwrap();
        rem -= n;
    }
    gz2.write_all(&[0]).unwrap(); // End root
    let huge_gz = gz2.finish().unwrap();
    fs::write(layout.level_dat(), &huge_gz).unwrap();

    let err2 = read_level_dat(&layout, &lock).unwrap_err();
    assert!(matches!(
        err2,
        StorageError::Nbt(zenith_nbt::Error::Accounting(
            zenith_nbt::AccountingError::QuotaExceeded { .. }
        ))
    ));
}

// --- Edge case 22: player data corruption policy ---

#[test]
fn player_data_write_read_and_copy_quarantine_policy() {
    let tmp = TempDir::new().unwrap();
    let layout = WorldLayout::new(tmp.path().join("world"));
    let players = PlayerDataStorage::new(&layout).unwrap();
    assert!(layout.players_data_dir().is_dir());

    let uuid = "069a79f4-44e9-4726-a5be-fca90e38aaf5";

    let mut p1 = Compound::new();
    p1.insert("Health", Tag::Float(20.0));
    p1.insert("oversized", Tag::from("z".repeat(70_000)));
    players.write_player_dat(uuid, &p1).unwrap();

    let loaded1 = players.load_player(uuid).unwrap();
    assert_eq!(loaded1.get("Health").and_then(Tag::as_float), Some(20.0));
    assert_eq!(loaded1.get("oversized").and_then(Tag::as_str), Some(""));

    // Second save rotates p1 -> <uuid>.dat_old
    let mut p2 = Compound::new();
    p2.insert("Health", Tag::Float(12.5));
    players.write_player_dat(uuid, &p2).unwrap();
    assert!(layout.player_dat_old(uuid).unwrap().is_file());

    // Corrupt <uuid>.dat:
    // - COPIES <uuid>.dat to <uuid>_corrupted_<ts>.dat (note .dat at the END!)
    // - LEAVES <uuid>.dat in place!
    // - reads <uuid>.dat_old WITHOUT replacing <uuid>.dat!
    let primary = layout.player_dat(uuid).unwrap();
    let backup = layout.player_dat_old(uuid).unwrap();
    fs::write(&primary, b"corrupt player bytes").unwrap();

    let fallback = players
        .load_player_with_timestamp(uuid, "2026-11-24_21-15-00")
        .unwrap();
    assert_eq!(fallback.get("Health").and_then(Tag::as_float), Some(20.0));

    let quarantine = layout
        .player_dat_corrupted(uuid, "2026-11-24_21-15-00")
        .unwrap();
    assert!(
        quarantine
            .to_string_lossy()
            .ends_with("_corrupted_2026-11-24_21-15-00.dat")
    );
    assert_eq!(fs::read(&quarantine).unwrap(), b"corrupt player bytes");
    // Primary was NOT replaced by .dat_old and .dat_old still exists!
    assert_eq!(fs::read(&primary).unwrap(), b"corrupt player bytes");
    assert!(backup.is_file());

    // Corrupt .dat_old too -> returns None and does NOT create any backup of .dat_old.
    fs::write(&backup, b"corrupt old").unwrap();
    assert!(
        players
            .load_player_with_timestamp(uuid, "2026-11-24_21-16-00")
            .is_none()
    );
    let entries: Vec<_> = fs::read_dir(players.dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(!entries.iter().any(|n| n.contains("dat_old_corrupted")));
}

// --- Edge case 25: auxiliary data/*.dat dual-format read vs direct O_SYNC gzip write ---

#[test]
fn saved_data_dual_format_read_and_direct_gzip_write() {
    let tmp = TempDir::new().unwrap();
    let layout = WorldLayout::new(tmp.path().join("world"));
    let id = zenith_storage::Identifier::parse("minecraft:scoreboard").unwrap();
    let path = layout.data_file(&id).unwrap();

    // 1. Direct gzip write wraps { "data": ..., "DataVersion": 5023 }, creates parent dirs,
    //    leaves NO temp files and NO .dat_old backup, and applies oversized-string fallback.
    let mut payload = Compound::new();
    payload.insert("Objectives", Tag::List(List::new()));
    payload.insert("huge", Tag::from("q".repeat(70_000)));
    write_saved_data_26_3(&path, &payload).unwrap();
    // Overwrite a second time to verify NO .dat_old is created!
    write_saved_data_26_3(&path, &payload).unwrap();

    let dir_entries: Vec<_> = fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(dir_entries, vec!["scoreboard.dat"]);

    let read_gz = read_saved_data(&path).unwrap();
    assert!(read_gz.was_gzip);
    assert_eq!(read_gz.data_version, DATA_VERSION_26_3);
    assert_eq!(
        read_gz.data().unwrap().get("huge").and_then(Tag::as_str),
        Some("")
    );

    // 2. Uncompressed named-root NBT file with missing DataVersion defaults to 1343.
    let raw_id = zenith_storage::Identifier::parse("minecraft:legacy_raw").unwrap();
    let raw_path = layout.data_file(&raw_id).unwrap();
    let mut raw_root = Compound::new();
    let mut inner = Compound::new();
    inner.insert("value", Tag::Int(42));
    raw_root.insert("data", Tag::Compound(inner));
    let mut raw_bytes = Vec::new();
    zenith_storage::write_named_compound_fallback(&mut raw_bytes, &raw_root).unwrap();
    fs::write(&raw_path, &raw_bytes).unwrap();

    let read_raw = read_saved_data(&raw_path).unwrap();
    assert!(!read_raw.was_gzip);
    assert_eq!(read_raw.data_version, SAVED_DATA_DEFAULT_VERSION);
    assert_eq!(
        read_raw.data().unwrap().get("value").and_then(Tag::as_int),
        Some(42)
    );
}
