use pumpkin_nbt::compound::NbtCompound;
use std::fs::{File, create_dir_all};
use std::io;
use std::path::PathBuf;
use tracing::{debug, error, warn};
use uuid::Uuid;

const PLAYER_DATA_SUFFIX: &str = ".dat";

/// Manages the storage and retrieval of player data from disk and memory cache.
///
/// This struct provides functions to load and save player data to/from NBT files,
/// with a memory cache to handle player disconnections temporarily.
pub struct PlayerDataStorage {
    /// Path to the directory where player data is stored
    data_path: PathBuf,
    /// Whether player data saving is enabled
    save_enabled: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum PlayerDataError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("NBT error: {0}")]
    Nbt(String),
}

impl PlayerDataStorage {
    /// Creates a new `PlayerDataStorage` with the specified data path and cache expiration time.
    pub fn new(data_path: impl Into<PathBuf>, enabled: bool) -> Self {
        let path = data_path.into();
        if !path.exists()
            && let Err(e) = create_dir_all(&path)
        {
            error!(
                "Failed to create player data directory at {}: {e}",
                path.display()
            );
        }

        Self {
            data_path: path,
            save_enabled: enabled,
        }
    }

    #[must_use]
    pub const fn get_data_path(&self) -> &PathBuf {
        &self.data_path
    }

    #[must_use]
    pub const fn is_save_enabled(&self) -> bool {
        self.save_enabled
    }

    pub const fn set_save_enabled(&mut self, enabled: bool) {
        self.save_enabled = enabled;
    }

    /// Returns the path for a player's data file based on their UUID.
    #[must_use]
    pub fn get_player_data_path(&self, uuid: &Uuid) -> PathBuf {
        self.get_data_path()
            .join(format!("{uuid}{PLAYER_DATA_SUFFIX}"))
    }

    /// Copies an unreadable player data file aside, mirroring vanilla's `PlayerDataStorage.backup`.
    ///
    /// The next save replaces the original file, so without this copy a corrupted
    /// inventory would be gone for good.
    fn backup(&self, uuid: &Uuid) {
        let path = self.get_player_data_path(uuid);
        if !path.is_file() {
            return;
        }

        // The timestamp format is vanilla's `FileNameDateFormatter.FORMATTER`.
        let now = time::OffsetDateTime::now_utc();
        let timestamp = format!(
            "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}",
            now.year(),
            now.month() as u8,
            now.day(),
            now.hour(),
            now.minute(),
            now.second()
        );
        let backup_path = self
            .get_data_path()
            .join(format!("{uuid}_corrupted_{timestamp}{PLAYER_DATA_SUFFIX}"));

        match std::fs::copy(&path, &backup_path) {
            Ok(_) => warn!(
                "Copied the unreadable player data for {uuid} to {}",
                backup_path.display()
            ),
            Err(e) => warn!("Failed to copy the player data file for {uuid}: {e}"),
        }
    }

    /// Loads player data from NBT file or cache.
    ///
    /// This function first checks if player data exists in the cache.
    /// If not, it attempts to load the data from a .dat file on disk.
    ///
    /// # Arguments
    ///
    /// * `uuid` - The UUID of the player to load data for.
    ///
    /// # Returns
    ///
    /// A Result containing either the player's NBT data or an error.
    pub fn load_player_data(&self, uuid: &Uuid) -> Result<(bool, NbtCompound), PlayerDataError> {
        // If player data saving is disabled, return empty data
        if !self.is_save_enabled() {
            return Ok((false, NbtCompound::new()));
        }

        // If not in cache, load from disk
        let path = self.get_player_data_path(uuid);
        if !path.exists() {
            debug!("No player data file found for {uuid}");
            return Ok((false, NbtCompound::new()));
        }

        let file = match File::open(&path) {
            Ok(file) => file,
            Err(e) => {
                error!("Failed to open player data file for {uuid}: {e}");
                self.backup(uuid);
                return Err(PlayerDataError::Io(e));
            }
        };

        match pumpkin_nbt::nbt_compress::read_gzip_compound_tag(file) {
            Ok(nbt) => {
                debug!("Loaded player data for {uuid} from disk");
                Ok((true, nbt))
            }
            Err(e) => {
                error!("Failed to read player data for {uuid}: {e}");
                self.backup(uuid);
                Err(PlayerDataError::Nbt(e.to_string()))
            }
        }
    }

    /// Saves player data to NBT file and updates cache.
    ///
    /// This function saves the player's data to a .dat file on disk and also
    /// updates the in-memory cache with the latest data.
    ///
    /// # Arguments
    ///
    /// * `uuid` - The UUID of the player to save data for.
    /// * `data` - The NBT compound data to save.
    ///
    /// # Returns
    ///
    /// A Result indicating success or the error that occurred.
    pub fn save_player_data(&self, uuid: &Uuid, data: NbtCompound) -> Result<(), PlayerDataError> {
        // Skip saving if disabled in config
        if !self.is_save_enabled() {
            return Ok(());
        }

        let path = self.get_player_data_path(uuid);

        // Ensure parent directory exists
        if let Some(parent) = path.parent()
            && let Err(e) = create_dir_all(parent)
        {
            error!("Failed to create player data directory for {uuid}: {e}");
            return Err(PlayerDataError::Io(e));
        }

        // Create the file and write directly with GZip compression
        match File::create(&path) {
            Ok(file) => {
                if let Err(e) = pumpkin_nbt::nbt_compress::write_gzip_compound_tag(data, file) {
                    error!("Failed to write compressed player data for {uuid}: {e}");
                    Err(PlayerDataError::Nbt(e.to_string()))
                } else {
                    debug!("Saved player data for {uuid} to disk");
                    Ok(())
                }
            }
            Err(e) => {
                error!("Failed to create player data file for {uuid}: {e}");
                Err(PlayerDataError::Io(e))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn unreadable_player_data_is_kept_as_a_backup() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage = PlayerDataStorage::new(temp_dir.path(), true);
        let uuid = Uuid::new_v4();

        let corrupt = b"not a gzip file".as_slice();
        File::create(storage.get_player_data_path(&uuid))
            .unwrap()
            .write_all(corrupt)
            .unwrap();

        assert!(storage.load_player_data(&uuid).is_err());

        let prefix = format!("{uuid}_corrupted_");
        let backups: Vec<_> = std::fs::read_dir(temp_dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
            .collect();

        assert_eq!(backups.len(), 1);
        assert_eq!(std::fs::read(backups[0].path()).unwrap(), corrupt);
    }
}
