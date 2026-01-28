pub mod api;
pub mod logic;
pub mod structs;

use anyhow::{Context, Result};
use models::FeedSkeletonResult;
use reqwest::Client;

pub use api::authenticate;

pub async fn get_feed_skeleton(
    client: &Client,
    user_jwt: &str,
    service_token: &str,
) -> Result<FeedSkeletonResult> {
    let did = api::extract_did_from_jwt(user_jwt).context("Failed to extract DID from auth")?;

    // TODOとDONEを並列で取得して、後で紐づける
    let (todos_res, dones_res) = tokio::join!(
        api::search_posts(client, "TODO", &did, service_token),
        api::search_posts(client, "DONE", &did, service_token)
    );

    let todos = todos_res.context("Failed to fetch TODOs")?;
    let dones = dones_res.context("Failed to fetch DONEs")?;

    let feed_items = logic::filter_todos(todos, dones);

    Ok(FeedSkeletonResult {
        cursor: None, // TODOフィードなので1ページ完結
        feed: feed_items,
    })
}
