//! Environment-backed configuration for Stardust Pond.
//!
//! Configuration is loaded once at startup from environment variables, with
//! `.env` support provided by `dotenvy` in `main.rs`. Required values fail fast;
//! optional values use conservative defaults that match the production bot.

use crate::{BotError, BotResult};
use std::env;
use std::fmt;
use std::path::PathBuf;

/// Runtime configuration validated before the Discord client starts.
#[derive(Debug, Clone)]
pub struct Config {
    /// Discord bot token. Required because the bot cannot start without an API identity.
    pub discord_token: String,
    /// Tracing filter used by `tracing_subscriber`; defaults to `info`.
    pub log_level: String,
    /// JSON file used for persisted fishing state.
    pub data_path: PathBuf,
    /// Directory used for rotating backup snapshots.
    pub backup_dir: PathBuf,
    /// Number of backup files to retain. Must be greater than zero.
    pub max_backups: usize,
    /// UTC hour for the daily reset. Must be in `0..=23`.
    pub reset_hour: u8,
    /// UTC minute for the daily reset. Must be in `0..=59`.
    pub reset_minute: u8,
}

impl Config {
    /// Loads configuration from environment variables.
    ///
    /// Precedence is environment variable first, then a built-in default for
    /// optional values. `DISCORD_BOT_TOKEN` is required. Reset time is validated
    /// as a real UTC wall-clock time, and `MAX_BACKUPS` must be non-zero so
    /// backup rotation always has a retention target.
    pub fn from_env() -> BotResult<Self> {
        let discord_token = env::var("DISCORD_BOT_TOKEN").map_err(|_| {
            BotError::Config("DISCORD_BOT_TOKEN is required but was not set".to_string())
        })?;

        let log_level = env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string());
        let data_path = PathBuf::from(
            env::var("DATA_PATH").unwrap_or_else(|_| "fishing_data.json".to_string()),
        );
        let backup_dir =
            PathBuf::from(env::var("BACKUP_DIR").unwrap_or_else(|_| "backups/".to_string()));
        let max_backups = parse_env("MAX_BACKUPS", 5)?;
        let reset_hour = parse_env("RESET_HOUR", 14)?;
        let reset_minute = parse_env("RESET_MINUTE", 30)?;

        if reset_hour > 23 {
            return Err(BotError::Config(format!(
                "RESET_HOUR must be between 0 and 23, got {reset_hour}"
            )));
        }

        if reset_minute > 59 {
            return Err(BotError::Config(format!(
                "RESET_MINUTE must be between 0 and 59, got {reset_minute}"
            )));
        }

        if max_backups == 0 {
            return Err(BotError::Config(
                "MAX_BACKUPS must be greater than 0".to_string(),
            ));
        }

        Ok(Self {
            discord_token,
            log_level,
            data_path,
            backup_dir,
            max_backups,
            reset_hour,
            reset_minute,
        })
    }
}

impl fmt::Display for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let token_preview = self.discord_token.chars().take(8).collect::<String>();

        write!(
            f,
            "Config {{ discord_token: {}..., log_level: {}, data_path: {}, backup_dir: {}, max_backups: {}, reset_hour: {}, reset_minute: {} }}",
            token_preview,
            self.log_level,
            self.data_path.display(),
            self.backup_dir.display(),
            self.max_backups,
            self.reset_hour,
            self.reset_minute
        )
    }
}

fn parse_env<T>(key: &str, default: T) -> BotResult<T>
where
    T: std::str::FromStr,
    T::Err: fmt::Display,
{
    match env::var(key) {
        Ok(value) => value
            .parse::<T>()
            .map_err(|e| BotError::Config(format!("{key} has invalid value {value:?}: {e}"))),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(e) => Err(BotError::Config(format!("{key} could not be read: {e}"))),
    }
}
