use serde::Deserialize;
use std::sync::{Arc, RwLock};

#[derive(Debug, Deserialize)]
pub struct FeedQuery {
    pub feed: String,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

pub type SharedState = Arc<RwLock<AppState>>;

#[derive(Default)]
pub struct AppState {
    pub helloworld: helloworld::State,
    pub http_client: reqwest::Client,
    pub service_token: Option<String>,
    pub service_did: Option<String>,
    pub auth_handle: String,
    pub auth_password: String,
}
