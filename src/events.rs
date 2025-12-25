use crate::game::{FishingError, FishingManager};
use chrono::Utc;
use poise::serenity_prelude as serenity;

/// Handle button interactions
pub async fn handle_button_interaction(
    ctx: &serenity::Context,
    interaction: &serenity::ComponentInteraction,
    data_manager: &std::sync::Arc<crate::data::DataManager>,
    fishing_manager: &std::sync::Arc<FishingManager>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if interaction.data.custom_id != "fish_button" {
        return Ok(());
    }

    let user_id = interaction.user.id.to_string();
    let username = interaction
        .member
        .as_ref()
        .and_then(|m| m.nick.as_ref())
        .unwrap_or(&interaction.user.name)
        .clone();


    // Call shared fishing logic
    let (current_streak, total_catches, daily_count, old_button_msg, old_button_channel) =
        match fishing_manager
            .handle_fishing(user_id, username.clone())
            .await
        {
            Ok((streak, catches, count)) => {
                let data = data_manager.data.read().await;
                let msg = data.button_message_id.clone();
                let ch = data.button_channel_id.clone();
                (streak, catches, count, msg, ch)
            }
            Err(FishingError::AlreadyFished) => {
                interaction
                    .create_response(
                        &ctx.http,
                        serenity::CreateInteractionResponse::Message(
                            serenity::CreateInteractionResponseMessage::new()
                                .content("❌ You've already fished today! Come back tomorrow.")
                                .ephemeral(true),
                        ),
                    )
                    .await?;
                return Ok(());
            }
            Err(e) => {
                tracing::error!("Error during button fishing: {:?}", e);
                return Err(e.into());
            }
        };

    // Create fish embed response
    let fish_embed = serenity::CreateEmbed::new()
        .color(0x0099FF)
        .title("🎣 Catch of the Day!")
        .description(format!(
            "**{}** cast their line and caught a fish! 🐟",
            username
        ))
        .thumbnail(interaction.user.face())
        .field("🔥 Streak", format!("{} Days", current_streak), true)
        .field("✨ Total Catches", format!("{}", total_catches), true)
        .field("🌍 Total Catches Today", format!("{}", daily_count), true)
        .timestamp(Utc::now())
        .footer(serenity::CreateEmbedFooter::new("Stardust Pond"));

    interaction
        .create_response(
            &ctx.http,
            serenity::CreateInteractionResponse::Message(
                serenity::CreateInteractionResponseMessage::new().embed(fish_embed),
            ),
        )
        .await?;

    // Create new button message
    let row = serenity::CreateActionRow::Buttons(vec![serenity::CreateButton::new("fish_button")
        .label("🎣 Fish!")
        .style(serenity::ButtonStyle::Primary)]);

    let new_button_msg = interaction
        .channel_id
        .send_message(
            &ctx.http,
            serenity::CreateMessage::new()
                .content("🎣 Welcome to Stardust Pond — click to fish!")
                .components(vec![row]),
        )
        .await?;

    // Update button info
    {
        let mut data = data_manager.data.write().await;
        data.button_message_id = Some(new_button_msg.id.to_string());
        data.button_channel_id = Some(interaction.channel_id.to_string());
    }
    data_manager.save().await;

    // Delete old button message
    if let (Some(old_msg_id), Some(old_ch_id)) = (old_button_msg, old_button_channel) {
        if let (Ok(msg_id), Ok(ch_id)) = (old_msg_id.parse::<u64>(), old_ch_id.parse::<u64>()) {
            let channel = serenity::ChannelId::new(ch_id);
            let message = serenity::MessageId::new(msg_id);
            let _ = channel.delete_message(&ctx.http, message).await; // Ignore errors
            tracing::info!("✅ Old button message deleted");
        }
    }

    Ok(())
}
