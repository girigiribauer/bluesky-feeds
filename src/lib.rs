pub mod analytics;
pub mod error;
pub mod handlers;
pub mod state;

use axum::{
    extract::{Query, Request},
    routing::get,
    Router,
};
use sqlx::SqlitePool;
use state::{FeedQuery, SharedState};
use tower_http::trace::TraceLayer;
use tracing::field::display;

fn request_span(request: &Request) -> tracing::Span {
    let query = Query::<FeedQuery>::try_from_uri(request.uri()).ok();
    let feed = query.as_ref().map(|q| display(q.feed_name()));
    tracing::info_span!("request", feed)
}

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
        .layer(TraceLayer::new_for_http().make_span_with(request_span))
        .with_state(state)
}

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use std::str::FromStr;

pub async fn connect_database(url: &str) -> anyhow::Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str(url)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal);

    let pool = SqlitePoolOptions::new().connect_with(options).await?;

    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 開いたデータベースが WAL 方式で、書き込みの同期設定が NORMAL になっていること
    #[tokio::test]
    async fn test_connect_database_uses_wal_and_normal_sync() {
        let path = std::env::temp_dir().join(format!(
            "bluesky-feeds-wal-{}-{:?}.db",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_file(&path);

        let pool = connect_database(&format!("sqlite:{}", path.display()))
            .await
            .unwrap();

        let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&pool)
            .await
            .unwrap();
        let synchronous: i32 = sqlx::query_scalar("PRAGMA synchronous")
            .fetch_one(&pool)
            .await
            .unwrap();

        pool.close().await;
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{}", path.display(), suffix));
        }

        assert_eq!(journal_mode, "wal");
        assert_eq!(synchronous, 1);
    }
}
