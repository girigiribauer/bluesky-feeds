pub mod api;
pub mod logic;
mod timezone;

use anyhow::Result;
use models::FeedSkeletonResult;
use reqwest::Client;
use crate::api::BlueskyFetcher;

pub async fn get_feed_skeleton(
    client: &Client,
    #[allow(unused_variables)]
    auth_header: &str,
    service_token: &str,
    actor: &str,
    limit: usize,
    cursor: Option<String>,
) -> Result<FeedSkeletonResult> {
    let fetcher = BlueskyFetcher::new(client.clone());
    let (feed_items, next_cursor) = logic::fetch_posts_from_past(
        &fetcher,
        service_token,
        auth_header,
        actor,
        limit,
        cursor,
        None
    ).await?;

    Ok(FeedSkeletonResult {
        cursor: next_cursor,
        feed: feed_items,
    })
}
