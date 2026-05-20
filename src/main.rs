mod commands;
mod config;
mod data;
mod error;
mod events;
mod game;

use config::Config;
use data::DataManager;
pub use error::*;
use game::FishingManager;
use poise::serenity_prelude as serenity;
use std::sync::Arc;
use tokio_cron_scheduler::{Job, JobScheduler};

pub struct Data {
    pub data_manager: Arc<DataManager>,
    pub fishing_manager: Arc<FishingManager>,
}

pub type Error = BotError;
pub type Context<'a> = poise::Context<'a, Data, Error>;

#[tokio::main]
async fn main() -> BotResult<()> {
    dotenvy::dotenv().ok();

    let config = Config::from_env()?;
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", &config.log_level);
    }
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(true)
        .with_thread_ids(true)
        .init();
    tracing::info!("Loaded {}", config);

    let data_manager = Arc::new(DataManager::new(&config));
    let fishing_manager = Arc::new(FishingManager::new(data_manager.clone(), &config));

    let token = config.discord_token.clone();
    let intents = serenity::GatewayIntents::GUILDS
        | serenity::GatewayIntents::GUILD_MESSAGES
        | serenity::GatewayIntents::GUILD_MEMBERS;

    // Schedule Daily Reset (defaults to 14:30 UTC / 8:00 PM IST)
    let sched = JobScheduler::new()
        .await
        .map_err(|e| BotError::State(format!("failed to create job scheduler: {e}")))?;
    let fishing_manager_clone = fishing_manager.clone();
    let token_clone = token.clone();
    let http = Arc::new(serenity::Http::new(&token_clone));
    let reset_cron = format!("0 {} {} * * *", config.reset_minute, config.reset_hour);

    let http_cron = http.clone();
    let reset_job = Job::new_async(reset_cron.as_str(), move |_uuid, _l| {
        let fishing_manager = fishing_manager_clone.clone();
        let http = http_cron.clone();
        Box::pin(async move {
            fishing_manager.run_daily_cron_job(&http).await;
        })
    })
    .map_err(|e| BotError::Config(format!("invalid reset schedule {reset_cron}: {e}")))?;

    sched
        .add(reset_job)
        .await
        .map_err(|e| BotError::State(format!("failed to schedule daily reset: {e}")))?;

    let fm_startup = fishing_manager.clone();
    let http_startup = http.clone();
    tokio::spawn(async move {
        // Give it a moment to initialize before potentially hammering the API
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        let last_reset = {
            fm_startup
                .get_data_manager()
                .data
                .read()
                .await
                .last_reset_timestamp
        };
        let now = chrono::Utc::now();
        let current_fishing_day = fm_startup.get_configured_fishing_day_string(now);
        let last_reset_date = if let Some(last_reset_time) =
            chrono::DateTime::from_timestamp_millis(last_reset as i64)
        {
            fm_startup.get_configured_fishing_day_string(last_reset_time)
        } else {
            "invalid timestamp".to_string()
        };

        if current_fishing_day != last_reset_date {
            tracing::warn!(
                "Missed daily reset detected (last reset: {last_reset_date}). Running catch-up reset now..."
            );
            fm_startup.run_daily_cron_job(&http_startup).await;
        }
    });

    sched
        .start()
        .await
        .map_err(|e| BotError::State(format!("failed to start job scheduler: {e}")))?;

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![
                commands::fishing::fish(),
                commands::fishing::summary(),
                commands::admin::fishsetup(),
                commands::admin::fishsummary(),
                commands::admin::setbestanglerstreak(),
                commands::admin::setreminderthreshold(),
                commands::admin::setrole(),
                commands::admin::setsummarychannel(),
                commands::admin::togglereminder(),
            ],
            event_handler: |ctx, event, _framework, data| {
                Box::pin(async move {
                    if let serenity::FullEvent::InteractionCreate {
                        interaction: serenity::Interaction::Component(component),
                    } = event
                    {
                        if let Err(e) = events::handle_button_interaction(
                            ctx,
                            component,
                            &data.data_manager,
                            &data.fishing_manager,
                        )
                        .await
                        {
                            tracing::error!("Error handling button interaction: {:?}", e);
                        }
                    }
                    Ok(())
                })
            },
            ..Default::default()
        })
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                Ok(Data {
                    data_manager,
                    fishing_manager,
                })
            })
        })
        .build();

    let client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await;

    client?.start().await?;

    Ok(())
}
