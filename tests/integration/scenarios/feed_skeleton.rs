use crate::helpers::{auth::TestAuth, client::TestClient};
use axum::http::StatusCode;

/// Helloworldフィードが正常に取得できるか（認証あり）
#[tokio::test]
async fn test_get_feed_skeleton_helloworld_success() {
    let client = TestClient::new().await;
    let auth = TestAuth::new("did:plc:alice");

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

/// OneYearAgoフィードが正常に取得できるか（JWTからDID抽出）
#[tokio::test]
async fn test_get_feed_skeleton_oneyearago_success() {
    let client = TestClient::new().await;
    let auth = TestAuth::new("did:plc:bob");

    let (status, body) = client
        .get_feed_skeleton(
            "at://did:example:123/app.bsky.feed.generator/oneyearago",
            Some(&auth.header_value()),
        )
        .await;

    if status != StatusCode::OK {
        println!("Body: {:?}", body);
    }
    assert_eq!(status, StatusCode::OK);
    assert!(body["feed"].is_array());
}

/// 認証ヘッダーがない場合に 401 Unauthorized を返すか
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

/// 存在しないフィードURIを指定した場合に 404 Not Found を返すか
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

/// FakeBlueskyフィードが正常に取得できるか（DB連携確認）
#[tokio::test]
async fn test_get_feed_skeleton_fakebluesky_success() {
    let client = TestClient::new().await;
    let auth = TestAuth::new("did:plc:dave");

    let (status, body) = client
        .get_feed_skeleton(
            "at://did:example:123/app.bsky.feed.generator/fakebluesky",
            Some(&auth.header_value()),
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body["feed"].is_array());
}

/// RealBlueskyフィードが正常に取得できるか（DB連携確認）
#[tokio::test]
async fn test_get_feed_skeleton_realbluesky_success() {
    let client = TestClient::new().await;
    let auth = TestAuth::new("did:plc:real");

    let (status, body) = client
        .get_feed_skeleton(
            "at://did:example:123/app.bsky.feed.generator/realbluesky",
            Some(&auth.header_value()),
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body["feed"].is_array());
}

/// TodoAppフィードが認証なしで 401 Unauthorized を返すか
#[tokio::test]
async fn test_get_feed_skeleton_todoapp_missing_auth() {
    let client = TestClient::new().await;

    let (status, _body) = client
        .get_feed_skeleton("at://did:example:123/app.bsky.feed.generator/todoapp", None)
        .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// 不正な形式のトークンを指定した場合に 401 Unauthorized を返すか
#[tokio::test]
async fn test_get_feed_skeleton_malformed_token() {
    let client = TestClient::new().await;

    let (status, _body) = client
        .get_feed_skeleton(
            "at://did:example:123/app.bsky.feed.generator/oneyearago",
            Some("Bearer invalid.token.structure"),
        )
        .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// OneYearAgo のクリーンアップが JST 午前4時以降に実行され、実行記録が残るか
///
/// NOTE: ハンドラー経由の `cleanup()` は `Utc::now()` に依存するため、JST 4時前の実行で
/// 条件を満たさずスキップされてしまい時刻依存のテスト失敗が発生する。
/// ここでは `cleanup_at()` に固定時刻を注入することで決定論的にテストする。
/// （ハンドラーがクリーンアップをトリガーするかの検証は、時刻注入の仕組みを整備した後に別途行う）
#[tokio::test]
async fn test_get_feed_skeleton_oneyearago_cleanup_trigger() {
    use chrono::TimeZone;
    use oneyearago::cache::CacheStore;

    let client = TestClient::new().await;
    let db = &client.state.oneyearago_db;
    let store = CacheStore::new(db.clone());

    let last_date: Option<String> =
        sqlx::query_scalar("SELECT value FROM cache WHERE key = 'internal:last_cleanup_date'")
            .fetch_optional(db)
            .await
            .unwrap();
    assert!(last_date.is_none(), "最初は記録がないはず");

    let jst_4am_utc = chrono::Utc.with_ymd_and_hms(2026, 2, 28, 19, 0, 0).unwrap();
    store.cleanup_at(jst_4am_utc).await.unwrap();

    let last_date: Option<String> =
        sqlx::query_scalar("SELECT value FROM cache WHERE key = 'internal:last_cleanup_date'")
            .fetch_optional(db)
            .await
            .unwrap();
    assert!(
        last_date.is_some(),
        "クリーンアップの実行記録が作成されるべき"
    );
}
