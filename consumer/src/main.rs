use bsky_core::database::connect_database;
use sqlx::SqlitePool;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

async fn connect(env_var: &str, default_url: &str) -> anyhow::Result<SqlitePool> {
    let url = std::env::var(env_var).unwrap_or_else(|_| default_url.to_string());
    tracing::info!("Connecting to {}: {}", env_var, url);
    connect_database(&url).await
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let helloworld_db = connect("HELLOWORLD_DB_URL", "sqlite:data/helloworld.db").await?;
    let realfakebluesky_db =
        connect("REALFAKEBLUESKY_DB_URL", "sqlite:data/fakebluesky.db").await?;
    let silentviews_db = connect("SILENTVIEWS_DB_URL", "sqlite:data/silentviews.db").await?;
    let consumer_db = connect("CONSUMER_DB_URL", "sqlite:data/consumer.db").await?;
    let silentviews_models = std::env::var("SILENTVIEWS_MODELS_DIR")
        .unwrap_or_else(|_| "crates/silentviews/consumer/models".to_string());

    let apps: Vec<Arc<dyn jetstream::App>> = vec![
        Arc::new(helloworld_consumer::HelloWorld::new(helloworld_db)),
        Arc::new(realfakebluesky_consumer::RealFakeBluesky::new(
            realfakebluesky_db.clone(),
        )),
        Arc::new(silentviews_consumer::app::SilentViews::new(
            silentviews_db,
            std::path::Path::new(&silentviews_models),
            std::env::var("SILENTVIEWS_CDN_BASE").ok(),
        )?),
    ];
    for app in &apps {
        app.migrate().await?;
    }

    jetstream::migrate_cursor(&consumer_db).await?;
    jetstream::adopt_legacy_cursor(&consumer_db, &realfakebluesky_db).await?;

    jetstream::start_consumer(consumer_db, jetstream::ConsumerConfig::from_env(), apps).await;
    Ok(())
}
