use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use bluesky_feeds::{
    app,
    state::{AppState, SharedState},
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower::ServiceExt; // for oneshot
                       // use tower::Service; // removed unused import

pub struct TestClient {
    pub router: Router,
    pub state: SharedState,
}

impl TestClient {
    pub async fn new() -> Self {
        Self::new_with_bsky_url(None).await
    }

    pub async fn new_with_bsky_url(bsky_api_url: Option<String>) -> Self {
        let state = create_test_state(bsky_api_url).await;
        let router = app(state.clone());
        Self { router, state }
    }

    pub async fn get_feed_skeleton(
        &self,
        feed_uri: &str,
        auth_header: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        let mut req_builder = Request::builder()
            .uri(format!(
                "/xrpc/app.bsky.feed.getFeedSkeleton?feed={}",
                feed_uri
            ))
            .header("Host", "feeds.localhost")
            .method("GET");

        if let Some(token) = auth_header {
            req_builder = req_builder.header("Authorization", token);
        }

        let request = req_builder.body(Body::empty()).unwrap();

        // Router implements Service<Request, Response=Response, Error=Infallible>
        // We need to clone it because oneshot consumes self, or we use a fresh router for each test if cheap.
        // Actually Router is cheap to clone.
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("Request failed");

        let status = response.status();
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        let body_json: serde_json::Value = if body_bytes.is_empty() {
            serde_json::json!(null)
        } else {
            serde_json::from_slice(&body_bytes).unwrap_or_else(
                |_| serde_json::json!({ "raw": String::from_utf8_lossy(&body_bytes) }),
            )
        };

        (status, body_json)
    }

    pub async fn get_health(&self) -> (StatusCode, String) {
        let request = Request::builder()
            .uri("/health")
            .method("GET")
            .header("Host", "feeds.localhost")
            .body(Body::empty())
            .unwrap();

        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("Request failed");
        let status = response.status();
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        (status, String::from_utf8_lossy(&body_bytes).to_string())
    }

    pub async fn get_did_json(&self) -> (StatusCode, serde_json::Value) {
        let request = Request::builder()
            .uri("/.well-known/did.json")
            .method("GET")
            .header("Host", "feeds.localhost")
            .body(Body::empty())
            .unwrap();

        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("Request failed");
        let status = response.status();
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        let body_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        (status, body_json)
    }
    pub async fn privatelist_add(&self, target_did: &str, auth_header: Option<&str>) -> StatusCode {
        let payload = serde_json::json!({ "target": target_did });
        let request = Request::builder()
            .uri("/privatelist/add")
            .method("POST")
            .header("Host", "privatelist.localhost")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&payload).unwrap()))
            .unwrap();

        // Add auth if provided
        let mut request = request;
        if let Some(token) = auth_header {
            request
                .headers_mut()
                .insert("Authorization", token.parse().unwrap());
        }

        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("Request failed");

        response.status()
    }

    pub async fn privatelist_refresh(&self, auth_header: Option<&str>) -> StatusCode {
        let request = Request::builder()
            .uri("/privatelist/refresh")
            .method("POST")
            .header("Host", "privatelist.localhost")
            .body(Body::empty())
            .unwrap();

        // Add auth if provided
        let mut request = request;
        if let Some(token) = auth_header {
            request
                .headers_mut()
                .insert("Authorization", token.parse().unwrap());
        }

        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("Request failed");

        response.status()
    }
}

async fn create_test_state(bsky_api_url: Option<String>) -> SharedState {
    let db = sqlx::sqlite::SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .unwrap();

    // Run migrations or schema creation
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS helloworld_posts (
            uri TEXT PRIMARY KEY,
            cid TEXT NOT NULL,
            indexed_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS fake_bluesky_posts (
            uri TEXT PRIMARY KEY,
            cid TEXT NOT NULL,
            indexed_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS private_list_members (
            user_did TEXT NOT NULL,
            target_did TEXT NOT NULL,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (user_did, target_did)
        );
        CREATE INDEX IF NOT EXISTS idx_private_list_members_user ON private_list_members(user_did);

        CREATE TABLE IF NOT EXISTS private_list_post_cache (
            uri TEXT PRIMARY KEY,
            cid TEXT NOT NULL,
            author_did TEXT NOT NULL,
            indexed_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_private_list_post_cache_author ON private_list_post_cache(author_did);
        CREATE INDEX IF NOT EXISTS idx_private_list_post_cache_indexed_at ON private_list_post_cache(indexed_at DESC);

        CREATE TABLE IF NOT EXISTS cache (
            key        TEXT    PRIMARY KEY,
            value      TEXT    NOT NULL,
            expires_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_cache_expires_at ON cache(expires_at);
        "#,
    )
    .execute(&db)
    .await
    .unwrap();

    AppState {
        config: bluesky_feeds::state::AppConfig {
            privatelist_url: "http://localhost:3000".to_string(),
            bsky_api_url: bsky_api_url.unwrap_or_else(|| "https://api.bsky.app".to_string()),
            client_id: "http://localhost:3000/client-metadata.json".to_string(),
            redirect_uri: "http://localhost:3000/oauth/callback".to_string(),
        },
        helloworld: helloworld::State::default(),
        http_client: reqwest::Client::new(),
        service_auth: Arc::new(RwLock::new(bluesky_feeds::state::ServiceAuth {
            token: Some("mock_service_token_for_testing".to_string()),
            did: Some("did:plc:test123456789".to_string()),
        })),
        auth_handle: "test.example.com".to_string(),
        auth_password: "dummy".to_string(),
        helloworld_db: db.clone(),
        fakebluesky_db: db.clone(),
        privatelist_db: db.clone(),
        oneyearago_db: db,
        umami: bluesky_feeds::analytics::UmamiClient::new(
            "http://localhost:3000".to_string(),
            "dummy_website_id".to_string(),
            Some("localhost".to_string()),
        ),
        key: axum_extra::extract::cookie::Key::generate(),
    }
}
