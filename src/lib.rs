pub mod handlers;
pub mod state;

use axum::{routing::get, Router};
use sqlx::SqlitePool;
use state::SharedState;
use tower_http::trace::TraceLayer;

pub fn app(state: SharedState) -> Router {
    Router::new()
        .route("/", get(handlers::root))
        .route("/health", get(handlers::health))
        .route(
            "/xrpc/app.bsky.feed.getFeedSkeleton",
            get(handlers::get_feed_skeleton),
        )
        .route(
            "/xrpc/app.bsky.feed.describeFeedGenerator",
            get(handlers::describe_feed_generator),
        )
        .route("/.well-known/did.json", get(handlers::get_did_json))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

pub async fn connect_database(url: &str) -> anyhow::Result<SqlitePool> {
    // Ensure the database file exists if it's a file path
    if url.starts_with("sqlite:") && !url.contains(":memory:") {
        let path = url.trim_start_matches("sqlite:");
        if !std::path::Path::new(path).exists() {
            std::fs::File::create(path)?;
        }
    }

    let pool = sqlx::sqlite::SqlitePoolOptions::new().connect(url).await?;

    Ok(pool)
}
