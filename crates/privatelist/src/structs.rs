use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct SearchResponse {
    pub posts: Vec<PostView>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct PostView {
    pub uri: String,
    pub cid: String,
    pub record: serde_json::Value,
    #[serde(rename = "indexedAt")]
    pub indexed_at: String,
}
