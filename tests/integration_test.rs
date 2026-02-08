use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use bluesky_feeds::{
    app,
    state::{AppState, SharedState},
};
use serde_json::Value;
use sqlx::{self};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower::util::ServiceExt;

async fn create_test_state() -> SharedState {
    let db = sqlx::sqlite::SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .unwrap();

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS helloworld_posts (
            uri TEXT PRIMARY KEY,
            cid TEXT NOT NULL,
            indexed_at INTEGER NOT NULL
        );
        "#,
    )
    .execute(&db)
    .await
    .unwrap();

    AppState {
        helloworld: helloworld::State::default(),
        http_client: reqwest::Client::new(),
        service_auth: Arc::new(RwLock::new(bluesky_feeds::state::ServiceAuth {
            token: Some("mock_service_token_for_testing".to_string()),
            did: Some("did:plc:test123456789".to_string()),
        })),
        auth_handle: "test.example.com".to_string(),
        auth_password: "dummy".to_string(),
        helloworld_db: db.clone(),
        fakebluesky_db: db,
        umami: bluesky_feeds::analytics::UmamiClient::new(
            "http://localhost:3000".to_string(),
            "dummy_website_id".to_string(),
            Some("localhost".to_string()),
        ),
    }
}

/// ヘルスチェック: /health が 200 OK を返すか検証
#[tokio::test]
async fn test_health_check() {
    let state = create_test_state().await;
    let app = app(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&body_bytes[..], b"OK");
}

/// DID認証情報: /.well-known/did.json が正しい構造とIDを返すか検証
#[tokio::test]
async fn test_did_json_response() {
    let state = create_test_state().await;
    let app = app(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/.well-known/did.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_json: Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(body_json["id"], "did:web:feeds.bsky.girigiribauer.com");

    let services = body_json["service"].as_array().unwrap();
    let first_service = &services[0];
    assert_eq!(
        first_service["serviceEndpoint"],
        "https://feeds.bsky.girigiribauer.com"
    );
}

/// フィード取得(異常系): 認証ヘッダーがない場合に 401 Unauthorized を返すか検証
#[tokio::test]
async fn test_feed_skeleton_missing_auth() {
    let state = create_test_state().await;
    let app = app(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/xrpc/app.bsky.feed.getFeedSkeleton?feed=at://did:example:123/app.bsky.feed.generator/helloworld")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// フィード取得(異常系): 必須パラメータ(feed)が不足している場合に 400 Bad Request を返すか検証
#[tokio::test]
async fn test_feed_skeleton_missing_param() {
    let state = create_test_state().await;
    let app = app(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/xrpc/app.bsky.feed.getFeedSkeleton") // Missing ?feed=...
                .header("Authorization", "Bearer dummy_token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// フィード取得(異常系): 存在しないフィード名を指定した場合に 404 Not Found を返すか検証
#[tokio::test]
async fn test_feed_skeleton_unknown_feed() {
    let state = create_test_state().await;
    let app = app(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/xrpc/app.bsky.feed.getFeedSkeleton?feed=at://did:example:123/app.bsky.feed.generator/unknown_feed")
                .header("Authorization", "Bearer dummy_token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// フィード取得(正常系): helloworld フィードが正常に取得できるか検証
#[tokio::test]
async fn test_feed_skeleton_helloworld_success() {
    let state = create_test_state().await;
    let app = app(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/xrpc/app.bsky.feed.getFeedSkeleton?feed=at://did:example:123/app.bsky.feed.generator/helloworld")
                .header("Authorization", "Bearer dummy_token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_json: Value =
        serde_json::from_slice(&body_bytes).expect("Failed to parse JSON response");

    assert!(body_json["feed"].is_array());
}

/// 初期認証: create_test_state() で認証トークンが設定されているか検証
#[tokio::test]
async fn test_initial_authentication() {
    let state = create_test_state().await;
    let auth = state.service_auth.read().await;

    assert!(
        auth.token.is_some(),
        "Service auth token should be initialized"
    );
    assert!(auth.did.is_some(), "Service auth DID should be initialized");
}

/// フィード取得(異常系): OneYearAgo フィードが認証なしで 401 を返すか検証
#[tokio::test]
async fn test_feed_skeleton_oneyearago_requires_auth() {
    let state = create_test_state().await;
    let app = app(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/xrpc/app.bsky.feed.getFeedSkeleton?feed=at://did:example:123/app.bsky.feed.generator/oneyearago")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// フィード取得(異常系): TodoApp フィードが認証なしで 401 を返すか検証
#[tokio::test]
async fn test_feed_skeleton_todoapp_requires_auth() {
    let state = create_test_state().await;
    let app = app(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/xrpc/app.bsky.feed.getFeedSkeleton?feed=at://did:example:123/app.bsky.feed.generator/todoapp")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
