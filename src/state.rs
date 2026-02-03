use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;
use sqlx::SqlitePool;

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
}

#[derive(Clone, Debug)]
pub struct ServiceAuth {
    pub token: Option<String>,
    pub did: Option<String>,
}
