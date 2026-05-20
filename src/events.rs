use crate::game::FishingManager;
use crate::{BotError, BotResult};
use chrono::Utc;
use poise::serenity_prelude as serenity;

/// Handle button interactions
pub async fn handle_button_interaction(
    ctx: &serenity::Context,
    interaction: &serenity::ComponentInteraction,
    _data_manager: &std::sync::Arc<crate::data::DataManager>,
    fishing_manager: &std::sync::Arc<FishingManager>,
) -> BotResult<()> {
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
    let (current_streak, total_catches, daily_count) = match fishing_manager
        .handle_fishing(
            user_id,
            username.clone(),
            interaction.guild_id.map(|id| id.to_string()),
        )
        .await
    {
        Ok((streak, catches, count)) => (streak, catches, count),
        Err(BotError::State(message)) if message == "ALREADY_FISHED" => {
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
        Err(BotError::State(message)) => {
            interaction
                .create_response(
                    &ctx.http,
                    serenity::CreateInteractionResponse::Message(
                        serenity::CreateInteractionResponseMessage::new()
                            .content(format!("❌ {message}"))
                            .ephemeral(true),
                    ),
                )
                .await?;
            return Ok(());
        }
        Err(e) => return Err(e),
    };

    // Create fish embed response
    let fish_embed = serenity::CreateEmbed::new()
        .color(0x0099FF)
        .title("🎣 Catch of the Day!")
        .description(format!(
            "**{username}** cast their line and caught a fish! 🐟"
        ))
        .thumbnail(interaction.user.face())
        .field("🔥 Streak", format!("{current_streak} Days"), true)
        .field("✨ Total Catches", total_catches.to_string(), true)
        .field("🌍 Total Catches Today", daily_count.to_string(), true)
        .timestamp(Utc::now())
        .footer(serenity::CreateEmbedFooter::new("Stardust Pond"));

    // 1. Acknowledge the interaction so the button stops loading (no reply arrow!)
    let _ = interaction
        .create_response(&ctx.http, serenity::CreateInteractionResponse::Acknowledge)
        .await;

    // 2. Send the user's catch information embed as a normal message
    interaction
        .channel_id
        .send_message(&ctx.http, serenity::CreateMessage::new().embed(fish_embed))
        .await?;

    // 3. Recreate and send the new "Welcome to Stardust Pond" button prompt
    let row = serenity::CreateActionRow::Buttons(vec![serenity::CreateButton::new("fish_button")
        .label("🎣 Fish!")
        .style(serenity::ButtonStyle::Primary)]);

    interaction
        .channel_id
        .send_message(
            &ctx.http,
            serenity::CreateMessage::new()
                .content("🎣 Welcome to Stardust Pond — click to fish!")
                .components(vec![row]),
        )
        .await?;

    // 4. Delete the old button message to effectively "jump" it to the bottom
    let _ = interaction.message.delete(&ctx.http).await;

    Ok(())
}
