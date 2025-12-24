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
    ResetNeeded,
    Internal(String),
}

impl std::fmt::Display for FishingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FishingError::AlreadyFished => write!(f, "ALREADY_FISHED"),
            FishingError::ResetNeeded => write!(f, "RESET_NEEDED"),
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

    pub fn get_date_string(timestamp: u64) -> String {
        DateTime::<Utc>::from(std::time::UNIX_EPOCH + std::time::Duration::from_millis(timestamp))
            .format("%Y-%m-%d")
            .to_string()
    }

    pub fn get_yesterday_date_string() -> String {
        (Utc::now() - chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string()
    }

    pub fn should_reset(last_reset_ms: u64) -> bool {
        let now = chrono::Utc::now().timestamp_millis() as u64;
        let last_date = Self::get_date_string(last_reset_ms);
        let now_date = Self::get_date_string(now);
        last_date != now_date
    }

    pub fn get_days_difference(date1: &str, date2: &str) -> i64 {
        let d1 = chrono::NaiveDate::parse_from_str(date1, "%Y-%m-%d")
            .unwrap_or_else(|_| chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap());
        let d2 = chrono::NaiveDate::parse_from_str(date2, "%Y-%m-%d")
            .unwrap_or_else(|_| chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap());
        d2.signed_duration_since(d1).num_days()
    }

    pub async fn handle_fishing(
        &self,
        user_id: String,
        username: String,
    ) -> Result<(u64, u64, u64), FishingError> {
        let today_date = Self::get_date_string(chrono::Utc::now().timestamp_millis() as u64);
        let yesterday_date = Self::get_yesterday_date_string();

        let mut data = self.data_manager.data.write().await;

        if Self::should_reset(data.last_reset_timestamp) {
            return Err(FishingError::ResetNeeded);
        }

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
        self.data_manager.save().await;

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

        let today_date = Self::get_date_string(chrono::Utc::now().timestamp_millis() as u64);

        let mut non_fishers = Vec::new();
        let mut best_anglers = Vec::new();

        let data = self.data_manager.data.read().await;
        // Optimization: Use references where possible
        let fished_today_ids = &data.users;

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
                                if !fished_today_ids.contains_key(&u_id_str) {
                                    let days_diff = if let Some(p_user) =
                                        data.persistent_users.get(&u_id_str)
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
            let pings = non_fishers
                .iter()
                .map(|id| format!("<@{}>", id))
                .collect::<Vec<_>>()
                .join(" ");

            if ping_reminder_enabled {
                if pings.len() > 1850 {
                    // Truncate to fit in one Discord message
                    let truncated = &pings[..1800];
                    let last_space = truncated.rfind(' ').unwrap_or(1800);
                    let content = format!(
                        "**Wake up! Many of you haven't fished today!** 🎣\n{} ...and {} others",
                        &truncated[..last_space],
                        non_fishers.len() - (truncated.split(' ').count())
                    );
                    msg = msg.content(content);
                } else {
                    let content =
                        format!("**Wake up! You haven't fished in a while!** 🎣\n{}", pings);
                    msg = msg.content(content);
                }
            }
        }

        if let Err(e) = channel_id.send_message(http, msg).await {
            tracing::error!("❌ Error sending summary: {}", e);
        }
    }

    pub async fn reset_daily_data(&self, ctx: &serenity::Context) {
        self.reset_daily_data_http(&ctx.http).await;
    }

    pub async fn reset_daily_data_http(&self, _http: &serenity::Http) {
        // Attempt to "lock" using AtomicBool
        if self
            .is_resetting
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            tracing::warn!("⚠️ Reset already in progress, skipping duplicate call");
            return;
        }

        // RAII Guard using AtomicBool
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

        tracing::info!("🔄 Resetting daily data...");
        // Removed redundant post_daily_summary_http call as main.rs handles the order now.

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

        self.data_manager.save().await;
        self.data_manager.backup().await;

        tracing::info!("✅ Daily data reset complete.");
    }
}
