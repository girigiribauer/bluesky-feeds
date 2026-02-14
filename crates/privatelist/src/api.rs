use crate::structs::{PostView, SearchResponse};
use anyhow::{Context, Result};
use reqwest::Client;

pub async fn search_posts(
    client: &Client,
    base_url: &str,
    q: &str,
    service_token: &str,
) -> Result<Vec<PostView>> {
    // Authenticated API request using Service Token
    let url = format!("{}/xrpc/app.bsky.feed.searchPosts", base_url);
    let query_param = q.to_string(); // q parameter

    let res = client
        .get(url)
        .header("Authorization", format!("Bearer {}", service_token))
        .query(&[
            ("q", query_param.as_str()),
            ("limit", "100"),
            ("sort", "latest"),
        ])
        .send()
        .await
        .context("Failed to send search request")?;

    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        anyhow::bail!("Search API failed: {} - {}", status, text);
    }

    let search_res: SearchResponse = res
        .json()
        .await
        .context("Failed to parse search response")?;
    Ok(search_res.posts)
}
