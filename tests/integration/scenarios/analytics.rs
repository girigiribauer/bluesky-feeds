use crate::helpers::{auth::TestAuth, client::TestClient};
use std::time::Duration;

/// 実在するフィードへの問い合わせは、Umami に /feeds/フィード名 として 1 件記録すること
#[tokio::test]
async fn test_existing_feed_request_is_sent_to_umami() {
    let client = TestClient::new().await;
    let auth = TestAuth::new("did:plc:alice");

    client
        .get_feed_skeleton(
            "at://did:example:123/app.bsky.feed.generator/helloworld",
            Some(&auth.header_value()),
        )
        .await;

    assert_eq!(
        client.umami.wait_for_urls(1).await,
        vec!["/feeds/helloworld?did=did:plc:alice".to_string()]
    );
}

/// 実在しないフィード名への問い合わせは、Umami に記録しないこと
#[tokio::test]
async fn test_unknown_feed_request_is_not_sent_to_umami() {
    let client = TestClient::new().await;
    let auth = TestAuth::new("did:plc:alice");

    client
        .get_feed_skeleton(
            "at://did:example:123/app.bsky.feed.generator/pdswatch-probe",
            Some(&auth.header_value()),
        )
        .await;
    client
        .get_feed_skeleton(
            "at://did:example:123/app.bsky.feed.generator/helloworld",
            Some(&auth.header_value()),
        )
        .await;
    client.umami.wait_for_urls(1).await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert_eq!(
        client.umami.urls(),
        vec!["/feeds/helloworld?did=did:plc:alice".to_string()]
    );
}
