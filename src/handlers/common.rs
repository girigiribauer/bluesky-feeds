use serde::Serialize;

#[derive(Serialize)]
pub struct DidResponse {
    #[serde(rename = "@context")]
    pub context: Vec<String>,
    pub id: String,
    pub service: Vec<DidService>,
}

#[derive(Serialize)]
pub struct DidService {
    pub id: String,
    #[serde(rename = "type")]
    pub service_type: String,
    #[serde(rename = "serviceEndpoint")]
    pub service_endpoint: String,
}

pub async fn root() -> &'static str {
    "お試しで Bluesky のフィードを作っています https://github.com/girigiribauer/bluesky-feeds"
}
