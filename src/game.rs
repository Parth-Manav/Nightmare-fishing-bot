// OBSERVABLE EVENTS
// fish.attempt - user tried to fish (user_id, guild_id, success: bool)
// fish.streak  - user's current streak after fishing (streak_length)
// reset.ran    - daily reset executed (member_count, fished_count)
// save.latency - time taken for atomic file write (duration_ms)

use crate::config::Config;
use crate::data::DataManager;
use crate::{BotError, BotResult};
use chrono::{DateTime, Utc};
use poise::serenity_prelude::{self as serenity, CreateEmbed, CreateMessage};
use std::collections::{hash_map::Entry, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Coordinates fishing rules, streak calculation, summaries, and daily resets.
///
/// The manager keeps game logic separate from Discord command handlers so the
/// critical rules can be tested directly with mocked timestamps.
pub struct FishingManager {
    data_manager: Arc<DataManager>,
    is_resetting: Arc<AtomicBool>, // FIXED: Atomic for safer RAII drop
    reset_hour: u8,
    reset_minute: u8,
}

impl FishingManager {
    /// Creates a fishing manager using validated reset-time configuration.
    pub fn new(data_manager: Arc<DataManager>, config: &Config) -> Self {
        Self {
            data_manager,
            is_resetting: Arc::new(AtomicBool::new(false)),
            reset_hour: config.reset_hour,
            reset_minute: config.reset_minute,
        }
    }

    /// Exposes the shared data manager for command handlers and startup checks.
    pub fn get_data_manager(&self) -> &Arc<DataManager> {
        &self.data_manager
    }

    /// Returns the fishing-day key for the default 14:30 UTC reset boundary.
    pub fn get_fishing_day_string(now: DateTime<Utc>) -> String {
        Self::get_fishing_day_string_for(now, 14, 30)
    }

    /// Returns the fishing-day key for an explicit reset boundary.
    ///
    /// The calculation shifts time backwards by the reset offset before
    /// formatting the date. This makes the reset instant inclusive for the new
    /// fishing day and avoids splitting data at midnight UTC.
    pub fn get_fishing_day_string_for(
        now: DateTime<Utc>,
        reset_hour: u8,
        reset_minute: u8,
    ) -> String {
        // Shift time back by 14 hours and 30 minutes.
        // This ensures e.g., 14:29 GMT maps to the previous day, 14:30 GMT maps to the current day.
        let shifted = now
            - chrono::Duration::hours(i64::from(reset_hour))
            - chrono::Duration::minutes(i64::from(reset_minute));
        shifted.format("%Y-%m-%d").to_string()
    }

    /// Returns yesterday's fishing-day key for the default reset boundary.
    pub fn get_yesterday_fishing_day_string(now: DateTime<Utc>) -> String {
        Self::get_yesterday_fishing_day_string_for(now, 14, 30)
    }

    /// Returns yesterday's fishing-day key for an explicit reset boundary.
    pub fn get_yesterday_fishing_day_string_for(
        now: DateTime<Utc>,
        reset_hour: u8,
        reset_minute: u8,
    ) -> String {
        let shifted = now
            - chrono::Duration::days(1)
            - chrono::Duration::hours(i64::from(reset_hour))
            - chrono::Duration::minutes(i64::from(reset_minute));
        shifted.format("%Y-%m-%d").to_string()
    }

    /// Returns the fishing-day key using this manager's configured reset time.
    pub fn get_configured_fishing_day_string(&self, now: DateTime<Utc>) -> String {
        Self::get_fishing_day_string_for(now, self.reset_hour, self.reset_minute)
    }

    fn get_configured_yesterday_fishing_day_string(&self, now: DateTime<Utc>) -> String {
        Self::get_yesterday_fishing_day_string_for(now, self.reset_hour, self.reset_minute)
    }

    /// Computes non-negative whole-day distance between persisted fishing-day keys.
    ///
    /// Invalid date strings return zero because reminder logic should fail
    /// closed instead of pinging users due to malformed persisted state.
    pub fn get_days_difference(date1: &str, date2: &str) -> i64 {
        let Ok(d1) = chrono::NaiveDate::parse_from_str(date1, "%Y-%m-%d") else {
            return 0;
        };
        let Ok(d2) = chrono::NaiveDate::parse_from_str(date2, "%Y-%m-%d") else {
            return 0;
        };

        d2.signed_duration_since(d1).num_days().max(0)
    }

    fn sorted_best_anglers(
        data: &crate::data::FishingData,
        best_angler_streak: u64,
    ) -> Vec<(String, String, u64, u64)> {
        let mut best_anglers = data
            .persistent_users
            .iter()
            .filter(|(_, p_user)| p_user.streak >= best_angler_streak)
            .map(|(user_id, p_user)| {
                (
                    user_id.clone(),
                    p_user.username.clone(),
                    p_user.streak,
                    p_user.total_catches,
                )
            })
            .collect::<Vec<_>>();

        // Deterministic order: streak DESC, total catches DESC, user id ASC.
        best_anglers.sort_by(|a, b| b.2.cmp(&a.2).then(b.3.cmp(&a.3)).then(a.0.cmp(&b.0)));
        best_anglers
    }

    #[tracing::instrument(
        skip(self),
        fields(user_id = %user_id, guild_id = tracing::field::Empty)
    )]
    /// Records a fishing attempt for the given user.
    ///
    /// Uses a write lock for the entire check-and-update sequence to prevent
    /// TOCTOU races: checking whether the user has already fished and recording
    /// a new fish happen atomically with respect to other callers.
    pub async fn handle_fishing(
        &self,
        user_id: String,
        username: String,
        guild_id: Option<String>,
    ) -> BotResult<(u64, u64, u64)> {
        tracing::Span::current().record("guild_id", guild_id.as_deref().unwrap_or("unknown"));
        self.handle_fishing_at(user_id, username, chrono::Utc::now())
            .await
    }

    async fn handle_fishing_at(
        &self,
        user_id: String,
        username: String,
        now: DateTime<Utc>,
    ) -> BotResult<(u64, u64, u64)> {
        let result = self.record_fishing_at(user_id, username, now).await?;
        self.data_manager.save().await?;

        Ok(result)
    }

    async fn record_fishing_at(
        &self,
        user_id: String,
        username: String,
        now: DateTime<Utc>,
    ) -> BotResult<(u64, u64, u64)> {
        let mut data = self.data_manager.data.write().await;

        if self.is_resetting.load(Ordering::SeqCst) {
            return Err(BotError::State(
                "A daily reset is currently in progress. Please try again in a few moments."
                    .to_string(),
            ));
        }

        let today_date = self.get_configured_fishing_day_string(now);
        let yesterday_date = self.get_configured_yesterday_fishing_day_string(now);

        if data.users.contains_key(&user_id) {
            return Err(BotError::State("ALREADY_FISHED".to_string()));
        }

        let (current_streak, total_catches) = match data.persistent_users.entry(user_id.clone()) {
            Entry::Vacant(entry) => {
                let p_user = entry.insert(crate::data::PersistentUserData {
                    username: username.clone(),
                    streak: 1,
                    last_fished_date: today_date.clone(),
                    total_catches: 1,
                });
                (p_user.streak, p_user.total_catches)
            }
            Entry::Occupied(mut entry) => {
                let p_user = entry.get_mut();

                if p_user.last_fished_date == yesterday_date {
                    p_user.streak += 1;
                } else if p_user.last_fished_date != today_date {
                    p_user.streak = 1;
                }

                p_user.last_fished_date = today_date.clone();
                p_user.username = username.clone();
                p_user.total_catches += 1;
                (p_user.streak, p_user.total_catches)
            }
        };

        data.users.insert(
            user_id.clone(),
            crate::data::UserData {
                username: username.clone(),
                fished_at: now.to_rfc3339(),
            },
        );
        data.daily_count += 1;
        let result = (current_streak, total_catches, data.daily_count);
        self.data_manager.mark_dirty();

        Ok(result)
    }

    /// Posts the daily summary using a Serenity context.
    pub async fn post_daily_summary(&self, ctx: &serenity::Context) {
        self.post_daily_summary_http(&ctx.http).await;
    }

    /// Posts the daily summary through a Serenity HTTP client.
    ///
    /// State needed for Discord pagination is cloned before network calls so
    /// the bot does not hold data locks while waiting on the Discord API.
    pub async fn post_daily_summary_http(&self, http: &serenity::Http) {
        let (
            summary_channel_id,
            guild_id,
            tracked_role_id,
            reminder_threshold,
            best_angler_streak,
            ping_reminder_enabled,
            daily_count,
        ) = {
            let data = self.data_manager.data.read().await;
            (
                data.summary_channel_id.clone(),
                data.guild_id.clone(),
                data.tracked_role_id.clone(),
                data.reminder_threshold,
                data.best_angler_streak,
                data.ping_reminder_enabled,
                data.daily_count,
            )
        };

        let channel_id = match summary_channel_id.and_then(|id| id.parse::<u64>().ok()) {
            Some(id) => serenity::ChannelId::new(id),
            None => return,
        };

        let g_id = match guild_id.and_then(|id| id.parse::<u64>().ok()) {
            Some(id) => serenity::GuildId::new(id),
            None => return,
        };

        let today_date = self.get_configured_fishing_day_string(chrono::Utc::now());

        let mut non_fishers = Vec::new();

        // Optimization: Use explicit clones to avoid locking data globally across long HTTP endpoints
        let (fished_today_ids_keys, persistent_users_cache) = {
            let data = self.data_manager.data.read().await;
            (
                data.users.keys().cloned().collect::<Vec<String>>(),
                data.persistent_users.clone(),
            )
        };

        if let Some(role_id_val) = tracked_role_id.and_then(|id| id.parse::<u64>().ok()) {
            let role_id = serenity::RoleId::new(role_id_val);

            // PAGINATION: Fetch all members reliably
            let mut after = None;
            loop {
                match g_id.members(http, Some(1000), after).await {
                    Ok(members) if members.is_empty() => break,
                    Ok(members) => {
                        let Some(last_member) = members.last() else {
                            break;
                        };
                        after = Some(last_member.user.id);
                        for member in members {
                            if member.roles.contains(&role_id) {
                                let u_id_str = member.user.id.to_string();
                                if !fished_today_ids_keys.contains(&u_id_str) {
                                    let days_diff = if let Some(p_user) =
                                        persistent_users_cache.get(&u_id_str)
                                    {
                                        Self::get_days_difference(
                                            &p_user.last_fished_date,
                                            &today_date,
                                        )
                                    } else {
                                        reminder_threshold as i64
                                    };

                                    if days_diff >= reminder_threshold as i64 {
                                        non_fishers.push(member.user.id);
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("❌ Error fetching members for summary: {e}");
                        break;
                    }
                }
            }
        }

        let best_anglers = {
            let data = self.data_manager.data.read().await;
            Self::sorted_best_anglers(&data, best_angler_streak)
        };

        let missed_count = non_fishers.len();
        let mut embed = CreateEmbed::new()
            .title("🐠 Daily Guild Aquarium Contributions")
            .description("Here is how the pond is doing today!")
            .color(0xFFD700)
            .field("🎣 Total Catches Today", format!("**{daily_count}**"), true)
            .field("😴 Members Missed", format!("**{missed_count}**"), true)
            .footer(serenity::CreateEmbedFooter::new(
                "Stardust Pond Daily Summary",
            ))
            .timestamp(Utc::now());

        if !best_anglers.is_empty() {
            let mut anglers_text = String::new();
            for (_, username, streak, total) in best_anglers.iter().take(10) {
                anglers_text.push_str(&format!(
                    "🏆 **{username}**: {total} 🐟 ({streak} day streak)\n"
                ));
            }
            embed = embed.field(
                format!("🔥 Best Anglers ({best_angler_streak}+ Day Streak)"),
                anglers_text,
                false,
            );
        }

        embed = embed.field("Message", "We miss you ❤️ \nPlease remember to fish daily 🙏🏻 Many lovely cats, cosmic dolphins and diamond rewards await us all 💎✨", false);

        let mut msg = CreateMessage::new().embed(embed);

        if !non_fishers.is_empty() && ping_reminder_enabled {
            let mut content = String::from("**Wake up! Many of you haven't fished today!** 🎣\n");
            let mut added = 0;
            for id in &non_fishers {
                let ping = format!("<@{id}> ");
                if content.len() + ping.len() > 1850 {
                    break;
                }
                content.push_str(&ping);
                added += 1;
            }

            if added < non_fishers.len() {
                let remaining = non_fishers.len() - added;
                content.push_str(&format!("...and {remaining} others"));
            }
            msg = msg.content(content);
        }

        if let Err(e) = channel_id.send_message(http, msg).await {
            tracing::error!("❌ Error sending summary: {e}");
        }
    }

    /// Runs the daily reset pipeline using a Serenity context.
    pub async fn run_daily_cron_job_ctx(&self, ctx: &serenity::Context) {
        self.run_daily_cron_job(&ctx.http).await;
    }

    /// Runs the daily reset pipeline.
    ///
    /// This method is used both by the cron scheduler and by startup catch-up
    /// logic when the bot was offline during the scheduled reset. The reset
    /// guard prevents overlapping reset jobs, and the data reset itself is
    /// idempotent for the same fishing day.
    pub async fn run_daily_cron_job(&self, http: &serenity::Http) {
        // Attempt to "lock" using AtomicBool for the ENTIRE sequence
        if self
            .is_resetting
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            tracing::warn!("⚠️ Reset already in progress, skipping duplicate call");
            return;
        }

        // RAII Guard using AtomicBool ensures lock is released
        struct ResetGuard {
            flag: Arc<AtomicBool>,
        }
        impl Drop for ResetGuard {
            fn drop(&mut self) {
                self.flag.store(false, Ordering::SeqCst);
            }
        }
        let _guard = ResetGuard {
            flag: self.is_resetting.clone(),
        };

        tracing::info!("🔄 Starting atomic daily cron job sequence...");
        self.post_daily_summary_http(http).await;
        self.data_manager.backup().await;

        tracing::info!("🔄 Resetting daily data...");
        self.reset_daily_data_at(chrono::Utc::now()).await;

        if let Err(e) = self.data_manager.save().await {
            tracing::error!("Error saving daily reset data: {e}");
        }
        self.data_manager.backup().await;

        tracing::info!("✅ Daily cron job complete.");
    }

    async fn reset_daily_data_at(&self, now: DateTime<Utc>) {
        let now_millis = now.timestamp_millis() as u64;
        let reset_day = self.get_configured_fishing_day_string(now);

        let mut data = self.data_manager.data.write().await;
        if data.users.is_empty() && data.daily_count == 0 {
            if let Some(last_reset_time) =
                chrono::DateTime::from_timestamp_millis(data.last_reset_timestamp as i64)
            {
                if self.get_configured_fishing_day_string(last_reset_time) == reset_day {
                    return;
                }
            }
        }

        let fished_ids: HashSet<String> = data.users.keys().cloned().collect();
        for (user_id, p_user) in data.persistent_users.iter_mut() {
            if !fished_ids.contains(user_id.as_str()) {
                p_user.streak = 0;
            }
        }
        data.daily_count = 0;
        data.last_reset_timestamp = now_millis;
        data.users.clear();
        self.data_manager.mark_dirty();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::data::DataManager;
    use chrono::{Datelike, NaiveDate, TimeZone};
    use std::collections::HashSet;
    use tempfile::tempdir;

    fn dt(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn test_manager(temp: &tempfile::TempDir) -> FishingManager {
        let data_manager = Arc::new(DataManager::from_paths(
            temp.path().join("fishing_data.json"),
            temp.path().join("backups"),
            5,
        ));
        let config = Config {
            discord_token: "test-token".to_string(),
            log_level: "info".to_string(),
            data_path: temp.path().join("fishing_data.json"),
            backup_dir: temp.path().join("backups"),
            max_backups: 5,
            reset_hour: 14,
            reset_minute: 30,
        };

        FishingManager::new(data_manager, &config)
    }

    fn assert_default_except_timestamp(actual: crate::data::FishingData) {
        let mut expected = crate::data::FishingData::default();
        expected.last_reset_timestamp = actual.last_reset_timestamp;
        assert_eq!(actual, expected);
    }

    #[derive(Debug, Default)]
    struct SimulationDayStats {
        fished: Vec<String>,
        errors: usize,
    }

    fn date_at(date: NaiveDate, hour: u32, minute: u32, second: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(date.year(), date.month(), date.day(), hour, minute, second)
            .unwrap()
    }

    fn simulation_users(count: usize) -> Vec<String> {
        (0..count)
            .map(|index| format!("sim-user-{index:04}"))
            .collect()
    }

    fn participates(day_index: usize, user_index: usize, participation_percent: u8) -> bool {
        user_index < 5
            || ((day_index * 73 + user_index * 37 + 17) % 100) < usize::from(participation_percent)
    }

    async fn simulate_day(
        manager: &FishingManager,
        date: NaiveDate,
        users: &[String],
        day_index: usize,
        participation_percent: u8,
    ) -> SimulationDayStats {
        let mut stats = SimulationDayStats::default();
        let fish_at = date_at(date, 15, 0, 0);
        let reset_at = date_at(date + chrono::Duration::days(1), 14, 30, 0);

        for (user_index, user_id) in users.iter().enumerate() {
            if participation_percent == 100
                || participates(day_index, user_index, participation_percent)
            {
                match manager
                    .record_fishing_at(user_id.clone(), format!("User {user_index}"), fish_at)
                    .await
                {
                    Ok(_) => stats.fished.push(user_id.clone()),
                    Err(_) => stats.errors += 1,
                }
            }
        }

        manager.reset_daily_data_at(reset_at).await;
        stats
    }

    #[cfg(test)]
    fn sorted_leaderboard(data: &crate::data::FishingData) -> Vec<(String, u64)> {
        let mut entries = data
            .persistent_users
            .iter()
            .map(|(user_id, user)| (user_id.clone(), user.total_catches))
            .collect::<Vec<_>>();
        entries.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        entries
    }

    #[test]
    fn test_boundary_exactly_at_reset_time() {
        // WHY: Off-by-one at the exact boundary second could give a user an
        // extra fish or wrongly lock them out of the new fishing day.
        let fishing_day = FishingManager::get_fishing_day_string(dt("2026-01-01T14:30:00Z"));

        assert_eq!(fishing_day, "2026-01-01");
    }

    #[test]
    fn test_boundary_one_second_before() {
        // WHY: Paired with the exact-boundary test, this pins the old-day side
        // of the reset boundary.
        let fishing_day = FishingManager::get_fishing_day_string(dt("2026-01-01T14:29:59Z"));

        assert_eq!(fishing_day, "2025-12-31");
    }

    #[test]
    fn test_boundary_one_second_after() {
        // WHY: Confirms the boundary is inclusive for the entire new day, not
        // only the exact reset instant.
        let fishing_day = FishingManager::get_fishing_day_string(dt("2026-01-01T14:30:01Z"));

        assert_eq!(fishing_day, "2026-01-01");
    }

    #[test]
    fn test_boundary_midnight_utc_is_same_fishing_day() {
        // WHY: A naive calendar-date implementation would split the leaderboard
        // at midnight UTC even though the reset is at 14:30 UTC.
        let fishing_day = FishingManager::get_fishing_day_string(dt("2026-01-02T00:01:00Z"));

        assert_eq!(fishing_day, "2026-01-01");
    }

    #[test]
    fn test_new_year_boundary() {
        // WHY: Year rollover is a classic source of date math bugs and should
        // not panic or corrupt the fishing-day string.
        let old_day = FishingManager::get_fishing_day_string(dt("2026-01-01T14:29:59Z"));
        let new_day = FishingManager::get_fishing_day_string(dt("2026-01-01T14:30:01Z"));

        assert_eq!(old_day, "2025-12-31");
        assert_eq!(new_day, "2026-01-01");
    }

    #[test]
    fn test_boundary_leap_day() {
        // WHY: Leap day handling is a known source of date-library crashes and
        // off-by-one mistakes.
        let fishing_day = FishingManager::get_fishing_day_string(dt("2028-02-29T14:30:01Z"));

        assert_eq!(fishing_day, "2028-02-29");
    }

    #[test]
    fn test_boundary_dst_ambiguous_hour() {
        // WHY: The bot uses UTC internally; this guards against accidental local
        // time conversion during a US DST transition.
        let fishing_day = FishingManager::get_fishing_day_string(dt("2025-11-02T06:30:00Z"));

        assert_eq!(fishing_day, "2025-11-01");
    }

    #[tokio::test]
    async fn test_double_fish_same_day() {
        let temp = tempdir().unwrap();
        let manager = test_manager(&temp);

        manager
            .handle_fishing_at(
                "user-1".to_string(),
                "Parth".to_string(),
                dt("2026-01-01T15:00:00Z"),
            )
            .await
            .unwrap();

        let result = manager
            .handle_fishing_at(
                "user-1".to_string(),
                "Parth".to_string(),
                dt("2026-01-01T16:00:00Z"),
            )
            .await;

        assert!(matches!(result, Err(BotError::State(message)) if message == "ALREADY_FISHED"));
    }

    #[tokio::test]
    async fn test_streak_starts_at_one() {
        // WHY: A fresh user's first successful fish must initialize the core
        // engagement counter to exactly one, not zero or an accidental double.
        let temp = tempdir().unwrap();
        let manager = test_manager(&temp);

        let (streak, total_catches, daily_count) = manager
            .handle_fishing_at(
                "user-1".to_string(),
                "Parth".to_string(),
                dt("2026-01-01T15:00:00Z"),
            )
            .await
            .unwrap();

        assert_eq!(streak, 1);
        assert_eq!(total_catches, 1);
        assert_eq!(daily_count, 1);
    }

    #[tokio::test]
    async fn test_streak_increments_on_consecutive_days() {
        // WHY: Consecutive daily participation is the core streak contract; a
        // regression here immediately damages user trust.
        let temp = tempdir().unwrap();
        let manager = test_manager(&temp);

        manager
            .handle_fishing_at(
                "user-1".to_string(),
                "Parth".to_string(),
                dt("2026-01-01T15:00:00Z"),
            )
            .await
            .unwrap();
        manager
            .reset_daily_data_at(dt("2026-01-02T14:30:00Z"))
            .await;
        manager
            .handle_fishing_at(
                "user-1".to_string(),
                "Parth".to_string(),
                dt("2026-01-02T15:00:00Z"),
            )
            .await
            .unwrap();
        manager
            .reset_daily_data_at(dt("2026-01-03T14:30:00Z"))
            .await;
        manager
            .handle_fishing_at(
                "user-1".to_string(),
                "Parth".to_string(),
                dt("2026-01-03T15:00:00Z"),
            )
            .await
            .unwrap();

        let data = manager.get_data_manager().data.read().await;
        assert_eq!(data.persistent_users["user-1"].streak, 3);
    }

    #[tokio::test]
    async fn test_streak_resets_to_one_after_miss() {
        // WHY: Missing a day should restart the streak, not increment it or
        // freeze the previous value.
        let temp = tempdir().unwrap();
        let manager = test_manager(&temp);

        manager
            .handle_fishing_at(
                "user-1".to_string(),
                "Parth".to_string(),
                dt("2026-01-01T15:00:00Z"),
            )
            .await
            .unwrap();
        manager
            .reset_daily_data_at(dt("2026-01-02T14:30:00Z"))
            .await;
        manager
            .reset_daily_data_at(dt("2026-01-03T14:30:00Z"))
            .await;
        manager
            .handle_fishing_at(
                "user-1".to_string(),
                "Parth".to_string(),
                dt("2026-01-03T15:00:00Z"),
            )
            .await
            .unwrap();

        let data = manager.get_data_manager().data.read().await;
        assert_eq!(data.persistent_users["user-1"].streak, 1);
    }

    #[tokio::test]
    async fn test_streak_does_not_increment_twice_same_day() {
        // WHY: Duplicate same-day fish attempts must not let users grind streaks
        // by spamming the command or retrying Discord interactions.
        let temp = tempdir().unwrap();
        let manager = test_manager(&temp);

        manager
            .handle_fishing_at(
                "user-1".to_string(),
                "Parth".to_string(),
                dt("2026-01-01T14:31:00Z"),
            )
            .await
            .unwrap();
        let duplicate = manager
            .handle_fishing_at(
                "user-1".to_string(),
                "Parth".to_string(),
                dt("2026-01-01T15:00:00Z"),
            )
            .await;
        manager
            .reset_daily_data_at(dt("2026-01-02T14:30:00Z"))
            .await;
        manager
            .handle_fishing_at(
                "user-1".to_string(),
                "Parth".to_string(),
                dt("2026-01-02T15:00:00Z"),
            )
            .await
            .unwrap();

        let data = manager.get_data_manager().data.read().await;
        assert!(matches!(duplicate, Err(BotError::State(message)) if message == "ALREADY_FISHED"));
        assert_eq!(data.persistent_users["user-1"].streak, 2);
        assert_eq!(data.persistent_users["user-1"].total_catches, 2);
    }

    #[tokio::test]
    async fn test_streak_survives_100_consecutive_days() {
        // WHY: Long streaks must not overflow a too-small counter or drift over
        // repeated reset/fish cycles.
        let temp = tempdir().unwrap();
        let manager = test_manager(&temp);
        let start = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();

        for day_offset in 0..100 {
            let date = start + chrono::Duration::days(day_offset);
            let fish_at = Utc
                .with_ymd_and_hms(date.year(), date.month(), date.day(), 15, 0, 0)
                .unwrap();
            manager
                .handle_fishing_at("user-1".to_string(), "Parth".to_string(), fish_at)
                .await
                .unwrap();

            if day_offset < 99 {
                let next = date + chrono::Duration::days(1);
                let reset_at = Utc
                    .with_ymd_and_hms(next.year(), next.month(), next.day(), 14, 30, 0)
                    .unwrap();
                manager.reset_daily_data_at(reset_at).await;
            }
        }

        let data = manager.get_data_manager().data.read().await;
        assert_eq!(data.persistent_users["user-1"].streak, 100);
    }

    #[tokio::test]
    async fn test_streak_max_fish_per_day_enforced() {
        // WHY: The guard must prevent both the visible success and the stored
        // write; otherwise the leaderboard count can exceed the daily limit.
        const MAX_FISH_PER_DAY: u64 = 1;
        let temp = tempdir().unwrap();
        let manager = test_manager(&temp);

        manager
            .handle_fishing_at(
                "user-1".to_string(),
                "Parth".to_string(),
                dt("2026-01-01T15:00:00Z"),
            )
            .await
            .unwrap();
        let result = manager
            .handle_fishing_at(
                "user-1".to_string(),
                "Parth".to_string(),
                dt("2026-01-01T15:01:00Z"),
            )
            .await;

        let data = manager.get_data_manager().data.read().await;
        assert!(matches!(result, Err(BotError::State(message)) if message == "ALREADY_FISHED"));
        assert_eq!(data.daily_count, MAX_FISH_PER_DAY);
        assert_eq!(data.users.len() as u64, MAX_FISH_PER_DAY);
        assert_eq!(
            data.persistent_users["user-1"].total_catches,
            MAX_FISH_PER_DAY
        );
    }

    #[tokio::test]
    async fn test_chaos_user_id_zero() {
        // WHY: Discord snowflakes should never be zero, but a caller bug should
        // not be able to panic and crash the bot process.
        let temp = tempdir().unwrap();
        let manager = test_manager(&temp);

        let result = manager
            .handle_fishing_at(
                "0".to_string(),
                "Zero".to_string(),
                dt("2026-01-01T15:00:00Z"),
            )
            .await;

        assert!(result.is_ok() || matches!(result, Err(BotError::State(_))));
    }

    #[tokio::test]
    async fn test_chaos_user_id_u64_max() {
        // WHY: Extreme snowflake-like values must be safe HashMap keys and must
        // not overflow any incidental ID math.
        let temp = tempdir().unwrap();
        let manager = test_manager(&temp);
        let user_id = u64::MAX.to_string();

        let result = manager
            .handle_fishing_at(
                user_id.clone(),
                "Max".to_string(),
                dt("2026-01-01T15:00:00Z"),
            )
            .await;

        let data = manager.get_data_manager().data.read().await;
        assert!(result.is_ok());
        assert!(data.persistent_users.contains_key(&user_id));
    }

    #[tokio::test]
    async fn test_chaos_empty_string_username() {
        // WHY: A transient Discord display-name issue should not panic or
        // poison saved JSON when the username is temporarily empty.
        let temp = tempdir().unwrap();
        let manager = test_manager(&temp);

        manager
            .handle_fishing_at(
                "user-empty".to_string(),
                String::new(),
                dt("2026-01-01T15:00:00Z"),
            )
            .await
            .unwrap();

        let data = manager.get_data_manager().data.read().await;
        assert_eq!(data.users["user-empty"].username, "");
        assert_eq!(data.persistent_users["user-empty"].username, "");
    }

    #[tokio::test]
    async fn test_chaos_username_with_special_characters() {
        // WHY: Usernames are persisted as JSON; injection-like text, null bytes,
        // emoji-only names, and very long strings must serialize cleanly.
        let temp = tempdir().unwrap();
        let manager = test_manager(&temp);
        let file_path = temp.path().join("fishing_data.json");
        let usernames = vec![
            "'; DROP TABLE users; --".to_string(),
            "<script>alert(1)</script>".to_string(),
            "\0\0\0".to_string(),
            "\u{1F3A3}".repeat(8),
            "a".repeat(10_000),
        ];

        for (index, username) in usernames.iter().enumerate() {
            manager
                .handle_fishing_at(
                    format!("special-user-{index}"),
                    username.clone(),
                    dt("2026-01-01T15:00:00Z"),
                )
                .await
                .unwrap();
        }

        let json = tokio::fs::read_to_string(file_path).await.unwrap();
        let saved: crate::data::FishingData = serde_json::from_str(&json).unwrap();

        for (index, username) in usernames.iter().enumerate() {
            let user_id = format!("special-user-{index}");
            assert_eq!(saved.persistent_users[&user_id].username, *username);
        }
    }

    #[tokio::test]
    async fn test_chaos_fish_count_does_not_go_negative() {
        // WHY: A negative persisted count should never underflow into a huge
        // unsigned value such as u64::MAX after deserialization or recovery.
        let temp = tempdir().unwrap();
        let file_path = temp.path().join("fishing_data.json");
        let manager = DataManager::from_paths(file_path.clone(), temp.path().join("backups"), 5);

        tokio::fs::write(
            &file_path,
            r#"{"persistentUsers":{"user-1":{"username":"Parth","streak":1,"lastFishedDate":"2026-01-01","totalCatches":-1}}}"#,
        )
        .await
        .unwrap();

        let loaded = manager.load().await.unwrap();
        assert!(loaded.persistent_users.is_empty());
        assert_eq!(loaded.daily_count, 0);
    }

    #[tokio::test]
    async fn test_chaos_corrupted_save_file_recovery() {
        // WHY: Crashes, interrupted writes, or manual edits can leave malformed
        // persistence behind; startup/load must recover to default state.
        let temp = tempdir().unwrap();
        let file_path = temp.path().join("fishing_data.json");
        let manager = DataManager::from_paths(file_path.clone(), temp.path().join("backups"), 5);
        let cases: Vec<Vec<u8>> = vec![
            b"".to_vec(),
            br#"{"totally":"wrong"}"#.to_vec(),
            br#"{"users":{"123":{"fish_count":5,"stre"#.to_vec(),
            (0..500).map(|value| (value % 251) as u8).collect(),
            br#"{"users":{"123":{"fish_count":"five"}}}"#.to_vec(),
            b"null".to_vec(),
        ];

        for content in cases {
            tokio::fs::write(&file_path, content).await.unwrap();
            let loaded = manager.load().await.unwrap();
            assert_default_except_timestamp(loaded);
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_chaos_simultaneous_fish_100_users_same_second() {
        // WHY: Concurrent first-fish writes must not interleave in memory or on
        // disk; every user should be counted exactly once.
        let temp = tempdir().unwrap();
        let file_path = temp.path().join("fishing_data.json");
        let manager = Arc::new(test_manager(&temp));
        let now = dt("2026-01-01T15:00:00Z");
        let mut tasks = Vec::new();

        for user_number in 0..100 {
            let manager = manager.clone();
            tasks.push(tokio::spawn(async move {
                manager
                    .handle_fishing_at(
                        format!("chaos-user-{user_number}"),
                        format!("User {user_number}"),
                        now,
                    )
                    .await
            }));
        }

        for task in tasks {
            task.await.unwrap().unwrap();
        }

        let data = manager.get_data_manager().data.read().await;
        assert_eq!(data.users.len(), 100);
        assert_eq!(data.daily_count, 100);
        assert!(data
            .persistent_users
            .values()
            .all(|user| user.total_catches == 1));
        drop(data);

        let json = tokio::fs::read_to_string(file_path).await.unwrap();
        let saved: crate::data::FishingData = serde_json::from_str(&json).unwrap();
        assert_eq!(saved.users.len(), 100);
        assert!(saved
            .persistent_users
            .values()
            .all(|user| user.total_catches == 1));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_chaos_simultaneous_fish_and_reset() {
        // WHY: A fish landing at the reset instant must be counted either before
        // or after the reset, never double-counted, partially written, or lost
        // from persistent totals.
        let temp = tempdir().unwrap();
        let file_path = temp.path().join("fishing_data.json");
        let manager = Arc::new(test_manager(&temp));
        let fish_manager = manager.clone();
        let reset_manager = manager.clone();

        let (fish_result, _) = tokio::join!(
            async move {
                fish_manager
                    .handle_fishing_at(
                        "user-123".to_string(),
                        "Parth".to_string(),
                        dt("2026-01-01T14:30:00Z"),
                    )
                    .await
            },
            async move {
                reset_manager
                    .reset_daily_data_at(dt("2026-01-01T14:30:00Z"))
                    .await;
            }
        );

        manager.get_data_manager().save().await.unwrap();
        let data = manager.get_data_manager().data.read().await;
        let total_catches = data
            .persistent_users
            .get("user-123")
            .map(|user| user.total_catches)
            .unwrap_or(0);
        assert!(fish_result.is_ok());
        assert!(total_catches <= 1);
        assert!(data.daily_count <= 1);
        assert!(data.users.len() <= 1);
        drop(data);

        let json = tokio::fs::read_to_string(file_path).await.unwrap();
        serde_json::from_str::<crate::data::FishingData>(&json).unwrap();
    }

    #[tokio::test]
    async fn test_simulation_one_year_simulation_500_users() {
        // WHY: A full production year can expose drift in streaks, totals, reset
        // behavior, and serialization that single-day tests never exercise.
        let temp = tempdir().unwrap();
        let manager = test_manager(&temp);
        let users = simulation_users(500);
        let start = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
        let mut total_fished = 0usize;

        for day_index in 0..365 {
            let stats = simulate_day(
                &manager,
                start + chrono::Duration::days(day_index),
                &users,
                day_index as usize,
                70,
            )
            .await;
            assert_eq!(stats.errors, 0);
            total_fished += stats.fished.len();
        }

        manager.get_data_manager().save().await.unwrap();
        let data = manager.get_data_manager().data.read().await.clone();
        let expected = 500.0 * 365.0 * 0.70;
        let lower = (expected * 0.95) as usize;
        let upper = (expected * 1.05) as usize;
        let highest_streak = data
            .persistent_users
            .values()
            .map(|user| user.streak)
            .max()
            .unwrap();
        let lowest_active_streak = data
            .persistent_users
            .values()
            .filter(|user| user.streak > 0)
            .map(|user| user.streak)
            .min()
            .unwrap();

        assert!((lower..=upper).contains(&total_fished));
        assert!((200..=365).contains(&highest_streak));
        assert!(lowest_active_streak >= 1);
        assert!(data
            .persistent_users
            .values()
            .all(|user| user.total_catches <= 365));

        let reloaded = manager.get_data_manager().load().await.unwrap();
        assert_eq!(reloaded, data);
    }

    #[tokio::test]
    async fn test_simulation_year_boundary_simulation() {
        // WHY: Multi-day runs across Dec 31 and Jan 1 should preserve streaks
        // and should not collapse the fishing day into the previous year.
        let temp = tempdir().unwrap();
        let manager = test_manager(&temp);
        let users = simulation_users(50);
        let start = NaiveDate::from_ymd_opt(2025, 12, 25).unwrap();

        for day_index in 0..14 {
            let stats = simulate_day(
                &manager,
                start + chrono::Duration::days(day_index),
                &users,
                day_index as usize,
                100,
            )
            .await;
            assert_eq!(stats.errors, 0);
            assert_eq!(stats.fished.len(), 50);
        }

        let data = manager.get_data_manager().data.read().await;
        assert!(data.persistent_users.values().all(|user| user.streak == 14));
        assert_eq!(
            FishingManager::get_fishing_day_string(dt("2026-01-01T15:00:00Z")),
            "2026-01-01"
        );
    }

    #[tokio::test]
    async fn test_simulation_long_running_memory_stability() {
        // WHY: Slow accumulation in daily maps or restart serialization drift
        // can crash a bot only after weeks of uptime.
        let temp = tempdir().unwrap();
        let manager = test_manager(&temp);
        let users = simulation_users(100);
        let start = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();

        for day_index in 0..1000 {
            simulate_day(
                &manager,
                start + chrono::Duration::days(day_index),
                &users,
                day_index as usize,
                70,
            )
            .await;

            if (day_index + 1) % 100 == 0 {
                let before = manager.get_data_manager().data.read().await.clone();
                manager.get_data_manager().save().await.unwrap();
                let loaded = manager.get_data_manager().load().await.unwrap();
                assert_eq!(loaded, before);
                assert!(loaded.users.is_empty());
                assert!(loaded.persistent_users.len() <= users.len());

                let mut data = manager.get_data_manager().data.write().await;
                *data = loaded;
            }
        }
    }

    #[tokio::test]
    async fn test_simulation_leaderboard_sort_stability_over_time() {
        // WHY: An unstable leaderboard makes equal-score users appear to swap
        // randomly between calls, which looks broken to Discord users.
        let temp = tempdir().unwrap();
        let manager = test_manager(&temp);
        let users = simulation_users(100);
        let start = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();

        for day_index in 0..365 {
            simulate_day(
                &manager,
                start + chrono::Duration::days(day_index),
                &users,
                day_index as usize,
                70,
            )
            .await;
        }

        let data = manager.get_data_manager().data.read().await;
        let first = sorted_leaderboard(&data);
        let second = sorted_leaderboard(&data);
        assert_eq!(first, second);

        for pair in first.windows(2) {
            let [left, right] = pair else { unreachable!() };
            assert!(
                left.1 > right.1 || (left.1 == right.1 && left.0 <= right.0),
                "leaderboard order is not deterministic: {left:?} before {right:?}"
            );
        }
    }

    #[tokio::test]
    async fn test_simulation_reset_idempotency() {
        // WHY: If the bot crashes mid-reset and catch-up reset runs on startup,
        // a duplicate reset must not wipe a valid streak or corrupt daily state.
        let temp = tempdir().unwrap();
        let manager = test_manager(&temp);

        manager
            .handle_fishing_at(
                "user-1".to_string(),
                "Parth".to_string(),
                dt("2026-01-01T15:00:00Z"),
            )
            .await
            .unwrap();
        manager
            .reset_daily_data_at(dt("2026-01-02T14:30:00Z"))
            .await;
        manager
            .handle_fishing_at(
                "user-1".to_string(),
                "Parth".to_string(),
                dt("2026-01-02T15:00:00Z"),
            )
            .await
            .unwrap();
        manager
            .reset_daily_data_at(dt("2026-01-03T14:30:00Z"))
            .await;
        manager
            .reset_daily_data_at(dt("2026-01-03T14:30:00Z"))
            .await;

        let data = manager.get_data_manager().data.read().await;
        assert_eq!(data.daily_count, 0);
        assert!(data.users.is_empty());
        assert_eq!(data.persistent_users["user-1"].streak, 2);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn test_stress_1000_concurrent_fish_requests() {
        // WHY: A busy Discord server can deliver many unique interactions at
        // once; all first-fish requests should complete without data loss.
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            let temp = tempdir().unwrap();
            let file_path = temp.path().join("fishing_data.json");
            let manager = Arc::new(test_manager(&temp));
            let mut joins = tokio::task::JoinSet::new();

            for user_number in 0..1000 {
                let manager = manager.clone();
                joins.spawn(async move {
                    manager
                        .handle_fishing_at(
                            format!("stress-user-{user_number}"),
                            format!("User {user_number}"),
                            dt("2026-01-01T15:00:00Z"),
                        )
                        .await
                });
            }

            let mut ok_count = 0;
            while let Some(result) = joins.join_next().await {
                result.unwrap().unwrap();
                ok_count += 1;
            }

            let data = manager.get_data_manager().data.read().await;
            assert_eq!(ok_count, 1000);
            assert_eq!(data.users.len(), 1000);
            drop(data);

            let json = tokio::fs::read_to_string(file_path).await.unwrap();
            let saved: crate::data::FishingData = serde_json::from_str(&json).unwrap();
            assert_eq!(saved.users.len(), 1000);
        })
        .await
        .expect("stress test timed out - possible deadlock");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn test_stress_1000_concurrent_fish_same_user() {
        // WHY: Discord retries or a double-click can send duplicate interactions;
        // only one should mutate state for the same user on the same day.
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            let temp = tempdir().unwrap();
            let manager = Arc::new(test_manager(&temp));
            let mut joins = tokio::task::JoinSet::new();

            for _ in 0..1000 {
                let manager = manager.clone();
                joins.spawn(async move {
                    manager
                        .handle_fishing_at(
                            "12345".to_string(),
                            "Parth".to_string(),
                            dt("2026-01-01T15:00:00Z"),
                        )
                        .await
                });
            }

            let mut ok_count = 0;
            let mut err_count = 0;
            while let Some(result) = joins.join_next().await {
                match result.unwrap() {
                    Ok(_) => ok_count += 1,
                    Err(BotError::State(message)) if message == "ALREADY_FISHED" => err_count += 1,
                    Err(error) => panic!("unexpected error: {error}"),
                }
            }

            let data = manager.get_data_manager().data.read().await;
            assert_eq!(ok_count, 1);
            assert_eq!(err_count, 999);
            assert_eq!(data.users.len(), 1);
            assert_eq!(data.persistent_users["12345"].total_catches, 1);
        })
        .await
        .expect("stress test timed out - possible deadlock");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn test_stress_concurrent_fish_and_leaderboard_read() {
        // WHY: Read/write lock contention between fishing and leaderboard reads
        // is a common deadlock and consistency risk.
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let temp = tempdir().unwrap();
            let manager = Arc::new(test_manager(&temp));
            let mut joins = tokio::task::JoinSet::new();

            for user_number in 0..100 {
                let manager = manager.clone();
                joins.spawn(async move {
                    manager
                        .handle_fishing_at(
                            format!("reader-user-{user_number}"),
                            format!("User {user_number}"),
                            dt("2026-01-01T15:00:00Z"),
                        )
                        .await
                        .map(|_| ())
                });
            }

            for _ in 0..50 {
                let manager = manager.clone();
                joins.spawn(async move {
                    let data = manager.get_data_manager().data.read().await;
                    let leaderboard = sorted_leaderboard(&data);
                    let mut seen = HashSet::new();
                    for (user_id, count) in leaderboard {
                        assert!(seen.insert(user_id));
                        assert!(count > 0);
                    }
                    Ok(())
                });
            }

            let mut completed = 0;
            while let Some(result) = joins.join_next().await {
                result.unwrap().unwrap();
                completed += 1;
            }
            assert_eq!(completed, 150);
        })
        .await
        .expect("stress test timed out - possible deadlock");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn test_stress_save_never_produces_partial_file() {
        // WHY: Concurrent saves must serialize through the save lock and atomic
        // rename path so the data file never contains interleaved partial JSON.
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            for iteration in 0..10 {
                let temp = tempdir().unwrap();
                let file_path = temp.path().join("fishing_data.json");
                let manager = test_manager(&temp);
                {
                    let mut data = manager.get_data_manager().data.write().await;
                    for user_number in 0..50 {
                        data.persistent_users.insert(
                            format!("save-user-{iteration}-{user_number}"),
                            crate::data::PersistentUserData {
                                username: format!("User {user_number}"),
                                streak: 1,
                                last_fished_date: "2026-01-01".to_string(),
                                total_catches: 1,
                            },
                        );
                    }
                }

                let data_manager = manager.get_data_manager().clone();
                let mut joins = tokio::task::JoinSet::new();
                for _ in 0..50 {
                    let data_manager = data_manager.clone();
                    joins.spawn(async move { data_manager.save().await });
                }

                while let Some(result) = joins.join_next().await {
                    result.unwrap().unwrap();
                }

                let json = tokio::fs::read_to_string(&file_path).await.unwrap();
                serde_json::from_str::<crate::data::FishingData>(&json).unwrap();
            }
        })
        .await
        .expect("stress test timed out - possible deadlock");
    }
}
