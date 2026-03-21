mod commands;
mod data;
mod events;
mod game;

use data::DataManager;
use game::FishingManager;
use poise::serenity_prelude as serenity;
use std::sync::Arc;
use tokio_cron_scheduler::{Job, JobScheduler};

pub struct Data {
    pub data_manager: Arc<DataManager>,
    pub fishing_manager: Arc<FishingManager>,
}

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    let data_manager = Arc::new(DataManager::new());
    let fishing_manager = Arc::new(FishingManager::new(data_manager.clone()));

    let token = std::env::var("DISCORD_BOT_TOKEN").expect("missing DISCORD_BOT_TOKEN");
    let intents = serenity::GatewayIntents::GUILDS
        | serenity::GatewayIntents::GUILD_MESSAGES
        | serenity::GatewayIntents::GUILD_MEMBERS;

    // Schedule Daily Reset (runs at 14:30 GMT / 8:00 PM IST)
    let sched = JobScheduler::new().await.unwrap();
    let fishing_manager_clone = fishing_manager.clone();
    let token_clone = token.clone();
    let http = Arc::new(serenity::Http::new(&token_clone));

    let http_cron = http.clone();
    sched
        .add(
            Job::new_async("0 30 14 * * *", move |_uuid, _l| {
                let fishing_manager = fishing_manager_clone.clone();
                let http = http_cron.clone();
                Box::pin(async move {
                    fishing_manager.run_daily_cron_job(&http).await;
                })
            })
            .unwrap(),
        )
        .await
        .unwrap();

    let fm_startup = fishing_manager.clone();
    let http_startup = http.clone();
    tokio::spawn(async move {
        // Give it a moment to initialize before potentially hammering the API
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        let last_reset = { fm_startup.get_data_manager().data.read().await.last_reset_timestamp };
        let now = chrono::Utc::now();
        let current_fishing_day = FishingManager::get_fishing_day_string(now);
        let last_reset_date = FishingManager::get_fishing_day_string(
            chrono::DateTime::from_timestamp_millis(last_reset as i64)
                .unwrap_or_default()
        );
        
        if current_fishing_day != last_reset_date {
            tracing::warn!("Missed daily reset detected (last reset: {}). Running catch-up reset now...", last_reset_date);
            fm_startup.run_daily_cron_job(&http_startup).await;
        }
    });

    sched.start().await.unwrap();

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

    client.unwrap().start().await.unwrap();
}
