pub mod api;
pub mod db;
pub mod structs;

use anyhow::{Context, Result};
use bsky_core::{FeedItem, FeedSkeletonResult};
use chrono::DateTime;
use reqwest::Client;
use sqlx::SqlitePool;

pub use db::{add_user, list_users, migrate, remove_user};

pub async fn refresh_list(
    pool: &SqlitePool,
    client: &Client,
    base_url: &str,
    user_did: &str,
    service_token: &str,
) -> Result<()> {
    // 1. Get target DIDs from DB
    let targets = list_users(pool, user_did).await?;

    for target_did in targets {
        // 2. Search posts for each target
        // We use "from:DID" query to get posts from specific user.
        // This is much more reliable than "OR" query in search API.
        let query = format!("from:{}", target_did);
        let posts = api::search_posts(client, base_url, &query, service_token)
            .await
            .context(format!("Failed to search posts for {}", target_did))?;

        // 3. Cache posts
        for post in posts {
            // Parse timestamp
            // indexedAt from search API is ISO 8601 string
            let indexed_at = DateTime::parse_from_rfc3339(&post.indexed_at)
                .context("Failed to parse indexed_at")?
                .timestamp_micros();

            db::cache_post(pool, &post.uri, &post.cid, &target_did, indexed_at).await?;
        }
    }

    Ok(())
}

pub async fn get_feed_skeleton(
    pool: &SqlitePool,
    _client: &Client, // Client is not used for reading from DB
    user_did: &str,
    _service_token: &str, // Token is not used for reading from DB
    cursor: Option<String>,
    limit: usize,
) -> Result<FeedSkeletonResult> {
    // 1. Get target DIDs from DB
    let targets = list_users(pool, user_did).await?;

    // 2. Empty State
    if targets.is_empty() {
        // Return a pinned post explaining how to use the feed
        return Ok(FeedSkeletonResult {
            cursor: None,
            feed: vec![FeedItem {
                // "How to use Private List Feed" post (placeholder)
                post: "at://did:plc:tsvcmd72oxp47wtixs4qllyi/app.bsky.feed.post/3letuz6sqa22o"
                    .to_string(),
            }],
        });
    }

    // 3. Fetch from DB Cache
    let cursor_val = cursor.as_ref().and_then(|c| c.parse::<i64>().ok());
    let posts = db::get_cached_posts(pool, &targets, limit, cursor_val).await?;

    let mut feed = Vec::new();
    let mut next_cursor = None;

    if let Some(last) = posts.last() {
        next_cursor = Some(last.indexed_at.to_string());
    }

    for post in posts {
        feed.push(FeedItem { post: post.uri });
    }

    Ok(FeedSkeletonResult {
        cursor: next_cursor,
        feed,
    })
}
