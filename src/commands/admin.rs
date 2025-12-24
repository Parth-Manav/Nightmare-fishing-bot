use crate::{Context, Error};
use poise::serenity_prelude as serenity;

/// Set up the fishing pond (creates the fish button)
#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR")]
pub async fn fishsetup(ctx: Context<'_>) -> Result<(), Error> {
    let row = serenity::CreateActionRow::Buttons(vec![serenity::CreateButton::new("fish_button")
        .label("🎣 Fish!")
        .style(serenity::ButtonStyle::Primary)]);

    let reply = ctx
        .send(
            poise::CreateReply::default()
                .content("🎣 Welcome to Stardust Pond — click to fish!")
                .components(vec![row])
                .ephemeral(true),
        )
        .await?;

    // Save button info to data
    {
        let mut data = ctx.data().data_manager.data.write().await;
        data.button_message_id = Some(reply.message().await?.id.to_string());
        data.button_channel_id = Some(ctx.channel_id().to_string());
        data.guild_id = ctx.guild_id().map(|id| id.to_string());
    }
    ctx.data().data_manager.save().await;

    Ok(())
}

/// Set the minimum streak for the Best Anglers list
#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR")]
pub async fn setbestanglerstreak(
    ctx: Context<'_>,
    #[description = "The minimum streak required (e.g., 5)"]
    #[min = 1]
    streak: u64,
) -> Result<(), Error> {
    {
        let mut data = ctx.data().data_manager.data.write().await;
        data.best_angler_streak = streak;
    }
    ctx.data().data_manager.save().await;

    ctx.send(
        poise::CreateReply::default()
            .content(format!(
                "✅ Best Angler minimum streak set to **{}** days.",
                streak
            ))
            .ephemeral(true),
    )
    .await?;

    Ok(())
}

/// Set the number of days of inactivity before pinging a member
#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR")]
pub async fn setreminderthreshold(
    ctx: Context<'_>,
    #[description = "Number of days (e.g., 1 for daily, 3 for every 3 days)"]
    #[min = 1]
    days: u64,
) -> Result<(), Error> {
    {
        let mut data = ctx.data().data_manager.data.write().await;
        data.reminder_threshold = days;
    }
    ctx.data().data_manager.save().await;

    ctx.send(poise::CreateReply::default()
        .content(format!("✅ Inactivity threshold set to **{} days**. Members will be pinged if they haven't fished for {} days or more.", days, days))
        .ephemeral(true))
        .await?;

    Ok(())
}

/// Set the role to track for fishing statistics
#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR")]
pub async fn setrole(
    ctx: Context<'_>,
    #[description = "The role to track"] role: serenity::Role,
) -> Result<(), Error> {
    {
        let mut data = ctx.data().data_manager.data.write().await;
        data.tracked_role_id = Some(role.id.to_string());
        data.guild_id = ctx.guild_id().map(|id| id.to_string());
    }
    ctx.data().data_manager.save().await;

    ctx.send(
        poise::CreateReply::default()
            .content(format!(
                "✅ Now tracking the **{}** role for fishing statistics!",
                role.name
            ))
            .ephemeral(true),
    )
    .await?;

    Ok(())
}

/// Set the channel for daily summaries
#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR")]
pub async fn setsummarychannel(ctx: Context<'_>) -> Result<(), Error> {
    {
        let mut data = ctx.data().data_manager.data.write().await;
        data.summary_channel_id = Some(ctx.channel_id().to_string());
        data.guild_id = ctx.guild_id().map(|id| id.to_string());
    }
    ctx.data().data_manager.save().await;

    ctx.send(
        poise::CreateReply::default()
            .content("✅ Daily summaries will be posted in this channel!")
            .ephemeral(true),
    )
    .await?;

    Ok(())
}

/// Enable or disable pinging members in the daily fishing reminder
#[poise::command(slash_command, default_member_permissions = "ADMINISTRATOR")]
pub async fn togglereminder(
    ctx: Context<'_>,
    #[description = "Set to true to enable pings, false to disable (shows nicknames instead)"]
    enabled: bool,
) -> Result<(), Error> {
    {
        let mut data = ctx.data().data_manager.data.write().await;
        data.ping_reminder_enabled = enabled;
    }
    ctx.data().data_manager.save().await;

    ctx.send(
        poise::CreateReply::default()
            .content(format!(
                "✅ Daily reminder pings have been {}.",
                if enabled { "ENABLED" } else { "DISABLED" }
            ))
            .ephemeral(true),
    )
    .await?;

    Ok(())
}

/// Get a summary of who has not fished today (for the tracked role)
#[poise::command(
    slash_command,
    rename = "fishsummary",
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn fishsummary(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = match ctx.guild_id() {
        Some(id) => id,
        None => {
            ctx.send(
                poise::CreateReply::default()
                    .content("❌ This command can only be used in a server.")
                    .ephemeral(true),
            )
            .await?;
            return Ok(());
        }
    };

    let (tracked_role_id, _role_name_str, fished_ids) = {
        let data = ctx.data().data_manager.data.read().await;

        let role_id =
            match &data.tracked_role_id {
                Some(id) => match id.parse::<u64>() {
                    Ok(parsed) => serenity::RoleId::new(parsed),
                    Err(_) => {
                        drop(data);
                        ctx.send(poise::CreateReply::default()
                        .content("❌ Invalid tracked role ID. Please set it again with `/setrole`.")
                        .ephemeral(true))
                        .await?;
                        return Ok(());
                    }
                },
                None => {
                    drop(data);
                    ctx.send(
                        poise::CreateReply::default()
                            .content("❌ No role is being tracked. Use `/setrole` to set one.")
                            .ephemeral(true),
                    )
                    .await?;
                    return Ok(());
                }
            };

        let fished: Vec<String> = data.users.keys().cloned().collect();
        drop(data);
        (role_id, format!("{}", role_id), fished)
    };

    // Fetch guild via HTTP to get role info
    let guild = guild_id.to_partial_guild(&ctx.http()).await?;
    let role = match guild.roles.get(&tracked_role_id) {
        Some(r) => r,
        None => {
            ctx.send(poise::CreateReply::default()
                .content("❌ The tracked role could not be found. Please set it again with `/setrole`.")
                .ephemeral(true))
                .await?;
            return Ok(());
        }
    };

    // Fetch all guild members reliably via pagination
    let mut members = Vec::new();
    let mut after = None;
    loop {
        let page = guild_id.members(&ctx.http(), Some(1000), after).await?;
        if page.is_empty() {
            break;
        }
        after = Some(page.last().unwrap().user.id);
        members.extend(page);
    }

    let non_fishers: Vec<serenity::UserId> = members
        .iter()
        .filter(|member| member.roles.contains(&tracked_role_id))
        .filter(|member| !fished_ids.contains(&member.user.id.to_string()))
        .map(|member| member.user.id)
        .collect();

    if non_fishers.is_empty() {
        ctx.send(
            poise::CreateReply::default()
                .content(format!(
                    "🎉 All members of the **{}** role have fished today!",
                    role.name
                ))
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    }

    let mentions = non_fishers
        .iter()
        .map(|id| format!("<@{}>", id))
        .collect::<Vec<_>>()
        .join("\n");

    // Handle Discord's 2000 character limit for messages
    if mentions.len() > 1850 {
        let truncated = &mentions[..1800];
        let last_newline = truncated.rfind('\n').unwrap_or(1800);
        ctx.send(
            poise::CreateReply::default()
                .content(format!(
                    "**Members of the {} role who have not fished today:**\n{} \n*...and more (too many to display)*",
                    role.name, &truncated[..last_newline]
                ))
                .ephemeral(true),
        )
        .await?;
    } else {
        ctx.send(
            poise::CreateReply::default()
                .content(format!(
                    "**Members of the {} role who have not fished today:**\n{}",
                    role.name, mentions
                ))
                .ephemeral(true),
        )
        .await?;
    }

    Ok(())
}
