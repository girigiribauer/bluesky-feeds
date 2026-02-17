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
pub struct AppConfig {
    pub privatelist_url: String,
    pub bsky_api_url: String,
    pub client_id: String,
    pub redirect_uri: String,
}

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub helloworld: helloworld::State,
    pub http_client: reqwest::Client,
    pub service_auth: Arc<RwLock<ServiceAuth>>,
    pub auth_handle: String,
    pub auth_password: String,
    pub helloworld_db: SqlitePool,
    pub fakebluesky_db: SqlitePool,
    pub privatelist_db: SqlitePool,
    pub umami: crate::analytics::UmamiClient,
    pub key: axum_extra::extract::cookie::Key,
}

impl axum::extract::FromRef<AppState> for axum_extra::extract::cookie::Key {
    fn from_ref(state: &AppState) -> Self {
        state.key.clone()
    }
}

#[derive(Clone, Debug)]
pub struct ServiceAuth {
    pub token: Option<String>,
    pub did: Option<String>,
}
