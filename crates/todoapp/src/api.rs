use crate::structs::{PostView, SearchResponse, SessionResponse};
use anyhow::{Context, Result};
use reqwest::Client;

pub async fn authenticate(
    client: &Client,
    handle: &str,
    password: &str,
) -> Result<(String, String)> {
    let url = "https://bsky.social/xrpc/com.atproto.server.createSession";
    let body = serde_json::json!({
        "identifier": handle,
        "password": password,
    });

    let res = client
        .post(url)
        .json(&body)
        .send()
        .await
        .context("Failed to send auth request")?;

    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        anyhow::bail!("Auth failed: {} - {}", status, text);
    }

    let session: SessionResponse = res.json().await.context("Failed to parse auth response")?;
    Ok((session.access_jwt, session.did))
}

pub async fn search_posts(
    client: &Client,
    q: &str,
    author_did: &str,
    service_token: &str,
) -> Result<Vec<PostView>> {
    // Authenticated API request using Service Token
    let url = "https://api.bsky.app/xrpc/app.bsky.feed.searchPosts";
    let query_param = q.to_string(); // q parameter

    let res = client
        .get(url)
        .header("Authorization", format!("Bearer {}", service_token))
        .query(&[
            ("q", query_param.as_str()),
            ("limit", "100"),
            ("author", author_did),
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
