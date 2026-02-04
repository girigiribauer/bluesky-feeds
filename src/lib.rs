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

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::str::FromStr;

pub async fn connect_database(url: &str) -> anyhow::Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str(url)?.create_if_missing(true);

    let pool = SqlitePoolOptions::new().connect_with(options).await?;

    Ok(pool)
}
