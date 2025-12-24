use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::fs;
use tokio::sync::RwLock;

// Define struct similar to existing JSON structure
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserData {
    pub username: String,
    #[serde(rename = "fishedAt")]
    pub fished_at: String, // Stored as ISO string in JSON
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PersistentUserData {
    pub username: String,
    pub streak: u64,
    #[serde(rename = "lastFishedDate")]
    pub last_fished_date: String,
    #[serde(rename = "totalCatches")]
    pub total_catches: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FishingData {
    #[serde(default)]
    pub daily_count: u64,

    #[serde(default = "default_timestamp")]
    pub last_reset_timestamp: u64,

    #[serde(default)]
    pub users: HashMap<String, UserData>,

    #[serde(default)]
    pub persistent_users: HashMap<String, PersistentUserData>,

    pub button_message_id: Option<String>,
    pub button_channel_id: Option<String>,
    pub tracked_role_id: Option<String>,
    pub summary_channel_id: Option<String>,
    pub guild_id: Option<String>,

    #[serde(default = "default_true")]
    pub ping_reminder_enabled: bool,

    #[serde(default = "default_streak")]
    pub best_angler_streak: u64,

    #[serde(default = "default_threshold")]
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

pub struct DataManager {
    pub data: RwLock<FishingData>,
    file_path: PathBuf,
    backup_dir: PathBuf,
}

impl DataManager {
    pub fn new() -> Self {
        let file_path = PathBuf::from("fishing_data.json");
        let backup_dir = PathBuf::from("backups");

        // Load data synchronously during initialization (this is fine, happens once)
        let data = if std::path::Path::new("fishing_data.json").exists() {
            match std::fs::read_to_string(&file_path) {
                Ok(content) => match serde_json::from_str(&content) {
                    Ok(d) => d,
                    Err(e) => {
                        tracing::error!("Error parsing data: {}", e);
                        FishingData::default()
                    }
                },
                Err(e) => {
                    tracing::error!("Error reading file: {}", e);
                    FishingData::default()
                }
            }
        } else {
            tracing::info!("ℹ️ No existing data file found, starting fresh");
            FishingData::default()
        };

        if !std::path::Path::new("backups").exists() {
            let _ = std::fs::create_dir_all(&backup_dir);
        }

        Self {
            data: RwLock::new(data),
            file_path,
            backup_dir,
        }
    }

    /// Atomic Save: Write to a temp file then rename it.
    /// This prevents corruption if the process is killed mid-write.
    pub async fn save(&self) {
        let data = self.data.read().await;
        match serde_json::to_string_pretty(&*data) {
            Ok(json) => {
                let temp_path = self.file_path.with_extension("json.tmp");
                // Use tokio::fs for async I/O
                if let Err(e) = fs::write(&temp_path, json).await {
                    tracing::error!("❌ Error writing temp data: {}", e);
                    return;
                }

                if let Err(e) = fs::rename(&temp_path, &self.file_path).await {
                    tracing::error!("❌ Error finalizing atomic save (rename): {}", e);
                }
            }
            Err(e) => tracing::error!("❌ Error serializing data: {}", e),
        }
    }

    pub async fn backup(&self) {
        // Keep only last 5 backups - using async I/O
        if let Ok(mut entries) = fs::read_dir(&self.backup_dir).await {
            let mut backups = Vec::new();

            while let Ok(Some(entry)) = entries.next_entry().await {
                if entry.path().extension().is_some_and(|ext| ext == "json") {
                    if let Ok(metadata) = entry.metadata().await {
                        if let Ok(modified) = metadata.modified() {
                            backups.push((entry.path(), modified));
                        }
                    }
                }
            }

            // Sort by modification time
            backups.sort_by_key(|b| b.1);

            while backups.len() >= 5 {
                if let Some(oldest) = backups.first() {
                    let _ = fs::remove_file(&oldest.0).await;
                    backups.remove(0);
                } else {
                    break;
                }
            }
        }

        let timestamp = chrono::Local::now().format("%Y-%m-%dT%H-%M-%S").to_string();
        let backup_path = self
            .backup_dir
            .join(format!("fishing_data_{}.json", timestamp));

        if fs::metadata(&self.file_path).await.is_ok() {
            let _ = fs::copy(&self.file_path, backup_path).await;
        }
    }
}
