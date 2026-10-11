use serde::Deserialize;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Deserialize)]
pub struct FeedQuery {
    pub feed: String,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

impl FeedQuery {
    pub fn feed_name(&self) -> &str {
        self.feed.rsplit('/').next().unwrap_or(&self.feed)
    }
}

pub type SharedState = AppState;

#[derive(Clone)]
pub struct AppState {
    pub http_client: reqwest::Client,
    pub service_auth: Arc<RwLock<ServiceAuth>>,
    pub auth_handle: String,
    pub auth_password: String,
    pub helloworld_db: SqlitePool,
    pub realfakebluesky_db: SqlitePool,
    pub oneyearago_db: SqlitePool,
    pub umami: crate::analytics::UmamiClient,
}

#[derive(Clone, Debug)]
pub struct ServiceAuth {
    pub token: Option<String>,
    pub did: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(feed: &str) -> FeedQuery {
        FeedQuery {
            feed: feed.to_string(),
            cursor: None,
            limit: None,
        }
    }

    /// フィードの AT URI の、最後の / より後ろをフィード名として取り出すこと
    #[test]
    fn test_feed_name_is_last_segment_of_feed_uri() {
        assert_eq!(
            query("at://did:plc:abc/app.bsky.feed.generator/helloworld").feed_name(),
            "helloworld"
        );
    }

    /// / を含まない値は、そのままフィード名として扱うこと
    #[test]
    fn test_feed_name_without_separator_is_whole_value() {
        assert_eq!(query("helloworld").feed_name(), "helloworld");
    }
}
