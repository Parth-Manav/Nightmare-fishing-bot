use crate::data::DataManager;
use chrono::{DateTime, Utc};
use poise::serenity_prelude::{self as serenity, CreateEmbed, CreateMessage};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub struct FishingManager {
    data_manager: Arc<DataManager>,
    is_resetting: Arc<AtomicBool>, // FIXED: Atomic for safer RAII drop
}

#[derive(Debug, PartialEq)]
pub enum FishingError {
    AlreadyFished,
    Internal(String),
}

impl std::fmt::Display for FishingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FishingError::AlreadyFished => write!(f, "ALREADY_FISHED"),
            FishingError::Internal(s) => write!(f, "Internal error: {}", s),
        }
    }
}

impl std::error::Error for FishingError {}

impl FishingManager {
    pub fn new(data_manager: Arc<DataManager>) -> Self {
        Self {
            data_manager,
            is_resetting: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn get_data_manager(&self) -> &Arc<DataManager> {
        &self.data_manager
    }

    // Calculates the "Fishing Day" string. A day rolls over exactly at 14:30 GMT.
    pub fn get_fishing_day_string(now: DateTime<Utc>) -> String {
        // Shift time back by 14 hours and 30 minutes. 
        // This ensures e.g., 14:29 GMT maps to the previous day, 14:30 GMT maps to the current day.
        let shifted = now - chrono::Duration::hours(14) - chrono::Duration::minutes(30);
        shifted.format("%Y-%m-%d").to_string()
    }

    pub fn get_yesterday_fishing_day_string(now: DateTime<Utc>) -> String {
        let shifted = now - chrono::Duration::days(1) - chrono::Duration::hours(14) - chrono::Duration::minutes(30);
        shifted.format("%Y-%m-%d").to_string()
    }

    pub fn get_days_difference(date1: &str, date2: &str) -> i64 {
        let d1 = chrono::NaiveDate::parse_from_str(date1, "%Y-%m-%d")
            .unwrap_or_else(|_| chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap());
        let d2 = chrono::NaiveDate::parse_from_str(date2, "%Y-%m-%d")
            .unwrap_or_else(|_| chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap());
        d2.signed_duration_since(d1).num_days().max(0)
    }

    pub async fn handle_fishing(
        &self,
        user_id: String,
        username: String,
    ) -> Result<(u64, u64, u64), FishingError> {
        let mut data = self.data_manager.data.write().await;

        if self.is_resetting.load(Ordering::SeqCst) {
            return Err(FishingError::Internal("A daily reset is currently in progress. Please try again in a few moments.".to_string()));
        }

        let now = chrono::Utc::now();
        let today_date = Self::get_fishing_day_string(now);
        let yesterday_date = Self::get_yesterday_fishing_day_string(now);


        if data.users.contains_key(&user_id) {
            return Err(FishingError::AlreadyFished);
        }

        if !data.persistent_users.contains_key(&user_id) {
            data.persistent_users.insert(
                user_id.clone(),
                crate::data::PersistentUserData {
                    username: username.clone(),
                    streak: 1,
                    last_fished_date: today_date.clone(),
                    total_catches: 1,
                },
            );
        } else {
            let p_user = data
                .persistent_users
                .get_mut(&user_id)
                .expect("Checked contains_key");

            if p_user.last_fished_date == yesterday_date {
                p_user.streak += 1;
            } else if p_user.last_fished_date != today_date {
                p_user.streak = 1;
            }

            p_user.last_fished_date = today_date.clone();
            p_user.username = username.clone();
            p_user.total_catches += 1;
        }

        data.users.insert(
            user_id.clone(),
            crate::data::UserData {
                username: username.clone(),
                fished_at: Utc::now().to_rfc3339(),
            },
        );
        data.daily_count += 1;

        let p_user = data
            .persistent_users
            .get(&user_id)
            .expect("Just inserted or updated");
        let result = (p_user.streak, p_user.total_catches, data.daily_count);

        drop(data);
        if let Err(e) = self.data_manager.save().await {
            return Err(FishingError::Internal(format!("Disk save failed: {}", e)));
        }

        Ok(result)
    }

    pub async fn post_daily_summary(&self, ctx: &serenity::Context) {
        self.post_daily_summary_http(&ctx.http).await;
    }

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

        let today_date = Self::get_fishing_day_string(chrono::Utc::now());

        let mut non_fishers = Vec::new();
        let mut best_anglers = Vec::new();

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
                        after = Some(members.last().unwrap().user.id);
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
                        tracing::error!("❌ Error fetching members for summary: {}", e);
                        break;
                    }
                }
            }
        }

        let data = self.data_manager.data.read().await;
        for (user_id, p_user) in &data.persistent_users {
            if p_user.streak >= best_angler_streak {
                best_anglers.push((
                    user_id.clone(),
                    p_user.username.clone(),
                    p_user.streak,
                    p_user.total_catches,
                ));
            }
        }
        drop(data);

        // Sort: Streak DESC, then Total CAT DESC
        best_anglers.sort_by(|a, b| b.2.cmp(&a.2).then(b.3.cmp(&a.3)));

        let mut embed = CreateEmbed::new()
            .title("🐠 Daily Guild Aquarium Contributions")
            .description("Here is how the pond is doing today!")
            .color(0xFFD700)
            .field(
                "🎣 Total Catches Today",
                format!("**{}**", daily_count),
                true,
            )
            .field(
                "😴 Members Missed",
                format!("**{}**", non_fishers.len()),
                true,
            )
            .footer(serenity::CreateEmbedFooter::new(
                "Stardust Pond Daily Summary",
            ))
            .timestamp(Utc::now());

        if !best_anglers.is_empty() {
            let mut anglers_text = String::new();
            for (_, username, streak, total) in best_anglers.iter().take(10) {
                anglers_text.push_str(&format!(
                    "🏆 **{}**: {} 🐟 ({} day streak)\n",
                    username, total, streak
                ));
            }
            embed = embed.field(
                format!("🔥 Best Anglers ({}+ Day Streak)", best_angler_streak),
                anglers_text,
                false,
            );
        }

        embed = embed.field("Message", "We miss you ❤️ \nPlease remember to fish daily 🙏🏻 Many lovely cats, cosmic dolphins and diamond rewards await us all 💎✨", false);

        let mut msg = CreateMessage::new().embed(embed);

        if !non_fishers.is_empty() {
            if ping_reminder_enabled {
                let mut content = String::from("**Wake up! Many of you haven't fished today!** 🎣\n");
                let mut added = 0;
                for id in &non_fishers {
                    let ping = format!("<@{}> ", id);
                    if content.len() + ping.len() > 1850 {
                        break;
                    }
                    content.push_str(&ping);
                    added += 1;
                }
                
                if added < non_fishers.len() {
                    content.push_str(&format!("...and {} others", non_fishers.len() - added));
                }
                msg = msg.content(content);
            }
        }

        if let Err(e) = channel_id.send_message(http, msg).await {
            tracing::error!("❌ Error sending summary: {}", e);
        }
    }

    pub async fn run_daily_cron_job_ctx(&self, ctx: &serenity::Context) {
        self.run_daily_cron_job(&ctx.http).await;
    }

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
        let now_millis = chrono::Utc::now().timestamp_millis() as u64;

        {
            let mut data = self.data_manager.data.write().await;
            let fished_ids: Vec<String> = data.users.keys().cloned().collect();
            for (user_id, p_user) in data.persistent_users.iter_mut() {
                if !fished_ids.contains(user_id) {
                    p_user.streak = 0;
                }
            }
            data.daily_count = 0;
            data.last_reset_timestamp = now_millis;
            data.users.clear();
        }

        let _ = self.data_manager.save().await;
        self.data_manager.backup().await;

        tracing::info!("✅ Daily cron job complete.");
    }
}
