pub mod api;
pub mod logic;
mod timezone;

use anyhow::Result;
use models::FeedSkeletonResult;
use reqwest::Client;
use crate::api::BlueskyFetcher;

pub async fn get_feed_skeleton(
    client: &Client,
    auth_header: &str,
    service_token: &str,
    actor: &str,
) -> Result<FeedSkeletonResult> {
    let fetcher = BlueskyFetcher::new(client.clone());
    let feed_items = logic::fetch_posts_from_past(&fetcher, service_token, auth_header, actor, None).await?;

    Ok(FeedSkeletonResult {
        cursor: None,
        feed: feed_items,
    })
}
