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

pub type SharedState = AppState;

#[derive(Clone)]
pub struct AppState {
    pub helloworld: helloworld::State,
    pub http_client: reqwest::Client,
    pub service_auth: Arc<RwLock<ServiceAuth>>,
    pub auth_handle: String,
    pub auth_password: String,
    pub helloworld_db: SqlitePool,
    pub fakebluesky_db: SqlitePool,
    pub umami: crate::analytics::UmamiClient,
}

#[derive(Clone, Debug)]
pub struct ServiceAuth {
    pub token: Option<String>,
    pub did: Option<String>,
}
