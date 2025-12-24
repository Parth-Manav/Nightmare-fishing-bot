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
    let data_manager_clone = data_manager.clone();
    let token_clone = token.clone();
    let http = Arc::new(serenity::Http::new(&token_clone));

    sched
        .add(
            Job::new_async("0 30 14 * * *", move |_uuid, _l| {
                let fishing_manager = fishing_manager_clone.clone();
                let data_manager = data_manager_clone.clone();
                let http = http.clone();
                Box::pin(async move {
                    // 1. Post final summary for the day
                    fishing_manager.post_daily_summary_http(&http).await;
                    // 2. Backup data before wipe
                    data_manager.backup().await;
                    // 3. Reset for next day
                    fishing_manager.reset_daily_data_http(&http).await;
                })
            })
            .unwrap(),
        )
        .await
        .unwrap();

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
