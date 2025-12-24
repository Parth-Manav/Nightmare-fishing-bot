use crate::game::{FishingError, FishingManager};
use crate::{Context, Error};
use chrono::Utc;
use poise::serenity_prelude as serenity;

/// Cast your line and catch a fish!
#[poise::command(slash_command)]
pub async fn fish(ctx: Context<'_>) -> Result<(), Error> {
    let user_id = ctx.author().id.to_string();

    // Get display name (nickname) if available, otherwise username
    let username = ctx
        .author_member()
        .await
        .and_then(|m| m.nick.clone())
        .unwrap_or_else(|| ctx.author().name.clone());

    // Check if reset is needed
    {
        let data = ctx.data().data_manager.data.read().await;
        if FishingManager::should_reset(data.last_reset_timestamp) {
            drop(data); // Release read lock before reset
            ctx.data()
                .fishing_manager
                .reset_daily_data(ctx.serenity_context())
                .await;
        }
    }

    // Call shared fishing logic
    match ctx
        .data()
        .fishing_manager
        .handle_fishing(user_id, username.clone())
        .await
    {
        Ok((current_streak, total_catches, daily_count)) => {
            // Create and send embed
            let embed = serenity::CreateEmbed::new()
                .color(0x0099FF)
                .title("🎣 Catch of the Day!")
                .description(format!(
                    "**{}** cast their line and caught a fish! 🐟",
                    username
                ))
                .thumbnail(ctx.author().face())
                .field("🔥 Streak", format!("{} Days", current_streak), true)
                .field("✨ Total Catches", format!("{}", total_catches), true)
                .field("🌍 Total Catches Today", format!("{}", daily_count), true)
                .timestamp(Utc::now())
                .footer(serenity::CreateEmbedFooter::new("Stardust Pond"));

            ctx.send(poise::CreateReply::default().embed(embed)).await?;
        }
        Err(FishingError::AlreadyFished) => {
            ctx.send(
                poise::CreateReply::default()
                    .content("❌ You've already fished today! Come back tomorrow.")
                    .ephemeral(true),
            )
            .await?;
        }
        Err(FishingError::ResetNeeded) => {
            // This should rarely happen as we check before calling, but for safety:
            ctx.data()
                .fishing_manager
                .reset_daily_data(ctx.serenity_context())
                .await;
            // Retry once? Or just tell user to try again
            ctx.send(
                poise::CreateReply::default()
                    .content("🔄 Day has just reset! Please try fishing again now.")
                    .ephemeral(true),
            )
            .await?;
        }
        Err(e) => return Err(e.into()),
    }

    Ok(())
}

/// Show the daily summary
#[poise::command(slash_command)]
pub async fn summary(ctx: Context<'_>) -> Result<(), Error> {
    ctx.data()
        .fishing_manager
        .post_daily_summary(ctx.serenity_context())
        .await;
    ctx.send(
        poise::CreateReply::default()
            .content("✅ Summary posted (check the configured channel if set)")
            .ephemeral(true),
    )
    .await?;
    Ok(())
}
