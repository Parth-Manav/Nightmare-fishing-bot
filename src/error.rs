//! Error types for the Stardust Pond bot.
//!
//! All fallible operations return [`BotResult<T>`], a type alias for
//! `Result<T, BotError>`. This keeps errors typed instead of stringly-typed,
//! so I/O, JSON, Discord API, configuration, and state failures stay distinct.

use poise::serenity_prelude as serenity;

/// Project-wide result type used by bot, persistence, and configuration code.
pub type BotResult<T> = Result<T, BotError>;

/// Typed error hierarchy for every recoverable failure path in the bot.
#[derive(thiserror::Error, Debug)]
pub enum BotError {
    /// Filesystem failure while reading, writing, renaming, or backing up data.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization or deserialization failure for persisted bot state.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Serenity/Discord API failure while sending messages or fetching guild data.
    #[error("Discord API error: {0}")]
    Discord(#[source] Box<serenity::Error>),

    /// Runtime state rejection, such as duplicate fishing or reset-in-progress.
    #[error("State error: {0}")]
    State(String),

    /// Invalid or missing environment configuration detected at startup.
    #[error("Configuration error: {0}")]
    Config(String),

    /// Higher-level persistence failure when the data file cannot be trusted.
    #[error("Persistence error: {0}")]
    Persistence(String),
}

impl From<serenity::Error> for BotError {
    fn from(error: serenity::Error) -> Self {
        Self::Discord(Box::new(error))
    }
}
