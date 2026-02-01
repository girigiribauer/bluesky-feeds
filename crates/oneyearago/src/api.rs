use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use crate::timezone;

#[derive(Debug, Deserialize, Clone)]
pub struct SearchResponse {
    pub posts: Vec<PostView>,
    pub cursor: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PostView {
    pub uri: String,
    #[allow(dead_code)]
    pub record: PostRecord,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PostRecord {
    #[allow(dead_code)]
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

#[async_trait::async_trait]
pub trait PostFetcher {
    async fn search_posts(
        &self,
        token: &str,
        author: &str,
        since: &str,
        until: &str,
        limit: usize,
        cursor: Option<String>,
    ) -> Result<(Vec<PostView>, Option<String>)>;

    async fn determine_timezone(&self, handle: &str, user_token: &str) -> Result<chrono::FixedOffset>;
}

pub struct BlueskyFetcher {
    client: Client,
}

impl BlueskyFetcher {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

#[async_trait::async_trait]
impl PostFetcher for BlueskyFetcher {
    async fn search_posts(
        &self,
        token: &str,
        author: &str,
        since: &str,
        until: &str,
        limit: usize,
        cursor: Option<String>,
    ) -> Result<(Vec<PostView>, Option<String>)> {
        let url = "https://api.bsky.app/xrpc/app.bsky.feed.searchPosts";
        let q = format!("from:{} since:{} until:{}", author, since, until);

        let mut req = self.client
            .get(url)
            .header("Authorization", format!("Bearer {}", token))
            .query(&[
                ("q", q.as_str()),
                ("limit", &limit.to_string()),
                ("author", author),
                ("sort", "latest"),
            ]);

        if let Some(c) = cursor {
            req = req.query(&[("cursor", c)]);
        }

        let res = req.send().await.context("Search request failed")?;

        if !res.status().is_success() {
            return Ok((vec![], None));
        }

        let search_res: SearchResponse = res.json().await.context("Failed to parse search response")?;
        Ok((search_res.posts, search_res.cursor))
    }

    async fn determine_timezone(&self, handle: &str, user_token: &str) -> Result<chrono::FixedOffset> {
        timezone::determine_timezone(&self.client, handle, user_token).await
    }
}

