use crate::helpers::{auth::TestAuth, client::TestClient};
use axum::http::StatusCode;

/// 観点: Helloworldフィードが正常に取得できるか（認証あり）
#[tokio::test]
async fn test_get_feed_skeleton_helloworld_success() {
    let client = TestClient::new().await;
    let auth = TestAuth::new("did:plc:alice");

    // helloworld currently allows any token (or even just presence of header? No, it checks header presence).
    // Let's use our "valid format" token.
    let (status, body) = client
        .get_feed_skeleton(
            "at://did:example:123/app.bsky.feed.generator/helloworld",
            Some(&auth.header_value()),
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body["feed"].is_array());
    assert!(body["cursor"].is_null() || body["cursor"].is_string());
}

/// 観点: OneYearAgoフィードが正常に取得できるか（JWTからDID抽出）
#[tokio::test]
async fn test_get_feed_skeleton_oneyearago_success() {
    let client = TestClient::new().await;
    let auth = TestAuth::new("did:plc:bob");

    // oneyearago parsers the JWT to extract DID.
    // Our TestAuth generates a JWT with "iss": "did:plc:bob".
    let (status, body) = client
        .get_feed_skeleton(
            "at://did:example:123/app.bsky.feed.generator/oneyearago",
            Some(&auth.header_value()),
        )
        .await;

    // If verification was strict, this would fail. But since it only extracts, it should succeed.
    // Wait, oneyearago implementation requires:
    // 1. Valid JWT structure (Check)
    // 2. Extracts DID (Check)
    // 3. Service auth token present (Check, handled by TestClient's mocked state)
    // 4. Calls oneyearago::get_feed_skeleton... which might fail if logic inside oneyearago crate expects real data?
    // Let's assume it works or returns empty feed.

    if status != StatusCode::OK {
        println!("Body: {:?}", body);
    }
    assert_eq!(status, StatusCode::OK);
    assert!(body["feed"].is_array());
}

/// 観点: 認証ヘッダーがない場合に 401 Unauthorized を返すか
#[tokio::test]
async fn test_get_feed_skeleton_missing_auth() {
    let client = TestClient::new().await;

    let (status, _body) = client
        .get_feed_skeleton(
            "at://did:example:123/app.bsky.feed.generator/helloworld",
            None,
        )
        .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// 観点: 存在しないフィードURIを指定した場合に 404 Not Found を返すか
#[tokio::test]
async fn test_get_feed_skeleton_invalid_feed() {
    let client = TestClient::new().await;
    let auth = TestAuth::new("did:plc:charlie");

    let (status, _body) = client
        .get_feed_skeleton(
            "at://did:example:123/app.bsky.feed.generator/unknown_feed",
            Some(&auth.header_value()),
        )
        .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// 観点: FakeBlueskyフィードが正常に取得できるか（DB連携確認）
#[tokio::test]
async fn test_get_feed_skeleton_fakebluesky_success() {
    let client = TestClient::new().await;
    let auth = TestAuth::new("did:plc:dave");

    // fakebluesky uses DB, which is empty but valid.
    let (status, body) = client
        .get_feed_skeleton(
            "at://did:example:123/app.bsky.feed.generator/fakebluesky",
            Some(&auth.header_value()),
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body["feed"].is_array());
}

/// 観点: TodoAppフィードが認証なしで 401 Unauthorized を返すか
#[tokio::test]
async fn test_get_feed_skeleton_todoapp_missing_auth() {
    let client = TestClient::new().await;

    let (status, _body) = client
        .get_feed_skeleton("at://did:example:123/app.bsky.feed.generator/todoapp", None)
        .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// 観点: 不正な形式のトークンを指定した場合に 401 Unauthorized を返すか
#[tokio::test]
async fn test_get_feed_skeleton_malformed_token() {
    let client = TestClient::new().await;

    let (status, _body) = client
        .get_feed_skeleton(
            "at://did:example:123/app.bsky.feed.generator/oneyearago",
            Some("Bearer invalid.token.structure"),
        )
        .await;

    // oneyearago expects valid JWT structure
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
