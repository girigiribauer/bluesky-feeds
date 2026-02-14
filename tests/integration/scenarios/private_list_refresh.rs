use crate::helpers::auth::TestAuth;
use crate::helpers::client::TestClient;
use crate::helpers::mock_server::MockServer;
use axum::http::StatusCode;
use bsky_core::FeedSkeletonResult;

/// 観点: Private List のリフレッシュフローが正常に動作するか
/// 1. ユーザーをリストに追加
/// 2. リフレッシュを実行（MockServerからデータ取得）
/// 3. フィードを取得し、MockServerのデータが含まれているか確認
#[tokio::test]
async fn test_private_list_refresh_flow() {
    // 1. Start mock server
    let mock_server = MockServer::start().await;
    let mock_url = mock_server.base_url();

    // 2. Initialize client with mock URL
    let client = TestClient::new_with_bsky_url(Some(mock_url)).await;

    // Auth token
    let did = "did:plc:test_user";
    let auth = TestAuth::new(did);
    let token = auth.header_value();

    // 3. Add user to list
    let target_did = "did:plc:target_user";
    let status = client.privatelist_add(target_did, Some(&token)).await;
    assert_eq!(status, StatusCode::OK);

    // 4. Call refresh
    // Mock server returns posts for any "from:DID" query.
    let status = client.privatelist_refresh(Some(&token)).await;
    assert_eq!(status, StatusCode::OK);

    // 5. Get Feed Skeleton
    // It should contain the post from mock server.
    // Mock server returns post with uri "at://did:plc:target_user/app.bsky.feed.post/1"
    let (status, body) = client.get_feed_skeleton("privatelist", Some(&token)).await;
    assert_eq!(status, StatusCode::OK);

    let feed_res: FeedSkeletonResult = serde_json::from_value(body).unwrap();
    assert!(!feed_res.feed.is_empty());
    assert_eq!(
        feed_res.feed[0].post,
        format!("at://{}/app.bsky.feed.post/1", target_did)
    );
}
