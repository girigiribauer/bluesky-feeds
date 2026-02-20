pub mod api;
pub mod cache;
pub mod logic;
mod timezone;

use crate::api::BlueskyFetcher;
use crate::cache::CacheStore;
use anyhow::Result;
use bsky_core::FeedSkeletonResult;
use reqwest::Client;

pub async fn get_feed_skeleton(
    client: &Client,
    #[allow(unused_variables)] auth_header: &str,
    service_token: &str,
    actor: &str,
    limit: usize,
    cursor: Option<String>,
    cache: Option<&CacheStore>,
) -> Result<FeedSkeletonResult> {
    let fetcher = BlueskyFetcher::new(client.clone());
    let (feed_items, next_cursor) = logic::fetch_posts_from_past(
        &fetcher,
        service_token,
        auth_header,
        actor,
        limit,
        cursor,
        None,
        cache,
    )
    .await?;

    Ok(FeedSkeletonResult {
        cursor: next_cursor,
        feed: feed_items,
    })
}
