use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct SearchResponse {
    pub posts: Vec<PostView>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct PostView {
    pub uri: String,
    pub record: serde_json::Value,
    #[serde(rename = "indexedAt")]
    pub indexed_at: String,
}

#[derive(Deserialize, Debug)]
pub struct Record {
    pub reply: Option<ReplyRef>,
}

#[derive(Deserialize, Debug)]
pub struct ReplyRef {
    pub parent: Link,
}

#[derive(Deserialize, Debug)]
pub struct Link {
    pub uri: String,
}

#[derive(Deserialize, Debug)]
pub struct JwtPayload {
    pub iss: String,
}

#[derive(Deserialize, Debug)]
pub struct SessionResponse {
    #[serde(rename = "accessJwt")]
    pub access_jwt: String,
}
