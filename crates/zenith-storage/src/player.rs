//! Per-player data storage (`<world>/players/data/<uuid>.dat`).

use std::fs::{self, File};
use std::io::BufReader;
use std::path::Path;

use flate2::read::GzDecoder;
use zenith_nbt::Compound;

use crate::compression::STREAM_BUFFER_SIZE;
use crate::error::Result;
use crate::fs_util::{
    copy_file_with_attributes, create_dir_all_resolving_symlinks, create_temp_file_in,
    format_quarantine_timestamp, is_regular_file,
};
use crate::layout::WorldLayout;
use crate::nbt_io::{read_named_compound_unlimited, write_gzip_named_compound_fallback};
use crate::safe_replace::safe_replace_file;

/// Manages `<world>/players/data/` per-player gzip-compressed NBT files.
#[derive(Debug, Clone)]
pub struct PlayerDataStorage {
    layout: WorldLayout,
}

impl PlayerDataStorage {
    /// Initializes player data storage, creating `<world>/players/data/` if missing.
    pub fn new(layout: &WorldLayout) -> Result<Self> {
        create_dir_all_resolving_symlinks(&layout.players_data_dir())?;
        Ok(Self {
            layout: layout.clone(),
        })
    }

    /// Path to `<world>/players/data/`.
    #[must_use]
    pub fn dir(&self) -> std::path::PathBuf {
        self.layout.players_data_dir()
    }

    /// Writes `<uuid>.dat` using the spec's exact sequence:
    /// 1. Create temp file in `players/data/` with prefix `"<uuid>-"` and suffix `".dat"` (`O_SYNC`).
    /// 2. Write gzip-compressed player NBT with oversized-string-to-empty fallback.
    /// 3. Run [`safe_replace_file`] with `(<uuid>.dat, tmp, <uuid>.dat_old, no_rollback = false)`.
    pub fn write_player_dat(&self, uuid: &str, data: &Compound) -> Result<()> {
        let target = self.layout.player_dat(uuid)?;
        let backup = self.layout.player_dat_old(uuid)?;
        let dir = self.layout.players_data_dir();
        create_dir_all_resolving_symlinks(&dir)?;

        let prefix = format!("{uuid}-");
        let (temp_path, temp_file) = create_temp_file_in(&dir, &prefix, ".dat", true)?;

        if let Err(err) = write_gzip_named_compound_fallback(temp_file, data) {
            let _ = fs::remove_file(&temp_path);
            return Err(err);
        }

        safe_replace_file(&target, &temp_path, &backup, false)
    }

    /// Same as [`PlayerDataStorage::write_player_dat`], catching and logging any error as a warning.
    pub fn save_player(&self, uuid: &str, data: &Compound) {
        if let Err(err) = self.write_player_dat(uuid, data) {
            log::warn!("Failed to save player data for {uuid}: {err}");
        }
    }

    /// Loads player data for `uuid` using the spec's player-specific corruption policy:
    /// 1. If `<uuid>.dat` is a regular file, reads it gzip-compressed with unlimited heap quota;
    ///    on any error, logs a warning and treats as missing.
    /// 2. If step 1 returned `None` (missing OR corrupt):
    ///    - If `<uuid>.dat` is a regular file, **copies** it (preserving attributes) to
    ///      `<uuid>_corrupted_<YYYY-MM-DD_HH-mm-ss>.dat` (leaving `<uuid>.dat` in place).
    ///    - Attempts to read `<uuid>.dat_old` with unlimited heap quota WITHOUT replacing
    ///      `<uuid>.dat` and WITHOUT backing up a corrupt `<uuid>.dat_old`.
    #[must_use]
    pub fn load_player(&self, uuid: &str) -> Option<Compound> {
        self.load_player_with_timestamp(uuid, &format_quarantine_timestamp())
    }

    /// Same as [`PlayerDataStorage::load_player`] with an explicit quarantine timestamp string
    /// (format `YYYY-MM-DD_HH-mm-ss`, useful for deterministic tests).
    #[must_use]
    pub fn load_player_with_timestamp(&self, uuid: &str, timestamp: &str) -> Option<Compound> {
        let primary = match self.layout.player_dat(uuid) {
            Ok(p) => p,
            Err(err) => {
                log::warn!("Invalid player UUID {uuid:?}: {err}");
                return None;
            }
        };

        if is_regular_file(&primary) {
            match read_gzip_player_file(&primary) {
                Ok(compound) => return Some(compound),
                Err(err) => {
                    log::warn!("Failed to load player data {primary:?}: {err}");
                }
            }
        }

        // Step 2: primary missing or corrupt.
        if is_regular_file(&primary) {
            match self.layout.player_dat_corrupted(uuid, timestamp) {
                Ok(corrupted_copy) => {
                    if let Err(err) = copy_file_with_attributes(&primary, &corrupted_copy) {
                        log::warn!(
                            "Failed to copy corrupted player data {primary:?} to {corrupted_copy:?}: {err}"
                        );
                    }
                }
                Err(err) => {
                    log::warn!("Failed to compute corrupted player path for {uuid}: {err}");
                }
            }
        }

        let backup = self.layout.player_dat_old(uuid).ok()?;
        if is_regular_file(&backup) {
            match read_gzip_player_file(&backup) {
                Ok(compound) => return Some(compound),
                Err(err) => {
                    log::warn!("Failed to load backup player data {backup:?}: {err}");
                }
            }
        }

        None
    }
}

fn read_gzip_player_file(path: &Path) -> Result<Compound> {
    let file = File::open(path)?;
    let mut decoder = BufReader::with_capacity(STREAM_BUFFER_SIZE, GzDecoder::new(file));
    read_named_compound_unlimited(&mut decoder)
}
