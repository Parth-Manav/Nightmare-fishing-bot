use crate::config::Config;
use crate::{BotError, BotResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::fs;
use tokio::sync::RwLock;

/// Daily in-memory record for a user who has already fished in the current day.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct UserData {
    /// Display name captured at the time of the fishing action.
    pub username: String,
    #[serde(rename = "fishedAt")]
    /// UTC timestamp stored as an RFC 3339 string for readable JSON recovery.
    pub fished_at: String,
}

/// Persistent user record that survives resets and bot restarts.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct PersistentUserData {
    /// Most recent display name seen for the user.
    pub username: String,
    /// Current consecutive-day streak.
    pub streak: u64,
    #[serde(rename = "lastFishedDate")]
    /// Fishing-day key for the user's most recent catch.
    pub last_fished_date: String,
    #[serde(rename = "totalCatches")]
    /// Lifetime catch count used for summaries and leaderboards.
    pub total_catches: u64,
}

/// Complete persisted bot state stored in `fishing_data.json`.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FishingData {
    #[serde(default)]
    /// Number of successful catches in the current fishing day.
    pub daily_count: u64,

    #[serde(default = "default_timestamp")]
    /// Millisecond timestamp of the last completed daily reset.
    pub last_reset_timestamp: u64,

    #[serde(default)]
    /// Users who have already fished in the current fishing day.
    pub users: HashMap<String, UserData>,

    #[serde(default)]
    /// Long-lived per-user streak and total-catch data.
    pub persistent_users: HashMap<String, PersistentUserData>,

    /// Discord message ID for the active fishing button, if configured.
    pub button_message_id: Option<String>,
    /// Discord channel ID containing the active fishing button, if configured.
    pub button_channel_id: Option<String>,
    /// Discord role ID used for reminder and summary tracking.
    pub tracked_role_id: Option<String>,
    /// Discord channel ID where daily summaries are posted.
    pub summary_channel_id: Option<String>,
    /// Discord guild ID this bot instance is configured to serve.
    pub guild_id: Option<String>,

    #[serde(default = "default_true")]
    /// Whether daily summaries should ping inactive tracked members.
    pub ping_reminder_enabled: bool,

    #[serde(default = "default_streak")]
    /// Minimum streak required to appear in the Best Anglers section.
    pub best_angler_streak: u64,

    #[serde(default = "default_threshold")]
    /// Days of inactivity before a tracked member is included in reminders.
    pub reminder_threshold: u64,
}

fn default_timestamp() -> u64 {
    chrono::Utc::now().timestamp_millis() as u64
}

fn default_true() -> bool {
    true
}
fn default_streak() -> u64 {
    5
}
fn default_threshold() -> u64 {
    1
}

impl Default for FishingData {
    fn default() -> Self {
        Self {
            daily_count: 0,
            last_reset_timestamp: default_timestamp(),
            users: HashMap::new(),
            persistent_users: HashMap::new(),
            button_message_id: None,
            button_channel_id: None,
            tracked_role_id: None,
            summary_channel_id: None,
            guild_id: None,
            ping_reminder_enabled: true,
            best_angler_streak: 5,
            reminder_threshold: 1,
        }
    }
}

/// Owns persisted fishing state and coordinates safe disk access.
///
/// `FishingData` is protected by an async `RwLock` so command handlers can take
/// short read snapshots while mutations use a write lock. Disk writes are
/// serialized separately with `save_lock`, which prevents concurrent saves from
/// interleaving bytes on disk.
pub struct DataManager {
    /// Shared bot state protected by an async read/write lock.
    pub data: RwLock<FishingData>,
    file_path: PathBuf,
    backup_dir: PathBuf,
    max_backups: usize,
    save_lock: tokio::sync::Mutex<()>,
    dirty_generation: AtomicU64,
    saved_generation: AtomicU64,
}

impl DataManager {
    /// Builds a data manager from validated runtime configuration.
    pub fn new(config: &Config) -> Self {
        Self::from_paths(
            config.data_path.clone(),
            config.backup_dir.clone(),
            config.max_backups,
        )
    }

    /// Builds a data manager with explicit paths.
    ///
    /// This is used by tests to isolate persistence in temporary directories,
    /// and by production startup after configuration has resolved the data
    /// locations.
    pub fn from_paths(file_path: PathBuf, backup_dir: PathBuf, max_backups: usize) -> Self {
        // Load data synchronously during initialization (this is fine, happens once)
        let data = if file_path.exists() {
            match std::fs::read_to_string(&file_path) {
                Ok(content) => match serde_json::from_str(&content) {
                    Ok(d) => d,
                    Err(e) => {
                        tracing::error!("Error parsing data: {e}");
                        FishingData::default()
                    }
                },
                Err(e) => {
                    tracing::error!("Error reading file: {e}");
                    FishingData::default()
                }
            }
        } else {
            tracing::info!("ℹ️ No existing data file found, starting fresh");
            FishingData::default()
        };

        if !backup_dir.exists() {
            if let Err(e) = std::fs::create_dir_all(&backup_dir) {
                tracing::error!("Error creating backup directory: {e}");
            }
        }

        Self {
            data: RwLock::new(data),
            file_path,
            backup_dir,
            max_backups,
            save_lock: tokio::sync::Mutex::new(()),
            dirty_generation: AtomicU64::new(0),
            saved_generation: AtomicU64::new(0),
        }
    }

    /// Marks the in-memory state as newer than the last completed save.
    ///
    /// Save generation tracking lets many concurrent callers queue `save()`
    /// safely without forcing every stale queued save to rewrite the same final
    /// state.
    pub fn mark_dirty(&self) {
        self.dirty_generation.fetch_add(1, Ordering::SeqCst);
    }

    /// Loads persisted state from disk.
    ///
    /// Missing files, malformed JSON, invalid UTF-8, or schema mismatches
    /// recover to `FishingData::default()` so a corrupted save cannot crash
    /// startup. Other I/O failures still return an error because they may
    /// indicate a real filesystem problem.
    #[tracing::instrument(skip(self))]
    pub async fn load(&self) -> BotResult<FishingData> {
        match fs::read_to_string(&self.file_path).await {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(data) => Ok(data),
                Err(e) => {
                    tracing::error!("Error parsing data: {e}");
                    Ok(FishingData::default())
                }
            },
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::InvalidData
                ) =>
            {
                Ok(FishingData::default())
            }
            Err(e) => Err(BotError::Io(e)),
        }
    }

    /// Persists the current state to disk atomically.
    ///
    /// Writes to a `.tmp` file first, then renames it to the target path. This
    /// keeps the data file from being left half-written: `rename()` is atomic at
    /// the OS level on supported platforms. A crash during the write leaves the
    /// original file intact.
    #[tracing::instrument(skip(self))]
    pub async fn save(&self) -> BotResult<()> {
        let requested_generation = self.dirty_generation.load(Ordering::SeqCst);
        let _lock = self.save_lock.lock().await;

        if self.saved_generation.load(Ordering::SeqCst) > requested_generation {
            return Ok(());
        }

        let data = self.data.read().await;
        // Use to_string instead of to_string_pretty to significantly cut down file size and I/O time
        let json = serde_json::to_string(&*data)?;
        drop(data); // Drop lock as soon as possible before disk I/O

        let temp_path = self.file_path.with_extension("json.tmp");
        fs::write(&temp_path, json).await?;
        fs::rename(&temp_path, &self.file_path).await?;
        self.saved_generation.store(
            self.dirty_generation.load(Ordering::SeqCst),
            Ordering::SeqCst,
        );

        Ok(())
    }

    async fn create_backup_snapshot(&self) -> BotResult<()> {
        fs::create_dir_all(&self.backup_dir).await?;

        // Keep only the configured number of backups - using async I/O
        if let Ok(mut entries) = fs::read_dir(&self.backup_dir).await {
            let mut backups = Vec::new();

            while let Ok(Some(entry)) = entries.next_entry().await {
                // Ensure the file is actually a structural backup file and not a random json file or spoofed
                if entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("fishing_data_")
                    && entry.path().extension().is_some_and(|ext| ext == "json")
                {
                    if let Ok(metadata) = entry.metadata().await {
                        if let Ok(modified) = metadata.modified() {
                            backups.push((entry.path(), modified));
                        }
                    }
                }
            }

            // Sort by modification time
            backups.sort_by_key(|b| b.1);

            while backups.len() >= self.max_backups {
                if let Some(oldest) = backups.first() {
                    fs::remove_file(&oldest.0).await?;
                    backups.remove(0);
                } else {
                    break;
                }
            }
        }

        let timestamp = chrono::Local::now()
            .format("%Y-%m-%dT%H-%M-%S%.9f")
            .to_string();
        let mut backup_path = self
            .backup_dir
            .join(format!("fishing_data_{timestamp}.json"));
        let mut suffix = 1;

        while fs::metadata(&backup_path).await.is_ok() {
            backup_path = self
                .backup_dir
                .join(format!("fishing_data_{timestamp}_{suffix}.json"));
            suffix += 1;
        }

        if fs::metadata(&self.file_path).await.is_ok() {
            fs::copy(&self.file_path, backup_path).await?;
        }

        Ok(())
    }

    /// Creates a backup snapshot without failing the caller.
    ///
    /// Backup errors are logged instead of returned because reset and command
    /// paths should not crash after the primary save path has already protected
    /// live data.
    pub async fn backup(&self) {
        let _lock = self.save_lock.lock().await; // Reuse save lock to prevent backing up during save

        if let Err(e) = self.create_backup_snapshot().await {
            tracing::error!("Error creating backup: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn test_manager(temp: &tempfile::TempDir) -> DataManager {
        DataManager::from_paths(
            temp.path().join("fishing_data.json"),
            temp.path().join("backups"),
            5,
        )
    }

    fn sample_data() -> FishingData {
        let mut users = HashMap::new();
        users.insert(
            "user-1".to_string(),
            UserData {
                username: "Parth".to_string(),
                fished_at: "2026-01-01T15:00:00+00:00".to_string(),
            },
        );

        let mut persistent_users = HashMap::new();
        persistent_users.insert(
            "user-1".to_string(),
            PersistentUserData {
                username: "Parth".to_string(),
                streak: 3,
                last_fished_date: "2026-01-01".to_string(),
                total_catches: 7,
            },
        );

        FishingData {
            daily_count: 1,
            last_reset_timestamp: 1_700_000_000_000,
            users,
            persistent_users,
            ..FishingData::default()
        }
    }

    #[tokio::test]
    async fn test_atomic_save_roundtrip() {
        let temp = tempdir().unwrap();
        let manager = test_manager(&temp);
        let expected = sample_data();

        {
            let mut data = manager.data.write().await;
            *data = expected.clone();
        }

        manager.save().await.unwrap();
        let loaded = manager.load().await.unwrap();

        assert_eq!(loaded, expected);
    }

    #[tokio::test]
    async fn test_backup_rotation() {
        let temp = tempdir().unwrap();
        let manager = test_manager(&temp);

        for count in 0..7 {
            {
                let mut data = manager.data.write().await;
                data.daily_count = count;
            }
            manager.save().await.unwrap();
            manager.backup().await;
        }

        let mut entries = fs::read_dir(temp.path().join("backups")).await.unwrap();
        let mut backup_count = 0;
        while let Some(entry) = entries.next_entry().await.unwrap() {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with("fishing_data_")
            {
                backup_count += 1;
            }
        }

        assert_eq!(backup_count, 5);
    }

    #[tokio::test]
    async fn test_corrupt_file_fallback() {
        let temp = tempdir().unwrap();
        let manager = test_manager(&temp);

        fs::write(
            temp.path().join("fishing_data.json"),
            "{ definitely invalid json",
        )
        .await
        .unwrap();

        let loaded = manager.load().await.unwrap();
        let mut expected = FishingData::default();
        expected.last_reset_timestamp = loaded.last_reset_timestamp;

        assert_eq!(loaded, expected);
    }
}
